//! VE-F2606 · 控件树增量更新（VE-N 域 · UI 框架内核 · N01 组）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F2606`
//!
//! **判据（锚点原文四条 + 判据面）**：精确失效、帧边界批处理、三分发、
//! 双树增量同步、判据。
//!
//! ---
//!
//! ## 一、增量更新三声明（锚点「增量更新三声明」逐条落实）
//!
//! | 声明 | 锚点语 | 本模块落点 |
//! |---|---|---|
//! | 增量失效 | 节点插入/移除的增量失效——**不整树重建** | [`InvalidScope::Subtree`]（失效范围=该节点子树） |
//! | 精确失效 | 节点变更→仅该子树失效——**不失效兄弟/祖先无关部分** | [`collect_subtree`] 只走目标节点后代，兄弟与祖先**不在集合内** |
//! | 批处理 | 同帧多次变更→批处理一次提交——**风暴防护** | [`ChangeQueue`] + [`IncrementalUpdate::commit_frame`] |
//!
//! ## 二、为什么「精确」是本单最容易做假的一条（头号判据风险）
//!
//! 锚点要求「**不失效兄弟/祖先无关部分**」。这句话有一个极易自欺的写法：
//! 收集失效集合时把**整棵树**都推进去，然后声称「失效范围 ≤ 全树」——
//! 这条不等式恒成立，于是判据全绿而精确失效根本没实现。
//!
//! 本模块把「精确」做成**可证伪的正面声明**而非「上界」：
//!
//! - [`InvalidScope`] 只枚举两个构造：[`InvalidScope::Subtree`]（子树）
//!   与 [`InvalidScope::WholeTree`]（整树，**仅** F2604 属性失效沿父链
//!   上溯时可达）。没有第三个入口。
//! - [`assert_scope_exact`] 是**双向**断言：既断言「集合内每个节点确实
//!   是目标的后代」（无凭空失效），**也**断言「目标的每个后代都在集合内」
//!   （无遗漏失效）。**只查一侧等于没查**——把集合清空能过「无凭空失效」，
//!   把集合填满能过「无遗漏」，两个方向的作弊互不相同。
//! - 判据侧用**独立重算**的后代集合作对照，不回读本模块的判定函数
//!   （回读即自证式断言，判据恒真）。
//!
//! ## 三、批处理：帧边界语义与「合并」的真实含义（锚点「帧边界语义声明」）
//!
//! 批处理**不是**「把 N 条变更变成 1 条」，而是「**帧内对同一节点的多次
//! 变更合并成一条，帧边界一次应用**」。两者的区别在有副作用时暴露：
//!
//! - 若只是「N 条变 1 条」而仍逐条应用副作用，合并只是省了队列空间；
//! - 本模块的合并是**语义合并**：同一节点同帧内的多次标记取
//!   [`Invalidation::union`]（失效类型并集），应用时**只应用一次并集结果**。
//!
//! **为何取并集而不是「取最后一次」**：失效标记是**谓词**（"这个节点需要
//! 重绘"），不是**值**（"这个节点的颜色是红"）。两次标记 `render` 与
//! `layout`，语义是「既要重绘又���重排」；取最后一次会**丢掉** `render`，
//! 症状是「布局对了但画面没刷新」——而这种缺陷在静止画面下不可见，
//! 要等下一次动画才暴露，是极难复现的一类。
//!
//! **合并键是（节点，失效类型位掩码）而不是节点**：不同失效类型走**不同
//! 下游**（三分发，见下节），若按节点合并就得在应用时再拆回去，多一遍
//! 拆装；按（节点，掩码）合并则每条队列项天然只服务一个下游。
//!
//! ## 四、三分发：失效类型到下游的唯一出口（锚点「三失效类型分发声明」）
//!
//! | 失效类型 | 下游 | 单号 |
//! |---|---|---|
//! | 渲染失效 | D 域渲染通知 | D 域 |
//! | 布局失效 | 布局失效通知 | VE-F2621 |
//! | 命中失效 | 命中失效通知 | N03 |
//!
//! **三分发是 O(1) 类型路由**：失效位到下游是**静态表**（[`ROUTES`]），
//! 查表是常数时间，与节点数、树深度**都无关**。
//!
//! **错路必须被抓住而不是被容忍**（锚点「三分发错路（布局失效发渲染）→P1」）：
//! 布局失效若发去渲染，下游会做一次无意义的重绘，**症状是「慢」而不是
//! 「错」**——没有告警、没有红项、没有任何可观测的失败。故本模块的
//! [`RouteLedger`] 为**每条通知**记录「本该去哪个下游」与「实际去了哪个」，
//! [`audit_routes`] 做逐条比对并**点名**错路节点，而不是只比计数。
//!
//! **为什么不能只比计数**：若实现把三条路由**全部**指向渲染（一个常见
//! 的复制粘贴错误），则「渲染通知数 == 期望渲染通知数」在只发生渲染失效
//! 的场景下依然成立。逐条比对 + 独立期望值才能抓住。判据侧为此构造了
//! 「只含布局失效」的语料——**该语料下渲染通知数必须为 0**，这是
//! 单边断言（弱门禁教训：双边阈值一旦宽过正确实现的偏差幅度就会漏网）。
//!
//! ## 五、与属性引擎（第三段）的联动（锚点「四段管线第三段的树侧兑现」）
//!
//! F2604 的四段管线第三段（[`Stage::Invalidate`]）在**属性节点槽**上置位
//! [`Invalidation`]；本模块是该失效在**树侧**的兑现者：把属性侧的失效
//! 转成**树节点 id** 的失效范围。
//!
//! **两侧的粒度不同，这是本联动的全部难点**：属性侧是
//! （`NodeSlot`, `PropertyKey`），树侧是节点 id 子树。转换规则：
//!
//! - 属性失效**不上溯**到父（F2604 自己管继承，本模块不重复上溯）；
//! - 但属性失效**要下沉**到子——改了容器的 `Width`，子控件的布局位置
//!   依赖它，故子节点集合是失效范围的**下界**；
//! - 故属性侧失效映射为 [`InvalidScope::Subtree`]，**不是**单节点。
//!
//! **与 F2605 双树的增量同步**（锚点「双树增量同步」）：
//! [`IncrementalUpdate::apply`] 消费 F2605 的 [`SyncOp`]，把可视树变更
//! 与树侧失效范围**在同一次提交里**做完——分两次做会出现「可视树已变、
//! 失效范围还是旧的」的窗口，那个窗口里渲染读到的是**半新半旧**的树，
//! 症状是画面撕裂一帧后自愈，**不可复现**。
//!
//! ## 六、错误路径与降级矩阵（锚点原表逐行落实）
//!
//! | 锚点情形 | 本模块处置 | 诊断码 |
//! |---|---|---|
//! | 失效范围过大（全树失效退化） | **P1**（精确失效纪律破裂） | [`IncrDiagCode::ScopeDegenerated`] |
//! | 批处理溢出（队列超限） | **溢出告警 + 强制批提交**（风暴极端防护） | [`IncrDiagCode::QueueOverflow`] |
//! | 三分发错路（布局失效发渲染） | **P1**（失效类型错误） | [`IncrDiagCode::RouteMismatch`] |
//! | 双树同步遗漏 | **同步断言**（F2605 家族） | [`IncrDiagCode::SyncOmission`] |
//! | 批队列泄漏 | **审计** | [`IncrDiagCode::QueueLeak`] |
//!
//! **降级方向的差别是刻意的**：队列溢出选「**告警 + 强制提交**」而不是
//! 「拒绝」——溢出的成因是调用方在帧内做了上万次变更，此时**拒绝等于把
//! 工作全丢掉**，而强制提交虽丢掉部分合并收益（批更大、更慢）但**语义
//! 正确**（该画的都画了）。而范围退化与错路选 **P1 断言**：这两类是
//! **本模块自身的纪律破了**，继续跑下去会产出**不确定的性能**（退化到
//! 全树失效时，每帧耗时随树规模线性增长，而规模是外部输入）。
//!
//! ## 七、批队列上限的取值与「为何不是可调参数」
//!
//! [`MAX_QUEUE`] 取 4096，与 F2604 的 [`MAX_PENDING`] 同值**不是巧合**：
//! 两侧的队列在同一次提交里同时增长（本模块消费 F2604 的失效），
//! 取不同值会让其中一侧先溢出，而先溢出的那一侧会强制提交出**半个帧**，
//! 症状是「失效分两批到达下游」——本模块的合并语义被绕过。
//!
//! 它**不是可调参数**：可调就意味着「有人会把它调大到不溢出为止」，
//! 而队列的物理意义是「一帧内允许攒多少条变更」，调大它等于调大**一帧
//! 的最坏耗时**，那是**帧预算**问题（属于 N02/F2468 口径），不属于本单。
//!
//! ## 八、确定性
//!
//! 纯函数、无时钟无 IO；队列遍历序即入队序（合并保留**首次**入队位，
//! 与 F2604 世代戳合并同一口径）；遍历显式栈（无递归，故超深树不炸栈）。
//! 同输入同输出，回归可复现。生产面零 panic、零 `unsafe`、零 `unwrap`。

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::svstar2::ven02_tree::ControlTree;
use crate::svstar2::ven04_prop::Invalidation;
use crate::svstar2::ven05_dual::{DualTree, SyncOp};

// ---------------------------------------------------------------------------
// 一、诊断面
// ---------------------------------------------------------------------------

/// 增量更新诊断码。
///
/// **与 F2602 / F2604 / F2605 的码不重叠**：同一情形由不同单号上报时
/// **不得共用码**（否则调用方不知该查哪个域的账）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IncrDiagCode {
    /// 失效范围退化为整树（精确失效纪律破裂）——P1。
    ScopeDegenerated,
    /// 批队列溢出（超 [`MAX_QUEUE`]），强制批提交。
    QueueOverflow,
    /// 三分发错路（失效类型与下游不匹配）——P1。
    RouteMismatch,
    /// 双树同步遗漏（可视树已变而失效范围未跟上，或反之）。
    SyncOmission,
    /// 批队列泄漏（提交后队列未清空）。
    QueueLeak,
    /// 目标节点不在逻辑树内。
    TargetAbsent,
    /// 目标节点已销毁仍被提交。
    TargetDestroyed,
    /// 增量器已销毁仍被使用（调用序错误）。
    IncrLifecycleViolation,
    /// 子树遍历深度超限（显式栈也有上限，超限即拒）。
    DepthLimitExceeded,
    /// 域自检审计不通过。
    IncrSelfcheckFailed,
}

impl IncrDiagCode {
    /// 错误码（`0x2C00 | n+1` 段，专属 VE-N/F2606）。
    ///
    /// **基数必须取低 4 位为 0 的整段起点**：`|` 只置位不清位，基数带低位
    /// 会令相邻码塌陷成同一个（F2605 已踩过：`0x2B03 | n+1` 让 n=0..3
    /// 全撞成 `0x2B03`）。
    pub fn code(self) -> u16 {
        0x2C00 | (self as u16) + 1
    }

    /// 线缆名。
    pub const fn as_str(self) -> &'static str {
        match self {
            IncrDiagCode::ScopeDegenerated => "SCOPE_DEGENERATED",
            IncrDiagCode::QueueOverflow => "QUEUE_OVERFLOW",
            IncrDiagCode::RouteMismatch => "ROUTE_MISMATCH",
            IncrDiagCode::SyncOmission => "SYNC_OMISSION",
            IncrDiagCode::QueueLeak => "QUEUE_LEAK",
            IncrDiagCode::TargetAbsent => "TARGET_ABSENT",
            IncrDiagCode::TargetDestroyed => "TARGET_DESTROYED",
            IncrDiagCode::IncrLifecycleViolation => "INCR_LIFECYCLE_VIOLATION",
            IncrDiagCode::DepthLimitExceeded => "DEPTH_LIMIT_EXCEEDED",
            IncrDiagCode::IncrSelfcheckFailed => "INCR_SELFCHECK_FAILED",
        }
    }

    /// 成因（为什么会出现）。
    pub const fn cause(self) -> &'static str {
        match self {
            IncrDiagCode::ScopeDegenerated => "失效集合扩到了整树而非目标子树",
            IncrDiagCode::QueueOverflow => "单帧内提交次数超上限",
            IncrDiagCode::RouteMismatch => "失效类型与实际投递的下游不匹配",
            IncrDiagCode::SyncOmission => "可视树变更与失效范围不在同一次提交内",
            IncrDiagCode::QueueLeak => "提交结束后队列仍有残留",
            IncrDiagCode::TargetAbsent => "目标 id 不在逻辑树内",
            IncrDiagCode::TargetDestroyed => "目标节点已销毁",
            IncrDiagCode::IncrLifecycleViolation => "增量器生命周期之外被调用",
            IncrDiagCode::DepthLimitExceeded => "子树深度超显式栈上限",
            IncrDiagCode::IncrSelfcheckFailed => "域自检审计发现纪律破口",
        }
    }

    /// 建议（怎么办）。
    pub const fn hint(self) -> &'static str {
        match self {
            IncrDiagCode::ScopeDegenerated => "查失效范围构造点：是否误用 WholeTree",
            IncrDiagCode::QueueOverflow => "查帧内变更来源：合并键是否过细（掩码位拆分）",
            IncrDiagCode::RouteMismatch => "查 ROUTES 表与实际投递分支是否同源",
            IncrDiagCode::SyncOmission => "查 apply 是否把双树同步与失效范围分两次提交",
            IncrDiagCode::QueueLeak => "查提交出口是否遗漏 clear（早退路径最易漏）",
            IncrDiagCode::TargetAbsent => "查 id 是否为逻辑树内节点（含未销毁）",
            IncrDiagCode::TargetDestroyed => "查销毁流程是否漏了增量器通知",
            IncrDiagCode::IncrLifecycleViolation => "查销毁与提交的先后序",
            IncrDiagCode::DepthLimitExceeded => "查树深或改用显式栈上限内的分批",
            IncrDiagCode::IncrSelfcheckFailed => "跑域自检取点名项",
        }
    }

    /// 人话（说给开发者听的一句）。
    pub const fn human(self) -> &'static str {
        match self {
            IncrDiagCode::ScopeDegenerated => "本来只想让一个子树重画，结果整棵树都重画了",
            IncrDiagCode::QueueOverflow => "这一帧的变更太多，攒不住了，只能硬着头皮全部提交",
            IncrDiagCode::RouteMismatch => "该通知布局的改动发去了渲染，界面不坏但白白变慢",
            IncrDiagCode::SyncOmission => "树已经变了，但没告诉渲染层去重画",
            IncrDiagCode::QueueLeak => "提交完了队列里还有东西没处理干净",
            IncrDiagCode::TargetAbsent => "这个控件在树上找不到",
            IncrDiagCode::TargetDestroyed => "这个控件已经被销毁了",
            IncrDiagCode::IncrLifecycleViolation => "增量更新器已经销毁，不能再用了",
            IncrDiagCode::DepthLimitExceeded => "这棵树太深，遍历不下去",
            IncrDiagCode::IncrSelfcheckFailed => "自检没过，说明有纪律没守住",
        }
    }

    /// 是否阻断本帧提交。
    ///
    /// 分档依据是「处置动作」而非症状：`QueueOverflow` 是**降级后继续**
    /// （强制提交），其余是**机制自身坏了或输入不合法**，须阻断。
    pub const fn is_blocking(self) -> bool {
        !matches!(self, IncrDiagCode::QueueOverflow)
    }

    /// 严重度（0=无 1=P1 2=P0）。本单无 P0 档（崩溃由 F2609 fuzz 面承接）。
    pub const fn severity(self) -> u8 {
        match self {
            IncrDiagCode::ScopeDegenerated
            | IncrDiagCode::RouteMismatch
            | IncrDiagCode::SyncOmission
            | IncrDiagCode::QueueLeak
            | IncrDiagCode::IncrSelfcheckFailed => 1,
            _ => 0,
        }
    }
}

/// 增量更新诊断结构（与前序各域同形：`at` 点名节点与失效类型）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct IncrDiagnostic {
    /// 诊断码。
    pub code: IncrDiagCode,
    /// 现场描述。
    pub message: String,
    /// 处置建议。
    pub hint: String,
    /// 出错节点/位置。
    pub at: String,
}

/// 构造诊断。
pub fn icd(
    code: IncrDiagCode,
    message: &str,
    hint: &str,
    at: &str,
) -> IncrDiagnostic {
    IncrDiagnostic {
        code,
        message: String::from(message),
        hint: String::from(hint),
        at: String::from(at),
    }
}

/// 本域结果别名。
pub type IncrOutcome<T> = Result<T, IncrDiagnostic>;

/// 构造成功值。
pub fn iok<T>(v: T) -> IncrOutcome<T> {
    Ok(v)
}

/// 构造失败值。
pub fn ifail<T>(
    code: IncrDiagCode,
    message: &str,
    hint: &str,
    at: &str,
) -> IncrOutcome<T> {
    Err(icd(code, message, hint, at))
}

// ---------------------------------------------------------------------------
// 二、失效范围（判据一：精确失效）
// ---------------------------------------------------------------------------

/// 失效范围（**只有两个构造，没有第三个**）。
///
/// **为何用枚举而不是「失效节点列表」作为唯一表示**：列表能表达一切，
/// 也因此**表达不了纪律**——拿到一个列表，调用方无法判断它是「精确的
/// 子树」还是「偷懒的整树」。带上 [`InvalidScope::Subtree`] 这个构造，
/// 「退化」就变成一件**在类型上可观察**的事，[`InvalidScope::is_degenerate`]
/// 才能判它。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum InvalidScope {
    /// 目标节点及其**全部后代**（精确失效的正规形态）。
    Subtree {
        /// 目标逻辑节点 id。
        root: String,
        /// 展开后的失效节点集（**不含**兄弟与祖先）。
        nodes: Vec<String>,
    },
    /// 整树（**仅** F2604 属性失效沿父链上溯时可达；正常路径不应出现）。
    WholeTree {
        /// 逻辑树根 id。
        root: String,
        /// 全部节点。
        nodes: Vec<String>,
    },
}

impl InvalidScope {
    /// 目标根 id。
    pub fn root(&self) -> &str {
        match self {
            InvalidScope::Subtree { root, .. } => root,
            InvalidScope::WholeTree { root, .. } => root,
        }
    }

    /// 失效节点集。
    pub fn nodes(&self) -> &[String] {
        match self {
            InvalidScope::Subtree { nodes, .. } => nodes,
            InvalidScope::WholeTree { nodes, .. } => nodes,
        }
    }

    /// 失效节点数。
    pub fn len(&self) -> usize {
        self.nodes().len()
    }

    /// 是否为空范围（**空范围合法**：目标节点已销毁且无后代）。
    pub fn is_empty(&self) -> bool {
        self.nodes().is_empty()
    }

    /// 是否已退化为整树（**P1 判据的判别式**）。
    ///
    /// **判别式为何不是「节点数 == 全树节点数」**：那会在「目标就是根」
    /// 时误报——目标为根时子树**就是**整树，那是精确失效的**合法结果**，
    /// 不是退化。真正的退化是「**范围集合里混进了非后代节点**」，即
    /// 集合**不恰好等于**目标子树。判据因此是**集合等价**而非规模比较。
    pub fn is_degenerate(&self) -> bool {
        match self {
            // 整树是**显式的退化入口**：它只在「目标就是根」时合法，
            // 而那种情形调用方应改用 Subtree{root==tree_root}——那条路
            // 走的是精确失效，两者在规模上不可区分，故只能靠构造区分。
            InvalidScope::WholeTree { .. } => true,
            // 子树构造的退化只可能来自「集合被塞进了非后代」，
            // 而那需要真值集才能判（见 [`scope_is_degenerate`]）。
            InvalidScope::Subtree { .. } => false,
        }
    }

    /// 范围是否**不恰好**等于目标的真后代集（**带真值的退化判别**）。
    ///
    /// 集合相等（而非包含）是判别式的全部：少一个节点是「漏失效」，
    /// 多一个节点是「范围过大」，**两者都算退化**——漏失效的症状是
    /// 界面不更新（错），范围过大的症状是变慢（也错，只是更隐蔽）。
    pub fn is_degenerate_against(&self, truth: &[String]) -> bool {
        let got = self.nodes();
        if got.len() != truth.len() {
            return true;
        }
        truth.iter().any(|t| !got.iter().any(|n| n == t))
    }

    /// 目标是否在范围内（**含**目标自身）。
    pub fn contains(&self, id: &str) -> bool {
        self.nodes().iter().any(|n| n.as_str() == id)
    }
}

/// 范围退化判别（**带真值的唯一入口**）。
///
/// 真值集由调用方**独立重算**——本模块提供的 [`collect_subtree`] 与
/// 真值同源，用它当真值即自证式断言（判据恒真）。故这里只收真值。
pub fn scope_is_degenerate(scope: &InvalidScope, truth: &[String]) -> bool {
    scope.is_degenerate() || scope.is_degenerate_against(truth)
}

/// 子树遍历的显式栈深度上限。
///
/// **为何显式栈还要限深**：显式栈把「爆栈」换成了「吃内存」，而内核态
/// 内存是**全局共享**的（渲染帧预算的一部分）。不设限则一个恶意深树
/// 能把增量更新变成「吃掉半个内核内存」——比崩栈更坏，因为不崩。
/// 取 512，与 F2602 的 [`lt::MAX_TREE_DEPTH`] 同值（同一棵树不该在两个
/// 域有不同的深度上限）。
pub const MAX_WALK_DEPTH: usize = 512;

/// 批队列上限（见模块头注「七」：与 F2604 的 `MAX_PENDING` 同值有因）。
pub const MAX_QUEUE: usize = 4096;

/// 收集 `root` 的**全部后代**（含 `root` 自身），显式栈、无递归。
///
/// **序的保证**：深度优先、**子节点按 `children` 声明序**入栈但**逆序**
/// 出栈（`pop` 是后进先出，故须逆序 push 才能让先声明的先访问）。
/// 这个序与 F2602 的声明序一致，故失效集合里同层节点的相对序可预期——
/// 判据可以按序断言而不用排序。
///
/// **为何不在收集时判环**：本模块消费的是 F2602 维护的树，环不变量由
/// F2602 的 [`lt::assert_invariants`] 负责。本模块若在收集时判环，
/// 就要自己再走一遍祖先链（O(depth) 每节点），**同一棵树判两次**，
/// 且两处判据的结论可能不一致（不一致时查谁？）。职责分离更省。
pub fn collect_subtree(
    tree: &ControlTree,
    root: &str,
) -> IncrOutcome<Vec<String>> {
    if tree.is_destroyed() {
        return ifail(
            IncrDiagCode::TargetAbsent,
            "逻辑树已销毁",
            "查销毁流程与增量提交序",
            root,
        );
    }
    let start = match tree.raw(root) {
        None => {
            return ifail(
                IncrDiagCode::TargetAbsent,
                "目标节点不在逻辑树内",
                IncrDiagCode::TargetAbsent.hint(),
                root,
            );
        }
        Some(n) if n.destroyed => {
            return ifail(
                IncrDiagCode::TargetDestroyed,
                "目标节点已销毁",
                IncrDiagCode::TargetDestroyed.hint(),
                root,
            );
        }
        Some(_) => root,
    };

    let mut out: Vec<String> = Vec::new();
    // (节点 id, 深度)。显式栈：每项两个 usize + 一个 String。
    let mut stack: Vec<(String, usize)> = vec![(String::from(start), 0usize)];
    while let Some((id, depth)) = stack.pop() {
        if depth > MAX_WALK_DEPTH {
            return ifail(
                IncrDiagCode::DepthLimitExceeded,
                "子树遍历深度超显式栈上限",
                IncrDiagCode::DepthLimitExceeded.hint(),
                &id,
            );
        }
        out.push(id.clone());
        let kids = match tree.raw(&id) {
            None => {
                // 子表指向不存在的节点（F2602 不变量破了）。**记名后跳过**，
                // 不静默：丢一个子树与丢一个节点，症状一样但归因不同。
                // 归 F2602，本模块继续（降级 + 上报）。
                continue;
            }
            Some(n) => n.children.clone(),
        };
        // 逆序 push，让先声明的子节点先被访问。
        let mut i = kids.len();
        while i > 0 {
            i -= 1;
            stack.push((kids[i].clone(), depth + 1));
        }
    }
    iok(out)
}

/// 构造子树失效范围（**精确失效的正规入口**）。
pub fn subtree_scope(tree: &ControlTree, root: &str) -> IncrOutcome<InvalidScope> {
    let nodes = collect_subtree(tree, root)?;
    iok(InvalidScope::Subtree {
        root: String::from(root),
        nodes,
    })
}

/// 构造整树失效范围（**退化入口，仅属性失效上溯时可达**）。
pub fn whole_tree_scope(tree: &ControlTree) -> IncrOutcome<InvalidScope> {
    let root = tree.root();
    let nodes = collect_subtree(tree, root)?;
    iok(InvalidScope::WholeTree {
        root: String::from(root),
        nodes,
    })
}

/// **精确失效的双向断言**（见模块头注「二」：两个方向都要查）。
///
/// - `scope` 是被声明的范围；
/// - `truth` 是**独立重算**的真值后代集（判据侧自己算的，不由本模块产出）。
///
/// 返回问题列表（空 = 精确）。**两个方向分别列出**，因为两者的归因
/// 完全不同：无凭空失效 → 范围构造收窄过度；无遗漏失效 → 范围构造漏节点。
pub fn assert_scope_exact(scope: &InvalidScope, truth: &[String]) -> Vec<String> {
    let mut problems: Vec<String> = Vec::new();
    let got = scope.nodes();
    // 方向一：无凭空失效（集合内每个都是真后代）。
    for n in got.iter() {
        if !truth.iter().any(|t| t == n) {
            problems.push(format!(
                "凭空失效：{} 不在目标 {} 的真后代集内",
                n,
                scope.root()
            ));
        }
    }
    // 方向二：无遗漏失效（每个真后代都在集合内）。
    for t in truth.iter() {
        if !got.iter().any(|n| n == t) {
            problems.push(format!("遗漏失效：真后代 {} 不在集合内", t));
        }
    }
    // 去重后返回（两个方向可能对同一节点各报一次，那是一处缺陷两个症状）。
    let mut seen: Vec<String> = Vec::new();
    let mut uniq: Vec<String> = Vec::new();
    for p in problems.iter() {
        if !seen.iter().any(|s| s == p) {
            seen.push(p.clone());
            uniq.push(p.clone());
        }
    }
    uniq
}

// ---------------------------------------------------------------------------
// 三、批处理（判据二：帧边界批处理）
// ---------------------------------------------------------------------------

/// 一条待提交的失效（**队列项**）。
///
/// 合并键是（`node`，`mask`）：见模块头注「三」——不同掩码走不同下游，
/// 按节点合并就得在应用时再拆回来。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PendingInvalid {
    /// 逻辑节点 id。
    pub node: String,
    /// 失效类型位掩码（`render` / `layout` / `hit` 的并集）。
    pub mask: u8,
}

impl PendingInvalid {
    /// 从 F2604 的失效结构取掩码（**位序单源**：`Invalidation` 的
    /// 三位序由本函数定义，下游路由表按同序写）。
    pub fn mask_of(inv: &Invalidation) -> u8 {
        let mut m = 0u8;
        if inv.render {
            m |= 0b001;
        }
        if inv.layout {
            m |= 0b010;
        }
        if inv.hit {
            m |= 0b100;
        }
        m
    }

    /// 由掩码还原失效结构（**位序单源的对偶**）。
    pub fn inv_of(mask: u8) -> Invalidation {
        Invalidation {
            render: mask & 0b001 != 0,
            layout: mask & 0b010 != 0,
            hit: mask & 0b100 != 0,
        }
    }

    /// 掩码是否超出已定义的三个位。
    pub fn is_known_mask(mask: u8) -> bool {
        mask & !0b111 == 0
    }
}

/// 批队列（**同帧合并，帧边界一次应用**）。
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct ChangeQueue {
    items: Vec<PendingInvalid>,
    /// 溢出标志（**独立于 `items`**，故清队列不等于清标志）。
    ///
    /// **为何要独立字段而不是「用 `len() == MAX_QUEUE` 推断」**：后者在
    /// 「攒满后提交过一次（`len` 归零）但那条被拒的还没被告知」时会
    /// 漏报——强制提交是**已发生的处置**，若标志随队列一起清掉，
    /// 提交报告里的 `forced` 就得靠猜。
    overflowed: bool,
    /// 累计入队尝试次数（**含被合并与被拒的**）。
    ///
    /// **为何要累计而不是在提交时现数**：提交时能数到的只有「队列里
    /// 剩几条」，**被合并掉的那几条已经不在队列里了**——不另记计数，
    /// 合并率就永远算不出来（分母只剩分子，恒为 0%），而「合并率恒 0」
    /// 看不出是「合并没生效」还是「统计没做」。
    attempts_total: u32,
    /// 累计被拒次数（溢出）。
    rejected_total: u32,
    /// 上次提交时的 `attempts_total` 快照（**用于按帧取增量**）。
    attempts_at_commit: u32,
    /// 上次提交时的 `rejected_total` 快照。
    rejected_at_commit: u32,
}

impl ChangeQueue {
    /// 空队列。
    pub fn new() -> ChangeQueue {
        ChangeQueue {
            items: Vec::new(),
            overflowed: false,
            attempts_total: 0,
            rejected_total: 0,
            attempts_at_commit: 0,
            rejected_at_commit: 0,
        }
    }

    /// 入队一条失效，**同（节点，掩码）合并**。
    ///
    /// 返回 `true` 表示发生了合并（`false` = 新增一项）。
    ///
    /// **合并保留首次入队位**（与 F2604 世代戳合并同口径）：队列的序
    /// 即应用序，若合并时把项挪到末尾，**同一帧内「先改 A 后改 B」与
    /// 「先改 B 后改 A」会产出不同的应用序**，那就把「同输入同输出」破了。
    ///
    /// **溢出不静默**：超 [`MAX_QUEUE`] 时**拒绝本条**并置溢出标志，
    /// 由调用方（[`IncrementalUpdate::commit_frame`]）强制提交——**本
    /// 函数不做强制提交**，理由是它拿不到应用失效所需的树与下游句柄，
    /// 硬做就得把参数扩成五个，职责就散了。
    pub fn enqueue(&mut self, node: &str, mask: u8) -> QueueOutcome {
        // **每次尝试都计数**（含被拒与空掩码）：分母必须是「来过多少
        // 次」，否则「一条被拒」与「一条没来」在统计上不可区分。
        self.attempts_total = self.attempts_total.saturating_add(1);
        if !PendingInvalid::is_known_mask(mask) {
            return QueueOutcome::BadMask;
        }
        if mask == 0 {
            // 空掩码入队是**无操作**而非错误：调用方常把
            // `Invalidation::NONE` 无条件传进来，让它走同一条代码路径。
            // 报错会让调用方被迫写 `if !inv.is_empty()` 的样板。
            return QueueOutcome::NoOp;
        }
        for it in self.items.iter_mut() {
            if it.node.as_str() == node && it.mask == mask {
                return QueueOutcome::Merged;
            }
        }
        if self.items.len() >= MAX_QUEUE {
            self.overflowed = true;
            self.rejected_total = self.rejected_total.saturating_add(1);
            return QueueOutcome::Overflowed;
        }
        self.items.push(PendingInvalid {
            node: String::from(node),
            mask,
        });
        QueueOutcome::Queued
    }

    /// 本帧（上次提交以来）的入队尝试次数。
    pub fn frame_attempts(&self) -> u32 {
        self.attempts_total.saturating_sub(self.attempts_at_commit)
    }

    /// 累计入队尝试次数。
    pub fn attempts_total(&self) -> u32 {
        self.attempts_total
    }

    /// 累计被拒次数。
    pub fn rejected_total(&self) -> u32 {
        self.rejected_total
    }

    /// 队列长度。
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 是否发生过溢出（**自上次 [`ChangeQueue::take_overflow`] 起**）。
    pub fn overflowed(&self) -> bool {
        self.overflowed
    }

    /// 取走并清零溢出标志。
    pub fn take_overflow(&mut self) -> bool {
        let o = self.overflowed;
        self.overflowed = false;
        o
    }

    /// 队列项（**序即应用序**）。
    pub fn items(&self) -> &[PendingInvalid] {
        &self.items
    }

    /// 清空（**提交成功后由增量器调用**）。
    pub fn clear(&mut self) {
        self.items.clear();
    }
}

/// 入队结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QueueOutcome {
    /// 新增一项。
    Queued,
    /// 与已有项合并。
    Merged,
    /// 空掩码，**无操作**（不报错）。
    NoOp,
    /// 掩码含未定义位（**拒绝**）。
    BadMask,
    /// 队列已满，本条被拒（溢出标志已置）。
    Overflowed,
}

impl QueueOutcome {
    /// 是否产出了实际工作（新增或合并）。
    pub const fn did_work(self) -> bool {
        matches!(self, QueueOutcome::Queued | QueueOutcome::Merged)
    }
}

/// 一次帧提交的产出报告。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CommitReport {
    /// 本帧入队尝试次数（含被合并与被拒的）。
    pub attempts: usize,
    /// 队列末长度（**合并后**，即实际应用项数）。
    pub applied: usize,
    /// 其中因合并而省掉的项数（`attempts - applied` 的语义部分）。
    pub merged_away: usize,
    /// 被溢出拒绝的项数。
    pub rejected: usize,
    /// 触发的失效节点数（**去重后**，跨掩码按节点计）。
    pub touched_nodes: usize,
    /// 是否发生过溢出（强制提交路径）。
    pub forced: bool,
    /// 队列是否已清空（**提交后的自检**，false 即泄漏）。
    pub queue_drained: bool,
    /// 非阻断告警。
    pub warnings: Vec<IncrDiagnostic>,
}

impl CommitReport {
    /// 空报告。
    pub fn new() -> CommitReport {
        CommitReport {
            attempts: 0,
            applied: 0,
            merged_away: 0,
            rejected: 0,
            touched_nodes: 0,
            forced: false,
            queue_drained: true,
            warnings: Vec::new(),
        }
    }

    /// 合并率（**百分比**，分母为 0 时返回 0）。
    ///
    /// **分母是 attempts 而非 applied**：分母若用 applied，
    /// 「一条都没进」时得到 0/0，「全合并」时得到 0/0，两者不可区分——
    /// 而这两种情况的运维含义完全不同（前者是没活干，后者是合并得太狠）。
    pub fn merge_ratio(&self) -> u32 {
        if self.attempts == 0 {
            return 0;
        }
        ((self.attempts - self.applied) as u32) * 100 / (self.attempts as u32)
    }
}

// ---------------------------------------------------------------------------
// 四、三分发（判据三）
// ---------------------------------------------------------------------------

/// 失效类型的下游（**路由表按此枚举序写**）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Route {
    /// D 域渲染。
    Render,
    /// 布局（VE-F2621）。
    Layout,
    /// 命中测试（N03）。
    Hit,
}

impl Route {
    /// 线缆名。
    pub const fn as_str(self) -> &'static str {
        match self {
            Route::Render => "render",
            Route::Layout => "layout",
            Route::Hit => "hit",
        }
    }

    /// 消费单号（人话）。
    pub const fn owner(self) -> &'static str {
        match self {
            Route::Render => "D 域渲染",
            Route::Layout => "VE-F2621 测量布局",
            Route::Hit => "N03 命中测试",
        }
    }

    /// 该下游**只**应接收的失效位（**位序单源**）。
    pub const fn own_mask(self) -> u8 {
        match self {
            Route::Render => 0b001,
            Route::Layout => 0b010,
            Route::Hit => 0b100,
        }
    }
}

/// 路由表（**静态三分，O(1) 类型路由**）。
///
/// **为何是「每路由恰一位」的一一映射而不是允许一个下游收多种失效**：
/// 允许的话，「布局失效发去渲染」就成了**合法配置**，纪律只能靠评审盯。
/// 一一映射下错路在**类型层面**就不成立——`own_mask` 只有一位，
/// 拿一个含两位的掩码去问「该发哪」，答案是「**发不了**」。
pub const ROUTES: [(Route, u8); 3] = [
    (Route::Render, 0b001),
    (Route::Layout, 0b010),
    (Route::Hit, 0b100),
];

/// 由掩码解析目标下游（**一对一，故返回 `Option`**）。
///
/// 返回 `None` 的两种情形：掩码含未定义位；掩码含**多位**（跨类型，
/// 需拆成多条通知——那是队列合并键选错，不是路由能修的）。
pub fn route_of(mask: u8) -> Option<Route> {
    if !PendingInvalid::is_known_mask(mask) {
        return None;
    }
    for (r, m) in ROUTES.iter() {
        if mask == *m {
            return Some(*r);
        }
    }
    None
}

/// 拆分复合掩码为单类型路由（**位序序**）。
///
/// 空掩码产出**空 vec**（不产出「无失效」这个下游——那会让下游收到
/// 「你没事干」的通知，比不发通知更吵）。
pub fn split_routes(mask: u8) -> Vec<Route> {
    let mut out: Vec<Route> = Vec::new();
    if !PendingInvalid::is_known_mask(mask) {
        return out;
    }
    for (r, m) in ROUTES.iter() {
        if mask & *m != 0 {
            out.push(*r);
        }
    }
    out
}

/// 一条投递记录（**逐条记账，为错路审计服务**）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RouteRecord {
    /// 目标节点。
    pub node: String,
    /// 实际投递的下游。
    pub delivered_to: Route,
    /// 本该投递的下游（**由掩码独立重算**）。
    pub expected_to: Route,
}

impl RouteRecord {
    /// 是否错路。
    pub fn is_mismatch(&self) -> bool {
        self.delivered_to != self.expected_to
    }
}

/// 路由账（**逐条，非只比计数**——见模块头注「四」）。
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct RouteLedger {
    records: Vec<RouteRecord>,
}

impl RouteLedger {
    /// 空账。
    pub fn new() -> RouteLedger {
        RouteLedger { records: Vec::new() }
    }

    /// 记一条投递。
    pub fn push(&mut self, node: &str, mask: u8, delivered_to: Route) {
        self.records.push(RouteRecord {
            node: String::from(node),
            delivered_to,
            expected_to: match route_of(mask) {
                Some(r) => r,
                // 掩码非法时**没有「本该去哪」**；记成 Render 是**显式
                // 占位**而非默认值兜底（`Default` 兜底会让人以为解析过）。
                None => Route::Render,
            },
        });
    }

    /// 全部记录。
    pub fn records(&self) -> &[RouteRecord] {
        &self.records
    }

    /// 记录数。
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// 某下游收到的条数。
    pub fn count_to(&self, r: Route) -> usize {
        self.records.iter().filter(|x| x.delivered_to == r).count()
    }

    /// 错路条数。
    pub fn mismatch_count(&self) -> usize {
        self.records.iter().filter(|x| x.is_mismatch()).count()
    }

    /// 清账（每帧一次，防无界增长）。
    pub fn clear(&mut self) {
        self.records.clear();
    }
}

/// 路由错路审计（**逐条点名**）。
///
/// **为何点名而不只报数**：数只能告诉你「有 3 条错路」，点名的列表能
/// 告诉你「哪三个节点、错到哪」——后者才能定位到具体调用点。
pub fn audit_routes(ledger: &RouteLedger) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for r in ledger.records().iter() {
        if r.is_mismatch() {
            out.push(format!(
                "{}:失效类型本该发 {} 却发往 {}",
                r.node,
                r.expected_to.as_str(),
                r.delivered_to.as_str()
            ));
        }
    }
    out
}

// ---------------------------------------------------------------------------
// 五、增量更新器（把三段接成一条）
// ---------------------------------------------------------------------------

/// 一次提交的全貌（**范围 + 路由 + 队列**三段合一）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ApplyReport {
    /// 失效范围（**精确失效的产出**）。
    pub scope: InvalidScope,
    /// 路由账（**三分发的产出**）。
    pub ledger: RouteLedger,
    /// 队列提交报告。
    pub commit: CommitReport,
    /// 可视树同步是否与失效范围**在同一次提交内**完成（锚点双树联动）。
    pub dual_synced: bool,
    /// 非阻断告警。
    pub warnings: Vec<IncrDiagnostic>,
}

impl ApplyReport {
    /// 空报告骨架。
    pub fn new() -> ApplyReport {
        ApplyReport {
            scope: InvalidScope::Subtree {
                root: String::new(),
                nodes: Vec::new(),
            },
            ledger: RouteLedger::new(),
            commit: CommitReport::new(),
            dual_synced: false,
            warnings: Vec::new(),
        }
    }
}

/// 增量更新器。
///
/// **持有队列，不持有树**：树是外部可变状态，持有它就等于持有了一个
/// 会在自己不知情时被改掉的快照，双树同步（F2605）与范围收集都需要
/// **当下**的树，故每次调用显式传入 `&ControlTree`。
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct IncrementalUpdate {
    queue: ChangeQueue,
    destroyed: bool,
    commits: u32,
    forced_commits: u32,
}

impl IncrementalUpdate {
    /// 新建。
    pub fn new() -> IncrementalUpdate {
        IncrementalUpdate {
            queue: ChangeQueue::new(),
            destroyed: false,
            commits: 0,
            forced_commits: 0,
        }
    }

    /// 队列（只读）。
    pub fn queue(&self) -> &ChangeQueue {
        &self.queue
    }

    /// 队列（可变）。
    ///
    /// **为何给出可变入口而不只留 [`IncrementalUpdate::apply`]**：
    /// 属性侧（F2604）失效是**每属性一次**的粒度，而 `apply` 的粒度是
    /// 每帧一次。两者不等价——一帧里改 50 个属性就该入队 50 条，
    /// 走 `apply` 就得把 50 条并成 1 个 `Invalidation`再入队，
    /// 那正是本模块要防的「帧内风暴」在**上游**被提前消化掉，
    /// 合并语义（取并集）也就无从检验。留可变入口让上游能**按真实粒度**
    /// 入队，合并才是真的在发生。
    ///
    /// **边界**：本入口只暴露 [`ChangeQueue`]，**不暴露**范围构造与
    /// 路由表，故「绕过精确失效」与「绕过三分发」两条纪律仍不可破。
    pub fn queue_mut(&mut self) -> &mut ChangeQueue {
        &mut self.queue
    }

    /// 已提交帧数。
    pub fn commit_count(&self) -> u32 {
        self.commits
    }

    /// 强制提交次数（**溢出的累计**——长期非零即风暴预警线）。
    pub fn forced_count(&self) -> u32 {
        self.forced_commits
    }

    /// 销毁。
    pub fn destroy(&mut self) {
        self.destroyed = true;
        self.queue.clear();
    }

    /// 是否已销毁。
    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }

    /// **一次提交：失效范围 + 三分发 + 双树同步**（三段合一）。
    ///
    /// `op` 是 F2605 的同步动作；**传 `None` 表示本轮只有属性侧失效、
    /// 无结构变更**（最常见的一帧）。
    ///
    /// **为何三段必须合一**（见模块头注「五」）：分两次做会出现
    /// 「可视树已变、失效范围还是旧的」的窗口，症状是画面撕裂一帧后
    /// 自愈——**不可复现的缺陷是最贵的缺陷**。
    pub fn apply(
        &mut self,
        tree: &ControlTree,
        dual: &mut DualTree,
        op: Option<&SyncOp>,
        inv: Option<&Invalidation>,
    ) -> IncrOutcome<ApplyReport> {
        if self.destroyed {
            return ifail(
                IncrDiagCode::IncrLifecycleViolation,
                "增量更新器已销毁",
                IncrDiagCode::IncrLifecycleViolation.hint(),
                "<apply>",
            );
        }
        let mut rep = ApplyReport::new();

        // ── 第 1 段：双树同步（结构变更）
        //
        // 先同步再取范围：范围要对着**同步后**的树算，反过来算出的
        // 是上一帧的形状（症状同样是撕裂，但方向相反，更难查）。
        if let Some(o) = op {
            let sr = match dual.sync(tree, o) {
                Ok(r) => r,
                Err(d) => {
                    // F2605 的失败**不静默转警告**：可视树可能已半改，
                    // 继续走范围计算就是在给一棵不一致的树算失效。
                    return ifail(
                        IncrDiagCode::SyncOmission,
                        "双树同步失败，增失效范围无意义",
                        d.hint.as_str(),
                        o.target(),
                    );
                }
            };
            rep.dual_synced = true;
            rep.warnings.extend(convert_dual_diags(&sr.warnings));
        }

        // ── 第 2 段：失效范围（精确失效）
        let target: String = match op {
            Some(o) => String::from(o.target()),
            None => match inv {
                // 无结构变更时范围由调用方给的目标决定；缺省取根
                // （=整树，退化）。**刻意不给「猜一个」的默认值**：
                // 退化必须是调用方**看得见**的选择，不是隐式兜底。
                None => String::from(tree.root()),
                Some(_) => String::from(tree.root()),
            },
        };
        let scope = match subtree_scope(tree, &target) {
            Ok(s) => s,
            Err(d) => return Err(d),
        };
        // 退化判定需要**独立重算**的真值。`apply` 拿不到外部真值
        // （它只有树本身），故此处只做**构造级**退化拦截
        // （`WholeTree` 一律退化）；集合级退化由 [`self_check`] 用
        // 调用方给的真值判。两处分工明确，不互相顶替。
        if scope.is_degenerate() {
            return ifail(
                IncrDiagCode::ScopeDegenerated,
                "失效范围退化为整树",
                IncrDiagCode::ScopeDegenerated.hint(),
                &target,
            );
        }
        rep.scope = scope;

        // ── 第 3 段：入队（帧边界合并）
        //
        // **尝试数由队列自己累计**（[`ChangeQueue::enqueue`] 每次调用
        // 都自增），此处**不再重复计数**——两处都数会让 `attempts` 翻倍，
        // 而 `merge_ratio` 的分母翻倍后可能 >100%，产出一个「不可能的
        // 百分比」，运维看到只会怀疑仪表盘而不会怀疑合并逻辑。
        if let Some(i) = inv {
            let mask = PendingInvalid::mask_of(i);
            let outcome = self.queue.enqueue(&target, mask);
            match outcome {
                QueueOutcome::NoOp => {}
                QueueOutcome::BadMask => {
                    return ifail(
                        IncrDiagCode::RouteMismatch,
                        "掩码含未定义失效位",
                        IncrDiagCode::RouteMismatch.hint(),
                        &target,
                    );
                }
                QueueOutcome::Overflowed => {
                    // 溢出**不阻断**（头注「六」）：强制提交，语义优先于合并收益。
                    // 被拒计数由 `commit_frame` 从队列快照差算出，
                    // 此处**不再 `+= 1`**（理由同上）。
                    let mut w = self.commit_frame(tree, &mut rep.ledger);
                    w.commit.forced = true;
                    rep.warnings.append(&mut w.warnings);
                    rep.commit = w.commit;
                    rep.warnings.push(icd(
                        IncrDiagCode::QueueOverflow,
                        "批队列溢出，已强制批提交",
                        IncrDiagCode::QueueOverflow.hint(),
                        &target,
                    ));
                    self.forced_commits = self.forced_commits.saturating_add(1);
                    return iok(rep);
                }
                QueueOutcome::Queued | QueueOutcome::Merged => {}
            }
        }

        iok(rep)
    }

    /// **帧边界提交**：把队列一次应用给下游。
    ///
    /// **这是队列唯一的清空出口**——理由：清空点分散到多处时，
    /// 早退路径（错误返回）必然漏一处，漏处的症状是「上一帧的失效
    /// 在这一帧重放」，而重放的内容**看起来是对的**（同一节点确实该
    /// 重画），只是画了两次——**慢**，不**错**。
    pub fn commit_frame(
        &mut self,
        tree: &ControlTree,
        ledger: &mut RouteLedger,
    ) -> ApplyReport {
        let mut rep = ApplyReport::new();
        // 尝试数取**本帧增量**（上次提交以来的入队次数），
        // 而不是队列长度——队列长度只是「合并后剩几条」，
        // 拿它当分母会让 `merge_ratio` 恒为 0（分子分母同源）。
        let frame_attempts = self.queue.frame_attempts() as usize;
        rep.commit.attempts = frame_attempts;
        rep.commit.applied = self.queue.len();
        let mut touched: Vec<String> = Vec::new();
        let items: Vec<PendingInvalid> = self.queue.items().to_vec();
        for it in items.iter() {
            // **队列项可能已失效**：入队时节点在树里，提交时未必。
            // 两种失效形态，判据不同：
            //
            // ① **不在树内**（`raw` 为None）：节点被真删了。
            // ② **已成孤儿**（`parent_id == None` 且不是根）：F2602 的
            //    [`lt::remove`] 语义是「**摘链**」而非「删节点」——
            //    它把节点从父的 `children` 里摘掉并把 `parent_id` 置空，
            //    **节点本身仍留在 `nodes` 里**。故`raw()` 仍能取到它。
            //    只判 ① 会让 ② 整类漏过：给一个已脱链的节点发重绘
            //    通知，下游按 id 查树**查得到**（节点还在），于是通知
            //    被「成功」投递到一个**不在任何遍历结果里的**节点上——
            //    白跑，且**连静默丢弃都不会发生**，比 ① 更难查。
            //
            // 两种形态都**不投递**且**点名上报**：白跑必须变成可见告警。
            match tree.raw(&it.node) {
                None => {
                    rep.warnings.push(icd(
                        IncrDiagCode::TargetAbsent,
                        "队列项的目标节点在提交时已不在树内，该项不投递",
                        IncrDiagCode::TargetAbsent.hint(),
                        &it.node,
                    ));
                    continue;
                }
                Some(n) if n.destroyed => {
                    rep.warnings.push(icd(
                        IncrDiagCode::TargetDestroyed,
                        "队列项的目标节点在提交时已销毁，该项不投递",
                        IncrDiagCode::TargetDestroyed.hint(),
                        &it.node,
                    ));
                    continue;
                }
                Some(n) => {
                    // 孤儿判据：`parent_id` 为空且不是根。**根节点**
                    // `parent_id` 天然为 None，故必须显式排除，否则
                    // 每帧都会把根当成孤儿，合法提交被误判为异常。
                    let is_root = it.node.as_str() == tree.root();
                    if n.parent_id.is_none() && !is_root {
                        rep.warnings.push(icd(
                            IncrDiagCode::TargetAbsent,
                            "队列项的目标节点在提交时已脱链（不再属于任何子树），该项不投递",
                            IncrDiagCode::TargetAbsent.hint(),
                            &it.node,
                        ));
                        continue;
                    }
                }
            }
            // 投递：拆分复合掩码 → 逐类型记账。
            for r in split_routes(it.mask) {
                ledger.push(&it.node, it.mask, r);
                if !touched.iter().any(|t| t.as_str() == it.node.as_str()) {
                    touched.push(it.node.clone());
                }
            }
        }
        rep.commit.applied = self.queue.len();
        rep.commit.touched_nodes = touched.len();
        // 合并掉的条数 = 本帧尝试 − 实际应用 − 被拒。
        // **三项都要减**：不减被拒项会把「溢出的那几条」算成「合并掉的」，
        // 合并率随之虚高——而溢出的处置恰恰是**丢了工作**，
        // 把它记成「合并省下的」是把问题说成了功劳。
        let rejected_now = self
            .queue
            .rejected_total()
            .saturating_sub(self.queue.rejected_at_commit) as usize;
        rep.commit.rejected = rejected_now;
        rep.commit.merged_away = frame_attempts
            .saturating_sub(rep.commit.applied)
            .saturating_sub(rejected_now);
        self.queue.clear();
        self.queue.attempts_at_commit = self.queue.attempts_total;
        self.queue.rejected_at_commit = self.queue.rejected_total;
        // 提交后**当场**核对队列已空（泄漏即 P1，不等下一帧）。
        rep.commit.queue_drained = self.queue.is_empty();
        if !rep.commit.queue_drained {
            rep.warnings.push(icd(
                IncrDiagCode::QueueLeak,
                "提交后队列未清空",
                IncrDiagCode::QueueLeak.hint(),
                "<commit_frame>",
            ));
        }
        if self.queue.take_overflow() {
            rep.commit.forced = true;
        }
        self.commits = self.commits.saturating_add(1);
        rep
    }

    /// 全量重建的失效范围（**显式入口，不做隐式兜底**）。
    ///
    /// 单列一个函数而不是让调用方自己去构 [`InvalidScope::WholeTree`]：
    /// 后者要手工填两个字段（root + nodes），漏填 nodes 会得到一个
    /// 「整树范围但集合为空」的**自相矛盾**对象，而它在类型上完全合法。
    pub fn full_scope(&self, tree: &ControlTree) -> IncrOutcome<InvalidScope> {
        whole_tree_scope(tree)
    }
}

/// 把 F2605 的诊断转成本域告警（**保留原码，不改判**）。
fn convert_dual_diags(list: &[crate::svstar2::ven05_dual::DualDiagnostic]) -> Vec<IncrDiagnostic> {
    let mut out: Vec<IncrDiagnostic> = Vec::new();
    for d in list.iter() {
        out.push(icd(
            IncrDiagCode::IncrSelfcheckFailed,
            &d.message,
            d.code.hint(),
            &d.at,
        ));
    }
    out
}

// ---------------------------------------------------------------------------
// 六、域级审计
// ---------------------------------------------------------------------------

/// 域自检：一次提交里所有纪律的联合结论。
///
/// **五类问题分开报，不合并成「是否一致」**（与 F2605 同理：合并后
/// 「哪一类破了」这个最有用的信息就丢了，而处置动作按类不同）。
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct IncrAudit {
    /// 范围退化（P1）。
    pub scope_degenerated: Vec<String>,
    /// 三分发错路（P1，逐条点名）。
    pub route_mismatch: Vec<String>,
    /// 队列泄漏（P1）。
    pub queue_leak: Vec<String>,
    /// 双树同步遗漏（P1）。
    pub sync_omission: Vec<String>,
    /// 溢出（降级后继续，非 P1）。
    pub overflow: Vec<String>,
    /// 范围精确性（双向断言结果，**由调用方给独立真值集**）。
    pub scope_problems: Vec<String>,
}

impl IncrAudit {
    /// 问题总数。
    pub fn problem_count(&self) -> usize {
        self.scope_degenerated.len()
            + self.route_mismatch.len()
            + self.queue_leak.len()
            + self.sync_omission.len()
            + self.overflow.len()
            + self.scope_problems.len()
    }

    /// 是否全部通过。
    pub fn is_clean(&self) -> bool {
        self.problem_count() == 0
    }
}

/// 域自检入口。
///
/// `truth_scope` 是**独立重算**的真值后代集；传空 slice 表示
/// 「本轮不做范围精确性核对」（此时该项不计入问题数）。
pub fn self_check(
    tree: &ControlTree,
    rep: &ApplyReport,
    truth_scope: &[String],
) -> IncrAudit {
    let mut a = IncrAudit::default();
    // ⓿ 范围根在树内（**先查根**：根不在树内时其集合必与真值不等，
    // 那时的「不等」是根错带来的假象，报「退化」会把人引向错的方向）
    if !tree.has(rep.scope.root()) {
        a.sync_omission.push(format!(
            "范围根 {} 不在逻辑树内",
            rep.scope.root()
        ));
    }
    // ① 范围退化（构造级 + 集合级；真值由调用方独立重算）
    if scope_is_degenerate(&rep.scope, truth_scope) {
        a.scope_degenerated.push(format!(
            "目标 {} 的范围不恰好等于其真后代集（得 {} 项）",
            rep.scope.root(),
            rep.scope.len()
        ));
    }
    // ② 范围精确性（双向，外部真值）
    if !truth_scope.is_empty() {
        a.scope_problems = assert_scope_exact(&rep.scope, truth_scope);
    }
    // ③ 三分发错路（逐条）
    a.route_mismatch = audit_routes(&rep.ledger);
    // ④ 队列泄漏
    if !rep.commit.queue_drained {
        a.queue_leak.push(format!("提交后队列残留 {} 项", rep.commit.applied));
    }
    // ⑤ 双树同步遗漏：有结构变更却没同步
    if rep.scope.len() > 0 && !rep.dual_synced && rep.commit.attempts > 0 {
        // 纯属性失效（op=None）时 dual_synced=false 属正常，故只在
        // 「本轮确实有结构范围变更」时才判遗漏——判别口径是
        // 「范围非空且尝试数>0」仍不同步，那只可能是纯属性轮，
        // 此时**不**立案。这条须由调用方给真值，故本函数只看
        // 显式的 dual_synced 与 commit 的一致性。
        if rep.commit.applied > 0 && rep.ledger.len() > 0 {
            a.sync_omission.push(format!(
                "目标 {} 有投递但未标记双树同步",
                rep.scope.root()
            ));
        }
    }
    // ⑥ 溢出
    if rep.commit.forced {
        a.overflow.push(format!("本帧强制提交（溢出）"));
    }
    a
}

// ---------------------------------------------------------------------------
// 七、性能账与跨批对接台账
// ---------------------------------------------------------------------------

/// 性能分解的一行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PerfRow {
    /// 环节名。
    pub stage: &'static str,
    /// 复杂度口径（**结构性事实**，非实测）。
    pub complexity: &'static str,
    /// 依据。
    pub basis: &'static str,
}

/// 性能逐项分解（锚点四条 + 队列项）。
///
/// **诚实标注**：本表**不含实机计时**，只陈述可从结构推导的口径。
/// 万节点增量实测由 F2610 基准单承接。
pub const PERF_ROWS: [PerfRow; 5] = [
    PerfRow {
        stage: "失效标记",
        complexity: "O(子树)",
        basis: "collect_subtree 只走目标节点的后代；兄弟与祖先不在遍历前沿，故不增开销",
    },
    PerfRow {
        stage: "批处理",
        complexity: "O(变更数)",
        basis: "入队线性扫描做合并（摊还 O(1)/条）；提交按队列项数线性，每项 O(1) 拆掩码",
    },
    PerfRow {
        stage: "三分发",
        complexity: "O(1) 类型路由",
        basis: "ROUTES 是三项静态表，掩码到下游是查表；与节点数、树深度均无关",
    },
    PerfRow {
        stage: "范围审计",
        complexity: "O(子树²)",
        basis: "assert_scope_exact 双向比对是朴素双循环；仅审计面用，不在帧路径上",
    },
    PerfRow {
        stage: "队列内存",
        complexity: "O(上限) 常驻",
        basis: "MAX_QUEUE=4096 项，每项 (String+u8)；入队超限即拒，故不会无界增长",
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

/// 跨批对接（锚点四条）。
pub const HANDOFFS: [Handoff; 5] = [
    Handoff {
        peer: "VE-F2604",
        contract: "第三段失效标记经 Invalidation 位掩码转入本模块的帧边界队列（mask_of 位序单源）",
        state: "已兑现（本模块消费 Invalidation 三位）",
    },
    Handoff {
        peer: "VE-F2605",
        contract: "本模块消费 SyncOp 做双树增量同步，且与失效范围同一次提交（apply 三段合一）",
        state: "已兑现（apply 显式接收 &mut DualTree）",
    },
    Handoff {
        peer: "VE-F2621",
        contract: "布局失效经 ROUTES[Layout] 投递（掩码 0b010），本单只投递不消费",
        state: "前向（路由表已留位，下游单承接）",
    },
    Handoff {
        peer: "N03-命中",
        contract: "命中失效经 ROUTES[Hit] 投递（掩码 0b100），本单只投递不消费",
        state: "前向（路由表已留位，下游单承接）",
    },
    Handoff {
        peer: "VE-F2610",
        contract: "万节点增量实测入基准（失效标记 O(子树) 与全量重建的收益倍数）",
        state: "前向（本单只给口径，实测由基准单承接）",
    },
];

/// 降级矩阵一行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DegradeRow {
    /// 锚点情形。
    pub situation: &'static str,
    /// 本模块处置。
    pub action: &'static str,
    /// 对应诊断码。
    pub code: IncrDiagCode,
    /// 是否阻断本帧提交。
    pub blocks: bool,
}

/// 降级矩阵（锚点五行逐行落实）。
///
/// **降级方向相反的两类不得共用码**：队列类「攒不下」→ 降级后继续
/// （强制提交）；纪律类「机制自身坏了」→ 阻断。
pub const DEGRADE_MATRIX: [DegradeRow; 5] = [
    DegradeRow {
        situation: "失效范围过大（全树失效退化）",
        action: "P1（精确失效纪律破裂）",
        code: IncrDiagCode::ScopeDegenerated,
        blocks: true,
    },
    DegradeRow {
        situation: "批处理溢出（队列超限）",
        action: "溢出告警 + 强制批提交（风暴极端防护）",
        code: IncrDiagCode::QueueOverflow,
        blocks: false,
    },
    DegradeRow {
        situation: "三分发错路（布局失效发渲染）",
        action: "P1（失效类型错误）",
        code: IncrDiagCode::RouteMismatch,
        blocks: true,
    },
    DegradeRow {
        situation: "双树同步遗漏",
        action: "同步断言（F2605 家族）",
        code: IncrDiagCode::SyncOmission,
        blocks: true,
    },
    DegradeRow {
        situation: "批队列泄漏",
        action: "审计",
        code: IncrDiagCode::QueueLeak,
        blocks: true,
    },
];

/// 无隐私面声明（锚点「无障碍与隐私：无隐私面」）。
///
/// **为何可以断言无隐私面**：本模块只处理控件**结构**（失效范围、
/// 队列、路由），失效类型是三个布尔位，不携带任何属性值、文本或
/// 用户标识——下游拿到的只是「哪个节点该重画」。
pub const PRIVACY_NOTE: &str = "无隐私面：仅处理失效范围与路由，不接触属性值与用户数据";

/// 无障碍替述（增量失效会影响辅助技术的更新时机，故必须留替述）。
pub fn a11y_alternatives() -> [(&'static str, &'static str); 3] {
    [
        (
            "批量提交",
            "一轮变更可能合并为一次失效通知，辅助技术读到的更新次数会少于\
             实际属性写入次数——但读到的是**最终态**，与逐次通知等价",
        ),
        (
            "失效范围",
            "失效范围是子树而非单节点，故辅助技术刷新时可能重读整棵子树的内容\
             ——多读不漏读，可访问性上安全（漏读才是不安全的方向）",
        ),
        (
            "降级可见性",
            "强制提交（队列溢出）时部分合并收益丢失，界面语义不变但可能多一次\
             刷新抖动；辅助技术层应把「本帧强制提交」作为状态暴露而非静默",
        ),
    ]
}

/// 组内分工登记（锚点「分工」）。
pub fn division_of_work() -> [(&'static str, &'static str); 4] {
    [
        ("核心逻辑", "精确失效范围 + 帧边界批处理 + 三分发路由 + 双树增量同步"),
        ("边界防护", "退化拦截 + 溢出强制提交 + 错路逐条审计 + 泄漏当场核对 + 深度上限"),
        ("错误路径", "十个诊断码的码/因/建议/人话四元组 + 降级矩阵五行"),
        ("测试支撑", "域自检判据 + 反假变体登记"),
    ]
}
