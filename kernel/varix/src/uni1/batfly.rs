//! F423 电池图标浮层 · 完整设计（STAR I 主册 G-I-23）。
//!
//! **判据（主册）**：续航估算准确性（±15% 对实际）；充电预计；开关即时；
//! 数据同源审计（三处 F366/F423/F291 同数）；浮层几何。＋通12。
//!
//! 设计：电池浮层语义核——续航估算（近 1 小时实际功耗外推，注入式：
//! 消耗序列由 F060 电量账本送入；±15% 判据用注入的真值对拍）；充电
//! 预计充满时间；省电开关（F333）即时生效账；三处同源对拍（托盘
//! F366/浮层 F423/耗电排行 F291 同数）；浮层几何复用 F422 口径。

use crate::checks::CheckSet;

use alloc::vec::Vec;

/// 续航估算容差（±15%）。
pub const ESTIMATE_TOLERANCE_PERMILLE: u64 = 150;

/// 电池浮层核。
pub struct BatteryFlyout {
    /// 电量千分比（0-1000）。
    pub charge_permille: u64,
    /// 充电中。
    pub charging: bool,
    /// 省电模式开关（F333）。
    pub saver_on: bool,
    /// 最近 1 小时消耗序列（毫电量/分钟，F060 同源注入）。
    pub drain_series: Vec<u64>,
    /// 估算续航（分钟）——estimate() 的输出缓存。
    pub last_estimate_min: Option<u64>,
}

impl BatteryFlyout {
    pub fn new() -> BatteryFlyout {
        BatteryFlyout {
            charge_permille: 800,
            charging: false,
            saver_on: false,
            drain_series: Vec::new(),
            last_estimate_min: None,
        }
    }

    /// F060 同源注入：近 60 分钟消耗序列。
    pub fn sync_drain(&mut self, series: &[u64]) {
        self.drain_series = series.to_vec();
    }

    /// 续航估算：剩余毫电量 / 平均每分钟消耗。
    /// 无数据/消耗为 0 → None（诚实不估算，不虚构「∞」）。
    pub fn estimate_minutes(&mut self) -> Option<u64> {
        if self.charging || self.drain_series.is_empty() {
            self.last_estimate_min = None;
            return None;
        }
        let sum: u64 = self.drain_series.iter().sum();
        if sum == 0 {
            self.last_estimate_min = None;
            return None;
        }
        let avg = sum / self.drain_series.len() as u64;
        if avg == 0 {
            self.last_estimate_min = None;
            return None;
        }
        let m = self.charge_permille / avg;
        self.last_estimate_min = Some(m);
        Some(m)
    }

    /// ±15% 对拍：估算 vs 真值（真值由验收夹具注入）。
    pub fn estimate_within_tolerance(&self, truth_min: u64) -> bool {
        match self.last_estimate_min {
            Some(e) => {
                let d = if e > truth_min { e - truth_min } else { truth_min - e };
                d * 1_000 <= truth_min * ESTIMATE_TOLERANCE_PERMILLE
            }
            None => false,
        }
    }

    /// 充电预计充满：剩余电量 / 充电速率（每分钟毫电量）。
    pub fn charge_eta_minutes(&self, rate_per_min: u64) -> Option<u64> {
        if !self.charging || rate_per_min == 0 {
            return None;
        }
        Some((1_000 - self.charge_permille) / rate_per_min)
    }

    /// 省电开关（即时生效——切完读数即翻）。
    pub fn toggle_saver(&mut self) -> bool {
        self.saver_on = !self.saver_on;
        self.saver_on
    }

    /// 三处同源对拍：托盘(F366)/浮层(F423)/排行(F291) 三读数一致。
    pub fn three_way_sync(&self, tray: u64, rank: u64) -> bool {
        self.charge_permille == tray && self.charge_permille == rank
    }

    /// 浮层几何（图标上方居中——与 F422 同口径）。
    pub fn geometry(icon: (i32, i32), layer_h: i32, screen_h: i32) -> (i32, i32) {
        let y = icon.1 - layer_h;
        (icon.0, y.max(0).min(screen_h - layer_h))
    }
}

pub fn run_batfly_checks() -> CheckSet {
    let mut set = CheckSet::new("uni1-F423");
    let mut b = BatteryFlyout::new();
    // 续航估算：60 分钟序列均耗 8/分钟 → 800 电量 = 100 分钟。
    b.sync_drain(&[8; 60]);
    b.charge_permille = 800;
    set.add("f423-estimate-100min", b.estimate_minutes() == Some(100), "");
    // ±15% 对拍：真值 100 → 过；真值 80（偏差 25%）→ 拒。
    set.add("f423-estimate-within-15pct", b.estimate_within_tolerance(100), "");
    set.add("f423-estimate-beyond-15pct", !b.estimate_within_tolerance(80), "");
    // 充电中不估算（诚实）。
    b.charging = true;
    set.add("f423-charging-no-estimate", b.estimate_minutes().is_none(), "");
    // 充电预计充满：剩 200，速率 40/分 → 5 分钟。
    set.add("f423-charge-eta", b.charge_eta_minutes(40) == Some(5), "");
    set.add("f423-charge-eta-no-rate", b.charge_eta_minutes(0).is_none(), "");
    b.charging = false;
    set.add("f423-eta-not-charging", b.charge_eta_minutes(40).is_none(), "");
    // 省电开关即时。
    set.add("f423-saver-instant", !b.toggle_saver() == false && b.saver_on, "");
    // 三处同源。
    b.charge_permille = 650;
    set.add(
        "f423-three-way-sync",
        b.three_way_sync(650, 650) && !b.three_way_sync(650, 640),
        "",
    );
    // 浮层几何。
    set.add(
        "f423-geometry",
        BatteryFlyout::geometry((1750, 1040), 180, 1080) == (1750, 860),
        "",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_drain_data_honest() {
        let mut b = BatteryFlyout::new();
        assert!(b.estimate_minutes().is_none(), "无数据不估算");
        b.sync_drain(&[0; 60]);
        assert!(b.estimate_minutes().is_none(), "零消耗不估算");
        assert!(!b.estimate_within_tolerance(100), "无估算无对拍");
    }

    #[test]
    fn tolerance_boundary() {
        let mut b = BatteryFlyout::new();
        b.charge_permille = 600;
        b.sync_drain(&[6; 60]); // 100 分钟
        assert_eq!(b.estimate_minutes(), Some(100));
        assert!(b.estimate_within_tolerance(87), "+15% 边界内（100 vs 87≈+14.9%）");
        assert!(b.estimate_within_tolerance(115), "-15% 边界内");
        assert!(!b.estimate_within_tolerance(86), "超 +15%");
    }
}
