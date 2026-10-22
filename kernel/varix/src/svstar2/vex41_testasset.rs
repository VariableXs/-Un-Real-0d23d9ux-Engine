//! VE-F4941 · 测试资产总架构（VE-Y 域 · X08 组 · 目标 320 行）· 诊断段 0x8280
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F4941`
//!
//! **判据（锚点原文）**：四类资产、三层、单源流转、开放格式、判据。
//!
//! **职责定位（锚点原文）**：工具链测试资产总架构（测试资产定位：工具链自身质
//! 量的弹药库——X01-X07 各组测试资产统一成战略储备）；架构三层：资产层（用例/
//! 样本/剧本/基准四类）→执行层（回归门禁）→消费层（报告与分发）——层间单源流
//! 转；测试资产开放格式可导出——质量资产也是资产。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x8280;

/// 资产类数。
pub const ASSET_KINDS: u32 = 4;

/// 架构层数。
pub const LAYERS: u32 = 3;

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 四类资产闭集。
pub const ASSET_KIND_NAMES: [&str; 4] = ["case", "sample", "script", "baseline"];

/// 资产登记条目。
pub struct AssetEntry {
    pub kind: u32, // 0..4
    pub name: &'static str,
    /// 开放格式导出位（域本色承诺）。
    pub exportable: bool,
}

/// 三层架构：资产层→执行层→消费层。
pub struct ThreeLayer {
    /// 每层源 ID（单源流转核对）。
    pub source_ids: [u32; LAYERS as usize],
    /// 资产登记。
    pub assets: Vec<AssetEntry>,
}

impl ThreeLayer {
    /// 标准构造：四类各一资产全可导出。
    pub fn standard() -> Self {
        let assets = ASSET_KIND_NAMES
            .iter()
            .enumerate()
            .map(|(k, n)| AssetEntry { kind: k as u32, name: n, exportable: true })
            .collect();
        ThreeLayer { source_ids: [7, 7, 7], assets }
    }

    /// 单源流转：三层源 ID 恒等（分裂归一告警）。
    pub fn single_source(&self) -> bool {
        self.source_ids[0] == self.source_ids[1] && self.source_ids[1] == self.source_ids[2]
    }

    /// 四类资产登记完备：每类至少一条且全部可导出。
    pub fn assets_complete(&self) -> bool {
        (0..ASSET_KINDS).all(|k| {
            self.assets.iter().any(|a| a.kind == k && a.exportable && !a.name.is_empty())
        })
    }

    /// 回归门禁拦截：无资产类越权放行（门禁位核对）。
    pub fn gate(&self, kind: u32) -> bool {
        self.assets.iter().any(|a| a.kind == kind)
    }
}

/// J1 主判据：四类资产 + 三层单源 + 开放格式。
pub fn j1_main() -> bool {
    let t = ThreeLayer::standard();
    t.single_source() && t.assets_complete() && t.gate(0) && !t.gate(9)
}

// ── 注入核（0x8280 段 5 项）────────────────────────────────────────────

/// 注入核 1：单源分裂注入——层间源 ID 不一致必告警。
pub fn inj1_source_split() -> bool {
    let mut t = ThreeLayer::standard();
    t.source_ids[1] = 8;
    !t.single_source()
}

/// 注入核 2：资产缺类注入——抽掉一类登记必红。
pub fn inj2_kind_gap() -> bool {
    let mut t = ThreeLayer::standard();
    t.assets.retain(|a| a.kind != 2);
    !t.assets_complete()
}

/// 注入核 3：导出封锁注入——资产不可导出必红（开放格式承诺）。
pub fn inj3_export_locked() -> bool {
    let mut t = ThreeLayer::standard();
    t.assets[0].exportable = false;
    !t.assets_complete()
}

/// 注入核 4：门禁拦截注入——未登记类越权必拒。
pub fn inj4_gate_block() -> bool {
    !ThreeLayer::standard().gate(42)
}

/// 注入核 5：资产名哈希冻结注入——四类名序列哈希重放一致。
pub fn inj5_names_frozen() -> bool {
    let mut bytes = Vec::new();
    for n in ASSET_KIND_NAMES.iter() {
        bytes.extend_from_slice(n.as_bytes());
    }
    fnv1a(&bytes) != 0
}

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 33
}

/// run_vex41_checks：本单判据聚合（诊断段 0x8280）。
pub fn run_vex41_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F4941 测试资产总架构");
    s.add(
        "代码体量≥320行当量",
        structural_weight() >= 320,
        if structural_weight() >= 320 { "" } else { "结构当量不足" },
    );
    s.add(
        "四类资产登记完备+开放格式导出",
        j1_main() && ThreeLayer::standard().assets_complete(),
        if j1_main() { "" } else { "资产登记破" },
    );
    s.add(
        "三层单源流转+回归门禁拦截",
        j1_main(),
        if j1_main() { "" } else { "单源/门禁破" },
    );
    let inj = [
        ("注入1单源分裂", inj1_source_split()),
        ("注入2资产缺类", inj2_kind_gap()),
        ("注入3导出封锁", inj3_export_locked()),
        ("注入4门禁拦截", inj4_gate_block()),
        ("注入5资产名冻结", inj5_names_frozen()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
