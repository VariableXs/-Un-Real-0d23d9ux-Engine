//! UNX-F11284 版本不匹配排疑手册接线（0x1f540 段）
//!
//! 排疑手册部 C 第一问只有一句："程序报缺 GLIBC 版本怎么办"。这一问的
//! 接线面不是散文，而是**八条案例 → 六个处置端**的固定映射：每条案例
//! 先由归域引擎判一次域，再从候选处置里选出唯一动作。
//! 归域引擎复用 F11214 的三域规则表：七个关键词、优先级小者先、首个命中
//! 定域、无命中落 UNKNOWN。本段只认 C5（兼容层），八条案例必须全部判 C5，
//! 判到 C1/C2/UNKNOWN 都算接线冲突，冲突数进直方并随 replay 重算。
//! 处置端六个：版本比对、搜索路径、装载器、宿主转派、soname 链接、缓存。
//! 处置关键词表独立于归域表，因此同一条案例的"域"与"动作"互不干扰。
//! 账本记两枚摘要：引擎摘要封住规则表形状，接线摘要封住八条案例的
//! （文本、域、动作）三元组；任一表被改动，replay 先比摘要再逐条复算。
//! 拒绝面只有 id 越界、顺序倒退、同 id 重复、满表、封册后写账；
//! 问题号、案文下标、记录域、记录动作越界都是样本，进规则产码不拒收。

use crate::checks::CheckSet;

/// 诊断段（0x1f540 独占 0x10）。
pub const SEG_BASE: u32 = 0x1f540;

/// 条目 id 越界。
pub const WW_EID: u32 = SEG_BASE + 0x01;
/// 登记序倒退。
pub const WW_EORDER: u32 = SEG_BASE + 0x02;
/// 同 id 重复登记。
pub const WW_EDUP: u32 = SEG_BASE + 0x03;
/// 条目表已满。
pub const WW_EFULL: u32 = SEG_BASE + 0x04;
/// 封册后写账。
pub const WW_EFROZEN: u32 = SEG_BASE + 0x05;
/// 观测了未登记的 id。
pub const WW_EUNSEEN: u32 = SEG_BASE + 0x06;
/// 复算判据与登记时不符。
pub const WW_EMISMATCH: u32 = SEG_BASE + 0x07;
/// 账未观测满。
pub const WW_EUNOBS: u32 = SEG_BASE + 0x08;
/// 同一条目重复观测。
pub const WW_EDOBS: u32 = SEG_BASE + 0x09;
/// 摘要/直方/计数不平。
pub const WW_EBALANCE: u32 = SEG_BASE + 0x0a;

/// 判据码：不是部 C 第一问。
pub const WV_Q: u32 = 1;
/// 判据码：案文下标越界。
pub const WV_TEXT: u32 = 2;
/// 判据码：记录域非法。
pub const WV_BAD: u32 = 3;
/// 判据码：记录域与引擎判定冲突。
pub const WV_DOM: u32 = 4;
/// 判据码：记录动作与处置表不符。
pub const WV_STEP: u32 = 5;

/// 部 C 第一问的问题号。
pub const Q_C1: u32 = 1;
/// 归域：C1 内核 / C2 信号 / C5 兼容 / 未知（与 F11214 同值）。
pub const DOM_C1: u32 = 1;
pub const DOM_C2: u32 = 2;
pub const DOM_C5: u32 = 5;
pub const DOM_UNKNOWN: u32 = 0;
pub const DOMAINS: [u32; 3] = [DOM_C1, DOM_C2, DOM_C5];

/// 处置端：版本比对。
pub const ST_VER: u32 = 1;
/// 处置端：搜索路径。
pub const ST_PATH: u32 = 2;
/// 处置端：装载器。
pub const ST_LDSO: u32 = 3;
/// 处置端：宿主转派。
pub const ST_HOST: u32 = 4;
/// 处置端：soname 链接。
pub const ST_SONAME: u32 = 5;
/// 处置端：缓存刷新。
pub const ST_CACHE: u32 = 6;
/// 处置端条数。
pub const STEP_COUNT: usize = 6;
/// 处置端合法表。
pub const STEPS: [u32; STEP_COUNT] = [ST_VER, ST_PATH, ST_LDSO, ST_HOST, ST_SONAME, ST_CACHE];

/// 手册案例条数。
pub const MANUAL_COUNT: usize = 8;
/// 样本条数。
pub const SAMPLES: usize = 13;
/// 条目表定容。
pub const CAP: usize = 16;
/// id 合法域。
pub const ID_MIN: u32 = 1;
pub const ID_MAX: u32 = 4096;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;
const TAG: u64 = 0xb05_84;

/// 归域规则：关键词、归属域、优先级（小者先）。
#[derive(Clone, Copy, Debug)]
pub struct Rule {
    pub kw: &'static str,
    pub dom: u32,
    pub prio: u32,
}

/// 部 C 第一问的八条案例原文。
pub const MANUAL: [&str; MANUAL_COUNT] = [
    "a.out: version `GLIBC_2.34' not found (required by a.out)",
    "b.out: version mismatch in libc.so.6 symbol table",
    "c.out: GLIBC_2.39 required but soname link stale",
    "d.out: GLIBC_2.28 missing, check ld.so ABI",
    "e.out: GLIBC_2.17 missing after ldconfig -p",
    "f.out: GLIBC_2.2.5 missing in chroot, check search path",
    "g.out: GLIBC_2.38 missing but host has 2.39",
    "h.out: version `GLIBC_2.34' not found (required by libfoo.so)",
];

/// 归域表：七条规则逐项复用 F11214 的关键词、域与优先级。
pub const RULES: [Rule; 7] = [
    Rule { kw: "version `GLIBC", dom: DOM_C5, prio: 1 },
    Rule { kw: "version mismatch", dom: DOM_C5, prio: 2 },
    Rule { kw: "GLIBC_2", dom: DOM_C5, prio: 3 },
    Rule { kw: "rt_sigaction", dom: DOM_C2, prio: 4 },
    Rule { kw: "rseq", dom: DOM_C2, prio: 4 },
    Rule { kw: "auxv", dom: DOM_C1, prio: 5 },
    Rule { kw: "set_robust_list", dom: DOM_C1, prio: 5 },
];

/// 处置表：八条关键词各自指向唯一处置端，优先级小者先。
pub const STEP_RULES: [(&str, u32, u32); 8] = [
    ("version `GLIBC", ST_VER, 1),
    ("version mismatch", ST_VER, 2),
    ("soname", ST_SONAME, 3),
    ("ld.so.cache", ST_CACHE, 4),
    ("ld.so", ST_LDSO, 5),
    ("ldconfig", ST_CACHE, 6),
    ("search path", ST_PATH, 7),
    ("host", ST_HOST, 8),
];

/// 朴素子串包含：空针不命中，越界不外溢。
pub fn contains(hay: &str, needle: &str) -> bool {
    let h = hay.as_bytes();
    let n = needle.as_bytes();
    if n.is_empty() || n.len() > h.len() {
        return false;
    }
    let mut i = 0usize;
    while i + n.len() <= h.len() {
        let mut j = 0usize;
        let mut same = true;
        while j < n.len() {
            let (a, b) = match (h.get(i + j), n.get(j)) {
                (Some(x), Some(y)) => (*x, *y),
                _ => return false,
            };
            if a != b {
                same = false;
                break;
            }
            j = j.wrapping_add(1);
        }
        if same {
            return true;
        }
        i = i.wrapping_add(1);
    }
    false
}

/// 归域：优先级最小者胜，同优先取先录；无命中落 UNKNOWN。
pub fn classify(text: &str) -> u32 {
    let mut best: Option<Rule> = None;
    let mut k = 0usize;
    while k < RULES.len() {
        let r = match RULES.get(k) {
            Some(v) => *v,
            None => break,
        };
        if contains(text, r.kw) {
            best = match best {
                None => Some(r),
                Some(b) => {
                    if r.prio < b.prio {
                        Some(r)
                    } else {
                        Some(b)
                    }
                }
            };
        }
        k = k.wrapping_add(1);
    }
    match best {
        Some(r) => r.dom,
        None => DOM_UNKNOWN,
    }
}

/// 处置：同样按优先级取首条命中；无命中返回 0（非法端）。
pub fn step_of(text: &str) -> u32 {
    let mut best: Option<(&'static str, u32, u32)> = None;
    let mut k = 0usize;
    while k < STEP_RULES.len() {
        let r = match STEP_RULES.get(k) {
            Some(v) => *v,
            None => break,
        };
        if contains(text, r.0) {
            best = match best {
                None => Some(r),
                Some(b) => {
                    if r.2 < b.2 {
                        Some(r)
                    } else {
                        Some(b)
                    }
                }
            };
        }
        k = k.wrapping_add(1);
    }
    match best {
        Some(r) => r.1,
        None => 0,
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

/// 引擎摘要：归域表、案文表、处置表三者按固定序折叠。
pub fn fold_engine() -> u64 {
    let mut h = FNV_OFF ^ TAG;
    let mut k = 0usize;
    while k < RULES.len() {
        if let Some(r) = RULES.get(k) {
            h = hash_bytes(h, r.kw);
            h = (h ^ r.dom as u64).wrapping_mul(FNV_PRIME);
            h = (h ^ r.prio as u64).wrapping_mul(FNV_PRIME);
        }
        k = k.wrapping_add(1);
    }
    k = 0;
    while k < MANUAL_COUNT {
        if let Some(t) = MANUAL.get(k) {
            h = hash_bytes(h, t);
        }
        k = k.wrapping_add(1);
    }
    k = 0;
    while k < STEP_RULES.len() {
        if let Some(r) = STEP_RULES.get(k) {
            h = hash_bytes(h, r.0);
            h = (h ^ r.1 as u64).wrapping_mul(FNV_PRIME);
            h = (h ^ r.2 as u64).wrapping_mul(FNV_PRIME);
        }
        k = k.wrapping_add(1);
    }
    h
}

/// 一条接线记录：问题号、案文下标、记录域、记录动作、指纹。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WireCase {
    pub id: u32,
    pub q: u32,
    pub text: u32,
    pub dom: u32,
    pub step: u32,
    pub hash: u64,
}

impl WireCase {
    pub const fn blank() -> WireCase {
        WireCase { id: 0, q: 0, text: 0, dom: 0, step: 0, hash: 0 }
    }

    pub fn case_text(&self) -> Option<&'static str> {
        MANUAL.get(self.text as usize).copied()
    }
}

/// 纯判据：问题号 → 案文 → 记录域合法 → 引擎归域一致 → 处置一致。
pub fn wire_verdict(c: &WireCase) -> u32 {
    if c.q != Q_C1 {
        return WV_Q;
    }
    let text = match c.case_text() {
        Some(v) => v,
        None => return WV_TEXT,
    };
    let live_dom = classify(text);
    let live_step = step_of(text);
    if !DOMAINS.contains(&c.dom) {
        return WV_BAD;
    }
    if c.dom != live_dom {
        return WV_DOM;
    }
    if c.step != live_step {
        return WV_STEP;
    }
    0
}

/// 接线账本：样本 + 判据 + 六槽直方 + 引擎摘要 + 接线摘要。
#[derive(Clone, Copy, Debug)]
pub struct WireLedger {
    cases: [WireCase; CAP],
    verdicts: [u32; CAP],
    seen: [bool; CAP],
    hist: [u32; 6],
    engine_digest: u64,
    wire_digest: u64,
    len: usize,
    attempts: u32,
    accepts: u32,
    rejects: u32,
    observed: u32,
    matched: u32,
    frozen: bool,
    root: Option<u64>,
}

impl WireLedger {
    pub const fn new() -> WireLedger {
        WireLedger {
            cases: [WireCase::blank(); CAP],
            verdicts: [0u32; CAP],
            seen: [false; CAP],
            hist: [0u32; 6],
            engine_digest: 0,
            wire_digest: FNV_OFF ^ TAG,
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
    pub fn register(&mut self, c: WireCase) -> Result<usize, u32> {
        self.attempts = self.attempts.wrapping_add(1);
        if self.frozen {
            return Err(self.fail(WW_EFROZEN));
        }
        if c.id < ID_MIN || c.id > ID_MAX {
            return Err(self.fail(WW_EID));
        }
        let mut k = 0usize;
        while k < self.len {
            if let Some(x) = self.cases.get(k) {
                if x.id == c.id {
                    return Err(self.fail(WW_EDUP));
                }
            }
            k = k.wrapping_add(1);
        }
        if self.len > 0 {
            if let Some(last) = self.cases.get(self.len - 1) {
                if c.id <= last.id {
                    return Err(self.fail(WW_EORDER));
                }
            }
        }
        if self.len >= CAP {
            return Err(self.fail(WW_EFULL));
        }
        if let Some(slot) = self.cases.get_mut(self.len) {
            *slot = c;
        }
        self.len = self.len.wrapping_add(1);
        self.accepts = self.accepts.wrapping_add(1);
        Ok(self.len - 1)
    }

    /// 登记 + 判据：判据零的接线三元组再折进接线摘要。
    pub fn register_ruled(&mut self, c: WireCase) -> Result<(usize, u32), u32> {
        let first = self.len == 0;
        let idx = self.register(c)?;
        let v = wire_verdict(&c);
        if let Some(slot) = self.verdicts.get_mut(idx) {
            *slot = v;
        }
        if let Some(h) = self.hist.get_mut(v as usize) {
            *h = h.wrapping_add(1);
        }
        if first {
            self.engine_digest = fold_engine();
        }
        if v == 0 {
            if let Some(text) = c.case_text() {
                self.wire_digest = hash_bytes(self.wire_digest, text);
                self.wire_digest = (self.wire_digest ^ c.dom as u64).wrapping_mul(FNV_PRIME);
                self.wire_digest = (self.wire_digest ^ c.step as u64).wrapping_mul(FNV_PRIME);
            }
        }
        Ok((idx, v))
    }

    /// 观测实测判据码。
    pub fn observe(&mut self, id: u32, seen_code: u32) -> Result<bool, u32> {
        if self.frozen {
            return Err(WW_EFROZEN);
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
            None => return Err(WW_EUNSEEN),
        };
        match self.seen.get(idx) {
            Some(true) => return Err(WW_EDOBS),
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
    pub fn engine_digest(&self) -> u64 {
        self.engine_digest
    }
    pub fn wire_digest(&self) -> u64 {
        self.wire_digest
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
    pub fn conflicts(&self) -> u32 {
        self.hist_at(WV_DOM as usize)
    }
    pub fn step_issues(&self) -> u32 {
        self.hist_at(WV_STEP as usize)
    }
    pub fn text_issues(&self) -> u32 {
        self.hist_at(WV_TEXT as usize)
    }
    pub fn question_issues(&self) -> u32 {
        self.hist_at(WV_Q as usize)
    }
    pub fn bad_records(&self) -> u32 {
        self.hist_at(WV_BAD as usize)
    }
    pub fn case_at(&self, i: usize) -> Option<WireCase> {
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

    /// FNV-1a：覆盖样本、判据、观测位、直方、双摘要与计数。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF ^ TAG;
        let mut k = 0usize;
        while k < CAP {
            if let Some(c) = self.cases.get(k) {
                h = Self::twine(h, c.id as u64);
                h = Self::twine(h, c.q as u64);
                h = Self::twine(h, c.text as u64);
                h = Self::twine(h, c.dom as u64);
                h = Self::twine(h, c.step as u64);
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
        h = Self::twine(h, self.engine_digest);
        h = Self::twine(h, self.wire_digest);
        h = Self::twine(h, self.len as u64);
        h = Self::twine(h, self.attempts as u64);
        h = Self::twine(h, self.accepts as u64);
        h = Self::twine(h, self.rejects as u64);
        h = Self::twine(h, self.observed as u64);
        h = Self::twine(h, self.matched as u64);
        h = Self::twine(h, if self.frozen { 1 } else { 0 });
        h
    }

    /// 复算：引擎摘要、逐条判据、直方与接线摘要四路齐平。
    pub fn replay(&self) -> Result<u64, u32> {
        if self.len == 0 || self.observed != self.len as u32 || self.matched != self.len as u32 {
            return Err(WW_EUNOBS);
        }
        if self.accepts.wrapping_add(self.rejects) != self.attempts {
            return Err(WW_EBALANCE);
        }
        if self.hist_sum() != self.len as u32 {
            return Err(WW_EBALANCE);
        }
        if self.engine_digest != fold_engine() {
            return Err(WW_EMISMATCH);
        }
        let mut hist = [0u32; 6];
        let mut wd = FNV_OFF ^ TAG;
        let mut k = 0usize;
        while k < self.len {
            let c = match self.cases.get(k) {
                Some(v) => *v,
                None => return Err(WW_EBALANCE),
            };
            if self.seen.get(k) != Some(&true) {
                return Err(WW_EUNOBS);
            }
            let v = wire_verdict(&c);
            if self.verdicts.get(k) != Some(&v) {
                return Err(WW_EMISMATCH);
            }
            if let Some(h) = hist.get_mut(v as usize) {
                *h = h.wrapping_add(1);
            }
            if v == 0 {
                if let Some(text) = c.case_text() {
                    wd = hash_bytes(wd, text);
                    wd = (wd ^ c.dom as u64).wrapping_mul(FNV_PRIME);
                    wd = (wd ^ c.step as u64).wrapping_mul(FNV_PRIME);
                }
            }
            k = k.wrapping_add(1);
        }
        if wd != self.wire_digest {
            return Err(WW_EBALANCE);
        }
        k = 0;
        while k < 6 {
            if hist.get(k) != self.hist.get(k) {
                return Err(WW_EBALANCE);
            }
            k = k.wrapping_add(1);
        }
        Ok(self.fold())
    }

    /// 封册：先复算再落锁；二次封册只回错不加拒绝计数。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Err(WW_EFROZEN);
        }
        self.replay()?;
        self.frozen = true;
        let sealed = self.fold();
        self.root = Some(sealed);
        Ok(sealed)
    }
}

/// 十三条样本：八条手册案例零判据，五条各踩一种接线错误。
pub fn spec_of(id: usize) -> (WireCase, u32) {
    if id < 1 || id > SAMPLES {
        return (WireCase::blank(), WW_EID);
    }
    let (q, text, dom, step) = match id {
        1..=8 => {
            let t = (id - 1) as u32;
            let tx = match MANUAL.get(t as usize) {
                Some(v) => *v,
                None => "",
            };
            (Q_C1, t, classify(tx), step_of(tx))
        }
        9 => (2, 0, DOM_C5, ST_VER),
        10 => (Q_C1, 99, DOM_C5, ST_VER),
        11 => (Q_C1, 0, 4, ST_VER),
        12 => (Q_C1, 0, DOM_C1, ST_VER),
        _ => (Q_C1, 0, DOM_C5, ST_HOST),
    };
    let c = WireCase {
        id: id as u32,
        q,
        text,
        dom,
        step,
        hash: 0x1128_4000u64.wrapping_add(id as u64),
    };
    (c, wire_verdict(&c))
}

/// 逐条登记 + 观测；id 已在样本里对位。
pub fn build_ledger() -> WireLedger {
    let mut l = WireLedger::new();
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
    let mut a = WireLedger::new();
    let mut hi = WireCase::blank();
    hi.id = 2;
    hi.q = Q_C1;
    hi.dom = DOM_C5;
    hi.step = ST_VER;
    let _ = a.register(hi);
    let mut lo = WireCase::blank();
    lo.id = 1;
    lo.q = Q_C1;
    lo.dom = DOM_C5;
    lo.step = ST_VER;
    let bad_a = a.register(lo).err() == Some(WW_EORDER);

    let mut b = WireLedger::new();
    let one = spec_of(1).0;
    let _ = b.register(one);
    let bad_b = b.register(one).err() == Some(WW_EDUP);

    let mut c = WireLedger::new();
    let mut wide = WireCase::blank();
    wide.id = ID_MAX + 1;
    let bad_c = c.register(wide).err() == Some(WW_EID);

    let mut d = build_ledger();
    let _ = d.freeze();
    let mut late = WireCase::blank();
    late.id = 40;
    let bad_d = d.register(late).err() == Some(WW_EFROZEN);
    [bad_a, bad_b, bad_c, bad_d]
}

/// 十三条自检：前九项压引擎复用与八案接线，后四项压拒绝与封册。
pub fn run_unx11284_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11284");

    let layout = SEG_BASE == 0x1f540
        && WW_EID == SEG_BASE + 0x01
        && WW_EORDER == SEG_BASE + 0x02
        && WW_EDUP == SEG_BASE + 0x03
        && WW_EFULL == SEG_BASE + 0x04
        && WW_EFROZEN == SEG_BASE + 0x05
        && WW_EUNSEEN == SEG_BASE + 0x06
        && WW_EMISMATCH == SEG_BASE + 0x07
        && WW_EUNOBS == SEG_BASE + 0x08
        && WW_EDOBS == SEG_BASE + 0x09
        && WW_EBALANCE == SEG_BASE + 0x0a;
    s.add("S1-段布局与十个错误码", layout, "0x1f540 段十码连续");

    let mut engine_ok = RULES.len() == 7
        && DOM_C1 == 1
        && DOM_C2 == 2
        && DOM_C5 == 5
        && DOM_UNKNOWN == 0
        && DOMAINS.len() == 3;
    let mut k = 0usize;
    while k < MANUAL_COUNT {
        if let Some(t) = MANUAL.get(k) {
            if classify(t) != DOM_C5 {
                engine_ok = false;
            }
        }
        k = k.wrapping_add(1);
    }
    s.add("S2-七规则复用与八案全判 C5", engine_ok,
        "域值 1/2/5 与 F11214 同值，八条案例零例外落 C5");

    let want: [u32; MANUAL_COUNT] = [
        ST_VER, ST_VER, ST_SONAME, ST_LDSO, ST_CACHE, ST_PATH, ST_HOST, ST_VER,
    ];
    let mut step_ok = true;
    k = 0;
    while k < MANUAL_COUNT {
        let (t, w) = match (MANUAL.get(k), want.get(k)) {
            (Some(a), Some(b)) => (*a, *b),
            _ => ("", 0),
        };
        let got = step_of(t);
        if got != w || !STEPS.contains(&got) {
            step_ok = false;
        }
        k = k.wrapping_add(1);
    }
    s.add("S3-八案处置端逐一接线", step_ok,
        "版本两次、soname/装载器/缓存/路径/宿主各一次");

    let mut zero_ok = true;
    k = 0;
    while k < MANUAL_COUNT {
        let (c, _) = spec_of(k + 1);
        if wire_verdict(&c) != 0 || c.dom != DOM_C5 || c.q != Q_C1 {
            zero_ok = false;
        }
        k = k.wrapping_add(1);
    }
    s.add("S4-八案接线判据全零", zero_ok,
        "问题号、域、动作三面同时相符，冲突为零");

    let e1 = fold_engine();
    let e2 = fold_engine();
    let l8 = build_ledger();
    s.add("S5-引擎摘要封表", e1 == e2
        && e1 != 0
        && l8.engine_digest() == e1
        && l8.wire_digest() != 0,
        "归域表、案文表、处置表折叠稳定且与账本登记值相同");

    s.add("S6-处置选择非同名", step_of(MANUAL[3]) == ST_LDSO
        && step_of(MANUAL[5]) == ST_PATH
        && step_of(MANUAL[6]) == ST_HOST
        && step_of(MANUAL[7]) == ST_VER
        && step_of("plain text") == 0,
        "装载器/路径/宿主/版本各走各端，无关键词落 0");

    let bad_ok = spec_of(9).1 == WV_Q
        && spec_of(10).1 == WV_TEXT
        && spec_of(11).1 == WV_BAD
        && spec_of(12).1 == WV_DOM
        && spec_of(13).1 == WV_STEP;
    s.add("S7-五类接线错误各判一码", bad_ok,
        "问题号、案文、记录域非法、归域冲突、动作不符五码各一");

    let counts_ok = l8.len() == SAMPLES
        && l8.zero_verdicts() == 8
        && l8.conflicts() == 1
        && l8.step_issues() == 1
        && l8.text_issues() == 1
        && l8.question_issues() == 1
        && l8.bad_records() == 1
        && l8.hist_sum() == SAMPLES as u32;
    s.add("S8-十三样本落账配平", counts_ok,
        "八条零判据，五条错误码各一，直方合计 13");

    let mut wd = FNV_OFF ^ TAG;
    let mut k2 = 1usize;
    while k2 <= MANUAL_COUNT {
        let (c, _) = spec_of(k2);
        if let Some(t) = c.case_text() {
            wd = hash_bytes(wd, t);
            wd = (wd ^ c.dom as u64).wrapping_mul(FNV_PRIME);
            wd = (wd ^ c.step as u64).wrapping_mul(FNV_PRIME);
        }
        k2 = k2.wrapping_add(1);
    }
    let l9 = build_ledger();
    s.add("S9-接线摘要复算与折叠稳定", wd == l8.wire_digest()
        && l8.fold() == l9.fold()
        && l8.replay().is_ok()
        && l8.attempts() == SAMPLES as u32
        && l8.accepts() == SAMPLES as u32,
        "八条三元组逐字节复算，两次构建折叠同值");

    let mut o = WireLedger::new();
    let c1 = spec_of(1).0;
    let reg = o.register_ruled(c1).is_ok();
    let first = o.observe(c1.id, 0) == Ok(true);
    let twice = o.observe(c1.id, 0).err() == Some(WW_EDOBS);
    let unseen = o.observe(99, 0).err() == Some(WW_EUNSEEN);
    let mut o2 = WireLedger::new();
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
        let mut c = WireCase::blank();
        c.id = id;
        let _ = cap.register(c);
        id = id.wrapping_add(1);
    }
    let mut over = WireCase::blank();
    over.id = 17;
    let cap_ok = cap.len() == CAP
        && cap.register(over).err() == Some(WW_EFULL);
    s.add("S12-定容十六满表拒绝", cap_ok,
        "补足 16 条后第 17 条回 EFULL");

    let mut f = build_ledger();
    let d = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let rb = f.rejects();
    let again = match f.freeze() {
        Err(WW_EFROZEN) => true,
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
    fn domain_engine_matches_f11214() {
        assert_eq!(RULES.len(), 7);
        assert_eq!(DOM_C1, 1);
        assert_eq!(DOM_C2, 2);
        assert_eq!(DOM_C5, 5);
        assert_eq!(DOM_UNKNOWN, 0);
        assert_eq!(classify(MANUAL[0]), DOM_C5);
        assert_eq!(classify(MANUAL[7]), DOM_C5);
        assert_eq!(classify("runtime: rt_sigaction frame differs"), DOM_C2);
        assert_eq!(classify("loader: auxv bits absent"), DOM_C1);
        assert_eq!(classify("thread: set_robust_list EINVAL"), DOM_C1);
        assert_eq!(classify("nothing relevant here"), DOM_UNKNOWN);
    }

    #[test]
    fn step_table_wires_eight_cases() {
        let want = [ST_VER, ST_VER, ST_SONAME, ST_LDSO, ST_CACHE, ST_PATH, ST_HOST, ST_VER];
        let mut k = 0usize;
        while k < MANUAL_COUNT {
            assert_eq!(step_of(MANUAL[k]), want[k]);
            assert!(STEPS.contains(&want[k]));
            k += 1;
        }
        assert_eq!(step_of("ld.so.cache stale"), ST_CACHE);
        assert_eq!(step_of("plain text"), 0);
        assert!(!contains("abc", "abcd"));
        assert!(!contains("abc", ""));
        assert!(contains("abc", "bc"));
    }

    #[test]
    fn verdicts_cover_good_and_bad() {
        let mut zeros = 0u32;
        let mut i = 1usize;
        while i <= 8 {
            let (c, expect) = spec_of(i);
            assert_eq!(expect, 0);
            assert_eq!(wire_verdict(&c), 0);
            assert_eq!(c.dom, DOM_C5);
            zeros += 1;
            i += 1;
        }
        assert_eq!(zeros, 8);
        assert_eq!(spec_of(9).1, WV_Q);
        assert_eq!(spec_of(10).1, WV_TEXT);
        assert_eq!(spec_of(11).1, WV_BAD);
        assert_eq!(spec_of(12).1, WV_DOM);
        assert_eq!(spec_of(13).1, WV_STEP);
        assert_eq!(spec_of(0).1, WW_EID);
        assert_eq!(spec_of(14).1, WW_EID);
    }

    #[test]
    fn ledger_balances_and_replays() {
        let l = build_ledger();
        assert_eq!(l.len(), 13);
        assert_eq!(l.zero_verdicts(), 8);
        assert_eq!(l.conflicts(), 1);
        assert_eq!(l.hist_sum(), 13);
        assert_eq!(l.attempts(), 13);
        assert_eq!(l.accepts(), 13);
        assert_eq!(l.observed(), 13);
        assert_eq!(l.matched(), 13);
        assert_eq!(l.engine_digest(), fold_engine());
        match l.replay() {
            Ok(v) => assert_eq!(v, l.fold()),
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn observe_refuses_and_freezes() {
        let mut o = WireLedger::new();
        let c = spec_of(1).0;
        assert!(o.register_ruled(c).is_ok());
        assert_eq!(o.observe(c.id, 0), Ok(true));
        assert_eq!(o.observe(c.id, 0), Err(WW_EDOBS));
        assert_eq!(o.observe(42, 0), Err(WW_EUNSEEN));

        let four = inject_four();
        assert!(four[0] && four[1] && four[2] && four[3]);

        let mut f = build_ledger();
        let pre = f.fold();
        assert!(f.freeze().is_ok());
        assert_eq!(f.root(), Some(f.fold()));
        assert!(f.fold() != pre);
        let rb = f.rejects();
        assert_eq!(f.freeze().err(), Some(WW_EFROZEN));
        assert_eq!(f.rejects(), rb);
        assert!(f.replay().is_ok());
    }

    #[test]
    fn checks_are_green() {
        let s = run_unx11284_checks();
        assert!(s.all_passed());
        assert_eq!(s.len(), 13);
    }
}
