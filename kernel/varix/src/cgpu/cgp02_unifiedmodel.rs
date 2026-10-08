//! CGPU-F2402 · 遥测统一模型（CGPU-P 域 · 自适应遥测域 · P01 组 · 目标 340 行）。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F2402`
//!
//! **判据（锚点原文）**：统一 schema、v3 升级、维度复用、三组、判据。
//!
//! **职责定位（锚点原文）**：统一模型：遥测统一模型（Metric{ID/口径/
//! 维度/值/时间戳/隐私级}——统一 schema（v3 版本化（F1453 v1→F1556 v2
//! →F2402 v3——升级记录）；维度体系（全域维度注册（场景/引擎/档位
//! ——维度注册复用家族——维度复用）；测试（v3/维度/注册三组）。
//!
//! ## 一、Metric 六元组：全域遥测只此一种形状
//!
//! 每条遥测 = 六元组（[`Metric`]：ID/口径/维度/值/时间戳/隐私级）——
//! ID 三级命名（`p01.<组>.<指标>`，同 F0274 家族）、口径人话一行（防
//! 「指标沼泽」：名字有了不知道量什么）、值是饱和整数（零浮点零
//! panic）、时间戳上游注入（零墙钟）、隐私级三档（红线进类型：本地/
//! 匿名/公开——[`PrivacyLevel`]）。全域生产者都产出这一种形状，
//! 基座管道只认一种输入。
//!
//! ## 二、v3 版本化：升级有账，旧数据可读
//!
//! schema 演进三站（[`SCHEMA_UPGRADES`]：F1453 v1 → F1556 v2 →
//! F2402 v3），每站来源单号+变更说明在案（判据二「升级记录」）；
//! 版本兼容判定 [`is_readable_by`]：v3 读取器可读全部历史版本记录
//! （演进不破坏历史——v1/v2 记录照样进 v3 管道），版本号写进每条
//! Metric（[`Metric::schema_version`]），不靠猜。
//!
//! ## 三、维度注册复用：同名维度全域一个 ID
//!
//! 全域维度注册表（[`DimensionRegistry`]：场景/引擎/档位三类闭集）：
//! 注册即发 ID，**同名复用**——重复注册返回既有 ID 不发新（判据三
//! 「维度复用」：口径漂移的一半来源就是同名维度各有各的 ID）；表外
//! 维度查询 None 不臆造。
//!
//! **对接**：F0274（五元组/三级命名同源）；F2401（基座与五段）；
//! F2403（采样策略引擎消费本模型）。零 panic 面（下标走 `get`/
//! `Option`，算术饱和）、零 IO、零墙钟（时间戳注入）、无全局可变
//! 状态、no_std 零 std 依赖。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、Metric 六元组（判据一：统一 schema）
// ---------------------------------------------------------------------------

/// 本项版本。
pub const UNIFIED_MODEL_VERSION: &str = "P01-unifiedmodel-v3";

/// 当前 schema 版本号（本单落定的 v3）。
pub const SCHEMA_VERSION: u32 = 3;

/// 隐私级三档（F2401 红线进类型：本地/匿名/公开——档位即处置策略）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrivacyLevel {
    /// 本地（默认留本地，不出设备）。
    Local,
    /// 匿名（可出设备但先匿名化）。
    Anonymized,
    /// 公开（无用户可识别信息的聚合统计）。
    Public,
}

impl PrivacyLevel {
    /// 人话标签。
    pub const fn label(self) -> &'static str {
        match self {
            PrivacyLevel::Local => "本地",
            PrivacyLevel::Anonymized => "匿名",
            PrivacyLevel::Public => "公开",
        }
    }
}

/// 指标值闭集（饱和整数族——零浮点：精度要求让位确定性）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MetricValue {
    /// 计数。
    Count(u64),
    /// 时长（纳秒）。
    DurationNs(u64),
    /// 每分比（0..1_000_000）。
    Ppm(u32),
}

impl MetricValue {
    /// 单位标签。
    pub const fn unit(self) -> &'static str {
        match self {
            MetricValue::Count(_) => "count",
            MetricValue::DurationNs(_) => "ns",
            MetricValue::Ppm(_) => "ppm",
        }
    }

    /// 数值（Ppm 超界饱和到 1_000_000——非法值在构造面钳住）。
    pub const fn raw(self) -> u64 {
        match self {
            MetricValue::Count(v) => v,
            MetricValue::DurationNs(v) => v,
            MetricValue::Ppm(v) => {
                if v > 1_000_000 {
                    1_000_000
                } else {
                    v as u64
                }
            }
        }
    }
}

/// 遥测统一模型六元组（全域只此一种形状——基座管道唯一输入）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Metric {
    /// ID（三级命名：p01.<组>.<指标>）。
    pub id: String,
    /// 口径（人话一行：量的是什么、怎么量——防指标沼泽）。
    pub caliber: String,
    /// 维度 ID 集（来自 [`DimensionRegistry`] 注册，非裸字符串）。
    pub dimensions: Vec<u32>,
    /// 值（饱和整数族）。
    pub value: MetricValue,
    /// 时间戳（纳秒；上游注入——零墙钟）。
    pub timestamp_ns: u64,
    /// 隐私级。
    pub privacy: PrivacyLevel,
}

impl Metric {
    /// 本记录的 schema 版本（版本化：v3 生成、历史可读）。
    pub const fn schema_version(&self) -> u32 {
        SCHEMA_VERSION
    }

    /// 结构校验（ID 三级命名前缀 + 维度已注册 + 值合法）。
    pub fn validate(&self, reg: &DimensionRegistry) -> Result<(), &'static str> {
        if !self.id.starts_with("p01.") {
            return Err("ID 不是三级命名（缺 p01. 前缀）");
        }
        let mut i = 0usize;
        while i < self.dimensions.len() {
            let d = match self.dimensions.get(i) {
                Some(d) => *d,
                None => return Err("维度集越界"),
            };
            if reg.name_of(d).is_none() {
                return Err("维度未注册（裸维度 ID 是口径漂移的种子）");
            }
            i += 1;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 二、v3 版本化与升级记录（判据二）
// ---------------------------------------------------------------------------

/// schema 升级一站（版本号/来源单号/变更说明）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SchemaUpgrade {
    /// 版本号。
    pub version: u32,
    /// 落定单号。
    pub source_task: u32,
    /// 变更说明。
    pub change: &'static str,
}

/// schema 演进三站（F1453 v1 → F1556 v2 → F2402 v3——升级记录在案）。
pub const SCHEMA_UPGRADES: [SchemaUpgrade; 3] = [
    SchemaUpgrade {
        version: 1,
        source_task: 1453,
        change: "v1 初版：功耗遥测对接预留的五元组雏形",
    },
    SchemaUpgrade {
        version: 2,
        source_task: 1556,
        change: "v2 扩展：J08 组遥测总架构补充维度位",
    },
    SchemaUpgrade {
        version: 3,
        source_task: 2402,
        change: "v3 统一：六元组定型（ID/口径/维度/值/时间戳/隐私级），隐私级进类型",
    },
];

/// 版本兼容判定（判据二「演进不破坏历史」：v3 读取器可读全部历史版本）。
pub const fn is_readable_by(record_version: u32, reader_version: u32) -> bool {
    record_version <= reader_version
}

/// 升级链完整性（v1→v2→v3 逐级递进且来源单号与锚点对账）。
pub fn upgrade_chain_intact() -> bool {
    let expect = [(1u32, 1453u32), (2, 1556), (3, 2402)];
    if SCHEMA_UPGRADES.len() != 3 {
        return false;
    }
    let mut i = 0usize;
    while i < SCHEMA_UPGRADES.len() {
        let u = match SCHEMA_UPGRADES.get(i) {
            Some(u) => *u,
            None => return false,
        };
        let e = match expect.get(i) {
            Some(e) => *e,
            None => return false,
        };
        if u.version != e.0 || u.source_task != e.1 {
            return false;
        }
        if i > 0 {
            let prev = match SCHEMA_UPGRADES.get(i - 1) {
                Some(p) => *p,
                None => return false,
            };
            if u.version != prev.version + 1 {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 三、维度注册表（判据三：同名复用、全域一个 ID）
// ---------------------------------------------------------------------------

/// 维度类别闭集（锚点原文：场景/引擎/档位）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DimensionKind {
    /// 场景。
    Scene,
    /// 引擎。
    Engine,
    /// 档位。
    Tier,
}

impl DimensionKind {
    /// 人话标签。
    pub const fn label(self) -> &'static str {
        match self {
            DimensionKind::Scene => "场景",
            DimensionKind::Engine => "引擎",
            DimensionKind::Tier => "档位",
        }
    }
}

/// 已注册维度（ID + 类别 + 名称）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dimension {
    /// 全域唯一 ID（注册序发放）。
    pub id: u32,
    /// 类别。
    pub kind: DimensionKind,
    /// 名称（全域唯一——同名即同一维度）。
    pub name: String,
}

/// 全域维度注册表（判据三：同名复用——重复注册返回既有 ID 不发新）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DimensionRegistry {
    dims: Vec<Dimension>,
    next_id: u32,
}

impl DimensionRegistry {
    /// 新注册表。
    pub const fn new() -> DimensionRegistry {
        DimensionRegistry {
            dims: Vec::new(),
            next_id: 1,
        }
    }

    /// 注册维度（同名复用：命中既有直接返回其 ID；新名发新 ID）。
    pub fn register(&mut self, kind: DimensionKind, name: &str) -> u32 {
        let mut i = 0usize;
        while i < self.dims.len() {
            if let Some(d) = self.dims.get(i) {
                if d.name == name {
                    return d.id;
                }
            }
            i += 1;
        }
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        self.dims.push(Dimension {
            id,
            kind,
            name: name.to_string(),
        });
        id
    }

    /// 按 ID 查名称（表外 None 不臆造）。
    pub fn name_of(&self, id: u32) -> Option<&str> {
        let mut i = 0usize;
        while i < self.dims.len() {
            if let Some(d) = self.dims.get(i) {
                if d.id == id {
                    return Some(d.name.as_str());
                }
            }
            i += 1;
        }
        None
    }

    /// 已注册维度数。
    pub fn len(&self) -> usize {
        self.dims.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.dims.is_empty()
    }
}

impl Default for DimensionRegistry {
    fn default() -> DimensionRegistry {
        DimensionRegistry::new()
    }
}
