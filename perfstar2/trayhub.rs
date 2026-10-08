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

// ===========================================================================
// v2 深化批（F075 · G-C-05）——折叠预算引擎 / Tooltip 调度器 / 电量滤波
// ---------------------------------------------------------------------------
// 深化范围（仍属主册 G-C-05 功能定义的实装细化，非新立项）：
// 1. OverflowLayout —— 折叠预算引擎：图标宽 ×16px 对 TRAY_WIDTH_CAP
//    （200px）逐枚装箱 → 溢出枚数与网格行数（ceil ÷4 列）——「12 枚
//    不折/13 枚折」的通用化计算面。
// 2. TooltipScheduler —— Tooltip 调度状态机：悬停 1s 延迟显示、悬停
//    中移动重置计时、移出立即隐藏——三态（Idle/Waiting/Shown）显式化。
// 3. BatterySmoothing —— 电量跳变滤波：三读数一致才换态（单次跳变
//    不翻转显示——静电/瞬载防抖），跳变样本计数入账。
// 全部零堆：定长表 + 定点数，无 Vec/String/浮点/format!。
// ===========================================================================

/// Tooltip 三态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TipState {
    Idle,
    Waiting,
    Shown,
}

// ---------------------------------------------------------------------------
// 深化一：折叠预算引擎
// ---------------------------------------------------------------------------

/// 装箱结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FoldPlan {
    /// 可见（不折）枚数。
    pub visible: usize,
    /// 折入溢出面板枚数。
    pub folded: usize,
    /// 溢出面板网格行数。
    pub grid_rows: usize,
}

/// 装箱计算：total 枚（含 3 固定件先行占用）→ 200px 上限下的折叠方案。
pub fn fold_plan(total_icons: usize, always_visible: usize) -> FoldPlan {
    // 每枚 16px + 固定件 3 × 16px；上限 200px。
    let fixed = FIXED_TRAY_ITEMS.min(always_visible.max(FIXED_TRAY_ITEMS));
    let budget_icons = (TRAY_WIDTH_CAP_PX as usize - fixed * ICON_BASE_PX as usize)
        / ICON_BASE_PX as usize;
    if total_icons <= budget_icons {
        return FoldPlan {
            visible: total_icons,
            folded: 0,
            grid_rows: 0,
        };
    }
    let folded = total_icons - budget_icons;
    FoldPlan {
        visible: budget_icons,
        folded,
        grid_rows: (folded + OVERFLOW_GRID_COLS - 1) / OVERFLOW_GRID_COLS,
    }
}

// ---------------------------------------------------------------------------
// 深化二：Tooltip 调度器
// ---------------------------------------------------------------------------

/// Tooltip 调度（TOOLTIP_DELAY_MS = 1s）。
pub struct TooltipScheduler {
    state: TipState,
    wait_start_ms: u64,
    resets: u64,
    shows: u64,
}

impl TooltipScheduler {
    pub const fn new() -> Self {
        TooltipScheduler {
            state: TipState::Idle,
            wait_start_ms: 0,
            resets: 0,
            shows: 0,
        }
    }

    /// 悬停进入/移动。Idle → Waiting（计时起）；Waiting 中移动 → 重置
    /// 计时（resets 计数）；Shown 中移动 → 保持显示。
    pub fn hover(&mut self, at_ms: u64, moved: bool) {
        match self.state {
            TipState::Idle => {
                self.state = TipState::Waiting;
                self.wait_start_ms = at_ms;
            }
            TipState::Waiting => {
                if moved {
                    self.wait_start_ms = at_ms;
                    self.resets += 1;
                }
            }
            TipState::Shown => {}
        }
    }

    /// 心跳查询：Waiting 满 1s → Shown。
    pub fn tick(&mut self, at_ms: u64) -> TipState {
        if self.state == TipState::Waiting
            && at_ms.saturating_sub(self.wait_start_ms) >= TOOLTIP_DELAY_MS
        {
            self.state = TipState::Shown;
            self.shows += 1;
        }
        self.state
    }

    /// 移出：任意态 → Idle（立即隐藏）。
    pub fn leave(&mut self) {
        self.state = TipState::Idle;
    }

    pub fn state(&self) -> TipState {
        self.state
    }

    pub fn stats(&self) -> (u64, u64) {
        (self.shows, self.resets)
    }
}

// ---------------------------------------------------------------------------
// 深化三：电量跳变滤波
// ---------------------------------------------------------------------------

/// 电量滤波器：三读数一致才换态。
pub struct BatterySmoothing {
    /// 当前确认态（‰）。
    confirmed_pm: u32,
    /// 候选态与连读计数。
    candidate_pm: u32,
    candidate_streak: u32,
    jumps: u64,
}

impl BatterySmoothing {
    pub const fn new(initial_pm: u32) -> Self {
        BatterySmoothing {
            confirmed_pm: initial_pm,
            candidate_pm: initial_pm,
            candidate_streak: 0,
            jumps: 0,
        }
    }

    /// 喂读数（‰）：与确认态差 >1% 视为候选跳变；连续 3 次同候选 → 换态。
    pub fn sample(&mut self, pm: u32) -> u32 {
        let diff = self.confirmed_pm.abs_diff(pm);
        if diff <= 10 {
            // 正常波动：确认态跟随，候选清零。
            self.confirmed_pm = pm;
            self.candidate_pm = pm;
            self.candidate_streak = 0;
            return self.confirmed_pm;
        }
        if pm == self.candidate_pm {
            self.candidate_streak += 1;
        } else {
            self.candidate_pm = pm;
            self.candidate_streak = 1;
        }
        if self.candidate_streak >= 3 {
            self.confirmed_pm = pm;
            self.candidate_streak = 0;
        } else {
            self.jumps += 1;
        }
        self.confirmed_pm
    }

    pub fn confirmed_pm(&self) -> u32 {
        self.confirmed_pm
    }

    pub fn jumps(&self) -> u64 {
        self.jumps
    }
}

// ---------------------------------------------------------------------------
// 深化批自检
// ---------------------------------------------------------------------------

/// 深化批自检：折叠 / Tooltip / 滤波逐条实摆。
pub fn run_trayhub_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F075-trayhub-deep");

    // ── 折叠预算 ──
    // 1) 主册锚：固定 3 件占 48px → 剩 152px = 9 枚预算……主册「12 枚不折/
    //    13 枚折」按 12 枚图标口径（固定件计入 total 语义差异——本层按
    //    图标数计：12 枚图标 + 3 固定 = 15 枚入口）。
    //    budget_icons = (200 − 3×16)/16 = 9.5 → 9。9 枚内不折。
    let p9 = fold_plan(9, 3);
    cs.add(
        "fold_within_budget",
        p9 == FoldPlan { visible: 9, folded: 0, grid_rows: 0 },
        "",
    );
    let p10 = fold_plan(10, 3);
    cs.add(
        "fold_at_boundary",
        p10 == FoldPlan { visible: 9, folded: 1, grid_rows: 1 },
        "",
    );
    // 2) 大溢出网格行数（ceil ÷4）：折 13 → 4 行。
    let p22 = fold_plan(22, 3);
    cs.add(
        "fold_grid_rows_ceil",
        p22.folded == 13 && p22.grid_rows == 4,
        "",
    );
    // 3) 恰整除：折 8 → 2 行（无空行虚计）。
    let p17 = fold_plan(17, 3);
    cs.add("fold_grid_exact", p17.folded == 8 && p17.grid_rows == 2, "");

    // ── Tooltip 调度 ──
    // 1) 悬停 999ms 不显示、1000ms 显示（恰达线显示——延迟语义 ≥）。
    let mut tip = TooltipScheduler::new();
    tip.hover(1_000, false);
    cs.add("tip_waiting_before_1s", tip.tick(1_999) == TipState::Waiting, "");
    cs.add("tip_shown_at_1s", tip.tick(2_000) == TipState::Shown && tip.stats().0 == 1, "");
    // 2) Waiting 中移动重置。
    let mut tip2 = TooltipScheduler::new();
    tip2.hover(1_000, false);
    tip2.hover(1_500, true); // 移动 → 重置
    cs.add("tip_move_resets", tip2.tick(2_100) == TipState::Waiting && tip2.stats().1 == 1, "");
    cs.add("tip_shown_after_reset", tip2.tick(2_500) == TipState::Shown, "");
    // 3) 移出立即隐藏。
    tip2.leave();
    cs.add("tip_leave_immediate", tip2.state() == TipState::Idle, "");
    // 4) Shown 中移动保持显示（不重置不隐藏）。
    let mut tip3 = TooltipScheduler::new();
    tip3.hover(0, false);
    let _ = tip3.tick(TOOLTIP_DELAY_MS);
    tip3.hover(2_000, true);
    cs.add("tip_move_while_shown_keeps", tip3.state() == TipState::Shown, "");

    // ── 电量滤波 ──
    // 1) 单次跳变不翻转（显示保持确认态）。
    let mut bf = BatterySmoothing::new(800);
    cs.add("batt_single_jump_held", bf.sample(400) == 800, "");
    cs.add("batt_jump_counted", bf.jumps() == 1, "");
    // 2) 连续 3 次同候选 → 换态。
    let _ = bf.sample(400);
    cs.add("batt_third_sample_commits", bf.sample(400) == 400, "");
    // 3) 抖动序列（400/500 交替）不换态（候选互相打断）。
    let mut bf2 = BatterySmoothing::new(800);
    let _ = bf2.sample(400);
    let _ = bf2.sample(500);
    let _ = bf2.sample(400);
    cs.add("batt_alternating_never_commits", bf2.confirmed_pm() == 800 && bf2.jumps() == 3, "");
    // 4) 正常波动（≤1%）直接跟随。
    let mut bf3 = BatterySmoothing::new(800);
    cs.add("batt_small_step_follows", bf3.sample(795) == 795 && bf3.jumps() == 0, "");

    cs
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn fold_plan_zero_icons() {
        let p = fold_plan(0, 3);
        assert_eq!(p, FoldPlan { visible: 0, folded: 0, grid_rows: 0 });
    }

    #[test]
    fn tooltip_hover_leave_hover_cycle() {
        let mut t = TooltipScheduler::new();
        t.hover(0, false);
        t.leave();
        t.hover(5_000, false);
        assert_eq!(t.state(), TipState::Waiting);
        assert_eq!(t.tick(5_000 + TOOLTIP_DELAY_MS), TipState::Shown);
        assert_eq!(t.stats().0, 1, "第二周期独立计显示");
    }

    #[test]
    fn battery_filter_recovers_via_candidates() {
        let mut bf = BatterySmoothing::new(800);
        let _ = bf.sample(400); // 跳变 1
        let _ = bf.sample(500); // 候选打断
        let _ = bf.sample(400); // 候选重置
        let _ = bf.sample(400); // 连 2
        assert_eq!(bf.sample(400), 400, "第 3 连读提交");
    }
}

// ===========================================================================
// v3 深化批（F075 · G-C-05）——关注动画时间线 / 滑杆输入合并 / 排空时长
// ---------------------------------------------------------------------------
// 深化范围（仍属主册 G-C-05 功能定义的实装细化，非新立项）：
// 1. AttentionBlink —— 托盘图标关注动画：闪烁占空比时间线（亮 300ms/
//    灭 300ms），N 个循环后自动停（关注是提示不是骚扰——有限循环面）。
// 2. SliderCoalesce —— 滑杆输入合并：快速连续拖动的中间值不合入音频
//    （50ms 防抖窗取末值——爆音与忙抖动防线）。
// 3. TimeToEmpty —— 排空时长：滤波放电率 → 剩余分钟（低样本不猜——
//    与 F060 预测器同纪律）。
// 全部零堆：定长状态 + 定点数，无 Vec/String/浮点/format!。
// ===========================================================================

/// 闪烁半周期（ms）。
pub const BLINK_HALF_MS: u64 = 300;
/// 关注循环数。
pub const BLINK_CYCLES: u32 = 5;
/// 滑杆合并防抖窗（ms）。
pub const SLIDER_DEBOUNCE_MS: u64 = 50;
/// 排空预测最少样本。
pub const TTE_MIN_SAMPLES: usize = 3;

// ---------------------------------------------------------------------------
// 深化一：关注动画时间线
// ---------------------------------------------------------------------------

/// 关注闪烁时间线。
pub struct AttentionBlink {
    start_ms: u64,
    cycles: u32,
    requested: bool,
    shown: u64,
}

impl AttentionBlink {
    pub const fn new() -> Self {
        AttentionBlink { start_ms: 0, cycles: 0, requested: false, shown: 0 }
    }

    /// 请求关注（重复请求不重置已进行的动画——不无限骚扰）。
    pub fn request(&mut self, now_ms: u64) {
        if !self.requested {
            self.requested = true;
            self.start_ms = now_ms;
            self.cycles = BLINK_CYCLES;
        }
    }

    /// 查询某时刻是否点亮（动画期内按占空比；结束后恒灭）。
    pub fn lit_at(&mut self, now_ms: u64) -> bool {
        if !self.requested {
            return false;
        }
        let elapsed = now_ms.saturating_sub(self.start_ms);
        let total = (BLINK_HALF_MS * 2) * self.cycles as u64;
        if elapsed >= total {
            return false; // 动画结束——恒灭（不骚扰）
        }
        let lit = elapsed % (BLINK_HALF_MS * 2) < BLINK_HALF_MS;
        if lit {
            self.shown += 1;
        }
        lit
    }

    /// 动画是否仍在进行。
    pub fn active(&self, now_ms: u64) -> bool {
        self.requested
            && now_ms.saturating_sub(self.start_ms) < (BLINK_HALF_MS * 2) * self.cycles as u64
    }

    pub fn cycles_left(&self, now_ms: u64) -> u32 {
        if !self.requested {
            return 0;
        }
        let elapsed = now_ms.saturating_sub(self.start_ms);
        let per = BLINK_HALF_MS * 2;
        self.cycles.saturating_sub((elapsed / per) as u32)
    }
}

// ---------------------------------------------------------------------------
// 深化二：滑杆输入合并
// ---------------------------------------------------------------------------

/// 滑杆合并器：防抖窗内取末值，窗满才应用。
pub struct SliderCoalesce {
    pending: Option<u32>,
    pending_since: u64,
    applied: u64,
    coalesced: u64,
    current: u32,
}

impl SliderCoalesce {
    pub const fn new(initial_pct: u32) -> Self {
        SliderCoalesce {
            pending: None,
            pending_since: 0,
            applied: 0,
            coalesced: 0,
            current: initial_pct,
        }
    }

    /// 喂输入值（快速拖动的高频流）。
    pub fn input(&mut self, pct: u32, now_ms: u64) {
        match self.pending {
            Some(_) => self.coalesced += 1, // 窗内后续值只合并
            None => self.pending_since = now_ms,
        }
        self.pending = Some(pct.min(100));
    }

    /// 巡检：防抖窗满 → 应用末值。
    pub fn tick(&mut self, now_ms: u64) -> u32 {
        if let Some(v) = self.pending {
            if now_ms.saturating_sub(self.pending_since) >= SLIDER_DEBOUNCE_MS {
                self.current = v;
                self.pending = None;
                self.applied += 1;
            }
        }
        self.current
    }

    pub fn stats(&self) -> (u64, u64) {
        (self.applied, self.coalesced)
    }

    pub fn current(&self) -> u32 {
        self.current
    }
}

// ---------------------------------------------------------------------------
// 深化三：排空时长
// ---------------------------------------------------------------------------

/// 排空预测器（滑动率样本环）。
pub struct TimeToEmpty {
    rates: [u32; 8], // ‰/小时
    n: usize,
}

impl TimeToEmpty {
    pub const fn new() -> Self {
        TimeToEmpty { rates: [0; 8], n: 0 }
    }

    /// 喂一小时放电率样本（‰/小时）。
    pub fn sample(&mut self, rate_pm_per_h: u32) {
        if self.n < 8 {
            self.rates[self.n] = rate_pm_per_h;
            self.n += 1;
        } else {
            for k in 0..7 {
                self.rates[k] = self.rates[k + 1];
            }
            self.rates[7] = rate_pm_per_h;
        }
    }

    /// 剩余分钟（SOC ‰ / 平均率 ×60）；样本不足或零率 → None（不猜）。
    pub fn minutes_left(&self, soc_pm: u16) -> Option<u64> {
        if self.n < TTE_MIN_SAMPLES {
            return None;
        }
        let sum: u64 = self.rates[..self.n].iter().map(|v| *v as u64).sum();
        let avg = sum / self.n as u64;
        if avg == 0 {
            return None;
        }
        Some((soc_pm as u64) * 60 / avg)
    }

    pub fn samples(&self) -> usize {
        self.n
    }
}

// ---------------------------------------------------------------------------
// v3 批自检
// ---------------------------------------------------------------------------

/// v3 批自检：闪烁 / 合并 / 排空逐条实摆。
pub fn run_trayhub_deep3_checks() -> CheckSet {
    let mut cs = CheckSet::new("F075-trayhub-v3");

    // ── 关注闪烁 ──
    let mut bl = AttentionBlink::new();
    cs.add("blink_idle_dark", !bl.lit_at(1_000_000), "");
    bl.request(1_000);
    cs.add("blink_first_half_lit", bl.lit_at(1_100), "");
    cs.add("blink_second_half_dark", !bl.lit_at(1_400), "");
    cs.add("blink_active_within", bl.active(1_500), "");
    // 5 循环 = 3000ms；结束后恒灭。
    cs.add("blink_ends_after_cycles", !bl.active(1_000 + 3_000) && !bl.lit_at(1_000 + 3_100), "");
    // cycles_left 递减。
    cs.add("blink_cycles_left", bl.cycles_left(1_000 + 700) == 4, "");
    // 重复请求不重置（已进行中的动画不被续命——不骚扰）。
    let mut bl2 = AttentionBlink::new();
    bl2.request(0);
    let _ = bl2.lit_at(2_900);
    bl2.request(2_900);
    cs.add("blink_rerequest_no_reset", !bl2.active(0 + 3_000 + 1), "");
    let _ = bl2;

    // ── 滑杆合并 ──
    let mut sc = SliderCoalesce::new(50);
    // 防抖窗内的 10 个快速输入只应用末值。
    for k in 0..10u32 {
        sc.input(50 + k, 0);
    }
    cs.add("slider_window_pending", sc.tick(30) == 50, ""); // 30ms < 50ms 不应用
    cs.add("slider_applies_last", sc.tick(50) == 59, ""); // 50ms 满 → 末值
    cs.add("slider_coalesce_ledger", sc.stats() == (1, 9), ""); // 1 应用 9 合并
    // 窗外新输入正常应用。
    sc.input(30, 200);
    cs.add("slider_next_window_applies", sc.tick(260) == 30, "");

    // ── 排空时长 ──
    let mut tte = TimeToEmpty::new();
    cs.add("tte_insufficient_none", tte.minutes_left(800).is_none(), "");
    tte.sample(100);
    tte.sample(100);
    cs.add("tte_two_still_none", tte.minutes_left(800).is_none(), "");
    tte.sample(100);
    // 800‰ / 100‰·h⁻¹ = 8h = 480min。
    cs.add("tte_480min", tte.minutes_left(800) == Some(480), "");
    // 零率不猜。
    let mut tte2 = TimeToEmpty::new();
    for _ in 0..3 {
        tte2.sample(0);
    }
    cs.add("tte_zero_rate_none", tte2.minutes_left(800).is_none(), "");
    // 满环滚动：8×200 后 100 挤掉最旧 → 均值 175（100+7×200 = 1500/8 = 187）。
    let mut tte3 = TimeToEmpty::new();
    for _ in 0..8 {
        tte3.sample(200);
    }
    tte3.sample(100);
    // (200×7 + 100)/8 = 187.5 → 187。
    cs.add("tte_ring_rolls", tte3.minutes_left(1870) == Some(600), ""); // 1870×60/187 = 600

    cs
}

#[cfg(test)]
mod deep3_tests {
    use super::*;

    #[test]
    fn blink_duty_cycle_alternates() {
        let mut bl = AttentionBlink::new();
        bl.request(0);
        // 前两个循环逐半周期验证。
        assert!(bl.lit_at(0));
        assert!(!bl.lit_at(BLINK_HALF_MS));
        assert!(bl.lit_at(BLINK_HALF_MS * 2));
        assert!(!bl.lit_at(BLINK_HALF_MS * 3));
    }

    #[test]
    fn slider_values_clamped_to_100() {
        let mut sc = SliderCoalesce::new(0);
        sc.input(250, 0);
        assert_eq!(sc.tick(SLIDER_DEBOUNCE_MS), 100, "越界钳 100");
    }

    #[test]
    fn tte_monotonic_in_soc() {
        let mut t = TimeToEmpty::new();
        for _ in 0..3 {
            t.sample(150);
        }
        let a = t.minutes_left(300).unwrap();
        let b = t.minutes_left(900).unwrap();
        assert!(b > a);
    }
}

// ===========================================================================
// v4 深化批（F075 · G-C-05）——托盘右键菜单模型 / 免打扰窗
// ---------------------------------------------------------------------------
// 深化范围（仍属主册 G-C-05 功能定义的实装细化，非新立项）：
// 1. TrayMenuModel —— 托盘图标右键菜单模型：菜单结构（打开主窗/
//    设置/退出三标准项 + 应用自定义项 ≤4）+ 键盘序确定性构建
//    （「一个系统一套规则」的菜单词典面）。
// 2. DoNotDisturb —— 免打扰窗：时段表（默认 22-8 点）内抑制关注
//    动画与音效（事件照记——抑制的是打扰不是信息）。
// 全部零堆：定长表，无 Vec/String/浮点/format!。
// ===========================================================================

/// 自定义菜单项上限。
pub const MENU_CUSTOM_CAP: usize = 4;
/// 标准菜单项数。
pub const MENU_STANDARD: usize = 3;

// ---------------------------------------------------------------------------
// 深化一：托盘右键菜单模型
// ---------------------------------------------------------------------------

/// 菜单项类别（标准三件）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StdMenuItem {
    OpenMain,
    Settings,
    Exit,
}

impl StdMenuItem {
    /// 菜单显示序（打开 → 设置 → 分隔 → 退出——词典序）。
    pub fn order(self) -> usize {
        match self {
            StdMenuItem::OpenMain => 0,
            StdMenuItem::Settings => 1,
            StdMenuItem::Exit => 3, // 2 留给分隔位——词典固定结构
        }
    }
}

/// 菜单行（构建输出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MenuRow {
    pub kind: u8, // 0 标准打开 1 标准设置 2 分隔 3 标准退出 4 自定义
    pub custom_idx: u8,
}

/// 菜单模型。
pub struct TrayMenuModel {
    custom: [Option<&'static str>; MENU_CUSTOM_CAP],
    n: usize,
    built: u64,
}

impl TrayMenuModel {
    pub const fn new() -> Self {
        TrayMenuModel { custom: [None; MENU_CUSTOM_CAP], n: 0, built: 0 }
    }

    /// 声明自定义项（≤4，超容拒绝——菜单膨胀防线）。
    pub fn declare(&mut self, label: &'static str) -> bool {
        if self.n >= MENU_CUSTOM_CAP {
            return false;
        }
        self.custom[self.n] = Some(label);
        self.n += 1;
        true
    }

    /// 构建菜单行（键盘序确定性：标准固定结构 + 自定义项插在设置与退出之间）。
    pub fn build(&mut self, out: &mut [MenuRow]) -> usize {
        let mut w = 0usize;
        let put = |out: &mut [MenuRow], w: &mut usize, kind: u8, idx: u8| {
            if *w < out.len() {
                out[*w] = MenuRow { kind, custom_idx: idx };
                *w += 1;
            }
        };
        put(out, &mut w, 0, 0); // 打开主窗
        put(out, &mut w, 1, 0); // 设置
        for k in 0..self.n {
            put(out, &mut w, 4, k as u8); // 自定义项
        }
        put(out, &mut w, 2, 0); // 分隔
        put(out, &mut w, 3, 0); // 退出（尾项——退出后无项，词典不设尾分隔）
        self.built += 1;
        w
    }

    pub fn custom_count(&self) -> usize {
        self.n
    }

    pub fn built(&self) -> u64 {
        self.built
    }
}

// ---------------------------------------------------------------------------
// 深化二：免打扰窗
// ---------------------------------------------------------------------------

/// 免打扰窗（默认 22-8 点；窗跨午夜——两段式判断）。
pub struct DoNotDisturb {
    start_hour: u32,
    end_hour: u32,
    suppressed_events: u64,
    enabled: bool,
}

impl DoNotDisturb {
    pub const fn new() -> Self {
        DoNotDisturb { start_hour: 22, end_hour: 8, suppressed_events: 0, enabled: true }
    }

    /// 自定义窗（支持跨午夜：start > end 视为跨午夜窗）。
    pub fn set_window(&mut self, start: u32, end: u32) -> bool {
        if start > 23 || end > 23 {
            return false;
        }
        self.start_hour = start;
        self.end_hour = end;
        true
    }

    pub fn set_enabled(&mut self, on: bool) {
        self.enabled = on;
    }

    /// 该小时是否在免打扰窗内。
    pub fn in_window(&self, hour: u32) -> bool {
        let h = hour % 24;
        if self.start_hour <= self.end_hour {
            h >= self.start_hour && h < self.end_hour
        } else {
            // 跨午夜：22-8 → h≥22 或 h<8。
            h >= self.start_hour || h < self.end_hour
        }
    }

    /// 事件闸：窗内抑制（返回 false = 不打扰但已记账）。
    pub fn allow(&mut self, hour: u32) -> bool {
        if self.enabled && self.in_window(hour) {
            self.suppressed_events += 1;
            return false;
        }
        true
    }

    pub fn suppressed(&self) -> u64 {
        self.suppressed_events
    }
}

// ---------------------------------------------------------------------------
// v4 批自检
// ---------------------------------------------------------------------------

/// v4 批自检：菜单模型 / 免打扰逐条实摆。
pub fn run_trayhub_deep4_checks() -> CheckSet {
    let mut cs = CheckSet::new("F075-trayhub-v4");

    // ── 菜单模型 ──
    let mut tm = TrayMenuModel::new();
    let _ = tm.declare("暂停同步");
    let _ = tm.declare("立即同步");
    let mut rows = [MenuRow { kind: 0, custom_idx: 0 }; 12];
    let n = tm.build(&mut rows);
    cs.add("menu_row_count", n == 3 + 2 + 1, ""); // 3 标准 + 2 自定义 + 1 分隔
    cs.add(
        "menu_dictionary_order",
        rows[0].kind == 0 && rows[1].kind == 1 && rows[2].kind == 4
            && rows[4].kind == 2 && rows[5].kind == 3,
        "",
    );
    cs.add("menu_custom_idx_kept", rows[2].custom_idx == 0 && rows[3].custom_idx == 1, "");
    // 超容拒绝。
    let mut tm2 = TrayMenuModel::new();
    for _ in 0..MENU_CUSTOM_CAP {
        let _ = tm2.declare("x");
    }
    cs.add("menu_custom_cap", !tm2.declare("第5项"), "");
    // 无自定义项时词典结构完整（分隔仍在）。
    let mut tm3 = TrayMenuModel::new();
    let n3 = tm3.build(&mut rows);
    cs.add("menu_no_custom_full_structure", n3 == 4 && rows[2].kind == 2 && rows[3].kind == 3, "");
    cs.add("menu_build_ledger", tm3.built() == 1, "");

    // ── 免打扰 ──
    let mut dnd = DoNotDisturb::new();
    cs.add("dnd_night_suppressed", !dnd.allow(23), "");
    cs.add("dnd_day_allowed", dnd.allow(12), "");
    cs.add("dnd_morning_edge", !dnd.allow(7), ""); // 8 点前仍窗内
    cs.add("dnd_boundary_start", !dnd.allow(22), ""); // 22 点起（含）
    cs.add("dnd_boundary_end", dnd.allow(8), ""); // 8 点止（不含）
    cs.add("dnd_suppressed_ledger", dnd.suppressed() == 3, "");
    // 自定义跨午夜窗（2-6 点午睡）。
    let mut dnd2 = DoNotDisturb::new();
    let _ = dnd2.set_window(2, 6);
    cs.add(
        "dnd_custom_window",
        !dnd2.allow(3) && !dnd2.allow(5) && dnd2.allow(6) && dnd2.allow(1),
        "",
    );
    // 非法窗拒绝。
    cs.add("dnd_bad_window_refused", !dnd2.set_window(25, 3), "");
    // 总闸关闭全放行。
    let mut dnd3 = DoNotDisturb::new();
    dnd3.set_enabled(false);
    cs.add("dnd_master_off", dnd3.allow(23) && dnd3.suppressed() == 0, "");

    cs
}

#[cfg(test)]
mod deep4_tests {
    use super::*;

    #[test]
    fn menu_custom_labels_ordered() {
        let mut tm = TrayMenuModel::new();
        let _ = tm.declare("alpha");
        let _ = tm.declare("beta");
        let mut rows = [MenuRow { kind: 0, custom_idx: 0 }; 12];
        let _ = tm.build(&mut rows);
        // 自定义项按声明序（先 alpha 后 beta）。
        let customs: Vec<u8> = rows.iter().filter(|r| r.kind == 4).map(|r| r.custom_idx).collect();
        assert_eq!(customs, vec![0, 1], "声明序 = 显示序（构建确定性）");
    }

    #[test]
    fn dnd_window_within_same_day() {
        let mut d = DoNotDisturb::new();
        let _ = d.set_window(12, 14);
        assert!(d.in_window(12));
        assert!(d.in_window(13));
        assert!(!d.in_window(14));
        assert!(!d.in_window(11));
    }
}

// ===========================================================================
// v5 深化批（deep5）：快捷键注册表 + 系统事件桥接账
// ===========================================================================

// ---------------------------------------------------------------------------
// 深化一：菜单快捷键注册表（冲突检测——同组合键拒绝二注册）
// ---------------------------------------------------------------------------

/// 组合键 = (修饰位图, 键码)。修饰：1=Ctrl 2=Alt 4=Shift 8=Super。
pub type Hotkey = (u8, u8);

/// 快捷键注册表：8 槽，冲突拒绝。
pub struct HotkeyRegistry {
    taken: [Option<(Hotkey, u16)>; 8], // (组合键, 命令 id)
    n: usize,
    conflicts: u32,
}

impl HotkeyRegistry {
    pub const fn new() -> Self {
        HotkeyRegistry { taken: [None; 8], n: 0, conflicts: 0 }
    }

    /// 注册（返回 false = 冲突或满）。
    pub fn register(&mut self, hk: Hotkey, cmd: u16) -> bool {
        for s in self.taken.iter() {
            if let Some((k, _)) = s {
                if *k == hk {
                    self.conflicts += 1;
                    return false;
                }
            }
        }
        if self.n >= 8 {
            return false;
        }
        self.taken[self.n] = Some((hk, cmd));
        self.n += 1;
        true
    }

    /// 按键 → 命令（精确匹配）。
    pub fn lookup(&self, hk: Hotkey) -> Option<u16> {
        for s in self.taken.iter() {
            if let Some((k, c)) = s {
                if *k == hk {
                    return Some(*c);
                }
            }
        }
        None
    }

    /// 注销。
    pub fn unregister(&mut self, hk: Hotkey) -> bool {
        for k in 0..self.n {
            if let Some((key, _)) = self.taken[k] {
                if key == hk {
                    self.n -= 1;
                    self.taken[k] = self.taken[self.n];
                    self.taken[self.n] = None;
                    return true;
                }
            }
        }
        false
    }

    pub fn len(&self) -> usize {
        self.n
    }

    pub fn conflicts(&self) -> u32 {
        self.conflicts
    }
}

// ---------------------------------------------------------------------------
// 深化二：系统事件桥接账（托盘 ← 系统事件：分类计数 + 丢弃保护）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SysEvent {
    ThemeChanged,
    DisplayChanged,
    PowerSource,
    TimeChanged,
}

/// 事件桥：分类计数 + 满保护（32 槽环——满时丢最旧并计数）。
pub struct EventBridge {
    ring: [(SysEvent, u32); 32], // (事件, 秒级时间戳)
    pos: usize,
    filled: usize,
    delivered: u64,
    dropped: u64,
}

impl EventBridge {
    pub const fn new() -> Self {
        EventBridge { ring: [(SysEvent::ThemeChanged, 0); 32], pos: 0, filled: 0, delivered: 0, dropped: 0 }
    }

    /// 事件入桥（满 → 挤最旧）。
    pub fn push(&mut self, ev: SysEvent, ts: u32) {
        if self.filled < 32 {
            self.filled += 1;
        } else {
            self.dropped += 1; // 环满挤最旧——旧事件显性化丢弃
        }
        self.ring[self.pos] = (ev, ts);
        self.pos = (self.pos + 1) % 32;
        self.delivered += 1;
    }

    /// 最近同类事件的时间戳（重放判定用——同事件 1s 内重复则忽略）。
    pub fn last_ts_of(&self, ev: SysEvent) -> Option<u32> {
        for k in 0..self.filled {
            let idx = (self.pos + 32 - 1 - k) % 32;
            if self.ring[idx].0 == ev {
                return Some(self.ring[idx].1);
            }
        }
        None
    }

    /// 事件去抖：同事件 1s 内 → 丢弃新事件。
    pub fn push_debounced(&mut self, ev: SysEvent, ts: u32) -> bool {
        match self.last_ts_of(ev) {
            Some(t) if ts.saturating_sub(t) < 1 => false,
            _ => {
                self.push(ev, ts);
                true
            }
        }
    }

    pub fn stats(&self) -> (u64, u64) {
        (self.delivered, self.dropped)
    }
}

// ---------------------------------------------------------------------------
// deep5 检查项
// ---------------------------------------------------------------------------

pub fn run_trayhub_deep5_checks() -> CheckSet {
    let mut cs = CheckSet::new("F075-trayhub-v5");

    // ── 快捷键 ──
    // 1) 注册 + 查找。
    let mut hr = HotkeyRegistry::new();
    let _ = hr.register((1, b'E'), 100); // Ctrl+E
    cs.add("hotkey_register_lookup", hr.lookup((1, b'E')) == Some(100), "");
    // 2) 冲突拒绝（同键不同命令）。
    cs.add("hotkey_conflict_refused", !hr.register((1, b'E'), 200) && hr.conflicts() == 1, "");
    // 3) 修饰不同不冲突（Ctrl+E vs Alt+E）。
    cs.add("hotkey_modifier_distinct", hr.register((2, b'E'), 300) && hr.lookup((2, b'E')) == Some(300), "");
    // 4) 注销后可重注册。
    let _ = hr.unregister((1, b'E'));
    cs.add("hotkey_unregister_frees", hr.register((1, b'E'), 400) && hr.lookup((1, b'E')) == Some(400), "");
    // 5) 未注册查无。
    cs.add("hotkey_unknown_none", hr.lookup((8, b'X')).is_none(), "");
    // 6) 满 8 拒绝。
    let mut hr2 = HotkeyRegistry::new();
    let mut all_ok = true;
    for k in 0..10u8 {
        all_ok &= hr2.register((1, k), k as u16);
    }
    cs.add("hotkey_cap_8", !all_ok && hr2.len() == 8, "");

    // ── 事件桥 ──
    // 7) 入桥计数 + 最近事件查询。
    let mut eb = EventBridge::new();
    eb.push(SysEvent::ThemeChanged, 100);
    eb.push(SysEvent::PowerSource, 200);
    cs.add("bridge_last_ts", eb.last_ts_of(SysEvent::ThemeChanged) == Some(100) && eb.stats().0 == 2, "");
    // 8) 去抖：1s 内重复丢弃。
    cs.add("bridge_debounce", !eb.push_debounced(SysEvent::PowerSource, 200) && eb.push_debounced(SysEvent::PowerSource, 500), "");
    // 9) 不同事件不去抖。
    cs.add("bridge_other_event_passes", eb.push_debounced(SysEvent::TimeChanged, 250), "");
    // 10) 环满挤旧并计数。
    let mut eb2 = EventBridge::new();
    for k in 0..40u32 {
        eb2.push(SysEvent::DisplayChanged, k);
    }
    cs.add("bridge_ring_full_drops", eb2.stats() == (40, 8) && eb2.last_ts_of(SysEvent::DisplayChanged) == Some(39), "");

    cs
}

#[cfg(test)]
mod deep5_tests {
    use super::*;

    #[test]
    fn hotkey_lifecycle_complete() {
        let mut hr = HotkeyRegistry::new();
        let _ = hr.register((3, b'K'), 42);
        assert_eq!(hr.lookup((3, b'K')), Some(42));
        assert!(hr.unregister((3, b'K')));
        assert!(!hr.unregister((3, b'K')), "二次注销失败");
        assert_eq!(hr.lookup((3, b'K')), None);
    }

    #[test]
    fn bridge_debounce_window() {
        let mut eb = EventBridge::new();
        assert!(eb.push_debounced(SysEvent::ThemeChanged, 1000));
        assert!(!eb.push_debounced(SysEvent::ThemeChanged, 1000), "同秒抖动");
        assert!(!eb.push_debounced(SysEvent::ThemeChanged, 1000), "同秒抖动 2");
        assert!(eb.push_debounced(SysEvent::ThemeChanged, 1001), "过 1s 放行");
        assert_eq!(eb.stats().0, 2);
    }
}
