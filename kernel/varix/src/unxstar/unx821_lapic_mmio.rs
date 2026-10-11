//! unx821_lapic_mmio ↔ UNX-F0821 LAPIC 基址检测与 MMIO 映射管理
//!
//! 锚点原文（CoRun Unxreal 主册 UNX-F0821，UNX-A1-B42）：
//! 「LAPIC 基址检测与 MMIO 映射管理 | 260 | 已深化 |
//!  UNX-F0821-J1 基址三源（MSR/MADT/ACPI 默认）交叉一致，
//!  映射只读校验位生效（写入拦截）」
//!
//! 设计要点：
//! 1. **三源交叉一致**：LAPIC 基址三源——①IA32_APIC_BASE MSR
//!     （权威）②MADT Local APIC 地址字段 ③x2APIC 不可用时的
//!     默认 0xFEE00000。`cross_check()` 三源必须一致，
//!     不一致即 `SourceDivergence`（不一致说明表解析错或
//!     MSR 读错——带病映射会全核 IPI 打飞）。
//! 2. **x2APIC 模式识别**：`ApicMode` 三态（XApic/X2Apic/
//!     Disabled），MSR bit 10/11 判 x2APIC；模式决定访问
//!     方式（MMIO vs MSR 窗口），错配即 `ModeMismatch`。
//! 3. **页对齐与保留区**：基址必须 4KB 对齐；映射尺寸 4KB
//!     （LAPIC 寄存器页）；低 12 位非零即 `BaseUnaligned`。
//! 4. **映射只读校验位**：映射PTE 带只读位（写拦截）；
//!     `write_guard()` 检查「目标页是否在映射内 + 是否只读」，
//!     越界写或可写页写即 `WriteViolation`——
//!     LAPIC 页被误写 = 中断控制器配置错乱，必须硬件级拦截。
//! 5. **总线地址边界**：MMIO 访问必须走 HHDM/映射窗口，
//!     `translate()` 校验虚拟窗口内；窗口外访问显性
//!     `UnmappedWindow`（禁直接解引用物理地址）。
//!
//! 诊断码：`LapicCode` 0x92 段（0x9200..=0x920F）。

use alloc::format;
use alloc::string::String;

use crate::checks::CheckSet;

/// 模块版本。
pub const UNX821_LAPIC_VERSION: &str = "unx821-lapic-mmio-v1";

/// LAPIC 默认基址（x2APIC 不可用时的传统地址）。
pub const LAPIC_DEFAULT_BASE: u64 = 0xFEE0_0000;

/// IA32_APIC_BASE MSR 号。
pub const MSR_APIC_BASE: u32 = 0x1B;

/// x2APIC 使能位（bit 10）。
pub const X2APIC_ENABLE_BIT: u64 = 1 << 10;

/// x2APIC 全局使能位（bit 11）。
pub const X2APIC_GLOBAL_BIT: u64 = 1 << 11;

/// 基址有效位（bit 8 之前的地址字段掩码）。
pub const APIC_BASE_ADDR_MASK: u64 = 0xFFFF_F000;

/// 映射页大小（4KB）。
pub const LAPIC_PAGE_SIZE: u64 = 4096;

/// APIC 模式。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ApicMode {
    /// xAPIC（MMIO 访问）
    XApic,
    /// x2APIC（MSR 窗口访问）
    X2Apic,
    /// 禁用
    Disabled,
}

impl ApicMode {
    /// 是否需要 MMIO 映射。
    pub const fn needs_mmio(self) -> bool {
        matches!(self, ApicMode::XApic)
    }

    pub const fn wire(self) -> &'static str {
        match self {
            ApicMode::XApic => "XAPIC",
            ApicMode::X2Apic => "X2APIC",
            ApicMode::Disabled => "DISABLED",
        }
    }
}

/// 基址三源。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ApicBaseSources {
    /// MSR 源（None = MSR 不可用）
    pub msr: Option<u64>,
    /// MADT 源（None = 无 MADT）
    pub madt: Option<u64>,
    /// 是否用默认值兜底
    pub used_default: bool,
}

/// MMIO 映射。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MmioMap {
    /// 物理基址
    pub phys: u64,
    /// 虚拟窗口基址
    pub virt: u64,
    /// 尺寸（4KB）
    pub len: u64,
    /// 只读校验位（true = 只读，写拦截）
    pub read_only: bool,
}

/// LAPIC 映射管理。
#[derive(Clone, Copy, Debug)]
pub struct LapicMmio {
    base: u64,
    mode: ApicMode,
    map: Option<MmioMap>,
}

/// LAPIC 错误。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LapicError {
    /// 源分歧
    SourceDivergence { msr: u64, madt: u64 },
    /// 基址未对齐
    BaseUnaligned { base: u64 },
    /// 模式与访问方式错配
    ModeMismatch { mode: ApicMode },
    /// 未映射
    UnmappedWindow { addr: u64 },
    /// 写侵犯（只读页或越界）
    WriteViolation { addr: u64, reason: &'static str },
    /// 默认兜底与 MSR 矛盾
    DefaultConflicts { msr: u64, default: u64 },
}

/// 诊断码（0x92 段）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LapicCode {
    /// 0x9200 源分歧
    Divergence,
    /// 0x9201 基址未对齐
    Unaligned,
    /// 0x9202 模式错配
    ModeBad,
    /// 0x9203 未映射
    Unmapped,
    /// 0x9204 写侵犯
    WriteBad,
    /// 0x9205 默认矛盾
    DefaultBad,
    /// 0x9206 交叉一致通过
    Consistent,
    /// 0x9207 只读校验生效
    GuardOk,
}

impl LapicCode {
    pub const fn code(self) -> u16 {
        match self {
            LapicCode::Divergence => 0x9200,
            LapicCode::Unaligned => 0x9201,
            LapicCode::ModeBad => 0x9202,
            LapicCode::Unmapped => 0x9203,
            LapicCode::WriteBad => 0x9204,
            LapicCode::DefaultBad => 0x9205,
            LapicCode::Consistent => 0x9206,
            LapicCode::GuardOk => 0x9207,
        }
    }

    pub const fn wire(self) -> &'static str {
        match self {
            LapicCode::Divergence => "LAPIC-DIVERGENCE",
            LapicCode::Unaligned => "LAPIC-UNALIGNED",
            LapicCode::ModeBad => "LAPIC-MODE-BAD",
            LapicCode::Unmapped => "LAPIC-UNMAPPED",
            LapicCode::WriteBad => "LAPIC-WRITE-BAD",
            LapicCode::DefaultBad => "LAPIC-DEFAULT-BAD",
            LapicCode::Consistent => "LAPIC-CONSISTENT",
            LapicCode::GuardOk => "LAPIC-GUARD-OK",
        }
    }

    pub fn detail(self) -> String {
        let base = match self {
            LapicCode::Divergence => "LAPIC 基址三源分歧：MSR 与 MADT 不一致，禁带病映射",
            LapicCode::Unaligned => "LAPIC 基址未页对齐：必须 4KB 对齐",
            LapicCode::ModeBad => "APIC 模式与访问方式错配：xAPIC 才能 MMIO，x2APIC 走 MSR 窗口",
            LapicCode::Unmapped => "访问未映射窗口：LAPIC 访问必须走映射窗口，禁裸物理地址",
            LapicCode::WriteBad => "写侵犯：只读页或越界写被拦截",
            LapicCode::DefaultBad => "默认兜底与 MSR 矛盾：有 MSR 值时禁回落默认",
            LapicCode::Consistent => "三源交叉一致：MSR/MADT/默认三源合一",
            LapicCode::GuardOk => "只读校验位生效：映射页写拦截",
        };
        let mut s = String::from(base);
        s.push_str("（源：unxstar/unx821_lapic_mmio.rs，码 0x");
        s.push_str(&format!("{:04X}", self.code()));
        s.push_str("）");
        s
    }

    pub const fn segment_guard(self) -> bool {
        let c = self.code();
        (c & 0xFF00) == 0x9200 && (c & 0x00FF) < 0x10
    }
}

impl LapicError {
    pub const fn code(self) -> LapicCode {
        match self {
            LapicError::SourceDivergence { .. } => LapicCode::Divergence,
            LapicError::BaseUnaligned { .. } => LapicCode::Unaligned,
            LapicError::ModeMismatch { .. } => LapicCode::ModeBad,
            LapicError::UnmappedWindow { .. } => LapicCode::Unmapped,
            LapicError::WriteViolation { .. } => LapicCode::WriteBad,
            LapicError::DefaultConflicts { .. } => LapicCode::DefaultBad,
        }
    }

    pub fn detail(self) -> String {
        self.code().detail()
    }
}

impl ApicBaseSources {
    /// 三源交叉一致核（返回裁定基址）。
    pub fn cross_check(&self) -> Result<u64, LapicError> {
        match (self.msr, self.madt) {
            (Some(m), Some(a)) => {
                let m_base = m & APIC_BASE_ADDR_MASK;
                let a_base = a & APIC_BASE_ADDR_MASK;
                if m_base != a_base {
                    return Err(LapicError::SourceDivergence { msr: m_base, madt: a_base });
                }
                Ok(m_base)
            }
            (Some(m), None) => {
                if self.used_default {
                    let m_base = m & APIC_BASE_ADDR_MASK;
                    if m_base != LAPIC_DEFAULT_BASE {
                        return Err(LapicError::DefaultConflicts {
                            msr: m_base,
                            default: LAPIC_DEFAULT_BASE,
                        });
                    }
                }
                Ok(m & APIC_BASE_ADDR_MASK)
            }
            (None, Some(a)) => {
                if self.used_default {
                    let a_base = a & APIC_BASE_ADDR_MASK;
                    if a_base != LAPIC_DEFAULT_BASE {
                        return Err(LapicError::DefaultConflicts {
                            msr: a_base,
                            default: LAPIC_DEFAULT_BASE,
                        });
                    }
                }
                Ok(a & APIC_BASE_ADDR_MASK)
            }
            (None, None) => {
                if !self.used_default {
                    return Err(LapicError::SourceDivergence {
                        msr: 0,
                        madt: 0,
                    });
                }
                Ok(LAPIC_DEFAULT_BASE)
            }
        }
    }

    /// 模式判定（MSR bit 10/11）。
    pub fn mode_of(msr: Option<u64>) -> ApicMode {
        match msr {
            None => ApicMode::XApic,
            Some(v) => {
                if v & X2APIC_ENABLE_BIT != 0 && v & X2APIC_GLOBAL_BIT != 0 {
                    ApicMode::X2Apic
                } else if v & 1 << 8 == 0 {
                    ApicMode::Disabled
                } else {
                    ApicMode::XApic
                }
            }
        }
    }
}

impl MmioMap {
    /// 构造映射（基址对齐 + 尺寸 4KB）。
    pub fn new(phys: u64, virt: u64, read_only: bool) -> Result<Self, LapicError> {
        if phys & (LAPIC_PAGE_SIZE - 1) != 0 {
            return Err(LapicError::BaseUnaligned { base: phys });
        }
        if virt & (LAPIC_PAGE_SIZE - 1) != 0 {
            return Err(LapicError::BaseUnaligned { base: virt });
        }
        Ok(MmioMap {
            phys,
            virt,
            len: LAPIC_PAGE_SIZE,
            read_only,
        })
    }

    /// 地址是否在窗口内。
    pub const fn contains(&self, virt_addr: u64) -> bool {
        virt_addr >= self.virt && virt_addr < self.virt + self.len
    }

    /// 读守卫（窗口内才放行）。
    pub const fn read_guard(&self, virt_addr: u64) -> Result<(), LapicError> {
        if !self.contains(virt_addr) {
            return Err(LapicError::UnmappedWindow { addr: virt_addr });
        }
        Ok(())
    }

    /// 写守卫（窗口内 + 只读位生效才拦截）。
    pub const fn write_guard(&self, virt_addr: u64) -> Result<(), LapicError> {
        if !self.contains(virt_addr) {
            return Err(LapicError::UnmappedWindow { addr: virt_addr });
        }
        if self.read_only {
            return Err(LapicError::WriteViolation {
                addr: virt_addr,
                reason: "read-only-page",
            });
        }
        Ok(())
    }

    /// 虚拟 → 物理投影（窗口内）。
    pub const fn translate(&self, virt_addr: u64) -> Result<u64, LapicError> {
        if !self.contains(virt_addr) {
            return Err(LapicError::UnmappedWindow { addr: virt_addr });
        }
        Ok(self.phys + (virt_addr - self.virt))
    }
}

impl LapicMmio {
    /// 从三源 + 模式构造（交叉一致 + 模式自洽）。
    pub fn new(sources: &ApicBaseSources) -> Result<Self, LapicError> {
        let base = sources.cross_check()?;
        let mode = ApicBaseSources::mode_of(sources.msr);
        if mode == ApicMode::X2Apic {
            // x2APIC 走 MSR 窗口，不需要 MMIO；禁建 MMIO 映射
            return Ok(LapicMmio {
                base,
                mode,
                map: None,
            });
        }
        if mode == ApicMode::Disabled {
            return Err(LapicError::ModeMismatch { mode });
        }
        Ok(LapicMmio {
            base,
            mode,
            map: None,
        })
    }

    /// 建映射（仅 xAPIC；只读校验位置位）。
    pub fn map(&mut self, virt: u64, read_only: bool) -> Result<MmioMap, LapicError> {
        if self.mode != ApicMode::XApic {
            return Err(LapicError::ModeMismatch { mode: self.mode });
        }
        let m = MmioMap::new(self.base, virt, read_only)?;
        self.map = Some(m);
        Ok(m)
    }

    /// 当前模式。
    pub const fn mode(&self) -> ApicMode {
        self.mode
    }

    /// 基址。
    pub const fn base(&self) -> u64 {
        self.base
    }

    /// 映射（None = 未映射或 x2APIC）。
    pub const fn mapping(&self) -> Option<MmioMap> {
        self.map
    }
}

/// 域自检。
pub fn run_unx821_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx821");

    // C821-01 三源一致（MSR + MADT 同基址）
    {
        let src = ApicBaseSources {
            msr: Some(0xFEE0_0B00 | LAPIC_DEFAULT_BASE),
            madt: Some(LAPIC_DEFAULT_BASE),
            used_default: false,
        };
        let r = src.cross_check();
        let good = r == Ok(LAPIC_DEFAULT_BASE);
        s.add("C821-01-三源交叉一致", good, if good { "" } else { "三源分歧被放行" });
    }

    // C821-02 源分歧拦截（MSR ≠ MADT）
    {
        let src = ApicBaseSources {
            msr: Some(0xFEE0_0000),
            madt: Some(0xFEE1_0000),
            used_default: false,
        };
        let r = src.cross_check();
        let good = matches!(r, Err(LapicError::SourceDivergence { .. }));
        s.add("C821-02-源分歧拦截", good, if good { "" } else { "分歧被放行" });
    }

    // C821-03 模式判定（x2APIC）
    {
        let msr = LAPIC_DEFAULT_BASE | X2APIC_ENABLE_BIT | X2APIC_GLOBAL_BIT | (1 << 8);
        let mode = ApicBaseSources::mode_of(Some(msr));
        let good = mode == ApicMode::X2Apic && !mode.needs_mmio();
        s.add("C821-03-x2APIC 模式", good, if good { "" } else { "模式判定错" });
    }

    // C821-04 xAPIC 模式 MMIO 映射构造
    {
        let src = ApicBaseSources {
            msr: Some(LAPIC_DEFAULT_BASE | (1 << 8)),
            madt: Some(LAPIC_DEFAULT_BASE),
            used_default: false,
        };
        let mut l = match LapicMmio::new(&src) {
            Ok(l) => l,
            Err(_) => {
                s.add("C821-04-构造失败", false, "lapic new failed");
                return s;
            }
        };
        let r = l.map(0xFFFF_8000_0000_0000, true);
        let good = r.is_ok() && l.mode() == ApicMode::XApic;
        s.add("C821-04-xAPIC 建映射", good, if good { "" } else { "建映射失败" });
    }

    // C821-05 x2APIC 禁建 MMIO（模式错配）
    {
        let msr = LAPIC_DEFAULT_BASE | X2APIC_ENABLE_BIT | X2APIC_GLOBAL_BIT | (1 << 8);
        let src = ApicBaseSources {
            msr: Some(msr),
            madt: Some(LAPIC_DEFAULT_BASE),
            used_default: false,
        };
        let mut l = match LapicMmio::new(&src) {
            Ok(l) => l,
            Err(_) => {
                s.add("C821-05-构造失败", false, "lapic new failed");
                return s;
            }
        };
        let r = l.map(0xFFFF_8000_0000_0000, true);
        let good = matches!(r, Err(LapicError::ModeMismatch { mode: ApicMode::X2Apic }));
        s.add("C821-05-x2APIC 禁 MMIO", good, if good { "" } else { "错配建映射被放行" });
    }

    // C821-06 只读页写拦截
    {
        let m = MmioMap::new(LAPIC_DEFAULT_BASE, 0xFFFF_8000_0000_0000, true);
        let good = match m {
            Ok(m) => {
                let w = m.write_guard(0xFFFF_8000_0000_0000 + 0x30);
                let rd = m.read_guard(0xFFFF_8000_0000_0000 + 0x30);
                matches!(w, Err(LapicError::WriteViolation { .. })) && rd.is_ok()
            }
            Err(_) => false,
        };
        s.add("C821-06-只读写拦截", good, if good { "" } else { "只读失效" });
    }

    // C821-07 窗口外访问拦截
    {
        let m = MmioMap::new(LAPIC_DEFAULT_BASE, 0xFFFF_8000_0000_0000, false);
        let good = match m {
            Ok(m) => {
                let w = m.write_guard(0xFFFF_8000_0000_0000 + 0x5000);
                matches!(w, Err(LapicError::UnmappedWindow { .. }))
            }
            Err(_) => false,
        };
        s.add("C821-07-窗口外拦截", good, if good { "" } else { "越界被放行" });
    }

    // C821-08 虚拟 → 物理投影
    {
        let m = MmioMap::new(LAPIC_DEFAULT_BASE, 0xFFFF_8000_0000_0000, false);
        let good = match m {
            Ok(m) => {
                let p = m.translate(0xFFFF_8000_0000_0000 + 0x100);
                p == Ok(LAPIC_DEFAULT_BASE + 0x100)
            }
            Err(_) => false,
        };
        s.add("C821-08-虚拟物理投影", good, if good { "" } else { "投影错" });
    }

    // C821-09 基址未对齐拒收
    {
        let m = MmioMap::new(0xFEE0_0001, 0xFFFF_8000_0000_0000, false);
        let good = matches!(m, Err(LapicError::BaseUnaligned { .. }));
        s.add("C821-09-基址对齐硬线", good, if good { "" } else { "未对齐被放行" });
    }

    // C821-10 默认兜底矛盾拦截
    {
        let src = ApicBaseSources {
            msr: Some(0xFEE1_0000),
            madt: None,
            used_default: true,
        };
        let r = src.cross_check();
        let good = matches!(r, Err(LapicError::DefaultConflicts { .. }));
        s.add("C821-10-默认矛盾拦截", good, if good { "" } else { "矛盾被放行" });
    }

    // C821-11 无源无默认拒收（禁凭空建基址）
    {
        let src = ApicBaseSources {
            msr: None,
            madt: None,
            used_default: false,
        };
        let r = src.cross_check();
        let good = matches!(r, Err(LapicError::SourceDivergence { .. }));
        s.add("C821-11-无源拒收", good, if good { "" } else { "凭空基址被放行" });
    }

    // C821-12 诊断码段独占
    {
        let codes = [
            LapicCode::Divergence,
            LapicCode::Unaligned,
            LapicCode::ModeBad,
            LapicCode::Unmapped,
            LapicCode::WriteBad,
            LapicCode::DefaultBad,
            LapicCode::Consistent,
            LapicCode::GuardOk,
        ];
        let mut all = true;
        for c in codes.iter() {
            all = all && c.segment_guard();
        }
        s.add("C821-12-诊断码 0x92 段独占", all, if all { "" } else { "码值越出 0x92 段" });
    }

    s
}
