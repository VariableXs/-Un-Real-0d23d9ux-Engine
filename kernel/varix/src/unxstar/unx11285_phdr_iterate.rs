//! UNX-F11285 dl_iterate_phdr 遍历语义（0x1f550 段）
//!
//! dl_iterate_phdr 不是"随便走一遍装载表"，它有三条硬语义：
//! 第一，遍历序等于 link_map 装载序——主程序在首，直接依赖按 DT_NEEDED
//! 声明序排开，随后才展开依赖的依赖（广度优先），同一对象只出现一次；
//! 第二，主程序条目的 dlpi_name 是空串，其余条目是库的完整路径；
//! 第三，回调返回值非零即中止遍历，该值原样成为 dl_iterate_phdr 的返回值，
//! 后续对象一条都不再回调。
//! 本段把装载表固化为六个对象：主程序依赖 libc/libm/libssl，libssl 再依赖
//! libcrypto，libc 依赖解释器，形成 0,1,2,4,3,5 的广度序。
//! 每条样本只动 stop_at、旗标与三枚记录值：停止位验回调中止语义，旗标验
//! 未定位错误，序签名验遍历序与装载序一致，PT_LOAD 计数验段摘要。
//! 拒绝面只有 id 越界、顺序倒退、同 id 重复、满表、封册后写账；
//! 停止位越界、未知旗标、记录序错、记录段数错、主程序位错都是样本。

use crate::checks::CheckSet;

/// 诊断段（0x1f550 独占 0x10）。
pub const SEG_BASE: u32 = 0x1f550;

/// 条目 id 越界。
pub const PL_EID: u32 = SEG_BASE + 0x01;
/// 登记序倒退。
pub const PL_EORDER: u32 = SEG_BASE + 0x02;
/// 同 id 重复登记。
pub const PL_EDUP: u32 = SEG_BASE + 0x03;
/// 条目表已满。
pub const PL_EFULL: u32 = SEG_BASE + 0x04;
/// 封册后写账。
pub const PL_EFROZEN: u32 = SEG_BASE + 0x05;
/// 观测了未登记的 id。
pub const PL_EUNSEEN: u32 = SEG_BASE + 0x06;
/// 复算判据与登记时不符。
pub const PL_EMISMATCH: u32 = SEG_BASE + 0x07;
/// 账未观测满。
pub const PL_EUNOBS: u32 = SEG_BASE + 0x08;
/// 同一条目重复观测。
pub const PL_EDOBS: u32 = SEG_BASE + 0x09;
/// 摘要/直方/计数不平。
pub const PL_EBALANCE: u32 = SEG_BASE + 0x0a;

/// 判据码：未知旗标位。
pub const PV_FLAG: u32 = 1;
/// 判据码：停止位越界。
pub const PV_STOP: u32 = 2;
/// 判据码：主程序条目不在首。
pub const PV_MAIN: u32 = 3;
/// 判据码：记录序签名与装载序不符。
pub const PV_ORDER: u32 = 4;
/// 判据码：记录 PT_LOAD 数与段表不符。
pub const PV_LOAD: u32 = 5;

/// 允许的旗标位：仅"逐条打印"提示位。
pub const F_VERBOSE: u32 = 0x01;
/// 已知旗标掩码。
pub const KNOWN_FLAGS: u32 = F_VERBOSE;
/// 回调非零返回的中止码（原样回传）。
pub const STOP_CODE: u32 = 23;
/// 装载对象数。
pub const OBJ_COUNT: usize = 6;
/// 依赖槽数。
pub const DEP_SLOTS: usize = 3;
/// 依赖空槽哨兵。
pub const NO_DEP: u32 = 0xff;
/// 装载序条数。
pub const ORDER_COUNT: usize = 6;
/// 样本条数。
pub const SAMPLES: usize = 13;
/// 条目表定容。
pub const CAP: usize = 16;
/// id 合法域。
pub const ID_MIN: u32 = 1;
pub const ID_MAX: u32 = 4096;
/// 主程序在遍历序中的位置（恒 0）。
pub const MAIN_POS: u32 = 0;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;
const TAG: u64 = 0xb05_85;

/// 装载对象：名字、装载基址、phdr 条数、PT_LOAD 段数、依赖槽。
#[derive(Clone, Copy, Debug)]
pub struct Obj {
    pub name: &'static str,
    pub bias: u64,
    pub phnum: u32,
    pub loads: u32,
    pub deps: [u32; DEP_SLOTS],
}

/// 六个对象的装载表：主程序 ← libc/libm/libssl，libssl ← libcrypto，libc ← 解释器。
pub const OBJS: [Obj; OBJ_COUNT] = [
    Obj { name: "", bias: 0x0040_0000, phnum: 13, loads: 4, deps: [1, 2, 4] },
    Obj {
        name: "/lib/x86_64-linux-gnu/libc.so.6",
        bias: 0x7f10_0000_0000,
        phnum: 13,
        loads: 4,
        deps: [3, NO_DEP, NO_DEP],
    },
    Obj {
        name: "/lib/x86_64-linux-gnu/libm.so.6",
        bias: 0x7f10_0002_0000,
        phnum: 9,
        loads: 3,
        deps: [1, NO_DEP, NO_DEP],
    },
    Obj {
        name: "/lib64/ld-linux-x86-64.so.2",
        bias: 0x7f10_0004_0000,
        phnum: 11,
        loads: 3,
        deps: [NO_DEP, NO_DEP, NO_DEP],
    },
    Obj {
        name: "/lib/x86_64-linux-gnu/libssl.so.3",
        bias: 0x7f10_0006_0000,
        phnum: 11,
        loads: 3,
        deps: [1, 5, NO_DEP],
    },
    Obj {
        name: "/lib/x86_64-linux-gnu/libcrypto.so.3",
        bias: 0x7f10_0008_0000,
        phnum: 12,
        loads: 4,
        deps: [1, NO_DEP, NO_DEP],
    },
];

/// 装载序：主程序入队，按依赖声明序展开，已见对象不重复入列（广度优先）。
pub fn load_order() -> ([u32; ORDER_COUNT], usize) {
    let mut order = [0u32; ORDER_COUNT];
    let mut seen = [false; OBJ_COUNT];
    let mut queue = [0u32; 16];
    let mut qh = 0usize;
    let mut qt = 0usize;
    let mut n = 0usize;
    if let Some(slot) = queue.get_mut(qt) {
        *slot = 0;
    }
    qt = qt.wrapping_add(1);
    while qh < qt {
        let cur = match queue.get(qh) {
            Some(v) => *v,
            None => break,
        };
        qh = qh.wrapping_add(1);
        let ci = cur as usize;
        if ci >= OBJ_COUNT || seen.get(ci) == Some(&true) {
            continue;
        }
        if let Some(s) = seen.get_mut(ci) {
            *s = true;
        }
        if let Some(slot) = order.get_mut(n) {
            *slot = cur;
        }
        n = n.wrapping_add(1);
        let deps = match OBJS.get(ci) {
            Some(o) => o.deps,
            None => [NO_DEP; DEP_SLOTS],
        };
        let mut d = 0usize;
        while d < DEP_SLOTS {
            if let Some(x) = deps.get(d) {
                if *x != NO_DEP {
                    if let Some(slot) = queue.get_mut(qt) {
                        *slot = *x;
                    }
                    qt = qt.wrapping_add(1);
                }
            }
            d = d.wrapping_add(1);
        }
    }
    (order, n)
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

/// 装载序签名：对象下标、名字与基址按序折叠，取低 32 位落成记录值。
pub fn order_sig() -> u32 {
    let (order, n) = load_order();
    let mut h = FNV_OFF ^ TAG;
    let mut k = 0usize;
    while k < n {
        if let Some(x) = order.get(k) {
            h = (h ^ *x as u64).wrapping_mul(FNV_PRIME);
            if let Some(o) = OBJS.get(*x as usize) {
                h = hash_bytes(h, o.name);
                h = (h ^ o.bias).wrapping_mul(FNV_PRIME);
            }
        }
        k = k.wrapping_add(1);
    }
    (h & 0xffff_ffff) as u32
}

/// 全表 PT_LOAD 段数之和。
pub fn loads_total() -> u32 {
    let mut s = 0u32;
    let mut k = 0usize;
    while k < OBJ_COUNT {
        if let Some(o) = OBJS.get(k) {
            s = s.wrapping_add(o.loads);
        }
        k = k.wrapping_add(1);
    }
    s
}

/// 一条遍历样本：停止位、旗标、主程序位、序签名、段数、指纹。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhdrCase {
    pub id: u32,
    pub stop_at: u32,
    pub flags: u32,
    pub main: u32,
    pub ord: u32,
    pub loads: u32,
    pub hash: u64,
}

impl PhdrCase {
    pub const fn blank() -> PhdrCase {
        PhdrCase { id: 0, stop_at: 0, flags: 0, main: 0, ord: 0, loads: 0, hash: 0 }
    }
}

/// 一次遍历的可观测结果：走过的名字、条数、回调中止码、摘要。
#[derive(Clone, Copy, Debug)]
pub struct Run {
    pub names: [&'static str; ORDER_COUNT],
    pub count: usize,
    pub code: u32,
    pub digest: u64,
}

impl Run {
    pub const fn new() -> Run {
        Run { names: [""; ORDER_COUNT], count: 0, code: 0, digest: FNV_OFF ^ TAG }
    }
}

/// 按 link_map 装载序走访；回调在 stop_at 条上返回 STOP_CODE 并中止。
pub fn run(c: &PhdrCase) -> Run {
    let mut r = Run::new();
    if c.stop_at > OBJ_COUNT as u32 {
        return r;
    }
    let (order, n) = load_order();
    let target = if c.stop_at == 0 {
        n
    } else if (c.stop_at as usize) < n {
        c.stop_at as usize
    } else {
        n
    };
    let mut k = 0usize;
    let mut h = FNV_OFF ^ TAG;
    while k < target {
        if let Some(ix) = order.get(k) {
            if let Some(o) = OBJS.get(*ix as usize) {
                if let Some(slot) = r.names.get_mut(k) {
                    *slot = o.name;
                }
                h = (h ^ *ix as u64).wrapping_mul(FNV_PRIME);
                h = hash_bytes(h, o.name);
            }
        }
        k = k.wrapping_add(1);
    }
    r.count = target;
    r.code = if c.stop_at == 0 { 0 } else { STOP_CODE };
    h = (h ^ r.code as u64).wrapping_mul(FNV_PRIME);
    h = (h ^ r.count as u64).wrapping_mul(FNV_PRIME);
    r.digest = h;
    r
}

/// 纯判据：旗标 → 停止位 → 主程序位 → 序签名 → 段数。
pub fn iterate_verdict(c: &PhdrCase) -> u32 {
    if c.flags & !KNOWN_FLAGS != 0 {
        return PV_FLAG;
    }
    if c.stop_at > OBJ_COUNT as u32 {
        return PV_STOP;
    }
    if c.main != MAIN_POS {
        return PV_MAIN;
    }
    if c.ord != order_sig() {
        return PV_ORDER;
    }
    if c.loads != loads_total() {
        return PV_LOAD;
    }
    let r = run(c);
    if r.count == 0 {
        return PV_STOP;
    }
    match r.names.get(0) {
        Some(s) => {
            if *s == "" {
                0
            } else {
                PV_MAIN
            }
        }
        None => PV_MAIN,
    }
}

/// 遍历账本：样本 + 判据 + 六槽直方 + 走访摘要与回调计数。
#[derive(Clone, Copy, Debug)]
pub struct PhdrLedger {
    cases: [PhdrCase; CAP],
    verdicts: [u32; CAP],
    seen: [bool; CAP],
    hist: [u32; 6],
    visit_digest: u64,
    callbacks: u32,
    stops: u32,
    len: usize,
    attempts: u32,
    accepts: u32,
    rejects: u32,
    observed: u32,
    matched: u32,
    frozen: bool,
    root: Option<u64>,
}

impl PhdrLedger {
    pub const fn new() -> PhdrLedger {
        PhdrLedger {
            cases: [PhdrCase::blank(); CAP],
            verdicts: [0u32; CAP],
            seen: [false; CAP],
            hist: [0u32; 6],
            visit_digest: FNV_OFF ^ TAG,
            callbacks: 0,
            stops: 0,
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
    pub fn register(&mut self, c: PhdrCase) -> Result<usize, u32> {
        self.attempts = self.attempts.wrapping_add(1);
        if self.frozen {
            return Err(self.fail(PL_EFROZEN));
        }
        if c.id < ID_MIN || c.id > ID_MAX {
            return Err(self.fail(PL_EID));
        }
        let mut k = 0usize;
        while k < self.len {
            if let Some(x) = self.cases.get(k) {
                if x.id == c.id {
                    return Err(self.fail(PL_EDUP));
                }
            }
            k = k.wrapping_add(1);
        }
        if self.len > 0 {
            if let Some(last) = self.cases.get(self.len - 1) {
                if c.id <= last.id {
                    return Err(self.fail(PL_EORDER));
                }
            }
        }
        if self.len >= CAP {
            return Err(self.fail(PL_EFULL));
        }
        if let Some(slot) = self.cases.get_mut(self.len) {
            *slot = c;
        }
        self.len = self.len.wrapping_add(1);
        self.accepts = self.accepts.wrapping_add(1);
        Ok(self.len - 1)
    }

    /// 登记 + 判据：零判据样本再走一遍遍历，攒走访摘要与回调计数。
    pub fn register_ruled(&mut self, c: PhdrCase) -> Result<(usize, u32), u32> {
        let idx = self.register(c)?;
        let v = iterate_verdict(&c);
        if let Some(slot) = self.verdicts.get_mut(idx) {
            *slot = v;
        }
        if let Some(h) = self.hist.get_mut(v as usize) {
            *h = h.wrapping_add(1);
        }
        if v == 0 {
            let r = run(&c);
            self.visit_digest = (self.visit_digest ^ r.digest).wrapping_mul(FNV_PRIME);
            self.callbacks = self.callbacks.wrapping_add(r.count as u32);
            if r.code != 0 {
                self.stops = self.stops.wrapping_add(1);
            }
        }
        Ok((idx, v))
    }

    /// 观测实测判据码。
    pub fn observe(&mut self, id: u32, seen_code: u32) -> Result<bool, u32> {
        if self.frozen {
            return Err(PL_EFROZEN);
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
            None => return Err(PL_EUNSEEN),
        };
        match self.seen.get(idx) {
            Some(true) => return Err(PL_EDOBS),
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
    pub fn visit_digest(&self) -> u64 {
        self.visit_digest
    }
    pub fn callbacks(&self) -> u32 {
        self.callbacks
    }
    pub fn stops(&self) -> u32 {
        self.stops
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
        self.hist_at(PV_FLAG as usize)
    }
    pub fn stop_issues(&self) -> u32 {
        self.hist_at(PV_STOP as usize)
    }
    pub fn main_issues(&self) -> u32 {
        self.hist_at(PV_MAIN as usize)
    }
    pub fn order_issues(&self) -> u32 {
        self.hist_at(PV_ORDER as usize)
    }
    pub fn load_issues(&self) -> u32 {
        self.hist_at(PV_LOAD as usize)
    }
    pub fn case_at(&self, i: usize) -> Option<PhdrCase> {
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

    /// FNV-1a：覆盖样本、判据、观测位、直方、走访摘要与计数。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF ^ TAG;
        let mut k = 0usize;
        while k < CAP {
            if let Some(c) = self.cases.get(k) {
                h = Self::twine(h, c.id as u64);
                h = Self::twine(h, c.stop_at as u64);
                h = Self::twine(h, c.flags as u64);
                h = Self::twine(h, c.main as u64);
                h = Self::twine(h, c.ord as u64);
                h = Self::twine(h, c.loads as u64);
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
        h = Self::twine(h, self.visit_digest);
        h = Self::twine(h, self.callbacks as u64);
        h = Self::twine(h, self.stops as u64);
        h = Self::twine(h, self.len as u64);
        h = Self::twine(h, self.attempts as u64);
        h = Self::twine(h, self.accepts as u64);
        h = Self::twine(h, self.rejects as u64);
        h = Self::twine(h, self.observed as u64);
        h = Self::twine(h, self.matched as u64);
        h = Self::twine(h, if self.frozen { 1 } else { 0 });
        h
    }

    /// 复算：逐条判据、直方、走访摘要与回调计数四路齐平。
    pub fn replay(&self) -> Result<u64, u32> {
        if self.len == 0 || self.observed != self.len as u32 || self.matched != self.len as u32 {
            return Err(PL_EUNOBS);
        }
        if self.accepts.wrapping_add(self.rejects) != self.attempts {
            return Err(PL_EBALANCE);
        }
        if self.hist_sum() != self.len as u32 {
            return Err(PL_EBALANCE);
        }
        let mut hist = [0u32; 6];
        let mut vd = FNV_OFF ^ TAG;
        let mut cb = 0u32;
        let mut st = 0u32;
        let mut k = 0usize;
        while k < self.len {
            let c = match self.cases.get(k) {
                Some(v) => *v,
                None => return Err(PL_EBALANCE),
            };
            if self.seen.get(k) != Some(&true) {
                return Err(PL_EUNOBS);
            }
            let v = iterate_verdict(&c);
            if self.verdicts.get(k) != Some(&v) {
                return Err(PL_EMISMATCH);
            }
            if let Some(h) = hist.get_mut(v as usize) {
                *h = h.wrapping_add(1);
            }
            if v == 0 {
                let r = run(&c);
                vd = (vd ^ r.digest).wrapping_mul(FNV_PRIME);
                cb = cb.wrapping_add(r.count as u32);
                if r.code != 0 {
                    st = st.wrapping_add(1);
                }
            }
            k = k.wrapping_add(1);
        }
        if vd != self.visit_digest || cb != self.callbacks || st != self.stops {
            return Err(PL_EBALANCE);
        }
        k = 0;
        while k < 6 {
            if hist.get(k) != self.hist.get(k) {
                return Err(PL_EBALANCE);
            }
            k = k.wrapping_add(1);
        }
        Ok(self.fold())
    }

    /// 封册：先复算再落锁；二次封册只回错不加拒绝计数。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Err(PL_EFROZEN);
        }
        self.replay()?;
        self.frozen = true;
        let sealed = self.fold();
        self.root = Some(sealed);
        Ok(sealed)
    }
}

/// 十三条样本：八条正常遍历、五条各踩一类记录错误。
pub fn spec_of(id: usize) -> (PhdrCase, u32) {
    if id < 1 || id > SAMPLES {
        return (PhdrCase::blank(), PL_EID);
    }
    let stops = [0u32, 1, 2, 3, 4, 5, 6, 6];
    let sig = order_sig();
    let loads = loads_total();
    let (stop_at, flags, main, ord, ld) = match id {
        1..=8 => {
            let s = match stops.get(id - 1) {
                Some(v) => *v,
                None => 0,
            };
            let f = if id % 2 == 0 { F_VERBOSE } else { 0 };
            (s, f, MAIN_POS, sig, loads)
        }
        9 => (0, 0x80, MAIN_POS, sig, loads),
        10 => (7, 0, MAIN_POS, sig, loads),
        11 => (0, 0, 1, sig, loads),
        12 => (0, 0, MAIN_POS, sig ^ 1, loads),
        _ => (0, 0, MAIN_POS, sig, loads.wrapping_add(1)),
    };
    let c = PhdrCase {
        id: id as u32,
        stop_at,
        flags,
        main,
        ord,
        loads: ld,
        hash: 0x1128_5000u64.wrapping_add(id as u64),
    };
    (c, iterate_verdict(&c))
}

/// 逐条登记 + 观测。
pub fn build_ledger() -> PhdrLedger {
    let mut l = PhdrLedger::new();
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
    let mut a = PhdrLedger::new();
    let mut hi = PhdrCase::blank();
    hi.id = 2;
    let _ = a.register(hi);
    let mut lo = PhdrCase::blank();
    lo.id = 1;
    let bad_a = a.register(lo).err() == Some(PL_EORDER);

    let mut b = PhdrLedger::new();
    let one = spec_of(1).0;
    let _ = b.register(one);
    let bad_b = b.register(one).err() == Some(PL_EDUP);

    let mut c = PhdrLedger::new();
    let mut wide = PhdrCase::blank();
    wide.id = ID_MAX + 1;
    let bad_c = c.register(wide).err() == Some(PL_EID);

    let mut d = build_ledger();
    let _ = d.freeze();
    let mut late = PhdrCase::blank();
    late.id = 40;
    let bad_d = d.register(late).err() == Some(PL_EFROZEN);
    [bad_a, bad_b, bad_c, bad_d]
}

/// 十三条自检：前九项压装载序与回调语义，后四项压拒绝与封册。
pub fn run_unx11285_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11285");

    let layout = SEG_BASE == 0x1f550
        && PL_EID == SEG_BASE + 0x01
        && PL_EORDER == SEG_BASE + 0x02
        && PL_EDUP == SEG_BASE + 0x03
        && PL_EFULL == SEG_BASE + 0x04
        && PL_EFROZEN == SEG_BASE + 0x05
        && PL_EUNSEEN == SEG_BASE + 0x06
        && PL_EMISMATCH == SEG_BASE + 0x07
        && PL_EUNOBS == SEG_BASE + 0x08
        && PL_EDOBS == SEG_BASE + 0x09
        && PL_EBALANCE == SEG_BASE + 0x0a;
    s.add("S1-段布局与十个错误码", layout, "0x1f550 段十码连续");

    let (order, n) = load_order();
    let mut table_ok = n == OBJ_COUNT && OBJS.len() == OBJ_COUNT && OBJ_COUNT == 6;
    let mut uniq = [false; OBJ_COUNT];
    let mut k = 0usize;
    while k < n {
        if let Some(x) = order.get(k) {
            let ix = *x as usize;
            if ix >= OBJ_COUNT || uniq.get(ix) == Some(&true) {
                table_ok = false;
            }
            if let Some(u) = uniq.get_mut(ix) {
                *u = true;
            }
            if let Some(o) = OBJS.get(ix) {
                if o.phnum < o.loads || o.loads == 0 {
                    table_ok = false;
                }
                let mut d = 0usize;
                while d < DEP_SLOTS {
                    if let Some(dep) = o.deps.get(d) {
                        if *dep != NO_DEP && *dep as usize >= OBJ_COUNT {
                            table_ok = false;
                        }
                    }
                    d = d.wrapping_add(1);
                }
            }
        }
        k = k.wrapping_add(1);
    }
    let main_name = match OBJS.get(0) {
        Some(o) => o.name,
        None => "x",
    };
    table_ok = table_ok && main_name == "";
    s.add("S2-六对象装载表自洽", table_ok,
        "主程序名空串，phnum≥PT_LOAD≥1，依赖全在表内且无重复");

    s.add("S3-广度装载序固定为 0,1,2,4,3,5",
        order == [0u32, 1, 2, 4, 3, 5] && n == 6,
        "直接依赖先排，libcrypto 与解释器在第二层展开");

    let r0 = run(&spec_of(1).0);
    let mut same = r0.count == OBJ_COUNT && r0.code == 0;
    let mut k2 = 0usize;
    while k2 < OBJ_COUNT {
        let want = match order.get(k2).and_then(|x| OBJS.get(*x as usize)) {
            Some(o) => o.name,
            None => "?",
        };
        match r0.names.get(k2) {
            Some(got) => {
                if *got != want {
                    same = false;
                }
            }
            None => same = false,
        }
        k2 = k2.wrapping_add(1);
    }
    s.add("S4-遍历序逐条等于装载序", same
        && r0.names.get(0) == Some(&""),
        "六条名字与 link_map 顺序一一对应，首条 dlpi_name 为空");

    let mut stop_ok = true;
    let mut st = 1u32;
    while st <= OBJ_COUNT as u32 {
        let mut c = spec_of(1).0;
        c.stop_at = st;
        let r = run(&c);
        if r.count != st as usize || r.code != STOP_CODE {
            stop_ok = false;
        }
        st = st.wrapping_add(1);
    }
    let mut c7 = spec_of(1).0;
    c7.stop_at = 7;
    let r7 = run(&c7);
    s.add("S5-回调非零即中止", stop_ok
        && r7.count == 0
        && r7.code == 0
        && iterate_verdict(&c7) == PV_STOP,
        "第 1..6 条回调返回 23 即停，越界停止位不走访");

    let good = spec_of(1).0;
    s.add("S6-旗标仅认打印位", iterate_verdict(&good) == 0
        && {
            let mut v = good;
            v.flags = F_VERBOSE;
            iterate_verdict(&v) == 0
        }
        && {
            let mut b = good;
            b.flags = 0x80;
            iterate_verdict(&b) == PV_FLAG
        }
        && order_sig() == good.ord
        && loads_total() == good.loads
        && loads_total() == 21,
        "F_VERBOSE 放行、0x80 判旗标错，序签名与 21 段记录相符");

    s.add("S7-五类记录错误各判一码", spec_of(9).1 == PV_FLAG
        && spec_of(10).1 == PV_STOP
        && spec_of(11).1 == PV_MAIN
        && spec_of(12).1 == PV_ORDER
        && spec_of(13).1 == PV_LOAD,
        "旗标、停止位、主程序位、序签名、段数五码各一");

    let led = build_ledger();
    let counts_ok = led.len() == SAMPLES
        && led.zero_verdicts() == 8
        && led.flag_issues() == 1
        && led.stop_issues() == 1
        && led.main_issues() == 1
        && led.order_issues() == 1
        && led.load_issues() == 1
        && led.hist_sum() == SAMPLES as u32
        && led.callbacks() == 33
        && led.stops() == 7;
    s.add("S8-十三样本落账配平", counts_ok,
        "八条零判据，回调 33 次、中止 7 次，五类错误各一");

    let mut vd = FNV_OFF ^ TAG;
    let mut i = 1usize;
    while i <= 8 {
        let (c, _) = spec_of(i);
        let r = run(&c);
        vd = (vd ^ r.digest).wrapping_mul(FNV_PRIME);
        i = i.wrapping_add(1);
    }
    let led2 = build_ledger();
    s.add("S9-走访摘要复算与折叠稳定", vd == led.visit_digest()
        && led.fold() == led2.fold()
        && led.replay().is_ok()
        && led.attempts() == SAMPLES as u32
        && led.accepts() == SAMPLES as u32,
        "八次正常遍历摘要逐字节复算，两次构建折叠同值");

    let mut o = PhdrLedger::new();
    let c1 = spec_of(1).0;
    let reg = o.register_ruled(c1).is_ok();
    let first = o.observe(c1.id, 0) == Ok(true);
    let twice = o.observe(c1.id, 0).err() == Some(PL_EDOBS);
    let unseen = o.observe(99, 0).err() == Some(PL_EUNSEEN);
    let mut o2 = PhdrLedger::new();
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
        let mut c = PhdrCase::blank();
        c.id = id;
        let _ = cap.register(c);
        id = id.wrapping_add(1);
    }
    let mut over = PhdrCase::blank();
    over.id = 17;
    let cap_ok = cap.len() == CAP && cap.register(over).err() == Some(PL_EFULL);
    s.add("S12-定容十六满表拒绝", cap_ok, "补足 16 条后第 17 条回 EFULL");

    let mut f = build_ledger();
    let d = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let rb = f.rejects();
    let again = match f.freeze() {
        Err(PL_EFROZEN) => true,
        _ => false,
    };
    s.add("S13-封册落根与二次拒绝", d != 0
        && d == f.fold()
        && f.root() == Some(d)
        && again
        && f.rejects() == rb
        && f.replay().is_ok(),
        "首封落根等于折叠值，二次封册回 EFROZEN 不计拒绝");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_order_is_breadth_first() {
        let (order, n) = load_order();
        assert_eq!(n, 6);
        assert_eq!(order, [0u32, 1, 2, 4, 3, 5]);
        let mut seen = [false; 6];
        let mut k = 0usize;
        while k < n {
            let ix = order[k] as usize;
            assert!(!seen[ix]);
            seen[ix] = true;
            k += 1;
        }
        assert_eq!(OBJS[0].name, "");
        assert_eq!(OBJS[1].name, "/lib/x86_64-linux-gnu/libc.so.6");
        assert_eq!(loads_total(), 21);
    }

    #[test]
    fn traversal_names_follow_link_map() {
        let c = spec_of(1).0;
        let r = run(&c);
        assert_eq!(r.count, 6);
        assert_eq!(r.code, 0);
        assert_eq!(r.names[0], "");
        assert_eq!(r.names[1], OBJS[1].name);
        assert_eq!(r.names[2], OBJS[2].name);
        assert_eq!(r.names[3], OBJS[4].name);
        assert_eq!(r.names[4], OBJS[3].name);
        assert_eq!(r.names[5], OBJS[5].name);
        let r2 = run(&c);
        assert_eq!(r.digest, r2.digest);
        assert_eq!(order_sig(), c.ord);
    }

    #[test]
    fn callback_stop_propagates() {
        let base = spec_of(1).0;
        let mut stop = 1u32;
        while stop <= 6 {
            let mut c = base;
            c.stop_at = stop;
            let r = run(&c);
            assert_eq!(r.count, stop as usize);
            assert_eq!(r.code, STOP_CODE);
            assert_eq!(iterate_verdict(&c), 0);
            stop += 1;
        }
        let mut full = base;
        full.stop_at = 0;
        assert_eq!(run(&full).code, 0);
        let mut over = base;
        over.stop_at = 7;
        assert_eq!(run(&over).count, 0);
        assert_eq!(iterate_verdict(&over), PV_STOP);
    }

    #[test]
    fn verdicts_catch_five_record_errors() {
        assert_eq!(iterate_verdict(&spec_of(1).0), 0);
        assert_eq!(iterate_verdict(&spec_of(8).0), 0);
        assert_eq!(spec_of(9).1, PV_FLAG);
        assert_eq!(spec_of(10).1, PV_STOP);
        assert_eq!(spec_of(11).1, PV_MAIN);
        assert_eq!(spec_of(12).1, PV_ORDER);
        assert_eq!(spec_of(13).1, PV_LOAD);
        assert_eq!(spec_of(0).1, PL_EID);
        assert_eq!(spec_of(14).1, PL_EID);
    }

    #[test]
    fn ledger_balances_and_replays() {
        let l = build_ledger();
        assert_eq!(l.len(), 13);
        assert_eq!(l.zero_verdicts(), 8);
        assert_eq!(l.hist_sum(), 13);
        assert_eq!(l.callbacks(), 33);
        assert_eq!(l.stops(), 7);
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
        assert_eq!(f.freeze().err(), Some(PL_EFROZEN));
        assert_eq!(f.rejects(), rb);
        assert!(f.replay().is_ok());
    }

    #[test]
    fn checks_are_green() {
        let s = run_unx11285_checks();
        assert!(s.all_passed());
        assert_eq!(s.len(), 13);
    }
}
