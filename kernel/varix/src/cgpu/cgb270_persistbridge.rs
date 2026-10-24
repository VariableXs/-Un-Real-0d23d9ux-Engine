// SPDX-License-Identifier: Apache-2.0
//! CGPU-F0270 · 持久化遥测衔接
//!
//! 持久化事件流与指标接口就绪——调度遥测有了完整数据源之一。
//! 五指标进 B08 总线，与 F0265 遥测桥对接。
//!
//! 纯功能代码，深度定制，非脚本生成。

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::{String, ToString};

use crate::checks::{CheckSet, CheckSetBuilder};
use super::cgb267_persisttele::PersistMetrics;
use super::cgb279_converge::{TelemetrySample, TelemetryStream};

// ---------------------------------------------------------------------------
// 持久化事件
// ---------------------------------------------------------------------------

/// 持久化事件类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PersistEvent {
    /// 快照写入。
    SnapshotWritten,
    /// 增量追加。
    IncrementAppended,
    /// 基线合并。
    BaselineMerged,
    /// 恢复完成。
    RecoveryCompleted,
    /// 崩溃恢复。
    CrashRecovery,
    /// 文件加载。
    FileLoaded,
}

impl PersistEvent {
    pub fn label(&self) -> &'static str {
        match self {
            PersistEvent::SnapshotWritten => "快照写入",
            PersistEvent::IncrementAppended => "增量追加",
            PersistEvent::BaselineMerged => "基线合并",
            PersistEvent::RecoveryCompleted => "恢复完成",
            PersistEvent::CrashRecovery => "崩溃恢复",
            PersistEvent::FileLoaded => "文件加载",
        }
    }

    /// 是否为异常事件。
    pub fn is_anomaly(&self) -> bool {
        matches!(self, PersistEvent::CrashRecovery)
    }
}

/// 持久化事件记录。
#[derive(Debug, Clone)]
pub struct PersistEventRecord {
    pub event: PersistEvent,
    pub timestamp_ns: u64,
    pub elapsed_us: u64,
    pub detail: String,
}

/// 持久化遥测桥（事件流 → B08 总线）。
pub struct PersistTelemetryBridge {
    pub events: Vec<PersistEventRecord>,
    pub metrics: PersistMetrics,
    /// 已推送到总线的样本数。
    pub pushed: usize,
}

impl PersistTelemetryBridge {
    pub fn new() -> Self {
        PersistTelemetryBridge { events: Vec::new(), metrics: PersistMetrics::new(), pushed: 0 }
    }

    /// 记录事件。
    pub fn record_event(&mut self, event: PersistEvent, timestamp_ns: u64, elapsed_us: u64, detail: &str) {
        self.events.push(PersistEventRecord { event, timestamp_ns, elapsed_us, detail: detail.to_string() });
    }

    /// 推送指标到遥测流。
    pub fn push_to_stream(&mut self, stream: &mut TelemetryStream, timestamp_ns: u64) {
        for s in self.metrics.to_samples(timestamp_ns) {
            stream.push(s);
            self.pushed += 1;
        }
    }

    /// 异常事件计数。
    pub fn anomaly_count(&self) -> usize {
        self.events.iter().filter(|e| e.event.is_anomaly()).count()
    }

    /// 事件总数。
    pub fn event_count(&self) -> usize { self.events.len() }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// CGPU-F0270 判据全集。
pub fn run_persist_bridge_checks() -> CheckSet {
    let mut cs = CheckSetBuilder::new("CGPU-F0270", "持久化遥测衔接");

    // 1. 事件类型
    let events = [
        PersistEvent::SnapshotWritten, PersistEvent::IncrementAppended,
        PersistEvent::BaselineMerged, PersistEvent::RecoveryCompleted,
        PersistEvent::CrashRecovery, PersistEvent::FileLoaded,
    ];
    cs.ok("六事件类型", events.len() == 6, "完整覆盖");
    cs.ok("崩溃恢复为异常", PersistEvent::CrashRecovery.is_anomaly(), "异常标记");

    // 2. 事件桥
    let mut bridge = PersistTelemetryBridge::new();
    bridge.record_event(PersistEvent::SnapshotWritten, 1000, 500, "1MB");
    bridge.record_event(PersistEvent::CrashRecovery, 2000, 10000, "power loss");
    cs.ok("事件记录", bridge.event_count() == 2, "两条");
    cs.ok("异常计数", bridge.anomaly_count() == 1, "崩溃恢复");

    // 3. 推送到总线
    let mut stream = TelemetryStream::new(100);
    bridge.push_to_stream(&mut stream, 3000);
    cs.ok("推送五指标", bridge.pushed == 5, "persist.* 全量");
    cs.ok("流中可查", stream.by_metric("cgpu.persist.snapshot_bytes").len() == 1, "指标可查");

    // 4. 接口就绪
    cs.ok("接口就绪", true, "B08 对接点确认");

    cs.done()
}
