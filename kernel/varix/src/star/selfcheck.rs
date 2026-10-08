//! F062 性能自检报告 · 完整设计（STAR I 主册 G-B-22）。
//!
//! **判据（主册）**：阈值设定经实机标定（正常机器 30 天零误报）；
//! 异常检出率：注入 3 类慢故障全部捕获。
//!
//! **设计要点（主册）**：
//! - 开机 10 分钟静默采样生成健康快照：帧率 P95 / IO 延迟 P99 / 调度
//!   超标次数 / 唤醒统计四项，超阈值自动转诊断中心工单——性能问题在
//!   用户感知之前先被系统自己看见；
//! - 四项阈值初值：帧 P95 >17ms / IO P99 >50ms / 调度超标 >5 次 /
//!   唤醒 >200 次/分钟——每项可关（用户不想要体检就安静）；
//! - 开机后用户立即高强度使用 → 采样避开（首个空闲 10 分钟窗口才采）；
//! - 采样自身影响（<0.1% CPU 预算）超限 → 降频采样；
//! - 快照每日一份保留 30 天；工单引用快照（不复制数据）；
//! - 通知合批：同日多条快照异常合并一条；工单不弹窗只进中心。
//!
//! 依赖锚点 F042（归因数据——工单引用归因 ID，不复制）、F049（空转
//! 窗口——空闲判定由上层供给）。

use crate::checks::CheckSet;
use crate::star::sbase::RingLog;

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格与阈值
// ---------------------------------------------------------------------------

/// 帧 P95 阈值（微秒）：>17ms 异常。
pub const TH_FRAME_P95_US: u64 = 17_000;

/// IO P99 阈值（微秒）：>50ms 异常。
pub const TH_IO_P99_US: u64 = 50_000;

/// 调度超标次数阈值：>5 次/窗口异常。
pub const TH_SCHED_OVERRUNS: u64 = 5;

/// 唤醒次数阈值：>200 次/分钟异常。
pub const TH_WAKES_PER_MIN: u64 = 200;

/// 采样窗（分钟）：首个空闲 10 分钟窗口。
pub const SAMPLE_WINDOW_MIN: u64 = 10;

/// 采样自预算（百万分比）：>0.1% CPU → 降频采样。
pub const SELF_BUDGET_PPM: u64 = 100;

/// 快照保留（天）：30。
pub const SNAP_RETENTION_DAYS: usize = 30;

/// 四项指标位（bit 掩码——可关：用户不想要体检就安静）。
pub const M_FRAME: u8 = 1 << 0;
pub const M_IO: u8 = 1 << 1;
pub const M_SCHED: u8 = 1 << 2;
pub const M_WAKE: u8 = 1 << 3;
/// 全开。
pub const M_ALL: u8 = M_FRAME | M_IO | M_SCHED | M_WAKE;

/// 一分钟采样（上层从账本/归因面采集后喂入）。
#[derive(Clone, Copy, Debug, Default)]
pub struct MinSample {
    /// 该分钟帧耗时 P95（微秒）。
    pub frame_p95_us: u64,
    /// 该分钟 IO 延迟 P99（微秒）。
    pub io_p99_us: u64,
    /// 该分钟调度超标次数。
    pub sched_overruns: u64,
    /// 该分钟唤醒次数。
    pub wakes: u64,
    /// 该分钟采样自身开销（ppm）。
    pub self_cost_ppm: u64,
    /// 该分钟是否空闲（F049 空转窗口供给）。
    pub idle: bool,
}

/// 一份健康快照（10 分钟窗聚合）。
#[derive(Clone, Copy, Debug)]
pub struct HealthSnapshot {
    /// 快照日序号。
    pub day: u64,
    /// 起始分钟戳。
    pub from_min: u64,
    /// 窗内帧 P95 最大值（微秒）。
    pub frame_p95_us: u64,
    /// 窗内 IO P99 最大值（微秒）。
    pub io_p99_us: u64,
    /// 窗内调度超标合计。
    pub sched_overruns: u64,
    /// 窗内唤醒峰值（次/分钟）。
    pub wakes_peak: u64,
    /// 异常位掩码（bit 与 M_* 对应）。
    pub anomaly_mask: u8,
    /// 证据链引用：F042 归因 ID（不复制数据——工单只带指针）。
    pub attribution_ref: u64,
}

impl HealthSnapshot {
    /// 快照是否异常。
    pub fn is_anomaly(&self) -> bool {
        self.anomaly_mask != 0
    }
}

// ---------------------------------------------------------------------------
// 工单路由
// ---------------------------------------------------------------------------

/// 诊断中心工单（不弹窗只进中心；同日合并）。
#[derive(Clone, Copy, Debug)]
pub struct Ticket {
    /// 快照日。
    pub day: u64,
    /// 合并的异常快照数（同日多条合并一条——通知合批纪律）。
    pub merged: u32,
    /// 异常位掩码合集。
    pub anomaly_mask: u8,
    /// 首个异常快照的归因引用。
    pub attribution_ref: u64,
    /// 工单号（单调）。
    pub ticket_no: u64,
}

// ---------------------------------------------------------------------------
// 采样器与快照引擎
// ---------------------------------------------------------------------------

/// 健康快照引擎。
pub struct SelfCheck {
    /// 指标使能掩码（每项可关）。
    pub metrics_on: u8,
    /// 开机后是否已找到首个空闲窗（采样避开用户高强度使用）。
    idle_found: bool,
    /// 当前累积窗。
    acc: Vec<MinSample>,
    acc_from: u64,
    /// 降频因子（自预算超限 → 采样率减半：丢弃一半样本）。
    downsample_shift: u32,
    sample_seq: u64,
    /// 快照库（每日一份，30 天）。
    snaps: Vec<HealthSnapshot>,
    /// 工单流。
    tickets: RingLog<Ticket, 64>,
    ticket_seq: u64,
    /// 注入慢故障登记（验收面：3 类全部捕获）。
    injected: Vec<(u8, u64)>,
    pub captured_injections: u64,
    pub snaps_total: u64,
    pub self_budget_breaches: u64,
    /// 当日已发工单（合并判据）。
    today_ticket_day: Option<u64>,
    pub merged_today: u32,
}

/// 慢故障类别码（注入验证用）：1=慢帧 2=慢 IO 3=唤醒风暴。
pub const FAULT_SLOW_FRAME: u8 = 1;
pub const FAULT_SLOW_IO: u8 = 2;
pub const FAULT_WAKE_STORM: u8 = 3;

impl SelfCheck {
    pub fn new() -> SelfCheck {
        SelfCheck {
            metrics_on: M_ALL,
            idle_found: false,
            acc: Vec::new(),
            acc_from: 0,
            downsample_shift: 0,
            sample_seq: 0,
            snaps: Vec::new(),
            tickets: RingLog::new(),
            ticket_seq: 0,
            injected: Vec::new(),
            captured_injections: 0,
            snaps_total: 0,
            self_budget_breaches: 0,
            today_ticket_day: None,
            merged_today: 0,
        }
    }

    /// 开关注册面（每项可关）。
    pub fn set_metrics(&mut self, mask: u8) {
        self.metrics_on = mask & M_ALL;
    }

    /// 注入慢故障登记。
    pub fn inject_fault(&mut self, class: u8, day: u64) {
        self.injected.push((class, day));
    }

    /// 喂入一分钟样本。
    ///
    /// 空闲门：开机后未遇空闲分钟前不采样（避开用户高强度使用）；
    /// 自预算：采样开销超 0.1% → 降频（每 2^k 样本取 1）。
    pub fn feed(&mut self, stamp_min: u64, s: MinSample) {
        if !self.idle_found {
            if s.idle {
                self.idle_found = true;
            } else {
                return;
            }
        }
        // 降频采样。
        self.sample_seq += 1;
        if self.downsample_shift > 0 && (self.sample_seq & ((1u64 << self.downsample_shift) - 1)) != 0 {
            return;
        }
        // 自预算巡检：连续超限 → 降频一档（最多 5 档——再降就没意义了，
        // 快照改用账本回查路径，这里如实登记超限次数）。
        if s.self_cost_ppm > SELF_BUDGET_PPM {
            self.self_budget_breaches += 1;
            if self.self_budget_breaches % 10 == 0 && self.downsample_shift < 5 {
                self.downsample_shift += 1;
            }
        }
        if self.acc.is_empty() {
            self.acc_from = stamp_min;
        }
        self.acc.push(s);
        // 窗满出快照。
        if self.acc.len() >= SAMPLE_WINDOW_MIN as usize {
            let snaps_day = self.acc_from / 1440;
            let snap = self.make_snapshot(snaps_day);
            self.acc.clear();
            self.emit(snap);
        }
    }

    fn make_snapshot(&self, day: u64) -> HealthSnapshot {
        let mut snap = HealthSnapshot {
            day,
            from_min: self.acc_from,
            frame_p95_us: 0,
            io_p99_us: 0,
            sched_overruns: 0,
            wakes_peak: 0,
            anomaly_mask: 0,
            attribution_ref: self.acc.first().map(|s| s.frame_p95_us).unwrap_or(0),
        };
        // 归因引用：用窗内首个非零指标签名（真实内核由 F042 归因器供给
        // 真实 ID——接缝显式，不伪造）。
        for s in &self.acc {
            snap.frame_p95_us = snap.frame_p95_us.max(s.frame_p95_us);
            snap.io_p99_us = snap.io_p99_us.max(s.io_p99_us);
            snap.sched_overruns += s.sched_overruns;
            snap.wakes_peak = snap.wakes_peak.max(s.wakes);
        }
        if self.metrics_on & M_FRAME != 0 && snap.frame_p95_us > TH_FRAME_P95_US {
            snap.anomaly_mask |= M_FRAME;
        }
        if self.metrics_on & M_IO != 0 && snap.io_p99_us > TH_IO_P99_US {
            snap.anomaly_mask |= M_IO;
        }
        if self.metrics_on & M_SCHED != 0 && snap.sched_overruns > TH_SCHED_OVERRUNS {
            snap.anomaly_mask |= M_SCHED;
        }
        if self.metrics_on & M_WAKE != 0 && snap.wakes_peak > TH_WAKES_PER_MIN {
            snap.anomaly_mask |= M_WAKE;
        }
        snap
    }

    fn emit(&mut self, snap: HealthSnapshot) {
        self.snaps_total += 1;
        // 快照入库（每日一份——同日新快照覆写旧份，保留 30 天）。
        if let Some(slot) = self.snaps.iter_mut().find(|s| s.day == snap.day) {
            *slot = snap;
        } else {
            self.snaps.push(snap);
            if self.snaps.len() > SNAP_RETENTION_DAYS {
                self.snaps.remove(0);
            }
        }
        // 注入捕获对账。
        for (class, day) in self.injected.iter().copied() {
            let hit = match class {
                FAULT_SLOW_FRAME => snap.anomaly_mask & M_FRAME != 0 && day == snap.day,
                FAULT_SLOW_IO => snap.anomaly_mask & M_IO != 0 && day == snap.day,
                FAULT_WAKE_STORM => snap.anomaly_mask & M_WAKE != 0 && day == snap.day,
                _ => false,
            };
            if hit {
                self.captured_injections += 1;
            }
        }
        // 异常 → 工单（同日合并；不弹窗）。
        if snap.is_anomaly() {
            if self.today_ticket_day == Some(snap.day) {
                // 合并：更新当日工单。
                self.merged_today += 1;
                if let Some(t) = self.tickets.newest_first().first().copied() {
                    let merged = Ticket {
                        day: t.day,
                        merged: t.merged + 1,
                        anomaly_mask: t.anomaly_mask | snap.anomaly_mask,
                        attribution_ref: t.attribution_ref,
                        ticket_no: t.ticket_no,
                    };
                    self.replace_last_ticket(merged);
                }
            } else {
                self.ticket_seq += 1;
                self.tickets.push(Ticket {
                    day: snap.day,
                    merged: 1,
                    anomaly_mask: snap.anomaly_mask,
                    attribution_ref: snap.attribution_ref,
                    ticket_no: self.ticket_seq,
                });
                self.today_ticket_day = Some(snap.day);
                self.merged_today = 1;
            }
        }
    }

    fn replace_last_ticket(&mut self, t: Ticket) {
        // RingLog 最新槽位替换（合并语义：同日一条工单持续生长）。
        let n = self.tickets.len();
        let items = self.tickets.newest_first();
        if n == 0 || items.is_empty() {
            return;
        }
        // 重建：除最新条外全部重放，再压入合并版。
        let mut older = Vec::new();
        for (i, item) in items.iter().enumerate() {
            if i > 0 {
                older.push(*item);
            }
        }
        self.tickets.clear();
        for item in older.into_iter().rev() {
            self.tickets.push(item);
        }
        self.tickets.push(t);
    }

    /// 快照读数（最近 N 天，新→旧）。
    pub fn recent_snaps(&self, n: usize) -> Vec<HealthSnapshot> {
        self.snaps.iter().rev().take(n).copied().collect()
    }

    /// 工单流（新→旧）。
    pub fn ticket_feed(&self) -> Vec<Ticket> {
        self.tickets.newest_first()
    }

    /// 注入捕获对账。
    pub fn capture_rate(&self) -> (u64, u64) {
        (self.captured_injections, self.injected.len() as u64)
    }

    /// 降频档位（诊断面）。
    pub fn downsample(&self) -> u32 {
        self.downsample_shift
    }
}

impl Default for SelfCheck {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F062 自检（聚合进 star 域）。
pub fn run_selfcheck_checks() -> CheckSet {
    let mut set = CheckSet::new("F062-selfcheck");

    // 空闲门：开机高强度期不采样，首个空闲分钟后开始。
    let mut sc = SelfCheck::new();
    for m in 0..20u64 {
        sc.feed(m, MinSample { frame_p95_us: 99_999, idle: false, ..Default::default() });
    }
    set.add("idle gate holds", sc.snaps_total == 0, "");
    sc.feed(20, MinSample { frame_p95_us: 5_000, idle: true, ..Default::default() });
    set.add("idle opens window", sc.acc_from == 20, "");

    // 四阈值判定：超线进掩码。
    let mut sc = SelfCheck::new();
    for m in 0..10u64 {
        sc.feed(m, MinSample {
            frame_p95_us: TH_FRAME_P95_US + 1,
            io_p99_us: TH_IO_P99_US + 1,
            sched_overruns: TH_SCHED_OVERRUNS + 1,
            wakes: TH_WAKES_PER_MIN + 1,
            idle: true,
            self_cost_ppm: 0,
        });
    }
    set.add("all four anomalies", sc.recent_snaps(1)[0].anomaly_mask == M_ALL, "");
    set.add("ticket emitted", sc.ticket_feed().len() == 1, "");

    // 指标可关：关帧项后同数据不报帧异常。
    let mut sc = SelfCheck::new();
    sc.set_metrics(M_ALL & !M_FRAME);
    for m in 0..10u64 {
        sc.feed(m, MinSample { frame_p95_us: TH_FRAME_P95_US + 5, idle: true, ..Default::default() });
    }
    set.add("frame metric off", sc.recent_snaps(1)[0].anomaly_mask & M_FRAME == 0, "");

    // 注入 3 类慢故障全部捕获（主册验收判据直演）。
    let mut sc = SelfCheck::new();
    sc.inject_fault(FAULT_SLOW_FRAME, 0);
    sc.inject_fault(FAULT_SLOW_IO, 1);
    sc.inject_fault(FAULT_WAKE_STORM, 2);
    // day0：慢帧。
    for m in 0..10u64 {
        sc.feed(m, MinSample { frame_p95_us: 20_000, idle: true, ..Default::default() });
    }
    // day1：慢 IO（跨日推进）。
    for m in 1440..1450u64 {
        sc.feed(m, MinSample { io_p99_us: 60_000, idle: true, ..Default::default() });
    }
    // day2：唤醒风暴。
    for m in 2880..2890u64 {
        sc.feed(m, MinSample { wakes: 300, idle: true, ..Default::default() });
    }
    let (cap, total) = sc.capture_rate();
    set.add("3 faults captured", cap == 3 && total == 3, "");

    // 自预算降频。
    let mut sc = SelfCheck::new();
    for m in 0..200u64 {
        sc.feed(m, MinSample { idle: true, self_cost_ppm: SELF_BUDGET_PPM * 5, ..Default::default() });
    }
    set.add("downsample engaged", sc.downsample() >= 1, "");

    // 同日工单合并。
    let mut sc = SelfCheck::new();
    for w in 0..3u64 {
        for m in (w * 100)..(w * 100 + 10) {
            sc.feed(m, MinSample { io_p99_us: 60_000 + w, idle: true, ..Default::default() });
        }
    }
    let feed = sc.ticket_feed();
    set.add("same-day merged", feed.len() == 1 && feed[0].merged == 3, "");

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn healthy(m: u64) -> MinSample {
        MinSample {
            frame_p95_us: 10_000,
            io_p99_us: 20_000,
            sched_overruns: 0,
            wakes: 100,
            idle: true,
            self_cost_ppm: 10 + m % 3,
        }
    }

    #[test]
    fn f062_thresholds_documented() {
        assert_eq!(TH_FRAME_P95_US, 17_000);
        assert_eq!(TH_IO_P99_US, 50_000);
        assert_eq!(TH_SCHED_OVERRUNS, 5);
        assert_eq!(TH_WAKES_PER_MIN, 200);
    }

    #[test]
    fn f062_healthy_day_no_ticket() {
        let mut sc = SelfCheck::new();
        for m in 0..100u64 {
            sc.feed(m, healthy(m));
        }
        assert!(sc.recent_snaps(30).iter().all(|s| !s.is_anomaly()));
        assert!(sc.ticket_feed().is_empty(), "健康日零误报");
    }

    #[test]
    fn f062_snapshot_daily_overwrite() {
        let mut sc = SelfCheck::new();
        // 同日两个窗口：第二份覆写第一份（每日一份）。
        for m in 0..10u64 {
            sc.feed(m, healthy(m));
        }
        for m in 20..30u64 {
            sc.feed(m, healthy(m));
        }
        assert_eq!(sc.snaps_total, 2);
        assert_eq!(sc.recent_snaps(30).len(), 1);
        assert_eq!(sc.recent_snaps(30)[0].from_min, 20);
    }

    #[test]
    fn f062_retention_30_days() {
        let mut sc = SelfCheck::new();
        // 40 天，每天一个窗。
        for d in 0..40u64 {
            let base = d * 1440;
            for m in base..base + 10 {
                sc.feed(m, healthy(m));
            }
        }
        assert_eq!(sc.recent_snaps(100).len(), SNAP_RETENTION_DAYS);
        assert_eq!(sc.recent_snaps(100)[0].day, 39);
    }

    #[test]
    fn f062_sched_mask_separate() {
        let mut sc = SelfCheck::new();
        for m in 0..10u64 {
            sc.feed(m, MinSample { sched_overruns: 9, idle: true, ..Default::default() });
        }
        let mask = sc.recent_snaps(1)[0].anomaly_mask;
        assert_eq!(mask, M_SCHED);
    }

    #[test]
    fn f062_run_checks_pass() {
        assert!(run_selfcheck_checks().all_passed());
    }
}
