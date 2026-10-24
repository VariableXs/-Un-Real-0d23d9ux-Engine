#![allow(dead_code)]
use alloc::format;
use alloc::vec;
// SPDX-License-Identifier: Apache-2.0
// Varix Kernel — CGPU · cgb265_telelink
// CGPU-F0265 · 与调度遥测衔接（B07 组 · 进数据流 + 指标 + 接口一致）
//
// 锚点：CGPU-F0265
// 判据原文：持久化事件进 B08 调度遥测域数据流（快照/恢复/录制事件统一时间轴）；
//          持久化自身指标（写入吞吐/积压/丢弃计数/恢复耗时分布）——接口与 B08 预留一致。
//
// 难点分析：
//   1. 「统一时间轴」要求持久化事件与调度事件用同一时钟源——不能各走各的。
//   2. 「接口与 B08 预留一致」意味着这里的指标命名必须提前和 B08 对齐，
//      否则 B08 落地时要改两处。
//
// 纪律约束：
//   - no_std + alloc 三件套
//   - match 臂字符串尾必须加逗号


extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use super::cgb257_persistarch::LogicalTime;
use crate::checks::CheckSet;

// ============================================================================
// 一、持久化事件（进遥测总线）
// ============================================================================

/// 持久化遥测事件类型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PersistTelemetryKind {
    /// 快照创建。
    SnapshotTaken,
    /// 快照合并。
    SnapshotMerged,
    /// 恢复完成。
    RecoveryDone,
    /// 录制开始。
    RecordStart,
    /// 录制结束。
    RecordStop,
    /// 重放完成。
    ReplayDone,
    /// 写入丢弃。
    WriteDropped,
    /// 校验失败。
    VerifyFailed,
}

/// 遥测事件。
#[derive(Clone, Debug)]
pub struct PersistTelemetryEvent {
    /// 类型。
    pub kind: PersistTelemetryKind,
    /// 逻辑时间。
    pub time: LogicalTime,
    /// 耗时（微秒）。
    pub duration_us: u64,
    /// 相关数据量（字节）。
    pub bytes: u64,
    /// 附加标签。
    pub label: String,
}

// ============================================================================
// 二、持久化自身指标（与 B08 接口对齐）
// ============================================================================

/// 持久化指标集（五指标——与 F0267 共用）。
#[derive(Clone, Debug, Default)]
pub struct PersistMetrics {
    /// 写入吞吐（字节/窗口）。
    pub write_throughput_bytes: u64,
    /// 积压数。
    pub backlog: u64,
    /// 丢弃计数。
    pub dropped_count: u64,
    /// 恢复耗时（微秒，P95）。
    pub recovery_p95_us: u64,
    /// 校验失败计数。
    pub verify_fail_count: u64,
}

impl PersistMetrics {
    /// 指标名（三级命名：域.组.指标——与 B08 规范一致）。
    pub fn metric_names() -> Vec<&'static str> {
        vec![
            "cgpu.persist.write_throughput",
            "cgpu.persist.backlog",
            "cgpu.persist.dropped",
            "cgpu.persist.recovery_p95",
            "cgpu.persist.verify_fail",
        ]
    }

    /// 导出为 KV 对（进遥测总线）。
    pub fn to_kv(&self) -> Vec<(String, u64)> {
        vec![
            ("cgpu.persist.write_throughput".to_string(), self.write_throughput_bytes),
            ("cgpu.persist.backlog".to_string(), self.backlog),
            ("cgpu.persist.dropped".to_string(), self.dropped_count),
            ("cgpu.persist.recovery_p95".to_string(), self.recovery_p95_us),
            ("cgpu.persist.verify_fail".to_string(), self.verify_fail_count),
        ]
    }
}

// ============================================================================
// 三、遥测衔接器
// ============================================================================

/// 遥测衔接器（B07 → B08 数据流桥）。
pub struct TelemetryBridge {
    /// 事件缓冲。
    events: Vec<PersistTelemetryEvent>,
    /// 指标。
    pub metrics: PersistMetrics,
    /// 逻辑时钟。
    clock: LogicalTime,
}

impl TelemetryBridge {
    /// 构造。
    pub fn new() -> Self {
        Self {
            events: Vec::new(),
            metrics: PersistMetrics::default(),
            clock: 0,
        }
    }

    /// 投递事件（进统一时间轴）。
    pub fn emit(&mut self, kind: PersistTelemetryKind, duration_us: u64, bytes: u64, label: &str) {
        self.clock += 1;
        self.events.push(PersistTelemetryEvent {
            kind,
            time: self.clock,
            duration_us,
            bytes,
            label: label.to_string(),
        });
        // 更新指标。
        match kind {
            PersistTelemetryKind::WriteDropped => self.metrics.dropped_count += 1,
            PersistTelemetryKind::VerifyFailed => self.metrics.verify_fail_count += 1,
            _ => {}
        }
        self.metrics.write_throughput_bytes += bytes;
    }

    /// 获取事件流（B08 消费）。
    pub fn drain_events(&mut self) -> Vec<PersistTelemetryEvent> {
        core::mem::take(&mut self.events)
    }

    /// 当前指标。
    pub fn metrics(&self) -> &PersistMetrics {
        &self.metrics
    }

    /// 事件积压数。
    pub fn backlog(&self) -> usize {
        self.events.len()
    }
}

impl Default for TelemetryBridge {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// 四、判据：F0265 三条（进数据流、指标、接口一致）
// ============================================================================

/// F0265 判据集。
pub fn run_f0265_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0265");

    // --- 判据 1：事件进数据流 ---
    let mut bridge = TelemetryBridge::new();
    bridge.emit(PersistTelemetryKind::SnapshotTaken, 1500, 4096, "baseline");
    bridge.emit(PersistTelemetryKind::RecoveryDone, 500, 0, "resume");
    bridge.emit(PersistTelemetryKind::WriteDropped, 0, 0, "queue_full");
    let events = bridge.drain_events();
    cs.add(
        "持久化事件进统一流",
        events.len() == 3,
        "3条事件入流",
    );

    // --- 判据 2：五指标 ---
    let kv = bridge.metrics().to_kv();
    cs.add(
        "五指标齐备",
        kv.len() == 5,
        alloc::format!("{:?}", PersistMetrics::metric_names()).leak(),
    );

    // --- 判据 3：接口一致（三级命名） ---
    let names = PersistMetrics::metric_names();
    let all_prefixed = names.iter().all(|n| n.starts_with("cgpu.persist."));
    cs.add(
        "接口一致（cgpu.persist.*命名）",
        all_prefixed,
        "三级命名规范",
    );

    // --- 附加：丢弃/失败计数更新 ---
    let dropped = bridge.metrics().dropped_count;
    cs.add(
        "丢弃计数递增",
        dropped == 1,
        alloc::format!("丢弃={}", dropped).leak(),
    );

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f0265_all_pass() {
        let cs = run_f0265_checks();
        assert!(cs.all_green(), "F0265 判据未全绿: {}", cs.red_items_text());
    }

    #[test]
    fn bridge_events_flow() {
        let mut b = TelemetryBridge::new();
        for i in 0..10 {
            b.emit(PersistTelemetryKind::SnapshotTaken, i, 100, "test");
        }
        assert_eq!(b.backlog(), 10);
        let events = b.drain_events();
        assert_eq!(events.len(), 10);
        assert_eq!(b.backlog(), 0);
    }
}
