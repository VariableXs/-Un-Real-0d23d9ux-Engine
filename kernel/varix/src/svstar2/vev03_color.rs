//! VE-F4403 · 色彩管理引擎（VE-W 域 · 显示与色彩组 · 目标 380 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F4403`
//!
//! **判据（锚点原文）**：四模块、按需激活、缺省标注、意图仲裁、判据。
//!
//! **职责定位（锚点原文）**：色彩管理引擎总成——**四模块**：显示器配置解析 /
//! 色彩变换调度 / 变换缓存 / 异常回退——**按需激活**：无校准数据时走缺省
//! sRGB 路径**并标注不静默**；引擎与设备层解耦声明。
//!
//! # 一、为什么是四模块而不是一个「色彩开关」
//!
//! 色彩管理失败的四种姿态需要四种独立的处置：配置坏了（解析）、两处要用
//! 不同的变换（调度）、同一变换反复用（缓存）、变换本身炸了（回退）。
//! 揉成一个模块，任何一处异常都会把「降级」和「失灵」混在一起——回退到
//! sRGB 是**合法降级**，缓存击穿是**性能缺陷**，两者共用一个入口就分不清
//! 「画面偏色是故意的还是坏了」。四模块各自可独立断言（判据「四模块」的
//! 本意），故障归因才有可能。
//!
//! # 二、缺省 sRGB 为什么必须「标注不静默」
//!
//! 无校准数据走 sRGB 是正确的降级；但**静默**走 sRGB 是事故——广色域屏
//! 显得灰暗、HDR 内容被压制，用户看到的是「颜色不对」却无处可查。
//! 故缺省路径必带 [`DefaultNote`]（原因/时刻/影响），进激活状态表可查询、
//! 可读屏（锚点「色彩状态对读屏可查」）。标注不是日志，是**契约输出**：
//! 消费方（设置界面/诊断面板）凭它区分「已校准」与「降级中」。
//!
//! # 三、调度冲突为什么按「应用意图」仲裁
//!
//! 两应用对同一显示器声明不同渲染意图（一个要色准、一个要鲜艳）时，
//! 引擎必须二选一。仲裁规则表（[`INTENT_PRIORITY`]）钉死优先级：
//! 色度精确 > 感知 > 饱和度——色度精确是唯一「不改变颜色值」的意图，
//! 作为公共路径最不会背叛任一方；显式声明晚到者**不**抢 先占用
//! （抢占用会让先到应用画面突然偏色，比共享次优意图更糟）。
//! 规则是单源常量表，判据钉死；ICC 全意图集的深化归 F4406（前向声明）。
//!
//! # 四、缓存为什么用版本戳失效而不是 LRU
//!
//! 变换的输入是「profile 对 + 意图」，profile 一旦重校准（版本戳递增），
//! 旧变换的输出就是**系统性错误**而非「放久了的旧数据」——LRU 会把
//! 失效变换留在缓存里（只要它还热）。版本戳失配即整槽作废（[`TransformCache::invalidate_version`]），
//! 语义是「这张表只对这版 profile 有效」。容量定容 64 槽（FNV 直接定槽），
//! O(1) 命中无分配（锚点性能口径）。
//!
//! # 五、与设备层的解耦（锚点「引擎与设备层解耦声明」）
//!
//! 本引擎**不持有设备句柄**：上游消费 F4402 [`crate::svstar2::vev02_monitor`]
//! 的能力快照（显示器以身份键为地址），下游变换数学归 F4406、同步归
//! F4407（均前向声明）。设备热插拔带来的只有「快照变了」，引擎重算
//! 激活表即可——设备层崩了引擎不该陪葬，反之亦然。
//!
//! **性能（锚点原文）**：解析 O(配置)（一次性）；调度 O(1)（查表）；
//! 缓存 O(1) 命中（FNV 定槽）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::svstar2::vev02_monitor::{CapabilityRecord, HdrCapability};

// ---------------------------------------------------------------------------
// 一、错误码（P/W 域字符串码家族格式）
// ---------------------------------------------------------------------------

/// 本项版本。
pub const COLOR_ENGINE_VERSION: &str = "V03-color-v1";

/// 配置解析失败（已降级缺省 + 标注，不静默）。
pub const E_COLOR_CONFIG: &str = "E_COLOR_CONFIG";
/// 无校准数据（缺省 sRGB 路径 + 标注）。
pub const E_COLOR_UNCALIBRATED: &str = "E_COLOR_UNCALIBRATED";
/// 调度输入非法（未知 profile/意图）。
pub const E_COLOR_SCHEDULE: &str = "E_COLOR_SCHEDULE";
/// 变换执行失败（回退缺省 + 立案）。
pub const E_COLOR_TRANSFORM: &str = "E_COLOR_TRANSFORM";

// ---------------------------------------------------------------------------
// 二、profile 与意图（封闭枚举，F4406 深化前的前向契约位）
// ---------------------------------------------------------------------------

/// 色彩 profile 类别（封闭全集；Custom 必须带已解析参数才算数）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfileKind {
    /// 缺省 sRGB（无校准/回退路径）。
    Srgb,
    /// Display-P3（广色域屏常见）。
    DisplayP3,
    /// Adobe RGB（摄影向）。
    AdobeRgb,
    /// 自定义（校准数据解析成功时）。
    Custom,
}

/// profile 闭集长度。
pub const PROFILE_KIND_COUNT: usize = 4;

const PROFILE_KINDS: [ProfileKind; PROFILE_KIND_COUNT] = [
    ProfileKind::Srgb,
    ProfileKind::DisplayP3,
    ProfileKind::AdobeRgb,
    ProfileKind::Custom,
];

impl ProfileKind {
    /// 全集。
    pub fn all() -> [ProfileKind; PROFILE_KIND_COUNT] {
        PROFILE_KINDS
    }

    /// 短码。
    pub fn wire(self) -> &'static str {
        match self {
            ProfileKind::Srgb => "srgb",
            ProfileKind::DisplayP3 => "p3",
            ProfileKind::AdobeRgb => "adobe",
            ProfileKind::Custom => "custom",
        }
    }

    /// 由短码还原（未知拒）。
    pub fn parse(s: &str) -> Option<ProfileKind> {
        Self::all().iter().copied().find(|k| k.wire() == s)
    }

    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            ProfileKind::Srgb => "sRGB 缺省",
            ProfileKind::DisplayP3 => "Display-P3",
            ProfileKind::AdobeRgb => "Adobe RGB",
            ProfileKind::Custom => "自定义校准",
        }
    }
}

/// 渲染意图（ICC 三意图；深化归 F4406）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum RenderIntent {
    /// 色度精确（不改颜色值——公共路径最不背叛任一方）。
    Colorimetric,
    /// 感知（整体观感均衡）。
    Perceptual,
    /// 饱和度（鲜艳优先）。
    Saturation,
}

impl RenderIntent {
    /// 短码。
    pub fn wire(self) -> &'static str {
        match self {
            RenderIntent::Colorimetric => "colorimetric",
            RenderIntent::Perceptual => "perceptual",
            RenderIntent::Saturation => "saturation",
        }
    }
}

/// 意图优先级表（**单源**：冲突仲裁按此序，越靠前越优先占用公共路径）。
pub const INTENT_PRIORITY: [RenderIntent; 3] = [
    RenderIntent::Colorimetric,
    RenderIntent::Perceptual,
    RenderIntent::Saturation,
];

/// 意图仲裁（锚点「调度冲突→按应用意图仲裁」）。
///
/// 多应用同显示器不同意图时，取 [`INTENT_PRIORITY`] 中**最高优先级**者
/// 作为公共变换意图。规则：色度精确不改颜色值，共享它不会背叛低优先方；
/// 反之若按「先到先得」占用，先到的鲜艳意图会把后到的色准应用拖偏——
/// 画面突变比「大家共享次优」更难排查。
pub fn arbitrate_intent(requested: &[RenderIntent]) -> Option<RenderIntent> {
    if requested.is_empty() {
        return None;
    }
    for p in INTENT_PRIORITY.iter() {
        if requested.iter().any(|r| r == p) {
            return Some(*p);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// 三、模块一：显示器配置解析（O(配置)，一次性）
// ---------------------------------------------------------------------------

/// 解析出的 profile（kind + 版本戳；变换数学归 F4406，这里只管「是哪张」）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedProfile {
    /// 类别。
    pub kind: ProfileKind,
    /// profile 版本戳（重校准递增——缓存失效依据）。
    pub version: u32,
    /// 身份键（来自 F4402 指纹；None = 未绑定具体显示器）。
    pub identity: Option<String>,
}

/// 校准数据载荷（引擎不解释设备私有格式，只认「自声明的 kind 行」）。
///
/// 解析规则（O(配置) 单遍）：首行 `kind=<wire>` 命中四类之一即为有效校准；
/// 其余行忽略。解析失败 = 首行缺失/kind 未知——**降级缺省 + 标注**，
/// 不抛给上层一个「半解析对象」。
pub fn parse_config(data: &str, identity: Option<String>) -> Result<ParsedProfile, ()> {
    let first = data.lines().next().unwrap_or("");
    let kind = first.strip_prefix("kind=").and_then(ProfileKind::parse);
    match kind {
        Some(k) if k != ProfileKind::Srgb => Ok(ParsedProfile { kind: k, version: 1, identity }),
        // sRGB 显式声明与「无校准」同走缺省路径：它没有需要调度的差异变换。
        _ => Err(()),
    }
}

// ---------------------------------------------------------------------------
// 四、模块二：色彩变换调度（O(1) 查表）
// ---------------------------------------------------------------------------

/// 调度结果（变换链的**形状声明**——执行归 F4406）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TransformPlan {
    /// 源 profile。
    pub from: ProfileKind,
    /// 目标 profile。
    pub to: ProfileKind,
    /// 仲裁后的意图。
    pub intent: RenderIntent,
    /// 是否直通（源==目标或源/目标均为 sRGB——无需变换）。
    pub passthrough: bool,
}

/// 调度（O(1)：无循环，纯比较）。源/目标同型或都为 sRGB ⇒ 直通。
pub fn schedule(from: ProfileKind, to: ProfileKind, intent: RenderIntent) -> TransformPlan {
    let passthrough = from == to || (from == ProfileKind::Srgb && to == ProfileKind::Srgb);
    TransformPlan { from, to, intent, passthrough }
}

// ---------------------------------------------------------------------------
// 五、模块三：变换缓存（FNV 定槽，O(1) 命中，版本戳失效）
// ---------------------------------------------------------------------------

/// 缓存容量（定容；超出后新条目替换同槽旧条目——冲突槽即淘汰位）。
pub const CACHE_CAPACITY: usize = 64;

/// FNV-1a 定槽（O(1)，无分配）。
fn slot_of(key: (u64, u64, u8)) -> usize {
    let (a, b, c) = key;
    let mut h: u64 = 2166136261 ^ a;
    h = h.wrapping_mul(16777619) ^ b;
    h = h.wrapping_mul(16777619) ^ c as u64;
    (h % CACHE_CAPACITY as u64) as usize
}

/// 缓存条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CacheEntry {
    /// 键（源 id / 目标 id / 意图）。
    pub key: (u64, u64, u8),
    /// 建立时所依据的 profile 版本戳。
    pub version: u32,
    /// 命中计数（性能可见性；只增不清）。
    pub hits: u32,
}

/// 变换缓存：定容 64 槽，FNV 定槽 O(1) 命中；版本戳失配即失效。
#[derive(Clone, Debug)]
pub struct TransformCache {
    slots: [Option<CacheEntry>; CACHE_CAPACITY],
    /// 当前认可的 profile 版本（parse/重校准后 bump）。
    pub version: u32,
    /// 失效次数（版本戳失效立案计数）。
    pub invalidations: u32,
}

impl TransformCache {
    /// 空缓存。
    pub fn new() -> TransformCache {
        TransformCache { slots: [None; CACHE_CAPACITY], version: 1, invalidations: 0 }
    }

    /// 查缓存（O(1)：定槽比较；版本失配视同未命中并清槽）。
    pub fn lookup(&mut self, key: (u64, u64, u8)) -> bool {
        let s = slot_of(key);
        match self.slots[s] {
            Some(e) if e.key == key && e.version == self.version => {
                self.slots[s] = Some(CacheEntry { hits: e.hits.saturating_add(1), ..e });
                true
            }
            Some(_) => {
                // 键同版本旧 或 槽被别的键占着：都按未命中处理；本键失配清槽。
                if self.slots[s].map(|e| e.key) == Some(key) {
                    self.slots[s] = None;
                    self.invalidations = self.invalidations.saturating_add(1);
                }
                false
            }
            None => false,
        }
    }

    /// 插入（版本戳取当前；同槽旧键被替换——冲突槽即淘汰位）。
    pub fn insert(&mut self, key: (u64, u64, u8)) {
        let s = slot_of(key);
        self.slots[s] = Some(CacheEntry { key, version: self.version, hits: 0 });
    }

    /// 版本戳递增：**全表失效**（重校准后旧变换系统性错误，不是过期数据）。
    pub fn bump_version(&mut self) -> u32 {
        self.version = self.version.saturating_add(1);
        let n = self.slots.iter_mut().filter(|s| s.is_some()).count();
        for s in self.slots.iter_mut() {
            *s = None;
        }
        self.invalidations = self.invalidations.saturating_add(n as u32);
        self.version
    }

    /// 判据访问器：定槽函数（确定性/O(1) 口径对账用）。
    pub fn slot_of_for_test(key: (u64, u64, u8)) -> usize {
        slot_of(key)
    }

    /// 判据访问器：按槽位读条目（命中计数对账用）。
    pub fn slot_entry(&self, i: usize) -> Option<CacheEntry> {
        self.slots.get(i).copied().flatten()
    }
}

// ---------------------------------------------------------------------------
// 六、模块四：异常回退（缺省 sRGB + 标注）
// ---------------------------------------------------------------------------

/// 缺省标注单条（锚点「缺省标注单」——可查询可读屏，非日志）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DefaultNote {
    /// 显示器身份键。
    pub identity: String,
    /// 原因码（E_COLOR_UNCALIBRATED / E_COLOR_CONFIG / E_COLOR_TRANSFORM）。
    pub reason: &'static str,
    /// 人话说明（读屏可读）。
    pub detail: String,
}

impl DefaultNote {
    /// 读屏单行。
    pub fn screen_line(&self) -> String {
        format!(
            "显示器 {} 色彩走 sRGB 缺省路径：{}（{}）",
            self.identity, self.detail, self.reason
        )
    }
}

/// 缺省原因归类。
pub fn fallback_note(identity: &str, reason: &'static str) -> DefaultNote {
    let detail = match reason {
        E_COLOR_UNCALIBRATED => "该显示器无校准数据",
        E_COLOR_CONFIG => "校准配置解析失败",
        E_COLOR_TRANSFORM => "专用变换执行失败，已回退",
        _ => "未知的降级原因",
    };
    DefaultNote { identity: identity.to_string(), reason, detail: detail.to_string() }
}

// ---------------------------------------------------------------------------
// 七、引擎总成：激活状态表 + 主流程编排
// ---------------------------------------------------------------------------

/// 单台显示器的色彩激活状态。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ColorActivation {
    /// 已激活专用 profile（校准解析成功）。
    Active { kind: ProfileKind, version: u32 },
    /// 缺省降级（sRGB + 标注在案）。
    DefaultFallback { reason: &'static str },
}

/// 引擎激活状态表（每台显示器一条；解耦设备层——只认身份键）。
#[derive(Clone, Debug, Default)]
pub struct ActivationTable {
    entries: Vec<(String, ColorActivation)>,
    /// 缺省标注单（可查询）。
    pub notes: Vec<DefaultNote>,
}

impl ActivationTable {
    /// 空表。
    pub fn new() -> ActivationTable {
        ActivationTable { entries: Vec::new(), notes: Vec::new() }
    }

    /// 查某显示器激活状态。
    pub fn get(&self, identity: &str) -> Option<&ColorActivation> {
        self.entries.iter().find(|(k, _)| k == identity).map(|(_, v)| v)
    }

    /// 主流程编排（锚点五类要点：解析→按需激活→缺省标注→缓存版本对齐）。
    ///
    /// 无校准数据 / 解析失败均走缺省 sRGB **并标注**（不静默）；
    /// 校准成功则激活专用 profile 并记录版本戳。
    pub fn activate(&mut self, identity: &str, config: Option<&str>) -> ColorActivation {
        let outcome = match config {
            None => Err(E_COLOR_UNCALIBRATED),
            Some(data) => match parse_config(data, Some(identity.to_string())) {
                Ok(p) => Ok(p),
                Err(()) => Err(E_COLOR_CONFIG),
            },
        };
        let act = match outcome {
            Ok(p) => ColorActivation::Active { kind: p.kind, version: p.version },
            Err(reason) => {
                self.notes.push(fallback_note(identity, reason));
                ColorActivation::DefaultFallback { reason }
            }
        };
        if let Some(slot) = self.entries.iter_mut().find(|(k, _)| k == identity) {
            slot.1 = act.clone();
        } else {
            self.entries.push((identity.to_string(), act.clone()));
        }
        act
    }

    /// 从 F4402 能力快照按需激活（锚点「按需激活」的设备侧入口）。
    ///
    /// HDR 屏无校准时标注**加粗**（缺省 sRGB 对 HDR 的观感损失最大），
    /// 但仍是缺省 sRGB——「标注加粗」是可查的 reason 附加语义，不改路径。
    pub fn activate_from_capability(
        &mut self,
        identity: &str,
        cap: &CapabilityRecord,
        config: Option<&str>,
    ) -> ColorActivation {
        let act = self.activate(identity, config);
        if let ColorActivation::DefaultFallback { reason } = &act {
            if cap.hdr == HdrCapability::Present && *reason == E_COLOR_UNCALIBRATED {
                if let Some(n) = self.notes.iter_mut().rev().find(|n| n.identity == identity) {
                    n.detail = format!("{}；HDR 屏未校准，广色域内容观感受限", n.detail);
                }
            }
        }
        act
    }

    /// 缺省降级的台数（诊断面板）。
    pub fn fallback_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|(_, v)| matches!(v, ColorActivation::DefaultFallback { .. }))
            .count()
    }

    /// 在册台数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 读屏摘要（色彩状态可查——锚点无障碍要求）。
    pub fn screen_summary(&self) -> String {
        format!(
            "色彩管理：{} 台显示器在册，{} 台走 sRGB 缺省，标注 {} 条",
            self.len(),
            self.fallback_count(),
            self.notes.len()
        )
    }
}
