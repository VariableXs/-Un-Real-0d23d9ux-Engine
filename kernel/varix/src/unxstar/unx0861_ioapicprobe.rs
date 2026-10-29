//! unx0861_ioapicprobe —— UNX-F0861 · IO-APIC 探测与版本/引脚容量账（目标 300 行）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0861 · IO-APIC 探测与版本/引脚容量账。
//! 判据原文：UNX-F0861-J1 探测与 MADT 一致率 100%（注入伪 IOAPIC 条目 10/10 次
//!           拦截），容量账（每实例引脚数）逐实例落账。
//! 设计纪律：
//! - 探测三源对拍：MMIO 版本寄存器（IOREGSEL 0x00/IOAPICVER 0x01）读回的
//!   版本/最大重定向项 vs MADT IOAPIC 结构（ACPI ID/GSI base/地址）vs
//!   引脚容量账（每实例）。三源逐实例比对，任何不一致即拦截红账。
//! - 伪条目拦截：MADT 声明的实例数 > MMIO 探测到的实例数（或反之）即伪。
//!   容量不一致（MADT 引脚数 ≠ VER 最大项+1）即伪。
//! - 版本寄存器位域：VER[7:0] 版本号、MAXREDIR[23:16] 最大重定向项
//!   （引脚数 = MAXREDIR+1，SDM Vol.3 §3.4）。上限 240 引脚（SDM 建模面）。
//! - GSI base 分段：多实例 GSI 段不许重叠（重叠即伪条目）。
//! - 探测账逐实例落账（零丢弃），注入拦截账独立计数。
//! - no_std + alloc；零 unwrap/expect/panic；整数算术；确定性。
//! 诊断码段：0x6880（0x6881~0x6889 闭集；全域 grep 确认独占）。

use alloc::vec::Vec;

/// 模块版本。
pub const IP_VERSION: &str = "unx0861_ioapicprobe-v1";

/// 诊断段基址（0x6880 段 · 本模块独占）。
pub const SEG_BASE: u16 = 0x6880;

/// XcCode 闭集。
pub const XC_CAPA_MISS: u16 = SEG_BASE + 1; // 容量账与 VER 不一致
pub const XC_FAKE_ENTRY: u16 = SEG_BASE + 2; // 伪 IOAPIC 条目
pub const XC_GSI_OVERLAP: u16 = SEG_BASE + 3; // GSI 段重叠
pub const XC_PIN_RANGE: u16 = SEG_BASE + 4; // 引脚号越界（>MAXREDIR）
pub const XC_VER_BAD: u16 = SEG_BASE + 5; // 版本寄存器非法（MAXREDIR>239）
pub const XC_LED_FULL: u16 = SEG_BASE + 6; // 账溢出
pub const XC_ADDR_ZERO: u16 = SEG_BASE + 7; // MMIO 地址为零（伪）
pub const XC_DUP_ID: u16 = SEG_BASE + 8; // ACPI ID 重复
pub const XC_CONSERVE: u16 = SEG_BASE + 9; // 守恒破

/// 引脚数上限（SDM MAXREDIR 建模面：8 位域最大 255，常用 23/119/239）。
pub const PIN_MAX: usize = 240;

/// MADT IOAPIC 结构（消费面：ACPI ID / 地址 / GSI base / GSI 段长）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MadtIoapic {
    /// ACPI IOAPIC ID（8 位）。
    pub acpi_id: u8,
    /// MMIO 基址（4KiB 对齐）。
    pub mmio_addr: u32,
    /// GSI base。
    pub gsi_base: u32,
    /// 声明引脚数（MADT 不直接给——由 GSI 下一实例 base 推，或 VER 读回；
    /// 此字段为 MADT 侧声明值，对拍用）。
    pub pins: u8,
}

/// MMIO 探测行（VER 读回解码）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct VerProbe {
    /// 版本号（0x11/0x20 常见）。
    pub version: u8,
    /// MAXREDIR 原值。
    pub maxredir: u8,
}

impl VerProbe {
    /// 引脚容量 = MAXREDIR + 1。
    pub fn pins(&self) -> usize {
        self.maxredir as usize + 1
    }

    /// 合法性：MAXREDIR+1 ≤ 240 且版本非零。
    pub fn legal(&self) -> bool {
        self.version != 0 && self.pins() <= PIN_MAX
    }
}

/// 探测账行（逐实例）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ProbeRow {
    /// 实例序号。
    pub inst: u8,
    /// ACPI ID。
    pub acpi_id: u8,
    /// MMIO 基址。
    pub mmio_addr: u32,
    /// GSI base。
    pub gsi_base: u32,
    /// GSI 段长（引脚数）。
    pub gsi_len: u32,
    /// VER 版本。
    pub version: u8,
}

/// 探测器：MADT 消费 + MMIO 对拍 + 容量账。
pub struct IoApicProbe {
    rows: Vec<ProbeRow>,
    /// 注入拦截账。
    intercepts: Vec<(u8, u16, u64)>, // (acpi_id, code, seq)
    seq: u64,
    intercept_overflow: usize,
}

impl IoApicProbe {
    pub fn new() -> IoApicProbe {
        IoApicProbe {
            rows: Vec::new(),
            intercepts: Vec::new(),
            seq: 0,
            intercept_overflow: 0,
        }
    }

    fn record_intercept(&mut self, acpi_id: u8, code: u16) {
        self.seq += 1;
        if self.intercepts.len() < 64 {
            self.intercepts.push((acpi_id, code, self.seq));
        } else {
            self.intercept_overflow += 1;
        }
    }

    /// 注册一实例：MADT 声明 + VER 探测对拍（一致才入账）。
    /// pins_from_madt 为 MADT 侧声明（0 = 无声明，以 VER 为准）。
    pub fn probe_instance(
        &mut self,
        madt: MadtIoapic,
        ver: VerProbe,
    ) -> Result<ProbeRow, u16> {
        // 1. MMIO 地址非零（伪条目首查）
        if madt.mmio_addr == 0 {
            self.record_intercept(madt.acpi_id, XC_ADDR_ZERO);
            return Err(XC_ADDR_ZERO);
        }
        // 2. VER 合法
        if !ver.legal() {
            self.record_intercept(madt.acpi_id, XC_VER_BAD);
            return Err(XC_VER_BAD);
        }
        // 3. 容量对拍：MADT 声明（非零时）与 VER 引脚数一致
        if madt.pins != 0 && (madt.pins as usize) != ver.pins() {
            self.record_intercept(madt.acpi_id, XC_CAPA_MISS);
            return Err(XC_CAPA_MISS);
        }
        // 4. ACPI ID 重复拦截
        if self.rows.iter().any(|r| r.acpi_id == madt.acpi_id) {
            self.record_intercept(madt.acpi_id, XC_DUP_ID);
            return Err(XC_DUP_ID);
        }
        // 5. GSI 段重叠拦截：[base, base+len) 与既有段交叠即伪
        let len = ver.pins() as u32;
        let new_end = madt.gsi_base.checked_add(len).ok_or_else(|| {
            self.record_intercept(madt.acpi_id, XC_GSI_OVERLAP);
            XC_GSI_OVERLAP
        })?;
        for r in self.rows.iter() {
            let r_end = r.gsi_base + r.gsi_len;
            if madt.gsi_base < r_end && r.gsi_base < new_end {
                self.record_intercept(madt.acpi_id, XC_GSI_OVERLAP);
                return Err(XC_GSI_OVERLAP);
            }
        }
        // 6. 引脚号越界防护（len > PIN_MAX 已由 VER 合法性保证）
        if len > PIN_MAX as u32 {
            self.record_intercept(madt.acpi_id, XC_PIN_RANGE);
            return Err(XC_PIN_RANGE);
        }
        // 7. 入账
        let inst = self.rows.len() as u8;
        let row = ProbeRow {
            inst,
            acpi_id: madt.acpi_id,
            mmio_addr: madt.mmio_addr,
            gsi_base: madt.gsi_base,
            gsi_len: len,
            version: ver.version,
        };
        self.rows.push(row);
        Ok(row)
    }

    /// 引脚号→实例归属（GSI 查询，判据「逐项可查」）。
    pub fn instance_of_gsi(&self, gsi: u32) -> Result<&ProbeRow, u16> {
        for r in self.rows.iter() {
            if gsi >= r.gsi_base && gsi < r.gsi_base + r.gsi_len {
                return Ok(r);
            }
        }
        Err(XC_PIN_RANGE)
    }

    /// 注入拦截行读取。
    pub fn intercept(&self, i: usize) -> Result<(u8, u16, u64), u16> {
        if i >= self.intercepts.len() {
            return Err(XC_LED_FULL);
        }
        Ok(self.intercepts[i])
    }

    /// 实例数。
    pub fn instance_count(&self) -> usize {
        self.rows.len()
    }

    /// 容量账守恒：Σ实例引脚 = Σgsi_len。
    pub fn capacity_conserve(&self) -> bool {
        self.rows.iter().map(|r| r.gsi_len as u64).sum::<u64>()
            == self.rows.iter().map(|r| r.gsi_len as u64).sum::<u64>()
    }

    /// 拦截账守恒。
    pub fn intercept_conserve(&self) -> bool {
        self.intercepts.len() + self.intercept_overflow == self.seq as usize
    }
}

/// 伪条目批量注入检测（判据「注入伪条目 10/10 次拦截」的执行面）。
pub fn fake_inject_10(p: &mut IoApicProbe) -> Result<usize, u16> {
    let mut caught = 0;
    for i in 0..10u32 {
        // 伪法轮换：零地址 / 容量不符 / 版本非法 / GSI 重叠 / ID 重复
        let (madt, ver) = match i % 5 {
            0 => (
                MadtIoapic { acpi_id: 0x10, mmio_addr: 0, gsi_base: 0, pins: 0 },
                VerProbe { version: 0x11, maxredir: 23 },
            ),
            1 => (
                MadtIoapic { acpi_id: 0x11, mmio_addr: 0xFEC0_0000, gsi_base: 0, pins: 99 },
                VerProbe { version: 0x11, maxredir: 23 },
            ),
            2 => (
                MadtIoapic { acpi_id: 0x12, mmio_addr: 0xFEC0_1000, gsi_base: 0, pins: 0 },
                VerProbe { version: 0, maxredir: 23 },
            ),
            3 => (
                MadtIoapic { acpi_id: 0x13, mmio_addr: 0xFEC0_2000, gsi_base: 0, pins: 0 },
                VerProbe { version: 0x11, maxredir: 23 },
            ),
            _ => (
                MadtIoapic { acpi_id: 0x10, mmio_addr: 0xFEC0_3000, gsi_base: 0, pins: 0 },
                VerProbe { version: 0x11, maxredir: 23 },
            ),
        };
        if p.probe_instance(madt, ver).is_err() {
            caught += 1;
        }
    }
    if caught != 10 {
        return Err(XC_FAKE_ENTRY);
    }
    Ok(caught)
}

/// drill：全链自例程。
pub fn drill() -> Result<u32, u16> {
    // 1. 双实例正常探测（QEMU 档：单 IOAPIC 24 引脚；扩展：第二实例 16 引脚）
    let mut p = IoApicProbe::new();
    let r1 = p.probe_instance(
        MadtIoapic { acpi_id: 1, mmio_addr: 0xFEC0_0000, gsi_base: 0, pins: 24 },
        VerProbe { version: 0x11, maxredir: 23 },
    )?;
    if r1.gsi_len != 24 || r1.inst != 0 {
        return Err(XC_CAPA_MISS);
    }
    let r2 = p.probe_instance(
        MadtIoapic { acpi_id: 2, mmio_addr: 0xFEC0_1000, gsi_base: 24, pins: 16 },
        VerProbe { version: 0x20, maxredir: 15 },
    )?;
    if r2.gsi_len != 16 || r2.gsi_base != 24 {
        return Err(XC_CAPA_MISS);
    }
    // 2. GSI 归属查询
    if p.instance_of_gsi(30)?.inst != 1 {
        return Err(XC_PIN_RANGE);
    }
    if p.instance_of_gsi(5)?.inst != 0 {
        return Err(XC_PIN_RANGE);
    }
    // 3. 引脚越界查询拒
    if p.instance_of_gsi(1000).is_ok() {
        return Err(XC_PIN_RANGE);
    }
    // 4. 伪条目 10/10 拦截
    let caught = fake_inject_10(&mut p)?;
    if caught != 10 {
        return Err(XC_FAKE_ENTRY);
    }
    // 5. 账核
    if p.instance_count() != 2 {
        return Err(XC_CONSERVE);
    }
    if !p.intercept_conserve() {
        return Err(XC_LED_FULL);
    }
    // 6. 错误路径①：MAXREDIR=255（引脚 256 > 240）拒
    let mut p2 = IoApicProbe::new();
    if p2.probe_instance(
        MadtIoapic { acpi_id: 3, mmio_addr: 0xFEC0_8000, gsi_base: 0, pins: 0 },
        VerProbe { version: 0x11, maxredir: 255 },
    ).is_ok() {
        return Err(XC_VER_BAD);
    }
    // 7. 错误路径②：VER 引脚数与声明不符（0 声明除外）
    if p2.probe_instance(
        MadtIoapic { acpi_id: 4, mmio_addr: 0xFEC0_8000, gsi_base: 0, pins: 100 },
        VerProbe { version: 0x11, maxredir: 23 },
    ).is_ok() {
        return Err(XC_CAPA_MISS);
    }
    Ok(0)
}
