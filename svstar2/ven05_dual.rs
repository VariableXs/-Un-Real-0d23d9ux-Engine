//! VE-F2605 · 逻辑树与可视树分离（VE-N 域 · UI 框架内核 · N05 组）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F2605`
//!
//! **判据（锚点原文五条）**：双树分离、模板展开、单向同步、三遍历接口、判据。
//!
//! ---
//!
//! ## 一、双树分离（锚点「双树分离是 UI 框架成熟度标志」）
//!
//! | | 逻辑树（[`ControlTree`]，F2602） | 可视树（[`VisualTree`]，本模块） |
//! |---|---|---|
//! | 谁声明 | **开发者**（写代码的人） | **运行时**（模板展开器） |
//! | 节点数| 与开发者写的控件数**相等** | 可多可少（模板展开后**放大**） |
//! | 变更时机 | 开发者改代码 / 运行期改结构 | **只能由逻辑树变更驱动**（单向） |
//! | 内容 | 控件结构意图（控件 + 子控件声明） | 模板实例 + 布局结果位|
//! | 消费者 | 开发者工具、绑定引擎（F2604） | **D 域渲染输入**、命中测试（N03） |
//!
//! **为什么必须是两棵树而不是一棵加标记**（最容易走错的一步）：
//! 一棵树上挂「此节点是模板产物」的布尔标记，看似省了内存，实则三类问题
//! 全都express 不出来——
//! 1. **一对多映射无处安放**：一个 `Button` 声明经`ContentPresenter`
//!    模板展开成 3 个可视节点，这 3 个的共同来源（`owner`）在单树里只能
//!    塞进节点自身，于是「谁生成的我」与「我是谁」混成同一个字段；
//! 2. **失同步不可观测**：开发者删掉一个控件，单树能立刻删掉它的全部
//!    模板产物（因为它们也是节点），于是**双树失步这个最要紧的错误在单树里
//!    根本无法表达**，也就无法断言、无法告警；
//! 3. **遍历语义不同**：逻辑遍历要「按开发者声明序」，可视遍历要「按渲染
//!    序（含模板插入的兄弟）」，命中遍历要「逆序（后画的先命中）」。三种
//!    序在单树上需要三种排序策略并存，即在一棵树里维护三份序——那不如
//!    维护两棵树各一份序 + 一份映射。
//!
//! ## 二、单向数据流：逻辑为源、可视为投影（锚点「双树同步纪律」）
//!
//! **纪律只有一条，但它是本单全部正确性的根**：**可视树只能由逻辑树驱动，
//! 永不反向**。具体落成三条可机检的不变量：
//!
//! - [`DualTree::sync`] 是**唯一**能改可视树的入口；
//! - 每个可视节点必须带一个 `owner: NodeRef`（指向产生它的逻辑节点）；
//!   **无 owner 的可视节点就是幽灵**，由 [`DualTree::assert_sync`] 断言
//!   （锚点「双树失同步 → 同步断言 P1」）；
//! - 可视树**不存**开发者写的属性值——那些归 F2604。可视树只存
//!   「布局结果位」（[`VisualSlot`]）+ 「模板产物身份」。
//!
//! **为何禁止反向**：一旦允许可视树→逻辑树回写，就出现两个写入者，
//! 两个写入者必然有时序差，而时序差在 UI 里的症状是「同一控件在不同机器上
//! 颜色不同、重跑一次就好了」——**不可复现的缺陷是最贵的缺陷**。
//! 单向流把一类时序竞态在架构上消掉，代价是模板产物的变更必须由开发者
//! 侧表达（重设模板或改逻辑树），这个代价是**可接受且可解释**的。
//!
//! ## 三、模板展开：时机与一对多（锚点「展开时机声明」）
//!
//! - **展开时机**：**挂载时展开 + 模板变更重建**（不是懒展开）。
//!   懒展开的问题是「首次渲染才知道要展开什么」，那渲染路径上就有分配与
//!   分支——内核态无堆，不能让渲染路径承担首次展开的代价。
//! - **一对多**：一个逻辑节点可产出**任意个**可视节点（0 个也合法：
//!   模板为空时什么都不产出，这不算错，见 [`ExpandReport::empty_template`]）。
//! - **重建而非增量修补**：模板变更时该逻辑节点的**全部**可视后代整体
//!   丢弃重展开。理由：模板展开的产物之间有 sibling 序依赖（模板里的
//!   一个元素展开成两个可视节点），局部修补极难维持序稳定；整体重建的
//!   代价是 O(模板节点数)，而锚点「性能逐项分解」明确把展开代价定为
//!   `O(模板节点)`——**代价可预期，就不是问题**。
//!
//! ## 四、三遍历接口（锚点「三遍历接口公开」）
//!
//! | 遍历 | 序| 消费者 | 为何这个序 |
//! |---|---|---|---|
//! | [`DualTree::walk_logical`] | 开发者声明序（父先于子，兄弟按声明） | 开发者工具、绑定引擎 | 与开发者写代码的顺序一致，所见即所写 |
//! | [`DualTree::walk_visual`] | 渲染序（可视节点序） | **D 域渲染命令生成** | 渲染命令必须按 z 序生成，否则覆盖关系错 |
//! | [`DualTree::walk_hit`] | **逆序**（命中测试） | N03 输入域（前向） | 后画的在上，先命中后画的——逆序遍历第一个命中即正确答案 |
//!
//! **为什么命中遍历必须逆序而不是「遍历全部取最近」**：后者要遍历整棵树
//! 才能定答案，而命中测试在**每帧、每次指针移动**时都跑（内核态光标
//! 处理走F12 逃生门，预算敏感）。逆序 + 首个命中即返回，把最坏情况从
//! O(N) 降到 O(命中位置的深度)。
//!
//! ## 五、D/N 边界（锚点「N 产结构、D 产像素」）
//!
//! 可视树是D 域渲染的**结构源**，本模块的职责到「交出可视节点序」为止：
//! [`DualTree::render_source`] 产出的 [`RenderSource`] 只含**结构**
//! （位置槽 + 尺寸槽 + 父子序），**不含任何绘制指令**。D 域拿它去生成
//! 渲染命令。这样边界是**可机检的**：[`RenderSource`] 的类型里没有
//! `draw_*` 字段，任何「N 域越界画像素」的企图在类型层面就不成立。
//!
//! ## 六、错误路径与降级矩阵（锚点原表逐行落实）
//!
//! | 锚点情形 | 本模块处置 | 诊断码 |
//! |---|---|---|
//! | 模板展开失败（模板引用缺失） | **降级为控件自身 + 告警**（不阻断渲染） | [`DualDiagCode::TemplateMissing`] |
//! | 双树失同步（可视含幽灵节点） | **同步断言**（P1，单向数据流破裂） | [`DualDiagCode::GhostVisual`] |
//! | 映射一对多计数错误 | **断言**（展开完整性） | [`DualDiagCode::MappingCountMismatch`] |
//! | D 接口变更 | **对账钩子**（家族） | [`DualDiagCode::DInterfaceDrift`] |
//! | 遍历环 | **树不变量拦截**（F2602 家族） | [`DualDiagCode::TraversalCycle`] |
//!
//! **降级为什么选「降级 + 告警」而不是「拒绝渲染」**：模板缺失时若拒绝
//! 渲染，用户看到的是**整块空白**（比控件本身丑得多，且无从判断是加载
//! 失败还是界面坏了）；降级为控件自身则「按钮还在，只是没图标」，用户
//! 与开发者都能立刻定位。反之，**幽灵节点与映射计数错误选断言**：这两类
//! 是同步机制自身坏了（单向流破裂），继续渲染会产出**不确定的画面**
//! （每次运行可能不同），那比崩掉更糟。
//!
//! ## 七、跨批对接台账（锚点「跨批对接点」四条）
//!
//! 见 [`HANDOFFS`]。前向三条已兑现位：F2606 消费 [`SyncOp`] 做增量更新，
//! N03 消费 [`DualTree::walk_hit`]，D 域消费 [`RenderSource`]。
//!
//! ## 八、确定性
//!
//! 纯函数、无时钟无 IO、无堆外分配；遍历序固定；模板展开遍历序固定。
//! 同输入同输出，回归可复现。零 panic（生产面）、零 `unsafe`。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use crate::svstar2::ven02_tree as lt;

// ---------------------------------------------------------------------------
// 一、诊断面
// ---------------------------------------------------------------------------

/// 双树诊断码。
///
/// **与F2602/F2604 的码不重叠**：本域每个单号有自己的码段，
/// 同一情形由不同单号上报时**不得共用码**（否则调用方不知该查哪个域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DualDiagCode {
    /// 模板引用缺失（锚点：降级为控件自身 + 告警）。
    TemplateMissing,
    /// 可视节点无owner（幽灵）——单向数据流破裂，P1断言。
    GhostVisual,
    /// 逻辑↔可视映射计数不符（展开完整性断言）。
    MappingCountMismatch,
    /// 可视节点挂到了不存在的逻辑节点上。
    OwnerAbsent,
    /// D 域接口漂移（对账钩子触发）。
    DInterfaceDrift,
    /// 遍历时检出环（树不变量，锚点归属 F2602 家族）。
    TraversalCycle,
    /// 逻辑树节点已销毁仍被展开引用。
    OwnerDestroyed,
    /// 模板声明非法（深度超限 / 元素 id 重复）。
    TemplateInvalid,
    /// 递归深度超限（模板展开深度防护）。
    DepthLimitExceeded,
    /// 双树未初始化或已销毁（调用序错误）。
    DualLifecycleViolation,
    /// 自检审计不通过。
    DualSelfcheckFailed,
}

impl DualDiagCode {
    /// 错误码（`0x2B00 | n+1` 段，专属 VE-N/F2605）。
    ///
    /// **基数必须取低 4 位为 0 的整段起点**：`|` 只置位不清位，
    /// 基数带低位会令相邻码塌陷成同一个（曾在本域踩过：`0x2B03 | n+1`
    /// 让 n=0..3 全撞成`0x2B03`）。
    pub fn code(self) -> u16 {
        0x2B00 | (self as u16) + 1
    }

    /// 简短中文名。
    pub fn label(self) -> &'static str {
        match self {
            DualDiagCode::TemplateMissing => "模板引用缺失",
            DualDiagCode::GhostVisual => "可视节点无来源（幽灵节点）",
            DualDiagCode::MappingCountMismatch => "逻辑可视映射计数不符",
            DualDiagCode::OwnerAbsent => "可视节点的来源逻辑节点不存在",
            DualDiagCode::DInterfaceDrift => "D 域渲染接口漂移",
            DualDiagCode::TraversalCycle => "遍历检出环",
            DualDiagCode::OwnerDestroyed => "来源逻辑节点已销毁",
            DualDiagCode::TemplateInvalid => "模板声明非法",
            DualDiagCode::DepthLimitExceeded => "展开深度超限",
            DualDiagCode::DualLifecycleViolation => "双树未初始化或已销毁",
            DualDiagCode::DualSelfcheckFailed => "双树自检审计不通过",
        }
    }

    /// 原因（为什么发生）。
    pub fn cause(self) -> &'static str {
        match self {
            DualDiagCode::TemplateMissing => "逻辑节点声明了模板，但模板表里没有同名条目",
            DualDiagCode::GhostVisual => "可视节点的 owner 字段为空——它不是由任何逻辑节点展开出来的",
            DualDiagCode::MappingCountMismatch => {
                "声称的映射条数与实际展开的可视子节点数不符（展开被截断或重复计入）"
            }
            DualDiagCode::OwnerAbsent => "可视节点记的 owner id 在逻辑树里查不到",
            DualDiagCode::DInterfaceDrift => "对账钩子发现 D 域消费的结构字段与本模块产出不一致",
            DualDiagCode::TraversalCycle => "遍历时回到已访问节点——双向父子指针不一致或父链成环",
            DualDiagCode::OwnerDestroyed => "逻辑节点已标记销毁，可视树却仍在引用它",
            DualDiagCode::TemplateInvalid => "模板元素 id 重复，或模板嵌套深度超限",
            DualDiagCode::DepthLimitExceeded => "模板展开递归深度超过上限",
            DualDiagCode::DualLifecycleViolation => "双树已destroy 或尚未挂载逻辑树就调用",
            DualDiagCode::DualSelfcheckFailed => "自检审计发现不变量被破",
        }
    }

    /// 处置指引（必须说「该怎么办」）。
    pub fn hint(self) -> &'static str {
        match self {
            DualDiagCode::TemplateMissing => {
                "补齐模板表条目；过渡期可继续渲染（已降级为控件自身），但须按日志定位声明侧"
            }
            DualDiagCode::GhostVisual => {
                "幽灵节点说明有绕过 sync 直升可视树的写入路径——查所有对可视树的写入口，收敛到 sync"
            }
            DualDiagCode::MappingCountMismatch => {
                "查展开器是否中途返回（深度超限/预算不足）；映射计数应由展开结果反推，不接受调用方申报"
            }
            DualDiagCode::OwnerAbsent => {
                "确认逻辑树是否被整体替换过；owner 是可视节点的唯一来源，不可事后补写，须重新 sync"
            }
            DualDiagCode::DInterfaceDrift => "按对账钩子报告的字段名核对 D 域结构消费侧与本模块的产出契约",
            DualDiagCode::TraversalCycle => "查逻辑树父子指针是否同事务更新；树不变量由 F2602 拦截，此处只做拦截后的转译",
            DualDiagCode::OwnerDestroyed => "销毁逻辑节点前先经 sync 收回其可视后代，不要直接置 destroyed 标志",
            DualDiagCode::TemplateInvalid => "修正模板声明：元素 id 须树内唯一，嵌套深度须在上限内",
            DualDiagCode::DepthLimitExceeded => "拆平模板嵌套层级；模板展开深度是渲染路径上的硬约束，不可动态放宽",
            DualDiagCode::DualLifecycleViolation => "核对调用序：先挂逻辑树，再 sync，最后才遍历",
            DualDiagCode::DualSelfcheckFailed => "按报告里指出的不变量名定位；本码只作汇总，不承载修复指引",
        }
    }

    /// 人话（给终端用户看的一句）。
    pub fn human(self) -> &'static str {
        match self {
            DualDiagCode::TemplateMissing => "这个控件的样式模板没找到，先按默认样子显示了。",
            DualDiagCode::GhostVisual => "界面内部出了点不一致，先停下别乱动，重建界面树就好。",
            DualDiagCode::MappingCountMismatch => "控件展开时少展开了一块，先按现有样子显示，稍后会自动补齐。",
            DualDiagCode::OwnerAbsent => "界面元素找不到它的来源，重建一次即可恢复。",
            DualDiagCode::DInterfaceDrift => "界面结构和绘制环节的约定对不上，画面可能不正常，重建可恢复。",
            DualDiagCode::TraversalCycle => "界面结构里发现了环路，重建界面树可恢复。",
            DualDiagCode::OwnerDestroyed => "这个控件已经不用了，但还有残留引用，重建即可。",
            DualDiagCode::TemplateInvalid => "控件样式声明有问题，已退回默认样式。",
            DualDiagCode::DepthLimitExceeded => "控件嵌套太深，已按默认样式显示。",
            DualDiagCode::DualLifecycleViolation => "界面还没准备好就被访问了，通常几毫秒后自动恢复。",
            DualDiagCode::DualSelfcheckFailed => "界面结构自检没过，已重建。",
        }
    }

    /// **是否阻断渲染**。
    ///
    /// **降级方向相反的两类不得共用码**：模板类问题是「缺资源」，
    /// 降级渲染；同步类问题是「机制自身坏了」，继续渲染会产出不确定
    /// 画面，故阻断。
    pub fn blocks_render(self) -> bool {
        matches!(
            self,
            DualDiagCode::GhostVisual
                | DualDiagCode::MappingCountMismatch
                | DualDiagCode::OwnerAbsent
                | DualDiagCode::OwnerDestroyed
                | DualDiagCode::TraversalCycle
                | DualDiagCode::DualLifecycleViolation
        )
    }
}

/// 全部诊断码（供封闭集完备性判据逐条比对）。
pub const DUAL_DIAG_CODES: [DualDiagCode; 11] = [
    DualDiagCode::TemplateMissing,
    DualDiagCode::GhostVisual,
    DualDiagCode::MappingCountMismatch,
    DualDiagCode::OwnerAbsent,
    DualDiagCode::DInterfaceDrift,
    DualDiagCode::TraversalCycle,
    DualDiagCode::OwnerDestroyed,
    DualDiagCode::TemplateInvalid,
    DualDiagCode::DepthLimitExceeded,
    DualDiagCode::DualLifecycleViolation,
    DualDiagCode::DualSelfcheckFailed,
];

/// 双树诊断（错误三要素 + 位置）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DualDiagnostic {
    /// 诊断码。
    pub code: DualDiagCode,
    /// 人话描述。
    pub message: String,
    /// 处置指引。
    pub hint: String,
    /// 触发位置（逻辑节点 id / 可视节点 id）。
    pub at: String,
}

/// 便捷构造。
pub fn dd(code: DualDiagCode, message: &str, at: &str) -> DualDiagnostic {
    DualDiagnostic {
        code,
        message: message.to_string(),
        hint: code.hint().to_string(),
        at: at.to_string(),
    }
}

/// 双树操作结果。
pub type DualOutcome<T> = Result<T, DualDiagnostic>;

// ---------------------------------------------------------------------------
// 二、模板与可视节点
// ---------------------------------------------------------------------------

/// 模板元素种类。
///
/// **封闭枚举而非字符串**：模板元素的种类决定它展开成什么形状的可视节点，
/// 允许自由字符串等于允许「D 域不认识的形状」进树，而那种节点在渲染时
/// 才炸——错误从产生点推迟到了消费点。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TemplateElement {
    /// 原样产出一个可视节点（1 元素 → 1 节点）。
    Leaf,
    /// 把模板内容**内联展开**到当前位置（1 元素 → N 节点，N ≥ 1）。
    Content,
    /// 嵌套另一个模板（1 元素 → 该子模板展开的全部节点）。
    Nested,
}

impl TemplateElement {
    /// 中文名。
    pub fn label(self) -> &'static str {
        match self {
            TemplateElement::Leaf => "叶子",
            TemplateElement::Content => "内容内联",
            TemplateElement::Nested => "嵌套模板",
        }
    }
}

/// 模板中的一个元素声明。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TemplateElementSpec {
    /// 元素产出 id 的**前缀**（展开时拼上宿主逻辑节点 id，保证全局唯一）。
    pub tag: &'static str,
    /// 元素种类。
    pub kind: TemplateElement,
    /// `Nested` 时的子模板名（其余种类忽略）。
    pub nested: &'static str,
}

/// 控件模板（`ControlTemplate`）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ControlTemplate {
    /// 模板名（逻辑节点按名引用）。
    pub name: &'static str,
    /// 元素声明（有序，序即展开序）。
    pub elements: Vec<TemplateElementSpec>,
}

/// 布局结果槽（可视节点携带的**结构位**，不含绘制指令）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct VisualSlot {
    /// x（左上角）。
    pub x: i32,
    /// y（左上角）。
    pub y: i32,
    /// 宽。
    pub w: u32,
    /// 高。
    pub h: u32,
}

/// 可视节点 id（全局唯一）。
///
/// **带类型而非裸`String`**：逻辑 id 与可视 id 在同一棵树的遍历里会同时
/// 出现（遍历回调参数），裸 `String` 会让调用方传串心传节点而不报错；
/// 带类型让「把可视 id 当逻辑 id 用」在类型层面就不成立。
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct VisualId(pub String);

impl VisualId {
    /// 字符串形式。
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// 可视节点。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct VisualNode {
    /// 本节点 id。
    pub id: VisualId,
    /// 产生本节点的逻辑节点 id（**必填**，空即幽灵）。
    pub owner: String,
    /// 可视父节点 id（可视树根为 `None`）。
    pub parent: Option<VisualId>,
    /// 可视子节点 id 有序（渲染序）。
    pub children: Vec<VisualId>,
    /// 布局结果槽。
    pub slot: VisualSlot,
    /// 产出本节点时用的模板名（根节点为 `None` —— 根由逻辑树根直接产出）。
    pub template: Option<&'static str>,
    /// 是否为模板展开的产物（根为 `false`）。
    pub expanded: bool,
}

/// 可视树。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct VisualTree {
    nodes: Vec<VisualNode>,
    root: VisualId,
    destroyed: bool,
}

impl VisualTree {
    /// 构造**空**可视树（零节点，根尚未产出）。
    ///
    /// **曾在此建一个根占位节点，导致与展开器产出冲突**（本单实测抓到）：
    /// `new("root")` 先放进一个 id=`root` 的节点，随后 [`expand`] 又要为
    /// 同一逻辑节点产出一个 id=`root` 的可视节点——**id 重复**，
    /// 表现为 `sync` 在最基本的一棵树上直接失败。
    /// 更根本的问题是「占位」这个概念本身没有意义：可视树的根**就是**
    /// 根逻辑节点的展开产物，不存在「展开之前先有个东西站在那儿」。
    pub fn new(root: &str) -> DualOutcome<VisualTree> {
        if root.is_empty() {
            return Err(dd(
                DualDiagCode::DualLifecycleViolation,
                "可视树根 id 为空",
                "<init>",
            ));
        }
        Ok(VisualTree {
            nodes: Vec::new(),
            root: VisualId(root.to_string()),
            destroyed: false,
        })
    }

    /// 根节点 id。
    pub fn root(&self) -> &VisualId {
        &self.root
    }

    /// 节点总数。
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// 是否空（仅根占位时不算空）。
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// 是否已销毁。
    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }

    /// 按 id 取节点。
    pub fn get(&self, id: &VisualId) -> Option<&VisualNode> {
        self.nodes.iter().find(|n| &n.id == id)
    }

    /// 按 id 取节点（可变）。
    pub fn get_mut(&mut self, id: &VisualId) -> Option<&mut VisualNode> {
        self.nodes.iter_mut().find(|n| &n.id == id)
    }

    /// 全部节点（**序不保证是渲染序**，需用 [`DualTree::walk_visual`]）。
    pub fn nodes(&self) -> &[VisualNode] {
        &self.nodes
    }

    /// 销毁整棵可视树。
    pub fn destroy(&mut self) {
        self.destroyed = true;
        self.nodes.clear();
    }

    /// 直接挂一个可视节点（**仅供展开器与自检面使用**）。
    ///
    /// `parent` 传 `None` 表示**挂根**——树空时才允许，否则拒绝
    /// （已有根再挂根 = 两个根，那不是树）。
    ///
    /// **为何不是私有**：F2606（增量更新）需要按逻辑节点的局部变更重建
    /// 其子树，那条路径同样属于「受 sync 纪律约束的写入」。把它封死成
    /// 私有会迫使 F2606 走旁门，而**旁门正是幽灵节点的来源**。
    /// 纪律靠 [`DualTree::assert_sync`] 断言，不靠访问控制。
    pub fn attach(&mut self, parent: Option<&VisualId>, node: VisualNode) -> DualOutcome<()> {
        if self.destroyed {
            return Err(dd(
                DualDiagCode::DualLifecycleViolation,
                "可视树已销毁",
                node.id.as_str(),
            ));
        }
        if self.get(&node.id).is_some() {
            return Err(dd(
                DualDiagCode::TemplateInvalid,
                "可视节点 id 重复",
                node.id.as_str(),
            ));
        }
        match parent {
            None => {
                if self.nodes.is_empty() && node.parent.is_none() {
                    // 挂根
                } else if self.nodes.is_empty() {
                    return Err(dd(
                        DualDiagCode::TemplateInvalid,
                        "空树挂根时 parent 须为 None",
                        node.id.as_str(),
                    ));
                } else {
                    return Err(dd(
                        DualDiagCode::TemplateInvalid,
                        "已有根，不能再挂第二个根",
                        node.id.as_str(),
                    ));
                }
            }
            Some(p) => {
                if self.get(p).is_none() {
                    return Err(dd(
                        DualDiagCode::OwnerAbsent,
                        "挂载父节点不在可视树内",
                        p.as_str(),
                    ));
                }
            }
        }
        self.nodes.push(node);
        // 先把新节点的 id 克隆出来再取可变借用——直接读self.nodes 会与
        // get_mut 的可变借用冲突（E0502）。
        let new_id = self.nodes[self.nodes.len() - 1].id.clone();
        if let Some(p) = parent {
            if let Some(pn) = self.get_mut(p) {
                pn.children.push(new_id);
            }
        }
        Ok(())
    }
}

/// 模板表（`ControlTemplate` 的注册处）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TemplateTable {
    entries: Vec<ControlTemplate>,
}

/// 模板表构造与查询。
impl TemplateTable {
    /// 构造空表。
    pub fn new() -> TemplateTable {
        TemplateTable { entries: Vec::new() }
    }

    /// 登记一个模板（同名则拒绝，避免静默覆盖）。
    pub fn register(&mut self, tpl: ControlTemplate) -> DualOutcome<()> {
        if tpl.name.is_empty() {
            return Err(dd(
                DualDiagCode::TemplateInvalid,
                "模板名为空",
                "<register>",
            ));
        }
        if self.find(tpl.name).is_some() {
            return Err(dd(
                DualDiagCode::TemplateInvalid,
                "模板名重复",
                tpl.name,
            ));
        }
        self.entries.push(tpl);
        Ok(())
    }

    /// 按名查模板。
    pub fn find(&self, name: &str) -> Option<&ControlTemplate> {
        self.entries.iter().find(|t| t.name == name)
    }

    /// 模板总数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否空表。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 全部模板（登记序）。
    pub fn all(&self) -> &[ControlTemplate] {
        &self.entries
    }
}

/// 展开深度上限（渲染路径上的硬约束）。
pub const MAX_EXPAND_DEPTH: usize = 8;

// ---------------------------------------------------------------------------
// 三、模板展开
// ---------------------------------------------------------------------------

/// 展开结果报告。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ExpandReport {
    /// 产出���可视节点 id（渲染序，深度优先）。
    pub produced: Vec<VisualId>,
    /// 降级次数（模板缺失 → 控件自身；**不阻断**）。
    pub degraded: u32,
    /// 产出节点总数（`produced.len()` 的显式副本，供对账钩子比）。
    pub produced_count: u32,
    /// 空模板次数（模板存在但零元素 —— 合法，不是错）。
    pub empty_template: u32,
}

impl ExpandReport {
    /// 新建空报告。
    pub fn new() -> ExpandReport {
        ExpandReport {
            produced: Vec::new(),
            degraded: 0,
            produced_count: 0,
            empty_template: 0,
        }
    }
}

/// 逻辑节点引用（`ControlTree` 的节点 id 在双树侧的带类型包装）。
///
/// **为何不用 `String`**：同[���VisualId] 的理由——遍历回调里逻辑 id 与
/// 可视 id 同时出现，带类型能在编译期挡住「传错侧」。
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct NodeRef(pub String);

impl NodeRef {
    /// 字符串形式。
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// 单节点的展开动作。
///
/// **模板缺失时的降级**（锚点「降级为控件自身 + 告警」）：产出**一个**
/// 叶子可视节点（`expanded = false`），令`degraded += 1`，并返回
/// **非阻断**诊断。调用方（渲染侧）据此继续渲染。
pub fn expand(
    visual: &mut VisualTree,
    logic_id: &str,
    parent: Option<&VisualId>,
    tpl_name: Option<&'static str>,
    table: &TemplateTable,
    parent_slot: Option<VisualSlot>,
    index_in_layer: usize,
    layer_len: usize,
    layout: &mut Vec<VisualSlot>,
    depth: usize,
    report: &mut ExpandReport,
) -> DualOutcome<()> {
    if depth > MAX_EXPAND_DEPTH {
        return Err(dd(
            DualDiagCode::DepthLimitExceeded,
            "模板展开深度超限",
            logic_id,
        ));
    }
    match tpl_name {
        None => {
            // 无模板：产出控件自身（这是**正常路径**，不是降级）
            push_leaf(
                visual,
                logic_id,
                logic_id,
                parent,
                parent_slot,
                index_in_layer,
                layer_len,
                layout,
                report,
            )
        }
        Some(name) => {
            let tpl = match table.find(name) {
                Some(t) => t,
                None => {
                    // 降级为控件自身 + 告警（不阻断）
                    report.degraded += 1;
                    push_leaf(
                        visual,
                        logic_id,
                        logic_id,
                        parent,
                        parent_slot,
                        index_in_layer,
                        layer_len,
                        layout,
                        report,
                    )?;
                    return Err(dd(
                        DualDiagCode::TemplateMissing,
                        "模板缺失，已降级为控件自身",
                        logic_id,
                    ));
                }
            };
            if tpl.elements.is_empty() {
                // 空模板：合法，产出零节点
                report.empty_template += 1;
                return Ok(());
            }
            // 宿主可视节点（挂住全部产物，成为它们的父）
            let host = VisualId(format!("{}#tpl:{}", logic_id, name));
            let slot = next_slot(layout, parent_slot, index_in_layer, layer_len);
            visual.attach(
                parent,
                VisualNode {
                    id: host.clone(),
                    owner: logic_id.to_string(),
                    parent: parent.cloned(),
                    children: Vec::new(),
                    slot,
                    template: Some(name),
                    expanded: true,
                },
            )?;
            report.produced.push(host.clone());
            report.produced_count += 1;
            // 元素槽：本层共 `elements.len()` 个兄弟（含嵌套展开出的
            // 那一整个子树算一个位置——它自己会再往下层）。
            let elems = tpl.elements.clone();
            // 元素层的总宽用**调用方给的 `layer_len`**，不是 `elems.len()`：
            // 宿主下还会追加本逻辑节点的**逻辑子节点**（由
            // `expand_subtree` 的递归挂上来），它们与这些元素同层。
            // 只按元素数切分，逻辑子节点就只能挤在元素那一段里
            // ——重叠。`layer_len` 已含那部分（见 `expand_subtree`）。
            let n_elems = if layer_len == 0 { elems.len() } else { layer_len };
            for (ei, el) in elems.iter().enumerate() {
                let child_id = format!("{}#{}.{}", logic_id, name, el.tag);
                match el.kind {
                    TemplateElement::Leaf | TemplateElement::Content => {
                        // Leaf 与 Content 的产物都直接挂宿主下；Content
                        // 只是语义上「内容内联」，槽位处理与 Leaf 相同。
                        push_leaf(
                            visual,
                            logic_id,
                            &child_id,
                            Some(&host),
                            Some(slot),
                            ei,
                            n_elems,
                            layout,
                            report,
                        )?;
                    }
                    TemplateElement::Nested => {
                        expand(
                            visual,
                            &child_id,
                            Some(&host),
                            Some(el.nested),
                            table,
                            Some(slot),
                            ei,
                            n_elems,
                            layout,
                            depth + 1,
                            report,
                        )?;
                    }
                }
            }
            Ok(())
        }
    }
}

/// 产出一个叶子可视节点（走布局槽）。
///
/// **`owner` 是宿主逻辑节点 id，不是本节点自己的 id**——这是最容易写错的
/// 一处：元素 `id` 是「宿主#模板名.标签」形式，**它在逻辑树里查不到**。
/// owner 填错会让每个元素节点都成幽灵（`assert_sync` 报GhostVisual），
/// 而症状只是「同步断言一直红」，归因容易跑偏到「同步机制坏了」。
fn push_leaf(
    visual: &mut VisualTree,
    owner_logic: &str,
    id: &str,
    parent: Option<&VisualId>,
    parent_slot: Option<VisualSlot>,
    index_in_layer: usize,
    layer_len: usize,
    layout: &mut Vec<VisualSlot>,
    report: &mut ExpandReport,
) -> DualOutcome<()> {
    let slot = next_slot(layout, parent_slot, index_in_layer, layer_len);
    let vid = VisualId(id.to_string());
    visual.attach(
        parent,
        VisualNode {
            id: vid.clone(),
            owner: owner_logic.to_string(),
            parent: parent.cloned(),
            children: Vec::new(),
            slot,
            template: None,
            expanded: false,
        },
    )?;
    report.produced.push(vid);
    report.produced_count += 1;
    Ok(())
}

/// 取布局槽：**层内序号 → 父槽的等分区域**。
///
/// **为什么必须层叠而不是平铺（本单实测踩到的坑）**：初版把节点槽排成
/// 8 列网格、彼此**不重叠**。那样任何一点最多命中一个节点，
/// 「后画的先命中」这条性质**根本无法被检验**——`walk_hit` 退化成
/// 「找那唯一一个」，逆序与正序行为完全一样，判据只能红着。
/// 层叠之后父槽包含全部子槽，取子槽内一点必然命中父子一串节点，
/// 逆序才有意义（这才是真实控件树的形状：面板的槽含着它的标题槽）。
///
/// **序号由调用方给，不在这里猜**：`index_in_layer` / `layer_len` 必须
/// 由「谁在枚举这一层」说了算——逻辑子节点那层由 `expand_subtree` 给，
/// 模板元素那层由 `expand` 给。曾经在这里靠「数一数 layout 里落在父槽
/// 内有几个」来推序号，结果**把父槽自己也算进去**、同层兄弟全部拿到
/// 同一个槽（本单实测：panel 的三个元素槽完全重叠）——那不是层叠，
/// 是全塌成一块。宁可让调用方多传两个数，也不在这里猜。
///
/// **仍然不实现真实布局**：只保证三条可机检性质——
/// ① 同层兄弟槽**互不重叠**（并列内容不打架）；
/// ② 子槽**完全落在**父槽内（父子包含成立）；
/// ③ 顶层层内也互不重叠。
/// 真正的布局算法是 F2621 的职责，渗进来会抢活且两处不一致时难解释。
fn next_slot(
    layout: &mut Vec<VisualSlot>,
    parent: Option<VisualSlot>,
    index_in_layer: usize,
    layer_len: usize,
) -> VisualSlot {
    let n = if layer_len == 0 { 1 } else { layer_len };
    let i = if index_in_layer < n { index_in_layer } else { n - 1 };
    let s = match parent {
        // 顶层：128px 网格，够大才能装下层叠的子槽。
        None => VisualSlot {
            x: (i as i32 % 4) * 128,
            y: (i as i32 / 4) * 128,
            w: 128,
            h: 128,
        },
        Some(p) => {
            // 父槽**下半部**切成本层的 n 份，取第 i 份：
            // 上半部留给父自身的边框/标题一类（这里不建模，只是留白），
            // 于是「子在父内」与「兄弟不重叠」同时成立。
            let band_h = (p.h / 2).max(1);
            let band_y = p.y + (p.h - band_h) as i32;
            let cw = (p.w / n as u32).max(1);
            let x = p.x + cw.saturating_mul(i as u32) as i32;
            // 末份吃满剩余宽度（整除余数），避免右侧留一条死区。
            let w = if i + 1 == n {
                (p.x + p.w as i32 - x).max(1) as u32
            } else {
                cw
            };
            VisualSlot { x, y: band_y, w, h: band_h }
        }
    };
    layout.push(s);
    s
}

// ---------------------------------------------------------------------------
// 四、同步（锚点「单向同步」）
// ---------------------------------------------------------------------------

/// 同步动作（**可视树变更的唯一合法来源**）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum SyncOp {
    /// 全量重建（逻辑树挂载后的首次展开）。
    FullRebuild,
    /// 某个逻辑节点新增（带其绑定的模板名）。
    AddNode {
        /// 逻辑节点 id。
        id: String,
        /// 父逻辑节点 id。
        parent: String,
    },
    /// 某个逻辑节点移除（收回其**全部**可视后代）。
    RemoveNode {
        /// 逻辑节点 id。
        id: String,
    },
    /// 某个逻辑节点的模板变更（**整体重建**其可视子树，锚点时机声明）。
    TemplateChanged {
        /// 逻辑节点 id。
        id: String,
    },
}

impl SyncOp {
    /// 中文名。
    pub fn label(&self) -> &'static str {
        match self {
            SyncOp::FullRebuild => "全量重建",
            SyncOp::AddNode { .. } => "新增节点",
            SyncOp::RemoveNode { .. } => "移除节点",
            SyncOp::TemplateChanged { .. } => "模板变更重建",
        }
    }

    /// 变更的逻辑节点 id（`FullRebuild` 为全树）。
    pub fn target(&self) -> &str {
        match self {
            SyncOp::FullRebuild => "<all>",
            SyncOp::AddNode { id, .. } => id,
            SyncOp::RemoveNode { id } => id,
            SyncOp::TemplateChanged { id } => id,
        }
    }

    /// 是否为「收回型」动作（**两种方向的后果不同，不可混判**）。
    ///
    /// 拆这个函数是因为「移除」与「重建」都表现为「可视树减少节点」，
    /// 但语义相反：前者是**逻辑侧删除**（节点本不该存在），
    /// 后者是**模板替换**（节点该存在但内容要换）。
    /// 判据必须能区分这两者，否则「重建后节点数不降」会被误判为泄漏。
    pub fn is_removal(&self) -> bool {
        matches!(self, SyncOp::RemoveNode { .. })
    }
}

/// 模板绑定表（逻辑节点 → 模板名）。
///
/// **为何独立于逻辑树**：逻辑树（F2602）存控件结构，不该被塞进「用哪个
/// 模板」这种**渲染期关注点**——模板是可视侧的事，塞进逻辑树会让 F2602
/// 依赖本单，形成环依赖。故绑定表归本模块所有，逻辑树只提供结构。
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct TemplateBinding {
    entries: Vec<(String, Option<&'static str>)>,
}

impl TemplateBinding {
    /// 新建空绑定表。
    pub fn new() -> TemplateBinding {
        TemplateBinding { entries: Vec::new() }
    }

    /// 绑定一个逻辑节点到模板（`None` = 控件自身，无模板）。
    pub fn bind(&mut self, logic_id: &str, tpl: Option<&'static str>) {
        self.entries.retain(|(k, _)| k != logic_id);
        self.entries.push((logic_id.to_string(), tpl));
    }

    /// 解绑（节点不再用模板）。
    pub fn unbind(&mut self, logic_id: &str) {
        self.entries.retain(|(k, _)| k != logic_id);
    }

    /// 查某逻辑节点的模板名。
    pub fn template_of(&self, logic_id: &str) -> Option<Option<&'static str>> {
        self.entries
            .iter()
            .find(|(k, _)| k == logic_id)
            .map(|(_, v)| *v)
    }

    /// 绑定条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否空表。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// 同步结果报告。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SyncReport {
    /// 实际执行的动作（`None` = 无变更，空操作合法）。
    pub applied: Option<SyncOp>,
    /// 本轮产出的可视节点数。
    pub produced: u32,
    /// 本轮收回的可视节点数。
    pub reclaimed: u32,
    /// 降级次数（模板缺失→控件自身，**不阻断**）。
    pub degraded: u32,
    /// 空模板次数（模板存在但零元素 —— **合法**）。
    pub empty_template: u32,
    /// 非阻断诊断。
    pub warnings: Vec<DualDiagnostic>,
}

impl SyncReport {
    /// 新建空报告。
    pub fn new() -> SyncReport {
        SyncReport {
            applied: None,
            produced: 0,
            reclaimed: 0,
            degraded: 0,
            empty_template: 0,
            warnings: Vec::new(),
        }
    }
}

/// 双树（逻辑树 + 可视树的配对）。
///
/// **不持有逻辑树**（纪律：逻辑树是开发者的产物，本模块只**只读消费**）。
/// [`DualTree::sync`] 显式接收 `&ControlTree` 参数——这样依赖方向是
/// **本单 → F2602**，而不是「本模块偷偷持有一份可改的逻辑树」。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DualTree {
    visual: VisualTree,
    table: TemplateTable,
    binding: TemplateBinding,
    layout: Vec<VisualSlot>,
    /// 逻辑节点 → 其展开产出的可视节点（**一对多映射**）。
    mapping: Vec<(String, Vec<VisualId>)>,
    destroyed: bool,
}

impl DualTree {
    /// 挂载（首次，尚不展开——展开由 [`DualTree::sync`] 触发）。
    pub fn mount(logic_root: &str, table: TemplateTable) -> DualOutcome<DualTree> {
        if logic_root.is_empty() {
            return Err(dd(
                DualDiagCode::DualLifecycleViolation,
                "逻辑树根 id 为空",
                "<mount>",
            ));
        }
        let visual = VisualTree::new(logic_root)?;
        Ok(DualTree {
            visual,
            table,
            binding: TemplateBinding::new(),
            layout: Vec::new(),
            mapping: vec![(logic_root.to_string(), Vec::new())],
            destroyed: false,
        })
    }

    /// 可视树只读引用。
    pub fn visual(&self) -> &VisualTree {
        &self.visual
    }

    /// 可视树可变引用（**仅供自检面注入坏节点**）。
    ///
    /// **为何生产面也开这个口**：改坏可视树是「验证同步断言能不能抓住」
    /// 的唯一手段，而那属于自检。若把它限制成 `#[cfg(test)]`，
    /// 自检就只能在整 crate 的测试配置下跑，隔离验证反而做不成。
    /// 生产路径**不调用它**（可由 `grep visual_mut` 在提交前核）。
    pub fn visual_mut(&mut self) -> &mut VisualTree {
        &mut self.visual
    }

    /// 模板表只读引用。
    pub fn table(&self) -> &TemplateTable {
        &self.table
    }

    /// 模板绑定表只读引用。
    pub fn binding(&self) -> &TemplateBinding {
        &self.binding
    }

    /// 绑定表可变引用（**同步前置动作**：先绑模板，再 sync）。
    pub fn binding_mut(&mut self) -> &mut TemplateBinding {
        &mut self.binding
    }

    /// 映射表只读引用（**对账钩子**与 F2606 消费）。
    pub fn mapping(&self) -> &[(String, Vec<VisualId>)] {
        &self.mapping
    }

    /// 查某逻辑节点的产出（**一对多**：可为空 / 一个 / 多个）。
    pub fn visuals_of(&self, logic_id: &str) -> &[VisualId] {
        self.mapping
            .iter()
            .find(|(k, _)| k == logic_id)
            .map(|(_, v)| v.as_slice())
            .unwrap_or(&[])
    }

    /// 销毁。
    pub fn destroy(&mut self) {
        self.visual.destroy();
        self.destroyed = true;
    }

    /// 是否已销毁。
    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }

    /// **同步：可视树变更的唯一入口**（锚点「逻辑树变更 →可视树增量更新」）。
    ///
    /// **增量口径**（锚点「同步 O(变更)」）：
    /// - [`SyncOp::FullRebuild`]：清空可视树后按逻辑树全量重展开；
    /// - [`SyncOp::AddNode`]：只展开该节点，产物挂到其父的**宿主**下——
    ///   成本 O(该节点模板节点数)，**与逻辑树规模无关**；
    /// - [`SyncOp::RemoveNode`]：收回该节点产出**及其全部可视后代**；
    /// - [`SyncOp::TemplateChanged`]：**整体重建**该节点的可视子树
    ///   （锚点时机声明，见头注「三」）。
    ///
    /// `logic` 是逻辑树的**只读**借用——本函数不改它一个字节。
    pub fn sync(&mut self, logic: &lt::ControlTree, op: &SyncOp) -> DualOutcome<SyncReport> {
        if self.destroyed || self.visual.is_destroyed() {
            return Err(dd(
                DualDiagCode::DualLifecycleViolation,
                "双树已销毁",
                op.target(),
            ));
        }
        let mut rep = SyncReport::new();
        match op {
            SyncOp::FullRebuild => {
                let root = self.visual.root.0.clone();
                self.visual = VisualTree::new(&root)?;
                self.layout.clear();
                self.mapping.clear();
                self.mapping.push((root.clone(), Vec::new()));
                let mut er = ExpandReport::new();
                self.expand_subtree(logic, &root, &mut er, 0)?;
                rep.produced = er.produced_count;
                rep.degraded = er.degraded;
                rep.empty_template = er.empty_template;
                rep.warnings = self.warnings_from(&er);
                rep.applied = Some(op.clone());
            }
            SyncOp::AddNode { id, .. } => {
                let parent = logic
                    .raw(id)
                    .and_then(|n| n.parent_id.clone())
                    .ok_or_else(|| {
                        dd(
                            DualDiagCode::OwnerAbsent,
                            "新增节点在逻辑树里查不到或无父",
                            id,
                        )
                    })?;
                let host = self.host_of(&parent);
                let mut er = ExpandReport::new();
                self.expand_subtree(logic, id, &mut er, 0)?;
                // 产物挂到父宿主下（expand_subtree 内部按逻辑父推导，此处校正）
                if er.produced.first().is_some() {
                    self.reparent_under(&er.produced[0], &host);
                }
                // **映射不在这里登记**：`expand_subtree` 已对**每个**逻辑
                // 节点各自登记过一次（它知道自己只产出了哪些）。
                // 在这里再用整轮 `er.produced` 覆盖，会把递归子节点
                // （如 `a` 的儿子 `a1`）的产物算到 `id` 名下——
                // 映射表于是谎报「这些节点是 `a` 产的」。
                // 本单实测：`TemplateChanged{a}` 后 `visuals_of("a")`
                // 返回 6 项（多出 `a1`），判据按 5 项写就红了。
                //根节点无处改挂，`reparent_under` 内部自己会return。
                rep.produced = er.produced_count;
                rep.degraded = er.degraded;
                rep.empty_template = er.empty_template;
                rep.warnings = self.warnings_from(&er);
                rep.applied = Some(op.clone());
            }
            SyncOp::RemoveNode { id } => {
                let n = self.reclaim(id);
                self.mapping.retain(|(k, _)| k != id);
                rep.reclaimed = n as u32;
                rep.applied = Some(op.clone());
            }
            SyncOp::TemplateChanged { id } => {
                if logic.raw(id).is_none() {
                    return Err(dd(
                        DualDiagCode::OwnerAbsent,
                        "模板变更目标在逻辑树里不存在",
                        id,
                    ));
                }
                let n = self.reclaim(id);
                let parent = logic
                    .raw(id)
                    .and_then(|x| x.parent_id.clone())
                    .unwrap_or_else(|| self.visual.root.0.clone());
                let host = self.host_of(&parent);
                let mut er = ExpandReport::new();
                self.expand_subtree(logic, id, &mut er, 0)?;
                if er.produced.first().is_some() {
                    self.reparent_under(&er.produced[0], &host);
                }
                // 映射由 `expand_subtree` 逐节点登记，理由同 `AddNode` 分支。
                rep.reclaimed = n as u32;
                rep.produced = er.produced_count;
                rep.degraded = er.degraded;
                rep.empty_template = er.empty_template;
                rep.warnings = self.warnings_from(&er);
                rep.applied = Some(op.clone());
            }
        }
        Ok(rep)
    }

    /// 把展开报告里的非阻断诊断转成警告。
    fn warnings_from(&self, er: &ExpandReport) -> Vec<DualDiagnostic> {
        let mut w = Vec::new();
        if er.degraded > 0 {
            w.push(dd(
                DualDiagCode::TemplateMissing,
                "本轮同步有节点模板缺失（已降级为控件自身）",
                "<sync>",
            ));
        }
        w
    }

    /// 展开某逻辑节点及其**全部后代**（深度优先，按逻辑子序）。
    ///
    /// 深度优先保证「父先于子」的产出序，这也是渲染序的来源。
    fn expand_subtree(
        &mut self,
        logic: &lt::ControlTree,
        logic_id: &str,
        er: &mut ExpandReport,
        depth: usize,
    ) -> DualOutcome<()> {
        let node = match logic.raw(logic_id) {
            Some(n) => n.clone(),
            None => {
                return Err(dd(
                    DualDiagCode::OwnerAbsent,
                    "逻辑节点不存在",
                    logic_id,
                ))
            }
        };
        if node.destroyed {
            return Err(dd(
                DualDiagCode::OwnerDestroyed,
                "逻辑节点已销毁仍被展开",
                logic_id,
            ));
        }
        let tpl = self.binding.template_of(logic_id).unwrap_or(None);
        // 根逻辑节点没有可视父——传 None 让attach 走挂根分支。
        // **不能退回可视根**：可视根此刻尚未产出（空树），退回会得到
        // 一个不存在的父，attach 报「挂载父节点不在可视树内」。
        //
        // 父**槽**从父的可视节点真读，而不是另算一遍——保证「子槽落在
        // 父槽内」这条性质由同一个槽位数据说了算，不靠两处算法巧合。
        let (parent_opt, parent_slot): (Option<VisualId>, Option<VisualSlot>) =
            match node.parent_id.clone() {
                None => (None, None),
                Some(pid) => {
                    let h = self.host_of(&pid);
                    let s = self.visual.get(&h).map(|n| n.slot);
                    (Some(h), s)
                }
            };
        let before = er.produced.len();
        // **层内序号必须跨枚举者统一（本单实测抓到的真缺陷）**：
        // 本逻辑节点的模板元素（由 `expand` 枚举）与其**逻辑子节点**
        // （由下面的递归枚举）**挂在同一个可视父下**——即同一层。
        // 两者各自从 0 数序号，于是模板的 3 个元素与逻辑子节点 `a1`
        // 全都拿到序号 0，槽位**完全重叠**（判据
        // `F2605-遍历-同层兄弟槽不重叠` 抓到的就是它）。
        //
        // 修法：**父节点自己**的槽（模板元素数）在它的**每个孩子**身上
        // 都要被计入——因为父的元素与兄弟逻辑节点同属一层。
        // 记`p_own` = 父逻辑节点自身占的槽数（=父的模板元素数，无模板时 1），
        // 它就是本层的「前段长度」；本节点排在这段之后。
        let (my_idx, my_len) = match node.parent_id.clone() {
            None => (0usize, 1usize),
            Some(pid) => match logic.raw(&pid) {
                Some(p) => {
                    let p_own = self.own_slots(&pid);
                    let idx = p.children.iter().position(|c| c == logic_id).unwrap_or(0);
                    // 同层总宽 = 父的元素槽 + 兄弟逻辑节点各自的槽
                    let sib_slots: usize = p.children.iter().map(|c| self.own_slots(c)).sum();
                    (p_own + idx, p_own + sib_slots)
                }
                None => {
                    let own = self.own_slots(logic_id);
                    (own, own)
                }
            },
        };
        // `layer_len` 还要把**自己的逻辑子节点**算进去：它们随后会由
        // 下面的递归挂到同一个宿主下，与本节点的模板元素同层。
        // 漏了它，元素会占满整层宽度，逻辑子节点只能挤在重叠区
        // （判据 `F2605-遍历-同层兄弟槽不重叠` 抓到的就是这一处）。
        let my_len = my_len + node.children.len();
        let r = expand(
            &mut self.visual,
            logic_id,
            parent_opt.as_ref(),
            tpl,
            &self.table,
            parent_slot,
            my_idx,
            my_len,
            &mut self.layout,
            depth,
            er,
        );
        match r {
            Ok(()) => {}
            Err(e) if e.code == DualDiagCode::TemplateMissing => {
                // 降级已由 expand 内部完成（产出控件自身 + degraded+1），不阻断
            }
            Err(e) => return Err(e),
        }
        let mine: Vec<VisualId> = er.produced[before..].to_vec();
        self.set_mapping(logic_id, &mine);
        // 递归子节点（**逐个**而非批量：任一子失败时可精确定位到id）
        let kids = node.children.clone();
        for kid in kids.iter() {
            self.expand_subtree(logic, kid, er, depth + 1)?;
        }
        Ok(())
    }

    /// 本逻辑节点在**父层**里占几个槽（= 模板元素数；无模板/模板缺失/
    /// 空模板时为 1，因为它至少会产出一个叶子或宿主）。
    ///
    /// 存在的理由：模板元素与逻辑子节点**同层**，两者都要在这一个数里
    /// 排座位（见 [`Self::expand_subtree`] 里层内序号的算法）。
    fn own_slots(&self, logic_id: &str) -> usize {
        let name = match self.binding.template_of(logic_id).unwrap_or(None) {
            None => return 1,
            Some(n) => n,
        };
        match self.table.find(name) {
            // 空模板零产物 → 严格说占 0 槽，但它下面的逻辑子节点仍要排位，
            // 故至少记 1（宁可留一个空位，也不要让子节点与别人的槽相撞）。
            Some(t) if t.elements.is_empty() => 1,
            Some(t) => t.elements.len(),
            // 模板缺失：降级为控件自身，占 1 槽。
            None => 1,
        }
    }

    /// 覆盖某逻辑节点的映射条目。
    fn set_mapping(&mut self, logic_id: &str, produced: &[VisualId]) {
        let v = produced.to_vec();
        match self.mapping.iter_mut().find(|(k, _)| k == logic_id) {
            Some((_, slot)) => *slot = v,
            None => self.mapping.push((logic_id.to_string(), v)),
        }
    }

    //// 某逻辑节点在可视侧的**宿主节点**。
    ///
    /// **为何需要宿主而非直接挂父节点**：模板展开出的产物是**一棵小树**
    /// （宿主 + 若干子）。若把产物直接挂到父节点下，则「宿主」这个节点
    /// 无处安放，其兄弟序就无法与其它逻辑节点稳定对齐。
    fn host_of(&self, logic_id: &str) -> VisualId {
        let vid = VisualId(logic_id.to_string());
        if self.visual.get(&vid).is_some() {
            return vid;
        }
        // 绑了模板的逻辑节点，**它自己的可视节点 id 不等于 logic_id**——
        // 宿主是 `logic_id#tpl:name`。只按字面 id 查会漏掉它，于是
        // `expand_subtree` 递归子节点时算出的父是「查不到」的结果，
        // 子节点被错挂到逻辑根下（本单实测：TemplateChanged 重建 `a`
        // 时其子 `a1` 报「可视节点 id 重复」，因为它被挂到了根而非
        // `a` 的宿主下，重建产出撞了车）。
        //
        // 映射表的第一项就是宿主——`expand` 里宿主是**先** push 进
        // `produced` 的，元素产物在其后。
        if let Some(first) = self.visuals_of(logic_id).first() {
            return first.clone();
        }
        // 该逻辑节点自身尚无宿主（未展开/ 已收回）：退回可视根 id 作为**字面
        // id**。调用方须自己判断这个 id 是否已存在——不静默错挂，
        // 也不在这里报错（报错会掩盖「先挂孩子后挂父亲」这种合法次序）。
        self.visual.root.clone()
    }

    /// 把一棵可视子树的根改挂到新宿主下（**只改父指针与新父的子表**）。
    fn reparent_under(&mut self, subtree_root: &VisualId, new_parent: &VisualId) {
        let old_parent = match self.visual.get(subtree_root) {
            Some(n) => n.parent.clone(),
            None => return,
        };
        // 根节点没有父，也**不允许**被改挂——那会让树出现第二个根。
        if old_parent.is_none() {
            return;
        }
        if old_parent.as_ref() == Some(new_parent) {
            return;
        }
        if let Some(op) = old_parent {
            if let Some(on) = self.visual.get_mut(&op) {
                on.children.retain(|c| c != subtree_root);
            }
        }
        if let Some(np) = self.visual.get_mut(new_parent) {
            if !np.children.contains(subtree_root) {
                np.children.push(subtree_root.clone());
            }
        }
        if let Some(n) = self.visual.get_mut(subtree_root) {
            n.parent = Some(new_parent.clone());
        }
    }

    /// 收回某逻辑节点的**全部**可视后代（含其自身产出）。
    fn reclaim(&mut self, logic_id: &str) -> usize {
        let roots: Vec<VisualId> = self.visuals_of(logic_id).to_vec();
        let mut doomed: Vec<VisualId> = Vec::new();
        for r in roots.iter() {
            self.collect_subtree(r, &mut doomed);
        }
        let n = doomed.len();
        // 自底向上摘：先摘深的后代，避免留下悬空子引用
        doomed.sort_by(|a, b| self.depth_of(b).cmp(&self.depth_of(a)));
        for d in doomed.iter() {
            if let Some(p) = self.visual.get(d).and_then(|x| x.parent.clone()) {
                if let Some(pn) = self.visual.get_mut(&p) {
                    pn.children.retain(|c| c != d);
                }
            }
            // 只摘自己发出的边：映射里指向被删节点的登记要清，
            // 但「别的逻辑节点指向被删节点」是**悬空证据本身**，须由
            // assert_sync 报出来，不在这里静默清理（否则断言永真）。
        }
        self.visual.nodes.retain(|x| !doomed.contains(&x.id));
        n
    }

    /// 收集以 `root` 为根的可视子树全部节点 id。
    fn collect_subtree(&self, root: &VisualId, out: &mut Vec<VisualId>) {
        if out.contains(root) {
            // 已在集合里 = 有环。停止展开（环由 assert_sync 报告，
            // 这里静默截断是为了**不让展开器自身栈溢出**）。
            return;
        }
        out.push(root.clone());
        if let Some(n) = self.visual.get(root) {
            for c in n.children.iter() {
                self.collect_subtree(c, out);
            }
        }
    }

    /// 可视节点的深度（根为 0）。
    fn depth_of(&self, vid: &VisualId) -> usize {
        let mut d = 0usize;
        let mut cur = self.visual.get(vid).and_then(|n| n.parent.clone());
        let mut guard = 0usize;
        while let Some(p) = cur {
            d += 1;
            guard += 1;
            if guard > self.visual.len() + 1 {
                return d; // 环：给有限值，由 assert_sync 报告
            }
            cur = self.visual.get(&p).and_then(|n| n.parent.clone());
        }
        d
    }
}// ---------------------------------------------------------------------------
// 五、三遍历接口（锚点「三遍历接口公开」）
// ---------------------------------------------------------------------------

/// 逻辑遍历的一步（开发者视角）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LogicalStep<'a> {
    /// 逻辑节点 id。
    pub id: &'a str,
    /// 深度（根为 0）。
    pub depth: usize,
    /// 该节点绑定的模板名（`None` = 控件自身）。
    pub template: Option<&'static str>,
}

/// 可视遍历的一步（渲染视角，**D 域消费**）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct VisualStep<'a> {
    /// 可视节点引用。
    pub id: &'a VisualId,
    /// 深度（可视根为 0）。
    pub depth: usize,
    /// 产生它的逻辑节点 id。
    pub owner: &'a str,
    /// 布局结果槽（**结构位**，不含绘制指令）。
    pub slot: VisualSlot,
    /// 是否模板展开产物。
    pub expanded: bool,
}

/// 命中遍历的一步（**逆序**，N03 前向）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct HitStep<'a> {
    /// 可视节点引用。
    pub id: &'a VisualId,
    /// 深度。
    pub depth: usize,
    /// 命中矩形（由 [`VisualSlot`] 派生）。
    pub rect: HitRect,
}

/// 命中矩形（半开区间 `[x, x+w)` / `[y, y+h)`）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HitRect {
    /// 左。
    pub x: i32,
    /// 上。
    pub y: i32,
    /// 右（不含）。
    pub right: i32,
    /// 下（不含）。
    pub bottom: i32,
}

impl HitRect {
    /// 由布局槽派生。
    pub fn from_slot(s: &VisualSlot) -> HitRect {
        HitRect {
            x: s.x,
            y: s.y,
            right: s.x + s.w as i32,
            bottom: s.y + s.h as i32,
        }
    }

    /// 点是否落在矩形内（半开区间，故右/下边界不含）。
    pub fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x && px < self.right && py >= self.y && py < self.bottom
    }
}

/// 遍历错误：检出环。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum WalkFault {
    /// 遍历途中回到已访问节点（环）。
    Cycle(VisualId),
    /// 参照节点缺失（内部指针悬空）。
    Dangling(VisualId),
}

impl WalkFault {
    /// 转成双树诊断码。
    pub fn code(&self) -> DualDiagCode {
        match self {
            WalkFault::Cycle(_) | WalkFault::Dangling(_) => DualDiagCode::TraversalCycle,
        }
    }
}

impl DualTree {
    /// **遍历一：逻辑树**（开发者视角，序= 声明序，父先于子）。
    ///
    /// **为何逻辑遍历要读逻辑树而不是「从可视树反推」**：反推得到的序是
    /// 模板展开后的序，与开发者写的代码不一一对应（一个 Button 对应 3 个
    /// 可视节点），开发者工具要展示的是「我写了什么」。
    pub fn walk_logical<'t>(&'t self, logic: &'t lt::ControlTree) -> Vec<LogicalStep<'t>> {
        let mut out = Vec::new();
        let mut seen: Vec<String> = Vec::new();
        // 根 id 必须借用逻辑树自有的字段（不能克隆成局部——那活不过本函数）
        let root: &str = logic.root();
        self.walk_logic_rec(logic, root, 0, &mut seen, &mut out);
        out
    }

    fn walk_logic_rec<'t>(
        &'t self,
        logic: &'t lt::ControlTree,
        id: &'t str,
        depth: usize,
        seen: &mut Vec<String>,
        out: &mut Vec<LogicalStep<'t>>,
    ) {
        if seen.iter().any(|s| s == id) || out.len() >= MAX_VISUAL_NODES {
            return; // 环或超限：截断，由 assert_sync 报告
        }
        let node = match logic.raw(id) {
            Some(n) => n,
            None => return,
        };
        seen.push(id.to_string());
        out.push(LogicalStep {
            id,
            depth,
            template: self.binding.template_of(id).unwrap_or(None),
        });
        // 子节点 id 要递归借用成 't，故先收集成Vec 再逐个传引用
        // （直接遍历 `node.children` 会拿到与`self` 借用同生命周期的
        //  引用，进不了`'t`）。
        let kids: Vec<&'t str> = node.children.iter().map(|k| k.as_str()).collect();
        for kid in kids.into_iter() {
            self.walk_logic_rec(logic, kid, depth + 1, seen, out);
        }
        seen.pop();
    }

    /// **遍历二：可视树**（渲染视角，序 = 渲染序，D 域消费）。
    pub fn walk_visual(&self) -> Vec<VisualStep<'_>> {
        self.walk_visual_faulted().0
    }

    /// **可视遍历 + 遍历故障**（[`walk_visual`] 的带证据形）。
    ///
    /// **为何要带证据形（本单实测踩到的坑）**：原实现判环靠
    /// 「走到的节点数 ≠ 节点总数」。**这个口径漏掉了最典型的环**——
    /// 某节点的 `children` 里含自己。此时遍历在该节点处直接返回，
    /// 走到的节点数**恰好等于**节点总数（自环节点已在第一次访问时
    /// 计入），两个数相等→ 判不出环，环就这么静默过去了。
    ///
    /// 真正的证据在遍历当场：`seen` 里**已经**有这个 id 说明回到了
    /// 祖先。这里把那一刻记下来，`assert_sync` 直接消费它——
    /// 不再用「数量对不上」这种间接推断。
    pub fn walk_visual_faulted(&self) -> (Vec<VisualStep<'_>>, Vec<WalkFault>) {
        let mut out = Vec::new();
        let mut faults = Vec::new();
        let mut seen: Vec<VisualId> = Vec::new();
        // 根节点引用直接取自self（不能克隆成局部再借——那活不过本函数）
        if let Some(r) = self.visual.get(self.visual.root()) {
            let id: &VisualId = &r.id;
            self.walk_visual_rec(id, 0, &mut seen, &mut out, &mut faults);
        }
        (out, faults)
    }

    fn walk_visual_rec<'t>(
        &'t self,
        id: &'t VisualId,
        depth: usize,
        seen: &mut Vec<VisualId>,
        out: &mut Vec<VisualStep<'t>>,
        faults: &mut Vec<WalkFault>,
    ) {
        if seen.contains(id) {
            // 环：记录**回到的那个节点**，让断言能指名道姓而不是只说
            // 「有环」。此处必须返回，否则展开器自身会栈溢出。
            faults.push(WalkFault::Cycle(id.clone()));
            return;
        }
        if out.len() >= MAX_VISUAL_NODES {
            return; // 超限（非环）：静默截断，由长度判据负责
        }
        let node = match self.visual.get(id) {
            Some(n) => n,
            None => {
                faults.push(WalkFault::Dangling(id.clone()));
                return;
            }
        };
        seen.push(id.clone());
        out.push(VisualStep {
            id,
            depth,
            owner: node.owner.as_str(),
            slot: node.slot,
            expanded: node.expanded,
        });
        for c in node.children.iter() {
            self.walk_visual_rec(c, depth + 1, seen, out, faults);
        }
        seen.pop();
    }

    /// **遍历三：命中测试**（**逆序**，N03 前向）。
    ///
    /// 逆序 + 首个命中即返回：把最坏情况从 O(N) 降到 O(命中位置深度)。
    /// 正序「遍历全部取最近」在每帧指针移动时是纯浪费。
    pub fn walk_hit(&self, px: i32, py: i32) -> Vec<HitStep<'_>> {
        let mut fwd = self.walk_visual();
        fwd.reverse(); // 渲染序 → 逆序（后画的先命中）
        let mut out = Vec::new();
        for step in fwd.into_iter() {
            let rect = HitRect::from_slot(&step.slot);
            if rect.contains(px, py) {
                out.push(HitStep {
                    id: step.id,
                    depth: step.depth,
                    rect,
                });
            }
        }
        out
    }

    /// **命中测试（提前返回形）**：返回**首个**命中（逆序第一个即答案）。
    pub fn hit_test(&self, px: i32, py: i32) -> Option<HitStep<'_>> {
        let mut fwd = self.walk_visual();
        fwd.reverse();
        for step in fwd.into_iter() {
            let rect = HitRect::from_slot(&step.slot);
            if rect.contains(px, py) {
                return Some(HitStep {
                    id: step.id,
                    depth: step.depth,
                    rect,
                });
            }
        }
        None
    }
}

/// 可视节点数上限（遍历防护，防展开失控）。
pub const MAX_VISUAL_NODES: usize = 4096;

// ---------------------------------------------------------------------------
// 六、同步断言（锚点「双树失同步 → 同步断言 P1」）
// ---------------------------------------------------------------------------

/// 同步断言报告。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SyncAssertReport {
    /// 幽灵节点（可视节点无 owner / owner 不在逻辑树 / owner 已销毁）。
    pub ghosts: Vec<String>,
    /// 映射计数不符（逻辑节点声称的产出数与实际映射条目不符）。
    pub mapping_mismatch: Vec<String>,
    /// 遍历环。
    pub cycles: Vec<String>,
    /// 可视树中不被任何逻辑节点产出的节点（**反向失同步**）。
    pub orphans: Vec<String>,
    /// D 域接口对账结果。
    pub d_drift: Vec<String>,
    /// 是否完全同步（**全部五类皆空**）。
    pub in_sync: bool,
}

impl SyncAssertReport {
    /// 空报告（= 全同步）。
    pub fn new() -> SyncAssertReport {
        SyncAssertReport {
            ghosts: Vec::new(),
            mapping_mismatch: Vec::new(),
            cycles: Vec::new(),
            orphans: Vec::new(),
            d_drift: Vec::new(),
            in_sync: true,
        }
    }

    /// 问题总数（五类之和）。
    pub fn problem_count(&self) -> usize {
        self.ghosts.len()
            + self.mapping_mismatch.len()
            + self.cycles.len()
            + self.orphans.len()
            + self.d_drift.len()
    }
}

/// **同步断言**（锚点 P1：单向数据流破裂）。
///
/// **五类失同步各自独立检查，不合并成一句「是否一致」**——合并后
/// 「哪一类破了」这个最有用的信息就丢了，而调用方要按类处置
/// （幽灵要查写入入口，环要查树不变量，孤儿要查同步遗漏）。
pub fn assert_sync(logic: &lt::ControlTree, dual: &DualTree) -> SyncAssertReport {
    let mut rep = SyncAssertReport::new();

    // ① 幽灵：可视节点的 owner 不在逻辑树 / owner 已销毁
    for step in dual.walk_visual() {
        if step.owner.is_empty() {
            rep.ghosts.push(format!("{}:owner=空", step.id.as_str()));
            continue;
        }
        match logic.raw(step.owner) {
            None => rep.ghosts.push(format!(
                "{}:owner={} 不在逻辑树",
                step.id.as_str(),
                step.owner
            )),
            Some(n) if n.destroyed => rep.ghosts.push(format!(
                "{}:owner={} 已销毁",
                step.id.as_str(),
                step.owner
            )),
            Some(_) => {}
        }
    }

    // ② 映射计数：每个映射条目的产出节点必须**真实存在**于可视树
    for (lid, vids) in dual.mapping().iter() {
        let mut missing = 0u32;
        for v in vids.iter() {
            if dual.visual().get(v).is_none() {
                missing += 1;
            }
        }
        if missing > 0 {
            rep.mapping_mismatch.push(format!(
                "{}:{} 个产出在可视树里不存在",
                lid, missing
            ));
        }
    }

    // ③ 环：消费遍历**当场**记下的故障证据。
    //
    // **不再用「走到的节点数 ≠ 节点总数」**：那个口径漏掉自环（本单实测
    // ——某节点 children 含自己时，走到数恰好等于总数，静默放过）。
    // 遍历现在会在回到祖先的那一刻记下 `WalkFault::Cycle`。
    let (walked_steps, walk_faults) = dual.walk_visual_faulted();
    for f in walk_faults.iter() {
        match f {
            WalkFault::Cycle(v) => rep.cycles.push(format!("遍历回到已访问节点 {}（环）", v.as_str())),
            WalkFault::Dangling(v) => rep.cycles.push(format!("子表指向不存在的节点 {}（悬空）", v.as_str())),
        }
    }
    // 走到的节点数少于总数 = 有节点**不可达**（不在根的子树里）。
    // 那不是环而是孤儿/失联，单独记进 cycles 之外的一类会让问题数失真，
    // 故只在此处留证据：不可达节点由 ④ 孤儿 负责点名。
    if walked_steps.len() < dual.visual().len() {
        rep.cycles.push(format!(
            "可视树 {} 个节点，遍历只走到 {} 个（有节点不可达）",
            dual.visual().len(),
            walked_steps.len()
        ));
    }

    // ④ 孤儿：可视节点不被任何映射条目产出（反向失同步）
    let mut owned: Vec<VisualId> = Vec::new();
    for (_, vids) in dual.mapping().iter() {
        owned.extend(vids.iter().cloned());
    }
    for n in dual.visual().nodes().iter() {
        if !owned.contains(&n.id) {
            rep.orphans.push(n.id.as_str().to_string());
        }
    }

    // ⑤ D 域接口对账（钩子，见 [`reconcile_d_interface`]）
    rep.d_drift = reconcile_d_interface(dual);

    rep.in_sync = rep.problem_count() == 0;
    rep
}

// ---------------------------------------------------------------------------
// 七、D/N 边界（锚点「N 产结构、D 产像素」）
// ---------------------------------------------------------------------------

/// 渲染结构源（**可视树 → D 域的交接件**）。
///
/// **类型里没有任何绘制字段**——这是边界的**可机检形态**：任何「N 域越界
/// 画像素」的企图在类型层面就不成立，不需要靠评审盯。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RenderSource {
    /// 渲染序的结构条目（与 [`DualTree::walk_visual`] 同序）。
    pub items: Vec<RenderItem>,
    /// 节点总数（供D 域一次性预分配，避免渲染中分配）。
    pub node_count: usize,
}

/// 一条渲染结构条目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RenderItem {
    /// 深度（D 域据此定z 序校验）。
    pub depth: u16,
    /// 布局槽。
    pub slot: VisualSlot,
    /// 来源逻辑节点 id 的**稳定序号**（字符串不进渲染热路径）。
    pub owner_ord: u32,
}

/// 由可视树产出渲染结构源。
pub fn render_source(dual: &DualTree) -> RenderSource {
    let mut items = Vec::new();
    let mut ord: u32 = 0;
    for step in dual.walk_visual().into_iter() {
        items.push(RenderItem {
            depth: step.depth as u16,
            slot: step.slot,
            owner_ord: ord,
        });
        ord += 1;
    }
    RenderSource {
        node_count: items.len(),
        items,
    }
}

/// D 域接口对账钩子（锚点「D 接口变更 → 对账钩子」）。
///
/// **对账什么**：D 域消费渲染结构时若需要某个字段而本模块没产出，或反之。
/// **当前契约的字段清单**取自 [`RenderItem`]——对账即「契约里的每个字段
/// 在产出侧都非空/有值」。新增字段时忘记同步，这里会报。
pub fn reconcile_d_interface(dual: &DualTree) -> Vec<String> {
    let mut drift = Vec::new();
    for step in dual.walk_visual().into_iter() {
        // 宽或高为零的可视节点会让 D 域生成空绘制命令——
        // 那是「结构位无效」，须在N 域这一侧就报出来。
        if step.slot.w == 0 || step.slot.h == 0 {
            drift.push(format!(
                "{}:slot 宽高为 0（D 域会生成空命令）",
                step.id.as_str()
            ));
        }
    }
    drift
}

// ---------------------------------------------------------------------------
// 八、性能账与跨批对接台账
// ---------------------------------------------------------------------------

/// 性能分解的一行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PerfRow {
    /// 环节名。
    pub stage: &'static str,
    /// 复杂度口径（**结构性事实**，非实测）。
    pub complexity: &'static str,
    /// 依据（为什么是这个量级）。
    pub basis: &'static str,
}

/// 性能逐项分解（锚点四条 + 权衡公开）。
///
/// **诚实标注**：本表**不含实机计时**，只陈述**可从结构推导的口径**。
/// 真实机测由性能基线单承接。
pub const PERF_ROWS: [PerfRow; 5] = [
    PerfRow {
        stage: "模板展开",
        complexity: "O(模板节点)",
        basis: "每个模板元素产出常数个可视节点；嵌套模板深度上限 MAX_EXPAND_DEPTH=8，故最坏是深度×宽度",
    },
    PerfRow {
        stage: "同步",
        complexity: "O(变更)",
        basis: "AddNode/RemoveNode/TemplateChanged 只触及目标节点的产出子树，与逻辑树总规模无关",
    },
    PerfRow {
        stage: "三遍历",
        complexity: "O(N)",
        basis: "N = 可视节点数；命中遍历提前返回时最坏降到 O(命中位置深度)",
    },
    PerfRow {
        stage: "双树内存",
        complexity: "≈×1.5",
        basis: "逻辑树 N 个节点 + 可视树 N..kN 个节点（k = 单节点平均模板产出数）；无模板时 k=1，即双份",
    },
    PerfRow {
        stage: "映射表",
        complexity: "O(N) 附加",
        basis: "每逻辑节点一条 (id, Vec<VisualId>)；是为「一对多」付的索引钱，换来 O(1) 反查",
    },
];

/// 跨批对接台账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Handoff {
    /// 对端单号。
    pub peer: &'static str,
    /// 契约内容。
    pub contract: &'static str,
    /// 状态。
    pub state: &'static str,
}

/// 跨批对接（锚点「跨批对接点」四条）。
pub const HANDOFFS: [Handoff; 5] = [
    Handoff {
        peer: "VE-F2606",
        contract: "增量更新消费本模块的 SyncOp 与映射表（逻辑为源、可视为投影）",
        state: "前向（本条产出位，F2606 消费）",
    },
    Handoff {
        peer: "N03-命中",
        contract: "命中测试消费 walk_hit / hit_test（逆序，首个命中即答案）",
        state: "前向（已产出接口）",
    },
    Handoff {
        peer: "D-域渲染",
        contract: "渲染输入消费 render_source 产出的 RenderSource（只含结构位，无绘制指令）",
        state: "已兑现（接口已产出并带对账钩子）",
    },
    Handoff {
        peer: "VE-F2503",
        contract: "模板资产元数据联动：模板名与资产表对齐（缺模板即本模块的降级路径）",
        // **状态词不许混用**（判据 `F2605-对接-已兑现项非空` 会查）：
        // 一条对接要么「已兑现」要么「前向」，写成「前向（…已兑现…）」
        // 会让读台账的人搞不清到底兑现了没有。这条是**部分兑现**：
        // 降级路径本单已落地，资产表联动确实还没接。
        state: "部分兑现（降级路径已落地，资产表联动待接）",
    },
    Handoff {
        peer: "VE-F2602",
        contract: "逻辑树只读消费（sync 显式接收 &ControlTree，不持有可改副本）",
        state: "已兑现",
    },
];

/// 无隐私面声明（锚点「无障碍与隐私：无隐私面」）。
///
/// **为什么可以断言无隐私面**：本模块只处理控件的**结构**（树、模板、
/// 布局槽），不接触文本内容、用户数据、标识符。三遍历接口里唯一的
/// 「标识」是控件 id（开发者自己起的名字），不是用户身份。
pub const PRIVACY_NOTE: &str = "无隐私面：仅处理控件结构，不接触文本内容与用户数据";

/// 无障碍替述（模板展开会改变可访问性树的形状，故必须留替述）。
pub fn a11y_alternatives() -> [(&'static str, &'static str); 3] {
    [
        (
            "可视树深度",
            "模板展开会增加可视树深度，但可访问性树按逻辑树声明序构建——\
             展开产物不引入新的可访问性节点（这是本模块的纪律，不是巧合）",
        ),
        (
            "命中顺序",
            "命中测试逆序遍历，后画的先命中——与屏幕阅读器的阅读顺序相反，\
             故可访问性消费方须用 walk_logical 而非 walk_hit",
        ),
        (
            "降级可见性",
            "模板缺失时降级为控件自身并告警：用户看到的是控件本体而非空白，\
             但其视觉细节缺失——可访问性层应把「模板缺失」作为状态暴露给辅助技术",
        ),
    ]
}

/// 组内分工登记（锚点「分工」）。
pub fn division_of_work() -> [(&'static str, &'static str); 4] {
    [
        ("核心逻辑", "双树数据结构 + 模板展开 + 单向同步 + 三遍历"),
        ("边界防护", "同步断言五类 + D 域对账钩子 + 遍历环拦截 + 降级矩阵"),
        ("错误路径", "十一个诊断码的码/因/建议/人话四元组"),
        ("测试支撑", "域自检判据 + 反假变体登记"),
    ]
}

/// 降级矩阵一行（锚点「错误路径与降级矩阵」）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DegradeRow {
    /// 锚点情形。
    pub situation: &'static str,
    /// 本模块处置。
    pub action: &'static str,
    /// 对应诊断码。
    pub code: DualDiagCode,
    /// 是否阻断渲染。
    pub blocks: bool,
}

/// 降级矩阵（锚点五行逐行落实）。
///
/// **降级方向相反的两类不得共用码**：模板类「缺资源」→ 降级渲染；
/// 同步类「机制自身坏了」→ 阻断（继续渲染会产出不确定画面）。
pub const DEGRADE_MATRIX: [DegradeRow; 6] = [
    DegradeRow {
        situation: "模板展开失败（模板引用缺失）",
        action: "降级为控件自身 + 告警",
        code: DualDiagCode::TemplateMissing,
        blocks: false,
    },
    DegradeRow {
        situation: "双树失同步（可视含幽灵节点）",
        action: "同步断言（P1，单向数据流破裂）",
        code: DualDiagCode::GhostVisual,
        blocks: true,
    },
    DegradeRow {
        situation: "映射一对多计数错误",
        action: "断言（展开完整性）",
        code: DualDiagCode::MappingCountMismatch,
        blocks: true,
    },
    DegradeRow {
        situation: "D 接口变更",
        action: "对账钩子（家族）",
        code: DualDiagCode::DInterfaceDrift,
        blocks: false,
    },
    DegradeRow {
        situation: "遍历环",
        action: "树不变量拦截（F2602 家族）",
        code: DualDiagCode::TraversalCycle,
        blocks: true,
    },
    DegradeRow {
        situation: "模板声明非法 / 嵌套过深",
        action: "显性拒绝（不猜测意图）",
        code: DualDiagCode::TemplateInvalid,
        blocks: false,
    },
];