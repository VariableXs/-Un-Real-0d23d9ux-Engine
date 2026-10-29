//! UNX-F11179 B39 段总账（验收段账闭合）（0x1f4f0 段）
//!
//! 模型：B39 段验收账由七行分段总账构成，每行三数——流入、流出、残差，
//! 行内恒等式为「流入减流出等于残差」，流出不得大于流入。七行收齐后申报
//! 表头三数，表头须与七行合计三方逐一相等；闭合条件为「流入合计减流出合计
//! 等于残差合计」且表头已对上。收行记受理，段号越界、重复、倒序、行内
//! 算式破裂与表头不符、闭合不成立记拒止。终核重放逐行复算再折叠为指纹，
//! 冻结后二次冻结与再写返回冻结码。拒收只针对结构，行内三数的越界值由
//! 规则函数判定，使探针能够真实触达算式与表头两条拒收路径。

use crate::checks::CheckSet;

/// 段基址（B39 段 · 段总账）。
const SEG_BASE: u32 = 0x1f4f0;

/// 段号越界。
pub const GD_ENO: u32 = SEG_BASE + 0x01;
/// 段号重复。
pub const GD_EDUP: u32 = SEG_BASE + 0x02;
/// 段号倒序。
pub const GD_EORDER: u32 = SEG_BASE + 0x03;
/// 行内算式破裂。
pub const GD_EARITH: u32 = SEG_BASE + 0x04;
/// 表头不符或未申报。
pub const GD_EDECL: u32 = SEG_BASE + 0x05;
/// 闭合不成立。
pub const GD_ECLOSE: u32 = SEG_BASE + 0x06;
/// 段数不齐。
pub const GD_ECOUNT: u32 = SEG_BASE + 0x07;
/// 终值已冻结。
pub const GD_EFROZEN: u32 = SEG_BASE + 0x08;

/// 分段数。
pub const SEGS: usize = 7;
/// 流入基数。
pub const INFLOW_BASE: u32 = 1000;
/// 流入步长。
pub const INFLOW_STEP: u32 = 120;
/// 流出基数。
pub const OUTFLOW_BASE: u32 = 400;
/// 流出步长。
pub const OUTFLOW_STEP: u32 = 50;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 一行分段总账。
#[derive(Clone, Copy, Debug)]
pub struct SegRow {
    pub seg: u8,
    pub inflow: u32,
    pub outflow: u32,
    pub residual: u32,
}

impl SegRow {
    /// 空行。
    pub const fn blank() -> SegRow {
        SegRow {
            seg: 0,
            inflow: 0,
            outflow: 0,
            residual: 0,
        }
    }

    /// 行内算式：流出不越流入，残差等于流入减流出。
    pub fn arith_ok(&self) -> bool {
        if self.inflow == 0 || self.outflow > self.inflow {
            return false;
        }
        self.residual == self.inflow.wrapping_sub(self.outflow)
    }
}

/// 段总账。
#[derive(Clone, Copy, Debug)]
pub struct SegmentLedger {
    rows: [SegRow; SEGS],
    len: usize,
    accepts: u32,
    rejects: u32,
    header: Option<(u32, u32, u32)>,
    frozen: bool,
    settled: Option<u64>,
}

impl SegmentLedger {
    /// 空账。
    pub const fn new() -> SegmentLedger {
        SegmentLedger {
            rows: [SegRow::blank(); SEGS],
            len: 0,
            accepts: 0,
            rejects: 0,
            header: None,
            frozen: false,
            settled: None,
        }
    }

    /// 记账一次拒止。
    fn fail(&mut self, code: u32) -> u32 {
        self.rejects = self.rejects.wrapping_add(1);
        code
    }

    /// 已收行数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 受理计数。
    pub fn accepted(&self) -> u32 {
        self.accepts
    }

    /// 拒止计数。
    pub fn rejected(&self) -> u32 {
        self.rejects
    }

    /// 表头三数。
    pub fn header(&self) -> Option<(u32, u32, u32)> {
        self.header
    }

    /// 第 i 行。
    pub fn row_at(&self, i: usize) -> Option<SegRow> {
        if i >= self.len {
            return None;
        }
        self.rows.get(i).copied()
    }

    /// 末行。
    pub fn last(&self) -> Option<SegRow> {
        if self.len == 0 {
            return None;
        }
        self.rows.get(self.len - 1).copied()
    }

    /// 按段号查序号。
    pub fn find(&self, seg: u8) -> Option<usize> {
        let mut i = 0usize;
        while i < self.len {
            if let Some(r) = self.rows.get(i) {
                if r.seg == seg {
                    return Some(i);
                }
            }
            i = i.wrapping_add(1);
        }
        None
    }

    /// 收行：冻结 → 越界 → 重复 → 倒序 → 算式。
    pub fn register(&mut self, r: SegRow) -> Result<u64, u32> {
        if self.frozen || self.settled.is_some() {
            return Err(self.fail(GD_EFROZEN));
        }
        if r.seg == 0 || r.seg as usize > SEGS {
            return Err(self.fail(GD_ENO));
        }
        if self.find(r.seg).is_some() {
            return Err(self.fail(GD_EDUP));
        }
        if let Some(last) = self.last() {
            if r.seg <= last.seg {
                return Err(self.fail(GD_EORDER));
            }
        }
        if !r.arith_ok() {
            return Err(self.fail(GD_EARITH));
        }
        match self.rows.get_mut(self.len) {
            Some(slot) => *slot = r,
            None => return Err(self.fail(GD_ECOUNT)),
        }
        self.len = self.len.wrapping_add(1);
        self.accepts = self.accepts.wrapping_add(1);
        Ok(self.fold())
    }

    /// 按段号观察收行：三数由规格给出。
    pub fn observe(&mut self, seg: usize) -> Result<u64, u32> {
        if seg == 0 || seg > SEGS {
            return Err(self.fail(GD_ENO));
        }
        let r = row_spec(seg as u8);
        self.register(r)
    }

    /// 流入合计。
    pub fn sum_in(&self) -> u32 {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < self.len {
            if let Some(r) = self.rows.get(i) {
                sum = sum.wrapping_add(r.inflow);
            }
            i = i.wrapping_add(1);
        }
        sum
    }

    /// 流出合计。
    pub fn sum_out(&self) -> u32 {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < self.len {
            if let Some(r) = self.rows.get(i) {
                sum = sum.wrapping_add(r.outflow);
            }
            i = i.wrapping_add(1);
        }
        sum
    }

    /// 残差合计。
    pub fn sum_residual(&self) -> u32 {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < self.len {
            if let Some(r) = self.rows.get(i) {
                sum = sum.wrapping_add(r.residual);
            }
            i = i.wrapping_add(1);
        }
        sum
    }

    /// 逐行算式全合且段号连续。
    pub fn rows_ok(&self) -> bool {
        let mut i = 0usize;
        while i < self.len {
            match self.rows.get(i) {
                Some(r) => {
                    if r.seg as usize != i.wrapping_add(1) || !r.arith_ok() {
                        return false;
                    }
                }
                None => return false,
            }
            i = i.wrapping_add(1);
        }
        true
    }

    /// 申报表头：七行齐后，三数须与合计逐一相等。
    pub fn declare_header(&mut self, inflow: u32, outflow: u32, residual: u32) -> Result<u64, u32> {
        if self.frozen || self.settled.is_some() {
            return Err(self.fail(GD_EFROZEN));
        }
        if self.len != SEGS {
            return Err(self.fail(GD_ECOUNT));
        }
        if inflow != self.sum_in() || outflow != self.sum_out() || residual != self.sum_residual() {
            return Err(self.fail(GD_EDECL));
        }
        self.header = Some((inflow, outflow, residual));
        Ok(self.fold())
    }

    /// 验收账闭合：三数合计恒等且表头三方对平。
    pub fn closed(&self) -> bool {
        if self.len != SEGS || self.sum_in() != self.sum_out().wrapping_add(self.sum_residual()) {
            return false;
        }
        match self.header {
            Some((i, o, r)) => {
                i == self.sum_in() && o == self.sum_out() && r == self.sum_residual()
            }
            None => false,
        }
    }

    /// 账本指纹。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            if let Some(r) = self.rows.get(i) {
                h = (h ^ r.seg as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ r.inflow as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ r.outflow as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ r.residual as u64).wrapping_mul(FNV_PRIME);
            }
            i = i.wrapping_add(1);
        }
        let hv = match self.header {
            Some((a, b, c)) => (a as u64).wrapping_mul(3)
                .wrapping_add((b as u64).wrapping_mul(5))
                .wrapping_add(c as u64),
            None => 0,
        };
        h = (h ^ self.len as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.accepts as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.rejects as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ hv).wrapping_mul(FNV_PRIME);
        h
    }

    /// 重放：七行齐、算式全合、表头三方对平、闭合成立。
    pub fn replay(&self) -> Result<u64, u32> {
        if self.len != SEGS {
            return Err(GD_ECOUNT);
        }
        if !self.rows_ok() {
            return Err(GD_EARITH);
        }
        match self.header {
            Some((i, o, r)) => {
                if i != self.sum_in() || o != self.sum_out() || r != self.sum_residual() {
                    return Err(GD_EDECL);
                }
            }
            None => return Err(GD_EDECL),
        }
        if !self.closed() {
            return Err(GD_ECLOSE);
        }
        Ok(self.fold())
    }

    /// 冻结：重放通过才锁值，二次冻结返回冻结码。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen || self.settled.is_some() {
            return Err(self.fail(GD_EFROZEN));
        }
        let v = match self.replay() {
            Ok(v) => v,
            Err(e) => return Err(self.fail(e)),
        };
        self.settled = Some(v);
        self.frozen = true;
        Ok(v)
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

/// 第 seg 段规格行。
pub fn row_spec(seg: u8) -> SegRow {
    let mut r = SegRow::blank();
    if seg == 0 || seg as usize > SEGS {
        return r;
    }
    r.seg = seg;
    r.inflow = INFLOW_BASE.wrapping_add((seg as u32) * INFLOW_STEP);
    r.outflow = OUTFLOW_BASE.wrapping_add((seg as u32) * OUTFLOW_STEP);
    r.residual = r.inflow.wrapping_sub(r.outflow);
    r
}

/// 第 seg 段规格三数。
pub fn spec_of(seg: usize) -> (u32, u32, u32) {
    if seg == 0 || seg > SEGS {
        return (0, 0, 0);
    }
    let r = row_spec(seg as u8);
    (r.inflow, r.outflow, r.residual)
}

/// 构建标准段总账：七行收齐并申报表头。
pub fn build_ledger() -> SegmentLedger {
    let mut l = SegmentLedger::new();
    let mut seg = 1usize;
    while seg <= SEGS {
        let _ = l.observe(seg);
        seg = seg.wrapping_add(1);
    }
    let (i, o, r) = (l.sum_in(), l.sum_out(), l.sum_residual());
    let _ = l.declare_header(i, o, r);
    l
}

/// 四探针：越界、重复/倒序、算式破裂、冻结后写。
pub fn inject_four() -> [bool; 4] {
    let mut r = [false; 4];

    let mut a = SegmentLedger::new();
    let _ = a.observe(1);
    let mut zero = row_spec(2);
    zero.seg = 0;
    let mut far = row_spec(2);
    far.seg = (SEGS + 1) as u8;
    if a.register(zero) == Err(GD_ENO) && a.register(far) == Err(GD_ENO) {
        r[0] = true;
    }

    let mut b = SegmentLedger::new();
    let b1 = row_spec(1);
    let _ = b.register(b1);
    if b.register(b1) == Err(GD_EDUP) {
        r[1] = true;
    }
    let mut back = SegmentLedger::new();
    let _ = back.register(row_spec(2));
    if back.register(row_spec(1)) != Err(GD_EORDER) {
        r[1] = false;
    }

    let mut c = SegmentLedger::new();
    let _ = c.register(row_spec(1));
    let mut over = row_spec(2);
    over.outflow = over.inflow.wrapping_add(1);
    let mut skew = row_spec(2);
    skew.residual = skew.residual.wrapping_add(1);
    if c.register(over) == Err(GD_EARITH) && c.register(skew) == Err(GD_EARITH) {
        r[2] = true;
    }

    let mut d = build_ledger();
    if d.freeze().is_ok()
        && d.freeze() == Err(GD_EFROZEN)
        && d.register(row_spec(1)) == Err(GD_EFROZEN)
        && d.declare_header(1, 1, 1) == Err(GD_EFROZEN)
    {
        r[3] = true;
    }
    r
}

/// 自检：13 项全绿。
pub fn run_unx11179_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11179");
    let base = build_ledger();

    s.add(
        "S1-段账逐行对平",
        base.len() == SEGS
            && base.accepted() == SEGS as u32
            && base.rejected() == 0
            && base.rows_ok(),
        "七行全收，逐行流入减流出等于残差",
    );

    s.add(
        "S2-三数合计对平",
        base.sum_in() == base.sum_out().wrapping_add(base.sum_residual()) && base.sum_in() > 0,
        "流入合计等于流出合计加残差合计",
    );

    s.add("S3-验收账闭合", base.closed(), "七行齐、三数恒等、表头对平");

    let header = base.header();
    s.add(
        "S4-表头三方对平",
        header == Some((base.sum_in(), base.sum_out(), base.sum_residual())),
        "表头流入流出残差与七行合计三方相等",
    );

    let mut safe = true;
    let mut i = 0usize;
    while i < base.len() {
        match base.row_at(i) {
            Some(r) => {
                if r.outflow > r.inflow || r.residual > r.inflow {
                    safe = false;
                }
            }
            None => safe = false,
        }
        i = i.wrapping_add(1);
    }
    s.add("S5-残差在域内", safe, "每行流出与残差均不越流入");

    let mut per_row = true;
    let mut k = 0usize;
    while k < base.len() {
        match base.row_at(k) {
            Some(r) => {
                let spec = spec_of(r.seg as usize);
                if r.inflow != spec.0 || r.outflow != spec.1 || r.residual != spec.2 {
                    per_row = false;
                }
            }
            None => per_row = false,
        }
        k = k.wrapping_add(1);
    }
    s.add("S6-逐行规格对平", per_row, "每行三数均等于规格值");

    s.add(
        "S7-折叠稳定",
        base.fold() == base.fold() && base.fold() != 0,
        "两次折叠同值且非零",
    );

    let mut fz = base.clone();
    let v1 = fz.freeze();
    let v2 = fz.freeze();
    let single = match v1 {
        Ok(v) => v2 == Err(GD_EFROZEN) && fz.settled_value() == Some(v),
        Err(_) => false,
    };
    s.add("S8-冻结单值", single && fz.is_frozen(), "首冻锁值、二次冻结返回冻结码");

    s.add(
        "S9-重放一致",
        base.replay() == Ok(base.fold()),
        "重放七行与表头闭合全过且指纹一致",
    );

    let mut early = SegmentLedger::new();
    let _ = early.observe(1);
    let short = early.declare_header(1, 1, 0);
    let mut no_head = SegmentLedger::new();
    let mut st = 1usize;
    while st <= SEGS {
        let _ = no_head.observe(st);
        st = st.wrapping_add(1);
    }
    let wrong = no_head.declare_header(no_head.sum_in().wrapping_add(1), no_head.sum_out(), no_head.sum_residual());
    s.add(
        "S10-表头闸",
        short == Err(GD_ECOUNT) && wrong == Err(GD_EDECL),
        "未收齐判不齐，表头不符判不符",
    );

    let mut la = SegmentLedger::new();
    let _ = la.observe(1);
    let mut zero = row_spec(2);
    zero.seg = 0;
    let mut far = row_spec(2);
    far.seg = (SEGS + 1) as u8;
    let r0 = la.register(zero);
    let r1 = la.register(far);
    s.add(
        "X11-越界拒收",
        r0 == Err(GD_ENO) && r1 == Err(GD_ENO) && la.accepted() == 1 && la.rejected() == 2,
        "段号 0 与 8 均判越界",
    );

    let mut lb = SegmentLedger::new();
    let b1 = row_spec(1);
    let _ = lb.register(b1);
    let dup = lb.register(b1);
    let mut lc = SegmentLedger::new();
    let _ = lc.register(row_spec(2));
    let ord = lc.register(row_spec(1));
    s.add(
        "X12-重复倒序",
        dup == Err(GD_EDUP) && ord == Err(GD_EORDER) && lb.accepted() == 1 && lc.rejected() == 1,
        "重收一段判重复、二段后补一段判倒序",
    );

    let mut ld = SegmentLedger::new();
    let _ = ld.register(row_spec(1));
    let mut over = row_spec(2);
    over.outflow = over.inflow.wrapping_add(1);
    let mut skew = row_spec(2);
    skew.residual = skew.residual.wrapping_add(1);
    let ov = ld.register(over);
    let sk = ld.register(skew);
    s.add(
        "X13-算式破裂",
        ov == Err(GD_EARITH)
            && sk == Err(GD_EARITH)
            && ld.accepted() == 1
            && ld.rejected() == 2,
        "流出越流入与残差不符分别拒收",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f11179_ledger_flow() {
        let l = build_ledger();
        assert_eq!(l.len(), SEGS);
        assert_eq!(l.accepted(), SEGS as u32);
        assert_eq!(l.rejected(), 0);
        assert!(l.rows_ok());
        assert!(l.closed());
        assert_eq!(l.sum_in(), l.sum_out() + l.sum_residual());
        assert_eq!(l.header(), Some((l.sum_in(), l.sum_out(), l.sum_residual())));
        assert_eq!(l.row_at(0).map(|r| r.seg), Some(1));
        assert!(l.row_at(SEGS).is_none());
        assert!(l.find(7).is_some());
        assert!(l.fold() != 0);
    }

    #[test]
    fn f11179_spec_rows() {
        assert_eq!(spec_of(0), (0, 0, 0));
        assert_eq!(spec_of(SEGS + 1), (0, 0, 0));
        let mut sum_in = 0u32;
        let mut sum_out = 0u32;
        let mut seg = 1usize;
        while seg <= SEGS {
            let (i, o, r) = spec_of(seg);
            assert_eq!(i.wrapping_sub(o), r);
            sum_in = sum_in.wrapping_add(i);
            sum_out = sum_out.wrapping_add(o);
            seg = seg.wrapping_add(1);
        }
        assert_eq!(sum_in - sum_out, build_ledger().sum_residual());
        assert_eq!(row_spec(0).inflow, 0);
        assert!(row_spec(1).arith_ok());
    }

    #[test]
    fn f11179_gates() {
        let mut a = SegmentLedger::new();
        assert!(a.observe(1).is_ok());
        assert_eq!(a.observe(1), Err(GD_EDUP));
        assert!(a.observe(3).is_ok());
        assert_eq!(a.observe(2), Err(GD_EORDER));
        let mut over = row_spec(4);
        over.outflow = over.inflow.wrapping_add(9);
        assert_eq!(a.register(over), Err(GD_EARITH));
        assert_eq!(a.declare_header(1, 1, 0), Err(GD_ECOUNT));
        assert_eq!(a.accepted(), 2);
        assert_eq!(a.rejected(), 4);
    }

    #[test]
    fn f11179_freeze() {
        let mut l = build_ledger();
        assert_eq!(l.replay(), Ok(l.fold()));
        let v = l.freeze();
        assert!(v.is_ok());
        assert!(l.is_frozen());
        assert_eq!(l.settled_value(), v.ok());
        assert_eq!(l.freeze(), Err(GD_EFROZEN));
        let mut open_rows = SegmentLedger::new();
        let mut seg = 1usize;
        while seg <= SEGS {
            let _ = open_rows.observe(seg);
            seg = seg.wrapping_add(1);
        }
        assert_eq!(open_rows.freeze(), Err(GD_EDECL));
    }

    #[test]
    fn f11179_checks_green() {
        let s = run_unx11179_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed(), "{:?}", s);
    }
}
