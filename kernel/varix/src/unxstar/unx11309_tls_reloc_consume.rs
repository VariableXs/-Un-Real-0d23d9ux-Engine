//! UNX-F11309 GD/LD 重定位消费（0xe940 段）
//!
//! 链接器把 GD/LD 访问序列的语义压缩成四种重定位：`DTPMOD64` 给出模块号，
//! `DTPOFF64` 给出块内偏移，`TPOFF64` 给出已写死的 TP 相对偏移，`TLSGD` 给出
//! 进入 `__tls_get_addr` 前的票据。动态装载器消费这些重定位时只做一件事：
//! 把符号表里的一条已定义符号加上加数，按种类投影成最终值。
//!
//! 消费顺序因此固定为五道闸门：种类未知、符号未定义、模块号越界、加数
//! 溢出与投影越界。闸门有优先级——种类错的条目即使符号也未定义，仍报种类错，
//! 因为装载器先按重定位类型分派，再查符号。模型把它做成单入口 `consume`，
//! 每个样本只走一条路径，拒绝样本只记录第一个命中的闸门。
//!
//! 三条不变量支撑账本：`DTPMOD64` 的值恒等于符号的模块号；`DTPOFF64` 与
//! `TPOFF64` 在同一符号同一加数下满足 `tpoff == base[module] + dtpoff`；
//! `TLSGD` 的票据必须能被解包回原符号。账本十四格，八格成功覆盖四种
//! 重定位与跨模块取值，六格拒绝分别命中未定义、类型、偏移、加数、区间与
//! 模块号六道闸门。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xe940;

/// 编号非法。
pub const RL_EID: u32 = SEG_BASE + 0x01;
/// 登记顺序错误。
pub const RL_EORDER: u32 = SEG_BASE + 0x02;
/// 重复登记。
pub const RL_EDUP: u32 = SEG_BASE + 0x03;
/// 已冻结。
pub const RL_EFROZEN: u32 = SEG_BASE + 0x04;
/// 观测不存在的格。
pub const RL_ENOBS: u32 = SEG_BASE + 0x05;
/// 结论不符。
pub const RL_EMISMATCH: u32 = SEG_BASE + 0x06;
/// 账本失衡。
pub const RL_EBAL: u32 = SEG_BASE + 0x07;
/// 未定义符号。
pub const RL_EUNDEF: u32 = SEG_BASE + 0x08;
/// 重定位类型未知。
pub const RL_ETYPE: u32 = SEG_BASE + 0x09;
/// 块内偏移越界。
pub const RL_EOFF: u32 = SEG_BASE + 0x0a;
/// 加数算术溢出。
pub const RL_EADD: u32 = SEG_BASE + 0x0b;
/// TP 相对地址越界。
pub const RL_ERANGE: u32 = SEG_BASE + 0x0c;
/// 模块号越界。
pub const RL_EMOD: u32 = SEG_BASE + 0x0d;
/// 票据字段越宽。
pub const RL_EW: u32 = SEG_BASE + 0x0e;
/// 票号不一致。
pub const RL_ESEAL: u32 = SEG_BASE + 0x0f;

/// 判决：成功。
pub const V_OK: u32 = 0;

/// `R_X86_64_DTPMOD64`：模块号。
pub const K_DTPMOD: u8 = 16;
/// `R_X86_64_DTPOFF64`：块内偏移。
pub const K_DTPOFF: u8 = 17;
/// `R_X86_64_TPOFF64`：TP 相对偏移。
pub const K_TPOFF: u8 = 18;
/// `R_X86_64_TLSGD`：通用票据。
pub const K_TLSGD: u8 = 19;

/// 模块数。
pub const MODS: usize = 4;
/// 票据偏移宽度。
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

/// 链接器宣告的符号：所属模块、块内偏移、是否已定义。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sym {
    pub module: u16,
    pub offset: u32,
    pub defined: bool,
}

impl Sym {
    /// 已定义符号。
    pub const fn def(module: u16, offset: u32) -> Sym {
        Sym {
            module,
            offset,
            defined: true,
        }
    }

    /// 未定义符号。
    pub const fn undef(module: u16, offset: u32) -> Sym {
        Sym {
            module,
            offset,
            defined: false,
        }
    }
}

/// 模块段表：每个模块在 TP 空间里的基址与容量。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Segs {
    base: [i32; MODS],
    size: [u32; MODS],
}

impl Segs {
    /// 标准表：模块 0 -128/64、模块 1 -64/32、模块 2 0/48、模块 3 64/16。
    pub const fn new() -> Segs {
        Segs {
            base: [-128, -64, 0, 64],
            size: [64, 32, 48, 16],
        }
    }

    /// 自定义表。
    pub const fn custom(base: [i32; MODS], size: [u32; MODS]) -> Segs {
        Segs { base, size }
    }

    /// 取模块段。
    pub fn seg(&self, m: usize) -> Option<(i32, u32)> {
        match (self.base.get(m), self.size.get(m)) {
            (Some(b), Some(s)) => Some((*b, *s)),
            _ => None,
        }
    }

    /// 模块数。
    pub fn len(&self) -> usize {
        MODS
    }
}

/// 票据入口：把符号偏移压进 24 位，超出即拒绝。
pub fn ticket(sym: Sym) -> Result<u64, u32> {
    if !sym.defined {
        return Err(RL_EUNDEF);
    }
    if (sym.module as usize) >= MODS || sym.module == MODULE_RESERVED {
        return Err(RL_EMOD);
    }
    if sym.offset >= OFFSET_LIMIT {
        return Err(RL_EW);
    }
    Ok(((sym.module as u64) << 32) | sym.offset as u64)
}

/// 票据解包，只接受合法字段。
pub fn unticket(v: u64) -> Result<Sym, u32> {
    if (v >> 48) != 0 {
        return Err(RL_EW);
    }
    let module = ((v >> 32) & 0xffff) as u16;
    let offset = (v & 0xffff_ffff) as u32;
    if module == MODULE_RESERVED || (module as usize) >= MODS {
        return Err(RL_EMOD);
    }
    if offset >= OFFSET_LIMIT {
        return Err(RL_EW);
    }
    Ok(Sym::def(module, offset))
}

/// 单入口：按种类投影一条重定位。
///
/// 闸门优先级：类型 → 定义 → 模块 → 加数 → 投影区间。
pub fn consume(kind: u8, sym: Sym, addend: i64, segs: &Segs) -> Result<u64, u32> {
    if kind != K_DTPMOD && kind != K_DTPOFF && kind != K_TPOFF && kind != K_TLSGD {
        return Err(RL_ETYPE);
    }
    if !sym.defined {
        return Err(RL_EUNDEF);
    }
    let (base, size) = match segs.seg(sym.module as usize) {
        Some(v) => v,
        None => return Err(RL_EMOD),
    };
    if kind == K_DTPMOD {
        return Ok(sym.module as u64);
    }
    if kind == K_TLSGD {
        return ticket(sym);
    }
    let off = match (sym.offset as i64).checked_add(addend) {
        Some(v) => v,
        None => return Err(RL_EADD),
    };
    if off < 0 || off >= size as i64 {
        return Err(RL_EOFF);
    }
    if kind == K_DTPOFF {
        return Ok(off as u64);
    }
    let addr = match base.checked_add(off as i32) {
        Some(v) => v,
        None => return Err(RL_EADD),
    };
    if addr < -TP_RANGE || addr >= TP_RANGE {
        return Err(RL_ERANGE);
    }
    Ok(addr as u32 as u64)
}

/// 一个账本样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    pub slot: u8,
    pub kind: u8,
    pub ok: bool,
    pub code: u32,
    pub module: u16,
    pub offset: u32,
    pub addend: i64,
    pub value: u64,
}

impl Sample {
    /// 空白格。
    pub const fn blank() -> Sample {
        Sample {
            slot: 0,
            kind: 0,
            ok: false,
            code: 0,
            module: 0,
            offset: 0,
            addend: 0,
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
            return Err(RL_EFROZEN);
        }
        if (row.slot as usize) != self.len {
            return Err(RL_EORDER);
        }
        if self.len >= SAMPLES {
            return Err(RL_EID);
        }
        let mut i = 0;
        while i < self.len {
            match self.rows.get(i) {
                Some(x) => {
                    if x.slot == row.slot {
                        return Err(RL_EDUP);
                    }
                }
                None => return Err(RL_ENOBS),
            }
            i += 1;
        }
        match self.rows.get_mut(self.len) {
            Some(x) => *x = row,
            None => return Err(RL_EID),
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

    /// 对账：格数、分布、地址轨迹、种类覆盖与拒绝码序列。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES {
            return Err(RL_EBAL);
        }
        if self.ok_count() != 8 || self.err_count() != 6 {
            return Err(RL_EBAL);
        }
        let mut kinds = [0u8; 4];
        let mut i = 0;
        while i < self.len {
            match self.rows.get(i) {
                Some(s) => {
                    if s.slot as usize != i {
                        return Err(RL_EORDER);
                    }
                    if s.ok {
                        if s.code != V_OK {
                            return Err(RL_EMISMATCH);
                        }
                        match s.kind {
                            K_DTPMOD => {
                                if let Some(k) = kinds.get_mut(0) {
                                    *k = k.saturating_add(1);
                                }
                            }
                            K_DTPOFF => {
                                if let Some(k) = kinds.get_mut(1) {
                                    *k = k.saturating_add(1);
                                }
                            }
                            K_TPOFF => {
                                if let Some(k) = kinds.get_mut(2) {
                                    *k = k.saturating_add(1);
                                }
                            }
                            K_TLSGD => {
                                if let Some(k) = kinds.get_mut(3) {
                                    *k = k.saturating_add(1);
                                }
                            }
                            _ => return Err(RL_ETYPE),
                        }
                    } else if s.code == V_OK || s.value != 0 {
                        return Err(RL_EMISMATCH);
                    }
                }
                None => return Err(RL_ENOBS),
            }
            i += 1;
        }
        if kinds != [2u8, 3, 2, 1] {
            return Err(RL_EMISMATCH);
        }
        let vals = [
            0u64,
            2,
            0,
            24,
            32,
            (-128i32 as u32) as u64,
            79,
            0x0000_0002_0000_0064,
        ];
        let mut k = 0;
        while k < vals.len() {
            match vals.get(k) {
                Some(w) => {
                    if self.value(k) != Some(*w) {
                        return Err(RL_EMISMATCH);
                    }
                }
                None => return Err(RL_EBAL),
            }
            k += 1;
        }
        let codes = [RL_EUNDEF, RL_ETYPE, RL_EOFF, RL_EADD, RL_ERANGE, RL_EMOD];
        let mut c = 0;
        let mut j = 0;
        while j < self.len {
            if let Some(s) = self.rows.get(j) {
                if !s.ok {
                    match codes.get(c) {
                        Some(w) => {
                            if s.code != *w {
                                return Err(RL_EMISMATCH);
                            }
                        }
                        None => return Err(RL_EBAL),
                    }
                    c += 1;
                }
            }
            j += 1;
        }
        if c != codes.len() {
            return Err(RL_EBAL);
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
                h = fold(h, s.kind as u64);
                h = fold(h, s.ok as u64);
                h = fold(h, s.code as u64);
                h = fold(h, s.module as u64);
                h = fold(h, s.offset as u64);
                h = fold(h, s.addend as u64);
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
            return Err(RL_EFROZEN);
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
            return Err(RL_EBAL);
        }
        let h = fresh.seal();
        if self.frozen && h != self.seal {
            return Err(RL_ESEAL);
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

/// 把一次消费的结果登记成一格。
fn row(slot: u8, kind: u8, sym: Sym, addend: i64, r: Result<u64, u32>) -> Sample {
    match r {
        Ok(v) => Sample {
            slot,
            kind,
            ok: true,
            code: V_OK,
            module: sym.module,
            offset: sym.offset,
            addend,
            value: v,
        },
        Err(c) => Sample {
            slot,
            kind,
            ok: false,
            code: c,
            module: sym.module,
            offset: sym.offset,
            addend,
            value: 0,
        },
    }
}

/// 构建十四格账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let segs = Segs::new();
    let odd = Segs::custom([-128, -64, 0, i32::MAX - 4], [64, 32, 48, 16]);

    let plan: [(u8, Sym, i64, &Segs); SAMPLES] = [
        (K_DTPMOD, Sym::def(0, 0), 0, &segs),
        (K_DTPMOD, Sym::def(2, 100), 0, &segs),
        (K_DTPOFF, Sym::def(0, 0), 0, &segs),
        (K_DTPOFF, Sym::def(1, 24), 0, &segs),
        (K_DTPOFF, Sym::def(2, 46), -14, &segs),
        (K_TPOFF, Sym::def(0, 0), 0, &segs),
        (K_TPOFF, Sym::def(3, 15), 0, &segs),
        (K_TLSGD, Sym::def(2, 100), 0, &segs),
        (K_DTPMOD, Sym::undef(1, 8), 0, &segs),
        (99, Sym::def(0, 0), 0, &segs),
        (K_DTPOFF, Sym::def(1, 31), 1, &segs),
        (K_DTPOFF, Sym::def(1, 1), i64::MAX, &segs),
        (K_TPOFF, Sym::def(3, 0), 0, &odd),
        (K_DTPMOD, Sym::def(9, 0), 0, &segs),
    ];

    let mut i = 0;
    while i < plan.len() {
        if let Some((kind, sym, addend, segs_ref)) = plan.get(i) {
            let s = row(i as u8, *kind, *sym, *addend, consume(*kind, *sym, *addend, segs_ref));
            let _ = l.register(s);
        }
        i += 1;
    }
    l
}

/// 四路注入：账本重放、模块号投影、TP 恒等式、闸门优先级。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let base = l.len() == SAMPLES && l.audit().is_ok();
    let rep = l.replay();
    let root = l.seal();
    out[0] = base && rep == Ok(root) && root != 0;

    let segs = Segs::new();
    let mut same = true;
    let mut m = 0;
    while m < MODS {
        let sym = Sym::def(m as u16, 3);
        if consume(K_DTPMOD, sym, 0, &segs) != Ok(m as u64) {
            same = false;
        }
        m += 1;
    }
    out[1] = same;

    let mut ident = true;
    let mut m2 = 0;
    while m2 < MODS {
        let (b, size) = match segs.seg(m2) {
            Some(v) => v,
            None => return out,
        };
        let mut add = 0i64;
        while add < size as i64 {
            let sym = Sym::def(m2 as u16, 0);
            if consume(K_TPOFF, sym, add, &segs) != Ok((b as i64 + add) as u32 as u64)
                || consume(K_DTPOFF, sym, add, &segs) != Ok(add as u64)
            {
                ident = false;
            }
            add += 7;
        }
        m2 += 1;
    }
    out[2] = ident;

    let undef = consume(K_DTPMOD, Sym::undef(0, 0), 0, &segs);
    let bad_kind = consume(7, Sym::def(0, 0), 0, &segs);
    let kind_first = consume(7, Sym::undef(0, 0), 0, &segs);
    let eoff = consume(K_DTPOFF, Sym::def(0, 64), 0, &segs);
    let erange = consume(K_TPOFF, Sym::def(3, 0), 0, &odd_gdt());
    let emod = consume(K_DTPMOD, Sym::def(MODULE_RESERVED, 0), 0, &segs);
    let wide = consume(K_TLSGD, Sym::def(0, OFFSET_LIMIT), 0, &segs);
    out[3] = undef == Err(RL_EUNDEF)
        && bad_kind == Err(RL_ETYPE)
        && kind_first == Err(RL_ETYPE)
        && eoff == Err(RL_EOFF)
        && erange == Err(RL_ERANGE)
        && emod == Err(RL_EMOD)
        && wide == Err(RL_EW)
        && ticket(Sym::def(2, 100)).map(unticket) == Ok(Ok(Sym::def(2, 100)));

    out
}

/// 溢出表：模块 3 的基址贴着 i32 上限。
pub fn odd_gdt() -> Segs {
    Segs::custom([-128, -64, 0, i32::MAX - 4], [64, 32, 48, 16])
}

/// 十三项判据。
pub fn run_unx11309_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11309.tlsreloc");
    let l = build_ledger();
    let segs = Segs::new();

    s.add(
        "RL1-十四格齐备",
        l.len() == SAMPLES && !l.is_empty() && l.sample(0).map(|x| x.kind) == Some(K_DTPMOD),
        "static",
    );

    s.add_dyn(
        "RL2-八允六拒分布正确",
        l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("ok={} err={}", l.ok_count(), l.err_count()),
    );

    s.add(
        "RL3-模块号投影",
        l.value(0) == Some(0)
            && l.value(1) == Some(2)
            && consume(K_DTPMOD, Sym::def(3, 0), 0, &segs) == Ok(3)
            && consume(K_DTPMOD, Sym::def(9, 0), 0, &segs) == Err(RL_EMOD)
            && consume(K_DTPMOD, Sym::def(MODULE_RESERVED, 0), 0, &segs) == Err(RL_EMOD),
        "static",
    );

    s.add(
        "RL4-块内偏移投影",
        l.value(2) == Some(0) && l.value(3) == Some(24) && l.value(4) == Some(32),
        "static",
    );

    s.add(
        "RL5-TP 偏移投影",
        l.value(5) == Some((-128i32 as u32) as u64) && l.value(6) == Some(79),
        "static",
    );

    s.add(
        "RL6-票据投影与解包",
        l.value(7) == Some(0x0000_0002_0000_0064)
            && unticket(0x0000_0002_0000_0064) == Ok(Sym::def(2, 100))
            && ticket(Sym::def(1, OFFSET_LIMIT - 1)).is_ok()
            && ticket(Sym::def(1, OFFSET_LIMIT)) == Err(RL_EW),
        "static",
    );

    s.add(
        "RL7-未定义闸门",
        l.sample(8).map(|x| x.code) == Some(RL_EUNDEF)
            && consume(K_TPOFF, Sym::undef(0, 0), 0, &segs) == Err(RL_EUNDEF),
        "static",
    );

    s.add(
        "RL8-类型闸门优先于定义",
        l.sample(9).map(|x| x.code) == Some(RL_ETYPE)
            && consume(0, Sym::def(0, 0), 0, &segs) == Err(RL_ETYPE)
            && consume(0, Sym::undef(0, 0), 0, &segs) == Err(RL_ETYPE),
        "static",
    );

    s.add(
        "RL9-偏移越界闸门",
        l.sample(10).map(|x| x.code) == Some(RL_EOFF)
            && consume(K_DTPOFF, Sym::def(0, 63), 1, &segs) == Err(RL_EOFF)
            && consume(K_DTPOFF, Sym::def(0, 0), -1, &segs) == Err(RL_EOFF),
        "static",
    );

    s.add(
        "RL10-加数溢出闸门",
        l.sample(11).map(|x| x.code) == Some(RL_EADD)
            && consume(K_DTPOFF, Sym::def(0, 1), i64::MAX, &segs) == Err(RL_EADD),
        "static",
    );

    s.add(
        "RL11-区间闸门",
        l.sample(12).map(|x| x.code) == Some(RL_ERANGE)
            && consume(K_TPOFF, Sym::def(2, 4090), 0, &segs) == Err(RL_EOFF)
            && consume(K_TPOFF, Sym::def(2, 0), 0, &segs) == Ok(0),
        "static",
    );

    s.add(
        "RL12-TP 恒等式全网格",
        {
            let mut ident = true;
            let mut m = 0;
            while m < MODS {
                if let Some((b, size)) = segs.seg(m) {
                    let mut add = 0i64;
                    while add < size as i64 {
                        let sym = Sym::def(m as u16, 0);
                        let tp = consume(K_TPOFF, sym, add, &segs);
                        let dt = consume(K_DTPOFF, sym, add, &segs);
                        if tp != Ok((b as i64 + add) as u32 as u64) || dt != Ok(add as u64) {
                            ident = false;
                        }
                        add += 1;
                    }
                } else {
                    ident = false;
                }
                m += 1;
            }
            ident && l.sample(13).map(|x| x.code) == Some(RL_EMOD)
        },
        "static",
    );

    let mut f = build_ledger();
    let rep = f.replay();
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = f.freeze() == Err(RL_EFROZEN);
    let inj = inject_four();
    s.add_dyn(
        "RL13-冻结重放与四路注入",
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
    fn dtpmod_tracks_module() {
        let segs = Segs::new();
        let mut m = 0;
        while m < MODS {
            assert_eq!(
                consume(K_DTPMOD, Sym::def(m as u16, 7), 0, &segs),
                Ok(m as u64)
            );
            m += 1;
        }
    }

    #[test]
    fn dtpoff_addend_checked() {
        let segs = Segs::new();
        assert_eq!(consume(K_DTPOFF, Sym::def(2, 46), -14, &segs), Ok(32));
        assert_eq!(consume(K_DTPOFF, Sym::def(2, 46), -47, &segs), Err(RL_EOFF));
        assert_eq!(consume(K_DTPOFF, Sym::def(0, 1), i64::MAX, &segs), Err(RL_EADD));
    }

    #[test]
    fn tpoff_is_base_plus_dtpoff() {
        let segs = Segs::new();
        let mut m = 0;
        while m < MODS {
            let (b, size) = segs.seg(m).expect("seg");
            let mut o = 0u32;
            while o < size {
                let sym = Sym::def(m as u16, o);
                assert_eq!(consume(K_TPOFF, sym, 0, &segs), Ok((b + o as i32) as u32 as u64));
                o += 1;
            }
            m += 1;
        }
    }

    #[test]
    fn gates_have_priority() {
        let segs = Segs::new();
        assert_eq!(consume(5, Sym::undef(0, 0), 0, &segs), Err(RL_ETYPE));
        assert_eq!(consume(K_DTPMOD, Sym::undef(0, 0), 0, &segs), Err(RL_EUNDEF));
        assert_eq!(
            consume(K_DTPMOD, Sym::undef(MODULE_RESERVED, 0), 0, &segs),
            Err(RL_EUNDEF)
        );
    }

    #[test]
    fn ticket_roundtrip_and_width() {
        let t = ticket(Sym::def(3, 1234)).expect("ticket");
        assert_eq!(unticket(t), Ok(Sym::def(3, 1234)));
        assert_eq!(ticket(Sym::def(0, OFFSET_LIMIT)), Err(RL_EW));
        assert_eq!(unticket(0x0001_0000_0000_0000), Err(RL_EW));
    }

    #[test]
    fn range_gate_uses_custom_table() {
        let odd = odd_gdt();
        assert_eq!(consume(K_TPOFF, Sym::def(3, 0), 0, &odd), Err(RL_ERANGE));
        assert_eq!(consume(K_TPOFF, Sym::def(2, 4090), 0, &Segs::new()), Err(RL_EOFF));
    }

    #[test]
    fn ledger_seals_and_replays() {
        let mut l = build_ledger();
        assert!(l.audit().is_ok());
        let root = l.freeze().expect("freeze");
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.freeze(), Err(RL_EFROZEN));
        assert_eq!(l.rejects(), 1);
    }

    #[test]
    fn checks_pass() {
        let s = run_unx11309_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
