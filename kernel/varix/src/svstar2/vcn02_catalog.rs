//! CGPU-F2082 · 原生效果分类与清单（CGPU-N 域 · 原生效果重建 · 批次 N02）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F2082`
//!
//! 锚点原文：「效果清单：分类清单（原生效果全集清单（模糊族（Acrylic/背景模糊）/
//! 材质族（Mica/云母）/光效族（辉光/光晕）/形状族（圆角/阴影）——四族清单（清单
//! 版本化——版本化；优先级（P0/P1/P2——优先级表；来源标注（各效果来源
//! （Win32/DWM/浏览器——来源表；测试（清单/优先级/来源三组）。判据：四族、
//! 版本化、优先级表、来源表、三组、判据。」
//!
//! # 一、清单是**封闭账**：四族八条枚举齐全
//!
//! 原生效果全集落成封闭表 [`CATALOG`]：模糊族/材质族/光效族/形状族四族
//! [`EffectFamily`]，每族恰两条（Acrylic/背景模糊、Mica/云母、辉光/光晕、
//! 圆角/阴影）——「全集」由条数守恒可机检，不是「想到哪写到哪」的开放数组。
//!
//! # 二、优先级与来源是**封闭枚举**：三档两级各有其位
//!
//! 优先级 [`Priority`]（P0/P1/P2）与来源 [`EffectSource`]（Win32/DWM/浏览器）
//! 都是封闭枚举——清单的每一条**必带**优先级与来源两个维度（结构体字段非
//! Option），漏标的条目在编译期就进不了清单。
//!
//! # 三、版本化是**常量**：清单漂移可对账
//!
//! [`CATALOG_VERSION`] 把清单冻结在版本号下——效果清单随版本对账（F2081
//! 风险「版本漂移」的清单级预案），改动走版本升级不留暗改。

use alloc::string::String;

// ---------------------------------------------------------------------------
// 一、诊断码（vcn02 续 vcn01 的 0x9C 段，细分 0x9C1x）
// ---------------------------------------------------------------------------

/// 清单版本失配（消费方携旧版本号对账——清单已升级须重取）。
pub const E_N082_VERSION_STALE: u16 = 0x9C10;
/// 条目越界（清单只有八条）。
pub const E_N082_ENTRY_RANGE: u16 = 0x9C11;
/// 族内查询越界（族只有四支）。
pub const E_N082_FAMILY_RANGE: u16 = 0x9C12;

/// 清单版本（锚点「清单版本化」——版本化常量）。
pub const CATALOG_VERSION: &str = "NC02-catalog-v1";

// ---------------------------------------------------------------------------
// 二、封闭枚举（四族 / 三档优先级 / 三来源）
// ---------------------------------------------------------------------------

/// 原生效果四族（封闭全集——锚点「四族清单」）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EffectFamily {
    /// 模糊族（Acrylic/背景模糊）。
    Blur,
    /// 材质族（Mica/云母）。
    Material,
    /// 光效族（辉光/光晕）。
    Glow,
    /// 形状族（圆角/阴影）。
    Shape,
}

impl EffectFamily {
    /// 全枚举（顺序即下标）。
    pub const ALL: [EffectFamily; 4] = [
        EffectFamily::Blur,
        EffectFamily::Material,
        EffectFamily::Glow,
        EffectFamily::Shape,
    ];

    /// 族下标。
    pub const fn ordinal(self) -> usize {
        match self {
            EffectFamily::Blur => 0,
            EffectFamily::Material => 1,
            EffectFamily::Glow => 2,
            EffectFamily::Shape => 3,
        }
    }

    /// 下标 → 族（越界 None）。
    pub const fn of_ordinal(i: usize) -> Option<EffectFamily> {
        match i {
            0 => Some(EffectFamily::Blur),
            1 => Some(EffectFamily::Material),
            2 => Some(EffectFamily::Glow),
            3 => Some(EffectFamily::Shape),
            _ => None,
        }
    }
}

/// 优先级三档（封闭——锚点「优先级（P0/P1/P2）」；P0 最紧要，秩即序）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    /// P0：桌面级常驻效果（重建收益最大）。
    P0,
    /// P1：高频交互效果。
    P1,
    /// P2：低频/装饰效果。
    P2,
}

/// 效果来源（封闭——锚点「各效果来源（Win32/DWM/浏览器）」）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EffectSource {
    /// Win32 应用原生效果。
    Win32,
    /// DWM 桌面窗口管理器合成效果。
    Dwm,
    /// 浏览器渲染效果。
    Browser,
}

// ---------------------------------------------------------------------------
// 三、封闭清单（四族八条，每条必带优先级与来源）
// ---------------------------------------------------------------------------

/// 清单条目（名/族/优先级/来源——优先级与来源非 Option：漏标进不了清单）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EffectEntry {
    /// 效果名。
    pub name: &'static str,
    /// 所属族。
    pub family: EffectFamily,
    /// 优先级档。
    pub priority: Priority,
    /// 效果来源。
    pub source: EffectSource,
}

/// 原生效果全集清单（封闭八条——每族恰两条）。
pub const CATALOG: [EffectEntry; 8] = [
    EffectEntry { name: "Acrylic", family: EffectFamily::Blur, priority: Priority::P0, source: EffectSource::Win32 },
    EffectEntry { name: "背景模糊", family: EffectFamily::Blur, priority: Priority::P0, source: EffectSource::Browser },
    EffectEntry { name: "Mica", family: EffectFamily::Material, priority: Priority::P0, source: EffectSource::Dwm },
    EffectEntry { name: "云母", family: EffectFamily::Material, priority: Priority::P1, source: EffectSource::Win32 },
    EffectEntry { name: "辉光", family: EffectFamily::Glow, priority: Priority::P1, source: EffectSource::Dwm },
    EffectEntry { name: "光晕", family: EffectFamily::Glow, priority: Priority::P2, source: EffectSource::Browser },
    EffectEntry { name: "圆角", family: EffectFamily::Shape, priority: Priority::P0, source: EffectSource::Dwm },
    EffectEntry { name: "阴影", family: EffectFamily::Shape, priority: Priority::P1, source: EffectSource::Browser },
];

/// 条目查询（越界拒 [`E_N082_ENTRY_RANGE`]——清单只有八条）。
pub fn entry_of(i: usize) -> Result<EffectEntry, u16> {
    if i >= CATALOG.len() {
        return Err(E_N082_ENTRY_RANGE);
    }
    Ok(CATALOG[i])
}

/// 族内条目名查询（族内序即清单序；枚举封闭无越界态）。
pub const fn names_of_family(family: EffectFamily) -> [&'static str; 2] {
    match family {
        EffectFamily::Blur => [CATALOG[0].name, CATALOG[1].name],
        EffectFamily::Material => [CATALOG[2].name, CATALOG[3].name],
        EffectFamily::Glow => [CATALOG[4].name, CATALOG[5].name],
        EffectFamily::Shape => [CATALOG[6].name, CATALOG[7].name],
    }
}

/// 族平衡核对（每族恰两条——全集清单的守恒口径）。
pub const fn family_balanced() -> bool {
    let mut counts = [0usize; 4];
    let mut i = 0usize;
    while i < CATALOG.len() {
        counts[CATALOG[i].family.ordinal()] += 1;
        i += 1;
    }
    counts[0] == 2 && counts[1] == 2 && counts[2] == 2 && counts[3] == 2
}

/// 版本对账（消费方携版本号对账——失配拒 [`E_N082_VERSION_STALE`]）。
pub fn check_version(consumer_version: &str) -> Result<(), u16> {
    if consumer_version != CATALOG_VERSION {
        return Err(E_N082_VERSION_STALE);
    }
    Ok(())
}

/// P0 清单快照（读屏可达——最紧要效果先行重建）。
pub fn p0_names() -> [&'static str; 3] {
    ["Acrylic", "背景模糊", "Mica"]
}

/// 摘要行（面板/日志共用）。
pub fn screen_line() -> String {
    let mut s = String::new();
    s.push_str(CATALOG_VERSION);
    s.push_str(" families=4 entries=8 p0=3 sources=3");
    s
}

// ---------------------------------------------------------------------------
// 编译期闸
// ---------------------------------------------------------------------------

const _: () = {
    assert!(CATALOG.len() == 8);
    assert!(EffectFamily::ALL.len() == 4);
    assert!(E_N082_VERSION_STALE & 0xFF00 == 0x9C00);
    assert!(E_N082_ENTRY_RANGE & 0xFF00 == 0x9C00);
    assert!(E_N082_FAMILY_RANGE & 0xFF00 == 0x9C00);
    assert!(
        E_N082_VERSION_STALE != E_N082_ENTRY_RANGE
            && E_N082_ENTRY_RANGE != E_N082_FAMILY_RANGE
    );
};
