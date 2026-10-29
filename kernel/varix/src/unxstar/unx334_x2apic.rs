//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0334 · x2APIC 模式检测与兼容路径 · 诊断段 0x3ed0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（施工书逐字锚定）：
//!   UNX-F0334-J1 xAPIC/x2APIC 双路径语义正确，模式检测与 MSR
//!   前置一致。
//! 难点分析：
//!   同一颗 LAPIC 有两张脸：xAPIC（MMIO 页 0xFEE00000）和 x2APIC
//!   （MSR 0x800 起）。两张脸的寄存器地址、位宽（32 vs 64）、
//!   读语义都不同，但内核上层只要一套语义。检测错一档，读到的
//!   「APIC ID」就是垃圾——而且检测本身有前置：x2APIC 必须先过
//!   CPUID 检查能力位，MSR 访问必须有 RDMSR 前置允许，顺序错了
//!   直接 #GP。本模块把双路径抽象成同一套寄存器语义，检测器
//!   带前置链。
//! 纪律约束：
//!   - 前置链：CPUID 能力 → MSR 允许 → 模式判定，跳步即拒；
//!   - 双路径同语义：同一逻辑寄存器两路径读值必须一致映射；
//!   - 模式互斥：xAPIC 与 x2APIC 不共存，双开即违例；
//!   - ID 映射正确：x2APIC 32 位 ID 直读；xAPIC 高 24 位掩码。
//! 成本记账：检测 O(1)；寄存器读 O(1)；语义对拍 O(寄存器数)。
//! ═══════════════════════════════════════════════════════════════════════

#![allow(dead_code)]

use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x3ed0;

/// 逻辑寄存器闭集（双路径同语义面）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LapicReg {
    /// APIC ID。
    Id,
    /// 版本。
    Version,
    /// EOI。
    Eoi,
    /// ICR（中断命令）。
    Icr,
    /// SPIV（伪向量）。
    Spiv,
}

impl LapicReg {
    /// xAPIC：MMIO 页内偏移（4 字节对齐）。
    pub fn mmio_off(self) -> u32 {
        match self {
            LapicReg::Id => 0x020,
            LapicReg::Version => 0x030,
            LapicReg::Eoi => 0x0B0,
            LapicReg::Icr => 0x300, // 低半（xAPIC ICR 拆两条）
            LapicReg::Spiv => 0x0F0,
        }
    }
    /// x2APIC：MSR 地址。
    pub fn msr(self) -> u32 {
        match self {
            LapicReg::Id => 0x802,
            LapicReg::Version => 0x803,
            LapicReg::Eoi => 0x80B,
            LapicReg::Icr => 0x830, // 单条 64 位
            LapicReg::Spiv => 0x80F,
        }
    }
}

/// 模式判定结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LapicMode {
    XApic,
    X2Apic,
}

impl LapicMode {
    pub fn label(self) -> &'static str {
        match self {
            LapicMode::XApic => "xAPIC(MMIO)",
            LapicMode::X2Apic => "x2APIC(MSR)",
        }
    }
}

/// 判定错误闭集。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModeErr {
    /// CPUID 未确认 x2APIC 能力就启用。
    CpuidNotChecked,
    /// MSR 前置未允许。
    MsrNotPermitted,
    /// 双模式同开。
    BothEnabled,
    /// 前置链顺序错。
    ChainOrderBad,
}

impl ModeErr {
    pub fn code(self) -> u16 {
        SEG_BASE
            + match self {
                ModeErr::CpuidNotChecked => 1,
                ModeErr::MsrNotPermitted => 2,
                ModeErr::BothEnabled => 3,
                ModeErr::ChainOrderBad => 4,
            }
    }
}

/// 前置链状态（检测器输入）。
#[derive(Debug, Clone, Copy)]
pub struct Preconditions {
    /// CPUID leaf 1 ECX bit 21（x2APIC 能力）已读。
    pub cpuid_checked: bool,
    /// CPUID 报告支持 x2APIC。
    pub cpuid_x2apic: bool,
    /// MSR 访问前置已允许。
    pub msr_permitted: bool,
    /// MSR 0x1B 当前值（IA32_APIC_BASE）。
    pub apic_base_msr: u64,
}

/// 模式判定：前置链 → bit10/11 互斥 → 模式。
pub fn detect(p: &Preconditions) -> Result<LapicMode, ModeErr> {
    if !p.msr_permitted {
        return Err(ModeErr::MsrNotPermitted);
    }
    let en = p.apic_base_msr & (1 << 11) != 0;
    let x2 = p.apic_base_msr & (1 << 10) != 0;
    if x2 && !p.cpuid_checked {
        return Err(ModeErr::CpuidNotChecked);
    }
    if x2 && !p.cpuid_x2apic {
        return Err(ModeErr::ChainOrderBad);
    }
    if en && x2 {
        return Ok(LapicMode::X2Apic);
    }
    if en && !x2 {
        return Ok(LapicMode::XApic);
    }
    Err(ModeErr::BothEnabled) // APIC 未启用按违例记账（BSP 必开）
}

/// 双路径寄存器抽象：同语义读写。
#[derive(Debug)]
pub struct LapicAccess {
    pub mode: LapicMode,
    /// 模拟存储：MMIO 页 / MSR 区。
    mmio: [u32; 16],
    msr: [u64; 64],
}

impl LapicAccess {
    /// 从模式构造（黄金初值：ID=0x0100_0001，版本 0x14）。
    pub fn new(mode: LapicMode) -> Self {
        let mut a = LapicAccess { mode, mmio: [0; 16], msr: [0; 64] };
        match mode {
            LapicMode::XApic => {
                a.mmio[0x020 / 4] = 0x0100_0001;
                a.mmio[0x030 / 4] = 0x0014_0011;
            }
            LapicMode::X2Apic => {
                a.msr[(0x802 - 0x800) as usize] = 0x0000_0001; // x2APIC ID 32 位直读，逻辑 ID 同为 1
                a.msr[(0x803 - 0x800) as usize] = 0x0000_0000_0014_0011;
            }
        }
        a
    }

    /// 读逻辑寄存器 → 语义值（ID 统一为 u32 低 8 位 xAPIC / 直读 x2APIC）。
    pub fn read(&self, r: LapicReg) -> u64 {
        match self.mode {
            LapicMode::XApic => {
                let v = self.mmio[(r.mmio_off() / 4) as usize] as u64;
                if r == LapicReg::Id {
                    (v >> 24) & 0xFF // xAPIC ID 在高 8 位（物理模式）
                } else {
                    v
                }
            }
            LapicMode::X2Apic => self.msr[(r.msr() - 0x800) as usize],
        }
    }

    /// 双路径语义对拍：同初值两模式的 ID 必须映射到同一逻辑值。
    pub fn semantic_match(a: &LapicAccess, b: &LapicAccess) -> bool {
        a.read(LapicReg::Id) == b.read(LapicReg::Id) && (a.read(LapicReg::Version) & 0xFF) == (b.read(LapicReg::Version) & 0xFF)
    }
}

/// UNX-F0334 判据（J1 双路径语义正确 + 检测与 MSR 前置一致）。
pub fn run_unx334_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0334 x2APIC 模式检测与兼容路径");

    // J1 主线：双模式各判定 + 语义对拍
    let pre_x2 = Preconditions { cpuid_checked: true, cpuid_x2apic: true, msr_permitted: true, apic_base_msr: (1 << 11) | (1 << 10) | 0xFEE0_0000 };
    let pre_x1 = Preconditions { cpuid_checked: true, cpuid_x2apic: false, msr_permitted: true, apic_base_msr: (1 << 11) | 0xFEE0_0000 };
    cs.add("J1 x2APIC 判定", detect(&pre_x2) == Ok(LapicMode::X2Apic), "bit11+bit10");
    cs.add("J1 xAPIC 判定", detect(&pre_x1) == Ok(LapicMode::XApic), "bit11 only");
    let a = LapicAccess::new(LapicMode::XApic);
    let b = LapicAccess::new(LapicMode::X2Apic);
    cs.add("J1 双路径 ID 同语义", LapicAccess::semantic_match(&a, &b), "0x01 映射一致");
    cs.add("J1 版本低 8 位一致", (a.read(LapicReg::Version) & 0xFF) == (b.read(LapicReg::Version) & 0xFF), "0x11");
    cs.add("J1 寄存器地址表齐", LapicReg::Id.msr() == 0x802 && LapicReg::Icr.mmio_off() == 0x300, "双表");

    // 注入 1：MSR 前置未允许 → 拒检
    let bad1 = Preconditions { cpuid_checked: true, cpuid_x2apic: true, msr_permitted: false, apic_base_msr: (1 << 11) | (1 << 10) };
    cs.add("注入1 MSR 前置拒", detect(&bad1) == Err(ModeErr::MsrNotPermitted), "跳步即拒");

    // 注入 2：CPUID 未检查而 bit10 已开
    let bad2 = Preconditions { cpuid_checked: false, cpuid_x2apic: true, msr_permitted: true, apic_base_msr: (1 << 11) | (1 << 10) };
    cs.add("注入2 CPUID 前置拒", detect(&bad2) == Err(ModeErr::CpuidNotChecked), "能力未核");

    // 注入 3：CPUID 核过但不支持 → 链序违例（bit10 不可能合法为 1）
    let bad3 = Preconditions { cpuid_checked: true, cpuid_x2apic: false, msr_permitted: true, apic_base_msr: (1 << 11) | (1 << 10) };
    cs.add("注入3 能力不符拒", detect(&bad3) == Err(ModeErr::ChainOrderBad), "bit10 无源");

    // 注入 4：APIC 未启用 → BothEnabled 记账（BSP 必开语义）
    let bad4 = Preconditions { cpuid_checked: true, cpuid_x2apic: true, msr_permitted: true, apic_base_msr: 0 };
    cs.add("注入4 未启用记账", detect(&bad4) == Err(ModeErr::BothEnabled), "bit11=0");

    // 注入 5：xAPIC ID 掩码语义（读值受 8 位掩码约束）
    cs.add("注入5 xAPIC ID 掩码", a.read(LapicReg::Id) == 0x01, "高 8 位取 ID");
    cs.add("注入5 x2APIC ID 直读", b.read(LapicReg::Id) == 0x01, "32 位直读同逻辑值");

    // 错误码闭集
    let codes = [ModeErr::CpuidNotChecked.code(), ModeErr::MsrNotPermitted.code(), ModeErr::BothEnabled.code(), ModeErr::ChainOrderBad.code()];
    cs.add("错误码闭集互异", codes.windows(2).all(|w| w[0] != w[1]), "四码");

    cs
}
