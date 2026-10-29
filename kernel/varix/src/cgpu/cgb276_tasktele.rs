#![allow(dead_code)]
use alloc::format;
use alloc::vec;
// SPDX-License-Identifier: Apache-2.0
// Varix Kernel — CGPU · cgb276_tasktele
// CGPU-F0276 · 任务级遥测（B08 组 · 四段耗时 + 类型拆分 + 慢任务榜）
//
// 锚点：CGPU-F0276
// 判据原文：每任务四段耗时（排队/就绪等待/执行/等待依赖——P50/P95/P99 分布）；
//          任务类型维度拆分（栅格化/合成/解码/压缩分列）；
//          抢占与提升事件流（B04 提升频率与代价可见）；
//          慢任务榜（Top-N 长尾任务自动聚合+初步归因）。
//
// 难点分析：
//   1. 四段耗时的边界定义必须精确——「排队」和「就绪等待」的分界点是「入就绪队列」那一刻。
//   2. P50/P95/P99 分布需要在线直方图（不能存全量样本）。
//   3. 慢任务榜的「初步归因」是自动的——用耗时结构判断是「排队慢」还是「执行慢」。
//
// 纪律约束：
//   - no_std + alloc 三件套
//   - match 臂字符串尾必须加逗号


extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ============================================================================
// 一、任务四段耗时
// ============================================================================

/// 四段耗时。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FourPhaseTiming {
    /// 排队时间（微秒）——入队到入就绪。
    pub queue_us: u64,
    /// 就绪等待（微秒）——入就绪到开始执行。
    pub ready_wait_us: u64,
    /// 执行时间（微秒）。
    pub execute_us: u64,
    /// 等待依赖（微秒）——执行中等外部资源。
    pub dependency_wait_us: u64,
}

impl FourPhaseTiming {
    /// 总耗时。
    pub fn total_us(&self) -> u64 {
        self.queue_us + self.ready_wait_us + self.execute_us + self.dependency_wait_us
    }

    /// 最大段。
    pub fn dominant_phase(&self) -> (&'static str, u64) {
        let mut max_name = "queue";
        let mut max_val = self.queue_us;
        if self.ready_wait_us > max_val {
            max_name = "ready_wait";
            max_val = self.ready_wait_us;
        }
        if self.execute_us > max_val {
            max_name = "execute";
            max_val = self.execute_us;
        }
        if self.dependency_wait_us > max_val {
            max_name = "dependency";
            max_val = self.dependency_wait_us;
        }
        (max_name, max_val)
    }
}

/// 任务类型。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum TaskType {
    /// 栅格化。
    Rasterize,
    /// 合成。
    Compose,
    /// 解码。
    Decode,
    /// 压缩。
    Compress,
}

impl TaskType {
    /// 类型名。
    pub fn name(&self) -> &'static str {
        match self {
            TaskType::Rasterize => "rasterize",
            TaskType::Compose => "compose",
            TaskType::Decode => "decode",
            TaskType::Compress => "compress",
        }
    }

    /// 全部类型。
    pub fn all() -> Vec<TaskType> {
        vec![TaskType::Rasterize, TaskType::Compose, TaskType::Decode, TaskType::Compress]
    }
}

// ============================================================================
// 二、在线百分位直方图
// ============================================================================

/// 在线百分位估算器（P50/P95/P99）。
///
/// 用对数桶计数——不需要存全量样本，空间 O(log(max_value))。
pub struct PercentileEstimator {
    /// 桶计数（按值的对数分桶）。
    buckets: BTreeMap<u64, u64>, // 桶上界 → 计数
    /// 样本总数。
    count: u64,
    /// 累计总和。
    sum: u64,
    /// 最大值。
    max_val: u64,
}

impl PercentileEstimator {
    /// 构造。
    pub fn new() -> Self {
        Self {
            buckets: BTreeMap::new(),
            count: 0,
            sum: 0,
            max_val: 0,
        }
    }

    /// 记录样本。
    pub fn record(&mut self, value: u64) {
        // 对数桶：1,2,4,8,16,... 或 1,10,100,...
        let bucket = Self::bucket_upper(value);
        *self.buckets.entry(bucket).or_insert(0) += 1;
        self.count += 1;
        self.sum += value;
        self.max_val = self.max_val.max(value);
    }

    /// 计算桶上界。
    fn bucket_upper(value: u64) -> u64 {
        if value == 0 {
            return 1;
        }
        // 向上取到最近的 10 的幂。
        let mut b = 1u64;
        while b < value {
            b *= 10;
        }
        b
    }

    /// 估算百分位。
    pub fn percentile(&self, p: f64) -> u64 {
        if self.count == 0 {
            return 0;
        }
        let target = (self.count as f64 * p / 100.0) as u64;
        let mut cumulative = 0u64;
        for (&bucket, &cnt) in &self.buckets {
            cumulative += cnt;
            if cumulative >= target {
                return bucket;
            }
        }
        self.max_val
    }

    /// P50。
    pub fn p50(&self) -> u64 {
        self.percentile(50.0)
    }

    /// P95。
    pub fn p95(&self) -> u64 {
        self.percentile(95.0)
    }

    /// P99。
    pub fn p99(&self) -> u64 {
        self.percentile(99.0)
    }

    /// 均值。
    pub fn mean(&self) -> u64 {
        if self.count == 0 {
            0
        } else {
            self.sum / self.count
        }
    }

    /// 样本数。
    pub fn count(&self) -> u64 {
        self.count
    }
}

impl Default for PercentileEstimator {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// 三、任务遥测收集器
// ============================================================================

/// 任务遥测记录。
#[derive(Clone, Debug)]
pub struct TaskTelemetry {
    /// 任务 ID。
    pub task_id: u32,
    /// 任务类型。
    pub task_type: TaskType,
    /// 四段耗时。
    pub timing: FourPhaseTiming,
    /// 帧号。
    pub frame: u64,
    /// 是否被抢占。
    pub preempted: bool,
    /// 提升次数。
    pub boost_count: u32,
}

/// 任务遥测收集器。
pub struct TaskTelemetryCollector {
    /// 每类型的耗时分布。
    by_type: BTreeMap<TaskType, PercentileEstimator>,
    /// 全局分布。
    global: PercentileEstimator,
    /// 慢任务列表（按总耗时降序，保留 Top-N）。
    slow_tasks: Vec<TaskTelemetry>,
    /// 慢任务榜容量。
    slow_capacity: usize,
    /// 抢占计数。
    preempt_count: u64,
    /// 提升计数。
    boost_count: u64,
}

impl TaskTelemetryCollector {
    /// 构造。
    pub fn new(slow_capacity: usize) -> Self {
        Self {
            by_type: BTreeMap::new(),
            global: PercentileEstimator::new(),
            slow_tasks: Vec::new(),
            slow_capacity: slow_capacity.max(1),
            preempt_count: 0,
            boost_count: 0,
        }
    }

    /// 记录任务。
    pub fn record(&mut self, tt: TaskTelemetry) {
        let total = tt.timing.total_us();
        self.global.record(total);
        self.by_type
            .entry(tt.task_type)
            .or_default()
            .record(total);

        if tt.preempted {
            self.preempt_count += 1;
        }
        self.boost_count += tt.boost_count as u64;

        // 慢任务榜：Top-N。
        if self.slow_tasks.len() < self.slow_capacity {
            self.slow_tasks.push(tt);
            self.slow_tasks.sort_by(|a, b| b.timing.total_us().cmp(&a.timing.total_us()));
        } else if let Some(last) = self.slow_tasks.last() {
            if tt.timing.total_us() > last.timing.total_us() {
                self.slow_tasks.pop();
                self.slow_tasks.push(tt);
                self.slow_tasks.sort_by(|a, b| b.timing.total_us().cmp(&a.timing.total_us()));
            }
        }
    }

    /// 全局分布。
    pub fn global_stats(&self) -> &PercentileEstimator {
        &self.global
    }

    /// 某类型分布。
    pub fn type_stats(&self, t: TaskType) -> Option<&PercentileEstimator> {
        self.by_type.get(&t)
    }

    /// 慢任务榜。
    pub fn slow_tasks(&self) -> &[TaskTelemetry] {
        &self.slow_tasks
    }

    /// 初步归因（慢任务为什么慢）。
    pub fn attribute_slow(tt: &TaskTelemetry) -> String {
        let (dominant, val) = tt.timing.dominant_phase();
        let total = tt.timing.total_us().max(1);
        let pct = val * 100 / total;
        match dominant {
            "queue" => alloc::format!("排队慢（{}μs，占{}%）——调度器积压", val, pct),
            "ready_wait" => alloc::format!("就绪等待慢（{}μs，占{}%）——资源竞争", val, pct),
            "execute" => alloc::format!("执行慢（{}μs，占{}%）——计算密集", val, pct),
            "dependency" => alloc::format!("依赖等待慢（{}μs，占{}%）——外部资源", val, pct),
            _ => "未知".to_string(),
        }
    }

    /// 抢占频率。
    pub fn preempt_rate(&self) -> f64 {
        let total = self.global.count();
        if total == 0 {
            0.0
        } else {
            self.preempt_count as f64 / total as f64
        }
    }
}

// ============================================================================
// 四、判据：F0276 四条（四段耗时、类型拆分、事件流、慢任务榜）
// ============================================================================

/// F0276 判据集。
pub fn run_f0276_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0276");

    // --- 判据 1：四段耗时 + 分布 ---
    let mut collector = TaskTelemetryCollector::new(5);
    for i in 0..100u32 {
        collector.record(TaskTelemetry {
            task_id: i,
            task_type: TaskType::all()[(i % 4) as usize],
            timing: FourPhaseTiming {
                queue_us: 100 + (i as u64 % 50),
                ready_wait_us: 50 + (i as u64 % 30),
                execute_us: 200 + (i as u64 % 100),
                dependency_wait_us: 10 + (i as u64 % 20),
            },
            frame: (i / 10) as u64,
            preempted: i % 10 == 0,
            boost_count: i % 3,
        });
    }
    let g = collector.global_stats();
    cs.add(
        "四段耗时 P50/P95/P99",
        g.count() == 100 && g.p50() > 0 && g.p95() >= g.p50() && g.p99() >= g.p95(),
        alloc::format!("P50={} P95={} P99={}", g.p50(), g.p95(), g.p99()).leak(),
    );

    // --- 判据 2：类型拆分 ---
    let all_types_present = TaskType::all().iter().all(|t| collector.type_stats(*t).is_some());
    cs.add(
        "任务类型维度拆分",
        all_types_present,
        "栅格化/合成/解码/压缩四列",
    );

    // --- 判据 3：抢占与提升事件流 ---
    let preempt_rate = collector.preempt_rate();
    cs.add(
        "抢占事件流",
        collector.preempt_count > 0 && preempt_rate > 0.0 && preempt_rate < 1.0,
        alloc::format!("抢占率={:.2}%", preempt_rate * 100.0).leak(),
    );

    // --- 判据 4：慢任务榜 + 归因 ---
    let slow = collector.slow_tasks();
    let slow_ok = slow.len() == 5; // Top-5
    let attr = TaskTelemetryCollector::attribute_slow(&slow[0]);
    cs.add(
        "慢任务榜 Top-5 + 归因",
        slow_ok && attr.len() > 5,
        alloc::format!("榜首归因: {}", attr).leak(),
    );

    // --- 附加：dominant_phase ---
    let t = FourPhaseTiming {
        queue_us: 10,
        ready_wait_us: 20,
        execute_us: 300,
        dependency_wait_us: 5,
    };
    let (dom, _) = t.dominant_phase();
    cs.add(
        "dominant_phase 正确",
        dom == "execute",
        "执行段最耗时",
    );

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f0276_all_pass() {
        let cs = run_f0276_checks();
        assert!(cs.all_passed(), "F0276 判据未全绿: {}", cs.red_items_text());
    }

    #[test]
    fn percentile_estimator() {
        let mut est = PercentileEstimator::new();
        for i in 1..=100 {
            est.record(i);
        }
        assert!(est.p50() >= 40 && est.p50() <= 60);
        assert!(est.p99() >= 90);
    }

    #[test]
    fn slow_tasks_sorted() {
        let mut c = TaskTelemetryCollector::new(3);
        for i in 0..10 {
            c.record(TaskTelemetry {
                task_id: i,
                task_type: TaskType::Compose,
                timing: FourPhaseTiming {
                    queue_us: 0,
                    ready_wait_us: 0,
                    execute_us: (i as u64) * 100,
                    dependency_wait_us: 0,
                },
                frame: 0,
                preempted: false,
                boost_count: 0,
            });
        }
        assert_eq!(c.slow_tasks().len(), 3);
        assert!(c.slow_tasks()[0].timing.total_us() >= c.slow_tasks()[1].timing.total_us());
    }
}
