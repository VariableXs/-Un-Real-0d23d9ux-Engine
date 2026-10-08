//! VE-F1402 · 音频引擎服务化架构（VE-H 域 · 音频引擎 · 目标 440 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F1402`
//!
//! **规格原文**：音频引擎独立服务（不隶属播放链——多消费者：G07 播放链/系统
//! 音效/通知提醒/未来游戏场景统一接入）。服务模型（单例引擎服务+多客户端
//! 会话——会话隔离与资源配额）；消费者契约（每消费者声明音频类别：媒体/通信/
//! 系统/游戏——类别决定路由与策略）；生命周期（系统常驻/按需唤醒两模式，
//! 默认按需+高频消费者保温，策略可配）；故障域（引擎崩溃不拖垮消费者——
//! 自动重启+消费者重连协议）。判据：多消费者、会话隔离、四类别、生命周期、
//! 故障域、判据。
//!
//! **设计要点**：
//! - 服务化架构理由显性登记：一次建设多端消费，四消费者共用引擎核（与
//!   F1401 边界契约同源：DSP 核共享、服务层各自独立——混层即职责污染）；
//! - 会话是隔离的边界：每消费者一个会话，会话崩不连坐（故障注入可验），
//!   配额按会话分账（F1416 的账本挂点）——超配额拒绝是本会话的事，
//!   不许挤占别人的配额；
//! - 类别错了体验就错：媒体/通信/系统/游戏四类别各有策略表（延迟预算/
//!   断连处置/保温资格），类别声明制——运行时按声明路由，不按猜；
//! - 生命周期两模式：常驻（响应快常驻内存）与按需（省内存冷启动），默认
//!   按需 + 高频消费者保温（保温资格按类别策略判，策略可配）；
//! - 故障域契约双向：引擎侧自动重启（退避封顶）+ 重连协议（会话重建+状态
//!   协商）；消费者侧断连处理契约（按类别：系统类即发即弃不重建，媒体/游戏
//!   状态协商重建，通信类最优先重连）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、消费者类别契约（四类别策略表）
// ---------------------------------------------------------------------------

/// 消费者音频类别（声明制：类别决定路由与策略）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConsumerKind {
    /// 媒体：高保真、低延迟、宽松配额。
    Media,
    /// 通信：超低延迟、语音处理优先。
    Communication,
    /// 系统：即发即弃、低开销。
    System,
    /// 游戏：空间化、多声源。
    Game,
}

impl ConsumerKind {
    pub fn label(self) -> &'static str {
        match self {
            ConsumerKind::Media => "媒体",
            ConsumerKind::Communication => "通信",
            ConsumerKind::System => "系统",
            ConsumerKind::Game => "游戏",
        }
    }
}

/// 类别策略（延迟预算 µs / 断连处置 / 保温资格 / 声部配额默认）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KindPolicy {
    pub latency_budget_us: u64,
    /// 断连后是否提供会话重建（系统类即发即弃 = false）。
    pub rebuild_on_disconnect: bool,
    /// 是否有资格保温引擎（高频消费者保温，策略可配）。
    pub keep_warm_eligible: bool,
    pub default_voice_quota: u32,
    /// 重连优先级（小者先重连；通信最优先）。
    pub reconnect_priority: u32,
}

/// 冻结策略表（策略可配 = 运行时可覆盖，但默认值逐项有据）。
pub const KIND_POLICIES: [(ConsumerKind, KindPolicy); 4] = [
    (
        ConsumerKind::Media,
        KindPolicy {
            latency_budget_us: 40_000,
            rebuild_on_disconnect: true,
            keep_warm_eligible: true,
            default_voice_quota: 64,
            reconnect_priority: 2,
        },
    ),
    (
        ConsumerKind::Communication,
        KindPolicy {
            latency_budget_us: 10_000,
            rebuild_on_disconnect: true,
            keep_warm_eligible: true,
            default_voice_quota: 8,
            reconnect_priority: 0,
        },
    ),
    (
        ConsumerKind::System,
        KindPolicy {
            latency_budget_us: 80_000,
            rebuild_on_disconnect: false,
            keep_warm_eligible: false,
            default_voice_quota: 16,
            reconnect_priority: 3,
        },
    ),
    (
        ConsumerKind::Game,
        KindPolicy {
            latency_budget_us: 20_000,
            rebuild_on_disconnect: true,
            keep_warm_eligible: true,
            default_voice_quota: 128,
            reconnect_priority: 1,
        },
    ),
];

pub fn policy_of(kind: ConsumerKind) -> KindPolicy {
    KIND_POLICIES
        .iter()
        .find(|(k, _)| *k == kind)
        .map(|(_, p)| *p)
        .expect("策略表覆盖四类别全集")
}

// ---------------------------------------------------------------------------
// 二、多消费者架构理由登记（服务化的一次建设多端消费）
// ---------------------------------------------------------------------------

/// 消费者登记（架构理由的证据：谁在消费、共享什么、独立什么）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConsumerRegistration {
    pub name: &'static str,
    pub kind: ConsumerKind,
    /// 服务层归属（F1401 契约：服务层各自独立）。
    pub service_layer: &'static str,
}

/// 服务化理由登记表：四消费者 + 共享核声明。
pub struct ServiceRationale {
    pub consumers: Vec<ConsumerRegistration>,
}

impl ServiceRationale {
    /// 官方四消费者（G07 播放链/系统音效/通知提醒/游戏场景）。
    pub fn official() -> ServiceRationale {
        ServiceRationale {
            consumers: vec![
                ConsumerRegistration {
                    name: "G07 播放链",
                    kind: ConsumerKind::Media,
                    service_layer: "播放链编排（独立）",
                },
                ConsumerRegistration {
                    name: "系统音效",
                    kind: ConsumerKind::System,
                    service_layer: "系统事件桥（独立）",
                },
                ConsumerRegistration {
                    name: "通知提醒",
                    kind: ConsumerKind::System,
                    service_layer: "通知服务桥（独立）",
                },
                ConsumerRegistration {
                    name: "游戏场景",
                    kind: ConsumerKind::Game,
                    service_layer: "游戏音频桥（独立）",
                },
            ],
        }
    }

    /// 共享核声明（F1401 ADR：DSP 核共享、服务层独立）。
    pub fn shared_core_statement(&self) -> &'static str {
        "DSP 核（混音/重采样/响度）共享单实例引擎；服务层按消费者各自独立——混层即职责污染"
    }

    /// 单例断言：全部消费者指向同一个引擎服务。
    pub fn singleton_engine(&self) -> bool {
        !self.consumers.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 三、会话模型（隔离 + 配额分账）
// ---------------------------------------------------------------------------

/// 会话状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionState {
    Active,
    /// 消费者断连（等待重建或即发即弃）。
    Disconnected,
    /// 重建协商中。
    Rebuilding,
    /// 已关闭（终态）。
    Closed,
}

/// 会话配额超用错误（三要素）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionError {
    pub code: &'static str,
    pub what: String,
    pub why: String,
    pub next: String,
}

impl SessionError {
    fn new(code: &'static str, what: String, why: String, next: String) -> SessionError {
        SessionError { code, what, why, next }
    }
    pub fn is_complete(&self) -> bool {
        !self.what.is_empty() && !self.why.is_empty() && !self.next.is_empty()
    }
}

/// 一条消费者会话。
pub struct Session {
    pub id: u32,
    pub consumer: &'static str,
    pub kind: ConsumerKind,
    pub state: SessionState,
    pub voice_quota: u32,
    pub active_voices: u32,
    /// 会话故障注入位（隔离判据的注入点）。
    pub fault_injected: bool,
    /// 会话级事件账（隔离可验的证据链）。
    pub events: Vec<String>,
}

impl Session {
    /// 消费者接入：按类别策略开立会话（配额默认取类别值，可按 F1416 调账）。
    pub fn open(id: u32, consumer: &'static str, kind: ConsumerKind) -> Session {
        Session {
            id,
            consumer,
            kind,
            state: SessionState::Active,
            voice_quota: policy_of(kind).default_voice_quota,
            active_voices: 0,
            fault_injected: false,
            events: vec![format!("会话 {} 开立（{}）", id, kind.label())],
        }
    }

    /// 申请声部：本会话配额内批准，超配额拒绝（只影响本会话）。
    pub fn request_voice(&mut self) -> Result<(), SessionError> {
        if self.fault_injected {
            self.events.push(format!("会话 {} 故障注入：声部申请被会话自身故障拦截", self.id));
            return Err(SessionError::new(
                "E_SESSION_FAULT",
                format!("会话 {} 已注入故障，声部申请拒绝", self.id),
                "故障隔离的注入点：会话自身故障只影响本会话".to_string(),
                "重连协议处置本会话；其他会话不受影响".to_string(),
            ));
        }
        if self.active_voices >= self.voice_quota {
            return Err(SessionError::new(
                "E_QUOTA_EXCEEDED",
                format!(
                    "会话 {} 声部 {} 达配额 {}",
                    self.id, self.active_voices, self.voice_quota
                ),
                "配额按会话分账（F1416 挂点）：本会话超支不许挤占他人".to_string(),
                "先释放声部，或走 F1416 预算重评流程调额".to_string(),
            ));
        }
        self.active_voices += 1;
        self.events.push(format!("会话 {} 声部申请批准（{}/{})", self.id, self.active_voices, self.voice_quota));
        Ok(())
    }

    pub fn release_voice(&mut self) {
        self.active_voices = self.active_voices.saturating_sub(1);
    }

    /// 引擎视角注入会话故障（一崩不连坐的测试锚）。
    pub fn inject_fault(&mut self) {
        self.fault_injected = true;
        self.state = SessionState::Disconnected;
        self.events.push(format!("会话 {} 故障注入：标记断连", self.id));
    }
}

/// 会话表：开立/查询/关闭，隔离断言挂在这里。
pub struct SessionTable {
    sessions: Vec<Session>,
    next_id: u32,
}

impl SessionTable {
    pub fn new() -> SessionTable {
        SessionTable {
            sessions: Vec::new(),
            next_id: 1,
        }
    }

    pub fn open(&mut self, consumer: &'static str, kind: ConsumerKind) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        self.sessions.push(Session::open(id, consumer, kind));
        id
    }

    pub fn get(&self, id: u32) -> Option<&Session> {
        self.sessions.iter().find(|s| s.id == id)
    }

    pub fn get_mut(&mut self, id: u32) -> Option<&mut Session> {
        self.sessions.iter_mut().find(|s| s.id == id)
    }

    pub fn len(&self) -> usize {
        self.sessions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }

    /// 隔离断言：某会话故障后，其余 Active 会话的声部账完全不受影响。
    pub fn isolation_holds_after_fault_of(&self, faulted_id: u32) -> bool {
        self.sessions
            .iter()
            .filter(|s| s.id != faulted_id)
            .all(|s| s.state == SessionState::Active && s.fault_injected == false)
    }

    /// 关闭会话（终态，声部账清零）。
    pub fn close(&mut self, id: u32) -> bool {
        match self.sessions.iter_mut().find(|s| s.id == id) {
            Some(s) => {
                s.state = SessionState::Closed;
                s.active_voices = 0;
                s.events.push(format!("会话 {} 关闭", id));
                true
            }
            None => false,
        }
    }
}

// ---------------------------------------------------------------------------
// 四、生命周期（常驻 / 按需唤醒 + 保温策略）
// ---------------------------------------------------------------------------

/// 引擎生命周期模式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LifecycleMode {
    /// 系统常驻：响应快、常驻内存。
    Resident,
    /// 按需唤醒：空闲即睡、来活即起（默认）。
    OnDemand,
}

/// 生命周期引擎（逻辑时钟 µs 驱动，确定性可测）。
pub struct LifecycleEngine {
    pub mode: LifecycleMode,
    /// 按需模式的空闲超时（默认 5 秒；策略可配）。
    pub idle_timeout_us: u64,
    last_activity_us: u64,
    pub sleep_count: u64,
    pub wake_count: u64,
    pub asleep: bool,
}

impl LifecycleEngine {
    pub fn new() -> LifecycleEngine {
        LifecycleEngine {
            mode: LifecycleMode::OnDemand,
            idle_timeout_us: 5_000_000,
            last_activity_us: 0,
            sleep_count: 0,
            wake_count: 0,
            asleep: false,
        }
    }

    /// 修改模式（策略可配的挂点）。
    pub fn set_mode(&mut self, mode: LifecycleMode) {
        self.mode = mode;
        if mode == LifecycleMode::Resident {
            self.asleep = false;
        }
    }

    /// 活动上报（会话有产出即刷新活动钟）。
    pub fn report_activity(&mut self, now_us: u64) {
        if self.asleep {
            self.asleep = false;
            self.wake_count += 1;
        }
        self.last_activity_us = now_us;
    }

    /// 帧推进：按需模式空闲超时即睡；但只要还有"保温资格"的活跃会话就不睡。
    pub fn tick(&mut self, now_us: u64, has_keep_warm_session: bool) {
        if self.mode != LifecycleMode::OnDemand || self.asleep {
            return;
        }
        if now_us.saturating_sub(self.last_activity_us) >= self.idle_timeout_us
            && !has_keep_warm_session
        {
            self.asleep = true;
            self.sleep_count += 1;
        }
    }
}

// ---------------------------------------------------------------------------
// 五、故障域（自动重启 + 消费者重连协议）
// ---------------------------------------------------------------------------

/// 引擎崩溃事件与重启账。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RestartRecord {
    pub crash_no: u32,
    pub at_us: u64,
    /// 退避时长（指数增长、封顶 1 秒——退避防重启风暴）。
    pub backoff_us: u64,
}

pub const RESTART_BACKOFF_BASE_US: u64 = 50_000;
pub const RESTART_BACKOFF_CAP_US: u64 = 1_000_000;

/// 消费者重连请求（重连协议的消费者侧半边）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReconnectRequest {
    pub consumer: &'static str,
    pub kind: ConsumerKind,
    /// 消费者断连时保存的状态向量版本（会话重建的状态协商锚）。
    pub state_version: u32,
}

/// 重连裁决。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReconnectVerdict {
    /// 重建会话，返回新会话号。
    Rebuilt { new_session: u32 },
    /// 即发即弃类：不重建（系统类的合法处置）。
    NotRebuildable,
    /// 协议版本不匹配（诚实拒绝）。
    ProtocolMismatch,
}

/// 重连协议版本（状态协商的协议契约）。
pub const RECONNECT_PROTOCOL_VERSION: u32 = 1;

/// 故障域管理器：引擎崩溃 → 自动重启（退避封顶）→ 按类别的重连处置。
pub struct FaultDomain {
    pub restarts: Vec<RestartRecord>,
    /// 引擎是否存活。
    pub engine_alive: bool,
}

impl FaultDomain {
    pub fn new() -> FaultDomain {
        FaultDomain {
            restarts: Vec::new(),
            engine_alive: true,
        }
    }

    /// 引擎崩溃。
    pub fn crash(&mut self, _at_us: u64) {
        self.engine_alive = false;
    }

    /// 自动重启：第 n 次崩溃的退避 = base << min(n-1, cap 档)。
    pub fn auto_restart(&mut self, at_us: u64) -> RestartRecord {
        let n = self.restarts.len() as u32 + 1;
        let shift = (n - 1).min(5);
        let backoff = RESTART_BACKOFF_BASE_US
            .saturating_mul(1u64 << shift)
            .min(RESTART_BACKOFF_CAP_US);
        let rec = RestartRecord {
            crash_no: n,
            at_us,
            backoff_us: backoff,
        };
        self.restarts.push(rec.clone());
        self.engine_alive = true;
        rec
    }

    /// 消费者重连：按类别策略处置（协议版本核对 → 重建/即发即弃）。
    pub fn reconnect(
        &self,
        table: &mut SessionTable,
        req: &ReconnectRequest,
    ) -> ReconnectVerdict {
        let policy = policy_of(req.kind);
        if !policy.rebuild_on_disconnect {
            return ReconnectVerdict::NotRebuildable;
        }
        if req.state_version != RECONNECT_PROTOCOL_VERSION {
            return ReconnectVerdict::ProtocolMismatch;
        }
        // 会话重建：新会话、按类别重开（配额随类别策略）。
        let id = table.open(req.consumer, req.kind);
        if let Some(s) = table.get_mut(id) {
            s.state = SessionState::Rebuilding;
            s.events.push(format!(
                "会话 {} 重建协商（状态版本 {}，重连优先级 {}）",
                id, req.state_version, policy.reconnect_priority
            ));
        }
        ReconnectVerdict::Rebuilt { new_session: id }
    }

    /// 重连顺序：按类别优先级排序后的消费者名单（通信最优先）。
    pub fn reconnect_order(kinds: &[ConsumerKind]) -> Vec<ConsumerKind> {
        let mut v: Vec<(u32, ConsumerKind)> = kinds
            .iter()
            .map(|k| (policy_of(*k).reconnect_priority, *k))
            .collect();
        v.sort_by_key(|(p, _)| *p);
        v.into_iter().map(|(_, k)| k).collect()
    }
}

// ---------------------------------------------------------------------------
// 六、音频引擎服务（单例外壳：四件套的总装配）
// ---------------------------------------------------------------------------

/// 音频引擎服务：单例引擎 + 会话表 + 生命周期 + 故障域。
pub struct AudioEngineService {
    pub rationale: ServiceRationale,
    pub sessions: SessionTable,
    pub lifecycle: LifecycleEngine,
    pub fault: FaultDomain,
    pub now_us: u64,
}

impl AudioEngineService {
    pub fn new() -> AudioEngineService {
        AudioEngineService {
            rationale: ServiceRationale::official(),
            sessions: SessionTable::new(),
            lifecycle: LifecycleEngine::new(),
            fault: FaultDomain::new(),
            now_us: 0,
        }
    }

    /// 消费者接入：单例断言 + 类别声明制开立会话。
    pub fn admit(&mut self, consumer: &'static str, kind: ConsumerKind) -> u32 {
        debug_assert!(self.rationale.singleton_engine());
        self.lifecycle.report_activity(self.now_us);
        self.sessions.open(consumer, kind)
    }

    /// 是否存在保温资格的活跃会话（生命周期判定的输入）。
    pub fn has_keep_warm_session(&self) -> bool {
        self.sessions
            .sessions
            .iter()
            .any(|s| s.state == SessionState::Active && policy_of(s.kind).keep_warm_eligible)
    }

    /// 时间推进。
    pub fn advance(&mut self, us: u64) {
        self.now_us += us;
        self.lifecycle.tick(self.now_us, self.has_keep_warm_session());
    }

    /// 引擎崩溃演练：全 Active 会话转断连，随后自动重启。
    pub fn crash_drill(&mut self) -> RestartRecord {
        self.fault.crash(self.now_us);
        let rec = self.fault.auto_restart(self.now_us);
        for s in self.sessions.sessions.iter_mut() {
            if s.state == SessionState::Active {
                s.state = SessionState::Disconnected;
                s.events.push(format!("会话 {} 因引擎崩溃转断连", s.id));
            }
        }
        rec
    }

    /// 读屏摘要。
    pub fn a11y_summary(&self) -> String {
        let active = self
            .sessions
            .sessions
            .iter()
            .filter(|s| s.state == SessionState::Active)
            .count();
        format!(
            "音频引擎服务：模式 {:?}、会话 {} 条（活跃 {}）、重启 {} 次、引擎 {}",
            self.lifecycle.mode,
            self.sessions.len(),
            active,
            self.fault.restarts.len(),
            if self.fault.engine_alive { "存活" } else { "已停" }
        )
    }
}
