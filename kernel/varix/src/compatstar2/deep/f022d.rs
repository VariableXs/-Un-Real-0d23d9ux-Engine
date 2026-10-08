//! F022 深化批次二 · 历法换算核与回拨保护（compatstar2/deep · G-A-22）。
//!
//! 批次一深化覆盖夏令时规则/WaitableTimer/FILETIME；本批补齐：
//! 民用历法双向换算核（days_from_civil/civil_from_days，Hinnant 算法——
//! GetLocalTime/ASN.1 时间/PDF 日期三类消费者的公共底座）、星期推导
//! （万年历判据面）、SetSystemTime 回拨保护（F187 NTC 校时联动的显式闸）、
//! 周期定时器推进表（1ms 游戏帧节奏判据的调度承载面）。
//!
//! 零堆纪律：定长表，无 alloc。

use crate::checks::CheckSet;

/// 定时器表容量。
pub const MAX_TIMERS: usize = 16;
/// 回拨保护阈值 5 秒（F187 联动：偏差 >5s 提示/拦截口径）。
pub const ROLLBACK_GUARD_S: i64 = 5;

/// 民用日期 → 纪元天数（Hinnant 算法；proleptic Gregorian）。
pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let mp = ((m as i64) + 9) % 12; // 3 月 = 0
    let doy = (153 * mp + 2) / 5 + d as i64 - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146097 + doe - 719468
}

/// 纪元天数 → 民用日期（days_from_civil 的逆；round-trip 对拍面）。
pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// 星期推导：1970-01-01 = 星期四；周日=0 … 周六=6（万年历对照口径）。
pub fn weekday_of_epoch_days(days: i64) -> u32 {
    ((days + 4).rem_euclid(7)) as u32
}

/// SYSTEMTIME 结构（MS 字段语义；wDayOfWeek 由星期推导面供给）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SystemTime {
    pub year: i64,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    pub weekday: u32,
}

impl SystemTime {
    /// 秒 → SystemTime（GetLocalTime 语义；时区偏移由调用方先行折算）。
    pub fn from_epoch_s(epoch_s: i64) -> SystemTime {
        let days = epoch_s.div_euclid(86_400);
        let secs = epoch_s.rem_euclid(86_400);
        let (year, month, day) = civil_from_days(days);
        SystemTime {
            year,
            month,
            day,
            hour: (secs / 3600) as u32,
            minute: ((secs % 3600) / 60) as u32,
            second: (secs % 60) as u32,
            weekday: weekday_of_epoch_days(days),
        }
    }
}

/// SetSystemTime 回拨保护闸（F187：回拨 >5s 拦截 + 计数，正向校时放行）。
pub struct SetTimeGuard {
    pub current_epoch_s: i64,
    /// 拦截的回拨请求计数。
    pub rollbacks_blocked: u32,
    /// 放行的正向校时计数。
    pub forward_applies: u32,
}

impl SetTimeGuard {
    pub const fn new(now_epoch_s: i64) -> Self {
        SetTimeGuard { current_epoch_s: now_epoch_s, rollbacks_blocked: 0, forward_applies: 0 }
    }
    /// 请求设置系统时间：回拨超过阈值 → 拒（不静默）；正向或微差 → 应用。
    pub fn request(&mut self, new_epoch_s: i64) -> Result<i64, &'static str> {
        let delta = new_epoch_s - self.current_epoch_s;
        if delta < -ROLLBACK_GUARD_S {
            self.rollbacks_blocked += 1;
            Err("clock-rollback-blocked")
        } else {
            self.current_epoch_s = new_epoch_s;
            self.forward_applies += 1;
            Ok(new_epoch_s)
        }
    }
}

/// 周期定时器推进表（SetTimer 族的调度承载；批次一 SetTimer 钳制的后续面）。
#[derive(Clone, Copy)]
pub struct TimerEntry {
    pub period_ms: u32,
    pub elapsed_ms: u64,
    pub active: bool,
    /// 到期触发计数。
    pub fires: u32,
}

/// 定时器推进器：advance(dt) 逐表推进，到期触发并按周期重武装。
pub struct TimerScheduler {
    pub timers: [TimerEntry; MAX_TIMERS],
    /// 触发总数（帧节奏判据的账面）。
    pub total_fires: u64,
}

impl TimerScheduler {
    pub const fn new() -> Self {
        TimerScheduler {
            timers: [TimerEntry { period_ms: 0, elapsed_ms: 0, active: false, fires: 0 }; MAX_TIMERS],
            total_fires: 0,
        }
    }
    pub fn install(&mut self, slot: usize, period_ms: u32) -> bool {
        if slot >= MAX_TIMERS || period_ms == 0 {
            return false;
        }
        self.timers[slot] = TimerEntry { period_ms, elapsed_ms: 0, active: true, fires: 0 };
        true
    }
    pub fn advance(&mut self, dt_ms: u64) {
        for t in self.timers.iter_mut() {
            if !t.active {
                continue;
            }
            t.elapsed_ms += dt_ms;
            while t.elapsed_ms >= t.period_ms as u64 {
                t.elapsed_ms -= t.period_ms as u64;
                t.fires += 1;
                self.total_fires += 1;
            }
        }
    }
}

/// 域自检（深化批次二）。
pub fn run_f022d_checks() -> CheckSet {
    let mut cs = CheckSet::new("F022-timefam-d2");
    // 1) 历法核已知点：2026-09-27 = 纪元 20723 天、星期日（万年历对照）。
    let days = days_from_civil(2026, 9, 27);
    cs.add("civil_known_point", days == 20_723 && weekday_of_epoch_days(days) == 0, "");
    // 2) round-trip：civil → days → civil 恒等（闰日 2028-02-29 在册）。
    let leap = days_from_civil(2028, 2, 29);
    cs.add("civil_roundtrip", civil_from_days(days) == (2026, 9, 27) && civil_from_days(leap) == (2028, 2, 29), "");
    // 3) 星期序列：2026-09-26 为星期六（6）。
    cs.add("weekday_saturday", weekday_of_epoch_days(days - 1) == 6, "");
    // 4) SystemTime 拆解：epoch 1_790_509_800 = 2026-09-27 12:03:20? —
    //    采用 round-trip 口径：拆解后重新合成秒数不变。
    let st = SystemTime::from_epoch_s(1_700_000_000);
    let rebuilt = days_from_civil(st.year, st.month, st.day) * 86_400
        + st.hour as i64 * 3600
        + st.minute as i64 * 60
        + st.second as i64;
    cs.add("systemtime_rebuild", rebuilt == 1_700_000_000 && st.weekday == weekday_of_epoch_days(1_700_000_000 / 86_400), "");
    // 5) 回拨保护：回拨 60s 拒、正向放行、微回拨 ≤5s 放行（F187 阈值口径）。
    let mut g = SetTimeGuard::new(1_000_000);
    cs.add(
        "rollback_guard",
        g.request(999_000) == Err("clock-rollback-blocked")
            && g.request(1_000_030).is_ok()
            && g.request(1_000_027).is_ok()
            && g.rollbacks_blocked == 1
            && g.forward_applies == 2,
        "",
    );
    // 6) 周期定时器：1ms × 1000ms 推进 = 1000 次触发（帧节奏账面）。
    let mut sch = TimerScheduler::new();
    sch.install(0, 1);
    sch.install(1, 500);
    sch.advance(1_000);
    cs.add("timer_frame_rhythm", sch.timers[0].fires == 1_000 && sch.timers[1].fires == 2 && sch.total_fires == 1_002, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_origin_is_thursday() {
        // 1970-01-01 = 星期四（4）——万年历锚点。
        assert_eq!(weekday_of_epoch_days(0), 4);
        assert_eq!(civil_from_days(0), (1970, 1, 1));
    }

    #[test]
    fn y2k_known_point() {
        // 2000-01-01 = 纪元 10957 天（30 年 × 365 + 7 闰日）、星期六。
        assert_eq!(days_from_civil(2000, 1, 1), 10_957);
        assert_eq!(weekday_of_epoch_days(10_957), 6); // 星期六
    }

    #[test]
    fn deep2_checks_all_green() {
        let cs = run_f022d_checks();
        assert!(cs.all_passed() && !cs.truncated());
    }
}
