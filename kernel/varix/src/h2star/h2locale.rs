//! H2 区域格式服务 · 深化批次三（F296 全格式化器——六格式项的
//! 唯一实现，三处同数是结构保证）。
//!
//! **承接判据**（主册 H 域正文，一处一事实）：
//! - **F296 区域显示格式**：格式服务单点审计（自格式化 = 0 处——
//!   本模块就是那个「单点」：日期四样式/时间两样式/数字千分位/
//!   大小单位/首日周/度量制六项全走这里）；大小单位 KB=1024B——
//!   资源管理器/属性/复制对话框三处同数 = 调用同一函数的结构保证；
//! - **默认值清单**：默认档在此定义（简中习惯：YMD 横杠、24 时、
//!   千分位逗号、周一为一周之首）；
//! - **解析回程**：本域样式格式化的日期可被同域解析器读回（格式
//!   →解析 round-trip——显示层不许有「写得出版读不回」的单向路）。
//!
//! 纯函数：无时钟、无全局态；`RegionProfile` 由设置层注入。

use crate::checks::CheckSet;

use alloc::string::String;

// ---------------------------------------------------------------------------
// 区域档案（六格式项）
// ---------------------------------------------------------------------------

/// 日期样式四档（主册 F296 判据「date styles」口径）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DateStyle {
    /// 2026-09-26（默认）。
    YmdDash,
    /// 2026/09/26。
    YmdSlash,
    /// 26.09.2026（欧式）。
    DmyDot,
    /// 09/26/2026（美式）。
    MdySlash,
}

/// 时间样式两档。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimeStyle {
    H24,
    H12,
}

/// 一周之首。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FirstDay {
    Monday,
    Sunday,
}

/// 区域档案：六项格式开关的唯一载体。
#[derive(Clone, Copy, Debug)]
pub struct RegionProfile {
    pub date: DateStyle,
    pub time: TimeStyle,
    pub thousands: char,
    pub decimal: char,
    pub first_day: FirstDay,
    /// 大小单位进制：1024（KB=1024B——判据原值）。
    pub unit_base: u64,
}

/// 默认档案（默认值清单的唯一落点）。
pub const DEFAULT: RegionProfile = RegionProfile {
    date: DateStyle::YmdDash,
    time: TimeStyle::H24,
    thousands: ',',
    decimal: '.',
    first_day: FirstDay::Monday,
    unit_base: 1024,
};

// ---------------------------------------------------------------------------
// 日期
// ---------------------------------------------------------------------------

/// 日期合法性（闰年规则——格式化入口拒 2 月 30 日，不静默格式化）。
pub fn valid_date(y: u32, m: u32, d: u32) -> bool {
    if m == 0 || m > 12 || d == 0 {
        return false;
    }
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let dim = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ => {
            if leap {
                29
            } else {
                28
            }
        }
    };
    d <= dim
}

/// 补零两位。
fn z2(v: u32) -> String {
    if v < 10 {
        alloc::format!("0{v}")
    } else {
        alloc::format!("{v}")
    }
}

/// 日期格式化（四样式）。非法日期返回 None——调用方显式处理，
/// 不许有「格式化出一个不存在的日子」。
pub fn format_date(p: &RegionProfile, y: u32, m: u32, d: u32) -> Option<String> {
    if !valid_date(y, m, d) {
        return None;
    }
    let (mm, dd) = (z2(m), z2(d));
    Some(match p.date {
        DateStyle::YmdDash => alloc::format!("{y}-{mm}-{dd}"),
        DateStyle::YmdSlash => alloc::format!("{y}/{mm}/{dd}"),
        DateStyle::DmyDot => alloc::format!("{dd}.{mm}.{y}"),
        DateStyle::MdySlash => alloc::format!("{mm}/{dd}/{y}"),
    })
}

/// 日期解析（仅识别本域四样式的输出形态——回程完整性以同域为界）。
/// 返回 (y, m, d)；形态不符返回 None。
pub fn parse_date(s: &str) -> Option<(u32, u32, u32)> {
    let parts: Vec<&str> = if s.contains('-') {
        s.split('-').collect()
    } else if s.contains('/') {
        s.split('/').collect()
    } else if s.contains('.') {
        s.split('.').collect()
    } else {
        return None;
    };
    if parts.len() != 3 {
        return None;
    }
    let n = |x: &str| -> Option<u32> {
        if x.len() == 2 || x.len() == 4 {
            x.parse().ok()
        } else {
            None
        }
    };
    if s.contains('-') {
        Some((n(parts[0])?, n(parts[1])?, n(parts[2])?))
    } else if s.contains('/') {
        // 斜杠二义：三段首段 4 位 = YMD，否则 MDY。
        if parts[0].len() == 4 {
            Some((n(parts[0])?, n(parts[1])?, n(parts[2])?))
        } else {
            Some((n(parts[2])?, n(parts[0])?, n(parts[1])?))
        }
    } else {
        Some((n(parts[2])?, n(parts[1])?, n(parts[0])?))
    }
}

// ---------------------------------------------------------------------------
// 时间
// ---------------------------------------------------------------------------

/// 时间格式化：`hms` (时,分,秒)；12 时制带 上午/下午 前缀。
pub fn format_time(p: &RegionProfile, h: u32, m: u32, s: u32) -> Option<String> {
    if h > 23 || m > 59 || s > 59 {
        return None;
    }
    match p.time {
        TimeStyle::H24 => Some(alloc::format!("{}:{}:{}", z2(h), z2(m), z2(s))),
        TimeStyle::H12 => {
            let am = h < 12;
            let h12 = match h % 12 {
                0 => 12,
                x => x,
            };
            Some(alloc::format!(
                "{}{}:{}:{}",
                if am { "上午 " } else { "下午 " },
                z2(h12),
                z2(m),
                z2(s)
            ))
        }
    }
}

// ---------------------------------------------------------------------------
// 数字与大小单位
// ---------------------------------------------------------------------------

/// 千分位分组（整数；负号保留）。
pub fn format_number(p: &RegionProfile, v: i64) -> String {
    let neg = v < 0;
    let digits = if neg { (v as i128 * -1).to_string() } else { (v as i128).to_string() };
    let bytes = digits.as_bytes();
    let mut out = String::new();
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 && (bytes.len() - i) % 3 == 0 {
            out.push(p.thousands);
        }
        out.push(*b as char);
    }
    if neg {
        out.insert(0, '-');
    }
    out
}

/// 大小单位（KB=1024B 判据的唯一实现；四舍五入口径：余数 ≥ 一半
/// 进一；< 单位的最小值保留为「0 B」诚实显示）。
pub fn format_size(p: &RegionProfile, bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let base = p.unit_base;
    let mut v = bytes;
    let mut u = 0usize;
    while u < UNITS.len() - 1 && v >= base {
        v /= base;
        u += 1;
    }
    // 余数四舍五入：bytes 对 (当前单位) 的余数折算进位。
    let mut shown = v;
    if u > 0 {
        let lower = base.pow(u as u32);
        let unit_val = bytes / lower;
        let rem = bytes % lower;
        if rem * 2 >= lower {
            shown = unit_val + 1;
            // 进位可能把 1023 推过界：1024 KB = 1 MB。
            if shown >= base && u < UNITS.len() - 1 {
                shown = 1;
                u += 1;
            }
        }
    }
    alloc::format!("{} {}", format_number(p, shown as i64), UNITS[u])
}

// ---------------------------------------------------------------------------
// 自检（判据逐条钉死）
// ---------------------------------------------------------------------------

pub fn run_h2locale_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-h2locale");
    let p = DEFAULT;
    // 日期四样式 + 补零。
    set.add(
        "h2locale date styles",
        format_date(&p, 2026, 9, 26).as_deref() == Some("2026-09-26")
            && format_date(&RegionProfile { date: DateStyle::YmdSlash, ..p }, 2026, 9, 26)
                .as_deref()
                == Some("2026/09/26")
            && format_date(&RegionProfile { date: DateStyle::DmyDot, ..p }, 2026, 9, 26)
                .as_deref()
                == Some("26.09.2026")
            && format_date(&RegionProfile { date: DateStyle::MdySlash, ..p }, 2026, 9, 26)
                .as_deref()
                == Some("09/26/2026"),
        "four styles",
    );
    // 闰年：2028-02-29 合法、2026-02-29 拒绝、2 月 30 拒绝。
    set.add(
        "h2locale leap gate",
        format_date(&p, 2028, 2, 29).is_some()
            && format_date(&p, 2026, 2, 29).is_none()
            && format_date(&p, 2026, 2, 30).is_none()
            && format_date(&p, 2000, 2, 29).is_some()
            && format_date(&p, 1900, 2, 29).is_none(),
        "leap rules honest",
    );
    // round-trip：四样式输出全部可读回同值。
    let rt = [DateStyle::YmdDash, DateStyle::YmdSlash, DateStyle::DmyDot, DateStyle::MdySlash]
        .iter()
        .all(|st| {
            let prof = RegionProfile { date: *st, ..p };
            let s = format_date(&prof, 2026, 9, 26).unwrap();
            parse_date(&s) == Some((2026, 9, 26))
        });
    set.add("h2locale roundtrip", rt, "format→parse both ways");
    set.add(
        "h2locale parse reject",
        parse_date("2026-9-6").is_none() && parse_date("26-09").is_none(),
        "strict width",
    );
    // 时间：24/12 两档；上下午；边界 0/12/23 点；越界拒绝。
    set.add(
        "h2locale time 24",
        format_time(&p, 9, 5, 3).as_deref() == Some("09:05:03")
            && format_time(&p, 23, 59, 59).as_deref() == Some("23:59:59"),
        "24h zero pad",
    );
    let p12 = RegionProfile { time: TimeStyle::H12, ..p };
    set.add(
        "h2locale time 12",
        format_time(&p12, 0, 5, 3).as_deref() == Some("上午 12:05:03")
            && format_time(&p12, 12, 5, 3).as_deref() == Some("下午 12:05:03")
            && format_time(&p12, 18, 0, 0).as_deref() == Some("下午 06:00:00"),
        "am/pm noon rules",
    );
    set.add("h2locale time reject", format_time(&p, 24, 0, 0).is_none(), "no 25:00");
    // 数字千分位：正负、零。
    set.add(
        "h2locale thousands",
        format_number(&p, 1234567) == "1,234,567"
            && format_number(&p, -9876543) == "-9,876,543"
            && format_number(&p, 42) == "42"
            && format_number(&RegionProfile { thousands: ' ', ..p }, 1234567) == "1 234 567",
        "grouping by 3",
    );
    // 大小单位：KB=1024B；四舍五入；单位进位；三处同函数（结构断言：
    // 同输入两次调用逐字符相等——单点服务的可测形态）。
    set.add(
        "h2locale size 1024",
        format_size(&p, 1023) == "1,023 B"
            && format_size(&p, 1024) == "1 KB"
            && format_size(&p, 1536) == "2 KB"
            && format_size(&p, 1024 * 1024) == "1 MB",
        "KB=1024B",
    );
    set.add(
        "h2locale size carry",
        format_size(&p, 1023 * 1024) == "1,023 KB"
            && format_size(&p, 1024 * 1024 - 1) == "1 MB",
        "round carries unit",
    );
    let a = format_size(&p, 5_368_709_120);
    let b = format_size(&p, 5_368_709_120);
    set.add("h2locale single point", a == b && a == "5 GB", "same fn same number");
    // 默认档案：默认值清单判据（周一为首、1024 基、千分位逗号）。
    set.add(
        "h2locale defaults",
        DEFAULT.date == DateStyle::YmdDash
            && DEFAULT.time == TimeStyle::H24
            && DEFAULT.first_day == FirstDay::Monday
            && DEFAULT.unit_base == 1024,
        "zh-CN defaults",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn h2locale_all_green() {
        let set = run_h2locale_checks();
        assert!(set.all_passed(), "h2locale 自检有红项");
        assert!(!set.truncated(), "h2locale 自检溢出");
    }

    #[test]
    fn every_leap_year_edge() {
        // 世纪闰年三判例：2000 闰、1900 平、2400 闰。
        assert!(valid_date(2000, 2, 29));
        assert!(!valid_date(1900, 2, 29));
        assert!(valid_date(2400, 2, 29));
    }

    #[test]
    fn size_never_shows_zero_for_nonzero() {
        // 非零字节永不显示为「0 B」（诚实下限）。
        for b in [1u64, 2, 511, 1023] {
            assert_ne!(format_size(&DEFAULT, b), "0 B");
        }
    }
}
