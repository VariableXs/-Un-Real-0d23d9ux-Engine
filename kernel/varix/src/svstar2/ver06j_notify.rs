//! VE-F3430 · 应用换肤通知 —— 按需订阅 + 最小内容 + 丢失补发 + 泄漏回收
//! + 「先落终态再发通知」的顺序保证。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3430`
//!
//! # 职责（锚点原文拆解）
//!
//! - **按需订阅**：不是所有人都在乎换肤完成。订阅者显式订阅
//!   （[`NotifyHub::subscribe`]），带租约；不订阅就收不到，也不会有人
//!   替它「默认订阅」——默认订阅等于默认泄漏；
//! - **通知内容最小化**：通知只带**变更摘要**（条数 + 最多 8 条代表性
//!   路径 + 影响分级），绝不带令牌全量。理由有两条：全量通知会把整份
//!   令牌表复制到每个订阅者手里（单源律被绕过），且序列化成本随令牌数
//!   线性增长，而订阅者通常只想知道「变了多少、变在哪」；
//! - **顺序保证**：先落终态，再发通知。终态进账本（[`TerminalState`]）
//!   拿到序号之后，通知才能引用这个序号发出。反过来的话，订阅者收到
//!   「已完成」却查不到对应终态，只能重试或猜——这是典型的竞态；
//! - **丢失补发**：订阅者按序号确认（ack）。超过补发窗口未确认即重投，
//!   最多 [`MAX_RESEND`] 次；
//! - **泄漏回收**：订阅带租约，到期自动回收。
//!
//! # 为什么通知里不许带令牌全量
//!
//! 通知是**广播**：一份内容发给 N 个订阅者。带全量令牌意味着 N 份副本，
//! 每份都可能被订阅者改（然后被当成真值用）。单源律在广播场景下的
//! 正确形态是：通知只说「哪里变了」，订阅者要值就回读门面（F3415），
//! 门面永远是唯一真值。
//!
//! # 复杂度
//!
//! - `finish`：O(1)；
//! - `subscribe` / `unsubscribe`：O(1) 摊还；
//! - `emit`：O(订阅者数)；
//! - `ack` / `resend_due` / `sweep`：O(订阅者数)。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`。测试区除外。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E18 段续：5483 起）
// ---------------------------------------------------------------------------

/// 换肤通知诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NotifyCode {
    /// 终态未落账就发通知（顺序破坏）。
    TermalNotPersisted = 5483,
    /// 订阅者容量满。
    CapacityExceeded = 5484,
    /// 句柄失效。
    StaleHandle = 5485,
    /// 确认序号超过已派发。
    AckAhead = 5486,
    /// 补发超限判死。
    Dropped = 5487,
    /// 摘要超长（路径数超过上限）。
    SummaryTooLong = 5488,
    /// 摘要里含完整令牌值（违反最小化）。
    SummaryLeaksValues = 5489,
    /// 租约过期。
    LeaseExpired = 5490,
    /// 终态序号回退。
    SeqBackwards = 5491,
}

impl NotifyCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            NotifyCode::TermalNotPersisted => "终态未落账就发通知",
            NotifyCode::CapacityExceeded => "订阅者容量已满",
            NotifyCode::StaleHandle => "订阅句柄已失效",
            NotifyCode::AckAhead => "确认序号超过已派发",
            NotifyCode::Dropped => "补发超限判死",
            NotifyCode::SummaryTooLong => "摘要路径数超限",
            NotifyCode::SummaryLeaksValues => "摘要携带了令牌值",
            NotifyCode::LeaseExpired => "订阅租约已过期",
            NotifyCode::SeqBackwards => "终态序号回退",
        }
    }
}

/// 摘要路径上限。8 条足够让订阅者判断「跟我有关吗」。
pub const SUMMARY_PATH_CAP: usize = 8;

/// 补发上限。
pub const MAX_RESEND: u8 = 2;

/// 补发窗口（tick）。
pub const RESEND_WINDOW: u64 = 3;

// ---------------------------------------------------------------------------
// 二、终态与摘要
// ---------------------------------------------------------------------------

/// 换肤终态种类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TerminalKind {
    /// 完成。
    Completed,
    /// 失败（已回滚或已切默认主题）。
    Failed,
}

impl TerminalKind {
    /// 中文名。
    pub fn label(self) -> &'static str {
        match self {
            TerminalKind::Completed => "已完成",
            TerminalKind::Failed => "已失败",
        }
    }
}

/// 变更摘要（最小内容）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ChangeSummary {
    /// 变更条数。
    pub changed: u32,
    /// 代表性路径（≤ 8 条）。
    pub paths: Vec<String>,
    /// 影响分级文本（`无感`/`局部重绘`/`全量重绘`）。
    pub impact: String,
}

impl ChangeSummary {
    /// 构造并校验：路径数不超上限、路径里不许出现 `=`（那是值）。
    pub fn new(changed: u32, paths: Vec<String>, impact: &str) -> Result<ChangeSummary, NotifyCode> {
        if paths.len() > SUMMARY_PATH_CAP {
            return Err(NotifyCode::SummaryTooLong);
        }
        for p in paths.iter() {
            if p.contains('=') || p.contains('#') {
                return Err(NotifyCode::SummaryLeaksValues);
            }
        }
        Ok(ChangeSummary {
            changed,
            paths,
            impact: impact.to_string(),
        })
    }

    /// 一行渲染。
    pub fn render(&self) -> String {
        format!(
            "{} 条变更，影响{}，代表路径：{}",
            self.changed,
            self.impact,
            if self.paths.is_empty() {
                "无".to_string()
            } else {
                self.paths.join("、")
            }
        )
    }
}

/// 终态记录（账本行）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TerminalState {
    /// 单调序号（账本发号）。
    pub seq: u64,
    /// 第几代换肤。
    pub generation: u64,
    /// 种类。
    pub kind: TerminalKind,
    /// 摘要。
    pub summary: ChangeSummary,
    /// 落账 tick。
    pub tick: u64,
}

// ---------------------------------------------------------------------------
// 三、订阅者
// ---------------------------------------------------------------------------

/// 订阅句柄。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SubId {
    /// 槽位。
    pub slot: u32,
    /// 代数。
    pub generation: u32,
}

/// 租约。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Lease {
    /// 槽位。
    pub slot: u32,
    /// 代数。
    pub generation: u32,
    /// 到期 tick。
    pub expire_tick: u64,
}

impl Lease {
    /// 构造。
    pub fn new(slot: u32, generation: u32, ttl: u64, now: u64) -> Lease {
        Lease {
            slot,
            generation,
            expire_tick: now.saturating_add(ttl),
        }
    }

    /// 是否到期。
    pub fn expired(&self, now: u64) -> bool {
        now >= self.expire_tick
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
struct Subscriber {
    id: SubId,
    lease: Lease,
    /// 是否只关心失败（`true` 时完成事件不投）。
    failures_only: bool,
    acked: u64,
    delivered: u64,
    last_tick: u64,
    resends: u8,
    alive: bool,
}

/// 一条通知。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Notification {
    /// 收件人。
    pub to: SubId,
    /// 终态序号。
    pub seq: u64,
    /// 种类。
    pub kind: TerminalKind,
    /// 摘要。
    pub summary: ChangeSummary,
    /// 是否补发。
    pub resend: bool,
}

// ---------------------------------------------------------------------------
// 四、通知中枢
// ---------------------------------------------------------------------------

/// 通知中枢。
#[derive(Clone, Debug)]
pub struct NotifyHub {
    ledger: Vec<TerminalState>,
    subs: Vec<Subscriber>,
    next_slot: u32,
    seq: u64,
    now: u64,
    dropped: u32,
}

impl Default for NotifyHub {
    fn default() -> Self {
        NotifyHub::new()
    }
}

impl NotifyHub {
    /// 新建。
    pub fn new() -> NotifyHub {
        NotifyHub {
            ledger: Vec::new(),
            subs: Vec::new(),
            next_slot: 0,
            seq: 0,
            now: 0,
            dropped: 0,
        }
    }

    /// 推进时钟。
    pub fn tick(&mut self) -> u64 {
        self.now = self.now.saturating_add(1);
        self.now
    }

    /// 订阅。
    pub fn subscribe(&mut self, lease: Lease, failures_only: bool) -> Result<SubId, NotifyCode> {
        if lease.expired(self.now) {
            return Err(NotifyCode::LeaseExpired);
        }
        if self.subs.len() >= 1024 {
            self.sweep();
            if self.subs.len() >= 1024 {
                return Err(NotifyCode::CapacityExceeded);
            }
        }
        let slot = self.next_slot;
        self.next_slot = self.next_slot.saturating_add(1);
        let id = SubId {
            slot,
            generation: lease.generation,
        };
        self.subs.push(Subscriber {
            id,
            lease,
            failures_only,
            acked: self.seq,
            delivered: self.seq,
            last_tick: 0,
            resends: 0,
            alive: true,
        });
        Ok(id)
    }

    /// 退订。
    pub fn unsubscribe(&mut self, id: SubId) -> Result<(), NotifyCode> {
        for s in self.subs.iter_mut() {
            if s.id == id && s.alive {
                s.alive = false;
                self.subs.retain(|x| x.alive);
                return Ok(());
            }
        }
        Err(NotifyCode::StaleHandle)
    }

    /// 落终态。返回账本序号。**这是发通知的前置条件。**
    pub fn finish(
        &mut self,
        generation: u64,
        kind: TerminalKind,
        summary: ChangeSummary,
    ) -> Result<u64, NotifyCode> {
        if generation == 0 {
            return Err(NotifyCode::SeqBackwards);
        }
        if let Some(last) = self.ledger.last() {
            if generation < last.generation {
                return Err(NotifyCode::SeqBackwards);
            }
        }
        self.seq = self.seq.saturating_add(1);
        let seq = self.seq;
        self.ledger.push(TerminalState {
            seq,
            generation,
            kind,
            summary,
            tick: self.now,
        });
        Ok(seq)
    }

    /// 发通知。只允许对**已落账**的终态发；找不到即顺序破坏。
    pub fn emit(&mut self, seq: u64) -> Result<Vec<Notification>, NotifyCode> {
        let state = match self.ledger.iter().find(|s| s.seq == seq) {
            Some(s) => s.clone(),
            None => return Err(NotifyCode::TermalNotPersisted),
        };
        let mut out = Vec::new();
        for s in self.subs.iter_mut() {
            if !s.alive {
                continue;
            }
            if s.failures_only && state.kind != TerminalKind::Failed {
                continue;
            }
            s.delivered = seq;
            s.last_tick = self.now;
            s.resends = 0;
            out.push(Notification {
                to: s.id,
                seq,
                kind: state.kind,
                summary: state.summary.clone(),
                resend: false,
            });
        }
        Ok(out)
    }

    /// 确认。
    pub fn ack(&mut self, id: SubId, seq: u64) -> Result<u64, NotifyCode> {
        for s in self.subs.iter_mut() {
            if s.id == id && s.alive {
                if seq > s.delivered {
                    return Err(NotifyCode::AckAhead);
                }
                if seq <= s.acked {
                    return Ok(0);
                }
                let advanced = seq - s.acked;
                s.acked = seq;
                s.resends = 0;
                return Ok(advanced);
            }
        }
        Err(NotifyCode::StaleHandle)
    }

    /// 补发：超过窗口未确认的，重投一次；超限判死。
    pub fn resend_due(&mut self) -> Vec<Notification> {
        let now = self.now;
        let mut out = Vec::new();
        let mut i = 0;
        while i < self.subs.len() {
            let need = match self.subs.get(i) {
                Some(s) => {
                    s.alive
                        && s.delivered > s.acked
                        && now.saturating_sub(s.last_tick) > RESEND_WINDOW
                }
                None => false,
            };
            if !need {
                i += 1;
                continue;
            }
            let over = match self.subs.get(i) {
                Some(s) => s.resends >= MAX_RESEND,
                None => false,
            };
            if over {
                if let Some(s) = self.subs.get_mut(i) {
                    s.alive = false;
                    self.dropped = self.dropped.saturating_add(1);
                }
                i += 1;
                continue;
            }
            let state = match self.ledger.iter().find(|s| s.seq == {
                match self.subs.get(i) {
                    Some(s) => s.delivered,
                    None => 0,
                }
            }) {
                Some(s) => s.clone(),
                None => {
                    i += 1;
                    continue;
                }
            };
            if let Some(s) = self.subs.get_mut(i) {
                s.resends = s.resends.saturating_add(1);
                s.last_tick = now;
                if !(s.failures_only && state.kind != TerminalKind::Failed) {
                    out.push(Notification {
                        to: s.id,
                        seq: state.seq,
                        kind: state.kind,
                        summary: state.summary.clone(),
                        resend: true,
                    });
                }
            }
            i += 1;
        }
        out
    }

    /// 回收过期租约。
    pub fn sweep(&mut self) -> usize {
        let now = self.now;
        let before = self.subs.len();
        self.subs.retain(|s| s.alive && !s.lease.expired(now));
        before.saturating_sub(self.subs.len())
    }

    /// 账本长度。
    pub fn ledger_len(&self) -> usize {
        self.ledger.len()
    }

    /// 订阅者数。
    pub fn subscriber_count(&self) -> usize {
        self.subs.len()
    }

    /// 判死数。
    pub fn dropped(&self) -> u32 {
        self.dropped
    }

    /// 读屏播报。
    pub fn reading_lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        out.push(format!(
            "换肤通知：账本 {} 条终态，{} 个订阅者，判死 {} 个",
            self.ledger.len(),
            self.subs.len(),
            self.dropped
        ));
        for s in self.ledger.iter().rev().take(4) {
            out.push(format!(
                "第{}代{}（序号{}）：{}",
                s.generation,
                s.kind.label(),
                s.seq,
                s.summary.render()
            ));
        }
        if out.len() == 1 {
            out.push("还没有换肤终态".to_string());
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

    fn summary() -> ChangeSummary {
        ChangeSummary::new(
            12,
            alloc::vec!["color.bg".to_string(), "spacing.gap".to_string()],
            "局部重绘",
        )
        .expect("摘要")
    }

    #[test]
    fn notify_requires_terminal_state_first() {
        let mut hub = NotifyHub::new();
        let lease = Lease::new(1, 1, 100, 0);
        let _ = hub.subscribe(lease, false).expect("订阅");
        // 未落账就发 -> 顺序破坏。
        assert_eq!(hub.emit(1).err(), Some(NotifyCode::TermalNotPersisted));
        let seq = hub.finish(1, TerminalKind::Completed, summary()).expect("落账");
        let notes = hub.emit(seq).expect("发");
        assert_eq!(notes.len(), 1);
        assert_eq!(notes.first().map(|n| n.kind), Some(TerminalKind::Completed));
    }

    #[test]
    fn summary_is_minimal_and_rejects_values() {
        let s = summary();
        assert!(s.paths.len() <= SUMMARY_PATH_CAP);
        assert!(!s.render().contains('='));
        // 带值的摘要拒收。
        assert_eq!(
            ChangeSummary::new(1, alloc::vec!["color.bg=#FF0000".to_string()], "局部")
                .err(),
            Some(NotifyCode::SummaryLeaksValues)
        );
        // 超长拒收。
        let mut many = Vec::new();
        let mut i = 0;
        while i < SUMMARY_PATH_CAP + 1 {
            many.push(format!("color.p{}", i));
            i += 1;
        }
        assert_eq!(
            ChangeSummary::new(9, many, "局部").err(),
            Some(NotifyCode::SummaryTooLong)
        );
    }

    #[test]
    fn failures_only_subscriber_skips_success() {
        let mut hub = NotifyHub::new();
        let lease = Lease::new(1, 1, 100, 0);
        let _ = hub.subscribe(lease, true).expect("订阅");
        let seq = hub.finish(1, TerminalKind::Completed, summary()).expect("落账");
        assert!(hub.emit(seq).expect("发").is_empty());
        let seq2 = hub.finish(2, TerminalKind::Failed, summary()).expect("落账");
        assert_eq!(hub.emit(seq2).expect("发").len(), 1);
    }

    #[test]
    fn lost_notification_is_resent_then_dropped() {
        let mut hub = NotifyHub::new();
        let lease = Lease::new(1, 1, 1000, 0);
        let id = hub.subscribe(lease, false).expect("订阅");
        let seq = hub.finish(1, TerminalKind::Completed, summary()).expect("落账");
        let _ = hub.emit(seq).expect("发");
        // 拖过窗口 -> 补发。
        let mut i = 0;
        while i < RESEND_WINDOW + 1 {
            hub.tick();
            i += 1;
        }
        let r = hub.resend_due();
        assert_eq!(r.len(), 1);
        assert!(r.first().map(|n| n.resend).unwrap_or(false));
        // 继续拖 -> 超限判死。
        let mut i = 0;
        while i < RESEND_WINDOW + 1 {
            hub.tick();
            i += 1;
        }
        let _ = hub.resend_due();
        let mut i = 0;
        while i < RESEND_WINDOW + 1 {
            hub.tick();
            i += 1;
        }
        let _ = hub.resend_due();
        assert!(hub.dropped() >= 1);
        assert!(hub.ack(id, seq).is_err());
    }

    #[test]
    fn lease_expiry_recycles_subscription() {
        let mut hub = NotifyHub::new();
        let lease = Lease::new(1, 1, 2, 0);
        let _ = hub.subscribe(lease, false).expect("订阅");
        hub.tick();
        hub.tick();
        assert_eq!(hub.sweep(), 1);
        assert_eq!(hub.subscriber_count(), 0);
    }

    #[test]
    fn ack_advances_and_reports_progress() {
        let mut hub = NotifyHub::new();
        let lease = Lease::new(1, 1, 100, 0);
        let id = hub.subscribe(lease, false).expect("订阅");
        let seq = hub.finish(1, TerminalKind::Completed, summary()).expect("落账");
        let _ = hub.emit(seq).expect("发");
        assert_eq!(hub.ack(id, seq).ok(), Some(1));
        assert_eq!(hub.ack(id, seq).ok(), Some(0), "重复确认不推进");
        // 越界确认拒绝。
        let seq2 = hub.finish(2, TerminalKind::Completed, summary()).expect("落账");
        let _ = hub.emit(seq2).expect("发");
        assert_eq!(hub.ack(id, 99).err(), Some(NotifyCode::AckAhead));
    }

    #[test]
    fn generation_cannot_go_backwards() {
        let mut hub = NotifyHub::new();
        hub.finish(5, TerminalKind::Completed, summary()).expect("落账");
        assert_eq!(
            hub.finish(4, TerminalKind::Completed, summary()).err(),
            Some(NotifyCode::SeqBackwards)
        );
    }

    #[test]
    fn reading_lines_show_ledger_and_subscribers() {
        let hub_lines = NotifyHub::new().reading_lines();
        assert!(hub_lines.iter().any(|l| l.contains("还没有换肤终态")));
        let mut hub = NotifyHub::new();
        let lease = Lease::new(1, 1, 100, 0);
        let _ = hub.subscribe(lease, false).expect("订阅");
        let _ = hub.finish(1, TerminalKind::Failed, summary()).expect("落账");
        let lines = hub.reading_lines();
        assert!(lines.first().map(|l| l.contains("1 个订阅者")).unwrap_or(false));
        assert!(lines.iter().any(|l| l.contains("已失败")));
    }
}
