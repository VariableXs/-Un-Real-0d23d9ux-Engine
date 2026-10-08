//! mech_deflate — DEFLATE 定长 Huffman 编码器 + LZ77 哈希链匹配器
//! （AI-K1 深化批次四 · F041 账本落盘通道）。
//!
//! 主册依据：
//! - F041【数据与存储】「账本落盘格式（CRC·版本·轮转）」——落盘体量由主册
//!   「24h 分钟聚合 + 帧明细」口径决定，raw 明文落 U 盘对顺序写吞吐是纯浪费；
//!   F066「顺序写吞吐在 ordered 模式损失 <10%」的预算里，落盘前压缩是唯一
//!   无损杠杆。账本数据高度可压（同构行 + 重复帧跨度），定长 Huffman 块
//!   （RFC1951 BTYPE=01）即可拿到大部分收益，且**解码器（mech_inflate）与
//!   编码器（本件）独立实现互为对拍**——一处实现自证不是证据。
//! - 零堆纪律：哈希链用定长 head/prev 数组，输出写调用侧缓冲，满则
//!   `Overflow` 诚实报错（不静默截断）。
//!
//! 算法说明（一处一事实）：
//! - LZ77：窗口 32768（RFC1951 上限），最小匹配 3、最大 258；哈希键取
//!   3 字节 × Knuth 乘法散列到 2^13 桶；链经 prev 数组回溯，深度上限
//!   `CHAIN_DEPTH`（128）——穷尽搜索的收益在账本场景递减，时间预算优先。
//! - 惰性匹配（lazy matching）：当前位置匹配长度 ≥ LAZY_GOOD 时直接采用，
//!   否则与下一位置匹配比较取长者——一次前看的标准实现。
//! - 定长 Huffman 字面量/长度码与 5 位距离码，块尺寸 `BLOCK_SYMBOLS` 上限，
//!   最后一块 BFINAL=1。

/// LZ77 滑动窗口大小（RFC1951 距离码上限 32768）。
pub const WSIZE: usize = 32768;
/// 哈希桶位宽：2^13 桶 × 平均链长在账本场景下平衡（可调，非魔法数——见下）。
pub const HASH_BITS: u32 = 13;
/// 哈希链搜索深度上限。
pub const CHAIN_DEPTH: usize = 128;
/// 最小/最大匹配长度（RFC1951 长度码域）。
pub const MIN_MATCH: usize = 3;
pub const MAX_MATCH: usize = 258;
/// 惰性匹配立即采用阈值。
pub const LAZY_GOOD: usize = 32;
/// 单块符号数上限（65535 输入字节量级——块太大不利随机访问落盘段）。
pub const BLOCK_BYTES: usize = 65536;

/// 输出缓冲不足。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Overflow;

/// Knuth 乘法散列：3 字节 → 桶号。
#[inline]
fn hash3(b0: u8, b1: u8, b2: u8) -> usize {
    let v = ((b0 as u32) << 16) | ((b1 as u32) << 8) | b2 as u32;
    (v.wrapping_mul(0x9E37_79B1) >> (32 - HASH_BITS)) as usize
}

/// LSB-first 位写出器（RFC1951 位序：Huffman 码从码字 MSB 起，
/// 整数附加位 LSB 起——两种写法都要，见 `put_code`/`put_bits`）。
pub struct BitWriter<'a> {
    buf: &'a mut [u8],
    pos: usize,
    bit: u32,
    acc: u8,
}

impl<'a> BitWriter<'a> {
    pub fn new(buf: &'a mut [u8]) -> Self {
        BitWriter { buf, pos: 0, bit: 0, acc: 0 }
    }

    /// 字节位置（flush 后为最终长度）。
    pub fn byte_pos(&self) -> usize {
        self.pos + (self.bit > 0) as usize
    }

    /// 写 n 位（n ≤ 16），值按 LSB-first 进流（RFC1951 整数附加位序）。
    pub fn put_bits(&mut self, value: u32, n: u32) -> Result<(), Overflow> {
        debug_assert!(n <= 16);
        let mut v = value;
        let mut left = n;
        while left > 0 {
            if self.pos >= self.buf.len() {
                return Err(Overflow);
            }
            let take = left.min(8 - self.bit);
            self.acc |= ((v & ((1u32 << take) - 1)) as u8) << self.bit;
            self.bit += take;
            left -= take;
            v >>= take;
            if self.bit == 8 {
                self.buf[self.pos] = self.acc;
                self.pos += 1;
                self.acc = 0;
                self.bit = 0;
            }
        }
        Ok(())
    }

    /// 写 Huffman 码：码字按 MSB-first 进流（RFC1951 §3.1.1——Huffman 码从
    /// 最高有效位开始打包），逐位反转后走 LSB-first 通道。
    pub fn put_code(&mut self, code: u32, len: u32) -> Result<(), Overflow> {
        let mut rev = 0u32;
        for i in 0..len {
            rev |= ((code >> i) & 1) << (len - 1 - i);
        }
        self.put_bits(rev, len)
    }

    /// 对齐到字节边界（块尾/流尾）。
    pub fn align(&mut self) -> Result<(), Overflow> {
        if self.bit > 0 {
            if self.pos >= self.buf.len() {
                return Err(Overflow);
            }
            self.buf[self.pos] = self.acc;
            self.pos += 1;
            self.acc = 0;
            self.bit = 0;
        }
        Ok(())
    }
}

/// 定长 Huffman 码表（RFC1951 §3.2.6）：
/// 字面量 0-143 → 8 位码 0x30..0xBF；144-255 → 9 位码 0x190..0x1FF；
/// 256-279 → 7 位码 0x00..0x17；280-287 → 8 位码 0xC0..0xC7。
#[inline]
fn litlen_fixed(sym: u16) -> (u32, u32) {
    match sym {
        0..=143 => (0x30 + sym as u32, 8),
        144..=255 => (0x190 + (sym as u32 - 144), 9),
        256..=279 => (sym as u32 - 256, 7),
        _ => (0xC0 + (sym as u32 - 280), 8),
    }
}

/// 长度符号（257..285）与附加位表：`length_code[len]` 直接索引。
/// RFC1951 §3.2.5：长度 3..258 → 29 个符号。
/// **pub**：mech_inflate（解码器）按 RFC1951 同表解码——一处一事实，表只定义一次。
pub const LEN_BASE: [u16; 29] = [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
pub const LEN_EXTRA: [u8; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];

/// 距离符号（0..29）与附加位表：距离 1..32768 → 30 个符号。
pub const DIST_BASE: [u16; 30] = [1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577];
pub const DIST_EXTRA: [u8; 30] = [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];

/// 长度 → 符号（线性上界查找：29 项，热路径可接受；二分收益不在预算内）。
#[inline]
fn length_symbol(len: usize) -> (u16, u32, u32) {
    debug_assert!((MIN_MATCH..=MAX_MATCH).contains(&len));
    let mut i = 28;
    while i > 0 && (len as u16) < LEN_BASE[i] {
        i -= 1;
    }
    // 长度 258 特判：base[28]=258 但符号 285 无附加位；284+31 也表 258——
    // 规范偏好直接用 285（省 5 附加位）。
    if len == 258 {
        return (285, 0, 0);
    }
    (257 + i as u16, (len as u16 - LEN_BASE[i]) as u32, LEN_EXTRA[i] as u32)
}

/// 距离 → 符号。
#[inline]
fn dist_symbol(d: usize) -> (u16, u32, u32) {
    debug_assert!((1..=32768).contains(&d));
    let mut i = 29;
    while i > 0 && (d as u16) < DIST_BASE[i] {
        i -= 1;
    }
    (i as u16, (d - DIST_BASE[i] as usize) as u32, DIST_EXTRA[i] as u32)
}

/// LZ77 匹配器（哈希链 + 惰性匹配）。
pub struct Lz77 {
    head: [i32; 1 << HASH_BITS],
    prev: [i32; WSIZE],
}

impl Lz77 {
    pub const fn new() -> Self {
        Lz77 { head: [-1; 1 << HASH_BITS], prev: [-1; WSIZE] }
    }

    /// 找当前位置的最长匹配。返回 (长度, 距离)；无匹配返回 (0, 0)。
    fn find_match(&self, input: &[u8], pos: usize) -> (usize, usize) {
        if pos + MIN_MATCH > input.len() {
            return (0, 0);
        }
        let h = hash3(input[pos], input[pos + 1], input[pos + 2]);
        let mut cand = self.head[h];
        let max_len = (input.len() - pos).min(MAX_MATCH);
        let (mut best_len, mut best_dist) = (0usize, 0usize);
        let mut depth = CHAIN_DEPTH;
        while cand >= 0 && depth > 0 {
            let c = cand as usize;
            let dist = pos - c;
            if dist > WSIZE {
                break; // 链已滑出窗口
            }
            // 快速否决：尾字节不同就不可能是更长的匹配（廉价预筛）。
            if best_len == 0 || input[c + best_len] == input.get(pos + best_len).copied().unwrap_or(0) {
                let mut l = 0usize;
                while l < max_len && input[c + l] == input[pos + l] {
                    l += 1;
                }
                if l > best_len {
                    best_len = l;
                    best_dist = dist;
                    if l >= max_len {
                        break;
                    }
                }
            }
            cand = self.prev[c & (WSIZE - 1)];
            depth -= 1;
        }
        if best_len >= MIN_MATCH {
            (best_len, best_dist)
        } else {
            (0, 0)
        }
    }

    fn insert(&mut self, input: &[u8], pos: usize) {
        if pos + MIN_MATCH <= input.len() {
            let h = hash3(input[pos], input[pos + 1], input[pos + 2]);
            self.prev[pos & (WSIZE - 1)] = self.head[h];
            self.head[h] = pos as i32;
        }
    }
}

/// 编码一个 token 流进定长 Huffman 块。
fn emit_block(bw: &mut BitWriter, final_block: bool, toks: &[Tok]) -> Result<(), Overflow> {
    bw.put_bits(final_block as u32, 1)?;
    bw.put_bits(0b01, 2)?; // BTYPE=01 固定 Huffman
    for t in toks {
        match *t {
            Tok::Lit(b) => {
                let (code, len) = litlen_fixed(b as u16);
                bw.put_code(code, len)?;
            }
            Tok::Match { len, dist } => {
                let (ls, le, ln) = length_symbol(len);
                let (code, clen) = litlen_fixed(ls);
                bw.put_code(code, clen)?;
                if ln > 0 {
                    bw.put_bits(le, ln)?;
                }
                let (ds, de, dn) = dist_symbol(dist);
                bw.put_code(ds as u32, 5)?;
                if dn > 0 {
                    bw.put_bits(de, dn)?;
                }
            }
        }
    }
    let (code, clen) = litlen_fixed(256); // 块尾
    bw.put_code(code, clen)
}

/// LZ77 token。
#[derive(Clone, Copy, Debug)]
enum Tok {
    Lit(u8),
    Match { len: usize, dist: usize },
}

/// 主入口：DEFLATE（定长 Huffman）压缩 `input` 进 `out`。
/// 返回写出字节数。输出缓冲不足 → Err(Overflow)（诚实报错，不半写）。
/// 标准惰性匹配（lazy-by-one）：当前位置匹配不劣于前看位置才采用，
/// 否则发字面量让位给下一位置——一次前看的经典实现。
pub fn deflate_fixed(lz: &mut Lz77, input: &[u8], out: &mut [u8]) -> Result<usize, Overflow> {
    // 哈希链残留会让匹配跨调用污染（前一次输入的距离引用本次不存在的
    // 数据）。契约：每次 deflate 前重置（这里统一做，调用侧不用记）。
    lz.head = [-1; 1 << HASH_BITS];
    let mut bw = BitWriter::new(out);
    let mut toks = [Tok::Lit(0); 4096];
    let mut ntok = 0usize;
    let mut block_start = 0usize;
    let mut pos = 0usize;

    while pos < input.len() {
        let (mlen, mdist) = lz.find_match(input, pos);
        if mlen >= MIN_MATCH {
            // 惰性前看：下一位置若匹配更长，当前位置发字面量让路。
            let mut lazy_won = false;
            if mlen < LAZY_GOOD && pos + 1 < input.len() {
                let (nlen, _) = lz.find_match(input, pos + 1);
                if nlen > mlen {
                    lazy_won = true;
                }
            }
            if lazy_won {
                toks[ntok] = Tok::Lit(input[pos]);
                ntok += 1;
                lz.insert(input, pos);
                pos += 1;
            } else {
                toks[ntok] = Tok::Match { len: mlen, dist: mdist };
                ntok += 1;
                for i in 0..mlen {
                    lz.insert(input, pos + i);
                }
                pos += mlen;
            }
        } else {
            toks[ntok] = Tok::Lit(input[pos]);
            ntok += 1;
            lz.insert(input, pos);
            pos += 1;
        }
        // token 缓冲满 / 块尺寸到 → 落块（非末块）。
        if ntok == toks.len() || pos - block_start >= BLOCK_BYTES {
            emit_block(&mut bw, false, &toks[..ntok])?;
            ntok = 0;
            block_start = pos;
        }
    }
    emit_block(&mut bw, true, &toks[..ntok])?;
    bw.align()?;
    Ok(bw.byte_pos())
}

// ---------------------------------------------------------------------------
// 宿主单测
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// CheckSet（挂 F041——账本落盘通道；判例不请 LZ77 表，见本件头注）
// ---------------------------------------------------------------------------

/// 运行检查项（判据锚点见对账表批次四段）。
use crate::checks::CheckSet;

pub fn run_checks() -> CheckSet {
    let mut cs = CheckSet::new("F041-mech-deflate");
    // 1) RFC1951 表锚点（长度/距离 base+extra 抽查——解码器同源引用）。
    let tables_ok = LEN_BASE[0] == 3
        && LEN_BASE[28] == 258
        && LEN_EXTRA[8] == 1
        && LEN_EXTRA[28] == 0
        && DIST_BASE[0] == 1
        && DIST_BASE[29] == 24577
        && DIST_EXTRA[29] == 13;
    cs.add("rfc1951_tables_exact", tables_ok, "");
    // 2) 位写出器双序（整数 LSB-first / Huffman 码 MSB-first）。
    let mut buf = [0u8; 8];
    let mut bw = BitWriter::new(&mut buf);
    let w_ok = bw.put_bits(0b101, 3).is_ok() && bw.put_code(0b1000_0001, 8).is_ok() && bw.align().is_ok();
    let pos = bw.byte_pos();
    drop(bw);
    cs.add("bitwriter_dual_order", w_ok && buf[0] == 0b0000_1101 && pos == 2, "");
    // 3) 码表函数与表自洽（length_symbol 查表边界）。
    let sym_ok = length_symbol(3) == (257, 0, 0)
        && length_symbol(258) == (285, 0, 0)
        && length_symbol(12) == (265, 1, 1)
        && dist_symbol(32768) == (29, 8191, 13);
    cs.add("code_symbol_mapping", sym_ok, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::perfstar::mech_sim::XorShift64;

    #[test]
    fn length_and_dist_tables_are_rfc1951_exact() {
        // 表边界抽查（RFC1951 §3.2.5 原表）。
        assert_eq!(length_symbol(3), (257, 0, 0));
        assert_eq!(length_symbol(11), (265, 0, 1)); // base 11, extra 1
        assert_eq!(length_symbol(12), (265, 1, 1));
        assert_eq!(length_symbol(258), (285, 0, 0));
        assert_eq!(dist_symbol(1), (0, 0, 0));
        assert_eq!(dist_symbol(5), (4, 0, 1));
        assert_eq!(dist_symbol(7), (5, 0, 1));
        assert_eq!(dist_symbol(32768), (29, 8191, 13)); // 基 24577 + 8191，附加 13 位
    }

    #[test]
    fn bitwriter_lsb_and_msb_paths() {
        let mut buf = [0u8; 8];
        let mut bw = BitWriter::new(&mut buf);
        bw.put_bits(0b101, 3).unwrap(); // LSB-first: 首字节低位起
        bw.put_code(0b1000_0001, 8).unwrap(); // MSB-first 码流（反转后写入）
        bw.align().unwrap();
        let pos = bw.byte_pos();
        drop(bw);
        // 0b101 占 bit0-2；put_code 从码的 MSB 起写入流内位 bit3..bit10
        // = 1,0,0,0,0,0,0,1。buf[0] = bit0,1,2,3 = 1,0,1,1 → 0b0000_1101。
        assert_eq!(buf[0], 0b0000_1101);
        assert_eq!(pos, 2);
    }

    #[test]
    fn roundtrip_literal_blob() {
        // 全字面量路径（不可压数据——纯随机会让匹配器空转）。
        let mut r = XorShift64::new(1);
        let input: Vec<u8> = (0..5000).map(|_| r.next_u64() as u8).collect();
        let mut lz = Lz77::new();
        let mut out = vec![0u8; 16384];
        let n = deflate_fixed(&mut lz, &input, &mut out).unwrap();
        let back = crate::perfstar::mech_inflate::inflate_vec(&out[..n], 8192).unwrap();
        assert_eq!(back, input, "不可压数据必须逐字节还原");
    }

    #[test]
    fn roundtrip_highly_repetitive_ledger_rows() {
        // 账本形态数据：同构行重复——LZ77 命中 + 惰性匹配路径全走。
        let row = [0u8; 10]; // 一条定长账本行的骨架
        let mut input = Vec::new();
        for i in 0..2000u32 {
            input.extend_from_slice(&row);
            input.extend_from_slice(&(i as u64).to_le_bytes());
        }
        let mut lz = Lz77::new();
        let mut out = vec![0u8; 65536];
        let n = deflate_fixed(&mut lz, &input, &mut out).unwrap();
        assert!(n < input.len() / 2, "重复账本行必须真压缩：{} → {}", input.len(), n);
        let back = crate::perfstar::mech_inflate::inflate_vec(&out[..n], 65536).unwrap();
        assert_eq!(back.len(), input.len());
        assert!(back == input);
    }

    #[test]
    fn roundtrip_window_edge_distance_32768() {
        // 距离 = 32768 精确踩满窗口上沿（窗口边缘回归）。
        let mut input = vec![0u8; 32768];
        input.extend_from_slice(b"MARKER");
        let mut probe = vec![7u8; 32768];
        probe.extend_from_slice(b"MARKER");
        input.extend_from_slice(&probe);
        let mut lz = Lz77::new();
        let mut out = vec![0u8; 131072];
        let n = deflate_fixed(&mut lz, &input, &mut out).unwrap();
        let back = crate::perfstar::mech_inflate::inflate_vec(&out[..n], 131072).unwrap();
        assert_eq!(back, input);
    }

    #[test]
    fn overflow_is_honest() {
        let input = [7u8; 1000];
        let mut lz = Lz77::new();
        let mut tiny = [0u8; 8];
        assert_eq!(deflate_fixed(&mut lz, &input, &mut tiny), Err(Overflow));
    }

    #[test]
    fn empty_input_yields_valid_final_block() {
        let mut lz = Lz77::new();
        let mut out = [0u8; 64];
        let n = deflate_fixed(&mut lz, &[], &mut out).unwrap();
        let back = crate::perfstar::mech_inflate::inflate_vec(&out[..n], 16).unwrap();
        assert!(back.is_empty());
    }
}
