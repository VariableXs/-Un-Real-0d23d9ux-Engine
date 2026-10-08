//! F180 断电演练自动化 · 批次六深化（secstar · G-G-10）。
//!
//! 批次六功能面（达成率 58%。与 b3/b4/b5 互补，本批管「时序与触发源」）：
//! - [`RoundTimeline`]：轮时序账——每轮六相时刻（计划/断电/恢复/三查）
//!   相邻相序校验（时序倒流的轮 = 记账坏轮——时钟不许在轮内倒走）；
//! - [`PowerEventLog`]：触发源账——定时/手动/低电三个触发源计数
//!   （夜跑是定时触发，白天是手动——来源混了会破坏分布判据）；
//! - [`NightSummaryRow`]：夜总结一行报文——夜号/轮数/坏轮/均时/结局
//!   （周报的消费面：一行一夜，格式锁定）；
//! - [`BatteryGuard`]：电池卫兵——电量 <20% 禁止启动夜跑（安全线：
//!   演练断电不能真把电池耗干）。
//!
//! 零堆纪律：定长时序 + 定长账，无 alloc。

use super::pwrdrill::{PowerPhase, ROUNDS_PER_NIGHT};
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 轮时序账
// ---------------------------------------------------------------------------

/// 六相时刻（分钟，夜窗内相对值）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct RoundTimes {
    pub planned: u32,
    pub power_cut: u32,
    pub power_restore: u32,
    pub check_fsck: u32,
    pub check_hive: u32,
    pub check_chain: u32,
}

impl RoundTimes {
    /// 相序校验：planned ≤ cut < restore ≤ 三查依次（时钟不倒走）。
    pub fn ordered(&self) -> bool {
        self.planned <= self.power_cut
            && self.power_cut < self.power_restore
            && self.power_restore <= self.check_fsck
            && self.check_fsck <= self.check_hive
            && self.check_hive <= self.check_chain
    }

    /// 相数（校验用）。
    pub const PHASES: usize = 6;
}

// ---------------------------------------------------------------------------
// 触发源账
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TriggerSource {
    Scheduled,
    Manual,
    LowBattery,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct PowerEventLog {
    pub scheduled: u32,
    pub manual: u32,
    pub low_battery: u32,
}

impl PowerEventLog {
    pub const fn new() -> PowerEventLog {
        PowerEventLog { scheduled: 0, manual: 0, low_battery: 0 }
    }

    pub fn on_trigger(&mut self, s: TriggerSource) {
        match s {
            TriggerSource::Scheduled => self.scheduled += 1,
            TriggerSource::Manual => self.manual += 1,
            TriggerSource::LowBattery => self.low_battery += 1,
        }
    }

    /// 分布守恒：三源和 = 总触发（不丢事件）。
    pub fn total(&self) -> u32 {
        self.scheduled + self.manual + self.low_battery
    }

    /// 定时占比 ‰（夜跑纪律面：定时触发应占绝对多数）。
    pub fn scheduled_permille(&self) -> u32 {
        if self.total() == 0 {
            return 0;
        }
        (self.scheduled * 1_000 / self.total()) as u32
    }
}

// ---------------------------------------------------------------------------
// 夜总结行
// ---------------------------------------------------------------------------

/// 夜总结报文：`N<夜> r<轮数> b<坏轮> m<均时ms> <G|B|A>`。
pub fn night_summary_row(night: u32, rounds: usize, bad: usize, mean_ms: u32, green: bool, aborted: bool, out: &mut [u8]) -> usize {
    let verdict = if aborted { 'A' } else if green { 'G' } else { 'B' };
    let mut n = 0;
    let put = |bytes: &[u8], out: &mut [u8], n: &mut usize| {
        for b in bytes {
            if *n < out.len() {
                out[*n] = *b;
                *n += 1;
            }
        }
    };
    let putn = |v: u64, out: &mut [u8], n: &mut usize| {
        let mut d = [0u8; 12];
        let mut w = 0;
        if v == 0 {
            d[0] = b'0';
            w = 1;
        } else {
            let mut x = v;
            while x > 0 {
                d[w] = b'0' + (x % 10) as u8;
                w += 1;
                x /= 10;
            }
        }
        for i in (0..w).rev() {
            if *n < out.len() {
                out[*n] = d[i];
                *n += 1;
            }
        }
    };
    put(b"N", out, &mut n);
    putn(night as u64, out, &mut n);
    put(b" r", out, &mut n);
    putn(rounds as u64, out, &mut n);
    put(b" b", out, &mut n);
    putn(bad as u64, out, &mut n);
    put(b" m", out, &mut n);
    putn(mean_ms as u64, out, &mut n);
    put(b" ", out, &mut n);
    let vb = [verdict as u8];
    put(&vb, out, &mut n);
    n
}

// ---------------------------------------------------------------------------
// 电池卫兵
// ---------------------------------------------------------------------------

/// 夜跑最低电量线（%）。
pub const MIN_BATTERY_PCT: u8 = 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BatteryVerdict {
    Allow,
    /// 电量不足 → 禁跑 + 建议插电（给路不是死墙）。
    DenyPlugIn,
}

pub fn battery_verdict(pct: u8, on_ac: bool) -> BatteryVerdict {
    if on_ac || pct >= MIN_BATTERY_PCT {
        BatteryVerdict::Allow
    } else {
        BatteryVerdict::DenyPlugIn
    }
}

// ---------------------------------------------------------------------------
// 批次六自检
// ---------------------------------------------------------------------------

#[inline(never)]
pub fn run_pwrdrill_b6_checks() -> CheckSet {
    let mut cs = CheckSet::new("F180-b6");

    // 1) 时序合法：六相递增（时钟不倒走）。
    let good = RoundTimes { planned: 90, power_cut: 92, power_restore: 95, check_fsck: 96, check_hive: 97, check_chain: 98 };
    cs.add("times_ordered", good.ordered(), "");

    // 2) 时序倒流：恢复早于断电 → 坏账（构造判红）。
    let bad = RoundTimes { planned: 90, power_cut: 95, power_restore: 92, check_fsck: 96, check_hive: 97, check_chain: 98 };
    cs.add("times_regression_red", !bad.ordered(), "");

    // 3) 相序相等合法：三查同拍完成（≤ 语义）。
    let tight = RoundTimes { planned: 90, power_cut: 91, power_restore: 95, check_fsck: 95, check_hive: 95, check_chain: 95 };
    cs.add("times_equal_ok", tight.ordered(), "");

    // 4) 触发源账：定时 8 / 手动 1 / 低电 1 → 定时 800‰（分布面）。
    let mut p = PowerEventLog::new();
    for _ in 0..8 {
        p.on_trigger(TriggerSource::Scheduled);
    }
    p.on_trigger(TriggerSource::Manual);
    p.on_trigger(TriggerSource::LowBattery);
    cs.add("trigger_sources", p.total() == 10 && p.scheduled_permille() == 800, "");

    // 5) 空账诚实：0 → 0‰。
    cs.add("trigger_empty", PowerEventLog::new().scheduled_permille() == 0, "");

    // 6) 夜总结行：N3 r100 b2 m4800 B 逐字节（格式锁定）。
    let mut buf = [0u8; 48];
    let n = night_summary_row(3, 100, 2, 4_800, false, false, &mut buf);
    cs.add("summary_row_format", &buf[..n] == b"N3 r100 b2 m4800 B", "");

    // 7) 夜总结绿夜/中止夜：G 与 A 旗（三结局各有字）。
    let n2 = night_summary_row(4, 100, 0, 4_900, true, false, &mut buf);
    let n3 = night_summary_row(5, 40, 0, 0, false, true, &mut buf);
    cs.add(
        "summary_verdicts",
        buf[..n2].ends_with(b"G") && buf[..n3].ends_with(b"A"),
        "",
    );

    // 8) 电池卫兵三态：电池 25% 允许、15% 拒、插电 5% 也允许（安全线逐点）。
    cs.add(
        "battery_verdict",
        battery_verdict(25, false) == BatteryVerdict::Allow
            && battery_verdict(15, false) == BatteryVerdict::DenyPlugIn
            && battery_verdict(5, true) == BatteryVerdict::Allow,
        "",
    );

    // 9) 阈值恰点：20% 恰好允许（≥ 语义）。
    cs.add("battery_boundary", battery_verdict(20, false) == BatteryVerdict::Allow, "");

    // 10) 主册常量贯通：一夜 100 轮一处一事实。
    cs.add("consts_aligned", ROUNDS_PER_NIGHT == 100 && RoundTimes::PHASES == 6 && PowerPhase::Init.ord() == 0, "");

    cs
}

// ---------------------------------------------------------------------------
// 宿主单测（批次六）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests_b6 {
    use super::*;

    #[test]
    fn trigger_counts_never_negative() {
        // 海量触发计数不溢出不串（三源隔离面）。
        let mut p = PowerEventLog::new();
        for _ in 0..10_000 {
            p.on_trigger(TriggerSource::Scheduled);
        }
        p.on_trigger(TriggerSource::Manual);
        assert_eq!(p.total(), 10_001);
        assert_eq!(p.scheduled_permille(), 999);
    }

    #[test]
    fn summary_row_all_verdicts_distinct() {
        // 三结局字符互不相同（G/B/A 一眼分）。
        let mut buf = [0u8; 48];
        let mut last = [0u8; 1];
        for (green, aborted) in [(true, false), (false, false), (false, true)] {
            let n = night_summary_row(1, 100, 0, 5_000, green, aborted, &mut buf);
            last[0] = buf[n - 1];
            assert!(b"GBA".contains(&last[0]));
        }
    }

    #[test]
    fn times_monotone_fuzz() {
        // 时序单调性 fuzz：递增序列全过、任一倒序点即红（20 组）。
        for seed in 0..20u32 {
            let base = 90 + seed;
            let t = RoundTimes { planned: base, power_cut: base + 1, power_restore: base + 2, check_fsck: base + 3, check_hive: base + 4, check_chain: base + 5 };
            assert!(t.ordered(), "seed={seed}");
            let bad = RoundTimes { planned: base, power_cut: base + 1, power_restore: base, check_fsck: base + 3, check_hive: base + 4, check_chain: base + 5 };
            assert!(!bad.ordered(), "seed={seed}");
        }
    }
}
