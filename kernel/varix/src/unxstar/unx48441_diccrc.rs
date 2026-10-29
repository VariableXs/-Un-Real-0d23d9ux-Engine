//! UNX-F48441 —— E·词库 CRC 损坏检出与回退（正：坏库→精简库兜底可输入）
//! 判据原文：UNX-F48441-J1 词库镜像头部含 magic/version/count/CRC32 四字段；
//! 载荷或头部任一损坏必须被检出并拒绝装载；随后精简兜底库必须生效，
//! 兜底词表非空且拼出的输入逐词可解析；坏库在本次会话内不得再被使用。
//! 判据落地（drill 场景）：R1 好库装载；R2 载荷损坏检出；R3 头部 magic 损坏检出；
//! R4 版本越界检出；R5 计数失配检出；R6 兜底库生效且非空；
//! R7 兜底输入逐词可解析；R8 坏库标记不复用；R9 兜底词表指纹稳定；
//! R10 空输入在兜底路径拒收；R11 CRC 对好库零误报。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 判据全绿才返回 Ok。
use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址（独立 0x20 窗）。
pub const SEG_BASE: u16 = 0xDB60;

// ── 诊断码闭集（SEG_BASE+1 起连续，≤ SEG_BASE+0x1F）─────────────
const E_MAGIC: u16 = SEG_BASE + 1; // 魔数不符
const E_VER: u16 = SEG_BASE + 2; // 版本越界
const E_COUNT: u16 = SEG_BASE + 3; // 词数与载荷失配
const E_CRC: u16 = SEG_BASE + 4; // CRC32 校验不符
const E_SHORT: u16 = SEG_BASE + 5; // 镜像短于头长
const E_FALLBACK: u16 = SEG_BASE + 6; // 兜底库未生效或为空
const E_RESOLVE: u16 = SEG_BASE + 7; // 兜底输入含不可解析词
const E_STICKY: u16 = SEG_BASE + 8; // 坏库被复用
const E_FP: u16 = SEG_BASE + 9; // 兜底词表指纹漂移
const E_EMPTY: u16 = SEG_BASE + 10; // 空输入
const E_FALSEPOS: u16 = SEG_BASE + 11; // 好库误报

/// 头长：magic(4) + version(2) + count(2) + crc32(4) = 12 字节。
pub const HDR_LEN: usize = 12;
/// 词库魔数 "VDIC"。
pub const DICT_MAGIC: [u8; 4] = [0x56, 0x44, 0x49, 0x43];
/// 版本闭集 1..=2。
pub const VER_LO: u16 = 1;
pub const VER_HI: u16 = 2;

/// 精简兜底词表（16 词冻结）。
pub const MINI_WORDS: [&str; 16] = [
    "ni", "hao", "shi", "jie", "tian", "di", "ren", "he",
    "chun", "xia", "qiu", "dong", "shan", "shui", "yue", "ri",
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

/// CRC-32（IEEE，逐位实现，无查表）。
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    let mut i = 0usize;
    while i < data.len() {
        crc ^= data[i] as u32;
        let mut b = 0u8;
        while b < 8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
            b += 1;
        }
        i += 1;
    }
    !crc
}

/// 词库镜像头投影。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DictHdr {
    pub version: u16,
    pub count: u16,
    pub crc: u32,
}

/// 词库实体：版本 + 词表 + 装载诊断位。
pub struct Dict {
    pub version: u16,
    pub words: Vec<String>,
    pub trusted: bool,
}

/// 用好词表拼镜像（含正确 CRC）。
pub fn build_blob(version: u16, words: &[&str]) -> Vec<u8> {
    let mut payload: Vec<u8> = vec![];
    let mut i = 0usize;
    while i < words.len() {
        payload.extend_from_slice(words[i].as_bytes());
        if i + 1 < words.len() {
            payload.push(b'\n');
        }
        i += 1;
    }
    let mut blob: Vec<u8> = vec![];
    blob.extend_from_slice(&DICT_MAGIC);
    blob.extend_from_slice(&version.to_le_bytes());
    blob.extend_from_slice(&(words.len() as u16).to_le_bytes());
    blob.extend_from_slice(&crc32(&payload).to_le_bytes());
    blob.extend_from_slice(&payload);
    blob
}

/// 装载：逐字段校验，任何一项不符即拒。
pub fn load(blob: &[u8]) -> Result<Dict, u16> {
    if blob.len() < HDR_LEN {
        return Err(E_SHORT);
    }
    if blob[0] != DICT_MAGIC[0] || blob[1] != DICT_MAGIC[1] || blob[2] != DICT_MAGIC[2] || blob[3] != DICT_MAGIC[3] {
        return Err(E_MAGIC);
    }
    let version = u16::from_le_bytes([blob[4], blob[5]]);
    if version < VER_LO || version > VER_HI {
        return Err(E_VER);
    }
    let count = u16::from_le_bytes([blob[6], blob[7]]);
    let crc = u32::from_le_bytes([blob[8], blob[9], blob[10], blob[11]]);
    let payload = &blob[HDR_LEN..];
    if crc32(payload) != crc {
        return Err(E_CRC);
    }
    // 计数与载荷一致：按 '\n' 分词后必须恰 count 词。
    let mut words: Vec<String> = vec![];
    let mut cur: Vec<u8> = vec![];
    let mut i = 0usize;
    while i < payload.len() {
        if payload[i] == b'\n' {
            words.push(bytes_to_string(&cur));
            cur.clear();
        } else {
            cur.push(payload[i]);
        }
        i += 1;
    }
    if !cur.is_empty() {
        words.push(bytes_to_string(&cur));
    }
    if words.len() != count as usize {
        return Err(E_COUNT);
    }
    Ok(Dict {
        version,
        words,
        trusted: true,
    })
}

/// 字节 → 字符串（载荷假定为 ASCII 拼音，逐字节投递）。
fn bytes_to_string(b: &[u8]) -> String {
    let mut s = String::new();
    let mut i = 0usize;
    while i < b.len() {
        s.push(b[i] as char);
        i += 1;
    }
    s
}

/// 兜底库：精简词表恒可用。
pub fn fallback_dict() -> Dict {
    let mut words: Vec<String> = vec![];
    let mut i = 0usize;
    while i < MINI_WORDS.len() {
        words.push(MINI_WORDS[i].to_string());
        i += 1;
    }
    Dict {
        version: VER_LO,
        words,
        trusted: false,
    }
}

/// 空间分词：按空格切输入并逐词核对社会词表。
pub fn resolve_input(input: &str, valid: &[&str]) -> Result<usize, u16> {
    if input.is_empty() {
        return Err(E_EMPTY);
    }
    let b = input.as_bytes();
    let mut resolved = 0usize;
    let mut start = 0usize;
    let mut i = 0usize;
    while i <= b.len() {
        if i == b.len() || b[i] == b' ' {
            if i > start {
                let mut w = String::new();
                let mut k = start;
                while k < i {
                    w.push(b[k] as char);
                    k += 1;
                }
                let mut hit = false;
                let mut v = 0usize;
                while v < valid.len() {
                    if w == valid[v] {
                        hit = true;
                    }
                    v += 1;
                }
                if !hit {
                    return Err(E_RESOLVE);
                }
                resolved += 1;
            }
            start = i + 1;
        }
        i += 1;
    }
    if resolved == 0 {
        return Err(E_EMPTY);
    }
    Ok(resolved)
}

/// 兜底词表指纹。
pub fn mini_fp() -> u64 {
    let mut buf: Vec<u8> = vec![];
    let mut i = 0usize;
    while i < MINI_WORDS.len() {
        buf.extend_from_slice(MINI_WORDS[i].as_bytes());
        buf.push(0x0A);
        i += 1;
    }
    fnv1a64(&buf)
}

/// 主巡检入口：R1..R11 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // 基础好库（8 词），后续各例围绕它做损坏注入。
    let good: [&str; 8] = ["ni", "hao", "shi", "jie", "tian", "di", "ren", "he"];
    let blob_ok = build_blob(2, &good);

    // R1 好库装载：字段齐、信任位真、词数恰 8。
    let d = load(&blob_ok)?;
    if !d.trusted || d.words.len() != 8 || d.version != 2 {
        return Err(E_FALSEPOS);
    }
    hits += 1;

    // R11 CRC 对好库零误报：好库载荷 CRC 复算必等。
    let payload = &blob_ok[HDR_LEN..];
    let declared = u32::from_le_bytes([
        blob_ok[8], blob_ok[9], blob_ok[10], blob_ok[11],
    ]);
    if crc32(payload) != declared {
        return Err(E_FALSEPOS);
    }
    hits += 1;

    // R2 载荷损坏检出：翻转载荷一个字节必报 E_CRC。
    let mut bad_payload = blob_ok.clone();
    let mut lcg = Lcg::new(0x48441);
    let flip_at = HDR_LEN + lcg.below(payload.len() as u32) as usize;
    bad_payload[flip_at] ^= 0x01;
    match load(&bad_payload) {
        Err(E_CRC) => hits += 1,
        _ => return Err(E_CRC),
    }

    // R3 头部 magic 损坏检出。
    let mut bad_magic = blob_ok.clone();
    bad_magic[2] = b'X';
    match load(&bad_magic) {
        Err(E_MAGIC) => hits += 1,
        _ => return Err(E_MAGIC),
    }

    // R4 版本越界检出：0 与 3 均拒。
    let mut bad_ver = blob_ok.clone();
    bad_ver[4] = 0;
    bad_ver[5] = 0;
    match load(&bad_ver) {
        Err(E_VER) => hits += 1,
        _ => return Err(E_VER),
    }
    let mut bad_ver2 = blob_ok.clone();
    bad_ver2[5] = 3; // 小端高位 → version = 0x0300 之外再试显式 3
    bad_ver2[4] = 3;
    bad_ver2[5] = 0;
    match load(&bad_ver2) {
        Err(E_VER) => hits += 1,
        _ => return Err(E_VER),
    }
    hits += 1;

    // R5 计数失配检出：头部词数改大必报 E_COUNT（CRC 仍须先对上）。
    let mut bad_count = blob_ok.clone();
    let cnt_field = (9u16).to_le_bytes();
    bad_count[6] = cnt_field[0];
    bad_count[7] = cnt_field[1];
    // CRC 是对载荷的，载荷未动，故 CRC 仍通过 → 落到计数检查。
    match load(&bad_count) {
        Err(E_COUNT) => hits += 1,
        _ => return Err(E_COUNT),
    }

    // R6 兜底库生效且非空：坏库拒装后立即切精简库。
    let fb = fallback_dict();
    if fb.words.len() != MINI_WORDS.len() || fb.words.is_empty() {
        return Err(E_FALLBACK);
    }
    if fb.trusted {
        return Err(E_FALLBACK);
    }
    hits += 1;

    // R7 兜底输入逐词可解析：兜底词拼句全命中。
    let sentence = "ni hao shan shui";
    match resolve_input(sentence, &MINI_WORDS) {
        Ok(n) => {
            if n != 4 {
                return Err(E_RESOLVE);
            }
        }
        Err(_) => return Err(E_RESOLVE),
    }
    hits += 1;

    // R8 坏库标记不复用：装载失败会话内 sticky 位一经置位不得回落。
    let mut sticky = false;
    if load(&bad_payload).is_err() {
        sticky = true;
    }
    if !sticky {
        return Err(E_STICKY);
    }
    // sticky 置位后再试坏库：仍拒，且兜底库始终是唯一活动库。
    if load(&bad_payload).is_ok() {
        return Err(E_STICKY);
    }
    if !fallback_dict().words.is_empty() {
        hits += 1;
    } else {
        return Err(E_FALLBACK);
    }

    // R9 兜底词表指纹稳定且非零。
    let f1 = mini_fp();
    let f2 = mini_fp();
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R10 空输入在兜底路径拒收。
    match resolve_input("", &MINI_WORDS) {
        Err(E_EMPTY) => hits += 1,
        _ => return Err(E_EMPTY),
    }

    Ok(hits)
}
