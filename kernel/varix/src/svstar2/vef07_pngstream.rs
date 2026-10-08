//! VE-F1007 · PNG 流式解码（增量接口，目标 380 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F1007`
//!
//! **判据（锚点原文逐条）**：
//! - 增量接口（`feed(chunk)` → 逐段解码 → 进度回调）→ `C07-FEED-*`
//! - **chunk 大小无关性**（1 字节到 1MB 切分结果一致）→ `C07-CHUNK-*`
//! - 行级回调（每完成一行即回调，渐进显示支持；异步派发不阻塞解码）→ `C07-ROW-*`
//! - 部分图像输出（非隔行自上而下；Adam7 逐遍低分辨率）→ `C07-PROG-*`
//! - 状态机 签名→header→中间块→图像数据→结束，全状态可中断可恢复 → `C07-STATE-*`
//! - 中断=保存块游标与 inflate 状态，恢复=从游标续解（**inflate 状态序列化**）→ `C07-SNAP-*`
//! - 边界（chunk 内多块 / 块跨 chunk 边界，缓冲器无缝拼接）→ `C07-SEAM-*`
//! - 非法数据流中段 → 已解码部分输出 + 三要素错误（不丢弃已完成工作）→ `C07-ERR-*`
//! - 性能（流式开销 ≤5%）→ `C07-COST-*`
//!
//! **职责定位（锚点原文）**：本模块只做**流式会话**——把「一个已经整份到手的
//! PNG 文件」换成「任意切分的字节流」。块语义、位深×颜色类型合法表、调色板、
//! tRNS、反滤波与色彩展开全部复用 F1001（`vef01_pngdec`）与 F1003
//! （`vef03_adam7`），**不另立一套解析**——另立一套必然与 F1001 分叉。
//!
//! **为什么必须自带可恢复 inflate（诚实登记，非重复造轮子）**：
//! 上游 `perfstar::mech_inflate::BitReader` 是**一次性**的：它持有 `&[u8]` 借用切片、
//! 上限 `IDAT_MAX = 16` 片、且**不暴露任何可序列化的中间状态**。而锚点要求
//! 「中断=保存块游标与 inflate 状态，恢复=从游标续解——inflate 状态序列化支持」。
//! 这三条与「一次性 + 借用 + 16 片上限」**直接冲突**，无法在不改上游的前提下满足。
//! 而上游属 mech 域，跨域改动会与并行会话冲突。故本模块在 PNG 域内实现
//! **`ResumableZlib`**：位缓冲、字节游标、块相位、码长表、符号解码累加器、
//! 待办动作全部是 POD，可逐字段序列化。
//! **对拍兜底**：自检 `C07-XCHECK-*` 用上游 `zlib_inflate_slices` 作**独立判据源**
//! 逐字节比对本模块 inflate 的输出——自研实现不因"自研"而免于被上游证伪。
//!
//! **chunk 大小无关性为何是结构性保证而非测试运气**：本模块**不存在**任何
//! "本次 feed 读了多少"的跨调用状态。块帧拼接器只在**凑齐一个完整块**（12+len
//! 字节）时才吐块；inflate 只按**绝对字节偏移**从 IDAT 合并缓冲取数，不假设
//! 任何一次调用的边界。因此 1 字节 × N 次与 1MB × 1 次走的是同一条状态机轨迹，
//! 结果必然一致——判据覆盖 1/2/3/7/13/64/4096/65536/1MB 九种切分做实证。
//!
//! **进度回调的诚实口径**：内核无线程，「回调在解码线程异步派发不阻塞解码」
//! 在本实现里落地为 **`RowQueue`（有界环形队列 + 溢出计数）**：解码侧只做
//! 入队（O(1)、无分配、可预算），消费侧在自身节奏上 `drain()`。这是单核内核上
//! 唯一诚实且可测的"异步派发"形态——不假装有线程，也不让回调阻塞解码循环。
//!
//! **性能判据的诚实口径**：内核自检无可靠时钟，**不谎报 wall-clock**。
//! `C07-COST-*` 测的是**字节流量口径**的流式开销：分帧器扫过的字节数相对
//! 有效载荷的超出比例（含每次 feed 的固定开销摊销），判据为 ≤5%。
//! 真实耗时基准属 F1016 专项，此处不越界。
//!
//! **跨批对接点**：上游 F1001（块语义/色彩展开/反滤波）、F1003（Adam7 七遍几何）；
//! 下游 F1013（内存治理：行级输出不整图驻留）、F1016（性能基准）、
//! **F1019（缩略图快路径：隔行逐遍低分辨率 + 覆盖图共用本模块的 `cov` 面）**、
//! F1059（语义化错误：不支持 vs 已损坏）。
//!
//! **确定性**：同输入同输出（纯函数式状态机，无时钟无 IO，遍历序固定）。
//! 零外部依赖，只用 `alloc` 与 `crate::checks`（自检侧）。

use crate::perfstar::imgsimd;
use crate::perfstar::mech_inflate;
use crate::svstar2::vef01_pngdec as dec;
use crate::svstar2::vef03_adam7 as ad7;

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

// ===========================================================================
// 一、错误面（五元组：码/原因/建议/人话/详情）
// ===========================================================================

/// PNG **流式**会话故障类别。
///
/// **码段归属**：F1001 已占 `0xF101..=0xF113`（19 个 `FaultKind` 变体，
/// `code() = 0xF100 | (self as u16) + 1`）。F1007 起自 **`0xF201`**，
/// 与既有码段**不重叠**——`dec::FaultKind` 与本枚举混用时不会撞码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StreamFaultKind {
    /// 块长度域超出流式上界（防止 `12+len` 在 32 位目标溢出）。
    ChunkTooLarge,
    /// 块 CRC 不符且该块为 critical（流式下仍按规范拒绝，但已出行的不丢）。
    CriticalCrc,
    /// 恢复点版本不符（拿旧快照喂新状态机——显性拒绝，不猜）。
    SnapshotVersion,
    /// 恢复点结构损坏（截断/长度越界）。
    SnapshotCorrupt,
    /// inflate 码表过订阅或不足（码长数组非法）。
    TableCorrupt,
    /// inflate 到达非法块类型（BTYPE=11）。
    BadBlockType,
    /// inflate 长度/距离码越界（码位 286+ / 30+）。
    BadSymbol,
    /// inflate 距离超出已产出窗口（回参考越界）。
    DistTooFar,
    /// zlib 头非法（CM≠8 或 FCHECK 不符）。
    ZlibHeader,
    /// Adler-32 与展开结果不符。
    AdlerMismatch,
    /// 输出缓冲越界（原始展开区放不下）。
    OutputFull,
    /// 签名阶段就收到非签名字节。
    NotSignature,
    /// IHDR 之前出现 IDAT/IEND 之外的非法次序。
    ChunkOrder,
    /// 恢复时快照与当前会话的尺寸/布局不一致。
    SnapshotMismatch,
    /// 队列溢出且策略为拒绝（默认策略为丢弃最旧，不走此路径——保留码位空洞）。
    QueueOverflow,
}

impl StreamFaultKind {
    /// 错误码（F 域 PNG **流式**子段，自 F1007 起，与 F1001 子段不重叠）。
    pub fn code(self) -> u16 {
        0xF200 + (self as u16) + 1
    }
    /// 简短中文名（诊断面文案）。
    pub fn label(self) -> &'static str {
        match self {
            StreamFaultKind::ChunkTooLarge => "块长度越界",
            StreamFaultKind::CriticalCrc => "critical 块 CRC 错",
            StreamFaultKind::SnapshotVersion => "恢复点版本不符",
            StreamFaultKind::SnapshotCorrupt => "恢复点损坏",
            StreamFaultKind::TableCorrupt => "inflate 码表非法",
            StreamFaultKind::BadBlockType => "非法 deflate 块类型",
            StreamFaultKind::BadSymbol => "非法长度/距离码",
            StreamFaultKind::DistTooFar => "回参考距离越界",
            StreamFaultKind::ZlibHeader => "zlib 头非法",
            StreamFaultKind::AdlerMismatch => "Adler-32 不符",
            StreamFaultKind::OutputFull => "输出缓冲已满",
            StreamFaultKind::NotSignature => "签名不符",
            StreamFaultKind::ChunkOrder => "块次序非法",
            StreamFaultKind::SnapshotMismatch => "恢复点与会话不匹配",
            StreamFaultKind::QueueOverflow => "队列溢出",
        }
    }
    /// 原因（为什么发生）。
    pub fn cause(self) -> &'static str {
        match self {
            StreamFaultKind::ChunkTooLarge => "块长度域超过流式上界，12+len 的加法在 32 位目标有溢出风险",
            StreamFaultKind::CriticalCrc => "critical 块 CRC 与声明值不符——按 PNG 规范必须拒绝该块",
            StreamFaultKind::SnapshotVersion => "恢复点的格式版本与本模块当前版本不一致",
            StreamFaultKind::SnapshotCorrupt => "恢复点字节流被截断或字段长度越界",
            StreamFaultKind::TableCorrupt => "动态 Huffman 码长数组过订阅或不满足前缀码完备性",
            StreamFaultKind::BadBlockType => "deflate 块类型为 11（规范保留值）",
            StreamFaultKind::BadSymbol => "长度码 ≥286 或距离码 ≥30，超出 RFC1951 定义域",
            StreamFaultKind::DistTooFar => "回参考距离大于已产出字节数",
            StreamFaultKind::ZlibHeader => "CMF 低四位非 8，或 CMF/FLG 组合的 31 倍数校验失败",
            StreamFaultKind::AdlerMismatch => "展开字节流的 Adler-32 与流尾声明值不符",
            StreamFaultKind::OutputFull => "原始展开区容量不足，无法继续产出扫描线",
            StreamFaultKind::NotSignature => "签名阶段收到的 8 字节与 PNG 魔数不符",
            StreamFaultKind::ChunkOrder => "IHDR 必须是首块；IDAT 必须连续；IEND 之后不再接受块",
            StreamFaultKind::SnapshotMismatch => "快照记录的尺寸/行跨度与恢复时会话推导值不同",
            StreamFaultKind::QueueOverflow => "有界队列已满且溢出策略为拒绝",
        }
    }
    /// 建议（调用方下一步该做什么——三要素之三）。
    pub fn advice(self) -> &'static str {
        match self {
            StreamFaultKind::ChunkTooLarge => "按 PNG 规范核对块长度；IDAT 负载上限见 IDAT_BYTES_MAX",
            StreamFaultKind::CriticalCrc => "向编码方核对文件；已解码的行仍可用 partial_output 取回",
            StreamFaultKind::SnapshotVersion | StreamFaultKind::SnapshotCorrupt => "丢弃该恢复点，从头重解；不要跨版本复用快照",
            StreamFaultKind::TableCorrupt => "按损坏处理——该 IDAT 之后的行不再可信",
            StreamFaultKind::BadBlockType | StreamFaultKind::BadSymbol | StreamFaultKind::DistTooFar => {
                "按 zlib 流损坏处理；此前已解出的行仍会交付（见 feed 返回的 fault 面）"
            }
            StreamFaultKind::ZlibHeader => "确认 IDAT 载荷确为 zlib 封装而非裸 deflate",
            StreamFaultKind::AdlerMismatch => "按损坏处理；已出行部分仍有效，可交给渐进显示面",
            StreamFaultKind::OutputFull => "提高输出上限，或改走 F1013 分块解码降低峰值",
            StreamFaultKind::NotSignature => "确认输入确为 PNG，或按 F1059 语义区分「不支持」与「已损坏」",
            StreamFaultKind::ChunkOrder => "按 PNG 规范重排块序",
            StreamFaultKind::SnapshotMismatch => "确认恢复的是同一个会话；不同尺寸必须重新解",
            StreamFaultKind::QueueOverflow => "提高队列容量，或改用 Latest 溢出策略",
        }
    }
}

/// 流式会话故障（五元组齐全；`feed` 报错时**不丢已出行**）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StreamFault {
    /// 故障类别。
    pub kind: StreamFaultKind,
    /// 出错块类型（四字节 ASCII，非可打印者归零）。
    pub chunk: [u8; 4],
    /// 附加数值（语义按 `kind` 解释）。
    pub detail_a: u64,
    /// 附加数值。
    pub detail_b: u64,
    /// 故障发生时**已交付的行数**——不丢弃已完成工作的自证面。
    ///
    /// 调用方据此知道 `partial_output` 里有多少行是可信的。
    pub rows_done: u32,
}

impl StreamFault {
    /// 构造故障。
    pub fn new(kind: StreamFaultKind) -> StreamFault {
        StreamFault { kind, chunk: [0; 4], detail_a: 0, detail_b: 0, rows_done: 0 }
    }
    /// 附加块名（非 ASCII 字节归零，避免诊断面出现乱码）。
    pub fn at_chunk(mut self, fourcc: &[u8]) -> StreamFault {
        for i in 0..4 {
            self.chunk[i] = match fourcc.get(i).copied() {
                Some(b) if (0x20..0x7F).contains(&b) => b,
                _ => 0,
            };
        }
        self
    }
    /// 附加两个数值。
    pub fn with(mut self, a: u64, b: u64) -> StreamFault {
        self.detail_a = a;
        self.detail_b = b;
        self
    }
    /// 盖上「已交付行数」。
    pub fn at_rows(mut self, rows: u32) -> StreamFault {
        self.rows_done = rows;
        self
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
    /// 人话（模板 + 三元细节，不含裸码）。
    pub fn human(&self) -> String {
        let mut s = String::new();
        s.push_str(self.kind.label());
        let c = core::str::from_utf8(&self.chunk).unwrap_or("????");
        match self.kind {
            StreamFaultKind::CriticalCrc => {
                s.push_str("：块 ");
                s.push_str(c);
                s.push_str(" 算得 ");
                s.push_str(&fmt_u64(self.detail_a as u64));
                s.push_str("，声明 ");
                s.push_str(&fmt_u64(self.detail_b as u64));
            }
            StreamFaultKind::AdlerMismatch => {
                s.push_str("：算得 ");
                s.push_str(&fmt_u64(self.detail_a as u64));
                s.push_str("，声明 ");
                s.push_str(&fmt_u64(self.detail_b as u64));
            }
            StreamFaultKind::ChunkTooLarge => {
                s.push_str("：声明 ");
                s.push_str(&fmt_u64(self.detail_a));
                s.push_str("，上限 ");
                s.push_str(&fmt_u64(self.detail_b));
            }
            StreamFaultKind::DistTooFar => {
                s.push_str("：距离 ");
                s.push_str(&fmt_u64(self.detail_a));
                s.push_str("，已产出仅 ");
                s.push_str(&fmt_u64(self.detail_b));
            }
            StreamFaultKind::SnapshotCorrupt | StreamFaultKind::SnapshotVersion => {
                s.push_str("（恢复点不可用，已出行部分仍有效）");
            }
            _ => {}
        }
        s.push_str("；已交付 ");
        s.push_str(&fmt_u64(self.rows_done as u64));
        s.push_str(" 行");
        s
    }
}

impl core::fmt::Display for StreamFault {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.human())
    }
}

/// 无 `format!` 依赖的十进制渲染（内核 no_std + 禁宏分配的稳妥写法）。
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

// ===========================================================================
// 二、常量表（RFC1951 / RFC1950 / PNG 规范 —— 唯一来源，散落即错）
// ===========================================================================

/// PNG 魔数（规范 §2.1）。
pub const PNG_SIG: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// 块帧固定开销：长度 4 + 类型 4 + CRC 4。
pub const CHUNK_OVERHEAD: usize = 12;

/// IDAT 单块长度域上界（压缩态字节）。
///
/// 与 F1001 的 `IDAT_BYTES_MAX` 同值同口径（256MB）：PNG 单块的 31 位长度域
/// 上限过大，直接相加 `12 + len` 在 32 位目标会溢出，故流式必须先卡上界。
pub const IDAT_BYTES_MAX: usize = 256 * 1024 * 1024;

/// 长度码基准值（码 257..285，29 项，RFC1951 §3.2.5 表）。
pub const LEN_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227,
    258,
];
/// 长度码附加位数（与 [`LEN_BASE`] 同序）。
pub const LEN_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
/// 距离码基准值（码 0..29，30 项，RFC1951 §3.2.5）。
pub const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073,
    4097, 6145, 8193, 12289, 16385, 24577,
];
/// 距离码附加位数（与 [`DIST_BASE`] 同序）。
pub const DIST_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13,
];
/// 码长码的传输序（19 项，RFC1951 §3.2.7）。
pub const CL_ORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];
/// 码长码的固定发送位数（每项 3 位）。
pub const CL_BITS: u8 = 3;
/// 长度码数量（码 257..285）。
pub const NLEN_CODES: usize = 29;
/// 距离码数量（码 0..29）。
pub const NDIST_CODES: usize = 30;
/// 字面量/长度码表槽数（码 0..285）——**动态表（BTYPE=10）的上限**。
pub const NLIT_CODES: usize = 286;
/// **fixed 表（BTYPE=01）的字面量/长度码槽数（码 0..287）**。
///
/// **必须是 288 而不是 286**：RFC1951 §3.2.6 的固定码把符号 280..287
/// 也编成 8 位码（8 个），于是长度 8 的码字共 `144 + 8 = 152` 个，
/// 9 位码的起点 `(48 + 152) << 1 = 400 (0x190)` —— 与规范一致。
/// 若只建 286 项，长度 8 只有 150 个码，9 位起点前移到 396，
/// **所有 144..255 的字面量都会解成偏 4 的错符号**
/// （实测 0xAB(171) 被解成 175，且后续 Match/距离连锁全错）。
/// 两个常量必须分开：动态表按规范只到 285，fixed 表必须到 287。
pub const NFIXED_LIT: usize = 288;
/// fixed 表的距离码槽数（码 0..31；30/31 无意义但占位以符合规范）。
pub const NFIXED_DIST: usize = 32;
/// Huffman 码最大长度（位）。
pub const MAX_CODE_BITS: u8 = 15;
/// deflate 回参考窗口上限（32KB，RFC1951 §3.2.5）。
pub const WINDOW_MAX: usize = 32768;
/// deflate 单次回参考最大长度（码 285）。
pub const MATCH_MAX: u16 = 258;

// ===========================================================================
// 三、快照序列化（inflate 状态 + 块游标 —— 中断/恢复的载体）
// ===========================================================================

/// 快照格式版本（结构变更即改；不兼容时 [`StreamFaultKind::SnapshotVersion`] 显性拒绝）。
pub const SNAPSHOT_VERSION: u8 = 1;

/// 快照魔数（"VF7S"）。
pub const SNAPSHOT_MAGIC: [u8; 4] = *b"VF7S";

/// 小端字节写入器（`no_std` 下无 `io`；只用 `alloc::vec::Vec`）。
struct W {
    b: Vec<u8>,
}

impl W {
    fn new() -> W {
        W { b: Vec::new() }
    }
    fn u8(&mut self, v: u8) {
        self.b.push(v);
    }
    fn u16(&mut self, v: u16) {
        self.b.extend_from_slice(&v.to_le_bytes());
    }
    fn u32(&mut self, v: u32) {
        self.b.extend_from_slice(&v.to_le_bytes());
    }
    fn u64(&mut self, v: u64) {
        self.b.extend_from_slice(&v.to_le_bytes());
    }
    fn bytes(&mut self, v: &[u8]) {
        self.u32(v.len() as u32);
        self.b.extend_from_slice(v);
    }
}

/// 小端字节读取器（越界即 `None`，绝不 panic）。
struct R<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> R<'a> {
    fn new(b: &'a [u8]) -> R<'a> {
        R { b, at: 0 }
    }
    fn u8(&mut self) -> Option<u8> {
        let v = *self.b.get(self.at)?;
        self.at += 1;
        Some(v)
    }

    fn u16(&mut self) -> Option<u16> {
        let v = self.take(2)?;
        Some(u16::from_le_bytes([v[0], v[1]]))
    }
    fn u32(&mut self) -> Option<u32> {
        let v = self.take(4)?;
        Some(u32::from_le_bytes([v[0], v[1], v[2], v[3]]))
    }
    fn u64(&mut self) -> Option<u64> {
        let v = self.take(8)?;
        let mut a = [0u8; 8];
        a.copy_from_slice(v);
        Some(u64::from_le_bytes(a))
    }
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.at.checked_add(n)?;
        let v = self.b.get(self.at..end)?;
        self.at = end;
        Some(v)
    }
    fn bytes(&mut self) -> Option<Vec<u8>> {
        let n = self.u32()? as usize;
        let v = self.take(n)?;
        Some(v.to_vec())
    }
    /// 剩余字节必须为空（结构损坏的硬闸）。
    fn end_ok(&self) -> bool {
        self.at == self.b.len()
    }
}

// ===========================================================================
// 四、可恢复 zlib（位状态机，全部 POD —— 本模块的核心）
// ===========================================================================

/// inflate 主相位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ZPhase {
    /// 待读 2 字节 zlib 头（CMF/FLG）。
    ZlibHeader,
    /// 待读 BFINAL + BTYPE。
    BlockHeader,
    /// 已定块类型为 stored，待读 LEN/NLEN。
    StoredLen,
    /// stored 块体逐字节拷贝中。
    StoredBody,
    /// 待建固定 Huffman 码表。
    BuildFixed,
    /// 动态码表子状态机。
    BuildDynamic,
    /// 块体符号解码中（lit/dist 已就绪）。
    Symbols,
    /// 末块结束，待读 4 字节 Adler-32。
    Tail,
    /// 全流结束（Adler 已核对）。
    Done,
}

/// 待办动作（把「解码到一半」的三段式固化为显式状态——这是可序列化的关键）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pend {
    /// 无待办。
    None,
    /// 已知长度码，待读附加位凑出总长度。
    ReadLenExtra {
        /// 长度基准值。
        base: u16,
        /// 附加位数。
        extra: u8,
    },
    /// 长度已知，待解距离符号。
    ReadDistSym {
        /// 已定长度。
        len: u16,
    },
    /// 距离码已解，待读附加位。
    ReadDistExtra {
        /// 已定长度。
        len: u16,
        /// 距离基准值。
        base: u16,
        /// 附加位数。
        extra: u8,
    },
    /// 待执行逐字节回参考拷贝（`len` 递减到 0 为止——字节级可恢复）。
    Copy {
        /// 剩余拷贝字节数。
        len: u16,
        /// 回参考距离。
        dist: u16,
    },
}

/// 动态码表子状态机（码长码 → 码长数组）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClPhase {
    /// 待读 HLIT/HDIST/HCLEN。
    Head,
    /// 逐项读 19 个码长码的码长（每项 3 位）。
    CodeLens,
    /// 逐符号读 lit/dist 码长（含 16/17/18 重复码）。
    Lengths,
    /// 码表已建成。
    Ready,
}

/// 码长表构建子状态（全部 POD —— 可序列化）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ClState {
    /// 子相位。
    pub phase: ClPhase,
    /// 字面量/长度码数量（HLIT，已含 +257）。
    pub nlit: u16,
    /// 距离码数量（HDIST，已含 +1）。
    pub ndist: u16,
    /// 码长码数量（HCLEN，已含 +4）。
    pub hclen: u16,
    /// 已收到的码长码长度个数。
    pub cl_got: u8,
    /// 19 个码长码的长度。
    pub cl_sym: [u8; 19],
    /// 正在填充的码长数组下标。
    pub fill_at: u16,
    /// 上一个非零码长（供 16 号重复码使用）。
    ///
    /// **16 号码的合法性判据就是它本身**：`prev_len == 0` 意味着「还没有
    /// 任何非零码长可复制」，此时 16 号码按 RFC1951 §3.2.7 属结构非法。
    /// 故无需额外的 `rep_done` 计数——曾有过一个，是重复逻辑的死码。
    pub prev_len: u8,
}

/// 规范 Huffman 码表（计数 + 符号序，RFC1951 §3.2.2 的规范构造）。
///
/// **不存码字本身**：解码用「逐位比较计数」的经典法（puff 算法），
/// 只需每长度的符号个数与符号升序表即可，故表是 POD，可逐字段序列化。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Huff {
    /// 每个码长的符号个数（`count[len]`）。
    pub count: [u16; (MAX_CODE_BITS as usize) + 1],
    /// 按码长升序、同长内按符号升序排列的符号表。
    ///
    /// 容量取 [`NFIXED_LIT`]（288）而非 [`NLIT_CODES`]（286）：fixed 表
    /// 需要放下符号 287，少 2 个槽会让 9 位码起点前移 4（见该常量注）。
    pub symbol: [u16; NFIXED_LIT],
    /// 有效符号总数。
    pub n: u16,
}

impl Huff {
    /// 空表（全零）。
    pub const fn empty() -> Huff {
        Huff { count: [0; (MAX_CODE_BITS as usize) + 1], symbol: [0; NFIXED_LIT], n: 0 }
    }

    /// 由码长数组构建规范码表（过订阅/不满足完备性即 `None`）。
    ///
    /// 校验口径：左填充后 `left > 0` 即过订阅；`n == 0`（空表）放行——
    /// 全零码长是合法的"无距离码"表（单块全字面量的合法形态）。
    pub fn build(lengths: &[u8]) -> Option<Huff> {
        let mut h = Huff::empty();
        for &l in lengths.iter() {
            if l as usize > MAX_CODE_BITS as usize {
                return None;
            }
            h.count[l as usize] += 1;
            h.n += 1;
        }
        if h.n == 0 {
            return Some(h);
        }
        // 计数越界自检：任一长度计数不得超过总符号数
        let mut total = 0u32;
        for l in 1..=MAX_CODE_BITS as usize {
            total += h.count[l] as u32;
        }
        if total != h.n as u32 {
            return None;
        }
        // 步进累积（left 即"剩余可用码字数"，RFC1951 §3.2.2 的完备性判据）
        let mut left: i32 = 1;
        for l in 1..=MAX_CODE_BITS as usize {
            left <<= 1;
            left -= h.count[l] as i32;
            if left < 0 {
                return None;
            }
        }
        // 偏移表 → 符号表
        let mut offs = [0u16; (MAX_CODE_BITS as usize) + 2];
        for l in 1..MAX_CODE_BITS as usize {
            offs[l + 1] = offs[l] + h.count[l];
        }
        for (sym, &l) in lengths.iter().enumerate() {
            if l != 0 {
                let slot = &mut h.symbol[offs[l as usize] as usize];
                *slot = sym as u16;
                offs[l as usize] += 1;
            }
        }
        Some(h)
    }

    /// 固定 Huffman 字面量/长度码表（RFC1951 §3.2.6）。
    pub fn fixed_lit() -> Huff {
        // 288 项：0..143 → 8位；144..255 → 9位；256..279 → 7位；
        // 280..287 → 8位（**含 286/287 两个占位符，规范如此**）。
        let mut lengths = [0u8; NFIXED_LIT];
        for (i, slot) in lengths.iter_mut().enumerate() {
            *slot = if i < 144 {
                8
            } else if i < 256 {
                9
            } else if i < 280 {
                7
            } else {
                8
            };
        }
        Huff::build(&lengths).unwrap_or(Huff::empty())
    }

    /// 固定 Huffman 距离码表（32 项等长 5 位；30/31 无意义但规范如此）。
    pub fn fixed_dist() -> Huff {
        let lengths = [5u8; NFIXED_DIST];
        Huff::build(&lengths).unwrap_or(Huff::empty())
    }
}

/// 符号解码累加器（逐位法的中间态 —— 必须持久化，否则跨 feed 不可恢复）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Acc {
    /// 当前码字前缀值。
    pub code: u32,
    /// 当前码长的首个码字值。
    pub first: u32,
    /// 当前码长在符号表中的起始下标。
    pub index: u32,
    /// 已读位数。
    pub len: u8,
}

/// inflate 核心状态（**全部可序列化**，无借用、无堆）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ZState {
    /// 主相位。
    pub phase: ZPhase,
    /// 位缓冲（LSB-first，RFC1951 §3.1.1）。
    pub bit_buf: u32,
    /// 位缓冲内有效位数。
    pub bit_cnt: u8,
    /// 已从 IDAT 合并缓冲消费的字节数（**绝对偏移**——不依赖任何 feed 边界）。
    pub src: u32,
    /// 末块标记（BFINAL）。
    pub last: bool,
    /// 当前块的类型（0=stored / 1=固定 / 2=动态）。
    pub btype: u8,
    /// stored 块剩余字节数。
    pub stored_left: u32,
    /// Adler-32 滚动值。
    pub adler: u32,
    /// 已产出字节总数（= 输出缓冲长度，两者恒等；快照里冗余存一份做对账）。
    pub out: u32,
    /// 字面量/长度码长数组（动态表）。
    pub lit_len: [u8; NLIT_CODES],
    /// 距离码长数组（动态表）。
    pub dist_len: [u8; NDIST_CODES],
    /// 码长表构建子状态。
    pub cl: ClState,
    /// 符号解码累加器。
    pub acc: Acc,
    /// 待办动作。
    pub pend: Pend,
    /// 尾随累积字节数（对齐后读 LEN/NLEN 或 adler 用）。
    pub tail_acc: u32,
    /// 读入的 zlib 头字节数（0..2）。
    pub hdr_got: u8,
}

impl ZState {
    /// 初始状态。
    pub fn new() -> ZState {
        ZState {
            phase: ZPhase::ZlibHeader,
            bit_buf: 0,
            bit_cnt: 0,
            src: 0,
            last: false,
            btype: 0,
            stored_left: 0,
            // Adler-32 初值 s1=1, s2=0（RFC1950 §2）。`adler` 按 `s2<<16|s1`
            // 存，故初值恰为 1。
            adler: 1,
            out: 0,
            lit_len: [0; NLIT_CODES],
            dist_len: [0; NDIST_CODES],
            cl: ClState {
                phase: ClPhase::Head,
                nlit: 0,
                ndist: 0,
                hclen: 0,
                cl_got: 0,
                cl_sym: [0; 19],
                fill_at: 0,
                prev_len: 0,
            },
            acc: Acc::default(),
            pend: Pend::None,
            tail_acc: 0,
            hdr_got: 0,
        }
    }
}

impl Default for ZState {
    fn default() -> ZState {
        ZState::new()
    }
}

/// 单次 `pump` 的产出。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct PumpStat {
    /// 本次新产出字节数。
    pub produced: usize,
    /// 是否已到流尾（Adler 核对通过）。
    pub done: bool,
}

/// 可恢复 zlib 解码器。
///
/// **持有码表但码表不进快照**——码表由码长数组**重建**，故快照只存
/// `lit_len`/`dist_len`/`cl`（POD）。这让快照体积与块数无关。
pub struct ResumableZlib {
    /// 可序列化的核心状态。
    pub st: ZState,
    /// 字面量/长度码表（由 `lit_len` 或固定规则派生）。
    lit: Huff,
    /// 距离码表。
    dist: Huff,
    /// 码表是否需要重建（置位时机：动态表建成 / 固定表切换 / 从快照恢复）。
    dirty: bool,
    /// 码长码表的运行期暂存（**不进快照**——由 `cl.cl_sym` 可无损重建，
    /// 故不占快照体积；恢复路径见 [`ResumableZlib::from_state`]）。
    pending_cl_table: Option<Huff>,
}

impl ResumableZlib {
    /// 新建解码器。
    pub fn new() -> ResumableZlib {
        ResumableZlib {
            st: ZState::new(),
            lit: Huff::empty(),
            dist: Huff::empty(),
            dirty: true,
            pending_cl_table: None,
        }
    }

    /// 从快照状态恢复（码表随之重建；码长码表由 `cl_sym` 重算）。
    pub fn from_state(st: ZState) -> ResumableZlib {
        let mut r = ResumableZlib {
            st,
            lit: Huff::empty(),
            dist: Huff::empty(),
            dirty: true,
            pending_cl_table: None,
        };
        // 若快照停在动态表的码长填充途中，码长码表必须重算，
        // 否则 resume 后无法继续解剩余码长——这是"状态恢复完整"的必要一环。
        if r.st.phase == ZPhase::BuildDynamic
            && matches!(r.st.cl.phase, ClPhase::Lengths | ClPhase::CodeLens)
        {
            let n = (r.st.cl.cl_got as usize).min(19);
            let mut sym = [0u8; 19];
            sym[..n].copy_from_slice(&r.st.cl.cl_sym[..n]);
            r.pending_cl_table = Huff::build(&sym);
        }
        r
    }
}

impl Default for ResumableZlib {
    fn default() -> ResumableZlib {
        ResumableZlib::new()
    }
}

impl ResumableZlib {
    /// 拍摄 **inflate 级**恢复点（锚点「inflate 状态序列化支持」的独立入口）。
    ///
    /// 与 [`StreamSession::checkpoint`] 的分工：后者把块游标 + 会话游标 +
    /// 滤波链一起打包（体积大、只在会话边界有意义）；本函数**只**序列化
    /// [`ZState`]，因而可把 inflate 从**任意**中间相位搬到另一处续解——
    /// 包括 `bit_buf` 里压着未取走的位、`acc` 正解到一半的码字、
    /// `Pend` 挂着待办动作的那一瞬。
    ///
    /// 编码与会话级快照共用 [`write_zstate`]，故两侧不可能漂移。
    pub fn snapshot(&self) -> Vec<u8> {
        let mut w = W::new();
        // 定长魔数裸写（走 `w.bytes` 会带 4 字节长度前缀，读端必须对称）
        w.b.extend_from_slice(&SNAPSHOT_MAGIC);
        w.u8(SNAPSHOT_VERSION);
        write_zstate(&mut w, &self.st);
        w.b
    }

    /// 从 inflate 级恢复点重建解码器（码表由 [`ResumableZlib::from_state`] 重建）。
    ///
    /// 尾部必须恰好读完（多余/缺失字节 → `SnapshotCorrupt`），不做半初始化。
    pub fn from_snapshot(snap: &[u8]) -> Result<ResumableZlib, StreamFault> {
        let mut r = R::new(snap);
        let magic = match r.take(SNAPSHOT_MAGIC.len()) {
            Some(m) => m,
            None => return Err(StreamFault::new(StreamFaultKind::SnapshotCorrupt)),
        };
        if magic != SNAPSHOT_MAGIC {
            return Err(StreamFault::new(StreamFaultKind::SnapshotCorrupt));
        }
        match r.u8() {
            Some(SNAPSHOT_VERSION) => {}
            _ => return Err(StreamFault::new(StreamFaultKind::SnapshotVersion)),
        }
        let st = match read_zstate(&mut r) {
            Some(v) => v,
            None => return Err(StreamFault::new(StreamFaultKind::SnapshotCorrupt)),
        };
        if !r.end_ok() {
            return Err(StreamFault::new(StreamFaultKind::SnapshotCorrupt));
        }
        Ok(ResumableZlib::from_state(st))
    }

    /// 已产出字节数（= 输出缓冲长度）。
    pub fn produced(&self) -> u32 {
        self.st.out
    }

    /// 是否解码完成。
    pub fn is_done(&self) -> bool {
        self.st.phase == ZPhase::Done
    }

    /// 码表就绪（`dirty` 时重建）。
    fn ensure_tables(&mut self) -> Result<(), StreamFault> {
        if !self.dirty {
            return Ok(());
        }
        match self.st.btype {
            1 => {
                self.lit = Huff::fixed_lit();
                self.dist = Huff::fixed_dist();
            }
            2 => {
                let nlit = self.st.cl.nlit as usize;
                let ndist = self.st.cl.ndist as usize;
                if nlit == 0 || nlit > NLIT_CODES || ndist == 0 || ndist > NDIST_CODES {
                    return Err(StreamFault::new(StreamFaultKind::TableCorrupt).with(nlit as u64, ndist as u64));
                }
                self.lit = match Huff::build(&self.st.lit_len[..nlit]) {
                    Some(t) => t,
                    None => return Err(StreamFault::new(StreamFaultKind::TableCorrupt)),
                };
                self.dist = match Huff::build(&self.st.dist_len[..ndist]) {
                    Some(t) => t,
                    None => return Err(StreamFault::new(StreamFaultKind::TableCorrupt)),
                };
            }
            _ => return Err(StreamFault::new(StreamFaultKind::TableCorrupt)),
        }
        self.dirty = false;
        Ok(())
    }

    /// 取 `n` 位（LSB-first）。输入不足返回 `None`（**状态保持不变，可续**）。
    #[inline]
    fn bits(&mut self, src: &[u8], n: u8) -> Option<u32> {
        debug_assert!(n <= 16);
        while self.st.bit_cnt < n {
            let b = *src.get(self.st.src as usize)?;
            self.st.src += 1;
            self.st.bit_buf |= (b as u32) << self.st.bit_cnt;
            self.st.bit_cnt += 8;
        }
        let mask = if n == 32 { u32::MAX } else { (1u32 << n) - 1 };
        let v = self.st.bit_buf & mask;
        self.st.bit_buf >>= n;
        self.st.bit_cnt -= n;
        Some(v)
    }

    /// 字节对齐（丢弃当前字节内剩余位——stored 块与尾随 adler 前必做）。
    #[inline]
    fn align(&mut self) {
        self.st.bit_buf = 0;
        self.st.bit_cnt = 0;
    }

    /// 逐位解一个 Huffman 符号（累加器持久化 → 跨 feed 可恢复）。
    ///
    /// **码表由 `which` 选择而非以参数传入**：若写成 `decode_sym(&self.lit, src)`
    /// 会同时持有 `&self`（取码表）与 `&mut self`（改累加器），借用检查直接
    /// 拒绝。选择器在函数体内取表，别名从源头消除。
    fn decode_sym(&mut self, which: u8, src: &[u8]) -> Option<Result<u16, StreamFault>> {
        loop {
            let b = match self.bits(src, 1) {
                Some(v) => v,
                // 输入不足：累加器原样留在 st.acc 里，下次从这里续
                None => return None,
            };
            self.st.acc.code |= b;
            self.st.acc.len += 1;
            let len = self.st.acc.len as usize;
            if len > MAX_CODE_BITS as usize {
                self.st.acc = Acc::default();
                return Some(Err(StreamFault::new(StreamFaultKind::BadSymbol)));
            }
            let h = match which {
                0 => &self.lit,
                1 => &self.dist,
                _ => match self.pending_cl_table {
                    Some(ref t) => t,
                    None => {
                        self.st.acc = Acc::default();
                        return Some(Err(StreamFault::new(StreamFaultKind::TableCorrupt)));
                    }
                },
            };
            let count = h.count[len] as u32;
            if self.st.acc.code >= self.st.acc.first
                && self.st.acc.code - self.st.acc.first < count
            {
                let sym = h.symbol[(self.st.acc.index + (self.st.acc.code - self.st.acc.first)) as usize];
                self.st.acc = Acc::default();
                return Some(Ok(sym));
            }
            self.st.acc.index += count;
            self.st.acc.first = (self.st.acc.first + count) << 1;
            self.st.acc.code <<= 1;
        }
    }

    /// Adler-32 增量更新（A=1 起算；此处自研以免在流式热路径上依赖上游函数）。
    #[inline]
    /// 单字节推进 Adler-32。
    ///
    /// **布局约定（与上游 `mech_inflate::adler32`逐位一致，判据要与之对拍）**：
    /// `st.adler` 存 `s2<<16 | s1`（s1 =逐字节和，s2 = s1 的累积和），
    /// 线上传**大端** 4 字节，故 `Tail` 的大端累积值与 `st.adler` 逐位相等。
    ///
    /// 写成 `(s1<<16)|s2` 会与上游对不上，且在数据量大时呈现为
    /// 「AdlerMismatch 但 got/want 都像合法值」——极难定位。
    fn adler_step(&mut self, b: u8) {
        const MOD: u32 = 65521;
        let s1 = self.st.adler & 0xFFFF;
        let s2 = self.st.adler >> 16;
        let s1 = (s1 + b as u32) % MOD;
        let s2 = (s2 + s1) % MOD;
        self.st.adler = (s2 << 16) | s1;
    }

    /// 推进解码，直到产出 `cap` 字节、输入耗尽（`Need`）、或流尾。
    ///
    /// `out` 是**已产出**的原始扫描线缓冲（回参考直接读它，故天然自带窗口历史）。
    ///
    /// **调用契约（`src` 语义，务必按此调用）**：`src` 是**当前完整缓冲**，
    /// `st.src` 是其内的读游标。位流状态机在跨feed 续解时要回到任意历史
    /// 位置取位（`bit_buf` 里还压着上一次没取够的位），故**不能只传增量
    /// 视图**——那会让 `st.src` 越界、解码静默停滞。「逐字节喂」的正确
    /// 模拟是**让缓冲每次只增长 1 字节**。`StreamSession` 传的
    /// `&self.idat`（IDAT 合并缓冲）正是此语义。
    ///
    /// **`st.out` 在此统一同步**（出口唯一一处）：内部推进有十余条
    /// 「输入不足」的提前返回路径，若让各路径自报 `st.out`，漏一条就
    /// 让对外的 `produced()` 撒谎——而 `produced()` 是快照/续解的
    /// 自洽基线，撒谎即等价于「续解会重复或漏数据」。故结构上收口。
    pub fn pump(&mut self, src: &[u8], out: &mut Vec<u8>, cap: usize) -> Result<PumpStat, StreamFault> {
        if cap == 0 {
            return Ok(PumpStat::default());
        }
        let r = self.pump_inner(src, out, cap);
        self.st.out = out.len() as u32;
        r
    }

    /// `pump` 的实际推进循环（出口不写 `st.out`，由 `pump` 收口）。
    fn pump_inner(
        &mut self,
        src: &[u8],
        out: &mut Vec<u8>,
        cap: usize,
    ) -> Result<PumpStat, StreamFault> {
        let mut stat = PumpStat::default();
        // **码表就绪闸必须开在「待办动作」之前**。
        //
        // `ensure_tables()` 原先只在 `BuildFixed`/`Symbols` 两个相位体内调用，
        // 而「待办动作」块位于主相位 `match` **之前**。于是快照恢复出的状态
        // 若停在 `Pend::ReadDistSym`（待解距离符号），恢复后第一件事就是
        // `decode_sym(1, …)` —— 它用 `from_state` 留下的**空表**解符号，
        // 必然解出越界码，抛 `BadSymbol`；而 `bit_cnt`/`pend` 都对，
        // 前缀输出也逐字节正确，故**不报错前缀、只在续解尾段崩**。
        //
        // 实测（VE-F1007 判据 C07-XCHECK-09 的 pend 口径）：
        //   probe=214 快照 → 恢复 → 续解 → `BadSymbol`，少解 11 字节。
        // cap 口径（停在 `Symbols` 相位）154/154 全过，正是因为它绕开了这条路。
        //
        // 修法：`dirty` 是「码表待重建」的唯一权威位，在循环入口冲一次即可
        // ——`ensure_tables` 自身首行即 `if !self.dirty { return Ok(()) }`，
        // 故常态路径零开销，且不改变任何既有推进顺序。
        //
        // `btype != 0` 这道门不可省：新建解码器 `dirty=true` 且 `btype=0`
        // （块类型尚未读到），此时抢跑 `ensure_tables` 会落进 `_ => TableCorrupt`
        // 分支，把**每一个**正常流的开局都判成损坏（实测 baseline 直接 out=0）。
        // 而 `btype==0`（stored）本就不查表，故这道门既安全又必要。
        if self.dirty && self.st.btype != 0 {
            self.ensure_tables()?;
        }
        loop {
            if stat.produced >= cap {
                return Ok(stat);
            }
            // ---- 待办优先：把「解码到一半」的三段式做完 ----
            match self.st.pend {
                Pend::Copy { mut len, dist } => {
                    // `out` 与 `self` 是两个独立对象，但 `self.adler_step` 需
                    // `&mut self`，而 `out` 是 `&mut Vec` 参数——二者不冲突。
                    // 此处唯一的别名风险是同时读 `out.len()` 与写 `out`，
                    // 故先把长度取到局部再进循环。
                    if dist == 0 || dist as usize > out.len() {
                        self.st.pend = Pend::None;
                        return Err(StreamFault::new(StreamFaultKind::DistTooFar)
                            .with(dist as u64, out.len() as u64));
                    }
                    let d = dist as usize;
                    // 逐字节拷（overlap 时语义与 RFC1951 §3.2.5 一致：自引用按序扩展）
                    while len > 0 {
                        let b = out[out.len() - d];
                        out.push(b);
                        self.adler_step(b);
                        len -= 1;
                        stat.produced += 1;
                        if stat.produced >= cap {
                            self.st.pend = Pend::Copy { len, dist: d as u16 };
                            self.st.out = out.len() as u32;
                            return Ok(stat);
                        }
                    }
                    self.st.pend = Pend::None;
                    continue;
                }
                Pend::ReadLenExtra { base, extra } => {
                    let v = match self.bits(src, extra) {
                        Some(v) => v,
                        None => return Ok(stat),
                    };
                    self.st.pend = Pend::ReadDistSym { len: base.wrapping_add(v as u16) };
                    continue;
                }
                Pend::ReadDistSym { len } => {
                    let sym = match self.decode_sym(1, src) {
                        Some(r) => match r {
                            Ok(v) => v,
                            Err(e) => return Err(e),
                        },
                        None => return Ok(stat),
                    };
                    if sym as usize >= NDIST_CODES {
                        return Err(StreamFault::new(StreamFaultKind::BadSymbol).with(sym as u64, NDIST_CODES as u64));
                    }
                    self.st.pend = Pend::ReadDistExtra {
                        len,
                        base: DIST_BASE[sym as usize],
                        extra: DIST_EXTRA[sym as usize],
                    };
                    continue;
                }
                Pend::ReadDistExtra { len, base, extra } => {
                    let v = match self.bits(src, extra) {
                        Some(v) => v,
                        None => return Ok(stat),
                    };
                    let dist = base.wrapping_add(v as u16);
                    self.st.pend = Pend::Copy { len, dist };
                    continue;
                }
                Pend::None => {}
            }

            // ---- 码表构建子状态（动态块）----
            if self.st.btype == 2
                && matches!(self.st.cl.phase, ClPhase::Head | ClPhase::CodeLens | ClPhase::Lengths)
            {
                if !self.step_build_dynamic(src)? {
                    return Ok(stat);
                }
                continue;
            }

            // ---- 主相位 ----
            match self.st.phase {
                ZPhase::Done => {
                    stat.done = true;
                    return Ok(stat);
                }
                ZPhase::ZlibHeader => {
                    while self.st.hdr_got < 2 {
                        let b = match src.get(self.st.src as usize) {
                            Some(&b) => b,
                            None => return Ok(stat),
                        };
                        self.st.src += 1;
                        if self.st.hdr_got == 0 {
                            if b & 0x0F != 8 {
                                return Err(StreamFault::new(StreamFaultKind::ZlibHeader).with(b as u64, 8));
                            }
                        } else {
                            let cmf = self.st.tail_acc as u32;
                            let check = ((cmf as u16) << 8 | b as u16) % 31;
                            if check != 0 {
                                return Err(StreamFault::new(StreamFaultKind::ZlibHeader)
                                    .with(((cmf as u16) << 8 | b as u16) as u64, 0));
                            }
                            // FLG 的 FDICT 位（bit5）置位意味着有 4 字节字典 ID ——
                            // PNG 规范不使用预置字典，显性拒绝而非静默跳过。
                            if b & 0x20 != 0 {
                                return Err(StreamFault::new(StreamFaultKind::ZlibHeader).with(b as u64, 0x20));
                            }
                        }
                        self.st.tail_acc = ((self.st.tail_acc as u32) << 8 | b as u32) as u32 & 0xFFFF;
                        self.st.hdr_got += 1;
                    }
                    self.st.tail_acc = 0;
                    self.st.hdr_got = 0;
                    self.st.phase = ZPhase::BlockHeader;
                    continue;
                }
                ZPhase::BlockHeader => {
                    let v = match self.bits(src, 3) {
                        Some(v) => v,
                        None => return Ok(stat),
                    };
                    self.st.last = v & 1 != 0;
                    let bt = (v >> 1) & 3;
                    if bt == 3 {
                        return Err(StreamFault::new(StreamFaultKind::BadBlockType).with(3, 0));
                    }
                    self.st.btype = bt as u8;
                    self.st.acc = Acc::default();
                    self.st.pend = Pend::None;
                    match bt {
                        0 => {
                            self.align();
                            // `stored_left` 兼作 StoredLen 相位的字节计数，
                            // 故进相位前必须清零（上一块残留的长度值会立刻越界）。
                            self.st.stored_left = 0;
                            self.st.phase = ZPhase::StoredLen;
                        }
                        1 => {
                            self.st.phase = ZPhase::BuildFixed;
                        }
                        _ => {
                            self.st.cl = ClState {
                                phase: ClPhase::Head,
                                nlit: 0,
                                ndist: 0,
                                hclen: 0,
                                cl_got: 0,
                                cl_sym: [0; 19],
                                fill_at: 0,
                                prev_len: 0,
                            };
                            self.st.lit_len = [0; NLIT_CODES];
                            self.st.dist_len = [0; NDIST_CODES];
                            self.st.phase = ZPhase::BuildDynamic;
                        }
                    }
                    continue;
                }
                ZPhase::BuildFixed => {
                    self.ensure_tables()?;
                    self.st.phase = ZPhase::Symbols;
                    continue;
                }
                ZPhase::StoredLen => {
                    // LEN/NLEN 是**小端**（RFC1951 §3.2.4）。
                    //
                    // 这里若图省事用大端累积（像 `Tail` 读 adler 那样），
                    // `len == !nlen` 校验**仍然会通过**——因为逐字节取反与
                    // 字节序反转可交换——但解出的 LEN 成了原值的字节序反转
                    // （40 → 0x2800 = 10240）。不报错、只静默读错长度，
                    // 是最难发现的一类字节序 bug。
                    //
                    // `stored_left` 在本相位**复用为已读字节计数**（进本相位时必为 0），
                    // 这样「已读几字节」本身就是可序列化状态，跨 feed 可续。
                    while self.st.stored_left < 4 {
                        let b = match src.get(self.st.src as usize) {
                            Some(&b) => b,
                            None => return Ok(stat),
                        };
                        self.st.src += 1;
                        self.st.tail_acc |= (b as u32) << (8 * self.st.stored_left);
                        self.st.stored_left += 1;
                    }
                    let len = self.st.tail_acc & 0xFFFF;
                    let nlen = (self.st.tail_acc >> 16) & 0xFFFF;
                    if len != (!nlen & 0xFFFF) {
                        return Err(StreamFault::new(StreamFaultKind::TableCorrupt).with(len as u64, nlen as u64));
                    }
                    self.st.stored_left = len;
                    self.st.tail_acc = 0;
                    self.st.phase = ZPhase::StoredBody;
                    continue;
                }
                ZPhase::StoredBody => {
                    while self.st.stored_left > 0 {
                        let b = match src.get(self.st.src as usize) {
                            Some(&b) => b,
                            None => return Ok(stat),
                        };
                        self.st.src += 1;
                        out.push(b);
                        self.adler_step(b);
                        self.st.stored_left -= 1;
                        stat.produced += 1;
                        if stat.produced >= cap {
                            self.st.out = out.len() as u32;
                            return Ok(stat);
                        }
                    }
                    // 块结束 → 回块头
                    self.st.phase = if self.st.last { ZPhase::Tail } else { ZPhase::BlockHeader };
                    continue;
                }
                ZPhase::BuildDynamic => {
                    if !self.step_build_dynamic(src)? {
                        return Ok(stat);
                    }
                    continue;
                }
                ZPhase::Symbols => {
                    self.ensure_tables()?;
                    let sym = match self.decode_sym(0, src) {
                        Some(r) => match r {
                            Ok(v) => v,
                            Err(e) => return Err(e),
                        },
                        None => return Ok(stat),
                    };
                    let s = sym as usize;
                    if s < 256 {
                        let b = sym as u8;
                        out.push(b);
                        self.adler_step(b);
                        stat.produced += 1;
                        if stat.produced >= cap {
                            self.st.out = out.len() as u32;
                            return Ok(stat);
                        }
                    } else if s == 256 {
                        // 块尾 → 回块头
                        self.st.phase = if self.st.last { ZPhase::Tail } else { ZPhase::BlockHeader };
                    } else if s - 257 < NLEN_CODES {
                        self.st.pend = Pend::ReadLenExtra {
                            base: LEN_BASE[s - 257],
                            extra: LEN_EXTRA[s - 257],
                        };
                    } else {
                        return Err(StreamFault::new(StreamFaultKind::BadSymbol).with(s as u64, 285));
                    }
                    continue;
                }
                ZPhase::Tail => {
                    // 对齐后读 4 字节大端 Adler-32
                    self.align();
                    // 同StoredLen：`stored_left` 兼作已读字节计数。
                    // **不可用 `tail_acc` 计数**——它是累积值，adler 首字节
                    // ≥ 4 时 `tail_acc < 4` 立即为假 → 只读 1 字节就比对。
                    while self.st.stored_left < 4 {
                        let b = match src.get(self.st.src as usize) {
                            Some(&b) => b,
                            None => return Ok(stat),
                        };
                        self.st.src += 1;
                        self.st.tail_acc = (self.st.tail_acc << 8) | b as u32;
                        self.st.stored_left += 1;
                    }
                    let got = self.st.tail_acc;
                    if got != self.st.adler {
                        return Err(StreamFault::new(StreamFaultKind::AdlerMismatch)
                            .with(got as u64, self.st.adler as u64));
                    }
                    self.st.tail_acc = 0;
                    self.st.stored_left = 0;
                    self.st.phase = ZPhase::Done;
                    stat.done = true;
                    self.st.out = out.len() as u32;
                    return Ok(stat);
                }
            }
        }
    }

    /// 动态码表构建。
    ///
    /// 返回值语义：`Ok(true)` = 本轮有进展可继续；`Ok(false)` = 输入不足
    /// （中间态已存盘，可跨 feed 续解）；`Err` = 结构非法。
    ///
    /// **刻意用 `Result` 而不是 bool**：`store_code_len` 的越界信号若与
    /// "输入不足" 共用 bool，会被当成 NeedInput 反复重入同一状态——死循环。
    fn step_build_dynamic(&mut self, src: &[u8]) -> Result<bool, StreamFault> {
        loop {
            match self.st.cl.phase {
                ClPhase::Head => {
                    let hl = match self.bits(src, 5) {
                        Some(v) => v,
                        None => return Ok(false),
                    };
                    let hd = match self.bits(src, 5) {
                        Some(v) => v,
                        None => return Ok(false),
                    };
                    let hc = match self.bits(src, 4) {
                        Some(v) => v,
                        None => return Ok(false),
                    };
                    let nlit = hl as u16 + 257;
                    let ndist = hd as u16 + 1;
                    let hclen = hc as u16 + 4;
                    self.st.cl.nlit = nlit;
                    self.st.cl.ndist = ndist;
                    self.st.cl.hclen = hclen;
                    if nlit as usize > NLIT_CODES || ndist as usize > NDIST_CODES || hclen as usize > 19 {
                        return Err(StreamFault::new(StreamFaultKind::TableCorrupt)
                            .with(nlit as u64, ndist as u64));
                    }
                    self.st.cl.cl_got = 0;
                    self.st.cl.phase = ClPhase::CodeLens;
                }
                ClPhase::CodeLens => {
                    while (self.st.cl.cl_got as usize) < (self.st.cl.hclen as usize) {
                        let v = match self.bits(src, CL_BITS) {
                            Some(v) => v,
                            None => return Ok(false),
                        };
                        self.st.cl.cl_sym[CL_ORDER[self.st.cl.cl_got as usize]] = v as u8;
                        self.st.cl.cl_got += 1;
                    }
                    // 码长码表本身非法 → 结构非法（不是"输入不足"）
                    let clh = match Huff::build(&self.st.cl.cl_sym) {
                        Some(t) => t,
                        None => return Err(StreamFault::new(StreamFaultKind::TableCorrupt)),
                    };
                    self.pending_cl_table = Some(clh);
                    self.st.cl.fill_at = 0;
                    self.st.cl.prev_len = 0;
                    self.st.cl.phase = ClPhase::Lengths;
                }
                ClPhase::Lengths => {
                    if self.pending_cl_table.is_none() {
                        return Err(StreamFault::new(StreamFaultKind::TableCorrupt));
                    }
                    let total = self.st.cl.nlit as usize + self.st.cl.ndist as usize;
                    while (self.st.cl.fill_at as usize) < total {
                        // which=2 → 码长码表（由 pending_cl_table 提供）
                        let sym = match self.decode_sym(2, src) {
                            Some(Ok(v)) => v,
                            Some(Err(e)) => return Err(e),
                            None => return Ok(false),
                        };
                        let s = sym as usize;
                        if s < 16 {
                            let l = sym as u8;
                            self.st.cl.prev_len = l;
                            self.put_len(l)?;
                        } else if s == 16 {
                            // 16 号码复制「上一个码长」；流首无上一个即非法
                            if self.st.cl.prev_len == 0 {
                                return Err(StreamFault::new(StreamFaultKind::TableCorrupt).with(s as u64, 0));
                            }
                            let n = match self.bits(src, 2) {
                                Some(v) => v as u16,
                                None => return Ok(false),
                            };
                            let l = self.st.cl.prev_len;
                            for _ in 0..(n + 3) {
                                self.put_len(l)?;
                            }
                        } else if s == 17 {
                            let n = match self.bits(src, 3) {
                                Some(v) => v as u16,
                                None => return Ok(false),
                            };
                            for _ in 0..(n + 3) {
                                self.put_len(0)?;
                            }
                        } else {
                            let n = match self.bits(src, 7) {
                                Some(v) => v as u16,
                                None => return Ok(false),
                            };
                            for _ in 0..(n + 11) {
                                self.put_len(0)?;
                            }
                        }
                    }
                    self.st.cl.phase = ClPhase::Ready;
                    self.dirty = true;
                    self.st.phase = ZPhase::Symbols;
                    return Ok(true);
                }
                ClPhase::Ready => {
                    self.dirty = true;
                    self.st.phase = ZPhase::Symbols;
                    return Ok(true);
                }
            }
        }
    }

    /// 把一个码长写入 lit/dist 数组（按 `fill_at` 落位）。
    ///
    /// 越界即 `Err`——与"输入不足"严格区分（见 [`ResumableZlib::step_build_dynamic`]）。
    #[inline]
    fn put_len(&mut self, l: u8) -> Result<(), StreamFault> {
        let i = self.st.cl.fill_at as usize;
        if i < self.st.cl.nlit as usize {
            self.st.lit_len[i] = l;
        } else if i < (self.st.cl.nlit as usize + self.st.cl.ndist as usize) {
            self.st.dist_len[i - self.st.cl.nlit as usize] = l;
        } else {
            return Err(StreamFault::new(StreamFaultKind::TableCorrupt).with(i as u64, l as u64));
        }
        self.st.cl.fill_at += 1;
        Ok(())
    }
}

// ===========================================================================
// 五、块帧拼接器（chunk 大小无关性的结构性保证所在）
// ===========================================================================

/// 块帧拼接器：把任意切分的字节流拼成**完整块**。
///
/// **这是「chunk 大小无关性」的物理实现**：块头 8 字节只在
/// [`ChunkStitcher::push`] 里**解析一次**，解析后把 `need`（本块总长）与
/// `scan`（下一未消费帧的起点）记进结构体。后续 feed 只做长度比较——
/// **块头不被重复解析**。若不缓存，1 字节切分时每字节都要重扫 8 字节块头，
/// 开销随块长线性放大（`C07-COST-*` 判据会当场转红）。
///
/// **读/消费分离**：`push` 只累积并记账，**不移动**已就绪帧的字节；
/// 消费端用 [`ChunkStitcher::peek_payload`] / [`ChunkStitcher::peek_crc`]
/// 读取，再用 [`ChunkStitcher::consume`] 推进 `scan`。若在 `push` 里就把帧
/// 从缓冲移走，读侧将永远看不到载荷——这是本实现刻意避免的一类错误。
///
/// 拼接语义（锚点「缓冲器无缝拼接」）：
/// - 一个 feed 内含**多个完整块** → `avail()` 返回块数（逐块处理，非只取一块）。
/// - 一个块**跨多个 feed** → 未满 `need` 时不计入 `avail`，字节留在缓冲。
/// - 块头本身跨 feed → 头未满 8 字节时继续攒（`scan` 不推进）。
#[derive(Clone, Debug, Default)]
pub struct ChunkStitcher {
    /// 未消费的字节（`scan` 之前是已出帧的残留，扫描时跳过）。
    pub staging: Vec<u8>,
    /// 下一未消费帧在 `staging` 中的起点。
    scan: usize,
    /// 当前帧的**总长**（含 12 字节帧开销）；`0` = 块头尚未凑满。
    need: usize,
    /// 当前帧类型。
    ctype: [u8; 4],
    /// 当前帧载荷长度（不含帧开销）。
    clen: usize,
    /// 分帧器**考察过的字节数**（性能判据分子）。
    pub scanned: u64,
    /// 已出帧数。
    pub chunks: u32,
    /// 已消费帧数（`consume` 次数）。
    pub consumed: u32,
}

impl ChunkStitcher {
    /// 新建拼接器。
    pub fn new() -> ChunkStitcher {
        ChunkStitcher::default()
    }

    /// 追加字节（**只累积，不消费**）。
    pub fn push(&mut self, data: &[u8]) {
        if !data.is_empty() {
            self.staging.extend_from_slice(data);
        }
    }

    /// 当前可消费的完整帧数（`0` 或 `1`；消费端反复调 [`ChunkStitcher::consume`]）。
    ///
    /// 单次只返回 0/1 是**刻意的**：帧按序消费，调用方 `while avail > 0`
    /// 逐帧取，天然覆盖"一个 feed 内多块"。
    pub fn avail(&self) -> usize {
        if self.scan + 8 > self.staging.len() {
            return 0;
        }
        if self.need == 0 {
            // 块头已就位但总长未定：试探性算一次（**不记账**，真正解析在 consume）
            let l = u32::from_be_bytes([
                self.staging[self.scan],
                self.staging[self.scan + 1],
                self.staging[self.scan + 2],
                self.staging[self.scan + 3],
            ]) as usize;
            if self.staging.len() - self.scan < l.saturating_add(CHUNK_OVERHEAD) {
                return 0;
            }
        }
        1
    }

    /// 当前帧类型（仅在 [`ChunkStitcher::avail`] 为 1 时有效）。
    #[inline]
    pub fn head_type(&self) -> [u8; 4] {
        let o = self.scan + 4;
        [
            self.staging[o],
            self.staging[o + 1],
            self.staging[o + 2],
            self.staging[o + 3],
        ]
    }

    /// 当前帧载荷长度（仅在 `avail == 1` 时有效）。
    #[inline]
    pub fn payload_len(&self) -> usize {
        u32::from_be_bytes([
            self.staging[self.scan],
            self.staging[self.scan + 1],
            self.staging[self.scan + 2],
            self.staging[self.scan + 3],
        ]) as usize
    }

    /// 当前帧载荷（借用 `staging`，仅在 `avail == 1` 时有效）。
    #[inline]
    pub fn peek_payload(&self) -> &[u8] {
        let o = self.scan + 8;
        &self.staging[o..o + self.payload_len()]
    }

    /// 当前帧 CRC 声明值（大端，仅在 `avail == 1` 时有效）。
    #[inline]
    pub fn peek_crc(&self) -> u32 {
        let o = self.scan + 8 + self.payload_len();
        u32::from_be_bytes([
            self.staging[o],
            self.staging[o + 1],
            self.staging[o + 2],
            self.staging[o + 3],
        ])
    }

    /// 消费当前帧（推进 `scan`，记账 `chunks`/`scanned`）。
    ///
    /// 载荷长度超上界时**拒绝**并把会话置为不可恢复（调用方据返回值处理）。
    pub fn consume(&mut self) -> Result<(), StreamFault> {
        let clen = self.payload_len();
        if clen > IDAT_BYTES_MAX {
            // 块头已在缓冲里但长度域荒谬：不推进 scan，让调用方显性失败。
            self.need = usize::MAX;
            return Err(StreamFault::new(StreamFaultKind::ChunkTooLarge)
                .with(clen as u64, IDAT_BYTES_MAX as u64));
        }
        let total = clen + CHUNK_OVERHEAD;
        // 解析一次块头并缓存 need（后续 feed 不再重扫）。
        // **先取 fourcc 到局部再写 self.ctype**：`self.head_type()` 借 `self`
        // 不可变，而 `copy_from_slice` 要 `&mut self.ctype` —— 直接写在
        // 一行里即别名。取局部变量同时消掉别名。
        let fourcc = self.head_type();
        self.ctype.copy_from_slice(&fourcc);
        self.clen = clen;
        self.scan += total;
        self.chunks += 1;
        self.consumed += 1;
        self.scanned += total as u64;
        // **`need` 必须无条件清零**——它是「下一帧长度已解析」的标志位。
        // 早前只在 `scan == staging.len()` 时清零，等于把「上一帧的长度」
        // 当成「下一帧已就绪」，于是 `avail()` 跳过长度校验直接返回 1，
        // 读侧随即按不存在的载荷越界（实测 panic: len is 34 but the index is 176）。
        // 代价是下一帧的块头要再解析一次（`avail` 里那次是**只读试探**，
        // `scanned` 不记账），故不违反「块头只解析一次」的性能判据。
        self.need = 0;
        self.clen = 0;
        // 全部消费完即回收缓冲（避免 staging 随流增长）
        if self.scan == self.staging.len() {
            self.staging.clear();
            self.scan = 0;
        }
        Ok(())
    }

    /// 未成帧的残留字节数（诊断面：跨 feed 边界的块有多大）。
    #[inline]
    pub fn pending(&self) -> usize {
        self.staging.len().saturating_sub(self.scan)
    }
}

// ===========================================================================
// 六、流式状态机 / 进度 / 行事件 / 接收端
// ===========================================================================

/// 会话相位（锚点原文：签名→header→中间块→图像数据→结束）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SessionPhase {
    /// 攒签名（8 字节）。
    Signature,
    /// 签名已验，等 IHDR 完整块。
    Header,
    /// 中间块（PLTE/tRNS/ancillary）——直到首个 IDAT。
    Chunks,
    /// 图像数据（IDAT 喂入 inflate + 逐行产出）。
    ImageData,
    /// 结束（IEND 已收）。
    End,
    /// 不可恢复故障（已产出的行仍有效）。
    Faulted,
}

impl SessionPhase {
    /// 相位名（诊断面）。
    pub fn label(self) -> &'static str {
        match self {
            SessionPhase::Signature => "签名",
            SessionPhase::Header => "头块",
            SessionPhase::Chunks => "中间块",
            SessionPhase::ImageData => "图像数据",
            SessionPhase::End => "结束",
            SessionPhase::Faulted => "故障",
        }
    }
    /// 稳定序号（断言相位单调推进用；`Faulted` 最大）。
    pub fn ordinal(self) -> u8 {
        match self {
            SessionPhase::Signature => 0,
            SessionPhase::Header => 1,
            SessionPhase::Chunks => 2,
            SessionPhase::ImageData => 3,
            SessionPhase::End => 4,
            SessionPhase::Faulted => 5,
        }
    }
}

/// 进度记录（`on_progress` 的载荷 —— 锚点「进度回调」）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Progress {
    /// 累计喂入字节数。
    pub bytes_in: u64,
    /// 已见块数。
    pub chunks: u32,
    /// IDAT 累计字节数。
    pub idat_bytes: u64,
    /// 已展开的原始扫描线字节数。
    pub raw_bytes: u64,
    /// 已交付行数。
    pub rows: u32,
    /// 全图总行数（IHDR 已知后有效，否则 0）。
    pub total_rows: u32,
    /// 当前相位。
    pub phase: SessionPhase,
    /// 完成百分比（0..=100，按行计；隔行按遍累计像素占比）。
    pub pct: u8,
}

impl Default for SessionPhase {
    /// 默认相位 = 签名（会话的天然起点）。
    fn default() -> SessionPhase {
        SessionPhase::Signature
    }
}

/// 一行完成事件（`on_row` 的载荷 —— 锚点「行级回调」）。
///
/// `col_start`/`col_step` 让消费端自行散射：**隔行时回调给出的是遍内行
/// （长度 = 遍宽×4），消费端按 `col_start + k*col_step` 写回全图**。
/// 非隔行时 `col_step == 1`、`col_start == 0`，语义退化为普通整行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RowEvent<'a> {
    /// Adam7 遍号（非隔行恒为 0）。
    pub pass: usize,
    /// 遍内行号。
    pub pass_row: u32,
    /// 该遍首像素在全图中的行号（隔行=映射后；非隔行=行号本身）。
    pub y: u32,
    /// 该遍首像素在全图中的列号。
    pub col_start: u32,
    /// 列步长（隔行=遍几何的 col_step；非隔行=1）。
    pub col_step: u32,
    /// 该行的 RGBA8（长度 = 遍宽×4）。
    pub rgba: &'a [u8],
}

/// 流式接收端（行级回调 + 进度 + 故障三出口）。
///
/// **回调在解码循环内同步调用但不做派发工作**：真正的"异步派发不阻塞解码"
/// 由 [`RowQueue`] 承担——它实现本 trait 时只做 O(1) 入队，消费侧自行
/// `drain`。本 trait 保持零抽象成本：直连消费端时没有任何中间层。
pub trait StreamSink {
    /// 一行解码完成。返回 `false` 要求提前中止（已出行不丢）。
    fn on_row(&mut self, ev: &RowEvent<'_>) -> bool;

    /// 进度更新（默认空实现）。
    fn on_progress(&mut self, _p: &Progress) {}

    /// 故障通知（默认空实现）——已交付行仍有效，故障**不废**已出内容。
    fn on_fault(&mut self, _f: &StreamFault) {}
}

/// 空接收端（丢弃全部行——仅做结构校验与字节流量测量用）。
#[derive(Clone, Copy, Debug, Default)]
pub struct NullSink;

impl StreamSink for NullSink {
    fn on_row(&mut self, _ev: &RowEvent<'_>) -> bool {
        true
    }
}

/// 行元信息（队列槽位的描述面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RowMeta {
    /// 遍号。
    pub pass: u8,
    /// 遍内行号。
    pub pass_row: u16,
    /// 全图行号。
    pub y: u32,
    /// 首列。
    pub col_start: u16,
    /// 列步长。
    pub col_step: u8,
    /// 本行像素宽（= rgba 长度 / 4）。
    pub width: u16,
}

/// 队列溢出策略。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OverflowPolicy {
    /// 丢最旧（默认）——渐进显示关心最新帧，丢中间行不损画质终态。
    DropOldest,
    /// 拒绝新行（计数 `dropped`）——需要零丢包的消费端用。
    Reject,
}

/// 有界行队列（内核上的"解码线程异步派发"形态）。
///
/// **为什么不是线程**：内核无抢占式线程池，且本模块在 `no_std` 下不能依赖
/// 任何调度器。诚实的等价形态是「解码侧 O(1) 入队 + 消费侧按自身节奏 drain」——
/// 解码循环永不因消费端慢而阻塞，这正是锚点要的性质。
#[derive(Clone, Debug)]
pub struct RowQueue {
    /// 槽位数。
    pub cap: usize,
    /// 每槽字节容量（须 ≥ 最大行 RGBA 长度）。
    pub row_cap: usize,
    /// 槽位数据（`cap * row_cap`）。
    data: Vec<u8>,
    /// 槽位元信息。
    meta: Vec<RowMeta>,
    /// 队头下标。
    head: usize,
    /// 队列长度。
    len: usize,
    /// 被丢弃的行数（可审计面——不静默丢）。
    pub dropped: u64,
    /// 已入队总行数。
    pub pushed: u64,
    /// 已出队总行数。
    pub drained: u64,
    /// 溢出策略。
    pub policy: OverflowPolicy,
}

impl RowQueue {
    /// 新建队列（`cap` 槽 × 每槽 `row_cap` 字节）。
    pub fn new(cap: usize, row_cap: usize) -> RowQueue {
        RowQueue {
            cap: cap.max(1),
            row_cap: row_cap.max(4),
            data: vec![0u8; cap.max(1) * row_cap.max(4)],
            meta: vec![
                RowMeta { pass: 0, pass_row: 0, y: 0, col_start: 0, col_step: 1, width: 0 };
                cap.max(1)
            ],
            head: 0,
            len: 0,
            dropped: 0,
            pushed: 0,
            drained: 0,
            policy: OverflowPolicy::DropOldest,
        }
    }

    /// 设溢出策略。
    pub fn with_policy(mut self, p: OverflowPolicy) -> RowQueue {
        self.policy = p;
        self
    }

    /// 队列长度。
    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }
    /// 队列是否空。
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 入队一行（O(1)，无分配）。返回是否入队成功。
    pub fn push(&mut self, m: RowMeta, rgba: &[u8]) -> bool {
        if rgba.len() > self.row_cap {
            // 槽位放不下：这是**配置错误**而非溢出策略问题，显性拒绝并计数
            self.dropped += 1;
            return false;
        }
        if self.len == self.cap {
            match self.policy {
                OverflowPolicy::Reject => {
                    self.dropped += 1;
                    return false;
                }
                OverflowPolicy::DropOldest => {
                    // 队满：头指针前移一格腾出**队尾**槽位（O(1)，不搬数据）
                    self.head = (self.head + 1) % self.cap;
                    self.len -= 1;
                    self.dropped += 1;
                }
            }
        }
        let at = (self.head + self.len) % self.cap;
        let off = at * self.row_cap;
        self.data[off..off + rgba.len()].copy_from_slice(rgba);
        self.meta[at] = m;
        self.len += 1;
        self.pushed += 1;
        true
    }

    /// 查看队头（不弹出）。
    pub fn peek(&self) -> Option<(RowMeta, &[u8])> {
        if self.len == 0 {
            return None;
        }
        let at = self.head;
        let m = self.meta[at];
        let w = (m.width as usize).saturating_mul(4);
        let off = at * self.row_cap;
        Some((m, &self.data[off..off + w.min(self.row_cap)]))
    }

    /// 弹出队头（返回元信息与行切片）。
    ///
    /// 切片**借用队列自身**——调用方须在下次 `push`/`drain` 前用完。
    pub fn pop(&mut self) -> Option<(RowMeta, &[u8])> {
        if self.len == 0 {
            return None;
        }
        let at = self.head;
        self.head = (self.head + 1) % self.cap;
        self.len -= 1;
        self.drained += 1;
        let m = self.meta[at];
        let w = (m.width as usize).saturating_mul(4).min(self.row_cap);
        let off = at * self.row_cap;
        Some((m, &self.data[off..off + w]))
    }

    /// 清空（不重置统计——统计是可审计面）。
    pub fn clear(&mut self) {
        self.head = 0;
        self.len = 0;
    }
}

impl StreamSink for RowQueue {
    fn on_row(&mut self, ev: &RowEvent<'_>) -> bool {
        let m = RowMeta {
            pass: ev.pass as u8,
            pass_row: ev.pass_row as u16,
            y: ev.y,
            col_start: ev.col_start as u16,
            col_step: ev.col_step as u8,
            width: (ev.rgba.len() / 4) as u16,
        };
        self.push(m, ev.rgba)
    }

    fn on_progress(&mut self, _p: &Progress) {}
}

// ===========================================================================
// 七、流式会话（状态机 + chunk 缓冲 + 进度记录 —— 锚点「数据结构」条）
// ===========================================================================

/// 会话统计（可审计面；不静默吞任何计数）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct SessionStats {
    /// `feed` 调用次数。
    pub feeds: u32,
    /// 已交付行数。
    pub rows: u32,
    /// 已成帧块数。
    pub chunks: u32,
    /// IDAT 块数。
    pub idat_chunks: u32,
    /// 跳过的 ancillary 块数。
    pub ancillary_skipped: u32,
    /// ancillary CRC 告警次数。
    pub crc_warned: u32,
    /// 累计喂入字节数。
    pub bytes_in: u64,
    /// 分帧器考察字节数（性能判据分子）。
    pub scanned: u64,
    /// 有效载荷字节数（性能判据分母：IDAT+IHDR+PLTE+tRNS 载荷）。
    pub payload: u64,
    /// 中断次数（`checkpoint` 被调用的次数）。
    pub interrupts: u32,
    /// 恢复次数（`resume` 被调用的次数）。
    pub resumes: u32,
}

/// 本次 `feed` 的结果（相位 + 产出 + 故障三面一体）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FeedReport {
    /// 调用后的相位。
    pub phase: SessionPhase,
    /// 本次新交付行数。
    pub rows: u32,
    /// 累计交付行数。
    pub total_rows: u32,
    /// 本次消耗字节数。
    pub consumed: u32,
    /// 故障（`None` = 本次无故障）。
    ///
    /// **故障不废已出行**：见 [`StreamFault::rows_done`] 与
    /// [`StreamSession::partial_rgba`]。
    pub fault: Option<StreamFault>,
    /// 会话是否已到 IEND。
    pub complete: bool,
}

impl FeedReport {
    /// 本次是否产出故障。
    #[inline]
    pub fn failed(&self) -> bool {
        self.fault.is_some()
    }
}

/// PNG 流式解码会话。
///
/// **用法**：`new()` → 反复 `feed(chunk, &mut sink)` → `finish()`。
/// 任意时刻可 `checkpoint()` 取快照、`restore(snap)` 恢复。
///
/// **内存诚实口径**：IDAT 载荷合并缓冲 + 原始展开区随图像规模增长
/// （与 F1001 的 `scratch_need` 同阶）。行级输出**不整图驻留**——
/// 但 `partial_rgba()` 的渐进帧缓冲按需分配（`want_frame = true` 时），
/// 不开就不占。这是 F1013 内存治理的直接抓手。
pub struct StreamSession {
    /// 当前相位。
    phase: SessionPhase,
    /// 签名累计字节。
    sig: Vec<u8>,
    /// 块帧拼接器。
    stitch: ChunkStitcher,
    /// IHDR（解析成功后有效）。
    head: Option<dec::Ihdr>,
    /// 调色板。
    palette: Option<dec::Palette>,
    /// 透明语义。
    trns: dec::Transparency,
    /// IDAT 合并缓冲（zlib 流连续）。
    idat: Vec<u8>,
    /// 可恢复 inflate。
    z: ResumableZlib,
    /// 原始展开区（回参考窗口历史即在此缓冲内）。
    raw: Vec<u8>,
    /// 原始展开区已消费游标（已转为行的字节数）。
    raw_taken: usize,
    /// 反滤波当前行缓冲（载荷，= row_bytes-1）。
    cur: Vec<u8>,
    /// 反滤波上一行缓冲（载荷）。
    prev: Vec<u8>,
    /// RGBA 行输出缓冲（最大行宽×4）。
    rgba: Vec<u8>,
    /// 行派发中转缓冲（避开 `&self.raw[..]` 与 `&mut self` 的别名）。
    line_buf: Vec<u8>,
    /// 已交付行数。
    rows: u32,
    /// Adam7 当前遍号（非隔行恒 0）。
    pass: usize,
    /// 当前遍内已交付行数。
    pass_row: u32,
    /// 渐进帧缓冲（RGBA，`want_frame` 时分配）。
    frame: Vec<u8>,
    /// 覆盖计数（每像素被写过几遍；F1019 缩略图路径共用此面）。
    cov: Vec<u8>,
    /// 是否保留渐进帧。
    want_frame: bool,
    /// 已见 IEND。
    saw_iend: bool,
    /// 统计。
    pub stats: SessionStats,
    /// 首次故障（会话进入 Faulted 后保持）。
    fault: Option<StreamFault>,
}

impl StreamSession {
    /// 新建会话。
    ///
    /// `want_frame = true` 时分配渐进帧缓冲（宽×高×4）与覆盖计数（宽×高），
    /// 供「部分图像输出」与 F1019 缩略图路径消费。
    pub fn new(want_frame: bool) -> StreamSession {
        StreamSession {
            phase: SessionPhase::Signature,
            sig: Vec::new(),
            stitch: ChunkStitcher::new(),
            head: None,
            palette: None,
            trns: dec::Transparency::None,
            idat: Vec::new(),
            z: ResumableZlib::new(),
            raw: Vec::new(),
            raw_taken: 0,
            cur: Vec::new(),
            prev: Vec::new(),
            rgba: Vec::new(),
            line_buf: Vec::new(),
            rows: 0,
            pass: 0,
            pass_row: 0,
            frame: Vec::new(),
            cov: Vec::new(),
            want_frame,
            saw_iend: false,
            stats: SessionStats::default(),
            fault: None,
        }
    }

    /// 当前相位。
    #[inline]
    pub fn phase(&self) -> SessionPhase {
        self.phase
    }

    /// IHDR（未解析成功时为 `None`）。
    #[inline]
    pub fn head(&self) -> Option<dec::Ihdr> {
        self.head
    }

    /// 已交付行数。
    #[inline]
    pub fn rows(&self) -> u32 {
        self.rows
    }

    /// inflate 解码器（只读观测面）。
    ///
    /// **为何暴露 `pub`**：域自检需要**直接断言 inflate 内部状态**
    /// （位缓冲/字节偏移/待办动作），而不是经聚合层转一道——
    /// 经聚合层就分不清「状态真被序列化并用上了」与「恰好从头重解也对了」。
    /// 字段本身全是 `pub`，故这是只读观测而非可变入口。
    #[allow(dead_code)]
    pub fn inflate(&self) -> &ResumableZlib {
        &self.z
    }

    /// 已交付行数（只读观测面）。
    #[allow(dead_code)]
    pub fn rows_delivered(&self) -> u32 {
        self.rows
    }

    /// 原始展开区已消费游标（只读观测面）。
    #[allow(dead_code)]
    pub fn raw_cursor(&self) -> usize {
        self.raw_taken
    }





    /// 首次故障。
    #[inline]
    pub fn fault(&self) -> Option<StreamFault> {
        self.fault
    }

    /// 覆盖计数（每像素被写过的遍数；F1019 缩略图路径共用）。
    #[inline]
    pub fn coverage(&self) -> &[u8] {
        &self.cov
    }

    /// 渐进帧缓冲（RGBA 整图尺寸；未分配时为空）。
    #[inline]
    pub fn partial_rgba(&self) -> &[u8] {
        &self.frame
    }

    /// 已覆盖像素数（覆盖计数 ≥1 的像素个数）——部分输出的自证面。
    pub fn covered_pixels(&self) -> u64 {
        self.cov.iter().filter(|&&c| c > 0).count() as u64
    }

    /// 流式开销（字节流量口径，超出部分相对有效载荷的百分比）。
    ///
    /// 分子 = 分帧器考察字节数（含每块 12 字节帧开销与残留）；
    /// 分母 = 有效载荷字节数。**不是 wall-clock**（内核自检无可靠时钟，
    /// 真实耗时基准属 F1016 专项）。
    pub fn overhead_pct(&self) -> f64 {
        let payload = self.stats.payload;
        if payload == 0 {
            return 0.0;
        }
        let scanned = self.stats.scanned.max(self.stats.bytes_in);
        (scanned as f64 - payload as f64) * 100.0 / payload as f64
    }

    /// 喂入一段字节。
    ///
    /// **chunk 大小无关性的入口保证**：本函数不假设 `chunk` 与任何结构边界对齐——
    /// 签名、块帧、inflate 位流三层各自按"未凑齐就攒着"处理。
    pub fn feed(&mut self, chunk: &[u8], sink: &mut dyn StreamSink) -> FeedReport {
        self.stats.feeds += 1;
        self.stats.bytes_in += chunk.len() as u64;
        let before = self.rows;
        if let Some(f) = self.fault {
            // 已故障：不再推进，但如实报告"本次 0 产出 + 原故障"
            return FeedReport {
                phase: self.phase,
                rows: 0,
                total_rows: self.rows,
                consumed: 0,
                fault: Some(f),
                complete: false,
            };
        }
        let mut consumed = 0u32;
        // 第一层：签名
        if self.phase == SessionPhase::Signature {
            let need = 8 - self.sig.len();
            let take = need.min(chunk.len());
            self.sig.extend_from_slice(&chunk[..take]);
            consumed += take as u32;
            if self.sig.len() < 8 {
                return self.report(0, consumed, false, None);
            }
            if self.sig[..8] != PNG_SIG {
                let f = StreamFault::new(StreamFaultKind::NotSignature).with(8, self.sig.len() as u64).at_rows(0);
                self.fail(f, sink);
                return self.report(0, consumed, false, self.fault);
            }
            self.phase = SessionPhase::Header;
        }
        // 第二层：块帧
        let rest = &chunk[consumed as usize..];
        self.stitch.push(rest);
        consumed += rest.len() as u32;
        self.stats.scanned = self.stitch.scanned;
        // 第三层：逐块处理（一个 feed 内可有多块 —— 锚点「chunk 内多块」）
        let mut fault = None;
        while self.stitch.avail() > 0 {
            if self.saw_iend {
                // IEND 之后忽略（规范允许尾部数据）
                let _ = self.stitch.consume();
                continue;
            }
            match self.handle_one(sink) {
                Ok(()) => {}
                Err(f) => {
                    fault = Some(f);
                    break;
                }
            }
        }
        self.stats.chunks = self.stitch.chunks;
        if let Some(f) = fault {
            self.fail(f, sink);
        }
        let rows = self.rows.saturating_sub(before);
        // 第四层：进度回调
        let p = self.progress();
        sink.on_progress(&p);
        self.report(rows, consumed, self.saw_iend, self.fault)
    }

    /// 组装一份结果报告。
    fn report(&self, rows: u32, consumed: u32, complete: bool, fault: Option<StreamFault>) -> FeedReport {
        FeedReport { phase: self.phase, rows, total_rows: self.rows, consumed, fault, complete }
    }

    /// 当前进度。
    pub fn progress(&self) -> Progress {
        let head = self.head;
        let (total_rows, pct) = match head {
            Some(h) => {
                let total = h.height;
                let done = if h.interlace == 0 {
                    self.rows.min(total)
                } else {
                    // 隔行：按已覆盖像素占比折算（覆盖图是唯一诚实的进度口径）
                    let want = h.width as u64 * h.height as u64;
                    if want == 0 {
                        0
                    } else {
                        ((self.covered_pixels() * 100) / want) as u32
                    }
                };
                let pct = if total == 0 { 0 } else { ((done.min(100) as u64 * 100) / total as u64) as u8 };
                (total, pct.min(100))
            }
            None => (0, 0),
        };
        Progress {
            bytes_in: self.stats.bytes_in,
            chunks: self.stats.chunks,
            idat_bytes: self.idat.len() as u64,
            raw_bytes: self.raw.len() as u64,
            rows: self.rows,
            total_rows,
            phase: self.phase,
            pct,
        }
    }

    /// 记录故障并进入 Faulted 相位（**已产出行不失效**）。
    fn fail(&mut self, f: StreamFault, sink: &mut dyn StreamSink) {
        let f = f.at_rows(self.rows);
        if self.fault.is_none() {
            self.fault = Some(f);
        }
        self.phase = SessionPhase::Faulted;
        sink.on_fault(&self.fault.expect("刚置位必有值"));
    }

    /// 处理一个已成帧的块（读载荷 → 校验 → 语义 → 消费帧）。
    fn handle_one(&mut self, sink: &mut dyn StreamSink) -> Result<(), StreamFault> {
        let fourcc = self.stitch.head_type();
        // **顺序要紧**：CRC 位于载荷之后的帧尾 4 字节。`consume` 会把帧从
        // 缓冲移走，故 CRC 与载荷都必须在它之前取。
        let crc = self.stitch.peek_crc();
        let payload = self.stitch.peek_payload().to_vec();
        self.stats.payload += payload.len() as u64;
        let got = mech_inflate::crc32_span(&fourcc, &payload);
        let consume_res = self.stitch.consume();
        consume_res?;
        if got != crc {
            match dec::classify_chunk(&fourcc) {
                dec::ChunkClass::Critical => {
                    return Err(StreamFault::new(StreamFaultKind::CriticalCrc)
                        .at_chunk(&fourcc)
                        .with(got as u64, crc as u64));
                }
                dec::ChunkClass::Ancillary => {
                    self.stats.crc_warned += 1;
                    self.stats.ancillary_skipped += 1;
                }
            }
        }
        match &fourcc {
            b"IHDR" => {
                if self.head.is_some() || self.phase != SessionPhase::Header {
                    return Err(StreamFault::new(StreamFaultKind::ChunkOrder).at_chunk(&fourcc));
                }
                // 隔行由 F1003 的七遍几何承接，故此处放行 interlace=1
                let h = dec::parse_ihdr_ex(&payload, true).map_err(map_dec_fault)?;
                dec::check_pixel_budget(&h).map_err(map_dec_fault)?;
                self.alloc_for(&h)?;
                self.head = Some(h);
                self.phase = SessionPhase::Chunks;
            }
            b"PLTE" => {
                let h = self.head.ok_or_else(|| {
                    StreamFault::new(StreamFaultKind::ChunkOrder).at_chunk(&fourcc)
                })?;
                self.palette = Some(dec::parse_plte(&payload, h.color).map_err(map_dec_fault)?);
            }
            b"tRNS" => {
                let h = self.head.ok_or_else(|| {
                    StreamFault::new(StreamFaultKind::ChunkOrder).at_chunk(&fourcc)
                })?;
                let plen = self.palette.as_ref().map(|p| p.len).unwrap_or(0);
                self.trns = dec::parse_trns(&payload, h.color, plen).map_err(map_dec_fault)?;
            }
            b"IDAT" => {
                if self.head.is_none() {
                    return Err(StreamFault::new(StreamFaultKind::ChunkOrder).at_chunk(&fourcc));
                }
                if self.phase == SessionPhase::Chunks {
                    self.phase = SessionPhase::ImageData;
                } else if self.phase != SessionPhase::ImageData {
                    // IDAT 必须连续；被 ancillary 块打断即非法（规范 §3.3）
                    return Err(StreamFault::new(StreamFaultKind::ChunkOrder).at_chunk(&fourcc));
                }
                self.stats.idat_chunks += 1;
                if self.idat.len().saturating_add(payload.len()) > IDAT_BYTES_MAX {
                    return Err(StreamFault::new(StreamFaultKind::ChunkTooLarge)
                        .at_chunk(&fourcc)
                        .with(IDAT_BYTES_MAX as u64, self.idat.len() as u64));
                }
                self.idat.extend_from_slice(&payload);
                self.pump_inflate(sink)?;
            }
            b"IEND" => {
                if self.head.is_none() {
                    return Err(StreamFault::new(StreamFaultKind::ChunkOrder).at_chunk(&fourcc));
                }
                self.saw_iend = true;
                self.phase = SessionPhase::End;
            }
            _ => {
                // 未知块按 ancillary/critical 分级处置（与 F1001 同口径）
                match dec::classify_chunk(&fourcc) {
                    dec::ChunkClass::Critical => {
                        return Err(StreamFault::new(StreamFaultKind::ChunkOrder).at_chunk(&fourcc));
                    }
                    dec::ChunkClass::Ancillary => {
                        self.stats.ancillary_skipped += 1;
                    }
                }
            }
        }
        Ok(())
    }

    /// 按 IHDR 分配各工作缓冲。
    fn alloc_for(&mut self, h: &dec::Ihdr) -> Result<(), StreamFault> {
        let (rb, rgba_len) = if h.interlace == 0 {
            (h.row_bytes(), h.width as usize * 4)
        } else {
            // 隔行：取七遍中最大的行宽（遍宽 ≤ 全宽，故全宽是上界）
            let mut max_rb = 0usize;
            let mut max_w = 0usize;
            for i in 0..ad7::PASS_COUNT {
                if let Some(e) = ad7::pass_extent(i, h.width, h.height, h) {
                    if e.row_bytes > max_rb {
                        max_rb = e.row_bytes;
                    }
                    if e.width as usize > max_w {
                        max_w = e.width as usize;
                    }
                }
            }
            (max_rb.max(1), max_w * 4)
        };
        // 原始展开区上界：非隔行 = h × row_bytes；隔行 = 七遍之和
        let raw_max = if h.interlace == 0 {
            h.raw_bytes()
        } else {
            ad7::total_raw_bytes(h.width, h.height, h)
        };
        if raw_max == 0 || raw_max > dec::PIXEL_BUDGET_BYTES {
            return Err(StreamFault::new(StreamFaultKind::OutputFull)
                .at_chunk(&dec::CHUNK_IHDR)
                .with(raw_max, dec::PIXEL_BUDGET_BYTES));
        }
        // Vec 容量按需增长，故只保留长度语义：这里只记录上界供越界判定
        self.cur = vec![0u8; rb.saturating_sub(1)];
        self.prev = vec![0u8; rb.saturating_sub(1)];
        self.rgba = vec![0u8; rgba_len];
        self.raw = Vec::with_capacity(raw_max as usize);
        if self.want_frame {
            let fb = h.rgba_bytes() as usize;
            if h.rgba_bytes() > dec::PIXEL_BUDGET_BYTES {
                return Err(StreamFault::new(StreamFaultKind::OutputFull)
                    .at_chunk(&dec::CHUNK_IHDR)
                    .with(fb as u64, dec::PIXEL_BUDGET_BYTES));
            }
            self.frame = vec![0u8; fb];
            self.cov = vec![0u8; h.width as usize * h.height as usize];
        }
        Ok(())
    }

    /// 推进 inflate 并把完整行转成行事件。
    fn pump_inflate(&mut self, sink: &mut dyn StreamSink) -> Result<(), StreamFault> {
        // 每次 pump 的产出上限 = 剩余原始字节上界（避免一次 pump 无界推进）
        let head = match self.head {
            Some(h) => h,
            None => return Ok(()),
        };
        let budget = self.pump_budget(&head);
        let mut stat = self.z.pump(&self.idat, &mut self.raw, budget).map_err(|e| e.at_rows(self.rows))?;
        while stat.produced > 0 && !self.z.is_done() {
            // IDAT 缓冲可能已全部消费，再 pump 一次只会得到 0
            if self.z.st.src as usize >= self.idat.len() {
                break;
            }
            stat = self.z.pump(&self.idat, &mut self.raw, budget).map_err(|e| e.at_rows(self.rows))?;
            if stat.produced == 0 {
                break;
            }
        }
        self.drain_rows(sink)?;
        Ok(())
    }

    /// 单次 pump 的产出上限。
    fn pump_budget(&self, h: &dec::Ihdr) -> usize {
        let total = if h.interlace == 0 {
            h.raw_bytes()
        } else {
            ad7::total_raw_bytes(h.width, h.height, h)
        };
        // 上限至少 1 行，避免 budget=0 时无法推进（行内数据已部分到达的情形）
        let one_row = if h.interlace == 0 {
            h.row_bytes()
        } else {
            let mut m = 1usize;
            for i in 0..ad7::PASS_COUNT {
                if let Some(e) = ad7::pass_extent(i, h.width, h.height, h) {
                    if e.row_bytes > m {
                        m = e.row_bytes;
                    }
                }
            }
            m
        };
        let cap = total.saturating_sub(self.raw.len() as u64) as usize;
        cap.max(one_row)
    }

    /// 把原始展开区里已完整的行转成行事件并派发。
    fn drain_rows(&mut self, sink: &mut dyn StreamSink) -> Result<(), StreamFault> {
        let head = match self.head {
            Some(h) => h,
            None => return Ok(()),
        };
        // **行缓冲走 `line_buf` 中转，不从 `self.raw` 直接借**：
        // `emit_row*` 取 `&mut self`，而 `&self.raw[..]` 是不可变借用，
        // 二者在同一作用域并存即别名。先拷进 `line_buf` 再派发。
        loop {
            // **消费端中止后必须真停**：派发循环体每轮都派发一行，
            // 若不在循环头检查 `Faulted`，中止信号会被同一轮的下一轮
            // 循环忽略掉——表现为「消费端说停，事件却还在来」，
            // 已交付行数与消费端的预期直接对不上。
            if self.phase == SessionPhase::Faulted || self.fault.is_some() {
                return Ok(());
            }
            if head.interlace == 0 {
                let rb = head.row_bytes();
                if self.raw.len() - self.raw_taken < rb {
                    return Ok(());
                }
                self.line_buf.clear();
                self.line_buf.extend_from_slice(&self.raw[self.raw_taken..self.raw_taken + rb]);
                self.raw_taken += rb;
                let line = core::mem::take(&mut self.line_buf);
                self.emit_row(&line, sink);
                self.line_buf = line;
            } else {
                // 隔行：逐遍推进，遍内行独立滤波（prev 在遍首清零）
                loop {
                    // 与非隔行分支同一条纪律：中止后立即停派。
                    if self.phase == SessionPhase::Faulted || self.fault.is_some() {
                        return Ok(());
                    }
                    if self.pass >= ad7::PASS_COUNT {
                        return Ok(());
                    }
                    let ext = match ad7::pass_extent(self.pass, head.width, head.height, &head) {
                        Some(e) => e,
                        None => return Ok(()),
                    };
                    if ext.is_empty() {
                        self.pass += 1;
                        self.pass_row = 0;
                        self.prev.clear();
                        self.prev.resize(ext.payload_bytes(), 0);
                        continue;
                    }
                    let rb = ext.row_bytes;
                    if self.raw.len() - self.raw_taken < rb {
                        return Ok(());
                    }
                    // **遍首必须把 `cur`/`prev` 收到本遍行宽**：
                    // 隔行七遍的行宽逐遍不同（9x9 时从 9 到 37 不等），
                    // 而 `alloc_for` 只按非隔行全宽分配一次。`prev` 在遍首
                    // 被清零后若不按本遍 resize，`emit_row_mapped` 的
                    // `prev.len() < payload` 守卫会**静默 return**——
                    // 表现为「只解出第 0 遍，后面六遍凭空消失」，
                    // 而 `complete` 仍为真，极难定位。
                    if self.pass_row == 0 {
                        let need = rb1(rb);
                        self.cur.clear();
                        self.cur.resize(need, 0);
                        self.prev.clear();
                        self.prev.resize(need, 0);
                    }
                    self.line_buf.clear();
                    self.line_buf.extend_from_slice(&self.raw[self.raw_taken..self.raw_taken + rb]);
                    self.raw_taken += rb;
                    let geom = ad7::pass_geom(self.pass).unwrap_or(ad7::ADAM7_PASSES[0]);
                    let y = geom.start_row + self.pass_row * geom.row_step;
                    let (p, pr, cs, cstep) = (self.pass, self.pass_row, geom.start_col, geom.col_step);
                    let line = core::mem::take(&mut self.line_buf);
                    self.emit_row_mapped(&line, p, pr, y, cs, cstep, sink);
                    self.line_buf = line;
                    self.pass_row += 1;
                    if self.pass_row >= ext.height {
                        self.pass += 1;
                        self.pass_row = 0;
                        self.prev.clear();
                    }
                }
            }
        }
    }

    /// 非隔行行派发（`col_step = 1`，`y` 即行号）。
    fn emit_row(&mut self, line: &[u8], sink: &mut dyn StreamSink) {
        let head = self.head.expect("仅在 head 就绪后调用");
        let bpp = head.filter_bpp();
        let y = self.rows;
        let payload = rb1(line.len());
        if self.cur.len() < payload || self.prev.len() < payload {
            return;
        }
        {
            let isa = imgsimd::detect_isa();
            let mut ctx = dec::DecodeCtx::new(&mut self.cur, &self.prev[..payload], isa);
            if ctx.unfilter_line(line, bpp).is_none() {
                return;
            }
        }
        let pal = self.palette.clone();
        if dec::expand_row_rgba(&self.cur[..payload], &head, pal.as_ref(), &self.trns, &mut self.rgba).is_none() {
            return;
        }
        let w = head.width as usize;
        let rgba = &self.rgba[..w * 4];
        if self.want_frame {
            let off = y as usize * w * 4;
            self.frame[off..off + w * 4].copy_from_slice(rgba);
            for i in 0..w {
                let at = y as usize * w + i;
                self.cov[at] = self.cov[at].saturating_add(1);
            }
        }
        // 先转 prev 再派发：回调若重入本会话，看到的 prev 已是本行
        self.prev[..payload].copy_from_slice(&self.cur[..payload]);
        self.rows += 1;
        let ev = RowEvent { pass: 0, pass_row: y, y, col_start: 0, col_step: 1, rgba };
        if !sink.on_row(&ev) {
            // 中止：停在当前行，已出行有效
            self.phase = SessionPhase::Faulted;
        }
    }


    /// 隔行行派发（带全图坐标映射）。
    #[allow(clippy::too_many_arguments)]
    fn emit_row_mapped(
        &mut self,
        line: &[u8],
        pass: usize,
        pass_row: u32,
        y: u32,
        col_start: u32,
        col_step: u32,
        sink: &mut dyn StreamSink,
    ) {
        let head = self.head.expect("仅在 head 就绪后调用");
        let bpp = head.filter_bpp();
        let payload = rb1(line.len());
        if self.cur.len() < payload || self.prev.len() < payload {
            return;
        }
        {
            let isa = imgsimd::detect_isa();
            let mut ctx = dec::DecodeCtx::new(&mut self.cur, &self.prev[..payload], isa);
            if ctx.unfilter_line(line, bpp).is_none() {
                return;
            }
        }
        let pal = self.palette.clone();
        if dec::expand_row_rgba(&self.cur[..payload], &head, pal.as_ref(), &self.trns, &mut self.rgba).is_none() {
            return;
        }
        let pw = (self.rgba.len() / 4).max(1);
        let rgba = &self.rgba[..pw * 4];
        if self.want_frame {
            let w = head.width as usize;
            for k in 0..pw {
                let gx = col_start as usize + k * col_step as usize;
                if gx >= w {
                    break;
                }
                let so = k * 4;
                let doff = (y as usize * w + gx) * 4;
                self.frame[doff..doff + 4].copy_from_slice(&rgba[so..so + 4]);
                let at = y as usize * w + gx;
                self.cov[at] = self.cov[at].saturating_add(1);
            }
        }
        self.prev[..payload].copy_from_slice(&self.cur[..payload]);
        self.rows += 1;
        let ev = RowEvent { pass, pass_row, y, col_start, col_step, rgba };
        if !sink.on_row(&ev) {
            self.phase = SessionPhase::Faulted;
        }
    }

    /// 流结束（`IEND` 之后调用）：把残留的完整行也派发干净。
    pub fn finish(&mut self, sink: &mut dyn StreamSink) -> FeedReport {
        if self.fault.is_some() {
            return self.report(0, 0, self.saw_iend, self.fault);
        }
        let before = self.rows;
        let _ = self.drain_rows(sink);
        let p = self.progress();
        sink.on_progress(&p);
        self.report(self.rows.saturating_sub(before), 0, self.saw_iend, self.fault)
    }
}

/// 行载荷长度（去掉滤波先行字节；下溢取 0）。
#[inline]
fn rb1(row_bytes: usize) -> usize {
    row_bytes.saturating_sub(1)
}

/// F1001 故障到流式故障的映射（保留原 kind 于 `chunk` 字段之外——
/// 流式侧用独立的 `StreamFaultKind` 值域，避免跨域码段相撞）。
fn map_dec_fault(e: dec::PngFault) -> StreamFault {
    use dec::FaultKind as F;
    let kind = match e.kind {
        F::BadSignature => StreamFaultKind::NotSignature,
        F::TruncatedChunk => StreamFaultKind::ChunkOrder,
        F::CrcCritical => StreamFaultKind::CriticalCrc,
        F::UnknownCritical => StreamFaultKind::ChunkOrder,
        F::MissingIhdr | F::IhdrField | F::IllegalCombo => StreamFaultKind::TableCorrupt,
        F::InterlaceUnsupported => StreamFaultKind::TableCorrupt,
        F::Palette | F::PaletteRequired => StreamFaultKind::TableCorrupt,
        F::TrnsLen | F::NoIdat | F::Dimension => StreamFaultKind::ChunkOrder,
        F::Zlib => StreamFaultKind::ZlibHeader,
        F::BadFilter => StreamFaultKind::TableCorrupt,
        F::BufferShort => StreamFaultKind::OutputFull,
        F::PixelBudget => StreamFaultKind::OutputFull,
        F::TooManyIdat => StreamFaultKind::ChunkTooLarge,
        F::CrcAncillary => StreamFaultKind::CriticalCrc,
    };
    StreamFault::new(kind).at_chunk(&e.chunk).with(e.detail_a, e.detail_b)
}

// ===========================================================================
// 八、中断 / 恢复（锚点：中断=保存块游标与 inflate 状态，恢复=从游标续解）
// ===========================================================================

/// 恢复点（快照字节流）。
///
/// **覆盖锚点要求的全部内容**：
/// - **块游标**：`stitch.staging` + `scan` + `need`（未成帧的残留也在内，
///   故恢复后可从半个块头继续）。
/// - **inflate 状态**：整个 [`ZState`]（位缓冲/字节偏移/块相位/码长表/
///   符号累加器/待办动作/Adler）逐字段序列化。
/// - **会话游标**：`raw_taken` / `rows` / `pass` / `pass_row` / 相位。
///
/// **回参考窗口不在快照里**：`raw` 原始展开区整体入快照，故恢复后
/// `out.len() >= dist` 恒成立——窗口历史由 `raw` 承载，无需另存 32KB。
impl StreamSession {
    /// 拍摄恢复点（块游标 + inflate 状态 + 会话游标）。
    pub fn checkpoint(&mut self) -> Vec<u8> {
        self.stats.interrupts += 1;
        let mut w = W::new();
        // 魔数是**定长**标记，不能走 `w.bytes()`（那会带 4 字节长度前缀），
        // 否则读端必须用 `r.bytes()`；写定长、读定长，两端必须对称。
        w.b.extend_from_slice(&SNAPSHOT_MAGIC);
        w.u8(SNAPSHOT_VERSION);
        // 相位
        w.u8(phase_num(self.phase));
        // 签名残留
        w.bytes(&self.sig);
        // 块游标
        w.bytes(&self.stitch.staging);
        w.u64(self.stitch.scan as u64);
        w.u64(self.stitch.need as u64);
        w.u32(self.stitch.chunks);
        w.u32(self.stitch.consumed);
        // 头语义
        match &self.head {
            Some(h) => {
                w.u8(1);
                w.u32(h.width);
                w.u32(h.height);
                w.u8(h.depth);
                // **必须写「线上码」而非 `color as u8`（枚举判别值）**：
                // 本枚举的判别值是Gray=0/Rgb=1/Palette=2/GrayAlpha=3/Rgba=4，
                // 而 PNG 线上的ColorType 是 0/2/3/4/6。写判别值会让恢复端
                // `from_u8` 把 Rgba(判别4) 读成 GrayAlpha(线码4)，
                // 于是 `row_bytes()` 算出 2 通道而非 4 通道，行切分全错
                // （实测：132 字节原始数据被派发出 5 行而不是 4 行）。
                w.u8(color_wire(h.color));
                w.u8(h.compression);
                w.u8(h.filter_method);
                w.u8(h.interlace);
            }
            None => {
                w.u8(0);
            }
        }
        // 调色板
        match &self.palette {
            Some(p) => {
                w.u8(1);
                w.u32(p.len as u32);
                w.bytes(&p.rgb);
            }
            None => {
                w.u8(0);
            }
        }
        // tRNS
        match &self.trns {
            dec::Transparency::None => w.u8(0),
            dec::Transparency::Gray(k) => {
                w.u8(1);
                w.u16(*k);
            }
            dec::Transparency::Rgb(r, g, b) => {
                w.u8(2);
                w.u16(*r);
                w.u16(*g);
                w.u16(*b);
            }
            dec::Transparency::PaletteAlpha(v) => {
                w.u8(3);
                w.bytes(v);
            }
        }
        // IDAT 合并缓冲 + 原始展开区（回参考窗口随 raw 一并保留）
        w.bytes(&self.idat);
        w.bytes(&self.raw);
        w.u64(self.raw_taken as u64);
        // inflate 状态（逐字段；与 inflate 级快照共用同一编码，见 `write_zstate`）
        write_zstate(&mut w, &self.z.st);
        // 会话游标
        w.u32(self.rows);
        w.u32(self.pass as u32);
        w.u32(self.pass_row);
        w.u8(self.saw_iend as u8);
        // 反滤波上一行（跨 feed 的滤波链必需——prev 是解码状态的一部分）
        w.bytes(&self.prev);
        w.bytes(&self.cov);
        // 交出快照字节流（`w.b` 是字段不是方法——写成 `w.b()` 会被解析为调用）
        w.b
    }

    /// 从恢复点重建会话。
    ///
    /// 版本不符 / 结构损坏 → [`StreamFaultKind::SnapshotVersion`] /
    /// [`StreamFaultKind::SnapshotCorrupt`] 显性拒绝，**不猜、不半初始化**。
    pub fn restore(snap: &[u8]) -> Result<StreamSession, StreamFault> {
        let mut r = R::new(snap);
        let bad = |k: StreamFaultKind| Err(StreamFault::new(k));
        let magic = match r.take(SNAPSHOT_MAGIC.len()) {
            Some(m) => m,
            None => return bad(StreamFaultKind::SnapshotCorrupt),
        };
        if magic != SNAPSHOT_MAGIC {
            return bad(StreamFaultKind::SnapshotCorrupt);
        }
        if r.u8() != Some(SNAPSHOT_VERSION) {
            return bad(StreamFaultKind::SnapshotVersion);
        }
        let phase = match r.u8() {
            Some(p) if p <= SessionPhase::Faulted as u8 => p,
            _ => return bad(StreamFaultKind::SnapshotCorrupt),
        };
        let sig = match r.bytes() {
            Some(v) => v,
            None => return bad(StreamFaultKind::SnapshotCorrupt),
        };
        let staging = match r.bytes() {
            Some(v) => v,
            None => return bad(StreamFaultKind::SnapshotCorrupt),
        };
        let scan = match r.u64() {
            Some(v) => v as usize,
            None => return bad(StreamFaultKind::SnapshotCorrupt),
        };
        let need = match r.u64() {
            Some(v) => v as usize,
            None => return bad(StreamFaultKind::SnapshotCorrupt),
        };
        let chunks = match r.u32() {
            Some(v) => v,
            None => return bad(StreamFaultKind::SnapshotCorrupt),
        };
        let consumed = match r.u32() {
            Some(v) => v,
            None => return bad(StreamFaultKind::SnapshotCorrupt),
        };
        // 头语义
        let head = match r.u8() {
            Some(0) => None,
            Some(1) => {
                let (width, height, depth, color, comp, filt, interlace) = match (
                    r.u32(),
                    r.u32(),
                    r.u8(),
                    r.u8(),
                    r.u8(),
                    r.u8(),
                    r.u8(),
                ) {
                    (Some(a), Some(b), Some(c), Some(d), Some(e), Some(f), Some(g)) => {
                        (a, b, c, d, e, f, g)
                    }
                    _ => return bad(StreamFaultKind::SnapshotCorrupt),
                };
                let ct = match dec::ColorType::from_u8(color) {
                    Some(v) => v,
                    None => return bad(StreamFaultKind::SnapshotCorrupt),
                };
                Some(dec::Ihdr {
                    width,
                    height,
                    depth,
                    color: ct,
                    compression: comp,
                    filter_method: filt,
                    interlace,
                })
            }
            _ => return bad(StreamFaultKind::SnapshotCorrupt),
        };
        // 调色板
        let palette = match r.u8() {
            Some(0) => None,
            Some(1) => {
                let len = match r.u32() {
                    Some(v) => v as usize,
                    None => return bad(StreamFaultKind::SnapshotCorrupt),
                };
                let rgb = match r.bytes() {
                    Some(v) => v,
                    None => return bad(StreamFaultKind::SnapshotCorrupt),
                };
                Some(dec::Palette { len, rgb })
            }
            _ => return bad(StreamFaultKind::SnapshotCorrupt),
        };
        // tRNS
        let trns = match r.u8() {
            Some(0) => dec::Transparency::None,
            Some(1) => match r.u16() {
                Some(k) => dec::Transparency::Gray(k),
                None => return bad(StreamFaultKind::SnapshotCorrupt),
            },
            Some(2) => match (r.u16(), r.u16(), r.u16()) {
                (Some(a), Some(b), Some(c)) => dec::Transparency::Rgb(a, b, c),
                _ => return bad(StreamFaultKind::SnapshotCorrupt),
            },
            Some(3) => match r.bytes() {
                Some(v) => dec::Transparency::PaletteAlpha(v),
                None => return bad(StreamFaultKind::SnapshotCorrupt),
            },
            _ => return bad(StreamFaultKind::SnapshotCorrupt),
        };
        let idat = match r.bytes() {
            Some(v) => v,
            None => return bad(StreamFaultKind::SnapshotCorrupt),
        };
        let raw = match r.bytes() {
            Some(v) => v,
            None => return bad(StreamFaultKind::SnapshotCorrupt),
        };
        let raw_taken = match r.u64() {
            Some(v) => v as usize,
            None => return bad(StreamFaultKind::SnapshotCorrupt),
        };
        // inflate 状态
        macro_rules! need_u8 {
            () => {
                match r.u8() {
                    Some(v) => v,
                    None => return bad(StreamFaultKind::SnapshotCorrupt),
                }
            };
        }
        macro_rules! need_u32 {
            () => {
                match r.u32() {
                    Some(v) => v,
                    None => return bad(StreamFaultKind::SnapshotCorrupt),
                }
            };
        }
        // inflate 状态（与会话级 checkpoint 同一编码，见 `read_zstate`）
        let st = match read_zstate(&mut r) {
            Some(v) => v,
            None => return bad(StreamFaultKind::SnapshotCorrupt),
        };
        // 会话游标
        let rows = need_u32!();
        let pass = need_u32!() as usize;
        let pass_row = need_u32!();
        let saw_iend = need_u8!() != 0;
        let prev = match r.bytes() {
            Some(v) => v,
            None => return bad(StreamFaultKind::SnapshotCorrupt),
        };
        let cov = match r.bytes() {
            Some(v) => v,
            None => return bad(StreamFaultKind::SnapshotCorrupt),
        };
        // 尾部必须恰好读完（多余字节 = 结构损坏）
        if !r.end_ok() {
            return bad(StreamFaultKind::SnapshotCorrupt);
        }
        // 布局一致性对账：快照自洽性硬闸（防"手拼快照喂出越界 panic"）
        let mut s = StreamSession::new(!cov.is_empty());
        s.phase = phase_of(phase);
        s.sig = sig;
        s.stitch.staging = staging;
        s.stitch.scan = scan;
        s.stitch.need = need;
        s.stitch.chunks = chunks;
        s.stitch.consumed = consumed;
        if s.stitch.scan > s.stitch.staging.len() {
            return bad(StreamFaultKind::SnapshotCorrupt);
        }
        s.head = head;
        s.palette = palette;
        s.trns = trns;
        s.idat = idat;
        s.raw = raw;
        s.raw_taken = raw_taken;
        if s.raw_taken > s.raw.len() {
            return bad(StreamFaultKind::SnapshotCorrupt);
        }
        if let Some(h) = s.head {
            // 工作缓冲按 IHDR 重建（尺寸来自快照，布局必须一致）
            let rb = if h.interlace == 0 {
                h.row_bytes()
            } else {
                let mut m = 1usize;
                for i in 0..ad7::PASS_COUNT {
                    if let Some(e) = ad7::pass_extent(i, h.width, h.height, &h) {
                        if e.row_bytes > m {
                            m = e.row_bytes;
                        }
                    }
                }
                m
            };
            let mut max_w = 0usize;
            if h.interlace == 0 {
                max_w = h.width as usize;
            } else {
                for i in 0..ad7::PASS_COUNT {
                    if let Some(e) = ad7::pass_extent(i, h.width, h.height, &h) {
                        if e.width as usize > max_w {
                            max_w = e.width as usize;
                        }
                    }
                }
            }
            s.cur = vec![0u8; rb.saturating_sub(1)];
            s.prev = prev;
            s.rgba = vec![0u8; max_w * 4];
            if s.want_frame {
                s.frame = vec![0u8; h.rgba_bytes() as usize];
                s.cov = cov;
            }
        }
        s.rows = rows;
        s.pass = pass;
        s.pass_row = pass_row;
        s.saw_iend = saw_iend;
        s.z = ResumableZlib::from_state(st);
        s.stats.resumes += 1;
        Ok(s)
    }
}

/// inflate 核心状态的**唯一**序列化编码（会话级快照与 inflate 级快照共用）。
///
/// **为什么抽成公共函数**：写端与读端一旦分头维护，同一字段漏一处就会
/// 表现为「快照能写不能读」，或更糟——**读得出但值错位**（定长/变长不对称时
/// 整条读序列平移若干字节，后面所有字段错位且不报错）。共用一个函数从
/// 结构上消灭这一类漂移。
fn write_zstate(w: &mut W, z: &ZState) {
    // 相位：**`ZPhase` 是无数据枚举，判别值即线码**（`zphase_of` 按 0..=8 还原）。
    // 改这里的编码必须同步改 `zphase_of`。
    w.u8(z.phase as u8);
    // 位流状态（位缓冲 / 有效位 / 字节游标）
    w.u32(z.bit_buf);
    w.u8(z.bit_cnt);
    w.u32(z.src);
    // 块相位
    w.u8(z.last as u8);
    w.u8(z.btype);
    w.u32(z.stored_left);
    // 校验与进度
    w.u32(z.adler);
    w.u32(z.out);
    // 动态码表码长数组（`w.bytes` 自带长度前缀，读端对账 `NLIT_CODES`/`NDIST_CODES`）
    w.bytes(&z.lit_len);
    w.bytes(&z.dist_len);
    // 码长表构建子状态
    w.u8(z.cl.phase as u8);
    w.u16(z.cl.nlit);
    w.u16(z.cl.ndist);
    w.u16(z.cl.hclen);
    w.u8(z.cl.cl_got);
    w.bytes(&z.cl.cl_sym);
    w.u16(z.cl.fill_at);
    w.u8(z.cl.prev_len);
    // 符号解码累加器
    w.u32(z.acc.code);
    w.u32(z.acc.first);
    w.u32(z.acc.index);
    w.u8(z.acc.len);
    // 待办动作（**显式线码**，不写 `pend as u8`——枚举判别值不是线上编码）
    w.u8(pend_tag(z.pend));
    match z.pend {
        Pend::None => {}
        Pend::ReadLenExtra { base, extra } => {
            w.u16(base);
            w.u8(extra);
        }
        Pend::ReadDistSym { len } => {
            w.u16(len);
        }
        Pend::ReadDistExtra { len, base, extra } => {
            w.u16(len);
            w.u16(base);
            w.u8(extra);
        }
        Pend::Copy { len, dist } => {
            w.u16(len);
            w.u16(dist);
        }
    }
    // 尾随累积 + zlib 头进度
    w.u32(z.tail_acc);
    w.u8(z.hdr_got);
}

/// [`write_zstate`] 的逆变换；任何缺字段 / 长度不符 → `None`（由调用方判损坏）。
fn read_zstate(r: &mut R) -> Option<ZState> {
    let mut z = ZState::new();
    let ph = r.u8()?;
    if ph > ZPhase::Done as u8 {
        return None;
    }
    z.phase = zphase_of(ph);
    z.bit_buf = r.u32()?;
    z.bit_cnt = r.u8()?;
    z.src = r.u32()?;
    z.last = r.u8()? != 0;
    z.btype = r.u8()?;
    z.stored_left = r.u32()?;
    z.adler = r.u32()?;
    z.out = r.u32()?;
    let lit_len = r.bytes()?;
    if lit_len.len() != NLIT_CODES {
        return None;
    }
    z.lit_len.copy_from_slice(&lit_len);
    let dist_len = r.bytes()?;
    if dist_len.len() != NDIST_CODES {
        return None;
    }
    z.dist_len.copy_from_slice(&dist_len);
    let clp = r.u8()?;
    if clp > ClPhase::Ready as u8 {
        return None;
    }
    z.cl.phase = clphase_of(clp);
    z.cl.nlit = r.u16()?;
    z.cl.ndist = r.u16()?;
    z.cl.hclen = r.u16()?;
    z.cl.cl_got = r.u8()?;
    let cl_sym = r.bytes()?;
    if cl_sym.len() != 19 {
        return None;
    }
    z.cl.cl_sym.copy_from_slice(&cl_sym);
    z.cl.fill_at = r.u16()?;
    z.cl.prev_len = r.u8()?;
    z.acc.code = r.u32()?;
    z.acc.first = r.u32()?;
    z.acc.index = r.u32()?;
    z.acc.len = r.u8()?;
    let pt = r.u8()?;
    z.pend = match pend_from_tag(pt) {
        Some(Pend::ReadLenExtra { .. }) => Pend::ReadLenExtra { base: r.u16()?, extra: r.u8()? },
        Some(Pend::ReadDistSym { .. }) => Pend::ReadDistSym { len: r.u16()? },
        Some(Pend::ReadDistExtra { .. }) => {
            Pend::ReadDistExtra { len: r.u16()?, base: r.u16()?, extra: r.u8()? }
        }
        Some(Pend::Copy { .. }) => Pend::Copy { len: r.u16()?, dist: r.u16()? },
        Some(Pend::None) => Pend::None,
        None => return None,
    };
    z.tail_acc = r.u32()?;
    z.hdr_got = r.u8()?;
    Some(z)
}

/// 相位序号还原。
fn phase_of(v: u8) -> SessionPhase {
    match v {
        0 => SessionPhase::Signature,
        1 => SessionPhase::Header,
        2 => SessionPhase::Chunks,
        3 => SessionPhase::ImageData,
        4 => SessionPhase::End,
        _ => SessionPhase::Faulted,
    }
}

/// 相位序号取反（写快照用）。
fn phase_num(p: SessionPhase) -> u8 {
    p as u8
}

/// 颜色类型的**线上码**（PNG ColorType：0/2/3/4/6）。
///
/// **不可写 `color as u8`**：本枚举判别值是 0/1/2/3/4，与线上码 0/2/3/4/6
/// 不同，两者只在 Gray(0↔0) 与 GrayAlpha(3↔4) 之外巧合相等。恢复端统一
/// 走 `dec::ColorType::from_u8`（按线码解码），故写端也必须给线码。
fn color_wire(c: dec::ColorType) -> u8 {
    match c {
        dec::ColorType::Gray => 0,
        dec::ColorType::Rgb => 2,
        dec::ColorType::Palette => 3,
        dec::ColorType::GrayAlpha => 4,
        dec::ColorType::Rgba => 6,
    }
}

/// [`Pend`] 的快照线码（**显式映射，不写 `pend as u8`**）。
///
/// `Pend` 是带字段枚举，`as` 转换对带字段枚举非法——即便能编过，
/// 枚举判别值也**不等于**线上编码值：增删变体会静默错位。显式映射 +
/// 恢复侧按码定位，两端必须同步改（[`pend_from_tag`]）。
fn pend_tag(p: Pend) -> u8 {
    match p {
        Pend::None => 0,
        Pend::ReadLenExtra { .. } => 1,
        Pend::ReadDistSym { .. } => 2,
        Pend::ReadDistExtra { .. } => 3,
        Pend::Copy { .. } => 4,
    }
}

/// [`pend_tag`] 的逆映射（未知码 → `None`，由调用方判损坏）。
fn pend_from_tag(t: u8) -> Option<Pend> {
    Some(match t {
        0 => Pend::None,
        1 => Pend::ReadLenExtra { base: 0, extra: 0 },
        2 => Pend::ReadDistSym { len: 0 },
        3 => Pend::ReadDistExtra { len: 0, base: 0, extra: 0 },
        4 => Pend::Copy { len: 0, dist: 0 },
        _ => return None,
    })
}

/// inflate 相位序号还原（快照反序列化用）。
fn zphase_of(v: u8) -> ZPhase {
    match v {
        0 => ZPhase::ZlibHeader,
        1 => ZPhase::BlockHeader,
        2 => ZPhase::StoredLen,
        3 => ZPhase::StoredBody,
        4 => ZPhase::BuildFixed,
        5 => ZPhase::BuildDynamic,
        6 => ZPhase::Symbols,
        7 => ZPhase::Tail,
        _ => ZPhase::Done,
    }
}

/// 码长子相位序号还原（快照反序列化用）。
fn clphase_of(v: u8) -> ClPhase {
    match v {
        0 => ClPhase::Head,
        1 => ClPhase::CodeLens,
        2 => ClPhase::Lengths,
        _ => ClPhase::Ready,
    }
}

// ===========================================================================
// 九、便利入口（全量对拍基线 —— 供 F1011 / F1019 复用）
// ===========================================================================

/// 一次性把整份字节流解完（便利形：内部按 4KB 切片喂，**不是**特例路径）。
///
/// 存在的意义：自检与对拍需要一个"非流式调用方"的等价入口，而它必须**走同一条**
/// 流式管线（否则对拍就是拿两条不同的实现互证，等于没对拍）。
pub fn decode_stream(file: &[u8], sink: &mut dyn StreamSink, slice: usize) -> FeedReport {
    let mut s = StreamSession::new(false);
    let step = slice.max(1);
    let mut at = 0usize;
    let mut last = FeedReport {
        phase: SessionPhase::Signature,
        rows: 0,
        total_rows: 0,
        consumed: 0,
        fault: None,
        complete: false,
    };
    while at < file.len() {
        let end = (at + step).min(file.len());
        last = s.feed(&file[at..end], sink);
        if last.failed() || last.complete {
            break;
        }
        at = end;
    }
    if !last.failed() && !last.complete {
        last = s.finish(sink);
    }
    last
}
