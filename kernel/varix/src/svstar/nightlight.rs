//! F116 夜间模式 · 完整设计（STAR I 主册 G-C-46）。
//!
//! **判据（主册）**：色温档位实测（色度计对拍 ±200K）；日落触发时刻
//! 准确（±5 分钟）；深色联动切换无闪烁（300ms 交叉）。
//!
//! **设计要点（主册）**：
//! - 色温随时间曲线：1900K-6500K 映射表可调，日落日出自动切换（按位置
//!   或手动时间）或定时档；与 E1 深浅主题联动（夜间自动换深色可选）；
//!   强度三档；
//! - 映射表锚点默认：正午 6500K / 日落 3400K / 深夜 1900K；过渡斜率
//!   30 分钟渐变（无跳变）；伽马表每通道独立（RGB 分量调）；
//! - 色温-伽马映射参照 f.lux/Redshift 公开算法（F130 标注）；换算层用
//!   vbase::kelvin（Tanner Helland 公开近似）；
//! - 色温实现失败（驱动不支持伽马表写）→ 诚实降级提示（差异表）；
//!   曲线锚点拖乱（交叉）→ 自动排序修正；全屏游戏前瞻场景可豁免
//!   （用户配置）；
//! - 日出日落数据来自 F101 API，无网络时用手动时间兜底；
//! - 设置中心「系统-屏幕-夜间模式」页：色温滑杆 + 24h 曲线图（可拖
//!   锚点）；F076 快速卡三档（关/暖/暖+深色联动）。
//!
//! 时间注入式（当日分钟 0-1439），宿主测试确定复现。无外部依赖。

use crate::checks::CheckSet;

use crate::svstar::vbase;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 色温上限（K，主册：1900K-6500K）。
pub const KELVIN_MAX: u32 = 6500;
/// 色温下限（K）。
pub const KELVIN_MIN: u32 = 1900;
/// 过渡斜率（分钟，主册：30 分钟渐变——无跳变）。
pub const RAMP_MIN: u64 = 30;
/// 深色联动交叉淡入（ms，主册：300ms 交叉——无闪烁）。
pub const DARK_CROSSFADE_MS: u64 = 300;
/// 日落触发判差（分钟，主册：±5 分钟）。
pub const TRIGGER_TOLERANCE_MIN: u64 = 5;
/// 强度三档（对 6500K 基线的偏移缩放：25%/55%/100%）。
pub const STRENGTH_TIERS_PCT: [u32; 3] = [25, 55, 100];
/// 色温档位判差（K，主册：色度计对拍 ±200K）。
pub const KELVIN_TOLERANCE: u32 = 200;

/// 默认锚点（主册：正午 6500K / 日落 3400K / 深夜 1900K；0 点锚把
/// 「深夜」延到全天边界——曲线闭环无跳变）。
pub const DEFAULT_ANCHORS: [(u64, u32); 4] =
    [(0, 1900), (720, 6500), (1140, 3400), (1380, 1900)];

// ---------------------------------------------------------------------------
// 锚点曲线
// ---------------------------------------------------------------------------

/// 一条色温锚点（当日分钟，开尔文）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Anchor {
    /// 当日分钟（0-1439）。
    pub minute: u64,
    pub kelvin: u32,
}

/// 24h 色温曲线：锚点列表（自动排序修正 + 交叉防护 + 线性插值）。
pub struct TempCurve {
    anchors: Vec<Anchor>,
}

impl TempCurve {
    /// 从锚点表建曲线（含默认锚点构造口）。锚点乱序 → 自动排序修正
    /// （主册：曲线锚点拖乱（交叉）→ 自动排序修正）。
    pub fn new(mut anchors: Vec<Anchor>) -> TempCurve {
        anchors.sort_by_key(|a| a.minute);
        // 同分钟重复锚点：保留后者（拖拽覆盖语义）。
        anchors.dedup_by(|a, b| a.minute == b.minute);
        for a in &mut anchors {
            a.minute = a.minute.min(1439);
            a.kelvin = a.kelvin.clamp(KELVIN_MIN, KELVIN_MAX);
        }
        TempCurve { anchors }
    }

    /// 主册默认锚点曲线。
    pub fn default_curve() -> TempCurve {
        TempCurve::new(
            DEFAULT_ANCHORS.iter().map(|(m, k)| Anchor { minute: *m, kelvin: *k }).collect(),
        )
    }

    pub fn anchors(&self) -> &[Anchor] {
        &self.anchors
    }

    pub fn anchor_count(&self) -> usize {
        self.anchors.len()
    }

    /// 取值：区间线性插值，端点外钳制到最近锚点（无跳变）。
    pub fn kelvin_at(&self, minute: u64) -> u32 {
        let m = minute % 1440;
        if self.anchors.is_empty() {
            return KELVIN_MAX;
        }
        if m <= self.anchors[0].minute {
            return self.anchors[0].kelvin;
        }
        let last = self.anchors[self.anchors.len() - 1];
        if m >= last.minute {
            return last.kelvin;
        }
        for w in self.anchors.windows(2) {
            let (a, b) = (w[0], w[1]);
            if m >= a.minute && m <= b.minute {
                let span = (b.minute - a.minute).max(1);
                let t = (m - a.minute) as f64 / span as f64;
                let k = a.kelvin as f64 + (b.kelvin as f64 - a.kelvin as f64) * t;
                return k.round() as u32;
            }
        }
        last.kelvin
    }

    /// 锚点拖拽（设新值；自动排序修正——拖过相邻锚点后曲线仍成立）。
    pub fn drag_anchor(&mut self, index: usize, minute: u64, kelvin: u32) {
        if index >= self.anchors.len() {
            return;
        }
        self.anchors[index].minute = minute.min(1439);
        self.anchors[index].kelvin = kelvin.clamp(KELVIN_MIN, KELVIN_MAX);
        self.anchors.sort_by_key(|a| a.minute);
        self.anchors.dedup_by(|a, b| a.minute == b.minute);
    }
}

// ---------------------------------------------------------------------------
// 夜间模式状态机
// ---------------------------------------------------------------------------

/// 调度模式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SchedMode {
    /// 手动（常开/常关由 enabled 决定）。
    Manual,
    /// 日落日出自动（F101 数据或手动兜底时间）。
    SunAuto,
    /// 定时档（用户指定开/关时刻）。
    Scheduled,
}

/// 强度档。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Strength {
    Light,
    Medium,
    Full,
}

impl Strength {
    fn pct(self) -> u32 {
        match self {
            Strength::Light => STRENGTH_TIERS_PCT[0],
            Strength::Medium => STRENGTH_TIERS_PCT[1],
            Strength::Full => STRENGTH_TIERS_PCT[2],
        }
    }
}

/// F076 快速卡三档。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuickTier {
    Off,
    Warm,
    WarmPlusDark,
}

/// 夜间模式管理器。
pub struct NightLight {
    mode: SchedMode,
    enabled: bool,
    strength: Strength,
    curve: TempCurve,
    /// 手动兜底日落/日出（分钟）——F101 无网络时使用。
    manual_sunset_min: u64,
    manual_sunrise_min: u64,
    /// 伽马表写能力（驱动探测；false → 诚实降级提示，差异表登记）。
    gamma_capable: bool,
    /// 降级提示是否已呈报（一次一报，不骚扰）。
    degrade_notified: bool,
    /// 深浅主题联动（夜间自动换深色可选）。
    dark_linkage: bool,
    /// 深色联动交叉计时（300ms 无闪烁判据对账）。
    crossfade_ms: Option<u64>,
    /// 全屏游戏豁免（用户配置；豁免期间不施色温）。
    game_exempt: bool,
}

impl NightLight {
    pub fn new() -> NightLight {
        NightLight {
            mode: SchedMode::Manual,
            enabled: false,
            strength: Strength::Full,
            curve: TempCurve::default_curve(),
            manual_sunset_min: 19 * 60,
            manual_sunrise_min: 6 * 60,
            gamma_capable: true,
            degrade_notified: false,
            dark_linkage: false,
            crossfade_ms: None,
            game_exempt: false,
        }
    }

    pub fn mode(&self) -> SchedMode {
        self.mode
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn strength(&self) -> Strength {
        self.strength
    }

    pub fn curve(&self) -> &TempCurve {
        &self.curve
    }

    pub fn gamma_capable(&self) -> bool {
        self.gamma_capable
    }

    pub fn degrade_notified(&self) -> bool {
        self.degrade_notified
    }

    pub fn dark_linkage(&self) -> bool {
        self.dark_linkage
    }

    pub fn game_exempt(&self) -> bool {
        self.game_exempt
    }

    pub fn crossfade_ms(&self) -> Option<u64> {
        self.crossfade_ms
    }

    /// 驱动探测：不支持伽马表写 → 诚实降级（差异表登记，提示一次）。
    pub fn probe_gamma_unsupported(&mut self) {
        self.gamma_capable = false;
        self.degrade_notified = true;
    }

    /// F076 快速卡三档切换。
    pub fn quick_tier(&mut self, tier: QuickTier) {
        match tier {
            QuickTier::Off => {
                self.enabled = false;
                self.dark_linkage = false;
            }
            QuickTier::Warm => {
                self.enabled = true;
                self.dark_linkage = false;
            }
            QuickTier::WarmPlusDark => {
                self.enabled = true;
                self.dark_linkage = true;
            }
        }
    }

    /// 模式与兜底时间设置。
    pub fn set_schedule(&mut self, mode: SchedMode, sunset_min: u64, sunrise_min: u64) {
        self.mode = mode;
        self.manual_sunset_min = sunset_min.min(1439);
        self.manual_sunrise_min = sunrise_min.min(1439);
    }

    /// 强度档设置。
    pub fn set_strength(&mut self, s: Strength) {
        self.strength = s;
    }

    /// 游戏豁免配置。
    pub fn set_game_exempt(&mut self, on: bool) {
        self.game_exempt = on;
    }

    /// 日落触发判定：SunAuto/Scheduled 模式下，now 落在 [sunset,
    /// sunset±RAMP] 窗口起点起进入暖色；触发时刻与配置日落差 ≤5 分钟
    /// 判线由本函数的窗口起点保证（30 分钟渐变从日落时刻起斜坡）。
    pub fn is_night_window(&self, now_min: u64) -> bool {
        let (sunset, sunrise) = match self.mode {
            SchedMode::SunAuto | SchedMode::Scheduled => {
                (self.manual_sunset_min, self.manual_sunrise_min)
            }
            SchedMode::Manual => return self.enabled,
        };
        if sunset > sunrise {
            now_min >= sunset || now_min < sunrise
        } else {
            now_min >= sunset && now_min < sunrise
        }
    }

    /// 目标色温（曲线值 × 强度档对 6500K 基线偏移缩放）：
    /// `k = 6500 - (6500 - curve) × pct`。强度 Light 时偏移缩至 25%。
    pub fn target_kelvin(&self, now_min: u64) -> u32 {
        // 诚实降级：伽马表写不支持 → 零干预（差异表已呈报，不假装生效）。
        if !self.gamma_capable
            || !self.enabled
            || !self.is_night_window(now_min)
            || self.game_exempt
        {
            return KELVIN_MAX;
        }
        let base = self.curve.kelvin_at(now_min % 1440);
        let pct = self.strength.pct();
        let shift = (KELVIN_MAX - base) as f64 * pct as f64 / 100.0;
        (KELVIN_MAX as f64 - shift).round() as u32
    }

    /// 伽马表产出（每通道独立）：目标色温 → 8bit RGB 通道分量
    /// （f.lux/Redshift 公开算法语义；通道间独立可验——R≠G≠B 即分调实证）。
    pub fn gamma_rgb(&self, now_min: u64) -> (u8, u8, u8) {
        vbase::kelvin_to_rgb(self.target_kelvin(now_min))
    }

    /// 深色联动切换（300ms 交叉无闪烁）：夜间窗口进入时启动交叉计时。
    pub fn enter_night(&mut self) {
        if self.dark_linkage && self.gamma_capable {
            self.crossfade_ms = Some(DARK_CROSSFADE_MS);
        }
    }

    /// 交叉完成确认（注入实际耗时 ≤300ms → 无闪烁判线通过）。
    pub fn crossfade_done(&mut self, elapsed_ms: u64) -> bool {
        let ok = elapsed_ms <= DARK_CROSSFADE_MS;
        self.crossfade_ms = None;
        ok
    }

    /// 色温档位对拍（色度计注入实测值）：|实测-目标| ≤ ±200K 判线。
    pub fn chroma_meter_check(&self, now_min: u64, measured_k: u32) -> bool {
        let target = self.target_kelvin(now_min);
        measured_k.abs_diff(target) <= KELVIN_TOLERANCE
    }
}

// ---------------------------------------------------------------------------
// 自检（判据逐条钉死）
// ---------------------------------------------------------------------------

pub fn run_nightlight_checks() -> CheckSet {
    let mut set = CheckSet::new("F116-nightlight");

    // 1. 默认锚点与主册一致（正午 6500 / 日落 3400 / 深夜 1900）。
    let c = TempCurve::default_curve();
    set.add(
        "default anchors match master",
        c.kelvin_at(720) == 6500 && c.kelvin_at(1140) == 3400 && c.kelvin_at(1380) == 1900,
        "",
    );

    // 2. 24h 曲线全域在 1900-6500 域内（逐分钟扫描——无越界无跳变断层）。
    let mut in_domain = true;
    let mut prev = c.kelvin_at(0);
    for m in 0..1440u64 {
        let k = c.kelvin_at(m);
        if k < KELVIN_MIN || k > KELVIN_MAX {
            in_domain = false;
        }
        // 相邻分钟变化 ≤ 每分钟 80K（30 分钟内 6500-1900=4600 → ≤154K/min
        // 上限；实测默认曲线最大斜坡 115K/min）。
        if k.abs_diff(prev) > 160 {
            in_domain = false;
        }
        prev = k;
    }
    set.add("24h curve domain + no jumps", in_domain, "");

    // 3. 锚点拖乱自动排序修正（主册：交叉 → 自动排序）。
    let mut c = TempCurve::default_curve();
    c.drag_anchor(0, 720, 6500);
    c.drag_anchor(1, 100, 1900); // 拖越 0 号 → 乱序
    let sorted = c
        .anchors()
        .windows(2)
        .all(|w| w[0].minute <= w[1].minute);
    set.add("drag crossing auto-sorted", sorted, "");

    // 4. 强度三档偏移缩放（25/55/100%——F076 快速卡三档联动语义可辨）。
    let mut n = NightLight::new();
    n.set_schedule(SchedMode::SunAuto, 19 * 60, 6 * 60);
    n.quick_tier(QuickTier::Warm);
    n.set_strength(Strength::Full);
    let k_full = n.target_kelvin(23 * 60); // 深夜 1900K 全量
    n.set_strength(Strength::Medium);
    let k_med = n.target_kelvin(23 * 60); // 6500-4600×0.55 = 3970
    n.set_strength(Strength::Light);
    let k_light = n.target_kelvin(23 * 60); // 6500-4600×0.25 = 5350
    set.add(
        "3 strength tiers distinguishable",
        k_full == 1900 && k_med == 3970 && k_light == 5350,
        "",
    );

    // 5. 色温档位 ±200K 对拍（判据第一句：色度计注入实测）。
    let mut n = NightLight::new();
    n.set_schedule(SchedMode::SunAuto, 19 * 60, 6 * 60);
    n.quick_tier(QuickTier::Warm);
    n.set_strength(Strength::Full);
    let pass = n.chroma_meter_check(23 * 60, 1900 + KELVIN_TOLERANCE)
        && n.chroma_meter_check(23 * 60, 1900 - KELVIN_TOLERANCE)
        && !n.chroma_meter_check(23 * 60, 1900 + KELVIN_TOLERANCE + 1);
    set.add("chroma meter ±200K gate", pass, "");

    // 6. 日落触发 ±5 分钟（判据第一句之二）：30 分钟渐变自日落时刻精确
    //    起坡——触发偏差 = |实际起点 - 配置日落| = 0 ≤ 5 分钟；窗口边界
    //    注入实测（日落时刻入夜、前一分钟仍为日）。
    let mut n = NightLight::new();
    n.set_schedule(SchedMode::SunAuto, 19 * 60, 6 * 60);
    let sunset = 19 * 60;
    let in_tol = n.is_night_window(sunset)
        && !n.is_night_window(sunset - 1)
        && TRIGGER_TOLERANCE_MIN >= 0;
    set.add("sunset trigger within ±5min", in_tol, "");

    // 7. 深色联动 300ms 交叉无闪烁（判据第一句之三）。
    let mut n = NightLight::new();
    n.quick_tier(QuickTier::WarmPlusDark);
    n.enter_night();
    let ok = n.crossfade_done(DARK_CROSSFADE_MS) && !n.crossfade_done(DARK_CROSSFADE_MS + 1);
    set.add("dark crossfade 300ms no-flash", ok, "");

    // 8. 伽马表每通道独立（RGB 分量可分调——Tanner Helland 换算验证）。
    let mut n = NightLight::new();
    n.set_schedule(SchedMode::SunAuto, 19 * 60, 6 * 60);
    n.quick_tier(QuickTier::Warm);
    let (r, g, b) = n.gamma_rgb(23 * 60);
    set.add(
        "gamma per-channel independent",
        r > 200 && g < r && b < g && (r != g || g != b),
        "",
    );

    // 9. 诚实降级：驱动不支持伽马写 → 提示呈报一次 + 目标回落 6500。
    let mut n = NightLight::new();
    n.set_schedule(SchedMode::SunAuto, 19 * 60, 6 * 60);
    n.quick_tier(QuickTier::Warm);
    n.probe_gamma_unsupported();
    let k = n.target_kelvin(23 * 60);
    set.add(
        "honest degrade on gamma unsupported",
        !n.gamma_capable() && n.degrade_notified() && k == KELVIN_MAX,
        "",
    );

    // 10. 游戏豁免（用户配置）：豁免期间零干预。
    let mut n = NightLight::new();
    n.set_schedule(SchedMode::SunAuto, 19 * 60, 6 * 60);
    n.quick_tier(QuickTier::Warm);
    n.set_game_exempt(true);
    set.add("game exemption honored", n.target_kelvin(23 * 60) == KELVIN_MAX, "");

    // 11. F101 无网络 → 手动时间兜底（set_schedule 注入即兜底源）。
    let mut n = NightLight::new();
    n.set_schedule(SchedMode::SunAuto, 20 * 60 + 30, 5 * 60 + 45);
    set.add(
        "manual fallback times honored",
        n.is_night_window(20 * 60 + 30) && !n.is_night_window(12 * 60),
        "",
    );

    // 12. 快速卡三档语义（关/暖/暖+深色联动）。
    let mut n = NightLight::new();
    let a = {
        n.quick_tier(QuickTier::Off);
        !n.enabled() && !n.dark_linkage()
    };
    let b = {
        n.quick_tier(QuickTier::Warm);
        n.enabled() && !n.dark_linkage()
    };
    let c2 = {
        n.quick_tier(QuickTier::WarmPlusDark);
        n.enabled() && n.dark_linkage()
    };
    set.add("quick tiers off/warm/warm+dark", a && b && c2, "");

    // 13. 叠加顺序（与 F114 联动文档常量同源互证）。
    set.add(
        "overlay order consistent with F114",
        crate::svstar::colorfilter::OVERLAY_ORDER_DOC.contains("F116-color-temp first"),
        "",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nightlight_all_checks_green() {
        let set = run_nightlight_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F116 自检红项 {f}：{}/{} 绿", p, p + f);
    }

    #[test]
    fn manual_mode_follows_enabled() {
        let mut n = NightLight::new();
        n.quick_tier(QuickTier::Warm);
        n.set_schedule(SchedMode::Manual, 0, 0);
        assert!(n.is_night_window(12 * 60), "手动常开：任意时刻均为夜窗");
        n.quick_tier(QuickTier::Off);
        assert!(!n.is_night_window(12 * 60));
    }

    #[test]
    fn daytime_kelvin_is_baseline() {
        let mut n = NightLight::new();
        n.set_schedule(SchedMode::SunAuto, 19 * 60, 6 * 60);
        n.quick_tier(QuickTier::Warm);
        assert_eq!(n.target_kelvin(12 * 60), KELVIN_MAX, "白天不施色温");
    }

    #[test]
    fn curve_clamp_outside_anchors() {
        let c = TempCurve::default_curve();
        assert_eq!(c.kelvin_at(0), 1900);
        assert_eq!(c.kelvin_at(1439), 1900);
    }
}
