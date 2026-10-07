//! VE-F1004 · PNG 色彩管理（ICC/sRGB/gAMA/cHRM，目标 380 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F1004`
//!
//! **判据（锚点原文逐条）**：
//! - iCCP 解析（zlib 解压 → 传 VE-V；**8MB 上限**防滥用）→ `C04-ICCP-*`
//! - sRGB 解析（渲染意图字段 0-3 校验）→ `C04-SRGB-*`
//! - gAMA 解析（gamma 值 + F0159 线性纪律联动：按 gAMA 转线性）→ `C04-GAMA-*`
//! - cHRM 解析（白点/三原色**七坐标**校验）→ `C04-CHRM-*`
//! - 冲突优先级（**iCCP > sRGB > gAMA+cHRM**，优先级表为唯一裁决来源，
//!   低优先级忽略并计数告警）→ `C04-CONF-*`
//! - 统一色彩标注对象（来源块 + 解析值 + 渲染意图），**单一出口**，
//!   下游只见标注不见原始块 → `C04-ANNO-*`
//! - 错误路径（iCCP 解压失败 → 降级 sRGB 假定并告警；gAMA 越界 → 拒绝该块
//!   按默认处理；全缺失 → 按 sRGB 假定）→ `C04-ERR-*`
//!
//! ---
//!
//! ## 设计要点一：为什么自带块扫描，而不复用 F1001 的容器解析
//!
//! F1001 的 [`dec::Container`] 只保留 IHDR / PLTE / tRNS / IDAT 四类**解码必需**
//! 的块，ancillary 块一律跳过（其职责是出像素，不是出色彩标注）。而本单要的
//! 四个块**全是 ancillary**，故容器结构里根本没有它们的位置——不是「不方便取」，
//! 是**取不到**。
//!
//! 若为此给 F1001 的 `Container` 加四个字段，就是把「解码器的产物结构」撑成
//! 「解码 + 色彩管理的联合产物」：日后 F1005（文本块）、F1006（APNG）再加
//! ancillary 字段，这个结构会无限膨胀，且让 F1001 为不属于自己的需求买单。
//!
//! 故本模块**自带一个只读块扫描器**，边界极窄：只找四个目标块、只校验它们的
//! CRC、其余块一概跳过（连长度都不解释）。**与 F1001 无耦合**——因此
//! F1001 日后改块遍历策略不会波及本模块，反之亦然。
//!
//! **代价（如实登记）**：块遍历逻辑有两份。这是「让 F1001 为别人的需求改结构」
//! 与「两份窄扫描器」之间的取舍——选后者，因为前者的耦合会随 ancillary 块
//! 种类增长而放大，且 F1001 是解码热路径，不该被色彩管理的需求侵入。
//!
//! ## 设计要点二：优先级表是唯一裁决来源（锚点硬要求）
//!
//! 锚点：「**优先级表为唯一裁决来源**，冲突时低优先级块忽略并计数告警」。
//!
//! [`SOURCE_RANK`] 是全模块唯一的裁决表，把四种来源映射到 0..=3 的秩；
//! [`ColorAnnotation::resolve`] **只按这张表**取胜者，不含任何
//! `if has_iccp && has_srgb` 之类的手写分支。理由与锚点一致：手写分支一多，
//! 「新增一种来源」就会漏改某一处分支——而漏改的那处不会报错，只会静默选错。
//!
//! **秩的语义**：`iCCP(3) > sRGB(2) > gAMA+cHRM(1) > 无(0)`。
//! gAMA 与 cHRM **同秩**——它们在规范里本就是一对（色度坐标须与 gamma 配套），
//! 单独抬高其中一个会让「只有 gAMA 没有 cHRM」的半截标注被误判为完整标注。
//! 故 `resolve` 对同秩来源**取并集**（gamma 与色度坐标分别来自各自胜者），
//! 而非二选一——这一点锚点未明说，但它是 gAMA+cHRM 作为一个整体参与竞争
//! 的唯一自洽读法。
//!
//! ## 设计要点三：冲突「忽略」而非「报错」（与错误面的分工）
//!
//! 锚点对冲突的处置是「低优先级块**忽略并计数告警**」，不是拒绝。理由：
//! 一份PNG 同时带 iCCP 与 gAMA 是**完全合法**的（规范允许），常见于
//! 「嵌入 ICC + 附带 gamma 提示」的导出器产物。判它非法会让大量合法文件
//! 打不开——那才是真错。
//!
//! 故 [`Conflict`] 只**记录**被忽略的来源，**不进错误面**；错误面只收
//! 「单个块自身不可用」（iCCP 解压失败、gAMA 越界、cHRM 坐标非法）。
//! **处置方向相反的两类状态不得共用码**：块坏了要降级/拒绝（阻断），
//! 块冲突只是记账（不阻断）。
//!
//! ## 设计要点四：gAMA 越界「拒绝该块」而非「钳到边界」
//!
//! 锚点：「gAMA 值越界（>5.0 或 <0.01）→ **拒绝该块**按默认处理」。
//!
//! 规范把 gAMA 存为 `gamma × 100000` 的 u32。越界值（如 0 或 10.0）在数学上
//! 让线性化退化为「全黑」或「恒等」，是**语义上无意义**的值。若钳到边界
//! （0.01 或 5.0），会产出一个「看起来合法、实则伪造」的标注——下游无从
//! 察觉自己拿到的是被钳过的值。故按锚点显式拒绝该块、走默认（sRGB 假定），
//! 并置 [`ColorAnnotation::gama_rejected`] 让下游知道「有这个块但我们没用」。
//!
//! ## 设计要点五：F0159 线性纪律的联动口径
//!
//! 锚点：「解码端按 gAMA 转线性供 HDR 管线」。本模块提供 [`srgb_to_linear`]
//! 与 [`gamma_to_linear`] 两个纯函数：前者是 sRGB 的**分段**传递函数
//! （线性段 + gamma 2.4 段，阈值 0.04045），后者是**纯幂函数** `x^γ`。
//! 两者都返回 `f32` 且**钳到 `[0,1]`**——HDR 管线拿到的必须是可用的归一化值。
//!
//! **不代做色调映射**：gamma 线性化只是把编码值还原到线性光空间，**不等于**
//! 适配目标显示器。本模块只做到规范要求的那一步；显示变换属VE-V 色彩管理域，
//! 本模块以 [`ColorAnnotation`] 把「已解析的gamma + 色彩空间」交出去即止。
//!
//! ## 跨批对接点
//!
//! 上游：F1001（签名常量与容器口径，本模块只借 [`dec::PNG_SIG`]）。
//! 下游：**VE-V 色彩管理域**（消费 ICC 载荷与统一标注）、**VE-D**
//! （合成侧假定）、**F0159**（线性纪律）、F1005（文本块族，共用 ancillary
//! 扫描的形状——但各扫各的，不合并，避免又造一个联合结构）。

use crate::perfstar::frameledger_ext;
use crate::perfstar::mech_inflate;
use crate::svstar2::vef01_pngdec as dec;

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、来源优先级表（锚点「优先级表为唯一裁决来源」· 全模块唯一裁决依据）
// ---------------------------------------------------------------------------

/// 色彩来源种类（四个目标块 + 「无块」这一合法默认）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ColorSource {
    /// 显式声明的 ICC 配置文件（`iCCP` 块）。
    Iccp,
    /// sRGB 标注（`sRGB` 块）。
    Srgb,
    /// gamma + 色度坐标配套标注（`gAMA` + `cHRM`）。
    GammaChrm,
}

impl ColorSource {
    /// 优先级秩（越大越优先；`0` 保留给「无来源」）。
    ///
    /// **全模块唯一裁决表**——[`ColorAnnotation::resolve`] 只读它。
    /// gAMA 与 cHRM 同秩的理由见头注设计要点二。
    #[inline]
    pub fn rank(self) -> u8 {
        match self {
            ColorSource::GammaChrm => 1,
            ColorSource::Srgb => 2,
            ColorSource::Iccp => 3,
        }
    }
    /// 短名（诊断用，不含裸码）。
    pub fn label(self) -> &'static str {
        match self {
            ColorSource::Iccp => "iCCP",
            ColorSource::Srgb => "sRGB",
            ColorSource::GammaChrm => "gAMA+cHRM",
        }
    }
}

/// 优先级表：按秩降序的来源序列（供逐级裁决与文档化对照）。
///
/// 数组顺序即优先级顺序，**与 [`ColorSource::rank`] 互为校验**——
/// 自检断言两者一致（`C04-CONF-01`），故改坏任一侧都会变红。
pub const SOURCE_RANK: [ColorSource; 3] =
    [ColorSource::Iccp, ColorSource::Srgb, ColorSource::GammaChrm];

// ---------------------------------------------------------------------------
// 二、色彩标注对象（锚点「单一出口供下游消费」）
// ---------------------------------------------------------------------------

/// 渲染意图（`sRGB` 块的渲染意图字段；iCCP 路径下由 ICC 自身承载）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RenderIntent {
    /// 0：感知（perceptual）。
    Perceptual,
    /// 1：相对比色（relative colorimetric）。
    RelativeColorimetric,
    /// 2：饱和度（saturation）。
    Saturation,
    /// 3：绝对比色（absolute colorimetric）。
    AbsoluteColorimetric,
}

impl RenderIntent {
    /// 由 `sRGB` 块的单字节字段解码。
    ///
    /// **锚点要求「渲染意图字段 0-3 校验」**：规范只定义 0..=3，
    /// 其余值属未定义 → 显性拒绝（[`ColorFaultKind::BadIntent`]），
    /// **不夹取到 3**（夹取会让「声明了未定义意图」的文件被当成
    /// 「声明了绝对比色」，语义静默改变）。
    pub fn from_wire(v: u8) -> Option<RenderIntent> {
        match v {
            0 => Some(RenderIntent::Perceptual),
            1 => Some(RenderIntent::RelativeColorimetric),
            2 => Some(RenderIntent::Saturation),
            3 => Some(RenderIntent::AbsoluteColorimetric),
            _ => None,
        }
    }
    /// 线值（0..=3）。
    pub fn wire(self) -> u8 {
        match self {
            RenderIntent::Perceptual => 0,
            RenderIntent::RelativeColorimetric => 1,
            RenderIntent::Saturation => 2,
            RenderIntent::AbsoluteColorimetric => 3,
        }
    }
    /// 自洽断言用：全部 4 个值往返一致。
    pub fn all() -> [RenderIntent; 4] {
        [
            RenderIntent::Perceptual,
            RenderIntent::RelativeColorimetric,
            RenderIntent::Saturation,
            RenderIntent::AbsoluteColorimetric,
        ]
    }
}

/// 被忽略的低优先级来源（锚点「低优先级块忽略并计数告警」）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Conflict {
    /// 被忽略的来源。
    pub ignored: ColorSource,
    /// 胜出来源（秩更高者）。
    pub winner: ColorSource,
}

/// **统一色彩标注——下游（VE-V / VE-D）的唯一入口。**
///
/// 锚点：「单一出口供下游（VE-V/VE-D）消费——**下游只见统一色彩标注不见原始块**」。
/// 故本结构**不携带**任何原始块字节（ICC 载荷除外，见下），下游无法绕过
/// 优先级裁决去直接读某个块——这是「优先级表为唯一裁决来源」得以成立的
/// **结构前提**：若原始块仍可直达，裁决就只是建议而非规则。
///
/// ICC 载荷是唯一例外，且是**规范要求的例外**：解码后的 ICC 配置要原样
/// 传给 VE-V 色彩管理域做实际变换，光给「有一个 ICC」这个事实没有用。
#[derive(Clone, PartialEq, Debug, Default)]
pub struct ColorAnnotation {
    /// 胜出来源（`None` = 全部缺失，按 sRGB 假定，见头注设计要点三）。
    pub source: Option<ColorSource>,
    /// 渲染意图（仅 `sRGB` 路径有；其余为 `None`）。
    pub intent: Option<RenderIntent>,
    /// 声明的 gamma（`gAMA + cHRM` 路径）。
    pub gamma: Option<f32>,
    /// 白点色度坐标（`cHRM` 路径；顺序 x,y 交替，共 8 个值中的白点两项）。
    pub white_point: Option<(f32, f32)>,
    /// 解压后的 ICC 配置（`iCCP` 路径；已过8MB 上限校验）。
    pub icc: Option<Vec<u8>>,
    /// 因越界被拒绝的 gAMA 块（值 = 原始 u32 线值）。
    ///
    /// **单独记账而非静默丢弃**：下游需要知道「文件里有过一个 gAMA 但我们
    /// 没采纳」，否则无法解释「为何按 sRGB 假定」。
    pub gama_rejected: Option<u32>,
    /// 因非法被拒的 cHRM 块（值 = 原始 32 字节载荷）。
    pub chrm_rejected: Option<[u8; 32]>,
    /// 冲突记账（被忽略的来源）。
    pub conflicts: Vec<Conflict>,
    /// 块级告警计数（CRC 错、载荷截断等；不阻断）。
    pub warnings: u32,
}

/// ICC 配置大小上限（锚点明文：8MB 防滥用）。
pub const ICCP_MAX_BYTES: usize = 8 * 1024 * 1024;

/// iCCP 解压的**初始**试探容量（渐进扩容起点）。
///
/// 取 64KB：典型 ICC 配置 100KB-3MB，首轮扩容一次到位；而小配置
/// （sRGB 替代用的微型 profile 常见仅数 KB）不必按 8MB 预留瞬时内存。
/// 后续遇`Overflow` 按 2 倍递增，封顶 [`ICCP_MAX_BYTES`] + 1。
const PROBE_MIN_BYTES: usize = 64 * 1024;

impl ColorAnnotation {
    /// 是否按sRGB 假定（无任何可用色彩块）。
    #[inline]
    pub fn is_default_srgb(&self) -> bool {
        self.source.is_none()
    }
    /// 供 VE-V 消费的统一出口：返回「胜出来源 + 是否为 ICC 路径」。
    pub fn handoff(&self) -> (Option<ColorSource>, bool) {
        (self.source, self.icc.is_some())
    }
}

// ---------------------------------------------------------------------------
// 三、错误面（只收「块自身不可用」；冲突不入错误面，见头注要点三）
// ---------------------------------------------------------------------------

/// 块自身故障类别。
///
/// **只收单块不可用**——签名错、IHDR 非法等由 [`dec::PngFault`] 承载，
/// 此处不重复登记（两个错误面各说一半会让调用方不知该 match 哪边）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ColorFaultKind {
    /// PNG 签名不符。
    BadSignature,
    /// 块结构越界（长度字段指向文件外）。
    ChunkTruncated,
    /// 色彩块 CRC 校验失败。
    CrcMismatch,
    /// `sRGB` 块长度非 1。
    SrgbLength,
    /// `sRGB` 渲染意图字段不在 0..=3。
    BadIntent,
    /// `gAMA` 块长度非 4。
    GamaLength,
    /// `gAMA` 值越界（>5.0 或 <0.01）。
    GamaOutOfRange,
    /// `cHRM` 块长度非 32。
    ChrmLength,
    /// `cHRM` 坐标非法（超出 [0,1] 或非有限）。
    ChrmCoordinate,
    /// `iCCP` 块长度 < 5（keyword + NUL + 压缩方法至少 5 字节）。
    IccpTooShort,
    /// `iCCP` 关键字非法（含内嵌 NUL 之外的坏字符，或超长）。
    IccpKeyword,
    /// `iCCP` 压缩方法非 0（规范只定义 0 = zlib deflate）。
    IccpCompression,
    /// `iCCP` 解压失败。
    IccpInflate,
    /// `iCCP` 解压后超过 8MB 上限。
    IccpTooLarge,
    /// 输出缓冲不足。
    BufferShort,
}

impl ColorFaultKind {
    /// 错误码（色彩段独立码段 `0xF400 | n+1`；基数低 4 位为 0 避免重码）。
    pub fn code(self) -> u16 {
        0xF400 | (self as u16) + 1
    }
    /// 短名。
    pub fn label(self) -> &'static str {
        match self {
            ColorFaultKind::BadSignature => "PNG 签名不符",
            ColorFaultKind::ChunkTruncated => "块结构越界",
            ColorFaultKind::CrcMismatch => "色彩块 CRC 错",
            ColorFaultKind::SrgbLength => "sRGB 块长度非法",
            ColorFaultKind::BadIntent => "sRGB 渲染意图越界",
            ColorFaultKind::GamaLength => "gAMA 块长度非法",
            ColorFaultKind::GamaOutOfRange => "gAMA 值越界",
            ColorFaultKind::ChrmLength => "cHRM 块长度非法",
            ColorFaultKind::ChrmCoordinate => "cHRM 坐标非法",
            ColorFaultKind::IccpTooShort => "iCCP 块过短",
            ColorFaultKind::IccpKeyword => "iCCP 关键字非法",
            ColorFaultKind::IccpCompression => "iCCP 压缩方法不支持",
            ColorFaultKind::IccpInflate => "iCCP 解压失败",
            ColorFaultKind::IccpTooLarge => "iCCP 超过 8MB 上限",
            ColorFaultKind::BufferShort => "输出缓冲不足",
        }
    }
    /// 原因。
    pub fn cause(self) -> &'static str {
        match self {
            ColorFaultKind::BadSignature => "文件头 8 字节不是 PNG 签名",
            ColorFaultKind::ChunkTruncated => "块长度字段指向的位置超出文件末尾",
            ColorFaultKind::CrcMismatch => "块 CRC32 与实际载荷不符",
            ColorFaultKind::SrgbLength => "sRGB 块载荷长度不等于 1",
            ColorFaultKind::BadIntent => "渲染意图字段不在 0..=3（规范未定义该值）",
            ColorFaultKind::GamaLength => "gAMA 块载荷长度不等于 4",
            ColorFaultKind::GamaOutOfRange => "gamma 越出 (0.01, 5.0) 区间",
            ColorFaultKind::ChrmLength => "cHRM 块载荷长度不等于 32",
            ColorFaultKind::ChrmCoordinate => "色度坐标越出 [0,1] 或非有限值",
            ColorFaultKind::IccpTooShort => "iCCP 载荷短于 keyword+NUL+压缩方法的最小 5 字节",
            ColorFaultKind::IccpKeyword => "profile 名含非法字符或长度超限",
            ColorFaultKind::IccpCompression => "压缩方法字节非 0（只定义 zlib deflate）",
            ColorFaultKind::IccpInflate => "zlib 流展开失败或校验不符",
            ColorFaultKind::IccpTooLarge => "解压后 ICC 配置超过 8MB 上限",
            ColorFaultKind::BufferShort => "调用方缓冲小于所需",
        }
    }
    /// 建议（三要素之三）。
    pub fn advice(self) -> &'static str {
        match self {
            ColorFaultKind::IccpInflate => "该 iCCP 块已按规范降级为 sRGB 假定；请检查嵌入工具的 zlib 输出",
            ColorFaultKind::IccpTooLarge => "移除或裁剪嵌入的 ICC 配置；8MB 是防滥用硬上限",
            ColorFaultKind::GamaOutOfRange => "该 gAMA 块已忽略，改按 sRGB 假定处理；请核对 gamma 编码",
            ColorFaultKind::BadIntent => "渲染意图只允许 0..=3；请修正导出工具的写入值",
            ColorFaultKind::CrcMismatch => "重新导出该 PNG——色彩块损坏，不影响像素解码",
            _ => "按 PNG 规范核对色彩辅助块格式后重试",
        }
    }
}

/// 色彩块故障（类别 + 两个数值细节）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ColorFault {
    /// 类别。
    pub kind: ColorFaultKind,
    /// 细节一（语义按类别解释）。
    pub a: u64,
    /// 细节二。
    pub b: u64,
}

impl ColorFault {
    /// 构造故障。
    pub fn new(kind: ColorFaultKind) -> ColorFault {
        ColorFault { kind, a: 0, b: 0 }
    }
    /// 附加两个数值细节。
    pub fn with(kind: ColorFaultKind, a: u64, b: u64) -> ColorFault {
        ColorFault { kind, a, b }
    }
    /// 错误码。
    pub fn code(&self) -> u16 {
        self.kind.code()
    }
    /// 原因。
    pub fn cause(&self) -> &'static str {
        self.kind.cause()
    }
    /// 建议。
    pub fn advice(&self) -> &'static str {
        self.kind.advice()
    }
    /// 人话（模板 + 三元细节）。
    pub fn human(&self) -> String {
        let mut s = String::new();
        s.push_str(self.kind.label());
        match self.kind {
            ColorFaultKind::GamaOutOfRange => {
                s.push_str("：gamma 线值 ");
                s.push_str(&fmt_u64(self.a));
                s.push_str("（越出允许区间）");
            }
            ColorFaultKind::CrcMismatch | ColorFaultKind::SrgbLength | ColorFaultKind::GamaLength
            | ColorFaultKind::ChrmLength => {
                s.push_str("：实得 ");
                s.push_str(&fmt_u64(self.a));
                s.push_str(" 字节");
            }
            ColorFaultKind::BadIntent => {
                s.push_str("：意图字段 ");
                s.push_str(&fmt_u64(self.a));
                s.push_str("（只允许 0..=3）");
            }
            ColorFaultKind::IccpTooLarge => {
                s.push_str("：解压后 ");
                s.push_str(&fmt_u64(self.a));
                s.push_str(" 字节，上限 ");
                s.push_str(&fmt_u64(self.b));
            }
            _ => {}
        }
        s
    }
}

/// 无 `format!` 的十进制渲染（内核no_std 稳妥写法）。
fn fmt_u64(mut v: u64) -> String {
    if v == 0 {
        return String::from("0");
    }
    let mut buf = [0u8; 20];
    let mut n = 0;
    while v > 0 {
        buf[n] = b'0' + (v % 10) as u8;
        v /= 10;
        n += 1;
    }
    let mut s = String::new();
    for i in (0..n).rev() {
        s.push(buf[i] as char);
    }
    s
}

// ---------------------------------------------------------------------------
// 四、块扫描器（只找四个目标块，见头注设计要点一）
// ---------------------------------------------------------------------------

/// 四个目标块的类型常量。
pub mod chunk {
    /// ICC 配置块。
    pub const ICCP: [u8; 4] = *b"iCCP";
    /// sRGB 标注块。
    pub const SRGB: [u8; 4] = *b"sRGB";
    /// gamma 块。
    pub const GAMA: [u8; 4] = *b"gAMA";
    /// 色度坐标块。
    pub const CHRM: [u8; 4] = *b"cHRM";
    /// 块结束。
    pub const IEND: [u8; 4] = *b"IEND";
}

/// 扫到的原始色彩块（未解析）。
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct RawColorChunks {
    /// iCCP 载荷。
    pub iccp: Option<Vec<u8>>,
    /// sRGB 载荷。
    pub srgb: Option<Vec<u8>>,
    /// gAMA 载荷。
    pub gama: Option<Vec<u8>>,
    /// cHRM 载荷。
    pub chrm: Option<Vec<u8>>,
    /// CRC 校验失败的块数（告警计数；锚点「ancillary CRC 错→ 告警」）。
    pub crc_warnings: u32,
}

/// 扫描 PNG 的四个色彩块（**只读、不解释其他块**）。
///
/// CRC 失败的块按规范**告警并跳过**（ancillary 分级），不阻断——
/// 一个坏 ICC 不该让整张图的像素解不出来。
pub fn scan_color_chunks(file: &[u8]) -> Result<RawColorChunks, ColorFault> {
    if file.len() < 8 || file[..8] != dec::PNG_SIG {
        return Err(ColorFault::new(ColorFaultKind::BadSignature));
    }
    let mut out = RawColorChunks::default();
    let mut pos = 8usize;
    while pos + 8 <= file.len() {
        let len = u32::from_be_bytes([file[pos], file[pos + 1], file[pos + 2], file[pos + 3]]) as usize;
        let ty = [file[pos + 4], file[pos + 5], file[pos + 6], file[pos + 7]];
        // 块总长 = 4(长度) + 4(类型) + len + 4(CRC)，整体须落在文件内
        if len > file.len() || pos + 12 + len > file.len() {
            return Err(ColorFault::with(ColorFaultKind::ChunkTruncated, len as u64, file.len() as u64));
        }
        let data = &file[pos + 8..pos + 8 + len];
        let crc_stored = u32::from_be_bytes([
            file[pos + 8 + len],
            file[pos + 8 + len + 1],
            file[pos + 8 + len + 2],
            file[pos + 8 + len + 3],
        ]);
        // CRC 覆盖「类型 + 载荷」两段（规范 §5.1）
        let crc_calc = frameledger_ext::crc32(&file[pos + 4..pos + 8 + len]);
        if crc_stored != crc_calc {
            out.crc_warnings += 1;
        } else {
            // 首次出现为准（规范允许重复块；本模块取首个，不因重复改行为）
            let fresh = match ty {
                t if t == chunk::ICCP => out.iccp.is_none(),
                t if t == chunk::SRGB => out.srgb.is_none(),
                t if t == chunk::GAMA => out.gama.is_none(),
                t if t == chunk::CHRM => out.chrm.is_none(),
                _ => false,
            };
            if fresh {
                if ty == chunk::ICCP {
                    out.iccp = Some(data.to_vec());
                } else if ty == chunk::SRGB {
                    out.srgb = Some(data.to_vec());
                } else if ty == chunk::GAMA {
                    out.gama = Some(data.to_vec());
                } else if ty == chunk::CHRM {
                    out.chrm = Some(data.to_vec());
                }
            }
        }
        pos += 12 + len;
        if ty == chunk::IEND {
            break;
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// 五、四块解析 + 优先级裁决
// ---------------------------------------------------------------------------

/// `gAMA` 允许区间（锚点明文：>5.0 或 <0.01 即越界）。
pub const GAMA_MIN: f32 = 0.01;
/// `gAMA` 上界。
pub const GAMA_MAX: f32 = 5.0;

/// 解析 `gAMA` 载荷（4 字节大端 u32，值 = gamma × 100000）。
pub fn parse_gama(data: &[u8]) -> Result<f32, ColorFault> {
    if data.len() != 4 {
        return Err(ColorFault::with(ColorFaultKind::GamaLength, data.len() as u64, 4));
    }
    // 大端取高字节在前——`u32::from_be_bytes` 一次成型，不手工移位（避免
    // 「`as u8` 取低位」那类静默取错一半的错误）
    let raw = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
    if raw == 0 {
        return Err(ColorFault::with(ColorFaultKind::GamaOutOfRange, raw as u64, 0));
    }
    let g = raw as f32 / 100000.0;
    // 区间取**闭区间** `[0.01, 5.0]`：锚点原文是「>5.0 或 <0.01 → 拒绝」，
    // 即越界的判据是**严格大于/小于**，边界值 0.01 与 5.0 本身合法。
    //
    // （旧实现写的是开区间 `g > MIN && g < MAX`，把两个合法边界值一并拒了
    //  ——与锚点原文相悖，且与紧邻其上的注释「不算越界」自相矛盾：注释按
    // 闭区间解释，代码按开区间执行。这类「注释与代码反向」最危险，因为读
    // 注释的人会以为边界已被正确处理。判据 `C04-GAMA-02` 的夹逼对
    // 500_000/500_001 与 999/1000 就是为钉死这一点而设。）
    if g < GAMA_MIN || g > GAMA_MAX {
        return Err(ColorFault::with(ColorFaultKind::GamaOutOfRange, raw as u64, 0));
    }
    Ok(g)
}

/// 解析 `cHRM` 载荷（32 字节 = 8 个大端 u32：白点 xy + 三原色 xy）。
///
/// 锚点「白点/三原色**七坐标**校验」：规范给 8 个数（4 组 xy），
/// 其中三原色 6 个 + 白点 2 个 = **八坐标**；锚点写「七」是笔误
/// （或指「三原色 6 + 白点 x」）。本模块按**规范实际的 8 个 u32** 解析并
/// **全部 8 个**都校验——多校验一个坐标不会拒绝任何合法文件，
/// 而少校验一个会让坏坐标静默通过。
pub fn parse_chrm(data: &[u8]) -> Result<[f32; 8], ColorFault> {
    // cHRM 载荷恒为 32 字节（8 个大端 u32：白点 xy + 三原色各 xy）。
    if data.len() != 32 {
        return Err(ColorFault::with(ColorFaultKind::ChrmLength, data.len() as u64, 32));
    }
    let mut out = [0f32; 8];
    for (i, slot) in out.iter_mut().enumerate() {
        let o = i * 4;
        let raw = u32::from_be_bytes([data[o], data[o + 1], data[o + 2], data[o + 3]]);
        let v = raw as f32 / 100000.0;
        // 校验：落在 [0,1] 且有限。色度坐标越界在物理上无意义。
        //
        // 故障五元组只带坐标序号与原始 u32 值——**不携带原始字节**。
        // （曾在此处拷贝 32 字节到临时 buf 却从不读取：纯死代码，且让人
        // 误以为错误里带了数据。诊断要的是「第几个坐标、原始值多少」，
        // 载荷字节对定位无用。）
        if !(v >= 0.0 && v <= 1.0) || !is_finite(v) {
            return Err(ColorFault { kind: ColorFaultKind::ChrmCoordinate, a: raw as u64, b: i as u64 });
        }
        *slot = v;
    }
    Ok(out)
}

/// 有限性检查（`f32::is_finite` 在 no_std 恒等可用，此处显式封装便于阅读）。
#[inline]
fn is_finite(v: f32) -> bool {
    v == v && v != f32::INFINITY && v != f32::NEG_INFINITY
}

/// iCCP profile 名长度上限（规范：1-79 字节）。
pub const ICCP_KEYWORD_MAX: usize = 79;

/// 解析 `iCCP` 载荷并解压出 ICC 配置。
///
/// 布局：`keyword(1-79) + NUL(1) + 压缩方法(1) + 压缩配置`。
pub fn parse_iccp(data: &[u8], out: &mut Vec<u8>) -> Result<(), ColorFault> {
    if data.len() < 5 {
        return Err(ColorFault::with(ColorFaultKind::IccpTooShort, data.len() as u64, 5));
    }
    // keyword：以 NUL 结尾，长度须落在 1..=79
    let nul = match data.iter().position(|&b| b == 0) {
        Some(i) => i,
        None => return Err(ColorFault::new(ColorFaultKind::IccpKeyword)),
    };
    if nul == 0 || nul > ICCP_KEYWORD_MAX {
        return Err(ColorFault::with(ColorFaultKind::IccpKeyword, nul as u64, ICCP_KEYWORD_MAX as u64));
    }
    // profile 名只允许 Latin-1 可见字符（规范 §4.14：不得含控制字符）
    for &b in &data[..nul] {
        if b < 0x20 || b > 0x7E {
            return Err(ColorFault::with(ColorFaultKind::IccpKeyword, b as u64, 0x20));
        }
    }
    let method = data[nul + 1];
    if method != 0 {
        return Err(ColorFault::with(ColorFaultKind::IccpCompression, method as u64, 0));
    }
    let zdata = &data[nul + 2..];
    out.clear();
    // 解压**渐进扩容**：ICC 常见 100KB-3MB，而闸门允许到 8MB。若每次都
    // 按 8MB 起步，一个 4KB 的小 profile 也要占 8MB 瞬时内存（内核侧不可接受）；
    // 故从 64KB 起步，遇 `Overflow` 逐级加倍，直到闸门上限为止。
    //
    // 容量上限取 **`ICCP_MAX_BYTES + 1`** 而非 `ICCP_MAX_BYTES`：多出的
    // 那 1 字节是「恰好超限」与「恰好等于上限」的唯一可分辨标志——
    // 若缓冲恰好只有 `ICCP_MAX_BYTES`，则 8MB 与 8MB+1 两种配置**都会**
    // 让 inflate 在写满后报 `Overflow`，二者被混成同一种「解压失败」，
    // 上限闸就退化成了「只会拒不会放」的弱门禁（对照判据 C04-ICCP-03
    // 的反向腿：恰好等于上限者必须被接受）。
    //
    // **压缩输入长度不设前置闸**：压缩体大小与解压后大小无固定关系，
    // 拿 `zdata.len()` 去比 8MB 会误杀合法配置——stored 块封装的
    // 8MB profile 其压缩体必然略大于 8MB（每 65535 字节附加 5 字节块头），
    // 正是判据 C04-ICCP-03 反向腿的语料。防炸弹的真闸门是**输出上限**
    // （解压产出超 8MB 即拒），它对任意压缩比都成立。
    let mut cap = PROBE_MIN_BYTES.min(ICCP_MAX_BYTES + 1);
    // `Overflow` 在封顶后**只能**意味着「解压产出超过上限」，不可能是别的：
    // 封顶缓冲是 `ICCP_MAX_BYTES + 1`，能写满并溢出即 n > 上限。故此处
    // 必须回 `IccpTooLarge` 而非笼统的 `IccpInflate`——
    //
    // **这不是措辞讲究，而是判据能不能立住的问题**：两者在调用方都被
    // 「降级 + 告警」消化，表面行为一致，于是「上限闸被摘掉」这种改动
    // 在外部**完全不可观测**——故上限必须有**只在超限时才走到**的独立出口，
    // 否则它就不是闸门，只是一条恰好与闸门同生共死的注脚。
    //
    // 【实测结论，2026-10-07 更新】此前注释写「删掉下面的
    // `n > ICCP_MAX_BYTES` 判定后 21 项判据仍全绿」。本次反假变体测试
    // （14 个变体，见 `_attic/2026-10-07-W010-VE-F1004`）**否证**了该自陈：
    // 注入 `if n > ICCP_MAX_BYTES` -> `if false` 后判据 `C04-ICCP-03` **转红**，
    // 说明该判据已补上「独立可观测的反向腿」（恰好等于上限者必须被接受，
    // 超限者必须被拒——两条腿互不依赖Overflow 分支）。原 pessimism 描述
    // 已过时，保留此注以免后人重复推导。
    let n = loop {
        out.resize(cap, 0);
        match mech_inflate::zlib_inflate_slices(&[zdata], out) {
            Ok(n) => break n,
            Err(mech_inflate::PngError::Inflate(mech_inflate::InflateErr::Overflow))
                if cap < ICCP_MAX_BYTES + 1 =>
            {
                cap = (cap * 2).min(ICCP_MAX_BYTES + 1);
            }
            // 已封顶仍溢出 → 产出必然超上限，走**专属**出口。
            Err(mech_inflate::PngError::Inflate(mech_inflate::InflateErr::Overflow)) => {
                out.clear();
                return Err(ColorFault::with(
                    ColorFaultKind::IccpTooLarge,
                    (ICCP_MAX_BYTES + 1) as u64,
                    ICCP_MAX_BYTES as u64,
                ));
            }
            Err(_) => {
                out.clear();
                return Err(ColorFault::new(ColorFaultKind::IccpInflate));
            }
        }
    };
    // 解压成功：先判上限，再判空。顺序不可颠倒——超限与空是两种不同降级理由。
    if n > ICCP_MAX_BYTES {
        out.clear();
        return Err(ColorFault::with(
            ColorFaultKind::IccpTooLarge,
            n as u64,
            ICCP_MAX_BYTES as u64,
        ));
    }
    // **解压成功但内容为空 ≠ 有 ICC 配置**（判据 C04-ICCP-04）：
    // 空 profile 进标注后，VE-V 只会拿到空引用。空一律按「块不可用」
    // 返回，由调用方降级 sRGB 假定并告警。
    //
    // 下限取**1字节**而非 ICC 规范的 12 字节头部：判据 `C04-ICCP-01` 要求
    // 1 字节 profile 可原样往返（把它当「最小可用」），而 0 字节无任何内容、
    // 必然是坏数据。两处口径必须一致，故此处只拒 0，不在这里替 VE-V 做
    // ICC 结构合法性校验（那是色彩管理域的职责，不是解析器的）。
    if n == 0 {
        out.clear();
        return Err(ColorFault::new(ColorFaultKind::IccpInflate));
    }
    out.truncate(n);
    Ok(())
}

/// 裁决：按 [`SOURCE_RANK`] 取胜者，低优先级来源记入冲突。
///
/// **只读 [`SOURCE_RANK`]**（头注设计要点二）——本函数内无任何
/// 「有 iCCP 就怎样、有 sRGB 就怎样」的手写分支。
pub fn resolve(
    iccp_ok: Option<Vec<u8>>,
    srgb: Option<RenderIntent>,
    gamma: Option<f32>,
    chrm: Option<[f32; 8]>,
) -> ColorAnnotation {
    // 收集「本可用」的来源。gAMA 与 cHRM 合并为一个来源（规范里二者配套）。
    //
    // 秩**一律取自** [`ColorSource::rank`]（即那张唯一裁决表），此处不写
    // 任何字面量：写了字面量，「唯一裁决来源」就成了文档谎言——改表不改
    // 这里的字面量（或反之）时裁决仍按旧值跑，且不报任何错。
    let mut avail: Vec<(ColorSource, u8)> = Vec::new();
    if iccp_ok.is_some() {
        avail.push((ColorSource::Iccp, ColorSource::Iccp.rank()));
    }
    if srgb.is_some() {
        avail.push((ColorSource::Srgb, ColorSource::Srgb.rank()));
    }
    if gamma.is_some() || chrm.is_some() {
        avail.push((ColorSource::GammaChrm, ColorSource::GammaChrm.rank()));
    }
    let mut ann = ColorAnnotation {
        icc: iccp_ok,
        intent: srgb,
        gamma,
        white_point: chrm.map(|c| (c[0], c[1])),
        ..ColorAnnotation::default()
    };
    if avail.is_empty() {
        // 全缺失 → 按 sRGB 假定（锚点明文；默认策略文档化在此）
        ann.source = None;
        return ann;
    }
    // 按 rank 降序取最高者为胜者；同秩（gAMA+cHRM 内部）取并集
    let mut best_rank = 0u8;
    for (_, r) in avail.iter() {
        if *r > best_rank {
            best_rank = *r;
        }
    }
    let winner_kind = avail.iter().find(|(_, r)| *r == best_rank).map(|(k, _)| *k);
    ann.source = winner_kind;
    // 败者记账（锚点「低优先级块忽略并计数告警」）
    for (kind, r) in avail.iter() {
        if *r < best_rank {
            if let Some(w) = winner_kind {
                ann.conflicts.push(Conflict { ignored: *kind, winner: w });
            }
        }
    }
    ann
}

/// 解析全文件并产出统一标注（**下游唯一入口**）。
///
/// 错误处置（锚点逐条）：
/// - `iCCP` **解压失败** → 降级 sRGB 假定并告警（`warnings += 1`），
///   **不返回 Err**——一个坏 ICC 不该让整图不可用；
/// - `gAMA` **越界** → 拒绝该块（记入 `gama_rejected`）并继续；
/// - `cHRM` **坐标非法** → 拒绝该块（记入 `chrm_rejected`）并继续；
/// - **全缺失** → 按 sRGB 假定（`source = None`）。
///
/// 仅**结构级**故障（签名错、块越界）返回 `Err`。
pub fn parse_color_annotation(file: &[u8]) -> Result<ColorAnnotation, ColorFault> {
    let raw = scan_color_chunks(file)?;
    let mut ann = ColorAnnotation { warnings: raw.crc_warnings, ..ColorAnnotation::default() };

    // --- sRGB ---
    let mut srgb_intent = None;
    if let Some(d) = raw.srgb.as_ref() {
        if d.len() != 1 {
            return Err(ColorFault::with(ColorFaultKind::SrgbLength, d.len() as u64, 1));
        }
        match RenderIntent::from_wire(d[0]) {
            Some(it) => srgb_intent = Some(it),
            None => return Err(ColorFault::with(ColorFaultKind::BadIntent, d[0] as u64, 3)),
        }
    }

    // --- gAMA（越界只拒该块，不阻断） ---
    let mut gamma = None;
    if let Some(d) = raw.gama.as_ref() {
        match parse_gama(d) {
            Ok(g) => gamma = Some(g),
            Err(e) => {
                if e.kind == ColorFaultKind::GamaLength {
                    return Err(e); // 长度错= 结构错，阻断
                }
                let rawv = u32::from_be_bytes([d[0], d[1], d[2], d[3]]);
                ann.gama_rejected = Some(rawv);
                ann.warnings += 1;
            }
        }
    }

    // --- cHRM（坐标非法只拒该块） ---
    let mut chrm = None;
    if let Some(d) = raw.chrm.as_ref() {
        match parse_chrm(d) {
            Ok(c) => chrm = Some(c),
            Err(e) => {
                if e.kind == ColorFaultKind::ChrmLength {
                    return Err(e);
                }
                let mut buf = [0u8; 32];
                let n = d.len().min(32);
                buf[..n].copy_from_slice(&d[..n]);
                ann.chrm_rejected = Some(buf);
                ann.warnings += 1;
            }
        }
    }

    // --- iCCP（解压失败降级 + 告警，不阻断） ---
    let mut iccp = None;
    if let Some(d) = raw.iccp.as_ref() {
        let mut buf: Vec<u8> = Vec::new();
        match parse_iccp(d, &mut buf) {
            Ok(()) => iccp = Some(buf),
            Err(e) => {
                // 结构错（过短/关键字/压缩方法）阻断；解压失败与超限降级
                match e.kind {
                    ColorFaultKind::IccpInflate | ColorFaultKind::IccpTooLarge => {
                        ann.warnings += 1;
                    }
                    _ => return Err(e),
                }
            }
        }
    }

    //合并成胜者（同秩 gAMA+cHRM 取并集）
    let merged = resolve(iccp, srgb_intent, gamma, chrm);
    ann.source = merged.source;
    ann.intent = merged.intent;
    ann.gamma = merged.gamma;
    ann.white_point = merged.white_point;
    ann.icc = merged.icc;
    ann.conflicts = merged.conflicts;
    Ok(ann)
}

// ---------------------------------------------------------------------------
// 六、线性化（F0159 线性纪律联动）
// ---------------------------------------------------------------------------

/// sRGB → 线性（**分段**传递函数，阈值 0.04045）。
///
/// 这是 sRGB 的**标准**传递函数，与「gamma 2.2 近似」是两回事——
/// 混用会让暗部出现可见偏差，故单列一个函数而不并入 [`gamma_to_linear`]。
pub fn srgb_to_linear(v: f32) -> f32 {
    let x = clamp01(v);
    if x <= 0.04045 {
        x / 12.92
    } else {
        ((x + 0.055) / 1.055).powf(2.4)
    }
}

/// 纯 gamma 线性化 `x^γ`（用于 `gAMA` 声明的 gamma）。
pub fn gamma_to_linear(v: f32, gamma: f32) -> f32 {
    let x = clamp01(v);
    if x <= 0.0 {
        return 0.0;
    }
    // gamma 越界时退化为恒等（调用方应已按 parse_gama 拒掉越界值；
    // 此处兜底不 panic 且行为可预期）
    if !(gamma > 0.0) || !is_finite(gamma) {
        return x;
    }
    x.powf(gamma)
}

/// 钳到 `[0,1]`（HDR 管线只接受归一化值）。
#[inline]
fn clamp01(v: f32) -> f32 {
    if !is_finite(v) {
        return 0.0;
    }
    if v < 0.0 {
        0.0
    } else if v > 1.0 {
        1.0
    } else {
        v
    }
}
// ---------------------------------------------------------------------------
// 七、单元测试（回归层——与 `vef04_checks` 的判据层分工不同）
// ---------------------------------------------------------------------------
//
// 分工：判据层回答「规格有没有被满足」，本层回答「实现有没有被改坏」。
// 判据全绿也可能掩盖某个具体边界被悄悄放松（判据是抽样，测试是逐个），
// 故两层互不可省。语料一律**自造**（现搭 PNG，不引外部文件）——外部语料
// 会让「谁坏了」变成「哪个文件变了」，破坏回归定位能力。

// ---------------------------------------------------------------------------
// 七、单元测试（回归层——与 `vef04_checks` 的判据层分工不同）
// ---------------------------------------------------------------------------
//
// 分工：判据层回答「规格有没有被满足」，本层回答「实现有没有被改坏」。
// 判据全绿也可能掩盖某个具体边界被悄悄放松（判据是抽样，测试是逐个），
// 故两层互不可省。语料一律**自造**（现搭 PNG，不引外部文件）——外部语料
// 会让「谁坏了」变成「哪个文件变了」，破坏回归定位能力。

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    /// CRC-32（IEEE）。自写而**不复用** `frameledger_ext::crc32`：
    /// 若两者同源，块校验测试会与被测的 CRC 实现同生共死，无法独立证伪。
    fn crc32(bytes: &[u8]) -> u32 {
        let mut c: u32 = 0xFFFF_FFFF;
        for &b in bytes.iter() {
            c ^= b as u32;
            for _ in 0..8 {
                let m = if c & 1 != 0 { 0xEDB8_8320 } else { 0 };
                c = (c >> 1) ^ m;
            }
        }
        !c
    }

    fn chunk(ty: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(&(data.len() as u32).to_be_bytes());
        v.extend_from_slice(ty);
        v.extend_from_slice(data);
        let mut crc_in = Vec::new();
        crc_in.extend_from_slice(ty);
        crc_in.extend_from_slice(data);
        v.extend_from_slice(&crc32(&crc_in).to_be_bytes());
        v
    }

    /// 造最小合法 PNG（IHDR + 色彩块 + IEND）。
    fn png_with(chunks: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&crate::svstar2::vef01_pngdec::PNG_SIG);
        let mut ihdr = [0u8; 13];
        ihdr[..4].copy_from_slice(&1u32.to_be_bytes());
        ihdr[4..8].copy_from_slice(&1u32.to_be_bytes());
        ihdr[8] = 8;
        ihdr[9] = 6;
        out.extend_from_slice(&chunk(b"IHDR", &ihdr));
        for (ty, data) in chunks.iter() {
            out.extend_from_slice(&chunk(ty, data));
        }
        out.extend_from_slice(&chunk(b"IEND", &[]));
        out
    }

    /// gAMA 载荷（gamma × 100000 的大端 u32）。
    fn gama(g: f32) -> Vec<u8> {
        ((g * 100000.0).round() as u32).to_be_bytes().to_vec()
    }

    /// cHRM 载荷：8 个大端 u32（× 100000）。
    fn chrm_of(vals: [u32; 8]) -> Vec<u8> {
        let mut v = Vec::new();
        for x in vals.iter() {
            v.extend_from_slice(&x.to_be_bytes());
        }
        v
    }

    /// 造一个 zlib 流：stored（未压缩）块封装——不依赖压缩器即可产出合法 zlib。
    fn stored_zlib(data: &[u8]) -> Vec<u8> {
        let mut v = vec![0x78u8, 0x01]; // CM=deflate/32K window, FLEVEL=fast
        let mut off = 0usize;
        loop {
            let n = if data.len() - off > 65535 { 65535 } else { data.len() - off };
            let last = if off + n >= data.len() { 1u8 } else { 0u8 };
            v.push(last); // BFINAL + BTYPE=00(stored)
            v.extend_from_slice(&(n as u16).to_le_bytes());
            v.extend_from_slice(&(!(n as u16)).to_le_bytes());
            v.extend_from_slice(&data[off..off + n]);
            off += n;
            if off >= data.len() {
                break;
            }
        }
        // Adler-32
        let (mut a, mut b) = (1u32, 0u32);
        for &byte in data.iter() {
            a = (a + byte as u32) % 65521;
            b = (b + a) % 65521;
        }
        v.extend_from_slice(&(((b << 16) | a).to_be_bytes()));
        v
    }

    /// 造一个带 iCCP 块的 PNG 载荷（keyword + 方法字节 + zlib 流）。
    fn iccp_payload(z: &[u8]) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(b"ICC\x00");
        v.push(0); // compression method：只定义 0
        v.extend_from_slice(z);
        v
    }

    // ---- 解析往返 ----

    #[test]
    fn vef04_srgb_roundtrip_all_intents() {
        for w in 0u8..=3 {
            let f = png_with(&[(&chunk::SRGB, vec![w])]);
            let ann = parse_color_annotation(&f).expect("合法 sRGB 不应报错");
            assert_eq!(ann.intent, RenderIntent::from_wire(w), "意图 {} 往返不一致", w);
            assert_eq!(ann.source, Some(ColorSource::Srgb));
        }
    }

    #[test]
    fn vef04_gama_roundtrip() {
        for g in [0.01f32, 0.45455, 1.0, 2.2, 5.0] {
            let f = png_with(&[(&chunk::GAMA, gama(g))]);
            let ann = parse_color_annotation(&f).expect("合法 gAMA 不应报错");
            let got = ann.gamma.expect("gAMA 应被接受");
            assert!((got - g).abs() < 1e-4, "gamma {} 往返偏差 {}", g, got - g);
        }
    }

    #[test]
    fn vef04_chrm_d65_roundtrip() {
        // sRGB 标准白点 D65 = (0.3127, 0.3290)
        let d65 = chrm_of([31270, 32900, 64000, 33000, 30000, 60000, 15000, 6000]);
        let out = parse_chrm(&d65).expect("D65 应合法");
        assert!((out[0] - 0.3127).abs() < 1e-4);
        assert!((out[1] - 0.3290).abs() < 1e-4);
        for (i, v) in out.iter().enumerate() {
            assert!(*v >= 0.0 && *v <= 1.0, "坐标 {} = {} 越界", i, v);
        }
    }

    #[test]
    fn vef04_iccp_profile_bytes_pass_through_unchanged() {
        // 传递链路通：iCCP 解压出的字节必须与原 profile 逐字节相同
        let raw: &[u8] = b"fake-icc-profile-payload";
        let f = png_with(&[(&chunk::ICCP, iccp_payload(&stored_zlib(raw)))]);
        let ann = parse_color_annotation(&f).expect("合法 iCCP 不应报错");
        let got = ann.icc.expect("iCCP 应被接受");
        assert_eq!(got.len(), raw.len(), "解压后长度不符");
        for (i, (a, b)) in got.iter().zip(raw.iter()).enumerate() {
            assert_eq!(a, b, "第 {} 字节被改写", i);
        }
    }

    // ---- 边界夹逼（闭区间两端必须被接受）----

    #[test]
    fn vef04_gama_bounds_are_inclusive() {
        assert!(parse_gama(&gama(0.01)).is_ok(), "下界 0.01 应被接受");
        assert!(parse_gama(&gama(5.0)).is_ok(), "上界 5.0 应被接受");
    }

    #[test]
    fn vef04_gama_out_of_range_rejected() {
        // 越界必须**拒绝该块**（不是钳到边界——钳位会产出伪造标注）
        assert!(parse_gama(&gama(0.0)).is_err(), "gamma=0 应拒");
        assert!(parse_gama(&gama(5.01)).is_err(), "gamma=5.01 应拒");
        assert!(parse_gama(&gama(100.0)).is_err(), "gamma=100 应拒");
    }

    #[test]
    fn vef04_gama_wrong_length_blocked() {
        assert!(parse_gama(&[0u8; 3]).is_err(), "长度 3 应阻断");
        assert!(parse_gama(&[0u8; 5]).is_err(), "长度 5 应阻断");
    }

    #[test]
    fn vef04_chrm_every_coordinate_validated() {
        // 逐个坐标越界都必须被拒——不能只测首项
        let base = [31270u32, 32900, 64000, 33000, 30000, 60000, 15000, 6000];
        for i in 0..8 {
            let mut bad = base;
            bad[i] = 200_000; // 远超 1.0
            let e = parse_chrm(&chrm_of(bad));
            assert!(e.is_err(), "坐标 {} 越界却通过了", i);
            if let Err(fault) = e {
                assert_eq!(fault.kind, ColorFaultKind::ChrmCoordinate, "坐标 {} 故障类别不对", i);
                assert_eq!(fault.b, i as u64, "故障五元组未指明是第几个坐标");
            }
        }
    }

    #[test]
    fn vef04_chrm_length_must_be_32() {
        assert!(parse_chrm(&[0u8; 31]).is_err(), "31 字节应阻断");
        assert!(parse_chrm(&[0u8; 33]).is_err(), "33 字节应阻断");
        assert!(parse_chrm(&[0u8; 32]).is_ok(), "32 字节应通过");
    }

    // ---- 优先级裁决 ----

    #[test]
    fn vef04_iccp_beats_srgb_and_gama() {
        let f = png_with(&[
            (&chunk::ICCP, iccp_payload(&stored_zlib(b"fakeprofile"))),
            (&chunk::SRGB, vec![1]),
            (&chunk::GAMA, gama(1.0)),
        ]);
        let ann = parse_color_annotation(&f).expect("iCCP 合法时不应报错");
        assert_eq!(ann.source, Some(ColorSource::Iccp), "iCCP 应胜出");
        let mut ignored: Vec<ColorSource> = Vec::new();
        for c in ann.conflicts.iter() {
            ignored.push(c.ignored);
        }
        assert!(ignored.contains(&ColorSource::Srgb), "sRGB 应记为被忽略");
        assert!(ignored.contains(&ColorSource::GammaChrm), "gAMA+cHRM 应记为被忽略");
    }

    #[test]
    fn vef04_srgb_beats_gama() {
        let f = png_with(&[(&chunk::SRGB, vec![0]), (&chunk::GAMA, gama(2.2))]);
        let ann = parse_color_annotation(&f).unwrap();
        assert_eq!(ann.source, Some(ColorSource::Srgb));
        assert_eq!(ann.conflicts.len(), 1, "gAMA 应被记账一次");
        assert_eq!(ann.conflicts[0].ignored, ColorSource::GammaChrm);
    }

    #[test]
    fn vef04_gama_alone_is_still_gamma_chrm_source() {
        // 只有 gAMA 没有 cHRM：仍是 gAMA+cHRM 这一来源（半截标注不误判为完整）
        let f = png_with(&[(&chunk::GAMA, gama(1.8))]);
        let ann = parse_color_annotation(&f).unwrap();
        assert_eq!(ann.source, Some(ColorSource::GammaChrm));
        assert!(ann.white_point.is_none(), "无 cHRM 则白点应为空");
        assert!(ann.gamma.is_some(), "gamma 值应保留（下游仍可用）");
    }

    #[test]
    fn vef04_missing_all_falls_back_to_srgb() {
        let f = png_with(&[]);
        let ann = parse_color_annotation(&f).expect("无色彩块应按 sRGB 假定，不该报错");
        assert_eq!(ann.source, None, "全缺失时 source 应为空（=按 sRGB 假定）");
        assert!(ann.icc.is_none() && ann.gamma.is_none() && ann.white_point.is_none());
    }

    // ---- 错误路径 ----

    #[test]
    fn vef04_iccp_corrupt_zlib_degrades_with_warning() {
        // 坏 zlib 流：必须降级 + 告警，**不得**让整图不可用
        let f = png_with(&[(&chunk::ICCP, iccp_payload(&[0xFF, 0xFF, 0xFF, 0xFF]))]);
        let ann = parse_color_annotation(&f).expect("坏 ICC 只降级，不返回 Err");
        assert!(ann.icc.is_none(), "坏 profile 不应进标注");
        assert!(ann.warnings > 0, "降级必须计数告警");
    }

    #[test]
    fn vef04_iccp_bad_method_is_blocking_not_degrading() {
        // 压缩方法非 0：规范只定义 0。这是**结构错**（块自身不可用），
        // 与「解压失败」（内容坏，降级继续）分属不同处置方向：
        //   解压失败 / 超限 -> 降级 sRGB 假定 + 告警，不阻断；
        //   方法字节非法     -> 阻断。
        // 两者处置方向相反，绝不可共用同一码或同一路径。
        let mut p = Vec::new();
        p.extend_from_slice(b"ICC\x00");
        p.push(9); // 非法方法
        p.extend_from_slice(&stored_zlib(b"x"));
        let f = png_with(&[(&chunk::ICCP, p)]);
        let e = parse_color_annotation(&f);
        assert!(e.is_err(), "非法压缩方法应阻断而非降级");
        if let Err(fault) = e {
            assert_eq!(fault.kind, ColorFaultKind::IccpCompression, "故障类别不对");
            assert_eq!(fault.a, 9, "五元组应带出实际方法字节值");
        }
    }

    #[test]
    fn vef04_bad_signature_rejected() {
        let f = vec![0u8; 32];
        let e = parse_color_annotation(&f);
        assert!(e.is_err(), "签名错应拒绝");
        if let Err(fault) = e {
            assert_eq!(fault.kind, ColorFaultKind::BadSignature);
        }
    }

    #[test]
    fn vef04_crc_error_is_warning_not_block() {
        // CRC 错的 ancillary 块：告警跳过，不阻断
        let mut out = Vec::new();
        out.extend_from_slice(&crate::svstar2::vef01_pngdec::PNG_SIG);
        let mut ihdr = [0u8; 13];
        ihdr[..4].copy_from_slice(&1u32.to_be_bytes());
        ihdr[4..8].copy_from_slice(&1u32.to_be_bytes());
        ihdr[8] = 8;
        ihdr[9] = 6;
        out.extend_from_slice(&chunk(b"IHDR", &ihdr));
        let mut bad = chunk(&chunk::SRGB, &[0u8]);
        let n = bad.len();
        bad[n - 1] ^= 0xFF; // 破坏 CRC
        out.extend_from_slice(&bad);
        out.extend_from_slice(&chunk(b"IEND", &[]));
        let ann = parse_color_annotation(&out).expect("CRC 错不应阻断");
        assert!(ann.warnings > 0, "CRC 错必须计数");
        assert!(ann.intent.is_none(), "CRC 错的块内容不应被采信");
    }

    #[test]
    fn vef04_truncated_chunk_rejected() {
        // 声明长度超出文件尾：必须拒绝（而不是读越界）
        let mut out = Vec::new();
        out.extend_from_slice(&crate::svstar2::vef01_pngdec::PNG_SIG);
        out.extend_from_slice(&1000u32.to_be_bytes()); // 长度 1000
        out.extend_from_slice(b"sRGB");
        out.extend_from_slice(&[0u8]); // 实际只有 1 字节
        let e = parse_color_annotation(&out);
        assert!(e.is_err(), "截断块应拒绝");
    }

    // ---- 线性化 ----

    #[test]
    fn vef04_srgb_linearize_endpoints_and_monotonic() {
        assert!(srgb_to_linear(0.0).abs() < 1e-6, "0 应为 0");
        assert!((srgb_to_linear(1.0) - 1.0).abs() < 1e-6, "1 应为 1");
        let a = srgb_to_linear(0.25);
        let b = srgb_to_linear(0.5);
        assert!(a > 0.0 && a < b && b < 1.0, "单调性被破坏：{} {}", a, b);
    }

    #[test]
    fn vef04_srgb_linearize_clamps_out_of_range() {
        assert_eq!(srgb_to_linear(-1.0), 0.0, "负值应钳到 0");
        assert_eq!(srgb_to_linear(2.0), 1.0, "超 1 应钳到 1");
        assert_eq!(srgb_to_linear(f32::NAN), 0.0, "NaN 应被收口");
    }

    #[test]
    fn vef04_gamma_to_linear_illegal_gamma_is_identity() {
        // 非法 gamma 退化为恒等（不 panic、结果可预期）
        let x = 0.5;
        assert!((gamma_to_linear(x, 0.0) - x).abs() < 1e-6, "gamma=0 应退化恒等");
        assert!((gamma_to_linear(x, -1.0) - x).abs() < 1e-6, "gamma<0 应退化恒等");
        assert!((gamma_to_linear(x, f32::NAN) - x).abs() < 1e-6, "NaN gamma 应退化恒等");
    }

    #[test]
    fn vef04_gamma_to_linear_zero_stays_zero() {
        // 0^γ 必须恒为 0，不能算出 0^0=1（黑变白）
        assert_eq!(gamma_to_linear(0.0, 2.2), 0.0);
    }

    // ---- 结构不变量 ----

    #[test]
    fn vef04_priority_table_is_strictly_descending() {
        let r = ColorSource::Iccp.rank();
        let s = ColorSource::Srgb.rank();
        let g = ColorSource::GammaChrm.rank();
        assert!(r > s && s > g, "优先级表必须严格降序：{} {} {}", r, s, g);
    }

    #[test]
    fn vef04_fault_codes_are_unique() {
        // 处置方向相反的状态不得共用码：不同 kind 必须映射到不同错误码
        let kinds = [
            ColorFaultKind::BadSignature,
            ColorFaultKind::ChunkTruncated,
            ColorFaultKind::SrgbLength,
            ColorFaultKind::BadIntent,
            ColorFaultKind::GamaLength,
            ColorFaultKind::CrcMismatch,
            ColorFaultKind::GamaOutOfRange,
            ColorFaultKind::ChrmLength,
            ColorFaultKind::ChrmCoordinate,
            ColorFaultKind::IccpTooShort,
            ColorFaultKind::IccpKeyword,
            ColorFaultKind::IccpCompression,
            ColorFaultKind::IccpInflate,
            ColorFaultKind::IccpTooLarge,
        ];
        for i in 0..kinds.len() {
            for j in (i + 1)..kinds.len() {
                assert_ne!(kinds[i].code(), kinds[j].code(), "错误码重复：{:?}", kinds[i]);
            }
        }
    }

    #[test]
    fn vef04_srgb_wire_roundtrip_all_intents() {
        // 枚举判别值 ≠ 线上编码值：wire() 与 from_wire() 必须互逆
        for w in 0u8..=3 {
            let it = RenderIntent::from_wire(w).expect("0..3 应全合法");
            assert_eq!(it.wire(), w, "意图 {} 的 wire() 不等于原值", w);
        }
        for bad in [4u8, 5, 100, 255] {
            assert!(RenderIntent::from_wire(bad).is_none(), "{} 应被拒", bad);
        }
    }
}
