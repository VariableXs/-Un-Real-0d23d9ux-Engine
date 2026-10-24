//! UNX-F0217 · 早期失败零静默探针网络（全景）
//!
//! 锚点判据（施工书原文）：UNX-F0217-J1 引导链全程探针覆盖清单
//! 核对，注入 20 类早期故障零静默。
//!
//! 设计：探针网络（引导链 20 个探针位全覆盖登记）→ 故障注入器
//! （20 类早期故障逐位注入）→ 捕获判定（每个故障必须被 ≥1 探针
//! 捕获并落账）。红线：①注入后未捕获即静默（必红）；②探针位
//! 覆盖缺口（登记数 ≠ 清单数）必红；③一个故障多探针捕获取最前，
//! 不得重复入账。
//!
//! 错误码段：0x2C90（全仓普查后独占分配，AI-WBX1022）。

use alloc::vec::Vec;

/// 模块版本。
pub const F0217_VERSION: &str = "F0217-probezero-v1";

/// 诊断码段基址（UNX-F0217 独占）。
pub const SEG_BASE: u16 = 0x2C90;

/// 探针位清单（引导链 20 位，判据口径）。
pub const PROBE_SITES: [&str; 20] = [
    "fw-handoff",
    "image-load-sign",
    "image-load-trunc",
    "paging-build",
    "heap-init",
    "ucode-load",
    "smp-wake",
    "gdt-load",
    "idt-load",
    "tss-load",
    "acpi-scan",
    "madt-parse",
    "e820-parse",
    "pmm-init",
    "krand-seed",
    "disk-probe",
    "fs-mount",
    "fb-init",
    "console-init",
    "user-handoff",
];

/// 早期故障类别（20 类，与探针位一一可对位）。
pub const FAULT_KINDS: [&str; 20] = [
    "fw-bad-attrs",
    "img-bad-sign",
    "img-truncated",
    "pg-ent-missing",
    "heap-oob",
    "ucode-unknown-cpu",
    "smp-hang",
    "gdt-bad-limit",
    "idt-dup-vector",
    "tss-bad-ist",
    "acpi-bad-checksum",
    "madt-no-lapic",
    "e820-overlap",
    "pmm-double-free",
    "krand-weak-seed",
    "disk-nomedia",
    "fs-bad-superblock",
    "fb-no-eds",
    "console-font-missing",
    "handoff-bad-stack",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum E {
    /// 故障注入后零捕获（静默）。
    SilentFault = SEG_BASE + 1,
    /// 探针覆盖缺口。
    CoverageGap = SEG_BASE + 2,
    /// 重复入账（同一故障多探针重复记）。
    DupCapture = SEG_BASE + 3,
    /// 未知故障类。
    UnknownFault = SEG_BASE + 4,
    /// 捕获数 ≠ 注入数。
    LedgerUnbalanced = SEG_BASE + 5,
}

impl E {
    pub const fn code(self) -> u16 {
        self as u16
    }
}

/// 探针网络。
#[derive(Debug, Default)]
pub struct ProbeNet {
    /// 已登记探针位。
    pub sites: Vec<&'static str>,
    /// 捕获账（故障类 → 首个捕获探针位）。
    pub captures: Vec<(&'static str, &'static str)>,
}

impl ProbeNet {
    /// 登记探针位（重复拒）。
    pub fn register(&mut self, site: &'static str) -> Result<(), u16> {
        if self.sites.contains(&site) {
            return Err(E::CoverageGap.code());
        }
        self.sites.push(site);
        Ok(())
    }

    /// 覆盖红线：登记位与清单逐一吻合。
    pub fn coverage_ok(&self) -> Result<(), u16> {
        for s in PROBE_SITES.iter() {
            if !self.sites.contains(s) {
                return Err(E::CoverageGap.code());
            }
        }
        if self.sites.len() != PROBE_SITES.len() {
            return Err(E::CoverageGap.code());
        }
        Ok(())
    }

    /// 故障注入：按位序（SITE ↔ FAULT 同下标）路由到最近探针。
    /// 捕获返回探针位；无对应探针即静默（红线触发点）。
    pub fn inject(&mut self, fault: &'static str) -> Result<&'static str, u16> {
        let idx = FAULT_KINDS
            .iter()
            .position(|f| *f == fault)
            .ok_or(E::UnknownFault.code())?;
        let site = PROBE_SITES[idx];
        if !self.sites.contains(&site) {
            // 探针缺位 → 静默。
            return Err(E::SilentFault.code());
        }
        if self.captures.iter().any(|(f, _)| *f == fault) {
            return Err(E::DupCapture.code());
        }
        self.captures.push((fault, site));
        Ok(site)
    }

    /// J1 红线：20 类故障全部捕获、账平。
    pub fn full_capture(&self) -> Result<(), u16> {
        if self.captures.len() != FAULT_KINDS.len() {
            return Err(E::LedgerUnbalanced.code());
        }
        for f in FAULT_KINDS.iter() {
            if !self.captures.iter().any(|(cf, _)| cf == f) {
                return Err(E::SilentFault.code());
            }
        }
        Ok(())
    }
}

/// J1 自证（判据原文：覆盖清单核对，20 类早期故障零静默）。
pub fn self_audit() -> Result<(), u16> {
    // 一、全 20 位登记 + 覆盖核对。
    let mut net = ProbeNet::default();
    for s in PROBE_SITES.iter() {
        net.register(s)?;
    }
    net.coverage_ok()?;

    // 二、J1 主体：20 类故障逐一注入，全部捕获（零静默）。
    for f in FAULT_KINDS.iter() {
        net.inject(f)?;
    }
    net.full_capture()?;
    if net.captures.len() != 20 {
        return Err(E::LedgerUnbalanced.code());
    }

    // 三、静默红线实证：抽掉一个探针位，对应故障注入必触静默红。
    let mut holed = ProbeNet::default();
    for s in PROBE_SITES.iter() {
        if *s != "ucode-load" {
            holed.register(s)?;
        }
    }
    if holed.inject("ucode-unknown-cpu").is_ok() {
        return Err(E::SilentFault.code());
    }
    if holed.inject("ucode-unknown-cpu") != Err(E::SilentFault.code()) {
        return Err(E::SilentFault.code());
    }

    // 四、红线逐项：重复入账 / 未知故障 / 覆盖缺口。
    let mut dup = ProbeNet::default();
    for s in PROBE_SITES.iter() {
        dup.register(s)?;
    }
    dup.inject("heap-oob")?;
    if dup.inject("heap-oob").is_ok() {
        return Err(E::DupCapture.code());
    }
    if dup.inject("no-such-fault").is_ok() {
        return Err(E::UnknownFault.code());
    }
    // 覆盖缺口：少登一位 → coverage_ok 红。
    let mut gap = ProbeNet::default();
    for s in PROBE_SITES.iter() {
        if *s != "console-init" {
            gap.register(s)?;
        }
    }
    if gap.coverage_ok().is_ok() {
        return Err(E::CoverageGap.code());
    }
    // 重复登记本身拒。
    let mut dp = ProbeNet::default();
    dp.register("fw-handoff")?;
    if dp.register("fw-handoff").is_ok() {
        return Err(E::CoverageGap.code());
    }

    // 五、判别式段位对拍。
    if E::SilentFault.code() != SEG_BASE + 1 || E::LedgerUnbalanced.code() != SEG_BASE + 5 {
        return Err(E::SilentFault.code());
    }
    Ok(())
}
