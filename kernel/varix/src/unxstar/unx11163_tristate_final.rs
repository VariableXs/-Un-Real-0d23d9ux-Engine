//! UNX-F11163 域 40 批三态终核（满账前核验）（0x1f3f0 段）
//!
//! 纯功能模型：满账前逐批终核，40 批各落三态之一——结清、待复核、豁免。终核
//! 要求三态计数恰为 36/3/1、批批带证据数（不少于 5 条）、批号连续；三态账
//! 对平后方可进入满账，单值冻结后终核不再重开。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------- 错误码
const SEG_BASE: u32 = 0x1f3f0;
/// 批号越界（只认 B01–B40）
pub const TS_ENO: u32 = SEG_BASE + 0x01;
/// 批重复终核
pub const TS_EDUP: u32 = SEG_BASE + 0x02;
/// 批序倒挂
pub const TS_EORDER: u32 = SEG_BASE + 0x03;
/// 态值越出三态（形状违规）
pub const TS_ESTATE: u32 = SEG_BASE + 0x04;
/// 四十批未齐
pub const TS_ESHORT: u32 = SEG_BASE + 0x05;
/// 三态计数偏离口径
pub const TS_ECOUNT: u32 = SEG_BASE + 0x06;
/// 证据数不足
pub const TS_EEVID: u32 = SEG_BASE + 0x07;
/// 终值已锁定
pub const TS_EFROZEN: u32 = SEG_BASE + 0x08;

/// 域批数。
pub const BATCHES: usize = 40;
/// 态：结清。
pub const ST_SETTLED: u8 = 0;
/// 态：待复核。
pub const ST_REVIEW: u8 = 1;
/// 态：豁免。
pub const ST_WAIVED: u8 = 2;
/// 态域宽度。
pub const STATES: u8 = 3;
/// 口径：结清批数。
pub const WANT_SETTLED: u32 = 36;
/// 口径：待复核批数。
pub const WANT_REVIEW: u32 = 3;
/// 口径：豁免批数。
pub const WANT_WAIVED: u32 = 1;
/// 每批最少证据条数。
pub const MIN_EVIDENCE: u16 = 5;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 一批终核件。
#[derive(Clone, Copy, Debug)]
pub struct TriItem {
    pub batch: u8,
    pub state: u8,
    pub evidence: u16,
}

impl TriItem {
    /// 态值是否在域内。
    pub fn state_valid(&self) -> bool {
        self.state < STATES
    }

    /// 证据是否足量。
    pub fn evidenced(&self) -> bool {
        self.evidence >= MIN_EVIDENCE
    }
}

/// 三态终核账。
#[derive(Clone, Debug)]
pub struct TriLedger {
    items: Vec<TriItem>,
    settled: Option<u64>,
    frozen: bool,
    pub accepts: u64,
    pub rejects: u64,
}

impl TriLedger {
    /// 空账。
    pub fn new() -> TriLedger {
        TriLedger {
            items: Vec::new(),
            settled: None,
            frozen: false,
            accepts: 0,
            rejects: 0,
        }
    }

    /// 已核批数。
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 取某批。
    pub fn item(&self, batch: usize) -> Option<TriItem> {
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].batch as usize == batch {
                return Some(self.items[i]);
            }
            i = i.wrapping_add(1);
        }
        None
    }

    /// 终核一批：冻结 → 越界 → 重记 → 倒序 → 态值 → 证据。
    pub fn record(&mut self, batch: usize, state: u8, evidence: u16) -> Result<u64, u32> {
        if self.frozen || self.settled.is_some() {
            return self.fail(TS_EFROZEN);
        }
        if batch == 0 || batch > BATCHES {
            return self.fail(TS_ENO);
        }
        if self.item(batch).is_some() {
            return self.fail(TS_EDUP);
        }
        if let Some(last) = self.items.last() {
            if batch <= last.batch as usize {
                return self.fail(TS_EORDER);
            }
        }
        if state >= STATES {
            return self.fail(TS_ESTATE);
        }
        if evidence < MIN_EVIDENCE {
            return self.fail(TS_EEVID);
        }
        self.items.push(TriItem {
            batch: batch as u8,
            state,
            evidence,
        });
        self.accepts = self.accepts.wrapping_add(1);
        Ok(self.fold())
    }

    /// 记账一次拒止。
    fn fail(&mut self, code: u32) -> Result<u64, u32> {
        self.rejects = self.rejects.wrapping_add(1);
        Err(code)
    }

    /// 某态批数。
    pub fn count_state(&self, state: u8) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].state == state {
                n = n.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 三态计数三元组。
    pub fn tri_counts(&self) -> (u32, u32, u32) {
        (
            self.count_state(ST_SETTLED),
            self.count_state(ST_REVIEW),
            self.count_state(ST_WAIVED),
        )
    }

    /// 三态计数是否合口径。
    pub fn counts_canonical(&self) -> bool {
        let (a, b, c) = self.tri_counts();
        a == WANT_SETTLED && b == WANT_REVIEW && c == WANT_WAIVED
    }

    /// 证据总数。
    pub fn evidence_sum(&self) -> u64 {
        let mut t = 0u64;
        let mut i = 0usize;
        while i < self.items.len() {
            t = t.wrapping_add(self.items[i].evidence as u64);
            i = i.wrapping_add(1);
        }
        t
    }

    /// 证据不足批数。
    pub fn under_evidenced(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.items.len() {
            if !self.items[i].evidenced() {
                n = n.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 缺批表。
    pub fn gaps(&self) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        let mut b = 1usize;
        while b <= BATCHES {
            if self.item(b).is_none() {
                out.push(b as u32);
            }
            b = b.wrapping_add(1);
        }
        out
    }

    /// 批序严格。
    pub fn order_strict(&self) -> bool {
        let mut i = 1usize;
        while i < self.items.len() {
            if self.items[i].batch <= self.items[i - 1].batch {
                return false;
            }
            i = i.wrapping_add(1);
        }
        true
    }

    /// 三态账指纹。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.items.len() {
            let it = self.items[i];
            h = (h ^ it.batch as u64).wrapping_mul(FNV_PRIME);
            h = (h ^ it.state as u64).wrapping_mul(FNV_PRIME);
            h = (h ^ it.evidence as u64).wrapping_mul(FNV_PRIME);
            i = i.wrapping_add(1);
        }
        h = (h ^ self.items.len() as u64).wrapping_mul(FNV_PRIME);
        (h ^ self.evidence_sum()).wrapping_mul(FNV_PRIME)
    }

    /// 终核：四十批齐 → 三态计数 → 证据齐 → 逐批在域内。
    pub fn settle(&mut self) -> Result<u64, u32> {
        if self.frozen || self.settled.is_some() {
            return self.fail(TS_EFROZEN);
        }
        if self.items.len() != BATCHES || !self.gaps().is_empty() {
            return self.fail(TS_ESHORT);
        }
        if !self.counts_canonical() {
            return self.fail(TS_ECOUNT);
        }
        if self.under_evidenced() != 0 {
            return self.fail(TS_EEVID);
        }
        let v = self.fold();
        self.settled = Some(v);
        self.frozen = true;
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

/// 构建标准终核账：40 批，36 结清、3 待复核、1 豁免，每批 5 条证据。
pub fn build_final() -> TriLedger {
    let mut l = TriLedger::new();
    let mut i = 0usize;
    while i < BATCHES {
        let batch = i + 1;
        let state = if i < 36 {
            ST_SETTLED
        } else if i < 39 {
            ST_REVIEW
        } else {
            ST_WAIVED
        };
        let _ = l.record(batch, state, MIN_EVIDENCE);
        i += 1;
    }
    let _ = l.settle();
    l
}

/// 四探针：越界、重复/乱序、三态/证据违例、冻结后写，各以本模块错误码拒绝。
pub fn inject_four() -> [bool; 4] {
    let mut r = [false; 4];

    let mut l0 = TriLedger::new();
    let _ = l0.record(1, ST_SETTLED, MIN_EVIDENCE);
    if l0.record(0, ST_SETTLED, MIN_EVIDENCE) == Err(TS_ENO)
        && l0.record(BATCHES + 1, ST_SETTLED, MIN_EVIDENCE) == Err(TS_ENO)
    {
        r[0] = true;
    }

    let mut l1 = TriLedger::new();
    let _ = l1.record(1, ST_SETTLED, MIN_EVIDENCE);
    let _ = l1.record(2, ST_SETTLED, MIN_EVIDENCE);
    if l1.record(1, ST_REVIEW, MIN_EVIDENCE) == Err(TS_EDUP) {
        let mut l1b = TriLedger::new();
        let _ = l1b.record(5, ST_SETTLED, MIN_EVIDENCE);
        if l1b.record(3, ST_SETTLED, MIN_EVIDENCE) == Err(TS_EORDER) {
            r[1] = true;
        }
    }

    let mut l2 = TriLedger::new();
    let _ = l2.record(1, ST_SETTLED, MIN_EVIDENCE);
    if l2.record(2, 7, MIN_EVIDENCE) == Err(TS_ESTATE)
        && l2.record(2, ST_SETTLED, 0) == Err(TS_EEVID)
    {
        r[2] = true;
    }

    let mut l3 = build_final();
    if l3.is_frozen()
        && l3.record(41, ST_SETTLED, MIN_EVIDENCE) == Err(TS_EFROZEN)
        && l3.settle() == Err(TS_EFROZEN)
    {
        r[3] = true;
    }
    r
}

/// 自检：13 项全绿。
pub fn run_unx11163_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11163");

    let base = build_final();
    s.add(
        "X1-账构建",
        base.len() == 40 && base.accepts == 40 && base.rejects == 0,
        "终核账应恰好 40 批且零拒收",
    );

    let tc = base.tri_counts();
    s.add_dyn(
        "X2-三态计数",
        tc == (WANT_SETTLED, WANT_REVIEW, WANT_WAIVED),
        &format!("settled={} review={} waived={}", tc.0, tc.1, tc.2),
    );

    s.add(
        "X3-证据总数",
        base.evidence_sum() == 200 && base.under_evidenced() == 0,
        "40 批 × 5 条证据应为 200",
    );

    s.add(
        "X4-批号连续",
        base.gaps().is_empty() && base.order_strict(),
        "批号 1..=40 无缺口且严格递增",
    );

    let mut fz = build_final();
    let v1 = fz.settled_value();
    let v2 = fz.freeze();
    let v3 = fz.freeze();
    s.add(
        "X5-冻结单值",
        v1.is_some() && v1 == v2 && v2 == v3 && fz.is_frozen(),
        "二次冻结返回同一终值",
    );

    let mut fl = TriLedger::new();
    let _ = fl.record(1, ST_SETTLED, MIN_EVIDENCE);
    let a = fl.fold();
    let b = fl.fold();
    s.add(
        "X6-折叠稳定",
        a == b && a != 0 && a != FNV_OFF,
        "折叠两次一致且非零",
    );

    let hits = inject_four();
    s.add(
        "X7-四探针",
        hits[0] && hits[1] && hits[2] && hits[3],
        "越界/重复乱序/违例/冻结后写四码可达",
    );

    let mut ld = TriLedger::new();
    let _ = ld.record(1, ST_SETTLED, MIN_EVIDENCE);
    let _ = ld.record(2, ST_SETTLED, MIN_EVIDENCE);
    let d = ld.record(1, ST_REVIEW, MIN_EVIDENCE);
    s.add(
        "X8-重复拒收",
        d == Err(TS_EDUP) && ld.accepts == 2 && ld.rejects == 1,
        "重复批号拒收且计数精确",
    );

    let mut lr = TriLedger::new();
    let _ = lr.record(1, ST_SETTLED, MIN_EVIDENCE);
    let r0 = lr.record(0, ST_SETTLED, MIN_EVIDENCE);
    let r1 = lr.record(41, ST_SETTLED, MIN_EVIDENCE);
    s.add(
        "X9-越界拒收",
        r0 == Err(TS_ENO) && r1 == Err(TS_ENO) && lr.accepts == 1 && lr.rejects == 2,
        "批号 0 与 41 均判越界",
    );

    let mut lo = TriLedger::new();
    let _ = lo.record(5, ST_SETTLED, MIN_EVIDENCE);
    let o = lo.record(3, ST_SETTLED, MIN_EVIDENCE);
    s.add(
        "X10-乱序拒收",
        o == Err(TS_EORDER) && lo.accepts == 1 && lo.rejects == 1,
        "5 后写 3 判乱序",
    );

    let mut lv = TriLedger::new();
    let _ = lv.record(1, ST_SETTLED, MIN_EVIDENCE);
    let e1 = lv.record(2, 7, MIN_EVIDENCE);
    let e2 = lv.record(2, ST_SETTLED, 0);
    s.add(
        "X11-三态与证据",
        e1 == Err(TS_ESTATE) && e2 == Err(TS_EEVID) && lv.accepts == 1 && lv.rejects == 2,
        "非法三态与证据不足分别拒收",
    );

    let mut sh = TriLedger::new();
    let mut i = 0usize;
    while i < 39 {
        let _ = sh.record(i + 1, ST_SETTLED, MIN_EVIDENCE);
        i += 1;
    }
    let es = sh.settle();
    let mut ec = TriLedger::new();
    let mut j = 0usize;
    while j < 40 {
        let _ = ec.record(j + 1, ST_SETTLED, MIN_EVIDENCE);
        j += 1;
    }
    let ee = ec.settle();
    s.add(
        "X12-未满账拒结",
        es == Err(TS_ESHORT) && ee == Err(TS_ECOUNT),
        "39 批判缺批、40 批全结清判计数违例",
    );

    let mut z = build_final();
    let zf = z.freeze();
    s.add_dyn(
        "X13-终值统计",
        zf.is_some()
            && zf == z.settled_value()
            && z.accepts == 40
            && z.rejects == 0
            && z.gaps().is_empty(),
        &format!("settled={} accepts={} rejects={}", zf.is_some(), z.accepts, z.rejects),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f11163_flow() {
        let mut l = build_final();
        assert_eq!(l.len(), 40);
        assert_eq!(l.tri_counts(), (36, 3, 1));
        assert_eq!(l.evidence_sum(), 200);
        assert!(l.gaps().is_empty());
        assert!(l.order_strict());
        assert_eq!(l.accepts, 40);
        assert_eq!(l.rejects, 0);
        assert!(l.is_frozen());
        let v = l.settled_value();
        assert!(v.is_some());
        assert_eq!(l.freeze(), v);
        assert_eq!(l.freeze(), v);
        assert!(l.fold() != 0);
        let hits = inject_four();
        assert_eq!(hits, [true, true, true, true]);
        assert_eq!(l.item(1).map(|i| i.state), Some(ST_SETTLED));
        assert_eq!(l.item(37).map(|i| i.state), Some(ST_REVIEW));
        assert_eq!(l.item(40).map(|i| i.state), Some(ST_WAIVED));
        assert!(l.item(41).is_none());
    }

    #[test]
    fn f11163_gates() {
        let mut l = TriLedger::new();
        assert!(l.record(1, ST_SETTLED, MIN_EVIDENCE).is_ok());
        assert!(l.record(2, ST_SETTLED, MIN_EVIDENCE).is_ok());
        assert_eq!((l.accepts, l.rejects), (2, 0));
        assert_eq!(l.record(1, ST_REVIEW, MIN_EVIDENCE), Err(TS_EDUP));
        assert_eq!((l.accepts, l.rejects), (2, 1));
        assert!(l.record(5, ST_SETTLED, MIN_EVIDENCE).is_ok());
        assert_eq!((l.accepts, l.rejects), (3, 1));
        assert_eq!(l.record(3, ST_SETTLED, MIN_EVIDENCE), Err(TS_EORDER));
        assert_eq!((l.accepts, l.rejects), (3, 2));
        assert_eq!(l.record(6, 7, MIN_EVIDENCE), Err(TS_ESTATE));
        assert_eq!((l.accepts, l.rejects), (3, 3));
        assert_eq!(l.record(6, ST_SETTLED, 0), Err(TS_EEVID));
        assert_eq!((l.accepts, l.rejects), (3, 4));
        assert_eq!(l.record(41, ST_SETTLED, MIN_EVIDENCE), Err(TS_ENO));
        assert_eq!(l.record(0, ST_SETTLED, MIN_EVIDENCE), Err(TS_ENO));
        assert_eq!((l.accepts, l.rejects), (3, 6));
        assert_eq!(l.settle(), Err(TS_ESHORT));
        assert_eq!((l.accepts, l.rejects), (3, 7));

        let mut short = TriLedger::new();
        let mut i = 0usize;
        while i < 39 {
            assert!(short.record(i + 1, ST_SETTLED, MIN_EVIDENCE).is_ok());
            i += 1;
        }
        assert_eq!(short.settle(), Err(TS_ESHORT));
        assert_eq!((short.accepts, short.rejects), (39, 1));

        let mut count = TriLedger::new();
        let mut j = 0usize;
        while j < 40 {
            assert!(count.record(j + 1, ST_SETTLED, MIN_EVIDENCE).is_ok());
            j += 1;
        }
        assert_eq!(count.settle(), Err(TS_ECOUNT));
        assert_eq!((count.accepts, count.rejects), (40, 1));

        let mut done = build_final();
        assert_eq!(done.settle(), Err(TS_EFROZEN));
        assert_eq!(done.record(41, ST_SETTLED, MIN_EVIDENCE), Err(TS_EFROZEN));
        assert_eq!((done.accepts, done.rejects), (40, 2));
    }

    #[test]
    fn f11163_checks_green() {
        let s = run_unx11163_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert!(!s.truncated());
    }
}
