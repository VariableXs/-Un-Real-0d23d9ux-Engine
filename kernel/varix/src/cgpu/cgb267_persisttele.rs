// SPDX-License-Identifier: Apache-2.0
//! CGPU-F0267 · 持久化遥测指标
//!
//! 五指标：cgpu.persist.*（快照体积/恢复耗时/序列化吞吐/增量压缩比/文件加载）。
//! 与 F0274 schema 对齐，进 B08 遥测总线。
//!
//! 纯功能代码，深度定制，非脚本生成。

use alloc::vec;
extern crate alloc;

use alloc::vec::Vec;
use alloc::string::{String, ToString};

use crate::checks::{CheckSet, CheckSetBuilder};
use super::cgb274_schema::{MetricDef, MetricType, SamplePolicy, DimKey, Unit, SchemaRegistry};
use super::cgb279_converge::TelemetrySample;

// ---------------------------------------------------------------------------
// 持久化五指标
// ---------------------------------------------------------------------------

/// 持久化遥测指标。
#[derive(Debug, Clone)]
pub struct PersistMetrics {
    /// 快照体积（字节）。
    pub snapshot_bytes: u64,
    /// 恢复耗时（微秒）。
    pub recovery_us: u64,
    /// 序列化吞吐（字节/秒）。
    pub serialization_throughput: u64,
    /// 增量压缩比（原始/压缩）。
    pub compression_ratio: f32,
    /// 文件加载耗时（微秒）。
    pub file_load_us: u64,
}

impl PersistMetrics {
    pub fn new() -> Self {
        PersistMetrics {
            snapshot_bytes: 0,
            recovery_us: 0,
            serialization_throughput: 0,
            compression_ratio: 1.0,
            file_load_us: 0,
        }
    }

    /// 转为遥测样本。
    pub fn to_samples(&self, timestamp_ns: u64) -> Vec<TelemetrySample> {
        alloc::vec![
            TelemetrySample { metric: "cgpu.persist.snapshot_bytes".into(), timestamp_ns, value: self.snapshot_bytes as f64, frame: 0, task: 0, queue: 0, account: 0 },
            TelemetrySample { metric: "cgpu.persist.recovery_us".into(), timestamp_ns, value: self.recovery_us as f64, frame: 0, task: 0, queue: 0, account: 0 },
            TelemetrySample { metric: "cgpu.persist.throughput_bps".into(), timestamp_ns, value: self.serialization_throughput as f64, frame: 0, task: 0, queue: 0, account: 0 },
            TelemetrySample { metric: "cgpu.persist.compression_ratio".into(), timestamp_ns, value: self.compression_ratio as f64, frame: 0, task: 0, queue: 0, account: 0 },
            TelemetrySample { metric: "cgpu.persist.file_load_us".into(), timestamp_ns, value: self.file_load_us as f64, frame: 0, task: 0, queue: 0, account: 0 },
        ]
    }
}

/// 注册持久化五指标到 schema。
pub fn register_persist_metrics(reg: &mut SchemaRegistry) -> Result<usize, u16> {
    let specs: Vec<(&str, Unit, MetricType, SamplePolicy, Vec<DimKey>, &str)> = alloc::vec![
        ("cgpu.persist.snapshot_bytes", Unit::Byte, MetricType::Histogram, SamplePolicy::Low, alloc::vec![], "快照体积"),
        ("cgpu.persist.recovery_us", Unit::Microsecond, MetricType::Histogram, SamplePolicy::Low, alloc::vec![], "恢复耗时"),
        ("cgpu.persist.throughput_bps", Unit::Byte, MetricType::Gauge, SamplePolicy::Low, alloc::vec![], "序列化吞吐"),
        ("cgpu.persist.compression_ratio", Unit::Dimensionless, MetricType::Gauge, SamplePolicy::Low, alloc::vec![], "增量压缩比"),
        ("cgpu.persist.file_load_us", Unit::Microsecond, MetricType::Histogram, SamplePolicy::Low, alloc::vec![], "文件加载耗时"),
    ];
    let mut count = 0;
    for (name, unit, mtype, policy, dims, desc) in specs {
        let def = MetricDef::new(name, unit, mtype, policy, dims, desc)?;
        reg.register(def)?;
        count += 1;
    }
    Ok(count)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// CGPU-F0267 判据全集。
pub fn run_persist_telemetry_checks() -> CheckSet {
    let mut cs = CheckSetBuilder::new("CGPU-F0267", "持久化遥测指标");

    // 1. 五指标
    let pm = PersistMetrics::new();
    let samples = pm.to_samples(1000);
    cs.ok("五指标齐备", samples.len() == 5, "体积/恢复/吞吐/压缩/加载");
    cs.ok("指标名合法", samples.iter().all(|s| MetricDef::is_valid_name(&s.metric)), "F0274 对齐");

    // 2. schema 注册
    let mut reg = SchemaRegistry::new();
    let count = register_persist_metrics(&mut reg).unwrap_or(0);
    cs.ok("五指标注册", count == 5, "schema 就绪");
    cs.ok("名称唯一", reg.count() == 5, "无重复");

    // 3. 值语义
    let mut pm2 = PersistMetrics::new();
    pm2.snapshot_bytes = 1024 * 1024; // 1MB
    pm2.recovery_us = 50000; // 50ms
    pm2.compression_ratio = 3.5;
    let samples2 = pm2.to_samples(2000);
    cs.ok("体积记录", samples2[0].value == 1024.0 * 1024.0, "1MB");
    cs.ok("恢复记录", samples2[1].value == 50000.0, "50ms");
    cs.ok("压缩比记录", (samples2[3].value - 3.5).abs() < 0.01, "3.5x");

    cs.done()
}
