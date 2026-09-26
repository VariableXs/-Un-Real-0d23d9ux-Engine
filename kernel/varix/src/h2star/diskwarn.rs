//! F268 磁盘空间预警 · 完整设计（STAR I 主册 H 域）。
//!
//! **判据（主册）**：三阈值触发用例；横幅/推送/劝退三级形制走查；大
//! 文件聚合准确性；每日一次提醒节流。
//!
//! **设计要点（主册）**：U 盘整机宿主的容量是稀缺资源，预警三级：
//! <10% 黄色横幅（资源管理器顶部+设置中心，附「查看大文件」入口）、
//! <5% 红色提示（通知中心推送，可延后但每天提醒一次）、<2% 保护态
//! （非核心写入劝退对话框，说明原因与紧急出口）——三级都给出路；
//! 「查看大文件」页按目录聚合排序（不逐文件枚举防隐私疲劳）。
//!
//! 实装：三级阈值评估器（剩余字节 → 级别+形制+出路文案）；每日一次
//! 提醒节流（天窗复用 [`h2base::days_between`]）；目录聚合器（按一级
//! 子目录聚合排序——不逐文件枚举的纪律落地）。

use crate::checks::CheckSet;
use crate::h2star::h2base::days_between;

use alloc::string::String;
use alloc::vec::Vec;

/// 三级阈值（主册定值：百分比）。
pub const WARN_PCT: u64 = 10;
pub const CRIT_PCT: u64 = 5;
pub const PROTECT_PCT: u64 = 2;

/// 预警级别。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum WarnLevel {
    /// 健康（无预警）。
    Ok,
    /// <10% 黄色横幅。
    Yellow,
    /// <5% 红色推送。
    Red,
    /// <2% 保护态。
    Protect,
}

/// 预警形态（形制+出路文案——三级都给出路）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WarnAction {
    pub level: WarnLevel,
    /// 形制标签（横幅/推送/劝退）。
    pub form: &'static str,
    /// 出路文案（人话，指向可执行动作）。
    pub way_out: &'static str,
}

/// 评估：总容量 + 剩余字节 → 预警形态。
pub fn evaluate(total_bytes: u64, free_bytes: u64) -> WarnAction {
    let free_pct = if total_bytes == 0 {
        0
    } else {
        free_bytes * 100 / total_bytes
    };
    if free_pct < PROTECT_PCT {
        WarnAction {
            level: WarnLevel::Protect,
            form: "劝退对话框",
            way_out: "非核心写入已暂停——请清理或外接存储；核心路径见「查看大文件」",
        }
    } else if free_pct < CRIT_PCT {
        WarnAction {
            level: WarnLevel::Red,
            form: "通知推送",
            way_out: "空间将满——打开「查看大文件」清理；每天提醒一次，可延后",
        }
    } else if free_pct < WARN_PCT {
        WarnAction {
            level: WarnLevel::Yellow,
            form: "顶部横幅",
            way_out: "空间偏低——点击「查看大文件」按目录清理",
        }
    } else {
        WarnAction { level: WarnLevel::Ok, form: "", way_out: "" }
    }
}

/// 每日一次提醒节流：距上次提醒不足一天则不重复。
pub fn should_remind(last_remind_min: u64, now_min: u64) -> bool {
    if last_remind_min == 0 {
        return true; // 从未提醒过。
    }
    days_between(last_remind_min, now_min) >= 1
}

/// 大文件目录聚合：按一级子目录聚合字节数，降序（防隐私疲劳——
/// 不逐文件枚举，聚合到目录层）。
pub fn aggregate_by_dir(files: &[(&str, u64)]) -> Vec<(String, u64)> {
    let mut out: Vec<(String, u64)> = Vec::new();
    for (path, size) in files {
        let dir = dir_top(path);
        match out.iter_mut().find(|(d, _)| d == dir) {
            Some((_, s)) => *s += size,
            None => out.push((String::from(dir), *size)),
        }
    }
    out.sort_by(|a, b| b.1.cmp(&a.1));
    out
}

/// 取一级目录名（`vx:/a/b/c.txt` → `vx:/a`；根文件 → `vx:/`）。
fn dir_top(path: &str) -> &str {
    let bytes = path.as_bytes();
    let mut depth = 0;
    for (i, b) in bytes.iter().enumerate() {
        if *b == b'/' || *b == b'\\' {
            depth += 1;
            if depth == 2 {
                return &path[..i];
            }
        }
    }
    path
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_diskwarn_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-F268");
    // 三阈值触发：11% 健康、9% 黄、4% 红、1% 保护。
    let a = evaluate(100_000_000_000, 11_000_000_000);
    let b = evaluate(100_000_000_000, 9_000_000_000);
    let c = evaluate(100_000_000_000, 4_000_000_000);
    let d = evaluate(100_000_000_000, 1_000_000_000);
    set.add(
        "F268 three thresholds",
        a.level == WarnLevel::Ok
            && b.level == WarnLevel::Yellow
            && c.level == WarnLevel::Red
            && d.level == WarnLevel::Protect,
        "11/9/4/1%",
    );
    // 三级形制与出路。
    set.add(
        "F268 forms+way out",
        b.form == "顶部横幅" && c.form == "通知推送" && d.form == "劝退对话框"
            && !b.way_out.is_empty()
            && !d.way_out.is_empty(),
        "all have exits",
    );
    // 每日一次节流。
    set.add(
        "F268 daily throttle",
        should_remind(0, 100)
            && !should_remind(1_000, 1_000 + 1439)
            && should_remind(1_000, 1_000 + 1440),
        "1/day",
    );
    // 大文件目录聚合：准确性 + 排序 + 不逐文件。
    let files = alloc::vec![
        ("vx:/影像/相机A/1.raw", 40u64),
        ("vx:/影像/相机B/2.raw", 30u64),
        ("vx:/文档/报告.docx", 1u64),
        ("vx:/文档/notes.md", 2u64),
    ];
    let agg = aggregate_by_dir(&files);
    set.add(
        "F268 aggregate",
        agg.len() == 2 && agg[0].0 == "vx:/影像" && agg[0].1 == 70,
        "dir-level only",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f268_levels_and_throttle() {
        let set = run_diskwarn_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F268 自检红 {f}/{p}");
    }

    #[test]
    fn zero_total_never_panics() {
        let a = evaluate(0, 0);
        assert_eq!(a.level, WarnLevel::Protect, "零容量按最险处理——不静默");
    }
}
