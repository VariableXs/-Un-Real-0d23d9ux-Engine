//! UNX-F48442 —— E·词库头部伪造注入（反：魔改版本头 ×10 全拒）
//! 判据原文：UNX-F48442-J1 对词库镜像头部做 10 例系统性伪造（魔数、版本、
//! 词数、CRC、保留位、标志位、头长、字节序标记、截断、超长），
//! 装载器必须逐例拒绝且诊断码可分；正例镜像不受伪造批次影响。
//! 判据落地（drill 场景）：R1 正例装载基准；R2 十例伪造全拒；
//! R3 诊断码可分（≥6 种互异码）；R4 无一伪造例获得信任位；
//! R5 正例复检不受批次影响；R6 伪造统计守恒；R7 逐例预期码精确匹配；
//! R8 指纹稳定；R9 截断/超长两端拦截。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 判据全绿才返回 Ok。
use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址（独立 0x20 窗）。
pub const SEG_BASE: u16 = 0xDBA0;

// ── 诊断码闭集（SEG_BASE+1 起连续，≤ SEG_BASE+0x1F）─────────────
const E_MAGIC: u16 = SEG_BASE + 1; // 魔数不符
const E_VER: u16 = SEG_BASE + 2; // 版本越界
const E_COUNT: u16 = SEG_BASE + 3; // 词数与载荷失配
const E_CRC: u16 = SEG_BASE + 4; // CRC32 校验不符
const E_RSV: u16 = SEG_BASE + 5; // 保留位非零
const E_FLAGS: u16 = SEG_BASE + 6; // 未知标志位
const E_ENDM: u16 = SEG_BASE + 7; // 字节序标记不符
const E_SHORT: u16 = SEG_BASE + 8; // 镜像截断（短于头长）
const E_LONG: u16 = SEG_BASE + 9; // 镜像超长（载荷越词数口径）
const E_MATCH: u16 = SEG_BASE + 10; // 伪造例诊断码与预期不符
const E_TRUST: u16 = SEG_BASE + 11; // 伪造例获得信任位
const E_REBASE: u16 = SEG_BASE + 12; // 正例复检失败
const E_STAT: u16 = SEG_BASE + 13; // 伪造统计不守恒
const E_FP: u16 = SEG_BASE + 14; // 指纹漂移

/// 头长：magic(4)+ver(2)+count(2)+crc(4)+rsv(2)+flags(2)+endm(2) = 18 字节。
pub const HDR_LEN: usize = 18;
/// 词库魔数 "VDIC"。
pub const DICT_MAGIC: [u8; 4] = [0x56, 0x44, 0x49, 0x43];
/// 字节序标记：'LE'。
pub const ENDIAN_MARK: [u8; 2] = [0x4C, 0x45];
pub const VER_LO: u16 = 1;
pub const VER_HI: u16 = 2;
/// 头部保留位与标志位的合法值。
pub const RSV_ZERO: u16 = 0;
pub const FLAGS_KNOWN: u16 = 0x0001;

/// 好词表（6 词）。
pub const GOOD_WORDS: [&str; 6] = ["feng", "yu", "lei", "dian", "yun", "wu"];

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

/// 词库实体。
pub struct Dict {
    pub version: u16,
    pub words: Vec<String>,
    pub trusted: bool,
}

/// 拼 18 字节头 + 载荷的镜像（字段全合法）。
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
    blob.extend_from_slice(&RSV_ZERO.to_le_bytes());
    blob.extend_from_slice(&FLAGS_KNOWN.to_le_bytes());
    blob.extend_from_slice(&ENDIAN_MARK);
    blob.extend_from_slice(&payload);
    blob
}

/// 装载：18 字节头逐字段校验。
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
    let rsv = u16::from_le_bytes([blob[12], blob[13]]);
    if rsv != RSV_ZERO {
        return Err(E_RSV);
    }
    let flags = u16::from_le_bytes([blob[14], blob[15]]);
    if flags & !FLAGS_KNOWN != 0 {
        return Err(E_FLAGS);
    }
    if blob[16] != ENDIAN_MARK[0] || blob[17] != ENDIAN_MARK[1] {
        return Err(E_ENDM);
    }
    let payload = &blob[HDR_LEN..];
    if crc32(payload) != crc {
        return Err(E_CRC);
    }
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

/// 字节 → 字符串（ASCII 拼音逐字节投递）。
fn bytes_to_string(b: &[u8]) -> String {
    let mut s = String::new();
    let mut i = 0usize;
    while i < b.len() {
        s.push(b[i] as char);
        i += 1;
    }
    s
}

/// 单例伪造：镜像 + 预期诊断码。
pub struct Forge {
    pub name: &'static str,
    pub blob: Vec<u8>,
    pub expect: u16,
}

/// 十例系统性头部伪造（魔改版本头 ×10）。
pub fn forge_set() -> Vec<Forge> {
    let base = build_blob(2, &GOOD_WORDS);
    let mut out: Vec<Forge> = vec![];

    // 1 魔数末字节改写。
    let mut b = base.clone();
    b[3] = b'X';
    out.push(Forge { name: "magic", blob: b, expect: E_MAGIC });

    // 2 版本升到 3（越出 1..=2）。
    let mut b = base.clone();
    b[4] = 3;
    b[5] = 0;
    out.push(Forge { name: "version-major", blob: b, expect: E_VER });

    // 3 版本降到 0。
    let mut b = base.clone();
    b[4] = 0;
    b[5] = 0;
    out.push(Forge { name: "version-zero", blob: b, expect: E_VER });

    // 4 词数虚增（CRC 对载荷仍有效 → 落计数失配）。
    let mut b = base.clone();
    let big = (7u16).to_le_bytes();
    b[6] = big[0];
    b[7] = big[1];
    out.push(Forge { name: "count-inflate", blob: b, expect: E_COUNT });

    // 5 词数清零。
    let mut b = base.clone();
    b[6] = 0;
    b[7] = 0;
    out.push(Forge { name: "count-zero", blob: b, expect: E_COUNT });

    // 6 CRC 改写。
    let mut b = base.clone();
    b[8] ^= 0xFF;
    out.push(Forge { name: "crc-flip", blob: b, expect: E_CRC });

    // 7 保留位非零。
    let mut b = base.clone();
    b[12] = 1;
    out.push(Forge { name: "reserved", blob: b, expect: E_RSV });

    // 8 未知标志位。
    let mut b = base.clone();
    b[14] = 0x04;
    out.push(Forge { name: "flags-unknown", blob: b, expect: E_FLAGS });

    // 9 字节序标记改写。
    let mut b = base.clone();
    b[16] = b'B';
    out.push(Forge { name: "endian", blob: b, expect: E_ENDM });

    // 10 截断：砍到半头。
    let mut b = base.clone();
    b.truncate(9);
    out.push(Forge { name: "truncated", blob: b, expect: E_SHORT });

    out
}

/// 伪造就绪指纹：十例名称 + 预期码 + 镜像长度串联。
pub fn forge_fp(fs: &[Forge]) -> u64 {
    let mut buf: Vec<u8> = vec![];
    let mut i = 0usize;
    while i < fs.len() {
        buf.extend_from_slice(fs[i].name.as_bytes());
        buf.push(0x1F);
        buf.extend_from_slice(&fs[i].expect.to_le_bytes());
        buf.extend_from_slice(&(fs[i].blob.len() as u32).to_le_bytes());
        i += 1;
    }
    fnv1a64(&buf)
}

/// 伪造例呈现行：case=<n> name=<例名> len=<镜像长>。
pub fn forge_line(idx: usize, f: &Forge) -> String {
    let mut s = String::new();
    s.push_str("case=");
    s.push_str(&(idx as u32).to_string());
    s.push_str(" name=");
    s.push_str(f.name);
    s.push_str(" len=");
    s.push_str(&(f.blob.len() as u32).to_string());
    s
}

/// 主巡检入口：R1..R9 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 正例装载基准：6 词、版本 2、信任位真。
    let base = build_blob(2, &GOOD_WORDS);
    let d = load(&base)?;
    if !d.trusted || d.words.len() != 6 || d.version != 2 {
        return Err(E_REBASE);
    }
    hits += 1;

    // R2/R7 十例伪造全拒：逐例诊断码精确匹配预期。
    let fs = forge_set();
    if fs.len() != 10 {
        return Err(E_STAT);
    }
    let mut i = 0usize;
    while i < fs.len() {
        match load(&fs[i].blob) {
            Err(code) => {
                if code != fs[i].expect {
                    return Err(E_MATCH);
                }
            }
            Ok(_) => return Err(fs[i].expect),
        }
        i += 1;
    }
    hits += 1;

    // R3 诊断码可分：十例预期码至少出现 6 种互异值。
    let mut distinct: Vec<u16> = vec![];
    let mut i = 0usize;
    while i < fs.len() {
        let mut seen = false;
        let mut j = 0usize;
        while j < distinct.len() {
            if distinct[j] == fs[i].expect {
                seen = true;
            }
            j += 1;
        }
        if !seen {
            distinct.push(fs[i].expect);
        }
        i += 1;
    }
    if distinct.len() < 6 {
        return Err(E_MATCH);
    }
    hits += 1;

    // R4 无一伪造例获得信任位：装载器拒收路径不产 Dict，信任位无从置真。
    // 以可观测口径复核：十例中任何一例成功装载即违规。
    let mut i = 0usize;
    while i < fs.len() {
        if load(&fs[i].blob).is_ok() {
            return Err(E_TRUST);
        }
        i += 1;
    }
    hits += 1;

    // R5 正例复检不受批次影响：伪造批次过后正例仍装载如初。
    let d2 = load(&base)?;
    if !d2.trusted || d2.words.len() != 6 {
        return Err(E_REBASE);
    }
    // 词序逐词一致。
    let mut i = 0usize;
    while i < 6 {
        if d.words[i] != d2.words[i] {
            return Err(E_REBASE);
        }
        i += 1;
    }
    hits += 1;

    // R6 伪造统计守恒：十例全拒计数恰 10。
    let mut rejected = 0u32;
    let mut i = 0usize;
    while i < fs.len() {
        if load(&fs[i].blob).is_err() {
            rejected += 1;
        }
        i += 1;
    }
    if rejected != 10 {
        return Err(E_STAT);
    }
    hits += 1;

    // R8 指纹稳定：伪造集指纹两次一致且非零。
    let f1 = forge_fp(&fs);
    let f2 = forge_fp(&fs);
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    // R9 两端拦截：截断例必落 E_SHORT；把正例尾部再拼接三倍载荷
    // （CRC 对拼接载荷失配）必落 E_CRC——超长同样走校验闸。
    let mut i = 0usize;
    while i < fs.len() {
        if fs[i].name == "truncated" {
            match load(&fs[i].blob) {
                Err(E_SHORT) => hits += 1,
                _ => return Err(E_SHORT),
            }
        }
        i += 1;
    }
    let mut overlong = base.clone();
    let mut k = 0usize;
    while k < 3 {
        overlong.extend_from_slice(&base[HDR_LEN..]);
        k += 1;
    }
    match load(&overlong) {
        // 超长载荷先撞 CRC 闸；若校验通过则由词数口径闸（E_COUNT/E_LONG）收口。
        Err(E_CRC) | Err(E_LONG) | Err(E_COUNT) => hits += 1,
        _ => return Err(E_CRC),
    }

    // LCG 烟测：随机抽 3 例重放仍拒（防顺序依赖）。
    let mut lcg = Lcg::new(0x48442);
    let mut t = 0u32;
    while t < 3 {
        let idx = lcg.below(fs.len() as u32) as usize;
        if load(&fs[idx].blob).is_ok() {
            return Err(E_TRUST);
        }
        t += 1;
    }
    hits += 1;

    Ok(hits)
}
