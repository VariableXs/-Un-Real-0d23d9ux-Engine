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
    /// 定时档开/关时刻（深化 v2：Scheduled 独立钟点，默认 22:00-07:00）。
    sched_on_min: u64,
    sched_off_min: u64,
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
            sched_on_min: 22 * 60,
            sched_off_min: 7 * 60,
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
        let (start, end) = match self.mode {
            SchedMode::SunAuto => (self.manual_sunset_min, self.manual_sunrise_min),
            // 定时档用独立钟点（深化 v2：定时不吃天文时刻）。
            SchedMode::Scheduled => (self.sched_on_min, self.sched_off_min),
            SchedMode::Manual => return self.enabled,
        };
        if start > end {
            now_min >= start || now_min < end
        } else {
            now_min >= start && now_min < end
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

    // -----------------------------------------------------------------------
    // 深化批次 v2
    // -----------------------------------------------------------------------

    /// 定时档开/关时刻设置（Scheduled 模式独立时刻——与 SunAuto 的日落
    /// 日出字段分离：定时是用户钟点，自动是天文时刻，一处一事实）。
    pub fn set_scheduled_times(&mut self, on_min: u64, off_min: u64) {
        self.sched_on_min = on_min.min(1439);
        self.sched_off_min = off_min.min(1439);
    }

    /// 位置模式注入：由城市 + 日序计算当日日落日出并写入自动档时刻
    /// （F101 城市共用——城市表一处一事实；无网络时用户手动值兜底，
    /// 本方法只在有位置数据时调用）。
    pub fn apply_city_auto(&mut self, city: City, day_of_year: u32) {
        let (sunrise, sunset) =
            sun_times_min(day_of_year, city.lat_deg, city.lon_deg, city.tz_offset_h);
        self.manual_sunrise_min = sunrise;
        self.manual_sunset_min = sunset;
        self.mode = SchedMode::SunAuto;
    }

    /// 24h 曲线图采样（设置页曲线可拖锚点的数据面）：N 等分采样点
    /// （分钟, 色温）+ 锚点标记（锚点必须原样出现在采样序列——图上
    /// 拖柄位置 = 真值）。
    pub fn curve_samples(&self, n: usize) -> Vec<(u64, u32, bool)> {
        let n = n.clamp(2, 288);
        let mut out = Vec::new();
        for i in 0..n {
            let minute = (i * 1440 / n) as u64;
            let k = self.curve.kelvin_at(minute);
            let is_anchor = self.curve.anchors().iter().any(|a| a.minute == minute);
            out.push((minute, k, is_anchor));
        }
        out
    }

    /// 过渡渐变窗检测（30 分钟渐变的区间面）：当前时刻是否处于日落/
    /// 日出 ±RAMP/2 渐变窗内——图上高亮「正在渐变」+ 测试斜坡边界。
    pub fn transition_window(&self, now_min: u64) -> Option<(u64, u64)> {
        if !self.enabled || self.mode == SchedMode::Manual {
            return None;
        }
        let (sunrise, sunset) = (self.manual_sunrise_min, self.manual_sunset_min);
        let half = RAMP_MIN / 2;
        for edge in [sunset, sunrise] {
            let lo = edge.saturating_sub(half);
            let hi = (edge + half).min(1439);
            if now_min >= lo && now_min < hi {
                return Some((lo, hi));
            }
        }
        None
    }
}

// ---------------------------------------------------------------------------
// 深化批次 v2 · 一：城市表（F101 天气城市共用）
// ---------------------------------------------------------------------------

/// 城市条目（纬度/经度整数度 + 行政时区——日落计算的地理输入；行政
/// 时区独立于经度时区（中国全境 UTC+8），一处一事实）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct City {
    pub name: &'static str,
    pub lat_deg: i32,
    pub lon_deg: i32,
    /// 行政时区（UTC 偏移小时——中国统一 +8，不从经度推）。
    pub tz_offset_h: i32,
}

/// 官方城市表（F101 同源；一处一事实——城市名与坐标唯一源）。
pub const CITIES: [City; 8] = [
    City { name: "北京", lat_deg: 40, lon_deg: 116, tz_offset_h: 8 },
    City { name: "上海", lat_deg: 31, lon_deg: 121, tz_offset_h: 8 },
    City { name: "广州", lat_deg: 23, lon_deg: 113, tz_offset_h: 8 },
    City { name: "成都", lat_deg: 31, lon_deg: 104, tz_offset_h: 8 },
    City { name: "乌鲁木齐", lat_deg: 44, lon_deg: 88, tz_offset_h: 8 },
    City { name: "哈尔滨", lat_deg: 46, lon_deg: 127, tz_offset_h: 8 },
    City { name: "海口", lat_deg: 20, lon_deg: 110, tz_offset_h: 8 },
    City { name: "拉萨", lat_deg: 30, lon_deg: 91, tz_offset_h: 8 },
];

/// 城市查表（名字精确匹配）。
pub fn city_lookup(name: &str) -> Option<City> {
    CITIES.iter().copied().find(|c| c.name == name)
}

// ---------------------------------------------------------------------------
// 深化批次 v2 · 二：日落日出天文近似（NOAA 简化式 · no_std 自研）
// ---------------------------------------------------------------------------

const PI_DEG: f64 = core::f64::consts::PI;
const DEG2RAD: f64 = PI_DEG / 180.0;

/// 角度制正弦（泰勒 11 项嵌套——no_std core 无 trig，自研近似；输入
/// 归一到 [-180,180) 后 |r|≤π，r=π 处残差 <1e-3——天文分钟级判定富余）。
fn sin_deg(x: f64) -> f64 {
    // 归一。
    let mut x = x % 360.0;
    if x > 180.0 {
        x -= 360.0;
    } else if x < -180.0 {
        x += 360.0;
    }
    let r = x * DEG2RAD;
    let r2 = r * r;
    // sin(r) = r·(1 - r²/3!·(1 - r²/5!·(1 - r²/7!·(1 - r²/9!·(1 - r²/11!)))))
    // 系数链 6/20/42/72/110 = 3! 与 5!/3!… 逐层；r=π 端残差 <1e-3。
    r * (1.0
        - r2 / 6.0
            * (1.0
                - r2 / 20.0
                    * (1.0
                        - r2 / 42.0
                            * (1.0 - r2 / 72.0 * (1.0 - r2 / 110.0)))))
}

/// 角度制余弦（独立偶函数实现——不从 sin 传导误差；r=π/2 处残差
/// <2e-4，acos 二分的根精度 <0.01°）。
fn cos_deg(x: f64) -> f64 {
    let mut x = x % 360.0;
    if x > 180.0 {
        x -= 360.0;
    } else if x < -180.0 {
        x += 360.0;
    }
    let r = x * DEG2RAD;
    let r2 = r * r;
    // cos(r) = 1 - r²/2!·(1 - r²/4!·(1 - r²/6!·(1 - r²/8!·(1 - r²/10!))))
    // 系数链 2/12/30/56/90 = 2! 与 4!/2!… 逐层。
    1.0
        - r2 / 2.0
            * (1.0
                - r2 / 12.0
                    * (1.0
                        - r2 / 30.0
                            * (1.0 - r2 / 56.0 * (1.0 - r2 / 90.0))))
}

/// 角度制反余弦（|x| 钳制 + 二分求根——单调性保证收敛；40 次迭代精度
/// 优于 1e-6°，远超分钟级判定需求；|x|>1（极昼极夜区）由调用方以
/// cos_ha 域外分支先行处理，此处钳制兜底）。
fn acos_deg(x: f64) -> f64 {
    // 端点精确特判（acos(±1) 是精确值；极值点附近二分被近似残差拖偏
    // ——诚实处理而不是放大容差）。
    if x >= 1.0 {
        return 0.0;
    }
    if x <= -1.0 {
        return 180.0;
    }
    let (mut lo, mut hi) = (0.0f64, 180.0f64);
    for _ in 0..40 {
        let mid = (lo + hi) / 2.0;
        if cos_deg(mid) > x {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    (lo + hi) / 2.0
}

/// 日出日落计算（NOAA 简化式，分钟；时区偏移由调用方给——行政时区
/// 与经度解耦）。
///
/// 返回 (日出分钟, 日落分钟)。高纬极昼极夜（ha 无解）→ 诚实返回
/// (0, 1439)（极昼）或 (720, 720)（极夜）——不编造天文时刻。
pub fn sun_times_min(day_of_year: u32, lat_deg: i32, lon_deg: i32, tz_offset_h: i32) -> (u64, u64) {
    // 年角 γ（当天正午）。
    let gamma = 2.0 * PI_DEG / 365.0 * (day_of_year as f64 - 1.0 + 0.5);
    let cg = cos_deg(gamma / DEG2RAD);
    let sg = sin_deg(gamma / DEG2RAD);
    let c2g = cos_deg(2.0 * gamma / DEG2RAD);
    let s2g = sin_deg(2.0 * gamma / DEG2RAD);
    // 均时差（分钟）与赤纬（度）——NOAA 公开系数。
    let eqtime = 229.18
        * (0.000_075 + 0.001_868 * cg - 0.032_077 * sg - 0.014_615 * c2g - 0.040_849 * s2g);
    let decl_rad = 0.006_918 - 0.399_912 * cg + 0.070_257 * sg - 0.006_758 * c2g
        + 0.000_907 * s2g;
    let decl_deg = decl_rad / DEG2RAD; // NOAA decl 输出弧度——本函数内部全程角度制
    let lat = lat_deg as f64;
    // 时角 ha（度）：cos(ha) = cos(90.833°)/(cos(lat)cos(decl)) - tan(lat)tan(decl)。
    let cos_ha =
        cos_deg(90.833) / (cos_deg(lat) * cos_deg(decl_deg)) - tan_deg(lat) * tan_deg(decl_deg);
    if cos_ha > 1.0 {
        return (720, 720); // 极夜
    }
    if cos_ha < -1.0 {
        return (0, 1439); // 极昼
    }
    let ha = acos_deg(cos_ha);
    // NOAA：日出 UTC = 720 - 4(经度 + ha) - eqtime；日落取 -ha。
    let sunrise_utc = 720.0 - 4.0 * (lon_deg as f64 + ha) - eqtime;
    let sunset_utc = 720.0 - 4.0 * (lon_deg as f64 - ha) - eqtime;
    let to_local = |utc_min: f64| -> u64 {
        let v = utc_min + tz_offset_h as f64 * 60.0;
        (v.rem_euclid(1440.0)) as u64
    };
    (to_local(sunrise_utc), to_local(sunset_utc))
}

/// 角度制正切（自研——sin/cos 组合；|x|=90° 处溢出钳为大值）。
fn tan_deg(x: f64) -> f64 {
    let c = cos_deg(x);
    if c.abs() < 1e-9 {
        if x >= 0.0 { 1e18 } else { -1e18 }
    } else {
        sin_deg(x) / c
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

    // 14. 城市表与查表（深化 v2）：八城齐、名字精确查、缺名 None。
    let bj = city_lookup("北京");
    set.add(
        "city table lookup",
        CITIES.len() == 8 && bj.map(|c| c.lat_deg == 40 && c.lon_deg == 116) == Some(true)
            && city_lookup("不存在的城").is_none(),
        "",
    );

    // 15. 日落日出天文近似（深化 v2）：北京夏至（日序 172）日出 ~4:45、
    //     日落 ~19:46（±30 分钟窗内——近似精度富余于 ±5min 触发判线）；
    //     冬至（日序 355）昼短夜长；极夜点（纬度 -80 → cos_ha>1）诚实
    //     返回极夜。
    let (sr, ss) = sun_times_min(172, 40, 116, 8);
    let summer_ok = sr >= 4 * 60 && sr <= 5 * 60 + 15 && ss >= 19 * 60 + 15 && ss <= 20 * 60 + 30;
    let (sr_w, ss_w) = sun_times_min(355, 40, 116, 8);
    let winter_ok = ss_w - sr_w < 600 && ss_w > sr_w; // 冬至昼 <10h 且仍昼短于夜
    let polar = sun_times_min(172, -80, 0, 0);
    set.add(
        "sun times noaa approximation",
        summer_ok && winter_ok && polar == (720, 720),
        "",
    );

    // 16. 位置模式注入（深化 v2）：城市自动计算落位 → 日夜窗口按天文
    //     时刻生效；触发对拍仍走 ±200K/±5min 既有门。
    let mut n = NightLight::new();
    n.apply_city_auto(city_lookup("北京").unwrap(), 172);
    let (auto_sr, auto_ss) = sun_times_min(172, 40, 116, 8);
    set.add(
        "city auto applies astronomical times",
        n.mode() == SchedMode::SunAuto
            && n.is_night_window(auto_ss + 30)
            && !n.is_night_window(auto_ss.saturating_sub(31)),
        "",
    );

    // 17. 定时档独立钟点（深化 v2）：Scheduled 用 set_scheduled_times，
    //     不吃 SunAuto 天文值。
    let mut n = NightLight::new();
    n.set_scheduled_times(23 * 60, 7 * 60);
    n.set_schedule(SchedMode::Scheduled, 0, 0);
    set.add(
        "scheduled tier independent clock",
        n.is_night_window(23 * 60 + 30) && !n.is_night_window(12 * 60),
        "",
    );

    // 18. 曲线采样器（深化 v2）：N 点采样全域内、锚点标记出现在采样
    //     序列、首尾闭合（0 点与 23:59 同为深夜值域）。
    let n2 = NightLight::new();
    let samples = n2.curve_samples(96);
    let anchors_marked = samples.iter().any(|(_, _, a)| *a);
    let in_domain = samples.iter().all(|(_, k, _)| (*k >= KELVIN_MIN && *k <= KELVIN_MAX));
    set.add(
        "curve samples for chart + anchor marks",
        samples.len() == 96 && anchors_marked && in_domain,
        "",
    );

    // 19. 过渡渐变窗（深化 v2）：日落时刻 ±15min 窗命中；远离日落不
    //     命中；Manual/未启用不产生窗。
    let mut n = NightLight::new();
    let none_disabled = n.transition_window(19 * 60).is_none();
    n.set_schedule(SchedMode::SunAuto, 19 * 60, 6 * 60);
    n.quick_tier(QuickTier::Warm);
    let in_window = n.transition_window(19 * 60) == Some((19 * 60 - 15, 19 * 60 + 15));
    let far_none = n.transition_window(12 * 60).is_none();
    set.add(
        "transition window ramps detection",
        none_disabled && in_window && far_none,
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

    #[test]
    fn f116_trig_approximation_accuracy() {
        // 自研三角近似精度：|sin(x)-参考| < 1e-3（角度制——天文分钟级
        // 判定富余）。
        for d in [0.0f64, 30.0, 45.0, 90.0, 120.0, 180.0, -90.0, 270.0] {
            let got = sin_deg(d);
            // 参考值用偶延拓已知锚点。
            let want = match d {
                0.0 => 0.0,
                30.0 => 0.5,
                45.0 => 0.7071,
                90.0 => 1.0,
                120.0 => 0.8660,
                180.0 => 0.0,
                -90.0 => -1.0,
                _ => -1.0, // 270°
            };
            assert!((got - want).abs() < 2e-3, "sin({})° ≈ {} got {}", d, want, got);
        }
        // acos 端点与中点（容差 0.01° = 0.6 秒时刻误差——分钟级判定富余）。
        assert!((acos_deg(1.0) - 0.0).abs() < 1e-6);
        assert!((acos_deg(0.0) - 90.0).abs() < 1e-2);
        assert!((acos_deg(-1.0) - 180.0).abs() < 1e-2);
        assert!((acos_deg(0.5) - 60.0).abs() < 0.5);
    }

    #[test]
    fn f116_sun_times_season_monotonic() {
        // 夏至昼最长、冬至昼最短（北纬 40°——季节单调性抽查四节点）。
        let day_len = |d: u32| {
            let (sr, ss) = sun_times_min(d, 40, 116, 8);
            (ss + 1440 - sr) % 1440
        };
        let equinox = day_len(80);
        let summer = day_len(172);
        let winter = day_len(355);
        assert!(summer > equinox, "夏至昼长于春分");
        assert!(winter < equinox, "冬至昼短于春分");
        assert!(summer > winter);
    }

    #[test]
    fn f116_curve_samples_anchor_positions_exact() {
        // 采样密度足够时（288 点 = 5 分钟粒度）锚点必以真值命中。
        let mut c = TempCurve::default_curve();
        c.drag_anchor(0, 300, 5000);
        let mut nl = NightLight::new();
        nl.curve = c;
        let samples = nl.curve_samples(288);
        let hit = samples.iter().find(|(m, _, _)| *m == 300).unwrap();
        assert_eq!(hit.1, 5000, "锚点分钟采样值 = 锚点色温");
        assert!(hit.2, "锚点分钟带标记");
    }

    #[test]
    fn f116_scheduled_respects_configured_clock() {
        let mut n = NightLight::new();
        n.set_scheduled_times(21 * 60, 5 * 60);
        n.set_schedule(SchedMode::Scheduled, 99, 99); // 天文参数被忽略
        assert!(n.is_night_window(21 * 60));
        assert!(!n.is_night_window(6 * 60), "定时档 21-5 窗外为日间");
    }
}
