//! UNX-F48537 —— E27 与 M5 USB 域边界声明（设备枚举语义不二写）
//! 判据原文：UNX-F48537-J1 本域（E27 热插拔域）对 USB 设备枚举语义
//! 只作引用方，枚举唯一权威在 M5 USB 域：边界契约表（枚举键/回放键/
//! 所有权/方向）冻结 8 条；本域不得自建枚举键（不二写）；
//! 引用键必须能在契约表中回查命中；所有权越界引用必拒。
//! 判据落地（drill 场景）：R1 契约表规模恰 8；R2 键位连续升序防重；
//! R3 所有权字段闭集（枚举权恒归 M5）；R4 方向字段闭集（引用单向）；
//! R5 引用回查全命中；R6 本域自建键清单与契约键零交集（不二写）；
//! R7 越权引用必拒；R8 指纹稳定；R9 呈现行齐全。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 判据全绿才返回 Ok。
use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址（独立 0x20 窗）。
pub const SEG_BASE: u16 = 0xF060;

// ── 诊断码闭集（SEG_BASE+1 起连续，≤ SEG_BASE+0x1F）─────────────
const E_N: u16 = SEG_BASE + 1; // 契约条数 ≠ 8
const E_GAP: u16 = SEG_BASE + 2; // 键位不连续/重复
const E_OWNER: u16 = SEG_BASE + 3; // 所有权字段越出闭集
const E_DIR: u16 = SEG_BASE + 4; // 方向字段越出闭集
const E_MISS: u16 = SEG_BASE + 5; // 引用键回查未命中
const E_DUAL: u16 = SEG_BASE + 6; // 本域自建键与契约键交集非空（二写）
const E_RIGHT: u16 = SEG_BASE + 7; // 越权引用（声明所有权在本域）
const E_FP: u16 = SEG_BASE + 8; // 指纹漂移
const E_LINE: u16 = SEG_BASE + 9; // 呈现行缺段

/// 契约条数冻结值。
pub const CONTRACT_N: usize = 8;

/// 所有权闭集：1 = M5 USB 域（枚举权威）。
pub const OWN_M5: u8 = 1;
/// 方向闭集：1 = E27→M5 单向引用。
pub const DIR_REF: u8 = 1;

/// 边界契约条目：键位 / 名称 / 所有权 / 方向。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BoundEntry {
    pub key: u16,
    pub name: &'static str,
    pub owner: u8,
    pub dir: u8,
}

/// 边界契约表（冻结 8 条，键位 SEG_BASE+0x80 起连续——引用 M5 段窗基）。
pub const M5_KEY_BASE: u16 = 0x5000;

pub const CONTRACT: [BoundEntry; 8] = [
    BoundEntry { key: M5_KEY_BASE + 1, name: "usb-enum-port", owner: OWN_M5, dir: DIR_REF },
    BoundEntry { key: M5_KEY_BASE + 2, name: "usb-enum-devaddr", owner: OWN_M5, dir: DIR_REF },
    BoundEntry { key: M5_KEY_BASE + 3, name: "usb-enum-descriptor", owner: OWN_M5, dir: DIR_REF },
    BoundEntry { key: M5_KEY_BASE + 4, name: "usb-enum-vidpid", owner: OWN_M5, dir: DIR_REF },
    BoundEntry { key: M5_KEY_BASE + 5, name: "usb-enum-interface", owner: OWN_M5, dir: DIR_REF },
    BoundEntry { key: M5_KEY_BASE + 6, name: "usb-enum-endpoint", owner: OWN_M5, dir: DIR_REF },
    BoundEntry { key: M5_KEY_BASE + 7, name: "usb-enum-hub-tier", owner: OWN_M5, dir: DIR_REF },
    BoundEntry { key: M5_KEY_BASE + 8, name: "usb-enum-remove-evt", owner: OWN_M5, dir: DIR_REF },
];

/// 本域自建键清单（热插拔语义键，冻结 6 条——不得与契约键重叠）。
pub const OWN_KEYS: [u16; 6] = [
    SEG_BASE + 0x21,
    SEG_BASE + 0x22,
    SEG_BASE + 0x23,
    SEG_BASE + 0x24,
    SEG_BASE + 0x25,
    SEG_BASE + 0x26,
];

/// FNV-1a 64 位（与本域其余哨兵同口径）。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 确定性 LCG（批内统一系数）。
pub struct Lcg {
    state: u32,
}

impl Lcg {
    pub fn new(seed: u32) -> Lcg {
        Lcg { state: seed }
    }
    pub fn next_u32(&mut self) -> u32 {
        self.state = self.state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.state
    }
    pub fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            0
        } else {
            self.next_u32() % bound
        }
    }
}

/// 键位回查：引用键必须命中契约表。
pub fn lookup(key: u16) -> Option<&'static BoundEntry> {
    let mut i = 0usize;
    while i < CONTRACT.len() {
        if CONTRACT[i].key == key {
            return Some(&CONTRACT[i]);
        }
        i += 1;
    }
    None
}

/// 越权引用判定：引用方（E27）不得声明枚举所有权。
pub fn owns_enum(key: u16, claimed_owner: u8) -> Result<(), u16> {
    match lookup(key) {
        Some(e) => {
            if claimed_owner != e.owner {
                return Err(E_RIGHT);
            }
            Ok(())
        }
        None => Err(E_MISS),
    }
}

/// 契约指纹。
pub fn contract_fp() -> u64 {
    let mut buf: Vec<u8> = vec![];
    let mut i = 0usize;
    while i < CONTRACT.len() {
        buf.extend_from_slice(&CONTRACT[i].key.to_le_bytes());
        buf.push(CONTRACT[i].owner);
        buf.push(CONTRACT[i].dir);
        buf.extend_from_slice(CONTRACT[i].name.as_bytes());
        buf.push(0x1F);
        i += 1;
    }
    fnv1a64(&buf)
}

/// 自建键与契约键交集（线性双扫，返回首个撞键）。
pub fn dual_write_key() -> Option<u16> {
    let mut i = 0usize;
    while i < OWN_KEYS.len() {
        let mut j = 0usize;
        while j < CONTRACT.len() {
            if OWN_KEYS[i] == CONTRACT[j].key {
                return Some(OWN_KEYS[i]);
            }
            j += 1;
        }
        i += 1;
    }
    None
}

/// 契约行（呈现面）：key=<hex> name=<名> owner=<n> dir=<n>。
pub fn entry_line(e: &BoundEntry) -> String {
    let mut s = String::new();
    s.push_str("key=");
    s.push_str(&e.key.to_string());
    s.push_str(" name=");
    s.push_str(e.name);
    s.push_str(" owner=");
    s.push_str(&(e.owner as u32).to_string());
    s.push_str(" dir=");
    s.push_str(&(e.dir as u32).to_string());
    s
}

/// 主巡检入口：R1..R9 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 契约条数恰 8。
    if CONTRACT.len() != CONTRACT_N {
        return Err(E_N);
    }
    hits += 1;

    // R2 键位连续升序防重：M5_KEY_BASE+1 起逐位 +1。
    let mut i = 0usize;
    while i < CONTRACT.len() {
        let expect = M5_KEY_BASE + 1 + i as u16;
        if CONTRACT[i].key != expect {
            return Err(E_GAP);
        }
        if CONTRACT[i].name.is_empty() {
            return Err(E_GAP);
        }
        let mut j = i + 1;
        while j < CONTRACT.len() {
            if CONTRACT[i].key == CONTRACT[j].key || CONTRACT[i].name == CONTRACT[j].name {
                return Err(E_GAP);
            }
            j += 1;
        }
        i += 1;
    }
    hits += 1;

    // R3 所有权字段闭集：枚举权恒归 M5。
    let mut i = 0usize;
    while i < CONTRACT.len() {
        if CONTRACT[i].owner != OWN_M5 {
            return Err(E_OWNER);
        }
        i += 1;
    }
    hits += 1;

    // R4 方向字段闭集：引用恒单向 E27→M5。
    let mut i = 0usize;
    while i < CONTRACT.len() {
        if CONTRACT[i].dir != DIR_REF {
            return Err(E_DIR);
        }
        i += 1;
    }
    hits += 1;

    // R5 引用回查全命中：LCG 随机取契约键回查逐条命中且字段一致。
    let mut lcg = Lcg::new(0x48537);
    let mut t = 0u32;
    while t < 16 {
        let idx = lcg.below(CONTRACT_N as u32) as usize;
        match lookup(M5_KEY_BASE + 1 + idx as u16) {
            Some(e) => {
                if e.key != M5_KEY_BASE + 1 + idx as u16 || e.owner != OWN_M5 {
                    return Err(E_MISS);
                }
            }
            None => return Err(E_MISS),
        }
        t += 1;
    }
    hits += 1;

    // R6 不二写：本域自建键与契约键交集必空。
    if dual_write_key().is_some() {
        return Err(E_DUAL);
    }
    // 自建键闭集内部也须防重。
    let mut i = 0usize;
    while i < OWN_KEYS.len() {
        let mut j = i + 1;
        while j < OWN_KEYS.len() {
            if OWN_KEYS[i] == OWN_KEYS[j] {
                return Err(E_DUAL);
            }
            j += 1;
        }
        i += 1;
    }
    hits += 1;

    // R7 越权引用必拒：契约键 + 声明所有权在本域 → E_RIGHT；键越出契约 → E_MISS。
    match owns_enum(M5_KEY_BASE + 3, 2) {
        Err(E_RIGHT) => {}
        _ => return Err(E_RIGHT),
    }
    match owns_enum(M5_KEY_BASE + 0x20, OWN_M5) {
        Err(E_MISS) => {}
        _ => return Err(E_MISS),
    }
    // 合法引用（M5 所有权）放行。
    if owns_enum(M5_KEY_BASE + 5, OWN_M5).is_err() {
        return Err(E_RIGHT);
    }
    hits += 1;

    // R8 指纹稳定：契约指纹两次一致且非零。
    let f1 = contract_fp();
    let f2 = contract_fp();
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R9 呈现行齐全：key=/name=/owner=/dir= 四段在位。
    let line = entry_line(&CONTRACT[0]);
    if !has_kw(line.as_bytes(), b"key=")
        || !has_kw(line.as_bytes(), b"name=")
        || !has_kw(line.as_bytes(), b"owner=")
        || !has_kw(line.as_bytes(), b"dir=")
    {
        return Err(E_LINE);
    }
    hits += 1;

    Ok(hits)
}

/// 子串判定（朴素扫描，统一口径）。
pub fn has_kw(hay: &[u8], kw: &[u8]) -> bool {
    if kw.is_empty() || hay.len() < kw.len() {
        return false;
    }
    let mut i = 0usize;
    while i + kw.len() <= hay.len() {
        let mut j = 0usize;
        while j < kw.len() && hay[i + j] == kw[j] {
            j += 1;
        }
        if j == kw.len() {
            return true;
        }
        i += 1;
    }
    false
}
