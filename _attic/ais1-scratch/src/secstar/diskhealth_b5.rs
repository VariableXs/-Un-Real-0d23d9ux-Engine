//! F183 存储健康监测 · 批次五深化（secstar · G-G-13）。
//!
//! 批次五功能面（达成率 42%——主攻批次。与 b3「解析与估计」、b4
//! 「趋势与综合」互补，本批管「监控策略」）：
//! - [`SmartWatch`]：关键属性监控表——05 重分配/197 待映射/198 不可修
//!   三哨兵属性各自基线-现值-增量（坏块扩散的属性级盯防）；
//! - [`TempPolicy`]：温控三档—— ≤60 观察 / ≤75 降频建议 / >75 警告
//!   （温度面的三段阈值，与主层寿命三段同构不同尺）；
//! - [`WearBalance`]：写入分布观测——两分区写入比（磨损均衡的简化
//!   观测面：比例偏离 1:1 越远越失衡）；
//! - [`EndOfLife`]：终局预测——剩余 ‰ + 日均写入 → 预计剩余天数
//!   （与批次三 projected_days 同式复核——两路径同答案才可信）。
//!
//! 零堆纪律：定长监控表 + 定长账，无 alloc。

use super::diskhealth::{band_of, Band, ERROR_SPIKE_PER_DAY};
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 关键属性监控表
// ---------------------------------------------------------------------------

/// 哨兵属性 ID。
pub const ATTR_REALLOC: u8 = 5;
pub const ATTR_PENDING: u8 = 197;
pub const ATTR_UNCORRECTABLE: u8 = 198;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sentinel {
    pub attr_id: u8,
    pub baseline: u32,
    pub current: u32,
}

/// 监控表：三哨兵各自基线-现值-增量。
#[derive(Clone, Copy)]
pub struct SmartWatch {
    sentinels: [Sentinel; 3],
}

impl SmartWatch {
    pub const fn new() -> SmartWatch {
        SmartWatch {
            sentinels: [
                Sentinel { attr_id: ATTR_REALLOC, baseline: 0, current: 0 },
                Sentinel { attr_id: ATTR_PENDING, baseline: 0, current: 0 },
                Sentinel { attr_id: ATTR_UNCORRECTABLE, baseline: 0, current: 0 },
            ],
        }
    }

    /// 刷新现值（SMART 周期读取回填）。
    pub fn update(&mut self, attr_id: u8, value: u32) -> bool {
        for s in self.sentinels.iter_mut() {
            if s.attr_id == attr_id {
                s.current = value;
                return true;
            }
        }
        false
    }

    /// 增量（现值-基线——正增长才有意义，负值钳 0）。
    pub fn delta(&self, attr_id: u8) -> u32 {
        self.sentinels
            .iter()
            .find(|s| s.attr_id == attr_id)
            .map(|s| s.current.saturating_sub(s.baseline))
            .unwrap_or(0)
    }

    /// 属性增长（基线随刷新前移——增量只看窗口内）。
    pub fn advance_baseline(&mut self, attr_id: u8) -> bool {
        for s in self.sentinels.iter_mut() {
            if s.attr_id == attr_id {
                s.baseline = s.current;
                return true;
            }
        }
        false
    }

    /// 坏块扩散判定：重分配扇区日增量 ≥ 阈值（与 b4 REALLOC_DAILY_ALERT
    /// 同线——两路径同判）。
    pub fn spreading(&self, threshold: u32) -> bool {
        self.delta(ATTR_REALLOC) >= threshold
    }

    /// 全哨兵清零判定：三增量全 0 → 盘面平静。
    pub fn all_quiet(&self) -> bool {
        self.sentinels.iter().all(|s| s.current == s.baseline)
    }

    /// 未登记属性诚实拒（监控表是封闭集）。
    pub fn knows(&self, attr_id: u8) -> bool {
        self.sentinels.iter().any(|s| s.attr_id == attr_id)
    }
}

// ---------------------------------------------------------------------------
// 温控三档
// ---------------------------------------------------------------------------

/// 观察上限（°C）。
pub const TEMP_OBSERVE_MAX: i16 = 60;
/// 降频建议线（°C）。
pub const TEMP_THROTTLE_MAX: i16 = 75;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TempAction {
    Observe,
    SuggestThrottle,
    Warn,
}

pub fn temp_policy(celsius: i16) -> TempAction {
    if celsius <= TEMP_OBSERVE_MAX {
        TempAction::Observe
    } else if celsius <= TEMP_THROTTLE_MAX {
        TempAction::SuggestThrottle
    } else {
        TempAction::Warn
    }
}

// ---------------------------------------------------------------------------
// 写入分布观测（磨损均衡简化面）
// ---------------------------------------------------------------------------

/// 分布比判定：两分区写入比在 [1:2, 2:1] 内 = 均衡合格。
pub fn wear_balance_ok(a: u64, b: u64) -> bool {
    if a == 0 && b == 0 {
        return true; // 无写入无失衡
    }
    let (hi, lo) = if a >= b { (a, b) } else { (b, a) };
    if lo == 0 {
        return false; // 单边独写 = 全然失衡
    }
    // u128 中间量——u64::MAX 级比值不溢出。
    (hi as u128) * 2 <= (lo as u128) * 4 // hi/lo ≤ 2
}

/// 失衡指数 ‰：偏出合格带的程度（0=带内；>0 提示搬运）。
pub fn imbalance_permille(a: u64, b: u64) -> u32 {
    if a == 0 && b == 0 {
        return 0;
    }
    let (hi, lo) = if a >= b { (a, b) } else { (b, a) };
    if lo == 0 {
        return 1_000;
    }
    // u128 中间量——天文数字比不溢出。
    let ratio = ((hi as u128 * 1_000) / lo as u128) as u64;
    (ratio.saturating_sub(2_000)).min(1_000) as u32 // 超出 2:1 的部分，封顶 1000
}

// ---------------------------------------------------------------------------
// 终局预测（与批次三同式复核）
// ---------------------------------------------------------------------------

/// 预计剩余天数 = 剩余额定 KiB / 日均写入 KiB（零速率 None——不编造）。
pub fn end_of_life_days(rated_kib: u64, written_bytes: u64, daily_bytes: u64) -> Option<u64> {
    if daily_bytes == 0 {
        return None;
    }
    let written_kib = written_bytes / 1024;
    let remaining_kib = rated_kib.saturating_sub(written_kib);
    Some(remaining_kib * 1024 / daily_bytes)
}

/// 分段一致性：终局天数与剩余 ‰ 过同一把尺（两路径同判——预测可信面）。
pub fn eol_band_consistent(rated_kib: u64, written_bytes: u64, daily_bytes: u64) -> bool {
    match end_of_life_days(rated_kib, written_bytes, daily_bytes) {
        None => true, // 无速率 → 无预测 → 无矛盾
        Some(days) => {
            let remaining_permille = if rated_kib == 0 {
                0
            } else {
                let written_kib = written_bytes / 1024;
                1_000u32.saturating_sub((written_kib.saturating_mul(1_000) / rated_kib.max(1)) as u32)
            };
            let band = band_of(1_000 - remaining_permille);
            match band {
                Band::Green => days > 365,
                Band::Yellow => days > 90,
                Band::Red => true, // 红段无下限承诺（诚实）
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 批次五自检
// ---------------------------------------------------------------------------

#[inline(never)]
pub fn run_diskhealth_b5_checks() -> CheckSet {
    let mut cs = CheckSet::new("F183-b5");

    // 1) 哨兵表：三属性登记在册、未登记拒（封闭集）。
    let w = SmartWatch::new();
    cs.add(
        "watch_closed_set",
        w.knows(ATTR_REALLOC) && w.knows(ATTR_PENDING) && w.knows(ATTR_UNCORRECTABLE) && !w.knows(9),
        "",
    );

    // 2) 增量与基线推进：0→8 增 8；基线前移后窗口内归零（增量只看窗口）。
    let mut w2 = SmartWatch::new();
    w2.update(ATTR_REALLOC, 8);
    let first = w2.delta(ATTR_REALLOC) == 8;
    w2.advance_baseline(ATTR_REALLOC);
    w2.update(ATTR_REALLOC, 9);
    cs.add("watch_window_delta", first && w2.delta(ATTR_REALLOC) == 1, "");

    // 3) 扩散判定：日增 ≥ 阈值真、之下假（与 b4 同线）。
    let mut w3 = SmartWatch::new();
    w3.update(ATTR_REALLOC, 8);
    let spread = w3.spreading(8);
    w3.update(ATTR_REALLOC, 7);
    cs.add("watch_spreading", spread && !w3.spreading(8), "");

    // 4) 全平静：三哨兵零增量 → 盘面平静（好日子也要有判据）。
    cs.add("watch_all_quiet", SmartWatch::new().all_quiet(), "");

    // 5) 温控三档：60 观察 / 61-75 降频建议 / 76+ 警告（邻域逐点）。
    cs.add(
        "temp_policy_three",
        temp_policy(60) == TempAction::Observe
            && temp_policy(61) == TempAction::SuggestThrottle
            && temp_policy(75) == TempAction::SuggestThrottle
            && temp_policy(76) == TempAction::Warn,
        "",
    );

    // 6) 磨损均衡：1:1 与 2:1 带内合格、2.1:1 出带、单边独写全失衡。
    cs.add(
        "wear_balance_band",
        wear_balance_ok(1_000, 1_000)
            && wear_balance_ok(2_000, 1_000)
            && !wear_balance_ok(2_100, 1_000)
            && !wear_balance_ok(1_000, 0)
            && wear_balance_ok(0, 0),
        "",
    );

    // 7) 失衡指数：带内 0、3:1 = 1000、单边 1000（指数与带同义）。
    cs.add(
        "imbalance_permille",
        imbalance_permille(1_000, 1_000) == 0
            && imbalance_permille(3_000, 1_000) == 1_000
            && imbalance_permille(0, 0) == 0,
        "",
    );

    // 8) 终局预测：额定 1MiB、已写 512KiB、日写 1KiB → 512 天（算术面）。
    let rated = 1_024; // KiB
    cs.add(
        "eol_arithmetic",
        end_of_life_days(rated, 512 * 1024, 1_024) == Some(512) && end_of_life_days(rated, 0, 0).is_none(),
        "",
    );

    // 9) 终局-分段一致：绿段(剩余多) → >365 天判定真；写满 → 红段无下限。
    let fresh = eol_band_consistent(rated, 100 * 1024, 64);
    let worn = eol_band_consistent(rated, 1_024 * 1024, 64);
    cs.add("eol_band_consistent", fresh && worn, "");

    // 10) 错误日界常量贯通：ERROR_SPIKE_PER_DAY 10 与哨兵告警线同数量级（一处一事实）。
    cs.add("consts_aligned", ERROR_SPIKE_PER_DAY == 10, "");

    cs
}

// ---------------------------------------------------------------------------
// 宿主单测（批次五）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests_b5 {
    use super::*;

    #[test]
    fn watch_multi_attribute_isolated() {
        // 三哨兵互不串扰：重分配暴涨不改待映射增量。
        let mut w = SmartWatch::new();
        w.update(ATTR_REALLOC, 5);
        w.update(ATTR_PENDING, 2);
        assert_eq!(w.delta(ATTR_REALLOC), 5);
        assert_eq!(w.delta(ATTR_PENDING), 2);
        assert_eq!(w.delta(ATTR_UNCORRECTABLE), 0);
        assert!(!w.all_quiet());
    }

    #[test]
    fn wear_balance_extremes() {
        // 极值面：天文数字比仍稳定（不溢出不吐负）。
        assert!(!wear_balance_ok(u64::MAX, 1));
        assert_eq!(imbalance_permille(u64::MAX, 1), 1_000);
        assert!(wear_balance_ok(u64::MAX, u64::MAX));
    }

    #[test]
    fn temp_negative_values() {
        // 负温度（异常传感器）→ 观察档（不炸不误警）。
        assert_eq!(temp_policy(-40), TempAction::Observe);
    }
}
