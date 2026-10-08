//! VE-F0415 · 源码编码处理（VE-C 域 · 着色器系统 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0415`
//!
//! **判据（锚点原文）**：BOM 优先、UTF-8 假定、非法报错、一次转换、判据。
//!
//! 锚点职责定位原文：
//! > 源码编码识别与转换：BOM 检测、无 BOM 时 UTF-8 假定语义（显性声明）、非法字节
//! > 序列报错（位置与字节值）、内部统一 UTF-8 处理（转换边界一次到位）；编码信息
//! > 入诊断元数据。
//!
//! 本条只做「把一段原始字节变成一段确定无歧义的 Rust `String`」这一个动作，并且把
//! 四件容易做错的事显式钉死：
//!
//! 1. **BOM 优先**（判据一）。字节序标记的优先级高于一切声明与假定，并且探测必须
//!    **最长匹配优先**——这是本条最容易埋雷的地方：UTF-32LE 的 BOM 是
//!    `FF FE 00 00`，它**以 UTF-16LE 的 BOM `FF FE` 为前缀**。若按短的可能先判，
//!    一个 UTF-32 文件会被切成 UTF-16LE 再把剩下的 `00 00` 当正文吃掉，产出半截
//!    乱码且**全程无报错**（静默数据损坏，比报错难查十倍）。`detect_bom` 因此把
//!    4 字节形态排在 2 字节形态之前，并在 `C15-BOM-*` 判据里钉死这个顺序。
//!    优先级总序：BOM > 构建系统显式声明（`DecodeConfig::declared`）> UTF-8 假定。
//!
//! 2. **UTF-8 假定语义显性**（判据二）。无 BOM 时按 UTF-8 处理，但这个决定必须
//!    **留痕**：`DetectionSource::AssumedUtf8` 进 `EncodingMeta`，诊断与工具链能
//!    区分「文件自己声明了编码」和「我们猜的」。项目若要求强制带 BOM，用
//!    `AssumePolicy::RequireBom` 把假定变成硬门——缺 BOM 直接报错，不许悄悄假定。
//!
//! 3. **非法字节序列报错带位置与字节值**（判据三）。UTF-8 的非法形态有五类
//!    （孤立续字节 / 过长编码 / 代理项 / 超 U+10FFFF / 序列截断），每类的
//!    字节边界都不同，混成一类报「非法 UTF-8」等于没报：作者看到 `0xE0 0x80`
//!    不知道是过长编码，代码要能告诉他。`EncodingFault` 五类分立，附字节下标、
//!    字节值、行列与修法建议。**越界方向**也是分类的一部分：第二字节**低于**
//!    紧下界是过长编码（`E0 80`），**高于**紧上界是代理项或越界（`ED A0` /
//!    `F4 90`）——把两者混为一谈会把作者引向错误方向。
//!
//! 4. **一次转换到位**（判据四）。产物 `DecodedSource.text` 恒为 UTF-8 `String`，
//!    下游 F0403 词法主路**不再做任何编码判断**。转换是**单遍**的：
//!    `ConversionBoundary::bytes_consumed` 记实际消费字节数、`passes` 恒为 1，
//!    两者相等即「读一遍就转完、没有偷偷重扫」的自证。UTF-8 路径更省——已经是
//!    UTF-8 就**不重拷一次**，只做校验。
//!
//! **不静默替换（锚点错误路径第三条）**：UTF-16 孤立代理项、UTF-32 越界标量、
//!    UTF-8 非法序列，一律 `Err` 显性失败，**绝不产出 U+FFFD（`�`）**。用
//!    `String::from_utf8_lossy` 一步就把坏文件「修好」的写法会把着色器编译错误
//!    推迟到语义期且位置全错，比直接失败有害得多。本模块提供
//!    `contains_replacement_char` 供上层与自检核验产物中不含替换字符。
//!
//! 零静默纪律：BOM 与声明/假定冲突 → 以 BOM 为准并**出注记**告知；BOM 剥离 →
//!   出注记记字节数；按假定处理 → 出注记；ZWNBSP 歧义 → 出注记；六类非法字节、
//!   孤立代理项、UTF-32 越界、奇数长度单元、缺 BOM（RequireBom 模式）→ 各自
//!   独立错误码，不合并、不吞。本模块不抛异常、不吞诊断、无全局可变状态、零 IO。
//!
//! 性能逐项分解：探测 O(头部长) 一次性（最多看 4 字节，`head_examined` 可核）；
//!   转换 O(文件长) 单遍（`bytes_consumed` 可核）；行列定位 O(prefix) 仅在出错
//!   时算一次；元数据构造 O(1)。UTF-8 且无 BOM 时**零拷贝**（只校验不重建）。
//!
//! 上游消费：F0414 `resolve_includes` 的展开文本字节流与其 `IncludeConfig`。
//!   下游 F0403 词法主路消费 `DecodedSource::text`（恒 UTF-8），F0407 字符串
//!   字面量消费 `EncodingMeta.encoding` 作为串内字节的源编码裁定依据。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 替换字符 U+FFFD——本模块**禁止**产出它（锚点「不静默替换」）。
pub const REPLACEMENT_CHAR: char = '\u{FFFD}';

/// BOM 最长字节数（UTF-32 形态 4 字节）。探测最多看这么多字节。
pub const BOM_MAX_LEN: usize = 4;

/// 产物文本中是否混进了替换字符（自检与上层核验用；恒应��� `false`）。
pub fn contains_replacement_char(text: &str) -> bool {
    text.chars().any(|c| c == REPLACEMENT_CHAR)
}

// ---------------------------------------------------------------------------
// 一、源编码（探测结论）
// ---------------------------------------------------------------------------

/// 源编码。VE-Shade 内部统一 UTF-8，这五类是**输入侧**的编码。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceEncoding {
    /// UTF-8（内部目标编码，也是无 BOM 时的假定值）。
    Utf8,
    /// UTF-16 小端。
    Utf16Le,
    /// UTF-16 大端。
    Utf16Be,
    /// UTF-32 小端。
    Utf32Le,
    /// UTF-32 大端。
    Utf32Be,
}

impl SourceEncoding {
    /// 人话标签（诊断用）。
    pub const fn label(self) -> &'static str {
        match self {
            SourceEncoding::Utf8 => "UTF-8",
            SourceEncoding::Utf16Le => "UTF-16LE",
            SourceEncoding::Utf16Be => "UTF-16BE",
            SourceEncoding::Utf32Le => "UTF-32LE",
            SourceEncoding::Utf32Be => "UTF-32BE",
        }
    }

    /// 该编码下一个码元占几字节（`line_col` 的列数单位）。
    pub const fn unit_bytes(self) -> usize {
        match self {
            SourceEncoding::Utf8 => 1,
            SourceEncoding::Utf16Le | SourceEncoding::Utf16Be => 2,
            SourceEncoding::Utf32Le | SourceEncoding::Utf32Be => 4,
        }
    }

    /// 是否为多字节编码（宽编码）。
    pub const fn is_wide(self) -> bool {
        !matches!(self, SourceEncoding::Utf8)
    }

    /// 该编码的 BOM 字节序列（无 BOM 则是空）。
    pub const fn bom_bytes(self) -> &'static [u8] {
        match self {
            SourceEncoding::Utf8 => &[0xEF, 0xBB, 0xBF],
            SourceEncoding::Utf16Le => &[0xFF, 0xFE],
            SourceEncoding::Utf16Be => &[0xFE, 0xFF],
            SourceEncoding::Utf32Le => &[0xFF, 0xFE, 0x00, 0x00],
            SourceEncoding::Utf32Be => &[0x00, 0x00, 0xFE, 0xFF],
        }
    }

    /// 由 BOM 判定编码（`BomKind::None` → `None`）。
    pub const fn from_bom(bom: BomKind) -> Option<SourceEncoding> {
        match bom {
            BomKind::None => None,
            BomKind::Utf8 => Some(SourceEncoding::Utf8),
            BomKind::Utf16Le => Some(SourceEncoding::Utf16Le),
            BomKind::Utf16Be => Some(SourceEncoding::Utf16Be),
            BomKind::Utf32Le => Some(SourceEncoding::Utf32Le),
            BomKind::Utf32Be => Some(SourceEncoding::Utf32Be),
        }
    }

    /// 单个原始字节在该编码内是否可直接存在（F0407 `\xNN` 裁定依据）。
    pub const fn byte_encodable(self, b: u8) -> bool {
        match self {
            SourceEncoding::Utf8 => b < 0x80,
            _ => true,
        }
    }
}

/// BOM 形态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BomKind {
    /// 无 BOM。
    None,
    /// UTF-8 BOM（`EF BB BF`）。
    Utf8,
    /// UTF-16LE BOM（`FF FE`）。
    Utf16Le,
    /// UTF-16BE BOM（`FE FF`）。
    Utf16Be,
    /// UTF-32LE BOM（`FF FE 00 00`）。
    Utf32Le,
    /// UTF-32BE BOM（`00 00 FE FF`）。
    Utf32Be,
}

impl BomKind {
    /// BOM 字节数（`None` 为 0）。
    pub const fn bom_len(self) -> usize {
        match self {
            BomKind::None => 0,
            BomKind::Utf8 => 3,
            BomKind::Utf16Le | BomKind::Utf16Be => 2,
            BomKind::Utf32Le | BomKind::Utf32Be => 4,
        }
    }

    /// 人话标签。
    pub const fn label(self) -> &'static str {
        match self {
            BomKind::None => "无 BOM",
            BomKind::Utf8 => "UTF-8 BOM",
            BomKind::Utf16Le => "UTF-16LE BOM",
            BomKind::Utf16Be => "UTF-16BE BOM",
            BomKind::Utf32Le => "UTF-32LE BOM",
            BomKind::Utf32Be => "UTF-32BE BOM",
        }
    }
}

/// BOM 探测记录（锚点「编码探测记录」数据结构）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BomMatch {
    /// 命中的 BOM 形态。
    pub kind: BomKind,
    /// BOM 字节数（0 表示无 BOM）。
    pub len: usize,
    /// 探测实际查看的字节数（≤ `BOM_MAX_LEN`，用于核验「探测 O(头部长) 一次性」）。
    pub head_examined: usize,
}

impl BomMatch {
    /// 无 BOM 记录。
    pub const fn absent() -> BomMatch {
        BomMatch {
            kind: BomKind::None,
            len: 0,
            head_examined: 0,
        }
    }
}

/// BOM 探测（判据一）。
///
/// **最长匹配优先**是本函数唯一的正确性要害：UTF-32LE（`FF FE 00 00`）以
/// UTF-16LE（`FF FE`）为前缀，故 4 字节形态必须先判。只看头部 4 字节，
/// `head_examined` 如实记账。
pub fn detect_bom(raw: &[u8]) -> BomMatch {
    let head_len = if raw.len() < BOM_MAX_LEN { raw.len() } else { BOM_MAX_LEN };
    let head = match raw.get(..head_len) {
        Some(h) => h,
        None => return BomMatch::absent(),
    };
    let examined = head.len();

    // 零 panic 面：全用 `get` 取字节，越界即 None（不按下标硬取）。
    // 头部长度已由上面 `raw.get(..head_len)` 保证不超过 BOM_MAX_LEN。

    // ---- 4 字节形态优先（UTF-32LE / UTF-32BE）----
    // 最长匹配要害：UTF-32LE 的 BOM 以 UTF-16LE 的 BOM 为前缀，短的可能先判
    // 会把 UTF-32 文件切成 UTF-16LE 并静默吃掉后两字节。
    if let Some(b) = head.get(..4) {
        let b4: [u8; 4] = [b[0], b[1], b[2], b[3]];
        if b4 == [0xFF, 0xFE, 0x00, 0x00] {
            return BomMatch { kind: BomKind::Utf32Le, len: 4, head_examined: examined };
        }
        if b4 == [0x00, 0x00, 0xFE, 0xFF] {
            return BomMatch { kind: BomKind::Utf32Be, len: 4, head_examined: examined };
        }
    }
    // ---- 3 字节形态（UTF-8）----
    if let Some(b) = head.get(..3) {
        if b[0] == 0xEF && b[1] == 0xBB && b[2] == 0xBF {
            return BomMatch { kind: BomKind::Utf8, len: 3, head_examined: examined };
        }
    }
    // ---- 2 字节形态（UTF-16）----
    if let Some(b) = head.get(..2) {
        if b[0] == 0xFF && b[1] == 0xFE {
            return BomMatch { kind: BomKind::Utf16Le, len: 2, head_examined: examined };
        }
        if b[0] == 0xFE && b[1] == 0xFF {
            return BomMatch { kind: BomKind::Utf16Be, len: 2, head_examined: examined };
        }
    }
    BomMatch { kind: BomKind::None, len: 0, head_examined: examined }
}

// ---------------------------------------------------------------------------
// 二、探测记录与诊断元数据（判据：编码信息入诊断元数据）
// ---------------------------------------------------------------------------

/// 编码是怎么定下来的（判据二「显性声明」）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DetectionSource {
    /// 由 BOM 判定（最高优先级）。
    Bom,
    /// 由构建系统显式声明（`DecodeConfig::declared`）。
    Declared,
    /// 无 BOM 无声明，按 UTF-8 假定（判据二）。
    AssumedUtf8,
}

impl DetectionSource {
    /// 人话标签。
    pub const fn label(self) -> &'static str {
        match self {
            DetectionSource::Bom => "BOM 判定",
            DetectionSource::Declared => "构建系统声明",
            DetectionSource::AssumedUtf8 => "UTF-8 假定",
        }
    }

    /// 是否为「猜的」（假定即猜）。
    pub const fn is_assumption(self) -> bool {
        matches!(self, DetectionSource::AssumedUtf8)
    }
}

/// 编码探测记录（锚点「编码探测记录」）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EncodingProbe {
    /// 探测出的源编码。
    pub encoding: SourceEncoding,
    /// 判定来源。
    pub source: DetectionSource,
    /// 命中的 BOM 形态。
    pub bom: BomKind,
    /// BOM 字节数（0 表示无 BOM）。
    pub bom_len: usize,
    /// 探测查看的字节数。
    pub head_examined: usize,
}

impl EncodingProbe {
    /// 探测编码（BOM > 声明 > UTF-8 假定）。
    ///
    /// `declared` 是构建系统告知的编码，仅在**无 BOM** 时生效——BOM 永远压过它
    /// （判据一）。
    pub fn probe(raw: &[u8], declared: Option<SourceEncoding>) -> EncodingProbe {
        let bom = detect_bom(raw);
        let (encoding, source) = match SourceEncoding::from_bom(bom.kind) {
            Some(enc) => (enc, DetectionSource::Bom),
            None => match declared {
                Some(enc) => (enc, DetectionSource::Declared),
                None => (SourceEncoding::Utf8, DetectionSource::AssumedUtf8),
            },
        };
        EncodingProbe {
            encoding,
            source,
            bom: bom.kind,
            bom_len: bom.len,
            head_examined: bom.head_examined,
        }
    }
}

/// 编码诊断元数据（锚点「诊断元数据」数据结构）。
///
/// 这是「编码信息入诊断元数据」的落点：下游 F0416 词法错误报告与 F0407 串编码
/// 裁定都从这里取「源编码 + 判定来源 + 字节跨度」，而不各自再猜一遍。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EncodingMeta {
    /// 源编码。
    pub encoding: SourceEncoding,
    /// 判定来源（BOM / 声明 / 假定）。
    pub source: DetectionSource,
    /// BOM 字节数（0 表示无 BOM）。
    pub bom_len: usize,
    /// 源字节总长。
    pub byte_len: usize,
    /// 标量值个数（转换后字符数）。
    pub char_len: usize,
    /// 剥离 BOM 后的正文字节数。
    pub body_bytes: usize,
    /// 是否走「只校验 + 整块拷贝」路径（UTF-8 且无需剥离/重建）。
    pub validate_only: bool,
}

impl EncodingMeta {
    /// 人话摘要（诊断抬头用）。
    pub fn summary(&self) -> String {
        format!(
            "源编码 {}（{}），共 {} 字节 / {} 字符{}",
            self.encoding.label(),
            self.source.label(),
            self.byte_len,
            self.char_len,
            if self.bom_len > 0 {
                format!("，已剥离 {} 字节 BOM", self.bom_len)
            } else {
                String::from("，无 BOM")
            }
        )
    }
}

// ---------------------------------------------------------------------------
// 三、故障分类（六类 UTF-8 非法形态 + 宽编码故障）
// ---------------------------------------------------------------------------

/// 编码故障（判据三：位置与字节值）。
///
/// UTF-8 的六类非法形态**分立不合并**：各类第二字节的合法区间不同，合并成
/// 一个「非法 UTF-8」作者无从下手。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EncodingFault {
    /// 孤立续字节：首字节不是合法首字节（`80..BF`）。
    UnexpectedContinuation { index: usize, byte: u8 },
    /// 过长编码：`C0`/`C1` 首字节，或 `E0 80`/`F0 80` 类第二字节越下界。
    Overlong { index: usize, byte: u8 },
    /// UTF-8 内出现 UTF-16 代理项码位（`ED A0..BF`）。
    SurrogateInUtf8 { index: usize, byte: u8 },
    /// 超出 U+10FFFF（首字节 `F5..FF`，或 `F4 90..`）。
    OutOfRange { index: usize, byte: u8 },
    /// 序列被截断（EOF 中途）。
    TruncatedSequence { index: usize, byte: u8, need: usize },
    /// UTF-16 孤立高代理项（后面不是低代理项）。
    UnpairedHighSurrogate { index: usize, unit: u16 },
    /// UTF-16 孤立低代理项（前面不是高代理项）。
    UnpairedLowSurrogate { index: usize, unit: u16 },
    /// UTF-16 字节数为奇数（末尾落单字节）。
    OddUtf16Length { index: usize },
    /// UTF-32 字节数不是 4 的整数倍。
    Utf32Length { index: usize, len: usize },
    /// UTF-32 标量越界（> U+10FFFF 或落在代理项区）。
    Utf32Scalar { index: usize, value: u32 },
    /// `RequireBom` 模式下缺 BOM。
    MissingBom { head_examined: usize },
}

impl EncodingFault {
    /// 稳定错误码（下游可引用；六类不合并）。
    pub const fn code(&self) -> &'static str {
        match self {
            EncodingFault::UnexpectedContinuation { .. } => "VE-F0415-UTF8-CONTINUATION",
            EncodingFault::Overlong { .. } => "VE-F0415-UTF8-OVERLONG",
            EncodingFault::SurrogateInUtf8 { .. } => "VE-F0415-UTF8-SURROGATE",
            EncodingFault::OutOfRange { .. } => "VE-F0415-UTF8-RANGE",
            EncodingFault::TruncatedSequence { .. } => "VE-F0415-UTF8-TRUNCATED",
            EncodingFault::UnpairedHighSurrogate { .. } => "VE-F0415-UTF16-HIGH",
            EncodingFault::UnpairedLowSurrogate { .. } => "VE-F0415-UTF16-LOW",
            EncodingFault::OddUtf16Length { .. } => "VE-F0415-UTF16-ODD",
            EncodingFault::Utf32Length { .. } => "VE-F0415-UTF32-LENGTH",
            EncodingFault::Utf32Scalar { .. } => "VE-F0415-UTF32-SCALAR",
            EncodingFault::MissingBom { .. } => "VE-F0415-BOM-REQUIRED",
        }
    }

    /// 出错字节下标（字节位置，判据三）。
    pub const fn index(&self) -> usize {
        match self {
            EncodingFault::UnexpectedContinuation { index, .. }
            | EncodingFault::Overlong { index, .. }
            | EncodingFault::SurrogateInUtf8 { index, .. }
            | EncodingFault::OutOfRange { index, .. }
            | EncodingFault::TruncatedSequence { index, .. }
            | EncodingFault::UnpairedHighSurrogate { index, .. }
            | EncodingFault::UnpairedLowSurrogate { index, .. }
            | EncodingFault::OddUtf16Length { index }
            | EncodingFault::Utf32Length { index, .. }
            | EncodingFault::Utf32Scalar { index, .. } => *index,
            EncodingFault::MissingBom { .. } => 0,
        }
    }

    /// 出错字节值（判据三要求「字节值」，宽编码给码元/标量）。
    pub const fn byte_value(&self) -> u32 {
        match self {
            EncodingFault::UnexpectedContinuation { byte, .. }
            | EncodingFault::Overlong { byte, .. }
            | EncodingFault::SurrogateInUtf8 { byte, .. }
            | EncodingFault::OutOfRange { byte, .. }
            | EncodingFault::TruncatedSequence { byte, .. } => *byte as u32,
            EncodingFault::UnpairedHighSurrogate { unit, .. }
            | EncodingFault::UnpairedLowSurrogate { unit, .. } => *unit as u32,
            EncodingFault::Utf32Scalar { value, .. } => *value,
            EncodingFault::OddUtf16Length { .. } => 0,
            EncodingFault::Utf32Length { len, .. } => *len as u32,
            EncodingFault::MissingBom { head_examined } => *head_examined as u32,
        }
    }

    /// 修法建议（判据三「带建议」）。
    pub const fn suggestion(&self) -> &'static str {
        match self {
            EncodingFault::UnexpectedContinuation { .. } => {
                "此处是 UTF-8 续字节但前面没有首字节——多半源文件不是 UTF-8，按实际编码重新保存"
            }
            EncodingFault::Overlong { .. } => {
                "这是 UTF-8 过长编码（NUL 等字符被多字节重写了）——换正确编码器重新导出"
            }
            EncodingFault::SurrogateInUtf8 { .. } => {
                "UTF-8 里不允许出现代理项码位——源文件很可能是 UTF-16 被当 UTF-8 存了，按实际编码重存"
            }
            EncodingFault::OutOfRange { .. } => {
                "码位超出 U+10FFFF 上限——不是合法 Unicode 文本，检查是否二进制数据被当源码"
            }
            EncodingFault::TruncatedSequence { .. } => {
                "多字节序列在文件尾被截断——补全末尾字符或删掉这个残缺字符"
            }
            EncodingFault::UnpairedHighSurrogate { .. } => {
                "UTF-16 高代理项后面不是低代理项——文件被截断或编码损坏，修复后重新导出"
            }
            EncodingFault::UnpairedLowSurrogate { .. } => {
                "UTF-16 低代理项前面没有高代理项——文件被截断或编码损坏，修复后重新导出"
            }
            EncodingFault::OddUtf16Length { .. } => {
                "UTF-16 字节数为奇数——末尾多出或缺少一个字节，补齐成偶数"
            }
            EncodingFault::Utf32Length { .. } => "UTF-32 字节数不是 4 的整数倍——文件被截断，补齐或重新导出",
            EncodingFault::Utf32Scalar { .. } => "UTF-32 标量越界或落在代理项区——不是合法 Unicode 文本",
            EncodingFault::MissingBom { .. } => "本项目要求源文件必须带 BOM——以带 BOM 的编码重新保存",
        }
    }

    /// 三要素呈现：发生了什么 / 在哪里 / 怎么修（判据三）。
    ///
    /// `line` `col` 已在 `convert_source` 内按**源编码码元**算好；`col` 的单位是
    /// 该编码的码元宽（UTF-8 为字节，UTF-16 为 2 字节单元，UTF-32 为 4 字节）。
    pub fn describe(&self, line: usize, col: usize) -> String {
        format!(
            "[{}] 第 {line} 行第 {col} 列（字节偏移 {}，字节值 0x{:02X}）：{}。建议：{}",
            self.code(),
            self.index(),
            self.byte_value(),
            self.what(),
            self.suggestion()
        )
    }

    /// 「发生了什么」短句。
    pub fn what(&self) -> &'static str {
        match self {
            EncodingFault::UnexpectedContinuation { .. } => "出现孤立 UTF-8 续字节",
            EncodingFault::Overlong { .. } => "出现 UTF-8 过长编码",
            EncodingFault::SurrogateInUtf8 { .. } => "UTF-8 中出现代理项码位",
            EncodingFault::OutOfRange { .. } => "码位超出 U+10FFFF",
            EncodingFault::TruncatedSequence { .. } => "多字节序列被截断",
            EncodingFault::UnpairedHighSurrogate { .. } => "UTF-16 孤立高代理项",
            EncodingFault::UnpairedLowSurrogate { .. } => "UTF-16 孤立低代理项",
            EncodingFault::OddUtf16Length { .. } => "UTF-16 字节数为奇数",
            EncodingFault::Utf32Length { .. } => "UTF-32 字节长度不整除",
            EncodingFault::Utf32Scalar { .. } => "UTF-32 标量越界",
            EncodingFault::MissingBom { .. } => "要求带 BOM 但源文件没有 BOM",
        }
    }
}

// ---------------------------------------------------------------------------
// 四、转换配置与注记
// ---------------------------------------------------------------------------

/// 无 BOM 时的处置策略（判据二）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssumePolicy {
    /// 按 UTF-8 假定处理并留痕（默认）。
    AssumeUtf8,
    /// 缺 BOM 即报错（项目强制带 BOM 时用）。
    RequireBom,
}

/// 转换配置。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DecodeConfig {
    /// 构建系统显式声明的源编码（仅无 BOM 时生效）。
    pub declared: Option<SourceEncoding>,
    /// 缺 BOM 时的处置策略。
    pub policy: AssumePolicy,
    /// 是否把开头 BOM 当编码标记剥离（默认 true）。
    ///
    /// 置 false 时首部 `EF BB BF` 按零宽不换行空格 U+FEFF 参与正文——用于正文
    /// 真的以 ZWNBSP 开头的极端场景（默认 true 下二者不可区分，出注记告知）。
    pub strip_bom: bool,
}

impl DecodeConfig {
    /// 默认配置：UTF-8 假定 + 剥离 BOM。
    pub const fn new() -> DecodeConfig {
        DecodeConfig {
            declared: None,
            policy: AssumePolicy::AssumeUtf8,
            strip_bom: true,
        }
    }

    /// 设定构建系统声明的源编码。
    pub const fn with_declared(mut self, enc: SourceEncoding) -> DecodeConfig {
        self.declared = Some(enc);
        self
    }

    /// 设定缺 BOM 策略。
    pub const fn with_policy(mut self, policy: AssumePolicy) -> DecodeConfig {
        self.policy = policy;
        self
    }

    /// 设定是否剥离 BOM。
    pub const fn with_strip_bom(mut self, strip: bool) -> DecodeConfig {
        self.strip_bom = strip;
        self
    }
}

impl Default for DecodeConfig {
    fn default() -> Self {
        DecodeConfig::new()
    }
}

/// 转换过程中的显性注记（零静默：这些都要让下游看见）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EncodingNote {
    /// 检测到 BOM 并已剥离。
    BomStripped { kind: BomKind, len: usize },
    /// BOM 与声明/假定冲突——以 BOM 为准（判据一）。
    BomOverrides { declared: SourceEncoding, actual: SourceEncoding },
    /// 无 BOM 无声明，按 UTF-8 假定处理（判据二留痕）。
    AssumedUtf8,
    /// 首部 BOM 字节与「正文以 ZWNBSP 开头」不可区分，已告知取舍。
    ZwspAmbiguity { len: usize },
}

impl EncodingNote {
    /// 人话呈现。
    pub fn describe(&self) -> String {
        match self {
            EncodingNote::BomStripped { kind, len } => {
                format!("检测到{}（{} 字节）并已剥离", kind.label(), len)
            }
            EncodingNote::BomOverrides { declared, actual } => format!(
                "源文件带 {} 的 BOM，与{}的 {} 冲突——以 BOM 为准",
                actual.label(),
                if *declared == SourceEncoding::Utf8 { "UTF-8 假定" } else { "构建系统声明" },
                declared.label()
            ),
            EncodingNote::AssumedUtf8 => {
                "源文件无 BOM 且无声明——按 UTF-8 假定处理（判定来源已记入诊断元数据）".to_string()
            }
            EncodingNote::ZwspAmbiguity { len } => format!(
                "首 {len} 字节既是 BOM 又是合法的 U+FEFF 零宽不换行空格——已按 BOM 剥离；若本意是正文 ZWNBSP，请把配置的 strip_bom 置 false"
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// 五、转换边界（判据四：一次转换到位）
// ---------------------------------------------------------------------------

/// 转换边界（锚点「转换边界」数据结构）。
///
/// 记的是「一次转换」的账：`passes` 恒为 1，`bytes_consumed` 与源字节数相等
/// 即证明单遍读完转完、没有回扫第二遍。`validate_only` 表示源本就是 UTF-8 且无 BOM，
/// 连重拷都省了（只校验不重建）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConversionBoundary {
    /// 剥离的 BOM 字节数。
    pub bom_stripped: usize,
    /// 源字节总数。
    pub source_bytes: usize,
    /// 转换实际消费的字节数（应等于 `source_bytes`）。
    pub bytes_consumed: usize,
    /// 转换遍数（恒为 1——单遍）。
    pub passes: u8,
    /// 目标形态恒为 UTF-8。
    pub target: &'static str,
    /// 是否零拷贝（UTF-8 且无 BOM）。
    pub validate_only: bool,
}

impl ConversionBoundary {
    /// 是否真单遍（消费字节数等于源字节数且遍数为 1）。
    pub const fn is_single_pass(&self) -> bool {
        self.passes == 1 && self.bytes_consumed == self.source_bytes
    }
}

/// 转换产物：恒 UTF-8 的正文 + 编码元数据 + 注记（判据四「一次转换到位」）。
pub struct DecodedSource {
    /// 正文（恒为有效 UTF-8，**不含** U+FFFD 替换字符）。
    pub text: String,
    /// 编码诊断元数据。
    pub meta: EncodingMeta,
    /// 转换边界。
    pub boundary: ConversionBoundary,
    /// 显性注记。
    pub notes: Vec<EncodingNote>,
}

impl DecodedSource {
    /// 正文（供 F0403 词法主路直接消费，恒 UTF-8，无需再判编码）。
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// 注记全文（诊断抬头）。
    pub fn note_report(&self) -> String {
        let mut s = String::new();
        for n in self.notes.iter() {
            s.push_str(n.describe().as_str());
            s.push('\n');
        }
        s
    }

    /// 供 F0403 使用的最小入口：只交正文，编码判断到此为止（判据四边界）。
    pub fn into_lexer_input(self) -> String {
        self.text
    }
}

// ---------------------------------------------------------------------------
// 六、UTF-8 序列表与单遍解码
// ---------------------------------------------------------------------------

/// 取 UTF-8 序列长度与第二字节合法区间。
///
/// 返回 `(总长, 第二字节下界, 第二字节上界)`。第二/三/四字节的其余续字节恒为
/// `80..BF`，故只需对第二字节做区间收紧——这正是过���编码与代理项能被区分的
/// 地方（`E0` 第二字节须 `A0..BF`、`ED` 须 `80..9F`、`F0` 须 `90..BF`、
/// `F4` 须 `80..8F`）。`None` = 非法首字节。
fn utf8_seq(lead: u8) -> Option<(usize, u8, u8)> {
    match lead {
        0x00..=0x7F => Some((1, 0x00, 0x00)),
        0xC2..=0xDF => Some((2, 0x80, 0xBF)),
        0xE0 => Some((3, 0xA0, 0xBF)),
        0xE1..=0xEC => Some((3, 0x80, 0xBF)),
        0xED => Some((3, 0x80, 0x9F)),
        0xEE..=0xEF => Some((3, 0x80, 0xBF)),
        0xF0 => Some((4, 0x90, 0xBF)),
        0xF1..=0xF3 => Some((4, 0x80, 0xBF)),
        0xF4 => Some((4, 0x80, 0x8F)),
        _ => None,
    }
}

/// 非法首字节的分类（`utf8_seq` 返回 `None` 时）。
fn classify_bad_lead(index: usize, byte: u8) -> EncodingFault {
    match byte {
        0x80..=0xBF => EncodingFault::UnexpectedContinuation { index, byte },
        0xC0 | 0xC1 => EncodingFault::Overlong { index, byte },
        0xF5..=0xFF => EncodingFault::OutOfRange { index, byte },
        // 理论不可达：0x00..=0x7F 与 0xC2..=0xF4 均由 utf8_seq 正常受理，
        // 0x80..=0xBF / 0xC0..0xC1 / 0xF5..=0xFF 已在上三臂覆盖。
        _ => EncodingFault::OutOfRange { index, byte },
    }
}

/// 第二字节越界时的分类。
///
/// 必须**按越界方向**分，不能只看「是否落在 `80..BF`」——`E0`/`F0` 的紧**下界**
/// 拒的是过短编码（overlong），而 `ED`/`F4` 的紧**上界** 拒的是代理项与越界。
/// 早期版本把两者混为一谈，导致 `F4 90`（真实成因：码位 > U+10FFFF）被误报成
/// 「过长编码」，诊断会把作者引向错误方向。
fn classify_bad_second(index: usize, lead: u8, second: u8, lo: u8, hi: u8) -> EncodingFault {
    if second < lo {
        // 低于紧下界：NUL 等字符被多字节重写了。
        return EncodingFault::Overlong { index, byte: lead };
    }
    if second > hi {
        // 高于紧上界：按首字节区分代理项区与真越界。
        return if lead == 0xED {
            EncodingFault::SurrogateInUtf8 { index, byte: second }
        } else {
            EncodingFault::OutOfRange { index, byte: lead }
        };
    }
    // 理论不可达（调用方已确认 second 落在 lo..=hi 之外）。
    EncodingFault::UnexpectedContinuation { index, byte: second }
}

/// 解码计数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Tally {
    chars: usize,
    bytes: usize,
}

/// UTF-8 正文单遍校验（源已是 UTF-8，故**只校验不重建**，零拷贝）。
///
/// 逐字节推进，命中任一非法形态立即 `Err` 并带字节下标 + 字节值（判据三）。
fn validate_utf8_body(src: &[u8]) -> Result<Tally, EncodingFault> {
    let mut i = 0usize;
    let mut chars = 0usize;
    while i < src.len() {
        let lead = src[i];
        let (len, lo, hi) = match utf8_seq(lead) {
            Some(s) => s,
            None => return Err(classify_bad_lead(i, lead)),
        };
        if len == 1 {
            i += 1;
            chars += 1;
            continue;
        }
        if i + len > src.len() {
            return Err(EncodingFault::TruncatedSequence { index: i, byte: lead, need: len });
        }
        let second = src[i + 1];
        if second < lo || second > hi {
            return Err(classify_bad_second(i, lead, second, lo, hi));
        }
        let mut k = 2usize;
        while k < len {
            let b = src[i + k];
            if !matches!(b, 0x80..=0xBF) {
                return Err(EncodingFault::TruncatedSequence { index: i + k, byte: b, need: len });
            }
            k += 1;
        }
        i += len;
        chars += 1;
    }
    Ok(Tally { chars, bytes: src.len() })
}

/// 由已校验字节取码点（长度 2..4）。
fn utf8_codepoint(src: &[u8], at: usize, len: usize) -> u32 {
    match len {
        2 => ((src[at] as u32 & 0x1F) << 6) | (src[at + 1] as u32 & 0x3F),
        3 => {
            ((src[at] as u32 & 0x0F) << 12)
                | ((src[at + 1] as u32 & 0x3F) << 6)
                | (src[at + 2] as u32 & 0x3F)
        }
        _ => {
            ((src[at] as u32 & 0x07) << 18)
                | ((src[at + 1] as u32 & 0x3F) << 12)
                | ((src[at + 2] as u32 & 0x3F) << 6)
                | (src[at + 3] as u32 & 0x3F)
        }
    }
}

/// UTF-8 → UTF-8 转换（重建路径；BOM 字节在调用前已剥离）。
///
/// 走 `core::str::from_utf8` 之外的显式循环，是为了在失败时给出**字节下标**——
/// 标准库只给 `Utf8Error`，拿不到「哪个字节坏」这一判据三要求的要素。
fn convert_utf8(src: &[u8]) -> Result<(String, Tally), (EncodingFault, usize)> {
    let mut out = String::new();
    let mut i = 0usize;
    let mut chars = 0usize;
    while i < src.len() {
        let lead = src[i];
        let (len, lo, hi) = match utf8_seq(lead) {
            Some(s) => s,
            None => return Err((classify_bad_lead(i, lead), i)),
        };
        if len == 1 {
            out.push(lead as char);
            i += 1;
            chars += 1;
            continue;
        }
        if i + len > src.len() {
            return Err((EncodingFault::TruncatedSequence { index: i, byte: lead, need: len }, i));
        }
        let second = src[i + 1];
        if second < lo || second > hi {
            return Err((classify_bad_second(i, lead, second, lo, hi), i));
        }
        let mut k = 2usize;
        while k < len {
            let b = src[i + k];
            if !matches!(b, 0x80..=0xBF) {
                return Err((EncodingFault::TruncatedSequence { index: i + k, byte: b, need: len }, i));
            }
            k += 1;
        }
        let cp = utf8_codepoint(src, i, len);
        match char::from_u32(cp) {
            Some(c) => out.push(c),
            // 防御分支：`utf8_seq` 的区间已排除全部非法标量，理论不可达。
            None => return Err((EncodingFault::OutOfRange { index: i, byte: lead }, i)),
        }
        i += len;
        chars += 1;
    }
    Ok((out, Tally { chars, bytes: src.len() }))
}

// ---------------------------------------------------------------------------
// 七、宽编码（UTF-16 / UTF-32）→ UTF-8
// ---------------------------------------------------------------------------

/// 读 `u16`（按字节序）。
fn rd_u16(src: &[u8], at: usize, little: bool) -> u16 {
    let a = src[at] as u16;
    let b = src[at + 1] as u16;
    if little {
        a | (b << 8)
    } else {
        (a << 8) | b
    }
}

/// 读 `u32`（按字节序）。
fn rd_u32(src: &[u8], at: usize, little: bool) -> u32 {
    let b0 = src[at] as u32;
    let b1 = src[at + 1] as u32;
    let b2 = src[at + 2] as u32;
    let b3 = src[at + 3] as u32;
    if little {
        b0 | (b1 << 8) | (b2 << 16) | (b3 << 24)
    } else {
        (b0 << 24) | (b1 << 16) | (b2 << 8) | b3
    }
}

/// UTF-16 → UTF-8 单遍转换。
///
/// 代理项规则：`D800..DBFF` 必须紧跟 `DC00..DFFF`，否则**孤立代理项报错**，
/// 绝不合成 U+FFFD（锚点「转换失败→显性失败不替换字符静默」）。
fn convert_utf16(src: &[u8], little: bool) -> Result<(String, Tally), (EncodingFault, usize)> {
    if src.len() % 2 != 0 {
        return Err((EncodingFault::OddUtf16Length { index: src.len() }, src.len()));
    }
    let mut out = String::new();
    let mut i = 0usize;
    let mut chars = 0usize;
    while i < src.len() {
        let unit = rd_u16(src, i, little);
        if (0xD800..0xDC00).contains(&unit) {
            // 高代理项：必须成对。
            if i + 4 > src.len() {
                return Err((EncodingFault::UnpairedHighSurrogate { index: i, unit }, i));
            }
            let low = rd_u16(src, i + 2, little);
            if !(0xDC00..0xE000).contains(&low) {
                return Err((EncodingFault::UnpairedHighSurrogate { index: i, unit }, i));
            }
            let cp = 0x1_0000u32
                + (((unit as u32) - 0xD800) << 10)
                + ((low as u32) - 0xDC00);
            match char::from_u32(cp) {
                Some(c) => out.push(c),
                None => return Err((EncodingFault::OutOfRange { index: i, byte: unit as u8 }, i)),
            }
            i += 4;
            chars += 1;
        } else if (0xDC00..0xE000).contains(&unit) {
            // 低代理项无前导高代理项。
            return Err((EncodingFault::UnpairedLowSurrogate { index: i, unit }, i));
        } else {
            // BMP 常规码元（`char::from_u32` 对全部非代理 BMP 值恒 Some）。
            match char::from_u32(unit as u32) {
                Some(c) => out.push(c),
                None => return Err((EncodingFault::OutOfRange { index: i, byte: unit as u8 }, i)),
            }
            i += 2;
            chars += 1;
        }
    }
    Ok((out, Tally { chars, bytes: src.len() }))
}

/// UTF-32 → UTF-8 单遍转换。
///
/// 越界标量（> U+10FFFF）与落在代理项区（D800..DFFF）的标量一律报错，不静默
/// 替换（锚点错误路径第三条）。
fn convert_utf32(src: &[u8], little: bool) -> Result<(String, Tally), (EncodingFault, usize)> {
    if src.len() % 4 != 0 {
        return Err((EncodingFault::Utf32Length { index: src.len(), len: src.len() }, src.len()));
    }
    let mut out = String::new();
    let mut i = 0usize;
    let mut chars = 0usize;
    while i < src.len() {
        let v = rd_u32(src, i, little);
        if v > 0x10_FFFF {
            return Err((EncodingFault::Utf32Scalar { index: i, value: v }, i));
        }
        if (0xD800..0xE000).contains(&v) {
            return Err((EncodingFault::Utf32Scalar { index: i, value: v }, i));
        }
        match char::from_u32(v) {
            Some(c) => out.push(c),
            None => return Err((EncodingFault::Utf32Scalar { index: i, value: v }, i)),
        }
        i += 4;
        chars += 1;
    }
    Ok((out, Tally { chars, bytes: src.len() }))
}

// ---------------------------------------------------------------------------
// 八、行列定位（仅出错时算一次）
// ---------------------------------------------------------------------------

/// 由字节下标算行列（列单位 = 源编码码元宽）。
///
/// 换行只认 `\n`（CRLF 的 `\r` 计入列宽，不单独算行）——、着色器源的行概念以
/// `\n` 为准，与 F0403 词法器的 `Position::advance` 同口径。
fn line_col_of(raw: &[u8], upto: usize, unit: usize) -> (usize, usize) {
    let end = if upto < raw.len() { upto } else { raw.len() };
    let mut line = 1usize;
    let mut last_nl: Option<usize> = None;
    let mut i = 0usize;
    while i < end {
        if raw[i] == b'\n' {
            line += 1;
            last_nl = Some(i);
        }
        i += 1;
    }
    let col_base = match last_nl {
        Some(p) => p + 1,
        None => 0,
    };
    let col = (end.saturating_sub(col_base) / unit.max(1)) + 1;
    (line, col)
}

// ---------------------------------------------------------------------------
// 九、主入口
// ---------------------------------------------------------------------------

/// 源码编码处理主入口（判据一至判据四的落点）。
///
/// 流程：探测（BOM > 声明 > 假定）→ 剥离 BOM → 单遍转换到 UTF-8 → 产出元数据
/// 与注记。全程零静默：不一致出注记，非法字节出错误码，任何路径都不产出 U+FFFD。
pub fn convert_source(raw: &[u8], cfg: &DecodeConfig) -> Result<DecodedSource, EncodingFault> {
    let probe = EncodingProbe::probe(raw, cfg.declared);
    let mut notes: Vec<EncodingNote> = Vec::new();

    // ---- RequireBom：缺 BOM 即硬门 ----
    if probe.bom == BomKind::None && cfg.policy == AssumePolicy::RequireBom {
        return Err(EncodingFault::MissingBom { head_examined: probe.head_examined });
    }

    // ---- BOM 与声明/假定冲突：以 BOM 为准并告知（判据一）----
    if probe.source == DetectionSource::Bom {
        if let Some(d) = cfg.declared {
            if d != probe.encoding {
                notes.push(EncodingNote::BomOverrides { declared: d, actual: probe.encoding });
            }
        } else if probe.encoding != SourceEncoding::Utf8 {
            notes.push(EncodingNote::BomOverrides {
                declared: SourceEncoding::Utf8,
                actual: probe.encoding,
            });
        }
    }
    if probe.source == DetectionSource::AssumedUtf8 {
        notes.push(EncodingNote::AssumedUtf8);
    }

    // ---- 剥离 BOM ----
    let body: &[u8] = if cfg.strip_bom && probe.bom_len > 0 {
        notes.push(EncodingNote::BomStripped { kind: probe.bom, len: probe.bom_len });
        if probe.bom == BomKind::Utf8 {
            // ZWNBSP 歧义告知：同一串字节既是 BOM 也是合法 U+FEFF。
            notes.push(EncodingNote::ZwspAmbiguity { len: probe.bom_len });
        }
        raw.get(probe.bom_len..).unwrap_or(&[])
    } else {
        raw
    };

    // ---- 单遍转换 ----
    let (text, tally, validate_only) = match probe.encoding {
        SourceEncoding::Utf8 => {
            // 已是 UTF-8：只校验不重建（零拷贝）。strip_bom 时 BOM 已剥离，
            // 正文非空即需重建一次，故零拷贝只在「无 BOM 且整体即合法 UTF-8」时成立。
            if probe.bom_len == 0 || !cfg.strip_bom {
                match validate_utf8_body(body) {
                    Ok(t) => {
                        // 已逐字节校验过 UTF-8，此处 std 复核只为拿整块拷贝。
                        // 复核失败（理论不可达）不 panic、不替换字符，退回重建路径。
                        match core::str::from_utf8(body) {
                            Ok(sl) => (sl.to_owned(), t, true),
                            Err(_) => match convert_utf8(body) {
                                Ok((s2, t2)) => (s2, t2, false),
                                Err((f, _)) => return Err(f),
                            },
                        }
                    }
                    Err(f) => return Err(f),
                }
            } else {
                match convert_utf8(body) {
                    Ok((s, t)) => (s, t, false),
                    Err((f, _)) => return Err(f),
                }
            }
        }
        SourceEncoding::Utf16Le => finish_wide(convert_utf16(body, true))?,
        SourceEncoding::Utf16Be => finish_wide(convert_utf16(body, false))?,
        SourceEncoding::Utf32Le => finish_wide(convert_utf32(body, true))?,
        SourceEncoding::Utf32Be => finish_wide(convert_utf32(body, false))?,
    };

    let boundary = ConversionBoundary {
        bom_stripped: probe.bom_len,
        source_bytes: raw.len(),
        bytes_consumed: tally.bytes + probe.bom_len,
        passes: 1,
        target: "UTF-8",
        validate_only,
    };
    let meta = EncodingMeta {
        encoding: probe.encoding,
        source: probe.source,
        bom_len: probe.bom_len,
        byte_len: raw.len(),
        char_len: tally.chars,
        body_bytes: body.len(),
        validate_only,
    };
    Ok(DecodedSource { text, meta, boundary, notes })
}

/// 宽编码转换结果收口（`Err` 直接上抛，零替换）。
fn finish_wide(
    r: Result<(String, Tally), (EncodingFault, usize)>,
) -> Result<(String, Tally, bool), EncodingFault> {
    match r {
        Ok((s, t)) => Ok((s, t, false)),
        Err((f, _)) => Err(f),
    }
}

/// 转换并把故障渲染成带行列的三要素文本（判据三的对外形态）。
pub fn convert_source_report(raw: &[u8], cfg: &DecodeConfig) -> Result<String, String> {
    match convert_source(raw, cfg) {
        Ok(d) => Ok(d.note_report()),
        Err(f) => {
            // 定位用源编码的码元宽；探测失败时按 UTF-8 假定宽度退回。
            let probe = EncodingProbe::probe(raw, cfg.declared);
            let (line, col) = line_col_of(raw, f.index(), probe.encoding.unit_bytes());
            Err(f.describe(line, col))
        }
    }
}

/// 供 F0403 词法主路消费的最小桥：字节进、UTF-8 正文出。
///
/// F0414 include 展开产物的字节流经此函数进入词法主路；下游不需要知道也不需要
/// 再判断编码（判据四：转换边界一次到位）。
pub fn to_lexer_text(raw: &[u8], cfg: &DecodeConfig) -> Result<String, EncodingFault> {
    convert_source(raw, cfg).map(|d| d.into_lexer_input())
}
