//! VE-F0624 · 附加混合族（VE-D 域 · 2D 合成引擎 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0624`
//!
//! **判据（锚点原文逐条）**：
//! - **plus-lighter（线性光加法）实现与其前提显性**：源与背景在线性光
//!   空间直接相加后裁剪到值域；
//! - **必须在 F0637 线性空间纪律下计算**——sRGB 空间直接相加在数学上
//!   是错的，**本条将此前置条件写成断言而非注释**；
//! - **plus-darker 与相关候选模式的登记与取舍（规范收录态如实）**；
//! - **应用场景登记**：辉光、能量条、高光叠加类合成的官方推荐路径；
//! - **与 F0622 normal 基线的区分语义（加法不是叠加）**。
//!
//! 锚点判据原文：**线性前提断言、规范取舍如实、场景指路、判据**。
//!
//! ## 〇、规范原文（本条一切公式的唯一出处）
//!
//! Compositing and Blending Level 1 §10.4（plus-lighter）：
//!
//! ```text
//! plus-lighter:
//!     Co = min(Cs + Cb, 1.0)          ← 三通道各自，值域裁剪
//!
//! 非预乘形式（W3C 一般混合模型 Cs'αs + Cb'αb(1−αs) + ... 之后的形式化简）：
//!     premultiplied:  Co_pre = min(Cs_pre + Cb_pre, 1.0)
//!     unpremultiplied: Co = (min(Cs_pre + Cb_pre, 1.0)) / min(αs+αb, 1.0)
//! ```
//!
//! 关键差异两条，都必须落地：
//!
//! 1. **`min(·, 1.0)` 在值域内，不是环绕也不是饱和到 1 而 alpha 不动**；
//! 2. **预乘形式相加后 alpha 也相加**（`αs+αb`，不是 `αs` 也不��� `1`）。
//!    锚点 F0625 要求内部流恒预乘，故主实现走**预乘**形式，
//!    另给直通 alpha 的**出口转换**（与 F0625 转换点登记制同方法论）。
//!
//! ## 一、线性前提为何是**断言**而非注释（锚点本条最强调的一点）
//!
//! 锚点原文：「sRGB 空间直接相加**在数学上是错的**——本条将此前置
//! 条件写成断言而非注释」。
//!
//! 这句话针对的是一个**极易发生的具体事故**：上一个人在 sRGB 编码值
//! 上写了 `Cs + Cb`，代码跑得通、色卡看着"还行"（加法混合的视觉差异
//! 在中灰区并不夸张），于是没人发现——**直到 F0637 的转换点登记表
//! 落地，混合路径里少了一次 sRGB→线性**。
//!
//! 注释挡不住这件事，因为注释只在**读代码时**生效，而事故发生在
//! **调用点**（有人拿 `EncodedRgb` 直接喂 `plus_lighter`）。故本条的
//! 做法是**类型系统 + 运行期双重拦截**：
//!
//! - **类型层**：[`PlusLighter`] 的输入参数类型是 [`LinearRgb`]，
//!   而**不是** [`SrgbRgb`]。二者是**不同的 struct**，
//!   `SrgbRgb` 到 `LinearRgb` 没有 `From` 隐式转换——传错类型**编译不过**；
//! - **运行期**：[`plus_lighter_checked`] 额外携带一个 [`SpaceTag`]，
//!   若标记与实际计算空间不符即返回 [`SpaceViolation`] 并记账。
//!
//! 两层缺一不可：类型层抓编译期，运行期抓**同一份数据被在 sRGB 空间
//! 算过一遍之后又标成线性**（如缓存里存了编码值、类型转换发生得太早）。
//!
//! 运行期两个诊断码**各司其职、互不遮蔽**，其分层次序是可验证的：
//! 「声明为编码空间」必须归 `EncodedInputRejected`（更严重的那个），
//! 而不是被「标记与声明不符」先命中而返回 `TagSpaceMismatch`——
//! 否则第三段检查成为**永不可达的死码**（有 label ≠ 可达，十诫第 13 条）。
//! 判据 [`C24-LINEAR-PREMISE-ASSERTED`] 用四条语料同时钉死四层：
//! 线性过/ 编码标记拦 / 两个线性态混用归 mismatch / 声明为编码归 encoded。
//!
//! ## 一之二、下面这条判据的来历（初版真实踩过的坑，保留作记录）
//!
//! 初版把声明检查放在**最后**，于是 `declared = SrgbEncoded` 时
//! `tag_src != declared` 必先命中，返回了 `TagSpaceMismatch`。
//! 现象是判据红项而**实现全对**——因为第三个期望写的是
//! `EncodedInputRejected`。
//!
//! 归因纪律在此生效：判据红 ≠ 判据对。这次两侧都不对——
//! 判据的期望码错了，**实现的分层次序也错了**（死码）。
//! 修法是修实现（声明检查提到标记一致性之前）并让判据覆盖四层，
//! **不是**把判据的期望改成实现当前返回的值（那会把死码固化成门禁）。
//!
//! 判据 [`C24-LINEAR-PREMISE-ASSERTED`] 用**两版正反语料**双向验证：
//! 线性输入必须通过，而 sRGB 输入必须被拦截——只断前者的话，把断言
//! 整个删掉（`Ok` 恒返回）判据仍全绿（十诫第 5 条：断言两侧同值）。
//!
//! ## 二、自实现幂函数（`no_std` 铁律）
//!
//! sRGB 传输函数需要 `x^2.4`（EOTF）与 `x^(1/2.4)`（OETF）。真内核
//! （`kernel-image`）是 `no_std`，**`f32::powf` / `f64::powi` 都不存在**
//! ——内核只保证四则运算与比较。宿主构建（`extern crate std`）下它们
//! 可用，但那会让本条在真内核上编译失败。
//!
//! 故本条自实现 [`fpow`]：把 `x^y` 分解为 `exp2(y·log2 x)`，其中
//! `log2` 用**区间归约 + 多项式**（把 `[1,2)` 上的 `log2` 用最小平方
//! 多项式逼近），`exp2` 同理。
//!
//! 精度口径：sRGB 传输函数的**往返误差**是 F0637 登记入账的量，
//! 本条以 [`LINEAR_ROUNDTRIP_LSB`]（≤1 LSB，对齐 F0622/F0623 口径）
//! 为验收线。**注意这不是"powf 精度"的泛化验收**——它只要求
//! "sRGB→线性→sRGB 往返落在 8 bit 量化分辨率以内"，因为往返后要落到
//! 8 bit 帧缓冲，误差小于 1 LSB 即不可观测。
//!
//! 幂函数另有**必须用反证语料**的理由：多项式在 `[0,1]` 端点附近
//! 误差最大，而 sRGB 传输函数恰恰在 `0`（EOTF 线性段）与 `1`
//! （OETF 线性段）附近换段。
//!
//! 换段断点另有一处**实测踩到的弱门禁**（保留作记录）：初版判据只验
//! 「断点两侧 ±δ 夹逼连续」，而把 `0.04045` 改成 `0.04` 后**两版都
//! 连续**（传输函数本就连续可导），变异MISSED。
//!
//! 根因值得记住：**连续性判据抓的是「换段处有没有缝」，而这个缺陷是
//! 「缝的位置偏了」——连续性对位置偏移完全不敏感**。连续性判据并非
//! 无用（它抓两段公式在交点处不自洽），但不能单独承担「位置正确」。
//! 补法与 F0623 核对 Lum 权重同款：断点做成可核对常量+ **逐字** `==`
//! 规范值。只断常量会自证式（改常量的人顺手改判据期望），故 oracle
//! 侧另断取值量级合理性作为**独立第二侧**。
//!
//! **顺带测出并记录一条可观测性边界**（本条最有价值的判据知识）：
//! `0.04045` 与 `0.04` 两条分支在断点附近本就近乎重合——实测两式在
//! `0.04045` 处差`≈1.3e-6`（约 `0.0003 LSB`）。也就是说**断点偏移在
//! 输出上原理不可观测**：对拍、往返、夹逼三样都抓不到它。
//!
//! 推论（可复用到其它参数类判据）：当两个候选实现在**某区间内本就
//! 近乎重合**时，「输出对拍」这条判据路线对该缺陷**原理无效**，
//! 必须退回「常量逐字核对」或「可观测性更强的中间量」路线。
//! 这也解释了为什么 F0623 要去断`clip_order_probe` 那个**中间结果**
//! 而非最终输出——同一类问题的两种处置。
//!
//! 幂函数对已知数值亦然（[`C24-FPOW-KNOWN-VALUES`]）：`x^0=1`、`x^1=x`、
//! `1^y=1`、`0.5^2=0.25` 四个**数学上精确**的值——用精确值而非
//! "看着差不多"的自洽性，否则 `fpow` 整体偏移一个量级也照过。
//!
//! ## 三、值域裁剪的次序（锚点「值域溢出→按规范裁剪」）
//!
//! 规范是 `min(Cs + Cb, 1.0)`：**先加后裁**，一步到位。
//!
//! 陷阱在于有人写成"边加边裁"（`min(min(Cs,1)+min(Cb,1), 1)`）——
//! 在定义域 `[0,1]` 内两者**完全等价**，于是这个错误在正常语料下
//! 不可观测。故判据必须用**越界输入**打它：[`C24-CLAMP-AFTER-ADD`]。
//!
//! 另一处次序陷阱：**alpha 裁剪与颜色裁剪必须同在一步**。
//! 若先裁 alpha 再用裁后的 alpha 做反预乘除法，颜色会被放大
//! （`αs+αb > 1` 时除以 `1.0` 而非真实和）——判据
//! [`C24-ALPHA-CLAMP-COORDINATED`] 用 `α` 和恰为 `1.0` 的语料
//! 与 `>1.0` 的语料对比，钉死两者必须**同步**裁剪。
//!
//! ## 四、plus-darker 与候选模式的**如实登记**（锚点硬要求）
//!
//! 锚点：「plus-darker 与相关候选模式的登记与取舍（**规范收录态如实**）」。
//!
//! 「如实」的具体含义是：**不能把没进规范的模式写成规范模式**。
//! 本条的 [`CandidateMode`] 把每个候选模式的 [`SpecStatus`] 显式登记为
//! 三态之一：
//!
//! - [`SpecStatus::Specified`]——已进规范（当前仅 `plus-lighter` 一个）；
//! - [`SpecStatus::DraftOnly`]——有草案/实现但**未进规范**；
//! - [`SpecStatus::UnspecifiedButReal`]——生态广泛实现、规范未收录，
//!   且**给出不收录的理由**（[`CandidateMode::rationale`]）。
//!
//! 判据 [`C24-CANDIDATE-REGISTRY-HONEST`] 逐条断言：
//! (a) `plus-lighter` 必须是 `Specified`；
//! (b) **每个**非 `Specified` 条目必须带非空 `rationale`（不许裸奔）；
//! (c) `keyword()` 不得重复（重复即注册表失效）；
//! (d) 计数与枚举一致（不许登记了但枚举里没有，反之亦然）。
//!
//! **为什么这条判据不可省**：一个把 plus-darker 标成"规范模式"的
//! 表格，会让下游 F0628 模板生成器**为不存在的规范条款生成着色器**，
//! 且对拍（F0630）会拿一个自造的"金标准"去比——**自证式**。
//!
//! ## 五、场景登记与"加法不是叠加"（锚点第四、五点）
//!
//! **场景登记**：[`UseCase`] 三条（辉光、能量条、高光叠加），每条
//! 带 [`UseCase::recommended_path`]——**官方推荐路径**指回
//! `plus-lighter` 或明确的替代（如高光叠加推荐 `screen` 而非
//! `plus-lighter`）。登记不是注释：判据断每条 `recommended_path`
//! 指向**注册表内存在**的模式键（十诫第 8 条：`pub` 数据不能依赖
//! 调用方自觉）。
//!
//! **加法 ≠ 叠加**：[`C24-ADDITION-NOT-SOURCE-OVER`] 用**同语料**
//! 跑 `plus_lighter` 与 F0622 的 `normal`，断言两者结果**不同**。
//! 理由：`normal` 是 `αs·Cs + αb·Cb·(1−αs)`（**源覆盖**，按 alpha
//! 加权），`plus-lighter` 是 `min(Cs + Cb, 1)`（**值域相加**，与
//! alpha 无关）。当 `αs + αb > 1` 或两者都半透明时二者差异显著。
//! 若某天实现退化成"加法但按 alpha 加权"，这条判据立刻转红。
//!
//! ## 六、四路同源（沿用 F0622/F0623 惯例，服务 F0628/F0629/F0630）
//!
//! | 路 | 入口 | 角色 |
//! |----|------|------|
//! | 标量 f32 | [`plus_lighter`] | 主实现 |
//! | 独立 f64 | [`oracle`] | 金标准（不读本文件的标量路） |
//! | 4 像素宽通道 | [`plus_lighter_simd`] | 摊薄面（下游 F0629 内核） |
//! | WGSL 文本 | [`wgsl_source`] | GPU 路模板（下游 F0628 来源） |
//!
//! 金标准用 `f64` 且**独立重写一遍 sRGB 传输与裁剪**，不复用标量
//! 路的 `fpow`——否则 oracle 与被测方共享同一个 `fpow`，
//! `fpow` 写错时**两侧一起错**，对拍恒过（十诫第 7 条：自证式）。
//!
//! ## 七、错误路径与降级矩阵（锚点原文三行）
//!
//! | 锚点情形 | 本条处置 | 不许做的事 |
//! |----------|----------|------------|
//! | 非线性空间误用 → 断言拦截 | 类型层 + [`plus_lighter_checked`] 运行期标记核对，[`SpaceLog`] 记账 | 只在注释里写"须线性" |
//! | 值域溢出 → 按规范裁剪 | [`plus_lighter`] 内 `min` 一步到位，[`C24-CLAMP-AFTER-ADD`] 打边加边裁变体 | 环绕（wrap）或饱和 alpha 不裁颜色 |
//! | 模式误选 → 文档三要素指路 | [`UseCase::recommended_path`] + [`MisPick`] 的三要素（错在哪 / 该用哪个 / 为什么） | 只说"用错模式了" |
//!
//! 另有本条自身的输入面：非有限输入（`NaN`/`±Inf`）由
//! [`guard_finite`] 兜底记账，**绝不静默传播**（`NaN` 进预乘链会
//! 毁掉整条下游，F0625）。
//!
//! ## 八、无障碍与隐私
//!
//! [`swatch_rows`] / [`scene_rows`] / [`candidate_rows`] 产出**纯文本行**
//! （ASCII 灰阶是**文字**不是色块，读屏可达），零用户像素内容——
//! 本条全部语料是自造的合成数值。零 IO、零墙钟、确定性算法。
//!
//! ## 九、跨批对接点
//!
//! 上游 [`ved21_blendreg`]（F0621 注册表）、[`ved22_separable`]（F0622：
//! 本条**真引用**其 `SWATCH_PROBES`/`RAMP`/`ramp_char`/`LSB8`/
//! `LSB_BUDGET` 与 `SepMode::Normal` 以做"加法≠叠加"对照）、
//! F0637（线性空间纪律，本条断言即其前置条件的形式化）；
//! 下游 F0628（GPU 路，本条 [`wgsl_source`] 是模板）、F0629（CPU SIMD，
//! 本条 [`plus_lighter_simd`] 是内核）、F0630（对拍，本条 [`oracle`]
//! 的 LSB 口径与其一致）、F0625（预乘纪律，本条预乘/直通出口转换点）。
//!
//! ## 十、本条的判据清单（[`run_ved24_checks`]）
//!
//! | 判据 | 抓什么 |
//! |------|--------|
//! | [`C24-LINEAR-PREMISE-ASSERTED`] | 线性前提只做了一半（单向断言恒过） |
//! | [`C24-FORMULA-VS-ORACLE`] | 公式抄错规范（对拍独立 f64） |
//! | [`C24-ORACLE-INDEPENDENT`] | oracle 复用标量路的 `fpow`（自证式） |
//! | [`C24-CLAMP-AFTER-ADD`] | 边加边裁（正常语料下不可观测） |
//! | [`C24-ALPHA-CLAMP-COORDINATED`] | alpha 与颜色裁剪不同步 |
//! | [`C24-PREMULTIPLIED-ADD-ALPHA`] | alpha 取 `αs` 或 `1` 而非 `αs+αb` |
//! | [`C24-TRANSPORT-ROUNDTRIP`] | sRGB↔线性往返误差超 1 LSB |
//! | [`C24-TRANSPORT-SEGMENT-SPLIT`] | 两个换段断点写错 |
//! | [`C24-FPOW-KNOWN-VALUES`] | `fpow` 自身错（对已知数值） |
//! | [`C24-CANDIDATE-REGISTRY-HONEST`] | 未收录模式被标成规范模式 / 无理由 |
//! | [`C24-USECASE-PATH-REGISTERED`] | 场景推荐路径指向不存在的模式 |
//! | [`C24-ADDITION-NOT-SOURCE-OVER`] | 加法退化成源覆盖（与 normal 同值） |
//! | [`C24-SIMD-BATCH-EQUIV`] | 宽通道与标量不等价 |
//! | [`C24-SIMD-AMORTIZATION`] | 宽通道退化成逐像素循环 |
//! | [`C24-WGSL-TEXT-KEYWORDS`] | GPU 路只吐壳 |
//! | [`C24-MISPICK-Triad`] | 误选提示三要素缺项 |
//! | [`C24-CLAMP-NON-FINITE`] | `NaN`/`±Inf` 静默传播 |
//! | [`C24-SWATCH-SHARED-PROBES`] | 另抄一份斜坡探针 |
//! | [`C24-KEY-OUT-OF-RANGE-NONE`] | 越界键静默变合法模式 |
//! | [`C24-LEDGER-EXACT`] | 记账被吞（幽灵记账 / 漏记） |

use crate::checks::CheckSet;

// no_std 导入三件套 + format!：本条产出摘要、WGSL 文本与文本行。
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// 与 F0622 共用色卡与对拍框架（锚点「共用色卡与对拍框架」同款硬要求）。
// 真引用而非复制：判据 C24-SWATCH-SHARED-PROBES 依赖这一致性。
use super::ved22_separable::{
    ramp_char, LSB8, LSB_BUDGET, RAMP, SWATCH_PROBES,
};
// 「加法不是叠加」需要与 normal 基线对照（锚点第五点）。
use super::ved22_separable::{eval_pixel_rgba, DivZeroLog, SepMode};

// ---------------------------------------------------------------------------
// 一、参数唯一源（规范原文值，不可调）
// ---------------------------------------------------------------------------

/// 附加族**规范已收录**的模式数（当前仅 `plus-lighter`）。
pub const SPECIFIED_COUNT: usize = 1;

/// 附加族**候选登记**总数（含已收录者；锚点要求「相关候选模式」也登记）。
pub const CANDIDATE_COUNT: usize = 4;

/// 8 bit 量化步长（LSB 口径，与 F0622/F0623 同源，判据共用尺度）。
pub const LSB8_STEP: f32 = LSB8;

/// 对拍允许的最大 LSB 偏差（F0622 确立，全组沿用）。
pub const LSB_LIMIT: f32 = LSB_BUDGET;

/// sRGB→线性往返误差预算（≤1 LSB，对齐 F0622/F0623 的对拍口径）。
pub const LINEAR_ROUNDTRIP_LSB: f32 = 1.0;

/// 值域下界（规范 `min(Cs+Cb, 1)` 的隐含下界：非负输入故恒 ≥ 0）。
pub const VALUE_LO: f32 = 0.0;

/// 值域上界（规范原文 `1.0`，**不可调**）。
pub const VALUE_HI: f32 = 1.0;

/// sRGB EOTF 线性段断点（规范原文 0.04045）。
///
/// EOTF：`c ≤ 0.04045 ? c/12.92 : ((c+0.055)/1.055)^2.4`
pub const EOTF_SPLIT: f32 = 0.04045;

/// sRGB EOTF 线性段斜率分母（规范原文 12.92）。
pub const EOTF_LINEAR_DIV: f32 = 12.92;

/// sRGB OETF 线性段断点（规范原文 0.0031308）。
///
/// OETF：`c ≤ 0.0031308 ? 12.92c : 1.055·c^(1/2.4) − 0.055`
pub const OETF_SPLIT: f32 = 0.0031308;

/// sRGB OETF 线性段斜率（规范原文 12.92）。
pub const OETF_LINEAR_MUL: f32 = 12.92;

/// EOTF 指数（规范原文 2.4）。
pub const EOTF_EXPONENT: f32 = 2.4;

/// OETF 指数（规范原文 `1/2.4`，此处写成除法形式以便逐字核对）。
pub const OETF_EXPONENT_NUM: f32 = 1.0;

/// EOTF 的 `a` 偏置（规范原文 0.055）。
pub const SRGB_ALPHA_OFFSET: f32 = 0.055;

/// EOTF/OETF 的 `a` 缩放（规范原文 1.055）。
pub const SRGB_SCALE: f32 = 1.055;

/// SIMD 宽通道宽度（与 F0622/F0623 同口径）。
pub const SIMD_WIDTH: usize = 4;

/// 色卡探针数（真引用 F0622 的常量长度）。
pub const SWATCH_STEPS: usize = SWATCH_PROBES.len();

/// 非有限输入钳制目标（定义域恒为 `[0,1]`）。
pub const CHANNEL_LO: f32 = VALUE_LO;
pub const CHANNEL_HI: f32 = VALUE_HI;

/// `fpow` 归约后的尾数下界（`[1,2)` 的下端，闭区间）。
pub const MANTISSA_LO: f32 = 1.0;

/// `fpow` 归约后的尾数上界（`[1,2)` 的上端，**开区间**）。
pub const MANTISSA_HI: f32 = 2.0;

/// `fpow` 次正规输入下界（低于此值直接返回 0，避免 `log2` 下溢）。
pub const FPOW_MIN: f32 = 1.0e-30;

/// `fpow` 输出上界（超出即饱和：本条只处理 `[0,1]` 定义域）。
pub const FPOW_MAX: f32 = 1.0e30;

// ---------------------------------------------------------------------------
// 二、色彩空间标签与两种色彩表示（线性前提的类型层）
// ---------------------------------------------------------------------------

/// 色彩空间标签（运行期断言的载体）。
///
/// 单独成枚举而非布尔 `is_linear`：**一个 bool 表达两件事必错**——
/// "在哪个空间" 与 "是否已转换" 是两个独立事实，用一个 bool 表达会在
/// 第三个场景（硬件 sRGB 帧缓冲，硬件自动转换）下失真（十诫：拆 enum）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpaceTag {
    /// sRGB 编码值空间（**非线性**，gamma 编码）。
    SrgbEncoded,
    /// 线性光空间（**gamma 1.0**，本条混合的唯一合法空间）。
    LinearLight,
    /// 硬件 sRGB 帧缓冲：驻留线性、由硬件在采样/写回时转换
    /// （F0637「硬件自动转换优先利用」，本条登记该态以便审计
    /// 「硬件与软件转换不得混用于同一帧」）。
    HardwareSrgbLinear,
}

impl SpaceTag {
    /// 全部标签（供审计遍历）。
    pub fn all() -> [SpaceTag; 3] {
        [
            SpaceTag::SrgbEncoded,
            SpaceTag::LinearLight,
            SpaceTag::HardwareSrgbLinear,
        ]
    }

    /// 该标签下**允许**进入 [`plus_lighter`] 计算的判据。
    ///
    /// 硬件 sRGB 帧缓冲态允许：它的驻留表示就是线性，采样拿到的是
    /// 硬件转换后的线性值（F0637）。而 `SrgbEncoded` 一律拒绝。
    pub fn permits_additive(&self) -> bool {
        match self {
            SpaceTag::LinearLight => true,
            SpaceTag::HardwareSrgbLinear => true,
            SpaceTag::SrgbEncoded => false,
        }
    }

    /// 关键字（色卡行与注册表用）。
    pub fn keyword(&self) -> &'static str {
        match self {
            SpaceTag::SrgbEncoded => "srgb-encoded",
            SpaceTag::LinearLight => "linear-light",
            SpaceTag::HardwareSrgbLinear => "hardware-srgb-linear",
        }
    }

    /// 该标签是否**已做** sRGB→线性转换（`HardwareSrgbLinear` 由硬件做）。
    pub fn decoded(&self) -> bool {
        match self {
            SpaceTag::SrgbEncoded => false,
            SpaceTag::LinearLight => true,
            SpaceTag::HardwareSrgbLinear => true,
        }
    }
}

/// 线性光 RGB（`plus_lighter` 的**唯一**合法输入类型）。
///
/// 与 [`SrgbRgb`] 是**不同 struct 且无 `From`**——传错类型编译不过。
/// 这是线性前提的**类型层**拦截（见头注 §一）。
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct LinearRgb {
    /// 预乘红色分量（线性光，`[0,1]`）。
    pub r: f32,
    /// 预乘绿色分量（线性光，`[0,1]`）。
    pub g: f32,
    /// 预乘蓝色分量（线性光，`[0,1]`）。
    pub b: f32,
    /// 源 alpha（`[0,1]`；预乘表示下分量已含 alpha）。
    pub a: f32,
}

impl LinearRgb {
    /// 构造（显式命名 `linear` 参数，防调用方以为这是 sRGB 值）。
    pub fn new_linear(r: f32, g: f32, b: f32, a: f32) -> LinearRgb {
        LinearRgb { r, g, b, a }
    }

    /// 灰（各通道等值）。
    pub fn gray(v: f32) -> LinearRgb {
        LinearRgb { r: v, g: v, b: v, a: v }
    }

    /// 按下标取色分量（`0=r,1=g,2=b`；`3` 取 alpha）。
    ///
    /// 越界返回 `0.0` 而**不 panic**（零 panic 面：判据语料常带越界下标）。
    pub fn get(&self, i: usize) -> f32 {
        match i {
            0 => self.r,
            1 => self.g,
            2 => self.b,
            3 => self.a,
            _ => 0.0,
        }
    }

    /// 按下标写色分量（越界原样返回，不改值）。
    pub fn with(&self, i: usize, v: f32) -> LinearRgb {
        let mut o = *self;
        match i {
            0 => o.r = v,
            1 => o.g = v,
            2 => o.b = v,
            3 => o.a = v,
            _ => {}
        }
        o
    }

    /// 三色分量的最小值。
    pub fn min_ch(&self) -> f32 {
        let m = if self.r < self.g { self.r } else { self.g };
        if self.b < m {
            self.b
        } else {
            m
        }
    }

    /// 三色分量的最大值。
    pub fn max_ch(&self) -> f32 {
        let m = if self.r > self.g { self.r } else { self.g };
        if self.b > m {
            self.b
        } else {
            m
        }
    }
}

/// sRGB 编码 RGB（**不能**直接喂 [`plus_lighter`]）。
///
/// 存在的意义是**让"传错空间"这件事在类型上可见**：若本条只定义
/// 一个 `Rgb`，调用方无从判断手里的值是编码还是线性。
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct SrgbRgb {
    /// 编码红（`[0,1]`）。
    pub r: f32,
    /// 编码绿（`[0,1]`）。
    pub g: f32,
    /// 编码蓝（`[0,1]`）。
    pub b: f32,
    /// 直通 alpha（`[0,1]`；**非**预乘）。
    pub a: f32,
}

impl SrgbRgb {
    /// 构造（显式命名 `encoded` 参数）。
    pub fn new_encoded(r: f32, g: f32, b: f32, a: f32) -> SrgbRgb {
        SrgbRgb { r, g, b, a }
    }

    /// 灰（各通道等值）。
    pub fn gray(v: f32) -> SrgbRgb {
        SrgbRgb { r: v, g: v, b: v, a: v }
    }

    /// 按下标取分量（越界 `0.0`，不 panic）。
    pub fn get(&self, i: usize) -> f32 {
        match i {
            0 => self.r,
            1 => self.g,
            2 => self.b,
            3 => self.a,
            _ => 0.0,
        }
    }

    /// 按下标写分量（越界不改）。
    pub fn with(&self, i: usize, v: f32) -> SrgbRgb {
        let mut o = *self;
        match i {
            0 => o.r = v,
            1 => o.g = v,
            2 => o.b = v,
            3 => o.a = v,
            _ => {}
        }
        o
    }

    /// **显式**转换到线性光空间（F0625/F0637 转换点登记制下的入口转换）。
    ///
    /// 直通 alpha → 预乘（先转色再乘 alpha，次序按 F0625）。
    pub fn to_linear(&self) -> LinearRgb {
        let lr = srgb_to_linear(self.r);
        let lg = srgb_to_linear(self.g);
        let lb = srgb_to_linear(self.b);
        LinearRgb {
            r: lr * self.a,
            g: lg * self.a,
            b: lb * self.a,
            a: self.a,
        }
    }
}

/// 直通 alpha 输出（`plus_lighter` 出口转换的产物类型）。
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct StraightRgb {
    /// 直通红（sRGB 编码）。
    pub r: f32,
    /// 直通绿（sRGB 编码）。
    pub g: f32,
    /// 直通蓝（sRGB 编码）。
    pub b: f32,
    /// alpha（`[0,1]`）。
    pub a: f32,
}

// ---------------------------------------------------------------------------
// 三、幂函数与 sRGB 传输（`no_std` 自实现）
// ---------------------------------------------------------------------------

/// 归约到 `[1,2)` 的尾数与二进制指数。
///
/// `v > 0` 时返回 `(m, e)` 满足 `v = m · 2^e` 且 `1 ≤ m < 2`。
fn frexp2(v: f32) -> (f32, i32) {
    if !(v > 0.0) {
        // 0 与负数：返回最小尾数，调用方另行处理（本条只传正数）。
        return (MANTISSA_LO, 0);
    }
    let mut m = v;
    let mut e = 0i32;
    // 归约到 [1,2)：先按 2 的幂缩放，再用乘法微调。
    // 上限 64 轮：f32 指数范围约 ±127，`[1,2)` 归约最多 127 轮足够。
    let mut guard = 0;
    while m >= MANTISSA_HI {
        m *= 0.5;
        e += 1;
        guard += 1;
        if guard > 64 {
            break;
        }
    }
    while m < MANTISSA_LO {
        m *= 2.0;
        e -= 1;
        guard += 1;
        if guard > 64 {
            break;
        }
    }
    (m, e)
}

/// `log2` 的尾数部分多项式：`log2(m)`，`m ∈ [1,2)`。
///
/// 用**变量替换** `t = (m−1)/(m+1)`（此时 `t ∈ [0, 1/3)`），
/// `ln(m) = 2(t + t³/3 + t⁵/5 + …)`；`t` 的收敛域远宽于 `[1,2)`，
/// 8 项（到 `t¹⁵`）的截断误差在该区间内远小于 `f32` 精度。
///
/// 系数为 `2/(k·2^k)` 形式的**字面量**而非运行时求幂——`no_std` 下
/// 求幂需再套一层 `fpow`，会形成循环依赖。
fn log2_mantissa_poly(t: f32) -> f32 {
    // atanh 级数：ln(m) = 2·Σ_{k=0..7} t^(2k+1)/(2k+1)，t = (m−1)/(m+1)。
    // 系数 2/(2k+1) 为字面量而非运行时求幂——`no_std` 下求幂需再套一层
    // `fpow`，会形成循环依赖。
    let mut acc = 0.0f32;
    let tp = t * t;
    let mut pow = t; // t^(2k+1) 的递推载体
    let mut k = 0i32;
    while k < 8 {
        acc += 2.0 * pow / (2.0 * k as f32 + 1.0);
        pow *= tp;
        k += 1;
    }
    acc
}

/// `2^f` 的多项式：`f ∈ [0,1)` 时返回 `2^f − 1 ∈ [0,1)`。
///
/// 同样用 `t` 替换：`2^f = e^(f·ln2)`，直接对 `z = f·ln2` 做
/// `exp(z)` 的泰勒展开（`|z| ≤ ln2 < 0.7`，收敛极快，10 项足够）。
fn exp2_frac_poly(f: f32) -> f32 {
    // ln2 的 f32 精确值（比 `core::f32::consts` 更明确，且不依赖宿主）。
    const LN2: f32 = 0.693_147_2;
    let z = f * LN2;
    let mut acc = 0.0f32;
    let mut term = 1.0f32;
    let mut k = 0i32;
    // exp(z) = Σ z^k/k!；再减 1（因为多项式给的是 e^z − 1）
    while k < 12 {
        acc += term;
        term *= z / (k as f32 + 1.0);
        k += 1;
    }
    acc - 1.0
}

/// 自实现 `x^y`（`x ≥ 0`，`y` 为实数）。
///
/// 分解：`x^y = 2^(y·log2 x)`，`log2 x = e + log2(m)`（[`frexp2`]）。
/// **本条的所有幂运算都走这里**——真内核 `no_std` 无 `powf`。
///
/// 输入面：`x ≤ 0` 返回 `0.0`（本条定义域为 `[0,1]`，负底无意义），
/// 非有限返回 `0.0` 并由调用方记账（绝不静默传播 `NaN`）。
pub fn fpow(x: f32, y: f32) -> f32 {
    let gx = guard_finite(x);
    let gy = guard_finite(y);
    if gx <= 0.0 {
        // 0^y：y>0 ⇒ 0；y=0 ⇒ 1（数学惯例）；y<0 ⇒ 本条不定义，取 0。
        return if gy > 0.0 { 0.0 } else { 0.0 };
    }
    if !(gx < FPOW_MAX) {
        return FPOW_MAX;
    }
    let (m, e) = frexp2(gx);
    let t = (m - 1.0) / (m + 1.0);
    let ln_m = log2_mantissa_poly(t);
    // log2(m) = ln(m) / ln2；ln2 常量与 exp2 侧共用同一口径。
    const LN2: f32 = 0.693_147_2;
    let log2_m = ln_m / LN2;
    let lg = e as f32 + log2_m;
    let ylg = gy * lg;
    // 2^ylg = 2^floor(ylg) · 2^frac(ylg)
    let ef = ylg.floor();
    let fr = ylg - ef;
    let mant = 1.0 + exp2_frac_poly(fr);
    let mut out = mant;
    let mut n = ef as i32;
    let mut guard = 0;
    while n > 0 {
        out *= 2.0;
        n -= 1;
        guard += 1;
        if guard > 200 {
            break;
        }
    }
    while n < 0 {
        out *= 0.5;
        n += 1;
        guard += 1;
        if guard > 200 {
            break;
        }
    }
    if out > FPOW_MAX {
        FPOW_MAX
    } else if out < 0.0 {
        0.0
    } else {
        out
    }
}

/// 非有限值兜底：`NaN`/`±Inf` → `0.0`；其余原样返回。
///
/// **返回 `0.0` 而非原值**是刻意的：`NaN` 进入预乘链会毁掉整条
/// 下游（F0625），而 `0.0` 在加法混合里是**可预期**的贡献。
/// 是否发生过兜底由 [`SpaceLog`] 记账，不是静默。
pub fn guard_finite(v: f32) -> f32 {
    if v.is_finite() {
        v
    } else {
        0.0
    }
}

/// sRGB EOTF：编码 → 线性（**入口转换点**，F0637 登记制）。
///
/// 规范原文：`c ≤ 0.04045 ? c/12.92 : ((c+0.055)/1.055)^2.4`
pub fn srgb_to_linear(c: f32) -> f32 {
    let v = guard_finite(c);
    if v <= EOTF_SPLIT {
        v / EOTF_LINEAR_DIV
    } else {
        fpow((v + SRGB_ALPHA_OFFSET) / SRGB_SCALE, EOTF_EXPONENT)
    }
}

/// sRGB OETF：线性 → 编码（**出口转换点**，F0637 登记制）。
///
/// 规范原文：`c ≤ 0.0031308 ? 12.92c : 1.055·c^(1/2.4) − 0.055`
pub fn linear_to_srgb(c: f32) -> f32 {
    let v = guard_finite(c);
    if v <= OETF_SPLIT {
        v * OETF_LINEAR_MUL
    } else {
        SRGB_SCALE * fpow(v, OETF_EXPONENT_NUM / EOTF_EXPONENT) - SRGB_ALPHA_OFFSET
    }
}

/// sRGB→线性→sRGB 往返误差（LSB 口径），供 [`C24-TRANSPORT-ROUNDTRIP`]。
pub fn transport_roundtrip_lsb(c: f32) -> f32 {
    let back = linear_to_srgb(srgb_to_linear(c));
    let d = back - c;
    if d < 0.0 {
        (-d) / LSB8_STEP
    } else {
        d / LSB8_STEP
    }
}

/// 值域裁剪到 `[0,1]`（规范 `min(·, 1)` 与隐含下界 0）。
///
/// 非有限先兜底再裁——**次序不可换**：先裁后兜底会让 `NaN` 逃逸
/// （`min(NaN, 1)` 在 `f32` 下返回非 `NaN` 的那个操作数，静默吞掉）。
pub fn clamp01(v: f32) -> f32 {
    // **`+Inf` 必须裁到上界 1.0，不能归零**——这是本函数与
    // [`guard_finite`] 的**语义分界**，也是初版真实踩过的坑：
    // 初版直接 `guard_finite` 后再裁，于是 `+Inf → 0.0`。
    //
    // 为什么归零是错的：`clamp01` 的契约是「把值收进 `[0,1]`」，
    // 而 `min(+Inf, 1) = 1` 是**数学上正确**的收口；归零等于把
    // "过曝的极亮"报成"全黑"，下游辉光/能量条会得到方向相反的结果。
    // [`guard_finite`] 归零是对的——它的契约是"让非有限值不污染
    // 下游"，此处需要的是 0（无贡献）而非 1（全亮）。
    //
    // 判据 [`C24-CLAMP-NON-FINITE`] 同时钉死三个方向：
    // `NaN → 0`、`−Inf → 0`、`+Inf → 1`——只断前两个的实现照过。
    let g = if v.is_nan() {
        // NaN 无序：既不是"过亮"也不是"过暗"，唯一安全目标是 0
        // （且**必须**在比较之前处理，否则 `min(NaN,1)` 会静默
        // 返回比较运算的另一操作数，把 NaN 吞掉）。
        VALUE_LO
    } else {
        v
    };
    if g < VALUE_LO {
        VALUE_LO
    } else if g > VALUE_HI {
        VALUE_HI
    } else {
        g
    }
}

// ---------------------------------------------------------------------------
// 四、模式键、候选登记与规范收录态
// ---------------------------------------------------------------------------

/// 规范收录态（锚点「**规范收录态如实**」的载体）。
///
/// 三态而非 bool：bool 会把"草案"与"生态实现但未收录"混成同一个
/// `false`——而这两者对下游的处置完全不同（草案要跟踪，生态实现
/// 要给理由）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpecStatus {
    /// 已进规范（当前仅 `plus-lighter`）。
    Specified,
    /// 有草案/实现，**未进规范**（需跟踪规范进展）。
    DraftOnly,
    /// 生态广泛实现、规范未收录，且**必须给不收录理由**。
    UnspecifiedButReal,
}

impl SpecStatus {
    /// 全部态（供审计遍历）。
    pub fn all() -> [SpecStatus; 3] {
        [
            SpecStatus::Specified,
            SpecStatus::DraftOnly,
            SpecStatus::UnspecifiedButReal,
        ]
    }

    /// 关键字（登记行用）。
    pub fn keyword(&self) -> &'static str {
        match self {
            SpecStatus::Specified => "specified",
            SpecStatus::DraftOnly => "draft-only",
            SpecStatus::UnspecifiedButReal => "unspecified-but-real",
        }
    }

    /// 是否已进规范。
    pub fn is_specified(&self) -> bool {
        matches!(self, SpecStatus::Specified)
    }

    /// 是否**必须**带登记理由（未收录者一律必须）。
    pub fn requires_rationale(&self) -> bool {
        !self.is_specified()
    }
}

/// 附加族模式键（**仅** `plus-lighter` 是规范模式）。
///
/// 候选模式**不进此枚举**——它们在 [`CandidateMode`] 里单独登记。
/// 分成两个类型是刻意的：让"能进混合公式分派的键"在类型上就只含
/// 规范模式，杜绝 F0628 模板生成器为不存在的条款生成着色器。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AddMode {
    /// `plus-lighter`：线性光空间值域相加。
    PlusLighter,
}

impl AddMode {
    /// 全部规范模式（当前 1 个）。
    pub fn all() -> [AddMode; SPECIFIED_COUNT] {
        [AddMode::PlusLighter]
    }

    /// 注册下标（与 [`AddMode::all`] 同序）。
    pub fn index(&self) -> usize {
        match self {
            AddMode::PlusLighter => 0,
        }
    }

    /// 按下标取模式（**越界返回 `None`**，绝不 panic）。
    pub fn from_index(i: usize) -> Option<AddMode> {
        match i {
            0 => Some(AddMode::PlusLighter),
            _ => None,
        }
    }

    /// 规范关键字（CSS 侧写法）。
    pub fn keyword(&self) -> &'static str {
        match self {
            AddMode::PlusLighter => "plus-lighter",
        }
    }

    /// 规范条款号（本条公式的唯一出处）。
    pub fn clause(&self) -> &'static str {
        match self {
            AddMode::PlusLighter => "compositing-1 §10.4",
        }
    }

    /// 该模式要求的输入色彩空间（**唯一**合法空间）。
    pub fn required_space(&self) -> SpaceTag {
        match self {
            AddMode::PlusLighter => SpaceTag::LinearLight,
        }
    }

    /// 公式散文（色卡行与文档用）。
    pub fn prose(&self) -> &'static str {
        match self {
            AddMode::PlusLighter => "Co = min(Cs + Cb, 1.0)，逐通道，线性光空间",
        }
    }

    /// 混合公式的规范形态：是否按预乘相加（`plus-lighter` 是）。
    pub fn premultiplied_add(&self) -> bool {
        match self {
            AddMode::PlusLighter => true,
        }
    }
}

/// 候选模式登记条目（**如实**登记规范收录态）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CandidateMode {
    /// 候选关键字（即便未收录也登记，便于生态对照）。
    pub keyword: &'static str,
    /// 收录态。
    pub status: SpecStatus,
    /// 收录态理由（未收录者**必须非空**；已收录者写其条款依据）。
    pub rationale: &'static str,
}

impl CandidateMode {
    /// 全部候选条目（含已收录者，锚点要求「相关候选模式」也登记）。
    pub fn all() -> [CandidateMode; CANDIDATE_COUNT] {
        [
            CandidateMode {
                keyword: "plus-lighter",
                status: SpecStatus::Specified,
                rationale: "compositing-1 §10.4 收录，条款号即公式出处",
            },
            CandidateMode {
                keyword: "plus-darker",
                status: SpecStatus::DraftOnly,
                rationale: "仅见于草案与实验实现，规范未收录；不实现，等规范进展",
            },
            CandidateMode {
                keyword: "linear-dodge",
                status: SpecStatus::UnspecifiedButReal,
                rationale: "生态（PDF/图像编辑器）广泛使用的取 min 变体；\
                            与 plus-lighter 的差异是**不做**线性光解码，故本条不实现",
            },
            CandidateMode {
                keyword: "linear-burn",
                status: SpecStatus::UnspecifiedButReal,
                rationale: "同上族的取 max 变体；规范未收录，语义可由 max 组合表达，无独立模式必要",
            },
        ]
    }

    /// 该条目是否已进规范。
    pub fn is_specified(&self) -> bool {
        self.status.is_specified()
    }

    /// 登记合规性：**未收录者必须带非空理由**（锚点硬要求）。
    pub fn registry_honest(&self) -> bool {
        if self.status.requires_rationale() {
            !self.rationale.is_empty()
        } else {
            !self.rationale.is_empty()
        }
    }
}

/// 规范收录模式数（自 [`CandidateMode`] 实点得出，**不硬写**）。
///
/// 这样判据"计数诚实"才不是自证：改枚举不加条目，计数自动变化。
pub fn specified_count_from_registry() -> usize {
    let mut n = 0usize;
    for c in CandidateMode::all().iter() {
        if c.is_specified() {
            n += 1;
        }
    }
    n
}

/// 候选关键字是否与规范键冲突（注册表完整性：不允许候选冒名）。
pub fn candidate_key_conflicts(key: &str) -> bool {
    AddMode::all().iter().any(|m| m.keyword() == key)
}

/// 场景登记条目（锚点「应用场景登记」）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UseCase {
    /// 场景名（中文，取自锚点「辉光、能量条、高光叠加」）。
    pub name: &'static str,
    /// 官方推荐路径指向的**规范模式关键字**。
    pub recommended_path: &'static str,
    /// 推荐理由（**非空**是判据要求）。
    pub why: &'static str,
    /// 该场景是否属于"加法类"（用于区分高光叠加这类推荐别的模式的）。
    pub additive_class: bool,
}

impl UseCase {
    /// 全部登记场景（锚点三条 + 一条反例）。
    pub fn all() -> [UseCase; 4] {
        [
            UseCase {
                name: "辉光",
                recommended_path: "plus-lighter",
                why: "辉光是把光**叠加**在画面上，screen 会压暗底色而plus-lighter 不会",
                additive_class: true,
            },
            UseCase {
                name: "能量条",
                recommended_path: "plus-lighter",
                why: "能量条按强度提亮，加法在底色上叠加且到顶后自然饱和",
                additive_class: true,
            },
            UseCase {
                name: "高光叠加",
                recommended_path: "screen",
                why: "高光只需提亮不需真正相加；screen 的上界恰为底色，\
                       不会像 plus-lighter 那样把纯色区推到全白",
                additive_class: false,
            },
            UseCase {
                name: "半透明色片叠加",
                recommended_path: "normal",
                why: "常规半透明合成应按 alpha 覆盖（normal），加法会累积到过曝",
                additive_class: false,
            },
        ]
    }

    /// 推荐路径是否指向**已登记**的规范模式关键字。
    ///
    /// 不查候选表：场景登记只允许指向**规范模式**——指向候选模式
    /// 等于推荐一个本条明确不实现的模式。
    pub fn path_registered(&self) -> bool {
        AddMode::all().iter().any(|m| m.keyword() == self.recommended_path)
            || matches!(
                self.recommended_path,
                "screen" | "normal" | "multiply" | "overlay" | "darken" | "lighten"
            )
    }

    /// 是否属于加法类（`plus-lighter` 专属场景）。
    pub fn is_additive(&self) -> bool {
        self.additive_class
    }
}

/// 模式误选提示的**三要素**（锚点「文档三要素指路」）。
///
/// 拆成三个字段而非一段散文：判据要能**逐项**断缺项，
/// 一段字符串只能断"非空"（那是弱门禁，十诫第 3 条）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MisPick {
    /// 一要素：错在哪（调用方做了什么）。
    pub where_wrong: &'static str,
    /// 二要素：该用哪个（规范关键字）。
    pub use_instead: &'static str,
    /// 三要素：为什么（区分语义）。
    pub why: &'static str,
}

impl MisPick {
    /// 全部登记的误选提示。
    pub fn all() -> [MisPick; 3] {
        [
            MisPick {
                where_wrong: "用 plus-lighter 表达常规半透明覆盖",
                use_instead: "normal",
                why: "加法与 alpha 无关：normal 按 alpha 加权，加法按值域相加，\
                       两者在半透明叠加时结果不同",
            },
            MisPick {
                where_wrong: "用 plus-lighter 提亮高光",
                use_instead: "screen",
                why: "screen 上界为底色，不会把高亮区推到全白；\
                       plus-lighter 会（min(·,1) 恰在纯色区饱和）",
            },
            MisPick {
                where_wrong: "在 sRGB 编码值上求 plus-lighter",
                use_instead: "linear-light",
                why: "编码值相加在数学上是错的；须经 srgb_to_linear 到线性光空间再相加",
            },
        ]
    }

    /// 三要素齐全（三个字段都非空）。
    pub fn triad_complete(&self) -> bool {
        !self.where_wrong.is_empty() && !self.use_instead.is_empty() && !self.why.is_empty()
    }

    /// 推荐关键字是否已登记（与规范模式或已知基线模式对得上）。
    pub fn target_registered(&self) -> bool {
        matches!(
            self.use_instead,
            "plus-lighter" | "normal" | "screen" | "multiply" | "overlay" | "linear-light"
        )
    }
}

// ---------------------------------------------------------------------------
// 五、台账（零静默）
// ---------------------------------------------------------------------------

/// 空间违规与兜底记账。
///
/// 单独成类型而非若干 `Cell`：一个 bool 表达两件事必错——「空间违规」
/// 与「非有限兜底」是两个独立事实，合并会让归因失真。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct SpaceLog {
    /// 非线性空间误用次数（应被断言拦截却算下去的次数，恒为 0）。
    pub space_violations: u32,
    /// 非有限输入兜底次数。
    pub nonfinite_guarded: u32,
    /// 值域裁剪次数（三色分量合计）。
    pub clamped_channels: u32,
    /// alpha 裁剪次数。
    pub alpha_clamped: u32,
    /// 预乘→直通出口转换次数。
    pub unpremultiply_count: u32,
}

impl SpaceLog {
    /// 新台账（全零）。
    pub fn new() -> SpaceLog {
        SpaceLog {
            space_violations: 0,
            nonfinite_guarded: 0,
            clamped_channels: 0,
            alpha_clamped: 0,
            unpremultiply_count: 0,
        }
    }

    /// 记账总次数（供"台账可核对"判据断言恰等于）。
    pub fn total(&self) -> u32 {
        self.space_violations
            + self.nonfinite_guarded
            + self.clamped_channels
            + self.alpha_clamped
            + self.unpremultiply_count
    }

    /// 台账是否干净（无违规、无兜底）。
    pub fn is_clean(&self) -> bool {
        self.total() == 0
    }
}

/// 空间断言失败的原因码（下游封闭枚举无权加变体，故自建）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpaceViolation {
    /// 标记为 sRGB 编码空间却要进线性加法（**必须拦截**）。
    EncodedInputRejected,
    /// 标记为线性却与实际计算空间不符（两侧都是线性态但**不是同一种**：
    /// `LinearLight` 与 `HardwareSrgbLinear` 混用，即同一帧混用硬件与
    /// 软件转换——F0637 明令禁止）。
    TagSpaceMismatch,
}

impl SpaceViolation {
    /// 全部原因码（供审计遍历）。
    pub fn all() -> [SpaceViolation; 2] {
        [
            SpaceViolation::EncodedInputRejected,
            SpaceViolation::TagSpaceMismatch,
        ]
    }

    /// 原因码字面量（下游诊断用）。
    pub fn code(&self) -> &'static str {
        match self {
            SpaceViolation::EncodedInputRejected => "F0624-ENC-INPUT",
            SpaceViolation::TagSpaceMismatch => "F0624-TAG-MISMATCH",
        }
    }

    /// 处置建议（三要素的简短形）。
    pub fn hint(&self) -> &'static str {
        match self {
            SpaceViolation::EncodedInputRejected => {
                "先经 srgb_to_linear（入口转换点）再进 plus_lighter"
            }
            SpaceViolation::TagSpaceMismatch => {
                "检查是否同一帧混用了硬件与软件 sRGB 转换（两侧线性态必须一致）"
            }
        }
    }

    /// 是否阻断（两类都阻断——线性前提不是可降级项）。
    pub fn blocking(&self) -> bool {
        match self {
            SpaceViolation::EncodedInputRejected => true,
            SpaceViolation::TagSpaceMismatch => true,
        }
    }
}

// ---------------------------------------------------------------------------
// 六、主实现
// ---------------------------------------------------------------------------

/// `plus-lighter` 主实现（标量 f32，**预乘**线性光空间）。
///
/// 规范：`Co_pre = min(Cs_pre + Cb_pre, 1.0)`，alpha 同步
/// `αo = min(αs + αb, 1.0)`。
///
/// **次序**：先加后裁（头注 §三），alpha 与颜色裁剪同步。
pub fn plus_lighter(
    mode: AddMode,
    src: LinearRgb,
    dst: LinearRgb,
    log: &mut SpaceLog,
) -> LinearRgb {
    match mode {
        AddMode::PlusLighter => plus_lighter_impl(src, dst, log),
    }
}

/// `plus_lighter` 的实际算式（不经枚举分派，供宽通道逐像素复用）。
fn plus_lighter_impl(src: LinearRgb, dst: LinearRgb, log: &mut SpaceLog) -> LinearRgb {
    // 非有限兜底先行（四个分量各自）。
    let sr = guard_counted(src.r, log);
    let sg = guard_counted(src.g, log);
    let sb = guard_counted(src.b, log);
    let sa = guard_counted(src.a, log);
    let dr = guard_counted(dst.r, log);
    let dg = guard_counted(dst.g, log);
    let db = guard_counted(dst.b, log);
    let da = guard_counted(dst.a, log);

    // 规范算式：先加后裁。
    let r = clamp_counted(sr + dr, log);
    let g = clamp_counted(sg + dg, log);
    let b = clamp_counted(sb + db, log);
    // alpha 与颜色**同步**裁剪（头注 §三）。
    let a_raw = sa + da;
    let a = if a_raw > VALUE_HI {
        log.alpha_clamped += 1;
        VALUE_HI
    } else if a_raw < VALUE_LO {
        log.alpha_clamped += 1;
        VALUE_LO
    } else {
        a_raw
    };
    LinearRgb { r, g, b, a }
}

/// 非有限兜底并记账（[`guard_finite`] 的记账版）。
fn guard_counted(v: f32, log: &mut SpaceLog) -> f32 {
    if v.is_finite() {
        v
    } else {
        log.nonfinite_guarded += 1;
        0.0
    }
}

/// 值域裁剪并记账（[`clamp01`] 的记账版）。
fn clamp_counted(v: f32, log: &mut SpaceLog) -> f32 {
    // 与 [`clamp01`] 同一语义：`+Inf` 裁到上界而非归零（见该函数注释
    // 里的语义分界说明）。
    //
    // 两类记账**分开**：非有限兜底记 `nonfinite_guarded`，
    // 值域越界裁剪记 `clamped_channels`。二者可同时发生（`+Inf`
    // 既非有限又越上界）——但只记前者会让"裁剪台账"在极端输入下
    // 恒为 0，从而**掩盖**裁剪路径从未被走过的缺陷。
    if !v.is_finite() {
        log.nonfinite_guarded += 1;
    }
    if v.is_finite() && (v > VALUE_HI || v < VALUE_LO) {
        log.clamped_channels += 1;
    } else if !v.is_finite() && !v.is_nan() {
        // ±Inf 也会被裁到边界（NaN 走 0 但不算"越界裁剪"——
        // 它是兜底，不是把越界值收到边界）。
        log.clamped_channels += 1;
    }
    clamp01(v)
}

/// 带**运行期空间断言**的 `plus_lighter`（线性前提的第二层拦截）。
///
/// `tag_src`/`tag_dst` 必须是**实际计算该像素时所用的空间**。
/// 若标记为 `SrgbEncoded` ⇒ 拦截（[`SpaceViolation::EncodedInputRejected`]）；
/// 若标记允许但与调用方声明的 `declared` 不符 ⇒ 拦截
/// （[`SpaceViolation::TagSpaceMismatch`]）。
///
/// 返回 `Err` 时**不算**——返回 `Err` 就是「不产生结果」，
/// 不存在"拦截了但还是给了个数"的可能（那才是静默）。
pub fn plus_lighter_checked(
    src: LinearRgb,
    dst: LinearRgb,
    tag_src: SpaceTag,
    tag_dst: SpaceTag,
    declared: SpaceTag,
    log: &mut SpaceLog,
) -> Result<LinearRgb, SpaceViolation> {
    // 零层：**声明**空间本身必须是线性（调用方自述）。
    //
    // 次序纪律：这一段必须在标记一致性检查**之前**。否则
    // `declared = SrgbEncoded` 时 `tag_src != declared` 必先命中，
    // 返回 `TagSpaceMismatch`——于是"声明为编码空间"这个更严重、
    // 更贴切的诊断被"标记不符"这个次要诊断**遮蔽**，第三段成为
    // 永不可达的死码（有 label ≠ 可达，十诫第 13 条）。
    if !declared.permits_additive() {
        log.space_violations += 1;
        return Err(SpaceViolation::EncodedInputRejected);
    }
    // 第一层：实际标记为编码空间一律拒绝。
    if !tag_src.permits_additive() || !tag_dst.permits_additive() {
        log.space_violations += 1;
        return Err(SpaceViolation::EncodedInputRejected);
    }
    // 第二层：标记与声明不符（缓存里存了另一种表示）。
    //
    // 走到这里两侧都已是线性，故此处的不符只能是**两个不同的线性态**
    // （`LinearLight` vs `HardwareSrgbLinear`）——那才是"同一帧混用
    // 硬件与软件转换"（F0637 明令禁止），诊断码归到这里才准确。
    if tag_src != declared || tag_dst != declared {
        log.space_violations += 1;
        return Err(SpaceViolation::TagSpaceMismatch);
    }
    Ok(plus_lighter_impl(src, dst, log))
}

/// 预乘 → 直通 alpha 出口转换（**出口转换点**，F0625/F0637 登记制）。
///
/// 内部流恒预乘；仅当消费者要求直通 alpha 时才调用本函数，
/// 且调用点必须登记（[`SpaceLog::unpremultiply_count`]）。
///
/// `α = 0` 时的除零兜底：取 `0.0` 并**由调用方记账**
/// （除零不静默——`0/0 = NaN` 会毁掉下游）。
pub fn unpremultiply(v: LinearRgb, log: &mut SpaceLog) -> StraightRgb {
    log.unpremultiply_count += 1;
    let a = guard_finite(v.a);
    let inv = if a > 0.0 { 1.0 / a } else { 0.0 };
    StraightRgb {
        r: clamp01(guard_finite(v.r) * inv),
        g: clamp01(guard_finite(v.g) * inv),
        b: clamp01(guard_finite(v.b) * inv),
        a: clamp01(a),
    }
}

/// 预乘 → sRGB 编码直通（出口的完整一步：反预乘 + OETF）。
pub fn export_srgb(v: LinearRgb, log: &mut SpaceLog) -> SrgbRgb {
    let s = unpremultiply(v, log);
    SrgbRgb {
        r: linear_to_srgb(s.r),
        g: linear_to_srgb(s.g),
        b: linear_to_srgb(s.b),
        a: s.a,
    }
}

/// sRGB 编码 → 线性预乘的完整入口（`to_linear` 的记账版）。
pub fn import_srgb(v: SrgbRgb, log: &mut SpaceLog) -> LinearRgb {
    let mut t = v.to_linear();
    t.r = guard_counted(t.r, log);
    t.g = guard_counted(t.g, log);
    t.b = guard_counted(t.b, log);
    t.a = guard_counted(t.a, log);
    t
}

// ---------------------------------------------------------------------------
// 七、金标准（独立 f64，**不复用**标量路的 `fpow`）
// ---------------------------------------------------------------------------

/// `fpow` 的**独立** f64 实现（oracle 专用，自带一套分解）。
///
/// 刻意**不**调用 [`fpow`]：若共用，两侧共享同一个 `fpow`，
/// `fpow` 写错时一起错 ⇒ 对拍恒过（十诫第 7 条：自证式）。
/// 这里用**不同的分解**：`x^y = e^(y·ln x)`，`ln` 用 `2·atanh` 级数，
/// `exp` 用 `2^f` 缩放 + 泰勒，指数路径与标量路**不同**。
fn oracle_fpow(x: f64, y: f64) -> f64 {
    if !(x > 0.0) || !x.is_finite() || !y.is_finite() {
        return 0.0;
    }
    // ln x：归约到 (1,2]，m = x / 2^e
    let mut m = x;
    let mut e = 0i32;
    while m >= 2.0 {
        m *= 0.5;
        e += 1;
    }
    while m < 1.0 {
        m *= 2.0;
        e -= 1;
    }
    // ln(m) = 2·atanh((m−1)/(m+1))
    let t = (m - 1.0) / (m + 1.0);
    let t2 = t * t;
    let mut term = t;
    let mut acc = 0.0f64;
    let mut k = 0i32;
    while k < 24 {
        acc += term / (2.0 * k as f64 + 1.0);
        term *= t2;
        k += 1;
    }
    let ln_x = 2.0 * acc + e as f64 * 0.693_147_180_559_945_3;
    let target = y * ln_x;
    // e^target = 2^(target/ln2)，用 2 的幂缩放 + exp 泰勒
    let lg = target / 0.693_147_180_559_945_3;
    let ef = lg.floor();
    let fr = lg - ef;
    // 2^fr = e^(fr·ln2)
    let z = fr * 0.693_147_180_559_945_3;
    let mut term2 = 1.0f64;
    let mut acc2 = 0.0f64;
    let mut k2 = 0i32;
    while k2 < 24 {
        acc2 += term2 / fact(k2);
        term2 *= z;
        k2 += 1;
    }
    let mut out = acc2;
    let mut n = ef as i32;
    let mut guard = 0;
    while n > 0 && guard < 400 {
        out *= 2.0;
        n -= 1;
        guard += 1;
    }
    while n < 0 && guard < 800 {
        out *= 0.5;
        n += 1;
        guard += 1;
    }
    if out > 1.0e30 {
        1.0e30
    } else {
        out
    }
}

/// 阶乘（oracle 专用，`k ≤ 24` 恒不溢出）。
fn fact(k: i32) -> f64 {
    let mut r = 1.0f64;
    let mut i = 2i32;
    while i <= k {
        r *= i as f64;
        i += 1;
    }
    r
}

/// oracle 侧 sRGB EOTF（f64，独立分解）。
fn oracle_srgb_to_linear(c: f64) -> f64 {
    if !(c.is_finite()) {
        return 0.0;
    }
    if c <= 0.040_45 {
        c / 12.92
    } else {
        oracle_fpow((c + 0.055) / 1.055, 2.4)
    }
}

/// oracle 侧 sRGB OETF（f64，独立分解）。
fn oracle_linear_to_srgb(c: f64) -> f64 {
    if !(c.is_finite()) {
        return 0.0;
    }
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * oracle_fpow(c, 1.0 / 2.4) - 0.055
    }
}

/// `plus-lighter` 金标准（**编码 sRGB 空间**输入输出，内部走 f64）。
///
/// 存在的意义：让判据能在**端到端**（编码进 → 混合 → 编码出）尺度上
/// 与主实现比对。主实现内部是线性预乘，故调用方用
/// [`import_srgb`] / [`export_srgb`] 包一层即可与本函数对齐。
///
/// 公式（f64 直译规范）：`Co = min(Cs + Cb, 1)`，逐通道，
/// **直通 alpha 形式**（`αo = min(αs+αb,1)`，颜色按预乘和除以 αo）。
pub fn oracle(src: SrgbRgb, dst: SrgbRgb) -> SrgbRgb {
    let cs = [
        oracle_srgb_to_linear(src.r as f64),
        oracle_srgb_to_linear(src.g as f64),
        oracle_srgb_to_linear(src.b as f64),
    ];
    let cb = [
        oracle_srgb_to_linear(dst.r as f64),
        oracle_srgb_to_linear(dst.g as f64),
        oracle_srgb_to_linear(dst.b as f64),
    ];
    let as_ = if src.a.is_finite() { src.a as f64 } else { 0.0 };
    let ab = if dst.a.is_finite() { dst.a as f64 } else { 0.0 };
    // 预乘和（线性光）
    let mut pre = [0.0f64; 3];
    for i in 0..3 {
        let s = cs[i] * as_;
        let b = cb[i] * ab;
        let sum = s + b;
        pre[i] = if sum > 1.0 { 1.0 } else { sum };
    }
    let a_raw = as_ + ab;
    let ao = if a_raw > 1.0 {
        1.0
    } else if a_raw < 0.0 {
        0.0
    } else {
        a_raw
    };
    let inv = if ao > 0.0 { 1.0 / ao } else { 0.0 };
    SrgbRgb {
        r: oracle_linear_to_srgb(pre[0] * inv) as f32,
        g: oracle_linear_to_srgb(pre[1] * inv) as f32,
        b: oracle_linear_to_srgb(pre[2] * inv) as f32,
        a: ao as f32,
    }
}

/// 逐通道 LSB 差（判据口径，与 F0622 的 `lsb_delta` 同定义）。
pub fn lsb_delta(a: f32, b: f32) -> f32 {
    let d = a - b;
    let m = if d < 0.0 { -d } else { d };
    m / LSB8_STEP
}

/// 四通道最大 LSB 差。
pub fn out_lsb_gap(a: SrgbRgb, b: SrgbRgb) -> f32 {
    let g0 = lsb_delta(a.r, b.r);
    let g1 = lsb_delta(a.g, b.g);
    let g2 = lsb_delta(a.b, b.b);
    let g3 = lsb_delta(a.a, b.a);
    let mut m = g0;
    if g1 > m {
        m = g1;
    }
    if g2 > m {
        m = g2;
    }
    if g3 > m {
        m = g3;
    }
    m
}

// ---------------------------------------------------------------------------
// 八、宽通道（SIMD 摊薄面，服务下游 F0629）
// ---------------------------------------------------------------------------

/// 宽通道操作计数（摊薄证据）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct SimdOp {
    /// 处理的像素数。
    pub pixels: u32,
    /// 宽通道启动次数。
    pub launches: u32,
    /// 尾块标量像素数（摊薄面必须含它，否则宽通道永不启动）。
    pub tail_scalars: u32,
}

impl SimdOp {
    /// 空计数。
    pub fn new() -> SimdOp {
        SimdOp {
            pixels: 0,
            launches: 0,
            tail_scalars: 0,
        }
    }

    /// 累加。
    pub fn add(&mut self, o: &SimdOp) {
        self.pixels += o.pixels;
        self.launches += o.launches;
        self.tail_scalars += o.tail_scalars;
    }

    /// 每像素操作数（宽通道应显著 < 1）。
    pub fn per_pixel(&self) -> f32 {
        if self.pixels == 0 {
            0.0
        } else {
            self.launches as f32 / self.pixels as f32
        }
    }
}

/// 宽通道批结果。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SimdBatch {
    /// 4 像素输出。
    pub out: [LinearRgb; SIMD_WIDTH],
    /// 摊薄计数。
    pub ops: SimdOp,
}

/// 宽通道 `plus_lighter`（4 像素批）。
///
/// **公式同源**：逐像素调的仍是 [`plus_lighter_impl`]，不另写一份
/// 宽通道公式（锚点 F0629「尾块不许走简化公式」同款纪律的先声）。
pub fn plus_lighter_simd(src: [LinearRgb; SIMD_WIDTH], dst: [LinearRgb; SIMD_WIDTH]) -> SimdBatch {
    let mut out = [LinearRgb::default(); SIMD_WIDTH];
    for i in 0..SIMD_WIDTH {
        let mut log = SpaceLog::new();
        out[i] = plus_lighter_impl(src[i], dst[i], &mut log);
    }
    SimdBatch {
        out,
        ops: SimdOp {
            pixels: SIMD_WIDTH as u32,
            launches: 1,
            tail_scalars: 0,
        },
    }
}

/// 标量逐像素的计数（对照面）。
pub fn scalar_batch_ops(pixels: usize) -> SimdOp {
    SimdOp {
        pixels: pixels as u32,
        launches: pixels as u32,
        tail_scalars: pixels as u32,
    }
}

/// 宽通道摊薄比（`>1` 说明确实摊薄了启动开销）。
pub fn simd_amortization(pixels: usize) -> f32 {
    if pixels == 0 {
        return 0.0;
    }
    let scalar = scalar_batch_ops(pixels);
    let batches = (pixels + SIMD_WIDTH - 1) / SIMD_WIDTH;
    let simd_launches = batches as f32;
    if scalar.launches == 0 {
        0.0
    } else {
        simd_launches / scalar.launches as f32
    }
}

// ---------------------------------------------------------------------------
// 九、GPU 路模板（下游 F0628 的公式来源）
// ---------------------------------------------------------------------------

/// `plus-lighter` 的 WGSL 源码（模板文本，非可编译保证）。
///
/// 关键纪律（锚点 F0628「单源生成」）：**公式单源**——本函数由
/// [`AddMode::prose`] 与本文件的常量生成，**不许手抄第二份**。
pub fn wgsl_source(mode: AddMode) -> String {
    let mut s = String::new();
    s.push_str("// VE-F0624 generated blend fragment (single-source: ved24_additive)\n");
    s.push_str("// mode: ");
    s.push_str(mode.keyword());
    s.push_str("  clause: ");
    s.push_str(mode.clause());
    s.push_str("\n// space: ");
    s.push_str(mode.required_space().keyword());
    s.push_str("  (linear light ONLY)\n");
    s.push_str("fn blend_pixel(src: vec4f, dst: vec4f) -> vec4f {\n");
    s.push_str("  // premultiplied additive with value-domain clamp (spec order: add, then clamp)\n");
    s.push_str("  var acc = src + dst;\n");
    s.push_str("  acc = clamp(acc, vec4f(0.0), vec4f(1.0));\n");
    s.push_str("  return acc;\n");
    s.push_str("}\n");
    s.push_str("fn srgb_to_linear(c: f32) -> f32 {\n");
    s.push_str("  if c <= ");
    s.push_str(&EOTF_SPLIT.to_string());
    s.push_str(" { return c / ");
    s.push_str(&EOTF_LINEAR_DIV.to_string());
    s.push_str("; }\n");
    s.push_str("  return pow((c + ");
    s.push_str(&SRGB_ALPHA_OFFSET.to_string());
    s.push_str(") / ");
    s.push_str(&SRGB_SCALE.to_string());
    s.push_str(", ");
    s.push_str(&EOTF_EXPONENT.to_string());
    s.push_str(");\n");
    s.push_str("}\n");
    s.push_str("fn linear_to_srgb(c: f32) -> f32 {\n");
    s.push_str("  if c <= ");
    s.push_str(&OETF_SPLIT.to_string());
    s.push_str(" { return c * ");
    s.push_str(&OETF_LINEAR_MUL.to_string());
    s.push_str("; }\n");
    s.push_str("  return ");
    s.push_str(&SRGB_SCALE.to_string());
    s.push_str(" * pow(c, ");
    s.push_str(&(OETF_EXPONENT_NUM / EOTF_EXPONENT).to_string());
    s.push_str(") - ");
    s.push_str(&SRGB_ALPHA_OFFSET.to_string());
    s.push_str(";\n");
    s.push_str("}\n");
    s
}

/// WGSL 自检（判据断"只吐壳"）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WgslSelfCheck {
    /// 含模式关键字。
    pub has_keyword: bool,
    /// 含规范条款号。
    pub has_clause: bool,
    /// 含线性空间声明。
    pub has_space: bool,
    /// 含传输函数两条。
    pub has_transfer: bool,
    /// 含值域裁剪。
    pub has_clamp: bool,
}

impl WgslSelfCheck {
    /// 是否全部具备。
    pub fn complete(&self) -> bool {
        self.has_keyword
            && self.has_clause
            && self.has_space
            && self.has_transfer
            && self.has_clamp
    }
}

/// WGSL 文本自检。
pub fn wgsl_selfcheck(mode: AddMode) -> WgslSelfCheck {
    let src = wgsl_source(mode);
    WgslSelfCheck {
        has_keyword: src.contains(mode.keyword()),
        has_clause: src.contains(mode.clause()),
        has_space: src.contains(mode.required_space().keyword()),
        has_transfer: src.contains("srgb_to_linear") && src.contains("linear_to_srgb"),
        has_clamp: src.contains("clamp"),
    }
}

// ---------------------------------------------------------------------------
// 十、色卡 / 场景 / 候选 文本行（无障碍：ASCII 灰阶是文字不是色块）
// ---------------------------------------------------------------------------

/// 色卡行（`plus-lighter` 在共用斜坡探针上的结果）。
pub fn swatch_rows() -> Vec<String> {
    let mut out = Vec::new();
    let mut idx = 0usize;
    for p in SWATCH_PROBES.iter() {
        // 探针 (Cb, Cs)：以灰阶代入，主路与金标准各跑一遍。
        let src = SrgbRgb::gray(p.1);
        let dst = SrgbRgb::gray(p.0);
        let mut log = SpaceLog::new();
        let lin = import_srgb(src, &mut log);
        let lin_b = import_srgb(dst, &mut log);
        let out_lin = plus_lighter(AddMode::PlusLighter, lin, lin_b, &mut log);
        let got = export_srgb(out_lin, &mut log);
        let want = oracle(src, dst);
        let gap = out_lsb_gap(got, want);
        out.push(format!(
            "swatch[{}] cb={:.3} cs={:.3} got=({:.3},{:.3},{:.3}) ramp={}{} lsb={:.2}",
            idx,
            p.0,
            p.1,
            got.r,
            got.g,
            got.b,
            ramp_char(got.r),
            ramp_char(got.b),
            gap
        ));
        idx += 1;
    }
    out
}

/// 场景登记行。
pub fn scene_rows() -> Vec<String> {
    let mut out = Vec::new();
    for u in UseCase::all().iter() {
        out.push(format!(
            "scene={} path={} additive={} registered={} why={}",
            u.name,
            u.recommended_path,
            u.is_additive(),
            u.path_registered(),
            u.why
        ));
    }
    out
}

/// 候选登记行。
pub fn candidate_rows() -> Vec<String> {
    let mut out = Vec::new();
    for c in CandidateMode::all().iter() {
        out.push(format!(
            "candidate={} status={} specified={} honest={} rationale_len={}",
            c.keyword,
            c.status.keyword(),
            c.is_specified(),
            c.registry_honest(),
            c.rationale.len()
        ));
    }
    out
}

/// 误选提示行。
pub fn mispick_rows() -> Vec<String> {
    let mut out = Vec::new();
    for m in MisPick::all().iter() {
        out.push(format!(
            "mispick where={} use={} triad={} target={}",
            m.where_wrong,
            m.use_instead,
            m.triad_complete(),
            m.target_registered()
        ));
    }
    out
}

/// 对拍台账（色卡 × 模式的最坏 LSB 差）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CrossCheck {
    /// 最坏逐通道 LSB 差。
    pub worst: f32,
    /// 是否落在预算内。
    pub within_budget: bool,
    /// 语料条数（供"独立重算用例数并断恰等于"）。
    pub cases: u32,
    /// 是否存在**区分性语料**（主路与金标准差异 > 0，
    /// 全同即语料退化——十诫第 11 条）。
    pub has_discriminating: bool,
}

/// 全色卡对拍（主路 vs 金标准）。
pub fn crosscheck() -> CrossCheck {
    let mut worst = 0.0f32;
    let mut cases = 0u32;
    let mut discriminating = false;
    for p in SWATCH_PROBES.iter() {
        for variant in 0..3usize {
            // 三个变体：灰、互补色相、纯色相偏移（避免语料全灰导致
            // "加法在灰轴上恰好与别的一致"的退化）。
            let (src, dst) = probe_colors(*p, variant);
            let mut log = SpaceLog::new();
            let lin = import_srgb(src, &mut log);
            let lin_b = import_srgb(dst, &mut log);
            let out_lin = plus_lighter(AddMode::PlusLighter, lin, lin_b, &mut log);
            let got = export_srgb(out_lin, &mut log);
            let want = oracle(src, dst);
            let gap = out_lsb_gap(got, want);
            if gap > worst {
                worst = gap;
            }
            if gap > 0.0 {
                discriminating = true;
            }
            cases += 1;
        }
    }
    CrossCheck {
        worst,
        within_budget: worst <= LSB_LIMIT,
        cases,
        has_discriminating: discriminating,
    }
}

/// 探针色彩构造（变体 0=灰、1=色相互补、2=单色相偏移）。
fn probe_colors(p: (f32, f32), variant: usize) -> (SrgbRgb, SrgbRgb) {
    match variant {
        0 => (SrgbRgb::gray(p.1), SrgbRgb::gray(p.0)),
        1 => (
            SrgbRgb::new_encoded(p.1, 1.0 - p.0, p.0, 1.0),
            SrgbRgb::new_encoded(p.0, 1.0 - p.1, p.1, 0.5),
        ),
        _ => (
            SrgbRgb::new_encoded(p.1, p.1, 1.0 - p.0, 1.0),
            SrgbRgb::new_encoded(p.0, 0.5, p.1, 0.5),
        ),
    }
}

/// 「加法不是叠加」探针（锚点第五点）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AdditionVsOverlay {
    /// `plus_lighter` 结果。
    pub additive: SrgbRgb,
    /// F0622 `normal` 结果。
    pub normal: SrgbRgb,
    /// 四通道最大 LSB 差（**必须 > 0** 才叫"不同"）。
    pub gap: f32,
    /// alpha 和（供判据确认语料确实是"半透明叠加"场景）。
    pub alpha_sum: f32,
}

/// 同语料双路对照。
pub fn addition_vs_overlay(src: SrgbRgb, dst: SrgbRgb) -> AdditionVsOverlay {
    let mut log = SpaceLog::new();
    let lin = import_srgb(src, &mut log);
    let lin_b = import_srgb(dst, &mut log);
    let add_lin = plus_lighter(AddMode::PlusLighter, lin, lin_b, &mut log);
    let additive = export_srgb(add_lin, &mut log);

    // F0622 normal：走其 `eval_pixel_rgba`（通道与 alpha 分开给，
    // alpha 原样透传——可分离模式不吃 alpha，alpha 覆盖归 F0625）。
    let mut dlog = DivZeroLog::new();
    let normal = sep_normal(src, dst, &mut dlog);

    AdditionVsOverlay {
        additive,
        normal,
        gap: out_lsb_gap(additive, normal),
        alpha_sum: src.a + dst.a,
    }
}

/// 调 F0622 的 `normal`（标量路），作为「加法 ≠ 叠加」的对照面。
fn sep_normal(src: SrgbRgb, dst: SrgbRgb, log: &mut DivZeroLog) -> SrgbRgb {
    // F0622 语义：`eval_scalar(Normal, cb, cs)` 即 `cs`（源覆盖），
    // alpha 不参与 RGB。本条对照时给**合成后**的 alpha（源覆盖语义）。
    let px = eval_pixel_rgba(
        SepMode::Normal,
        [dst.r, dst.g, dst.b],
        [src.r, src.g, src.b],
        src.a + dst.a * (1.0 - src.a),
        log,
    );
    SrgbRgb {
        r: px[0],
        g: px[1],
        b: px[2],
        a: px[3],
    }
}

// ---------------------------------------------------------------------------
// 十一、判据装配
// ---------------------------------------------------------------------------

/// VE-F0624 判据集（20 条）。
pub fn run_ved24_checks() -> CheckSet {
    let mut s = CheckSet::new("ved24");

    // --- 判据 1：线性前提断言（**双向**：线性过、sRGB 拦） ----------
    // 抓错：断言只做了一半——只断"线性输入通过"的话，把断言整个
    // 删掉（恒 `Ok`）判据仍全绿（十诫第 5 条：断言两侧同值）。
    {
        let mut log = SpaceLog::new();
        let lin = LinearRgb::gray(0.25);
        let ok_linear = plus_lighter_checked(
            lin,
            lin,
            SpaceTag::LinearLight,
            SpaceTag::LinearLight,
            SpaceTag::LinearLight,
            &mut log,
        );
        // 标记为编码空间 ⇒ 必须拦截，且**不产生结果**。
        let enc = plus_lighter_checked(
            lin,
            lin,
            SpaceTag::SrgbEncoded,
            SpaceTag::LinearLight,
            SpaceTag::LinearLight,
            &mut log,
        );
        // 两个**不同的线性态**混用（软件线性 vs 硬件 sRGB 帧缓冲）⇒归 mismatch。
        // 这是该码的语义所在，必须有语料验证它可达，否则它会退化为
        // 「有 label 但不可达」的死码（十诫第 13 条）。
        let mixed_linear = plus_lighter_checked(
            lin,
            lin,
            SpaceTag::LinearLight,
            SpaceTag::HardwareSrgbLinear,
            SpaceTag::LinearLight,
            &mut log,
        );
        // 标记与声明不符 ⇒ 必须拦截（缓存里存了别的表示）。
        let mismatch = plus_lighter_checked(
            lin,
            lin,
            SpaceTag::HardwareSrgbLinear,
            SpaceTag::LinearLight,
            SpaceTag::LinearLight,
            &mut log,
        );
        // 声明本身是编码空间 ⇒ 必须归到encoded 码（不得被 mismatch 遮蔽）。
        let declared_enc = plus_lighter_checked(
            lin,
            lin,
            SpaceTag::LinearLight,
            SpaceTag::LinearLight,
            SpaceTag::SrgbEncoded,
            &mut log,
        );
        s.add(
            "C24-LINEAR-PREMISE-ASSERTED",
            ok_linear.is_ok()
                && enc == Err(SpaceViolation::EncodedInputRejected)
                && mixed_linear == Err(SpaceViolation::TagSpaceMismatch)
                && mismatch == Err(SpaceViolation::TagSpaceMismatch)
                && declared_enc == Err(SpaceViolation::EncodedInputRejected)
                && log.space_violations == 4,
            "线性输入通过；编码标记拦截、两个线性态混用归 mismatch、\
             声明为编码归 encoded（不被遮蔽），且违规恰记 4 次（不多不少）",
        );
    }

    // --- 判据 2：公式对拍金标准 --------------------------------------
    let cc = crosscheck();
    s.add(
        "C24-FORMULA-VS-ORACLE",
        cc.within_budget && cc.has_discriminating,
        "色卡×3 变体全量对拍 f64 金标准，最坏逐通道差 ≤1 LSB，\
         且语料含真差异点（全同即语料退化）",
    );

    // --- 判据 3：oracle 独立性（反自证式） --------------------------
    // 抓错：oracle 复用标量路的 `fpow` ⇒ `fpow` 写错时两侧一起错，
    // 对拍恒过。判据直接断**两路在已知点上给出不同量级的偏差**：
    // 用一个标量路 `fpow` 精度不足而 oracle 精确的点，验证 f64 路
    // 往返误差显著更小。
    {
        // 在 OETF 幂段中部取点：标量路（f32 + 8 项级数）的往返误差
        // 应落在 1 LSB 内，而 oracle（f64 + 24 项）在同一遍历上**不更差**。
        // 判据断的是"oracle 不劣于标量路"——若 oracle 复用 f32 路径，
        // 两序列将**逐位相同**，此时 `exact_match` 为真而判据要求为假。
        let mut f32_worst = 0.0f32;
        let mut f64_worst = 0.0f32;
        let mut exact_match = true;
        let mut i = 0usize;
        while i <= 32usize {
            let c = i as f32 / 32.0;
            let a = transport_roundtrip_lsb(c);
            // oracle 侧往返（f64 → f32 落地）
            let back = oracle_linear_to_srgb(oracle_srgb_to_linear(c as f64)) as f32;
            let d = back - c;
            let b = if d < 0.0 { -d } else { d } / LSB8_STEP;
            if a > f32_worst {
                f32_worst = a;
            }
            if b > f64_worst {
                f64_worst = b;
            }
            if (a - b).abs() > 1.0e-6 {
                exact_match = false;
            }
            i += 1;
        }
        s.add(
            "C24-ORACLE-INDEPENDENT",
            f32_worst <= LINEAR_ROUNDTRIP_LSB
                && f64_worst <= LINEAR_ROUNDTRIP_LSB
                && f64_worst <= f32_worst + LSB8_STEP
                && !exact_match,
            "标量路与 oracle 路往返均 ≤1 LSB；oracle 不劣于标量路；\
             且两路**逐点不全等**（全等即共用同一实现，自证式）",
        );
    }

    // --- 判据 4：先加后裁（越界语料打边加边裁变体） ----------------
    // 抓错：把 `min(Cs+Cb,1)` 写成 `min(min(Cs,1)+min(Cb,1),1)`。
    // 定义域内两者**完全等价**⇒正常语料不可观测，必须用越界输入。
    {
        let mut log = SpaceLog::new();
        let over = LinearRgb::new_linear(0.8, 1.4, 2.0, 1.0);
        let bg = LinearRgb::new_linear(0.7, 0.3, 0.1, 0.5);
        let out = plus_lighter(AddMode::PlusLighter, over, bg, &mut log);
        // 正确版：0.8+0.7=1.5→1.0；1.4+0.3=1.7→1.0；2.0+0.1=2.1→1.0
        // 变体（边加边裁）：min(0.8,1)+min(0.7,1)=1.5→1.0（此处恰好相同）
        // 故另取一通道使两者分歧：Cs=-0.2（越下界）、Cb=0.9
        let neg = LinearRgb::new_linear(-0.2, 0.5, 0.5, 1.0);
        let bg2 = LinearRgb::new_linear(0.9, 0.5, 0.5, 1.0);
        let out2 = plus_lighter(AddMode::PlusLighter, neg, bg2, &mut log);
        // 正确版：-0.2+0.9=0.7 ⇒ 0.7；变体：max(min(-0.2,1),0)+0.9 = 0.9 ⇒ 不同
        let correct_differs = out2.r < out2.g && (out2.r - 0.7).abs() < 1.0e-5;
        s.add(
            "C24-CLAMP-AFTER-ADD",
            out.r == VALUE_HI
                && out.g == VALUE_HI
                && out.b == VALUE_HI
                && out.a == VALUE_HI
                && correct_differs,
            "越界输入三通道与 alpha 全部裁到 1；负分量与背景相加得 0.7（\
             先加后裁），而边加边裁会得 0.9——用负越界语料钉死次序",
        );
    }

    // --- 判据 5：alpha 与颜色裁剪同步 -------------------------------
    // 抓错：alpha 先裁再反预乘除法 ⇒ 颜色被放大（除以 1.0 而非真实和）。
    {
        // αs+αb 恰为 1.0：裁剪不触发，除数为 1.0，颜色应等于预乘和。
        let mut log = SpaceLog::new();
        let s1 = LinearRgb::new_linear(0.4, 0.4, 0.4, 0.5);
        let d1 = LinearRgb::new_linear(0.2, 0.2, 0.2, 0.5);
        let o1 = plus_lighter(AddMode::PlusLighter, s1, d1, &mut log);
        let s1_ok = (o1.r - 0.6).abs() < 1.0e-6 && (o1.a - 1.0).abs() < 1.0e-6;
        // αs+αb = 1.5：裁剪触发。若除数用 1.0（同步裁剪，正确）则
        // 颜色仍是预乘和；若先用未裁的 αo=1.5 做除法再裁，颜色会缩小。
        let s2 = LinearRgb::new_linear(0.4, 0.4, 0.4, 0.5);
        let d2 = LinearRgb::new_linear(0.2, 0.2, 0.2, 1.0);
        let o2 = plus_lighter(AddMode::PlusLighter, s2, d2, &mut log);
        // 预乘和 = 0.6，颜色不因 α 裁剪而变（预乘流里颜色不除 α）
        let s2_ok = (o2.r - 0.6).abs() < 1.0e-6 && o2.a == VALUE_HI;
        s.add(
            "C24-ALPHA-CLAMP-COORDINATED",
            s1_ok && s2_ok && log.alpha_clamped >= 1,
            "α 和恰 1.0 时不裁且颜色=预乘和；α 和 1.5 时 alpha 裁到 1 而\
             预乘颜色**不**被除小（预乘流纪律），alpha 裁剪有记账",
        );
    }

    // --- 判据 6：预乘相加 alpha（不是 αs 也不是 1） -----------------
    // 抓错：alpha 取 `αs` 或恒 `1.0`。用两组不同 α 的语料区分。
    {
        let mut log = SpaceLog::new();
        let src = LinearRgb::new_linear(0.1, 0.1, 0.1, 0.25);
        let dst = LinearRgb::new_linear(0.1, 0.1, 0.1, 0.5);
        let o = plus_lighter(AddMode::PlusLighter, src, dst, &mut log);
        // 正确：αo = 0.75。若取 αs ⇒ 0.25；若恒 1 ⇒ 1.0。
        let s_ok = (o.a - 0.75).abs() < 1.0e-6
            && (o.a - 0.25).abs() > 1.0e-3
            && (o.a - 1.0).abs() > 1.0e-3;
        s.add(
            "C24-PREMULTIPLIED-ADD-ALPHA",
            s_ok,
            "αo = αs+αb = 0.75，且与 αs(0.25)、恒 1.0 三者互不相等",
        );
    }

    // --- 判据 7：sRGB↔线性往返误差 ----------------------------------
    {
        let mut worst = 0.0f32;
        let mut i = 0usize;
        while i <= 64usize {
            let c = i as f32 / 64.0;
            let e = transport_roundtrip_lsb(c);
            if e > worst {
                worst = e;
            }
            i += 1;
        }
        s.add(
            "C24-TRANSPORT-ROUNDTRIP",
            worst <= LINEAR_ROUNDTRIP_LSB,
            "65 点往返最坏误差 ≤1 LSB（8 bit 量化分辨率内即不可观测）",
        );
    }

    // --- 判据 8：两个换段断点（**逐字核对 + 夹逼对**双管） ---------------
    // 抓错：EOTF/OETF 断点写错（如 0.04045 写成 0.04）。
    //
    // **本条初版就是个弱门禁，变异实测抓到过**：初版只验「断点两侧
    // ±δ 夹逼连续」，而把断点改成 0.04 之后**两版都连续**
    // （sRGB 传输函数本来就是连续可导的），于是变异 MISSED。
    //
    // 根因：连续性判据抓的是「换段处有没有缝」，而这个缺陷是
    // 「缝的位置偏了」——连续性对位置偏移**完全不敏感**。
    // 连续性判据并非无用（它抓两段公式在交点处不自洽），但它
    // **不能单独**承担「断点位置正确」这件事。
    //
    // 补法（与 F0623 核对 Lum 权重同款）：把断点做成**可核对的常量**
    // 并断言其**逐字**等于规范值，且 oracle 侧**独立硬写**同一数字——
    // 只断常量的话，改常量的人也会顺手改判据里的期望（自证式）；
    // oracle 侧硬写才是独立的一侧。
    {
        // 规范原文值（硬写，不引用常量——引用则改常量时两侧一起变）。
        const SPEC_EOTF_SPLIT: f32 = 0.04045;
        const SPEC_OETF_SPLIT: f32 = 0.0031308;
        const SPEC_EOTF_DIV: f32 = 12.92;
        const SPEC_OETF_MUL: f32 = 12.92;

        // (a) 逐字核对：常量必须与规范字面量**完全相等**（不用近似比较——
        // 这两个数是十进制精确值，f32 能精确表示，改动必然可检出）。
        let verbatim = EOTF_SPLIT == SPEC_EOTF_SPLIT
            && OETF_SPLIT == SPEC_OETF_SPLIT
            && EOTF_LINEAR_DIV == SPEC_EOTF_DIV
            && OETF_LINEAR_MUL == SPEC_OETF_MUL;

        // (b) oracle 侧的断点**取值合理性** + 与标量路同向。
        //
        // 这里有一处必须说清的**可观测性边界**：0.04045 与 0.04 两条
        // 分支在断点附近本就近乎重合（实测两式在 0.04045 处差
        // ≈1.3e-6，约 0.0003 LSB），所以**断点偏移在输出上原理不可观测**
        // ——对拍、往返、夹逼三样都抓不到它。这正是 (a) 存在的理由：
        // 断点是**参数**，其正确性是**规范符合性**问题，只能逐字核对。
        //
        // oracle 侧的断点硬写在其函数体内（判据侧读不到字面量），
        // 故这里只断"取值在合理量级且与标量路同向"——抓的是 gross
        // 错误（如 oracle 用了 0.5 或 0.005 这类量级错），不假装能
        // 抓 0.04045/0.04 级别的偏移。
        let oracle_split_plausible = oracle_srgb_to_linear(0.02f64) > 0.0
            && oracle_srgb_to_linear(0.05f64) > oracle_srgb_to_linear(0.02f64)
            && oracle_linear_to_srgb(0.002f64) > 0.0
            && oracle_linear_to_srgb(0.02f64) < oracle_linear_to_srgb(0.05f64)
            && (oracle_srgb_to_linear(0.02f64) - 0.02f64 / 12.92).abs() < 1.0e-9
            && (oracle_linear_to_srgb(0.002f64) - 0.002f64 * 12.92).abs() < 1.0e-9;

        // (c) 夹逼连续（保留原判据：抓两段公式在交点不自洽）。
        let d = 1.0e-6; // 远大于 f32 在该量级的 1 ULP
        let e_gap = (srgb_to_linear(EOTF_SPLIT + d) - srgb_to_linear(EOTF_SPLIT - d)).abs();
        let e_at = srgb_to_linear(EOTF_SPLIT);
        let e_lin = EOTF_SPLIT / EOTF_LINEAR_DIV;
        let e_ok = e_gap < 1.0e-4 && (e_at - e_lin).abs() < 1.0e-6;

        let od = d * 0.01;
        let o_gap = (linear_to_srgb(OETF_SPLIT + od) - linear_to_srgb(OETF_SPLIT - od)).abs();
        let o_at = linear_to_srgb(OETF_SPLIT);
        let o_lin = OETF_SPLIT * OETF_LINEAR_MUL;
        let o_ok = o_gap < 1.0e-4 && (o_at - o_lin).abs() < 1.0e-6;

        s.add(
            "C24-TRANSPORT-SEGMENT-SPLIT",
            verbatim && oracle_split_plausible && e_ok && o_ok,
            "断点常量逐字等于规范值 0.04045 / 0.0031308（`==` 非近似，\
             断点偏移输出不可观测故只能逐字核对）；oracle 侧断点取值在\
             合理量级且与标量路同向；断点两侧 ±δ 夹逼连续\
             （连续性抓交点不自洽，位置由逐字核对负责）",
        );
    }

    // --- 判据 9：`fpow` 对已知数值 ------------------------------------
    // 抓错：`fpow` 自身错（级数项数不足 / 归约区间写错）。
    // 断言用**规范可算的精确值**：x^0=1、x^1=x、1^y=1、0.5^2=0.25。
    {
        let mut ok = true;
        ok = ok && (fpow(0.37, 0.0) - 1.0).abs() < 1.0e-5;
        ok = ok && (fpow(0.37, 1.0) - 0.37).abs() < 1.0e-6;
        ok = ok && (fpow(1.0, 2.4) - 1.0).abs() < 1.0e-6;
        ok = ok && (fpow(0.5, 2.0) - 0.25).abs() < 1.0e-5;
        // 2.4 与 1/2.4 互逆
        let rt = fpow(fpow(0.42, 2.4), 1.0 / 2.4);
        ok = ok && (rt - 0.42).abs() < 1.0e-4;
        s.add(
            "C24-FPOW-KNOWN-VALUES",
            ok,
            "fpow 对 x^0=1、x^1=x、1^y=1、0.5^2=0.25 四个精确值成立，\
             且 (x^2.4)^(1/2.4) 往返回 x",
        );
    }

    // --- 判据 10：候选登记如实 ----------------------------------------
    // 抓错：把未收录模式标成规范模式（会让 F0628 为不存在的条款生成
    // 着色器，且 F0630 拿自造金标准对拍⇒自证式）。
    {
        let cands = CandidateMode::all();
        let mut honest = true;
        let mut dup_free = true;
        let mut i = 0usize;
        while i < cands.len() {
            let c = cands[i];
            if !c.registry_honest() {
                honest = false;
            }
            if c.status.requires_rationale() && c.rationale.is_empty() {
                honest = false;
            }
            let mut j = i + 1;
            while j < cands.len() {
                if cands[j].keyword == c.keyword {
                    dup_free = false;
                }
                j += 1;
            }
            i += 1;
        }
        // plus-lighter 必须且仅有一个 Specified
        let specified_ok = specified_count_from_registry() == SPECIFIED_COUNT
            && cands[0].keyword == "plus-lighter"
            && cands[0].is_specified();
        // 候选不得冒名规范键
        let no_conflict = !cands.iter().any(|c| candidate_key_conflicts(c.keyword)
            && c.keyword != "plus-lighter");
        s.add(
            "C24-CANDIDATE-REGISTRY-HONEST",
            honest && dup_free && specified_ok && no_conflict,
            "4 条候选全部登记如实：未收录者必带理由、关键字无重复、\
             plus-lighter 是唯一 Specified、其余不冒名规范键",
        );
    }

    // --- 判据 11：场景推荐路径已登记 ---------------------------------
    {
        let cases = UseCase::all();
        let mut ok = true;
        let mut additive_seen = 0usize;
        let mut i = 0usize;
        while i < cases.len() {
            let u = cases[i];
            if !u.path_registered() {
                ok = false;
            }
            if u.why.is_empty() {
                ok = false;
            }
            if u.is_additive() {
                additive_seen += 1;
                if u.recommended_path != "plus-lighter" {
                    ok = false;
                }
            }
            i += 1;
        }
        s.add(
            "C24-USECASE-PATH-REGISTERED",
            ok && additive_seen == 2 && cases.len() == 4,
            "4 条场景推荐路径全部指向已登记模式、理由非空；\
             2 条加法类场景均推荐 plus-lighter，高光叠加反例推荐 screen",
        );
    }

    // --- 判据 12：加法不是叠加（与 F0622 normal 同值即退化） ---------
    {
        // 半透明叠加语料：两者结果必须显著不同。
        let src = SrgbRgb::new_encoded(0.6, 0.2, 0.8, 0.5);
        let dst = SrgbRgb::new_encoded(0.3, 0.7, 0.4, 0.6);
        let cmp = addition_vs_overlay(src, dst);
        // 第二组：两者皆不透明但颜色相反（加法必饱和到 1，normal 必近似 Cs）
        let src2 = SrgbRgb::new_encoded(0.9, 0.1, 0.1, 1.0);
        let dst2 = SrgbRgb::new_encoded(0.1, 0.9, 0.1, 1.0);
        let cmp2 = addition_vs_overlay(src2, dst2);
        s.add(
            "C24-ADDITION-NOT-SOURCE-OVER",
            cmp.gap > 8.0 * LSB8_STEP
                && cmp2.gap > 8.0 * LSB8_STEP
                && cmp.alpha_sum > 1.0,
            "半透明叠加语料下 plus_lighter 与 F0622 normal 相差 >8 LSB；\
             全不透明互补色语料同样显著不同——加法不是源覆盖",
        );
    }

    // --- 判据 13：宽通道与标量等价 -----------------------------------
    {
        let src = [
            LinearRgb::new_linear(0.1, 0.2, 0.3, 0.5),
            LinearRgb::new_linear(0.9, 0.1, 0.4, 1.0),
            LinearRgb::new_linear(0.0, 0.0, 0.0, 0.25),
            LinearRgb::new_linear(0.6, 0.6, 0.6, 0.75),
        ];
        let dst = [
            LinearRgb::new_linear(0.5, 0.5, 0.5, 0.5),
            LinearRgb::new_linear(0.2, 0.3, 0.4, 0.0),
            LinearRgb::new_linear(1.0, 1.0, 1.0, 1.0),
            LinearRgb::new_linear(0.3, 0.2, 0.1, 0.25),
        ];
        let batch = plus_lighter_simd(src, dst);
        let mut same = true;
        let mut i = 0usize;
        while i < SIMD_WIDTH {
            let mut log = SpaceLog::new();
            let scalar = plus_lighter(AddMode::PlusLighter, src[i], dst[i], &mut log);
            if batch.out[i] != scalar {
                same = false;
            }
            i += 1;
        }
        s.add(
            "C24-SIMD-BATCH-EQUIV",
            same && batch.ops.pixels == SIMD_WIDTH as u32 && batch.ops.launches == 1,
            "宽通道 4 像素与标量逐像素**逐位相等**；1 次启动处理 4 像素",
        );
    }

    // --- 判据 14：宽通道摊薄 ------------------------------------------
    {
        let a = simd_amortization(64);
        let b = simd_amortization(8);
        s.add(
            "C24-SIMD-AMORTIZATION",
            a > 0.0 && a < 0.3 && b < 1.0 && scalar_batch_ops(64).launches == 64,
            "64 像素宽通道启动数为标量的 <0.3（确实摊薄）；8 像素档 <1；\
             标量基线 64 像素 64 次启动",
        );
    }

    // --- 判据 15：WGSL 文本非空壳 -------------------------------------
    {
        let w = wgsl_selfcheck(AddMode::PlusLighter);
        s.add(
            "C24-WGSL-TEXT-KEYWORDS",
            w.complete(),
            "GPU 路模板含模式关键字、规范条款号、线性空间声明、\
             传输函数两条与值域裁剪（非空壳）",
        );
    }

    // --- 判据 16：误选提示三要素齐全 ---------------------------------
    {
        let ms = MisPick::all();
        let mut ok = ms.len() == 3;
        let mut i = 0usize;
        while i < ms.len() {
            if !ms[i].triad_complete() || !ms[i].target_registered() {
                ok = false;
            }
            i += 1;
        }
        // 其中一条必须指向线性空间（锚点「非线性误用」）
        let has_space = ms
            .iter()
            .any(|m| m.use_instead == "linear-light");
        s.add(
            "C24-MISPICK-Triad",
            ok && has_space,
            "3 条误选提示三要素齐全、目标关键字已登记；\
             其中一条指向 linear-light（覆盖非线性空间误用）",
        );
    }

    // --- 判据 17：非有限输入不静默传播 -------------------------------
    {
        let mut log = SpaceLog::new();
        let before = log.nonfinite_guarded;
        let nan_src = LinearRgb::new_linear(f32::NAN, 0.5, 0.5, 1.0);
        let inf_dst = LinearRgb::new_linear(f32::INFINITY, 0.5, 0.5, 1.0);
        let out = plus_lighter(AddMode::PlusLighter, nan_src, inf_dst, &mut log);
        let finite = out.r.is_finite() && out.g.is_finite() && out.b.is_finite();
        // 计数**独立重算**：源 NaN 记 1、背景 Inf 记 1，其余 6 个分量
        // 皆有限 ⇒ 恰 +2。用 `==` 而非 `>=`（`>=` 抓不住"多记了"
        // 这类缺陷——如把兜底重复记两次的实现会照过）。
        let counted = log.nonfinite_guarded == before + 2;
        // 值也独立重算：兜底后源 r=0、背景 r=0 ⇒ 和为 0（未溢出故不裁）。
        let value_ok = (out.r - 0.0).abs() < 1.0e-6;
        // g: 0.5+0.5 = 1.0（恰在边界，不裁亦为 1.0）
        let g_ok = (out.g - 1.0).abs() < 1.0e-6;
        // clamp01 单独打非有限：`min(NaN,1)` 在 f32 下会返回非 NaN 的
        // 那个操作数（静默吞掉 NaN），故必须先兜底再裁。
        let c_nan = clamp01(f32::NAN);
        let c_inf = clamp01(f32::NEG_INFINITY);
        let c_pos = clamp01(f32::INFINITY);
        s.add(
            "C24-CLAMP-NON-FINITE",
            finite
                && counted
                && value_ok
                && g_ok
                && c_nan == VALUE_LO
                && c_inf == VALUE_LO
                && c_pos == VALUE_HI,
            "NaN/±Inf 输入经兜底后输出有限；兜底计数独立重算恰 +2（不多记不少记）；\
             输出值独立重算（r=0、g=1）；clamp01 对 NaN→0、−Inf→0、+Inf→1",
        );
    }

    // --- 判据 18：色卡与 F0622 共用（真引用） --------------------------
    {
        let rows = swatch_rows();
        s.add(
            "C24-SWATCH-SHARED-PROBES",
            rows.len() == SWATCH_STEPS
                && SWATCH_STEPS == SWATCH_PROBES.len()
                && RAMP.len() == 10
                && SPECIFIED_COUNT == 1,
            "色卡行数 = F0622 的 SWATCH_PROBES 探针数（真引用）；\
             灰阶字符全在共用 RAMP 内；规范模式 1 个",
        );
    }

    // --- 判据 19：越界键返回 None --------------------------------------
    {
        let ok = AddMode::from_index(1).is_none()
            && AddMode::from_index(usize::MAX).is_none()
            && AddMode::from_index(0).is_some()
            && LinearRgb::gray(0.5).get(9) == VALUE_LO
            && SrgbRgb::gray(0.5).get(9) == VALUE_LO
            && ramp_char(-1.0) == RAMP[0]
            && ramp_char(2.0) == RAMP[9];
        s.add(
            "C24-KEY-OUT-OF-RANGE-NONE",
            ok,
            "越界模式键返回 None 不 panic；色彩分量越界下标返回 0 不 panic；\
             灰阶取值越界夹到两端不 panic",
        );
    }

    // --- 判据 20：零静默——台账可核对 ---------------------------------
    // 抓错：记账项被吞。逐项断言：构造输入必须让对应计数**恰好**增 1。
    {
        let mut log = SpaceLog::new();
        let clean = log.total();
        // 干净输入（分量皆在域内、和皆不越界）⇒ 台账必须为空。
        let _ = plus_lighter(
            AddMode::PlusLighter,
            LinearRgb::new_linear(0.2, 0.2, 0.2, 0.2),
            LinearRgb::new_linear(0.2, 0.2, 0.2, 0.2),
            &mut log,
        );
        let after_clean = log.total();
        // 单通道溢出 ⇒ clamped_channels 恰 +1
        let c_before = log.clamped_channels;
        let _ = plus_lighter(
            AddMode::PlusLighter,
            LinearRgb::new_linear(0.8, 0.1, 0.1, 0.1),
            LinearRgb::new_linear(0.5, 0.1, 0.1, 0.1),
            &mut log,
        );
        let clamp_ok = log.clamped_channels == c_before + 1;
        // alpha 溢出 ⇒ alpha_clamped 恰 +1
        let a_before = log.alpha_clamped;
        let _ = plus_lighter(
            AddMode::PlusLighter,
            LinearRgb::new_linear(0.1, 0.1, 0.1, 0.8),
            LinearRgb::new_linear(0.1, 0.1, 0.1, 0.5),
            &mut log,
        );
        let alpha_ok = log.alpha_clamped == a_before + 1;
        // 出口转换 ⇒ unpremultiply_count 恰 +1
        let u_before = log.unpremultiply_count;
        let _ = export_srgb(LinearRgb::gray(0.5), &mut log);
        let un_ok = log.unpremultiply_count == u_before + 1;
        // 非有限 ⇒ nonfinite_guarded 恰 +2（两个 NaN 分量）
        let n_before = log.nonfinite_guarded;
        let _ = plus_lighter(
            AddMode::PlusLighter,
            LinearRgb::new_linear(f32::NAN, f32::NAN, 0.1, 0.1),
            LinearRgb::new_linear(0.1, 0.1, 0.1, 0.1),
            &mut log,
        );
        let nonfinite_ok = log.nonfinite_guarded == n_before + 2;
        s.add(
            "C24-LEDGER-EXACT",
            clean == 0 && after_clean == 0 && clamp_ok && alpha_ok && un_ok && nonfinite_ok,
            "干净输入台账为空（无幽灵记账）；单通道溢出恰 +1、alpha 溢出恰 +1、\
             出口转换恰 +1、两个 NaN 分量恰 +2",
        );
    }

    s
}

// ---------------------------------------------------------------------------
// 十二、反假变体登记（变异测试实测结果；判据聚合器不读本表，仅作文档）
// ---------------------------------------------------------------------------

/// 反假变体登记表。
///
/// **为什么必须登记**：判据全绿只证明"当前实现合判据"。若某条判据在
/// 原理上无法被某类缺陷转红，它就是弱门禁。本表记录每个变异**实际捕获
/// 的判据**，任何人改判据时可用同一变异集复核——若某变异改完仍全绿，
/// 说明该判据已退化为恒真。
///
/// 本轮实测（隔离探针，基线 20/20 绿）：**9/9 变异全部被捕获**。
///
/// 其中 **V5 初测 MISSED**（断点 0.04045 → 0.04 未被捕获）：初版判据
/// 只验「断点两侧夹逼连续」，而传输函数本就连续可导，两版都连续。
/// 补法是加逐字 `==` 规范值 + oracle 侧独立硬写，补后捕获。
/// 另有 **V1 初测 MISSED 属变异选错**（十诫第 14 条）：初版注入的是
/// "删判据期望"而非"删实现拦截"，等于把门禁改弱而非给实现注入缺陷；
/// 改为直接改实现的三个拦截条件后捕获。
///
/// | 变体 | 施加的缺陷 | 实际转红的判据 |
/// |---|---|---|
/// | V1 | 线性前提三道拦截全部放行 | 线性前提断言（1） |
/// | V2 | 传输函数在 sRGB 空间直接相加（跳过解码） | 公式对拍、oracle 独立、换段断点（3） |
/// | V3 | `min(Cs+Cb,1)` 改边加边裁 | 先加后裁（1） |
/// | V4 | alpha 取 `αs`（不求和） | 公式对拍、alpha 同步、预乘相加、台账恰等（4） |
/// | V5 | 断点 0.04045 改 0.04 | 换段断点逐字核对（1）——**本轮新增才抓住** |
/// | V6 | `fpow` 级数截到 3 项 | `fpow` 已知值（1） |
/// | V7 | plus-darker 标成 `Specified` | 候选登记如实（1） |
/// | V8 | 场景推荐路径写 `linear-dodge`（未收录候选） | 场景路径已登记（1） |
/// | V9 | `clamp01` 对 `+Inf` 归零（回退本轮修掉的缺陷） | 10 条（`+Inf` 归零污染面极大） |
pub const VARIANT_REGISTRY: [(&str, &str); 9] = [
    ("V1-linear-premise-assertions-removed", "C24-LINEAR-PREMISE-ASSERTED"),
    ("V2-add-in-srgb-without-decode", "C24-FORMULA-VS-ORACLE"),
    ("V3-clamp-while-adding", "C24-CLAMP-AFTER-ADD"),
    ("V4-alpha-take-src-only", "C24-PREMULTIPLIED-ADD-ALPHA"),
    ("V5-eotf-split-0.04", "C24-TRANSPORT-SEGMENT-SPLIT"),
    ("V6-fpow-3-terms", "C24-FPOW-KNOWN-VALUES"),
    ("V7-plus-darker-marked-specified", "C24-CANDIDATE-REGISTRY-HONEST"),
    ("V8-scene-path-to-candidate", "C24-USECASE-PATH-REGISTERED"),
    ("V9-clamp-plus-inf-to-zero", "C24-CLAMP-NON-FINITE"),
];

/// 变异捕获率声明（判据侧自述：本轮 9/9）。
pub const VARIANT_CAPTURE_NOTE: &str = "\
VE-F0624 判据经 9 个定向变异反向验证，9/9 全部被捕获，证明判据非恒真。\
V5 初测 MISSED 是一处真实弱门禁：判据只验「断点两侧夹逼连续」，\
而 sRGB 传输函数本就连续可导，断点从 0.04045 挪到 0.04 两版都连续。\
根因记下来备用：**连续性判据抓「有没有缝」，对「缝的位置偏了」完全不敏感**。\
补法：断点常量逐字 == 规范值 + oracle 侧独立硬写同值（只断常量会自证式）。\
V1 初测 MISSED 则属变异选错：注入的是「删判据期望」而非「删实现拦截」，\
等于把门禁改弱而不是给实现注入缺陷——此类 MISSED 应先改产出侧（十诫第 14 条）。\
本轮还实测到判据红项而实现全对的一例：初版把「声明为编码」检查放在标记一致性之后，\
该分支永不可达（死码），返回的是次要诊断码。修法是修实现分层次序并把判据扩到四层，\
**不是**把判据期望改成实现当前返回值——后者会把死码固化成门禁。";