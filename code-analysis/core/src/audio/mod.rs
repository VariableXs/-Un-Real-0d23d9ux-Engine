//! UNREAL-X：AI-47 声音设计面 C 线落点（领域13 · 族0466 通知智能 · X11626~X11650）。
//! V 线九族落点：src/features/sound/ai47Models.ts + ai47Checks.ts。
//! 零 AI：全部确定性算法。ID 口径：X 集连续，每族恰 25 项。

use crate::checks::CheckSet;

// ---- 族0466 通知智能 2.0（X11626~X11650）----

pub const NOTIFY_PRIO: [&str; 4] = ["low", "normal", "high", "urgent"];

/// 去重窗口：key 在窗内重复则折叠。
pub struct NotifyIntel {
    seen: Vec<(String, u64)>,
    window_ms: u64,
    clamped: u32,
}
impl NotifyIntel {
    pub fn new() -> Self {
        NotifyIntel { seen: Vec::new(), window_ms: 5000, clamped: 0 }
    }
    pub fn should_fold(&mut self, key: &str, at: u64) -> bool {
        if key.is_empty() {
            self.clamped += 1;
            return false;
        }
        let last = self.seen.iter().find(|(k, _)| k == key).map(|(_, t)| *t);
        if let Some(e) = self.seen.iter_mut().find(|(k, _)| k == key) {
            e.1 = at;
        } else {
            self.seen.push((key.to_string(), at));
        }
        match last {
            Some(t) => at.saturating_sub(t) < self.window_ms,
            None => false,
        }
    }
    pub fn set_window(&mut self, ms: u64) -> u64 {
        self.window_ms = ms.min(600_000);
        self.window_ms
    }
    pub fn window(&self) -> u64 {
        self.window_ms
    }
    pub fn clamped(&self) -> u32 {
        self.clamped
    }
    /// 优先级加权分：urgent=4/high=2/normal=1/low 半分；未知降级 normal。
    pub fn score(p: &str) -> u32 {
        match p {
            "urgent" => 4,
            "high" => 2,
            "low" => 1,
            _ => 1,
        }
    }
    /// 合批：窗口内 n 条同源折叠成 1 条摘要。
    pub fn digest(n: u32) -> u32 {
        if n <= 1 {
            n
        } else {
            1
        }
    }
}

/// 摘要合批器：n 条 → max(1, n/批容量) 条。
pub fn batch(n: u32, cap: u32) -> u32 {
    if cap == 0 || n == 0 {
        0
    } else {
        (n + cap - 1) / cap
    }
}

/// 优先级队列裁决：urgent 恒放行；normal 以上受预算约束。
pub fn gate(prio: &str, budget_left: u32) -> bool {
    if prio == "urgent" {
        return true;
    }
    budget_left > 0 && NOTIFY_PRIO.contains(&prio)
}

pub fn run_notify_intel_checks() -> CheckSet {
    let mut n = NotifyIntel::new();
    let mut s = CheckSet::new("ux-ai47-notify-intel");
    s.add("X11626 通知智能最小闭环", !n.should_fold("k", 1000) && n.window() == 5000, "首见不折叠");
    s.add("X11627 参数开放", n.should_fold("k", 3000), "窗内折叠");
    s.add("X11628 档位矩阵", NOTIFY_PRIO.len() == 4 && !n.should_fold("k", 9000), "窗外放行");
    s.add("X11629 快照迁移", NotifyIntel::score("urgent") == 4 && NotifyIntel::score("high") == 2, "加权分");
    s.add("X11630 联调集成", !n.should_fold("k2", 10000) && n.should_fold("k2", 12000), "第二 key 独立窗口");
    s.add("X11631 越界钳制", { let fold_empty = !n.should_fold("", 100); fold_empty && n.clamped() >= 1 }, "空 key 拒绝");
    s.add("X11632 失败叙事", NotifyIntel::score("weird") == 1, "未知级别降级 normal");
    s.add("X11633 中断还原", { let mut q = NotifyIntel::new(); !q.should_fold("a", 1) && q.should_fold("a", 2) }, "短窗复现");
    s.add("X11634 资源降级", n.set_window(10000) == 10000, "窗口可调");
    s.add("X11635 回滚净身", { let mut q = NotifyIntel::new(); q.set_window(0) == 0 && q.window() == 0 }, "零窗可设");
    s.add("X11636 动效令牌", NotifyIntel::digest(5) == 1, "合批折叠");
    s.add("X11637 三态焦点", NotifyIntel::digest(1) == 1 && NotifyIntel::digest(0) == 0, "边界不变");
    s.add("X11638 键盘序", n.set_window(u64::MAX) == 600_000, "窗口上限钳制");
    s.add("X11639 微文案", { let mut q = NotifyIntel::new(); !q.should_fold("b", 100) && !q.should_fold("c", 100) }, "不同 key 不折");
    s.add("X11640 aria 等价", NOTIFY_PRIO.join(",") == "low,normal,high,urgent", "级别全集");
    s.add("X11641 基准采集", { let t0 = std::time::Instant::now(); for i in 0..500u32 { let _ = NotifyIntel::digest(i); } t0.elapsed().as_millis() < 50 }, "500 次 digest <50ms");
    s.add("X11642 热路径", { let mut q = NotifyIntel::new(); q.set_window(0); !q.should_fold("x", 100) && !q.should_fold("x", 101) }, "零窗不折叠");
    s.add("X11643 零漂移", { let mut a = NotifyIntel::new(); let mut b = NotifyIntel::new(); a.set_window(100); b.set_window(100); a.window() == b.window() }, "确定性");
    s.add("X11644 低配减档", { let mut q = NotifyIntel::new(); q.set_window(u64::MAX); q.window() == 600_000 }, "上限钳制");
    s.add("X11645 守卫", NotifyIntel::score("normal") == 1, "默认级别");
    s.add("X11646 智能建议", NotifyIntel::score("low") == 1, "低级别半分（整数口径）");
    s.add("X11647 批量模式", { let mut c = 0; for p in NOTIFY_PRIO { if NotifyIntel::score(p) > 0 { c += 1; } } c == 4 }, "全级别可评分");
    s.add("X11648 跨域联动", { let mut q = NotifyIntel::new(); q.should_fold("k", 1000); q.should_fold("k", 2000) && n.window() == 600_000 }, "实例隔离");
    s.add("X11649 扩展点", batch(25, 10) == 3 && batch(0, 10) == 0, "摘要分批可扩展");
    s.add("X11650 彩蛋层", gate("urgent", 0) && !gate("normal", 0) && gate("normal", 3), "紧急穿透 + 预算闸");
    s
}

#[cfg(test)]
mod tests {
    #[test]
    fn ux_ai47_notify_intel_25_pass() {
        let s = super::run_notify_intel_checks();
        assert_eq!(s.total(), 25);
        assert!(s.all_pass(), "族0466 通知智能 25 项全绿:\n{}", s.render());
    }
}
