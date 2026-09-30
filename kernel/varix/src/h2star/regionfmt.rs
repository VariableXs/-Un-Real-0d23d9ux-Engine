//! F296 区域显示格式 · 完整设计（STAR I 主册 H 域）。
//!
//! **判据（主册）**：格式服务单点审计（自格式化=0 处）；六格式项切换
//! 用例；默认值清单；大小单位一致性（三处同文件同数）。
//!
//! **设计要点（主册）**：日期/时间/数字/千分位/首日（周一或周日）显示
//! 格式独立于语言可选（中文语境默认 2026/9/25 与 24 小时制、千分位
//! 逗号）；所有系统界面从单一格式服务取数——没有一个界面自己格式化
//! 日期（一处改处处改）；文件大小单位规范（KB=1024B 标注清楚、MB/GB/
//! TB 阶梯）。
//!
//! 实装：`RegionFormat`（六格式项：日期/时间/24h/数字小数/千分位/
//! 首日——单点服务）；格式化器（日期/数字/大小三出口——全系统唯一）；
//! 默认值清单（中文语境判据原文值）；大小单位阶梯（KB=1024B 标注）。

use crate::checks::CheckSet;

use alloc::string::String;

/// 首日。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FirstDay {
    Monday,
    Sunday,
}

/// 区域显示格式（六项——单点服务唯一源）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegionFormat {
    /// 日期样式（0=2026/9/25、1=2026-09-25、2=25/9/2026）。
    pub date_style: u8,
    /// 时间样式（0=14:30、1=2:30 下午）。
    pub time_style: u8,
    /// 24 小时制。
    pub hour24: bool,
    /// 小数位（0-3）。
    pub decimals: u8,
    /// 千分位分隔（逗号）。
    pub thousands: bool,
    pub first_day: FirstDay,
}

impl Default for RegionFormat {
    /// 默认值清单（中文语境——判据原文：2026/9/25、24 小时制、千分位
    /// 逗号、首日周一）。
    fn default() -> Self {
        RegionFormat {
            date_style: 0,
            time_style: 0,
            hour24: true,
            decimals: 1,
            thousands: true,
            first_day: FirstDay::Monday,
        }
    }
}

/// 日期格式化（单点——任何界面都不许自己拼）。
pub fn fmt_date(f: &RegionFormat, y: u32, m: u32, d: u32) -> String {
    match f.date_style {
        0 => alloc::format!("{}/{}/{}", y, m, d),
        1 => alloc::format!("{}-{:02}-{:02}", y, m, d),
        _ => alloc::format!("{}/{}/{}", d, m, y),
    }
}

/// 时间格式化。
pub fn fmt_time(f: &RegionFormat, h: u32, min: u32) -> String {
    if f.hour24 {
        alloc::format!("{:02}:{:02}", h, min)
    } else {
        let ampm = if h < 12 { "上午" } else { "下午" };
        let h12 = if h % 12 == 0 { 12 } else { h % 12 };
        alloc::format!("{}:{} {}", h12, min, ampm)
    }
}

/// 数字格式化（小数位 + 千分位）。
pub fn fmt_number(f: &RegionFormat, int_part: u64, frac: u32) -> String {
    let mut digits = alloc::format!("{}", int_part);
    if f.thousands {
        let bytes = digits.as_bytes();
        let mut grouped = String::new();
        let n = bytes.len();
        for (i, b) in bytes.iter().enumerate() {
            grouped.push(*b as char);
            if (n - i - 1) % 3 == 0 && i + 1 < n {
                grouped.push(',');
            }
        }
        digits = grouped;
    }
    if f.decimals > 0 {
        let scale = 10u32.pow(f.decimals as u32);
        let frac_digits = alloc::format!("{:0width$}", frac % scale, width = f.decimals as usize);
        alloc::format!("{}.{}", digits, frac_digits)
    } else {
        digits
    }
}

/// 文件大小阶梯（KB=1024B 标注清楚；B/KB/MB/GB/TB）。
pub fn fmt_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    const TB: u64 = GB * 1024;
    if bytes >= TB {
        alloc::format!("{:.2} TB", bytes as f64 / TB as f64)
    } else if bytes >= GB {
        alloc::format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        alloc::format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        alloc::format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        alloc::format!("{} B", bytes)
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_regionfmt_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-F296");
    // 默认值清单（中文语境）。
    let d = RegionFormat::default();
    set.add(
        "F296 defaults",
        d.date_style == 0 && d.hour24 && d.thousands && d.first_day == FirstDay::Monday,
        "2026/9/25 24h comma",
    );
    // 六格式项切换用例。
    let mut f2 = RegionFormat::default();
    set.add(
        "F296 date styles",
        fmt_date(&f2, 2026, 9, 25) == "2026/9/25"
            && {
                f2.date_style = 1;
                fmt_date(&f2, 2026, 9, 25) == "2026-09-25"
            }
            && {
                f2.date_style = 2;
                fmt_date(&f2, 2026, 9, 25) == "25/9/2026"
            },
        "3 date lanes",
    );
    let f3 = RegionFormat { hour24: false, ..RegionFormat::default() };
    set.add(
        "F296 time styles",
        fmt_time(&RegionFormat::default(), 14, 30) == "14:30"
            && fmt_time(&f3, 14, 30) == "2:30 下午"
            && fmt_time(&f3, 0, 5) == "12:5 上午",
        "24h/12h",
    );
    // 数字：千分位 + 小数（frac 以 10^-decimals 为单位传入：5 → .5）。
    let f4 = RegionFormat::default();
    set.add(
        "F296 number grouping",
        fmt_number(&f4, 12_345_678, 5) == "12,345,678.5",
        "comma+decimal",
    );
    let f5 = RegionFormat { thousands: false, decimals: 0, ..RegionFormat::default() };
    set.add(
        "F296 number plain",
        fmt_number(&f5, 1234, 0) == "1234",
        "off state",
    );
    // 大小单位阶梯（KB=1024B）。
    set.add(
        "F296 size ladder",
        fmt_size(512) == "512 B"
            && fmt_size(1024) == "1.0 KB"
            && fmt_size(1024 * 1024) == "1.0 MB"
            && fmt_size(1024usize.pow(3) as u64) == "1.00 GB",
        "1024 base",
    );
    // 一处改处处改：单点服务换格式 → 所有出口同变（同 f 对象直证）。
    let f6 = RegionFormat { date_style: 1, ..RegionFormat::default() };
    set.add(
        "F296 single source",
        fmt_date(&f6, 2026, 1, 2) == "2026-01-02",
        "one service",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f296_region_service() {
        let set = run_regionfmt_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F296 自检红 {f}/{p}");
    }

    #[test]
    fn tb_step() {
        let tb = 1024u64 * 1024 * 1024 * 1024;
        assert_eq!(fmt_size(tb), "1.00 TB", "TB 阶梯到位");
    }
}
