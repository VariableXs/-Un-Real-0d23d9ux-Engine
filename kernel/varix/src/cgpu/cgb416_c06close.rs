//! CGPU-F0416 · C06 组收口与 C07 移交（目标 320 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - C06 组收口：16 项自检入树/六域集成回归全绿报告/核销清单终版归档/
//!   组双签/台账更新/向 C07 长期运行与统计组移交（集成层稳定——长期
//!   统计有了全域数据源）。
//! - 判据：入树、回归报告、归档、双签、移交、判据。
//!
//! 设计纪律：
//! - 16 项自检入树（C06 组清单固定坐标）；任何红项/缺项收口拒绝。
//! - 六域集成回归全绿报告：矩阵（F0409 语义）跑 full 档，六点各自
//!   「用例数/绿数」汇总，单点有红即报告红。
//! - 核销清单终版归档：F0414 终版闸语义复用——九条全兑三证齐才归档。
//! - 组双签 + 台账更新 + 移交断点（C07 起点 F0417）。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0D30 独占。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x0D30，全仓独占）。
pub const SEG_BASE: u16 = 0x0D30;

pub const E_C6_RED: u16 = SEG_BASE + 1; // 自检有红项
pub const E_C6_MISSING: u16 = SEG_BASE + 2; // 16 项缺
pub const E_C6_REGRESS: u16 = SEG_BASE + 3; // 回归报告有红点
pub const E_C6_ARCHIVE: u16 = SEG_BASE + 4; // 终版归档未齐
pub const E_C6_SIGN: u16 = SEG_BASE + 5; // 双签不成立
pub const E_C6_HANDOFF: u16 = SEG_BASE + 6; // 移交包裹缺件

/// C06 起点锚（移交断点 = F0417）。
pub const C07_BREAKPOINT: u32 = 417;

/// C06 组 16 项自检清单。
pub const C06_ITEMS: [&str; 16] = [
    "集成组开工与预留盘点",
    "D 域帧闭环集成",
    "E 域表面配额集成",
    "H 显存域集成",
    "I 带宽域集成",
    "J 功耗域集成",
    "K 异构域集成",
    "VE 卷 T 域联动预留",
    "集成回归矩阵",
    "集成故障隔离",
    "集成遥测",
    "集成 fuzz",
    "集成 API 冻结 v1",
    "集成文档",
    "集成基准",
    "组收口与移交",
];

/// 六域回归点报告行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegressRow {
    pub point: u8,
    pub cases: u32,
    pub green: u32,
}

impl RegressRow {
    pub fn all_green(&self) -> bool {
        self.cases > 0 && self.green == self.cases
    }
}

/// 六域集成回归全绿报告。
#[derive(Clone, Debug, Default)]
pub struct RegressReport {
    pub rows: Vec<RegressRow>,
}

impl RegressReport {
    pub fn put(&mut self, point: u8, cases: u32, green: u32) -> Result<(), u16> {
        if point >= 6 || green > cases {
            return Err(E_C6_REGRESS);
        }
        match self.rows.iter_mut().find(|r| r.point == point) {
            Some(e) => {
                e.cases = cases;
                e.green = green;
            }
            None => self.rows.push(RegressRow { point, cases, green }),
        }
        Ok(())
    }

    /// 全绿报告闸：六点齐且逐点全绿。
    pub fn validate(&self) -> Result<(), u16> {
        for p in 0..6u8 {
            match self.rows.iter().find(|r| r.point == p) {
                None => return Err(E_C6_REGRESS),
                Some(r) if !r.all_green() => return Err(E_C6_REGRESS),
                _ => {}
            }
        }
        Ok(())
    }

    /// 汇总（报告文本素材）。
    pub fn summary(&self) -> (u32, u32, Vec<u8>) {
        let cases: u32 = self.rows.iter().map(|r| r.cases).sum();
        let green: u32 = self.rows.iter().map(|r| r.green).sum();
        let reds = self
            .rows
            .iter()
            .filter(|r| !r.all_green())
            .map(|r| r.point)
            .collect();
        (green, cases, reds)
    }
}

/// 组双签（与 F0400 同构——角色互异 + 指纹互异 + 同台账）。
#[derive(Clone, Debug, Default)]
pub struct C6DualSign {
    pub sigs: Vec<(u8, u64, u32)>,
}

impl C6DualSign {
    pub fn sign(&mut self, role: u8, who: u64, ledger: u32) -> Result<(), u16> {
        if role > 1 {
            return Err(E_C6_SIGN);
        }
        if self.sigs.iter().any(|(_, w, _)| *w == who) {
            return Err(E_C6_SIGN);
        }
        if let Some((_, _, l)) = self.sigs.first() {
            if *l != ledger {
                return Err(E_C6_SIGN);
            }
        }
        self.sigs.push((role, who, ledger));
        Ok(())
    }

    pub fn complete(&self) -> Result<(), u16> {
        if self.sigs.len() != 2 {
            return Err(E_C6_SIGN);
        }
        if self.sigs[0].0 == self.sigs[1].0 {
            return Err(E_C6_SIGN);
        }
        Ok(())
    }
}

/// C07 移交包裹（数据源就绪声明）。
#[derive(Clone, Debug)]
pub struct C7Handoff {
    /// 遥测/账目数据源清单（长期统计的输入面）。
    pub sources: Vec<String>,
    /// 台账号。
    pub ledger: u32,
    pub breakpoint: u32,
}

impl C7Handoff {
    pub fn new(sources: &[&str], ledger: u32) -> Self {
        C7Handoff {
            sources: sources.iter().map(|s| s.to_string()).collect(),
            ledger,
            breakpoint: C07_BREAKPOINT,
        }
    }

    /// 移交完整：断点正确 + 数据源齐（≥4：集成遥测/开销基准/回归矩阵/
    /// 功耗计量）+ 台账号非零。
    pub fn validate(&self) -> Result<(), u16> {
        if self.breakpoint != C07_BREAKPOINT || self.ledger == 0 {
            return Err(E_C6_HANDOFF);
        }
        if self.sources.len() < 4 {
            return Err(E_C6_HANDOFF);
        }
        Ok(())
    }
}

/// C06 收口门。
#[derive(Clone, Debug, Default)]
pub struct C6CloseGate {
    /// 16 项自检（名 → 绿）。
    pub checks: Vec<(String, bool)>,
    pub regress: RegressReport,
    /// 终版归档行（锚点, 对端, 已兑, 三证齐）。
    pub archive: Vec<(u32, u8, bool, bool)>,
    pub signs: C6DualSign,
    pub handoff: Option<C7Handoff>,
    pub ledger: u32,
}

impl C6CloseGate {
    pub fn check(&mut self, name: &str, green: bool) {
        self.checks.push((name.to_string(), green));
    }

    /// 收口总闸。
    pub fn close(&self) -> Result<(usize, usize), u16> {
        // ① 16 项自检。
        if self.checks.len() < C06_ITEMS.len() {
            return Err(E_C6_MISSING);
        }
        for want in C06_ITEMS {
            match self.checks.iter().find(|(n, _)| n == want) {
                None => return Err(E_C6_MISSING),
                Some((_, g)) if !g => return Err(E_C6_RED),
                _ => {}
            }
        }
        // ② 六域回归全绿。
        self.regress.validate()?;
        // ③ 终版归档：九条全兑全证。
        for (a, p, redeemed, evid) in &self.archive {
            let _ = (a, p);
            if !redeemed || !evid {
                return Err(E_C6_ARCHIVE);
            }
        }
        if self.archive.len() < 9 {
            return Err(E_C6_ARCHIVE);
        }
        // ④ 双签。
        self.signs.complete()?;
        // ⑤ 移交。
        let h = self.handoff.as_ref().ok_or(E_C6_HANDOFF)?;
        h.validate()?;
        if h.ledger != self.ledger {
            return Err(E_C6_HANDOFF);
        }
        let (g, t, _) = self.regress.summary();
        Ok((g as usize, t as usize))
    }
}

// ==========================================================================
// 判据测试
// ==========================================================================
#[cfg(test)]
mod tests {
    use super::*;

    fn gate() -> C6CloseGate {
        let mut g = C6CloseGate::default();
        for name in C06_ITEMS {
            g.check(name, true);
        }
        for p in 0..6u8 {
            g.regress.put(p, 3, 3).ok();
        }
        for (a, p) in [(332u32, b'D'), (342, b'E'), (338, b'H'), (338, b'I'), (338, b'J'), (346, b'K'), (377, b'P'), (378, b'N'), (394, b'U')] {
            g.archive.push((a, p, true, true));
        }
        g.ledger = 600;
        g.signs.sign(0, 0xA11CE, 600).ok();
        g.signs.sign(1, 0xB0B, 600).ok();
        g.handoff = Some(C7Handoff::new(
            &["集成遥测四指标", "开销基准册", "回归矩阵", "功耗计量账"],
            600,
        ));
        g
    }

    #[test]
    fn a01_入树_十六项齐全缺一即拒() {
        let g = gate();
        assert_eq!(g.checks.len(), 16);
        // 缺项。
        let mut g2 = gate();
        g2.checks.pop();
        assert_eq!(g2.close(), Err(E_C6_MISSING));
        // 红项。
        let mut g3 = gate();
        g3.checks[9].1 = false;
        assert_eq!(g3.close(), Err(E_C6_RED));
    }

    #[test]
    fn a02_回归报告_六点全绿闸() {
        let mut g = gate();
        assert_eq!(g.regress.validate(), Ok(()));
        let (green, cases, reds) = g.regress.summary();
        assert_eq!((green, cases, reds.len()), (18, 18, 0));
        // 红点报告。
        let mut g2 = gate();
        g2.regress.put(3, 3, 2).ok();
        assert_eq!(g2.regress.validate(), Err(E_C6_REGRESS));
        let (_, _, reds2) = g2.regress.summary();
        assert_eq!(reds2, vec![3]);
        // 零用例点不算全绿。
        let mut g3 = gate();
        g3.regress.put(5, 0, 0).ok();
        assert_eq!(g3.regress.validate(), Err(E_C6_REGRESS));
        // 绿数超用例数拒。
        assert_eq!(g.regress.put(0, 2, 3), Err(E_C6_REGRESS));
    }

    #[test]
    fn a03_归档_终版九条全兑全证() {
        let g = gate();
        assert_eq!(g.archive.len(), 9);
        // 未兑行挡闸。
        let mut g2 = gate();
        g2.archive[7].2 = false;
        assert_eq!(g2.close(), Err(E_C6_ARCHIVE));
        // 缺证行挡闸。
        let mut g3 = gate();
        g3.archive[0].3 = false;
        assert_eq!(g3.close(), Err(E_C6_ARCHIVE));
        // 归档不足九条挡闸。
        let mut g4 = gate();
        g4.archive.pop();
        assert_eq!(g4.close(), Err(E_C6_ARCHIVE));
    }

    #[test]
    fn a04_双签_角色互异同台账() {
        let mut s = C6DualSign::default();
        s.sign(0, 1, 600).ok();
        s.sign(1, 2, 600).ok();
        assert_eq!(s.complete(), Ok(()));
        // 同人重签。
        let mut s2 = C6DualSign::default();
        s2.sign(0, 1, 600).ok();
        assert_eq!(s2.sign(1, 1, 600), Err(E_C6_SIGN));
        // 台账不一致。
        let mut s3 = C6DualSign::default();
        s3.sign(0, 1, 600).ok();
        assert_eq!(s3.sign(1, 2, 601), Err(E_C6_SIGN));
        // 单签。
        let mut s4 = C6DualSign::default();
        s4.sign(0, 1, 600).ok();
        assert_eq!(s4.complete(), Err(E_C6_SIGN));
    }

    #[test]
    fn a05_移交断点_F417_收口全绿() {
        let mut g = gate();
        assert_eq!(g.close(), Ok((18, 18)));
        // 断点坐标固化。
        assert_eq!(C07_BREAKPOINT, 417);
        assert_eq!(g.handoff.as_ref().and_then(|h| h.validate().err()), None);
        // 数据源不足挡闸。
        let mut g2 = gate();
        g2.handoff = Some(C7Handoff::new(&["只有一源"], 600));
        assert_eq!(g2.close(), Err(E_C6_HANDOFF));
        // 台账不一致挡闸。
        let mut g3 = gate();
        g3.ledger = 0;
        assert_eq!(g3.close(), Err(E_C6_HANDOFF));
    }
}
