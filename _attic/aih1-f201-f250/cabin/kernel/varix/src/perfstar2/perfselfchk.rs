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
