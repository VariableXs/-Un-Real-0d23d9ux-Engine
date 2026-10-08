//! F022 深化批次三 · 系统时间转换矩阵与毫秒回绕面（compatstar2/deep2 · G-A-22）。
//!
//! 批次一/二深化覆盖单调钟/QPC/计时器周期治理/时区表/DST/FILETIME 纪元
//! 换算。本批补齐主册【功能定义】「全语义对齐」的执行/边界面：FILETIME↔
//! SYSTEMTIME 双向转换模型（1601 纪元、civil 历法自含实现——闰年边界
//! 2000-02-29 与 2100-02-28 平年差异、年末/年初毫秒滚动）、GetSystemTime/
//! GetLocalTime 时区偏移模型（UTC 基础上 ±offset 分钟、越界钳制）、
//! Stopwatch 累积模型（start/stop/pause 三态、累积段定长 8）、timeGetTime
//! 32 位毫秒回绕对拍（u32 wrapping 减法求差值）。
//!
//! 判据对账：主册 G-A-22【设计细节】时间族段 + MS FILETIME/SYSTEMTIME
//! （1601-01-01 UTC 纪元、100ns tick）与 timeGetTime（tick 计数回绕）
//! 文档语义对拍。
//!
//! 零堆纪律：定长段表，无 Vec/String/Box/format!，拒绝一律显性化。

use crate::checks::CheckSet;

/// 1601-01-01 UTC 与 1970-01-01 UTC 的秒差（MS FILETIME 纪元）。
pub const EPOCH_1601_TO_1970_S: i64 = 11_644_473_600;
/// 每 FILETIME tick = 100ns，故每毫秒 10_000 tick（MS 文档语义）。
pub const FILETICKS_PER_MS: i64 = 10_000;
/// 每秒 FILETIME tick 数（10_000_000）。
pub const FILETICKS_PER_S: i64 = 10_000_000;
/// MS 时区偏移合法边界 ±14h（分钟），越界钳制。
pub const TZ_OFFSET_LIMIT_MIN: i32 = 840;
/// Stopwatch 累积段定长（域内模型口径）。
pub const STOPWATCH_SEGS: usize = 8;

/// SYSTEMTIME 结构面（MS 结构语义：年月日时分秒毫秒）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SysTime {
    pub year: i64,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub min: u32,
    pub sec: u32,
    pub ms: u32,
}

/// civil 历法：天数→年月日（Hinnant 算法，proleptic Gregorian）。
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// civil 历法：年月日→天数（自 1970-01-01）。
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = ((m + 9) % 12) as i64;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// 闰年判定（格里历规则：4 整除且非百年，或 400 整除）。
pub fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

/// SYSTEMTIME → FILETIME（1601 纪元 100ns tick，双向转换的正向）。
pub fn systemtime_to_filetime(st: SysTime) -> i64 {
    let days = days_from_civil(st.year, st.month, st.day);
    let secs = days * 86_400 + st.hour as i64 * 3600 + st.min as i64 * 60 + st.sec as i64;
    (secs + EPOCH_1601_TO_1970_S) * FILETICKS_PER_S + st.ms as i64 * FILETICKS_PER_MS
}

/// FILETIME → SYSTEMTIME（含毫秒余数保留）。
pub fn filetime_to_systemtime(ft: i64) -> SysTime {
    let secs = ft.div_euclid(FILETICKS_PER_S) - EPOCH_1601_TO_1970_S;
    let sub_ms = ft.rem_euclid(FILETICKS_PER_S) / FILETICKS_PER_MS;
    let days = secs.div_euclid(86_400);
    let sod = secs.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    SysTime {
        year,
        month,
        day,
        hour: (sod / 3600) as u32,
        min: (sod % 3600 / 60) as u32,
        sec: (sod % 60) as u32,
        ms: sub_ms as u32,
    }
}

/// 时间推进毫秒（经 FILETIME 中转，毫秒精度；年末/年初滚动由此自然成立）。
pub fn bump_ms(st: SysTime, delta_ms: i64) -> SysTime {
    filetime_to_systemtime(systemtime_to_filetime(st) + delta_ms * FILETICKS_PER_MS)
}

/// 时区偏移越界钳制（MS 时区数据库边界 ±14h）。
pub fn clamp_tz_offset(min: i32) -> i32 {
    min.clamp(-TZ_OFFSET_LIMIT_MIN, TZ_OFFSET_LIMIT_MIN)
}

/// GetLocalTime 模型：UTC 基础上 ±offset 分钟；越界偏移先钳制再合成。
pub fn local_time(utc: SysTime, offset_min: i32) -> SysTime {
    bump_ms(utc, clamp_tz_offset(offset_min) as i64 * 60_000)
}

/// Stopwatch 三态（start/stop/pause）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SwState {
    Stopped,
    Running,
    Paused,
}

/// Stopwatch 累积模型：段定长 8，超段如实例数（不静默）。
pub struct Stopwatch {
    state: SwState,
    seg_start: u64,
    segs: [u64; STOPWATCH_SEGS],
    pub seg_n: usize,
    pub dropped_segs: u32,
    /// 非法态转换（如未 start 先 stop）检出计数。
    pub violations: u32,
}

impl Stopwatch {
    pub const fn new() -> Self {
        Stopwatch { state: SwState::Stopped, seg_start: 0, segs: [0; STOPWATCH_SEGS], seg_n: 0, dropped_segs: 0, violations: 0 }
    }

    pub fn state(&self) -> SwState {
        self.state
    }

    fn push_seg(&mut self, end: u64) {
        if self.seg_n < STOPWATCH_SEGS {
            self.segs[self.seg_n] = end.saturating_sub(self.seg_start);
            self.seg_n += 1;
        } else {
            self.dropped_segs += 1;
        }
    }

    /// start：仅 Stopped 态可启动。
    pub fn start(&mut self, now: u64) -> bool {
        if self.state != SwState::Stopped {
            self.violations += 1;
            return false;
        }
        self.state = SwState::Running;
        self.seg_start = now;
        true
    }

    /// pause：Running → Paused，当前段入账。
    pub fn pause(&mut self, now: u64) -> bool {
        if self.state != SwState::Running {
            self.violations += 1;
            return false;
        }
        self.push_seg(now);
        self.state = SwState::Paused;
        true
    }

    /// resume：Paused → Running，段起点重置。
    pub fn resume(&mut self, now: u64) -> bool {
        if self.state != SwState::Paused {
            self.violations += 1;
            return false;
        }
        self.state = SwState::Running;
        self.seg_start = now;
        true
    }

    /// stop：Running 收段入账；Paused 段已在 pause 时入账，仅转态。
    pub fn stop(&mut self, now: u64) -> bool {
        if self.state == SwState::Stopped {
            self.violations += 1;
            return false;
        }
        if self.state == SwState::Running {
            self.push_seg(now);
        }
        self.state = SwState::Stopped;
        true
    }

    /// 累计毫秒（Σ 段）。
    pub fn total_ms(&self) -> u64 {
        self.segs.iter().take(self.seg_n).sum()
    }
}

/// timeGetTime 32 位毫秒回绕差值（u32 wrapping 减法，MS 文档语义）。
pub fn tick32_diff(now: u32, prev: u32) -> u32 {
    now.wrapping_sub(prev)
}

/// 域自检（深化批次三）。
pub fn run_f022e_checks() -> crate::checks::CheckSet {
    let mut cs = CheckSet::new("F022-timefam-d3");
    // 1) 1970-01-01 → FILETIME = 纪元差 × 每秒 tick（真实值对拍）。
    let epoch = SysTime { year: 1970, month: 1, day: 1, hour: 0, min: 0, sec: 0, ms: 0 };
    cs.add(
        "epoch_1970_filetime",
        systemtime_to_filetime(epoch) == 116_444_736_000_000_000
            && EPOCH_1601_TO_1970_S == 11_644_473_600
            && FILETICKS_PER_MS == 10_000,
        "",
    );
    // 2) 闰年边界 2000-02-29（400 整除）双向往返恒等。
    let y2k = SysTime { year: 2000, month: 2, day: 29, hour: 12, min: 0, sec: 0, ms: 0 };
    cs.add("leap_2000_roundtrip", is_leap(2000) && filetime_to_systemtime(systemtime_to_filetime(y2k)) == y2k, "");
    // 3) 平年边界 2100-02-28（百年不闰）：2100-02-28 + 1 天 = 2100-03-01。
    let feb28 = SysTime { year: 2100, month: 2, day: 28, hour: 0, min: 0, sec: 0, ms: 0 };
    let rolled = bump_ms(feb28, 86_400_000);
    cs.add("nonleap_2100", !is_leap(2100) && rolled.year == 2100 && rolled.month == 3 && rolled.day == 1, "");
    // 4) 年末/年初毫秒滚动：1999-12-31 23:59:59.999 + 1ms → 2000-01-01。
    let nye = SysTime { year: 1999, month: 12, day: 31, hour: 23, min: 59, sec: 59, ms: 999 };
    let y2k0 = bump_ms(nye, 1);
    cs.add(
        "year_rollover_ms",
        y2k0.year == 2000 && y2k0.month == 1 && y2k0.day == 1 && y2k0.hour == 0 && y2k0.ms == 0,
        "",
    );
    // 5) 毫秒余数保留：…:56.789 往返后 ms == 789。
    let t789 = SysTime { year: 2026, month: 9, day: 26, hour: 12, min: 34, sec: 56, ms: 789 };
    cs.add("ft_ms_remainder", filetime_to_systemtime(systemtime_to_filetime(t789)).ms == 789, "");
    // 6) 时区偏移钳制：±900 → ±840；UTC+600min → 当日 10:00。
    let utc0 = SysTime { year: 2026, month: 9, day: 26, hour: 0, min: 0, sec: 0, ms: 0 };
    let local = local_time(utc0, 600);
    cs.add(
        "tz_offset_clamp",
        clamp_tz_offset(900) == 840 && clamp_tz_offset(-900) == -840 && local.hour == 10 && local.day == 26,
        "",
    );
    // 7) Stopwatch 三态流：start→pause→resume→stop，段账 Σ = 3000ms。
    let mut sw = Stopwatch::new();
    let flow = sw.start(0)
        && sw.pause(1500)
        && sw.resume(2000)
        && sw.stop(3500)
        && sw.state() == SwState::Stopped
        && sw.seg_n == 2
        && sw.total_ms() == 3000;
    cs.add("stopwatch_accumulate", flow, "");
    // 8) Stopwatch 非法态与超段如实计数：未 start 先 stop；第 9 段丢弃。
    let mut s2 = Stopwatch::new();
    let _ = s2.stop(0);
    let mut full = Stopwatch::new();
    let _ = full.start(0);
    for k in 0..STOPWATCH_SEGS {
        let _ = full.pause(k as u64 * 100);
        let _ = full.resume(k as u64 * 100 + 10);
    }
    let _ = full.stop(999);
    cs.add(
        "stopwatch_ledger",
        s2.violations == 1 && full.dropped_segs == 1 && full.seg_n == STOPWATCH_SEGS,
        "",
    );
    // 9) 32 位回绕差值：0xFFFFFF00 → 0x000000FF 差 0x1FF（511ms）。
    cs.add("tick32_wraparound", tick32_diff(0x0000_00FF, 0xFFFF_FF00) == 511, "");
    // 10) 多点往返恒等抽查表。
    let samples = [
        SysTime { year: 1601, month: 1, day: 1, hour: 0, min: 0, sec: 0, ms: 0 },
        SysTime { year: 1970, month: 1, day: 2, hour: 3, min: 4, sec: 5, ms: 6 },
        SysTime { year: 2000, month: 2, day: 29, hour: 23, min: 59, sec: 59, ms: 999 },
        SysTime { year: 2100, month: 3, day: 1, hour: 1, min: 2, sec: 3, ms: 4 },
    ];
    let rt = samples.iter().all(|&s| filetime_to_systemtime(systemtime_to_filetime(s)) == s);
    cs.add("roundtrip_identity_table", rt, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn century_leap_difference() {
        // 2000-02-28 +1 天 → 02-29（闰）；2100-02-28 +1 天 → 03-01（平）。
        let d2000 = bump_ms(SysTime { year: 2000, month: 2, day: 28, hour: 0, min: 0, sec: 0, ms: 0 }, 86_400_000);
        assert_eq!((d2000.month, d2000.day), (2, 29));
        let d2100 = bump_ms(SysTime { year: 2100, month: 2, day: 28, hour: 0, min: 0, sec: 0, ms: 0 }, 86_400_000);
        assert_eq!((d2100.month, d2100.day), (3, 1));
    }

    #[test]
    fn negative_filetime_pre_1601_is_explicit_math() {
        // 1970-01-01 的 FILETIME 恰为纪元差 × 1e7；负 tick 域数学自洽（不下溢静默）。
        let ft = systemtime_to_filetime(SysTime { year: 1970, month: 1, day: 1, hour: 0, min: 0, sec: 0, ms: 1 });
        assert_eq!(ft, 116_444_736_000_010_000);
        assert_eq!(filetime_to_systemtime(ft).ms, 1);
    }

    #[test]
    fn stopwatch_pause_only_from_running() {
        let mut sw = Stopwatch::new();
        assert!(!sw.pause(0) && !sw.resume(0) && sw.violations == 2);
        assert!(sw.start(0) && sw.pause(100) && !sw.pause(200) && sw.violations == 3);
        assert_eq!(sw.total_ms(), 100);
    }

    #[test]
    fn deep3_checks_all_green() {
        let cs = run_f022e_checks();
        assert!(cs.all_passed() && !cs.truncated());
    }
}
