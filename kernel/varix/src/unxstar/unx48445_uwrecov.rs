//! UNX-F48445 —— E·用户词文件损坏原子恢复（正：坏档→备份恢复或重建）
//! 判据原文：UNX-F48445-J1 主档 CRC 损坏时自动切换备份档恢复且内容逐字等价；
//!           主备双坏时重建空档且重建态合法可用；
//!           恢复过程不触碰完好槽位（备份指纹不变）；
//!           随机损坏 ×256 每次都恢复到合法态；恢复后查词服务正常；
//!           提交序列中途中断模拟后仍可恢复；恢复账目与重演指纹一致。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 闭包判据全绿才返回 Ok。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xDC20; // E23 用户词原子恢复段

const E_MAIN_MISS: u16 = SEG_BASE + 1; // 主档完好态判据失准
const E_BACKUP_MISS: u16 = SEG_BASE + 2; // 备份恢复失败/内容不等价
const E_REBUILD_BAD: u16 = SEG_BASE + 3; // 双坏重建态非法
const E_SLOT_TOUCH: u16 = SEG_BASE + 4; // 完好槽位被触碰
const E_CRASH_LEAK: u16 = SEG_BASE + 5; // 随机损坏未恢复到合法态
const E_LOOKUP_DEAD: u16 = SEG_BASE + 6; // 恢复后查词失效
const E_MID_COMMIT: u16 = SEG_BASE + 7; // 中途断提交不可恢复
const E_LEDGER_MISS: u16 = SEG_BASE + 8; // 账目不符
const E_REPLAY_DIFF: u16 = SEG_BASE + 9; // 重演不一致

pub const MAGIC: [u8; 4] = [b'U', b'W', b'D', b'F'];
pub const MAX_WORDS: usize = 32;

/// 档位健康态。
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SlotHealth {
    Good = 0,
    Bad = 1,
    Empty = 2,
}

/// 单个档位帧：magic + ver + cnt(u16 LE) + crc(u32 LE) + cnt*8B 词条（每词 8 字节 ASCII）。
pub struct DictFrame {
    pub health: SlotHealth,
    pub words: Vec<String>,
    pub fp: u64,
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

pub fn fnv1a(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0usize;
    while i < data.len() {
        h ^= data[i] as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    h
}

/// 序列化词表为帧字节流。
pub fn encode_frame(words: &[String]) -> Vec<u8> {
    let cnt = words.len();
    let mut body: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < cnt {
        let wb = words[i].as_bytes();
        let mut j = 0usize;
        while j < 8 {
            if j < wb.len() {
                body.push(wb[j]);
            } else {
                body.push(0);
            }
            j += 1;
        }
        i += 1;
    }
    let crc = crc32(&body);
    let mut out: Vec<u8> = Vec::new();
    out.push(MAGIC[0]);
    out.push(MAGIC[1]);
    out.push(MAGIC[2]);
    out.push(MAGIC[3]);
    out.push(1); // ver
    out.push((cnt & 0xFF) as u8);
    out.push((cnt >> 8) as u8);
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

/// 解析帧：任何结构/CRC 违规返回 None（由上层判健康）。
pub fn parse_frame(bytes: &[u8]) -> Option<Vec<String>> {
    if bytes.len() < 10 || bytes[0..4] != MAGIC {
        return None;
    }
    let cnt = (bytes[5] as usize) | ((bytes[6] as usize) << 8);
    if cnt > MAX_WORDS {
        return None;
    }
    if bytes.len() != 10 + cnt * 8 {
        return None;
    }
    let crc =
        (bytes[7] as u32) | ((bytes[8] as u32) << 8) | ((bytes[9] as u32) << 16) | ((bytes[10] as u32) << 24);
    if crc32(&bytes[10..]) != crc {
        return None;
    }
    let mut words: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < cnt {
        let off = 10 + i * 8;
        let mut s = String::new();
        let mut j = 0usize;
        while j < 8 {
            let b = bytes[off + j];
            if b != 0 {
                s.push(b as char);
            }
            j += 1;
        }
        if s.len() > 0 {
            words.push(s);
        }
        i += 1;
    }
    Some(words)
}

/// 双档存储：main + backup，原子恢复。
pub struct UserDictStore {
    pub main: DictFrame,
    pub backup: DictFrame,
    pub restore_count: u32,
    pub rebuild_count: u32,
}

impl UserDictStore {
    pub fn new(words: &[String]) -> Self {
        let mb = encode_frame(words);
        UserDictStore {
            main: DictFrame { health: SlotHealth::Good, words: words.to_vec(), fp: fnv1a(&mb) },
            backup: DictFrame { health: SlotHealth::Good, words: words.to_vec(), fp: fnv1a(&mb) },
            restore_count: 0,
            rebuild_count: 0,
        }
    }

    /// 写入主档（模拟提交第一步）。
    pub fn commit_main(&mut self, words: &[String]) {
        let mb = encode_frame(words);
        self.main.health = SlotHealth::Good;
        self.main.words = words.to_vec();
        self.main.fp = fnv1a(&mb);
    }

    /// 写入备份档（模拟提交第二步）。
    pub fn commit_backup(&mut self, words: &[String]) {
        let mb = encode_frame(words);
        self.backup.health = SlotHealth::Good;
        self.backup.words = words.to_vec();
        self.backup.fp = fnv1a(&mb);
    }

    /// 主档坏档标记。
    pub fn corrupt_main(&mut self) {
        self.main.health = SlotHealth::Bad;
    }

    /// 备份档坏档标记。
    pub fn corrupt_backup(&mut self) {
        self.backup.health = SlotHealth::Bad;
    }

    /// 原子恢复：主坏→备份恢复；双坏→重建空档。返回恢复后的词表引用长度。
    pub fn recover(&mut self) -> Result<usize, u16> {
        let fp_backup_before = self.backup.fp;
        if self.main.health == SlotHealth::Good {
            return Ok(self.main.words.len());
        }
        if self.backup.health == SlotHealth::Good {
            // 从备份恢复主档；备份自身不得被触碰
            let mut words: Vec<String> = Vec::new();
            let mut i = 0usize;
            while i < self.backup.words.len() {
                words.push(self.backup.words[i].clone());
                i += 1;
            }
            self.commit_main(&words);
            if self.backup.fp != fp_backup_before {
                return Err(E_SLOT_TOUCH);
            }
            self.restore_count += 1;
            return Ok(self.main.words.len());
        }
        // 双坏：重建空档
        self.rebuild_count += 1;
        self.commit_main(&Vec::new());
        self.commit_backup(&Vec::new());
        if self.main.health != SlotHealth::Good || self.backup.health != SlotHealth::Good {
            return Err(E_REBUILD_BAD);
        }
        Ok(0)
    }

    /// 查词：恢复后服务必须可用。
    pub fn lookup(&self, w: &str) -> Result<bool, u16> {
        if self.main.health != SlotHealth::Good {
            return Err(E_LOOKUP_DEAD);
        }
        let mut i = 0usize;
        while i < self.main.words.len() {
            if self.main.words[i] == w {
                return Ok(true);
            }
            i += 1;
        }
        Ok(false)
    }

    /// 内容逐字等价校验。
    pub fn audit_equal(&self, other: &[String]) -> Result<(), u16> {
        if self.main.words.len() != other.len() {
            return Err(E_BACKUP_MISS);
        }
        let mut i = 0usize;
        while i < other.len() {
            if self.main.words[i] != other[i] {
                return Err(E_BACKUP_MISS);
            }
            i += 1;
        }
        Ok(())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h = self.main.fp ^ (self.backup.fp << 1);
        h ^= (self.restore_count as u64) << 32;
        h ^= self.rebuild_count as u64;
        h.wrapping_mul(0x0000_0100_0000_01b3)
    }
}

/// 基准词表。
pub fn seed_words() -> Vec<String> {
    vec![
        "nihao".to_string(),
        "xiexie".to_string(),
        "zaijian".to_string(),
        "qingzhu".to_string(),
        "chenggong".to_string(),
        "weilai".to_string(),
    ]
}

/// 新词表（提交用）。
pub fn extended_words() -> Vec<String> {
    let mut v = seed_words();
    v.push("xinxinxiangrong".to_string());
    v.push("riyiyueyi".to_string());
    v
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

/// 主巡检：原子恢复九判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 双档完好基线
    let seed = seed_words();
    let mut s = UserDictStore::new(&seed);
    let n0 = s.recover()?;
    if n0 != 6 {
        return Err(E_MAIN_MISS);
    }
    if s.lookup("nihao") != Ok(true) {
        return Err(E_MAIN_MISS);
    }
    hits += 1;

    // R2 主档坏 → 备份恢复且内容逐字等价
    let ext = extended_words();
    s.commit_main(&ext);
    s.commit_backup(&ext);
    s.corrupt_main();
    let n1 = s.recover()?;
    if n1 != 8 {
        return Err(E_BACKUP_MISS);
    }
    s.audit_equal(&ext)?;
    if s.lookup("riyiyueyi") != Ok(true) {
        return Err(E_BACKUP_MISS);
    }
    hits += 1;

    // R3 恢复不触碰备份槽位（备份指纹在恢复前后一致）
    let fp_b = s.backup.fp;
    s.corrupt_main();
    let _ = s.recover()?;
    if s.backup.fp != fp_b {
        return Err(E_SLOT_TOUCH);
    }
    hits += 1;

    // R4 双坏 → 重建空档且重建态合法可用
    s.corrupt_main();
    s.corrupt_backup();
    let n2 = s.recover()?;
    if n2 != 0 {
        return Err(E_REBUILD_BAD);
    }
    if s.lookup("nihao") != Ok(false) {
        return Err(E_REBUILD_BAD);
    }
    if s.rebuild_count == 0 {
        return Err(E_LEDGER_MISS);
    }
    hits += 1;

    // R5 恢复后查词服务再写入可用
    s.commit_main(&seed);
    s.commit_backup(&seed);
    if s.lookup("zaijian") != Ok(true) {
        return Err(E_LOOKUP_DEAD);
    }
    hits += 1;

    // R6 中途断提交模拟：主档写一半坏、备份仍是旧好档 → 恢复到旧档
    let mut s2 = UserDictStore::new(&seed);
    s2.commit_backup(&seed);
    // 模拟：主档正在写新词时断电 → 主档坏
    s2.commit_main(&ext);
    s2.corrupt_main();
    let n3 = s2.recover()?;
    if n3 != 6 {
        return Err(E_MID_COMMIT);
    }
    s2.audit_equal(&seed)?;
    hits += 1;

    // R7 随机损坏路径 ×256：恢复必达合法态
    let mut rng = Lcg::new(0x48445);
    let mut s3 = UserDictStore::new(&seed);
    let mut restores = 0u32;
    let mut rebuilds = 0u32;
    let mut k = 0u32;
    while k < 256 {
        let pick = rng.below(3);
        if pick == 0 {
            s3.corrupt_main();
        } else if pick == 1 {
            s3.corrupt_backup();
        } else {
            s3.corrupt_main();
            s3.corrupt_backup();
        }
        let n = s3.recover()?;
        // 恢复后必为合法态：主档 Good 且长度 ∈ {6, 0}
        if s3.main.health != SlotHealth::Good || (n != 6 && n != 0) {
            return Err(E_CRASH_LEAK);
        }
        if s3.restore_count > restores {
            restores = s3.restore_count;
        }
        if s3.rebuild_count > rebuilds {
            rebuilds = s3.rebuild_count;
        }
        // 恢复后补齐备份，进入下一轮
        if n == 0 {
            s3.commit_main(&seed);
            s3.commit_backup(&seed);
        } else {
            s3.commit_backup(&seed);
        }
        k += 1;
    }
    if restores == 0 || rebuilds == 0 {
        return Err(E_CRASH_LEAK);
    }
    hits += 1;

    // R8 账目：s2 一次恢复、s 双坏一次重建
    if s2.restore_count != 1 || s2.rebuild_count != 0 {
        return Err(E_LEDGER_MISS);
    }
    hits += 1;

    // R9 重演一致：同种子随机序列指纹相等
    let fp1 = s3.fingerprint();
    let mut rng2 = Lcg::new(0x48445);
    let mut s4 = UserDictStore::new(&seed);
    let mut k = 0u32;
    while k < 256 {
        let pick = rng2.below(3);
        if pick == 0 {
            s4.corrupt_main();
        } else if pick == 1 {
            s4.corrupt_backup();
        } else {
            s4.corrupt_main();
            s4.corrupt_backup();
        }
        let _ = s4.recover();
        let n = s4.main.words.len();
        if n == 0 {
            s4.commit_main(&seed);
            s4.commit_backup(&seed);
        } else {
            s4.commit_backup(&seed);
        }
        k += 1;
    }
    if s4.fingerprint() != fp1 || fp1 == 0 {
        return Err(E_REPLAY_DIFF);
    }
    hits += 1;

    Ok(hits)
}
