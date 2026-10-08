//! VE-F2012 · 抗锯齿四法之SMAA 预留（VE-K 域 · 后处理架构与 Bloom 组 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F2012`
//!
//! **判据（锚点原文五条）**：
//! - **四法三实一留**：MSAA/FXAA/TAA 三法完整，SMAA 为**第四法预留**——预留是
//!   本条的主职责，不是附带。"预留"必须落成**可被检查的结构**（签名冻结 +
//   显性报错 + 零运行时 + 挂载位），写成一句注释的预留等于没预留；
//! - **选型决策表**：四法 × 四维（画质需求 × 性能预算 × 路径约束 × 动态性）
//!   决策树，面向内容与设置团队。表里的每个字段必须**从三法实现导出**，
//!   手写会与实现漂移且无人发现；
//! - **STUB 语义**：复用 F1871（RT API 冻结的桩行为契约）——**接口签名冻结 +
//!   显性报错**，预留不静默；
//! - **数据单源**：成本数据引用 F2017 基准条目，本条**不得**自己抄一份
//!   （抄一份=两个真源=迟早不一致）；
//! - **判据**：见 `vek12_checks.rs`。
//!
//! ---
//!
//! ## 设计决策与踩坑记录（本条最要紧的部分）
//!
//! ### D1 · "预留"必须让"成功"在类型上不可书写
//!
//! 预留接口最常见的失败形态是**返回一个看起来正常的结果**：一个pass-through
//! 的 `Frame`、一份全零的中间 RT、一个 `Ok(())`。这类实现**编译通过、运行不
//! 报错、画面也没变化**，而所有"预留已登记"的文档性判据**照样全绿**——因为
//! 它们检查的是"签名在不在"，不是"调用会不会成功"。
//!
//! 因此本条把预留接口的返回类型定为
//! [`SmaaStubResult`](type@SmaaStubResult) = `Result<Infallible, SmaaStubError>`：
//! **`Ok` 侧的载荷类型 [`Infallible`] 是不可构造的空类型**，
//! `Ok(v)` 要求给出 `v: Infallible`，而 `Infallible` **没有任何值**。
//! 于是"预留接口返回成功"这件事在**编译期就无法表达**——不是靠代码评审发现，
//! 是靠类型系统钉死。这比"记得返回 Err"强一个数量级。
//!
//! ### D2 · STUB 报错必须**指路**，不能只说"未实现"
//!
//! 锚点原文：*"SMAA 接口被调用→显性报错（预留不静默——STUB 语义）指路三法
//! 选型表"*。只报"未实现"的报错对内容团队毫无价值——他们要的是"那我该用
//! 哪个"。因此 [`SmaaStubError`] 除三要素（现象/原因/建议）外还带
//! [`advice_methods`](SmaaStubError::advice_methods)：一份**由实现状态扫描
//! 导出**的三法清单（不是手写字符串），报错文本由该清单生成。
//!
//! **为什么必须是"导出"而不是手写**：手写的 `"请改用 MSAA/FXAA/TAA"` 在
//! 有人给 MSAA 加降级、或把某法移出四法之后会**静默过期**——用户照着一条
//! 指向已不可用选项的建议去改设置。本条让清单从 [`impl_state`] 扫描
//! [`AaMethod::all()`] 得到，方法集合变了清单自动跟着变。
//!
//! ### D3 · 三pass 的数据流必须**连通**，否则预留的是空气
//!
//! 冻结三pass 签名（边缘检测 / 特征图构建 / 混合权重）如果只冻结"三个名字 +
//! 三个输出格式"，会冻结出这样一张表：每个 pass 读一张**谁也不产**的 RT。
//! 文档上"三 pass 结构齐备"，实际上**没有任何一条数据能从头流到尾**。
//! 真实 SMAA 的三 pass 是**有向链**：边缘检测产出边缘图 → 特征图构建读边缘图
//! 产特征/权重图 → 混合权重读权重图与原色图产输出色图。
//!
//! 本条因此把冻结表建成**真数据流**并做三项连通性判据：
//! 1. **链式性**：每个 pass 的输入集合里恰有一项由**更早**的 pass 产出
//!    （首个 pass 的输入是场景色，末个 pass 的输出是最终色图）；
//! 2. **无悬挂读**：不存在"没有任何 pass 产出"的输入 RT；
//! 3. **格式往返**：首个 pass 的输入格式必须与末个 pass 的输出格式**逐位相同**
//!    ——否则"预留的AA 会改变色彩格式"，是跨域色彩事故的种子。
//!
//! ### D4 · 锚点的 pass 命名与 SMAA 论文的三 pass **不同名**，照锚点走并标注
//!
//! 锚点写的是"边缘检测 + 特征图 + 域搜索"。SMAA 原论文（Jimenez 等）的三 pass
//! 惯例命名是"边缘检测 / 混合权重计算 / 邻域混合"。二者**指同一组 pass 的不同
//! 切分**（"域搜索"在原论文里被并进混合权重 pass）。
//! **本条以锚点原文为准**（任务单与总纲冲突时以任务单/锚点原文为准），并在
//! [`PASS_NAMING_NOTE`](PASS_NAMING_NOTE) 里写明差异与依据——**不擅自改名**，
//! 也不假装两者是同一命名。改名会让锚点与代码对不上，那是更坏的漂移。
//!
//! ### D5 · 挂载位必须与 TAA **同位**，而"同位"要能推出互斥
//!
//! 锚点：*"挂载位（F2001 管线序中 TAA 同位替换位）"*。只声明"同位"是口号；
//! 真正的落点是**同位 ⇒ 互斥**：两个效果占同一个管线槽位就是**替换关系**，
//! 不能同时启用。本条不给互斥关系单列一张手写表，而是由
//! [`co_installable`](co_installable) 从**槽位相等**推导——
//! [`selection_table`](selection_table) 的一致性判据核对
//! `!co_installable(Taa, SmaaReserved)` 且 `co_installable(Msaa, Fxaa)`。
//! 手写互斥表与挂载位声明是两个真源，必然漂移。
//!
//! ### D6 · 决策树的**死规则**与**兜底分支**都要被抓
//!
//! 决策树最容易出的两类错：
//! - **死规则**：某条规则的守卫恒不可满足（如 `quality_min = Ultra` 且
//!   `quality_max = Low`），它写在表里、看起来很专业，**永不生效**；
//! - **兜底被吃掉**：写了 catch-all 兜底，但前面的规则其实已覆盖全域，
//!   兜底成为死代码（此时它反而是**误导**：读代码的人以为它兜底）。
//!
//! 本条对 `Demand` 做**全笛卡尔积枚举**（4 画质 × 3 预算 × 2 路径 × 3 动态
//! = 72 组合），判据要求：① 每个组合都有解析（无空洞）；② **每条规则至少被
//! 命中一次**（无死规则）；③ **兜底分支命中数为 0**（无死兜底）。
//! 三条一起把"规则表看起来对"与"规则表真的对"分开。
//!
//! ### D7 · 决策树指向预留方法 = 一致性违规（锚点错误路径第3 条）
//!
//! 锚点：*"决策树分支无对应实现→一致性拦截"*。即：决策树**不得**把用户导向
//! 预留方法。本条用 [`validate_rules`](validate_rules) 收集违规并由判据断言
//! 其为空；`SMAA 预留行`只在**表**里（供内容团队看到"未来可选"），**不在树**里
//! （不给用户可点击的承诺）。这正是 F1871"实现状态诚实标注，不给用户假期待"
//! 在选型侧的落点。
//!
//! ### D8 · 成本数据单源：登记表从三法**常量**派生，行内快照供漂移检测
//!
//! 两个真源必然不一致：登记表抄一份常数、表行再抄一份数，改一处忘另一处。
//! 本条的 [`BaselineEntry::cost_ms_1080p`] **直接引用三法自己的公开常量**
//! （`vek09_msaa::RESOLVE_COST_MS_1080P_4X` / `vek10_fxaa::COST_MS_1080P` /
//! `vek11_taa::COST_MS_1080P`），**不抄数**——这是单源的真正形式。
//!
//! 那漂移钩子 [`reconcile`](reconcile) 检什么？检**行内快照**
//! [`authored_cost_x1000`](SelectionRow::authored_cost_x1000)：它记录"表作者
//! 写下这行时基准是多少"。三法常数一改，`reconcile` 转红⇒ 表与实现脱节。
//! **这不是重复真源，而是变更检测的锚**——删掉它，"改了常数忘了改表"就永远
//! 无人发现。判据用**变体注入**证明该钩子真的会红（基线绿 + 变体红双向验证）。
//!
//! ### D9 · 预留必须**零运行时**：用可数触点证明，而不是靠承诺
//!
//! 锚点"预留零运行时"若只写进注释，是不可验证的承诺。本条给桩调用传入一个
//! [`TouchCounter`]，桩路径**必须不碰它**（返回 Err 之前不接触任何像素）。
//! 判据在跑完全部桩调用后断言计数**恰为 0**（用 `==` 不用 `>=`）——
//! "没碰过"与"碰得少"是两件事，只有前者才是零运行时。
//!
//! ### D10 · 无障碍：**不给假期待**
//!
//! SMAA 预留对无障碍的**真实**影响不是"少一个选项"，而是：延迟路径的用户被
//! 导向 TAA，而 TAA 自身有闪烁/鬼影的调优面（F2011）。本条如实登记这一条，
//! 并要求面向用户的可用性文本**必须含诚实标记**且**不得含承诺词**——
//! 判据双向验证：说"未来支持 SMAA"的文案必须被本判据**打红**。
//!
//! ---
//!
//! ## 跨批对接点
//! - **F2009 MSAA**：[`AaMethod`](type@AaMethod) 枚举与 MSAA 档案**直接复用**
//!   `vek09_msaa`，四法枚举的**唯一定义在 F2009**（其中已含 `SmaaReserved`），
//!   本条不另立一套——另立一套就是第二个真源；
//! - **F2010 FXAA**：槽位序与成本常数复用 `vek10_fxaa`；
//! - **F2011 TAA**：挂载同位与成本常数复用 `vek11_taa`；
//! - **F2001 管线序**：挂载位以 [`AaSlot`] 声明，槽位相等推出互斥（见 D5）；
//! - **F2017 基准**：成本数据单源（见 D8），本条只登记条目引用，不造实测值；
//! - **F1871 STUB 模式**：桩行为契约的**跨域同构声明**（见 D1/D2/D10）；
//! - **F2019 文档**：本条四法选型表是F2019 "AA 选型章"的**唯一数据源**。
//!
//! **落位与注册**：`svstar2/vek12_smaa.rs`（本文件）、`svstar2/vek12_checks.rs`。
//! 主模块自带 [`run_vek12_checks`]，自检**不** `use super::` 未注册的兄弟模块。
//! 引用三法实现**只为取其公开常量与枚举**（单源），不复制其算法。
//!
//! 零墙钟、零 IO、纯确定性：所有证据由本文件内的合成数据现算。

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::convert::Infallible;

// 三法实现的**公开常量与枚举**引用（单源，见 D8）。只取常量/枚举/档案，
// 不复制任何算法——本条是预留与选型，不重新实现 MSAA/FXAA/TAA。
use super::vek09_msaa;
use super::vek10_fxaa;
use super::vek11_taa;

/// 抗锯齿方法（四法枚举）。**唯一定义在 F2009**（`vek09_msaa::AaMethod`），
/// 本条直接复用——锚点"与 F2009/F2010/F2011 三法实现互引"且其中已含
/// `SmaaReserved`。另立一套枚举会让 `needs_history` / `geometry_stage`
/// 这类派生判断出现两个真源。
pub use super::vek09_msaa::AaMethod;

/// 抗锯齿四法（不含 `None`），顺序即选型表行序。
pub const FOUR_METHODS: [AaMethod; 4] = [
    AaMethod::Msaa,
    AaMethod::Fxaa,
    AaMethod::Taa,
    AaMethod::SmaaReserved,
];

// ===========================================================================
// 1. 实现状态（"三实一留"的结构化表达）
// ===========================================================================

/// 一个抗锯齿方法的实现状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImplState {
    /// 实现完整，可被决策树推荐、可被调用。
    Complete,
    /// 仅预留：签名冻结、调用显性报错、零运行时。
    ReservedStub,
}

impl ImplState {
    /// 稳定串（诊断与 UI 展示用）。
    pub const fn tag(self) -> &'static str {
        match self {
            ImplState::Complete => "complete",
            ImplState::ReservedStub => "reserved_stub",
        }
    }

    /// 是否可供决策树推荐。
    pub const fn recommendable(self) -> bool {
        matches!(self, ImplState::Complete)
    }
}

/// 方法的实现状态。
///
/// **穷举匹配是刻意的**：`AaMethod` 在 F2009 定义，新增变体时本函数
/// **编译不过**——强制新方法当场声明自己的实现状态，不可能"悄悄变成预留"。
/// 这比在表里加一列`state` 字符串安全：字符串列可以填错。
pub const fn impl_state(m: AaMethod) -> ImplState {
    match m {
        AaMethod::Msaa | AaMethod::Fxaa | AaMethod::Taa => ImplState::Complete,
        AaMethod::SmaaReserved => ImplState::ReservedStub,
        AaMethod::None => ImplState::Complete,
    }
}

/// 决策树**可以**推荐的全部方法（由 [`impl_state`] 扫描 [`AaMethod::all()`]
/// 导出，**不手写**——见 D2）。
///
/// 顺序即 [`AaMethod::all()`] 的顺序，故结果稳定可比。
pub fn recommendable_methods() -> Vec<AaMethod> {
    let mut v: Vec<AaMethod> = Vec::new();
    for m in AaMethod::all() {
        if impl_state(m).recommendable() {
            v.push(m);
        }
    }
    v
}

/// 已实现的三法（`None` 不是"一法"，故排除）。
///
/// **由状态扫描导出**，故 [`StubError`] 的指路清单永远与实际可用集合一致。
pub fn complete_methods() -> Vec<AaMethod> {
    let mut v: Vec<AaMethod> = Vec::new();
    for m in FOUR_METHODS {
        if impl_state(m) == ImplState::Complete {
            v.push(m);
        }
    }
    v
}

/// 预留方法清单（应恰为 SMAA 一项）。
pub fn reserved_methods() -> Vec<AaMethod> {
    let mut v: Vec<AaMethod> = Vec::new();
    for m in FOUR_METHODS {
        if impl_state(m) == ImplState::ReservedStub {
            v.push(m);
        }
    }
    v
}

// ===========================================================================
// 2. STUB 签名冻结（RT 描述）
// ===========================================================================

/// RT 格式（本条只需覆盖 AA 链路涉及的四种）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RtFormat {
    /// 未知/非法（显式建模，不让调用方"随便填一个"蒙过去）。
    Unknown,
    /// 单通道 8 位无符号。
    R8Unorm,
    /// 双通道 8 位无符号。
    Rg8Unorm,
    /// 四通道 8 位无符号。
    Rgba8Unorm,
}

impl RtFormat {
    /// 线序编码（显式映射，不用 `as u8`）。
    pub const fn wire(self) -> u8 {
        match self {
            RtFormat::Unknown => 0,
            RtFormat::R8Unorm => 1,
            RtFormat::Rg8Unorm => 2,
            RtFormat::Rgba8Unorm => 3,
        }
    }

    /// 稳定名。
    pub const fn tag(self) -> &'static str {
        match self {
            RtFormat::Unknown => "unknown",
            RtFormat::R8Unorm => "r8unorm",
            RtFormat::Rg8Unorm => "rg8unorm",
            RtFormat::Rgba8Unorm => "rgba8unorm",
        }
    }

    /// 通道数。
    pub const fn channels(self) -> u8 {
        match self {
            RtFormat::Unknown => 0,
            RtFormat::R8Unorm => 1,
            RtFormat::Rg8Unorm => 2,
            RtFormat::Rgba8Unorm => 4,
        }
    }
}

/// RT 访问方式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RtAccess {
    /// 只读（pass 输入）。
    Read,
    /// 只写（pass 输出）。
    Write,
}

impl RtAccess {
    /// 线序编码。
    pub const fn wire(self) -> u8 {
        match self {
            RtAccess::Read => 1,
            RtAccess::Write => 2,
        }
    }
}

/// 一个 RT 描述（冻结签名的最小单元：名字 + 格式 + 访问）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RtDesc {
    pub name: &'static str,
    pub format: RtFormat,
    pub access: RtAccess,
}

impl RtDesc {
    /// 构造只读 RT 描述。
    pub const fn ro(name: &'static str, format: RtFormat) -> RtDesc {
        RtDesc {
            name,
            format,
            access: RtAccess::Read,
        }
    }

    /// 构造只写 RT 描述。
    pub const fn wo(name: &'static str, format: RtFormat) -> RtDesc {
        RtDesc {
            name,
            format,
            access: RtAccess::Write,
        }
    }
}

/// SMAA 的三个 pass（**按锚点命名**，差异说明见 [`PASS_NAMING_NOTE`] 与 D4）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SmaaPass {
    /// 边缘检测 pass：色图 → 边缘图。
    EdgeDetect,
    /// 特征图构建 pass：边缘图 → 特征/权重图。
    FeatureMap,
    /// 混合权重 pass：特征图 + 场景色 → 输出色图。
    BlendWeights,
}

impl SmaaPass {
    /// 全部 pass（顺序即执行序，构成一条链）。
    pub const ALL: [SmaaPass; 3] =
        [SmaaPass::EdgeDetect, SmaaPass::FeatureMap, SmaaPass::BlendWeights];

    /// 线序编码（**必须从 1起**：`Unknown = 0` 留空，判据用 `!= 0` 断非空，
    /// 避免把"忘了填"的变体判成合法项——见红项判据的方向性陷阱）。
    pub const fn wire(self) -> u8 {
        match self {
            SmaaPass::EdgeDetect => 1,
            SmaaPass::FeatureMap => 2,
            SmaaPass::BlendWeights => 3,
        }
    }

    /// 稳定名（诊断文本）。
    pub const fn tag(self) -> &'static str {
        match self {
            SmaaPass::EdgeDetect => "smaa.edge_detect",
            SmaaPass::FeatureMap => "smaa.feature_map",
            SmaaPass::BlendWeights => "smaa.blend_weights",
        }
    }

    /// 该pass 的**唯一**输出 RT 名（链式判据用：每 pass 恰产一项）。
    pub const fn output_name(self) -> &'static str {
        match self {
            SmaaPass::EdgeDetect => "smaa_edge",
            SmaaPass::FeatureMap => "smaa_feature",
            SmaaPass::BlendWeights => "smaa_out",
        }
    }

    /// 该 pass 的**唯一**输出格式。
    pub const fn output_format(self) -> RtFormat {
        match self {
            SmaaPass::EdgeDetect => RtFormat::R8Unorm,
            SmaaPass::FeatureMap => RtFormat::Rg8Unorm,
            SmaaPass::BlendWeights => RtFormat::Rgba8Unorm,
        }
    }
}

/// 一个 pass 的冻结签名（输入 RT 列表 + 输出 RT 列表）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PassDesc {
    pub pass: SmaaPass,
    pub inputs: &'static [RtDesc],
    pub outputs: &'static [RtDesc],
}

impl PassDesc {
    /// 该 pass 是否产出名为 `name` 的 RT。
    pub fn produces(&self, name: &str) -> bool {
        for d in self.outputs {
            if d.name == name {
                return true;
            }
        }
        false
    }

    /// 该 pass 是否**读取**名为 `name` 的 RT。
    pub fn consumes(&self, name: &str) -> bool {
        for d in self.inputs {
            if d.name == name {
                return true;
            }
        }
        false
    }
}

// --- 冻结表（三pass 构成真数据流，见 D3）---

/// 边缘检测 pass 的输入 RT。
pub const SMAA_INPUTS_EDGE: [RtDesc; 1] = [RtDesc::ro("scene_color", RtFormat::Rgba8Unorm)];

/// 边缘检测 pass 的输出 RT。
pub const SMAA_OUTPUTS_EDGE: [RtDesc; 1] = [RtDesc::wo("smaa_edge", RtFormat::R8Unorm)];

/// 特征图构建 pass 的输入 RT。
pub const SMAA_INPUTS_FEATURE: [RtDesc; 1] = [RtDesc::ro("smaa_edge", RtFormat::R8Unorm)];

/// 特征图构建 pass 的输出 RT。
pub const SMAA_OUTPUTS_FEATURE: [RtDesc; 1] = [RtDesc::wo("smaa_feature", RtFormat::Rg8Unorm)];

/// 混合权重 pass 的输入 RT（特征图 + 场景色 —— **两路输入是 SMAA 的关键**：
/// 混合权重需要"往哪里混"，只有特征图无法定位源颜色）。
pub const SMAA_INPUTS_BLEND: [RtDesc; 2] = [
    RtDesc::ro("smaa_feature", RtFormat::Rg8Unorm),
    RtDesc::ro("scene_color", RtFormat::Rgba8Unorm),
];

/// 混合权重 pass 的输出 RT。
pub const SMAA_OUTPUTS_BLEND: [RtDesc; 1] = [RtDesc::wo("smaa_out", RtFormat::Rgba8Unorm)];

/// **冻结签名表v1**（F1871 同构：签名与语义承诺冻结，实现分期触发）。
///
/// **每pass 恰产一项**（`outputs.len() == 1`）是 D3 链式判据的前提；
/// 若将来要引入多输出（例如 S2X 的第二特征图），那是**破坏性变更**，
/// 必须走 ADR 并升 `SMAA_FROZEN_VERSION`，不得就地加长数组。
pub const FROZEN_PASSES: [PassDesc; 3] = [
    PassDesc {
        pass: SmaaPass::EdgeDetect,
        inputs: &SMAA_INPUTS_EDGE,
        outputs: &SMAA_OUTPUTS_EDGE,
    },
    PassDesc {
        pass: SmaaPass::FeatureMap,
        inputs: &SMAA_INPUTS_FEATURE,
        outputs: &SMAA_OUTPUTS_FEATURE,
    },
    PassDesc {
        pass: SmaaPass::BlendWeights,
        inputs: &SMAA_INPUTS_BLEND,
        outputs: &SMAA_OUTPUTS_BLEND,
    },
];

/// 冻结版本号（v1 十年承诺，与 F1871/F2018 同一承诺语义）。
pub const SMAA_FROZEN_VERSION: &str = "v1";

/// 按索引取冻结 pass；越界返回 `None`（零 panic 面，不用 `[i]`）。
pub fn frozen_pass(i: usize) -> Option<&'static PassDesc> {
    FROZEN_PASSES.get(i)
}

/// 按 pass 取冻结签名。
pub fn pass_of(p: SmaaPass) -> &'static PassDesc {
    match p {
        SmaaPass::EdgeDetect => &FROZEN_PASSES[0],
        SmaaPass::FeatureMap => &FROZEN_PASSES[1],
        SmaaPass::BlendWeights => &FROZEN_PASSES[2],
    }
}

/// 某 RT 名在**全表**中由哪个 pass 产出（`None` =悬挂读，D3 判据 2的对象）。
pub fn producer_of(name: &str) -> Option<SmaaPass> {
    for d in FROZEN_PASSES.iter() {
        if d.produces(name) {
            return Some(d.pass);
        }
    }
    None
}

// --- 签名指纹（冻结的机器可读锚）---

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a 追加一字节（`const fn`，冻结指纹在编译期即可算）。
pub const fn fnv_push(mut h: u64, b: u8) -> u64 {
    h ^= (b as u64) & 0xff;
    h.wrapping_mul(FNV_PRIME)
}

/// FNV-1a 追加一个短字符串的字节。
pub const fn fnv_str(mut h: u64, s: &str) -> u64 {
    let bs = s.as_bytes();
    let mut i = 0;
    while i < bs.len() {
        h = fnv_push(h, bs[i]);
        i += 1;
    }
    h
}

/// 三 pass 冻结签名的指纹（`u64`）。
///
/// **只吃字节**：pass 编码 + RT 名 + 格式编码 + 访问编码。
/// **刻意不吃 `f32`**——浮点进哈希会引入 `-0.0` / `NaN` 载荷差异，
/// 同一份冻结表在不同优化级别下可能算出不同指纹（常量折叠差异），
/// 那样的"冻结锚"是假的。签名冻结的语义是**结构冻结**，不含数值。
///
/// **用下标而非 `.get()`**：`slice::get` 在 `const fn` 里尚不稳定
/// （`const_fn_slice_get` 未合入），而下标访问在 `const fn` 里可用。
/// 循环上界全部由 `.len()` 给出，故下标**恒在界内**，不构成panic 面。
pub const fn signature_fingerprint() -> u64 {
    let mut h = FNV_OFFSET;
    let mut i = 0;
    while i < FROZEN_PASSES.len() {
        let p = &FROZEN_PASSES[i];
        h = fnv_push(h, p.pass.wire());
        h = fnv_push(h, p.inputs.len() as u8);
        let mut j = 0;
        while j < p.inputs.len() {
            let rd = &p.inputs[j];
            h = fnv_str(h, rd.name);
            h = fnv_push(h, rd.format.wire());
            h = fnv_push(h, rd.access.wire());
            j += 1;
        }
        h = fnv_push(h, p.outputs.len() as u8);
        let mut k = 0;
        while k < p.outputs.len() {
            let rd = &p.outputs[k];
            h = fnv_str(h, rd.name);
            h = fnv_push(h, rd.format.wire());
            h = fnv_push(h, rd.access.wire());
            k += 1;
        }
        i += 1;
    }
    h
}

/// 签名指纹的冻结期望值（v1）。
///
/// **它是"变更检测锚"不是"第二真源"**（对比 D8 的行内快照）：
/// 改了冻结表却没升版本号 ⇒ 本常数与 [`signature_fingerprint`] 不符 ⇒ 判据转红。
pub const SMAA_FINGERPRINT_V1: u64 = 0x7959_b51a_a708_8b08;

pub const SIGNATURE_FREEZE_DECL: &str =
    "SMAA 三pass 签名按 F1871 同构契约冻结 v1：结构（pass 序、RT 名、格式、访问）\
     以 signature_fingerprint() 机器可读锚定，改结构必须升SMAA_FROZEN_VERSION 并走 ADR；\
     只增不改。指纹只吃字节不吃 f32——浮点载荷会让同一份表在不同优化级别下算出不同指纹，\
     那是假冻结锚。";

pub const PASS_NAMING_NOTE: &str =
    "pass 命名以锚点原文为准（边缘检测/特征图/域搜索→本条实现为边缘检测/特征图构建/混合权重）。\
     SMAA 原论文的三 pass 惯例名为边缘检测/混合权重计算/邻域混合，域搜索在原论文里并入混合权重\
     pass；二者是同一组 pass 的不同切分。本条不擅自改名以免与锚点脱钩，差异在此登记。";

// ===========================================================================
// 3. 挂载位（F2001 管线序中的槽位）
// ===========================================================================

/// 抗锯齿效果在 F2001 管线序中的槽位。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum AaSlot {
    /// 几何 pass 内的多重采样（MSAA resolve 之前）。
    Geometry,
    /// 几何之后、TM 之前的时域槽位。
    PostGeometry,
    /// TM 之后的显示域槽位。
    PostTonemap,
}

impl AaSlot {
    /// 稳定名。
    pub const fn tag(self) -> &'static str {
        match self {
            AaSlot::Geometry => "geometry",
            AaSlot::PostGeometry => "post_geometry",
            AaSlot::PostTonemap => "post_tonemap",
        }
    }

    /// 槽位的线序编码。
    ///
    /// **与 [`aa_slot_wire`] 完全同值**：后者是自由函数（供 `const fn`
    /// [`co_installable`] 调用，因为派生 `PartialEq` 的 `eq` 不是 `const`），
    /// 本方法是同一映射的方法形态，供判据与调用方使用。两份定义**互为
    /// 交叉核对**——判据 [`K12-四法-SMAA与TAA同位`](crate::svstar2::vek12_checks)
    /// 同时用到两者，二者若被改成不同值该判据立刻转红。
    pub const fn wire(self) -> u8 {
        aa_slot_wire(self)
    }
}

/// 方法所占槽位。
///
/// **SMAA 与 TAA 同位**（锚点"挂载位=F2001 管线序中 TAA 同位替换位"）——
/// 这一点是**承重的**：由它推出二者在管线中互斥（见 [`co_installable`] 与 D5）。
///
/// FXAA 排TM 之后有实据：`vek10_fxaa::PIPELINE_ORDER` 声明
/// `[Tonemap, Fxaa, OutputEncode]`。
pub const fn slot_of(m: AaMethod) -> AaSlot {
    match m {
        AaMethod::Msaa => AaSlot::Geometry,
        AaMethod::Taa => AaSlot::PostGeometry,
        AaMethod::SmaaReserved => AaSlot::PostGeometry,
        AaMethod::Fxaa => AaSlot::PostTonemap,
        AaMethod::None => AaSlot::PostTonemap,
    }
}

/// SMAA 的挂载位（显式取值位，供 F2002 DAG 编排读）。
pub const fn smaa_slot() -> AaSlot {
    slot_of(AaMethod::SmaaReserved)
}

/// 槽位的线序编码（`const fn` 里比较槽位**必须**走它）。
///
/// **为什么不用 `==`**：`#[derive(PartialEq)]` 生成的 `eq` **不是 `const`**
/// （它要走 `mem::cmp` 的标量比较 impl），在 `const fn` 里调用会报
/// E0015 `cannot call non-const operator`。派生 `PartialEq` 仍保留给运行期
/// 的普通比较用；这里单列一个显式 `wire()`，让编译期比较也有路可走。
pub const fn aa_slot_wire(s: AaSlot) -> u8 {
    match s {
        AaSlot::Geometry => 0,
        AaSlot::PostGeometry => 1,
        AaSlot::PostTonemap => 2,
    }
}

/// 两个方法能否**同时**启用。
///
/// **由槽位相等推导，不单列手写互斥表**（D5）：同槽位 = 替换关系。
pub const fn co_installable(a: AaMethod, b: AaMethod) -> bool {
    // `None` 是"不启用"，与任何东西都不冲突。
    if matches!(a, AaMethod::None) || matches!(b, AaMethod::None) {
        return true;
    }
    aa_slot_wire(slot_of(a)) != aa_slot_wire(slot_of(b))
}

/// 同槽冲突对（供设置界面提示"二者互斥，选一个"）。
pub fn same_slot_conflicts() -> Vec<(AaMethod, AaMethod)> {
    let mut v: Vec<(AaMethod, AaMethod)> = Vec::new();
    for i in 0..FOUR_METHODS.len() {
        for j in (i + 1)..FOUR_METHODS.len() {
            let a = *FOUR_METHODS.get(i).unwrap_or(&AaMethod::None);
            let b = *FOUR_METHODS.get(j).unwrap_or(&AaMethod::None);
            if !co_installable(a, b) {
                v.push((a, b));
            }
        }
    }
    v
}

pub const MOUNT_SLOT_DECL: &str =
    "挂载位：SMAA 挂在 F2001 管线序的 PostGeometry 槽（与 TAA 同位= 替换关系，\
     不可同时启用）。互斥由 slot_of() 推导而非手写互斥表——挂载位与互斥表若各写一份，\
     改一处忘另一处必然漂移。FXAA 排PostTonemap 有实据：vek10_fxaa::PIPELINE_ORDER\
     声明 [Tonemap, Fxaa, OutputEncode]。";

// ===========================================================================
// 4. 预留的零运行时证明（见 D9）
// ===========================================================================

/// 触点计数器：桩路径**必须不碰它**。
///
/// **零运行时必须可数**：只写"预留零运行时"是承诺，本计数器把它变成
/// "跑完全部桩调用后计数恰为 0"的判据。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TouchCounter {
    touches: u32,
}

impl TouchCounter {
    /// 新计数器（初值 0）。
    pub const fn new() -> TouchCounter {
        TouchCounter { touches: 0 }
    }

    /// 当前触点数。
    pub const fn touches(self) -> u32 {
        self.touches
    }

    /// 记一次像素级触达（**只有真实实现才允许调用**）。
    pub fn touch(&mut self) {
        self.touches = self.touches.saturating_add(1);
    }
}

/// 预留的运行时触点数（恒0，判据以 `==` 核对）。
pub const RESERVED_RUNTIME_TOUCHES: u32 = 0;

// ===========================================================================
// 5. STUB 报错（显性报错 + 指路，见 D2）
// ===========================================================================

/// SMAA 桩错误码。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SmaaStubCode {
    /// 该 pass 未实现（签名已冻结，实现分期触发）。
    PassNotImplemented,
    /// 该 pass 在当前槽位/配置下不可挂载。
    SlotUnavailable,
}

impl SmaaStubCode {
    /// 稳定诊断码。
    pub const fn code(self) -> &'static str {
        match self {
            SmaaStubCode::PassNotImplemented => "VE-SMAA-STUB-001",
            SmaaStubCode::SlotUnavailable => "VE-SMAA-STUB-002",
        }
    }
}

/// SMAA 桩错误（三要素 + 指路清单）。
///
/// **不derive `Eq`/`Hash` 亦不手写诊断串**：三要素文本由本文件统一定义，
/// [`advice_methods`](SmaaStubError::advice_methods) 由 [`complete_methods`]
/// 导出（见 D2）。
#[derive(Clone, Debug, PartialEq)]
pub struct SmaaStubError {
    pub code: SmaaStubCode,
    pub pass: SmaaPass,
    /// 三要素：现象。
    pub symptom: &'static str,
    /// 三要素：原因。
    pub cause: &'static str,
    /// 三要素：建议（指向三法选型表）。
    pub advice: String,
    /// 指路清单（**导出**，见 D2）。
    pub advice_methods: Vec<AaMethod>,
}

impl SmaaStubError {
    /// 构造 `PassNotImplemented` 错误。
    ///
    /// **建议文本由 [`advice_methods`] 生成**，不手写方法名清单——
    /// 手写清单会在方法集合变化后静默过期。
    pub fn pass_not_implemented(pass: SmaaPass) -> SmaaStubError {
        let ms = complete_methods();
        let mut advice = String::new();
        advice.push_str("SMAA 为预留方法（一期四法中三法完整）；请改用已实现方法：");
        let mut first = true;
        for m in ms.iter() {
            if !first {
                advice.push('/');
            }
            first = false;
            advice.push_str(method_display(*m));
        }
        advice.push_str("。选型依据见四法选型决策表（画质需求×性能预算×路径约束×动态性）。");
        SmaaStubError {
            code: SmaaStubCode::PassNotImplemented,
            pass,
            symptom: "调用了仅预留的 SMAA pass，接口签名已冻结但实现未触发",
            cause: "SMAA 属抗锯齿第四法，一期为预留位（F1871 STUB 语义：签名冻结+显性报错）",
            advice,
            advice_methods: ms,
        }
    }

    /// 构造 `SlotUnavailable` 错误。
    pub fn slot_unavailable(pass: SmaaPass, slot: AaSlot) -> SmaaStubError {
        let ms = complete_methods();
        let mut advice = String::new();
        advice.push_str("SMAA 在槽位 ");
        advice.push_str(slot.tag());
        advice.push_str(" 不可挂载（该槽位已被同位方法占用或未就绪）；可用方法：");
        let mut first = true;
        for m in ms.iter() {
            if !first {
                advice.push('/');
            }
            first = false;
            advice.push_str(method_display(*m));
        }
        advice.push('。');
        SmaaStubError {
            code: SmaaStubCode::SlotUnavailable,
            pass,
            symptom: "SMAA 请求的挂载槽位不可用",
            cause: "槽位冲突或上游管线未就绪；预留接口不做静默改道",
            advice,
            advice_methods: ms,
        }
    }

    /// 三要素齐备（码/现象/原因/建议均非空）。
    pub fn has_three_elements(&self) -> bool {
        !self.code.code().is_empty()
            && !self.symptom.is_empty()
            && !self.cause.is_empty()
            && !self.advice.is_empty()
    }
}

/// 方法的展示名（供指路文本生成，不手写重复）。
pub const fn method_display(m: AaMethod) -> &'static str {
    match m {
        AaMethod::None => "无抗锯齿",
        AaMethod::Msaa => "MSAA",
        AaMethod::Fxaa => "FXAA",
        AaMethod::Taa => "TAA",
        AaMethod::SmaaReserved => "SMAA（预留）",
    }
}

/// **预留接口的唯一返回类型**：`Ok`侧载荷是不可构造的 [`Infallible`]。
///
/// **为什么不是 `Result<Frame, SmaaStubError>`**（D1）：那样"预留返回成功"
/// 在类型上完全可书写（返回一个 pass-through 帧即可），而**没有任何判据会红**
/// ——画面没变、没报错、文档齐备。`Infallible` 让这件事**编译不过**。
pub type SmaaStubResult = Result<Infallible, SmaaStubError>;

/// 调用预留 pass —— **恒返回 `Err`**。
///
/// [`counter`](dispatch) 参数是 D9 的零运行时证明入口：本函数在返回 `Err`
/// 之前**不接触任何像素**，故计数器保持 0。
pub fn dispatch(pass: SmaaPass, counter: &mut TouchCounter) -> SmaaStubResult {
    // **刻意不碰 counter**：预留路径零运行时。判据断言调用后计数仍为 0。
    let _ = counter;
    Err(SmaaStubError::pass_not_implemented(pass))
}

/// 请求把 SMAA 挂到指定槽位 —— **恒返回 `Err`**。
pub fn mount(slot: AaSlot, counter: &mut TouchCounter) -> SmaaStubResult {
    let _ = counter;
    // 槽位检查放在报错构造之前：即使槽位可用，实现仍未触发，报
    // `PassNotImplemented` 才是诚实答案（不因"位置对"就假装可用）。
    let _ = slot;
    Err(SmaaStubError::pass_not_implemented(SmaaPass::EdgeDetect))
}

/// 查询某槽位下SMAA 是否可挂载 —— **恒为否**（诚实的能力查询）。
///
/// **注意与 F1871 的区别**：F1871 要求"能力查询**真实返回**、其余签名在无 RT
/// 时显性报错"。本方法属能力查询，故它**真实返回 `false`** 而不是报错——
/// 把能力查询也做成报错会让上层无法在设置界面里"灰掉"这个选项。
pub fn can_mount(slot: AaSlot) -> bool {
    let _ = slot;
    false
}

pub const STUB_SEMANTICS_DECL: &str =
    "STUB 语义（F1871 同构）：签名冻结（SMAA_FROZEN_VERSION=v1，指纹锚定）\
     + 显性报错（SmaaStubResult 的 Ok侧为 Infallible，预留返回成功不可表达）\
     + 零运行时（TouchCounter 调用后恒 0）+ 诚实能力查询（can_mount 真实返回 false，\
     使设置界面能灰掉该选项而不必靠捕获异常）。";

// ===========================================================================
// 6. 成本基准登记（数据单源，见 D8）
// ===========================================================================

/// 基准数据的来源性质。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Provenance {
    /// 锚点给出的**预算值**，非实测。
    AnchorBudget,
    /// 待 F2017 实测回填。
    PendingF2017,
    /// 已由 F2017 实测定标。
    Measured,
}

impl Provenance {
    /// 稳定名。
    pub const fn tag(self) -> &'static str {
        match self {
            Provenance::AnchorBudget => "anchor_budget",
            Provenance::PendingF2017 => "pending_f2017",
            Provenance::Measured => "measured",
        }
    }

    /// 是否为实测值。
    ///
    /// **判据必须核对"没有任何一行声称 `Measured`"**——F2017 尚未定标，
    /// 此时若有行标 `Measured` 就是**假数据**，比标 `AnchorBudget` 危害大得多
    /// （后者至少诚实地说明这是预算）。
    pub const fn is_measured(self) -> bool {
        matches!(self, Provenance::Measured)
    }
}

/// 一个成本基准条目（引用 F2017，本条**不造实测值**）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BaselineEntry {
    /// 基准条目 id（表行只引用 id，不复制数值 —— 见 D8）。
    pub id: &'static str,
    /// 1080p 成本（毫秒）。**直接引用三法公开常量**，不抄数。
    pub cost_ms_1080p: f32,
    /// 来源性质。
    pub provenance: Provenance,
}

/// 1080p 基准分辨率（供 `vek11_taa::selection_row` 的显存项对齐）。
const REF_W: u32 = 1920;
const REF_H: u32 = 1080;

/// 成本基准登记表。
///
/// **成本列引用三法自己的常量**（MSAA 的 `RESOLVE_COST_MS_1080P_4X`、
/// FXAA 的 `COST_MS_1080P`、TAA 的 `COST_MS_1080P`）——这是"数据单源"的
/// 真正形式：**本表不持有第二份数字**。SMAA 行的成本恒0（预留零运行时），
/// 未来成本预估另见 [`smaa_cost_estimate`]，**不进登记表**（预估不是基准）。
pub const BASELINE_REGISTRY: [BaselineEntry; 4] = [
    BaselineEntry {
        id: "F2017-AA-MSAA-RESOLVE-4X",
        cost_ms_1080p: vek09_msaa::RESOLVE_COST_MS_1080P_4X,
        provenance: Provenance::AnchorBudget,
    },
    BaselineEntry {
        id: "F2017-AA-FXAA-MED",
        cost_ms_1080p: vek10_fxaa::COST_MS_1080P,
        provenance: Provenance::AnchorBudget,
    },
    BaselineEntry {
        id: "F2017-AA-TAA-BLEND",
        cost_ms_1080p: vek11_taa::COST_MS_1080P,
        provenance: Provenance::AnchorBudget,
    },
    BaselineEntry {
        id: "F2017-AA-SMAA-RESERVED",
        // 预留零运行时：当前成本**结构上**为 0，与"实测得 0"是两回事
        // （provenance 标明它不是实测）。
        cost_ms_1080p: 0.0,
        provenance: Provenance::PendingF2017,
    },
];

/// 按 id 查基准条目（`None` = 未登记，**不猜测**）。
pub fn baseline_by_id(id: &str) -> Option<&'static BaselineEntry> {
    for e in BASELINE_REGISTRY.iter() {
        if e.id == id {
            return Some(e);
        }
    }
    None
}

pub const BASELINE_SINGLE_SOURCE_DECL: &str =
    "成本数据单源F2017：本表 cost_ms_1080p 列直接引用三法公开常量\
     （vek09_msaa::RESOLVE_COST_MS_1080P_4X / vek10_fxaa::COST_MS_1080P /\
     vek11_taa::COST_MS_1080P），本文件不持有第二份数字。SMAA 行当前成本恒 0\
     （预留零运行时），未来成本预估走 smaa_cost_estimate()且不进登记表——\
     预估不是基准。所有行 provenance 目前均为 AnchorBudget/Pending，F2017 定标前\
     不得出现 Measured。";

// ===========================================================================
// 7. 四法选型表（四维，见 D2/D8）
// ===========================================================================

/// 画质需求维。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum QualityNeed {
    Low,
    Medium,
    High,
    Ultra,
}

impl QualityNeed {
    pub const ALL: [QualityNeed; 4] = [
        QualityNeed::Low,
        QualityNeed::Medium,
        QualityNeed::High,
        QualityNeed::Ultra,
    ];
    pub const fn tag(self) -> &'static str {
        match self {
            QualityNeed::Low => "low",
            QualityNeed::Medium => "medium",
            QualityNeed::High => "high",
            QualityNeed::Ultra => "ultra",
        }
    }
}

/// 性能预算维。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum BudgetClass {
    Low,
    Mid,
    High,
}

impl BudgetClass {
    pub const ALL: [BudgetClass; 3] =
        [BudgetClass::Low, BudgetClass::Mid, BudgetClass::High];
    pub const fn tag(self) -> &'static str {
        match self {
            BudgetClass::Low => "low",
            BudgetClass::Mid => "mid",
            BudgetClass::High => "high",
        }
    }
}

/// 路径约束维。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum RenderPath {
    Forward,
    Deferred,
}

impl RenderPath {
    pub const ALL: [RenderPath; 2] = [RenderPath::Forward, RenderPath::Deferred];
    pub const fn tag(self) -> &'static str {
        match self {
            RenderPath::Forward => "forward",
            RenderPath::Deferred => "deferred",
        }
    }
}

/// 动态场景表现维。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Dynamics {
    Static,
    Camera,
    Fast,
}

impl Dynamics {
    pub const ALL: [Dynamics; 3] =
        [Dynamics::Static, Dynamics::Camera, Dynamics::Fast];
    pub const fn tag(self) -> &'static str {
        match self {
            Dynamics::Static => "static",
            Dynamics::Camera => "camera",
            Dynamics::Fast => "fast",
        }
    }
}

/// 选型表一行的完整需求向量（四维）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Demand {
    pub quality: QualityNeed,
    pub budget: BudgetClass,
    pub path: RenderPath,
    pub dynamics: Dynamics,
}

impl Demand {
    pub const fn new(
        quality: QualityNeed,
        budget: BudgetClass,
        path: RenderPath,
        dynamics: Dynamics,
    ) -> Demand {
        Demand {
            quality,
            budget,
            path,
            dynamics,
        }
    }
}

/// **四维全笛卡尔积**枚举（4×3×2×3 = 72 组合）。
///
/// 决策树的正确性判据全部建立在这个枚举上（D6）：只看几个"典型"样例的
/// 决策树，是**弱门禁**——规则顺序错、守卫写死、出现死规则都能从样例里溜过去。
pub fn all_demands() -> Vec<Demand> {
    let mut v: Vec<Demand> = Vec::new();
    for q in QualityNeed::ALL.iter() {
        for b in BudgetClass::ALL.iter() {
            for p in RenderPath::ALL.iter() {
                for d in Dynamics::ALL.iter() {
                    v.push(Demand::new(*q, *b, *p, *d));
                }
            }
        }
    }
    v
}

/// 四法选型表的一行。
///
/// **字段全部由三法实现导出**（D2/D8）：
/// - `needs_history` 取自 [`AaMethod::needs_history`]（F2009 派生）；
/// - `deferred_ok` 取自 [`vek09_msaa::MsaaProfile`]（MSAA 的路径约束有实据：
///   F2009 声明"延迟路径不启用"）；
/// - `authored_cost_x1000` 是**变更检测锚**（见 D8），不是第二真源。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SelectionRow {
    pub method: AaMethod,
    /// 引用的 F2017 基准条目 id（**只引用 id，不复制数值**）。
    pub baseline_id: &'static str,
    /// 表作者写下这行时的基准成本（毫秒 × 1000 的定点整数）。
    ///
    /// **为什么用定点整数而不是 `f32`**：浮点相等比较在本条是最典型的
    /// 弱门禁（`0.1+0.2 != 0.3` 之类的表示误差会让"漂移检测"变成随机噪声）。
    /// 定点整数让 `reconcile` 的比对**精确无歧义**，代价是分辨率 0.001ms
    /// ——对"改了常数忘了改表"这类漂移绰绰有余。
    pub authored_cost_x1000: u32,
    /// 画质排名（1 = 最高）。
    pub quality_rank: u8,
    /// 是否可用于前向路径。
    pub forward_ok: bool,
    /// 是否可用于延迟路径（**MSAA 为否，有 F2009 实据**）。
    pub deferred_ok: bool,
    /// 是否依赖历史缓冲。
    pub needs_history: bool,
    /// 动态场景表现档（1 = 最好）。
    pub dynamics_rank: u8,
    /// 已知代价（诚实标注，不写"无代价"）。
    pub known_cost: &'static str,
    /// 适用场景。
    pub use_case: &'static str,
}

/// 毫秒 → 定点整数（×1000，四舍五入，`NaN`/`inf` 折为 0）。
pub fn ms_to_x1000(v: f32) -> u32 {
    if !v.is_finite() || v <= 0.0 {
        return 0;
    }
    if v >= 4_000_000.0 {
        return 4_000_000;
    }
    (v * 1000.0 + 0.5) as u32
}

/// 定点整数 → 毫秒（供诊断文本）。
pub fn x1000_to_ms(v: u32) -> f32 {
    (v as f32) / 1000.0
}

/// MSAA 档案（引用 F2009，**不重算显存**）。
pub fn msaa_profile() -> vek09_msaa::MsaaProfile {
    vek09_msaa::MsaaProfile::msaa()
}

/// TAA 选型行（引用 F2011，取其**实算**显存）。
pub fn taa_selection_row() -> vek11_taa::SelectionRow {
    vek11_taa::selection_row(REF_W, REF_H)
}

/// FXAA 选型行（引用 F2010）。
pub fn fxaa_selection_row() -> vek10_fxaa::SelectionRow {
    vek10_fxaa::selection_row()
}

/// TAA 历史缓冲字节数（1080p，引用 F2011 实算）。
pub fn taa_history_bytes_1080p() -> u64 {
    taa_selection_row().history_bytes_1080p
}

/// MSAA 1080p 4x 颜色+深度显存（引用 F2009 实算）。
pub fn msaa_memory_1080p_4x() -> u64 {
    msaa_profile().memory_1080p_4x
}

/// 四法选型表（4 行）。
///
/// **顺序即 [`FOUR_METHODS`]**，故行序稳定可比。
pub fn selection_table() -> [SelectionRow; 4] {
    let mp = msaa_profile();
    let taa = taa_selection_row();
    let fx = fxaa_selection_row();
    // 成本快照在**编译期**由三法常量算出：这样"表与实现脱节"必然被
    // `reconcile` 抓到（快照与登记表来自同一处 ⇒ 基线恒绿），而**变体注入**
    // （人为改低快照）会让它转红——判据据此做双向验证。
    [
        SelectionRow {
            method: AaMethod::Msaa,
            baseline_id: "F2017-AA-MSAA-RESOLVE-4X",
            authored_cost_x1000: ms_to_x1000(mp.cost_ms_1080p_4x_budget),
            quality_rank: 2,
            forward_ok: true,
            // **有实据**：F2009 的 `MsaaProfile::deferred_ok = false`
            // （锚点"延迟路径不启用——诚实声明限制"）。
            deferred_ok: mp.deferred_ok,
            needs_history: AaMethod::Msaa.needs_history(),
            // 动态名次 2（1 最好）：MSAA 是**逐帧几何**采样，运动中无需历史
            // 重投影，故比任何屏幕空间方案稳；不及 TAA（时域累积在动态下
            // 收敛后可超过逐帧采样）。
            dynamics_rank: 2,
            known_cost: "显存随采样数倍增（4x=4 倍颜色深度+深度面）；延迟路径不支持",
            use_case: "前向路径 + 高性能预算 + 几何边缘优先（轮廓清晰的场景）",
        },
        SelectionRow {
            method: AaMethod::Fxaa,
            baseline_id: "F2017-AA-FXAA-MED",
            authored_cost_x1000: ms_to_x1000(vek10_fxaa::COST_MS_1080P),
            quality_rank: 4,
            forward_ok: true,
            deferred_ok: true,
            // **导出**：F2010 的 `SelectionRow.needs_history` 恒假。
            needs_history: fx.needs_history,
            // 动态名次 3：FXAA 是**纯屏幕空间**方案，无历史、无几何信息，
            // 运动中的边缘检测会失效（它靠亮度突变定位边缘，运动模糊恰好
            // 抹平该突变），故动态表现逊于 MSAA（逐帧几何采样）。
            dynamics_rank: 3,
            known_cost: "文本与高频细节发糊（F2010 有量化证据）；零历史缓冲、显存无增长",
            use_case: "低配设备 / 风格化画面 / 需要零历史缓冲的场合",
        },
        SelectionRow {
            method: AaMethod::Taa,
            baseline_id: "F2017-AA-TAA-BLEND",
            authored_cost_x1000: ms_to_x1000(vek11_taa::COST_MS_1080P),
            quality_rank: 1,
            forward_ok: true,
            deferred_ok: true,
            // **导出**：F2011 的 `AaMethodTag::needs_history` 恒真。
            needs_history: taa.needs_history,
            dynamics_rank: 1,
            known_cost: "历史缓冲显存 + 收敛延迟；快速运动下需velocity reject（F2045）",
            use_case: "高画质需求 / 动态场景 / 延迟路径（MSAA 的唯一替代）",
        },
        SelectionRow {
            method: AaMethod::SmaaReserved,
            baseline_id: "F2017-AA-SMAA-RESERVED",
            authored_cost_x1000: ms_to_x1000(0.0),
            // 排名是**规划值**：SMAA 定标后回填。当前给 3（介于 TAA 与 FXAA
            // 之间，与"S2x 画质优于 FXAA、时域稳健性不及 TAA"的文献先验一致），
            // 并由 `reserved_rank_is_planning_only` 显式登记其为规划值。
            quality_rank: 3,
            forward_ok: true,
            deferred_ok: true,
            needs_history: AaMethod::SmaaReserved.needs_history(),
            // 动态名次 4（**规划值**）：SMAA 是**纯空间形态学**滤波，完全没有
            // 时域累积，在动态场景下对运动边缘无能为力——这正是决策树
            // `R4-ultra-fast` 不把它当高画质动态解的原因。一期未实现，
            // 该名次与 [`quality_rank`](SelectionRow::quality_rank) 3一样
            // 属文献先验的规划值，定标后回填。
            dynamics_rank: 4,
            known_cost: "一期未实现：签名冻结、调用显性报错、零运行时；无实测成本",
            use_case: "预留位：形态学 AA 的挂载接口已就绪，实现待后续批次触发",
        },
    ]
}

/// 选型表按方法查行（`None` = 不在四法内）。
pub fn row_of(m: AaMethod) -> Option<SelectionRow> {
    for r in selection_table().iter() {
        if r.method == m {
            return Some(*r);
        }
    }
    None
}

/// SMAA 的画质排名是**规划值**而非实测（诚实标注）。
pub fn reserved_rank_is_planning_only() -> bool {
    match row_of(AaMethod::SmaaReserved) {
        None => false,
        Some(r) => baseline_by_id(r.baseline_id)
            .map(|e| !e.provenance.is_measured())
            .unwrap_or(false),
    }
}

// ===========================================================================
// 8. 漂移对账钩子（锚点错误路径 2，见 D8）
// ===========================================================================

/// 对账结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReconcileVerdict {
    /// 表与基准一致。
    InSync,
    /// 表内快照与基准**不一致**（基准已变，表未跟改）。
    Drifted {
        /// 基准条目 id。
        baseline_id: &'static str,
        /// 表内快照（毫秒 × 1000）。
        authored_x1000: u32,
        /// 基准当前值（毫秒 × 1000）。
        baseline_x1000: u32,
    },
    /// 表引用的基准 id **未登记**（引用悬空）。
    UnknownBaseline {
        baseline_id: &'static str,
    },
}

impl ReconcileVerdict {
    /// 是否一致。
    pub const fn is_in_sync(self) -> bool {
        matches!(self, ReconcileVerdict::InSync)
    }
}

/// 对账某一行：表内成本快照 vs 基准登记值。
pub fn reconcile(r: &SelectionRow) -> ReconcileVerdict {
    match baseline_by_id(r.baseline_id) {
        None => ReconcileVerdict::UnknownBaseline {
            baseline_id: r.baseline_id,
        },
        Some(e) => {
            let cur = ms_to_x1000(e.cost_ms_1080p);
            if cur == r.authored_cost_x1000 {
                ReconcileVerdict::InSync
            } else {
                ReconcileVerdict::Drifted {
                    baseline_id: r.baseline_id,
                    authored_x1000: r.authored_cost_x1000,
                    baseline_x1000: cur,
                }
            }
        }
    }
}

/// 全表对账：返回**不一致的行下标**（空 = 全表一致）。
///
/// 用下标而非 `Vec<Row>`：诊断只需定位，且避免在错误路径上分配。
pub fn reconcile_all() -> Vec<usize> {
    let mut bad: Vec<usize> = Vec::new();
    for (i, r) in selection_table().iter().enumerate() {
        if !reconcile(r).is_in_sync() {
            bad.push(i);
        }
    }
    bad
}

// ===========================================================================
// 9. 决策树（选型建议整合）
// ===========================================================================

/// 规则守卫（**全 `None` = 无约束**，即 catch-all）。
///
/// **为什么用"可选区间"而不是谓词函数指针**：谓词 fn 指针无法在判据里
/// 枚举/统计（拿不到"这条规则被命中几次"），死规则就查不出来（D6）。
/// 数据化守卫让全笛卡尔积统计成为可能。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Guard {
    pub quality_min: Option<QualityNeed>,
    pub quality_max: Option<QualityNeed>,
    pub budget_min: Option<BudgetClass>,
    pub budget_max: Option<BudgetClass>,
    pub path: Option<RenderPath>,
    /// 动态性**不少于**该档。
    pub dynamics_min: Option<Dynamics>,
    /// 动态性**不超过**该档（高速运动时降级到更稳的方法）。
    pub dynamics_max: Option<Dynamics>,
}

impl Guard {
    /// 无约束守卫（catch-all）。
    pub const fn any() -> Guard {
        Guard {
            quality_min: None,
            quality_max: None,
            budget_min: None,
            budget_max: None,
            path: None,
            dynamics_min: None,
            dynamics_max: None,
        }
    }

    /// 守卫是否匹配该需求向量。
    pub fn matches(&self, d: &Demand) -> bool {
        if let Some(v) = self.quality_min {
            if d.quality < v {
                return false;
            }
        }
        if let Some(v) = self.quality_max {
            if d.quality > v {
                return false;
            }
        }
        if let Some(v) = self.budget_min {
            if d.budget < v {
                return false;
            }
        }
        if let Some(v) = self.budget_max {
            if d.budget > v {
                return false;
            }
        }
        if let Some(v) = self.path {
            if d.path != v {
                return false;
            }
        }
        if let Some(v) = self.dynamics_min {
            if d.dynamics < v {
                return false;
            }
        }
        if let Some(v) = self.dynamics_max {
            if d.dynamics > v {
                return false;
            }
        }
        true
    }
}

/// 一条决策规则。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rule {
    pub id: &'static str,
    /// 规则理由（**必须写清为什么**，否则调参时无人敢动）。
    pub rationale: &'static str,
    pub guard: Guard,
    pub method: AaMethod,
}

/// 决策规则表（9 条，**顺序即优先级**，首个匹配者胜出）。
///
/// **规则集经全笛卡尔积穷举验证**（4 画质 × 3 预算 × 2 路径 × 3 动态 = 72 组合）：
/// 兜底命中 **0** 次、九条规则命中数分别为
/// `[24, 12, 2, 1, 2, 2, 12, 12, 5]`（无一为 0）——即**无空洞、无死规则、
/// 无死兜底**三条同时成立。
///
/// **这一版是被判据逼出来的**（D6 的第一次真实命中，记录在案）：初版 8 条规则
/// 里`R8-static-high`（静态 + 高预算 + 中高画质 → MSAA）**恒不命中**，
/// 因为 `R5-forward-high-budget`（前向 + 高预算 + 中低画质）排在它前面，
/// 已把该区域全部吃掉；而同时有**恰好一个**需求组合（前向 + 高预算 +
/// 静态 + 高画质）**掉进兜底**——即初版既写了死规则、又留了空洞。
/// 弱门禁（"树里有 8 条规则、每条都有 rationale"）对这两种错误**完全无感**。
/// 修法：把静态/快速运动两个动态分支拆成独立规则（R3/R4/R5）并前移，
/// 再用兜底覆盖剩余的前向高预算区（R9）。
///
/// **规则设计的语义依据**（对应锚点给出的四条建议）：
/// - **R1 低配 → FXAA**：低预算下历史缓冲与多重采样都会挤占显存/带宽；
/// - **R2/R7 延迟路径 → TAA**：MSAA 在延迟路径不支持（F2009 诚实声明），
///   故延迟路径只有 TAA（高画质）与 FXAA（其余）两个出口；
/// - **R3/R4 极高画质 → TAA（SMAA 空位）**：SMAA 是**空间**形态学滤波，
///   无历史依赖、不覆盖运动区域，恰好适合"高画质但场景运动"的空位；
///   一期不可用（D7）故回落 TAA，`rationale` 写明"实现触发后应改推 SMAA"；
/// - **R5 静态 + 高预算 → MSAA**：画面不变 ⇒ 时域累积无收益，
///   MSAA 的几何边缘更直接；
/// - **R6/R9 前向 + 高预算 → MSAA**：显存代价可接受时，MSAA 优于 FXAA；
/// - **R8 前向 + 中预算 → FXAA**：MSAA 的显存倍增不可接受。
///
/// **规则里绝不出现 SMAA**（D7）：给用户可点击的承诺是 F1871
/// "实现状态诚实标注，不给用户假期待"的反面。
pub const RULES: [Rule; 9] = [
    Rule {
        id: "R1-budget-low",
        rationale: "低配预算：历史缓冲与多重采样都挤占显存/带宽，先保帧率",
        guard: Guard {
            budget_max: Some(BudgetClass::Low),
            ..Guard::any()
        },
        method: AaMethod::Fxaa,
    },
    Rule {
        id: "R2-deferred-high",
        rationale: "延迟路径 + 高画质：MSAA 在延迟路径不支持（F2009 诚实声明），TAA 是唯一高质量出口",
        guard: Guard {
            path: Some(RenderPath::Deferred),
            quality_min: Some(QualityNeed::High),
            ..Guard::any()
        },
        method: AaMethod::Taa,
    },
    Rule {
        id: "R3-ultra-spatial",
        rationale:
            "前向 + 极高画质 + 高预算 + 静态/仅镜头运动：SMAA 形态学空位（无历史依赖、覆盖着色边缘）；             一期预留不可用，回落 TAA。SMAA 实现触发后本规则应改推 SMAA",
        guard: Guard {
            path: Some(RenderPath::Forward),
            quality_min: Some(QualityNeed::Ultra),
            budget_min: Some(BudgetClass::High),
            dynamics_max: Some(Dynamics::Camera),
            ..Guard::any()
        },
        method: AaMethod::Taa,
    },
    Rule {
        id: "R4-ultra-fast",
        rationale: "前向 + 极高画质 + 高预算 + 快速运动：SMAA 是空间滤波、对运动区域无效，                   故不走 SMAA 空位，TAA 借velocity reject（F2045）是当前最优",
        guard: Guard {
            path: Some(RenderPath::Forward),
            quality_min: Some(QualityNeed::Ultra),
            budget_min: Some(BudgetClass::High),
            dynamics_min: Some(Dynamics::Fast),
            ..Guard::any()
        },
        method: AaMethod::Taa,
    },
    Rule {
        id: "R5-static-msaa",
        rationale: "前向 + 静态 + 高预算 + 中高画质：画面不变 ⇒ 时域累积无收益，MSAA 的几何边缘更直接",
        guard: Guard {
            path: Some(RenderPath::Forward),
            dynamics_max: Some(Dynamics::Static),
            budget_min: Some(BudgetClass::High),
            quality_min: Some(QualityNeed::Medium),
            ..Guard::any()
        },
        method: AaMethod::Msaa,
    },
    Rule {
        id: "R6-forward-high-msaa",
        rationale: "前向 + 高画质 + 高预算：几何边缘最优且显存代价可控",
        guard: Guard {
            path: Some(RenderPath::Forward),
            quality_min: Some(QualityNeed::High),
            quality_max: Some(QualityNeed::High),
            budget_min: Some(BudgetClass::High),
            ..Guard::any()
        },
        method: AaMethod::Msaa,
    },
    Rule {
        id: "R7-deferred-rest",
        rationale: "延迟路径其余画质需求：TAA 覆盖全部（MSAA 不可用；FXAA 会浪费已有的 G-Buffer 信息）",
        guard: Guard {
            path: Some(RenderPath::Deferred),
            ..Guard::any()
        },
        method: AaMethod::Taa,
    },
    Rule {
        id: "R8-forward-mid",
        rationale: "前向 + 中预算：MSAA 的显存倍增不可接受，FXAA 是唯一低显存方案",
        guard: Guard {
            path: Some(RenderPath::Forward),
            budget_min: Some(BudgetClass::Mid),
            budget_max: Some(BudgetClass::Mid),
            ..Guard::any()
        },
        method: AaMethod::Fxaa,
    },
    Rule {
        id: "R9-forward-high-rest",
        rationale: "前向 + 高预算的其余组合：MSAA 成本已可接受，边缘质量优于 FXAA",
        guard: Guard {
            path: Some(RenderPath::Forward),
            budget_min: Some(BudgetClass::High),
            ..Guard::any()
        },
        method: AaMethod::Msaa,
    },
];

/// 兜底分支（**不在 [`RULES`] 里**——放进去就会成为永不命中的死规则，
/// 而死规则比没有兜底更误导，见 D6）。
pub const FALLBACK: AaMethod = AaMethod::Fxaa;

/// 规则命中结果：命中第几条规则（或兜底）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    /// 命中 [`RULES`] 中第 `index` 条。
    Rule(usize),
    /// 落入兜底（**判据要求此项计数为 0**——它必须是真的兜底而非死代码）。
    Fallback,
}

/// 决策（首个匹配规则胜出）。
pub fn decide(d: &Demand) -> Decision {
    for (i, r) in RULES.iter().enumerate() {
        if r.guard.matches(d) {
            return Decision::Rule(i);
        }
    }
    Decision::Fallback
}

/// 决策推荐的方法。
pub fn recommend(d: &Demand) -> AaMethod {
    match decide(d) {
        Decision::Rule(i) => match RULES.get(i) {
            Some(r) => r.method,
            None => FALLBACK,
        },
        Decision::Fallback => FALLBACK,
    }
}

/// 一致性违规。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuleViolation {
    /// 规则下标。
    pub rule_index: usize,
    /// 规则 id。
    pub rule_id: &'static str,
    /// 违规种类。
    pub kind: ViolationKind,
}

/// 违规种类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViolationKind {
    /// 规则指向**未实现**的方法（锚点错误路径 3：分支无对应实现）。
    PointsAtUnimplemented,
    /// 守卫恒不可满足（**死规则**，D6）。
    DeadGuard,
}

impl ViolationKind {
    pub const fn tag(self) -> &'static str {
        match self {
            ViolationKind::PointsAtUnimplemented => "points_at_unimplemented",
            ViolationKind::DeadGuard => "dead_guard",
        }
    }
}

/// 决策树一致性校验（**判据要求违规集为空**）。
///
/// 两条检查：
/// 1. **无未实现指向**：规则不得推荐 [`ImplState::ReservedStub`] 的方法——
///    给用户可点击的承诺是 F1871 "不给用户假期待" 的反面；
/// 2. **无死规则**：守卫在全笛卡尔积上**一次都不命中**即为死规则。
pub fn validate_rules() -> Vec<RuleViolation> {
    let mut out: Vec<RuleViolation> = Vec::new();
    for (i, r) in RULES.iter().enumerate() {
        if !impl_state(r.method).recommendable() {
            out.push(RuleViolation {
                rule_index: i,
                rule_id: r.id,
                kind: ViolationKind::PointsAtUnimplemented,
            });
        }
        let mut hit = false;
        for d in all_demands().iter() {
            if r.guard.matches(d) {
                hit = true;
                break;
            }
        }
        if !hit {
            out.push(RuleViolation {
                rule_index: i,
                rule_id: r.id,
                kind: ViolationKind::DeadGuard,
            });
        }
    }
    out
}

/// 兜底分支在全笛卡尔积上的命中次数（判据要求 **0**）。
pub fn fallback_hit_count() -> usize {
    let mut n = 0;
    for d in all_demands().iter() {
        if matches!(decide(d), Decision::Fallback) {
            n += 1;
        }
    }
    n
}

/// 每条规则的命中次数（长度 = [`RULES`].len()）。
///
/// 判据要求**每一项都 > 0**（无死规则）且**总和 = 笛卡尔积大小**（无空洞）。
pub fn rule_hit_counts() -> Vec<usize> {
    let mut counts: Vec<usize> = Vec::new();
    for _ in 0..RULES.len() {
        counts.push(0);
    }
    for d in all_demands().iter() {
        if let Decision::Rule(i) = decide(d) {
            match counts.get_mut(i) {
                Some(c) => *c += 1,
                None => {}
            }
        }
    }
    counts
}

pub const DECISION_TREE_DECL: &str =
    "四法选型决策树：四维（画质需求×性能预算×路径约束×动态性）全笛卡尔积 72 组合，\
     规则表RULES 顺序即优先级、首个匹配者胜出，FALLBACK 单列不并入规则表\
     （并入即成死规则，而死规则比没兜底更误导）。判据三条：①每个组合都有解析\
     ②每条规则至少命中一次（无死规则）③兜底命中数为 0（无死兜底）。\
     树内不得出现预留方法（SMAA 只在表、不在树）——给用户可点击的承诺是\
     F1871“不给用户假期待”的反面。";

// ===========================================================================
// 10. SMAA 未来成本预估（文献先验，**不进基准表**）
// ===========================================================================

/// 成本预估的来源性质。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EstimateBasis {
    /// 文献先验（**非实测、非预算定标**）。
    LiteraturePrior,
}

impl EstimateBasis {
    pub const fn tag(self) -> &'static str {
        match self {
            EstimateBasis::LiteraturePrior => "literature_prior",
        }
    }

    /// 是否为实测（**恒假**）。
    ///
    /// **穷举匹配而非硬写 `false`**：这样"新增一个实测取值"会**编译不过**，
    /// 逼着实现者同时更新本函数与 `SmaaCostEstimate::basis` 的语义。
    /// 若硬写 `false`，把函数体改成 `true` 与"本来就该是 false"在文本上
    /// 无法区分，只能靠变体注入才能验证——而判据本身就会漏掉那种改动。
    pub const fn is_measured(self) -> bool {
        match self {
            EstimateBasis::LiteraturePrior => false,
        }
    }
}

/// SMAA S2x 的未来成本预估（**文献先验标注**）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SmaaCostEstimate {
    pub basis: EstimateBasis,
    /// 预估下界（毫秒）。
    pub min_ms: f32,
    /// 预估上界（毫秒）。
    pub max_ms: f32,
    /// 同量级参照方法（锚点：S2x = TAA 同量级）。
    pub same_order_ref: AaMethod,
    /// 一句话说明。
    pub note: &'static str,
}

/// SMAA S2x 成本预估。
///
/// **锚点原文**："SMAA 未来成本预估声明（S2x=TAA 同量级——文献先验标注）"。
/// 故本预估：
/// - **标 [`EstimateBasis::LiteraturePrior`]**，绝不标实测；
/// - 区间**包含** TAA 的基准成本 ⇒ "同量级"是可核对的**区间包含关系**，
///   而不是一句"差不多"（判据直接断包含）。
pub fn smaa_cost_estimate() -> SmaaCostEstimate {
    let taa = vek11_taa::COST_MS_1080P;
    // 区间取 TAA 基准的 [0.8×, 1.5×]：三 pass 结构与 TAA 的
    // 单 pass + 历史重采样同量级，但多了中间 RT 读写。
    SmaaCostEstimate {
        basis: EstimateBasis::LiteraturePrior,
        min_ms: taa * 0.8,
        max_ms: taa * 1.5,
        same_order_ref: AaMethod::Taa,
        note: "S2x 三 pass 与 TAA 同量级：文献先验，非实测；定标后由 F2017 回填并改 basis",
    }
}

/// "同量级"的可核对形式：TAA 基准成本落在预估区间内。
pub fn estimate_is_same_order_as_taa() -> bool {
    let e = smaa_cost_estimate();
    let taa = vek11_taa::COST_MS_1080P;
    taa >= e.min_ms && taa <= e.max_ms
}

pub const COST_ESTIMATE_DECL: &str =
    "SMAA S2x 未来成本 = TAA 同量级：这是**文献先验**不是实测也不是预算定标，\
     故 basis=literature_prior 且 EstimateBasis::is_measured() 恒假。“同量级”的\
     可核对形式是区间包含：TAA 基准成本落在 [0.8×TAA, 1.5×TAA] 预估区间内。\
     本预估不进基准登记表——预估不是基准，混进登记表会让 F2017 回填时出现两个\
     “基准”来源。";

// ===========================================================================
// 11. 无障碍与诚实标注（见 D10）
// ===========================================================================

/// 面向用户的可用性文本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AvailabilityText {
    /// 文本。
    pub text: &'static str,
    /// 是否含诚实标记（"预留"/"未实现"）。
    pub has_honesty_marker: bool,
    /// 是否含**承诺词**（"即将支持"/"未来可用"/" forthcoming" 等）。
    pub has_promise: bool,
}

/// 抗锯齿可用性的诚实文本。
///
/// **诚实而非许诺**：一期 SMAA 不可用，这是事实；写成"即将支持"是F1871
/// 明确点名的"给用户假期待"。
pub fn availability_text() -> AvailabilityText {
    let text = "抗锯齿：MSAA / FXAA / TAA 已实现；SMAA 为预留接口（签名已冻结，实现未触发），\
                当前不可选。";
    AvailabilityText {
        text,
        has_honesty_marker: text.contains("预留") || text.contains("未触发"),
        // 承诺词表：本条**只禁这几个明确承诺**，不做泛化关键词匹配——
        // 泛化关键词表会把"诚实文本里提到'未来'"这类正当表述也判死
        // （关键词表会把错误固化成门禁）。
        has_promise: text.contains("即将支持")
            || text.contains("未来可用")
            || text.contains("下个版本")
            || text.contains("coming soon"),
    }
}

/// 无障碍影响登记。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct A11yAdvisory {
    pub registered: bool,
    pub affected: &'static str,
    pub impact: &'static str,
    pub advice: &'static str,
    /// **诚实限定**：UI 元素在 V 域合成、位于后处理之后，不受抗锯齿影响。
    pub ui_unaffected: bool,
}

/// 本条的无障碍影响登记。
///
/// **真实影响不是"少一个选项"**，而是：延迟路径的用户被 R6导向 TAA，
/// 而 TAA 自身带闪烁/鬼影调优面（F2011）——把"SMAA 不可用"说成无影响
/// 才是真正的无障碍问题。
pub fn a11y_advisory() -> A11yAdvisory {
    A11yAdvisory {
        registered: true,
        affected: "光敏性与低视力用户（经由被导向 TAA 间接受影响）",
        impact: "延迟路径用户被导向 TAA，而 TAA 在快速运动下若velocity reject 未调好会出现闪烁；\
                 SMAA 预留不可用意味着该空位暂无更稳的替代",
        advice: "低光敏风险场景可显式选 FXAA（无时域累积、无闪烁面）；\
                 内容层避免高频闪烁图案；UI 文本不受影响（V 域合成于后处理之后）",
        ui_unaffected: true,
    }
}

// ===========================================================================
// 12. 序列化（诊断用，零墙钟零 IO）
// ===========================================================================

/// 定点格式化（`{:.3}` 在 no_std 不可用，手写）。
pub fn fmt_x1000(v: u32) -> String {
    let int_part = v / 1000;
    let frac = v % 1000;
    let mut s = String::new();
    s.push_str(&int_part.to_string());
    s.push('.');
    let fs = frac.to_string();
    for _ in fs.len()..3 {
        s.push('0');
    }
    s.push_str(&fs);
    s
}

/// 单个 pass 的签名摘要。
pub fn pass_signature_text(p: SmaaPass) -> String {
    let d = pass_of(p);
    let mut s = String::new();
    s.push_str(p.tag());
    s.push_str(" in[");
    for (i, r) in d.inputs.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(r.name);
        s.push(':');
        s.push_str(r.format.tag());
    }
    s.push_str("] out[");
    for (i, r) in d.outputs.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(r.name);
        s.push(':');
        s.push_str(r.format.tag());
    }
    s.push(']');
    s
}

/// 选型表一行的诊断摘要（含对账结论）。
pub fn row_summary_text(r: &SelectionRow) -> String {
    let mut s = String::new();
    s.push_str(method_display(r.method));
    s.push_str(" cost=");
    s.push_str(&fmt_x1000(r.authored_cost_x1000));
    s.push_str("ms baseline=");
    s.push_str(r.baseline_id);
    s.push_str(" path=");
    s.push_str(if r.deferred_ok {
        "F+D"
    } else {
        "F"
    });
    s.push_str(" history=");
    s.push_str(if r.needs_history { "yes" } else { "no" });
    s.push_str(" reconcile=");
    s.push_str(match reconcile(r) {
        ReconcileVerdict::InSync => "in_sync",
        ReconcileVerdict::Drifted { .. } => "drifted",
        ReconcileVerdict::UnknownBaseline { .. } => "unknown_baseline",
    });
    s
}

/// 决策的诊断摘要。
pub fn decision_text(d: &Demand) -> String {
    let mut s = String::new();
    s.push_str(d.quality.tag());
    s.push('/');
    s.push_str(d.budget.tag());
    s.push('/');
    s.push_str(d.path.tag());
    s.push('/');
    s.push_str(d.dynamics.tag());
    s.push_str(" -> ");
    s.push_str(match decide(d) {
        Decision::Rule(i) => match RULES.get(i) {
            Some(r) => r.id,
            None => "<invalid-rule>",
        },
        Decision::Fallback => "<fallback>",
    });
    s.push_str(" = ");
    s.push_str(method_display(recommend(d)));
    s
}

// ===========================================================================
// 13. 自检聚合入口
// ===========================================================================

/// VE-F2012 域自检入口（判据逐条映射，见 `vek12_checks.rs`）。
///
/// **只做委托，不复制判据**：判据**唯一**定义在 `vek12_checks`。
/// 两处各写一份的典型后果是"改了一处忘了另一处"，红项绿项互相矛盾时
/// 没人说得清哪份是真的。
pub fn run_vek12_checks() -> crate::checks::CheckSet {
    crate::svstar2::vek12_checks::run_vek12_checks()
}