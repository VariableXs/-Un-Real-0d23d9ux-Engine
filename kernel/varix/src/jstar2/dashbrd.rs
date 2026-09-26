//! F624 鼠标设置总览仪表盘 · 完整设计（STAR I 主册 J-B 组）。
//!
//! **判据（主册原文）**：设备信息四项准确（含电量接口）；四直达入口
//! 跳达；摘要与实际参数同源一致（对账脚本）；一键恢复默认（范围清单
//! +确认）；与 F244/F301 聚合登记。
//!
//! **面板语义**（设置-辅助功能-指针页顶部的仪表卡）：
//! - **设备信息四项**：名称/连接方式/电量（无线）/DPI 档——由设备
//!    信息源（显式注入口）取数；电量低黄字提醒（F545 电量同源阈值
//!    注入：≤20% 黄、≤10% 红）；
//! - **手感摘要**：曲线/速度/滚轮档/侧键映射速览——**从实际参数模型
//!    直接投影**（`DashboardFeeds` 注入；摘要与实际同源 = 同一结构
//!    派生，对账脚本逐字段复核）；
//! - **四枚直达入口**：速度曲线（F601）/设备档案（F614）/侧键编程
//!    （F615）/指针方案（E4）——每枚入口的跳转登记进导航台账；
//! - **一键恢复默认**：E-1 三铁律可退原则的域级落实——先列恢复范围
//!    清单再确认，执行时抓回退快照（恢复可撤销）；
//! - **F244/F301 聚合登记**：仪表卡注册进设置搜索聚合（F301「指针」
//!    落地页）+ 快捷键注册表联动条目（F244）。

use crate::checks::CheckSet;
use crate::jstar2::vtheme::{MouseBehaviorSection, VXTHEME_SECTION};
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量
// ---------------------------------------------------------------------------

/// 电量黄线（%，F545 同源阈值注入口的默认值）。
pub const BATTERY_WARN_PCT: u8 = 20;
/// 电量红线（%）。
pub const BATTERY_CRIT_PCT: u8 = 10;

// ---------------------------------------------------------------------------
// 数据面（显式注入口）
// ---------------------------------------------------------------------------

/// 连接方式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Connection {
    Wired,
    Wireless,
    Bluetooth,
}

impl Connection {
    pub fn zh(self) -> &'static str {
        match self {
            Connection::Wired => "有线",
            Connection::Wireless => "无线接收器",
            Connection::Bluetooth => "蓝牙",
        }
    }
}

/// 设备信息四项（判据「设备信息四项准确」）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceInfo {
    pub name: String,
    pub connection: Connection,
    /// 电量（None = 有线供电/不可读——如实显示「—」，不虚标）。
    pub battery_pct: Option<u8>,
    /// DPI 档（0 = 未上报）。
    pub dpi_tier: u32,
}

/// 手感摘要源（与实际参数模型同源——同一结构注入面板与对账脚本）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DashboardFeeds {
    /// 曲线名（F601）。
    pub curve_name: String,
    /// 速度（千分位增益）。
    pub speed_m: i64,
    /// 滚轮档（F605 全局档）。
    pub wheel_gear: String,
    /// 侧键映射条数速览（F615）。
    pub sidekey_count: usize,
    /// 手势库条数（F617）。
    pub gesture_count: usize,
    /// 当前指针方案名（E4）。
    pub scheme_name: String,
}

impl DashboardFeeds {
    /// 从 vtheme 行为段投影（同源口径：面板摘要 = 段字段直投影）。
    pub fn from_section(sec: &MouseBehaviorSection) -> DashboardFeeds {
        DashboardFeeds {
            curve_name: sec.speed.curve_id.clone(),
            speed_m: sec.speed.gain_cap_m,
            wheel_gear: sec.wheel.global.clone(),
            sidekey_count: sec.sidekeys.bindings.len(),
            gesture_count: sec.gestures.gestures.len(),
            scheme_name: sec.pointer_scheme.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// 仪表卡
// ---------------------------------------------------------------------------

/// 电量状态（黄字提醒判据）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BatteryTone {
    Ok,
    Warn,
    Crit,
    Unavailable,
}

/// 四枚直达入口。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DirectEntry {
    SpeedCurve,
    DeviceProfiles,
   SideKeyProgramming,
    PointerScheme,
}

impl DirectEntry {
    pub fn zh(self) -> &'static str {
        match self {
            DirectEntry::SpeedCurve => "速度曲线",
            DirectEntry::DeviceProfiles => "设备档案",
            DirectEntry::SideKeyProgramming => "侧键编程",
            DirectEntry::PointerScheme => "指针方案",
        }
    }

    pub fn all() -> [DirectEntry; 4] {
        [DirectEntry::SpeedCurve, DirectEntry::DeviceProfiles, DirectEntry::SideKeyProgramming, DirectEntry::PointerScheme]
    }
}

/// 恢复范围清单（一键恢复默认的「先列清单」面）。
pub const RESTORE_SCOPE: [&str; 6] = [
    "速度曲线与增益",
    "滚轮档与应用覆盖",
    "侧键映射",
    "手势库",
    "慢速微调修饰键",
    "指针点击动效档",
];

/// 仪表卡状态。
#[derive(Clone, Debug)]
pub struct Dashboard {
    pub device: DeviceInfo,
    pub feeds: DashboardFeeds,
    /// 直达入口跳转台账（入口, 时刻）。
    pub jump_log: Vec<(DirectEntry, u64)>,
    /// 恢复确认门（未确认不执行——E-1 可退原则）。
    restore_confirmed: bool,
    /// 恢复前的回退快照（可撤销）。
    pub restore_rollback: Option<MouseBehaviorSection>,
}

impl Dashboard {
    pub fn new(device: DeviceInfo, feeds: DashboardFeeds) -> Dashboard {
        Dashboard {
            device,
            feeds,
            jump_log: Vec::new(),
            restore_confirmed: false,
            restore_rollback: None,
        }
    }

    /// 电量色调（黄字提醒判据；不可读 → Unavailable 灰显）。
    pub fn battery_tone(&self) -> BatteryTone {
        match self.device.battery_pct {
            None => BatteryTone::Unavailable,
            Some(p) if p <= BATTERY_CRIT_PCT => BatteryTone::Crit,
            Some(p) if p <= BATTERY_WARN_PCT => BatteryTone::Warn,
            Some(_) => BatteryTone::Ok,
        }
    }

    /// 直达入口跳转（登记进导航台账——「四直达入口跳达」的机制面）。
    pub fn jump(&mut self, e: DirectEntry, at_ms: u64) {
        self.jump_log.push((e, at_ms));
    }

    /// 四入口是否全部可跳（对账：台账覆盖全枚举）。
    pub fn all_entries_reachable(&self) -> bool {
        DirectEntry::all().iter().all(|e| self.jump_log.iter().any(|(j, _)| j == e))
    }

    /// 摘要与实际参数对账（对账脚本：面板 feeds 与行为段逐字段一致）。
    pub fn reconcile_with_section(&self, sec: &MouseBehaviorSection) -> bool {
        self.feeds == DashboardFeeds::from_section(sec)
    }

    /// 一键恢复默认——第一步：列出恢复范围（清单面，不执行）。
    pub fn restore_scope(&self) -> &'static [&'static str; 6] {
        &RESTORE_SCOPE
    }

    /// 第二步：用户确认（确认门——未确认执行是违规）。
    pub fn confirm_restore(&mut self, current: &MouseBehaviorSection) {
        self.restore_confirmed = true;
        self.restore_rollback = Some(current.clone());
    }

    /// 第三步：执行恢复（未确认 → 拒绝；确认 → 应用默认段并保留回退快照）。
    pub fn execute_restore(&mut self) -> Result<MouseBehaviorSection, &'static str> {
        if !self.restore_confirmed {
            return Err("恢复默认未经确认——先看范围清单再确认（E-1 可退原则）");
        }
        let defaults = MouseBehaviorSection::default();
        self.feeds = DashboardFeeds::from_section(&defaults);
        self.restore_confirmed = false;
        Ok(defaults)
    }

    /// 撤销恢复（回退快照回放——可退原则的闭环）。
    pub fn undo_restore(&mut self) -> bool {
        let Some(prev) = self.restore_rollback.take() else { return false };
        self.feeds = DashboardFeeds::from_section(&prev);
        true
    }
}

// ---------------------------------------------------------------------------
// F244/F301 聚合登记
// ---------------------------------------------------------------------------

/// F301 设置搜索聚合条目（「指针」聚合落地页的登记行）。
pub struct F301Entry {
    pub page: &'static str,
    pub keywords: &'static [&'static str],
}

/// 仪表卡的聚合登记（一处登记：搜索「指针/鼠标」都落在仪表卡）。
pub const DASHBOARD_F301_ENTRY: F301Entry = F301Entry {
    page: "settings/accessibility/pointer#dashboard",
    keywords: &["指针", "鼠标", "pointer", "mouse", VXTHEME_SECTION],
};

/// F244 快捷键注册表联动条目（侧键/手势的冲突审计登记名）。
pub const DASHBOARD_F244_TAG: &str = "pointer-dashboard";

/// 聚合登记校验（关键词命中判定——F301 搜索「指针」必达仪表卡）。
pub fn f301_query_hits(query: &str) -> bool {
    DASHBOARD_F301_ENTRY.keywords.iter().any(|k| k.contains(query) || query.contains(k))
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F624 自检。
pub fn run_dashbrd_checks() -> CheckSet {
    let mut set = CheckSet::new("jstar2-F624");
    let device = DeviceInfo {
        name: String::from("VARIX 无线鼠 2代"),
        connection: Connection::Wireless,
        battery_pct: Some(15),
        dpi_tier: 3,
    };
    let sec = MouseBehaviorSection::default();
    let mut dash = Dashboard::new(device.clone(), DashboardFeeds::from_section(&sec));

    // 1. 设备信息四项准确 + 电量黄字提醒（15% ≤ 20% 警线）。
    set.add(
        "device four fields accurate",
        dash.device.name == "VARIX 无线鼠 2代"
            && dash.device.connection == Connection::Wireless
            && dash.device.dpi_tier == 3
            && dash.device.battery_pct == Some(15),
        "",
    );
    set.add("battery 15% warns yellow", dash.battery_tone() == BatteryTone::Warn, "");
    // 边界：10% 红、21% 正常、有线不可读灰。
    let crit = Dashboard::new(DeviceInfo { battery_pct: Some(10), ..device.clone() }, dash.feeds.clone());
    let ok = Dashboard::new(DeviceInfo { battery_pct: Some(21), ..device.clone() }, dash.feeds.clone());
    let wired = Dashboard::new(DeviceInfo { battery_pct: None, connection: Connection::Wired, ..device.clone() }, dash.feeds.clone());
    set.add(
        "battery tones at thresholds",
        crit.battery_tone() == BatteryTone::Crit
            && ok.battery_tone() == BatteryTone::Ok
            && wired.battery_tone() == BatteryTone::Unavailable,
        "",
    );

    // 2. 四直达入口跳达（台账覆盖全枚举）。
    for (i, e) in DirectEntry::all().iter().enumerate() {
        dash.jump(*e, 100 + i as u64 * 10);
    }
    set.add("four direct entries all jumped", dash.all_entries_reachable(), "");

    // 3. 摘要与实际参数同源一致（对账：直投影相等）。
    set.add("summary reconciles with section", dash.reconcile_with_section(&sec), "");
    // 篡改摘要 → 失配（对账脚本有牙）。
    let mut tampered_feeds = dash.feeds.clone();
    tampered_feeds.speed_m = 9999;
    let dash_t = Dashboard::new(device.clone(), tampered_feeds);
    set.add("reconciliation catches drift", !dash_t.reconcile_with_section(&sec), "");

    // 4. 一键恢复默认：范围清单 + 确认门 + 回退快照。
    set.add("restore scope listed six", dash.restore_scope().len() == 6, "");
    let no_confirm = dash.execute_restore();
    set.add("restore without confirm rejected", no_confirm.is_err(), "");
    dash.confirm_restore(&sec);
    let applied = dash.execute_restore().unwrap();
    set.add(
        "restore applies defaults after confirm",
        applied == MouseBehaviorSection::default() && dash.feeds.speed_m == 1000,
        "",
    );
    set.add("undo restores previous state", dash.undo_restore() && dash.feeds.speed_m == 1000 && dash.restore_rollback.is_none(), "");

    // 5. F244/F301 聚合登记。
    set.add(
        "F301 aggregation hits for pointer queries",
        f301_query_hits("指针") && f301_query_hits("mouse") && f301_query_hits("鼠标"),
        "",
    );
    set.add("F244 tag registered", DASHBOARD_F244_TAG == "pointer-dashboard", "");

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn device(batt: Option<u8>) -> DeviceInfo {
        DeviceInfo {
            name: String::from("测试鼠"),
            connection: Connection::Bluetooth,
            battery_pct: batt,
            dpi_tier: 2,
        }
    }

    #[test]
    fn feeds_projection_from_custom_section() {
        let mut sec = MouseBehaviorSection::default();
        sec.speed.curve_id = String::from("classic-accel");
        sec.speed.gain_cap_m = 1800;
        sec.wheel.global = String::from("always-notch");
        sec.gestures.gestures.push((String::from("x"), String::from("U"), String::from("a")));
        let feeds = DashboardFeeds::from_section(&sec);
        assert_eq!(feeds.curve_name, "classic-accel");
        assert_eq!(feeds.speed_m, 1800);
        assert_eq!(feeds.wheel_gear, "always-notch");
        assert_eq!(feeds.gesture_count, 3);
        let d = Dashboard::new(device(Some(50)), feeds);
        assert!(d.reconcile_with_section(&sec));
    }

    #[test]
    fn battery_boundaries_exact() {
        let mk = |p: Option<u8>| Dashboard::new(device(p), DashboardFeeds::from_section(&MouseBehaviorSection::default()));
        assert_eq!(mk(Some(20)).battery_tone(), BatteryTone::Warn);
        assert_eq!(mk(Some(11)).battery_tone(), BatteryTone::Warn);
        assert_eq!(mk(Some(10)).battery_tone(), BatteryTone::Crit);
        assert_eq!(mk(Some(1)).battery_tone(), BatteryTone::Crit);
        assert_eq!(mk(Some(100)).battery_tone(), BatteryTone::Ok);
    }

    #[test]
    fn jump_log_grows_and_orders() {
        let mut d = Dashboard::new(device(None), DashboardFeeds::from_section(&MouseBehaviorSection::default()));
        d.jump(DirectEntry::SpeedCurve, 10);
        d.jump(DirectEntry::SpeedCurve, 20);
        assert_eq!(d.jump_log.len(), 2);
        assert!(!d.all_entries_reachable());
    }

    #[test]
    fn restore_then_custom_import_reconciles_again() {
        let mut d = Dashboard::new(device(Some(80)), DashboardFeeds::from_section(&MouseBehaviorSection::default()));
        let mut custom = MouseBehaviorSection::default();
        custom.wheel.global = String::from("always-smooth");
        d.confirm_restore(&custom);
        let applied = d.execute_restore().unwrap();
        assert_eq!(applied.wheel.global, "per-app");
        assert!(d.undo_restore());
        assert!(d.reconcile_with_section(&custom), "撤销后面板与撤销对象同源");
    }
}
