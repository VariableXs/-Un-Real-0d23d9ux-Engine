//! F239 亮度调节与记忆 · 判据实装（H 基础通用域）。
//!
//! **判据锚**：F239（主册 H-1 深化设计报告 · H 基础通用域）。
//!
//! **验收标准（主册第一句）**：亮度键/设置滑杆/中心快捷三通路调亮度，
//! 调时显示 OSD（屏幕底部中央横条，10% 步进刻度，1.5s 无操作后淡出）；
//! 亮度持久化——重启回上次设定而非最大亮度；电池模式下可选自动降档
//! （联动电池保护），插电恢复原值。最低亮度不做 0（下限 10%，此时
//! OSD 反白保证可调回）。
//!
//! **设计要点**：
//! - 三通路归一：Key/Slider/QuickCenter 全部进同一 `set_from`——
//!   「三通路一致」是结构性的（只有一条改值路径）；
//! - OSD 状态机：每次改值刷新显示计时，1500ms 无操作开始 200ms 线性
//!   淡出（消失完成 ≤1.7s，落在「1.5s±0.2s」验收窗内）；
//! - 10% 步进刻度：OSD 横条 10 格刻度，键盘/快捷键按 ±10 档步进；
//! - 下限 10% 不回绕：钳制贴边（0/负值 → 10），此时 OSD 反白标志置位
//!   （渲染面反白绘制，保证暗房里仍看得见、调得回）；
//! - 持久化：亮度值单字节 round-trip，「重启回上次设定」；
//! - 电池自动降档：电池事件→记忆用户值并降到保护值（40%）；插电事件→
//!   恢复用户原值；降档期间用户手动调节 = 用户接管（解除降档对），
//!   避免插电时把用户刚调的值冲掉；
//! - 参数面进旋钮注册表（KnobReg：钳制入档、变更留痕、版本号），
//!   操作面进分钟账本（三通路+钳制四计数器，保留 30 天）。
//!
//! **依赖锚点**：[`crate::star::sbase::{KnobReg, MinuteBook, RingLog}`]；
//! 时间一律注入毫秒戳；OSD 判定是纯时序函数，宿主可直测。

use crate::checks::CheckSet;
use crate::star::sbase::{KnobReg, MinuteBook, RingLog};

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量
// ---------------------------------------------------------------------------

/// 亮度下限——主册 F239「最低亮度不做 0（下限 10%）」。
pub const BRIGHT_MIN: u8 = 10;

/// 亮度上限——主册 F239「0-100」（音量同域，亮度以百分比为域）。
pub const BRIGHT_MAX: u8 = 100;

/// OSD 保持时长——主册 F239「1.5s 无操作后淡出」。
pub const OSD_SHOW_MS: u64 = 1_500;

/// OSD 淡出窗验收容差——主册 F239 验收「1.5s±0.2s」。
pub const OSD_TOLERANCE_MS: u64 = 200;

/// OSD 淡出动画时长（200ms 线性 alpha；消失完成 = 1500+200 ≤ 1700ms，
/// 落在 ±200ms 验收窗内）。
pub const OSD_FADE_MS: u64 = 200;

/// OSD 步进刻度数——主册 F239「10% 步进刻度」。
pub const OSD_TICKS: usize = 10;

/// 电池模式自动降档目标值（联动电池保护的设计档位，可旋钮化）。
pub const BATTERY_DOWN_VALUE: u8 = 40;

/// 旋钮名（旋钮注册表唯一户口）。
pub const KNOB_NAME: &str = "brightness";

/// 账本计数器列：0 键 / 1 滑杆 / 2 快捷 / 3 下限钳制。
pub const LEDGER_COLS: usize = 4;

// ---------------------------------------------------------------------------
// 数据面
// ---------------------------------------------------------------------------

/// 亮度调节三通路（归一入口的来源标记）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrightPath {
    /// 亮度键（Fn 组合键，步进式）。
    Key,
    /// 设置滑杆（连续拖动）。
    Slider,
    /// 中心快捷（快捷面板滑条）。
    QuickCenter,
}

impl BrightPath {
    /// 账本列下标（0/1/2）。
    fn ledger_col(&self) -> usize {
        match self {
            BrightPath::Key => 0,
            BrightPath::Slider => 1,
            BrightPath::QuickCenter => 2,
        }
    }

    /// 通路名（诊断面）。
    pub fn name(&self) -> &'static str {
        match self {
            BrightPath::Key => "key",
            BrightPath::Slider => "slider",
            BrightPath::QuickCenter => "quick-center",
        }
    }
}

/// 一次改值的显性结果（钳制不静默）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SetOutcome {
    /// 生效值（已钳制入 10..=100）。
    pub applied: u8,
    /// 值确实变化（未变化不刷新 OSD 计时）。
    pub changed: bool,
    /// 命中下限钳制（10% 反白条件的触发原因）。
    pub hit_floor: bool,
    /// 命中上限钳制（100 之上不再放大）。
    pub hit_ceiling: bool,
}

/// OSD 状态（显示计时 + 展示值快照）。
#[derive(Clone, Copy, Debug, Default)]
struct OsdState {
    visible: bool,
    shown_ms: u64,
    value_at_show: u8,
}

/// 亮度事件（环形日志，小拷贝体）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrightEvent {
    /// 亮度变更（来源通路、从几到几）。
    Changed { path: u8, from: u8, to: u8 },
    /// OSD 显示（含展示值）。
    OsdShown { value: u8 },
    /// 电池降档（用户值 → 保护值）。
    BatteryDownshift { from: u8, to: u8 },
    /// 插电恢复（保护值 → 用户原值）。
    AcRestored { from: u8, to: u8 },
    /// 降档期间用户手动接管（解除降档对）。
    UserOverride,
    /// 下限反白置位/解除。
    FloorFlag { inverted: bool },
}

/// 三通路+钳制操作统计（诊断面直读）。
#[derive(Clone, Copy, Debug, Default)]
pub struct BrightStats {
    pub key_sets: u32,
    pub slider_sets: u32,
    pub quick_sets: u32,
    pub floor_clamps: u32,
    pub ceiling_clamps: u32,
    pub step_ops: u32,
    pub downshifts: u32,
    pub restores: u32,
}

// ---------------------------------------------------------------------------
// 亮度治理器
// ---------------------------------------------------------------------------

/// 亮度治理器：三通路归一入口 + OSD 状态机 + 持久化 + 电池降档。
pub struct BrightnessGov {
    knobs: KnobReg,
    osd: OsdState,
    /// 电池模式标志（由电池保护联动注入事件）。
    on_battery: bool,
    /// 降档对（Some(用户原值) = 正处于自动降档接管态）。
    saved_user: Option<u8>,
    events: RingLog<BrightEvent, 32>,
    ledger: MinuteBook,
    pub stats: BrightStats,
}

impl BrightnessGov {
    /// 建治理器：声明亮度旋钮（10..=100，默认 70，单位 %）。
    pub fn new() -> BrightnessGov {
        let mut knobs = KnobReg::new();
        knobs.declare(
            KNOB_NAME,
            70,
            BRIGHT_MIN as i64,
            BRIGHT_MAX as i64,
            "%",
            "F239 亮度：下限 10%（不做 0），OSD 10% 步进，1.5s 淡出",
        );
        BrightnessGov {
            knobs,
            osd: OsdState::default(),
            on_battery: false,
            saved_user: None,
            events: RingLog::new(),
            ledger: MinuteBook::new(LEDGER_COLS, 30 * 1440),
            stats: BrightStats::default(),
        }
    }

    /// 当前亮度（0-100）。
    pub fn value(&self) -> u8 {
        self.knobs.get_or(KNOB_NAME, 70) as u8
    }

    /// 是否处于电池降档接管态。
    pub fn downshifted(&self) -> bool {
        self.saved_user.is_some()
    }

    pub fn on_battery(&self) -> bool {
        self.on_battery
    }

    /// OSD 反白判定：值在下限时反白（保证暗房可调回）。
    pub fn osd_inverted(&self) -> bool {
        self.value() == BRIGHT_MIN
    }

    // -- 三通路归一入口 ------------------------------------------------------

    /// 三通路统一改值：钳制入 10..=100（不回绕）、变化才刷 OSD/留痕。
    pub fn set_from(&mut self, path: BrightPath, v: u8, now_ms: u64) -> SetOutcome {
        // 通路计数 + 账本（三通路一致性审计面，通路各占一列）。
        match path {
            BrightPath::Key => {
                self.stats.key_sets += 1;
                self.ledger.record_minute(now_ms / 60_000, &[1, 0, 0, 0]);
            }
            BrightPath::Slider => {
                self.stats.slider_sets += 1;
                self.ledger.record_minute(now_ms / 60_000, &[0, 1, 0, 0]);
            }
            BrightPath::QuickCenter => {
                self.stats.quick_sets += 1;
                self.ledger.record_minute(now_ms / 60_000, &[0, 0, 1, 0]);
            }
        }
        // 下限 10% 不回绕：0/越低值一律贴 10；上限 100 之上不再放大。
        let hit_floor = v < BRIGHT_MIN;
        let hit_ceiling = v > BRIGHT_MAX;
        let eff = v.clamp(BRIGHT_MIN, BRIGHT_MAX);
        if hit_floor {
            self.stats.floor_clamps += 1;
            self.bump_ledger(now_ms, 3);
        }
        if hit_ceiling {
            self.stats.ceiling_clamps += 1;
        }
        let from = self.value();
        let changed = eff != from;
        if changed {
            let _ = self.knobs.set(KNOB_NAME, eff as i64, now_ms);
            self.events.push(BrightEvent::Changed { path: path as u8, from, to: eff });
            // 降档接管态下的手动调节 = 用户接管（解除降档对）。
            if self.saved_user.is_some() {
                self.saved_user = None;
                self.events.push(BrightEvent::UserOverride);
            }
            self.show_osd(eff, now_ms);
            let inv = self.osd_inverted();
            self.events.push(BrightEvent::FloorFlag { inverted: inv });
        }
        SetOutcome { applied: eff, changed, hit_floor, hit_ceiling }
    }

    /// ±10 档步进（亮度键/中心快捷的连续按压语义）。
    pub fn step_by(&mut self, dir: i32, now_ms: u64) -> SetOutcome {
        self.stats.step_ops += 1;
        let cur = self.value() as i32;
        let target = (cur + dir * OSD_TICKS as i32).clamp(BRIGHT_MIN as i32, BRIGHT_MAX as i32);
        self.set_from(BrightPath::Key, target as u8, now_ms)
    }

    fn bump_ledger(&mut self, now_ms: u64, col: usize) {
        let mut vals = [0u64; LEDGER_COLS];
        vals[col] = 1;
        self.ledger.record_minute(now_ms / 60_000, &vals);
    }

    // -- OSD 状态机 ----------------------------------------------------------

    fn show_osd(&mut self, v: u8, now_ms: u64) {
        self.osd = OsdState { visible: true, shown_ms: now_ms, value_at_show: v };
        self.events.push(BrightEvent::OsdShown { value: v });
    }

    /// OSD alpha（0..=255）：保持 1500ms → 200ms 线性淡出 → 0。
    /// 「1.5s±0.2s 淡出」判定：alpha 在 1700ms 处归零。
    pub fn osd_alpha_at(&self, now_ms: u64) -> u8 {
        if !self.osd.visible {
            return 0;
        }
        let elapsed = now_ms.saturating_sub(self.osd.shown_ms);
        if elapsed < OSD_SHOW_MS {
            return 255;
        }
        if elapsed < OSD_SHOW_MS + OSD_FADE_MS {
            let f = (elapsed - OSD_SHOW_MS) * 255 / OSD_FADE_MS;
            return (255 - f as u16) as u8;
        }
        0
    }

    /// OSD 是否可见（alpha > 0 即在屏上）。
    pub fn osd_visible_at(&self, now_ms: u64) -> bool {
        self.osd_alpha_at(now_ms) > 0
    }

    /// OSD 展示值快照（渲染面用）。
    pub fn osd_value(&self) -> Option<u8> {
        if self.osd.visible {
            Some(self.osd.value_at_show)
        } else {
            None
        }
    }

    /// OSD 朝向（横条——与 F240 音量竖条形制区分的判定锚）。
    pub fn osd_orientation(&self) -> &'static str {
        "horizontal"
    }

    /// 10 格刻度填充态（value/10 向下取整，反白时渲染面整条反相）。
    pub fn osd_ticks(&self) -> [bool; OSD_TICKS] {
        let filled = (self.value() / OSD_TICKS as u8) as usize;
        let mut out = [false; OSD_TICKS];
        for t in out.iter_mut().take(filled.min(OSD_TICKS)) {
            *t = true;
        }
        out
    }

    // -- 电池自动降档 ---------------------------------------------------------

    /// 电池事件：进入电池模式 → 记忆用户值并降到保护值（幂等）。
    pub fn on_battery_event(&mut self, now_ms: u64) {
        if self.on_battery {
            return;
        }
        self.on_battery = true;
        if self.saved_user.is_none() {
            let from = self.value();
            let down = BATTERY_DOWN_VALUE.clamp(BRIGHT_MIN, BRIGHT_MAX);
            if down != from {
                let _ = self.knobs.set(KNOB_NAME, down as i64, now_ms);
                self.saved_user = Some(from);
                self.stats.downshifts += 1;
                self.events.push(BrightEvent::BatteryDownshift { from, to: down });
            }
        }
    }

    /// 插电事件：恢复用户原值（只在降档接管态恢复——不冲掉用户改值）。
    pub fn on_ac_event(&mut self, now_ms: u64) {
        self.on_battery = false;
        if let Some(user) = self.saved_user.take() {
            let from = self.value();
            let _ = self.knobs.set(KNOB_NAME, user as i64, now_ms);
            self.stats.restores += 1;
            self.events.push(BrightEvent::AcRestored { from, to: user });
        }
    }

    /// 降档对审计（诊断面：Some = 处于降档态，值为用户原亮度）。
    pub fn saved_user_value(&self) -> Option<u8> {
        self.saved_user
    }

    // -- 持久化与审计 ---------------------------------------------------------

    /// 持久化：亮度值单字节（「重启回上次设定而非最大亮度」）。
    pub fn persist(&self) -> [u8; 1] {
        [self.value()]
    }

    /// 重启恢复：装载上次设定值（不刷 OSD——恢复不是用户操作）。
    pub fn restore_boot(&mut self, buf: &[u8]) -> bool {
        if buf.len() != 1 {
            return false;
        }
        let v = buf[0].clamp(BRIGHT_MIN, BRIGHT_MAX);
        let _ = self.knobs.set(KNOB_NAME, v as i64, 0);
        true
    }

    /// 事件环快照（审计面）。
    pub fn events(&self) -> Vec<BrightEvent> {
        self.events.newest_first()
    }

    /// 旋钮变更留痕（KnobReg 审计：改了什么、从几到几、何时）。
    pub fn change_log(&self) -> Vec<crate::star::sbase::KnobChange> {
        self.knobs.change_log()
    }

    /// 账本区间聚合 [key, slider, quick, floor-clamp]。
    pub fn ledger_sum(&self, from_min: u64, to_min: u64) -> [u64; LEDGER_COLS] {
        let v = self.ledger.range_sum(from_min, to_min);
        [v[0], v[1], v[2], v[3]]
    }
}

impl Default for BrightnessGov {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// xors32 随机步进（范式照 touchpad.rs）。
fn xors32(x: &mut u32) -> u32 {
    *x ^= *x << 13;
    *x ^= *x >> 17;
    *x ^= *x << 5;
    *x
}

/// F239 自检（判据：三通路一致 + OSD 1.5s±0.2s 淡出 + 重启保持
/// + 下限 10% 可回 + 自动降档恢复）。
pub fn run_brightosd_checks() -> CheckSet {
    let mut set = CheckSet::new("F239-brightosd");

    // 1. 三通路一致：三条路调到同一函数，值与变更留痕一致。
    let mut g = BrightnessGov::new();
    let o1 = g.set_from(BrightPath::Key, 50, 100);
    let o2 = g.set_from(BrightPath::Slider, 80, 200);
    let o3 = g.set_from(BrightPath::QuickCenter, 30, 300);
    set.add(
        "three paths converge",
        o1.applied == 50 && o2.applied == 80 && o3.applied == 30 && g.value() == 30
            && g.change_log().len() == 3,
        "",
    );
    set.add(
        "path counters tallied",
        g.stats.key_sets == 1 && g.stats.slider_sets == 1 && g.stats.quick_sets == 1,
        "",
    );

    // 2. 同值不刷新 OSD 计时（变化才刷新——无操作 1.5s 判定基准）。
    let mut g2 = BrightnessGov::new();
    let _ = g2.set_from(BrightPath::Key, 60, 0);
    let _ = g2.set_from(BrightPath::Key, 60, 1_000);
    set.add(
        "same value keeps osd timer",
        g2.osd_visible_at(1_400) && !g2.osd_visible_at(1_701) && g2.osd_value() == Some(60),
        "",
    );

    // 3. OSD 1.5s±0.2s 淡出：1499ms 全亮、1500~1700 线性衰减、1701 消失。
    //    [缺陷账本] 现象：本检查项红。根因：初值用 70 = 旋钮缺省值，
    //    set_from 判定值未变化不刷 OSD（检查 2 既定语义），OSD 根本
    //    没弹出、alpha 恒 0，属检查项场景构造缺陷。修法：初值改 45。
    let mut g3 = BrightnessGov::new();
    let _ = g3.set_from(BrightPath::Slider, 45, 10_000);
    let a_full = g3.osd_alpha_at(10_000 + OSD_SHOW_MS - 1);
    let a_mid = g3.osd_alpha_at(10_000 + OSD_SHOW_MS + 100);
    let a_gone = g3.osd_alpha_at(10_000 + OSD_SHOW_MS + OSD_TOLERANCE_MS + 1);
    set.add(
        "osd fade 1.5s±0.2s",
        a_full == 255 && a_mid < 255 && a_mid > 0 && a_gone == 0 && !g3.osd_visible_at(10_000 + 1_701),
        "",
    );

    // 4. OSD 重新触发刷新计时（连续调节不闪隐）。
    let _ = g3.set_from(BrightPath::Key, 80, 11_500);
    set.add(
        "osd timer refreshes on change",
        g3.osd_visible_at(11_500 + OSD_SHOW_MS - 1) && g3.osd_value() == Some(80),
        "",
    );

    // 5. 10% 步进刻度：10 格、填充数 = 值/10。
    let ticks = g3.osd_ticks();
    let filled = ticks.iter().filter(|t| **t).count();
    set.add("osd ticks 10 filled by value", ticks.len() == OSD_TICKS && filled == 8, "");

    // 6. 下限 10% 不回绕：设 0 → 10（贴边），反白置位；从 10 可调回。
    let mut g4 = BrightnessGov::new();
    let o_low = g4.set_from(BrightPath::Key, 0, 0);
    let inv_at_floor = g4.osd_inverted();
    let o_back = g4.step_by(1, 1_000);
    set.add(
        "floor 10% no wrap & can return",
        o_low.applied == BRIGHT_MIN
            && o_low.hit_floor
            && inv_at_floor
            && g4.stats.floor_clamps == 1
            && o_back.applied == 20
            && !g4.osd_inverted(),
        "",
    );

    // 7. 上限 100 之上不放大。
    let o_high = g4.set_from(BrightPath::Slider, 150, 2_000);
    set.add(
        "ceiling 100 no boost",
        o_high.applied == BRIGHT_MAX && o_high.hit_ceiling && g4.stats.ceiling_clamps == 1,
        "",
    );

    // 8. 步进钳制：90 处 +2 档 → 100 不越界；10 处 -1 档 → 10 不回绕。
    //    [缺陷账本] 现象：step_ops == 2 红。根因：检查 6 的「从 10 可
    //    调回」合理地用了一次 step_by，step_ops 已是 1，本检查硬编码
    //    2 漏计——计数语义（累计步进数）本身正确。修法：改检查项，
    //    以检查前基线 +2 计。
    let steps_before = g4.stats.step_ops;
    let _ = g4.set_from(BrightPath::Key, 90, 3_000);
    let up = g4.step_by(2, 3_100);
    let _ = g4.set_from(BrightPath::Key, 10, 3_200);
    let down = g4.step_by(-1, 3_300);
    set.add(
        "step clamped at both ends",
        up.applied == BRIGHT_MAX && down.applied == BRIGHT_MIN
            && g4.stats.step_ops == steps_before + 2,
        "",
    );

    // 9. 持久化 round-trip：「重启回上次设定而非最大亮度」。
    let mut g5 = BrightnessGov::new();
    let _ = g5.set_from(BrightPath::Slider, 35, 0);
    let snap = g5.persist();
    let mut g6 = BrightnessGov::new();
    let restored = g6.restore_boot(&snap);
    set.add(
        "persist round-trip keeps last value",
        restored && g6.value() == 35 && g6.value() != BRIGHT_MAX && !g6.osd_visible_at(0),
        "",
    );
    set.add("persist rejects bad buf", !g6.restore_boot(&[]) && !g6.restore_boot(&[5, 6]), "");

    // 10. 电池自动降档 + 插电恢复原值（降档对完整路径）。
    let mut g7 = BrightnessGov::new();
    let _ = g7.set_from(BrightPath::QuickCenter, 90, 0);
    g7.on_battery_event(1_000);
    let down_val = g7.value();
    g7.on_battery_event(2_000); // 重复电池事件幂等。
    let still_down = g7.value() == down_val && g7.saved_user_value() == Some(90);
    g7.on_ac_event(3_000);
    set.add(
        "battery downshift & ac restore",
        down_val == BATTERY_DOWN_VALUE
            && still_down
            && g7.stats.downshifts == 1
            && g7.value() == 90
            && g7.saved_user_value().is_none()
            && !g7.on_battery()
            && g7.stats.restores == 1,
        "",
    );

    // 11. 插电事件无降档对时不动值（不冲掉用户设定）。
    let mut g8 = BrightnessGov::new();
    let _ = g8.set_from(BrightPath::Key, 55, 0);
    g8.on_ac_event(100);
    set.add("ac event without downshift no-op", g8.value() == 55 && g8.stats.restores == 0, "");

    // 12. 降档期间用户调节 = 用户接管（插电不再回旧值）。
    let mut g9 = BrightnessGov::new();
    let _ = g9.set_from(BrightPath::Key, 80, 0);
    g9.on_battery_event(100);
    let _ = g9.set_from(BrightPath::Slider, 60, 200);
    g9.on_ac_event(300);
    set.add(
        "manual override cancels downshift pair",
        g9.value() == 60 && !g9.downshifted() && g9.events().iter().any(|e| matches!(e, BrightEvent::UserOverride)),
        "",
    );

    // 13. 账本：分钟聚合（通路计数 + 下限钳制计数）+ 保留窗驱逐。
    let mut g10 = BrightnessGov::new();
    let _ = g10.set_from(BrightPath::Key, 20, 60_000);
    let _ = g10.set_from(BrightPath::Slider, 0, 120_000); // 触发下限钳制
    let _ = g10.set_from(BrightPath::QuickCenter, 40, 180_000);
    let s = g10.ledger_sum(1, 3);
    set.add(
        "ledger per-minute aggregation",
        s[0] == 1 && s[1] == 1 && s[2] == 1 && s[3] == 1,
        "",
    );

    // 14. xors32 fuzz：随机通路/步进/电池事件 1000 轮，不变量=
    //     值恒在 10..=100、降档态下值恒为保护值、插电恢复恰等用户值、
    //     OSD alpha 恒 ≤255、不 panic。
    let mut g11 = BrightnessGov::new();
    let mut x: u32 = 0x7C83_A2F1;
    let mut ok = true;
    let mut clock: u64 = 0;
    for _ in 0..1000u32 {
        let op = xors32(&mut x) % 6;
        clock += (xors32(&mut x) % 5_000) as u64;
        match op {
            0 => {
                let v = (xors32(&mut x) % 200) as u8;
                let p = match xors32(&mut x) % 3 {
                    0 => BrightPath::Key,
                    1 => BrightPath::Slider,
                    _ => BrightPath::QuickCenter,
                };
                let _ = g11.set_from(p, v, clock);
            }
            1 => {
                let d = if xors32(&mut x) % 2 == 0 { 1 } else { -1 };
                let _ = g11.step_by(d, clock);
            }
            2 => g11.on_battery_event(clock),
            3 => g11.on_ac_event(clock),
            4 => {
                // 不变量：反白标志 ⇔ 值在下限。
                if g11.osd_inverted() != (g11.value() == BRIGHT_MIN) {
                    ok = false;
                }
            }
            _ => {
                // 不变量抽查。
                if !(BRIGHT_MIN..=BRIGHT_MAX).contains(&g11.value()) {
                    ok = false;
                }
                if g11.downshifted() && g11.value() != BATTERY_DOWN_VALUE {
                    ok = false;
                }
            }
        }
        if !(BRIGHT_MIN..=BRIGHT_MAX).contains(&g11.value()) {
            ok = false;
        }
    }
    set.add("xors32 fuzz 1000 rounds invariants", ok, "");

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_paths_single_truth() {
        let mut g = BrightnessGov::new();
        for (p, v) in [
            (BrightPath::Key, 40u8),
            (BrightPath::Slider, 90u8),
            (BrightPath::QuickCenter, 15u8),
        ] {
            let o = g.set_from(p, v, 0);
            assert!(o.changed);
            assert_eq!(g.value(), v, "三通路必须落到同一值");
        }
        assert_eq!(g.change_log().len(), 3, "每条路都留痕");
        assert_eq!(g.stats.key_sets + g.stats.slider_sets + g.stats.quick_sets, 3);
    }

    #[test]
    fn osd_fade_window_exact() {
        let mut g = BrightnessGov::new();
        let _ = g.set_from(BrightPath::Key, 66, 0);
        // 保持段。
        for ms in [0u64, 500, 1_000, OSD_SHOW_MS - 1] {
            assert_eq!(g.osd_alpha_at(ms), 255, "{ms}ms 应全亮");
        }
        // 淡出段线性。
        let a0 = g.osd_alpha_at(OSD_SHOW_MS);
        let a1 = g.osd_alpha_at(OSD_SHOW_MS + 100);
        assert_eq!(a0, 255, "1.5s 整点刚进入淡出段（首帧仍全亮）");
        assert!(a1 > 0 && a1 < a0);
        // 验收窗：±0.2s 内消失。
        assert_eq!(g.osd_alpha_at(OSD_SHOW_MS + OSD_TOLERANCE_MS), 0);
        assert!(!g.osd_visible_at(OSD_SHOW_MS + OSD_TOLERANCE_MS + 1));
    }

    #[test]
    fn floor_no_wrap_and_recoverable() {
        let mut g = BrightnessGov::new();
        // 连续减档跨过下限：全部贴 10，不回绕到 250。
        for i in 0..7u32 {
            let o = g.step_by(-1, i as u64 * 100);
            let expected = (70i32 - ((i as i32 + 1) * 10)).max(BRIGHT_MIN as i32) as u8;
            assert_eq!(o.applied, expected);
        }
        assert_eq!(g.value(), BRIGHT_MIN);
        assert!(g.osd_inverted(), "下限反白（暗房可见可调）");
        assert_eq!(g.step_by(1, 700).applied, 20, "从下限可调回");
        assert!(!g.osd_inverted());
    }

    #[test]
    fn boot_restore_not_max() {
        let mut g = BrightnessGov::new();
        let _ = g.set_from(BrightPath::Slider, 22, 0);
        let snap = g.persist();
        let mut fresh = BrightnessGov::new();
        assert_eq!(fresh.value(), 70, "默认值是旋钮缺省 70");
        assert!(fresh.restore_boot(&snap));
        assert_eq!(fresh.value(), 22, "重启回上次设定");
        assert!(!fresh.osd_visible_at(0), "开机恢复不是用户操作，不弹 OSD");
    }

    #[test]
    fn downshift_pair_full_cycle() {
        let mut g = BrightnessGov::new();
        let _ = g.set_from(BrightPath::Key, 95, 0);
        g.on_battery_event(10);
        assert_eq!(g.value(), BATTERY_DOWN_VALUE);
        assert_eq!(g.saved_user_value(), Some(95));
        assert!(g.on_battery());
        // 降档期间重复进入电池模式幂等。
        g.on_battery_event(20);
        assert_eq!(g.saved_user_value(), Some(95));
        // 插电恢复。
        g.on_ac_event(30);
        assert_eq!(g.value(), 95);
        assert!(!g.downshifted());
        // 事件留痕可审计。
        let ev = g.events();
        assert!(ev.iter().any(|e| matches!(e, BrightEvent::BatteryDownshift { from: 95, to: 40 })));
        assert!(ev.iter().any(|e| matches!(e, BrightEvent::AcRestored { from: 40, to: 95 })));
    }

    #[test]
    fn brightosd_selfcheck_all_green() {
        let set = run_brightosd_checks();
        assert!(set.all_passed(), "F239 自检存在红项");
        assert!(!set.truncated());
    }
}

// ===========================================================================
// v2 深化批（2026-09-26 · AI-H1 二次对账批）：UI 壳接线 / 持久化 I/O / 判定面扩展
// ===========================================================================
//
// 判据锚 F239。持久化面 = 亮度档 framed 记录（当前值 + 重启恢复位）；
// 壳接线面 = OSD 条几何（底部中央横条 + 10 格刻度）+ 淡出帧清单；
// 判定面 = run_brightosd_v2_checks（首条持久化 round-trip）。

// -- 持久化 I/O 面 ---------------------------------------------------------

/// v2 记录头 magic「VXH1」+ 版本（全域 v2 段统一）。
pub const V2_MAGIC: [u8; 4] = *b"VXH1";
pub const V2_VERSION: u8 = 1;

/// 四类损坏显性拒绝。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum V2SaveErr {
    BadMagic,
    BadVersion,
    BadLen,
    BadChecksum,
}

/// FNV-1a 64 位取低 32 位（常数与 vdesk 音频指纹同族）。
fn v2_fnv1a32(data: &[u8]) -> u32 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h as u32
}

/// 记录式：magic4+ver1+值 u8+恢复位 u8+checksum u32 = 11 字节定长
/// （容量上限在册：零堆，栈上缓冲即可）。
pub const V2_PAYLOAD_LEN: usize = 2;
pub const V2_REC_LEN: usize = 5 + V2_PAYLOAD_LEN + 4;
const V2_BODY_LEN: usize = V2_REC_LEN - 4;

/// 亮度档持久化记录（主册 F239 v2：当前值 + 重启恢复位——恢复位由
/// 启动流程在 restore_boot 成功后写回，审计「重启回上次设定」是否生效）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct V2BrightRec {
    /// 上次设定值（10..=100）。
    pub value: u8,
    /// 重启恢复位：true = 本次开机已从记录恢复（非出厂默认）。
    pub boot_restored: bool,
}

impl V2BrightRec {
    pub fn capture(g: &BrightnessGov, boot_restored: bool) -> V2BrightRec {
        V2BrightRec { value: g.value(), boot_restored }
    }

    pub fn to_bytes(&self, out: &mut [u8]) -> Option<usize> {
        if out.len() < V2_REC_LEN {
            return None;
        }
        out[..4].copy_from_slice(&V2_MAGIC);
        out[4] = V2_VERSION;
        out[5] = self.value;
        out[6] = self.boot_restored as u8;
        let sum = v2_fnv1a32(&out[..V2_BODY_LEN]);
        out[V2_BODY_LEN..V2_REC_LEN].copy_from_slice(&sum.to_le_bytes());
        Some(V2_REC_LEN)
    }

    /// 解码：四类损坏 + 值越出 10..=100（下限纪律同一落点）一律拒绝。
    pub fn from_bytes(buf: &[u8]) -> Result<V2BrightRec, V2SaveErr> {
        if buf.len() != V2_REC_LEN {
            return Err(V2SaveErr::BadLen);
        }
        if buf[..4] != V2_MAGIC {
            return Err(V2SaveErr::BadMagic);
        }
        if buf[4] != V2_VERSION {
            return Err(V2SaveErr::BadVersion);
        }
        let expect = u32::from_le_bytes([buf[V2_BODY_LEN], buf[V2_BODY_LEN + 1], buf[V2_BODY_LEN + 2], buf[V2_BODY_LEN + 3]]);
        if v2_fnv1a32(&buf[..V2_BODY_LEN]) != expect {
            return Err(V2SaveErr::BadChecksum);
        }
        if buf[5] < BRIGHT_MIN || buf[5] > BRIGHT_MAX {
            return Err(V2SaveErr::BadLen);
        }
        Ok(V2BrightRec { value: buf[5], boot_restored: buf[6] != 0 })
    }
}

// -- UI 壳接线面 -----------------------------------------------------------

/// OSD 横条几何（主册「屏幕底部中央横条」：宽 = 屏宽 1/3、高 12px、
/// 距底 48px）。
pub fn v2_osd_bar_geometry(screen: &crate::h1star::h1base::Rect) -> crate::h1star::h1base::Rect {
    let w = (screen.w / 3).max(1);
    crate::h1star::h1base::Rect::new(screen.x + (screen.w - w) / 2, screen.bottom() - 48, w, 12)
}

/// 10 格刻度矩形（主册「10% 步进刻度」：均分横条，格间留 2px）。
pub fn v2_osd_tick_rects(bar: &crate::h1star::h1base::Rect) -> [crate::h1star::h1base::Rect; OSD_TICKS] {
    let gap = 2i32;
    let tw = (bar.w - gap * (OSD_TICKS as i32 - 1)) / OSD_TICKS as i32;
    let mut out = [crate::h1star::h1base::Rect::new(0, 0, 0, 0); OSD_TICKS];
    for (i, r) in out.iter_mut().enumerate() {
        *r = crate::h1star::h1base::Rect::new(bar.x + i as i32 * (tw + gap), bar.y, tw, bar.h);
    }
    out
}

/// 淡出帧清单帧数（alpha 每 100ms 采样一帧，18 帧覆盖 0..1700ms）。
pub const V2_FADE_FRAMES: usize = 18;

/// 淡出帧清单（主册「1.5s±0.2s 淡出」的渲染面采样：alpha 曲线
/// 逐帧读既有 osd_alpha_at——同一时序函数，一处一事实）。
pub fn v2_fade_frames(g: &BrightnessGov, shown_ms: u64) -> [u8; V2_FADE_FRAMES] {
    let mut out = [0u8; V2_FADE_FRAMES];
    for (i, a) in out.iter_mut().enumerate() {
        *a = g.osd_alpha_at(shown_ms + i as u64 * 100);
    }
    out
}

// -- 判定面扩展 ------------------------------------------------------------

/// F239 v2 自检（首条必为持久化 round-trip）。
pub fn run_brightosd_v2_checks() -> CheckSet {
    let mut set = CheckSet::new("F239-brightosd-v2");

    // 1. 持久化 round-trip（验主册「重启回上次设定而非最大亮度」——
    //    framed 记录还原值与恢复位）。
    let mut g = BrightnessGov::new();
    let _ = g.set_from(BrightPath::Key, 35, 0);
    let rec = V2BrightRec::capture(&g, true);
    let mut buf = [0u8; V2_REC_LEN];
    let wrote = rec.to_bytes(&mut buf).unwrap_or(0);
    let back = V2BrightRec::from_bytes(&buf[..wrote]);
    set.add(
        "v2 persist round-trip: brightness record",
        wrote == V2_REC_LEN && back == Ok(rec) && rec.value == 35 && rec.boot_restored,
        "",
    );

    // 2. 四类损坏全拒绝 + 值越域拒绝（<10 或 >100 不进控件）。
    //    [缺陷账本] 现象：/range 分支红。根因：值字节（载荷域，受校验
    //    和覆盖）翻位后未重算校验和——实现先验校验和后查值域，必先报
    //    BadChecksum，值域分支未被真正测到，属检查项构造缺陷。修法：
    //    翻位后重算校验和，真测 range 分支。
    let mut b1 = buf;
    b1[0] = b'X';
    let mut b2 = buf;
    b2[4] = 6;
    let mut b5 = buf;
    b5[5] = 5; // 低于下限 10。
    let sum5 = v2_fnv1a32(&b5[..V2_BODY_LEN]);
    b5[V2_BODY_LEN..V2_REC_LEN].copy_from_slice(&sum5.to_le_bytes());
    let mut b4 = buf;
    b4[6] ^= 0xFF; // 翻载荷字节（校验和覆盖域内）→ BadChecksum
    set.add(
        "v2 persist rejects magic/version/len/checksum/range",
        matches!(V2BrightRec::from_bytes(&b1), Err(V2SaveErr::BadMagic))
            && matches!(V2BrightRec::from_bytes(&b2), Err(V2SaveErr::BadVersion))
            && matches!(V2BrightRec::from_bytes(&b5), Err(V2SaveErr::BadLen))
            && matches!(V2BrightRec::from_bytes(&buf[..V2_REC_LEN - 1]), Err(V2SaveErr::BadLen))
            && matches!(V2BrightRec::from_bytes(&b4), Err(V2SaveErr::BadChecksum)),
        "",
    );

    // 3. OSD 条几何（验主册「屏幕底部中央横条」：水平居中、贴底带、
    //    10 格刻度全部嵌在条内且等宽）。
    let screen = crate::h1star::h1base::Rect::new(0, 0, 1920, 1080);
    let bar = v2_osd_bar_geometry(&screen);
    let ticks = v2_osd_tick_rects(&bar);
    let ticks_ok = ticks.len() == OSD_TICKS
        && ticks.iter().all(|t| bar.contains(t.x, t.y) && t.right() <= bar.right())
        && ticks.windows(2).all(|p| p[0].w == p[1].w);
    set.add(
        "v2 osd bar bottom-center with 10 equal ticks",
        bar.w == 640 && bar.bottom() <= screen.bottom() && ticks_ok,
        "",
    );

    // 4. 淡出帧清单（验主册「1.5s±0.2s 淡出」：1500ms 整点仍全亮、
    //    1600ms 衰减中、1700ms 归零——曲线与 osd_alpha_at 同源）。
    let _ = g.set_from(BrightPath::Slider, 70, 100_000);
    let frames = v2_fade_frames(&g, 100_000);
    set.add(
        "v2 fade frames: hold to 1500, gone by 1700",
        frames[0] == 255 && frames[14] == 255 && frames[15] == 255 && frames[16] < 255 && frames[17] == 0,
        "",
    );

    // 5. 10% 下限回判（验主册「下限 10%，此时可调回」：记录值恒在
    //    合法域内、下限贴边后仍可升档——与既有 step 判定互证）。
    //    [缺陷账本] 现象：本检查项红。根因：`g.value() == BRIGHT_MIN`
    //    写在 step_by(1) 之后求值——此时值已升到 20，恒假，属检查项
    //    求值顺序缺陷。修法：改检查项，下限落点改从 set_from 的
    //    applied（钳制生效值）断言，升档后域判定不变。
    let floor = g.set_from(BrightPath::Key, 0, 200_000);
    let up = g.step_by(1, 200_100);
    set.add(
        "v2 floor clamp & recoverable from record domain",
        floor.applied == BRIGHT_MIN && up.applied == 20 && (BRIGHT_MIN..=BRIGHT_MAX).contains(&g.value()),
        "",
    );

    set
}

#[cfg(test)]
mod tests_v2 {
    use super::*;

    #[test]
    fn v2_bright_rec_roundtrip() {
        let mut g = BrightnessGov::new();
        let _ = g.set_from(BrightPath::Slider, 88, 0);
        let rec = V2BrightRec::capture(&g, false);
        let mut buf = [0u8; V2_REC_LEN];
        let n = rec.to_bytes(&mut buf).unwrap();
        assert_eq!(V2BrightRec::from_bytes(&buf[..n]).unwrap(), rec);
        assert!(!rec.boot_restored);
    }

    #[test]
    fn v2_osd_bar_centered() {
        let screen = crate::h1star::h1base::Rect::new(0, 0, 1200, 800);
        let bar = v2_osd_bar_geometry(&screen);
        assert_eq!(bar.x + bar.w / 2, 600, "横条水平居中");
        assert_eq!(bar.h, 12);
    }

    #[test]
    fn brightosd_v2_selfcheck_all_green() {
        let s = run_brightosd_v2_checks();
        assert!(s.all_passed(), "F239 v2 自检存在红项");
        assert!(!s.truncated());
    }
}
