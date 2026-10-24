//! UNX-F0066 · 内存图快照落盘与序列化 · 诊断段 0x2110
//! 验收锚点：UNX-F0066-J1 内存图快照定长二进制序列化/反序列化
//! 往返一致，版本头+魔数+FNV 指纹三重校验判据通过；
//! 纯功能代码 ≥260 行。零浮点、零 unwrap、零 expect。


use crate::alloc::{vec::Vec};

pub const SEG_BASE: u16 = 0x2110;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_MAGIC: u16 = SEG_BASE + 1;    // 魔数不符
const E_VER: u16 = SEG_BASE + 2;      // 版本不支持
const E_LEN: u16 = SEG_BASE + 3;      // 长度域与实际不符
const E_FP: u16 = SEG_BASE + 4;       // 指纹不符（数据损坏）
const E_ENTRY: u16 = SEG_BASE + 5;    // 条目流截断
const E_RANGE: u16 = SEG_BASE + 6;    // 条目区间倒序
const E_BAL: u16 = SEG_BASE + 7;      // 条目计数与声明不符
const E_RDONLY: u16 = SEG_BASE + 8;   // 快照已封板再注入
const E_DUP: u16 = SEG_BASE + 9;      // 重复基址条目

/// 快照魔数 "VMS1"（Varix Memory Snapshot v1）。
pub const SNAP_MAGIC: [u8; 4] = [0x56, 0x4D, 0x53, 0x31];
/// 快照版本。
pub const SNAP_VERSION: u32 = 1;
/// 头长：魔数 4 + 版本 4 + 条目数 4 + 总长 4 + 指纹 8 + 保留 8 = 32。
pub const SNAP_HDR_LEN: usize = 32;
/// 条目定长：基址 8 + 长度 8 + 类型 4 = 20。
pub const SNAP_ENTRY_LEN: usize = 20;

/// 快照条目类型闭集（与 E820/EFI 桥接对齐的内部统一类）。
pub const K_USABLE: u32 = 1;
pub const K_RESERVED: u32 = 2;
pub const K_RECLAIM: u32 = 3;
pub const K_NVS: u32 = 4;
pub const K_BADRAM: u32 = 5;

pub fn kind_name(k: u32) -> &'static str {
    match k {
        K_USABLE => "可用",
        K_RESERVED => "保留",
        K_RECLAIM => "可回收",
        K_NVS => "常驻 NVS",
        K_BADRAM => "坏页",
        _ => "超纲",
    }
}

pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_MAGIC => "魔数非 VMS1：不是本仓快照流",
        E_VER => "版本不支持：仅 v1 可反序列化",
        E_LEN => "长度域与实际流长不符：截断或拼接损坏",
        E_FP => "指纹不符：载荷损坏，快照作废",
        E_ENTRY => "条目流截断：条目数 × 20 ≠ 载荷长",
        E_RANGE => "条目区间倒序：base+len 回绕或 end ≤ base",
        E_BAL => "条目计数与声明不符：头/体不一致",
        E_RDONLY => "快照已封板：再注入属误用",
        E_DUP => "重复基址条目：后到拒绝",
        _ => "未知诊断码",
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SnapEntry {
    pub base: u64,
    pub len: u64,
    pub kind: u32,
}

impl SnapEntry {
    pub fn end(&self) -> Result<u64, u16> {
        self.base.checked_add(self.len).ok_or(E_RANGE)
    }
    pub fn pages(&self) -> u64 {
        self.len / 4096
    }
}

// ── 快照账（构建侧）──────────────────────────────────────────
pub struct SnapBuilder {
    pub entries: Vec<SnapEntry>,
    pub sealed: bool,
}

impl SnapBuilder {
    pub fn new() -> SnapBuilder {
        SnapBuilder {
            entries: Vec::new(),
            sealed: false,
        }
    }

    pub fn add(&mut self, base: u64, len: u64, kind: u32) -> Result<(), u16> {
        if self.sealed {
            return Err(E_RDONLY);
        }
        if len == 0 || base.checked_add(len).is_none() {
            return Err(E_RANGE);
        }
        if self.entries.iter().any(|e| e.base == base) {
            return Err(E_DUP);
        }
        self.entries.push(SnapEntry { base, len, kind });
        Ok(())
    }

    /// 封板并序列化：头（32B）+ 条目流，指纹覆盖条目流全量。
    pub fn seal_and_serialize(&mut self) -> Result<Vec<u8>, u16> {
        if self.entries.is_empty() {
            return Err(E_BAL);
        }
        self.sealed = true;
        let mut body: Vec<u8> = Vec::new();
        for e in &self.entries {
            body.extend_from_slice(&e.base.to_le_bytes());
            body.extend_from_slice(&e.len.to_le_bytes());
            body.extend_from_slice(&e.kind.to_le_bytes());
        }
        let fp = fnv1a64(&body);
        let mut out: Vec<u8> = Vec::new();
        out.extend_from_slice(&SNAP_MAGIC);
        out.extend_from_slice(&SNAP_VERSION.to_le_bytes());
        out.extend_from_slice(&(self.entries.len() as u32).to_le_bytes());
        out.extend_from_slice(&((SNAP_HDR_LEN + body.len()) as u32).to_le_bytes());
        out.extend_from_slice(&fp.to_le_bytes());
        out.extend_from_slice(&[0u8; 8]); // 保留域
        out.extend_from_slice(&body);
        Ok(out)
    }
}

// ── 反序列化（读取侧）────────────────────────────────────────
pub struct SnapView {
    pub entries: Vec<SnapEntry>,
    pub fingerprint: u64,
}

pub fn deserialize(blob: &[u8]) -> Result<SnapView, u16> {
    if blob.len() < SNAP_HDR_LEN {
        return Err(E_LEN);
    }
    if blob[0..4] != SNAP_MAGIC {
        return Err(E_MAGIC);
    }
    let ver = u32::from_le_bytes([blob[4], blob[5], blob[6], blob[7]]);
    if ver != SNAP_VERSION {
        return Err(E_VER);
    }
    let count = u32::from_le_bytes([blob[8], blob[9], blob[10], blob[11]]) as usize;
    let total = u32::from_le_bytes([blob[12], blob[13], blob[14], blob[15]]) as usize;
    if total != blob.len() {
        return Err(E_LEN);
    }
    if total != SNAP_HDR_LEN + count * SNAP_ENTRY_LEN {
        return Err(E_BAL);
    }
    let fp = u64::from_le_bytes([
        blob[16], blob[17], blob[18], blob[19],
        blob[20], blob[21], blob[22], blob[23],
    ]);
    let body = &blob[SNAP_HDR_LEN..];
    if fnv1a64(body) != fp {
        return Err(E_FP);
    }
    let mut entries = Vec::new();
    let mut off = 0usize;
    for _ in 0..count {
        if off + SNAP_ENTRY_LEN > body.len() {
            return Err(E_ENTRY);
        }
        let b = &body[off..off + SNAP_ENTRY_LEN];
        let e = SnapEntry {
            base: u64::from_le_bytes([
                b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
            ]),
            len: u64::from_le_bytes([
                b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15],
            ]),
            kind: u32::from_le_bytes([b[16], b[17], b[18], b[19]]),
        };
        e.end()?;
        if e.len == 0 {
            return Err(E_RANGE);
        }
        entries.push(e);
        off += SNAP_ENTRY_LEN;
    }
    Ok(SnapView {
        entries,
        fingerprint: fp,
    })
}

impl SnapView {
    /// 可用内存页数（kind=usable）。
    pub fn usable_pages(&self) -> u64 {
        self.entries
            .iter()
            .filter(|e| e.kind == K_USABLE)
            .map(|e| e.pages())
            .sum()
    }

    /// 坏页清单页数。
    pub fn bad_pages(&self) -> u64 {
        self.entries
            .iter()
            .filter(|e| e.kind == K_BADRAM)
            .map(|e| e.pages())
            .sum()
    }

    /// 类型直方图（6 桶：0=超纲）。
    pub fn histogram(&self) -> [u64; 6] {
        let mut h = [0u64; 6];
        for e in &self.entries {
            let i = if e.kind >= 1 && e.kind <= 5 { e.kind as usize } else { 0 };
            h[i] += 1;
        }
        h
    }

    /// 与另一快照对拍：条目流指纹一致即同图。
    pub fn cross_check(&self, other: &SnapView) -> Result<(), u16> {
        if self.fingerprint != other.fingerprint {
            return Err(E_FP);
        }
        Ok(())
    }
}

// ── 确定性演练：合法 3 例 + 注入拦截 5 例 ────────────────────────
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：构建→封板→序列化→反序列化→逐条目往返一致。
    let mut sb = SnapBuilder::new();
    sb.add(0x0000_0000, 0x0009_FC00, K_USABLE)?;
    sb.add(0x0009_FC00, 0x0000_0400, K_RESERVED)?;
    sb.add(0x0010_0000, 0x1000_0000, K_USABLE)?;
    sb.add(0x1111_0000, 0x0000_1000, K_BADRAM)?;
    let blob = sb.seal_and_serialize()?;
    let v = deserialize(&blob)?;
    if v.entries.len() != 4 {
        return Err(E_BAL);
    }
    if v.usable_pages() != (0x9FC00 + 0x1000_0000) / 4096 {
        return Err(E_BAL);
    }
    if v.bad_pages() != 1 {
        return Err(E_BAL);
    }
    hits += 1;

    // R2：二次反序列化对拍（同 blob 指纹一致）。
    let v2 = deserialize(&blob)?;
    v.cross_check(&v2)?;
    hits += 1;

    // R3：直方图桶闭集。
    let h = v.histogram();
    if h[K_USABLE as usize] != 2 || h[K_RESERVED as usize] != 1 || h[K_BADRAM as usize] != 1 {
        return Err(E_BAL);
    }
    hits += 1;

    // I1：魔数注入 → E_MAGIC。
    let mut b1 = blob.clone();
    b1[0] = 0x58;
    if deserialize(&b1).err() != Some(E_MAGIC) {
        return Err(E_MAGIC);
    }
    hits += 1;

    // I2：版本改坏 → E_VER。
    let mut b2 = blob.clone();
    b2[4] = 9;
    if deserialize(&b2).err() != Some(E_VER) {
        return Err(E_VER);
    }
    hits += 1;

    // I3：载荷翻转 → E_FP（指纹拦截）。
    let mut b3 = blob.clone();
    let n = b3.len();
    b3[n - 1] ^= 0xFF;
    if deserialize(&b3).err() != Some(E_FP) {
        return Err(E_FP);
    }
    hits += 1;

    // I4：长度域改坏 → E_LEN。
    let mut b4 = blob.clone();
    b4[12] = 0xFF;
    if deserialize(&b4).err() != Some(E_LEN) {
        return Err(E_LEN);
    }
    hits += 1;

    // I5：条目计数改坏 → E_BAL。
    let mut b5 = blob.clone();
    b5[8] = 0x63; // 99 条
    if deserialize(&b5).err() != Some(E_BAL) {
        return Err(E_BAL);
    }
    hits += 1;

    // I6：封板后再注入 → E_RDONLY。
    let mut sb2 = SnapBuilder::new();
    sb2.add(0x1000, 0x1000, K_USABLE)?;
    sb2.seal_and_serialize()?;
    if sb2.add(0x2000, 0x1000, K_USABLE) != Err(E_RDONLY) {
        return Err(E_RDONLY);
    }
    hits += 1;

    Ok(hits)
}
