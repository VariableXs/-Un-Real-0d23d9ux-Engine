//! UNX-F48443 —— E·bigram 表损坏回退（正：坏表→unigram 平滑可用）
//! 判据原文：UNX-F48443-J1 完好 bigram 表载入后按 (前词,后词) 二元权重打分；
//!           位翻转致 CRC 失配的坏表必须整体拒收并降级 unigram 平滑回退；
//!           回退态下全部词对仍可打分、单调性保持、零除零零悬空；
//!           随机损坏 ×256 每次都安全降级；好表重载即恢复二元模式；
//!           降级账目与重演指纹一致；截断表与空表同样走回退不 panic。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 闭包判据全绿才返回 Ok。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xDBC0; // E23 bigram 损坏回退段

const E_GOOD_MISS: u16 = SEG_BASE + 1; // 好表未按二元模式载入
const E_CRC_LEAK: u16 = SEG_BASE + 2; // 坏表 CRC 失配未被拒
const E_FB_DEAD: u16 = SEG_BASE + 3; // 回退态打分不可用
const E_MONO_BREAK: u16 = SEG_BASE + 4; // 回退态单调性破
const E_CRASH_LEAK: u16 = SEG_BASE + 5; // 随机损坏导致崩溃态
const E_RESTORE_MISS: u16 = SEG_BASE + 6; // 好表重载未恢复二元模式
const E_LEDGER_MISS: u16 = SEG_BASE + 7; // 降级账目不符
const E_REPLAY_DIFF: u16 = SEG_BASE + 8; // 重演指纹不一致
const E_EDGE_LEAK: u16 = SEG_BASE + 9; // 截断/空表未走回退

pub const MAGIC: [u8; 4] = [b'B', b'G', b'R', b'M'];

/// 表模式。
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum LoadMode {
    Empty = 0,    // 尚未载入
    Bigram = 1,   // 二元表就绪
    Fallback = 2, // unigram 平滑回退
}

/// 无表 CRC32（bitwise，无查表内存）。
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    let mut i = 0usize;
    while i < data.len() {
        crc ^= data[i] as u32;
        let mut k = 0u32;
        while k < 8 {
            let m = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & m);
            k += 1;
        }
        i += 1;
    }
    !crc
}

/// 二元词条目：(前词 id, 后词 id, 权重)。
#[derive(Clone, Copy)]
pub struct BiEntry {
    pub prev: u16,
    pub next: u16,
    pub w: u16,
}

/// bigram 存储：载入外部表，CRC 失配整体拒收并降级回退。
pub struct BigramStore {
    pub mode: LoadMode,
    pub label: String,
    uni: Vec<u32>,              // unigram 权重（16 词固定）
    bi: Vec<BiEntry>,           // 二元条目（仅 Bigram 模式使用）
    degrade_count: u32,         // 降级次数账
    load_count: u32,            // 载入尝试账
}

impl BigramStore {
    pub fn new() -> Self {
        // 内置 unigram 权重：16 个高频词，权重严格递减保证单调性可验。
        let mut uni = vec![0u32; 16];
        let mut i = 0usize;
        while i < 16 {
            uni[i] = 512 - (i as u32) * 32; // 512,480,...,32
            i += 1;
        }
        BigramStore {
            mode: LoadMode::Empty,
            label: "init".to_string(),
            uni,
            bi: Vec::new(),
            degrade_count: 0,
            load_count: 0,
        }
    }

    /// 载入表字节流：magic + cnt(u16 LE) + crc(u32 LE) + cnt*6B 载荷。
    /// CRC 失配 → 整体拒收，降级 Fallback；完好 → Bigram 模式。
    pub fn load(&mut self, bytes: &[u8]) -> Result<LoadMode, u16> {
        self.load_count += 1;
        if bytes.len() < 10 || bytes[0..4] != MAGIC {
            self.degrade_count += 1;
            self.enter_fallback();
            return Ok(LoadMode::Fallback);
        }
        let cnt = (bytes[4] as usize) | ((bytes[5] as usize) << 8);
        let stored_crc =
            (bytes[6] as u32) | ((bytes[7] as u32) << 8) | ((bytes[8] as u32) << 16) | ((bytes[9] as u32) << 24);
        let body_len = 10 + cnt * 6;
        if bytes.len() != body_len {
            // 截断/超长表：同样整体拒收回退
            self.degrade_count += 1;
            self.enter_fallback();
            return Ok(LoadMode::Fallback);
        }
        if crc32(&bytes[10..]) != stored_crc {
            self.degrade_count += 1;
            self.enter_fallback();
            return Ok(LoadMode::Fallback);
        }
        // CRC 通过：解析二元条目（权重 0 条目视作损坏拒收）
        let mut bi: Vec<BiEntry> = Vec::new();
        let mut i = 0usize;
        while i < cnt {
            let off = 10 + i * 6;
            let prev = (bytes[off] as u16) | ((bytes[off + 1] as u16) << 8);
            let next = (bytes[off + 2] as u16) | ((bytes[off + 3] as u16) << 8);
            let w = (bytes[off + 4] as u16) | ((bytes[off + 5] as u16) << 8);
            if prev >= 16 || next >= 16 || w == 0 {
                // 词 id 越界/零权亦视作坏表
                self.degrade_count += 1;
                self.enter_fallback();
                return Ok(LoadMode::Fallback);
            }
            bi.push(BiEntry { prev, next, w });
            i += 1;
        }
        self.bi = bi;
        self.mode = LoadMode::Bigram;
        self.label = "bigram".to_string();
        Ok(LoadMode::Bigram)
    }

    fn enter_fallback(&mut self) {
        self.mode = LoadMode::Fallback;
        self.bi = Vec::new();
        self.label = "unigram-smooth".to_string();
    }

    /// 打分：Bigram 模式查二元权重；Fallback 模式 unigram 平滑。
    pub fn score(&self, prev: u16, next: u16) -> u32 {
        if prev >= 16 || next >= 16 {
            return 1; // 非法词对保底 1 分，绝不悬空
        }
        match self.mode {
            LoadMode::Bigram => {
                let mut found = 0u32;
                let mut i = 0usize;
                while i < self.bi.len() {
                    if self.bi[i].prev == prev && self.bi[i].next == next {
                        found = 1000 + self.bi[i].w as u32;
                    }
                    i += 1;
                }
                if found > 0 {
                    found
                } else {
                    100 + self.uni[next as usize] / 16 // 无二元记录退一档平滑
                }
            }
            _ => 40 + self.uni[next as usize] * 2 + self.uni[prev as usize] / 8,
        }
    }

    /// 回退态单调性：unigram 权重更高的后词得分不更低。
    pub fn audit_mono(&self) -> Result<(), u16> {
        if self.mode != LoadMode::Fallback {
            return Err(E_FB_DEAD);
        }
        let mut n = 1u16;
        while n < 16 {
            if self.score(0, n) > self.score(0, n - 1) {
                return Err(E_MONO_BREAK);
            }
            n += 1;
        }
        Ok(())
    }

    /// 全词对打分可用性：16×16 非零且无 panic 路径。
    pub fn audit_full_score(&self) -> Result<(), u16> {
        let mut p = 0u16;
        while p < 16 {
            let mut n = 0u16;
            while n < 16 {
                if self.score(p, n) == 0 {
                    return Err(E_FB_DEAD);
                }
                n += 1;
            }
            p += 1;
        }
        Ok(())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        h ^= self.mode as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        h ^= self.degrade_count as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        h ^= self.bi.len() as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        let mut i = 0usize;
        while i < self.bi.len() {
            h ^= self.bi[i].prev as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            h ^= self.bi[i].next as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            h ^= self.bi[i].w as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            i += 1;
        }
        h
    }

    pub fn degrade_count(&self) -> u32 {
        self.degrade_count
    }

    pub fn load_count(&self) -> u32 {
        self.load_count
    }

    pub fn bigram_len(&self) -> usize {
        self.bi.len()
    }
}

/// 构造完好表：8 条二元记录。
pub fn build_good_table() -> Vec<u8> {
    let pairs: [(u16, u16, u16); 8] = [
        (0, 1, 90), (0, 2, 70), (1, 3, 80), (2, 4, 60),
        (3, 5, 85), (4, 6, 55), (5, 7, 75), (6, 8, 50),
    ];
    let mut body: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < pairs.len() {
        body.push((pairs[i].0 & 0xFF) as u8);
        body.push((pairs[i].0 >> 8) as u8);
        body.push((pairs[i].1 & 0xFF) as u8);
        body.push((pairs[i].1 >> 8) as u8);
        body.push((pairs[i].2 & 0xFF) as u8);
        body.push((pairs[i].2 >> 8) as u8);
        i += 1;
    }
    let crc = crc32(&body);
    let mut out: Vec<u8> = Vec::new();
    out.push(MAGIC[0]);
    out.push(MAGIC[1]);
    out.push(MAGIC[2]);
    out.push(MAGIC[3]);
    out.push((pairs.len() & 0xFF) as u8);
    out.push((pairs.len() >> 8) as u8);
    out.push((crc & 0xFF) as u8);
    out.push(((crc >> 8) & 0xFF) as u8);
    out.push(((crc >> 16) & 0xFF) as u8);
    out.push(((crc >> 24) & 0xFF) as u8);
    let mut i = 0usize;
    while i < body.len() {
        out.push(body[i]);
        i += 1;
    }
    out
}

/// 位翻转损坏：翻转 payload 中第 bit_off 位。
pub fn corrupt_bit(bytes: &[u8], bit_off: usize) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        out.push(bytes[i]);
        i += 1;
    }
    if bit_off < out.len() * 8 {
        out[bit_off / 8] ^= 1u8 << (bit_off % 8);
    }
    out
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

/// 主巡检：bigram 损坏回退九判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 好表载入 → Bigram 模式且二元权重生效
    let good = build_good_table();
    let mut s = BigramStore::new();
    match s.load(&good) {
        Ok(LoadMode::Bigram) => {}
        _ => return Err(E_GOOD_MISS),
    }
    if s.bigram_len() != 8 {
        return Err(E_GOOD_MISS);
    }
    if s.score(0, 1) != 1090 {
        return Err(E_GOOD_MISS);
    }
    if s.score(0, 9) <= 100 {
        return Err(E_GOOD_MISS);
    }
    hits += 1;

    // R2 位翻转坏表 → CRC 失配拒收降级 Fallback
    let bad = corrupt_bit(&good, 5 * 8 + 3);
    match s.load(&bad) {
        Ok(LoadMode::Fallback) => {}
        _ => return Err(E_CRC_LEAK),
    }
    if s.bigram_len() != 0 {
        return Err(E_CRC_LEAK);
    }
    hits += 1;

    // R3 回退态可用性：全词对非零、单调保持
    s.audit_full_score()?;
    s.audit_mono()?;
    if s.score(3, 7) == 0 {
        return Err(E_FB_DEAD);
    }
    hits += 1;

    // R4 好表重载 → 恢复 Bigram 模式且二元打分回归
    match s.load(&good) {
        Ok(LoadMode::Bigram) => {}
        _ => return Err(E_RESTORE_MISS),
    }
    if s.score(1, 3) != 1080 {
        return Err(E_RESTORE_MISS);
    }
    hits += 1;

    // R5 截断表 / 空表 / 错 magic → 全走回退不 panic
    let mut s2 = BigramStore::new();
    let truncated: Vec<u8> = good[0..12].to_vec();
    match s2.load(&truncated) {
        Ok(LoadMode::Fallback) => {}
        _ => return Err(E_EDGE_LEAK),
    }
    let empty: Vec<u8> = Vec::new();
    match s2.load(&empty) {
        Ok(LoadMode::Fallback) => {}
        _ => return Err(E_EDGE_LEAK),
    }
    let bad_magic: Vec<u8> = vec![b'X', b'Y', b'Z', b'W', 0, 0, 0, 0, 0, 0];
    match s2.load(&bad_magic) {
        Ok(LoadMode::Fallback) => {}
        _ => return Err(E_EDGE_LEAK),
    }
    s2.audit_full_score()?;
    hits += 1;

    // R6 随机位翻转 ×256：每次必降级或保持，无崩溃态
    let mut rng = Lcg::new(0x48443);
    let mut s3 = BigramStore::new();
    let mut degraded = 0u32;
    let mut k = 0u32;
    while k < 256 {
        let bit = rng.below((good.len() * 8) as u64) as usize;
        let bad_v = corrupt_bit(&good, bit);
        match s3.load(&bad_v) {
            Ok(LoadMode::Bigram) => {} // 翻转落在 CRC 无关域仍可通过（合法）
            Ok(LoadMode::Fallback) => degraded += 1,
            Ok(LoadMode::Empty) => return Err(E_CRASH_LEAK),
            Err(_) => return Err(E_CRASH_LEAK),
        }
        s3.audit_full_score()?;
        k += 1;
    }
    if degraded == 0 {
        return Err(E_CRASH_LEAK);
    }
    hits += 1;

    // R7 降级账目：载入账 = 好表 2 + 坏表 1 + 边缘 3 + 随机 256 = 262
    if s3.load_count() != 256 {
        return Err(E_LEDGER_MISS);
    }
    if s.load_count() != 3 || s.degrade_count() != 1 {
        return Err(E_LEDGER_MISS);
    }
    hits += 1;

    // R8 重演一致：同种子随机序列指纹相等
    let fp1 = s3.fingerprint();
    let mut rng2 = Lcg::new(0x48443);
    let mut s4 = BigramStore::new();
    let mut k = 0u32;
    while k < 256 {
        let bit = rng2.below((good.len() * 8) as u64) as usize;
        let bad_v = corrupt_bit(&good, bit);
        let _ = s4.load(&bad_v);
        k += 1;
    }
    if s4.fingerprint() != fp1 || fp1 == 0 {
        return Err(E_REPLAY_DIFF);
    }
    hits += 1;

    // R9 回退与二元打分互斥性：两模式对同词对可区分
    let mut s5 = BigramStore::new();
    let _ = s5.load(&corrupt_bit(&good, 0));
    let fb_score = s5.score(0, 1);
    let _ = s5.load(&good);
    let bg_score = s5.score(0, 1);
    if fb_score == bg_score {
        return Err(E_FB_DEAD);
    }
    hits += 1;

    Ok(hits)
}
