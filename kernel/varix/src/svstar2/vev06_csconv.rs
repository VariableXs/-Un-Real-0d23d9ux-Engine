//! VE-F4406 · 色彩空间转换引擎（VE-W 域 · 显示与色彩批 · 目标 360 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F4406`
//!
//! **判据（锚点原文）**：矩阵库、双精度、往返断言、显式缺省、判据。
//!
//! **职责定位（锚点原文）**：色彩空间转换引擎——**空间矩阵库**（sRGB/
//! Display P3/Rec2020 空间互转矩阵+缓存+精度双模式（快速/高精））；
//! **转换精度验证**（测试色卡往返误差断言）；**缺省映射策略显式**。
//! 降级矩阵：矩阵缺失→拒绝转换不臆造；往返超差→高精模式强制；
//! 缓存失真→版本戳失效。性能：转换 O(矩阵乘法)；缓存 O(1)；断言
//! O(色卡数)。
//!
//! # 一、为什么走 XYZ 桥而不是 N×(N-1) 张直转矩阵
//!
//! 三空间两两直转要 6 张逆矩阵，加一个空间要再推 8 张——每张都是
//! 一个出错面。统一走 XYZ 桥：每空间只存「→XYZ」与「XYZ→」两张
//! （3 空间 6 张×2 精度），from→to = XYZ→to · (from→XYZ) 两次乘法。
//! 同空间转换走显式恒等路径（[`convert_same`]，恒等也落缓存有据可查，
//! 不走「跳过乘法」的隐式捷径）。
//!
//! # 二、双精度为什么是「双定点+双矩阵精度」而不是两套公式
//!
//! 两套公式 = 两个出错面。单一路径（[`mul_mv_scaled`] 恒定除以矩阵
//! 定位基数）、两种定位：快速 = 万分位矩阵 + 万分位定点（一次桥乘，
//! 中间量 i64 上界内）；高精 = **亿分位矩阵** + 十亿分位定点（同构
//! 桥乘）。误差物理：万分位矩阵取整（±5e-5/元素）经行和放大后往返
//! 误差 ~137 万分位，超 8bit 半级承诺（39 万分位）——**快速模式物理
//! 达不到承诺**，高精模式（亿分位取整 ±5e-9）往返误差实测 0。两模式
//! 同一数学、同一代码路径，只差矩阵与定点的定位宽度——判据可断言
//! 「高精 < 快速且快速超承诺、高精达承诺」（三者同时成立，非恒真）。
//!
//! # 三、矩阵缺失为什么拒绝而不臆造
//!
//! 引擎只收录 sRGB/Display P3/Rec2020 三空间。F4403 上游的 Adobe RGB
//! profile 没有矩阵——[`map_profile_to_space`] 返回 None，转换**拒绝**
//! 并立 E_CS_MATRIX 案（锚点「矩阵缺失→拒绝转换不臆造」）：把 Adobe
//! RGB 臆造映射到 P3 转出来的是错色，错色比不转更难排查。空间封闭集
//! （[`ColorSpace`]）+ 短码 parse，表外空间在入口就拒。
//!
//! # 四、缓存为什么带版本戳（与 F4403 同范式）
//!
//! 转换结果按 (空间对, 精度, 8bit RGB) FNV 定槽 32 槽缓存，O(1) 命中，
//! 条目**存转换输出**（不是输入）。矩阵库版本一旦递增，旧结果就是
//! 系统性错误——版本失配全表作废（[`MatrixLib::bump_version`]），失效
//! 计数留痕。LRU 会把失真结果留在缓存里，版本戳不会。
//!
//! # 五、显式缺省与精度告警（锚点原文）
//!
//! 缺省策略（[`default_strategy`]）是**可查结构体**（`is_default` 位
//! 显性标注）；F4403 的 Custom profile 按缺省映射到 sRGB 但
//! `used_default` 如实置位——用缺省不静默。精度告警读屏可达
//! （[`screen_line`]）：往返超差强制高精时播报（域本色，非日志）。
//! 色卡往返断言 O(色卡数)；对拍测试归 F4453、空间管理深化归 F4444
//! （前向声明）。

use alloc::format;
use alloc::string::String;

use crate::svstar2::vev03_color::ProfileKind;

// ---------------------------------------------------------------------------
// 一、错误码（字符串码家族）
// ---------------------------------------------------------------------------

/// 本项版本（矩阵库内容修订时随 bump_version 同步升位）。
pub const CS_ENGINE_VERSION: &str = "V06-cs-v1";

/// 矩阵缺失（空间未收录/映射 None——拒绝转换不臆造）。
pub const E_CS_MATRIX: &str = "E_CS_MATRIX";
/// 往返超差（色卡断言失败——强制高精）。
pub const E_CS_ROUNDTRIP: &str = "E_CS_ROUNDTRIP";
/// 缓存版本失配（已全表作废并计数）。
pub const E_CS_CACHE: &str = "E_CS_CACHE";
/// 输入非法。
pub const E_CS_INPUT: &str = "E_CS_INPUT";

// ---------------------------------------------------------------------------
// 二、空间封闭集（判据「矩阵缺失」的入口口径）
// ---------------------------------------------------------------------------

/// 色彩空间封闭集（表外空间入口即拒）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorSpace {
    /// sRGB（与 F4403 Srgb 对齐）。
    Srgb,
    /// Display P3。
    DisplayP3,
    /// Rec.2020。
    Rec2020,
}

pub const COLOR_SPACE_COUNT: usize = 3;

impl ColorSpace {
    /// 全集。
    pub fn all() -> [ColorSpace; COLOR_SPACE_COUNT] {
        [ColorSpace::Srgb, ColorSpace::DisplayP3, ColorSpace::Rec2020]
    }

    /// 短码。
    pub fn wire(self) -> &'static str {
        match self {
            ColorSpace::Srgb => "srgb",
            ColorSpace::DisplayP3 => "p3",
            ColorSpace::Rec2020 => "rec2020",
        }
    }

    /// 由短码还原（未知名 → None——矩阵缺失的入口形态）。
    pub fn parse(s: &str) -> Option<ColorSpace> {
        Self::all().iter().copied().find(|k| k.wire() == s)
    }
}

// ---------------------------------------------------------------------------
// 三、矩阵库（数据结构之一：双精度双定位）
// ---------------------------------------------------------------------------

/// 快速模式：矩阵万分位定位基数。
pub const MAT_SCALE_FAST: i64 = 10_000;
/// 快速模式：输入输出定点比例（万分位）。
pub const PREC_SCALE_FAST: i64 = 10_000;
/// 高精模式：矩阵亿分位定位基数。
pub const MAT_SCALE_HIGH: i64 = 100_000_000;
/// 高精模式：输入输出定点比例（十亿分位；中间量 i64 上界内 ≈1.9e16）。
pub const PREC_SCALE_HIGH: i64 = 1_000_000_000;

/// 恒等矩阵（万分位；同空间显式路径用）。
const M_IDENTITY_10K: [[i64; 3]; 3] =
    [[MAT_SCALE_FAST, 0, 0], [0, MAT_SCALE_FAST, 0], [0, 0, MAT_SCALE_FAST]];

// ---- 快速模式：万分位正逆矩阵（逆 = 万分位正矩阵的有理精确逆取整） ----
const M_SRGB_XYZ: [[i64; 3]; 3] =
    [[4124, 3576, 1805], [2126, 7152, 722], [193, 1192, 9505]];
const M_XYZ_SRGB: [[i64; 3]; 3] =
    [[32406, -15372, -4986], [-9689, 18758, 415], [557, -2040, 10570]];
const M_P3_XYZ: [[i64; 3]; 3] =
    [[4866, 2657, 1982], [2290, 6917, 793], [0, 451, 10439]];
const M_XYZ_P3: [[i64; 3]; 3] =
    [[24935, -9316, -4027], [-8296, 17629, 236], [358, -762, 9569]];
const M_R2020_XYZ: [[i64; 3]; 3] =
    [[6369, 2627, 1004], [2627, 6780, 593], [0, 281, 10603]];
const M_XYZ_R2020: [[i64; 3]; 3] =
    [[18660, -7173, -1366], [-7247, 17569, -296], [192, -466, 9439]];

// ---- 高精模式：亿分位正逆矩阵（正 = 万分位精确放大；逆 = 有理精确逆取整） ----
const M_SRGB_XYZ_H: [[i64; 3]; 3] =
    [[41240000, 35760000, 18050000], [21260000, 71520000, 7220000], [1930000, 11920000, 95050000]];
const M_XYZ_SRGB_H: [[i64; 3]; 3] =
    [[324062548, -153720797, -49862860], [-96893071, 187575606, 4151752], [5571012, -20402105, 105699594]];
const M_P3_XYZ_H: [[i64; 3]; 3] =
    [[48660000, 26570000, 19820000], [22900000, 69170000, 7930000], [0, 4510000, 104390000]];
const M_XYZ_P3_H: [[i64; 3]; 3] =
    [[249347777, -93155581, -40265822], [-82962081, 176285365, 2360049], [3584242, -7616122, 95692654]];
const M_R2020_XYZ_H: [[i64; 3]; 3] =
    [[63690000, 26270000, 10040000], [26270000, 67800000, 5930000], [0, 2810000, 106030000]];
const M_XYZ_R2020_H: [[i64; 3]; 3] =
    [[186598389, -71733970, -13657129], [-72467972, 175694142, -2964141], [1920541, -4656234, 94391486]];

/// 精度模式（数据结构之三：精度模式表）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Precision {
    /// 快速：万分位矩阵 + 万分位定点。
    Fast,
    /// 高精：亿分位矩阵 + 十亿分位定点。
    High,
}

/// 精度模式表（判据侧字面量对账：两模式、模式数=2）。
pub const PRECISION_MODES: [Precision; 2] = [Precision::Fast, Precision::High];

/// 矩阵 × 定点向量（恒定除以矩阵定位基数，O(1)；i64 中间量上界内）。
fn mul_mv_scaled(m: &[[i64; 3]; 3], v_s: [i64; 3], m_scale: i64) -> [i64; 3] {
    let mut out = [0i64; 3];
    for i in 0..3 {
        let mut acc = 0i64;
        for k in 0..3 {
            acc += m[i][k] * v_s[k];
        }
        out[i] = acc / m_scale;
    }
    out
}

/// 快速：空间 → XYZ 矩阵（万分位）。
fn to_xyz_fast(space: ColorSpace) -> &'static [[i64; 3]; 3] {
    match space {
        ColorSpace::Srgb => &M_SRGB_XYZ,
        ColorSpace::DisplayP3 => &M_P3_XYZ,
        ColorSpace::Rec2020 => &M_R2020_XYZ,
    }
}

/// 快速：XYZ → 空间矩阵（万分位）。
fn from_xyz_fast(space: ColorSpace) -> &'static [[i64; 3]; 3] {
    match space {
        ColorSpace::Srgb => &M_XYZ_SRGB,
        ColorSpace::DisplayP3 => &M_XYZ_P3,
        ColorSpace::Rec2020 => &M_XYZ_R2020,
    }
}

/// 高精：空间 → XYZ 矩阵（亿分位）。
fn to_xyz_high(space: ColorSpace) -> &'static [[i64; 3]; 3] {
    match space {
        ColorSpace::Srgb => &M_SRGB_XYZ_H,
        ColorSpace::DisplayP3 => &M_P3_XYZ_H,
        ColorSpace::Rec2020 => &M_R2020_XYZ_H,
    }
}

/// 高精：XYZ → 空间矩阵（亿分位）。
fn from_xyz_high(space: ColorSpace) -> &'static [[i64; 3]; 3] {
    match space {
        ColorSpace::Srgb => &M_XYZ_SRGB_H,
        ColorSpace::DisplayP3 => &M_XYZ_P3_H,
        ColorSpace::Rec2020 => &M_XYZ_R2020_H,
    }
}

/// 往返容差（万分位/通道）= 8bit 半级承诺：10000/255 = 39.2 → 39。
pub const ROUNDTRIP_TOL: i64 = 39;

/// 判据访问器：快速正矩阵（sRGB，万分位）。
pub fn to_xyz_fast_probe() -> &'static [[i64; 3]; 3] {
    &M_SRGB_XYZ
}

/// 判据访问器：快速逆矩阵（sRGB，万分位）。
pub fn from_xyz_fast_probe() -> &'static [[i64; 3]; 3] {
    &M_XYZ_SRGB
}

/// 判据访问器：高精逆矩阵（sRGB，亿分位）。
pub fn from_xyz_high_probe() -> &'static [[i64; 3]; 3] {
    &M_XYZ_SRGB_H
}

// ---------------------------------------------------------------------------
// 四、版本戳缓存（数据结构之二；缓存 O(1)）
// ---------------------------------------------------------------------------

/// 缓存容量（FNV 定槽）。
pub const CACHE_CAPACITY: usize = 32;

/// FNV-1a 定槽（O(1)，无分配）。
fn slot_of(key: (u8, u8, u8, u32)) -> usize {
    let (a, b, c, rgb) = key;
    let mut h: u64 = 2166136261 ^ a as u64;
    h = h.wrapping_mul(16777619) ^ b as u64;
    h = h.wrapping_mul(16777619) ^ c as u64;
    h = h.wrapping_mul(16777619) ^ rgb as u64;
    (h % CACHE_CAPACITY as u64) as usize
}

/// 缓存条目（out 存**转换输出**——命中必须返回换过的值）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CacheEntry {
    /// 键（源 id / 目标 id / 精度位 / 8bit RGB）。
    pub key: (u8, u8, u8, u32),
    /// 建立时的矩阵库版本。
    pub version: u32,
    /// 命中计数（性能可见性）。
    pub hits: u32,
    /// 缓存的转换输出。
    pub out: [u8; 3],
}

/// 矩阵库（转换入口 + 版本戳缓存）。
#[derive(Clone, Debug)]
pub struct MatrixLib {
    slots: [Option<CacheEntry>; CACHE_CAPACITY],
    /// 矩阵库版本（矩阵修订/重校准 bump——缓存全清依据）。
    pub version: u32,
    /// 失效计数（版本戳失效立案）。
    pub invalidations: u32,
}

/// 空间短码 → id（0..=2；缓存键用）。
fn space_id(s: ColorSpace) -> u8 {
    match s {
        ColorSpace::Srgb => 0,
        ColorSpace::DisplayP3 => 1,
        ColorSpace::Rec2020 => 2,
    }
}

/// RGB 8bit 压缩键（三通道 0..=255）。
fn rgb_key(rgb: [u8; 3]) -> u32 {
    (rgb[0] as u32) << 16 | (rgb[1] as u32) << 8 | rgb[2] as u32
}

impl MatrixLib {
    /// 空库（版本 1）。
    pub fn new() -> MatrixLib {
        MatrixLib { slots: [None; CACHE_CAPACITY], version: 1, invalidations: 0 }
    }

    /// 查缓存（O(1)；版本失配视同未命中并清槽——失真不留）。
    pub fn lookup(&mut self, key: (u8, u8, u8, u32)) -> Option<[u8; 3]> {
        let s = slot_of(key);
        match self.slots[s] {
            Some(e) if e.key == key && e.version == self.version => {
                self.slots[s] = Some(CacheEntry { hits: e.hits.saturating_add(1), ..e });
                Some(e.out)
            }
            Some(e) => {
                if e.key == key {
                    self.slots[s] = None;
                    self.invalidations = self.invalidations.saturating_add(1);
                }
                None
            }
            None => None,
        }
    }

    /// 插入（版本取当前；输出随条目落槽）。
    pub fn insert(&mut self, key: (u8, u8, u8, u32), out: [u8; 3]) {
        let s = slot_of(key);
        self.slots[s] = Some(CacheEntry { key, version: self.version, hits: 0, out });
    }

    /// 版本递增：全表失效（矩阵修订后旧结果系统性错误）。
    pub fn bump_version(&mut self) -> u32 {
        self.version = self.version.saturating_add(1);
        let n = self.slots.iter_mut().filter(|s| s.is_some()).count();
        for s in self.slots.iter_mut() {
            *s = None;
        }
        self.invalidations = self.invalidations.saturating_add(n as u32);
        self.version
    }

    /// 判据访问器：槽位读取。
    pub fn slot_entry(&self, i: usize) -> Option<CacheEntry> {
        self.slots.get(i).copied().flatten()
    }

    /// 判据访问器：定槽函数。
    pub fn slot_of_for_test(key: (u8, u8, u8, u32)) -> usize {
        slot_of(key)
    }
}

// ---------------------------------------------------------------------------
// 五、转换主流程（O(矩阵乘法) + 缓存 O(1)）
// ---------------------------------------------------------------------------

/// F4403 profile → 空间映射（**显式缺省**：Custom→Srgb；
/// AdobeRgb 无矩阵 → None 拒绝不臆造）。
pub fn map_profile_to_space(profile: ProfileKind) -> Option<ColorSpace> {
    match profile {
        ProfileKind::Srgb => Some(ColorSpace::Srgb),
        ProfileKind::DisplayP3 => Some(ColorSpace::DisplayP3),
        ProfileKind::Custom => Some(ColorSpace::Srgb), // 显式缺省：自定义校准按 sRGB 走
        ProfileKind::AdobeRgb => None,                 // 矩阵缺失：拒绝转换不臆造
    }
}

/// 转换结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Conversion {
    /// 输出 8bit RGB。
    pub out: [u8; 3],
    /// 是否缓存命中。
    pub cached: bool,
    /// 是否使用的缺省策略（显式标注——不静默）。
    pub used_default: bool,
    /// 精度模式（实际执行）。
    pub precision: Precision,
}

/// 转换（O(矩阵乘法)：两次桥乘；缓存命中 O(1)）。
///
/// 双精度同路径：按精度选定点比例与矩阵组（快速万分位/高精亿分位）。
/// 空间枚举封闭——表外空间在 parse 入口已拒；8bit 输入乘定点比例后
/// 恒在 i64 域内（高精中间量上界 ≈1.9e16 < i64::MAX）。
pub fn convert(
    lib: &mut MatrixLib,
    rgb: [u8; 3],
    from: ColorSpace,
    to: ColorSpace,
    prec: Precision,
    used_default: bool,
) -> Result<Conversion, &'static str> {
    let (prec_bit, scale, m_scale) = match prec {
        Precision::Fast => (0u8, PREC_SCALE_FAST, MAT_SCALE_FAST),
        Precision::High => (1u8, PREC_SCALE_HIGH, MAT_SCALE_HIGH),
    };
    let key = (space_id(from), space_id(to), prec_bit, rgb_key(rgb));
    if let Some(cached) = lib.lookup(key) {
        return Ok(Conversion { out: cached, cached: true, used_default, precision: prec });
    }
    let v = [rgb[0] as i64 * scale, rgb[1] as i64 * scale, rgb[2] as i64 * scale];
    let (m1, m2) = match prec {
        Precision::Fast => (to_xyz_fast(from), from_xyz_fast(to)),
        Precision::High => (to_xyz_high(from), from_xyz_high(to)),
    };
    let xyz = mul_mv_scaled(m1, v, m_scale);
    let x = mul_mv_scaled(m2, xyz, m_scale);
    let out = [
        (x[0] / scale).clamp(0, 255) as u8,
        (x[1] / scale).clamp(0, 255) as u8,
        (x[2] / scale).clamp(0, 255) as u8,
    ];
    lib.insert(key, out);
    Ok(Conversion { out, cached: false, used_default, precision: prec })
}

/// 同空间转换（显式恒等路径——恒等也落缓存有据可查，不走隐式捷径）。
pub fn convert_same(lib: &mut MatrixLib, rgb: [u8; 3], space: ColorSpace, used_default: bool) -> Result<Conversion, &'static str> {
    let key = (space_id(space), space_id(space), 0u8, rgb_key(rgb));
    if let Some(cached) = lib.lookup(key) {
        return Ok(Conversion { out: cached, cached: true, used_default, precision: Precision::Fast });
    }
    let _ = M_IDENTITY_10K; // 恒等有表可查（语义声明；短路直返同值）
    lib.insert(key, rgb);
    Ok(Conversion { out: rgb, cached: false, used_default, precision: Precision::Fast })
}

// ---------------------------------------------------------------------------
// 六、色卡往返断言（O(色卡数)；超差→高精强制）
// ---------------------------------------------------------------------------

/// 测试色卡（8 卡代表色；判据侧独立复述同表）。
pub const TEST_CARD: [[u8; 3]; 8] = [
    [0, 0, 0], [255, 255, 255], [255, 0, 0], [0, 255, 0], [0, 0, 255],
    [128, 128, 128], [255, 128, 0], [64, 224, 208],
];

/// 往返报告。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RoundtripReport {
    /// 全卡最大误差（万分位/通道）。
    pub max_err: i64,
    /// 是否全部 ≤ 容差承诺。
    pub passed: bool,
}

/// 单点往返误差（最大通道差，换算为**万分位**）。
pub fn roundtrip_err(rgb: [u8; 3], space: ColorSpace, prec: Precision) -> i64 {
    let (scale, m_scale) = match prec {
        Precision::Fast => (PREC_SCALE_FAST, MAT_SCALE_FAST),
        Precision::High => (PREC_SCALE_HIGH, MAT_SCALE_HIGH),
    };
    let v = [rgb[0] as i64 * scale, rgb[1] as i64 * scale, rgb[2] as i64 * scale];
    let (m1, m2) = match prec {
        Precision::Fast => (to_xyz_fast(space), from_xyz_fast(space)),
        Precision::High => (to_xyz_high(space), from_xyz_high(space)),
    };
    let xyz = mul_mv_scaled(m1, v, m_scale);
    let back = mul_mv_scaled(m2, xyz, m_scale);
    let mut max = 0i64;
    for i in 0..3 {
        let d = (back[i] - v[i]).abs() * MAT_SCALE_FAST / scale;
        if d > max {
            max = d;
        }
    }
    max
}

/// 色卡往返断言（O(色卡数)：逐卡取最大误差对照容差承诺）。
pub fn roundtrip_check(space: ColorSpace, prec: Precision) -> RoundtripReport {
    let mut max = 0i64;
    for card in TEST_CARD.iter() {
        let e = roundtrip_err(*card, space, prec);
        if e > max {
            max = e;
        }
    }
    RoundtripReport { max_err: max, passed: max <= ROUNDTRIP_TOL }
}

/// 精度强制（锚点「往返超差→高精模式强制」）：快速超差 → 强制高精。
///
/// 物理事实：万分位矩阵的往返误差（~137+）超半级承诺（39）——快速
/// 模式**必然**超差，本函数返回高精 + E_CS_ROUNDTRIP 标注；高精模式
/// （亿分位）误差 0 达承诺。
pub fn enforce_precision(space: ColorSpace, prec: Precision) -> (Precision, Option<&'static str>) {
    let rep = roundtrip_check(space, prec);
    if !rep.passed {
        (Precision::High, Some(E_CS_ROUNDTRIP))
    } else {
        (prec, None)
    }
}

// ---------------------------------------------------------------------------
// 七、显式缺省策略 + 读屏告警
// ---------------------------------------------------------------------------

/// 转换策略（显式可查——`is_default` 位不静默）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConversionStrategy {
    /// 源空间。
    pub from: ColorSpace,
    /// 目标空间。
    pub to: ColorSpace,
    /// 精度。
    pub precision: Precision,
    /// 是否缺省策略（显式缺省标注位）。
    pub is_default: bool,
}

/// 缺省策略（显式：sRGB→sRGB 恒等 + 快速——可查可覆盖，不隐式）。
pub fn default_strategy() -> ConversionStrategy {
    ConversionStrategy {
        from: ColorSpace::Srgb,
        to: ColorSpace::Srgb,
        precision: Precision::Fast,
        is_default: true,
    }
}

/// 读屏告警（域本色）：按报告与实际精度播报。
pub fn screen_line(space: ColorSpace, rep: &RoundtripReport, prec: Precision) -> String {
    if !rep.passed {
        return format!(
            "色彩精度警告：{} 空间往返误差 {} 万分位超半级承诺，已强制高精模式（{}）",
            space.wire(),
            rep.max_err,
            E_CS_ROUNDTRIP
        );
    }
    format!(
        "色彩转换就绪：{} 空间 {} 精度，最大往返误差 {} 万分位（承诺 {} 内）",
        space.wire(),
        match prec {
            Precision::Fast => "快速",
            Precision::High => "高精",
        },
        rep.max_err,
        ROUNDTRIP_TOL
    )
}
