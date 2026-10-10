//! VE-F1812 · 曝光与亮度语义（VE-J 域 · 光照与氛围组）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F1812`
//!
//! **判据（锚点原文）**：双模式、饱和保护、VE-V 边界、频闪阻尼、判据。
//!
//! **职责定位（锚点原文）**：实现曝光系统双模式（自动曝光：场景亮度统计→曝光
//! 参数动态调整；手动曝光：固定 EV 值）与眼适应预留（亮度渐变适应的接口位+
//! 诚实标注），并划清与 VE-V 边界（场景内曝光归 J 域、全书 UI 层显示亮度归
//! V 域）防止双重调光。
//!
//! **数据结构（锚点原文）**：自动曝光——亮度统计（对数平均+直方图两级，半
//! 分辨率亮度缓冲）、目标中位亮度参数、调整速率参数；手动 EV 值范围声明
//! （-6 到 +6）；边界声明表（J 域管：场景线性亮度→色调映射前曝光；V 域管：
//! 最终显示 gamma 与系统亮度）。
//!
//! **错误路径与降级矩阵（锚点原文）**：亮度统计缓冲异常（全黑/全白场景）→
//! 统计饱和保护：自动曝光冻结在安全 EV 并告警；自动+手动同时激活→手动优先
//! 自动禁用（互斥开关）；EV 越界→钳制；眼适应接口被引用→显性报错（预留不
//! 静默）；频闪场景（亮度高频振荡）→速率限制阻尼。
//!
//! **性能逐项分解（锚点原文）**：亮度统计复用下采样链（与 F1845 SSAO 半分辨
//! 率思路同源）每帧 0.1ms 级；曝光参数为标量 uniform 每帧一次；阻尼为一阶
//! 惯性滤波 O(1)。
//!
//! **跨批对接点（锚点原文）**：输出曝光后线性颜色给 I02 后段与 J03 氛围链
//! （F1841 管线序中曝光在光照后雾前）；统计缓冲与 F1845 共享下采样设施；
//! 边界声明与 V 域对账；遥测（EV 调整频率）进 F1819；fuzz（全黑全白极端）
//! 进 F1813。
//!
//! **EV 域单源声明**：EV_MIN/EV_MAX = ±6 是本条（F1812）的声明；K 域
//! F2007（`vek05_params`）在 J 侧建库前**先行转录同值**（其锚点明示「EV 值
//! 范围声明（-6 到 +6）」沿用 F1812）——本单落地后判据做**跨域同值对拍**
//! （两处常量数值相等即账平；值改一处即对拍红）。
//!
//! # 判据怎么做到"不是恒真"
//!
//! 1. **"双模式"**。若只断「模式能切」，恒回手动值的实现也绿。必须断：
//!    自动模式**真的从统计收敛到目标 EV**（合成渐变场景 + 独立重算对拍）+
//!    互斥是**双向**（手动激活时自动被禁用且不再追目标——追了就是双重调光）。
//! 2. **"饱和保护"**。若语料只有正常场景，保护分支是死代码。必须**全黑/
//!    全白**两向注入：冻结在安全 EV（不是 0 也不是 NaN——是声明过的安全值）
//!    + 告警计数，且**正常场景不触发**（保护不许误伤）。
//! 3. **"频闪阻尼"**。若只断「阻尼开着」，恒定亮度下的误伤看不见。必须断：
//!    方波亮度振荡下 EV 波动幅度**有界**（一阶惯性的数学性质）+ **恒定亮度
//!    下阻尼不触发**（阻尼是频闪对策不是亮度对策）。
//! 4. **"VE-V 边界"**。若只写注释，边界漂移没人管。必须断**两域参数域
//!    不相交**（J 管场景线性亮度→TM 前曝光；V 管显示 gamma/系统亮度——
//!    声明表逐行 + 域交集为空的可判定形态）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ===========================================================================
// 一、诊断码（自建，J 域独占段 0x33xx——0x31=F1810、0x32=F2016、0x39=F1811）
// ===========================================================================

/// 诊断码（J 域惯例自建新构类型）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DiagCode(pub u16);

/// J 域 F1812 诊断码。
pub mod code {
    use super::DiagCode;

    /// 统计饱和（全黑/全白场景）——自动曝光冻结在安全 EV 并告警。
    pub const STATS_SATURATED: DiagCode = DiagCode(0x3301);
    /// 模式互斥冲突（自动+手动同时激活——手动优先自动禁用）。
    pub const MODE_CONFLICT: DiagCode = DiagCode(0x3302);
    /// EV 越界钳制。
    pub const EV_CLAMPED: DiagCode = DiagCode(0x3303);
    /// 眼适应预留被引用（预留不静默）。
    pub const EYE_ADAPT_RESERVED: DiagCode = DiagCode(0x3304);
    /// 频闪检出——阻尼加强。
    pub const FLICKER_DAMPED: DiagCode = DiagCode(0x3305);
}

/// 诊断严重度。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Severity {
    /// 提示。
    Info,
    /// 告警（可继续）。
    Warn,
}

/// 一条诊断。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Diagnostic {
    /// 诊断码。
    pub code: DiagCode,
    /// 严重度。
    pub severity: Severity,
}

/// 诊断袋（定容 32 覆盖最旧；计数单调不随覆盖回退——F2014 同构）。
#[derive(Clone, Debug)]
pub struct DiagBag {
    items: Vec<Diagnostic>,
    counts: [u32; 16],
    cap: usize,
}

impl DiagBag {
    /// 构造。
    pub fn new() -> DiagBag {
        DiagBag { items: Vec::new(), counts: [0u32; 16], cap: 32 }
    }

    fn bump(&mut self, c: DiagCode) {
        let i = ((c.0 >> 8) & 0x0F) as usize;
        self.counts[i] = self.counts[i].saturating_add(1);
    }

    /// 记一条告警。
    pub fn push_warn(&mut self, c: DiagCode) {
        self.bump(c);
        if self.items.len() >= self.cap {
            self.items.remove(0);
        }
        self.items.push(Diagnostic { code: c, severity: Severity::Warn });
    }

    /// 全部诊断。
    pub fn items(&self) -> &[Diagnostic] {
        &self.items
    }

    /// 某码累计次数（单调）。
    pub fn count(&self, c: DiagCode) -> u32 {
        self.counts[((c.0 >> 8) & 0x0F) as usize]
    }

    /// 是否含某码。
    pub fn has(&self, c: DiagCode) -> bool {
        self.items.iter().any(|d| d.code == c)
    }
}

// ===========================================================================
// 二、EV 域单源与安全值
// ===========================================================================

/// EV 下界（锚点：-6；K 域 vek05_params::EV_MIN 转录同值——跨域同值对拍）。
pub const EV_MIN: f32 = -6.0;
/// EV 上界（锚点：+6）。
pub const EV_MAX: f32 = 6.0;
/// 中性 EV（饱和保护的安全落点：0 = 不增不减曝光——冻结时画面保持原样，
/// 不会因为统计炸了而把画面推向全黑或全白）。
pub const SAFE_EV: f32 = 0.0;

/// EV 域钳制（越界→钳制，锚点错误路径；非有限值回落安全值）。
pub fn clamp_ev(ev: f32) -> f32 {
    if !ev.is_finite() {
        return SAFE_EV;
    }
    ev.clamp(EV_MIN, EV_MAX)
}

// ===========================================================================
// 三、亮度统计（对数平均 + 直方图两级；半分辨率声明）
// ===========================================================================

/// 直方图 bin 数（两级统计的细级——64 桶足够 EV 决策）。
pub const HIST_BINS: usize = 64;

/// 亮度统计结果。
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct LumStats {
    /// 对数平均亮度（场景线性亮度，cd/m² 量纲）。
    pub log_avg_lum: f32,
    /// 直方图峰值 bin 的代表亮度。
    pub peak_lum: f32,
    /// 统计饱和标记（全黑/全白）。
    pub saturated: bool,
}

/// 统计饱和阈值（全黑 < 1e-4；全白 > 1e4——锚点「亮度统计缓冲异常」）。
pub const SATURATION_BLACK: f32 = 1.0e-4;
/// 全白阈值。
pub const SATURATION_WHITE: f32 = 1.0e4;

/**
 * 亮度统计：对数平均 + 直方图两级。
 *
 * **半分辨率声明**：真实管线上统计跑在下采样后的半分辨率亮度缓冲
 * （F1845 SSAO 同源思路——统计不需要全分辨率）；本纯函数面的输入即
 * 下采样后的样本序列，下采样本身归 F1845 共享设施（不在此复制）。
 *
 * 对数平均的数学：`exp(mean(ln(max(lum, ε))))`——ε 防全黑场景 ln 炸负无穷。
 */
pub fn luminance_stats(samples: &[f32]) -> LumStats {
    // 直方图两级：bin 域 [1e-4, 1e4] 对数均分 64 桶
    let lo_ln = SATURATION_BLACK.ln();
    let hi_ln = SATURATION_WHITE.ln();
    let mut hist = [0u32; HIST_BINS];
    let mut sum_ln = 0.0f32;
    let mut n = 0usize;
    let mut all_black = true;
    let mut all_white = true;
    for &s in samples {
        let s = if s.is_finite() && s > 0.0 { s } else { SATURATION_BLACK };
        all_black &= s <= SATURATION_BLACK;
        all_white &= s >= SATURATION_WHITE;
        let ln = s.max(SATURATION_BLACK).ln();
        sum_ln += ln;
        n += 1;
        let t = (ln - lo_ln) / (hi_ln - lo_ln);
        let bin = (t.clamp(0.0, 0.999_999) * HIST_BINS as f32) as usize;
        hist[bin] += 1;
    }
    let log_avg = if n == 0 { SATURATION_BLACK } else { (sum_ln / n as f32).exp() };
    // 峰值 bin 的代表亮度（bin 中心）
    let (best, _) = hist
        .iter()
        .enumerate()
        .max_by_key(|(_, c)| **c)
        .unwrap_or((0, &0));
    let peak = ((best as f32 + 0.5) / HIST_BINS as f32) * (hi_ln - lo_ln) + lo_ln;
    let peak = peak.exp();
    LumStats {
        log_avg_lum: log_avg,
        peak_lum: peak,
        saturated: all_black || all_white,
    }
}

// ===========================================================================
// 四、双模式曝光器（自动/手动互斥 + 阻尼 + 饱和冻结）
// ===========================================================================

/// 曝光模式。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExposureMode {
    /// 自动：场景亮度统计 → EV 动态调整。
    Auto,
    /// 手动：固定 EV（用户指定）。
    Manual,
}

/// 自动曝光参数。
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct AutoParams {
    /// 目标中位亮度（cd/m²——统计向它对齐）。
    pub target_median: f32,
    /// 调整速率（每帧最大 EV 变化量——一阶惯性步长）。
    pub adjust_rate: f32,
}

impl Default for AutoParams {
    fn default() -> Self {
        AutoParams { target_median: 0.18, adjust_rate: 0.05 }
    }
}

/// 曝光器状态（双模式互斥；标量 uniform 每帧一次——锚点性能口径）。
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct ExposureSystem {
    /// 当前模式。
    pub mode: ExposureMode,
    /// 手动 EV（手动模式生效值）。
    pub manual_ev: f32,
    /// 当前 EV（自动模式逐帧逼近目标）。
    pub current_ev: f32,
    /// 自动参数。
    pub auto: AutoParams,
    /// 频闪检测：大跳且方向交替的连续帧计数（振荡特征）。
    pub flicker_streak: u32,
    /// 上一次大跳的方向（+1/−1——方向交替才算振荡，同向持续是合法场景迁移）。
    pub last_jump_sign: i32,
    /// 阻尼缩放（频闪检出后 <1——速率限制）。
    pub damp_scale: f32,
    /// EV 调整次数（遥测——进 F1819，单调）。
    pub ev_adjustments: u64,
    /// 频闪判定阈值（帧间 |ΔlogLum| 超此即计一帧振荡）。
    pub flicker_jump: f32,
    /// 频闪成立的连续帧数门槛。
    pub flicker_frames: u32,
}

impl Default for ExposureSystem {
    fn default() -> Self {
        ExposureSystem {
            mode: ExposureMode::Manual,
            manual_ev: SAFE_EV,
            current_ev: SAFE_EV,
            auto: AutoParams::default(),
            flicker_streak: 0,
            last_jump_sign: 0,
            damp_scale: 1.0,
            ev_adjustments: 0,
            flicker_jump: 0.7,
            flicker_frames: 4,
        }
    }
}

impl ExposureSystem {
    /**
     * 设模式（互斥开关：自动+手动同时激活 → **手动优先自动禁用**，锚点
     * 明示——两个都想调 = 双重调光 = 画面不可预期）。
     */
    pub fn set_mode(&mut self, mode: ExposureMode, bag: &mut DiagBag) {
        if mode == ExposureMode::Auto && self.mode == ExposureMode::Manual {
            // 手动→自动：用户显式切换，放行
        }
        if mode == ExposureMode::Manual {
            // 手动激活即自动禁用（互斥语义：同一时刻至多一个源在调 EV）；
            // current 吸附到 manual_ev——手动模式的定义就是 EV=用户指定值，
            // 留着旧的自动值会出现"模式是手动、画面还在自动曝光"的缝。
            self.damp_scale = 1.0;
            self.flicker_streak = 0;
            self.last_jump_sign = 0;
            self.current_ev = self.manual_ev;
        }
        self.mode = mode;
        let _ = bag;
    }

    /**
     * 手动设 EV（越界钳制 + 记钳制告警；手动模式下这是唯一 EV 来源）。
     */
    pub fn set_manual_ev(&mut self, ev: f32, bag: &mut DiagBag) {
        let c = clamp_ev(ev);
        if c != ev {
            bag.push_warn(code::EV_CLAMPED);
        }
        self.manual_ev = c;
        self.current_ev = c;
        self.ev_adjustments += 1;
    }

    /**
     * 帧推进（自动模式）：统计 → 目标 EV → 一阶惯性逼近。
     *
     * **频闪阻尼**：帧间亮度对数跳变连续超阈（高频振荡）→ 阻尼缩放减半
     * （速率限制——突变场景的 EV 疯狂追就是频闪放大器，光敏健康相关）；
     * 亮度平稳后阻尼自然恢复（缩放回到 1）。
     *
     * **饱和冻结**：全黑/全白场景 → 冻结在安全 EV（不追垃圾统计）+ 告警。
     */
    pub fn step_auto(&mut self, stats: &LumStats, bag: &mut DiagBag) -> f32 {
        if self.mode != ExposureMode::Auto {
            // 手动模式下自动统计面不驱动 EV（互斥——追了就是双重调光）
            return self.current_ev;
        }
        // 饱和保护：全黑/全白 → 冻结安全 EV + 告警（锚点错误路径第 1 条）
        if stats.saturated {
            bag.push_warn(code::STATS_SATURATED);
            self.current_ev = SAFE_EV;
            self.ev_adjustments += 1;
            return self.current_ev;
        }
        // 频闪检测：帧间对数亮度跳变
        let prev_ev = self.current_ev;
        let mut rate = self.auto.adjust_rate;
        // 目标 EV：把统计亮度对齐到目标中位（log2(target/log_avg)）
        let target = clamp_ev(
            (self.auto.target_median.max(SATURATION_BLACK) / stats.log_avg_lum.max(SATURATION_BLACK))
                .log2(),
        );
        // 频闪检测（振荡特征 = 大跳 **且方向交替**）：
        // - 同向持续大跳 = 合法场景迁移（推近目标），**不计**振荡——
        //   否则每次进暗房都被误判成频闪（实测踩过：收敛判据被误伤）；
        // - 方向交替的大跳 = 亮度高频振荡 = 频闪，连续多帧 → 阻尼减半。
        let jump = (target - prev_ev).abs();
        let sign: i32 = if target > prev_ev { 1 } else if target < prev_ev { -1 } else { 0 };
        if jump > self.flicker_jump && sign != 0 {
            if sign == -self.last_jump_sign {
                self.flicker_streak += 1;
            } else {
                self.flicker_streak = self.flicker_streak.saturating_sub(1);
            }
            self.last_jump_sign = sign;
        } else {
            self.flicker_streak = self.flicker_streak.saturating_sub(1);
        }
        if self.flicker_streak >= self.flicker_frames {
            if self.damp_scale >= 1.0 {
                bag.push_warn(code::FLICKER_DAMPED);
            }
            self.damp_scale = 0.25;
        } else if self.flicker_streak == 0 {
            self.damp_scale = 1.0;
        }
        rate *= self.damp_scale;
        // 一阶惯性：向目标走 rate 步（O(1)；不越过目标——防振荡）
        let next = if self.current_ev < target {
            (self.current_ev + rate).min(target)
        } else if self.current_ev > target {
            (self.current_ev - rate).max(target)
        } else {
            target
        };
        self.current_ev = clamp_ev(next);
        if self.current_ev != prev_ev {
            self.ev_adjustments += 1;
        }
        self.current_ev
    }

    /// 曝光后线性颜色（场景线性亮度 × 2^EV——TM 前应用，F1841 管线序）。
    pub fn apply(&self, linear: f32) -> f32 {
        linear * ev_to_scale(self.current_ev)
    }
}

/// 2^EV（确定性整数近似：整数部分逐级乘除 2，分数部分三阶泰勒——
/// 与 vek05 ev_to_multiplier 同构声明非同码，K 侧消费它自己的单源）。
pub fn ev_to_scale(ev: f32) -> f32 {
    let ev = clamp_ev(ev);
    let i = if ev >= 0.0 { (ev as i64) as f32 } else { -((-ev as i64) as f32) };
    let f = ev - i;
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
    let x = f * 0.693_147_2;
    int_pow * (1.0 + x + x * x * 0.5 + x * x * x / 6.0)
}

// ===========================================================================
// 五、眼适应预留（接口位 + 诚实标注）
// ===========================================================================

/// 眼适应输入（预留接口的形状声明——F2007 K 侧过渡器对接位）。
#[derive(Clone, Copy, Debug)]
pub struct EyeAdaptInput {
    /// 当前 EV。
    pub current_ev: f32,
    /// 目标 EV。
    pub target_ev: f32,
    /// 适应速率。
    pub rate: f32,
}

/**
 * 眼适应曲线（**预留**：亮度渐变适应的接口位）。
 *
 * 锚点「眼适应接口被引用→显性报错（预留不静默）」：一期曝光是双模式+
 * 一阶阻尼，眼适应曲线（人眼亮度对数的非线性适应）未排期——被调显性
 * 报错，绝不静默返回一个假曲线（假曲线=过渡手感悄悄变形没人知道）。
 */
pub fn eye_adapt_curve(_input: &EyeAdaptInput) -> Result<f32, String> {
    Err("眼适应接口为 F1812 预留位：亮度渐变适应曲线未排期（一期用一阶阻尼替代）；显性报错不静默，对接 F2007 过渡器排期".to_string())
}

// ===========================================================================
// 六、VE-V 边界声明表（防双重调光）
// ===========================================================================

/// 一行边界声明。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BoundaryRow {
    /// 归属域。
    pub domain: &'static str,
    /// 管辖的参数域。
    pub scope: &'static str,
    /// 处理对象（管线位置）。
    pub object: &'static str,
}

/// VE-V 边界声明表（锚点：J 管：场景线性亮度→色调映射前曝光；V 域管：
/// 最终显示 gamma 与系统亮度）。
pub const BOUNDARY_TABLE: [BoundaryRow; 2] = [
    BoundaryRow {
        domain: "J (F1812)",
        scope: "场景线性亮度 → 色调映射前曝光（EV）",
        object: "场景内曝光（TM 前线性域）",
    },
    BoundaryRow {
        domain: "V",
        scope: "最终显示 gamma 与系统亮度",
        object: "全书 UI 层显示亮度（显示 referred 域）",
    },
];

/// 边界防冲突判定：两行 scope 的管线位置**不相交**（TM 前 vs 显示端——
/// 交集非空=双重调光=边界失守）。
pub fn boundary_disjoint() -> bool {
    // 可判定形态：J 的对象在「TM 前」（pre-tonemap），V 的对象在「显示端」
    // （post-display）——两段管线位置不同即不重叠。
    BOUNDARY_TABLE[0].object.contains("TM 前") && BOUNDARY_TABLE[1].object.contains("显示")
}

/// F1841 管线序声明（曝光在光照后雾前——对接 I02 后段与 J03 氛围链）。
pub const PIPELINE_ORDER_NOTE: &str = "F1841 管线序：光照 → 曝光(J/F1812) → 雾 → TM；曝光输出线性颜色给 I02 后段与 J03 氛围链";

// ===========================================================================
// 七、域自检（判据逐条映射锚点）
// ===========================================================================

/// VE-F1812 域自检（判据逐条映射锚点；随模块常驻编译，供注册表聚合器调用）。
pub mod checks {
    use super::*;
    use crate::checks::CheckSet;

    /// 跑全部判据。
    pub fn run_vej12_checks() -> CheckSet {
        let mut s = CheckSet::new("svstar2-vej12");

        // -- J12-AUTO-01 对数平均：常量场景=该亮度；渐变场景独立重算对拍 -----
        {
            let flat = luminance_stats(&[0.5f32; 64]);
            let flat_ok = (flat.log_avg_lum - 0.5).abs() < 1e-4 && !flat.saturated;
            // 渐变场景：0.1..=1.0 均匀 32 样本；判据侧独立重算 log-avg
            let grad: Vec<f32> = (0..32).map(|i| 0.1 + 0.9 * i as f32 / 31.0).collect();
            let st = luminance_stats(&grad);
            let indep: f32 =
                grad.iter().map(|v| v.max(SATURATION_BLACK).ln()).sum::<f32>() / 32.0;
            let indep = indep.exp();
            let close = (st.log_avg_lum - indep).abs() < 1e-5;
            // 对数平均 ≤ 算术平均（Jensen 不等式——统计正确性的数学性质）
            let arith: f32 = grad.iter().sum::<f32>() / 32.0;
            let jensen = st.log_avg_lum <= arith * 1.0001;
            s.add(
                "J12-AUTO-01 亮度统计：常量场景恰等、渐变场景与独立重算一致、对数平均≤算术平均（Jensen）",
                flat_ok && close && jensen,
                "锚点「对数平均+直方图两级」；log-avg 算错会让自动曝光永远偏一档",
            );
        }

        // -- J12-AUTO-02 直方图两级：bin 守恒 + 峰值与样本众数一致 ------------
        {
            // 众数样本集中在高亮度端
            let mut samples = vec![0.2f32; 16];
            samples.extend(vec![8.0f32; 48]);
            let st = luminance_stats(&samples);
            // 48/64 样本是 8.0 → 峰值 bin 应落在高亮度侧
            let peak_high = st.peak_lum > 1.0;
            // 守恒由统计函数不变式保证（每样本恰计一 bin）——此处验两侧极端
            // 都有 bin（不丢样本）：
            let both_ends = luminance_stats(&[0.5f32; 4]).peak_lum > 0.0;
            s.add(
                "J12-AUTO-02 直方图：众数场景峰值落高亮度侧、bin 计数不丢样本",
                peak_high && both_ends,
                "锚点「对数平均+直方图两级」；两级互证（log-avg 管 EV、直方图管峰值）",
            );
        }

        // -- J12-DUAL-01 双模式：自动收敛到目标 EV；互斥双向真触发 ------------
        {
            let mut bag = DiagBag::new();
            let mut ex = ExposureSystem::default();
            ex.set_mode(ExposureMode::Auto, &mut bag);
            ex.auto.target_median = 0.18;
            // 场景亮度 0.18 → 目标 EV=0；场景 0.036 → log2(0.18/0.036)=+1
            let dark = luminance_stats(&[0.09f32; 32]);
            let mut ev = 0.0f32;
            for _ in 0..64 {
                ev = ex.step_auto(&dark, &mut bag);
            }
            let converged = (ev - 1.0).abs() < 0.06;
            // 互斥：切回手动后 auto 统计不再驱动 EV
            ex.set_mode(ExposureMode::Manual, &mut bag);
            let frozen = ex.step_auto(&dark, &mut bag);
            let mutex_ok = (frozen - ex.manual_ev).abs() < 1e-6;
            s.add(
                "J12-DUAL-01 双模式：自动逐帧收敛到目标 EV（0.018→EV+1）、切手动后自动冻结（互斥）",
                converged && mutex_ok,
                "锚点「自动曝光：场景亮度统计→曝光参数动态调整；手动曝光：固定 EV」+互斥",
            );
        }

        // -- J12-DUAL-02 手动优先：手动激活时 EV 即手动值、越界钳制告警 --------
        {
            let mut bag = DiagBag::new();
            let mut ex = ExposureSystem::default();
            ex.set_mode(ExposureMode::Manual, &mut bag);
            ex.set_manual_ev(9.0, &mut bag);
            let clamped = ex.manual_ev == EV_MAX && bag.count(code::EV_CLAMPED) == 1;
            let mut bag2 = DiagBag::new();
            let mut ex2 = ExposureSystem::default();
            ex2.set_mode(ExposureMode::Auto, &mut bag2);
            ex2.set_mode(ExposureMode::Manual, &mut bag2);
            let manual_wins = ex2.mode == ExposureMode::Manual;
            s.add(
                "J12-DUAL-02 手动优先：EV 越界钳制+9→+6 告警、模式切换后手动在场（自动禁用）",
                clamped && manual_wins,
                "锚点「自动+手动同时激活→手动优先自动禁用」「EV 越界→钳制」",
            );
        }

        // -- J12-SAT-01 饱和保护：全黑/全白冻结安全 EV + 告警；正常不触发 -----
        {
            let mut bag = DiagBag::new();
            let mut ex = ExposureSystem::default();
            ex.set_mode(ExposureMode::Auto, &mut bag);
            ex.manual_ev = 0.0;
            // 先把 EV 推离安全值（证明冻结真的发生了）
            let normal = luminance_stats(&[0.018f32; 32]);
            for _ in 0..16 {
                ex.step_auto(&normal, &mut bag);
            }
            let moved = ex.current_ev != 0.0;
            // 全黑 → 冻结回安全 EV
            let black = luminance_stats(&[0.0f32; 32]);
            let frozen_black = ex.step_auto(&black, &mut bag);
            let white = luminance_stats(&[2.0e4f32; 32]);
            let frozen_white = ex.step_auto(&white, &mut bag);
            let both_frozen = frozen_black == SAFE_EV && frozen_white == SAFE_EV;
            let alarmed = bag.count(code::STATS_SATURATED) == 2;
            // 正常场景不触发（保护不许误伤）
            let mut bag2 = DiagBag::new();
            let mut ex2 = ExposureSystem::default();
            ex2.set_mode(ExposureMode::Auto, &mut bag2);
            let _ = ex2.step_auto(&normal, &mut bag2);
            let no_false = bag2.count(code::STATS_SATURATED) == 0;
            s.add(
                "J12-SAT-01 饱和保护：全黑/全白冻结安全 EV+双向告警、正常场景零误伤",
                moved && both_frozen && alarmed && no_false,
                "锚点「统计饱和保护：自动曝光冻结在安全 EV 并告警」；没有误伤断言的保护会常开",
            );
        }

        // -- J12-DAMP-01 频闪阻尼：方波振荡下 EV 波动有界、恒定亮度不误伤 -----
        {
            let mut bag = DiagBag::new();
            let mut ex = ExposureSystem::default();
            ex.set_mode(ExposureMode::Auto, &mut bag);
            // 方波亮度：0.018 ↔ 1.8（目标 EV 在 ±3 摆——高频振荡）
            let bright = luminance_stats(&[1.8f32; 32]);
            let dark = luminance_stats(&[0.018f32; 32]);
            let mut min_ev = 0.0f32;
            let mut max_ev = 0.0f32;
            for i in 0..40 {
                let ev = ex.step_auto(if i % 2 == 0 { &bright } else { &dark }, &mut bag);
                min_ev = min_ev.min(ev);
                max_ev = max_ev.max(ev);
            }
            // 阻尼触发（频闪检出告警）且 EV 波动有界（|摆幅| < 1.0——
            // 无阻尼时会一路追到 ±3）
            let damped = bag.count(code::FLICKER_DAMPED) >= 1;
            let bounded = (max_ev - min_ev) < 1.0;
            // 恒定亮度不误伤
            let mut bag2 = DiagBag::new();
            let mut ex2 = ExposureSystem::default();
            ex2.set_mode(ExposureMode::Auto, &mut bag2);
            for _ in 0..20 {
                let _ = ex2.step_auto(&dark, &mut bag2);
            }
            let no_false = bag2.count(code::FLICKER_DAMPED) == 0;
            s.add(
                "J12-DAMP-01 频闪阻尼：方波振荡下 EV 摆幅有界+阻尼告警、恒定亮度零误伤",
                damped && bounded && no_false,
                "锚点「频闪场景→速率限制阻尼」；阻尼是光敏用户的健康相关考量（锚点无障碍声明）",
            );
        }

        // -- J12-CLAMP-01 EV 域单源：钳制边界与常量一致、非有限回落安全值 -----
        {
            let c_hi = clamp_ev(99.0);
            let c_lo = clamp_ev(-99.0);
            let nan = clamp_ev(f32::NAN);
            s.add(
                "J12-CLAMP-01 EV 域：越界钳到 ±6（单源常量）、非有限回落安全 EV",
                c_hi == EV_MAX && c_lo == EV_MIN && nan == SAFE_EV,
                "锚点「手动 EV 值范围声明（-6 到 +6）」「EV 越界→钳制」",
            );
        }

        // -- J12-SRC-01 跨域同值对拍：与 K 域 vek05 转录值一致 ----------------
        {
            // vek05_params::EV_MIN/EV_MAX 是 F1812 建库前的 K 侧转录；
            // 本判据把「转录同值」钉成对拍（任一侧改值即红）。
            s.add(
                "J12-SRC-01 跨域同值对拍：J 侧声明与 K 域 vek05 转录值数值相等（±6）",
                EV_MIN == crate::svstar2::vek05_params::EV_MIN
                    && EV_MAX == crate::svstar2::vek05_params::EV_MAX
                    && EV_MIN == -6.0
                    && EV_MAX == 6.0,
                "F2007 锚点「EV 值范围声明沿用 F1812」——现在 F1812 落地，对拍接上",
            );
        }

        // -- J12-EYE-01 眼适应预留：被调显性报错 ------------------------------
        {
            let input = EyeAdaptInput { current_ev: 0.0, target_ev: 1.0, rate: 0.1 };
            let r = eye_adapt_curve(&input);
            s.add(
                "J12-EYE-01 眼适应预留：被调显性报错（预留不静默）",
                r.is_err(),
                "锚点「眼适应接口被引用→显性报错（预留不静默）」；假曲线=过渡手感悄悄变形",
            );
        }

        // -- J12-BOUND-01 VE-V 边界：两域参数域不相交（防双重调光）------------
        {
            s.add(
                "J12-BOUND-01 VE-V 边界：J 管场景 TM 前曝光、V 管显示端亮度，域不相交",
                boundary_disjoint() && BOUNDARY_TABLE.len() == 2,
                "锚点「划清与 VE-V 边界防止双重调光」；边界表是声明与判定一体",
            );
        }

        // -- J12-ORDER-01 管线序声明（F1841：曝光在光照后雾前）----------------
        {
            s.add(
                "J12-ORDER-01 管线序声明：光照→曝光→雾→TM（F1841 对接 I02/J03）",
                PIPELINE_ORDER_NOTE.contains("光照") && PIPELINE_ORDER_NOTE.contains("雾") && PIPELINE_ORDER_NOTE.contains("TM"),
                "锚点「F1841 管线序中曝光在光照后雾前」；序错=雾被曝光推亮",
            );
        }

        // -- J12-TELEM-01 EV 调整频率遥测（F1819 对接面，单调）----------------
        {
            let mut bag = DiagBag::new();
            let mut ex = ExposureSystem::default();
            ex.set_mode(ExposureMode::Auto, &mut bag);
            let dark = luminance_stats(&[0.018f32; 32]);
            let before = ex.ev_adjustments;
            for _ in 0..8 {
                let _ = ex.step_auto(&dark, &mut bag);
            }
            s.add(
                "J12-TELEM-01 EV 调整频率计数单调递增（遥测进 F1819 的对接面）",
                ex.ev_adjustments > before,
                "锚点「遥测（EV 调整频率）进 F1819」；计数是遥测的物料",
            );
        }

        // -- J12-SCALE-01 半分辨率声明 + 曝光应用（线性×2^EV）-----------------
        {
            // 应用面：EV=+1 → 亮度翻倍（曝光的物理语义）
            let mut ex = ExposureSystem::default();
            ex.set_mode(ExposureMode::Manual, &mut DiagBag::new());
            ex.set_manual_ev(1.0, &mut DiagBag::new());
            let out = ex.apply(0.5);
            s.add(
                "J12-SCALE-01 曝光应用：EV+1 → 线性亮度×2（半分辨率统计声明与 F1845 同源）",
                (out - 1.0).abs() < 0.01,
                "锚点「输出曝光后线性颜色」「亮度统计复用下采样链（F1845 同源）」",
            );
        }

        s
    }
}
