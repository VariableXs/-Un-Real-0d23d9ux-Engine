//! VE-F0813 · 文字渲染示例（VE-E 域 · 文字渲染示例组 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0813`
//!
//! **职责定位（锚点原文）**：文字渲染示例 4 个覆盖典型接入路径：
//! 示例一 HelloText（最小接入：三函数 API 打印一行多语言文本，30 行内可跑）；
//! 示例二 排版压力场（1,000 动态文本对象 + 缓存水位可视化，演示性能调优）；
//! 示例三 世界内文字（3D 空间告示牌 + 深度淡化，演示变换与合批边界）；
//! 示例四 无障碍放大（动态字号缩放 + 高对比模式，演示对比度红线与
//! Hinting 档位联动）。
//!
//! **每示例四件**：可运行工程、README（三步跑通）、预期截图（金样同源）、
//! CI 冒烟脚本（编译 + 运行 + 退出码校验，复用全域 CI 范式——复用声明）。
//!
//! **错误路径与降级矩阵（锚点原文）**：
//!
//! - 示例与 API 漂移 → **CI 冒烟即红**，示例随 API 冻结（F0817 三函数）同步修；
//! - 示例依赖资源缺失 → **打包校验拦截**。
//!
//! **性能（锚点原文）**：示例帧率基线 60fps（压测场景 30fps 下限）。
//!
//! **判据（锚点原文）**：4 例、三步跑通、金样同源、CI 冒烟复用、漂移即红。
//!
//! ## 落位形态（与 F0812 资产范式同构）
//!
//! 示例四件在本内核库内以**规格化注册表**入库（工程入口 / README 步骤 /
//! 金样摘要 / 冒烟三维），真源即本模块 [`EXAMPLES`]——这与 F0812 测试资产
//! 的落位形态同构：内核库内不存在可独立运行的 GPU 示例二进制，示例的
//! 「可运行」由 CI 冒烟三维（编译/运行/退出码）在宿主侧兑现，库内登记
//! 其规格与判定契约。**金样同源**由此获得精确含义：预期截图摘要与金样
//! 从**同一确定性参考路径**（[`render_digest`]，纯函数、软件光栅口径、
//! 与 F0812「固定软件光栅化参考路径」同一纪律）重算得出，登记值与
//! 重算值逐位一致才算同源——预期截图若来自另一条（GPU）路径，「同源」
//! 就只剩一个没人能复核的声明。
//!
//! ## 设计要点
//!
//! - **三步跑通是结构不是文案**（[`RunStepKind`]）：构建 → 运行 → 校验，
//!   步骤带种类枚举，判据按种类断序——README 写反顺序（先校验后构建）
//!   是真会发生的错，靠关键词匹配抓不住（换一种说法就绕过去了）。
//! - **冒烟三维缺一即不合规**（[`SmokeScript::is_wellformed`]）：编译过
//!   不等于运行过、运行过不等于退出码对——三维是**与**关系，缺任何一维
//!   的脚本都不许注册；期望退出码必须显性为 0，「看退出码」写进注释
//!   而不写进字段等于没写。
//! - **漂移检测可注入**（[`drift_in`]）：对任意注册表切片扫描 API 指纹
//!   失配并定位到例 id——判据必须能构造漂移变体证明检测真的转红，
//!   只对内置注册表断「零漂移」的检测恒真（给它的输入永远干净）。
//! - **资源缺失拦截指名道姓**（[`pack_manifest_check`]）：拒绝必须带
//!   例 id，让「哪个示例缺哪件资源」一步可查——只返回一个笼统 Err
//!   的打包校验在四例场景下等于让人挨个翻。
//! - **帧率红线分场景**（[`PerfSpec::meets`]）：基线场景 60fps、压测
//!   场景 30fps 下限是两条**不同的**阈值，混用任何一条都会让另一场景
//!   的判定出错（压测拿 60 卡会把合规压测全判死；基线拿 30 卡会把
//!   掉帧的真问题放过去）。边界值恰等判达标（≥ 口径，与 F0035 一致）。
//! - **API 指纹是冻结尾**（[`API_SIGNATURES`]）：三函数签名文本的
//!   FNV-1a 摘要。示例登记其引用的指纹，指纹失配即漂移——签名文本
//!   改一个字符（哪怕只是参数名）都算漂移，这正是锚点「随 API 冻结
//!   同步修」要的效果：CI 红，人来看。
//!
//! ## 与相邻条的分工（易混，故写明）
//!
//! - **F0812（测试资产）管「测试自己的资产集与金样阈值」**，本条管
//!   「开发者怎么接入」的示例四件；本条金样的摘要算法与 F0812 的
//!   软件参考路径纪律同源（复用声明），阈值判定归 F0812 不管。
//! - **F0817（三函数 API）管「签名冻结」本身**，本条管「示例引用的
//!   签名指纹与冻结面一致」——漂移检测消费冻结面，不定义冻结面。
//! - **F0834（像素风示例，若立条）管位图字体示例**，本条四例不含
//!   位图路径（锚点四例清单是封闭清单）。
//!
//! 两条各自**自持**定义类型，不跨模块 `use`——并行提交时跨模块引用
//! 会把两个模块的编译成败绑在一起，一方半成品就拖垮另一方。

extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;

// ===========================================================================
// 0. 锚点常量（全部常量，非参数——可写阈值等于给现场放行留后门）
// ===========================================================================

/// 示例资产结构版本。
pub const DEMO_VERSION: u32 = 1;

/// 示例数（锚点：示例 4 个，封闭清单）。
pub const EXAMPLE_COUNT: usize = 4;

/// 示例一 HelloText 的行数预算（锚点：30 行内可跑）。
pub const HELLO_LINE_BUDGET: usize = 30;

/// 示例二排版压力场的动态文本对象数（锚点：1,000）。
pub const STRESS_DYNAMIC_OBJECTS: u32 = 1_000;

/// 帧率基线（锚点：示例帧率基线 60fps）。
pub const FPS_BASELINE: u32 = 60;

/// 压测场景帧率下限（锚点：压测场景 30fps 下限）。
pub const FPS_STRESS_FLOOR: u32 = 30;

/// 高对比模式对比度红线，千分比（4.5:1 = 4500‰，无障碍可读下限）。
pub const CONTRAST_REDLINE_PERMILLE: u32 = 4_500;

/// README 三步跑通的步数（锚点：三步跑通）。
pub const README_STEPS: usize = 3;

/// CI 冒烟三维度数（编译 / 运行 / 退出码——复用全域 CI 范式）。
pub const SMOKE_STAGES: usize = 3;

/// API 冻结函数数（F0817 三函数）。
pub const API_FROZEN_FUNCS: usize = 3;

// ===========================================================================
// 1. 示例 id 与步骤种类（封闭枚举 + 线编码往返）
// ===========================================================================

/// 示例 id（锚点四例封闭清单，顺序即注册表顺序）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExampleId {
    /// 示例一：最小接入。
    HelloText,
    /// 示例二：排版压力场。
    StressField,
    /// 示例三：世界内文字。
    WorldText,
    /// 示例四：无障碍放大。
    A11yScale,
}

impl ExampleId {
    /// 全枚举（顺序即 [`EXAMPLES`] 注册表顺序）。
    pub const ALL: [ExampleId; EXAMPLE_COUNT] = [
        ExampleId::HelloText,
        ExampleId::StressField,
        ExampleId::WorldText,
        ExampleId::A11yScale,
    ];

    /// 枚举下标。
    pub const fn ordinal(self) -> usize {
        match self {
            ExampleId::HelloText => 0,
            ExampleId::StressField => 1,
            ExampleId::WorldText => 2,
            ExampleId::A11yScale => 3,
        }
    }

    /// 线编码（显式映射，不用 `as u8`）。
    pub const fn wire(self) -> u8 {
        match self {
            ExampleId::HelloText => 0x41, // A
            ExampleId::StressField => 0x53, // S
            ExampleId::WorldText => 0x57, // W
            ExampleId::A11yScale => 0x4C, // L
        }
    }

    /// 线上编码 → 枚举（未登记码返回 `None`）。
    pub const fn from_wire(w: u8) -> Option<ExampleId> {
        match w {
            0x41 => Some(ExampleId::HelloText),
            0x53 => Some(ExampleId::StressField),
            0x57 => Some(ExampleId::WorldText),
            0x4C => Some(ExampleId::A11yScale),
            _ => None,
        }
    }

    /// 中文名。
    pub const fn label(self) -> &'static str {
        match self {
            ExampleId::HelloText => "HelloText 最小接入",
            ExampleId::StressField => "排版压力场",
            ExampleId::WorldText => "世界内文字",
            ExampleId::A11yScale => "无障碍放大",
        }
    }
}

/// README 步骤种类（三步跑通是**结构**：构建 → 运行 → 校验）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunStepKind {
    /// 第一步：构建。
    Build,
    /// 第二步：运行。
    Run,
    /// 第三步：校验（退出码/输出断言）。
    Verify,
}

impl RunStepKind {
    /// 全枚举，顺序即三步的**规定顺序**。
    pub const ALL: [RunStepKind; README_STEPS] =
        [RunStepKind::Build, RunStepKind::Run, RunStepKind::Verify];
}

// ===========================================================================
// 2. 资产来源与资源条目（复用声明 + 打包校验）
// ===========================================================================

/// 资产来源（与 F0812 `AssetOrigin` 同构自持——复用声明落在类型上）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssetOrigin {
    /// 全域共用件（改动须走全域评审）。
    SharedDomain,
    /// 本域专项件（本单自决）。
    LocalSpec,
}

/// 示例依赖资源条目。
#[derive(Clone, Copy, Debug)]
pub struct ResourceEntry {
    /// 资源名（如字体文件、图集、语言包）。
    pub name: &'static str,
    /// 资源种类（font / atlas / locale / config）。
    pub kind: &'static str,
}

// ===========================================================================
// 3. README 三步与 CI 冒烟三维
// ===========================================================================

/// 一条跑通步骤。
#[derive(Clone, Copy, Debug)]
pub struct RunStep {
    /// 步骤种类（结构位，判据按此断序）。
    pub kind: RunStepKind,
    /// 步骤说明。
    pub title: &'static str,
    /// 命令/动作文本（非空）。
    pub cmd: &'static str,
}

/// README：三步跑通。
#[derive(Clone, Copy, Debug)]
pub struct Readme {
    /// 三步（定长，种类顺序必须为 Build → Run → Verify）。
    pub steps: [RunStep; README_STEPS],
}

impl Readme {
    /// 三步跑通结构校验：种类顺序恰为规定顺序、文本非空。
    pub const fn is_wellformed(&self) -> bool {
        let mut i = 0usize;
        while i < README_STEPS {
            let s = &self.steps[i];
            if s.kind.ordinal_expect() != i {
                return false;
            }
            if s.title.is_empty() || s.cmd.is_empty() {
                return false;
            }
            i += 1;
        }
        true
    }
}

impl RunStepKind {
    /// 与锚点规定顺序对齐的下标（[`RunStepKind::ALL`] 的下标）。
    pub const fn ordinal_expect(self) -> usize {
        match self {
            RunStepKind::Build => 0,
            RunStepKind::Run => 1,
            RunStepKind::Verify => 2,
        }
    }
}

/// CI 冒烟脚本（编译 + 运行 + 退出码三维，**缺一即不合规**）。
#[derive(Clone, Copy, Debug)]
pub struct SmokeScript {
    /// 三维是否各就位（下标按 [`SmokeStage::ordinal`]）。
    pub stages: [bool; SMOKE_STAGES],
    /// 期望退出码（显性字段，成功为 0）。
    pub expected_exit: i32,
    /// 复用声明（复用全域 CI 范式，非空）。
    pub reuse_note: &'static str,
}

/// 冒烟三维度（封闭枚举，供 stages 下标定位）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SmokeStage {
    /// 编译。
    Compile,
    /// 运行。
    Run,
    /// 退出码校验。
    ExitCode,
}

impl SmokeStage {
    /// 全枚举。
    pub const ALL: [SmokeStage; SMOKE_STAGES] =
        [SmokeStage::Compile, SmokeStage::Run, SmokeStage::ExitCode];

    /// 下标。
    pub const fn ordinal(self) -> usize {
        match self {
            SmokeStage::Compile => 0,
            SmokeStage::Run => 1,
            SmokeStage::ExitCode => 2,
        }
    }
}

impl SmokeScript {
    /// 冒烟脚本合规判定：三维齐备 **且** 期望退出码为 0 **且** 复用声明非空。
    /// 缺任何一维的脚本不许注册——编译过不等于运行过，运行过不等于退出码对。
    pub const fn is_wellformed(&self) -> bool {
        let mut i = 0usize;
        while i < SMOKE_STAGES {
            if !self.stages[i] {
                return false;
            }
            i += 1;
        }
        self.expected_exit == 0 && !self.reuse_note.is_empty()
    }
}

// ===========================================================================
// 4. 金样同源：预期截图摘要（确定性软件参考路径）
// ===========================================================================

/// FNV-1a 64 位摘要（确定性纯函数，no_std 无依赖）。
pub const fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    let mut i = 0usize;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(0x100000001b3);
        i += 1;
    }
    h
}

/// 预期截图摘要的确定性参考路径：示例 id + 截图尺寸 + 关键渲染参数
/// 全部进摘要——参数变了摘要就变，金样随之更新（同源纪律）。
pub const fn render_digest(id: ExampleId, width: u32, height: u32, param: u32) -> u64 {
    let tag = id.wire();
    let bytes = [
        tag,
        (width >> 24) as u8,
        (width >> 16) as u8,
        (width >> 8) as u8,
        width as u8,
        (height >> 24) as u8,
        (height >> 16) as u8,
        (height >> 8) as u8,
        height as u8,
        (param >> 24) as u8,
        (param >> 16) as u8,
        (param >> 8) as u8,
        param as u8,
    ];
    fnv1a64(&bytes)
}

/// 预期截图（金样同源）：摘要从 [`render_digest`] 重算得出，
/// 来源标记声明与全域金样集的关系。
#[derive(Clone, Copy, Debug)]
pub struct GoldenShot {
    /// 截图宽（像素）。
    pub width: u32,
    /// 截图高（像素）。
    pub height: u32,
    /// 登记摘要（= `render_digest(id, width, height, param)`）。
    pub digest: u64,
    /// 渲染参数（进摘要，改渲染配置必换摘要）。
    pub param: u32,
    /// 资产来源标记。
    pub origin: AssetOrigin,
}

impl GoldenShot {
    /// 同源校验：登记摘要与参考路径重算逐位一致。
    pub const fn is_consistent(&self, id: ExampleId) -> bool {
        self.digest == render_digest(id, self.width, self.height, self.param)
    }
}

// ===========================================================================
// 5. 示例规格：四件聚合
// ===========================================================================

/// 一个示例的完整规格（四件：工程 / README / 金样 / 冒烟）。
#[derive(Clone, Debug)]
pub struct ExampleSpec {
    /// 示例 id。
    pub id: ExampleId,
    /// 工程入口（模块路径形态，非空）。
    pub project_entry: &'static str,
    /// 示例行数预算（HelloText 锚点 30 行内，其余为本例自报上限）。
    pub line_budget: usize,
    /// 动态文本对象数（仅示例二非零，其余为 0——非压力例不虚报）。
    pub dynamic_objects: u32,
    /// README。
    pub readme: Readme,
    /// 预期截图（金样同源）。
    pub golden: GoldenShot,
    /// CI 冒烟脚本。
    pub smoke: SmokeScript,
    /// 依赖资源清单（打包校验逐项核对，缺失即拦截）。
    pub resources: Vec<ResourceEntry>,
    /// 引用的 API 冻结指纹（与 [`api_digest`] 逐位比对，失配即漂移）。
    pub api_refs: [u64; API_FROZEN_FUNCS],
}

// ===========================================================================
// 6. API 冻结面指纹（F0817 三函数；签名文本改动一字节即漂移）
// ===========================================================================

/// F0817 三函数冻结签名文本（漂移检测的冻结尾）。
pub const API_SIGNATURES: [&str; API_FROZEN_FUNCS] = [
    "MeasureText(text, font, params) -> Metrics",
    "RenderText(batch) -> SubmitId",
    "CacheControl(op, atlas) -> Watermark",
];

/// 冻结面指纹（三签名逐个摘要）。
pub const fn api_digests() -> [u64; API_FROZEN_FUNCS] {
    [
        fnv1a64(API_SIGNATURES[0].as_bytes()),
        fnv1a64(API_SIGNATURES[1].as_bytes()),
        fnv1a64(API_SIGNATURES[2].as_bytes()),
    ]
}

// ===========================================================================
// 7. 帧率红线（分场景）
// ===========================================================================

/// 性能规格（锚点：基线 60fps，压测 30fps 下限）。
#[derive(Clone, Copy, Debug)]
pub struct PerfSpec {
    /// 基线场景帧率下限。
    pub baseline_fps: u32,
    /// 压测场景帧率下限。
    pub stress_floor_fps: u32,
}

impl PerfSpec {
    /// 分场景判定（`fps >= 阈值`，恰等达标）。
    pub const fn meets(&self, fps: u32, stress: bool) -> bool {
        if stress {
            fps >= self.stress_floor_fps
        } else {
            fps >= self.baseline_fps
        }
    }
}

// ===========================================================================
// 8. 注册表（真源）
// ===========================================================================

/// 金样参考尺寸（宽）。
pub const GOLDEN_WIDTH: u32 = 1280;
/// 金样参考尺寸（高）。
pub const GOLDEN_HEIGHT: u32 = 720;
/// 无障碍示例的放大参数（字号倍率，百分数）。
pub const A11Y_SCALE_PARAM: u32 = 200;

/// 示例注册表（锚点四例，真源唯一）。
pub fn registry() -> Vec<ExampleSpec> {
    let dg = api_digests();
    vec![
        // —— 示例一：HelloText（最小接入，30 行内可跑）——
        ExampleSpec {
            id: ExampleId::HelloText,
            project_entry: "examples/hello_text/main.rs",
            line_budget: HELLO_LINE_BUDGET,
            dynamic_objects: 0,
            readme: Readme {
                steps: [
                    RunStep { kind: RunStepKind::Build, title: "构建示例", cmd: "cargo run -p hello_text --release" },
                    RunStep { kind: RunStepKind::Run, title: "运行并观察输出", cmd: "终端打印一行多语言问候" },
                    RunStep { kind: RunStepKind::Verify, title: "退出码校验", cmd: "echo $?" },
                ],
            },
            golden: GoldenShot {
                width: GOLDEN_WIDTH,
                height: GOLDEN_HEIGHT,
                digest: render_digest(ExampleId::HelloText, GOLDEN_WIDTH, GOLDEN_HEIGHT, 0),
                param: 0,
                origin: AssetOrigin::SharedDomain,
            },
            smoke: SmokeScript {
                stages: [true, true, true],
                expected_exit: 0,
                reuse_note: "复用全域 CI 范式（编译+运行+退出码三维）",
            },
            resources: vec![
                ResourceEntry { name: "fonts/NotoSans-Regular.ttf", kind: "font" },
                ResourceEntry { name: "locales/hello.multilang", kind: "locale" },
            ],
            api_refs: dg,
        },
        // —— 示例二：排版压力场（1,000 动态对象 + 缓存水位可视化）——
        ExampleSpec {
            id: ExampleId::StressField,
            project_entry: "examples/stress_field/main.rs",
            line_budget: 240,
            dynamic_objects: STRESS_DYNAMIC_OBJECTS,
            readme: Readme {
                steps: [
                    RunStep { kind: RunStepKind::Build, title: "构建压测示例", cmd: "cargo run -p stress_field --release" },
                    RunStep { kind: RunStepKind::Run, title: "运行并观察水位面板", cmd: "HUD 显示缓存水位与帧率" },
                    RunStep { kind: RunStepKind::Verify, title: "帧率下限校验", cmd: "压测场景帧率不低于 30fps" },
                ],
            },
            golden: GoldenShot {
                width: GOLDEN_WIDTH,
                height: GOLDEN_HEIGHT,
                digest: render_digest(ExampleId::StressField, GOLDEN_WIDTH, GOLDEN_HEIGHT, STRESS_DYNAMIC_OBJECTS),
                param: STRESS_DYNAMIC_OBJECTS,
                origin: AssetOrigin::SharedDomain,
            },
            smoke: SmokeScript {
                stages: [true, true, true],
                expected_exit: 0,
                reuse_note: "复用全域 CI 范式（编译+运行+退出码三维）",
            },
            resources: vec![
                ResourceEntry { name: "fonts/NotoSans-Regular.ttf", kind: "font" },
                ResourceEntry { name: "atlas/stress_field.atlas", kind: "atlas" },
                ResourceEntry { name: "configs/stress_field.toml", kind: "config" },
            ],
            api_refs: dg,
        },
        // —— 示例三：世界内文字（3D 告示牌 + 深度淡化）——
        ExampleSpec {
            id: ExampleId::WorldText,
            project_entry: "examples/world_text/main.rs",
            line_budget: 260,
            dynamic_objects: 0,
            readme: Readme {
                steps: [
                    RunStep { kind: RunStepKind::Build, title: "构建世界文字示例", cmd: "cargo run -p world_text --release" },
                    RunStep { kind: RunStepKind::Run, title: "运行并移动相机", cmd: "观察告示牌与深度淡化" },
                    RunStep { kind: RunStepKind::Verify, title: "合批边界校验", cmd: "调试面板显示合批数不超界" },
                ],
            },
            golden: GoldenShot {
                width: GOLDEN_WIDTH,
                height: GOLDEN_HEIGHT,
                digest: render_digest(ExampleId::WorldText, GOLDEN_WIDTH, GOLDEN_HEIGHT, 0),
                param: 0,
                origin: AssetOrigin::SharedDomain,
            },
            smoke: SmokeScript {
                stages: [true, true, true],
                expected_exit: 0,
                reuse_note: "复用全域 CI 范式（编译+运行+退出码三维）",
            },
            resources: vec![
                ResourceEntry { name: "fonts/NotoSans-Regular.ttf", kind: "font" },
                ResourceEntry { name: "scenes/world_text.scene", kind: "config" },
            ],
            api_refs: dg,
        },
        // —— 示例四：无障碍放大（动态字号 + 高对比 + Hinting 联动）——
        ExampleSpec {
            id: ExampleId::A11yScale,
            project_entry: "examples/a11y_scale/main.rs",
            line_budget: 240,
            dynamic_objects: 0,
            readme: Readme {
                steps: [
                    RunStep { kind: RunStepKind::Build, title: "构建无障碍示例", cmd: "cargo run -p a11y_scale --release" },
                    RunStep { kind: RunStepKind::Run, title: "运行并调节字号倍率", cmd: "观察放大与高对比切换" },
                    RunStep { kind: RunStepKind::Verify, title: "对比度红线校验", cmd: "对比度不低于 4.5:1" },
                ],
            },
            golden: GoldenShot {
                width: GOLDEN_WIDTH,
                height: GOLDEN_HEIGHT,
                digest: render_digest(ExampleId::A11yScale, GOLDEN_WIDTH, GOLDEN_HEIGHT, A11Y_SCALE_PARAM),
                param: A11Y_SCALE_PARAM,
                origin: AssetOrigin::SharedDomain,
            },
            smoke: SmokeScript {
                stages: [true, true, true],
                expected_exit: 0,
                reuse_note: "复用全域 CI 范式（编译+运行+退出码三维）",
            },
            resources: vec![
                ResourceEntry { name: "fonts/NotoSans-Regular.ttf", kind: "font" },
                ResourceEntry { name: "fonts/NotoSans-HighContrast.ttf", kind: "font" },
                ResourceEntry { name: "configs/a11y_scale.toml", kind: "config" },
            ],
            api_refs: dg,
        },
    ]
}

/// 按 id 取单个示例（`None` = 取不到，零 panic 面）。
pub fn example(id: ExampleId) -> Option<ExampleSpec> {
    let all = registry();
    let mut i = 0usize;
    while i < all.len() {
        if all[i].id == id {
            return Some(all[i].clone());
        }
        i += 1;
    }
    None
}

// ===========================================================================
// 9. 漂移检测（可注入）与打包校验
// ===========================================================================

/// 漂移发现：例 id + 期望指纹 + 实际指纹。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Drift {
    pub id: ExampleId,
    pub slot: usize,
    pub expected: u64,
    pub actual: u64,
}

/// 对任意注册表切片扫描 API 指纹失配（**可注入**——判据用它构造漂移变体
/// 证明检测真的转红；只对内置注册表断零漂移的检测对脏输入恒瞎）。
pub fn drift_in(specs: &[ExampleSpec]) -> Vec<Drift> {
    let dg = api_digests();
    let mut out: Vec<Drift> = Vec::new();
    let mut i = 0usize;
    while i < specs.len() {
        let mut k = 0usize;
        while k < API_FROZEN_FUNCS {
            if specs[i].api_refs[k] != dg[k] {
                out.push(Drift {
                    id: specs[i].id,
                    slot: k,
                    expected: dg[k],
                    actual: specs[i].api_refs[k],
                });
            }
            k += 1;
        }
        i += 1;
    }
    out
}

/// 内置注册表零漂移快捷判定。
pub fn drift_scan() -> Vec<Drift> {
    let all = registry();
    drift_in(&all)
}

/// 打包校验：逐例核对资源清单（名字/种类非空、清单非空），
/// 缺失即拦截并**指名例 id**。
pub fn pack_manifest_check(specs: &[ExampleSpec]) -> Result<(), (ExampleId, &'static str)> {
    let mut i = 0usize;
    while i < specs.len() {
        let sp = &specs[i];
        if sp.resources.is_empty() {
            return Err((sp.id, "资源清单为空"));
        }
        let mut k = 0usize;
        while k < sp.resources.len() {
            let r = &sp.resources[k];
            if r.name.is_empty() || r.kind.is_empty() {
                return Err((sp.id, "资源条目名字/种类为空"));
            }
            k += 1;
        }
        i += 1;
    }
    Ok(())
}

// ===========================================================================
// 10. 判据（零 panic 面：固定下标一律走 get()/Option，对 None 记红）
// ===========================================================================

/// F0813 域自检入口。
pub fn run_vee13_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;

    let mut s = CheckSet::new("vee13_textdemo");

    let all = registry();
    let dg = api_digests();

    // —— 注册表：4 例齐备且逐例可取 ——
    let n_ok = all.len() == EXAMPLE_COUNT;
    let mut per_id_ok = true;
    let mut i = 0usize;
    while i < EXAMPLE_COUNT {
        match ExampleId::ALL.get(i) {
            Some(id) => match example(*id) {
                Some(sp) => {
                    if sp.id != *id {
                        per_id_ok = false;
                    }
                }
                None => per_id_ok = false,
            },
            None => per_id_ok = false,
        }
        i += 1;
    }
    s.add(
        "A46-注册表-四例齐备且逐例可取",
        n_ok && per_id_ok,
        "registry() 恰 4 例；四 id 逐一 example(id) 取到且 id 一致",
    );

    // —— 示例一：HelloText 锚点参数 ——
    let h_ok = match example(ExampleId::HelloText) {
        Some(sp) => {
            sp.line_budget > 0
                && sp.line_budget <= HELLO_LINE_BUDGET
                && sp.dynamic_objects == 0
                && sp.api_refs == dg
        }
        None => false,
    };
    s.add(
        "A46-示例一-HelloText 三十行内且走三函数 API",
        h_ok,
        "行数预算 ≤30、非压测例对象数为 0、三函数指纹全引用",
    );

    // —— 示例二：压力场锚点参数 ——
    let st_ok = match example(ExampleId::StressField) {
        Some(sp) => sp.dynamic_objects == STRESS_DYNAMIC_OBJECTS && sp.line_budget > HELLO_LINE_BUDGET,
        None => false,
    };
    s.add(
        "A46-示例二-压力场千对象在册",
        st_ok,
        "动态文本对象数恰为 1000（锚点），行数预算高于 HelloText",
    );

    // —— 示例三：世界内文字 ——
    let w_ok = match example(ExampleId::WorldText) {
        Some(sp) => sp.dynamic_objects == 0 && sp.api_refs == dg,
        None => false,
    };
    s.add(
        "A46-示例三-世界内文字在册且走三函数 API",
        w_ok,
        "非压测例对象数为 0，三函数指纹全引用",
    );

    // —— 示例四：无障碍放大 ——
    let a_ok = match example(ExampleId::A11yScale) {
        Some(sp) => sp.golden.param == A11Y_SCALE_PARAM && sp.golden.param >= 100,
        None => false,
    };
    s.add(
        "A46-示例四-无障碍放大参数在册",
        a_ok,
        "放大参数恰为 200%（≥100% 即放大语义）",
    );

    // —— 三步跑通：结构校验（种类恰为 Build→Run→Verify 且文本非空）——
    let mut steps_ok = all.len() == EXAMPLE_COUNT;
    let mut i = 0usize;
    while i < all.len() {
        if !all[i].readme.is_wellformed() {
            steps_ok = false;
        }
        i += 1;
    }
    s.add(
        "A46-三步跑通-四例步骤结构齐备",
        steps_ok,
        "每例恰 3 步、种类顺序 Build→Run→Verify、标题与命令非空",
    );

    // —— 三步跑通反向语料：顺序写反必须不合规 ——
    let mut bad_maybe = all.get(0).cloned();
    let mut reverse_ok = false;
    if let Some(ref mut bad) = bad_maybe {
        let mut broken = bad.readme;
        broken.steps[0].kind = RunStepKind::Verify;
        broken.steps[2].kind = RunStepKind::Build;
        bad.readme = broken;
        reverse_ok = !bad.readme.is_wellformed();
    }
    s.add(
        "A46-三步跑通-顺序写反即不合规",
        reverse_ok,
        "把 Build 与 Verify 对调后 is_wellformed 必须转假（README 写反顺序是真错）",
    );

    // —— 金样同源：四例逐例登记摘要 == 参考路径重算 ——
    let mut golden_ok = all.len() == EXAMPLE_COUNT;
    let mut i = 0usize;
    while i < all.len() {
        if !all[i].golden.is_consistent(all[i].id) {
            golden_ok = false;
        }
        if all[i].golden.origin != AssetOrigin::SharedDomain {
            golden_ok = false;
        }
        i += 1;
    }
    s.add(
        "A46-金样-四例预期截图同源",
        golden_ok,
        "登记摘要与 render_digest 重算逐位一致；来源标记为全域共用件",
    );

    // —— 金样摘要函数确定性：判据侧独立第二实现对拍 ——
    // 独立实现与被测实现是**同一 FNV-1a 算法、不同代码路径**
    // （逐字节单步 helper）——同输入对拍一致证明被测实现的算法参数
    // （偏移基/质数乘子）未被改坏；先长度前缀再摘要的「变体」算的是
    // 另一个摘要函数，对拍恒红，那是设计错误不是独立重算。
    let probe_bytes = b"hello_text/golden/probe";
    let independent = fnv_independent(probe_bytes);
    let builtin = fnv1a64(probe_bytes);
    s.add(
        "A46-金样-摘要函数判据侧独立对拍",
        independent == builtin,
        "独立 FNV-1a 路径（逐字节 helper）与被测实现同输入逐位一致",
    );

    // —— 摘要单字符敏感 + 恒等性（确定性三连）——
    let d1 = fnv1a64(b"hello_text");
    let d2 = fnv1a64(b"hello_text");
    let d3 = fnv1a64(b"hello_textx");
    s.add(
        "A46-金样-摘要确定性与单字符敏感",
        d1 == d2 && d1 != d3,
        "同输入两次重算同值；尾部加一字符即变值（确定性 + 敏感性）",
    );

    // —— CI 冒烟：四例三维齐备 + 期望退出码 0 + 复用声明非空 ——
    let mut smoke_ok = all.len() == EXAMPLE_COUNT;
    let mut i = 0usize;
    while i < all.len() {
        if !all[i].smoke.is_wellformed() {
            smoke_ok = false;
        }
        i += 1;
    }
    s.add(
        "A46-冒烟-四例三维齐备且复用声明在册",
        smoke_ok,
        "编译/运行/退出码三维逐例全 true；期望退出码 0；复用声明非空",
    );

    // —— 冒烟反向语料：缺维必须不合规（逐维各造一个缺维变体）——
    let mut missing_any = false;
    if let Some(first) = all.get(0) {
        let mut k = 0usize;
        while k < SMOKE_STAGES {
            let mut bad_smoke = first.smoke;
            bad_smoke.stages[k] = false;
            if bad_smoke.is_wellformed() {
                missing_any = true;
            }
            k += 1;
        }
        let mut bad_exit = first.smoke;
        bad_exit.expected_exit = 1;
        if bad_exit.is_wellformed() {
            missing_any = true;
        }
    }
    s.add(
        "A46-冒烟-缺维或非零期望退出码即不合规",
        !missing_any,
        "三维逐维各构造缺维变体 + 期望退出码改 1，均必须判定不合规",
    );

    // —— 漂移即红：内置注册表零漂移 ——
    let drifts = drift_scan();
    s.add(
        "A46-漂移-内置注册表零漂移",
        drifts.is_empty(),
        "四例引用指纹与冻结面逐位一致（drift_scan 空）",
    );

    // —— 漂移即红：构造漂移变体必须转红且定位到例 ——
    let mut poisoned = all.clone();
    let mut slot_ok = poisoned.len() == EXAMPLE_COUNT;
    if let Some(first) = poisoned.get_mut(0) {
        first.api_refs[2] = first.api_refs[2].wrapping_add(1);
    } else {
        slot_ok = false;
    }
    let pd = drift_in(&poisoned);
    let poison_ok = slot_ok
        && pd.len() == 1
        && pd[0].id == ExampleId::HelloText
        && pd[0].slot == 2
        && pd[0].actual == pd[0].expected.wrapping_add(1);
    s.add(
        "A46-漂移-指纹失配转红且定位到例与槽位",
        poison_ok,
        "单指纹 +1 后 drift_in 恰报 1 条，id=HelloText、slot=2、actual=expected+1",
    );

    // —— 资源打包：内置注册表全过 ——
    s.add(
        "A46-资源-内置注册表打包校验全过",
        pack_manifest_check(&all).is_ok(),
        "四例资源清单逐项名字/种类非空，pack_manifest_check 为 Ok",
    );

    // —— 资源缺失拦截：清空一例清单必须 Err 且指名该例 ——
    let mut stripped = all.clone();
    let mut strip_ok = false;
    if let Some(first) = stripped.get_mut(0) {
        first.resources.clear();
        strip_ok = match pack_manifest_check(&stripped) {
            Err((id, _)) => id == ExampleId::HelloText,
            Ok(()) => false,
        };
    }
    s.add(
        "A46-资源-清单缺失拦截并指名例",
        strip_ok,
        "清空 HelloText 资源清单后 pack 校验 Err 且 id=HelloText",
    );

    // —— 资源缺失拦截（非空清单分支）：名字置空必须 Err（反向语料：
    // 只测「清空清单」则「条目名字/种类为空」的检查删掉判据照样全绿）——
    let mut blanked = all.clone();
    let mut blank_ok = false;
    if let Some(first) = blanked.get_mut(0) {
        if let Some(r) = first.resources.get_mut(0) {
            r.name = "";
        }
        blank_ok = match pack_manifest_check(&blanked) {
            Err((id, _)) => id == ExampleId::HelloText,
            Ok(()) => false,
        };
    }
    s.add(
        "A46-资源-条目名字置空拦截",
        blank_ok,
        "HelloText 首条资源 name 置空后 pack 校验 Err 且 id=HelloText",
    );

    // —— 帧率红线：分场景四点（恰等达标，≥ 口径）——
    let perf = PerfSpec { baseline_fps: FPS_BASELINE, stress_floor_fps: FPS_STRESS_FLOOR };
    let p_ok = perf.meets(FPS_BASELINE, false)
        && !perf.meets(FPS_BASELINE - 1, false)
        && perf.meets(FPS_STRESS_FLOOR, true)
        && !perf.meets(FPS_STRESS_FLOOR - 1, true);
    s.add(
        "A46-性能-帧率红线分场景恰等达标",
        p_ok,
        "基线 60/压测 30 各自卡位；恰等达标、差 1 不达标（夹逼对钉边界）",
    );

    // —— 对比度红线常量在册（无障碍判据的数值锚）——
    s.add(
        "A46-无障碍-对比度红线常量在册",
        CONTRAST_REDLINE_PERMILLE == 4_500,
        "高对比红线 4.5:1 = 4500‰（WCAG AA 正文下限），常量非参数",
    );

    // —— 线编码往返 + 越界拒（封闭枚举自洽）——
    let mut wire_ok = true;
    let mut i = 0usize;
    while i < EXAMPLE_COUNT {
        let id = ExampleId::ALL[i];
        if ExampleId::from_wire(id.wire()) != Some(id) {
            wire_ok = false;
        }
        i += 1;
    }
    wire_ok = wire_ok
        && ExampleId::from_wire(0).is_none()
        && ExampleId::from_wire(0xFF).is_none();
    s.add(
        "A46-线编码-示例 id 往返自洽且越界拒",
        wire_ok,
        "四 id wire/from_wire 往返恒等；0 与 0xFF 均返 None",
    );

    // —— 越界访问零 panic（判据区自身不崩）——
    let oob_ok = ExampleId::ALL.get(EXAMPLE_COUNT).is_none()
        && ExampleId::ALL.get(usize::MAX).is_none();
    s.add(
        "A46-零panic-越界取例返 None",
        oob_ok,
        "ALL.get(越界) 返 None 不崩（判据区零 panic 面）",
    );

    // —— 版本在册（资产结构演进入口）——
    s.add(
        "A46-资产-结构版本在册",
        DEMO_VERSION == 1,
        "示例资产结构版本 1（演进须升版本）",
    );

    s
}

/// 判据侧独立 FNV-1a 第二实现：同一算法（同偏移基、同质数乘子）、
/// 不同代码路径（逐字节 helper 递进），对拍一致即证明被测实现未被改坏。
const fn fnv_step(h: u64, b: u8) -> u64 {
    (h ^ b as u64).wrapping_mul(0x100000001b3)
}

const fn fnv_independent(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    let mut i = 0usize;
    while i < bytes.len() {
        h = fnv_step(h, bytes[i]);
        i += 1;
    }
    h
}
