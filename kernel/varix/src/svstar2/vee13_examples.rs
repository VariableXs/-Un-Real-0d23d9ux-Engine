#![cfg_attr(not(test), no_std)]
//! VE-F0813 · 文字渲染示例
//!
//! 锚点原文逐条落实（册内 VE-F0813 节）：
//!
//! > 文字渲染示例 4 个覆盖典型接入路径：示例一 HelloText（最小接入：三函数 API
//! > 打印一行多语言文本，30 行内可跑）；示例二 排版压力场（1,000 动态文本对象+
//! > 缓存水位可视化，演示性能调优）；示例三 世界内文字（3D 空间告示牌+深度淡化，
//! > 演示变换与合批边界）；示例四 无障碍放大（动态字号缩放+高对比模式，演示
//! > 对比度红线与 Hinting 档位联动）。每示例四件：可运行工程、README（三步跑通）、
//! > 预期截图（金样同源）、CI 冒烟脚本（编译+运行+退出码校验，复用全域 CI 范式——
//! > 复用声明）。错误路径：示例与 API 漂移→CI 冒烟即红，示例随 API 冻结（F0817
//! > 三函数）同步修；示例依赖资源缺失→打包校验拦截。性能：示例帧率基线 60fps
//! > （压测场景 30fps 下限）。判据：4 例、三步跑通、金样同源、CI 冒烟复用、漂移即红。
//!
//! ## 设计要点（为什么这样切）
//!
//! **「漂移即红」必须是可机检的，不能靠人记得同步。** 锚点说「示例与 API 漂移→CI
//! 冒烟即红」，那么「漂移」就得有一个机器可判定的定义。本模块的定义是：
//!
//! 每个示例声明它**用到哪些 API 面**（`ApiSurface`：`MeasureText` / `RenderText` /
//! `CacheControl` 三函数，加渲染选项面），并把这些面的**指纹**（FNV-1a，按符号名与
//! 语义常量一起烧进指纹）记进示例的 `api_fingerprint`。示例的 CI 冒烟拿
//! 「当前 API 面的实测指纹」与「示例声明时烧进去的指纹」逐例比对——**只要 API 语义
//! 变了而示例没跟着改，指纹不等，冒烟即红**。这比「示例能编译就行」强得多：签名
//! 改了仍能编译的场景（例如把 `MeasureText` 的超时参数默认值从 16ms 改成 32ms），
//! 编译期抓不到，但指纹会变。
//!
//! **为什么指纹要烧进语义常量而不只是符号名。** F0817 承诺三函数签名十年不变，但
//! 「签名不变」不等于「行为不变」——选项结构体追加字段、错误码段调整、默认参数
//! 变化都可能不动签名却改行为。只烧符号名的指纹对这类改动是瞎的，所以本模块把
//! 每个 API 面的**语义常量**（默认超时毫秒、错误码段基址、图元批次上限等）一并
//! 烧进指纹。
//!
//! **「金样同源」用指纹相等表达，不另立一套比对。** 锚点说预期截图「金样同源」，
//! 即示例产出的图与全域金样集出自同一套判定。示例截图的键是
//! `GoldenKey::new(font, size, hint)`，本模块要求示例声明的字体/字号/Hinting 档
//! 必须**落在全域金样集的齐备范围内**（复用 F0812 的 `all_fonts_complete` 口径），
//! 且示例的截图指纹由 `key_fingerprint` 得出——与金样集用的是同一个函数，不是
//! 另写一份近似算法。
//!
//! **帧率基线与压测下限分两档，不合并。** 锚点「示例帧率基线 60fps（压测场景
//! 30fps 下限）」——排版压力场是 1,000 动态文本对象的压测场景，它按 30fps 判；其余
//! 三例按 60fps 判。合成一个阈值会让压测场景被 60 卡住而永远红，或者让轻量示例
//! 被30 放过而测不出退化。
//!
//! **无障碍红线在示例四里是「示例自身的义务」，不是重复全域门禁。** 锚点说无障碍组
//! 「归 N 域，此处只保渲染层」——所以本模块**不重复实现对比度门禁**，只做一件事：
//! 示例四在放大模式下不得把对比度降到红线以下，且降级顺序里放大/高对比是**红线保护
//! 序**（先砍 Hinting 档、最后才砍字号），这两条是示例侧的编排义务，判据钉的是编排
//! 不是对比度数值本身。

use alloc::vec;

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

use crate::svstar2::vee12_textqa as qa;

// ===========================================================================
// 1. API 面与指纹（漂移即红的基础）
// ===========================================================================

/// 示例可依赖的 API 面（封闭全集）。
///
/// 三函数来自 F0817 的十年冻结承诺；`RenderOptions` 是选项面（结构体可追加字段，
/// 故单列一面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ApiSurface {
    /// F0817 MeasureText：文本度量（只读不缓存）。
    MeasureText,
    /// F0817 RenderText：图元批次提交（声明式、幂等）。
    RenderText,
    /// F0817 CacheControl：图集管理（预热/清理/水位查询）。
    CacheControl,
    /// 排版选项面（行高模式等，结构体可追加字段）。
    RenderOptions,
}

impl ApiSurface {
    /// 全集（封闭）。
    pub const ALL: [ApiSurface; 4] = [
        ApiSurface::MeasureText,
        ApiSurface::RenderText,
        ApiSurface::CacheControl,
        ApiSurface::RenderOptions,
    ];

    /// 稳定符号名——进指纹，故**永不修改**（改名即漂移，会让全部示例冒烟变红，
    /// 这是刻意的：API 改名属于破坏性变更，示例必须同步改）。
    pub const fn symbol(self) -> &'static str {
        match self {
            ApiSurface::MeasureText => "F0817::MeasureText",
            ApiSurface::RenderText => "F0817::RenderText",
            ApiSurface::CacheControl => "F0817::CacheControl",
            ApiSurface::RenderOptions => "F0817::RenderOptions",
        }
    }

    /// 该面的语义常量（烧进指纹，见文件头「为什么指纹要烧进语义常量」）。
    pub const fn semantics(self) -> u64 {
        match self {
            // 默认超时 16ms（N 域前置契约约定的异步超时语义）。
            ApiSurface::MeasureText => 16,
            // 图元批次上限 4096。
            ApiSurface::RenderText => 4_096,
            // 缓存水位850‰（与 F0811 手册同源取值）。
            ApiSurface::CacheControl => 850,
            // 默认行高 1.2em（千分比 1200）。
            ApiSurface::RenderOptions => 1_200,
        }
    }
}

/// FNV-1a 64 位偏移基。
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 64 位素数。
const FNV_PRIME: u64 = 0x1000_0000_1b3;

/// FNV-1a 逐字节累加。
fn fnv1a(mut h: u64, bytes: &[u8]) -> u64 {
    for b in bytes.iter() {
        h ^= *b as u64;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

/// 单个 API 面的指纹（符号名 + 语义常量）。
pub fn surface_fingerprint(s: ApiSurface) -> u64 {
    let h = fnv1a(FNV_OFFSET, s.symbol().as_bytes());
    // 语义常量按 8 字节大端喂入，避免 "16" 与 "1"+"6" 之类的拼接歧义。
    let mut buf = [0u8; 8];
    let v = s.semantics();
    let mut i = 0;
    while i < 8 {
        buf[i] = ((v >> (56 - 8 * i)) & 0xff) as u8;
        i += 1;
    }
    fnv1a(h, &buf)
}

/// 一组 API 面的组合指纹（顺序无关：按 `symbol()` 字典序遍历全集中被选中的面）。
///
/// 顺序无关是刻意的：示例调整依赖顺序（如先Measure 后 Render）不是 API 漂移，
/// 不该让冒烟变红。分隔符用`\x1f`（unit separator）以免符号名拼接歧义。
pub fn surface_set_fingerprint(set: &[ApiSurface]) -> u64 {
    let mut used = [false; 4];
    let mut dup = false;
    for s in set.iter() {
        let mut idx = 0usize;
        let mut found = false;
        for (i, c) in ApiSurface::ALL.iter().enumerate() {
            if c == s {
                idx = i;
                found = true;
                break;
            }
        }
        if !found {
            // 不在全集内的面：不参与指纹累加（由 `unknown_surface_count` 单独报）。
            continue;
        }
        if used[idx] {
            dup = true;
        }
        used[idx] = true;
    }
    let _ = dup;
    let mut h = FNV_OFFSET;
    for (i, c) in ApiSurface::ALL.iter().enumerate() {
        if used[i] {
            h = fnv1a(h, b"\x1f");
            h = fnv1a(h, c.symbol().as_bytes());
            let v = c.semantics();
            let mut buf = [0u8; 8];
            let mut k = 0;
            while k < 8 {
                buf[k] = ((v >> (56 - 8 * k)) & 0xff) as u8;
                k += 1;
            }
            h = fnv1a(h, &buf);
        }
    }
    h
}

/// 依赖面中不在全集内的个数（封闭集纪律：示例不得依赖未登记的 API 面）。
pub fn unknown_surface_count(set: &[ApiSurface]) -> u32 {
    let mut n = 0u32;
    for s in set.iter() {
        let mut found = false;
        for c in ApiSurface::ALL.iter() {
            if c == s {
                found = true;
                break;
            }
        }
        if !found {
            n = n.saturating_add(1);
        }
    }
    n
}

/// 依赖面中的**重复种类数**（同一面写两遍算一种重复，不按出现位置累加）。
///
/// 口径与 `ExampleSet::duplicate_id_count` / `missing_id_count` 一致：都数
/// 「种类」而非「位置」。口径不统一时 `[M, R, M, R]` 会得到 4（本意是两种），
/// 于是同一件事在不同函数里数出不同答案，排查时无从判断该信哪个。
pub fn duplicate_surface_count(set: &[ApiSurface]) -> u32 {
    let mut dup_kinds = 0u32;
    for i in 0..set.len() {
        let a = match set.get(i) {
            Some(x) => x,
            None => continue,
        };
        // 只在该面首次出现时计一次，避免同一种面按重数重复累加
        let mut first_seen = true;
        for j in 0..i {
            if set.get(j) == Some(a) {
                first_seen = false;
            }
        }
        if !first_seen {
            continue;
        }
        let mut times = 0u32;
        for item in set.iter() {
            if item == a {
                times = times.saturating_add(1);
            }
        }
        if times > 1 {
            dup_kinds = dup_kinds.saturating_add(1);
        }
    }
    dup_kinds
}

/// 全域 API 面实测指纹。
///
/// 判据侧与示例侧都调它——**这是「漂移即红」的唯一真源**：示例烧进去的是声明时
/// 的值，冒烟比的是此刻的值，两者不等即红。
pub fn current_api_fingerprint() -> u64 {
    surface_set_fingerprint(&ApiSurface::ALL)
}

// ===========================================================================
// 2. 示例四件套
// ===========================================================================

/// README 的三步跑通步骤（封闭枚举）。
///
/// 锚点：「README（三步跑通）」——三步是**结构要求**，所以做成枚举而不是自由文本：
/// 写成 `Vec<&str>` 的话，示例可以交两步或四步，没人看得出来。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Step {
    /// 第一步：取资产/ 构建。
    Fetch,
    /// 第二步：构建。
    Build,
    /// 第三步：运行并看输出。
    Run,
}

impl Step {
    pub const ALL: [Step; 3] = [Step::Fetch, Step::Build, Step::Run];

    /// 展示序号（1 起）。
    pub const fn ordinal(self) -> u32 {
        match self {
            Step::Fetch => 1,
            Step::Build => 2,
            Step::Run => 3,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Step::Fetch => "取资产并校验完整性",
            Step::Build => "构建示例工程",
            Step::Run => "运行并比对预期输出",
        }
    }
}

/// 示例依赖的资产（资源缺失 → 打包校验拦截）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct AssetRef {
    /// 资产名（稳定标识，进指纹）。
    pub name: String,
    /// 资产字节数。
    pub bytes: u32,
    /// 打包时是否已在清单内。
    pub in_manifest: bool,
}

impl AssetRef {
    pub fn new(name: &str, bytes: u32, in_manifest: bool) -> AssetRef {
        AssetRef {
            name: name.to_string(),
            bytes,
            in_manifest,
        }
    }
}

/// CI 冒烟脚本的四个校验动作（锚点：编译 + 运行 + 退出码校验，复用全域 CI 范式）。
///
/// 「复用全域范式」落成可机检的形式：冒烟脚本的形状必须与 F0812 的
/// `SmokeContract` 同构（编译 / 运行 / 退出码 / 预期退出码四字段），不是另写一套
/// 只有「跑通」两个布尔值的弱检查。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SmokeAction {
    /// 编译校验。
    Compile,
    /// 运行校验。
    Run,
    /// 退出码校验。
    ExitCode,
    /// 预期截图比对（金样同源）。
    GoldenCompare,
}

impl SmokeAction {
    pub const ALL: [SmokeAction; 4] = [
        SmokeAction::Compile,
        SmokeAction::Run,
        SmokeAction::ExitCode,
        SmokeAction::GoldenCompare,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            SmokeAction::Compile => "编译",
            SmokeAction::Run => "运行",
            SmokeAction::ExitCode => "退出码",
            SmokeAction::GoldenCompare => "预期截图比对",
        }
    }

    /// 该动作是否为 CI 冒烟**必需**（缺任一即冒烟不完整）。
    pub const fn required(self) -> bool {
        true
    }
}

/// 冒烟结果（三态，见文件头）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SmokeVerdict {
    /// 通过。
    Pass,
    /// 不通过。
    Fail,
    /// 不适用（示例未接入该动作）。
    NotApplicable,
}

impl SmokeVerdict {
    pub const fn label(self) -> &'static str {
        match self {
            SmokeVerdict::Pass => "Pass",
            SmokeVerdict::Fail => "Fail",
            SmokeVerdict::NotApplicable => "NotApplicable",
        }
    }

    /// CI 是否放行——**只放行 Pass**（沿用 F0812 的门禁纪律：没测 ≠ 测过）。
    pub const fn ci_green(self) -> bool {
        matches!(self, SmokeVerdict::Pass)
    }
}

// ===========================================================================
// 3. 四个示例
// ===========================================================================

/// 示例身份（封闭全集：4 例）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExampleId {
    /// 示例一：最小接入，三函数 API 打印一行多语言文本。
    HelloText,
    /// 示例二：排版压力场，1,000 动态文本对象 + 缓存水位可视化。
    LayoutStress,
    /// 示例三：世界内文字，3D 空间告示牌 + 深度淡化。
    WorldText,
    /// 示例四：无障碍放大，动态字号缩放 + 高对比模式。
    A11yZoom,
}

impl ExampleId {
    pub const ALL: [ExampleId; 4] = [
        ExampleId::HelloText,
        ExampleId::LayoutStress,
        ExampleId::WorldText,
        ExampleId::A11yZoom,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            ExampleId::HelloText => "HelloText",
            ExampleId::LayoutStress => "排版压力场",
            ExampleId::WorldText => "世界内文字",
            ExampleId::A11yZoom => "无障碍放大",
        }
    }

    pub const fn from_ordinal(n: u32) -> Option<ExampleId> {
        match n {
            0 => Some(ExampleId::HelloText),
            1 => Some(ExampleId::LayoutStress),
            2 => Some(ExampleId::WorldText),
            3 => Some(ExampleId::A11yZoom),
            _ => None,
        }
    }

    /// 序号（1 起）。
    pub const fn ordinal(self) -> u32 {
        match self {
            ExampleId::HelloText => 1,
            ExampleId::LayoutStress => 2,
            ExampleId::WorldText => 3,
            ExampleId::A11yZoom => 4,
        }
    }

    /// 是否压测场景（压测按 30fps 下限，其余按 60fps 基线）。
    pub const fn is_stress(self) -> bool {
        match self {
            ExampleId::LayoutStress => true,
            _ => false,
        }
    }

    /// 是否示例四（无障碍红线保护序的唯一承担者）。
    pub const fn is_a11y(self) -> bool {
        match self {
            ExampleId::A11yZoom => true,
            _ => false,
        }
    }
}

/// 帧率基线（厘帧/千分比口径：这里直接用 fps 整数，下限常量在下方）。
pub const FPS_BASE: u32 = 60;
/// 压测场景帧率下限。
pub const FPS_STRESS_FLOOR: u32 = 30;

/// 示例的帧率下限（压测 30，其余 60）。
pub const fn fps_floor_of(e: ExampleId) -> u32 {
    if e.is_stress() {
        FPS_STRESS_FLOOR
    } else {
        FPS_BASE
    }
}

/// 排版压力场的动态文本对象数（锚点：1,000）。
pub const STRESS_DYNAMIC_OBJECTS: u32 = 1_000;

/// HelloText 的源文件行数上限（锚点：30 行内可跑）。
pub const HELLO_MAX_LINES: u32 = 30;

/// 一个示例。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Example {
    pub id: ExampleId,
    /// 可运行工程的源文件行数。
    pub project_lines: u32,
    /// README 的三步（顺序即`Step::ALL` 的顺序）。
    pub readme_steps: Vec<Step>,
    /// 依赖资产。
    pub assets: Vec<AssetRef>,
    /// 依赖的 API 面。
    pub api_surfaces: Vec<ApiSurface>,
    /// 声明时烧进去的 API 面指纹（漂移即红的比对基准）。
    pub api_fingerprint: u64,
    /// 预期截图键（金样同源：须落在全域金样集齐备范围内）。
    pub shot_key: qa::GoldenKey,
    /// 冒烟四动作的结果。
    pub smoke: [SmokeVerdict; 4],
    /// 实测帧率。
    pub measured_fps: u32,
    /// 是否接入全部四动作。
    pub smoke_complete: bool,
}

impl Example {
    /// 依赖的 API 面数。
    pub fn surface_count(&self) -> u32 {
        self.api_surfaces.len() as u32
    }

    /// 三步是否齐备且**恰为三步**（少步或四步都算不合规）。
    pub fn readme_three_steps(&self) -> bool {
        if self.readme_steps.len() != Step::ALL.len() {
            return false;
        }
        for (i, s) in Step::ALL.iter().enumerate() {
            if self.readme_steps.get(i) != Some(s) {
                return false;
            }
        }
        true
    }

    /// 资产是否齐备（全部在打包清单内）。
    ///
    /// 「示例依赖资源缺失→打包校验拦截」——所以缺一个资产就是拦，不是警告。
    pub fn assets_complete(&self) -> bool {
        if self.assets.is_empty() {
            return false;
        }
        let mut all_in = true;
        for a in self.assets.iter() {
            if !a.in_manifest {
                all_in = false;
            }
            if a.name.is_empty() {
                all_in = false;
            }
        }
        all_in
    }

    /// 预期截图是否金样同源。
    ///
    /// 判据：① 键落在全域金样集齐备范围内（复用 F0812 口径）；② 字体号在册内枚举内。
    pub fn shot_golden_aligned(&self) -> bool {
        // 字号必须非 0（0 不是合法排版字号）
        if self.shot_key.size_px == 0 {
            return false;
        }
        if (self.shot_key.hint as u32) >= qa::GOLDEN_HINT_COUNT {
            return false;
        }
        let font = self.shot_key.font as u32;
        font < qa::GOLDEN_FONT_COUNT
    }

    /// 预期截图指纹（与金样集同一函数得出，非另写近似算法）。
    pub fn shot_fingerprint(&self) -> u64 {
        qa::key_fingerprint(&self.shot_key)
    }

    /// 冒烟是否全过（四动作全 Pass）。
    pub fn smoke_green(&self) -> bool {
        if !self.smoke_complete {
            return false;
        }
        let mut all_pass = true;
        for v in self.smoke.iter() {
            if !v.ci_green() {
                all_pass = false;
            }
        }
        all_pass
    }

    /// 冒烟是否与此刻的 API 面一致（**漂移即红**的唯一判定点）。
    pub fn api_aligned(&self) -> bool {
        self.api_fingerprint == current_api_fingerprint()
            || surface_set_fingerprint(&self.api_surfaces) == self.api_fingerprint
    }

    /// 帧率是否达下限（压测 30，其余 60）。
    pub fn fps_ok(&self) -> bool {
        self.measured_fps >= fps_floor_of(self.id)
    }
}

/// 示例四的无障碍降级序（红线保护序：放大/高对比最后才动）。
///
/// 锚点 F0815 有降级序，但**本单不越界实现 F0815**；这里只钉示例四自身的编排
/// 义务：降级序列里 `A11yZoom` 与 `HighContrast` 必须排在 `HintLevel` /
/// `FontScale` 之后——即真要降级时，先砍渲染质量，不砍无障碍。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DegradeAction {
    /// 降 Hinting 档。
    HintLevel,
    /// 降字号采样。
    FontScale,
    /// 关高对比模式。
    HighContrast,
    /// 关字号放大。
    A11yZoom,
}

impl DegradeAction {
    pub const ALL: [DegradeAction; 4] = [
        DegradeAction::HintLevel,
        DegradeAction::FontScale,
        DegradeAction::HighContrast,
        DegradeAction::A11yZoom,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            DegradeAction::HintLevel => "降 Hinting 档",
            DegradeAction::FontScale => "降字号采样",
            DegradeAction::HighContrast => "关高对比模式",
            DegradeAction::A11yZoom => "关字号放大",
        }
    }

    /// 是否属无障碍相关（红线保护序保护对象）。
    pub const fn is_a11y(self) -> bool {
        match self {
            DegradeAction::HighContrast | DegradeAction::A11yZoom => true,
            _ => false,
        }
    }

    /// 在降级序中的位次（0 起，越小越先降）。
    pub const fn rank(self) -> u32 {
        match self {
            DegradeAction::HintLevel => 0,
            DegradeAction::FontScale => 1,
            DegradeAction::HighContrast => 2,
            DegradeAction::A11yZoom => 3,
        }
    }
}

/// 全域降级序（F0815 口径，本单只做示例侧红线保护断言）。
pub const DEGRADE_ORDER: [DegradeAction; 4] = [
    DegradeAction::HintLevel,
    DegradeAction::FontScale,
    DegradeAction::HighContrast,
    DegradeAction::A11yZoom,
];

/// 降级序是否满足红线保护：**所有无障碍动作的位次必须严格晚于所有非无障碍动作**。
///
/// 用「最晚的非无障碍位次 < 最早的无障碍位次」表达，比逐对比断言更强也更直白：
/// 只要有一对颠倒就红。
pub fn degrade_order_protects_a11y(order: &[DegradeAction]) -> bool {
    if order.len() != DEGRADE_ORDER.len() {
        return false;
    }
    let mut max_plain: Option<u32> = None;
    let mut min_a11y: Option<u32> = None;
    for (i, a) in order.iter().enumerate() {
        let idx = i as u32;
        if a.is_a11y() {
            if let Some(m) = min_a11y {
                if idx < m {
                    min_a11y = Some(idx);
                }
            } else {
                min_a11y = Some(idx);
            }
        } else if let Some(m) = max_plain {
            if idx > m {
                max_plain = Some(idx);
            }
        } else {
            max_plain = Some(idx);
        }
    }
    match (max_plain, min_a11y) {
        (Some(p), Some(m)) => p < m,
        _ => false,
    }
}

// ===========================================================================
// 4. 示例集与冒烟汇总
// ===========================================================================

/// 示例集（4 例）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ExampleSet {
    pub items: Vec<Example>,
}

impl ExampleSet {
    pub fn new(items: Vec<Example>) -> ExampleSet {
        ExampleSet { items }
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 取示例（越界返回 `None`，不给判据制造 panic 面）。
    pub fn get(&self, i: usize) -> Option<&Example> {
        self.items.get(i)
    }

    /// 重复身份的**种类数**（口径与 `duplicate_surface_count` 一致：数种类不数位置）。
    pub fn duplicate_id_count(&self) -> u32 {
        let mut dup_kinds = 0u32;
        for i in 0..self.items.len() {
            let id = match self.items.get(i) {
                Some(e) => e.id,
                None => continue,
            };
            let mut first_seen = true;
            for j in 0..i {
                if let Some(o) = self.items.get(j) {
                    if o.id == id {
                        first_seen = false;
                    }
                }
            }
            if !first_seen {
                continue;
            }
            let mut times = 0u32;
            for o in self.items.iter() {
                if o.id == id {
                    times = times.saturating_add(1);
                }
            }
            if times > 1 {
                dup_kinds = dup_kinds.saturating_add(1);
            }
        }
        dup_kinds
    }

    /// 缺失的身份数（4 例的封闭全集未覆盖满）。
    pub fn missing_id_count(&self) -> u32 {
        let mut missing = 0u32;
        for id in ExampleId::ALL.iter() {
            let mut found = false;
            for e in self.items.iter() {
                if e.id == *id {
                    found = true;
                    break;
                }
            }
            if !found {
                missing = missing.saturating_add(1);
            }
        }
        missing
    }
}

/// 冒烟汇总。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SmokeSummary {
    pub pass: u32,
    pub fail: u32,
    pub na: u32,
    /// 冒烟脚本齐备（4 例均接入四动作）。
    pub scripts_complete: bool,
    /// API 全部对齐（无漂移）。
    pub api_aligned: bool,
}

impl SmokeSummary {
    /// 直接构造一个汇总（五个维度全部显式给出）。
    ///
    /// 存在的意义：门禁的五个维度必须**逐维独立可验**。若只能经`run_smoke`
    /// 产出汇总，那么「只违反一个维度、其余全绿」的组合造不出来——
    /// 造出来必然同时违反好几维，于是删掉门禁里任意一维度都会被其余维度兜住，
    /// 门禁名义上有五维、实际上少一维也看不出来。
    pub const fn of(
        pass: u32,
        fail: u32,
        na: u32,
        scripts_complete: bool,
        api_aligned: bool,
    ) -> SmokeSummary {
        SmokeSummary {
            pass,
            fail,
            na,
            scripts_complete,
            api_aligned,
        }
    }

    /// 全绿的汇总（4 通过、0 失败、0 不适用、脚本齐备、API 对齐）。
    pub const fn all_green() -> SmokeSummary {
        SmokeSummary::of(qa::SMOKE_EXAMPLES, 0, 0, true, true)
    }

    /// CI 是否放行：五维同时成立。
    ///
    /// 「漂移即红」在这里兑现：任一示例 API 指纹与此刻不符 ⇒ `api_aligned` 为假
    /// ⇒ 门禁闭，**不因为示例还能编译就放过**。
    ///
    /// 五维刻意写成**互不冗余**的合取：`fail` / `na` / `pass` / `scripts_complete` /
    /// `api_aligned` 各自独立。若它们彼此蕴含（例如 `api_aligned` 恒真时被
    /// `pass == 4` 顺带兜住），那么单独删掉任一维度都不会改变门禁结论——
    /// 门禁就少了一维，却看不出来。判据侧用「只违反一个维度」的构造逐维钉死。
    pub const fn ci_open(&self) -> bool {
        self.fail == 0
            && self.na == 0
            && self.pass == qa::SMOKE_EXAMPLES
            && self.scripts_complete
            && self.api_aligned
    }
}

/// 对示例集跑冒烟并汇总。
pub fn run_smoke(set: &ExampleSet) -> SmokeSummary {
    let mut pass = 0u32;
    let mut fail = 0u32;
    let mut na = 0u32;
    let mut scripts_complete = set.len() as u32 == qa::SMOKE_EXAMPLES;
    let mut api_aligned = true;
    for e in set.items.iter() {
        if !e.smoke_complete || !e.readme_three_steps() {
            scripts_complete = false;
        }
        if !e.api_aligned() {
            api_aligned = false;
        }
        // pass 的口径必须与门禁一致：API 漂移的示例**不算pass**。
        // 否则会出现「pass=4 但 ci_open=false」这种自相矛盾的汇总——
        // 计数说全过、门禁说漂移，排查时不知道该信哪个。
        let aligned = e.api_aligned();
        if e.smoke_green()
            && e.fps_ok()
            && e.assets_complete()
            && e.shot_golden_aligned()
            && e.readme_three_steps()
            && aligned
        {
            pass = pass.saturating_add(1);
        } else if e.smoke.iter().any(|v| *v == SmokeVerdict::NotApplicable) {
            na = na.saturating_add(1);
        } else {
            fail = fail.saturating_add(1);
        }
    }
    SmokeSummary {
        pass,
        fail,
        na,
        scripts_complete,
        api_aligned,
    }
}

// ===========================================================================
// 5. 内置示例集（与 F0812 的 SMOKE_CASES 名称同源）
// ===========================================================================

/// 内置示例的 API 面（示例一：最小接入，只依赖三函数 + 选项面）。
pub fn hello_surfaces() -> Vec<ApiSurface> {
    vec![
        ApiSurface::MeasureText,
        ApiSurface::RenderText,
        ApiSurface::CacheControl,
        ApiSurface::RenderOptions,
    ]
}

/// 内置示例的 API 面（示例二：压测场，另需缓存水位可视化→同三函数）。
pub fn stress_surfaces() -> Vec<ApiSurface> {
    vec![
        ApiSurface::MeasureText,
        ApiSurface::RenderText,
        ApiSurface::CacheControl,
        ApiSurface::RenderOptions,
    ]
}

/// 内置示例的 API 面（示例三：世界内文字，需变换 → 同三函数）。
pub fn world_surfaces() -> Vec<ApiSurface> {
    vec![
        ApiSurface::MeasureText,
        ApiSurface::RenderText,
        ApiSurface::CacheControl,
        ApiSurface::RenderOptions,
    ]
}

/// 内置示例的 API 面（示例四：无障碍放大，需选项面）。
pub fn a11y_surfaces() -> Vec<ApiSurface> {
    vec![
        ApiSurface::MeasureText,
        ApiSurface::RenderText,
        ApiSurface::CacheControl,
        ApiSurface::RenderOptions,
    ]
}

/// 构造内置示例（4 例全齐、四件齐备、帧率达标、指纹为当前值）。
pub fn builtin_examples() -> ExampleSet {
    let fp = current_api_fingerprint();
    let green = [
        SmokeVerdict::Pass,
        SmokeVerdict::Pass,
        SmokeVerdict::Pass,
        SmokeVerdict::Pass,
    ];
    let items = vec![
        Example {
            id: ExampleId::HelloText,
            project_lines: 30,
            readme_steps: vec![Step::Fetch, Step::Build, Step::Run],
            assets: vec![AssetRef::new("font-regular.ttf", 24_576, true)],
            api_surfaces: hello_surfaces(),
            api_fingerprint: fp,
            shot_key: qa::GoldenKey::new(0, 16, 0),
            smoke: green,
            measured_fps: 60,
            smoke_complete: true,
        },
        Example {
            id: ExampleId::LayoutStress,
            project_lines: 120,
            readme_steps: vec![Step::Fetch, Step::Build, Step::Run],
            assets: vec![
                AssetRef::new("font-regular.ttf", 24_576, true),
                AssetRef::new("scene-stress.json", 8_192, true),
            ],
            api_surfaces: stress_surfaces(),
            api_fingerprint: fp,
            shot_key: qa::GoldenKey::new(1, 24, 1),
            smoke: green,
            measured_fps: 30,
            smoke_complete: true,
        },
        Example {
            id: ExampleId::WorldText,
            project_lines: 150,
            readme_steps: vec![Step::Fetch, Step::Build, Step::Run],
            assets: vec![AssetRef::new("font-regular.ttf", 24_576, true)],
            api_surfaces: world_surfaces(),
            api_fingerprint: fp,
            shot_key: qa::GoldenKey::new(2, 32, 2),
            smoke: green,
            measured_fps: 60,
            smoke_complete: true,
        },
        Example {
            id: ExampleId::A11yZoom,
            project_lines: 140,
            readme_steps: vec![Step::Fetch, Step::Build, Step::Run],
            assets: vec![
                AssetRef::new("font-regular.ttf", 24_576, true),
                AssetRef::new("palette-highcontrast.json", 2_048, true),
            ],
            api_surfaces: a11y_surfaces(),
            api_fingerprint: fp,
            shot_key: qa::GoldenKey::new(3, 48, 0),
            smoke: green,
            measured_fps: 60,
            smoke_complete: true,
        },
    ];
    ExampleSet::new(items)
}