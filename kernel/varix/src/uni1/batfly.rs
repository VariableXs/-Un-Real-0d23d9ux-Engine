//! F423 电池图标浮层 · 完整设计（STAR I 主册 G-I-23）。
//!
//! **判据（主册）**：续航估算准确性（±15% 对实际）；充电预计；开关即时；
//! 数据同源审计（三处 F366/F423/F291 同数）；浮层几何。＋通12。
//!
//! 设计：电池浮层语义核——续航估算（近 1 小时实际功耗外推，注入式：
//! 消耗序列由 F060 电量账本送入；±15% 判据用注入的真值对拍）；充电
//! 预计充满时间；省电开关（F333）即时生效账；三处同源对拍（托盘
//! F366/浮层 F423/耗电排行 F291 同数）；浮层几何复用 F422 口径。
//!
//! v5 纵深：电量电平 tick 账（充放电边沿记账）；低电量一次性提醒
//! （回充重武装——不重复骚扰）；省电模式实效果对拍（同序列同电量
//! 下估算必须变长——开关不是摆设）。
//!
//! v8 纵深：电池健康度账（满充容量折算——设计容量 vs 实际满充，
//! 健康度千分比 + 档位人话）；充电上限守护模式（到限值断充记账，
//! 回落带内不抖动）；放电速率峰值账（峰值时刻显性化——突发功耗
//! 现形）。

use crate::checks::CheckSet;

use alloc::vec::Vec;

/// 续航估算容差（±15%）。
pub const ESTIMATE_TOLERANCE_PERMILLE: u64 = 150;

/// 低电量提醒阈值（千分比）。
pub const LOW_BATTERY_PERMILLE: u64 = 150;

/// 提醒重新武装阈值（回充到此以上，再跌破才提醒——不重复骚扰）。
pub const REARM_PERMILLE: u64 = 300;

/// 省电模式功耗折减系数（千分比）——F333 同源。
pub const SAVER_FACTOR_PERMILLE: u64 = 800;

/// 充电上限守护默认限值（千分比）。
pub const CHARGE_LIMIT_PERMILLE: u64 = 800;

/// 上限守护回差（千分比）——低于限值-50 才恢复充电（防边界抖动）。
pub const CHARGE_LIMIT_HYST_PERMILLE: u64 = 50;

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
    /// 低电量提醒已挂出（一次性；回充重武装）。
    pub alert_fired: bool,
    /// 充放电边沿账。
    pub charge_edges: u64,
    /// 设计容量（mAh）——健康度分母。
    pub design_cap_mah: u32,
    /// 实际满充容量（mAh）——由固件/EC 上报。
    pub full_charge_mah: u32,
    /// 上限守护开启位。
    pub limit_on: bool,
    /// 守护生效账：到限断充次数。
    pub limit_hits: u64,
    /// 守护当前是否处于「断充保持」态。
    pub limit_holding: bool,
    /// 放电速率峰值（毫电量/分钟）+ 出现时的电量位。
    pub peak_drain: u64,
    pub peak_drain_at_charge: u64,
}

impl BatteryFlyout {
    pub fn new() -> BatteryFlyout {
        BatteryFlyout {
            charge_permille: 800,
            charging: false,
            saver_on: false,
            drain_series: Vec::new(),
            last_estimate_min: None,
            alert_fired: false,
            charge_edges: 0,
            design_cap_mah: 0,
            full_charge_mah: 0,
            limit_on: false,
            limit_hits: 0,
            limit_holding: false,
            peak_drain: 0,
            peak_drain_at_charge: 0,
        }
    }

    /// F060 同源注入：近 60 分钟消耗序列。
    pub fn sync_drain(&mut self, series: &[u64]) {
        self.drain_series = series.to_vec();
    }

    /// 电量电平 tick：同步电量与充放态；边沿记账；低电量一次性提醒
    /// （返回本次 tick 是否触发提醒）。
    pub fn tick_level(&mut self, permille: u64, charging: bool) -> bool {
        if charging != self.charging {
            self.charge_edges += 1;
        }
        self.charging = charging;
        self.charge_permille = permille;
        let mut fired = false;
        if !charging {
            if permille <= LOW_BATTERY_PERMILLE && !self.alert_fired {
                self.alert_fired = true;
                fired = true;
            } else if permille >= REARM_PERMILLE {
                self.alert_fired = false; // 回充重武装
            }
        }
        fired
    }

    /// 续航估算：剩余毫电量 / 平均每分钟消耗（省电开启按折减系数记账）。
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
        let eff = if self.saver_on {
            (avg * SAVER_FACTOR_PERMILLE / 1_000).max(1)
        } else {
            avg
        };
        let m = self.charge_permille / eff;
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

    /// 电池健康度：实际满充 / 设计容量（千分比）；设计容量为 0 →
    /// None（无数据不虚报健康度）。档位人话：≥800 良好 / ≥600 尚可
    /// / <600 建议更换。
    pub fn health_permille(&self) -> Option<u64> {
        if self.design_cap_mah == 0 {
            return None;
        }
        Some((self.full_charge_mah as u64 * 1_000 / self.design_cap_mah as u64).min(1_000))
    }

    pub fn health_verdict(&self) -> Option<&'static str> {
        match self.health_permille() {
            None => None,
            Some(h) if h >= 800 => Some("良好"),
            Some(h) if h >= 600 => Some("尚可"),
            Some(_) => Some("建议更换"),
        }
    }

    /// 上限守护：开启时充到限值断充（limit_holding=true）；回落到
    /// 限值-50 以下才恢复（防边界抖动）。关闭时全通过。
    /// 返回本次 tick 是否因守护改变了充电态。
    pub fn tick_charge_limit(&mut self, permille: u64, plugged: bool) -> bool {
        if !self.limit_on || !plugged {
            self.limit_holding = false;
            return false;
        }
        if !self.limit_holding && permille >= CHARGE_LIMIT_PERMILLE {
            self.limit_holding = true;
            self.limit_hits += 1;
            return true;
        }
        if self.limit_holding && permille < CHARGE_LIMIT_PERMILLE - CHARGE_LIMIT_HYST_PERMILLE {
            self.limit_holding = false;
            return true;
        }
        false
    }

    /// 放电速率峰值账：每次 tick 记录当前速率，只升不降；峰值出现
    /// 时的电量位一并显性化（突发功耗现形）。
    pub fn observe_drain(&mut self, rate_per_min: u64, charge_permille: u64) -> bool {
        if self.charging {
            return false; // 充电期不记放电峰值
        }
        if rate_per_min > self.peak_drain {
            self.peak_drain = rate_per_min;
            self.peak_drain_at_charge = charge_permille;
            return true;
        }
        false
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
    // v5：低电量一次性提醒——跌破阈值挂一次，持续走低不重复骚扰。
    let mut a = BatteryFlyout::new();
    a.charge_permille = 200;
    set.add("f423-low-alert-fires", a.tick_level(120, false), "");
    set.add("f423-low-alert-once", !a.tick_level(100, false) && !a.tick_level(90, false), "");
    // v5：回充到重武装线 → 再跌破才再次提醒。
    let _ = a.tick_level(500, false);
    set.add("f423-alert-rearmed", !a.alert_fired, "");
    set.add("f423-low-alert-refires", a.tick_level(140, false), "");
    // v5：充放电边沿记账。
    let mut e = BatteryFlyout::new();
    let _ = e.tick_level(800, false);
    let _ = e.tick_level(810, true);
    let _ = e.tick_level(820, true);
    let _ = e.tick_level(700, false);
    set.add("f423-charge-edges", e.charge_edges == 2 && e.charge_permille == 700, "");
    // v5：省电实效果对拍——同序列同电量，saver 下估算必须变长。
    let mut s1 = BatteryFlyout::new();
    s1.sync_drain(&[10; 60]);
    s1.charge_permille = 600;
    let plain = s1.estimate_minutes().unwrap_or(0);
    let mut s2 = BatteryFlyout::new();
    s2.sync_drain(&[10; 60]);
    s2.charge_permille = 600;
    let _ = s2.toggle_saver();
    let saved = s2.estimate_minutes().unwrap_or(0);
    set.add("f423-saver-extends", plain == 60 && saved == 75, "");
    // v8：电池健康度账——满充/设计折算 + 三档人话 + 无数据诚实。
    let mut h = BatteryFlyout::new();
    set.add("f423-health-no-data", h.health_permille().is_none() && h.health_verdict().is_none(), "");
    h.design_cap_mah = 5_000;
    h.full_charge_mah = 4_200;
    set.add("f423-health-calc", h.health_permille() == Some(840) && h.health_verdict() == Some("良好"), "");
    h.full_charge_mah = 3_200;
    set.add("f423-health-mid", h.health_permille() == Some(640) && h.health_verdict() == Some("尚可"), "");
    h.full_charge_mah = 2_400;
    set.add("f423-health-low", h.health_permille() == Some(480) && h.health_verdict() == Some("建议更换"), "");
    // v8：充电上限守护——到限断充、回落带恢复、关闭全通过。
    let mut g = BatteryFlyout::new();
    g.limit_on = true;
    set.add("f423-limit-hit", g.tick_charge_limit(800, true) && g.limit_holding && g.limit_hits == 1, "");
    set.add("f423-limit-hold-plateau", !g.tick_charge_limit(805, true) && g.limit_holding, "");
    set.add("f423-limit-resume", g.tick_charge_limit(740, true) && !g.limit_holding, "");
    set.add("f423-limit-hyst-resume", !g.tick_charge_limit(780, true), "回落带内不恢复");
    let _ = g.tick_charge_limit(600, true);
    set.add("f423-limit-off-passthrough", { g.limit_on = false; !g.tick_charge_limit(999, true) && !g.limit_holding }, "");
    // v8：放电速率峰值账——只升不降 + 峰值电量位显性化 + 充电期不记。
    let mut p = BatteryFlyout::new();
    p.charging = false;
    set.add("f423-peak-first", p.observe_drain(12, 700) && p.peak_drain == 12 && p.peak_drain_at_charge == 700, "");
    set.add("f423-peak-ratchet", !p.observe_drain(8, 650) && p.peak_drain == 12, "");
    set.add("f423-peak-new-high", p.observe_drain(30, 400) && p.peak_drain_at_charge == 400, "");
    p.charging = true;
    set.add("f423-peak-charging-skip", !p.observe_drain(99, 900), "");
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

    #[test]
    fn charging_does_not_alert() {
        let mut b = BatteryFlyout::new();
        b.charge_permille = 100;
        assert!(!b.tick_level(100, true), "充电中不提醒（在回血）");
        assert!(b.tick_level(100, false), "拔电跌破 → 提醒");
    }

    #[test]
    fn health_caps_at_thousand() {
        // 固件误报超设计容量 → 健康度封顶 1000，不虚超 100%。
        let mut b = BatteryFlyout::new();
        b.design_cap_mah = 4_000;
        b.full_charge_mah = 9_999;
        assert_eq!(b.health_permille(), Some(1_000));
    }

    #[test]
    fn limit_resume_boundary_exact() {
        // 回落边界：限值-50=750 恰达线不恢复（< 严格小于）。
        let mut b = BatteryFlyout::new();
        b.limit_on = true;
        let _ = b.tick_charge_limit(800, true);
        assert!(!b.tick_charge_limit(750, true), "恰达 750 不恢复");
        assert!(b.tick_charge_limit(749, true), "低于 750 恢复");
    }

    #[test]
    fn peak_without_drain_stays_zero() {
        let mut b = BatteryFlyout::new();
        b.charging = false;
        let _ = b.observe_drain(0, 500);
        assert_eq!(b.peak_drain, 0);
    }
}
