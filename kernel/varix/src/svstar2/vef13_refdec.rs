//! VE-F1011 · 独立**参考解码器**与独立**语料字节生成器**（本单的第一路与第二路）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F1011`
//!
//! 本文件承载对拍矩阵的**两条独立支路**，两条都不调用被测实现（F1001 / F1003 /
//! F1002）的任何代码：
//!
//! 1. **字节生成侧**（[`deflate_stored`] / [`deflate_fixed_literal`] /
//!    [`deflate_dynamic_literal`] / [`zlib_wrap`] / [`crc32`] / [`adler32`] /
//!    [`apply_filter`] / [`pack_bits`]）：自带 CRC-32、Adler-32、三种 deflate
//!    块、五种滤波器、位打包。**固定块与动态块刻意只发字面量、不发 LZ77 匹配**
//!    ——被测的 F1002 发的是 LZ77 匹配流，两侧字节形态不同才是真互操作。
//! 2. **解码侧**（[`ref_decode`]）：自带 zlib 头校验、inflate（stored / fixed /
//!    dynamic 三种块全支持，含长度/距离码）、反滤波、Adam7 反交织、调色板与
//!    tRNS 展开。
//!
//! ## 为什么不共用一份 Huffman 表
//!
//! 两侧各写一份 canonical Huffman（写侧 [`canonical_codes`]、读侧
//! [`Huff`]）是刻意的：共用一份会让「写出的表」与「读回的表」在同一个
//! 公式下同时对或同时错，对拍恒绿。本文件两处实现方式不同（写侧用
//! `next[]` 增量分配 + Kraft 上界校验，读侧用 puff 风格的 `counts/symbols` +
//! 逐位试探），因此**同一张表被两端各自验证**。
//!
//! ## 零 panic 面
//!
//! 无 `unwrap` / `expect` / `panic!`（[`Huff::build`] 的 `fixed_tables` 走
//! `unwrap_or` 空表分支，随后解码立刻报 [`RefError::ZlibStream`] 而不是 panic）。

#![allow(clippy::needless_range_loop)]

use alloc::vec;
use alloc::vec::Vec;

// ===========================================================================
// 一、校验和
// ===========================================================================

/// CRC-32（PNG 用的反reflect 多项式 0xEDB88320；逐位实现，无表）。
pub fn crc32(bytes: &[u8]) -> u32 {
    let mut c: u32 = 0xFFFF_FFFF;
    let mut i = 0usize;
    while i < bytes.len() {
        c ^= bytes[i] as u32;
        let mut k = 0u32;
        while k < 8 {
            c = if c & 1 != 0 { (c >> 1) ^ 0xEDB8_8320 } else { c >> 1 };
            k += 1;
        }
        i += 1;
    }
    !c
}

/// CRC-32 跨两段（块类型 + 块载荷，状态连续）。
pub fn crc32_two(a: &[u8], b: &[u8]) -> u32 {
    let mut c: u32 = 0xFFFF_FFFF;
    let mut i = 0usize;
    while i < a.len() {
        c ^= a[i] as u32;
        let mut k = 0u32;
        while k < 8 {
            c = if c & 1 != 0 { (c >> 1) ^ 0xEDB8_8320 } else { c >> 1 };
            k += 1;
        }
        i += 1;
    }
    let mut j = 0usize;
    while j < b.len() {
        c ^= b[j] as u32;
        let mut k = 0u32;
        while k < 8 {
            c = if c & 1 != 0 { (c >> 1) ^ 0xEDB8_8320 } else { c >> 1 };
            k += 1;
        }
        j += 1;
    }
    !c
}

/// Adler-32（zlib 校验）。
pub fn adler32(data: &[u8]) -> u32 {
    let mut a: u32 = 1;
    let mut b: u32 = 0;
    let mut i = 0usize;
    while i < data.len() {
        a = (a + data[i] as u32) % 65521u32;
        b = (b + a) % 65521u32;
        i += 1;
    }
    (b << 16) | a
}

// ===========================================================================
// 二、位流读写
// ===========================================================================

/// 位写入器（DEFLATE 约定：低位在前，字节内也是低位在前）。
struct BitWriter {
    out: Vec<u8>,
    acc: u32,
    nbits: u32,
}

impl BitWriter {
    fn new() -> BitWriter {
        BitWriter { out: Vec::new(), acc: 0, nbits: 0 }
    }

    /// 写 `n` 位（低位在前）。
    fn put(&mut self, value: u32, n: u32) {
        let mut v = value;
        let mut left = n;
        while left > 0 {
            let space = 8 - self.nbits;
            let take = core::cmp::min(space, left);
            let mask = (1u32 << take) - 1u32;
            self.acc |= (v & mask) << self.nbits;
            self.nbits += take;
            v >>= take;
            left -= take;
            while self.nbits >= 8 {
                self.out.push((self.acc & 0xFF) as u8);
                self.acc >>= 8;
                self.nbits -= 8;
            }
        }
    }

    /// 写 Huffman 码（码字本身**高位在前**，故先翻转再按低位在前写）。
    fn put_code(&mut self, code: u32, n: u32) {
        let mut rev = 0u32;
        let mut i = 0u32;
        while i < n {
            rev |= ((code >> i) & 1) << (n - 1 - i);
            i += 1;
        }
        self.put(rev, n);
    }

    /// 补齐到字节边界。
    fn align(&mut self) {
        if self.nbits > 0 {
            self.out.push((self.acc & 0xFF) as u8);
            self.acc = 0;
            self.nbits = 0;
        }
    }

    fn finish(mut self) -> Vec<u8> {
        self.align();
        self.out
    }
}

/// 位读取器（低位在前）。
struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
    bit: u32,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> BitReader<'a> {
        BitReader { data, pos: 0, bit: 0 }
    }

    /// 读 `n` 位（低位在前）。
    fn bits(&mut self, n: u32) -> Result<u32, RefError> {
        let mut v = 0u32;
        let mut got = 0u32;
        while got < n {
            let byte = match self.data.get(self.pos) {
                Some(b) => *b,
                None => return Err(RefError::ZlibStream),
            };
            let avail = 8 - self.bit;
            let take = core::cmp::min(avail, n - got);
            let part = ((byte >> self.bit) as u32) & ((1u32 << take) - 1u32);
            v |= part << got;
            got += take;
            self.bit += take;
            if self.bit >= 8 {
                self.bit = 0;
                self.pos += 1;
            }
        }
        Ok(v)
    }

    /// 补齐到字节边界（跳掉当前字节的剩余位）。
    fn align(&mut self) {
        if self.bit != 0 {
            self.bit = 0;
            self.pos += 1;
        }
    }
}

// ===========================================================================
// 三、deflate 写侧（三种块）与 zlib 封装
// ===========================================================================

/// fixed Huffman 字面量的码长与码值（RFC 1951 §3.2.6）。
fn fixed_literal_code(sym: u16) -> (u32, u32) {
    if sym < 144 {
        (8, 0x30 + sym as u32)
    } else if sym < 256 {
        (9, 0x190 + (sym as u32 - 144))
    } else if sym < 280 {
        (7, sym as u32 - 256)
    } else {
        (8, 0xC0 + (sym as u32 - 280))
    }
}

/// deflate：stored 块（BTYPE=00）。
pub fn deflate_stored(raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(raw.len() + 64);
    let mut off = 0usize;
    loop {
        let n = core::cmp::min(65535, raw.len() - off);
        let last = off + n >= raw.len();
        out.push(if last { 1u8 } else { 0u8 });
        let l = n as u16;
        out.extend_from_slice(&l.to_le_bytes());
        out.extend_from_slice(&(!l).to_le_bytes());
        out.extend_from_slice(&raw[off..off + n]);
        off += n;
        if last {
            break;
        }
    }
    out
}

/// deflate：fixed Huffman 块（BTYPE=01），**纯字面量、无 LZ77 匹配**。
pub fn deflate_fixed_literal(raw: &[u8]) -> Vec<u8> {
    let mut w = BitWriter::new();
    w.put(1, 1); // BFINAL = 1
    w.put(1, 2); // BTYPE = 01（低位在前：位序 1,0）
    let mut i = 0usize;
    while i < raw.len() {
        let (n, code) = fixed_literal_code(raw[i] as u16);
        w.put_code(code, n);
        i += 1;
    }
    let (n, code) = fixed_literal_code(256); // EOB
    w.put_code(code, n);
    w.finish()
}

/// dynamic 块的字面量码长表（**完全码**：255 个 8 位 + 符号 255/256 各 9 位，
/// Kraft 和 = 255/256 + 2/512 = 1）。
///
/// 距离表只有 1 个码（长度 1）——zlib 的 `inflate_table` 对 LENS/DISTS 类
/// **不允许**欠订阅（唯一例外是「距离树恰好一个码」），这一点是 dynamic 块
/// 能被第三方解码器接受的前提。
fn dynamic_literal_lengths() -> ([u8; 257], [u8; 1]) {
    let mut lit = [0u8; 257];
    let mut i = 0usize;
    while i < 255 {
        lit[i] = 8;
        i += 1;
    }
    lit[255] = 9;
    lit[256] = 9;
    let dist = [1u8; 1];
    (lit, dist)
}

/// canonical Huffman 码表分配（写侧：`next[]` 增量法 + Kraft 上界校验）。
///
/// 过订阅（Kraft 和 > 1）显式报错：canonical 分配在过订阅时会**静默**把后面
/// 的码挤出定义域，发出去的是「解码器读得到、但读到的是另一个符号」的坏流，
/// 极难定位，故此处失败即报错。
fn canonical_codes(lengths: &[u8]) -> Result<Vec<(u32, u32)>, RefError> {
    let mut count = [0u32; 16];
    let mut i = 0usize;
    while i < lengths.len() {
        let l = lengths[i] as usize;
        if l > 15 {
            return Err(RefError::ZlibStream);
        }
        count[l] += 1;
        i += 1;
    }
    if count[0] as usize == lengths.len() {
        return Err(RefError::HuffmanEmpty);
    }
    let mut kraft: u32 = 0;
    let mut len = 1usize;
    while len <= 15 {
        kraft += count[len] << (15 - len);
        len += 1;
    }
    if kraft > 32768 {
        return Err(RefError::HuffmanOverSubscribed);
    }
    let mut next = [0u32; 16];
    let mut code = 0u32;
    len = 1usize;
    while len <= 15 {
        code = (code + count[len - 1]) << 1;
        next[len] = code;
        len += 1;
    }
    let mut table = vec![(0u32, 0u32); lengths.len()];
    let mut sym = 0usize;
    while sym < lengths.len() {
        let l = lengths[sym] as usize;
        if l != 0 {
            table[sym] = (l as u32, next[l]);
            next[l] += 1;
        }
        sym += 1;
    }
    Ok(table)
}

/// 码长码的排列顺序（RFC 1951 §3.2.7）。
const CL_ORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];

/// deflate：dynamic Huffman 块（BTYPE=10），带完整表头，**纯字面量**。
///
/// 表头构造（全手写）：
/// - 码长码字表对符号 `{1, 8, 9, 16}` 各给 2 位（4 × 2⁻² = 1，**完全码**）；
/// - 码长序列（258 项）用 16/17/18 游程压缩；
/// - HCLEN 取到 ORDER 中最后一个非零下标（此处恒为 17 → 字段值 13）。
pub fn deflate_dynamic_literal(raw: &[u8]) -> Result<Vec<u8>, RefError> {
    let (lit_lengths, dist_lengths) = dynamic_literal_lengths();
    let lit_table = canonical_codes(&lit_lengths)?;
    let dist_table = canonical_codes(&dist_lengths)?;
    let mut cl_len = [0u8; 19];
    cl_len[1] = 2;
    cl_len[8] = 2;
    cl_len[9] = 2;
    cl_len[16] = 2;
    let cl_table = canonical_codes(&cl_len)?;
    let mut all: Vec<u8> = Vec::with_capacity(258);
    all.extend_from_slice(&lit_lengths);
    all.extend_from_slice(&dist_lengths);
    let mut w = BitWriter::new();
    w.put(1, 1); // BFINAL
    w.put(2, 2); // BTYPE = 10（值 2 → 位序 0,1）
    w.put((lit_lengths.len() - 257) as u32, 5); // HLIT 字段（257 → 0）
    w.put((dist_lengths.len() - 1) as u32, 5); // HDIST 字段（1 → 0）
    let mut last = 0usize;
    let mut k = 0usize;
    while k < CL_ORDER.len() {
        if cl_len[CL_ORDER[k]] != 0 {
            last = k;
        }
        k += 1;
    }
    w.put(last as u32, 4); // HCLEN 字段 = (last + 4) − 4
    let mut j = 0usize;
    while j <= last {
        w.put(cl_len[CL_ORDER[j]] as u32, 3);
        j += 1;
    }
    // ---- 码长序列的游程压缩 ----
    let mut i = 0usize;
    while i < all.len() {
        let cur = all[i];
        let mut run = 1usize;
        while i + run < all.len() && all[i + run] == cur {
            run += 1;
        }
        if cur == 0 {
            let mut left = run;
            while left >= 11 {
                let n = core::cmp::min(left, 138);
                let (cd, cl) = cl_table[18];
                w.put_code(cd, cl);
                w.put((n - 11) as u32, 7);
                left -= n;
            }
            while left >= 3 {
                let n = core::cmp::min(left, 10);
                let (cd, cl) = cl_table[17];
                w.put_code(cd, cl);
                w.put((n - 3) as u32, 3);
                left -= n;
            }
            while left > 0 {
                let (cd, cl) = cl_table[0];
                w.put_code(cd, cl);
                left -= 1;
            }
        } else {
            let (cd, cl) = cl_table[cur as usize];
            w.put_code(cd, cl);
            let mut left = run - 1;
            while left >= 3 {
                let n = core::cmp::min(left, 6);
                let (cd2, cl2) = cl_table[16];
                w.put_code(cd2, cl2);
                w.put((n - 3) as u32, 2);
                left -= n;
            }
            while left > 0 {
                let (cd2, cl2) = cl_table[cur as usize];
                w.put_code(cd2, cl2);
                left -= 1;
            }
        }
        i += run;
    }
    // ---- 数据：字面量 + EOB（不使用任何距离码） ----
    let mut n = 0usize;
    while n < raw.len() {
        let (cd, cl) = lit_table[raw[n] as usize];
        w.put_code(cd, cl);
        n += 1;
    }
    let (cd, cl) = lit_table[256];
    w.put_code(cd, cl);
    Ok(w.finish())
}

/// zlib 封装（CMF/FLG + deflate + Adler-32）。
///
/// `cm` 非 8 时 CMF 的低 4 位就写成非 8 —— 这是**故意**的非法输入开关，
/// 供畸形语料 `xzlb01` 使用。
pub fn zlib_wrap(deflated: &[u8], raw: &[u8], cm: u8) -> Vec<u8> {
    let cmf = (cm & 0x0F) | 0x08; // CINFO = 0（32K 窗口）
    let mut f = 1u8; // FLEVEL = 0，FCHECK 初值
    while ((cmf as u32) * 256 + (f as u32)) % 31 != 0 {
        f = f.wrapping_add(1);
    }
    let mut out = Vec::with_capacity(deflated.len() + 6);
    out.push(cmf);
    out.push(f);
    out.extend_from_slice(deflated);
    out.extend_from_slice(&adler32(raw).to_be_bytes());
    out
}

// ===========================================================================
// 四、滤波器与位打包（写侧，自带实现）
// ===========================================================================

/// Paeth 预测（规范 §9.2.3）。
pub fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let p = a as i32 + b as i32 - c as i32;
    let pa = (p - a as i32).abs();
    let pb = (p - b as i32).abs();
    let pc = (p - c as i32).abs();
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

/// 逐行滤波（`fno`：0 None / 1 Sub / 2 Up / 3 Average / 4 Paeth）。
///
/// 越界滤波号或缓冲长度不足一律给 `false`，**绝不**带着非法滤波号继续写。
pub fn apply_filter(fno: u8, cur: &[u8], prev: &[u8], bpp: usize, out: &mut [u8]) -> bool {
    if fno > 4 || bpp == 0 || out.len() < cur.len() || prev.len() < cur.len() {
        return false;
    }
    let n = cur.len();
    let mut i = 0usize;
    while i < n {
        let a = if i >= bpp { cur[i - bpp] } else { 0 };
        let b = prev.get(i).copied().unwrap_or(0);
        let c = if i >= bpp { prev.get(i - bpp).copied().unwrap_or(0) } else { 0 };
        let x = cur[i];
        let v = match fno {
            0 => x,
            1 => x.wrapping_add(a),
            2 => x.wrapping_add(b),
            3 => {
                let p = a as u32 + b as u32;
                x.wrapping_add((p / 2) as u8)
            }
            _ => x.wrapping_add(paeth(a, b, c)),
        };
        out[i] = v;
        i += 1;
    }
    true
}

/// 规范最小和启发式（逐行试五种滤波，取绝对差和最小者）。
pub fn choose_filter(cur: &[u8], prev: &[u8], bpp: usize, trial: &mut [u8]) -> u8 {
    let mut best = 0u8;
    let mut best_score = u64::MAX;
    let mut fno = 0u8;
    while fno < 5 {
        if !apply_filter(fno, cur, prev, bpp, trial) {
            break;
        }
        let mut score = 0u64;
        let mut i = 0usize;
        while i < trial.len() {
            let v = trial[i];
            score += if v < 128 { v as u64 } else { 256 - v as u64 };
            i += 1;
        }
        if score < best_score {
            best_score = score;
            best = fno;
        }
        fno += 1;
    }
    best
}

/// 位深 <8 的位打包（规范 §9.1：高位在左，逐像素跨字节）。
pub fn pack_bits(values: &[u8], depth: u8, out: &mut [u8]) -> usize {
    let per = 8 / depth as usize;
    let need = (values.len() + per - 1) / per;
    if out.len() < need || depth == 0 || depth > 4 || (8 % depth as usize) != 0 {
        return 0;
    }
    let mask = ((1u16 << depth) - 1) as u8;
    let mut i = 0usize;
    while i < need {
        out[i] = 0;
        i += 1;
    }
    let mut v = 0usize;
    while v < values.len() {
        let byte = v / per;
        let slot = v % per;
        let shift = 8 - depth as usize * (slot + 1);
        let cur = out[byte];
        out[byte] = cur | ((values[v] & mask) << shift);
        v += 1;
    }
    need
}

// ===========================================================================
// 五、inflate 读侧（三种块全支持）
// ===========================================================================

/// puff 风格的 canonical Huffman 解码表（`counts` + 顺序 `symbols`）。
struct Huff {
    counts: [u16; 16],
    symbols: Vec<u16>,
}

impl Huff {
    /// 建表：过订阅显式报错，全零显式报错。
    fn build(lengths: &[u8]) -> Result<Huff, RefError> {
        let mut counts = [0u16; 16];
        let mut i = 0usize;
        while i < lengths.len() {
            let l = lengths[i] as usize;
            if l > 15 {
                return Err(RefError::ZlibStream);
            }
            counts[l] += 1;
            i += 1;
        }
        if counts[0] as usize == lengths.len() {
            return Err(RefError::HuffmanEmpty);
        }
        let mut left = 1i32;
        let mut len = 1usize;
        while len <= 15 {
            left -= (counts[len] as i32) << (len as i32 - 1);
            if left < 0 {
                return Err(RefError::HuffmanOverSubscribed);
            }
            len += 1;
        }
        let mut offs = [0u16; 16];
        let mut sum = 0u16;
        len = 1usize;
        while len <= 15 {
            offs[len] = sum;
            sum += counts[len];
            len += 1;
        }
        let mut symbols = vec![0u16; sum as usize];
        let mut s = 0usize;
        while s < lengths.len() {
            let l = lengths[s] as usize;
            if l != 0 {
                let slot = offs[l] as usize;
                if let Some(cell) = symbols.get_mut(slot) {
                    *cell = s as u16;
                }
                offs[l] += 1;
            }
            s += 1;
        }
        Ok(Huff { counts, symbols })
    }

    /// 解一个符号（逐位试探；从高位往低位累积码值）。
    fn decode(&self, br: &mut BitReader<'_>) -> Result<u16, RefError> {
        let mut code = 0i32;
        let mut first = 0i32;
        let mut index = 0i32;
        let mut len = 1usize;
        while len <= 15 {
            code |= br.bits(1)? as i32;
            let count = self.counts[len] as i32;
            if code - first < count {
                let slot = (index + (code - first)) as usize;
                return match self.symbols.get(slot) {
                    Some(s) => Ok(*s),
                    None => Err(RefError::ZlibStream),
                };
            }
            index += count;
            first = (first + count) << 1;
            code <<= 1;
            len += 1;
        }
        Err(RefError::ZlibStream)
    }
}

/// fixed Huffman 的两张表（按需构造；语料很小不值得缓存）。
fn fixed_tables() -> (Huff, Huff) {
    let mut lit = [0u8; 288];
    let mut i = 0usize;
    while i < 288 {
        let s = i as u16;
        lit[i] = if s < 144 {
            8
        } else if s < 256 {
            9
        } else if s < 280 {
            7
        } else {
            8
        };
        i += 1;
    }
    let mut dist = [0u8; 30];
    let mut k = 0usize;
    while k < 30 {
        dist[k] = 5;
        k += 1;
    }
    let empty = Huff { counts: [0u16; 16], symbols: Vec::new() };
    // `unwrap_or` 而非 `expect`：建表失败返回空表，随后的解码立刻报 ZlibStream
    // （固定表的码长是常量，不可能失败；真失败时也不该 panic——参考解码器是
    // 被畸形语料喂的入口，任何 panic 都是事故）。
    let l = Huff::build(&lit).unwrap_or(Huff { counts: empty.counts, symbols: Vec::new() });
    let d = Huff::build(&dist).unwrap_or(Huff { counts: empty.counts, symbols: Vec::new() });
    (l, d)
}

/// 读 dynamic 块头并建两张表。
fn dynamic_tables(br: &mut BitReader<'_>) -> Result<(Huff, Huff), RefError> {
    let hlit = br.bits(5)? as usize + 257;
    let hdist = br.bits(5)? as usize + 1;
    let hclen = br.bits(4)? as usize + 4;
    let mut cl = [0u8; 19];
    let mut i = 0usize;
    while i < hclen {
        if i < CL_ORDER.len() {
            cl[CL_ORDER[i]] = br.bits(3)? as u8;
        } else {
            let _ = br.bits(3)?;
        }
        i += 1;
    }
    let cl_huff = Huff::build(&cl)?;
    let total = hlit + hdist;
    let mut lengths = vec![0u8; total];
    let mut k = 0usize;
    while k < total {
        let sym = cl_huff.decode(br)?;
        match sym {
            0..=15 => {
                if let Some(cell) = lengths.get_mut(k) {
                    *cell = sym as u8;
                }
                k += 1;
            }
            16 => {
                if k == 0 {
                    return Err(RefError::ZlibStream);
                }
                let prev = lengths[k - 1];
                let rep = 3 + br.bits(2)? as usize;
                let mut r = 0usize;
                while r < rep && k < total {
                    lengths[k] = prev;
                    k += 1;
                    r += 1;
                }
            }
            17 => {
                let rep = 3 + br.bits(3)? as usize;
                let mut r = 0usize;
                while r < rep && k < total {
                    lengths[k] = 0;
                    k += 1;
                    r += 1;
                }
            }
            18 => {
                let rep = 11 + br.bits(7)? as usize;
                let mut r = 0usize;
                while r < rep && k < total {
                    lengths[k] = 0;
                    k += 1;
                    r += 1;
                }
            }
            _ => return Err(RefError::ZlibStream),
        }
    }
    let lit = Huff::build(&lengths[..hlit])?;
    let dist = Huff::build(&lengths[hlit..])?;
    Ok((lit, dist))
}

/// 长度码基值。
const LEN_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
/// 长度码附加位。
const LEN_EXTRA: [u32; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
/// 距离码基值。
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
/// 距离码附加位。
const DIST_EXTRA: [u32; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

/// 展开一个 deflate 块（字面量 + 回溯拷贝）。
fn inflate_block(
    br: &mut BitReader<'_>,
    lit: &Huff,
    dist: &Huff,
    out: &mut Vec<u8>,
    limit: usize,
) -> Result<(), RefError> {
    loop {
        let sym = lit.decode(br)?;
        if sym < 256 {
            if out.len() >= limit {
                return Err(RefError::ZlibStream);
            }
            out.push(sym as u8);
        } else if sym == 256 {
            return Ok(());
        } else {
            let li = sym as usize - 257;
            if li >= 29 {
                return Err(RefError::ZlibStream);
            }
            let len = LEN_BASE[li] as usize + br.bits(LEN_EXTRA[li])? as usize;
            let dsym = dist.decode(br)? as usize;
            if dsym >= 30 {
                return Err(RefError::ZlibStream);
            }
            let d = DIST_BASE[dsym] as usize + br.bits(DIST_EXTRA[dsym])? as usize;
            if d == 0 || d > out.len() {
                return Err(RefError::ZlibStream);
            }
            if out.len() + len > limit {
                return Err(RefError::ZlibStream);
            }
            let start = out.len() - d;
            let mut i = 0usize;
            while i < len {
                let b = match out.get(start + i) {
                    Some(v) => *v,
                    None => return Err(RefError::ZlibStream),
                };
                out.push(b);
                i += 1;
            }
        }
    }
}

/// 展开 deflate 原始流（stored / fixed / dynamic 三种块全支持）。
pub fn inflate_raw(data: &[u8], limit: usize) -> Result<Vec<u8>, RefError> {
    let mut br = BitReader::new(data);
    let mut out: Vec<u8> = Vec::new();
    loop {
        let bfinal = br.bits(1)?;
        let btype = br.bits(2)?;
        match btype {
            0 => {
                br.align();
                let len = br.bits(16)? as usize;
                let nlen = br.bits(16)? as usize;
                if len != (!nlen & 0xFFFF) {
                    return Err(RefError::ZlibStream);
                }
                if out.len() + len > limit {
                    return Err(RefError::ZlibStream);
                }
                let mut i = 0usize;
                while i < len {
                    let byte = br.bits(8)?;
                    out.push(byte as u8);
                    i += 1;
                }
            }
            1 => {
                let (l, d) = fixed_tables();
                inflate_block(&mut br, &l, &d, &mut out, limit)?;
            }
            2 => {
                let (l, d) = dynamic_tables(&mut br)?;
                inflate_block(&mut br, &l, &d, &mut out, limit)?;
            }
            _ => return Err(RefError::ZlibStream),
        }
        if bfinal == 1 {
            break;
        }
    }
    Ok(out)
}

/// zlib 流解包（头校验 + 展开 + Adler 校验）。
pub fn zlib_inflate(data: &[u8], limit: usize) -> Result<Vec<u8>, RefError> {
    let cmf = match data.first() {
        Some(v) => *v,
        None => return Err(RefError::ZlibStream),
    };
    let flg = match data.get(1) {
        Some(v) => *v,
        None => return Err(RefError::ZlibStream),
    };
    if cmf & 0x0F != 8 || (cmf >> 4) > 7 {
        return Err(RefError::ZlibStream);
    }
    if ((cmf as u32) * 256 + (flg as u32)) % 31 != 0 {
        return Err(RefError::ZlibStream);
    }
    if (flg & 0x20) != 0 {
        return Err(RefError::ZlibStream); // FDICT：本语料不使用预置字典
    }
    if data.len() < 6 {
        return Err(RefError::ZlibStream);
    }
    let body = &data[2..];
    let out = inflate_raw(body, limit)?;
    let tail = &body[body.len() - 4..];
    let got = u32::from_be_bytes([tail[0], tail[1], tail[2], tail[3]]);
    if got != adler32(&out) {
        return Err(RefError::ZlibStream);
    }
    Ok(out)
}

// ===========================================================================
// 六、参考解码器：容器 / 反滤波 / Adam7 / 色彩展开
// ===========================================================================

/// 参考解码器的错误（**自有错误面**，与被测侧的 `PngFault` 无任何共用类型）。
///
/// 分类层（[`RejectReason`](super::vef13_pngsuite::RejectReason)）是两侧唯一
/// 公共语；映射由 `super::vef13_pngsuite::ref_error_to_reason` 一处承担。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RefError {
    /// 签名不符。
    Signature,
    /// 字节流在块结构上截断。
    Truncated,
    /// IHDR 缺失。
    MissingIhdr,
    /// IHDR 字段非法。
    IhdrField,
    /// 位深 × 颜色类型组合非法。
    IllegalCombo,
    /// 宽高非法。
    Dimension,
    /// 像素总量超预算。
    PixelBudget,
    /// critical 块 CRC 错。
    CrcCritical,
    /// 未知 critical 块。
    UnknownCritical,
    /// 调色板非法 / 索引越界 / 类型不允许调色板。
    Palette,
    /// tRNS 载荷长度不符。
    TrnsLen,
    /// IDAT 缺失。
    NoIdat,
    /// 调色板图缺 PLTE。
    PaletteRequired,
    /// zlib 流损坏（头非法 / 展开失败 / Adler 不符 / 位流越界）。
    ZlibStream,
    /// 滤波号非法。
    BadFilter,
    /// 扫描线数据不足（按 IHDR 推导的行数凑不齐）。
    RowShort,
    /// Huffman 表过订阅（写侧或第三方写侧的错误）。
    HuffmanOverSubscribed,
    /// Huffman 表全零。
    HuffmanEmpty,
}

impl RefError {
    /// 中文短名（诊断面）。
    pub fn label(self) -> &'static str {
        match self {
            RefError::Signature => "签名不符",
            RefError::Truncated => "块截断",
            RefError::MissingIhdr => "IHDR 缺失",
            RefError::IhdrField => "IHDR 字段非法",
            RefError::IllegalCombo => "位深×类型非法组合",
            RefError::Dimension => "尺寸非法",
            RefError::PixelBudget => "像素预算超限",
            RefError::CrcCritical => "critical CRC 错",
            RefError::UnknownCritical => "未知 critical 块",
            RefError::Palette => "调色板非法",
            RefError::TrnsLen => "tRNS 长度不符",
            RefError::NoIdat => "IDAT 缺失",
            RefError::PaletteRequired => "调色板图缺 PLTE",
            RefError::ZlibStream => "zlib 流损坏",
            RefError::BadFilter => "滤波号非法",
            RefError::RowShort => "扫描线不足",
            RefError::HuffmanOverSubscribed => "Huffman 过订阅",
            RefError::HuffmanEmpty => "Huffman 全零",
        }
    }
}

/// 参考解码产物。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RefImage {
    /// 宽。
    pub width: u32,
    /// 高。
    pub height: u32,
    /// RGBA8（长度 = width × height × 4）。
    pub rgba: Vec<u8>,
}

/// PNG 签名常量（参考侧自带一份，不借用被测模块的常量）。
const REF_SIG: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
/// 尺寸上限（规范 §4.2.1）。
const REF_MAX_DIM: u32 = 0x7FFF_FFFF;
/// 像素预算（RGBA 字节总量上限，与被测侧同数量级、同意图）。
const REF_PIXEL_BUDGET: u64 = 512 * 1024 * 1024;

/// 4 字节块名。
const F_IHDR: u32 = 0x4948_4452;
const F_PLTE: u32 = 0x504C_5445;
const F_TRNS: u32 = 0x7452_4E53;
const F_IDAT: u32 = 0x4944_4154;
const F_IEND: u32 = 0x4945_4E44;

/// Adam7 七遍几何（`(x0, y0, dx, dy)`，规范 §9.6）。
const REF_ADAM7: [(u32, u32, u32, u32); 7] = [
    (0, 0, 8, 8),
    (4, 0, 8, 8),
    (0, 4, 4, 8),
    (2, 0, 4, 4),
    (0, 2, 2, 4),
    (1, 0, 2, 2),
    (0, 1, 1, 2),
];

/// 容器解析结果。
struct RefContainer {
    width: u32,
    height: u32,
    depth: u8,
    color_raw: u8,
    interlace: u8,
    palette: Vec<u8>,
    palette_len: usize,
    trns: Option<Vec<u8>>,
    idat: Vec<u8>,
}

/// 参考侧的块游标：CRC、块序、critical 分级全部自判。
fn ref_parse(file: &[u8]) -> Result<RefContainer, RefError> {
    if file.len() < 8 {
        return Err(RefError::Signature);
    }
    let mut i = 0usize;
    while i < 8 {
        if file[i] != REF_SIG[i] {
            return Err(RefError::Signature);
        }
        i += 1;
    }
    let mut c = RefContainer {
        width: 0,
        height: 0,
        depth: 0,
        color_raw: 0,
        interlace: 0,
        palette: Vec::new(),
        palette_len: 0,
        trns: None,
        idat: Vec::new(),
    };
    let mut ihdr: Option<[u8; 13]> = None;
    let mut pos = 8usize;
    let mut seen_iend = false;
    while pos < file.len() {
        if seen_iend {
            break;
        }
        if pos + 8 > file.len() {
            return Err(RefError::Truncated);
        }
        let len = u32::from_be_bytes([file[pos], file[pos + 1], file[pos + 2], file[pos + 3]]) as usize;
        let ty = u32::from_be_bytes([file[pos + 4], file[pos + 5], file[pos + 6], file[pos + 7]]);
        if len > file.len() || pos + 12 > file.len() || pos + 12 + len > file.len() {
            return Err(RefError::Truncated);
        }
        let data = &file[pos + 8..pos + 8 + len];
        let ty_bytes = [(ty >> 24) as u8, (ty >> 16) as u8, (ty >> 8) as u8, ty as u8];
        let want = u32::from_be_bytes([
            file[pos + 8 + len],
            file[pos + 9 + len],
            file[pos + 10 + len],
            file[pos + 11 + len],
        ]);
        let got = crc32_two(&ty_bytes, data);
        pos += 12 + len;
        let critical = (ty_bytes[0] & 0x20) == 0;
        if got != want {
            if critical {
                return Err(RefError::CrcCritical);
            }
            continue; // ancillary CRC 错：告警后跳过（与被测侧同口径）
        }
        match ty {
            F_IHDR => {
                if data.len() < 13 {
                    return Err(RefError::IhdrField);
                }
                let mut h = [0u8; 13];
                h.copy_from_slice(&data[..13]);
                ihdr = Some(h);
            }
            F_PLTE => {
                if !c.idat.is_empty() {
                    return Err(RefError::Palette);
                }
                if data.is_empty() || data.len() % 3 != 0 {
                    return Err(RefError::Palette);
                }
                c.palette = data.to_vec();
                c.palette_len = data.len() / 3;
            }
            F_TRNS => {
                if !c.idat.is_empty() {
                    return Err(RefError::Truncated);
                }
                c.trns = Some(data.to_vec());
            }
            F_IDAT => c.idat.extend_from_slice(data),
            F_IEND => seen_iend = true,
            _ => {
                if critical {
                    return Err(RefError::UnknownCritical);
                }
            }
        }
    }
    let h = match ihdr {
        Some(h) => h,
        None => return Err(RefError::MissingIhdr),
    };
    c.width = u32::from_be_bytes([h[0], h[1], h[2], h[3]]);
    c.height = u32::from_be_bytes([h[4], h[5], h[6], h[7]]);
    c.depth = h[8];
    c.color_raw = h[9];
    c.interlace = h[12];
    if c.width == 0 || c.height == 0 || c.width > REF_MAX_DIM || c.height > REF_MAX_DIM {
        return Err(RefError::Dimension);
    }
    if !matches!(c.color_raw, 0 | 2 | 3 | 4 | 6) {
        return Err(RefError::IhdrField);
    }
    if h[10] != 0 || h[11] != 0 {
        return Err(RefError::IhdrField);
    }
    if c.interlace > 1 {
        return Err(RefError::IhdrField);
    }
    if !matches!(c.depth, 1 | 2 | 4 | 8 | 16) {
        return Err(RefError::IhdrField);
    }
    let legal = match (c.color_raw, c.depth) {
        (0, 1..=16) => true,
        (2, 8) | (2, 16) => true,
        (3, 1..=8) => true,
        (4, 8) | (4, 16) => true,
        (6, 8) | (6, 16) => true,
        _ => false,
    };
    if !legal {
        return Err(RefError::IllegalCombo);
    }
    if c.idat.is_empty() {
        return Err(RefError::NoIdat);
    }
    if c.color_raw == 3 && c.palette_len == 0 {
        return Err(RefError::PaletteRequired);
    }
    if c.color_raw != 3 && c.palette_len != 0 {
        return Err(RefError::Palette);
    }
    if let Some(t) = &c.trns {
        let ok = match c.color_raw {
            0 => t.len() == 2,
            2 => t.len() == 6,
            3 => !t.is_empty() && t.len() <= c.palette_len,
            _ => false,
        };
        if !ok {
            return Err(RefError::TrnsLen);
        }
    }
    if (c.width as u64) * (c.height as u64) * 4u64 > REF_PIXEL_BUDGET {
        return Err(RefError::PixelBudget);
    }
    Ok(c)
}

/// 参考侧反滤波（滤波号先查域）。
fn ref_unfilter(fno: u8, cur: &mut [u8], prev: &[u8], bpp: usize) -> Result<(), RefError> {
    if fno > 4 {
        return Err(RefError::BadFilter);
    }
    let n = cur.len();
    let mut i = 0usize;
    while i < n {
        let a = if i >= bpp { cur[i - bpp] } else { 0 };
        let b = prev.get(i).copied().unwrap_or(0);
        let c = if i >= bpp { prev.get(i - bpp).copied().unwrap_or(0) } else { 0 };
        let x = cur[i];
        cur[i] = match fno {
            0 => x,
            1 => x.wrapping_sub(a),
            2 => x.wrapping_sub(b),
            3 => {
                let p = a as u32 + b as u32;
                x.wrapping_sub(((p - p / 2) as u8))
            }
            _ => x.wrapping_sub(paeth(a, b, c)),
        };
        i += 1;
    }
    Ok(())
}

/// 位深 <8 的取样（规范 §9.1，高位在左）。
fn take_bits(row: &[u8], index: u32, depth: u8) -> u8 {
    match depth {
        1 => (row[(index / 8) as usize] >> (7 - (index % 8))) & 1,
        2 => (row[(index / 4) as usize] >> (6 - 2 * (index % 4))) & 3,
        4 => (row[(index / 2) as usize] >> (4 - 4 * (index % 2))) & 15,
        _ => 0,
    }
}

/// 灰度按位深归一化到 0..255（规范 §13）。
fn scale_gray(v: u8, depth: u8) -> u8 {
    match depth {
        1 => {
            if v & 1 != 0 {
                255
            } else {
                0
            }
        }
        2 => ((v & 3) as u16 * 85) as u8,
        4 => ((v & 15) as u16 * 17) as u8,
        _ => v,
    }
}

/// 通道数（按颜色类型）。
fn channels(color_raw: u8) -> usize {
    match color_raw {
        0 | 3 => 1,
        2 => 3,
        4 => 2,
        _ => 4,
    }
}

/// 一行已反滤波的原始样本 → RGBA8（参考侧色彩展开，自带 tRNS 语义）。
///
/// **tRNS 语义按规范逐类型实现**：调色板 per-entry（缺省 255）、灰度/真彩
/// 色键按**完整样本**（16 位比 16 位）精确匹配——与被测侧展开层的取样口径
/// 是两套独立代码，哪一套错都会在对拍里现形。
fn expand_row(
    row: &[u8],
    width: u32,
    depth: u8,
    color_raw: u8,
    palette: &[u8],
    palette_len: usize,
    trns: Option<&Vec<u8>>,
    out: &mut [u8],
) -> Result<(), RefError> {
    let ch = channels(color_raw);
    let w = width as usize;
    let mut x = 0usize;
    while x < w {
        let base = if depth < 8 { 0 } else { x * ch * (depth as usize / 8) };
        let (r, g, b, a) = match color_raw {
            0 => {
                if depth == 16 {
                    let hi = row.get(base).copied().unwrap_or(0);
                    let lo = row.get(base + 1).copied().unwrap_or(0);
                    let full = ((hi as u16) << 8) | (lo as u16);
                    let key = match trns {
                        Some(t) if t.len() == 2 => Some(((t[0] as u16) << 8) | (t[1] as u16)),
                        _ => None,
                    };
                    (hi, hi, hi, if key == Some(full) { 0 } else { 255 })
                } else {
                    let raw8 = if depth == 8 {
                        row.get(base).copied().unwrap_or(0)
                    } else {
                        take_bits(row, x as u32, depth)
                    };
                    let v = scale_gray(raw8, depth);
                    let hit = match trns {
                        Some(t) if t.len() == 2 => {
                            let key = ((t[0] as u16) << 8) | (t[1] as u16);
                            let full = if depth == 8 {
                                raw8 as u16
                            } else {
                                (raw8 as u16) * 65535u16 / ((1u16 << depth) - 1u16)
                            };
                            full == key
                        }
                        _ => false,
                    };
                    (v, v, v, if hit { 0 } else { 255 })
                }
            }
            2 => {
                if depth == 16 {
                    let r0 = row.get(base).copied().unwrap_or(0);
                    let g0 = row.get(base + 2).copied().unwrap_or(0);
                    let b0 = row.get(base + 4).copied().unwrap_or(0);
                    let key_hit = match trns {
                        Some(t) if t.len() == 6 => {
                            let full_r = ((row.get(base).copied().unwrap_or(0) as u16) << 8)
                                | row.get(base + 1).copied().unwrap_or(0) as u16;
                            let full_g = ((row.get(base + 2).copied().unwrap_or(0) as u16) << 8)
                                | row.get(base + 3).copied().unwrap_or(0) as u16;
                            let full_b = ((row.get(base + 4).copied().unwrap_or(0) as u16) << 8)
                                | row.get(base + 5).copied().unwrap_or(0) as u16;
                            let kr = ((t[0] as u16) << 8) | (t[1] as u16);
                            let kg = ((t[2] as u16) << 8) | (t[3] as u16);
                            let kb = ((t[4] as u16) << 8) | (t[5] as u16);
                            full_r == kr && full_g == kg && full_b == kb
                        }
                        _ => false,
                    };
                    (r0, g0, b0, if key_hit { 0 } else { 255 })
                } else {
                    let r0 = row.get(base).copied().unwrap_or(0);
                    let g0 = row.get(base + 1).copied().unwrap_or(0);
                    let b0 = row.get(base + 2).copied().unwrap_or(0);
                    let key_hit = match trns {
                        Some(t) if t.len() == 6 => {
                            r0 == t[0] && g0 == t[1] && b0 == t[2]
                        }
                        _ => false,
                    };
                    (r0, g0, b0, if key_hit { 0 } else { 255 })
                }
            }
            3 => {
                let idx = if depth == 8 {
                    row.get(base).copied().unwrap_or(0) as usize
                } else {
                    take_bits(row, x as u32, depth) as usize
                };
                if idx >= palette_len {
                    return Err(RefError::Palette);
                }
                let o = idx * 3;
                let pr = palette.get(o).copied().unwrap_or(0);
                let pg = palette.get(o + 1).copied().unwrap_or(0);
                let pb = palette.get(o + 2).copied().unwrap_or(0);
                let pa = match trns {
                    Some(t) => t.get(idx).copied().unwrap_or(255),
                    None => 255,
                };
                (pr, pg, pb, pa)
            }
            4 => {
                if depth == 16 {
                    let g0 = row.get(base).copied().unwrap_or(0);
                    let a0 = row.get(base + 2).copied().unwrap_or(0);
                    (g0, g0, g0, a0)
                } else {
                    let g0 = row.get(base).copied().unwrap_or(0);
                    let a0 = row.get(base + 1).copied().unwrap_or(255);
                    (g0, g0, g0, a0)
                }
            }
            _ => {
                if depth == 16 {
                    (
                        row.get(base).copied().unwrap_or(0),
                        row.get(base + 2).copied().unwrap_or(0),
                        row.get(base + 4).copied().unwrap_or(0),
                        row.get(base + 6).copied().unwrap_or(0),
                    )
                } else {
                    (
                        row.get(base).copied().unwrap_or(0),
                        row.get(base + 1).copied().unwrap_or(0),
                        row.get(base + 2).copied().unwrap_or(0),
                        row.get(base + 3).copied().unwrap_or(255),
                    )
                }
            }
        };
        let o = x * 4;
        out[o] = r;
        out[o + 1] = g;
        out[o + 2] = b;
        out[o + 3] = a;
        x += 1;
    }
    Ok(())
}

/// 逐行流解码（非隔行）：`raw` 是反滤波前的扫描线流。
fn decode_rows_raw(
    raw: &[u8],
    c: &RefContainer,
    rgba: &mut [u8],
) -> Result<(), RefError> {
    let ch = channels(c.color_raw);
    let bpp = core::cmp::max(1, ch * (c.depth as usize / 8));
    let rb = ((c.width as usize) * ch * (c.depth as usize) + 7) / 8;
    let stride = rb + 1;
    let need = stride * (c.height as usize);
    if raw.len() < need {
        return Err(RefError::RowShort);
    }
    let mut prev = vec![0u8; rb];
    let mut cur = vec![0u8; rb];
    let mut y = 0u32;
    while y < c.height {
        let off = y as usize * stride;
        let fno = raw[off];
        let mut k = 0usize;
        while k < rb {
            cur[k] = raw[off + 1 + k];
            k += 1;
        }
        ref_unfilter(fno, &mut cur, &prev, bpp)?;
        expand_row(
            &cur,
            c.width,
            c.depth,
            c.color_raw,
            &c.palette,
            c.palette_len,
            c.trns.as_ref(),
            &mut rgba[y as usize * c.width as usize * 4..(y as usize + 1) * c.width as usize * 4],
        )?;
        prev.copy_from_slice(&cur);
        y += 1;
    }
    Ok(())
}

/// Adam7 反交织：`raw` 是七遍拼接的扫描线流，逐遍铺回整图。
fn decode_rows_interlaced(
    raw: &[u8],
    c: &RefContainer,
    rgba: &mut [u8],
) -> Result<(), RefError> {
    let ch = channels(c.color_raw);
    let bpp = core::cmp::max(1, ch * (c.depth as usize / 8));
    let mut off = 0usize;
    let mut pass = 0usize;
    while pass < 7 {
        let g = REF_ADAM7[pass];
        if c.width <= g.0 || c.height <= g.1 {
            pass += 1;
            continue;
        }
        let pw = (c.width - g.0 + g.2 - 1) / g.2;
        let ph = (c.height - g.1 + g.3 - 1) / g.3;
        if pw == 0 || ph == 0 {
            pass += 1;
            continue;
        }
        let rb = ((pw as usize) * ch * (c.depth as usize) + 7) / 8;
        let stride = rb + 1;
        if off + stride * (ph as usize) > raw.len() {
            return Err(RefError::RowShort);
        }
        let mut prev = vec![0u8; rb];
        let mut cur = vec![0u8; rb];
        let mut py = 0u32;
        while py < ph {
            let base = off + py as usize * stride;
            let fno = raw[base];
            let mut k = 0usize;
            while k < rb {
                cur[k] = raw[base + 1 + k];
                k += 1;
            }
            ref_unfilter(fno, &mut cur, &prev, bpp)?;
            // 遍内一行 → 整图散列位置
            let mut line = vec![0u8; pw as usize * 4];
            expand_row(
                &cur,
                pw,
                c.depth,
                c.color_raw,
                &c.palette,
                c.palette_len,
                c.trns.as_ref(),
                &mut line,
            )?;
            let mut px = 0u32;
            while px < pw {
                let gx = g.0 + px * g.2;
                let gy = g.1 + py * g.3;
                let dst = (gy as usize * c.width as usize + gx as usize) * 4;
                let src = px as usize * 4;
                if dst + 3 < rgba.len() {
                    rgba[dst] = line[src];
                    rgba[dst + 1] = line[src + 1];
                    rgba[dst + 2] = line[src + 2];
                    rgba[dst + 3] = line[src + 3];
                }
                px += 1;
            }
            prev.copy_from_slice(&cur);
            py += 1;
        }
        off += stride * ph as usize;
        pass += 1;
    }
    if off != raw.len() {
        // 遍后有多余字节：规范未定义，按扫描线不足处理（显性而非静默忽略）
        return Err(RefError::RowShort);
    }
    Ok(())
}

/// **参考解码入口**：PNG 字节流 → RGBA8（与被测侧完全独立的一条实现）。
///
/// 展开输出上限 = 按 IHDR 推导的原始字节数 + [`INFLATE_SLACK`](super::vef13_pngsuite::INFLATE_SLACK_MAX)
/// ——这是锚点 F1013「反序列化前 32KB 数据量」纪律在本单的口径：**展开输出
/// 不得无限增长**，一个 42 字节的 IDAT 不能膨胀成 512MB 缓冲。
pub fn ref_decode(file: &[u8]) -> Result<RefImage, RefError> {
    let c = ref_parse(file)?;
    let pixels = (c.width as usize) * (c.height as usize);
    let mut rgba = vec![0u8; pixels * 4];
    // 反滤波前的扫描线字节数（隔行按七遍之和）
    let ch = channels(c.color_raw);
    let row_bytes = (pixels * ch * (c.depth as usize) + 7) / 8 + c.height as usize;
    let slack = super::vef13_pngsuite::INFLATE_SLACK_MAX as usize;
    let raw = zlib_inflate(&c.idat, row_bytes + slack)?;
    if c.interlace == 1 {
        decode_rows_interlaced(&raw, &c, &mut rgba)?;
    } else {
        decode_rows_raw(&raw, &c, &mut rgba)?;
    }
    Ok(RefImage { width: c.width, height: c.height, rgba })
}
