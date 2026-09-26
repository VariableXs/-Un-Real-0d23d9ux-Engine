//! F267 存储感知自动清理 · 完整设计（STAR I 主册 H 域）。
//!
//! **判据（主册）**：三策略独立开关用例；30 天边界测试（29/30/31 天）；
//! 事后账完整性；空闲时段执行判据（前台重载时暂停）。
//!
//! **设计要点（主册）**：回收站超 30 天自动清（可关、清理前不动手）、
//! 临时目录超 7 天清、下载目录只提醒不动手（下载的东西动不得是纪律）
//! ——三条策略入设置中心可调；每次自动清理在通知中心留一条事后账
//! （「清了 X 项，释放 X MB」），清理动作本身在空闲时段执行。
//!
//! 实装：三策略独立开关（旋钮）；天窗边界复用 [`h2base::days_between`]
//! （29/30/31 判据）；空闲闸（busy 标志注入——前台重载时不执行）；
//! 事后账条目（项数+释放字节——通知中心文案直读）。

use crate::checks::CheckSet;
use crate::h2star::h2base::days_between;

use alloc::string::String;
use alloc::vec::Vec;

/// 临时目录天数阈值（主册定值）。
pub const TEMP_DAYS: u64 = 7;
/// 回收站天数阈值（主册定值）。
pub const TRASH_DAYS: u64 = 30;

/// 三条策略的独立开关（设置中心可调——判据「三策略独立开关」）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SenseSwitches {
    pub trash: bool,
    pub temp: bool,
    /// 下载目录只提醒不动手——开关只控制提醒本身。
    pub download_notice: bool,
}

impl Default for SenseSwitches {
    fn default() -> Self {
        SenseSwitches { trash: true, temp: true, download_notice: true }
    }
}

/// 候选清理条目。
#[derive(Clone, Debug)]
pub struct SenseItem {
    pub path: String,
    pub kind: SenseKind,
    /// 进入目录的分钟戳。
    pub since_min: u64,
    pub bytes: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SenseKind {
    Trash,
    Temp,
    Download,
}

/// 事后账条目（通知中心文案直读）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CleanupReport {
    pub items: usize,
    pub freed_bytes: u64,
    pub at_min: u64,
}

impl CleanupReport {
    pub fn notice_text(&self) -> String {
        alloc::format!("存储感知：清了 {} 项，释放 {} MB", self.items, self.freed_bytes / 1024 / 1024)
    }
}

/// 存储感知引擎。
pub struct StorageSense {
    pub switches: SenseSwitches,
    /// 空闲闸：true=前台重载中（引擎暂停——判据「空闲时段执行」）。
    pub busy: bool,
}

impl StorageSense {
    pub fn new() -> StorageSense {
        StorageSense { switches: SenseSwitches::default(), busy: false }
    }

    /// 扫描候选（现在时刻 `now_min` 注入）。下载目录永不进清理候选
    /// （只提醒不动手——纪律的结构保证）。
    pub fn scan<'a>(&self, items: &'a [SenseItem], now_min: u64) -> Vec<&'a SenseItem> {
        if self.busy {
            return Vec::new(); // 前台重载：本轮不执行。
        }
        items
            .iter()
            .filter(|it| match it.kind {
                SenseKind::Trash => {
                    self.switches.trash && days_between(it.since_min, now_min) >= TRASH_DAYS
                }
                SenseKind::Temp => {
                    self.switches.temp && days_between(it.since_min, now_min) >= TEMP_DAYS
                }
                SenseKind::Download => false, // 动不得。
            })
            .collect()
    }

    /// 下载目录提醒条目（只提醒——文案层输出，不动文件）。
    pub fn download_reminders<'a>(&self, items: &'a [SenseItem]) -> Vec<&'a SenseItem> {
        if !self.switches.download_notice {
            return Vec::new();
        }
        items.iter().filter(|it| it.kind == SenseKind::Download).collect()
    }

    /// 出事后账（清完调用——项数与字节如实汇总）。
    pub fn report(items: usize, freed: u64, at_min: u64) -> CleanupReport {
        CleanupReport { items, freed_bytes: freed, at_min }
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

fn item(path: &str, kind: SenseKind, days_ago: u64, bytes: u64) -> SenseItem {
    SenseItem {
        path: String::from(path),
        kind,
        since_min: days_ago * 1440,
        bytes,
    }
}

pub fn run_storagesense_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-F267");
    let mut engine = StorageSense::new();
    // 时间线：现在=第 90 天；条目按「已存在 N 天」注入。
    let now = 90 * 1440;
    let mk = |path: &str, kind: SenseKind, age_days: u64, bytes: u64| SenseItem {
        path: String::from(path),
        kind,
        since_min: now - age_days * 1440,
        bytes,
    };
    let pool = alloc::vec![
        mk("trash/a", SenseKind::Trash, 29, 1),
        mk("trash/b", SenseKind::Trash, 30, 2),
        mk("trash/c", SenseKind::Trash, 31, 4),
        mk("temp/x", SenseKind::Temp, 6, 8),
        mk("temp/y", SenseKind::Temp, 7, 16),
        mk("dl/z", SenseKind::Download, 90, 32),
    ];
    // 30 天边界：29 天不动、30/31 天清。
    let hits = engine.scan(&pool, now);
    set.add(
        "F267 29/30/31 boundary",
        !hits.iter().any(|h| h.path == "trash/a")
            && hits.iter().any(|h| h.path == "trash/b")
            && hits.iter().any(|h| h.path == "trash/c"),
        "day edge",
    );
    // 临时目录 7 天线。
    set.add(
        "F267 temp 7d",
        hits.iter().any(|h| h.path == "temp/y") && !hits.iter().any(|h| h.path == "temp/x"),
        "temp edge",
    );
    // 下载目录动不得。
    set.add("F267 download untouched", !hits.iter().any(|h| h.kind == SenseKind::Download), "hands off");
    // 三开关独立：关回收站策略后回收站项消失，临时项不受影响。
    engine.switches.trash = false;
    let hits2 = engine.scan(&pool, now);
    set.add(
        "F267 independent switches",
        !hits2.iter().any(|h| h.kind == SenseKind::Trash)
            && hits2.iter().any(|h| h.path == "temp/y"),
        "solo toggle",
    );
    // 空闲闸：前台重载暂停。
    engine.switches.trash = true;
    engine.busy = true;
    set.add("F267 idle gate", engine.scan(&pool, now).is_empty(), "paused on busy");
    engine.busy = false;
    // 事后账完整性。
    let rep = StorageSense::report(3, 6 * 1024 * 1024, 0);
    set.add(
        "F267 report",
        rep.items == 3 && rep.notice_text().contains("3 项") && rep.notice_text().contains("6 MB"),
        "after-action ledger",
    );
    // 下载提醒条目（只提醒不动手）。
    set.add(
        "F267 dl reminders",
        engine.download_reminders(&pool).len() == 1,
        "notice only",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f267_boundaries_green() {
        let set = run_storagesense_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F267 自检红 {f}/{p}");
    }

    #[test]
    fn download_never_in_scan_even_when_all_on() {
        let e = StorageSense::new();
        let pool = alloc::vec![item("dl/a", SenseKind::Download, 3650, 1)];
        assert!(e.scan(&pool, u64::MAX - 1).is_empty(), "下载目录纪律无例外");
    }
}
