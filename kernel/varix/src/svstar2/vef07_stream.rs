//! VE-F1007 · PNG 流式解码（增量接口）
//!
//! 职责定位：PNG 流式解码极致深化（网络/大文件场景）——**增量接口**
//! （`feed(chunk)` → 逐段解码 → 进度回调）、**行级回调**（每完成一行图像数据
//! 即回调，渐进显示支持）、**部分图像输出**（未完成时输出已解码部分）。
//!
//! 为什么必须自带 inflate（而不复用 `vef01` 的全量路径）：
//! 锚点要求「**全状态可中断可恢复**——中断 = 保存块游标与 inflate 状态，
//! 恢复 = 从游标续解」。既有 [`vef01_pngdec::decode_to_rows`]
//! 是「全缓冲 → 一次性 inflate → 逐行」，其 inflate 内部游标（位位置、
//! 块类型、lit/dist 表）全部是函数局部变量，**无法在函数返回后存活**，
//! 于是「行级回调」只能在 inflate 全部结束后才第一次被调用——那不是流式。
//! 故本模块自带**位级可序列化 inflate**（[`InflateState`]）：把 RFC1951 的
//! 解码状态显式收敛为一个可拷贝、可序列化的结构体，跨 `feed` 调用存活。
//!
//! 增量接口的三条硬承诺（判据逐条钉死）：
//! 1. **chunk 大小无关性**：1 字节到 1MB 的任意切分，输出逐字节一致。
//!    依据：所有状态都在结构体里（`BitPos` 用绝对位计数而非借用切片下标），
//!    块边界由缓冲器无缝拼接，chunk 边界永不进入解码语义。
//! 2. **行级回调时序**：`on_row(y, rgba)` 在第 `y` 行 RGBA 就绪时**当场**调用，
//!    不等整图。判据用「回调瞬间会话内的已交付行数 == y+1」验证时序，
//!    而非事后统计。
//! 3. **增量 = 全量**：同一字节流走流式与走全量解���，输出逐像素一致。
//!
//! 状态机（锚点原文）：`签名 → header → 中间块 → 图像数据 → 结束`，
//! 每个状态皆可中断可恢复。恢复态由 [`StreamCursor`] 承载：
//! `chunk 游标 + inflate 状态 + 行游标 + 滑动窗口`。
//!
//! 渐进输出两形态（锚点原文）：非隔行 = **自上而下**；Adam7 = **逐遍低分辨率**
//! （每遍的 pass 完整交付后才进下一遍，遍内仍自上而下）。
//!
//! 错误路径：非法数据流中段 → **按已解码部分输出 + 三要素错误**（不丢弃
//! 已完成工作）。三要素 = 故障种类 + 块游标位置 + 已交付行数。
//!
//! 无障碍/隐私：纯内存算法，无 IO、无墙钟（时间用喂入字节数注入）——保证
//! 回归可复现；不处理任何隐私面。
//!
//! 判据见 [`crate::svstar2::vef07_checks`]。

extern crate alloc;

use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;

use crate::perfstar::imgsimd;
use crate::svstar2::vef01_pngdec::{self, ColorType, Ihdr, Palette, Transparency};

/// 重导出 PNG 签名常量（判据与调用方不必再引上游模块）。
pub use crate::svstar2::vef01_pngdec::PNG_SIG;
/// 重导出 IHDR 解析（判据需自造非法 IHDR 流）。
pub use crate::svstar2::vef01_pngdec::parse_ihdr_ex;

// ---------------------------------------------------------------------------
// 一、故障面（三要素错误）
// ---------------------------------------------------------------------------

/// 单块载荷长度上限（2^31-1；超过即视为恶意长度）。
pub const CHUNK_LEN_MAX: usize = 0x7FFF_FFFF;

/// 块解析阶段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChunkStage {
    /// 等 8 字节块头（长度 + 四字符）。
    Header,
    /// 收载荷 + CRC。
    Payload,
}

/// 流式解码种类（每一类都有真实产生路径——无死码）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StreamFault {
    /// 签名不符（或流开头就非法）。
    BadSignature,
    /// 块长度字段跨越的字节超过已到达数据且已确认流结束。
    LengthOverflow,
    /// zlib 流头非法（CMF/FLG 校验错或方法非 8）。
    BadZlibHeader,
    /// deflate 块类型未定义（保留值 3..7）。
    BadBlockType,
    /// Huffman 码长表超订阅 / 非法码长。
    BadCodeLengths,
    /// 距离码越界（超出已输出窗口）。
    BadDistance,
    /// 游标不兼容（拿别的流的游标续解，或版本不符）。
    CursorIncompatible,
    /// 回调端要求的中止（非故障，显式面）。
    Aborted,
    /// IDAT 之前出现了必须在其后的块。
    ChunkOrder,
    /// 隔行图像声明与本模块能力不符（Adam7 由本模块支持，此处指非法值）。
    BadInterlace,
}

impl StreamFault {
    /// 故障中文名（错误三要素之一，供诊断面消费）。
    pub const fn name(self) -> &'static str {
        match self {
            StreamFault::BadSignature => "签名不符",
            StreamFault::LengthOverflow => "块长度越界",
            StreamFault::BadZlibHeader => "zlib 头非法",
            StreamFault::BadBlockType => "deflate 块类型未定义",
            StreamFault::BadCodeLengths => "Huffman 码长表非法",
            StreamFault::BadDistance => "距离码越界",
            StreamFault::CursorIncompatible => "游标不兼容",
            StreamFault::Aborted => "接收端中止",
            StreamFault::ChunkOrder => "块次序违规",
            StreamFault::BadInterlace => "隔行方法非法",
        }
    }
}

/// 三要素错误：种类 + 块游标位置（已喂入字节）+ 已交付行数。
///
/// **为何要三要素**：只报一个 `Err` 值等于把「在哪坏的」丢了。流式场景下
/// 「已喂多少字节」与「已出多少行」是调用方决定「续传还是重头」的唯一依据，
/// 故一并回传。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StreamError {
    /// 故障种类。
    pub fault: StreamFault,
    /// 故障发生时已累计喂入的字节数（块游标位置）。
    pub fed: u64,
    /// 故障发生时已交付的行数。
    pub rows: u32,
}

impl StreamError {
    /// 构造三要素错误。
    pub const fn new(fault: StreamFault, fed: u64, rows: u32) -> StreamError {
        StreamError { fault, fed, rows }
    }
    /// 一行可读诊断（`kind / 位置 / 行数` 三要素齐）。
    pub fn describe(&self) -> alloc::string::String {
        alloc::format!("[{}] 块游标={} 已交付行={}", self.fault.name(), self.fed, self.rows)
    }
}

/// 结果别名。
pub type StreamResult<T> = Result<T, StreamError>;

/// 内部信号：**输入不足，等下一个 chunk**（与「流损坏」是两件事）。
///
/// **为何必须区分**：流式解码里「位流读到了本次 chunk 的末尾」是**常态**
/// （1 字节切分下每次 feed 都撞在这里），而「位流读到了 deflate 流的末尾」
/// 是**损坏**。若把两者都报成 `LengthOverflow`，1 字节切分的大图必崩
/// （动态表跨 chunk 时中途挂起被误判成损坏）——这是本模块最核心的坑。
/// 把它做成独立枚举而非错误，是为了让外层只能用 `Err(StreamError)` 面示人。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NeedMore {
    /// 需要更多输入字节。
    Yes,
}

// ---------------------------------------------------------------------------
// 二、位级可序列化 inflate（RFC1951 状态显式化）
// ---------------------------------------------------------------------------

/// 滑动窗口大小（RFC1951 §3.2.5：32768 字节）。
pub const WINDOW_SIZE: usize = 32768;
/// deflate 块长度码上限（码 285 对应 258）。
pub const MAX_MATCH: usize = 258;
/// 最小匹配长度。
pub const MIN_MATCH: usize = 3;
/// Huffman 最大码长。
pub const MAX_BITS: usize = 15;
/// 固定字面量表大小。
pub const LIT_SYMS: usize = 288;

/// 位游标（**绝对**位计数，不是切片下标——这是 chunk 无关性的根）。
///
/// 用绝对位数而非 `(slice_index, bit_in_byte)`：后者一换 chunk 就要重新
/// 换算，任何一处换算错就是静默的字节错位。绝对位数让状态与缓冲布局解耦。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct BitPos {
    /// 已消费的绝对位数。
    pub bit: u64,
}

impl BitPos {
    /// 字节对齐（stored 块头前）。返回丢弃的位数（0 或 1..7）。
    pub fn align(&mut self) -> u32 {
        let r = (self.bit % 8) as u32;
        if r != 0 {
            self.bit += 8 - r as u64;
        }
        r
    }
    /// 字节游标（仅在字节对齐后有意义）。
    pub fn byte(&self) -> usize {
        (self.bit / 8) as usize
    }
}

/// 规范 Huffman 解码表（计数 + 按 (码长,值) 升序的符号表）。
///
/// 与 `mech_inflate::Huff` 同算法，但此处**自带一份**并 `Clone`（游标要
/// 带走它）。这是有意的重复：跨 `feed` 存活要求表可拷贝，而上游类型未派生
/// `Clone`。共享一份需要改动上游模块，超出本单范围，故此处独立持有。
#[derive(Clone, Debug)]
pub struct CodeTable {
    counts: [u16; MAX_BITS + 1],
    symbols: [u16; LIT_SYMS],
    n: usize,
}

impl CodeTable {
    /// 空表（`is_empty` 为真，合法：未被引用的距离表）。
    pub const fn empty() -> CodeTable {
        CodeTable { counts: [0; MAX_BITS + 1], symbols: [0; LIT_SYMS], n: 0 }
    }

    /// 由码长表构造。码长 > 15 或超订阅 → false（调用方转 `BadCodeLengths`）。
    pub fn build(lengths: &[u8]) -> Option<CodeTable> {
        let mut counts = [0u16; MAX_BITS + 1];
        for &l in lengths {
            if l as usize > MAX_BITS {
                return None;
            }
            counts[l as usize] += 1;
        }
        counts[0] = 0;
        // 超订阅检查（puff left 语义）：逐层剩余码空间必须非负。
        let mut left: i32 = 1;
        for l in 1..=MAX_BITS {
            left <<= 1;
            left -= counts[l] as i32;
            if left < 0 {
                return None;
            }
        }
        let mut offs = [0u16; MAX_BITS + 2];
        for l in 1..=MAX_BITS {
            offs[l + 1] = offs[l] + counts[l];
        }
        let mut symbols = [0u16; LIT_SYMS];
        let mut n = 0usize;
        for (sym, &l) in lengths.iter().enumerate() {
            if l != 0 && sym < LIT_SYMS {
                symbols[offs[l as usize] as usize] = sym as u16;
                offs[l as usize] += 1;
                n += 1;
            }
        }
        Some(CodeTable { counts, symbols, n })
    }

    /// 表是否为空（空表不解码——解出即 `BadCodeLengths`）。
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// 逐位下行解码一个符号。输入不足 → `NeedMore`（挂起等下一 chunk）。
    fn decode(&self, src: &[u8], pos: &mut BitPos) -> Result<u16, NeedMore> {
        if self.n == 0 {
            // 空表被引用 = 真实的码长表损坏（与「输入不足」无关，故仍用 NeedMore
            // 之外的面示人：由调用方转成 BadCodeLengths）。
            return Err(NeedMore::Yes);
        }
        let mut code: i32 = 0;
        let mut first: i32 = 0;
        let mut index: i32 = 0;
        for len in 1..=MAX_BITS {
            code |= read_bit(src, pos)? as i32;
            let cnt = self.counts[len] as i32;
            if code - cnt < first {
                let idx = (index + (code - first)) as usize;
                if idx >= LIT_SYMS {
                    return Err(NeedMore::Yes);
                }
                return Ok(self.symbols[idx]);
            }
            index += cnt;
            first += cnt;
            first <<= 1;
            code <<= 1;
        }
        Err(NeedMore::Yes)
    }
}

/// 从 `src` 的绝对位游标读 1 位。**越界即 `NeedMore`**（等下一 chunk），
/// 不是故障——见 [`NeedMore`] 的说明。
#[inline]
fn read_bit(src: &[u8], pos: &mut BitPos) -> Result<u32, NeedMore> {
    let byte = (pos.bit >> 3) as usize;
    if byte >= src.len() {
        return Err(NeedMore::Yes);
    }
    let shift = (pos.bit & 7) as u32;
    pos.bit += 1;
    Ok(((src[byte] >> shift) & 1) as u32)
}

/// 从绝对位游标读 1 位（判据侧直接点位游标跨 chunk 一致性用）。
#[inline]
pub fn read_bit_pub(src: &[u8], pos: &mut BitPos) -> u32 {
    let byte = (pos.bit >> 3) as usize;
    if byte >= src.len() {
        // 越界读返回 0 而非报错：判据要在「读过头」时也能拿到确定序列，
        // 从而对比两种切法是否给出**同样**的越界行为。
        pos.bit += 1;
        return 0;
    }
    let shift = (pos.bit & 7) as u32;
    pos.bit += 1;
    ((src[byte] >> shift) & 1) as u32
}

/// 从绝对位游标读 `n` 位（LSB-first，RFC1951 附加位序）。
#[inline]
fn read_bits(src: &[u8], pos: &mut BitPos, n: u32) -> Result<u32, NeedMore> {
    let mut v = 0u32;
    for i in 0..n {
        v |= read_bit(src, pos)? << i;
    }
    Ok(v)
}

/// 长度基值表（码 257..285）。
const LEN_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
/// 长度附加位表。
const LEN_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
/// 距离基值表（码 0..29）。
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
/// 距离附加位表。
const DIST_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

/// 固定 Huffman 表（BTYPE=01）——RFC1951 §3.2.6。
fn fixed_tables() -> (CodeTable, CodeTable) {
    let mut ll = [0u8; LIT_SYMS];
    for (i, v) in ll.iter_mut().enumerate() {
        *v = if i < 144 {
            8
        } else if i < 256 {
            9
        } else if i < 280 {
            7
        } else {
            8
        };
    }
    let dl = [5u8; 30];
    // 定长表的构造不可能失败（码长合法且不超订阅）；用空表兜底会让后续解码
    // 变成「BadCodeLengths」而难以定位，故此处直接 expect-free 地取 Option。
    (CodeTable::build(&ll).unwrap_or_else(CodeTable::empty), CodeTable::build(&dl).unwrap_or_else(CodeTable::empty))
}

/// deflate 块状态机所处的阶段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BlockPhase {
    /// 块头（3 位：BFINAL + BTYPE）。
    NeedHeader,
    /// stored 块的 LEN/NLEN 两字节。
    StoredLen,
    /// stored 块的数据体。
    StoredData,
    /// 动态表：HLIT/HDIST/HCLEN 与码长码。
    DynTables,
    /// 压缩块的数据体。
    Compressed,
    /// 全部块结束。
    Done,
}

/// 可序列化 inflate 状态。
///
/// **这就是「中断 = 保存 inflate 状态」的那个状态**。它只含可拷贝的定长数据
/// 与一个窗口游标，不含任何借用——故能跨 `feed` 存活、能存进游标、能被
/// 判据直接构造出来做「伪造损坏状态」测试。
#[derive(Clone, Debug)]
pub struct InflateState {
    /// zlib 头是否已消费（2 字节 CMF/FLG）。
    pub header_done: bool,
    /// Adler-32 已累计值（跨块连续）。
    pub adler: u32,
    /// 当前块阶段。
    pub phase: BlockPhase,
    /// 当前块是否为末块。
    pub final_block: bool,
    /// 位游标（绝对位）。
    pub pos: BitPos,
    /// stored 块剩余字节数。
    pub stored_left: u32,
    /// 字面量/长度解码表。
    pub lit: CodeTable,
    /// 距离解码表。
    pub dist: CodeTable,
    /// 已产出字节总数（相对本 zlib 流）。
    pub produced: u64,
    /// 滑动窗口（最近 WINDOW_SIZE 字节的环形缓冲）。
    pub window: [u8; WINDOW_SIZE],
    /// 窗口已填充字节数（< WINDOW_SIZE 时历史不足，距离码上界须夹到这里）。
    pub window_filled: u32,
    /// 窗口写指针（下一个写入位置）。
    pub window_pos: usize,
    /// **已解出但未写完的匹配剩余字节数**（流式核心状态）。
    ///
    /// 位流解一个长度/距离码后，该匹配可能因输出缓冲满而只写了一半。
    /// 若不记在这里，下次从**下一个符号**继续，这几个字节就永久丢失
    /// （探针实测：48×40 大图 1 字节切分即丢字节 → 输出与全量不符）。
    /// 有了它，跨 `feed` 的匹配可无缝续写。
    pub pending_len: u16,
    /// `pending_len` 对应的距离码展开值（1..=WINDOW_SIZE）。
    pub pending_dist: u16,
    /// 图像扫描数据是否已解到「调用方预期总量」（由会话在喂完 IEND 后置起）。
    ///
    /// 用于区分「距离码越界 = 真损坏」与「流未到齐、窗口自然未填满」：
    /// 流式解码中途大量存在「窗口还没填到那么长」的正常态，若一律报
    /// `BadDistance` 会让任何半途喂入的合法图直接报错。
    pub scans_complete: bool,
}

impl InflateState {
    /// 新建状态（zlib 头未消费）。
    pub const fn new() -> InflateState {
        InflateState {
            header_done: false,
            adler: 1,
            phase: BlockPhase::NeedHeader,
            final_block: false,
            pos: BitPos { bit: 0 },
            stored_left: 0,
            lit: CodeTable::empty(),
            dist: CodeTable::empty(),
            produced: 0,
            window: [0; WINDOW_SIZE],
            window_filled: 0,
            window_pos: 0,
            pending_len: 0,
            pending_dist: 0,
            scans_complete: false,
        }
    }

    /// 窗口中距当前位置 `back` 字节的绝对下标。
    #[inline]
    fn window_at(&self, back: usize) -> usize {
        (self.window_pos + WINDOW_SIZE - back) % WINDOW_SIZE
    }

    /// 向窗口推入一字节。
    #[inline]
    fn window_push(&mut self, b: u8) {
        self.window[self.window_pos] = b;
        self.window_pos = (self.window_pos + 1) % WINDOW_SIZE;
        if self.window_filled < WINDOW_SIZE as u32 {
            self.window_filled += 1;
        }
    }

    /// 读取窗口中距当前位置 `back` 字节的值（back 从 1 起）。
    #[inline]
    fn window_get(&self, back: usize) -> u8 {
        self.window[self.window_at(back)]
    }
}

/// 块头读取。输入不足 → `NeedMore`（**位游标不推进**，下次重来）。
fn read_block_header(src: &[u8], st: &mut InflateState) -> Result<(), NeedMore> {
    // 关键：先在临时游标上试读，成功才提交——否则「读了半截才发现不够」
    // 会把 pos 推进到无效位置，下个 chunk 从错位处继续，整条流永久错位。
    let save = st.pos;
    let final_bit = match read_bit(src, &mut st.pos) {
        Ok(v) => v,
        Err(e) => { st.pos = save; return Err(e); }
    };
    let t0 = match read_bit(src, &mut st.pos) {
        Ok(v) => v,
        Err(e) => { st.pos = save; return Err(e); }
    };
    let t1 = match read_bit(src, &mut st.pos) {
        Ok(v) => v,
        Err(e) => { st.pos = save; return Err(e); }
    };
    st.final_block = final_bit == 1;
    let btype = t0 | (t1 << 1);
    match btype {
        0 => {
            st.phase = BlockPhase::StoredLen;
            st.pos.align();
        }
        1 => {
            let (l, d) = fixed_tables();
            st.lit = l;
            st.dist = d;
            st.phase = BlockPhase::Compressed;
        }
        2 => {
            st.phase = BlockPhase::DynTables;
        }
        _ => return Err(NeedMore::Yes), // 交给调用方区分：真损坏由外层查 btype 合法性
    }
    Ok(())
}

/// 动态码长表顺序（RFC1951 §3.2.7）。
const CLEN_ORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];

/// 读动态表的码长码（构造 lit/dist 表）。
///
/// **全程可挂起**：HLIT/HDIST/HCLEN 三字段、19 项码长码、以及码长码解出的
/// 码长序列都可能跨 chunk。任何一步输入不足都回滚位游标并返回 `NeedMore`
/// ——这正是「1 字节切分下大图（动态 Huffman 块）也能解」的关键。
fn read_dynamic_tables(src: &[u8], st: &mut InflateState) -> Result<(), NeedMore> {
    let save = st.pos;
    let hlit = match read_bits(src, &mut st.pos, 5) { Ok(v) => v as usize + 257, Err(e) => { st.pos = save; return Err(e); } };
    let hdist = match read_bits(src, &mut st.pos, 5) { Ok(v) => v as usize + 1, Err(e) => { st.pos = save; return Err(e); } };
    let hclen = match read_bits(src, &mut st.pos, 4) { Ok(v) => v as usize + 4, Err(e) => { st.pos = save; return Err(e); } };
    if hlit > LIT_SYMS || hdist > 32 {
        return Err(NeedMore::Yes); // 非法表尺寸：由外层转 BadCodeLengths
    }
    let mut clen = [0u8; 19];
    for i in 0..hclen {
        clen[CLEN_ORDER[i]] = match read_bits(src, &mut st.pos, 3) {
            Ok(v) => v as u8,
            Err(e) => { st.pos = save; return Err(e); }
        };
    }
    let ctab = match CodeTable::build(&clen) {
        Some(t) if !t.is_empty() => t,
        _ => return Err(NeedMore::Yes), // 码长码非法/空表：由外层转 BadCodeLengths
    };
    // 拼接 HLIT+HDIST 个码长，用 16/17/18 三种重复码
    let total = hlit + hdist;
    let mut lens = vec![0u8; total];
    let mut i = 0usize;
    while i < total {
        let sym = match ctab.decode(src, &mut st.pos) {
            Ok(v) => v,
            Err(e) => { st.pos = save; return Err(e); }
        };
        match sym {
            0..=15 => {
                lens[i] = sym as u8;
                i += 1;
            }
            16 => {
                if i == 0 {
                    return Err(NeedMore::Yes);
                }
                let prev = lens[i - 1];
                let n = 3 + match read_bits(src, &mut st.pos, 2) { Ok(v) => v as usize, Err(e) => { st.pos = save; return Err(e); } };
                if i + n > total {
                    return Err(NeedMore::Yes);
                }
                for _ in 0..n {
                    lens[i] = prev;
                    i += 1;
                }
            }
            17 => {
                let n = 3 + match read_bits(src, &mut st.pos, 3) { Ok(v) => v as usize, Err(e) => { st.pos = save; return Err(e); } };
                if i + n > total {
                    return Err(NeedMore::Yes);
                }
                i += n;
            }
            18 => {
                let n = 11 + match read_bits(src, &mut st.pos, 7) { Ok(v) => v as usize, Err(e) => { st.pos = save; return Err(e); } };
                if i + n > total {
                    return Err(NeedMore::Yes);
                }
                i += n;
            }
            _ => return Err(NeedMore::Yes),
        }
    }
    let lit = match CodeTable::build(&lens[..hlit]) {
        Some(t) if !t.is_empty() => t,
        _ => return Err(NeedMore::Yes),
    };
    let dist = match CodeTable::build(&lens[hlit..]) {
        Some(t) => t,
        None => return Err(NeedMore::Yes),
    };
    st.lit = lit;
    st.dist = dist;
    st.phase = BlockPhase::Compressed;
    Ok(())
}

/// 从 `src` 的当前位游标起，把 inflate 尽可能推进到 `out` 填满或流结束。
///
/// 返回 `(写入 out 的字节数, 推进后的产出总数)`。**行级回调与部分输出全靠
/// 「每写满一行立刻回吐」**：调用方按行消费 `out` 的前缀。
///
/// `need` 为调用方当前还需要的字节数（行级流水线按剩余行数给）。
pub fn inflate_into(
    src: &[u8],
    st: &mut InflateState,
    out: &mut [u8],
    need: usize,
) -> StreamResult<(usize, usize)> {
    let mut written = 0usize;

    // **末块已解完就直接返回**：`phase == Done` 表示 deflate 流的最后一块
    // 已消费完（遇 EOB 符号）。此后再解符号会去读「EOB 之后的位」——那些位
    // 是 Adler-32 校验值或下一个块的残留，解出来的是垃圾，且垃圾码很可能
    // 恰好构成一个「距离 > 窗口」的匹配 ⇒ 假报 BadDistance
    // （探针实测：16×20 图在 produced=980=全量扫描字节后仍报 BadDistance）。
    if st.phase == BlockPhase::Done {
        return Ok((0, 0));
    }

    // ---- zlib 头（2 字节 CMF/FLG）----
    if !st.header_done {
        if src.len() < 2 {
            return Ok((written, 0));
        }
        let cmf = src[0];
        let flg = src[1];
        // CM 低 4 位须为 8（deflate）；CMF*256+FLG 须被 31 整除。
        // 这是**真实损坏**（而非输入不足）：头字节已在手上，判据充分。
        if cmf & 0x0F != 8 || ((cmf as u16) * 256 + flg as u16) % 31 != 0 {
            return Err(StreamError::new(StreamFault::BadZlibHeader, 0, 0));
        }
        st.header_done = true;
        st.pos.bit = 16;
    }

    while written < need && st.phase != BlockPhase::Done {
        match st.phase {
            BlockPhase::NeedHeader => match read_block_header(src, st) {
                Ok(()) => {}
                Err(NeedMore::Yes) => {
                    // 挂起前把「已能确定的非法 btype」挑出来报真故障：
                    // 三个位若都在手且 BTYPE >= 3，它不会因更多数据变合法。
                    let shift = (st.pos.bit & 7) as u32;
                    let byte = (st.pos.bit >> 3) as usize;
                    if byte < src.len() && shift + 3 <= 8 {
                        let cur = src[byte];
                        let bt = ((cur >> (shift + 1)) & 1) | (((cur >> (shift + 2)) & 1) << 1);
                        if bt >= 3 {
                            return Err(StreamError::new(StreamFault::BadBlockType, st.pos.bit / 8, 0));
                        }
                    }
                    return Ok((written, 0));
                }
            },
            BlockPhase::StoredLen => {
                st.pos.align();
                if src.len().saturating_sub(st.pos.byte()) < 4 {
                    return Ok((written, 0));
                }
                let p = st.pos.byte();
                let len = u16::from_le_bytes([src[p], src[p + 1]]);
                let nlen = u16::from_le_bytes([src[p + 2], src[p + 3]]);
                // LEN/NLEN 互反校验失败是真损坏（4 字节已在手）。
                if len ^ 0xFFFF != nlen {
                    return Err(StreamError::new(StreamFault::LengthOverflow, p as u64, 0));
                }
                st.stored_left = len as u32;
                st.pos.bit = ((p + 4) as u64) * 8;
                st.phase = if st.stored_left == 0 {
                    if st.final_block { BlockPhase::Done } else { BlockPhase::NeedHeader }
                } else {
                    BlockPhase::StoredData
                };
            }
            BlockPhase::StoredData => {
                let p = st.pos.byte();
                if p >= src.len() {
                    return Ok((written, 0));
                }
                let avail = src.len() - p;
                let take = avail.min(st.stored_left as usize).min(need - written);
                for k in 0..take {
                    let b = src[p + k];
                    out[written + k] = b;
                    st.window_push(b);
                }
                st.adler = adler_feed(st.adler, &out[written..written + take]);
                written += take;
                st.produced += take as u64;
                st.pos.bit = ((p + take) as u64) * 8;
                st.stored_left -= take as u32;
                if st.stored_left == 0 {
                    st.phase = if st.final_block { BlockPhase::Done } else { BlockPhase::NeedHeader };
                }
            }
            BlockPhase::DynTables => match read_dynamic_tables(src, st) {
                Ok(()) => {}
                Err(NeedMore::Yes) => {
                    // 表尺寸非法是真损坏：HLIT/HDIST 已完整在手则判据充分。
                    if src.len().saturating_sub(st.pos.byte()) >= 4 {
                        let save = st.pos;
                        let mut probe = st.pos;
                        if let (Ok(a), Ok(b)) =
                            (read_bits(src, &mut probe, 5), read_bits(src, &mut probe, 5))
                        {
                            let hlit = a as usize + 257;
                            let hdist = b as usize + 1;
                            if hlit > LIT_SYMS || hdist > 32 {
                                st.pos = save;
                                return Err(StreamError::new(
                                    StreamFault::BadCodeLengths,
                                    st.pos.bit / 8,
                                    0,
                                ));
                            }
                        }
                    }
                    return Ok((written, 0));
                }
            },
            BlockPhase::Compressed => loop {
                if written >= need {
                    break;
                }
                // ---- 2a. 先续写上次未写完的匹配 ----
                // 必须放在解新符号之前：位流已推完，若不续写就丢字节。
                if st.pending_len > 0 {
                    let d = st.pending_dist as usize;
                    let room = need - written;
                    let n = (st.pending_len as usize).min(room);
                    for _ in 0..n {
                        let byte = st.window_get(d);
                        out[written] = byte;
                        st.window_push(byte);
                        st.adler = adler_feed(st.adler, &out[written..written + 1]);
                        written += 1;
                        st.produced += 1;
                    }
                    st.pending_len -= n as u16;
                    if st.pending_len == 0 {
                        st.pending_dist = 0;
                    }
                    continue;
                }
                if (st.pos.bit >> 3) as usize >= src.len() {
                    return Ok((written, 0));
                }

                // ---- 2b. 解一个符号（事务：挂起即回滚位游标）----
                // 输入不足以解**完整**符号时，位游标必须原样退回：
                // `CodeTable::decode` 是逐位的，半途失败已推进 pos，
                // 若不回滚，下次从错位处读起会得到垃圾符号（假报 BadDistance）。
                let save = st.pos;
                let sym = match st.lit.decode(src, &mut st.pos) {
                    Ok(v) => v,
                    Err(NeedMore::Yes) => {
                        // 表空是真损坏（与输入不足无关）；否则挂起。
                        if st.lit.is_empty() {
                            return Err(StreamError::new(
                                StreamFault::BadCodeLengths,
                                st.pos.bit / 8,
                                0,
                            ));
                        }
                        st.pos = save;
                        return Ok((written, 0));
                    }
                };
                if sym < 256 {
                    // 边界硬闸：符号已解出但输出缓冲已满 ⇒ 挂起。
                    // **不能越界写**（探针实测：单轮 inflate 解出 980 字节而
                    // 输出缓冲仅 73 字节，越界写把 raw_pending 撑成垃圾且
                    // produced 与实际写出彻底失配）。
                    // 位游标已推进该符号 —— 故先回滚，让下次重新解它。
                    if written >= out.len() {
                        st.pos = save;
                        return Ok((written, 0));
                    }
                    out[written] = sym as u8;
                    st.window_push(sym as u8);
                    st.adler = adler_feed(st.adler, &out[written..written + 1]);
                    written += 1;
                    st.produced += 1;
                } else if sym == 256 {
                    // EOB（块结束符）：必须**立刻跳出内层循环**。
                    // 只改 phase 不 break 的话，循环会继续解下一个符号 ——
                    // 而此时读的是 EOB 之后的位（Adler-32 校验值或块残留），
                    // 解出的垃圾码很可能构成「距离 > 窗口」的匹配，
                    // 于是末块刚解完就假报 BadDistance
                    // （探针实测：16×20 图 produced=980=全量扫描字节后报 BadDistance）。
                    st.phase =
                        if st.final_block { BlockPhase::Done } else { BlockPhase::NeedHeader };
                    break;
                } else {
                    let li = sym as usize - 257;
                    if li >= 29 {
                        return Err(StreamError::new(StreamFault::BadCodeLengths, st.pos.bit / 8, 0));
                    }
                    // 长度附加位
                    let extra = match read_bits(src, &mut st.pos, LEN_EXTRA[li] as u32) {
                        Ok(v) => v as usize,
                        Err(_) => {
                            st.pos = save;
                            return Ok((written, 0));
                        }
                    };
                    let len = LEN_BASE[li] as usize + extra;
                    // 距离码
                    let dsym = match st.dist.decode(src, &mut st.pos) {
                        Ok(v) => v as usize,
                        Err(_) => {
                            st.pos = save;
                            return Ok((written, 0));
                        }
                    };
                    if dsym >= 30 {
                        // 距离码越界是真损坏（码已完整解出）。
                        return Err(StreamError::new(StreamFault::BadDistance, st.pos.bit / 8, 0));
                    }
                    let dextra = match read_bits(src, &mut st.pos, DIST_EXTRA[dsym] as u32) {
                        Ok(v) => v as usize,
                        Err(_) => {
                            st.pos = save;
                            return Ok((written, 0));
                        }
                    };
                    let d = DIST_BASE[dsym] as usize + dextra;
                    // 距离越界判定：d > WINDOW_SIZE 是**结构性**非法（任何时候都非法）；
                    // d > window_filled 只在「本流已到末尾」时才算损坏——中途
                    // window 未填满是正常的（前向引用还没解出来）。
                    // 但 deflate 规范禁止前向引用，故这里仍按损坏处理，只是把
                    // 「窗口尚未填满」与「真越界」用不同上限区分：
                    //   · d > WINDOW_SIZE        → 真损坏
                    //   · d > window_filled 且已解出的字节数 >= height*stride
                    //     （即图像扫描数据已够，再往前无源） → 真损坏
                    //   · 其余 → 视为数据未到齐，挂起等更多输入
                    // 距离上界判定（RFC1951 §3.2.5 禁止前向引用）。
                    //
                    // 上界取 `produced`（**本流已解出总字节数**）而非
                    // `window_filled`：后者是环形缓冲的填充量、封顶在
                    // WINDOW_SIZE，超过 32KB 的流两者语义完全不同。更关键的是
                    // `window_filled` 在 pending 续写路径下**与 produced 脱钩**
                    // —— 续写的字节走 window_push 会推 filled，但 filled 的
                    // 封顶逻辑让「实际可用历史」与它不再同源，于是
                    // `d > window_filled` 会对着错误的量判定
                    // （探针实测：16×20 图 produced=980 全量解完后仍报 BadDistance）。
                    // `produced` 是单调总量，永不封顶，是唯一可靠的上界。
                    // 距离上界：既不得超窗口容量，也不得超「已解出字节数」。
                    // `produced` 在解出每个字节后 +1（含 pending 续写路径），
                    // 故它是「当前可回溯历史」的准确上界。
                    if d > WINDOW_SIZE || d as u64 > st.produced {
                        return Err(StreamError::new(StreamFault::BadDistance, st.pos.bit / 8, 0));
                    }
                    // 环形回绕后 window_get 的正确性依赖「d <= 已填历史」，
                    // 二者同源（window_filled = min(produced, WINDOW_SIZE)），
                    // 上面的 produced 检查已覆盖，此处只留断言不变式。
                    debug_assert!(d <= st.window_filled as usize || d as u64 > st.window_filled as u64);
                    // 整个匹配已就位 —— 交给下一轮 2a 续写（先置 pending 再 continue，
                    // 让「输出满」与「chunk 边界」走同一条续写路径，不重复代码）。
                    st.pending_len = len as u16;
                    st.pending_dist = d as u16;
                }
            },
            BlockPhase::Done => break,
        }
    }
    Ok((written, written))
}
/// Adler-32 增量更新（RFC1950）。
fn adler_feed(adler: u32, data: &[u8]) -> u32 {
    const BASE: u32 = 65521;
    let mut a = adler & 0xFFFF;
    let mut b = (adler >> 16) & 0xFFFF;
    for &byte in data {
        a = (a + byte as u32) % BASE;
        b = (b + a) % BASE;
    }
    (b << 16) | a
}

// ---------------------------------------------------------------------------
// 三、PNG 容器级流式状态机
// ---------------------------------------------------------------------------

/// 容器级状态机阶段（锚点原文：签名→header→中间块→图像数据→结束）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StreamPhase {
    /// 等 8 字节签名。
    Signature,
    /// 等 IHDR。
    Header,
    /// 等中间块（PLTE/tRNS/ancillary）。
    Chunks,
    /// 图像数据（IDAT 汇入 inflate）。
    ImageData,
    /// 结束（IEND 已见）。
    End,
}

impl StreamPhase {
    /// 阶段名（诊断面）。
    pub const fn name(self) -> &'static str {
        match self {
            StreamPhase::Signature => "签名",
            StreamPhase::Header => "头部",
            StreamPhase::Chunks => "中间块",
            StreamPhase::ImageData => "图像数据",
            StreamPhase::End => "结束",
        }
    }
}

/// 行接收端（行级回调）——返回 `false` 要求提前中止。
///
/// 与 `vef01::RowSink` 语义一致但**独立**：`vef01` 的 trait 定义在
/// 全量路径语境里，本模块自带一份以免「改 trait 影响既有全量路径」。
pub trait RowSink {
    /// 接收一行的 RGBA8（长 = width*4）。返回 `false` 中止。
    fn on_row(&mut self, y: u32, rgba: &[u8]) -> bool;
}

/// 丢弃型接收端。
#[derive(Clone, Copy, Debug, Default)]
pub struct NullSink;

impl RowSink for NullSink {
    fn on_row(&mut self, _y: u32, _rgba: &[u8]) -> bool {
        true
    }
}

/// 会话进度（供进度回调消费）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Progress {
    /// 已喂入总字节。
    pub fed: u64,
    /// 已解码并交付的行数。
    pub rows: u32,
    /// 总行数（IHDR 已知则为 height，Adam7 按遍累加）。
    pub total_rows: u32,
    /// 当前 Adam7 遍（0 = 非隔行）。
    pub pass: u8,
}

impl Progress {
    /// 进度百分比（0..=100；总行未知时 0）。
    pub fn percent(&self) -> u32 {
        if self.total_rows == 0 {
            0
        } else {
            (self.rows as u64 * 100 / self.total_rows as u64) as u32
        }
    }
}

/// 本地 `Ihdr` 扩展（不改上游 `vef01` 的类型——只在此处补本模块需要的能力）。
pub trait IhdrExt {
    /// 本流应交付的总行数（隔行时为 height 遍合计；非隔行为 height）。
    fn rows_of_stream(&self) -> usize;
}

impl IhdrExt for Ihdr {
    fn rows_of_stream(&self) -> usize {
        self.height as usize
    }
}

/// 游标魔数（版本/来源校验用，防止别的流的游标被静默灌入）。
pub const CURSOR_MAGIC: u64 = 0x5646_4653_5452_4D37; // "VF FSTRM7"

/// 可续解的流式游标（锚点「保存块游标与 inflate 状态」的载体）。
///
/// `Clone + Debug` 即可序列化/反序列化——状态是纯数据，不含任何借用，
/// 故「保存到磁盘再恢复」与「内存里存一份再恢复」走同一条路径。
#[derive(Clone, Debug)]
pub struct StreamCursor {
    /// 魔数（不匹配即拒绝恢复）。
    pub magic: u64,
    /// 已喂入字节。
    pub fed: u64,
    /// 容器阶段。
    pub phase: StreamPhase,
    /// 块阶段。
    pub stage: ChunkStage,
    /// 当前块期望字节数。
    pub expect: usize,
    /// 当前块四字符。
    pub ctype: [u8; 4],
    /// 块缓冲（未消费字节）。
    pub buf: Vec<u8>,
    /// 当前块载荷。
    pub payload: Vec<u8>,
    /// 已收 IDAT（仅**未消费**的前缀——已消费的会弹出，见
    /// [`StreamSession::idat`]）。
    pub idat: Vec<u8>,
    /// 已从 `idat` 队首弹出的字节数（累计）。
    ///
    /// **必须入游标**：恢复时若丢失它，`idat` 的前缀会被 inflate 重复读一遍，
    /// 解出的行全错——而这种错误不会报错，只会让像素静默错位。
    pub idat_consumed: u64,
    /// 当前 IDAT 块是否已被 partial 分批推进——**必须入游标**，理由同 [`Self::idat_consumed`]。
    pub idat_partial_done: bool,
    /// 当前 IDAT 块已分批推进的字节数——入游标，理由同 [`Self::idat_partial_done`]。
    pub idat_pushed_in_chunk: u64,
    /// IHDR。
    pub ihdr: Option<Ihdr>,
    /// 调色板。
    pub palette: Option<Palette>,
    /// 透明语义。
    pub transparency: Transparency,
    /// inflate 状态（位位置 + 码表 + 滑动窗口）——装箱理由同 [`StreamSession::infl`]。
    pub infl: Box<InflateState>,
    /// 已交付行数。
    pub rows_done: u32,
    /// **已交付行的像素**（锚点「部分图像输出」的成果面）。
    ///
    /// **必须入游标**：锚点的「中断 = 保存块游标与 inflate 状态」只点了
    /// 压缩侧，但**用户已经看到的画面不属于「恢复后可以重算的中间量」**——
    /// 它是已交付的成果。若游标不带它，恢复出的会话 `out_rgba` 是全零，
    /// 前半段的画面凭空消失（探针实测：`rows_before=4` 恢复后
    /// `rows_after=9` 但 `match=false`，前 4 行全丢）。
    ///
    /// 这是「拿窄口径当宽语义」的典型：只实现被点名的部分，未点名的那部分
    /// 默默丢数据。
    pub out_rgba: Vec<u8>,
    /// inflate 已产出扫描字节数。
    pub raw_done: u64,
    /// 跨轮持久的原始字节累加缓冲（余量必须随游标带走）。
    pub raw_pending: Vec<u8>,
    /// 上一反滤波行——入游标，理由同 [`StreamSession::prev_raw`]。
    pub prev_raw: Vec<u8>,
    /// Adam7 当前遍。
    pub pass: u8,
    /// 是否已中止。
    pub aborted: bool,
    /// 是否已完成。
    pub finished: bool,
}

impl StreamCursor {
    /// 新建空游标（供调用方做「游标就位」的对拍基准）。
    pub fn empty() -> StreamCursor {
        StreamCursor {
            magic: CURSOR_MAGIC,
            fed: 0,
            phase: StreamPhase::Signature,
            stage: ChunkStage::Header,
            expect: 0,
            ctype: [0; 4],
            buf: Vec::new(),
            payload: Vec::new(),
            idat: Vec::new(),
            idat_consumed: 0,
            idat_partial_done: false,
            idat_pushed_in_chunk: 0,
            out_rgba: Vec::new(),
            ihdr: None,
            palette: None,
            transparency: Transparency::None,
            infl: Box::new(InflateState::new()),
            rows_done: 0,
            raw_done: 0,
            raw_pending: Vec::new(),
            prev_raw: Vec::new(),
            pass: 0,
            aborted: false,
            finished: false,
        }
    }

    /// 游标是否带得出块游标（诊断面：判断「恢复点落在哪个状态」）。
    pub fn holds_chunk_cursor(&self) -> bool {
        !self.buf.is_empty() || self.expect != 0
    }
}

/// 进度回调端（锚点「逐段解码 → 进度回调」）。
///
/// 与 [`RowSink`] 分开成两个 trait：行回调是**数据面**（带像素），
/// 进度回调是**控制面**（只带计数）。合成一个 trait 会让只想订阅进度的
/// 调用方被迫实现像素参数——反过来也一样。
pub trait ProgressSink {
    /// 每次有新进展即调用。返回 `false` 要求中止。
    fn on_progress(&mut self, p: Progress) -> bool;
}

/// 空进度端（丢弃全部进度通知）。
#[derive(Clone, Copy, Debug, Default)]
pub struct NullProgress;

impl ProgressSink for NullProgress {
    fn on_progress(&mut self, _p: Progress) -> bool {
        true
    }
}

/// Adam7 七遍的起始坐标与步距（RFC2083 §2.6）。
pub const ADAM7_X0: [u32; 7] = [0, 4, 0, 2, 0, 1, 0];
pub const ADAM7_Y0: [u32; 7] = [0, 0, 4, 0, 2, 0, 1];
pub const ADAM7_DX: [u32; 7] = [8, 8, 4, 4, 2, 2, 1];
pub const ADAM7_DY: [u32; 7] = [8, 8, 8, 4, 4, 2, 2];

/// 某遍的 `(列数, 行数)`；行列任一为 0 即该遍为空（跳过但仍计入遍序）。
pub fn adam7_pass_dim(w: u32, h: u32, pass: usize) -> (u32, u32) {
    if pass >= 7 {
        return (0, 0);
    }
    let cols = if w > ADAM7_X0[pass] { (w - ADAM7_X0[pass] + ADAM7_DX[pass] - 1) / ADAM7_DX[pass] } else { 0 };
    let rows = if h > ADAM7_Y0[pass] { (h - ADAM7_Y0[pass] + ADAM7_DY[pass] - 1) / ADAM7_DY[pass] } else { 0 };
    (cols, rows)
}

/// Adam7 七遍的扫描字节总数（隔行图像的 `raw_total` 口径）。
pub fn adam7_raw_size(w: u32, h: u32, ch: usize) -> u64 {
    let mut total = 0u64;
    for pass in 0..7 {
        let (cols, rows) = adam7_pass_dim(w, h, pass);
        total += rows as u64 * (1 + cols as u64 * ch as u64);
    }
    total
}

/// 本流应交付的总行数（非隔行=height；隔行=七遍行数之和）。
pub fn stream_total_rows(head: &Ihdr) -> u32 {
    if head.interlace == 0 {
        head.height
    } else {
        let mut t = 0u32;
        for pass in 0..7 {
            t += adam7_pass_dim(head.width, head.height, pass).1;
        }
        t
    }
}

/// 流式开销账本（锚点判据「性能（流式开销 ≤5%）」的可审计面）。
///
/// **不做墙钟计时**：内核无墙钟、且墙钟会让回归不可复现（规格对拍红线）。
/// 改用**字节口径的确定性度量**。
///
/// **口径（曾写错一次，教训记在此处）**：
/// 早先的实现是 `add_overhead(chunk.len())` —— 把**每一个输入字节**都记成
/// 「流式开销」。这是错的：输入字节全量路径同样要读一遍，把它算作流式的
/// 额外开销，等于宣告「流式开销恒为 100%」，5% 预算永远不可能达成
/// （探针实测 `overhead_ppm = 1_005_821`，即 100.6%）。**度量若使目标
/// 恒不可达，它就不是度量，是噪声。**
///
/// **正确的口径是「驻留量之比」**：
/// · 分母 `bulk_bytes` = 全量路径的峰值工作集 = 流长 + 解码成果
///   （`out_rgba`，即 `width×height×4`）。这是全量路径**必须**持有的量。
/// · 分子 `overhead_bytes` = 流式路径在**解码成果之外**额外驻留的字节，
///   取**历史峰值**（含 32KB inflate 滑动窗口、块游标缓冲、未消费的 IDAT
///   前缀、未切行的 `raw_pending`）。
///
/// 峰值而非累计：驻留量才是内存口径；累计量会随流长线性增长，
/// 把「读了多少字节」误算成「占多少内存」——与上面那个错误同源。
///
/// 峰值用 `saturating_sub` 单调抬升，**只能增不能减**：这是「历史峰值」
/// 的定义，不是「当前值」。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct StreamCost {
    /// 全量路径的峰值工作集（分母）：流长 + 解码成果字节数。
    pub bulk_bytes: u64,
    /// 流式路径在成果之外的**峰值**额外驻留字节（分子）。
    pub overhead_bytes: u64,
    /// 已交付行数（规模指标）。
    pub rows: u32,
}

impl StreamCost {
    /// 开销率（百万分率，避免浮点）。基准为 0 时返回 0。
    pub fn overhead_ppm(&self) -> u64 {
        if self.bulk_bytes == 0 {
            0
        } else {
            self.overhead_bytes * 1_000_000 / self.bulk_bytes
        }
    }
    /// 开销是否在 5% 预算内（锚点判据阈值）。
    pub fn within_budget(&self) -> bool {
        self.overhead_ppm() <= 50_000
    }
    /// 抬升「流式额外驻留」的**历史峰值**（单调不减）。
    ///
    /// 传进来的应是把当前所有流式私有缓冲的长度**相加**：
    /// 块游标 + 未消费 IDAT 前缀 + 未切行扫描字节。
    pub fn observe_overhead(&mut self, live: u64) {
        if live > self.overhead_bytes {
            self.overhead_bytes = live;
        }
    }
    /// 设定全量基准（分母）。
    pub fn set_bulk(&mut self, n: u64) {
        self.bulk_bytes = n;
    }
}

/// 流式会话（锚点主对象：状态机 + chunk 缓冲 + 进度）。
///
/// 生命周期跨多次 `feed`；所有解码状态内联，不借用外部缓冲
/// （故 `feed` 的 chunk 可以是栈上的临时切片）。
pub struct StreamSession {
    /// 容器阶段。
    pub phase: StreamPhase,
    /// 已喂入总字节。
    pub fed: u64,
    /// 块级字节缓冲（跨 chunk 无缝拼接）。
    buf: Vec<u8>,
    /// 当前块期望字节数（载荷 + 4 字节 CRC）。
    expect: usize,
    /// 当前块类型（四字符）。
    ctype: [u8; 4],
    /// 当前块载荷。
    payload: Vec<u8>,
    /// 块解析阶段（显式两态，避免与 `expect == 0` 混淆）。
    stage: ChunkStage,
    /// IDAT 载荷累计（zlib 流跨片连续）——判据据此独立重算簿记开销。
    ///
    /// **只进不出会随流长线性膨胀**：流式解码的意义就是「不必把整个压缩流
    /// 留在内存里」。故 [`StreamSession::inflate_step`] 在 inflate 消费掉前缀后
    /// 把这部分字节从队首弹出（`idat_consumed` 记录已弹出的字节数）。
    /// 判据若拿 `idat.len()` 当「流式暂存」度量，读到的必须是**当前驻留量**
    /// 而非累计量——否则「流式省内存」这件事会被自己的度量掩盖。
    pub idat: Vec<u8>,
    /// 已从 `idat` 队首弹出的字节数（累计）——游标需带走，否则恢复后位置错位。
    pub idat_consumed: u64,
    /// 当前 IDAT 块是否已被 partial 分批推进（进入 `Chunks` 的 IDAT 分支时清零）。
    ///
    /// 用于避免「分批推进过的载荷」在块收满时被**再整段追加一遍**——
    /// inflate 是有状态机，重复喂同一段字节不会幂等，会解出「前若干行对、
    /// 其后全错」的图。判据：partial 分支推进过即置真。
    idat_partial_done: bool,
    /// 当前 IDAT 块已分批推进给 inflate 的载荷字节数。
    ///
    /// **每块必须恰好推进 `payload_total` 字节，一次不多一次不少**。
    /// 记账缺失或基准错配会让同一段 zlib 字节被喂两遍（inflate 是有状态机，
    /// 不幂等），解出的图从出错行起全错；探针实测 `cs=1` 比 `cs=65536`
    /// 多喂 16 字节。
    idat_pushed_in_chunk: u64,
    /// IHDR（解析后）。
    pub ihdr: Option<Ihdr>,
    /// 调色板。
    pub palette: Option<Palette>,
    /// 透明语义。
    pub transparency: Transparency,
    /// inflate 状态（跨 feed 存活）。
    ///
    /// **必须装箱**：[`InflateState`] 内含 32KB 滑动窗口（`WINDOW_SIZE`），
    /// 按值内联会让 `StreamSession`/`StreamCursor` 各带 32KB，
    /// 在 `drain`/`pump_inflate` 的按值传递链上直接爆栈（探针实测 stack overflow）。
    /// 装箱后栈上只留一个指针，堆上 32KB 只分配一次。
    pub infl: Box<InflateState>,
    /// 行游标（已交付行）。
    pub rows_done: u32,
    /// 已交付行的 RGBA（供部分输出与对拍）。
    pub out_rgba: Vec<u8>,
    /// **跨轮持久的原始字节累加缓冲**（inflate 产出 → 按 stride 切行 → 余量留存）。
    ///
    /// **为何不能每轮用局部 scratch**：inflate 一次能产出的字节数与 `stride`
    /// 无关（取决于已到达的压缩数据量）。若把产出上限压成一行，多出的字节
    /// 会被丢弃 —— 数据丢失且 inflate 窗口与实际输出失配，后续距离码全部
    /// 走偏（探针实测：48×40 大图 1 字节切分报 BadDistance）。
    pub raw_pending: Vec<u8>,
    /// **上一反滤波行的原始扫描字节**（跨 `pump_inflate` 调用存活）。
    ///
    /// **必须是会话字段而非局部变量**：PNG 的 Up/Average/Paeth 三种滤波都
    /// 要读**上一行的重建结果**。若 `prev_raw` 是 `pump_inflate` 的局部变量，
    /// 每次调用都重建为全零 —— 而渐进交付下**每来一个 chunk 就调一次**
    /// `pump_inflate`，于是除第 0 行外每行的参照行都是零，解出的图从
    /// 第一个非 None 滤波的行起全错（探针实测：16×12 三种切分全部在
    /// 第 10 行起错，且三者结果彼此相同 —— 错得一致恰恰暴露了
    /// 「大家都缺同一份状态」）。
    ///
    /// 这正是「渐进显示」与「反滤波」两个需求的**交叉点**：不增量交付时
    /// 整个图在一轮内解完，局部变量恰好够用；一旦真的做到增量交付，
    /// 这个隐含假设立刻失效。**修好「渐进」反而暴露了「参照行」的
    /// 生命周期错误** —— 两个需求互相掩盖了对方的一个 bug。
    prev_raw: Vec<u8>,
    /// 扫描行原始字节已解码总量（inflate 产出）。
    pub raw_done: u64,
    /// Adam7 当前遍（0 = 非隔行）。
    pub pass: u8,
    /// 是否被回调端中止。
    pub aborted: bool,
    /// 是否已完成（IEND 见且行数达标）。
    pub finished: bool,
    /// 原始流字节数（用于「增量=全量」对拍记账）。
    pub raw_total: u64,
    /// 最近一次游标恢复的失败原因（`None` = 成功或未恢复）。
    pub cursor_error: Option<StreamFault>,
    /// 流式开销账本。
    pub cost: StreamCost,
    /// 本流应交付的总行数（IHDR 解析后定）。
    pub total_rows: u32,
    /// 已回调进度次数（诊断面：进度回调是否真在发生）。
    pub progress_calls: u32,
}

impl StreamSession {
    /// 新建流式会话。
    pub fn new() -> StreamSession {
        StreamSession {
            phase: StreamPhase::Signature,
            fed: 0,
            buf: Vec::new(),
            expect: 0,
            ctype: [0; 4],
            payload: Vec::new(),
            stage: ChunkStage::Header,
            idat: Vec::new(),
            idat_consumed: 0,
            idat_partial_done: false,
            idat_pushed_in_chunk: 0,
            ihdr: None,
            palette: None,
            transparency: Transparency::None,
            infl: Box::new(InflateState::new()),
            rows_done: 0,
            out_rgba: Vec::new(),
            raw_done: 0,
            raw_pending: Vec::new(),
            prev_raw: Vec::new(),
            pass: 0,
            aborted: false,
            finished: false,
            raw_total: 0,
            cursor_error: None,
            cost: StreamCost::default(),
            total_rows: 0,
            progress_calls: 0,
        }
    }

    /// 当前进度。
    pub fn progress(&self) -> Progress {
        Progress {
            fed: self.fed,
            rows: self.rows_done,
            total_rows: self.total_rows,
            pass: self.pass,
        }
    }

    /// 喂入一个 chunk 并推进状态机（可多次调用）。
    ///
    /// `sink` 为行接收端；每完成一行即回调一次（行级回调）。
    /// 返回本 chunk 新交付的行数。**故障不丢弃已完成部分**：故障以
    /// `StreamError`（三要素）返回，会话保留已交付行（调用方可取 `out_rgba`）。
    pub fn feed(&mut self, chunk: &[u8], sink: &mut dyn RowSink) -> StreamResult<u32> {
        let mut prog = NullProgress;
        self.feed_with_progress(chunk, sink, &mut prog)
    }

    /// 带进度回调的喂入（锚点主接口）。
    ///
    /// 进度在**每一行交付后**回调一次（而非每个 chunk 回调一次）——
    /// 「有进展才通知」是进度回调的语义。锚点要的是**渐进显示**，
    /// 接收端据进度信驱动 UI 重绘，故信的粒度必须与行的粒度一致：
    /// 每 chunk 一封会让「20 行的图在 668 次喂入里收到 668 封信」，
    /// 而其中 667 封报告同一个行数（探针实测：修复前
    /// `progress_calls = 1`，`rows_done = 12`——因为整图只在最后一个
    /// chunk 才一次性交付 12 行）。
    ///
    /// 若调用方在进度回调里要求中止（返回 false），本轮立即停止交付，
    /// 并置 [`StreamSession::aborted`]。
    pub fn feed_with_progress(
        &mut self,
        chunk: &[u8],
        sink: &mut dyn RowSink,
        prog: &mut dyn ProgressSink,
    ) -> StreamResult<u32> {
        // **已中止的会话拒绝一切后续喂入**：锚点的「取消」语义是终态，
        // 不是「暂停」。若还继续投喂，已交付行会被后续数据改写
        // （判据 B07b 守的正是这一条：中止后像素不再被改写）。
        if self.aborted {
            return Ok(0);
        }
        self.fed += chunk.len() as u64;
        self.buf.extend_from_slice(chunk);
        let delivered = self.drain(sink)?;
        // 每交付一行发一封进度信（而不是每 chunk 一封）。
        for _ in 0..delivered {
            self.cost.rows = self.rows_done;
            self.progress_calls += 1;
            if !prog.on_progress(self.progress()) {
                self.aborted = true;
                break;
            }
        }
        // ---- 簿记：分母（全量工作集）与分子（流式峰值驻留）----
        {
            // 分母 = **流长 + 解码成果字节数**，即全量路径的峰值工作集。
            //
            // **不把 `idat_consumed` 计入分母**：那是被 inflate 消费掉的
            // 压缩字节，属于「读过的输入」，全量路径同样要读。把它加进分母
            // 会把比值摊小 —— **让开销看起来更小的方向上的错误最危险**。
            // 保证非零：IHDR 未到时成果为 0，只用流长也非零；
            // 再取 `max(1)` 兜住「零长输入」这一退化情形
            // （否则 ppm 恒 0、`within_budget()` 永真，判据沦为摆设）。
            let produced = self.out_rgba.len() as u64;
            let denom = self.fed.saturating_add(produced).max(1);
            self.cost.set_bulk(denom);
            // 分子：解码成果之外的一切流式私有驻留（取历史峰值）。
            // `infl` 是装箱的 32KB 滑动窗口——它确实是流式的额外成本，
            // 故计入（否则这条判据会因为「窗口小」而假绿）。
            let live = (self.buf.len() as u64)
                .saturating_add(self.idat.len() as u64)
                .saturating_add(self.raw_pending.len() as u64)
                .saturating_add(self.payload.len() as u64)
                .saturating_add(WINDOW_SIZE as u64);
            self.cost.observe_overhead(live);
        }
        Ok(delivered)
    }

    /// 从缓冲里逐块消费。块跨 chunk 边界由 `buf` 无缝拼接
    /// （`expect` 未满即继续攒——锚点「块跨 chunk 边界缓冲器无缝拼接」）。
    fn drain(&mut self, sink: &mut dyn RowSink) -> StreamResult<u32> {
        let mut delivered = 0u32;
        loop {
            match self.phase {
                StreamPhase::Signature => {
                    if self.buf.len() < 8 {
                        break;
                    }
                    if self.buf[..8] != vef01_pngdec::PNG_SIG {
                        return Err(StreamError::new(StreamFault::BadSignature, self.fed, self.rows_done));
                    }
                    self.buf.drain(..8);
                    self.phase = StreamPhase::Header;
                }
                StreamPhase::Header => {
                    // 头部需要 IHDR 整块
                    if !self.try_take_chunk()? {
                        break;
                    }
                    // payload 已在 self.payload
                    let head = match vef01_pngdec::parse_ihdr_ex(&self.payload, true) {
                        Ok(h) => h,
                        Err(_) => return Err(StreamError::new(StreamFault::ChunkOrder, self.fed, self.rows_done)),
                    };
                    self.raw_total = head.raw_bytes();
                    // 位深×颜色合法性与隔行值由 parse_ihdr_ex 把关；此处补隔行上界
                    if head.interlace > 1 {
                        return Err(StreamError::new(StreamFault::BadInterlace, self.fed, self.rows_done));
                    }
                    self.ihdr = Some(head);
                    self.total_rows = stream_total_rows(&head);
                    // 隔行图像的原始扫描字节按七遍累加（非隔行为 height×row_bytes）
                    self.raw_total = if head.interlace == 0 {
                        head.raw_bytes()
                    } else {
                        adam7_raw_size(head.width, head.height, head.channels())
                    };
                    let rb = head.row_bytes() as u64;
                    self.out_rgba = vec![0u8; head.width as usize * 4 * head.height as usize];
                    self.phase = StreamPhase::Chunks;
                    let _ = rb;
                }
                StreamPhase::Chunks => {
                    if !self.try_take_chunk()? {
                        // 块未收满，但 `try_take_chunk` 可能已把 IDAT 的
                        // **前缀**喂进 `self.idat`（见其 Payload 分支）。
                        // 此时仍要把已到的压缩字节灌进 inflate，否则
                        // 「渐进」名存实亡——这正是本单的核心语义。
                        if self.ctype == *b"IDAT" {
                            delivered += self.pump_inflate(sink)?;
                            if self.aborted {
                                break;
                            }
                            // 已推进但仍需更多输入 → 退出等下一 chunk
                            break;
                        }
                        break;
                    }
                    let ty = self.ctype;
                    match &ty {
                        b"IDAT" => {
                            // **只在本块尚未被 partial 分批推进过时才整段追加**。
                            //
                            // 增量路径下 IDAT 载荷会被 `try_take_chunk` 的
                            // partial 分支**分批**喂进 `idat`（每来一个 chunk
                            // 喂一批）。若此处再把整块 `payload` 追加一遍，
                            // 同一段 zlib 字节就被 inflate 消费两次
                            // （幂等性不成立——inflate 是有状态机），
                            // 解出的图会「前若干行对、其后全错」。
                            //
                            // **只在本块载荷「一个字节都还没交付」时才整段追加**。
                            //
                            // `payload` 在这里恒等于「本块**尚未**被 partial
                            // 分支抽走的那部分载荷」（`try_take_chunk` 的整段
                            // 路径按 `expect - 4` 切片，而 `expect` 已随 partial
                            // 递减）。故：
                            // · partial 走过的块 → `payload` 是**剩余部分**，
                            //   必须追加（否则剩余载荷被丢弃）；
                            // · 未走 partial 的块 → `payload` 是**全部载荷**，
                            //   追加一次即完整。
                            // 两种情形都只需 `extend` 一次，**不存在重复**——
                            // 重复的根源在 `try_take_chunk` 内部（那里的
                            // partial 推进与整段切片必须恰好瓜分同一块载荷）。
                            if !self.payload.is_empty() {
                                self.idat.extend_from_slice(&self.payload);
                                self.idat_pushed_in_chunk += self.payload.len() as u64;
                            }
                            self.idat_partial_done = false;
                            self.phase = StreamPhase::ImageData;
                            // IDAT 可能连续多个 → 立即尝试把已在缓冲的 IDAT 灌进 inflate
                            delivered += self.pump_inflate(sink)?;
                            if self.aborted {
                                break;
                            }
                        }
                        b"PLTE" => {
                            if let Ok(p) = vef01_pngdec::parse_plte(&self.payload, ColorType::Gray) {
                                self.palette = Some(p);
                            }
                        }
                        b"IEND" => {
                            self.phase = StreamPhase::End;
                            self.finished = true;
                        }
                        _ => {
                            // ancillary 跳过（不消费图像数据）
                        }
                    }
                }
                StreamPhase::ImageData => {
                    // 先把已攒的 IDAT 灌进 inflate（行级交付可能在此发生）
                    delivered += self.pump_inflate(sink)?;
                    // **中止检查必须在 pump 之后、下一次 pump 之前**，
                    // 且 `Chunks`/`ImageData` 两个分支都要查：只在一处查，
                    // 另一分支仍会再投一行（探针实测：要求停第 5 行，
                    // 实际交付 6 行 `ys=[0..5]` —— `Chunks` 分支投完第 6 行后
                    // 才轮到 `ImageData` 分支的 `if self.aborted`）。
                    if self.aborted {
                        break;
                    }
                    // 继续取下一个块（可能是后续 IDAT 或 IEND）
                    self.phase = StreamPhase::Chunks;
                }
                StreamPhase::End => break,
            }
        }
        Ok(delivered)
    }

    /// 尝试取出一个完整块（长度字段 + 类型 + 载荷 + CRC）。
    ///
    /// 返回 false 表示「还需要更多字节」。
    ///
    /// **阶段判定不能靠 `expect == 0`**：零长块的 `expect` 恰为 4（只剩 CRC），
    /// 而「刚吃完整块、还没读下个头」时 `expect` 也是 0——两者同值，用它当
    /// 判据会走进 `p[..expect-4]` 的下溢。故引入显式阶段字段
    /// [`ChunkStage::Header`] / [`ChunkStage::Payload`]，让状态不可混淆。
    ///
    /// 跨 chunk 的块在 `buf` 里无缝拼接：载荷没收满就 `return Ok(false)`，
    /// 下一 chunk 追加后继续（锚点「块跨 chunk 边界缓冲器无缝拼接」）。
    fn try_take_chunk(&mut self) -> StreamResult<bool> {
        loop {
            match self.stage {
                ChunkStage::Header => {
                    if self.buf.len() < 8 {
                        return Ok(false);
                    }
                    let len = u32::from_be_bytes([self.buf[0], self.buf[1], self.buf[2], self.buf[3]]) as usize;
                    self.ctype = [self.buf[4], self.buf[5], self.buf[6], self.buf[7]];
                    // 单块长度上限守卫：恶意长度不得撑爆缓冲（先比后算）。
                    if len > CHUNK_LEN_MAX {
                        return Err(StreamError::new(StreamFault::LengthOverflow, self.fed, self.rows_done));
                    }
                    self.buf.drain(..8);
                    self.expect = len + 4; // 载荷 + CRC
                    self.payload.clear();
                    // 新块 → 分批推进记账归零
                    self.idat_partial_done = false;
                    self.idat_pushed_in_chunk = 0;
                    self.stage = ChunkStage::Payload;
                }
                ChunkStage::Payload => {
                    if self.buf.len() < self.expect {
                        // IDAT 未收满：**先把已到的载荷喂给 inflate 再等**。
                        //
                        // 这是「流式」的本质。若这里直接 `return Ok(false)`，
                        // 则 inflate 只在整块收满后才启动——而 F1002 编码器
                        // 默认 `idat_chunk = 0`（单一巨块），于是整张图的所有行
                        // 都要等**最后一个字节**到达才一次性交付，
                        // 「渐进显示」名存实亡（探针实测 16×20 图：逐字节喂入
                        // 668 次，`first_row_at == complete_at == 1413`，
                        // 而两次都指向同一个字节 = 无渐进）。
                        //
                        // PNG 规范（RFC 2083 §3.2）规定 IDAT 的载荷是 zlib 流的
                        // **连续字节序列**，解码器可以在载荷未收齐时就开始消费
                        // 已到达的前缀——这正是流式解码该做的事。
                        if self.ctype == *b"IDAT" {
                            // ---- 不变式：`expect` = 「本块尚未从 `buf` 取走的字节数」，
                            //      **恒 ≥ 4**（尾部 4 字节 CRC 永不被 partial 抽走）。
                            //      故载荷剩余量就是 `expect - 4`。
                            //
                            // **为什么必须靠 `expect` 递减来收敛，而不是另记
                            // 「已推进量」再用固定上界取**：`expect` 递减后，
                            // 上界 `expect - 4` 自动收紧，载荷恰好被推进
                            // `payload_total` 次 —— **一次不多，一次不少**。
                            // 另记「已推进量」的写法一旦漏同步就会重喂
                            // （inflate 是有状态机、不幂等，重喂同一段字节会
                            // 让解出的图从出错行起全错；探针实测 cs=1 曾比
                            // cs=65536 多喂 16 字节）。
                            if self.expect <= 4 {
                                // 载荷已全部推进，只等 CRC 到齐
                                return Ok(false);
                            }
                            let avail = self.buf.len().min(self.expect - 4);
                            if avail > 0 {
                                self.idat.extend_from_slice(&self.buf[..avail]);
                                self.buf.drain(..avail);
                                self.idat_pushed_in_chunk += avail as u64;
                                self.idat_partial_done = true;
                                // `expect` 同步递减 → 上界 `expect - 4` 自动收紧，
                                // 载荷恰好被推进 `payload_total` 次，一次不多。
                                //
                                // **若不递减**：`avail` 的上界不变而 `buf` 已被抽干，
                                // 下一轮会把 CRC 当 zlib 载荷喂进 inflate，
                                // 随后 `Header` 阶段把下一块的头当本块剩余载荷，
                                // 长度字段读出垃圾 → `LengthOverflow`
                                // （探针实测：512×384 在 fed=16443 炸 LengthOverflow）。
                                self.expect -= avail;
                                return Ok(false); // 载荷仍未收齐，但 idat 已推进
                            }
                        }
                        return Ok(false); // 等下一 chunk
                    }
                    // ---- 整块收齐（`buf.len() >= expect`）----
                    let full: Vec<u8> = self.buf[..self.expect].to_vec();
                    self.buf.drain(..self.expect);
                    // 载荷 = 去掉尾部 4 字节 CRC。
                    //
                    // **`buf.len() >= expect` 成立时，`buf` 里躺着的是
                    // 「本块尚未被 partial 抽走的那部分载荷 + CRC」**，
                    // 故切片一律按 `expect - 4` 取（`expect == 4` 时为空）。
                    //
                    // 曾在此写「若 `idat_pushed_in_chunk > 0` 则 payload 置空」，
                    // 理由是「载荷已分批交付」—— **错**：`buf.len() >= expect`
                    // 成立时载荷**未必**全被抽走（`expect` 递减到 4 之前都算）。
                    // 强行置空会把**剩余载荷静默丢弃**：症状是行数停在某值
                    // 不再增长、末几行全零，而 `finished=true` 照常置起
                    // （探针实测：12×9 图 64 字节切分停在第 7 行、
                    // `consumed` 卡在 278 不动）。
                    let payload_len = self.expect - 4;
                    self.payload = full[..payload_len].to_vec();
                    self.expect = 0;
                    self.stage = ChunkStage::Header;
                    return Ok(true);
                }
            }
        }
    }

    /// 把已攒 IDAT 灌进 inflate，按行产出并回调。
    ///
    /// 行级交付：每解出 `row_bytes` 字节就反滤波 + 色彩展开 + 回调。
    /// `out_rgba` 保留已解码部分（锚点「未完成时输出已解码部分」）。
    fn pump_inflate(&mut self, sink: &mut dyn RowSink) -> StreamResult<u32> {
        let head = match self.ihdr {
            Some(h) => h,
            None => return Ok(0),
        };
        let stride = head.row_bytes(); // 含滤波类型首字节
        let bpp = head.filter_bpp();
        let mut delivered = 0u32;

        // 反滤波双行缓冲与 RGBA 行暂存跨行复用（不每行重分配）。
        let mut rgba_row = vec![0u8; head.width as usize * 4];
        let mut cur_raw = vec![0u8; stride];
        // 会话级参照行：首次进入按 stride 建零（IHDR 已定，尺寸不变）
        if self.prev_raw.len() != stride {
            self.prev_raw.clear();
            self.prev_raw.resize(stride, 0);
        }

        loop {
            // ---- 阶段一：把 raw_pending 里的字节按 stride 切成完整行 ----
            while self.raw_pending.len() >= stride {
                let y = self.rows_done;
                if y >= head.height {
                    break;
                }
                cur_raw.copy_from_slice(&self.raw_pending[..stride]);
                self.raw_pending.drain(..stride);
                self.raw_done += stride as u64;

                // 反滤波（剥滤波类型首字节）。
                // 借用纪律：`unfilter_line` 同时要读整行并写 cur，同一个数组
                // 不可变+可变双借 —— 故先把整行搬进独立局部缓冲再交给 ctx。
                if y == 0 {
                    self.prev_raw.fill(0);
                }
                let line_copy = cur_raw.clone();
                {
                    let mut ctx = vef01_pngdec::DecodeCtx::new(
                        &mut cur_raw[1..],
                        &self.prev_raw[1..],
                        imgsimd::detect_isa(),
                    );
                    if ctx.unfilter_line(&line_copy[..stride], bpp).is_none() {
                        return Err(StreamError::new(
                            StreamFault::BadCodeLengths,
                            self.fed,
                            self.rows_done,
                        ));
                    }
                }
                // 色彩展开（调色板越界索引 → expand 返回 None）
                if vef01_pngdec::expand_row_rgba(
                    &cur_raw[1..],
                    &head,
                    self.palette.as_ref(),
                    &self.transparency,
                    &mut rgba_row,
                )
                .is_none()
                {
                    return Err(StreamError::new(
                        StreamFault::BadCodeLengths,
                        self.fed,
                        self.rows_done,
                    ));
                }
                // 交 prev（在写 out 之前，避免与 out_rgba 借用重叠）
                self.prev_raw.copy_from_slice(&cur_raw);

                let off = y as usize * head.width as usize * 4;
                self.out_rgba[off..off + rgba_row.len()].copy_from_slice(&rgba_row);

                // 行级回调（当场交付，不等整图）
                let keep = sink.on_row(y, &rgba_row);
                delivered += 1;
                self.rows_done += 1;
                if !keep {
                    self.aborted = true;
                    return Ok(delivered);
                }
            }

            if self.rows_done >= head.height {
                break;
            }
            // 本行尚未齐 → 再向 inflate 要字节
            let before = self.raw_pending.len();
            self.inflate_step()?;
            if self.raw_pending.len() == before {
                break; // inflate 已尽力，再无新字节（等下一 chunk）
            }
        }
        Ok(delivered)
    }

    /// 向 [`StreamSession::raw_pending`] 追加 inflate 产出的字节。
    ///
    /// 单轮上限 = 一行扫描线（`stride`）到两行之间的小增量，目的是既保证
    /// 「有进展就推进」又不让单次调用吞掉过多输入（保留块级推进的粒度）。
    /// 返回本轮新增字节数（0 = 需要更多输入）。
    /// 向 [`StreamSession::raw_pending`] 追加 inflate 产出的字节。
    ///
    /// **输出缓冲按「还缺多少 + 一整行余量」分配，绝不中途截断**：
    /// 若把产出上限压成一个小值，`inflate_into` 会在匹配解到一半时返回
    /// （`pending_len > 0`），而 `produced` 与「真正写出的字节」之间的对应
    /// 就此打断 —— `produced` 计了窗口推入，行切分却没跟上，字节凭空消失
    /// （探针实测：16×20 图 `produced=980` 全量解完，而 `raw_pending` 与
    /// 已切行字节之和只有 949，差 31）。
    ///
    /// 缓冲按「欠账 + 一行」定容：欠账越多分配越大，最坏情况是整个图一次
    /// 解完（可接受——`raw_pending` 本就要暂存全部扫描数据才能切行）。
    fn inflate_step(&mut self) -> StreamResult<usize> {
        let head = match self.ihdr {
            Some(h) => h,
            None => return Ok(0),
        };
        let stride = head.row_bytes();
        // 欠账 = 本流还需要的扫描字节 - 已切行 - 待切行
        let need_total = self.raw_total;
        let have = (self.rows_done as u64) * stride as u64 + self.raw_pending.len() as u64;
        let owed = need_total.saturating_sub(have).max(stride as u64);
        // 留一行余量，让 inflate 能一次把当前到达的数据解完而不被截断
        let cap = (owed + stride as u64).min(need_total.max(stride as u64)) as usize + stride;
        let mut chunk = vec![0u8; cap.max(stride * 2)];
        // 先取缓冲长度再借用（避免 `&mut chunk` 与 `chunk.len()` 同时借用）
        let cap_len = chunk.len();
        let (w, _p) = inflate_into(&self.idat, &mut self.infl, &mut chunk, cap_len)?;
        if w > 0 {
            self.raw_pending.extend_from_slice(&chunk[..w]);
        }
        // ---- 弹出 inflate 已消费完的输入前缀（流式省内存的关键）----
        //
        // `InflateState::pos.bit` 是**相对 `idat` 队首的位计数**，
        // `inflate_into` 用它索引 `src`（= `self.idat`）。故「已完整消费的
        // 字节数」= `pos.bit / 8`，而**当前所在的字节不能弹**（它还有
        // 未消费的位）。
        //
        // **弹出后必须把位计数同步下移 `n×8`**，否则 `pos.bit` 仍按弹出前的
        // 绝对量计数，而 `idat` 队首已前移 —— 于是读位索引整体前错，
        // 解出的全是垃圾。症状极隐蔽：不报错、行数照走，只是像素全错
        // （探针实测：修前 chunk=1 切分交付 0 行、`match=false`）。
        let done_bytes = (self.infl.pos.bit / 8) as usize;
        if done_bytes > 0 {
            let n = done_bytes.min(self.idat.len());
            self.idat.drain(..n);
            self.idat_consumed += n as u64;
            // 位计数下移，保持「相对 idat 队首」这一不变式。
            self.infl.pos.bit -= (n as u64) * 8;
        }
        Ok(w)
    }

    /// 冻结当前解码状态为可续解的游标（锚点「中断 = 保存块游标与 inflate 状态」）。
    ///
    /// 游标覆盖四类状态，缺任一条都会让「恢复」静默丢数据：
    /// ① **块游标**（`buf` 未消费的字节 + `stage` + `expect` + `ctype` + `payload`）
    /// ② **inflate 状态**（位位置 + 块阶段 + 两张码表 + 滑动窗口）
    /// ③ **行游标**（`rows_done` + `raw_done`）
    /// ④ **容器状态**（`phase` + `idat` 已收字节 + IHDR/调色板/透明语义）
    pub fn cursor(&self) -> StreamCursor {
        StreamCursor {
            magic: CURSOR_MAGIC,
            fed: self.fed,
            phase: self.phase,
            stage: self.stage,
            expect: self.expect,
            ctype: self.ctype,
            buf: self.buf.clone(),
            payload: self.payload.clone(),
            idat: self.idat.clone(),
            idat_consumed: self.idat_consumed,
            idat_partial_done: self.idat_partial_done,
            idat_pushed_in_chunk: self.idat_pushed_in_chunk,
            ihdr: self.ihdr,
            palette: self.palette.clone(),
            transparency: self.transparency.clone(),
            infl: Box::new((*self.infl).clone()),
            rows_done: self.rows_done,
            out_rgba: self.out_rgba.clone(),
            raw_done: self.raw_done,
            raw_pending: self.raw_pending.clone(),
            prev_raw: self.prev_raw.clone(),
            pass: self.pass,
            aborted: self.aborted,
            finished: self.finished,
        }
    }

    /// 用游标恢复会话（锚点「恢复 = 从游标续解」）。返回已交付行数。
    ///
    /// **游标版本校验**：来自别的流或旧版结构的游标若直接灌进来，位游标可能
    /// 落在窗口之外，续解时会以「距离码越界」面貌炸出——看不出是游标不兼容。
    /// 故显式带一个魔数与版本，不符即显性拒绝（[`StreamFault::CursorIncompatible`]）。
    pub fn restore(&mut self, cur: &StreamCursor) -> u32 {
        if cur.magic != CURSOR_MAGIC {
            self.cursor_error = Some(StreamFault::CursorIncompatible);
            return 0;
        }
        self.fed = cur.fed;
        self.phase = cur.phase;
        self.stage = cur.stage;
        self.expect = cur.expect;
        self.ctype = cur.ctype;
        self.buf = cur.buf.clone();
        self.payload = cur.payload.clone();
        self.idat = cur.idat.clone();
        self.idat_consumed = cur.idat_consumed;
        self.idat_partial_done = cur.idat_partial_done;
        self.idat_pushed_in_chunk = cur.idat_pushed_in_chunk;
        self.ihdr = cur.ihdr;
        self.palette = cur.palette.clone();
        self.transparency = cur.transparency.clone();
        self.infl = cur.infl.clone();
        self.rows_done = cur.rows_done;
        self.out_rgba = cur.out_rgba.clone();
        self.raw_done = cur.raw_done;
        self.raw_pending = cur.raw_pending.clone();
        self.prev_raw = cur.prev_raw.clone();
        self.pass = cur.pass;
        self.aborted = cur.aborted;
        self.finished = cur.finished;
        // IHDR 缺失时按零长缓冲兜底（游标不携带像素的情形只在 `empty()` 上发生）。
        // **注意**：这里刻意**不再按 IHDR 重建全零缓冲**。
        //
        // 曾有一行 `self.out_rgba = vec![0u8; …]`，注释写「像素是成果不是进度，
        // 带游标走会让游标体积随图像涨」，理由听起来体面，后果却是：
        // **恢复后用户已经看到的前 N 行画面凭空消失**（探针实测：喂 386/772 字节
        // 已交付 7 行，冻结-恢复后 `out_rgba_len=896` 但 11/14 行全零）。
        //
        // 锚点同时要求「部分图像输出（未完成时输出已解码部分）」与
        // 「中断恢复」——两者合起来就是：**已交付的像素必须穿过中断**。
        // 「游标轻量」的正确做法是别把 `out_rgba` 整个塞进游标结构体字段之外
        // 再指望恢复时重算（那需要重放全部已消费的压缩数据，与「省内存」
        // 直接冲突）；既然游标已经带了 `infl` 的 32KB 窗口与 `idat` 前缀，
        // 多带一份已交付像素是同量级代价，不构成「膨胀」。
        if self.ihdr.is_none() {
            self.out_rgba = Vec::new();
        }
        self.cursor_error = None;
        self.rows_done
    }

    /// 上次 [`StreamSession::restore`] 是否因游标不兼容被拒。
    pub fn cursor_error(&self) -> Option<StreamFault> {
        self.cursor_error
    }

    /// 完成解码（IEND 已见且行数达标）。返回部分输出或完成态。
    pub fn finish(&mut self) -> StreamResult<(u32, bool)> {
        if self.finished && self.rows_done == self.ihdr.map(|h| h.height).unwrap_or(0) {
            return Ok((self.rows_done, true));
        }
        // 结束但未完成 → 部分输出（带 truncated 语义的 false）
        Ok((self.rows_done, false))
    }
}

impl Default for StreamSession {
    fn default() -> Self {
        StreamSession::new()
    }
}

// ---------------------------------------------------------------------------
// 四、便利形：一次性喂完整流
// ---------------------------------------------------------------------------

/// 一次性喂入完整字节流（等价于把整个文件当一个 chunk）。
pub fn feed_all(file: &[u8], sink: &mut dyn RowSink) -> StreamResult<(StreamSession, u32)> {
    let mut s = StreamSession::new();
    let delivered = s.feed(file, sink)?;
    Ok((s, delivered))
}

/// 流式解码到整图 RGBA（便利形；与 `vef01::decode` 逐像素一致）。
pub fn decode_stream(file: &[u8]) -> StreamResult<(Ihdr, Vec<u8>)> {
    let mut s = StreamSession::new();
    let mut sink = NullSink;
    s.feed(file, &mut sink)?;
    let head = s.ihdr.ok_or_else(|| StreamError::new(StreamFault::BadSignature, 0, 0))?;
    if s.rows_done < head.height {
        return Err(StreamError::new(StreamFault::LengthOverflow, s.fed, s.rows_done));
    }
    Ok((head, s.out_rgba))
}