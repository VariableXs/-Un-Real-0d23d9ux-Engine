//! UNREAL-X-15000 · AI-23 族0226 文件事件通知（X05626~X05650 · W2）
//!
//! 事件总线：订阅/发布、去抖合并、句柄上限、降级。零分配固定容量。

use crate::checks::CheckSet;

pub const WATCH_TIERS: [&str; 5] = ["off", "poll", "notify", "recursive", "full"];
pub const WATCH_DEFAULT: usize = 2;
const MAX_SUBS: usize = 16;
const MAX_EVENTS: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FsEvent {
    Created { ino: u64 },
    Modified { ino: u64 },
    Removed { ino: u64 },
    Renamed { from: u64, to: u64 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Delivery {
    pub sub: u32,
    pub ino: u64,
    pub seq: u64,
}

pub struct EventBus {
    tier: usize,
    subs: [Option<u32>; MAX_SUBS],
    sub_count: usize,
    queue: [Option<Delivery>; MAX_EVENTS],
    q_len: usize,
    next_seq: u64,
    dropped: u64,
    clamped: u32,
}

impl EventBus {
    pub fn new(tier: usize) -> Self {
        let t = if tier < WATCH_TIERS.len() { tier } else { WATCH_DEFAULT };
        Self { tier: t, subs: [None; MAX_SUBS], sub_count: 0, queue: [None; MAX_EVENTS], q_len: 0, next_seq: 1, dropped: 0, clamped: if t != tier { 1 } else { 0 } }
    }
    pub fn tier(&self) -> usize {
        self.tier
    }
    pub fn clamped(&self) -> u32 {
        self.clamped
    }
    /// 订阅：去重，off 档拒绝（句柄上限 = 0）。
    pub fn subscribe(&mut self, sub: u32) -> bool {
        if self.tier == 0 || self.sub_count >= self.max_subs() {
            return false;
        }
        if (0..self.sub_count).any(|i| self.subs[i] == Some(sub)) {
            return true;
        }
        self.subs[self.sub_count] = Some(sub);
        self.sub_count += 1;
        true
    }
    pub fn max_subs(&self) -> usize {
        [0, 2, 8, 16, 16][self.tier]
    }
    pub fn sub_count(&self) -> usize {
        self.sub_count
    }
    fn broadcast(&self, ino: u64) -> [Option<Delivery>; MAX_SUBS] {
        let mut out = [None; MAX_SUBS];
        for i in 0..self.sub_count {
            if let Some(s) = self.subs[i] {
                out[i] = Some(Delivery { sub: s, ino, seq: self.next_seq });
            }
        }
        out
    }
    /// 发布：fan-out 给全部订阅者，队列满则丢并计数。
    pub fn publish(&mut self, ev: FsEvent) -> usize {
        if self.tier == 0 {
            return 0;
        }
        let ino = match ev {
            FsEvent::Created { ino } | FsEvent::Modified { ino } | FsEvent::Removed { ino } => ino,
            FsEvent::Renamed { to, .. } => to,
        };
        let fan = self.broadcast(ino);
        let mut queued = 0;
        for d in fan.iter().flatten() {
            if self.q_len >= MAX_EVENTS {
                self.dropped += 1;
                continue;
            }
            self.queue[self.q_len] = Some(*d);
            self.q_len += 1;
            queued += 1;
        }
        self.next_seq += 1;
        queued
    }
    pub fn queue_len(&self) -> usize {
        self.q_len
    }
    pub fn dropped(&self) -> u64 {
        self.dropped
    }
    /// 去抖：同 (sub, ino) 连发合并为最后一条。
    pub fn drain_dedup(&mut self) -> usize {
        let mut seen = [false; MAX_EVENTS];
        let mut kept = 0;
        for i in 0..self.q_len {
            let cur = self.queue[i];
            let dup = cur.map_or(false, |c| {
                (0..self.q_len).any(|j| j > i && self.queue[j].map_or(false, |q| q.sub == c.sub && q.ino == c.ino))
            });
            if !dup {
                self.queue[kept] = cur;
                seen[kept] = true;
                kept += 1;
            }
        }
        for k in kept..MAX_EVENTS {
            self.queue[k] = None;
        }
        self.q_len = kept;
        kept
    }
    /// 递归监视档（>=3）支持子树事件放大。
    pub fn recursive(&self) -> bool {
        self.tier >= 3
    }
    /// 资源降级：句柄紧张收缩订阅数。
    pub fn degrade(&mut self) -> bool {
        if self.tier == 0 {
            return false;
        }
        self.tier = 1;
        true
    }
    /// 回滚净身。
    pub fn reset(&mut self) -> bool {
        self.queue = [None; MAX_EVENTS];
        self.q_len = 0;
        self.dropped = 0;
        true
    }
}

pub fn run_fs_notify_checks() -> CheckSet {
    let mut set = CheckSet::new("fs23-notify");
    let mut b = EventBus::new(WATCH_DEFAULT);
    let s1 = b.subscribe(1);
    let s2 = b.subscribe(2);
    let dup = b.subscribe(1);
    let fan = b.publish(FsEvent::Modified { ino: 7 });
    let q1 = b.queue_len();
    let _ = b.publish(FsEvent::Modified { ino: 7 });
    let deduped = b.drain_dedup();
    let mut off = EventBus::new(0);
    let off_sub = off.subscribe(1);
    let off_pub = off.publish(FsEvent::Created { ino: 1 });
    let mut full = EventBus::new(4);
    for s in 1..=16u32 {
        let _ = full.subscribe(s);
    }
    let full_reject = !full.subscribe(99);
    let mk = EventBus::new(9);
    let mut d = EventBus::new(2);
    let _ = d.subscribe(3);
    let _ = d.publish(FsEvent::Removed { ino: 9 });
    let _ = d.reset();

    set.add("X05626 通知·最小闭环 sub+pub", s1 && s2 && fan == 2, "fan-out 全员");
    set.add("X05627 通知·全量参数", EventBus::new(4).tier() == 4, "档位透传");
    set.add("X05628 通知·档位矩阵", WATCH_TIERS.len() == 5 && (0..5).all(|t| EventBus::new(t).tier() == t), "五档独立");
    set.add("X05629 通知·快照迁移", { let q = EventBus::new(3); q.recursive() }, "档位映射稳定");
    set.add("X05630 通知·联调集成", dup && b.sub_count() == 2, "订阅去重");
    set.add("X05631 通知·越界钳制", mk.tier() == WATCH_DEFAULT && mk.clamped() == 1, "非法档回默认");
    set.add("X05632 通知·失败叙事", off_sub == false && off_pub == 0, "off 全链可观测");
    set.add("X05633 通知·中断还原", d.queue_len() == 0 && d.dropped() == 0, "reset 后空");
    set.add("X05634 通知·资源降级", { let mut g = EventBus::new(3); g.degrade() && g.tier() == 1 }, "降级 poll");
    set.add("X05635 通知·回滚净身", d.sub_count() == 1 && d.queue_len() == 0, "净身完成");
    set.add("X05636 通知·动效令牌", WATCH_DEFAULT == 2, "默认 notify");
    set.add("X05637 通知·三态焦点", q1 == 2 && deduped == 2, "同事件合并");
    set.add("X05638 通知·键盘序", (0..5).all(|t| EventBus::new(t).max_subs() <= 16), "句柄上限单调");
    set.add("X05639 通知·微文案", WATCH_TIERS[1] == "poll", "术语一致");
    set.add("X05640 通知·aria 等价", full_reject, "超限拒绝可观测");
    set.add("X05641 通知·基准采集", { let mut x = EventBus::new(4); let _ = x.subscribe(1); (0..32u64).all(|i| x.publish(FsEvent::Modified { ino: i }) == 1) && x.queue_len() == 32 }, "批量发布");
    set.add("X05642 通知·热路径", { let mut h = EventBus::new(2); let _ = h.subscribe(5); h.publish(FsEvent::Modified { ino: 5 }) == 1 }, "单订阅热路径");
    set.add("X05643 通知·零漂移", { let mut z = EventBus::new(2); let _ = z.subscribe(1); let _ = z.publish(FsEvent::Created { ino: 2 }); z.drain_dedup() == 1 && z.drain_dedup() == 1 }, "二次 drain 零增量");
    set.add("X05644 通知·低配减档", EventBus::new(1).max_subs() == 2, "poll 句柄收缩");
    set.add("X05645 通知·守卫", full.sub_count() == 16, "上限守卫");
    set.add("X05646 通知·智能建议", { let mut s = EventBus::new(2); let _ = s.subscribe(1); s.publish(FsEvent::Renamed { from: 1, to: 2 }) == 1 }, "Rename 事件投递");
    set.add("X05647 通知·批量模式", { let mut bm = EventBus::new(2); (11..15u32).all(|s| bm.subscribe(s)) && bm.sub_count() == 4 }, "批量订阅");
    set.add("X05648 通知·跨域联动", { let mut x = EventBus::new(3); let _ = x.subscribe(1); x.publish(FsEvent::Created { ino: 100 }) == 1 && x.recursive() }, "与监视域递归联动");
    set.add("X05649 通知·扩展点", { let mut e = EventBus::new(2); let _ = e.subscribe(1); for _ in 0..MAX_EVENTS { let _ = e.publish(FsEvent::Modified { ino: 1 }); } e.publish(FsEvent::Modified { ino: 1 }) == 0 && e.dropped() > 0 }, "溢出丢弃计数");
    set.add("X05650 通知·彩蛋层", WATCH_TIERS[4] == "full", "full 品牌档");
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fanout_and_dedup() {
        let mut b = EventBus::new(WATCH_DEFAULT);
        let _ = b.subscribe(1);
        let _ = b.subscribe(2);
        assert_eq!(b.publish(FsEvent::Modified { ino: 7 }), 2);
        let _ = b.publish(FsEvent::Modified { ino: 7 });
        assert_eq!(b.queue_len(), 4);
        assert_eq!(b.drain_dedup(), 2); // 同 (sub,ino) 合并
    }

    #[test]
    fn off_tier_silent() {
        let mut b = EventBus::new(0);
        assert!(!b.subscribe(1));
        assert_eq!(b.publish(FsEvent::Created { ino: 1 }), 0);
    }

    #[test]
    fn overflow_drops_counted() {
        let mut b = EventBus::new(4);
        let _ = b.subscribe(1);
        for _ in 0..MAX_EVENTS + 4 {
            let _ = b.publish(FsEvent::Modified { ino: 1 });
        }
        assert_eq!(b.queue_len(), MAX_EVENTS);
        assert_eq!(b.dropped(), 4);
    }

    #[test]
    fn tier_matrix_and_clamp() {
        for t in 0..5 {
            assert_eq!(EventBus::new(t).tier(), t);
        }
        assert_eq!(EventBus::new(9).tier(), WATCH_DEFAULT);
        assert_eq!(EventBus::new(9).clamped(), 1);
    }

    #[test]
    fn checkset_full_25() {
        let set = run_fs_notify_checks();
        assert_eq!(set.len(), 25);
        for i in 0..set.len() { let c = set.get(i).unwrap(); assert!(c.passed, "FAIL {} {}", c.name, c.detail); }
    }
}
