//! UNX-F48449 —— E·音节表损坏拒载（正：坏表回退内置表）
//! 判据原文：UNX-F48449-J1 外置音节表 magic/CRC/条目结构任一损坏必须整体拒载；
//!           拒载后自动回退内置音节表且查表服务可用；
//!           内置表容量不小于最小可用规模；截断表与空表同样回退不 panic；
//!           随机损坏 ×256 每次都安全回退；好表重载即恢复外置模式；
//!           回退账目与重演指纹一致；音节查表全幂等。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 闭包判据全绿才返回 Ok。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xDCC0; // E23 音节表损坏拒载段

const E_EXT_MISS: u16 = SEG_BASE + 1; // 好外置表未生效
const E_BAD_ACCEPT: u16 = SEG_BASE + 2; // 坏表被采纳
const E_FB_DEAD: u16 = SEG_BASE + 3; // 回退态查表失效
const E_BUILTIN_SMALL: u16 = SEG_BASE + 4; // 内置表低于最小规模
const E_CRASH_LEAK: u16 = SEG_BASE + 5; // 随机损坏崩溃
const E_RESTORE_MISS: u16 = SEG_BASE + 6; // 重载未恢复外置
const E_LEDGER_MISS: u16 = SEG_BASE + 7; // 账目不符
const E_REPLAY_DIFF: u16 = SEG_BASE + 8; // 重演不一致
const E_IDEM_MISS: u16 = SEG_BASE + 9; // 查表非幂等

pub const MAGIC: [u8; 4] = [b'S', b'Y', b'L', b'T'];
pub const MIN_SYL: usize = 12;

/// 表模式。
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SylMode {
    Builtin = 0,
    External = 1,
}

/// 音节条目：声母/韵母/调号编码为 3 字节。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SylEntry {
    pub initial: u8,
    pub fin: u8,
    pub tone: u8,
}

/// 音节表管理器。
pub struct SylTable {
    pub mode: SylMode,
    pub label: String,
    builtin: Vec<SylEntry>,
    ext: Vec<SylEntry>,
    fallback_count: u32,
    load_count: u32,
}

impl SylTable {
    pub fn new() -> Self {
        // 内置表：16 条基础音节（initial/final/tone 编码域受限）
        let mut builtin = vec![
            SylEntry { initial: 1, fin: 1, tone: 1 },
            SylEntry { initial: 1, fin: 2, tone: 2 },
            SylEntry { initial: 2, fin: 1, tone: 4 },
            SylEntry { initial: 2, fin: 3, tone: 3 },
            SylEntry { initial: 3, fin: 2, tone: 1 },
            SylEntry { initial: 3, fin: 4, tone: 2 },
            SylEntry { initial: 4, fin: 1, tone: 3 },
            SylEntry { initial: 4, fin: 3, tone: 4 },
            SylEntry { initial: 5, fin: 2, tone: 2 },
            SylEntry { initial: 5, fin: 5, tone: 1 },
            SylEntry { initial: 6, fin: 1, tone: 4 },
            SylEntry { initial: 6, fin: 4, tone: 3 },
            SylEntry { initial: 7, fin: 5, tone: 2 },
            SylEntry { initial: 7, fin: 3, tone: 1 },
            SylEntry { initial: 8, fin: 2, tone: 4 },
            SylEntry { initial: 8, fin: 1, tone: 2 },
        ];
        if builtin.len() < MIN_SYL {
            builtin.push(SylEntry { initial: 9, fin: 1, tone: 1 });
        }
        SylTable {
            mode: SylMode::Builtin,
            label: "builtin".to_string(),
            builtin,
            ext: Vec::new(),
            fallback_count: 0,
            load_count: 0,
        }
    }

    /// 载入外置表：magic(4) + cnt(u16 LE) + crc(u32 LE) + cnt*3B。
    /// 任何结构违规 → 整体拒载，回退内置。
    pub fn load(&mut self, bytes: &[u8]) -> Result<SylMode, u16> {
        self.load_count += 1;
        let ok = self.try_parse(bytes);
        match ok {
            Some(entries) => {
                self.ext = entries;
                self.mode = SylMode::External;
                self.label = "external".to_string();
                Ok(SylMode::External)
            }
            None => {
                self.fallback_count += 1;
                self.ext = Vec::new();
                self.mode = SylMode::Builtin;
                self.label = "builtin-fallback".to_string();
                Ok(SylMode::Builtin)
            }
        }
    }

    fn try_parse(&self, bytes: &[u8]) -> Option<Vec<SylEntry>> {
        if bytes.len() < 10 || bytes[0..4] != MAGIC {
            return None;
        }
        let cnt = (bytes[4] as usize) | ((bytes[5] as usize) << 8);
        if cnt < MIN_SYL || cnt > 4096 || bytes.len() != 10 + cnt * 3 {
            return None;
        }
        let crc = (bytes[6] as u32)
            | ((bytes[7] as u32) << 8)
            | ((bytes[8] as u32) << 16)
            | ((bytes[9] as u32) << 24);
        if crc32(&bytes[10..]) != crc {
            return None;
        }
        let mut v: Vec<SylEntry> = Vec::new();
        let mut i = 0usize;
        while i < cnt {
            let off = 10 + i * 3;
            let ini = bytes[off];
            let fin = bytes[off + 1];
            let tn = bytes[off + 2];
            // 域校验：initial 1..8、final 1..5、tone 1..4
            if ini < 1 || ini > 8 || fin < 1 || fin > 5 || tn < 1 || tn > 4 {
                return None;
            }
            v.push(SylEntry { initial: ini, fin: fin, tone: tn });
            i += 1;
        }
        Some(v)
    }

    /// 查表：外置优先，回退内置；越界索引一律 None。
    pub fn get(&self, idx: usize) -> Option<SylEntry> {
        match self.mode {
            SylMode::External => {
                if idx < self.ext.len() {
                    Some(self.ext[idx])
                } else {
                    None
                }
            }
            SylMode::Builtin => {
                if idx < self.builtin.len() {
                    Some(self.builtin[idx])
                } else {
                    None
                }
            }
        }
    }

    /// 查表幂等：同索引两次结果一致。
    pub fn audit_idem(&self, idx: usize) -> Result<(), u16> {
        let a = self.get(idx);
        let b = self.get(idx);
        if a != b {
            return Err(E_IDEM_MISS);
        }
        Ok(())
    }

    /// 回退态可用性：内置表全索引可查且域合法。
    pub fn audit_builtin(&self) -> Result<(), u16> {
        if self.mode != SylMode::Builtin {
            return Err(E_FB_DEAD);
        }
        if self.builtin.len() < MIN_SYL {
            return Err(E_BUILTIN_SMALL);
        }
        let mut i = 0usize;
        while i < self.builtin.len() {
            match self.get(i) {
                Some(e) => {
                    if e.initial < 1 || e.initial > 8 || e.tone < 1 || e.tone > 4 {
                        return Err(E_FB_DEAD);
                    }
                }
                None => return Err(E_FB_DEAD),
            }
            self.audit_idem(i)?;
            i += 1;
        }
        Ok(())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        h ^= self.mode as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        h ^= self.fallback_count as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        h ^= self.ext.len() as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        h
    }

    pub fn fallback_count(&self) -> u32 {
        self.fallback_count
    }

    pub fn load_count(&self) -> u32 {
        self.load_count
    }

    pub fn builtin_len(&self) -> usize {
        self.builtin.len()
    }

    pub fn ext_len(&self) -> usize {
        self.ext.len()
    }
}

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

/// 构造完好外置表：14 条合法音节。
pub fn build_good_table() -> Vec<u8> {
    let raw: [(u8, u8, u8); 14] = [
        (1, 1, 1), (2, 2, 2), (3, 3, 3), (4, 4, 4), (5, 5, 1), (6, 1, 2),
        (7, 2, 3), (8, 3, 4), (1, 4, 2), (2, 5, 1), (3, 1, 4), (4, 2, 3),
        (5, 3, 2), (6, 5, 1),
    ];
    let mut body: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < raw.len() {
        body.push(raw[i].0);
        body.push(raw[i].1);
        body.push(raw[i].2);
        i += 1;
    }
    let crc = crc32(&body);
    let mut out: Vec<u8> = Vec::new();
    out.push(MAGIC[0]);
    out.push(MAGIC[1]);
    out.push(MAGIC[2]);
    out.push(MAGIC[3]);
    out.push((raw.len() & 0xFF) as u8);
    out.push((raw.len() >> 8) as u8);
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

/// 主巡检：音节表损坏回退九判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 好外置表生效：14 条全可查
    let good = build_good_table();
    let mut s = SylTable::new();
    match s.load(&good) {
        Ok(SylMode::External) => {}
        _ => return Err(E_EXT_MISS),
    }
    if s.ext_len() != 14 || s.get(0).is_none() || s.get(13).is_none() {
        return Err(E_EXT_MISS);
    }
    s.audit_idem(5)?;
    hits += 1;

    // R2 位翻转坏表拒载 → 回退内置
    let mut bad = good.clone();
    let body_off = 10 + 5 * 3 + 1;
    bad[body_off] ^= 0x20;
    match s.load(&bad) {
        Ok(SylMode::Builtin) => {}
        _ => return Err(E_BAD_ACCEPT),
    }
    if s.ext_len() != 0 {
        return Err(E_BAD_ACCEPT);
    }
    s.audit_builtin()?;
    hits += 1;

    // R3 回退态查表服务可用：全索引幂等
    let mut i = 0usize;
    while i < s.builtin_len() {
        s.audit_idem(i)?;
        if s.get(i).is_none() {
            return Err(E_FB_DEAD);
        }
        i += 1;
    }
    hits += 1;

    // R4 截断/空/错 magic 表同样回退
    let mut s2 = SylTable::new();
    let trunc = good[0..11].to_vec();
    match s2.load(&trunc) {
        Ok(SylMode::Builtin) => {}
        _ => return Err(E_BAD_ACCEPT),
    }
    let empty: Vec<u8> = Vec::new();
    match s2.load(&empty) {
        Ok(SylMode::Builtin) => {}
        _ => return Err(E_BAD_ACCEPT),
    }
    let wrong_magic: Vec<u8> = vec![b'X', b'X', b'X', b'X', 12, 0, 0, 0, 0, 0];
    match s2.load(&wrong_magic) {
        Ok(SylMode::Builtin) => {}
        _ => return Err(E_BAD_ACCEPT),
    }
    s2.audit_builtin()?;
    hits += 1;

    // R5 好表重载恢复外置模式
    match s2.load(&good) {
        Ok(SylMode::External) => {}
        _ => return Err(E_RESTORE_MISS),
    }
    if s2.ext_len() != 14 || s2.get(7).is_none() {
        return Err(E_RESTORE_MISS);
    }
    hits += 1;

    // R6 随机损坏 ×256 全部安全回退
    let mut rng = Lcg::new(0x48449);
    let mut s3 = SylTable::new();
    let mut k = 0u32;
    while k < 256 {
        let mut v = good.clone();
        let bit = rng.below((v.len() * 8) as u64) as usize;
        v[bit / 8] ^= 1u8 << (bit % 8);
        match s3.load(&v) {
            Ok(SylMode::Builtin) => {}
            Ok(SylMode::External) => {}
            Err(_) => return Err(E_CRASH_LEAK),
        }
        s3.audit_builtin()?;
        k += 1;
    }
    if s3.fallback_count() == 0 {
        return Err(E_CRASH_LEAK);
    }
    hits += 1;

    // R7 账目：s2 载 4 次（截/空/错 magic/好表）回退 3 次；s 载 2 次回退 1 次
    if s2.load_count() != 4 || s2.fallback_count() != 3 {
        return Err(E_LEDGER_MISS);
    }
    if s.load_count() != 2 || s.fallback_count() != 1 {
        return Err(E_LEDGER_MISS);
    }
    hits += 1;

    // R8 重演一致：同种子指纹相等
    let fp1 = s3.fingerprint();
    let mut rng2 = Lcg::new(0x48449);
    let mut s4 = SylTable::new();
    let mut k = 0u32;
    while k < 256 {
        let mut v = good.clone();
        let bit = rng2.below((v.len() * 8) as u64) as usize;
        v[bit / 8] ^= 1u8 << (bit % 8);
        let _ = s4.load(&v);
        k += 1;
    }
    if s4.fingerprint() != fp1 || fp1 == 0 {
        return Err(E_REPLAY_DIFF);
    }
    hits += 1;

    // R9 外置/内置模式互斥可辨：同索引两模式结果不同（首条目不同表）
    let mut s5 = SylTable::new();
    let _ = s5.load(&good);
    let e_ext = s5.get(2);
    let mut bad2 = good.clone();
    bad2[10] ^= 0x01;
    let _ = s5.load(&bad2);
    let e_fb = s5.get(2);
    if e_ext == e_fb {
        return Err(E_FB_DEAD);
    }
    hits += 1;

    Ok(hits)
}
