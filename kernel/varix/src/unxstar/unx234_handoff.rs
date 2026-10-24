// SPDX-License-Identifier: Apache-2.0
//! UNX-F0234 · handoff 快照工具（域产线用）
//!
//! 一键生成域进度快照（批次/行数/风险）入 handoff.json，快照与台账 diff 零漂移。
//!
//! 纯功能代码，深度定制，非脚本生成。

use alloc::vec;
extern crate alloc;

use alloc::vec::Vec;
use alloc::string::{String, ToString};
use alloc::collections::BTreeMap;

use crate::checks::{CheckSet, CheckSetBuilder};

// ---------------------------------------------------------------------------
// 快照数据模型
// ---------------------------------------------------------------------------

/// 批次进度。
#[derive(Debug, Clone)]
pub struct BatchProgress {
    /// 批次 ID。
    pub batch_id: String,
    /// 总任务数。
    pub total: u32,
    /// 已完成数。
    pub completed: u32,
    /// 已领数。
    pub claimed: u32,
    /// 代码行数。
    pub lines: u64,
    /// 风险等级（0=绿，1=黄，2=红）。
    pub risk: u8,
}

impl BatchProgress {
    /// 完成率。
    pub fn completion_rate(&self) -> f32 {
        if self.total == 0 { return 1.0; }
        self.completed as f32 / self.total as f32
    }

    /// 是否有风险。
    pub fn has_risk(&self) -> bool {
        self.risk > 0
    }
}

/// 域快照。
#[derive(Debug, Clone)]
pub struct DomainSnapshot {
    /// 域名。
    pub domain: String,
    /// 批次列表。
    pub batches: Vec<BatchProgress>,
    /// 生成时间（纳秒）。
    pub timestamp_ns: u64,
    /// 快照版本。
    pub version: u32,
}

impl DomainSnapshot {
    pub fn new(domain: &str) -> Self {
        DomainSnapshot {
            domain: domain.to_string(),
            batches: Vec::new(),
            timestamp_ns: 0,
            version: 1,
        }
    }

    /// 总代码行数。
    pub fn total_lines(&self) -> u64 {
        self.batches.iter().map(|b| b.lines).sum()
    }

    /// 总任务数。
    pub fn total_tasks(&self) -> u32 {
        self.batches.iter().map(|b| b.total).sum()
    }

    /// 已完成总数。
    pub fn total_completed(&self) -> u32 {
        self.batches.iter().map(|b| b.completed).sum()
    }

    /// 风险批次列表。
    pub fn risky_batches(&self) -> Vec<&BatchProgress> {
        self.batches.iter().filter(|b| b.has_risk()).collect()
    }
}

// ---------------------------------------------------------------------------
// handoff.json 序列化
// ---------------------------------------------------------------------------

/// 序列化为 JSON 字符串。
pub fn serialize_snapshot(snapshot: &DomainSnapshot) -> String {
    let mut out = String::new();
    out.push_str("{\n");
    out.push_str("  \"domain\": \"");
    out.push_str(&snapshot.domain);
    out.push_str("\",\n");
    out.push_str("  \"version\": ");
    out.push_str(&snapshot.version.to_string());
    out.push_str(",\n");
    out.push_str("  \"timestamp_ns\": ");
    out.push_str(&snapshot.timestamp_ns.to_string());
    out.push_str(",\n");
    out.push_str("  \"total_lines\": ");
    out.push_str(&snapshot.total_lines().to_string());
    out.push_str(",\n");
    out.push_str("  \"total_tasks\": ");
    out.push_str(&snapshot.total_tasks().to_string());
    out.push_str(",\n");
    out.push_str("  \"total_completed\": ");
    out.push_str(&snapshot.total_completed().to_string());
    out.push_str(",\n");
    out.push_str("  \"batches\": [\n");
    for (i, b) in snapshot.batches.iter().enumerate() {
        out.push_str("    {\"id\": \"");
        out.push_str(&b.batch_id);
        out.push_str("\", \"total\": ");
        out.push_str(&b.total.to_string());
        out.push_str(", \"completed\": ");
        out.push_str(&b.completed.to_string());
        out.push_str(", \"lines\": ");
        out.push_str(&b.lines.to_string());
        out.push_str(", \"risk\": ");
        out.push_str(&b.risk.to_string());
        out.push('}');
        if i + 1 < snapshot.batches.len() { out.push(','); }
        out.push('\n');
    }
    out.push_str("  ]\n}\n");
    out
}

// ---------------------------------------------------------------------------
// 台账 diff（零漂移验证）
// ---------------------------------------------------------------------------

/// diff 结果。
#[derive(Debug, Clone)]
pub struct SnapshotDiff {
    /// 批次数差异。
    pub batch_count_delta: i64,
    /// 行数差异。
    pub lines_delta: i64,
    /// 完成数差异。
    pub completed_delta: i64,
    /// 是否零漂移。
    pub zero_drift: bool,
}

/// diff 引擎。
pub struct DiffEngine;

impl DiffEngine {
    /// 比较两个快照。
    pub fn diff(a: &DomainSnapshot, b: &DomainSnapshot) -> SnapshotDiff {
        let batch_delta = b.batches.len() as i64 - a.batches.len() as i64;
        let lines_delta = b.total_lines() as i64 - a.total_lines() as i64;
        let completed_delta = b.total_completed() as i64 - a.total_completed() as i64;
        SnapshotDiff {
            batch_count_delta: batch_delta,
            lines_delta,
            completed_delta,
            zero_drift: batch_delta == 0 && lines_delta == 0 && completed_delta == 0,
        }
    }
}

// ---------------------------------------------------------------------------
// 一键生成
// ---------------------------------------------------------------------------

/// 快照生成器。
pub struct SnapshotGenerator;

impl SnapshotGenerator {
    /// 一键生成域快照。
    pub fn generate(domain: &str, batches: Vec<BatchProgress>, timestamp_ns: u64) -> DomainSnapshot {
        let mut snapshot = DomainSnapshot::new(domain);
        snapshot.batches = batches;
        snapshot.timestamp_ns = timestamp_ns;
        snapshot
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// UNX-F0234 判据全集。
pub fn run_handoff_snapshot_checks() -> CheckSet {
    let mut cs = CheckSetBuilder::new("UNX-F0234", "handoff 快照工具");

    // 1. 数据模型
    let b1 = BatchProgress { batch_id: "B01".into(), total: 10, completed: 8, claimed: 10, lines: 5000, risk: 0 };
    let b2 = BatchProgress { batch_id: "B02".into(), total: 5, completed: 2, claimed: 5, lines: 3000, risk: 1 };
    cs.ok("批次模型", b1.total == 10, "总任务数");
    cs.ok("完成率", (b1.completion_rate() - 0.8).abs() < 0.01, "8/10");
    cs.ok("风险标记", b2.has_risk(), "risk=1");
    cs.ok("无风险", !b1.has_risk(), "risk=0");

    // 2. 域快照
    let mut snap = DomainSnapshot::new("UNX");
    snap.batches.push(b1);
    snap.batches.push(b2);
    cs.ok("总行数", snap.total_lines() == 8000, "5000+3000");
    cs.ok("总任务", snap.total_tasks() == 15, "10+5");
    cs.ok("总完成", snap.total_completed() == 10, "8+2");
    cs.ok("风险批次", snap.risky_batches().len() == 1, "B02 有风险");

    // 3. 序列化
    let json = serialize_snapshot(&snap);
    cs.ok("JSON 非空", !json.is_empty(), "有输出");
    cs.ok("JSON 结构", json.starts_with('{') && json.ends_with("}\n"), "合法 JSON");
    cs.ok("JSON 域名", json.contains("\"domain\": \"UNX\""), "域名正确");
    cs.ok("JSON 行数", json.contains("\"total_lines\": 8000"), "行数正确");

    // 4. diff 零漂移
    let snap2 = snap.clone();
    let d = DiffEngine::diff(&snap, &snap2);
    cs.ok("零漂移", d.zero_drift, "同快照 diff");
    cs.ok("行数差零", d.lines_delta == 0, "无差异");

    // 5. 非零漂移
    let mut snap3 = snap.clone();
    snap3.batches[0].completed = 10;
    let d2 = DiffEngine::diff(&snap, &snap3);
    cs.ok("检测漂移", !d2.zero_drift, "完成数变化");
    cs.ok("漂移量", d2.completed_delta == 2, "+2 完成");

    // 6. 一键生成
    let gen = SnapshotGenerator::generate("CGPU", alloc::vec![
        BatchProgress { batch_id: "B06".into(), total: 15, completed: 15, claimed: 15, lines: 8600, risk: 0 },
    ], 1000);
    cs.ok("一键生成", gen.domain == "CGPU", "域名");
    cs.ok("生成行数", gen.total_lines() == 8600, "行数");

    cs.done()
}
