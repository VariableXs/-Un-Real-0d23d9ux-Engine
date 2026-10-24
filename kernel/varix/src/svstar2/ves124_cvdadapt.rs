//! VE-F3824 · 色弱渲染适配
//!
//! 三类色弱（红绿/蓝黄/全色弱）模拟与适配。
//! 色弱安全重映射：语义色→色弱可辨映射。可辨断言。语义保持红线（重映射不改语义）。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ── 诊断码段（0xACxx）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0xAC00;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CACCode(pub u16);

impl CACCode {
    pub const OK: Self = Self(SEG_BASE);
    pub const SEMANTIC_BROKEN: Self = Self(SEG_BASE + 1);
    pub const INDISTINGUISHABLE: Self = Self(SEG_BASE + 2);
    pub const SIM_DISTORTED: Self = Self(SEG_BASE + 3);
    pub const TYPE_MISSING: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::SEMANTIC_BROKEN => "重映射破语义（P1 复用红线实测）",
            Self::INDISTINGUISHABLE => "可辨不过：重映射修正（模拟断言）",
            Self::SIM_DISTORTED => "模拟失真，需校准",
            Self::TYPE_MISSING => "三类覆盖断言失败",
            _ => "未知",
        }
    }
}

// ── 数据结构 ────────────────────────────────────────────────────────
/// 色弱类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CvdType {
    RedGreen,
    BlueYellow,
    Total,
}

impl CvdType {
    pub const ALL: [CvdType; 3] = [Self::RedGreen, Self::BlueYellow, Self::Total];
}

/// 语义色
#[derive(Debug, Clone, Copy)]
pub struct SemanticColor {
    pub id: u16,
    pub meaning: u8,
}

/// 重映射规则
#[derive(Debug, Clone, Copy)]
pub struct RemapRule {
    pub cvd: CvdType,
    pub from_id: u16,
    pub to_color: u32,
    pub distinguishable: bool,
    pub semantic_preserved: bool,
}

/// 适配结果
#[derive(Debug, Clone)]
pub struct AdaptResult {
    pub cvd: CvdType,
    pub applied: Vec<RemapRule>,
}

// ── 主逻辑 ──────────────────────────────────────────────────────────
pub struct CvdAdapter {
    rules: Vec<RemapRule>,
    sim_calibrated: bool,
}

impl CvdAdapter {
    pub fn new() -> Self {
        Self { rules: Vec::new(), sim_calibrated: true }
    }

    pub fn add_rule(&mut self, r: RemapRule) {
        self.rules.push(r);
    }

    /// 三类覆盖断言
    pub fn assert_three_types(&self) -> Result<(), CACCode> {
        for t in CvdType::ALL {
            if !self.rules.iter().any(|r| r.cvd == t) {
                return Err(CACCode::TYPE_MISSING);
            }
        }
        Ok(())
    }

    /// 语义保持红线 P1
    pub fn assert_semantic_preserved(&self) -> Result<(), CACCode> {
        if self.rules.iter().any(|r| !r.semantic_preserved) {
            Err(CACCode::SEMANTIC_BROKEN)
        } else {
            Ok(())
        }
    }

    /// 可辨断言
    pub fn assert_distinguishable(&self) -> Result<(), CACCode> {
        if self.rules.iter().any(|r| !r.distinguishable) {
            Err(CACCode::INDISTINGUISHABLE)
        } else {
            Ok(())
        }
    }

    /// 模拟校准
    pub fn set_calibrated(&mut self, ok: bool) {
        self.sim_calibrated = ok;
    }

    pub fn assert_calibrated(&self) -> Result<(), CACCode> {
        if self.sim_calibrated {
            Ok(())
        } else {
            Err(CACCode::SIM_DISTORTED)
        }
    }

    /// 应用重映射
    pub fn apply(&self, cvd: CvdType) -> AdaptResult {
        AdaptResult {
            cvd,
            applied: self.rules.iter().filter(|r| r.cvd == cvd).copied().collect(),
        }
    }
}

impl Default for CvdAdapter {
    fn default() -> Self {
        Self::new()
    }
}

// ── 判据 ────────────────────────────────────────────────────────────
pub fn run_ves124_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves124_cvdadapt");

    fn full_rules() -> CvdAdapter {
        let mut a = CvdAdapter::new();
        for (i, t) in CvdType::ALL.iter().enumerate() {
            a.add_rule(RemapRule {
                cvd: *t,
                from_id: i as u16,
                to_color: 0xFFFFFF,
                distinguishable: true,
                semantic_preserved: true,
            });
        }
        a
    }

    // 01 三类覆盖
    {
        let a = full_rules();
        chk(&mut set, "C3824-01", a.assert_three_types().is_ok(), "err");
    }
    // 02 类型缺失
    {
        let mut a = CvdAdapter::new();
        a.add_rule(RemapRule { cvd: CvdType::RedGreen, from_id: 0, to_color: 0, distinguishable: true, semantic_preserved: true });
        chk(&mut set, "C3824-02", a.assert_three_types() == Err(CACCode::TYPE_MISSING), "err");
    }
    // 03 语义破 P1
    {
        let mut a = full_rules();
        a.add_rule(RemapRule { cvd: CvdType::RedGreen, from_id: 9, to_color: 0, distinguishable: true, semantic_preserved: false });
        chk(&mut set, "C3824-03", a.assert_semantic_preserved() == Err(CACCode::SEMANTIC_BROKEN), "err");
    }
    // 04 可辨不过
    {
        let mut a = full_rules();
        a.add_rule(RemapRule { cvd: CvdType::BlueYellow, from_id: 9, to_color: 0, distinguishable: false, semantic_preserved: true });
        chk(&mut set, "C3824-04", a.assert_distinguishable() == Err(CACCode::INDISTINGUISHABLE), "err");
    }
    // 05 模拟失真
    {
        let mut a = full_rules();
        a.set_calibrated(false);
        chk(&mut set, "C3824-05", a.assert_calibrated() == Err(CACCode::SIM_DISTORTED), "err");
    }
    // 06 应用
    {
        let a = full_rules();
        let r = a.apply(CvdType::RedGreen);
        chk(&mut set, "C3824-06", r.applied.len() == 1, "err");
    }
    // 07 全部断言通过
    {
        let a = full_rules();
        chk(&mut set, "C3824-07", a.assert_semantic_preserved().is_ok() && a.assert_distinguishable().is_ok(), "err");
    }
    // 08 模拟正常
    {
        let a = full_rules();
        chk(&mut set, "C3824-08", a.assert_calibrated().is_ok(), "err");
    }
    // 09 空规则三类
    {
        let a = CvdAdapter::new();
        chk(&mut set, "C3824-09", a.assert_three_types().is_err(), "err");
    }
    // 10 解释非空
    {
        chk(&mut set, "C3824-10", !CACCode::SEMANTIC_BROKEN.explain().is_empty(), "err");
    }

    set
}
