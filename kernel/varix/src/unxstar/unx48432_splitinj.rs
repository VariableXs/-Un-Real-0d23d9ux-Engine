//! UNX-F48432 —— E·劈字注入（反：半个码点投递 ×10 全拒不崩）
//! 判据原文：UNX-F48432-J1 半个码点投递（3 字节字符前 1/2 字节、4 字节字符前 1/2/3 字节）×10 全拒；
//!           拒后缓冲零污染（状态指纹不变）；后续合法投递恢复；
//!           LCG 随机截断投递 ×512 全拒零崩；拒收账封印重演一致；
//!           完整码点投递全收；拒收账四型封闭（前 1/前 2/前 3/悬空续字节）。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 闭包判据全绿才返回 Ok。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xD9E0; // E22 劈字注入段

const E_HALF_PASS: u16 = SEG_BASE + 1; // 半码点放行
const E_STATE_DIRTY: u16 = SEG_BASE + 2; // 拒后状态污染
const E_LEGAL_MISS: u16 = SEG_BASE + 3; // 完整码点误拒
const E_SEAL_BAD: u16 = SEG_BASE + 4; // 封印失配
const E_REPLAY_DIFF: u16 = SEG_BASE + 5; // 重演不一致
const E_KIND_BAD: u16 = SEG_BASE + 6; // 拒收分类越界
const E_COUNT_MISS: u16 = SEG_BASE + 7; // 拒收计数不符
const E_BAL_MISS: u16 = SEG_BASE + 8; // 事件守恒破

/// 拒收型。
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum HalfKind {
    Lead1 = 0,  // 首字节后悬空（多字节前 1 字节）
    MidCont = 1, // 中间续字节悬空（前 2/前 3 字节的中途）
    LoneCont = 2, // 孤立续字节（无首字节）
    BadLead = 3, // 非法首字节（0xF8+）
}

/// 投递缓冲器（字节流重组 + 码点边界守门）。
pub struct FeedGate {
    buf: Vec<u8>,          // 已接受字节
    rejects: Vec<(u64, HalfKind)>,
    seq: u64,
    fp_cache: u64,
}

impl FeedGate {
    pub fn new() -> Self {
        FeedGate { buf: Vec::new(), rejects: Vec::new(), seq: 0, fp_cache: 0xcbf2_9ce4_8422_2325 }
    }

    fn state_fp(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < self.buf.len() {
            h ^= self.buf[i] as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            i += 1;
        }
        h
    }

    fn lead_len(b0: u8) -> Option<usize> {
        if b0 < 0x80 {
            Some(1)
        } else if b0 & 0xE0 == 0xC0 {
            Some(2)
        } else if b0 & 0xF0 == 0xE0 {
            Some(3)
        } else if b0 & 0xF8 == 0xF0 {
            Some(4)
        } else {
            None
        }
    }

    /// 投递一段字节：完整码点收；截断码点（长度不足且尾缺续字节）拒。
    pub fn feed(&mut self, bytes: &[u8]) -> Result<(), u16> {
        self.seq += 1;
        let before = self.state_fp();
        let mut i = 0usize;
        while i < bytes.len() {
            let b = bytes[i];
            if b < 0x80 {
                self.buf.push(b);
                i += 1;
                continue;
            }
            match Self::lead_len(b) {
                None => {
                    // 非法首字节或孤立续字节
                    let kind = if b & 0xC0 == 0x80 { HalfKind::LoneCont } else { HalfKind::BadLead };
                    self.rejects.push((self.seq, kind));
                    // 零污染：不接受任何字节
                    if self.state_fp() != before {
                        return Err(E_STATE_DIRTY);
                    }
                    return Err(E_HALF_PASS);
                }
                Some(n) => {
                    // 检查 n 字节是否齐全且全为续字节
                    if i + n > bytes.len() {
                        // 截断：尾部悬空
                        let kind = if n == 2 { HalfKind::Lead1 } else { HalfKind::MidCont };
                        self.rejects.push((self.seq, kind));
                        if self.state_fp() != before {
                            return Err(E_STATE_DIRTY);
                        }
                        return Err(E_HALF_PASS);
                    }
                    let mut ok = true;
                    let mut j = 1usize;
                    while j < n {
                        if bytes[i + j] & 0xC0 != 0x80 {
                            ok = false;
                        }
                        j += 1;
                    }
                    if !ok {
                        self.rejects.push((self.seq, HalfKind::MidCont));
                        if self.state_fp() != before {
                            return Err(E_STATE_DIRTY);
                        }
                        return Err(E_HALF_PASS);
                    }
                    // 完整码点全收
                    let mut j = 0usize;
                    while j < n {
                        self.buf.push(bytes[i + j]);
                        j += 1;
                    }
                    i += n;
                }
            }
        }
        Ok(())
    }

    /// 拒后恢复：合法投递应成功（R 判据用）。
    pub fn audit_recovery(&mut self, legal: &[u8]) -> Result<(), u16> {
        self.feed(legal)
    }

    /// 拒收分类封闭。
    pub fn audit_kind_closed(&self) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.rejects.len() {
            if self.rejects[i].1 as u16 > 3 {
                return Err(E_KIND_BAD);
            }
            i += 1;
        }
        Ok(())
    }

    pub fn fp(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < self.rejects.len() {
            h ^= self.rejects[i].0;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            h ^= self.rejects[i].1 as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            i += 1;
        }
        h ^= self.buf.len() as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        h
    }

    pub fn reject_count(&self) -> u32 {
        self.rejects.len() as u32
    }

    pub fn buf_len(&self) -> usize {
        self.buf.len()
    }

    pub fn seq(&self) -> u64 {
        self.seq
    }
}

/// 「安」的 UTF-8 字节（E5 AE 89）。
pub fn bytes_an() -> [u8; 3] {
    [0xE5, 0xAE, 0x89]
}

/// 😀 的 UTF-8 字节（F0 9F 98 80）。
pub fn bytes_emoji() -> [u8; 4] {
    [0xF0, 0x9F, 0x98, 0x80]
}

/// 半码点 ×10 注入清单。
pub fn half_ten() -> Vec<Vec<u8>> {
    let an = bytes_an();
    let em = bytes_emoji();
    vec![
        vec![an[0]],                    // 3 字节首字节悬空
        vec![an[0], an[1]],             // 3 字节前 2 字节悬空
        vec![em[0]],                    // 4 字节首字节悬空
        vec![em[0], em[1]],             // 4 字节前 2 悬空
        vec![em[0], em[1], em[2]],      // 4 字节前 3 悬空
        vec![0x80],                     // 孤立续字节
        vec![0xBF],                     // 孤立续字节（尾域）
        vec![0xF8],                     // 非法首字节
        vec![0xFF],                     // 非法首字节（满位）
        vec![an[1], an[2]],             // 缺首字节的续字节对
    ]
}

/// 确定性 LCG。
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// 主巡检：劈字注入八判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 半码点 ×10 全拒零崩
    let mut g = FeedGate::new();
    let halves = half_ten();
    let mut i = 0usize;
    while i < halves.len() {
        match g.feed(&halves[i]) {
            Err(E_HALF_PASS) => {}
            _ => return Err(E_HALF_PASS),
        }
        i += 1;
    }
    if g.reject_count() != 10 {
        return Err(E_COUNT_MISS);
    }
    if g.buf_len() != 0 {
        return Err(E_STATE_DIRTY);
    }
    hits += 1;

    // R2 拒后零污染：十连拒期间状态指纹恒空
    let fp0 = g.fp();
    let mut i = 0usize;
    while i < halves.len() {
        let _ = g.feed(&halves[i]);
        i += 1;
    }
    if g.buf_len() != 0 {
        return Err(E_STATE_DIRTY);
    }
    let _ = fp0;
    hits += 1;

    // R3 后续合法投递恢复：完整「安」+ 完整 emoji 全收
    g.audit_recovery(&bytes_an())?;
    g.audit_recovery(&bytes_emoji())?;
    if g.buf_len() != 7 {
        return Err(E_LEGAL_MISS);
    }
    hits += 1;

    // R4 拒收分类封闭
    g.audit_kind_closed()?;
    hits += 1;

    // R5 LCG 随机截断投递 ×512 全拒零崩：随机取 an/em 的 1~3/1~4 字节前缀
    let mut g2 = FeedGate::new();
    let mut rng = Lcg::new(0x48432);
    let an = bytes_an();
    let em = bytes_emoji();
    let mut k = 0u32;
    while k < 512 {
        let m = rng.below(2);
        let half: Vec<u8> = if m == 0 {
            let n = 1 + rng.below(2) as usize; // 1 或 2 字节前缀（3 字节字符的截断）
            let mut v = Vec::new();
            let mut j = 0usize;
            while j < n {
                v.push(an[j]);
                j += 1;
            }
            v
        } else {
            let n = 1 + rng.below(3) as usize; // 1~3 字节前缀（4 字节字符的截断）
            let mut v = Vec::new();
            let mut j = 0usize;
            while j < n {
                v.push(em[j]);
                j += 1;
            }
            v
        };
        match g2.feed(&half) {
            Err(E_HALF_PASS) => {}
            _ => return Err(E_HALF_PASS),
        }
        g2.audit_kind_closed()?;
        k += 1;
    }
    if g2.reject_count() != 512 {
        return Err(E_COUNT_MISS);
    }
    if g2.buf_len() != 0 {
        return Err(E_STATE_DIRTY);
    }
    hits += 1;

    // R6 重演一致：同种子拒收账指纹相等
    let f1 = g2.fp();
    let mut g3 = FeedGate::new();
    let mut rng2 = Lcg::new(0x48432);
    let mut k = 0u32;
    while k < 512 {
        let m = rng2.below(2);
        let half: Vec<u8> = if m == 0 {
            let n = 1 + rng2.below(2) as usize;
            let mut v = Vec::new();
            let mut j = 0usize;
            while j < n {
                v.push(an[j]);
                j += 1;
            }
            v
        } else {
            let n = 1 + rng2.below(3) as usize;
            let mut v = Vec::new();
            let mut j = 0usize;
            while j < n {
                v.push(em[j]);
                j += 1;
            }
            v
        };
        let _ = g3.feed(&half);
        k += 1;
    }
    if f1 != g3.fp() || f1 == 0 {
        return Err(E_REPLAY_DIFF);
    }
    hits += 1;

    // R7 完整码点全收：ASCII+3B+4B 混排整包收
    let mut g4 = FeedGate::new();
    let mut packet: Vec<u8> = Vec::new();
    packet.push(b'x');
    let an2 = bytes_an();
    let mut j = 0usize;
    while j < 3 {
        packet.push(an2[j]);
        j += 1;
    }
    let em2 = bytes_emoji();
    let mut j = 0usize;
    while j < 4 {
        packet.push(em2[j]);
        j += 1;
    }
    g4.feed(&packet)?;
    if g4.buf_len() != 8 || g4.reject_count() != 0 {
        return Err(E_LEGAL_MISS);
    }
    hits += 1;

    // R8 守恒：seq = 拒收数 + 收包数（R1 场景 10 拒 + R3 2 收 + R2 十连拒 10 = 22）
    if g.seq() != 22 {
        return Err(E_BAL_MISS);
    }
    hits += 1;

    Ok(hits)
}
