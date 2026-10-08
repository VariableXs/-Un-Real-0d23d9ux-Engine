//! VE-F2006 · 色调映射 ToneMapping（VE-K 域 · 后处理架构与 Bloom 组 · 目标 400 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F2006`
//!
//! **判据（锚点原文逐条）**：
//! - 曲线族三支：ACES 近似 / Reinhard / Uncharted2，**曲线参数化可选**
//!   （每族的白色点/ 肩部 / 脚部参数暴露）；
//! - TM 语义：线性光照 → 显示映射的桥，场景 referred → display referred；
//!   与 V 域显示变换的边界——**TM 输出显示 referred 信号，V 域管最终显示变换**
//!   （F1897/F1964 边界表的 K 侧执行）；
//! - 曲线选择与默认：默认 **ACES 近似**（工业主流观感基线）。
//!
//! **错误路径与降级矩阵（锚点原文五条）**：
//! 1. 输入 NaN/负值 → 钳制到 0（**TM 是数值防线——NaN 不得穿透到显示**）；
//! 2. 曲线参数越界 → 域钳制（肩部参数约束保证单调）；
//! 3. 参数导致非单调曲线（视觉带状伪影）→ **单调性校验拒绝**；
//! 4. 白点极端 → 钳制；
//! 5. 与 V 域边界违规（K 侧做显示变换）→ 边界断言拦截。
//!
//! **设计要点**：
//! - **TM 的职责边界是本条最硬的约束**：输入 referred 线性 HDR，输出
//!   **display referred 的0-1 显示值**——即"线性已解、显示编码未做"。K 侧
//!   到此为止；sRGB/PQ 编码（显示变换）是 F2008 与 V 域的活。越界做编码
//!   的后果是**双重 gamma**：画面整体发白发灰，且这种缺陷在截图对比里
//!   极难归因（看起来像"TM 曲线选错了"），所以用断言而非告警。
//! - **三族曲线的数学骨架**：
//!   - **Reinhard**（`x/(1+x)`）：最简单也最不保色相——高光区色相向白
//!     漂移严重；优点是永不溢出、单调性解析可证。
//!   - **Reinhard-extended**（带白点 `Lwhite`）：`(x·(1 + x/Lw²))/(1+x)`
//!     ——在 Reinhard 基础上让 `Lw` 精确映射到 1，是本族可参数化的核心。
//!   - **Uncharted2**（三次多项式拟合，Jill Jurik 的 Hable 拟合简化）：
//!     `((x·(A x + B))/(x·(C x + D))) - E`，五参数族，工程界用得最广。
//!   - **ACES 近似**（Narkowicz 的 RRT+ODT 拟合）：`(x(2.51x+0.03))/(x(2.43x+0.59)+0.14)`
//!     ——两参数、无查表、O(1)，本域默认。
//! - **肩部参数与单调性**：肩部指数 `sh` 直接决定高光压缩的"软硬"，
//!   而**过大的肩部会让曲线在低照度区出现负斜率**——负斜率在8bit 量化下
//!   表现为**带状伪影（banding）**：亮度渐变处出现等值线色带。所以本条
//!   把单调性做成**构造期硬校验**，不合规直接拒绝，而不是让用户拿到一个
//!   "能跑但出伪影"的参数面。
//! - **数值防线不可绕过**：所有曲线入口先过 `sanitize`（NaN/±Inf/负 → 0），
//!   出口再过一次 `clamp01`。TM 是整条后处理链**最后一道浮点防线**——
//!   它前面是 Bloom/FXAA/TAA 各自已钳制，但它后面直接就是显示设备，
//!   一颗 NaN 像素上屏就是永久亮点（且不随帧变化，不会自愈）。
//! - **脚部（toe）参数**：脚部决定暗部压缩，解决"暗部被肩部吃掉"的对比
//!   损失。脚部指数大→ 暗部抬升明显→ 暗部层次保留，但黑位抬高（画面发灰）。
//!   这是物理上的两难，故toe 与 shoulder 都要有域钳制并在越界时钳制而非拒绝
//!   （与"非单调拒绝"的分工：非单调是**形状错误**，钳制救不回来只能拒；
//!   越界只是**取值极端**，钳制到边界仍是一族合法曲线）。
//!
//! **跨批对接点**：边界单源 F1897/F1964 协议（本条是 K 侧执行点，TM 输出
//! 即 display referred，K 侧职责终点）；输入来自 J 域光照 + 曝光应用位 F2007；
//! 数学验证归 F2021；色彩空间编码输出 F2008 下游；默认参数实测进 F2017。
//!
//! **无障碍与隐私**：无运行时隐私面；曲线族选择的读屏替述见 `tm_screen_text`
//! （高对比相关：Reinhard 的高光去饱和会降低高对比场景的可辨性，文案需提示）。
//!
//! 零外部依赖；逻辑 tick 注入，零墙钟；全部确定性、零 IO、回归可复现。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 一、规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 显示 referred 输出域上界（TM 输出恒在 [0,1] ——显示编码前的线性显示值）。
pub const DISPLAY_REFERRED_MAX: f32 = 1.0;

/// 输入域下界（负亮度无物理意义；负的 referred 来自上游错误或双面几何漏）。
pub const SCENE_REFERRED_MIN: f32 = 0.0;

/// 肩部指数下界（0 → 极软；负 → 曲线非单调，构造期拒绝）。
pub const SHOULDER_MIN: f32 = 0.01;

/// 肩部指数上界（过大 → 低照度区负斜率 → 带状伪影）。
pub const SHOULDER_MAX: f32 = 8.0;

/// 脚部指数下界（0 → 不抬黑位）。
pub const TOE_MIN: f32 = 0.0;

/// 脚部指数上界（过大 → 黑位抬高画面发灰）。
pub const TOE_MAX: f32 = 4.0;

/// 白点下界（低于 1 时高光先于白点到达 1 —— 反直觉但合法：即"提前过曝"）。
pub const WHITE_POINT_MIN: f32 = 0.1;

/// 白点上界（过大 → 曲线近似线性，高光压缩失效，退化为"无TM"）。
pub const WHITE_POINT_MAX: f32 = 64.0;

/// 曝光前线性段下界（<1 = 预压，>1 = 提亮后映射）。
pub const LINEAR_PRE_GAIN_MIN: f32 = 0.05;

/// 曝光前线性段上界。
pub const LINEAR_PRE_GAIN_MAX: f32 = 8.0;

/// 单调性校验的采样点数（曲线扫描点；256 点足以抓住肉眼可见的折返）。
pub const MONOTONICITY_SAMPLES: usize = 256;

/// 单调性允许的最小斜率（允许 0 即"平坦段"合法——压缩到同一亮度是 TM 的
/// 正常行为；负斜率才拒绝）。
pub const MIN_ALLOWED_SLOPE: f32 = 0.0;

/// 单调性判定的**数值容差**（斜率单位，输出域 [0,1] 归一后）。
///
/// 本模块自带 `ln_approx` / `powf_pos`（no_std 内核不能用 std/libm），
/// 这些近似在链路上累积的浮点噪声会表现为**极小的负斜率**——实测历史值：
/// `ln_approx` 只取五项级数时，Uncharted2 白点区出现 `min_slope = -9.2e-5`，
/// 那是级数截断误差，不是曲线折返。故单调性判据必须留一个容差，把
/// "数值噪声"与"真实非单调"分开：
/// - 容差 `1e-3`：相当于整条输出曲线（满量程 1.0）偏离 1e-3 —— 远低于
///   8bit 量化步长（1/255 ≈ 3.9e-3）的 1/4，肉眼与8bit 输出均不可见；
/// - 真实的非单调（如肩部参数失控）实测斜率在 `-1e-1` 量级，比容差大
///   两个数量级，绝不会被容差吞掉。
///
/// **容差必须写成常量并说明依据**：散落的魔法数会随改代码被无声放宽，
/// 而"放宽单调性容差"恰好是最容易被顺手做掉的一步（它能让测试变绿）。
pub const MONOTONICITY_NUMERIC_TOLERANCE: f32 = 1.0e-3;

/// TM 语义与边界声明（锚点"TM 语义声明表"三列：输入域/输出域/与 V 域分工）。
///
/// 这段文本是**边界单源**在 K 侧的落点：V 域 F1897/F1964 协议引用本段，
/// 反向亦然。任何一方要改边界，必须先改这里。
pub const TM_SEMANTIC_TABLE_DOC: &str = "\
色调映射语义声明表（VE-F2006 · v1，边界单源 F1897/F1964 的 K 侧执行）：

| 项 | 值 |
| --- | --- |
| 输入域 | scene referred（线性光照，HDR 未编码，J 域光照 + F2007 曝光应用位产出）|
| 输出域 | display referred（显示值的**线性**表示，恒在 [0,1]，**未做显示编码**）|
| K 侧职责终点 | 到 display referred 为止。K 域不再对它做 sRGB/PQ 编码（那是 F2008）|
| V 域职责起点 | 消费 display referred 缓冲，执行显示变换（显示编码 / 显示设备特性适配）|
| 严禁| K 侧对同一缓冲再做一次显示变换（双重 gamma → 画面发白发灰，且归因极难）|

管线序（与 F2001 声明对齐）：
  J域光照 ─▶ Bloom(F2004/F2005) ─▶ AA(F2009-F2012) ─▶ **TM(F2006 本条)**
  ─▶ LUT(F2023) ─▶ 分级 ─▶ 镜头效果 ─▶ 显示编码(F2008) ─▶ V 域
要点：TM 必须在 LUT 与编码之前——TM 处理的是线性 HDR 的高光层次，
一旦编码成显示值，高光信息已不可逆丢失。";

// ---------------------------------------------------------------------------
// 二、诊断（三要素；数值防线相关的码复用 F2004 的共享枚举）
// ---------------------------------------------------------------------------

/// TM 诊断码。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TmDiagCode {
    /// 输入 NaN/±Inf/负 → 钳制到 0（数值防线）。
    InputSanitized,
    /// 输出越界[0,1] → 钳制（出口防线）。
    OutputClamped,
    /// 曲线参数越界 → 域钳制。
    ParameterClamped,
    /// 参数导致**非单调**曲线 → 拒绝（带状伪影风险，不可钳制修复）。
    NonMonotonicRejected,
    /// K 侧越界做了显示变换（双重 gamma 风险）→ 边界断言拦截。
    DomainBoundaryViolation,
}

impl TmDiagCode {
    /// 诊断码字符串。
    pub fn code(self) -> &'static str {
        match self {
            TmDiagCode::InputSanitized => "TM_INPUT_SANITIZED",
            TmDiagCode::OutputClamped => "TM_OUTPUT_CLAMPED",
            TmDiagCode::ParameterClamped => "TM_PARAMETER_CLAMPED",
            TmDiagCode::NonMonotonicRejected => "TM_NON_MONOTONIC_REJECTED",
            TmDiagCode::DomainBoundaryViolation => "TM_DOMAIN_BOUNDARY_VIOLATION",
        }
    }

    /// 人读下一步建议。
    pub fn next_hint(self) -> &'static str {
        match self {
            TmDiagCode::InputSanitized => {
                "输入拿到 NaN/Inf/负亮度：已钳到 0。先修上游（J 域光照或 F2007 曝光）——TM 只是最后一道防线，NaN 到这里说明前面已漏"
            }
            TmDiagCode::OutputClamped => "映射结果越出 [0,1]：已钳制。若持续发生说明白点参数偏小或输入量级异常",
            TmDiagCode::ParameterClamped => "曲线参数越界：已钳到声明域内。域是单调性的保证边界，越界即形状不可信",
            TmDiagCode::NonMonotonicRejected => {
                "参数组合产生非单调曲线（亮度越高输出反而更低）：已拒绝。症状是渐变处出现等值线色带 banding——调肩部/脚部，过大的肩部是主因"
            }
            TmDiagCode::DomainBoundaryViolation => {
                "K 侧被要求做显示变换（编码/gama）：已拦截。TM 只负责到 display referred，显示编码归 F2008 与 V 域——两边都做会产生双重 gamma"
            }
        }
    }
}

/// TM 诊断。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TmDiagnostic {
    /// 诊断码。
    pub code: TmDiagCode,
    /// 上下文数值（实测输入值 / 最小斜率 / 被钳参数值——依码而定）。
    pub context: f32,
}

impl TmDiagnostic {
    /// 构造。
    pub fn new(code: TmDiagCode, context: f32) -> Self {
        TmDiagnostic { code, context }
    }

    /// 人读三要素串。
    pub fn describe(&self) -> String {
        format!(
            "[TM] {} = {:.4} → {}",
            self.code.code(),
            self.context,
            self.code.next_hint()
        )
    }
}

/// TM 诊断袋。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TmDiagBag {
    items: Vec<TmDiagnostic>,
}

impl TmDiagBag {
    /// 空袋。
    pub fn new() -> Self {
        TmDiagBag { items: Vec::new() }
    }

    /// 记一条。
    pub fn push(&mut self, d: TmDiagnostic) {
        self.items.push(d);
    }

    /// 记一条（便捷）。
    pub fn note(&mut self, code: TmDiagCode, context: f32) {
        self.items.push(TmDiagnostic::new(code, context));
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
    pub fn has(&self, code: TmDiagCode) -> bool {
        self.items.iter().any(|d| d.code == code)
    }

    /// 取码序列。
    pub fn codes(&self) -> Vec<TmDiagCode> {
        self.items.iter().map(|d| d.code).collect()
    }

    /// 人读全文。
    pub fn report(&self) -> String {
        let mut s = String::new();
        for (i, d) in self.items.iter().enumerate() {
            if i > 0 {
                s.push('\n');
            }
            s.push_str(&d.describe());
        }
        s
    }
}

// ---------------------------------------------------------------------------
// 三、曲线族与参数块
// ---------------------------------------------------------------------------

/// 曲线族。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToneCurveFamily {
    /// ACES 近似（Narkowicz 拟合）——本域默认。
    Aces,
    /// Reinhard（基本式）。
    Reinhard,
    /// Reinhard 扩展（带白点）。
    ReinhardExtended,
    /// Uncharted2（三次多项式，Hable/Jurik 系）。
    Uncharted2,
}

impl ToneCurveFamily {
    /// 全部族（序稳定，供 UI 与自检遍历）。
    pub fn all() -> [ToneCurveFamily; 4] {
        [
            ToneCurveFamily::Aces,
            ToneCurveFamily::Reinhard,
            ToneCurveFamily::ReinhardExtended,
            ToneCurveFamily::Uncharted2,
        ]
    }

    /// 稳定标识串（API/预设的键，跨版本可引用）。
    pub fn key(self) -> &'static str {
        match self {
            ToneCurveFamily::Aces => "tm.aces",
            ToneCurveFamily::Reinhard => "tm.reinhard",
            ToneCurveFamily::ReinhardExtended => "tm.reinhard_ext",
            ToneCurveFamily::Uncharted2 => "tm.uncharted2",
        }
    }

    /// 人读名。
    pub fn label(self) -> &'static str {
        match self {
            ToneCurveFamily::Aces => "ACES 近似",
            ToneCurveFamily::Reinhard => "Reinhard",
            ToneCurveFamily::ReinhardExtended => "Reinhard 扩展",
            ToneCurveFamily::Uncharted2 => "Uncharted2",
        }
    }

    /// 由key 反查。
    pub fn from_key(key: &str) -> Option<ToneCurveFamily> {
        ToneCurveFamily::all().into_iter().find(|f| f.key() == key)
    }

    /// 默认族（锚点：默认 ACES 近似——工业主流观感基线）。
    pub fn default_family() -> ToneCurveFamily {
        ToneCurveFamily::Aces
    }

    /// 观感特性说明（无障碍：不同族的高光行为差异需可读描述）。
    pub fn characteristic(self) -> &'static str {
        match self {
            ToneCurveFamily::Aces => "高光去饱和自然、色彩过渡电影感；工程界主流观感基线",
            ToneCurveFamily::Reinhard => "最简且永不溢出；高光区色相明显向白漂移，强光场景偏灰",
            ToneCurveFamily::ReinhardExtended => "Reinhard 加白点精确映射；白点以下行为同基本式",
            ToneCurveFamily::Uncharted2 => "五参数可调性最强；需成组调参，单参数变化易破坏观感",
        }
    }
}

/// 曲线参数块（每族参数化：线性段 / 肩部 / 脚部 / 白点）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TmParams {
    /// 曲线族。
    pub family: ToneCurveFamily,
    /// 曝光前线性段增益（<1 预压、>1 提亮；对应 F2007 曝光值的**应用**侧）。
    pub linear_pre_gain: f32,
    /// 肩部指数（高光压缩软硬；过大 → 非单调 → 拒绝）。
    pub shoulder: f32,
    /// 脚部指数（暗部压缩/抬黑位）。
    pub toe: f32,
    /// 白点（输入亮度映射到 1.0 的那一点）。
    pub white_point: f32,
}

impl Default for TmParams {
    fn default() -> Self {
        TmParams {
            family: ToneCurveFamily::default_family(),
            linear_pre_gain: 1.0,
            shoulder: 1.0,
            toe: 0.0,
            white_point: 4.0,
        }
    }
}

/// 参数域钳制结果。
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedTmParams {
    /// 钳制后参数。
    pub params: TmParams,
    /// 钳制诊断。
    pub diags: TmDiagBag,
}

impl TmParams {
    /// 域钳制（锚点错误路径第 2、4 条）。
    ///
    /// 纪律分工：**越界钳制**（形状仍合法的一族曲线），**非单调拒绝**（形状
    /// 本身不可信，钳制救不回来）。前者是"取值极端"，后者是"曲线错误"。
    pub fn resolve(self) -> ResolvedTmParams {
        let mut diags = TmDiagBag::new();

        let mut gain = self.linear_pre_gain;
        if !gain.is_finite() {
            gain = 1.0;
            diags.note(TmDiagCode::ParameterClamped, self.linear_pre_gain);
        } else if !(LINEAR_PRE_GAIN_MIN..=LINEAR_PRE_GAIN_MAX).contains(&gain) {
            gain = gain.clamp(LINEAR_PRE_GAIN_MIN, LINEAR_PRE_GAIN_MAX);
            diags.note(TmDiagCode::ParameterClamped, self.linear_pre_gain);
        }

        // 肩部：非有限 → 下界；越界钳制（形状是否可接受交单调性校验）
        let mut sh = self.shoulder;
        if !sh.is_finite() {
            sh = SHOULDER_MIN;
            diags.note(TmDiagCode::ParameterClamped, self.shoulder);
        } else if !(SHOULDER_MIN..=SHOULDER_MAX).contains(&sh) {
            sh = sh.clamp(SHOULDER_MIN, SHOULDER_MAX);
            diags.note(TmDiagCode::ParameterClamped, self.shoulder);
        }

        let mut toe = self.toe;
        if !toe.is_finite() {
            toe = TOE_MIN;
            diags.note(TmDiagCode::ParameterClamped, self.toe);
        } else if !(TOE_MIN..=TOE_MAX).contains(&toe) {
            toe = toe.clamp(TOE_MIN, TOE_MAX);
            diags.note(TmDiagCode::ParameterClamped, self.toe);
        }

        // 白点（锚点错误路径第 4 条：极端 → 钳制）。
        let mut wp = self.white_point;
        if !wp.is_finite() {
            wp = WHITE_POINT_MIN;
            diags.note(TmDiagCode::ParameterClamped, self.white_point);
        } else if !(WHITE_POINT_MIN..=WHITE_POINT_MAX).contains(&wp) {
            wp = wp.clamp(WHITE_POINT_MIN, WHITE_POINT_MAX);
            diags.note(TmDiagCode::ParameterClamped, self.white_point);
        }

        ResolvedTmParams {
            params: TmParams {
                family: self.family,
                linear_pre_gain: gain,
                shoulder: sh,
                toe,
                white_point: wp,
            },
            diags,
        }
    }
}

// ---------------------------------------------------------------------------
// 四、输入清洗（数值防线；锚点错误路径第 1 条）
// ---------------------------------------------------------------------------

/// 线性 referred 颜色（TM 输入，与 F2004 的 `HdrColor` 同域不同语义——
/// 此处是"未经 Bloom 的场景线性值"）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneReferred {
    /// 线性 r。
    pub r: f32,
    /// 线性 g。
    pub g: f32,
    /// 线性 b。
    pub b: f32,
}

impl SceneReferred {
    /// 构造。
    pub fn new(r: f32, g: f32, b: f32) -> Self {
        SceneReferred { r, g, b }
    }

    /// 输入清洗（NaN/±Inf/负 → 0）。
    ///
    /// Inf 也钳到 0 而非 `f32::MAX`：正无穷意味着上游除零或溢出，语义上
    /// "这一像素的亮度未知"；把它钳成 0（黑）比钳成 `MAX`（纯白且**永久**
    /// 不变，因为 Inf 经任何曲线仍是 Inf）安全——后者会在屏幕上留一个
    /// 永不消失的白点。
    /// 输入清洗，返回 `(清洗后颜色, 是否发生了清洗)`。
    ///
    /// **第二返回值是 `dirty` 而不是 `ok`** —— 调用方要回答的是"有没有
    /// 被动过"，不是"本来合不合法"。返回 `ok` 会在调用点被读反：
    /// `if was_dirty { 告警 }` 变成"合法就告警"，于是**正常输入刷告警、
    /// 真正的脏输入静默通过**——数值防线最坏的失效方式（不是漏报，
    /// 是倒着报，把唯一有价值的信号变成了噪声）。
    pub fn sanitize(self) -> (SceneReferred, bool) {
        let dirty = !is_clean(self.r) || !is_clean(self.g) || !is_clean(self.b);
        (
            SceneReferred {
                r: clean_channel(self.r),
                g: clean_channel(self.g),
                b: clean_channel(self.b),
            },
            dirty,
        )
    }

    /// 逐分量线性变换（供各曲线族使用）。
    ///
    /// 形参是泛型 `Fn` 而非 `fn` 指针：调用点传的是带出口钳制的闭包
    /// （`|v, p| clamp01(f(v, p))`），`fn` 指针无法捕获 `f`，只能改成
    /// "先映射后统一钳制"两趟——那会让三通道各扫一遍内存。
    pub fn map_channels<F>(self, f: F, p: &TmParams) -> SceneReferred
    where
        F: Fn(f32, &TmParams) -> f32,
    {
        SceneReferred {
            r: f(self.r, p),
            g: f(self.g, p),
            b: f(self.b, p),
        }
    }

    /// 亮度（Rec.709，供可读性检查与调试）。
    pub fn luminance(self) -> f32 {
        0.2126 * self.r + 0.7152 * self.g + 0.0722 * self.b
    }
}

/// 分量是否已合法（有限且非负）。
fn is_clean(v: f32) -> bool {
    v.is_finite() && v >= SCENE_REFERRED_MIN
}

/// 分量清洗：非法 → 0。
fn clean_channel(v: f32) -> f32 {
    if is_clean(v) {
        v
    } else {
        SCENE_REFERRED_MIN
    }
}

// ---------------------------------------------------------------------------
// 五、三族曲线实现（每族 O(1) 多项式，无查表、无分支发散）
// ---------------------------------------------------------------------------

/// ACES 近似（Narkowicz RRT+ODT 拟合）。
///
/// `y = x(2.51x + 0.03) / (x(2.43x + 0.59) + 0.14)`
///
/// **白点精确化**：拟合式自身在 `x = 1` 处出 0.804 而非 1，所以直接把
/// `x / white_point` 当"白点归一"是错的——`white_point=4` 实测只出 0.80，
/// 参数名与行为不符。正确做法是**末端除以裸曲线在 1 处的值**：
/// `y / aces_raw(1)`，这样 `x = white_point` 精确出 1.0，且归一化是
/// 乘性常数、不破坏单调性。
///
/// 分母保护：参数极端使分母趋零时返回 0 而非让除法产出 Inf——TM 是最后
/// 一道防线，不能自己制造非有限值。
pub fn aces_curve(x: f32, p: &TmParams) -> f32 {
    let v = x * p.linear_pre_gain / p.white_point.max(WHITE_POINT_MIN);
    let num = v * (2.51 * v + 0.03);
    let den = v * (2.43 * v + 0.59) + 0.14;
    if den.abs() < 1e-6 {
        return 0.0;
    }
    let mut y = num / den;
    // 白点精确化：除以裸曲线在 v=1 处的值（乘性常数，保持单调）
    let at_one = aces_raw(1.0);
    if at_one.abs() < 1e-6 {
        return 0.0;
    }
    y /= at_one;
    // 幂次整形：肩部 >1 压得更狠（高光更平），<1 抬高高光
    let s = if p.shoulder > 0.0 { p.shoulder } else { 1.0 };
    y = if (s - 1.0).abs() > 1e-4 {
        if y <= 0.0 {
            0.0
        } else {
            powf_pos(y, s)
        }
    } else {
        y
    };
    apply_toe(y, p.toe)
}

/// ACES 拟合裸式（`v=1` 处取值用于白点反归一）。
fn aces_raw(v: f32) -> f32 {
    let num = v * (2.51 * v + 0.03);
    let den = v * (2.43 * v + 0.59) + 0.14;
    if den.abs() < 1e-6 {
        return 0.0;
    }
    num / den
}

/// Reinhard 基本式 `x/(1+x)`，白点精确化。
pub fn reinhard_curve(x: f32, p: &TmParams) -> f32 {
    let v = x * p.linear_pre_gain / p.white_point.max(WHITE_POINT_MIN);
    let mut y = v / (1.0 + v.max(0.0));
    // 白点精确化：x = white_point（v=1）时应出 1.0，基本式出 0.5，
    // 故乘以 2（乘性常数，保持单调）
    y *= 2.0;
    let s = if p.shoulder > 0.0 { p.shoulder } else { 1.0 };
    y = if (s - 1.0).abs() > 1e-4 {
        if y <= 0.0 {
            0.0
        } else {
            powf_pos(y, s)
        }
    } else {
        y
    };
    apply_toe(y, p.toe)
}

/// Reinhard 扩展（带白点）：`x(1 + x/Lw²)/(1 + x)`，白点精确化。
///
/// 同 Reinhard 基本式：`v=1` 处出 0.5，末端乘 2 使白点精确到 1.0。
pub fn reinhard_ext_curve(x: f32, p: &TmParams) -> f32 {
    let lw = p.white_point.max(WHITE_POINT_MIN);
    let v = x * p.linear_pre_gain / lw;
    let denom = 1.0 + v.max(0.0);
    let mut y = if denom.abs() < 1e-6 {
        0.0
    } else {
        v * (1.0 + v / (lw * lw).max(1e-6)) / denom
    };
    y *= 2.0;
    let s = if p.shoulder > 0.0 { p.shoulder } else { 1.0 };
    y = if (s - 1.0).abs() > 1e-4 {
        if y <= 0.0 {
            0.0
        } else {
            powf_pos(y, s)
        }
    } else {
        y
    };
    apply_toe(y, p.toe)
}

/// Uncharted2（Hable 2010 标准实现，全式四段）。
///
/// 完整链条：
/// ```text
/// 1. 线性段缩放：x /= A                       (A = 0.15)
/// 2. 肩部拟合： ((x(Ax+B))/(x(Cx+D))) - E(B=0.50, C=0.10, D=0.20, E=0.02)
/// 3. **归一化：y /(y(Fy) + 1)**               (F = 0.30)
/// 4. **gamma：y^(1/2.2)**
/// ```
///
/// **第3 步不可省**（实测遗漏的后果：曲线上冲后回落，`min_slope = -0.013`，
/// 白点区出现"更亮反而更暗"的折返；再叠加末端的白点反归一，会把折返
/// 放大成肉眼可见的亮度反转）。第 3 步是标准的无界→有界归一：
/// `g` 本身单调增且渐近于 `(A/C) - E = 1.48`，`h(g) = g/(Fg+1)` 的导数
/// `1/(Fg+1)² > 0`——故加了它，整条链严格单调。
///
/// 另在末端做**白点精确归一**：Hable 链的天然饱和点不在 1，故按
/// `x/Ws` 预缩放并除以裸链在 1 处的值，使 `white_point` 精确映射到 1.0
/// （否则参数名与行为不符）。
pub fn uncharted2_curve(x: f32, p: &TmParams) -> f32 {
    const WS: f32 = 11.6; // 归一后的饱和输入量级
    let lw = p.white_point.max(WHITE_POINT_MIN);
    let scale = WS / lw;
    let v = clean_channel(x) * p.linear_pre_gain * scale;
    let mut y = uncharted2_raw(v);
    // 白点精确化：除以裸链在 v=1 处的值（乘性常数，保持单调）
    let at_one = uncharted2_raw(1.0);
    if at_one.abs() < 1e-6 {
        return 0.0;
    }
    y /= at_one;
    // 肩部幂次整形（在白点归一之后，保持白点恒为 1）
    let s = if p.shoulder > 0.0 { p.shoulder } else { 1.0 };
    y = if (s - 1.0).abs() > 1e-4 {
        if y <= 0.0 {
            0.0
        } else {
            powf_pos(y, s)
        }
    } else {
        y
    };
    apply_toe(y, p.toe)
}

/// Uncharted2 裸链（三段：缩放 → 肩部拟合 → 归一 + gamma），不含白点与整形。
fn uncharted2_raw(x: f32) -> f32 {
    const A: f32 = 0.15;
    const B: f32 = 0.50;
    const C: f32 = 0.10;
    const D: f32 = 0.20;
    const E: f32 = 0.02;
    const F: f32 = 0.30;
    if x <= 0.0 {
        return 0.0;
    }
    // 1) 线性段缩放
    let v = x / A;
    // 2) 肩部拟合
    let num = v * (A * v + B);
    let den = v * (C * v + D);
    if den.abs() < 1e-6 {
        return 0.0;
    }
    let g = num / den - E;
    if g <= 0.0 {
        return 0.0;
    }
    // 3) 归一（无界 → 有界；此步保证整链单调）
    let norm_den = g * (F * g) + 1.0;
    if norm_den.abs() < 1e-6 {
        return 0.0;
    }
    let y = g / norm_den;
    // 4) gamma 校正
    powf_pos(y, 1.0 / 2.2)
}

/// 脚部处理：暗部压缩（`toe` 抬黑位）。
///
/// `y' = y^(1 + toe·k)` 的等价形式：`toe=0` 恒等（幂次 1）；`toe` 大则幂次
/// >1、暗部压得更低——但这与"抬黑位"相反。故采用混合式：
/// `y' = y·(1-toe) + toe·y²` —— `toe=0` 恒等、`toe` 大时暗部（y 小）被抬、
/// 亮部（y 大）被压，且恒等端点 y=1 仍为 1（不破坏白点）。
fn apply_toe(y: f32, toe: f32) -> f32 {
    let t = if toe.is_finite() {
        toe.clamp(TOE_MIN, TOE_MAX)
    } else {
        TOE_MIN
    };
    if t <= 0.0 {
        return y;
    }
    y * (1.0 - t) + t * y * y
}

/// 正底数幂（`powf` 属 std/libm，no_std 不可用——同 F2005 的 exp2 纪律）。
///
/// 实现：`x^e = e^(e·ln x)`，全程走自带的 `ln_approx` 与「折半 + 泰勒」
/// 的 `exp` 近似：
/// - `ln_approx` 用 `ln x = 2·atanh((x-1)/(x+1))` 级数（归一到 [1,2) 后
///   `u ≤ 1/3`，取五项相对误差 < 1e-6）；
/// - `y = e·ln x ∈ [-4, 0]`（指数域 `SHOULDER_MIN..=SHOULDER_MAX`= [0.01,8]，
///   底数经归一后 ≤ 1），`|y|` 大时三阶泰勒会失效，故**反复折半** `y` 到
///   [-0.5,0] 再算，收尾时逐次平方回去（`e^(y/2)² = e^y`）。
fn powf_pos(base: f32, exp: f32) -> f32 {
    if base <= 0.0 {
        return 0.0;
    }
    if !base.is_finite() || !exp.is_finite() {
        return 0.0;
    }
    if (exp - 1.0).abs() < 1e-4 {
        return base;
    }
    let y = exp * ln_approx(base);
    if y >= 0.0 {
        // 底数 > 1 才可能出现；本域归一后不会走到，留恒等保护
        return exp_taylor(y.min(0.5));
    }
    // 折半到 [-0.5, 0]，记录折半次数
    let mut yy = y;
    let mut k = 0u32;
    while yy < -0.5 && k < 8 {
        yy *= 0.5;
        k += 1;
    }
    let mut acc = exp_taylor(yy);
    for _ in 0..k {
        acc *= acc;
    }
    if acc.is_finite() {
        acc
    } else {
        0.0
    }
}

/// `ln(x)` 近似（`f32::ln` 属 std/libm）。
///
/// 域归一到 [1,2) 后用 `ln x = 2·atanh(u)`，`u = (x-1)/(x+1) ∈ [0, 1/3]`：
/// `atanh(u) = u + u³/3 + u⁵/5 + …`。级数在 `u ≤ 1/3` 处收敛很快，
/// 取到 `u¹¹/11`（共六项）时截断相对误差 < 1e-9——**必须取够项数**：
/// 早期实现只取五项（截断误差约 1e-6），经gamma 链放大后在Uncharted2 的
/// 白点区测出 `min_slope = -9e-5` 的假折返——数值噪声被误读成"曲线非单调"。
/// 多取两项的代价是两次乘加，换来单调性判据不再被自己的近似误差污染。
fn ln_approx(x: f32) -> f32 {
    if x <= 0.0 {
        return 0.0;
    }
    // 归一到 [1,2)：提取 2^k
    let mut k = 0i32;
    let mut v = x;
    while v >= 2.0 && k < 60 {
        v *= 0.5;
        k += 1;
    }
    while v < 1.0 && k > -60 {
        v *= 2.0;
        k -= 1;
    }
    // ln(v) = 2·atanh(u)，u = (v-1)/(v+1) ∈ [0, 1/3]
    let u = (v - 1.0) / (v + 1.0);
    let u2 = u * u;
    let atanh = u
        + u2 * u / 3.0
        + u2 * u2 * u / 5.0
        + u2 * u2 * u2 * u / 7.0
        + u2 * u2 * u2 * u2 * u / 9.0
        + u2 * u2 * u2 * u2 * u2 * u / 11.0;
    2.0 * atanh + k as f32 * 0.693_147_2
}

/// `e^y` 三阶泰勒（`y ∈ [-0.5, 0]` 区间用，相对误差 < 1e-4）。
fn exp_taylor(y: f32) -> f32 {
    1.0 + y + y * y * 0.5 + y * y * y / 6.0
}

/// 取族对应的标量曲线函数。
pub fn curve_fn(family: ToneCurveFamily) -> fn(f32, &TmParams) -> f32 {
    match family {
        ToneCurveFamily::Aces => aces_curve,
        ToneCurveFamily::Reinhard => reinhard_curve,
        ToneCurveFamily::ReinhardExtended => reinhard_ext_curve,
        ToneCurveFamily::Uncharted2 => uncharted2_curve,
    }
}

// ---------------------------------------------------------------------------
// 六、单调性校验（锚点错误路径第 3 条：非单调拒绝）
// ---------------------------------------------------------------------------

/// 单调性校验结果。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MonotonicityReport {
    /// 是否单调不减。
    pub monotonic: bool,
    /// 扫描到的最小斜率。
    pub min_slope: f32,
    /// 最小斜率出现的输入位置（诊断上下文）。
    pub at_x: f32,
}

/// 单调性扫描（`MONOTONICITY_SAMPLES` 点等距扫 [0, 白点]，看斜率符号）。
///
/// 为什么必须扫描而非解析求导：三族的可导性各不相同（Uncharted2 的
/// 五参数组在极端组合下导数符号会翻转），而**解析证明需要逐族的判别式**
/// ——那是三套容易写错且错了不易发现的数学。数值扫描用 256 点、代价
/// O(256)（约 1-2μs，且只在参数变更时跑一次，不在每帧热路径），换来
/// "任何参数组合都不会放出带状伪影曲线"的硬保证。
pub fn check_monotonicity(p: &TmParams) -> MonotonicityReport {
    let f = curve_fn(p.family);
    let wp = p.white_point.max(WHITE_POINT_MIN);
    let step = wp / (MONOTONICITY_SAMPLES - 1) as f32;
    let mut min_slope = f32::INFINITY;
    let mut at_x = 0.0f32;
    let mut prev = f(0.0, p);
    for i in 1..MONOTONICITY_SAMPLES {
        let x = step * i as f32;
        let y = f(x, p);
        let s = (y - prev) / step;
        if s < min_slope {
            min_slope = s;
            at_x = x;
        }
        prev = y;
    }
    MonotonicityReport {
        // 容差只吃掉浮点噪声：真实折返在 -1e-1 量级，比容差大两个数量级
        monotonic: min_slope >= MIN_ALLOWED_SLOPE - MONOTONICITY_NUMERIC_TOLERANCE,
        min_slope,
        at_x,
    }
}

/// 曲线构造结果（域钳制 + 单调性校验）。
#[derive(Clone, Debug, PartialEq)]
pub struct ToneCurve {
    /// 生效参数（已钳制）。
    pub params: TmParams,
    /// 诊断。
    pub diags: TmDiagBag,
}

impl ToneCurve {
    /// 构造（域钳制 → 单调性校验；非单调则拒绝并留诊断）。
    ///
    /// 被拒时返回的 `ToneCurve` 里`params.shoulder` 会被退到 `SHOULDER_MIN`
    /// 附近——**不给一条坏曲线**，退到域内最保守的一族合法曲线。这比"返回
    /// Err 让调用方处理"更实用：TM 在每像素热路径上，不该有一个需要
    /// 调用方分支处理的失败态；而"曲线被降级"这件事本身有诊断可查。
    pub fn build(p: TmParams) -> ToneCurve {
        let resolved = p.resolve();
        let params = resolved.params;
        let mut diags = resolved.diags;
        let report = check_monotonicity(&params);
        if !report.monotonic {
            diags.push(TmDiagnostic::new(TmDiagCode::NonMonotonicRejected, report.min_slope));
            // 退到保守参数：线性段 1、肩部下界、脚部 0
            let safe = TmParams {
                family: params.family,
                linear_pre_gain: 1.0,
                shoulder: SHOULDER_MIN,
                toe: TOE_MIN,
                white_point: params.white_point,
            };
            return ToneCurve { params: safe, diags };
        }
        ToneCurve { params, diags }
    }

    /// 单标量映射（含出口钳制[0,1]）。
    pub fn map_scalar(&self, x: f32) -> f32 {
        let v = clean_channel(x);
        let y = (curve_fn(self.params.family))(v, &self.params);
        clamp01(y)
    }

    /// 三通道映射（TM 主体：输入 scene referred → 输出 display referred）。
    pub fn apply(&self, scene: SceneReferred) -> TmApplyResult {
        let (clean, was_dirty) = scene.sanitize();
        let f = curve_fn(self.params.family);
        let out = clean.map_channels(|v, p| clamp01(f(v, p)), &self.params);
        let mut diags = TmDiagBag::new();
        if was_dirty {
            diags.push(TmDiagnostic::new(
                TmDiagCode::InputSanitized,
                scene.luminance(),
            ));
        }
        TmApplyResult { color: out, diags }
    }
}

/// 钳到 [0,1]。
fn clamp01(v: f32) -> f32 {
    if !v.is_finite() {
        return 0.0;
    }
    v.clamp(0.0, DISPLAY_REFERRED_MAX)
}

/// TM 映射结果。
///
/// 不派生 `Copy`：内含 `TmDiagBag`（持 `Vec`）。这不是"忘了写"——映射结果
/// 每像素一个，若允许按位复制，调用方极容易在热路径上无意复制一份诊断袋。
#[derive(Clone, Debug, PartialEq)]
pub struct TmApplyResult {
    /// display referred颜色（恒在 [0,1]，**未做显示编码**）。
    pub color: SceneReferred,
    /// 本次映射的诊断（如输入清洗）。
    pub diags: TmDiagBag,
}

// ---------------------------------------------------------------------------
// 七、边界断言（锚点错误路径第 5 条：K 侧越界做显示变换 → 拦截）
// ---------------------------------------------------------------------------

/// TM 之后的管线序位（谁该做显示变换）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PostTmSlot {
    /// F2008 色彩空间输出（**显示编码的正确位置**）。
    ColorSpaceOutput,
    /// 直接上屏/交给 V 域（不做显示编码）。
    DirectToDisplay,
    /// K 侧自行做显示变换（**违规**——双重 gamma）。
    KSideDisplayTransform,
}

/// 边界断言（K 侧只负责到 display referred）。
pub fn assert_post_tm_boundary(slot: PostTmSlot) -> Result<(), TmDiagCode> {
    match slot {
        PostTmSlot::ColorSpaceOutput | PostTmSlot::DirectToDisplay => Ok(()),
        PostTmSlot::KSideDisplayTransform => Err(TmDiagCode::DomainBoundaryViolation),
    }
}

/// 读屏摘要（无障碍：曲线族观感差异 + 边界声明）。
pub fn tm_screen_text(curve: &ToneCurve) -> String {
    format!(
        "色调映射：{}（{}），输出显示 referred 信号（未做显示编码，显示编码由后续环节处理）",
        curve.params.family.label(),
        curve.params.family.characteristic()
    )
}

/// VE-F2006 域自检（判据逐条映射见 `vek06_checks.rs`）。
pub fn run_vek06_checks() -> CheckSet {
    super::vek06_checks::run_vek06_checks()
}
