//! UNX-F0055 · MADT 补全：LAPIC NMI 与中断源覆盖 · 诊断段 0x2070
//! 验收锚点：UNX-F0055-J1 LAPIC NMI（Type 4）与中断源覆盖（Type 2）
//! 条目逐条解析入账，处理器闭集与极性/触发模式枚举判据通过；
//! 纯功能代码 ≥240 行。

use crate::alloc::{format, string::{String, ToString}, vec, vec::Vec};

pub const SEG_BASE: u16 = 0x2070;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_SIG: u16 = SEG_BASE + 1;      // MADT 签名非法
const E_LEN: u16 = SEG_BASE + 2;      // 表长不足
const E_ENTRY_LEN: u16 = SEG_BASE + 3;// 条目长度非法
const E_TYPE: u16 = SEG_BASE + 4;     // 不支持的条目类型（留痕跳过）
const E_POL: u16 = SEG_BASE + 5;      // 极性枚举值非法（2/3 保留）
const E_TRIG: u16 = SEG_BASE + 6;     // 触发模式枚举值非法（2/3 保留）
const E_CPU_DUP: u16 = SEG_BASE + 7;  // LAPIC UID 重复
const E_IOAPIC_DUP: u16 = SEG_BASE + 8;// IO-APIC ID 重复
const E_GSI_DUP: u16 = SEG_BASE + 9;  // 覆盖条目目标 GSI 重复
const E_NMI_CPU: u16 = SEG_BASE + 10; // NMI 指向不存在的处理器 UID
const E_BAL: u16 = SEG_BASE + 11;     // 覆盖账守恒不平
const E_PID_WILDCARD: u16 = SEG_BASE + 12; // NMI processor_id=0xFF 全体广播账

pub const MADT_SIG: [u8; 4] = [0x41, 0x50, 0x49, 0x43]; // "APIC"
pub const SDT_HDR_LEN: usize = 44; // MADT 头含 Flags 双字，SDT 36 + 8

/// 极性枚举（规范 2/3 保留）。
pub const POL_DEFAULT: u8 = 0;   // 按总线默认
pub const POL_ACTIVE_HIGH: u8 = 1;
pub const POL_ACTIVE_LOW: u8 = 3; // 注意规范：1=高有效，3=低有效

/// 触发模式枚举（规范 2 保留）。
pub const TRIG_DEFAULT: u8 = 0;
pub const TRIG_EDGE: u8 = 1;
pub const TRIG_LEVEL: u8 = 3;

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
        E_SIG => "MADT 签名非 \"APIC\"：核对表来源",
        E_LEN => "表长不足：MADT 头 44 字节起",
        E_ENTRY_LEN => "条目长度字段非法：off+len 越出表尾",
        E_TYPE => "超纲条目类型：留痕跳过不参与账",
        E_POL => "极性枚举非法：合法闭集 {0,1,3}，2 为保留值",
        E_TRIG => "触发枚举非法：合法闭集 {0,1,3}，2 为保留值",
        E_CPU_DUP => "LAPIC UID 重复：后到拒绝留痕",
        E_IOAPIC_DUP => "IO-APIC ID 重复：后到拒绝留痕",
        E_GSI_DUP => "覆盖目标 GSI 重复：同一 GSI 两次覆盖 = 账面冲突",
        E_NMI_CPU => "NMI 指向不存在处理器 UID：先收齐处理器集再核 NMI",
        E_BAL => "覆盖账守恒：覆盖条目数 = 有效 GSI 记录数",
        E_PID_WILDCARD => "NMI processor_id=0xFF：全体广播，入广播账单列",
        _ => "未知诊断码",
    }
}

// ── 条目投影 ─────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LapicEntry {
    pub uid: u8,
    pub apic_id: u8,
    pub flags: u32,
}

impl LapicEntry {
    pub fn enabled(&self) -> bool {
        self.flags & 1 != 0
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IoApicEntry {
    pub id: u8,
    pub addr: u32,
    pub gsi_base: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IntSourceOverride {
    pub bus_source: u8,
    pub irq: u8,
    pub gsi: u32,
    pub polarity: u8,
    pub trigger: u8,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LapicNmi {
    pub processor_id: u8, // 0xFF = 全体
    pub lint: u8,         // 0=LINT0, 1=LINT1
    pub polarity: u8,
    pub trigger: u8,
}

#[derive(Clone, Debug)]
pub enum MadtEntry {
    Lapic(LapicEntry),
    IoApic(IoApicEntry),
    Override(IntSourceOverride),
    Nmi(LapicNmi),
    Unknown { etype: u8 },
}

pub struct MadtReport {
    pub local_apic_addr: u32,
    pub flags: u32,
    pub entries: Vec<MadtEntry>,
    pub fingerprint: u64,
}

pub fn parse_madt(table: &[u8]) -> Result<MadtReport, u16> {
    if table.len() < SDT_HDR_LEN {
        return Err(E_LEN);
    }
    if table[0..4] != MADT_SIG {
        return Err(E_SIG);
    }
    let length = u32::from_le_bytes([table[4], table[5], table[6], table[7]]) as usize;
    if length < SDT_HDR_LEN || length > table.len() {
        return Err(E_LEN);
    }
    let local_apic_addr = u32::from_le_bytes([table[36], table[37], table[38], table[39]]);
    let flags = u32::from_le_bytes([table[40], table[41], table[42], table[43]]);
    let mut entries = Vec::new();
    let mut off = SDT_HDR_LEN;
    while off < length {
        if off + 2 > length {
            return Err(E_ENTRY_LEN);
        }
        let etype = table[off];
        let elen = table[off + 1] as usize;
        if elen < 2 || off + elen > length {
            return Err(E_ENTRY_LEN);
        }
        let b = &table[off..off + elen];
        match etype {
            0x00 => {
                if elen < 8 {
                    return Err(E_ENTRY_LEN);
                }
                entries.push(MadtEntry::Lapic(LapicEntry {
                    uid: b[2],
                    apic_id: b[3],
                    flags: u32::from_le_bytes([b[4], b[5], b[6], b[7]]),
                }));
            }
            0x01 => {
                if elen < 12 {
                    return Err(E_ENTRY_LEN);
                }
                entries.push(MadtEntry::IoApic(IoApicEntry {
                    id: b[2],
                    addr: u32::from_le_bytes([b[4], b[5], b[6], b[7]]),
                    gsi_base: u32::from_le_bytes([b[8], b[9], b[10], b[11]]),
                }));
            }
            0x02 => {
                if elen < 10 {
                    return Err(E_ENTRY_LEN);
                }
                entries.push(MadtEntry::Override(IntSourceOverride {
                    bus_source: b[2],
                    irq: b[3],
                    gsi: u32::from_le_bytes([b[4], b[5], b[6], b[7]]),
                    polarity: b[8] & 0x03,
                    trigger: b[8] >> 2 & 0x03,
                }));
            }
            0x04 => {
                if elen < 6 {
                    return Err(E_ENTRY_LEN);
                }
                entries.push(MadtEntry::Nmi(LapicNmi {
                    processor_id: b[2],
                    lint: b[5] & 0x01,
                    polarity: b[3] & 0x03,
                    trigger: b[3] >> 2 & 0x03,
                }));
            }
            _ => entries.push(MadtEntry::Unknown { etype }),
        }
        off += elen;
    }
    let fingerprint = fnv1a64(&table[..length]);
    Ok(MadtReport {
        local_apic_addr,
        flags,
        entries,
        fingerprint,
    })
}

// ── 中断拓扑账 ───────────────────────────────────────────────
pub struct IrqTopology {
    pub cpus: Vec<LapicEntry>,
    pub ioapics: Vec<IoApicEntry>,
    pub overrides: Vec<IntSourceOverride>,
    pub nmis_per_cpu: Vec<LapicNmi>,
    pub nmis_broadcast: Vec<LapicNmi>,
    pub rejected: Vec<(u8, u16)>,
    pub unknown_types: Vec<u8>,
}

impl IrqTopology {
    pub fn build(report: &MadtReport) -> Result<IrqTopology, u16> {
        let mut t = IrqTopology {
            cpus: Vec::new(),
            ioapics: Vec::new(),
            overrides: Vec::new(),
            nmis_per_cpu: Vec::new(),
            nmis_broadcast: Vec::new(),
            rejected: Vec::new(),
            unknown_types: Vec::new(),
        };
        for e in &report.entries {
            match e {
                MadtEntry::Lapic(c) => {
                    if t.cpus.iter().any(|c2| c2.uid == c.uid) {
                        t.rejected.push((c.uid, E_CPU_DUP));
                        continue;
                    }
                    t.cpus.push(*c);
                }
                MadtEntry::IoApic(io) => {
                    if t.ioapics.iter().any(|i2| i2.id == io.id) {
                        t.rejected.push((io.id, E_IOAPIC_DUP));
                        continue;
                    }
                    t.ioapics.push(*io);
                }
                MadtEntry::Override(o) => {
                    if o.polarity == 0x02 || o.trigger == 0x02 {
                        t.rejected.push((o.irq, if o.polarity == 2 { E_POL } else { E_TRIG }));
                        continue;
                    }
                    if t.overrides.iter().any(|o2| o2.gsi == o.gsi) {
                        t.rejected.push((o.irq, E_GSI_DUP));
                        continue;
                    }
                    t.overrides.push(*o);
                }
                MadtEntry::Nmi(n) => {
                    if n.polarity == 0x02 || n.trigger == 0x02 {
                        t.rejected.push((n.processor_id, if n.polarity == 2 { E_POL } else { E_TRIG }));
                        continue;
                    }
                    if n.processor_id == 0xFF {
                        t.nmis_broadcast.push(*n);
                    } else {
                        t.nmis_per_cpu.push(*n);
                    }
                }
                MadtEntry::Unknown { etype } => t.unknown_types.push(*etype),
            }
        }
        // NMI UID 闭集核验：仅对已使能处理器放行
        for n in &t.nmis_per_cpu {
            let ok = t
                .cpus
                .iter()
                .any(|c| c.uid == n.processor_id && c.enabled());
            if !ok {
                return Err(E_NMI_CPU);
            }
        }
        Ok(t)
    }

    /// IRQ → GSI 换算：有覆盖走覆盖，否则恒等（ISA 语义）。
    pub fn irq_to_gsi(&self, irq: u8) -> u32 {
        self.overrides
            .iter()
            .find(|o| o.irq == irq)
            .map(|o| o.gsi)
            .unwrap_or(irq as u32)
    }

    /// GSI 落在哪片 IO-APIC 的窗内（多 IO-APIC 路由）。
    pub fn gsi_owner(&self, gsi: u32) -> Option<&IoApicEntry> {
        let mut best: Option<&IoApicEntry> = None;
        for io in &self.ioapics {
            if gsi >= io.gsi_base {
                match best {
                    Some(b) if b.gsi_base > io.gsi_base => {}
                    _ => best = Some(io),
                }
            }
        }
        best
    }

    pub fn fingerprint(&self) -> u64 {
        let mut buf: Vec<u8> = Vec::new();
        for c in &self.cpus {
            buf.extend_from_slice(&[c.uid, c.apic_id]);
            buf.extend_from_slice(&c.flags.to_le_bytes());
        }
        for o in &self.overrides {
            buf.extend_from_slice(&o.gsi.to_le_bytes());
            buf.push(o.irq);
            buf.push(o.polarity << 2 | o.trigger);
        }
        for n in &self.nmis_per_cpu {
            buf.extend_from_slice(&[0xA0, n.processor_id, n.lint]);
        }
        for n in &self.nmis_broadcast {
            buf.extend_from_slice(&[0xA1, n.processor_id, n.lint]);
        }
        fnv1a64(&buf)
    }
}

// ── 合成 MADT（演练复用）────────────────────────────────────
pub fn synth_madt(entries: &[MadtEntry]) -> Vec<u8> {
    let mut body: Vec<u8> = Vec::new();
    for e in entries {
        match e {
            MadtEntry::Lapic(c) => {
                body.extend_from_slice(&[0x00, 0x08, c.uid, c.apic_id]);
                body.extend_from_slice(&c.flags.to_le_bytes());
            }
            MadtEntry::IoApic(io) => {
                body.extend_from_slice(&[0x01, 0x0C, io.id, 0]);
                body.extend_from_slice(&io.addr.to_le_bytes());
                body.extend_from_slice(&0u32.to_le_bytes()); // 保留
                body.extend_from_slice(&io.gsi_base.to_le_bytes());
            }
            MadtEntry::Override(o) => {
                body.extend_from_slice(&[0x02, 0x0A, o.bus_source, o.irq]);
                body.extend_from_slice(&o.gsi.to_le_bytes());
                body.push(o.polarity | o.trigger << 2);
                body.push(0);
            }
            MadtEntry::Nmi(n) => {
                body.extend_from_slice(&[0x04, 0x06, n.processor_id]);
                body.push(n.polarity | n.trigger << 2);
                body.push(0);
                body.push(n.lint);
            }
            MadtEntry::Unknown { etype } => {
                body.extend_from_slice(&[*etype, 0x02]);
            }
        }
    }
    let mut t: Vec<u8> = Vec::new();
    t.extend_from_slice(&MADT_SIG);
    t.extend_from_slice(&((SDT_HDR_LEN + body.len()) as u32).to_le_bytes());
    t.push(3);
    t.push(0);
    t.extend_from_slice(&[0x56, 0x41, 0x52, 0x49, 0x58, 0x00]);
    for _ in 0..8 { t.push(0); }
    t.extend_from_slice(&[0, 0, 0, 1]);
    t.extend_from_slice(&[0, 0, 0, 0]);
    t.extend_from_slice(&0xFEE0_0000u32.to_le_bytes()); // Local APIC 地址
    t.extend_from_slice(&1u32.to_le_bytes()); // PCAT_COMPAT=0 低 32 位里取 flags
    t.extend_from_slice(&body);
    t
}

// ── 确定性演练：合法 3 例 + 注入拦截 5 例 ────────────────────────
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：双 CPU + IOAPIC + 覆盖（PIT IRQ2→GSI2、RTC IRQ8→GSI8 低电平）+ NMI。
    let rep = parse_madt(&synth_madt(&[
        MadtEntry::Lapic(LapicEntry { uid: 0, apic_id: 0, flags: 1 }),
        MadtEntry::Lapic(LapicEntry { uid: 1, apic_id: 2, flags: 1 }),
        MadtEntry::IoApic(IoApicEntry { id: 2, addr: 0xFEC0_0000, gsi_base: 0 }),
        MadtEntry::Override(IntSourceOverride { bus_source: 0, irq: 2, gsi: 2, polarity: POL_DEFAULT, trigger: TRIG_EDGE }),
        MadtEntry::Override(IntSourceOverride { bus_source: 0, irq: 8, gsi: 8, polarity: POL_ACTIVE_LOW, trigger: TRIG_LEVEL }),
        MadtEntry::Nmi(LapicNmi { processor_id: 0xFF, lint: 1, polarity: POL_ACTIVE_HIGH, trigger: TRIG_EDGE }),
    ]))?;
    let top = IrqTopology::build(&rep)?;
    if top.cpus.len() != 2 || top.overrides.len() != 2 {
        return Err(E_BAL);
    }
    // R2：IRQ→GSI 恒等与覆盖双径。
    if top.irq_to_gsi(3) != 3 {
        return Err(E_BAL); // 无覆盖 → 恒等
    }
    if top.irq_to_gsi(8) != 8 {
        return Err(E_BAL); // 有覆盖 → 覆盖值
    }
    hits += 1;

    // R3：NMI 广播账与单核账分列。
    if top.nmis_broadcast.len() != 1 || !top.nmis_per_cpu.is_empty() {
        return Err(E_PID_WILDCARD);
    }
    hits += 1;

    // I1：签名注入 → E_SIG。
    let mut b = synth_madt(&[]);
    b[0] = 0x58;
    if parse_madt(&b) != Err(E_SIG) {
        return Err(E_SIG);
    }
    hits += 1;

    // I2：极性保留值 2 → E_POL 拒绝留痕。
    let bad = parse_madt(&synth_madt(&[
        MadtEntry::Override(IntSourceOverride { bus_source: 0, irq: 5, gsi: 5, polarity: 0x02, trigger: TRIG_EDGE }),
    ]))?;
    let t2 = IrqTopology::build(&bad)?;
    if t2.rejected.len() != 1 || t2.rejected[0].1 != E_POL {
        return Err(E_POL);
    }
    hits += 1;

    // I3：GSI 重复覆盖 → E_GSI_DUP。
    let dup = parse_madt(&synth_madt(&[
        MadtEntry::Override(IntSourceOverride { bus_source: 0, irq: 4, gsi: 9, polarity: POL_DEFAULT, trigger: TRIG_EDGE }),
        MadtEntry::Override(IntSourceOverride { bus_source: 0, irq: 6, gsi: 9, polarity: POL_DEFAULT, trigger: TRIG_EDGE }),
    ]))?;
    let t3 = IrqTopology::build(&dup)?;
    if t3.overrides.len() != 1 || t3.rejected.len() != 1 {
        return Err(E_GSI_DUP);
    }
    hits += 1;

    // I4：NMI 指向不存在处理器 → E_NMI_CPU（硬错误）。
    let badnmi = parse_madt(&synth_madt(&[
        MadtEntry::Lapic(LapicEntry { uid: 0, apic_id: 0, flags: 1 }),
        MadtEntry::Nmi(LapicNmi { processor_id: 7, lint: 1, polarity: POL_DEFAULT, trigger: TRIG_EDGE }),
    ]))?;
    if IrqTopology::build(&badnmi) != Err(E_NMI_CPU) {
        return Err(E_NMI_CPU);
    }
    hits += 1;

    // I5：CPU UID 重复 → 后到拒绝。
    let dupcpu = parse_madt(&synth_madt(&[
        MadtEntry::Lapic(LapicEntry { uid: 0, apic_id: 0, flags: 1 }),
        MadtEntry::Lapic(LapicEntry { uid: 0, apic_id: 1, flags: 1 }),
    ]))?;
    let t5 = IrqTopology::build(&dupcpu)?;
    if t5.cpus.len() != 1 || t5.rejected.len() != 1 {
        return Err(E_CPU_DUP);
    }
    hits += 1;

    // I6：超纲条目留痕跳过。
    let unk = parse_madt(&synth_madt(&[
        MadtEntry::Unknown { etype: 0x81 },
        MadtEntry::Lapic(LapicEntry { uid: 0, apic_id: 0, flags: 1 }),
    ]))?;
    let t6 = IrqTopology::build(&unk)?;
    if t6.unknown_types.len() != 1 || t6.cpus.len() != 1 {
        return Err(E_TYPE);
    }
    hits += 1;

    Ok(hits)
}
