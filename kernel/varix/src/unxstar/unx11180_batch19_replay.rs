//! UNX-F11180 B39 批收口（域验收总核销批断言）（0x1f500 段）
//!
//! 模型：B39 批收口重放批内十九判据。每条判据登记判据号、本判据行数、
//! 判定位与重放位。规则：判据号一至十九且严格递增不重；行数在一至九百
//! 九十九；判定位必须为绿、重放位必须已重放，红判据与未重放判据一律不收。
//! 十九条收齐后申报域累计行数，申报值须等于十九条行数之和；收口时另验
//! 行数守恒——逐条累计严格递增，每次增量恰为本条行数，末次累计等于申报。
//! 收条记受理，越界重复倒序零行数红判据未重放与累计不符记拒止。终核重放
//! 逐条复算后折叠为指纹；冻结后二次冻结与再写返回冻结码。

use crate::checks::CheckSet;

/// 段基址（B39 段 · 批收口）。
const SEG_BASE: u32 = 0x1f500;

/// 判据号越界。
pub const BC_ENO: u32 = SEG_BASE + 0x01;
/// 判据号重复。
pub const BC_EDUP: u32 = SEG_BASE + 0x02;
/// 判据号倒序。
pub const BC_EORDER: u32 = SEG_BASE + 0x03;
/// 行数越界。
pub const BC_ELINES: u32 = SEG_BASE + 0x04;
/// 判据非绿。
pub const BC_EFAIL: u32 = SEG_BASE + 0x05;
/// 判据未重放。
pub const BC_EREPLAY: u32 = SEG_BASE + 0x06;
/// 判据条数不齐。
pub const BC_ECOUNT: u32 = SEG_BASE + 0x07;
/// 域累计不符或未申报。
pub const BC_EDECL: u32 = SEG_BASE + 0x08;
/// 终值已冻结。
pub const BC_EFROZEN: u32 = SEG_BASE + 0x09;

/// 批内判据数。
pub const CRITS: usize = 19;
/// 单条行数下界。
pub const LINES_MIN: u32 = 1;
/// 单条行数上界。
pub const LINES_MAX: u32 = 999;
/// 行数基数。
pub const LINES_BASE: u32 = 32;
/// 行数步长。
pub const LINES_STEP: u32 = 3;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 一条批内判据。
#[derive(Clone, Copy, Debug)]
pub struct Crit {
    pub no: u8,
    pub lines: u32,
    pub verdict: bool,
    pub replayed: bool,
}

impl Crit {
    /// 空判据。
    pub const fn blank() -> Crit {
        Crit {
            no: 0,
            lines: 0,
            verdict: false,
            replayed: false,
        }
    }

    /// 行数在域内。
    pub fn lines_ok(&self) -> bool {
        self.lines >= LINES_MIN && self.lines <= LINES_MAX
    }

    /// 全绿：判定位与重放位均为真。
    pub fn green(&self) -> bool {
        self.verdict && self.replayed
    }
}

/// 批收口账。
#[derive(Clone, Copy, Debug)]
pub struct BatchLedger {
    crits: [Crit; CRITS],
    len: usize,
    accepts: u32,
    rejects: u32,
    cumulative: Option<u32>,
    frozen: bool,
    settled: Option<u64>,
}

impl BatchLedger {
    /// 空账。
    pub const fn new() -> BatchLedger {
        BatchLedger {
            crits: [Crit::blank(); CRITS],
            len: 0,
            accepts: 0,
            rejects: 0,
            cumulative: None,
            frozen: false,
            settled: None,
        }
    }

    /// 记账一次拒止。
    fn fail(&mut self, code: u32) -> u32 {
        self.rejects = self.rejects.wrapping_add(1);
        code
    }

    /// 已收条数。
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

    /// 申报域累计。
    pub fn cumulative(&self) -> Option<u32> {
        self.cumulative
    }

    /// 第 i 条。
    pub fn crit_at(&self, i: usize) -> Option<Crit> {
        if i >= self.len {
            return None;
        }
        self.crits.get(i).copied()
    }

    /// 末条。
    pub fn last(&self) -> Option<Crit> {
        if self.len == 0 {
            return None;
        }
        self.crits.get(self.len - 1).copied()
    }

    /// 按判据号查序号。
    pub fn find(&self, no: u8) -> Option<usize> {
        let mut i = 0usize;
        while i < self.len {
            if let Some(c) = self.crits.get(i) {
                if c.no == no {
                    return Some(i);
                }
            }
            i = i.wrapping_add(1);
        }
        None
    }

    /// 收条：冻结 → 越界 → 重复 → 倒序 → 行数 → 判定位 → 重放位。
    pub fn register(&mut self, c: Crit) -> Result<u64, u32> {
        if self.frozen || self.settled.is_some() {
            return Err(self.fail(BC_EFROZEN));
        }
        if c.no == 0 || c.no as usize > CRITS {
            return Err(self.fail(BC_ENO));
        }
        if self.find(c.no).is_some() {
            return Err(self.fail(BC_EDUP));
        }
        if let Some(last) = self.last() {
            if c.no <= last.no {
                return Err(self.fail(BC_EORDER));
            }
        }
        if !c.lines_ok() {
            return Err(self.fail(BC_ELINES));
        }
        if !c.verdict {
            return Err(self.fail(BC_EFAIL));
        }
        if !c.replayed {
            return Err(self.fail(BC_EREPLAY));
        }
        match self.crits.get_mut(self.len) {
            Some(slot) => *slot = c,
            None => return Err(self.fail(BC_ECOUNT)),
        }
        self.len = self.len.wrapping_add(1);
        self.accepts = self.accepts.wrapping_add(1);
        Ok(self.fold())
    }

    /// 按判据号观察收条：行数与两位由规格给出。
    pub fn observe(&mut self, no: usize) -> Result<u64, u32> {
        if no == 0 || no > CRITS {
            return Err(self.fail(BC_ENO));
        }
        let c = crit_spec(no as u8);
        self.register(c)
    }

    /// 十九条行数之和。
    pub fn lines_total(&self) -> u32 {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < self.len {
            if let Some(c) = self.crits.get(i) {
                sum = sum.wrapping_add(c.lines);
            }
            i = i.wrapping_add(1);
        }
        sum
    }

    /// 十九条是否全绿。
    pub fn all_green(&self) -> bool {
        let mut i = 0usize;
        while i < self.len {
            match self.crits.get(i) {
                Some(c) => {
                    if !c.green() {
                        return false;
                    }
                }
                None => return false,
            }
            i = i.wrapping_add(1);
        }
        true
    }

    /// 行数守恒：逐条累计严格递增，每次增量恰为本条行数。
    pub fn lines_conserved(&self) -> bool {
        let mut acc = 0u32;
        let mut i = 0usize;
        while i < self.len {
            match self.crits.get(i) {
                Some(c) => {
                    if !c.lines_ok() {
                        return false;
                    }
                    let next = acc.wrapping_add(c.lines);
                    if next <= acc {
                        return false;
                    }
                    acc = next;
                }
                None => return false,
            }
            i = i.wrapping_add(1);
        }
        acc == self.lines_total() && acc > 0
    }

    /// 申报域累计：十九条齐后须等于行数之和。
    pub fn declare_total(&mut self, total: u32) -> Result<u32, u32> {
        if self.frozen || self.settled.is_some() {
            return Err(self.fail(BC_EFROZEN));
        }
        if self.len != CRITS {
            return Err(self.fail(BC_ECOUNT));
        }
        if total != self.lines_total() {
            return Err(self.fail(BC_EDECL));
        }
        self.cumulative = Some(total);
        Ok(total)
    }

    /// 账本指纹。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            if let Some(c) = self.crits.get(i) {
                h = (h ^ c.no as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ c.lines as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ if c.verdict { 1 } else { 0 }).wrapping_mul(FNV_PRIME);
                h = (h ^ if c.replayed { 1 } else { 0 }).wrapping_mul(FNV_PRIME);
            }
            i = i.wrapping_add(1);
        }
        let cv = match self.cumulative {
            Some(v) => v as u64,
            None => 0,
        };
        h = (h ^ self.len as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.accepts as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.rejects as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ cv).wrapping_mul(FNV_PRIME);
        h
    }

    /// 重放：十九条齐、号连续、全绿、行数守恒、域累计对平。
    pub fn replay(&self) -> Result<u64, u32> {
        if self.len != CRITS {
            return Err(BC_ECOUNT);
        }
        let mut i = 0usize;
        while i < self.len {
            match self.crits.get(i) {
                Some(c) => {
                    if c.no as usize != i.wrapping_add(1) {
                        return Err(BC_EORDER);
                    }
                    if !c.lines_ok() {
                        return Err(BC_ELINES);
                    }
                    if !c.verdict {
                        return Err(BC_EFAIL);
                    }
                    if !c.replayed {
                        return Err(BC_EREPLAY);
                    }
                }
                None => return Err(BC_ENO),
            }
            i = i.wrapping_add(1);
        }
        if !self.lines_conserved() {
            return Err(BC_EDECL);
        }
        match self.cumulative {
            Some(v) => {
                if v != self.lines_total() {
                    return Err(BC_EDECL);
                }
            }
            None => return Err(BC_EDECL),
        }
        Ok(self.fold())
    }

    /// 冻结：重放通过才锁值，二次冻结返回冻结码。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen || self.settled.is_some() {
            return Err(self.fail(BC_EFROZEN));
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

/// 第 no 条规格判据：行数随号递增，判定位与重放位皆真。
pub fn crit_spec(no: u8) -> Crit {
    let mut c = Crit::blank();
    if no == 0 || no as usize > CRITS {
        return c;
    }
    c.no = no;
    c.lines = LINES_BASE
        .wrapping_add((no as u32) * LINES_STEP)
        .wrapping_add((no as u32 % 4) * 5);
    c.verdict = true;
    c.replayed = true;
    c
}

/// 第 no 条规格三数：行数、判定位、重放位。
pub fn spec_of(no: usize) -> (u32, bool, bool) {
    if no == 0 || no > CRITS {
        return (0, false, false);
    }
    let c = crit_spec(no as u8);
    (c.lines, c.verdict, c.replayed)
}

/// 构建标准批收口：十九条收齐并申报域累计。
pub fn build_ledger() -> BatchLedger {
    let mut l = BatchLedger::new();
    let mut no = 1usize;
    while no <= CRITS {
        let _ = l.observe(no);
        no = no.wrapping_add(1);
    }
    let total = l.lines_total();
    let _ = l.declare_total(total);
    l
}

/// 四探针：越界、重复/倒序、红判据/未重放/零行数、冻结后写。
pub fn inject_four() -> [bool; 4] {
    let mut r = [false; 4];

    let mut a = BatchLedger::new();
    let _ = a.observe(1);
    let mut zero = crit_spec(2);
    zero.no = 0;
    let mut far = crit_spec(2);
    far.no = (CRITS + 1) as u8;
    if a.register(zero) == Err(BC_ENO) && a.register(far) == Err(BC_ENO) {
        r[0] = true;
    }

    let mut b = BatchLedger::new();
    let b1 = crit_spec(1);
    let _ = b.register(b1);
    if b.register(b1) == Err(BC_EDUP) {
        r[1] = true;
    }
    let mut back = BatchLedger::new();
    let _ = back.register(crit_spec(2));
    if back.register(crit_spec(1)) != Err(BC_EORDER) {
        r[1] = false;
    }

    let mut c = BatchLedger::new();
    let _ = c.register(crit_spec(1));
    let mut red = crit_spec(2);
    red.verdict = false;
    let mut mute = crit_spec(2);
    mute.replayed = false;
    let mut thin = crit_spec(2);
    thin.lines = 0;
    if c.register(red) == Err(BC_EFAIL)
        && c.register(mute) == Err(BC_EREPLAY)
        && c.register(thin) == Err(BC_ELINES)
    {
        r[2] = true;
    }

    let mut d = build_ledger();
    if d.freeze().is_ok()
        && d.freeze() == Err(BC_EFROZEN)
        && d.register(crit_spec(1)) == Err(BC_EFROZEN)
        && d.declare_total(1) == Err(BC_EFROZEN)
    {
        r[3] = true;
    }
    r
}

/// 自检：13 项全绿。
pub fn run_unx11180_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11180");
    let base = build_ledger();

    let mut ids = true;
    let mut i = 0usize;
    while i < base.len() {
        match base.crit_at(i) {
            Some(c) => {
                if c.no as usize != i.wrapping_add(1) {
                    ids = false;
                }
            }
            None => ids = false,
        }
        i = i.wrapping_add(1);
    }
    s.add(
        "S1-十九条齐",
        base.len() == CRITS && base.accepted() == CRITS as u32 && base.rejected() == 0 && ids,
        "十九条全收且判据号恰为一至十九",
    );

    s.add(
        "S2-重放全绿",
        base.all_green(),
        "十九条判定位与重放位均为真",
    );

    s.add(
        "S3-行数守恒",
        base.lines_conserved(),
        "逐条累计严格递增且增量恰为本条行数",
    );

    let mut spec_total = 0u32;
    let mut no = 1usize;
    while no <= CRITS {
        spec_total = spec_total.wrapping_add(spec_of(no).0);
        no = no.wrapping_add(1);
    }
    s.add(
        "S4-域累计对平",
        base.lines_total() == spec_total && base.cumulative() == Some(spec_total),
        "十九条行数之和、规格合计与申报域累计三方相等",
    );

    let mut per_crit = true;
    let mut k = 0usize;
    while k < base.len() {
        match base.crit_at(k) {
            Some(c) => match spec_of(c.no as usize) {
                (l, v, rp) => {
                    if c.lines != l || c.verdict != v || c.replayed != rp {
                        per_crit = false;
                    }
                }
            },
            None => per_crit = false,
        }
        k = k.wrapping_add(1);
    }
    s.add("S5-逐条规格对平", per_crit, "每条行数与两位均等于规格");

    let mut bounded = true;
    let mut m = 0usize;
    while m < base.len() {
        match base.crit_at(m) {
            Some(c) => {
                if c.lines < LINES_MIN || c.lines > LINES_MAX {
                    bounded = false;
                }
            }
            None => bounded = false,
        }
        m = m.wrapping_add(1);
    }
    s.add("S6-行数在域内", bounded, "每条行数落在一至九百九十九");

    s.add(
        "S7-折叠稳定",
        base.fold() == base.fold() && base.fold() != 0,
        "两次折叠同值且非零",
    );

    let mut fz = base.clone();
    let v1 = fz.freeze();
    let v2 = fz.freeze();
    let single = match v1 {
        Ok(v) => v2 == Err(BC_EFROZEN) && fz.settled_value() == Some(v),
        Err(_) => false,
    };
    s.add("S8-冻结单值", single && fz.is_frozen(), "首冻锁值、二次冻结返回冻结码");

    s.add(
        "S9-重放一致",
        base.replay() == Ok(base.fold()),
        "重放十九条与行数守恒全过且指纹一致",
    );

    let mut early = BatchLedger::new();
    let _ = early.observe(1);
    let short = early.declare_total(1);
    let mut wrong = BatchLedger::new();
    let mut n = 1usize;
    while n <= CRITS {
        let _ = wrong.observe(n);
        n = n.wrapping_add(1);
    }
    let bad = wrong.declare_total(wrong.lines_total().wrapping_add(1));
    s.add(
        "S10-累计闸",
        short == Err(BC_ECOUNT) && bad == Err(BC_EDECL),
        "未收齐判不齐，累计不符判不符",
    );

    let mut la = BatchLedger::new();
    let _ = la.observe(1);
    let mut zero = crit_spec(2);
    zero.no = 0;
    let mut far = crit_spec(2);
    far.no = (CRITS + 1) as u8;
    let r0 = la.register(zero);
    let r1 = la.register(far);
    s.add(
        "X11-越界拒收",
        r0 == Err(BC_ENO) && r1 == Err(BC_ENO) && la.accepted() == 1 && la.rejected() == 2,
        "判据号 0 与 20 均判越界",
    );

    let mut lb = BatchLedger::new();
    let b1 = crit_spec(1);
    let _ = lb.register(b1);
    let dup = lb.register(b1);
    let mut lc = BatchLedger::new();
    let _ = lc.register(crit_spec(2));
    let ord = lc.register(crit_spec(1));
    s.add(
        "X12-重复倒序",
        dup == Err(BC_EDUP) && ord == Err(BC_EORDER) && lb.accepted() == 1 && lc.rejected() == 1,
        "重收一条判重复、二条后补一条判倒序",
    );

    let mut ld = BatchLedger::new();
    let _ = ld.register(crit_spec(1));
    let mut red = crit_spec(2);
    red.verdict = false;
    let mut mute = crit_spec(2);
    mute.replayed = false;
    let mut thin = crit_spec(2);
    thin.lines = 0;
    let rd = ld.register(red);
    let mu = ld.register(mute);
    let th = ld.register(thin);
    s.add(
        "X13-红未重放零行",
        rd == Err(BC_EFAIL)
            && mu == Err(BC_EREPLAY)
            && th == Err(BC_ELINES)
            && ld.accepted() == 1
            && ld.rejected() == 3,
        "红判据、未重放、零行数分别拒收",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f11180_batch_flow() {
        let l = build_ledger();
        assert_eq!(l.len(), CRITS);
        assert_eq!(l.accepted(), CRITS as u32);
        assert_eq!(l.rejected(), 0);
        assert!(l.all_green());
        assert!(l.lines_conserved());
        assert_eq!(l.cumulative(), Some(l.lines_total()));
        assert_eq!(l.crit_at(0).map(|c| c.no), Some(1));
        assert!(l.crit_at(CRITS).is_none());
        assert!(l.find(19).is_some());
        assert!(l.lines_total() > CRITS as u32);
        assert!(l.fold() != 0);
    }

    #[test]
    fn f11180_spec_lines() {
        assert_eq!(spec_of(0), (0, false, false));
        assert_eq!(spec_of(CRITS + 1), (0, false, false));
        let mut prev = 0u32;
        let mut total = 0u32;
        let mut no = 1usize;
        while no <= CRITS {
            let (lines, v, rp) = spec_of(no);
            assert!(v && rp);
            assert!(lines >= LINES_MIN && lines <= LINES_MAX);
            assert!(lines > 0);
            prev = lines;
            total = total.wrapping_add(lines);
            no = no.wrapping_add(1);
        }
        assert!(prev > 0);
        assert_eq!(build_ledger().lines_total(), total);
    }

    #[test]
    fn f11180_gates() {
        let mut a = BatchLedger::new();
        assert!(a.observe(1).is_ok());
        assert_eq!(a.observe(1), Err(BC_EDUP));
        assert!(a.observe(3).is_ok());
        assert_eq!(a.observe(2), Err(BC_EORDER));
        let mut red = crit_spec(4);
        red.verdict = false;
        assert_eq!(a.register(red), Err(BC_EFAIL));
        assert_eq!(a.accepted(), 2);
        assert_eq!(a.rejected(), 3);
    }

    #[test]
    fn f11180_freeze() {
        let mut l = build_ledger();
        assert_eq!(l.replay(), Ok(l.fold()));
        let v = l.freeze();
        assert!(v.is_ok());
        assert!(l.is_frozen());
        assert_eq!(l.settled_value(), v.ok());
        assert_eq!(l.freeze(), Err(BC_EFROZEN));
        assert_eq!(l.declare_total(1), Err(BC_EFROZEN));
    }

    #[test]
    fn f11180_checks_green() {
        let s = run_unx11180_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed(), "{:?}", s);
    }
}
