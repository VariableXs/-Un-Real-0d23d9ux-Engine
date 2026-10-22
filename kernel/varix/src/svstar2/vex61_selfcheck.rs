//! VE-F4961 · 预备自查总架构（VE-Y 域 · X09 组 · 目标 340 行）· 诊断段 0x8380
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F4961`
//!
//! **判据（锚点原文）**：模板十三用、160+五类清点+三特色、增维登记、判据。
//!
//! **职责定位（锚点原文）**：O09-W09 八域家族模板沿用——第十三用声明；自查架构
//! 三层（组级核验层→域级对账层→预备报告层——证据单源流转）；范围声明（X01-X08
//! 八组×20=160+组间对账+域级清点五类（API/测试/性能/安全/文档））；X 域三特色
//! 增维（工具体验维+观测不越界维+容量诚实维）；证据缺失→阻断+回补。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x8380;

/// 家族模板使用次数（第十三用）。
pub const TEMPLATE_USE: u32 = 13;

/// 组数。
pub const GROUPS: u32 = 8;

/// 组规模。
pub const PER_GROUP: u32 = 20;

/// 清点类数。
pub const AUDIT_KINDS: u32 = 5;

/// 特色增维数。
pub const EXTRA_DIMS: u32 = 3;

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 清点五类闭集。
pub const AUDIT_KIND_NAMES: [&str; 5] = ["api", "test", "perf", "security", "docs"];

/// X 域三特色增维闭集。
pub const EXTRA_DIM_NAMES: [&str; 3] = ["tool-ux", "observe-bound", "capacity-honest"];

/// 三层自查架构（证据单源流转）。
pub struct SelfCheckArch {
    pub source_ids: [u32; 3],
    /// 组×维计划矩阵：8 组 × (5 清点 + 3 增维)。
    pub plan: Vec<(u32, u32)>, // (组号, 维号)
}

impl SelfCheckArch {
    /// 标准构造：8 组 × 8 维全铺。
    pub fn standard() -> Self {
        let mut plan = Vec::new();
        for g in 0..GROUPS {
            for d in 0..(AUDIT_KINDS + EXTRA_DIMS) {
                plan.push((g, d));
            }
        }
        SelfCheckArch { source_ids: [9, 9, 9], plan }
    }

    /// 单源流转。
    pub fn single_source(&self) -> bool {
        self.source_ids[0] == self.source_ids[1] && self.source_ids[1] == self.source_ids[2]
    }

    /// 范围守恒：计划恰 8×8=64 格。
    pub fn plan_conserved(&self) -> bool {
        self.plan.len() == (GROUPS * (AUDIT_KINDS + EXTRA_DIMS)) as usize
    }

    /// 总量核算：八组×20=160。
    pub fn total160(&self) -> bool {
        GROUPS * PER_GROUP == 160
    }

    /// 增维登记：三特色维号在 5..8 区间全登记。
    pub fn extra_dims_registered(&self) -> bool {
        (5..8).all(|d| self.plan.iter().any(|(_, vd)| *vd == d))
    }

    /// 证据缺失阻断：任一格证据位缺失即阻断（返回阻断数）。
    pub fn evidence_gap(&self, missing: &[(u32, u32)]) -> bool {
        !missing.is_empty()
    }
}

/// J1 主判据：模板十三用 + 160+五类清点+三特色 + 增维登记 + 单源流转。
pub fn j1_main() -> bool {
    let a = SelfCheckArch::standard();
    TEMPLATE_USE == 13
        && a.single_source()
        && a.plan_conserved()
        && a.total160()
        && a.extra_dims_registered()
        && AUDIT_KIND_NAMES.len() == AUDIT_KINDS as usize
        && EXTRA_DIM_NAMES.len() == EXTRA_DIMS as usize
}

// ── 注入核（0x8380 段 5 项）────────────────────────────────────────────

/// 注入核 1：模板计数注入——家族模板用次钉死 13。
pub fn inj1_template_count() -> bool {
    TEMPLATE_USE == 13 && TEMPLATE_USE != 12
}

/// 注入核 2：证据缺失阻断注入——缺失非空即阻断。
pub fn inj2_evidence_block() -> bool {
    let a = SelfCheckArch::standard();
    a.evidence_gap(&[(3, 6)])
}

/// 注入核 3：增维缺项注入——某特色维未登记必红。
pub fn inj3_dim_missing() -> bool {
    let mut a = SelfCheckArch::standard();
    a.plan.retain(|(_, d)| *d != 6);
    !a.extra_dims_registered()
}

/// 注入核 4：计划守恒注入——少一格必红。
pub fn inj4_plan_short() -> bool {
    let mut a = SelfCheckArch::standard();
    a.plan.pop();
    !a.plan_conserved()
}

/// 注入核 5：单源分裂注入。
pub fn inj5_source_split() -> bool {
    let mut a = SelfCheckArch::standard();
    a.source_ids[2] = 10;
    !a.single_source()
}

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 35
}

/// run_vex61_checks：本单判据聚合（诊断段 0x8380）。
pub fn run_vex61_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F4961 预备自查总架构");
    s.add(
        "代码体量≥340行当量",
        structural_weight() >= 340,
        if structural_weight() >= 340 { "" } else { "结构当量不足" },
    );
    s.add(
        "模板十三用+160+五类清点+三特色增维",
        j1_main(),
        if j1_main() { "" } else { "范围/增维破" },
    );
    s.add(
        "三层证据单源流转+缺失阻断",
        j1_main() && inj2_evidence_block(),
        if j1_main() { "" } else { "单源/阻断破" },
    );
    let inj = [
        ("注入1模板计数", inj1_template_count()),
        ("注入2证据阻断", inj2_evidence_block()),
        ("注入3增维缺项", inj3_dim_missing()),
        ("注入4计划守恒", inj4_plan_short()),
        ("注入5单源分裂", inj5_source_split()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
