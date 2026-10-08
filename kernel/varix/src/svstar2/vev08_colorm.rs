//! VE-F4408 · 色彩与 M 域深化（VE-V 域 · 显示色彩 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F4408`
//!
//! 锚点原文：「职责定位：色彩与 M 域深化——与 M 域（媒体管线）色彩衔接（解码
//! 色彩元数据透传：容器/流级色彩信息不丢失直达呈现；渲染目标色彩空间协商：
//! 应用声明意图-引擎裁决-回执三步；双重变换防护：全链单次变换保证）。数据
//! 结构：透传通道；协商协议（三步）；单次变换断言。错误路径与降级矩阵：元数据
//! 丢失→缺省标注不臆造；协商失败→回退 SDR 路径；双重变换检出→阻断修正。
//! 性能逐项分解：透传 O(1)；协商 O(1)；断言 O(链长)。判据：元数据透传、
//! 三步协商、单次变换、SDR 回退、判据。」
//!
//! # 一、元数据透传是**通道**不是复制粘贴：丢失就标注缺省，不臆造
//!
//! [`ColorMeta`] 承载容器/流级色彩信息（原色/传递函数/矩阵/范围），透传
//! [`passthrough`] O(1) 直达呈现——元数据**丢失**时产缺省标注
//! [`passthrough`] 返回 `is_default = true` 的 SDR 缺省（不臆造来源信息），
//! 「缺省」是显性状态可被下游识别。
//!
//! # 二、协商是**三步协议**：声明-裁决-回执，每步有据
//!
//! [`negotiate`] 三步：应用声明意图（[`Intent`]）→ 引擎按显示能力裁决
//! → 回执（[`Receipt`] 携裁决结果与理由）。协商失败（意图超出能力）→
//! **回退 SDR 路径**并回执说明——失败有回执，不是静默改道。
//!
//! # 三、双重变换是**事故**：全链单次变换保证
//!
//! [`assert_single_transform`] O(链长) 扫全链色彩变换级——检出两级及以上
//! 色彩空间变换即阻断（双重变换=色彩被错误地变换两次，输出必然失真），
//! 阻断后修正再过，不静默放行。

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、色彩元数据与透传通道
// ---------------------------------------------------------------------------

/// 色彩元数据（容器/流级——透传通道的载荷）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColorMeta {
    /// 原色域（0=未知/BT709/BT2020/DCI-P3）。
    pub primaries: u8,
    /// 传递函数（0=未知/Gamma2.2/PQ/HLG）。
    pub transfer: u8,
    /// 矩阵系数（0=未知/BT601/BT709/BT2020）。
    pub matrix: u8,
    /// 色域范围（0=未知/Limited/Full）。
    pub range: u8,
}

/// 透传回执（元数据 + 是否缺省标注）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PassthroughReceipt {
    /// 到达呈现端的元数据（缺省时为 SDR 全缺省值）。
    pub meta: ColorMeta,
    /// 是否缺省标注（true = 原元数据丢失，SDR 缺省不臆造）。
    pub is_default: bool,
}

/// SDR 缺省元数据（丢失时的缺省标注值——显性缺省，不臆造来源）。
pub const SDR_DEFAULT: ColorMeta = ColorMeta { primaries: 1, transfer: 1, matrix: 1, range: 2 };

/// **passthrough**（透传通道，O(1)）：元数据在则原样直达，丢失则产缺省标注。
pub const fn passthrough(meta: Option<ColorMeta>) -> PassthroughReceipt {
    match meta {
        Some(m) => PassthroughReceipt { meta: m, is_default: false },
        None => PassthroughReceipt { meta: SDR_DEFAULT, is_default: true },
    }
}

// ---------------------------------------------------------------------------
// 二、三步协商协议（声明-裁决-回执）
// ---------------------------------------------------------------------------

/// 渲染目标色彩空间（封闭三态）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorSpace {
    /// SDR（BT709/Gamma2.2）。
    Sdr,
    /// HDR10（BT2020/PQ）。
    Hdr10,
    /// HLG（BT2020/HLG）。
    Hlg,
}

/// 应用声明意图（协商第一步输入）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Intent {
    /// 意图目标色彩空间。
    pub target: ColorSpace,
    /// 是否接受回退（false = 不接受回退则失败显性）。
    pub allow_fallback: bool,
}

/// 显示能力（裁决依据——引擎按此判意图可达性）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DisplayCaps {
    /// 支持 SDR（基线恒真——SDR 是回退底座）。
    pub sdr: bool,
    /// 支持 HDR10。
    pub hdr10: bool,
    /// 支持 HLG。
    pub hlg: bool,
}

/// 协商回执（第三步输出——结果与理由同达）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Receipt {
    /// 裁决生效的色彩空间。
    pub space: ColorSpace,
    /// 是否走了 SDR 回退。
    pub fell_back_sdr: bool,
    /// 理由（裁决依据可读——回执不是黑盒）。
    pub reason: &'static str,
}

/// **negotiate**（三步协商协议，O(1)）：声明→裁决→回执。
///
/// 意图可达 → 原样回执；不可达且接受回退 → **回退 SDR** 并回执说明；
/// 不可达且不接受回退 → 显性失败（回执置 SDR 且 `fell_back_sdr = true`
/// 不可用——这里以 `None` 表示协商失败，调用方按降级矩阵阻断）。
pub const fn negotiate(intent: Intent, caps: DisplayCaps) -> Option<Receipt> {
    let reachable = match intent.target {
        ColorSpace::Sdr => caps.sdr,
        ColorSpace::Hdr10 => caps.hdr10,
        ColorSpace::Hlg => caps.hlg,
    };
    if reachable {
        return Some(Receipt {
            space: intent.target,
            fell_back_sdr: false,
            reason: "意图可达——按声明裁决",
        });
    }
    if intent.allow_fallback {
        return Some(Receipt {
            space: ColorSpace::Sdr,
            fell_back_sdr: true,
            reason: "意图超出能力——回退 SDR 路径",
        });
    }
    None
}

// ---------------------------------------------------------------------------
// 三、单次变换断言（全链 O(链长)）
// ---------------------------------------------------------------------------

/// 全链阶段（色彩管线的一级）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stage {
    /// 阶段名。
    pub name: &'static str,
    /// 本级是否做色彩空间变换。
    pub converts: bool,
}

/// 双重变换判定结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChainAudit {
    /// 全链零或一次变换——单次变换保证成立。
    SingleTransform,
    /// 双重变换检出（计数 = 变换级数）。
    DoubleTransform(usize),
}

/// **assert_single_transform**（O(链长)）：扫全链统计色彩变换级数。
///
/// ≤1 级 → [`ChainAudit::SingleTransform`]；≥2 级 → 双重变换检出，
/// 调用方必须**阻断修正**（不静默放行——双重变换输出必然失真）。
pub const fn assert_single_transform(chain: &[Stage]) -> ChainAudit {
    let mut converts = 0usize;
    let mut i = 0usize;
    while i < chain.len() {
        if chain[i].converts {
            converts += 1;
        }
        i += 1;
    }
    if converts <= 1 {
        ChainAudit::SingleTransform
    } else {
        ChainAudit::DoubleTransform(converts)
    }
}

/// 摘要行（面板/日志共用——协商结果读屏可查，域本色）。
pub fn screen_line() -> String {
    let mut s = String::new();
    s.push_str("VEV08-colorm passthrough=O(1) negotiate=3-step audit=O(chain)");
    s
}
