//! UNX-F11300 B05 批 ktest 断言集聚合（0xe8b0 段）
//!
//! 一个批次的验收面不是单条判据，而是整批判据的聚合跑道：每条判据有编号、
//! 权重与预期结论，聚合器按编号升序逐条核对“实际结论 == 预期结论”，再给出
//! 通过数、失败数、加权得分与批次闸门。闸门只在“无失败、无缺格、无重复编号、
//! 编号连续”四条同时成立时放行，任何一条不满足都必须整体判红。
//!
//! 聚合器最容易犯的错是把“所有行都通过”当成“批次通过”，却漏掉行数不足或
//! 编号重复——重复编号会让一条判据顶替另一条的结论，缺格会让整批少检。模型
//! 在登记阶段就拒绝重复编号与越界编号，在汇总阶段再核对格数与编号连续性，
//! 两级防线保证“少检”无法伪装成“全绿”。
//!
//! 加权得分用于呈现：权重为 1 的普通判据与权重为 2 的核心判据分别累计，
//! 得分按整数百分比向下取整。得分为 100 并不等于闸门放行，闸门看的是失败数
//! 与完整性，这一点在账本里用“16/19 通过但含 3 失败”的格子显式区分。
//!
//! 账本登记十九格（对应 B05 批 11301~11319），其中三格预期失败，用于验证
//! 失败传播；另有两条全通过变体与一条重复编号变体用于注入检查。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::vec::Vec;

/// 段基址。
pub const SEG_BASE: u32 = 0xe8b0;

/// 编号非法。
pub const AG_EID: u32 = SEG_BASE + 0x01;
/// 顺序错误。
pub const AG_EORDER: u32 = SEG_BASE + 0x02;
/// 重复登记。
pub const AG_EDUP: u32 = SEG_BASE + 0x03;
/// 已冻结。
pub const AG_EFROZEN: u32 = SEG_BASE + 0x04;
/// 观测不存在的格。
pub const AG_ENOBS: u32 = SEG_BASE + 0x05;
/// 结论不符。
pub const AG_EMISMATCH: u32 = SEG_BASE + 0x06;
/// 账本失衡。
pub const AG_EBAL: u32 = SEG_BASE + 0x07;
/// 空账本。
pub const AG_EEMPTY: u32 = SEG_BASE + 0x08;
/// 权重非法。
pub const AG_EWEIGHT: u32 = SEG_BASE + 0x09;
/// 闸门未放行。
pub const AG_EGATE: u32 = SEG_BASE + 0x0a;
/// 编号不连续。
pub const AG_EGAP: u32 = SEG_BASE + 0x0b;
/// 票号不一致。
pub const AG_ESEAL: u32 = SEG_BASE + 0x0c;

/// 批次格数。
pub const SAMPLES: usize = 19;
/// 批次首个编号。
pub const FIRST_ID: u16 = 11301;
/// 核心判据权重。
pub const WEIGHT_CORE: u8 = 2;
/// 普通判据权重。
pub const WEIGHT_PLAIN: u8 = 1;

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 一格判据结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Row {
    pub id: u16,
    pub weight: u8,
    pub passed: bool,
    pub expect_pass: bool,
}

impl Row {
    /// 构造。
    pub const fn make(id: u16, weight: u8, passed: bool, expect_pass: bool) -> Row {
        Row { id, weight, passed, expect_pass }
    }
}

/// 聚合结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Verdict {
    pub total: u16,
    pub passed: u16,
    pub failed: u16,
    pub score: u16,
    pub gate: bool,
}

impl Verdict {
    /// 空结论。
    pub const fn none() -> Verdict {
        Verdict { total: 0, passed: 0, failed: 0, score: 0, gate: false }
    }
}

/// 批次账本。
pub struct Batch {
    rows: Vec<Row>,
    frozen: bool,
    root: u64,
    rejects: u32,
}

impl Batch {
    /// 空账本。
    pub fn new() -> Batch {
        Batch { rows: Vec::new(), frozen: false, root: 0, rejects: 0 }
    }

    /// 登记一格。
    pub fn add(&mut self, id: u16, weight: u8, passed: bool, expect_pass: bool) -> Result<(), u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(AG_EFROZEN);
        }
        if id < FIRST_ID || (id as usize) >= FIRST_ID as usize + SAMPLES {
            self.rejects += 1;
            return Err(AG_EID);
        }
        if weight != WEIGHT_PLAIN && weight != WEIGHT_CORE {
            self.rejects += 1;
            return Err(AG_EWEIGHT);
        }
        let mut i = 0;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                if r.id == id {
                    self.rejects += 1;
                    return Err(AG_EDUP);
                }
            }
            i += 1;
        }
        if self.rows.len() >= SAMPLES {
            self.rejects += 1;
            return Err(AG_EBAL);
        }
        self.rows.push(Row::make(id, weight, passed, expect_pass));
        Ok(())
    }

    /// 格数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// 取行。
    pub fn row(&self, i: usize) -> Option<Row> {
        match self.rows.get(i) {
            Some(v) => Some(*v),
            None => None,
        }
    }

    /// 按编号取行。
    pub fn by_id(&self, id: u16) -> Option<Row> {
        let mut i = 0;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                if r.id == id {
                    return Some(*r);
                }
            }
            i += 1;
        }
        None
    }

    /// 编号升序且连续。
    pub fn contiguous(&self) -> bool {
        if self.rows.len() != SAMPLES {
            return false;
        }
        let mut i = 0;
        while i < self.rows.len() {
            match self.rows.get(i) {
                Some(r) => {
                    if r.id as usize != FIRST_ID as usize + i {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        true
    }

    /// 汇总。
    pub fn verdict(&self) -> Verdict {
        let mut v = Verdict::none();
        let mut i = 0;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                v.total += 1;
                if r.passed {
                    v.passed += 1;
                } else {
                    v.failed += 1;
                }
            }
            i += 1;
        }
        let max = self.max_score();
        if max != 0 {
            v.score = ((self.raw_score() as u32 * 100) / max as u32) as u16;
        }
        v.gate = v.failed == 0 && self.contiguous() && self.audit().is_ok();
        v
    }

    /// 加权原始分。
    pub fn raw_score(&self) -> u16 {
        let mut s = 0u16;
        let mut i = 0;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                if r.passed {
                    s += r.weight as u16;
                }
            }
            i += 1;
        }
        s
    }

    /// 加权满分。
    pub fn max_score(&self) -> u16 {
        let mut s = 0u16;
        let mut i = 0;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                s += r.weight as u16;
            }
            i += 1;
        }
        s
    }

    /// 失败编号列表（最多 8 个）。
    pub fn failed_ids(&self) -> Vec<u16> {
        let mut v = Vec::new();
        let mut i = 0;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                if !r.passed && v.len() < 8 {
                    v.push(r.id);
                }
            }
            i += 1;
        }
        v
    }

    /// 账本自洽：行数、行内结论与预期一致。
    pub fn audit(&self) -> Result<(), u32> {
        if self.rows.is_empty() || self.rows.len() > SAMPLES {
            return Err(AG_EBAL);
        }
        let mut i = 0;
        while i < self.rows.len() {
            let r = match self.rows.get(i) {
                Some(v) => v,
                None => return Err(AG_ENOBS),
            };
            if r.passed != r.expect_pass {
                return Err(AG_EMISMATCH);
            }
            i += 1;
        }
        Ok(())
    }

    /// 票面签名。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                h = fold(h, r.id as u64);
                h = fold(h, r.weight as u64);
                h = fold(h, if r.passed { 1 } else { 0 });
            }
            i += 1;
        }
        h
    }

    /// 冻结。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(AG_EFROZEN);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 冻结根。
    pub fn root(&self) -> u64 {
        self.root
    }

    /// 拒绝计数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 是否冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }
}

fn fold(h: u64, v: u64) -> u64 {
    let mut h = h;
    let mut i = 0u32;
    while i < 8 {
        h ^= (v >> (i * 8)) & 0xff;
        h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}

/// 构建十九格账本：三格失败，其余通过。
pub fn build_batch() -> Batch {
    let mut b = Batch::new();
    let mut i = 0u16;
    while i < SAMPLES as u16 {
        let id = FIRST_ID + i;
        let failed = i == 4 || i == 11 || i == 17;
        let weight = if i % 5 == 0 { WEIGHT_CORE } else { WEIGHT_PLAIN };
        let _ = b.add(id, weight, !failed, !failed);
        i += 1;
    }
    b
}

/// 全通过变体。
pub fn build_green() -> Batch {
    let mut b = Batch::new();
    let mut i = 0u16;
    while i < SAMPLES as u16 {
        let _ = b.add(FIRST_ID + i, WEIGHT_PLAIN, true, true);
        i += 1;
    }
    b
}

/// 四路注入：失败传播、全绿闸门、重复编号、冻结。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let b = build_batch();
    let v = b.verdict();
    let ids = b.failed_ids();
    out[0] = v.total == 19 && v.passed == 16 && v.failed == 3 && !v.gate && ids == [11305u16, 11312, 11318];

    let g = build_green();
    let gv = g.verdict();
    out[1] = gv.total == 19 && gv.failed == 0 && gv.score == 100 && gv.gate;

    let mut d = Batch::new();
    let a = d.add(11301, WEIGHT_PLAIN, true, true).is_ok();
    let b2 = d.add(11301, WEIGHT_PLAIN, true, true) == Err(AG_EDUP);
    let c = d.add(11299, WEIGHT_PLAIN, true, true) == Err(AG_EID);
    let e = d.add(11302, 7, true, true) == Err(AG_EWEIGHT);
    out[2] = a && b2 && c && e && d.len() == 1 && d.rejects() == 3;

    let mut f = build_batch();
    let sig = f.seal();
    let fr = f.freeze();
    let again = f.freeze() == Err(AG_EFROZEN);
    let blo = f.add(11301, WEIGHT_PLAIN, true, true) == Err(AG_EFROZEN);
    out[3] = fr == Ok(sig) && again && blo && f.root() == sig;

    out
}

/// 十三项判据。
pub fn run_unx11300_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11300.ktestagg");
    let b = build_batch();
    let v = b.verdict();

    s.add(
        "AG1-十九格齐备且编号连续",
        b.len() == SAMPLES && b.contiguous() && b.row(0).map(|r| r.id) == Some(FIRST_ID),
        "static",
    );

    s.add_dyn(
        "AG2-三应红十六应绿",
        v.total == 19 && v.passed == 16 && v.failed == 3,
        &format!("p={} f={}", v.passed, v.failed),
    );

    s.add(
        "AG3-失败传播闸门闭合",
        !v.gate && b.audit().is_ok() && b.failed_ids() == [11305u16, 11312, 11318],
        "static",
    );

    s.add(
        "AG4-加权得分向下取整",
        b.raw_score() == 20 && b.max_score() == 23 && v.score == 86,
        "static",
    );

    s.add(
        "AG5-全绿变体闸门放行",
        {
            let g = build_green();
            let gv = g.verdict();
            gv.gate && gv.score == 100 && gv.failed == 0
        },
        "static",
    );

    s.add(
        "AG6-重复编号拒绝",
        {
            let mut d = Batch::new();
            let _ = d.add(11301, WEIGHT_PLAIN, true, true);
            d.add(11301, WEIGHT_CORE, true, true) == Err(AG_EDUP) && d.len() == 1
        },
        "static",
    );

    s.add(
        "AG7-越界编号与权重拒绝",
        {
            let mut d = Batch::new();
            let a = d.add(11300, WEIGHT_PLAIN, true, true) == Err(AG_EID);
            let b2 = d.add(11320, WEIGHT_PLAIN, true, true) == Err(AG_EID);
            let c = d.add(11301, 0, true, true) == Err(AG_EWEIGHT);
            a && b2 && c && d.len() == 0 && d.rejects() == 3
        },
        "static",
    );

    s.add(
        "AG8-缺格判红",
        {
            let mut d = Batch::new();
            let mut i = 0u16;
            while i < 18 {
                let _ = d.add(FIRST_ID + i, WEIGHT_PLAIN, true, true);
                i += 1;
            }
            let dv = d.verdict();
            d.len() == 18 && !d.contiguous() && !dv.gate && dv.failed == 0
        },
        "static",
    );

    s.add(
        "AG9-行内结论须与预期一致",
        {
            let mut d = Batch::new();
            let _ = d.add(11301, WEIGHT_PLAIN, false, true);
            d.audit() == Err(AG_EMISMATCH)
        },
        "static",
    );

    s.add(
        "AG10-按编号取行",
        b.by_id(11301).map(|r| r.passed) == Some(true)
            && b.by_id(11305).map(|r| r.passed) == Some(false)
            && b.by_id(11399).is_none(),
        "static",
    );

    s.add(
        "AG11-签名随结论变化",
        {
            let mut d = Batch::new();
            let _ = d.add(11301, WEIGHT_PLAIN, true, true);
            let sig = d.seal();
            let _ = d.add(11302, WEIGHT_PLAIN, true, true);
            d.seal() != sig
        },
        "static",
    );

    let mut f = build_batch();
    let sig = f.seal();
    let fr = f.freeze();
    let again = f.freeze() == Err(AG_EFROZEN);
    let after = f.add(11301, WEIGHT_PLAIN, true, true) == Err(AG_EFROZEN);
    s.add_dyn(
        "AG12-冻结幂等与拒绝计数",
        fr == Ok(sig) && again && after && f.root() == sig && f.rejects() == 2,
        &format!("root={:016x}", sig),
    );

    let inj = inject_four();
    s.add_dyn("AG13-四路注入全绿", inj == [true; 4], &format!("inject={:?}", inj));

    s
}

/// 摘要。
pub fn summary(b: &Batch) -> alloc::string::String {
    let v = b.verdict();
    format!("total={} pass={} fail={} score={} gate={}", v.total, v.passed, v.failed, v.score, v.gate)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ledger_counts() {
        let b = build_batch();
        let v = b.verdict();
        assert_eq!(b.len(), 19);
        assert_eq!(v.passed, 16);
        assert_eq!(v.failed, 3);
        assert!(!v.gate);
    }

    #[test]
    fn green_gate() {
        let g = build_green();
        assert!(g.verdict().gate);
        assert_eq!(g.verdict().score, 100);
    }

    #[test]
    fn duplicates_and_bounds() {
        let mut d = Batch::new();
        assert_eq!(d.add(11301, WEIGHT_PLAIN, true, true), Ok(()));
        assert_eq!(d.add(11301, WEIGHT_PLAIN, true, true), Err(AG_EDUP));
        assert_eq!(d.add(11320, WEIGHT_PLAIN, true, true), Err(AG_EID));
        assert_eq!(d.add(11302, 3, true, true), Err(AG_EWEIGHT));
    }

    #[test]
    fn gap_fails_gate() {
        let mut d = Batch::new();
        let mut i = 0u16;
        while i < 18 {
            let _ = d.add(FIRST_ID + i, WEIGHT_PLAIN, true, true);
            i += 1;
        }
        assert!(!d.contiguous());
        assert!(!d.verdict().gate);
    }

    #[test]
    fn freeze_and_seal() {
        let mut f = build_batch();
        let sig = f.seal();
        assert_eq!(f.freeze(), Ok(sig));
        assert_eq!(f.freeze(), Err(AG_EFROZEN));
        assert_eq!(f.root(), sig);
    }

    #[test]
    fn checks_green() {
        let s = run_unx11300_checks();
        assert_eq!(s.len(), 13);
        assert_eq!(s.failed_count(), 0);
        assert_eq!(inject_four(), [true; 4]);
        assert!(summary(&build_batch()).starts_with("total=19"));
    }
}
