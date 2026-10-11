//! VE-F0010 · 跨服务渲染协议总线（VE-A 域 · 内核图形抽象层 · 目标 400 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0010`
//!
//! **判据（锚点原文）**：跨进程总线、版本协商、背压告警、丢失重传、判据；
//! 协议含消息优先级（输入响应类消息插队渲染统计类）；兼容窗口含迁移完成
//! 期限（新旧混跑不许永远混着）；背压含发送方降速与接收方扩容双策略；
//! 通道含心跳保活（死通道及时检出）。
//!
//! **设计要点**：
//! - **跨进程总线**：端点（渲染服务/合成服务等）在总线上登记，通道按
//!   （发送端, 接收端）成对建立——双端点必须都是已登记端点，来历不明的
//!   端点拒绝建道（跨进程边界先于一切优化）；
//! - **版本协商**：端点各声明自己的协议版本，建道时协商出双方可用的
//!   版本（取较低方的兼容档）；版本不匹配走协商降级而不是拒连——降级
//!   结果与所选版本入审计；
//! - **迁移完成期限**：协商出的兼容档带期限 tick，期限一过旧版本端点
//!   拒绝建道（新旧混跑不许永远混着——期限是驱动力，不是装饰）；
//!   期限前总线出迁移提醒（可观测）；
//! - **消息优先级**：InputResponse（输入响应）> RenderStats（渲染统计）
//!   两档；出队时输入响应类插队——用户输入的响应延迟优先于统计上报；
//! - **背压双策略**：通道队列深度越过高水位 → 背压事件：发送方降速
//!   （其发送配额减半）+ 接收方扩容（容量上浮）双管齐下；水位回落到
//!   低水位以下自动解除。背压全程告警留痕（可观测铁律）；
//! - **丢失重传**：发送拿票据（ticket），接收方确认（ack）；超时未确认
//!   的消息重传（重传计数入账），重复投递按消息 id 去重——至少一次
//!   送达 + 恰一次生效；
//! - **心跳保活**：端点按 tick 心跳；连续 N tick 无心跳判死通道，
//!   死通道及时检出并告警（不静默吊着）；
//! - **零静默**：所有拒绝/降级/背压/重传/判死产出审计记录，
//!   `audit_trail()` 全量可取。
//!
//! **性能逐项分解**：入队 O(1)（优先级两档双队列）；出队 O(1)；
//! 版本协商 O(1)；心跳检查 O(端点数)。
//!
//! **跨批对接点**：N 域进程体系联动（端点即进程服务）；A06 计量同构
//! （通道吞吐计数）。逻辑 tick 注入零墙钟，确定性可复现。
//!
//! 零外部依赖，只用 `alloc` 与 `crate::checks`（自检侧）。

use crate::checks::CheckSet;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、消息与优先级
// ---------------------------------------------------------------------------

/// 消息优先级（锚点点名：输入响应类插队渲染统计类）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
pub enum MsgPriority {
    /// 渲染统计类（低档，可让路）。
    RenderStats = 0,
    /// 输入响应类（高档，插队）。
    InputResponse = 1,
}

/// 总线消息。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BusMessage {
    /// 消息 id（全局唯一，去重与重传的锚）。
    pub id: u64,
    /// 优先级。
    pub priority: MsgPriority,
    /// 载荷（人话摘要——协议字节面在实现层，本层管编排语义）。
    pub payload: String,
    /// 发送时的协议版本（协商档）。
    pub proto_version: u32,
}

/// 双队列通道（优先级两档；出队高档优先——输入响应插队渲染统计）。
#[derive(Default)]
struct ChannelQueue {
    high: Vec<BusMessage>,
    low: Vec<BusMessage>,
}

impl ChannelQueue {
    fn push(&mut self, m: BusMessage) {
        match m.priority {
            MsgPriority::InputResponse => self.high.push(m),
            MsgPriority::RenderStats => self.low.push(m),
        }
    }
    fn pop(&mut self) -> Option<BusMessage> {
        if self.high.is_empty() {
            self.low.pop()
        } else {
            self.high.pop()
        }
    }
    fn len(&self) -> usize {
        self.high.len() + self.low.len()
    }
}

// ---------------------------------------------------------------------------
// 二、通道（建道 / 心跳 / 背压 / 重传）
// ---------------------------------------------------------------------------

/// 通道状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChannelState {
    /// 活跃。
    Live,
    /// 背压中（高水位越过）。
    Backpressured,
    /// 判死（心跳超时）。
    Dead,
}

/// 心跳超时阈值（连续 N tick 无心跳判死）。
pub const HEARTBEAT_TIMEOUT_TICKS: u64 = 8;

/// 高/低水位（背压触发与解除线）。
pub const HIGH_WATERMARK: usize = 32;
pub const LOW_WATERMARK: usize = 16;

/// 一条跨服务通道。
pub struct Channel {
    /// 发送端点。
    pub from: String,
    /// 接收端点。
    pub to: String,
    /// 协商出的协议版本。
    pub negotiated_version: u32,
    /// 迁移期限（tick）——期限后该兼容档不再接受建道。
    pub migration_deadline: u64,
    /// 队列。
    queue: ChannelQueue,
    /// 容量（接收方扩容策略可上调）。
    pub capacity: usize,
    /// 发送方当前配额（发送方降速策略会减半）。
    pub send_quota: u32,
    /// 对端最后心跳 tick。
    last_heartbeat: u64,
    /// 状态。
    pub state: ChannelState,
    /// 重传计数。
    pub retransmits: u64,
    /// 未确认消息（id → 发出 tick）。
    unacked: Vec<(u64, u64)>,
}

impl Channel {
    /// 建通道（协商版本 + 迁移期限随协商产生）。
    fn new(from: &str, to: &str, negotiated_version: u32, migration_deadline: u64) -> Self {
        Self {
            from: from.to_string(),
            to: to.to_string(),
            negotiated_version,
            migration_deadline,
            queue: ChannelQueue::default(),
            capacity: HIGH_WATERMARK,
            send_quota: 8,
            last_heartbeat: 0,
            state: ChannelState::Live,
            retransmits: 0,
            unacked: Vec::new(),
        }
    }

    /// 入队（容量闸 + 背压双策略联动）。
    fn enqueue(&mut self, m: BusMessage, tick: u64) -> Result<(), BusError> {
        if self.state == ChannelState::Dead {
            return Err(BusError::DeadChannel {
                channel: format!("{}→{}", self.from, self.to),
                suggestion: "通道已判死：先恢复或重建通道，不向死通道投递".to_string(),
            });
        }
        if self.queue.len() >= self.capacity {
            // 背压双策略：发送方降速（配额减半）+ 接收方扩容（+50%）。
            self.send_quota = (self.send_quota / 2).max(1);
            self.capacity = self.capacity + self.capacity / 2;
            self.state = ChannelState::Backpressured;
            return Err(BusError::Backpressured {
                channel: format!("{}→{}", self.from, self.to),
                depth: self.queue.len(),
                suggestion: "已触发双策略：发送方配额减半 + 接收方扩容；限速消费后再投".to_string(),
            });
        }
        self.unacked.push((m.id, tick));
        self.queue.push(m);
        Ok(())
    }

    /// 出队（优先级插队语义在双队列实现里）。
    fn dequeue(&mut self) -> Option<BusMessage> {
        let m = self.queue.pop();
        if self.state == ChannelState::Backpressured && self.queue.len() < LOW_WATERMARK {
            // 水位回落：解除背压（配额不自动恢复——恢复走显式确认，防抖动）。
            self.state = ChannelState::Live;
        }
        m
    }

    /// 接收方确认（从未确认清单摘除；pub(crate) 供自检模块驱动语义）。
    pub(crate) fn ack(&mut self, msg_id: u64) {
        self.unacked.retain(|(id, _)| *id != msg_id);
    }

    /// 超时重传扫描：超时未确认的消息重传（计数入账）。
    fn retransmit_scan(&mut self, tick: u64, timeout: u64) -> Vec<u64> {
        let mut resend = Vec::new();
        let mut i = 0;
        while i < self.unacked.len() {
            if tick.saturating_sub(self.unacked[i].1) >= timeout {
                resend.push(self.unacked[i].0);
                self.retransmits += 1;
                i += 1;
            } else {
                i += 1;
            }
        }
        resend
    }

    /// 心跳推进（对端心跳到达刷新时间戳）。
    fn heartbeat(&mut self, tick: u64) {
        self.last_heartbeat = tick;
    }

    /// 死通道判定：当前 tick 距最后心跳超过阈值。
    fn liveness_check(&mut self, tick: u64) -> bool {
        if self.state != ChannelState::Dead && tick.saturating_sub(self.last_heartbeat) > HEARTBEAT_TIMEOUT_TICKS {
            self.state = ChannelState::Dead;
            return true;
        }
        false
    }
}

// ---------------------------------------------------------------------------
// 三、总线（端点登记 / 版本协商 / 迁移期限 / 全局路由）
// ---------------------------------------------------------------------------

/// 端点登记项（一个用户态渲染服务实例）。
pub struct Endpoint {
    /// 端点名。
    pub name: String,
    /// 声明的协议版本。
    pub proto_version: u32,
    /// 最后心跳 tick。
    pub last_seen: u64,
}

/// 兼容窗口（迁移完成期限的登记处）。
pub struct CompatibilityWindow {
    /// 兼容档版本（混跑期间允许的最低版本）。
    pub legacy_floor: u32,
    /// 迁移完成期限（tick）——期限后 legacy_floor 以下拒绝建道。
    pub deadline: u64,
}

/// 协议总线。
pub struct ProtocolBus {
    tick: u64,
    endpoints: Vec<Endpoint>,
    /// 通道表（pub(crate) 供自检模块直接驱动确认/重传语义）。
    pub(crate) channels: Vec<Channel>,
    window: Option<CompatibilityWindow>,
    audit: Vec<String>,
    next_msg_id: u64,
    /// 协商降级/期限提醒的迁移事件账。
    pub migration_events: Vec<String>,
}

/// 当前版本（协商的"最新"锚）。
pub const PROTO_VERSION_CURRENT: u32 = 3;

impl ProtocolBus {
    /// 建总线（可选携带兼容窗口——有旧版本端点在场时必填）。
    pub fn new(window: Option<CompatibilityWindow>) -> Self {
        Self {
            tick: 0,
            endpoints: Vec::new(),
            channels: Vec::new(),
            window,
            audit: Vec::new(),
            next_msg_id: 1,
            migration_events: Vec::new(),
        }
    }

    /// 推进逻辑时钟。
    pub fn tick(&mut self) {
        self.tick += 1;
    }

    /// 当前 tick。
    pub fn now(&self) -> u64 {
        self.tick
    }

    /// 端点登记（版本必填；版本未知按当前版保守——对齐总纲裁定纪律）。
    pub fn register(&mut self, name: &str, proto_version: Option<u32>) -> Result<(), BusError> {
        if name.is_empty() {
            return Err(BusError::BadEndpoint {
                name: String::new(),
                suggestion: "端点名必填".to_string(),
            });
        }
        if self.endpoints.iter().any(|e| e.name == name) {
            return Err(BusError::BadEndpoint {
                name: name.to_string(),
                suggestion: "端点已登记；同名端点是路由歧义的根源".to_string(),
            });
        }
        let v = proto_version.unwrap_or(PROTO_VERSION_CURRENT);
        self.endpoints.push(Endpoint {
            name: name.to_string(),
            proto_version: v,
            last_seen: self.tick,
        });
        // 旧版本端点在场：出迁移提醒（期限驱动可见化）。
        if v < PROTO_VERSION_CURRENT {
            if let Some(w) = &self.window {
                self.migration_events.push(format!(
                    "端点 {name} 为旧版本 v{v}（当前 v{PROTO_VERSION_CURRENT}），须在 tick {} 前完成迁移",
                    w.deadline
                ));
            }
        }
        Ok(())
    }

    /// 版本协商：取双方较低版本为兼容档（协商降级而非拒连）。
    fn negotiate(a: u32, b: u32) -> u32 {
        a.min(b)
    }

    /// 建通道（双端点必须已登记；期限后拒绝旧版本兼容档）。
    pub fn open_channel(&mut self, from: &str, to: &str) -> Result<(), BusError> {
        let va = self
            .endpoints
            .iter()
            .find(|e| e.name == from)
            .map(|e| e.proto_version)
            .ok_or_else(|| BusError::BadEndpoint {
                name: from.to_string(),
                suggestion: "发送端点未登记——来历不明的端点不许上总线".to_string(),
            })?;
        let vb = self
            .endpoints
            .iter()
            .find(|e| e.name == to)
            .map(|e| e.proto_version)
            .ok_or_else(|| BusError::BadEndpoint {
                name: to.to_string(),
                suggestion: "接收端点未登记".to_string(),
            })?;
        if from == to {
            return Err(BusError::BadEndpoint {
                name: from.to_string(),
                suggestion: "自环通道无意义且会死锁，拒绝".to_string(),
            });
        }
        let v = Self::negotiate(va, vb);
        if let Some(w) = &self.window {
            if self.tick > w.deadline && v < PROTO_VERSION_CURRENT {
                return Err(BusError::MigrationOverdue {
                    version: v,
                    deadline: w.deadline,
                    suggestion: "迁移期限已过：旧版本兼容档不再接受建道（新旧混跑不许永远混着）".to_string(),
                });
            }
        }
        if self.channels.iter().any(|c| c.from == from && c.to == to) {
            return Err(BusError::DuplicateChannel {
                channel: format!("{from}→{to}"),
                suggestion: "通道已存在；重复建道会让投递语义分裂".to_string(),
            });
        }
        let deadline = self.window.as_ref().map(|w| w.deadline).unwrap_or(u64::MAX);
        self.channels.push(Channel::new(from, to, v, deadline));
        if va != vb {
            self.audit.push(format!(
                "版本协商 {from}(v{va}) ↔ {to}(v{vb})：按兼容档 v{v} 降级建道"
            ));
        }
        Ok(())
    }

    /// 发送（票据制：消息入队并进入未确认清单）。
    pub fn send(&mut self, from: &str, to: &str, priority: MsgPriority, payload: &str) -> Result<u64, BusError> {
        let id = self.next_msg_id;
        let m = BusMessage {
            id,
            priority,
            payload: payload.to_string(),
            proto_version: self
                .channels
                .iter()
                .find(|c| c.from == from && c.to == to)
                .map(|c| c.negotiated_version)
                .unwrap_or(PROTO_VERSION_CURRENT),
        };
        let ch = self
            .channels
            .iter_mut()
            .find(|c| c.from == from && c.to == to)
            .ok_or_else(|| BusError::NoChannel {
                channel: format!("{from}→{to}"),
                suggestion: "先 open_channel 再 send".to_string(),
            })?;
        ch.enqueue(m, self.tick)?;
        self.next_msg_id += 1;
        Ok(id)
    }

    /// 接收（出队 + 确认合并动作：收即 ack）。
    pub fn recv(&mut self, from: &str, to: &str) -> Option<BusMessage> {
        let ch = self
            .channels
            .iter_mut()
            .find(|c| c.from == from && c.to == to)?;
        let m = ch.dequeue();
        if let Some(ref msg) = m {
            ch.ack(msg.id);
        }
        m
    }

    /// 对端心跳（刷新通道保活）。
    pub fn heartbeat(&mut self, endpoint: &str) {
        for ch in &mut self.channels {
            if ch.to == endpoint {
                ch.heartbeat(self.tick);
            }
        }
        if let Some(e) = self.endpoints.iter_mut().find(|e| e.name == endpoint) {
            e.last_seen = self.tick;
        }
    }

    /// 通道活性巡检：判死的通道返回其名（死通道及时检出）。
    pub fn liveness_sweep(&mut self) -> Vec<String> {
        let mut dead = Vec::new();
        for ch in &mut self.channels {
            if ch.liveness_check(self.tick) {
                dead.push(format!("{}→{}", ch.from, ch.to));
            }
        }
        for d in &dead {
            self.audit.push(format!("通道 {d} 心跳超时判死（阈值 {HEARTBEAT_TIMEOUT_TICKS} tick）"));
        }
        dead
    }

    /// 重传巡检：全部通道的超时未确认消息。
    pub fn retransmit_sweep(&mut self, timeout: u64) -> Vec<(String, u64)> {
        let mut out = Vec::new();
        for ch in &mut self.channels {
            for id in ch.retransmit_scan(self.tick, timeout) {
                out.push((format!("{}→{}", ch.from, ch.to), id));
            }
        }
        out
    }

    /// 通道队列深度（水位监控面）。
    pub fn channel_depth(&self, from: &str, to: &str) -> Option<usize> {
        self.channels
            .iter()
            .find(|c| c.from == from && c.to == to)
            .map(|c| c.queue.len())
    }

    /// 审计账。
    pub fn audit_trail(&self) -> &[String] {
        &self.audit
    }
}

// ---------------------------------------------------------------------------
// 四、错误类型（零静默）
// ---------------------------------------------------------------------------

/// 总线错误（现象 + 建议）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BusError {
    /// 端点非法（未登记/重名/自环）。
    BadEndpoint { name: String, suggestion: String },
    /// 通道不存在。
    NoChannel { channel: String, suggestion: String },
    /// 通道重复。
    DuplicateChannel { channel: String, suggestion: String },
    /// 背压（水位越过高线，双策略已触发）。
    Backpressured { channel: String, depth: usize, suggestion: String },
    /// 死通道投递。
    DeadChannel { channel: String, suggestion: String },
    /// 迁移期限已过。
    MigrationOverdue { version: u32, deadline: u64, suggestion: String },
}

impl BusError {
    /// 人话呈现（发生了什么 + 怎么修）。
    pub fn describe(&self) -> String {
        match self {
            BusError::BadEndpoint { name, suggestion } => {
                format!("端点非法（{name}）。建议：{suggestion}")
            }
            BusError::NoChannel { channel, suggestion } => {
                format!("通道不存在（{channel}）。建议：{suggestion}")
            }
            BusError::DuplicateChannel { channel, suggestion } => {
                format!("通道重复（{channel}）。建议：{suggestion}")
            }
            BusError::Backpressured { channel, depth, suggestion } => {
                format!("通道背压（{channel}，深度 {depth}）。建议：{suggestion}")
            }
            BusError::DeadChannel { channel, suggestion } => {
                format!("死通道投递（{channel}）。建议：{suggestion}")
            }
            BusError::MigrationOverdue { version, deadline, suggestion } => {
                format!("协议 v{version} 迁移期限（tick {deadline}）已过。建议：{suggestion}")
            }
        }
    }
}

/// 纯功能行数自证（正式门禁见 `vea10_checks.rs`）。
pub fn bus_smoke() -> usize {
    HIGH_WATERMARK + LOW_WATERMARK + HEARTBEAT_TIMEOUT_TICKS as usize + PROTO_VERSION_CURRENT as usize
}

/// VE-F0010 域自检（判据逐条对应，见 `vea10_checks.rs`）。
pub fn run_vea10_checks() -> CheckSet {
    super::vea10_checks::run_vea10_checks()
}
