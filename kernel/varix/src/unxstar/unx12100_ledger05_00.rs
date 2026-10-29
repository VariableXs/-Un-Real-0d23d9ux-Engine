//! UNX-F12100 B05 批域内集成账与联签清单（0x19ea0 段）
//!
//! 卡面：B05 批域内集成——跨卡联签对清单（如句柄表接口↔表项权限、
//! 结构语义↔对照表等六对），逐对签署、未签对显性、自对偶拒。
//! 域：CoRun·UNX-D1-B05。
//!
//! 判据原文：
//! - J1 联签对闭集：六对跨卡联签对逐一登记。
//! - J2 自对偶拒：对两端同卡必拒。
//! - J3 重对拒：同对二次签署必拒（含换序）。
//! - J4 成员闭集：对端编号须在 B05 卡号闭集 [12081,12098]，越界必拒。
//! - J5 未签显性：清单逐对可查签态，未签对显性。
//! - J6 清单闭合：六对全签后闭合可判。
//! - J7 印账冻结：seal 后签署必拒。
//! - J8 注入五况全抓。
//!
//! 结构：B05Pair 卡号对 + CosignLedger 联签账 + 折叠指纹（211 进制
//! 卷积）+ 注入核（自对偶/越界端/重对/未签判闭合/印后签）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x19ea0 独占 0x20）。
pub const SEG_BASE: u32 = 0x19ea0;
pub const IG_SELF_PAIR: u32 = SEG_BASE + 0x01; // 自对偶
pub const IG_BAD_MEMBER: u32 = SEG_BASE + 0x02; // 越界端
pub const IG_DUP_PAIR: u32 = SEG_BASE + 0x03; // 重对
pub const IG_NOT_CLOSED: u32 = SEG_BASE + 0x04; // 未签判闭合
pub const IG_FROZEN: u32 = SEG_BASE + 0x05; // 印账后签

/// B05 卡号闭集。
pub const B05_LO: u32 = 12081;
pub const B05_HI: u32 = 12098;

/// 六对跨卡联签对（清单锚）。
pub const CANON_PAIRS: [(u32, u32); 6] = [
    (12081, 12098), // 结构语义 ↔ 对照表
    (12084, 12092), // 内核句柄位 ↔ 表项权限
    (12085, 12094), // 位族 ↔ 拒止矩阵
    (12086, 12087), // 相对打开 ↔ 名引用
    (12088, 12089), // 安全描述符 ↔ SQOS
    (12091, 12096), // 句柄表接口 ↔ 对象复制
];

/// 卡号对。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct B05Pair {
    pub a: u32,
    pub b: u32,
}

impl B05Pair {
    /// 对校验：闭集 → 自对偶。
    pub fn validate(&self) -> Result<(), u32> {
        let inb = |x: u32| x >= B05_LO && x <= B05_HI;
        if !inb(self.a) || !inb(self.b) {
            return Err(IG_BAD_MEMBER);
        }
        if self.a == self.b {
            return Err(IG_SELF_PAIR);
        }
        Ok(())
    }
    /// 对偶键（小端在前），用于重对判定。
    pub fn key(&self) -> (u32, u32) {
        if self.a <= self.b {
            (self.a, self.b)
        } else {
            (self.b, self.a)
        }
    }
    pub fn render(&self) -> String {
        format!("P{}<->{}", self.a, self.b)
    }
}

/// 联签账。
pub struct CosignLedger {
    pub pairs: Vec<B05Pair>,
    pub sealed: bool,
}

impl CosignLedger {
    pub fn new() -> CosignLedger {
        CosignLedger { pairs: Vec::new(), sealed: false }
    }

    /// 签署：对校验 → 重对（含换序）。
    pub fn sign(&mut self, a: u32, b: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(IG_FROZEN);
        }
        let p = B05Pair { a, b };
        p.validate()?;
        let k = p.key();
        if self.pairs.iter().any(|q| q.key() == k) {
            return Err(IG_DUP_PAIR);
        }
        self.pairs.push(p);
        Ok(())
    }

    /// 对签态查询。
    pub fn signed(&self, a: u32, b: u32) -> bool {
        let k = (if a <= b { (a, b) } else { (b, a) });
        self.pairs.iter().any(|q| q.key() == k)
    }

    /// 清单闭合：六对全签。
    pub fn closed(&self) -> bool {
        CANON_PAIRS.iter().all(|p| self.signed(p.0, p.1))
    }

    /// 未签对清单。
    pub fn unsigned_pairs(&self) -> Vec<(u32, u32)> {
        CANON_PAIRS
            .iter()
            .filter(|p| !self.signed(p.0, p.1))
            .map(|p| (p.0, p.1))
            .collect()
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for p in &self.pairs {
            let k = p.key();
            h = h.wrapping_mul(211).wrapping_add(k.0);
            h = h.wrapping_mul(211).wrapping_add(k.1);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 诊断面：摘要。
    pub fn summary(&self) -> String {
        format!("pairs={}/{}", self.pairs.len(), CANON_PAIRS.len())
    }

    /// 诊断面：对渲染。
    pub fn line_of(&self, a: u32, b: u32) -> String {
        if self.signed(a, b) {
            format!("S{}<->{}", a, b)
        } else {
            "unsigned".to_string()
        }
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut l = CosignLedger::new();
    // 1) 自对偶必拒。
    got[0] = l.sign(12081, 12081).err() == Some(IG_SELF_PAIR);
    // 2) 越界端必拒。
    got[1] = l.sign(12081, 12099).err() == Some(IG_BAD_MEMBER);
    // 3) 重对必拒（含换序）。
    let _ = l.sign(12081, 12098);
    got[2] = l.sign(12098, 12081).err() == Some(IG_DUP_PAIR);
    // 4) 未签判闭合必拒（闭合判定返回假且未签清单非空）。
    got[3] = !l.closed() && l.unsigned_pairs().len() == 5;
    // 5) 印账后签必抓。
    l.seal();
    got[4] = l.sign(12084, 12092).err() == Some(IG_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12100_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12100");

    // J1 清单闭集：六对全在卡号闭集内且不自对偶。
    let j1 = CANON_PAIRS
        .iter()
        .all(|p| B05Pair { a: p.0, b: p.1 }.validate().is_ok());
    s.add("J1-清单闭集", j1, "六对皆合法");

    // J2 自对偶拒。
    let j2 = {
        let p = B05Pair { a: 12085, b: 12085 };
        p.validate().err() == Some(IG_SELF_PAIR)
    };
    s.add("J2-自对偶拒", j2, "两端同卡必拒");

    // J3 越界端拒。
    let j3 = {
        let p1 = B05Pair { a: 12080, b: 12081 };
        let p2 = B05Pair { a: 12081, b: 12099 };
        p1.validate().err() == Some(IG_BAD_MEMBER)
            && p2.validate().err() == Some(IG_BAD_MEMBER)
    };
    s.add("J3-越界端拒", j3, "闭集外必拒");

    // J4 重对拒（含换序）。
    let j4 = {
        let mut l = CosignLedger::new();
        let _ = l.sign(12088, 12089);
        l.sign(12089, 12088).err() == Some(IG_DUP_PAIR)
    };
    s.add("J4-重对拒", j4, "换序亦判重");

    // J5 六对全签闭合。
    let j5 = {
        let mut l = CosignLedger::new();
        for p in CANON_PAIRS.iter() {
            let _ = l.sign(p.0, p.1);
        }
        l.closed() && l.unsigned_pairs().is_empty()
    };
    s.add("J5-清单闭合", j5, "六签闭合");

    // J6 未签显性。
    let j6 = {
        let mut l = CosignLedger::new();
        let _ = l.sign(12086, 12087);
        l.unsigned_pairs().len() == 5 && l.summary().contains("pairs=1/6")
    };
    s.add("J6-未签显性", j6, "缺五对可查");

    // J7 对偶键稳定。
    let j7 = {
        let p1 = B05Pair { a: 12090, b: 12082 };
        let p2 = B05Pair { a: 12082, b: 12090 };
        p1.key() == p2.key()
    };
    s.add("J7-对偶键", j7, "换序同键");

    // J8 签态行渲染。
    let j8 = {
        let mut l = CosignLedger::new();
        let _ = l.sign(12091, 12096);
        l.line_of(12091, 12096).contains("S") && l.line_of(12085, 12094) == "unsigned"
    };
    s.add("J8-签态渲染", j8, "已签未签分明");

    // J9 印账冻结。
    let j9 = {
        let mut l = CosignLedger::new();
        b05_sign_two(&mut l);
        l.seal();
        l.sign(12088, 12089).err() == Some(IG_FROZEN)
    };
    s.add("J9-印账冻结", j9, "seal 后必拒");

    // J10 指纹幂等。
    let j10 = {
        let mut l = CosignLedger::new();
        let _ = l.sign(12081, 12098);
        l.fold() == l.fold()
    };
    s.add("J10-指纹幂等", j10, "同态同指纹");

    // J11 注入核 5/5。
    let got = inject_five();
    let j11 = got.iter().all(|x| *x);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", got));

    // 汇总样例账：签三对。
    let mut l = CosignLedger::new();
    let _ = l.sign(12081, 12098);
    let _ = l.sign(12084, 12092);
    let _ = l.sign(12085, 12094);

    s.add("摘要回读", l.summary().contains("pairs=3/6"), "三签在账");
    s.add("未签清单", l.unsigned_pairs().len() == 3, "余三对未签");
    s.add("渲染行", l.line_of(12081, 12098).contains("12081"), "首签渲染");
    s
}

/// 辅助：签两对（用于冻结判据构造）。
fn b05_sign_two(l: &mut CosignLedger) {
    let _ = l.sign(12086, 12087);
    let _ = l.sign(12081, 12098);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12100_cosign_flow() {
        let mut l = CosignLedger::new();
        assert!(l.sign(12081, 12098).is_ok());
        assert_eq!(l.sign(12098, 12081), Err(IG_DUP_PAIR));
        assert_eq!(l.sign(12081, 12081), Err(IG_SELF_PAIR));
        assert!(!l.closed());
    }

    #[test]
    fn unx12100_checks_green() {
        let s = run_unx12100_checks();
        assert!(s.all_passed(), "UNX-F12100 判据未全绿");
    }

    #[test]
    fn canon_pair_count() {
        assert_eq!(CANON_PAIRS.len(), 6);
        assert_eq!(B05_LO, 12081);
        assert_eq!(B05_HI, 12098);
    }
}
