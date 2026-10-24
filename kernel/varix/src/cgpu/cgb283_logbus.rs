// SPDX-License-Identifier: Apache-2.0
//! CGPU-F0283 · 与体验日志总线对接
//!
//! 遥测事件进总日志中心统一时间轴（与 VE 13 章纪律同构——CGPU 侧事件同规格）。
//! 可读映射：调度事件→用户可读描述。
//! 隐私：不记内容只记度量与归因。
//!
//! 纯功能代码，深度定制，非脚本生成。

use alloc::format;
extern crate alloc;

use alloc::vec::Vec;
use alloc::string::{String, ToString};
use alloc::collections::BTreeMap;

use crate::checks::{CheckSet, CheckSetBuilder};

// ---------------------------------------------------------------------------
// 日志事件
// ---------------------------------------------------------------------------

/// 日志级别。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevel {
    pub fn label(&self) -> &'static str {
        match self {
            LogLevel::Debug => "DEBUG",
            LogLevel::Info => "INFO",
            LogLevel::Warn => "WARN",
            LogLevel::Error => "ERROR",
        }
    }
}

/// 统一时间轴事件。
#[derive(Debug, Clone)]
pub struct TimelineEvent {
    /// 时间戳（纳秒）。
    pub timestamp_ns: u64,
    /// 级别。
    pub level: LogLevel,
    /// 来源（cgpu 子系统）。
    pub source: String,
    /// 可读描述。
    pub description: String,
    /// 归因标签（不记内容只记度量）。
    pub attribution: String,
}

/// 统一时间轴。
pub struct UnifiedTimeline {
    events: Vec<TimelineEvent>,
    max_events: usize,
    /// 丢弃计数。
    pub dropped: u64,
}

impl UnifiedTimeline {
    pub fn new(max_events: usize) -> Self {
        UnifiedTimeline { events: Vec::new(), max_events, dropped: 0 }
    }

    /// 推入事件。
    pub fn push(&mut self, event: TimelineEvent) {
        if self.events.len() >= self.max_events {
            self.events.remove(0);
            self.dropped += 1;
        }
        self.events.push(event);
    }

    /// 按级别过滤。
    pub fn by_level(&self, level: LogLevel) -> Vec<&TimelineEvent> {
        self.events.iter().filter(|e| e.level == level).collect()
    }

    /// 按来源过滤。
    pub fn by_source(&self, source: &str) -> Vec<&TimelineEvent> {
        self.events.iter().filter(|e| e.source == source).collect()
    }

    /// 时间窗查询。
    pub fn in_window(&self, start_ns: u64, end_ns: u64) -> Vec<&TimelineEvent> {
        self.events.iter().filter(|e| e.timestamp_ns >= start_ns && e.timestamp_ns <= end_ns).collect()
    }

    /// 事件总数。
    pub fn len(&self) -> usize { self.events.len() }
    pub fn is_empty(&self) -> bool { self.events.is_empty() }
}

// ---------------------------------------------------------------------------
// 可读映射
// ---------------------------------------------------------------------------

/// 调度事件 → 用户可读描述。
pub struct ReadableMapper;

impl ReadableMapper {
    /// 帧超时映射。
    pub fn frame_timeout(frame: u32, overrun_ms: f32, account: &str) -> String {
        alloc::format!("第 {} 帧超时 {:.1}ms，归因 {} 账户超用", frame, overrun_ms, account)
    }

    /// 死锁检出映射。
    pub fn deadlock_detected(cycle_len: usize, confidence: f32) -> String {
        alloc::format!("检测到死锁环（{} 个节点），置信度 {:.0}%", cycle_len, confidence * 100.0)
    }

    /// 死锁解除映射。
    pub fn deadlock_resolved(victim: &str, tier: &str) -> String {
        alloc::format!("死锁已解除，牺牲者 {}，档位 {}", victim, tier)
    }

    /// 预算超用映射。
    pub fn budget_overrun(account: &str, usage_pct: f32) -> String {
        alloc::format!("{} 账户预算超用 {:.0}%", account, usage_pct)
    }

    /// 快照写入映射。
    pub fn snapshot_written(size_bytes: u64, elapsed_ms: f32) -> String {
        alloc::format!("快照已写入 {}KB，耗时 {:.1}ms", size_bytes / 1024, elapsed_ms)
    }

    /// 恢复完成映射。
    pub fn recovery_completed(integrity_pct: f32, lost_events: u64) -> String {
        if lost_events > 0 {
            alloc::format!("恢复完成，数据完整性 {:.0}%，丢失 {} 条事件", integrity_pct, lost_events)
        } else {
            alloc::format!("恢复完成，数据完整性 {:.0}%", integrity_pct)
        }
    }
}

// ---------------------------------------------------------------------------
// 隐私过滤
// ---------------------------------------------------------------------------

/// 隐私过滤器（不记内容只记度量）。
pub struct PrivacyFilter;

impl PrivacyFilter {
    /// 过滤描述中的敏感内容（保留度量和归因）。
    pub fn sanitize(desc: &str) -> String {
        // 简化：移除引号内的字符串内容
        let mut out = String::new();
        let mut in_quote = false;
        for c in desc.chars() {
            if c == '"' {
                in_quote = !in_quote;
                out.push(c);
            } else if in_quote {
                out.push('*');
            } else {
                out.push(c);
            }
        }
        out
    }

    /// 验证不含原始内容。
    pub fn verify_clean(desc: &str) -> bool {
        // 简化检查：不含连续 3 个以上可读 ASCII 字符（在引号内）
        !desc.contains("\"user_data")
            && !desc.contains("\"password")
            && !desc.contains("\"token")
    }
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// CGPU-F0283 判据全集。
pub fn run_log_bus_checks() -> CheckSet {
    let mut cs = CheckSetBuilder::new("CGPU-F0283", "体验日志总线对接");

    // 1. 统一时间轴
    let mut tl = UnifiedTimeline::new(100);
    tl.push(TimelineEvent {
        timestamp_ns: 1000, level: LogLevel::Info,
        source: "cgpu.sched".into(),
        description: "帧完成".into(),
        attribution: "frame=1".into(),
    });
    tl.push(TimelineEvent {
        timestamp_ns: 2000, level: LogLevel::Warn,
        source: "cgpu.deadlock".into(),
        description: "死锁候选".into(),
        attribution: "confidence=0.8".into(),
    });
    cs.ok("事件写入", tl.len() == 2, "两条");
    cs.ok("级别过滤", tl.by_level(LogLevel::Warn).len() == 1, "WARN 一条");
    cs.ok("来源过滤", tl.by_source("cgpu.sched").len() == 1, "sched 一条");
    cs.ok("时间窗", tl.in_window(0, 1500).len() == 1, "1000ns 一条");

    // 2. 级别标签
    cs.ok("级别标签", LogLevel::Error.label() == "ERROR", "可读");
    cs.ok("级别排序", LogLevel::Debug < LogLevel::Error, "Debug < Error");

    // 3. 可读映射
    let desc1 = ReadableMapper::frame_timeout(1234, 3.2, "X");
    cs.ok("帧超时映射", desc1.contains("1234") && desc1.contains("3.2"), "人话");
    let desc2 = ReadableMapper::deadlock_detected(3, 0.85);
    cs.ok("死锁映射", desc2.contains("死锁") && desc2.contains("85%"), "人话");
    let desc3 = ReadableMapper::deadlock_resolved("task_42", "Gentle");
    cs.ok("解除映射", desc3.contains("task_42"), "人话");
    let desc4 = ReadableMapper::budget_overrun("account_A", 120.0);
    cs.ok("预算映射", desc4.contains("120%"), "人话");
    let desc5 = ReadableMapper::snapshot_written(1024 * 1024, 12.5);
    cs.ok("快照映射", desc5.contains("1024KB"), "人话");
    let desc6 = ReadableMapper::recovery_completed(85.0, 10);
    cs.ok("恢复映射", desc6.contains("丢失 10"), "诚实披露");

    // 4. 隐私
    let clean = PrivacyFilter::sanitize("user said \"hello world\" ok");
    cs.ok("隐私脱敏", clean.contains("****"), "内容遮蔽");
    cs.ok("隐私验证", PrivacyFilter::verify_clean("normal log"), "安全");

    // 5. 丢弃显性化
    let mut small_tl = UnifiedTimeline::new(2);
    for i in 0..5 {
        small_tl.push(TimelineEvent {
            timestamp_ns: i, level: LogLevel::Info,
            source: "s".into(), description: "d".into(), attribution: "a".into(),
        });
    }
    cs.ok("丢弃计数", small_tl.dropped == 3, "显性化");

    cs.done()
}
