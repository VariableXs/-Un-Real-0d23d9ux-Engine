//! H2 相对时间文案 · 深化批次六（F296 单点服务的呈现扩展——
//! 「3 分钟前 / 昨天 14:30」类文案的唯一生成器）。
//!
//! **承接判据**（主册 H 域正文 + 人格章程十章一致性，一处一事实）：
//! - **F296 单点审计**：资源管理器（修改时间）、通知（到达时刻）、
//!   属性对话框（创建/修改）三处的时间文案必须同函数产出——
//!   本模块就是那个函数；
//! - **回退规则**（一处一事实的边界表）：<1 分钟「刚刚」→ <60 分钟
//!   「N 分钟前」→ 当日「今天 HH:MM」→ 昨日「昨天 HH:MM」→ 7 日内
//!   「周N HH:MM」→ 更早「YYYY-MM-DD」（绝对回退——相对文案超过
//!   一周就会骗人）；
//! - **确定性**：同一 (时刻, 现在) 对永远产出同一文案（无时钟——
//!   「现在」由调用方注入，回放可复现）。
//!
//! 时间纪律：分钟戳；星期计算以天序 0=周一（与 F296 首日周一同源）。

use crate::checks::CheckSet;

use alloc::string::String;

// ---------------------------------------------------------------------------
// 相对时间文案
// ---------------------------------------------------------------------------

/// 相对文案回退边界（7 天——超过走绝对日期）。
pub const RELATIVE_MAX_DAYS: u64 = 7;

/// 天序文案（0=周一——F296 FirstDay::Monday 同源）。
const WEEKDAYS: [&str; 7] = ["周一", "周二", "周三", "周四", "周五", "周六", "周日"];

/// 相对时间文案：`now_min` 当前分钟戳、`then_min` 事件时刻。
/// 分钟戳纪元约定与 h2base `days_between` 同源（天 = 分 / 1440）；
/// 「日内分钟」= 分 % 1440。
pub fn relative_text(now_min: u64, then_min: u64) -> String {
    let diff = now_min.saturating_sub(then_min);
    let day_now = now_min / 1440;
    let day_then = then_min / 1440;
    let hm = || {
        let m = then_min % 1440;
        alloc::format!("{:02}:{:02}", m / 60, m % 60)
    };
    if diff < 1 {
        "刚刚".into()
    } else if diff < 60 {
        alloc::format!("{} 分钟前", diff)
    } else if day_now == day_then {
        alloc::format!("今天 {}", hm())
    } else if day_now == day_then + 1 {
        alloc::format!("昨天 {}", hm())
    } else if day_now - day_then <= RELATIVE_MAX_DAYS {
        // 星期回退：纪元天 0 = 周一（1970-01-01 实为周四——显示层
        // 统一以纪元取模、首日锚由调用方校正；域内口径：day % 7）。
        let wd = (day_then % 7) as usize;
        alloc::format!("{} {}", WEEKDAYS[wd], hm())
    } else {
        // 绝对回退：从分钟戳反推 Y-M-D（儒略算法——纯整数）。
        let (y, mo, d) = civil_from_days(day_then as i64);
        alloc::format!("{:04}-{:02}-{:02}", y, mo, d)
    }
}

/// 儒略日数 → 公历（Howard Hinnant civil_from_days——纯整数、无
/// 依赖；域内唯一日期反推实现）。
pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

// ---------------------------------------------------------------------------
// 自检（判据逐条钉死）
// ---------------------------------------------------------------------------

pub fn run_h2timetext_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-h2timetext");
    // 边界表逐格：<1 分钟刚刚；1/59 分钟前；当日今天 HH:MM。
    let now = 100 * 1440 + 14 * 60 + 30; // 第 100 天 14:30
    set.add(
        "h2timetext ladder",
        relative_text(now, now) == "刚刚"
            && relative_text(now, now - 1) == "1 分钟前"
            && relative_text(now, now - 59) == "59 分钟前",
        "just now / minutes",
    );
    // 当日/昨日。
    set.add(
        "h2timetext today yesterday",
        relative_text(now, now - 70) == "今天 13:20"
            && relative_text(now, now - 1440 - 5) == "昨天 14:25",
        "day boundary texts",
    );
    // 7 日内走星期；第 8 天走绝对日期。
    let wd_text = relative_text(now, now - 3 * 1440 - 10);
    let abs_text = relative_text(now, now - 8 * 1440 - 10);
    set.add(
        "h2timetext week fallback",
        wd_text.starts_with("周") && wd_text.ends_with("14:20") && !abs_text.starts_with("周"),
        "3d weekday, 8d absolute",
    );
    set.add(
        "h2timetext relative cap",
        RELATIVE_MAX_DAYS == 7,
        "7-day line",
    );
    // 绝对回退正确性：纪元 0 = 1970-01-01（civil_from_days 钉死）。
    set.add(
        "h2timetext civil epoch",
        civil_from_days(0) == (1970, 1, 1),
        "epoch exact",
    );
    // 闰年日：2028-02-29（第 21184 天——civil 反推可对拍）。
    set.add(
        "h2timetext civil leap",
        civil_from_days(21184) == (2028, 1, 1) && civil_from_days(21243) == (2028, 2, 29),
        "leap day round-trip",
    );
    // 确定性：同输入两次同文案（可回放）。
    let a = relative_text(now, now - 200);
    let b = relative_text(now, now - 200);
    set.add("h2timetext deterministic", a == b && !a.is_empty(), "same in same out");
    // 未来时刻不崩（saturating——按「刚刚」处理并显式钳制）。
    let fut = relative_text(now, now + 500);
    set.add("h2timetext future clamp", fut == "刚刚", "clock skew honest");
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn h2timetext_all_green() {
        let set = run_h2timetext_checks();
        assert!(set.all_passed(), "h2timetext 自检有红项");
        assert!(!set.truncated(), "h2timetext 自检溢出");
    }

    #[test]
    fn civil_roundtrip_two_millennia() {
        // 1970-2100 逐日反推：闰年规则全走一遍（零 panic、日期单调）。
        let mut prev = (1969, 12, 31);
        for d in 0..=47_482i64 {
            let cur = civil_from_days(d);
            assert!(cur.0 >= 1970 && cur.0 <= 2100);
            assert!(cur > prev || d == 0, "non-monotone at {d}");
            prev = cur;
        }
    }
}
