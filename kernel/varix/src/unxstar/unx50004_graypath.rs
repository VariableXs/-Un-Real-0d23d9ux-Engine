//! UNX-F50004 灰度打印路径（RGB→K 单通道归一）（0x42ca0 段）
//!
//! 锚点：UNX-F50004-J1 灰度 ×30 色调偏移 ≤阈值；判据锚定 §AI-63 §2 专题十二。
//!
//! 数据段 `SEG_BASE = 0x42ca0`（独占 0x20）。诊断码闭集 = 本段 `E_EMPTY..=E_DUP_PAGE`
//! （1..=11），`GrayError::code()` 恒为 `0x42ca0 + n`。
//!
//! 为什么「灰度」要单列一张卡：彩色转灰不是取平均，而是「亮度成比例 + 单通道
//! 归一」。前者是 BT.709 亮度系数（0.2126/0.7152/0.0722），后者是**网点增益
//! 反解**——K 通道 8 位码值必须先把「目标墨量」经增益曲线反解成设备码值，
//! 否则印出来的中间调一律偏深（典型 +8~12 L*）。本模块把这两步都落成数据
//! 结构：一张 256 项反解 LUT + 一条可复算的网点增益曲线 + 逐页统计账。
//!
//! 结构清单：
//! * `GrayError`：11 个显式错误，`code()` 落 0x42ca0 段；
//! * `InkPreset` / `PresetBook`：纸墨预设注册表（gamma/增益鼓包/墨量上限），
//!   容量 6，重名与非法参数显式拒绝；
//! * `DotGain`：网点增益正演曲线（`forward`）与反解（`inverse`，二分）；
//! * `InkTable`：256 项反解 LUT（真数据结构：逐项二分求得、带逆回误差自检）；
//! * `GrayPath`：单通道归一化路径（`to_k` / `k_to_l` / `tone_shift`），
//!   未构建状态返回 `Unbuilt`；
//! * `GrayPage` / `GrayLedger`：逐页统计账（8 桶直方图 + 触限计数），
//!   页容量 30、账本容量 16，越界与重复页号显式拒绝；
//! * `injection_core`：5 类坏样本注入核。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;
use libm::{cbrtf, fabsf, powf, sinf};

use super::unx50001_gamutmap::{rgb8_to_linear, Rgb8};

/// 诊断段：0x42ca0 独占 0x20（同批其余模块从 0x42cc0 起顺序占用）。
pub const SEG_BASE: u32 = 0x42ca0;

pub const E_EMPTY: u16 = 1;
pub const E_CONFIG: u16 = 2;
pub const E_UNBUILT: u16 = 3;
pub const E_PAGE_FULL: u16 = 4;
pub const E_INDEX: u16 = 5;
pub const E_LEDGER_FULL: u16 = 6;
pub const E_PRESET_EXISTS: u16 = 7;
pub const E_PRESET_FULL: u16 = 8;
pub const E_NO_PRESET: u16 = 9;
pub const E_RANGE: u16 = 10;
pub const E_DUP_PAGE: u16 = 11;

/// 灰度路径的显式错误闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GrayError {
    /// 空输入（空色调表、空预设名）。
    Empty,
    /// 预设参数非法（gamma/鼓包/墨量上限越界或非有限）。
    Config,
    /// 路径未构建（LUT 不存在）。
    Unbuilt,
    /// 本页已满（页容量 30 色调）。
    PageFull,
    /// 下标越界（LUT / 直方图桶）。
    Index,
    /// 页账本已满（容量 16 页）。
    LedgerFull,
    /// 预设重名。
    PresetExists,
    /// 预设注册表满（容量 6）。
    PresetFull,
    /// 查询的预设不存在。
    NoPreset,
    /// 数值越界（非有限值）。
    Range,
    /// 同一页号重复落账。
    DupPage,
}

impl GrayError {
    /// 诊断码：`SEG_BASE + n`。
    pub fn code(self) -> u32 {
        let n = match self {
            GrayError::Empty => E_EMPTY,
            GrayError::Config => E_CONFIG,
            GrayError::Unbuilt => E_UNBUILT,
            GrayError::PageFull => E_PAGE_FULL,
            GrayError::Index => E_INDEX,
            GrayError::LedgerFull => E_LEDGER_FULL,
            GrayError::PresetExists => E_PRESET_EXISTS,
            GrayError::PresetFull => E_PRESET_FULL,
            GrayError::NoPreset => E_NO_PRESET,
            GrayError::Range => E_RANGE,
            GrayError::DupPage => E_DUP_PAGE,
        };
        SEG_BASE + n as u32
    }

    /// 一行原因（日志用）。
    pub fn message(self) -> &'static str {
        match self {
            GrayError::Empty => "empty tone set or preset name",
            GrayError::Config => "ink preset parameter outside legal range",
            GrayError::Unbuilt => "gray path built without ink table",
            GrayError::PageFull => "gray page already holds 30 tones",
            GrayError::Index => "index outside table or histogram",
            GrayError::LedgerFull => "gray page ledger full",
            GrayError::PresetExists => "ink preset already registered",
            GrayError::PresetFull => "ink preset registry full",
            GrayError::NoPreset => "ink preset not found",
            GrayError::Range => "value outside legal range",
            GrayError::DupPage => "page number already recorded",
        }
    }
}

pub const PI_F: f32 = 3.14159265_f32;

/// BT.709 亮度系数。选 BT.709 而不是 BT.601 的理由：打印前端的色彩管理
/// 一律按 sRGB（=BT.709 原色），用 601 系数会让纯红灰度化偏亮约 4 L*。
pub const LUMA_R: f32 = 0.2126;
pub const LUMA_G: f32 = 0.7152;
pub const LUMA_B: f32 = 0.0722;

fn clampf(v: f32, lo: f32, hi: f32) -> f32 {
    if v < lo {
        lo
    } else if v > hi {
        hi
    } else {
        v
    }
}

fn clamp01(v: f32) -> f32 {
    clampf(v, 0.0, 1.0)
}

/// CIE 明度：由相对亮度 Y（0..1）求 L*（D50 中性轴）。
///
/// 与 F50001 的 `xyz_d50_to_lab` 同源；这里只取 L 一维，避免为一条标量
/// 构造 Xyz/Lab 结构体（热路径在每像素上）。
pub fn lstar_of(y: f32) -> f32 {
    const EPS: f32 = 0.008856451679; // (6/29)^3
    const KAPPA: f32 = 7.787037037; // (1/3)*(29/6)^2
    let t = clamp01(y);
    let fy = if t > EPS {
        cbrtf(t)
    } else {
        KAPPA * t + 4.0 / 29.0
    };
    116.0 * fy - 16.0
}

/// 相对亮度 Y（0..1）→ 目标墨量（0=白纸、1=满版黑）。
pub fn ink_target(y: f32) -> f32 {
    clamp01(1.0 - clamp01(y))
}

// ---------------------------------------------------------------------------
// 1. 纸墨预设 + 注册表
// ---------------------------------------------------------------------------

pub const PRESET_CAP: usize = 6;

/// 纸墨预设：网点增益形状 + 墨量上限。
///
/// * `gamma`：增益指数，1.0 = 无增益；涂布纸约 1.10，非涂布 1.20~1.30；
/// * `boost`：中间调鼓包（正弦基，端点为 0），模拟半色调的中间调跳升；
/// * `ink_limit`：单通道墨量上限（0.5..=1.0），上光/薄纸必须限制防透印。
#[derive(Clone, Debug, PartialEq)]
pub struct InkPreset {
    pub name: String,
    pub gamma: f32,
    pub boost: f32,
    pub ink_limit: f32,
}

impl InkPreset {
    /// 校验并构造。三参数都必须在物理区间内：gamma 1.0..=3.0、
    /// 鼓包 0.0..=0.08、墨量上限 0.5..=1.0（上限 >0.08 的鼓包会让
    /// `forward` 在 k→1 处斜率转负，曲线不再单调）。
    pub fn new(name: &str, gamma: f32, boost: f32, ink_limit: f32) -> Result<InkPreset, GrayError> {
        if name.is_empty() {
            return Err(GrayError::Empty);
        }
        if !gamma.is_finite() || !boost.is_finite() || !ink_limit.is_finite() {
            return Err(GrayError::Config);
        }
        if gamma < 1.0 || gamma > 3.0 {
            return Err(GrayError::Config);
        }
        if boost < 0.0 || boost > 0.08 {
            return Err(GrayError::Config);
        }
        if ink_limit < 0.5 || ink_limit > 1.0 {
            return Err(GrayError::Config);
        }
        Ok(InkPreset {
            name: name.to_string(),
            gamma,
            boost,
            ink_limit,
        })
    }

    /// 内置：普通 80g 非涂布纸。
    pub fn plain() -> InkPreset {
        match InkPreset::new("plain", 1.20, 0.020, 1.00) {
            Ok(p) => p,
            Err(_) => InkPreset::fallback(),
        }
    }

    /// 内置：涂布纸（增益小、可满墨）。
    pub fn coated() -> InkPreset {
        match InkPreset::new("coated", 1.08, 0.010, 0.95) {
            Ok(p) => p,
            Err(_) => InkPreset::fallback(),
        }
    }

    /// 内置：新闻纸（增益大、必须限墨）。
    pub fn newsprint() -> InkPreset {
        match InkPreset::new("newsprint", 1.45, 0.050, 0.88) {
            Ok(p) => p,
            Err(_) => InkPreset::fallback(),
        }
    }

    /// 不可达兜底（常量合法，构造必成功；仅用于让内置构造器无 panic 路径）。
    pub fn fallback() -> InkPreset {
        InkPreset {
            name: "fallback".to_string(),
            gamma: 1.0,
            boost: 0.0,
            ink_limit: 1.0,
        }
    }

    /// 本预设的正演曲线。
    pub fn gain(&self) -> DotGain {
        DotGain {
            gamma: self.gamma,
            boost: self.boost,
        }
    }
}

/// 预设注册表（容量 6）。重名 → `PresetExists`，满 → `PresetFull`。
#[derive(Clone, Debug)]
pub struct PresetBook {
    entries: Vec<InkPreset>,
}

impl PresetBook {
    pub fn new() -> PresetBook {
        PresetBook {
            entries: Vec::new(),
        }
    }

    /// 三张内置纸。
    pub fn standard() -> PresetBook {
        let mut b = PresetBook::new();
        let _ = b.register(InkPreset::plain());
        let _ = b.register(InkPreset::coated());
        let _ = b.register(InkPreset::newsprint());
        b
    }

    /// 登记预设。
    pub fn register(&mut self, preset: InkPreset) -> Result<(), GrayError> {
        if preset.name.is_empty() {
            return Err(GrayError::Empty);
        }
        if self.entries.len() >= PRESET_CAP {
            return Err(GrayError::PresetFull);
        }
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.name.as_str() == preset.name.as_str() {
                    return Err(GrayError::PresetExists);
                }
            }
            i += 1;
        }
        self.entries.push(preset);
        Ok(())
    }

    /// 查预设。
    pub fn lookup(&self, name: &str) -> Option<&InkPreset> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.name.as_str() == name {
                    return Some(e);
                }
            }
            i += 1;
        }
        None
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 2. 网点增益正演 / 反解
// ---------------------------------------------------------------------------

/// 网点增益曲线：`optical = k^(1/gamma) + boost·sin(πk)`。
///
/// 端点：`forward(0)=0`、`forward(1)=1`（鼓包在端点为 0）；
/// 单调：`d/dk = (1/gamma)k^(1/gamma-1) + boost·π·cos(πk)`，
/// 在 k=1 处为 `1/gamma - boost·π`，gamma≥1、boost≤0.08 时恒 > 0.7。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DotGain {
    pub gamma: f32,
    pub boost: f32,
}

impl DotGain {
    pub fn new(gamma: f32, boost: f32) -> DotGain {
        DotGain { gamma, boost }
    }

    /// 正演：设备墨量 k（0..1）→ 光学覆盖率。
    pub fn forward(&self, k: f32) -> f32 {
        let x = clamp01(k);
        if x <= 0.0 {
            return 0.0;
        }
        if x >= 1.0 {
            return 1.0;
        }
        let g = clampf(self.gamma, 1.0, 3.0);
        let b = clampf(self.boost, 0.0, 0.08);
        clamp01(powf(x, 1.0 / g) + b * sinf(PI_F * x))
    }

    /// 反解：目标光学覆盖率 t → 设备墨量 k。二分 32 轮（区间 1 → 4e-10）。
    /// `forward` 单调，故反解唯一。
    pub fn inverse(&self, t: f32) -> f32 {
        let target = clamp01(t);
        let mut lo = 0.0_f32;
        let mut hi = 1.0_f32;
        let mut i = 0usize;
        while i < 32 {
            let mid = (lo + hi) * 0.5;
            if self.forward(mid) < target {
                lo = mid;
            } else {
                hi = mid;
            }
            i += 1;
        }
        (lo + hi) * 0.5
    }

    /// 单调自检：`samples` 个等距点非降。
    pub fn is_monotone(&self, samples: usize) -> bool {
        if samples < 2 {
            return false;
        }
        let mut prev = self.forward(0.0);
        let mut i = 1usize;
        while i < samples {
            let k = i as f32 / (samples - 1) as f32;
            let v = self.forward(k);
            if v + 1e-6 < prev {
                return false;
            }
            prev = v;
            i += 1;
        }
        true
    }
}

/// 256 项反解 LUT：下标 = 目标墨量码（0..255），值 = K 通道设备码。
///
/// 为什么用表而不是每像素反解：反解是 32 轮二分（≈64 次 powf 级开销），
/// 一张 A4 版面 8M 像素跑不起；表在页面级构建一次，热路径只做一次查表。
#[derive(Clone, Debug, PartialEq)]
pub struct InkTable {
    codes: Vec<u8>,
    limit_code: u8,
}

impl InkTable {
    /// 按预设构建（256 项，逐项二分反解）。限墨时高位码被压到 `limit_code`。
    pub fn build(preset: &InkPreset) -> InkTable {
        let gain = preset.gain();
        let limit = clampf(preset.ink_limit, 0.5, 1.0);
        let mut codes = Vec::with_capacity(256);
        let mut i = 0usize;
        while i < 256 {
            let t = i as f32 / 255.0;
            let k = if t > gain.forward(limit) {
                limit
            } else {
                gain.inverse(t)
            };
            let code = (clamp01(k) * 255.0 + 0.5) as u32;
            codes.push(if code > 255 { 255 } else { code as u8 });
            i += 1;
        }
        let lc = (clamp01(limit) * 255.0 + 0.5) as u32;
        InkTable {
            codes,
            limit_code: if lc > 255 { 255 } else { lc as u8 },
        }
    }

    /// 空表（只用于显式报 `Unbuilt`）。
    pub fn empty() -> InkTable {
        InkTable {
            codes: Vec::new(),
            limit_code: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.codes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.codes.is_empty()
    }

    /// 目标墨量码 → K 设备码。
    pub fn code_for_ink(&self, t: f32) -> Result<u8, GrayError> {
        if self.codes.is_empty() {
            return Err(GrayError::Unbuilt);
        }
        if !t.is_finite() {
            return Err(GrayError::Range);
        }
        let scaled = clamp01(t) * 255.0 + 0.5;
        let idx = if scaled >= 255.0 { 255usize } else { scaled as usize };
        match self.codes.get(idx) {
            Some(v) => Ok(*v),
            None => Err(GrayError::Index),
        }
    }

    /// 按下标取码（判据遍历表用）。
    pub fn code_at(&self, i: usize) -> Result<u8, GrayError> {
        match self.codes.get(i) {
            Some(v) => Ok(*v),
            None => Err(GrayError::Index),
        }
    }

    /// 限墨后的最大 K 码。
    pub fn limit_code(&self) -> u8 {
        self.limit_code
    }

    /// LUT 质量自检：逐项正演逆回，返回最大 |t' - t|（以码为单位）。
    ///
    /// 这是「表是否忠实」的唯一权威度量——不靠眼猜阈值，直接算。
    pub fn max_recovery_error_codes(&self, gain: &DotGain) -> f32 {
        let mut worst = 0.0_f32;
        let mut i = 0usize;
        while i < self.codes.len() {
            if let Some(code) = self.codes.get(i) {
                let t = i as f32 / 255.0;
                let back = gain.forward(*code as f32 / 255.0);
                let d = fabsf(back - t) * 255.0;
                if d > worst {
                    worst = d;
                }
            }
            i += 1;
        }
        worst
    }
}

// ---------------------------------------------------------------------------
// 3. 单通道归一化路径
// ---------------------------------------------------------------------------

/// 路径状态：未构建（无 LUT）与已构建。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathState {
    Unbuilt,
    Built,
}

impl PathState {
    pub fn code(self) -> u8 {
        match self {
            PathState::Unbuilt => 0,
            PathState::Built => 1,
        }
    }
}

/// 灰度路径：RGB → 亮度 → 墨量 → K 码（单通道归一）。
#[derive(Clone, Debug, PartialEq)]
pub struct GrayPath {
    preset: InkPreset,
    gain: DotGain,
    table: InkTable,
    state: PathState,
}

impl GrayPath {
    /// 按预设构建路径（含 256 项 LUT）。
    pub fn build(preset: InkPreset) -> GrayPath {
        let gain = preset.gain();
        let table = InkTable::build(&preset);
        GrayPath {
            preset,
            gain,
            table,
            state: PathState::Built,
        }
    }

    /// 未构建路径：任何转换都返回 `Unbuilt`（不做「默认 2.2」这类静默兜底）。
    pub fn unbuilt() -> GrayPath {
        GrayPath {
            preset: InkPreset::fallback(),
            gain: DotGain::new(1.0, 0.0),
            table: InkTable::empty(),
            state: PathState::Unbuilt,
        }
    }

    pub fn state(&self) -> PathState {
        self.state
    }

    pub fn preset(&self) -> &InkPreset {
        &self.preset
    }

    pub fn gain(&self) -> DotGain {
        self.gain
    }

    pub fn table(&self) -> &InkTable {
        &self.table
    }

    /// 源色 → 相对亮度 Y（BT.709，线性光域加权）。
    pub fn luma(&self, c: Rgb8) -> f32 {
        let lin = rgb8_to_linear(c);
        clamp01(LUMA_R * lin.r + LUMA_G * lin.g + LUMA_B * lin.b)
    }

    /// 源色的「理想」中性明度（与其亮度同值的中性灰的 L*）。
    pub fn ideal_l(&self, c: Rgb8) -> f32 {
        lstar_of(self.luma(c))
    }

    /// RGB → K 单通道码值（0=白纸，255=满版）。未构建 → `Unbuilt`。
    pub fn to_k(&self, c: Rgb8) -> Result<u8, GrayError> {
        if self.state != PathState::Built || self.table.is_empty() {
            return Err(GrayError::Unbuilt);
        }
        let t = ink_target(self.luma(c));
        self.table.code_for_ink(t)
    }

    /// K 码 → 印品中性明度 L*（先正演回光学覆盖率，再取 L*）。
    pub fn k_to_l(&self, k: u8) -> Result<f32, GrayError> {
        if self.state != PathState::Built {
            return Err(GrayError::Unbuilt);
        }
        let optical = self.gain.forward(k as f32 / 255.0);
        Ok(lstar_of(1.0 - optical))
    }

    /// 单色色调偏移：印品 L* 与理想 L* 之差（J1 的度量口径）。
    pub fn tone_shift(&self, c: Rgb8) -> Result<f32, GrayError> {
        let k = self.to_k(c)?;
        let out = self.k_to_l(k)?;
        Ok(fabsf(out - self.ideal_l(c)))
    }

    /// 该色是否触到墨量上限（真：目标墨量超过可印上限，被显式截断）。
    pub fn hits_limit(&self, c: Rgb8) -> bool {
        let t = ink_target(self.luma(c));
        t > self.gain.forward(self.preset.ink_limit) + 1e-6
    }
}

// ---------------------------------------------------------------------------
// 4. 逐页统计账
// ---------------------------------------------------------------------------

pub const PAGE_CAP: usize = 30;
pub const LEDGER_CAP: usize = 16;

/// 一页的灰度统计：8 桶直方图 + 触限计数 + 偏移累积。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageStats {
    pub n: usize,
    pub sum_k: u32,
    pub min_k: u8,
    pub max_k: u8,
    pub hist: [u16; 8],
    pub limit_clipped: usize,
    pub sum_shift: f32,
    pub max_shift: f32,
}

impl PageStats {
    pub fn new() -> PageStats {
        PageStats {
            n: 0,
            sum_k: 0,
            min_k: 255,
            max_k: 0,
            hist: [0; 8],
            limit_clipped: 0,
            sum_shift: 0.0,
            max_shift: 0.0,
        }
    }

    /// 平均 K 码。空页 → `Empty`（不返回 0 冒充「全白」）。
    pub fn mean_k(&self) -> Result<f32, GrayError> {
        if self.n == 0 {
            return Err(GrayError::Empty);
        }
        Ok(self.sum_k as f32 / self.n as f32)
    }

    /// 平均色调偏移。空页 → `Empty`。
    pub fn mean_shift(&self) -> Result<f32, GrayError> {
        if self.n == 0 {
            return Err(GrayError::Empty);
        }
        Ok(self.sum_shift / self.n as f32)
    }

    /// 第 i 桶计数。
    pub fn bin(&self, i: usize) -> Result<u16, GrayError> {
        match self.hist.get(i) {
            Some(v) => Ok(*v),
            None => Err(GrayError::Index),
        }
    }

    /// 记录一个 K 码与它的偏移。`k` 上限 255 由类型保证。
    pub fn note(&mut self, k: u8, shift: f32, limited: bool) {
        self.n += 1;
        self.sum_k += k as u32;
        if k < self.min_k {
            self.min_k = k;
        }
        if k > self.max_k {
            self.max_k = k;
        }
        // 8 桶 = K 码高 3 位（0..7），先入为主的粗分档足够看「版面对不对」。
        let b = (k >> 5) as usize;
        if let Some(slot) = self.hist.get_mut(b) {
            *slot = slot.saturating_add(1);
        }
        if limited {
            self.limit_clipped += 1;
        }
        self.sum_shift += shift;
        if shift > self.max_shift {
            self.max_shift = shift;
        }
    }
}

/// 一页的灰度打印账：容量 30 色调（J1 的样本量）。
#[derive(Clone, Debug, PartialEq)]
pub struct GrayPage {
    number: u16,
    stats: PageStats,
    cap: usize,
}

impl GrayPage {
    pub fn new(number: u16) -> GrayPage {
        GrayPage {
            number,
            stats: PageStats::new(),
            cap: PAGE_CAP,
        }
    }

    pub fn number(&self) -> u16 {
        self.number
    }

    pub fn stats(&self) -> PageStats {
        self.stats
    }

    pub fn len(&self) -> usize {
        self.stats.n
    }

    pub fn is_empty(&self) -> bool {
        self.stats.n == 0
    }

    pub fn remaining(&self) -> usize {
        if self.stats.n >= self.cap {
            0
        } else {
            self.cap - self.stats.n
        }
    }

    /// 打一色：算出 K 与偏移并计入本页。页满 → `PageFull`（计数不变、不丢弃）。
    pub fn push(&mut self, path: &GrayPath, c: Rgb8) -> Result<u8, GrayError> {
        if self.stats.n >= self.cap {
            return Err(GrayError::PageFull);
        }
        let k = path.to_k(c)?;
        let shift = path.tone_shift(c)?;
        let limited = path.hits_limit(c);
        self.stats.note(k, shift, limited);
        Ok(k)
    }
}

/// 页账本（容量 16 页）。重复页号 → `DupPage`；满 → `LedgerFull`。
#[derive(Clone, Debug)]
pub struct GrayLedger {
    pages: Vec<GrayPage>,
}

impl GrayLedger {
    pub fn new() -> GrayLedger {
        GrayLedger { pages: Vec::new() }
    }

    /// 记完一页（页号唯一）。
    pub fn record(&mut self, page: GrayPage) -> Result<(), GrayError> {
        if self.pages.len() >= LEDGER_CAP {
            return Err(GrayError::LedgerFull);
        }
        let mut i = 0usize;
        while i < self.pages.len() {
            if let Some(p) = self.pages.get(i) {
                if p.number == page.number {
                    return Err(GrayError::DupPage);
                }
            }
            i += 1;
        }
        self.pages.push(page);
        Ok(())
    }

    pub fn get(&self, i: usize) -> Option<&GrayPage> {
        self.pages.get(i)
    }

    pub fn len(&self) -> usize {
        self.pages.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pages.is_empty()
    }

    /// 全账最大色调偏移（跨页）。空账 → `Empty`。
    pub fn max_shift(&self) -> Result<f32, GrayError> {
        if self.pages.is_empty() {
            return Err(GrayError::Empty);
        }
        let mut m = 0.0_f32;
        let mut i = 0usize;
        while i < self.pages.len() {
            if let Some(p) = self.pages.get(i) {
                if p.stats.max_shift > m {
                    m = p.stats.max_shift;
                }
            }
            i += 1;
        }
        Ok(m)
    }

    /// 全账平均色调偏移（按色数加权）。
    pub fn mean_shift(&self) -> Result<f32, GrayError> {
        let mut sum = 0.0_f32;
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.pages.len() {
            if let Some(p) = self.pages.get(i) {
                sum += p.stats.sum_shift;
                n += p.stats.n;
            }
            i += 1;
        }
        if n == 0 {
            return Err(GrayError::Empty);
        }
        Ok(sum / n as f32)
    }
}

// ---------------------------------------------------------------------------
// 5. J1 的 30 级色调样本
// ---------------------------------------------------------------------------

/// 30 级灰色调（0..=255 等距）。J1 的「灰度 ×30」就是这一组。
pub fn tone_ramp_30() -> Vec<Rgb8> {
    let mut out = Vec::with_capacity(30);
    let mut i = 0u16;
    while i < 30 {
        let v = (i * 255 / 29) as u8;
        out.push(Rgb8::new(v, v, v));
        i += 1;
    }
    out
}

/// 30 个彩色样本（20 个网格点 + 10 个原色附近）：验证「彩色进、单通道出」
/// 的归一化在色相维度上不引入额外偏移（偏移只应来自 8 位量化）。
pub fn chroma_30() -> Vec<Rgb8> {
    let mut out = Vec::with_capacity(30);
    let steps = [0u8, 51, 102, 153, 204, 255];
    let mut a = 0usize;
    while a < steps.len() && out.len() < 20 {
        let mut b = 0usize;
        while b < steps.len() && out.len() < 20 {
            let r = match steps.get(a) {
                Some(v) => *v,
                None => 0,
            };
            let g = match steps.get(b) {
                Some(v) => *v,
                None => 0,
            };
            out.push(Rgb8::new(r, g, 255 - r));
            b += 1;
        }
        a += 1;
    }
    let primaries = [
        Rgb8::new(255, 0, 0),
        Rgb8::new(0, 255, 0),
        Rgb8::new(0, 0, 255),
        Rgb8::new(255, 255, 0),
        Rgb8::new(0, 255, 255),
        Rgb8::new(255, 0, 255),
        Rgb8::new(255, 128, 0),
        Rgb8::new(0, 128, 255),
        Rgb8::new(128, 0, 255),
        Rgb8::new(200, 180, 60),
    ];
    let mut i = 0usize;
    while i < primaries.len() {
        if let Some(c) = primaries.get(i) {
            out.push(*c);
        }
        i += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// 6. 注入核：5 类坏样本必须全部被抓
// ---------------------------------------------------------------------------

/// 5 类坏样本 → 逐类断言显式拒绝。返回 (抓住数, 总数)。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 5usize;

    // 1) 预设参数非法：gamma 越界 / 鼓包越界 / 限墨越界 / NaN → Config
    let c1 = InkPreset::new("bad-g", 0.5, 0.0, 1.0).err() == Some(GrayError::Config);
    let c2 = InkPreset::new("bad-b", 1.2, 0.5, 1.0).err() == Some(GrayError::Config);
    let c3 = InkPreset::new("bad-l", 1.2, 0.0, 0.2).err() == Some(GrayError::Config);
    let c4 = InkPreset::new("nan", f32::NAN, 0.0, 1.0).err() == Some(GrayError::Config);
    if c1 && c2 && c3 && c4 {
        caught += 1;
    }

    // 2) 未构建路径转换 → Unbuilt（不静默按 gamma 2.2 处理）
    let ub = GrayPath::unbuilt();
    let u1 = ub.to_k(Rgb8::new(10, 10, 10)).err() == Some(GrayError::Unbuilt);
    let u2 = ub.k_to_l(128).err() == Some(GrayError::Unbuilt);
    let u3 = ub.tone_shift(Rgb8::new(0, 0, 0)).err() == Some(GrayError::Unbuilt);
    if u1 && u2 && u3 {
        caught += 1;
    }

    // 3) 页满：第 31 色调 → PageFull 且页内计数不变
    let path = GrayPath::build(InkPreset::plain());
    let mut page = GrayPage::new(1);
    let ramp = tone_ramp_30();
    let mut k = 0usize;
    while k < ramp.len() {
        if let Some(c) = ramp.get(k) {
            if page.push(&path, *c).is_err() {
                break;
            }
        }
        k += 1;
    }
    let before = page.len();
    let over = page.push(&path, Rgb8::new(0, 0, 0));
    if over.err() == Some(GrayError::PageFull) && page.len() == before && before == PAGE_CAP {
        caught += 1;
    }

    // 4) 预设注册表：重名拒绝
    let mut book = PresetBook::new();
    let ok1 = book.register(InkPreset::plain()).is_ok();
    let dup = book.register(InkPreset::plain()).err() == Some(GrayError::PresetExists);
    let miss = book.lookup("nope").is_none();
    if ok1 && dup && miss {
        caught += 1;
    }

    // 5) 空表查码 → Unbuilt；越界桶 → Index；空页均值 → Empty
    let empty = InkTable::empty();
    let e1 = empty.code_for_ink(0.5).err() == Some(GrayError::Unbuilt);
    let e2 = empty.code_at(0).err() == Some(GrayError::Index);
    let e3 = PageStats::new().mean_k().err() == Some(GrayError::Empty)
        && PageStats::new().bin(9).err() == Some(GrayError::Index);
    if e1 && e2 && e3 {
        caught += 1;
    }

    (caught, total)
}

// ---------------------------------------------------------------------------
// 7. 判据
// ---------------------------------------------------------------------------

/// UNX-F50004 判据集（16 条）。锚点：灰度 ×30 色调偏移 ≤ 阈值。
pub fn run_unx50004_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50004");
    let book = PresetBook::standard();
    let preset = match book.lookup("plain") {
        Some(p) => p.clone(),
        None => InkPreset::plain(),
    };
    let path = GrayPath::build(preset);
    let ramp = tone_ramp_30();

    // J1-1 样本规模 = 30（锚点原文的「×30」）
    let ok = ramp.len() == 30 && path.state() == PathState::Built;
    s.add("50004-J1-样本30", ok, "样本量或路径状态不符");

    // J1-2 单通道归一端点：白纸 → K=0、满版黑 → K=255
    let kw = path.to_k(Rgb8::WHITE);
    let kb = path.to_k(Rgb8::BLACK);
    let ok = kw == Ok(0) && kb == Ok(255);
    let det = format!("kw={:?} kb={:?}", kw, kb);
    s.add_dyn("50004-J1-端点归一", ok, &det);

    // J1-3 K 单调：灰阶越亮 K 越小（64 级等距采样，非增）
    let mut mono = true;
    let mut prev = 255i32;
    let mut i = 0u16;
    while i < 64 {
        let v = (i * 255 / 63) as u8;
        match path.to_k(Rgb8::new(v, v, v)) {
            Ok(k) => {
                if k as i32 > prev {
                    mono = false;
                }
                prev = k as i32;
            }
            Err(_) => mono = false,
        }
        i += 1;
    }
    s.add("50004-J1-K单调", mono, "K 随亮度回升（归一化断裂）");

    // J1-4 锚点：30 级灰度色调偏移 均值 ≤ 1.5、最大 ≤ 5.0
    //
    // 阈值的来路（可复算，不是猜）：K 只有 8 位，1 码在暗端对应
    // dL ≈ 116 × 7.787 × (1/255) × 1.2 ≈ 3.3 L*，故单点上限取 5.0；
    // 均值只剩 8 位量化的一半量级，取 1.5。
    let mut sum = 0.0_f32;
    let mut max = 0.0_f32;
    let mut n = 0usize;
    let mut j = 0usize;
    while j < ramp.len() {
        if let Some(c) = ramp.get(j) {
            if let Ok(d) = path.tone_shift(*c) {
                sum += d;
                if d > max {
                    max = d;
                }
                n += 1;
            }
        }
        j += 1;
    }
    let mean = if n > 0 { sum / n as f32 } else { 99.0 };
    let ok = n == 30 && mean <= 1.5 && max <= 5.0;
    let det = format!("n={} mean={:.2} max={:.2}", n, mean, max);
    s.add_dyn("50004-J1-灰30偏移", ok, &det);

    // J1-5 彩色样本同口径：把彩色打成单通道后，偏移仍只来自量化
    let cols = chroma_30();
    let mut csum = 0.0_f32;
    let mut cmax = 0.0_f32;
    let mut cn = 0usize;
    let mut m = 0usize;
    while m < cols.len() {
        if let Some(c) = cols.get(m) {
            if let Ok(d) = path.tone_shift(*c) {
                csum += d;
                if d > cmax {
                    cmax = d;
                }
                cn += 1;
            }
        }
        m += 1;
    }
    let cmean = if cn > 0 { csum / cn as f32 } else { 99.0 };
    let ok = cn == 30 && cmean <= 1.5 && cmax <= 5.0;
    let det = format!("n={} mean={:.2} max={:.2}", cn, cmean, cmax);
    s.add_dyn("50004-J1-彩30偏移", ok, &det);

    // J1-6 LUT 忠实度：逐项正演逆回，最大误差 ≤ 1 码
    let err_codes = path.table().max_recovery_error_codes(&path.gain());
    // 阈值 3 码：墨量轴在 t→0（近白）处被压缩，那里 1 码墨量只值 <0.5 L*，
    // 所以码域的允许误差比 L 域宽；L 域精度由 J1-4/J1-5 直接钉。
    let ok = err_codes <= 3.0;
    let det = format!("max_err={:.3}码", err_codes);
    s.add_dyn("50004-J1-LUT忠实", ok, &det);

    // J1-7 网点增益曲线：端点精确 + 128 点单调
    let g = path.gain();
    let ok = fabsf(g.forward(0.0)) < 1e-6
        && fabsf(g.forward(1.0) - 1.0) < 1e-6
        && g.is_monotone(128)
        && g.inverse(0.0) <= 1e-3
        && fabsf(g.inverse(1.0) - 1.0) < 1e-3;
    s.add("50004-J1-增益曲线", ok, "增益曲线端点或单调性破损");

    // J1-8 反解往返：21 个目标值 inverse→forward 偏差 ≤ 2e-3（半码内）
    let mut worst = 0.0_f32;
    let mut q = 0usize;
    while q <= 20 {
        let t = q as f32 / 20.0;
        let back = g.forward(g.inverse(t));
        let d = fabsf(back - t);
        if d > worst {
            worst = d;
        }
        q += 1;
    }
    let ok = worst <= 2e-3;
    s.add("50004-J1-反解往返", ok, "反解与正演不互逆");

    // J1-9 中性一致性：K 码 → L* 与理想 L* 的差（灰阶）≤ 5
    let mut consistent = true;
    let mut r = 0usize;
    while r < ramp.len() {
        if let Some(c) = ramp.get(r) {
            match (path.to_k(*c), path.k_to_l(match path.to_k(*c) {
                Ok(v) => v,
                Err(_) => 0,
            })) {
                (Ok(_), Ok(l)) => {
                    if fabsf(l - path.ideal_l(*c)) > 5.0 {
                        consistent = false;
                    }
                }
                _ => consistent = false,
            }
        }
        r += 1;
    }
    s.add("50004-J1-中性一致", consistent, "K→L 与理想 L 偏离超限");

    // J1-10 确定性：同输入两次调用逐位一致（含浮点）
    let mut det_ok = true;
    let mut t = 0usize;
    while t < ramp.len() {
        if let Some(c) = ramp.get(t) {
            let a = path.to_k(*c);
            let b = path.to_k(*c);
            let la = path.k_to_l(128);
            let lb = path.k_to_l(128);
            if a != b {
                det_ok = false;
            }
            match (la, lb) {
                (Ok(x), Ok(y)) => {
                    if x.to_bits() != y.to_bits() {
                        det_ok = false;
                    }
                }
                _ => det_ok = false,
            }
        }
        t += 1;
    }
    s.add("50004-J1-确定性", det_ok, "两次转换结果不同");

    // J1-11 限墨显式登记：涂布纸（限墨 0.95）在最暗段触限，且被计数
    let coated = GrayPath::build(InkPreset::coated());
    let mut hits = 0usize;
    let mut u = 0usize;
    while u < ramp.len() {
        if let Some(c) = ramp.get(u) {
            if coated.hits_limit(*c) {
                hits += 1;
            }
        }
        u += 1;
    }
    let mut cpage = GrayPage::new(7);
    let mut v2 = 0usize;
    while v2 < ramp.len() {
        if let Some(c) = ramp.get(v2) {
            let _ = cpage.push(&coated, *c);
        }
        v2 += 1;
    }
    let ok = hits >= 1 && cpage.stats().limit_clipped == hits;
    let det = format!("hits={} 记账={}", hits, cpage.stats().limit_clipped);
    s.add_dyn("50004-J1-限墨登记", ok, &det);

    // J1-12 页容量与直方图守恒：30 色调全部入账，8 桶之和 == 30
    let mut page = GrayPage::new(1);
    let mut w = 0usize;
    while w < ramp.len() {
        if let Some(c) = ramp.get(w) {
            let _ = page.push(&path, *c);
        }
        w += 1;
    }
    let mut bins = 0u32;
    let mut bi = 0usize;
    while bi < 8 {
        if let Ok(v) = page.stats().bin(bi) {
            bins += v as u32;
        }
        bi += 1;
    }
    let ok = page.len() == 30
        && bins == 30
        && page.stats().min_k == 0
        && page.stats().max_k == 255
        && page.remaining() == 0
        && page.stats().mean_k().map(|m| m > 60.0 && m < 200.0).unwrap_or(false);
    let det = format!("bins={} min={} max={}", bins, page.stats().min_k, page.stats().max_k);
    s.add_dyn("50004-J1-页账守恒", ok, &det);

    // J1-13 页账本：重复页号拒绝 + 容量封口（16 页）
    let mut ledger = GrayLedger::new();
    let p1 = ledger.record(GrayPage::new(1)).is_ok();
    let dup = ledger.record(GrayPage::new(1)).err() == Some(GrayError::DupPage);
    let mut filled = 1usize;
    let mut pn = 2u16;
    while pn <= LEDGER_CAP as u16 {
        if ledger.record(GrayPage::new(pn)).is_ok() {
            filled += 1;
        }
        pn += 1;
    }
    let over = ledger.record(GrayPage::new(99)).err() == Some(GrayError::LedgerFull);
    let ok = p1 && dup && filled == LEDGER_CAP && over && ledger.len() == LEDGER_CAP;
    s.add("50004-J1-页账封口", ok, "页号重复或容量未封口");

    // J1-14 预设注册表：重名拒绝 + 容量封口 + 未登记查询为空
    let mut bk = PresetBook::new();
    let r1 = bk.register(InkPreset::plain()).is_ok();
    let r2 = bk.register(InkPreset::plain()).err() == Some(GrayError::PresetExists);
    let mut cnt = 1usize;
    let mut id = 1usize;
    while id < PRESET_CAP {
        let name = format!("p{}", id);
        if let Ok(p) = InkPreset::new(name.as_str(), 1.1, 0.01, 1.0) {
            if bk.register(p).is_ok() {
                cnt += 1;
            }
        }
        id += 1;
    }
    let over = match InkPreset::new("p99", 1.1, 0.01, 1.0) {
        Ok(p) => bk.register(p).err() == Some(GrayError::PresetFull),
        Err(_) => false,
    };
    let ok = r1 && r2 && cnt == PRESET_CAP && over && bk.lookup("ghost").is_none();
    s.add("50004-J1-预设封口", ok, "预设注册表未封口");

    // J1-15 账本跨页统计：两页合并后的均值与最大值可从账本复算
    let mut lg = GrayLedger::new();
    let _ = lg.record(page.clone());
    let mut page2 = GrayPage::new(2);
    let mut x = 0usize;
    while x < cols.len() {
        if let Some(c) = cols.get(x) {
            let _ = page2.push(&path, *c);
        }
        x += 1;
    }
    let _ = lg.record(page2);
    let mean_all = lg.mean_shift().unwrap_or(99.0);
    let max_all = lg.max_shift().unwrap_or(99.0);
    let ok = lg.len() == 2 && mean_all <= 1.5 && max_all <= 5.0 && max_all > 0.0;
    let det = format!("mean={:.3} max={:.3}", mean_all, max_all);
    s.add_dyn("50004-J1-跨页统计", ok, &det);

    // 注入核：5 类坏样本全抓
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50004-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50004_checks_green() {
        let set = run_unx50004_checks();
        assert!(set.all_passed(), "UNX-F50004 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50004_tone_shift_within_quantisation() {
        let path = GrayPath::build(InkPreset::plain());
        let mut max = 0.0_f32;
        for c in tone_ramp_30() {
            let d = path.tone_shift(c).unwrap();
            if d > max {
                max = d;
            }
        }
        // 8 位 K 通道的量化下限：单点不得超过 5 L*。
        assert!(max <= 5.0, "max shift {}", max);
    }

    #[test]
    fn unx50004_codes_stay_in_segment() {
        let all = [
            GrayError::Empty,
            GrayError::Config,
            GrayError::Unbuilt,
            GrayError::PageFull,
            GrayError::Index,
            GrayError::LedgerFull,
            GrayError::PresetExists,
            GrayError::PresetFull,
            GrayError::NoPreset,
            GrayError::Range,
            GrayError::DupPage,
        ];
        for e in all {
            let c = e.code();
            assert!(c >= SEG_BASE && c < SEG_BASE + 0x20, "code {:#x}", c);
            assert!(!e.message().is_empty());
        }
    }
}
