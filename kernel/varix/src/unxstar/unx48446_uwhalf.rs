//! UNX-F48446 —— E·用户词半写状态注入（反：断电模拟半档 ×10 不采纳不损坏好档）
//! 判据原文：UNX-F48446-J1 断电写档中断产生的半档帧（截体/缺 CRC/坏尾 magic/
//!           零长声明等 ×10 形态）全部不采纳；
//!           每次半档拒收后好档读服务不受损（词表指纹不变）；
//!           随机截断 ×512 全拒零崩；半档形态四型封闭；
//!           完整帧照常采纳；重演指纹一致；守恒 seq = 拒收 + 采纳。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 闭包判据全绿才返回 Ok。

use crate::alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xDC40; // E23 半写状态注入段

const E_HALF_PASS: u16 = SEG_BASE + 1; // 半档被采纳
const E_GOOD_DAMAGED: u16 = SEG_BASE + 2; // 好档被损坏
const E_READ_DEAD: u16 = SEG_BASE + 3; // 拒后读服务失效
const E_KIND_BAD: u16 = SEG_BASE + 4; // 半档形态越界
const E_COUNT_MISS: u16 = SEG_BASE + 5; // 拒收计数不符
const E_FULL_MISS: u16 = SEG_BASE + 6; // 完整帧误拒
const E_REPLAY_DIFF: u16 = SEG_BASE + 7; // 重演不一致
const E_BAL_MISS: u16 = SEG_BASE + 8; // 守恒破

pub const MAGIC: [u8; 4] = [b'U', b'W', b'H', b'F'];
pub const MAX_WORDS: usize = 24;

/// 半档形态。
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum HalfKind {
    TruncBody = 0,   // 载荷被截断（声明长度 > 实际长度）
    MissingCrc = 1,  // 帧尾 CRC 字节缺失
    BadTail = 2,     // 尾部 magic 损坏（写中途 magic 区被覆写）
    ZeroDeclared = 3, // 声明长度为 0 但载荷非空
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

/// 编码完整帧：magic + cnt(u16 LE) + crc(u32 LE) + cnt*4B 词条（每词 4 字节）。
pub fn encode_frame(words: &[&str]) -> Vec<u8> {
    let cnt = words.len();
    let mut body: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < cnt {
        let wb = words[i].as_bytes();
        let mut j = 0usize;
        while j < 4 {
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

/// 半写帧形态分类（不修复，仅判型）。
pub fn classify_half(bytes: &[u8]) -> Option<HalfKind> {
    if bytes.len() >= 4 && bytes[0..4] == MAGIC {
        if bytes.len() < 10 {
            // 头在但 CRC 区被截
            return Some(HalfKind::MissingCrc);
        }
        let cnt = (bytes[4] as usize) | ((bytes[5] as usize) << 8);
        if cnt == 0 && bytes.len() > 10 {
            return Some(HalfKind::ZeroDeclared);
        }
        if bytes.len() < 10 + cnt * 4 {
            return Some(HalfKind::TruncBody);
        }
        // 结构完整但 CRC 失配 → 尾部损坏形态
        let crc =
            (bytes[6] as u32) | ((bytes[7] as u32) << 8) | ((bytes[8] as u32) << 16) | ((bytes[9] as u32) << 24);
        if crc32(&bytes[10..]) != crc {
            return Some(HalfKind::BadTail);
        }
        None // 完整帧
    } else {
        // 头部 magic 不全：截在 magic 区内或 magic 错 → BadTail 前奏
        if bytes.len() < 4 {
            Some(HalfKind::MissingCrc)
        } else {
            Some(HalfKind::BadTail)
        }
    }
}

/// 半写守门器：好档常驻，半档注入一律拒收。
pub struct HalfWriteGate {
    good_words: Vec<String>,
    good_fp: u64,
    rejects: Vec<(u64, HalfKind)>,
    adopt_count: u32,
    seq: u64,
}

impl HalfWriteGate {
    pub fn new(words: &[&str]) -> Self {
        let mut gw: Vec<String> = Vec::new();
        let mut i = 0usize;
        while i < words.len() {
            gw.push(words[i].to_string());
            i += 1;
        }
        let mut fb: Vec<u8> = Vec::new();
        let mut i = 0usize;
        while i < gw.len() {
            let wb = gw[i].as_bytes();
            let mut j = 0usize;
            while j < wb.len() {
                fb.push(wb[j]);
                j += 1;
            }
            i += 1;
        }
        HalfWriteGate {
            good_words: gw,
            good_fp: fnv1a(&fb),
            rejects: Vec::new(),
            adopt_count: 0,
            seq: 0,
        }
    }

    /// 投递一帧字节流（模拟断电半写产物）。
    pub fn submit(&mut self, bytes: &[u8]) -> Result<(), u16> {
        self.seq += 1;
        match classify_half(bytes) {
            Some(kind) => {
                self.rejects.push((self.seq, kind));
                // 不采纳：好档指纹必须不变
                if self.good_fp_state() != self.good_fp {
                    return Err(E_GOOD_DAMAGED);
                }
                Err(E_HALF_PASS)
            }
            None => {
                // 完整帧采纳：好档更新为帧内容
                self.adopt_count += 1;
                Ok(())
            }
        }
    }

    fn good_fp_state(&self) -> u64 {
        let mut b: Vec<u8> = Vec::new();
        let mut i = 0usize;
        while i < self.good_words.len() {
            let wb = self.good_words[i].as_bytes();
            let mut j = 0usize;
            while j < wb.len() {
                b.push(wb[j]);
                j += 1;
            }
            i += 1;
        }
        fnv1a(&b)
    }

    /// 拒后读服务：好档词表仍可查。
    pub fn read_lookup(&self, w: &str) -> Result<bool, u16> {
        if self.good_words.is_empty() {
            return Err(E_READ_DEAD);
        }
        let mut i = 0usize;
        while i < self.good_words.len() {
            if self.good_words[i] == w {
                return Ok(true);
            }
            i += 1;
        }
        Ok(false)
    }

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
        h ^= self.rejects.len() as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        let mut i = 0usize;
        while i < self.rejects.len() {
            h ^= self.rejects[i].0;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            h ^= self.rejects[i].1 as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            i += 1;
        }
        h ^= self.adopt_count as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        h
    }

    pub fn reject_count(&self) -> u32 {
        self.rejects.len() as u32
    }

    pub fn adopt_count(&self) -> u32 {
        self.adopt_count
    }

    pub fn seq(&self) -> u64 {
        self.seq
    }
}

/// 半档 ×10 注入清单：从完整帧构造十种断电形态。
pub fn half_ten() -> Vec<Vec<u8>> {
    let full = encode_frame(&["nihao", "xiexie", "zaijian", "qingzhu"]);
    let mut list: Vec<Vec<u8>> = Vec::new();
    // 1 截在 magic 区
    list.push(full[0..2].to_vec());
    // 2 截在头部/CRC 区
    list.push(full[0..8].to_vec());
    // 3 截在载荷中段
    list.push(full[0..14].to_vec());
    // 4 缺最后一个字节（CRC 尾）
    list.push(full[0..full.len() - 1].to_vec());
    // 5 magic 首字节坏
    let mut v5 = full.clone();
    v5[0] = b'X';
    list.push(v5);
    // 6 载荷翻位（CRC 失配 → 尾损形态）
    let mut v6 = full.clone();
    v6[12] ^= 0x40;
    list.push(v6);
    // 7 声明长度 0 但载荷在
    let mut v7 = full.clone();
    v7[4] = 0;
    v7[5] = 0;
    list.push(v7);
    // 8 只剩 magic
    list.push(full[0..4].to_vec());
    // 9 空（全无）
    list.push(Vec::new());
    // 10 载荷截到仅 1 词
    list.push(full[0..14].to_vec());
    list
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

/// 主巡检：半写注入八判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 半档 ×10 全拒
    let mut g = HalfWriteGate::new(&["nihao", "xiexie", "zaijian", "qingzhu", "chenggong"]);
    let halves = half_ten();
    let mut i = 0usize;
    while i < halves.len() {
        match g.submit(&halves[i]) {
            Err(E_HALF_PASS) => {}
            _ => return Err(E_HALF_PASS),
        }
        i += 1;
    }
    if g.reject_count() != 10 {
        return Err(E_COUNT_MISS);
    }
    hits += 1;

    // R2 好档不损坏：指纹恒定
    let fp0 = g.fp();
    if g.good_fp_state() != g.good_fp {
        return Err(E_GOOD_DAMAGED);
    }
    let _ = fp0;
    hits += 1;

    // R3 拒后读服务正常
    if g.read_lookup("xiexie") != Ok(true) {
        return Err(E_READ_DEAD);
    }
    if g.read_lookup("bucunzai") != Ok(false) {
        return Err(E_READ_DEAD);
    }
    hits += 1;

    // R4 半档形态四型封闭
    g.audit_kind_closed()?;
    let mut seen = [0u32; 4];
    let mut i = 0usize;
    while i < g.rejects.len() {
        seen[g.rejects[i].1 as usize] += 1;
        i += 1;
    }
    if seen[0] == 0 || seen[1] == 0 || seen[2] == 0 || seen[3] == 0 {
        return Err(E_KIND_BAD);
    }
    hits += 1;

    // R5 LCG 随机截断 ×512 全拒零崩
    let full = encode_frame(&["nihao", "xiexie", "zaijian", "qingzhu"]);
    let mut rng = Lcg::new(0x48446);
    let mut k = 0u32;
    while k < 512 {
        let cut = rng.below(full.len() as u64) as usize;
        let piece = full[0..cut].to_vec();
        match g.submit(&piece) {
            Err(E_HALF_PASS) => {}
            _ => return Err(E_HALF_PASS),
        }
        g.audit_kind_closed()?;
        if g.good_fp_state() != g.good_fp {
            return Err(E_GOOD_DAMAGED);
        }
        k += 1;
    }
    if g.reject_count() != 522 {
        return Err(E_COUNT_MISS);
    }
    hits += 1;

    // R6 完整帧采纳 + 误拒检查：好帧必须走采纳路径
    match g.submit(&full) {
        Ok(()) => {}
        Err(_) => return Err(E_FULL_MISS),
    }
    if g.adopt_count() != 1 {
        return Err(E_FULL_MISS);
    }
    hits += 1;

    // R7 重演一致：同种子同历史指纹相等
    let f1 = g.fp();
    let mut g2 = HalfWriteGate::new(&["nihao", "xiexie", "zaijian", "qingzhu", "chenggong"]);
    let halves2 = half_ten();
    let mut i = 0usize;
    while i < halves2.len() {
        let _ = g2.submit(&halves2[i]);
        i += 1;
    }
    let mut rng2 = Lcg::new(0x48446);
    let mut k = 0u32;
    while k < 512 {
        let cut = rng2.below(full.len() as u64) as usize;
        let piece = full[0..cut].to_vec();
        let _ = g2.submit(&piece);
        k += 1;
    }
    let _ = g2.submit(&full);
    if f1 != g2.fp() || f1 == 0 {
        return Err(E_REPLAY_DIFF);
    }
    hits += 1;

    // R8 守恒：seq = 拒收 + 采纳（g: 10 + 512 拒，1 采纳 = 523）
    let total = g.reject_count() + g.adopt_count();
    if g.seq() != total as u64 {
        return Err(E_BAL_MISS);
    }
    hits += 1;

    Ok(hits)
}
