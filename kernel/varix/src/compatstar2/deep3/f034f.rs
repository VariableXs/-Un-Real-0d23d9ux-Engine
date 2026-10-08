//! F034 深化批次四 · 流式转码状态面（compatstar2/deep3 · G-A-34）。
//!
//! 批次一/二/三已覆盖 F034 的码页翻译面、执行治理面与双字节引擎面；本批
//! 补主册【功能定义】「全语义对齐」的序列化/账本/容错面：跨块多字节状态
//! 机（字节流逐字节喂入：GBK 双字节 lead 拆块续接——lead 在块尾、次字节
//! 在下一块首的正确续接；定长 pending 缓冲 4 字节）、round-trip 随机账
//! （N 个码点编码→解码往返，不等计数）、无损/有损双模（有损模式 U+FFFD
//! 替换计损、无损模式不可映射显性 Err）、块边界统计（跨块续接发生次数/
//! 拆散多字节字符计数两账）。
//!
//! 判据对账：深化以主册【状态与异常】「转换不可逆字符 → U+FFFD + 日志
//! （乱码显式可见不静默吞）」未落地面为源，一处一事实（GBK 双字节区间
//! 规范：lead 0x81-0xFE、trailer 0x40-0xFE 除 0x7F；Unicode REPLACEMENT
//! CHARACTER U+FFFD 语义对拍）。
//!
//! 零堆纪律：定长 pending 缓冲/稠密表/输出流，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量与 GBK 稠密样本表（GB2312 汉字区锚点，判例对拍用）
// ---------------------------------------------------------------------------

/// 有损替换码点（Unicode REPLACEMENT CHARACTER）。
pub const REPLACEMENT: u32 = 0xFFFD;
/// pending 缓冲容量（定长 4 字节）。
pub const PENDING_CAP: usize = 4;
/// 解码输出流容量。
pub const EMIT_CAP: usize = 64;
/// round-trip 抽样码点数。
pub const ROUND_N: usize = 24;

/// GBK 稠密样本表：8 个常用字（码位 = (0xA0+区)<<8 | 0xA0+位）。
pub const GBK_TABLE: [(u16, u32); 8] = [
    (0xD6D0, 0x4E2D), (0xCEC4, 0x6587), (0xB9FA, 0x56FD), (0xC8CB, 0x4EBA), // 中 文 国 人
    (0xBAC3, 0x597D), (0xC4E3, 0x4F60), (0xB1E0, 0x7F16), (0xC2EB, 0x7801), // 好 你 编 码
];

/// GBK → Unicode：ASCII 直通 + 稠密表查表；未登记 → None。
pub fn gbk_to_unicode(code: u16) -> Option<u32> {
    if code < 0x80 { return Some(code as u32); }
    for &(c, u) in GBK_TABLE.iter() {
        if c == code { return Some(u); }
    }
    None
}

/// Unicode → GBK（编码方向，round-trip 用）。
pub fn unicode_to_gbk(cp: u32) -> Option<u16> {
    if cp < 0x80 { return Some(cp as u16); }
    for &(c, u) in GBK_TABLE.iter() {
        if u == cp { return Some(c); }
    }
    None
}

/// 编码一个码点入 out，返回字节数；不可映射 → None。
pub fn encode_cp(cp: u32, out: &mut [u8; 4]) -> Option<usize> {
    match unicode_to_gbk(cp) {
        Some(c) if c < 0x80 => {
            out[0] = c as u8;
            Some(1)
        }
        Some(c) => {
            out[0] = (c >> 8) as u8;
            out[1] = (c & 0xFF) as u8;
            Some(2)
        }
        None => None,
    }
}

// ---------------------------------------------------------------------------
// 跨块多字节状态机（定长 pending，lead 拆块续接）
// ---------------------------------------------------------------------------

/// 流式 GBK → Unicode 解码器：逐字节状态机 + 块边界统计两账。
pub struct StreamDecoder {
    pending: [u8; PENDING_CAP],
    pending_n: usize,
    /// 已产出字符数（含 U+FFFD 替换）。
    pub chars: u32,
    /// 有损替换计数（每个 U+FFFD 计 1）。
    pub lossy: u32,
    /// 跨块续接发生次数（块首承接上块 lead 的续接事件）。
    pub continuations: u32,
    /// 被块边界拆散后完成的多字节字符计数。
    pub split_chars: u32,
    /// 解码输出流（round-trip 对拍用）。
    pub emitted: [u32; EMIT_CAP],
    pub emitted_n: usize,
    /// 输出缓冲溢出丢弃计数（零静默）。
    pub emit_dropped: u32,
}

impl StreamDecoder {
    pub const fn new() -> Self {
        StreamDecoder { pending: [0; PENDING_CAP], pending_n: 0, chars: 0, lossy: 0, continuations: 0, split_chars: 0, emitted: [0; EMIT_CAP], emitted_n: 0, emit_dropped: 0 }
    }
    fn emit(&mut self, cp: u32) {
        if self.emitted_n < EMIT_CAP { self.emitted[self.emitted_n] = cp; self.emitted_n += 1; }
        else { self.emit_dropped += 1; }
        self.chars += 1;
    }
    /// pending 待续字节数（块边界诊断用）。
    pub fn pending_len(&self) -> usize { self.pending_n }
    /// 逐字节喂入一块。lossless=true：不可映射/非法序列 → 显性 Err；
    /// false：U+FFFD 计损续行。非法次字节时该字节不吞、按新字节重扫。
    pub fn feed(&mut self, chunk: &[u8], lossless: bool) -> Result<u32, &'static str> {
        let mut out = 0u32;
        let mut carried = self.pending_n > 0;
        let mut i = 0usize;
        while i < chunk.len() {
            let b = chunk[i];
            if self.pending_n == 0 {
                if b < 0x80 {
                    self.emit(b as u32);
                    out += 1;
                } else if (0x81..=0xFE).contains(&b) {
                    self.pending[0] = b;
                    self.pending_n = 1;
                } else {
                    if lossless { return Err("unmappable-byte"); }
                    self.emit(REPLACEMENT);
                    self.lossy += 1;
                    out += 1;
                }
            } else {
                let lead = self.pending[0];
                let valid_trailer = (0x40..=0xFE).contains(&b) && b != 0x7F;
                let was_carried = carried;
                self.pending_n = 0;
                carried = false;
                if valid_trailer {
                    match gbk_to_unicode(((lead as u16) << 8) | b as u16) {
                        Some(cp) => {
                            self.emit(cp);
                            out += 1;
                            if was_carried {
                                self.continuations += 1;
                                self.split_chars += 1;
                            }
                        }
                        None => {
                            if lossless { return Err("unmappable-pair"); }
                            self.emit(REPLACEMENT);
                            self.lossy += 1;
                            out += 1;
                        }
                    }
                } else {
                    // 非法次字节：lead 计损；b 不吞——下一轮按新字节重扫。
                    if lossless { return Err("unmappable-pair"); }
                    self.emit(REPLACEMENT);
                    self.lossy += 1;
                    out += 1;
                    continue;
                }
            }
            i += 1;
        }
        Ok(out)
    }
    /// 流收尾：悬空 lead → 有损记 1 损 / 无损显性 Err。
    pub fn finish(&mut self, lossless: bool) -> Result<u32, &'static str> {
        if self.pending_n == 0 { return Ok(0); }
        self.pending_n = 0;
        if lossless { return Err("dangling-lead"); }
        self.emit(REPLACEMENT);
        self.lossy += 1;
        Ok(1)
    }
}

/// 域自检（深化批次四）。
pub fn run_f034f_checks() -> CheckSet {
    let mut cs = CheckSet::new("F034-stream-transcode-d4");
    // 1) ASCII 直通：8 字节 → 8 字符零损耗。
    let mut d1 = StreamDecoder::new();
    let fed = d1.feed(b"ABCxyz01", true);
    cs.add("ascii_passthrough", fed == Ok(8) && d1.emitted[0] == 0x41 && d1.emitted_n == 8 && d1.lossy == 0, "");
    // 2) GBK 双字节整块解码：D6D0 → 中。
    let mut d2 = StreamDecoder::new();
    let got = d2.feed(&[0xD6, 0xD0], true);
    cs.add("gbk_pair_decode", got == Ok(1) && d2.emitted[0] == 0x4E2D && d2.chars == 1, "");
    // 3) lead 在块尾、次字节在下一块首 → 正确续接 + 两账各计 1。
    let mut d3 = StreamDecoder::new();
    let _ = d3.feed(&[0xD6], true);
    let held = d3.pending_len() == 1;
    let done = d3.feed(&[0xD0], true);
    cs.add("cross_chunk_continuation", held && done == Ok(1) && d3.emitted[0] == 0x4E2D && d3.split_chars == 1 && d3.continuations == 1, "");
    // 4) pending 跨空块存活：lead → 空块 → 次字节仍续接。
    let mut d4 = StreamDecoder::new();
    let _ = d4.feed(&[0xCE], true);
    let _ = d4.feed(&[], true);
    let done4 = d4.feed(&[0xC4], true);
    cs.add("pending_survives_empty_chunks", done4 == Ok(1) && d4.emitted[0] == 0x6587 && d4.split_chars == 1, "");
    // 5) LCG 抽样 24 码点、单字节切块 round-trip：零不等且拆散账独立复算一致。
    let (m1, s1) = round_trip(7, 1);
    let mut state = 7u32 | 1;
    let mut cjk = 0u32;
    for _ in 0..ROUND_N {
        state = state.wrapping_mul(1664525).wrapping_add(1013904223);
        if SUPPORTED[((state >> 16) as usize) % SUPPORTED.len()] >= 0x80 {
            cjk += 1;
        }
    }
    cs.add("round_trip_chunked_lossless", m1 == 0 && s1 == cjk && cjk > 0, "");
    // 6) 多字节切块（4 字节/块）同样零不等。
    let (m2, _) = round_trip(0xBEEF, 4);
    cs.add("round_trip_multi_chunk", m2 == 0, "");
    // 7) 有损模式：0xFF 超出 lead 区间 → U+FFFD 计损，后续 ASCII 直通。
    let mut d5 = StreamDecoder::new();
    let r5 = d5.feed(&[0xFF, 0x41], false);
    cs.add("lossy_replacement_counted", r5 == Ok(2) && d5.lossy == 1 && d5.emitted[0] == REPLACEMENT && d5.emitted[1] == 0x41, "");
    // 8) 无损模式：不可映射字节显性 Err。
    let mut d6 = StreamDecoder::new();
    cs.add("lossless_explicit_err", d6.feed(&[0xFF], true) == Err("unmappable-byte"), "");
    // 9) 非法次字节（0x20）：lead 计损、字节重扫按 ASCII 直通。
    let mut d7 = StreamDecoder::new();
    let r7 = d7.feed(&[0xD6, 0x20], false);
    cs.add("invalid_trailer_reprocess", r7 == Ok(2) && d7.lossy == 1 && d7.emitted[0] == REPLACEMENT && d7.emitted[1] == 0x20, "");
    // 10) 双模对拍：未登记码对无损 Err / 有损计损。
    let mut d8 = StreamDecoder::new();
    let mut d9 = StreamDecoder::new();
    let r8 = d8.feed(&[0x81, 0x40], false);
    cs.add("unmapped_pair_dual_mode", d9.feed(&[0x81, 0x40], true) == Err("unmappable-pair") && r8 == Ok(1) && d8.lossy == 1 && d8.emitted[0] == REPLACEMENT, "");
    // 11) 悬空 lead 收尾：有损记 1 损 / 无损显性 Err。
    let mut da = StreamDecoder::new();
    let _ = da.feed(&[0xB9], false);
    let fin = da.finish(false);
    let mut db = StreamDecoder::new();
    let _ = db.feed(&[0xB9], false);
    cs.add("dangling_lead_finish", fin == Ok(1) && da.lossy == 1 && db.finish(true) == Err("dangling-lead"), "");
    cs
}

// ---------------------------------------------------------------------------
// round-trip 随机账（LCG 确定性抽样 → 编码 → 分块解码 → 逐码点对拍）
// ---------------------------------------------------------------------------

/// round-trip 抽样支撑集（ASCII + GBK 表锚点字）。
const SUPPORTED: [u32; 16] = [0x41, 0x7E, 0x30, 0x61, 0x4E2D, 0x6587, 0x56FD, 0x4EBA, 0x597D, 0x7F16, 0x7801, 0x4F60, 0x62, 0x63, 0x64, 0x65];

/// round-trip 账：LCG 抽样 ROUND_N 个码点 → 编码 → 按 chunk_size 分块
/// 无损喂入 → 逐码点对拍。返回（不等计数, 拆散字符数）。
pub fn round_trip(seed: u32, chunk_size: usize) -> (u32, u32) {
    let mut state = seed | 1;
    let mut cps = [0u32; ROUND_N];
    for i in 0..ROUND_N {
        state = state.wrapping_mul(1664525).wrapping_add(1013904223);
        cps[i] = SUPPORTED[((state >> 16) as usize) % SUPPORTED.len()];
    }
    let mut bytes = [0u8; 2 * ROUND_N];
    let mut blen = 0usize;
    for i in 0..ROUND_N {
        let mut buf = [0u8; 4];
        match encode_cp(cps[i], &mut buf) {
            Some(n) => {
                for k in 0..n {
                    bytes[blen] = buf[k];
                    blen += 1;
                }
            }
            None => return (u32::MAX, 0), // 支撑集内编码失败 = 账面崩坏，显性满损
        }
    }
    let mut dec = StreamDecoder::new();
    let step = if chunk_size == 0 { blen } else { chunk_size };
    let mut off = 0usize;
    while off < blen {
        let end = if off + step < blen { off + step } else { blen };
        if dec.feed(&bytes[off..end], true).is_err() {
            return (u32::MAX, dec.split_chars);
        }
        off = end;
    }
    let mut mismatches = 0u32;
    for i in 0..ROUND_N {
        if dec.emitted[i] != cps[i] {
            mismatches += 1;
        }
    }
    (mismatches, dec.split_chars)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn chunk_boundary_never_corrupts() {
        // 同一字节流按 1/2/3/5/8 字节切块，解码输出必须逐码点一致。
        let src: [u32; 6] = [0x4F60, 0x597D, 0x41, 0x7F16, 0x7801, 0x42];
        let mut bytes = [0u8; 12];
        let mut blen = 0usize;
        for &cp in src.iter() {
            let mut buf = [0u8; 4];
            let n = encode_cp(cp, &mut buf).expect("判例码点必可编码");
            for k in 0..n {
                bytes[blen] = buf[k];
                blen += 1;
            }
        }
        let mut whole = StreamDecoder::new();
        whole.feed(&bytes[..blen], true).expect("无损模式必成");
        for chunk in [1usize, 2, 3, 5, 8] {
            let mut dec = StreamDecoder::new();
            let mut off = 0usize;
            while off < blen {
                let end = if off + chunk < blen { off + chunk } else { blen };
                dec.feed(&bytes[off..end], true).expect("无损模式必成");
                off = end;
            }
            for i in 0..src.len() {
                assert_eq!(dec.emitted[i], whole.emitted[i], "切块 {} 下第 {} 码点必须一致", chunk, i);
            }
            assert_eq!(dec.chars, whole.chars);
        }
    }
    #[test]
    fn dual_mode_loss_accounting() {
        let mut lossy = StreamDecoder::new();
        assert_eq!(lossy.feed(&[0xD6, 0x7F, 0xFF], false), Ok(3));
        assert_eq!(lossy.lossy, 2, "非法次字节的 lead 一损 + 0xFF 一损");
        assert_eq!(lossy.chars, 3, "0x7F 重扫按 ASCII 直通");
        let mut strict = StreamDecoder::new();
        assert_eq!(strict.feed(&[0xD6, 0x7F], true), Err("unmappable-pair"));
        assert_eq!(strict.feed(&[0xFF], true), Err("unmappable-byte"));
    }
    #[test]
    fn deep4_checks_all_green() {
        let cs = run_f034f_checks();
        assert!(cs.all_passed() && !cs.truncated());
    }
}
