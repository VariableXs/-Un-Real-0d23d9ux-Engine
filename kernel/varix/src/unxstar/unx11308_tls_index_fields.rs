//! UNX-F11308 tls_index 双字段与通用访问（0xe930 段）
//!
//! `tls_index` 是通用动态 TLS 的寻址票据，只有两个字段：模块号与块内偏移。
//! 编译器把这对字段写进重定位位置，运行期由 `__tls_get_addr` 用它查表得到
//! 本线程地址。字段宽度因此受 ABI 约束：模块号是 16 位（保留值 0xffff 表示
//! 无效），偏移被压进 24 位，超出即视为不可重定位。模型把打包/解包做成
//! 显式闸门，任何越宽字段、保留位、保留模块号都必须在解包前拒绝。
//!
//! 通用访问有两条语义等价的路径：GD（模块 + 偏移，经 DTV 查块）与
//! LD（同一票据，先打包再解包后查块）。二者在编码无损时必须逐样本同值——
//! 这正是编译器把 GD 优化成 LE/LD 快路径所依赖的前提。模型对每个样本同时
//! 求两条路径并比较，只要有一条不等即判失败。
//!
//! IE 快路径是第三条路径：启动期模块的变量偏移被写死成 TP 负偏移，访问
//! 不查表。模型允许用 IE 偏移直接访问，但要求它与 GD 求得的地址一致，
//! 否则按“偏移错配”拒绝——这让“同一变量三条路径同址”成为可验不变量。
//!
//! 账本十四格：八格成功（五格 GD、一格 LD、一格解包往返、一格 IE）、
//! 六格拒绝（偏移越界、模块越界、保留模块号、打包越宽、解包保留位、
//! IE 错配）。每个成功样本记录三种路径的地址，拒绝样本记录闸门错误码。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xe930;

/// 编号非法。
pub const TG_EID: u32 = SEG_BASE + 0x01;
/// 登记顺序错误。
pub const TG_EORDER: u32 = SEG_BASE + 0x02;
/// 重复登记。
pub const TG_EDUP: u32 = SEG_BASE + 0x03;
/// 已冻结。
pub const TG_EFROZEN: u32 = SEG_BASE + 0x04;
/// 观测不存在的格。
pub const TG_ENOBS: u32 = SEG_BASE + 0x05;
/// 结论不符。
pub const TG_EMISMATCH: u32 = SEG_BASE + 0x06;
/// 账本失衡。
pub const TG_EBAL: u32 = SEG_BASE + 0x07;
/// 模块号越界。
pub const TG_EMOD: u32 = SEG_BASE + 0x08;
/// 偏移越界。
pub const TG_EOFF: u32 = SEG_BASE + 0x09;
/// 偏移超过编码宽度。
pub const TG_EW: u32 = SEG_BASE + 0x0a;
/// 保留位非零。
pub const TG_ERES: u32 = SEG_BASE + 0x0b;
/// IE 偏移错配。
pub const TG_EIE: u32 = SEG_BASE + 0x0c;
/// 算术溢出。
pub const TG_EOVER: u32 = SEG_BASE + 0x0d;
/// 票号不一致。
pub const TG_ESEAL: u32 = SEG_BASE + 0x0e;

/// 判决：成功。
pub const V_OK: u32 = 0;

/// 操作：GD 访问。
pub const OP_GD: u8 = 1;
/// 操作：LD 访问。
pub const OP_LD: u8 = 2;
/// 操作：打包。
pub const OP_PACK: u8 = 3;
/// 操作：解包。
pub const OP_UNPACK: u8 = 4;
/// 操作：IE 访问。
pub const OP_IE: u8 = 5;

/// 模块数。
pub const MODS: usize = 4;
/// 偏移编码宽度。
pub const OFFSET_BITS: u32 = 24;
/// 偏移编码上限。
pub const OFFSET_LIMIT: u32 = 1 << OFFSET_BITS;
/// 保留模块号。
pub const MODULE_RESERVED: u16 = 0xffff;
/// TP 偏移允许区间。
pub const TP_RANGE: i32 = 4096;
/// 样本格数。
pub const SAMPLES: usize = 14;

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 通用 TLS 票据。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TlsIndex {
    pub module: u16,
    pub offset: u32,
}

impl TlsIndex {
    /// 构造。
    pub const fn make(module: u16, offset: u32) -> TlsIndex {
        TlsIndex { module, offset }
    }

    /// 打包成 64 位字：模块号占 32~47 位，偏移占 0~23 位，其余必须为零。
    pub fn pack(&self) -> Result<u64, u32> {
        if self.module == MODULE_RESERVED {
            return Err(TG_EMOD);
        }
        if self.offset >= OFFSET_LIMIT {
            return Err(TG_EW);
        }
        Ok(((self.module as u64) << 32) | self.offset as u64)
    }

    /// 解包，拒绝保留位与非法的模块号/偏移。
    pub fn unpack(v: u64) -> Result<TlsIndex, u32> {
        if (v >> 48) != 0 {
            return Err(TG_ERES);
        }
        let module = ((v >> 32) & 0xffff) as u16;
        let offset = (v & 0xffff_ffff) as u32;
        if offset >= OFFSET_LIMIT {
            return Err(TG_EW);
        }
        if module == MODULE_RESERVED {
            return Err(TG_EMOD);
        }
        Ok(TlsIndex { module, offset })
    }
}

/// TP 空间里的模块段表。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Gdt {
    base: [i32; MODS],
    size: [u32; MODS],
}

impl Gdt {
    /// 标准表：模块 0 -128/64、模块 1 -64/32、模块 2 0/48、模块 3 64/16。
    pub const fn new() -> Gdt {
        Gdt {
            base: [-128, -64, 0, 64],
            size: [64, 32, 48, 16],
        }
    }

    /// 自定义表。
    pub const fn custom(base: [i32; MODS], size: [u32; MODS]) -> Gdt {
        Gdt { base, size }
    }

    /// 取段。
    pub fn seg(&self, m: usize) -> Option<(i32, u32)> {
        match (self.base.get(m), self.size.get(m)) {
            (Some(b), Some(s)) => Some((*b, *s)),
            _ => None,
        }
    }

    /// 段数。
    pub fn len(&self) -> usize {
        MODS
    }
}

/// 通用访问：模块号 + 偏移 → TP 相对地址。
pub fn access(ti: TlsIndex, gdt: &Gdt) -> Result<i32, u32> {
    if ti.module == MODULE_RESERVED {
        return Err(TG_EMOD);
    }
    let (base, size) = match gdt.seg(ti.module as usize) {
        Some(v) => v,
        None => return Err(TG_EMOD),
    };
    if ti.offset >= size {
        return Err(TG_EOFF);
    }
    let addr = match base.checked_add(ti.offset as i32) {
        Some(v) => v,
        None => return Err(TG_EOVER),
    };
    if addr < -TP_RANGE || addr >= TP_RANGE {
        return Err(TG_EOFF);
    }
    Ok(addr)
}

/// LD 路径：先打包再解包，随后与 GD 同法。
pub fn access_ld(ti: TlsIndex, gdt: &Gdt) -> Result<i32, u32> {
    let packed = ti.pack()?;
    let back = TlsIndex::unpack(packed)?;
    access(back, gdt)
}

/// IE 快路径：偏移写死，要求与 GD 求得的地址一致。
pub fn access_ie(ti: TlsIndex, ie_off: i32, gdt: &Gdt) -> Result<i32, u32> {
    let want = access(ti, gdt)?;
    if ie_off != want {
        return Err(TG_EIE);
    }
    if ie_off < -TP_RANGE || ie_off >= TP_RANGE {
        return Err(TG_EOFF);
    }
    Ok(ie_off)
}

/// 一个账本样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    pub slot: u8,
    pub op: u8,
    pub ok: bool,
    pub code: u32,
    pub module: u16,
    pub offset: u32,
    pub addr: i32,
    pub gd: i32,
    pub ld: i32,
}

impl Sample {
    /// 空白格。
    pub const fn blank() -> Sample {
        Sample {
            slot: 0,
            op: 0,
            ok: false,
            code: 0,
            module: 0,
            offset: 0,
            addr: 0,
            gd: 0,
            ld: 0,
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

    /// 登记一格：票据、操作、主路径结果、GD/LD 双值。
    pub fn register(
        &mut self,
        slot: u8,
        op: u8,
        ti: TlsIndex,
        r: Result<i32, u32>,
        gd: i32,
        ld: i32,
    ) -> Result<(), u32> {
        if self.frozen {
            return Err(TG_EFROZEN);
        }
        if (slot as usize) != self.len {
            return Err(TG_EORDER);
        }
        if self.len >= SAMPLES {
            return Err(TG_EID);
        }
        let mut i = 0;
        while i < self.len {
            match self.rows.get(i) {
                Some(x) => {
                    if x.slot == slot {
                        return Err(TG_EDUP);
                    }
                }
                None => return Err(TG_ENOBS),
            }
            i += 1;
        }
        let row = match r {
            Ok(addr) => Sample {
                slot,
                op,
                ok: true,
                code: V_OK,
                module: ti.module,
                offset: ti.offset,
                addr,
                gd,
                ld,
            },
            Err(c) => {
                if c == V_OK {
                    return Err(TG_EMISMATCH);
                }
                Sample {
                    slot,
                    op,
                    ok: false,
                    code: c,
                    module: ti.module,
                    offset: ti.offset,
                    addr: 0,
                    gd,
                    ld,
                }
            }
        };
        match self.rows.get_mut(self.len) {
            Some(x) => *x = row,
            None => return Err(TG_EID),
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

    /// 取地址。
    pub fn addr(&self, i: usize) -> Option<i32> {
        match self.sample(i) {
            Some(s) => {
                if s.ok {
                    Some(s.addr)
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
            return Err(TG_EBAL);
        }
        if self.ok_count() != 8 || self.err_count() != 6 {
            return Err(TG_EBAL);
        }
        let mut i = 0;
        while i < self.len {
            match self.rows.get(i) {
                Some(s) => {
                    if s.slot as usize != i {
                        return Err(TG_EORDER);
                    }
                    if s.ok {
                        if s.code != V_OK {
                            return Err(TG_EMISMATCH);
                        }
                        if s.op == OP_GD && (s.gd != s.addr || s.ld != s.addr) {
                            return Err(TG_EMISMATCH);
                        }
                        if s.op == OP_IE && s.gd != s.addr {
                            return Err(TG_EMISMATCH);
                        }
                    } else {
                        if s.code == V_OK || s.addr != 0 {
                            return Err(TG_EMISMATCH);
                        }
                        if s.op == OP_GD && (s.gd != 0 || s.ld != 0) {
                            return Err(TG_EMISMATCH);
                        }
                    }
                }
                None => return Err(TG_ENOBS),
            }
            i += 1;
        }
        let addrs = [-128i32, -65, -33, 0, 79];
        let mut k = 0;
        while k < addrs.len() {
            match addrs.get(k) {
                Some(a) => {
                    if self.addr(k) != Some(*a) {
                        return Err(TG_EMISMATCH);
                    }
                }
                None => return Err(TG_EBAL),
            }
            k += 1;
        }
        if self.addr(8) != Some(0) || self.addr(11) != Some(100) || self.addr(12) != Some(-65) {
            return Err(TG_EMISMATCH);
        }
        let codes = [TG_EOFF, TG_EMOD, TG_EMOD, TG_EW, TG_ERES, TG_EIE];
        let mut c = 0;
        let mut j = 0;
        while j < self.len {
            if let Some(s) = self.rows.get(j) {
                if !s.ok {
                    match codes.get(c) {
                        Some(w) => {
                            if s.code != *w {
                                return Err(TG_EMISMATCH);
                            }
                        }
                        None => return Err(TG_EBAL),
                    }
                    c += 1;
                }
            }
            j += 1;
        }
        if c != codes.len() {
            return Err(TG_EBAL);
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
                h = fold(h, s.module as u64);
                h = fold(h, s.offset as u64);
                h = fold(h, s.addr as u32 as u64);
                h = fold(h, s.gd as u32 as u64);
                h = fold(h, s.ld as u32 as u64);
            }
            i += 1;
        }
        h
    }

    /// 冻结。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(TG_EFROZEN);
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
            return Err(TG_EBAL);
        }
        let h = fresh.seal();
        if self.frozen && h != self.seal {
            return Err(TG_ESEAL);
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

/// 登记一格时同时求 GD 与 LD，保证双路径可对照。
fn sample_op(l: &mut Ledger, slot: u8, op: u8, ti: TlsIndex, gdt: &Gdt, r: Result<i32, u32>) {
    let gd = match r {
        Ok(v) => v,
        Err(_) => 0,
    };
    let ld = match access_ld(ti, gdt) {
        Ok(v) => v,
        Err(_) => 0,
    };
    let _ = l.register(slot, op, ti, r, gd, ld);
}

/// 构建十四格账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let g = Gdt::new();

    sample_op(&mut l, 0, OP_GD, TlsIndex::make(0, 0), &g, access(TlsIndex::make(0, 0), &g));
    sample_op(&mut l, 1, OP_GD, TlsIndex::make(0, 63), &g, access(TlsIndex::make(0, 63), &g));
    sample_op(&mut l, 2, OP_GD, TlsIndex::make(1, 31), &g, access(TlsIndex::make(1, 31), &g));
    sample_op(&mut l, 3, OP_GD, TlsIndex::make(2, 0), &g, access(TlsIndex::make(2, 0), &g));
    sample_op(&mut l, 4, OP_GD, TlsIndex::make(3, 15), &g, access(TlsIndex::make(3, 15), &g));
    sample_op(&mut l, 5, OP_GD, TlsIndex::make(1, 32), &g, access(TlsIndex::make(1, 32), &g));
    sample_op(&mut l, 6, OP_GD, TlsIndex::make(4, 0), &g, access(TlsIndex::make(4, 0), &g));
    sample_op(
        &mut l,
        7,
        OP_GD,
        TlsIndex::make(MODULE_RESERVED, 0),
        &g,
        access(TlsIndex::make(MODULE_RESERVED, 0), &g),
    );
    sample_op(&mut l, 8, OP_LD, TlsIndex::make(2, 0), &g, access_ld(TlsIndex::make(2, 0), &g));

    let wide = TlsIndex::make(1, OFFSET_LIMIT);
    let wide_r = match wide.pack() {
        Ok(v) => Ok(v as i32),
        Err(c) => Err(c),
    };
    sample_op(&mut l, 9, OP_PACK, wide, &g, wide_r);

    let res = match TlsIndex::unpack(0x0001_0000_0000_0000) {
        Ok(t) => Ok(t.offset as i32),
        Err(c) => Err(c),
    };
    sample_op(&mut l, 10, OP_UNPACK, TlsIndex::make(0, 0), &g, res);

    let round = TlsIndex::make(2, 100);
    let round_r = match round.pack() {
        Ok(v) => match TlsIndex::unpack(v) {
            Ok(t) => Ok(t.offset as i32),
            Err(c) => Err(c),
        },
        Err(c) => Err(c),
    };
    sample_op(&mut l, 11, OP_UNPACK, round, &g, round_r);

    sample_op(
        &mut l,
        12,
        OP_IE,
        TlsIndex::make(0, 63),
        &g,
        access_ie(TlsIndex::make(0, 63), -65, &g),
    );
    sample_op(
        &mut l,
        13,
        OP_IE,
        TlsIndex::make(0, 63),
        &g,
        access_ie(TlsIndex::make(0, 63), -66, &g),
    );

    l
}

/// 四路注入：账本重放、编码往返、双路径等价、闸门优先级。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let base = l.len() == SAMPLES && l.audit().is_ok();
    let rep = l.replay();
    let root = l.seal();
    out[0] = base && rep == Ok(root) && root != 0;

    let cases = [
        TlsIndex::make(0, 0),
        TlsIndex::make(1, 23),
        TlsIndex::make(2, 100),
        TlsIndex::make(3, OFFSET_LIMIT - 1),
    ];
    let mut ok = true;
    let mut i = 0;
    while i < cases.len() {
        match cases.get(i) {
            Some(t) => match t.pack() {
                Ok(v) => {
                    if TlsIndex::unpack(v) != Ok(*t) {
                        ok = false;
                    }
                }
                Err(_) => ok = false,
            },
            None => ok = false,
        }
        i += 1;
    }
    out[1] = ok;

    let g = Gdt::new();
    let mut same = true;
    let mut m = 0;
    while m < MODS {
        let (base, size) = match g.seg(m) {
            Some(v) => v,
            None => return out,
        };
        let probes = [0u32, size - 1];
        let mut p = 0;
        while p < probes.len() {
            match probes.get(p) {
                Some(o) => {
                    let ti = TlsIndex::make(m as u16, *o);
                    if access(ti, &g) != access_ld(ti, &g) || access(ti, &g) != Ok(base + *o as i32) {
                        same = false;
                    }
                }
                None => same = false,
            }
            p += 1;
        }
        m += 1;
    }
    out[2] = same;

    let reserved = TlsIndex::make(MODULE_RESERVED, 0).pack();
    let wide = TlsIndex::make(0, OFFSET_LIMIT).pack();
    let res = TlsIndex::unpack(0x0000_ffff_0000_0000);
    let eoff = access(TlsIndex::make(0, 64), &g);
    let emod = access(TlsIndex::make(9, 0), &g);
    let eie = access_ie(TlsIndex::make(0, 63), -64, &g);
    out[3] = reserved == Err(TG_EMOD)
        && wide == Err(TG_EW)
        && res == Err(TG_EMOD)
        && eoff == Err(TG_EOFF)
        && emod == Err(TG_EMOD)
        && eie == Err(TG_EIE);

    out
}

/// 十三项判据。
pub fn run_unx11308_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11308.tlsindex");
    let l = build_ledger();
    let g = Gdt::new();

    s.add(
        "TG1-十四格齐备",
        l.len() == SAMPLES && !l.is_empty() && l.sample(0).map(|x| x.op) == Some(OP_GD),
        "static",
    );

    s.add_dyn(
        "TG2-八允六拒分布正确",
        l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("ok={} err={}", l.ok_count(), l.err_count()),
    );

    s.add(
        "TG3-地址轨迹",
        l.addr(0) == Some(-128)
            && l.addr(1) == Some(-65)
            && l.addr(2) == Some(-33)
            && l.addr(3) == Some(0)
            && l.addr(4) == Some(79),
        "static",
    );

    s.add(
        "TG4-GD 与 LD 等价",
        {
            let mut same = true;
            let mut m = 0;
            while m < MODS {
                let mut o = 0;
                while o < 4 {
                    let ti = TlsIndex::make(m as u16, o);
                    let a = access(ti, &g);
                    let b = access_ld(ti, &g);
                    if a != b {
                        same = false;
                    }
                    o += 1;
                }
                m += 1;
            }
            same && l.sample(8).map(|x| x.gd) == l.sample(3).map(|x| x.gd)
        },
        "static",
    );

    s.add(
        "TG5-偏移边界",
        l.sample(5).map(|x| x.code) == Some(TG_EOFF)
            && access(TlsIndex::make(1, 31), &g) == Ok(-33)
            && access(TlsIndex::make(1, 32), &g) == Err(TG_EOFF),
        "static",
    );

    s.add(
        "TG6-模块越界与保留号",
        l.sample(6).map(|x| x.code) == Some(TG_EMOD)
            && l.sample(7).map(|x| x.code) == Some(TG_EMOD)
            && access(TlsIndex::make(MODULE_RESERVED, 0), &g) == Err(TG_EMOD),
        "static",
    );

    s.add(
        "TG7-编码宽度闸门",
        l.sample(9).map(|x| x.code) == Some(TG_EW)
            && TlsIndex::make(1, OFFSET_LIMIT).pack() == Err(TG_EW)
            && TlsIndex::unpack((1u64 << 32) | OFFSET_LIMIT as u64) == Err(TG_EW),
        "static",
    );

    s.add(
        "TG8-保留位闸门",
        l.sample(10).map(|x| x.code) == Some(TG_ERES)
            && TlsIndex::unpack(0x8000_0000_0000_0000) == Err(TG_ERES),
        "static",
    );

    s.add(
        "TG9-打包往返无损",
        l.sample(11).map(|x| x.addr) == Some(100)
            && TlsIndex::make(3, 0).pack().map(TlsIndex::unpack) == Ok(Ok(TlsIndex::make(3, 0)))
            && TlsIndex::make(2, OFFSET_LIMIT - 1).pack().map(TlsIndex::unpack)
                == Ok(Ok(TlsIndex::make(2, OFFSET_LIMIT - 1))),
        "static",
    );

    s.add(
        "TG10-IE 快路径一致",
        l.addr(12) == l.addr(1) && access_ie(TlsIndex::make(1, 0), -64, &g) == Ok(-64),
        "static",
    );

    s.add(
        "TG11-IE 偏移错配",
        l.sample(13).map(|x| x.code) == Some(TG_EIE)
            && access_ie(TlsIndex::make(0, 0), 0, &g) == Err(TG_EIE),
        "static",
    );

    s.add(
        "TG12-正向溢出拒绝",
        {
            let odd = Gdt::custom([-128i32, -64, 0, i32::MAX - 8], [64u32, 32, 48, 16]);
            access(TlsIndex::make(3, 9), &odd) == Err(TG_EOVER)
                && access(TlsIndex::make(3, 8), &odd) == Err(TG_EOFF)
        },
        "static",
    );

    let mut f = build_ledger();
    let rep = f.replay();
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = f.freeze() == Err(TG_EFROZEN);
    let inj = inject_four();
    s.add_dyn(
        "TG13-冻结重放与四路注入",
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
    fn pack_roundtrip() {
        let ti = TlsIndex::make(3, 1234);
        let v = ti.pack().expect("pack");
        assert_eq!(TlsIndex::unpack(v), Ok(ti));
        assert_eq!(v, (3u64 << 32) | 1234);
    }

    #[test]
    fn pack_rejects_reserved_module_and_wide_offset() {
        assert_eq!(TlsIndex::make(MODULE_RESERVED, 0).pack(), Err(TG_EMOD));
        assert_eq!(TlsIndex::make(0, OFFSET_LIMIT).pack(), Err(TG_EW));
        assert!(TlsIndex::make(0, OFFSET_LIMIT - 1).pack().is_ok());
    }

    #[test]
    fn unpack_rejects_high_bits() {
        assert_eq!(TlsIndex::unpack(0xffff_0000_0000_0000), Err(TG_ERES));
        assert_eq!(TlsIndex::unpack((1u64 << 32) | OFFSET_LIMIT as u64), Err(TG_EW));
    }

    #[test]
    fn gd_and_ld_agree_everywhere() {
        let g = Gdt::new();
        let mut m = 0;
        while m < MODS {
            let (base, size) = g.seg(m).expect("seg");
            let mut o = 0;
            while o < size {
                let ti = TlsIndex::make(m as u16, o);
                assert_eq!(access(ti, &g), Ok(base + o as i32));
                assert_eq!(access_ld(ti, &g), access(ti, &g));
                o += 1;
            }
            m += 1;
        }
    }

    #[test]
    fn ie_must_match_gd() {
        let g = Gdt::new();
        assert_eq!(access_ie(TlsIndex::make(0, 63), -65, &g), Ok(-65));
        assert_eq!(access_ie(TlsIndex::make(0, 63), -64, &g), Err(TG_EIE));
    }

    #[test]
    fn overflow_is_refused() {
        let odd = Gdt::custom([-128, -64, 0, i32::MAX - 8], [64, 32, 48, 16]);
        assert_eq!(access(TlsIndex::make(3, 9), &odd), Err(TG_EOVER));
        assert_eq!(access(TlsIndex::make(3, 16), &odd), Err(TG_EOFF));
    }

    #[test]
    fn ledger_seals_and_replays() {
        let mut l = build_ledger();
        assert!(l.audit().is_ok());
        let root = l.freeze().expect("freeze");
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.freeze(), Err(TG_EFROZEN));
    }

    #[test]
    fn checks_pass() {
        let s = run_unx11308_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
