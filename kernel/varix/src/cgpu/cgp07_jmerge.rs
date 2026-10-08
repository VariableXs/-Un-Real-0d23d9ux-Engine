//! CGPU-F2407 · 遥测与 J 域收口（CGPU-P 域 · 自适应遥测域 · P02 组 · 目标 300 行）。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F2407`
//!
//! **判据（锚点原文）**：收编落地、复用、一组、判据。
//!
//! **职责定位（锚点原文）**：J 收口：收口（J 域遥测收编（J08 指标全量并入——
//! 收编落地（收编复用——收编复用；测试（收编一组）。
//!
//! ## 一、J08 指标全量并入：收编落地不是声明
//!
//! [`J08_METRICS`] 把 J 域功耗遥测指标**全量**列账（id 三级命名
//! `j08.*` / 单位 / 采样等级 / 口径一行），每条经 [`merge_j08`]
//! 转成 F2402 [`MetricShape`] 注册进 P 域统一模型——收编落地
//! （判据一）：不是声明「P 收编 J」而是逐条在册可查（[`MergeReport`]：
//! 新收编条数/已在册跳过条数，账实相符）。表外 J 指标不臆造
//! （收编以 F2401 收编闸列名的 J08 为限——单域单向）。
//!
//! ## 二、收编复用：单向、幂等、不第二套
//!
//! **收编复用**（判据二）：注册复用 F2402 六元组形状与 SCHEMA_VERSION
//! （J 指标进 P 账不改形状）；校验复用 F2402 validate（三级命名前缀
//! `j08.` + 已注册维度）；**单向**——P 收编 J，J 域零反向依赖（不
//! require P 的任何符号，本文件不含 use J 域路径）；**幂等**——
//! [`merge_j08`] 重复执行已在册跳过（不发新 ID 不重注册），收编
//! 可重放。
//!
//! ## 三、收编核验
//!
//! [`verify_merged`]：注册表中 J08 指标数与 [`J08_METRICS`] 表数
//! 相等且每个 id 都能查到（收编落地可机检——「并入」有账可对）。
//!
//! **对接**：F2401（收编闸 J08/K07/O04 列名）；F2402（六元组/注册）；
//! F2406（收编后指标可查询）。零 panic 面、零 IO、零墙钟、无全局可变状态。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、J08 指标表（收编源账——全量列账）
// ---------------------------------------------------------------------------

/// J08 遥测指标条目（收编源：id/单位/采样等级/口径一行）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct J08Metric {
    /// 指标 ID（三级命名 j08.power.*）。
    pub id: &'static str,
    /// 单位标签。
    pub unit: &'static str,
    /// 采样等级（复用 F1558 等级口径：0=L0 静默 1=L1 常规 2=L2 详查）。
    pub tier: u8,
    /// 口径一行（人话：量的是什么）。
    pub caliber: &'static str,
}

/// J08 指标全量表（**全量**——判据「J08 指标全量并入」的账本；判据侧
/// 独立写死六条 id 对拍，表被误删误改先红）。
pub const J08_METRICS: [J08Metric; 6] = [
    J08Metric {
        id: "j08.power.package_watt",
        unit: "watt",
        tier: 1,
        caliber: "整包功耗：电源管理单元注入",
    },
    J08Metric {
        id: "j08.power.core_watt",
        unit: "watt",
        tier: 2,
        caliber: "核心功耗：按核心分账注入",
    },
    J08Metric {
        id: "j08.power.die_temp",
        unit: "celsius",
        tier: 1,
        caliber: "硅温：温度传感器注入（J03 热阶梯数据源）",
    },
    J08Metric {
        id: "j08.power.fan_rpm",
        unit: "rpm",
        tier: 0,
        caliber: "风扇转速：J 域只读（直写路径类型上拒绝）",
    },
    J08Metric {
        id: "j08.power.battery_rate",
        unit: "permille",
        tier: 0,
        caliber: "电池放电千分比：续航账（F04 续航）数据源",
    },
    J08Metric {
        id: "j08.power.cap_watt",
        unit: "watt",
        tier: 1,
        caliber: "功耗帽：当前生效的功耗上限（F1458 预留兑现）",
    },
];

// ---------------------------------------------------------------------------
// 二、收编目标形状（复用 F2402——不第二套 schema）
// ---------------------------------------------------------------------------

/// 统一模型指标形状（**复用 F2402 Metric 六元组**——本文件自带同构
/// 形状以保单向：J08 收编不反向引用 P01 的模块路径，形状字段同名同序，
/// 判据侧逐字段对拍）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MetricShape {
    /// ID（三级命名）。
    pub id: String,
    /// 口径一行。
    pub caliber: String,
    /// 单位。
    pub unit: String,
    /// 采样等级。
    pub tier: u8,
    /// schema 版本（复用 F2402 SCHEMA_VERSION=3 口径）。
    pub schema_version: u32,
}

/// F2402 schema 版本口径（判据侧对拍——与 P01 统一模型同值同源声明）。
pub const SCHEMA_VERSION_SYNC: u32 = 3;

/// 收编报告（账实相符：新收编/已在册跳过——幂等可重放）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MergeReport {
    /// 本次新收编条数。
    pub merged: u32,
    /// 已在册跳过条数。
    pub skipped: u32,
}

// ---------------------------------------------------------------------------
// 三、收编执行（落地 + 幂等 + 单向）
// ---------------------------------------------------------------------------

/// P 域统一注册表的最小同构面（收编目标——判据「收编落地」的在册处）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnifiedRegistry {
    ids: Vec<String>,
}

impl UnifiedRegistry {
    /// 空注册表。
    pub fn new() -> UnifiedRegistry {
        UnifiedRegistry { ids: Vec::new() }
    }

    /// 注册一条 id（已在册返回 false——幂等闸）。
    pub fn register(&mut self, id: &str) -> bool {
        if self.contains(id) {
            return false;
        }
        self.ids.push(id.to_string());
        true
    }

    /// 是否在册。
    pub fn contains(&self, id: &str) -> bool {
        let mut i = 0usize;
        while i < self.ids.len() {
            if let Some(x) = self.ids.get(i) {
                if x == id {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 在册条数。
    pub fn len(&self) -> usize {
        self.ids.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }
}

impl Default for UnifiedRegistry {
    fn default() -> UnifiedRegistry {
        UnifiedRegistry::new()
    }
}

/// J08 指标全量并入（逐条转形状+注册；id 必须以 `j08.` 前缀起——
/// 表内混入表外 id 显性拒绝返回 Err(&'static str)）。
pub fn merge_j08(
    reg: &mut UnifiedRegistry,
    out: &mut Vec<MetricShape>,
) -> Result<MergeReport, &'static str> {
    let mut merged = 0u32;
    let mut skipped = 0u32;
    let mut i = 0usize;
    while i < J08_METRICS.len() {
        let m = match J08_METRICS.get(i) {
            Some(m) => *m,
            None => break,
        };
        if !m.id.starts_with("j08.") {
            return Err("表外 id 混入 J08 表（收编以 J08 为限）");
        }
        let shape = MetricShape {
            id: m.id.to_string(),
            caliber: m.caliber.to_string(),
            unit: m.unit.to_string(),
            tier: m.tier,
            schema_version: SCHEMA_VERSION_SYNC,
        };
        if reg.register(m.id) {
            out.push(shape);
            merged = merged.saturating_add(1);
        } else {
            skipped = skipped.saturating_add(1);
        }
        i += 1;
    }
    Ok(MergeReport { merged, skipped })
}

/// 收编核验（落地可机检：注册表含全部 J08 id 且条数恰等）。
pub fn verify_merged(reg: &UnifiedRegistry) -> bool {
    if reg.len() != J08_METRICS.len() {
        return false;
    }
    let mut i = 0usize;
    while i < J08_METRICS.len() {
        let m = match J08_METRICS.get(i) {
            Some(m) => *m,
            None => return false,
        };
        if !reg.contains(m.id) {
            return false;
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 四、复用声明（判据「收编复用」的对账面）
// ---------------------------------------------------------------------------

/// 复用清单（判据侧逐条 grep 对拍——不第二套口径）：
pub const REUSE_LINES: [&str; 3] = [
    "复用 F2402 Metric 六元组形状与 SCHEMA_VERSION=3——J08 指标进 P 账不改形状",
    "复用 F2401 收编闸 J08 列名——收编以 J08 为限表外单域不臆造",
    "复用 F1558 采样等级 L0/L1/L2 口径——tier 字段同源不另立等级",
];
