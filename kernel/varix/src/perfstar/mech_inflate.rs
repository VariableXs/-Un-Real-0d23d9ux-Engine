//! mech_inflate — DEFLATE 解码器（RFC1951 全三型块）+ zlib 封装 + PNG 装配
//! （AI-K1 深化批次四 · F054 图像解码通道 · F041 账本落盘解码端）。
//!
//! 主册依据：
//! - F054 判据「4K PNG 解码 ≤150ms……SIMD 与标量结果逐像素一致（**正确性
//!   优先于速度**）」——4K PNG 的容器面（IHDR/PLTE/IDAT/IEND、CRC、zlib
//!   封装、Adam7）此前不存在；批次三只落了行过滤五型与 IDCT。没有容器
//!   解析，「PNG 解码」只完成了像素算术那一半。本件补齐另一半。
//! - F041 账本落盘通道的解码端：mech_deflate 编码 → 本件解码，同一对
//!   RFC1951 语义两个消费域共用；解码器独立手写与编码器互为对拍——
//!   一处实现自证不是证据。
//!
//! 诚实边界（一处一事实）：
//! - 位深仅支持 8（主册 4K 资产口径；16 位随闸门按需补）。
//! - 不支持 tRNS 色键（调色板透明随闸门）；调色板展开 A 通道恒 255。
//! - IDAT 分片上限 [`IDAT_MAX`] 片——超出如实报错，不静默拼弃。
//! - 行反滤波**复用** `imgsimd_ext::row_filter`（五型一份实现，零冗余）；
//!   CRC 与 `frameledger_ext::crc32` 同多项式，本件给流式增量形并在单测
//!   与其 one-shot 形对拍（同一数学两种签名，不是两份真相）。
//! - 零堆：解码输出进调用侧缓冲；错误一律显式枚举（不 panic 不截断）。
//! - CheckSet 侧只用 stored 块构造容器判例（编码器哈希链表栈占 ~160KB，
//!   不进内核启动检查栈）；编码器↔解码器全链对拍在宿主单测（3897 套件
//!   同样在册），口径见本件与 mech_deflate 的 tests。

use crate::checks::CheckSet;
use crate::perfstar::imgsimd_ext::{row_filter, Filter};
use crate::perfstar::mech_deflate::{DIST_BASE, DIST_EXTRA, LEN_BASE, LEN_EXTRA};
// CheckSet 判例组装需要块缓冲：alloc 在内核 lib.rs 无条件 extern（宿主
// std 构建与 kernel-image 构建均可解析）；判定数据流本身仍零堆（定长数组）。
use alloc::vec::Vec;

/// LZ77 距离上限（与编码器同源口径）。
pub const WSIZE: usize = 32768;
/// IDAT 分片上限（PNG 规范允许任意分片；超限报 `TooManyIdat`——不静默拼弃）。
pub const IDAT_MAX: usize = 16;
/// Huffman 码长上限（RFC1951：码长 1..15）。
pub const MAX_BITS: usize = 15;

// ---------------------------------------------------------------------------
// 1. 错误面（三要素的「发生了什么」——枚举即文案索引，零裸异常码）
// ---------------------------------------------------------------------------

/// DEFLATE 流错误。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InflateErr {
    /// 流提前结束。
    Truncated,
    /// Huffman 解码落入无码区（流已坏）。
    BadCode,
    /// 距离引用了尚未产出的输出（流已坏）。
    BadDist,
    /// 长度码越域（流已坏）。
    BadLen,
    /// 动态表码长超订阅（RFC1951 前缀约束被破坏）。
    InvalidLengths,
    /// 输出缓冲不足（诚实拒绝，不半写后谎报成功）。
    Overflow,
}

/// PNG 容器错误。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PngError {
    NotPng,
    Truncated,
    BadCrc { got: u32, want: u32 },
    BadHeader,
    Unsupported,
    ChunkOrder,
    TooManyIdat,
    NoIdat,
    BadZlib,
    BadAdler { got: u32, want: u32 },
    BadFilter,
    BadPalette,
    ScratchSize { need: usize, got: usize },
    Inflate(InflateErr),
}

impl From<InflateErr> for PngError {
    fn from(e: InflateErr) -> Self {
        PngError::Inflate(e)
    }
}

// ---------------------------------------------------------------------------
// 2. 位读取器（LSB-first，跨 IDAT 分片连续）
// ---------------------------------------------------------------------------

/// 跨多片的位读取器：PNG 的 IDAT 可任意分片，zlib 流必须跨片连续读——
/// 分片边界对位流透明。单缓冲时传一片即可。
pub struct BitReader<'a> {
    slices: [&'a [u8]; IDAT_MAX],
    n: usize,
    si: usize,
    bi: usize,
    bb: u32,
    bits_consumed: u64,
}

impl<'a> BitReader<'a> {
    pub fn new(slices: &[&'a [u8]]) -> Self {
        let mut s: [&[u8]; IDAT_MAX] = [&[]; IDAT_MAX];
        let n = s.len().min(slices.len());
        s[..n].copy_from_slice(&slices[..n]);
        BitReader { slices: s, n, si: 0, bi: 0, bb: 0, bits_consumed: 0 }
    }

    #[inline]
    fn read_bit(&mut self) -> Result<u32, InflateErr> {
        loop {
            if self.si >= self.n {
                return Err(InflateErr::Truncated);
            }
            let s = self.slices[self.si];
            if self.bi >= s.len() {
                self.si += 1;
                self.bi = 0;
                self.bb = 0;
                continue;
            }
            let b = (s[self.bi] >> self.bb) & 1;
            self.bits_consumed += 1;
            self.bb += 1;
            if self.bb == 8 {
                self.bb = 0;
                self.bi += 1;
            }
            return Ok(b as u32);
        }
    }

    /// 读 n 位（n ≤ 16），LSB-first 组装（RFC1951 整数附加位序）。
    #[inline]
    fn read_bits(&mut self, n: u32) -> Result<u32, InflateErr> {
        let mut v = 0u32;
        for i in 0..n {
            v |= self.read_bit()? << i;
        }
        Ok(v)
    }

    /// 对齐到字节边界（stored 块头前）。
    fn align_byte(&mut self) {
        if self.bb != 0 {
            self.bb = 0;
            self.bi += 1;
        }
    }

    /// 位流内已消费位数（诊断用）。
    pub fn consumed_bits(&self) -> u64 {
        self.bits_consumed
    }
}

// ---------------------------------------------------------------------------
// 3. Huffman（规范码构造 + 逐位解码，puff 算法整数化）
// ---------------------------------------------------------------------------

/// 规范 Huffman 解码表：按 (码长, 符号值) 排序的符号表 + 每长度计数。
pub struct Huff {
    counts: [u16; MAX_BITS + 1],
    symbols: [u16; 288],
    n: usize,
}

impl Huff {
    /// 由码长表构造。全部零 = 空表（合法：未被引用的距离表）；
    /// 超订阅（前缀约束破坏）= Err。
    pub fn build(lengths: &[u8]) -> Result<Huff, InflateErr> {
        let mut counts = [0u16; MAX_BITS + 1];
        for &l in lengths {
            if l as usize > MAX_BITS {
                return Err(InflateErr::InvalidLengths);
            }
            counts[l as usize] += 1;
        }
        counts[0] = 0;
        // 超订阅检查：每层剩余码空间必须非负（puff left 语义）。
        let mut left: i32 = 1;
        for l in 1..=MAX_BITS {
            left <<= 1;
            left -= counts[l] as i32;
            if left < 0 {
                return Err(InflateErr::InvalidLengths);
            }
        }
        // 计数排序：偏移表 → 符号按 (码长, 值) 升序落位。
        let mut offs = [0u16; MAX_BITS + 2];
        for l in 1..=MAX_BITS {
            offs[l + 1] = offs[l] + counts[l];
        }
        let mut symbols = [0u16; 288];
        let mut n = 0usize;
        for (sym, &l) in lengths.iter().enumerate() {
            if l != 0 && sym < 288 {
                symbols[offs[l as usize] as usize] = sym as u16;
                offs[l as usize] += 1;
                n += 1;
            }
        }
        Ok(Huff { counts, symbols, n })
    }

    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// 解码一个符号（逐位下行，规范码语义）。
    #[inline]
    pub fn decode(&self, br: &mut BitReader) -> Result<u16, InflateErr> {
        if self.n == 0 {
            return Err(InflateErr::BadCode);
        }
        let mut code: i32 = 0;
        let mut first: i32 = 0;
        let mut index: usize = 0;
        for len in 1..=MAX_BITS {
            code |= br.read_bit()? as i32;
            let count = self.counts[len] as i32;
            if code - first < count {
                return Ok(self.symbols[index + (code - first) as usize]);
            }
            index += count as usize;
            first = (first + count) << 1;
            code <<= 1;
        }
        Err(InflateErr::BadCode)
    }
}

// ---------------------------------------------------------------------------
// 4. DEFLATE 主循环
// ---------------------------------------------------------------------------

/// 长度/距离符号的 base+extra 展开（表与 mech_deflate 同源——一处一事实）。
#[inline]
fn read_len_sym(br: &mut BitReader, sym: u16) -> Result<usize, InflateErr> {
    if !(257..=285).contains(&sym) {
        return Err(InflateErr::BadLen);
    }
    let i = (sym - 257) as usize;
    let extra = LEN_EXTRA[i] as u32;
    let base = LEN_BASE[i] as usize;
    Ok(base + br.read_bits(extra)? as usize)
}

#[inline]
fn read_dist_sym(br: &mut BitReader, sym: u16) -> Result<usize, InflateErr> {
    if sym as usize >= DIST_BASE.len() {
        return Err(InflateErr::BadDist);
    }
    let i = sym as usize;
    let extra = DIST_EXTRA[i] as u32;
    let base = DIST_BASE[i] as usize;
    Ok(base + br.read_bits(extra)? as usize)
}

/// 解码一个 Huffman 块的符号流（固定/动态表共用；fixed=true 时用 RFC1951
/// §3.2.6 固定表，否则用调用侧构造的表）。
fn inflate_syms(
    br: &mut BitReader,
    out: &mut [u8],
    out_pos: &mut usize,
    lit: &Huff,
    dist: &Huff,
) -> Result<(), InflateErr> {
    loop {
        let sym = lit.decode(br)?;
        match sym {
            0..=255 => {
                if *out_pos >= out.len() {
                    return Err(InflateErr::Overflow);
                }
                out[*out_pos] = sym as u8;
                *out_pos += 1;
            }
            256 => return Ok(()),
            _ => {
                let len = read_len_sym(br, sym)?;
                let dsym = dist.decode(br)?;
                let d = read_dist_sym(br, dsym)?;
                if d > *out_pos {
                    return Err(InflateErr::BadDist);
                }
                if *out_pos + len > out.len() {
                    return Err(InflateErr::Overflow);
                }
                // 逐字节复制：LZ77 的 RLE 段常态重叠（dist < len），不可 memcpy。
                let mut src = *out_pos - d;
                let end = *out_pos + len;
                while *out_pos < end {
                    out[*out_pos] = out[src];
                    src += 1;
                    *out_pos += 1;
                }
            }
        }
    }
}

/// 固定 Huffman 表（RFC1951 §3.2.6 字面量长度表：0-143→8 位，144-255→9 位，
/// 256-279→7 位，280-287→8 位；距离 30 码 × 5 位）。
fn fixed_tables() -> (Huff, Huff) {
    let mut ll = [0u8; 288];
    for (i, l) in ll.iter_mut().enumerate() {
        *l = match i {
            0..=143 => 8,
            144..=255 => 9,
            256..=279 => 7,
            _ => 8,
        };
    }
    let dl = [5u8; 30];
    (Huff::build(&ll).unwrap(), Huff::build(&dl).unwrap())
}

/// 码长字母表的规范顺序（RFC1951 §3.2.7）。
const CLC_ORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];

/// 解码一个块（返回是否末块）。
fn inflate_block(br: &mut BitReader, out: &mut [u8], out_pos: &mut usize) -> Result<bool, InflateErr> {
    let bfinal = br.read_bit()?;
    let btype = br.read_bits(2)?;
    match btype {
        0 => {
            // stored：对齐 → LEN/NLEN 互补校验 → 原样拷贝。
            br.align_byte();
            let len = br.read_bits(8)? | (br.read_bits(8)? << 8);
            let nlen = br.read_bits(8)? | (br.read_bits(8)? << 8);
            if len != (!nlen & 0xFFFF) {
                return Err(InflateErr::InvalidLengths);
            }
            for _ in 0..len {
                if *out_pos >= out.len() {
                    return Err(InflateErr::Overflow);
                }
                out[*out_pos] = br.read_bits(8)? as u8;
                *out_pos += 1;
            }
        }
        1 => {
            let (lit, dist) = fixed_tables();
            inflate_syms(br, out, out_pos, &lit, &dist)?;
        }
        2 => {
            let hlit = br.read_bits(5)? as usize + 257;
            let hdist = br.read_bits(5)? as usize + 1;
            let hclen = br.read_bits(4)? as usize + 4;
            if hlit > 286 || hdist > 30 {
                return Err(InflateErr::InvalidLengths);
            }
            let mut cl_lens = [0u8; 19];
            for i in 0..hclen {
                cl_lens[CLC_ORDER[i]] = br.read_bits(3)? as u8;
            }
            let cl = Huff::build(&cl_lens)?;
            if cl.is_empty() {
                return Err(InflateErr::InvalidLengths);
            }
            // 码长序列（字面量 + 距离两段连着解码），带 16/17/18 重复码。
            let total = hlit + hdist;
            let mut lens = [0u8; 286 + 30];
            let mut i = 0usize;
            while i < total {
                let sym = cl.decode(br)?;
                match sym {
                    0..=15 => {
                        lens[i] = sym as u8;
                        i += 1;
                    }
                    16 => {
                        if i == 0 {
                            return Err(InflateErr::InvalidLengths);
                        }
                        let prev = lens[i - 1];
                        let rep = 3 + br.read_bits(2)? as usize;
                        if i + rep > total {
                            return Err(InflateErr::InvalidLengths);
                        }
                        for _ in 0..rep {
                            lens[i] = prev;
                            i += 1;
                        }
                    }
                    17 => {
                        let rep = 3 + br.read_bits(3)? as usize;
                        if i + rep > total {
                            return Err(InflateErr::InvalidLengths);
                        }
                        i += rep;
                    }
                    18 => {
                        let rep = 11 + br.read_bits(7)? as usize;
                        if i + rep > total {
                            return Err(InflateErr::InvalidLengths);
                        }
                        i += rep;
                    }
                    _ => return Err(InflateErr::InvalidLengths),
                }
            }
            let lit = Huff::build(&lens[..hlit])?;
            let dist = Huff::build(&lens[hlit..total])?;
            if lit.is_empty() {
                return Err(InflateErr::InvalidLengths);
            }
            inflate_syms(br, out, out_pos, &lit, &dist)?;
        }
        _ => return Err(InflateErr::BadCode), // BTYPE=11 保留
    }
    Ok(bfinal == 1)
}

/// 解压完整 DEFLATE 裸流进 `out`，返回产出字节数。
pub fn inflate(slices: &[&[u8]], out: &mut [u8]) -> Result<usize, InflateErr> {
    let mut br = BitReader::new(slices);
    let mut out_pos = 0usize;
    loop {
        if inflate_block(&mut br, out, &mut out_pos)? {
            break;
        }
    }
    Ok(out_pos)
}

/// 宿主测试/对拍便捷入口（进 Vec——cfg(test)，不进内核路径）。
#[cfg(test)]
pub fn inflate_vec(input: &[u8], cap: usize) -> Result<Vec<u8>, InflateErr> {
    let mut out = vec![0u8; cap];
    let n = inflate(&[input], &mut out)?;
    out.truncate(n);
    Ok(out)
}

// ---------------------------------------------------------------------------
// 5. zlib 封装（RFC1950：CMF/FLG 头 + Adler-32 尾）
// ---------------------------------------------------------------------------

/// Adler-32 一次性计算（RFC1950 §8：s1/s2 mod 65521；5552 为不溢出块长）。
pub fn adler32(data: &[u8]) -> u32 {
    adler32_feed(1, data)
}

/// Adler-32 增量形（流式校验用；`adler_in` 为上次原始状态，初始 1）。
pub fn adler32_feed(adler_in: u32, data: &[u8]) -> u32 {
    const MOD: u32 = 65521;
    let mut s1 = adler_in & 0xFFFF;
    let mut s2 = adler_in >> 16;
    for chunk in data.chunks(5552) {
        for &b in chunk {
            s1 += b as u32;
            s2 += s1;
        }
        s1 %= MOD;
        s2 %= MOD;
    }
    (s2 << 16) | s1
}

/// 取多片流的末 4 字节（zlib 尾部校验用；总长 <4 → None）。
fn tail4(slices: &[&[u8]]) -> Option<[u8; 4]> {
    let total: usize = slices.iter().map(|s| s.len()).sum();
    if total < 4 {
        return None;
    }
    let mut out = [0u8; 4];
    let mut need = 4usize;
    let mut fill = 4usize;
    for s in slices.iter().rev() {
        if need == 0 {
            break;
        }
        let take = s.len().min(need);
        out[fill - take..fill].copy_from_slice(&s[s.len() - take..]);
        fill -= take;
        need -= take;
    }
    Some(out)
}

/// zlib 全流解压：头校验 → 裸 DEFLATE → Adler-32 尾校验。
pub fn zlib_inflate_slices(slices: &[&[u8]], out: &mut [u8]) -> Result<usize, PngError> {
    let first = slices.first().copied().unwrap_or(&[]);
    if first.len() < 2 {
        return Err(PngError::BadZlib);
    }
    let (cmf, flg) = (first[0], first[1]);
    if cmf & 0x0F != 8 {
        return Err(PngError::BadZlib); // CM 只认 deflate
    }
    if ((cmf as u32) << 8 | flg as u32) % 31 != 0 {
        return Err(PngError::BadZlib); // FCHECK 校验
    }
    if flg & 0x20 != 0 {
        return Err(PngError::Unsupported); // FDICT 预设字典：PNG 禁用，诚实拒
    }
    // 剥离 2 字节 zlib 头（RFC1950 §2.2：CMF/FLG 不属于 DEFLATE 位流），
    // 余下片才是裸 DEFLATE——不剥会把 0x78 当块头解析成 stored 块。
    let mut rest: [&[u8]; IDAT_MAX] = [&[]; IDAT_MAX];
    let mut rn = 0usize;
    if first.len() > 2 {
        rest[0] = &first[2..];
        rn = 1;
    }
    for s in slices.iter().skip(1) {
        if rn >= IDAT_MAX {
            return Err(PngError::TooManyIdat);
        }
        rest[rn] = s;
        rn += 1;
    }
    let n = inflate(&rest[..rn], out).map_err(PngError::from)?;
    let want = u32::from_be_bytes(tail4(slices).ok_or(PngError::BadZlib)?);
    let got = adler32(&out[..n]);
    if got != want {
        return Err(PngError::BadAdler { got, want });
    }
    Ok(n)
}

// ---------------------------------------------------------------------------
// 6. CRC32 流式增量（与 frameledger_ext::crc32 同多项式——单测对拍）
// ---------------------------------------------------------------------------

/// CRC-32/IEEE 增量形：`state` 传入未反转中间态（初始 0xFFFF_FFFF），
/// 返回未反转中间态；终值取反由收口函数做。
pub fn crc32_feed(state: u32, bytes: &[u8]) -> u32 {
    let mut c = state;
    for &b in bytes {
        c ^= b as u32;
        for _ in 0..8 {
            let mask = (c & 1).wrapping_neg();
            c = (c >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    c
}

/// 两段 CRC（chunk 校验 = type ++ data，不拼接大缓冲）。
pub fn crc32_span(a: &[u8], b: &[u8]) -> u32 {
    !crc32_feed(crc32_feed(0xFFFF_FFFF, a), b)
}

// ---------------------------------------------------------------------------
// 7. PNG 容器：块遍历 / 头解析 / Adam7 / 装配
// ---------------------------------------------------------------------------

const PNG_SIG: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// PNG 块遍历器（每块 CRC 全验——CRC 错误不过夜）。
pub struct ChunkIter<'a> {
    file: &'a [u8],
    pos: usize,
}

impl<'a> ChunkIter<'a> {
    pub fn new(file: &'a [u8]) -> Self {
        let start = if file.len() >= 8 && file[..8] == PNG_SIG { 8 } else { 0 };
        ChunkIter { file, pos: start }
    }

    /// 下一块：(类型, 数据)。CRC 域覆盖 type+data；非法即 Err。
    pub fn next_chunk(&mut self) -> Result<Option<(&'a [u8], &'a [u8])>, PngError> {
        if self.pos >= self.file.len() {
            return Ok(None);
        }
        let f = self.file;
        if self.pos + 8 > f.len() {
            return Err(PngError::Truncated);
        }
        let len = u32::from_be_bytes([f[self.pos], f[self.pos + 1], f[self.pos + 2], f[self.pos + 3]]) as usize;
        let ty = &f[self.pos + 4..self.pos + 8];
        if self.pos + 12 + len > f.len() {
            return Err(PngError::Truncated);
        }
        let data = &f[self.pos + 8..self.pos + 8 + len];
        let want = u32::from_be_bytes([f[self.pos + 8 + len], f[self.pos + 9 + len], f[self.pos + 10 + len], f[self.pos + 11 + len]]);
        let got = crc32_span(ty, data);
        if got != want {
            return Err(PngError::BadCrc { got, want });
        }
        self.pos += 12 + len;
        Ok(Some((ty, data)))
    }
}

/// PNG 颜色类型（深度 8 通道数见 [`ColorType::channels`]）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorType {
    Gray,
    Rgb,
    Palette,
    GrayAlpha,
    Rgba,
}

impl ColorType {
    pub fn from_u8(v: u8) -> Option<ColorType> {
        Some(match v {
            0 => ColorType::Gray,
            2 => ColorType::Rgb,
            3 => ColorType::Palette,
            4 => ColorType::GrayAlpha,
            6 => ColorType::Rgba,
            _ => return None,
        })
    }
    pub fn channels(self) -> usize {
        match self {
            ColorType::Gray | ColorType::Palette => 1,
            ColorType::GrayAlpha => 2,
            ColorType::Rgb => 3,
            ColorType::Rgba => 4,
        }
    }
}

/// PNG 头（IHDR 解析结果）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PngHeader {
    pub width: u32,
    pub height: u32,
    pub depth: u8,
    pub color: ColorType,
    pub interlace: u8,
}

/// Adam7 起点与步长（PNG 规范 §8.2 原表——七遍一事实）。
pub const ADAM7_X0: [u32; 7] = [0, 4, 0, 2, 0, 1, 0];
pub const ADAM7_Y0: [u32; 7] = [0, 0, 4, 0, 2, 0, 1];
pub const ADAM7_DX: [u32; 7] = [8, 8, 4, 4, 2, 2, 1];
pub const ADAM7_DY: [u32; 7] = [8, 8, 8, 4, 4, 2, 2];

/// 某一遍的 (列数, 行数)。
pub fn adam7_pass_dim(w: u32, h: u32, pass: usize) -> (u32, u32) {
    debug_assert!(pass < 7);
    let cols = if w > ADAM7_X0[pass] { (w - ADAM7_X0[pass] + ADAM7_DX[pass] - 1) / ADAM7_DX[pass] } else { 0 };
    let rows = if h > ADAM7_Y0[pass] { (h - ADAM7_Y0[pass] + ADAM7_DY[pass] - 1) / ADAM7_DY[pass] } else { 0 };
    (cols, rows)
}

/// 原始（滤波后）扫描线总字节数——调用侧 scratch 的精确需求。
pub fn png_raw_size(w: u32, h: u32, ch: usize, interlace: u8) -> u64 {
    let mut total: u64 = 0;
    if interlace == 0 {
        total = (h as u64) * (1 + w as u64 * ch as u64);
    } else {
        for p in 0..7 {
            let (cols, rows) = adam7_pass_dim(w, h, p);
            total += rows as u64 * (1 + cols as u64 * ch as u64);
        }
    }
    total
}

/// PNG 解码器：头部 + 调色板 + IDAT 分片（零拷贝引用原文件）。
pub struct PngDecoder<'a> {
    pub header: PngHeader,
    palette: [u8; 768],
    palette_len: usize,
    idat: [&'a [u8]; IDAT_MAX],
    idat_n: usize,
}

impl<'a> PngDecoder<'a> {
    /// 解析容器（IHDR/PLTE/IDAT/IEND；未知块按规范跳过；每块 CRC 全验）。
    pub fn new(file: &'a [u8]) -> Result<PngDecoder<'a>, PngError> {
        if file.len() < 8 || file[..8] != PNG_SIG {
            return Err(PngError::NotPng);
        }
        let mut it = ChunkIter::new(file);
        // IHDR 必须是首块。
        let (ty, data) = it.next_chunk()?.ok_or(PngError::Truncated)?;
        if ty != b"IHDR" || data.len() != 13 {
            return Err(PngError::ChunkOrder);
        }
        let width = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
        let height = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
        let depth = data[8];
        let color = ColorType::from_u8(data[9]).ok_or(PngError::Unsupported)?;
        if width == 0 || height == 0 || width > 16384 || height > 16384 {
            return Err(PngError::BadHeader);
        }
        if depth != 8 {
            return Err(PngError::Unsupported); // 诚实边界：8 位/4K 资产口径
        }
        if data[10] != 0 || data[11] != 0 {
            return Err(PngError::Unsupported); // 压缩/滤波方法仅定义值 0
        }
        if data[12] > 1 {
            return Err(PngError::Unsupported);
        }
        let mut dec = PngDecoder {
            header: PngHeader { width, height, depth, color, interlace: data[12] },
            palette: [0; 768],
            palette_len: 0,
            idat: [&[]; IDAT_MAX],
            idat_n: 0,
        };
        let mut saw_iend = false;
        while let Some((ty, data)) = it.next_chunk()? {
            match ty {
                b"PLTE" => {
                    if data.len() % 3 != 0 || data.len() > 768 || dec.idat_n > 0 {
                        return Err(PngError::BadPalette);
                    }
                    dec.palette[..data.len()].copy_from_slice(data);
                    dec.palette_len = data.len();
                }
                b"IDAT" => {
                    if dec.idat_n == IDAT_MAX {
                        return Err(PngError::TooManyIdat);
                    }
                    dec.idat[dec.idat_n] = data;
                    dec.idat_n += 1;
                }
                b"IEND" => {
                    saw_iend = true;
                    break;
                }
                _ => {} // 未知/辅助块：规范允许跳过（不静默——CRC 已逐块验证）
            }
        }
        if dec.idat_n == 0 {
            return Err(PngError::NoIdat);
        }
        if !saw_iend {
            return Err(PngError::Truncated);
        }
        Ok(dec)
    }

    /// 解码到 RGBA8 图像缓冲。
    /// `raw`：精确 [`png_raw_size`] 大小的滤波域 scratch（不足/超额都拒）；
    /// `prev`：一行重建值 scratch（≥ 宽×通道）；`image`/`stride`：RGBA 出口。
    pub fn decode_rgba(
        &self,
        raw: &mut [u8],
        prev: &mut [u8],
        image: &mut [u8],
        stride: usize,
    ) -> Result<PngHeader, PngError> {
        let h = &self.header;
        let ch = h.color.channels();
        let sw = h.width as usize * ch;
        let need = png_raw_size(h.width, h.height, ch, h.interlace) as usize;
        if raw.len() != need {
            return Err(PngError::ScratchSize { need, got: raw.len() });
        }
        if prev.len() < sw {
            return Err(PngError::ScratchSize { need: sw, got: prev.len() });
        }
        let img_need = h.height as usize * stride;
        if image.len() < img_need {
            return Err(PngError::ScratchSize { need: img_need, got: image.len() });
        }
        let slices: &[&[u8]] = &self.idat[..self.idat_n];
        let n = zlib_inflate_slices(slices, raw)?;
        if n != need {
            return Err(PngError::Truncated);
        }
        if h.interlace == 0 {
            self.decode_progressive(raw, prev, image, stride, ch, sw)?;
        } else {
            self.decode_adam7(raw, prev, image, stride, ch)?;
        }
        Ok(self.header)
    }

    fn decode_progressive(&self, raw: &mut [u8], prev: &mut [u8], image: &mut [u8], stride: usize, ch: usize, sw: usize) -> Result<(), PngError> {
        let hdr = &self.header;
        let row_bytes = 1 + sw;
        for y in 0..hdr.height as usize {
            let base = y * row_bytes;
            let ftype = filter_from_u8(raw[base]).ok_or(PngError::BadFilter)?;
            let (row, prev_row): (&mut [u8], &[u8]) = if y == 0 {
                for p in prev[..sw].iter_mut() {
                    *p = 0;
                }
                (&mut raw[base + 1..base + 1 + sw], &prev[..sw])
            } else {
                // 相邻两行都在 raw 内：split_at_mut 在行边界切开，借用互斥化。
                // 上一行数据域 = raw[base-sw..base]（base = 本行 filter 字节位）。
                let (head, tail) = raw.split_at_mut(base + 1);
                (&mut tail[..sw], &head[base - sw..base])
            };
            row_filter(ftype, row, prev_row, ch).map_err(|_| PngError::BadFilter)?;
            let dst = &mut image[y * stride..y * stride + hdr.width as usize * 4];
            for x in 0..hdr.width as usize {
                expand_pixel(hdr.color, &self.palette, self.palette_len, &row[x * ch..x * ch + ch], &mut dst[x * 4..x * 4 + 4])?;
            }
        }
        Ok(())
    }

    fn decode_adam7(&self, raw: &mut [u8], prev: &mut [u8], image: &mut [u8], stride: usize, ch: usize) -> Result<(), PngError> {
        let hdr = &self.header;
        let mut off = 0usize;
        for p in 0..7 {
            let (cols, rows) = adam7_pass_dim(hdr.width, hdr.height, p);
            if rows == 0 {
                continue;
            }
            let sp = cols as usize * ch;
            for q in prev[..sp].iter_mut() {
                *q = 0; // 每遍首行以上一空行为基准（遍间不延续）
            }
            for j in 0..rows as usize {
                let ftype = filter_from_u8(raw[off]).ok_or(PngError::BadFilter)?;
                let row = &mut raw[off + 1..off + 1 + sp];
                row_filter(ftype, row, &prev[..sp], ch).map_err(|_| PngError::BadFilter)?;
                prev[..sp].copy_from_slice(row);
                let y = (ADAM7_Y0[p] as usize) + j * ADAM7_DY[p] as usize;
                let dst = &mut image[y * stride..y * stride + hdr.width as usize * 4];
                for k in 0..cols as usize {
                    let x = (ADAM7_X0[p] as usize) + k * ADAM7_DX[p] as usize;
                    expand_pixel(hdr.color, &self.palette, self.palette_len, &row[k * ch..k * ch + ch], &mut dst[x * 4..x * 4 + 4])?;
                }
                off += 1 + sp;
            }
        }
        Ok(())
    }
}

/// 过滤类型字节 → Filter（PNG §6.3：0-4；5-7 保留）。
fn filter_from_u8(v: u8) -> Option<Filter> {
    Some(match v {
        0 => Filter::None,
        1 => Filter::Sub,
        2 => Filter::Up,
        3 => Filter::Average,
        4 => Filter::Paeth,
        _ => return None,
    })
}

/// 单像素展开到 RGBA8（调色板越界 = BadPalette——外部数据不信任）。
fn expand_pixel(color: ColorType, palette: &[u8; 768], pal_len: usize, src: &[u8], dst: &mut [u8]) -> Result<(), PngError> {
    match color {
        ColorType::Rgba => dst.copy_from_slice(src),
        ColorType::Rgb => {
            dst[0] = src[0];
            dst[1] = src[1];
            dst[2] = src[2];
            dst[3] = 255;
        }
        ColorType::Gray => {
            dst[0] = src[0];
            dst[1] = src[0];
            dst[2] = src[0];
            dst[3] = 255;
        }
        ColorType::GrayAlpha => {
            dst[0] = src[0];
            dst[1] = src[0];
            dst[2] = src[0];
            dst[3] = src[1];
        }
        ColorType::Palette => {
            let idx = src[0] as usize;
            if idx * 3 + 3 > pal_len {
                return Err(PngError::BadPalette);
            }
            dst[0] = palette[idx * 3];
            dst[1] = palette[idx * 3 + 1];
            dst[2] = palette[idx * 3 + 2];
            dst[3] = 255;
        }
    }
    Ok(())
}

/// 一步到位：解析 + 解码到 RGBA8。
pub fn png_decode_rgba(file: &[u8], raw: &mut [u8], prev: &mut [u8], image: &mut [u8], stride: usize) -> Result<PngHeader, PngError> {
    PngDecoder::new(file)?.decode_rgba(raw, prev, image, stride)
}

// ---------------------------------------------------------------------------
// 8. CheckSet（挂 F054；容器判例用 stored 块——不把 LZ77 表请进内核栈）
// ---------------------------------------------------------------------------

/// 运行检查项（17 域 CheckSet 之一；判据锚点见对账表批次四段）。
pub fn run_checks() -> CheckSet {
    let mut cs = CheckSet::new("F054-mech-inflate");
    // 1) stored 块 + zlib 封装全链：手造 zlib 流 → 头/尾校验 → 解压一致。
    {
        let payload = [0xABu8; 300];
        let mut stream = [0u8; 320];
        stream[0] = 0x78;
        stream[1] = 0x01; // CM=8 FCHECK 合法 (0x7801 % 31 == 0)
        stream[2] = 0x01; // BFINAL + BTYPE=00
        stream[3] = (300 & 0xFF) as u8;
        stream[4] = (300 >> 8) as u8;
        stream[5] = !(300 & 0xFF) as u8;
        stream[6] = !(300 >> 8) as u8;
        stream[7..307].copy_from_slice(&payload);
        let ad = adler32(&payload).to_be_bytes();
        stream[307..311].copy_from_slice(&ad);
        let mut out = [0u8; 300];
        // 切到有效流长（311）——tail4 读片的末 4 字节，多垫的零会被当 adler。
        let n = zlib_inflate_slices(&[&stream[..311]], &mut out).unwrap();
        cs.add("zlib_stored_roundtrip", n == 300 && out == payload, "");
    }
    // 2) 动态 Huffman 块（手工构造最小表——解码器动态路径全走）。
    {
        use crate::perfstar::mech_deflate::BitWriter;
        let mut buf = [0u8; 32];
        let mut bw = BitWriter::new(&mut buf);
        bw.put_bits(1, 1).unwrap(); // BFINAL
        bw.put_bits(2, 2).unwrap(); // BTYPE=10 动态
        bw.put_bits(0, 5).unwrap(); // HLIT=257
        bw.put_bits(0, 5).unwrap(); // HDIST=1
        bw.put_bits(14, 4).unwrap(); // HCLEN 原始值 14 → 14+4 = 18 项
        // CL 码长按 CLC_ORDER 规范序（16,17,18,0,8,…,2,14,1）写 18 项：
        // sym18=1、sym0=2、sym1=2，其余 0。规范码：sym18='0'，sym0='10'，sym1='11'。
        for v in [0u32, 0, 1, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2] {
            bw.put_bits(v, 3).unwrap();
        }
        // 码长序列（lit 0..256 共 257 项）：
        // 97 零（18+86）→ sym1(lit97 码长1) → 158 零（138+20）→ sym1(lit256=EOB 码长1)。
        bw.put_code(0b0, 1).unwrap(); // sym18
        bw.put_bits(86, 7).unwrap(); // 11+86 = 97 零
        bw.put_code(0b11, 2).unwrap(); // sym1
        bw.put_code(0b0, 1).unwrap(); // sym18
        bw.put_bits(127, 7).unwrap(); // 11+127 = 138 零
        bw.put_code(0b0, 1).unwrap(); // sym18
        bw.put_bits(9, 7).unwrap(); // 11+9 = 20 零
        bw.put_code(0b11, 2).unwrap(); // sym1
        // HDIST=1：距离表 1 项、码长 0（空距离表——本块无匹配）。
        bw.put_code(0b10, 2).unwrap(); // sym0
        // 数据：'a'（码 0）×3 + EOB（码 1）。
        for _ in 0..3 {
            bw.put_code(0, 1).unwrap();
        }
        bw.put_code(1, 1).unwrap();
        bw.align().unwrap();
        let pos = bw.byte_pos();
        drop(bw);
        let mut dout = [0u8; 8];
        let dn = inflate(&[&buf[..pos]], &mut dout).unwrap();
        cs.add("dynamic_huffman_handmade", &dout[..dn] == b"aaa", "");
    }
    // 3) PNG 容器全链（RGB 3×2，两行两种滤波；stored zlib；CRC/Adler 全验）。
    {
        // 像素：行 0 = R G B（filter=None）；行 1 filter=Up + 增量全零 → 同行 0。
        // 原始流 = [filter0][9 像素] + [filter2][9 增量]，共 20 字节。
        let row0 = [255u8, 0, 0, 0, 255, 0, 0, 0, 255];
        let mut rawpng_body = vec![0u8];
        rawpng_body.extend_from_slice(&row0);
        rawpng_body.push(2u8);
        rawpng_body.extend_from_slice(&[0u8; 9]);
        let mut z = [0u8; 64];
        z[0] = 0x78;
        z[1] = 0x01;
        z[2] = 0x01;
        let bl = rawpng_body.len() as u16;
        z[3] = bl as u8;
        z[4] = (bl >> 8) as u8;
        z[5] = !bl as u8;
        z[6] = !(bl >> 8) as u8;
        z[7..7 + rawpng_body.len()].copy_from_slice(&rawpng_body);
        let tail = 7 + rawpng_body.len();
        z[tail..tail + 4].copy_from_slice(&adler32(&rawpng_body).to_be_bytes());
        // PNG 块组装。
        let mut png = Vec::new();
        png.extend_from_slice(&PNG_SIG);
        push_chunk(&mut png, b"IHDR", &[0, 0, 0, 3, 0, 0, 0, 2, 8, 2, 0, 0, 0]);
        push_chunk(&mut png, b"IDAT", &z[..tail + 4]);
        push_chunk(&mut png, b"IEND", &[]);
        let mut image = [0u8; 3 * 2 * 4];
        let need = png_raw_size(3, 2, 3, 0) as usize;
        let mut scratch = vec![0u8; need];
        let mut prev = [0u8; 16];
        let hdr = png_decode_rgba(&png, &mut scratch, &mut prev, &mut image, 12).unwrap();
        let ok = hdr.width == 3
            && image[0..8] == [255, 0, 0, 255, 0, 255, 0, 255]
            && image[8..12] == [0, 0, 255, 255]
            && image[12..24] == [255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255];
        cs.add("png_container_roundtrip", ok, "");
    }
    // 4) CRC 与 Adler 篡改必拒（零静默：坏数据不进像素）。
    {
        let mut bad_crc = {
            let mut png = Vec::new();
            png.extend_from_slice(&PNG_SIG);
            push_chunk(&mut png, b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 0, 0, 0, 0]);
            // stored：payload=[0x55]，adler([0x55]) = 0x0056_0056。
            push_chunk(&mut png, b"IDAT", &[0x78, 0x01, 0x01, 1, 0, 0xFE, 0xFF, 0x55, 0x00, 0x56, 0x00, 0x56]);
            push_chunk(&mut png, b"IEND", &[]);
            png
        };
        let n = bad_crc.len();
        bad_crc[n - 1] ^= 0xFF; // 翻 IEND CRC
        let mut image = [0u8; 4];
        let mut scratch = [0u8; 2];
        let mut prev = [0u8; 1];
        let crc_rejected = matches!(png_decode_rgba(&bad_crc, &mut scratch, &mut prev, &mut image, 4), Err(PngError::BadCrc { .. }));
        // Adler 篡改：IDAT 数据内翻 adler 一位，但必须重建块（push_chunk
        // 重算 CRC）——在组装后的 PNG 上翻字节会连带破坏块 CRC，
        // 那测的是 CRC 而不是 Adler。
        let bad_idat = {
            let mut d = vec![0x78u8, 0x01, 0x01, 1, 0, 0xFE, 0xFF, 0x55, 0x00, 0x56, 0x00, 0x56];
            d[8] ^= 0x01; // adler 首字节（0x0056_0056 → 0x0156_0056）
            d
        };
        let bad_adler = {
            let mut png = Vec::new();
            png.extend_from_slice(&PNG_SIG);
            push_chunk(&mut png, b"IHDR", &[0, 0, 0, 1, 0, 0, 0, 1, 8, 0, 0, 0, 0]);
            push_chunk(&mut png, b"IDAT", &bad_idat);
            push_chunk(&mut png, b"IEND", &[]);
            png
        };
        let adler_rejected = matches!(png_decode_rgba(&bad_adler, &mut scratch, &mut prev, &mut image, 4), Err(PngError::BadAdler { .. }));
        cs.add("crc_adler_enforced", crc_rejected && adler_rejected, "");
    }
    // 5) 截断/坏头诚实拒绝。
    {
        // sig + 3 字节（块长度域被截断）→ Truncated。
        let mut trunc = [0u8; 11];
        trunc[..8].copy_from_slice(&PNG_SIG);
        let mut image = [0u8; 4];
        let mut scratch = [0u8; 2];
        let mut prev = [0u8; 1];
        let t = matches!(png_decode_rgba(&trunc, &mut scratch, &mut prev, &mut image, 4), Err(PngError::Truncated));
        let notpng = matches!(png_decode_rgba(&[1, 2, 3, 4, 5, 6, 7, 8], &mut scratch, &mut prev, &mut image, 4), Err(PngError::NotPng));
        cs.add("truncated_and_sig_rejected", t && notpng, "");
    }
    // 6) Adam7 平面恒等式：七遍像素总和 = 宽×高（多组尺寸）。
    {
        let mut ok = true;
        for (w, hh) in [(9u32, 9u32), (3840, 2160), (17, 13), (1, 1), (8, 8)] {
            let mut sum: u64 = 0;
            for p in 0..7 {
                let (c, r) = adam7_pass_dim(w, hh, p);
                sum += c as u64 * r as u64;
            }
            ok &= sum == w as u64 * hh as u64;
        }
        cs.add("adam7_plan_totals", ok, "");
    }
    // 7) Adam7 真解码：9×9 交错 PNG → 还原逐像素一致。
    {
        // 源图 9×9 RGB：pixel(x,y) = (x*28, y*28, (x+y)*14)（值域内确定）。
        let px = |x: u32, y: u32| -> [u8; 3] { [(x * 28) as u8, (y * 28) as u8, ((x + y) * 14) as u8] };
        // 编码：七遍扫描行（filter=0）。stored zlib 封装。
        let mut body: Vec<u8> = Vec::new();
        for p in 0..7 {
            let (cols, rows) = adam7_pass_dim(9, 9, p);
            for j in 0..rows {
                body.push(0u8); // filter None
                for k in 0..cols {
                    let x = ADAM7_X0[p] + k * ADAM7_DX[p];
                    let y = ADAM7_Y0[p] + j * ADAM7_DY[p];
                    body.extend_from_slice(&px(x, y));
                }
            }
        }
        let mut z = [0u8; 512];
        z[0] = 0x78;
        z[1] = 0x01;
        z[2] = 0x01;
        let bl = body.len() as u16;
        z[3] = bl as u8;
        z[4] = (bl >> 8) as u8;
        z[5] = !bl as u8;
        z[6] = !(bl >> 8) as u8;
        z[7..7 + body.len()].copy_from_slice(&body);
        let tail = 7 + body.len();
        z[tail..tail + 4].copy_from_slice(&adler32(&body).to_be_bytes());
        let mut png = Vec::new();
        png.extend_from_slice(&PNG_SIG);
        push_chunk(&mut png, b"IHDR", &[0, 0, 0, 9, 0, 0, 0, 9, 8, 2, 0, 0, 1]);
        push_chunk(&mut png, b"IDAT", &z[..tail + 4]);
        push_chunk(&mut png, b"IEND", &[]);
        let mut image = [0u8; 9 * 9 * 4];
        let need = png_raw_size(9, 9, 3, 1) as usize;
        let mut scratch = vec![0u8; need];
        let mut prev = [0u8; 32];
        let hdr = png_decode_rgba(&png, &mut scratch, &mut prev, &mut image, 9 * 4).unwrap();
        let mut ok = hdr.interlace == 1;
        for y in 0..9u32 {
            for x in 0..9u32 {
                let [r, g, b] = px(x, y);
                let o = (y * 9 + x) as usize * 4;
                ok &= image[o] == r && image[o + 1] == g && image[o + 2] == b && image[o + 3] == 255;
            }
        }
        cs.add("adam7_decode_roundtrip", ok, "");
    }
    // 8) 调色板 + 灰度路径（展开语义对拍）。
    {
        // 调色板 2 色：黑、白。1×2 图像 idx [0,1]。原始流 = [filter0][0][1]。
        // stored：LEN=3、NLEN=0xFFFC；adler([0,0,1])：s1=2、s2=4 → 0x0004_0002。
        let mut png = Vec::new();
        png.extend_from_slice(&PNG_SIG);
        push_chunk(&mut png, b"IHDR", &[0, 0, 0, 2, 0, 0, 0, 1, 8, 3, 0, 0, 0]);
        push_chunk(&mut png, b"PLTE", &[0, 0, 0, 255, 255, 255]);
        push_chunk(&mut png, b"IDAT", &[0x78, 0x01, 0x01, 3, 0, 0xFC, 0xFF, 0x00, 0x00, 0x01, 0x00, 0x04, 0x00, 0x02]);
        push_chunk(&mut png, b"IEND", &[]);
        let mut image = [0u8; 8];
        let mut scratch = [0u8; 3];
        let mut prev = [0u8; 2];
        let r = png_decode_rgba(&png, &mut scratch, &mut prev, &mut image, 8);
        // stored 块 LEN=3（filter+2 索引字节），NLEN 取反、adler=(1+0)+... 手工对齐。
        let ok_pal = matches!(r, Ok(h) if h.color == ColorType::Palette)
            && image[0..4] == [0, 0, 0, 255]
            && image[4..8] == [255, 255, 255, 255];
        cs.add("palette_expand", ok_pal, "");
    }
    cs
}

/// 检查判例专用块写入器（stored 块——检查栈不请 LZ77 表；附 CRC）。
fn push_chunk(out: &mut Vec<u8>, ty: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(ty);
    out.extend_from_slice(data);
    // CRC = type ++ data（两段流式——正是 crc32_span 的用武之地）。
    out.extend_from_slice(&crc32_span(ty, data).to_be_bytes());
}

// ---------------------------------------------------------------------------
// 9. 宿主单测（编码器↔解码器全链对拍在这里——栈预算允许 LZ77 表）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::perfstar::frameledger_ext::crc32;
    use crate::perfstar::mech_deflate::{deflate_fixed, Lz77};
    use crate::perfstar::mech_sim::XorShift64;

    #[test]
    fn crc_span_matches_one_shot() {
        // 与 frameledger_ext::crc32（one-shot）对拍：同一数学必须同一结果。
        let a = b"IDAT";
        let b: Vec<u8> = (0..500u32).map(|i| (i % 251) as u8).collect();
        let mut concat = Vec::new();
        concat.extend_from_slice(a);
        concat.extend_from_slice(&b);
        assert_eq!(crc32_span(a, &b), crc32(&concat));
        // 空段退化也一致。
        assert_eq!(crc32_span(a, &[]), crc32(a));
    }

    #[test]
    fn adler32_known_vector() {
        assert_eq!(adler32(b""), 1);
        assert_eq!(adler32(b"a"), 0x0062_0062); // s1=98, s2=1+97=98（RFC1950：s2 累积 s1）
        assert_eq!(adler32(b"abc"), 0x024d_0127);
    }

    #[test]
    fn tail4_across_slices() {
        assert_eq!(tail4(&[&[1u8, 2], &[3, 4]]), Some([1, 2, 3, 4]));
        assert_eq!(tail4(&[&[1u8]]), None);
        assert_eq!(tail4(&[&[1u8, 2, 3, 4, 5]]), Some([2, 3, 4, 5]));
    }

    #[test]
    fn encoder_decoder_full_crosscheck() {
        // 编码器（mech_deflate，独立实现）→ 解码器（本件）逐字节一致。
        // 三种形态：随机不可压 / 重复可压 / 混合。
        let mut r = XorShift64::new(99);
        let mut mixed = Vec::new();
        for i in 0..2000u32 {
            if i % 7 == 0 {
                let v = r.next_u64() as u8;
                for _ in 0..40 {
                    mixed.push(v);
                }
            } else {
                mixed.push(r.next_u64() as u8);
            }
        }
        let sets: [Vec<u8>; 3] = [
            (0..3000).map(|_| r.next_u64() as u8).collect(),
            vec![0xAAu8; 10000],
            mixed,
        ];
        for input in sets {
            let mut lz = Lz77::new();
            let mut enc = vec![0u8; input.len() + 4096];
            let n = deflate_fixed(&mut lz, &input, &mut enc).unwrap();
            let back = inflate_vec(&enc[..n], input.len() + 16).unwrap();
            assert_eq!(back, input, "编码器↔解码器对拍失败（len={}）", input.len());
        }
    }

    #[test]
    fn overflow_and_bad_length_honest() {
        let payload = [7u8; 100];
        let mut z = [0u8; 128];
        z[0] = 0x78;
        z[1] = 0x01;
        z[2] = 0x01;
        z[3] = 100;
        z[4] = 0;
        z[5] = 0x9B; // !100 低字节
        z[6] = 0xFF;
        z[7..107].copy_from_slice(&payload);
        z[107..111].copy_from_slice(&adler32(&payload).to_be_bytes());
        // 输出缓冲太小 → Overflow。
        let mut tiny = [0u8; 10];
        assert_eq!(zlib_inflate_slices(&[&z], &mut tiny), Err(PngError::Inflate(InflateErr::Overflow)));
        // LEN/NLEN 不互补 → InvalidLengths。
        let mut bad = z;
        bad[5] = 0x64; // NLEN 改成与 LEN 相同（互补被破坏）
        let mut full = [0u8; 200];
        assert_eq!(zlib_inflate_slices(&[&bad], &mut full), Err(PngError::Inflate(InflateErr::InvalidLengths)));
    }

    #[test]
    fn bad_dist_rejected() {
        // 定长块：length=3 dist=2 但输出只有 1 字节 → BadDist。
        // 手工位流：BFINAL=1 BTYPE=01；lit 'A'(65)：8 位码 0x30+65=0x71；
        // 长度码 257（len 3）：7 位码 1；dist 码 1（d=2）：5 位码 00001。
        use crate::perfstar::mech_deflate::BitWriter;
        let mut buf = [0u8; 8];
        let mut bw = BitWriter::new(&mut buf);
        bw.put_bits(1, 1).unwrap();
        bw.put_bits(1, 2).unwrap();
        bw.put_code(0x71, 8).unwrap(); // 'A'
        bw.put_code(1, 7).unwrap(); // len sym 257 → 3
        bw.put_code(1, 5).unwrap(); // dist sym 1 → d=2 > 产出 1
        bw.align().unwrap();
        let pos = bw.byte_pos();
        drop(bw);
        let mut out = [0u8; 16];
        assert_eq!(inflate(&[&buf[..pos]], &mut out), Err(InflateErr::BadDist));
    }

    #[test]
    fn idat_split_across_chunks_is_transparent() {
        // 同一 zlib 流拆 3 片 IDAT——分片边界对解码透明。
        // payload 长度必须恰为 png_raw_size(88,1,1,0) = 1 行 × (1 filter + 88 px) = 89，
        // 否则 decode 的 n != need 契约如实报 Truncated。
        let mut payload = vec![0u8; 89];
        for (i, p) in payload.iter_mut().enumerate().skip(1) {
            *p = ((i - 1) as u32 % 97) as u8; // 首字节是 filter 0，其后是像素
        }
        let mut z: Vec<u8> = vec![0x78, 0x01, 0x01];
        let bl = payload.len() as u16;
        z.extend_from_slice(&bl.to_le_bytes());
        z.extend_from_slice(&(!bl).to_le_bytes());
        z.extend_from_slice(&payload);
        z.extend_from_slice(&adler32(&payload).to_be_bytes());
        let (cut1, cut2) = (5, 11);
        let mut png = Vec::new();
        png.extend_from_slice(&PNG_SIG);
        push_chunk(&mut png, b"IHDR", &[0, 0, 0, 0x58, 0, 0, 0, 1, 8, 0, 0, 0, 0]); // 88×1 gray
        push_chunk(&mut png, b"IDAT", &z[..cut1]);
        push_chunk(&mut png, b"IDAT", &z[cut1..cut2]);
        push_chunk(&mut png, b"IDAT", &z[cut2..]);
        push_chunk(&mut png, b"IEND", &[]);
        let dec = PngDecoder::new(&png).unwrap();
        assert_eq!(dec.idat_n, 3);
        let need = png_raw_size(88, 1, 1, 0) as usize;
        let mut scratch = vec![0u8; need];
        let mut image = vec![0u8; 88 * 4];
        let mut prev = [0u8; 88];
        dec.decode_rgba(&mut scratch, &mut prev, &mut image, 88 * 4).unwrap();
        for x in 0..88usize {
            assert_eq!(image[x * 4], (x as u32 % 97) as u8);
            assert_eq!(image[x * 4 + 3], 255);
        }
    }

    #[test]
    fn scratch_size_is_exact() {
        let mut png = Vec::new();
        png.extend_from_slice(&PNG_SIG);
        push_chunk(&mut png, b"IHDR", &[0, 0, 0, 4, 0, 0, 0, 2, 8, 0, 0, 0, 0]); // 4×2 gray
        // zlib 流全量：2 头 + 1 块头 + 2 LEN + 2 NLEN + 10 payload + 4 adler = 21。
        let mut z = [0u8; 21];
        z[0] = 0x78;
        z[1] = 0x01;
        z[2] = 0x01;
        z[3] = 10;
        z[4] = 0;
        z[5] = 0xF5;
        z[6] = 0xFF;
        // 2 行：filter0 + 4 像素 ×2（LEN=10 的 payload 全量在流内）。
        z[7] = 0;
        z[8] = 1;
        z[9] = 2;
        z[10] = 3;
        z[11] = 4;
        z[12] = 0;
        z[13] = 5;
        z[14] = 6;
        z[15] = 7;
        z[16] = 8;
        let ad = adler32(&z[7..17]).to_be_bytes();
        z[17..21].copy_from_slice(&ad);
        push_chunk(&mut png, b"IDAT", &z);
        push_chunk(&mut png, b"IEND", &[]);
        // 需求 = 2*(1+4) = 10；多 1 字节也拒（精确契约）。
        let mut big = vec![0u8; 11];
        let mut image = vec![0u8; 2 * 16];
        let mut prev = [0u8; 4];
        assert_eq!(
            png_decode_rgba(&png, &mut big, &mut prev, &mut image, 16),
            Err(PngError::ScratchSize { need: 10, got: 11 })
        );
        big.truncate(10);
        let hdr = png_decode_rgba(&png, &mut big, &mut prev, &mut image, 16).unwrap();
        assert_eq!(hdr.width, 4);
        assert_eq!(&image[..8], &[1, 1, 1, 255, 2, 2, 2, 255]);
        assert_eq!(&image[16..24], &[5, 5, 5, 255, 6, 6, 6, 255]);
    }
}

