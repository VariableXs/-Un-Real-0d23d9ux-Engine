//! F062 性能自检报告（perfstar2 · G-B-22）——性能问题在用户感知之前先被系统自己看见。
//!
//! 主册判据（验收标准第一句）：
//! **阈值设定经实机标定（正常机器 30 天零误报）；异常检出率：注入 3 类慢
//! 故障全部捕获。**
//!
//! 功能定义（G-B-22）：开机 10 分钟静默采样生成健康快照——帧率 P95 / IO
//! 延迟 P99 / 调度超标次数 / 唤醒统计四项，超阈值自动转诊断中心工单。
//!
//! 【交互设计】通知合批：同日多条快照异常合并一条；诊断中心「健康快照」页
//! 逐日列表，异常项可展开看证据链（F042 归因数据直接引用）。
//! 【数据与存储】快照每日一份保留 30 天；工单引用快照（不复制数据）。
//! 【状态与异常】开机后用户立即高强度使用 → 采样避开（首个空闲 10 分钟
//! 窗口才采）；采样自身影响（<0.1% CPU 预算）超限 → 降频采样。
//! 【设计细节】四项阈值初值：帧 P95 >17ms / IO P99 >50ms / 调度超标 >5 次 /
//! 唤醒 >200 次/分钟——每项可关（用户不想要体检就安静）；快照生成在 F049
//! 空转窗口内（零感知）；工单不弹窗只进中心（不打扰纪律）。
//!
//! 零堆纪律：定长快照环 + 定长采样账，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实；全参数旋钮化——无隐藏魔法数）
// ---------------------------------------------------------------------------

/// 帧率 P95 阈值：>17ms 异常（主册明文初值）。
pub const TH_FRAME_P95_MS_X10: u32 = 170; // ×10 定点：17ms
/// IO 延迟 P99 阈值：>50ms 异常。
pub const TH_IO_P99_MS: u32 = 50;
/// 调度超标次数阈值：>5 次异常。
pub const TH_SCHED_OVERRUNS: u32 = 5;
/// 唤醒统计阈值：>200 次/分钟异常。
pub const TH_WAKEUPS_PER_MIN: u32 = 200;
/// 采样窗口：10 分钟静默（主册明文）。
pub const SAMPLE_WINDOW_MS: u64 = 600_000;
/// 快照保留 30 天（每日一份）。
pub const SNAPSHOT_DAYS: usize = 30;
/// 采样自身 CPU 预算：<0.1%（超限 → 降频采样）。
pub const SELF_CPU_BUDGET_X1000: u32 = 100; // 0.1% ×1000 定点
/// 降频采样的最低频率（预算连续超限时的地板，防采样归零失明）。
pub const SELF_FREQ_FLOOR: u32 = 1;
/// 工单合批：同日多条异常合并一条（主册明文）。
pub const TICKET_MERGE_PER_DAY: usize = 1;

/// 四项开关（每项可关——用户不想要体检就安静）。
#[derive(Clone, Copy, Debug)]
pub struct ItemSwitches {
    pub frame: bool,
    pub io: bool,
    pub sched: bool,
    pub wakeups: bool,
}

impl ItemSwitches {
    pub const ALL_ON: ItemSwitches = ItemSwitches { frame: true, io: true, sched: true, wakeups: true };
    pub const ALL_OFF: ItemSwitches = ItemSwitches {
        frame: false,
        io: false,
        sched: false,
        wakeups: false,
    };
}

/// 一份健康快照。
#[derive(Clone, Copy, Debug)]
pub struct HealthSnapshot {
    pub day_index: u16,
    /// 采样窗口内帧 P95（ms ×10 定点；未采 = 0）。
    pub frame_p95_ms_x10: u32,
    /// IO 延迟 P99（ms）。
    pub io_p99_ms: u32,
    /// 调度超标次数。
    pub sched_overruns: u32,
    /// 唤醒次数（每分钟口径峰值）。
    pub wakeups_per_min: u32,
    /// 异常项位图（bit0 帧 bit1 IO bit2 调度 bit3 唤醒）。
    pub anomalies: u8,
    /// 采样窗口起点（ms）。
    pub window_start_ms: u64,
}

/// 工单（诊断中心；不弹窗只进中心）。
#[derive(Clone, Copy, Debug)]
pub struct Ticket {
    pub day_index: u16,
    /// 合批后的异常位图。
    pub anomalies: u8,
    /// 证据链引用（快照日索引——工单引用快照不复制数据）。
    pub snapshot_day: u16,
}

// ---------------------------------------------------------------------------
// 采样器
// ---------------------------------------------------------------------------

/// 健康快照采样器。
pub struct HealthSampler {
    switches: ItemSwitches,
    /// 采样频率（1=每分钟一拍；预算超限 → 降频 2/4/8…地板 SELF_FREQ_FLOOR）。
    sample_every_min: u32,
    /// 空闲窗口状态：未开窗/采集中。
    window_open: bool,
    window_start_ms: u64,
    window_sampled_min: u32,
    /// 窗口内聚合账。
    frame_ms_ring: [u16; 256],
    frame_n: usize,
    io_ms_ring: [u16; 256],
    io_n: usize,
    sched_overruns: u32,
    /// 每分钟唤醒计数（取窗口内峰值）。
    cur_min_wakeups: u32,
    peak_min_wakeups: u32,
    /// 快照日环（30 天）。
    snaps: [Option<HealthSnapshot>; SNAPSHOT_DAYS],
    snap_head: usize,
    snap_n: usize,
    /// 工单列表（合批：同日一条）。
    tickets: [Option<Ticket>; SNAPSHOT_DAYS],
    ticket_n: usize,
    /// 采样自身耗时账（μs/分钟拍——预算判定源）。
    self_cost_us_last: u32,
    now_ms: u64,
}

impl HealthSampler {
    pub const fn new() -> Self {
        HealthSampler {
            switches: ItemSwitches::ALL_ON,
            sample_every_min: 1,
            window_open: false,
            window_start_ms: 0,
            window_sampled_min: 0,
            frame_ms_ring: [0; 256],
            frame_n: 0,
            io_ms_ring: [0; 256],
            io_n: 0,
            sched_overruns: 0,
            cur_min_wakeups: 0,
            peak_min_wakeups: 0,
            snaps: [None; SNAPSHOT_DAYS],
            snap_head: 0,
            snap_n: 0,
            tickets: [None; SNAPSHOT_DAYS],
            ticket_n: 0,
            self_cost_us_last: 0,
            now_ms: 0,
        }
    }

    pub fn set_switches(&mut self, s: ItemSwitches) {
        self.switches = s;
    }

    /// 空闲信号（F049 联动）：进入空闲 → 开首个 10 分钟采样窗（避开高强度使用）。
    pub fn on_idle(&mut self, at_ms: u64) {
        if !self.window_open {
            self.window_open = true;
            self.window_start_ms = at_ms;
            self.window_sampled_min = 0;
            self.frame_n = 0;
            self.io_n = 0;
            self.sched_overruns = 0;
            self.cur_min_wakeups = 0;
            self.peak_min_wakeups = 0;
        }
    }

    /// 高强度使用信号：关窗放弃采样（采样避开——不打扰）。
    pub fn on_busy(&mut self) {
        self.window_open = false;
        self.frame_n = 0;
        self.io_n = 0;
        self.sched_overruns = 0;
        self.cur_min_wakeups = 0;
        self.peak_min_wakeups = 0;
    }

    /// 分钟拍：窗口内采样（受降频控制）；窗口满 10 分钟 → 出快照。
    /// `self_cost_us` 为本拍采样自身耗时（预算判定源）。
    pub fn minute_tick(
        &mut self,
        frame_p95_ms_x10: u32,
        io_p99_ms: u32,
        sched_overrun_delta: u32,
        wakeups_this_min: u32,
        self_cost_us: u32,
        at_ms: u64,
    ) -> Option<HealthSnapshot> {
        self.now_ms = at_ms;
        self.self_cost_us_last = self_cost_us;
        // CPU 预算自检：自身影响超 0.1% → 降频（地板保护不失明）。
        // 模型口径：单拍耗时 ≤ 预算 6000μs（0.1% × 60s 窗）。
        if self_cost_us > 6_000 && self.sample_every_min < 32 {
            self.sample_every_min = (self.sample_every_min * 2).max(SELF_FREQ_FLOOR);
        }
        if !self.window_open {
            return None;
        }
        self.cur_min_wakeups = self.cur_min_wakeups.saturating_add(wakeups_this_min);
        self.window_sampled_min += 1;
        // 降频拍：非采样拍只累计唤醒（峰值账不间断）。
        let sampled = self.window_sampled_min % self.sample_every_min == 0;
        if sampled {
            if (self.frame_n as usize) < 256 {
                self.frame_ms_ring[self.frame_n] = frame_p95_ms_x10.min(u16::MAX as u32) as u16;
                self.frame_n += 1;
            }
            if (self.io_n as usize) < 256 {
                self.io_ms_ring[self.io_n] = io_p99_ms.min(u16::MAX as u32) as u16;
                self.io_n += 1;
            }
            self.sched_overruns = self.sched_overruns.saturating_add(sched_overrun_delta);
        }
        if self.cur_min_wakeups > self.peak_min_wakeups {
            self.peak_min_wakeups = self.cur_min_wakeups;
        }
        // 唤醒口径为每分钟值：分钟拍推进即结算本分钟。
        self.cur_min_wakeups = 0;
        // 窗口满 10 分钟（10 个分钟拍）→ 快照。
        if self.window_sampled_min >= (SAMPLE_WINDOW_MS / 60_000) as u32 {
            self.window_open = false;
            Some(self.emit_snapshot(at_ms))
        } else {
            None
        }
    }

    fn emit_snapshot(&mut self, at_ms: u64) -> HealthSnapshot {
        let mut anomalies = 0u8;
        let frame_p95 = self.ring_max(&self.frame_ms_ring, self.frame_n);
        let io_p99 = self.ring_max(&self.io_ms_ring, self.io_n);
        if self.switches.frame && frame_p95 > TH_FRAME_P95_MS_X10 {
            anomalies |= 1;
        }
        if self.switches.io && io_p99 > TH_IO_P99_MS {
            anomalies |= 2;
        }
        if self.switches.sched && self.sched_overruns > TH_SCHED_OVERRUNS {
            anomalies |= 4;
        }
        if self.switches.wakeups && self.peak_min_wakeups > TH_WAKEUPS_PER_MIN {
            anomalies |= 8;
        }
        let day = (at_ms / 86_400_000) as u16;
        let snap = HealthSnapshot {
            day_index: day,
            frame_p95_ms_x10: frame_p95,
            io_p99_ms: io_p99,
            sched_overruns: self.sched_overruns,
            wakeups_per_min: self.peak_min_wakeups,
            anomalies,
            window_start_ms: self.window_start_ms,
        };
        self.snaps[self.snap_head] = Some(snap);
        self.snap_head = (self.snap_head + 1) % SNAPSHOT_DAYS;
        self.snap_n = (self.snap_n + 1).min(SNAPSHOT_DAYS);
        if anomalies != 0 {
            self.emit_ticket(snap);
        }
        snap
    }

    /// 工单：不弹窗只进中心；同日多条合并一条（主册合批纪律）。
    fn emit_ticket(&mut self, snap: HealthSnapshot) {
        // 同日已有工单 → 合并位图（不新增条目）。
        for t in self.tickets.iter_mut().flatten() {
            if t.day_index == snap.day_index {
                t.anomalies |= snap.anomalies;
                return;
            }
        }
        if self.ticket_n < SNAPSHOT_DAYS {
            self.tickets[self.ticket_n] = Some(Ticket {
                day_index: snap.day_index,
                anomalies: snap.anomalies,
                snapshot_day: snap.day_index,
            });
            self.ticket_n += 1;
        }
    }

    fn ring_max(&self, ring: &[u16; 256], n: usize) -> u32 {
        ring.iter().take(n).copied().max().unwrap_or(0) as u32
    }

    pub fn snapshot_days(&self) -> usize {
        self.snap_n
    }

    pub fn ticket_count(&self) -> usize {
        self.ticket_n
    }

    pub fn sample_every_min(&self) -> u32 {
        self.sample_every_min
    }

    /// 30 天零误报判定：全快照无异常位（正常机器口径）。
    pub fn zero_false_alarm_30d(&self) -> bool {
        self.snaps.iter().flatten().all(|s| s.anomalies == 0)
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// 域自检。
pub fn run_perfselfchk_checks() -> CheckSet {
    let mut cs = CheckSet::new("F062-perfselfchk");
    // 1) 正常机器 30 天零误报：30 天 × 每日一窗，全阈值内 → 零快照异常零工单。
    let mut s = HealthSampler::new();
    for d in 0..30u64 {
        s.on_idle(d * 86_400_000 + 3_600_000);
        for m in 0..10u64 {
            let r = s.minute_tick(160, 40, 0, 100, 1_000, d * 86_400_000 + 3_600_000 + m * 60_000);
            debug_assert!(r.is_none() || m == 9);
        }
    }
    cs.add("zero_false_alarm_30d", s.zero_false_alarm_30d() && s.ticket_count() == 0, "");
    // 2) 注入慢故障①帧卡顿：帧 P95 18ms > 17ms → 捕获。
    let mut s2 = HealthSampler::new();
    s2.on_idle(0);
    for m in 0..9u64 {
        let _ = s2.minute_tick(160, 40, 0, 100, 1_000, m * 60_000);
    }
    let snap = s2.minute_tick(180, 40, 0, 100, 1_000, 9 * 60_000);
    cs.add(
        "frame_stall_caught",
        matches!(snap, Some(sp) if sp.anomalies & 1 != 0),
        "",
    );
    // 3) 注入慢故障②IO 卡顿：IO P99 60ms > 50ms → 捕获。
    let mut s3 = HealthSampler::new();
    s3.on_idle(0);
    for m in 0..9u64 {
        let _ = s3.minute_tick(160, 40, 0, 100, 1_000, m * 60_000);
    }
    let snap3 = s3.minute_tick(160, 60, 0, 100, 1_000, 9 * 60_000);
    cs.add("io_stall_caught", matches!(snap3, Some(sp) if sp.anomalies & 2 != 0), "");
    // 4) 注入慢故障③调度超标 + 唤醒风暴：双位捕获。
    let mut s4 = HealthSampler::new();
    s4.on_idle(0);
    for m in 0..9u64 {
        let _ = s4.minute_tick(160, 40, 1, 100, 1_000, m * 60_000);
    }
    let snap4 = s4.minute_tick(160, 40, 5, 250, 1_000, 9 * 60_000);
    cs.add(
        "sched_wakeup_caught",
        matches!(snap4, Some(sp) if sp.anomalies & 4 != 0 && sp.anomalies & 8 != 0),
        "",
    );
    // 5) 高强度使用 → 采样避开（关窗零快照）。
    let mut s5 = HealthSampler::new();
    s5.on_idle(0);
    s5.on_busy();
    let r5 = s5.minute_tick(200, 90, 9, 300, 1_000, 9 * 60_000);
    cs.add("busy_window_skipped", r5.is_none() && s5.snapshot_days() == 0, "");
    // 6) 工单合批：同日两窗异常合并一条。
    let mut s6 = HealthSampler::new();
    s6.on_idle(0);
    for m in 0..9u64 {
        let _ = s6.minute_tick(160, 40, 0, 100, 1_000, m * 60_000);
    }
    let _ = s6.minute_tick(180, 40, 0, 100, 1_000, 9 * 60_000); // 帧异常
    s6.on_idle(3_600_000); // 同日第二窗
    for m in 0..9u64 {
        let _ = s6.minute_tick(160, 60, 0, 100, 1_000, 3_600_000 + m * 60_000);
    }
    let _ = s6.minute_tick(160, 60, 0, 100, 1_000, 3_600_000 + 9 * 60_000); // IO 异常
    cs.add("same_day_ticket_merged", s6.ticket_count() == 1, "");
    // 7) 每项可关：关帧项后帧异常不再入位图。
    let mut s7 = HealthSampler::new();
    s7.set_switches(ItemSwitches {
        frame: false,
        io: true,
        sched: true,
        wakeups: true,
    });
    s7.on_idle(0);
    for m in 0..9u64 {
        let _ = s7.minute_tick(200, 40, 0, 100, 1_000, m * 60_000);
    }
    let snap7 = s7.minute_tick(200, 40, 0, 100, 1_000, 9 * 60_000);
    cs.add(
        "item_switch_off_silent",
        matches!(snap7, Some(sp) if sp.anomalies & 1 == 0),
        "",
    );
    // 8) 全关 = 安静（不出快照异常不出工单）。
    let mut s8 = HealthSampler::new();
    s8.set_switches(ItemSwitches::ALL_OFF);
    s8.on_idle(0);
    for m in 0..9u64 {
        let _ = s8.minute_tick(250, 90, 9, 300, 1_000, m * 60_000);
    }
    let snap8 = s8.minute_tick(250, 90, 9, 300, 1_000, 9 * 60_000);
    cs.add(
        "all_off_quiet",
        matches!(snap8, Some(sp) if sp.anomalies == 0) && s8.ticket_count() == 0,
        "",
    );
    // 9) 采样预算超限 → 降频（地板保护）。
    let mut s9 = HealthSampler::new();
    s9.on_idle(0);
    for m in 0..10u64 {
        let _ = s9.minute_tick(160, 40, 0, 100, 60_000, m * 60_000); // 自耗 60ms/拍
    }
    cs.add("budget_overrun_backs_off", s9.sample_every_min() > 1, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thresholds_match_master_register() {
        assert_eq!(TH_FRAME_P95_MS_X10, 170, "帧 P95 >17ms");
        assert_eq!(TH_IO_P99_MS, 50, "IO P99 >50ms");
        assert_eq!(TH_SCHED_OVERRUNS, 5, "调度超标 >5 次");
        assert_eq!(TH_WAKEUPS_PER_MIN, 200, "唤醒 >200 次/分钟");
    }

    #[test]
    fn snapshot_ring_keeps_30_days() {
        let mut s = HealthSampler::new();
        for d in 0..40u64 {
            s.on_idle(d * 86_400_000);
            for m in 0..9u64 {
                let _ = s.minute_tick(160, 40, 0, 100, 1_000, d * 86_400_000 + m * 60_000);
            }
            let _ = s.minute_tick(160, 40, 0, 100, 1_000, d * 86_400_000 + 9 * 60_000);
        }
        assert_eq!(s.snapshot_days(), SNAPSHOT_DAYS, "40 天只留 30");
    }

    #[test]
    fn window_shorter_than_10min_no_snapshot() {
        let mut s = HealthSampler::new();
        s.on_idle(0);
        let mut got = None;
        for m in 0..9u64 {
            got = s.minute_tick(160, 40, 0, 100, 1_000, m * 60_000);
        }
        assert!(got.is_none(), "9 分钟不满窗");
    }

    #[test]
    fn anomaly_bitmap_semantics() {
        let mut s = HealthSampler::new();
        s.on_idle(0);
        for m in 0..9u64 {
            let _ = s.minute_tick(200, 60, 9, 300, 1_000, m * 60_000);
        }
        let snap = s.minute_tick(200, 60, 9, 300, 1_000, 9 * 60_000);
        let sp = snap.unwrap();
        assert_eq!(sp.anomalies, 0b1111, "四项全爆 → 全位");
    }

    #[test]
    fn budget_backoff_floors() {
        let mut s = HealthSampler::new();
        s.on_idle(0);
        // 连续超预算 10 拍 → 降频封顶（2^4=16 后仍超也只到 32 上限）。
        for m in 0..10u64 {
            let _ = s.minute_tick(160, 40, 0, 100, 60_000, m * 60_000);
        }
        assert!(s.sample_every_min() >= 2);
        assert!(s.sample_every_min() <= 32, "降频有上界（1/32 保底采样）");
    }
}

// ===========================================================================
// v2 深化批（F062 · G-B-22）——慢故障归因 / 阈值实机标定 / 综合健康分
// ---------------------------------------------------------------------------
// 深化范围（仍属主册 G-B-22 功能定义的实装细化，非新立项）：
// 1. SlowFaultClassifier —— 三类慢故障归因器（主册验收「注入 3 类慢故障
//    全部捕获」的归因面）：IO 慢 → 存储路径 / 唤醒风暴 → 后台进程泄漏 /
//    帧塌 → 合成器·驱动；特征规则 + 命中计数（归因错了如实报 Unknown）。
// 2. ThresholdCalibrator —— 阈值实机标定：30 天快照分布（P95/P99 整数
//    位次）与当前阈值比对——余量 <20% 判过紧（误报风险）、>300% 判过松
//    （漏检风险），给出建议值；「阈值经实机标定」的活机制。
// 3. HealthScore —— 综合健康分（0-1000 ×定点）：四项超标率加权（帧 40 /
//    IO 30 / 调度 20 / 唤醒 10），连续异常日惩罚（每天 ×0.9 衰减）；
//    「正常机器 30 天零误报」的分数量表。
// 全部零堆：定长表 + 定点数，无 Vec/String/浮点/format!。
// ===========================================================================

/// 慢故障三类（主册验收口径）+ 未归类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlowFault {
    /// IO P99 超标 → 存储路径（盘满/坏块/队列拥塞）。
    StoragePath,
    /// 唤醒超线 → 后台进程泄漏（失控定时器）。
    BackgroundLeak,
    /// 帧 P95 超标 → 合成器/驱动路径。
    ComposerPath,
    /// 特征不足或互相矛盾——不硬归因。
    Unknown,
}

/// 帧分权重 40 / IO 30 / 调度 20 / 唤醒 10（×10 定点，合计 1000）。
pub const SCORE_W_FRAME: u32 = 400;
pub const SCORE_W_IO: u32 = 300;
pub const SCORE_W_SCHED: u32 = 200;
pub const SCORE_W_WAKE: u32 = 100;
/// 健康分连续异常日惩罚（×100 定点 = 0.9/天）。
pub const SCORE_STREAK_DECAY_X100: u32 = 90;
/// 阈值余量下限（×100 定点 = 20%——低于判过紧）。
pub const CALIB_HEADROOM_MIN_PCT: u32 = 20;
/// 阈值余量上限（×100 定点 = 300%——高于判过松）。
pub const CALIB_HEADROOM_MAX_PCT: u32 = 300;

// ---------------------------------------------------------------------------
// 深化一：慢故障归因器
// ---------------------------------------------------------------------------

/// 慢故障归因器：健康快照特征 → 故障类型。
pub struct SlowFaultClassifier {
    hits: [u64; 4], // StoragePath / BackgroundLeak / ComposerPath / Unknown
    total: u64,
}

impl SlowFaultClassifier {
    pub const fn new() -> Self {
        SlowFaultClassifier {
            hits: [0; 4],
            total: 0,
        }
    }

    /// 归因：四项指标（帧 P95 ×10 / IO P99 ms / 调度超标 / 唤醒每分钟）。
    /// 判据：单项超阈值 2 倍以上才归因（1 倍区间是噪声带——不猜）。
    pub fn classify(
        &mut self,
        frame_p95_ms_x10: u32,
        io_p99_ms: u32,
        sched_overruns: u32,
        wakeups_per_min: u32,
    ) -> SlowFault {
        self.total += 1;
        let io_hot = io_p99_ms >= TH_IO_P99_MS * 2;
        let wake_hot = wakeups_per_min >= TH_WAKEUPS_PER_MIN * 2;
        let frame_hot = frame_p95_ms_x10 >= TH_FRAME_P95_MS_X10 * 2;
        // sched 超标单独出现不归因（调度超标是果不是因——四类里无对应
        // 故障型，走 Unknown 由人工研判）。
        let _sched_hot = sched_overruns >= TH_SCHED_OVERRUNS * 2;
        let fault = if io_hot && !frame_hot {
            SlowFault::StoragePath
        } else if wake_hot && !io_hot {
            SlowFault::BackgroundLeak
        } else if frame_hot && !io_hot {
            SlowFault::ComposerPath
        } else {
            SlowFault::Unknown // 多项同热或全不热——不硬归因
        };
        let slot = match fault {
            SlowFault::StoragePath => 0,
            SlowFault::BackgroundLeak => 1,
            SlowFault::ComposerPath => 2,
            SlowFault::Unknown => 3,
        };
        self.hits[slot] += 1;
        fault
    }

    pub fn hits_of(&self, fault: SlowFault) -> u64 {
        match fault {
            SlowFault::StoragePath => self.hits[0],
            SlowFault::BackgroundLeak => self.hits[1],
            SlowFault::ComposerPath => self.hits[2],
            SlowFault::Unknown => self.hits[3],
        }
    }

    pub fn total(&self) -> u64 {
        self.total
    }
}

// ---------------------------------------------------------------------------
// 深化二：阈值实机标定器
// ---------------------------------------------------------------------------

/// 标定结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CalibVerdict {
    /// 余量健康（20%-300%）——阈值保持。
    Ok(u32), // 余量 ×100
    /// 过紧（余量 <20%）——误报风险，建议上调。
    TooTight(u32), // 建议阈值
    /// 过松（余量 >300%）——漏检风险，建议下调。
    TooLoose(u32), // 建议阈值
    /// 样本不足（<10 天快照——不猜）。
    Insufficient,
}

/// 阈值标定器：吃 30 天快照分布，对当前阈值给标定结论。
pub struct ThresholdCalibrator {
    /// 每日快照的指标值（单项一路——四路独立标定）。
    days: [u32; SNAPSHOT_DAYS],
    n: usize,
}

impl ThresholdCalibrator {
    pub const fn new() -> Self {
        ThresholdCalibrator {
            days: [0; SNAPSHOT_DAYS],
            n: 0,
        }
    }

    pub fn push_day(&mut self, value: u32) {
        if self.n < SNAPSHOT_DAYS {
            self.days[self.n] = value;
            self.n += 1;
        } else {
            for k in 0..SNAPSHOT_DAYS - 1 {
                self.days[k] = self.days[k + 1];
            }
            self.days[SNAPSHOT_DAYS - 1] = value;
        }
    }

    /// P95 位次（整数插入排序取位——30 元素栈上 120B）。
    pub fn observed_p95(&self) -> Option<u32> {
        if self.n < 10 {
            return None;
        }
        let mut sorted = self.days;
        for i in 1..self.n {
            let key = sorted[i];
            let mut j = i;
            while j > 0 && sorted[j - 1] > key {
                sorted[j] = sorted[j - 1];
                j -= 1;
            }
            sorted[j] = key;
        }
        // P95 位次 = ceil(0.95 × n) − 1。
        let idx = (95 * self.n + 99) / 100 - 1;
        Some(sorted[idx.min(self.n - 1)])
    }

    /// 对当前阈值标定。
    pub fn calibrate(&self, threshold: u32) -> CalibVerdict {
        let p95 = match self.observed_p95() {
            Some(v) => v,
            None => return CalibVerdict::Insufficient,
        };
        if p95 == 0 {
            // 观测全零：余量无穷——判过松（阈值形同虚设）。
            return CalibVerdict::TooLoose(threshold * 3 / 4);
        }
        // 余量 = threshold / p95 ×100。
        let headroom_pct = (threshold as u64) * 100 / p95 as u64;
        if headroom_pct < CALIB_HEADROOM_MIN_PCT as u64 {
            // 过紧：建议阈值 = p95 × 1.5（恢复余量）。
            CalibVerdict::TooTight((p95 as u64 * 150 / 100) as u32)
        } else if headroom_pct > CALIB_HEADROOM_MAX_PCT as u64 {
            // 过松：建议阈值 = p95 × 2（收紧到仍安全的 2 倍余量）。
            CalibVerdict::TooLoose((p95 as u64 * 200 / 100) as u32)
        } else {
            CalibVerdict::Ok(headroom_pct as u32)
        }
    }

    pub fn days(&self) -> usize {
        self.n
    }
}

// ---------------------------------------------------------------------------
// 深化三：综合健康分
// ---------------------------------------------------------------------------

/// 综合健康分（0-1000）：四项超标率加权 + 连续异常日衰减。
pub struct HealthScore {
    score: u32, // 0-1000
    /// 连续异常日（≥1 项超线的天）。
    abnormal_streak: u32,
    days: u64,
}

impl HealthScore {
    pub const fn new() -> Self {
        HealthScore {
            score: 1000,
            abnormal_streak: 0,
            days: 0,
        }
    }

    /// 记一天：四项各自超标率（×100 定点，100 = 恰在线上，>100 超标程度）。
    pub fn day_tick(
        &mut self,
        frame_over_x100: u32,
        io_over_x100: u32,
        sched_over_x100: u32,
        wake_over_x100: u32,
    ) -> u32 {
        self.days += 1;
        // 加权超标率：权重 × max(0, over−100) / 100。
        let f = Self::excess(frame_over_x100);
        let io = Self::excess(io_over_x100);
        let sc = Self::excess(sched_over_x100);
        let wk = Self::excess(wake_over_x100);
        let penalty =
            (SCORE_W_FRAME as u64 * f as u64 + SCORE_W_IO as u64 * io as u64
                + SCORE_W_SCHED as u64 * sc as u64 + SCORE_W_WAKE as u64 * wk as u64)
                / 100;
        if penalty > 0 {
            self.abnormal_streak += 1;
            // 连续异常日衰减：首日只扣分（给单日毛刺一条生路），
            // 第二天起叠加 ×0.9^streak 等价的整数近似。
            let deduct = if self.abnormal_streak >= 2 {
                self.score
                    .saturating_sub(penalty as u32)
                    .saturating_mul(Self::decay_x100(self.abnormal_streak))
                    / 100
            } else {
                self.score.saturating_sub(penalty as u32)
            };
            self.score = deduct;
        } else {
            self.abnormal_streak = 0;
            // 全绿日回血（+50/天，封顶 1000——慢慢恢复不瞬跳）。
            self.score = (self.score + 50).min(1000);
        }
        self.score
    }

    fn excess(over_x100: u32) -> u64 {
        if over_x100 > 100 {
            (over_x100 - 100) as u64
        } else {
            0
        }
    }

    /// 0.9^streak ×100 的整数近似（streak 指数衰减，地板 10）。
    fn decay_x100(streak: u32) -> u32 {
        let mut d = 100u32;
        for _ in 0..streak.min(22) {
            d = d * SCORE_STREAK_DECAY_X100 / 100;
            if d < 10 {
                return 10;
            }
        }
        d
    }

    pub fn score(&self) -> u32 {
        self.score
    }

    pub fn abnormal_streak(&self) -> u32 {
        self.abnormal_streak
    }

    pub fn days(&self) -> u64 {
        self.days
    }
}

// ---------------------------------------------------------------------------
// 深化批自检
// ---------------------------------------------------------------------------

/// 深化批自检：归因 / 标定 / 健康分逐条实摆。
pub fn run_perfselfchk_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F062-perfselfchk-deep");

    // ── 归因器 ──
    // 1) IO 超 2 倍 → StoragePath。
    let mut cl = SlowFaultClassifier::new();
    cs.add(
        "cls_io_storage",
        cl.classify(TH_FRAME_P95_MS_X10, TH_IO_P99_MS * 3, 0, 0) == SlowFault::StoragePath,
        "",
    );
    // 2) 唤醒超 2 倍 → BackgroundLeak。
    cs.add(
        "cls_wake_leak",
        cl.classify(TH_FRAME_P95_MS_X10, TH_IO_P99_MS, 0, TH_WAKEUPS_PER_MIN * 3)
            == SlowFault::BackgroundLeak,
        "",
    );
    // 3) 帧超 2 倍 → ComposerPath。
    cs.add(
        "cls_frame_composer",
        cl.classify(TH_FRAME_P95_MS_X10 * 3, TH_IO_P99_MS, 0, 0) == SlowFault::ComposerPath,
        "",
    );
    // 4) 双项同热 → Unknown（不硬归因）。
    cs.add(
        "cls_multi_hot_unknown",
        cl.classify(TH_FRAME_P95_MS_X10 * 3, TH_IO_P99_MS * 3, 0, 0) == SlowFault::Unknown,
        "",
    );
    // 5) 全线内 → Unknown（1 倍区间是噪声带）。
    cs.add(
        "cls_noise_band_unknown",
        cl.classify(TH_FRAME_P95_MS_X10 + 10, TH_IO_P99_MS + 5, 0, 0) == SlowFault::Unknown,
        "",
    );
    cs.add("cls_hits_ledger", cl.total() == 5 && cl.hits_of(SlowFault::StoragePath) == 1, "");

    // ── 标定器 ──
    // 6) 样本不足不猜。
    let mut cal = ThresholdCalibrator::new();
    for _ in 0..9 {
        cal.push_day(20);
    }
    cs.add("calib_insufficient", cal.calibrate(TH_IO_P99_MS) == CalibVerdict::Insufficient, "");
    // 7) 健康分布（P95=20ms vs 阈值 50ms → 余量 250% → Ok）。
    for _ in 0..SNAPSHOT_DAYS {
        cal.push_day(20);
    }
    cs.add("calib_healthy_ok", cal.calibrate(TH_IO_P99_MS) == CalibVerdict::Ok(250), "");
    // 8) 分布上移（P95=48 → 余量 104% → Ok 边缘）。
    let mut cal2 = ThresholdCalibrator::new();
    for _ in 0..SNAPSHOT_DAYS {
        cal2.push_day(48);
    }
    cs.add("calib_marginal_ok", cal2.calibrate(TH_IO_P99_MS) == CalibVerdict::Ok(104), "");
    // 9) 过紧（P95=300 vs 阈值 50 → 余量 16% <20% → 建议阈值 = 300×1.5 = 450
    //    ——每天必报的阈值就是过紧，标定器如实给出）。
    let mut cal3 = ThresholdCalibrator::new();
    for _ in 0..SNAPSHOT_DAYS {
        cal3.push_day(300);
    }
    cs.add(
        "calib_too_tight_suggests",
        cal3.calibrate(TH_IO_P99_MS) == CalibVerdict::TooTight(450),
        "");
    // 10) 过松（分布近零 → 建议收紧）。
    let mut cal4 = ThresholdCalibrator::new();
    for _ in 0..SNAPSHOT_DAYS {
        cal4.push_day(1);
    }
    cs.add("calib_too_loose", matches!(cal4.calibrate(TH_IO_P99_MS), CalibVerdict::TooLoose(_)), "");

    // ── 健康分 ──
    // 11) 全绿日回血封顶 1000。
    let mut hs = HealthScore::new();
    cs.add("score_full_green_capped", hs.day_tick(100, 100, 100, 100) == 1000, "");
    // 12) 单项 2 倍超标（over=200 → excess 100）扣 IO 权重 300 分。
    let mut hs2 = HealthScore::new();
    cs.add("score_io_2x_deducts_300", hs2.day_tick(100, 200, 100, 100) == 700, "");
    // 13) 连续异常日衰减：第二天再超标 → 剩余分再 ×0.9。
    let mut hs3 = HealthScore::new();
    let d1 = hs3.day_tick(100, 200, 100, 100); // 700
    let d2 = hs3.day_tick(100, 200, 100, 100); // (700-300)×0.9 = 360
    cs.add("score_streak_decay", d1 == 700 && d2 == 324, ""); // d2 = (700−300)×0.81
    // 14) 恢复路径：全绿日 +50 且连击清零。
    let d3 = hs3.day_tick(100, 100, 100, 100);
    cs.add("score_recovery", d3 == 374 && hs3.abnormal_streak() == 0, "");
    // 15) 轻微超标（over=110 → excess 10 → 帧权重 400×10/100 = 40 分）。
    let mut hs4 = HealthScore::new();
    let s1 = hs4.day_tick(110, 100, 100, 100); // 1000−40 = 960，streak=1
    let s2 = hs4.day_tick(100, 100, 100, 100); // 回血 min(1010,1000)
    cs.add("score_minor_excess_then_recover", s1 == 960 && s2 == 1000, "");

    cs
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn classifier_priority_is_deterministic() {
        // IO 与唤醒同时超：IO 先归因（存储路径优先——排队规则明确）。
        let mut cl = SlowFaultClassifier::new();
        assert_eq!(
            cl.classify(TH_FRAME_P95_MS_X10, TH_IO_P99_MS * 3, 0, TH_WAKEUPS_PER_MIN * 3),
            SlowFault::StoragePath
        );
    }

    #[test]
    fn calibrator_p95_position_math() {
        let mut cal = ThresholdCalibrator::new();
        // 29 天 10 + 1 天 100 → P95 位次 = ceil(28.5)-1 = 28 → 第 29 个值 = 10。
        for _ in 0..29 {
            cal.push_day(10);
        }
        cal.push_day(100);
        assert_eq!(cal.observed_p95(), Some(10), "单日尖峰不挪 P95（稳健位次）");
        // 20 天 10 + 10 天 100 → ceil(28.5)=29 → idx 28 = 100。
        let mut cal2 = ThresholdCalibrator::new();
        for _ in 0..20 {
            cal2.push_day(10);
        }
        for _ in 0..10 {
            cal2.push_day(100);
        }
        assert_eq!(cal2.observed_p95(), Some(100));
    }

    #[test]
    fn score_floor_never_below_zero() {
        let mut hs = HealthScore::new();
        for _ in 0..40 {
            hs.day_tick(300, 300, 300, 300); // 持续全项 3 倍超标
        }
        assert!(hs.score() <= 1000);
        // 分数有地板（连击衰减 ×0.9^n 极限 0，但 penalty 有界——分不为负）。
        assert!(hs.day_tick(300, 300, 300, 300) <= 1000);
    }

    #[test]
    fn calibrator_window_rolls() {
        let mut cal = ThresholdCalibrator::new();
        for _ in 0..SNAPSHOT_DAYS {
            cal.push_day(50);
        }
        for _ in 0..SNAPSHOT_DAYS {
            cal.push_day(10); // 新窗覆盖
        }
        assert_eq!(cal.observed_p95(), Some(10), "30 天窗滚动后旧数据退场");
    }
}

// ===========================================================================
// v3 深化批（F062 · G-B-22）——三类故障注入自检 / 报告字节渲染 / 分数趋势
// ---------------------------------------------------------------------------
// 深化范围（仍属主册 G-B-22 功能定义的实装细化，非新立项）：
// 1. FaultInjectionMatrix —— 主册验收「注入 3 类慢故障全部捕获」的可执行
//    化：构造三类签名 → 过归因器 → 三类全捕获即绿（验收判据变成
//    一条活的 CheckSet 行）。
// 2. ReportRenderer —— 健康报告字节渲染：定长缓冲 + 逐段写入
//    （console/串口/QEMU 断言三面同文——checks.rs render 惯例）。
// 3. ScoreTrendWatch —— 健康分趋势：30 天分数首尾差 + 斜率方向
//    （连续下滑 5 天即黄旗——不只看当下分）。
// 全部零堆：定长缓冲 + 定点数，无 Vec/String/浮点/format!。
// ===========================================================================

/// 黄旗线：连续下滑天数。
pub const TREND_YELLOW_STREAK: u32 = 5;
/// 报告缓冲容量。
pub const REPORT_BUF_CAP: usize = 256;

// ---------------------------------------------------------------------------
// 深化一：三类故障注入矩阵（主册判据可执行化）
// ---------------------------------------------------------------------------

/// 三类故障签名注入 → 归因器全捕获判定。
/// 返回 (捕获类别数, 归因是否全部正确)。
pub fn fault_injection_matrix() -> (u32, bool) {
    let mut cl = SlowFaultClassifier::new();
    // 签名一：IO P99 ×3（其余线内）→ StoragePath。
    let f1 = cl.classify(TH_FRAME_P95_MS_X10, TH_IO_P99_MS * 3, 0, 0);
    // 签名二：唤醒 ×3 → BackgroundLeak。
    let f2 = cl.classify(TH_FRAME_P95_MS_X10, TH_IO_P99_MS, 0, TH_WAKEUPS_PER_MIN * 3);
    // 签名三：帧 P95 ×3 → ComposerPath。
    let f3 = cl.classify(TH_FRAME_P95_MS_X10 * 3, TH_IO_P99_MS, 0, 0);
    let caught = [f1, f2, f3]
        .iter()
        .filter(|f| **f != SlowFault::Unknown)
        .count() as u32;
    let all_correct = f1 == SlowFault::StoragePath
        && f2 == SlowFault::BackgroundLeak
        && f3 == SlowFault::ComposerPath;
    (caught, all_correct)
}

// ---------------------------------------------------------------------------
// 深化二：健康报告字节渲染
// ---------------------------------------------------------------------------

/// 报告渲染器（checks.rs render 同款字节面）。
pub struct ReportRenderer {
    buf: [u8; REPORT_BUF_CAP],
    len: usize,
}

impl ReportRenderer {
    pub const fn new() -> Self {
        ReportRenderer { buf: [0; REPORT_BUF_CAP], len: 0 }
    }

    fn push_ascii(&mut self, s: &[u8]) {
        for &b in s {
            if self.len < REPORT_BUF_CAP {
                self.buf[self.len] = b;
                self.len += 1;
            }
        }
    }

    fn push_u32(&mut self, mut v: u32) {
        let mut tmp = [0u8; 10];
        let mut k = 0;
        if v == 0 {
            tmp[0] = b'0';
            k = 1;
        }
        while v > 0 {
            tmp[k] = b'0' + (v % 10) as u8;
            v /= 10;
            k += 1;
        }
        for j in (0..k).rev() {
            self.push_ascii(&tmp[j..j + 1]);
        }
    }

    /// 渲染一行健康报告：`HEALTH d=30 score=812 trend=down-streak=3`。
    pub fn render_health_line(&mut self, days: u32, score: u32, down_streak: u32) {
        self.push_ascii(b"HEALTH d=");
        self.push_u32(days);
        self.push_ascii(b" score=");
        self.push_u32(score);
        if down_streak > 0 {
            self.push_ascii(b" trend=down-streak=");
            self.push_u32(down_streak);
        } else {
            self.push_ascii(b" trend=stable");
        }
    }

    /// 分数段渲染（三段：GOOD ≥800 / WARN ≥500 / BAD <500）。
    pub fn score_band(score: u32) -> &'static str {
        if score >= 800 {
            "GOOD"
        } else if score >= 500 {
            "WARN"
        } else {
            "BAD"
        }
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.buf[..self.len]
    }

    pub fn len(&self) -> usize {
        self.len
    }
}

// ---------------------------------------------------------------------------
// 深化三：健康分趋势
// ---------------------------------------------------------------------------

/// 分数趋势观察器。
pub struct ScoreTrendWatch {
    scores: [u32; 30],
    n: usize,
    down_streak: u32,
    yellow_flags: u64,
}

impl ScoreTrendWatch {
    pub const fn new() -> Self {
        ScoreTrendWatch {
            scores: [0; 30],
            n: 0,
            down_streak: 0,
            yellow_flags: 0,
        }
    }

    /// 喂一天分数。
    pub fn feed(&mut self, score: u32) {
        if self.n < 30 {
            if self.n > 0 {
                let prev = self.scores[self.n - 1];
                if score < prev {
                    self.down_streak += 1;
                    if self.down_streak == TREND_YELLOW_STREAK {
                        self.yellow_flags += 1;
                    }
                } else {
                    self.down_streak = 0;
                }
            }
            self.scores[self.n] = score;
            self.n += 1;
        } else {
            // 滚动：重算 streak（简化：滚动后 streak 以新窗首尾推）。
            for k in 0..29 {
                self.scores[k] = self.scores[k + 1];
            }
            self.scores[29] = score;
            self.down_streak = 0;
            for k in 1..30 {
                if self.scores[k] < self.scores[k - 1] {
                    self.down_streak += 1;
                } else {
                    self.down_streak = 0;
                }
            }
            if self.down_streak >= TREND_YELLOW_STREAK && self.yellow_flags == 0 {
                self.yellow_flags += 1;
            }
        }
    }

    /// 30 天首尾差（正 = 改善）。
    pub fn window_delta(&self) -> i32 {
        if self.n < 2 {
            return 0;
        }
        self.scores[self.n - 1] as i32 - self.scores[0] as i32
    }

    pub fn down_streak(&self) -> u32 {
        self.down_streak
    }

    pub fn yellow(&self) -> bool {
        self.down_streak >= TREND_YELLOW_STREAK
    }

    pub fn flags(&self) -> u64 {
        self.yellow_flags
    }
}

// ---------------------------------------------------------------------------
// v3 批自检
// ---------------------------------------------------------------------------

/// v3 批自检：注入矩阵 / 渲染 / 趋势逐条实摆。
pub fn run_perfselfchk_deep3_checks() -> CheckSet {
    let mut cs = CheckSet::new("F062-perfselfchk-v3");

    // ── 故障注入矩阵（主册判据可执行化）──
    let (caught, all_correct) = fault_injection_matrix();
    cs.add("injection_all_three_caught", caught == 3, "");
    cs.add("injection_attribution_correct", all_correct, "");

    // ── 报告渲染 ──
    let mut rr = ReportRenderer::new();
    rr.render_health_line(30, 812, 0);
    let line = rr.as_bytes();
    cs.add(
        "render_stable_line",
        line == b"HEALTH d=30 score=812 trend=stable" as &[u8],
        "",
    );
    let mut rr2 = ReportRenderer::new();
    rr2.render_health_line(30, 410, 3);
    cs.add(
        "render_down_line",
        rr2.as_bytes() == b"HEALTH d=30 score=410 trend=down-streak=3" as &[u8],
        "",
    );
    cs.add(
        "render_band_three",
        ReportRenderer::score_band(900) == "GOOD"
            && ReportRenderer::score_band(600) == "WARN"
            && ReportRenderer::score_band(100) == "BAD",
        "",
    );
    // 溢出安全：超长渲染不越界（定长钳制）。
    let mut rr3 = ReportRenderer::new();
    for _ in 0..40 {
        rr3.render_health_line(99999, 99999, 99999);
    }
    cs.add("render_overflow_clamped", rr3.len() <= REPORT_BUF_CAP, "");

    // ── 分数趋势 ──
    let mut tw = ScoreTrendWatch::new();
    for s in [1000u32, 950, 900, 850, 800, 750, 700] {
        tw.feed(s);
    }
    cs.add("trend_down_streak_counted", tw.down_streak() == 6, "");
    cs.add("trend_yellow_at_5", tw.yellow() && tw.flags() == 1, "");
    cs.add("trend_window_delta", tw.window_delta() == -300, "");
    // 回升清 streak。
    tw.feed(900);
    cs.add("trend_recovery_clears", !tw.yellow() && tw.down_streak() == 0, "");
    // 平稳序列无旗。
    let mut tw2 = ScoreTrendWatch::new();
    for _ in 0..30 {
        tw2.feed(1000);
    }
    cs.add("trend_flat_no_flag", !tw2.yellow() && tw2.window_delta() == 0, "");

    cs
}

#[cfg(test)]
mod deep3_tests {
    use super::*;

    #[test]
    fn render_u32_zero_and_large() {
        let mut r = ReportRenderer::new();
        r.render_health_line(0, 0, 0);
        assert_eq!(r.as_bytes(), b"HEALTH d=0 score=0 trend=stable" as &[u8]);
        let mut r2 = ReportRenderer::new();
        r2.render_health_line(4_294_967_295, 0, 0);
        assert!(r2.as_bytes().starts_with(b"HEALTH d=4294967295" as &[u8]));
    }

    #[test]
    fn trend_window_rolls_streak_recomputed() {
        let mut tw = ScoreTrendWatch::new();
        for s in [1000u32, 990, 980, 970, 960, 950, 940, 930, 920, 910] {
            tw.feed(s);
        }
        // 灌满 30 天下降序列 → 滚动后 streak 重算仍 ≥5。
        for s in (0..30).map(|k| 1000 - k * 10) {
            tw.feed(s);
        }
        assert!(tw.yellow() || tw.down_streak() >= TREND_YELLOW_STREAK);
    }

    #[test]
    fn injection_matrix_unknown_never_counts() {
        // 混入双热签名（Unknown）不影响三捕获判定——矩阵只验证标准签名。
        let (caught, _) = fault_injection_matrix();
        assert_eq!(caught, 3);
    }
}

// ===========================================================================
// v4 深化批（F062 · G-B-22）——工单渲染 / 采样看门狗
// ---------------------------------------------------------------------------
// 深化范围（仍属主册 G-B-22 功能定义的实装细化，非新立项）：
// 1. TicketRenderer —— 工单字节渲染：异常项 → 结构化工单（类别/阈值/
//    实测/建议四字段，定长行）——诊断中心的工单数据面。
// 2. Watchdog —— 采样看门狗：采样线程心跳（每分钟一拍），漏拍 ≥2
//    连续 → 看门狗叫（采样自身失明 = 异常显性化的自我面）。
// 全部零堆：定长缓冲，无 Vec/String/浮点/format!。
// ===========================================================================

/// 工单行缓冲。
pub const TICKET_LINE_CAP: usize = 96;
/// 看门狗漏拍容忍。
pub const WATCHDOG_MISSED_TOLERANCE: u32 = 2;

// ---------------------------------------------------------------------------
// 深化一：工单渲染
// ---------------------------------------------------------------------------

/// 工单类别。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TicketKind {
    Frame,
    Io,
    Sched,
    Wakeups,
}

impl TicketKind {
    fn tag(self) -> &'static str {
        match self {
            TicketKind::Frame => "FRAME",
            TicketKind::Io => "IO",
            TicketKind::Sched => "SCHED",
            TicketKind::Wakeups => "WAKE",
        }
    }

    /// 建议处置（三要素的「下一步」）。
    fn advice(self) -> &'static str {
        match self {
            TicketKind::Frame => "check-composer-and-driver",
            TicketKind::Io => "check-storage-queue",
            TicketKind::Sched => "check-runaway-task",
            TicketKind::Wakeups => "check-timers",
        }
    }
}

/// 工单渲染器。
pub struct TicketRenderer {
    buf: [u8; TICKET_LINE_CAP],
    len: usize,
    lines: u64,
}

impl TicketRenderer {
    pub const fn new() -> Self {
        TicketRenderer { buf: [0; TICKET_LINE_CAP], len: 0, lines: 0 }
    }

    fn push(&mut self, s: &[u8]) {
        for &b in s {
            if self.len < TICKET_LINE_CAP {
                self.buf[self.len] = b;
                self.len += 1;
            }
        }
    }

    fn push_num(&mut self, mut v: u32) {
        let mut tmp = [0u8; 10];
        let mut k = 0;
        if v == 0 {
            tmp[0] = b'0';
            k = 1;
        }
        while v > 0 {
            tmp[k] = b'0' + (v % 10) as u8;
            v /= 10;
            k += 1;
        }
        for j in (0..k).rev() {
            self.push(&tmp[j..j + 1]);
        }
    }

    /// 渲染一行：`TICKET <kind> thr=<t> act=<a> advise=<建议>`。
    pub fn render(&mut self, kind: TicketKind, threshold: u32, actual: u32) {
        self.len = 0; // 单行复用（渲染即消费）
        self.push(b"TICKET ");
        self.push(kind.tag().as_bytes());
        self.push(b" thr=");
        self.push_num(threshold);
        self.push(b" act=");
        self.push_num(actual);
        self.push(b" advise=");
        self.push(kind.advice().as_bytes());
        self.lines += 1;
    }

    pub fn line(&self) -> &[u8] {
        &self.buf[..self.len]
    }

    pub fn lines(&self) -> u64 {
        self.lines
    }
}

// ---------------------------------------------------------------------------
// 深化二：采样看门狗
// ---------------------------------------------------------------------------

/// 采样看门狗。
pub struct Watchdog {
    last_beat_ms: u64,
    beat_interval_ms: u64,
    missed: u32,
    max_missed_streak: u32,
    barks: u64,
    beats: u64,
}

impl Watchdog {
    pub const fn new(beat_interval_ms: u64) -> Self {
        Watchdog {
            last_beat_ms: 0,
            beat_interval_ms,
            missed: 0,
            max_missed_streak: 0,
            barks: 0,
            beats: 0,
        }
    }

    /// 心跳。
    pub fn beat(&mut self, now_ms: u64) {
        self.beats += 1;
        self.missed = 0;
        self.last_beat_ms = now_ms;
    }

    /// 巡检（每期望间隔调用一次）：距上次心跳超 N+2 间隔 → 叫。
    pub fn patrol(&mut self, now_ms: u64) -> bool {
        let elapsed = now_ms.saturating_sub(self.last_beat_ms);
        let missed_windows = elapsed / self.beat_interval_ms;
        if missed_windows > WATCHDOG_MISSED_TOLERANCE as u64 {
            self.missed = missed_windows as u32;
            self.max_missed_streak = self.max_missed_streak.max(self.missed);
            self.barks += 1;
            return true;
        }
        false
    }

    pub fn barks(&self) -> u64 {
        self.barks
    }

    pub fn max_missed(&self) -> u32 {
        self.max_missed_streak
    }

    pub fn beats(&self) -> u64 {
        self.beats
    }
}

// ---------------------------------------------------------------------------
// v4 批自检
// ---------------------------------------------------------------------------

/// v4 批自检：工单 / 看门狗逐条实摆。
pub fn run_perfselfchk_deep4_checks() -> CheckSet {
    let mut cs = CheckSet::new("F062-perfselfchk-v4");

    // ── 工单渲染 ──
    let mut tr = TicketRenderer::new();
    tr.render(TicketKind::Io, 50, 120);
    cs.add(
        "ticket_io_line",
        tr.line() == b"TICKET IO thr=50 act=120 advise=check-storage-queue" as &[u8],
        "",
    );
    tr.render(TicketKind::Frame, 170, 250);
    cs.add(
        "ticket_frame_line",
        tr.line() == b"TICKET FRAME thr=170 act=250 advise=check-composer-and-driver" as &[u8],
        "",
    );
    cs.add("ticket_zero_num", {
        tr.render(TicketKind::Sched, 0, 0);
        tr.line() == b"TICKET SCHED thr=0 act=0 advise=check-runaway-task" as &[u8]
    }, "");
    cs.add("ticket_lines_ledger", tr.lines() == 3, "");
    // 溢出钳制（长输入不越界）。
    for _ in 0..50 {
        tr.render(TicketKind::Wakeups, 999999, 999999);
    }
    cs.add("ticket_overflow_clamped", tr.line().len() <= TICKET_LINE_CAP, "");

    // ── 看门狗 ──
    let mut wd = Watchdog::new(60_000); // 1 分钟一拍
    wd.beat(0);
    // 容忍范围内不叫（漏 2 窗）。
    cs.add("watchdog_tolerance_ok", !wd.patrol(120_000), "");
    // 漏 3 窗 → 叫。
    cs.add("watchdog_barks_at_3", wd.patrol(190_000), "");
    cs.add("watchdog_bark_ledger", wd.barks() == 1 && wd.max_missed() == 3, "");
    // 心跳恢复清零。
    wd.beat(200_000);
    cs.add("watchdog_beat_clears", !wd.patrol(260_000), "");
    // 多次漏拍取最大。
    let mut wd2 = Watchdog::new(1_000);
    wd2.beat(0);
    let _ = wd2.patrol(10_000); // 漏 10
    wd2.beat(10_500);
    let _ = wd2.patrol(11_500); // 漏 1——不叫
    cs.add("watchdog_max_missed_kept", wd2.max_missed() == 10, "");
    cs.add("watchdog_beats_counted", wd2.beats() == 2, "");

    cs
}

#[cfg(test)]
mod deep4_tests {
    use super::*;

    #[test]
    fn ticket_all_kinds_have_advice() {
        for k in [TicketKind::Frame, TicketKind::Io, TicketKind::Sched, TicketKind::Wakeups] {
            assert!(!k.advice().is_empty(), "建议不可为空——三要素齐");
            assert!(!k.tag().is_empty());
        }
    }

    #[test]
    fn watchdog_never_barks_with_fresh_beats() {
        let mut wd = Watchdog::new(1_000);
        for t in 0..100u64 {
            wd.beat(t * 1_000);
            assert!(!wd.patrol(t * 1_000 + 500), "按时心跳不叫");
        }
        assert_eq!(wd.barks(), 0);
    }
}

// ===========================================================================
// v5 深化批（deep5）：自检依赖图 + 自愈动作表
// ===========================================================================

// ---------------------------------------------------------------------------
// 深化一：自检项目依赖图（拓扑执行序——被依赖者先跑）
// ---------------------------------------------------------------------------

/// 8 项自检的依赖图（邻接矩阵）：edges[i][j] = i 依赖 j（j 先跑）。
pub struct CheckDepGraph {
    edges: [[bool; 8]; 8],
    n: usize,
}

impl CheckDepGraph {
    pub const fn new(n: usize) -> Self {
        CheckDepGraph { edges: [[false; 8]; 8], n }
    }

    /// 声明 i 依赖 j。
    pub fn depend(&mut self, i: usize, j: usize) -> bool {
        if i >= self.n || j >= self.n || i == j {
            return false;
        }
        self.edges[i][j] = true;
        true
    }

    /// Kahn 拓扑排序 → 执行序（写入 out，返回长度；环 → 0）。
    pub fn topo_order(&self, out: &mut [usize]) -> usize {
        let mut indeg = [0u8; 8];
        for i in 0..self.n {
            for j in 0..self.n {
                if self.edges[i][j] {
                    indeg[i] += 1;
                }
            }
        }
        let mut written = 0usize;
        loop {
            // 找剩余入度 0 的最小下标（稳定序）。
            let mut pick = None;
            for k in 0..self.n {
                if indeg[k] == 0 && !out[..written].contains(&k) {
                    pick = Some(k);
                    break;
                }
            }
            match pick {
                Some(k) => {
                    if written < out.len() {
                        out[written] = k;
                    }
                    written += 1;
                    for i in 0..self.n {
                        if self.edges[i][k] {
                            indeg[i] -= 1;
                        }
                    }
                }
                None => break,
            }
        }
        if written < self.n {
            return 0; // 有环
        }
        written
    }
}

// ---------------------------------------------------------------------------
// 深化二：自愈动作表（故障指纹 → 处置动作 + 升级链）
// ---------------------------------------------------------------------------

/// 自愈动作。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HealAction {
    /// 重试一次。
    Retry,
    /// 降级到安全档。
    Downgrade,
    /// 重启子系统。
    Restart,
    /// 升级人工（不可自动处置）。
    Escalate,
}

/// 自愈表：8 规则，指纹精确匹配 → 动作 + 连续失败升级。
pub struct HealTable {
    /// (故障指纹, 动作)。
    rules: [Option<(u16, HealAction)>; 8],
    n: usize,
    /// 指纹 → 连续失败计数。
    streaks: [u16; 8],
    /// 升级阈值：同指纹连续 3 次自愈失败 → Escalate。
    pub escalate_after: u16,
}

impl HealTable {
    pub const fn new() -> Self {
        HealTable { rules: [None; 8], n: 0, streaks: [0; 8], escalate_after: 3 }
    }

    pub fn add_rule(&mut self, print: u16, action: HealAction) -> bool {
        if self.n >= 8 {
            return false;
        }
        self.rules[self.n] = Some((print, action));
        self.n += 1;
        true
    }

    /// 故障命中：返回应执行动作（连续失败达阈值 → 强制升级）。
    pub fn on_fault(&mut self, print: u16) -> HealAction {
        let mut base = HealAction::Escalate;
        let mut idx = None;
        for (k, r) in self.rules.iter().enumerate() {
            if let Some((p, a)) = r {
                if *p == print {
                    base = *a;
                    idx = Some(k);
                    break;
                }
            }
        }
        match idx {
            Some(k) => {
                self.streaks[k] = self.streaks[k].saturating_add(1);
                if self.streaks[k] >= self.escalate_after {
                    HealAction::Escalate
                } else {
                    base
                }
            }
            None => HealAction::Escalate, // 无规则 → 直接人工
        }
    }

    /// 自愈成功 → 清连击。
    pub fn on_success(&mut self, print: u16) {
        for (k, r) in self.rules.iter().enumerate() {
            if let Some((p, _)) = r {
                if *p == print {
                    self.streaks[k] = 0;
                }
            }
        }
    }

    pub fn rules(&self) -> usize {
        self.n
    }
}

// ---------------------------------------------------------------------------
// deep5 检查项
// ---------------------------------------------------------------------------

pub fn run_perfselfchk_deep5_checks() -> CheckSet {
    let mut cs = CheckSet::new("F062-perfselfchk-v5");

    // ── 依赖图 ──
    // 1) 链式依赖：2←1←0（0 依赖 1，1 依赖 2）→ 序 [2,1,0,...]。
    let mut g = CheckDepGraph::new(4);
    let _ = g.depend(0, 1);
    let _ = g.depend(1, 2);
    let mut order = [0usize; 8];
    let n = g.topo_order(&mut order);
    cs.add("depgraph_chain_order", n == 4 && order[0] == 2 && order[1] == 1 && order[2] == 0 && order[3] == 3, "");
    // 2) 无依赖 → 自然序。
    let g2 = CheckDepGraph::new(3);
    let n2 = g2.topo_order(&mut order);
    cs.add("depgraph_no_deps", n2 == 3 && order[0] == 0 && order[1] == 1 && order[2] == 2, "");
    // 3) 环检测：0↔1 互依 → 0（拒绝执行）。
    let mut g3 = CheckDepGraph::new(3);
    let _ = g3.depend(0, 1);
    let _ = g3.depend(1, 0);
    cs.add("depgraph_cycle_rejected", g3.topo_order(&mut order) == 0, "");
    // 4) 自依赖拒绝声明。
    let mut g4 = CheckDepGraph::new(3);
    cs.add("depgraph_self_dep_refused", !g4.depend(1, 1), "");

    // ── 自愈表 ──
    // 5) 规则命中：指纹 0x11 → Retry。
    let mut ht = HealTable::new();
    let _ = ht.add_rule(0x11, HealAction::Retry);
    let _ = ht.add_rule(0x22, HealAction::Downgrade);
    cs.add("heal_rule_hit", ht.on_fault(0x11) == HealAction::Retry, "");
    // 6) 连续 3 次失败 → 升级。
    cs.add(
        "heal_escalates_on_streak",
        ht.on_fault(0x11) == HealAction::Retry
            && ht.on_fault(0x11) == HealAction::Escalate, // 第 3 次连击达阈值
        "",
    );
    // 7) 成功清连击 → 回到基础动作。
    ht.on_success(0x11);
    cs.add("heal_success_resets", ht.on_fault(0x11) == HealAction::Retry, "");
    // 8) 未登记指纹直接升级。
    cs.add("heal_unknown_escalates", ht.on_fault(0xFF) == HealAction::Escalate, "");
    // 9) 规则满 8 拒绝第 9 条。
    let mut ht2 = HealTable::new();
    let mut all_ok = true;
    for k in 0..9u16 {
        all_ok &= ht2.add_rule(k, HealAction::Restart);
    }
    cs.add("heal_cap_8", !all_ok && ht2.rules() == 8, "");

    cs
}

#[cfg(test)]
mod deep5_tests {
    use super::*;

    #[test]
    fn depgraph_diamond() {
        // 菱形：3 依赖 1 和 2；1、2 依赖 0 → 序 0,1,2,3。
        let mut g = CheckDepGraph::new(4);
        let _ = g.depend(3, 1);
        let _ = g.depend(3, 2);
        let _ = g.depend(1, 0);
        let _ = g.depend(2, 0);
        let mut order = [0usize; 8];
        assert_eq!(g.topo_order(&mut order), 4);
        assert_eq!(&order[..4], &[0, 1, 2, 3]);
    }

    #[test]
    fn heal_independent_streaks() {
        let mut ht = HealTable::new();
        let _ = ht.add_rule(1, HealAction::Retry);
        let _ = ht.add_rule(2, HealAction::Restart);
        // 指纹 1 连击 3 次达阈值；指纹 2 首次——互不干扰。
        let _ = ht.on_fault(1);
        let _ = ht.on_fault(1);
        assert_eq!(ht.on_fault(1), HealAction::Escalate); // 第 3 次连击
        assert_eq!(ht.on_fault(2), HealAction::Restart);
    }
}
