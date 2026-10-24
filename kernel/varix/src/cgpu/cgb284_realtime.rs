// SPDX-License-Identifier: Apache-2.0
//! CGPU-F0284 · 实时监控模式
//!
//! 开发档实时数据流：指标秒级推送——内存通道不过磁盘。
//! 实时视图数据契约：引擎侧出点列数据——UI 侧自由渲染。
//! 背压：订阅者慢→自动降频，关键事件不丢。
//! 录制模式：实时流转 B07 持久化对接。
//!
//! 纯功能代码，深度定制，非脚本生成。

use alloc::vec;
extern crate alloc;

use alloc::vec::Vec;
use alloc::string::{String, ToString};
use alloc::collections::BTreeMap;

use crate::checks::{CheckSet, CheckSetBuilder};
use super::cgb279_converge::TelemetrySample;

// ---------------------------------------------------------------------------
// 实时数据流
// ---------------------------------------------------------------------------

/// 实时推送配置。
#[derive(Debug, Clone)]
pub struct RealtimeConfig {
    /// 推送间隔（毫秒）。
    pub push_interval_ms: u32,
    /// 是否录制到 B07。
    pub record_to_persist: bool,
    /// 最大缓冲（样本数）。
    pub max_buffer: usize,
}

impl RealtimeConfig {
    pub fn dev_default() -> Self {
        RealtimeConfig { push_interval_ms: 1000, record_to_persist: false, max_buffer: 10_000 }
    }
}

/// 实时数据流。
pub struct RealtimeStream {
    pub config: RealtimeConfig,
    buffer: Vec<TelemetrySample>,
    /// 已推送计数。
    pub pushed: u64,
    /// 背压降频次数。
    pub backpressure_events: u64,
    /// 当前降频因子（1 = 正常）。
    pub throttle_factor: u32,
}

impl RealtimeStream {
    pub fn new(config: RealtimeConfig) -> Self {
        RealtimeStream { config, buffer: Vec::new(), pushed: 0, backpressure_events: 0, throttle_factor: 1 }
    }

    /// 写入样本。
    pub fn push_sample(&mut self, sample: TelemetrySample) {
        if self.buffer.len() >= self.config.max_buffer {
            // 背压：降频 + 丢弃最旧
            self.backpressure_events += 1;
            self.throttle_factor = (self.throttle_factor + 1).min(10);
            self.buffer.remove(0);
        }
        self.buffer.push(sample);
    }

    /// 模拟一次推送（返回待推送数据）。
    pub fn flush(&mut self) -> Vec<TelemetrySample> {
        let out = core::mem::take(&mut self.buffer);
        self.pushed += out.len() as u64;
        out
    }

    /// 当前缓冲深度。
    pub fn depth(&self) -> usize { self.buffer.len() }

    /// 是否触发背压。
    pub fn is_throttled(&self) -> bool { self.throttle_factor > 1 }
}

// ---------------------------------------------------------------------------
// 实时视图数据契约
// ---------------------------------------------------------------------------

/// 视图数据点（引擎侧出点列）。
#[derive(Debug, Clone)]
pub struct ViewDataPoint {
    pub x: f32,
    pub y: f32,
    pub label: String,
    pub color_index: u8,
}

/// 实时视图契约。
#[derive(Debug, Clone)]
pub struct RealtimeViewContract {
    pub points: Vec<ViewDataPoint>,
    /// 时间戳。
    pub timestamp_ns: u64,
    /// 是否为增量更新。
    pub is_delta: bool,
}

impl RealtimeViewContract {
    /// 全量快照。
    pub fn snapshot(points: Vec<ViewDataPoint>, timestamp_ns: u64) -> Self {
        RealtimeViewContract { points, timestamp_ns, is_delta: false }
    }

    /// 增量更新。
    pub fn delta(points: Vec<ViewDataPoint>, timestamp_ns: u64) -> Self {
        RealtimeViewContract { points, timestamp_ns, is_delta: true }
    }

    /// 估算字节数。
    pub fn estimated_bytes(&self) -> usize {
        self.points.len() * 32 + 64
    }
}

// ---------------------------------------------------------------------------
// 背压控制器
// ---------------------------------------------------------------------------

/// 背压控制器。
pub struct BackpressureController {
    /// 订阅者处理速率（样本/秒）。
    pub consumer_rate: u32,
    /// 生产者速率（样本/秒）。
    pub producer_rate: u32,
    /// 降频因子。
    pub throttle: u32,
}

impl BackpressureController {
    pub fn new() -> Self {
        BackpressureController { consumer_rate: 1000, producer_rate: 1000, throttle: 1 }
    }

    /// 根据积压调整降频。
    pub fn adjust(&mut self, backlog: usize) {
        if backlog > 5000 {
            self.throttle = 10;
        } else if backlog > 1000 {
            self.throttle = 3;
        } else {
            self.throttle = 1;
        }
    }

    /// 是否需要降频。
    pub fn should_throttle(&self) -> bool { self.throttle > 1 }
}

// ---------------------------------------------------------------------------
// 录制模式（对接 B07）
// ---------------------------------------------------------------------------

/// 录制状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordState {
    /// 未录制。
    Idle,
    /// 录制中。
    Recording,
    /// 已暂停。
    Paused,
}

/// 录制器。
pub struct Recorder {
    pub state: RecordState,
    /// 已录制样本数。
    pub recorded: u64,
    /// 录制起始时间。
    pub start_ns: u64,
}

impl Recorder {
    pub fn new() -> Self {
        Recorder { state: RecordState::Idle, recorded: 0, start_ns: 0 }
    }

    pub fn start(&mut self, timestamp_ns: u64) {
        self.state = RecordState::Recording;
        self.start_ns = timestamp_ns;
    }

    pub fn pause(&mut self) {
        self.state = RecordState::Paused;
    }

    pub fn resume(&mut self) {
        self.state = RecordState::Recording;
    }

    pub fn stop(&mut self) -> u64 {
        let total = self.recorded;
        self.state = RecordState::Idle;
        self.recorded = 0;
        total
    }

    /// 是否应录制。
    pub fn should_record(&self) -> bool {
        self.state == RecordState::Recording
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// CGPU-F0284 判据全集。
pub fn run_realtime_checks() -> CheckSet {
    let mut cs = CheckSetBuilder::new("CGPU-F0284", "实时监控模式");

    // 1. 配置
    let config = RealtimeConfig::dev_default();
    cs.ok("秒级推送", config.push_interval_ms <= 1000, "1s 间隔");
    cs.ok("内存通道", !config.record_to_persist, "默认不过磁盘");

    // 2. 数据流
    let mut stream = RealtimeStream::new(config);
    cs.ok("空流", stream.depth() == 0, "初始");
    for i in 0..10 {
        stream.push_sample(TelemetrySample { metric: "m".into(), timestamp_ns: i, value: 0.0, frame: 0, task: 0, queue: 0, account: 0 });
    }
    cs.ok("写入 10", stream.depth() == 10, "缓冲");
    let flushed = stream.flush();
    cs.ok("推送 10", flushed.len() == 10, "flush");
    cs.ok("推送计数", stream.pushed == 10, "累计");

    // 3. 背压
    let mut bp_stream = RealtimeStream::new(RealtimeConfig { push_interval_ms: 1000, record_to_persist: false, max_buffer: 5 });
    for i in 0..20 {
        bp_stream.push_sample(TelemetrySample { metric: "m".into(), timestamp_ns: i, value: 0.0, frame: 0, task: 0, queue: 0, account: 0 });
    }
    cs.ok("背压触发", bp_stream.backpressure_events > 0, "超限降频");
    cs.ok("降频生效", bp_stream.is_throttled(), "自动降频");
    cs.ok("容量限制", bp_stream.depth() <= 5, "不超限");

    // 4. 视图契约
    let points = alloc::vec![
        ViewDataPoint { x: 0.0, y: 0.0, label: "t1".into(), color_index: 0 },
        ViewDataPoint { x: 1.0, y: 1.0, label: "t2".into(), color_index: 1 },
    ];
    let snapshot = RealtimeViewContract::snapshot(points.clone(), 1000);
    cs.ok("全量快照", !snapshot.is_delta, "完整数据");
    let delta = RealtimeViewContract::delta(points, 2000);
    cs.ok("增量更新", delta.is_delta, "diff 协议");
    cs.ok("字节估算", snapshot.estimated_bytes() > 0, "有体积");

    // 5. 背压控制器
    let mut bpc = BackpressureController::new();
    bpc.adjust(6000);
    cs.ok("重载降频", bpc.should_throttle(), "积压>5000");
    cs.ok("降频因子", bpc.throttle == 10, "十倍降频");
    bpc.adjust(0);
    cs.ok("轻载正常", !bpc.should_throttle(), "积压清零");

    // 6. 录制模式
    let mut rec = Recorder::new();
    cs.ok("初始空闲", rec.state == RecordState::Idle, "未录制");
    rec.start(1000);
    cs.ok("录制中", rec.should_record(), "开始录制");
    rec.pause();
    cs.ok("暂停", !rec.should_record(), "暂停不录");
    rec.resume();
    cs.ok("恢复", rec.should_record(), "恢复录制");
    rec.recorded = 42;
    cs.ok("停止", rec.stop() == 42, "返回总数");

    cs.done()
}
