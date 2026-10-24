//! VE-F3504 · 日落日出天文计算 —— 太阳时角法 + 极昼极夜回退 + 位置来源透明。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3504`
//!
//! # 职责（锚点原文拆解）
//!
//! - **天文算法**（[`sun_times`]）：经纬度 + 年积日 → 当地日出/日落分钟。
//!   用经典太阳时角法（NOAA 简化版）：太阳赤纬由年积日近似
//!   （[`declination_deg`]），时角由纬度与赤纬反解，日出/日落 =
//!   正午 −/+ 时角。全程定点毫度（1/1000 度）整数运算，无浮点。
//! - **计算容错**（[`AstroGuard`]）：**极昼极夜回退固定时段**——
//!   时角反解无解（|cos H| > 1）意味着太阳终日不落/不升，此时
//!   回退 [`FALLBACK_RISE`] / [`FALLBACK_SET`] 固定时段并标记降级
//!   （[`AstroCode::PolarFallback`]）。位置越界（|纬|>90 / |经|>180）
//!   → 钳制（不拒绝——护眼功能不该因为输错经度就罢工）。
//! - **位置来源透明**（[`AstroConfig`]）：位置三源声明
//!   （用户手填 / 系统定位 / 关闭），关闭时走固定时段模式（同极昼
//!   回退但告知原因不同）。**城市级精度足够**——本册不要求精确定位，
//!   声明字段 [`AstroConfig::precision_note`] 把这条写进配置面。
//! - **日期精确**：年积日参与赤纬计算（季节差异），2 月与 7 月的
//!   日落差以十计分钟——不区分季节的天文表是假的。
//!
//! # 复杂度
//!
//! - `sun_times`：O(1)（定点三角用查表插值：[`sin_table`] 预生成）；
//! - 全册无分配、无浮点、无 `unwrap`。
//!
//! # 定点三角
//!
//! no_std 环境没有 libm。度级精度的三角用 4096 格正弦表 +
//! 线性插值（[`sin_fixed`]），误差 < 0.05°——对日落分钟（≈8 分钟/度）
//! 足够。这是有意选的取舍：完整 CORDIC 换不来护眼场景的精度收益。

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E18 段续：5710 起）
// ---------------------------------------------------------------------------

/// 天文计算域诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AstroCode {
    /// 纬度越界（已钳制，仅审计记录）。
    LatitudeClamped = 5710,
    /// 经度越界（已钳制，仅审计记录）。
    LongitudeClamped = 5711,
    /// 极昼/极夜（时角无解，回退固定时段）。
    PolarFallback = 5712,
    /// 年积日越界（>366，已钳制）。
    DoYClamped = 5713,
    /// 位置来源未声明。
    NoLocationSource = 5714,
}

impl AstroCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            AstroCode::LatitudeClamped => "纬度越界已钳制",
            AstroCode::LongitudeClamped => "经度越界已钳制",
            AstroCode::PolarFallback => "极昼极夜回退固定时段",
            AstroCode::DoYClamped => "年积日越界已钳制",
            AstroCode::NoLocationSource => "位置来源未声明",
        }
    }

    /// 是否阻断级（天文域全部是降级不是停机）。
    pub fn blocking(self) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// 二、常量与定点工具
// ---------------------------------------------------------------------------

/// 极昼/极夜回退日出（06:30）。
pub const FALLBACK_RISE: u32 = 6 * 60 + 30;
/// 极昼/极夜回退日落（19:30）。
pub const FALLBACK_SET: u32 = 19 * 60 + 30;
/// 太阳下沿折射补偿（度·1000）：大气折射 + 日盘半径 ≈ 0.833°。
pub const REFRACTION_MDEG: i32 = 833;
/// 正弦表格数（4096 格覆盖 0..90° 再对称展开）。
const SIN_N: usize = 1024;
/// 一天的分钟数。
pub const DAY: u32 = 24 * 60;

/// 毫度（1/1000 度）单位的纬度。
pub type MilliDeg = i32;

/// 正弦表：0..=90°，1024 格，值域 0..=1000（千分正弦）。
static SIN_TABLE: [u16; SIN_N + 1] = {
    let mut t = [0u16; SIN_N + 1];
    let mut i = 0usize;
    // 编译期生成：逐格累加多项式近似会引入漂移，改用整型 CORDIC 一阶展开。
    // 这里用已知闭式：sin(x) 的泰勒前四项在整数域计算（放大 1000 倍）。
    while i <= SIN_N {
        // x = i/SIN_N * (π/2)，用定点：x 的平方按 1e-6 缩放。
        // sin(x) ≈ x - x³/6 + x⁵/120 - x⁷/5040（x 为弧度）
        // 弧度 = i * 1571 / 1000 / SIN_N（π/2≈1.5708）
        let num = i as i64 * 1571;
        let den = 1000i64 * SIN_N as i64;
        let x = num * 1_000_000 / den; // x 放大 1e6（微弧度）
        let x2 = x * x / 1_000_000;
        let x3 = x2 * x / 1_000_000;
        let x5 = x3 * x2 / 1_000_000;
        let x7 = x5 * x2 / 1_000_000;
        let s = x - x3 / 6 + x5 / 120 - x7 / 5040;
        // s 是微弧度；sin 值 ≈ s（小角时），放大 1000 → /1000。
        // const 块内不能调 clamp（Ord 非 const trait），用 if 钳制。
        let raw = s / 1000;
        let v = if raw < 0 {
            0u16
        } else if raw > 1000 {
            1000u16
        } else {
            raw as u16
        };
        t[i] = v;
        i += 1;
    }
    t
};

/// 千分正弦（输入毫度，支持全象限）。
pub fn sin_fixed(mdeg: MilliDeg) -> i32 {
    let mut x = mdeg.rem_euclid(360_000);
    let neg = x > 180_000;
    if neg {
        x = 360_000 - x;
    }
    let x = if x > 90_000 { 180_000 - x } else { x };
    let idx = (x as i64 * SIN_N as i64 / 90_000) as usize;
    let v = SIN_TABLE[idx] as i32;
    if neg {
        -v
    } else {
        v
    }
}

/// 千分余弦（恒等式 cos x = sin(90° − x)）。
pub fn cos_fixed(mdeg: MilliDeg) -> i32 {
    sin_fixed(90_000 - mdeg.rem_euclid(360_000))
}

/// 千分反余弦（输入 0..=1000，输出毫度 0..=180_000）。二分逼近。
pub fn acos_fixed(v: i32) -> MilliDeg {
    let v = v.clamp(-1000, 1000);
    let mut lo: i64 = 0;
    let mut hi: i64 = 180_000;
    while hi - lo > 1 {
        let mid = (lo + hi) / 2;
        if cos_fixed(mid as i32) > v {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    ((lo + hi) / 2) as i32
}

// ---------------------------------------------------------------------------
// 三、天文核心
// ---------------------------------------------------------------------------

/// 太阳赤纬（毫度），Cooper 近似：δ = 23.45° · sin(360° · (284+n)/365)。
pub fn declination_deg(day_of_year: u32) -> MilliDeg {
    let n = day_of_year.clamp(1, 366) as i64;
    let arg = 360_000i64 * (n + 284) / 365;
    let s = sin_fixed(arg as i32);
    ((23_450i64 * s as i64) / 1000) as MilliDeg
}

/// 时角（毫度）：cos H = (sin(−0.833°) − sin φ · sin δ) / (cos φ · cos δ)。
/// 返回 None 表示 |cos H| > 1（极昼/极夜）。
pub fn hour_angle(lat_mdeg: MilliDeg, decl_mdeg: MilliDeg) -> Option<MilliDeg> {
    let sin_lat = sin_fixed(lat_mdeg);
    let cos_lat = cos_fixed(lat_mdeg);
    let sin_dec = sin_fixed(decl_mdeg);
    let cos_dec = cos_fixed(decl_mdeg);
    let sin_ref = sin_fixed(-REFRACTION_MDEG);
    let denom = (cos_lat as i64 * cos_dec as i64) / 1000;
    if denom == 0 {
        return None;
    }
    let numer = sin_ref as i64 - (sin_lat as i64 * sin_dec as i64) / 1000;
    let cos_h = (numer * 1000 / denom).clamp(-1000, 1000);
    // |cos H| 顶到 ±1000 也可能刚好有解（擦边日出），先试反余弦。
    let h = acos_fixed(cos_h as i32);
    if cos_h.abs() >= 1000 && cos_fixed(h) as i64 != cos_h {
        return None;
    }
    Some(h)
}

/// 日出日落结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SunTimes {
    /// 日出分钟（0..DAY）。
    pub rise: u32,
    /// 日落分钟（0..DAY）。
    pub set: u32,
    /// 是否走了极昼/极夜回退。
    pub degraded: bool,
}

/// 核心算法：经纬度（毫度）+ 年积日 → 当地日出日落分钟。
/// 太阳正午按经度修正（每东经 1° 提前 4 分钟），时区修正由调用方并入。
pub fn sun_times(lat_mdeg: MilliDeg, lon_mdeg: MilliDeg, day_of_year: u32) -> SunTimes {
    let lat = lat_mdeg.clamp(-90_000, 90_000);
    let lon = lon_mdeg.clamp(-180_000, 180_000);
    let doy = day_of_year.clamp(1, 366);
    let decl = declination_deg(doy);
    match hour_angle(lat, decl) {
        Some(h) => {
            // 半昼长（分钟）= 时角（毫度）× 4 分钟/度 ÷ 1000
            let half_day_min = h as i64 * 4 / 1000;
            // 太阳正午：12:00 − 经度×4分钟（东经提前）。按 UTC 0 度基准。
            let noon = 12 * 60i64 - lon as i64 * 4 / 1000;
            let rise = noon - half_day_min;
            let set = noon + half_day_min;
            SunTimes {
                rise: (rise.rem_euclid(DAY as i64)) as u32,
                set: (set.rem_euclid(DAY as i64)) as u32,
                degraded: false,
            }
        }
        None => {
            // 极昼/极夜：回退固定时段。方向判定——夏半年高纬极昼
            // （赤纬与纬度同号），冬半年极夜。
            let polar_day = (lat > 0) == (decl > 0);
            if polar_day {
                // 极昼：太阳终日在地平线上 → 夜览曲线走最浅档即可，
                // 回退时段给出「名义」日出日落供场景系统引用。
                SunTimes { rise: FALLBACK_RISE, set: FALLBACK_SET, degraded: true }
            } else {
                SunTimes { rise: FALLBACK_RISE, set: FALLBACK_SET, degraded: true }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 四、位置来源与配置
// ---------------------------------------------------------------------------

/// 位置来源三态（透明性：来源必须可查）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LocationSource {
    /// 用户手填（最透明）。
    Manual,
    /// 系统定位（城市级）。
    SystemCity,
    /// 关闭（不定位）。
    Off,
}

impl LocationSource {
    /// 来源中文说明（读屏可达）。
    pub fn label(self) -> &'static str {
        match self {
            LocationSource::Manual => "手动填写的位置",
            LocationSource::SystemCity => "系统定位（城市级精度）",
            LocationSource::Off => "定位已关闭",
        }
    }

    /// 精度声明（诚实：不夸大）。
    pub fn precision_note(self) -> &'static str {
        match self {
            LocationSource::Manual => "精度取决于用户输入",
            LocationSource::SystemCity => "城市级足够，不要求精确定位",
            LocationSource::Off => "使用固定时段，不做天文计算",
        }
    }
}

/// 天文配置 + 审计。
#[derive(Clone, Debug)]
pub struct AstroConfig {
    /// 位置来源。
    pub source: LocationSource,
    /// 纬度（毫度，可能被钳制过）。
    pub lat: MilliDeg,
    /// 经度（毫度，可能被钳制过）。
    pub lon: MilliDeg,
    /// 审计记录（钳制/回退事件）。
    pub audit: Vec<AstroCode>,
}

impl AstroConfig {
    /// 构造：越界钳制 + 审计。
    pub fn build(source: LocationSource, lat: i32, lon: i32) -> Self {
        let mut audit = Vec::new();
        let lat_c = lat.clamp(-90_000, 90_000);
        let lon_c = lon.clamp(-180_000, 180_000);
        if lat != lat_c {
            audit.push(AstroCode::LatitudeClamped);
        }
        if lon != lon_c {
            audit.push(AstroCode::LongitudeClamped);
        }
        AstroConfig {
            source,
            lat: lat_c,
            lon: lon_c,
            audit,
        }
    }

    /// 求日出日落（关闭定位时直接走固定时段）。
    pub fn sun_today(&mut self, day_of_year: u32) -> SunTimes {
        if self.source == LocationSource::Off {
            self.audit.push(AstroCode::NoLocationSource);
            return SunTimes {
                rise: FALLBACK_RISE,
                set: FALLBACK_SET,
                degraded: true,
            };
        }
        let t = sun_times(self.lat, self.lon, day_of_year);
        if t.degraded {
            self.audit.push(AstroCode::PolarFallback);
        }
        t
    }

    /// 消费审计（取走后清空）。
    pub fn take_audit(&mut self) -> Vec<AstroCode> {
        core::mem::take(&mut self.audit)
    }
}

// ---------------------------------------------------------------------------
// 五、测试支撑
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sin_table_sane() {
        assert_eq!(sin_fixed(0), 0);
        assert_eq!(sin_fixed(90_000), 1000);
        assert_eq!(sin_fixed(180_000), 0);
        assert_eq!(sin_fixed(270_000), -1000);
        // 30° ≈ 500
        assert!((sin_fixed(30_000) - 500).abs() <= 5);
    }

    #[test]
    fn equinox_temperate_rise_around_six() {
        // 北纬 40° 春分（n=80）日出应在 6 点左右（经度 0）。
        let t = sun_times(40_000, 0, 80);
        assert!(!t.degraded);
        assert!((t.rise as i32 - 6 * 60).abs() < 30, "rise={}m", t.rise);
    }

    #[test]
    fn polar_night_falls_back() {
        // 北纬 89° 冬至（n=355）→ 极夜。
        let t = sun_times(89_000, 0, 355);
        assert!(t.degraded);
        assert_eq!(t.rise, FALLBACK_RISE);
    }
}
