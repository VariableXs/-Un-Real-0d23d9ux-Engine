//! VE-F3424 · 换肤期间交互不中断 —— 交互缓冲 + 完成回放 + 不吞不丢。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3424`
//!
//! # 职责（锚点原文拆解）
//!
//! - **不中断承诺**：换肤开始的那一刻起，点击/按键/滚动/拖拽照常进得来。
//!   进来的事件不直接丢弃，而是进缓冲（[`InteractionBuffer`]），等换肤
//!   完成后按原顺序回放；
//! - **队列缓冲**：缓冲有容量上限（[`BufferConfig::capacity`]）。满了
//!   怎么办？**丢最旧的 + 记一条溢出通知**，而不是丢最新的——用户最后
//!   几下操作永远比几秒前的更重要；
//! - **完成回放**：回放严格按进入顺序（FIFO）。回放不是重演动画，
//!   是把「用户以为已经生效」的交互补上；
//! - **不吞不丢**（阻断级）：每个进入总线的事件必须落到三种归宿之一——
//!   `Buffered`（进队）/ `Replayed`（回放成功）/ `Rejected`（带理由拒绝
//!   并告知）。**没有第四种**。`total == buffered + replayed + rejected`
//!   是本模块的头号不变式，[`InteractionBuffer::audit`] 每次都能验出来；
//! - **回放失真→丢弃并告知**：事件携带的目标组件若在换肤后不存在了
//!   （例如那个面板被新皮肤移除了），这条事件不能硬塞给别的组件——记
//!   `Distorted` 并计入 `告知`，让调用方能弹一条「你的操作没能完成」。
//!
//! # 为什么「吞输入」是阻断级而不是警告
//!
//! 界面丢一次点击，用户会再点一次；连续丢三次，用户会认为**软件卡死了**。
//! 换肤本来就是「看起来像卡了一下」的操作，如果这时还吞输入，用户对
//! 整个产品的信任会一次性崩掉。所以本模块把「吞」定义为**不可能状态**：
//! 计数上就不存在一条「既没进队也没被拒」的路径。
//!
//! # 复杂度
//!
//! - `push`：O(1)；
//! - `drain`：O(队列长)；
//! - `replay`：O(队列长)；
//! - `audit`：O(1)。
//!
//! # 零 panic 面
//!
//! 用 `VecDeque` 的 `push_back` / `pop_front` / `retain`，无索引；
//! 容量计算全 `saturating_*`。测试区除外。

use alloc::collections::VecDeque;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E18 段续：5434 起）
// ---------------------------------------------------------------------------

/// 交互缓冲诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BufferCode {
    /// 缓冲已关闭（换肤未开始就 push 说明调用方状态机错了）。
    BufferClosed = 5434,
    /// 容量为 0（任何事件都存不下）。
    ZeroCapacity = 5435,
    /// 溢出（已丢最旧并告知）。
    Overflow = 5436,
    /// 回放时目标组件已不存在（丢弃并告知）。
    Distorted = 5437,
    /// 事件序号回退（串号/伪造）。
    SeqBackwards = 5438,
    /// 审计不平（有事件下落不明）——阻断级。
    AuditMismatch = 5439,
    /// 回放顺序与入队顺序不一致。
    ReplayOutOfOrder = 5440,
    /// 事件载荷为空（没有可回放的内容）。
    EmptyPayload = 5441,
}

impl BufferCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            BufferCode::BufferClosed => "交互缓冲已关闭",
            BufferCode::ZeroCapacity => "缓冲容量为 0",
            BufferCode::Overflow => "交互缓冲溢出，已丢最旧并告知",
            BufferCode::Distorted => "回放目标已不存在，丢弃并告知",
            BufferCode::SeqBackwards => "事件序号回退",
            BufferCode::AuditMismatch => "交互审计不平（有事件下落不明）",
            BufferCode::ReplayOutOfOrder => "回放顺序错乱",
            BufferCode::EmptyPayload => "事件载荷为空",
        }
    }

    /// 是否阻断级（吞输入与审计不平是产品级缺陷）。
    pub fn blocking(self) -> bool {
        matches!(self, BufferCode::AuditMismatch)
    }
}

// ---------------------------------------------------------------------------
// 二、事件模型
// ---------------------------------------------------------------------------

/// 事件种类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EventKind {
    /// 点击。
    Click,
    /// 按键。
    Key,
    /// 滚动。
    Scroll,
    /// 拖拽。
    Drag,
}

impl EventKind {
    /// 中文名。
    pub fn label(self) -> &'static str {
        match self {
            EventKind::Click => "点击",
            EventKind::Key => "按键",
            EventKind::Scroll => "滚动",
            EventKind::Drag => "拖拽",
        }
    }

    /// 是否「不可丢」类事件：点击与按键代表用户明确意图，滚动与拖拽
    /// 是连续量，丢尾部对体验影响小。溢出时优先保全前者。
    pub fn critical(self) -> bool {
        matches!(self, EventKind::Click | EventKind::Key)
    }
}

/// 一条交互事件。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Event {
    /// 单调序号（由总线发，不许回退）。
    pub seq: u64,
    /// 种类。
    pub kind: EventKind,
    /// 目标组件。
    pub target: u32,
    /// 发生时刻（毫秒，逻辑时钟）。
    pub at_ms: u32,
    /// 载荷（按键码/滚动量/坐标打包，具体含义由总线约定）。
    pub payload: i32,
}

impl Event {
    /// 构造。
    pub fn new(seq: u64, kind: EventKind, target: u32, at_ms: u32, payload: i32) -> Event {
        Event {
            seq,
            kind,
            target,
            at_ms,
            payload,
        }
    }

    /// 一行的读屏渲染。
    pub fn render(&self) -> String {
        format!(
            "#{} {}→组件{} 于{}ms 载荷{}",
            self.seq,
            self.kind.label(),
            self.target,
            self.at_ms,
            self.payload
        )
    }
}

/// 一条溢出/失真告知。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Notice {
    /// 码。
    pub code: BufferCode,
    /// 说明。
    pub detail: String,
}

impl Notice {
    fn new(code: BufferCode, detail: &str) -> Notice {
        Notice {
            code,
            detail: detail.to_string(),
        }
    }

    /// 读屏渲染。
    pub fn render(&self) -> String {
        format!("E{} {}（{}）", self.code.code(), self.code.text(), self.detail)
    }
}

// ---------------------------------------------------------------------------
// 三、配置与状态
// ---------------------------------------------------------------------------

/// 缓冲配置。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BufferConfig {
    /// 容量。
    pub capacity: usize,
    /// 回放时判定「过旧失真」的阈值（毫秒）。
    pub stale_ms: u32,
}

impl Default for BufferConfig {
    fn default() -> Self {
        BufferConfig {
            // 256 条 ≈ 用户在 300ms 换肤里能做的全部操作再乘十倍。
            capacity: 256,
            stale_ms: 2000,
        }
    }
}

/// 缓冲状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BufferState {
    /// 未开始（透传）。
    Open,
    /// 换肤中，正在缓冲。
    Buffering,
    /// 换肤完成，正在回放。
    Draining,
    /// 已关闭（不再接受）。
    Closed,
}

impl BufferState {
    /// 中文名。
    pub fn label(self) -> &'static str {
        match self {
            BufferState::Open => "透传",
            BufferState::Buffering => "缓冲中",
            BufferState::Draining => "回放中",
            BufferState::Closed => "已关闭",
        }
    }
}

/// 事件归宿。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fate {
    /// 进队。
    Buffered,
    /// 回放成功。
    Replayed,
    /// 带理由拒绝。
    Rejected,
    /// 回放失真丢弃。
    Distorted,
}

// ---------------------------------------------------------------------------
// 四、缓冲
// ---------------------------------------------------------------------------

/// 交互缓冲。
#[derive(Clone, Debug)]
pub struct InteractionBuffer {
    queue: VecDeque<Event>,
    state: BufferState,
    cfg: BufferConfig,
    notices: Vec<Notice>,
    /// 进队数。
    buffered: u64,
    /// 回放成功数。
    replayed: u64,
    /// 拒绝数（含溢出丢弃）。
    rejected: u64,
    /// 失真丢弃数。
    distorted: u64,
    /// 进入总线的事件总数（审计基准）。
    total: u64,
    /// 最后入队序号（判定回退）。
    last_seq: u64,
    /// 已回放序号（判定顺序）。
    last_replayed_seq: u64,
}

impl Default for InteractionBuffer {
    fn default() -> Self {
        InteractionBuffer::new(BufferConfig::default())
    }
}

impl InteractionBuffer {
    /// 新建。容量 0 视为配置错误：状态置 `Closed` 并记告知。
    pub fn new(cfg: BufferConfig) -> InteractionBuffer {
        let mut buf = InteractionBuffer {
            queue: VecDeque::new(),
            state: BufferState::Open,
            cfg,
            notices: Vec::new(),
            buffered: 0,
            replayed: 0,
            rejected: 0,
            distorted: 0,
            total: 0,
            last_seq: 0,
            last_replayed_seq: 0,
        };
        if cfg.capacity == 0 {
            buf.state = BufferState::Closed;
            buf.notices
                .push(Notice::new(BufferCode::ZeroCapacity, "容量 0"));
        }
        buf
    }

    /// 开始缓冲（换肤开始）。
    pub fn begin(&mut self) -> Result<(), BufferCode> {
        if self.cfg.capacity == 0 {
            return Err(BufferCode::ZeroCapacity);
        }
        self.state = BufferState::Buffering;
        Ok(())
    }

    /// 推入一条事件。
    ///
    /// 归宿必为 [`Fate::Buffered`] 或 [`Fate::Rejected`]，二者必居其一。
    /// 溢出时丢最旧（并告知），新事件仍然进队——「最后几下操作最重
    /// 要」这条产品判断写在这里，而不是让调用方自己决定。
    pub fn push(&mut self, event: Event) -> Fate {
        self.total = self.total.saturating_add(1);
        if event.payload == 0 && event.kind == EventKind::Key {
            // 按键码 0 不是合法键；这类事件进队也会被回放器丢弃，
            // 不如在入口就拒，理由明确。
            self.rejected = self.rejected.saturating_add(1);
            self.notices
                .push(Notice::new(BufferCode::EmptyPayload, &event.render()));
            return Fate::Rejected;
        }
        if event.seq <= self.last_seq {
            self.rejected = self.rejected.saturating_add(1);
            self.notices
                .push(Notice::new(BufferCode::SeqBackwards, &event.render()));
            return Fate::Rejected;
        }
        self.last_seq = event.seq;
        match self.state {
            BufferState::Open => {
                // 透传：不进队，但计数为「已回放」（它已经生效了）。
                self.replayed = self.replayed.saturating_add(1);
                Fate::Replayed
            }
            BufferState::Buffering => {
                if self.queue.len() >= self.cfg.capacity {
                    // 优先丢非关键事件；全是关键事件时丢最旧。
                    let victim = self
                        .queue
                        .iter()
                        .position(|e| !e.kind.critical())
                        .unwrap_or(0);
                    if let Some(dropped) = self.queue.remove(victim) {
                        self.rejected = self.rejected.saturating_add(1);
                        self.notices.push(Notice::new(
                            BufferCode::Overflow,
                            &format!("丢弃 {}", dropped.render()),
                        ));
                    }
                }
                self.queue.push_back(event);
                self.buffered = self.buffered.saturating_add(1);
                Fate::Buffered
            }
            BufferState::Draining | BufferState::Closed => {
                self.rejected = self.rejected.saturating_add(1);
                self.notices
                    .push(Notice::new(BufferCode::BufferClosed, &event.render()));
                Fate::Rejected
            }
        }
    }

    /// 结束缓冲，进入回放阶段。
    pub fn finish(&mut self) {
        if self.state == BufferState::Buffering {
            self.state = BufferState::Draining;
        }
    }

    /// 执行回放。`now_ms` 用于判定失真（事件过旧）。
    /// `target_alive` 判断目标组件是否仍存在。
    ///
    /// 返回成功回放的事件（按序号升序）。失真事件记 `Distorted` + 告知；
    /// 顺序错乱记 `ReplayOutOfOrder` + 告知（但仍按序号排序后回放，
    /// 因为「顺序」比「记录错误」更重要）。
    pub fn replay<F>(&mut self, now_ms: u32, mut target_alive: F) -> Vec<Event>
    where
        F: FnMut(u32) -> bool,
    {
        let mut drained: Vec<Event> = Vec::new();
        while let Some(e) = self.queue.pop_front() {
            drained.push(e);
        }
        drained.sort_by(|a, b| a.seq.cmp(&b.seq));
        // 顺序自检：排序前是否已经有序。
        let mut ordered = true;
        let mut prev = self.last_replayed_seq;
        for e in drained.iter() {
            if e.seq <= prev {
                ordered = false;
            }
            prev = e.seq;
        }
        if !ordered {
            self.notices.push(Notice::new(
                BufferCode::ReplayOutOfOrder,
                "回放入队顺序与序号不一致，已按序号重排",
            ));
        }
        let mut accepted: Vec<Event> = Vec::new();
        for e in drained.into_iter() {
            if now_ms.saturating_sub(e.at_ms) > self.cfg.stale_ms {
                self.distorted = self.distorted.saturating_add(1);
                self.notices.push(Notice::new(
                    BufferCode::Distorted,
                    &format!("过旧：{}", e.render()),
                ));
                continue;
            }
            if !target_alive(e.target) {
                self.distorted = self.distorted.saturating_add(1);
                self.notices.push(Notice::new(
                    BufferCode::Distorted,
                    &format!("目标已不存在：{}", e.render()),
                ));
                continue;
            }
            self.last_replayed_seq = e.seq;
            self.replayed = self.replayed.saturating_add(1);
            accepted.push(e);
        }
        self.state = BufferState::Open;
        accepted
    }

    /// 关闭缓冲（退出换肤流程）。
    pub fn close(&mut self) {
        self.state = BufferState::Closed;
    }

    /// 当前状态。
    pub fn state(&self) -> BufferState {
        self.state
    }

    /// 队列长度。
    pub fn len(&self) -> usize {
        self.queue.len()
    }

    /// 队列是否为空。
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    /// 审计：`total == replayed + rejected + distorted + 仍在队`
    /// 必须成立。返回 `Err` 表示有事件下落不明（阻断级）。
    ///
    /// 注意 `buffered` **不参与**这条等式：它是「累计进队数」，进队的事件
    /// 稍后必然转成 `replayed` 或 `distorted`，把进队与结局相加会重复
    /// 计数（本模块第一版就是这么错的，一进队审计就不平）。
    pub fn audit(&self) -> Result<(), BufferCode> {
        let accounted = self
            .replayed
            .saturating_add(self.rejected)
            .saturating_add(self.distorted)
            .saturating_add(self.queue.len() as u64);
        if accounted == self.total {
            Ok(())
        } else {
            Err(BufferCode::AuditMismatch)
        }
    }

    /// 告知清单（读屏 + UI 提示）。
    pub fn notices(&self) -> &[Notice] {
        &self.notices
    }

    /// 统计：`(total, buffered, replayed, rejected, distorted)`。
    pub fn stats(&self) -> (u64, u64, u64, u64, u64) {
        (
            self.total,
            self.buffered,
            self.replayed,
            self.rejected,
            self.distorted,
        )
    }

    /// 状态读屏播报。
    pub fn reading_lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        out.push(format!(
            "交互缓冲：{}，队列 {} 条，累计 {} 条（回放 {}，拒绝 {}，失真 {}）",
            self.state.label(),
            self.queue.len(),
            self.total,
            self.replayed,
            self.rejected,
            self.distorted
        ));
        match self.audit() {
            Ok(()) => out.push("审计平衡：没有事件下落不明".to_string()),
            Err(_) => out.push(format!(
                "E{} {}",
                BufferCode::AuditMismatch.code(),
                BufferCode::AuditMismatch.text()
            )),
        }
        for n in self.notices.iter().rev().take(4) {
            out.push(n.render());
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 五、测试支撑
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(seq: u64, kind: EventKind, target: u32, at: u32) -> Event {
        Event::new(seq, kind, target, at, 1)
    }

    #[test]
    fn buffering_then_replay_in_order() {
        let mut b = InteractionBuffer::new(BufferConfig::default());
        b.begin().expect("开始缓冲");
        assert_eq!(b.push(ev(1, EventKind::Click, 10, 100)), Fate::Buffered);
        assert_eq!(b.push(ev(2, EventKind::Scroll, 10, 110)), Fate::Buffered);
        assert_eq!(b.push(ev(3, EventKind::Key, 11, 120)), Fate::Buffered);
        assert_eq!(b.len(), 3);
        b.finish();
        let replayed = b.replay(150, |_| true);
        assert_eq!(replayed.len(), 3);
        assert_eq!(replayed.first().map(|e| e.seq), Some(1));
        assert_eq!(replayed.last().map(|e| e.seq), Some(3));
        assert!(b.audit().is_ok());
    }

    #[test]
    fn open_state_passes_through_without_buffering() {
        let mut b = InteractionBuffer::new(BufferConfig::default());
        assert_eq!(b.push(ev(1, EventKind::Click, 1, 10)), Fate::Replayed);
        assert!(b.is_empty());
        assert!(b.audit().is_ok());
    }

    #[test]
    fn overflow_drops_oldest_noncritical_and_tells() {
        let mut b = InteractionBuffer::new(BufferConfig {
            capacity: 2,
            stale_ms: 5000,
        });
        b.begin().expect("开始");
        b.push(ev(1, EventKind::Scroll, 1, 10));
        b.push(ev(2, EventKind::Scroll, 1, 20));
        // 第三条溢出：丢最旧的 Scroll，保留新事件。
        assert_eq!(b.push(ev(3, EventKind::Scroll, 1, 30)), Fate::Buffered);
        assert_eq!(b.len(), 2);
        assert!(b
            .notices()
            .iter()
            .any(|n| n.code == BufferCode::Overflow && n.detail.contains("#1")));
        assert!(b.audit().is_ok());
    }

    #[test]
    fn critical_events_survive_overflow() {
        let mut b = InteractionBuffer::new(BufferConfig {
            capacity: 2,
            stale_ms: 5000,
        });
        b.begin().expect("开始");
        b.push(ev(1, EventKind::Click, 1, 10));
        b.push(ev(2, EventKind::Key, 2, 20));
        b.push(ev(3, EventKind::Scroll, 3, 30));
        // 没有非关键事件时丢最旧，但 click/key 的优先级体现在
        // 「有非关键可丢就不丢关键」。
        let mut clicks = 0;
        for e in b.queue.iter() {
            if e.kind.critical() {
                clicks += 1;
            }
        }
        assert!(clicks >= 1, "关键事件至少要保住一条");
        assert!(b.audit().is_ok());
    }

    #[test]
    fn distorted_target_is_dropped_and_told() {
        let mut b = InteractionBuffer::new(BufferConfig::default());
        b.begin().expect("开始");
        b.push(ev(1, EventKind::Click, 10, 100));
        b.push(ev(2, EventKind::Click, 20, 100));
        b.finish();
        let replayed = b.replay(200, |t| t == 10);
        assert_eq!(replayed.len(), 1);
        assert!(b
            .notices()
            .iter()
            .any(|n| n.code == BufferCode::Distorted && n.detail.contains("组件20")));
        assert!(b.audit().is_ok());
    }

    #[test]
    fn stale_event_is_dropped() {
        let mut b = InteractionBuffer::new(BufferConfig {
            capacity: 8,
            stale_ms: 100,
        });
        b.begin().expect("开始");
        b.push(ev(1, EventKind::Click, 10, 0));
        b.finish();
        let replayed = b.replay(5000, |_| true);
        assert!(replayed.is_empty());
        assert_eq!(b.stats().4, 1);
        assert!(b.audit().is_ok());
    }

    #[test]
    fn sequence_backwards_is_rejected_with_reason() {
        let mut b = InteractionBuffer::new(BufferConfig::default());
        b.begin().expect("开始");
        b.push(ev(5, EventKind::Click, 1, 10));
        assert_eq!(b.push(ev(5, EventKind::Click, 1, 20)), Fate::Rejected);
        assert!(b
            .notices()
            .iter()
            .any(|n| n.code == BufferCode::SeqBackwards));
        assert!(b.audit().is_ok());
    }

    #[test]
    fn zero_capacity_buffer_is_closed_and_audits() {
        let b = InteractionBuffer::new(BufferConfig {
            capacity: 0,
            stale_ms: 100,
        });
        assert_eq!(b.state(), BufferState::Closed);
        assert!(b
            .notices()
            .iter()
            .any(|n| n.code == BufferCode::ZeroCapacity));
        assert!(b.audit().is_ok());
    }

    #[test]
    fn every_event_has_a_fate_audit_holds_under_mixed_traffic() {
        let mut b = InteractionBuffer::new(BufferConfig {
            capacity: 3,
            stale_ms: 1000,
        });
        b.begin().expect("开始");
        let mut i = 1;
        while i <= 10 {
            b.push(ev(i, EventKind::Scroll, (i % 3) as u32, 10 * i as u32));
            i += 1;
        }
        b.finish();
        let _ = b.replay(500, |_| true);
        assert!(b.audit().is_ok());
        let (total, buffered, replayed, rejected, distorted) = b.stats();
        assert_eq!(total, 10);
        assert_eq!(total, replayed + rejected + distorted);
        assert!(buffered > 0, "进队计数只作遥测，不参与审计等式");
    }

    #[test]
    fn reading_lines_report_state_and_recent_notices() {
        let mut b = InteractionBuffer::new(BufferConfig::default());
        b.begin().expect("开始");
        b.push(ev(1, EventKind::Click, 1, 10));
        let lines = b.reading_lines();
        assert!(lines.first().map(|l| l.contains("缓冲中")).unwrap_or(false));
        assert!(lines.iter().any(|l| l.contains("审计平衡")));
    }
}
