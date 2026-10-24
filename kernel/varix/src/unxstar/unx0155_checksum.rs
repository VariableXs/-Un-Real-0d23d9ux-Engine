//! UNX-F0155 · 发布校验和与摘要文件 · 诊断段 0x2630
//!
//! 判据原文（UNX-F0155-J1）：产物 → 校验和行（算法 + 十六进制摘要 +
//! 文件名）→ 摘要文件序列化 → 读回重算一致；篡改任一字节即验红。
//!
//! 设计要点：摘要册（DigestBook）——FNV-1a64 主摘要 + CRC8 复核；
//! 摘要行规范化（定宽 hex）；往返账：生成 = 读回逐行等。
//! 零浮点、零 unwrap/expect。

use crate::alloc::{format, string::{String, ToString}, vec::Vec};

pub const SEG_BASE: u16 = 0x2640;

const E_EMPTY: u16 = SEG_BASE + 1; // 空内容摘要拒
const E_DUP: u16 = SEG_BASE + 2; // 摘要册重名文件
const E_FMT: u16 = SEG_BASE + 3; // 摘要行格式非法
const E_CRC: u16 = SEG_BASE + 4; // 复核 CRC 不符
const E_BAL: u16 = SEG_BASE + 5; // 往返账守恒被破坏
const E_MISMATCH: u16 = SEG_BASE + 6; // 重算摘要不符（篡改）

const FNV64_OFF: u64 = 0xCBF2_9CE4_8422_2325;
const FNV64_PRIME: u64 = 0x0000_0100_0000_01B3;

/// FNV-1a 64 位折叠。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h = FNV64_OFF;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(FNV64_PRIME);
    }
    h
}

/// CRC8（多项式 0x07）。
pub fn crc8(data: &[u8]) -> u8 {
    let mut crc: u8 = 0;
    for &b in data {
        crc ^= b;
        for _ in 0..8 {
            let msb = crc & 0x80;
            crc <<= 1;
            if msb != 0 {
                crc ^= 0x07;
            }
        }
    }
    crc
}

/// 摘要行。
#[derive(Clone, PartialEq, Eq)]
pub struct DigestLine {
    pub filename: String,
    pub digest: u64,
    pub check: u8, // 摘要低字节复核位
}

impl DigestLine {
    /// 规范化行文本："fnv1a64 <16位hex> <名>"。
    pub fn render(&self) -> String {
        format!("fnv1a64 {:016x} {}", self.digest, self.filename)
    }
}

/// 行解析：`fnv1a64 <hex16> <名>`；算法名闭集校验。
pub fn parse_line(line: &str) -> Result<DigestLine, u16> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() != 3 {
        return Err(E_FMT);
    }
    if parts[0] != "fnv1a64" {
        return Err(E_FMT);
    }
    let hx = parts[1];
    if hx.len() != 16 || !hx.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(E_FMT);
    }
    let mut digest = 0u64;
    for b in hx.bytes() {
        let v = match b {
            b'0'..=b'9' => b - b'0',
            b'a'..=b'f' => b - b'a' + 10,
            _ => b - b'A' + 10,
        };
        digest = (digest << 4) | v as u64;
    }
    if parts[2].is_empty() {
        return Err(E_FMT);
    }
    Ok(DigestLine {
        filename: parts[2].to_string(),
        digest,
        check: (digest & 0xFF) as u8,
    })
}

/// 摘要册。
pub struct DigestBook {
    pub lines: Vec<DigestLine>,
}

impl DigestBook {
    pub fn new() -> Result<Self, u16> {
        Ok(DigestBook { lines: Vec::new() })
    }

    /// 登记文件摘要（空内容/重名即拒）。
    pub fn add(&mut self, filename: &str, content: &[u8]) -> Result<u64, u16> {
        if content.is_empty() {
            return Err(E_EMPTY);
        }
        if filename.is_empty() || self.lines.iter().any(|l| l.filename == filename) {
            return Err(E_DUP);
        }
        let d = fnv1a64(content);
        self.lines.push(DigestLine {
            filename: filename.to_string(),
            digest: d,
            check: (d & 0xFF) as u8,
        });
        Ok(d)
    }

    /// 序列化（每行一段：行文本 + CRC8）。
    pub fn serialize(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for l in &self.lines {
            let row = l.render();
            out.extend_from_slice(&(row.len() as u16).to_le_bytes());
            out.extend_from_slice(row.as_bytes());
            out.push(crc8(row.as_bytes()));
        }
        out
    }

    /// 反序列化逐行还原（含 CRC 复核）。
    pub fn deserialize(data: &[u8]) -> Result<Self, u16> {
        let mut book = DigestBook::new()?;
        let mut off = 0usize;
        while off < data.len() {
            if off + 2 > data.len() {
                return Err(E_FMT);
            }
            let rl = u16::from_le_bytes([data[off], data[off + 1]]) as usize;
            off += 2;
            if off + rl + 1 > data.len() {
                return Err(E_FMT);
            }
            let row = core::str::from_utf8(&data[off..off + rl]).map_err(|_| E_FMT)?;
            off += rl;
            if crc8(row.as_bytes()) != data[off] {
                return Err(E_CRC);
            }
            off += 1;
            let line = parse_line(row)?;
            book.lines.push(line);
        }
        Ok(book)
    }

    /// 往返账：册自序列化读回后逐行等。
    pub fn roundtrip(&self) -> Result<(), u16> {
        let snap = self.serialize();
        let back = DigestBook::deserialize(&snap)?;
        if back.lines.len() != self.lines.len() {
            return Err(E_BAL);
        }
        for (a, b) in self.lines.iter().zip(back.lines.iter()) {
            if a != b {
                return Err(E_BAL);
            }
        }
        Ok(())
    }

    /// 验收：现算摘要 vs 册内登记（篡改即红）。
    pub fn accept(&self, filename: &str, content: &[u8]) -> Result<(), u16> {
        let l = self
            .lines
            .iter()
            .find(|l| l.filename == filename)
            .ok_or(E_BAL)?;
        if fnv1a64(content) != l.digest || crc8(content) != l.check {
            return Err(E_MISMATCH);
        }
        Ok(())
    }
}

// ── 确定性演练：合法 4 例 + 注入拦截 4 例 ─────────────────────────

/// 演练入口（ktest 挂点）。
pub fn drill() -> Result<(), u16> {
    let mut hits = 0u32;

    // L1：登记 → 往返逐行等。
    let mut b = DigestBook::new()?;
    b.add("varix.bin", b"VARIXPAYLOAD")?;
    b.add("boot.cfg", b"quiet loglevel=3")?;
    b.roundtrip()?;
    hits += 1;

    // L2：行规范化——hex 定宽 16 位。
    let row = b.lines[0].render();
    if !row.starts_with("fnv1a64 ") || row.len() < 8 + 17 + 1 {
        return Err(E_BAL);
    }
    let parsed = parse_line(&row)?;
    if parsed != b.lines[0] {
        return Err(E_BAL);
    }
    hits += 1;

    // L3：验收一致放行。
    b.accept("varix.bin", b"VARIXPAYLOAD")?;
    hits += 1;

    // L4：摘要确定性——同内容同摘要。
    let d1 = fnv1a64(b"same");
    let d2 = fnv1a64(b"same");
    if d1 != d2 {
        return Err(E_BAL);
    }
    hits += 1;

    // I1：空内容摘要拒 + 重名拒。
    let mut i1 = DigestBook::new()?;
    if i1.add("a", b"") != Err(E_EMPTY) {
        return Err(E_EMPTY);
    }
    i1.add("a", b"x")?;
    if i1.add("a", b"y") != Err(E_DUP) {
        return Err(E_DUP);
    }
    hits += 1;

    // I2：行格式非法——错算法名 / 短 hex / 段数不符。
    if !matches!(parse_line("md5 abcd x"), Err(E_FMT)) {
        return Err(E_FMT);
    }
    if !matches!(parse_line("fnv1a64 abc x"), Err(E_FMT)) {
        return Err(E_FMT);
    }
    if !matches!(parse_line("fnv1a64 0123456789abcdef"), Err(E_FMT)) {
        return Err(E_FMT);
    }
    hits += 1;

    // I3：序列化 CRC 翻转 → 读回拒。
    let mut i3 = DigestBook::new()?;
    i3.add("f", b"DATA")?;
    let mut s3 = i3.serialize();
    let n = s3.len();
    s3[n - 1] ^= 0xFF;
    if !matches!(DigestBook::deserialize(&s3), Err(E_CRC)) {
        return Err(E_CRC);
    }
    hits += 1;

    // I4：验收——内容被篡改即红。
    if b.accept("varix.bin", b"VARIXPAYLOAD!") != Err(E_MISMATCH) {
        return Err(E_MISMATCH);
    }
    hits += 1;

    // I5：往返注入——册内行被改后 roundtrip 破。
    let mut i5 = DigestBook::new()?;
    i5.add("f", b"DATA")?;
    i5.lines[0].digest ^= 1;
    if i5.roundtrip() != Err(E_BAL) {
        return Err(E_BAL);
    }
    hits += 1;

    let _ = hits;
    Ok(())
}
