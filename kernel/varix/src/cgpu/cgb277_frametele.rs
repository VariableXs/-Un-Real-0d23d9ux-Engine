// SPDX-License-Identifier: Apache-2.0
// Varix Kernel — CGPU · cgb277_frametele
// CGPU-F0277 · 帧级遥测（B08 组 · 四段分解 + 帧节奏 + 超帧归因 + 双向钻取）
//
// 锚点：CGPU-F0277
// 判据原文：帧时间全分解（提交/执行/等待/呈现四段——与 F0264 预算执行对比）；
//          帧节奏指标（帧间隔分布/抖动 stddev——80 帧合同的直接观测量）；
//          超帧事件（>12.5ms 帧逐帧记录+归因标签——哪个账户超用）；
//          帧级↔任务级关联（一帧→任务树钻取）。
//
// 难点分析：
//   1. 80 帧合同 = 12.5ms/帧。超帧检测是「体验守护」的第一道关。
//   2. 超帧归因要把「哪段超了」和「哪个账户超用」关联起来。
//   3. 双向钻取：从帧号查任务列表，从任务 ID 查所在帧。
//
// 纪律约束：
//   - no_std + alloc 三件套
//   - match 臂字符串尾必须加逗号

#![allow(dead_code)]

use alloc::format;
extern crate alloc;
use crate::float::FloatExt;

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use super::cgb276_tasktele::FourPhaseTiming;
use crate::checks::CheckSet;

// ============================================================================
// 一、帧时间四段分解
// ============================================================================

/// 80 帧合同：12.5ms/帧。
pub const FRAME_BUDGET_US: u64 = 12_500;

/// 帧四段耗时。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameTiming {
    /// 提交（微秒）。
    pub submit_us: u64,
    /// 执行（微秒）。
    pub execute_us: u64,
    /// 等待（微秒）。
    pub wait_us: u64,
    /// 呈现（微秒）。
    pub present_us: u64,
}

impl FrameTiming {
    /// 总耗时。
    pub fn total_us(&self) -> u64 {
        self.submit_us + self.execute_us + self.wait_us + self.present_us
    }

    /// 是否超帧。
    pub fn is_over_budget(&self) -> bool {
        self.total_us() > FRAME_BUDGET_US
    }

    /// 超了多少。
    pub fn over_by_us(&self) -> u64 {
        self.total_us().saturating_sub(FRAME_BUDGET_US)
    }

    /// 超帧时最耗时的段。
    pub fn dominant_segment(&self) -> (&'static str, u64) {
        let mut name = "submit";
        let mut val = self.submit_us;
        if self.execute_us > val {
            name = "execute";
            val = self.execute_us;
        }
        if self.wait_us > val {
            name = "wait";
            val = self.wait_us;
        }
        if self.present_us > val {
            name = "present";
            val = self.present_us;
        }
        (name, val)
    }
}

// ============================================================================
// 二、帧节奏指标（间隔分布 + 抖动 stddev）
// ============================================================================

/// 帧节奏指标。
pub struct FrameCadence {
    /// 帧间隔样本（微秒）。
    intervals: Vec<u64>,
}

impl FrameCadence {
    /// 构造。
    pub fn new() -> Self {
        Self {
            intervals: Vec::new(),
        }
    }

    /// 记录一帧间隔。
    pub fn record_interval(&mut self, interval_us: u64) {
        self.intervals.push(interval_us);
    }

    /// 平均间隔。
    pub fn mean(&self) -> f64 {
        if self.intervals.is_empty() {
            return 0.0;
        }
        self.intervals.iter().sum::<u64>() as f64 / self.intervals.len() as f64
    }

    /// 标准差（抖动）。
    pub fn stddev(&self) -> f64 {
        if self.intervals.len() < 2 {
            return 0.0;
        }
        let mean = self.mean();
        let var = self
            .intervals
            .iter()
            .map(|&x| {
                let d = x as f64 - mean;
                d * d
            })
            .sum::<f64>()
            / self.intervals.len() as f64;
        var.m_sqrt()
    }

    /// 最大间隔。
    pub fn max(&self) -> u64 {
        self.intervals.iter().copied().max().unwrap_or(0)
    }

    /// 样本数。
    pub fn count(&self) -> usize {
        self.intervals.len()
    }
}

impl Default for FrameCadence {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// 三、超帧事件（>12.5ms + 归因）
// ============================================================================

/// 超帧事件。
#[derive(Clone, Debug)]
pub struct OverBudgetEvent {
    /// 帧号。
    pub frame: u64,
    /// 帧耗时。
    pub timing: FrameTiming,
    /// 超出量。
    pub over_by: u64,
    /// 归因账户。
    pub attributed_account: String,
    /// 归因描述。
    pub attribution: String,
}

/// 超帧追踪器。
pub struct OverBudgetTracker {
    /// 超帧事件。
    pub events: Vec<OverBudgetEvent>,
    /// 超帧计数。
    pub count: u64,
    /// 连续超帧最高纪录。
    pub max_streak: u64,
    /// 当前连续超帧数。
    current_streak: u64,
}

impl OverBudgetTracker {
    /// 构造。
    pub fn new() -> Self {
        Self {
            events: Vec::new(),
            count: 0,
            max_streak: 0,
            current_streak: 0,
        }
    }

    /// 记录一帧。
    pub fn record_frame(&mut self, frame: u64, timing: FrameTiming, account: &str) {
        if timing.is_over_budget() {
            let (segment, seg_us) = timing.dominant_segment();
            let total = timing.total_us().max(1);
            let pct = seg_us * 100 / total;
            let over_by = timing.over_by_us();
            self.events.push(OverBudgetEvent {
                frame,
                timing,
                over_by,
                attributed_account: account.to_string(),
                attribution: alloc::format!(
                    "帧{}超{}μs（{}段占{}%）归因账户[{}]",
                    frame, over_by, segment, pct, account
                ),
            });
            self.count += 1;
            self.current_streak += 1;
            self.max_streak = self.max_streak.max(self.current_streak);
        } else {
            self.current_streak = 0;
        }
    }

    /// 超帧率。
    pub fn over_budget_rate(&self, total_frames: u64) -> f64 {
        if total_frames == 0 {
            0.0
        } else {
            self.count as f64 / total_frames as f64
        }
    }
}

impl Default for OverBudgetTracker {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// 四、帧级↔任务级双向钻取
// ============================================================================

/// 帧-任务关联表。
pub struct FrameTaskIndex {
    /// 帧 → 任务 ID 列表。
    frame_to_tasks: BTreeMap<u64, Vec<u32>>,
    /// 任务 → 帧号。
    task_to_frame: BTreeMap<u32, u64>,
}

impl FrameTaskIndex {
    /// 构造。
    pub fn new() -> Self {
        Self {
            frame_to_tasks: BTreeMap::new(),
            task_to_frame: BTreeMap::new(),
        }
    }

    /// 记录任务在帧中。
    pub fn record(&mut self, frame: u64, task_id: u32) {
        self.frame_to_tasks.entry(frame).or_default().push(task_id);
        self.task_to_frame.insert(task_id, frame);
    }

    /// 帧 → 任务列表。
    pub fn tasks_in_frame(&self, frame: u64) -> &[u32] {
        self.frame_to_tasks.get(&frame).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// 任务 → 帧号。
    pub fn frame_of_task(&self, task_id: u32) -> Option<u64> {
        self.task_to_frame.get(&task_id).copied()
    }

    /// 帧数。
    pub fn frame_count(&self) -> usize {
        self.frame_to_tasks.len()
    }
}

impl Default for FrameTaskIndex {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// 五、判据：F0277 四条（四段分解、帧间隔分布、超帧归因、双向钻取）
// ============================================================================

/// F0277 判据集。
pub fn run_f0277_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0277");

    // --- 判据 1：四段分解 + 超帧检测 ---
    let normal = FrameTiming {
        submit_us: 1_000,
        execute_us: 8_000,
        wait_us: 1_000,
        present_us: 1_000,
    };
    let over = FrameTiming {
        submit_us: 2_000,
        execute_us: 9_000,
        wait_us: 2_000,
        present_us: 1_000,
    };
    cs.add(
        "帧四段分解 + 超帧检测",
        normal.total_us() == 11_000
            && !normal.is_over_budget()
            && over.is_over_budget()
            && over.over_by_us() == 1_500,
        "正常11ms/超帧14ms（超1.5ms）",
    );

    // --- 判据 2：帧间隔分布 + 抖动 ---
    let mut cadence = FrameCadence::new();
    for i in 0..80 {
        // 80 帧合同：间隔约 12.5ms，带抖动。
        let interval = 12_500 + (i % 7) * 100 - 300;
        cadence.record_interval(interval);
    }
    let mean = cadence.mean();
    let stddev = cadence.stddev();
    cs.add(
        "帧间隔分布 + 抖动 stddev",
        cadence.count() == 80 && mean > 12_000.0 && mean < 13_000.0 && stddev > 0.0,
        alloc::format!("mean={:.0}μs stddev={:.1}μs", mean, stddev).leak(),
    );

    // --- 判据 3：超帧归因 ---
    let mut tracker = OverBudgetTracker::new();
    for i in 0..10 {
        let timing = if i % 3 == 0 {
            FrameTiming { submit_us: 3_000, execute_us: 8_000, wait_us: 3_000, present_us: 1_000 }
        } else {
            normal
        };
        tracker.record_frame(i, timing, "account_A");
    }
    cs.add(
        "超帧归因",
        tracker.count == 4 && tracker.events[0].attribution.contains("account_A"),
        alloc::format!("4/10 帧超预算，归因含账户名").leak(),
    );

    // --- 判据 4：双向钻取 ---
    let mut index = FrameTaskIndex::new();
    index.record(1, 101);
    index.record(1, 102);
    index.record(2, 103);
    let tasks_f1 = index.tasks_in_frame(1);
    let frame_of_103 = index.frame_of_task(103);
    cs.add(
        "帧↔任务双向钻取",
        tasks_f1.len() == 2 && frame_of_103 == Some(2),
        "帧1→2任务/任务103→帧2",
    );

    // --- 附加：dominant_segment ---
    let (seg, _) = over.dominant_segment();
    cs.add(
        "dominant_segment",
        seg == "execute",
        "执行段最耗时",
    );

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f0277_all_pass() {
        let cs = run_f0277_checks();
        assert!(cs.all_passed(), "F0277 判据未全绿: {}", cs.red_items_text());
    }

    #[test]
    fn over_budget_detected() {
        let t = FrameTiming { submit_us: 0, execute_us: 13_000, wait_us: 0, present_us: 0 };
        assert!(t.is_over_budget());
        assert_eq!(t.over_by_us(), 500);
    }

    #[test]
    fn cadence_stddev() {
        let mut c = FrameCadence::new();
        for _ in 0..10 {
            c.record_interval(12_500);
        }
        assert_eq!(c.stddev(), 0.0);
    }
}
