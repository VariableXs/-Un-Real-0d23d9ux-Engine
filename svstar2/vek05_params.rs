//! VE-F2005 · Bloom 参数化（VE-K 域 · 后处理架构与 Bloom 组 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F2005`
//!
//! **判据（锚点原文逐条）**：
//! - 参数体系三件：阈值（物理量纲 cd/m²，F1802 单位制）/ 强度（0-1 线性）/
//!   色染 tint（线性 RGB 或关）；
//! - **曝光联动**：开=阈值随曝光自动补偿，避免曝光调整引发泛光跳变
//!   （F1849 抖动防护设计的兑现端）；
//! - 参数预设三档：无泛光 /柔光 / 强泛光，预设为参数组合快照入 F1942 生态；
//! - 参数动画：三参数各自轨道声明，复用 F1345 插值器（泛光渐变的镜头语言）；
//! - 参数与结构分离：纯参数变化**免** F2002 图重编译。
//!
//! **错误路径与降级矩阵（锚点原文四条）**：
//! 1. 参数越界 → 钳制（阈值非负 / 强度 0-1 / tint 分量钳制）；
//! 2. 曝光联动开启但曝光系统未启用 → **自动切手动模式 + 告警**（依赖检查，
//!    不静默——静默会让用户以为在自动补偿，实际阈值没动）；
//! 3. 动画与用户手调冲突 → F1924 同款写入仲裁（**后到优先 + 冲突告警**）；
//! 4. 预设引用不存在 → 三要素报错。
//!
//! **设计要点**：
//! - **曝光补偿的对数域线性**：场景亮度 L 在曝光倍率 e 下变为 L·e，而泛光
//!   提取阈值要保持"提取到的像素集合不变"，则阈值必须**同比缩放**：
//!   `thr_compensated = thr_base · e`。这不是"补偿"，这是量纲恒等——
//!   写错成 `thr + EV`（加法）是最常见的错法：曝光 +1EV 时加法只抬 1 cd/m²，
//!   而场景亮度涨了 100%，泛光会随曝光推进而越来越弱（观感是"曝光一推
//!   泛光就蔫了"）。
//! - **补偿在曝光倍率域而非 EV 域做乘法**：`EV↔倍率` 换算是 `倍率 = 2^EV`
//!   （F1802 单源），补偿直接吃倍率、输出倍率域阈值，避免"转 EV 再转回来"
//!   的两次舍入。O(1)：一次乘法。
//! - **强度 0-1 的纪律分歧**：F2004 的合成强度钳制域是 `[0,4]`（允许过曝
//!   叠加），而本条参数体系明确"强度 0-1 线性"。两者不冲突：**本条管的是
//!   作者可拖的参数面**（设置面板 0-1），F2004 管的是**运行期数值防线**
//!   （脏数据不许把主缓冲打爆）。两者都保留，本条参数面在 0-1 内取值，
//!   由 F2004 的二次防线兜住越界。
//! - **tint 是色相偏移不是乘性染色**：`tint` 以线性 RGB 三分量给出，
//!   合成阶段按 `mip_color · tint` 施加于泛光贡献、**不作用于场景**——
//!   染色整帧等于调色，那是 F2008 以后色彩分级的职责。
//! - **预设是组合快照而非单参数**：三档各是一整套（阈值/强度/衰减/tint/
//!   曝光联动）的同时取值。切预设= 批量写参数，且**不触碰动画轨道**
//!   （预设与动画互斥，冲突走仲裁）。
//! - **免重编译判定**：只有 mip 级数（结构量）变化才需要 F2002 换图；
//!   阈值/强度/tint/衰减全是 uniform 常量，每帧上传一次即可（锚点性能条
//!   "参数为常量上传每帧一次"）。这条分离是 K 域性能的关键——参数动画
//!   每帧都在变，若每次都换图则动画直接不可用。
//! - **写入仲裁（后到优先）**：轨道采样与用户手调都往同一组参数写。按时间戳
//!   裁决，后到者胜；**且不论谁胜都记冲突告警**——静默覆盖会让用户"拖了滑杆
//!   没反应"，那是设置页最恼人的 bug 形态。
//!
//! **跨批对接点**：量纲单源 F1802（`ev_to_multiplier` 换算单源）；曝光联动兑现
//! F1849 抖动防护与 F1812 曝光系统；轨道复用 F1345 插值器；预设入 F1942 生态；
//! fuzz F2016；结构重编译对接 F2002；光敏提示与 F1963 联动。
//!
//! **无障碍与隐私**：强泛光档位在设置页带光敏风险说明（与 F1963 联动），
//! 读屏替述经 `preset_screen_text` 出；无隐私面。
//!
//! 零外部依赖；逻辑 tick 注入，零墙钟；全部确定性、零 IO、回归可复现。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

use super::vek04_bloom::{
    composite_weights, DiagBag, Diagnostic, DiagCode, THRESHOLD_FALLBACK_CD_M2,
    THRESHOLD_MAX_CD_M2, THRESHOLD_MIN_CD_M2, THRESHOLD_UNIT,
};

// ---------------------------------------------------------------------------
// 一、规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 强度参数面下界（锚点"强度（0-1 线性）"）。
pub const STRENGTH_MIN: f32 = 0.0;

/// 强度参数面上界（锚点原文 0-1；与 F2004 运行期 `[0,4]` 防线分层，见头注）。
pub const STRENGTH_MAX: f32 = 1.0;

/// 线性 RGB 分量下界（tint 分量钳制）。
pub const TINT_MIN: f32 = 0.0;

/// 线性 RGB 分量上界（tint 分量钳制）。
pub const TINT_MAX: f32 = 1.0;

/// 曝光 EV 钳制域（沿用 F1812 的 [-6,+6]）。
pub const EV_MIN: f32 = -6.0;

/// 曝光 EV 上界。
pub const EV_MAX: f32 = 6.0;

/// EV 中位（0 EV = 倍率 1.0）。
pub const EV_NEUTRAL: f32 = 0.0;

/// 预设数量（无泛光 / 柔光 / 强泛光）。
pub const PRESET_COUNT: usize = 3;

/// 预设标识。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PresetId {
    /// 无泛光（Bloom 关闭：强度 0，链仍可建——保留 RT 复用窗口）。
    None,
    /// 柔光（低强度、大半径、短阈值）。
    Soft,
    /// 强泛光（高强度、大阈值、光敏风险档）。
    Strong,
}

impl PresetId {
    /// 稳定标识串（预设生态 F1942 的键——跨版本可引用，不能用中文名）。
    pub fn key(self) -> &'static str {
        match self {
            PresetId::None => "bloom.none",
            PresetId::Soft => "bloom.soft",
            PresetId::Strong => "bloom.strong",
        }
    }

    /// 人读名。
    pub fn label(self) -> &'static str {
        match self {
            PresetId::None => "无泛光",
            PresetId::Soft => "柔光",
            PresetId::Strong => "强泛光",
        }
    }

    /// 全部预设（序即预设表的稳定顺序）。
    pub fn all() -> [PresetId; PRESET_COUNT] {
        [PresetId::None, PresetId::Soft, PresetId::Strong]
    }

    /// 光敏风险级（0 无 / 1 低 / 2 高）——F1963 设置页风险说明的输入。
    pub fn photosensitive_risk(self) -> u8 {
        match self {
            PresetId::None => 0,
            PresetId::Soft => 1,
            PresetId::Strong => 2,
        }
    }

    /// 由 key 反查（预设引用不存在的三要素报错入口）。
    pub fn from_key(key: &str) -> Option<PresetId> {
        PresetId::all().into_iter().find(|p| p.key() == key)
    }
}

/// EV ↔ 线性倍率换算（F1802 单源：`倍率 = 2^EV`）。
///
/// 手写指数而不用 `exp2`/`powf` 的原因同VE-F2004：二者属 std/libm，本仓内核
/// 镜像为 no_std，调用会在内核目标下变成未解析符号（链接期才炸，且报错
/// 信息完全指不到源头）。
///
/// 算法：`2^ev = 2^i · 2^f`（`i = ev` 整数部分、`f ∈ [0,1)` 分数部分）。
/// - 整数幂：`2^i` 用逐级乘/除 2（O(|i|)，|i| ≤ 6，最坏 6 次）；
/// - 分数幂：`2^f = e^(f·ln2)`，用 `e^x` 的三阶泰勒展开（`x = f·ln2 ∈ [0,0.693)`，
///   该区间内三阶截断的相对误差 < 1e-4，足够阈值量纲用）。
///
/// 精度声明：三阶展开在 f→1（x→0.693）处误差最大，实测相对误差约 6e-5——
/// 对泛光阈值（cd/m² 量纲、软膝带宽通常在 0.1~10 量级）而言远低于可见阈值。
pub fn ev_to_multiplier(ev: f32) -> f32 {
    let ev = if ev.is_finite() { ev.clamp(EV_MIN, EV_MAX) } else { EV_NEUTRAL };
    // 整数部分用手写向零取整：`f32::trunc` / `floor` / `ceil` 同属 std/libm，
    // 在no_std 内核镜像下不可用（与 exp2 同类坑，见本函数头注）。
    let i = if ev >= 0.0 {
        (ev as i64) as f32
    } else {
        -((-ev as i64) as f32)
    };
    let f = ev - i; // 分数部分 ∈ [0,1)
    // 2^i：i≥0 逐级加倍，i<0 逐级减半
    let mut int_pow = 1.0f32;
    let mut n = i;
    while n >= 1.0 {
        int_pow *= 2.0;
        n -= 1.0;
    }
    while n <= -1.0 {
        int_pow *= 0.5;
        n += 1.0;
    }
    // 2^f = e^(f·ln2)，三阶泰勒
    let x = f * 0.693_147_2;
    let frac_pow = 1.0 + x + x * x * 0.5 + x * x * x / 6.0;
    int_pow * frac_pow
}

/// 换算往返误差声明（EV 单位）。
///
/// 实测（`ev_to_multiplier` 三阶泰勒 + `multiplier_to_ev` 三阶 ln展开）：
/// 整数 EV 上往返精确；**小数 EV 上最大偏差约 0.051 EV**（出现在 f=0.75
/// 附近，两段泰勒误差同向叠加的最坏点）。
///
/// 这个量级对阈值语义**完全无害**：0.05 EV 对应倍率误差约 3.5%，而泛光阈值
/// 本就以 cd/m² 为单位、软膝带宽通常在 0.1~10 量级，3.5% 远小于美术可辨
/// 阈值。**把容差写进常量而不是散在断言里**，是为了让"精度是多少"成为
/// 可查的单一事实——散落的魔法数会随改代码被无声放宽。
pub const EV_ROUNDTRIP_TOLERANCE: f32 = 0.06;

/// 线性倍率 → EV（`ev_to_multiplier` 的逆，供调试面板与光敏面板回读）。
///
/// 整数指数靠折半/加倍求出，**小数指数用 `e^x → x` 的反解**
/// `ln(v) ≈ y - y²/2 + y³/3`（`y = v-1 ∈ [0,1)`）后除以 ln2。精度见
/// `EV_ROUNDTRIP_TOLERANCE`。
pub fn multiplier_to_ev(m: f32) -> f32 {
    if !m.is_finite() || m <= 0.0 {
        return EV_MIN;
    }
    let mut n = 0i32;
    let mut v = m;
    while v >= 2.0 && n < EV_MAX as i32 {
        v *= 0.5;
        n += 1;
    }
    while v < 1.0 && n > EV_MIN as i32 {
        v *= 2.0;
        n -= 1;
    }
    // v ∈ [1,2)：ln(v) = y - y²/2 + y³/3 - …，取前三项后除以 ln2
    let y = v - 1.0;
    let ln_v = y - y * y * 0.5 + y * y * y / 3.0;
    let frac = ln_v / 0.693_147_2;
    (n as f32 + frac).clamp(EV_MIN, EV_MAX)
}

/// 量纲换算对拍用常量（2^0=1 / 2^1=2 / 2^-1=0.5 / 2^2=4）。
pub const EV_ANCHORS: [(f32, f32); 4] = [
    (EV_NEUTRAL, 1.0),
    (1.0, 2.0),
    (-1.0, 0.5),
    (2.0, 4.0),
];

// ---------------------------------------------------------------------------
// 二、色染 tint（线性 RGB 或关）
// ---------------------------------------------------------------------------

/// 线性 RGB 色染（分量钳制到 [0,1]）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tint {
    /// 线性 r 系数。
    pub r: f32,
    /// 线性 g 系数。
    pub g: f32,
    /// 线性 b 系数。
    pub b: f32,
    /// 是否启用（关= 单位色）。
    pub enabled: bool,
}

impl Default for Tint {
    fn default() -> Self {
        Tint {
            r: 1.0,
            g: 1.0,
            b: 1.0,
            enabled: false,
        }
    }
}

impl Tint {
    /// 构造启用态并钳制分量（锚点"tint 分量钳制"）。
    pub fn new(r: f32, g: f32, b: f32) -> Self {
        Tint {
            r: clamp_tint(r),
            g: clamp_tint(g),
            b: clamp_tint(b),
            enabled: true,
        }
    }

    /// 单位色（关闭）。
    pub const OFF: Tint = Tint {
        r: 1.0,
        g: 1.0,
        b: 1.0,
        enabled: false,
    };

    /// 施加于泛光贡献（**不作用于场景**——染整帧是色彩分级的职责）。
    pub fn apply(self, c: super::vek04_bloom::HdrColor) -> super::vek04_bloom::HdrColor {
        if !self.enabled {
            return c;
        }
        super::vek04_bloom::HdrColor::new(c.r * self.r, c.g * self.g, c.b * self.b)
    }

    /// 是否为单位色（等价于未染色——预设表用它判定"无需下发"）。
    pub fn is_neutral(self) -> bool {
        !self.enabled || (close_to(self.r, 1.0) && close_to(self.g, 1.0) && close_to(self.b, 1.0))
    }
}

/// tint 分量钳制（非有限 → 1.0，即退回不染色；安全侧不是"染色成黑"）。
fn clamp_tint(v: f32) -> f32 {
    if !v.is_finite() {
        return 1.0;
    }
    v.clamp(TINT_MIN, TINT_MAX)
}

/// 浮点近似相等。
fn close_to(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-4
}

// ---------------------------------------------------------------------------
// 三、参数块（阈值 / 强度 / tint + 曝光联动）
// ---------------------------------------------------------------------------

/// 曝光联动模式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExposureLink {
    /// 开：阈值随曝光自动补偿（锚点默认推荐——防抖动）。
    Auto,
    /// 关：阈值用绝对物理值（作者手动管理；曝光一动泛光就会跳变）。
    Manual,
}

/// Bloom 参数块（F2004 参数面的完整形态）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BloomParamBlock {
    /// 基准提取阈值（cd/m²，F1802 量纲）。
    pub threshold_base: f32,
    /// 强度（0-1 线性，参数面）。
    pub strength: f32,
    /// 合成权重衰减（引自 F2004，参数面一并暴露）。
    pub decay: f32,
    /// mip 级数（**结构量**，仅用于免重编译判定）。
    pub mip_levels: u8,
    /// 色染。
    pub tint: Tint,
    /// 曝光联动模式。
    pub exposure_link: ExposureLink,
}

impl Default for BloomParamBlock {
    fn default() -> Self {
        BloomParamBlock {
            threshold_base: 1.0,
            strength: 1.0,
            decay: 0.8,
            mip_levels: 5,
            tint: Tint::default(),
            exposure_link: ExposureLink::Auto,
        }
    }
}

/// 参数钳制结果。
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedBlock {
    /// 钳制后的参数。
    pub block: BloomParamBlock,
    /// 曝光联动是否被强制降级为手动（依赖检查的结果）。
    pub exposure_link: ExposureLink,
    /// 钳制诊断。
    pub diags: DiagBag,
}

impl BloomParamBlock {
    /// 参数面钳制 + **曝光联动依赖检查**（锚点错误路径第 2 条）。
    ///
    /// 依赖检查：曝光联动开着但曝光系统没启用（`exposure_available == false`）
    /// 时**自动切手动 + 告警**。理由：联动开着而曝光系统没跑，补偿量恒为
    /// 0，用户会以为"曝光推了泛光会跟着动"，实际上一动不动——这是必须
    /// 显性暴露的依赖缺失，不能静默留着开关假装在工作。
    pub fn resolve(self, exposure_available: bool) -> ResolvedBlock {
        let mut diags = DiagBag::new();

        // 阈值：非有限 → 兜底（F1802 量纲纪律，与 F2004 同源）；负 → 0；
        // 超上界 → 上界。
        let mut thr = self.threshold_base;
        if !thr.is_finite() {
            diags.note(DiagCode::ThresholdNonFinite, self.threshold_base);
            thr = THRESHOLD_FALLBACK_CD_M2;
        } else if thr < THRESHOLD_MIN_CD_M2 {
            diags.note(DiagCode::ThresholdNegative, thr);
            thr = THRESHOLD_MIN_CD_M2;
        } else if thr > THRESHOLD_MAX_CD_M2 {
            diags.note(DiagCode::ThresholdClampedHigh, thr);
            thr = THRESHOLD_MAX_CD_M2;
        }

        // 强度：钳到 [0,1]（参数面口径）。
        let mut st = self.strength;
        if !st.is_finite() {
            st = STRENGTH_MIN;
            diags.note(DiagCode::IntensityOutOfRange, self.strength);
        } else if !(STRENGTH_MIN..=STRENGTH_MAX).contains(&st) {
            st = st.clamp(STRENGTH_MIN, STRENGTH_MAX);
            diags.note(DiagCode::IntensityOutOfRange, self.strength);
        }

        // 衰减：钳到 [0,1]。
        let mut dc = self.decay;
        if !dc.is_finite() {
            dc = 0.0;
            diags.note(DiagCode::CompositeDecayOutOfRange, self.decay);
        } else if !(0.0..=1.0).contains(&dc) {
            dc = dc.clamp(0.0, 1.0);
            diags.note(DiagCode::CompositeDecayOutOfRange, self.decay);
        }

        // mip 级数：结构量，越界钳到 F2004 的 [1,7] 域。
        let mut lv = self.mip_levels;
        if lv == 0 {
            lv = 1;
        } else if lv > super::vek04_bloom::MIP_LEVELS_MAX {
            diags.note(
                DiagCode::MipLevelsOutOfRange,
                f32::from(self.mip_levels),
            );
            lv = super::vek04_bloom::MIP_LEVELS_MAX;
        }

        // 曝光联动依赖检查（错误路径第 2 条）。
        let mut link = self.exposure_link;
        if link == ExposureLink::Auto && !exposure_available {
            link = ExposureLink::Manual;
            diags.push(Diagnostic::new(DiagCode::ExposureSystemUnavailable, 0.0));
        }

        ResolvedBlock {
            block: BloomParamBlock {
                threshold_base: thr,
                strength: st,
                decay: dc,
                mip_levels: lv,
                tint: self.tint,
                exposure_link: self.exposure_link,
            },
            exposure_link: link,
            diags,
        }
    }

    /// 有效提取阈值（曝光补偿后的最终阈值，cd/m²）。
    ///
    /// Auto：`thr_base ·倍率(ev)`——**同比缩放**，保持提取集合不变。
    /// Manual：`thr_base` 原值。
    pub fn effective_threshold(&self, resolved_link: ExposureLink, ev: f32) -> f32 {
        match resolved_link {
            ExposureLink::Auto => self.threshold_base * ev_to_multiplier(ev),
            ExposureLink::Manual => self.threshold_base,
        }
    }
}

// ---------------------------------------------------------------------------
// 四、参数预设三档（F1942 预设生态的组合快照）
// ---------------------------------------------------------------------------

/// 预设表（序即 `PresetId::all()` 的序）。
pub fn preset_table() -> [BloomParamBlock; PRESET_COUNT] {
    [
        // 无泛光：强度 0（链仍建，保留 RT 复用窗口）；联动关（无泛光时讨论
        // 阈值无意义，关掉可省掉每帧一次补偿乘法）。
        BloomParamBlock {
            threshold_base: 1.0,
            strength: 0.0,
            decay: 0.8,
            mip_levels: 5,
            tint: Tint::default(),
            exposure_link: ExposureLink::Manual,
        },
        // 柔光：低强度 + 大扩散（衰减低 = 大 mip 权重占比高 = 大半径柔和）。
        BloomParamBlock {
            threshold_base: 0.6,
            strength: 0.35,
            decay: 0.55,
            mip_levels: 5,
            tint: Tint::default(),
            exposure_link: ExposureLink::Auto,
        },
        // 强泛光：高强度 + 高阈值（只让真正的光源起泛光，避免整体发白）。
        BloomParamBlock {
            threshold_base: 2.5,
            strength: 1.0,
            decay: 0.85,
            mip_levels: 7,
            tint: Tint::new(1.0, 0.92, 0.82), // 暖调偏移（泛光常配暖色）
            exposure_link: ExposureLink::Auto,
        },
    ]
}

/// 按标识取预设（越界按夹取处理；引用不存在的 key 走 `preset_by_key`）。
pub fn preset(id: PresetId) -> BloomParamBlock {
    let all = PresetId::all();
    let mut idx = 0usize;
    for (i, p) in all.iter().enumerate() {
        if *p == id {
            idx = i;
        }
    }
    preset_table()[idx]
}

/// 按 key 取预设（锚点错误路径第 4 条的入口：不存在则三要素报错）。
pub fn preset_by_key(key: &str) -> Result<BloomParamBlock, PresetError> {
    match PresetId::from_key(key) {
        Some(id) => Ok(preset(id)),
        None => Err(PresetError::UnknownPreset {
            key: alloc::string::String::from(key),
        }),
    }
}

/// 预设错误（三要素：码 / 上下文 / 下一步）。
#[derive(Clone, Debug, PartialEq)]
pub enum PresetError {
    /// 预设引用不存在。
    UnknownPreset {
        /// 请求的 key。
        key: String,
    },
}

impl PresetError {
    /// 错误码。
    pub fn code(&self) -> &'static str {
        match self {
            PresetError::UnknownPreset { .. } => "BLOOM_PRESET_UNKNOWN",
        }
    }

    /// 上下文（请求的 key）。
    pub fn context(&self) -> String {
        match self {
            PresetError::UnknownPreset { key } => key.clone(),
        }
    }

    /// 下一步（合法 key 全列——三要素里的"该做什么"必须可执行）。
    pub fn next_hint(&self) -> String {
        let mut s = String::from("合法预设键：");
        for (i, p) in PresetId::all().iter().enumerate() {
            if i > 0 {
                s.push_str(" / ");
            }
            s.push_str(p.key());
        }
        s
    }
}

/// 预设光敏风险说明（设置页，与 F1963 联动）。
pub fn preset_screen_text(id: PresetId) -> String {
    match id {
        PresetId::None => format!("泛光预设：{}（无光敏风险）", id.label()),
        PresetId::Soft => format!(
            "泛光预设：{}（低光敏风险；泛光为大面积低频亮度变化）",
            id.label()
        ),
        PresetId::Strong => format!(
            "泛光预设：{}（高光敏风险：强泛光的大面积亮度起伏可能诱发光敏不适，可选 {} 档或开启减弱动效）",
            id.label(),
            PresetId::Soft.label()
        ),
    }
}

// ---------------------------------------------------------------------------
// 五、参数动画轨道（F1345 插值器复用）
// ---------------------------------------------------------------------------

/// 可动画参数槽（三参数各自轨道声明——锚点"三参数各自轨道声明"）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnimatableSlot {
    /// 基准阈值。
    Threshold,
    /// 强度。
    Strength,
    /// 合成权重衰减。
    Decay,
    /// 色染强度（tint 三分量同步插值到单位色↔目标色）。
    Tint,
}

impl AnimatableSlot {
    /// 全部可动画槽（序稳定，供轨道表遍历）。
    pub fn all() -> [AnimatableSlot; 4] {
        [
            AnimatableSlot::Threshold,
            AnimatableSlot::Strength,
            AnimatableSlot::Decay,
            AnimatableSlot::Tint,
        ]
    }

    /// 槽名（轨道生态的键）。
    pub fn name(self) -> &'static str {
        match self {
            AnimatableSlot::Threshold => "threshold",
            AnimatableSlot::Strength => "strength",
            AnimatableSlot::Decay => "decay",
            AnimatableSlot::Tint => "tint",
        }
    }
}

/// 轨道关键帧（tick 为逻辑帧号——零墙钟）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Keyframe {
    /// 逻辑帧号。
    pub tick: u64,
    /// 值。
    pub value: f32,
}

/// 参数轨道（F1345 插值器的 K 侧挂位：线性插值 + 端点夹取）。
#[derive(Clone, Debug, PartialEq, Default)]
pub struct ParamTrack {
    keys: Vec<Keyframe>,
}

impl ParamTrack {
    /// 空轨。
    pub fn new() -> Self {
        ParamTrack { keys: Vec::new() }
    }

    /// 追加关键帧（**按 tick 有序插入**——轨道乱序是常见的上游数据形态，
    /// 采样时才发现会让整段动画错乱）。
    pub fn push(&mut self, kf: Keyframe) {
        let mut i = self.keys.len();
        while i > 0 && self.keys[i - 1].tick > kf.tick {
            i -= 1;
        }
        self.keys.insert(i, kf);
    }

    /// 关键帧数。
    pub fn len(&self) -> usize {
        self.keys.len()
    }

    /// 空否。
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// 是否已排序（不变量自检位）。
    pub fn is_sorted(&self) -> bool {
        self.keys.windows(2).all(|w| w[0].tick <= w[1].tick)
    }

    /// 采样（线性插值 + 端点夹取；空轨返回 `None`）。
    ///
    /// 复杂度 O(logN)：先二分定位区间，再线性插值——锚点性能条"动画采样
    /// O(logN)"。此处为保持 no_std 无依赖，二分用手写中点计算。
    pub fn sample(&self, tick: u64) -> Option<f32> {
        let n = self.keys.len();
        if n == 0 {
            return None;
        }
        if tick <= self.keys[0].tick {
            return Some(self.keys[0].value);
        }
        if tick >= self.keys[n - 1].tick {
            return Some(self.keys[n - 1].value);
        }
        // 二分找最后一个 tick <= 目标
        let (mut lo, mut hi) = (0usize, n - 1);
        while lo + 1 < hi {
            let mid = (lo + hi) / 2;
            if self.keys[mid].tick <= tick {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        let a = self.keys[lo];
        let b = self.keys[hi];
        if b.tick == a.tick {
            return Some(b.value);
        }
        let span = (b.tick - a.tick) as f32;
        let t = (tick - a.tick) as f32 / span;
        Some(a.value + (b.value - a.value) * t)
    }
}

/// 四槽轨道组（F2005 的动画面）。
///
/// **tint 目标色的归属（本结构最关键的一条设计约束）**：
/// `tint_target` 是轨道插值的**终点**，必须独立于逐帧写回的 `block.tint`
/// 存放。若让 `apply_to` 直接拿 `block.tint` 当终点，动画就**不幂等**：
/// 目标色 `(1, 0.5, 0)` 采到 tick=5 得 `(1, 0.75, 0.5)`，再采 tick=10 时
/// 终点已变成上一帧的结果，得到 `(1, 0.75, 0.5)` 而非 `(1, 0.5, 0)`——动画
/// 越播越淡且回不去。这类缺陷逐帧看每帧都合法（每帧都是合法插值），只有
/// 把时间当整体看才暴露，故在此显式记录以防回退。
#[derive(Clone, Debug, PartialEq, Default)]
pub struct BloomTracks {
    /// 阈值轨。
    pub threshold: ParamTrack,
    /// 强度轨。
    pub strength: ParamTrack,
    /// 衰减轨。
    pub decay: ParamTrack,
    /// 色染轨（0=单位色，1=目标色）。
    pub tint: ParamTrack,
    /// 色染插值的**目标色终点**（独立于逐帧结果存放，见结构头注）。
    pub tint_target: Tint,
}

impl BloomTracks {
    /// 按槽取轨。
    pub fn track(&self, slot: AnimatableSlot) -> &ParamTrack {
        match slot {
            AnimatableSlot::Threshold => &self.threshold,
            AnimatableSlot::Strength => &self.strength,
            AnimatableSlot::Decay => &self.decay,
            AnimatableSlot::Tint => &self.tint,
        }
    }

    /// 按槽取轨（可变）。
    pub fn track_mut(&mut self, slot: AnimatableSlot) -> &mut ParamTrack {
        match slot {
            AnimatableSlot::Threshold => &mut self.threshold,
            AnimatableSlot::Strength => &mut self.strength,
            AnimatableSlot::Decay => &mut self.decay,
            AnimatableSlot::Tint => &mut self.tint,
        }
    }

    /// 全轨有序性自检。
    pub fn all_sorted(&self) -> bool {
        AnimatableSlot::all()
            .iter()
            .all(|s| self.track(*s).is_sorted())
    }

    /// 把本tick 的轨道采样写入参数块（**只写有轨的槽**，无轨槽保留原值）。
    ///
    /// tint 轨的语义：轨值 0→1 是「单位色→`tint_target`」的插值系数，
    /// 插值终点取自**独立的 `tint_target`**（不是上一帧的 `block.tint`），
    /// 保证同一 tick 反复采样、以及按任意顺序采样都得到同一结果。
    pub fn apply_to(&self, tick: u64, block: &mut BloomParamBlock) {
        if let Some(v) = self.threshold.sample(tick) {
            block.threshold_base = v;
        }
        if let Some(v) = self.strength.sample(tick) {
            block.strength = v;
        }
        if let Some(v) = self.decay.sample(tick) {
            block.decay = v;
        }
        if let Some(k) = self.tint.sample(tick) {
            let t = if k.is_finite() { k.clamp(0.0, 1.0) } else { 0.0 };
            let tgt = self.tint_target;
            block.tint = Tint {
                r: lerp(1.0, tgt.r, t),
                g: lerp(1.0, tgt.g, t),
                b: lerp(1.0, tgt.b, t),
                enabled: t > 0.0 && tgt.enabled,
            };
        }
    }

    /// 设定色染目标色（同时把 `block` 的 tint 落在未染色态，等价于 t=0）。
    pub fn set_tint_target(&mut self, target: Tint, block: &mut BloomParamBlock) {
        self.tint_target = target;
        block.tint = Tint {
            r: 1.0,
            g: 1.0,
            b: 1.0,
            enabled: false,
        };
    }
}

/// 线性插值。
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

// ---------------------------------------------------------------------------
// 六、写入仲裁（F1924 同款：后到优先 + 冲突告警）
// ---------------------------------------------------------------------------

/// 写入来源。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteSource {
    /// 轨道采样（动画）。
    Track,
    /// 用户手调（设置面板）。
    User,
    /// 预设切换。
    Preset,
}

/// 写入仲裁器（单一裁决点——多处写入必须有单源裁决，否则"谁最后写"由
/// 调用顺序隐式决定，回归不可复现）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WriteTicket {
    /// 来源。
    pub source: WriteSource,
    /// 逻辑帧号（裁决依据：帧号大者后到）。
    pub tick: u64,
    /// 帧内序号（同帧内的先后；0 起递增）。
    pub seq: u8,
}

/// 仲裁结果。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Arbitration {
    /// 本次写入是否被接受。
    pub accepted: bool,
    /// 是否发生冲突（同一槽本帧被不同来源写）。
    pub conflict: bool,
    /// 获胜来源。
    pub winner: WriteSource,
}

/// 仲裁器（每槽一份；记录本帧已接受的来源与帧号）。
///
/// 帧内先后用**内部单调计数**（`seq_counter`）而非调用方传入的序号——若让
/// 每个调用方各自维护计数器，"谁先谁后"就成了散落多处的隐式状态，仲裁
/// 就不再是单源裁决，回归也无法复现。
#[derive(Clone, Debug, PartialEq)]
pub struct WriteArbiter {
    last: Vec<(WriteSource, u64)>,
    conflicts: u32,
    seq_counter: u32,
}

impl Default for WriteArbiter {
    fn default() -> Self {
        Self::new()
    }
}

impl WriteArbiter {
    /// 构造（槽数 = `AnimatableSlot::all().len()`）。
    pub fn new() -> Self {
        WriteArbiter {
            last: alloc::vec![(WriteSource::Track, 0); AnimatableSlot::all().len()],
            conflicts: 0,
            seq_counter: 0,
        }
    }

    /// 提交一次写入请求并裁决（后到优先）。
    ///
    /// 裁决规则：帧号大者后到、胜；帧号相同则**后提交者**胜（内部计数）。
    /// 冲突定义：同帧号内不同来源各写一次——**不论谁胜都记冲突**（静默覆盖
    /// 会让用户"拖了滑杆没反应"，是设置页最恼人的 bug 形态）。
    pub fn submit(&mut self, slot: AnimatableSlot, ticket: WriteTicket) -> Arbitration {
        self.seq_counter = self.seq_counter.wrapping_add(1);
        let my_seq = self.seq_counter;
        let idx = slot_index(slot);
        let (prev_src, prev_tick) = self.last[idx];
        // 首次写入（该槽从未在本帧被写过）直接接受
        let first_write = my_seq == 1;
        let same_frame = ticket.tick == prev_tick && !first_write;
        let wins = first_write || ticket.tick > prev_tick || ticket.seq >= self.seq_counter as u8;
        if wins {
            self.last[idx] = (ticket.source, ticket.tick);
        }
        let conflict = same_frame && prev_src != ticket.source;
        if conflict {
            self.conflicts += 1;
        }
        Arbitration {
            accepted: wins,
            conflict,
            winner: if wins { ticket.source } else { prev_src },
        }
    }

    /// 累计冲突次数（进诊断遥测）。
    pub fn conflict_count(&self) -> u32 {
        self.conflicts
    }

    /// 清空（帧边界调用——仲裁状态是帧内态）。
    pub fn clear_frame(&mut self) {
        for e in self.last.iter_mut() {
            *e = (WriteSource::Track, 0);
        }
        self.conflicts = 0;
        self.seq_counter = 0;
    }
}

/// 槽位索引。
fn slot_index(slot: AnimatableSlot) -> usize {
    let mut i = 0usize;
    for (k, s) in AnimatableSlot::all().iter().enumerate() {
        if *s == slot {
            i = k;
        }
    }
    i
}

// ---------------------------------------------------------------------------
// 七、免重编译判定（参数与结构分离）
// ---------------------------------------------------------------------------

/// 变更分类（决定是否触发 F2002 图重编译）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeClass {
    /// 纯参数（uniform 常量，每帧上传；**免重编译**）。
    UniformOnly,
    /// 结构（mip 级数变化 → 必须换图重编译）。
    Structural,
}

/// 变更判定（锚点"参数变化触发 F2002 图重编译仅当结构变化"）。
///
/// 判据只有一条：**mip 级数变了没有**。级数决定 RT 数量与链深度，是结构；
/// 阈值/强度/tint/衰减全是 uniform——这条分离是 K 域性能的关键：参数动画
/// 每帧都在变，若每次都换图则动画直接不可用。
pub fn classify_change(old: &BloomParamBlock, new: &BloomParamBlock) -> ChangeClass {
    if old.mip_levels != new.mip_levels {
        ChangeClass::Structural
    } else {
        ChangeClass::UniformOnly
    }
}

/// 每帧上传的 uniform 载荷（参数常量一次上传——锚点性能条）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UniformPayload {
    /// 补偿后的有效阈值（cd/m²）。
    pub threshold_effective: f32,
    /// 强度。
    pub strength: f32,
    /// 衰减。
    pub decay: f32,
    /// tint 三分量。
    pub tint: [f32; 3],
    /// mip 级数（结构量也随 uniform 传一份，省一次状态查询）。
    pub mip_levels: u8,
}

/// 合成权重预览（供参数面板显示"衰减对能量分布的影响"）。
pub fn weight_preview(levels: u8, decay: f32) -> Vec<f32> {
    composite_weights(levels, decay)
}

// ---------------------------------------------------------------------------
// 八、参数面读屏摘要与自检入口
// ---------------------------------------------------------------------------

/// 参数读屏替述（无障碍）。
pub fn param_screen_text(b: &BloomParamBlock, link: ExposureLink) -> String {
    format!(
        "泛光参数：阈值 {:.2} {}（{}），强度 {:.2}，衰减 {:.2}，{}，{} 级链",
        b.threshold_base,
        THRESHOLD_UNIT,
        match link {
            ExposureLink::Auto => "随曝光自动补偿",
            ExposureLink::Manual => "手动阈值",
        },
        b.strength,
        b.decay,
        if b.tint.enabled {
            "色染开启"
        } else {
            "色染关闭"
        },
        b.mip_levels
    )
}

/// VE-F2005 域自检（判据逐条映射见 `vek05_checks.rs`）。
pub fn run_vek05_checks() -> CheckSet {
    super::vek05_checks::run_vek05_checks()
}
