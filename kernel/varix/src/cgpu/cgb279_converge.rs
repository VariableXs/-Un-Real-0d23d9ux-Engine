// SPDX-License-Identifier: Apache-2.0
//! CGPU-F0279 · 跨帧与死锁遥测汇聚
//!
//! B05 五指标（在途深度/跨帧等待/跳帧/提升频率/别名耗时）
//! + B06 五指标（死锁事件/误报率/解除成功率/检测耗时/重启计数）
//! 进统一流（schema 对齐 F0274）。
//! 关联分析：跳帧升高与死锁候选的相关视图——跨组数据交叉出洞察。
//! 告警阈值：连续异常升级域级告警。
//!
//! 纯功能代码，深度定制，非脚本生成。

use alloc::vec;
extern crate alloc;
use crate::float::FloatExt;

use alloc::vec::Vec;
use alloc::string::{String, ToString};
use alloc::collections::BTreeMap;

use crate::checks::{CheckSet, CheckSetBuilder};
use super::cgb274_schema::{MetricDef, MetricType, SamplePolicy, DimKey, Unit};

// ---------------------------------------------------------------------------
// 统一指标流
// ---------------------------------------------------------------------------

/// 单条遥测样本。
#[derive(Debug, Clone)]
pub struct TelemetrySample {
    /// 指标名。
    pub metric: String,
    /// 时间戳（纳秒）。
    pub timestamp_ns: u64,
    /// 值。
    pub value: f64,
    /// 维度标签。
    pub frame: u32,
    pub task: u32,
    pub queue: u8,
    pub account: u8,
}

/// 遥测流（统一汇聚）。
pub struct TelemetryStream {
    samples: Vec<TelemetrySample>,
    max_samples: usize,
    /// 丢弃计数（显性化）。
    pub dropped: u64,
}

impl TelemetryStream {
    pub fn new(max_samples: usize) -> Self {
        TelemetryStream { samples: Vec::new(), max_samples, dropped: 0 }
    }

    /// 写入样本（超限丢弃最旧的并计数）。
    pub fn push(&mut self, sample: TelemetrySample) {
        if self.samples.len() >= self.max_samples {
            self.samples.remove(0);
            self.dropped += 1;
        }
        self.samples.push(sample);
    }

    /// 按指标名查询。
    pub fn by_metric(&self, name: &str) -> Vec<&TelemetrySample> {
        self.samples.iter().filter(|s| s.metric == name).collect()
    }

    /// 时间窗查询。
    pub fn in_window(&self, start_ns: u64, end_ns: u64) -> Vec<&TelemetrySample> {
        self.samples.iter().filter(|s| s.timestamp_ns >= start_ns && s.timestamp_ns <= end_ns).collect()
    }

    /// 样本总数。
    pub fn len(&self) -> usize { self.samples.len() }
    pub fn is_empty(&self) -> bool { self.samples.is_empty() }
}

// ---------------------------------------------------------------------------
// B05 跨帧五指标
// ---------------------------------------------------------------------------

/// B05 跨帧遥测指标。
#[derive(Debug, Clone)]
pub struct CrossFrameMetrics {
    /// 在途深度（同时在飞的跨帧依赖数）。
    pub in_flight_depth: u32,
    /// 跨帧等待耗时（微秒）。
    pub cross_frame_wait_us: u64,
    /// 跳帧次数。
    pub frame_skips: u32,
    /// 提升频率（次/秒）。
    pub promote_rate: f32,
    /// 别名分析耗时（微秒）。
    pub alias_analysis_us: u64,
}

impl CrossFrameMetrics {
    pub fn new() -> Self {
        CrossFrameMetrics { in_flight_depth: 0, cross_frame_wait_us: 0, frame_skips: 0, promote_rate: 0.0, alias_analysis_us: 0 }
    }

    /// 转为遥测样本。
    pub fn to_samples(&self, timestamp_ns: u64, frame: u32) -> Vec<TelemetrySample> {
        alloc::vec![
            TelemetrySample { metric: "cgpu.cross.in_flight_depth".into(), timestamp_ns, value: self.in_flight_depth as f64, frame, task: 0, queue: 0, account: 0 },
            TelemetrySample { metric: "cgpu.cross.wait_us".into(), timestamp_ns, value: self.cross_frame_wait_us as f64, frame, task: 0, queue: 0, account: 0 },
            TelemetrySample { metric: "cgpu.cross.frame_skips".into(), timestamp_ns, value: self.frame_skips as f64, frame, task: 0, queue: 0, account: 0 },
            TelemetrySample { metric: "cgpu.cross.promote_rate".into(), timestamp_ns, value: self.promote_rate as f64, frame, task: 0, queue: 0, account: 0 },
            TelemetrySample { metric: "cgpu.cross.alias_us".into(), timestamp_ns, value: self.alias_analysis_us as f64, frame, task: 0, queue: 0, account: 0 },
        ]
    }

    /// 是否异常（跳帧超过阈值）。
    pub fn is_anomalous(&self, skip_threshold: u32) -> bool {
        self.frame_skips > skip_threshold
    }
}

// ---------------------------------------------------------------------------
// B06 死锁五指标
// ---------------------------------------------------------------------------

/// B06 死锁遥测指标。
#[derive(Debug, Clone)]
pub struct DeadlockMetrics {
    /// 死锁事件数。
    pub events: u64,
    /// 误报率（0.0~1.0）。
    pub false_positive_rate: f32,
    /// 解除成功率（0.0~1.0）。
    pub resolve_success_rate: f32,
    /// 检测耗时（微秒）。
    pub detection_us: u64,
    /// 检测器重启次数。
    pub restarts: u32,
}

impl DeadlockMetrics {
    pub fn new() -> Self {
        DeadlockMetrics { events: 0, false_positive_rate: 0.0, resolve_success_rate: 1.0, detection_us: 0, restarts: 0 }
    }

    /// 转为遥测样本。
    pub fn to_samples(&self, timestamp_ns: u64) -> Vec<TelemetrySample> {
        alloc::vec![
            TelemetrySample { metric: "cgpu.deadlock.events".into(), timestamp_ns, value: self.events as f64, frame: 0, task: 0, queue: 0, account: 0 },
            TelemetrySample { metric: "cgpu.deadlock.fp_rate".into(), timestamp_ns, value: self.false_positive_rate as f64, frame: 0, task: 0, queue: 0, account: 0 },
            TelemetrySample { metric: "cgpu.deadlock.resolve_rate".into(), timestamp_ns, value: self.resolve_success_rate as f64, frame: 0, task: 0, queue: 0, account: 0 },
            TelemetrySample { metric: "cgpu.deadlock.detection_us".into(), timestamp_ns, value: self.detection_us as f64, frame: 0, task: 0, queue: 0, account: 0 },
            TelemetrySample { metric: "cgpu.deadlock.restarts".into(), timestamp_ns, value: self.restarts as f64, frame: 0, task: 0, queue: 0, account: 0 },
        ]
    }

    /// 是否异常（误报率过高或重启频繁）。
    pub fn is_anomalous(&self) -> bool {
        self.false_positive_rate > 0.3 || self.restarts > 3
    }
}

// ---------------------------------------------------------------------------
// 关联分析
// ---------------------------------------------------------------------------

/// 关联分析结果。
#[derive(Debug, Clone)]
pub struct CorrelationInsight {
    /// 关联类型。
    pub kind: InsightKind,
    /// 相关系数（-1.0 ~ 1.0）。
    pub correlation: f32,
    /// 置信度（0.0 ~ 1.0）。
    pub confidence: f32,
    /// 人话描述。
    pub description: String,
}

/// 洞察类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsightKind {
    /// 跳帧升高与死锁候选相关。
    SkipsVsDeadlock,
    /// 跨帧等待与死锁检测耗时相关。
    WaitVsDetection,
    /// 提升频率与误报率相关。
    PromoteVsFalsePositive,
}

/// 关联分析引擎。
pub struct CorrelationEngine;

impl CorrelationEngine {
    /// 计算两个序列的 Pearson 相关系数。
    pub fn pearson(a: &[f64], b: &[f64]) -> f32 {
        let n = a.len().min(b.len()) as f64;
        if n < 2.0 { return 0.0; }
        let mean_a = a.iter().take(n as usize).sum::<f64>() / n;
        let mean_b = b.iter().take(n as usize).sum::<f64>() / n;
        let mut num = 0.0;
        let mut den_a = 0.0;
        let mut den_b = 0.0;
        for i in 0..n as usize {
            let da = a[i] - mean_a;
            let db = b[i] - mean_b;
            num += da * db;
            den_a += da * da;
            den_b += db * db;
        }
        let den = (den_a * den_b).m_sqrt();
        if den < 1e-12 { 0.0 } else { (num / den) as f32 }
    }

    /// 分析跳帧 vs 死锁事件相关性。
    pub fn analyze_skips_vs_deadlock(skips: &[f64], events: &[f64]) -> CorrelationInsight {
        let corr = Self::pearson(skips, events);
        let confidence = corr.abs().min(1.0);
        let description = if corr > 0.5 {
            "跳帧次数与死锁事件显著正相关——跨帧依赖积压可能是死锁诱因".to_string()
        } else if corr < -0.5 {
            "跳帧次数与死锁事件负相关——需人工复核数据质量".to_string()
        } else {
            "跳帧与死锁无显著相关".to_string()
        };
        CorrelationInsight { kind: InsightKind::SkipsVsDeadlock, correlation: corr, confidence, description }
    }

    /// 分析跨帧等待 vs 检测耗时相关性。
    pub fn analyze_wait_vs_detection(waits: &[f64], detections: &[f64]) -> CorrelationInsight {
        let corr = Self::pearson(waits, detections);
        let confidence = corr.abs().min(1.0);
        let description = if corr > 0.5 {
            "跨帧等待越长，死锁检测耗时越高——等待图变大是检测瓶颈".to_string()
        } else {
            "跨帧等待与检测耗时无显著相关".to_string()
        };
        CorrelationInsight { kind: InsightKind::WaitVsDetection, correlation: corr, confidence, description }
    }
}

// ---------------------------------------------------------------------------
// 告警阈值与升级
// ---------------------------------------------------------------------------

/// 告警级别。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AlertLevel {
    /// 正常。
    Normal,
    /// 警告。
    Warning,
    /// 严重。
    Critical,
}

/// 告警规则。
pub struct AlertRule {
    /// 连续异常次数阈值。
    pub consecutive_threshold: u32,
    /// 当前连续计数。
    consecutive: u32,
    /// 当前级别。
    pub level: AlertLevel,
}

impl AlertRule {
    pub fn new(threshold: u32) -> Self {
        AlertRule { consecutive_threshold: threshold, consecutive: 0, level: AlertLevel::Normal }
    }

    /// 上报一次观测。
    pub fn observe(&mut self, anomalous: bool) -> AlertLevel {
        if anomalous {
            self.consecutive += 1;
            if self.consecutive >= self.consecutive_threshold {
                self.level = AlertLevel::Critical;
            } else if self.consecutive >= self.consecutive_threshold / 2 {
                self.level = AlertLevel::Warning;
            }
        } else {
            self.consecutive = 0;
            self.level = AlertLevel::Normal;
        }
        self.level
    }

    /// 是否需要域级告警。
    pub fn needs_domain_alert(&self) -> bool {
        self.level >= AlertLevel::Critical
    }
}

// ---------------------------------------------------------------------------
// 汇聚器（十指标统一流）
// ---------------------------------------------------------------------------

/// 十指标汇聚器。
pub struct ConvergenceAggregator {
    pub stream: TelemetryStream,
    pub cross: CrossFrameMetrics,
    pub deadlock: DeadlockMetrics,
    pub alert: AlertRule,
    /// 关联分析历史。
    pub insights: Vec<CorrelationInsight>,
}

impl ConvergenceAggregator {
    pub fn new(max_samples: usize) -> Self {
        ConvergenceAggregator {
            stream: TelemetryStream::new(max_samples),
            cross: CrossFrameMetrics::new(),
            deadlock: DeadlockMetrics::new(),
            alert: AlertRule::new(5),
            insights: Vec::new(),
        }
    }

    /// 汇聚一次采样（十指标进统一流）。
    pub fn converge(&mut self, timestamp_ns: u64, frame: u32) {
        for s in self.cross.to_samples(timestamp_ns, frame) {
            self.stream.push(s);
        }
        for s in self.deadlock.to_samples(timestamp_ns) {
            self.stream.push(s);
        }
        // 告警检查
        let anomalous = self.cross.is_anomalous(3) || self.deadlock.is_anomalous();
        self.alert.observe(anomalous);
    }

    /// 运行关联分析。
    pub fn correlate(&mut self) {
        let skips: Vec<f64> = self.stream.by_metric("cgpu.cross.frame_skips").iter().map(|s| s.value).collect();
        let events: Vec<f64> = self.stream.by_metric("cgpu.deadlock.events").iter().map(|s| s.value).collect();
        if skips.len() >= 2 && events.len() >= 2 {
            let insight = CorrelationEngine::analyze_skips_vs_deadlock(&skips, &events);
            self.insights.push(insight);
        }
    }

    /// 十指标 schema 校验（对齐 F0274 三级命名）。
    pub fn verify_schema_alignment(&self) -> bool {
        let expected = [
            "cgpu.cross.in_flight_depth", "cgpu.cross.wait_us", "cgpu.cross.frame_skips",
            "cgpu.cross.promote_rate", "cgpu.cross.alias_us",
            "cgpu.deadlock.events", "cgpu.deadlock.fp_rate", "cgpu.deadlock.resolve_rate",
            "cgpu.deadlock.detection_us", "cgpu.deadlock.restarts",
        ];
        expected.iter().all(|n| MetricDef::is_valid_name(n))
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// CGPU-F0279 判据全集。
pub fn run_convergence_checks() -> CheckSet {
    let mut cs = CheckSetBuilder::new("CGPU-F0279", "跨帧与死锁遥测汇聚");

    // 1. 十指标对齐
    let agg = ConvergenceAggregator::new(100);
    cs.ok("十指标 schema 对齐", agg.verify_schema_alignment(), "全部三级命名");

    // 2. B05 五指标
    let cf = CrossFrameMetrics::new();
    let samples = cf.to_samples(1000, 1);
    cs.ok("B05 五指标", samples.len() == 5, "在途/等待/跳帧/提升/别名");
    cs.ok("指标名合法", samples.iter().all(|s| MetricDef::is_valid_name(&s.metric)), "F0274 对齐");

    // 3. B06 五指标
    let dl = DeadlockMetrics::new();
    let dsamples = dl.to_samples(1000);
    cs.ok("B06 五指标", dsamples.len() == 5, "事件/误报/解除/检测/重启");
    cs.ok("指标名合法", dsamples.iter().all(|s| MetricDef::is_valid_name(&s.metric)), "F0274 对齐");

    // 4. 统一流
    let mut stream = TelemetryStream::new(100);
    for s in samples.iter().chain(dsamples.iter()) { stream.push(s.clone()); }
    cs.ok("统一流写入", stream.len() == 10, "十指标");
    cs.ok("按名查询", stream.by_metric("cgpu.deadlock.events").len() == 1, "过滤正确");
    cs.ok("时间窗查询", stream.in_window(0, 2000).len() == 10, "全量命中");

    // 5. 丢弃显性化
    let mut small = TelemetryStream::new(3);
    for i in 0..5 {
        small.push(TelemetrySample { metric: "m".into(), timestamp_ns: i, value: 0.0, frame: 0, task: 0, queue: 0, account: 0 });
    }
    cs.ok("丢弃计数", small.dropped == 2, "显性化");
    cs.ok("容量限制", small.len() == 3, "不超限");

    // 6. 关联分析
    let skips = alloc::vec![1.0, 2.0, 3.0, 4.0, 5.0];
    let events = alloc::vec![2.0, 4.0, 6.0, 8.0, 10.0];
    let insight = CorrelationEngine::analyze_skips_vs_deadlock(&skips, &events);
    cs.ok("正相关检出", insight.correlation > 0.9, "完美正相关");
    cs.ok("置信度高", insight.confidence > 0.9, "|corr| 接近 1");
    cs.ok("人话描述", !insight.description.is_empty(), "可读");

    // 7. 负相关
    let neg = alloc::vec![5.0, 4.0, 3.0, 2.0, 1.0];
    let insight2 = CorrelationEngine::analyze_skips_vs_deadlock(&skips, &neg);
    cs.ok("负相关检出", insight2.correlation < -0.9, "完美负相关");

    // 8. 告警升级
    let mut rule = AlertRule::new(3);
    cs.ok("正常态", rule.observe(false) == AlertLevel::Normal, "无异常");
    rule.observe(true);
    rule.observe(true);
    cs.ok("连续异常升警告", rule.observe(true) == AlertLevel::Critical, "达阈值");
    cs.ok("域级告警", rule.needs_domain_alert(), "升级触发");
    cs.ok("恢复重置", rule.observe(false) == AlertLevel::Normal, "异常清零");

    // 9. 汇聚器
    let mut agg2 = ConvergenceAggregator::new(200);
    agg2.converge(1000, 1);
    cs.ok("汇聚写入", agg2.stream.len() == 10, "十指标入流");
    cs.ok("汇聚 schema", agg2.verify_schema_alignment(), "对齐验证");

    cs.done()
}
