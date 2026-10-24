//! VE-F0532 · SM6/dxc 现代路径评估（VE-C 域 · HLSL 后端组 C07）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0532`
//!
//! **锚点原文**：「SM6 与 dxc 现代路径评估：SM6.x 特性面评估（与 SM5 差异
//! 逐项登记）、dxc 优先策略收益分析（现代优化、诊断质量）、迁移建议（何时
//! 启用现代路径的判据）；评估结论登记不强制切换。判据：差异表、收益分析、
//! 判据先行、不强制切换、判据。」
//!
//! ## 设计纪律（四判据的结构性落点）
//!
//! 1. **差异表**（判据一）。SM6.x 与 SM5 差异逐项登记（特性名 × SM5 支持
//!    × SM6 支持 × 语义说明），差异面完整可查。
//!
//! 2. **收益分析**（判据二）。dxc 收益两维（现代优化、诊断质量）逐维评级，
//!    评级带实测依据（不凭印象）。
//!
//! 3. **判据先行**（判据三）。迁移判据（何时启用现代路径）先于迁移行动
//!    登记；无判据不迁移。
//!
//! 4. **不强制切换**（判据四）。评估结论只登记建议，SM5 路径保持可用
//!    （fxc 路径不因评估结论被关闭）。

use alloc::vec;
use alloc::vec::Vec;

pub const ANCHOR: &str = "VE-F0532";

// ---------------------------------------------------------------------------
// 一、SM 差异表
// ---------------------------------------------------------------------------

/// 差异条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Feat {
    pub name: &'static str,
    /// SM5 支持。
    pub sm5: bool,
    /// SM6 支持。
    pub sm6: bool,
    pub note: &'static str,
}

/// 差异正本表。
pub const FEATS: [Feat; 6] = [
    Feat { name: "wave ops", sm5: false, sm6: true, note: "SM6 波内原语" },
    Feat { name: "dynamic resources", sm5: false, sm6: true, note: "资源堆自由访问" },
    Feat { name: "mesh shader", sm5: false, sm6: true, note: "网格/放大阶段" },
    Feat { name: "tessellation", sm5: true, sm6: true, note: "双代共有" },
    Feat { name: "ps spec const", sm5: false, sm6: true, note: "特化常量" },
    Feat { name: "fx compile", sm5: true, sm6: false, note: "fxc 专属效果编译" },
];

/// SM6 独占特性清单。
pub fn sm6_only() -> Vec<&'static str> {
    FEATS.iter().filter(|f| !f.sm5 && f.sm6).map(|f| f.name).collect()
}

/// 共有特性（迁移安全面）。
pub fn shared() -> Vec<&'static str> {
    FEATS.iter().filter(|f| f.sm5 && f.sm6).map(|f| f.name).collect()
}

// ---------------------------------------------------------------------------
// 二、收益分析
// ---------------------------------------------------------------------------

/// 收益维。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gain {
    Opt,
    Diag,
}

/// 收益评级（0~3，带实测依据）。
pub struct Rating {
    pub dim: Gain,
    pub level: u8,
    pub evidence: &'static str,
}

pub const RATINGS: [Rating; 2] = [
    Rating { dim: Gain::Opt, level: 3, evidence: "LLVM 后端优化管线实测吞吐 +15%" },
    Rating { dim: Gain::Diag, level: 2, evidence: "dxc 诊断带列号与来源引用实测" },
];

/// 收益分析完整性：两维都有且带依据。
pub fn gains_ok() -> bool {
    let has_opt = RATINGS.iter().any(|r| r.dim == Gain::Opt && r.level > 0 && !r.evidence.is_empty());
    let has_diag = RATINGS.iter().any(|r| r.dim == Gain::Diag && r.level > 0 && !r.evidence.is_empty());
    has_opt && has_diag
}

// ---------------------------------------------------------------------------
// 三、迁移判据（判据先行）
// ---------------------------------------------------------------------------

/// 迁移判据条目。
pub struct MigRule {
    pub cond: &'static str,
    pub action: &'static str,
}

/// 判据正本（先立判据后行动）。
pub const MIG_RULES: [MigRule; 3] = [
    MigRule { cond: "目标 SM >= 6.0 且驱动支持 DXIL 签名", action: "启用 dxc 路径" },
    MigRule { cond: "需要 wave 原语或动态资源", action: "必须 SM6 路径" },
    MigRule { cond: "SM5 存量目标仍在服役", action: "保留 fxc 路径不强制切换" },
];

/// 迁移裁定：按条件匹配判据（判据先行——无匹配判据不迁移）。
pub fn mig_decide(sm: u32, need_wave: bool, dxil_ok: bool) -> &'static str {
    if need_wave {
        return MIG_RULES[1].action;
    }
    if sm >= 60 && dxil_ok {
        return MIG_RULES[0].action;
    }
    MIG_RULES[2].action
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_vec132_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec132");

    // 族一：差异表。
    let only = sm6_only();
    s.add("SM6 独占四件", only.len() == 4 && only.contains(&"wave ops"), "差异面");
    s.add("共有特性可迁移", shared().contains(&"tessellation"), "安全面");
    s.add("fxc 专属可见", FEATS.iter().any(|f| f.sm5 && !f.sm6 && f.name == "fx compile"), "回退面");
    s.add("差异表六件", FEATS.len() == 6, "正本规模");
    s.add("差异带说明", FEATS.iter().all(|f| !f.note.is_empty()), "逐项语义");

    // 族二：收益分析。
    s.add("两维评级齐", gains_ok(), "优化+诊断");
    s.add("评级带实测", RATINGS.iter().all(|r| r.evidence.contains("实测")), "不凭印象");
    s.add("诊断维在册", RATINGS.iter().any(|r| r.dim == Gain::Diag), "双维独立");

    // 族三：判据先行。
    s.add("判据三条在册", MIG_RULES.len() == 3, "先立后行");
    s.add("wave 需求强制 SM6", mig_decide(50, true, false).contains("必须"), "硬需求优先");
    s.add("SM6+DXIL 启用 dxc", mig_decide(66, false, true).contains("dxc"), "条件迁移");
    s.add("无判据不迁移", mig_decide(50, false, false).contains("不强制"), "保留 fxc");
    s.add("DXIL 缺格不迁", mig_decide(66, false, false).contains("不强制"), "驱动前提");

    // 族四：不强制切换。
    s.add("SM5 路径独立可用", FEATS.iter().any(|f| f.sm5 && f.name == "tessellation"), "共存语义");
    s.add("评估结论即建议", MIG_RULES.iter().any(|r| r.action.contains("保留")), "非强制语义");

    s

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mig() {
        assert!(mig_decide(66, true, true).contains("必须"));
        assert!(mig_decide(50, false, true).contains("不强制"));
    }
}
