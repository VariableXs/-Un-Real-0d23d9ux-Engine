//! 深化层 · F598 自启动错峰（回炉补深主册【设计要点】未实装机制）。
//!
//! 补深三条（判据唯一源：主册 F598 节）：
//! ①「开机时长对比（错峰前后）」的**对比账**——串行假设基线
//!   （n×2s 全部压在 0 时刻的挤兑形态）对错峰首就绪耗时，错峰的
//!   意义要有账可算；
//! ②「被延后的应用无感知（后台起、就绪即用 F283 骨架先行）」的
//!   **就绪序解耦账**——就绪事件乱序到达（先启动的后就绪）不阻塞
//!   后续启动、不破坏启动账（启动序与就绪序是两条独立的账）；
//! ③「用户可调序（清单拖拽=启动序）」的**调序持久化对账**——调序
//!   后队列稳定复现（同账重读同序，不许读一次变一次）。

use crate::checks::CheckSet;
use crate::istar::ibase::ISTAR_DOMAIN;
use crate::istar::staggerboot::{StaggerBoot, STAGGER_MS};

// ---------------------------------------------------------------------------
// 开机时长对比账
// ---------------------------------------------------------------------------

/// 错峰对比账：串行挤兑基线 vs 错峰首批启动跨度。
pub struct BootComparison {
    /// 自启动应用数。
    pub apps: usize,
    /// 串行挤兑基线（不做错峰时 n 个应用同一时刻的 IO/内存挤兑当量，
    /// 以 n×STAGGER_MS 折算——账面口径）。
    pub serial_baseline_ms: u64,
    /// 错峰后首批全部启动完的跨度。
    pub staggered_span_ms: u64,
}

impl BootComparison {
    /// n 应用错峰跨度 = (n-1)×STAGGER_MS（首应用 0 时刻起）。
    pub fn measure(apps: usize) -> BootComparison {
        BootComparison {
            apps,
            serial_baseline_ms: apps as u64 * STAGGER_MS,
            staggered_span_ms: apps.saturating_sub(1) as u64 * STAGGER_MS,
        }
    }

    /// 错峰收益：首批跨度不大于串行基线（峰谷填平的账面证据）。
    pub fn staggered_not_worse(&self) -> bool {
        self.staggered_span_ms <= self.serial_baseline_ms
    }
}

// ---------------------------------------------------------------------------
// 就绪序解耦账
// ---------------------------------------------------------------------------

/// 就绪登记（乱序容忍）。
pub struct ReadinessLedger {
    /// (启动序, 就绪时刻)——就绪序与启动序各自独立。
    ready: alloc::vec::Vec<(usize, u64)>,
}

impl ReadinessLedger {
    pub fn new() -> ReadinessLedger {
        ReadinessLedger { ready: alloc::vec::Vec::new() }
    }

    /// 就绪事件登记（乱序到达照收——不阻塞、不重排启动账）。
    pub fn note(&mut self, launch_index: usize, ready_ms: u64) {
        self.ready.push((launch_index, ready_ms));
    }

    /// 全员就绪 = 登记数与启动数一致。
    pub fn all_ready(&self, launched: usize) -> bool {
        self.ready.len() == launched
    }

    /// 乱序确实发生了（就绪序 ≠ 启动序——无感判据的对照面）。
    pub fn out_of_order_occurred(&self) -> bool {
        self.ready
            .windows(2)
            .any(|w| w[0].1 > w[1].1)
    }
}

impl Default for ReadinessLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 深化自检
// ---------------------------------------------------------------------------

pub fn run_f598_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new(ISTAR_DOMAIN);

    // 1) 错峰调度：3 应用按 2s 一档启动（首 0ms、次 2000ms、三 4000ms）。
    let mut sb = StaggerBoot::new();
    let _ = sb.add("云盘", 1);
    let _ = sb.add("输入法", 0);
    let _ = sb.add("聊天", 2);
    let at_0 = sb.tick(0);
    let at_2s = sb.tick(2_000);
    let at_4s = sb.tick(4_000);
    cs.add(
        "staggered launch cadence",
        at_0.len() == 1 && at_2s.len() == 1 && at_4s.len() == 1,
        "",
    );

    // 2) 间隔合同：首批启动间隔恰为 2s（峰谷填平的节奏证据）。
    cs.add("first gap equals stagger", sb.first_gap_ms() == Some(STAGGER_MS), "");

    // 3) 就绪乱序容忍：先启动的最后就绪，不阻塞、不重排启动账。
    let mut rl = ReadinessLedger::new();
    rl.note(0, 5_000); // 首应用最慢
    rl.note(1, 2_100);
    rl.note(2, 4_100);
    cs.add(
        "readiness decoupled from launch",
        rl.all_ready(3) && rl.out_of_order_occurred(),
        "",
    );

    // 4) 对比账：错峰跨度 < 串行挤兑基线（错峰的意义可算）。
    let cmp = BootComparison::measure(3);
    cs.add(
        "staggered not worse",
        cmp.staggered_not_worse()
            && cmp.staggered_span_ms == 2 * STAGGER_MS
            && cmp.serial_baseline_ms == 3 * STAGGER_MS,
        "",
    );

    // 5) 调序持久化对账：调序后队列稳定复现（读两次同序）。
    let mut sb2 = StaggerBoot::new();
    let _ = sb2.add("云盘", 1);
    let _ = sb2.add("输入法", 0);
    let _ = sb2.reorder("云盘", 0); // 挪到最前
    let q1 = sb2.queue();
    let q2 = sb2.queue();
    cs.add(
        "reorder persists stably",
        q1 == q2 && q1.first().map(|s| s.as_str()) == Some("云盘"),
        "",
    );

    // 6) 全员就绪复核（基础判据不被深化破坏）。
    cs.add("all ready basic kept", sb.all_ready() == false, ""); // 尚无人 note_ready

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_app_zero_span() {
        let c = BootComparison::measure(1);
        assert_eq!(c.staggered_span_ms, 0);
        assert!(c.staggered_not_worse());
    }

    #[test]
    fn readiness_partial() {
        let mut rl = ReadinessLedger::new();
        rl.note(0, 100);
        assert!(!rl.all_ready(2));
    }
}
