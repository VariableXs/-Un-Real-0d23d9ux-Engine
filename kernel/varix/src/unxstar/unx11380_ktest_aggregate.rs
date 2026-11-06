//! UNX-F11380 B09 批 ktest 断言集聚合
//!
//! 本批十九条判据要聚合成一套 ktest 断言面，一次命令全跑，全部绿了才许
//! 宣称「本批通过」。聚合器只认**先登记、后开跑、再裁定**的次序：
//!
//! 1. **登记闸门**：判据号必须落在有效区间；开跑之后不许再补条目，补进来
//!    的条目没跑过，却会被算进通过数。
//! 2. **豁免闸门**：动态判据允许豁免，但必须**先跑过**、必须是动态位、
//!    且只能豁免一次；固定判据的失败不许拿豁免抹掉。
//! 3. **裁定闸门**：没开跑不许裁定；只要还剩一条未豁免的失败，整批就是
//!    `EFAIL`，通过率不许报 100%。
//!
//! 闸门次序：
//! - `add`：判据号越界 → 已开跑 → 重复登记 → 表满；
//! - `run`：空表 → 重复开跑；
//! - `verdict`：尚未开跑 → 未豁免失败；
//! - `waive`：尚未开跑 → 未知判据 → 非动态判据 → 重复豁免；
//! - `passing`：尚未开跑 → 未豁免失败。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xedc0;

/// 判据号越界。
pub const KT_ERANGE: u32 = SEG_BASE + 0x01;
/// 重复登记。
pub const KT_EDUP: u32 = SEG_BASE + 0x02;
/// 断言表已满。
pub const KT_EFULL: u32 = SEG_BASE + 0x03;
/// 空表或尚未开跑。
pub const KT_EGAP: u32 = SEG_BASE + 0x04;
/// 仍有未豁免的失败。
pub const KT_EFAIL: u32 = SEG_BASE + 0x05;
/// 未知判据。
pub const KT_EUNK: u32 = SEG_BASE + 0x06;
/// 阶段不符。
pub const KT_EPHASE: u32 = SEG_BASE + 0x07;
/// 非动态判据不许豁免。
pub const KT_EDYN: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const KT_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const KT_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const KT_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const KT_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const KT_ENOBS: u32 = SEG_BASE + 0x0d;

/// 断言表容量。
pub const CAP: usize = 24;
/// 本批判据总条数。
pub const WANT: usize = 19;
/// 账本格数。
pub const SAMPLES: usize = 14;
/// 判据号上界。
pub const ID_MAX: u32 = 64;

/// 操作码：登记断言。
pub const OP_ADD: u8 = 1;
/// 操作码：开跑。
pub const OP_RUN: u8 = 2;
/// 操作码：裁定。
pub const OP_VERDICT: u8 = 3;
/// 操作码：豁免。
pub const OP_WAIVE: u8 = 4;
/// 操作码：观察全绿条数。
pub const OP_PASS: u8 = 5;

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折入一个 64 位字。
pub fn fold(h: u64, v: u64) -> u64 {
    let mut x = h;
    let mut n = v;
    let mut i = 0u32;
    while i < 8 {
        x ^= n & 0xff;
        x = x.wrapping_mul(FNV_PRIME);
        n >>= 8;
        i += 1;
    }
    x
}

/// 一条断言。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Case {
    /// 判据号。
    pub id: u32,
    /// 实测值。
    pub got: u32,
    /// 期望值。
    pub want: u32,
    /// 是否动态判据。
    pub dynamic: bool,
    /// 是否已豁免。
    pub waived: bool,
}

/// 断言集聚合器。
#[derive(Clone, Copy, Debug)]
pub struct Suite {
    c: [Case; CAP],
    n: usize,
    ran: bool,
    waived: u32,
}

impl Suite {
    /// 空表。
    pub const fn new() -> Suite {
        Suite {
            c: [Case { id: 0, got: 0, want: 0, dynamic: false, waived: false }; CAP],
            n: 0,
            ran: false,
            waived: 0,
        }
    }

    /// 断言条数。
    pub fn count(&self) -> usize {
        self.n
    }

    /// 是否已开跑。
    pub fn ran(&self) -> bool {
        self.ran
    }

    /// 已豁免条数。
    pub fn waived(&self) -> u32 {
        self.waived
    }

    /// 第 i 条。
    pub fn at(&self, i: usize) -> Option<Case> {
        if i >= self.n {
            return None;
        }
        self.c.get(i).copied()
    }

    fn find(&self, id: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.n {
            match self.c.get(i) {
                Some(e) => {
                    if e.id == id {
                        return Some(i);
                    }
                }
                None => return None,
            }
            i += 1;
        }
        None
    }

    /// 登记一条断言，返回累计条数。
    pub fn add(&mut self, id: u32, got: u32, want: u32, dynamic: bool) -> Result<usize, u32> {
        if id == 0 || id > ID_MAX {
            return Err(KT_ERANGE);
        }
        if self.ran {
            return Err(KT_EPHASE);
        }
        if self.find(id).is_some() {
            return Err(KT_EDUP);
        }
        if self.n >= CAP {
            return Err(KT_EFULL);
        }
        if let Some(slot) = self.c.get_mut(self.n) {
            *slot = Case { id, got, want, dynamic, waived: false };
        }
        self.n += 1;
        Ok(self.n)
    }

    /// 开跑全部断言，返回当场通过的条数。
    pub fn run(&mut self) -> Result<u32, u32> {
        if self.n == 0 {
            return Err(KT_EGAP);
        }
        if self.ran {
            return Err(KT_EPHASE);
        }
        self.ran = true;
        let mut pass = 0u32;
        let mut i = 0usize;
        while i < self.n {
            if let Some(e) = self.c.get(i) {
                if e.got == e.want {
                    pass = pass.saturating_add(1);
                }
            }
            i += 1;
        }
        Ok(pass)
    }

    /// 未豁免的失败条数。
    fn open_failures(&self) -> u32 {
        let mut bad = 0u32;
        let mut i = 0usize;
        while i < self.n {
            if let Some(e) = self.c.get(i) {
                if e.got != e.want && !e.waived {
                    bad = bad.saturating_add(1);
                }
            }
            i += 1;
        }
        bad
    }

    /// 裁定整批：有未豁免失败即拒。
    pub fn verdict(&self) -> Result<u32, u32> {
        if !self.ran {
            return Err(KT_EGAP);
        }
        if self.open_failures() != 0 {
            return Err(KT_EFAIL);
        }
        Ok(self.n as u32)
    }

    /// 豁免一条动态断言，返回累计豁免数。
    pub fn waive(&mut self, id: u32) -> Result<u32, u32> {
        if !self.ran {
            return Err(KT_EPHASE);
        }
        let at = match self.find(id) {
            Some(v) => v,
            None => return Err(KT_EUNK),
        };
        let row = match self.c.get(at) {
            Some(v) => *v,
            None => return Err(KT_EUNK),
        };
        if !row.dynamic {
            return Err(KT_EDYN);
        }
        if row.waived {
            return Err(KT_EDUP);
        }
        if let Some(slot) = self.c.get_mut(at) {
            slot.waived = true;
        }
        self.waived = self.waived.saturating_add(1);
        Ok(self.waived)
    }

    /// 全绿条数：通过或已豁免的断言；还有失败就拒。
    pub fn passing(&self) -> Result<u32, u32> {
        if !self.ran {
            return Err(KT_EGAP);
        }
        if self.open_failures() != 0 {
            return Err(KT_EFAIL);
        }
        let mut green = 0u32;
        let mut i = 0usize;
        while i < self.n {
            if let Some(e) = self.c.get(i) {
                if e.got == e.want || e.waived {
                    green = green.saturating_add(1);
                }
            }
            i += 1;
        }
        Ok(green)
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.n {
            if let Some(e) = self.c.get(i) {
                h = fold(h, e.id as u64);
                h = fold(h, e.got as u64);
                h = fold(h, e.want as u64);
                h = fold(h, e.dynamic as u64);
                h = fold(h, e.waived as u64);
            }
            i += 1;
        }
        h = fold(h, self.ran as u64);
        fold(h, self.waived as u64)
    }

    /// 不变式自检。
    pub fn consistent(&self) -> bool {
        if self.n > CAP {
            return false;
        }
        let mut w = 0u32;
        let mut i = 0usize;
        while i < self.n {
            let e = match self.c.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if e.id == 0 || e.id > ID_MAX {
                return false;
            }
            if e.waived {
                if !e.dynamic || !self.ran {
                    return false;
                }
                w = w.saturating_add(1);
            }
            let mut j = 0usize;
            while j < i {
                match self.c.get(j) {
                    Some(p) => {
                        if p.id == e.id {
                            return false;
                        }
                    }
                    None => return false,
                }
                j += 1;
            }
            i += 1;
        }
        w == self.waived
    }
}

/// 一个账本样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    /// 槽位。
    pub slot: u8,
    /// 操作码。
    pub op: u8,
    /// 是否成功。
    pub ok: bool,
    /// 错误码。
    pub code: u32,
    /// 观测值。
    pub value: u32,
}

impl Sample {
    /// 空白格。
    pub const fn blank() -> Sample {
        Sample { slot: 0, op: 0, ok: false, code: 0, value: 0 }
    }
}

/// `Result<usize, u32>` → 样本。
fn cell(slot: u8, op: u8, r: Result<usize, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v as u32 },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// `Result<u32, u32>` → 样本。
fn word_cell(slot: u8, op: u8, r: Result<u32, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// 样本账本。
#[derive(Clone, Copy, Debug)]
pub struct Ledger {
    rows: [Sample; SAMPLES],
    len: usize,
    root: u64,
    frozen: bool,
    rejects: u32,
}

impl Ledger {
    /// 空账本。
    pub const fn new() -> Ledger {
        Ledger { rows: [Sample::blank(); SAMPLES], len: 0, root: 0, frozen: false, rejects: 0 }
    }

    /// 登记一格。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects += 1;
            return Err(KT_ESLOT);
        }
        if let Some(dst) = self.rows.get_mut(self.len) {
            *dst = s;
        }
        self.len += 1;
        Ok(())
    }

    /// 格数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 第 i 格。
    pub fn sample(&self, i: usize) -> Option<Sample> {
        if i >= self.len {
            return None;
        }
        self.rows.get(i).copied()
    }

    /// 成功格数。
    pub fn ok_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
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

    /// 失败格数。
    pub fn err_count(&self) -> usize {
        self.len - self.ok_count()
    }

    /// 对账。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != 8 || self.err_count() != 6 {
            return Err(KT_EBAL);
        }
        const VALS: [u32; 8] = [1, 2, 3, 1, 1, 2, 3, 3];
        const CODES: [u32; 6] = [KT_EDUP, KT_ERANGE, KT_EGAP, KT_EFAIL, KT_EDYN, KT_EUNK];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(KT_EBAL),
            };
            if s.slot as usize != i {
                return Err(KT_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(KT_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(KT_EVALUE);
                        }
                    }
                    None => return Err(KT_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(KT_EVALUE);
                        }
                    }
                    None => return Err(KT_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(KT_EBAL);
        }
        Ok(())
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
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
        fold(h, self.rejects as u64)
    }

    /// 冻结。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(KT_EBAL);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        if fresh.seal() == self.seal() {
            Ok(fresh.seal())
        } else {
            Err(KT_ESEAL)
        }
    }

    /// 冻结指纹。
    pub fn root(&self) -> u64 {
        self.root
    }

    /// 是否冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 拒绝计数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }
}

/// 十四格账本：三条登记 → 重复/越界 → 未跑裁定 → 开跑 → 未豁免失败 →
/// 两次豁免 / 非动态 / 未知 / 重复 → 裁定全绿 → 全绿条数。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut s = Suite::new();

    let _ = l.register(cell(0, OP_ADD, s.add(1, 1, 1, false)));
    let _ = l.register(cell(1, OP_ADD, s.add(2, 2, 3, true)));
    let _ = l.register(cell(2, OP_ADD, s.add(3, 4, 5, true)));
    let _ = l.register(cell(3, OP_ADD, s.add(1, 0, 0, false)));
    let _ = l.register(cell(4, OP_ADD, s.add(0, 0, 0, false)));
    let _ = l.register(word_cell(5, OP_VERDICT, s.verdict()));
    let _ = l.register(word_cell(6, OP_RUN, s.run()));
    let _ = l.register(word_cell(7, OP_VERDICT, s.verdict()));
    let _ = l.register(word_cell(8, OP_WAIVE, s.waive(2)));
    let _ = l.register(word_cell(9, OP_WAIVE, s.waive(1)));
    let _ = l.register(word_cell(10, OP_WAIVE, s.waive(9)));
    let _ = l.register(word_cell(11, OP_WAIVE, s.waive(3)));
    let _ = l.register(word_cell(12, OP_VERDICT, s.verdict()));
    let _ = l.register(word_cell(13, OP_PASS, s.passing()));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：三条断言，一条动态失败先拒后豁免，全绿收批。
    let mut a = Suite::new();
    let a1 = a.add(1, 5, 5, false);
    let a2 = a.add(2, 6, 7, true);
    let a3 = a.add(3, 8, 8, false);
    let r = a.run();
    let early = a.verdict() == Err(KT_EFAIL);
    let w = a.waive(2);
    let v = a.verdict();
    let p = a.passing();
    out[1] = a1 == Ok(1)
        && a2 == Ok(2)
        && a3 == Ok(3)
        && r == Ok(2)
        && early
        && w == Ok(1)
        && v == Ok(3)
        && p == Ok(3)
        && a.consistent();

    // 二路：八类失败各自可辨，失败不改断言表。
    let mut b = Suite::new();
    let range = b.add(0, 1, 1, false) == Err(KT_ERANGE);
    let high = b.add(ID_MAX + 1, 1, 1, false) == Err(KT_ERANGE);
    let untouched = b.count() == 0;
    let first = b.add(1, 1, 2, false) == Ok(1);
    let second = b.add(2, 3, 3, false) == Ok(2);
    let third = b.add(3, 4, 5, true) == Ok(3);
    let dup = b.add(1, 0, 0, false) == Err(KT_EDUP);
    let empty_run = Suite::new().run() == Err(KT_EGAP);
    let not_yet = b.verdict() == Err(KT_EGAP);
    let h0 = b.seal();
    let phase_waive = b.waive(1) == Err(KT_EPHASE) && b.seal() == h0;
    let ran = b.run() == Ok(1);
    let h1 = b.seal();
    let phase_add = b.add(4, 0, 0, false) == Err(KT_EPHASE) && b.seal() == h1;
    let again = b.run() == Err(KT_EPHASE) && b.seal() == h1;
    let fixed = b.waive(1) == Err(KT_EDYN) && b.seal() == h1;
    let unknown = b.waive(9) == Err(KT_EUNK) && b.seal() == h1;
    let fail = b.verdict() == Err(KT_EFAIL);
    let w1 = b.waive(3) == Ok(1);
    let h2 = b.seal();
    let wdup = b.waive(3) == Err(KT_EDUP) && b.seal() == h2;
    let mut c = Suite::new();
    let mut i = 1u32;
    while (i as usize) <= CAP {
        let _ = c.add(i, i, i, false);
        i += 1;
    }
    let full = c.count() == CAP && c.add((CAP + 1) as u32, 0, 0, false) == Err(KT_EFULL);
    out[2] = range
        && high
        && untouched
        && first
        && second
        && third
        && dup
        && empty_run
        && not_yet
        && phase_waive
        && ran
        && phase_add
        && again
        && fixed
        && unknown
        && fail
        && w1
        && wdup
        && full
        && b.consistent()
        && c.consistent();

    // 三路：表满硬上限，满表全绿才算通过。
    let mut q = Suite::new();
    let mut i = 1u32;
    while (i as usize) <= CAP {
        let _ = q.add(i, i, i, false);
        i += 1;
    }
    let q1 = q.count() == CAP;
    let q2 = q.add((CAP + 1) as u32, 0, 0, false) == Err(KT_EFULL);
    let q3 = q.run() == Ok(CAP as u32);
    let q4 = q.add(1, 1, 1, false) == Err(KT_EPHASE);
    let q5 = q.verdict() == Ok(CAP as u32);
    let q6 = q.passing() == Ok(CAP as u32);
    out[3] = q1 && q2 && q3 && q4 && q5 && q6 && q.consistent();

    out
}

/// 十三项判据。
pub fn run_unx11380_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11380.ktest_aggregate");
    let l = build_ledger();

    s.add_dyn(
        "KT1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "KT2-本批十九条全绿",
        {
            let mut a = Suite::new();
            let mut i = 1u32;
            let mut ok = true;
            while (i as usize) <= WANT {
                if a.add(i, i, i, false) != Ok(i as usize) {
                    ok = false;
                }
                i += 1;
            }
            ok && a.count() == WANT
                && a.run() == Ok(WANT as u32)
                && a.verdict() == Ok(WANT as u32)
                && a.passing() == Ok(WANT as u32)
                && a.consistent()
        },
        "static",
    );

    s.add(
        "KT3-未跑不许裁定",
        {
            let mut a = Suite::new();
            let _ = a.add(1, 1, 1, false);
            let early = a.verdict() == Err(KT_EGAP);
            let none = Suite::new().run() == Err(KT_EGAP);
            early && none && a.seal() != 0 && l.sample(5).map(|x| x.code) == Some(KT_EGAP)
        },
        "static",
    );

    s.add(
        "KT4-未跑不许豁免",
        {
            let mut a = Suite::new();
            let _ = a.add(1, 1, 2, true);
            let early = a.waive(1) == Err(KT_EPHASE);
            let h = a.seal();
            let phase = a.waive(1) == Err(KT_EPHASE) && a.seal() == h;
            let _ = a.run();
            early && phase && a.waive(1) == Ok(1) && a.consistent()
        },
        "static",
    );

    s.add(
        "KT5-非动态不许豁免",
        {
            let mut a = Suite::new();
            let _ = a.add(1, 1, 2, false);
            let _ = a.add(2, 3, 3, false);
            let _ = a.run();
            let h = a.seal();
            a.waive(1) == Err(KT_EDYN)
                && a.seal() == h
                && a.verdict() == Err(KT_EFAIL)
                && a.consistent()
                && l.sample(9).map(|x| x.code) == Some(KT_EDYN)
        },
        "static",
    );

    s.add(
        "KT6-重复豁免被拒",
        {
            let mut a = Suite::new();
            let _ = a.add(1, 1, 2, true);
            let _ = a.run();
            let first = a.waive(1) == Ok(1);
            let h = a.seal();
            let again = a.waive(1) == Err(KT_EDUP) && a.seal() == h;
            first && again && a.waived() == 1 && a.verdict() == Ok(1) && a.consistent()
        },
        "static",
    );

    s.add(
        "KT7-未知判据豁免被拒",
        {
            let mut a = Suite::new();
            let _ = a.add(1, 1, 1, true);
            let _ = a.run();
            let h = a.seal();
            a.waive(9) == Err(KT_EUNK)
                && a.seal() == h
                && a.waived() == 0
                && a.verdict() == Ok(1)
                && l.sample(10).map(|x| x.code) == Some(KT_EUNK)
        },
        "static",
    );

    s.add(
        "KT8-未豁免失败拒绿",
        {
            let mut a = Suite::new();
            let _ = a.add(1, 1, 2, false);
            let _ = a.add(2, 3, 4, true);
            let _ = a.add(3, 5, 5, false);
            let _ = a.run();
            let h = a.seal();
            a.verdict() == Err(KT_EFAIL)
                && a.passing() == Err(KT_EFAIL)
                && a.seal() == h
                && a.waive(2) == Ok(1)
                && a.verdict() == Err(KT_EFAIL)
                && a.passing() == Err(KT_EFAIL)
                && a.consistent()
                && l.sample(7).map(|x| x.code) == Some(KT_EFAIL)
        },
        "static",
    );

    s.add(
        "KT9-判据号越界被拒",
        {
            let mut a = Suite::new();
            a.add(0, 1, 1, false) == Err(KT_ERANGE)
                && a.add(ID_MAX + 1, 1, 1, false) == Err(KT_ERANGE)
                && a.count() == 0
                && l.sample(4).map(|x| x.code) == Some(KT_ERANGE)
        },
        "static",
    );

    s.add(
        "KT10-重复登记被拒",
        {
            let mut a = Suite::new();
            let _ = a.add(1, 2, 3, true);
            let h = a.seal();
            a.add(1, 0, 0, false) == Err(KT_EDUP)
                && a.seal() == h
                && a.count() == 1
                && l.sample(3).map(|x| x.code) == Some(KT_EDUP)
        },
        "static",
    );

    s.add(
        "KT11-跑后不许再登记",
        {
            let mut a = Suite::new();
            let _ = a.add(1, 1, 1, false);
            let _ = a.run();
            let h = a.seal();
            let late = a.add(2, 2, 2, false) == Err(KT_EPHASE);
            let again = a.run() == Err(KT_EPHASE);
            late && again && a.count() == 1 && a.seal() == h && a.consistent()
        },
        "static",
    );

    s.add(
        "KT12-满表硬上限",
        {
            let mut a = Suite::new();
            let mut i = 1u32;
            while (i as usize) <= CAP {
                let _ = a.add(i, i, i, false);
                i += 1;
            }
            a.count() == CAP
                && a.add((CAP + 1) as u32, 0, 0, false) == Err(KT_EFULL)
                && a.run() == Ok(CAP as u32)
                && a.verdict() == Ok(CAP as u32)
                && a.consistent()
        },
        "static",
    );

    s.add(
        "KT13-冻结重放四注入",
        {
            let mut f = build_ledger();
            let rep = f.replay();
            let one = f.freeze();
            let two = f.freeze();
            let inj = inject_four();
            one.is_ok()
                && one == rep
                && two.is_err()
                && f.is_frozen()
                && f.root() != 0
                && f.rejects() == 1
                && inj == [true; 4]
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
        assert_eq!(l.rejects(), 0);
        assert!(l.audit().is_ok());
        let mut i = 0usize;
        while i < l.len() {
            assert_eq!(l.sample(i).map(|x| x.slot as usize), Some(i));
            i += 1;
        }
    }

    #[test]
    fn checks_pass() {
        let s = run_unx11380_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn batch_all_green() {
        let mut a = Suite::new();
        let mut i = 1u32;
        while (i as usize) <= WANT {
            assert_eq!(a.add(i, i, i, false), Ok(i as usize));
            i += 1;
        }
        assert_eq!(a.run(), Ok(WANT as u32));
        assert_eq!(a.verdict(), Ok(WANT as u32));
        assert_eq!(a.passing(), Ok(WANT as u32));
        assert!(a.consistent());
    }

    #[test]
    fn phase_gates() {
        let mut a = Suite::new();
        assert_eq!(a.run(), Err(KT_EGAP));
        assert_eq!(a.add(1, 1, 1, false), Ok(1));
        assert_eq!(a.verdict(), Err(KT_EGAP));
        assert_eq!(a.run(), Ok(1));
        assert_eq!(a.run(), Err(KT_EPHASE));
        assert_eq!(a.add(2, 2, 2, false), Err(KT_EPHASE));
        assert_eq!(a.verdict(), Ok(1));
        assert!(a.consistent());
    }

    #[test]
    fn waiver_rules() {
        let mut a = Suite::new();
        assert_eq!(a.add(1, 1, 2, false), Ok(1));
        assert_eq!(a.add(2, 2, 3, true), Ok(2));
        assert_eq!(a.waive(1), Err(KT_EPHASE));
        assert_eq!(a.run(), Ok(0));
        assert_eq!(a.waive(1), Err(KT_EDYN));
        assert_eq!(a.waive(9), Err(KT_EUNK));
        assert_eq!(a.verdict(), Err(KT_EFAIL));
        assert_eq!(a.waive(2), Ok(1));
        assert_eq!(a.waive(2), Err(KT_EDUP));
        assert_eq!(a.verdict(), Err(KT_EFAIL));
        assert_eq!(a.passing(), Err(KT_EFAIL));
        assert!(a.consistent());
    }

    #[test]
    fn capacity_limit() {
        let mut a = Suite::new();
        let mut i = 1u32;
        while (i as usize) <= CAP {
            assert_eq!(a.add(i, i, i, false), Ok(i as usize));
            i += 1;
        }
        assert_eq!(a.add((CAP + 1) as u32, 0, 0, false), Err(KT_EFULL));
        assert_eq!(a.count(), CAP);
        assert_eq!(a.run(), Ok(CAP as u32));
        assert_eq!(a.passing(), Ok(CAP as u32));
        assert!(a.consistent());
    }

    #[test]
    fn inject_four_holds() {
        assert_eq!(inject_four(), [true; 4]);
    }
}
