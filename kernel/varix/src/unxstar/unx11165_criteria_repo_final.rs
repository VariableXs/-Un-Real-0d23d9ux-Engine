//! UNX-F11165 判据库终盘（800 条连续零断档终验）（0x1f410 段）
//!
//! 纯功能模型：判据库终盘逐号登记八百条判据，号段 1..=800 必须连续、
//! 严格递增、零断档；每二十条归一批、共四十批，批号由判据号唯一导出。
//! 终盘以连续断言收口，锁值后不再增补或重验。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址。
const SEG_BASE: u32 = 0x1f410;

/// 判据号越界。
pub const CR_ENO: u32 = SEG_BASE + 0x01;
/// 判据号重复。
pub const CR_EDUP: u32 = SEG_BASE + 0x02;
/// 判据号倒序。
pub const CR_EORDER: u32 = SEG_BASE + 0x03;
/// 批号与判据号导出不符。
pub const CR_EBATCH: u32 = SEG_BASE + 0x04;
/// 判定值越出二值。
pub const CR_EVERDICT: u32 = SEG_BASE + 0x05;
/// 判据号断档。
pub const CR_EGAP: u32 = SEG_BASE + 0x06;
/// 终值已锁定。
pub const CR_EFROZEN: u32 = SEG_BASE + 0x07;

/// 判据总数。
pub const TOTAL: usize = 800;
/// 每批判据数。
pub const PER_BATCH: usize = 20;
/// 批数。
pub const BATCHES: usize = 40;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 一条判据。
#[derive(Clone, Copy, Debug)]
pub struct Criterion {
    pub no: u16,
    pub batch: u8,
    pub verdict: u8,
}

impl Criterion {
    /// 由判据号导出批号（1..=40）。
    pub fn derive_batch(no: u16) -> u8 {
        ((no as usize - 1) / PER_BATCH as usize + 1) as u8
    }

    /// 批号是否与判据号一致。
    pub fn batch_ok(&self) -> bool {
        self.batch == Criterion::derive_batch(self.no)
    }

    /// 判定值是否二值。
    pub fn verdict_ok(&self) -> bool {
        self.verdict <= 1
    }
}

/// 判据库终盘。
#[derive(Clone, Debug)]
pub struct CriteriaRepo {
    items: Vec<Criterion>,
    settled: Option<u64>,
    frozen: bool,
    pub accepts: u64,
    pub rejects: u64,
}

impl CriteriaRepo {
    /// 空库。
    pub fn new() -> CriteriaRepo {
        CriteriaRepo {
            items: Vec::new(),
            settled: None,
            frozen: false,
            accepts: 0,
            rejects: 0,
        }
    }

    /// 已入判据数。
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 是否空库。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 按号取判据。
    pub fn item(&self, no: usize) -> Option<Criterion> {
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].no as usize == no {
                return Some(self.items[i]);
            }
            i = i.wrapping_add(1);
        }
        None
    }

    /// 库内尾号。
    pub fn last_no(&self) -> u16 {
        match self.items.last() {
            Some(c) => c.no,
            None => 0,
        }
    }
}

impl CriteriaRepo {
    /// 登记一条：冻结 → 越界 → 重记 → 倒序 → 批号 → 判定值。
    pub fn record(&mut self, no: usize, batch: u8, verdict: u8) -> Result<u64, u32> {
        if self.frozen || self.settled.is_some() {
            return self.fail(CR_EFROZEN);
        }
        if no == 0 || no > TOTAL {
            return self.fail(CR_ENO);
        }
        if self.item(no).is_some() {
            return self.fail(CR_EDUP);
        }
        if no <= self.last_no() as usize {
            return self.fail(CR_EORDER);
        }
        let c = Criterion {
            no: no as u16,
            batch,
            verdict,
        };
        if !c.batch_ok() {
            return self.fail(CR_EBATCH);
        }
        if !c.verdict_ok() {
            return self.fail(CR_EVERDICT);
        }
        self.items.push(c);
        self.accepts = self.accepts.wrapping_add(1);
        Ok(self.fold())
    }

    /// 记账一次拒止。
    fn fail(&mut self, code: u32) -> Result<u64, u32> {
        self.rejects = self.rejects.wrapping_add(1);
        Err(code)
    }

    /// 缺号表。
    pub fn gaps(&self) -> Vec<u16> {
        let mut out: Vec<u16> = Vec::new();
        let mut n = 1usize;
        while n <= TOTAL {
            if self.item(n).is_none() {
                out.push(n as u16);
            }
            n = n.wrapping_add(1);
        }
        out
    }

    /// 判据号严格递增。
    pub fn order_strict(&self) -> bool {
        let mut i = 1usize;
        while i < self.items.len() {
            if self.items[i].no <= self.items[i - 1].no {
                return false;
            }
            i = i.wrapping_add(1);
        }
        true
    }

    /// 某批判据数。
    pub fn batch_count(&self, batch: u8) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].batch == batch {
                n = n.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        n
    }

    /// 四十批各二十条且总数八百。
    pub fn batch_coverage(&self) -> bool {
        if self.items.len() != TOTAL {
            return false;
        }
        let mut b = 1u8;
        while b as usize <= BATCHES {
            if self.batch_count(b) != PER_BATCH {
                return false;
            }
            b = b.wrapping_add(1);
        }
        true
    }

    /// 某判定值的条数。
    pub fn verdict_count(&self, verdict: u8) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].verdict == verdict {
                n = n.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        n
    }
}

impl CriteriaRepo {
    /// 判据库指纹。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.items.len() {
            let c = self.items[i];
            h = (h ^ c.no as u64).wrapping_mul(FNV_PRIME);
            h = (h ^ c.batch as u64).wrapping_mul(FNV_PRIME);
            h = (h ^ c.verdict as u64).wrapping_mul(FNV_PRIME);
            i = i.wrapping_add(1);
        }
        h = (h ^ self.items.len() as u64).wrapping_mul(FNV_PRIME);
        (h ^ self.verdict_count(1) as u64).wrapping_mul(FNV_PRIME)
    }

    /// 终盘：八百条齐且零断档 → 批覆盖 → 批序。
    pub fn settle(&mut self) -> Result<u64, u32> {
        if self.frozen || self.settled.is_some() {
            return self.fail(CR_EFROZEN);
        }
        if self.items.len() != TOTAL || !self.gaps().is_empty() {
            return self.fail(CR_EGAP);
        }
        if !self.batch_coverage() {
            return self.fail(CR_EBATCH);
        }
        if !self.order_strict() {
            return self.fail(CR_EORDER);
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

/// 构建标准判据库：1..=800 连续，判定值按号奇偶交替。
pub fn build_final() -> CriteriaRepo {
    let mut r = CriteriaRepo::new();
    let mut no = 1usize;
    while no <= TOTAL {
        let batch = Criterion::derive_batch(no as u16);
        let verdict = (no % 2) as u8;
        let _ = r.record(no, batch, verdict);
        no += 1;
    }
    let _ = r.settle();
    r
}

/// 四探针：越界、重复/倒序、批号/判定值违例、冻结后写，各以本模块错误码拒绝。
pub fn inject_four() -> [bool; 4] {
    let mut r = [false; 4];

    let mut r0 = CriteriaRepo::new();
    let _ = r0.record(1, 1, 0);
    if r0.record(0, 1, 0) == Err(CR_ENO) && r0.record(TOTAL + 1, 40, 0) == Err(CR_ENO) {
        r[0] = true;
    }

    let mut r1 = CriteriaRepo::new();
    let _ = r1.record(1, 1, 0);
    let _ = r1.record(2, 1, 0);
    if r1.record(1, 1, 0) == Err(CR_EDUP) {
        let mut r1b = CriteriaRepo::new();
        let _ = r1b.record(5, 1, 0);
        if r1b.record(3, 1, 0) == Err(CR_EORDER) {
            r[1] = true;
        }
    }

    let mut r2 = CriteriaRepo::new();
    let _ = r2.record(1, 1, 0);
    if r2.record(2, 2, 0) == Err(CR_EBATCH) && r2.record(2, 1, 2) == Err(CR_EVERDICT) {
        r[2] = true;
    }

    let mut r3 = build_final();
    if r3.is_frozen()
        && r3.record(TOTAL + 1, 40, 0) == Err(CR_EFROZEN)
        && r3.settle() == Err(CR_EFROZEN)
    {
        r[3] = true;
    }
    r
}

/// 自检：13 项全绿。
pub fn run_unx11165_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11165");

    let base = build_final();
    s.add(
        "X1-终盘构建",
        base.len() == TOTAL && base.accepts == 800 && base.rejects == 0 && base.is_frozen(),
        "八百条全收、零拒收且已锁定",
    );
    s.add(
        "X2-号段连续",
        base.gaps().is_empty() && base.order_strict(),
        "1..=800 零断档且严格递增",
    );
    s.add_dyn(
        "X3-批覆盖",
        base.batch_coverage(),
        &format!("batches={} b1={} b40={}", BATCHES, base.batch_count(1), base.batch_count(40)),
    );
    s.add(
        "X4-判定分布",
        base.verdict_count(0) + base.verdict_count(1) == TOTAL,
        "二值判定合计恰为八百",
    );

    let mut fz = base.clone();
    let v = fz.freeze();
    s.add(
        "X5-冻结单值",
        v == fz.settled_value() && v == fz.freeze() && fz.is_frozen(),
        "二次冻结同一终值",
    );

    let mut fl = CriteriaRepo::new();
    let _ = fl.record(1, 1, 0);
    s.add(
        "X6-折叠稳定",
        fl.fold() == fl.fold() && fl.fold() != 0,
        "折叠两次一致且非零",
    );

    let hits = inject_four();
    s.add(
        "X7-四探针",
        hits[0] && hits[1] && hits[2] && hits[3],
        "越界/重复倒序/批号判定/冻结后写四码可达",
    );

    let mut ld = CriteriaRepo::new();
    let _ = ld.record(1, 1, 0);
    let _ = ld.record(2, 1, 0);
    let d = ld.record(1, 1, 0);
    s.add(
        "X8-重复拒收",
        d == Err(CR_EDUP) && ld.accepts == 2 && ld.rejects == 1,
        "重复判据号拒收且计数精确",
    );

    let mut lr = CriteriaRepo::new();
    let _ = lr.record(1, 1, 0);
    let r0 = lr.record(0, 1, 0);
    let r1 = lr.record(TOTAL + 1, 40, 0);
    s.add(
        "X9-越界拒收",
        r0 == Err(CR_ENO) && r1 == Err(CR_ENO) && lr.accepts == 1 && lr.rejects == 2,
        "判据号 0 与 801 均判越界",
    );

    let mut lo = CriteriaRepo::new();
    let _ = lo.record(5, 1, 0);
    let o = lo.record(3, 1, 0);
    s.add(
        "X10-倒序拒收",
        o == Err(CR_EORDER) && lo.accepts == 1 && lo.rejects == 1,
        "5 后写 3 判倒序",
    );

    let mut lv = CriteriaRepo::new();
    let _ = lv.record(1, 1, 0);
    let b = lv.record(2, 2, 0);
    let w = lv.record(2, 1, 2);
    s.add(
        "X11-批号与判定",
        b == Err(CR_EBATCH) && w == Err(CR_EVERDICT) && lv.accepts == 1 && lv.rejects == 2,
        "批号导出不符与二值越界分别拒收",
    );

    let mut lg = CriteriaRepo::new();
    let mut no = 1usize;
    while no <= TOTAL {
        if no != 400 {
            let _ = lg.record(no, Criterion::derive_batch(no as u16), 0);
        }
        no += 1;
    }
    let g = lg.settle();
    s.add(
        "X12-断档终验",
        g == Err(CR_EGAP) && lg.len() == 799 && lg.gaps() == Vec::from([400u16]),
        "缺 400 一条即判断档",
    );

    let mut z = build_final();
    let zf = z.freeze();
    s.add_dyn(
        "X13-终值统计",
        zf.is_some() && zf == z.settled_value() && z.accepts == 800 && z.rejects == 0,
        &format!("acc={} rej={} len={}", z.accepts, z.rejects, z.len()),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f11165_flow() {
        let mut r = build_final();
        assert_eq!(r.len(), 800);
        assert!(r.gaps().is_empty());
        assert!(r.order_strict());
        assert!(r.batch_coverage());
        assert_eq!(r.batch_count(1), 20);
        assert_eq!(r.batch_count(40), 20);
        assert_eq!(r.verdict_count(0), 400);
        assert_eq!(r.verdict_count(1), 400);
        assert!(r.is_frozen());
        assert_eq!((r.accepts, r.rejects), (800, 0));
        let v = r.settled_value();
        assert!(v.is_some());
        assert_eq!(r.freeze(), v);
        assert_eq!(r.freeze(), v);
        assert_eq!(r.item(1).map(|c| c.batch), Some(1));
        assert_eq!(r.item(800).map(|c| c.batch), Some(40));
        assert!(r.item(801).is_none());
        assert!(r.fold() != 0);
        assert_eq!(inject_four(), [true, true, true, true]);
    }

    #[test]
    fn f11165_gates() {
        let mut l = CriteriaRepo::new();
        assert!(l.record(1, 1, 0).is_ok());
        assert!(l.record(2, 1, 0).is_ok());
        assert_eq!((l.accepts, l.rejects), (2, 0));
        assert_eq!(l.record(1, 1, 0), Err(CR_EDUP));
        assert_eq!((l.accepts, l.rejects), (2, 1));
        assert!(l.record(5, 1, 0).is_ok());
        assert_eq!((l.accepts, l.rejects), (3, 1));
        assert_eq!(l.record(3, 1, 0), Err(CR_EORDER));
        assert_eq!((l.accepts, l.rejects), (3, 2));
        assert_eq!(l.record(6, 2, 0), Err(CR_EBATCH));
        assert_eq!((l.accepts, l.rejects), (3, 3));
        assert_eq!(l.record(6, 1, 2), Err(CR_EVERDICT));
        assert_eq!((l.accepts, l.rejects), (3, 4));
        assert_eq!(l.record(0, 1, 0), Err(CR_ENO));
        assert_eq!(l.record(TOTAL + 1, 40, 0), Err(CR_ENO));
        assert_eq!((l.accepts, l.rejects), (3, 6));
        assert_eq!(l.settle(), Err(CR_EGAP));
        assert_eq!((l.accepts, l.rejects), (3, 7));

        let mut gap = CriteriaRepo::new();
        let mut no = 1usize;
        while no <= TOTAL {
            if no != 400 {
                let b = Criterion::derive_batch(no as u16);
                assert!(gap.record(no, b, 0).is_ok());
            }
            no += 1;
        }
        assert_eq!(gap.len(), 799);
        assert_eq!(gap.gaps(), Vec::from([400u16]));
        assert_eq!(gap.settle(), Err(CR_EGAP));
        assert_eq!((gap.accepts, gap.rejects), (799, 1));

        let mut done = build_final();
        assert_eq!(done.settle(), Err(CR_EFROZEN));
        assert_eq!(done.record(TOTAL + 1, 40, 0), Err(CR_EFROZEN));
        assert_eq!((done.accepts, done.rejects), (800, 2));
    }

    #[test]
    fn f11165_checks_green() {
        let s = run_unx11165_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert!(!s.truncated());
    }
}
