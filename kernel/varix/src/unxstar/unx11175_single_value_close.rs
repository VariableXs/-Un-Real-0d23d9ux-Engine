//! UNX-F11175 验收总核销断言（域级单值）（0x1f4b0 段）
//!
//! 模型：域验收核销写成六道账行，每行登记道号、总数、通过、豁免、失败与行摘要。
//! 行内恒等式为「通过 + 豁免 + 失败 = 总数」，终态要求失败为零。每行按
//! 「总数×1 + 通过×10 + 豁免×100 + 失败×1000」折出一个行值，六行行值之和
//! 即域级单值；账本另记申报单值，申报值必须与复算值逐位相等，否则判申报不符。
//! 账本带版本戳，初始为一，建账过程逐次升版至七；冻结时锁定单值与版本，
//! 冻结后升版与收行一律返回冻结码。收行记受理，越界、重复、倒序、行内
//! 恒等破裂、终态带失败与摘要不符记拒止。终核重放逐行复算后折叠为指纹。

use crate::checks::CheckSet;

/// 段基址（B39 段 · 域级单值）。
const SEG_BASE: u32 = 0x1f4b0;

/// 道号越界。
pub const LV_ENO: u32 = SEG_BASE + 0x01;
/// 道号重复。
pub const LV_EDUP: u32 = SEG_BASE + 0x02;
/// 道号倒序。
pub const LV_EORDER: u32 = SEG_BASE + 0x03;
/// 行内恒等破裂。
pub const LV_ESHAPE: u32 = SEG_BASE + 0x04;
/// 终态带失败。
pub const LV_EFAIL: u32 = SEG_BASE + 0x05;
/// 行摘要不符。
pub const LV_EDIGEST: u32 = SEG_BASE + 0x06;
/// 申报单值不符或未申报。
pub const LV_EDECL: u32 = SEG_BASE + 0x07;
/// 道数不齐。
pub const LV_ECOUNT: u32 = SEG_BASE + 0x08;
/// 终值已冻结。
pub const LV_EFROZEN: u32 = SEG_BASE + 0x09;

/// 道数。
pub const LANES: usize = 6;
/// 单道总数上界。
pub const TOTAL_MAX: u32 = 64;
/// 建账终版版本号。
pub const VERSION: u8 = 7;
/// 版本上界。
pub const MAX_VERSION: u8 = 15;
/// 行值权重：总数、通过、豁免、失败。
pub const W_TOTAL: u64 = 1;
pub const W_PASS: u64 = 10;
pub const W_WAIVE: u64 = 100;
pub const W_FAIL: u64 = 1000;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 一道验收账行。
#[derive(Clone, Copy, Debug)]
pub struct Lane {
    pub no: u8,
    pub total: u32,
    pub passed: u32,
    pub waived: u32,
    pub failed: u32,
    pub digest: u32,
}

impl Lane {
    /// 空行。
    pub const fn blank() -> Lane {
        Lane {
            no: 0,
            total: 0,
            passed: 0,
            waived: 0,
            failed: 0,
            digest: 0,
        }
    }

    /// 行内恒等：通过、豁免、失败三者之和等于总数。
    pub fn shape_ok(&self) -> bool {
        if self.total == 0 || self.total > TOTAL_MAX {
            return false;
        }
        if self.passed == 0 || self.waived > self.total {
            return false;
        }
        self.passed
            .wrapping_add(self.waived)
            .wrapping_add(self.failed)
            == self.total
    }

    /// 终态无失败。
    pub fn final_ok(&self) -> bool {
        self.failed == 0
    }

    /// 行摘要：五字段卷成 32 位。
    pub fn digest_of(no: u8, total: u32, passed: u32, waived: u32, failed: u32) -> u32 {
        let mut x = (no as u32).wrapping_mul(0x9e37_79b1) ^ total;
        x = x.rotate_left(13) ^ passed;
        x = x.wrapping_mul(0x85eb_ca6b) ^ waived;
        x = x.rotate_left(3) ^ failed;
        if x == 0 {
            1
        } else {
            x
        }
    }

    /// 行摘要自洽。
    pub fn digest_ok(&self) -> bool {
        self.digest == Lane::digest_of(self.no, self.total, self.passed, self.waived, self.failed)
    }

    /// 行值（加权）。
    pub fn value(&self) -> u64 {
        let v = (self.total as u64).wrapping_mul(W_TOTAL)
            + (self.passed as u64).wrapping_mul(W_PASS)
            + (self.waived as u64).wrapping_mul(W_WAIVE)
            + (self.failed as u64).wrapping_mul(W_FAIL);
        v
    }
}

/// 域级单值核销账。
#[derive(Clone, Copy, Debug)]
pub struct ValueLedger {
    lanes: [Lane; LANES],
    len: usize,
    accepts: u32,
    rejects: u32,
    declared: Option<u64>,
    version: u8,
    frozen: bool,
    settled: Option<u64>,
}

impl ValueLedger {
    /// 空账，版本从一启。
    pub const fn new() -> ValueLedger {
        ValueLedger {
            lanes: [Lane::blank(); LANES],
            len: 0,
            accepts: 0,
            rejects: 0,
            declared: None,
            version: 1,
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

    /// 版本戳。
    pub fn version(&self) -> u8 {
        self.version
    }

    /// 申报单值。
    pub fn declared(&self) -> Option<u64> {
        self.declared
    }

    /// 第 i 行。
    pub fn lane_at(&self, i: usize) -> Option<Lane> {
        if i >= self.len {
            return None;
        }
        self.lanes.get(i).copied()
    }

    /// 末行。
    pub fn last(&self) -> Option<Lane> {
        if self.len == 0 {
            return None;
        }
        self.lanes.get(self.len - 1).copied()
    }

    /// 按道号查序号。
    pub fn find(&self, no: u8) -> Option<usize> {
        let mut i = 0usize;
        while i < self.len {
            if let Some(l) = self.lanes.get(i) {
                if l.no == no {
                    return Some(i);
                }
            }
            i = i.wrapping_add(1);
        }
        None
    }

    /// 收行：冻结 → 越界 → 重复 → 倒序 → 恒等 → 终态 → 摘要。
    pub fn register(&mut self, l: Lane) -> Result<u64, u32> {
        if self.frozen || self.settled.is_some() {
            return Err(self.fail(LV_EFROZEN));
        }
        if l.no == 0 || l.no as usize > LANES {
            return Err(self.fail(LV_ENO));
        }
        if self.find(l.no).is_some() {
            return Err(self.fail(LV_EDUP));
        }
        if let Some(last) = self.last() {
            if l.no <= last.no {
                return Err(self.fail(LV_EORDER));
            }
        }
        if !l.shape_ok() {
            return Err(self.fail(LV_ESHAPE));
        }
        if !l.final_ok() {
            return Err(self.fail(LV_EFAIL));
        }
        if !l.digest_ok() {
            return Err(self.fail(LV_EDIGEST));
        }
        match self.lanes.get_mut(self.len) {
            Some(slot) => *slot = l,
            None => return Err(self.fail(LV_ECOUNT)),
        }
        self.len = self.len.wrapping_add(1);
        self.accepts = self.accepts.wrapping_add(1);
        Ok(self.fold())
    }

    /// 按道号观察收行：总数按道号递减，通过为总数减本道豁免。
    pub fn observe(&mut self, no: usize) -> Result<u64, u32> {
        if no == 0 || no > LANES {
            return Err(self.fail(LV_ENO));
        }
        let n = no as u8;
        let l = lane_spec(n);
        self.register(l)
    }

    /// 升版一次；冻结后拒绝。
    pub fn bump_version(&mut self) -> Result<u8, u32> {
        if self.frozen || self.settled.is_some() {
            return Err(self.fail(LV_EFROZEN));
        }
        if self.version >= MAX_VERSION {
            return Err(self.fail(LV_ECOUNT));
        }
        self.version = self.version.wrapping_add(1);
        Ok(self.version)
    }

    /// 六行行值之和。
    pub fn single_value(&self) -> u64 {
        let mut sum = 0u64;
        let mut i = 0usize;
        while i < self.len {
            if let Some(l) = self.lanes.get(i) {
                sum = sum.wrapping_add(l.value());
            }
            i = i.wrapping_add(1);
        }
        sum
    }

    /// 逐行贡献：第 i 行值与规格值相等。
    pub fn contributions_ok(&self) -> bool {
        let mut i = 0usize;
        while i < self.len {
            match self.lanes.get(i) {
                Some(l) => {
                    if l.value() != lane_spec(l.no).value() {
                        return false;
                    }
                }
                None => return false,
            }
            i = i.wrapping_add(1);
        }
        true
    }

    /// 申报单值：必须与当前复算值相等，否则记拒止。
    pub fn declare_single(&mut self, v: u64) -> Result<u64, u32> {
        if self.frozen || self.settled.is_some() {
            return Err(self.fail(LV_EFROZEN));
        }
        if self.len != LANES {
            return Err(self.fail(LV_ECOUNT));
        }
        if v != self.single_value() {
            return Err(self.fail(LV_EDECL));
        }
        self.declared = Some(v);
        Ok(v)
    }

    /// 账本指纹。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            if let Some(l) = self.lanes.get(i) {
                h = (h ^ l.no as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ l.total as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ l.passed as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ l.waived as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ l.failed as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ l.digest as u64).wrapping_mul(FNV_PRIME);
            }
            i = i.wrapping_add(1);
        }
        let dv = match self.declared {
            Some(v) => v,
            None => 0,
        };
        h = (h ^ self.len as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.accepts as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.rejects as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.version as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ dv).wrapping_mul(FNV_PRIME);
        h
    }

    /// 重放：六行齐、号连续、恒等终态摘要全过、贡献与申报对平、版本落定。
    pub fn replay(&self) -> Result<u64, u32> {
        if self.len != LANES {
            return Err(LV_ECOUNT);
        }
        let mut i = 0usize;
        while i < self.len {
            let l = match self.lanes.get(i) {
                Some(v) => *v,
                None => return Err(LV_ENO),
            };
            if l.no as usize != i.wrapping_add(1) {
                return Err(LV_EORDER);
            }
            if !l.shape_ok() {
                return Err(LV_ESHAPE);
            }
            if !l.final_ok() {
                return Err(LV_EFAIL);
            }
            if !l.digest_ok() {
                return Err(LV_EDIGEST);
            }
            i = i.wrapping_add(1);
        }
        if !self.contributions_ok() {
            return Err(LV_ESHAPE);
        }
        if self.version != VERSION {
            return Err(LV_ECOUNT);
        }
        match self.declared {
            Some(v) => {
                if v != self.single_value() {
                    return Err(LV_EDECL);
                }
            }
            None => return Err(LV_EDECL),
        }
        Ok(self.fold())
    }

    /// 冻结：重放通过才锁单值，二次冻结返回冻结码。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen || self.settled.is_some() {
            return Err(self.fail(LV_EFROZEN));
        }
        if let Err(e) = self.replay() {
            return Err(self.fail(e));
        }
        let v = self.single_value();
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

/// 第 no 道规格行：总数按道号递减，豁免随道号递增。
pub fn lane_spec(no: u8) -> Lane {
    let mut l = Lane::blank();
    if no == 0 || no as usize > LANES {
        return l;
    }
    l.no = no;
    l.total = 42u32.wrapping_sub((no as u32) * 2);
    l.waived = (no as u32).wrapping_add(1);
    l.passed = l.total.wrapping_sub(l.waived);
    l.failed = 0;
    l.digest = Lane::digest_of(l.no, l.total, l.passed, l.waived, l.failed);
    l
}

/// 收满六道但不申报（供申报探针使用）。
pub fn build_open() -> ValueLedger {
    let mut l = ValueLedger::new();
    let mut no = 1usize;
    while no <= LANES {
        let _ = l.observe(no);
        no = no.wrapping_add(1);
    }
    l
}

/// 构建标准核销账：六道收齐、版本升至七、申报单值。
pub fn build_ledger() -> ValueLedger {
    let mut l = build_open();
    let mut k = 0usize;
    while k < (VERSION as usize - 1) {
        let _ = l.bump_version();
        k = k.wrapping_add(1);
    }
    let v = l.single_value();
    let _ = l.declare_single(v);
    l
}

/// 四探针：越界、重复/倒序、恒等/失败/摘要、冻结后写。
pub fn inject_four() -> [bool; 4] {
    let mut r = [false; 4];

    let mut a = ValueLedger::new();
    let _ = a.observe(1);
    let mut bad = lane_spec(2);
    bad.no = 0;
    if a.register(bad) == Err(LV_ENO) && a.observe(LANES + 1) == Err(LV_ENO) {
        r[0] = true;
    }

    let mut b = ValueLedger::new();
    let b1 = lane_spec(1);
    let _ = b.register(b1);
    let mut b3 = ValueLedger::new();
    let _ = b3.register(lane_spec(3));
    if b.register(b1) == Err(LV_EDUP) && b3.observe(2) == Err(LV_EORDER) {
        r[1] = true;
    }

    let mut c = ValueLedger::new();
    let _ = c.observe(1);
    let mut broken = lane_spec(2);
    broken.passed = 1;
    let mut failing = lane_spec(2);
    failing.failed = 1;
    failing.passed = failing.total.wrapping_sub(failing.waived).wrapping_sub(1);
    let mut forged = lane_spec(2);
    forged.digest ^= 1;
    if c.register(broken) == Err(LV_ESHAPE)
        && c.register(failing) == Err(LV_EFAIL)
        && c.register(forged) == Err(LV_EDIGEST)
    {
        r[2] = true;
    }

    let mut d = build_ledger();
    if d.freeze().is_ok()
        && d.freeze() == Err(LV_EFROZEN)
        && d.bump_version() == Err(LV_EFROZEN)
        && d.register(lane_spec(1)) == Err(LV_EFROZEN)
    {
        r[3] = true;
    }
    r
}

/// 自检：13 项全绿。
pub fn run_unx11175_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11175");
    let base = build_ledger();

    let mut ids = true;
    let mut i = 0usize;
    while i < base.len() {
        match base.lane_at(i) {
            Some(l) => {
                if l.no as usize != i.wrapping_add(1) {
                    ids = false;
                }
            }
            None => ids = false,
        }
        i = i.wrapping_add(1);
    }
    s.add(
        "S1-六道对平",
        base.len() == LANES && base.accepted() == LANES as u32 && base.rejected() == 0 && ids,
        "六道全收且道号恰为一至六",
    );

    let mut shape = true;
    let mut zero_fail = true;
    let mut j = 0usize;
    while j < base.len() {
        match base.lane_at(j) {
            Some(l) => {
                if !l.shape_ok() {
                    shape = false;
                }
                if !l.final_ok() {
                    zero_fail = false;
                }
            }
            None => {
                shape = false;
                zero_fail = false;
            }
        }
        j = j.wrapping_add(1);
    }
    s.add("S2-行内恒等", shape, "每行通过加豁免加失败等于总数");
    s.add("S3-终态零失败", zero_fail, "六行失败列全为零");

    let mut sum = 0u64;
    let mut a = 0u64;
    let mut k = 0usize;
    while k < base.len() {
        match base.lane_at(k) {
            Some(l) => {
                a = a.wrapping_add(
                    (l.total as u64).wrapping_mul(W_TOTAL)
                        + (l.passed as u64).wrapping_mul(W_PASS)
                        + (l.waived as u64).wrapping_mul(W_WAIVE)
                        + (l.failed as u64).wrapping_mul(W_FAIL),
                );
            }
            None => a = a.wrapping_add(1),
        }
        k = k.wrapping_add(1);
    }
    sum = sum.wrapping_add(a);
    s.add(
        "S4-单值可复算",
        sum == base.single_value() && base.single_value() == base.single_value() && sum > 0,
        "独立加权复算与账本单值相等且非零",
    );

    s.add(
        "S5-申报单值对平",
        base.declared() == Some(base.single_value()),
        "申报单值与六行复算值逐位相等",
    );

    s.add(
        "S6-分道贡献对平",
        base.contributions_ok(),
        "每行行值等于本道规格行值",
    );

    s.add(
        "S7-折叠稳定",
        base.fold() == base.fold() && base.fold() != 0,
        "两次折叠同值且非零",
    );

    s.add("S8-版本落定", base.version() == VERSION, "版本戳升至七并落定");

    let mut fz = base.clone();
    let v1 = fz.freeze();
    let v2 = fz.freeze();
    let locked = match v1 {
        Ok(v) => {
            v2 == Err(LV_EFROZEN)
                && fz.settled_value() == Some(v)
                && v == fz.single_value()
                && fz.bump_version() == Err(LV_EFROZEN)
                && fz.version() == VERSION
        }
        Err(_) => false,
    };
    s.add(
        "S9-冻结锁单值与版本",
        locked && fz.is_frozen(),
        "冻结锁定单值与版本，二次冻升版均拒",
    );

    let mut lo = build_open();
    let truth = lo.single_value();
    let wrong = lo.declare_single(truth.wrapping_add(1));
    let right = lo.declare_single(truth);
    s.add(
        "S10-申报不符拒收",
        wrong == Err(LV_EDECL) && right == Ok(truth) && lo.declared() == Some(truth),
        "申报加一判不符，申报真值收下",
    );

    let mut la = ValueLedger::new();
    let _ = la.observe(1);
    let mut bad = lane_spec(2);
    bad.no = 0;
    let r0 = la.register(bad);
    let r1 = la.observe(LANES + 1);
    s.add(
        "X11-越界拒收",
        r0 == Err(LV_ENO) && r1 == Err(LV_ENO) && la.accepted() == 1 && la.rejected() == 2,
        "道号 0 与 7 均判越界",
    );

    let mut lb = ValueLedger::new();
    let b1 = lane_spec(1);
    let _ = lb.register(b1);
    let dup = lb.register(b1);
    let mut lc = ValueLedger::new();
    let _ = lc.register(lane_spec(3));
    let ord = lc.observe(2);
    s.add(
        "X12-重复倒序",
        dup == Err(LV_EDUP) && ord == Err(LV_EORDER) && lb.accepted() == 1 && lc.rejected() == 1,
        "重收一道判重复、三道后补二道判倒序",
    );

    let mut ld = ValueLedger::new();
    let _ = ld.observe(1);
    let mut broken = lane_spec(2);
    broken.passed = 1;
    let mut failing = lane_spec(2);
    failing.failed = 1;
    failing.passed = failing.total.wrapping_sub(failing.waived).wrapping_sub(1);
    let mut forged = lane_spec(2);
    forged.digest ^= 1;
    let sh = ld.register(broken);
    let fa = ld.register(failing);
    let di = ld.register(forged);
    s.add(
        "X13-恒等失败摘要",
        sh == Err(LV_ESHAPE)
            && fa == Err(LV_EFAIL)
            && di == Err(LV_EDIGEST)
            && ld.accepted() == 1
            && ld.rejected() == 3,
        "恒等破裂、终态带失败、摘要不符分别拒收",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f11175_value_flow() {
        let l = build_ledger();
        assert_eq!(l.len(), LANES);
        assert_eq!(l.accepted(), LANES as u32);
        assert_eq!(l.rejected(), 0);
        assert_eq!(l.version(), VERSION);
        assert!(l.contributions_ok());
        let v = l.single_value();
        assert!(v > 0);
        assert_eq!(l.declared(), Some(v));
        assert_eq!(l.lane_at(0).map(|x| x.no), Some(1));
        assert!(l.lane_at(LANES).is_none());
        assert_eq!(l.find(6).map(|i| i), Some(5));
        assert!(l.fold() != 0);
    }

    #[test]
    fn f11175_lane_specs() {
        assert_eq!(lane_spec(0).total, 0);
        assert_eq!(lane_spec(LANES as u8 + 1).total, 0);
        let l1 = lane_spec(1);
        let l6 = lane_spec(6);
        assert!(l1.total > l6.total);
        assert!(l1.shape_ok() && l6.shape_ok());
        assert!(l1.digest_ok() && l6.digest_ok());
        assert_eq!(l1.failed, 0);
        assert_eq!(
            l1.value(),
            (l1.total as u64) + (l1.passed as u64) * 10 + (l1.waived as u64) * 100
        );
    }

    #[test]
    fn f11175_gates() {
        let mut a = ValueLedger::new();
        assert!(a.observe(1).is_ok());
        assert_eq!(a.observe(1), Err(LV_EDUP));
        assert!(a.observe(3).is_ok());
        assert_eq!(a.observe(2), Err(LV_EORDER));
        assert_eq!(a.observe(0), Err(LV_ENO));
        let mut failing = lane_spec(4);
        failing.failed = 2;
        failing.passed = failing.total.wrapping_sub(failing.waived).wrapping_sub(2);
        assert_eq!(a.register(failing), Err(LV_EFAIL));
        assert_eq!(a.accepted(), 2);
        assert_eq!(a.rejected(), 4);
    }

    #[test]
    fn f11175_freeze() {
        let mut l = build_ledger();
        assert_eq!(l.replay(), Ok(l.fold()));
        let v = l.freeze();
        assert!(v.is_ok());
        assert!(l.is_frozen());
        assert_eq!(l.settled_value(), v.ok());
        assert_eq!(l.freeze(), Err(LV_EFROZEN));
        assert_eq!(l.bump_version(), Err(LV_EFROZEN));
        assert_eq!(l.version(), VERSION);
    }

    #[test]
    fn f11175_checks_green() {
        let s = run_unx11175_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed(), "{:?}", s);
    }
}
