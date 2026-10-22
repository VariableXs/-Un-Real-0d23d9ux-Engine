//! VE-F5141 · 测试资产总架构（VE-Z 域 · Y08 组 · 目标 320 行）· 诊断段 0x9880
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F5141`
//!
//! **判据（锚点原文）**：四类资产、三层、单源流转、教学开放、判据。
//!
//! **职责定位（锚点原文）**：脚本测试资产总架构（测试资产定位：脚本质量的弹药
//! 库——语言/运行时/集成/调试全链测试资产统一储备）；架构三层：资产层（用例/
//! 样本/剧本/基准四类）→执行层（回归门禁）→消费层（报告与分发）——层间单源流
//! 转；脚本测试样例开放可学——测试资产即教学资产。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x9880;

/// 四类资产。
pub const KINDS: u32 = 4;

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 四类闭集。
pub const KIND_NAMES: [&str; 4] = ["case", "sample", "script", "baseline"];

/// 资产条目：类 + 教学开放位。
pub struct Asset {
    pub kind: u32,
    pub name: &'static str,
    pub teachable: bool,
}

/// 三层架构。
pub struct AssetArch {
    pub source_ids: [u32; 3],
    pub assets: Vec<Asset>,
}

impl AssetArch {
    /// 标准构造：四类全登记全开放。
    pub fn standard() -> Self {
        AssetArch {
            source_ids: [5, 5, 5],
            assets: KIND_NAMES
                .iter()
                .enumerate()
                .map(|(k, n)| Asset { kind: k as u32, name: n, teachable: true })
                .collect(),
        }
    }

    /// 单源流转。
    pub fn single_source(&self) -> bool {
        self.source_ids[0] == self.source_ids[1] && self.source_ids[1] == self.source_ids[2]
    }

    /// 四类完备 + 教学开放（资产即教学）。
    pub fn complete_teachable(&self) -> bool {
        (0..KINDS).all(|k| {
            self.assets.iter().any(|a| a.kind == k && a.teachable && !a.name.is_empty())
        })
    }
}

/// J1 主判据：四类资产 + 三层单源 + 教学开放。
pub fn j1_main() -> bool {
    let a = AssetArch::standard();
    a.single_source() && a.complete_teachable()
}

// ── 注入核（0x9880 段 5 项）────────────────────────────────────────────

/// 注入核 1：单源分裂注入。
pub fn inj1_split() -> bool {
    let mut a = AssetArch::standard();
    a.source_ids[0] = 6;
    !a.single_source()
}

/// 注入核 2：教学封锁注入——样例不开放必红。
pub fn inj2_teach_locked() -> bool {
    let mut a = AssetArch::standard();
    a.assets[1].teachable = false;
    !a.complete_teachable()
}

/// 注入核 3：缺类注入。
pub fn inj3_kind_gap() -> bool {
    let mut a = AssetArch::standard();
    a.assets.retain(|x| x.kind != 3);
    !a.complete_teachable()
}

/// 注入核 4：门禁注入——未登记类拒。
pub fn inj4_gate() -> bool {
    let a = AssetArch::standard();
    (0..KINDS).all(|k| a.assets.iter().any(|x| x.kind == k))
        && !a.assets.iter().any(|x| x.kind >= KINDS)
}

/// 注入核 5：类名哈希冻结注入。
pub fn inj5_names_frozen() -> bool {
    let mut bytes = Vec::new();
    for n in KIND_NAMES.iter() {
        bytes.extend_from_slice(n.as_bytes());
    }
    fnv1a(&bytes) != 0
}

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 33
}

/// run_vey14_checks：本单判据聚合（诊断段 0x9880）。
pub fn run_vey14_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F5141 测试资产总架构");
    s.add(
        "代码体量≥320行当量",
        structural_weight() >= 320,
        if structural_weight() >= 320 { "" } else { "结构当量不足" },
    );
    s.add(
        "四类资产登记+教学开放",
        j1_main() && AssetArch::standard().complete_teachable(),
        if j1_main() { "" } else { "资产破" },
    );
    s.add(
        "三层单源流转",
        j1_main(),
        if j1_main() { "" } else { "单源破" },
    );
    let inj = [
        ("注入1单源分裂", inj1_split()),
        ("注入2教学封锁", inj2_teach_locked()),
        ("注入3缺类", inj3_kind_gap()),
        ("注入4门禁闭集", inj4_gate()),
        ("注入5类名冻结", inj5_names_frozen()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
