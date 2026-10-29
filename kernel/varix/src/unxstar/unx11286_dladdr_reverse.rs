//! UNX-F11286 dladdr 反查语义（0x1f560 段）
//!
//! dladdr 把一枚运行时地址反查成三元组（库、符号、偏移）。Linux 语义有三层：
//! 第一层找人——地址必须落在某个对象的装载区间 [bias, bias+span) 内，
//! 落在缝隙、区间末端或零地址都返回 0，不猜相邻库；
//! 第二层找符号——在同一对象内取"起点不超过地址且覆盖该地址"的符号里
//! 起点最大的那个，符号区间左闭右开，恰好等于符号起点偏移为 0，
//! 恰好等于符号终点则不属于该符号；
//! 第三层兜底——对象命中但没有符号覆盖时，仍返回库名与相对基址偏移，
//! 符号名置空、符号地址置零，这正是 glibc 对"库内未知地址"的回答。
//! 本段固化四个对象（主程序、libc、libm、libssl）与八个符号，
//! 其中 malloc/free 首尾相接，用来压左闭右开的边界。
//! 账本记命中数、落空数与结果摘要；replay 时逐地址重查、重算摘要。
//! 拒绝面只有 id 越界、顺序倒退、同 id 重复、满表、封册后写账；
//! 未知旗标、零地址、记录库号/符号号/偏移错都是样本，进规则产码不拒收。

use crate::checks::CheckSet;

/// 诊断段（0x1f560 独占 0x10）。
pub const SEG_BASE: u32 = 0x1f560;

/// 条目 id 越界。
pub const DA_EID: u32 = SEG_BASE + 0x01;
/// 登记序倒退。
pub const DA_EORDER: u32 = SEG_BASE + 0x02;
/// 同 id 重复登记。
pub const DA_EDUP: u32 = SEG_BASE + 0x03;
/// 条目表已满。
pub const DA_EFULL: u32 = SEG_BASE + 0x04;
/// 封册后写账。
pub const DA_EFROZEN: u32 = SEG_BASE + 0x05;
/// 观测了未登记的 id。
pub const DA_EUNSEEN: u32 = SEG_BASE + 0x06;
/// 复算判据与登记时不符。
pub const DA_EMISMATCH: u32 = SEG_BASE + 0x07;
/// 账未观测满。
pub const DA_EUNOBS: u32 = SEG_BASE + 0x08;
/// 同一条目重复观测。
pub const DA_EDOBS: u32 = SEG_BASE + 0x09;
/// 摘要/直方/计数不平。
pub const DA_EBALANCE: u32 = SEG_BASE + 0x0a;

/// 判据码：未知旗标位。
pub const DV_FLAG: u32 = 1;
/// 判据码：零地址边界。
pub const DV_ADDR: u32 = 2;
/// 判据码：记录库号错或命中态不符。
pub const DV_LIB: u32 = 3;
/// 判据码：记录符号号错或命中态不符。
pub const DV_SYM: u32 = 4;
/// 判据码：记录偏移不符。
pub const DV_OFF: u32 = 5;

/// 允许的旗标位：仅"保留符号名"提示位。
pub const F_KEEP: u32 = 0x01;
/// 已知旗标掩码。
pub const KNOWN_FLAGS: u32 = F_KEEP;
/// "无此对象/无此符号"哨兵。
pub const NONE: u32 = 0xff;
/// 对象数。
pub const LIB_COUNT: usize = 4;
/// 符号数。
pub const SYM_COUNT: usize = 8;
/// 样本条数。
pub const SAMPLES: usize = 13;
/// 条目表定容。
pub const CAP: usize = 16;
/// id 合法域。
pub const ID_MIN: u32 = 1;
pub const ID_MAX: u32 = 4096;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;
const TAG: u64 = 0xb05_86;

/// 装载对象：路径、基址、区间长度（左闭右开）。
#[derive(Clone, Copy, Debug)]
pub struct Obj {
    pub name: &'static str,
    pub bias: u64,
    pub span: u64,
}

/// 符号：名字、所属对象、绝对起点、长度（左闭右开）。
#[derive(Clone, Copy, Debug)]
pub struct Sym {
    pub name: &'static str,
    pub lib: u32,
    pub value: u64,
    pub size: u64,
}

/// 四个对象：主程序在低址，三个库依次上排，libm 与 libssl 之间留缝。
pub const OBJS: [Obj; LIB_COUNT] = [
    Obj { name: "/usr/bin/prog", bias: 0x0040_0000, span: 0x4000 },
    Obj { name: "/lib/x86_64-linux-gnu/libc.so.6", bias: 0x7f10_0000_0000, span: 0x20_0000 },
    Obj { name: "/lib/x86_64-linux-gnu/libm.so.6", bias: 0x7f10_0020_0000, span: 0x10_0000 },
    Obj { name: "/lib/x86_64-linux-gnu/libssl.so.3", bias: 0x7f10_0040_0000, span: 0x8_0000 },
];

/// 八个符号：malloc/free 首尾相接，free 与 memcpy 之间留空。
pub const SYMS: [Sym; SYM_COUNT] = [
    Sym { name: "main", lib: 0, value: 0x0040_1000, size: 0x200 },
    Sym { name: "malloc", lib: 1, value: 0x7f10_0000_1000, size: 0x400 },
    Sym { name: "free", lib: 1, value: 0x7f10_0000_1400, size: 0x100 },
    Sym { name: "memcpy", lib: 1, value: 0x7f10_0000_1800, size: 0x800 },
    Sym { name: "sin", lib: 2, value: 0x7f10_0020_0800, size: 0x100 },
    Sym { name: "cos", lib: 2, value: 0x7f10_0020_0900, size: 0x100 },
    Sym { name: "SSL_new", lib: 3, value: 0x7f10_0040_0200, size: 0x180 },
    Sym { name: "SSL_free", lib: 3, value: 0x7f10_0040_0400, size: 0x100 },
];

/// 反查结果：命中与否、库号、符号号（NONE 表示无符号）、相对偏移。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lookup {
    pub found: bool,
    pub lib: u32,
    pub sym: u32,
    pub off: u64,
}

impl Lookup {
    pub const fn miss() -> Lookup {
        Lookup { found: false, lib: NONE, sym: NONE, off: 0 }
    }
}

/// 反查：先找对象，再在该对象里取覆盖地址且起点最大的符号。
pub fn lookup(addr: u64) -> Lookup {
    let mut oi = NONE;
    let mut k = 0usize;
    while k < LIB_COUNT {
        if let Some(o) = OBJS.get(k) {
            if addr >= o.bias && addr < o.bias.wrapping_add(o.span) {
                oi = k as u32;
            }
        }
        k = k.wrapping_add(1);
    }
    if oi == NONE {
        return Lookup::miss();
    }
    let obj = match OBJS.get(oi as usize) {
        Some(v) => *v,
        None => return Lookup::miss(),
    };
    let mut best = NONE;
    let mut best_v = 0u64;
    k = 0;
    while k < SYM_COUNT {
        if let Some(s) = SYMS.get(k) {
            if s.lib == oi
                && addr >= s.value
                && addr < s.value.wrapping_add(s.size)
                && (best == NONE || s.value >= best_v)
            {
                best = k as u32;
                best_v = s.value;
            }
        }
        k = k.wrapping_add(1);
    }
    if best == NONE {
        Lookup { found: true, lib: oi, sym: NONE, off: addr.wrapping_sub(obj.bias) }
    } else {
        let sv = match SYMS.get(best as usize) {
            Some(v) => v.value,
            None => 0,
        };
        Lookup { found: true, lib: oi, sym: best, off: addr.wrapping_sub(sv) }
    }
}

fn hash_bytes(h0: u64, s: &str) -> u64 {
    let bytes = s.as_bytes();
    let mut h = h0;
    let mut b = 0usize;
    while b < bytes.len() {
        if let Some(x) = bytes.get(b) {
            h = (h ^ *x as u64).wrapping_mul(FNV_PRIME);
        }
        b = b.wrapping_add(1);
    }
    h
}

/// 结果摘要：库名、符号名、库号、符号号、偏移按固定序折叠。
pub fn result_digest(r: &Lookup) -> u64 {
    let mut h = FNV_OFF ^ TAG;
    let lib_name = match OBJS.get(r.lib as usize) {
        Some(o) => o.name,
        None => "",
    };
    h = hash_bytes(h, lib_name);
    let sym_name = match SYMS.get(r.sym as usize) {
        Some(s) => s.name,
        None => "",
    };
    h = hash_bytes(h, sym_name);
    h = (h ^ r.lib as u64).wrapping_mul(FNV_PRIME);
    h = (h ^ r.sym as u64).wrapping_mul(FNV_PRIME);
    h = (h ^ r.off).wrapping_mul(FNV_PRIME);
    h = (h ^ if r.found { 1 } else { 0 }).wrapping_mul(FNV_PRIME);
    h
}

/// 一条反查样本：地址、旗标、记录库号/符号号/偏移、指纹。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DlCase {
    pub id: u32,
    pub addr: u64,
    pub flags: u32,
    pub want_lib: u32,
    pub want_sym: u32,
    pub off: u64,
    pub hash: u64,
}

impl DlCase {
    pub const fn blank() -> DlCase {
        DlCase { id: 0, addr: 0, flags: 0, want_lib: 0, want_sym: 0, off: 0, hash: 0 }
    }

    pub fn lib_name(&self) -> Option<&'static str> {
        OBJS.get(self.want_lib as usize).map(|o| o.name)
    }

    pub fn sym_name(&self) -> Option<&'static str> {
        SYMS.get(self.want_sym as usize).map(|s| s.name)
    }
}

/// 纯判据：旗标 → 零地址 → 记录号域 → 实查三元组逐项比对。
pub fn addr_verdict(c: &DlCase) -> u32 {
    if c.flags & !KNOWN_FLAGS != 0 {
        return DV_FLAG;
    }
    if c.addr == 0 {
        return DV_ADDR;
    }
    if c.want_lib != NONE && c.want_lib >= LIB_COUNT as u32 {
        return DV_LIB;
    }
    if c.want_sym != NONE && c.want_sym >= SYM_COUNT as u32 {
        return DV_SYM;
    }
    let r = lookup(c.addr);
    if !r.found {
        if c.want_lib != NONE {
            return DV_LIB;
        }
        if c.want_sym != NONE {
            return DV_SYM;
        }
        if c.off != 0 {
            return DV_OFF;
        }
        return 0;
    }
    if c.want_lib != r.lib {
        return DV_LIB;
    }
    if c.want_sym != r.sym {
        return DV_SYM;
    }
    if c.off != r.off {
        return DV_OFF;
    }
    0
}

/// 反查账本：样本 + 判据 + 六槽直方 + 命中/落空数 + 结果摘要。
#[derive(Clone, Copy, Debug)]
pub struct DlLedger {
    cases: [DlCase; CAP],
    verdicts: [u32; CAP],
    seen: [bool; CAP],
    hist: [u32; 6],
    found_count: u32,
    miss_count: u32,
    addr_digest: u64,
    len: usize,
    attempts: u32,
    accepts: u32,
    rejects: u32,
    observed: u32,
    matched: u32,
    frozen: bool,
    root: Option<u64>,
}

impl DlLedger {
    pub const fn new() -> DlLedger {
        DlLedger {
            cases: [DlCase::blank(); CAP],
            verdicts: [0u32; CAP],
            seen: [false; CAP],
            hist: [0u32; 6],
            found_count: 0,
            miss_count: 0,
            addr_digest: FNV_OFF ^ TAG,
            len: 0,
            attempts: 0,
            accepts: 0,
            rejects: 0,
            observed: 0,
            matched: 0,
            frozen: false,
            root: None,
        }
    }

    fn fail(&mut self, code: u32) -> u32 {
        self.rejects = self.rejects.wrapping_add(1);
        code
    }

    /// 登记：封册 → id 域 → 重复 → 顺序 → 定容。
    pub fn register(&mut self, c: DlCase) -> Result<usize, u32> {
        self.attempts = self.attempts.wrapping_add(1);
        if self.frozen {
            return Err(self.fail(DA_EFROZEN));
        }
        if c.id < ID_MIN || c.id > ID_MAX {
            return Err(self.fail(DA_EID));
        }
        let mut k = 0usize;
        while k < self.len {
            if let Some(x) = self.cases.get(k) {
                if x.id == c.id {
                    return Err(self.fail(DA_EDUP));
                }
            }
            k = k.wrapping_add(1);
        }
        if self.len > 0 {
            if let Some(last) = self.cases.get(self.len - 1) {
                if c.id <= last.id {
                    return Err(self.fail(DA_EORDER));
                }
            }
        }
        if self.len >= CAP {
            return Err(self.fail(DA_EFULL));
        }
        if let Some(slot) = self.cases.get_mut(self.len) {
            *slot = c;
        }
        self.len = self.len.wrapping_add(1);
        self.accepts = self.accepts.wrapping_add(1);
        Ok(self.len - 1)
    }

    /// 登记 + 判据：零判据样本再反查一次，攒命中/落空与结果摘要。
    pub fn register_ruled(&mut self, c: DlCase) -> Result<(usize, u32), u32> {
        let idx = self.register(c)?;
        let v = addr_verdict(&c);
        if let Some(slot) = self.verdicts.get_mut(idx) {
            *slot = v;
        }
        if let Some(h) = self.hist.get_mut(v as usize) {
            *h = h.wrapping_add(1);
        }
        if v == 0 {
            let r = lookup(c.addr);
            self.addr_digest = (self.addr_digest ^ result_digest(&r)).wrapping_mul(FNV_PRIME);
            if r.found {
                self.found_count = self.found_count.wrapping_add(1);
            } else {
                self.miss_count = self.miss_count.wrapping_add(1);
            }
        }
        Ok((idx, v))
    }

    /// 观测实测判据码。
    pub fn observe(&mut self, id: u32, seen_code: u32) -> Result<bool, u32> {
        if self.frozen {
            return Err(DA_EFROZEN);
        }
        let mut k = 0usize;
        let mut hit: Option<usize> = None;
        while k < self.len {
            if let Some(x) = self.cases.get(k) {
                if x.id == id {
                    hit = Some(k);
                }
            }
            k = k.wrapping_add(1);
        }
        let idx = match hit {
            Some(v) => v,
            None => return Err(DA_EUNSEEN),
        };
        match self.seen.get(idx) {
            Some(true) => return Err(DA_EDOBS),
            _ => {}
        }
        if let Some(slot) = self.seen.get_mut(idx) {
            *slot = true;
        }
        self.observed = self.observed.wrapping_add(1);
        let expect = match self.verdicts.get(idx) {
            Some(v) => *v,
            None => 0,
        };
        if seen_code == expect {
            self.matched = self.matched.wrapping_add(1);
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }
    pub fn attempts(&self) -> u32 {
        self.attempts
    }
    pub fn accepts(&self) -> u32 {
        self.accepts
    }
    pub fn rejects(&self) -> u32 {
        self.rejects
    }
    pub fn observed(&self) -> u32 {
        self.observed
    }
    pub fn matched(&self) -> u32 {
        self.matched
    }
    pub fn found_count(&self) -> u32 {
        self.found_count
    }
    pub fn miss_count(&self) -> u32 {
        self.miss_count
    }
    pub fn addr_digest(&self) -> u64 {
        self.addr_digest
    }
    pub fn hist_at(&self, i: usize) -> u32 {
        match self.hist.get(i) {
            Some(v) => *v,
            None => 0,
        }
    }
    pub fn hist_sum(&self) -> u32 {
        let mut s = 0u32;
        let mut k = 0usize;
        while k < 6 {
            if let Some(v) = self.hist.get(k) {
                s = s.wrapping_add(*v);
            }
            k = k.wrapping_add(1);
        }
        s
    }
    pub fn zero_verdicts(&self) -> u32 {
        self.hist_at(0)
    }
    pub fn flag_issues(&self) -> u32 {
        self.hist_at(DV_FLAG as usize)
    }
    pub fn addr_issues(&self) -> u32 {
        self.hist_at(DV_ADDR as usize)
    }
    pub fn lib_issues(&self) -> u32 {
        self.hist_at(DV_LIB as usize)
    }
    pub fn sym_issues(&self) -> u32 {
        self.hist_at(DV_SYM as usize)
    }
    pub fn off_issues(&self) -> u32 {
        self.hist_at(DV_OFF as usize)
    }
    pub fn case_at(&self, i: usize) -> Option<DlCase> {
        self.cases.get(i).copied()
    }
    pub fn verdict_at(&self, i: usize) -> Option<u32> {
        self.verdicts.get(i).copied()
    }
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }
    pub fn root(&self) -> Option<u64> {
        self.root
    }

    fn twine(h: u64, v: u64) -> u64 {
        (h ^ v).wrapping_mul(FNV_PRIME)
    }

    /// FNV-1a：覆盖样本、判据、观测位、直方、结果摘要与计数。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF ^ TAG;
        let mut k = 0usize;
        while k < CAP {
            if let Some(c) = self.cases.get(k) {
                h = Self::twine(h, c.id as u64);
                h = Self::twine(h, c.addr);
                h = Self::twine(h, c.flags as u64);
                h = Self::twine(h, c.want_lib as u64);
                h = Self::twine(h, c.want_sym as u64);
                h = Self::twine(h, c.off);
                h = Self::twine(h, c.hash);
            }
            if let Some(v) = self.verdicts.get(k) {
                h = Self::twine(h, *v as u64);
            }
            if let Some(b) = self.seen.get(k) {
                h = Self::twine(h, if *b { 1 } else { 0 });
            }
            k = k.wrapping_add(1);
        }
        k = 0;
        while k < 6 {
            if let Some(x) = self.hist.get(k) {
                h = Self::twine(h, *x as u64);
            }
            k = k.wrapping_add(1);
        }
        h = Self::twine(h, self.found_count as u64);
        h = Self::twine(h, self.miss_count as u64);
        h = Self::twine(h, self.addr_digest);
        h = Self::twine(h, self.len as u64);
        h = Self::twine(h, self.attempts as u64);
        h = Self::twine(h, self.accepts as u64);
        h = Self::twine(h, self.rejects as u64);
        h = Self::twine(h, self.observed as u64);
        h = Self::twine(h, self.matched as u64);
        h = Self::twine(h, if self.frozen { 1 } else { 0 });
        h
    }

    /// 复算：逐地址重查、重算命中/落空与结果摘要，并核直方。
    pub fn replay(&self) -> Result<u64, u32> {
        if self.len == 0 || self.observed != self.len as u32 || self.matched != self.len as u32 {
            return Err(DA_EUNOBS);
        }
        if self.accepts.wrapping_add(self.rejects) != self.attempts {
            return Err(DA_EBALANCE);
        }
        if self.hist_sum() != self.len as u32 {
            return Err(DA_EBALANCE);
        }
        let mut hist = [0u32; 6];
        let mut found = 0u32;
        let mut miss = 0u32;
        let mut dig = FNV_OFF ^ TAG;
        let mut k = 0usize;
        while k < self.len {
            let c = match self.cases.get(k) {
                Some(v) => *v,
                None => return Err(DA_EBALANCE),
            };
            if self.seen.get(k) != Some(&true) {
                return Err(DA_EUNOBS);
            }
            let v = addr_verdict(&c);
            if self.verdicts.get(k) != Some(&v) {
                return Err(DA_EMISMATCH);
            }
            if let Some(h) = hist.get_mut(v as usize) {
                *h = h.wrapping_add(1);
            }
            if v == 0 {
                let r = lookup(c.addr);
                dig = (dig ^ result_digest(&r)).wrapping_mul(FNV_PRIME);
                if r.found {
                    found = found.wrapping_add(1);
                } else {
                    miss = miss.wrapping_add(1);
                }
            }
            k = k.wrapping_add(1);
        }
        if dig != self.addr_digest || found != self.found_count || miss != self.miss_count {
            return Err(DA_EBALANCE);
        }
        k = 0;
        while k < 6 {
            if hist.get(k) != self.hist.get(k) {
                return Err(DA_EBALANCE);
            }
            k = k.wrapping_add(1);
        }
        Ok(self.fold())
    }

    /// 封册：先复算再落锁；二次封册只回错不加拒绝计数。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Err(DA_EFROZEN);
        }
        self.replay()?;
        self.frozen = true;
        let sealed = self.fold();
        self.root = Some(sealed);
        Ok(sealed)
    }
}

/// 十三条样本：八条正常反查（含一条落空）、五条各踩一类记录错误。
pub fn spec_of(id: usize) -> (DlCase, u32) {
    if id < 1 || id > SAMPLES {
        return (DlCase::blank(), DA_EID);
    }
    let (addr, flags, wl, ws, off) = match id {
        1 => (0x0040_1000u64, 0u32, 0u32, 0u32, 0u64),
        2 => (0x0040_10ff, F_KEEP, 0, 0, 0xff),
        3 => (0x0040_1200, 0, 0, NONE, 0x1200),
        4 => (0x7f10_0000_1400, 0, 1, 2, 0),
        5 => (0x7f10_0000_14ff, F_KEEP, 1, 2, 0xff),
        6 => (0x7f10_0000_5000, 0, 1, NONE, 0x5000),
        7 => (0x7f10_0020_08ff, 0, 2, 4, 0xff),
        8 => (0x7f10_0038_0000, 0, NONE, NONE, 0),
        9 => (0x0040_1000, 0x80, 0, 0, 0),
        10 => (0, 0, NONE, NONE, 0),
        11 => (0x0040_1000, 0, 9, 0, 0),
        12 => (0x0040_1000, 0, 0, 99, 0),
        _ => (0x0040_1000, 0, 0, 0, 1),
    };
    let c = DlCase {
        id: id as u32,
        addr,
        flags,
        want_lib: wl,
        want_sym: ws,
        off,
        hash: 0x1128_6000u64.wrapping_add(id as u64),
    };
    (c, addr_verdict(&c))
}

/// 逐条登记 + 观测。
pub fn build_ledger() -> DlLedger {
    let mut l = DlLedger::new();
    let mut i = 1usize;
    while i <= SAMPLES {
        let (c, expect) = spec_of(i);
        if l.register_ruled(c).is_ok() {
            let _ = l.observe(c.id, expect);
        }
        i = i.wrapping_add(1);
    }
    l
}

/// 四条独立拒绝：顺序、重复、id 越界、封册后写账。
pub fn inject_four() -> [bool; 4] {
    let mut a = DlLedger::new();
    let mut hi = DlCase::blank();
    hi.id = 2;
    let _ = a.register(hi);
    let mut lo = DlCase::blank();
    lo.id = 1;
    let bad_a = a.register(lo).err() == Some(DA_EORDER);

    let mut b = DlLedger::new();
    let one = spec_of(1).0;
    let _ = b.register(one);
    let bad_b = b.register(one).err() == Some(DA_EDUP);

    let mut c = DlLedger::new();
    let mut wide = DlCase::blank();
    wide.id = ID_MAX + 1;
    let bad_c = c.register(wide).err() == Some(DA_EID);

    let mut d = build_ledger();
    let _ = d.freeze();
    let mut late = DlCase::blank();
    late.id = 40;
    let bad_d = d.register(late).err() == Some(DA_EFROZEN);
    [bad_a, bad_b, bad_c, bad_d]
}

/// 十三条自检：前九项压反查三层语义，后四项压拒绝与封册。
pub fn run_unx11286_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11286");

    let layout = SEG_BASE == 0x1f560
        && DA_EID == SEG_BASE + 0x01
        && DA_EORDER == SEG_BASE + 0x02
        && DA_EDUP == SEG_BASE + 0x03
        && DA_EFULL == SEG_BASE + 0x04
        && DA_EFROZEN == SEG_BASE + 0x05
        && DA_EUNSEEN == SEG_BASE + 0x06
        && DA_EMISMATCH == SEG_BASE + 0x07
        && DA_EUNOBS == SEG_BASE + 0x08
        && DA_EDOBS == SEG_BASE + 0x09
        && DA_EBALANCE == SEG_BASE + 0x0a;
    s.add("S1-段布局与十个错误码", layout, "0x1f560 段十码连续");

    let mut objs_ok = OBJS.len() == LIB_COUNT && LIB_COUNT == 4;
    let mut k = 0usize;
    while k < LIB_COUNT {
        if let Some(o) = OBJS.get(k) {
            if o.span == 0 || o.name.is_empty() {
                objs_ok = false;
            }
            if k > 0 {
                if let Some(p) = OBJS.get(k - 1) {
                    if p.bias.wrapping_add(p.span) > o.bias {
                        objs_ok = false;
                    }
                }
            }
        }
        k = k.wrapping_add(1);
    }
    s.add("S2-四对象区间升序不重叠", objs_ok,
        "主程序低址，libc/libm 相接，libssl 留缝，段长非零");

    let mut syms_ok = SYMS.len() == SYM_COUNT && SYM_COUNT == 8;
    k = 0;
    while k < SYM_COUNT {
        if let Some(y) = SYMS.get(k) {
            if y.size == 0 || y.name.is_empty() || y.lib as usize >= LIB_COUNT {
                syms_ok = false;
            }
            match OBJS.get(y.lib as usize) {
                Some(o) => {
                    if y.value < o.bias || y.value.wrapping_add(y.size) > o.bias.wrapping_add(o.span) {
                        syms_ok = false;
                    }
                }
                None => syms_ok = false,
            }
            if k > 0 {
                if let Some(p) = SYMS.get(k - 1) {
                    if p.lib == y.lib && p.value.wrapping_add(p.size) > y.value {
                        syms_ok = false;
                    }
                }
            }
        }
        k = k.wrapping_add(1);
    }
    s.add("S3-八符号区间自洽", syms_ok,
        "符号全在对象内、同库按址递增、区间不重叠且长度为正");

    let mut good_ok = true;
    let mut i = 1usize;
    while i <= 8 {
        let (c, v) = spec_of(i);
        let r = lookup(c.addr);
        let found_want = c.want_lib != NONE;
        if v != 0 || r.found != found_want {
            good_ok = false;
        } else if r.found {
            if r.lib != c.want_lib || r.sym != c.want_sym || r.off != c.off {
                good_ok = false;
            }
        }
        i = i.wrapping_add(1);
    }
    s.add("S4-八条样本三元组全中", good_ok,
        "库、符号、偏移逐项等于记录，唯一条落空样本一致为空");

    let main = match OBJS.get(0) {
        Some(v) => *v,
        None => Obj { name: "", bias: 0, span: 1 },
    };
    let sym0 = match SYMS.get(0) {
        Some(v) => *v,
        None => Sym { name: "", lib: 0, value: 0, size: 1 },
    };
    let rb = lookup(main.bias);
    let re = lookup(main.bias.wrapping_add(main.span));
    let rl = lookup(main.bias.wrapping_add(main.span).wrapping_sub(1));
    let r0 = lookup(sym0.value);
    let r0e = lookup(sym0.value.wrapping_add(sym0.size).wrapping_sub(1));
    let rfree = lookup(match SYMS.get(2) {
        Some(v) => v.value,
        None => 0,
    });
    s.add("S5-左闭右开边界", rb.found
        && rb.sym == NONE
        && rb.off == 0
        && rl.found
        && rl.off == main.span - 1
        && !re.found
        && r0.sym == 0
        && r0.off == 0
        && r0e.sym == 0
        && r0e.off == sym0.size - 1
        && rfree.sym == 2
        && rfree.off == 0,
        "基址无符号、末址可查、末端越界、符号两端各归其位");

    s.add("S6-缝隙零址与低址不猜", !lookup(0).found
        && !lookup(0x7f10_0038_0000).found
        && !lookup(0x003f_ffff).found
        && lookup(0x7f10_0020_0000).found
        && spec_of(10).1 == DV_ADDR,
        "零地址、库间缝、首对象之前都返回 0，相接处归后库");

    s.add("S7-五类记录错误各判一码", spec_of(9).1 == DV_FLAG
        && spec_of(10).1 == DV_ADDR
        && spec_of(11).1 == DV_LIB
        && spec_of(12).1 == DV_SYM
        && spec_of(13).1 == DV_OFF,
        "旗标、零址、库号、符号号、偏移五码各一");

    let led = build_ledger();
    let counts_ok = led.len() == SAMPLES
        && led.zero_verdicts() == 8
        && led.flag_issues() == 1
        && led.addr_issues() == 1
        && led.lib_issues() == 1
        && led.sym_issues() == 1
        && led.off_issues() == 1
        && led.hist_sum() == SAMPLES as u32
        && led.found_count() == 7
        && led.miss_count() == 1;
    s.add("S8-十三样本落账配平", counts_ok,
        "八条零判据，命中 7 落空 1，五类错误各一");

    let mut dig = FNV_OFF ^ TAG;
    let mut fd = 0u32;
    let mut ms = 0u32;
    let mut i2 = 1usize;
    while i2 <= 8 {
        let (c, _) = spec_of(i2);
        let r = lookup(c.addr);
        dig = (dig ^ result_digest(&r)).wrapping_mul(FNV_PRIME);
        if r.found {
            fd = fd.wrapping_add(1);
        } else {
            ms = ms.wrapping_add(1);
        }
        i2 = i2.wrapping_add(1);
    }
    let led2 = build_ledger();
    s.add("S9-结果摘要复算与折叠稳定", dig == led.addr_digest()
        && fd == led.found_count()
        && ms == led.miss_count()
        && led.fold() == led2.fold()
        && led.replay().is_ok()
        && led.attempts() == SAMPLES as u32
        && led.accepts() == SAMPLES as u32,
        "八条反查摘要逐字节复算，两次构建折叠同值");

    let mut o = DlLedger::new();
    let c1 = spec_of(1).0;
    let reg = o.register_ruled(c1).is_ok();
    let first = o.observe(c1.id, 0) == Ok(true);
    let twice = o.observe(c1.id, 0).err() == Some(DA_EDOBS);
    let unseen = o.observe(99, 0).err() == Some(DA_EUNSEEN);
    let mut o2 = DlLedger::new();
    let c13 = spec_of(13).0;
    let _ = o2.register_ruled(c13);
    let wrong = o2.observe(c13.id, 0) == Ok(false);
    s.add("S10-观测语义", reg && first && twice && unseen && wrong,
        "正确回真、重复回 EDOBS、未见回 EUNSEEN、错码回假");

    let four = inject_four();
    s.add("S11-四条独立拒绝", four[0] && four[1] && four[2] && four[3],
        "顺序、重复、id 越界、封册后写账各拒一次");

    let mut cap = build_ledger();
    let mut id = 14u32;
    while id <= 16 {
        let mut c = DlCase::blank();
        c.id = id;
        let _ = cap.register(c);
        id = id.wrapping_add(1);
    }
    let mut over = DlCase::blank();
    over.id = 17;
    let cap_ok = cap.len() == CAP && cap.register(over).err() == Some(DA_EFULL);
    s.add("S12-定容十六满表拒绝", cap_ok, "补足 16 条后第 17 条回 EFULL");

    let mut f = build_ledger();
    let d = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let rb2 = f.rejects();
    let again = match f.freeze() {
        Err(DA_EFROZEN) => true,
        _ => false,
    };
    s.add("S13-封册落根与二次拒绝", d != 0
        && d == f.fold()
        && f.root() == Some(d)
        && again
        && f.rejects() == rb2
        && f.replay().is_ok(),
        "首封落根等于折叠值，二次封册回 EFROZEN 不计拒绝");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_are_sorted_and_bounded() {
        assert_eq!(OBJS.len(), 4);
        assert_eq!(SYMS.len(), 8);
        assert_eq!(OBJS[0].name, "/usr/bin/prog");
        assert!(OBJS[1].bias + OBJS[1].span <= OBJS[2].bias);
        assert!(OBJS[2].bias + OBJS[2].span < OBJS[3].bias);
        let mut k = 0usize;
        while k < SYM_COUNT {
            let y = SYMS[k];
            let o = OBJS[y.lib as usize];
            assert!(y.value >= o.bias);
            assert!(y.value + y.size <= o.bias + o.span);
            k += 1;
        }
    }

    #[test]
    fn lookup_has_three_layers() {
        let m = lookup(0x0040_10ff);
        assert!(m.found);
        assert_eq!(m.lib, 0);
        assert_eq!(m.sym, 0);
        assert_eq!(m.off, 0xff);
        let obj_only = lookup(0x0040_1200);
        assert!(obj_only.found);
        assert_eq!(obj_only.sym, NONE);
        assert_eq!(obj_only.off, 0x1200);
        assert!(!lookup(0x7f10_0038_0000).found);
        assert_eq!(lookup(0), Lookup::miss());
    }

    #[test]
    fn boundaries_are_half_open() {
        let m = SYMS[0];
        assert_eq!(lookup(m.value).off, 0);
        assert_eq!(lookup(m.value + m.size).sym, NONE);
        assert_eq!(lookup(m.value + m.size - 1).off, m.size - 1);
        assert_eq!(lookup(SYMS[2].value).sym, 2);
        assert_eq!(lookup(OBJS[1].bias).sym, NONE);
        assert!(lookup(OBJS[1].bias + OBJS[1].span).found);
        assert_eq!(lookup(OBJS[2].bias + OBJS[2].span).found, false);
        assert_eq!(lookup(OBJS[1].bias - 1).found, false);
    }

    #[test]
    fn verdicts_catch_five_record_errors() {
        assert_eq!(addr_verdict(&spec_of(1).0), 0);
        assert_eq!(addr_verdict(&spec_of(8).0), 0);
        assert_eq!(spec_of(9).1, DV_FLAG);
        assert_eq!(spec_of(10).1, DV_ADDR);
        assert_eq!(spec_of(11).1, DV_LIB);
        assert_eq!(spec_of(12).1, DV_SYM);
        assert_eq!(spec_of(13).1, DV_OFF);
        assert_eq!(spec_of(0).1, DA_EID);
        assert_eq!(spec_of(14).1, DA_EID);
    }

    #[test]
    fn ledger_balances_and_replays() {
        let l = build_ledger();
        assert_eq!(l.len(), 13);
        assert_eq!(l.zero_verdicts(), 8);
        assert_eq!(l.found_count(), 7);
        assert_eq!(l.miss_count(), 1);
        assert_eq!(l.hist_sum(), 13);
        assert_eq!(l.attempts(), 13);
        assert_eq!(l.observed(), 13);
        assert_eq!(l.matched(), 13);
        match l.replay() {
            Ok(v) => assert_eq!(v, l.fold()),
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn refusals_and_freeze() {
        let four = inject_four();
        assert!(four[0] && four[1] && four[2] && four[3]);
        let mut f = build_ledger();
        let pre = f.fold();
        assert!(f.freeze().is_ok());
        assert_eq!(f.root(), Some(f.fold()));
        assert!(f.fold() != pre);
        let rb = f.rejects();
        assert_eq!(f.freeze().err(), Some(DA_EFROZEN));
        assert_eq!(f.rejects(), rb);
        assert!(f.replay().is_ok());
    }

    #[test]
    fn checks_are_green() {
        let s = run_unx11286_checks();
        assert!(s.all_passed());
        assert_eq!(s.len(), 13);
    }
}
