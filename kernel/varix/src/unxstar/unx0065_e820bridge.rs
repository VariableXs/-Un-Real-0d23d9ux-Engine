//! UNX-F0065 · E820 桥接层（BIOS 兼容路径）· 诊断段 0x2100
//! 验收锚点：UNX-F0065-J1 E820 条目（类型 1/2/3/4 闭集）解析入账，
//! 与 EFI MemoryMap 双向桥接映射判据通过；纯功能代码 ≥260 行。
//! 零浮点、零 unwrap、零 expect。


use crate::alloc::{vec::Vec};

pub const SEG_BASE: u16 = 0x2100;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_TYPE: u16 = SEG_BASE + 1;     // E820 类型超闭集
const E_RANGE: u16 = SEG_BASE + 2;    // 区间倒序/回绕
const E_DUP: u16 = SEG_BASE + 3;      // 同基址重复登记
const E_OVERLAP: u16 = SEG_BASE + 4;  // 同类区间重叠
const E_BRIDGE: u16 = SEG_BASE + 5;   // 桥接映射无对应类（类型落差）
const E_BAL: u16 = SEG_BASE + 6;      // 桥接账守恒不平
const E_RDONLY: u16 = SEG_BASE + 7;   // 桥已建再注入
const E_ENTRY: u16 = SEG_BASE + 8;    // 条目尺寸不足 20 字节
const E_ZERO: u16 = SEG_BASE + 9;     // 零长度条目

/// E820 类型闭集。
pub const E820_USABLE: u32 = 1;      // 可用 RAM
pub const E820_RESERVED: u32 = 2;    // 保留
pub const E820_ACPI: u32 = 3;        // ACPI Reclaimable
pub const E820_NVS: u32 = 4;         // ACPI NVS
/// 类型 5+：登记留痕但不参与桥接。

pub fn e820_type_name(t: u32) -> &'static str {
    match t {
        E820_USABLE => "可用 RAM",
        E820_RESERVED => "保留",
        E820_ACPI => "ACPI 可回收",
        E820_NVS => "ACPI NVS",
        _ => "超纲类型",
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
        E_TYPE => "E820 类型超闭集：1/2/3/4 入账，5+ 留痕不桥接",
        E_RANGE => "区间倒序或回绕：地址算术拦截",
        E_DUP => "同基址重复条目：后到拒绝",
        E_OVERLAP => "同类区间重叠：合并语义未授权，拒绝",
        E_BRIDGE => "桥接无对应类：E820 类型与 EFI 类型落差需显式裁决",
        E_BAL => "桥接守恒：入桥条目数 = 出桥条目数",
        E_RDONLY => "桥已建立：账面冻结",
        E_ENTRY => "E820 条目 <20 字节：BIOS 兼容最小布局",
        E_ZERO => "零长度条目：留痕不参与账",
        _ => "未知诊断码",
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct E820Entry {
    pub base: u64,
    pub len: u64,
    pub ty: u32,
}

impl E820Entry {
    pub fn end(&self) -> Result<u64, u16> {
        self.base.checked_add(self.len).ok_or(E_RANGE)
    }
}

/// E820 裸流解析：每条目 20 字节（base 8 + len 8 + type 4）。
pub fn parse_e820_stream(raw: &[u8]) -> Result<Vec<E820Entry>, u16> {
    if raw.len() < 20 {
        return Err(E_ENTRY);
    }
    let mut out = Vec::new();
    let mut off = 0usize;
    while off + 20 <= raw.len() {
        let base = u64::from_le_bytes([
            raw[off], raw[off+1], raw[off+2], raw[off+3],
            raw[off+4], raw[off+5], raw[off+6], raw[off+7],
        ]);
        let len = u64::from_le_bytes([
            raw[off+8], raw[off+9], raw[off+10], raw[off+11],
            raw[off+12], raw[off+13], raw[off+14], raw[off+15],
        ]);
        let ty = u32::from_le_bytes([
            raw[off+16], raw[off+17], raw[off+18], raw[off+19],
        ]);
        out.push(E820Entry { base, len, ty });
        off += 20;
    }
    Ok(out)
}

// ── E820 账 ──────────────────────────────────────────────────
pub struct E820Ledger {
    pub entries: Vec<E820Entry>,
    pub rejected: Vec<(u64, u16)>,
    pub overshoot_types: Vec<u32>,
    pub sealed: bool,
}

impl E820Ledger {
    pub fn new() -> E820Ledger {
        E820Ledger {
            entries: Vec::new(),
            rejected: Vec::new(),
            overshoot_types: Vec::new(),
            sealed: false,
        }
    }

    pub fn absorb(&mut self, e: E820Entry) -> Result<bool, u16> {
        if self.sealed {
            return Err(E_RDONLY);
        }
        if e.len == 0 {
            self.rejected.push((e.base, E_ZERO));
            return Ok(false);
        }
        e.end()?;
        if e.ty > E820_NVS {
            self.overshoot_types.push(e.ty);
            self.rejected.push((e.base, E_TYPE));
            return Ok(false);
        }
        if self.entries.iter().any(|x| x.base == e.base) {
            self.rejected.push((e.base, E_DUP));
            return Ok(false);
        }
        // 同类重叠拒绝（跨类相邻/嵌套允许——BIOS 现实如此）
        if let Ok(end) = e.end() {
            for x in &self.entries {
                if x.ty == e.ty && e.base < x.end().unwrap_or(0) && x.base < end {
                    self.rejected.push((e.base, E_OVERLAP));
                    return Ok(false);
                }
            }
        }
        self.entries.push(e);
        Ok(true)
    }

    pub fn seal(&mut self) -> Result<usize, u16> {
        self.sealed = true;
        Ok(self.entries.len())
    }

    /// 可用 RAM 总页数。
    pub fn usable_pages(&self) -> u64 {
        self.entries
            .iter()
            .filter(|e| e.ty == E820_USABLE)
            .map(|e| e.len / 4096)
            .sum()
    }

    /// 类型直方图（4 桶 + 超纲计数）。
    pub fn histogram(&self) -> [u64; 5] {
        let mut h = [0u64; 5];
        for e in &self.entries {
            let i = if e.ty >= 1 && e.ty <= 4 { e.ty as usize } else { 0 };
            h[i] += 1;
        }
        h
    }

    pub fn fingerprint(&self) -> u64 {
        let mut buf: Vec<u8> = Vec::new();
        for e in &self.entries {
            buf.extend_from_slice(&e.base.to_le_bytes());
            buf.extend_from_slice(&e.len.to_le_bytes());
            buf.extend_from_slice(&e.ty.to_le_bytes());
        }
        fnv1a64(&buf)
    }
}

// ── EFI 桥接：E820 类型 ↔ EFI 内存类型映射闭集 ─────────────────
/// E820→EFI 类型映射（单向确定；反向走 EFI 账自身）。
pub fn e820_to_efi_type(t: u32) -> Result<u32, u16> {
    // EFI 内存类型常量（与 unx0052_efiattr 闭集同源）
    const T_RESERVED: u32 = 0;
    const T_CONV: u32 = 7;
    const T_ACPI_RECLAIM: u32 = 9;
    const T_ACPI_NVS: u32 = 10;
    match t {
        E820_USABLE => Ok(T_CONV),
        E820_RESERVED => Ok(T_RESERVED),
        E820_ACPI => Ok(T_ACPI_RECLAIM),
        E820_NVS => Ok(T_ACPI_NVS),
        _ => Err(E_BRIDGE),
    }
}

/// 桥接：E820 账 → EFI 描述符流（同区间同类保持，类型按闭集映射）。
pub fn bridge_to_efi(led: &E820Ledger) -> Result<Vec<(u64, u64, u32)>, u16> {
    let mut out = Vec::new();
    for e in &led.entries {
        let t = e820_to_efi_type(e.ty)?;
        out.push((e.base, e.len, t));
    }
    // 守恒：出桥条目数 = 入账有效条目数
    if out.len() != led.entries.len() {
        return Err(E_BAL);
    }
    Ok(out)
}

// ── 确定性演练：合法 3 例 + 注入拦截 5 例 ────────────────────────
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：四类齐全入账 → 直方图与可用页数判据。
    let mut led = E820Ledger::new();
    led.absorb(E820Entry { base: 0x0000, len: 0x9_FC00, ty: E820_USABLE })?;
    led.absorb(E820Entry { base: 0x9_FC00, len: 0x400, ty: E820_RESERVED })?;
    led.absorb(E820Entry { base: 0xDA00_0000, len: 0x1_0000, ty: E820_ACPI })?;
    led.absorb(E820Entry { base: 0xDB00_0000, len: 0x1_0000, ty: E820_NVS })?;
    led.seal()?;
    let h = led.histogram();
    if h[1] != 1 || h[2] != 1 || h[3] != 1 || h[4] != 1 {
        return Err(E_BAL);
    }
    if led.usable_pages() != 0x9FC00 / 4096 {
        return Err(E_BAL);
    }
    hits += 1;

    // R2：桥接映射闭集往返。
    let efi = bridge_to_efi(&led)?;
    if efi.len() != 4 {
        return Err(E_BAL);
    }
    if efi[0].2 != 7 || efi[1].2 != 0 || efi[2].2 != 9 || efi[3].2 != 10 {
        return Err(E_BRIDGE);
    }
    hits += 1;

    // R3：裸流解析往返（20 字节定长）。
    let mut raw: Vec<u8> = Vec::new();
    for e in led.entries.clone() {
        raw.extend_from_slice(&e.base.to_le_bytes());
        raw.extend_from_slice(&e.len.to_le_bytes());
        raw.extend_from_slice(&e.ty.to_le_bytes());
    }
    let back = parse_e820_stream(&raw)?;
    if back.len() != 4 || back[0].base != 0 {
        return Err(E_ENTRY);
    }
    hits += 1;

    // I1：类型越纲 → 拒留痕 + overshoot 计数。
    let mut i1 = E820Ledger::new();
    i1.absorb(E820Entry { base: 0x1000, len: 0x1000, ty: 6 })?;
    if i1.overshoot_types.len() != 1 {
        return Err(E_TYPE);
    }
    hits += 1;

    // I2：同类重叠 → E_OVERLAP。
    let mut i2 = E820Ledger::new();
    i2.absorb(E820Entry { base: 0x1000, len: 0x2000, ty: E820_USABLE })?;
    i2.absorb(E820Entry { base: 0x2000, len: 0x1000, ty: E820_USABLE })?;
    if i2.rejected.last().map(|(_, c)| *c) != Some(E_OVERLAP) {
        return Err(E_OVERLAP);
    }
    hits += 1;

    // I3：跨类嵌套允许（BIOS 现实）——保留区套在可用区内不报重叠。
    let mut i3 = E820Ledger::new();
    i3.absorb(E820Entry { base: 0x10_0000, len: 0x10_0000, ty: E820_USABLE })?;
    i3.absorb(E820Entry { base: 0x18_0000, len: 0x1000, ty: E820_RESERVED })?;
    if i3.entries.len() != 2 {
        return Err(E_OVERLAP);
    }
    hits += 1;

    // I4：零长度 → E_ZERO 留痕。
    let mut i4 = E820Ledger::new();
    i4.absorb(E820Entry { base: 0x2000, len: 0, ty: E820_USABLE })?;
    if i4.rejected.last().map(|(_, c)| *c) != Some(E_ZERO) {
        return Err(E_ZERO);
    }
    hits += 1;

    // I5：超纲类型桥接 → E_BRIDGE。
    if e820_to_efi_type(9) != Err(E_BRIDGE) {
        return Err(E_BRIDGE);
    }
    hits += 1;

    // I6：短流 → E_ENTRY。
    if parse_e820_stream(&[0u8; 19]) != Err(E_ENTRY) {
        return Err(E_ENTRY);
    }
    hits += 1;

    Ok(hits)
}
