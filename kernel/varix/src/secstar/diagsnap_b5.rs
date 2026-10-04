//! F174 诊断快照键 · 批次五深化（secstar · G-G-04）。
//!
//! 批次五功能面（达成率已 92%——小批收尾：节流与账窗交互）：
//! - [`ThrottleGate`]：连按节流门——10s 窗内第二次按键合并（节流是
//!   合并不是丢弃：窗口满后一次补拍——既有语义的机械化）；
//! - [`ledger_window_consistent`]：30s 账窗与节流窗一致性——节流窗
//!   恒小于等于账窗（数据面包含关系：补拍永远有料可拍）；
//! - [`capture_ledger`]：捕获账——本次快照覆盖的账本窗口字节数。
//!
//! 零堆纪律：状态字段，无 alloc。

use super::diagsnap::{LEDGER_WINDOW_MS, THROTTLE_WINDOW_MS};
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 节流门
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThrottleVerdict {
    /// 放行采集。
    Capture,
    /// 节流合并（记录合并——窗口满后一次补拍）。
    Merged,
}

pub struct ThrottleGate {
    last_capture_ms: u64,
    /// 首按武装旗（未采集过 → 首按必放行——零时刻哨兵问题的构造解）。
    armed: bool,
    pub merged_count: u32,
}

impl ThrottleGate {
    pub const fn new() -> ThrottleGate {
        ThrottleGate { last_capture_ms: 0, armed: false, merged_count: 0 }
    }

    /// 按键裁决：未武装 → 放行；距上次采集 ≥10s → 放行；否则合并。
    pub fn key(&mut self, now_ms: u64) -> ThrottleVerdict {
        if !self.armed || now_ms.saturating_sub(self.last_capture_ms) >= THROTTLE_WINDOW_MS {
            self.armed = true;
            self.last_capture_ms = now_ms;
            ThrottleVerdict::Capture
        } else {
            self.merged_count += 1;
            ThrottleVerdict::Merged
        }
    }

    pub fn last_capture(&self) -> u64 {
        self.last_capture_ms
    }
}

// ---------------------------------------------------------------------------
// 账窗包含关系
// ---------------------------------------------------------------------------

/// 账窗与节流窗一致性：节流窗 ≤ 账窗（补拍时 10s 内的变化必然在
/// 30s 账窗里有料——数据面包含关系的机械判定）。
pub fn ledger_window_consistent() -> bool {
    THROTTLE_WINDOW_MS <= LEDGER_WINDOW_MS
}

/// 捕获账：本次快照覆盖的账窗字节数预估（样本 × 每样本字节）。
pub fn capture_ledger(samples_in_window: usize, bytes_per_sample: usize) -> usize {
    samples_in_window.saturating_mul(bytes_per_sample)
}

// ---------------------------------------------------------------------------
// 批次五自检
// ---------------------------------------------------------------------------

#[inline(never)]
pub fn run_diagsnap_b5_checks() -> CheckSet {
    let mut cs = CheckSet::new("F174-b5");

    // 1) 节流放行：首次按键 → 采集（窗口从零起算）。
    let mut g = ThrottleGate::new();
    let first = g.key(1_000);
    cs.add("throttle_first_captures", first == ThrottleVerdict::Capture && g.last_capture() == 1_000, "");

    // 2) 节流合并：10s 内再按 → 合并计数（不重复采集）。
    let merged = g.key(5_000);
    cs.add(
        "throttle_merges",
        merged == ThrottleVerdict::Merged && g.merged_count == 1 && g.last_capture() == 1_000,
        "",
    );

    // 3) 窗口满补拍：满 10s 后再按 → 放行（合并的补拍出口存在）。
    let recapture = g.key(11_000);
    cs.add("throttle_recaptures", recapture == ThrottleVerdict::Capture && g.last_capture() == 11_000, "");

    // 4) 零时刻边界：首按 now=0 也放行（armed 旗构造解——不靠哨兵值）。
    let mut g2 = ThrottleGate::new();
    let at_zero = g2.key(0);
    let merged_next = g2.key(THROTTLE_WINDOW_MS / 2);
    let recapture = g2.key(THROTTLE_WINDOW_MS);
    cs.add(
        "throttle_zero_edge_honest",
        at_zero == ThrottleVerdict::Capture
            && merged_next == ThrottleVerdict::Merged
            && recapture == ThrottleVerdict::Capture,
        "",
    );

    // 5) 账窗包含：10s ≤ 30s（补拍永远有料——包含关系恒真）。
    cs.add("ledger_window_contains", ledger_window_consistent(), "");

    // 6) 捕获账：30s 窗 60 样本 × 48B = 2880B（体积预估面）。
    cs.add("capture_ledger_bytes", capture_ledger(60, 48) == 2_880 && capture_ledger(0, 48) == 0, "");

    // 7) 主册常量贯通：节流 10s / 账窗 30s 一处一事实。
    cs.add("consts_aligned", THROTTLE_WINDOW_MS == 10_000 && LEDGER_WINDOW_MS == 30_000, "");

    cs
}

// ---------------------------------------------------------------------------
// 宿主单测（批次五）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests_b5 {
    use super::*;

    #[test]
    fn throttle_rapid_burst_one_capture() {
        // 连按风暴：首按采集、其余 9 次全合并（节流的全部意义）。
        let mut g = ThrottleGate::new();
        let mut captures = 0;
        for i in 0..10u64 {
            if g.key(i * 100) == ThrottleVerdict::Capture {
                captures += 1;
            }
        }
        assert_eq!(captures, 1);
        assert_eq!(g.merged_count, 9);
    }

    #[test]
    fn throttle_long_term_cadence() {
        // 长跑节奏：每 10s 一采、中间全合并（稳态节拍）。
        let mut g = ThrottleGate::new();
        let mut captures = 0;
        for i in 0..100u64 {
            if g.key(i * 1_000) == ThrottleVerdict::Capture {
                captures += 1;
            }
        }
        assert_eq!(captures, 10); // 0s 首采（armed）+ 10s..=90s 每 10s 一采 = 10
    }
}
