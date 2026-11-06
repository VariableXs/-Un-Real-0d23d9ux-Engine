//! UNX-F11310 TCB 布局与 dtv 指针位（0xe950 段）
//!
//! 线程控制块（TCB）是 TP 指针正下方的固定头部，六个字段按 ABI 顺序紧凑
//! 排列：`tcb`（自指针）、`dtv`（动态线程向量）、`self`（pthread 描述符）、
//! `multithread`（多线程标志）、`sysinfo`（栈信息）、`stack_guard`（栈哨兵）。
//! 字段次序不是自由选择——`dtv` 必须是 `tcb + 8`，因为 `__tls_get_addr`
//! 用 `TP + 8` 一条寻址就能取到 DTV，省下寄存器；`self` 又必须等于 TP，
//! 因为 `pthread_self()` 直接读 `TP + 16`。
//!
//! 模型把布局与读写拆开：`Layout` 只声明偏移/宽度，读写函数只认声明。所有
//! 写操作过四道闸门——字段号、区间、宽度、对齐；校验函数另走关系闸门——
//! `tcb` 与 `self` 必须指回 TP，`dtv` 必须非零、八字节对齐且恰为 `TP + 8`，
//! 标志位只能取 0/1，哨兵不得为零。闸门顺序固定，错误码因此有优先级：
//! 未知字段号优先于一切，区间越界先于宽度不符。
//!
//! 账本十四格：前六格依次写满六个字段，第十二格校验通过，第十三格回读
//! 哨兵；六格拒绝分别命中字段号、宽度、区间、自指不一致、哨兵为零与
//! 对齐错位。校验对同一块缓冲幂等，重复校验不改变任何字节，这是装载器
//! 在每次进入新线程时复用同一套检查所依赖的性质。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xe950;

/// 编号非法。
pub const TC_EID: u32 = SEG_BASE + 0x01;
/// 登记顺序错误。
pub const TC_EORDER: u32 = SEG_BASE + 0x02;
/// 重复登记。
pub const TC_EDUP: u32 = SEG_BASE + 0x03;
/// 已冻结。
pub const TC_EFROZEN: u32 = SEG_BASE + 0x04;
/// 观测不存在的格。
pub const TC_ENOBS: u32 = SEG_BASE + 0x05;
/// 结论不符。
pub const TC_EMISMATCH: u32 = SEG_BASE + 0x06;
/// 账本失衡。
pub const TC_EBAL: u32 = SEG_BASE + 0x07;
/// 区间越界。
pub const TC_EOFF: u32 = SEG_BASE + 0x08;
/// 字段宽度不符。
pub const TC_EW: u32 = SEG_BASE + 0x09;
/// 对齐错位。
pub const TC_EALIGN: u32 = SEG_BASE + 0x0a;
/// 自指不一致。
pub const TC_EMIS: u32 = SEG_BASE + 0x0b;
/// 哨兵为零。
pub const TC_ECAN: u32 = SEG_BASE + 0x0c;
/// DTV 为空。
pub const TC_ENULL: u32 = SEG_BASE + 0x0d;
/// 标志位非法。
pub const TC_EMT: u32 = SEG_BASE + 0x0e;
/// 票号不一致。
pub const TC_ESEAL: u32 = SEG_BASE + 0x0f;

/// 判决：成功。
pub const V_OK: u32 = 0;

/// 操作：写八字节。
pub const OP_W64: u8 = 1;
/// 操作：写四字节。
pub const OP_W32: u8 = 2;
/// 操作：校验。
pub const OP_VERIFY: u8 = 3;
/// 操作：读八字节。
pub const OP_R64: u8 = 4;

/// 字段：TCB 自指针。
pub const F_TCB: u8 = 0;
/// 字段：DTV 指针。
pub const F_DTV: u8 = 1;
/// 字段：pthread 自指。
pub const F_SELF: u8 = 2;
/// 字段：多线程标志。
pub const F_MT: u8 = 3;
/// 字段：栈信息指针。
pub const F_SYSINFO: u8 = 4;
/// 字段：栈哨兵。
pub const F_GUARD: u8 = 5;
/// 字段数。
pub const N_FIELDS: usize = 6;
/// 头部长度。
pub const HEADER_LEN: u32 = 48;
/// 缓冲容量。
pub const ARENA: usize = 128;
/// 测试 TP 地址。
pub const TP: u64 = 0x0000_7f00_0000_0000;
/// 测试 sysinfo 地址。
pub const SYSINFO: u64 = 0x0000_7f00_0000_0040;
/// 测试哨兵。
pub const GUARD: u64 = 0xdead_beef_0000_0001;
/// 样本格数。
pub const SAMPLES: usize = 14;

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 单个字段声明。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Field {
    pub id: u8,
    pub offset: u32,
    pub width: u8,
}

impl Field {
    /// 构造。
    pub const fn new(id: u8, offset: u32, width: u8) -> Field {
        Field { id, offset, width }
    }
}

/// 头部布局。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    fields: [Field; N_FIELDS],
    total: u32,
}

impl Layout {
    /// 标准布局：0/8/16/24/32/40，宽度 8/8/8/4/8/8。
    pub const fn new() -> Layout {
        Layout {
            fields: [
                Field::new(F_TCB, 0, 8),
                Field::new(F_DTV, 8, 8),
                Field::new(F_SELF, 16, 8),
                Field::new(F_MT, 24, 4),
                Field::new(F_SYSINFO, 32, 8),
                Field::new(F_GUARD, 40, 8),
            ],
            total: HEADER_LEN,
        }
    }

    /// 自定义布局：总长取所有字段末端的最大值。
    pub const fn custom(fields: [Field; N_FIELDS]) -> Layout {
        let mut i = 0;
        let mut total = 0;
        while i < N_FIELDS {
            let end = fields[i].offset + fields[i].width as u32;
            if end > total {
                total = end;
            }
            i += 1;
        }
        Layout { fields, total }
    }

    /// 按字段号取声明。
    pub fn field(&self, id: u8) -> Option<Field> {
        let mut i = 0;
        while i < self.fields.len() {
            if let Some(f) = self.fields.get(i) {
                if f.id == id {
                    return Some(*f);
                }
            }
            i += 1;
        }
        None
    }

    /// 头部总长。
    pub fn total(&self) -> u32 {
        self.total
    }

    /// 字段数。
    pub fn len(&self) -> usize {
        self.fields.len()
    }

    /// 第 i 个声明。
    pub fn nth(&self, i: usize) -> Option<Field> {
        self.fields.get(i).copied()
    }
}

/// 写八字节字段：闸门顺序 字段号 → 区间 → 宽度 → 对齐。
pub fn write_u64(buf: &mut [u8; ARENA], l: &Layout, id: u8, v: u64) -> Result<(), u32> {
    let f = match l.field(id) {
        Some(x) => x,
        None => return Err(TC_EID),
    };
    let end = match f.offset.checked_add(8) {
        Some(e) => e,
        None => return Err(TC_EOFF),
    };
    if (end as usize) > ARENA || (f.offset as usize) >= ARENA {
        return Err(TC_EOFF);
    }
    if f.width != 8 {
        return Err(TC_EW);
    }
    if f.offset % 8 != 0 {
        return Err(TC_EALIGN);
    }
    let off = f.offset as usize;
    let mut i = 0;
    while i < 8 {
        match (buf.get_mut(off + i), off.checked_add(i)) {
            (Some(b), Some(_)) => *b = ((v >> (i * 8)) & 0xff) as u8,
            _ => return Err(TC_EOFF),
        }
        i += 1;
    }
    Ok(())
}

/// 写四字节字段：闸门顺序 字段号 → 区间 → 宽度 → 对齐 → 值宽。
pub fn write_u32(buf: &mut [u8; ARENA], l: &Layout, id: u8, v: u32) -> Result<(), u32> {
    let f = match l.field(id) {
        Some(x) => x,
        None => return Err(TC_EID),
    };
    let end = match f.offset.checked_add(4) {
        Some(e) => e,
        None => return Err(TC_EOFF),
    };
    if (end as usize) > ARENA || (f.offset as usize) >= ARENA {
        return Err(TC_EOFF);
    }
    if f.width != 4 {
        return Err(TC_EW);
    }
    if f.offset % 4 != 0 {
        return Err(TC_EALIGN);
    }
    let off = f.offset as usize;
    let mut i = 0;
    while i < 4 {
        match buf.get_mut(off + i) {
            Some(b) => *b = ((v >> (i * 8)) & 0xff) as u8,
            None => return Err(TC_EOFF),
        }
        i += 1;
    }
    Ok(())
}

/// 读八字节字段。
pub fn read_u64(buf: &[u8; ARENA], l: &Layout, id: u8) -> Result<u64, u32> {
    let f = match l.field(id) {
        Some(x) => x,
        None => return Err(TC_EID),
    };
    if f.width != 8 {
        return Err(TC_EW);
    }
    let off = f.offset as usize;
    if off >= ARENA || off + 8 > ARENA {
        return Err(TC_EOFF);
    }
    let mut v = 0u64;
    let mut i = 0;
    while i < 8 {
        match buf.get(off + i) {
            Some(b) => v |= (*b as u64) << (i * 8),
            None => return Err(TC_EOFF),
        }
        i += 1;
    }
    Ok(v)
}

/// 读四字节字段。
pub fn read_u32(buf: &[u8; ARENA], l: &Layout, id: u8) -> Result<u32, u32> {
    let f = match l.field(id) {
        Some(x) => x,
        None => return Err(TC_EID),
    };
    if f.width != 4 {
        return Err(TC_EW);
    }
    let off = f.offset as usize;
    if off >= ARENA || off + 4 > ARENA {
        return Err(TC_EOFF);
    }
    let mut v = 0u32;
    let mut i = 0;
    while i < 4 {
        match buf.get(off + i) {
            Some(b) => v |= (*b as u32) << (i * 8),
            None => return Err(TC_EOFF),
        }
        i += 1;
    }
    Ok(v)
}

/// 初始化：按 ABI 顺序写满六个字段，错误即停。
pub fn init(buf: &mut [u8; ARENA], l: &Layout, tp: u64, dtv: u64, sysinfo: u64, guard: u64) -> Result<(), u32> {
    write_u64(buf, l, F_TCB, tp)?;
    write_u64(buf, l, F_DTV, dtv)?;
    write_u64(buf, l, F_SELF, tp)?;
    write_u32(buf, l, F_MT, 1)?;
    write_u64(buf, l, F_SYSINFO, sysinfo)?;
    write_u64(buf, l, F_GUARD, guard)?;
    Ok(())
}

/// 校验：关系闸门 自指 → DTV 空/对齐/位置 → 标志 → 哨兵 → 栈信息对齐。
pub fn verify(buf: &[u8; ARENA], l: &Layout, tp: u64) -> Result<(), u32> {
    let tcb = read_u64(buf, l, F_TCB)?;
    if tcb != tp {
        return Err(TC_EMIS);
    }
    let selfp = read_u64(buf, l, F_SELF)?;
    if selfp != tp {
        return Err(TC_EMIS);
    }
    let dtv = read_u64(buf, l, F_DTV)?;
    if dtv == 0 {
        return Err(TC_ENULL);
    }
    if dtv % 8 != 0 {
        return Err(TC_EALIGN);
    }
    if dtv != tp + 8 {
        return Err(TC_EMIS);
    }
    let mt = read_u32(buf, l, F_MT)?;
    if mt > 1 {
        return Err(TC_EMT);
    }
    let guard = read_u64(buf, l, F_GUARD)?;
    if guard == 0 {
        return Err(TC_ECAN);
    }
    let si = read_u64(buf, l, F_SYSINFO)?;
    if si % 8 != 0 {
        return Err(TC_EALIGN);
    }
    Ok(())
}

/// 一个账本样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    pub slot: u8,
    pub id: u8,
    pub op: u8,
    pub ok: bool,
    pub code: u32,
    pub value: u64,
}

impl Sample {
    /// 空白格。
    pub const fn blank() -> Sample {
        Sample {
            slot: 0,
            id: 0,
            op: 0,
            ok: false,
            code: 0,
            value: 0,
        }
    }
}

/// FNV 折入。
pub fn fold(h: u64, v: u64) -> u64 {
    let mut x = h;
    let mut n = v;
    let mut i = 0;
    while i < 8 {
        x ^= n & 0xff;
        x = x.wrapping_mul(FNV_PRIME);
        n >>= 8;
        i += 1;
    }
    x
}

/// 样本账本。
#[derive(Clone, Copy, Debug)]
pub struct Ledger {
    rows: [Sample; SAMPLES],
    len: usize,
    seal: u64,
    frozen: bool,
    rejects: u32,
}

impl Ledger {
    /// 空账本。
    pub const fn new() -> Ledger {
        Ledger {
            rows: [Sample::blank(); SAMPLES],
            len: 0,
            seal: 0,
            frozen: false,
            rejects: 0,
        }
    }

    /// 登记一格。
    pub fn register(&mut self, row: Sample) -> Result<(), u32> {
        if self.frozen {
            return Err(TC_EFROZEN);
        }
        if (row.slot as usize) != self.len {
            return Err(TC_EORDER);
        }
        if self.len >= SAMPLES {
            return Err(TC_EID);
        }
        let mut i = 0;
        while i < self.len {
            match self.rows.get(i) {
                Some(x) => {
                    if x.slot == row.slot {
                        return Err(TC_EDUP);
                    }
                }
                None => return Err(TC_ENOBS),
            }
            i += 1;
        }
        match self.rows.get_mut(self.len) {
            Some(x) => *x = row,
            None => return Err(TC_EID),
        }
        self.len += 1;
        Ok(())
    }

    /// 格数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取样本。
    pub fn sample(&self, i: usize) -> Option<Sample> {
        if i >= self.len {
            return None;
        }
        self.rows.get(i).copied()
    }

    /// 取成功值。
    pub fn value(&self, i: usize) -> Option<u64> {
        match self.sample(i) {
            Some(s) => {
                if s.ok {
                    Some(s.value)
                } else {
                    None
                }
            }
            None => None,
        }
    }

    /// 成功计数。
    pub fn ok_count(&self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < self.len {
            if let Some(s) = self.rows.get(i) {
                if s.ok {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 拒绝计数。
    pub fn err_count(&self) -> usize {
        self.len - self.ok_count()
    }

    /// 对账。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES {
            return Err(TC_EBAL);
        }
        if self.ok_count() != 8 || self.err_count() != 6 {
            return Err(TC_EBAL);
        }
        let mut i = 0;
        while i < self.len {
            match self.rows.get(i) {
                Some(s) => {
                    if s.slot as usize != i {
                        return Err(TC_EORDER);
                    }
                    if s.ok {
                        if s.code != V_OK {
                            return Err(TC_EMISMATCH);
                        }
                    } else if s.code == V_OK || s.value != 0 {
                        return Err(TC_EMISMATCH);
                    }
                }
                None => return Err(TC_ENOBS),
            }
            i += 1;
        }
        let vals = [
            (0usize, TP),
            (1, TP + 8),
            (2, TP),
            (3, 1),
            (4, SYSINFO),
            (5, GUARD),
            (12, 0),
            (13, GUARD),
        ];
        let mut k = 0;
        while k < vals.len() {
            match vals.get(k) {
                Some((i, w)) => {
                    if self.value(*i) != Some(*w) {
                        return Err(TC_EMISMATCH);
                    }
                }
                None => return Err(TC_EBAL),
            }
            k += 1;
        }
        let ids = [F_TCB, F_DTV, F_SELF, F_MT, F_SYSINFO, F_GUARD];
        let mut j = 0;
        while j < ids.len() {
            match (self.sample(j), ids.get(j)) {
                (Some(s), Some(w)) => {
                    if s.id != *w {
                        return Err(TC_EMISMATCH);
                    }
                }
                _ => return Err(TC_EBAL),
            }
            j += 1;
        }
        let codes = [TC_EID, TC_EW, TC_EOFF, TC_EMIS, TC_ECAN, TC_EALIGN];
        let mut c = 0;
        let mut m = 0;
        while m < self.len {
            if let Some(s) = self.rows.get(m) {
                if !s.ok {
                    match codes.get(c) {
                        Some(w) => {
                            if s.code != *w {
                                return Err(TC_EMISMATCH);
                            }
                        }
                        None => return Err(TC_EBAL),
                    }
                    c += 1;
                }
            }
            m += 1;
        }
        if c != codes.len() {
            return Err(TC_EBAL);
        }
        Ok(())
    }

    /// 熔断票号。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0;
        while i < self.len {
            if let Some(s) = self.rows.get(i) {
                h = fold(h, s.slot as u64);
                h = fold(h, s.id as u64);
                h = fold(h, s.op as u64);
                h = fold(h, s.ok as u64);
                h = fold(h, s.code as u64);
                h = fold(h, s.value);
            }
            i += 1;
        }
        h
    }

    /// 冻结。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(TC_EFROZEN);
        }
        let s = self.seal();
        self.seal = s;
        self.frozen = true;
        Ok(s)
    }

    /// 重放。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        if fresh.len != self.len {
            return Err(TC_EBAL);
        }
        let h = fresh.seal();
        if self.frozen && h != self.seal {
            return Err(TC_ESEAL);
        }
        Ok(h)
    }

    /// 封存票号。
    pub fn root(&self) -> u64 {
        self.seal
    }

    /// 二次冻结次数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 是否已冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }
}

/// 由结果生成一格。
fn cell(slot: u8, id: u8, op: u8, r: Result<u64, u32>) -> Sample {
    match r {
        Ok(v) => Sample {
            slot,
            id,
            op,
            ok: true,
            code: V_OK,
            value: v,
        },
        Err(c) => Sample {
            slot,
            id,
            op,
            ok: false,
            code: c,
            value: 0,
        },
    }
}

/// 溢出布局：第六字段偏移 124，末端越过缓冲。
pub fn wide_layout() -> Layout {
    Layout::custom([
        Field::new(F_TCB, 0, 8),
        Field::new(F_DTV, 8, 8),
        Field::new(F_SELF, 16, 8),
        Field::new(F_MT, 24, 4),
        Field::new(F_SYSINFO, 32, 8),
        Field::new(F_GUARD, 124, 8),
    ])
}

/// 构建十四格账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let lay = Layout::new();
    let mut buf = [0u8; ARENA];

    let _ = l.register(cell(0, F_TCB, OP_W64, write_u64(&mut buf, &lay, F_TCB, TP).map(|_| TP)));
    let _ = l.register(cell(1, F_DTV, OP_W64, write_u64(&mut buf, &lay, F_DTV, TP + 8).map(|_| TP + 8)));
    let _ = l.register(cell(2, F_SELF, OP_W64, write_u64(&mut buf, &lay, F_SELF, TP).map(|_| TP)));
    let _ = l.register(cell(3, F_MT, OP_W32, write_u32(&mut buf, &lay, F_MT, 1).map(|_| 1)));
    let _ = l.register(cell(
        4,
        F_SYSINFO,
        OP_W64,
        write_u64(&mut buf, &lay, F_SYSINFO, SYSINFO).map(|_| SYSINFO),
    ));
    let _ = l.register(cell(
        5,
        F_GUARD,
        OP_W64,
        write_u64(&mut buf, &lay, F_GUARD, GUARD).map(|_| GUARD),
    ));

    let _ = l.register(cell(6, 9, OP_W64, write_u64(&mut buf, &lay, 9, 0).map(|_| 0)));
    let _ = l.register(cell(7, F_MT, OP_W64, write_u64(&mut buf, &lay, F_MT, 2).map(|_| 2)));
    let mut wide = [0u8; ARENA];
    let _ = l.register(cell(
        8,
        F_GUARD,
        OP_W64,
        write_u64(&mut wide, &wide_layout(), F_GUARD, 1).map(|_| 1),
    ));

    let mut bad_self = buf;
    let _ = write_u64(&mut bad_self, &lay, F_SELF, TP + 0x10);
    let _ = l.register(cell(
        9,
        F_SELF,
        OP_VERIFY,
        verify(&bad_self, &lay, TP).map(|_| 0),
    ));

    let mut no_guard = [0u8; ARENA];
    let _ = init(&mut no_guard, &lay, TP, TP + 8, SYSINFO, 0);
    let _ = l.register(cell(
        10,
        F_GUARD,
        OP_VERIFY,
        verify(&no_guard, &lay, TP).map(|_| 0),
    ));

    let mut odd_dtv = [0u8; ARENA];
    let _ = init(&mut odd_dtv, &lay, TP, TP + 9, SYSINFO, GUARD);
    let _ = l.register(cell(
        11,
        F_DTV,
        OP_VERIFY,
        verify(&odd_dtv, &lay, TP).map(|_| 0),
    ));

    let _ = l.register(cell(12, F_SELF, OP_VERIFY, verify(&buf, &lay, TP).map(|_| 0)));
    let _ = l.register(cell(
        13,
        F_GUARD,
        OP_R64,
        read_u64(&buf, &lay, F_GUARD),
    ));

    l
}

/// 四路注入：账本重放、偏移表、初始化回读、闸门优先级。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let base = l.len() == SAMPLES && l.audit().is_ok();
    let rep = l.replay();
    let root = l.seal();
    out[0] = base && rep == Ok(root) && root != 0;

    let lay = Layout::new();
    let want = [0u32, 8, 16, 24, 32, 40];
    let widths = [8u8, 8, 8, 4, 8, 8];
    let mut ok = true;
    let mut i = 0;
    while i < N_FIELDS {
        match (lay.nth(i), want.get(i), widths.get(i)) {
            (Some(f), Some(o), Some(w)) => {
                if f.offset != *o || f.width != *w || f.id != i as u8 {
                    ok = false;
                }
            }
            _ => ok = false,
        }
        i += 1;
    }
    out[1] = ok && lay.total() == HEADER_LEN;

    let mut buf = [0u8; ARENA];
    let wrote = init(&mut buf, &lay, TP, TP + 8, SYSINFO, GUARD).is_ok();
    let mut back = true;
    let mut j = 0;
    while j < N_FIELDS {
        let f = match lay.nth(j) {
            Some(v) => v,
            None => return out,
        };
        let want_v = match f.id {
            F_TCB => Some(TP),
            F_DTV => Some(TP + 8),
            F_SELF => Some(TP),
            F_SYSINFO => Some(SYSINFO),
            F_GUARD => Some(GUARD),
            _ => None,
        };
        if let Some(w) = want_v {
            if read_u64(&buf, &lay, f.id) != Ok(w) {
                back = false;
            }
        }
        j += 1;
    }
    if read_u32(&buf, &lay, F_MT) != Ok(1) {
        back = false;
    }
    out[2] = wrote && back && verify(&buf, &lay, TP).is_ok();

    let mut probe = [0u8; ARENA];
    let e_id = write_u64(&mut probe, &lay, 9, 0);
    let e_wide = write_u64(&mut probe, &lay, F_MT, 0);
    let e_off = write_u64(&mut probe, &wide_layout(), F_GUARD, 0);
    let mut zero_dtv = [0u8; ARENA];
    let _ = init(&mut zero_dtv, &lay, TP, 0, SYSINFO, GUARD);
    let e_null = verify(&zero_dtv, &lay, TP);
    let mut bad_mt = [0u8; ARENA];
    let _ = init(&mut bad_mt, &lay, TP, TP + 8, SYSINFO, GUARD);
    let _ = write_u32(&mut bad_mt, &lay, F_MT, 2);
    let e_mt = verify(&bad_mt, &lay, TP);
    out[3] = e_id == Err(TC_EID)
        && e_wide == Err(TC_EW)
        && e_off == Err(TC_EOFF)
        && e_null == Err(TC_ENULL)
        && e_mt == Err(TC_EMT)
        && write_u64(&mut probe, &wide_layout(), 9, 0) == Err(TC_EID);

    out
}

/// 十三项判据。
pub fn run_unx11310_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11310.tcblayout");
    let l = build_ledger();
    let lay = Layout::new();

    s.add(
        "TC1-十四格齐备",
        l.len() == SAMPLES && !l.is_empty() && l.sample(0).map(|x| x.op) == Some(OP_W64),
        "static",
    );

    s.add_dyn(
        "TC2-八允六拒分布正确",
        l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("ok={} err={}", l.ok_count(), l.err_count()),
    );

    s.add(
        "TC3-六字段偏移与宽度",
        {
            let want = [0u32, 8, 16, 24, 32, 40];
            let widths = [8u8, 8, 8, 4, 8, 8];
            let mut ok = true;
            let mut i = 0;
            while i < N_FIELDS {
                match (lay.nth(i), want.get(i), widths.get(i)) {
                    (Some(f), Some(o), Some(w)) => {
                        if f.offset != *o || f.width != *w {
                            ok = false;
                        }
                    }
                    _ => ok = false,
                }
                i += 1;
            }
            ok
        },
        "static",
    );

    s.add(
        "TC4-头部紧凑无重叠",
        {
            let mut prev_end = 0u32;
            let mut ok = lay.total() == HEADER_LEN;
            let mut i = 0;
            while i < N_FIELDS {
                if let Some(f) = lay.nth(i) {
                    if f.offset < prev_end || f.offset % f.width as u32 != 0 {
                        ok = false;
                    }
                    prev_end = f.offset + f.width as u32;
                } else {
                    ok = false;
                }
                i += 1;
            }
            ok && prev_end == HEADER_LEN
        },
        "static",
    );

    s.add(
        "TC5-写入落点正确",
        {
            let mut buf = [0u8; ARENA];
            let mut ok = write_u64(&mut buf, &lay, F_TCB, 0x0102_0304_0506_0708).is_ok();
            ok = ok && buf.get(0) == Some(&0x08) && buf.get(7) == Some(&0x01);
            ok = ok && write_u32(&mut buf, &lay, F_MT, 0xaabb_ccdd).is_ok();
            ok = ok && buf.get(24) == Some(&0xdd) && buf.get(27) == Some(&0xaa);
            ok = ok && buf.get(28) == Some(&0);
            ok
        },
        "static",
    );

    s.add(
        "TC6-DTV 恒为 TP 加八",
        {
            let mut buf = [0u8; ARENA];
            let wrote = init(&mut buf, &lay, TP, TP + 8, SYSINFO, GUARD).is_ok();
            let dtv = read_u64(&buf, &lay, F_DTV);
            let selfp = read_u64(&buf, &lay, F_SELF);
            wrote && dtv == Ok(TP + 8) && selfp == Ok(TP) && dtv == read_u64(&buf, &lay, F_TCB).map(|v| v + 8)
        },
        "static",
    );

    s.add(
        "TC7-字段号与宽度闸门",
        l.sample(6).map(|x| x.code) == Some(TC_EID)
            && l.sample(7).map(|x| x.code) == Some(TC_EW)
            && {
                let mut buf = [0u8; ARENA];
                write_u64(&mut buf, &lay, 9, 0) == Err(TC_EID)
                    && write_u32(&mut buf, &lay, F_GUARD, 0) == Err(TC_EW)
            },
        "static",
    );

    s.add(
        "TC8-区间闸门",
        l.sample(8).map(|x| x.code) == Some(TC_EOFF)
            && wide_layout().total() == 132
            && {
                let mut buf = [0u8; ARENA];
                write_u64(&mut buf, &wide_layout(), F_GUARD, 0) == Err(TC_EOFF)
            },
        "static",
    );

    s.add(
        "TC9-自指不一致闸门",
        l.sample(9).map(|x| x.code) == Some(TC_EMIS)
            && {
                let mut buf = [0u8; ARENA];
                let _ = init(&mut buf, &lay, TP, TP + 8, SYSINFO, GUARD);
                let _ = write_u64(&mut buf, &lay, F_TCB, TP + 8);
                verify(&buf, &lay, TP) == Err(TC_EMIS)
            },
        "static",
    );

    s.add(
        "TC10-哨兵闸门",
        l.sample(10).map(|x| x.code) == Some(TC_ECAN)
            && {
                let mut buf = [0u8; ARENA];
                let _ = init(&mut buf, &lay, TP, TP + 8, SYSINFO, 0);
                verify(&buf, &lay, TP) == Err(TC_ECAN)
            },
        "static",
    );

    s.add(
        "TC11-DTV 空与对齐闸门",
        l.sample(11).map(|x| x.code) == Some(TC_EALIGN)
            && {
                let mut a = [0u8; ARENA];
                let _ = init(&mut a, &lay, TP, 0, SYSINFO, GUARD);
                let mut b = [0u8; ARENA];
                let _ = init(&mut b, &lay, TP, TP + 8, SYSINFO + 1, GUARD);
                verify(&a, &lay, TP) == Err(TC_ENULL)
                    && verify(&b, &lay, TP) == Err(TC_EALIGN)
            },
        "static",
    );

    s.add(
        "TC12-校验幂等",
        {
            let mut buf = [0u8; ARENA];
            let _ = init(&mut buf, &lay, TP, TP + 8, SYSINFO, GUARD);
            let before = buf;
            let a = verify(&buf, &lay, TP);
            let b = verify(&buf, &lay, TP);
            a.is_ok() && b.is_ok() && buf == before && l.sample(12).map(|x| x.ok) == Some(true)
        },
        "static",
    );

    let mut f = build_ledger();
    let rep = f.replay();
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = f.freeze() == Err(TC_EFROZEN);
    let inj = inject_four();
    s.add_dyn(
        "TC13-冻结重放与四路注入",
        root != 0 && rep == Ok(root) && again && f.root() == root && f.rejects() == 1 && inj == [true; 4],
        &format!("root={:016x} inject={:?}", root, inj),
    );

    s
}

/// 摘要。
pub fn summary(l: &Ledger) -> alloc::string::String {
    format!("rows={} ok={} err={}", l.len(), l.ok_count(), l.err_count())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_offsets_match_abi() {
        let l = Layout::new();
        let want = [(F_TCB, 0u32, 8u8), (F_DTV, 8, 8), (F_SELF, 16, 8), (F_MT, 24, 4), (F_SYSINFO, 32, 8), (F_GUARD, 40, 8)];
        let mut i = 0;
        while i < want.len() {
            let (id, off, w) = want[i];
            assert_eq!(l.field(id), Some(Field::new(id, off, w)));
            i += 1;
        }
        assert_eq!(l.total(), HEADER_LEN);
        assert_eq!(l.field(9), None);
    }

    #[test]
    fn write_read_roundtrip() {
        let l = Layout::new();
        let mut buf = [0u8; ARENA];
        assert!(write_u64(&mut buf, &l, F_DTV, 0x1122_3344_5566_7788).is_ok());
        assert_eq!(read_u64(&buf, &l, F_DTV), Ok(0x1122_3344_5566_7788));
        assert!(write_u32(&mut buf, &l, F_MT, 7).is_ok());
        assert_eq!(read_u32(&buf, &l, F_MT), Ok(7));
        assert_eq!(read_u32(&buf, &l, F_DTV), Err(TC_EW));
    }

    #[test]
    fn gates_in_order() {
        let l = Layout::new();
        let mut buf = [0u8; ARENA];
        assert_eq!(write_u64(&mut buf, &l, 9, 0), Err(TC_EID));
        assert_eq!(write_u64(&mut buf, &l, F_MT, 0), Err(TC_EW));
        assert_eq!(write_u64(&mut buf, &wide_layout(), F_GUARD, 0), Err(TC_EOFF));
    }

    #[test]
    fn init_then_verify() {
        let l = Layout::new();
        let mut buf = [0u8; ARENA];
        assert!(init(&mut buf, &l, TP, TP + 8, SYSINFO, GUARD).is_ok());
        assert_eq!(verify(&buf, &l, TP), Ok(()));
        assert_eq!(read_u64(&buf, &l, F_TCB), Ok(TP));
        assert_eq!(read_u64(&buf, &l, F_GUARD), Ok(GUARD));
    }

    #[test]
    fn verify_rejects_broken_relations() {
        let l = Layout::new();
        let mut a = [0u8; ARENA];
        let _ = init(&mut a, &l, TP, 0, SYSINFO, GUARD);
        assert_eq!(verify(&a, &l, TP), Err(TC_ENULL));

        let mut b = [0u8; ARENA];
        let _ = init(&mut b, &l, TP, TP + 9, SYSINFO, GUARD);
        assert_eq!(verify(&b, &l, TP), Err(TC_EALIGN));

        let mut c = [0u8; ARENA];
        let _ = init(&mut c, &l, TP, TP + 8, SYSINFO, 0);
        assert_eq!(verify(&c, &l, TP), Err(TC_ECAN));

        let mut d = [0u8; ARENA];
        let _ = init(&mut d, &l, TP, TP + 8, SYSINFO, GUARD);
        let _ = write_u64(&mut d, &l, F_SELF, TP + 8);
        assert_eq!(verify(&d, &l, TP), Err(TC_EMIS));
    }

    #[test]
    fn custom_layout_total_is_max_end() {
        let l = wide_layout();
        assert_eq!(l.total(), 132);
        let l2 = Layout::custom([
            Field::new(0, 4, 4),
            Field::new(1, 8, 8),
            Field::new(2, 0, 4),
            Field::new(3, 16, 2),
            Field::new(4, 20, 4),
            Field::new(5, 24, 8),
        ]);
        assert_eq!(l2.total(), 32);
    }

    #[test]
    fn ledger_seals_and_replays() {
        let mut l = build_ledger();
        assert!(l.audit().is_ok());
        let root = l.freeze().expect("freeze");
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.freeze(), Err(TC_EFROZEN));
        assert_eq!(l.rejects(), 1);
    }

    #[test]
    fn checks_pass() {
        let s = run_unx11310_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
