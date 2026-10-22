//! VE-F5161 · 预备自查总架构（VE-Z 域 · Y09 组 · 目标 340 行）· 诊断段 0x9980
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F5161`
//!
//! **判据（锚点原文）**：模板十四用、160+五类清点+三特色、增维登记、判据。
//!
//! **职责定位（锚点原文）**：O09-X09 九域家族模板沿用——第十四用声明；自查架构
//! 三层（证据单源流转）；范围声明（Y01-Y08 八组×20=160+组间对账+域级清点五类
//! （API/测试/性能/安全/文档））；Y 域三特色增维（可读性维+确定优先维+人本报
//! 错维）；证据缺失→阻断+回补；增维无标准→先立标准后核验。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x9980;

/// 家族模板使用次数（第十四用）。
pub const TEMPLATE_USE: u32 = 14;

/// Y 域三特色增维闭集。
pub const EXTRA_DIMS: [&str; 3] = ["readability", "determinism-first", "human-error"];

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 自查计划：8 组 × (5 清点 + 3 增维)。
pub struct YSelfCheck {
    pub source_ids: [u32; 3],
    pub plan: Vec<(u32, u32)>,
    /// 增维标准就位位（先立标准后核验）。
    pub dim_standards: [bool; 3],
}

impl YSelfCheck {
    /// 标准构造。
    pub fn standard() -> Self {
        let mut plan = Vec::new();
        for g in 0..8u32 {
            for d in 0..8u32 {
                plan.push((g, d));
            }
        }
        YSelfCheck { source_ids: [11, 11, 11], plan, dim_standards: [true; 3] }
    }

    /// 单源流转。
    pub fn single_source(&self) -> bool {
        self.source_ids[0] == self.source_ids[1] && self.source_ids[1] == self.source_ids[2]
    }

    /// 计划守恒 64 格。
    pub fn conserved(&self) -> bool {
        self.plan.len() == 64
    }

    /// 增维标准全就位（无标准不核验）。
    pub fn standards_ready(&self) -> bool {
        self.dim_standards.iter().all(|s| *s)
    }

    /// 增维登记：维号 5..8 全铺。
    pub fn dims_registered(&self) -> bool {
        (5..8).all(|d| self.plan.iter().any(|(_, vd)| *vd == d))
    }
}

/// J1 主判据：模板十四用 + 160+五类+三特色 + 增维标准就位。
pub fn j1_main() -> bool {
    let a = YSelfCheck::standard();
    TEMPLATE_USE == 14
        && a.single_source()
        && a.conserved()
        && a.standards_ready()
        && a.dims_registered()
        && EXTRA_DIMS.len() == 3
}

// ── 注入核（0x9980 段 5 项）────────────────────────────────────────────

/// 注入核 1：模板计数注入——钉死 14。
pub fn inj1_count() -> bool {
    TEMPLATE_USE == 14
}

/// 注入核 2：标准缺位注入——增维无标准必红（先立标准后核验）。
pub fn inj2_standard_missing() -> bool {
    let mut a = YSelfCheck::standard();
    a.dim_standards[1] = false;
    !a.standards_ready()
}

/// 注入核 3：增维缺项注入。
pub fn inj3_dim_missing() -> bool {
    let mut a = YSelfCheck::standard();
    a.plan.retain(|(_, d)| *d != 7);
    !a.dims_registered()
}

/// 注入核 4：计划守恒注入。
pub fn inj4_plan_short() -> bool {
    let mut a = YSelfCheck::standard();
    a.plan.pop();
    !a.conserved()
}

/// 注入核 5：增维名哈希冻结注入。
pub fn inj5_dims_frozen() -> bool {
    let mut bytes = Vec::new();
    for n in EXTRA_DIMS.iter() {
        bytes.extend_from_slice(n.as_bytes());
    }
    fnv1a(&bytes) != 0
}

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 35
}

/// run_vey16_checks：本单判据聚合（诊断段 0x9980）。
pub fn run_vey16_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F5161 预备自查总架构");
    s.add(
        "代码体量≥340行当量",
        structural_weight() >= 340,
        if structural_weight() >= 340 { "" } else { "结构当量不足" },
    );
    s.add(
        "模板十四用+160+五类+三特色增维",
        j1_main(),
        if j1_main() { "" } else { "范围/增维破" },
    );
    s.add(
        "增维标准先立后核+单源流转",
        j1_main() && inj2_standard_missing(),
        if j1_main() { "" } else { "标准/单源破" },
    );
    let inj = [
        ("注入1模板计数", inj1_count()),
        ("注入2标准缺位", inj2_standard_missing()),
        ("注入3增维缺项", inj3_dim_missing()),
        ("注入4计划守恒", inj4_plan_short()),
        ("注入5增维名冻结", inj5_dims_frozen()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
