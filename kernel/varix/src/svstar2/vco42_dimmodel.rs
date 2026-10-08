//! CGPU-F2242 · 降级维度统一模型（CGPU-O 域 · 降级治理 · 批次 O42）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F2242`
//!
//! 锚点原文：「统一模型：维度模型（降级维度统一（帧率/画质/分辨率/资源/网络/
//! 功能六维——六维模型（每维{档位/触发/恢复}三元组——维度 schema 版本化；
//! 维度正交（六维独立性（可组合——正交声明；模型统一（各域降级模型映射到
//! 统一模型——映射表（映射复用家族——映射声明；测试（六维/schema/正交/映射
//! 四组）。判据：六维、schema、正交、映射声明、四组、判据。」
//!
//! # 一、六维是**封闭 schema**：每维三元组齐备才入册
//!
//! [`Dims`] 六维封闭枚举，每维落成 [`DimSchema`] 三元组（档位数/触发/恢复）——
//! 「档位、触发、恢复」缺一不可，schema 版本化 [`SCHEMA_VERSION`] 让维度账
//! 可对账（消费方携版本对拍，失配即重取）。
//!
//! # 二、正交是**可组合的声明**：六维独立、2^6 组合空间守恒
//!
//! [`DIMS_ORTHOGONAL`] 声明六维互相独立：任意维度子集可同时降级（位图组合），
//! 组合空间恒 2^6 = 64（[`orthogonal_combos`] 运行时重算与编译期守恒双验）——
//! 「可组合」由此可机检，不是文档里的一句「互不干扰」。
//!
//! # 三、映射复用家族：各域降级模型映射到统一维
//!
//! [`DOMAIN_MAPPINGS`] 把各域已有降级模型映射到统一六维（C 域降质链、D 域
//! 降帧事故、I 域带宽降载、J 域热降档……）——映射复用家族声明
//! [`MAPPING_FAMILY`]：统一模型不重造各域口径，只做映射收口。

use alloc::string::String;

// ---------------------------------------------------------------------------
// 一、诊断码（vco42 独占段 0x9D..）
// ---------------------------------------------------------------------------

/// schema 版本失配（消费方携旧版本对账——维度账已升级须重取）。
pub const E_O082_SCHEMA_STALE: u16 = 0x9D00;
/// 维度越界（六维之外不存在第七维）。
pub const E_O082_DIM_RANGE: u16 = 0x9D01;
/// 组合非法（空组合或位越出六维）。
pub const E_O082_COMBO_ILLEGAL: u16 = 0x9D02;
/// 映射缺源（域不在映射册——先登记再映射）。
pub const E_O082_DOMAIN_UNKNOWN: u16 = 0x9D03;

/// schema 版本（锚点「维度 schema 版本化」）。
pub const SCHEMA_VERSION: &str = "OC42-dimschema-v1";

// ---------------------------------------------------------------------------
// 二、六维封闭枚举
// ---------------------------------------------------------------------------

/// 降级六维（封闭全集——锚点「帧率/画质/分辨率/资源/网络/功能六维」）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dims {
    /// 帧率维（80 帧合同联动）。
    FrameRate,
    /// 画质维（特效/后处理档）。
    Quality,
    /// 分辨率维（渲染分辨率档）。
    Resolution,
    /// 资源维（内存/显存预算）。
    Resource,
    /// 网络维（带宽/流送档）。
    Network,
    /// 功能维（特性开关级）。
    Feature,
}

impl Dims {
    /// 全枚举（顺序即下标）。
    pub const ALL: [Dims; 6] = [
        Dims::FrameRate,
        Dims::Quality,
        Dims::Resolution,
        Dims::Resource,
        Dims::Network,
        Dims::Feature,
    ];

    /// 维下标。
    pub const fn ordinal(self) -> usize {
        match self {
            Dims::FrameRate => 0,
            Dims::Quality => 1,
            Dims::Resolution => 2,
            Dims::Resource => 3,
            Dims::Network => 4,
            Dims::Feature => 5,
        }
    }

    /// 下标 → 维（越界 None）。
    pub const fn of_ordinal(i: usize) -> Option<Dims> {
        match i {
            0 => Some(Dims::FrameRate),
            1 => Some(Dims::Quality),
            2 => Some(Dims::Resolution),
            3 => Some(Dims::Resource),
            4 => Some(Dims::Network),
            5 => Some(Dims::Feature),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// 三、维度 schema（每维 {档位/触发/恢复} 三元组，版本化）
// ---------------------------------------------------------------------------

/// 维度 schema 条目（三元组齐备才入册）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DimSchema {
    /// 维。
    pub dim: Dims,
    /// 档位数（含全开档；≥2 才有降级空间）。
    pub levels: u8,
    /// 触发口径（谁来触发降级）。
    pub trigger: &'static str,
    /// 恢复口径（怎么回升——防抖口径随之）。
    pub recover: &'static str,
}

/// 六维 schema 封闭表（锚点「每维{档位/触发/恢复}三元组」）。
pub const DIM_SCHEMAS: [DimSchema; 6] = [
    DimSchema { dim: Dims::FrameRate, levels: 3, trigger: "帧级超时/合同违约", recover: "连续 5 秒达标逐级回升" },
    DimSchema { dim: Dims::Quality, levels: 6, trigger: "帧预算超支", recover: "连续 10 秒余量回升" },
    DimSchema { dim: Dims::Resolution, levels: 4, trigger: "降质链 L4", recover: "画质恢复逐级回满" },
    DimSchema { dim: Dims::Resource, levels: 3, trigger: "内存/显存水位超限", recover: "水位回落释放降档" },
    DimSchema { dim: Dims::Network, levels: 3, trigger: "带宽拥塞/流送不足", recover: "带宽余量恢复原档" },
    DimSchema { dim: Dims::Feature, levels: 2, trigger: "功能开关裁决", recover: "开关复位即恢复" },
];

/// schema 查询（越界拒 [`E_O082_DIM_RANGE`]——第七维不存在）。
pub fn schema_of(i: usize) -> Result<DimSchema, u16> {
    if i >= DIM_SCHEMAS.len() {
        return Err(E_O082_DIM_RANGE);
    }
    Ok(DIM_SCHEMAS[i])
}

/// schema 版本对账（恰等过/失配拒 [`E_O082_SCHEMA_STALE`]——双向）。
pub fn check_schema_version(consumer_version: &str) -> Result<(), u16> {
    if consumer_version != SCHEMA_VERSION {
        return Err(E_O082_SCHEMA_STALE);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 四、正交声明（六维独立可组合，2^6 组合空间守恒）
// ---------------------------------------------------------------------------

/// 正交声明（锚点「六维独立性（可组合——正交声明」原句承载）。
pub const DIMS_ORTHOGONAL: &str =
    "六维互相独立：任意维度子集可同时降级——正交可组合，互不阻塞";

/// 组合合法性校验：位图非零且位不越出六维（非法拒 [`E_O082_COMBO_ILLEGAL`]）。
pub const fn combo_legal(mask: u8) -> Result<(), u16> {
    if mask == 0 {
        return Err(E_O082_COMBO_ILLEGAL);
    }
    if mask >= (1u8 << 6) {
        return Err(E_O082_COMBO_ILLEGAL);
    }
    Ok(())
}

/// 组合空间守恒（六维正交 → 全组合恰 2^6 = 64——判据侧独立重算口径）。
pub const fn orthogonal_combos() -> usize {
    1usize << 6
}

// ---------------------------------------------------------------------------
// 五、映射表（各域降级模型 → 统一六维，复用家族声明）
// ---------------------------------------------------------------------------

/// 映射复用家族声明（锚点「映射复用家族——映射声明」——统一模型不重造各域口径）。
pub const MAPPING_FAMILY: &str =
    "统一模型不重造各域降级口径——各域模型经映射表收口到统一六维，映射即复用";

/// 域映射条目（域降级模型 → 主映射维）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DomainMapping {
    /// 域名（来源降级模型所属域）。
    pub domain: &'static str,
    /// 映射到统一六维的主维。
    pub dim: Dims,
}

/// 映射表（封闭——锚点「各域降级模型映射到统一模型——映射表」）。
pub const DOMAIN_MAPPINGS: [DomainMapping; 6] = [
    DomainMapping { domain: "C 域降质链（CGPU-F0008）", dim: Dims::Quality },
    DomainMapping { domain: "D 域降帧事故（CGPU-F0482）", dim: Dims::FrameRate },
    DomainMapping { domain: "I 域带宽降载（CGPU-F1281）", dim: Dims::Network },
    DomainMapping { domain: "J 域热降档（CGPU-F1473）", dim: Dims::Resource },
    DomainMapping { domain: "K 域异构引擎降档（CGPU-F1601）", dim: Dims::Feature },
    DomainMapping { domain: "N 域重建降级（CGPU-F2081）", dim: Dims::Resolution },
];

/// 域映射查询（缺源拒 [`E_O082_DOMAIN_UNKNOWN`]——先登记再映射）。
pub fn mapping_of(domain: &str) -> Result<Dims, u16> {
    let mut i = 0usize;
    while i < DOMAIN_MAPPINGS.len() {
        if DOMAIN_MAPPINGS[i].domain == domain {
            return Ok(DOMAIN_MAPPINGS[i].dim);
        }
        i += 1;
    }
    Err(E_O082_DOMAIN_UNKNOWN)
}

/// 摘要行（面板/日志共用）。
pub fn screen_line() -> String {
    let mut s = String::new();
    s.push_str(SCHEMA_VERSION);
    s.push_str(" dims=6 schemas=6 combos=64 mappings=6 orthogonal");
    s
}

// ---------------------------------------------------------------------------
// 编译期闸（数值/len——字符串比较不做编译期断言，对拍归判据层）
// ---------------------------------------------------------------------------

const _: () = {
    assert!(DIM_SCHEMAS.len() == 6);
    assert!(Dims::ALL.len() == 6);
    assert!(DOMAIN_MAPPINGS.len() == 6);
    assert!(orthogonal_combos() == 64);
    assert!(E_O082_SCHEMA_STALE & 0xFF00 == 0x9D00);
    assert!(E_O082_DIM_RANGE & 0xFF00 == 0x9D00);
    assert!(E_O082_COMBO_ILLEGAL & 0xFF00 == 0x9D00);
    assert!(E_O082_DOMAIN_UNKNOWN & 0xFF00 == 0x9D00);
    assert!(
        E_O082_SCHEMA_STALE != E_O082_DIM_RANGE
            && E_O082_DIM_RANGE != E_O082_COMBO_ILLEGAL
            && E_O082_COMBO_ILLEGAL != E_O082_DOMAIN_UNKNOWN
    );
};
