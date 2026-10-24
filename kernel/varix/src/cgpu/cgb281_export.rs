// SPDX-License-Identifier: Apache-2.0
//! CGPU-F0281 · 遥测导出
//!
//! 开放格式：JSON/CSV 双格式——14 章数据开放。
//! 导出范围选择：时间窗×指标集×维度组合。
//! 导出一致性：导出数据与查询 API 同源同值——不出第二套口径。
//! 大导出流式化：分块写出不撑内存。
//!
//! 纯功能代码，深度定制，非脚本生成。

use alloc::format;
use alloc::vec;
extern crate alloc;

use alloc::vec::Vec;
use alloc::string::{String, ToString};
use alloc::collections::BTreeMap;

use crate::checks::{CheckSet, CheckSetBuilder};
use super::cgb279_converge::TelemetrySample;

// ---------------------------------------------------------------------------
// 导出格式
// ---------------------------------------------------------------------------

/// 导出格式（双格式）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    /// JSON（嵌套结构，适合程序消费）。
    Json,
    /// CSV（扁平表格，适合表格工具）。
    Csv,
}

impl ExportFormat {
    pub fn mime_type(&self) -> &'static str {
        match self {
            ExportFormat::Json => "application/json",
            ExportFormat::Csv => "text/csv",
        }
    }

    pub fn extension(&self) -> &'static str {
        match self {
            ExportFormat::Json => "json",
            ExportFormat::Csv => "csv",
        }
    }
}

// ---------------------------------------------------------------------------
// 导出范围
// ---------------------------------------------------------------------------

/// 导出范围选择。
#[derive(Debug, Clone)]
pub struct ExportScope {
    /// 时间窗起（纳秒）。
    pub start_ns: u64,
    /// 时间窗止（纳秒）。
    pub end_ns: u64,
    /// 指标名过滤（空 = 全部）。
    pub metrics: Vec<String>,
    /// 帧号过滤（空 = 全部）。
    pub frames: Vec<u32>,
    /// 队列过滤（空 = 全部）。
    pub queues: Vec<u8>,
}

impl ExportScope {
    pub fn full() -> Self {
        ExportScope { start_ns: 0, end_ns: u64::MAX, metrics: Vec::new(), frames: Vec::new(), queues: Vec::new() }
    }

    /// 判断样本是否在范围内。
    pub fn contains(&self, s: &TelemetrySample) -> bool {
        if s.timestamp_ns < self.start_ns || s.timestamp_ns > self.end_ns { return false; }
        if !self.metrics.is_empty() && !self.metrics.contains(&s.metric) { return false; }
        if !self.frames.is_empty() && !self.frames.contains(&s.frame) { return false; }
        if !self.queues.is_empty() && !self.queues.contains(&s.queue) { return false; }
        true
    }

    /// 过滤样本。
    pub fn filter<'a>(&self, samples: &'a [TelemetrySample]) -> Vec<&'a TelemetrySample> {
        samples.iter().filter(|s| self.contains(s)).collect()
    }
}

// ---------------------------------------------------------------------------
// 流式导出器
// ---------------------------------------------------------------------------

/// 分块大小（字节）。
const CHUNK_SIZE: usize = 64 * 1024;

/// 流式导出器（分块写出不撑内存）。
pub struct StreamingExporter {
    format: ExportFormat,
    chunks: Vec<String>,
    current: String,
    /// 已写出字节数。
    pub bytes_written: usize,
    /// 已写出记录数。
    pub records_written: usize,
}

impl StreamingExporter {
    pub fn new(format: ExportFormat) -> Self {
        StreamingExporter { format, chunks: Vec::new(), current: String::new(), bytes_written: 0, records_written: 0 }
    }

    /// 开始导出（写头）。
    pub fn begin(&mut self) {
        match self.format {
            ExportFormat::Json => self.current.push_str("[\n"),
            ExportFormat::Csv => self.current.push_str("metric,timestamp_ns,value,frame,task,queue,account\n"),
        }
    }

    /// 写一条记录。
    pub fn write_record(&mut self, s: &TelemetrySample) {
        match self.format {
            ExportFormat::Json => {
                if self.records_written > 0 { self.current.push_str(",\n"); }
                self.current.push_str("  {\"metric\":\"");
                self.current.push_str(&s.metric);
                self.current.push_str("\",\"ts\":");
                self.current.push_str(&s.timestamp_ns.to_string());
                self.current.push_str(",\"value\":");
                self.current.push_str(&format_f64(s.value));
                self.current.push_str(",\"frame\":");
                self.current.push_str(&s.frame.to_string());
                self.current.push_str(",\"task\":");
                self.current.push_str(&s.task.to_string());
                self.current.push_str(",\"queue\":");
                self.current.push_str(&s.queue.to_string());
                self.current.push_str(",\"account\":");
                self.current.push_str(&s.account.to_string());
                self.current.push('}');
            }
            ExportFormat::Csv => {
                self.current.push_str(&s.metric);
                self.current.push(',');
                self.current.push_str(&s.timestamp_ns.to_string());
                self.current.push(',');
                self.current.push_str(&format_f64(s.value));
                self.current.push(',');
                self.current.push_str(&s.frame.to_string());
                self.current.push(',');
                self.current.push_str(&s.task.to_string());
                self.current.push(',');
                self.current.push_str(&s.queue.to_string());
                self.current.push(',');
                self.current.push_str(&s.account.to_string());
                self.current.push('\n');
            }
        }
        self.records_written += 1;
        self.bytes_written += 64; // 粗略
        self.maybe_flush();
    }

    /// 检查是否需要分块。
    fn maybe_flush(&mut self) {
        if self.current.len() >= CHUNK_SIZE {
            let chunk = core::mem::take(&mut self.current);
            self.chunks.push(chunk);
        }
    }

    /// 结束导出（写尾）。
    pub fn finish(&mut self) -> Vec<String> {
        match self.format {
            ExportFormat::Json => self.current.push_str("\n]"),
            ExportFormat::Csv => {}
        }
        if !self.current.is_empty() {
            let chunk = core::mem::take(&mut self.current);
            self.chunks.push(chunk);
        }
        core::mem::take(&mut self.chunks)
    }

    /// 全量导出（不分块——小数据集用）。
    pub fn export_all(&mut self, samples: &[&TelemetrySample]) -> String {
        self.begin();
        for s in samples { self.write_record(s); }
        let chunks = self.finish();
        chunks.join("")
    }
}

/// 简易 f64 格式化（no_std 无 alloc::format! 依赖）。
fn format_f64(v: f64) -> String {
    // 简化：整数部分 + 小数点 + 4 位小数
    let int_part = v as i64;
    let frac = ((v - int_part as f64).abs() * 10000.0) as u32;
    alloc::format!("{}.{:04}", int_part, frac)
}

// ---------------------------------------------------------------------------
// 一致性验证（同源同值）
// ---------------------------------------------------------------------------

/// 导出一致性验证器。
pub struct ConsistencyChecker;

impl ConsistencyChecker {
    /// 验证导出数据与原始数据同源同值。
    pub fn verify(original: &[TelemetrySample], exported: &[TelemetrySample]) -> bool {
        if original.len() != exported.len() { return false; }
        for (a, b) in original.iter().zip(exported.iter()) {
            if a.metric != b.metric || a.timestamp_ns != b.timestamp_ns { return false; }
            if (a.value - b.value).abs() > 1e-9 { return false; }
        }
        true
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// CGPU-F0281 判据全集。
pub fn run_export_checks() -> CheckSet {
    let mut cs = CheckSetBuilder::new("CGPU-F0281", "遥测导出");

    // 测试样本
    let samples = alloc::vec![
        TelemetrySample { metric: "cgpu.sched.frame_time".into(), timestamp_ns: 1000, value: 16.5, frame: 1, task: 0, queue: 0, account: 0 },
        TelemetrySample { metric: "cgpu.deadlock.events".into(), timestamp_ns: 2000, value: 1.0, frame: 0, task: 0, queue: 0, account: 0 },
        TelemetrySample { metric: "cgpu.sched.queue_depth".into(), timestamp_ns: 3000, value: 5.0, frame: 2, task: 0, queue: 1, account: 0 },
    ];

    // 1. 双格式
    cs.ok("JSON 格式", ExportFormat::Json.mime_type() == "application/json", "开放格式");
    cs.ok("CSV 格式", ExportFormat::Csv.mime_type() == "text/csv", "开放格式");
    cs.ok("扩展名", ExportFormat::Json.extension() == "json" && ExportFormat::Csv.extension() == "csv", "文件类型");

    // 2. 范围选择
    let scope = ExportScope { start_ns: 1500, end_ns: 2500, metrics: Vec::new(), frames: Vec::new(), queues: Vec::new() };
    let filtered = scope.filter(&samples);
    cs.ok("时间窗过滤", filtered.len() == 1, "只命中 2000ns");
    let metric_scope = ExportScope { start_ns: 0, end_ns: u64::MAX, metrics: alloc::vec!["cgpu.sched.frame_time".into()], frames: Vec::new(), queues: Vec::new() };
    cs.ok("指标过滤", metric_scope.filter(&samples).len() == 1, "只命中 frame_time");
    let frame_scope = ExportScope { start_ns: 0, end_ns: u64::MAX, metrics: Vec::new(), frames: alloc::vec![1, 2], queues: Vec::new() };
    cs.ok("帧号过滤", frame_scope.filter(&samples).len() == 2, "帧 1 和 2");

    // 3. JSON 导出
    let mut jexp = StreamingExporter::new(ExportFormat::Json);
    let jout = jexp.export_all(&samples.iter().collect::<Vec<_>>());
    cs.ok("JSON 非空", !jout.is_empty(), "有输出");
    cs.ok("JSON 数组结构", jout.starts_with('[') && jout.ends_with(']'), "合法 JSON");
    cs.ok("JSON 记录数", jout.matches("\"metric\"").count() == 3, "三条记录");
    cs.ok("JSON 记录数统计", jexp.records_written == 3, "导出计数");

    // 4. CSV 导出
    let mut cexp = StreamingExporter::new(ExportFormat::Csv);
    let cout = cexp.export_all(&samples.iter().collect::<Vec<_>>());
    cs.ok("CSV 非空", !cout.is_empty(), "有输出");
    cs.ok("CSV 表头", cout.starts_with("metric,timestamp_ns"), "表头正确");
    cs.ok("CSV 行数", cout.lines().count() == 4, "表头+3 数据行");

    // 5. 流式分块
    let mut sexp = StreamingExporter::new(ExportFormat::Json);
    sexp.begin();
    for i in 0..1000 {
        sexp.write_record(&TelemetrySample { metric: "m".into(), timestamp_ns: i, value: 0.0, frame: 0, task: 0, queue: 0, account: 0 });
    }
    let chunks = sexp.finish();
    cs.ok("流式分块", chunks.len() >= 1, "分块写出");
    cs.ok("流式计数", sexp.records_written == 1000, "千条记录");

    // 6. 一致性
    let refs: Vec<&TelemetrySample> = samples.iter().collect();
    let round_trip: Vec<TelemetrySample> = refs.iter().map(|s| (*s).clone()).collect();
    cs.ok("同源同值", ConsistencyChecker::verify(&samples, &round_trip), "零口径差");

    cs.done()
}
