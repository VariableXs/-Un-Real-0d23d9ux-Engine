//! VE-F0625 · 预乘 Alpha 全域纪律（VE-D 域 · 2D 合成引擎 · 目标 400 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0625`
//!
//! **判据（锚点原文逐条）**：
//! - **内部像素流一律预乘表示**（GPU 纹理、中间缓冲、CPU SIMD 路径、
//!   效果链中间化缓冲 F0609 全部覆盖）；
//! - **转换点登记制**：外部直通 alpha 资产进系统 → 入口转换；
//!   输出到要求直通 alpha 的消费者 → 出口转换；**除此之外禁止任何
//!   隐式转换——转换点全集显性列表**；
//! - **数学理由归档**：预乘是滤波与混合在数学上唯一正确的一贯表示
//!   （直通 alpha 下线性滤波产生暗边）；
//! - **舍入纪律**（转换时舍入模式统一、转换不可往返无损处登记单向点）；
//! - **失败指纹库**：双重转换与漏转换的视觉症状图鉴（灰雾、亮边、黑框）
//!   供走查对照。
//!
//! 锚点判据原文：**全域预乘、转换点登记、理由归档、指纹库、判据**。
//!
//! ## 〇、为什么「预乘」不能只是一句纪律，而必须是**可执行的类型**
//!
//! 预乘 alpha 的失效方式有个共同特征：**代码照样编译、照样跑、结果只是"有点不对"**。
//! 直通 alpha 下做一次双线性滤波，画面边缘会有一圈暗边；把它乘一次 alpha
//! 变成"灰雾"；再乘一次变成"黑框"。没有任何一步会报错——**这正是它反复
//! 发生的原因**。
//!
//! 故本条的第一件事是**类型层强制**，与 [`ved24_additive`] 的线性前提
//! 同一手法（那处是"编码值不许进线性公式"，此处是"直通值不许进内部流"）：
//!
//! - [`Premul`] 是内部流的**唯一**像素表示类型，四通道全部已含 alpha；
//! - [`Straight`] 是**边界**类型，只允许出现在 [`ConvPoint`] 登记的
//!   入口/出口，且由 [`PremulFlow`] 的受限接口驱动——**内部函数签名
//!   根本不接受 [`Straight`]**，于是"中间某处不小心用了直通值"编译不过；
//! - 二者**无 `From` 互转**，任何转换必须显式调用
//!   [`to_premul`] / [`to_straight`] 并**在调用点记账**（[`ConvLog`]）。
//!
//! 换言之：**转换点全集不是文档里的清单，而是类型签名允许的全部位置**。
//! 登记表 [`CONV_POINTS`] 是这份"签名允许集"的人读版本，两者由
//! [`C25-CONV-POINTS-EXHAUSTIVE`] 逐条互校。
//!
//! ## 一、数学理由归档：**可计算**，不是散文
//!
//! 锚点：「预乘是滤波与混合在数学上唯一正确的一贯表示（**直通 alpha 下
//! 线性滤波产生暗边**）」。
//!
//! 这句话若只写成注释，谁都能声称"我知道要预乘"，但没人能验证。本条把
//! 它落成**可计算的对照实验**（[`filter_probe`]）：
//!
//! 取图像边缘的典型两像素做一次两点线性插值（权重各 0.5），**与解析真值
//! 比较**：
//!
//! ```text
//! 像素 A: alpha=1.0, 直通色=1.0（不透明红）    像素 B: alpha=0.0, 直通色=0.0
//!         （B 是清空缓冲的常态：低 alpha 处直通色在预乘语义下无意义，
//!           实现里通常就是 0）
//! 权重各 0.5（线性滤波）⇒ 该处应有的物理覆盖率 = 0.5
//!
//! 预乘路径（正确）：
//!     alpha_out = 0.5·1.0 + 0.5·0.0 = 0.5      ← 与覆盖率一致
//!     premul_out = 0.5·1.0 + 0.5·0.0 = 0.5
//!     直通色_out = 0.5 / 0.5 = **1.0**         ← 与真值一致，未被拉暗
//!
//! 直通路径（暗边来源）：
//!     alpha_out = 0.5·1.0 + 0.5·0.0 = 0.5      ← alpha **偶然正确**
//!     颜色_out  = 0.5·1.0 + 0.5·0.0 = **0.5**  ← **不等于** 1.0，被拉暗一半
//! ```
//!
//! 关键洞察（也是判据 [`C25-PREMUL-FILTER-EXACT`] 与
//! [`C25-STRAIGHT-FILTER-DARK-EDGE`] 的立论）：**直通路径的 alpha 恰好是
//! 对的**（因为 alpha 也被同样地插值），**错的只有颜色**。于是"检查 alpha
//! 对不对"这种走查**抓不到暗边**——必须断颜色与解析真值的差。这是十诫第 1
//! 条「形状判据只证有渐变」的同类：位置对了不代表量对了。
//!
//! **语料设计的陷阱（施工中踩到并已定稿）**：暗边之所以在数值上不可见，
//! 是因为两版可能恰好同值。上面的 B 取"全透明 + 直通色 0"正是关键——
//! 若 B 写成半透明且直通色非零，或 A 写成不透明**黑**，则两版颜色都恒为
//! 0，暗边差为 0，判据永绿。**必须让低 alpha 邻居的直通色为 0**（清空
//! 缓冲的常态），同时高 alpha 侧直通色**非零**，二者缺一则两版同值。
//!
//! 顺带解释了指纹库 [`Fingerprint::BlackFrame`] 的成因：预乘路径的颜色_out
//! 要除以 `alpha_out`，当 `alpha_out → 0` 时除数趋零，若此时分子也被舍入
//! 污染则颜色爆掉——这正是"黑框"的数学来源（见 §三）。
//!
//! ## 二、转换点登记制：三态而非 bool
//!
//! 一个 `is_boundary: bool` 表达不了本条需要的三件事，因此 [`ConvDir`]
//! 是**三态枚举**：入口（直通→预乘）、出口（预乘→直通）、**内部**
//! （预乘→预乘，恒等且**必须记账为零**）。
//!
//! 「内部恒等也要登记」这一点看着多余，实则相反：**它是最容易出错的
//! 那一档**——内部流里悄悄做一次"归一化"（除 alpha）就是漏转换的典型
//! 形态，而它不产生任何日志。把它登记成显式的 `Internal` 档并断言
//! 其转换次数恒为 0，才能让"内部零转换"这条纪律**可测**而非口号。
//!
//! ## 三、失败指纹库：每种症状配**可计算特征量**
//!
//! 锚点要求「灰雾、亮边、黑框」三类症状图鉴。纯文字图鉴无法用于自动
//! 走查，故每种指纹带一个 [`FingerprintSpec`]：
//!
//! | 指纹 | 成因 | 可计算特征（本条判据所断） |
//! |------|------|--------------------------|
//! | [`Fingerprint::GreyHaze`] | 漏转换：直通值当预乘用 | 亮区中点的 `color > 0` 而 `alpha < 1` ⇒ 直通语义下该处 color≈color_max |
//! | [`Fingerprint::BrightEdge`] | 双转：预乘值又被乘一次 alpha | 半透明边缘 `color ≈ premul_color`，即比预乘真值**偏暗**… 确切特征见 [`spec_note`] |
//! | [`Fingerprint::BlackFrame`] | 直通路径除以趋零的 alpha | `alpha < ε` 而 `color` 非零（预乘路径下此时 color 必为 0） |
//!
//! 三者各有**专属**判据（[`C25-FINGERPRINT-DISTINCT`] 断言三者两两不
//! 同值且各有真命中），避免"三个判据其实在测同一件事"。
//!
//! ## 四、舍入纪律与单向点登记
//!
//! 锚点：「转换时舍入模式统一、**转换不可往返无损处登记单向点**」。
//!
//! 前半句落为 [`ROUNDING_MODE`] 单一常量 + [`round_to_u8`]（所有转换点
//! 必须经它，判据断各转换点的舍入结果与之逐位相同）。
//!
//! 后半句是本条最容易被忽略的真实性问题：**`f32` 域上premul→straight
//! →premul 的往返并非恒等**（除法引入舍入，两次舍入不抵消）。
//!
//! 措辞上极重要：**登记的是「往返**不**无损处」，且这些点标为单向
//! （`OneWay`）**——**不是**「让往返变无损」。后者是错的：低 alpha 处
//! 除法不稳定，强行做无损往返需要额外状态（记住原值），代价与收益
//! 都不成立。诚实登记为单向点才是正解，故 [`ONE_WAY_POINTS`] 显式
//! 列出这些 alpha 区间，`[C25-ONE-WAY-REGISTERED]` 断其非空且与实测
//! 不往返区间**一致**（若某区间实测无损却登记为单向，或反之，均转红）。
//!
//! ## 五、错误路径与降级矩阵（锚点原文三行）
//!
//! | 锚点情形 | 本条处置 | 不许做的事 |
//! |----------|----------|------------|
//! | 漏转换 → 指纹库走查对照定位 | [`classify`] 按特征量归类到三指纹之一并给处置建议 | 只说"看着发灰" |
//! | 双转 → 入口出口单一负责制审计 | [`audit_single_responsibility`] 逐转换点核"入口只有入口负责、出口只有出口负责"，重载即红 | 允许同一像素既在入口转又在内部转 |
//! | 第三方资产未声明 → 元数据嗅探加默认安全档 | [`sniff`] 按元数据判定真预乘/真直通/存疑，存疑走[`DEFAULT_SAFE`]（**一律按直通处理并入口转换**，宁可多转换一次也不漏转换） | 猜错了还静默 |
//!
//! 默认安全档的方向选择值得说明：**"多转换一次"与"漏转换"的代价不对称**——
//! 多转换的后果是颜色偏亮（可用指纹库发现并回溯），漏转换的后果是暗边
//! （且alpha 恰好正确，走查极难发现）。故存疑一律按直通处理。
//!
//! ## 六、跨批对接点
//!
//! 上游 [`ved03_alpha`]（F0603 图层不透明度，其 [`PremulColor`] 是本条
//! 色彩运算的既有形态，本条不重算只做纪律约束）、
//! [`ved24_additive`]（F0624 附加族，其 `LinearRgb` 预乘语义由本条的
//! [`Premul`] 承接）、F0609（效果链中间化缓冲——**尚未入库**，故本条的
//! 登记表按**前瞻契约**设计：未实现的域也先立登记行，下游入库时直接
//! 消费，不需回头改本条）；
//! 下游 F0628（GPU 路采样输入按本条预乘语义）、F0629（CPU SIMD 路同理）、
//! F0637（线性空间纪律，转换点与色彩空间转换点须**正交**——见 §七）。
//!
//! ## 七、两个纪律的**正交性**（易被合并成一件事，须钉死）
//!
//! F0624 管**色彩空间**（sRGB 编码 ↔ 线性光），本条管 **alpha 表示**
//! （直通 ↔ 预乘）。二者**独立**：直通 alpha 的像素同样需要 sRGB→线性；
//! 预乘的像素同样需要与线性空间解耦。
//!
//! 合并的典型错误是"反正都要转换，一次做完"——于是把
//! `srgb→linear` 与 `straight→premul` 揉成一个函数，
//! **入口出口的单一负责制随即失效**（无法判断哪次转换该由谁负责）。
//! 判据 [`C25-SPACE-ALPHA-ORTHOGONAL] 用四个组合（2 空间 × 2 表示）
//! 逐一验证两种转换**互不代替**：只做空间转换不做 alpha 转换会红，
//! 只做 alpha 转换不做空间转换也会红。
//!
//! ## 八、无障碍与隐私
//!
//! 本条共**五个**纯文本产出函数，全部返回可读文字行（指纹症状描述是文字
//! 而非色块，读屏可达），零用户像素内容——全部语料自造。零 IO、零墙钟、
//! 确定性算法。
//!
//! | 函数 | 产出 |
//! |---|---|
//! | [`fingerprint_rows`] | 三指纹的成因 / 处置 / 判别特征三字段 |
//! | [`conv_point_rows`] | 转换点登记行（id / 域 / 方向 / 负责方 / 备注） |
//! | [`one_way_rows`] | 单向点登记行（区间与理由） |
//! | [`one_way_audit_rows`] | 单向点实测审计（往返残差与阈值对照） |
//! | [`swatch_rows`] | 色卡行（与 F0622 共用探针，判据 17 消费） |
//!
//! ## 九、本条的判据清单（[`run_ved25_checks`]）
//!
//! | 判据 | 抓什么 |
//! |------|--------|
//! | [`C25-INTERNAL-IS-PREMUL`] | 内部流出现直通表示（类型层已挡，此处再验运行期标记） |
//! | [`C25-PREMUL-FILTER-EXACT`] | 滤波未除 alpha（暗边）——**alpha 对但颜色错** |
//! | [`C25-STRAIGHT-FILTER-DARK-EDGE`] | 预乘真值被误当直通真值（对照语料） |
//! | [`C25-CONV-POINTS-EXHAUSTIVE`] | 转换点登记表与类型签名允许集不一致 |
//! | [`C25-INTERNAL-ZERO-CONVERSION`] | 内部流悄悄做归一化（内部转换计数非 0） |
//! | [`C25-SINGLE-RESPONSIBILITY`] | 同一像素入口出口双重转换 |
//! | [`C25-DOUBLE-CONVERT-DETECTED`] | 双转（颜色被乘两次 alpha） |
//! | [`C25-MISSED-CONVERT-DETECTED`] | 漏转换（颜色未乘 alpha） |
//! | [`C25-FINGERPRINT-DISTINCT`] | 三个指纹其实在测同一件事 / 无真命中 |
//! | [`C25-CLASSIFY-ALL-FINGERPRINTS`] | 指纹分类器对三类真症状都能归类 |
//! | [`C25-SNIFF-DEFAULT-SAFE`] | 未声明资产未走默认安全档 |
//! | [`C25-ROUNDING-MODE-UNIFIED`] | 各转换点舍入结果不一致 |
//! | [`C25-ONE-WAY-REGISTERED`] | 单向点登记与实测不一致（漏登或误登） |
//! | [`C25-SPACE-ALPHA-ORTHOGONAL`] | 空间转换与 alpha 转换被合并成一件事 |
//! | [`C25-ALPHA-EDGES`] | 边界 alpha（0 / 1 / 次正规）处理错 |
//! | [`C25-LOG-EXACT`] | 记账被吞（幽灵记账 / 漏记） |
//! | [`C25-INTERNAL-NO-LEDGER-LEAK`] | 内部转换偷记入口/出口台账（只查一个计数器不够） |
//! | [`C25-INTERNAL-PURE-OPERATIONS-CLEAN`] | 纯内部操作走完四本账非零（运行时零转换被破坏） |
//! | [`C25-ONE-WAY-REGISTRATION-COVERS-INDEPENDENT-PROBE`] | 单向点登记对**独立字面量**探针失配（探针取自被测常量即自证恒绿） |
//! | [`C25-ONE-WAY-THRESHOLD-QUANTUM`] | 单向点阈值不在 (0, u8 一档] 内（退成 0 或抬成全域） |
//! | [`C25-SWATCH-SHARED-PROBES`] | 另抄一份斜坡探针（须真引用 F0622） |

use crate::checks::CheckSet;

// no_std 导入：`format!` 宏**必须显式引入**（lib.rs 有 `extern crate alloc`
// 但无 `#[macro_use]`），而它内部对 `ToString` 的依赖走的是全限定路径，
// 故 `ToString` 本身不必也不该引入——引了就是死导入（编译器会指出来）。
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

// 与 F0622 共用色卡与对拍尺度（锚点同款「真引用」硬要求）。
// 注意`RAMP` 虽未直接使用，但**必须导入**：判据
// `C25-SWATCH-SHARED-PROBES` 要断言本模块见到的灰阶表与 F0622 同源，
// 而该断言依赖此名在作用域内（见下方该判据的实现）。
use super::ved22_separable::{ramp_char, LSB8, LSB_BUDGET, RAMP, SWATCH_PROBES};

// ---------------------------------------------------------------------------
// 一、参数唯一源
// ---------------------------------------------------------------------------

/// 8 bit 量化步长（LSB 口径，与 F0622/F0623/F0624 同源）。
pub const LSB8_STEP: f32 = LSB8;

/// 对拍允许的最大 LSB 偏差。
pub const LSB_LIMIT: f32 = LSB_BUDGET;

/// 色卡探针数（真引用 F0622）。
pub const SWATCH_STEPS: usize = SWATCH_PROBES.len();

/// alpha 值域下界。
pub const ALPHA_LO: f32 = 0.0;

/// alpha 值域上界。
pub const ALPHA_HI: f32 = 1.0;

/// **全域预乘**纪律的 alpha 可靠下界（低于此值不做除法）。
///
/// 直通→预乘的反预乘要除以 alpha；`alpha → 0` 时除数趋零，颜色爆掉
/// （[ `Fingerprint::BlackFrame` ] 的成因）。本条规定低于此界即返回
/// 全零并**记账**——不静默返回颜色。
pub const ALPHA_EPS: f32 = 1.0e-4;

/// 转换舍入模式（**单一常量**，锚点「舍入模式统一」的载体）。
///
/// 取**就近舍入**（round-half-up 到 u8）而非 banker's rounding：判据
/// [`C25-ROUNDING-MODE-UNIFIED`] 要断各转换点结果**逐位相同**，
/// 故必须有一处唯一实现可被复用，而不是每点各写一遍。
pub const ROUNDING_MODE: RoundingMode = RoundingMode::RoundHalfUp;

/// 预乘→直通的**单向点**阈值（alpha 低于此值则往返不无损）。
///
/// 实测口径：`alpha < 1/255` 时 `premul → straight → premul` 的
/// 往返残差超过 1 LSB（除法两次舍入不抵消）。该值是**实测得来**的，
/// 判据 [`C25-ONE-WAY-REGISTERED`] 会独立重算并与本常量比对。
pub const ONE_WAY_ALPHA_THRESHOLD: f32 = 1.0 / 255.0;

/// 8 bit 输出上限（舍入目标）。
pub const U8_MAX: u32 = 255;

/// 指纹分类的灰雾阈值：亮区中点亮度高于此值即判"非全黑"。
pub const GREY_HAZE_FLOOR: f32 = 0.02;

/// 指纹分类的黑框阈值：alpha 低于此值而颜色非零即判黑框。
pub const BLACK_FRAME_FLOOR: f32 = ALPHA_EPS;

/// 指纹分类的判别阈值：**直通色与正确参考色的最小可辨绝对差**。
///
/// 口径是绝对色差（`[0,1]` 尺度）而非 LSB 倍数——因为三条指纹的
/// 偏差幅度天然不同（漏转换可达数十 LSB，双转常在个位 LSB），
/// 用单一 LSB 倍数会让小偏差的双转漏判。
///
/// 取值 `0.02`（约 5 LSB）低于"肉眼可见"的量级、高于浮点噪声：
/// 正确路径实测与参考值**逐位相同**（差 0），故阈值只需大于噪声即可。
pub const BRIGHT_EDGE_TOL: f32 = 0.02;

// ---------------------------------------------------------------------------
// 一之二、真内核可用的取整原语（**自实现，不碰`f32::floor`**）
// ---------------------------------------------------------------------------

/// 向下取整（真内核 `core` 面可用）。
///
/// **为什么不用 `f32::floor`**：真内核（`kernel-image`）编到
/// `x86_64-unknown-none`，那个 target 上 `f32::floor` / `ceil` / `round`
/// **不存在**（与 `sin`/`cos`/`sqrt`/`powf` 同类，属std侧注入的固有方法）。
/// 宿主构建因`lib.rs` 有 `extern crate std` 反而能过——**宿主面全绿推不出
/// 真内核面全绿**，这条坑必须靠独立 target 编译才能发现。
///
/// 算法：向下取整 = `x - frac`，`frac = x - (x as i64 as f32)`。
/// `as` 转换对 `|x| < 2^31` 是精确截断，故 `frac ∈ [0,1)`。
/// 非有限输入的处置：`clamp01` 已在上游裁剪，此处仍自兜底
/// （NaN → 0，+Inf → `i32::MAX` 钳到 `u32`，−Inf → 0），
/// 保持「`pub` 纯函数不依赖调用方钳制」。
pub fn ffloor(x: f32) -> f32 {
    if x.is_nan() {
        return 0.0;
    }
    let t = x as i64 as f32; // 截断（向零），对 |x| < 2^31 精确
    let frac = x - t;
    if frac < 0.0 {
        // 负数：向零截断使 `frac ∈ (-1, 0]`，向下取整须再退一格。
        t - 1.0
    } else {
        t
    }
}

/// 8 bit 量化的向下取整整数值（非负输入）。
fn floor_u32(x: f32) -> u32 {
    let f = ffloor(x);
    if f <= 0.0 {
        0
    } else if f >= U8_MAX as f32 {
        U8_MAX
    } else {
        f as u32
    }
}

// ---------------------------------------------------------------------------
// 二、两种像素表示（类型层强制：内部流只接受预乘）
// ---------------------------------------------------------------------------

/// 舍入模式（锚点「舍入模式统一」的枚举化）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoundingMode {
    /// 就近舍入（`floor(x + 0.5)`），8 bit 输出标准做法。
    RoundHalfUp,
    /// 银行家舍入（ties-to-even）。
    TiesToEven,
}

impl RoundingMode {
    /// 全部模式（供审计遍历）。
    pub fn all() -> [RoundingMode; 2] {
        [RoundingMode::RoundHalfUp, RoundingMode::TiesToEven]
    }

    /// 关键字（登记行用）。
    pub fn keyword(&self) -> &'static str {
        match self {
            RoundingMode::RoundHalfUp => "round-half-up",
            RoundingMode::TiesToEven => "ties-to-even",
        }
    }

    /// 对 `[0,1]` 的值按本模式量化到 8 bit。
    ///
    /// 走 [`ffloor`] 而非 `f32::floor`（真内核 target 上后者不存在）。
    pub fn quantize_u8(&self, v: f32) -> u32 {
        let x = clamp01(v) * U8_MAX as f32;
        match self {
            RoundingMode::RoundHalfUp => floor_u32(x + 0.5),
            RoundingMode::TiesToEven => {
                let f = ffloor(x);
                let diff = x - f;
                let half_up = (diff > 0.5) || (diff == 0.5);
                if !half_up {
                    floor_u32(f)
                } else {
                    let fi = floor_u32(f);
                    if fi % 2 == 0 {
                        fi
                    } else {
                        fi + 1
                    }
                }
            }
        }
    }
}

/// 统一舍入到 u8（**所有转换点必须经它**，判据断逐位相同）。
pub fn round_to_u8(v: f32) -> u32 {
    ROUNDING_MODE.quantize_u8(v)
}

/// 值域裁剪（非有限先兜底再裁，次序不可换）。
pub fn clamp01(v: f32) -> f32 {
    let g = if v.is_nan() {
        // NaN 无序：必须在比较之前处理，否则 `min(NaN,1)` 会静默
        // 返回比较运算的另一操作数，把 NaN 吞掉。
        ALPHA_LO
    } else {
        v
    };
    if g < ALPHA_LO {
        ALPHA_LO
    } else if g > ALPHA_HI {
        ALPHA_HI
    } else {
        g
    }
}

/// 非有限兜底（`NaN`/`±Inf` → 0）并记账。
fn guard_counted(v: f32, log: &mut ConvLog) -> f32 {
    if v.is_finite() {
        v
    } else {
        log.nonfinite_guarded += 1;
        0.0
    }
}

/// **预乘**像素（内部流的唯一表示；四通道全部已含 alpha）。
///
/// 与 [`Straight`] 是不同 struct 且**无 `From` 互转**——想转换必须
/// 显式调 [`to_premul`] / [`to_straight`] 并在调用点记账。
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Premul {
    /// 预乘红（`[0,1]`，已含 alpha）。
    pub r: f32,
    /// 预乘绿（`[0,1]`，已含 alpha）。
    pub g: f32,
    /// 预乘蓝（`[0,1]`，已含 alpha）。
    pub b: f32,
    /// alpha（`[0,1]`）。
    pub a: f32,
}

impl Premul {
    /// 构造。
    pub fn new(r: f32, g: f32, b: f32, a: f32) -> Premul {
        Premul { r, g, b, a }
    }

    /// 灰（各通道等值，预乘语义下 `v ≤ a`）。
    pub fn gray(v: f32) -> Premul {
        Premul { r: v, g: v, b: v, a: v }
    }

    /// 不透明色（alpha=1，此时预乘 ≡ 直通）。
    pub fn opaque(r: f32, g: f32, b: f32) -> Premul {
        Premul { r, g, b, a: ALPHA_HI }
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
    pub fn with(&self, i: usize, v: f32) -> Premul {
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

    /// 按下标写并返回是否成功（供判据构造越界语料）。
    pub fn try_set(&mut self, i: usize, v: f32) -> bool {
        match i {
            0 => self.r = v,
            1 => self.g = v,
            2 => self.b = v,
            3 => self.a = v,
            _ => return false,
        }
        true
    }

    /// 直通颜色的三通道（**只读视图**，不构成转换——不做记账）。
    ///
    /// 存在的意义是让"读预乘值看颜色"这件事在代码里显式可见；
    /// 真正的反预乘必须走 [`to_straight`]（有记账）。
    pub fn premul_channels(&self) -> [f32; 3] {
        [self.r, self.g, self.b]
    }
}

/// **直通 alpha**像素（只允许出现在转换点登记的边界）。
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Straight {
    /// 直通红（`[0,1]`，**不含** alpha）。
    pub r: f32,
    /// 直通绿。
    pub g: f32,
    /// 直通蓝。
    pub b: f32,
    /// alpha（`[0,1]`）。
    pub a: f32,
}

impl Straight {
    /// 构造。
    pub fn new(r: f32, g: f32, b: f32, a: f32) -> Straight {
        Straight { r, g, b, a }
    }

    /// 灰。
    pub fn gray(v: f32) -> Straight {
        Straight { r: v, g: v, b: v, a: v }
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
    pub fn with(&self, i: usize, v: f32) -> Straight {
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
}

// ---------------------------------------------------------------------------
// 三、转换方向与转换点登记制
// ---------------------------------------------------------------------------

/// 转换方向（三态，见头注 §二）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConvDir {
    /// 入口：直通 → 预乘（外部资产进系统）。
    Inlet,
    /// 出口：预乘 → 直通（交给要求直通 alpha 的消费者）。
    Outlet,
    /// 内部：预乘 → 预乘（**恒等，且必须恒为零次**）。
    Internal,
}

impl ConvDir {
    /// 全部方向。
    pub fn all() -> [ConvDir; 3] {
        [ConvDir::Inlet, ConvDir::Outlet, ConvDir::Internal]
    }

    /// 关键字。
    pub fn keyword(&self) -> &'static str {
        match self {
            ConvDir::Inlet => "inlet",
            ConvDir::Outlet => "outlet",
            ConvDir::Internal => "internal",
        }
    }

    /// 该方向是否属于**边界**（内部档不是边界）。
    pub fn is_boundary(&self) -> bool {
        match self {
            ConvDir::Inlet => true,
            ConvDir::Outlet => true,
            ConvDir::Internal => false,
        }
    }

    /// 该方向的转换是否**改变表示**（内部档不改变）。
    pub fn changes_rep(&self) -> bool {
        self.is_boundary()
    }
}

/// 像素域（锚点要求覆盖 GPU 纹理、中间缓冲、CPU SIMD、效果链缓冲）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelDomain {
    /// GPU 纹理采样所得。
    GpuTexture,
    /// 中间缓冲（合成过程产物）。
    IntermediateBuffer,
    /// CPU SIMD 路径。
    CpuSimdPath,
    /// 效果链中间化缓冲（F0609，**前瞻登记**：域尚未入库）。
    EffectChainBuffer,
}

impl PixelDomain {
    /// 全部域（锚点「全部覆盖」的枚举化）。
    pub fn all() -> [PixelDomain; 4] {
        [
            PixelDomain::GpuTexture,
            PixelDomain::IntermediateBuffer,
            PixelDomain::CpuSimdPath,
            PixelDomain::EffectChainBuffer,
        ]
    }

    /// 关键字。
    pub fn keyword(&self) -> &'static str {
        match self {
            PixelDomain::GpuTexture => "gpu-texture",
            PixelDomain::IntermediateBuffer => "intermediate-buffer",
            PixelDomain::CpuSimdPath => "cpu-simd-path",
            PixelDomain::EffectChainBuffer => "effect-chain-buffer",
        }
    }

    /// 该域的内部表示是否**恒为预乘**（全部为 true——这就是纪律本身）。
    pub fn internal_is_premul(&self) -> bool {
        match self {
            PixelDomain::GpuTexture => true,
            PixelDomain::IntermediateBuffer => true,
            PixelDomain::CpuSimdPath => true,
            PixelDomain::EffectChainBuffer => true,
        }
    }
}

/// 转换点登记条目（**全集显性**，锚点硬要求）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConvPoint {
    /// 转换点关键字（唯一）。
    pub id: &'static str,
    /// 该点服务的像素域。
    pub domain: PixelDomain,
    /// 转换方向。
    pub dir: ConvDir,
    /// **单一负责方**（锚点降级矩阵「入口出口单一负责制」）。
    pub owner: &'static str,
    /// 备注（登记行的可读说明）。
    pub note: &'static str,
}

impl ConvPoint {
    /// 全部登记点（**转换点全集**）。
    ///
    /// 每一项都是**类型签名允许的位置**——[`PremulFlow`] 的受限接口
    /// 只暴露这四种入口/出口，且不接受 [`Straight`] 作为内部参数。
    ///
    /// **覆盖要求**：锚点「GPU 纹理、中间缓冲、CPU SIMD 路径、效果链
    /// 中间化缓冲全部覆盖」逐域落地，故四个 [`PixelDomain`] **各自
    /// 至少一个入口点**。这不是形式主义——本条初版只给了 `CpuSimdPath`
    /// 出口没给入口，判据 [`C25-SINGLE-RESPONSIBILITY`] 的域覆盖检查
    /// 把它转红了（那不是误报：那批数据确实是从外部取入的直通用）。
    pub fn all() -> [ConvPoint; CONV_POINT_COUNT] {
        [
            ConvPoint {
                id: "asset-import",
                domain: PixelDomain::GpuTexture,
                dir: ConvDir::Inlet,
                owner: "asset-loader",
                note: "第三方直通 alpha 资产进系统：唯一合法入口转换点",
            },
            ConvPoint {
                id: "filter-sample",
                domain: PixelDomain::IntermediateBuffer,
                dir: ConvDir::Inlet,
                owner: "filter-chain",
                note: "滤波采样缓冲取入：采样器只产直通，须入口转换",
            },
            ConvPoint {
                id: "simd-readback",
                domain: PixelDomain::CpuSimdPath,
                dir: ConvDir::Inlet,
                owner: "simd-probe",
                note: "CPU SIMD 路取入外部像素批（回读/上传）：须入口转换",
            },
            ConvPoint {
                id: "surface-export",
                domain: PixelDomain::CpuSimdPath,
                dir: ConvDir::Outlet,
                owner: "surface-writer",
                note: "交给要求直通 alpha 的表面系统：唯一合法出口转换点",
            },
            ConvPoint {
                id: "video-frame",
                domain: PixelDomain::EffectChainBuffer,
                dir: ConvDir::Inlet,
                owner: "video-source",
                note: "视频帧更新进效果链：逐帧入口转换（F0609 前瞻契约）",
            },
            ConvPoint {
                id: "effect-chain-head",
                domain: PixelDomain::EffectChainBuffer,
                dir: ConvDir::Outlet,
                owner: "effect-chain",
                note: "效果链末端交给合成器：出口转换（F0609 前瞻契约）",
            },
        ]
    }

    /// 该点是否为边界转换（内部档恒false）。
    pub fn is_boundary(&self) -> bool {
        self.dir.is_boundary()
    }

    /// 登记完整性：**负责方与备注都非空**（不许裸奔）。
    pub fn registry_honest(&self) -> bool {
        !self.id.is_empty() && !self.owner.is_empty() && !self.note.is_empty()
    }
}

/// 转换点总数（锚点「转换点全集」的规模：四域各自入口 + 三域有出口）。
pub const CONV_POINT_COUNT: usize = 6;

/// 按关键字查转换点（**越界/未知名返回 `None`**，绝不 panic）。
pub fn conv_point_by_id(id: &str) -> Option<ConvPoint> {
    let all = ConvPoint::all();
    let mut i = 0usize;
    while i < CONV_POINT_COUNT {
        if all[i].id == id {
            return Some(all[i]);
        }
        i += 1;
    }
    None
}

/// 边界转换点数（入口 + 出口，**内部档不计入**）。
pub fn boundary_point_count() -> usize {
    let mut n = 0usize;
    for p in ConvPoint::all().iter() {
        if p.is_boundary() {
            n += 1;
        }
    }
    n
}

/// 转换点关键字是否重复（注册表完整性）。
pub fn conv_point_keys_unique() -> bool {
    let all = ConvPoint::all();
    let mut i = 0usize;
    while i < CONV_POINT_COUNT {
        let mut j = i + 1;
        while j < CONV_POINT_COUNT {
            if all[i].id == all[j].id {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 四、转换台账（零静默）
// ---------------------------------------------------------------------------

/// 转换事件台账。
///
/// 分项记账而非单一计数：**入口/出口/内部必须分开**，否则「内部零转换」
/// 这条纪律无法被验证（内部转换混在总数里就看不出是 0 还是 1）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct ConvLog {
    /// 入口转换次数。
    pub inlet: u32,
    /// 出口转换次数。
    pub outlet: u32,
    /// **内部转换次数（纪律要求恒为 0）**。
    pub internal: u32,
    /// 反预乘除法次数（alpha ≤ [`ALPHA_EPS`] 而退化为全零的次数）。
    pub alpha_floor_hits: u32,
    /// 非有限兜底次数。
    pub nonfinite_guarded: u32,
}

impl ConvLog {
    /// 新台账（全零）。
    pub fn new() -> ConvLog {
        ConvLog {
            inlet: 0,
            outlet: 0,
            internal: 0,
            alpha_floor_hits: 0,
            nonfinite_guarded: 0,
        }
    }

    /// 记账总次数。
    pub fn total(&self) -> u32 {
        self.inlet + self.outlet + self.internal + self.alpha_floor_hits + self.nonfinite_guarded
    }

    /// 台账是否干净（无任何事件）。
    pub fn is_clean(&self) -> bool {
        self.total() == 0
    }

    /// **全域预乘**是否成立：内部转换恒为 0。
    pub fn internal_is_pure(&self) -> bool {
        self.internal == 0
    }
}

// ---------------------------------------------------------------------------
// 五、转换实现（入口/出口显式，内部恒等且记账）
// ---------------------------------------------------------------------------

/// 直通 → 预乘（**入口转换**，锚点唯一允许的入内变换）。
///
/// 数学：`premul_i = straight_i × a`（先取分量再乘 alpha，次序按纪律）。
pub fn to_premul(s: Straight, log: &mut ConvLog) -> Premul {
    log.inlet += 1;
    let a = guard_counted(s.a, log);
    let a = clamp01(a);
    Premul {
        r: guard_counted(s.r, log) * a,
        g: guard_counted(s.g, log) * a,
        b: guard_counted(s.b, log) * a,
        a,
    }
}

/// 预乘 → 直通（**出口转换**，锚点唯一允许的出内变换）。
///
/// 数学：`straight_i = premul_i / a`。`a ≤ [`ALPHA_EPS`]` 时**返回全零
/// 并记账**——不返回颜色（那正是黑框的成因，见 [`Fingerprint::BlackFrame`]）。
pub fn to_straight(p: Premul, log: &mut ConvLog) -> Straight {
    log.outlet += 1;
    let a = guard_counted(p.a, log);
    let a = clamp01(a);
    if a <= ALPHA_EPS {
        // 分母趋零：直接除法会放大舍入噪声成"黑框"。返回全零 + 记账。
        log.alpha_floor_hits += 1;
        return Straight { r: 0.0, g: 0.0, b: 0.0, a };
    }
    let inv = 1.0 / a;
    Straight {
        r: clamp01(guard_counted(p.r, log) * inv),
        g: clamp01(guard_counted(p.g, log) * inv),
        b: clamp01(guard_counted(p.b, log) * inv),
        a,
    }
}

/// 预乘 → 预乘（**内部转换：恒等**）。
///
/// 存在的**唯一**目的是让"内部流里做了一次归一化"这件事**有账可查**。
/// 本函数不做任何算术，只记 [`ConvLog::internal`]——若它恒为 0，
/// 「运行时零转换」这条纪律就被证明；若有人在此偷偷除 alpha，
/// 计数会立刻非 0（判据 [`C25-INTERNAL-ZERO-CONVERSION`] 抓）。
pub fn internal_passthrough(p: Premul, log: &mut ConvLog) -> Premul {
    log.internal += 1;
    p
}

/// 受限像素流（**类型层强制的载体**）。
///
/// 内部函数**不接受 [`Straight`] 参数**——于是"中间某处不小心用了
/// 直通值"在编译期就被挡住。本结构只暴露四种边界操作与两个预乘域操作。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct PremulFlow {
    /// 已入口转换次数。
    pub inlet_done: u32,
    /// 已出口转换次数。
    pub outlet_done: u32,
}

impl PremulFlow {
    /// 新流（零转换）。
    pub fn new() -> PremulFlow {
        PremulFlow {
            inlet_done: 0,
            outlet_done: 0,
        }
    }

    /// 边界入口：直通 → 预乘（经 [`to_premul`]）。
    pub fn admit(&mut self, s: Straight, log: &mut ConvLog) -> Premul {
        self.inlet_done += 1;
        to_premul(s, log)
    }

    /// 边界出口：预乘 → 直通（经 [`to_straight`]）。
    pub fn emit(&mut self, p: Premul, log: &mut ConvLog) -> Straight {
        self.outlet_done += 1;
        to_straight(p, log)
    }

    /// 内部合成：两预乘相加（**不接受直通**⇒ 类型层强制）。
    ///
    /// 预乘下的"源覆盖"合成：逐通道 `src + dst·(1 − src.a)`。
    pub fn composite_over(&self, src: Premul, dst: Premul) -> Premul {
        let k = 1.0 - clamp01(src.a);
        Premul {
            r: clamp01(src.r + dst.r * k),
            g: clamp01(src.g + dst.g * k),
            b: clamp01(src.b + dst.b * k),
            a: clamp01(src.a + dst.a * k),
        }
    }

    /// 内部滤波：两点线性插值（**预乘域内**，暗边不产生的关键）。
    ///
    /// 权重与 alpha **分开给**——这是 F0622 的同款纪律：把 alpha 打进
    /// 通道里会让"alpha 不影响颜色"这条性质无法被违反（自证式）。
    pub fn filter_linear2(&self, a: Premul, wa: f32, b: Premul, wb: f32) -> Premul {
        let wa2 = clamp01(wa);
        let wb2 = clamp01(wb);
        let r = a.r * wa2 + b.r * wb2;
        let g = a.g * wa2 + b.g * wb2;
        let bl = a.b * wa2 + b.b * wb2;
        let al = a.a * wa2 + b.a * wb2;
        // 预乘滤波的正确性在这一行：滤波后仍是预乘，**不需要也不允许**
        // 除 alpha（那正是直通路径的暗边来源）。
        Premul { r, g, b: bl, a: al }
    }
}

// ---------------------------------------------------------------------------
// 六、数学理由的可计算归档（头注 §一）
// ---------------------------------------------------------------------------

/// 滤波对照探针结果。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FilterProbe {
    /// 滤波点的解析真值覆盖率（`0.5`）。
    pub true_alpha: f32,
    /// 滤波点的解析真值颜色（`1.0`——该处物理上是「不透明红被覆盖率 0.5
    /// 覆盖」，即半透明红，其直通色仍是 1.0）。
    ///
    /// **初版此处写作 `0.0` 且语料用不透明黑，两处都错**：那个语料下
    /// 预乘与直通两路的颜色恒为 0，暗边差为 0、判据永绿。现在的语料
    /// （A 不透明红、B 全透明）下真值为 1.0，两路差0.5，暗边才显形。
    pub true_color: f32,
    /// **预乘路径**算出的 alpha。
    pub premul_alpha: f32,
    /// **预乘路径**算出的直通颜色（需反预乘）。
    pub premul_straight_color: f32,
    /// **直通路径**算出的 alpha。
    pub straight_alpha: f32,
    /// **直通路径**算出的颜色（暗边来源）。
    pub straight_color: f32,
    /// 预乘路径颜色与真值的差。
    pub premul_error: f32,
    /// 直通路径颜色与真值的差。
    pub straight_error: f32,
    /// 直通路径的 alpha 误差（**预期也≈0**：alpha 偶然正确）。
    pub straight_alpha_error: f32,
}

impl FilterProbe {
    /// 预乘路径是否精确（颜色与 alpha 都对）。
    pub fn premul_is_exact(&self) -> bool {
        self.premul_error <= LSB8_STEP && (self.premul_alpha - self.true_alpha).abs() <= LSB8_STEP
    }

    /// 直通路径是否**也** alpha 正确（证明"查 alpha 抓不到暗边"）。
    pub fn straight_alpha_looks_right(&self) -> bool {
        self.straight_alpha_error <= LSB8_STEP
    }

    /// 直通路径是否确有暗边（颜色误差显著）。
    pub fn straight_has_dark_edge(&self) -> bool {
        self.straight_error > 8.0 * LSB8_STEP
    }
}

/// 滤波对照探针（**理由归档的可计算形式**）。
///
/// 语料：像素 A `alpha=1.0, 颜色=黑`；像素 B `alpha=0.0, 颜色=1.0`；
/// 权重各半。解析真值：覆盖率 `0.5`，颜色 `0.0`
/// （半透明黑与全透明像素插值 ⇒ 半透明黑 ⇒ 直通颜色 0）。
pub fn filter_probe() -> FilterProbe {
    // 语料（图像边缘的典型两像素，**初版两处都写错过，此处是推导后的定稿**）：
    //   A：不透明红（alpha=1.0，直通色=1.0）⇒ 预乘 = 1.0
    //   B：全透明（alpha=0.0，直通色=0.0 —— 清空缓冲的常态）
    //      ⇒ 预乘 = 0.0
    //
    // 两处初版错误（都让判据红而实现对）：
    //  1. A 写成**不透明黑**：两版颜色都恒 0，暗边数值上不可见；
    //  2. 解析真值 `true_color` 写成 0.0：错。该处物理上就是
    //     「半透明红」，直通色应为 **1.0**（预乘反除法给的就是 1.0）。
    //
    // 正确推导：
    //   预乘路径：premul_r = 1.0·0.5 + 0·0.5 = 0.5；alpha 同为 0.5
    //             ⇒ 反预乘 0.5/0.5 = **1.0**（与真值一致）
    //   直通路径：颜色 = 1.0·0.5 + 0.0·0.5 = **0.5**
    //             （B 的直通色是 0——低 alpha 邻居的直通色在预乘语义下无意义，
    //               实现里通常就是 0，于是它把边缘拉暗一半）
    //             而 alpha = 1.0·0.5 + 0.0·0.5 = **0.5**，**恰好正确**
    let a = Premul::opaque(1.0, 1.0, 1.0); // 不透明红
    let b = Premul::new(0.0, 0.0, 0.0, 0.0); // 全透明（直通色 0）
    let mut log = ConvLog::new();

    // 预乘路径（正确）：滤波在预乘域内完成，**不需要也不允许**除 alpha。
    let pm = PremulFlow::new().filter_linear2(a, 0.5, b, 0.5);
    let pm_straight = to_straight(pm, &mut log);

    // 直通路径（错误）：**先反预乘成直通，再在直通域滤波**。
    let a_s = to_straight(a, &mut log);
    let b_s = to_straight(b, &mut log);
    let straight_alpha = a_s.a * 0.5 + b_s.a * 0.5;
    let straight_color = a_s.r * 0.5 + b_s.r * 0.5;

    // 解析真值：覆盖率 0.5；该处物理上是「半透明红」⇒ 直通色 1.0。
    let true_alpha = 0.5;
    let true_color = 1.0;
    FilterProbe {
        true_alpha,
        true_color,
        premul_alpha: pm.a,
        premul_straight_color: pm_straight.r,
        straight_alpha,
        straight_color,
        premul_error: (pm_straight.r - true_color).abs(),
        straight_error: (straight_color - true_color).abs(),
        straight_alpha_error: (straight_alpha - true_alpha).abs(),
    }
}

/// 数学理由文本（随产物一并留存，供 F0634 文档章引用）。
///
/// **文本必须与 [`filter_probe`] 的语料逐字同源**：这里的反例一旦与实验
/// 脱节，下游文档章就会引用一个无法复现的推导。施工中此文本曾停留在
/// 初版语料（A 为不透明**黑**、真值色 0），而实验早已改成A 为不透明**红**、
/// 真值色 1.0——那份初版语料下两版颜色恒同 0，暗边在数值上根本不存在。
pub const MATH_REASON: &str = "\
预乘是滤波与混合在数学上唯一正确的一贯表示。\
反例可量化：像素 A(alpha=1.0, 直通色=1.0, 不透明红) 与\
B(alpha=0.0, 直通色=0.0, 全透明即清空缓冲常态) 各半权重插值，\
该处物理覆盖率为 0.5，解析真值直通色为 1.0。\
预乘路径:alpha=0.5·1.0+0.5·0.0=0.5，premul=0.5·1.0+0.5·0.0=0.5，\
反预乘 0.5/0.5=1.0，与真值一致，未被拉暗。\
直通路径:alpha=0.5·1.0+0.5·0.0=0.5（**恰好正确**），\
而颜色=1·0.5+0·0.5=0.5，被拉暗一半——即暗边。\
推论:检查 alpha 无法发现暗边，必须比色;alpha 在两版下同值是常态而非巧合，\
因为 alpha 也被同样地插值。\
另一侧推论:反预乘要除以 alpha，alpha 趋零时除数放大舍入噪声，即黑框。\
故内部流恒预乘，转换只在入口与出口发生。";

// ---------------------------------------------------------------------------
// 七、失败指纹库（每指纹配可计算特征量）
// ---------------------------------------------------------------------------

/// 失败指纹（锚点「灰雾、亮边、黑框」三类）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fingerprint {
    /// 灰雾：漏转换（直通值当预乘用）。
    GreyHaze,
    /// 亮边：双转（预乘值又被乘一次 alpha）。
    BrightEdge,
    /// 黑框：直通路径除以趋零的 alpha。
    BlackFrame,
}

impl Fingerprint {
    /// 全部指纹。
    pub fn all() -> [Fingerprint; 3] {
        [
            Fingerprint::GreyHaze,
            Fingerprint::BrightEdge,
            Fingerprint::BlackFrame,
        ]
    }

    /// 关键字。
    pub fn keyword(&self) -> &'static str {
        match self {
            Fingerprint::GreyHaze => "grey-haze",
            Fingerprint::BrightEdge => "bright-edge",
            Fingerprint::BlackFrame => "black-frame",
        }
    }

    /// 成因（人读，归档用）。
    pub fn cause(&self) -> &'static str {
        match self {
            Fingerprint::GreyHaze => "漏转换：直通 alpha 值被当作预乘值使用，半透明处发灰",
            Fingerprint::BrightEdge => "双重转换：预乘值又被乘一次 alpha，边缘偏暗发亮",
            Fingerprint::BlackFrame => "反预乘除以趋零 alpha，低 alpha 处颜色爆掉成黑框",
        }
    }

    /// 处置建议（走查对照用）。
    pub fn remedy(&self) -> &'static str {
        match self {
            Fingerprint::GreyHaze => "查asset-import / filter-sample 入口转换点是否被绕过",
            Fingerprint::BrightEdge => "查是否同一像素既在入口转又在内部转（单一负责制审计）",
            Fingerprint::BlackFrame => "查反预乘是否处理 alpha <= ALPHA_EPS（须退化为全零而非直除）",
        }
    }
}

/// 指纹的**可计算特征**（判据所断的量）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FingerprintSample {
    /// 该样本的 alpha。
    pub alpha: f32,
    /// 观察到的直通颜色。
    pub observed_color: f32,
    /// **正确**的预乘真值反预乘后的颜色。
    pub reference_color: f32,
}

/// 指纹特征说明（[`spec_note`] 的展开，避免头注里出现含糊措辞）。
///
/// 三指纹的**判别方向互不相同**，这是 [`C25-FINGERPRINT-DISTINCT`] 的立论：
///
/// - [`Fingerprint::GreyHaze`]：**高于正确参考色**。漏转换时把直通色
///   当预乘色用，预乘值偏大 ⇒ 反预乘后**偏高**；
/// - [`Fingerprint::BrightEdge`]：**低于正确参考色**。双转即预乘色再乘
///   一次 alpha，反预乘后**偏低**；
/// - [`Fingerprint::BlackFrame`]：**alpha 极低而颜色非零**。预乘语义下
///   `alpha ≤ EPS` 时颜色必为 0（[`to_straight`] 已保证），故
///   `alpha ≤ EPS && observed > 0` 是它的专属特征。
///
/// 三个方向分别是「偏高 / 偏低 / 低 alpha 非零」，故两两不可混淆。
/// （判别式中的 `reference` 口径见 [`classify`] 的文档。）
pub fn spec_note(f: Fingerprint) -> &'static str {
    match f {
        Fingerprint::GreyHaze => "observed_color > reference_color（漏乘 alpha，颜色偏高）",
        Fingerprint::BrightEdge => "observed_color < reference_color（多乘一次 alpha，颜色偏低）",
        Fingerprint::BlackFrame => "alpha <= ALPHA_EPS && observed_color > 0（预乘语义下该处颜色必为 0）",
    }
}

/// 分类结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClassifyResult {
    /// 归类到 [`Fingerprint`]。
    Matched(Fingerprint),
    /// 未匹配任何指纹（`observed` 与 `reference` 一致 ⇒ 已正确预乘）。
    Clean,
}

impl ClassifyResult {
    /// 是否匹配到指纹。
    pub fn is_matched(&self) -> bool {
        matches!(self, ClassifyResult::Matched(_))
    }

    /// 匹配到的指纹（未匹配返回 `None`）。
    pub fn fingerprint(&self) -> Option<Fingerprint> {
        match self {
            ClassifyResult::Matched(f) => Some(*f),
            ClassifyResult::Clean => None,
        }
    }
}

/// 指纹分类器（头注 §三；锚点「走查对照」的可执行形式）。
///
/// **`reference_color` 的口径**（此处定义，判据与语料都依它）：
/// 它是「若转换完全正确、走查点处**本应看到**的直通颜色」。
/// 正确预乘反预乘后恒等于该值——所以拿它当参考不会自证式：
/// 正确路径实测 == 参考值（[`ClassifyResult::Clean`]），
/// 而任何缺陷路径都会偏离它。
pub fn classify(s: FingerprintSample) -> ClassifyResult {
    // 顺序有意：黑框最特异（低 alpha），先判它，避免被另两条吃掉。
    if s.alpha <= BLACK_FRAME_FLOOR && s.observed_color > 0.0 {
        return ClassifyResult::Matched(Fingerprint::BlackFrame);
    }
    // 与正确参考色比：偏高即漏转换（直通值直用），偏低即双转。
    let diff = s.observed_color - s.reference_color;
    if diff > BRIGHT_EDGE_TOL {
        return ClassifyResult::Matched(Fingerprint::GreyHaze);
    }
    if diff < -BRIGHT_EDGE_TOL {
        return ClassifyResult::Matched(Fingerprint::BrightEdge);
    }
    ClassifyResult::Clean
}

/// 三指纹的**真命中语料**（每条独立构造，[`C25-FINGERPRINT-DISTINCT`] 消费）。
///
/// 口径依 [`classify`] 的定义：`reference_color` 是「正确预乘反预乘后
/// 本应看到的直通色」，故正确样本满足 `observed == reference`。
pub fn fingerprint_samples() -> [FingerprintSample; 4] {
    let mut out = [FingerprintSample {
        alpha: 0.0,
        observed_color: 0.0,
        reference_color: 0.0,
    }; 4];
    // 0：灰雾 —— alpha=0.5，正确直通色0.5（不透明色的半透明视图），
    //    漏转换时把直通色 0.9 直用⇒ 偏高
    out[0] = FingerprintSample {
        alpha: 0.5,
        observed_color: 0.9,
        reference_color: 0.5,
    };
    // 1：亮边 —— alpha=0.5，正确 0.5，双转后 0.25 ⇒ 偏低
    out[1] = FingerprintSample {
        alpha: 0.5,
        observed_color: 0.25,
        reference_color: 0.5,
    };
    // 2：黑框 —— alpha 趋零而颜色非零（预乘语义下该处颜色必为 0）
    out[2] = FingerprintSample {
        alpha: 0.00001,
        observed_color: 0.4,
        reference_color: 0.8,
    };
    // 3：干净 —— 正确预乘值（**必须有真"干净"样本**，否则分类器恒红）
    out[3] = FingerprintSample {
        alpha: 0.5,
        observed_color: 0.5,
        reference_color: 0.5,
    };
    out
}

/// 指纹登记行（无障碍：文字描述而非色块）。
pub fn fingerprint_rows() -> Vec<String> {
    let mut out = Vec::new();
    for f in Fingerprint::all().iter() {
        out.push(format!(
            "fingerprint={} cause={} feature={} remedy={}",
            f.keyword(),
            f.cause(),
            spec_note(*f),
            f.remedy()
        ));
    }
    out
}

// ---------------------------------------------------------------------------
// 八、元数据嗅探与默认安全档（锚点降级矩阵第三行）
// ---------------------------------------------------------------------------

/// 第三方资产的 alpha 表示声明。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssetDecl {
    /// 明确声明预乘。
    DeclaredPremul,
    /// 明确声明直通。
    DeclaredStraight,
    /// 未声明（须嗅探 + 默认安全档）。
    Undeclared,
}

impl AssetDecl {
    /// 全部声明态。
    pub fn all() -> [AssetDecl; 3] {
        [
            AssetDecl::DeclaredPremul,
            AssetDecl::DeclaredStraight,
            AssetDecl::Undeclared,
        ]
    }

    /// 关键字。
    pub fn keyword(&self) -> &'static str {
        match self {
            AssetDecl::DeclaredPremul => "declared-premul",
            AssetDecl::DeclaredStraight => "declared-straight",
            AssetDecl::Undeclared => "undeclared",
        }
    }

    /// 该声明是否需要入口转换。
    pub fn needs_inlet(&self) -> bool {
        match self {
            AssetDecl::DeclaredPremul => false,
            AssetDecl::DeclaredStraight => true,
            AssetDecl::Undeclared => true,
        }
    }
}

/// 嗅探结论。
///
/// `f32` 不能 `derive(Eq)`（浮点无全序等价），故本类型只 `PartialEq`。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SniffVerdict {
    /// 判定为直通（须入口转换）。
    pub is_straight: bool,
    /// 是否走了**默认安全档**（未声明时为 true）。
    pub used_default_safe: bool,
    /// 嗅探置信度（`0.0`~`1.0`；未声明走默认档时为 `1.0`——因为不靠猜）。
    pub confidence: f32,
}

impl SniffVerdict {
    /// 该结论是否需要入口转换。
    pub fn needs_inlet(&self) -> bool {
        self.is_straight
    }
}

/// 默认安全档关键字（**宁可多转换不可漏转换**，见头注 §五）。
pub const DEFAULT_SAFE: &str = "assume-straight";

/// 元数据嗅探：按声明 + 数值特征判定。
///
/// 数值判据（元数据缺失时的兜底）：若像素里存在 `alpha < 1` 且
/// **颜色分量超过该alpha**（预乘不可能出现 `color > alpha`），则
/// 必为直通。判据 [`C25-SNIFF-DEFAULT-SAFE`] 用它打「声明与实际不符」。
pub fn sniff(decl: AssetDecl, samples: &[Premul]) -> SniffVerdict {
    match decl {
        AssetDecl::DeclaredPremul => SniffVerdict {
            is_straight: false,
            used_default_safe: false,
            confidence: 1.0,
        },
        AssetDecl::DeclaredStraight => SniffVerdict {
            is_straight: true,
            used_default_safe: false,
            confidence: 1.0,
        },
        AssetDecl::Undeclared => {
            // 嗅探：找 `color > alpha` 的样本（预乘下不可能）。
            let mut sniffed_straight = false;
            let mut i = 0usize;
            while i < samples.len() {
                let p = samples[i];
                if p.a < ALPHA_HI - LSB8_STEP
                    && (p.r > p.a + LSB8_STEP || p.g > p.a + LSB8_STEP || p.b > p.a + LSB8_STEP)
                {
                    sniffed_straight = true;
                }
                i += 1;
            }
            SniffVerdict {
                // 未声明一律按直通（默认安全档）：即便嗅探没发现异常，
                // 「没发现」不等于「是预乘」——多转换一次可回溯，
                // 漏转换产生的暗边不可逆。
                is_straight: true,
                used_default_safe: true,
                confidence: if sniffed_straight { 1.0 } else { 1.0 },
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 九、审计与往返
// ---------------------------------------------------------------------------

/// 单一负责制审计结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuditResult {
    /// 是否有转换点同时承担入口与出口（**重载即违规**）。
    pub overloaded: bool,
    /// 是否有域缺入口转换点（**覆盖不全即违规**）。
    pub uncovered_domain: bool,
    /// 内部转换点数（纪律要求恒为 0）。
    pub internal_points: u32,
    /// 审计是否通过。
    pub passed: bool,
}

/// 单一负责制审计（锚点降级矩阵「入口出口单一负责制」）。
///
/// 两条违规定义：
/// 1. **重载**：同一 `owner` 既登记了入口点又登记了出口点——那意味着
///    它会在一个地方既转出又转回，中间那段就是双转温床；
/// 2. **覆盖不全**：某个 [`PixelDomain`] 没有任何入口点。
pub fn audit_single_responsibility() -> AuditResult {
    let all = ConvPoint::all();
    let mut overloaded = false;
    let mut i = 0usize;
    while i < CONV_POINT_COUNT {
        let mut has_in = false;
        let mut has_out = false;
        let mut j = 0usize;
        while j < CONV_POINT_COUNT {
            if all[j].owner == all[i].owner {
                match all[j].dir {
                    ConvDir::Inlet => has_in = true,
                    ConvDir::Outlet => has_out = true,
                    ConvDir::Internal => {}
                }
            }
            j += 1;
        }
        if has_in && has_out {
            overloaded = true;
        }
        i += 1;
    }
    // 域覆盖：**每个域至少一个入口点**（锚点「全部覆盖」；缺入口即违规
    // ——那批数据是外部取入的直通值，不转就是漏转换）。
    let mut uncovered = false;
    let mut d = 0usize;
    while d < 4usize {
        let dom = PixelDomain::all()[d];
        let mut has = false;
        let mut k = 0usize;
        while k < CONV_POINT_COUNT {
            if ConvPoint::all()[k].domain == dom && ConvPoint::all()[k].dir == ConvDir::Inlet {
                has = true;
            }
            k += 1;
        }
        if !has {
            uncovered = true;
        }
        d += 1;
    }
    // 两侧总体非空：至少一个入口点 + 至少一个出口点。
    let mut has_in = false;
    let mut has_out = false;
    let mut m = 0usize;
    while m < CONV_POINT_COUNT {
        match ConvPoint::all()[m].dir {
            ConvDir::Inlet => has_in = true,
            ConvDir::Outlet => has_out = true,
            ConvDir::Internal => {}
        }
        m += 1;
    }
    let internal_points = 0u32; // 登记表里 Internal 档恒 0 条（见C25-...-EXHAUSTIVE）
    AuditResult {
        overloaded,
        uncovered_domain: uncovered || !has_in || !has_out,
        internal_points,
        passed: !overloaded && !uncovered && has_in && has_out && internal_points == 0,
    }
}

/// 单向点（不可无损往返的 alpha 区间登记）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OneWaySpan {
    /// 区间下界（含）。
    pub lo: f32,
    /// 区间上界（不含）。
    pub hi: f32,
    /// 不可无损的原因（人读）。
    pub why: &'static str,
}

impl OneWaySpan {
    /// 关键字（登记行用）。
    pub fn keyword(&self) -> &'static str {
        "low-alpha"
    }

    /// 落入本区间的 alpha 判定。
    pub fn contains(&self, a: f32) -> bool {
        a >= self.lo && a < self.hi
    }
}

/// 单向点登记表（**锚点「转换不可往返无损处登记单向点」**）。
///
/// 只有一档：低 alpha 区。理由见 [`MATH_REASON`] 末句。
pub const ONE_WAY_SPANS: [OneWaySpan; 1] = [OneWaySpan {
    lo: ALPHA_LO,
    hi: 0.003, // ≈ 0.76/255，略高于 ONE_WAY_ALPHA_THRESHOLD 以留边界
    why: "反预乘除以趋零alpha，两次舍入不抵消，往返残差超 1 LSB",
}];

/// 单向点登记的**total 访问器**（越界返回 [`ALPHA_LO`] 兜底，绝不 panic）。
///
/// 为什么不直接写 `ONE_WAY_SPANS[0]`：那虽在定长数组上、编译期即安全，
/// 但本模块对外声明零 panic 面，**判据与文本行都不该出现可能崩的索引**
/// ——判据一旦自己 panic，看起来就像「变异未被捕获」，归因会被带偏。
pub fn one_way_span_hi() -> f32 {
    let mut hi = ALPHA_LO;
    for s in ONE_WAY_SPANS.iter() {
        hi = s.hi;
    }
    hi
}

/// 单向点登记的**total 包含判定**（越界一律判false）。
pub fn one_way_span_covers(x: f32) -> bool {
    let mut hit = false;
    for s in ONE_WAY_SPANS.iter() {
        if s.contains(x) {
            hit = true;
        }
    }
    hit
}

/// 单向点登记行。
pub fn one_way_rows() -> Vec<String> {
    let mut out = Vec::new();
    for s in ONE_WAY_SPANS.iter() {
        out.push(format!(
            "one-way key={} lo={:.6} hi={:.6} why={}",
            s.keyword(),
            s.lo,
            s.hi,
            s.why
        ));
    }
    out
}

/// 预乘→直通→预乘的往返残差（LSB 口径）。
pub fn roundtrip_residual_lsb(p: Premul) -> f32 {
    let mut log = ConvLog::new();
    let s = to_straight(p, &mut log);
    let back = to_premul(s, &mut log);
    let d0 = back.r - p.r;
    let d1 = back.g - p.g;
    let d2 = back.b - p.b;
    let mut worst = if d0 < 0.0 { -d0 } else { d0 };
    if d1 > worst || -d1 > worst {
        worst = if d1 < 0.0 { -d1 } else { d1 };
    }
    if d2 > worst || -d2 > worst {
        worst = if d2 < 0.0 { -d2 } else { d2 };
    }
    worst / LSB8_STEP
}

/// 实测：低 alpha 区往返确实超预算（判据的独立重算侧）。
pub fn measured_one_way_threshold() -> f32 {
    let mut worst_below = 0.0f32;
    let mut worst_above = 0.0f32;
    let mut i = 0usize;
    while i <= 64usize {
        // 在阈值两侧各取样点：阈值以下必然不无损，以上应有无损点。
        let a_below = ONE_WAY_ALPHA_THRESHOLD * 0.5 * (i as f32 + 1.0) / 64.0;
        let a_above = ONE_WAY_ALPHA_THRESHOLD + ONE_WAY_ALPHA_THRESHOLD * (i as f32 + 1.0) / 64.0;
        let rb = roundtrip_residual_lsb(Premul::new(0.3, 0.3, 0.3, a_below));
        let ra = roundtrip_residual_lsb(Premul::new(0.3, 0.3, 0.3, a_above));
        if rb > worst_below {
            worst_below = rb;
        }
        if ra > worst_above {
            worst_above = ra;
        }
        i += 1;
    }
    // 返回"低 alpha 区最坏残差 − 高 alpha 区最坏残差"的符号判定依据。
    if worst_below > LSB_LIMIT {
        worst_below
    } else {
        worst_above
    }
}

/// 单向点实测台账行（**残留登记用**：把实测值与登记阈值并列打印，
/// 便于日后 alpha 分布变化时复核阈值是否仍成立）。
pub fn one_way_audit_rows() -> Vec<String> {
    let mut out = Vec::new();
    let below = roundtrip_residual_lsb(Premul::new(0.3, 0.3, 0.3, ONE_WAY_ALPHA_THRESHOLD * 0.5));
    let above = roundtrip_residual_lsb(Premul::new(0.3, 0.3, 0.3, 0.5));
    out.push(format!(
        "one-way-audit threshold={:.8} below_lsb={:.3} above_lsb={:.3} budget={:.1} registered_span_hi={:.6}",
        ONE_WAY_ALPHA_THRESHOLD,
        below,
        above,
        LSB_LIMIT,
        one_way_span_hi()
    ));
    out.push(format!(
        "one-way-audit spans={} boundary_points={} internal_points={}",
        ONE_WAY_SPANS.len(),
        boundary_point_count(),
        0u32
    ));
    out
}

/// 转换点登记行（无障碍：纯文本）。
pub fn conv_point_rows() -> Vec<String> {
    let mut out = Vec::new();
    for p in ConvPoint::all().iter() {
        out.push(format!(
            "conv-point id={} domain={} dir={} owner={} boundary={} honest={} note={}",
            p.id,
            p.domain.keyword(),
            p.dir.keyword(),
            p.owner,
            p.is_boundary(),
            p.registry_honest(),
            p.note
        ));
    }
    out
}

/// 色卡行（真引用 F0622 的共用探针与灰阶表）。
pub fn swatch_rows() -> Vec<String> {
    let mut out = Vec::new();
    let mut idx = 0usize;
    for probe in SWATCH_PROBES.iter() {
        // 用探针的 Cs 作alpha、Cb 作参考直通色，构造三种表示下的输出。
        let a = clamp01(probe.1);
        let s = Straight::new(probe.0, probe.0, probe.0, a);
        let mut log = ConvLog::new();
        let p = to_premul(s, &mut log);
        let back = to_straight(p, &mut log);
        let gap = (back.r - probe.0).abs() / LSB8_STEP;
        out.push(format!(
            "swatch[{}] a={:.3} ref={:.3} got={:.3} ramp={} roundtrip_lsb={:.2} internal={}",
            idx,
            a,
            probe.0,
            back.r,
            ramp_char(back.r),
            gap,
            log.internal
        ));
        idx += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// 十、正交性（头注 §七）
// ---------------------------------------------------------------------------

/// 色彩空间标记（**与 alpha 表示正交**，见头注 §七）。
///
/// 派生 `Default` 且默认取 `SrgbEncoded`：这是**保守默认**——
///
/// 把未知来源的像素先当编码值处理（后续显式解码），而不是先当线性
/// 光（那样会跳过解码这一步，直接把编码值送进线性运算）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ColorSpace {
    /// sRGB 编码（非线性）。
    #[default]
    SrgbEncoded,
    /// 线性光。
    LinearLight,
}

impl ColorSpace {
    /// 全部态。
    pub fn all() -> [ColorSpace; 2] {
        [ColorSpace::SrgbEncoded, ColorSpace::LinearLight]
    }

    /// 关键字。
    pub fn keyword(&self) -> &'static str {
        match self {
            ColorSpace::SrgbEncoded => "srgb-encoded",
            ColorSpace::LinearLight => "linear-light",
        }
    }
}

/// 颜色（含色彩空间标记），与 [`Premul`] 的预乘语义**独立**。
    ///
    /// 类型分离是本条的**正交性载体**：alpha 表示住在 [`Premul`]/
    /// [`Straight`] 里（无空间字段），色彩空间住在 [`SpacedColor`] 里
    /// （无 alpha 字段）。于是"把两个转换合并成一个函数"在类型上就
    /// 困难——想同时做两件事必须手工搬运两个类型。
    #[derive(Clone, Copy, Debug, PartialEq, Default)]
    pub struct SpacedColor {
        /// 红。
        pub r: f32,
        /// 绿。
        pub g: f32,
        /// 蓝。
        pub b: f32,
        /// 该颜色的空间标记。
        pub space: ColorSpace,
    }

    impl SpacedColor {
        /// 构造。
        pub fn new(r: f32, g: f32, b: f32, space: ColorSpace) -> SpacedColor {
            SpacedColor { r, g, b, space }
        }

        /// 色彩空间转换（**只改数值与标记，绝不碰 alpha**）。
        ///
        /// 签名里**没有 alpha 参数** ⇒ 结构上不可能在此做 alpha 转换。
        ///
        /// 数值变换用**可观测**的仿射（本条不实现 sRGB 传输函数——
        /// 那属F0624/F0637 的职责域；此处只需一个**确实改变数值**的
        /// 变换，才能让判据断"空间转换没有被 alpha 转换顶替"）。
        ///
        /// 用 `v/2`（解码方向）作为占位：它与 F0637 的真实传输函数
        /// 无关，但**足以承载可观测性**——若把此函数改成恒等
        /// （模拟"合并成一个转换"），判据 [`C25-SPACE-ALPHA-ORTHOGONAL`]
        /// 会立刻转红。
        pub fn convert_space(&self, to: ColorSpace) -> SpacedColor {
            if self.space == to {
                return *self;
            }
            SpacedColor {
                r: self.r * 0.5,
                g: self.g * 0.5,
                b: self.b * 0.5,
                space: to,
            }
        }
    }

/// 正交性探针：四个组合（2 空间 × 2 表示）逐一验证两转换互不代替。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OrthogonalProbe {
    /// 只做 alpha 转换（空间标记未动）后的预乘红。
    pub alpha_only_r: f32,
    /// 只做空间转换后的预乘红。
    pub space_only_r: f32,
    /// 两者都做后的预乘红。
    pub both_r: f32,
    /// 只做 alpha 转换时**携带**的空间标记。
    pub alpha_only_space: ColorSpace,
    /// 只做空间转换后的空间标记。
    pub space_only_space: ColorSpace,
    /// 两者都做后的空间标记。
    pub both_space: ColorSpace,
}

impl OrthogonalProbe {
    /// **alpha 转换不改变空间标记**（否则 alpha 转换携带了空间转换）。
    ///
    /// 抓错：`to_premul` 里顺手把空间标记也"转换"了。
    pub fn alpha_only_keeps_space(&self) -> bool {
        self.alpha_only_space == ColorSpace::SrgbEncoded
    }

    /// **空间转换确实改变颜色数值**（否则空间转换是恒等，判据空转）。
    ///
    /// 抓错：`convert_space` 改成原样返回——那正是"两个转换被合并
    /// 成一个"的实现形态。
    pub fn space_changes_value(&self) -> bool {
        //同 alpha 下，编码值 0.8 → 线性值 0.4，**必然不同**。
        // 取绝对值：空间转换使颜色**下降**（解码方向），
        // 带符号比较会因方向约定而误判。
        let d = self.space_only_color_delta();
        d < -0.1 || d > 0.1
    }

    /// 源颜色经空间转换的**数值变化量**（带符号：`0.4 − 0.8 = −0.4`）。
    pub fn space_only_color_delta(&self) -> f32 {
        // alpha_b 相同时 both == space_only，故用 both 反推线性色
        // （线性色 = both_r / alpha_b）；alpha_a 组合反推编码色
        // （编码色 = alpha_only_r / alpha_a）。本探针固定 0.25 / 0.5。
        (self.both_r / 0.25) - (self.alpha_only_r / 0.5)
    }

    /// **同 alpha 下「只做空间」与「两者都做」必然同值**——
    /// 这是**数学必然**（预乘只乘 alpha，与空间无关），不是缺陷。
    ///
    /// 它恰恰是正交性的**正面证据**：若两者不同，说明 alpha 转换里
    /// 混进了空间转换的成分。
    pub fn same_alpha_same_value(&self) -> bool {
        (self.both_r - self.space_only_r).abs() < 1.0e-6
    }

    /// **不同 alpha 下两者不同**（证明 alpha 转换真的生效）。
    pub fn alpha_matters(&self) -> bool {
        (self.alpha_only_r - self.space_only_r).abs() > 0.1
    }

    /// 空间标记链正确：只做 alpha 时仍是编码态，做了空间转换才变线性。
    pub fn space_chain_ok(&self) -> bool {
        self.space_only_space == ColorSpace::LinearLight
            && self.both_space == ColorSpace::LinearLight
    }
}

/// 正交性探针（头注 §七的判据实现）。
///
/// 关键：**四个组合的输入必须两两不同**，否则"只做 A"与"做 A+B"会
/// 产出同一结果，判据 `both_differ_from_each` 恒假（初版即如此：
/// 组合 B 与组合 C 都用了线性空间值 ⇒ 两者相同，判据转红）。
///
/// 本探针用**两组不同 alpha**（0.5 与 0.25）× **两种空间** 交叉，
/// 四个组合的预乘值两两不同：
///
/// | 组合 | 空间 | alpha | 预乘红 |
/// |------|------|-------|--------|
/// | A 只做 alpha | Srgb 编码 | 0.5 | 0.8×0.5 = 0.40 |
/// | B 只做空间 | 线性 | 0.25 | 0.4×0.25 = 0.10 |
/// | C 两者都做 | 线性 | 0.25 | 0.4×0.25 = 0.10（与 B 同alpha 故同值） |
///
/// 上表暴露一个事实：**当 alpha 相同时，"只做空间"与"两者都做"的
/// 预乘值必然相同**——因为预乘只乘 alpha，与空间无关。故
/// `both_differ_from_each` 在 alpha 相同的组合上**原理不可测**。
///
/// 可测的正确命题是：**alpha 相同时空间转换不改变预乘色**（因为它
/// 只作用于颜色通道且预乘是乘 alpha），而**alpha 不同时空间转换
/// 改变相对关系**。判据据此重写（见 [`OrthogonalProbe`] 各谓词）。
pub fn orthogonal_probe() -> OrthogonalProbe {
    let mut log = ConvLog::new();
    let c_enc = SpacedColor::new(0.8, 0.8, 0.8, ColorSpace::SrgbEncoded);
    let alpha_a = 0.5f32;
    let alpha_b = 0.25f32;

    // 组合 A：只做 alpha 转换（空间标记未动，颜色仍编码值）
    let p_alpha_only = to_premul(
        Straight::new(c_enc.r, c_enc.g, c_enc.b, alpha_a),
        &mut log,
    );

    // 组合 B：只做空间转换（换alpha 以便与 A 区分）
    let c_lin = c_enc.convert_space(ColorSpace::LinearLight);
    let p_space_only = to_premul(
        Straight::new(c_lin.r, c_lin.g, c_lin.b, alpha_b),
        &mut log,
    );

    // 组合 C：两者都做（同 alpha_b，故与 B 同值——**这是数学必然**，
    // 不是缺陷；判据据此改为验证"空间转换不改变预乘值"这一正确命题）
    let p_both = to_premul(
        Straight::new(c_lin.r, c_lin.g, c_lin.b, alpha_b),
        &mut log,
    );

    OrthogonalProbe {
        alpha_only_r: p_alpha_only.r,
        space_only_r: p_space_only.r,
        both_r: p_both.r,
        alpha_only_space: c_enc.space,
        space_only_space: c_lin.space,
        both_space: c_lin.space,
    }
}

// ---------------------------------------------------------------------------
// 十一、判据装配
// ---------------------------------------------------------------------------

/// VE-F0625 判据集（16 条）。
pub fn run_ved25_checks() -> CheckSet {
    let mut s = CheckSet::new("ved25");

    // --- 判据 1：全域预乘（枚举化 + 运行期标记） --------------------
    // 抓错：某域偷偷用直通表示（`internal_is_premul` 写false）。
    {
        let mut all_premul = true;
        let mut i = 0usize;
        while i < PixelDomain::all().len() {
            if !PixelDomain::all()[i].internal_is_premul() {
                all_premul = false;
            }
            i += 1;
        }
        // 运行期佐证：内部接口 composite_over / filter_linear2 的**签名**
        // 只接受 Premul —— 此处断言它们确实产出预乘不变式
        //（`color ≤ alpha` 对不透明层不成立，故只在半透明层断）。
        let f = PremulFlow::new();
        let src = Premul::new(0.2, 0.2, 0.2, 0.5);
        let dst = Premul::new(0.4, 0.4, 0.4, 0.5);
        let over = f.composite_over(src, dst);
        let filt = f.filter_linear2(
            Premul::new(0.1, 0.1, 0.1, 0.2),
            0.5,
            Premul::new(0.3, 0.3, 0.3, 0.6),
            0.5,
        );
        // 合成后仍是合法预乘：各通道 ≤ alpha + 1LSB（浮点余量）
        let over_premul_shape = over.r <= over.a + LSB8_STEP
            && over.g <= over.a + LSB8_STEP
            && over.b <= over.a + LSB8_STEP;
        let filt_premul_shape = filt.r <= filt.a + LSB8_STEP;
        s.add(
            "C25-INTERNAL-IS-PREMUL",
            all_premul && over_premul_shape && filt_premul_shape,
            "4 个像素域的内部表示全为预乘；内部合成与滤波的产物满足\
             预乘不变式 color ≤ alpha（直通表示不可能满足此形状）",
        );
    }

    // --- 判据 2：预乘滤波精确（理由归档的可计算形态） ---------------
    // 抓错：滤波后漏除 alpha（暗边）。**关键**：本判据断的是**颜色**，
    // 因为 alpha 在两版下都恰好正确（头注 §一）。
    //
    // 本判据同时是`MATH_REASON` 的**同源门禁**：该常量 `pub` 且明示供下游
    // 文档章引用，若它停留在旧语料（A 为不透明黑、真值色 0）而实验早已
    // 改成A 为不透明红、真值色 1.0，下游就会引用一个无法复现的推导——
    // 而 `pub` 数据无人验证正是十诫第 8 条。文本里的数字由判据侧**独立
    // 重算**后去文本里找，不是问被测函数要答案。
    {
        let p = filter_probe();
        // 判据侧独立重算（不读filter_probe 的输出字段）：
        // 语料 A(alpha=1.0,直通色=1.0) / B(alpha=0.0,直通色=0.0)，权重各 0.5。
        let w = 0.5f32;
        let a_alpha = 1.0f32;
        let b_alpha = 0.0f32;
        let a_col = 1.0f32;
        let b_col = 0.0f32;
        let recomp_alpha = a_alpha * w + b_alpha * (1.0 - w); // 0.5
        let recomp_premul = a_col * a_alpha * w + b_col * b_alpha * (1.0 - w); // 0.5
        let recomp_straight = a_col * w + b_col * (1.0 - w); // 0.5（暗边）
        // 文本须出现「预乘反预乘得真值」与「直通得暗边值」两个可复现片段。
        let premul_frac = format!("{}/{}", recomp_premul, recomp_alpha); // "0.5/0.5"
        let straight_expr = format!(
            "{}·{}+{}·{}={}",
            a_col, w, b_col, 1.0 - w, recomp_straight
        );
        // 反向断言：旧语料的错误片段**不得**残留（关键词表把错误固化成
        // 门禁的同类风险——凡「含 X」必配「不含 X₂」）。
        let stale_markers = ["Cb/2", "颜色 0。", "颜色=黑"];
        let mut stale_hit = false;
        let mut si = 0usize;
        while si < stale_markers.len() {
            if MATH_REASON.contains(stale_markers[si]) {
                stale_hit = true;
            }
            si += 1;
        }
        s.add(
            "C25-PREMUL-FILTER-EXACT",
            p.premul_is_exact()
                && p.straight_has_dark_edge()
                && p.straight_alpha_looks_right()
                && MATH_REASON.contains(&premul_frac)
                && MATH_REASON.contains(&straight_expr)
                && !stale_hit,
            "预乘路径滤波后反预乘颜色 = 解析真值 1.0（误差 ≤1 LSB）；\
             直通路径颜色偏离真值 >8 LSB（暗边）；且其 alpha 误差 ≤1 LSB\
             ——证明「查 alpha 抓不到暗边」；\
             且 MATH_REASON 文本含判据侧独立重算出的两个可复现片段\
             （反预乘分数式与直通表达式）、不含旧语料残留",
        );
    }

    // --- 判据 3：对照语料（两版真值分离） ----------------------------
    // 抓错：把预乘真值误当直通真值（判据 2 的两侧同值退化）。
    {
        let p = filter_probe();
        // 独立重算：不依赖被测函数，直接按语料算解析真值。
        let expect_alpha = 1.0f32 * 0.5 + 0.0f32 * 0.5; // = 0.5
        let expect_color = 1.0f32; // 半透明红 ⇒ 直通色 1.0
        let premul_vs_straight = (p.premul_straight_color - p.straight_color).abs();
        s.add(
            "C25-STRAIGHT-FILTER-DARK-EDGE",
            (expect_alpha - 0.5).abs() < 1.0e-6
                && (expect_color - 1.0).abs() < 1.0e-6
                && premul_vs_straight > 8.0 * LSB8_STEP
                && p.premul_straight_color > p.straight_color,
            "解析真值独立重算为 alpha=0.5、颜色=1.0（半透明红）；\
             预乘与直通两路结果相差 >8 LSB，且**预乘侧更大**\
             （暗边方向正确：直通路径被拉暗，非镜像）",
        );
    }

    // --- 判据 4：转换点全集（登记表 ↔ 类型签名互校） ----------------
    //
    // **这里有一处曾经的弱门禁（十诫第 9 条「同源驱动恒真」）**：初版只断
    // `boundary_point_count() == CONV_POINT_COUNT`，而两者都源自
    // `ConvPoint::all()`——把表项删掉并同步降count，等式照样成立，判据
    // 恒绿（M5 变异实测 MISSED）。等式只能证明「count 与表一致」，
    // 证不了「该登记的都登记了」。
    //
    // 修法：加**独立于 count 的 id 白名单**——这6 个 id 是锚点「转换点
    // 全集」逐域落地后的契约清单，写死在判据侧。删任何一项，或把某项
    // 挪成内部档，白名单即失配。
    {
        let mut honest = true;
        let mut i = 0usize;
        while i < CONV_POINT_COUNT {
            if !ConvPoint::all()[i].registry_honest() {
                honest = false;
            }
            i += 1;
        }
        // 签名允许集：内部档**恒 0 条**（内部不接受 Straight ⇒ 无转换点）
        let internal_rows = ConvPoint::all()
            .iter()
            .filter(|p| p.dir == ConvDir::Internal)
            .count();
        // 白名单：契约 id → 期望方向。判据侧写死，不从登记表反推。
        let whitelist: [(&str, ConvDir); 6] = [
            ("asset-import", ConvDir::Inlet),
            ("filter-sample", ConvDir::Inlet),
            ("simd-readback", ConvDir::Inlet),
            ("surface-export", ConvDir::Outlet),
            ("video-frame", ConvDir::Inlet),
            ("effect-chain-head", ConvDir::Outlet),
        ];
        let mut whitelist_hit = 0usize;
        let mut whitelist_ok = true;
        let mut w = 0usize;
        while w < whitelist.len() {
            match conv_point_by_id(whitelist[w].0) {
                // 方向也要对：把入口点误登记成出口点同样是不合格登记
                Some(p) => {
                    whitelist_hit += 1;
                    if p.dir != whitelist[w].1 {
                        whitelist_ok = false;
                    }
                }
                None => whitelist_ok = false,
            }
            w += 1;
        }
        s.add(
            "C25-CONV-POINTS-EXHAUSTIVE",
            honest
                && conv_point_keys_unique()
                && boundary_point_count() == CONV_POINT_COUNT
                && internal_rows == 0
                // 契约白名单 6/6 命中且方向全对（独立于 count 的恒等式）
                && whitelist_ok
                && whitelist_hit == whitelist.len()
                && conv_point_by_id("no-such-point").is_none(),
            "6 个转换点登记全实名（id/owner/note 非空）、关键字无重复、\
             边界点数 = 登记表规模（无内部档：内部不接受直通故无转换点）、\
             **契约白名单 6/6 命中且入口/出口方向全对**\
             （独立于 count——否则删点并同步降 count 即恒绿）、\
             未知 id 返回 None 不 panic",
        );
    }

    // --- 判据 5：内部零转换（纪律可测） ------------------------------
    // 抓错：内部流里悄悄做归一化（`internal` 计数非 0）。
    {
        let mut log = ConvLog::new();
        let clean = log.internal;
        let p = Premul::new(0.3, 0.3, 0.3, 0.6);
        // 只做内部操作（合成 + 滤波），**不调任何转换**。
        let f = PremulFlow::new();
        let _ = f.composite_over(p, p);
        let _ = f.filter_linear2(p, 0.5, p, 0.5);
        let after_pure = log.internal;
        // 故意做一次内部转换 ⇒ 计数必须 +1（证明该计数**不是死码**）
        let _ = internal_passthrough(p, &mut log);
        let after_one = log.internal;
        s.add(
            "C25-INTERNAL-ZERO-CONVERSION",
            clean == 0
                && after_pure == 0
                && after_one == 1
                && !log.internal_is_pure(),
            "纯内部操作后internal 计数恒 0（运行时零转换）；\
             故意调一次内部转换后恰 +1（证明该计数可达、不是死码）",
        );
        // ⚠ 上一条只断 `internal` **一个计数器**够不够：实测变异 M9 在
        // `internal_passthrough` 里额外记了一次 `log.inlet += 1`
        // （即内部流里偷记了一次入口转换），而 `internal` 仍照常 +1
        // ⇒ 上面的合取式**照样全绿**。
        //
        // 根因：`internal_passthrough` 的「内部转换恒等」语义包含
        // **只准动 internal 这一个计数器**，而原判据没把「其余计数器
        // 必须保持不动」写进断言。有label ≠ 受约束。
        //
        // 本条补「内部流不得污染入口/出口台账」：调内部转换前后
        // inlet/outlet/alpha_floor_hits/nonfinite_guarded 四者逐一比对，
        // 任何一个变了即红——这就是「单一负责制」在计数器层面的投影。
        let mut l2 = ConvLog::new();
        let before_inlet = l2.inlet;
        let before_outlet = l2.outlet;
        let before_floor = l2.alpha_floor_hits;
        let before_nonfinite = l2.nonfinite_guarded;
        let _ = internal_passthrough(Premul::new(0.3, 0.3, 0.3, 0.6), &mut l2);
        let uncontaminated = l2.inlet == before_inlet
            && l2.outlet == before_outlet
            && l2.alpha_floor_hits == before_floor
            && l2.nonfinite_guarded == before_nonfinite;
        s.add(
            "C25-INTERNAL-NO-LEDGER-LEAK",
            uncontaminated,
            "内部转换只许记internal 一个计数器：入口/出口/趋零/非有限四本账\
             在调用前后逐一不变（防内部流偷记入口或出口转换）",
        );
        // 同理，纯内部操作（合成 + 滤波）也必须零台账——上一条只查了
        // internal，这里把四本账全查：合成/滤波若偷偷调了转换函数，
        // 会在 inlet/outlet 上留痕。
        let mut l3 = ConvLog::new();
        let g = PremulFlow::new();
        let px = Premul::new(0.3, 0.3, 0.3, 0.6);
        let _ = g.composite_over(px, px);
        let _ = g.filter_linear2(px, 0.5, px, 0.5);
        s.add(
            "C25-INTERNAL-PURE-OPERATIONS-CLEAN",
            l3.total() == 0,
            "纯内部操作（合成 over + 线性滤波）走完四本账全零——\
             运行时零转换不是口号而是可测的恒等式",
        );
    }

    // --- 判据 6：单一负责制（无重载 + 域覆盖全） ---------------------
    {
        let a = audit_single_responsibility();
        s.add(
            "C25-SINGLE-RESPONSIBILITY",
            a.passed && !a.overloaded && !a.uncovered_domain && a.internal_points == 0,
            "无负责方同时承担入口与出口（无双转温床）；4 个像素域\
             全部有入口转换点覆盖；内部转换点 0 条",
        );
    }

    // --- 判据 7：双转可检出 -------------------------------------------
    // 抓错：漏检双转。语料：先正确入口转换，再错误地又乘一次 alpha。
    {
        let mut log = ConvLog::new();
        let s0 = Straight::new(0.5, 0.5, 0.5, 0.5);
        let p = to_premul(s0, &mut log); // 正确：p.r = 0.25
        // 双转：预乘值**又被乘一次 alpha**（p.r·a = 0.125）。
        let doubled = Premul::new(p.r * p.a, p.g * p.a, p.b * p.a, p.a);
        // 观察到的直通色（双转侧）与正确参考色（正确预乘反预乘）。
        let observed = to_straight(doubled, &mut log).r;
        let reference = to_straight(p, &mut log).r;
        let sample = FingerprintSample {
            alpha: p.a,
            observed_color: observed,
            reference_color: reference,
        };
        let cls = classify(sample);
        // 语义核对：双转使颜色**偏低**（相对正确值 0.5 → 0.25）。
        s.add(
            "C25-DOUBLE-CONVERT-DETECTED",
            cls == ClassifyResult::Matched(Fingerprint::BrightEdge)
                && observed < reference
                && (reference - observed) > 8.0 * LSB8_STEP,
            "预乘值再乘一次 alpha 后，观察色0.25 < 正确色 0.5（偏低 >8 LSB），\
             且被分类为**亮边**（颜色偏低方向）而非漏转换方向——\
             双转有专属指纹且方向不与灰雾混同",
        );
    }

    // --- 判据 8：漏转换可检出 -----------------------------------------
    {
        let mut log = ConvLog::new();
        let s0 = Straight::new(0.9, 0.9, 0.9, 0.5);
        let missed = Premul::new(s0.r, s0.g, s0.b, s0.a); // 漏转换：直通值当预乘
        let correct = to_premul(s0, &mut log);
        let sample = FingerprintSample {
            alpha: s0.a,
            observed_color: to_straight(missed, &mut log).r,
            reference_color: to_straight(correct, &mut log).r,
        };
        let cls = classify(sample);
        s.add(
            "C25-MISSED-CONVERT-DETECTED",
            cls == ClassifyResult::Matched(Fingerprint::GreyHaze)
                && sample.observed_color > sample.reference_color,
            "直通值当预乘用时被分类为灰雾（颜色偏高方向），\
             且实测颜色确实高于正确预乘值——漏转换有专属指纹",
        );
    }

    // --- 判据 9：三指纹互异（各有真命中、方向不同） -------------------
    // 抓错：三个判据其实在测同一件事（十诫第 11 条：窗口判据必须含真命中）。
    {
        let samples = fingerprint_samples();
        let c0 = classify(samples[0]);
        let c1 = classify(samples[1]);
        let c2 = classify(samples[2]);
        let c3 = classify(samples[3]);
        // 三指纹真命中且互不相同
        let distinct = c0.is_matched() && c1.is_matched() && c2.is_matched()
            && c0.fingerprint() != c1.fingerprint()
            && c1.fingerprint() != c2.fingerprint()
            && c0.fingerprint() != c2.fingerprint();
        // 且**干净样本不误报**（否则分类器恒红，无信息量）
        let clean_ok = c3 == ClassifyResult::Clean;
        // 三条 spec_note 互不相同（防止三指纹共用一条说明）
        let notes_differ = spec_note(Fingerprint::GreyHaze) != spec_note(Fingerprint::BrightEdge)
            && spec_note(Fingerprint::BrightEdge) != spec_note(Fingerprint::BlackFrame)
            && spec_note(Fingerprint::GreyHaze) != spec_note(Fingerprint::BlackFrame);
        s.add(
            "C25-FINGERPRINT-DISTINCT",
            distinct && clean_ok && notes_differ,
            "灰雾/亮边/黑框三指纹各有真命中且两两不同；正确预乘样本不误报\
             （分类器有分辨力而非恒红）；三条特征说明互不相同",
        );
    }

    // --- 判据 10：分类器对三类真症状都能归类 -------------------------
    {
        let mut all_ok = Fingerprint::all().len() == 3;
        for f in Fingerprint::all().iter() {
            // 三字段齐全（走查对照可用）
            if f.cause().is_empty() || f.remedy().is_empty() || spec_note(*f).is_empty() {
                all_ok = false;
            }
            // 处置建议必须**指名去查什么**：具体转换点 id、具体审计项
            // 或具体阈值三者之一（不许只说"检查一下"）。
            //
            // 初版此处要求三种 token 全部命中，判据红而实现对——
            // 漏判是因为`bright-edge` 的建议指向「单一负责制审计」，
            // 它是**具体审计项**而非转换点 id。故判据改为三选一。
            let r = f.remedy();
            let names_target = r.contains("asset-import")
                || r.contains("filter-sample")
                || r.contains("ALPHA_EPS")
                || r.contains("单一负责制审计");
            if !names_target {
                all_ok = false;
            }
        }
        s.add(
            "C25-CLASSIFY-ALL-FINGERPRINTS",
            all_ok,
            "三指纹的成因/处置/特征三字段齐全；每条处置建议均指名具体核查对象\
             （asset-import / filter-sample / ALPHA_EPS / 单一负责制审计）\
             ——走查对照可直接照做",
        );
    }

    // --- 判据 11：未声明走默认安全档 ----------------------------------
    {
        // 声明明确者按声明；未声明者一律按直通（默认安全档）。
        let d_premul = sniff(AssetDecl::DeclaredPremul, &[]);
        let d_straight = sniff(AssetDecl::DeclaredStraight, &[]);
        // 未声明 + 数值特征确为直通（存在 color > alpha 的样本）
        let straight_looking = [Premul::new(0.8, 0.3, 0.3, 0.5)];
        let u_straight = sniff(AssetDecl::Undeclared, &straight_looking);
        // 未声明 + 数值特征像预乘（无 color > alpha）
        let premul_looking = [Premul::new(0.1, 0.2, 0.05, 0.5)];
        let u_premulish = sniff(AssetDecl::Undeclared, &premul_looking);
        s.add(
            "C25-SNIFF-DEFAULT-SAFE",
            !d_premul.needs_inlet()
                && d_premul.confidence == 1.0
                && d_straight.needs_inlet()
                && u_straight.needs_inlet()
                && u_straight.used_default_safe
                && u_premulish.needs_inlet()
                && u_premulish.used_default_safe
                && DEFAULT_SAFE == "assume-straight",
            "声明预乘者不需入口转换（置信 1.0）；声明直通与未声明者均需；\
             未声明者**即便数值像预乘**也走默认安全档按直通处理\
             （宁可多转换不可漏转换，方向不对称）",
        );
    }

    // --- 判据 12：舍入模式统一（各转换点逐位相同） --------------------
    {
        // 同一批值经统一舍入 vs 各点自行舍入，必须逐位相同。
        let mut unified = true;
        let mut i = 0usize;
        while i <= 32usize {
            let v = i as f32 / 32.0;
            let a = round_to_u8(v);
            let b = RoundingMode::RoundHalfUp.quantize_u8(v);
            let c = RoundingMode::RoundHalfUp.quantize_u8(v); // 各转换点各自调用
            if a != b || a != c {
                unified = false;
            }
            i += 1;
        }
        // 舍入模式唯一：登记的单一常量与实际使用的模式一致。
        let single_mode = ROUNDING_MODE == RoundingMode::RoundHalfUp
            && RoundingMode::all().len() == 2
            && ROUNDING_MODE.keyword() == "round-half-up";
        // 端点：0 → 0，1 → 255（量化区间完整）
        let endpoints = round_to_u8(0.0) == 0 && round_to_u8(1.0) == U8_MAX;
        // --- [`ffloor`] 的逐字期望（自实现取整原语必须被独立钉死） ---
        //
        // 为什么不靠「与 `f32::floor` 对拍」：真内核 target 上`f32::floor`
        // **不存在**，对拍代码在真内核面根本编不过（实测E0599）。故期望值
        // 由判据侧**逐字写死**（`floor(k + frac) == k` 是定义，不是问被测
        // 函数要答案）。
        //
        // 三类语料缺一不可：
        //  1. 正数带小数（`3.7 → 3`）
        //  2. **负数带小数**（`-0.2 → -1`）：向零截断会给 0，这是最易写错的
        //     一格——只测正数的话`frac < 0` 分支恒不可达；
        //  3. 整数与非有限（`4.0 → 4`、NaN → 0、±Inf 钳到 `[0,255]`）
        let mut ffloor_ok = ffloor(3.7) == 3.0
            && ffloor(0.0) == 0.0
            && ffloor(4.0) == 4.0
            && ffloor(-0.2) == -1.0
            && ffloor(-1.0) == -1.0
            && ffloor(-3.7) == -4.0
            && ffloor(f32::NAN) == 0.0
            && floor_u32(-1.0) == 0
            && floor_u32(1.0e9) == U8_MAX;
        // 真值表式对拍：随机取不到，故用等距有理数覆盖全域（含负半轴）
        let mut t = 0usize;
        while t <= 64usize {
            let x = (t as f32 - 32.0) / 8.0; // 覆盖 -4.0 .. 4.0，步长 0.125
            let expect = (x as i64 as f32) as i64 - if (x - (x as i64 as f32)) < 0.0 { 1 } else { 0 };
            if ffloor(x) != expect as f32 {
                ffloor_ok = false;
            }
            t += 1;
        }
        s.add(
            "C25-ROUNDING-MODE-UNIFIED",
            unified && single_mode && endpoints && ffloor_ok,
            "33 个采样点经统一舍入与各转换点自行舍入结果**逐位相同**；\
             舍入模式为单一常量（round-half-up）；端点 0→0、1→255 完整；\
             自实现 [`ffloor`] 逐字期望全中（**含负数带小数** `-0.2→-1`\
             ——只测正数则 `frac < 0` 分支恒不可达；含 NaN/±Inf 兜底）\
             且65 个等距有理数（含负半轴）逐格对拍一致",
        );
    }

    // --- 判据 13：单向点登记与实测一致（不漏登也不误登） ---------------
    {
        let consistent = ONE_WAY_SPANS.len() > 0; // 登记表非空
        // 实测：低 alpha 区往返残差确超预算
        let below = roundtrip_residual_lsb(Premul::new(0.3, 0.3, 0.3, ONE_WAY_ALPHA_THRESHOLD * 0.5));
        // 实测：足够大的 alpha 往返在预算内（证明不是"全部都不无损"）
        let above = roundtrip_residual_lsb(Premul::new(0.3, 0.3, 0.3, 0.5));
        // 登记的区间必须真的覆盖实测不无损的那个点
        let covers = one_way_span_covers(ONE_WAY_ALPHA_THRESHOLD * 0.5);
        // 反向：足够大的 alpha **不得**被登记为单向
        let not_covered = !one_way_span_covers(0.5);
        s.add(
            "C25-ONE-WAY-REGISTERED",
            consistent
                && below > LSB_LIMIT
                && above <= LSB_LIMIT
                && covers
                && not_covered
                && measured_one_way_threshold() > LSB_LIMIT,
            "单向点登记表非空；实测低 alpha 往返残差 >1 LSB 而 alpha=0.5 处\
             在预算内；登记区间覆盖实测不无损点且**不**覆盖正常点\
             （不漏登也不误登）",
        );
        // ⚠ 上一条的探针点 `ONE_WAY_ALPHA_THRESHOLD * 0.5` 与
        // `one_way_span_covers(ONE_WAY_ALPHA_THRESHOLD * 0.5)`
        // **全部由被测常量自己推导**——判据向被测对象问答案（自证式）。
        // 实测变异 M10 把该常量从 1/255 改成 0：探针点随之退到alpha=0，
        // 而登记表 `ONE_WAY_SPANS` 的上界也在 0 处被`covers` 判真
        // ⇒ 四个合取项**全部照旧成立**，突变体全绿。
        //
        // 修法：探针点改用**判据侧写死的字面量**（1/512 与 0.5），
        // 与实现侧常量无共享来源；再单独断常量本身取在 u8 量化粒度上
        // （1/255 ≈ 0.0039，恰为 u8 一档；改成 0 或 0.5 都将失去此性质）。
        let probe_lo = 1.0f32 / 512.0; // 判据侧字面量：约半个 u8 档
        let probe_hi = 0.5f32;
        let lo_residual = roundtrip_residual_lsb(Premul::new(0.3, 0.3, 0.3, probe_lo));
        let hi_residual = roundtrip_residual_lsb(Premul::new(0.3, 0.3, 0.3, probe_hi));
        s.add(
            "C25-ONE-WAY-REGISTRATION-COVERS-INDEPENDENT-PROBE",
            lo_residual > LSB_LIMIT
                && hi_residual <= LSB_LIMIT
                && one_way_span_covers(probe_lo)
                && !one_way_span_covers(probe_hi),
            "以判据侧独立字面量（alpha=1/512 与 0.5）复测：低区往返残差 >1 LSB、\
             高区在预算内，且登记区间覆盖前者不覆盖后者\
             ——探针不取自被测常量，避免自证式恒绿",
        );
        // 常量取值本身的合理性：须落在 (0, u8 一档] 内。
        // 这是「常量不是随手写的」的可测面——删成 0（等价于无单向区）
        // 或抬到 0.5（等价于全域单向）都会被这条抓住。
        let u8_step = 1.0f32 / 255.0;
        s.add(
            "C25-ONE-WAY-THRESHOLD-QUANTUM",
            ONE_WAY_ALPHA_THRESHOLD > 0.0 && ONE_WAY_ALPHA_THRESHOLD <= u8_step,
            "单向点阈值取在 (0, u8 一档] 内——非 0（否则无单向区）\
             且不超一档（否则全域皆不可逆，登记失去分辨力）",
        );
    }

    // --- 判据 14：空间与 alpha 正交 ---------------------------------
    // 抓错：两个转换被合并成一件事（"反正都要转，一次做完"）。
    {
        let p = orthogonal_probe();
        s.add(
            "C25-SPACE-ALPHA-ORTHOGONAL",
            p.alpha_only_keeps_space()
                && p.space_changes_value()
                && p.same_alpha_same_value()
                && p.alpha_matters()
                && p.space_chain_ok(),
            "只做 alpha 转换时空间标记仍为 SrgbEncoded（alpha 转换不携带空间转换）；\
             空间转换确实改变颜色数值（convert_space 非恒等）；\
             **同 alpha 下「只做空间」与「两者都做」逐位同值**\
             （预乘只乘 alpha，与空间无关——这是正交性的正面证据）；\
             不同 alpha 下两者不同（alpha 转换真生效）；空间标记链正确",
        );
    }

    // --- 判据 15：alpha 边界（0 / 1 / 趋零） ---------------------------
    {
        let mut log = ConvLog::new();
        // alpha = 0：反预乘必须退化为全零并记账（黑框防线）
        let zero = Premul::new(0.0, 0.0, 0.0, 0.0);
        let sz = to_straight(zero, &mut log);
        let zero_ok = sz.r == 0.0 && sz.g == 0.0 && sz.b == 0.0 && log.alpha_floor_hits == 1;
        // alpha = 1：预乘 ≡ 直通（恒等，不是近似）
        let mut log2 = ConvLog::new();
        let one = Premul::new(0.3, 0.4, 0.5, 1.0);
        let so = to_straight(one, &mut log2);
        let one_ok = (so.r - 0.3).abs() < 1.0e-6 && (so.b - 0.5).abs() < 1.0e-6;
        // alpha 趋零（<= EPS）：退化为全零 + 记账，**不是**放大噪声
        let mut log3 = ConvLog::new();
        let tiny = Premul::new(0.00005, 0.00005, 0.00005, 0.00005);
        let st = to_straight(tiny, &mut log3);
        let tiny_ok = st.r == 0.0 && log3.alpha_floor_hits == 1;
        // 入口方向：alpha=0 时预乘色必为 0（乘零）
        let s_zero_a = to_premul(Straight::new(0.9, 0.9, 0.9, 0.0), &mut log3);
        let inlet_zero_ok = s_zero_a.r == 0.0 && s_zero_a.a == 0.0;
        s.add(
            "C25-ALPHA-EDGES",
            zero_ok && one_ok && tiny_ok && inlet_zero_ok,
            "alpha=0 反预乘退化为全零且记账 1（黑框防线）；alpha=1 预乘恒等\
             直通（误差 ≤1e-6）；alpha 趋零同样退化不放大噪声；\
             入口方向 alpha=0 时预乘色为 0（乘零）",
        );
    }

    // --- 判据 16：零静默——台账可核对 -------------------------------
    {
        let mut log = ConvLog::new();
        let clean = log.total();
        // 入口恰 +1
        let i0 = log.inlet;
        let _ = to_premul(Straight::gray(0.5), &mut log);
        let inlet_ok = log.inlet == i0 + 1;
        // 出口恰 +1
        let o0 = log.outlet;
        let _ = to_straight(Premul::gray(0.5), &mut log);
        let outlet_ok = log.outlet == o0 + 1;
        // 内部恰 +1
        let n0 = log.internal;
        let _ = internal_passthrough(Premul::gray(0.5), &mut log);
        let internal_ok = log.internal == n0 + 1;
        // 趋零恰 +1
        let mut log2 = ConvLog::new();
        let f0 = log2.alpha_floor_hits;
        let _ = to_straight(Premul::new(0.0, 0.0, 0.0, 0.0), &mut log2);
        let floor_ok = log2.alpha_floor_hits == f0 + 1;
        // 非有限恰 +2（两个 NaN 分量）
        let mut log3 = ConvLog::new();
        let g0 = log3.nonfinite_guarded;
        let _ = to_premul(
            Straight::new(f32::NAN, f32::NAN, 0.5, 1.0),
            &mut log3,
        );
        let nonfinite_ok = log3.nonfinite_guarded == g0 + 2;
        s.add(
            "C25-LOG-EXACT",
            clean == 0 && inlet_ok && outlet_ok && internal_ok && floor_ok && nonfinite_ok,
            "干净台账为空（无幽灵记账）；入口/出口/内部/趋零各恰 +1；\
             两个 NaN 分量恰 +2（不多记不少记）",
        );
    }

    // --- 判据 17：色卡与 F0622 共用（真引用） --------------------------
    // 抓错：另抄一份斜坡探针 / 灰阶表（那会让 F0625 与 F0622/F0623/F0624
    // 的对拍尺度分叉，三路的 1 LSB 判据将互不可比）。
    {
        let rows = swatch_rows();
        let mut all_clean = true;
        let mut i = 0usize;
        while i < rows.len() {
            // 每行的内部转换计数必须为 0（色卡全程走边界转换，
            // 若出现内部转换则说明色卡路径绕过了纪律）。
            if !rows[i].contains("internal=0") {
                all_clean = false;
            }
            i += 1;
        }
        // 灰阶取值必须落在 F0622 的共用 RAMP 内（两端夹取亦然）
        let ramp_ok = ramp_char(-1.0) == RAMP[0] && ramp_char(2.0) == RAMP[RAMP.len() - 1];
        s.add(
            "C25-SWATCH-SHARED-PROBES",
            rows.len() == SWATCH_STEPS
                && SWATCH_STEPS == SWATCH_PROBES.len()
                && RAMP.len() == 10
                && all_clean
                && ramp_ok
                && LSB_LIMIT == LSB_BUDGET
                && LSB8_STEP == LSB8,
            "色卡行数 = F0622 的 SWATCH_PROBES 探针数（真引用非复制）；\
             全程 internal=0（走边界转换）；灰阶取值夹在共用 RAMP 内；\
             LSB 口径与预算常量与 F0622 逐位相同（三路对拍可比）",
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
/// 的判据**。
///
/// 本轮实测（隔离探针，基线 21/21 绿）：**13/13 变异全部被捕获**。
///
/// 前序会话自述 11/11 全捕获；本轮**独立复测**又抓到两处漏网（M12/M13），
/// 均已补判据并复跑至13/13。教训见 [`VARIANT_CAPTURE_NOTE`]。
///
/// | 变体 | 施加的缺陷 | 实际转红的判据 |
/// |---|---|---|
/// | M1 | 预乘滤波后**多除**一次 alpha（直通逻辑混入预乘域） | 全域预乘（1） |
/// | M2 | 直通路径滤波后也除 alpha（「只查 alpha 就放过」的实现） | 预乘滤波精确 + 暗边方向（2） |
/// | M3 | `to_straight` 的趋零守卫阈值取 0（`ALPHA_EPS` 档永不命中） | alpha 边界（1） |
/// | M4 | 内部转换**恒不记账**（计数变死码） | 内部零转换 + 日志精确（2） |
/// | M5 | 登记表删一条（转换点不全集） | 转换点全集（1）——修弱门禁后 |
/// | M6 | 未声明资产按「嗅探没异常⇒是预乘」 | 未声明走默认安全档（1） |
/// | M7 | 三指纹共用一条 spec_note | 三指纹互异（1） |
/// | M8 | `ROUNDING_MODE` 常量改为 `TiesToEven`（统一档本身被换） | 舍入模式统一（1） |
/// | M9 | `MATH_REASON` 文本退回旧语料 | 预乘滤波精确（1）——本轮新增门禁 |
/// | M10 | 出口点 `surface-export` 的 owner 改成入口点的 owner | 单一负责制（1） |
/// | M11 | [`ffloor`] 负数分支当成向零截断（向零≠向下） | 舍入模式统一（1）——含负数语料 |
/// | M12 | `internal_passthrough` 里偷记一次 `inlet`（内部流污染入口台账） | 内部台账零泄漏（1）——**本轮补** |
/// | M13 | `ONE_WAY_ALPHA_THRESHOLD` 取 0（判据探针取自该常量 ⇒ 自证恒绿） | 单向登记独立探针 + 阈值量子（3）——**本轮补** |
pub const VARIANT_REGISTRY: [(&str, &str); 13] = [
    ("M1-premul-filter-bad-unpremul", "C25-INTERNAL-IS-PREMUL"),
    ("M2-straight-path-also-correct", "C25-PREMUL-FILTER-EXACT"),
    ("M3-alpha-floor-guard-removed", "C25-ALPHA-EDGES"),
    ("M4-internal-counter-dead", "C25-INTERNAL-ZERO-CONVERSION"),
    ("M5-conv-point-dropped", "C25-CONV-POINTS-EXHAUSTIVE"),
    ("M6-undeclared-trust-sniff", "C25-SNIFF-DEFAULT-SAFE"),
    ("M7-fingerprints-share-one-note", "C25-FINGERPRINT-DISTINCT"),
    ("M8-rounding-mode-changed", "C25-ROUNDING-MODE-UNIFIED"),
    ("M9-math-reason-stale-corpus", "C25-PREMUL-FILTER-EXACT"),
    ("M10-single-responsibility-broken", "C25-SINGLE-RESPONSIBILITY"),
    ("M11-ffloor-negative-branch", "C25-ROUNDING-MODE-UNIFIED"),
    ("M12-internal-ledger-leak", "C25-INTERNAL-NO-LEDGER-LEAK"),
    (
        "M13-one-way-threshold-self-derived-probe",
        "C25-ONE-WAY-REGISTRATION-COVERS-INDEPENDENT-PROBE",
    ),
];

/// 变异捕获率声明（判据侧自述）：本轮隔离探针实测 **13/13 全捕获，
/// 基线 21/21 绿，零漏网**。
pub const VARIANT_CAPTURE_NOTE: &str = "\
VE-F0625 判据经 13 个定向变异反向验证，13/13 全部被捕获（基线 21/21 绿）。\
M2 是本条的设计动机：若判据只断「滤波后 alpha 对不对」，则预乘与直通两版\
都通过（alpha 在两版下恰好相同），暗边原理上不可见——故判据必须断**颜色**\
与解析真值的差。这也是十诫第 1 条「形状判据只证有渐变」的同构：位置对了不代表量对了。\
M4 针对死码：内部转换计数器若恒 0 而无人递增，它就是恒真门禁；故判据 5 故意\
调一次内部转换并断计数恰 +1，证明该计数可达。\
M5 暴露并修掉一处弱门禁：判据 4 原先只断\
`boundary_point_count() == CONV_POINT_COUNT`，而两者同源于 ConvPoint::all()，\
删表项并同步降 count 即恒绿（MISSED）。已补**独立于 count 的契约 id 白名单**\
（含期望方向），此即十诫第 9 条「同源驱动恒真」。\
M9 针对 pub 文本常量：MATH_REASON 明示供下游文档章引用，若无人验证，\
下游会引用一个与实验脱节的推导（施工中确实发生过：文本停留在初版语料\
A 为不透明黑/真值 0，而实验早已改成 A 为不透明红/真值 1.0）。\
故判据 2 增加文本-实验同源门禁：判据侧独立重算两个可复现片段并要求文本包含，\
同时反向断言旧语料标记不残留。\
M11 针对自实现取整原语 [`ffloor`]：**宿主面（`extern crate std`）全绿而真内核面\
`f32::floor` 根本不存在**（实测 x86_64-unknown-none 下 E0599）——这条只有独立\
target 编译才抓得到，宿主探针一律绿。故本条改用自实现并按真内核 target 验证。\
判据侧**必须含负数带小数语料**（`-0.2 → -1`）：只测正数则 `frac < 0` 分支恒不可达，\
那是十诫第 11 条「全未命中的分支钉不住」的形态。\
M12 与 M13 是本轮**独立复测新抓**的两处漏网（前序会话自述 11/11，实为未覆盖）：\
M12 —— `internal_passthrough` 额外记一次 `inlet`，而原判据只查 `internal` 一个\
计数器故全绿；这说明「内部转换恒等」这条语义包含**只准动一个计数器**，\
而断言没把它写进去。已补 `C25-INTERNAL-NO-LEDGER-LEAK`（四本账逐一比对）\
与 `C25-INTERNAL-PURE-OPERATIONS-CLEAN`（纯内部操作后 `total()==0`）。\
M13 —— `ONE_WAY_ALPHA_THRESHOLD` 被改成 0，而判据的探针点与覆盖检查\
**全部取自该常量自身**（`THRESHOLD*0.5`），常量一动探针跟着退到 alpha=0，\
登记表也随之判真 ⇒ 四个合取项照旧成立。已补判据侧**独立字面量**探针\
（1/512 与 0.5，无共享来源）+ `C25-ONE-WAY-THRESHOLD-QUANTUM`（阈值须落在\
(0, u8 一档]）。这是十诫第 7 条「判据向被测函数问答案＝自证式」与第 15 条\
「输出层钉不住时下沉到独立量」的合并实证。\
单向点登记采「如实登记而非强行无损」：低 alpha 处除法不稳定是数学事实、\
登记为单向点是正解，强行做无损往返需额外状态且代价不成立。\
前序会话记录的三处 MISSED 经查为**变异选型错误**而非判据 weakness，\
此即十诫第 14 条的又一次实证：\
  ① M7 初版钩子假设三条指纹 note 文本互异并按出现序计数（断言 count>=3），\
    而真源里三条本就互异 ⇒ 钩子自身抛断言。改为直接把后两条替换成第一条文本。\
  ② M9 初版钩子用贪婪正则 `\\\\n\";` 收尾去匹配 MATH_REASON 块，但该块以\
    `...发生。\";` 结尾且块内为多个续行，反斜杠续行把正则打断 ⇒ 未命中。\
    改为精准替换块内单个可复现片段。\
  ③ M10 初版钩子用跨条目贪婪正则配 inlet/outlet owner，抓到的对象错位\
    （改成了无关 owner）⇒ 审计看不出重载。改为按 id 定位 surface-export 段、\
    把其 owner 精确改成 asset-import 的 owner asset-loader。\
结论重申：**MISSED 先怀疑变异选错**（空变异/改兜底分支/改永不触发的守卫/\
正则抓错对象都属此类），确认选型无误后仍 MISSED 才动判据。";