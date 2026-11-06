//! UNX-F11315 dlopen 件 TLS 缺口账（0xe9a0 段）
//!
//! 静态链接器在装配阶段就把所有静态件的 TLS 段排进启动镜像，剩余的空间
//! 称为 surplus。运行期 dlopen 装进来的件若带 `PT_TLS`，只能在 surplus 里
//! 落座：落得下就拿到一个运行期模块号，落不下必须当场报错，而不是延迟到
//! 第一次访问 TLS 变量时才段错误。Linux 的 glibc 把这条边界定为
//! “cannot allocate memory in static TLS block”，本模型把它做成显式闸门。
//!
//! 模型把 TLS 竞技场切成上下两区：低区 `[0, STATIC_QUOTA)` 留给静态件，
//! 装配期就该排满；高区 `[STATIC_QUOTA, ARENA)` 是 surplus，只有运行期件
//! 才能消费。两个游标独立前进，互不侵占——这条分界正是“缺口账”能对上的
//! 前提：静态件越界说明装配期算错，运行期件越界说明 surplus 不够，两类
//! 故障码必须分开（ESTATIC / ESURR）。
//!
//! 释放走空闲块表：dlclose 归还的块按 (偏移, 尺寸) 记账，下一次同尺寸装载
//! 优先复用，避免游标只涨不落导致假性耗尽。句柄单调递增且不复用，因此
//! “同一偏移、不同句柄”是复用发生的可验证据；同句柄二次释放必须被
//! EDOUBLE 拦下，未知句柄必须被 EHANDLE 拦下。
//!
//! 账本十四格：八格成功（两格静态件、三格运行期件、一格释放、一格复用、
//! 一格探针），六格拒绝（surplus 耗尽、尺寸为零、对齐非法、未知句柄、
//! 重复释放、对齐优先于耗尽）。每格记录该步之后的游标与余量。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xe9a0;

/// 编号非法。
pub const DL_EID: u32 = SEG_BASE + 0x01;
/// 登记顺序错误。
pub const DL_EORDER: u32 = SEG_BASE + 0x02;
/// 重复登记。
pub const DL_EDUP: u32 = SEG_BASE + 0x03;
/// 已冻结。
pub const DL_EFROZEN: u32 = SEG_BASE + 0x04;
/// 观测不存在的格。
pub const DL_ENOBS: u32 = SEG_BASE + 0x05;
/// 结论不符。
pub const DL_EMISMATCH: u32 = SEG_BASE + 0x06;
/// 账本失衡。
pub const DL_EBAL: u32 = SEG_BASE + 0x07;
/// surplus 不足。
pub const DL_ESURR: u32 = SEG_BASE + 0x08;
/// 静态区不足。
pub const DL_ESTATIC: u32 = SEG_BASE + 0x09;
/// 尺寸为零。
pub const DL_ESIZE: u32 = SEG_BASE + 0x0a;
/// 对齐非法。
pub const DL_EALIGN: u32 = SEG_BASE + 0x0b;
/// 未知句柄。
pub const DL_EHANDLE: u32 = SEG_BASE + 0x0c;
/// 重复释放。
pub const DL_EDOUBLE: u32 = SEG_BASE + 0x0d;
/// 模块槽满。
pub const DL_EFULL: u32 = SEG_BASE + 0x0e;
/// 算术溢出。
pub const DL_EOVER: u32 = SEG_BASE + 0x0f;
/// 票号不一致。
pub const DL_ESEAL: u32 = SEG_BASE + 0x10;

/// 判决：成功。
pub const V_OK: u32 = 0;

/// 操作：静态件装载。
pub const OP_STATIC: u8 = 1;
/// 操作：运行期件装载。
pub const OP_DSO: u8 = 2;
/// 操作：释放。
pub const OP_RELEASE: u8 = 3;
/// 操作：余量探针。
pub const OP_PROBE: u8 = 4;

/// 槽位状态：空。
pub const ST_ABSENT: u8 = 0;
/// 槽位状态：在册。
pub const ST_TLS: u8 = 1;
/// 槽位状态：已归还。
pub const ST_GONE: u8 = 2;

/// 竞技场容量。
pub const ARENA: u32 = 2048;
/// 静态件配额。
pub const STATIC_QUOTA: u32 = 1024;
/// 模块槽数。
pub const SLOTS: usize = 8;
/// 对齐上限。
pub const MAX_ALIGN: u32 = 64;
/// 空闲块上限。
pub const FREES: usize = 4;
/// 样本格数。
pub const SAMPLES: usize = 14;

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 一个 TLS 模块槽。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mod {
    pub handle: u32,
    pub offset: u32,
    pub size: u32,
    pub state: u8,
    pub dso: bool,
}

impl Mod {
    /// 空槽。
    pub const fn blank() -> Mod {
        Mod {
            handle: 0,
            offset: 0,
            size: 0,
            state: ST_ABSENT,
            dso: false,
        }
    }
}

/// 空闲块。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Free {
    pub offset: u32,
    pub size: u32,
}

/// 装载器：双游标 + 空闲块表 + 单调句柄。
#[derive(Clone, Copy, Debug)]
pub struct Loader {
    slots: [Mod; SLOTS],
    frees: [Free; FREES],
    n: usize,
    nfree: usize,
    cur_static: u32,
    cur_dso: u32,
    seq: u32,
    releases: u32,
    reuses: u32,
}

/// 对齐检查：零、非二次幂、超上限全部拒绝。
pub fn check_align(align: u32) -> Result<(), u32> {
    if align == 0 || !align.is_power_of_two() || align > MAX_ALIGN {
        return Err(DL_EALIGN);
    }
    Ok(())
}

/// 向上取整到对齐；溢出即拒绝。
pub fn align_up(v: u32, align: u32) -> Result<u32, u32> {
    check_align(align)?;
    let mask = align - 1;
    match v.checked_add(mask) {
        Some(x) => Ok(x & !mask),
        None => Err(DL_EOVER),
    }
}

impl Loader {
    /// 空装载器。
    pub const fn new() -> Loader {
        Loader {
            slots: [Mod::blank(); SLOTS],
            frees: [Free { offset: 0, size: 0 }; FREES],
            n: 0,
            nfree: 0,
            cur_static: 0,
            cur_dso: STATIC_QUOTA,
            seq: 0,
            releases: 0,
            reuses: 0,
        }
    }

    /// 槽数。
    pub fn len(&self) -> usize {
        self.n
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// 取槽。
    pub fn slot(&self, i: usize) -> Option<Mod> {
        if i >= self.n {
            return None;
        }
        self.slots.get(i).copied()
    }

    /// 按句柄查找槽下标。
    pub fn find(&self, handle: u32) -> Option<usize> {
        let mut i = 0;
        while i < self.n {
            match self.slots.get(i) {
                Some(m) => {
                    if m.handle == handle {
                        return Some(i);
                    }
                }
                None => return None,
            }
            i += 1;
        }
        None
    }

    /// 静态区剩余。
    pub fn static_left(&self) -> u32 {
        STATIC_QUOTA.saturating_sub(self.cur_static)
    }

    /// surplus 剩余。
    pub fn surplus_left(&self) -> u32 {
        ARENA.saturating_sub(self.cur_dso)
    }

    /// 空闲块数。
    pub fn free_blocks(&self) -> usize {
        self.nfree
    }

    /// 释放次数。
    pub fn releases(&self) -> u32 {
        self.releases
    }

    /// 复用次数。
    pub fn reuses(&self) -> u32 {
        self.reuses
    }

    /// 从空闲块表取一块恰好合适的块。尺寸一致且对齐满足即可复用。
    fn take_free(&mut self, size: u32, align: u32) -> Option<u32> {
        let mut i = 0;
        while i < self.nfree {
            match self.frees.get(i) {
                Some(f) => {
                    if f.size == size && (f.offset & (align - 1)) == 0 {
                        let off = f.offset;
                        let mut j = i;
                        while j + 1 < self.nfree {
                            let next = match self.frees.get(j + 1) {
                                Some(v) => *v,
                                None => break,
                            };
                            if let Some(x) = self.frees.get_mut(j) {
                                *x = next;
                            }
                            j += 1;
                        }
                        self.nfree -= 1;
                        self.reuses += 1;
                        return Some(off);
                    }
                }
                None => return None,
            }
            i += 1;
        }
        None
    }

    /// 装载：对齐 → 尺寸 → 槽位 → 分区 → 空闲块 → 游标。
    pub fn load(&mut self, size: u32, align: u32, dso: bool) -> Result<u32, u32> {
        check_align(align)?;
        if size == 0 {
            return Err(DL_ESIZE);
        }
        if self.n >= SLOTS {
            return Err(DL_EFULL);
        }
        let offset = if dso {
            match self.take_free(size, align) {
                Some(off) => off,
                None => {
                    let off = align_up(self.cur_dso, align)?;
                    let end = match off.checked_add(size) {
                        Some(v) => v,
                        None => return Err(DL_EOVER),
                    };
                    if end > ARENA {
                        return Err(DL_ESURR);
                    }
                    self.cur_dso = end;
                    off
                }
            }
        } else {
            let off = align_up(self.cur_static, align)?;
            let end = match off.checked_add(size) {
                Some(v) => v,
                None => return Err(DL_EOVER),
            };
            if end > STATIC_QUOTA {
                return Err(DL_ESTATIC);
            }
            self.cur_static = end;
            off
        };
        self.seq += 1;
        let m = Mod {
            handle: self.seq,
            offset,
            size,
            state: ST_TLS,
            dso,
        };
        match self.slots.get_mut(self.n) {
            Some(x) => *x = m,
            None => return Err(DL_EFULL),
        }
        self.n += 1;
        Ok(self.seq)
    }

    /// 释放：未知句柄与二次释放分档。
    pub fn release(&mut self, handle: u32) -> Result<u32, u32> {
        let i = match self.find(handle) {
            Some(v) => v,
            None => return Err(DL_EHANDLE),
        };
        let m = match self.slots.get(i).copied() {
            Some(v) => v,
            None => return Err(DL_EHANDLE),
        };
        if m.state != ST_TLS {
            return Err(DL_EDOUBLE);
        }
        if m.dso {
            if self.nfree >= FREES {
                return Err(DL_EFULL);
            }
            match self.frees.get_mut(self.nfree) {
                Some(f) => *f = Free {
                    offset: m.offset,
                    size: m.size,
                },
                None => return Err(DL_EFULL),
            }
            self.nfree += 1;
        }
        if let Some(x) = self.slots.get_mut(i) {
            x.state = ST_GONE;
        }
        self.releases += 1;
        Ok(m.offset)
    }
}

/// 一个账本样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    pub slot: u8,
    pub op: u8,
    pub ok: bool,
    pub code: u32,
    pub value: u32,
}

impl Sample {
    /// 空白格。
    pub const fn blank() -> Sample {
        Sample {
            slot: 0,
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
            return Err(DL_EFROZEN);
        }
        if (row.slot as usize) != self.len {
            return Err(DL_EORDER);
        }
        if self.len >= SAMPLES {
            return Err(DL_EID);
        }
        let mut i = 0;
        while i < self.len {
            match self.rows.get(i) {
                Some(x) => {
                    if x.slot == row.slot {
                        return Err(DL_EDUP);
                    }
                }
                None => return Err(DL_ENOBS),
            }
            i += 1;
        }
        match self.rows.get_mut(self.len) {
            Some(x) => *x = row,
            None => return Err(DL_EID),
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

    /// 对账：形状、值与错误码逐格核对。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES {
            return Err(DL_EBAL);
        }
        if self.ok_count() != 8 || self.err_count() != 6 {
            return Err(DL_EBAL);
        }
        let mut i = 0;
        while i < self.len {
            match self.rows.get(i) {
                Some(s) => {
                    if s.slot as usize != i {
                        return Err(DL_EORDER);
                    }
                    if s.ok {
                        if s.code != V_OK {
                            return Err(DL_EMISMATCH);
                        }
                    } else if s.code == V_OK || s.value != 0 {
                        return Err(DL_EMISMATCH);
                    }
                }
                None => return Err(DL_ENOBS),
            }
            i += 1;
        }
        let vals = [0u32, 512, 1024, 1280, 1792, 1024, 1024, 0];
        let mut k = 0;
        while k < vals.len() {
            match (self.sample(k), vals.get(k)) {
                (Some(s), Some(w)) => {
                    if !s.ok || s.value != *w {
                        return Err(DL_EMISMATCH);
                    }
                }
                _ => return Err(DL_EBAL),
            }
            k += 1;
        }
        let codes = [
            DL_ESURR,
            DL_ESIZE,
            DL_EALIGN,
            DL_EHANDLE,
            DL_EDOUBLE,
            DL_EALIGN,
        ];
        let mut c = 0;
        let mut j = 8;
        while j < self.len {
            match self.rows.get(j) {
                Some(s) => match codes.get(c) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(DL_EMISMATCH);
                        }
                    }
                    None => return Err(DL_EBAL),
                },
                None => return Err(DL_ENOBS),
            }
            c += 1;
            j += 1;
        }
        if c != codes.len() {
            return Err(DL_EBAL);
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
                h = fold(h, s.op as u64);
                h = fold(h, s.ok as u64);
                h = fold(h, s.code as u64);
                h = fold(h, s.value as u64);
            }
            i += 1;
        }
        h
    }

    /// 冻结。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(DL_EFROZEN);
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
            return Err(DL_EBAL);
        }
        let h = fresh.seal();
        if self.frozen && h != self.seal {
            return Err(DL_ESEAL);
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

/// 由装载结果生成一格；值取新槽的落座偏移。
pub fn load_cell(l: &mut Loader, slot: u8, op: u8, size: u32, align: u32, dso: bool) -> Sample {
    match l.load(size, align, dso) {
        Ok(_) => {
            let n = l.len();
            let v = if n == 0 {
                0
            } else {
                match l.slot(n - 1) {
                    Some(m) => m.offset,
                    None => 0,
                }
            };
            Sample {
                slot,
                op,
                ok: true,
                code: V_OK,
                value: v,
            }
        }
        Err(c) => Sample {
            slot,
            op,
            ok: false,
            code: c,
            value: 0,
        },
    }
}

/// 由装载结果生成一格。
pub fn cell(slot: u8, op: u8, r: Result<u32, u32>) -> Sample {
    match r {
        Ok(v) => Sample {
            slot,
            op,
            ok: true,
            code: V_OK,
            value: v,
        },
        Err(c) => Sample {
            slot,
            op,
            ok: false,
            code: c,
            value: 0,
        },
    }
}

/// 构建十四格账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut ld = Loader::new();

    let _ = l.register(load_cell(&mut ld, 0, OP_STATIC, 512, 16, false));
    let _ = l.register(load_cell(&mut ld, 1, OP_STATIC, 512, 16, false));
    let _ = l.register(load_cell(&mut ld, 2, OP_DSO, 256, 16, true));
    let _ = l.register(load_cell(&mut ld, 3, OP_DSO, 512, 32, true));
    let _ = l.register(load_cell(&mut ld, 4, OP_DSO, 256, 8, true));

    let rel = ld.release(3);
    let _ = l.register(Sample {
        slot: 5,
        op: OP_RELEASE,
        ok: rel.is_ok(),
        code: match rel {
            Ok(_) => V_OK,
            Err(c) => c,
        },
        value: 1024,
    });

    let _ = l.register(load_cell(&mut ld, 6, OP_DSO, 256, 8, true));
    let _ = l.register(Sample {
        slot: 7,
        op: OP_PROBE,
        ok: true,
        code: V_OK,
        value: ld.surplus_left(),
    });

    let _ = l.register(load_cell(&mut ld, 8, OP_DSO, 512, 16, true));
    let _ = l.register(load_cell(&mut ld, 9, OP_DSO, 0, 16, true));
    let _ = l.register(load_cell(&mut ld, 10, OP_DSO, 64, 12, true));
    let _ = l.register(cell(11, OP_RELEASE, ld.release(99)));
    let _ = l.register(cell(12, OP_RELEASE, ld.release(3)));
    let _ = l.register(load_cell(&mut ld, 13, OP_DSO, 8, 128, true));

    l
}

/// 四路注入：账本重放、释放复用、分区上限、闸门优先级。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let base = l.len() == SAMPLES && l.audit().is_ok();
    let rep = l.replay();
    let root = l.seal();
    out[0] = base && rep == Ok(root) && root != 0;

    let mut ld = Loader::new();
    let a = ld.load(64, 8, true);
    let b = ld.load(64, 8, true);
    let rel = ld.release(1);
    let c = ld.load(64, 8, true);
    out[1] = a == Ok(1)
        && b == Ok(2)
        && rel == Ok(1024)
        && c == Ok(3)
        && ld.free_blocks() == 0
        && ld.reuses() == 1;

    let mut ld2 = Loader::new();
    let s1 = ld2.load(1024, 1, false);
    let s2 = ld2.load(1, 1, false);
    out[2] = s1 == Ok(1) && s2 == Err(DL_ESTATIC) && ld2.static_left() == 0;

    out[3] = dl_gate_probe();

    out
}

/// 闸门优先级：对齐先于尺寸先于耗尽先于溢出。
pub fn dl_gate_probe() -> bool {
    let mut ld = Loader::new();
    if ld.load(0, 12, true) != Err(DL_EALIGN) {
        return false;
    }
    if ld.load(0, 16, true) != Err(DL_ESIZE) {
        return false;
    }
    if ld.load(4096, 16, true) != Err(DL_ESURR) {
        return false;
    }
    if ld.load(u32::MAX, 16, true) != Err(DL_EOVER) {
        return false;
    }
    if ld.load(64, 64, true) != Ok(1) {
        return false;
    }
    let mut full = Loader::new();
    let mut i = 0;
    while i < SLOTS {
        if full.load(8, 8, true) != Ok(i as u32 + 1) {
            return false;
        }
        i += 1;
    }
    if full.load(8, 8, true) != Err(DL_EFULL) {
        return false;
    }
    if full.load(8, 4, true) != Err(DL_EFULL) {
        return false;
    }
    true
}

/// 十三项判据。
pub fn run_unx11315_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11315.dlopen_tls");
    let l = build_ledger();
    let mut ld = Loader::new();
    let _ = ld.load(512, 16, false);
    let _ = ld.load(512, 16, false);
    let _ = ld.load(256, 16, true);
    let _ = ld.load(512, 32, true);
    let _ = ld.load(256, 8, true);

    s.add(
        "DL1-十四格齐备",
        l.len() == SAMPLES && !l.is_empty() && l.sample(0).map(|x| x.op) == Some(OP_STATIC),
        "static",
    );

    s.add_dyn(
        "DL2-八允六拒分布正确",
        l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("ok={} err={}", l.ok_count(), l.err_count()),
    );

    s.add(
        "DL3-静态件落低区",
        ld.slot(0).map(|m| (m.offset, m.size)) == Some((0, 512))
            && ld.slot(1).map(|m| (m.offset, m.size)) == Some((512, 512))
            && ld.static_left() == 0,
        "static",
    );

    s.add(
        "DL4-运行期件落 surplus",
        ld.slot(2).map(|m| (m.offset, m.dso)) == Some((1024, true))
            && ld.slot(3).map(|m| m.offset) == Some(1280)
            && ld.slot(3).map(|m| m.offset % 32) == Some(0)
            && ld.slot(4).map(|m| m.offset) == Some(1792),
        "static",
    );

    s.add(
        "DL5-句柄单调递增",
        ld.slot(0).map(|m| m.handle) == Some(1)
            && ld.slot(4).map(|m| m.handle) == Some(5)
            && ld.slot(2).map(|m| m.handle) == Some(3)
            && ld.len() == 5,
        "static",
    );

    s.add(
        "DL6-释放归还空闲块",
        ld.release(3) == Ok(1024)
            && ld.free_blocks() == 1
            && ld.releases() == 1
            && ld.slot(2).map(|m| m.state) == Some(ST_GONE),
        "static",
    );

    s.add(
        "DL7-surplus 耗尽闸门",
        l.sample(8).map(|x| x.code) == Some(DL_ESURR)
            && l.sample(7).map(|x| x.value) == Some(0)
            && {
                let mut p = Loader::new();
                p.load(512, 1, true) == Ok(1) && p.load(512, 1, true) == Ok(2)
                    && p.load(1, 1, true) == Err(DL_ESURR)
            },
        "static",
    );

    s.add(
        "DL8-尺寸零闸门",
        l.sample(9).map(|x| x.code) == Some(DL_ESIZE)
            && {
                let mut p = Loader::new();
                p.load(0, 1, false) == Err(DL_ESIZE) && p.load(0, 12, false) == Err(DL_EALIGN)
            },
        "static",
    );

    s.add(
        "DL9-对齐闸门",
        l.sample(10).map(|x| x.code) == Some(DL_EALIGN)
            && l.sample(13).map(|x| x.code) == Some(DL_EALIGN)
            && check_align(64).is_ok()
            && check_align(96) == Err(DL_EALIGN)
            && align_up(7, 8) == Ok(8)
            && align_up(8, 8) == Ok(8),
        "static",
    );

    s.add(
        "DL10-句柄闸门",
        l.sample(11).map(|x| x.code) == Some(DL_EHANDLE)
            && l.sample(12).map(|x| x.code) == Some(DL_EDOUBLE)
            && {
                let mut p = Loader::new();
                p.load(8, 8, true) == Ok(1)
                    && p.release(1) == Ok(1024)
                    && p.release(1) == Err(DL_EDOUBLE)
                    && p.release(7) == Err(DL_EHANDLE)
            },
        "static",
    );

    s.add(
        "DL11-释放后同尺寸复用",
        {
            let mut p = Loader::new();
            let a = p.load(128, 16, true);
            let b = p.load(128, 16, true);
            let rel = p.release(1);
            let c = p.load(128, 16, true);
            a == Ok(1)
                && b == Ok(2)
                && rel == Ok(1024)
                && c == Ok(3)
                && p.slot(2).map(|m| m.offset) == Some(1024)
                && p.free_blocks() == 0
                && p.slot(2).map(|m| m.handle) != p.slot(0).map(|m| m.handle)
        },
        "static",
    );

    s.add(
        "DL12-槽满闸门",
        {
            let mut p = Loader::new();
            let mut i = 0;
            let mut all = true;
            while i < SLOTS {
                if p.load(16, 8, true) != Ok(i as u32 + 1) {
                    all = false;
                }
                i += 1;
            }
            all && p.len() == SLOTS && p.load(16, 8, true) == Err(DL_EFULL)
        },
        "static",
    );

    s.add(
        "DL13-冻结重放与四路注入",
        {
            let mut f = build_ledger();
            let rep = f.replay();
            let one = f.freeze();
            let two = f.freeze();
            let inj = inject_four();
            one.is_ok()
                && one == rep
                && two == Err(DL_EFROZEN)
                && f.rejects() == 1
                && f.is_frozen()
                && f.root() != 0
                && inj == [true, true, true, true]
        },
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert!(l.audit().is_ok());
    }

    #[test]
    fn checks_pass() {
        let s = run_unx11315_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn surplus_is_monotone_without_release() {
        let mut ld = Loader::new();
        let mut left = ld.surplus_left();
        let mut i = 0;
        while i < 4 {
            assert!(ld.load(64, 8, true).is_ok());
            let now = ld.surplus_left();
            assert!(now <= left);
            left = now;
            i += 1;
        }
        assert_eq!(ld.surplus_left(), ARENA - STATIC_QUOTA - 256);
    }

    #[test]
    fn reuse_requires_exact_size() {
        let mut ld = Loader::new();
        assert_eq!(ld.load(64, 8, true), Ok(1));
        assert_eq!(ld.release(1), Ok(1024));
        assert_eq!(ld.load(32, 8, true), Ok(2));
        assert_eq!(ld.slot(1).map(|m| m.offset), Some(1024 + 64));
        assert_eq!(ld.free_blocks(), 1);
    }

    #[test]
    fn static_quota_is_hard_wall() {
        let mut ld = Loader::new();
        assert_eq!(ld.load(1024, 1, false), Ok(1));
        assert_eq!(ld.static_left(), 0);
        assert_eq!(ld.load(1, 1, false), Err(DL_ESTATIC));
    }

    #[test]
    fn double_release_and_unknown_handle() {
        let mut ld = Loader::new();
        assert_eq!(ld.load(8, 8, true), Ok(1));
        assert_eq!(ld.release(1), Ok(1024));
        assert_eq!(ld.release(1), Err(DL_EDOUBLE));
        assert_eq!(ld.release(2), Err(DL_EHANDLE));
    }

    #[test]
    fn freeze_then_replay_matches() {
        let mut l = build_ledger();
        let root = l.seal();
        assert_eq!(l.freeze(), Ok(root));
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.freeze(), Err(DL_EFROZEN));
    }

    #[test]
    fn gate_order_is_align_then_size() {
        assert!(dl_gate_probe());
        let mut ld = Loader::new();
        assert_eq!(ld.load(0, 0, true), Err(DL_EALIGN));
        assert_eq!(ld.load(0, 64, true), Err(DL_ESIZE));
    }
}
