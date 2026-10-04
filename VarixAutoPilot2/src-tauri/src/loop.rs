//! 待发队列 + 循环引擎。
//!
//! 动因（实测教训）：忙时点发送，若只是弹一句错误就结束，用户干看着没辙
//! ——这就是 Variable 反馈的"发送功能无法运行"的体验侧。
//! 正解：忙时把内容**存进待发队列**，对方一空闲就自动发。
//!
//! 与手动发送的关系：**共用同一条发送路径**，只是来源不同——
//! 手动发送 = 往队列塞一条 + 唤醒 worker；循环 = 连续塞 N 条。
//! 这样两条路绝不会出现行为不一致。

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::cdp::Cdp;
use crate::engine;

/// 一条待发项的对外快照（给前端渲染用）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueItem {
    pub id: u64,
    /// 预览用（长内容截断）
    pub preview: String,
    /// 完整内容
    pub text: String,
    /// 目标对话 id；空串 = 当前选中对话
    pub conv_id: String,
    /// 属于第几轮；0 = 单发（非循环）
    pub round: u32,
    pub state: String,
    pub evidence: String,
    pub err: String,
    pub enqueued_at: String,
}

#[derive(Debug, Clone)]
struct Inner {
    items: VecDeque<QueueItem>,
    /// 循环配置（None = 不循环）
    loop_cfg: Option<LoopCfg>,
    /// 循环已发出的轮数
    round_done: u32,
}

/// 循环配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopCfg {
    /// 触发方式："idle" | "interval" | "hybrid"
    pub trigger: String,
    /// 轮数；u32::MAX 表示无限
    pub rounds: u32,
    /// interval 模式的间隔秒
    pub interval_s: u32,
    /// hybrid 模式等空闲的超时秒
    pub idle_timeout_s: u32,
}

pub struct Queue {
    inner: Mutex<Inner>,
    next_id: AtomicU64,
    /// 队列变化通知（worker 靠它唤醒）
    pub notify: Arc<tokio::sync::Notify>,
}

impl Queue {
    pub fn new() -> Arc<Queue> {
        Arc::new(Queue {
            inner: Mutex::new(Inner {
                items: VecDeque::new(),
                loop_cfg: None,
                round_done: 0,
            }),
            next_id: AtomicU64::new(1),
            notify: Arc::new(tokio::sync::Notify::new()),
        })
    }

    /// 往队列加一条，返回 id。
    pub async fn push(&self, text: &str, conv_id: &str, round: u32) -> u64 {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let preview: String = text.chars().take(60).collect();
        let item = QueueItem {
            id,
            preview: if text.chars().count() > 60 {
                format!("{preview}…")
            } else {
                preview
            },
            text: text.to_string(),
            conv_id: conv_id.to_string(),
            round,
            state: "pending".into(),
            evidence: String::new(),
            err: String::new(),
            enqueued_at: now_hm(),
        };
        let mut g = self.inner.lock().await;
        g.items.push_back(item);
        drop(g);
        self.notify.notify_one();
        id
    }

    /// 待发条数（pending + sending）。
    pub async fn pending_count(&self) -> usize {
        let g = self.inner.lock().await;
        g.items
            .iter()
            .filter(|i| i.state == "pending" || i.state == "sending")
            .count()
    }

    /// 取一条最早的 pending 并标记为 sending（避免 worker 重复取）。
    pub async fn take_next(&self) -> Option<QueueItem> {
        let mut g = self.inner.lock().await;
        let pos = g.items.iter().position(|i| i.state == "pending")?;
        g.items[pos].state = "sending".into();
        Some(g.items[pos].clone())
    }

    /// 更新某条的状态/证据。
    pub async fn finish(&self, id: u64, state: &str, evidence: &str, err: &str) {
        let mut g = self.inner.lock().await;
        if let Some(it) = g.items.iter_mut().find(|i| i.id == id) {
            it.state = state.into();
            it.evidence = evidence.into();
            it.err = err.into();
        }
    }

    /// 清掉已完成的（保持队列整洁，只留未完成项）。
    pub async fn prune(&self, keep: usize) {
        let mut g = self.inner.lock().await;
        while g.items.len() > keep {
            if g.items.back().map(|i| i.state == "pending" || i.state == "sending") == Some(true) {
                break;
            }
            g.items.pop_back();
        }
    }

    /// 全部快照（最新的在后）。
    pub async fn snapshot(&self) -> Vec<QueueItem> {
        let g = self.inner.lock().await;
        g.items.iter().cloned().collect()
    }

    /// 取消某条（仅 pending 可取消）。
    pub async fn cancel(&self, id: u64) -> bool {
        let mut g = self.inner.lock().await;
        if let Some(it) = g.items.iter_mut().find(|i| i.id == id && i.state == "pending") {
            it.state = "canceled".into();
            true
        } else {
            false
        }
    }

    /// 配置循环。
    pub async fn set_loop(&self, cfg: Option<LoopCfg>) {
        let mut g = self.inner.lock().await;
        g.loop_cfg = cfg;
        g.round_done = 0;
    }

    pub async fn loop_cfg(&self) -> Option<LoopCfg> {
        let g = self.inner.lock().await;
        g.loop_cfg.clone()
    }

    pub async fn round_done(&self) -> u32 {
        let g = self.inner.lock().await;
        g.round_done
    }

    pub async fn bump_round(&self) {
        let mut g = self.inner.lock().await;
        g.round_done = g.round_done.saturating_add(1);
    }

    /// 是否应继续循环（未达轮数、且未停）。
    pub async fn should_continue(&self, stop: &Arc<AtomicBool>) -> bool {
        if stop.load(Ordering::SeqCst) {
            return false;
        }
        let g = self.inner.lock().await;
        match &g.loop_cfg {
            None => false,
            Some(c) => c.rounds == u32::MAX || g.round_done < c.rounds,
        }
    }

    /// 立即唤醒 worker。
    pub fn kick(&self) {
        self.notify.notify_one();
    }
}

/// 后台 worker：持续把队列里的 pending 发出去。
///
/// 关键设计：
/// - 忙时**不取**队列项（一个都不碰），只等——保证"忙时一个字都不写"。
/// - 单条失败不阻塞后续：标记 failed 后继续下一条。
/// - 停止靠AtomicBool，不靠关窗口（关窗口时 worker 直接死，队列里 sending 的项会丢）。
pub async fn worker(
    q: Arc<Queue>,
    cdp: Arc<Cdp>,
    stop: Arc<AtomicBool>,
    interval_hint: Arc<Mutex<Duration>>,
) {
    let mut last_send: Option<Instant> = None;
    loop {
        if stop.load(Ordering::SeqCst) {
            return;
        }

        // 等唤醒或超时（超时是为了周期性检查循环轮次上限）
        let _ = tokio::time::timeout(Duration::from_millis(500), q.notify.notified()).await;

        if stop.load(Ordering::SeqCst) {
            return;
        }

        // 取一条 pending
        let Some(item) = q.take_next().await else {
            // 队列空：看看循环还要不要继续
            if q.should_continue(&stop).await {
                // 循环 mode：这里应补下一轮内容。
                // 但"下一轮内容从哪来"是上层决策（可能来自多轮模板），
                // 所以由 lib.rs 注册的回调处理——见 loop_fill_next。
                // 找不到内容就停下，不空转。
            }
            continue;
        };

        // 忙闲判定（三判据多数一致，保守：任一判据说忙就等）
        let v = match engine::idle_verdict(&cdp).await {
            Ok(v) => v,
            Err(e) => {
                // 读不到状态：保守当作忙，退回 pending 等下一轮
                q.finish(item.id, "pending", "", &format!("读忙闲失败：{e}")).await;
                tokio::time::sleep(Duration::from_millis(1500)).await;
                continue;
            }
        };
        if !v.idle {
            // 放回pending（不消耗它），等下一轮
            q.finish(item.id, "pending", "", "等待对方空闲").await;
            // 间隔模式的最小等待，避免疯狂轮询
            let gap = *interval_hint.lock().await;
            tokio::time::sleep(gap.min(Duration::from_millis(1500))).await;
            continue;
        }

        // 间隔模式：距上次发送不足间隔则等
        if let Some(t) = last_send {
            let gap = *interval_hint.lock().await;
            let el = t.elapsed();
            if el < gap {
                q.finish(item.id, "pending", "", "按间隔等待").await;
                tokio::time::sleep(gap - el).await;
                continue;
            }
        }

        // 真发
        match engine::send_only(&cdp, &item.text, &item.conv_id).await {
            Ok(ev) => {
                q.finish(item.id, "done", &ev, "").await;
                q.bump_round().await;
                last_send = Some(Instant::now());
            }
            Err(e) => {
                let msg = format!("{e}");
                //忙 → 放回重试；其它错 → 标记失败但不阻塞
                if msg.contains("忙") || msg.contains("生成中") {
                    q.finish(item.id, "pending", "", "等待对方空闲").await;
                    tokio::time::sleep(Duration::from_millis(1200)).await;
                } else {
                    q.finish(item.id, "failed", "", &msg).await;
                }
            }
        }
    }
}

fn now_hm() -> String {
    // 极简时间戳：只到秒，够界面展示
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{}", secs % 100000)
}
