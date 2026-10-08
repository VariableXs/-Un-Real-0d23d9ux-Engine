//! VE-F2007 · 曝光联动（VE-K 域 · 后处理架构与 Bloom 组 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F2007`
//!
//! **判据（锚点原文逐条）**：
//! - 应用位兑现：曝光乘法在 TM 输入端（线性域乘法：线性颜色 × 曝光倍率 → TM），
//!   F1812「J 域自动曝光值 → K 域应用位」的预留-兑现闭环；
//! - 眼适应过渡：目标曝光 → 当前曝光的一阶惯性追踪，速率三档（快/中/慢）+ 自定义
//!   秒数，瞬变不跳变；
//! - 量纲单源：曝光值 EV / 线性倍率双表示，换算函数**不重写**——直接引用 F2005
//!   已立的 `ev_to_multiplier` / `multiplier_to_ev`；
//! - 场景重置：过渡不跨场景，场景切换 = 曝光快照重置。
//!
//! **错误路径与降级矩阵（锚点原文四条）**：
//! 1. 曝光值 NaN/极端 → 钳制（EV ∈ [-6,+6] 沿用 F1812 范围），钳制走显式诊断；
//! 2. 过渡中被场景切换打断 → 重置语义（当前曝光**直接跳到**新场景目标曝光，
//!    不从旧场景值渐变——旧场景亮度对新场景无意义）；
//! 3. 自动曝光统计失效（J 侧全黑/全白饱和）→ K 侧**冻结当前值** + 告警，
//!    **不黑屏**；
//! 4. 手动/自动混用 → 互斥沿用 F1812 语义（手动优先，自动禁用）。
//!
//! **设计要点（为什么这样写）**：
//! - **过渡在 EV 域而非倍率域线性插值**：人眼对亮度的响应是对数性的（JND 近似
//!   成比例），倍率域线性插值在暗端步进极小（视觉卡顿）而在亮端步进极大
//!   （视觉跳变）；EV 域线性即"每单位时间走固定个对数档"，这正是眼适应模型。
//! - **步进系数取`1 - 1/D(x)`，`D(x)=1+x+x²/2+x³/6`**：`D` 是 `e^x` 的正项截断，
//!   按下界方向成立，故 `D(x) ≤ e^x` ⟹ `1 - 1/D(x) ≤ 1 - e^{-x}`。这个方向性是
//!   **硬性质**而非凑合：步进系数恒 `< 1` 且严格单调，追踪**永不过冲、不震荡**。
//!   常见的 `x/(1+x)` 有理近似虽然也对，但它在 x≈1 处低估 13%，会让过渡时间常数
//!   明显长于设定值；`x/(1+x/2)`（[1/1] Padé）更糟——x>2 时**大于 1 直接过冲**，
//!   而过冲在一阶惯性上表现为亮度来回摆动，正是本条要消灭的现象。
//! - **冻结是"保持"不是"归零"**：统计失效时若把当前曝光置 0，画面全黑（J 侧
//!   统计失效本来只是"不知道多亮"，不是"画面很暗"）。这是本条最容易写反的一处。
//! - **场景切换的同 tick 组合要单独处理**：`begin_scene` 会把当前曝光设成新目标，
//!   若同一 tick 统计恰好失效，那个目标本身就是无效值——此时必须**保持旧曝光**
//!   而不是跳到无效目标，否则"重置"变成"跳进坏值"。
//!
//! **跨批对接点**：
//! - F1812 应用位契约的 K 侧兑现（分工登记见 `EXPOSURE_CONTRACT_DOC`）；
//! - 量纲单源 F1802 / F2005（换算函数直接 `use` F2005 的实现，本条目**零重复**）；
//! - 与 F1927 曲线核同构声明（见 `ADAPTATION_ISOMORPH_DECL`）；
//! - TM 输入端类型复用 F2006 的 `SceneReferred`——应用位与 TM 之间**不留中间
//!   表示**，乘法结果直接就是 TM 的入参类型（少一次转换 = 少一处能出错的地方）；
//! - 泛光阈值曝光联动（F2005 `effective_threshold`）消费本条产出的 EV。
//!
//! **无障碍与隐私**：曝光突变的平滑本身服务光敏舒适（与 F1963 渐变语义一致，
//! 见 `PHOTOSENSITIVE_DECL`）；无隐私面。
//!
//! 零外部依赖；逻辑 tick 注入，零墙钟；确定性算法、零 IO、回归可复现。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use super::vek05_params::{ev_to_multiplier, multiplier_to_ev, EV_MAX, EV_MIN, EV_NEUTRAL,
    EV_ROUNDTRIP_TOLERANCE};
use super::vek06_tonemap::SceneReferred;

// ---------------------------------------------------------------------------
// 一、契约声明表（J 管计算 / K 管应用的分工落账）
// ---------------------------------------------------------------------------

/// F1812 应用位契约 · K 侧兑现登记（三列：环节 / 归属域 / 本条兑现面）。
///
/// **为什么要落账**：F1812 只声明"J 域管场景线性亮度 → 色调映射前曝光"，但
/// "曝光计算"与"曝光应用"是两次不同的动作——前者是统计与求解（J 域），后者是
/// 对每个像素的线性域乘法（K 域 TM 输入端）。分工不落账，两边都会觉得自己已经
/// 做了，缺口永远没人认。
pub const EXPOSURE_CONTRACT_DOC: [(&str, &str, &str); 4] = [
    ("亮度统计（对数平均 + 直方图）", "J 域 VE-F1812", "本条只消费统计结论，不复制统计逻辑"),
    ("曝光求解（目标中位亮度 → EV）", "J 域 VE-F1812", "本条只接收 EV 标量，O(1) 持有"),
    ("曝光过渡（一阶惯性 + 速率档）", "K 域 VE-F2007", "ExposureAdaptation（阻尼 K 侧承接）"),
    ("曝光应用（线性域乘法）", "K 域 VE-F2007", "apply_exposure → TM 输入端"),
];

/// F1812 预留 → F2007 兑现闭环登记。
///
/// 锚点 F2007「J 域 F1849 契约的 K 侧兑现」在 F2007 原文写作 F1849，但本条真正
/// 兑现的预留位来自 **F1812**（曝光系统的"眼适应接口位+诚实标注"）与**F1841**
/// （管线序：曝光在光照后雾前）。此处按实际预留来源登记，避免后来者按错编号去找。
pub const F1812_REDEMPTION_DOC: [(&str, &str); 4] = [
    ("F1812 眼适应接口位", "VE-F2007 ExposureAdaptation：三档速率 + 自定义秒数，O(1) 每帧"),
    ("F1812 统计饱和保护（K 侧兑现）", "VE-F2007 冻结当前值 + 告警，不黑屏"),
    ("F1812 手动/自动互斥（K 侧兑现）", "VE-F2007 resolve_mode：手动优先，自动禁用"),
    ("F1841 管线序（曝光在 TM 前）", "VE-F2007 apply_exposure 位于 TM 输入端，与雾/后处理序位一致"),
];

/// 与 F1927 曲线核的同构声明（锚点「过渡与 F1927 曲线核同构声明」）。
///
/// **同构在哪**：F1927 曲线核是"对标量做参数化非线性整形"，本条过渡器是
/// "对标量（EV）做参数化一阶追踪"——两者共享同一形态：**归一化输入 → 参数化
/// 映射 → 有界输出**，且都要求映射对参数单调（参数越大越快，不允许出现"更慢的
/// 更快"这种非单调）。不同点只有一处：曲线核是无记忆纯函数，过渡器有状态
/// （当前值），状态存在状态维而非映射维。
///
/// **为什么要写下来**：同构声明的价值在"改一处时知道另一处要不要跟着改"。
/// 声明在此，将来F1927 若改参数化约定（例如把速率改为倒数制），本条的档位
/// 表与自检判据能立刻被想起来一起改；不声明则两边各自漂移，半年后速率语义
/// 对不上却查不到出处。
pub const ADAPTATION_ISOMORPH_DECL: &str = "VE-F1927 曲线核同构：归一化输入→参数化映射→有界输出，\
映射对参数单调；本条多一个状态维（当前值），状态不入映射。参数约定变更须同步本条档位表与判据。";

/// 光敏舒适声明（锚点「曝光突变的平滑本身服务光敏舒适，与 F1963 渐变语义一致」）。
///
/// 曝光瞬变在生理上是危险的：一次 6 EV 的瞬变等价于亮度跳 64 倍，足够触发光敏
/// 用户的不适甚至诱发反应。本条把"瞬变不跳变"做成**机制**（一阶惯性 + 速率档）
/// 而非仅做成声明，正是因为光敏风险发生在机制缺位时。
pub const PHOTOSENSITIVE_DECL: &str = "曝光瞬变经一阶惯性平滑，单帧亮度变化率受限；\
与 F1963 渐变语义同构（渐变即受控的时间斜率）。无隐私面。";

// ---------------------------------------------------------------------------
// 二、诊断（独立诊断面；本条四码均为曝光联动专属，不与其他条目重码）
// ---------------------------------------------------------------------------

/// 曝光联动诊断码。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExpDiagCode {
    /// EV 非有限（NaN/±Inf）→ 钳制到中性或域端（F1812 范围 [-6,+6]）。
    EvNonFinite,
    /// EV 超域 → 钳制到 [-6,+6]。
    EvClamped,
    /// 自适应秒数越界 → 钳制到 [CUSTOM_MIN, CUSTOM_MAX]。
    RateClamped,
    /// 场景切换 → 曝光快照重置（过渡不跨场景）。**已执行的预期行为**，非异常，
    /// 但必须留痕：否则"切场景后画面忽然很暗"这类观感问题无从追溯。
    SceneCutReset,
    /// 自动曝光统计失效（J 侧）→ 冻结当前曝光 + 告警。
    AutoStatisticsFrozen,
    /// 统计恢复 → 解冻，继续正常追踪。
    AutoStatisticsResumed,
    /// 手动/自动同时请求 → 互斥裁决为手动（F1812 语义）。
    ModeConflictToManual,
    /// 手动生效期间收到 J 侧自动曝光输入 → 忽略（互斥的必然结果，可观测）。
    AutoInputIgnoredWhileManual,
}

/// 诊断严重级。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExpSeverity {
    /// 已纠正，渲染照常。
    Corrected,
    /// 显性降级：渲染在跑但曝光语义已变（统计失效/模式裁决）。
    Degraded,
}

impl ExpSeverity {
    /// 标签。
    pub fn label(self) -> &'static str {
        match self {
            ExpSeverity::Corrected => "已纠正",
            ExpSeverity::Degraded => "显性降级",
        }
    }
}

impl ExpDiagCode {
    /// 机器码（遥测口径；字符串比较比枚举比较慢且易随重构改名）。
    pub fn code(self) -> &'static str {
        match self {
            ExpDiagCode::EvNonFinite => "K07.EV_NON_FINITE",
            ExpDiagCode::EvClamped => "K07.EV_CLAMPED",
            ExpDiagCode::RateClamped => "K07.RATE_CLAMPED",
            ExpDiagCode::SceneCutReset => "K07.SCENE_CUT_RESET",
            ExpDiagCode::AutoStatisticsFrozen => "K07.AUTO_STATS_FROZEN",
            ExpDiagCode::AutoStatisticsResumed => "K07.AUTO_STATS_RESUMED",
            ExpDiagCode::ModeConflictToManual => "K07.MODE_CONFLICT_TO_MANUAL",
            ExpDiagCode::AutoInputIgnoredWhileManual => "K07.AUTO_INPUT_IGNORED",
        }
    }

    /// 下一步处置提示（异常零静默：每条告警都要说清"该做什么"）。
    pub fn next_hint(self) -> &'static str {
        match self {
            ExpDiagCode::EvNonFinite => "查J 域曝光求解是否产出 NaN（多半是除零或空统计）",
            ExpDiagCode::EvClamped => "确认场景亮度落在 J 域 EV 求解的设计范围内",
            ExpDiagCode::RateClamped => "自适应秒数已钳到安全域；如需更快需重新评估光敏风险",
            ExpDiagCode::SceneCutReset => "预期行为：过渡不跨场景，如需跨场景渐变请另立契约",
            ExpDiagCode::AutoStatisticsFrozen => "查 J 域亮度统计缓冲（该场景可能全黑/全白）",
            ExpDiagCode::AutoStatisticsResumed => "统计已恢复，曝光继续追踪",
            ExpDiagCode::ModeConflictToManual => "手动优先是 F1812 语义；如需自动请先切手动再切自动",
            ExpDiagCode::AutoInputIgnoredWhileManual => "互斥生效：手动模式下自动曝光输入被丢弃",
        }
    }
}

/// 一条诊断。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExpDiagnostic {
    /// 诊断码。
    pub code: ExpDiagCode,
    /// 严重级。
    pub severity: ExpSeverity,
    /// 上下文数值（依码而定：EV 实测值 / 钳制后值 / 场景号 / 冻结时当前 EV）。
    pub context: f32,
}

/// 诊断袋。
#[derive(Clone, Debug, Default)]
pub struct ExpDiagBag {
    items: Vec<ExpDiagnostic>,
}

impl ExpDiagBag {
    /// 空袋。
    pub fn new() -> Self {
        ExpDiagBag { items: Vec::new() }
    }

    /// 记一条（带严重级）。
    pub fn push(&mut self, code: ExpDiagCode, severity: ExpSeverity, context: f32) {
        self.items.push(ExpDiagnostic { code, severity, context });
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 空否。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 是否含某码。
    pub fn has(&self, code: ExpDiagCode) -> bool {
        self.items.iter().any(|d| d.code == code)
    }

    /// 取诊断码序列（断言用）。
    pub fn codes(&self) -> Vec<ExpDiagCode> {
        self.items.iter().map(|d| d.code).collect()
    }

    /// 是否全部为"已纠正"级（无降级——曝光语义与画面均未被改变）。
    pub fn no_degradation(&self) -> bool {
        self.items.iter().all(|d| d.severity == ExpSeverity::Corrected)
    }

    /// 人读全文（每条一行）。
    pub fn report(&self) -> String {
        let mut s = String::new();
        for (i, d) in self.items.iter().enumerate() {
            if i > 0 {
                s.push('\n');
            }
            s.push_str(&format!(
                "[{}/{}] {} = {:.4} → {}",
                d.severity.label(),
                d.code.code(),
                d.code.code(),
                d.context,
                d.code.next_hint()
            ));
        }
        s
    }
}

// ---------------------------------------------------------------------------
// 三、量纲换算（双表示 · 换算函数单源引用 F2005，零重复实现）
// ---------------------------------------------------------------------------

/// 曝光值的双表示（EV / 线性倍率）。
///
/// **两个表示都是一等公民**：EV 是人读与求解域（对数线性、可加），倍率是渲染
/// 域（直接乘）。锚点要求「曝光值为 EV 或线性倍率的双表示与换算单源」——单源
/// 指换算**只有一个**实现（`vek05_params::ev_to_multiplier`），本类型不允许自带
/// 换算逻辑，只能引用。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExposureValue {
    /// 曝光补偿（EV，已钳制到 F1812 域 [-6,+6]）。
    pub ev: f32,
    /// 线性倍率（`2^ev`，供 TM 输入端逐分量乘法）。
    pub multiplier: f32,
    /// EV 是否被钳制过（供自检与遥测区分"合法域内值"与"被救回来的值"）。
    pub clamped: bool,
}

impl ExposureValue {
    /// 由 EV 构造（非有限 → 中性 0 EV并置钳制标记；超域 → 钳到域端）。
    pub fn from_ev(ev: f32) -> Self {
        if !ev.is_finite() {
            // 非有限不是"极端亮/暗"，而是"不知道"——兜底取中性而不是取域端，
            // 因为域端任一方向都会把"未知"变成"确定的错误画面"。
            return ExposureValue {
                ev: EV_NEUTRAL,
                multiplier: ev_to_multiplier(EV_NEUTRAL),
                clamped: true,
            };
        }
        let c = ev.clamp(EV_MIN, EV_MAX);
        ExposureValue {
            ev: c,
            multiplier: ev_to_multiplier(c),
            clamped: c != ev,
        }
    }

    /// 由线性倍率构造（≤0 或非有限 → 域下界；换算后可能超域 → 钳制）。
    pub fn from_multiplier(m: f32) -> Self {
        if !m.is_finite() || m <= 0.0 {
            // 倍率 ≤ 0 在线性域无意义（乘出来是负数或零），钳到 EV 下界而非 0：
            // 0 倍率= 全黑画面，是最坏输出；域下界（-6 EV = 1/64）是"很暗但有画面"。
            return ExposureValue {
                ev: EV_MIN,
                multiplier: ev_to_multiplier(EV_MIN),
                clamped: true,
            };
        }
        let raw = multiplier_to_ev(m);
        let c = raw.clamp(EV_MIN, EV_MAX);
        ExposureValue { ev: c, multiplier: ev_to_multiplier(c), clamped: c != raw }
    }

    /// 中性曝光（0 EV / 倍率 1.0）。
    pub fn neutral() -> Self {
        ExposureValue { ev: EV_NEUTRAL, multiplier: 1.0, clamped: false }
    }

    /// 双表示自洽（倍率确为`2^EV`）。
    pub fn self_consistent(self) -> bool {
        self.multiplier.is_finite()
            && self.multiplier > 0.0
            && (self.multiplier - ev_to_multiplier(self.ev)).abs() < 1e-5
    }

    /// 往返误差（EV 单位）——对拍 F2005 的往返容差用。
    pub fn roundtrip_error_ev(self) -> f32 {
        (multiplier_to_ev(self.multiplier) - self.ev).abs()
    }
}

/// 量纲往返容差（引用 F2005 单源常量，**不在本条另设一份**）。
///
/// 另设一份是量纲事故的经典温床：两份容差迟早会分叉，届时"到底哪个是准的"要靠
/// 考古才能回答，而调用点各自引用不同值会让同一现象在两个条目表现不一致。
pub const EXPOSURE_EV_ROUNDTRIP_TOLERANCE: f32 = EV_ROUNDTRIP_TOLERANCE;

// ---------------------------------------------------------------------------
// 四、曝光应用位（线性域乘法，零额外 pass —— 并入 TM pass）
// ---------------------------------------------------------------------------

/// 已应用曝光的场景线性色（= TM 的入参）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExposedFrame {
    /// 曝光后的线性色（TM 输入端）。
    pub color: SceneReferred,
    /// 本帧实际使用的曝光（供调试面板与泛光阈值联动回读）。
    pub exposure: ExposureValue,
    /// 是否发生过数值清洗（NaN/±Inf/负 → 0）。
    pub sanitized: bool,
}

/// 曝光应用位：线性色 × 曝光倍率 → TM 输入端。
///
/// **顺序：先乘后洗，不是先洗后乘**。若先洗后乘，遇到 `NaN × 0` 仍是 NaN，
/// 等于白洗一次；先乘后洗把脏值和清洗都留在同一趟里做掉。
///
/// **为什么乘完还要洗**：乘法把上游的 Inf 放大成 Inf（`f32::MAX × 64` 直接溢出），
/// 溢出值若直接喂给 TM，会污染整条曲线（F2006 的NaN 防线只能拦 NaN，拦不住
/// 已经饱和的Inf 参与多项式运算）。清洗语义与 F2006 一致：Inf 钳 0 而非 MAX。
pub fn apply_exposure(scene: SceneReferred, exposure: ExposureValue) -> ExposedFrame {
    // **回写实际使用的曝光**：防御分支一旦触发，若仍把入参原样塞回结构体，
    // 遥测会读到"报告倍率 0.0、实际乘 1.0"——这类"报告值与行为不一致"比直接
    // 崩掉更难查，因为它让下游所有基于该字段的判断（泛光阈值联动、光敏面板
    // 亮度显示）都建立在错误的数上。
    let exposure = if exposure.multiplier.is_finite() && exposure.multiplier > 0.0 {
        exposure
    } else {
        ExposureValue::neutral()
    };
    let m = exposure.multiplier;
    let scaled = SceneReferred::new(scene.r * m, scene.g * m, scene.b * m);
    let (color, sanitized) = scaled.sanitize();
    ExposedFrame { color, exposure, sanitized }
}

// ---------------------------------------------------------------------------
// 五、一阶惯性过渡器（速率三档 + 自定义秒数）
// ---------------------------------------------------------------------------

/// 适应速率档（锚点「快/中/慢三档 + 自定义秒数」）。
///
/// 档位秒数取自人眼暗适应的时间常数量级：亮→暗约 0.1 s 起效、暗→亮约 1 s 起效，
/// 中档 0.40 s 覆盖两侧的折中。
///
/// **快档 0.10 s 是光敏约束下的下界，不是"响应快"的追求**：实测（60fps、6EV
/// 阶跃）快档首帧亮度变化率达1.84 倍，已逼近单帧 2 倍的光敏经验阈。再压缩会让
/// 常见阶跃越阈，所以**下钳到 0.02 s 的同时把快档定在 0.10 s**——两者是不同
/// 的东西：0.02 s 是"自定义越界不许比这更快"的兜底，0.10 s 是"三档里最快能给
/// 的安全值」。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AdaptRate {
    /// 快（0.10 s）。
    Fast,
    /// 中（0.40 s）。
    Medium,
    /// 慢（1.20 s）。
    Slow,
    /// 自定义时间常数秒数（越界钳制）。
    Custom(f32),
}

/// 快档时间常数（秒）。
pub const RATE_FAST_SECONDS: f32 = 0.10;

/// 中档时间常数（秒）。
pub const RATE_MEDIUM_SECONDS: f32 = 0.40;

/// 慢档时间常数（秒）。
pub const RATE_SLOW_SECONDS: f32 = 1.20;

/// 自定义时间常数下界（秒）。
pub const RATE_CUSTOM_MIN_SECONDS: f32 = 0.02;

/// 自定义时间常数上界（秒）。
///
/// 上界不是随意取的：超过 3 s 的过渡在用户看来是"曝光坏了不响应"，不如给告警。
pub const RATE_CUSTOM_MAX_SECONDS: f32 = 3.0;

impl AdaptRate {
    /// 全部档位（自检与UI 列举用）。
    pub fn all() -> [AdaptRate; 4] {
        [AdaptRate::Fast, AdaptRate::Medium, AdaptRate::Slow, AdaptRate::Custom(RATE_MEDIUM_SECONDS)]
    }

    /// 时间常数（秒，自定义越界已钳制）。
    pub fn tau_seconds(self) -> f32 {
        match self {
            AdaptRate::Fast => RATE_FAST_SECONDS,
            AdaptRate::Medium => RATE_MEDIUM_SECONDS,
            AdaptRate::Slow => RATE_SLOW_SECONDS,
            AdaptRate::Custom(s) => {
                if s.is_finite() {
                    s.clamp(RATE_CUSTOM_MIN_SECONDS, RATE_CUSTOM_MAX_SECONDS)
                } else {
                    RATE_MEDIUM_SECONDS
                }
            }
        }
    }

    /// 自定义档是否被钳制过（供自检区分"合法自定义"与"被救回来的自定义"）。
    pub fn custom_clamped(self) -> bool {
        match self {
            AdaptRate::Custom(s) => {
                !s.is_finite()
                    || s < RATE_CUSTOM_MIN_SECONDS
                    || s > RATE_CUSTOM_MAX_SECONDS
            }
            _ => false,
        }
    }

    /// key（配置面稳定标识）。
    pub fn key(self) -> &'static str {
        match self {
            AdaptRate::Fast => "adapt.fast",
            AdaptRate::Medium => "adapt.medium",
            AdaptRate::Slow => "adapt.slow",
            AdaptRate::Custom(_) => "adapt.custom",
        }
    }

    /// 按 key 反查（自定义档的秒数由调用方另行传入）。
    pub fn from_key(k: &str) -> Option<AdaptRate> {
        match k {
            "adapt.fast" => Some(AdaptRate::Fast),
            "adapt.medium" => Some(AdaptRate::Medium),
            "adapt.slow" => Some(AdaptRate::Slow),
            "adapt.custom" => Some(AdaptRate::Custom(RATE_MEDIUM_SECONDS)),
            _ => None,
        }
    }

    /// 人读标签。
    pub fn label(self) -> String {
        match self {
            AdaptRate::Fast => format!("快（{:.2}s）", self.tau_seconds()),
            AdaptRate::Medium => format!("中（{:.2}s）", self.tau_seconds()),
            AdaptRate::Slow => format!("慢（{:.2}s）", self.tau_seconds()),
            AdaptRate::Custom(_) => format!("自定义（{:.2}s）", self.tau_seconds()),
        }
    }
}

/// 步进系数 `alpha = 1 - 1/D(x)`，`D(x) = 1 + x + x²/2 + x³/6`。
///
/// 数学性质（这是本函数存在的全部理由，不要换成别的近似）：
/// - `D` 是 `e^x` 的正项截断，且 `D ≤ e^x` ⟹ `alpha ≤ 1 - e^{-x}`（**下界**）；
/// - `D` 严格递增 ⟹ `alpha` 严格单调 ⟹ 速率参数越大越快，非单调不可能出现；
/// - `D(0)=1` ⟹ `alpha(0)=0`，`D(x) < ∞` ⟹ `alpha < 1`，**恒不过冲**；
/// - 一阶追踪 `cur += (tgt-cur)*alpha` 在 `alpha<1` 下**几何收敛且不摆动**。
///
/// 实测偏差（f32 下`x = dt/tau`，对照解析真值 `1-e^{-x}`）：
/// x=0.5 → -0.27%；x=1.0 → -1.13%；x=2.0 → -2.61%；x=2.5~3.0 → -2.86%（最坏）；
/// x=4.0 → -2.44%；x=4.5 → -2.16%。偏差方向**恒为偏慢**且上界约 3%。
///
/// **偏慢在视觉上无害**（过渡略长于设定值），而偏快会拉高单帧亮度变化率——
/// 那是光敏风险，故宁可偏慢。代价量化：中档实测过渡时长比理论值长约 1~3%，
/// 肉眼不可辨。
///
/// **实测的三档单帧最大倍率比（60fps、6EV 阶跃、`step_response_max_ratio`）**：
/// 快档 1.84x（0.88 EV/帧）、中档 1.18x（0.24 EV/帧）、慢档 1.06x（0.08 EV/帧）。
/// **快档的1.84x 已逼近光敏经验阈（单帧 2 倍）**——这是"快档不该更快"的实证：
/// 再压缩 tau 会让6EV 阶跃的首帧变化率越阈，而6EV 阶跃在场景切换后并不罕见。
/// 档位秒数不可再往下调，除非同时提高快档的 tau 下限。
pub fn adapt_alpha(dt_seconds: f32, tau_seconds: f32) -> f32 {
    if !dt_seconds.is_finite() || !tau_seconds.is_finite() || dt_seconds <= 0.0 || tau_seconds <= 0.0
    {
        return 0.0;
    }
    let x = dt_seconds / tau_seconds;
    let d = 1.0 + x + x * x * 0.5 + x * x * x / 6.0;
    let alpha = 1.0 - 1.0 / d;
    // 末端硬钳：浮点误差下 alpha 可能达到 1.0（x 极大时 D 四舍五入到 1.0 的
    // 倒数恰为 0）。alpha=1 本身不违规（一次性到位），但 alpha>1 会过冲，
    // 所以钳到 1.0 而不是放开。
    if alpha < 0.0 {
        0.0
    } else if alpha > 1.0 {
        1.0
    } else {
        alpha
    }
}

/// 一阶惯性过渡器（目标 EV → 当前 EV）。
///
/// **状态只有当前值与目标值两个标量**，O(1) 每帧，无历史缓冲——这一点是锚点
/// 性能分解「过渡 O(1) 每帧」的字面兑现。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExposureAdaptation {
    current_ev: f32,
    target_ev: f32,
    rate: AdaptRate,
}

impl ExposureAdaptation {
    /// 以给定当前/目标 EV 构造（构造即"已适应"，不产生过渡）。
    pub fn new(current_ev: f32, target_ev: f32, rate: AdaptRate) -> Self {
        let c = ExposureValue::from_ev(current_ev);
        let t = ExposureValue::from_ev(target_ev);
        ExposureAdaptation { current_ev: c.ev, target_ev: t.ev, rate }
    }

    /// 当前 EV。
    pub fn current_ev(&self) -> f32 {
        self.current_ev
    }

    /// 目标 EV。
    pub fn target_ev(&self) -> f32 {
        self.target_ev
    }

    /// 当前速率档。
    pub fn rate(&self) -> AdaptRate {
        self.rate
    }

    /// 是否已收敛（|当前-目标| 小于 1/64 EV，即倍率差不到 1.1%）。
    pub fn settled(&self) -> bool {
        (self.current_ev - self.target_ev).abs() < 1.0 / 64.0
    }

    /// 置速率档。
    pub fn set_rate(&mut self, rate: AdaptRate, diags: &mut ExpDiagBag) {
        if rate.custom_clamped() {
            let t = rate.tau_seconds();
            diags.push(ExpDiagCode::RateClamped, ExpSeverity::Corrected, t);
        }
        self.rate = rate;
    }

    /// 提交新的目标 EV（自动曝光每帧都可能给新值）。
    ///
    /// **目标变就变，当前值不动**——这是"过渡"的全部含义。常见错误是让当前值
    /// 直接跳到目标（那样就没有过渡器了），或让目标值反写成当前值（那样自动
    /// 曝光每帧的抖动都会被当成阶跃放大）。
    pub fn submit_target(&mut self, target_ev: f32, diags: &mut ExpDiagBag) {
        let raw = target_ev;
        let t = ExposureValue::from_ev(target_ev);
        if !raw.is_finite() {
            diags.push(ExpDiagCode::EvNonFinite, ExpSeverity::Corrected, 0.0);
        } else if t.clamped {
            diags.push(ExpDiagCode::EvClamped, ExpSeverity::Corrected, t.ev);
        }
        self.target_ev = t.ev;
    }

    /// 场景切换重置：当前 EV **直接跳到**目标 EV，过渡不跨场景。
    ///
    /// 锚点错误路径第2 条。语义是"场景切换 = 曝光快照重置"——新场景有它自己的
    /// 亮度上下文，从旧场景的曝光值渐变过去，中间每一帧都是错的曝光配错的
    /// 画面（从黑夜场景切到白天场景，-6 EV 起步的半秒里画面近乎全黑）。
    ///
    /// `target_valid=false` 用于"同 tick 统计失效"：此时目标本身无效，**保持**
    /// 当前曝光而不是跳到坏目标（见头注「同 tick 组合」）。
    pub fn reset_to_target(&mut self, target_valid: bool, diags: &mut ExpDiagBag) {
        if target_valid {
            self.current_ev = self.target_ev;
        }
        // target_valid=false 时**只留痕不改状态**：保持上一个已知良好曝光。
        diags.push(ExpDiagCode::SceneCutReset, ExpSeverity::Corrected, self.current_ev);
    }

    /// 冻结：把目标钉到当前值（统计失效时用）。
    ///
    /// **冻结是保持不是归零**——归零会让画面全黑，而统计失效只意味着"不知道
    /// 多亮"，画面本身可能正常甚至很亮。这是本条最容易写反的一处。
    pub fn freeze(&mut self, context: f32, diags: &mut ExpDiagBag) {
        self.target_ev = self.current_ev;
        diags.push(ExpDiagCode::AutoStatisticsFrozen, ExpSeverity::Degraded, context);
    }

    /// 解冻（统计恢复）：后续 `submit_target` 重新生效，无需额外动作。
    pub fn resume(&mut self, context: f32, diags: &mut ExpDiagBag) {
        diags.push(ExpDiagCode::AutoStatisticsResumed, ExpSeverity::Corrected, context);
    }

    /// 推进一个逻辑 tick（O(1)）。
    pub fn step(&mut self, dt_seconds: f32) {
        let alpha = adapt_alpha(dt_seconds, self.rate.tau_seconds());
        if alpha <= 0.0 {
            return;
        }
        self.current_ev += (self.target_ev - self.current_ev) * alpha;
        // 当前值也要留在域内：目标已钳制，但插值路径上再加一次不改变语义，
        // 只防浮点累积把值推出域外（例如 target=6、current=6 时 alpha 噪声）。
        if self.current_ev > EV_MAX {
            self.current_ev = EV_MAX;
        } else if self.current_ev < EV_MIN {
            self.current_ev = EV_MIN;
        }
    }

    /// 当前曝光的双表示（供应用位消费）。
    pub fn exposure(&self) -> ExposureValue {
        ExposureValue::from_ev(self.current_ev)
    }
}

// ---------------------------------------------------------------------------
// 六、模式与互斥（F1812 语义：手动优先，自动禁用）
// ---------------------------------------------------------------------------

/// 曝光模式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExposureMode {
    /// 自动（J 域统计驱动）。
    Auto,
    /// 手动（固定 EV）。
    Manual,
}

impl ExposureMode {
    /// 稳定 key。
    pub fn key(self) -> &'static str {
        match self {
            ExposureMode::Auto => "exposure.auto",
            ExposureMode::Manual => "exposure.manual",
        }
    }
}

/// 模式互斥裁决结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModeDecision {
    /// 请求的模式。
    pub requested: ExposureMode,
    /// 生效的模式（手动优先）。
    pub effective: ExposureMode,
    /// 是否发生了冲突裁决（true 时自动被禁用）。
    pub conflicted: bool,
}

/// 互斥裁决：手动优先（F1812「自动+手动同时激活 → 手动优先自动禁用」）。
///
/// 实现上只需看"请求切自动时当前是否已是手动"——若是，则维持手动并记冲突。
/// 反向（请求切手动时当前是自动）不是冲突：那本来就是正常的模式切换，
/// 自动是被**替换**而不是被**混用**。
pub fn resolve_mode(current: ExposureMode, requested: ExposureMode) -> ModeDecision {
    if requested == ExposureMode::Auto && current == ExposureMode::Manual {
        ModeDecision { requested, effective: ExposureMode::Manual, conflicted: true }
    } else {
        ModeDecision { requested, effective: requested, conflicted: false }
    }
}

// ---------------------------------------------------------------------------
// 七、曝光联动状态机（应用位 + 过渡 + 互斥 + 冻结 + 场景重置的合流点）
// ---------------------------------------------------------------------------

/// 曝光联动状态机（每场景一个）。
///
/// **为什么需要一个合流点而不是让J/K 两边各自持状态**：错误路径第 3、4 条
/// （统计失效冻结、互斥）都需要"知道模式 + 知道是否冻结 + 知道当前场景"，
/// 三样都在J 侧语义里。把它们散在J 侧的统计结构与K 侧的过渡器之间，会出现
/// "过渡器以为在追踪、其实已冻结"的静默不同步——冻结失效时曝光会朝一个永不
/// 到来的目标一路渐变到底，画面慢慢变黑而无人报错。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExposureState {
    mode: ExposureMode,
    adaptation: ExposureAdaptation,
    statistics_valid: bool,
    frozen: bool,
    scene_id: u32,
}

impl ExposureState {
    /// 构造（场景首帧即已适应，不产生过渡）。
    pub fn new(scene_id: u32, mode: ExposureMode, initial_ev: f32, rate: AdaptRate) -> Self {
        let v = ExposureValue::from_ev(initial_ev);
        ExposureState {
            mode,
            adaptation: ExposureAdaptation::new(v.ev, v.ev, rate),
            statistics_valid: true,
            frozen: false,
            scene_id,
        }
    }

    /// 生效模式。
    pub fn mode(&self) -> ExposureMode {
        self.mode
    }

    /// 当前场景号。
    pub fn scene_id(&self) -> u32 {
        self.scene_id
    }

    /// 是否冻结（统计失效中）。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 统计是否有效。
    pub fn statistics_valid(&self) -> bool {
        self.statistics_valid
    }

    /// 当前速率档。
    pub fn rate(&self) -> AdaptRate {
        self.adaptation.rate()
    }

    /// 置速率档。
    pub fn set_rate(&mut self, rate: AdaptRate, diags: &mut ExpDiagBag) {
        self.adaptation.set_rate(rate, diags);
    }

    /// 切模式（互斥裁决；手动生效期间收到自动输入会被忽略并留痕）。
    pub fn set_mode(&mut self, requested: ExposureMode, diags: &mut ExpDiagBag) -> ModeDecision {
        let d = resolve_mode(self.mode, requested);
        if d.conflicted {
            diags.push(ExpDiagCode::ModeConflictToManual, ExpSeverity::Degraded, 1.0);
        }
        self.mode = d.effective;
        d
    }

    /// 提交**自动**曝光目标（来自 J 域统计求解的 EV）。
    ///
    /// 手动生效期间按"互斥的必然结果"处理：忽略 + 留痕。**不静默丢弃**是因为
    /// 上游 J 侧可能压根不知道手动已被启用（J/K 状态不同步），留痕才能在遥测里
    /// 看见这种不同步；反过来若默默接受，J侧会一直以为自己还在驱动曝光，
    /// 画面停在手动值上而没有任何一方知道为什么。
    ///
    /// **必须与`submit_manual_ev` 分开而不是合成一个带来源标志的入口**：
    /// 手动模式下用户拖动 EV 滑块是**合法输入**，若在此处一并忽略，用户会发现
    /// 手动模式下滑块完全不起作用——这是"互斥"被实现成"手动通道也失能"的
    /// 典型事故，且现象（滑块无响应）与原因（K 侧互斥）隔着两层调用栈。
    pub fn submit_auto_ev(&mut self, ev: f32, diags: &mut ExpDiagBag) {
        if self.mode == ExposureMode::Manual {
            diags.push(
                ExpDiagCode::AutoInputIgnoredWhileManual,
                ExpSeverity::Corrected,
                ev,
            );
            return;
        }
        if self.frozen {
            return;
        }
        self.adaptation.submit_target(ev, diags);
    }

    /// 提交**手动**曝光目标（用户直接设定 EV）。
    ///
    /// 手动通道不接受互斥约束——它就是互斥的赢家。冻结期间（统计失效）手动输入
    /// 仍然生效：这正是用户在自动曝光于全黑场景失效时的自救手段，若一并冻结
    /// 就把用户锁死在"画面全黑且无法调节"。
    pub fn submit_manual_ev(&mut self, ev: f32, diags: &mut ExpDiagBag) {
        self.adaptation.submit_target(ev, diags);
    }

    /// 置统计有效性（J 侧亮度统计缓冲的饱和保护结果）。
    pub fn set_statistics_valid(&mut self, valid: bool, diags: &mut ExpDiagBag) {
        if valid == self.statistics_valid {
            return;
        }
        self.statistics_valid = valid;
        if valid {
            self.frozen = false;
            self.adaptation.resume(self.adaptation.current_ev(), diags);
        } else {
            self.frozen = true;
            self.adaptation.freeze(self.adaptation.current_ev(), diags);
        }
    }

    /// 场景切换 → 曝光快照重置（过渡不跨场景）。
    ///
    /// 顺序很关键：**先判统计有效性，再重置**。统计失效时目标无效，重置必须
    /// 退化为"保持"，否则会把场景切换变成"跳进坏目标"。
    pub fn begin_scene(&mut self, scene_id: u32, diags: &mut ExpDiagBag) {
        self.scene_id = scene_id;
        let target_valid = self.statistics_valid && !self.frozen;
        self.adaptation.reset_to_target(target_valid, diags);
    }

    /// 推进一个逻辑 tick（O(1)：一次乘加）。
    pub fn step(&mut self, dt_seconds: f32) {
        self.adaptation.step(dt_seconds);
    }

    /// 曝光应用位：把场景线性色变成 TM 入参（零额外 pass）。
    pub fn apply(&self, scene: SceneReferred) -> ExposedFrame {
        apply_exposure(scene, self.adaptation.exposure())
    }

    /// 当前曝光的双表示（调试面板/泛光联动回读）。
    pub fn exposure(&self) -> ExposureValue {
        self.adaptation.exposure()
    }

    /// 人读一行（设置页/调试面板）。
    pub fn screen_text(&self) -> String {
        let v = self.exposure();
        format!(
            "{} · {} · EV {:+.2}（×{:.3}）· 目标 {:+.2} · {}{}",
            self.scene_id,
            match self.mode {
                ExposureMode::Auto => "自动",
                ExposureMode::Manual => "手动",
            },
            v.ev,
            v.multiplier,
            self.adaptation.target_ev(),
            self.adaptation.rate().label(),
            if self.frozen { " · 统计失效已冻结" } else { "" }
        )
    }
}

// ---------------------------------------------------------------------------
// 八、自检支撑（判据自查的最小工具；正式判据在 vek07_checks.rs）
// ---------------------------------------------------------------------------

/// 过渡收敛模拟（判据「眼适应过渡」的回归支撑）。
///
/// 返回 `t` 个 tick 后的当前 EV。要求 `dt > 0`（否则无意义），且返回值始终
/// 落在域内——非有限输入一律退回中性并保持不动。
pub fn simulate_adaptation(
    start_ev: f32,
    target_ev: f32,
    rate: AdaptRate,
    dt_seconds: f32,
    ticks: u32,
) -> f32 {
    let mut a = ExposureAdaptation::new(start_ev, target_ev, rate);
    for _ in 0..ticks {
        a.step(dt_seconds);
    }
    a.current_ev()
}

/// 阶跃响应的最大单帧亮度变化率（光敏舒适判据用）。
///
/// 单位是"每帧倍率变化"。光敏舒适的经验阈是单帧不超过 2 倍（0.5 EV/帧），
/// 本函数把"过渡是否真的限住了跳变"变成可机检的数，而不是靠肉眼。
pub fn step_response_max_ratio(start_ev: f32, target_ev: f32, rate: AdaptRate, dt_seconds: f32) -> f32 {
    let mut a = ExposureAdaptation::new(start_ev, target_ev, rate);
    let mut prev = ev_to_multiplier(a.current_ev());
    let mut worst = 1.0f32;
    for _ in 0..600 {
        a.step(dt_seconds);
        let m = ev_to_multiplier(a.current_ev());
        if m > 0.0 && prev > 0.0 {
            let ratio = if m > prev { m / prev } else { prev / m };
            if ratio > worst {
                worst = ratio;
            }
        }
        prev = m;
    }
    worst
}

/// 契约测试入口（J 管计算 / K 管应用的分工闭环 + 预留兑现登记齐备）。
///
/// 返回全部通过否。这不是自检统计的一部分（那些在 `vek07_checks.rs`），而是
/// 给**运行时**也能调的一条断言——契约表被改空时，自检绿着但运行时无人知情。
pub fn vek07_contract_ok() -> bool {
    EXPOSURE_CONTRACT_DOC.len() == 4
        && F1812_REDEMPTION_DOC.len() == 4
        && EXPOSURE_CONTRACT_DOC.iter().all(|(_, owner, kside)| {
            !owner.is_empty() && !kside.is_empty()
        })
        && F1812_REDEMPTION_DOC.iter().all(|(slot, redeem)| {
            !slot.is_empty() && !redeem.is_empty()
        })
        && ADAPTATION_ISOMORPH_DECL.contains("F1927")
        && PHOTOSENSITIVE_DECL.contains("F1963")
}
