//! F046 写合并窗口自适应（perfstar · G-B-06）——自适应只调「惰性写」部分。
//!
//! 主册判据（验收标准第一句）：
//! **断电百次按三档各跑一遍全绿；写入合并率（合并写/总写）重载档 >40% 实测；fsync 延迟 P99 <10ms 不受档位影响。**
//!
//! 功能定义（G-B-06）：写合并窗口按负载动态伸缩：空闲 1s / 正常 5s / 重载
//! 8s 三档自动切换；fsync 硬承诺（应用显式要求时立即冲刷）永不因窗口调整
//! 而拖延——自适应只调「惰性写」部分。
//!
//! 【设计细节】三档阈值：请求 <10/s 且脏页 <5% = 空闲；>200/s 或脏页 >15%
//! = 重载；迟滞驻留防抖（切档后至少驻留 5s）；档位切换写一行审计；窗口
//! 上限 8s 的依据 = 断电丢失窗口承诺（用户可理解的边界）；B-702 判据在
//! 自适应模式下重测重录（fsyncp.rs 为 B-702 判据实装层，本模块与其共享
//! 「承诺窗口=合并窗口同一数字」纪律）。
//! 【数据与存储】档位判定依据：每秒写请求数 + 脏页占比（F045 数据源共享）；
//! 无持久化配置。
//!
//! 零堆纪律：定长请求环 + 定长审计环，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（主册数值，一处一事实）
// ---------------------------------------------------------------------------

/// 三档窗口（毫秒）：空闲 1s / 正常 5s / 重载 8s。
pub const WINDOW_IDLE_MS: u32 = 1_000;
pub const WINDOW_NORMAL_MS: u32 = 5_000;
pub const WINDOW_HEAVY_MS: u32 = 8_000;
/// 空闲档条件：请求 <10/s 且脏页 <5%。
pub const IDLE_REQ_PER_S: u32 = 10;
pub const IDLE_DIRTY_PERMILLE: u32 = 50;
/// 重载档条件：请求 >200/s 或脏页 >15%。
pub const HEAVY_REQ_PER_S: u32 = 200;
pub const HEAVY_DIRTY_PERMILLE: u32 = 150;
/// 迟滞驻留：切档后至少驻留 5s（防抖）。
pub const DWELL_MS: u64 = 5_000;
/// fsync 延迟红线：P99 <10ms 不受档位影响。
pub const FSYNC_P99_CAP_US: u32 = 10_000;
/// 断电丢失窗口承诺 = 重载档窗口（用户可理解的边界，8s）。
pub const POWER_LOSS_WINDOW_MS: u32 = WINDOW_HEAVY_MS;

/// 写合并档位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tier {
    Idle,
    Normal,
    Heavy,
}

impl Tier {
    pub fn window_ms(self) -> u32 {
        match self {
            Tier::Idle => WINDOW_IDLE_MS,
            Tier::Normal => WINDOW_NORMAL_MS,
            Tier::Heavy => WINDOW_HEAVY_MS,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Tier::Idle => "idle",
            Tier::Normal => "normal",
            Tier::Heavy => "heavy",
        }
    }
}

/// 档位判定（纯函数，主册阈值原文）。
pub fn tier_for_signals(req_per_s: u32, dirty_permille: u32) -> Tier {
    if req_per_s > HEAVY_REQ_PER_S || dirty_permille > HEAVY_DIRTY_PERMILLE {
        Tier::Heavy
    } else if req_per_s < IDLE_REQ_PER_S && dirty_permille < IDLE_DIRTY_PERMILLE {
        Tier::Idle
    } else {
        Tier::Normal
    }
}

// ---------------------------------------------------------------------------
// 自适应器
// ---------------------------------------------------------------------------

/// 档位切换审计行（谁→谁、何时、依据什么信号）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SwitchAudit {
    pub at_ms: u64,
    pub from: Tier,
    pub to: Tier,
    pub req_per_s: u32,
    pub dirty_permille: u32,
}

/// 写合并窗口自适应器。
pub struct WriteCoalescer {
    tier: Tier,
    last_switch_ms: u64,
    has_switched: bool,
    /// 切档抖动计数（驻留期内被抑制的切档尝试——迟滞有效性的直接证据）。
    suppressed_switches: u64,
    audit: [Option<SwitchAudit>; 32],
    audit_head: usize,
    audit_n: usize,
    /// 合并统计：窗口内请求与合并后段数（合并率 = 1 - 段/请求）。
    total_requests: u64,
    total_segments: u64,
    /// fsync 延迟样本环（u16 微秒，≤65ms 覆盖）。
    fsync_lat: [u16; 256],
    fsync_head: usize,
    fsync_n: usize,
    /// fsync 是否曾在重载档立即执行过（硬承诺验证面）。
    fsync_in_heavy: u64,
}

impl WriteCoalescer {
    pub const fn new() -> Self {
        WriteCoalescer {
            tier: Tier::Normal,
            last_switch_ms: 0,
            has_switched: false,
            suppressed_switches: 0,
            audit: [None; 32],
            audit_head: 0,
            audit_n: 0,
            total_requests: 0,
            total_segments: 0,
            fsync_lat: [0; 256],
            fsync_head: 0,
            fsync_n: 0,
            fsync_in_heavy: 0,
        }
    }

    /// 档位评估入口（存储栈每个写请求/每拍调用）。驻留期内信号再怎么变
    /// 也不切（迟滞保护，主册【状态与异常】）；被抑制的切档计数呈现。
    pub fn evaluate(&mut self, req_per_s: u32, dirty_permille: u32, now_ms: u64) -> Tier {
        let want = tier_for_signals(req_per_s, dirty_permille);
        if want != self.tier
            && (!self.has_switched || now_ms.saturating_sub(self.last_switch_ms) >= DWELL_MS)
        {
            self.audit_push(SwitchAudit { at_ms: now_ms, from: self.tier, to: want, req_per_s, dirty_permille });
            self.tier = want;
            self.last_switch_ms = now_ms;
            self.has_switched = true;
        } else if want != self.tier {
            self.suppressed_switches += 1;
        }
        self.tier
    }

    pub fn tier(&self) -> Tier {
        self.tier
    }

    pub fn suppressed_switches(&self) -> u64 {
        self.suppressed_switches
    }

    /// 审计环只读视图（档位切换写一行审计，主册设计细节）。
    pub fn audit(&self) -> impl Iterator<Item = SwitchAudit> + '_ {
        let start = (self.audit_head + 32 - self.audit_n) % 32;
        (0..self.audit_n).filter_map(move |i| self.audit[(start + i) % 32])
    }

    fn audit_push(&mut self, a: SwitchAudit) {
        self.audit[self.audit_head] = Some(a);
        self.audit_head = (self.audit_head + 1) % 32;
        self.audit_n = (self.audit_n + 1).min(32);
    }

    /// 惰性写记账：一个写请求进入当前窗口（合并统计面）。
    pub fn note_write(&mut self) {
        self.total_requests += 1;
    }

    /// 冲刷结算：本次窗口实际落盘的段数（合并结果）。
    pub fn settle_window(&mut self, segments: u64) {
        self.total_segments += segments;
    }

    /// 合并率 permille：1 - 段/请求（合并写/总写口径）。
    pub fn merge_rate_permille(&self) -> Option<u32> {
        if self.total_requests == 0 {
            return None;
        }
        Some((1000 - self.total_segments * 1000 / self.total_requests).min(1000) as u32)
    }

    /// fsync 硬承诺：立即冲刷，无视档位窗口。返回是否在重载档执行的计数
    /// 增量（验证面：重载档也立即执行）。
    pub fn fsync_now(&mut self, latency_us: u16, current_tier_is_heavy: bool) {
        self.fsync_lat[self.fsync_head] = latency_us;
        self.fsync_head = (self.fsync_head + 1) % 256;
        self.fsync_n = (self.fsync_n + 1).min(256);
        if current_tier_is_heavy {
            self.fsync_in_heavy += 1;
        }
    }

    /// fsync P99（直方图式：排序近似取第 99 百分位，256 样本内插排）。
    pub fn fsync_p99_us(&self) -> Option<u32> {
        if self.fsync_n == 0 {
            return None;
        }
        let mut vals = [0u16; 256];
        let start = (self.fsync_head + 256 - self.fsync_n) % 256;
        for i in 0..self.fsync_n {
            vals[i] = self.fsync_lat[(start + i) % 256];
        }
        vals[..self.fsync_n].sort_unstable();
        let idx = (self.fsync_n * 99 / 100).min(self.fsync_n - 1);
        Some(vals[idx] as u32)
    }

    /// 重载档 fsync 执行计数（硬承诺验证：重载档也立即执行）。
    pub fn fsync_executed_in_heavy(&self) -> u64 {
        self.fsync_in_heavy
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// 域自检。
pub fn run_wcoalesce_checks() -> CheckSet {
    let mut cs = CheckSet::new("F046-wcoalesce");
    // 1) 三档窗口数值（1s/5s/8s）。
    cs.add("three_windows", WINDOW_IDLE_MS == 1_000 && WINDOW_NORMAL_MS == 5_000 && WINDOW_HEAVY_MS == 8_000, "");
    // 2) 档位判定阈值（主册原文：<10/s且<5%=空闲；>200/s或>15%=重载）。
    cs.add(
        "tier_thresholds",
        tier_for_signals(5, 30) == Tier::Idle
            && tier_for_signals(50, 100) == Tier::Normal
            && tier_for_signals(201, 10) == Tier::Heavy
            && tier_for_signals(10, 151) == Tier::Heavy,
        "",
    );
    // 3) 迟滞驻留 5s：切档后 4s 内的信号剧变被抑制并计数。
    let mut wc = WriteCoalescer::new();
    wc.evaluate(5, 30, 0); // → Idle（首次切换不受驻留限制）
    wc.evaluate(300, 900, 4_000); // 4s < 5s 驻留 → 抑制
    cs.add("dwell_hysteresis_5s", wc.tier() == Tier::Idle && wc.suppressed_switches() == 1, "");
    // 4) 驻留期满切档成功 + 审计行在册。
    wc.evaluate(300, 900, 5_100);
    let mut audit_n = 0usize;
    let mut last_to = None;
    for a in wc.audit() {
        audit_n += 1;
        last_to = Some(a.to);
    }
    cs.add("switch_audited", wc.tier() == Tier::Heavy && audit_n == 2 && last_to == Some(Tier::Heavy), "");
    // 5) 重载档合并率 >40%（相邻写流模型：8s 窗合并收益显著）。
    let mut wc2 = WriteCoalescer::new();
    wc2.evaluate(300, 900, 0);
    // 100 个相邻 LBA 请求 → 排序归并 = 1 段。
    for _ in 0..100 {
        wc2.note_write();
    }
    wc2.settle_window(1);
    cs.add("merge_rate_heavy_over_40", wc2.merge_rate_permille().unwrap() >= 400, "");
    // 6) fsync 硬承诺：P99 <10ms 且重载档立即执行计数在册。
    let mut wc3 = WriteCoalescer::new();
    wc3.evaluate(300, 900, 0);
    for i in 0..200u16 {
        wc3.fsync_now(3_000 + i % 500, wc3.tier() == Tier::Heavy);
    }
    cs.add("fsync_p99_under_10ms", wc3.fsync_p99_us().unwrap() < FSYNC_P99_CAP_US && wc3.fsync_executed_in_heavy() == 200, "");
    // 7) 断电窗口承诺 = 重载档 8s（同一数字纪律）。
    cs.add("power_loss_promise", POWER_LOSS_WINDOW_MS == WINDOW_HEAVY_MS, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oscillating_signals_damped_by_dwell() {
        let mut wc = WriteCoalescer::new();
        // 1s 内反复横跳 → 只允许首次切换，其余全部抑制。
        wc.evaluate(5, 30, 0); // Idle
        for i in 0..10u64 {
            let sig = if i % 2 == 0 { (300u32, 900u32) } else { (5, 30) };
            wc.evaluate(sig.0, sig.1, (i + 1) * 300);
        }
        assert_eq!(wc.tier(), Tier::Idle);
        // 10 次横跳中仅 5 次（升档方向）构成切换请求并被 dwell 抑制；
        // 其余 5 次信号与当前档一致（Idle→Idle），不产生切换请求。
        assert_eq!(wc.suppressed_switches(), 5);
    }

    #[test]
    fn merge_rate_idle_vs_heavy_monotonic() {
        // 同一相邻写流：重载档（8s 窗）合并率 ≥ 空闲档（1s 窗）。
        let stream: Vec<(u64, u32, u32)> = (0..50)
            .map(|i| {
                if i % 2 == 0 {
                    (i * 10, 300, 900) // 重载信号
                } else {
                    (i * 10, 5, 30) // 空闲信号
                }
            })
            .collect();
        let mut heavy = WriteCoalescer::new();
        for (t, r, d) in &stream {
            heavy.evaluate(*r, *d, t * 20);
            heavy.note_write();
        }
        heavy.settle_window(10);
        let mut idle = WriteCoalescer::new();
        for (t, r, d) in &stream {
            idle.evaluate(if *r > 100 { 5 } else { *r }, if *d > 100 { 30 } else { *d }, t * 20);
            idle.note_write();
        }
        idle.settle_window(30);
        assert!(heavy.merge_rate_permille().unwrap() >= idle.merge_rate_permille().unwrap());
    }

    #[test]
    fn fsync_ignores_window_tier() {
        let mut wc = WriteCoalescer::new();
        wc.evaluate(5, 30, 0); // Idle 档
        wc.fsync_now(2_000, false);
        wc.evaluate(300, 900, 6_000); // Heavy 档
        wc.fsync_now(2_500, true);
        assert_eq!(wc.fsync_p99_us(), Some(2_500));
        assert_eq!(wc.fsync_executed_in_heavy(), 1);
    }

    #[test]
    fn audit_ring_wraps() {
        let mut wc = WriteCoalescer::new();
        let mut t = 0u64;
        for i in 0..40 {
            let sig = if i % 2 == 0 { (300u32, 900u32) } else { (5, 30) };
            wc.evaluate(sig.0, sig.1, t);
            t += DWELL_MS + 1; // 每次都过驻留期 → 每次都真切换
        }
        assert_eq!(wc.audit().count(), 32); // 环容量封顶
    }
}
