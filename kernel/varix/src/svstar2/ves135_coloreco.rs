//! VE-F3835 · 色彩适配与 E 域联动
//!
//! 高对比/色弱×E 域审查协同。标准对齐。双端对拍。单源复用声明。

#![allow(dead_code)]

use alloc::vec;
extern crate alloc;

use alloc::vec::Vec;

// ── 诊断码段（0xB7xx）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0xB700;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CB7Code(pub u16);

impl CB7Code {
    pub const OK: Self = Self(SEG_BASE);
    pub const FORK: Self = Self(SEG_BASE + 1);
    pub const STANDARD_MISMATCH: Self = Self(SEG_BASE + 2);
    pub const DUALSIGN_MISSING: Self = Self(SEG_BASE + 3);
    pub const SOURCE_BROKEN: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::FORK => "分叉检出（红线）：双端结论不一致",
            Self::STANDARD_MISMATCH => "标准不一致，需对齐",
            Self::DUALSIGN_MISSING => "双签缺失，不生效",
            Self::SOURCE_BROKEN => "单源破，需归一",
            _ => "未知",
        }
    }
}

// ── 数据结构 ────────────────────────────────────────────────────────
/// 色彩审查结论
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorVerdict {
    Pass,
    Warn,
    Block,
}

/// 双端审查
#[derive(Debug, Clone, Copy)]
pub struct DualColorReview {
    pub color_verdict: ColorVerdict,
    pub e_domain_verdict: ColorVerdict,
    pub color_score: u16,
    pub e_domain_score: u16,
}

/// 联动配置
#[derive(Debug, Clone)]
pub struct EColorLink {
    pub single_source: bool,
    pub dual_sign: bool,
    pub reviews: Vec<DualColorReview>,
}

// ── 主逻辑 ──────────────────────────────────────────────────────────
pub struct ColorEcoLink {
    config: EColorLink,
}

impl ColorEcoLink {
    pub fn new(config: EColorLink) -> Self {
        Self { config }
    }

    /// 分叉红线
    pub fn assert_consistent(&self) -> Result<(), CB7Code> {
        for r in &self.config.reviews {
            if r.color_verdict != r.e_domain_verdict {
                return Err(CB7Code::FORK);
            }
        }
        Ok(())
    }

    /// 标准对齐（分差≤5）
    pub fn assert_aligned(&self) -> Result<(), CB7Code> {
        for r in &self.config.reviews {
            let d = (r.color_score as i32 - r.e_domain_score as i32).abs();
            if d > 5 {
                return Err(CB7Code::STANDARD_MISMATCH);
            }
        }
        Ok(())
    }

    /// 单源断言
    pub fn assert_single_source(&self) -> Result<(), CB7Code> {
        if self.config.single_source {
            Ok(())
        } else {
            Err(CB7Code::SOURCE_BROKEN)
        }
    }

    /// 双签断言
    pub fn assert_dual_sign(&self) -> Result<(), CB7Code> {
        if self.config.dual_sign {
            Ok(())
        } else {
            Err(CB7Code::DUALSIGN_MISSING)
        }
    }

    /// 全链核验
    pub fn full_check(&self) -> Result<(), CB7Code> {
        self.assert_single_source()?;
        self.assert_dual_sign()?;
        self.assert_consistent()?;
        self.assert_aligned()?;
        Ok(())
    }
}

// ── 判据 ────────────────────────────────────────────────────────────
pub fn run_ves135_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves135_coloreco");

    fn good_config() -> EColorLink {
        EColorLink {
            single_source: true,
            dual_sign: true,
            reviews: alloc::vec![DualColorReview {
                color_verdict: ColorVerdict::Pass,
                e_domain_verdict: ColorVerdict::Pass,
                color_score: 80,
                e_domain_score: 82,
            }],
        }
    }

    // 01 分叉红线
    {
        let mut cfg = good_config();
        cfg.reviews[0].e_domain_verdict = ColorVerdict::Block;
        let m = ColorEcoLink::new(cfg);
        chk(&mut set, "C3835-01", m.assert_consistent() == Err(CB7Code::FORK), "err");
    }
    // 02 一致
    {
        let m = ColorEcoLink::new(good_config());
        chk(&mut set, "C3835-02", m.assert_consistent().is_ok(), "err");
    }
    // 03 标准不一致
    {
        let mut cfg = good_config();
        cfg.reviews[0].color_score = 95;
        let m = ColorEcoLink::new(cfg);
        chk(&mut set, "C3835-03", m.assert_aligned() == Err(CB7Code::STANDARD_MISMATCH), "err");
    }
    // 04 标准对齐
    {
        let m = ColorEcoLink::new(good_config());
        chk(&mut set, "C3835-04", m.assert_aligned().is_ok(), "err");
    }
    // 05 单源破
    {
        let mut cfg = good_config();
        cfg.single_source = false;
        let m = ColorEcoLink::new(cfg);
        chk(&mut set, "C3835-05", m.assert_single_source() == Err(CB7Code::SOURCE_BROKEN), "err");
    }
    // 06 双签缺
    {
        let mut cfg = good_config();
        cfg.dual_sign = false;
        let m = ColorEcoLink::new(cfg);
        chk(&mut set, "C3835-06", m.assert_dual_sign() == Err(CB7Code::DUALSIGN_MISSING), "err");
    }
    // 07 全链通过
    {
        let m = ColorEcoLink::new(good_config());
        chk(&mut set, "C3835-07", m.full_check().is_ok(), "err");
    }
    // 08 全链失败
    {
        let mut cfg = good_config();
        cfg.single_source = false;
        let m = ColorEcoLink::new(cfg);
        chk(&mut set, "C3835-08", m.full_check().is_err(), "err");
    }
    // 09 空审查
    {
        let cfg = EColorLink { single_source: true, dual_sign: true, reviews: Vec::new() };
        let m = ColorEcoLink::new(cfg);
        chk(&mut set, "C3835-09", m.full_check().is_ok(), "err");
    }
    // 10 解释非空
    {
        chk(&mut set, "C3835-10", !CB7Code::FORK.explain().is_empty(), "err");
    }

    set
}
