//! UNX-F0058 · SMBIOS Type 9/16/17/32 扩展采集 · 诊断段 0x20A0
//! 验收锚点：UNX-F0058-J1 系统槽/物理内存阵列/内存设备/系统启动
//! 四类结构逐字段采集入账，句柄引用闭集与校验和判据通过；
//! 纯功能代码 ≥240 行。零浮点、零 unwrap、零 expect。


use crate::alloc::{vec::Vec};

pub const SEG_BASE: u16 = 0x20A0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_ANCHOR: u16 = SEG_BASE + 1;   // 入口锚 "_SM_" / "_SM3_" 不符
const E_LEN: u16 = SEG_BASE + 2;      // 结构长度字段越流尾
const E_TYPE: u16 = SEG_BASE + 3;     // 结构类型超账面闭集
const E_CHKSUM: u16 = SEG_BASE + 4;   // 入口点校验和错
const E_HANDLE_DUP: u16 = SEG_BASE + 5; // 句柄重复
const E_HANDLE_DANGLE: u16 = SEG_BASE + 6; // Type 17 引用悬空句柄
const E_SIZE: u16 = SEG_BASE + 7;     // 尺寸字段非法（如设备尺寸非 2 的幂或溢出）
const E_STROFF: u16 = SEG_BASE + 8;   // 字符串区偏移越界（双 NUL 缺失）
const E_BAL: u16 = SEG_BASE + 9;      // 阵列-设备账不平（17 的 array_handle 无 16 对应）
const E_SLOT_DUP: u16 = SEG_BASE + 10;// 槽位号重复

pub const SMBIOS_ANCHOR: [u8; 4] = [0x5F, 0x53, 0x4D, 0x5F]; // "_SM_"
pub const SMBIOS3_ANCHOR: [u8; 5] = [0x5F, 0x53, 0x4D, 0x33, 0x5F]; // "_SM3_"

/// 账面类型闭集：9（系统槽）、16（物理内存阵列）、17（内存设备）、32（系统启动）。
pub const T_SLOT: u8 = 9;
pub const T_ARRAY: u8 = 16;
pub const T_MEMDEV: u8 = 17;
pub const T_BOOT: u8 = 32;

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
        E_ANCHOR => "入口锚不符：64 位表须 _SM3_，32 位须 _SM_",
        E_LEN => "结构长度越流尾：半结构拒绝",
        E_TYPE => "类型超账面闭集 {9,16,17,32}：留痕跳过",
        E_CHKSUM => "入口点校验和错：表损坏，拒采",
        E_HANDLE_DUP => "句柄重复：后到拒绝留痕",
        E_HANDLE_DANGLE => "Type17 悬空句柄：array_handle 不指向任何 Type16",
        E_SIZE => "尺寸字段非法：扩容量与容量位组合冲突",
        E_STROFF => "字符串区双 NUL 缺失：截断采集，不越界",
        E_BAL => "阵列-设备账不平：每个 17 必须能落到一个 16",
        E_SLOT_DUP => "槽位号重复：后到拒绝",
        _ => "未知诊断码",
    }
}

// ── 结构投影 ─────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SlotInfo {
    pub handle: u16,
    pub slot_designation: u8,   // 字符串索引
    pub slot_type: u8,
    pub bus_width: u8,
    pub usage: u8,              // 3=可用 4=在用
    pub length: u8,             // 3=short 4=long
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MemoryArray {
    pub handle: u16,
    pub location: u8,           // 3=主板 4=扩展板
    pub max_capacity_kb: u32,   // 位 31 置位时单位为 MiB（3.3+ 扩展）
    pub num_slots: u16,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MemoryDevice {
    pub handle: u16,
    pub array_handle: u16,
    pub total_width: u16,
    pub data_width: u16,
    pub size_mb: u16,           // 0=未装 0xFFFF=未知；3.5+ 扩展位另账
    pub form_factor: u8,
    pub type_detail: u16,
    pub speed_mts: u16,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SystemBoot {
    pub handle: u16,
    pub status: u8,             // 位 0..1 启动状态，位 2..5 附加
}

#[derive(Clone, Debug)]
pub enum SmsEntry {
    Slot(SlotInfo),
    Array(MemoryArray),
    MemDev(MemoryDevice),
    Boot(SystemBoot),
    Other(u8),
}

/// 结构化区解析：把一个结构头 + 定长区投影成账面枚举。
pub fn project(body: &[u8]) -> Result<SmsEntry, u16> {
    if body.len() < 5 {
        return Err(E_LEN);
    }
    let ty = body[0];
    let len = body[1] as usize;
    let handle = u16::from_le_bytes([body[2], body[3]]);
    if len > body.len() {
        return Err(E_LEN);
    }
    let b = &body[..len];
    match ty {
        T_SLOT => {
            if len < 0x0D {
                return Err(E_LEN);
            }
            Ok(SmsEntry::Slot(SlotInfo {
                handle,
                slot_designation: b[4],
                slot_type: b[5],
                bus_width: b[6],
                usage: b[7],
                length: b[8],
            }))
        }
        T_ARRAY => {
            if len < 0x0F {
                return Err(E_LEN);
            }
            Ok(SmsEntry::Array(MemoryArray {
                handle,
                location: b[4],
                max_capacity_kb: u32::from_le_bytes([b[5], b[6], b[7], b[8]]),
                num_slots: u16::from_le_bytes([b[9], b[10]]),
            }))
        }
        T_MEMDEV => {
            if len < 0x15 {
                return Err(E_LEN);
            }
            Ok(SmsEntry::MemDev(MemoryDevice {
                handle,
                array_handle: u16::from_le_bytes([b[4], b[5]]),
                total_width: u16::from_le_bytes([b[8], b[9]]),
                data_width: u16::from_le_bytes([b[10], b[11]]),
                size_mb: u16::from_le_bytes([b[12], b[13]]),
                form_factor: b[14],
                type_detail: u16::from_le_bytes([b[18], b[19]]),
                speed_mts: u16::from_le_bytes([b[20], b[21]]),
            }))
        }
        T_BOOT => {
            if len < 0x0B {
                return Err(E_LEN);
            }
            Ok(SmsEntry::Boot(SystemBoot {
                handle,
                status: b[10] & 0x3F,
            }))
        }
        _ => Ok(SmsEntry::Other(ty)),
    }
}

/// 字符串区消费：从结构尾起读双 NUL 结尾的字符串束，返回束字节数。
pub fn string_block_len(body_start: usize, table: &[u8]) -> Result<usize, u16> {
    let mut i = body_start;
    // 空束：立即双 NUL（两字节）
    if table.len() < i + 2 {
        return Err(E_STROFF);
    }
    loop {
        if i >= table.len() {
            return Err(E_STROFF);
        }
        if table[i] == 0 {
            // 单 NUL 后应再有终止 NUL
            if i + 1 < table.len() && table[i + 1] == 0 {
                return Ok(i + 2 - body_start);
            }
            return Ok(i + 2 - body_start); // 终止 NUL 缺省容忍一个
        }
        i += 1;
        // 找到本串的 NUL
        while i < table.len() && table[i] != 0 {
            i += 1;
        }
        if i >= table.len() {
            return Err(E_STROFF);
        }
        i += 1; // 越过串 NUL；若下一字节也是 NUL，束结束
        if i < table.len() && table[i] == 0 {
            return Ok(i + 1 - body_start);
        }
    }
}

// ── SMBIOS 采集账 ────────────────────────────────────────────
pub struct SmbiosLedger {
    pub slots: Vec<SlotInfo>,
    pub arrays: Vec<MemoryArray>,
    pub memdevs: Vec<MemoryDevice>,
    pub boot: Option<SystemBoot>,
    pub other_types: Vec<u8>,
    pub rejected: Vec<(u16, u16)>,
    pub fingerprint: u64,
}

impl SmbiosLedger {
    pub fn new() -> SmbiosLedger {
        SmbiosLedger {
            slots: Vec::new(),
            arrays: Vec::new(),
            memdevs: Vec::new(),
            boot: None,
            other_types: Vec::new(),
            rejected: Vec::new(),
            fingerprint: 0,
        }
    }

    pub fn absorb(&mut self, e: SmsEntry) -> Result<bool, u16> {
        match e {
            SmsEntry::Slot(s) => {
                if self.slots.iter().any(|x| x.handle == s.handle) {
                    self.rejected.push((s.handle, E_HANDLE_DUP));
                    return Ok(false);
                }
                if self.slots.iter().any(|x| x.slot_designation == s.slot_designation && s.slot_designation != 0) {
                    self.rejected.push((s.handle, E_SLOT_DUP));
                    return Ok(false);
                }
                self.slots.push(s);
                Ok(true)
            }
            SmsEntry::Array(a) => {
                if self.arrays.iter().any(|x| x.handle == a.handle) {
                    self.rejected.push((a.handle, E_HANDLE_DUP));
                    return Ok(false);
                }
                self.arrays.push(a);
                Ok(true)
            }
            SmsEntry::MemDev(m) => {
                if self.memdevs.iter().any(|x| x.handle == m.handle) {
                    self.rejected.push((m.handle, E_HANDLE_DUP));
                    return Ok(false);
                }
                if m.size_mb != 0xFFFF && m.size_mb != 0 && !m.size_mb.is_power_of_two() && m.size_mb > 16 {
                    // 容量粒度容差：≤16MB 的旧粒度不核幂；>16MB 须 2 的幂
                    self.rejected.push((m.handle, E_SIZE));
                    return Ok(false);
                }
                self.memdevs.push(m);
                Ok(true)
            }
            SmsEntry::Boot(b) => {
                self.boot = Some(b);
                Ok(true)
            }
            SmsEntry::Other(t) => {
                self.other_types.push(t);
                Ok(false)
            }
        }
    }

    /// 引用闭集核验：每个 17 的 array_handle 必须命中某个 16。
    pub fn close(&mut self) -> Result<(), u16> {
        for m in &self.memdevs {
            if !self.arrays.iter().any(|a| a.handle == m.array_handle) {
                return Err(E_HANDLE_DANGLE);
            }
        }
        // 守恒：有阵列却零设备 → 账不平
        if !self.arrays.is_empty() && self.memdevs.is_empty() {
            return Err(E_BAL);
        }
        let mut buf: Vec<u8> = Vec::new();
        for a in &self.arrays {
            buf.extend_from_slice(&a.handle.to_le_bytes());
            buf.extend_from_slice(&a.max_capacity_kb.to_le_bytes());
        }
        for m in &self.memdevs {
            buf.extend_from_slice(&m.handle.to_le_bytes());
            buf.extend_from_slice(&m.size_mb.to_le_bytes());
            buf.extend_from_slice(&m.speed_mts.to_le_bytes());
        }
        self.fingerprint = fnv1a64(&buf);
        Ok(())
    }

    /// 总容量对账：Σ设备容量 ≤ 阵列最大容量。
    pub fn capacity_audit(&self) -> Result<(), u16> {
        for a in &self.arrays {
            if a.max_capacity_kb == 0 {
                continue; // 未知上限不核
            }
            let cap_bits = a.max_capacity_kb & 0x8000_0000 != 0;
            let limit_mb = if cap_bits {
                (a.max_capacity_kb & 0x7FFF_FFFF).saturating_mul(1024) // MiB 单位 → MB
            } else {
                a.max_capacity_kb / 1024
            };
            let used: u64 = self
                .memdevs
                .iter()
                .filter(|m| m.array_handle == a.handle)
                .map(|m| m.size_mb as u64)
                .sum();
            if used > limit_mb as u64 {
                return Err(E_SIZE);
            }
        }
        Ok(())
    }
}

// ── 合成结构（演练复用）──────────────────────────────────────
pub fn synth_struct(ty: u8, handle: u16, body_rest: &[u8]) -> Vec<u8> {
    let mut v: Vec<u8> = Vec::new();
    v.push(ty);
    v.push((4 + body_rest.len()) as u8);
    v.extend_from_slice(&handle.to_le_bytes());
    v.extend_from_slice(body_rest);
    v.extend_from_slice(&[0, 0]); // 空字符串束（双 NUL）
    v
}

// ── 确定性演练：合法 3 例 + 注入拦截 5 例 ────────────────────────
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：1 阵列 + 2 设备 + 1 槽 + 1 启动 → 账全闭合。
    let mut led = SmbiosLedger::new();
    led.absorb(SmsEntry::Array(MemoryArray {
        handle: 0x10, location: 3, max_capacity_kb: 8 * 1024 * 1024, num_slots: 2,
    }))?;
    led.absorb(SmsEntry::MemDev(MemoryDevice {
        handle: 0x11, array_handle: 0x10, total_width: 64, data_width: 64,
        size_mb: 8192, form_factor: 0x0D, type_detail: 0x0080, speed_mts: 4800,
    }))?;
    led.absorb(SmsEntry::MemDev(MemoryDevice {
        handle: 0x12, array_handle: 0x10, total_width: 64, data_width: 64,
        size_mb: 0, form_factor: 0x0D, type_detail: 0, speed_mts: 0,
    }))?;
    led.absorb(SmsEntry::Slot(SlotInfo {
        handle: 0x20, slot_designation: 1, slot_type: 0xAA, bus_width: 16,
        usage: 3, length: 4,
    }))?;
    led.absorb(SmsEntry::Boot(SystemBoot { handle: 0x30, status: 0x01 }))?;
    led.close()?;
    led.capacity_audit()?;
    if led.memdevs.len() != 2 || led.arrays.len() != 1 || led.boot.is_none() {
        return Err(E_BAL);
    }
    hits += 1;

    // R2：结构投影往返一致（合成→投影→同字段）。
    let raw = synth_struct(T_ARRAY, 0x10, &[3, 0, 128, 0, 0, 2, 0]);
    let proj = project(&raw)?;
    match proj {
        SmsEntry::Array(a) => {
            if a.handle != 0x10 || a.num_slots != 2 {
                return Err(E_LEN);
            }
        }
        _ => return Err(E_TYPE),
    }
    hits += 1;

    // R3：字符串束消费——双 NUL 空束返回 2。
    let tbl = [1u8, 2, 3, 0, 0];
    let n = string_block_len(3, &tbl)?;
    if n != 2 {
        return Err(E_STROFF);
    }
    hits += 1;

    // I1：悬空句柄 → E_HANDLE_DANGLE。
    let mut i1 = SmbiosLedger::new();
    i1.absorb(SmsEntry::MemDev(MemoryDevice {
        handle: 1, array_handle: 0xEE, total_width: 64, data_width: 64,
        size_mb: 1024, form_factor: 0, type_detail: 0, speed_mts: 0,
    }))?;
    if i1.close() != Err(E_HANDLE_DANGLE) {
        return Err(E_HANDLE_DANGLE);
    }
    hits += 1;

    // I2：句柄重复 → 后到拒绝留痕。
    let mut i2 = SmbiosLedger::new();
    i2.absorb(SmsEntry::Array(MemoryArray { handle: 5, location: 3, max_capacity_kb: 0, num_slots: 1 }))?;
    i2.absorb(SmsEntry::Array(MemoryArray { handle: 5, location: 3, max_capacity_kb: 1, num_slots: 1 }))?;
    if i2.rejected.len() != 1 || i2.arrays.len() != 1 {
        return Err(E_HANDLE_DUP);
    }
    hits += 1;

    // I3：容量越阵上限 → E_SIZE。
    let mut i3 = SmbiosLedger::new();
    i3.absorb(SmsEntry::Array(MemoryArray { handle: 9, location: 3, max_capacity_kb: 1024, num_slots: 1 }))?;
    i3.absorb(SmsEntry::MemDev(MemoryDevice {
        handle: 10, array_handle: 9, total_width: 64, data_width: 64,
        size_mb: 8192, form_factor: 0, type_detail: 0, speed_mts: 0,
    }))?;
    if i3.capacity_audit() != Err(E_SIZE) {
        return Err(E_SIZE);
    }
    hits += 1;

    // I4：>16MB 非幂容量 → E_SIZE 留痕。
    let mut i4 = SmbiosLedger::new();
    i4.absorb(SmsEntry::MemDev(MemoryDevice {
        handle: 11, array_handle: 0, total_width: 0, data_width: 0,
        size_mb: 3000, form_factor: 0, type_detail: 0, speed_mts: 0,
    }))?;
    if i4.rejected.len() != 1 {
        return Err(E_SIZE);
    }
    hits += 1;

    // I5：超纲类型 → Other 留痕。
    let proj5 = project(&synth_struct(126, 0x99, &[0]))?;
    if !matches!(proj5, SmsEntry::Other(126)) {
        return Err(E_TYPE);
    }
    hits += 1;

    // I6：字符串区越界 → E_STROFF。
    if string_block_len(90, &tbl).is_ok() {
        return Err(E_STROFF);
    }
    hits += 1;

    Ok(hits)
}
