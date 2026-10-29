//! UNX-F11152 B33 清账（LTP 对照判据终重放）（0x1f340 段）
//!
//! 纯功能模型：B33 批为 LTP 对照批，每个用例同时带「预期态」与「实测态」，态域
//! 只有通过/失败/跳过三值。终重放要求三态账逐条对平——预期与实测逐例相等、
//! 三态计数恰为 6/2/2、判据合计 20 条；终值冻结后 B33 不再收例。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1f340;
/// 用例号越界（只认 0..10）
pub const LT_ENO: u32 = SEG_BASE + 0x01;
/// 用例重记
pub const LT_EDUP: u32 = SEG_BASE + 0x02;
/// 用例序倒挂
pub const LT_EORDER: u32 = SEG_BASE + 0x03;
/// 态值越出三态（形状违规）
pub const LT_ESTATE: u32 = SEG_BASE + 0x04;
/// 十例未齐即清账
pub const LT_ESHORT: u32 = SEG_BASE + 0x05;
/// 三态计数偏离口径
pub const LT_ECOUNT: u32 = SEG_BASE + 0x06;
/// 预期与实测不等
pub const LT_EMISMATCH: u32 = SEG_BASE + 0x07;
/// 终值已锁定
pub const LT_EFROZEN: u32 = SEG_BASE + 0x08;

/// 用例数。
pub const CASES: usize = 10;
/// 三态：通过。
pub const ST_PASS: u8 = 0;
/// 三态：失败。
pub const ST_FAIL: u8 = 1;
/// 三态：跳过。
pub const ST_SKIP: u8 = 2;
/// 态域宽度。
pub const STATES: u8 = 3;
/// 每例判据条数（态合法 + 对照相等）。
pub const CRITERIA_PER_CASE: u32 = 2;
/// 本批判据合计。
pub const B33_CRITERIA: u32 = 20;
/// 通过例数口径。
pub const WANT_PASS: u32 = 6;
/// 失败例数口径。
pub const WANT_FAIL: u32 = 2;
/// 跳过例数口径。
pub const WANT_SKIP: u32 = 2;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 一例 LTP 对照。
#[derive(Clone, Copy, Debug)]
pub struct LtpCase {
    pub id: u8,
    pub expect: u8,
    pub actual: u8,
}

impl LtpCase {
    /// 预期与实测是否相等。
    pub fn matched(&self) -> bool {
        self.expect == self.actual
    }

    /// 两态是否都落在三态域内。
    pub fn states_valid(&self) -> bool {
        self.expect < STATES && self.actual < STATES
    }
}

/// B33 LTP 三态对照账。
#[derive(Clone, Debug)]
pub struct LtpLedger {
    cases: Vec<LtpCase>,
    settled: Option<u64>,
    frozen: bool,
    pub accepts: u64,
    pub rejects: u64,
}

impl LtpLedger {
    /// 空账。
    pub fn new() -> LtpLedger {
        LtpLedger {
            cases: Vec::new(),
            settled: None,
            frozen: false,
            accepts: 0,
            rejects: 0,
        }
    }

    /// 已记例数。
    pub fn len(&self) -> usize {
        self.cases.len()
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.cases.is_empty()
    }

    /// 取某例。
    pub fn case(&self, id: usize) -> Option<LtpCase> {
        let mut i = 0usize;
        while i < self.cases.len() {
            if self.cases[i].id as usize == id {
                return Some(self.cases[i]);
            }
            i = i.wrapping_add(1);
        }
        None
    }

    /// 记一例：冻结 → 越界 → 重记 → 倒序 → 三态形状。
    ///
    /// 重记先于倒序：同一实例再来一次只报 LT_EDUP；态值越界单独报 LT_ESTATE。
    pub fn record(&mut self, id: usize, expect: u8, actual: u8) -> Result<u64, u32> {
        if self.frozen || self.settled.is_some() {
            return self.fail(LT_EFROZEN);
        }
        if id >= CASES {
            return self.fail(LT_ENO);
        }
        if self.case(id).is_some() {
            return self.fail(LT_EDUP);
        }
        if let Some(last) = self.cases.last() {
            if id <= last.id as usize {
                return self.fail(LT_EORDER);
            }
        }
        if expect >= STATES || actual >= STATES {
            return self.fail(LT_ESTATE);
        }
        self.cases.push(LtpCase {
            id: id as u8,
            expect,
            actual,
        });
        self.accepts = self.accepts.wrapping_add(1);
        Ok(self.fold())
    }

    /// 记账一次拒止。
    fn fail(&mut self, code: u32) -> Result<u64, u32> {
        self.rejects = self.rejects.wrapping_add(1);
        Err(code)
    }
}

impl LtpLedger {
    /// 某态例数。
    pub fn count_state(&self, state: u8) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.cases.len() {
            if self.cases[i].actual == state {
                n = n.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 对照失配例数。
    pub fn mismatches(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.cases.len() {
            if !self.cases[i].matched() {
                n = n.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 三态计数三元组。
    pub fn tri_counts(&self) -> (u32, u32, u32) {
        (
            self.count_state(ST_PASS),
            self.count_state(ST_FAIL),
            self.count_state(ST_SKIP),
        )
    }

    /// 三态计数是否合口径。
    pub fn counts_canonical(&self) -> bool {
        let (p, f, s) = self.tri_counts();
        p == WANT_PASS && f == WANT_FAIL && s == WANT_SKIP
    }

    /// 缺例表。
    pub fn gaps(&self) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        let mut id = 0usize;
        while id < CASES {
            if self.case(id).is_none() {
                out.push(id as u32);
            }
            id = id.wrapping_add(1);
        }
        out
    }

    /// 用例序严格递增。
    pub fn order_strict(&self) -> bool {
        let mut i = 1usize;
        while i < self.cases.len() {
            if self.cases[i].id <= self.cases[i - 1].id {
                return false;
            }
            i = i.wrapping_add(1);
        }
        true
    }

    /// 判据重放条数：每例 2 条（态合法 + 对照相等）。
    pub fn criteria_replayed(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.cases.len() {
            if self.cases[i].states_valid() {
                n = n.wrapping_add(1);
            }
            if self.cases[i].matched() {
                n = n.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 对照账指纹。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.cases.len() {
            let c = self.cases[i];
            h = (h ^ c.id as u64).wrapping_mul(FNV_PRIME);
            h = (h ^ c.expect as u64).wrapping_mul(FNV_PRIME);
            h = (h ^ c.actual as u64).wrapping_mul(FNV_PRIME);
            i = i.wrapping_add(1);
        }
        h = (h ^ self.cases.len() as u64).wrapping_mul(FNV_PRIME);
        let (p, f, s) = self.tri_counts();
        h = (h ^ p as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ f as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ s as u64).wrapping_mul(FNV_PRIME);
        h
    }

    /// 终清账：十例齐 → 三态计数 → 对照无失配 → 判据满 20。
    pub fn settle(&mut self) -> Result<u64, u32> {
        if self.frozen || self.settled.is_some() {
            return self.fail(LT_EFROZEN);
        }
        if self.cases.len() != CASES || !self.gaps().is_empty() {
            return self.fail(LT_ESHORT);
        }
        if !self.counts_canonical() {
            return self.fail(LT_ECOUNT);
        }
        if self.mismatches() != 0 {
            return self.fail(LT_EMISMATCH);
        }
        if self.criteria_replayed() != B33_CRITERIA {
            return self.fail(LT_EMISMATCH);
        }
        let v = self.fold();
        self.settled = Some(v);
        Ok(v)
    }

    /// 冻结：锁当前值，幂等。
    pub fn freeze(&mut self) -> Option<u64> {
        if self.frozen {
            return self.settled;
        }
        if self.settled.is_none() {
            self.settled = Some(self.fold());
        }
        self.frozen = true;
        self.settled
    }

    /// 终值。
    pub fn settled_value(&self) -> Option<u64> {
        self.settled
    }

    /// 是否冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }
}

/// 建账：六例通过、两例失败、两例跳过，预期与实测逐例相等，再清账。
pub fn build_b33() -> (LtpLedger, bool) {
    let mut l = LtpLedger::new();
    let mut id = 0usize;
    while id < CASES {
        let st = if id < 6 {
            ST_PASS
        } else if id < 8 {
            ST_FAIL
        } else {
            ST_SKIP
        };
        if l.record(id, st, st).is_err() {
            return (l, false);
        }
        id = id.wrapping_add(1);
    }
    let ok = l.settle().is_ok();
    (l, ok)
}

/// 四类注入：用例越界 / 重记与倒序 / 三态越界 / 终账后再记。
pub fn inject_four() -> [bool; 4] {
    // ① 用例号越界
    let mut a0 = LtpLedger::new();
    let r0 = a0.record(CASES, ST_PASS, ST_PASS).err() == Some(LT_ENO)
        && a0
            .record(CASES.wrapping_add(5), ST_SKIP, ST_SKIP)
            .err()
            == Some(LT_ENO)
        && a0.is_empty();

    // ② 重记与倒序
    let mut a1 = LtpLedger::new();
    let s0 = a1.record(0, ST_PASS, ST_PASS).is_ok();
    let s1 = a1.record(1, ST_PASS, ST_PASS).is_ok();
    let dup = s0 && s1 && a1.record(0, ST_PASS, ST_PASS).err() == Some(LT_EDUP);
    let mut a2 = LtpLedger::new();
    let _ = a2.record(3, ST_PASS, ST_PASS);
    let order = a2.record(2, ST_PASS, ST_PASS).err() == Some(LT_EORDER);
    let r1 = dup && order;

    // ③ 三态越界与计数偏离
    let mut a3 = LtpLedger::new();
    let shape = a3.record(0, 3, ST_PASS).err() == Some(LT_ESTATE)
        && a3.record(0, ST_PASS, 9).err() == Some(LT_ESTATE);
    let mut a4 = LtpLedger::new();
    let mut id = 0usize;
    while id < CASES {
        let _ = a4.record(id, ST_PASS, ST_PASS);
        id = id.wrapping_add(1);
    }
    let count = a4.settle().err() == Some(LT_ECOUNT);
    let r2 = shape && count;

    // ④ 终账后再记 / 再清账
    let (mut a5, ok5) = build_b33();
    let locked = a5.freeze().is_some();
    let r3 = ok5
        && locked
        && a5.record(0, ST_PASS, ST_PASS).err() == Some(LT_EFROZEN)
        && a5.settle().err() == Some(LT_EFROZEN);

    [r0, r1, r2, r3]
}

/// 判据：UNX-F11152 B33 LTP 对照判据终重放。
pub fn run_unx11152_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11152");
    let (mut l, ok) = build_b33();

    // X1 十例齐备
    s.add_dyn(
        "X1-十例齐备",
        ok && l.len() == CASES && l.gaps().is_empty(),
        &format!("cases={}/{} gaps={}", l.len(), CASES, l.gaps().len()),
    );

    // X2 三态计数对口径
    let (p, f, k) = l.tri_counts();
    s.add_dyn(
        "X2-三态计数对口径",
        l.counts_canonical(),
        &format!("pass={} fail={} skip={}", p, f, k),
    );

    // X3 对照零失配
    s.add_dyn(
        "X3-对照零失配",
        l.mismatches() == 0,
        &format!("mismatch={}", l.mismatches()),
    );

    // X4 判据满 20
    s.add_dyn(
        "X4-判据满二十",
        l.criteria_replayed() == B33_CRITERIA,
        &format!("criteria={}/{}", l.criteria_replayed(), B33_CRITERIA),
    );

    // X5 用例序严格
    s.add("X5-用例序严格", l.order_strict(), "十例按 0..9 递增入账");

    // X6 终值即指纹
    let root = l.settled_value();
    s.add(
        "X6-终值即指纹",
        root == Some(l.fold()),
        "三态对照清单值等于逐例指纹",
    );

    // X7 冻结幂等
    let f1 = l.freeze();
    let f2 = l.freeze();
    s.add("X7-冻结幂等", f1.is_some() && f1 == f2, "二次冻结返回同一单值");

    // X8 冻结后拒记拒清
    s.add(
        "X8-冻结后拒记拒清",
        l.is_frozen()
            && l.record(0, ST_PASS, ST_PASS).err() == Some(LT_EFROZEN)
            && l.settle().err() == Some(LT_EFROZEN),
        "冻结后 record/settle 都落 LT_EFROZEN",
    );

    // X9 用例越界拒止
    let mut g = LtpLedger::new();
    s.add(
        "X9-用例越界拒止",
        g.record(CASES, ST_PASS, ST_PASS).err() == Some(LT_ENO)
            && g.record(CASES.wrapping_add(3), ST_SKIP, ST_SKIP)
                .err()
                == Some(LT_ENO)
            && g.is_empty(),
        "0..10 之外的用例号一律 LT_ENO",
    );

    // X10 重记先于倒序
    let mut d = LtpLedger::new();
    let _ = d.record(0, ST_PASS, ST_PASS);
    let _ = d.record(1, ST_PASS, ST_PASS);
    let mut o = LtpLedger::new();
    let _ = o.record(3, ST_PASS, ST_PASS);
    s.add(
        "X10-重记先于倒序",
        d.record(0, ST_PASS, ST_PASS).err() == Some(LT_EDUP)
            && o.record(2, ST_PASS, ST_PASS).err() == Some(LT_EORDER),
        "同例重记报 EDUP，真倒序才报 EORDER",
    );

    // X11 三态越界拒止
    let mut sh = LtpLedger::new();
    s.add(
        "X11-三态越界拒止",
        sh.record(0, 3, ST_PASS).err() == Some(LT_ESTATE)
            && sh.record(0, ST_PASS, 9).err() == Some(LT_ESTATE)
            && sh.is_empty(),
        "态值 ≥3 一律 LT_ESTATE",
    );

    // X12 未齐 / 计数偏离 / 对照失配
    let mut q1 = LtpLedger::new();
    let mut id = 0usize;
    while id < CASES - 1 {
        let _ = q1.record(id, ST_PASS, ST_PASS);
        id = id.wrapping_add(1);
    }
    let mut q2 = LtpLedger::new();
    let mut id = 0usize;
    while id < CASES {
        let _ = q2.record(id, ST_PASS, ST_PASS);
        id = id.wrapping_add(1);
    }
    let mut q3 = LtpLedger::new();
    let mut id = 0usize;
    while id < CASES {
        let (e, a) = if id < 6 {
            (ST_PASS, ST_PASS)
        } else if id < 8 {
            (ST_PASS, ST_FAIL)
        } else {
            (ST_PASS, ST_SKIP)
        };
        let _ = q3.record(id, e, a);
        id = id.wrapping_add(1);
    }
    s.add(
        "X12-未齐计数失配三拒",
        q1.settle().err() == Some(LT_ESHORT)
            && q2.settle().err() == Some(LT_ECOUNT)
            && q3.settle().err() == Some(LT_EMISMATCH),
        "未齐/计数偏离/对照失配各按其码",
    );

    // X13 注入四拒与账目
    let hits = inject_four();
    let all = hits[0] && hits[1] && hits[2] && hits[3];
    let (mut fresh, ok2) = build_b33();
    let root2 = fresh.freeze().unwrap_or(0);
    s.add_dyn(
        "X13-注入四拒与账目",
        all && ok2 && fresh.accepts == CASES as u64 && fresh.rejects == 0 && root2 != 0,
        &format!(
            "inject={}{}{}{} acc={} rej={} root={:#x}",
            hits[0] as u8, hits[1] as u8, hits[2] as u8, hits[3] as u8,
            fresh.accepts, fresh.rejects, root2
        ),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f11152_flow() {
        let (mut l, ok) = build_b33();
        assert!(ok);
        assert_eq!(l.len(), CASES);
        assert!(l.gaps().is_empty());
        assert!(l.order_strict());
        assert!(l.counts_canonical());
        assert_eq!(l.tri_counts(), (6, 2, 2));
        assert_eq!(l.mismatches(), 0);
        assert_eq!(l.criteria_replayed(), B33_CRITERIA);
        let root = l.settled_value();
        assert_eq!(root, Some(l.fold()));
        assert_eq!(l.freeze(), root);
        assert_eq!(l.freeze(), root);
        assert!(l.is_frozen());
        assert_eq!(l.record(0, ST_PASS, ST_PASS), Err(LT_EFROZEN));
        assert_eq!(l.settle(), Err(LT_EFROZEN));
    }

    #[test]
    fn f11152_gates() {
        let mut l = LtpLedger::new();
        assert!(l.record(0, ST_PASS, ST_PASS).is_ok());
        assert!(l.record(1, ST_PASS, ST_PASS).is_ok());
        assert_eq!((l.accepts, l.rejects), (2, 0));
        assert_eq!(l.record(CASES, ST_PASS, ST_PASS), Err(LT_ENO));
        assert_eq!(l.record(0, ST_PASS, ST_PASS), Err(LT_EDUP));
        assert_eq!((l.accepts, l.rejects), (2, 2));

        let mut o = LtpLedger::new();
        let _ = o.record(3, ST_PASS, ST_PASS);
        assert_eq!(o.record(2, ST_PASS, ST_PASS), Err(LT_EORDER));
        assert_eq!((o.accepts, o.rejects), (1, 1));

        let mut sh = LtpLedger::new();
        assert_eq!(sh.record(0, 3, ST_PASS), Err(LT_ESTATE));
        assert_eq!(sh.record(0, ST_PASS, 9), Err(LT_ESTATE));
        assert_eq!((sh.accepts, sh.rejects), (0, 2));

        let mut q = LtpLedger::new();
        let mut id = 0usize;
        while id < CASES - 1 {
            let _ = q.record(id, ST_PASS, ST_PASS);
            id = id.wrapping_add(1);
        }
        assert_eq!(q.settle(), Err(LT_ESHORT));
        assert_eq!((q.accepts, q.rejects), (9, 1));

        let mut m = LtpLedger::new();
        let mut id = 0usize;
        while id < CASES {
            let _ = m.record(id, ST_PASS, ST_PASS);
            id = id.wrapping_add(1);
        }
        assert_eq!(m.settle(), Err(LT_ECOUNT));
        assert_eq!((m.accepts, m.rejects), (10, 1));

        let mut mm = LtpLedger::new();
        let mut id = 0usize;
        while id < CASES {
            let (e, a) = if id < 6 {
                (ST_PASS, ST_PASS)
            } else if id < 8 {
                (ST_PASS, ST_FAIL)
            } else {
                (ST_PASS, ST_SKIP)
            };
            let _ = mm.record(id, e, a);
            id = id.wrapping_add(1);
        }
        assert_eq!(mm.mismatches(), 4);
        assert_eq!(mm.settle(), Err(LT_EMISMATCH));
        assert_eq!((mm.accepts, mm.rejects), (10, 1));

        let (mut d, ok) = build_b33();
        assert!(ok);
        assert!(d.freeze().is_some());
        assert_eq!(d.record(0, ST_PASS, ST_PASS), Err(LT_EFROZEN));
        assert_eq!(d.settle(), Err(LT_EFROZEN));
        assert_eq!((d.accepts, d.rejects), (10, 2));
    }

    #[test]
    fn f11152_checks_green() {
        assert!(run_unx11152_checks().all_passed());
    }
}
