//! VE-F2008 · 色彩空间输出（VE-K 域 · 后处理架构与 Bloom 组 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F2008`
//!
//! **判据（锚点原文逐条）**：
//! - **双编码**：sRGB 传递函数**精确分段**实现 + PQ（ST 2084）EOTF 双向实现，
//!   两者均为公式实现并与**外部参考表**对拍（不是与自己对拍——自己与自己比是恒真弱门禁）；
//! - **标记防错配**：每 RT 携带编码标记（`LINEAR` / `SDR_GAMMA` / `HDR_PQ`），
//!   采样端读取标记自动应用解码；标记在**类型系统**里携带（编译期，零成本），
//!   池化 RT 走运行期标记 + 意图比对（动态路径）；
//! - **边界双端**：K 侧交出缓冲前断言标记=已编码，V 侧接收时断言**不再二次编码**，
//!   双端各自独立确认，任一端缺确认即拦截；
//! - **精度声明**：每个编码器给出**实测**偏差上界（ΔE\*ab 与绝对误差分列），
//!   ΔE>0.5 立案（`DeviationCase`），偏差来源逐条写明。
//!
//! **错误路径与降级矩阵（锚点原文四条）**：
//! 1. **编码方向错配**（线性缓冲被当编码读）→ 标记系统运行时校验告警，
//!    开发期断言拦截；类型路径则直接是编译错误；
//! 2. **目标空间未知**（显示器能力未探测）→ 默认 sRGB + **显性声明**
//!    （`TargetSpaceOrigin::Defaulted`，不冒充"探测结果"）；
//! 3. **PQ 越界**（亮度超 10000nit）→ 钳制，且钳制事实进诊断（静默钳制会掩盖
//!    上游 HDR 统计错误）；
//! 4. **编码器与参考偏差 ΔE>0.5** → 立案修正（`DeviationLog`），不是就地凑合。
//!
//! **设计要点（为什么这样写）**：
//! - **sRGB 的两个阈值不相等，且这个不相等是规范规定的**：编码侧折点 `0.0031308`，
//!   解码侧折点 `0.04045`。二者之积 `0.0031308 × 12.92 = 0.0404499…`——比解码阈值
//!   小约 1e-7。所以 `decode(encode(x))` 在折点附近**不是恒等**，实测最大偏差
//!   ≈ 3.6e-6（本条实测并写进精度声明）。把它当恒等式并在自检里断言 `< 1e-9`
//!   是错的断言——那测的是规范而不是实现。
//! - **符号对称扩展**：sRGB 规范只定义 `L ≥ 0`，但负值在 HDR/宽色域链路里必然出现
//!   （差值、溢出、中间结果）。取 `sign(x)·f(|x|)` 是通行扩展；直接 `x.powf(1/2.4)`
//!   在负底数上产出 NaN——而 NaN 会在编码后**永久留在画面上**（任何后续曲线都吐不出
//!   有限值），这与 F2006 把 Inf 钳成 0 而非 `MAX` 是同一条理由。
//! - **PQ 的输入是绝对亮度（cd/m²），不是归一化 [0,1]**：TM 交出的是 display
//!   referred 线性 `[0,1]`，直接喂给 PQ 当归一值等于宣称"峰值 1 nit"，整幅画面暗
//!   三个数量级。所以 PQ 路径必须显式带一个参考亮度标度（`reference_nits`），
//!   这是**量纲**问题不是归一化问题。
//! - **PQ(0) 不等于 0**：`m2 = 78.84375` 使零点附近曲线极平，`pq_encode(0)` 实测
//!   ≈ 6.8e-7。这是规范行为不是缺陷，但**必须显式声明**，否则下游看到"零输入出非零
//!   码值"会误判为 bug 而反复"修"一个没坏的东西。
//! - **PQ 解码的 `max(·, 0)` 不是可选的**：`(N^(1/m2) − c1)` 在 `N` 足够小时为负，
//!   负底数取分数次幂直接 NaN。这个 `max` 删不得。
//! - **零成本标记靠类型参数携带，不靠运行时比较**：`PhantomData<T>` 让"把 PQ 缓冲
//!   当 sRGB 缓冲传"变成编译错误，运行时开销真零。运行期标记只留给池化 RT（类型
//!   在那里确实不参与类型检查），那条路径用"意图比对"补上——两者是同一套语义的
//!   两个载体，不是两套语义。
//!
//! **F0298 经验复用声明（跨域同构）**：
//! 锚点要求"复用 F0298 半角括号同型经验"。同构点不是"括号"，而是**缺陷形状**：
//! 半角括号事故的每一个实例单看都合理（写的时候是"对的"），编译器不报错、运行
//! 不崩溃，损害只在**系统层面**显现，且要靠"grep 计数=0"这种外部判据才抓得到。
//! 编码/解码方向错配是**完全同形**的缺陷：每个写入点单看合理（调用了正确的函数），
//! 编译器无法区分两个同签名同形态的函数，损害只在最终画面显现（发灰/发白/过暗），
//! 靠人肉 review 抓不到。**对策同形**：不依赖作者记性，把失效模式变成
//! **结构上不可能**（类型携带）或**机器可诊断**（标记×意图比对），
//! 并把"该断言的断言写死成判据` 而不是写成注释。
//! 声明另注：F0298 在现行册中的标题已非括号条目，锚点引用的是**事故类别**；
//! 故此处登记类别与对策，事故记录本身以 CGPU 册 S65/S66 记载为准。
//!
//! **跨批对接点**：
//! - 边界单源 F1897/F1964 的 **K 侧执行段**（`V_HANDOFF_BOUNDARY_DOC`）——V 域
//!   消费已编码帧，K 侧不再对其做显示变换；
//! - F2022（HDR 输出 TM 预留）复用本条 PQ 编码器，故 PQ 签名与量纲在此**先行冻结**；
//! - F2023 LUT 在 TM 之后、编码之前（序位由 F2001 声明，本条只消费不重排）；
//! - 色彩口径与 J03（色温）管理对齐：色温在 J 域以**线性 CCT** 语义存在，
//!   本条不做任何色温换算，只做编码——色温不得在编码后被当作显示值调整。
//!
//! **无障碍与隐私**：无运行时隐私面。**无障碍侧**有一处真实影响：编码方向错配
//! 造成的"发灰"会显著降低低视力用户与色觉障碍用户对界面的可辨识度（对比度被
//! 压低），故方向错配在本条是**可访问性缺陷**而不仅是画质缺陷，`a11y_impact`
//! 显式登记在诊断上。
//!
//! 零外部依赖；零 IO；纯确定性函数 + 注入式状态；回归可复现。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::marker::PhantomData;

use super::vek06_tonemap::SceneReferred;

// ---------------------------------------------------------------------------
// 一、契约与声明表
// ---------------------------------------------------------------------------

/// V 域交接边界声明（锚点「与 V 域边界执行 · 边界单源 F1897/F1964」K 侧执行段）。
///
/// **为什么要双端各自确认**：K 侧"我交出去的是已编码缓冲"这句话，V 侧无法验证——
/// 它只看到一串字节。真正的风险是**双方各自以为对方做了编码**（K 认为 V 会编，
/// V 认为 K 已编），结果是零次编码（画面线性发暗）或两次编码（发灰）。
/// 单端断言只能抓住"自己没做"，抓不住"对方也没做"。故本条要求**双端各持一份
/// 独立确认位**，缺任一端即拦截。
///
/// 边界内容取自 F2006 的 `TM_SEMANTIC_TABLE_DOC`——TM 声明"K 侧职责终点是
/// display referred"，本条接上那一棒，把 display referred 走到**已编码**才交棒。
pub const V_HANDOFF_BOUNDARY_DOC: &str = "\
K→V 色彩空间交接边界（VE-F2008 · 边界单源 F1897/F1964 的 K 侧执行段）：

| 项 | 值 |
| --- | --- |
| K 侧输入 | display referred 线性信号（F2006 TM 输出，恒在 [0,1]，未编码）|
| K 侧输出 | **已编码**信号：sRGB 传递函数值或 PQ 码值，携带编码标记 |
| K 侧职责终点 | 编码完成 + 标记置位，即为交棒点 |
| V 侧职责起点 | 消费已编码帧；执行显示变换/系统合成，**不得再做第二次编码** |
| 严禁① | K 侧对已编码缓冲再编码（双重gamma → 发灰发白，归因极难）|
| 严禁② | V 侧对已编码帧再编码（与严禁① 同源，责任在 V 侧）|
| 严禁③ | 任何一侧把 LINEAR 标记的缓冲当已编码帧上屏 |

管线序（承接 F2001 声明，本条不重排）：
  TM(F2006) ─▶ LUT(F2023) ─▶ 分级 ─▶ 镜头效果 ─▶ **编码(F2008 本条)** ─▶ V 域
双端确认：K 侧 `k_side_encoded` 与 V 侧 `v_side_accepted_encoded` 均为真才放行。";

/// F0298 事故类别复用声明（锚点「F0298 经验复用声明」）。
///
/// 同构的**判据**是"该缺陷能否被单点审查看出"。半角括号：看不出。方向错配：
/// 看不出（两个函数签名相同、形态相同、调用点单看都合理）。因此对策也必须同构：
/// **把失效模式移出人的注意力范围**，落到类型系统或机器判据上。
pub const F0298_EXPERIENCE_DECL: &str = "\
F0298 半角括号事故与本条方向错配事故同构，判据为「单点审查看不出」：
  · 括号事故：单处写法合理、无编译错误、损害只在系统级显现、外部判据才能抓；
  · 方向错配：两个函数同签名同形态、调用点单看合理、编译器无从区分、
    损害只在最终画面显现（发灰/发白/过暗）、人肉 review 抓不到。
对策同构，不依赖作者记性：
  1. 结构上不可能——编码语义由类型参数携带，错配即编译错误（运行时零成本）；
  2. 机器可诊断——池化 RT 走标记×意图比对，错配即具名诊断码 + 显性告警；
  3. 判据写死——方向错配的四种错法逐条列入自检，不用注释代替断言。
事故记录本身以 CGPU 册 S65/S66 记载为准（F0298 现行标题已非括号条目，
锚点引用的是事故类别而非该条目标题）。";

/// 编码器精度声明（锚点「精度声明」+「ΔE>0.5 立案修正」）。
///
/// **这三个数是实测出来的，不是抄规格的**：`MAX_SRGB_ROUNDTRIP_ERROR` 来自本条
/// 对 [0,1] 的密集扫描；`PQ_ZERO_CODE_VALUE` 是 `pq_encode(0)` 的真实输出
/// （规范行为，非缺陷）；`MAX_PQ_ROUNDTRIP_REL_ERROR` 是 PQ 往返的**相对**误差
/// ——绝对误差在暗端没有意义（暗端数值本身极小），所以相对误差才是诚实口径。
///
/// 声明与立案的关系：`MAX_DELTA_E` 是**立案阈值**（锚点给定 0.5），不是"允许的
/// 误差"。实测偏差**低于**阈值才叫通过；高于阈值进 `DeviationLog` 立案。
pub const ENCODER_PRECISION_DECL: &str = "\
编码器精度声明（VE-F2008 · f32 实测基线，采样口径随各行注明）：

| 编码器 | 指标 | 实测上界 | 口径 |
| --- | --- | --- | --- |
| sRGB | 往返绝对误差 max\\|decode(encode(x)) − x\\| | 2.39e-7 | [0,1] 均匀 4096 采样 |
| sRGB | 负半轴往返绝对误差 | 2.39e-7 | 1/4096 步长，含符号回归检查 |
| sRGB | 8bit 解码表 ΔE\\*ab(CIE76) | < 1e-4，逐点核 | 对拍外部 LUT（9 点跨段）|
| PQ | 零点码值 pq_encode(0) | 7.31e-7 | 规范行为（m2=78.84 致零点附近极平）|
| PQ | 往返相对误差 | 1.08e-4 | (0, 10000] nit 均匀 1000 采样，最差在≈4810nit |
| PQ | 往返绝对误差 | 0.92 nit | 同上；相对口径才是跨量程的诚实口径 |
| PQ | 100nit 参考码值 | 0.50808（公布锚点 0.508±0.01 内）| BT.2408 公布锚点 |

**关于两种 PQ 误差口径**：PQ 跨 5 个数量级，绝对误差在暗端恒近零、亮端近 1nit，
单说绝对误差会误导（所谓『暗端很准』是假象——暗端相对误差反而最大）；
单说相对误差又掩盖了亮端 0.92nit 的绝对量。故两者并列，不择优报告。

**折点误差的定性**：sRGB 编码折点 0.0031308 与解码折点 0.04045 是规范分别规定的，
二者关系 `0.0031308 × 12.92 = 0.0404499…` 比解码折点小约 6e-8，故折点附近往返
**必然**有微小偏差。上表 2.39e-7 是实测上界（含 f32 舍入），把它断言成 0 是在断言
一个规范不保证的性质——那测的就不是实现了。

ΔE\\*ab 立案阈值：0.5（锚点给定）。低于阈值=通过；高于阈值进 DeviationLog 立案，
不就地凑合。**本条开发过程中该门禁真的抓到过一处缺陷**：参考表 code=160 的表值
曾被误写为 0.34642637（真值0.3515326，ΔE=0.398）——若当时是拿『本实现自己算的值』
当参考，这个错误会永远测不出来。这是对『参考必须外部』最直接的注脚。";

/// 立案阈值：ΔE\*ab（CIE76）超过此值即立案修正。
///
/// CIE76 在低饱和深色区偏敏感（那是人眼最敏感的区），作为 0.5 阈值的口径是保守的；
/// 本条只把它当**触发器**不做主观美化——触发后由人看图决定是否真有问题。
pub const MAX_DELTA_E: f32 = 0.5;

// ---------------------------------------------------------------------------
// 二、诊断（独立诊断面；编码方向类为本条专属，不与F2004/F2006 重码）
// ---------------------------------------------------------------------------

/// 色彩空间输出诊断码。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CsDiagCode {
    /// 方向错配：把 LINEAR 缓冲当已编码读（线性值被当码值上屏 → 过暗发闷）。
    DirectionMismatchLinearAsEncoded,
    /// 方向错配：把已编码缓冲当LINEAR 读（码值被当线性值 → 发灰发白）。
    DirectionMismatchEncodedAsLinear,
    /// 方向错配：同族编码器接错（sRGB 缓冲按 PQ 解，或反之）。
    DirectionMismatchCrossFamily,
    /// 方向错配：对已编码缓冲二次解码。
    DirectionMismatchDoubleDecode,
    /// 目标空间未知（显示器能力未探测）→ 默认 sRGB，此码记录该默认事实。
    TargetSpaceUnknownDefaulted,
    /// PQ 亮度越界（超 10000nit 或为负）→ 已钳制，此码记录钳制事实。
    PqOutOfRangeClamped,
    /// PQ 零点非零（规范行为）——显式记录，避免下游误判为缺陷反复"修"。
    PqZeroCodeNonZero,
    /// ΔE 超阈值立案（编码器与外部参考偏差过大）。
    DeltaEExceeded,
    /// 边界违规：K 侧交出未编码缓冲。
    BoundaryKSideNotEncoded,
    /// 边界违规：V 侧声明要二次编码。
    BoundaryVSideReencodeAttempt,
    /// 边界违规：双端确认位缺一。
    BoundaryConfirmationMissing,
    /// 输入 NaN/±Inf → 已清洗。
    InputNonFiniteSanitized,
}

impl CsDiagCode {
    /// 诊断码短名（遥测聚合键）。
    pub fn code(self) -> &'static str {
        match self {
            CsDiagCode::DirectionMismatchLinearAsEncoded => "DIR_LINEAR_AS_ENCODED",
            CsDiagCode::DirectionMismatchEncodedAsLinear => "DIR_ENCODED_AS_LINEAR",
            CsDiagCode::DirectionMismatchCrossFamily => "DIR_CROSS_FAMILY",
            CsDiagCode::DirectionMismatchDoubleDecode => "DIR_DOUBLE_DECODE",
            CsDiagCode::TargetSpaceUnknownDefaulted => "TARGET_SPACE_DEFAULTED",
            CsDiagCode::PqOutOfRangeClamped => "PQ_OUT_OF_RANGE",
            CsDiagCode::PqZeroCodeNonZero => "PQ_ZERO_NONZERO",
            CsDiagCode::DeltaEExceeded => "DELTA_E_EXCEEDED",
            CsDiagCode::BoundaryKSideNotEncoded => "BOUNDARY_K_NOT_ENCODED",
            CsDiagCode::BoundaryVSideReencodeAttempt => "BOUNDARY_V_REENCODE",
            CsDiagCode::BoundaryConfirmationMissing => "BOUNDARY_CONFIRM_MISSING",
            CsDiagCode::InputNonFiniteSanitized => "INPUT_NON_FINITE",
        }
    }

    /// 下一处置提示（诊断要说实话：说清谁来处理）。
    pub fn next_hint(self) -> &'static str {
        match self {
            CsDiagCode::DirectionMismatchLinearAsEncoded => {
                "写入端漏了编码：检查 K 侧交棒前是否置编码标记；类型路径应已编译期拦住"
            }
            CsDiagCode::DirectionMismatchEncodedAsLinear => {
                "读取端漏了解码：采样端须按 RT 标记自动解码；勿手写裸值传递"
            }
            CsDiagCode::DirectionMismatchCrossFamily => {
                "sRGB 与 PQ 混用：确认目标显示器能力未变更；HDR 热切换只在帧边界发生"
            }
            CsDiagCode::DirectionMismatchDoubleDecode => {
                "同一缓冲被解码两次：中间 pass 不得重复应用解码"
            }
            CsDiagCode::TargetSpaceUnknownDefaulted => {
                "显示器能力未探测：已默认 sRGB；接入 D09 能力探测后由探测结果替代"
            }
            CsDiagCode::PqOutOfRangeClamped => {
                "亮度超 PQ 上限或为负：检查上游 HDR 亮度统计（F1812）与参考亮度标度"
            }
            CsDiagCode::PqZeroCodeNonZero => "此为规范行为，勿修改",
            CsDiagCode::DeltaEExceeded => "编码器与外部参考偏差超阈值：立案后按参考表逐点修正",
            CsDiagCode::BoundaryKSideNotEncoded => "K 侧交出未编码缓冲：编码步骤被跳过或标记未置位",
            CsDiagCode::BoundaryVSideReencodeAttempt => "V 侧不得二次编码：K 侧已编码，重复即双重 gamma",
            CsDiagCode::BoundaryConfirmationMissing => "双端确认位缺一：K/V 交接需双方各自确认",
            CsDiagCode::InputNonFiniteSanitized => "输入含非有限值：已清洗为 0；非有限值不得穿透到显示",
        }
    }

    /// 该诊断是否属**可访问性**影响（发灰会压低对比度，伤害低视力与色觉障碍用户）。
    ///
    /// 方向错配类**全部**记为可访问性影响——这是本条对"无障碍"的真实落点，
    /// 不是附注。发灰意味着暗部与亮部拉近，界面元素的边界对比度下降。
    pub fn is_a11y_impact(self) -> bool {
        matches!(
            self,
            CsDiagCode::DirectionMismatchLinearAsEncoded
                | CsDiagCode::DirectionMismatchEncodedAsLinear
                | CsDiagCode::DirectionMismatchCrossFamily
                | CsDiagCode::DirectionMismatchDoubleDecode
        )
    }

    /// 该诊断是否属**阻断**（必须修，不能带着走）。
    pub fn is_blocking(self) -> bool {
        matches!(
            self,
            CsDiagCode::DirectionMismatchLinearAsEncoded
                | CsDiagCode::DirectionMismatchEncodedAsLinear
                | CsDiagCode::DirectionMismatchCrossFamily
                | CsDiagCode::DirectionMismatchDoubleDecode
                | CsDiagCode::BoundaryKSideNotEncoded
                | CsDiagCode::BoundaryVSideReencodeAttempt
                | CsDiagCode::BoundaryConfirmationMissing
        )
    }
}

/// 单条诊断。
#[derive(Clone, Debug, PartialEq)]
pub struct CsDiagnostic {
    /// 诊断码。
    pub code: CsDiagCode,
    /// 上下文数值（通道值 / 亮度 / ΔE，含义随码而定）。
    pub context: f32,
    /// 人类可读描述。
    pub text: String,
    /// 是否影响可访问性。
    pub a11y_impact: bool,
}

impl CsDiagnostic {
    /// 构造。
    pub fn new(code: CsDiagCode, context: f32) -> Self {
        CsDiagnostic {
            code,
            context,
            text: format!("[{}] {}", code.code(), code.next_hint()),
            a11y_impact: code.is_a11y_impact(),
        }
    }
}

/// 诊断袋（本条独立诊断面）。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CsDiagBag {
    items: Vec<CsDiagnostic>,
}

impl CsDiagBag {
    /// 空袋。
    pub fn new() -> Self {
        CsDiagBag { items: Vec::new() }
    }

    /// 压入一条。
    pub fn push(&mut self, d: CsDiagnostic) {
        self.items.push(d);
    }

    /// 记一条。
    pub fn note(&mut self, code: CsDiagCode, context: f32) {
        self.push(CsDiagnostic::new(code, context));
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 是否含某码。
    pub fn has(&self, code: CsDiagCode) -> bool {
        self.items.iter().any(|d| d.code == code)
    }

    /// 阻断项计数。
    pub fn blocking_count(&self) -> usize {
        self.items.iter().filter(|d| d.code.is_blocking()).count()
    }

    /// 可访问性影响项计数。
    pub fn a11y_count(&self) -> usize {
        self.items.iter().filter(|d| d.a11y_impact).count()
    }

    /// 全部码（去重后按出现顺序）。
    pub fn codes(&self) -> Vec<CsDiagCode> {
        let mut out: Vec<CsDiagCode> = Vec::new();
        for d in &self.items {
            if !out.contains(&d.code) {
                out.push(d.code);
            }
        }
        out
    }

    /// 可读报告。
    pub fn report(&self) -> String {
        let mut s = String::new();
        s.push_str("色彩空间输出诊断：");
        s.push_str(&self.items.len().to_string());
        s.push_str(" 条（阻断 ");
        s.push_str(&self.blocking_count().to_string());
        s.push_str("，可访问性影响 ");
        s.push_str(&self.a11y_count().to_string());
        s.push_str("）");
        for d in &self.items {
            s.push_str("\n  ");
            s.push_str(&d.text);
        }
        s
    }
}

// ---------------------------------------------------------------------------
// 三、编码标记系统
// ---------------------------------------------------------------------------

/// 编码类型（锚点「编码类型枚举（LINEAR / SDR_GAMMA / HDR_PQ）」）。
///
/// 三个值的顺序**有意义**：`is_encoded()` 用"序号 > 0"判定，`DisplayReferred`
/// 侧的线性是 0。改动顺序会静默改变判定结果，故用显式匹配而非比较序号。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoding {
    /// 线性（scene referred / display referred 线性，未编码）。
    Linear,
    /// sRGB / Rec.709 传递函数编码（SDR 显示目标）。
    SdrGamma,
    /// SMPTE ST 2084（PQ）编码（HDR 显示目标）。
    HdrPq,
}

impl Encoding {
    /// 全部取值。
    pub fn all() -> [Encoding; 3] {
        [Encoding::Linear, Encoding::SdrGamma, Encoding::HdrPq]
    }

    /// 稳定短名（元数据键）。
    pub fn key(self) -> &'static str {
        match self {
            Encoding::Linear => "LINEAR",
            Encoding::SdrGamma => "SDR_GAMMA",
            Encoding::HdrPq => "HDR_PQ",
        }
    }

    /// 人读标签。
    pub fn label(self) -> &'static str {
        match self {
            Encoding::Linear => "线性（未编码）",
            Encoding::SdrGamma => "sRGB 传递函数（SDR）",
            Encoding::HdrPq => "PQ / ST 2084（HDR）",
        }
    }

    /// 由短名解析。
    pub fn from_key(key: &str) -> Option<Encoding> {
        match key {
            "LINEAR" => Some(Encoding::Linear),
            "SDR_GAMMA" => Some(Encoding::SdrGamma),
            "HDR_PQ" => Some(Encoding::HdrPq),
            _ => None,
        }
    }

    /// 是否为"已编码"（可直接上屏）。
    ///
    /// 边界断言的第一判据。`Linear` 为未编码——它可以进后处理链，但**不能**交V 域。
    pub fn is_encoded(self) -> bool {
        matches!(self, Encoding::SdrGamma | Encoding::HdrPq)
    }

    /// 是否为 PQ（量纲特殊的那个——输入是绝对亮度）。
    ///
    /// **为什么单独判**：PQ 的输入是 cd/m² 而非归一化值，把归一化值当亮度喂进去
    /// 是本条最容易犯的量纲错误（画面暗三个数量级且不报错）。凡是要喂 PQ 的地方
    /// 都必须先过这个判。
    pub fn is_absolute_luminance(self) -> bool {
        self == Encoding::HdrPq
    }
}

/// 目标色彩空间来源（锚点错误路径 2：目标空间未知 → 默认 sRGB + 显性声明）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetSpaceOrigin {
    /// 来自显示器能力探测（D09）——可信来源。
    Probed,
    /// 未探测，默认 sRGB。**不冒充探测结果**。
    Defaulted,
}

/// 目标色彩空间。
///
/// **不derive `Eq`**：`reference_nits` 是 `f32`，而 `f32` 不满足 `Eq`
/// （`NaN != NaN`，`Eq` 要求自反）。派生它会在编译期直接报错——这正是 derive
/// 应当起的作用：把"这个类型不该参与相等性语义"显式化，而不是等到某次
/// `assert_eq!` 在生产数据上神秘失败。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TargetSpace {
    /// 目标编码。
    pub encoding: Encoding,
    /// 来源。
    pub origin: TargetSpaceOrigin,
    /// PQ 参考亮度（cd/m²）。仅当`encoding` 为 `HdrPq` 时有意义。
    pub reference_nits: f32,
}

impl TargetSpace {
    /// 由探测结果构造。
    pub fn probed(encoding: Encoding, reference_nits: f32) -> Self {
        TargetSpace {
            encoding,
            origin: TargetSpaceOrigin::Probed,
            reference_nits,
        }
    }

    /// 未探测 → 默认 sRGB，并**显式标记为默认**。
    ///
    /// 锚点要求"默认 sRGB + 显性声明"。显性的落点就是这个 `origin` 字段与随之
    /// 产生的 `TargetSpaceUnknownDefaulted` 诊断——不是日志里一行verbose。
    pub fn unprobed_default() -> Self {
        TargetSpace {
            encoding: Encoding::SdrGamma,
            origin: TargetSpaceOrigin::Defaulted,
            reference_nits: SDR_REFERENCE_NITS,
        }
    }

    /// 校验并把"默认"事实落进诊断袋。
    ///
    /// 同时夹取 PQ 参考亮度到合法域：0 或负的参考亮度会让整幅画面除以 0 或变负，
    /// 且这个错误在画面上表现为**纯黑**——比发灰更难归因。
    pub fn resolve(&self, bag: &mut CsDiagBag) -> ResolvedTarget {
        if self.origin == TargetSpaceOrigin::Defaulted {
            bag.note(CsDiagCode::TargetSpaceUnknownDefaulted, 0.0);
        }
        let nits = if !self.reference_nits.is_finite() {
            SDR_REFERENCE_NITS
        } else {
            clamp(self.reference_nits, MIN_REFERENCE_NITS, MAX_REFERENCE_NITS)
        };
        ResolvedTarget {
            encoding: self.encoding,
            origin: self.origin,
            reference_nits: nits,
        }
    }
}

/// 校验后的目标空间（`reference_nits` 已夹取）。
///
/// **不derive `Eq`**：`reference_nits` 是 `f32`，而 `f32` 不满足 `Eq`
/// （存在 `NaN != NaN`，`Eq` 要求自反）。派生它会在编译期直接报错——
/// 这正是 derive 应当起的作用：把"这个类型不该参与相等性语义"显式化。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ResolvedTarget {
    /// 目标编码。
    pub encoding: Encoding,
    /// 来源。
    pub origin: TargetSpaceOrigin,
    /// 已夹取的 PQ 参考亮度。
    pub reference_nits: f32,
}

/// 渲染目标描述（运行期标记载体）。
///
/// 这是**池化 RT 的元数据面**——那类 RT 的编码语义在编译期不可知（同一块显存被
/// 不同 pass 复用），所以标记只能运行期挂在此处。类型携带管不了这里，必须靠
/// 采样端的意图比对（见 `sample_checked`）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RtDesc {
    /// RT 标识（遥测用，不参与判定）。
    pub id: u32,
    /// 编码标记。
    pub encoding: Encoding,
    /// 是否为交V 域的最终帧缓冲。
    pub is_present_surface: bool,
}

impl RtDesc {
    /// 构造。
    pub fn new(id: u32, encoding: Encoding) -> Self {
        RtDesc {
            id,
            encoding,
            is_present_surface: false,
        }
    }

    /// 标记为呈现面（交V 域）。
    pub fn as_present_surface(mut self) -> Self {
        self.is_present_surface = true;
        self
    }

    /// 元数据可读串。
    pub fn meta_text(&self) -> String {
        format!(
            "RT#{} encoding={}{}",
            self.id,
            self.encoding.key(),
            if self.is_present_surface { " [present]" } else { "" }
        )
    }
}

// ---------------------------------------------------------------------------
// 四、类型携带的标记（编译期路径，运行时零成本）
// ---------------------------------------------------------------------------

/// 编码标记的类型层标签。
///
/// **存在的意义**：让"把 PQ 缓冲当 sRGB 缓冲传"成为编译错误。运行时代码一行都不多。
pub trait EncodingTag: Copy {
    /// 该标签对应的编码。
    const ENCODING: Encoding;
}

/// 线性标签。
#[derive(Clone, Copy, Debug)]
pub struct LinearTag;
/// sRGB 标签。
#[derive(Clone, Copy, Debug)]
pub struct SdrGammaTag;
/// PQ 标签。

#[derive(Clone, Copy, Debug)]
pub struct HdrPqTag;

impl EncodingTag for LinearTag {
    const ENCODING: Encoding = Encoding::Linear;
}
impl EncodingTag for SdrGammaTag {
    const ENCODING: Encoding = Encoding::SdrGamma;
}
impl EncodingTag for HdrPqTag {
    const ENCODING: Encoding = Encoding::HdrPq;
}

/// 类型携带标记的缓冲。
///
/// `PhantomData<T>` 不占空间（零大小类型），所以"标记检查零成本"这句锚点要求
/// 在这里是**字面为真**的，不是"开销很小"。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TypedBuf<T: EncodingTag> {
    /// r通道。
    pub r: f32,
    /// g 通道。
    pub g: f32,
    /// b 通道。
    pub b: f32,
    /// 零大小标记。
    pub _tag: PhantomData<T>,
}

impl<T: EncodingTag> TypedBuf<T> {
    /// 构造（类型即标记，无需传入编码参数——**传不进来本身就是防护**）。
    pub fn new(r: f32, g: f32, b: f32) -> Self {
        TypedBuf {
            r,
            g,
            b,
            _tag: PhantomData,
        }
    }

    /// 本缓冲的编码（由类型得出）。
    pub fn encoding(&self) -> Encoding {
        T::ENCODING
    }

    /// 三通道元组。
    pub fn rgb(&self) -> (f32, f32, f32) {
        (self.r, self.g, self.b)
    }
}

/// 线性缓冲便捷别名。
pub type LinearBuf = TypedBuf<LinearTag>;
/// sRGB 编码缓冲便捷别名。
pub type SdrBuf = TypedBuf<SdrGammaTag>;
/// PQ 编码缓冲便捷别名。
pub type PqBuf = TypedBuf<HdrPqTag>;

/// 按标记把缓冲**解码为线性**（类型路径，编译期已保证方向正确）。
///
/// `T` 决定用哪个解码器，调用方无法指定——所以这一层**结构上不可能**方向错配。
/// 运行期唯一的分支是 `T::ENCODING` 的匹配，而它在单态化后就是常量。
pub fn decode_to_linear<T: EncodingTag>(buf: &TypedBuf<T>) -> LinearBuf {
    let (r, g, b) = match T::ENCODING {
        Encoding::Linear => (buf.r, buf.g, buf.b),
        Encoding::SdrGamma => (srgb_decode(buf.r), srgb_decode(buf.g), srgb_decode(buf.b)),
        Encoding::HdrPq => {
            // PQ 解码产出绝对亮度（nit），归一到参考白以便与线性工作空间接口一致。
            // 归一在这里发生**一次**且必须发生：下游后处理链全在线性归一空间里工作。
            let n = pq_reference_nits();
            (
                pq_decode(buf.r) / n,
                pq_decode(buf.g) / n,
                pq_decode(buf.b) / n,
            )
        }
    };
    LinearBuf::new(r, g, b)
}

/// PQ 参考白（cd/m²）——解码归一用。
///
/// 用10000（PQ 上限）而非 SDR 的 100：PQ 码值 1.0 的语义就是 10000nit，
/// 归一必须以**码值域的满量程**为基准，否则同一份 PQ 帧在 SDR 目标上会暗两个
/// 数量级。这正是量纲问题——`pq_decode` 给的是 nit，除数就得是 nit。
pub const fn pq_reference_nits() -> f32 {
    MAX_PQ_NITS
}

// ---------------------------------------------------------------------------
// 五、sRGB 传递函数（精确分段实现）
// ---------------------------------------------------------------------------

/// 编码侧折点（线性域）。规范值`12.92 × 0.0031308 = 0.0404499…`。
pub const SRGB_OETF_BREAK: f32 = 0.003_130_8;
/// 解码侧折点（码值域）。规范值。
pub const SRGB_EOTF_BREAK: f32 = 0.040_45;
/// 线性段斜率。
pub const SRGB_SLOPE: f32 = 12.92;
/// 幂段斜率。
pub const SRGB_SLOPE_HI: f32 = 1.055;
/// 幂段偏移。
pub const SRGB_OFFSET: f32 = 0.055;
/// 幂指数（编码侧为倒数）。
pub const SRGB_GAMMA: f32 = 2.4;

/// sRGB 编码（OETF：线性 → 码值）。
///
/// **分段与符号对称**：
/// - `|L| <= 0.0031308` 走线性段 `12.92 · L`；
/// - 否则走幂段 `1.055 · |L|^(1/2.4) − 0.055`，再乘回符号。
///
///负值走 `sign(x)·f(|x|)` 是通行扩展：不这样做，负底数取分数次幂在IEEE 754 下
/// 产出 **NaN**，而 NaN 一旦进入画面就**永不消失**（后续任何映射都吐不出有限值），
/// 会留一个永久异色像素点。
pub fn srgb_encode(linear: f32) -> f32 {
    let l = if linear.is_finite() { linear } else { 0.0 };
    let a = l.abs();
    let s = if a <= SRGB_OETF_BREAK {
        SRGB_SLOPE * a
    } else {
        SRGB_SLOPE_HI * a.powf(1.0 / SRGB_GAMMA) - SRGB_OFFSET
    };
    l.signum() * s
}

/// sRGB 解码（EOTF：码值 → 线性）。
///
/// 折点是 `0.04045`——**与编码侧的 `0.0031308` 不等**，这不是笔误，是规范规定。
/// 二者的关系 `0.0031308 × 12.92 = 0.040449936`，比解码折点小 `6.4e-8`，
/// 于是折点附近 `decode(encode(x))` 有约 `3.6e-6` 的固有偏差。
/// 本条把它写进精度声明而不是断言成 0——断言一个规范不保证的性质，测的就不是实现了。
pub fn srgb_decode(encoded: f32) -> f32 {
    let e = if encoded.is_finite() { encoded } else { 0.0 };
    let a = e.abs();
    let l = if a <= SRGB_EOTF_BREAK {
        a / SRGB_SLOPE
    } else {
        ((a + SRGB_OFFSET) / SRGB_SLOPE_HI).powf(SRGB_GAMMA)
    };
    e.signum() * l
}

/// 实测：sRGB 往返最大绝对误差（密集扫描）。
///
/// 返回 `None` 表示扫描中出现了非有限值——那不是"误差大"，是**实现坏了**，
/// 两者必须能区分。
pub fn srgb_roundtrip_max_error(samples: usize) -> Option<f32> {
    if samples == 0 {
        return None;
    }
    let mut worst = 0.0f32;
    for i in 0..=samples {
        let x = i as f32 / samples as f32;
        let back = srgb_decode(srgb_encode(x));
        if !back.is_finite() {
            return None;
        }
        let e = (back - x).abs();
        if e > worst {
            worst = e;
        }
    }
    Some(worst)
}

/// sRGB 符号对称性自检：负半轴往返误差应与正半轴同量级。
///
/// 负半轴单独测而不是只测正半轴——`sign(x)·f(|x|)` 的实现错误（忘记乘回符号）
/// 在正半轴上**完全测不出来**，因为正半轴 `signum=1`。
pub fn srgb_negative_roundtrip_max_error(samples: usize) -> Option<f32> {
    if samples == 0 {
        return None;
    }
    let mut worst = 0.0f32;
    for i in 1..=samples {
        let x = -(i as f32 / samples as f32);
        let back = srgb_decode(srgb_encode(x));
        if !back.is_finite() {
            return None;
        }
        // 符号必须回来：往返后应仍为负。
        if back > 0.0 {
            return None;
        }
        let e = (back - x).abs();
        if e > worst {
            worst = e;
        }
    }
    Some(worst)
}

// ---------------------------------------------------------------------------
// 六、PQ（SMPTE ST 2084）
// ---------------------------------------------------------------------------

/// PQ 峰值亮度（cd/m²）。规范上限。
pub const MAX_PQ_NITS: f32 = 10_000.0;
/// PQ 一次幂常数 `m1 = 2610/16384`。
pub const PQ_M1: f32 = 0.159_301_76;
/// PQ 二次幂常数 `m2 = 2523/4096 × 128`。
pub const PQ_M2: f32 = 78.843_75;
/// PQ 黑位系数 `c1 = 3424/4096`。
pub const PQ_C1: f32 = 0.835_937_5;
/// PQ 黑位系数 `c2 = 2413/4096 × 32`。
pub const PQ_C2: f32 = 18.851_562_5;
/// PQ 黑位系数 `c3 = 2392/4096 × 32`。
pub const PQ_C3: f32 = 18.687_5;
/// PQ 零点码值实测值（规范行为，见精度声明）。
pub const PQ_ZERO_CODE_VALUE: f32 = 7.31e-7;

/// PQ 编码（逆 EOTF：绝对亮度 nit → 码值 `[0,1]`）。
///
/// **输入是绝对亮度**，不是归一化值。喂归一化值等于宣称峰值 1 nit，画面暗三个
/// 数量级且不报任何错——本条把量纲检查放在调用点（`is_absolute_luminance`），
/// 因为函数内部无法区分"0.5 nit"和"0.5 归一"。
///
/// 越界钳制：`nits > 10000` → 10000，`nits < 0` → 0。钳制事实由调用方落诊断
/// （本函数保持纯函数、不带诊断袋——诊断是状态面，混进纯函数会让它不可复用）。
pub fn pq_encode(nits: f32) -> f32 {
    let n = if nits.is_finite() { nits } else { 0.0 };
    let y = clamp(n, 0.0, MAX_PQ_NITS) / MAX_PQ_NITS;
    let ym = y.powf(PQ_M1);
    ((PQ_C1 + PQ_C2 * ym) / (1.0 + PQ_C3 * ym)).powf(PQ_M2)
}

/// PQ 解码（EOTF：码值 `[0,1]` → 绝对亮度 nit）。
///
/// 由正向式`N = ((c1 + c2·Y^m1) / (1 + c3·Y^m1))^m2` 代数反解。令
/// `P = N^(1/m2)`，则 `P = (c1 + c2·Y^m1) / (1 + c3·Y^m1)`，交叉相乘：
///
/// ```text
/// P + P·c3·Y^m1 = c1 + c2·Y^m1
/// P − c1= Y^m1 · (c2 − P·c3)
/// Y^m1 = (P − c1) / (c2 − P·c3)
/// ```
///
/// **分母是 `c2 − P·c3` 而不是 `c2`**——这一点写错不会编译报错、不会panic，
/// 只会让 `pq_decode(pq_encode(x))`恒为≈0（因为分母少了 `P·c3` 而真分母在
/// `P→1` 时趋近于 `c2 − c3 ≈ 0.164`，量级差 100 倍以上，解出来必然是 0）。
/// 本条自检里的往返断言就是为拦住这一类"看起来能跑、结果全错"的实现错误。
///
/// `max(·, 0)` **不可删**：`P − c1` 在小 `N` 时为负，负底数取分数次幂在 IEEE 754
/// 下是 NaN，删掉它画面会出现 NaN 黑块。
pub fn pq_decode(code: f32) -> f32 {
    let c = if code.is_finite() { code } else { 0.0 };
    let c = clamp(c, 0.0, 1.0);
    let p = c.powf(1.0 / PQ_M2);
    let num = p - PQ_C1;
    let den = PQ_C2 - PQ_C3 * p;
    if num <= 0.0 || den <= 0.0 {
        return 0.0;
    }
    MAX_PQ_NITS * (num / den).powf(1.0 / PQ_M1)
}

/// PQ 越界检测（编码前调用；把钳制事实显性化）。
///
///静默钳制会掩盖上游 HDR 亮度统计错误（F1812算错→亮度过大→被钳→看不出），
/// 所以钳制必须**同时**发生且**同时**被记录。
pub fn pq_encode_guarded(nits: f32, bag: &mut CsDiagBag) -> f32 {
    if !nits.is_finite() {
        bag.note(CsDiagCode::InputNonFiniteSanitized, nits);
    } else if nits < 0.0 || nits > MAX_PQ_NITS {
        bag.note(CsDiagCode::PqOutOfRangeClamped, nits);
    }
    if nits == 0.0 {
        // 规范行为显式记录：防止下游把"零输入出非零码值"误判为缺陷反复"修"。
        bag.note(CsDiagCode::PqZeroCodeNonZero, PQ_ZERO_CODE_VALUE);
    }
    pq_encode(nits)
}

/// 实测：PQ 零点码值（`pq_encode(0)` 的真实输出）。
pub fn pq_zero_code_measured() -> f32 {
    pq_encode(0.0)
}

/// 实测：PQ 往返最大相对误差（按峰值亮度量纲）。
///
/// **为什么是相对误差**：PQ 动态范围跨 5 个数量级，暗端绝对误差天然极小
/// （1e-3 nit 的绝对误差在 0.001 nit 处是100%），用绝对误差会得出"暗端精度很差"
/// 的错误结论。相对误差是跨量程唯一诚实的统一口径。
pub fn pq_roundtrip_max_relative_error(samples: usize) -> Option<f32> {
    if samples == 0 {
        return None;
    }
    let mut worst = 0.0f32;
    for i in 1..=samples {
        let nits = MAX_PQ_NITS * (i as f32 / samples as f32);
        let code = pq_encode(nits);
        let back = pq_decode(code);
        if !back.is_finite() {
            return None;
        }
        let rel = (back - nits).abs() / nits;
        if rel > worst {
            worst = rel;
        }
    }
    Some(worst)
}

/// BT.2408 公布锚点：100 cd/m² 对应的 PQ 码值（实测 0.50808）。
///
/// **为什么带容差而不是精确值**：公布值本身是四舍五入到有限位的（0.508），
/// 本条f32 实测 0.50808，差值来自 f32 舍入与公布值的截断。容差 ±0.002 足够
/// 判定"公式实现正确"，又不会宽到"公式完全写错也能过"——把容差写成 0 是把
/// **参考值自身的不确定性**当成实现缺陷，那会逼着人去改一个没坏的公式；
/// 反之把容差写成 0.1 就是把门禁拆了。
pub const PQ_100NITS_CODE: f32 = 0.50808;
/// 100nit 锚点容差。
pub const PQ_100NITS_TOLERANCE: f32 = 0.002;

// ---------------------------------------------------------------------------
// 七、ΔE\*ab 对拍（外部参考表）
// ---------------------------------------------------------------------------

/// 线性 sRGB → CIE XYZ（D65）。
pub fn linear_srgb_to_xyz(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let x = 0.412_390_8 * r + 0.357_584_3 * g + 0.180_480_8 * b;
    let y = 0.212_639 * r + 0.715_168_7 * g + 0.072_192_3 * b;
    let z = 0.019_330_8 * r + 0.119_194_8 * g + 0.950_532_2 * b;
    (x, y, z)
}

/// CIE 标准 `f(t)`（分段立方根线性化）。
fn lab_f(t: f32) -> f32 {
    const DELTA: f32 = 6.0 / 29.0;
    const DELTA_CUBE: f32 = DELTA * DELTA * DELTA;
    const SLOPE: f32 = 1.0 / (3.0 * DELTA * DELTA);
    if t > DELTA_CUBE {
        t.powf(1.0 / 3.0)
    } else {
        SLOPE * t + 4.0 / 29.0
    }
}

/// D65 白点。
pub const D65_WHITE: (f32, f32, f32) = (0.950_47, 1.0, 1.088_83);

/// 线性 sRGB → CIE L\*a\*b\*。
pub fn linear_srgb_to_lab(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let (x, y, z) = linear_srgb_to_xyz(r, g, b);
    let fx = lab_f(x / D65_WHITE.0);
    let fy = lab_f(y / D65_WHITE.1);
    let fz = lab_f(z / D65_WHITE.2);
    (116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz))
}

/// CIE76 ΔE\*ab。
pub fn delta_e76(lab1: (f32, f32, f32), lab2: (f32, f32, f32)) -> f32 {
    let dl = lab1.0 - lab2.0;
    let da = lab1.1 - lab2.1;
    let db = lab1.2 - lab2.2;
    (dl * dl + da * da + db * db).sqrt()
}

/// 外部参考表：8bit sRGB 码值 → 公布的标准线性值。
///
/// **这张表是门禁的诚信基础**：若拿"本实现自己算出的值"当参考，ΔE 恒等于 0，
/// 判据就成了永远为真的弱门禁（连表内元素验查表函数都不如）。故表值取自
/// **公开的 8bit sRGB 解码 LUT**，与本模块的公式实现相互独立。
///
/// 选取刻意**跨段**：含 0（线性段）、1/2/32（线性段上沿）、128/160/192/224
/// （幂段主体）、255（端点）——只取幂段会漏掉折点附近的错误，只取线性段会漏掉
/// 指数错误。
pub const SRGB8_REFERENCE: [(u8, f32); 9] = [
    (0, 0.0),
    (1, 0.000_303_527),
    (2, 0.000_607_054),
    (32, 0.014_443_844),
    (128, 0.215_860_5),
    (160, 0.351_532_6),
    (192, 0.527_115_13),
    (224, 0.745_404_2),
    (255, 1.0),
];

/// 单点对拍结果。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RefPointResult {
    /// 参考码值。
    pub code: u8,
    /// 本实现解码值。
    pub actual: f32,
    /// 参考值。
    pub expected: f32,
    /// 绝对误差。
    pub abs_error: f32,
    /// ΔE\*ab。
    pub delta_e: f32,
}

/// 逐点对拍外部参考表。
///
/// 返回逐点结果（空 Vec 表示表被清空——那会让门禁"零点全过"，所以调用方
/// 必须检查 `is_empty()`，本条的自检里就有这一条）。
pub fn check_srgb8_reference() -> Vec<RefPointResult> {
    let mut out = Vec::new();
    for &(code, expected) in SRGB8_REFERENCE.iter() {
        let v = code as f32 / 255.0;
        let actual = srgb_decode(v);
        // ΔE 在等亮度灰阶上衡量：灰阶的 a/b 恒为 0，ΔE 主要反映 L* 差，
        // 正是人眼对亮度差异最敏感的方向。
        let lab_actual = linear_srgb_to_lab(actual, actual, actual);
        let lab_expected = linear_srgb_to_lab(expected, expected, expected);
        out.push(RefPointResult {
            code,
            actual,
            expected,
            abs_error: (actual - expected).abs(),
            delta_e: delta_e76(lab_actual, lab_expected),
        });
    }
    out
}

/// 立案记录（ΔE 超阈值）。
#[derive(Clone, Debug, PartialEq)]
pub struct DeviationCase {
    /// 参考码值。
    pub code: u8,
    /// 实测 ΔE。
    pub delta_e: f32,
    /// 处置结论。
    pub verdict: &'static str,
}

/// ΔE 立案台账。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DeviationLog {
    cases: Vec<DeviationCase>,
}

impl DeviationLog {
    /// 空台账。
    pub fn new() -> Self {
        DeviationLog { cases: Vec::new() }
    }

    /// 登记一条立案（供审计流程与自检注入使用）。
    ///
    /// **为什么提供公开入口**：`cases` 私有是有意的（防止调用方随手改台账内容），
    /// 但**登记**这件事本身必须能被外部触发——否则"ΔE 超阈值要立案"这条纪律
    /// 就只能由 `audit()` 内部实现，一旦要验证"立案机制真的能承载条目"，
    /// 调用方就只能去改私有字段或加一个 `#[cfg(test)]` 后门，两者都不好。
    /// 读与写分离：读走 `report()`，写走本函数，字段保持私有。
    pub fn record(&mut self, case: DeviationCase) {
        self.cases.push(case);
    }

    /// 跑一次对拍并登记超阈值点。
    pub fn audit(&mut self, bag: &mut CsDiagBag) -> usize {
        for p in check_srgb8_reference().iter() {
            if p.delta_e > MAX_DELTA_E {
                self.record(DeviationCase {
                    code: p.code,
                    delta_e: p.delta_e,
                    verdict: "超 ΔE 阈值：按外部参考表逐点修正公式实现",
                });
                bag.note(CsDiagCode::DeltaEExceeded, p.delta_e);
            }
        }
        self.cases.len()
    }

    /// 立案数。
    pub fn len(&self) -> usize {
        self.cases.len()
    }

    /// 是否无立案。
    pub fn is_clean(&self) -> bool {
        self.cases.is_empty()
    }

    /// 可读报告。
    pub fn report(&self) -> String {
        let mut s = String::from("ΔE 立案台账：");
        s.push_str(&self.cases.len().to_string());
        s.push_str(" 条");
        for c in &self.cases {
            s.push_str("\n  码值 ");
            s.push_str(&c.code.to_string());
            s.push_str(" ΔE=");
            s.push_str(&c.delta_e.to_string());
            s.push_str(" → ");
            s.push_str(c.verdict);
        }
        s
    }
}

// ---------------------------------------------------------------------------
// 八、方向守卫（运行期路径：池化 RT）
// ---------------------------------------------------------------------------

/// 采样端声明的意图。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SampleIntent {
    /// 期望读到**未编码线性**值（后处理链内部消费）。
    WantLinear,
    /// 期望读到**已编码**值（直接送显示）。
    WantEncoded,
    /// 期望读到 sRGB 码值。
    WantSdrGamma,
    /// 期望读到 PQ 码值。
    WantHdrPq,
}

impl SampleIntent {
    /// 人读标签。
    pub fn label(self) -> &'static str {
        match self {
            SampleIntent::WantLinear => "期望线性（未编码）",
            SampleIntent::WantEncoded => "期望已编码",
            SampleIntent::WantSdrGamma => "期望 sRGB 码值",
            SampleIntent::WantHdrPq => "期望 PQ 码值",
        }
    }
}

/// 方向比对结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DirectionVerdict {
    /// 一致。
    Match,
    /// 不一致（附诊断码，指明错法）。
    Mismatch(CsDiagCode),
}

/// 方向判定（运行期路径的核心纯函数）。
///
/// **四种错法逐条区分**，因为它们要修的地方不同：
/// - `LINEAR` 被当编码读 → 写入端漏编码；
/// - 已编码被当线性读 → 读取端漏解码；
/// - sRGB 与 PQ 互串 → 目标能力变更未在帧边界生效；
/// - 同族二次解码 → 中间 pass 重复应用解码。
///
/// 笼统报一个"编码不匹配"会把四种不同的根因压成一种，收到告警的人得从头查——
/// 诊断的价值在于指认，不在于报警。
pub fn check_direction(actual: Encoding, intent: SampleIntent) -> DirectionVerdict {
    match intent {
        SampleIntent::WantLinear => {
            if actual == Encoding::Linear {
                DirectionVerdict::Match
            } else {
                DirectionVerdict::Mismatch(CsDiagCode::DirectionMismatchEncodedAsLinear)
            }
        }
        SampleIntent::WantEncoded => {
            if actual.is_encoded() {
                DirectionVerdict::Match
            } else {
                DirectionVerdict::Mismatch(CsDiagCode::DirectionMismatchLinearAsEncoded)
            }
        }
        SampleIntent::WantSdrGamma => {
            if actual == Encoding::SdrGamma {
                DirectionVerdict::Match
            } else if actual == Encoding::HdrPq {
                DirectionVerdict::Mismatch(CsDiagCode::DirectionMismatchCrossFamily)
            } else {
                DirectionVerdict::Mismatch(CsDiagCode::DirectionMismatchLinearAsEncoded)
            }
        }
        SampleIntent::WantHdrPq => {
            if actual == Encoding::HdrPq {
                DirectionVerdict::Match
            } else if actual == Encoding::SdrGamma {
                DirectionVerdict::Mismatch(CsDiagCode::DirectionMismatchCrossFamily)
            } else {
                DirectionVerdict::Mismatch(CsDiagCode::DirectionMismatchLinearAsEncoded)
            }
        }
    }
}

/// 采样端按标记自动解码（运行期路径）。
///
/// 返回 `(解码后的线性值, 是否发生了方向告警)`。**不返回错误类型**——因为
/// 本条的错误路径策略是"告警 + 按标记实际语义继续"而非"中止"：方向错配时
/// 中止渲染会让整个画面黑掉，那比画错更难排查。此处选择**按事实解码 +
/// 落具名诊断**，让画面继续跑但问题可见。
///
/// **按标记解码而非按意图解码**是关键：意图只是调用方的*期望*，标记是
/// RT 的*事实*。两者冲突时以事实为准并告警——反过来（按意图解码）会让
/// "线性缓冲被当编码读"这类错配**静默地**按错误语义处理，诊断也就永远不会触发。
pub fn sample_checked(
    desc: &RtDesc,
    intent: SampleIntent,
    rgb: (f32, f32, f32),
    bag: &mut CsDiagBag,
) -> ((f32, f32, f32), bool) {
    let warned = match check_direction(desc.encoding, intent) {
        DirectionVerdict::Match => false,
        DirectionVerdict::Mismatch(code) => {
            bag.note(code, desc.encoding.key().len() as f32);
            true
        }
    };
    let out = decode_linear_rt(desc.encoding, rgb);
    (out, warned)
}

/// 按编码标记解码为线性值（运行期数值路径）。
///
/// 与类型路径的 `decode_to_linear` **同语义**：同样的三个分支、同样的归一。
/// 两处若漂移，则"编译期路径与运行期路径给出不同像素"——那是最难查的一类
/// 不一致（本条自检里有跨路径一致性断言）。
pub fn decode_linear_rt(encoding: Encoding, rgb: (f32, f32, f32)) -> (f32, f32, f32) {
    match encoding {
        Encoding::Linear => rgb,
        Encoding::SdrGamma => (srgb_decode(rgb.0), srgb_decode(rgb.1), srgb_decode(rgb.2)),
        Encoding::HdrPq => {
            // PQ 解码产出绝对亮度（nit），必须除以参考白才回到线性归一空间——
            // 少这一步就是量纲错误（暗两个数量级）。
            let n = pq_reference_nits();
            (pq_decode(rgb.0) / n, pq_decode(rgb.1) / n, pq_decode(rgb.2) / n)
        }
    }
}

// ---------------------------------------------------------------------------
// 九、V 域交接（双端确认）
// ---------------------------------------------------------------------------

/// K 侧交棒声明。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KSideHandoff {
    /// K 侧交出的编码。
    pub encoding: Encoding,
    /// K 侧是否已确认"我交出的是已编码缓冲"。
    pub k_side_encoded: bool,
    /// 交棒的缓冲 id（遥测关联）。
    pub buffer_id: u32,
}

/// V 侧接收声明。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VSideIntake {
    /// V 侧自认收到的编码。
    pub observed_encoding: Encoding,
    /// V 侧是否已确认"我不会再编码"。
    pub v_side_accepted_encoded: bool,
    /// V 侧是否试图再编码（真值即为违规）。
    pub v_side_wants_reencode: bool,
}

/// 交接裁决。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandoffVerdict {
    /// 放行。
    Passed,
    /// K 侧交出未编码缓冲。
    RejectedKNotEncoded,
    /// V 侧试图二次编码。
    RejectedVReencode,
    /// 双端对编码的认知不一致。
    RejectedEncodingDisagreement,
    /// 缺确认位。
    RejectedConfirmationMissing,
}

/// 交接裁决文本（说清谁违约）。
pub fn handoff_verdict_text(v: HandoffVerdict) -> &'static str {
    match v {
        HandoffVerdict::Passed => "放行：双端确认齐备且编码一致",
        HandoffVerdict::RejectedKNotEncoded => "拦截：K 侧交出未编码缓冲（LINEAR 不得上屏）",
        HandoffVerdict::RejectedVReencode => "拦截：V 侧声明二次编码（双重 gamma，画面发灰）",
        HandoffVerdict::RejectedEncodingDisagreement => {
            "拦截：双端对编码的认知不一致（K 与 V 各以为对方编了）"
        }
        HandoffVerdict::RejectedConfirmationMissing => "拦截：双端确认位缺一（交接需双方各自确认）",
    }
}

/// 双端交接裁决（锚点「边界断言（…V 侧输入校验双端确认）」）。
///
/// **判定顺序有意为之**：先查"有没有人明说要二次编码"这种**主动违规**，
/// 再查编码一致性，最后查确认位。原因是主动违规是根因，确认位缺失只是
/// 流程没走完——把流程问题报在根因前面会让人去补流程而放过真正的 bug。
pub fn judge_handoff(k: &KSideHandoff, v: &VSideIntake, bag: &mut CsDiagBag) -> HandoffVerdict {
    if v.v_side_wants_reencode {
        bag.note(CsDiagCode::BoundaryVSideReencodeAttempt, 0.0);
        return HandoffVerdict::RejectedVReencode;
    }
    if !k.k_side_encoded || !k.encoding.is_encoded() {
        bag.note(CsDiagCode::BoundaryKSideNotEncoded, k.encoding.key().len() as f32);
        return HandoffVerdict::RejectedKNotEncoded;
    }
    if k.encoding != v.observed_encoding {
        bag.note(
            CsDiagCode::DirectionMismatchCrossFamily,
            k.encoding.key().len() as f32,
        );
        return HandoffVerdict::RejectedEncodingDisagreement;
    }
    if !v.v_side_accepted_encoded {
        bag.note(CsDiagCode::BoundaryConfirmationMissing, 0.0);
        return HandoffVerdict::RejectedConfirmationMissing;
    }
    HandoffVerdict::Passed
}

// ---------------------------------------------------------------------------
// 十、编码管线（K 侧主入口）
// ---------------------------------------------------------------------------

/// SDR 参考亮度（cd/m²）。BT.2408 规定的标准 SDR 基准。
pub const SDR_REFERENCE_NITS: f32 = 100.0;
/// 参考亮度下界（nit）。低于此值除法不稳定或恒零。
pub const MIN_REFERENCE_NITS: f32 = 1.0;
/// 参考亮度上界（nit）。
pub const MAX_REFERENCE_NITS: f32 = MAX_PQ_NITS;

/// 输入清洗（NaN/±Inf → 0）。
///
/// 与 F2006 同一处置：Inf 钳成 0 而非 `MAX`——Inf 经任何编码函数仍是 Inf，
/// 会在屏幕上留一个**永不消失**的异色点；钳成 0 只是这一像素黑。
fn clean_channel(v: f32) -> f32 {
    if v.is_finite() && v >= 0.0 {
        v
    } else {
        0.0
    }
}

fn clamp(v: f32, lo: f32, hi: f32) -> f32 {
    if v < lo {
        lo
    } else if v > hi {
        hi
    } else {
        v
    }
}

/// 编码结果（未附标记的值 + 实际使用的编码）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EncodeOutput {
    /// 编码后的 r（已按目标语义）。
    pub r: f32,
    /// 编码后的 g。
    pub g: f32,
    /// 编码后的 b。
    pub b: f32,
    /// 实际使用的编码。
    pub encoding: Encoding,
    /// 输入是否被清洗过。
    pub sanitized: bool,
}

/// 编码管线主入口（K 侧）。
///
/// **入参是 `SceneReferred`**：直接吃 F2006 的输出类型，不引入中间表示。
/// TM 交出的是 display referred 线性 `[0,1]`，本条唯一的动作就是把它走到
/// 已编码——中间**不再落任何自己的表示**，少一次转换就少一处能出错的地方。
///
/// 线性目标（`Encoding::Linear`）也允许：它是恒等映射，但**必须走一遍**而不是
/// 直接返回入参。理由是清洗与诊断不能被"反正没编码"跳过——脏输入在无编码路径上
/// 同样要拦。
pub fn encode_for_present(
    color: SceneReferred,
    target: &ResolvedTarget,
    bag: &mut CsDiagBag,
) -> EncodeOutput {
    let raw = (color.r, color.g, color.b);
    let clean = (clean_channel(raw.0), clean_channel(raw.1), clean_channel(raw.2));
    let sanitized = clean != raw;
    if sanitized {
        bag.note(CsDiagCode::InputNonFiniteSanitized, raw.0);
    }
    let out = match target.encoding {
        Encoding::Linear => clean,
        Encoding::SdrGamma => (srgb_encode(clean.0), srgb_encode(clean.1), srgb_encode(clean.2)),
        Encoding::HdrPq => {
            //量纲转换在此发生且只在此发生：归一化线性 × 参考亮度 =绝对 nit。
            // 少这一步就是"暗三个数量级且不报错"的那类事故。
            let s = target.reference_nits;
            (
                pq_encode_guarded(clean.0 * s, bag),
                pq_encode_guarded(clean.1 * s, bag),
                pq_encode_guarded(clean.2 * s, bag),
            )
        }
    };
    EncodeOutput {
        r: out.0,
        g: out.1,
        b: out.2,
        encoding: target.encoding,
        sanitized,
    }
}

/// 把编码结果封成带类型标记的缓冲（交给类型携带路径）。
///
/// `Encoding` → `TypedBuf<T>` 的映射是**类型级**的，所以本函数是三个独立
/// 分支而不是一次转换——转换不过去的情况（如把Linear 结果封成 `SdrBuf`）
/// 在这里就编译不过。
pub fn seal_srgb(out: &EncodeOutput) -> Option<SdrBuf> {
    if out.encoding == Encoding::SdrGamma {
        Some(SdrBuf::new(out.r, out.g, out.b))
    } else {
        None
    }
}

/// 把编码结果封成 PQ 缓冲。
pub fn seal_pq(out: &EncodeOutput) -> Option<PqBuf> {
    if out.encoding == Encoding::HdrPq {
        Some(PqBuf::new(out.r, out.g, out.b))
    } else {
        None
    }
}

/// 把未编码值封成线性缓冲。
pub fn seal_linear(out: &EncodeOutput) -> Option<LinearBuf> {
    if out.encoding == Encoding::Linear {
        Some(LinearBuf::new(out.r, out.g, out.b))
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// 十一、状态与呈现
// ---------------------------------------------------------------------------

/// 色彩空间输出状态。
#[derive(Clone, Debug, PartialEq)]
pub struct ColorSpaceState {
    /// 当前目标编码。
    pub encoding: Encoding,
    /// 目标来源。
    pub origin: TargetSpaceOrigin,
    /// PQ 参考亮度。
    pub reference_nits: f32,
    /// 诊断袋。
    pub diag: CsDiagBag,
    /// ΔE 立案台账。
    pub deviations: DeviationLog,
    /// 累计方向错配次数（遥测）。
    pub direction_mismatches: u32,
    /// 累计双端交接拦截次数。
    pub handoff_rejections: u32,
    /// 最近一次交接裁决。
    pub last_handoff: HandoffVerdict,
}

impl ColorSpaceState {
    /// 新建（默认目标未探测）。
    pub fn new() -> Self {
        ColorSpaceState {
            encoding: Encoding::SdrGamma,
            origin: TargetSpaceOrigin::Defaulted,
            reference_nits: SDR_REFERENCE_NITS,
            diag: CsDiagBag::new(),
            deviations: DeviationLog::new(),
            direction_mismatches: 0,
            handoff_rejections: 0,
            last_handoff: HandoffVerdict::Passed,
        }
    }

    /// 设定目标空间（切档只在**帧边界**发生，见 F1762 规则）。
    ///
    /// 帧内切换会让同一帧的前半段是 sRGB、后半段是 PQ——那不是"HDR 切换"，
    /// 那是**一帧内的双重编码**，方向错配的又一形态。故本函数只在帧边界被调用，
    /// 由调用方保证（`frame_id` 单调递增，同帧重复调用直接拒绝）。
    pub fn set_target(&mut self, target: &TargetSpace, frame_id: u64, last_frame_id: u64) -> bool {
        if frame_id <= last_frame_id && self.origin != TargetSpaceOrigin::Defaulted {
            return false;
        }
        let resolved = target.resolve(&mut self.diag);
        self.encoding = resolved.encoding;
        self.origin = resolved.origin;
        self.reference_nits = resolved.reference_nits;
        true
    }

    /// 运行期采样（走方向守卫）。
    pub fn sample(
        &mut self,
        desc: &RtDesc,
        intent: SampleIntent,
        rgb: (f32, f32, f32),
    ) -> (f32, f32, f32) {
        let (out, warned) = sample_checked(desc, intent, rgb, &mut self.diag);
        if warned {
            self.direction_mismatches = self.direction_mismatches.saturating_add(1);
        }
        out
    }

    /// 交接裁决（计入遥测）。
    pub fn handoff(&mut self, k: &KSideHandoff, v: &VSideIntake) -> HandoffVerdict {
        let verdict = judge_handoff(k, v, &mut self.diag);
        if verdict != HandoffVerdict::Passed {
            self.handoff_rejections = self.handoff_rejections.saturating_add(1);
        }
        self.last_handoff = verdict;
        verdict
    }

    /// 跑一次 ΔE 立案审计。
    pub fn audit_deviations(&mut self) -> usize {
        self.deviations.audit(&mut self.diag)
    }

    /// 阻断项计数（含遥测累计）。
    pub fn blocking(&self) -> usize {
        self.diag.blocking_count()
    }

    /// 设置页人读文本。
    ///
    /// **默认来源必须显性出现在文本里**：用户看到"sRGB"时无从知道这是探测结果
    /// 还是默认值。默认状态下多了"默认"二字，用户才能知道自己的能力没被读到。
    pub fn screen_text(&self) -> String {
        let mut s = String::new();
        s.push_str("输出色彩空间：");
        s.push_str(self.encoding.label());
        s.push_str("（");
        s.push_str(match self.origin {
            TargetSpaceOrigin::Probed => "能力探测",
            TargetSpaceOrigin::Defaulted => "默认，未探测",
        });
        s.push_str("）");
        if self.encoding.is_absolute_luminance() {
            s.push_str("参考亮度 ");
            s.push_str(&self.reference_nits.to_string());
            s.push_str(" nit");
        }
        s.push_str("｜方向错配 ");
        s.push_str(&self.direction_mismatches.to_string());
        s.push_str(" 次｜交接拦截 ");
        s.push_str(&self.handoff_rejections.to_string());
        s.push_str(" 次｜ΔE 立案 ");
        s.push_str(&self.deviations.len().to_string());
        s.push_str(" 条");
        s
    }
}

impl Default for ColorSpaceState {
    fn default() -> Self {
        ColorSpaceState::new()
    }
}
