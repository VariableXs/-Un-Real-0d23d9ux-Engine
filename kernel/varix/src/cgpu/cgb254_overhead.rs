//! CGPU-F0254 · 开销控制与遥测（cgb254_overhead）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F0254`
//! **判据原文摘录**：检测频率自适应（等待图规模驱动——小图高频/大图降频）、
//! 遥测五指标（死锁事件数 / 误报率 / 解除成功率 / 检测耗时 P95 / 检测器重启计数
//! ——五指标进调度遥测）、误报率目标 <5% 且逐例归档分析。
//!
//! ## 自适应频率
//!
//! 检测频率不是常数——图小检测快、图大检测慢。若大图仍高频检测，
//! 检测器自身吃掉大量 CPU。
//!
//! 自适应策略：
//! - 节点数 < 128：增量检测频率 = 1（每边事件都检测）；
//! - 128 ≤ 节点数 < 512：频率 = 2（每 2 个事件检测一次）；
//! - 节点数 ≥ 512：频率 = 4（每 4 个事件检测一次）+ 全量周期加倍。
//!
//! 降频必须**显性**——`degradation_notice` 非空，用户知道精度下降。
//!
//! ## 五指标
//!
//! | 指标 | 含义 | 目标 |
//! |---|---|---|
//! | 死锁事件数 | 检出的死锁次数 | — |
//! | 误报率 | Dismissed / Settled | <5% |
//! | 解除成功率 | 验证通过 / 总解除 | >95% |
//! | 检测耗时 P95 | 95 分位检测耗时 | <1ms |
//! | 检测器重启计数 | 崩溃重启次数 | 0 |

use alloc::format;
use alloc::string::String;
use alloc::vec;
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 诊断码（0xCC10 起）
// ---------------------------------------------------------------------------

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0xCC10;
/// 遥测缓冲满。
pub const E_BUFFER_FULL: u16 = SEG_BASE + 1;

// ---------------------------------------------------------------------------
// 容量
// ---------------------------------------------------------------------------

/// 遥测缓冲容量（用于 P95 计算）。
pub const TELEMETRY_BUFFER: usize = 256;
/// 误报率目标（百分比）。
pub const FP_RATE_TARGET: f32 = 5.0;
/// 解除成功率目标（百分比）。
pub const RESOLVE_RATE_TARGET: f32 = 95.0;

// ---------------------------------------------------------------------------
// 自适应频率
// ---------------------------------------------------------------------------

/// 频率档位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FreqLevel {
    /// 高频（每事件检测）。
    High,
    /// 中频（每 2 事件）。
    Medium,
    /// 低频（每 4 事件）。
    Low,
}

impl FreqLevel {
    /// 事件间隔。
    pub const fn interval(self) -> u32 {
        match self {
            FreqLevel::High => 1,
            FreqLevel::Medium => 2,
            FreqLevel::Low => 4,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            FreqLevel::High => "high",
            FreqLevel::Medium => "medium",
            FreqLevel::Low => "low",
        }
    }
}

/// 自适应频率控制器。
pub struct AdaptiveFrequency {
    pub level: FreqLevel,
    /// 精度下降告知。
    pub degradation_notice: alloc::string::String,
    /// 遥测：降频次数。
    pub throttles: u64,
}

impl AdaptiveFrequency {
    pub fn new() -> AdaptiveFrequency {
        AdaptiveFrequency {
            level: FreqLevel::High,
            degradation_notice: alloc::string::String::new(),
            throttles: 0,
        }
    }

    /// 根据图规模调频。
    pub fn adjust(&mut self, node_count: usize) {
        let new_level = if node_count < 128 {
            FreqLevel::High
        } else if node_count < 512 {
            FreqLevel::Medium
        } else {
            FreqLevel::Low
        };
        if new_level != self.level {
            if (new_level as u8) > (self.level as u8) {
                self.throttles += 1;
                self.degradation_notice = alloc::string::String::from(match new_level {
                    FreqLevel::Medium => "图规模增大，检测降频至 1/2，精度略有下降",
                    FreqLevel::Low => "图规模增大，检测降频至 1/4，精度明显下降",
                    _ => "",
                });
            } else {
                self.degradation_notice = alloc::string::String::new();
            }
            self.level = new_level;
        }
    }

    /// 是否有精度告知。
    pub fn has_degradation(&self) -> bool {
        !self.degradation_notice.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 遥测采集
// ---------------------------------------------------------------------------

/// 五指标遥测。
pub struct Telemetry {
    /// ① 死锁事件数。
    pub deadlock_events: u64,
    /// ② 误报数。
    pub false_positives: u64,
    /// ② 总判定数（分母）。
    pub total_verdicts: u64,
    /// ③ 解除总数。
    pub resolutions: u64,
    /// ③ 解除验证通过数。
    pub resolve_passed: u64,
    /// ④ 检测耗时样本（tick）。
    detect_times: Vec<u32>,
    /// ⑤ 检测器重启计数。
    pub restart_count: u64,
}

impl Telemetry {
    pub fn new() -> Telemetry {
        Telemetry {
            deadlock_events: 0,
            false_positives: 0,
            total_verdicts: 0,
            resolutions: 0,
            resolve_passed: 0,
            detect_times: Vec::new(),
            restart_count: 0,
        }
    }

    /// 记录一次检测耗时。
    pub fn record_detect_time(&mut self, ticks: u32) {
        if self.detect_times.len() >= TELEMETRY_BUFFER {
            self.detect_times.remove(0);
        }
        self.detect_times.push(ticks);
    }

    /// 误报率（%）。
    pub fn false_positive_rate(&self) -> f32 {
        if self.total_verdicts == 0 {
            return 0.0;
        }
        self.false_positives as f32 * 100.0 / self.total_verdicts as f32
    }

    /// 解除成功率（%）。
    pub fn resolve_success_rate(&self) -> f32 {
        if self.resolutions == 0 {
            return 100.0;
        }
        self.resolve_passed as f32 * 100.0 / self.resolutions as f32
    }

    /// 检测耗时 P95。
    pub fn detect_p95(&self) -> u32 {
        if self.detect_times.is_empty() {
            return 0;
        }
        let mut sorted = self.detect_times.clone();
        sorted.sort_unstable();
        let idx = (sorted.len() as f32 * 0.95) as usize;
        sorted[idx.min(sorted.len() - 1)]
    }

    /// 五指标快照。
    pub fn snapshot(&self) -> [(alloc::string::String, alloc::string::String); 5] {
        [
            (
                alloc::string::String::from("deadlock_events"),
                alloc::format!("{}", self.deadlock_events),
            ),
            (
                alloc::string::String::from("false_positive_rate"),
                alloc::format!("{:.1}%", self.false_positive_rate()),
            ),
            (
                alloc::string::String::from("resolve_success_rate"),
                alloc::format!("{:.1}%", self.resolve_success_rate()),
            ),
            (
                alloc::string::String::from("detect_p95"),
                alloc::format!("{}tick", self.detect_p95()),
            ),
            (
                alloc::string::String::from("restart_count"),
                alloc::format!("{}", self.restart_count),
            ),
        ]
    }

    /// 是否达标。
    pub fn meets_targets(&self) -> CheckSet {
        let mut cs = CheckSet::new("cgb254_targets");
        let fpr = self.false_positive_rate();
        cs.add(
            "误报率<5%",
            fpr < FP_RATE_TARGET,
            alloc::format!("{:.1}%", fpr).leak(),
        );
        let rsr = self.resolve_success_rate();
        cs.add(
            "解除成功率>95%",
            rsr >= RESOLVE_RATE_TARGET,
            alloc::format!("{:.1}%", rsr).leak(),
        );
        cs.add(
            "重启为零",
            self.restart_count == 0,
            "restart==0",
        );
        cs
    }
}

// ---------------------------------------------------------------------------
// 逐例归档
// ---------------------------------------------------------------------------

/// 误报归档条目。
#[derive(Clone, Debug)]
pub struct FpArchive {
    /// 环签名。
    pub signature: alloc::vec::Vec<u32>,
    /// 误报原因。
    pub reason: alloc::string::String,
    /// 发生 tick。
    pub at: u32,
    /// 分析结论。
    pub analysis: alloc::string::String,
}

/// 误报归档器（逐例分析）。
pub struct FpArchiver {
    pub archive: Vec<FpArchive>,
}

impl FpArchiver {
    pub fn new() -> FpArchiver {
        FpArchiver {
            archive: Vec::new(),
        }
    }

    /// 归档一例误报。
    pub fn archive(
        &mut self,
        signature: alloc::vec::Vec<u32>,
        reason: &str,
        analysis: &str,
        at: u32,
    ) {
        self.archive.push(FpArchive {
            signature,
            reason: alloc::string::String::from(reason),
            at,
            analysis: alloc::string::String::from(analysis),
        });
    }

    pub fn count(&self) -> usize {
        self.archive.len()
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// CGPU-F0254 域自检。
pub fn run_cgb254_checks() -> CheckSet {
    let mut cs = CheckSet::new("cgb254_overhead");
    let mut af = AdaptiveFrequency::new();
    let mut tel = Telemetry::new();
    let mut archiver = FpArchiver::new();

    // --- 自适应频率 ---
    af.adjust(50);
    cs.add(
        "小图高频",
        af.level == FreqLevel::High,
        "<128 high",
    );
    af.adjust(200);
    cs.add(
        "中图中频",
        af.level == FreqLevel::Medium,
        "128-512 medium",
    );
    cs.add(
        "降频显性告知",
        af.has_degradation(),
        "notice 非空",
    );
    af.adjust(600);
    cs.add(
        "大图低频",
        af.level == FreqLevel::Low,
        "≥512 low",
    );
    af.adjust(50);
    cs.add(
        "小图恢复",
        af.level == FreqLevel::High,
        "恢复高频",
    );
    cs.add(
        "恢复清除告知",
        !af.has_degradation(),
        "notice 清空",
    );

    // --- 五指标 ---
    tel.deadlock_events = 10;
    tel.total_verdicts = 100;
    tel.false_positives = 3;
    tel.resolutions = 10;
    tel.resolve_passed = 10;
    tel.restart_count = 0;
    for i in 0..20u32 {
        tel.record_detect_time(100 + i);
    }
    let snap = tel.snapshot();
    cs.add(
        "五指标快照",
        snap.len() == 5,
        "5 metrics",
    );
    cs.add(
        "误报率 3%",
        tel.false_positive_rate() < FP_RATE_TARGET,
        "3%<5%",
    );
    cs.add(
        "解除全成",
        tel.resolve_success_rate() == 100.0,
        "100%>95%",
    );
    cs.add(
        "P95 可算",
        tel.detect_p95() > 0,
        "p95>0",
    );

    // --- 目标达标 ---
    let targets = tel.meets_targets();
    let mut all_met = true;
    for i in 0..targets.len() {
        if let Some(c) = targets.get(i) {
            if !c.passed {
                all_met = false;
            }
        }
    }
    cs.add("三目标达标", all_met, "FP<5%/RS>95%/restart=0");

    // --- 误报归档 ---
    archiver.archive(
        alloc::vec![1, 2, 3],
        "慢任务误判",
        "心跳活跃但阈值过严",
        100,
    );
    archiver.archive(
        alloc::vec![4, 5],
        "条件变量误判",
        "广播面建模错误",
        200,
    );
    cs.add(
        "逐例归档",
        archiver.count() == 2,
        "2 archives",
    );

    // --- 误报率超标的路径 ---
    let mut tel_bad = Telemetry::new();
    tel_bad.total_verdicts = 10;
    tel_bad.false_positives = 8;
    cs.add(
        "超标可检",
        tel_bad.false_positive_rate() > FP_RATE_TARGET,
        "80%>5%",
    );

    cs
}
