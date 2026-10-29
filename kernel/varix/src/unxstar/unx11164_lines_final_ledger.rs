//! UNX-F11164 域行数总账终核（满账前账面核验）（0x1f400 段）
//!
//! 纯功能模型：满账前对域行数总账做终核——逐批登记行数与累计链，十批
//! 必须批批对平、口径恰为每批二万一千六百行、总计二十一万六千行，且
//! 累计链自首批起逐批连续。终核一次锁值，锁后再写、再核一律拒止。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址。
const SEG_BASE: u32 = 0x1f400;

/// 批号越界。
pub const LF_ENO: u32 = SEG_BASE + 0x01;
/// 批号重复。
pub const LF_EDUP: u32 = SEG_BASE + 0x02;
/// 批序倒挂。
pub const LF_EORDER: u32 = SEG_BASE + 0x03;
/// 行数形状违规（零行或零累计）。
pub const LF_ESHAPE: u32 = SEG_BASE + 0x04;
/// 总行数偏离口径。
pub const LF_ESUM: u32 = SEG_BASE + 0x05;
/// 批数或缺批、单批行数偏离口径。
pub const LF_ECOUNT: u32 = SEG_BASE + 0x06;
/// 累计链断档。
pub const LF_ECHAIN: u32 = SEG_BASE + 0x07;
/// 终值已锁定。
pub const LF_EFROZEN: u32 = SEG_BASE + 0x08;

/// 域批数。
pub const BATCHES: usize = 10;
/// 每批行数口径。
pub const LINES_PER_BATCH: u32 = 21_600;
/// 总行数口径。
pub const TOTAL_LINES: u32 = 216_000;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 一批行数账目。
#[derive(Clone, Copy, Debug)]
pub struct LineBatch {
    pub batch: u8,
    pub lines: u32,
    pub cumulative: u32,
}

impl LineBatch {
    /// 形状合格：正行数、正累计。
    pub fn shaped(&self) -> bool {
        self.lines > 0 && self.cumulative > 0
    }

    /// 口径合格：单批行数恰为两万一千六。
    pub fn canonical(&self) -> bool {
        self.lines == LINES_PER_BATCH
    }
}

/// 行数总账。
#[derive(Clone, Debug)]
pub struct LinesLedger {
    items: Vec<LineBatch>,
    settled: Option<u64>,
    frozen: bool,
    pub accepts: u64,
    pub rejects: u64,
}

impl LinesLedger {
    /// 空账。
    pub fn new() -> LinesLedger {
        LinesLedger {
            items: Vec::new(),
            settled: None,
            frozen: false,
            accepts: 0,
            rejects: 0,
        }
    }

    /// 已登批数。
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 取某批。
    pub fn item(&self, batch: usize) -> Option<LineBatch> {
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].batch as usize == batch {
                return Some(self.items[i]);
            }
            i = i.wrapping_add(1);
        }
        None
    }
}

impl LinesLedger {
    /// 登记一批：冻结 → 越界 → 重记 → 倒序 → 形状。
    pub fn record(&mut self, batch: usize, lines: u32, cumulative: u32) -> Result<u64, u32> {
        if self.frozen || self.settled.is_some() {
            return self.fail(LF_EFROZEN);
        }
        if batch == 0 || batch > BATCHES {
            return self.fail(LF_ENO);
        }
        if self.item(batch).is_some() {
            return self.fail(LF_EDUP);
        }
        if let Some(last) = self.items.last() {
            if batch <= last.batch as usize {
                return self.fail(LF_EORDER);
            }
        }
        if lines == 0 || cumulative == 0 {
            return self.fail(LF_ESHAPE);
        }
        self.items.push(LineBatch {
            batch: batch as u8,
            lines,
            cumulative,
        });
        self.accepts = self.accepts.wrapping_add(1);
        Ok(self.fold())
    }

    /// 记账一次拒止。
    fn fail(&mut self, code: u32) -> Result<u64, u32> {
        self.rejects = self.rejects.wrapping_add(1);
        Err(code)
    }

    /// 行数总和。
    pub fn lines_sum(&self) -> u32 {
        let mut t = 0u32;
        let mut i = 0usize;
        while i < self.items.len() {
            t = t.wrapping_add(self.items[i].lines);
            i = i.wrapping_add(1);
        }
        t
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

    /// 逐批口径全对平。
    pub fn batches_canonical(&self) -> bool {
        let mut i = 0usize;
        while i < self.items.len() {
            if !self.items[i].canonical() {
                return false;
            }
            i = i.wrapping_add(1);
        }
        self.items.len() == BATCHES
    }

    /// 累计链连续：首批判累计==行数，其后逐批累计==前累计+本批行数。
    pub fn chain_ok(&self) -> bool {
        let mut run = 0u32;
        let mut i = 0usize;
        while i < self.items.len() {
            let it = self.items[i];
            run = run.wrapping_add(it.lines);
            if it.cumulative != run {
                return false;
            }
            i = i.wrapping_add(1);
        }
        self.items.len() >= BATCHES
    }
}

impl LinesLedger {
    /// 行数总账指纹。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.items.len() {
            let it = self.items[i];
            h = (h ^ it.batch as u64).wrapping_mul(FNV_PRIME);
            h = (h ^ it.lines as u64).wrapping_mul(FNV_PRIME);
            h = (h ^ it.cumulative as u64).wrapping_mul(FNV_PRIME);
            i = i.wrapping_add(1);
        }
        h = (h ^ self.items.len() as u64).wrapping_mul(FNV_PRIME);
        (h ^ self.lines_sum() as u64).wrapping_mul(FNV_PRIME)
    }

    /// 终核：十批齐 → 总行数 → 逐批口径 → 累计链。
    pub fn settle(&mut self) -> Result<u64, u32> {
        if self.frozen || self.settled.is_some() {
            return self.fail(LF_EFROZEN);
        }
        if self.items.len() != BATCHES || !self.gaps().is_empty() || !self.order_strict() {
            return self.fail(LF_ECOUNT);
        }
        if self.lines_sum() != TOTAL_LINES {
            return self.fail(LF_ESUM);
        }
        if !self.batches_canonical() {
            return self.fail(LF_ECOUNT);
        }
        if !self.chain_ok() {
            return self.fail(LF_ECHAIN);
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

/// 构建标准行数总账：十批各二万一千六，累计链连续。
pub fn build_final() -> LinesLedger {
    let mut l = LinesLedger::new();
    let mut i = 0usize;
    while i < BATCHES {
        let batch = i + 1;
        let cum = (batch as u32).wrapping_mul(LINES_PER_BATCH);
        let _ = l.record(batch, LINES_PER_BATCH, cum);
        i += 1;
    }
    let _ = l.settle();
    l
}

/// 四探针：越界、重复/乱序、形状与链断、冻结后写，各以本模块错误码拒绝。
pub fn inject_four() -> [bool; 4] {
    let mut r = [false; 4];

    let mut l0 = LinesLedger::new();
    let _ = l0.record(1, LINES_PER_BATCH, LINES_PER_BATCH);
    if l0.record(0, LINES_PER_BATCH, LINES_PER_BATCH) == Err(LF_ENO)
        && l0.record(BATCHES + 1, LINES_PER_BATCH, LINES_PER_BATCH) == Err(LF_ENO)
    {
        r[0] = true;
    }

    let mut l1 = LinesLedger::new();
    let _ = l1.record(1, LINES_PER_BATCH, LINES_PER_BATCH);
    let _ = l1.record(2, LINES_PER_BATCH, 2 * LINES_PER_BATCH);
    if l1.record(1, LINES_PER_BATCH, 3 * LINES_PER_BATCH) == Err(LF_EDUP) {
        let mut l1b = LinesLedger::new();
        let _ = l1b.record(5, LINES_PER_BATCH, 5 * LINES_PER_BATCH);
        if l1b.record(3, LINES_PER_BATCH, 8 * LINES_PER_BATCH) == Err(LF_EORDER) {
            r[1] = true;
        }
    }

    let mut l2 = LinesLedger::new();
    let _ = l2.record(1, LINES_PER_BATCH, LINES_PER_BATCH);
    if l2.record(2, 0, 2 * LINES_PER_BATCH) == Err(LF_ESHAPE) {
        let mut l2b = LinesLedger::new();
        let mut i = 0usize;
        while i < BATCHES {
            let batch = i + 1;
            let cum = if batch == 5 {
                99_999
            } else {
                (batch as u32).wrapping_mul(LINES_PER_BATCH)
            };
            let _ = l2b.record(batch, LINES_PER_BATCH, cum);
            i += 1;
        }
        if l2b.settle() == Err(LF_ECHAIN) {
            r[2] = true;
        }
    }

    let mut l3 = build_final();
    if l3.is_frozen()
        && l3.record(BATCHES + 1, LINES_PER_BATCH, TOTAL_LINES) == Err(LF_EFROZEN)
        && l3.settle() == Err(LF_EFROZEN)
    {
        r[3] = true;
    }
    r
}

/// 自检：13 项全绿。
pub fn run_unx11164_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11164");

    let base = build_final();
    s.add(
        "X1-账构建",
        base.len() == BATCHES && base.accepts == 10 && base.rejects == 0 && base.is_frozen(),
        "十批全收、零拒收且已锁定",
    );
    s.add_dyn(
        "X2-逐批对平",
        base.batches_canonical(),
        &format!("batches={} sum={}", base.len(), base.lines_sum()),
    );
    s.add(
        "X3-总行数",
        base.lines_sum() == TOTAL_LINES,
        "十批合计应为 216000 行",
    );
    s.add(
        "X4-累计链",
        base.chain_ok() && base.gaps().is_empty() && base.order_strict(),
        "累计链逐批连续且批号无缺",
    );

    let mut fz = base.clone();
    let v = fz.freeze();
    s.add(
        "X5-冻结单值",
        v == fz.settled_value() && v == fz.freeze() && fz.is_frozen(),
        "二次冻结同一终值",
    );

    let mut fl = LinesLedger::new();
    let _ = fl.record(1, LINES_PER_BATCH, LINES_PER_BATCH);
    s.add(
        "X6-折叠稳定",
        fl.fold() == fl.fold() && fl.fold() != 0,
        "折叠两次一致且非零",
    );

    let hits = inject_four();
    s.add(
        "X7-四探针",
        hits[0] && hits[1] && hits[2] && hits[3],
        "越界/重复乱序/形状链断/冻结后写四码可达",
    );

    let mut ld = LinesLedger::new();
    let _ = ld.record(1, LINES_PER_BATCH, LINES_PER_BATCH);
    let _ = ld.record(2, LINES_PER_BATCH, 2 * LINES_PER_BATCH);
    let d = ld.record(1, LINES_PER_BATCH, 3 * LINES_PER_BATCH);
    s.add(
        "X8-重复拒收",
        d == Err(LF_EDUP) && ld.accepts == 2 && ld.rejects == 1,
        "重复批号拒收且计数精确",
    );

    let mut lr = LinesLedger::new();
    let _ = lr.record(1, LINES_PER_BATCH, LINES_PER_BATCH);
    let r0 = lr.record(0, LINES_PER_BATCH, LINES_PER_BATCH);
    let r1 = lr.record(11, LINES_PER_BATCH, LINES_PER_BATCH);
    s.add(
        "X9-越界拒收",
        r0 == Err(LF_ENO) && r1 == Err(LF_ENO) && lr.accepts == 1 && lr.rejects == 2,
        "批号 0 与 11 均判越界",
    );

    let mut lo = LinesLedger::new();
    let _ = lo.record(5, LINES_PER_BATCH, 5 * LINES_PER_BATCH);
    let o = lo.record(3, LINES_PER_BATCH, 8 * LINES_PER_BATCH);
    s.add(
        "X10-乱序拒收",
        o == Err(LF_EORDER) && lo.accepts == 1 && lo.rejects == 1,
        "5 后写 3 判乱序",
    );

    let mut ls = LinesLedger::new();
    let shape = ls.record(1, 0, LINES_PER_BATCH);
    let mut lc = LinesLedger::new();
    let mut i = 0usize;
    while i < BATCHES {
        let batch = i + 1;
        let cum = if batch == 5 {
            99_999
        } else {
            (batch as u32).wrapping_mul(LINES_PER_BATCH)
        };
        let _ = lc.record(batch, LINES_PER_BATCH, cum);
        i += 1;
    }
    let ch = lc.settle();
    s.add(
        "X11-形状与链断",
        shape == Err(LF_ESHAPE) && ch == Err(LF_ECHAIN) && ls.rejects == 1 && lc.rejects == 1,
        "零行判形状、累计错位判链断",
    );

    let mut lsum = LinesLedger::new();
    let mut j = 0usize;
    while j < BATCHES {
        let batch = j + 1;
        let lines = if batch == BATCHES { 1_000 } else { LINES_PER_BATCH };
        let _ = lsum.record(batch, lines, 1);
        j += 1;
    }
    let es = lsum.settle();
    let mut lcnt = LinesLedger::new();
    let mut k = 0usize;
    let mut run = 0u32;
    while k < BATCHES {
        let batch = k + 1;
        let lines = if batch == 1 {
            10_000
        } else if batch == 2 {
            33_200
        } else {
            LINES_PER_BATCH
        };
        run = run.wrapping_add(lines);
        let _ = lcnt.record(batch, lines, run);
        k += 1;
    }
    let ec = lcnt.settle();
    s.add(
        "X12-和与口径",
        es == Err(LF_ESUM) && ec == Err(LF_ECOUNT),
        "总行数缺额判和、单批偏额判口径",
    );

    let mut z = build_final();
    let zf = z.freeze();
    s.add_dyn(
        "X13-终值统计",
        zf.is_some() && zf == z.settled_value() && z.accepts == 10 && z.rejects == 0,
        &format!("acc={} rej={} sum={}", z.accepts, z.rejects, z.lines_sum()),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f11164_flow() {
        let mut l = build_final();
        assert_eq!(l.len(), 10);
        assert_eq!(l.lines_sum(), 216_000);
        assert!(l.batches_canonical());
        assert!(l.chain_ok());
        assert!(l.order_strict());
        assert!(l.gaps().is_empty());
        assert!(l.is_frozen());
        assert_eq!(l.accepts, 10);
        assert_eq!(l.rejects, 0);
        let v = l.settled_value();
        assert!(v.is_some());
        assert_eq!(l.freeze(), v);
        assert_eq!(l.freeze(), v);
        assert_eq!(l.item(1).map(|b| b.lines), Some(21_600));
        assert_eq!(l.item(10).map(|b| b.cumulative), Some(216_000));
        assert!(l.item(11).is_none());
        assert!(l.fold() != 0);
        assert_eq!(inject_four(), [true, true, true, true]);
    }

    #[test]
    fn f11164_gates() {
        let mut l = LinesLedger::new();
        assert!(l.record(1, LINES_PER_BATCH, LINES_PER_BATCH).is_ok());
        assert!(l.record(2, LINES_PER_BATCH, 2 * LINES_PER_BATCH).is_ok());
        assert_eq!((l.accepts, l.rejects), (2, 0));
        assert_eq!(
            l.record(1, LINES_PER_BATCH, 3 * LINES_PER_BATCH),
            Err(LF_EDUP)
        );
        assert_eq!((l.accepts, l.rejects), (2, 1));
        assert!(l.record(5, LINES_PER_BATCH, 5 * LINES_PER_BATCH).is_ok());
        assert_eq!((l.accepts, l.rejects), (3, 1));
        assert_eq!(
            l.record(3, LINES_PER_BATCH, 8 * LINES_PER_BATCH),
            Err(LF_EORDER)
        );
        assert_eq!((l.accepts, l.rejects), (3, 2));
        assert_eq!(l.record(6, 0, 6 * LINES_PER_BATCH), Err(LF_ESHAPE));
        assert_eq!((l.accepts, l.rejects), (3, 3));
        assert_eq!(l.record(0, LINES_PER_BATCH, 1), Err(LF_ENO));
        assert_eq!(l.record(11, LINES_PER_BATCH, 1), Err(LF_ENO));
        assert_eq!((l.accepts, l.rejects), (3, 5));
        assert_eq!(l.settle(), Err(LF_ECOUNT));
        assert_eq!((l.accepts, l.rejects), (3, 6));

        let mut short = LinesLedger::new();
        let mut i = 0usize;
        while i < 9 {
            let b = i + 1;
            assert!(short
                .record(b, LINES_PER_BATCH, (b as u32) * LINES_PER_BATCH)
                .is_ok());
            i += 1;
        }
        assert_eq!(short.settle(), Err(LF_ECOUNT));
        assert_eq!((short.accepts, short.rejects), (9, 1));

        let mut sum = LinesLedger::new();
        let mut j = 0usize;
        while j < BATCHES {
            let b = j + 1;
            let lines = if b == BATCHES { 1_000 } else { LINES_PER_BATCH };
            assert!(sum.record(b, lines, 1).is_ok());
            j += 1;
        }
        assert_eq!(sum.settle(), Err(LF_ESUM));
        assert_eq!((sum.accepts, sum.rejects), (10, 1));

        let mut count = LinesLedger::new();
        let mut k = 0usize;
        let mut run = 0u32;
        while k < BATCHES {
            let b = k + 1;
            let lines = if b == 1 {
                10_000
            } else if b == 2 {
                33_200
            } else {
                LINES_PER_BATCH
            };
            run = run.wrapping_add(lines);
            assert!(count.record(b, lines, run).is_ok());
            k += 1;
        }
        assert_eq!(count.lines_sum(), TOTAL_LINES);
        assert_eq!(count.settle(), Err(LF_ECOUNT));
        assert_eq!((count.accepts, count.rejects), (10, 1));

        let mut chain = LinesLedger::new();
        let mut m = 0usize;
        while m < BATCHES {
            let b = m + 1;
            let cum = if b == 5 {
                99_999
            } else {
                (b as u32) * LINES_PER_BATCH
            };
            assert!(chain.record(b, LINES_PER_BATCH, cum).is_ok());
            m += 1;
        }
        assert_eq!(chain.settle(), Err(LF_ECHAIN));
        assert_eq!((chain.accepts, chain.rejects), (10, 1));

        let mut done = build_final();
        assert_eq!(done.settle(), Err(LF_EFROZEN));
        assert_eq!(done.record(11, LINES_PER_BATCH, 1), Err(LF_EFROZEN));
        assert_eq!((done.accepts, done.rejects), (10, 2));
    }

    #[test]
    fn f11164_checks_green() {
        let s = run_unx11164_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert!(!s.truncated());
    }
}
