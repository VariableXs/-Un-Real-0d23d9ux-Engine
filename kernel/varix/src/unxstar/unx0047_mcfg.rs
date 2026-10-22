//! UNX-F0047 · MCFG 表与 PCIe ECAM 基址固化 · 诊断段 0x2000
//! 验收锚点：UNX-F0047-J1 ECAM 基址/段号/总线范围与真机一致，
//! 配置空间首字节读取判据通过；纯功能代码 ≥240 行。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x2000;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_SIG: u16 = SEG_BASE + 1;      // MCFG 签名非法
const E_LEN: u16 = SEG_BASE + 2;      // 表长不足以容纳头/条目
const E_ENTRY_ODD: u16 = SEG_BASE + 3; // 条目区长度非 16 的倍数
const E_BUS_RANGE: u16 = SEG_BASE + 4; // 起始总线 > 结束总线
const E_RESERVED: u16 = SEG_BASE + 5;  // 保留字段非零（严格模式）
const E_OVERLAP: u16 = SEG_BASE + 6;   // ECAM 区间重叠
const E_ALIGN: u16 = SEG_BASE + 7;     // 基址非 1MiB 对齐
const E_SEG_DUP: u16 = SEG_BASE + 8;   // 段号重复
const E_OFFSET: u16 = SEG_BASE + 9;    // 配置偏移越出该段 ECAM 窗
const E_NOTFOUND: u16 = SEG_BASE + 10; // 段号未登记
const E_EMPTY: u16 = SEG_BASE + 11;    // 条目区为空
const E_BAL: u16 = SEG_BASE + 12;      // 登记账与固化账对账不平

/// MCFG 表签名 "MCFG"（小端字节序逐字节比对）。
pub const MCFG_SIG: [u8; 4] = [0x4D, 0x43, 0x46, 0x47];

/// ACPI SDT 头长 36 字节；MCFG 条目定长 16 字节。
pub const SDT_HDR_LEN: usize = 36;
pub const ENTRY_LEN: usize = 16;

/// ECAM 基址最低 1MiB 对齐（每总线 256 设备 × 8 功能 × 4KiB = 1MiB）。
pub const ECAM_ALIGN_BYTES: u64 = 1024 * 1024;

/// 每总线 ECAM 窗口大小：256 设备 × 8 功能 × 4096 字节。
pub const ECAM_BUS_WINDOW: u64 = 256 * 8 * 4096;

// ── FNV-1a 64 ────────────────────────────────────────────────
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 诊断详情：把错误码翻译成中文可操作建议。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_SIG => "表签名非 \"MCFG\"：确认 ACPI 表来源与字节序，禁止按猜测偏移硬读",
        E_LEN => "表长不足：length 字段小于 36+16，先核对 ACPI SDT 头再解析条目",
        E_ENTRY_ODD => "条目区长度非 16 的倍数：表被截断或固件损坏，拒绝半条目",
        E_BUS_RANGE => "起始总线号大于结束总线号：固件条目非法，固化前必须拦截",
        E_RESERVED => "条目保留字段非零：严格模式下视为固件违规，登记后拒绝",
        E_OVERLAP => "ECAM 物理区间重叠：多段基址窗口相交，映射前必须拆账裁决",
        E_ALIGN => "ECAM 基址未按 1MiB 对齐：无法按总线粒度切窗，拒绝固化",
        E_SEG_DUP => "PCI 段号重复：同段两条基址声明，取首个并登记冲突",
        E_OFFSET => "配置偏移越出该段总线窗：检查 B/D/F 与寄存器偏移编码",
        E_NOTFOUND => "段号未登记：先固化 MCFG 条目再访问该段配置空间",
        E_EMPTY => "MCFG 条目区为空：平台无 PCIe ECAM，走传统 CF8/CFC 路径",
        E_BAL => "登记账与固化账数量不平：每条登记必须恰好对应一次固化",
        _ => "未知诊断码",
    }
}

// ── 原始条目（固件布局的 Rust 投影）────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RawMcfgEntry {
    pub base: u64,        // ECAM 基址（物理）
    pub segment: u16,     // PCI 段组号
    pub bus_start: u8,    // 起始总线
    pub bus_end: u8,      // 结束总线
    pub reserved: u32,
}

impl RawMcfgEntry {
    pub fn from_bytes(b: &[u8]) -> Result<RawMcfgEntry, u16> {
        if b.len() < ENTRY_LEN {
            return Err(E_LEN);
        }
        let base = u64::from_le_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]);
        let segment = u16::from_le_bytes([b[8], b[9]]);
        Ok(RawMcfgEntry {
            base,
            segment,
            bus_start: b[10],
            bus_end: b[11],
            reserved: u32::from_le_bytes([b[12], b[13], b[14], b[15]]),
        })
    }
}

// ── 固化后的段账（经过校验、可查询）────────────────────────────
#[derive(Clone, Debug)]
pub struct EcamSegment {
    pub base: u64,
    pub segment: u16,
    pub bus_start: u8,
    pub bus_end: u8,
    pub window_end: u64, // base + (bus_end - bus_start + 1) × 1MiB
}

impl EcamSegment {
    /// 该段覆盖的总线数。
    pub fn bus_count(&self) -> u32 {
        (self.bus_end as u32) - (self.bus_start as u32) + 1
    }

    /// 物理区间 [base, window_end) 是否包含 addr。
    pub fn contains(&self, addr: u64) -> bool {
        addr >= self.base && addr < self.window_end
    }
}

/// 判据：条目 → 固化段，逐字段校验。
pub fn solidify(e: &RawMcfgEntry, strict_reserved: bool) -> Result<EcamSegment, u16> {
    if e.bus_start > e.bus_end {
        return Err(E_BUS_RANGE);
    }
    if strict_reserved && e.reserved != 0 {
        return Err(E_RESERVED);
    }
    if e.base % ECAM_ALIGN_BYTES != 0 {
        return Err(E_ALIGN);
    }
    let window_end = e
        .base
        .checked_add((e.bus_end as u64 - e.bus_start as u64 + 1) * ECAM_BUS_WINDOW)
        .ok_or(E_OVERLAP)?;
    Ok(EcamSegment {
        base: e.base,
        segment: e.segment,
        bus_start: e.bus_start,
        bus_end: e.bus_end,
        window_end,
    })
}

/// 两段 ECAM 物理区间是否相交（含端点闭开语义）。
pub fn windows_overlap(a: &EcamSegment, b: &EcamSegment) -> bool {
    a.base < b.window_end && b.base < a.window_end
}

// ── 解析器：从 ACPI 表字节流提取全部条目 ────────────────────────
pub struct McfgParseReport {
    pub sig_ok: bool,
    pub revision: u8,
    pub length: u32,
    pub entries_raw: Vec<RawMcfgEntry>,
    pub fingerprint: u64,
}

pub fn parse_mcfg(table: &[u8]) -> Result<McfgParseReport, u16> {
    if table.len() < SDT_HDR_LEN {
        return Err(E_LEN);
    }
    if table[0..4] != MCFG_SIG {
        return Err(E_SIG);
    }
    let length = u32::from_le_bytes([table[4], table[5], table[6], table[7]]) as usize;
    if length < SDT_HDR_LEN + ENTRY_LEN || length > table.len() {
        return Err(E_LEN);
    }
    let revision = table[8];
    let body = &table[SDT_HDR_LEN..length];
    if body.len() % ENTRY_LEN != 0 {
        return Err(E_ENTRY_ODD);
    }
    let mut entries_raw = Vec::new();
    let mut off = 0usize;
    while off < body.len() {
        entries_raw.push(RawMcfgEntry::from_bytes(&body[off..off + ENTRY_LEN])?);
        off += ENTRY_LEN;
    }
    if entries_raw.is_empty() {
        return Err(E_EMPTY);
    }
    let fingerprint = fnv1a64(&table[..length]);
    Ok(McfgParseReport {
        sig_ok: true,
        revision,
        length: length as u32,
        entries_raw,
        fingerprint,
    })
}

// ── 固化账：解析 → 校验 → 去重 → 登记 ─────────────────────────
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SolidifyStep {
    Registered,
    Rejected(u16),
}

pub struct EcamLedger {
    pub segments: Vec<EcamSegment>,
    pub rejected: Vec<(u16, u16)>, // (segment_group, 诊断码)
    pub strict_reserved: bool,
}

impl EcamLedger {
    pub fn new(strict_reserved: bool) -> EcamLedger {
        EcamLedger {
            segments: Vec::new(),
            rejected: Vec::new(),
            strict_reserved,
        }
    }

    /// 逐条固化：失败条目记账不中断，同段重复取首个并记 E_SEG_DUP。
    pub fn absorb(&mut self, entries: &[RawMcfgEntry]) -> Vec<SolidifyStep> {
        let mut steps = Vec::new();
        for e in entries {
            match solidify(e, self.strict_reserved) {
                Ok(seg) => {
                    if self.segments.iter().any(|s| s.segment == seg.segment) {
                        self.rejected.push((seg.segment, E_SEG_DUP));
                        steps.push(SolidifyStep::Rejected(E_SEG_DUP));
                        continue;
                    }
                    if self
                        .segments
                        .iter()
                        .any(|s| windows_overlap(s, &seg))
                    {
                        self.rejected.push((seg.segment, E_OVERLAP));
                        steps.push(SolidifyStep::Rejected(E_OVERLAP));
                        continue;
                    }
                    self.segments.push(seg);
                    steps.push(SolidifyStep::Registered);
                }
                Err(c) => {
                    self.rejected.push((e.segment, c));
                    steps.push(SolidifyStep::Rejected(c));
                }
            }
        }
        steps
    }

    pub fn find_segment(&self, segment: u16) -> Option<&EcamSegment> {
        self.segments.iter().find(|s| s.segment == segment)
    }

    /// 配置空间首字节物理地址 = base + bus 偏移窗 + D/F 编码。
    /// J1 判据的读取路径：先验段登记，再验总线在界，再验偏移不越窗。
    pub fn cfg_addr(
        &self,
        segment: u16,
        bus: u8,
        device: u8,
        func: u8,
        offset: u16,
    ) -> Result<u64, u16> {
        if device > 31 || func > 7 {
            return Err(E_OFFSET);
        }
        let seg = self.find_segment(segment).ok_or(E_NOTFOUND)?;
        if bus < seg.bus_start || bus > seg.bus_end {
            return Err(E_BUS_RANGE);
        }
        let bus_off = (bus as u64 - seg.bus_start as u64) * ECAM_BUS_WINDOW;
        let dev_off = (device as u64) * 8 * 4096 + (func as u64) * 4096;
        let off = bus_off + dev_off + offset as u64;
        if offset >= 4096 || off >= seg.window_end - seg.base {
            return Err(E_OFFSET);
        }
        Ok(seg.base + off)
    }

    /// 登记账对账：被拒条目 + 已登记条目 ≡ 吸收总数。
    pub fn reconcile(&self, absorbed_total: usize) -> Result<(), u16> {
        if self.segments.len() + self.rejected.len() != absorbed_total {
            return Err(E_BAL);
        }
        Ok(())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut buf: Vec<u8> = Vec::new();
        for s in &self.segments {
            buf.extend_from_slice(&s.base.to_le_bytes());
            buf.extend_from_slice(&s.segment.to_le_bytes());
            buf.push(s.bus_start);
            buf.push(s.bus_end);
        }
        for (g, c) in &self.rejected {
            buf.extend_from_slice(&g.to_le_bytes());
            buf.extend_from_slice(&c.to_le_bytes());
        }
        fnv1a64(&buf)
    }
}

/// 一站式入口：原始表字节 → 解析 → 固化账。
pub fn bring_up(table: &[u8], strict_reserved: bool) -> Result<EcamLedger, u16> {
    let report = parse_mcfg(table)?;
    let mut ledger = EcamLedger::new(strict_reserved);
    ledger.absorb(&report.entries_raw);
    ledger.reconcile(report.entries_raw.len())?;
    Ok(ledger)
}

// ── 自演练：真实路径 + 注入对拍（确定性，零随机源）───────────────
/// 构造一张最小合法 MCFG 表（供演练与本仓 verify 复用）。
pub fn synth_table(entries: &[RawMcfgEntry]) -> Vec<u8> {
    let mut t: Vec<u8> = Vec::new();
    t.extend_from_slice(&MCFG_SIG);
    let len = (SDT_HDR_LEN + entries.len() * ENTRY_LEN) as u32;
    t.extend_from_slice(&len.to_le_bytes());
    t.push(1); // revision
    t.push(0); // checksum（本层不做校验和复核，属 A1 域职责）
    t.extend_from_slice(&[0x56, 0x41, 0x52, 0x49, 0x58, 0x00]); // OEMID "VARIX"
    for _ in 0..8 {
        t.push(0); // OEM Table ID
    }
    t.extend_from_slice(&[0, 0, 0, 1]); // OEM Rev
    t.extend_from_slice(&[0, 0, 0, 0]); // Creator Rev
    for e in entries {
        t.extend_from_slice(&e.base.to_le_bytes());
        t.extend_from_slice(&e.segment.to_le_bytes());
        t.push(e.bus_start);
        t.push(e.bus_end);
        t.extend_from_slice(&e.reserved.to_le_bytes());
    }
    t
}

/// 确定性演练：合法路径 3 例 + 注入拦截 6 例，全部必须命中预期。
/// 返回 Ok(命中数)；任一未命中返回对应诊断码。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：单段合法表，固化 + 配置空间首字节地址判据。
    let e1 = RawMcfgEntry {
        base: 0xE000_0000,
        segment: 0,
        bus_start: 0,
        bus_end: 255,
        reserved: 0,
    };
    let t1 = synth_table(&[e1]);
    let led1 = bring_up(&t1, true)?;
    let addr = led1.cfg_addr(0, 0, 2, 0, 0)?; // 00:02.0 首 4 字节（VID/DID）
    if addr != 0xE000_0000 + 2 * 8 * 4096 {
        return Err(E_OFFSET);
    }
    hits += 1;

    // R2：双段不重叠表，登记 2，账平。
    let e2 = RawMcfgEntry {
        base: 0xE100_0000,
        segment: 1,
        bus_start: 0,
        bus_end: 15,
        reserved: 0,
    };
    let led2 = bring_up(&synth_table(&[e1, e2]), true)?;
    if led2.segments.len() != 2 {
        return Err(E_BAL);
    }
    led2.reconcile(2)?;
    hits += 1;

    // R3：总线子域（非 0..255），窗口端点判据。
    let e3 = RawMcfgEntry {
        base: 0xE000_0000,
        segment: 0,
        bus_start: 16,
        bus_end: 31,
        reserved: 0,
    };
    let led3 = bring_up(&synth_table(&[e3]), true)?;
    let s3 = led3.find_segment(0).ok_or(E_NOTFOUND)?;
    if s3.window_end != 0xE000_0000 + 16 * ECAM_BUS_WINDOW {
        return Err(E_OVERLAP);
    }
    // 界外总线必须拒。
    if led3.cfg_addr(0, 15, 0, 0, 0).is_ok() {
        return Err(E_BUS_RANGE);
    }
    hits += 1;

    // I1：签名注入 → E_SIG。
    let mut bad = synth_table(&[e1]);
    bad[0] = 0x58; // "XCFG"
    if !matches!(bring_up(&bad, true), Err(E_SIG)) {
        return Err(E_SIG);
    }
    hits += 1;

    // I2：起始总线 > 结束总线 → 固化拒绝 E_BUS_RANGE。
    let bad_e = RawMcfgEntry { bus_start: 8, bus_end: 4, ..e1 };
    let mut led_i2 = EcamLedger::new(true);
    led_i2.absorb(&[bad_e]);
    if led_i2.rejected.len() != 1 || led_i2.rejected[0].1 != E_BUS_RANGE {
        return Err(E_BUS_RANGE);
    }
    hits += 1;

    // I3：保留字段非零（严格）→ E_RESERVED；宽松下放行。
    let rez = RawMcfgEntry { reserved: 0xFFFF_FFFF, ..e1 };
    let mut led_i3 = EcamLedger::new(true);
    led_i3.absorb(&[rez]);
    if led_i3.rejected.len() != 1 {
        return Err(E_RESERVED);
    }
    let mut led_i3b = EcamLedger::new(false);
    led_i3b.absorb(&[rez]);
    if led_i3b.segments.len() != 1 {
        return Err(E_RESERVED);
    }
    hits += 1;

    // I4：1MiB 未对齐基址 → E_ALIGN。
    let unaligned = RawMcfgEntry { base: 0xE000_0400, ..e1 };
    let mut led_i4 = EcamLedger::new(false);
    led_i4.absorb(&[unaligned]);
    if led_i4.rejected.len() != 1 || led_i4.rejected[0].1 != E_ALIGN {
        return Err(E_ALIGN);
    }
    hits += 1;

    // I5：窗口重叠 → 后者 E_OVERLAP。
    let ov = RawMcfgEntry { base: 0xE000_0000, segment: 1, bus_start: 0, bus_end: 7, reserved: 0 };
    let mut led_i5 = EcamLedger::new(false);
    led_i5.absorb(&[e1, ov]);
    if led_i5.segments.len() != 1 || led_i5.rejected.len() != 1 {
        return Err(E_OVERLAP);
    }
    hits += 1;

    // I6：device/func 越界 → E_OFFSET。
    if led1.cfg_addr(0, 0, 32, 0, 0).is_ok() || led1.cfg_addr(0, 0, 0, 8, 0).is_ok() {
        return Err(E_OFFSET);
    }
    if led1.cfg_addr(0, 0, 0, 0, 4096).is_ok() {
        return Err(E_OFFSET);
    }
    hits += 1;

    Ok(hits)
}
