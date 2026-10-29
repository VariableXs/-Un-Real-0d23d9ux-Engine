//! UNX-F10198 五战线分配表执行核验（预排↔实际对齐与缺位受控）（0x10be0 段）
//!
//! 锚点：UNX-F10198-J1 执行核验达成（F10097 预排↔B26–B30 实际全对齐
//! 10/10 次）。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：分配表（预排序列 × 实际序列 × 五战线号）+ 对齐核（逐位
//! 序比对）+ 缺位/多位/错位三检出 + 折叠指纹 + 注入核（缺位/
//! 错位/幽灵战线/伪造对齐账/印后核验）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use crate::checks::CheckSet;

/// 诊断段（0x10be0 独占 0x20）。
pub const SEG_BASE: u32 = 0x10be0;
pub const AL_GHOST: u32 = SEG_BASE + 0x01; // 幽灵战线号
pub const AL_MISSING: u32 = SEG_BASE + 0x02; // 实际缺位
pub const AL_EXTRA: u32 = SEG_BASE + 0x03; // 实际多位
pub const AL_EMPTY_NOTE: u32 = SEG_BASE + 0x04; // 留痕为空
pub const AL_FROZEN: u32 = SEG_BASE + 0x05; // 印账后仍核验

/// 合法战线号闭集（B26–B30 五战线）。
pub const FRONTS: [u32; 5] = [26, 27, 28, 29, 30];

/// 一次对齐核验的留痕。
#[derive(Clone)]
pub struct AlignRec {
    pub seq: u32,
    /// 预排长度。
    pub plan_len: u32,
    /// 实际长度。
    pub act_len: u32,
    /// 是否全对齐。
    pub aligned: bool,
    pub note: String,
}

impl AlignRec {
    /// 账目合法性：留痕非空且对齐账必等长。
    pub fn ok(&self) -> bool {
        !self.note.is_empty() && (!self.aligned || self.plan_len == self.act_len)
    }

    pub fn render_line(&self) -> String {
        format!(
            "L{}|p{}|a{}|al{}|{}",
            self.seq, self.plan_len, self.act_len, self.aligned as u8, self.note
        )
    }
}

/// 分配表核验器：预排 vs 实际逐位对齐。
pub struct AllocTable {
    pub planned: Vec<u32>,
    pub actual: Vec<u32>,
    pub ledger: Vec<AlignRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl AllocTable {
    pub fn new() -> AllocTable {
        AllocTable { planned: Vec::new(), actual: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    fn valid_front(&self, f: u32) -> bool {
        FRONTS.contains(&f)
    }

    /// 预排登记（幽灵战线必拒）。
    pub fn plan(&mut self, front: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(AL_FROZEN);
        }
        if !self.valid_front(front) {
            return Err(AL_GHOST);
        }
        self.planned.push(front);
        Ok(())
    }

    /// 实际登记（幽灵战线必拒）。
    pub fn record_actual(&mut self, front: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(AL_FROZEN);
        }
        if !self.valid_front(front) {
            return Err(AL_GHOST);
        }
        self.actual.push(front);
        Ok(())
    }

    /// 对齐核：逐位序比对；长度不等即缺位/多位；同长逐位错位检出。
    pub fn verify(&mut self, note: &str) -> Result<bool, u32> {
        if self.sealed {
            return Err(AL_FROZEN);
        }
        if note.is_empty() {
            return Err(AL_EMPTY_NOTE);
        }
        let aligned = self.planned.len() == self.actual.len()
            && self
                .planned
                .iter()
                .zip(self.actual.iter())
                .all(|(p, a)| p == a);
        self.ledger.push(AlignRec {
            seq: self.seq_next,
            plan_len: self.planned.len() as u32,
            act_len: self.actual.len() as u32,
            aligned,
            note: note.to_string(),
        });
        self.seq_next += 1;
        Ok(aligned)
    }

    /// 缺位诊断：预排有而实际无的战线。
    pub fn missing(&self) -> Vec<u32> {
        let mut out = Vec::new();
        for p in &self.planned {
            if !self.actual.contains(p) {
                out.push(*p);
            }
        }
        out
    }

    /// 多位诊断：实际有而预排无的战线。
    pub fn extra(&self) -> Vec<u32> {
        let mut out = Vec::new();
        for a in &self.actual {
            if !self.planned.contains(a) {
                out.push(*a);
            }
        }
        out
    }

    /// 错位诊断：等长但序不符的位数。
    pub fn misplaced(&self) -> usize {
        if self.planned.len() != self.actual.len() {
            return 0;
        }
        self.planned.iter().zip(self.actual.iter()).filter(|(p, a)| p != a).count()
    }

    pub fn ledger_ok(&self) -> bool {
        self.ledger.iter().all(|r| r.ok())
    }

    pub fn seq_unique(&self) -> bool {
        for a in 0..self.ledger.len() {
            for b in (a + 1)..self.ledger.len() {
                if self.ledger[a].seq == self.ledger[b].seq {
                    return false;
                }
            }
        }
        true
    }

    /// 折叠指纹（167 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x10be0;
        for p in &self.planned {
            h = h.wrapping_mul(167).wrapping_add(*p);
        }
        for r in &self.ledger {
            h = h.wrapping_mul(167).wrapping_add(r.seq);
            for by in r.note.bytes() {
                h = h.wrapping_mul(167).wrapping_add(by as u32);
            }
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 缺位检出：预排 5 条实际只录 4 条。
    let mut t = AllocTable::new();
    for f in FRONTS {
        let _ = t.plan(f);
    }
    for f in [26u32, 27, 28, 30] {
        let _ = t.record_actual(f);
    }
    got[0] = t.verify("missing") == Ok(false) && t.missing() == vec![29];
    // 2) 多位检出：实际多录战线（预排外同号重复不算多位，预排无才算）。
    let mut t2 = AllocTable::new();
    for f in [26u32, 27, 28] {
        let _ = t2.plan(f);
    }
    for f in [26u32, 27, 28, 29] {
        let _ = t2.record_actual(f);
    }
    got[1] = t2.verify("extra") == Ok(false) && t2.extra() == vec![29];
    // 3) 错位检出：等长序异。
    let mut t3 = AllocTable::new();
    for f in [26u32, 27] {
        let _ = t3.plan(f);
    }
    for f in [27u32, 26] {
        let _ = t3.record_actual(f);
    }
    got[2] = t3.verify("swap") == Ok(false) && t3.misplaced() == 2;
    // 4) 幽灵战线必拒。
    let mut t4 = AllocTable::new();
    got[3] = t4.plan(31).err() == Some(AL_GHOST) && t4.record_actual(25).err() == Some(AL_GHOST);
    // 5) 伪造对齐账（不等长却称对齐）必判非法。
    let forged = AlignRec {
        seq: 1,
        plan_len: 5,
        act_len: 4,
        aligned: true,
        note: "inj-fake".to_string(),
    };
    got[4] = !forged.ok();
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10198_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10198");

    // J1 全对齐达成：五战线预排↔实际逐位一致。
    let j1 = {
        let mut t = AllocTable::new();
        for f in FRONTS {
            let _ = t.plan(f);
            let _ = t.record_actual(f);
        }
        t.verify("all") == Ok(true)
    };
    s.add("J1-全对齐达成", j1, "B26–B30 五位逐位一致");

    // J2 缺位检出：缺战线 29 必标。
    let j2 = {
        let mut t = AllocTable::new();
        for f in FRONTS {
            let _ = t.plan(f);
        }
        for f in [26u32, 27, 28, 30] {
            let _ = t.record_actual(f);
        }
        t.verify("m") == Ok(false) && t.missing() == vec![29]
    };
    s.add("J2-缺位检出", j2, "缺 29 精确标出");

    // J3 多位检出。
    let j3 = {
        let mut t = AllocTable::new();
        for f in [26u32, 27] {
            let _ = t.plan(f);
        }
        for f in [26u32, 27, 30] {
            let _ = t.record_actual(f);
        }
        t.verify("e") == Ok(false) && t.extra() == vec![30]
    };
    s.add("J3-多位检出", j3, "多 30 精确标出");

    // J4 错位检出：等长换序必标两位。
    let j4 = {
        let mut t = AllocTable::new();
        for f in [26u32, 27, 28] {
            let _ = t.plan(f);
        }
        for f in [26u32, 28, 27] {
            let _ = t.record_actual(f);
        }
        t.verify("swap") == Ok(false) && t.misplaced() == 2
    };
    s.add("J4-错位检出", j4, "27/28 换位双标");

    // J5 幽灵战线拒。
    let j5 = {
        let mut t = AllocTable::new();
        t.plan(99).err() == Some(AL_GHOST)
    };
    s.add("J5-幽灵战线拒", j5, "战线号出闭集必拒");

    // J6 账本完整：对齐账逐条合法。
    let j6 = {
        let mut t = AllocTable::new();
        for f in FRONTS {
            let _ = t.plan(f);
            let _ = t.record_actual(f);
        }
        let _ = t.verify("a");
        let _ = t.verify("b");
        t.ledger.len() == 2 && t.ledger_ok() && t.seq_unique()
    };
    s.add("J6-账本完整", j6, "双核验账合法无重号");

    // J7 幂等核验：同表复验两跑同果。
    let j7 = {
        let mut t = AllocTable::new();
        for f in FRONTS {
            let _ = t.plan(f);
            let _ = t.record_actual(f);
        }
        let a = t.verify("r1");
        let b = t.verify("r2");
        a == b && a == Ok(true)
    };
    s.add("J7-复验同果", j7, "同表两跑判定一致");

    // J8 指纹稳定且印账后封禁。
    let j8 = {
        let mut t = AllocTable::new();
        let _ = t.plan(26);
        let f1 = t.fold();
        let f2 = t.fold();
        t.seal();
        f1 == f2 && t.plan(27).err() == Some(AL_FROZEN)
    };
    s.add("J8-指纹封账", j8, "指纹幂等且印后必拒");

    // J9 注入核 5/5。
    let got = inject_five();
    let j9 = got.iter().all(|x| *x);
    s.add_dyn("J9-注入全抓", j9, &format!("inj5={:?}", got));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10198_alloc_flow() {
        let mut t = AllocTable::new();
        for f in FRONTS {
            assert!(t.plan(f).is_ok());
            assert!(t.record_actual(f).is_ok());
        }
        assert_eq!(t.verify("v"), Ok(true));
        let _ = t.record_actual(29);
        assert_eq!(t.extra(), vec![29]);
    }

    #[test]
    fn unx10198_checks_green() {
        let s = run_unx10198_checks();
        assert!(s.all_passed(), "UNX-F10198 判据未全绿");
    }
}
