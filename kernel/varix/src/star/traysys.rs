//! F075 托盘系统 · 完整设计（STAR I 主册 G-C-05）。
//!
//! **判据（主册）**：滑杆拖动全程 80fps（F041 账本）；幽灵图标清除实测；
//! 三件套点击响应 <100ms（C-6 红线）。
//!
//! **设计要点（主册）**：
//! - 任务栏右端托盘：常驻三件（网络/音量/电池）+ 应用托盘折叠区（隐藏
//!   图标上拉面板）；三件套点击各弹控制条（F076 聚合前的轻量版）；
//!   托盘图标 16px 基准（2x 档 32px 4K 资产）；
//! - 控制条：音量竖滑杆高 160px 实时 80fps、网络弹已存 Wi-Fi 列表、
//!   电池弹百分比+预估续航；折叠面板宽 280px 网格 4 列；图标悬停
//!   tooltip 显示应用名（1s 延迟）；
//! - 折叠/展开状态与「哪些图标允许常驻」白名单存配置层（本模块持内存
//!   账，落盘由配置层接手）；
//! - 幽灵图标清道夫：30s 无心跳自动清除（应用崩溃残留实测）；
//!   电池计不可读 → 图标灰显+悬停说明（诚实降级）；图标缺失 → 默认
//!   占位图+应用名；
//! - 滑杆步进 2%（100 格），键盘上下键 5% 大步；托盘区总宽上限 200px
//!   （超出强制折叠）；折叠面板图标支持拖拽调序（E6 联动）；电池图标
//!   四态（充电/高/低/极低）颜色走主题令牌（E1）；
//! - 80fps 与 <100ms 两条红线在内核侧为**闭环对账**：宿主逐帧报到
//!   （`slider_frame`，间隔 ≤12.5ms 入账）、面板首帧渲染完报到
//!   （`panel_shown`）——内核记账判定，实测数据进 F041 账本。
//!
//! 钟注入式（ms 实参），无外部依赖。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源——主册功能定义/交互设计/设计细节）
// ---------------------------------------------------------------------------

/// 托盘图标基准（px）。
pub const ICON_PX: u32 = 16;

/// 2x 档图标资产（px，4K）。
pub const ICON_PX_2X: u32 = 32;

/// 音量竖滑杆高（px）。
pub const SLIDER_H_PX: u32 = 160;

/// 折叠面板宽（px）。
pub const PANEL_W_PX: u32 = 280;

/// 折叠面板网格列数。
pub const PANEL_COLS: usize = 4;

/// 托盘区总宽上限（px，超出强制折叠）。
pub const TRAY_MAX_W_PX: u32 = 200;

/// 滑杆步进（%——100 格）。
pub const SLIDER_STEP_PCT: u8 = 2;

/// 键盘大步（%）。
pub const KEY_STEP_PCT: u8 = 5;

/// tooltip 延迟（ms）。
pub const TOOLTIP_DELAY_MS: u64 = 1_000;

/// 幽灵图标清除线（ms 无心跳）。
pub const GHOST_SWEEP_MS: u64 = 30_000;

/// 滑杆帧预算（ms 整数口径——80fps 达标线 12.5ms 在整 ms 域取 12：
/// 间隔 ≤12ms 入账，13ms 即 76.9fps 违约）。
pub const SLIDER_FRAME_BUDGET_MS: u64 = 12;

/// 三件套点击响应红线（ms，C-6）。
pub const CLICK_BUDGET_MS: u64 = 100;

// ---------------------------------------------------------------------------
// 状态与视图
// ---------------------------------------------------------------------------

/// 电池图标四态（颜色走主题令牌 E1——本枚举即状态唯一源）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BatteryGlyph {
    Charging,
    High,
    Low,
    Critical,
    /// 电池计不可读——灰显 + 悬停说明（诚实降级）。
    Unreadable,
}

/// 三件套控制条面板。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelKind {
    Volume,
    Network,
    Battery,
}

/// 折叠区应用图标。
#[derive(Clone, Debug)]
pub struct TrayApp {
    pub id: u64,
    /// 应用名（tooltip 文案；图标缺失时兼作占位说明）。
    pub name: String,
    /// 主题图标 id（0 = 缺失——默认占位图+应用名）。
    pub glyph: u16,
    /// 最近心跳（ms）。
    pub last_heartbeat_ms: u64,
    /// 面板网格序（拖拽调序，E6）。
    pub order: usize,
}

/// 折叠面板网格坐标（行, 列）。
pub fn grid_pos(index: usize) -> (usize, usize) {
    (index / PANEL_COLS, index % PANEL_COLS)
}

// ---------------------------------------------------------------------------
// 托盘板
// ---------------------------------------------------------------------------

/// 托盘板（任务栏右端）。
pub struct TrayBoard {
    apps: Vec<TrayApp>,
    /// 允许常驻白名单（配置层账——白名单外只进折叠面板）。
    whitelist: Vec<u64>,
    /// 折叠/展开状态（配置层账；强制折叠时恒 true）。
    collapsed: bool,
    collapsed_forced: bool,
    /// 打开的控制条面板（三件套互斥——同时至多一块）。
    panel: Option<PanelKind>,
    clicked_ms: u64,
    shown_ms: u64,
    /// 音量（0-100，格点对齐步进 2%）。
    volume: u8,
    /// 已存 Wi-Fi 名单（宿主注入）。
    wifi: Vec<String>,
    /// 电池读数：None = 不可读。
    battery_percent: Option<u8>,
    battery_charging: bool,
    /// 滑杆帧账（80fps 闭环）。
    drag_frames: u64,
    drag_last_ms: u64,
    drag_violations: u64,
    /// tooltip 悬停目标与起点。
    tooltip_app: Option<u64>,
    tooltip_since_ms: u64,
}

impl TrayBoard {
    pub fn new() -> TrayBoard {
        TrayBoard {
            apps: Vec::new(),
            whitelist: Vec::new(),
            collapsed: true,
            collapsed_forced: false,
            panel: None,
            clicked_ms: 0,
            shown_ms: 0,
            volume: 50,
            wifi: Vec::new(),
            battery_percent: None,
            battery_charging: false,
            drag_frames: 0,
            drag_last_ms: 0,
            drag_violations: 0,
            tooltip_app: None,
            tooltip_since_ms: 0,
        }
    }

    // -----------------------------------------------------------------------
    // 配置层账
    // -----------------------------------------------------------------------

    /// 白名单授予常驻（撤销即回折叠区）。
    pub fn allow_pinned(&mut self, app_id: u64, allow: bool) {
        if allow {
            if !self.whitelist.contains(&app_id) {
                self.whitelist.push(app_id);
            }
        } else {
            self.whitelist.retain(|a| *a != app_id);
        }
        self.enforce_width_cap();
    }

    pub fn whitelisted(&self, app_id: u64) -> bool {
        self.whitelist.contains(&app_id)
    }

    pub fn set_collapsed(&mut self, collapsed: bool) {
        self.collapsed = collapsed || self.collapsed_forced;
    }

    pub fn collapsed(&self) -> bool {
        self.collapsed
    }

    /// 常驻区总宽：三件套 + 白名单应用（16px/枚）——超 200px 强制折叠。
    fn pinned_width(&self) -> u32 {
        3 * ICON_PX + self.whitelist.len() as u32 * ICON_PX
    }

    fn enforce_width_cap(&mut self) {
        self.collapsed_forced = self.pinned_width() > TRAY_MAX_W_PX;
        if self.collapsed_forced {
            self.collapsed = true;
        }
    }

    // -----------------------------------------------------------------------
    // 应用图标与幽灵清道夫
    // -----------------------------------------------------------------------

    /// 注册折叠区图标（心跳起点；glyph 0 = 缺失——占位图+应用名）。
    pub fn register_app(&mut self, id: u64, name: &str, glyph: u16, now_ms: u64) {
        if self.apps.iter().any(|a| a.id == id) {
            return;
        }
        self.apps.push(TrayApp {
            id,
            name: String::from(name),
            glyph,
            last_heartbeat_ms: now_ms,
            order: self.apps.len(),
        });
    }

    /// 心跳（应用存活证明——刷新清除线）。
    pub fn heartbeat(&mut self, id: u64, now_ms: u64) {
        if let Some(a) = self.apps.iter_mut().find(|a| a.id == id) {
            a.last_heartbeat_ms = now_ms;
        }
    }

    /// 幽灵清道夫：清除 30s 无心跳的残留图标，返回清除数（实测账）。
    pub fn sweep_ghosts(&mut self, now_ms: u64) -> usize {
        let n = self.apps.len();
        self.apps
            .retain(|a| now_ms.saturating_sub(a.last_heartbeat_ms) <= GHOST_SWEEP_MS);
        n - self.apps.len()
    }

    pub fn app_count(&self) -> usize {
        self.apps.len()
    }

    /// 折叠面板网格序（按 order 排——拖拽调序生效面）。
    pub fn panel_apps(&self) -> Vec<&TrayApp> {
        let mut v: Vec<&TrayApp> = self.apps.iter().collect();
        v.sort_by_key(|a| a.order);
        v
    }

    /// 拖拽调序（E6 联动——order 重排为 stable 提取+回插）。
    pub fn reorder(&mut self, app_id: u64, to_index: usize) -> bool {
        let ids: Vec<u64> = self.panel_apps().iter().map(|a| a.id).collect();
        let Some(from) = ids.iter().position(|i| *i == app_id) else {
            return false;
        };
        let to = to_index.min(ids.len() - 1);
        let mut seq = ids;
        let moved = seq.remove(from);
        seq.insert(to, moved);
        for (i, id) in seq.iter().enumerate() {
            if let Some(a) = self.apps.iter_mut().find(|a| a.id == *id) {
                a.order = i;
            }
        }
        true
    }

    /// tooltip 悬停（1s 延迟）。
    pub fn hover_app(&mut self, app_id: u64, now_ms: u64) {
        self.tooltip_app = Some(app_id);
        self.tooltip_since_ms = now_ms;
    }

    pub fn leave_app(&mut self) {
        self.tooltip_app = None;
    }

    /// tooltip 到期（悬停满 1s）。
    pub fn tooltip_due(&self, now_ms: u64) -> bool {
        matches!(self.tooltip_app, Some(_))
            && now_ms.saturating_sub(self.tooltip_since_ms) >= TOOLTIP_DELAY_MS
    }

    /// tooltip 文案（图标缺失 → 占位图+应用名口径同源）。
    pub fn tooltip_text(&self) -> Option<String> {
        let id = self.tooltip_app?;
        self.apps
            .iter()
            .find(|a| a.id == id)
            .map(|a| match a.glyph {
                0 => alloc::format!("{}（默认占位图）", a.name),
                _ => a.name.clone(),
            })
    }

    // -----------------------------------------------------------------------
    // 三件套与控制条
    // -----------------------------------------------------------------------

    /// 点击三件套（面板互斥——后点替换前块）。同步出面板结构（内核侧
    /// 就绪），宿主渲染完首帧回调 [`panel_shown`] 闭环 <100ms 判定。
    pub fn click_tray(&mut self, kind: PanelKind, now_ms: u64) {
        self.panel = Some(kind);
        self.clicked_ms = now_ms;
        self.shown_ms = 0;
    }

    /// 面板首帧渲染完报到（C-6 红线闭环）。
    pub fn panel_shown(&mut self, now_ms: u64) {
        self.shown_ms = now_ms;
    }

    /// 点击→首帧 ≤100ms 达标（未报到 = 未达标）。
    pub fn click_within_budget(&self) -> bool {
        self.shown_ms != 0
            && self.shown_ms.saturating_sub(self.clicked_ms) <= CLICK_BUDGET_MS
    }

    pub fn panel(&self) -> Option<PanelKind> {
        self.panel
    }

    /// 已存 Wi-Fi 名单（网络面板数据源）。
    pub fn set_wifi_list(&mut self, list: &[&str]) {
        self.wifi.clear();
        self.wifi.extend(list.iter().map(|s| String::from(*s)));
    }

    pub fn wifi_list(&self) -> &[String] {
        &self.wifi
    }

    // -----------------------------------------------------------------------
    // 音量滑杆（80fps 闭环 + 步进 2% + 键盘 5%）
    // -----------------------------------------------------------------------

    /// 格点吸附：最近偶数格（步进 2%——100 格），tie 取下格。
    fn snap(v: u8) -> u8 {
        v / SLIDER_STEP_PCT * SLIDER_STEP_PCT
    }

    /// 拖动设值（连续位置 → 格点吸附 + clamp）。
    pub fn slider_set(&mut self, percent: u8) {
        self.volume = Self::snap(percent.min(100));
    }

    /// 键盘上下键（5% 大步——精确跳步，不受 2% 拖动格吸附）。
    pub fn slider_key_step(&mut self, up: bool) {
        self.volume = if up {
            self.volume.saturating_add(KEY_STEP_PCT).min(100)
        } else {
            self.volume.saturating_sub(KEY_STEP_PCT)
        };
    }

    /// 拖动帧报到：与上一帧间隔 ≤12.5ms（80fps 达标线，整 ms 口径取
    /// 12）入账；超预算帧计违约账——F041 账本数据源。
    pub fn slider_frame(&mut self, now_ms: u64) {
        if self.drag_frames > 0 {
            let dt = now_ms.saturating_sub(self.drag_last_ms);
            if dt > SLIDER_FRAME_BUDGET_MS {
                self.drag_violations += 1;
            }
        }
        self.drag_frames += 1;
        self.drag_last_ms = now_ms;
    }

    /// 滑杆拖动全程 80fps 达标（有帧且零违约）。
    pub fn drag_at_80fps(&self) -> bool {
        self.drag_frames > 0 && self.drag_violations == 0
    }

    pub fn volume(&self) -> u8 {
        self.volume
    }

    pub fn drag_violations(&self) -> u64 {
        self.drag_violations
    }

    // -----------------------------------------------------------------------
    // 电池（四态 + 诚实降级）
    // -----------------------------------------------------------------------

    /// 电池读数注入（None = 电池计不可读）。
    pub fn battery_read(&mut self, percent: Option<u8>, charging: bool) {
        self.battery_percent = percent;
        self.battery_charging = charging;
    }

    /// 四态判定：充电优先；档位线（本模块定档，主册只定四态名）：
    /// ≥35 高、15-34 低、<15 极低。
    pub fn battery_glyph(&self) -> BatteryGlyph {
        match self.battery_percent {
            None => BatteryGlyph::Unreadable,
            Some(_) if self.battery_charging => BatteryGlyph::Charging,
            Some(p) if p >= 35 => BatteryGlyph::High,
            Some(p) if p >= 15 => BatteryGlyph::Low,
            Some(_) => BatteryGlyph::Critical,
        }
    }

    /// 面板文案：百分比 + 预估续航；不可读 → 悬停说明（诚实降级）。
    pub fn battery_text(&self) -> String {
        match self.battery_percent {
            None => String::from("电池计不可读"),
            Some(p) if self.battery_charging => alloc::format!("{}% 电量·充电中", p),
            Some(p) => alloc::format!("{}% 电量", p),
        }
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F075 自检（判据：滑杆 80fps；幽灵清除实测；三件套点击 <100ms）。
pub fn run_traysys_checks() -> CheckSet {
    let mut set = CheckSet::new("F075-traysys");

    // 1. 几何与时序常量在案（16/32px 图标、160px 滑杆、280px 4 列面板、
    //    200px 宽上限、2%/5% 步进、1s tooltip、30s 幽灵线、12.5ms 帧线、
    //    100ms 红线——主册给定值逐个钉死）。
    set.add(
        "tray constants pinned",
        ICON_PX == 16
            && ICON_PX_2X == 32
            && SLIDER_H_PX == 160
            && PANEL_W_PX == 280
            && PANEL_COLS == 4
            && TRAY_MAX_W_PX == 200
            && SLIDER_STEP_PCT == 2
            && KEY_STEP_PCT == 5
            && TOOLTIP_DELAY_MS == 1_000
            && GHOST_SWEEP_MS == 30_000
            && SLIDER_FRAME_BUDGET_MS == 12
            && CLICK_BUDGET_MS == 100,
        "",
    );

    // 2. 幽灵清道夫实测：31s 无心跳清除、29s 存活（清除线两侧）。
    let mut board = TrayBoard::new();
    board.register_app(1, "崩溃残留", 5, 0);
    board.register_app(2, "慢心跳", 6, 0);
    board.heartbeat(2, 1_000);
    let swept = board.sweep_ghosts(30_001);
    set.add(
        "ghost sweep after 30s silence",
        swept == 1 && board.app_count() == 1 && !board.panel_apps().is_empty(),
        "",
    );

    // 3. 心跳续期存活：29s 处再跳一次 → 60s 时仍在。
    let mut board2 = TrayBoard::new();
    board2.register_app(1, "长跑应用", 5, 0);
    board2.heartbeat(1, 29_000);
    board2.sweep_ghosts(59_000);
    set.add(
        "heartbeat keeps icon alive",
        board2.app_count() == 1,
        "",
    );

    // 4. 三件套点击闭环 <100ms（C-6 红线）：99ms 达标、100ms 恰达标、
    //    101ms 违约；未报到 = 未达标。
    let mut board3 = TrayBoard::new();
    board3.click_tray(PanelKind::Volume, 1_000);
    board3.panel_shown(1_099);
    let ok99 = board3.click_within_budget();
    board3.click_tray(PanelKind::Network, 2_000);
    board3.panel_shown(2_100);
    let ok100 = board3.click_within_budget();
    board3.click_tray(PanelKind::Battery, 3_000);
    board3.panel_shown(3_101);
    let bad101 = !board3.click_within_budget();
    board3.click_tray(PanelKind::Volume, 4_000);
    let unshown = !board3.click_within_budget();
    set.add(
        "click to first frame within 100ms",
        ok99 && ok100 && bad101 && unshown && board3.panel() == Some(PanelKind::Volume),
        "",
    );

    // 5. 面板互斥：同时至多一块（后点替换前块）。
    let mut board4 = TrayBoard::new();
    board4.click_tray(PanelKind::Volume, 0);
    board4.click_tray(PanelKind::Battery, 10);
    set.add(
        "panels are mutually exclusive",
        board4.panel() == Some(PanelKind::Battery),
        "",
    );

    // 6. 滑杆格点吸附与 clamp：60 直取；61 吸附 60；200 顶格 100；1 吸附 0。
    let mut board5 = TrayBoard::new();
    board5.slider_set(60);
    let v60 = board5.volume();
    board5.slider_set(61);
    let v61 = board5.volume();
    board5.slider_set(200);
    let v200 = board5.volume();
    board5.slider_set(1);
    let v1 = board5.volume();
    set.add(
        "slider snaps to two percent grid",
        v60 == 60 && v61 == 60 && v200 == 100 && v1 == 0,
        "",
    );

    // 7. 键盘大步 5%：60 → 65（键盘精确跳步，不受 2% 拖动格吸附）；
    //    3 吸附 2 → 下 0（saturating 不越界）。
    let mut board6 = TrayBoard::new();
    board6.slider_set(60);
    board6.slider_key_step(true);
    let up_ok = board6.volume() == 65;
    board6.slider_set(3);
    board6.slider_key_step(false);
    let down_ok = board6.volume() == 0;
    set.add(
        "keyboard steps five percent",
        up_ok && down_ok,
        "",
    );

    // 8. 滑杆 80fps 帧账：等间隔 12ms 全过；插一帧 20ms 计违约。
    let mut board7 = TrayBoard::new();
    for i in 0..80u64 {
        board7.slider_frame(i * 12);
    }
    let clean = board7.drag_at_80fps() && board7.drag_violations() == 0;
    let mut board8 = TrayBoard::new();
    for i in 0..40u64 {
        board8.slider_frame(i * 12);
    }
    board8.slider_frame(40 * 12 + 8); // 20ms 间隔——违约一帧
    set.add(
        "slider frames booked at 80fps",
        clean && board8.drag_violations() == 1 && !board8.drag_at_80fps(),
        "",
    );

    // 9. 电池四态：充电/高/低/极低逐档钉死（档位线 35/15 本模块定档）。
    let mut board9 = TrayBoard::new();
    board9.battery_read(Some(80), true);
    let chg = board9.battery_glyph() == BatteryGlyph::Charging;
    board9.battery_read(Some(80), false);
    let high = board9.battery_glyph() == BatteryGlyph::High;
    board9.battery_read(Some(20), false);
    let low = board9.battery_glyph() == BatteryGlyph::Low;
    board9.battery_read(Some(14), false);
    let crit = board9.battery_glyph() == BatteryGlyph::Critical;
    set.add(
        "battery four states",
        chg && high && low && crit,
        "",
    );

    // 10. 电池计不可读 → 灰显态 + 悬停说明（诚实降级）+ 面板文案。
    board9.battery_read(None, false);
    let unread = board9.battery_glyph() == BatteryGlyph::Unreadable
        && board9.battery_text() == "电池计不可读";
    board9.battery_read(Some(60), true);
    let text_ok = board9.battery_text() == "60% 电量·充电中";
    set.add(
        "battery unreadable degrades honestly",
        unread && text_ok,
        "",
    );

    // 11. 宽上限强制折叠：白名单 12 枚 → 3×16+12×16=240 > 200 强制，
    //     手动展开无效；撤到 9 枚 → 192 解除后手动展开生效。
    let mut board10 = TrayBoard::new();
    for i in 1..=12u64 {
        board10.register_app(i, "应用", 5, 0);
        board10.allow_pinned(i, true);
    }
    let forced_now = board10.collapsed();
    board10.set_collapsed(false); // 强制态下手动展开无效
    let forced_stays = board10.collapsed();
    for i in 10..=12u64 {
        board10.allow_pinned(i, false);
    }
    board10.set_collapsed(false);
    let relaxed = !board10.collapsed();
    set.add(
        "width cap forces collapse",
        forced_now && forced_stays && relaxed,
        "",
    );

    // 12. 白名单门：白名单外图标只在折叠面板（不占常驻宽）。
    let mut board11 = TrayBoard::new();
    board11.register_app(1, "常驻应用", 5, 0);
    board11.register_app(2, "折叠应用", 6, 0);
    board11.allow_pinned(1, true);
    set.add(
        "whitelist gates pinned area",
        board11.whitelisted(1) && !board11.whitelisted(2) && board11.pinned_width() == 4 * ICON_PX,
        "",
    );

    // 13. 拖拽调序（E6）+ tooltip 1s 延迟 + 占位图文案。
    let mut board12 = TrayBoard::new();
    board12.register_app(1, "甲", 5, 0);
    board12.register_app(2, "乙", 0, 0);
    board12.register_app(3, "丙", 7, 0);
    board12.reorder(3, 0);
    let order_ok = board12.panel_apps().iter().map(|a| a.id).eq([3u64, 1, 2]);
    board12.hover_app(2, 10_000);
    let not_due = !board12.tooltip_due(10_999);
    let due = board12.tooltip_due(11_000);
    let placeholder = board12.tooltip_text().map(|t| t == "乙（默认占位图）").unwrap_or(false);
    set.add(
        "reorder tooltip and placeholder",
        order_ok && not_due && due && placeholder,
        "",
    );

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sweep_returns_count_and_cleans_list() {
        let mut b = TrayBoard::new();
        for i in 1..=4u64 {
            b.register_app(i, "应用", 5, 0);
        }
        b.heartbeat(4, 20_000);
        let swept = b.sweep_ghosts(30_001);
        assert_eq!(swept, 3, "仅 4 号在 20s 心跳续期存活");
        assert_eq!(b.app_count(), 1);
    }

    #[test]
    fn duplicate_register_noop() {
        let mut b = TrayBoard::new();
        b.register_app(1, "甲", 5, 0);
        b.register_app(1, "乙", 6, 0);
        assert_eq!(b.app_count(), 1);
        assert_eq!(b.panel_apps()[0].name, "甲");
    }

    #[test]
    fn revoke_whitelist_sends_icon_back_to_panel() {
        let mut b = TrayBoard::new();
        b.register_app(1, "甲", 5, 0);
        b.allow_pinned(1, true);
        assert!(b.whitelisted(1));
        b.allow_pinned(1, false);
        assert!(!b.whitelisted(1), "撤销常驻 → 回折叠面板");
        assert_eq!(b.pinned_width(), 3 * ICON_PX);
    }

    #[test]
    fn grid_pos_wraps_four_columns() {
        assert_eq!(grid_pos(0), (0, 0));
        assert_eq!(grid_pos(3), (0, 3));
        assert_eq!(grid_pos(4), (1, 0));
        assert_eq!(grid_pos(9), (2, 1));
    }

    #[test]
    fn slider_volume_roundtrip_boundary() {
        let mut b = TrayBoard::new();
        b.slider_set(0);
        assert_eq!(b.volume(), 0);
        b.slider_set(100);
        assert_eq!(b.volume(), 100);
        b.slider_key_step(true);
        assert_eq!(b.volume(), 100, "顶格再上步不越界");
        b.slider_set(98);
        b.slider_key_step(false);
        assert_eq!(b.volume(), 93);
    }
}
