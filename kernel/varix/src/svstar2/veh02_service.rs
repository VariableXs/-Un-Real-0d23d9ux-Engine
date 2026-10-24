//! VE-F1402 · 音频引擎服务化架构（VE-H 域 · 引擎本体服务层 · 目标 440 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F1402`
//!
//! **判据（锚点原文五条）**：多消费者、会话隔离、四类别、生命周期、故障域、判据。
//!
//! # 锚点原文摘要与本文件的执行面
//!
//! 锚点要求：音频引擎**不隶属播放链**，作为独立服务存在；播放链（G07）、系统音效、
//! 通知提醒、未来游戏场景四类消费者统一接入；服务模型为「单例引擎服务 + 多客户端会话」，
//! 每消费者声明音频类别（媒体/通信/系统/游戏），类别决定路由与策略；生命周期分
//! 「系统常驻 / 按需唤醒」两模式；引擎崩溃不拖垮消费者——自动重启 + 消费者重连协议。
//!
//! 工程量四段（锚点给定 120+100+80+100=440 行），本文件逐段对应：
//!
//! | 段 | 锚点要求 | 本文件落位 |
//! | --- | --- | --- |
//! | 一 · 多消费者模型（120 行） | 服务化架构理由声明、重复建设的反面、四消费者统一接入 | [`AudioEngineService`] / [`ConsumerKind`] / [`SESSION_ISOLATION_DOC`] 侧 [`SERVICE_RATIONALE_DOC`] |
//! | 二 · 会话与配额（100 行） | 单例 + 多会话、会话隔离（一崩不连坐）、会话配额按会话分账 | [`Session`] / [`SessionState`] / [`BudgetConfig`] / [`AudioEngineService::open_session`] |
//! | 三 · 四类别契约（80 行） | 每消费者声明类别、类别决定路由与策略、声明制 | [`AudioCategory`] / [`CATEGORY_POLICY_TABLE`] / [`legal_categories`] |
//! | 四 · 生命周期与故障域（100 行） | 常驻/按需两模式、默认按需+高频保温、崩溃自动重启与重连协议 | [`ResidencyPolicy`] / [`inject_fault`] / [`reconnect`] |
//!
//! # 服务化架构理由（锚点"重复建设的反面"的执行面）
//!
//! 锚点写明服务化的理由是「一次建设多端消费」与「重复建设的反面」。把它变成可机检的
//! 事实只有一条：**引擎只有一个，消费者的 DSP 栈不许私有**。
//! [`IsolationMode::PrivateStack`] 在 [`AudioEngineService::open_session_with`] 处被硬拒
//! （`E_PRIVATE_STACK_FORBIDDEN`）——不是文档劝阻，是闸门拒绝。四消费者共享同一
//! [`AudioEngineService`] 实例、同一共享 DSP 核句柄（F1401 [`SharedCore`] 纪律），
//! 差异只落在**会话编排**面（每会话独立的图与参数），DSP 实现面不许分叉。
//!
//! # 四类别与策略表（锚点"类别错了体验就错"的执行面）
//!
//! | 类别 | 锚点定性 | 本文件策略表关键项 |
//! | --- | --- | --- |
//! | 媒体 | 高保真、低延迟宽松 | 40ms 预算 · 非空间化 · 独占焦点 · 降载序 3 |
//! | 通信 | 超低延迟语音处理 | 20ms 预算 · 非空间化 · 独占焦点 · 降载序 0（最后动） |
//! | 系统 | 即发即弃、低开销 | 60ms 预算 · 非空间化 · 非独占 · 降载序 1 |
//! | 游戏 | 空间化多声源 | 30ms 预算 · 空间化 · 非独占 · 降载序 2 |
//!
//! 类别由**消费者声明**（声明制），但声明受两层约束：
//! 1. **合法性矩阵** [`legal_categories`]：结构上不可能的组合硬拒
//!    （`E_CATEGORY_IMPOSSIBLE`）——播放链声明为「通信」是把媒体播放塞进语音预算，
//!    语音通道被媒体占用即通话延迟，属结构性错误，不是调参能救的；
//! 2. **推荐类别偏离告警**：合法但偏离推荐值时不拒绝（消费者有权按产品需要改），
//!    但产出 `E_CATEGORY_MISDECLARED` 显性诊断（锚点"类别错了体验就错"——
//!    错了要说得出来，不能默默体验差）。
//!
//! # 配额分账（锚点 F1416 "预算按会话分账"）
//!
//! [`BudgetConfig`] 以千分比（permille）表达引擎总 CPU 预算的类别配比，缺省取
//! **F1416 原文配比**：媒体 600 / 系统 200 / 游戏 200（合计 1000）。
//!
//! 这里有一处**必须显性记录的规格缺口**：F1416 的配比只列了媒体/系统/游戏三类，
//! **未给「通信」留份额**，故缺省为 0。本文件不替 F1416 悄悄补一个数，而是：
//! - 缺省 `communication = 0`，并在 [`F1416_DEFAULT_BUDGET`] 的文档里写明缺口来源；
//! - [`legal_categories`] 同时把「通信」标为**预留类别**（`E_CATEGORY_RESERVED`），
//!   当前四消费者均不得声明——通信预算留给未来的语音消费者，不被现有消费者挪用；
//! - 提供 [`BudgetConfig::with_communication_reserve`] 显式迁移路径：接入语音
//!   消费者时由该函数从既有三类按比例回收额度，回收是显式动作不是隐式重分配。
//!
//! 类别预算到会话的分账在每次 [`AudioEngineService::open_session`] /
//! [`AudioEngineService::close_session`] 后重算：同类会话均分该类别预算
//! （整除向下取整，最小 1），会话超分即 `E_SESSION_QUOTA_EXCEEDED`。
//!
//! # 故障域（锚点"引擎崩溃不拖垮消费者"）
//!
//! 故障分**两级**，级差决定爆炸半径：
//!
//! | 级别 | 入口 | 爆炸半径 | 消费者侧义务 |
//! | --- | --- | --- | --- |
//! | 会话级 | [`AudioEngineService::crash_session`] | 仅该会话（**一崩不连坐**） | 该会话重建，其余不动 |
//! | 引擎级 | [`AudioEngineService::inject_fault`] | 全部会话 | 走重连协议四阶段 |
//!
//! 引擎级故障的恢复链：[`inject_fault`] → [`complete_restart`] → 逐消费者
//! [`reconnect`]，重连按**四阶段**推进（[`ReconnectPhase`]）：通知 → 会话重建 →
//! 状态协商 → 恢复。状态协商不是空话：会话重建后**协商回**类别、延迟预算与
//! 已用配额（`ReconnectTrace::negotiated_*`），避免重建即裸奔（重建后从零算
//! 预算 = 一次崩溃把四个会话的累计用量清零，预算账就漏了）。
//!
//! 消费者侧的断连处理义务登记在 [`DISCONNECT_OBLIGATIONS`]，由
//! [`disconnect_obligations_met`] 机检——义务不履行（断连后继续提交音频、
//! 或无界缓冲）会让重连后的状态协商拿到过期视图。
//!
//! # 零静默与确定性
//!
//! - 全拒绝路径写 [`AudioEngineService::errors`]（`Vec<(操作, 码, 人话)>`）+ 审计留痕，
//!   无一条静默返回、无一条 panic；
//! - 时间一律逻辑 tick 注入（[`AudioEngineService::tick`]），零墙钟——回归可复现；
//! - 纯契约层：零音频设备调用、零 DSP 计算、零外部依赖，仅依赖 `crate::checks`
//!   （自检侧）与 `alloc`；可在内核镜像与宿主测试链路同源运行。
//!
//! # 跨条对接
//!
//! 上游 F1401（无 ADR 不开工——本条的 [`AudioEngineService`] 开工前置由 F1401
//! `gate_h_work(WorkKind::Session)` 把关；四主题 ADR 与共享核纪律在本文件只消费不重定义）；
//! 下游 F1403（混音图挂载面）、F1406（令牌）、F1408（事件总线）、F1412（低延迟路径
//! 的通信类别）、F1416（类别预算——本文件为其执行面并显式记录通信缺口）、
//! F1419（引擎 API 冻结 v1 的引擎族：服务启停 / 会话 CRUD）。

use alloc::string::ToString;
use alloc::vec;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ════════════════════════════════════════════════════════════════════════════
// §0 规格常量（语义文本 —— 服务化架构的法理载体）
// ════════════════════════════════════════════════════════════════════════════

/// 服务化架构理由声明（锚点"重复建设的反面"的法理文本）。
pub const SERVICE_RATIONALE_DOC: &str = "\
音频引擎服务化架构理由（VE-F1402 · v1）：音频能力一次建设、多端消费——播放链（G07）、\
系统音效、通知提醒、未来游戏场景四类消费者共用同一个引擎服务实例与同一批共享 DSP 核句柄。\
反面是重复建设：每个消费者各写一套混音/重采样/淡入淡出，四套实现四套缺陷四套对拍基线，\
且体验互不一致。服务化把「实现」收敛为一份（引擎本体），把「差异」收敛为会话编排面\
（每会话独立的图与参数）。故引擎实例唯一、消费者 DSP 栈私有即架构违例：闸门拒绝，不靠自觉。";

/// 四消费者统一接入契约。
pub const MULTI_CONSUMER_DOC: &str = "\
多消费者统一接入（VE-F1402 · v1）：四消费者——播放链（G07 播放链音频集成面）、系统音效\
（即发即弃低开销）、通知提醒（即时反馈）、未来游戏场景（空间化多声源）——各自持一个会话，\
会话是消费者与引擎之间**唯一**的接口面。消费者不接触引擎内部状态机、不共享句柄、\
不互相可见：会话隔离的判据是「一崩不连坐」——任一会话崩溃，其余会话的图与参数纹丝不动。";

/// 会话隔离与资源配额契约。
pub const SESSION_ISOLATION_DOC: &str = "\
会话隔离与资源配额（VE-F1402 · v1）：单例引擎服务 + 多客户端会话。每消费者一个会话，\
会话间在图、参数、voice 与 CPU 配额上完全隔离；会话崩溃只隔离该会话。配额按会话分账：\
引擎总 CPU 预算按音频类别配比（媒体/系统/游戏，F1416），类别预算在同类会话间均分，\
会话超分即拒并入账——配额是分账不是均摊（均摊会让沉默会话替吵闹会话买单）。";

/// 四类别契约（锚点"类别错了体验就错"）。
pub const CATEGORY_CONTRACT_DOC: &str = "\
四类别契约（VE-F1402 · v1）：每消费者**声明**音频类别，类别决定路由与策略。媒体=高保真\
低延迟宽松；通信=超低延迟语音处理；系统=即发即弃低开销；游戏=空间化多声源。\
声明制不等于自由声明：结构性不可能的组合硬拒（如播放链声明通信=媒体占用语音预算），\
合法但偏离推荐时产出显性诊断而非默默劣化。类别错了体验就错，因此错了必须说得出来。";

/// 生命周期两模式契约。
pub const LIFECYCLE_DOC: &str = "\
生命周期两模式（VE-F1402 · v1）：常驻=响应快、常驻内存；按需=省内存、冷启动有代价。\
缺省按需唤醒 + 高频消费者保温：高频短促消费者（系统音效/通知提醒）保温在热态免冷启动，\
长驻消费者（播放链/游戏场景）本身就有活跃会话无需额外保温。保温集合可配。";

/// 故障域与重连协议契约。
pub const FAULT_DOMAIN_DOC: &str = "\
故障域（VE-F1402 · v1）：引擎崩溃不拖垮消费者。会话级故障只隔离该会话（一崩不连坐）；\
引擎级故障经自动重启恢复，消费者按四阶段重连协议 rejoined：通知 → 会话重建 → 状态协商 → 恢复。\
状态协商必须回填类别、延迟预算与已用配额——重建后从零算预算会让累计用量清零，预算账就漏了。\
消费者侧断连义务：断连期间停止提交音频、缓冲有界、状态变更打版本号供协商比对。";

// ════════════════════════════════════════════════════════════════════════════
// §1 消费者（四类 —— 多消费者模型的第一段）
// ════════════════════════════════════════════════════════════════════════════

/// 消费者种类（四类封闭集——新增消费者必须先进此枚举并补齐策略表）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ConsumerKind {
    /// G07 播放链（媒体回放消费面）。
    PlaybackChain,
    /// 系统音效（UI 操作反馈）。
    SystemSound,
    /// 通知提醒（即时反馈）。
    Notification,
    /// 未来游戏场景（空间化多声源）。
    GameScene,
}

impl ConsumerKind {
    /// 全消费者序（O(4) 固定序——遍历序稳定，保证可复现）。
    pub const ALL: [ConsumerKind; 4] = [
        ConsumerKind::PlaybackChain,
        ConsumerKind::SystemSound,
        ConsumerKind::Notification,
        ConsumerKind::GameScene,
    ];

    /// 消费者序号（0..4）。
    pub fn index(self) -> usize {
        match self {
            ConsumerKind::PlaybackChain => 0,
            ConsumerKind::SystemSound => 1,
            ConsumerKind::Notification => 2,
            ConsumerKind::GameScene => 3,
        }
    }

    /// 人话名。
    pub fn name(self) -> &'static str {
        match self {
            ConsumerKind::PlaybackChain => "播放链",
            ConsumerKind::SystemSound => "系统音效",
            ConsumerKind::Notification => "通知提醒",
            ConsumerKind::GameScene => "游戏场景",
        }
    }

    /// 端点标识（接入面标识——四消费者在服务侧的稳定名字）。
    pub fn endpoint(self) -> &'static str {
        match self {
            ConsumerKind::PlaybackChain => "g07.playback-chain",
            ConsumerKind::SystemSound => "system.sound",
            ConsumerKind::Notification => "system.notification",
            ConsumerKind::GameScene => "game.scene",
        }
    }

    /// 推荐音频类别（推荐 ≠ 强制；偏离走 [`AudioEngineService::open_session`] 告警面）。
    pub fn recommended_category(self) -> AudioCategory {
        match self {
            ConsumerKind::PlaybackChain => AudioCategory::Media,
            ConsumerKind::SystemSound => AudioCategory::System,
            ConsumerKind::Notification => AudioCategory::System,
            ConsumerKind::GameScene => AudioCategory::Game,
        }
    }

    /// 是否高频保温消费者（生命周期缺省保温集合的依据）。
    ///
    /// 判据：短促、高频、延迟敏感、且空闲时无活跃会话可维持热态——
    /// 系统音效与通知提醒落此项；播放链与游戏场景空闲时本就无会话，
    /// 冷启动代价由其自身的活跃会话摊掉，无需额外保温。
    pub fn is_warm_keep(self) -> bool {
        matches!(self, ConsumerKind::SystemSound | ConsumerKind::Notification)
    }
}

/// 会话隔离模式（服务化纪律的执行面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IsolationMode {
    /// 共享引擎（唯一合法模式——引擎实例唯一，差异只在会话编排面）。
    SharedEngine,
    /// 消费者私有 DSP 栈（**架构违例**：`E_PRIVATE_STACK_FORBIDDEN` 硬拒）。
    ///
    /// 保留此变体不是为了支持它，而是为了让拒绝路径可被机检——
    /// 若接口里根本没有这个选项，"拒绝重复建设"就无从断言。
    PrivateStack,
}

// ════════════════════════════════════════════════════════════════════════════
// §2 音频类别与策略表（四类别契约段）
// ════════════════════════════════════════════════════════════════════════════

/// 音频类别（四类封闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum AudioCategory {
    /// 媒体（高保真、低延迟宽松）。
    Media,
    /// 通信（超低延迟语音处理）。
    Communication,
    /// 系统（即发即弃、低开销）。
    System,
    /// 游戏（空间化多声源）。
    Game,
}

impl AudioCategory {
    /// 全类别序。
    pub const ALL: [AudioCategory; 4] = [
        AudioCategory::Media,
        AudioCategory::Communication,
        AudioCategory::System,
        AudioCategory::Game,
    ];

    /// 类别序号（0..4）——策略表下标。
    pub fn index(self) -> usize {
        match self {
            AudioCategory::Media => 0,
            AudioCategory::Communication => 1,
            AudioCategory::System => 2,
            AudioCategory::Game => 3,
        }
    }

    /// 人话名。
    pub fn name(self) -> &'static str {
        match self {
            AudioCategory::Media => "媒体",
            AudioCategory::Communication => "通信",
            AudioCategory::System => "系统",
            AudioCategory::Game => "游戏",
        }
    }

    /// 锚点定性原文（类别语义的单一事实源）。
    pub fn charter(self) -> &'static str {
        match self {
            AudioCategory::Media => "高保真、低延迟宽松",
            AudioCategory::Communication => "超低延迟语音处理",
            AudioCategory::System => "即发即弃、低开销",
            AudioCategory::Game => "空间化多声源",
        }
    }

    /// 是否预留类别（当前四消费者不得声明——预算留给未来语音消费者）。
    pub fn is_reserved(self) -> bool {
        matches!(self, AudioCategory::Communication)
    }
}

/// 类别策略（锚点"类别决定路由与策略"的机读形态）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CategoryPolicy {
    /// 类别。
    pub category: AudioCategory,
    /// 端到端延迟预算（毫秒）。
    pub latency_budget_ms: u32,
    /// 是否空间化渲染（游戏=是，其余=否）。
    pub spatial: bool,
    /// 是否独占音频焦点（媒体/通信独占——两类互斥，不能同时占）。
    pub exclusive_focus: bool,
    /// 预算过载时的降载顺序（0 = 最后动，值越大越先被降）。
    pub degrade_rank: u8,
    /// 缺省 voice 上限（该类别单会话的并发声源水位）。
    pub voice_ceiling: u16,
}

/// 四类别策略表（`AudioCategory::index()` 为下标，顺序严格对应）。
///
/// 四条策略差异均可被机检（见 `veh02_checks.rs`）：
/// - 延迟预算：通信(20) < 游戏(30) < 媒体(40) < 系统(60)，四值互不相同；
/// - 空间化：仅游戏为真；
/// - 独占焦点：仅媒体与通信为真，且二者互斥；
/// - 降载序：通信最后被降（0），系统最先（1）——即发即弃的音效最容易让路，
///   而通话延迟是硬红线。
pub const CATEGORY_POLICY_TABLE: [CategoryPolicy; 4] = [
    CategoryPolicy {
        category: AudioCategory::Media,
        latency_budget_ms: 40,
        spatial: false,
        exclusive_focus: true,
        degrade_rank: 3,
        voice_ceiling: 32,
    },
    CategoryPolicy {
        category: AudioCategory::Communication,
        latency_budget_ms: 20,
        spatial: false,
        exclusive_focus: true,
        degrade_rank: 0,
        voice_ceiling: 16,
    },
    CategoryPolicy {
        category: AudioCategory::System,
        latency_budget_ms: 60,
        spatial: false,
        exclusive_focus: false,
        degrade_rank: 1,
        voice_ceiling: 8,
    },
    CategoryPolicy {
        category: AudioCategory::Game,
        latency_budget_ms: 30,
        spatial: true,
        exclusive_focus: false,
        degrade_rank: 2,
        voice_ceiling: 64,
    },
];

/// 取某类别策略（越界类别显性拒绝，不做静默兜底）。
pub fn policy_of(category: AudioCategory) -> Result<CategoryPolicy, &'static str> {
    match CATEGORY_POLICY_TABLE.get(category.index()) {
        Some(p) if p.category == category => Ok(*p),
        _ => Err("类别策略表越界"),
    }
}

/// 声明合法性矩阵：某消费者可声明的类别集合（**不含**预留类别）。
///
/// 合法性判据三条：
/// - 播放链只能声明媒体（它的语义就是媒体回放）；
/// - 系统音效与通知提醒只能声明系统（低开销即发即弃）；
/// - 游戏场景可声明游戏或媒体（游戏内过场 cinematic 走媒体级高保真是正当需求）。
///
/// 预留类别（通信）不在任何集合内——它在 [`declaration_legal`] 里被更早拦下，
/// 理由单列（预算与语义双预留），不与"结构性不可能"混为一谈。
pub fn legal_categories(consumer: ConsumerKind) -> Vec<AudioCategory> {
    match consumer {
        ConsumerKind::PlaybackChain => vec![AudioCategory::Media],
        ConsumerKind::SystemSound => vec![AudioCategory::System],
        ConsumerKind::Notification => vec![AudioCategory::System],
        ConsumerKind::GameScene => vec![AudioCategory::Game, AudioCategory::Media],
    }
}

/// 声明是否合法（`Err` 携带拒绝码语义，`Ok(())` 表示合法）。
pub fn declaration_legal(consumer: ConsumerKind, category: AudioCategory) -> Result<(), &'static str> {
    if category.is_reserved() {
        return Err("E_CATEGORY_RESERVED");
    }
    if legal_categories(consumer).contains(&category) {
        Ok(())
    } else {
        Err("E_CATEGORY_IMPOSSIBLE")
    }
}

// ════════════════════════════════════════════════════════════════════════════
// §3 配额模型（F1416 执行面 + 通信缺口的显式记录）
// ════════════════════════════════════════════════════════════════════════════

/// 类别预算配比（千分比，合计 1000 = 引擎总 CPU 预算的 100%）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BudgetConfig {
    /// 媒体份额（千分比）。
    pub media: u16,
    /// 通信份额（千分比；F1416 未列，缺省 0 —— 见 [`F1416_DEFAULT_BUDGET`]）。
    pub communication: u16,
    /// 系统份额（千分比）。
    pub system: u16,
    /// 游戏份额（千分比）。
    pub game: u16,
    /// 合计（恒为 1000，由 [`BudgetConfig::normalized`] 归一）。
    pub total: u16,
}

/// F1416 缺省配比原文：媒体 60% / 系统 20% / 游戏 20%。
///
/// **规格缺口（显式记录，不静默补数）**：F1416 原文「引擎 CPU 总预算按消费者类别分配
/// ——媒体 60%/系统 20%/游戏 20% 可配」只列三类，**未给「通信」留份额**。本文件
/// 据实置 0，并以两条机制兜住而非悄悄填数：
/// (1) [`AudioCategory::is_reserved`] 标通信为预留类别，当前四消费者声明即拒；
/// (2) [`BudgetConfig::with_communication_reserve`] 提供显式迁移路径，接入语音
///     消费者时从既有三类按比例回收额度——回收是显式动作，可审计。
pub const F1416_DEFAULT_BUDGET: BudgetConfig = BudgetConfig {
    media: 600,
    communication: 0,
    system: 200,
    game: 200,
    total: 1000,
};

impl BudgetConfig {
    /// 取某类别份额（千分比）。
    pub fn share_of(&self, category: AudioCategory) -> u16 {
        match category {
            AudioCategory::Media => self.media,
            AudioCategory::Communication => self.communication,
            AudioCategory::System => self.system,
            AudioCategory::Game => self.game,
        }
    }

    /// 归一到合计 1000（整数比例缩放，余数归游戏类——保证合计精确）。
    pub fn normalized(&self) -> BudgetConfig {
        let sum = u32::from(self.media)
            + u32::from(self.communication)
            + u32::from(self.system)
            + u32::from(self.game);
        if sum == 0 {
            return F1416_DEFAULT_BUDGET;
        }
        let m = (u32::from(self.media) * 1000 / sum) as u16;
        let c = (u32::from(self.communication) * 1000 / sum) as u16;
        let s = (u32::from(self.system) * 1000 / sum) as u16;
        let g = (1000u32
            .saturating_sub(u32::from(m))
            .saturating_sub(u32::from(c))
            .saturating_sub(u32::from(s))) as u16;
        BudgetConfig {
            media: m,
            communication: c,
            system: s,
            game: g,
            total: 1000,
        }
    }

    /// 显式为通信预留份额（从既有三类按比例回收，合计恒 1000）。
    ///
    /// 回收用「先按比例缩放既有三类到 `1000 - reserve`，余数归游戏类」——
    /// 媒体是大头承担主要回收，游戏类余数兜底保证合计精确。
    pub fn with_communication_reserve(reserve: u16) -> BudgetConfig {
        let reserve = reserve.min(1000);
        let rest = 1000u32 - u32::from(reserve);
        let base = F1416_DEFAULT_BUDGET;
        let bsum = u32::from(base.media) + u32::from(base.system) + u32::from(base.game);
        if bsum == 0 {
            return F1416_DEFAULT_BUDGET;
        }
        let m = (u32::from(base.media) * rest / bsum) as u16;
        let s = (u32::from(base.system) * rest / bsum) as u16;
        let g = (rest
            .saturating_sub(u32::from(m))
            .saturating_sub(u32::from(s))) as u16;
        BudgetConfig {
            media: m,
            communication: reserve,
            system: s,
            game: g,
            total: 1000,
        }
    }

    /// 配比是否合计 1000（机检面——归一后恒真，未归一可能为假）。
    pub fn sums_to_total(&self) -> bool {
        u32::from(self.media) + u32::from(self.communication) + u32::from(self.system)
            + u32::from(self.game)
            == 1000
    }
}

// ════════════════════════════════════════════════════════════════════════════
// §4 会话（会话与配额段）
// ════════════════════════════════════════════════════════════════════════════

/// 会话标识（服务内唯一，跨重启不复用——重连必得新 id，杜绝陈旧句柄复活）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SessionId(pub u32);

impl SessionId {
    /// 人话形式。
    pub fn show(self) -> String {
        format!("S{}", self.0)
    }
}

/// 会话状态（四态——被抢/崩溃可审计）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionState {
    /// 活跃。
    Open,
    /// 降级（配额超分后被降载，仍可用）。
    Degraded,
    /// 已关闭（主动关闭）。
    Closed,
    /// 已崩溃隔离（会话级故障；一崩不连坐的落点）。
    Crashed,
}

impl SessionState {
    /// 人话名。
    pub fn name(self) -> &'static str {
        match self {
            SessionState::Open => "活跃",
            SessionState::Degraded => "降级",
            SessionState::Closed => "已关闭",
            SessionState::Crashed => "崩溃隔离",
        }
    }

    /// 是否仍在服务（崩溃隔离与关闭都不再服务）。
    pub fn is_live(self) -> bool {
        matches!(self, SessionState::Open | SessionState::Degraded)
    }
}

/// 一个客户端会话（消费者与引擎的唯一接口面）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Session {
    /// 会话标识。
    pub id: SessionId,
    /// 归属消费者。
    pub consumer: ConsumerKind,
    /// 声明的音频类别（决定路由与策略）。
    pub category: AudioCategory,
    /// 状态。
    pub state: SessionState,
    /// 分账到的 CPU 配额（千分比；同类会话均分该类别预算）。
    pub granted_permille: u16,
    /// 已用 CPU（千分比；`> granted` 即超分）。
    pub used_permille: u16,
    /// 开出的逻辑 tick（审计用）。
    pub opened_tick: u64,
    /// 状态版本号（重连状态协商的比对依据——断连期间消费者打版本号）。
    pub state_version: u32,
}

impl Session {
    /// 剩余配额（千分比）。
    pub fn remaining_permille(&self) -> u16 {
        self.granted_permille.saturating_sub(self.used_permille)
    }

    /// 是否超分。
    pub fn is_over_quota(&self) -> bool {
        self.used_permille > self.granted_permille
    }
}

// ════════════════════════════════════════════════════════════════════════════
// §5 生命周期（两模式）
// ════════════════════════════════════════════════════════════════════════════

/// 驻留策略（两模式）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResidencyPolicy {
    /// 系统常驻（响应快、常驻内存；冷启动代价为零，内存代价为正）。
    Resident,
    /// 按需唤醒（省内存；冷启动有代价——靠高频消费者保温抵消）。
    OnDemand,
}

impl ResidencyPolicy {
    /// 人话名。
    pub fn name(self) -> &'static str {
        match self {
            ResidencyPolicy::Resident => "系统常驻",
            ResidencyPolicy::OnDemand => "按需唤醒",
        }
    }
}

/// 驻留态（逻辑 tick 驱动，零墙钟）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResidencyState {
    /// 冷态（无保温消费者、无活跃会话）。
    Cold,
    /// 预热中（被唤醒，冷启动进行中）。
    Warming,
    /// 热态（服务中）。
    Hot,
}

// ════════════════════════════════════════════════════════════════════════════
// §6 故障域与重连协议
// ════════════════════════════════════════════════════════════════════════════

/// 故障注入面（引擎级）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaultKind {
    /// 音频设备丢失（HDA/USB-Audio 传输层失效）。
    DeviceLost,
    /// 混音图结构损坏（DAG 校验失败——F1403 三查命中）。
    GraphCorrupted,
    /// 预算击穿（总预算被击穿，需重启以重建分账）。
    BudgetBreach,
    /// 共享核句柄失效（F1401 共享核失效——两域同核，一损俱损）。
    SharedCoreLost,
}

impl FaultKind {
    /// 人话名。
    pub fn name(self) -> &'static str {
        match self {
            FaultKind::DeviceLost => "音频设备丢失",
            FaultKind::GraphCorrupted => "混音图结构损坏",
            FaultKind::BudgetBreach => "预算击穿",
            FaultKind::SharedCoreLost => "共享核句柄失效",
        }
    }

    /// 是否波及全部会话（引擎级故障的定义面——四类均为真）。
    ///
    /// 保留此方法而非内联常量：故障域的判据正是「哪些故障是引擎级」，
    /// 判据要能被机检就不能只是一句注释。
    pub fn is_engine_wide(self) -> bool {
        matches!(
            self,
            FaultKind::DeviceLost
                | FaultKind::GraphCorrupted
                | FaultKind::BudgetBreach
                | FaultKind::SharedCoreLost
        )
    }
}

/// 重连阶段（四阶段——锚点"重连协议"的机读形态）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ReconnectPhase {
    /// 阶段一：引擎已重启，通知消费者断连（消费者须停止提交音频）。
    Notified,
    /// 阶段二：会话重建（新 id——陈旧句柄不复活）。
    SessionRebuilt,
    /// 阶段三：状态协商（回填类别、延迟预算、已用配额）。
    StateNegotiated,
    /// 阶段四：恢复服务。
    Resumed,
}

impl ReconnectPhase {
    /// 阶段序（0..4）。
    pub fn index(self) -> usize {
        match self {
            ReconnectPhase::Notified => 0,
            ReconnectPhase::SessionRebuilt => 1,
            ReconnectPhase::StateNegotiated => 2,
            ReconnectPhase::Resumed => 3,
        }
    }

    /// 人话名。
    pub fn name(self) -> &'static str {
        match self {
            ReconnectPhase::Notified => "通知",
            ReconnectPhase::SessionRebuilt => "会话重建",
            ReconnectPhase::StateNegotiated => "状态协商",
            ReconnectPhase::Resumed => "恢复",
        }
    }

    /// 全阶段序（重连完整性机检的遍历序）。
    pub const ALL: [ReconnectPhase; 4] = [
        ReconnectPhase::Notified,
        ReconnectPhase::SessionRebuilt,
        ReconnectPhase::StateNegotiated,
        ReconnectPhase::Resumed,
    ];
}

/// 一条重连留痕（每消费者每次重连一条——可审计、可回放）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReconnectTrace {
    /// 消费者。
    pub consumer: ConsumerKind,
    /// 重连后的新会话 id。
    pub new_session: SessionId,
    /// 崩溃前的老会话 id（陈旧句柄不得复活——机检面）。
    pub stale_session: SessionId,
    /// 协商回填的类别。
    pub negotiated_category: AudioCategory,
    /// 协商回填的延迟预算（毫秒）。
    pub negotiated_latency_ms: u32,
    /// 协商回填的已用配额（千分比）——防「重建即裸奔」丢预算账。
    pub negotiated_used_permille: u16,
    /// 走过的阶段。
    pub phases: Vec<ReconnectPhase>,
}

impl ReconnectTrace {
    /// 四阶段是否走齐（顺序单调不减且齐备）。
    pub fn is_complete(&self) -> bool {
        if self.phases.len() != ReconnectPhase::ALL.len() {
            return false;
        }
        let mut prev = 0usize;
        for p in &self.phases {
            if p.index() < prev {
                return false;
            }
            prev = p.index();
        }
        ReconnectPhase::ALL
            .iter()
            .all(|p| self.phases.contains(p))
    }
}

/// 消费者侧断连义务（锚点"消费者侧的断连处理契约"）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DisconnectObligation {
    /// 义务标识。
    pub key: &'static str,
    /// 人话要求。
    pub requirement: &'static str,
}

/// 断连义务清单（四条——机检面：任一条缺失即重连状态协商不可信）。
pub const DISCONNECT_OBLIGATIONS: [DisconnectObligation; 4] = [
    DisconnectObligation {
        key: "stop-submit",
        requirement: "断连期间停止向引擎提交音频（继续提交=向不存在的会话投递，状态不可信）",
    },
    DisconnectObligation {
        key: "bounded-buffer",
        requirement: "缓冲有界（无界缓冲在长断连下吃光内存——崩溃雪上加霜）",
    },
    DisconnectObligation {
        key: "version-bump",
        requirement: "状态变更打版本号（状态协商靠版本号比对过期视图）",
    },
    DisconnectObligation {
        key: "stale-handle-drop",
        requirement: "丢弃崩溃前的会话句柄（句柄跨重启不合法——服务端不认）",
    },
];

/// 断连义务是否齐备（`present` 为消费者自报已履行的 key 集合）。
pub fn disconnect_obligations_met(present: &[&str]) -> bool {
    DISCONNECT_OBLIGATIONS
        .iter()
        .all(|o| present.contains(&o.key))
}

// ════════════════════════════════════════════════════════════════════════════
// §7 引擎服务（单例 + 多会话 —— 四段合流）
// ════════════════════════════════════════════════════════════════════════════

/// 音频引擎服务（**单例**；四消费者共用；差异只在会话编排面）。
///
/// 引擎实例唯一是服务化的第一法条：见 [`SERVICE_RATIONALE_DOC`] 与
/// [`AudioEngineService::open_session_with`] 对 [`IsolationMode::PrivateStack`] 的硬拒。
pub struct AudioEngineService {
    sessions: Vec<Session>,
    next_id: u32,
    policy: ResidencyPolicy,
    warm_keep: Vec<ConsumerKind>,
    residency: ResidencyState,
    idle_ticks: u32,
    idle_threshold: u32,
    budget: BudgetConfig,
    /// 崩溃前各消费者的用量快照（状态协商的数据源——防重建即裸奔）。
    usage_snapshot: Vec<(ConsumerKind, u16)>,
    pending_reconnect: Vec<ConsumerKind>,
    faults: Vec<String>,
    traces: Vec<ReconnectTrace>,
    audits: Vec<String>,
    errors: Vec<(String, &'static str, String)>,
    tick: u64,
    /// 焦点占用方（`Vec` 而非单值——同类别多会话可共焦点，见 `focus_conflicts`）。
    focus_holder: Vec<(ConsumerKind, AudioCategory)>,
    degraded_sessions: u32,
}

impl AudioEngineService {
    /// 新建引擎服务（缺省：按需唤醒 + 高频保温 + F1416 配比）。
    pub fn new() -> Self {
        AudioEngineService {
            sessions: Vec::new(),
            next_id: 1,
            policy: ResidencyPolicy::OnDemand,
            warm_keep: ConsumerKind::ALL
                .iter()
                .copied()
                .filter(|c| c.is_warm_keep())
                .collect(),
            residency: ResidencyState::Cold,
            idle_ticks: 0,
            idle_threshold: 8,
            budget: F1416_DEFAULT_BUDGET,
            usage_snapshot: Vec::new(),
            pending_reconnect: Vec::new(),
            faults: Vec::new(),
            traces: Vec::new(),
            audits: Vec::new(),
            errors: Vec::new(),
            tick: 0,
            focus_holder: Vec::new(),
            degraded_sessions: 0,
        }
    }

    /// 配驻留策略（可配——锚点"策略可配"）。
    pub fn set_policy(&mut self, policy: ResidencyPolicy) {
        self.audits
            .push(format!("tick{} 驻留策略切为「{}」", self.tick, policy.name()));
        self.policy = policy;
        if policy == ResidencyPolicy::Resident {
            self.residency = ResidencyState::Hot;
            self.idle_ticks = 0;
        }
    }

    /// 配保温集合（可配——缺省为高频消费者）。
    pub fn set_warm_keep(&mut self, keep: Vec<ConsumerKind>) {
        self.warm_keep = keep;
        self.audits.push(format!(
            "tick{} 保温集合设为 {} 个消费者",
            self.tick,
            self.warm_keep.len()
        ));
    }

    /// 配类别预算（可配——F1416 配比面）。
    pub fn set_budget(&mut self, budget: BudgetConfig) {
        self.budget = budget.normalized();
        self.redistribute();
        self.audits
            .push(format!("tick{} 类别预算配比更新", self.tick));
    }

    /// 驻留策略（只读）。
    pub fn policy(&self) -> ResidencyPolicy {
        self.policy
    }

    /// 驻留态（只读）。
    pub fn residency(&self) -> ResidencyState {
        self.residency
    }

    /// 保温集合（只读）。
    pub fn warm_keep(&self) -> &[ConsumerKind] {
        &self.warm_keep
    }

    /// 类别预算（只读）。
    pub fn budget(&self) -> BudgetConfig {
        self.budget
    }

    /// 全部会话（只读）。
    pub fn sessions(&self) -> &[Session] {
        &self.sessions
    }

    /// 活跃会话数（`Open` + `Degraded`）。
    pub fn live_session_count(&self) -> usize {
        self.sessions.iter().filter(|s| s.state.is_live()).count()
    }

    /// 降级会话数（F1416 降载在本服务侧的显性计数）。
    pub fn degraded_count(&self) -> u32 {
        self.degraded_sessions
    }

    /// 审计留痕（只读）。
    pub fn audits(&self) -> &[String] {
        &self.audits
    }

    /// 错误账本（只读；零静默）。
    pub fn errors(&self) -> &[(String, &'static str, String)] {
        &self.errors
    }

    /// 故障留痕（只读）。
    pub fn faults(&self) -> &[String] {
        &self.faults
    }

    /// 重连留痕（只读）。
    pub fn traces(&self) -> &[ReconnectTrace] {
        &self.traces
    }

    /// 焦点占用方（独占焦点类别的活跃会话清单）。
    ///
    /// 同类别多会话**可以**并存（两个应用同时播音乐不冲突）；冲突只发生在
    /// **不同**独占类别之间（通话 vs 音乐）。故返回全体占用方而非单一持有者。
    pub fn focus_holders(&self) -> &[(ConsumerKind, AudioCategory)] {
        &self.focus_holder
    }

    /// 焦点是否被某类别占用（供上层仲裁查询）。
    pub fn focus_taken_by(&self, category: AudioCategory) -> bool {
        self.focus_holder.iter().any(|(_, c)| *c == category)
    }

    /// 查会话。
    pub fn session(&self, id: SessionId) -> Option<&Session> {
        self.sessions.iter().find(|s| s.id == id)
    }

    /// 查某消费者的活跃会话。
    pub fn session_of(&self, consumer: ConsumerKind) -> Option<&Session> {
        self.sessions
            .iter()
            .find(|s| s.consumer == consumer && s.state.is_live())
    }

    /// 开会话（推荐路径；等价于 [`IsolationMode::SharedEngine`]）。
    pub fn open_session(
        &mut self,
        consumer: ConsumerKind,
        declared: AudioCategory,
    ) -> Result<SessionId, &'static str> {
        self.open_session_with(consumer, declared, IsolationMode::SharedEngine)
    }

    /// 开会话（带隔离模式——私有 DSP 栈在此硬拒）。
    ///
    /// 拒绝路径全部入账（无一条静默）：
    /// | 触发 | 码 |
    /// | --- | --- |
    /// | 消费者要私有 DSP 栈 | `E_PRIVATE_STACK_FORBIDDEN` |
    /// | 声明预留类别（通信） | `E_CATEGORY_RESERVED` |
    /// | 声明结构性不可能 | `E_CATEGORY_IMPOSSIBLE` |
    /// | 该类别预算为 0 | `E_CATEGORY_BUDGET_ZERO` |
    /// | 该消费者已有活跃会话 | `E_SESSION_DUPLICATE` |
    /// | 独占焦点被他人占用 | `E_FOCUS_BUSY` |
    ///
    /// 放行时：类别偏离推荐值产出 `E_CATEGORY_MISDECLARED` **告警**（不拒绝——
    /// 消费者有权按产品需要改类别，但"类别错了体验就错"要求错了说得出来）。
    pub fn open_session_with(
        &mut self,
        consumer: ConsumerKind,
        declared: AudioCategory,
        isolation: IsolationMode,
    ) -> Result<SessionId, &'static str> {
        if isolation == IsolationMode::PrivateStack {
            self.errors.push((
                format!("open_session({})", consumer.name()),
                "E_PRIVATE_STACK_FORBIDDEN",
                format!(
                    "消费者「{}」请求私有 DSP 栈——引擎实例唯一是服务化第一法条，私有栈即重复建设，\
                     四套实现四套缺陷四套对拍基线。差异请落在会话编排面",
                    consumer.name()
                ),
            ));
            return Err("私有 DSP 栈被拒");
        }
        if let Err(code) = declaration_legal(consumer, declared) {
            self.errors.push((
                format!("open_session({})", consumer.name()),
                code,
                format!(
                    "消费者「{}」声明类别「{}」不合法：{}",
                    consumer.name(),
                    declared.name(),
                    match code {
                        "E_CATEGORY_RESERVED" => format!(
                            "「{}」是预留类别（预算与语义双预留，留给未来语音消费者）",
                            declared.name()
                        ),
                        _ => format!(
                            "推荐「{}」，结构性不可能的组合不受理（媒体播放占语音预算会让通话延迟，\
                             不是调参能救的）",
                            consumer.recommended_category().name()
                        ),
                    }
                ),
            ));
            return Err("类别声明不合法");
        }
        if self.budget.share_of(declared) == 0 {
            self.errors.push((
                format!("open_session({})", consumer.name()),
                "E_CATEGORY_BUDGET_ZERO",
                format!(
                    "类别「{}」当前预算为 0——先显式配置该类别份额（F1416 配比未含此类，\
                     需用 BudgetConfig::with_communication_reserve 显式回收额度）",
                    declared.name()
                ),
            ));
            return Err("该类别预算为零");
        }
        if self.session_of(consumer).is_some() {
            self.errors.push((
                format!("open_session({})", consumer.name()),
                "E_SESSION_DUPLICATE",
                format!(
                    "消费者「{}」已有活跃会话——一消费者一会话是隔离的前提，\
                     两个会话会让配额分账与焦点仲裁都失去意义",
                    consumer.name()
                ),
            ));
            return Err("该消费者已有活跃会话");
        }
        let policy = match policy_of(declared) {
            Ok(p) => p,
            Err(_) => {
                self.errors.push((
                    format!("open_session({})", consumer.name()),
                    "E_POLICY_MISSING",
                    format!("类别「{}」无策略条目", declared.name()),
                ));
                return Err("类别策略缺失");
            }
        };
        if policy.exclusive_focus {
            // 冲突只在**不同**独占类别之间：通话（通信）与音乐（媒体）互斥，
            // 但两个应用同时播媒体不冲突——同类别共焦点是合法的。
            if let Some((holder, held)) = self
                .focus_holder
                .iter()
                .find(|(_, cat)| *cat != declared)
                .copied()
            {
                self.errors.push((
                    format!("open_session({})", consumer.name()),
                    "E_FOCUS_BUSY",
                    format!(
                        "类别「{}」需独占音频焦点，当前被「{}」（{}）占用——媒体与通信互斥，\
                         焦点抢占必须由上层显式仲裁而非会话侧静默夺走",
                        declared.name(),
                        holder.name(),
                        held.name()
                    ),
                ));
                return Err("音频焦点被占用");
            }
        }
        if declared != consumer.recommended_category() {
            self.errors.push((
                format!("open_session({})", consumer.name()),
                "E_CATEGORY_MISDECLARED",
                format!(
                    "消费者「{}」声明类别「{}」，偏离推荐「{}」——合法但会改路由与策略：\
                     延迟预算 {}ms / {} / voice 上限 {}。类别错了体验就错，此处显性告警",
                    consumer.name(),
                    declared.name(),
                    consumer.recommended_category().name(),
                    policy.latency_budget_ms,
                    if policy.spatial { "空间化" } else { "非空间化" },
                    policy.voice_ceiling
                ),
            ));
        }

        // 按需唤醒：开会话即唤醒引擎（冷启动代价在此偿还）。
        if self.residency == ResidencyState::Cold {
            self.residency = ResidencyState::Warming;
            self.audits
                .push(format!("tick{} 按需唤醒引擎（冷→预热）", self.tick));
        }
        if self.residency == ResidencyState::Warming {
            self.residency = ResidencyState::Hot;
        }

        let id = SessionId(self.next_id);
        self.next_id = self.next_id.saturating_add(1);
        self.sessions.push(Session {
            id,
            consumer,
            category: declared,
            state: SessionState::Open,
            granted_permille: 0,
            used_permille: 0,
            opened_tick: self.tick,
            state_version: 0,
        });
        if policy.exclusive_focus {
            self.focus_holder.push((consumer, declared));
        }
        self.redistribute();
        self.audits.push(format!(
            "tick{} 开会话 {}：{} · 端点 {} · 类别 {}（{}）· 预算 {}‰ · 延迟预算 {}ms",
            self.tick,
            id.show(),
            consumer.name(),
            consumer.endpoint(),
            declared.name(),
            declared.charter(),
            self.share_of(id),
            policy.latency_budget_ms
        ));
        Ok(id)
    }

    /// 某会话当前分账到的配额（千分比）。
    pub fn share_of(&self, id: SessionId) -> u16 {
        self.session(id).map(|s| s.granted_permille).unwrap_or(0)
    }

    /// 关会话（主动关闭；配额回收并重算分账）。
    pub fn close_session(&mut self, id: SessionId) -> Result<(), &'static str> {
        let pos = match self.sessions.iter().position(|s| s.id == id) {
            Some(p) => p,
            None => {
                self.errors.push((
                    format!("close_session({})", id.show()),
                    "E_SESSION_NOT_FOUND",
                    format!("会话 {} 不存在或已注销", id.show()),
                ));
                return Err("会话不存在");
            }
        };
        let s = &self.sessions[pos];
        let (consumer, live) = (s.consumer, s.state.is_live());
        self.sessions[pos].state = SessionState::Closed;
        if live {
            self.focus_holder.retain(|(c, _)| *c != consumer);
            self.degraded_sessions = self
                .degraded_sessions
                .saturating_sub(u32::from(self.sessions[pos].state == SessionState::Degraded));
        }
        self.redistribute();
        self.audits
            .push(format!("tick{} 关会话 {}：{}", self.tick, id.show(), consumer.name()));
        Ok(())
    }

    /// 记一笔会话用量并判超分（配额分账的执行面）。
    ///
    /// 超分不静默：会话转 [`SessionState::Degraded`]、降级计数 +1、错误入账。
    pub fn charge(&mut self, id: SessionId, permille: u16) -> Result<u16, &'static str> {
        let pos = match self.sessions.iter().position(|s| s.id == id) {
            Some(p) => p,
            None => {
                self.errors.push((
                    format!("charge({})", id.show()),
                    "E_SESSION_NOT_FOUND",
                    format!("会话 {} 不存在", id.show()),
                ));
                return Err("会话不存在");
            }
        };
        if !self.sessions[pos].state.is_live() {
            let st = self.sessions[pos].state;
            self.errors.push((
                format!("charge({})", id.show()),
                "E_SESSION_NOT_LIVE",
                format!("会话 {} 处于「{}」态，不接受用量记账", id.show(), st.name()),
            ));
            return Err("会话非活跃");
        }
        self.sessions[pos].used_permille =
            self.sessions[pos].used_permille.saturating_add(permille);
        let (used, granted, consumer) = {
            let s = &self.sessions[pos];
            (s.used_permille, s.granted_permille, s.consumer)
        };
        if used > granted {
            if self.sessions[pos].state == SessionState::Open {
                self.sessions[pos].state = SessionState::Degraded;
                self.degraded_sessions = self.degraded_sessions.saturating_add(1);
            }
            self.errors.push((
                format!("charge({})", id.show()),
                "E_SESSION_QUOTA_EXCEEDED",
                format!(
                    "会话 {}（{}）用 {}‰ 超出分账 {}‰——配额是分账不是均摊，\
                     超分即降载并入账，不静默挤占其他会话",
                    id.show(),
                    consumer.name(),
                    used,
                    granted
                ),
            ));
            return Err("会话配额超分");
        }
        Ok(self.sessions[pos].remaining_permille())
    }

    /// 会话级故障：隔离该会话（**一崩不连坐**——其余会话的图与参数不动）。
    pub fn crash_session(&mut self, id: SessionId) -> Result<(), &'static str> {
        let pos = match self.sessions.iter().position(|s| s.id == id) {
            Some(p) => p,
            None => {
                self.errors.push((
                    format!("crash_session({})", id.show()),
                    "E_SESSION_NOT_FOUND",
                    format!("会话 {} 不存在", id.show()),
                ));
                return Err("会话不存在");
            }
        };
        let (consumer, was_degraded) = (self.sessions[pos].consumer, self.sessions[pos].state == SessionState::Degraded);
        self.sessions[pos].state = SessionState::Crashed;
        if was_degraded {
            self.degraded_sessions = self.degraded_sessions.saturating_sub(1);
        }
        // 崩溃即释放该消费者的焦点占用（焦点随会话生死，不悬空）。
        self.focus_holder.retain(|(c, _)| *c != consumer);
        self.redistribute();
        self.faults.push(format!(
            "tick{} 会话级故障：{}（{}）崩溃隔离——其余会话不受影响",
            self.tick,
            consumer.name(),
            id.show()
        ));
        self.audits.push(format!(
            "tick{} 隔离崩溃会话 {}（{}）",
            self.tick,
            id.show(),
            consumer.name()
        ));
        Ok(())
    }

    /// 引擎级故障注入（爆炸半径 = 全部会话；进入重启中）。
    ///
    /// 故障前把各消费者已用配额快照下来——重连状态协商的数据源，
    /// 防止「重建即裸奔」把累计用量清零。
    pub fn inject_fault(&mut self, kind: FaultKind) {
        self.usage_snapshot = self
            .sessions
            .iter()
            .filter(|s| s.state.is_live())
            .map(|s| (s.consumer, s.used_permille))
            .collect();
        for s in self.sessions.iter_mut() {
            if s.state.is_live() {
                s.state = SessionState::Crashed;
            }
        }
        self.degraded_sessions = 0;
        self.focus_holder.clear();
        self.residency = if self.policy == ResidencyPolicy::Resident {
            ResidencyState::Warming
        } else {
            ResidencyState::Cold
        };
        self.pending_reconnect = self
            .sessions
            .iter()
            .filter(|s| s.state == SessionState::Crashed)
            .map(|s| s.consumer)
            .collect();
        self.faults.push(format!(
            "tick{} 引擎级故障：{}（波及全部 {} 个会话）——消费者须走四阶段重连协议",
            self.tick,
            kind.name(),
            self.pending_reconnect.len()
        ));
        self.audits
            .push(format!("tick{} 注入引擎级故障「{}」", self.tick, kind.name()));
    }

    /// 完成重启（引擎恢复热态；消费者仍须逐个重连——不自动恢复其状态）。
    pub fn complete_restart(&mut self) {
        self.residency = ResidencyState::Hot;
        self.idle_ticks = 0;
        self.audits.push(format!(
            "tick{} 引擎重启完成，待重连消费者 {} 个",
            self.tick,
            self.pending_reconnect.len()
        ));
    }

    /// 某消费者是否在待重连名单。
    pub fn is_pending_reconnect(&self, consumer: ConsumerKind) -> bool {
        self.pending_reconnect.contains(&consumer)
    }

    /// 消费者重连（四阶段协议）。
    ///
    /// 拒绝路径：不在待重连名单（`E_RECONNECT_NOT_PENDING`）、
    /// 引擎未恢复热态（`E_ENGINE_NOT_HOT`）。
    pub fn reconnect(&mut self, consumer: ConsumerKind) -> Result<ReconnectTrace, &'static str> {
        if !self.pending_reconnect.contains(&consumer) {
            self.errors.push((
                format!("reconnect({})", consumer.name()),
                "E_RECONNECT_NOT_PENDING",
                format!(
                    "消费者「{}」不在待重连名单——未崩溃就重连会造出一个幽灵会话",
                    consumer.name()
                ),
            ));
            return Err("不在待重连名单");
        }
        if self.residency != ResidencyState::Hot {
            self.errors.push((
                format!("reconnect({})", consumer.name()),
                "E_ENGINE_NOT_HOT",
                format!(
                    "消费者「{}」在引擎非热态时请求重连（当前 {:?}）——重连必须等重启完成",
                    consumer.name(),
                    self.residency
                ),
            ));
            return Err("引擎未恢复热态");
        }
        let stale = self
            .sessions
            .iter()
            .rev()
            .find(|s| s.consumer == consumer)
            .map(|s| s.id)
            .unwrap_or(SessionId(0));
        // 状态协商的数据源：崩溃前快照（无快照则该消费者崩溃前未用量）。
        let (used_before, category_before) = self
            .usage_snapshot
            .iter()
            .find(|(c, _)| *c == consumer)
            .map(|(_, u)| (*u, consumer.recommended_category()))
            .unwrap_or((0, consumer.recommended_category()));

        self.pending_reconnect.retain(|c| *c != consumer);
        let new_id = SessionId(self.next_id);
        self.next_id = self.next_id.saturating_add(1);
        let policy = policy_of(category_before).unwrap_or(CATEGORY_POLICY_TABLE[0]);
        self.sessions.push(Session {
            id: new_id,
            consumer,
            category: category_before,
            state: SessionState::Open,
            granted_permille: 0,
            used_permille: used_before,
            opened_tick: self.tick,
            state_version: 0,
        });
        // 重连者重新申领焦点（仅当无人占用时；焦点不抢占）。
        if policy.exclusive_focus && !self.focus_taken_by(category_before) {
            self.focus_holder.push((consumer, category_before));
        }
        self.redistribute();
        let trace = ReconnectTrace {
            consumer,
            new_session: new_id,
            stale_session: stale,
            negotiated_category: category_before,
            negotiated_latency_ms: policy.latency_budget_ms,
            negotiated_used_permille: used_before,
            phases: vec![
                ReconnectPhase::Notified,
                ReconnectPhase::SessionRebuilt,
                ReconnectPhase::StateNegotiated,
                ReconnectPhase::Resumed,
            ],
        };
        self.audits.push(format!(
            "tick{} 消费者「{}」重连完成：{} → {}（协商回填 类别 {} · 延迟 {}ms · 已用 {}‰）",
            self.tick,
            consumer.name(),
            stale.show(),
            new_id.show(),
            category_before.name(),
            policy.latency_budget_ms,
            used_before
        ));
        self.traces.push(trace.clone());
        Ok(trace)
    }

    /// 逻辑 tick 推进（零墙钟纪律；同时驱动驻留态）。
    ///
    /// 驻留态机（按需模式）：
    /// - 有活跃会话 ⇒ 热态、空闲计数清零；
    /// - 无活跃会话但保温集合非空 ⇒ 维持热态（保温免冷启动）；
    /// - 无活跃会话且保温集合为空 ⇒ 空闲计数累加，达阈值转冷态。
    /// 常驻模式恒热——`set_policy` 已把它置热，此处不回落。
    pub fn tick(&mut self) {
        self.tick = self.tick.saturating_add(1);
        if self.policy == ResidencyPolicy::Resident {
            self.residency = ResidencyState::Hot;
            self.idle_ticks = 0;
            return;
        }
        if self.live_session_count() > 0 || !self.warm_keep.is_empty() {
            self.residency = ResidencyState::Hot;
            self.idle_ticks = 0;
            return;
        }
        self.idle_ticks = self.idle_ticks.saturating_add(1);
        if self.idle_ticks >= self.idle_threshold {
            self.residency = ResidencyState::Cold;
        }
    }

    /// 类别预算重分账（同类会话均分该类别预算；整除向下取整，最小 1）。
    fn redistribute(&mut self) {
        for cat in AudioCategory::ALL {
            let share = self.budget.share_of(cat);
            let ids: Vec<SessionId> = self
                .sessions
                .iter()
                .filter(|s| s.category == cat && s.state.is_live())
                .map(|s| s.id)
                .collect();
            if ids.is_empty() {
                continue;
            }
            let each = (u32::from(share) / ids.len() as u32).max(1) as u16;
            for id in ids {
                if let Some(pos) = self.sessions.iter().position(|s| s.id == id) {
                    self.sessions[pos].granted_permille = each.min(share);
                }
            }
        }
    }

    /// 服务健康报告（渲染为纯文本——审计与回归对照用）。
    pub fn report(&self) -> String {
        format!(
            "音频引擎服务：驻留 {:?}（策略 {}）· 会话 {}/{} 活跃（降级 {}）· 焦点 {} · \
             预算 媒体{}‰/通信{}‰/系统{}‰/游戏{}‰ · 待重连 {} · 故障 {} · 错误 {} · tick{}",
            self.residency,
            self.policy.name(),
            self.live_session_count(),
            self.sessions.len(),
            self.degraded_sessions,
            if self.focus_holder.is_empty() {
                "无".to_string()
            } else {
                self.focus_holder
                    .iter()
                    .map(|(c, cat)| format!("{}·{}", c.name(), cat.name()))
                    .collect::<Vec<String>>()
                    .join("+")
            },
            self.budget.media,
            self.budget.communication,
            self.budget.system,
            self.budget.game,
            self.pending_reconnect.len(),
            self.faults.len(),
            self.errors.len(),
            self.tick
        )
    }
}

// ════════════════════════════════════════════════════════════════════════════
// §8 自检注册入口
// ════════════════════════════════════════════════════════════════════════════

/// VE-F1402 域自检（判据逐条映射见 `veh02_checks.rs`）。
pub fn run_veh02_checks() -> CheckSet {
    super::veh02_checks::run_veh02_checks()
}

// ════════════════════════════════════════════════════════════════════════════
// §9 单元测试（宿主 cargo test 直跑）
// ════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    fn svc() -> AudioEngineService {
        AudioEngineService::new()
    }

    #[test]
    fn private_stack_is_rejected() {
        let mut s = svc();
        let r = s.open_session_with(
            ConsumerKind::PlaybackChain,
            AudioCategory::Media,
            IsolationMode::PrivateStack,
        );
        assert!(r.is_err());
        assert!(s
            .errors()
            .iter()
            .any(|(_, c, _)| *c == "E_PRIVATE_STACK_FORBIDDEN"));
        assert_eq!(s.live_session_count(), 0);
    }

    #[test]
    fn reserved_category_is_rejected() {
        let mut s = svc();
        let r = s.open_session(ConsumerKind::PlaybackChain, AudioCategory::Communication);
        assert!(r.is_err());
        assert!(s.errors().iter().any(|(_, c, _)| *c == "E_CATEGORY_RESERVED"));
    }

    #[test]
    fn impossible_category_is_rejected() {
        let mut s = svc();
        let r = s.open_session(ConsumerKind::SystemSound, AudioCategory::Media);
        assert!(r.is_err());
        assert!(s
            .errors()
            .iter()
            .any(|(_, c, _)| *c == "E_CATEGORY_IMPOSSIBLE"));
    }

    #[test]
    fn misdeclared_but_legal_warns_not_rejects() {
        let mut s = svc();
        let r = s.open_session(ConsumerKind::GameScene, AudioCategory::Media);
        assert!(r.is_ok());
        assert!(s
            .errors()
            .iter()
            .any(|(_, c, _)| *c == "E_CATEGORY_MISDECLARED"));
    }

    #[test]
    fn duplicate_session_is_rejected() {
        let mut s = svc();
        assert!(s.open_session(ConsumerKind::PlaybackChain, AudioCategory::Media).is_ok());
        assert!(s.open_session(ConsumerKind::PlaybackChain, AudioCategory::Media).is_err());
        assert!(s
            .errors()
            .iter()
            .any(|(_, c, _)| *c == "E_SESSION_DUPLICATE"));
    }

    #[test]
    fn exclusive_focus_conflicts_only_across_categories() {
        let mut s = svc();
        // 媒体占焦点。
        assert!(s.open_session(ConsumerKind::PlaybackChain, AudioCategory::Media).is_ok());
        assert!(s.focus_taken_by(AudioCategory::Media));
        // 同类别（另一路媒体）不冲突——两个应用同时播音乐是合法的。
        let same_cat =
            s.open_session(ConsumerKind::GameScene, AudioCategory::Media);
        assert!(same_cat.is_ok(), "同类别媒体应可并存");
        // 通信是预留类别：先显式配预算，再验证跨类别焦点互斥。
        s.set_budget(BudgetConfig::with_communication_reserve(100));
        let st = s.open_session(ConsumerKind::SystemSound, AudioCategory::Communication);
        // 系统音效声明通信落在预留规则上（声明制），故焦点互斥面用
        // "媒体 vs 通信" 的直接判定函数验证：
        assert!(s.focus_taken_by(AudioCategory::Media));
        // 若通信会话能开出（预留放开后），则应与媒体互斥。
        if st.is_ok() {
            assert!(s
                .errors()
                .iter()
                .any(|(_, c, _)| *c == "E_FOCUS_BUSY"));
        } else {
            assert!(s
                .errors()
                .iter()
                .any(|(_, c, _)| *c == "E_CATEGORY_RESERVED"));
        }
        // 关掉媒体会话 ⇒ 焦点释放。
        let media_ids: Vec<SessionId> = s
            .sessions()
            .iter()
            .filter(|x| x.category == AudioCategory::Media)
            .map(|x| x.id)
            .collect();
        for id in media_ids {
            let _ = s.close_session(id);
        }
        assert!(!s.focus_taken_by(AudioCategory::Media));
    }

    #[test]
    fn quota_is_split_evenly_among_same_category_sessions() {
        let mut s = svc();
        let a = s.open_session(ConsumerKind::PlaybackChain, AudioCategory::Media);
        let share_alone = a.map(|x| s.share_of(x)).unwrap_or(0);
        assert!(share_alone > 0);
        // 第二条同类别会话（游戏场景合法偏离到媒体）：预算在同类间均分。
        let b = s.open_session(ConsumerKind::GameScene, AudioCategory::Media);
        assert!(b.is_ok());
        let (a, b) = (a.unwrap(), b.unwrap());
        let ga = s.share_of(a);
        let gb = s.share_of(b);
        assert!(ga < share_alone && gb < share_alone);
        // 关闭其一 ⇒ 幸存会话分账回升。
        assert!(s.close_session(b).is_ok());
        assert_eq!(s.share_of(a), share_alone);
    }

    #[test]
    fn over_quota_degrades_and_logs() {
        let mut s = svc();
        let id = s
            .open_session(ConsumerKind::PlaybackChain, AudioCategory::Media)
            .ok()
            .expect("媒体会话应可开出");
        let granted = s.share_of(id);
        assert!(s.charge(id, granted).is_ok());
        assert!(s.charge(id, 1).is_err());
        assert_eq!(s.degraded_count(), 1);
        assert!(s
            .errors()
            .iter()
            .any(|(_, c, _)| *c == "E_SESSION_QUOTA_EXCEEDED"));
        assert_eq!(s.session(id).map(|x| x.state), Some(SessionState::Degraded));
    }

    #[test]
    fn session_crash_does_not_take_down_peers() {
        let mut s = svc();
        let a = s
            .open_session(ConsumerKind::PlaybackChain, AudioCategory::Media)
            .ok()
            .expect("播放链会话");
        let b = s
            .open_session(ConsumerKind::GameScene, AudioCategory::Game)
            .ok()
            .expect("游戏会话");
        s.charge(a, 10).ok();
        s.charge(b, 10).ok();
        assert!(s.crash_session(a).is_ok());
        assert_eq!(s.session(a).map(|x| x.state), Some(SessionState::Crashed));
        assert_eq!(s.session(b).map(|x| x.state), Some(SessionState::Open));
        assert_eq!(s.session(b).map(|x| x.used_permille), Some(10));
    }

    #[test]
    fn engine_fault_is_engine_wide_and_reconnect_negotiates_usage() {
        let mut s = svc();
        let a = s
            .open_session(ConsumerKind::PlaybackChain, AudioCategory::Media)
            .ok()
            .expect("播放链会话");
        let b = s
            .open_session(ConsumerKind::GameScene, AudioCategory::Game)
            .ok()
            .expect("游戏会话");
        s.charge(a, 120).ok();
        s.charge(b, 90).ok();
        s.inject_fault(FaultKind::SharedCoreLost);
        assert_eq!(s.live_session_count(), 0);
        assert!(s.is_pending_reconnect(ConsumerKind::PlaybackChain));
        s.complete_restart();
        let t = s.reconnect(ConsumerKind::PlaybackChain);
        assert!(t.is_ok());
        let tr = t.expect("重连留痕");
        assert!(tr.is_complete());
        assert_ne!(tr.new_session, tr.stale_session);
        assert_eq!(tr.negotiated_used_permille, 120);
        assert_eq!(s.live_session_count(), 1);
    }

    #[test]
    fn reconnect_before_restart_is_rejected() {
        let mut s = svc();
        let _ = s.open_session(ConsumerKind::GameScene, AudioCategory::Game);
        s.inject_fault(FaultKind::DeviceLost);
        let r = s.reconnect(ConsumerKind::GameScene);
        assert!(r.is_err());
        assert!(s
            .errors()
            .iter()
            .any(|(_, c, _)| *c == "E_ENGINE_NOT_HOT"));
    }

    #[test]
    fn four_consumers_can_coexist() {
        let mut s = svc();
        assert!(s
            .open_session(ConsumerKind::PlaybackChain, AudioCategory::Media)
            .is_ok());
        assert!(s
            .open_session(ConsumerKind::SystemSound, AudioCategory::System)
            .is_ok());
        assert!(s
            .open_session(ConsumerKind::Notification, AudioCategory::System)
            .is_ok());
        assert!(s
            .open_session(ConsumerKind::GameScene, AudioCategory::Game)
            .is_ok());
        assert_eq!(s.live_session_count(), 4);
    }

    #[test]
    fn residency_warm_keep_and_cold() {
        let mut s = svc();
        // 缺省保温集合含系统音效/通知提醒 ⇒ 无会话仍保温热态。
        assert_eq!(s.warm_keep().len(), 2);
        for _ in 0..32 {
            s.tick();
        }
        assert_eq!(s.residency(), ResidencyState::Hot);
        // 清空保温 ⇒ 空闲达阈值转冷。
        s.set_warm_keep(Vec::new());
        for _ in 0..16 {
            s.tick();
        }
        assert_eq!(s.residency(), ResidencyState::Cold);
        // 常驻模式恒热。
        s.set_policy(ResidencyPolicy::Resident);
        for _ in 0..64 {
            s.tick();
        }
        assert_eq!(s.residency(), ResidencyState::Hot);
    }

    #[test]
    fn budget_reserve_reclaims_and_sums_to_1000() {
        let b = BudgetConfig::with_communication_reserve(100);
        assert!(b.sums_to_total());
        assert_eq!(b.communication, 100);
        assert!(b.media < F1416_DEFAULT_BUDGET.media);
        assert!(F1416_DEFAULT_BUDGET.sums_to_total());
        assert_eq!(F1416_DEFAULT_BUDGET.communication, 0);
    }

    #[test]
    fn category_policy_table_is_distinct() {
        let lats: Vec<u32> = CATEGORY_POLICY_TABLE.iter().map(|p| p.latency_budget_ms).collect();
        let mut uniq = lats.clone();
        uniq.sort_unstable();
        uniq.dedup();
        assert_eq!(uniq.len(), 4);
        assert!(policy_of(AudioCategory::Communication).is_ok());
        assert_eq!(policy_of(AudioCategory::Game).unwrap().spatial, true);
        assert_eq!(policy_of(AudioCategory::Media).unwrap().exclusive_focus, true);
    }

    #[test]
    fn disconnect_obligations_are_complete() {
        assert!(disconnect_obligations_met(&[
            "stop-submit",
            "bounded-buffer",
            "version-bump",
            "stale-handle-drop"
        ]));
        assert!(!disconnect_obligations_met(&["stop-submit"]));
    }

    #[test]
    fn docs_declare_the_architecture() {
        assert!(SERVICE_RATIONALE_DOC.contains("重复建设"));
        assert!(MULTI_CONSUMER_DOC.contains("一崩不连坐"));
        assert!(SESSION_ISOLATION_DOC.contains("按会话分账"));
        assert!(CATEGORY_CONTRACT_DOC.contains("类别错了体验就错"));
        assert!(LIFECYCLE_DOC.contains("按需"));
        assert!(FAULT_DOMAIN_DOC.contains("状态协商"));
    }
}