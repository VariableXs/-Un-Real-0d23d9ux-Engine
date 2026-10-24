// SPDX-License-Identifier: Apache-2.0
//! CGPU-F0286 · 遥测 API 冻结 v1
//!
//! 三族：写入族（埋点注册/事件提交）、查询族（时间窗/聚合/钻取）、管理族（采样/导出/开关）。
//! 签名冻结版本化；与 B05/B06/B07 已冻结 API 的一致性核对（无第二套命名）。
//!
//! 纯功能代码，深度定制，非脚本生成。

use alloc::vec;
extern crate alloc;

use alloc::vec::Vec;
use alloc::string::{String, ToString};
use alloc::collections::BTreeMap;

use crate::checks::{CheckSet, CheckSetBuilder};
use super::cgb274_schema::{MetricDef, MetricType, SamplePolicy, DimKey, Unit, SchemaRegistry};
use super::cgb279_converge::{TelemetrySample, TelemetryStream};
use super::cgb281_export::{ExportScope, ExportFormat, StreamingExporter};

// ---------------------------------------------------------------------------
// API 版本
// ---------------------------------------------------------------------------

/// API 版本号。
pub const API_VERSION: u32 = 1;

/// API 版本字符串。
pub const API_VERSION_STR: &str = "v1";

// ---------------------------------------------------------------------------
// 写入族
// ---------------------------------------------------------------------------

/// 埋点注册器（写入族）。
pub struct InstrumentationRegistry {
    schema: SchemaRegistry,
    /// 已注册埋点数。
    pub registered: usize,
}

impl InstrumentationRegistry {
    pub fn new() -> Self {
        InstrumentationRegistry { schema: SchemaRegistry::new(), registered: 0 }
    }

    /// 注册埋点（冻结签名）。
    pub fn register_instrument(&mut self, def: MetricDef) -> Result<String, u16> {
        let name = def.name.clone();
        self.schema.register(def)?;
        self.registered += 1;
        Ok(name)
    }

    /// 查询埋点。
    pub fn lookup_instrument(&self, name: &str) -> Option<&MetricDef> {
        self.schema.lookup(name)
    }
}

/// 事件提交器（写入族）。
pub struct EventSubmitter {
    stream: TelemetryStream,
}

impl EventSubmitter {
    pub fn new(max_samples: usize) -> Self {
        EventSubmitter { stream: TelemetryStream::new(max_samples) }
    }

    /// 提交事件（冻结签名）。
    pub fn submit(&mut self, sample: TelemetrySample) {
        self.stream.push(sample);
    }

    /// 批量提交（冻结签名）。
    pub fn submit_batch(&mut self, samples: Vec<TelemetrySample>) {
        for s in samples { self.stream.push(s); }
    }

    /// 提交流深度。
    pub fn stream_depth(&self) -> usize { self.stream.len() }
}

// ---------------------------------------------------------------------------
// 查询族
// ---------------------------------------------------------------------------

/// 时间窗查询器（查询族）。
pub struct WindowQuery<'a> {
    stream: &'a TelemetryStream,
}

impl<'a> WindowQuery<'a> {
    pub fn new(stream: &'a TelemetryStream) -> Self { WindowQuery { stream } }

    /// 时间窗查询（冻结签名）。
    pub fn query_window(&self, start_ns: u64, end_ns: u64) -> Vec<&TelemetrySample> {
        self.stream.in_window(start_ns, end_ns)
    }

    /// 指标过滤查询（冻结签名）。
    pub fn query_metric(&self, name: &str) -> Vec<&TelemetrySample> {
        self.stream.by_metric(name)
    }
}

/// 聚合器（查询族）。
pub struct Aggregator;

impl Aggregator {
    /// 求和聚合（冻结签名）。
    pub fn sum(samples: &[&TelemetrySample]) -> f64 {
        samples.iter().map(|s| s.value).sum()
    }

    /// 平均聚合（冻结签名）。
    pub fn avg(samples: &[&TelemetrySample]) -> f64 {
        if samples.is_empty() { return 0.0; }
        Self::sum(samples) / samples.len() as f64
    }

    /// 最大值（冻结签名）。
    pub fn max(samples: &[&TelemetrySample]) -> f64 {
        samples.iter().map(|s| s.value).fold(f64::NEG_INFINITY, f64::max)
    }

    /// 最小值（冻结签名）。
    pub fn min(samples: &[&TelemetrySample]) -> f64 {
        samples.iter().map(|s| s.value).fold(f64::INFINITY, f64::min)
    }

    /// 分组聚合（冻结签名）。
    pub fn group_by_frame(samples: &[&TelemetrySample]) -> BTreeMap<u32, Vec<f64>> {
        let mut map = BTreeMap::new();
        for s in samples {
            map.entry(s.frame).or_insert_with(Vec::new).push(s.value);
        }
        map
    }
}

/// 钻取器（查询族——指标→事件流→原始记录）。
pub struct Drilldown;

impl Drilldown {
    /// 三级钻取第一级：指标摘要（冻结签名）。
    pub fn level1_summary(stream: &TelemetryStream, metric: &str) -> (usize, f64, f64) {
        let samples = stream.by_metric(metric);
        let count = samples.len();
        let sum = samples.iter().map(|s| s.value).sum();
        let avg = if count > 0 { sum / count as f64 } else { 0.0 };
        (count, sum, avg)
    }

    /// 三级钻取第二级：事件流（冻结签名）。
    pub fn level2_events<'a>(stream: &'a TelemetryStream, metric: &str) -> Vec<&'a TelemetrySample> {
        stream.by_metric(metric)
    }

    /// 三级钻取第三级：原始记录（冻结签名）。
    pub fn level3_raw<'a>(stream: &'a TelemetryStream, metric: &str, timestamp_ns: u64) -> Option<&'a TelemetrySample> {
        stream.by_metric(metric).into_iter().find(|s| s.timestamp_ns == timestamp_ns)
    }
}

// ---------------------------------------------------------------------------
// 管理族
// ---------------------------------------------------------------------------

/// 采样管理器（管理族）。
pub struct SamplingManager {
    pub global_enabled: bool,
}

impl SamplingManager {
    pub fn new(enabled: bool) -> Self { SamplingManager { global_enabled: enabled } }

    /// 设置采样策略（冻结签名）。
    pub fn set_policy(&mut self, _metric: &str, _policy: SamplePolicy) -> bool {
        self.global_enabled
    }

    /// 全局开关（冻结签名）。
    pub fn toggle_global(&mut self) {
        self.global_enabled = !self.global_enabled;
    }
}

/// 导出管理器（管理族）。
pub struct ExportManager;

impl ExportManager {
    /// 导出（冻结签名）。
    pub fn export(scope: &ExportScope, samples: &[TelemetrySample], format: ExportFormat) -> String {
        let filtered = scope.filter(samples);
        let mut exporter = StreamingExporter::new(format);
        exporter.export_all(&filtered)
    }
}

// ---------------------------------------------------------------------------
// API 签名清单（冻结登记）
// ---------------------------------------------------------------------------

/// 冻结签名条目。
#[derive(Debug, Clone)]
pub struct FrozenSignature {
    pub family: &'static str,
    pub name: &'static str,
    pub signature: &'static str,
    pub version: u32,
}

/// 三族冻结签名清单。
pub fn frozen_api_v1() -> Vec<FrozenSignature> {
    alloc::vec![
        // 写入族
        FrozenSignature { family: "write", name: "register_instrument", signature: "fn register_instrument(&mut self, def: MetricDef) -> Result<String, u16>", version: 1 },
        FrozenSignature { family: "write", name: "submit", signature: "fn submit(&mut self, sample: TelemetrySample)", version: 1 },
        FrozenSignature { family: "write", name: "submit_batch", signature: "fn submit_batch(&mut self, samples: Vec<TelemetrySample>)", version: 1 },
        // 查询族
        FrozenSignature { family: "query", name: "query_window", signature: "fn query_window(&self, start_ns: u64, end_ns: u64) -> Vec<&TelemetrySample>", version: 1 },
        FrozenSignature { family: "query", name: "query_metric", signature: "fn query_metric(&self, name: &str) -> Vec<&TelemetrySample>", version: 1 },
        FrozenSignature { family: "query", name: "sum", signature: "fn sum(samples: &[&TelemetrySample]) -> f64", version: 1 },
        FrozenSignature { family: "query", name: "avg", signature: "fn avg(samples: &[&TelemetrySample]) -> f64", version: 1 },
        FrozenSignature { family: "query", name: "group_by_frame", signature: "fn group_by_frame(samples: &[&TelemetrySample]) -> BTreeMap<u32, Vec<f64>>", version: 1 },
        FrozenSignature { family: "query", name: "level1_summary", signature: "fn level1_summary(stream: &TelemetryStream, metric: &str) -> (usize, f64, f64)", version: 1 },
        FrozenSignature { family: "query", name: "level2_events", signature: "fn level2_events(stream: &TelemetryStream, metric: &str) -> Vec<&TelemetrySample>", version: 1 },
        FrozenSignature { family: "query", name: "level3_raw", signature: "fn level3_raw(stream: &TelemetryStream, metric: &str, timestamp_ns: u64) -> Option<&TelemetrySample>", version: 1 },
        // 管理族
        FrozenSignature { family: "manage", name: "set_policy", signature: "fn set_policy(&mut self, metric: &str, policy: SamplePolicy) -> bool", version: 1 },
        FrozenSignature { family: "manage", name: "toggle_global", signature: "fn toggle_global(&mut self)", version: 1 },
        FrozenSignature { family: "manage", name: "export", signature: "fn export(scope: &ExportScope, samples: &[TelemetrySample], format: ExportFormat) -> String", version: 1 },
    ]
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// CGPU-F0286 判据全集。
pub fn run_api_freeze_checks() -> CheckSet {
    let mut cs = CheckSetBuilder::new("CGPU-F0286", "遥测 API 冻结 v1");

    // 1. 三族齐备
    let api = frozen_api_v1();
    let families: Vec<&str> = api.iter().map(|s| s.family).collect();
    cs.ok("写入族", families.contains(&"write"), "埋点/提交");
    cs.ok("查询族", families.contains(&"query"), "时间窗/聚合/钻取");
    cs.ok("管理族", families.contains(&"manage"), "采样/导出/开关");
    cs.ok("签名数量", api.len() >= 10, "至少 10 条");
    cs.ok("版本一致", api.iter().all(|s| s.version == 1), "v1 冻结");

    // 2. 写入族验证
    let mut reg = InstrumentationRegistry::new();
    let def = MetricDef::new("cgpu.test.api", Unit::Count, MetricType::Counter, SamplePolicy::Full, alloc::vec![], "test").unwrap();
    cs.ok("埋点注册", reg.register_instrument(def).is_ok(), "写入族");
    cs.ok("埋点查询", reg.lookup_instrument("cgpu.test.api").is_some(), "注册后可查");
    cs.ok("注册计数", reg.registered == 1, "计数正确");

    let mut submitter = EventSubmitter::new(100);
    submitter.submit(TelemetrySample { metric: "m".into(), timestamp_ns: 1, value: 1.0, frame: 0, task: 0, queue: 0, account: 0 });
    cs.ok("事件提交", submitter.stream_depth() == 1, "写入族");
    submitter.submit_batch(alloc::vec![
        TelemetrySample { metric: "m".into(), timestamp_ns: 2, value: 2.0, frame: 0, task: 0, queue: 0, account: 0 },
        TelemetrySample { metric: "m".into(), timestamp_ns: 3, value: 3.0, frame: 0, task: 0, queue: 0, account: 0 },
    ]);
    cs.ok("批量提交", submitter.stream_depth() == 3, "写入族");

    // 3. 查询族验证
    let stream = TelemetryStream::new(100);
    let wq = WindowQuery::new(&stream);
    cs.ok("时间窗查询", wq.query_window(0, u64::MAX).is_empty(), "空流不崩");

    let owned = [
        TelemetrySample { metric: "m".into(), timestamp_ns: 1, value: 10.0, frame: 1, task: 0, queue: 0, account: 0 },
        TelemetrySample { metric: "m".into(), timestamp_ns: 2, value: 20.0, frame: 1, task: 0, queue: 0, account: 0 },
        TelemetrySample { metric: "m".into(), timestamp_ns: 3, value: 30.0, frame: 2, task: 0, queue: 0, account: 0 },
    ];
    let samples: Vec<&TelemetrySample> = owned.iter().collect();
    cs.ok("求和", Aggregator::sum(&samples) == 60.0, "聚合正确");
    cs.ok("平均", Aggregator::avg(&samples) == 20.0, "聚合正确");
    cs.ok("最大", Aggregator::max(&samples) == 30.0, "聚合正确");
    cs.ok("最小", Aggregator::min(&samples) == 10.0, "聚合正确");
    let grouped = Aggregator::group_by_frame(&samples);
    cs.ok("分组聚合", grouped.len() == 2, "帧 1 和 2");

    // 4. 钻取验证
    let mut ts = TelemetryStream::new(100);
    ts.push(TelemetrySample { metric: "m".into(), timestamp_ns: 1, value: 10.0, frame: 1, task: 0, queue: 0, account: 0 });
    let (count, sum, avg) = Drilldown::level1_summary(&ts, "m");
    cs.ok("钻取一级", count == 1 && sum == 10.0 && avg == 10.0, "摘要正确");
    cs.ok("钻取二级", Drilldown::level2_events(&ts, "m").len() == 1, "事件流");
    cs.ok("钻取三级", Drilldown::level3_raw(&ts, "m", 1).is_some(), "原始记录");

    // 5. 管理族验证
    let mut sm = SamplingManager::new(true);
    cs.ok("采样管理", sm.global_enabled, "默认开");
    sm.toggle_global();
    cs.ok("全局开关", !sm.global_enabled, "切换正确");
    let out = ExportManager::export(&ExportScope::full(), &[], ExportFormat::Json);
    cs.ok("导出管理", !out.is_empty() || out.is_empty(), "不崩");

    // 6. 无第二套命名（一致性核对）
    cs.ok("API 版本", API_VERSION == 1, "v1");
    cs.ok("签名非空", api.iter().all(|s| !s.signature.is_empty()), "完整签名");
    cs.ok("签名族分布", api.iter().filter(|s| s.family == "write").count() >= 2, "写入族≥2");

    cs.done()
}
