//! F182 双时钟 · 批次八（v8）——时间显示偏好账、对时失败退避、
//! 偏差日历热力行。零堆、no_std。

use crate::checks::CheckSet;

/// 显示偏好槽位（12/24 小时制 + 时区偏移 + 秒显开关）。
pub const PREF_SLOTS: usize = 3;
/// 对时失败退避基数（分钟）。
pub const RETRY_BASE_MIN: u32 = 15;
/// 退避封顶（分钟）。
pub const RETRY_CAP_MIN: u32 = 240;
/// 热力行天数。
pub const HEAT_DAYS: usize = 14;

/// 时间显示偏好：12/24 制、时区偏移（分钟 −720..=840）、秒显开关。
/// 偏好变更留痕（用户改了什么要可考）。
#[derive(Clone, Copy)]
pub struct DisplayPrefs {
    hour12: bool,
    tz_offset_min: i32,
    show_seconds: bool,
    pub changes: u32,
}

impl DisplayPrefs {
    pub const fn new() -> DisplayPrefs {
        DisplayPrefs { hour12: false, tz_offset_min: 480, show_seconds: true, changes: 0 }
    }

    /// 设时制：变更才计数（同值重复设不记——账面只记真变化）。
    pub fn set_hour12(&mut self, v: bool) {
        if self.hour12 != v {
            self.hour12 = v;
            self.changes += 1;
        }
    }

    pub fn set_show_seconds(&mut self, v: bool) {
        if self.show_seconds != v {
            self.show_seconds = v;
            self.changes += 1;
        }
    }

    /// 设时区：越界拒（−720..=840——UTC−12 到 UTC+14 的合法域）。
    pub fn set_tz(&mut self, min: i32) -> bool {
        if !(-720..=840).contains(&min) {
            return false;
        }
        if self.tz_offset_min != min {
            self.tz_offset_min = min;
            self.changes += 1;
        }
        true
    }

    pub fn is_hour12(&self) -> bool {
        self.hour12
    }

    pub fn tz(&self) -> i32 {
        self.tz_offset_min
    }

    pub fn seconds_visible(&self) -> bool {
        self.show_seconds
    }

    pub const fn slot_count() -> usize {
        PREF_SLOTS
    }
}

/// 对时失败退避：15 → 30 → 60 → 120 → 240 封顶（倍增，cap 纪律）。
pub fn next_retry_min(failures: u32) -> u32 {
    let doubled = RETRY_BASE_MIN << failures.min(16);
    doubled.min(RETRY_CAP_MIN)
}

/// 退避复位：成功一次即归基数（不记仇——恢复就回到正常节奏）。
pub fn retry_after_success() -> u32 {
    RETRY_BASE_MIN
}

/// 偏差日历热力行：14 天每日最大偏差 → 分级字符行（G/Y/R/空 .）。
/// 返回定宽字节数组与长度。
pub fn heat_row(daily_max_ms: &[Option<u32>; HEAT_DAYS]) -> ([u8; HEAT_DAYS], usize) {
    let mut out = [b'.'; HEAT_DAYS];
    for (i, d) in daily_max_ms.iter().enumerate() {
        out[i] = match d {
            None => b'.',
            Some(ms) if *ms < 50 => b'G',
            Some(ms) if *ms < 250 => b'Y',
            _ => b'R',
        };
    }
    (out, HEAT_DAYS)
}

/// 热力行红日计数（一眼看病的统计面）。
pub fn heat_red_days(row: &[u8; HEAT_DAYS]) -> u32 {
    row.iter().filter(|c| **c == b'R').count() as u32
}

#[inline(never)]
pub fn run_duoclock_b8_checks() -> CheckSet {
    let mut cs = CheckSet::new("F182-b8");

    // 1) 偏好默认值：24 制 + UTC+8 + 秒显（出厂语义直核）。
    let p = DisplayPrefs::new();
    cs.add(
        "prefs_defaults",
        !p.is_hour12() && p.tz() == 480 && p.seconds_visible() && p.changes == 0,
        "",
    );

    // 2) 变更只记真变化：同值重复设不涨账（账面只记用户真实动作）。
    let mut p2 = DisplayPrefs::new();
    p2.set_hour12(true);
    p2.set_hour12(true);
    p2.set_show_seconds(false);
    cs.add("prefs_changes_deduped", p2.changes == 2 && p2.is_hour12() && !p2.seconds_visible(), "");

    // 3) 时区界：+840（UTC+14）收、−720（UTC−12）收、越界拒（域守门）。
    let mut p3 = DisplayPrefs::new();
    let hi = p3.set_tz(840);
    let lo = p3.set_tz(-720);
    let oob = !p3.set_tz(841) && !p3.set_tz(-721);
    cs.add("prefs_tz_bounds", hi && lo && oob && p3.changes == 2, "");

    // 4) 退避序列：1 败 30、2 败 60、4 败 240 恰封顶、8 败仍 240（cap 不破）。
    cs.add(
        "retry_backoff_ladder",
        next_retry_min(0) == 15
            && next_retry_min(1) == 30
            && next_retry_min(2) == 60
            && next_retry_min(4) == 240
            && next_retry_min(8) == 240,
        "",
    );

    // 5) 退避复位：成功即回基数（不记仇语义）。
    cs.add("retry_success_resets", retry_after_success() == RETRY_BASE_MIN, "");

    // 6) 热力行：混合偏差 → G/Y/R 分级逐位（阈值 50/250 恰点）。
    let mut days: [Option<u32>; HEAT_DAYS] = [None; HEAT_DAYS];
    days[0] = Some(30);
    days[1] = Some(50);
    days[2] = Some(249);
    days[3] = Some(250);
    days[4] = Some(1_000);
    let (row, len) = heat_row(&days);
    cs.add(
        "heat_row_grades",
        len == HEAT_DAYS
            && row[0] == b'G'
            && row[1] == b'Y'
            && row[2] == b'Y'
            && row[3] == b'R'
            && row[4] == b'R'
            && row[5] == b'.',
        "",
    );

    // 7) 热力红日计数：恰 2 红（统计面直核）。
    cs.add("heat_red_count", heat_red_days(&row) == 2, "");

    // 8) 热力空行：全 None → 全点、零红（无数据不装病）。
    let empty: [Option<u32>; HEAT_DAYS] = [None; HEAT_DAYS];
    let (erow, _) = heat_row(&empty);
    cs.add("heat_empty_honest", erow.iter().all(|c| *c == b'.') && heat_red_days(&erow) == 0, "");

    // 9) 常量自洽：槽 3、基数 15、封顶 240、热力 14 天。
    cs.add(
        "b8_constants",
        PREF_SLOTS == 3 && RETRY_BASE_MIN == 15 && RETRY_CAP_MIN == 240 && HEAT_DAYS == 14,
        "",
    );

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_ladder_full_walk() {
        // 全阶梯行走：15/30/60/120/240/240（倍增到封顶的完整节奏）。
        let expect = [15, 30, 60, 120, 240, 240, 240];
        for (f, e) in expect.iter().enumerate() {
            assert_eq!(next_retry_min(f as u32), *e, "fail={f}");
        }
    }

    #[test]
    fn prefs_slot_count_matches_consts() {
        // 槽位常量与偏好字段一一对应（时制/时区/秒显）。
        assert_eq!(DisplayPrefs::slot_count(), PREF_SLOTS);
    }

    #[test]
    fn heat_all_red_or_all_green() {
        // 两个极端：全红行与全绿行的统计一致性。
        let mut bad: [Option<u32>; HEAT_DAYS] = [None; HEAT_DAYS];
        for d in bad.iter_mut() {
            *d = Some(999);
        }
        let (r1, _) = heat_row(&bad);
        assert_eq!(heat_red_days(&r1), HEAT_DAYS as u32);
        let mut good: [Option<u32>; HEAT_DAYS] = [None; HEAT_DAYS];
        for d in good.iter_mut() {
            *d = Some(1);
        }
        let (r2, _) = heat_row(&good);
        assert_eq!(heat_red_days(&r2), 0);
    }
}
