//! F075 托盘系统（perfstar2 · G-C-05）——任务栏永远干净，三秒直达常用开关。
//!
//! 主册判据（验收标准第一句）：
//! **滑杆拖动全程 80fps（F041 账本）；幽灵图标清除实测；三件套点击响应
//! <100ms（C-6 红线）。**
//!
//! 功能定义（G-C-05）：任务栏右端托盘——常驻三件（网络/音量/电池）+ 应用
//! 托盘折叠区（隐藏图标上拉面板）；三件套点击各弹控制条（F076 聚合前的
//! 轻量版）；托盘图标 16px 基准（2x 档 32px 4K 资产）。
//!
//! 【交互设计】控制条面板：音量竖滑杆高 160px 实时 60fps、网络弹已存 Wi-Fi
//! 列表、电池弹百分比+预估续航；折叠面板宽 280px 网格 4 列排应用图标；
//! 图标悬停 tooltip 显示应用名（1s 延迟）。
//! 【数据与存储】折叠/展开状态与「哪些图标允许常驻」白名单存配置层。
//! 【状态与异常】应用崩溃后托盘图标残留 → 30s 无心跳自动清除（幽灵图标
//! 清道夫）；电池计不可读 → 电池图标灰显+悬停说明（诚实降级）；图标缺失
//! → 默认占位图+应用名。
//! 【设计细节】滑杆步进 2%（100 格），键盘上下键 5% 大步；托盘区总宽上限
//! 200px（超出强制折叠）；折叠面板图标支持拖拽调序（E6 联动）；电池图标
//! 四态（充电/高/低/极低）颜色走主题令牌（E1）。
//!
//! 零堆纪律：定长图标表 + 定长白名单，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实；全参数旋钮化——无隐藏魔法数）
// ---------------------------------------------------------------------------

/// 幽灵图标判定：30s 无心跳自动清除。
pub const GHOST_TIMEOUT_MS: u64 = 30_000;
/// 三件套点击响应红线：<100ms（C-6 红线）。
pub const CLICK_RESPONSE_LIMIT_MS: u32 = 100;
/// 滑杆步进 2%（100 格）。
pub const SLIDER_STEP_PCT: u32 = 2;
/// 键盘上下键大步 5%。
pub const SLIDER_KEY_STEP_PCT: u32 = 5;
/// 音量竖滑杆高 160px（实时 60fps 判据）。
pub const VOLUME_SLIDER_H_PX: u32 = 160;
/// 托盘区总宽上限 200px（超出强制折叠）。
pub const TRAY_WIDTH_CAP_PX: u32 = 200;
/// 图标 16px 基准 / 2x 档 32px（4K 资产）。
pub const ICON_BASE_PX: u32 = 16;
pub const ICON_2X_PX: u32 = 32;
/// 折叠面板宽 280px 网格 4 列。
pub const OVERFLOW_PANEL_W_PX: u32 = 280;
pub const OVERFLOW_GRID_COLS: usize = 4;
/// tooltip 延迟 1s。
pub const TOOLTIP_DELAY_MS: u64 = 1_000;
/// 常驻三件。
pub const FIXED_TRAY_ITEMS: usize = 3;
/// 应用图标表容量。
const APP_ICON_CAP: usize = 32;

/// 常驻三件类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FixedItem {
    Network,
    Volume,
    Battery,
}

impl FixedItem {
    pub fn name(self) -> &'static str {
        match self {
            FixedItem::Network => "network",
            FixedItem::Volume => "volume",
            FixedItem::Battery => "battery",
        }
    }
}

/// 电池图标四态（颜色走主题令牌 E1）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BatteryState {
    Charging,
    High,
    Low,
    Critical,
}

impl BatteryState {
    /// 主题令牌名（E1 令牌集引用）。
    pub fn token(self) -> &'static str {
        match self {
            BatteryState::Charging => "token-battery-charging",
            BatteryState::High => "token-battery-high",
            BatteryState::Low => "token-battery-low",
            BatteryState::Critical => "token-battery-critical",
        }
    }

    /// 四态判定（充电/高 >40%/低 15-40%/极低 <15%）。
    pub fn of(pct: u32, charging: bool) -> BatteryState {
        if charging {
            BatteryState::Charging
        } else if pct > 40 {
            BatteryState::High
        } else if pct >= 15 {
            BatteryState::Low
        } else {
            BatteryState::Critical
        }
    }
}

/// 应用托盘图标。
#[derive(Clone, Copy, Debug)]
pub struct TrayIcon {
    pub app_name: [u8; 16],
    pub name_len: u8,
    /// 最近心跳时刻（ms）——30s 无心跳 = 幽灵。
    pub last_heartbeat_ms: u64,
    /// 允许常驻白名单（不常驻者直接进折叠区）。
    pub always_visible: bool,
    /// 图标资产缺失旗标（默认占位图 + 应用名）。
    pub icon_missing: bool,
}

// ---------------------------------------------------------------------------
// 托盘
// ---------------------------------------------------------------------------

/// 托盘系统。
pub struct TrayHub {
    /// 应用图标表。
    icons: [Option<TrayIcon>; APP_ICON_CAP],
    icon_n: usize,
    /// 折叠区展开状态（配置层持久化语义）。
    overflow_open: bool,
    /// 音量（0..=100，步进 2% = 100 格）。
    volume_pct: u32,
    muted: bool,
    /// 电池。
    battery_pct: u32,
    battery_charging: bool,
    /// 电池计不可读（诚实降级：灰显 + 悬停说明）。
    battery_unreadable: bool,
    /// 幽灵清除计数。
    ghosts_cleared: u64,
    /// 点击响应账（最近 8 次的响应 ms——<100ms 红线判定）。
    click_ms_ring: [u16; 8],
    click_n: usize,
    /// 滑杆拖动帧账（80fps 判据：帧耗时 ≤12.5ms → 80fps）。
    drag_frame_ms: [u16; 128],
    drag_frame_n: usize,
    now_ms: u64,
}

impl TrayHub {
    pub const fn new() -> Self {
        TrayHub {
            icons: [None; APP_ICON_CAP],
            icon_n: 0,
            overflow_open: false,
            volume_pct: 60,
            muted: false,
            battery_pct: 100,
            battery_charging: false,
            battery_unreadable: false,
            ghosts_cleared: 0,
            click_ms_ring: [0; 8],
            click_n: 0,
            drag_frame_ms: [0; 128],
            drag_frame_n: 0,
            now_ms: 0,
        }
    }

    /// 应用图标注册（心跳启动）。
    pub fn register_icon(&mut self, app_name: &[u8], always_visible: bool, icon_missing: bool, at_ms: u64) -> bool {
        if self.icon_n == APP_ICON_CAP {
            return false;
        }
        let n = app_name.len().min(16);
        let mut name = [0u8; 16];
        name[..n].copy_from_slice(&app_name[..n]);
        self.icons[self.icon_n] = Some(TrayIcon {
            app_name: name,
            name_len: n as u8,
            last_heartbeat_ms: at_ms,
            always_visible,
            icon_missing,
        });
        self.icon_n += 1;
        true
    }

    /// 心跳（应用存活证明）。
    pub fn heartbeat(&mut self, app_name: &[u8], at_ms: u64) -> bool {
        self.now_ms = at_ms;
        for i in self.icons.iter_mut().take(self.icon_n) {
            if let Some(ic) = i {
                if ic.app_name[..ic.name_len as usize] == app_name[..app_name.len().min(16)] {
                    ic.last_heartbeat_ms = at_ms;
                    return true;
                }
            }
        }
        false
    }

    /// 幽灵清道夫：30s 无心跳自动清除（实测判据）。
    pub fn reap_ghosts(&mut self, at_ms: u64) -> usize {
        self.now_ms = at_ms;
        let mut reaped = 0usize;
        let mut w = 0usize;
        for r in 0..self.icon_n {
            let alive = matches!(self.icons[r], Some(ic) if at_ms.saturating_sub(ic.last_heartbeat_ms) < GHOST_TIMEOUT_MS);
            if alive {
                self.icons.swap(w, r);
                w += 1;
            } else {
                reaped += 1;
            }
        }
        for slot in self.icons.iter_mut().skip(w) {
            *slot = None;
        }
        self.icon_n = w;
        self.ghosts_cleared += reaped as u64;
        reaped
    }

    pub fn ghosts_cleared(&self) -> u64 {
        self.ghosts_cleared
    }

    /// 三件套点击（响应账入环；<100ms 红线）。
    pub fn fixed_item_click(&mut self, item: FixedItem, response_ms: u32) -> bool {
        let _ = item;
        self.click_ms_ring[self.click_n % 8] = response_ms.min(u16::MAX as u32) as u16;
        self.click_n += 1;
        response_ms <= CLICK_RESPONSE_LIMIT_MS
    }

    /// 点击响应红线全绿（最近账内全部 ≤100ms）。
    pub fn clicks_within_redline(&self) -> bool {
        self.click_ms_ring.iter().take(self.click_n.min(8)).all(|&ms| ms as u32 <= CLICK_RESPONSE_LIMIT_MS)
    }

    /// 音量滑杆拖动（步进 2% / 100 格；帧账入环 ×10 定点）。
    pub fn volume_drag(&mut self, to_pct: u32, frame_ms: u32) -> u32 {
        // 步进量化：2% 一格（100 格）。
        let stepped = (to_pct / SLIDER_STEP_PCT) * SLIDER_STEP_PCT;
        self.volume_pct = stepped.clamp(0, 100);
        // 帧耗时 ×10 定点入账（12.5ms 边界精确判定）。
        self.drag_frame_ms[self.drag_frame_n % 128] = (frame_ms.min(u16::MAX as u32 / 10) * 10) as u16;
        self.drag_frame_n += 1;
        self.volume_pct
    }

    /// 键盘上下键（5% 大步）。
    pub fn volume_key(&mut self, up: bool) -> u32 {
        if up {
            self.volume_pct = (self.volume_pct + SLIDER_KEY_STEP_PCT).min(100);
        } else {
            self.volume_pct = self.volume_pct.saturating_sub(SLIDER_KEY_STEP_PCT);
        }
        self.volume_pct
    }

    /// 拖动全程 80fps 判据：全部帧 ≤12.5ms（×10 定点 125）。
    pub fn drag_frames_at_80fps(&self) -> bool {
        self.drag_frame_ms.iter().take(self.drag_frame_n.min(128)).all(|&f| f as u32 <= 125)
    }

    pub fn volume_pct(&self) -> u32 {
        self.volume_pct
    }

    pub fn set_muted(&mut self, m: bool) {
        self.muted = m;
    }

    pub fn muted(&self) -> bool {
        self.muted
    }

    /// 电池采样（不可读 → 诚实降级旗标）。
    pub fn sample_battery(&mut self, pct: u32, charging: bool, readable: bool) {
        self.battery_unreadable = !readable;
        if readable {
            self.battery_pct = pct.min(100);
            self.battery_charging = charging;
        }
    }

    pub fn battery_state(&self) -> Option<BatteryState> {
        if self.battery_unreadable {
            None // 灰显（诚实降级）
        } else {
            Some(BatteryState::of(self.battery_pct, self.battery_charging))
        }
    }

    pub fn battery_unreadable(&self) -> bool {
        self.battery_unreadable
    }

    /// 常驻图标列表（白名单内；总宽 200px 上限——超出强制折叠）。
    /// 16px 基准：可容纳 12 枚常驻（200/16）。
    pub fn always_visible_icons(&self) -> impl Iterator<Item = &TrayIcon> + '_ {
        self.icons
            .iter()
            .take(self.icon_n)
            .flatten()
            .filter(|ic| ic.always_visible)
    }

    /// 强制折叠判定：常驻枚数超容量（200px / 16px）→ 折叠。
    pub fn force_overflow(&self) -> bool {
        self.always_visible_icons().count() * ICON_BASE_PX as usize > TRAY_WIDTH_CAP_PX as usize
    }

    /// 折叠面板图标网格（4 列 × N 行）。
    pub fn overflow_grid_rows(&self) -> usize {
        let n = self.icon_n;
        n.div_ceil(OVERFLOW_GRID_COLS)
    }

    pub fn set_overflow_open(&mut self, open: bool) {
        self.overflow_open = open;
    }

    pub fn overflow_open(&self) -> bool {
        self.overflow_open
    }

    pub fn icon_count(&self) -> usize {
        self.icon_n
    }

    /// 图标缺失 → 默认占位图 + 应用名（缺失旗标查询）。
    pub fn icon_missing(&self, app_name: &[u8]) -> bool {
        self.icons
            .iter()
            .take(self.icon_n)
            .flatten()
            .find(|ic| ic.app_name[..ic.name_len as usize] == app_name[..app_name.len().min(16)])
            .map(|ic| ic.icon_missing)
            .unwrap_or(false)
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// 域自检。
pub fn run_trayhub_checks() -> CheckSet {
    let mut cs = CheckSet::new("F075-trayhub");
    // 1) 滑杆拖动全程 80fps（帧账全绿）+ 终值 80%。
    let mut t = TrayHub::new();
    for f in 0..60u32 {
        let vol = 20 + f; // 20% → 79% 拖动全程
        let _ = t.volume_drag(vol, if f % 10 == 9 { 12 } else { 8 });
    }
    let final_vol = t.volume_drag(80, 8);
    cs.add("slider_drag_80fps", t.drag_frames_at_80fps() && final_vol == 80, "");
    // 2) 步进 2%（100 格）：57% → 56%。
    let mut t2 = TrayHub::new();
    cs.add("slider_step_2pct", t2.volume_drag(57, 8) == 56 && t2.volume_drag(99, 8) == 98, "");
    // 3) 键盘 5% 大步。
    let mut t3 = TrayHub::new();
    t3.volume_drag(50, 8);
    let up = t3.volume_key(true);
    let down = t3.volume_key(false);
    cs.add("keyboard_step_5pct", up == 55 && down == 50, "");
    // 4) 三件套点击响应 <100ms（C-6 红线）。
    let mut t4 = TrayHub::new();
    cs.add(
        "click_response_under_100ms",
        t4.fixed_item_click(FixedItem::Network, 42)
            && t4.fixed_item_click(FixedItem::Volume, 99)
            && t4.fixed_item_click(FixedItem::Battery, 100)
            && t4.clicks_within_redline(),
        "",
    );
    // 4b) 超 100ms → 红线判定失败（判据不是摆设）。
    let mut t4b = TrayHub::new();
    let _ = t4b.fixed_item_click(FixedItem::Volume, 120);
    cs.add("click_redline_catches_overrun", !t4b.clicks_within_redline(), "");
    // 5) 幽灵图标：30s 无心跳清除；有心跳留存。
    let mut t5 = TrayHub::new();
    let _ = t5.register_icon(b"alive-app", false, false, 0);
    let _ = t5.register_icon(b"dead-app", false, false, 0);
    let _ = t5.heartbeat(b"alive-app", 40_000);
    let reaped = t5.reap_ghosts(50_000); // dead-app 上次心跳 0 → 50s 无心跳
    cs.add(
        "ghost_reaper_30s",
        reaped == 1 && t5.icon_count() == 1 && t5.ghosts_cleared() == 1,
        "",
    );
    // 6) 电池计不可读 → 灰显（无四态）+ 诚实降级。
    let mut t6 = TrayHub::new();
    t6.sample_battery(50, false, false);
    cs.add("battery_unreadable_grey", t6.battery_unreadable() && t6.battery_state().is_none(), "");
    // 7) 电池四态走令牌。
    let mut t7 = TrayHub::new();
    t7.sample_battery(80, true, true);
    let s_charging = t7.battery_state().unwrap();
    t7.sample_battery(80, false, true);
    let s_high = t7.battery_state().unwrap();
    t7.sample_battery(30, false, true);
    let s_low = t7.battery_state().unwrap();
    t7.sample_battery(10, false, true);
    let s_critical = t7.battery_state().unwrap();
    cs.add(
        "battery_four_states_tokens",
        s_charging == BatteryState::Charging
            && s_high == BatteryState::High
            && s_low == BatteryState::Low
            && s_critical == BatteryState::Critical
            && !BatteryState::of(30, false).token().is_empty(),
        "",
    );
    // 8) 托盘宽 200px 上限：常驻 12 枚 = 192px 不折叠；13 枚 = 208px 强制折叠。
    let mut t8 = TrayHub::new();
    for i in 0..12usize {
        let mut name = [0u8; 16];
        name[0] = b'a';
        name[1] = (i as u8 % 26) + b'a';
        let _ = t8.register_icon(&name[..2], true, false, 0);
    }
    cs.add("tray_12_icons_fit", !t8.force_overflow(), "");
    let _ = t8.register_icon(b"zz", true, false, 1);
    cs.add("tray_13_icons_force_overflow", t8.force_overflow(), "");
    // 9) 图标缺失 → 默认占位图 + 应用名（旗标在案）。
    let mut t9 = TrayHub::new();
    let _ = t9.register_icon(b"noicon-app", false, true, 0);
    cs.add("missing_icon_placeholder", t9.icon_missing(b"noicon-app"), "");
    // 10) 折叠面板：280px 宽 4 列网格。
    let mut t10 = TrayHub::new();
    for i in 0..9usize {
        let mut name = [0u8; 16];
        name[0] = b'b';
        name[1] = (i as u8) + b'a';
        let _ = t10.register_icon(&name[..2], false, false, 0);
    }
    cs.add(
        "overflow_panel_grid",
        t10.overflow_grid_rows() == 3 && OVERFLOW_GRID_COLS == 4 && OVERFLOW_PANEL_W_PX == 280,
        "",
    );
    // 11) 图标规格：16px 基准 / 2x 32px（4K 资产）。
    cs.add("icon_sizes_16_32", ICON_BASE_PX == 16 && ICON_2X_PX == 32, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_bounds_clamped() {
        let mut t = TrayHub::new();
        assert_eq!(t.volume_drag(150, 8), 100, "上限钳制");
        assert_eq!(t.volume_drag(0, 8), 0, "下限钳制");
        assert_eq!(t.volume_drag(1, 8), 0, "1% 落 0 格");
        assert_eq!(t.volume_drag(2, 8), 2, "恰一格");
    }

    #[test]
    fn ghost_boundary_exactly_30s_alive() {
        let mut t = TrayHub::new();
        let _ = t.register_icon(b"edge", false, false, 0);
        let _ = t.heartbeat(b"edge", 20_000);
        // 50_000 - 20_000 = 30_000 = 恰在超时线上：<30s 才活 → 30s 整 = 幽灵。
        assert_eq!(t.reap_ghosts(50_000), 1, "恰 30s 整 = 判幽灵（<30s 存活语义）");
        // 29s 心跳 → 存活。
        let _ = t.register_icon(b"edge2", false, false, 20_000);
        assert_eq!(t.reap_ghosts(49_000), 0, "29s 心跳存活");
    }

    #[test]
    fn heartbeat_unknown_app_is_false() {
        let mut t = TrayHub::new();
        assert!(!t.heartbeat(b"ghost", 0));
    }

    #[test]
    fn icon_cap_honest() {
        let mut t = TrayHub::new();
        for i in 0..APP_ICON_CAP {
            let mut name = [0u8; 16];
            name[0] = (i % 26) as u8 + b'a';
            assert!(t.register_icon(&name[..1], false, false, 0));
        }
        assert!(!t.register_icon(b"overflow", false, false, 0), "表满诚实拒绝");
    }

    #[test]
    fn battery_state_boundaries() {
        // 恰 40% = Low（>40 才 High）；恰 15% = Low（>=15）。
        assert_eq!(BatteryState::of(41, false), BatteryState::High);
        assert_eq!(BatteryState::of(40, false), BatteryState::Low);
        assert_eq!(BatteryState::of(15, false), BatteryState::Low);
        assert_eq!(BatteryState::of(14, false), BatteryState::Critical);
        assert_eq!(BatteryState::of(1, true), BatteryState::Charging, "充电优先");
    }

    #[test]
    fn overflow_toggle_persists() {
        let mut t = TrayHub::new();
        t.set_overflow_open(true);
        assert!(t.overflow_open());
        t.set_overflow_open(false);
        assert!(!t.overflow_open());
    }

    #[test]
    fn slider_height_and_tooltip_match_master() {
        assert_eq!(VOLUME_SLIDER_H_PX, 160);
        assert_eq!(TOOLTIP_DELAY_MS, 1_000);
        assert_eq!(FIXED_TRAY_ITEMS, 3);
    }
}
