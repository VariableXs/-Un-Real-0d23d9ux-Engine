//! mech_gov — cpufreq 调频策略本体（AI-K1 深化批次五 · F048）。
//!
//! 主册依据：
//! - F048【设计细节】「调频」——此前域内有负载信号账与温度联动表，但
//!   **频率决策算法本体**缺席：负载多高跳满频、降频为什么必须慢、
//!   util→freq 怎么映射、两次调频之间为什么要限速。本件按 Linux 两支
//!   主流 governor 语义实现：
//!   · ondemand：负载超 UP_THRESHOLD 一步跳满（响应突发），降频逐级走
//!     且受 sampling_down_factor 节流（防抖动），无跳变中间档按
//!     freq × load / 100 落表。
//!   · schedutil：util 直接映射 freq（1.25 倍头寸），跟随调度器节奏，
//!     rate limit 两次调频最小间隔。
//! - 锚点：F048「性能档与功耗的联动表」的**联动算法**；F047 输入类
//!   预算的上游——核不够快时预算必然爆，governor 是第一道防线。
//! - 零堆、零浮点（load 用 permille 千分数）。

// ---------------------------------------------------------------------------
// 1. 频率表
// ---------------------------------------------------------------------------

/// 频率表 MHz（升序；一处一事实：消费域按平台表实例化）。
pub const FREQ_TABLE_MHZ: [u32; 6] = [800, 1200, 1800, 2400, 3000, 3400];

/// ondemand 满频跳转阈值（permille，Linux 默认 80%）。
pub const OD_UP_THRESHOLD_PM: u32 = 800;
/// 降频节流系数：连续 N 个采样窗才允许降一级。
pub const OD_DOWN_FACTOR: u32 = 5;
/// schedutil util→freq 头寸（1.25 倍，×1000/800）。
pub const SU_HEADROOM_PM: u32 = 1250;
/// schedutil 速率限制（两次调频最小间隔 μs，Linux 默认 1000μs 级）。
pub const SU_RATE_LIMIT_US: u64 = 1000;

// ---------------------------------------------------------------------------
// 2. ondemand governor
// ---------------------------------------------------------------------------

/// ondemand 决策器（状态 = 采样计数 + 当前档）。
pub struct OndemandGov {
    level: usize,
    /// 距上次降频已过的采样窗数。
    since_down: u32,
    /// 统计面：升档/降档/保持次数。
    pub ups: u32,
    pub downs: u32,
    pub holds: u32,
}

impl OndemandGov {
    pub const fn new() -> Self {
        OndemandGov { level: 0, since_down: 0, ups: 0, downs: 0, holds: 0 }
    }

    pub fn level(&self) -> usize {
        self.level
    }

    pub fn freq_mhz(&self) -> u32 {
        FREQ_TABLE_MHZ[self.level]
    }

    /// 采样一次负载（permille 0..=1000），返回决策（目标档）。
    pub fn sample(&mut self, load_pm: u32) -> usize {
        let target = if load_pm >= OD_UP_THRESHOLD_PM {
            // 突发：一步满频。
            FREQ_TABLE_MHZ.len() - 1
        } else {
            // 目标频率 = max × load / 1000，落到 ≤ 该值的最大表项。
            let want = (FREQ_TABLE_MHZ[FREQ_TABLE_MHZ.len() - 1] as u64)
                .saturating_mul(load_pm as u64)
                / 1000;
            let mut t = 0usize;
            for (i, f) in FREQ_TABLE_MHZ.iter().enumerate() {
                if (*f as u64) <= want {
                    t = i;
                }
            }
            t
        };
        if target > self.level {
            // 升：允许一步跳到目标（突发友好）。
            self.level = target;
            self.since_down = 0;
            self.ups += 1;
        } else if target < self.level {
            // 降：逐级 + sampling_down_factor 节流。
            self.since_down += 1;
            if self.since_down >= OD_DOWN_FACTOR {
                self.level -= 1;
                self.since_down = 0;
                self.downs += 1;
            } else {
                self.holds += 1;
            }
        } else {
            self.since_down = 0;
            self.holds += 1;
        }
        self.level
    }
}

// ---------------------------------------------------------------------------
// 3. schedutil governor
// ---------------------------------------------------------------------------

/// schedutil 决策器（util 直映 + 速率限制）。
pub struct SchedutilGov {
    level: usize,
    /// 上次调频时刻（μs）。
    last_change_us: i64,
    /// 挂起的请求档（rate limit 窗口内先记，窗口到再执行）。
    pub pending_kept: u32,
}

impl SchedutilGov {
    pub const fn new() -> Self {
        SchedutilGov { level: 0, last_change_us: i64::MIN / 2, pending_kept: 0 }
    }

    pub fn level(&self) -> usize {
        self.level
    }

    /// util（0..=1024，调度器容量口径）+ 当前时刻（μs）→ 决策档。
    pub fn update(&mut self, util: u32, now_us: i64) -> usize {
        // freq = 1.25 × util / 1024 × max，落表（取 ≥ 的最小表项——
        // schedutil 向上取整以满足头寸；饱和钳 max）。
        let max = FREQ_TABLE_MHZ[FREQ_TABLE_MHZ.len() - 1] as u64;
        let want = max
            .saturating_mul(SU_HEADROOM_PM as u64)
            .saturating_mul(util.min(1024) as u64)
            / (1024 * 1000);
        let mut t = FREQ_TABLE_MHZ.len() - 1;
        for (i, f) in FREQ_TABLE_MHZ.iter().enumerate() {
            if (*f as u64) >= want {
                t = i;
                break;
            }
        }
        // 速率限制：窗口内不实际改频（请求被记为 pending）。
        if (now_us - self.last_change_us) < SU_RATE_LIMIT_US as i64 {
            self.pending_kept += 1;
            return self.level;
        }
        if t != self.level {
            self.level = t;
            self.last_change_us = now_us;
        }
        self.level
    }
}

// ---------------------------------------------------------------------------
// 4. CheckSet
// ---------------------------------------------------------------------------

pub fn run_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    let mut cs = CheckSet::new("mech_gov");

    // 1) ondemand 突发响应：低负载平稳期一记 95% 立刻满档。
    {
        let mut g = OndemandGov::new();
        for _ in 0..10 {
            g.sample(200);
        }
        let lvl = g.sample(950);
        cs.add("od_burst_to_max", lvl == FREQ_TABLE_MHZ.len() - 1, "");
    }

    // 2) ondemand 降频慢：满档后负载骤降，降一级要等 DOWN_FACTOR 个窗。
    {
        let mut g = OndemandGov::new();
        g.sample(950);
        g.sample(100); // 不够 5 窗 → 不降
        let after_one = g.level();
        for _ in 0..(OD_DOWN_FACTOR - 2) {
            g.sample(100);
        }
        let after_five = g.level();
        cs.add("od_down_slow", after_one == FREQ_TABLE_MHZ.len() - 1 && after_five == FREQ_TABLE_MHZ.len() - 2, "");
    }

    // 3) ondemand 中载映射：load 40% → want = 3400×0.4 = 1360 → 表项 1200。
    {
        let mut g = OndemandGov::new();
        let lvl = g.sample(400);
        cs.add("od_mid_map", lvl == 1, "");
    }

    // 4) schedutil 比例：util=1024 → 满频；util=512 → 1.25×0.5×3400=2125 → 2400 档。
    {
        let mut a = SchedutilGov::new();
        let hi = a.update(1024, 10_000);
        let mut b = SchedutilGov::new();
        let mid = b.update(512, 10_000);
        cs.add("su_proportional", hi == FREQ_TABLE_MHZ.len() - 1 && mid == 3, "");
    }

    // 5) schedutil 速率限制：1ms 内连发两次不同请求，第二次不动。
    {
        let mut g = SchedutilGov::new();
        g.update(1024, 10_000);
        let lvl = g.update(0, 10_500); // 窗口内
        cs.add("su_rate_limit", lvl == FREQ_TABLE_MHZ.len() - 1 && g.pending_kept == 1, "");
    }

    // 6) 表边界诚实：零负载不越下界、满负载不越上界。
    {
        let mut g = OndemandGov::new();
        g.sample(0);
        let lo = g.freq_mhz();
        g.sample(1000);
        let hi = g.freq_mhz();
        cs.add("gov_bounds", lo == FREQ_TABLE_MHZ[0] && hi == FREQ_TABLE_MHZ[5], "");
    }

    cs
}

// ---------------------------------------------------------------------------
// 5. 宿主单测
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ondemand_burst_then_throttled_descent() {
        let mut g = OndemandGov::new();
        // 平稳期在低档。
        for _ in 0..12 {
            g.sample(150);
        }
        assert_eq!(g.freq_mhz(), 800);
        // 突发 → 满频一步。
        g.sample(900);
        assert_eq!(g.freq_mhz(), 3400);
        // 负载骤降：连续 DOWN_FACTOR 窗才降一级。
        for _ in 0..(OD_DOWN_FACTOR - 1) {
            g.sample(50);
        }
        assert_eq!(g.freq_mhz(), 3400, "节流期内不许降");
        g.sample(50); // 第 5 窗
        assert_eq!(g.freq_mhz(), 3000);
        // 再来 5 窗再降一级。
        for _ in 0..OD_DOWN_FACTOR {
            g.sample(50);
        }
        assert_eq!(g.freq_mhz(), 2400);
        // 升降计数对账：12 平稳 holds + 1 up + 4 holds + 1 down + 4 holds + 1 down。
        assert_eq!(g.ups + g.downs + g.holds, 23);
        assert_eq!(g.ups, 1);
        assert_eq!(g.downs, 2);
    }

    #[test]
    fn ondemand_mid_load_lands_below_target() {
        // 50% → want 1700 → 表项 ≤1700 的最大是 1200（档 1）。
        let mut g = OndemandGov::new();
        let l = g.sample(500);
        assert_eq!(l, 1);
        assert_eq!(FREQ_TABLE_MHZ[l], 1200);
    }

    #[test]
    fn ondemand_threshold_boundary() {
        // 恰好 800（阈值）→ 满频路径；799 → 映射路径（3400×0.799=2717 → 2400）。
        let mut a = OndemandGov::new();
        assert_eq!(a.sample(800), 5);
        let mut b = OndemandGov::new();
        assert_eq!(b.sample(799), 3);
    }

    #[test]
    fn schedutil_maps_headroom() {
        let mut g = SchedutilGov::new();
        // util=0 → want=0 → 表最低项（≥0 的最小 = 800）。
        assert_eq!(g.update(0, 0), 0);
        // util=1024 → want=4250 > max → 满档。
        assert_eq!(g.update(1024, 10_000), 5);
        // util=256 → want=1062 → ≥1062 的最小表项 = 1200。
        assert_eq!(g.update(256, 20_000), 1);
        // util=384 → want=1594 → 1800。
        assert_eq!(g.update(384, 30_000), 2);
    }

    #[test]
    fn schedutil_rate_limit_defers_inside_window() {
        let mut g = SchedutilGov::new();
        g.update(0, 0);
        // 同窗口内请求满频 → 被压。
        let l = g.update(1024, (SU_RATE_LIMIT_US - 1) as i64);
        assert_eq!(l, 0);
        assert!(g.pending_kept >= 1);
        // 窗口外 → 执行。
        g.update(1024, (SU_RATE_LIMIT_US + 1) as i64);
        assert_eq!(g.level(), 5);
    }

    #[test]
    fn never_leaves_table() {
        let mut a = OndemandGov::new();
        for pm in [0u32, 100, 500, 799, 800, 999, 1000, 1500] {
            let l = a.sample(pm);
            assert!(l < FREQ_TABLE_MHZ.len());
        }
        let mut b = SchedutilGov::new();
        for u in [0u32, 1, 512, 1023, 1024, 5000] {
            let l = b.update(u, 100_000);
            assert!(l < FREQ_TABLE_MHZ.len());
        }
    }
}
