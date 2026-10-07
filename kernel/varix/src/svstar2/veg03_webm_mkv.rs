//! VE-F1203 · WebM/MKV 容器解封装（VE-G 域 · G01 视频解码组 · L1 容器层 · 目标 380 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F1203`
//!
//! **权威源声明**：本模块是 F1203 的**权威实现**。仓库存量
//! `src/system/ve/videoDecode/f1203-webm-mkv-demux.ts` 为 TS 迁移前的半成品
//! （归档会话遗留，未入库），按 Variable 2026-10-07「TS 存量全面迁移」指令，
//! 其对应的既有 TS 模块已由本文件以 Rust 重新实现；TS 文件保留但不再作为
//! 语义依据。下方「与存量 TS 的差异裁决」逐条列出语义分歧及裁决理由。
//!
//! **判据（锚点原文逐条）**：EBML 编码解析（变长整数 VINT / EBML 头 DocType
//! webm-matroska 分流 / 无限长度元素 Unknown-Size）/ Segment-Track-Cluster 结构
//! （TrackEntry 编解码器、时长、默认帧率；Cluster 时间戳基准）/ Block-SimpleBlock
//! 帧（时间戳、关键帧标志、lacing 三模式 Xiph-fixed-EBML 帧拆分）/ 编解码器识别
//! （V_MPEG4-ISO-AVC / V_VP9 / V_AV1 映射解码器注册表 F1058 语义）/ Attachment
//! 附件（字体、封面提取）/ 与 MP4 的差异文档化（F1202 对照）/ 错误路径（EBML 尺寸
//! 溢出拒绝；Cluster 时间戳回跳容错重同步并计数）。判据：EBML 解析正确、Block 提取、
//! 编解码器全测、Attachment、对拍。
//!
//! ## 深度打磨要点（超出锚点字面的部分，逐条说明为什么必须做）
//!
//! ### 一、未知长度元素的**重同步边界**（本模块最关键的一处硬化）
//!
//! 锚点只说「无限长度元素是流式容器的合法形态」。但「延伸至父元素末尾」这句
//! 话在有兄弟元素时会吃错东西：直播 MKV 常见形态是
//! `Segment(unknown) > [Info, Tracks, Cluster(unknown), Cluster(unknown), Cues]`，
//! 若Cluster 一律延伸到 Segment 末尾，**第二个 Cluster 会被第一个吞掉**——
//! 文件仍能「解析成功」，帧数却少一半，且不报任何错。这正是零静默纪律要防的
//! 静默数据缺陷。
//!
//! 本模块的处理：对未知长度的 `Segment` / `Cluster`，先做一次**只读元素头扫描**
//! （不解码payload），在遇到第一个「Segment 级元素」（Cluster / Cues / Info /
//! Tracks / Attachments / Chapters / Tags / SeekHead）时停住，把该处当作本元素
//! 的真实末尾；扫到父末尾仍未遇到则取父末尾。代价是每个未知长度元素多一趟
//! O(元素数) 的头扫描，收益是直播形态与文件形态解析结果一致。
//!
//! 未知长度的**其他** master 元素没有可靠的兄弟边界定义，一律延伸至父末尾并记
//! 诊断——不猜。
//!
//! ### 二、VINT 宽度判定不依赖「移位巧合」
//!
//! 「有效位是否全 1」有两种写法：一种是 `first & (0xff >> width) == 0xff >> width`，
//! 另一种是按宽度显式分派。前者在宽度 8 时 `0xff >> 8` 的结果依赖整数宽度语义
//! （JS 得 0、Rust 中`u8 >> 8` 在 debug 下 panic、release 下被掩码成`>> 0` 得
//! 0xff），三处行为不一致却「看起来都成立」。本模块用
//! [`vint_value_bits_all_set`]：首字节有效位数 `8 - width`，宽度 8 时首字节**没有
//! 有效位**，全 1 判定完全交给后续7 字节。宽度 1/2/8 三形态由自检逐字节验证。
//!
//! ### 三、时间戳换算走整数，不走浮点
//!
//! `timeMs = absTicks * TimecodeScale / 1_000_000` 若用 f64，跨平台与跨优化级别
//! 的舍入不完全一致，回归对拍（判据要求与ffprobe 交叉验证）会出现无法复现的
//! 末位差。本模块用 `u128` 中间量做乘法再整除，并对结果做`u64` 值域检查，
//! 全程无浮点。
//!
//! ### 四、lacing 帧的关键帧标志：**整块继承**（与 ffprobe 对齐）
//!
//! Matroska 规范里关键帧标志是**块级**属性；带 lacing 的块内多帧是同一张画面的
//! 不同 slice/平面，**共享同一时间戳与同一关键帧属性**。因此本模块让块内所有帧
//! 继承块的关键帧标志。存量 TS 把关键帧只给 `frame_in_block == 0` 的那一帧，
//! 与 ffprobe 的 `flags=K` 逐帧输出不一致——这是 TS 侧的一处语义缺陷，本模块按
//! 规范与对拍口径修正，并在此登记分歧。
//!
//! ### 五、EBML lacing 的「全 1 长度」不被当127
//!
//! 长度 VINT 的全 1 形态表示**未知长度**。在 lacing 尺寸表位置读到全 1 VINT 时，
//! 语义是「非法」（帧大小未知），而不是「值 = 127」。存量 TS 直接取`value`，
//! 会把 `0xFF` 静默当成 127 字节帧——畸形文件因此被「成功」解析。本模块对
//! lacing 帧数与帧尺寸**显式拒绝未知长度**。
//!
//! ### 六、Xiph lacing 尺寸累加是真溢出检查
//!
//! 存量 TS 在Xiph 累加循环里写的是 `checkedAdd(size, 0)`——恒真的空断言
//! （加0 不可能溢出），属自证式算术。本模块改为对每次累加做`checked_add`。
//!
//! ## 与存量 TS 的差异裁决（迁移留痕）
//!
//! | 项 | 存量 TS | 本模块 | 裁决理由 |
//! |---|---|---|---|
//! | 未知长度 Cluster 边界 | 延伸至父末尾 | 兄弟 Segment 级元素处重同步 | 前者静默吞掉后续 Cluster（帧数少一半且不报错） |
//! | lacing 帧关键帧 | 仅首帧为true | 整块继承 | 与Matroska 规范及 ffprobe `flags=K` 一致 |
//! | lacing 尺寸全 1 VINT | 当作普通值 | 显性拒绝 | 帧大小未知不是「127字节」 |
//! | Xiph 累加溢出检查 | `checkedAdd(size, 0)` | 每次累加 `checked_add` | 前者恒真，是空断言 |
//! | 时间戳换算 | f64 除法 | u128 中间量整除 | 跨平台可复现，支撑对拍 |
//! | 音频编解码器承接 | 指向 VE-F1207（AV1 视频） | 指向 VE-H 音频引擎域 | F1207 是 AV1 **视频**解码器；音频解码属 H 域，TS 映射是错的 |
//! | 视频编解码器承接 | V_MPEG4→F1204、V_AV1→F1207 等 | 按施工书逐条校对 | 承接单号必须与书内条目一致 |
//!
//! ## 整数纪律（对齐 F1122）
//!
//! 所有由字节流导出的长度／偏移／计数，一律经 `checked_add` / `checked_mul` /
//! `checked_range`；元素 payload 必须完整落在父元素范围内，越界即拒绝。
//! 计数上限集中在 [`MkvLimits`]，数据驱动，防元素炸弹与嵌套炸弹。
//!
//! ## 零静默纪律
//!
//! 畸形（VINT 非法 / 元素越界 / 必需元素缺失 / 未知 DocType / Cluster 时间码回跳 /
//! 未登记编解码器 / lacing 数据不足 / 附件超限）全部产出三要素诊断
//! （[`DiagCode`] + message + hint），不抛异常、不吞诊断；可容错的（时间码回跳、
//! 未登记元素 ID 跳过）计数上报，不可容错的显性拒绝。
//!
//! ## 与 MP4 的差异文档化（锚点要求「F1202 对照」）
//!
//! 见 [`FORMAT_DIFFERENCES`]：时间戳粒度 / 索引方式 / lacing 有无 三维逐项对照。
//! 该表是**数据表而非注释**，自检拿它跟**实测解析结果**对撞（不是拿表内元素验表内
//! 元素——那是恒真的弱门禁）。
//!
//! ## 对拍（ffprobe 交叉验证）
//!
//! [`cross_check`] 接受一份外部探测事实（`FfprobeFacts`，即 ffprobe 侧给出的
//! 容器/流事实），与本模块的解析结果逐字段对撞并给出**分歧清单**。语料为
//! 固定字节构造的合成 MKV，外部事实以 fixture 形式在本文件内登记；接真实
//! ffprobe 时只需替换 facts 来源。分歧清单非空即判红——这是「对拍」在离线
//! 自检里的可执行形态。
//!
//! 零 IO、零墙钟、类型自持（不 import 未注册的兄弟模块）；跨平台逐位可复现。

use crate::checks::CheckSet;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ════════════════════════════════════════════════════════════════════════════
// §1 诊断与结果类型（零静默基础设施；与 F1201/F1202 同纪律，各自独立实现）
// ════════════════════════════════════════════════════════════════════════════

/// 诊断码：每种畸形独立可检索。处置方向不同的状态不共用码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DiagCode {
    /// VINT 编码非法（首字节0x00、宽度超限、或被截断）。
    VintInvalid,
    /// 元素长度越界（超出父元素或文件末尾）。
    ElementOutOfRange,
    /// 元素 ID 未登记（宽容跳过，非致命）。
    IdUnknown,
    /// 必需的顶层元素缺失（缺 Segment 即无轨道）。
    SegmentMissing,
    /// 必需的子元素缺失（如Tracks 内无 TrackEntry）。
    ChildMissing,
    /// EBML 头非法（版本号不在支持范围）。
    HeaderInvalid,
    /// 整数溢出（字节流导出的数值超出值域）。
    ArithmeticOverflow,
    /// 元素数／嵌套深度超过计数上限（防炸弹）。
    CountLimitExceeded,
    /// 编解码器 ID 未登记（显性不支持，不静默送入解码器）。
    CodecUnsupported,
    /// Cluster／Block 时间码回跳（容错重同步，已计数）。
    TimecodeRegression,
    /// lacing 数据不足（声明帧数与字节数矛盾）。
    LacingInsufficientData,
    /// Block 引用的轨道不存在。
    TrackNotFound,
    /// Block 数据在文件内越界。
    BlockOutOfRange,
    /// 文件既非 WebM 也非 Matroska（DocType 不匹配）。
    DocTypeUnsupported,
    /// 字符串字段不是合法 UTF-8（已取合法前缀，诊断留痕）。
    StringInvalidUtf8,
    /// 未知长度元素出现在无可靠兄弟边界的 master 内（延伸父末尾并留痕）。
    UnknownSizeUnbounded,
    /// 判据自检不通过。
    CriterionSelfCheckFailed,
}

/// 诊断码的稳定字符串名（诊断台账与 grep 可检索）。
pub const fn diag_name(code: DiagCode) -> &'static str {
    match code {
        DiagCode::VintInvalid => "EBML_VINT_INVALID",
        DiagCode::ElementOutOfRange => "EBML_ELEMENT_OUT_OF_RANGE",
        DiagCode::IdUnknown => "EBML_ID_UNKNOWN",
        DiagCode::SegmentMissing => "EBML_SEGMENT_MISSING",
        DiagCode::ChildMissing => "EBML_CHILD_MISSING",
        DiagCode::HeaderInvalid => "EBML_HEADER_INVALID",
        DiagCode::ArithmeticOverflow => "EBML_ARITHMETIC_OVERFLOW",
        DiagCode::CountLimitExceeded => "EBML_COUNT_LIMIT_EXCEEDED",
        DiagCode::CodecUnsupported => "MKV_CODEC_UNSUPPORTED",
        DiagCode::TimecodeRegression => "MKV_TIMECODE_REGRESSION",
        DiagCode::LacingInsufficientData => "MKV_LACING_INSUFFICIENT_DATA",
        DiagCode::TrackNotFound => "MKV_TRACK_NOT_FOUND",
        DiagCode::BlockOutOfRange => "MKV_BLOCK_OUT_OF_RANGE",
        DiagCode::DocTypeUnsupported => "MKV_DOCTYPE_UNSUPPORTED",
        DiagCode::StringInvalidUtf8 => "MKV_STRING_INVALID_UTF8",
        DiagCode::UnknownSizeUnbounded => "MKV_UNKNOWN_SIZE_UNBOUNDED",
        DiagCode::CriterionSelfCheckFailed => "CRITERION_SELFCHECK_FAILED",
    }
}

/// 一条诊断：发生了什么（code+message）、影响什么、下一步怎么办（hint）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Diagnostic {
    pub code: DiagCode,
    pub message: String,
    pub hint: String,
}

/// 内部失败载体：三要素齐备。
///
/// 刻意**不**用 `Outcome<()>` 之类的类型承载它——那会撞 core 的
/// `impl<T> From<T> for T`（E0119）。这里用独立 struct，内部函数返回
/// `Result<T, Failure>`，故可自由使用 `?`。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Failure {
    pub code: DiagCode,
    pub message: String,
    pub hint: String,
}

impl Failure {
    pub fn new(code: DiagCode, message: String, hint: &str) -> Failure {
        Failure { code, message, hint: hint.to_string() }
    }

    /// 诊断三要素视图（供诊断袋回填）。
    pub fn to_diagnostic(&self) -> Diagnostic {
        Diagnostic { code: self.code, message: self.message.clone(), hint: self.hint.clone() }
    }
}

/// 对外结果判别联合：成功必带 value 与诊断袋，失败必带 code/message/hint。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Outcome<T> {
    Ok { value: T, diagnostics: Vec<Diagnostic> },
    Err { failure: Failure, diagnostics: Vec<Diagnostic> },
}

impl<T> Outcome<T> {
    /// 成功构造。
    pub fn ok(value: T, diagnostics: Vec<Diagnostic>) -> Outcome<T> {
        Outcome::Ok { value, diagnostics }
    }

    /// 失败构造。
    pub fn err(failure: Failure, diagnostics: Vec<Diagnostic>) -> Outcome<T> {
        Outcome::Err { failure, diagnostics }
    }

    /// 取值（失败时消耗 `self` 返回兜底值——按值消费，不强加 `T: Clone`）。
    pub fn value_or(self, fallback: T) -> T {
        match self {
            Outcome::Ok { value, .. } => value,
            Outcome::Err { .. } => fallback,
        }
    }

    /// 是否成功。
    pub fn is_ok(&self) -> bool {
        matches!(self, Outcome::Ok { .. })
    }

    /// 诊断列表（成功与失败都带——失败时含三要素本身）。
    pub fn diagnostics(&self) -> &[Diagnostic] {
        match self {
            Outcome::Ok { diagnostics, .. } => diagnostics,
            Outcome::Err { failure, diagnostics } => {
                // 失败时若三要素未入袋则补入，保证「失败必留痕」。
                if diagnostics.iter().any(|d| d.code == failure.code && d.message == failure.message) {
                    diagnostics
                } else {
                    // 借用层无法返回临时列表，故失败路径在构造时已保证入袋；
                    // 此处仅为可读性保留分支。
                    diagnostics
                }
            }
        }
    }
}

/// 诊断袋：累积非致命发现（宽容跳过的元素、容错重同步等）。
#[derive(Clone, Default, Debug)]
pub struct DiagBag {
    items: Vec<Diagnostic>,
}

impl DiagBag {
    pub fn new() -> DiagBag {
        DiagBag { items: Vec::new() }
    }

    /// 记一条非致命诊断。
    pub fn push(&mut self, code: DiagCode, message: String, hint: &str) {
        self.items.push(Diagnostic { code, message, hint: hint.to_string() });
    }

    /// 记一条致命诊断（同时作为失败三要素留痕）。
    pub fn push_failure(&mut self, f: &Failure) {
        self.items.push(f.to_diagnostic());
    }

    /// 取全部诊断。
    pub fn all(&self) -> &[Diagnostic] {
        &self.items
    }

    /// 诊断条数。
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 无诊断。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 某诊断码的出现次数（自检按码点名核对用）。
    pub fn count_of(&self, code: DiagCode) -> usize {
        self.items.iter().filter(|d| d.code == code).count()
    }
}

/// 解析统计：宽容处理与容错重同步的**计数**（不回退为「无异常」）。
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct ParseStats {
    /// 跳过的未登记元素 ID 数。
    pub skipped_unknown_ids: u64,
    /// 未知长度元素数。
    pub unknown_size_elements: u64,
    /// 时间码回跳次数（容错重同步）。
    pub timecode_regressions: u64,
    /// 解析出的元素总数。
    pub elements_seen: u64,
}

// ════════════════════════════════════════════════════════════════════════════
// §2 整数安全算术 + 计数上限（F1122 本地落地面）
// ════════════════════════════════════════════════════════════════════════════

/// 加法：溢出即 `None`（调用方必须显式处理，不得当 0 用）。
pub fn checked_add(a: u64, b: u64) -> Option<u64> {
    a.checked_add(b)
}

/// 乘法：溢出即 `None`。
pub fn checked_mul(a: u64, b: u64) -> Option<u64> {
    a.checked_mul(b)
}

/// 区间校验：`[offset, offset+size)` 是否完整落在 `[0, limit)` 内。
///
/// 端点用加法前先判`offset <= limit`——否则 `offset + size` 自身的溢出
/// 会被 `checked_add` 挡下，但「offset 已在limit 之外」这一事实会丢失，
/// 产生「越界被误判为合法」的漏洞。
pub fn checked_range(offset: u64, size: u64, limit: u64) -> bool {
    if offset > limit || size > limit {
        return false;
    }
    match checked_add(offset, size) {
        Some(end) => end <= limit,
        None => false,
    }
}

/// 每格式合理计数上限（数据驱动，F1122 要求）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MkvLimits {
    /// 单层子元素数上限（防元素炸弹）。
    pub child_count: usize,
    /// 元素嵌套深度上限（MKV 合法不超过 5 层）。
    pub depth: u32,
    /// Cluster 数上限。
    pub cluster_count: usize,
    /// 轨道数上限。
    pub track_count: usize,
    /// 单轨 Block 数上限。
    pub block_count: usize,
    /// 单个 Block 内 lacing 帧数上限。
    pub lacing_frame_count: u32,
    /// Attachment 数上限。
    pub attachment_count: usize,
    /// 时间码换算结果的上界（毫秒；约 1193 小时，够任何真实媒体）。
    pub max_time_ms: u64,
}

/// 生产上限。
pub const MKV_LIMITS: MkvLimits = MkvLimits {
    child_count: 200_000,
    depth: 16,
    cluster_count: 1_000_000,
    track_count: 64,
    block_count: 5_000_000,
    lacing_frame_count: 256,
    attachment_count: 256,
    max_time_ms: 4_294_000_000,
};

/// 内部统一的 Result 别名（`?` 可用；不暴露给 crate 外）。
type R<T> = Result<T, Failure>;

/// 溢出失败构造。
fn overflow_failure(what: &str) -> Failure {
    Failure::new(
        DiagCode::ArithmeticOverflow,
        format!("{} 溢出 u64 值域", what),
        "字节流声明的数值超出可表示范围；拒绝解析（F1122 纪律）",
    )
}

/// 越界失败构造。
fn range_failure(what: &str, offset: u64, size: u64, limit: u64) -> Failure {
    Failure::new(
        DiagCode::ElementOutOfRange,
        format!("{} 越界：[{},+{}) 超出上界 {}", what, offset, size, limit),
        "声明长度超出父元素；这是损坏文件或恶意构造的典型特征，拒绝解析",
    )
}

// ════════════════════════════════════════════════════════════════════════════
// §3 EBML 元素 ID 登记表与 VINT 变长整数（EBML 编码基础）
// ════════════════════════════════════════════════════════════════════════════

/// EBML 元素 ID（Matroska 规范核心子集；`Segment` 级元素另见 [`SEGMENT_LEVEL_IDS`]）。
pub mod ids {
    pub const EBML: u32 = 0x1a45dfa3;
    pub const EBML_VERSION: u32 = 0x4286;
    pub const EBML_READ_VERSION: u32 = 0x42f7;
    pub const EBML_MAX_ID_LENGTH: u32 = 0x42f2;
    pub const EBML_MAX_SIZE_LENGTH: u32 = 0x42f3;
    pub const DOC_TYPE: u32 = 0x4282;
    pub const DOC_TYPE_VERSION: u32 = 0x4287;
    pub const DOC_TYPE_READ_VERSION: u32 = 0x4285;
    pub const SEGMENT: u32 = 0x18538067;
    pub const SEEK_HEAD: u32 = 0x114d9b74;
    pub const INFO: u32 = 0x1549a966;
    pub const TIMECODE_SCALE: u32 = 0x2ad7b1;
    pub const DURATION: u32 = 0x4489;
    pub const MUXING_APP: u32 = 0x4d80;
    pub const WRITING_APP: u32 = 0x5741;
    pub const TRACKS: u32 = 0x1654ae6b;
    pub const TRACK_ENTRY: u32 = 0xae;
    pub const TRACK_NUMBER: u32 = 0xd7;
    pub const TRACK_UID: u32 = 0x73c5;
    pub const TRACK_TYPE: u32 = 0x83;
    pub const CODEC_ID: u32 = 0x86;
    pub const DEFAULT_DURATION: u32 = 0x23e383;
    pub const VIDEO: u32 = 0xe0;
    pub const PIXEL_WIDTH: u32 = 0xb0;
    pub const PIXEL_HEIGHT: u32 = 0xba;
    pub const DISPLAY_WIDTH: u32 = 0x54b0;
    pub const DISPLAY_HEIGHT: u32 = 0x54ba;
    pub const AUDIO: u32 = 0xe1;
    pub const SAMPLING_FREQUENCY: u32 = 0xb5;
    pub const OUTPUT_SAMPLING_FREQUENCY: u32 = 0x78b5;
    pub const CHANNELS: u32 = 0x9f;
    pub const BIT_DEPTH: u32 = 0x6264;
    pub const CLUSTER: u32 = 0x1f43b675;
    pub const TIMECODE: u32 = 0xe7;
    pub const POSITION: u32 = 0xa7;
    pub const PREV_SIZE: u32 = 0xab;
    pub const SIMPLE_BLOCK: u32 = 0xa3;
    pub const BLOCK_GROUP: u32 = 0xa0;
    pub const BLOCK: u32 = 0xa1;
    pub const BLOCK_DURATION: u32 = 0x9b;
    pub const REFERENCE_BLOCK: u32 = 0xfb;
    pub const CUES: u32 = 0x1c53bb6b;
    pub const CUE_POINT: u32 = 0xbb;
    pub const CUE_TIME: u32 = 0xb3;
    pub const CUE_TRACK_POSITIONS: u32 = 0xb7;
    pub const CUE_TRACK: u32 = 0xf7;
    pub const CUE_CLUSTER_POSITION: u32 = 0xf1;
    pub const ATTACHMENTS: u32 = 0x1941a469;
    pub const ATTACHED_FILE: u32 = 0x61a7;
    pub const FILE_DESCRIPTION: u32 = 0x467e;
    pub const FILE_NAME: u32 = 0x466e;
    pub const FILE_MIME_TYPE: u32 = 0x4660;
    pub const FILE_DATA: u32 = 0x465c;
    pub const FILE_UID: u32 = 0x46ae;
    pub const CHAPTERS: u32 = 0x1043a770;
    pub const TAGS: u32 = 0x1254c367;
}

/// Segment 级元素 ID：未知长度元素的**重同步边界**（见模块头注「深度打磨要点一」）。
///
/// 这些 ID 出现在 Segment 的直接子层。未知长度的 Cluster 扫到其中任一个即停。
pub const SEGMENT_LEVEL_IDS: &[u32] = &[
    ids::SEEK_HEAD,
    ids::INFO,
    ids::TRACKS,
    ids::CUES,
    ids::CLUSTER,
    ids::ATTACHMENTS,
    ids::CHAPTERS,
    ids::TAGS,
];

/// 已登记的全部元素 ID（诊断与「未登记」判定的唯一源）。
pub const KNOWN_IDS: &[u32] = &[
    ids::EBML,
    ids::EBML_VERSION,
    ids::EBML_READ_VERSION,
    ids::EBML_MAX_ID_LENGTH,
    ids::EBML_MAX_SIZE_LENGTH,
    ids::DOC_TYPE,
    ids::DOC_TYPE_VERSION,
    ids::DOC_TYPE_READ_VERSION,
    ids::SEGMENT,
    ids::SEEK_HEAD,
    ids::INFO,
    ids::TIMECODE_SCALE,
    ids::DURATION,
    ids::MUXING_APP,
    ids::WRITING_APP,
    ids::TRACKS,
    ids::TRACK_ENTRY,
    ids::TRACK_NUMBER,
    ids::TRACK_UID,
    ids::TRACK_TYPE,
    ids::CODEC_ID,
    ids::DEFAULT_DURATION,
    ids::VIDEO,
    ids::PIXEL_WIDTH,
    ids::PIXEL_HEIGHT,
    ids::DISPLAY_WIDTH,
    ids::DISPLAY_HEIGHT,
    ids::AUDIO,
    ids::SAMPLING_FREQUENCY,
    ids::OUTPUT_SAMPLING_FREQUENCY,
    ids::CHANNELS,
    ids::BIT_DEPTH,
    ids::CLUSTER,
    ids::TIMECODE,
    ids::POSITION,
    ids::PREV_SIZE,
    ids::SIMPLE_BLOCK,
    ids::BLOCK_GROUP,
    ids::BLOCK,
    ids::BLOCK_DURATION,
    ids::REFERENCE_BLOCK,
    ids::CUES,
    ids::CUE_POINT,
    ids::CUE_TIME,
    ids::CUE_TRACK_POSITIONS,
    ids::CUE_TRACK,
    ids::CUE_CLUSTER_POSITION,
    ids::ATTACHMENTS,
    ids::ATTACHED_FILE,
    ids::FILE_DESCRIPTION,
    ids::FILE_NAME,
    ids::FILE_MIME_TYPE,
    ids::FILE_DATA,
    ids::FILE_UID,
    ids::CHAPTERS,
    ids::TAGS,
];

/// 元素 ID 是否已登记。
pub fn is_known_id(id: u32) -> bool {
    KNOWN_IDS.contains(&id)
}

/// 元素 ID 是否为 Segment 级（重同步边界）。
pub fn is_segment_level(id: u32) -> bool {
    SEGMENT_LEVEL_IDS.contains(&id)
}

/// master 元素（含子元素）——只有这些需要递归。
pub fn is_master_id(id: u32) -> bool {
    matches!(
        id,
        ids::EBML
            | ids::SEGMENT
            | ids::INFO
            | ids::TRACKS
            | ids::TRACK_ENTRY
            | ids::VIDEO
            | ids::AUDIO
            | ids::CLUSTER
            | ids::BLOCK_GROUP
            | ids::ATTACHMENTS
            | ids::ATTACHED_FILE
            | ids::SEEK_HEAD
            | ids::CUES
            | ids::CUE_POINT
            | ids::CUE_TRACK_POSITIONS
    )
}

/// 未知长度的 master 是否有可靠的兄弟边界定义。
///
/// - `Cluster`：有。其在 Segment 层的兄弟（Segment 级元素）都是可靠的结束标志。
/// - `Segment`：有，但边界**只认另一个 `Segment` ID**——`Info` / `Tracks` /
///   `Cluster` / `Cues` 全都是 Segment 自己的合法子元素，绝不能当结束标志。
///   （若不加这个区分，未知长度的 Segment 会被自己的第一个 Cluster 截断。）
pub fn resync_boundary_ids(id: u32) -> &'static [u32] {
    if id == ids::CLUSTER {
        SEGMENT_LEVEL_IDS
    } else if id == ids::SEGMENT {
        // 顶层只以「下一个 Segment」为界。
        SEGMENT_BOUNDARY_IDS
    } else {
        EMPTY_IDS
    }
}

/// 未知长度的 master 是否需要重同步边界扫描。
pub fn has_resync_boundary(id: u32) -> bool {
    matches!(id, ids::SEGMENT | ids::CLUSTER)
}

/// Segment 的边界 ID 集合（仅「另一个 Segment」）。
pub const SEGMENT_BOUNDARY_IDS: &[u32] = &[ids::SEGMENT];
/// 空边界集合。
pub const EMPTY_IDS: &[u32] = &[];

/// VINT（变长整数）解码结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Vint {
    /// 解出的数值（元素 ID 保留标记位；长度去掉标记位）。
    pub value: u64,
    /// 编码宽度（字节数，1~8）。
    pub width: u8,
    /// 长度 VINT 的「全1 有效位」形态 = 未知长度。
    pub unknown: bool,
}

/// 由首字节前导零个数求 VINT 宽度。
///
/// 首字节 `0x00` 无定义（返回 9，调用方按超限拒绝）。
pub const fn vint_width_from_first(first: u8) -> u8 {
    let mut width = 1u8;
    let mut mask = 0x80u8;
    while mask > 0 && (first & mask) == 0 {
        width += 1;
        mask >>= 1;
    }
    width
}

/// 首字节里**有效位**的个数（宽度 8 时为 0——首字节只有标记位）。
pub const fn vint_head_value_bits(width: u8) -> u8 {
    8 - width
}

/// 「有效位是否全 1」判定，**按宽度显式分派**，不依赖 `0xff >> width` 的移位语义。
///
/// -宽度 1~7：首字节低 `8-width` 位必须全 1，且后续 `width-1` 字节全为 `0xFF`；
/// - 宽度 8：首字节**没有有效位**，全 1 判定完全由后续 7 字节决定。
pub fn vint_value_bits_all_set(first: u8, tail: &[u8], width: u8) -> bool {
    let head_bits = vint_head_value_bits(width);
    if head_bits > 0 {
        let mask = (1u16 << head_bits) - 1;
        if (first as u16 & mask) != mask {
            return false;
        }
    }
    tail.iter().all(|b| *b == 0xff)
}

/// 取首字节的有效位部分（宽度 8 时为 0）。
pub const fn vint_head_value(first: u8, width: u8) -> u8 {
    let head_bits = vint_head_value_bits(width);
    if head_bits == 0 {
        0
    } else {
        first & (((1u16 << head_bits) - 1) as u8)
    }
}

/// 解码 VINT——EBML 的地基。
///
/// 两类 VINT：
/// - **元素 ID**：宽度 1~4，**保留标记位**（ID 不能全 0，否则与「长度 0」混淆）；
/// - **元素长度**：宽度 1~8，去掉标记位；有效位全 1 表示**未知长度**——这是流式
///   容器的合法形态（`Segment` / `Cluster` 常用），解析时须延伸至父元素末尾。
pub fn decode_vint(data: &[u8], offset: u64, limit: u64, max_width: u8, keep_marker: bool) -> R<Vint> {
    if offset >= limit {
        return Err(Failure::new(
            DiagCode::VintInvalid,
            format!("偏移 {} 处无可读字节（limit={}）", offset, limit),
            "VINT 至少需要 1 字节；此处已到容器末尾，元素被截断",
        ));
    }
    if offset >= data.len() as u64 {
        return Err(Failure::new(
            DiagCode::VintInvalid,
            format!("偏移 {} 超出实际字节数 {}", offset, data.len()),
            "声明的解析范围超出缓冲区；拒绝解析",
        ));
    }
    let first = data[offset as usize];
    if first == 0 {
        return Err(Failure::new(
            DiagCode::VintInvalid,
            format!("偏移 {} 处 VINT 首字节为 0x00", offset),
            "0x00 不是合法 VINT 起点（全零宽度未定义）；文件损坏或此处不是元素边界",
        ));
    }
    let width = vint_width_from_first(first);
    if width > max_width {
        return Err(Failure::new(
            DiagCode::VintInvalid,
            format!("偏移 {} 处 VINT 宽度 {} 超过允许上限 {}", offset, width, max_width),
            "更宽说明字节序判断或元素边界定位有误；此处不应是合法的 ID/长度",
        ));
    }
    if !checked_range(offset, width as u64, limit) {
        return Err(Failure::new(
            DiagCode::VintInvalid,
            format!("偏移 {} 处 VINT 宽度 {} 越界（limit={}）", offset, width, limit),
            "元素被截断；请核对文件长度与父元素范围",
        ));
    }
    let w = width as usize;
    let tail = &data[offset as usize + 1..offset as usize + w];

    // 累加：首字节贡献有效位（ID 保留标记位），后续字节按大端权重全取。
    //
    // **首字节必须左移 `8*(width-1)`**——它是大端序列的最高有效字节。
    // 漏掉这一移位会让所有 width≥2 的 VINT 数值静默错位（例如两字节长度
    // VINT `42 86` 会被读成 0x02+0x86=0x88=136，而正确值是 0x0286=646），
    // 元素边界随即全部错位——这是最难察觉的一类半对错。
    let head_shift = 8 * (width as u32 - 1);
    let head_value: u64 = if keep_marker {
        first as u64
    } else {
        vint_head_value(first, width) as u64
    };
    let mut value: u64 = match checked_mul(head_value, 1u64 << head_shift) {
        Some(v) => v,
        None => return Err(overflow_failure("VINT 首字节移位")),
    };
    for (i, byte) in tail.iter().enumerate() {
        // tail[0] 位于大端序列的第 `width-2` 字节位（首字节已占第 `width-1` 位）。
        // 写成 `width-1-i` 会让每个后续字节都高移一个字节位——宽度 4 的 ID
        // `1A 45 DF A3` 会被读成 `0x5FDFA300`，元素 ID 全表失配。
        let shift = 8 * (width as u32 - 2 - i as u32);
        let part = match checked_mul(*byte as u64, 1u64 << shift) {
            Some(p) => p,
            None => return Err(overflow_failure("VINT 字节权重")),
        };
        value = match checked_add(value, part) {
            Some(v) => v,
            None => return Err(overflow_failure("VINT 数值累加")),
        };
    }

    // 未知长度：有效位全 1（仅长度 VINT 有此语义）。
    let unknown = !keep_marker && vint_value_bits_all_set(first, tail, width);
    Ok(Vint { value, width, unknown })
}

/// 元素头（ID + 长度）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ElementHeader {
    pub id: u32,
    /// ID 编码宽度（1~4）。
    pub id_length: u8,
    ///长度编码宽度（1~8）。
    pub size_length: u8,
    /// payload 起点（绝对偏移）。
    pub data_offset: u64,
    /// payload 长度（未知长度已按父末尾解析为具体值）。
    pub data_size: u64,
    /// 该元素是否以「未知长度」编码。
    pub size_unknown: bool,
}

impl ElementHeader {
    /// payload 绝对末尾。
    pub fn payload_end(&self) -> Option<u64> {
        checked_add(self.data_offset, self.data_size)
    }
}

/// 读一个元素头（ID + 长度），并校验 payload 完整落在 `parent_end` 内。
pub fn read_element_header(data: &[u8], offset: u64, parent_end: u64) -> R<ElementHeader> {
    let id = decode_vint(data, offset, parent_end, 4, true)?;
    // ID 值必须落在 u32（登记表的宽度上限即 4 字节）。
    if id.value > u32::MAX as u64 {
        return Err(Failure::new(
            DiagCode::VintInvalid,
            format!("偏移 {} 处元素 ID 0x{:x} 超出 u32", offset, id.value),
            "EBML 元素 ID 至多 4 字节；更宽的 ID 不是合法 Matroska 元素",
        ));
    }
    let size_offset = match checked_add(offset, id.width as u64) {
        Some(v) => v,
        None => return Err(overflow_failure("ID 偏移推进")),
    };
    let size = decode_vint(data, size_offset, parent_end, 8, false)?;
    let data_offset = match checked_add(size_offset, size.width as u64) {
        Some(v) => v,
        None => return Err(overflow_failure("长度偏移推进")),
    };
    if data_offset > parent_end {
        return Err(range_failure("元素头", offset, data_offset - offset, parent_end));
    }
    // 未知长度 → 延伸至父元素末尾（流式容器的合法形态）。
    let data_size = if size.unknown {
        match checked_add(parent_end, 0).and_then(|p| p.checked_sub(data_offset)) {
            Some(v) => v,
            None => return Err(overflow_failure("未知长度元素的 payload 长度")),
        }
    } else {
        size.value
    };
    if !checked_range(data_offset, data_size, parent_end) {
        return Err(range_failure("元素 payload", data_offset, data_size, parent_end));
    }
    Ok(ElementHeader {
        id: id.value as u32,
        id_length: id.width,
        size_length: size.width,
        data_offset,
        data_size,
        size_unknown: size.unknown,
    })
}

/// 一个 EBML 元素的解析结果。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct EbmlElement {
    pub id: u32,
    pub id_length: u8,
    pub data_offset: u64,
    pub data_size: u64,
    pub size_unknown: bool,
    pub children: Vec<EbmlElement>,
}

impl EbmlElement {
    /// payload 绝对末尾。
    pub fn payload_end(&self) -> Option<u64> {
        checked_add(self.data_offset, self.data_size)
    }

    /// 元素总长度（含头）。
    pub fn total_size(&self) -> Option<u64> {
        self.payload_end().and_then(|e| e.checked_sub(self.data_offset))
    }
}

/// 解析上下文：字节源 + 诊断袋 + 统计（避免三参数在每层传递）。
pub struct Ctx<'a> {
    pub data: &'a [u8],
    pub bag: DiagBag,
    pub stats: ParseStats,
    pub limits: MkvLimits,
}

impl<'a> Ctx<'a> {
    pub fn new(data: &'a [u8]) -> Ctx<'a> {
        Ctx { data, bag: DiagBag::new(), stats: ParseStats::default(), limits: MKV_LIMITS }
    }

    /// 限上限定的上下文（自检用小上限驱动限额路径）。
    pub fn with_limits(data: &'a [u8], limits: MkvLimits) -> Ctx<'a> {
        Ctx { data, bag: DiagBag::new(), stats: ParseStats::default(), limits }
    }

    /// 文件长度（u64 视图）。
    pub fn len(&self) -> u64 {
        self.data.len() as u64
    }

    /// 文件是否为空。
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// 取一个字节（越界即 `None`，不用索引 panic）。
    pub fn byte(&self, at: u64) -> Option<u8> {
        if at < self.data.len() as u64 {
            Some(self.data[at as usize])
        } else {
            None
        }
    }

    /// 取一段字节切片（越界即 `None`）。
    pub fn slice(&self, at: u64, len: u64) -> Option<&'a [u8]> {
        if !checked_range(at, len, self.len()) {
            return None;
        }
        let start = at as usize;
        let end = match checked_add(at, len) {
            Some(e) => e as usize,
            None => return None,
        };
        self.data.get(start..end)
    }
}

/// 未知长度元素的真实末尾：只读元素头扫描，遇边界元素即停。
///
/// 不解码 payload，故不受 payload 内任意字节影响；扫描本身同样走
/// [`read_element_header`]，头部非法即显性失败（不猜）。
///
/// `boundary` 由 [`resync_boundary_ids`] 按元素 ID 给出——**Cluster 与 Segment
/// 的边界集合不同**，混用会截断文件（见该函数说明）。
pub fn resync_end(ctx: &mut Ctx, start: u64, parent_end: u64, boundary: &[u32]) -> R<u64> {
    let mut cursor = start;
    let mut scanned: u64 = 0;
    while cursor < parent_end {
        if scanned >= ctx.limits.child_count as u64 {
            return Err(Failure::new(
                DiagCode::CountLimitExceeded,
                format!("未知长度元素的重同步扫描超过 {} 个元素", ctx.limits.child_count),
                "无边界可寻的巨型元素；拒绝解析",
            ));
        }
        let h = read_element_header(ctx.data, cursor, parent_end)?;
        if boundary.contains(&h.id) {
            return Ok(cursor);
        }
        let end = match h.payload_end() {
            Some(e) => e,
            None => return Err(overflow_failure("重同步扫描的元素末尾")),
        };
        if end <= cursor {
            return Err(Failure::new(
                DiagCode::ElementOutOfRange,
                format!("重同步扫描在偏移 {} 处未推进游标", cursor),
                "零长度或负长度元素会造成死循环；拒绝解析",
            ));
        }
        if h.size_unknown && !has_resync_boundary(h.id) {
            // 未知长度的非 Segment/Cluster 元素：无可靠兄弟边界，延伸父末尾。
            ctx.bag.push(
                DiagCode::UnknownSizeUnbounded,
                format!(
                    "元素 0x{:x} 在偏移 {} 处为未知长度且无可靠兄弟边界，延伸至父末尾 {}",
                    h.id, cursor, parent_end
                ),
                "该 master 元素在未知长度形态下无标准边界定义；已按父末尾处理并留痕，不做猜测",
            );
            ctx.stats.unknown_size_elements += 1;
            return Ok(parent_end);
        }
        cursor = end;
        scanned += 1;
    }
    Ok(parent_end)
}

/// 解析同一父元素下的连续子元素。
///
/// 遇到未登记 ID 时记诊断并按长度跳过（**宽容模式**），不中断整树解析——
/// 因为长度已知故可安全越过。真正越界／非法则显性拒绝。
pub fn parse_children(ctx: &mut Ctx, start: u64, end: u64, depth: u32) -> R<Vec<EbmlElement>> {
    if depth > ctx.limits.depth {
        return Err(Failure::new(
            DiagCode::CountLimitExceeded,
            format!("元素嵌套深度超过 {}（起始 offset={}）", ctx.limits.depth, start),
            "MKV 合法嵌套不超过 5 层；深度异常通常是恶意构造的递归元素",
        ));
    }
    let mut out: Vec<EbmlElement> = Vec::new();
    let mut cursor = start;
    while cursor < end {
        if out.len() >= ctx.limits.child_count {
            return Err(Failure::new(
                DiagCode::CountLimitExceeded,
                format!("单层子元素数超过上限 {}", ctx.limits.child_count),
                "元素炸弹的典型特征（海量微型元素）；拒绝解析",
            ));
        }
        let h = read_element_header(ctx.data, cursor, end)?;
        ctx.stats.elements_seen += 1;
        if h.size_unknown {
            ctx.stats.unknown_size_elements += 1;
        }

        // 元素真实末尾：未知长度且有边界语义者走重同步扫描。
        let mut effective_end = match h.payload_end() {
            Some(v) => v,
            None => return Err(overflow_failure("元素 payload 末尾")),
        };
        if h.size_unknown && has_resync_boundary(h.id) {
            effective_end = resync_end(ctx, h.data_offset, effective_end, resync_boundary_ids(h.id))?;
        }
        if effective_end < h.data_offset {
            return Err(range_failure("重同步后的元素范围", h.data_offset, 0, end));
        }

        let mut children: Vec<EbmlElement> = Vec::new();
        if is_master_id(h.id) {
            children = parse_children(ctx, h.data_offset, effective_end, depth + 1)?;
        } else if !is_known_id(h.id) {
            ctx.bag.push(
                DiagCode::IdUnknown,
                format!(
                    "跳过未登记的元素 ID 0x{:x}（offset={}，长度 {}）",
                    h.id, cursor, h.data_size
                ),
                "该元素未在 KNOWN_IDS 登记；其长度已知故可安全跳过。若它承载必要数据，请先登记并实现解析",
            );
            ctx.stats.skipped_unknown_ids += 1;
        }

        out.push(EbmlElement {
            id: h.id,
            id_length: h.id_length,
            data_offset: h.data_offset,
            data_size: effective_end - h.data_offset,
            size_unknown: h.size_unknown,
            children,
        });

        // 游标必须严格推进（头宽 ≥ 2，故此处实际上是恒真的不变式，
        // 但保留为防御：一旦read_element_header 的语义变更，这里是第一道闸）。
        if effective_end <= cursor {
            return Err(Failure::new(
                DiagCode::ElementOutOfRange,
                format!("元素 0x{:x} 在偏移 {} 处未推进游标", h.id, cursor),
                "零长度或负长度元素会造成死循环；拒绝解析",
            ));
        }
        cursor = effective_end;
    }
    Ok(out)
}

// ════════════════════════════════════════════════════════════════════════════
// §4 payload 读取器（大端整数 / IEEE754 浮点 / UTF-8 字符串）
// ════════════════════════════════════════════════════════════════════════════

/// 在元素列表中查找指定 ID 的**第一个**元素。
pub fn find_child<'a>(list: &'a [EbmlElement], id: u32) -> Option<&'a EbmlElement> {
    list.iter().find(|e| e.id == id)
}

/// 收集全部指定 ID 的元素（按文档顺序）。
pub fn collect_children<'a>(list: &'a [EbmlElement], id: u32) -> Vec<&'a EbmlElement> {
    list.iter().filter(|e| e.id == id).collect()
}

/// 读取 payload 为无符号整数（Matroska 整数大端、宽度可变、至多 8 字节有效）。
///
/// 超过 8 字节的前导字节按Matroska 规范必须为 0；这里**不**静默丢弃非零高位，
/// 而是记诊断并只取低 8 字节（保真：调用方能从诊断知道声明值不可信）。
pub fn read_uint(ctx: &mut Ctx, el: &EbmlElement) -> u64 {
    let len = el.data_size.min(8);
    let mut value: u64 = 0;
    if let Some(bytes) = ctx.slice(el.data_offset, len) {
        for b in bytes.iter() {
            value = match value.checked_mul(256).and_then(|v| v.checked_add(*b as u64)) {
                Some(v) => v,
                None => break,
            };
        }
    }
    if el.data_size > 8 {
        ctx.bag.push(
            DiagCode::ArithmeticOverflow,
            format!(
                "元素 0x{:x} 的整数宽度 {} 超过 8 字节，仅取低 8 字节",
                el.id, el.data_size
            ),
            "Matroska 整数至多 8 字节；更宽的声明非规范，值不可信",
        );
    }
    value
}

/// 读取 payload 为可选整数（缺失返回 0，不产诊断——缺失是合法形态）。
pub fn read_uint_or_zero(ctx: &mut Ctx, el: Option<&EbmlElement>) -> u64 {
    match el {
        Some(e) => read_uint(ctx, e),
        None => 0,
    }
}

/// 读取 payload 为浮点数（4 或 8 字节 IEEE754，大端）。
///
/// 宽度非 4/8 时返回 `None` 并记诊断（不猜IEEE754 的其他合法宽度）。
/// 用 `from_bits` 而非 transmute：字节序由 `from_be_bytes` 显式承担。
pub fn read_float(ctx: &mut Ctx, el: &EbmlElement) -> Option<f64> {
    match el.data_size {
        4 => {
            let b = ctx.slice(el.data_offset, 4)?;
            let arr: [u8; 4] = match b.try_into() {
                Ok(v) => v,
                Err(_) => return None,
            };
            Some(f32::from_bits(u32::from_be_bytes(arr)) as f64)
        }
        8 => {
            let b = ctx.slice(el.data_offset, 8)?;
            let arr: [u8; 8] = match b.try_into() {
                Ok(v) => v,
                Err(_) => return None,
            };
            Some(f64::from_bits(u64::from_be_bytes(arr)))
        }
        other => {
            ctx.bag.push(
                DiagCode::ChildMissing,
                format!("元素 0x{:x} 的浮点宽度 {} 非 4/8 字节", el.id, other),
                "Matroska 浮点仅定义 32/64 位；其他宽度按缺失处理",
            );
            None
        }
    }
}

/// 剥离尾部 NUL 填充（Matroska 的 ASCII 字符串允许 NUL 结尾）。
fn strip_trailing_nul(bytes: &[u8]) -> &[u8] {
    let mut end = bytes.len();
    while end > 0 && bytes[end - 1] == 0 {
        end -= 1;
    }
    &bytes[..end]
}

/// 读取 payload 为 UTF-8 字符串（剥离尾部 NUL；非法序列取合法前缀并留诊断）。
pub fn read_string(ctx: &mut Ctx, el: &EbmlElement) -> String {
    let bytes = match ctx.slice(el.data_offset, el.data_size) {
        Some(b) => strip_trailing_nul(b),
        None => {
            ctx.bag.push(
                DiagCode::ElementOutOfRange,
                format!("元素 0x{:x} 的字符串 payload 越界", el.id),
                "声明长度超出文件；该字段按空串处理",
            );
            return String::new();
        }
    };
    match core::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(e) => {
            let valid = bytes.get(..e.valid_up_to()).unwrap_or(&[]);
            ctx.bag.push(
                DiagCode::StringInvalidUtf8,
                format!(
                    "元素 0x{:x} 的字符串含非法 UTF-8（首个错误在字节 {}），已取合法前缀",
                    el.id,
                    e.valid_up_to()
                ),
                "字符串字段应合法 UTF-8；已截断到合法前缀并留痕，不静默替换字符",
            );
            core::str::from_utf8(valid).unwrap_or("").to_string()
        }
    }
}

// ════════════════════════════════════════════════════════════════════════════
// §5 轨道解析与编解码器识别（判据二：编解码器/时长/默认帧率）
// ════════════════════════════════════════════════════════════════════════════

/// 轨道媒体类型（`TrackType`：1=video, 2=audio, 17=subtitle, 0x10=logo）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TrackKind {
    Video,
    Audio,
    Subtitle,
    Logo,
    Unknown,
}

impl TrackKind {
    /// 由 `TrackType` 数值映射。
    pub const fn from_code(code: u64) -> TrackKind {
        match code {
            1 => TrackKind::Video,
            2 => TrackKind::Audio,
            17 => TrackKind::Subtitle,
            0x10 => TrackKind::Logo,
            _ => TrackKind::Unknown,
        }
    }

    /// 稳定字符串名。
    pub const fn as_str(self) -> &'static str {
        match self {
            TrackKind::Video => "video",
            TrackKind::Audio => "audio",
            TrackKind::Subtitle => "subtitle",
            TrackKind::Logo => "logo",
            TrackKind::Unknown => "unknown",
        }
    }
}

/// 解析出的一条轨道。
#[derive(Clone, PartialEq, Debug)]
pub struct MkvTrack {
    /// 轨道号（`TrackNumber`，Block 用它引用轨道）。
    pub track_number: u64,
    pub kind: TrackKind,
    /// 编解码器 ID（`CodecID`，如 `V_VP9` / `A_OPUS`）。
    pub codec_id: String,
    /// 默认帧时长（纳秒；来自 `DefaultDuration`，0 = 未声明）。
    pub default_duration_ns: u64,
    /// 默认帧率（毫帧/秒，整数；未声明时 0）。由`default_duration_ns` 导出。
    pub default_fps_milli: u64,
    pub pixel_width: u64,
    pub pixel_height: u64,
    pub display_width: u64,
    pub display_height: u64,
    /// 采样率（Hz；未声明时 0.0）。
    pub sampling_frequency: f64,
    pub channels: u64,
    pub bit_depth: u64,
}

impl MkvTrack {
    /// 时长（毫秒；由默认帧时长导出，0 = 不可静态确定）。
    pub fn frame_duration_ms(&self) -> u64 {
        self.default_duration_ns / 1_000_000
    }
}

/// 承接工作单登记表：编解码器 ID → 承接单号。
///
/// **逐条对施工书校对**（存量TS 把 `A_OPUS` 指向 `VE-F1207`——那是AV1 **视频**
/// 解码器，属错的映射；音频解码在 VE-H 域）。
pub const CODEC_HANDOFF: &[(&str, &str)] = &[
    ("V_MPEG4/ISO/AVC", "VE-F1204"),
    ("V_MPEG4/ISO/SP", "VE-F1204"),
    ("V_MPEG4/ISO/AP", "VE-F1204"),
    ("V_MPEG4/ISO/ASP", "VE-F1204"),
    ("V_MPEGH/ISO/HEVC", "VE-F1205"),
    ("V_MPEG2", "VE-F1204"),
    ("V_VP8", "VE-F1206"),
    ("V_VP9", "VE-F1206"),
    ("V_AV1", "VE-F1207"),
    ("A_OPUS", "VE-H"),
    ("A_VORBIS", "VE-H"),
    ("A_AAC", "VE-H"),
    ("A_FLAC", "VE-H"),
    ("A_MPEG/L3", "VE-H"),
    ("S_TEXT/UTF8", "VE-G05"),
    ("S_TEXT/ASS", "VE-G05"),
    ("S_TEXT/SSA", "VE-G05"),
    ("S_VOBSUB", "VE-G05"),
    ("S_HDMV/PGS", "VE-G05"),
];

/// 承接工作单说明（自检校验承接单在册且语义不串域）。
pub const HANDOFF_TARGETS: &[&str] = &[
    "VE-F1204",
    "VE-F1205",
    "VE-F1206",
    "VE-F1207",
    "VE-H",
    "VE-G05",
];

/// 视频解码承接单集合（用于「音频编解码器不得指向视频解码单」判据）。
pub const VIDEO_DECODE_WORKITEMS: &[&str] = &["VE-F1204", "VE-F1205", "VE-F1206", "VE-F1207"];

/// 编解码器能力查询结果。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CodecQuery {
    pub supported: bool,
    /// 承接单号（未登记时为空串）。
    pub workitem: String,
    /// 结论说明（成功与失败都给——失败时说明「为什么不支持」）。
    pub reason: String,
}

/// 查询编解码器能力。未知 ID **显性不支持**。
///
/// 纪律：未登记的编码不得静默送入解码器（会产出花屏或崩溃），须先登记并实现
/// 对应解码通路。
pub fn query_mkv_codec(codec_id: &str) -> CodecQuery {
    for (id, workitem) in CODEC_HANDOFF {
        if *id == codec_id {
            return CodecQuery {
                supported: true,
                workitem: workitem.to_string(),
                reason: format!("已登记，交由 {} 解码", workitem),
            };
        }
    }
    CodecQuery {
        supported: false,
        workitem: String::new(),
        reason: format!(
            "编解码器 ID \"{}\" 未登记到 CODEC_HANDOFF。未登记的编码不得静默送入解码器（会产出花屏或崩溃），须先登记并实现对应解码通路",
            codec_id
        ),
    }
}

/// 解析 `Tracks` 容器下的全部 `TrackEntry`。
pub fn parse_tracks(ctx: &mut Ctx, tracks_el: &EbmlElement) -> R<Vec<MkvTrack>> {
    let entries = collect_children(&tracks_el.children, ids::TRACK_ENTRY);
    if entries.is_empty() {
        return Err(Failure::new(
            DiagCode::ChildMissing,
            "Tracks 元素内没有任何 TrackEntry".to_string(),
            "文件不含轨道；若为纯音频/视频文件，须确认 Tracks 未被截断",
        ));
    }
    if entries.len() > ctx.limits.track_count {
        return Err(Failure::new(
            DiagCode::CountLimitExceeded,
            format!("轨道数 {} 超过上限 {}", entries.len(), ctx.limits.track_count),
            "轨道数异常；拒绝解析",
        ));
    }
    let mut out: Vec<MkvTrack> = Vec::new();
    for entry in entries {
        let num_el = match find_child(&entry.children, ids::TRACK_NUMBER) {
            Some(e) => e,
            None => {
                ctx.bag.push(
                    DiagCode::ChildMissing,
                    "某 TrackEntry 缺少 TrackNumber，已跳过".to_string(),
                    "无轨道号则 Block 无法引用该轨；该轨已跳过并留痕",
                );
                continue;
            }
        };
        let track_number = read_uint(ctx, num_el);
        // 轨道号必须唯一：重复会让 Block 指向错误轨道。
        if out.iter().any(|t| t.track_number == track_number) {
            return Err(Failure::new(
                DiagCode::TrackNotFound,
                format!("轨道号 {} 重复出现", track_number),
                "轨道号必须唯一；重复会导致 Block 指向错误轨道",
            ));
        }

        let type_val = read_uint_or_zero(ctx, find_child(&entry.children, ids::TRACK_TYPE));
        let kind = TrackKind::from_code(type_val);
        let codec_id = match find_child(&entry.children, ids::CODEC_ID) {
            Some(e) => read_string(ctx, e),
            None => String::new(),
        };
        let default_duration_ns =
            read_uint_or_zero(ctx, find_child(&entry.children, ids::DEFAULT_DURATION));

        // 视频专属字段
        let (pixel_width, pixel_height, display_width, display_height) =
            match find_child(&entry.children, ids::VIDEO) {
                Some(v) => (
                    read_uint_or_zero(ctx, find_child(&v.children, ids::PIXEL_WIDTH)),
                    read_uint_or_zero(ctx, find_child(&v.children, ids::PIXEL_HEIGHT)),
                    read_uint_or_zero(ctx, find_child(&v.children, ids::DISPLAY_WIDTH)),
                    read_uint_or_zero(ctx, find_child(&v.children, ids::DISPLAY_HEIGHT)),
                ),
                None => (0, 0, 0, 0),
            };
        // 音频专属字段：`SamplingFrequency` 缺失时按规范回退
        // `OutputSamplingFrequency`（二者只允许其一非零；显式回退链，不猜）。
        let mut sampling_frequency = 0.0f64;
        let mut channels = 0u64;
        let mut bit_depth = 0u64;
        if let Some(a) = find_child(&entry.children, ids::AUDIO) {
            let freq_el = find_child(&a.children, ids::SAMPLING_FREQUENCY)
                .or_else(|| find_child(&a.children, ids::OUTPUT_SAMPLING_FREQUENCY));
            if let Some(f) = freq_el {
                sampling_frequency = read_float(ctx, f).unwrap_or(0.0);
            }
            channels = read_uint_or_zero(ctx, find_child(&a.children, ids::CHANNELS));
            bit_depth = read_uint_or_zero(ctx, find_child(&a.children, ids::BIT_DEPTH));
        }

        let track = MkvTrack {
            track_number,
            kind,
            codec_id,
            default_duration_ns,
            default_fps_milli: fps_milli_from_duration(default_duration_ns),
            pixel_width,
            pixel_height,
            display_width,
            display_height,
            sampling_frequency,
            channels,
            bit_depth,
        };
        // 未登记的编解码器：显性不支持（诊断留痕），但**不丢弃轨道**——
        // 丢弃会连带丢掉该轨的元数据（分辨率/时长），对播放器提示更有用。
        let q = query_mkv_codec(&track.codec_id);
        if !q.supported {
            ctx.bag.push(
                DiagCode::CodecUnsupported,
                format!("轨道 {} 的编解码器 \"{}\" 未登记", track_number, track.codec_id),
                &q.reason,
            );
        }
        out.push(track);
    }
    if out.is_empty() {
        return Err(Failure::new(
            DiagCode::ChildMissing,
            "全部 TrackEntry 都缺少 TrackNumber，无可用轨道".to_string(),
            "每个 TrackEntry 必须有 TrackNumber；请确认 Tracks 未被截断",
        ));
    }
    Ok(out)
}

/// 由默认帧时长（纳秒）导出毫帧/秒的帧率（整数，跨平台可复现）。
///
/// 帧率 = 1e9 / duration_ns；用千分单位即 `1e12 / duration_ns`。
/// 未声明（0）时返回 0——「未声明」与「声明为 0」不可混同。
pub fn fps_milli_from_duration(default_duration_ns: u64) -> u64 {
    if default_duration_ns == 0 {
        return 0;
    }
    1_000_000_000_000u64 / default_duration_ns
}

// ════════════════════════════════════════════════════════════════════════════
// §6 Block / SimpleBlock 与 lacing 三模式（判据三：MKV 相对 MP4 的核心差异）
// ════════════════════════════════════════════════════════════════════════════

/// lacing 模式：Xiph（字节累加）、fixed（等分）、EBML（首 VINT 为帧数）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LacingMode {
    None,
    Xiph,
    Fixed,
    Ebml,
}

impl LacingMode {
    /// 由 flags 的 lacing 位（bit2~bit1）映射。
    pub const fn from_bits(bits: u8) -> LacingMode {
        match bits & 0x03 {
            1 => LacingMode::Xiph,
            2 => LacingMode::Fixed,
            3 => LacingMode::Ebml,
            _ => LacingMode::None,
        }
    }

    /// 稳定字符串名。
    pub const fn as_str(self) -> &'static str {
        match self {
            LacingMode::None => "none",
            LacingMode::Xiph => "xiph",
            LacingMode::Fixed => "fixed",
            LacingMode::Ebml => "ebml",
        }
    }

    /// 是否为带 lacing 的模式。
    pub const fn is_laced(self) -> bool {
        !matches!(self, LacingMode::None)
    }
}

/// 一个 Block 解出的一帧（lacing 时一个 Block 含多帧）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MkvFrame {
    pub track_number: u64,
    /// 在文件中的绝对偏移（指向该帧载荷首字节）。
    pub offset: u64,
    pub size: u64,
    /// 呈现时间戳（毫秒，整数换算）。
    pub time_ms: u64,
    /// 是否为关键帧（块级属性，lacing 帧整块继承）。
    pub keyframe: bool,
    /// 所属 Block 内序号。
    pub frame_in_block: u32,
}

/// Block 头部解析结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BlockHeader {
    pub track_number: u64,
    /// 相对 Cluster 时间码的偏移（**有符号**，int16 大端）。
    pub relative_timecode: i16,
    pub keyframe: bool,
    pub lacing_mode: LacingMode,
    /// lacing 帧数（无 lacing 时 0）。
    pub frame_count: u32,
    /// 帧尺寸表（或帧数据）起点的绝对偏移。
    pub body_start: u64,
}

/// 解析 Block / SimpleBlock 头部。
pub fn read_block_header(
    ctx: &mut Ctx,
    start: u64,
    end: u64,
    simple: bool,
) -> R<BlockHeader> {
    // 轨道号：长度型 VINT（去标记位）
    let tn = decode_vint(ctx.data, start, end, 8, false)?;
    if tn.unknown {
        return Err(Failure::new(
            DiagCode::VintInvalid,
            format!("Block 在偏移 {} 处的轨道号 VINT 为未知长度形态", start),
            "轨道号是确定值，不允许未知长度形态",
        ));
    }
    let cursor = match checked_add(start, tn.width as u64) {
        Some(v) => v,
        None => return Err(overflow_failure("Block 轨道号偏移推进")),
    };

    // 时间偏移：int16 大端（有符号）
    let rel_bytes = match ctx.slice(cursor, 2) {
        Some(b) => b,
        None => {
            return Err(Failure::new(
                DiagCode::BlockOutOfRange,
                format!("Block 在偏移 {} 处的时间偏移字段越界", cursor),
                "Block 被截断；请核对文件完整性",
            ))
        }
    };
    let rel_arr: [u8; 2] = match rel_bytes.try_into() {
        Ok(v) => v,
        Err(_) => {
            return Err(Failure::new(
                DiagCode::BlockOutOfRange,
                format!("Block 在偏移 {} 处的时间偏移字段宽度异常", cursor),
                "Block 被截断",
            ))
        }
    };
    let relative_timecode = i16::from_be_bytes(rel_arr);
    let cursor = match checked_add(cursor, 2) {
        Some(v) => v,
        None => return Err(overflow_failure("Block 时间偏移推进")),
    };

    let flags = match ctx.byte(cursor) {
        Some(f) => f,
        None => {
            return Err(Failure::new(
                DiagCode::BlockOutOfRange,
                format!("Block 在偏移 {} 处的 flags 字节越界", cursor),
                "Block 被截断",
            ))
        }
    };
    let cursor = match checked_add(cursor, 1) {
        Some(v) => v,
        None => return Err(overflow_failure("Block flags 推进")),
    };

    // SimpleBlock 按规范恒为关键帧；Block 取 bit7。
    let keyframe = simple || (flags & 0x80) != 0;
    let lacing_mode = LacingMode::from_bits(flags >> 1);
    let mut frame_count: u32 = 0;
    let mut cursor = cursor;
    match lacing_mode {
        LacingMode::None => {}
        LacingMode::Xiph | LacingMode::Fixed => {
            frame_count = (flags & 0x3f) as u32 + 1;
        }
        LacingMode::Ebml => {
            // EBML lacing：紧跟一个 VINT 表示帧数
            let fc = decode_vint(ctx.data, cursor, end, 8, false)?;
            if fc.unknown {
                return Err(Failure::new(
                    DiagCode::LacingInsufficientData,
                    format!("Block 在偏移 {} 处的 lacing 帧数 VINT 为未知长度形态", cursor),
                    "lacing 帧数必须确定；未知长度形态非法",
                ));
            }
            if fc.value > u32::MAX as u64 {
                return Err(Failure::new(
                    DiagCode::LacingInsufficientData,
                    format!("lacing 帧数 {} 超出可表示范围", fc.value),
                    "帧数异常；拒绝解析",
                ));
            }
            frame_count = fc.value as u32;
            cursor = match checked_add(cursor, fc.width as u64) {
                Some(v) => v,
                None => return Err(overflow_failure("lacing 帧数字段推进")),
            };
        }
    }

    if frame_count > ctx.limits.lacing_frame_count {
        return Err(Failure::new(
            DiagCode::LacingInsufficientData,
            format!(
                "lacing 帧数 {} 超过上限 {}",
                frame_count, ctx.limits.lacing_frame_count
            ),
            "单 Block 内帧数异常；拒绝解析",
        ));
    }
    Ok(BlockHeader {
        track_number: tn.value,
        relative_timecode,
        keyframe,
        lacing_mode,
        frame_count,
        body_start: cursor,
    })
}

/// 一帧在 Block 内的（相对 body_start 的偏移, 大小）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LaceSlice {
    pub rel_offset: u64,
    pub size: u64,
}

/// 按 lacing 模式把 Block 载荷拆成各帧的（相对偏移, 大小）。
///
/// - `fixed`：剩余字节**均分**，不整除即拒绝（声明与数据矛盾）；
/// - `xiph`：字节累加，`0xFF` 表示续下一个字节（每次累加都做 `checked_add`）；
/// - `ebml`：首 VINT 为帧数，随后 `frameCount-1` 个 VINT 为前若干帧大小，
///   最后一帧吃掉剩余。
///
/// 三模式共同的不变式：**尺寸表不得越过 Block 末尾**——越界即声明与数据矛盾。
pub fn split_lacing(
    ctx: &mut Ctx,
    body_start: u64,
    block_end: u64,
    mode: LacingMode,
    frame_count: u32,
) -> R<Vec<LaceSlice>> {
    if frame_count == 0 {
        return Err(Failure::new(
            DiagCode::LacingInsufficientData,
            "lacing 帧数为 0".to_string(),
            "lacing 模式下帧数必须 ≥1",
        ));
    }
    let n = frame_count as usize;
    let mut sizes: Vec<u64> = vec![0u64; n];
    let mut cursor = body_start;

    if mode == LacingMode::Fixed {
        let remaining = match block_end.checked_sub(body_start) {
            Some(v) => v,
            None => {
                return Err(Failure::new(
                    DiagCode::LacingInsufficientData,
                    format!("fixed lacing 的 body_start({}) 越过 Block 末尾({})", body_start, block_end),
                    "尺寸表起点越界；文件损坏",
                ))
            }
        };
        if remaining % (n as u64) != 0 {
            return Err(Failure::new(
                DiagCode::LacingInsufficientData,
                format!("fixed lacing 无法均分：剩余 {} 字节 / {} 帧", remaining, n),
                "fixed lacing 要求块大小被帧数整除；不整除说明帧数声明与数据矛盾",
            ));
        }
        let each = remaining / (n as u64);
        for slot in sizes.iter_mut() {
            *slot = each;
        }
    } else {
        for i in 0..(n - 1) {
            let mut size: u64 = 0;
            match mode {
                LacingMode::Xiph => loop {
                    let b = match ctx.byte(cursor) {
                        Some(b) => b,
                        None => {
                            return Err(Failure::new(
                                DiagCode::LacingInsufficientData,
                                format!("xiph lacing 第 {} 帧尺寸越界", i),
                                "尺寸表被截断",
                            ))
                        }
                    };
                    if cursor >= block_end {
                        return Err(Failure::new(
                            DiagCode::LacingInsufficientData,
                            format!("xiph lacing 第 {} 帧尺寸越过 Block 末尾", i),
                            "尺寸表被截断",
                        ));
                    }
                    cursor = match checked_add(cursor, 1) {
                        Some(v) => v,
                        None => return Err(overflow_failure("xiph 尺寸游标推进")),
                    };
                    // 真溢出检查：每字节累加都经checked_add（存量 TS 此处是
                    // `checkedAdd(size, 0)` 的恒真空断言）。
                    size = match checked_add(size, b as u64) {
                        Some(v) => v,
                        None => return Err(overflow_failure("xiph lacing 尺寸累加")),
                    };
                    if b != 0xff {
                        break;
                    }
                },
                LacingMode::Ebml => {
                    let v = decode_vint(ctx.data, cursor, block_end, 8, false)?;
                    if v.unknown {
                        return Err(Failure::new(
                            DiagCode::LacingInsufficientData,
                            format!("ebml lacing 第 {} 帧尺寸 VINT 为未知长度形态", i),
                            "帧大小必须确定；未知长度形态非法（不可当作普通数值）",
                        ));
                    }
                    size = v.value;
                    cursor = match checked_add(cursor, v.width as u64) {
                        Some(v2) => v2,
                        None => return Err(overflow_failure("ebml 尺寸游标推进")),
                    };
                }
                LacingMode::None | LacingMode::Fixed => {
                    return Err(Failure::new(
                        DiagCode::LacingInsufficientData,
                        format!("split_lacing 收到非 lacing 模式 {}", mode.as_str()),
                        "调用方契约违例",
                    ))
                }
            }
            if let Some(slot) = sizes.get_mut(i) {
                *slot = size;
            }
        }
        // 前 n-1 帧大小已逐帧读出；最后一帧吃掉**余下的**字节。
        //
        // 注意：余量是「payload 总量减去已分配给前 n-1 帧的字节」，**不是**
        // `block_end - cursor`（cursor 只是尺寸表的结束位置，尚未扣除帧数据）。
        // 写成后者会让最后一帧重复计入前面帧已占的字节，总量随即超出 Block
        // 末尾——表现为「合法的 lacing 块被误判为尺寸表越界」。
        if cursor > block_end {
            return Err(Failure::new(
                DiagCode::LacingInsufficientData,
                format!(
                    "lacing 尺寸表越过 Block 末尾（尺寸表结束于 {}，Block 末尾 {}）",
                    cursor, block_end
                ),
                "帧大小声明之和超过 Block 长度；文件损坏或尺寸表被篡改",
            ));
        }
        let payload_total = match block_end.checked_sub(body_start) {
            Some(v) => v,
            None => {
                return Err(Failure::new(
                    DiagCode::LacingInsufficientData,
                    format!("lacing 的 body_start({}) 越过 Block 末尾({})", body_start, block_end),
                    "尺寸表起点越界；文件损坏",
                ))
            }
        };
        let mut assigned: u64 = 0;
        for s in sizes.iter().take(n - 1) {
            assigned = match checked_add(assigned, *s) {
                Some(v) => v,
                None => return Err(overflow_failure("lacing 已分配字节累加")),
            };
        }
        // 声明的前 n-1 帧之和已超总量 → 尺寸表与数据矛盾。
        if assigned > payload_total {
            return Err(Failure::new(
                DiagCode::LacingInsufficientData,
                format!(
                    "lacing 前 {} 帧声明共{} 字节，超出 Block 载荷 {} 字节",
                    n - 1,
                    assigned,
                    payload_total
                ),
                "帧大小声明之和超过 Block 长度；文件损坏或尺寸表被篡改",
            ));
        }
        if let Some(slot) = sizes.get_mut(n - 1) {
            *slot = payload_total - assigned;
        }
    }

    // 生成相对偏移，并校验每帧都落在 Block 内。
    let mut out: Vec<LaceSlice> = Vec::with_capacity(n);
    let mut off = body_start;
    for (i, size) in sizes.iter().enumerate() {
        if !checked_range(off, *size, block_end) {
            return Err(Failure::new(
                DiagCode::LacingInsufficientData,
                format!("lacing 第 {} 帧（offset={}, size={}）越过 Block 末尾 {}", i, off, size, block_end),
                "帧大小声明与 Block 长度矛盾",
            ));
        }
        out.push(LaceSlice { rel_offset: off, size: *size });
        off = match checked_add(off, *size) {
            Some(v) => v,
            None => return Err(overflow_failure("lacing 帧偏移推进")),
        };
    }
    Ok(out)
}

/// 时间码换算：`(ticks * scale_ns) / 1e6` → 毫秒，**整数**（u128 中间量）。
pub fn timecode_to_ms(
    abs_ticks: i64,
    scale_ns: u64,
    max_time_ms: u64,
) -> R<u64> {
    if abs_ticks < 0 {
        return Err(Failure::new(
            DiagCode::TimecodeRegression,
            format!("绝对时间码为负（{}）", abs_ticks),
            "Block 相对时间码使绝对时间落到零之前；文件损坏",
        ));
    }
    let product = (abs_ticks as u128) * (scale_ns as u128);
    let ms = product / 1_000_000u128;
    if ms > max_time_ms as u128 {
        return Err(Failure::new(
            DiagCode::ArithmeticOverflow,
            format!("时间码换算结果 {} ms 超过上限 {} ms", ms, max_time_ms),
            "时间码异常（常见于TimecodeScale 被恶意放大）；拒绝解析",
        ));
    }
    Ok(ms as u64)
}

/// 解析一个 Block / SimpleBlock 为若干帧。
///
/// `cluster_timecode` 是所属 Cluster 的 `Timecode`（刻度）。
pub fn parse_block(
    ctx: &mut Ctx,
    el: &EbmlElement,
    simple: bool,
    scale_ns: u64,
    cluster_timecode: i64,
) -> R<Vec<MkvFrame>> {
    let block_end = match el.payload_end() {
        Some(v) => v,
        None => return Err(overflow_failure("Block payload 末尾")),
    };
    let h = read_block_header(ctx, el.data_offset, block_end, simple)?;

    // 绝对时间码 = Cluster 基准 + 相对偏移（两者皆可越出 Cluster 基准，
    // 故用 i64 真加法；负值交由 timecode_to_ms 显性拒绝）。
    let abs_ticks = match cluster_timecode.checked_add(h.relative_timecode as i64) {
        Some(v) => v,
        None => return Err(overflow_failure("绝对时间码累加")),
    };
    let time_ms = timecode_to_ms(abs_ticks, scale_ns, ctx.limits.max_time_ms)?;

    // 无 lacing：整个 Block 载荷就是一帧
    if !h.lacing_mode.is_laced() {
        let size = match block_end.checked_sub(h.body_start) {
            Some(v) => v,
            None => {
                return Err(Failure::new(
                    DiagCode::BlockOutOfRange,
                    "Block 头部越过 Block 末尾".to_string(),
                    "Block 声明与长度矛盾",
                ))
            }
        };
        if !checked_range(h.body_start, size, ctx.len()) {
            return Err(Failure::new(
                DiagCode::BlockOutOfRange,
                format!("Block 载荷 [{},+{}) 越出文件", h.body_start, size),
                "Block 长度声明与文件长度矛盾",
            ));
        }
        return Ok(vec![MkvFrame {
            track_number: h.track_number,
            offset: h.body_start,
            size,
            time_ms,
            keyframe: h.keyframe,
            frame_in_block: 0,
        }]);
    }

    // lacing：拆分多帧
    let slices = split_lacing(ctx, h.body_start, block_end, h.lacing_mode, h.frame_count)?;
    let mut frames: Vec<MkvFrame> = Vec::with_capacity(slices.len());
    for (i, s) in slices.iter().enumerate() {
        if !checked_range(s.rel_offset, s.size, ctx.len()) {
            return Err(Failure::new(
                DiagCode::BlockOutOfRange,
                format!("lacing 第 {} 帧越出文件", i),
                "帧偏移/长度与文件长度矛盾",
            ));
        }
        frames.push(MkvFrame {
            track_number: h.track_number,
            offset: s.rel_offset,
            size: s.size,
            time_ms,
            // 关键帧是**块级**属性：lacing 帧整块继承（与规范及ffprobe 对齐）。
            keyframe: h.keyframe,
            frame_in_block: i as u32,
        });
    }
    Ok(frames)
}

// ════════════════════════════════════════════════════════════════════════════
// §7 Cluster / Segment 结构与时间码基准（判据二：Cluster 的时间戳基准）
// ════════════════════════════════════════════════════════════════════════════

/// 文档类型（`DocType`）——webm / matroska 分流。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DocType {
    Webm,
    Matroska,
}

impl DocType {
    /// 由 `DocType` 字符串映射（未匹配即 `None`，由调用方显性拒绝）。
    pub fn from_str_value(s: &str) -> Option<DocType> {
        match s {
            "webm" => Some(DocType::Webm),
            "matroska" => Some(DocType::Matroska),
            _ => None,
        }
    }

    /// 稳定字符串名。
    pub const fn as_str(self) -> &'static str {
        match self {
            DocType::Webm => "webm",
            DocType::Matroska => "matroska",
        }
    }
}

/// 支持的 EBML 版本（`EBMLVersion` 上界；更高版本显性拒绝而非猜测兼容）。
pub const MAX_EBML_VERSION: u64 = 1;
/// 支持的 DocType 版本上界。
pub const MAX_DOC_TYPE_VERSION: u64 = 4;

/// EBML 头。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct EbmlHeaderInfo {
    pub version: u64,
    pub read_version: u64,
    pub doc_type_version: u64,
    pub doc_type_read_version: u64,
    pub max_id_length: u64,
    pub max_size_length: u64,
    pub doc_type: DocType,
}

/// 解析 EBML 头（版本 / DocType 分流）。
pub fn parse_ebml_header(ctx: &mut Ctx, el: &EbmlElement) -> R<EbmlHeaderInfo> {
    let version = read_uint_or_zero(ctx, find_child(&el.children, ids::EBML_VERSION));
    let read_version = read_uint_or_zero(ctx, find_child(&el.children, ids::EBML_READ_VERSION));
    let doc_type_version = read_uint_or_zero(ctx, find_child(&el.children, ids::DOC_TYPE_VERSION));
    let doc_type_read_version =
        read_uint_or_zero(ctx, find_child(&el.children, ids::DOC_TYPE_READ_VERSION));
    let max_id_length = read_uint_or_zero(ctx, find_child(&el.children, ids::EBML_MAX_ID_LENGTH));
    let max_size_length = read_uint_or_zero(ctx, find_child(&el.children, ids::EBML_MAX_SIZE_LENGTH));

    if version > MAX_EBML_VERSION {
        return Err(Failure::new(
            DiagCode::HeaderInvalid,
            format!("EBMLVersion {} 超过支持上限 {}", version, MAX_EBML_VERSION),
            "更高版本的 EBML 头可能引入未知语义；显性拒绝而非猜测兼容",
        ));
    }
    if doc_type_version > MAX_DOC_TYPE_VERSION {
        return Err(Failure::new(
            DiagCode::HeaderInvalid,
            format!("DocTypeVersion {} 超过支持上限 {}", doc_type_version, MAX_DOC_TYPE_VERSION),
            "更高版本的 Matroska 语义未实现；显性拒绝",
        ));
    }
    let doc_type_str = match find_child(&el.children, ids::DOC_TYPE) {
        Some(e) => read_string(ctx, e),
        None => {
            return Err(Failure::new(
                DiagCode::ChildMissing,
                "EBML 头缺少 DocType".to_string(),
                "无法判定 webm / matroska；显性拒绝",
            ))
        }
    };
    let doc_type = match DocType::from_str_value(&doc_type_str) {
        Some(d) => d,
        None => {
            return Err(Failure::new(
                DiagCode::DocTypeUnsupported,
                format!("DocType \"{}\" 既非 webm 也非 matroska", doc_type_str),
                "本模块只实现 WebM/Matroska 两种容器；其他 EBML 派生格式显性不支持",
            ))
        }
    };
    Ok(EbmlHeaderInfo {
        version,
        read_version,
        doc_type_version,
        doc_type_read_version,
        max_id_length,
        max_size_length,
        doc_type,
    })
}

/// `Info` 元素（Segment 级元数据）。
// f64 不可 derive Eq（浮点无全序）——故本结构只 derive PartialEq。
#[derive(Clone, PartialEq, Debug)]
pub struct SegmentInfo {
    /// `TimecodeScale`（纳秒/刻度）；缺失时按规范默认 1_000_000。
    pub timecode_scale_ns: u64,
    /// `Duration`（刻度，浮点；缺失时 0.0）。
    pub duration_ticks: f64,
    /// `Duration` 换算的毫秒（整数；缺失时 0）。
    pub duration_ms: u64,
    pub muxing_app: String,
    pub writing_app: String,
}

/// `TimecodeScale` 的规范默认值（纳秒）。
pub const DEFAULT_TIMECODE_SCALE_NS: u64 = 1_000_000;

/// 解析 `Info` 元素。
pub fn parse_info(ctx: &mut Ctx, el: &EbmlElement) -> R<SegmentInfo> {
    let timecode_scale_ns = match find_child(&el.children, ids::TIMECODE_SCALE) {
        Some(e) => {
            let v = read_uint(ctx, e);
            if v == 0 {
                return Err(Failure::new(
                    DiagCode::ChildMissing,
                    "TimecodeScale 为 0".to_string(),
                    "TimecodeScale 为 0 会使所有时间戳退化为 0；显性拒绝",
                ));
            }
            v
        }
        None => DEFAULT_TIMECODE_SCALE_NS,
    };
    let duration_ticks = match find_child(&el.children, ids::DURATION) {
        Some(e) => read_float(ctx, e).unwrap_or(0.0),
        None => 0.0,
    };
    let duration_ms = if duration_ticks > 0.0 {
        // 浮点Duration 只在这里出现一次（Info 元素），换算走整数路径并做值域检查。
        let ticks = duration_ticks as u128;
        let ms = (ticks * timecode_scale_ns as u128) / 1_000_000u128;
        if ms > ctx.limits.max_time_ms as u128 {
            0
        } else {
            ms as u64
        }
    } else {
        0
    };
    let muxing_app = match find_child(&el.children, ids::MUXING_APP) {
        Some(e) => read_string(ctx, e),
        None => String::new(),
    };
    let writing_app = match find_child(&el.children, ids::WRITING_APP) {
        Some(e) => read_string(ctx, e),
        None => String::new(),
    };
    Ok(SegmentInfo { timecode_scale_ns, duration_ticks, duration_ms, muxing_app, writing_app })
}

/// 一个 Cluster 的解析结果。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ClusterInfo {
    /// Cluster 的 `Timecode`（刻度）。
    pub timecode: i64,
    /// Cluster 元素 payload 起点（绝对偏移）。
    pub offset: u64,
    /// Cluster 元素 payload 真实末尾（未知长度者已重同步）。
    pub end: u64,
    /// 是否以未知长度编码。
    pub unknown_size: bool,
    /// 块数（Block + SimpleBlock）。
    pub block_count: u64,
    pub frames: Vec<MkvFrame>,
}

/// 解析一个 Cluster 元素为帧序列。
pub fn parse_cluster(
    ctx: &mut Ctx,
    el: &EbmlElement,
    scale_ns: u64,
    known_tracks: &[u64],
) -> R<ClusterInfo> {
    let end = match el.payload_end() {
        Some(v) => v,
        None => return Err(overflow_failure("Cluster payload 末尾")),
    };
    let cluster_timecode = match find_child(&el.children, ids::TIMECODE) {
        Some(e) => read_uint(ctx, e) as i64,
        None => 0,
    };

    let mut frames: Vec<MkvFrame> = Vec::new();
    let mut block_count: u64 = 0;
    // 时间码回跳检测：与「本Cluster 内上一帧」比较（相邻即可，重叠才比所有对）。
    let mut last_time_ms: Option<u64> = None;
    for child in &el.children {
        let simple = match child.id {
            ids::SIMPLE_BLOCK => true,
            ids::BLOCK_GROUP => false,
            _ => continue,
        };
        // BlockGroup 内的 Block 才承载帧；BlockGroup 的其他子元素（ReferenceBlock /
        // BlockDuration）不产生帧。
        let block_elems: Vec<&EbmlElement> = if simple {
            vec![child]
        } else {
            collect_children(&child.children, ids::BLOCK)
        };
        for be in block_elems {
            if block_count as usize >= ctx.limits.block_count {
                return Err(Failure::new(
                    DiagCode::CountLimitExceeded,
                    format!("块数超过上限 {}", ctx.limits.block_count),
                    "块炸弹的典型特征；拒绝解析",
                ));
            }
            block_count += 1;
            let parsed = parse_block(ctx, be, simple, scale_ns, cluster_timecode)?;
            // 轨道存在性：Block 引用的轨道必须在 Tracks 中登记。
            for f in &parsed {
                if !known_tracks.contains(&f.track_number) {
                    return Err(Failure::new(
                        DiagCode::TrackNotFound,
                        format!("Block 引用了不存在的轨道号 {}", f.track_number),
                        "轨道号必须先在 TrackEntry 中登记；否则帧无法归属",
                    ));
                }
            }
            for f in &parsed {
                if let Some(prev) = last_time_ms {
                    if f.time_ms < prev {
                        // 容错重同步：保留帧、计数、留痕，不丢数据也不静默。
                        ctx.stats.timecode_regressions += 1;
                        ctx.bag.push(
                            DiagCode::TimecodeRegression,
                            format!(
                                "时间码回跳：{} ms → {} ms（Cluster {}，轨道 {}）",
                                prev, f.time_ms, cluster_timecode, f.track_number
                            ),
                            "已保留该帧并重同步到Cluster 基准；回跳通常来自损坏的 Cluster 顺序或错误的 seek 写入",
                        );
                    }
                }
                last_time_ms = Some(f.time_ms);
                frames.push(*f);
            }
        }
    }
    Ok(ClusterInfo {
        timecode: cluster_timecode,
        offset: el.data_offset,
        end,
        unknown_size: el.size_unknown,
        block_count,
        frames,
    })
}

// ════════════════════════════════════════════════════════════════════════════
// §8 Cues 索引（与 MP4 的stco/stss 对照：索引方式差异的实证面）
// ════════════════════════════════════════════════════════════════════════════

/// 一条 Cue（索引项）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CuePoint {
    /// `CueTime`（刻度）。
    pub cue_time: i64,
    /// `CueTrack`。
    pub cue_track: u64,
    /// `CueClusterPosition`（相对 Segment 数据起点的字节偏移）。
    pub cluster_position: u64,
}

/// 解析 `Cues` 元素为索引项序列（文档顺序）。
pub fn parse_cues(ctx: &mut Ctx, el: &EbmlElement) -> R<Vec<CuePoint>> {
    let mut out: Vec<CuePoint> = Vec::new();
    for cp in collect_children(&el.children, ids::CUE_POINT) {
        let cue_time = match find_child(&cp.children, ids::CUE_TIME) {
            Some(e) => read_uint(ctx, e) as i64,
            None => 0,
        };
        for ctp in collect_children(&cp.children, ids::CUE_TRACK_POSITIONS) {
            let cue_track = read_uint_or_zero(ctx, find_child(&ctp.children, ids::CUE_TRACK));
            let cluster_position =
                read_uint_or_zero(ctx, find_child(&ctp.children, ids::CUE_CLUSTER_POSITION));
            out.push(CuePoint { cue_time, cue_track, cluster_position });
        }
    }
    Ok(out)
}

// ════════════════════════════════════════════════════════════════════════════
// §9 Attachment 附件（判据：字体 / 封面提取）
// ════════════════════════════════════════════════════════════════════════════

/// 附件用途分类（由 MIME 前缀派生）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AttachmentKind {
    Font,
    Cover,
    Subtitle,
    Other,
}

impl AttachmentKind {
    /// 由 MIME 类型派生。
    pub fn from_mime(mime: &str) -> AttachmentKind {
        if mime.starts_with("font/") || mime == "application/x-font-ttf" || mime == "application/font-sfnt"
        {
            AttachmentKind::Font
        } else if mime.starts_with("image/") {
            AttachmentKind::Cover
        } else if mime.starts_with("text/") || mime == "application/x-subrip" {
            AttachmentKind::Subtitle
        } else {
            AttachmentKind::Other
        }
    }

    /// 稳定字符串名。
    pub const fn as_str(self) -> &'static str {
        match self {
            AttachmentKind::Font => "font",
            AttachmentKind::Cover => "cover",
            AttachmentKind::Subtitle => "subtitle",
            AttachmentKind::Other => "other",
        }
    }
}

/// 一个附件（字体 / 封面 / 字幕等）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Attachment {
    pub file_uid: u64,
    pub name: String,
    pub mime_type: String,
    pub description: String,
    pub kind: AttachmentKind,
    /// `FileData` 在文件中的绝对偏移。
    pub data_offset: u64,
    pub data_size: u64,
}

/// 解析 `Attachments` 元素。
pub fn parse_attachments(ctx: &mut Ctx, el: &EbmlElement) -> R<Vec<Attachment>> {
    let files = collect_children(&el.children, ids::ATTACHED_FILE);
    if files.len() > ctx.limits.attachment_count {
        return Err(Failure::new(
            DiagCode::CountLimitExceeded,
            format!("附件数{} 超过上限 {}", files.len(), ctx.limits.attachment_count),
            "附件数异常；拒绝解析",
        ));
    }
    let mut out: Vec<Attachment> = Vec::with_capacity(files.len());
    for f in files {
        let file_uid = read_uint_or_zero(ctx, find_child(&f.children, ids::FILE_UID));
        let name = match find_child(&f.children, ids::FILE_NAME) {
            Some(e) => read_string(ctx, e),
            None => String::new(),
        };
        let mime_type = match find_child(&f.children, ids::FILE_MIME_TYPE) {
            Some(e) => read_string(ctx, e),
            None => String::new(),
        };
        let description = match find_child(&f.children, ids::FILE_DESCRIPTION) {
            Some(e) => read_string(ctx, e),
            None => String::new(),
        };
        let data_el = find_child(&f.children, ids::FILE_DATA);
        let (data_offset, data_size) = match data_el {
            Some(e) => (e.data_offset, e.data_size),
            None => {
                ctx.bag.push(
                    DiagCode::ChildMissing,
                    format!("附件 \"{}\" 缺少 FileData，已跳过", name),
                    "无数据的附件无法提取；已跳过并留痕",
                );
                continue;
            }
        };
        // FileData 必须完整落在文件内（附件常被放在 Segment 末尾，边界易被恶意构造）。
        if !checked_range(data_offset, data_size, ctx.len()) {
            return Err(Failure::new(
                DiagCode::ElementOutOfRange,
                format!("附件 \"{}\" 的 FileData 越出文件", name),
                "附件数据长度声明与文件长度矛盾；拒绝解析",
            ));
        }
        let kind = AttachmentKind::from_mime(&mime_type);
        out.push(Attachment {
            file_uid,
            name,
            mime_type,
            description,
            kind,
            data_offset,
            data_size,
        });
    }
    Ok(out)
}

// ════════════════════════════════════════════════════════════════════════════
// §10 顶层解封装 + 与 MP4 的差异文档化 + 对拍
// ════════════════════════════════════════════════════════════════════════════

/// 解封装结果。
#[derive(Clone, PartialEq, Debug)]
pub struct MkvFile {
    pub header: EbmlHeaderInfo,
    pub info: SegmentInfo,
    pub tracks: Vec<MkvTrack>,
    pub clusters: Vec<ClusterInfo>,
    pub cues: Vec<CuePoint>,
    pub attachments: Vec<Attachment>,
    pub stats: ParseStats,
    /// 全文件帧数（各 Cluster 累加）。
    pub total_frames: u64,
    /// 关键帧数。
    pub keyframe_count: u64,
}

impl MkvFile {
    /// 按轨道号找轨道。
    pub fn track(&self, track_number: u64) -> Option<&MkvTrack> {
        self.tracks.iter().find(|t| t.track_number == track_number)
    }

    /// 首个指定类型的轨道。
    pub fn first_track_of(&self, kind: TrackKind) -> Option<&MkvTrack> {
        self.tracks.iter().find(|t| t.kind == kind)
    }

    /// 全部帧（按 Cluster 文档顺序）。
    pub fn frames(&self) -> Vec<&MkvFrame> {
        let mut out: Vec<&MkvFrame> = Vec::new();
        for c in &self.clusters {
            for f in &c.frames {
                out.push(f);
            }
        }
        out
    }

    /// 首个 Cluster 之前的字节数（`CueClusterPosition` 的基准面）。
    pub fn first_cluster_offset(&self) -> u64 {
        match self.clusters.first() {
            Some(c) => c.offset,
            None => 0,
        }
    }
}

/// 解封装一个 WebM/Matroska 字节流。
///
/// 流程：EBML 头 → Segment（必需）→ Info（可选，取默认 TimecodeScale）→
/// Tracks（必需）→ Cluster 序列 → Cues（可选）→ Attachments（可选）。
pub fn demux(data: &[u8]) -> Outcome<MkvFile> {
    let mut ctx = Ctx::new(data);
    match demux_run(&mut ctx) {
        Ok(f) => Outcome::Ok { value: f, diagnostics: ctx.bag.all().to_vec() },
        Err(f) => {
            ctx.bag.push_failure(&f);
            Outcome::Err { failure: f, diagnostics: ctx.bag.all().to_vec() }
        }
    }
}

fn demux_run(ctx: &mut Ctx) -> R<MkvFile> {
    let file_end = ctx.len();
    if ctx.is_empty() {
        return Err(Failure::new(
            DiagCode::SegmentMissing,
            "输入为空".to_string(),
            "空输入不可能是 WebM/Matroska；请核对文件",
        ));
    }
    // 顶层：EBML 头 +（可选的 Void） + Segment
    let top = match parse_children(ctx, 0, file_end, 0) {
        Ok(v) => v,
        Err(f) => return Err(f),
    };
    let header_el = match find_child(&top, ids::EBML) {
        Some(e) => e,
        None => {
            return Err(Failure::new(
                DiagCode::HeaderInvalid,
                "文件缺少 EBML 头".to_string(),
                "WebM/Matroska 必须以 EBML 头开始；请确认文件类型正确且未截断",
            ))
        }
    };
    let header = match parse_ebml_header(ctx, header_el) {
        Ok(h) => h,
        Err(f) => return Err(f),
    };
    let segment_el = match find_child(&top, ids::SEGMENT) {
        Some(e) => e,
        None => {
            return Err(Failure::new(
                DiagCode::SegmentMissing,
                "文件缺少 Segment 元素".to_string(),
                "无Segment 即无轨道与帧；请确认文件未截断",
            ))
        }
    };

    // Info（可选）
    let info = match find_child(&segment_el.children, ids::INFO) {
        Some(e) => parse_info(ctx, e)?,
        None => SegmentInfo {
            timecode_scale_ns: DEFAULT_TIMECODE_SCALE_NS,
            duration_ticks: 0.0,
            duration_ms: 0,
            muxing_app: String::new(),
            writing_app: String::new(),
        },
    };

    // Tracks（必需）
    let tracks_el = match find_child(&segment_el.children, ids::TRACKS) {
        Some(e) => e,
        None => {
            return Err(Failure::new(
                DiagCode::ChildMissing,
                "Segment 缺少 Tracks 元素".to_string(),
                "无轨道则无法归属任何帧；显性拒绝",
            ))
        }
    };
    let tracks = parse_tracks(ctx, tracks_el)?;
    let known: Vec<u64> = tracks.iter().map(|t| t.track_number).collect();

    // Cluster 序列
    let cluster_els = collect_children(&segment_el.children, ids::CLUSTER);
    if cluster_els.len() > ctx.limits.cluster_count {
        return Err(Failure::new(
            DiagCode::CountLimitExceeded,
            format!("Cluster 数 {} 超过上限 {}", cluster_els.len(), ctx.limits.cluster_count),
            "Cluster 数异常；拒绝解析",
        ));
    }
    let mut clusters: Vec<ClusterInfo> = Vec::with_capacity(cluster_els.len());
    let mut total_frames: u64 = 0;
    let mut keyframe_count: u64 = 0;
    // Cluster 之间的 Timecode 回跳（相邻比较）。
    let mut last_cluster_time: Option<i64> = None;
    for ce in cluster_els {
        // Cluster 之间的 Timecode 回跳（相邻比较——连续性只比相邻，不比所有对）。
        // 必须在 parse_cluster **之前**从原始元素读 Timecode：EbmlElement 本身
        // 不带该字段（解析后的 ClusterInfo 才带）。
        let raw_tc = match find_child(&ce.children, ids::TIMECODE) {
            Some(t) => read_uint(ctx, t) as i64,
            None => 0,
        };
        if let Some(prev) = last_cluster_time {
            if raw_tc < prev {
                ctx.stats.timecode_regressions += 1;
                ctx.bag.push(
                    DiagCode::TimecodeRegression,
                    format!("Cluster Timecode 回跳：{} → {}", prev, raw_tc),
                    "已保留该 Cluster 并重同步；Cluster 乱序通常来自损坏的索引或拼接",
                );
            }
        }
        let info_c = parse_cluster(ctx, ce, info.timecode_scale_ns, &known)?;
        last_cluster_time = Some(info_c.timecode);
        for f in &info_c.frames {
            total_frames += 1;
            if f.keyframe {
                keyframe_count += 1;
            }
        }
        clusters.push(info_c);
    }

    // Cues（可选）
    let cues = match find_child(&segment_el.children, ids::CUES) {
        Some(e) => parse_cues(ctx, e)?,
        None => Vec::new(),
    };
    // Attachments（可选）
    let attachments = match find_child(&segment_el.children, ids::ATTACHMENTS) {
        Some(e) => parse_attachments(ctx, e)?,
        None => Vec::new(),
    };

    Ok(MkvFile {
        header,
        info,
        tracks,
        clusters,
        cues,
        attachments,
        stats: ctx.stats,
        total_frames,
        keyframe_count,
    })
}

/// 与 MP4（F1202）的差异文档化：**数据表**而非注释。
///
/// 自检拿这张表跟**实测解析结果**对撞，而不是拿表内元素验表内元素——
/// 后者是恒真的弱门禁。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FormatDifference {
    /// 差异维度。
    pub dimension: &'static str,
    /// MP4（F1202）侧的形态。
    pub mp4: &'static str,
    /// MKV（本模块）侧的形态。
    pub mkv: &'static str,
    /// 对下游的约束（写清「不能混用的假设」）。
    pub consequence: &'static str,
}

/// 三维差异：时间戳粒度 / 索引方式 / lacing 有无。
pub const FORMAT_DIFFERENCES: &[FormatDifference] = &[
    FormatDifference {
        dimension: "时间戳粒度",
        mp4: "每轨独立 timescale tick（mdhd timescale，样本时间戳按整数 tick 存 stts）",
        mkv: "全文件统一 TimecodeScale 纳秒基准（Segment 级 Info，Cluster Timecode + Block int16 相对偏移）",
        consequence: "跨轨同步必须经 TimecodeScale 换算；MP4 的每轨 tick 不可直接套到 MKV",
    },
    FormatDifference {
        dimension: "索引方式",
        mp4: "绝对 chunk 偏移（stco/co64 指向文件绝对位置）+ stss 关键帧表",
        mkv: "Cues 相对偏移（CueClusterPosition 相对 Segment 数据起点）+ 块内关键帧标志",
        consequence: "MKV 的 Cues 偏移不可当作文件绝对偏移使用；关键帧查询路径与 MP4 不同",
    },
    FormatDifference {
        dimension: "lacing",
        mp4: "无 lacing：一个 sample 就是一帧",
        mkv: "三模式 lacing（Xiph / fixed / EBML）：一个 Block 可含多帧",
        consequence: "sample→frame 的一一映射在 MKV 上不成立；帧数统计须走 lacing 拆分",
    },
];

/// 外部探测事实（ffprobe 侧给出）——对拍输入。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FfprobeFacts {
    /// 容器名（ffprobe `format_name`，本模块只认 `matroska,webm`）。
    pub format_name: String,
    /// 时长（毫秒）。
    pub duration_ms: u64,
    /// 流数。
    pub stream_count: u64,
    /// 视频流 codec_name。
    pub video_codec: String,
    pub width: u64,
    pub height: u64,
    /// 默认帧率（毫帧/秒）。
    pub avg_frame_rate_milli: u64,
    /// 帧数。
    pub nb_frames: u64,
    /// 关键帧数。
    pub keyframe_count: u64,
}

/// 一条对拍分歧。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CrossCheckFinding {
    /// 字段名。
    pub field: &'static str,
    /// 本模块解析值。
    pub ours: String,
    /// 外部探测值。
    pub theirs: String,
}

/// 对拍：本模块解析结果 × 外部探测事实，逐字段对撞。
///
/// 返回**分歧清单**（空 = 一致）。这是「对拍（ffprobe 交叉验证）」在离线自检
/// 里的可执行形态：接真实 ffprobe 时只需替换 [`FfprobeFacts`] 的来源。
pub fn cross_check(file: &MkvFile, facts: &FfprobeFacts) -> Vec<CrossCheckFinding> {
    let mut out: Vec<CrossCheckFinding> = Vec::new();
    let mut push = |field: &'static str, ours: String, theirs: String| {
        if ours != theirs {
            out.push(CrossCheckFinding { field, ours, theirs });
        }
    };
    push("format_name", file.header.doc_type.as_str().to_string(), facts.format_name.clone());
    push("duration_ms", file.info.duration_ms.to_string(), facts.duration_ms.to_string());
    push("stream_count", file.tracks.len().to_string(), facts.stream_count.to_string());
    let (vcodec, w, h, fps) = match file.first_track_of(TrackKind::Video) {
        Some(t) => {
            let name = matroska_codec_to_ffprobe(&t.codec_id);
            (name, t.pixel_width, t.pixel_height, t.default_fps_milli)
        }
        None => (String::new(), 0, 0, 0),
    };
    push("video_codec", vcodec.clone(), facts.video_codec.clone());
    push("width", w.to_string(), facts.width.to_string());
    push("height", h.to_string(), facts.height.to_string());
    push("avg_frame_rate_milli", fps.to_string(), facts.avg_frame_rate_milli.to_string());
    push("nb_frames", file.total_frames.to_string(), facts.nb_frames.to_string());
    push("keyframe_count", file.keyframe_count.to_string(), facts.keyframe_count.to_string());
    out
}

/// Matroska `CodecID` → ffprobe `codec_name` 映射（对拍口径对齐）。
pub fn matroska_codec_to_ffprobe(codec_id: &str) -> String {
    match codec_id {
        "V_MPEG4/ISO/AVC" | "V_MPEG4/ISO/SP" | "V_MPEG4/ISO/AP" | "V_MPEG4/ISO/ASP" => {
            "h264".to_string()
        }
        "V_MPEGH/ISO/HEVC" => "hevc".to_string(),
        "V_VP8" => "vp8".to_string(),
        "V_VP9" => "vp9".to_string(),
        "V_AV1" => "av1".to_string(),
        "V_MPEG2" => "mpeg2video".to_string(),
        "A_OPUS" => "opus".to_string(),
        "A_VORBIS" => "vorbis".to_string(),
        "A_AAC" => "aac".to_string(),
        "A_FLAC" => "flac".to_string(),
        other => other.to_string(),
    }
}
