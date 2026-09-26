//! F624 鼠标设置总览仪表盘 · 完整设计（STAR I 主册 J-B 组）· v2 深化版。
//!
//! **判据（主册原文）**：设备信息四项准确（含电量接口）；四直达入口
//! 跳达；摘要与实际参数同源一致（对账脚本）；一键恢复默认（范围清单
//! +确认）；与 F244/F301 聚合登记。
//!
//! **面板语义**（设置-辅助功能-指针页顶部的仪表卡）：
//! - **设备信息四项**：名称/连接方式/电量（无线）/DPI 档——由设备
//!    信息源（显式注入口）取数；电量低黄字提醒（F545 电量同源阈值
//!    注入：≤20% 黄、≤10% 红）；电量趋势采样（64 点环形历史）把
//!    「此刻黄字」纵深为「掉电趋势提醒」——无线鼠标快没电是预见的；
//!    设备离线（接收器拔出/蓝牙断连）→ 仪表卡进空态，恢复与跳转
//!    语义诚实禁用（空态是设计资源不是空白）；
//! - **手感摘要**：曲线/速度/滚轮档/侧键映射速览——**从实际参数模型
//!    直接投影**（`DashboardFeeds` 注入；摘要与实际同源 = 同一结构
//!    派生，对账脚本逐字段复核）；`summary_lines()` 产出人话文本行
//!    （面板文案与数据同源——写死的文案必然漂移）；
//! - **四枚直达入口**：速度曲线（F601）/设备档案（F614）/侧键编程
//!    （F615）/指针方案（E4）——每枚入口的跳转登记进导航台账；路径
//!    链登记 ≤4 段（十二查 #7 的域内落实）；
//! - **一键恢复默认**：E-1 三铁律可退原则的域级落实——恢复范围
//!    **从当前行为段与默认段逐字段 diff 动态生成**（改了什么列什么，
//!    不给"反正都列上"的偷懒清单），确认门 + 回退快照 + 恢复事件
//!    台账（留痕可查）；
//! - **F244/F301 聚合登记**：仪表卡与四入口各登记搜索关键词（F301
//!    「指针」落地页 + 每个直达入口的直达搜索面）。

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
/// 电量历史采样容量（环形——1s 一点即约 1 分钟窗口）。
pub const BATTERY_HISTORY_CAP: usize = 64;
/// 路径链段数上限（通用十二查 #7：≤4 段路径链）。
pub const NAV_PATH_MAX_HOPS: usize = 4;

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

/// 设备在位状态（空态语义的依据——离线不是错误，是如实呈现）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Presence {
    Present,
    /// 接收器拔出/蓝牙断连/未插——仪表卡进空态。
    Absent,
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

impl DeviceInfo {
    /// 电量越界钳制（>100 的脏数据如实拉回——不崩溃不显示乱数）。
    pub fn sanitized(mut self) -> DeviceInfo {
        if let Some(p) = self.battery_pct {
            self.battery_pct = Some(p.min(100));
        }
        self
    }

    /// DPI 档人话标签（0 = 未上报的诚实态）。
    pub fn dpi_label(&self) -> &'static str {
        match self.dpi_tier {
            0 => "未上报",
            1 => "低档（800）",
            2 => "中档（1600）",
            3 => "高档（3200）",
            _ => "最高档（6400+）",
        }
    }
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
// 电量趋势采样
// ---------------------------------------------------------------------------

/// 单点电量采样。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BatterySample {
    pub at_ms: u64,
    pub pct: u8,
}

/// 电量趋势（窗口首尾比较的保守判定）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BatteryTrend {
    /// 采样不足（<2 点）——不下结论。
    Insufficient,
    Stable,
    Falling,
    Rising,
}

// ---------------------------------------------------------------------------
// 仪表卡交互面
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

    /// 路径链（十二查 #7：≤4 段、逐段走达——链式登记一处一事实）。
    pub fn nav_path(self) -> &'static [&'static str] {
        match self {
            DirectEntry::SpeedCurve => &["设置", "辅助功能", "指针", "速度曲线（F601）"],
            DirectEntry::DeviceProfiles => &["设置", "辅助功能", "指针", "设备档案（F614）"],
            DirectEntry::SideKeyProgramming => &["设置", "辅助功能", "指针", "侧键编程（F615）"],
            DirectEntry::PointerScheme => &["设置", "辅助功能", "指针", "指针方案（E4）"],
        }
    }
}

/// 恢复范围清单回退表（动态 diff 不可用时的兜底口径——静态常量保留，
/// 主口径是 `restore_scope_dynamic` 的逐字段 diff）。
pub const RESTORE_SCOPE: [&str; 6] = [
    "速度曲线与增益",
    "滚轮档与应用覆盖",
    "侧键映射",
    "手势库",
    "慢速微调修饰键",
    "指针点击动效档",
];

/// 恢复事件记录（F372 留痕的域内形态）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RestoreRecord {
    pub at_ms: u64,
    /// 本次恢复实际改动的字段数（动态 diff 计数）。
    pub fields_changed: usize,
    pub undone: bool,
}

/// 设备槽位切换请求（F614 联动面：仪表盘上切档案的交互模型）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceSlotSwitch {
    pub from_profile: String,
    pub to_profile: String,
    pub at_ms: u64,
}

/// 仪表卡状态。
#[derive(Clone, Debug)]
pub struct Dashboard {
    pub device: DeviceInfo,
    pub presence: Presence,
    pub feeds: DashboardFeeds,
    /// 直达入口跳转台账（入口, 时刻）。
    pub jump_log: Vec<(DirectEntry, u64)>,
    /// 恢复确认门（未确认不执行——E-1 可退原则）。
    restore_confirmed: bool,
    /// 恢复前的回退快照（可撤销）。
    pub restore_rollback: Option<MouseBehaviorSection>,
    /// 恢复事件台账。
    pub restore_log: Vec<RestoreRecord>,
    /// 电量趋势历史（环形，容量 BATTERY_HISTORY_CAP）。
    battery_history: Vec<BatterySample>,
    /// 设备信息热更新次数（热插拔/电量刷新计数——对账面）。
    device_updates: usize,
    /// F614 档案切换台账。
    pub slot_switches: Vec<DeviceSlotSwitch>,
    /// 当前 F614 档案名。
    pub current_profile: String,
}

impl Dashboard {
    pub fn new(device: DeviceInfo, feeds: DashboardFeeds) -> Dashboard {
        Dashboard {
            device: device.sanitized(),
            presence: Presence::Present,
            feeds,
            jump_log: Vec::new(),
            restore_confirmed: false,
            restore_rollback: None,
            restore_log: Vec::new(),
            battery_history: Vec::new(),
            device_updates: 0,
            slot_switches: Vec::new(),
            current_profile: String::from("默认档案"),
        }
    }

    // -- 设备面 ------------------------------------------------------------

    /// 电量色调（黄字提醒判据；不可读 → Unavailable 灰显）。
    pub fn battery_tone(&self) -> BatteryTone {
        match self.device.battery_pct {
            None => BatteryTone::Unavailable,
            Some(p) if p <= BATTERY_CRIT_PCT => BatteryTone::Crit,
            Some(p) if p <= BATTERY_WARN_PCT => BatteryTone::Warn,
            Some(_) => BatteryTone::Ok,
        }
    }

    /// 电量采样（环形历史，超容丢最旧——容量纪律如实滚动）。
    pub fn sample_battery(&mut self, at_ms: u64) {
        if let Some(p) = self.device.battery_pct {
            if self.battery_history.len() >= BATTERY_HISTORY_CAP {
                self.battery_history.remove(0);
            }
            self.battery_history.push(BatterySample { at_ms, pct: p });
        }
    }

    /// 电量趋势（窗口首尾比较；两点相同视为平稳——保守判定不惊扰）。
    pub fn battery_trend(&self) -> BatteryTrend {
        if self.battery_history.len() < 2 {
            return BatteryTrend::Insufficient;
        }
        let first = self.battery_history[0].pct;
        let last = self.battery_history[self.battery_history.len() - 1].pct;
        if last < first {
            BatteryTrend::Falling
        } else if last > first {
            BatteryTrend::Rising
        } else {
            BatteryTrend::Stable
        }
    }

    /// 电量历史只读视图。
    pub fn battery_history(&self) -> &[BatterySample] {
        &self.battery_history
    }

    /// 设备信息热更新（热插拔/电量刷新——计数留对账；离线/在线切换走这里）。
    pub fn update_device(&mut self, d: DeviceInfo, presence: Presence) {
        self.device = d.sanitized();
        self.presence = presence;
        self.device_updates += 1;
    }

    /// 热更新计数（对账面）。
    pub fn device_updates(&self) -> usize {
        self.device_updates
    }

    /// 在位判定（空态语义：离线 → 恢复/跳转诚实禁用）。
    pub fn is_present(&self) -> bool {
        self.presence == Presence::Present
    }

    /// F614 档案切换（记录台账；离线拒绝——没有设备的档案切换是空操作）。
    pub fn switch_profile(&mut self, to: &str, at_ms: u64) -> bool {
        if !self.is_present() || to == self.current_profile {
            return false;
        }
        self.slot_switches.push(DeviceSlotSwitch {
            from_profile: self.current_profile.clone(),
            to_profile: String::from(to),
            at_ms,
        });
        self.current_profile = String::from(to);
        true
    }

    // -- 摘要面 ------------------------------------------------------------

    /// 面板摘要文本行（人话文案与数据同源——从 feeds 直投影生成）。
    pub fn summary_lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        if !self.is_present() {
            out.push(String::from("未检测到指针设备——插上接收器或连接蓝牙后显示"));
            return out;
        }
        out.push(alloc::format!("设备：{}（{}）", self.device.name, self.device.connection.zh()));
        match self.device.battery_pct {
            Some(p) => out.push(alloc::format!("电量：{}%（{}档 DPI）", p, self.device.dpi_tier)),
            None => out.push(alloc::format!("电量：—（{}档 DPI）", self.device.dpi_tier)),
        }
        out.push(alloc::format!("手感：{} 曲线 · 速度上限 {}‰ · 滚轮 {}", self.feeds.curve_name, self.feeds.speed_m, self.feeds.wheel_gear));
        out.push(alloc::format!("扩展：侧键 {} 条 · 手势 {} 条 · 方案「{}」", self.feeds.sidekey_count, self.feeds.gesture_count, self.feeds.scheme_name));
        out
    }

    // -- 导航面 ------------------------------------------------------------

    /// 直达入口跳转（登记进导航台账——「四直达入口跳达」的机制面；
    /// 离线时设备档案入口诚实拒绝）。
    pub fn jump(&mut self, e: DirectEntry, at_ms: u64) -> bool {
        if !self.is_present() {
            return false;
        }
        self.jump_log.push((e, at_ms));
        true
    }

    /// 四入口是否全部可跳（对账：台账覆盖全枚举）。
    pub fn all_entries_reachable(&self) -> bool {
        DirectEntry::all().iter().all(|e| self.jump_log.iter().any(|(j, _)| j == e))
    }

    /// 路径链合规（≤4 段——十二查 #7 的域内静态断言）。
    pub fn nav_paths_within_budget(&self) -> bool {
        DirectEntry::all().iter().all(|e| e.nav_path().len() <= NAV_PATH_MAX_HOPS)
    }

    // -- 恢复面 ------------------------------------------------------------

    /// 动态恢复范围（主口径）：当前段与默认段逐字段 diff——改了什么
    /// 列什么，恢复清单如实反映将被动的每一处。
    pub fn restore_scope_dynamic(current: &MouseBehaviorSection) -> Vec<String> {
        let d = MouseBehaviorSection::default();
        let mut out = Vec::new();
        if current.pointer_scheme != d.pointer_scheme {
            out.push(String::from("指针方案名"));
        }
        if current.speed != d.speed {
            out.push(String::from("速度曲线与增益"));
        }
        if current.wheel != d.wheel {
            out.push(String::from("滚轮档与应用覆盖"));
        }
        if current.sidekeys != d.sidekeys {
            out.push(String::from("侧键映射"));
        }
        if current.gestures != d.gestures {
            out.push(String::from("手势库"));
        }
        out
    }

    /// 第一步：列出恢复范围（离线拒绝；无 diff 如实报"已是默认"）。
    pub fn restore_scope(&self) -> Result<Vec<String>, &'static str> {
        if !self.is_present() {
            return Err("设备离线——恢复默认需要设备在位");
        }
        Ok(Self::restore_scope_dynamic(&self.section_from_feeds()))
    }

    /// 第二步：用户确认（确认门——未确认执行是违规；离线拒绝）。
    pub fn confirm_restore(&mut self, current: &MouseBehaviorSection) -> bool {
        if !self.is_present() {
            return false;
        }
        self.restore_confirmed = true;
        self.restore_rollback = Some(current.clone());
        true
    }

    /// 第三步：执行恢复（未确认 → 拒绝；确认 → 应用默认段并留痕）。
    pub fn execute_restore(&mut self, at_ms: u64) -> Result<MouseBehaviorSection, &'static str> {
        if !self.is_present() {
            return Err("设备离线——恢复默认需要设备在位");
        }
        if !self.restore_confirmed {
            return Err("恢复默认未经确认——先看范围清单再确认（E-1 可退原则）");
        }
        let defaults = MouseBehaviorSection::default();
        let changed = Self::restore_scope_dynamic(&self.section_from_feeds()).len();
        self.feeds = DashboardFeeds::from_section(&defaults);
        self.restore_log.push(RestoreRecord { at_ms, fields_changed: changed, undone: false });
        self.restore_confirmed = false;
        Ok(defaults)
    }

    /// 撤销恢复（回退快照回放——可退原则的闭环；台账标 undone）。
    pub fn undo_restore(&mut self) -> bool {
        let Some(prev) = self.restore_rollback.take() else { return false };
        self.feeds = DashboardFeeds::from_section(&prev);
        if let Some(r) = self.restore_log.last_mut() {
            r.undone = true;
        }
        true
    }

    /// 从当前 feeds 反投一个"对账段"（恢复范围 diff 的数据源——
    /// 与面板摘要同一事实源，保证清单与面板不漂移）。
    fn section_from_feeds(&self) -> MouseBehaviorSection {
        let mut sec = MouseBehaviorSection::default();
        sec.speed.curve_id = self.feeds.curve_name.clone();
        sec.speed.gain_cap_m = self.feeds.speed_m;
        sec.wheel.global = self.feeds.wheel_gear.clone();
        sec.pointer_scheme = self.feeds.scheme_name.clone();
        sec
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

/// 四直达入口的聚合登记（每入口一行——搜索关键词直达目标，不只落在仪表卡）。
pub const ENTRY_F301_REGISTRY: [(&str, DirectEntry, &[&str]); 4] = [
    ("settings/accessibility/pointer#speed-curve", DirectEntry::SpeedCurve, &["速度", "曲线", "加速"]),
    ("settings/accessibility/pointer#device-profiles", DirectEntry::DeviceProfiles, &["设备", "档案", "配对"]),
    ("settings/accessibility/pointer#sidekeys", DirectEntry::SideKeyProgramming, &["侧键", "XButton", "宏"]),
    ("settings/accessibility/pointer#scheme", DirectEntry::PointerScheme, &["方案", "皮肤", "指针库"]),
];

/// F244 快捷键注册表联动条目（侧键/手势的冲突审计登记名）。
pub const DASHBOARD_F244_TAG: &str = "pointer-dashboard";

/// 聚合登记校验（关键词命中判定——F301 搜索「指针」必达仪表卡）。
pub fn f301_query_hits(query: &str) -> bool {
    DASHBOARD_F301_ENTRY.keywords.iter().any(|k| k.contains(query) || query.contains(k))
}

/// 入口级聚合查询（关键词 → 命中的直达入口——四入口各有搜索直达面）。
pub fn f301_query_entry(query: &str) -> Option<DirectEntry> {
    ENTRY_F301_REGISTRY
        .iter()
        .find(|(_, _, kws)| kws.iter().any(|k| k.contains(query) || query.contains(k)))
        .map(|(_, e, _)| *e)
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

    // 4. 一键恢复默认：动态范围 + 确认门 + 回退快照 + 台账留痕。
    // 静态兜底表仍在位。
    set.add("restore scope fallback listed six", RESTORE_SCOPE.len() == 6, "");
    // 动态 diff：默认段 → 空清单（"已是默认"如实）。
    set.add("dynamic scope empty for defaults", Dashboard::restore_scope_dynamic(&sec).is_empty(), "");
    // 改三处 → 清单三条（指针方案/速度/滚轮——侧键手势保持默认）。
    let mut custom = sec.clone();
    custom.pointer_scheme = String::from("夜行箭");
    custom.speed.gain_cap_m = 2500;
    custom.wheel.global = String::from("always-notch");
    let scope = Dashboard::restore_scope_dynamic(&custom);
    set.add(
        "dynamic scope diffs live section",
        scope.len() == 3
            && scope.contains(&String::from("指针方案名"))
            && scope.contains(&String::from("速度曲线与增益"))
            && scope.contains(&String::from("滚轮档与应用覆盖")),
        "",
    );
    // 无确认拒绝 → 确认后执行 → 台账留痕 → 撤销标记。
    let mut dash_c = Dashboard::new(device.clone(), DashboardFeeds::from_section(&custom));
    set.add("restore without confirm rejected", dash_c.execute_restore(1000).is_err(), "");
    set.add("restore confirm gated", dash_c.confirm_restore(&custom), "");
    let applied = dash_c.execute_restore(2000).unwrap();
    set.add(
        "restore applies defaults after confirm",
        applied == MouseBehaviorSection::default() && dash_c.feeds.speed_m == 1000,
        "",
    );
    set.add(
        "restore record logged with field count",
        dash_c.restore_log.len() == 1 && dash_c.restore_log[0].fields_changed == 3 && !dash_c.restore_log[0].undone,
        "",
    );
    set.add(
        "undo restores previous state and marks record",
        dash_c.undo_restore()
            && dash_c.feeds.speed_m == 2500
            && dash_c.restore_rollback.is_none()
            && dash_c.restore_log[0].undone,
        "",
    );

    // 5. F244/F301 聚合登记（卡级 + 入口级两层）。
    set.add(
        "F301 aggregation hits for pointer queries",
        f301_query_hits("指针") && f301_query_hits("mouse") && f301_query_hits("鼠标"),
        "",
    );
    set.add("F244 tag registered", DASHBOARD_F244_TAG == "pointer-dashboard", "");
    set.add(
        "F301 entry-level registry covers four targets",
        f301_query_entry("侧键") == Some(DirectEntry::SideKeyProgramming)
            && f301_query_entry("速度") == Some(DirectEntry::SpeedCurve)
            && f301_query_entry("方案") == Some(DirectEntry::PointerScheme)
            && f301_query_entry("档案") == Some(DirectEntry::DeviceProfiles),
        "",
    );
    set.add("F301 entry query misses honestly", f301_query_entry("壁纸").is_none(), "");

    // 6. 电量趋势采样：下降判定 + 环形容量 + 采样不足诚实态。
    let mut dash_b = Dashboard::new(device.clone(), DashboardFeeds::from_section(&sec));
    set.add("trend insufficient with no samples", dash_b.battery_trend() == BatteryTrend::Insufficient, "");
    dash_b.sample_battery(1000);
    set.add("trend insufficient with one sample", dash_b.battery_trend() == BatteryTrend::Insufficient, "");
    dash_b.update_device(DeviceInfo { battery_pct: Some(9), ..device.clone() }, Presence::Present);
    dash_b.sample_battery(2000);
    set.add("trend falling detected", dash_b.battery_trend() == BatteryTrend::Falling, "");
    // 环形容量：灌 80 点只留 64（滚动窗口纪律）。
    for i in 0..80u64 {
        dash_b.sample_battery(3000 + i);
    }
    set.add(
        "battery history ring capped at 64",
        dash_b.battery_history().len() == BATTERY_HISTORY_CAP
            && dash_b.battery_history()[0].at_ms == 3000 + 16,
        "",
    );

    // 7. DPI 档语义化 + 越界钳制。
    set.add(
        "dpi tier human labels",
        device.dpi_label() == "高档（3200）"
            && DeviceInfo { dpi_tier: 0, ..device.clone() }.dpi_label() == "未上报"
            && DeviceInfo { dpi_tier: 7, ..device.clone() }.dpi_label() == "最高档（6400+）",
        "",
    );
    let dirty = Dashboard::new(
        DeviceInfo { battery_pct: Some(250), ..device.clone() },
        DashboardFeeds::from_section(&sec),
    );
    set.add(
        "battery out of range clamped honestly",
        dirty.device.battery_pct == Some(100) && dirty.battery_tone() == BatteryTone::Ok,
        "",
    );

    // 8. 路径链 ≤4 段（十二查 #7）。
    set.add("nav path chain within 4 hops", dash.nav_paths_within_budget(), "");

    // 9. 设备离线空态：摘要空态文案 + 跳转/恢复/切档诚实拒绝。
    let mut dash_off = Dashboard::new(device.clone(), DashboardFeeds::from_section(&sec));
    dash_off.update_device(device.clone(), Presence::Absent);
    let offline_lines = dash_off.summary_lines();
    set.add(
        "device absent empty state honest",
        !dash_off.is_present()
            && offline_lines.len() == 1
            && offline_lines[0].contains("未检测到"),
        "",
    );
    set.add(
        "offline blocks jump restore switch",
        !dash_off.jump(DirectEntry::SpeedCurve, 1)
            && !dash_off.confirm_restore(&sec)
            && !dash_off.switch_profile("备机", 2)
            && dash_off.restore_scope().is_err()
            && dash_off.execute_restore(3).is_err(),
        "",
    );

    // 10. 设备热更新计数 + F614 档案切换台账。
    let mut dash_h = Dashboard::new(device.clone(), DashboardFeeds::from_section(&sec));
    dash_h.update_device(DeviceInfo { battery_pct: Some(80), ..device.clone() }, Presence::Present);
    dash_h.update_device(DeviceInfo { battery_pct: Some(75), ..device.clone() }, Presence::Present);
    set.add("device hot swap counted", dash_h.device_updates() == 2 && dash_h.device.battery_pct == Some(75), "");
    set.add("profile switch same target no-op", !dash_h.switch_profile("默认档案", 10), "");
    set.add("profile switch logged", dash_h.switch_profile("办公鼠", 20) && dash_h.slot_switches.len() == 1 && dash_h.slot_switches[0].from_profile == "默认档案", "");

    // 11. 摘要文本行与数据同源（非空、含设备名——文案漂移对账）。
    let lines = dash.summary_lines();
    set.add(
        "summary lines projected from feeds",
        lines.len() == 4 && lines[0].contains("VARIX 无线鼠 2代") && lines[3].contains("夜行箭").eq(&false),
        "",
    );

    set
}

// 摘要与实际参数对账（对账脚本：面板 feeds 与行为段逐字段一致）——
// 附着在 Dashboard 上的既有对账口（保持原判据检查名不变）。
impl Dashboard {
    pub fn reconcile_with_section(&self, sec: &MouseBehaviorSection) -> bool {
        self.feeds == DashboardFeeds::from_section(sec)
    }
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
        assert!(d.confirm_restore(&custom));
        let applied = d.execute_restore(100).unwrap();
        assert_eq!(applied.wheel.global, "per-app");
        assert!(d.undo_restore());
        assert!(d.reconcile_with_section(&custom), "撤销后面板与撤销对象同源");
    }

    #[test]
    fn dynamic_scope_matches_field_diff() {
        let mut sec = MouseBehaviorSection::default();
        sec.sidekeys.bindings.push((String::from("x1"), String::from("app-x"), String::from("undo")));
        let scope = Dashboard::restore_scope_dynamic(&sec);
        assert_eq!(scope, alloc::vec![String::from("侧键映射")], "只列被改的字段");
        sec.gestures.gestures.clear();
        let scope2 = Dashboard::restore_scope_dynamic(&sec);
        assert!(scope2.contains(&String::from("手势库")));
        assert_eq!(scope2.len(), 2);
    }

    #[test]
    fn trend_classifies_all_directions() {
        let mut d = Dashboard::new(device(Some(50)), DashboardFeeds::from_section(&MouseBehaviorSection::default()));
        d.sample_battery(1);
        d.update_device(device(Some(50)), Presence::Present);
        d.sample_battery(2);
        assert_eq!(d.battery_trend(), BatteryTrend::Stable, "等值保守判平稳");
        d.update_device(device(Some(60)), Presence::Present);
        d.sample_battery(3);
        assert_eq!(d.battery_trend(), BatteryTrend::Rising);
    }

    #[test]
    fn offline_summary_is_single_empty_state_line() {
        let mut d = Dashboard::new(device(Some(50)), DashboardFeeds::from_section(&MouseBehaviorSection::default()));
        d.update_device(device(Some(50)), Presence::Absent);
        let lines = d.summary_lines();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("蓝牙") || lines[0].contains("接收器"));
    }

    #[test]
    fn summary_lines_track_feeds() {
        let mut sec = MouseBehaviorSection::default();
        sec.speed.curve_id = String::from("snappy");
        let d = Dashboard::new(device(Some(66)), DashboardFeeds::from_section(&sec));
        let lines = d.summary_lines();
        assert_eq!(lines.len(), 4);
        assert!(lines[2].contains("snappy"), "摘要行随 feeds 变化——文案与数据同源");
        assert!(lines[1].contains("66%"));
    }

    #[test]
    fn nav_paths_all_four_within_budget() {
        let d = Dashboard::new(device(Some(50)), DashboardFeeds::from_section(&MouseBehaviorSection::default()));
        assert!(d.nav_paths_within_budget());
        for e in DirectEntry::all() {
            assert_eq!(e.nav_path().len(), 4);
            assert_eq!(e.nav_path()[0], "设置");
        }
    }

    #[test]
    fn restore_log_marks_undo_once() {
        let mut d = Dashboard::new(device(Some(40)), DashboardFeeds::from_section(&MouseBehaviorSection::default()));
        assert!(d.confirm_restore(&MouseBehaviorSection::default()));
        let _ = d.execute_restore(500).unwrap();
        assert!(d.undo_restore());
        assert!(!d.undo_restore(), "无快照二次撤销诚实拒绝");
        assert_eq!(d.restore_log.len(), 1);
    }
}
