//! UNX-F0148 · 镜像版本号注入 · 诊断段 0x2570
//!
//! 判据原文（UNX-F0148-J1）：构建时把版本号（主.次.补 + 哈希尾）注入
//! 镜像固定偏移；注入后版本账可读回且逐字段一致；非法版本串全拒。
//!
//! 设计要点：版本串解析闭集（主.次.补[-尾]），注入账守恒：
//! 注入 = 读回（逐字节往返）；偏移对齐账（固定偏移 0x20，版本块
//! 定长 16 字节）。零浮点、零 unwrap/expect。

use crate::alloc::{format, string::String, vec, vec::Vec};

pub const SEG_BASE: u16 = 0x2570;

const E_FMT: u16 = SEG_BASE + 1; // 版本串格式非法
const E_RANGE: u16 = SEG_BASE + 2; // 字段数值越纲（>999 或尾长≠8）
const E_OFF: u16 = SEG_BASE + 3; // 偏移/长度不符（镜像过小或块越界）
const E_BAL: u16 = SEG_BASE + 4; // 注入账守恒被破坏
const E_HEX: u16 = SEG_BASE + 5; // 尾哈希非十六进制

/// 版本块固定偏移。
pub const VER_OFFSET: usize = 0x20;
/// 版本块定长。
pub const VER_BLOCK_LEN: usize = 16;
/// 哈希尾定长（十六进制字符数）。
pub const HASH_TAIL_LEN: usize = 8;

/// 解析后版本。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Version {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
    pub tail: u32, // 哈希尾按 8 个 hex 字符折叠的 u32
}

impl Version {
    /// 渲染标准串 "主.次.补-尾"。
    pub fn render(&self) -> String {
        format!(
            "{}.{}.{}-{:08x}",
            self.major, self.minor, self.patch, self.tail
        )
    }
}

fn hex_val(c: u8) -> Result<u8, u16> {
    match c {
        b'0'..=b'9' => Ok(c - b'0'),
        b'a'..=b'f' => Ok(c - b'a' + 10),
        b'A'..=b'F' => Ok(c - b'A' + 10),
        _ => Err(E_HEX),
    }
}

/// 版本串解析：`主.次.补[-尾]`；字段 0~999；尾恰 8 位 hex（含则必全）。
pub fn parse_version(s: &str) -> Result<Version, u16> {
    let (nums, tail) = match s.split_once('-') {
        Some((n, t)) => (n, Some(t)),
        None => (s, None),
    };
    let parts: Vec<&str> = nums.split('.').collect();
    if parts.len() != 3 {
        return Err(E_FMT);
    }
    let mut fields = [0u16; 3];
    for (i, p) in parts.iter().enumerate() {
        if p.is_empty() || p.len() > 3 || !p.bytes().all(|b| b.is_ascii_digit()) {
            return Err(E_FMT);
        }
        let v: u32 = p.bytes().fold(0, |acc, b| acc * 10 + (b - b'0') as u32);
        if v > 999 {
            return Err(E_RANGE);
        }
        fields[i] = v as u16;
    }
    let tail = match tail {
        None => 0,
        Some(t) => {
            if t.len() != HASH_TAIL_LEN {
                return Err(E_RANGE);
            }
            let tb = t.as_bytes();
            let mut acc = 0u32;
            for &c in tb {
                acc = (acc << 4) | hex_val(c)? as u32;
            }
            acc
        }
    };
    Ok(Version {
        major: fields[0],
        minor: fields[1],
        patch: fields[2],
        tail,
    })
}

/// 版本块序列化（16 字节）：magic'V' 3B'ER' + 主/次/补各 u16 + 尾 u32 + 备用 u16。
pub fn ver_block(v: &Version) -> Vec<u8> {
    let mut b = Vec::with_capacity(VER_BLOCK_LEN);
    b.extend_from_slice(b"VER");
    b.extend_from_slice(&v.major.to_le_bytes());
    b.extend_from_slice(&v.minor.to_le_bytes());
    b.extend_from_slice(&v.patch.to_le_bytes());
    b.extend_from_slice(&v.tail.to_le_bytes());
    b.extend_from_slice(&0u16.to_le_bytes());
    b
}

/// 注入：镜像 VER_OFFSET 处覆写版本块；镜像过小即拒。
pub fn inject(image: &mut Vec<u8>, v: &Version) -> Result<(), u16> {
    if image.len() < VER_OFFSET + VER_BLOCK_LEN {
        return Err(E_OFF);
    }
    let blk = ver_block(v);
    image[VER_OFFSET..VER_OFFSET + VER_BLOCK_LEN].copy_from_slice(&blk);
    Ok(())
}

/// 读回：从镜像固定偏移解析版本块。
pub fn read_back(image: &[u8]) -> Result<Version, u16> {
    if image.len() < VER_OFFSET + VER_BLOCK_LEN {
        return Err(E_OFF);
    }
    let blk = &image[VER_OFFSET..VER_OFFSET + VER_BLOCK_LEN];
    if &blk[0..3] != b"VER" {
        return Err(E_FMT);
    }
    let g2 = |i: usize| u16::from_le_bytes([blk[i], blk[i + 1]]);
    let tail = u32::from_le_bytes([blk[9], blk[10], blk[11], blk[12]]);
    Ok(Version {
        major: g2(3),
        minor: g2(5),
        patch: g2(7),
        tail,
    })
}

/// 注入账：往返守恒（注入 = 读回逐字段）。
pub fn roundtrip_ok(before: &Version, after: &Version) -> Result<(), u16> {
    if before != after {
        return Err(E_BAL);
    }
    Ok(())
}

// ── 确定性演练：合法 4 例 + 注入拦截 4 例 ─────────────────────────

/// 演练入口（ktest 挂点）。
pub fn drill() -> Result<(), u16> {
    let mut hits = 0u32;

    // L1：解析含尾版本 → 注入 → 读回逐字段一致。
    let v = parse_version("1.4.7-9a3bc0de")?;
    if v.major != 1 || v.minor != 4 || v.patch != 7 || v.tail != 0x9A3BC0DE {
        return Err(E_BAL);
    }
    let mut img = vec![0u8; 0x40];
    inject(&mut img, &v)?;
    let back = read_back(&img)?;
    roundtrip_ok(&v, &back)?;
    hits += 1;

    // L2：无尾版本（尾补零）往返一致 + 渲染串含主次补。
    let v2 = parse_version("0.1.0")?;
    if v2.tail != 0 {
        return Err(E_BAL);
    }
    let mut img2 = vec![0xFFu8; 0x40];
    inject(&mut img2, &v2)?;
    roundtrip_ok(&v2, &read_back(&img2)?)?;
    let s = v2.render();
    if !s.starts_with("0.1.0") {
        return Err(E_BAL);
    }
    hits += 1;

    // L3：注入是覆写语义——二次注入读回新值。
    let mut img3 = vec![0u8; 0x40];
    inject(&mut img3, &v)?;
    let v3 = parse_version("2.0.1-cafebab0")?;
    inject(&mut img3, &v3)?;
    roundtrip_ok(&v3, &read_back(&img3)?)?;
    hits += 1;

    // L4：版本块十六字节布局锚（"VER" 头 + 备用尾零）。
    let blk = ver_block(&v);
    if blk.len() != VER_BLOCK_LEN || &blk[0..3] != b"VER" || &blk[14..16] != &[0, 0] {
        return Err(E_BAL);
    }
    hits += 1;

    // I1：格式非法——两段 / 四段 / 空字段 / 非数字。
    if !matches!(parse_version("1.2"), Err(E_FMT)) {
        return Err(E_FMT);
    }
    if !matches!(parse_version("1.2.3.4"), Err(E_FMT)) {
        return Err(E_FMT);
    }
    if !matches!(parse_version("1..3"), Err(E_FMT)) {
        return Err(E_FMT);
    }
    if !matches!(parse_version("1.a.3"), Err(E_FMT)) {
        return Err(E_FMT);
    }
    hits += 1;

    // I2：数值越纲——999 以外（1000）与尾长不符。
    if !matches!(parse_version("1000.0.0"), Err(E_RANGE)) {
        return Err(E_RANGE);
    }
    if !matches!(parse_version("1.0.0-abc"), Err(E_RANGE)) {
        return Err(E_RANGE);
    }
    hits += 1;

    // I3：哈希尾非十六进制。
    if !matches!(parse_version("1.0.0-zzzzzzzz"), Err(E_HEX)) {
        return Err(E_HEX);
    }
    hits += 1;

    // I4：镜像过小注入/读回双拒（E_OFF）。
    let mut small = vec![0u8; VER_OFFSET + 3];
    if inject(&mut small, &v) != Err(E_OFF) {
        return Err(E_OFF);
    }
    if read_back(&small) != Err(E_OFF) {
        return Err(E_OFF);
    }
    hits += 1;

    // I5：守恒注入——注入后篡改块内字段，读回与原版不等。
    let mut img5 = vec![0u8; 0x40];
    inject(&mut img5, &v)?;
    img5[VER_OFFSET + 3] ^= 0xFF; // 篡主版本低位
    let back5 = read_back(&img5)?;
    if roundtrip_ok(&v, &back5) != Err(E_BAL) {
        return Err(E_BAL);
    }
    hits += 1;

    let _ = hits;
    Ok(())
}
