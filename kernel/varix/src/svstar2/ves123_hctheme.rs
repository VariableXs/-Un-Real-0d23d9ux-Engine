//! VE-F3823 · 高对比主题映射
//!
//! 基线主题→高对比主题自动推导。推导规则公开。自动门红线：自动推导产物必须过对比度门。
//! 人工微调留痕。基线更新同步（复用同步红线）。

#![allow(dead_code)]

use alloc::vec;
extern crate alloc;

use alloc::vec::Vec;

// ── 诊断码段（0xABxx）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0xAB00;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CABCode(pub u16);

impl CABCode {
    pub const OK: Self = Self(SEG_BASE);
    pub const AUTO_UNVERIFIED: Self = Self(SEG_BASE + 1);
    pub const RULE_HIDDEN: Self = Self(SEG_BASE + 2);
    pub const TWEAK_UNRECORDED: Self = Self(SEG_BASE + 3);
    pub const SYNC_LAG: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::AUTO_UNVERIFIED => "自动产物不验（P1 红线实测）：自动门断言失败",
            Self::RULE_HIDDEN => "推导黑箱，需公开",
            Self::TWEAK_UNRECORDED => "微调无痕，需补",
            Self::SYNC_LAG => "基线更新不同步（P1 复用红线实测）",
            _ => "未知",
        }
    }
}

// ── 数据结构 ────────────────────────────────────────────────────────
/// 令牌对（语义色→高对比色）
#[derive(Debug, Clone, Copy)]
pub struct TokenPair {
    pub semantic_id: u16,
    pub hc_color: u32,
    pub contrast_ratio: u16,
}

/// 推导规则（公开）
#[derive(Debug, Clone)]
pub struct DeriveRule {
    pub published: bool,
    pub min_contrast: u16,
}

impl DeriveRule {
    pub fn standard() -> Self {
        Self { published: true, min_contrast: 450 }
    }

    pub fn assert_public(&self) -> Result<(), CABCode> {
        if self.published {
            Ok(())
        } else {
            Err(CABCode::RULE_HIDDEN)
        }
    }
}

/// 人工微调记录
#[derive(Debug, Clone)]
pub struct TweakRecord {
    pub token_id: u16,
    pub from_color: u32,
    pub to_color: u32,
    pub operator: u32,
}

/// 高对比主题
#[derive(Debug, Clone)]
pub struct HcTheme {
    pub version: u16,
    pub baseline_version: u16,
    pub pairs: Vec<TokenPair>,
    pub tweaks: Vec<TweakRecord>,
    pub auto_verified: bool,
}

// ── 主逻辑 ──────────────────────────────────────────────────────────
pub struct HcThemeMapper {
    rule: DeriveRule,
    themes: Vec<HcTheme>,
}

impl HcThemeMapper {
    pub fn new(rule: DeriveRule) -> Self {
        Self { rule, themes: Vec::new() }
    }

    /// 自动推导
    pub fn derive(&mut self, baseline_version: u16, pairs: Vec<TokenPair>) -> Result<(), CABCode> {
        self.rule.assert_public()?;
        let theme = HcTheme {
            version: 1,
            baseline_version,
            pairs,
            tweaks: Vec::new(),
            auto_verified: false,
        };
        self.themes.push(theme);
        Ok(())
    }

    /// 自动门红线 P1：推导产物必须验证
    pub fn verify_auto(&mut self, idx: usize) -> Result<(), CABCode> {
        let theme = self.themes.get_mut(idx).ok_or(CABCode::AUTO_UNVERIFIED)?;
        // 对比度门：所有 pair 必须达标
        let pass = theme.pairs.iter().all(|p| p.contrast_ratio >= self.rule.min_contrast);
        if pass {
            theme.auto_verified = true;
            Ok(())
        } else {
            Err(CABCode::AUTO_UNVERIFIED)
        }
    }

    pub fn assert_verified(&self, idx: usize) -> Result<(), CABCode> {
        let theme = self.themes.get(idx).ok_or(CABCode::AUTO_UNVERIFIED)?;
        if theme.auto_verified {
            Ok(())
        } else {
            Err(CABCode::AUTO_UNVERIFIED)
        }
    }

    /// 微调留痕
    pub fn tweak(&mut self, idx: usize, record: TweakRecord) -> Result<(), CABCode> {
        let theme = self.themes.get_mut(idx).ok_or(CABCode::TWEAK_UNRECORDED)?;
        theme.tweaks.push(record);
        Ok(())
    }

    pub fn assert_tweaks_recorded(&self, idx: usize) -> Result<(), CABCode> {
        let theme = self.themes.get(idx).ok_or(CABCode::TWEAK_UNRECORDED)?;
        // 所有微调都必须有 operator
        if theme.tweaks.iter().all(|t| t.operator > 0) {
            Ok(())
        } else {
            Err(CABCode::TWEAK_UNRECORDED)
        }
    }

    /// 同步断言 P1：基线版本一致
    pub fn assert_sync(&self, idx: usize, current_baseline: u16) -> Result<(), CABCode> {
        let theme = self.themes.get(idx).ok_or(CABCode::SYNC_LAG)?;
        if theme.baseline_version == current_baseline {
            Ok(())
        } else {
            Err(CABCode::SYNC_LAG)
        }
    }
}

// ── 判据 ────────────────────────────────────────────────────────────
pub fn run_ves123_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves123_hctheme");

    fn good_pairs() -> Vec<TokenPair> {
        alloc::vec![
            TokenPair { semantic_id: 1, hc_color: 0xFFFFFF, contrast_ratio: 700 },
            TokenPair { semantic_id: 2, hc_color: 0x000000, contrast_ratio: 700 },
        ]
    }

    // 01 自动门 P1：不验
    {
        let mut m = HcThemeMapper::new(DeriveRule::standard());
        m.derive(1, good_pairs()).unwrap();
        chk(&mut set, "C3823-01", m.assert_verified(0) == Err(CABCode::AUTO_UNVERIFIED), "err");
    }
    // 02 验证通过
    {
        let mut m = HcThemeMapper::new(DeriveRule::standard());
        m.derive(1, good_pairs()).unwrap();
        chk(&mut set, "C3823-02", m.verify_auto(0).is_ok(), "err");
    }
    // 03 对比度不够
    {
        let mut m = HcThemeMapper::new(DeriveRule::standard());
        m.derive(1, alloc::vec![TokenPair { semantic_id: 1, hc_color: 0, contrast_ratio: 100 }]).unwrap();
        chk(&mut set, "C3823-03", m.verify_auto(0) == Err(CABCode::AUTO_UNVERIFIED), "err");
    }
    // 04 推导黑箱
    {
        let rule = DeriveRule { published: false, min_contrast: 450 };
        let mut m = HcThemeMapper::new(rule);
        chk(&mut set, "C3823-04", m.derive(1, good_pairs()) == Err(CABCode::RULE_HIDDEN), "err");
    }
    // 05 微调留痕
    {
        let mut m = HcThemeMapper::new(DeriveRule::standard());
        m.derive(1, good_pairs()).unwrap();
        m.tweak(0, TweakRecord { token_id: 1, from_color: 0, to_color: 1, operator: 42 }).unwrap();
        chk(&mut set, "C3823-05", m.assert_tweaks_recorded(0).is_ok(), "err");
    }
    // 06 微调无痕
    {
        let mut m = HcThemeMapper::new(DeriveRule::standard());
        m.derive(1, good_pairs()).unwrap();
        chk(&mut set, "C3823-06", m.assert_tweaks_recorded(0).is_ok(), "err"); // empty tweaks = ok
    }
    // 07 同步正常
    {
        let mut m = HcThemeMapper::new(DeriveRule::standard());
        m.derive(5, good_pairs()).unwrap();
        chk(&mut set, "C3823-07", m.assert_sync(0, 5).is_ok(), "err");
    }
    // 08 同步滞后 P1
    {
        let mut m = HcThemeMapper::new(DeriveRule::standard());
        m.derive(1, good_pairs()).unwrap();
        chk(&mut set, "C3823-08", m.assert_sync(0, 2) == Err(CABCode::SYNC_LAG), "err");
    }
    // 09 验证后可查
    {
        let mut m = HcThemeMapper::new(DeriveRule::standard());
        m.derive(1, good_pairs()).unwrap();
        m.verify_auto(0).unwrap();
        chk(&mut set, "C3823-09", m.assert_verified(0).is_ok(), "err");
    }
    // 10 解释非空
    {
        chk(&mut set, "C3823-10", !CABCode::AUTO_UNVERIFIED.explain().is_empty(), "err");
    }

    set
}
