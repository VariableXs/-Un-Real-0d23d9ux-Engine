//! VE-F3717 · 质量与 E 域审查协同
//!
//! 单源归一：质量引擎无障碍评分复用 E 域引擎。分叉检出→归一。
//! 标准对齐断言：同一主题两边结论必须一致。双端对拍。协同双签。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ── 诊断码段（0xA5xx）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0xA500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CA5Code(pub u16);

impl CA5Code {
    pub const OK: Self = Self(SEG_BASE);
    pub const FORK: Self = Self(SEG_BASE + 1);
    pub const STANDARD_MISMATCH: Self = Self(SEG_BASE + 2);
    pub const DUALSIGN_MISSING: Self = Self(SEG_BASE + 3);
    pub const CONTRADICTION: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::FORK => "分叉检出（红线）：双端结论不一致，需归一",
            Self::STANDARD_MISMATCH => "标准不一致，需对齐+立案",
            Self::DUALSIGN_MISSING => "双签缺失，不生效",
            Self::CONTRADICTION => "结论矛盾，需仲裁",
            _ => "未知",
        }
    }
}

// ── 数据结构 ────────────────────────────────────────────────────────
/// 审查结论
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Pass,
    Warn,
    Block,
}

/// 单源复用声明
#[derive(Debug, Clone)]
pub struct SingleSourceDecl {
    pub engine_id: u32,
    pub reused: bool,
}

/// 双端审查结果
#[derive(Debug, Clone, Copy)]
pub struct DualReview {
    pub quality_verdict: Verdict,
    pub e_domain_verdict: Verdict,
    pub quality_score: u16,
    pub e_domain_score: u16,
}

/// 协同配置
#[derive(Debug, Clone)]
pub struct AuditCollab {
    pub decl: SingleSourceDecl,
    pub dual_sign_required: bool,
    pub quality_signed: bool,
    pub e_domain_signed: bool,
}

impl AuditCollab {
    pub fn standard() -> Self {
        Self {
            decl: SingleSourceDecl { engine_id: 1, reused: true },
            dual_sign_required: true,
            quality_signed: true,
            e_domain_signed: true,
        }
    }

    /// 单源执法：必须复用
    pub fn assert_single_source(&self) -> Result<(), CA5Code> {
        if self.decl.reused {
            Ok(())
        } else {
            Err(CA5Code::FORK)
        }
    }

    /// 双签断言
    pub fn assert_dual_sign(&self) -> Result<(), CA5Code> {
        if self.dual_sign_required && !(self.quality_signed && self.e_domain_signed) {
            Err(CA5Code::DUALSIGN_MISSING)
        } else {
            Ok(())
        }
    }
}

// ── 主逻辑 ──────────────────────────────────────────────────────────
pub struct ECoAudit {
    config: AuditCollab,
    reviews: Vec<DualReview>,
}

impl ECoAudit {
    pub fn new(config: AuditCollab) -> Self {
        Self { config, reviews: Vec::new() }
    }

    pub fn add_review(&mut self, r: DualReview) {
        self.reviews.push(r);
    }

    /// 一致断言：同一主题两边结论必须一致
    pub fn assert_consistent(&self) -> Result<(), CA5Code> {
        for r in &self.reviews {
            if r.quality_verdict != r.e_domain_verdict {
                return Err(CA5Code::FORK);
            }
        }
        Ok(())
    }

    /// 标准对齐：分差 ≤5
    pub fn assert_standard_aligned(&self) -> Result<(), CA5Code> {
        for r in &self.reviews {
            let d = (r.quality_score as i32 - r.e_domain_score as i32).abs();
            if d > 5 {
                return Err(CA5Code::STANDARD_MISMATCH);
            }
        }
        Ok(())
    }

    /// 双端对拍
    pub fn dual_check(&self) -> Result<(), CA5Code> {
        self.config.assert_single_source()?;
        self.config.assert_dual_sign()?;
        self.assert_consistent()?;
        self.assert_standard_aligned()?;
        Ok(())
    }

    /// 矛盾仲裁
    pub fn arbitrate(&self) -> Result<Verdict, CA5Code> {
        for r in &self.reviews {
            if r.quality_verdict != r.e_domain_verdict {
                return Err(CA5Code::CONTRADICTION);
            }
        }
        Ok(self.reviews.first().map(|r| r.quality_verdict).unwrap_or(Verdict::Pass))
    }
}

// ── 判据 ────────────────────────────────────────────────────────────
pub fn run_ves117_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves117_ecoaudit");

    // 01 分叉红线：结论不一致
    {
        let mut a = ECoAudit::new(AuditCollab::standard());
        a.add_review(DualReview { quality_verdict: Verdict::Pass, e_domain_verdict: Verdict::Block, quality_score: 80, e_domain_score: 80 });
        chk(&mut set, "C3717-01", a.assert_consistent() == Err(CA5Code::FORK), "err");
    }
    // 02 一致
    {
        let mut a = ECoAudit::new(AuditCollab::standard());
        a.add_review(DualReview { quality_verdict: Verdict::Pass, e_domain_verdict: Verdict::Pass, quality_score: 80, e_domain_score: 82 });
        chk(&mut set, "C3717-02", a.assert_consistent().is_ok(), "err");
    }
    // 03 标准不一致
    {
        let mut a = ECoAudit::new(AuditCollab::standard());
        a.add_review(DualReview { quality_verdict: Verdict::Pass, e_domain_verdict: Verdict::Pass, quality_score: 90, e_domain_score: 70 });
        chk(&mut set, "C3717-03", a.assert_standard_aligned() == Err(CA5Code::STANDARD_MISMATCH), "err");
    }
    // 04 标准对齐
    {
        let mut a = ECoAudit::new(AuditCollab::standard());
        a.add_review(DualReview { quality_verdict: Verdict::Pass, e_domain_verdict: Verdict::Pass, quality_score: 82, e_domain_score: 80 });
        chk(&mut set, "C3717-04", a.assert_standard_aligned().is_ok(), "err");
    }
    // 05 双签缺失
    {
        let cfg = AuditCollab { decl: SingleSourceDecl { engine_id: 1, reused: true }, dual_sign_required: true, quality_signed: true, e_domain_signed: false };
        let a = ECoAudit::new(cfg);
        chk(&mut set, "C3717-05", a.config.assert_dual_sign() == Err(CA5Code::DUALSIGN_MISSING), "err");
    }
    // 06 单源分叉
    {
        let cfg = AuditCollab { decl: SingleSourceDecl { engine_id: 1, reused: false }, dual_sign_required: true, quality_signed: true, e_domain_signed: true };
        let a = ECoAudit::new(cfg);
        chk(&mut set, "C3717-06", a.config.assert_single_source() == Err(CA5Code::FORK), "err");
    }
    // 07 双端对拍全绿
    {
        let mut a = ECoAudit::new(AuditCollab::standard());
        a.add_review(DualReview { quality_verdict: Verdict::Pass, e_domain_verdict: Verdict::Pass, quality_score: 80, e_domain_score: 82 });
        chk(&mut set, "C3717-07", a.dual_check().is_ok(), "err");
    }
    // 08 仲裁矛盾
    {
        let mut a = ECoAudit::new(AuditCollab::standard());
        a.add_review(DualReview { quality_verdict: Verdict::Warn, e_domain_verdict: Verdict::Block, quality_score: 70, e_domain_score: 70 });
        chk(&mut set, "C3717-08", a.arbitrate() == Err(CA5Code::CONTRADICTION), "err");
    }
    // 09 仲裁一致
    {
        let mut a = ECoAudit::new(AuditCollab::standard());
        a.add_review(DualReview { quality_verdict: Verdict::Warn, e_domain_verdict: Verdict::Warn, quality_score: 70, e_domain_score: 71 });
        chk(&mut set, "C3717-09", a.arbitrate() == Ok(Verdict::Warn), "err");
    }
    // 10 解释非空
    {
        chk(&mut set, "C3717-10", !CA5Code::FORK.explain().is_empty(), "err");
    }

    set
}
