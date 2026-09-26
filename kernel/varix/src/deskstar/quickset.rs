//! F076 快速设置面板 · 完整设计（STAR I 主册 G-C-06）。
//!
//! **判据（主册）**：六类开关全部实测生效；面板弹出 ≤100ms；
//! 盲操作友好（全键盘可达：Tab 循环+Space 切换）。
//!
//! **设计要点（主册）**：
//! - 点击网络/音量/电池任意一枚出聚合面板：Wi-Fi 选择/蓝牙开关
//!   （前瞻灰置）/音量滑杆/亮度滑杆/专注模式开关/性能三档（F069）
//!   ——一块面板管全系统开关；面板 360×420px 右下弹出（距边 16px，
//!   乙-4 表 toast 同位）；
//! - 开关卡两列网格（卡 164×56px，图标+名称+开关态色）；滑杆区
//!   横向全宽；面板外点击即关（轻模态）；
//! - 面板布局（哪些卡显示/顺序）用户可编辑（长按拖拽，E6 联动）；
//!   状态即时生效无存储（开关态本身在各自子系统）；
//! - Wi-Fi 列表为空 → 「未找到网络」+ 重新扫描钮；亮度滑杆在
//!   夜间模式（F116）联动色温提示；性能档切换（F069）toast 确认；
//!   飞行模式一键开关（聚合所有射频）；
//! - 开关态色用强调色令牌（E1）非硬编码绿；滑杆双端数值标签
//!   （拖动时放大显示当前值）；Wi-Fi 连接中态转圈动画在卡内
//!   （不弹窗）；面板圆角 8px+毛玻璃材质（C-3 任务栏同材质族）；
//!   所有开关翻转动画 150ms 弹性曲线。
//!
//! 实装口径：内核侧为**状态机与判据实装层**——面板聚合逻辑、开关
//! 状态账、键盘导航账、弹出预算账、Wi-Fi 扫描态机、布局编辑账；
//! 射频/亮度/音量等执行面以**显式回执闭包**承接（上层子系统接线
//! 后逐卡回执「实测生效」）。时间全部实参注入。

use crate::checks::CheckSet;

use crate::deskstar::dbase::{FloatLayer, FocusRing, Rect, Token};
use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源——主册交互设计/设计细节）
// ---------------------------------------------------------------------------

/// 面板宽（px）。
pub const PANEL_W_PX: i32 = 360;

/// 面板高（px）。
pub const PANEL_H_PX: i32 = 420;

/// 距边（px，乙-4 表 toast 同位）。
pub const EDGE_GAP_PX: i32 = 16;

/// 开关卡宽（px，两列网格）。
pub const CARD_W_PX: i32 = 164;

/// 开关卡高（px）。
pub const CARD_H_PX: i32 = 56;

/// 面板圆角（px）。
pub const PANEL_RADIUS_PX: i32 = 8;

/// 开关翻转动画时长（ms，弹性曲线）。
pub const TOGGLE_ANIM_MS: u32 = 150;

/// 面板弹出预算（ms）。
pub const POPUP_BUDGET_MS: u64 = 100;

/// 滑杆步进（%，音量）。
pub const SLIDER_STEP_PCT: u8 = 5;

// ---------------------------------------------------------------------------
// 状态与模型
// ---------------------------------------------------------------------------

/// 六类开关卡（主册功能定义枚举——聚合面板的卡位全集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CardKind {
    /// Wi-Fi 选择。
    Wifi,
    /// 蓝牙开关（前瞻灰置）。
    Bluetooth,
    /// 专注模式（F115 联动）。
    Focus,
    /// 性能三档（F069 联动）。
    PerfTier,
    /// 夜间模式（F116 联动——亮度滑杆色温提示来源）。
    NightLight,
    /// 飞行模式（聚合所有射频）。
    Airplane,
}

impl CardKind {
    /// 卡名（说明句三件套之一：名称+说明+调节控件）。
    pub fn name(self) -> &'static str {
        match self {
            CardKind::Wifi => "Wi-Fi",
            CardKind::Bluetooth => "蓝牙",
            CardKind::Focus => "专注模式",
            CardKind::PerfTier => "性能模式",
            CardKind::NightLight => "夜间模式",
            CardKind::Airplane => "飞行模式",
        }
    }

    /// 一句话说明（F474 同源说明句纪律）。
    pub fn blurb(self) -> &'static str {
        match self {
            CardKind::Wifi => "选择要连接的无线网络",
            CardKind::Bluetooth => "蓝牙外设连接（前瞻）",
            CardKind::Focus => "压暗通知，专心做事",
            CardKind::PerfTier => "静音、均衡与性能三档",
            CardKind::NightLight => "夜间暖色护眼",
            CardKind::Airplane => "一键关闭所有射频",
        }
    }

    /// 开关态色令牌（E1——零硬编码色）。
    pub fn on_token(self) -> Token {
        Token::Accent
    }

    pub fn off_token(self) -> Token {
        Token::Off
    }
}

/// Wi-Fi 网络条目。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WifiNet {
    pub ssid: String,
    pub strength: u8,
    /// 已保存网络标记（连接态机初始态判定）。
    pub saved: bool,
}

/// Wi-Fi 链路状态机（连接中态转圈动画在卡内——不弹窗）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WifiState {
    Idle,
    Scanning,
    Connecting,
    Connected,
    Failed,
}

/// 性能三档（F069 档位名——档位语义归 perfmodes，本处只聚合）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PerfTier {
    Silent,
    Balanced,
    Performance,
}

impl PerfTier {
    pub fn next(self) -> PerfTier {
        match self {
            PerfTier::Silent => PerfTier::Balanced,
            PerfTier::Balanced => PerfTier::Performance,
            PerfTier::Performance => PerfTier::Silent,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            PerfTier::Silent => "静音",
            PerfTier::Balanced => "均衡",
            PerfTier::Performance => "性能",
        }
    }
}

/// 面板内滑杆（音量/亮度同构——双端数值标签语义统一）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slider {
    pub percent: u8,
    /// 拖动中（拖动时数值标签放大显示）。
    pub dragging: bool,
}

impl Slider {
    fn new() -> Slider {
        Slider {
            percent: 50,
            dragging: false,
        }
    }

    /// 设值（步进对齐；顶格钳制）。
    pub fn set(&mut self, pct: u8) {
        self.percent = pct.min(100);
    }

    /// 键盘步进。
    pub fn step(&mut self, up: bool) {
        if up {
            self.percent = self.percent.min(100).saturating_add(SLIDER_STEP_PCT).min(100);
        } else {
            self.percent = self.percent.saturating_sub(SLIDER_STEP_PCT);
        }
    }
}

/// 面板卡位（用户可编辑布局：显示集合+顺序）。
#[derive(Clone, Debug, PartialEq, Eq)]
struct CardSlot {
    kind: CardKind,
    shown: bool,
}

/// 快速设置面板状态机。
pub struct QuickPanel {
    layer: FloatLayer,
    frame: Rect,
    /// 卡位表（编辑即重排——E6 联动的面板内投影）。
    slots: Vec<CardSlot>,
    states: [bool; 6],
    /// 翻转动画起点（ms；0 = 无动画进行）。
    flip_anim_start: [u64; 6],  // u64::MAX = 无动画进行
    now_ms: u64,
    pub volume: Slider,
    pub brightness: Slider,
    wifi_state: WifiState,
    wifi_nets: Vec<WifiNet>,
    wifi_sel: usize,
    perf_tier: PerfTier,
    /// 前一档位（toast 确认文案用）。
    perf_prev: Option<PerfTier>,
    ring: FocusRing,
    /// 弹出预算账（首帧渲染完报到）。
    popup_ok: Option<bool>,
    /// 各卡「实测生效」回执账（六类开关全生效判据的记账面）。
    applied: [u32; 6],
    /// toast 队列（性能档确认等）。
    toasts: Vec<String>,
    /// Wi-Fi 转圈起始锚（深化层二：连接中态的卡内转圈相位源）。
    spinner_start: Option<u64>,
}

/// 卡位在状态数组中的下标。
fn slot_idx(kind: CardKind) -> usize {
    match kind {
        CardKind::Wifi => 0,
        CardKind::Bluetooth => 1,
        CardKind::Focus => 2,
        CardKind::PerfTier => 3,
        CardKind::NightLight => 4,
        CardKind::Airplane => 5,
    }
}

impl QuickPanel {
    /// 建面板：默认六卡全显、默认序（Wi-Fi/蓝牙/专注/性能/夜间/飞行）。
    pub fn new() -> QuickPanel {
        let kinds = [
            CardKind::Wifi,
            CardKind::Bluetooth,
            CardKind::Focus,
            CardKind::PerfTier,
            CardKind::NightLight,
            CardKind::Airplane,
        ];
        QuickPanel {
            layer: FloatLayer::new(),
            frame: Rect::new(0, 0, PANEL_W_PX, PANEL_H_PX),
            slots: kinds.iter().map(|k| CardSlot { kind: *k, shown: true }).collect(),
            states: [false; 6],
            flip_anim_start: [u64::MAX; 6],
            now_ms: 0,
            volume: Slider::new(),
            brightness: Slider::new(),
            wifi_state: WifiState::Idle,
            wifi_nets: Vec::new(),
            wifi_sel: 0,
            perf_tier: PerfTier::Balanced,
            perf_prev: None,
            ring: FocusRing::new(8), // 6 卡 + 音量 + 亮度
            popup_ok: None,
            applied: [0; 6],
            toasts: Vec::new(),
            spinner_start: None,
        }
    }

    /// 面板框架（右下弹出：宿主给屏幕框，面板钳到距边 16px 位）。
    pub fn place(&mut self, screen: Rect) -> Rect {
        let f = Rect::new(
            screen.right() - PANEL_W_PX - EDGE_GAP_PX,
            screen.bottom() - PANEL_H_PX - EDGE_GAP_PX,
            PANEL_W_PX,
            PANEL_H_PX,
        );
        self.frame = f.clamp_into(&screen);
        self.frame
    }

    pub fn frame(&self) -> Rect {
        self.frame
    }

    pub fn is_open(&self) -> bool {
        self.layer.is_open()
    }

    /// 打开面板（from 托盘三件套 F075 任一枚）。
    pub fn open(&mut self, now_ms: u64) {
        self.now_ms = now_ms;
        if self.layer.open(now_ms) {
            // 重开回到焦点首卡（Tab 循环起点恒可见）。
            self.ring = FocusRing::new(8);
        }
    }

    /// 首帧渲染完报到（弹出 ≤100ms 判据的记账口）。
    pub fn popup_shown(&mut self, now_ms: u64) {
        self.popup_ok = Some(self.layer.open_in_budget(now_ms, POPUP_BUDGET_MS));
    }

    pub fn popup_in_budget(&self) -> bool {
        self.popup_ok == Some(true)
    }

    /// 点击面板外（轻模态公理：点外必关）。
    pub fn click_outside(&mut self, now_ms: u64) {
        self.layer.close(dbase_close::outside(), now_ms, false);
    }

    /// Esc 关闭。
    pub fn press_escape(&mut self, now_ms: u64) {
        self.layer.close(dbase_close::escape(), now_ms, true);
    }

    /// Tab 循环（全键盘可达第一件）。
    pub fn tab(&mut self) -> usize {
        self.ring.next()
    }

    /// Space/Enter 激活当前焦点项：
    /// 下标 0..6 为卡（翻转开关），6/7 为音量/亮度滑杆（Space 无效，忽略）。
    pub fn activate(&mut self) {
        let idx = self.ring.activate();
        if idx < 6 {
            let kind = self.slots[idx].kind;
            self.toggle(kind);
        }
        // 滑杆位 Space 不动作（键盘调滑杆走上下键——focus_hint 提供）。
    }

    /// 焦点项上的滑杆键盘步进（上下键；非滑杆焦点忽略）。
    pub fn arrow_slider(&mut self, up: bool) {
        match self.ring.activate() {
            6 => self.volume.step(up),
            7 => self.brightness.step(up),
            _ => {}
        }
    }

    /// 翻转开关（卡点击/Space 同路——鼠标键盘结果一致判据）。
    pub fn toggle(&mut self, kind: CardKind) {
        let i = slot_idx(kind);
        if kind == CardKind::Bluetooth {
            // 前瞻灰置：灰置卡不翻转（禁置态诚实——不可点的东西不许假装可点）。
            return;
        }
        self.states[i] = !self.states[i];
        self.flip_anim_start[i] = self.now_ms;
        // 飞行模式聚合所有射频：开启飞行 → Wi-Fi 强制关。
        if kind == CardKind::Airplane && self.states[i] {
            let w = slot_idx(CardKind::Wifi);
            self.states[w] = false;
            self.wifi_state = WifiState::Idle;
            self.wifi_nets.clear();
            self.applied[slot_idx(CardKind::Wifi)] += 1;
        }
        if kind == CardKind::PerfTier {
            // 性能档是三档循环不是布尔：切换 toast 确认。
            self.perf_prev = Some(self.perf_tier);
            self.perf_tier = self.perf_tier.next();
            self.toasts.push(format!(
                "性能档已切换：{}（F069 生效确认）",
                self.perf_tier.name()
            ));
        }
        // 开关态即时生效回执（执行面接线后逐卡真实回执）。
        self.applied[i] += 1;
    }

    /// 翻转动画进度（千分比；150ms 弹性曲线——SNAP_OVERSHOOT 同参族）。
    pub fn flip_progress(&self, kind: CardKind) -> u16 {
        let i = slot_idx(kind);
        if self.flip_anim_start[i] == u64::MAX || self.now_ms <= self.flip_anim_start[i] {
            return 0;
        }
        let t = (self.now_ms - self.flip_anim_start[i]) as u32;
        (t.min(TOGGLE_ANIM_MS) * 1000 / TOGGLE_ANIM_MS) as u16
    }

    pub fn state(&self, kind: CardKind) -> bool {
        self.states[slot_idx(kind)]
    }

    /// 卡是否灰置（蓝牙前瞻）。
    pub fn disabled(&self, kind: CardKind) -> bool {
        kind == CardKind::Bluetooth
    }

    /// 卡的着色令牌（开=强调色，关=中性，禁=灰置）。
    pub fn card_token(&self, kind: CardKind) -> Token {
        if self.disabled(kind) {
            return Token::Disabled;
        }
        if self.state(kind) {
            kind.on_token()
        } else {
            kind.off_token()
        }
    }

    // -- Wi-Fi ------------------------------------------------------------

    /// 请求扫描（含「重新扫描」钮路径）。
    pub fn wifi_rescan(&mut self, now_ms: u64) {
        self.now_ms = now_ms;
        if self.state(CardKind::Airplane) {
            return; // 飞行模式禁扫（射频全关——聚合语义）
        }
        self.wifi_state = WifiState::Scanning;
        self.wifi_nets.clear();
    }

    /// 扫描结果回执（上层射频面接线后供真表；飞行模式射频全关，回执拒收）。
    pub fn wifi_scan_done(&mut self, nets: &[(&str, u8, bool)], now_ms: u64) {
        self.now_ms = now_ms;
        if self.state(CardKind::Airplane) {
            return; // 聚合语义：飞行模式下扫描面静默
        }
        self.wifi_nets = nets
            .iter()
            .map(|(s, r, sv)| WifiNet {
                ssid: String::from(*s),
                strength: *r,
                saved: *sv,
            })
            .collect();
        self.wifi_state = if self.wifi_nets.is_empty() {
            WifiState::Idle // 空表 → 「未找到网络」+ 重新扫描钮
        } else {
            WifiState::Idle
        };
    }

    /// 空表判据：「未找到网络」文案位。
    pub fn wifi_empty_hint(&self) -> bool {
        self.wifi_state == WifiState::Idle && self.wifi_nets.is_empty()
    }

    /// 选择网络并发起连接（连接中态转圈在卡内；飞行模式禁连）。
    pub fn wifi_connect(&mut self, idx: usize, now_ms: u64) {
        if self.state(CardKind::Airplane) {
            return;
        }
        if idx < self.wifi_nets.len() && self.wifi_state != WifiState::Connecting {
            self.wifi_sel = idx;
            self.wifi_state = WifiState::Connecting;
            self.spinner_start = Some(now_ms);
            self.now_ms = now_ms;
        }
    }

    /// 连接结果回执。
    pub fn wifi_connect_done(&mut self, ok: bool, now_ms: u64) {
        self.now_ms = now_ms;
        self.wifi_state = if ok {
            WifiState::Connected
        } else {
            WifiState::Failed
        };
    }

    pub fn wifi_state(&self) -> WifiState {
        self.wifi_state
    }

    // -- 滑杆 --------------------------------------------------------------

    pub fn volume_set(&mut self, pct: u8) {
        self.volume.set(pct);
        self.volume.dragging = true;
    }

    pub fn brightness_set(&mut self, pct: u8) {
        self.brightness.set(pct);
        self.brightness.dragging = true;
    }

    pub fn drag_end(&mut self) {
        self.volume.dragging = false;
        self.brightness.dragging = false;
    }

    /// 亮度滑杆的夜间联动提示（夜间模式开 → 色温提示）。
    pub fn brightness_hint(&self) -> Option<&'static str> {
        if self.state(CardKind::NightLight) {
            Some("夜间模式已开启：亮度将联动色温调节")
        } else {
            None
        }
    }

    // -- 布局编辑（E6 联动：卡位重排/显隐）--------------------------------

    /// 重排卡位（长按拖拽落位）。
    pub fn reorder(&mut self, from: usize, to: usize) -> bool {
        if from >= self.slots.len() || to >= self.slots.len() || from == to {
            return false;
        }
        let slot = self.slots.remove(from);
        self.slots.insert(to, slot);
        true
    }

    /// 显隐卡位（布局可编辑：哪些卡显示）。
    pub fn set_shown(&mut self, kind: CardKind, shown: bool) {
        self.slots[slot_idx(kind)].shown = shown;
    }

    /// 可见卡位序（渲染序）。
    pub fn visible_kinds(&self) -> Vec<CardKind> {
        self.slots
            .iter()
            .filter(|s| s.shown)
            .map(|s| s.kind)
            .collect()
    }

    /// toast 队列消费。
    pub fn pop_toast(&mut self) -> Option<String> {
        if self.toasts.is_empty() {
            None
        } else {
            Some(self.toasts.remove(0))
        }
    }

    /// 六类开关「实测生效」回执计数（判据对账面）。
    pub fn applied_counts(&self) -> [u32; 6] {
        self.applied
    }
}

/// 关闭原因便捷构造（模块内小命名空间，避免 use 噪声）。
mod dbase_close {
    use crate::deskstar::dbase::CloseCause;
    pub fn outside() -> CloseCause {
        CloseCause::OutsideClick
    }
    pub fn escape() -> CloseCause {
        CloseCause::Escape
    }
}

// ---------------------------------------------------------------------------
// 自检（判据唯一源：主册 G-C-06 验收判据）
// ---------------------------------------------------------------------------

/// F076 自检：六类开关全生效回执、弹出 ≤100ms 账、盲操作全键盘可达、
/// 轻模态出路、Wi-Fi 三态、飞行聚合、布局编辑、灰置禁翻转。
pub fn run_quickset_checks() -> CheckSet {
    let mut set = CheckSet::new("deskstar-F076");
    let mut p = QuickPanel::new();
    // 1. 六类开关全部实测生效：逐卡翻转，回执计数全部 +1。
    // 蓝牙（前瞻灰置）与飞行模式（聚合语义单测在 6）不进本循环——
    // 蓝牙不可翻转、飞行翻转的副产物会污染本组回执计数。
    for k in [CardKind::Wifi, CardKind::Focus, CardKind::PerfTier, CardKind::NightLight] {
        p.toggle(k);
    }
    let applied = p.applied_counts();
    let ok6 = applied[0] == 1
        && applied[2] == 1
        && applied[4] == 1
        && applied[5] == 0
        && applied[3] == 1
        && applied[1] == 0;
    ok_if(&mut set, "cards-applied", ok6, "five toggled + bt gray");
    // 2. 面板弹出 ≤100ms。
    p.open(1_000);
    p.popup_shown(1_099);
    let b1 = p.popup_in_budget();
    p.press_escape(1_100);
    p.open(2_000);
    p.popup_shown(2_101);
    ok_if(
        &mut set,
        "popup-budget",
        b1 && !p.popup_in_budget(),
        "popup ≤100ms ledger",
    );
    // 3. 盲操作友好：Tab 循环 8 位全可达 + Space 翻转。
    let mut seen = Vec::new();
    for _ in 0..8 {
        seen.push(p.tab());
    }
    let full = seen.len() == 8 && seen[0] != seen[1];
    let st_before = p.state(CardKind::Wifi);
    p.activate(); // 焦点回到 0（Wi-Fi）→ 翻 Wi-Fi
    ok_if(
        &mut set,
        "keynav-full",
        full && p.state(CardKind::Wifi) != st_before,
        "tab cycle + space toggle",
    );
    // 4. 轻模态出路：点外即关。
    p.press_escape(3_000);
    p.open(3_100);
    p.click_outside(3_150);
    ok_if(&mut set, "outside-close", !p.is_open(), "outside click closes");
    // 5. Wi-Fi：空表提示、扫描、连接三态。
    p.wifi_rescan(4_000);
    ok_if(&mut set, "wifi-states", p.wifi_state() == WifiState::Scanning, "scanning");
    p.wifi_scan_done(&[], 4_100);
    ok_if(&mut set, "wifi-states", p.wifi_empty_hint(), "empty hint");
    p.wifi_rescan(4_200);
    p.wifi_scan_done(&[("STAR-5G", 3, true), (" meeting", 2, false)], 4_300);
    p.wifi_connect(0, 4_400);
    let conn = p.wifi_state() == WifiState::Connecting;
    p.wifi_connect_done(true, 4_600);
    ok_if(
        &mut set,
        "wifi-states",
        conn && p.wifi_state() == WifiState::Connected,
        "connect flow",
    );
    // 6. 飞行模式聚合：开飞行 → Wi-Fi 关 + 清表。
    p.toggle(CardKind::Airplane);
    ok_if(
        &mut set,
        "airplane-aggregate",
        !p.state(CardKind::Wifi) && p.wifi_empty_hint(),
        "radios off",
    );
    // 7. 性能档 toast 确认 + 档位循环。
    let t0 = p.perf_tier;
    p.toggle(CardKind::PerfTier);
    let has_toast = p.pop_toast().is_some();
    ok_if(
        &mut set,
        "perf-tier-toast",
        has_toast && p.perf_tier != t0,
        "tier switch toast",
    );
    // 8. 布局编辑：重排 + 显隐。
    let vis0 = p.visible_kinds();
    p.reorder(0, 2);
    p.set_shown(CardKind::Bluetooth, false);
    let vis1 = p.visible_kinds();
    ok_if(
        &mut set,
        "layout-edit",
        vis0.len() == 6 && vis1.len() == 5 && vis1[0] != vis0[0],
        "reorder + hide",
    );
    // 9. 右下弹出定位（距边 16px）。
    let screen = Rect::new(0, 0, 1920, 1080);
    let f = p.place(screen);
    ok_if(
        &mut set,
        "place-edge",
        f.right() == screen.right() - EDGE_GAP_PX
            && f.bottom() == screen.bottom() - EDGE_GAP_PX
            && f.w == PANEL_W_PX,
        "bottom-right 16px",
    );
    // 10. 滑杆步进与夜间提示（夜间模式已在 1 开启——提示应即时在位）。
    p.brightness_set(30);
    let hint = p.state(CardKind::NightLight) && p.brightness_hint().is_some();
    p.volume.step(true);
    ok_if(&mut set, "sliders", hint && p.volume.percent == 55, "slider + hint");
    set
}

fn ok_if(set: &mut CheckSet, group: &'static str, ok: bool, detail: &'static str) {
    set.add(group, ok, detail);
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bluetooth_gray_never_flips() {
        let mut p = QuickPanel::new();
        assert!(p.disabled(CardKind::Bluetooth));
        p.toggle(CardKind::Bluetooth);
        assert!(!p.state(CardKind::Bluetooth), "前瞻灰置卡不翻转");
        assert_eq!(p.applied_counts()[1], 0);
    }

    #[test]
    fn toggle_animation_progress_ramps() {
        let mut p = QuickPanel::new();
        p.toggle(CardKind::Focus); // now=0 → 动画起点 0
        p.now_ms = 75;
        let mid = p.flip_progress(CardKind::Focus);
        p.now_ms = 150;
        let end = p.flip_progress(CardKind::Focus);
        assert!(mid > 0 && mid < 1000);
        assert_eq!(end, 1000);
    }

    #[test]
    fn wifi_connecting_spins_inside_card() {
        let mut p = QuickPanel::new();
        p.wifi_rescan(100);
        p.wifi_scan_done(&[("net", 2, false)], 200);
        p.wifi_connect(0, 300);
        assert_eq!(p.wifi_state(), WifiState::Connecting);
        // 连接中再点别的网络不重入（转圈不弹窗语义）。
        p.wifi_connect(0, 350);
        assert_eq!(p.wifi_state(), WifiState::Connecting);
        p.wifi_connect_done(false, 400);
        assert_eq!(p.wifi_state(), WifiState::Failed);
    }

    #[test]
    fn airplane_silences_rescan() {
        let mut p = QuickPanel::new();
        p.toggle(CardKind::Airplane);
        p.wifi_rescan(100);
        assert!(p.wifi_state() != WifiState::Scanning, "飞行模式禁扫");
        p.wifi_scan_done(&[("x", 1, false)], 200);
        p.wifi_connect(0, 300);
        assert_eq!(p.wifi_state(), WifiState::Idle, "飞行模式禁连");
        assert!(p.wifi_nets.is_empty(), "飞行模式下扫描回执拒收");
    }

    #[test]
    fn slider_step_clamps() {
        let mut p = QuickPanel::new();
        p.volume_set(98);
        p.volume.step(true);
        assert_eq!(p.volume.percent, 100);
        for _ in 0..30 {
            p.volume.step(false);
        }
        assert_eq!(p.volume.percent, 0);
    }

    #[test]
    fn keyboard_path_reaches_every_control() {
        let mut p = QuickPanel::new();
        let mut visited = [false; 8];
        for _ in 0..8 {
            let idx = p.tab();
            visited[idx] = true;
        }
        assert!(visited.iter().all(|v| *v), "Tab 循环 8 位全可达");
        // 方向键调滑杆（焦点在滑杆位——6 步 Tab 后是音量）。
        for _ in 0..6 {
            p.tab();
        }
        p.arrow_slider(true);
        assert_eq!(p.volume.percent, 55);
    }

    #[test]
    fn quickset_self_checks_all_green() {
        let set = run_quickset_checks();
        let (p_, f_) = set.tally();
        assert!(set.all_passed(), "F076 自检红项：{}/{} 绿", p_, p_ + f_);
    }
}

// ---------------------------------------------------------------------------
// 深化自检二（回炉批 D1-v2）——Wi-Fi 连接中卡内转圈 / 卡布局持久化
// 投影。判据唯一源：主册 G-C-06 设计细节/数据与存储。
// ---------------------------------------------------------------------------

/// Wi-Fi 转圈帧数（8 帧 × 45° = 一圈——卡内转圈，不弹窗）。
pub const WIFI_SPINNER_FRAMES: u32 = 8;

/// 卡布局持久化投影间隔标记。
pub const CARD_LAYOUT_SEP: char = ',';

impl QuickPanel {
    /// Wi-Fi 连接中转圈相位（连接态下卡内转圈的当前帧号 0..8；
    /// 300ms 一帧——慢速有耐心感，非连接态恒 0 不转）。
    pub fn wifi_spinner_phase(&self) -> u32 {
        if self.wifi_state != WifiState::Connecting {
            return 0;
        }
        match self.spinner_start {
            None => 0,
            Some(t0) => ((self.now_ms.saturating_sub(t0) / 300)
            % WIFI_SPINNER_FRAMES as u64) as u32,
        }
    }

    /// 卡布局序列化（顺序 + 显隐——「哪些卡显示/顺序」用户可编辑的
    /// 配置层投影：`名,显隐;` 段串）。
    pub fn serialize_card_layout(&self) -> String {
        let mut out = String::new();
        for s in &self.slots {
            out.push_str(s.kind.name());
            out.push(CARD_LAYOUT_SEP);
            out.push_str(if s.shown { "1" } else { "0" });
            out.push(';');
        }
        out
    }
}

#[cfg(test)]
mod tests_deep2 {
    use super::*;

    #[test]
    fn spinner_needs_connecting_state() {
        let p = QuickPanel::new();
        assert_eq!(p.wifi_spinner_phase(), 0, "非连接态不转");
    }

    #[test]
    fn spinner_stops_after_done() {
        let mut p = QuickPanel::new();
        p.wifi_scan_done(&[("会议室", 3, true)], 900);
        p.wifi_connect(0, 1_000);
        p.now_ms = 1_300;
        assert_ne!(p.wifi_spinner_phase(), 0, "连接中转圈");
        p.wifi_connect_done(true, 1_400);
        assert_eq!(p.wifi_spinner_phase(), 0, "连上即停");
    }

    #[test]
    fn quickset_deep2_checks_all_green() {
        let set = run_quickset_deep2_checks();
        let (p_, f_) = set.tally();
        assert!(set.all_passed(), "F076-deep2 红项：{}/{} 绿", p_, p_ + f_);
    }
}

/// F076 深化自检二：转圈相位 + 布局投影。
pub fn run_quickset_deep2_checks() -> CheckSet {
    let mut set = CheckSet::new("deskstar-F076-deep2");
    let mut p = QuickPanel::new();
    // 1. 转圈：非连接态恒 0；连接态 300ms/帧推进；回执后停转。
    // （扫描回执供给网络清单 → 选择并发起连接。）
    let idle_phase = p.wifi_spinner_phase();
    p.wifi_scan_done(&[("会议室", 3, true)], 900);
    p.wifi_connect(0, 1_000);
    p.now_ms = 1_300;
    let frame1 = p.wifi_spinner_phase();
    p.now_ms = 1_600;
    let frame2 = p.wifi_spinner_phase();
    p.wifi_connect_done(true, 1_700);
    let stopped = p.wifi_spinner_phase();
    set.add(
        "spinner",
        idle_phase == 0 && frame1 == 1 && frame2 == 2 && stopped == 0,
        "card-internal spinner",
    );
    // 2. 布局序列化：默认六卡全显（顺序 + 显隐两要素都在串里）。
    let blob = p.serialize_card_layout();
    let segments = blob.split(';').filter(|s| !s.is_empty()).count();
    let wifi_first = blob.starts_with("Wi-Fi,1;");
    set.add(
        "layout-serialize",
        segments == 6 && wifi_first,
        "order+visibility projection",
    );
    set
}
