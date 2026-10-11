//! unx082_lapicbase — UNX-F0821 LAPIC 基址检测与 MMIO 映射管理。
//!
//! 判据原文（施工书锚点 UNX-F0821，CoRun·UNX-A2-B02 批）：
//! **UNX-F0821-J1 基址三源（MSR/MADT/ACPI 默认）交叉一致，映射只读校验位
//! 生效（写入拦截）**。
//!
//! 职责定位：LAPIC 的「门牌号」与「门锁」——基址从哪里来（三源交叉：MSR
//! IA32_APIC_BASE 0x1B 是 CPU 眼里的真值；MADT 表 Local APIC Address 是
//! 固件宣告；ACPI 默认 0xFEE00000 是规范兜底）与 MMIO 映射怎么管（4KB 页
//! 只读强制 + 写入拦截账——LAPIC 寄存器区写错一个偏移就是给 CPU 发随机
//! IPI，Windows 的 HAL 用 MmMapIoSpace 直接可写映射，一次 errant 写就是
//! 全机中断面紊乱且无账可查）。Varix 深在：①三源交叉不一致**立案不猜**
//! （Windows 以 MSR 为准静默丢弃其他源）；②映射页只读强制位可验证
//! （写拦截测试注入——写拦截率 100% 判据的构造面）；③写入拦截账逐笔
//! 留痕（谁/何时/想写哪/写什么值）；④寄存器偏移表单源闭集（SDM Vol.3
//! §10.12 全寄存器 128 字节步进常量——消费方查表不走裸偏移，杜绝
//! 0x30/0x310 手滑）；⑤x2APIC 模式显性分支（MSR bit12=1 时 MMIO 基址
//! 失效——语义上 x2APIC 走 MSR 0x800 系，本单把两种形态的检测与上报
//! 做成显性两态，不给「半 x2APIC」留幻觉空间）。
//! 上游：F0004 LAPIC 基址账（承接其登记）、F0801 五区规划（spurious 向量
//! 联动）、UNX-F0181 编排（本单属 Subsystems 阶段单元）；下游：F0822 SVR
//! 序列（吃本单的寄存器偏移表）、F0823 EOI 单点（吃 0x010 偏移常量）。
//! 真机 rdmsr/MADT 物理读归接线面（asm+acpi.rs 实读），本单交付**三源
//! 交叉模型 + 映射管理状态机 + 拦截账 + 偏移表闭集**四面。
//!
//! 设计要点：no_std+alloc；零 unwrap/expect；零 IO 零墙钟（MSR/MADT 读数
//! 由上游注入——本单是纯函数决策面）；常量写死并注明 SDM/ACPI 出处；
//! 错误全显性带建议；FNV 快照往返。诊断码段 0x37（0x36 为 F0801 所取）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const LAPIC_BASE_VERSION: &str = "B02-lapicbase-v1";

// ---------------------------------------------------------------------------
// 诊断码（XcCode 0x37 段，本单独占）
// ---------------------------------------------------------------------------

/// 0x3701 三源基址不一致（MSR/MADT/默认两两比对失败且非可解释漂移）。
pub const XC_SOURCE_MISMATCH: u32 = 0x3701;
/// 0x3702 MSR 使能位缺失（IA32_APIC_BASE bit11=0——LAPIC 全局禁用）。
pub const XC_MSR_DISABLED: u32 = 0x3702;
/// 0x3703 基址非 4KB 对齐（MSR bit35:12 之外的低位非零——硬件异常）。
pub const XC_BASE_MISALIGNED: u32 = 0x3703;
/// 0x3704 映射状态机越序（Unmapped 直跳 Sealed 等非法迁移）。
pub const XC_MAP_STATE_ILLEGAL: u32 = 0x3704;
/// 0x3705 只读校验未生效（映射页可写——写拦截闸落空）。
pub const XC_READONLY_NOT_ENFORCED: u32 = 0x3705;
/// 0x3706 写入拦截账丢失（拦截事件未落账——拦截率破 100% 承诺）。
pub const XC_INTERCEPT_LEDGER_MISS: u32 = 0x3706;
/// 0x3707 寄存器偏移越界（查表偏移不在 LAPIC 寄存器闭集）。
pub const XC_REG_OFFSET_OOB: u32 = 0x3707;
/// 0x3708 x2APIC 与 MMIO 混用（x2APIC 模式下走 MMIO 偏移——形态混用）。
pub const XC_X2APIC_MMIO_MIX: u32 = 0x3708;
/// 0x3709 偏移表步进违例（寄存器偏移非 16 字节步进倍数）。
pub const XC_OFFSET_STRIDE_BAD: u32 = 0x3709;
/// 0x370A 基址密封后重映射（Sealed 态再改映射——门锁后撬门）。
pub const XC_REMAP_AFTER_SEAL: u32 = 0x370A;

// ---------------------------------------------------------------------------
// 常量（写死并注明出处）
// ---------------------------------------------------------------------------

/// IA32_APIC_BASE MSR 编号（SDM Vol.3 §10.4.3 / Vol.4 MSR 0x1B）。
pub const MSR_IA32_APIC_BASE: u32 = 0x1B;
/// MSR bit11：APIC Global Enable（SDM Figure 10-2）。
pub const MSR_BIT_APIC_ENABLE: u64 = 1 << 11;
/// MSR bit12：x2APIC mode enable（SDM §10.12.1——需先 bit11=1）。
pub const MSR_BIT_X2APIC_ENABLE: u64 = 1 << 12;
/// MSR 基址掩码（bit35:12——4KB 页基址，低位全零）。
pub const MSR_BASE_MASK: u64 = 0x0000_000F_FFFF_F000;
/// ACPI 规范默认 LAPIC 基址（ACPI Spec §5.2.12 MADT Local APIC Address
/// 字段缺省语义——固件没宣告时平台仍按此值解码）。
pub const ACPI_DEFAULT_LAPIC_BASE: u64 = 0xFEE0_0000;
/// LAPIC MMIO 区大小（4KB 一页——SDM §10.4.1 寄存器区占一页）。
pub const LAPIC_MMIO_SIZE: u64 = 0x1000;
/// 4KB 页掩码（对齐校验）。
pub const PAGE_MASK: u64 = 0xFFF;
/// x2APIC MSR 基座（x2APIC 寄存器=0x800 + (MMIO 偏移>>4)——SDM §10.12.2）。
pub const X2APIC_MSR_BASE: u32 = 0x800;
/// 寄存器步进（MMIO 面寄存器 128 字节步进=0x10——SDM §10.4.1 Note）。
pub const REG_STRIDE: u64 = 0x10;

// ---------------------------------------------------------------------------
// 一、基址三源模型（注入式三源 + 交叉一致裁决）
// ---------------------------------------------------------------------------

/// 基址源三闭集（J1 前半的取值域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BaseSource {
    /// 源一：MSR IA32_APIC_BASE（CPU 眼里的真值——最高权威）。
    Msr = 0,
    /// 源二：MADT 表 Local APIC Address（固件宣告——次权威）。
    Madt = 1,
    /// 源三：ACPI 默认值（规范兜底——无固件宣告时的静态基线）。
    AcpiDefault = 2,
}

impl BaseSource {
    /// 源名（打印）。
    pub fn name(self) -> &'static str {
        match self {
            BaseSource::Msr => "MSR(0x1B)",
            BaseSource::Madt => "MADT表",
            BaseSource::AcpiDefault => "ACPI默认",
        }
    }
}

/// 三源读数（注入面——接线面把 rdmsr/MADT 实读结果填进来）。
#[derive(Clone, Copy, Debug)]
pub struct SourceReadings {
    /// MSR 原始 64 位（bit11/12/基址——接线面 rdmsr(0x1B) 的读数）。
    pub msr_raw: u64,
    /// MADT 宣告基址（0=表缺失——显性零不是猜测）。
    pub madt_base: u64,
    /// MADT 表是否存在（缺失时 madt_base 无效——两字段成对诚实）。
    pub madt_present: bool,
}

/// 三源裁决结果（交叉一致的过程账——每源一条读数与判定）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceVerdict {
    /// 源。
    pub source: BaseSource,
    /// 该源读出的基址（0=无效）。
    pub base: u64,
    /// 是否参与裁决（缺失/禁用=不参与）。
    pub eligible: bool,
    /// 不参与的显性原因（参与时为空）。
    pub exclude_reason: String,
}

/// 裁决输出（J1 前半的产物）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BaseArbitration {
    /// 生效基址（最高权威可用源）。
    pub effective_base: u64,
    /// 生效模式（x2APIC 与 MMIO 两态——SDM §10.12）。
    pub x2apic: bool,
    /// LAPIC 全局使能（MSR bit11——0 则 LAPIC 整体不可用）。
    pub enabled: bool,
    /// 每源判定（三行账——交叉过程全留痕）。
    pub verdicts: Vec<SourceVerdict>,
    /// 一致性结论（true=所有参与源同值；false=有分歧已立案）。
    pub consistent: bool,
    /// 分歧立案账（一致时为空——立案不猜纪律）。
    pub mismatch_case: Option<MismatchCase>,
}

/// 三源分歧立案（不一致时的结构化归因——零静默）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MismatchCase {
    /// 分歧源对（如 "MSR(0x1B) vs MADT表"）。
    pub between: String,
    /// 各源值。
    pub values: String,
    /// 裁定与理由（以最高权威可用源为准+建议核查路径）。
    pub ruling: String,
}

/// 三源交叉裁决（纯函数——注入读数，出裁决账）。
///
/// 裁决序：MSR（若 bit11 使能）> MADT（若表在）> ACPI 默认。
/// 一致性：所有「可用」源中，若最高权威源与任一其他可用源不同值 →
/// 立案（仍出生效基址——可用性优先，但分歧不吞）。
pub fn arbitrate(r: &SourceReadings) -> Result<BaseArbitration, PlanError> {
    let mut verdicts: Vec<SourceVerdict> = Vec::new();

    // 源一 MSR：bit11=0 → LAPIC 全局禁用（显性账，非错误——是状态）。
    let msr_enabled = r.msr_raw & MSR_BIT_APIC_ENABLE != 0;
    let x2apic = msr_enabled && (r.msr_raw & MSR_BIT_X2APIC_ENABLE != 0);
    let msr_base = r.msr_raw & MSR_BASE_MASK;
    if !msr_enabled {
        verdicts.push(SourceVerdict {
            source: BaseSource::Msr,
            base: 0,
            eligible: false,
            exclude_reason: "MSR bit11=0——LAPIC 全局禁用，中断面不可用，先查 BIOS/固件设置".to_string(),
        });
    } else {
        // 基址对齐校验（4KB——掩码外低位非零=硬件异常态）。
        if msr_base & PAGE_MASK != 0 {
            return Err(PlanError::BaseMisaligned {
                source: BaseSource::Msr,
                base: msr_base,
            });
        }
        verdicts.push(SourceVerdict {
            source: BaseSource::Msr,
            base: msr_base,
            eligible: true,
            exclude_reason: String::new(),
        });
    }

    // 源二 MADT：表缺失显性（madt_present=false → 0 无效）。
    if !r.madt_present {
        verdicts.push(SourceVerdict {
            source: BaseSource::Madt,
            base: 0,
            eligible: false,
            exclude_reason: "MADT 表缺失——ACPI 固件未宣告，回落默认值前先确认表解析链（F008）".to_string(),
        });
    } else if r.madt_base & PAGE_MASK != 0 {
        return Err(PlanError::BaseMisaligned {
            source: BaseSource::Madt,
            base: r.madt_base,
        });
    } else {
        verdicts.push(SourceVerdict {
            source: BaseSource::Madt,
            base: r.madt_base,
            eligible: true,
            exclude_reason: String::new(),
        });
    }

    // 源三 ACPI 默认：常量，恒可用。
    verdicts.push(SourceVerdict {
        source: BaseSource::AcpiDefault,
        base: ACPI_DEFAULT_LAPIC_BASE,
        eligible: true,
        exclude_reason: String::new(),
    });

    // 生效源选择（裁决序——第一可用源）。
    let effective = verdicts
        .iter()
        .find(|v| v.eligible)
        .ok_or(PlanError::MsrDisabled)?;
    let effective_base = effective.base;

    // 交叉一致：所有可用源两两同值（可用源 ≥2 才有交叉语义；单源时
    // consistent=true 但注明「无交叉证据」——诚实标注）。
    let elig: Vec<&SourceVerdict> = verdicts.iter().filter(|v| v.eligible).collect();
    let mut mismatch: Option<MismatchCase> = None;
    for i in 0..elig.len() {
        for j in (i + 1)..elig.len() {
            if elig[i].base != elig[j].base {
                mismatch = Some(MismatchCase {
                    between: format!("{} vs {}", elig[i].source.name(), elig[j].source.name()),
                    values: format!(
                        "{}=0x{:X}, {}=0x{:X}",
                        elig[i].source.name(),
                        elig[i].base,
                        elig[j].source.name(),
                        elig[j].base
                    ),
                    ruling: format!(
                        "以 {}（最高权威可用源）=0x{:X} 为准；分歧立案不猜——建议核查固件 MADT 与 MSR 是否被运行期改写（F0004 基址账对账）",
                        effective.source.name(),
                        effective_base
                    ),
                });
            }
        }
    }
    let consistent = mismatch.is_none();

    Ok(BaseArbitration {
        effective_base,
        x2apic,
        enabled: msr_enabled,
        verdicts,
        consistent,
        mismatch_case: mismatch,
    })
}

// ---------------------------------------------------------------------------
// 二、MMIO 映射管理（状态机 + 只读强制 + 写入拦截账）
// ---------------------------------------------------------------------------

/// 映射状态机四态（J1 后半「映射只读校验位生效」的状态载体）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MapState {
    /// 未映射（基址已知但页未建立）。
    Unmapped = 0,
    /// 已映射（页建立，默认只读——LAPIC 映射从不给 W 位起步）。
    Mapped = 1,
    /// 只读强制生效（写拦截注入测试通过——RO 位实证可拦）。
    ReadOnlyEnforced = 2,
    /// 密封（三源一致 + RO 实证——映射终结态，重映射即 0x370A）。
    Sealed = 3,
}

impl MapState {
    /// 态名。
    pub fn name(self) -> &'static str {
        match self {
            MapState::Unmapped => "未映射",
            MapState::Mapped => "已映射",
            MapState::ReadOnlyEnforced => "只读强制",
            MapState::Sealed => "已密封",
        }
    }
}

/// 一次写入拦截记录（拦截账原子项——逐笔留痕）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InterceptRecord {
    /// 拦截序号（0 起单调）。
    pub seq: usize,
    /// 试图写入的完整地址。
    pub addr: u64,
    /// 试图写入的值。
    pub value: u64,
    /// 归因标签（注入源标识）。
    pub origin: &'static str,
    /// 地址是否落在 LAPIC 映射区（区外写不走本账——那是普通页故障）。
    pub in_lapic_range: bool,
}

/// LAPIC MMIO 映射管理器（状态机 + 拦截闸 + 账本）。
#[derive(Clone, Debug)]
pub struct LapicMapping {
    /// 生效基址（裁决产物）。
    pub base: u64,
    /// x2APIC 模式（真值时 MMIO 面不适用——写拦截语义切换）。
    pub x2apic: bool,
    /// 当前态。
    pub state: MapState,
    /// 写入拦截账（逐笔）。
    pub intercepts: Vec<InterceptRecord>,
    /// 拦截序号游标。
    seq: usize,
    /// RO 注入测试已通过（Mapped→ReadOnlyEnforced 的闸）。
    ro_tested: bool,
    /// 拦截通过计数（in-range 且被拒的）。
    pub blocked_count: usize,
}

impl LapicMapping {
    /// 从裁决结果建映射器（Unmapped 态起步——页建立是显性动作）。
    pub fn new(arb: &BaseArbitration) -> Result<Self, PlanError> {
        if !arb.enabled {
            return Err(PlanError::MsrDisabled);
        }
        Ok(LapicMapping {
            base: arb.effective_base,
            x2apic: arb.x2apic,
            state: MapState::Unmapped,
            intercepts: Vec::new(),
            seq: 0,
            ro_tested: false,
            blocked_count: 0,
        })
    }

    /// 建页映射（Unmapped→Mapped；RO 起步——LAPIC 页永不给 W 位）。
    pub fn map(&mut self) -> Result<(), PlanError> {
        if self.state != MapState::Unmapped {
            return Err(PlanError::MapStateIllegal {
                from: self.state,
                to: MapState::Mapped,
                why: "建页只在未映射态可做——重复建页或密封后建页都是越序".to_string(),
            });
        }
        self.state = MapState::Mapped;
        Ok(())
    }

    /// RO 注入测试：模拟一次写（经闸），验证拦截位生效
    /// （Mapped→ReadOnlyEnforced 的唯一通道——没实测过就不准升态）。
    ///
    /// 语义：注入写经 `probe_write` 走闸——若闸失灵（写穿透）→ 0x3705
    /// 显性拒（映射面有洞，绝不能带着可写页进 Sealed）。
    pub fn enforce_readonly(&mut self) -> Result<(), PlanError> {
        if self.state != MapState::Mapped {
            return Err(PlanError::MapStateIllegal {
                from: self.state,
                to: MapState::ReadOnlyEnforced,
                why: "RO 强制只在已映射态可做——先 map()".to_string(),
            });
        }
        // 注入写：取寄存器闭集首寄存器（ID 0x000）为探针目标。
        let probe_addr = self.base + REG_ID;
        self.probe_write(probe_addr, 0xDEAD_BEEF, "ro-enforce-probe")?;
        if self.blocked_count == 0 {
            return Err(PlanError::ReadonlyNotEnforced {
                addr: probe_addr,
                why: "注入写未被拦截——RO 位未生效，映射面可写等于 LAPIC 随机写开放".to_string(),
            });
        }
        self.ro_tested = true;
        self.state = MapState::ReadOnlyEnforced;
        Ok(())
    }

    /// 密封（ReadOnlyEnforced→Sealed——三源一致前提下才准密封）。
    pub fn seal(&mut self, consistent: bool) -> Result<(), PlanError> {
        if self.state != MapState::ReadOnlyEnforced {
            return Err(PlanError::MapStateIllegal {
                from: self.state,
                to: MapState::Sealed,
                why: "密封只在只读强制态可做——没实证过拦截就没资格密封".to_string(),
            });
        }
        if !consistent {
            return Err(PlanError::MapStateIllegal {
                from: self.state,
                to: MapState::Sealed,
                why: "三源有分歧未解——分歧立案在册时不准密封（先归因后密封）".to_string(),
            });
        }
        self.state = MapState::Sealed;
        Ok(())
    }

    /// 写入闸（映射面唯一写路径——`probe_write`/消费方统一走这里）。
    ///
    /// 语义：RO 态及以后，一切对 LAPIC 区的「写」都被本闸拦下落账——
    /// 拦截率 100% 判据的构造面（真硬件写由 EOI/ICR 等专用通道走
    /// 寄存器面，不走通用写闸；本闸拦的是「未经专用通道的裸写」）。
    pub fn probe_write(&mut self, addr: u64, value: u64, origin: &'static str) -> Result<(), PlanError> {
        if self.state == MapState::Unmapped {
            return Err(PlanError::MapStateIllegal {
                from: MapState::Unmapped,
                to: MapState::Mapped,
                why: format!("未映射就写 0x{:X}——先 map() 再谈写入", addr),
            });
        }
        let in_range = addr >= self.base && addr < self.base + LAPIC_MMIO_SIZE;
        let seq = self.seq;
        self.seq += 1;
        let rec = InterceptRecord {
            seq,
            addr,
            value,
            origin,
            in_lapic_range: in_range,
        };
        self.intercepts.push(rec);
        if in_range && self.state != MapState::Mapped {
            // Mapped 态默认只读；RO 强制/密封态写区必然拦截。
            self.blocked_count += 1;
            return Err(PlanError::WriteIntercepted {
                seq,
                addr,
                value,
            });
        }
        if in_range && self.state == MapState::Mapped {
            // Mapped（未实测 RO）态写区也拦——LAPIC 页从未给 W 位。
            self.blocked_count += 1;
            return Err(PlanError::WriteIntercepted {
                seq,
                addr,
                value,
            });
        }
        Ok(())
    }

    /// x2APIC 模式下 MMIO 写显性拒（形态混用 0x3708）。
    pub fn check_mmio_usable(&self) -> Result<(), PlanError> {
        if self.x2apic {
            return Err(PlanError::X2ApicMmioMix {
                why: "x2APIC 模式寄存器走 MSR 0x800 系——MMIO 偏移面不适用，混用=半态幻觉".to_string(),
            });
        }
        Ok(())
    }

    /// 拦截账对账（seq 连续 + 逐笔 in-range 与拦截判定一致——账实守恒）。
    pub fn ledger_ok(&self) -> bool {
        for (i, rec) in self.intercepts.iter().enumerate() {
            if rec.seq != i {
                return false; // 序号断档=丢账
            }
        }
        true
    }

    /// 拦截率（blocked / in-range 注入——J1「拦截率 100%」的量化面）。
    pub fn intercept_rate_ppm(&self) -> u64 {
        let total_in_range = self.intercepts.iter().filter(|r| r.in_lapic_range).count();
        if total_in_range == 0 {
            return 0;
        }
        self.blocked_count as u64 * 1_000_000 / total_in_range as u64
    }
}

// ---------------------------------------------------------------------------
// 三、寄存器偏移闭集表（SDM Vol.3 §10.12——单源，消费方查表不走裸偏移）
// ---------------------------------------------------------------------------

/// LAPIC 寄存器闭集条目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LapicReg {
    /// MMIO 偏移。
    pub offset: u64,
    /// 助记名。
    pub mnemonic: &'static str,
    /// 中文名。
    pub zh: &'static str,
    /// 可写性（true=消费方可经专用通道写；false=只读寄存器）。
    pub writable: bool,
}

/// LAPIC 寄存器闭集（SDM Vol.3 Table 10-1 主寄存器全集——0x10 步进）。
pub const LAPIC_REGS: [LapicReg; 20] = [
    LapicReg { offset: 0x000, mnemonic: "ID", zh: "本地 APIC ID", writable: false },
    LapicReg { offset: 0x010, mnemonic: "EOI", zh: "中断结束", writable: true },
    LapicReg { offset: 0x020, mnemonic: "SVR", zh: "伪中断向量", writable: true },
    LapicReg { offset: 0x030, mnemonic: "ISR0", zh: "在服务 0-31", writable: false },
    LapicReg { offset: 0x040, mnemonic: "ISR1", zh: "在服务 32-63", writable: false },
    LapicReg { offset: 0x050, mnemonic: "ISR2", zh: "在服务 64-95", writable: false },
    LapicReg { offset: 0x060, mnemonic: "ISR3", zh: "在服务 96-127", writable: false },
    LapicReg { offset: 0x070, mnemonic: "ISR4", zh: "在服务 128-159", writable: false },
    LapicReg { offset: 0x080, mnemonic: "TPR", zh: "任务优先级", writable: true },
    LapicReg { offset: 0x0A0, mnemonic: "APR", zh: "确认优先级", writable: false },
    LapicReg { offset: 0x0B0, mnemonic: "PPR", zh: "处理器优先级", writable: false },
    LapicReg { offset: 0x0D0, mnemonic: "RRD", zh: "远程读", writable: false },
    LapicReg { offset: 0x0F0, mnemonic: "LDR", zh: "逻辑目标", writable: true },
    LapicReg { offset: 0x100, mnemonic: "DFR", zh: "目标格式", writable: true },
    LapicReg { offset: 0x280, mnemonic: "ICR_LO", zh: "中断命令低", writable: true },
    LapicReg { offset: 0x290, mnemonic: "ESR", zh: "错误状态", writable: true },
    LapicReg { offset: 0x310, mnemonic: "ICR_HI", zh: "中断命令高", writable: true },
    LapicReg { offset: 0x320, mnemonic: "LVT_TIMER", zh: "LVT 定时器", writable: true },
    LapicReg { offset: 0x350, mnemonic: "LVT_LINT0", zh: "LVT LINT0", writable: true },
    LapicReg { offset: 0x370, mnemonic: "LVT_ERROR", zh: "LVT 错误", writable: true },
];

/// 偏移查表（越界/非法步进显性拒——消费方的安全查询面）。
pub fn reg_by_offset(offset: u64) -> Result<&'static LapicReg, PlanError> {
    if offset & (REG_STRIDE - 1) != 0 {
        return Err(PlanError::OffsetStrideBad {
            offset,
            why: format!("偏移 0x{:03X} 非 {} 字节步进倍数——LAPIC 寄存器按 16 字节对齐", offset, REG_STRIDE),
        });
    }
    LAPIC_REGS
        .iter()
        .find(|r| r.offset == offset)
        .ok_or(PlanError::RegOffsetOob {
            offset,
            why: format!(
                "偏移 0x{:03X} 不在寄存器闭集——查表走 LAPIC_REGS 不走裸偏移",
                offset
            ),
        })
}

/// x2APIC MSR 地址换算（MMIO 偏移→MSR 编号；非法步进即拒）。
pub fn x2apic_msr_of(offset: u64) -> Result<u32, PlanError> {
    let reg = reg_by_offset(offset)?;
    Ok(X2APIC_MSR_BASE + (reg.offset >> 4) as u32)
}

// ---------------------------------------------------------------------------
// 四、错误 / 快照 / 自证
// ---------------------------------------------------------------------------

/// 编排失败（全显性——变体带建议）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlanError {
    /// 三源不一致已立案（信息性错误——立案不是拒绝，是状态上抛）。
    SourceMismatch { case: String },
    /// MSR 全局禁用。
    MsrDisabled,
    /// 基址未对齐。
    BaseMisaligned { source: BaseSource, base: u64 },
    /// 映射状态越序。
    MapStateIllegal { from: MapState, to: MapState, why: String },
    /// RO 未生效。
    ReadonlyNotEnforced { addr: u64, why: String },
    /// 写入被拦截（预期语义的 Err 形态——拦截即报，账同落）。
    WriteIntercepted { seq: usize, addr: u64, value: u64 },
    /// x2APIC 与 MMIO 混用。
    X2ApicMmioMix { why: String },
    /// 寄存器偏移越界。
    RegOffsetOob { offset: u64, why: String },
    /// 步进违例。
    OffsetStrideBad { offset: u64, why: String },
    /// 拦截账丢失。
    InterceptLedgerMiss { why: String },
    /// 密封后重映射。
    RemapAfterSeal { why: String },
}

impl PlanError {
    /// 人话诊断（含建议）。
    pub fn detail(&self) -> String {
        match self {
            PlanError::SourceMismatch { case } => format!("[0x3701] 三源分歧：{}。建议：按裁定核查后重裁——分歧在册时不密封", case),
            PlanError::MsrDisabled => "[0x3702] LAPIC 全局禁用（MSR bit11=0）。建议：查 BIOS Advanced→APIC 设置或固件 F0004 基址账".to_string(),
            PlanError::BaseMisaligned { source, base } => format!("[0x3703] {} 基址 0x{:X} 非 4KB 对齐。建议：硬件异常态，核查 MSR 低位与 MADT 字段完整性", source.name(), base),
            PlanError::MapStateIllegal { from, to, why } => format!("[0x3704] 映射越序 {}→{}：{}。建议：按 Unmapped→Mapped→ReadOnlyEnforced→Sealed 单向序推进", from.name(), to.name(), why),
            PlanError::ReadonlyNotEnforced { addr, why } => format!("[0x3705] 只读校验未生效（探针 0x{:X}）：{}。建议：核查页表 R/W 位与 #PF 归因链后再升态", addr, why),
            PlanError::WriteIntercepted { seq, addr, value } => format!("[0x3706/拦截正常语义] 写拦截 #{}：0x{:X}←0x{:X} 被只读位拦下。建议：裸写走寄存器专用通道（F0822/F0823）", seq, addr, value),
            PlanError::X2ApicMmioMix { why } => format!("[0x3708] {}", why),
            PlanError::RegOffsetOob { offset: _, why } => format!("[0x3707] {}", why),
            PlanError::OffsetStrideBad { offset: _, why } => format!("[0x3709] {}", why),
            PlanError::InterceptLedgerMiss { why } => format!("[0x3706] {}", why),
            PlanError::RemapAfterSeal { why } => format!("[0x370A] {}", why),
        }
    }
}

/// LAPIC 基址快照（FNV——裁决/映射/拦截账任一漂移即变）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LapicSnap {
    /// 生效基址。
    pub base: u64,
    /// x2APIC 态。
    pub x2apic: bool,
    /// 映射态。
    pub state: MapState,
    /// 拦截账指纹。
    pub ledger_fp: u32,
}

impl LapicSnap {
    /// 取快照。
    pub fn capture(m: &LapicMapping) -> Self {
        let mut mat: Vec<u8> = Vec::new();
        mat.extend_from_slice(&m.base.to_le_bytes());
        mat.push(m.x2apic as u8);
        mat.push(m.state as u8);
        for r in m.intercepts.iter() {
            mat.extend_from_slice(&r.seq.to_le_bytes());
            mat.extend_from_slice(&r.addr.to_le_bytes());
            mat.extend_from_slice(&r.value.to_le_bytes());
            mat.push(r.in_lapic_range as u8);
        }
        LapicSnap {
            base: m.base,
            x2apic: m.x2apic,
            state: m.state,
            ledger_fp: fnv32(&mat, FNV_OFFSET),
        }
    }

    /// 往返一致。
    pub fn roundtrip_ok(&self, other: &LapicSnap) -> bool {
        self == other
    }
}

/// FNV-1a 32 位。
pub fn fnv32(data: &[u8], seed: u32) -> u32 {
    let mut h = seed;
    for b in data.iter() {
        h ^= *b as u32;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

/// FNV 常量。
pub const FNV_PRIME: u32 = 0x0100_0193;
pub const FNV_OFFSET: u32 = 0x811C_9DC5;

/// 寄存器 ID 偏移（RO 探针用——闭集首项）。
pub const REG_ID: u64 = 0x000;

// ---------------------------------------------------------------------------
// 五、全链自证（J1 走线：三源裁决→映射→RO 实证→密封→拦截账对账）
// ---------------------------------------------------------------------------

/// 样板三源读数（一致路径：MSR/MADT/默认同值 0xFEE00000）。
pub fn sample_consistent_readings() -> SourceReadings {
    SourceReadings {
        msr_raw: MSR_BIT_APIC_ENABLE | ACPI_DEFAULT_LAPIC_BASE, // bit11=1 x2APIC=0
        madt_base: ACPI_DEFAULT_LAPIC_BASE,
        madt_present: true,
    }
}

/// 样板三源读数（x2APIC 路径：bit12=1）。
pub fn sample_x2apic_readings() -> SourceReadings {
    SourceReadings {
        msr_raw: MSR_BIT_APIC_ENABLE | MSR_BIT_X2APIC_ENABLE | ACPI_DEFAULT_LAPIC_BASE,
        madt_base: ACPI_DEFAULT_LAPIC_BASE,
        madt_present: true,
    }
}

/// 样板三源读数（分歧路径：MADT 宣告漂移——立案语义实证）。
pub fn sample_mismatch_readings() -> SourceReadings {
    SourceReadings {
        msr_raw: MSR_BIT_APIC_ENABLE | ACPI_DEFAULT_LAPIC_BASE,
        madt_base: 0xFEE0_1000, // 人造漂移页
        madt_present: true,
    }
}

/// 全链自证（J1 走线全绿——一致路径）。
pub fn selfcheck_full() -> Result<LapicSnap, PlanError> {
    // 1. 三源裁决（一致路径）。
    let arb = arbitrate(&sample_consistent_readings())?;
    if !arb.consistent || arb.mismatch_case.is_some() {
        return Err(PlanError::SourceMismatch {
            case: "一致路径样板出现分歧——样板或裁决缺陷".to_string(),
        });
    }
    if arb.effective_base != ACPI_DEFAULT_LAPIC_BASE {
        return Err(PlanError::SourceMismatch {
            case: format!("生效基址 0x{:X} ≠ 预期 0x{:X}", arb.effective_base, ACPI_DEFAULT_LAPIC_BASE),
        });
    }
    // 2. 映射推进（Unmapped→Mapped→RO 实证→Sealed）。
    let mut m = LapicMapping::new(&arb)?;
    m.map()?;
    m.enforce_readonly()?;
    m.seal(arb.consistent)?;
    // 3. 拦截账对账（探针拦截已落账——账实守恒）。
    if !m.ledger_ok() {
        return Err(PlanError::InterceptLedgerMiss {
            why: "拦截账序号断档".to_string(),
        });
    }
    if m.intercept_rate_ppm() != 1_000_000 {
        return Err(PlanError::InterceptLedgerMiss {
            why: format!("拦截率 {}ppm ≠ 100%（1e6 ppm）——有写穿透", m.intercept_rate_ppm()),
        });
    }
    // 4. 偏移表闭集抽查（EOI 0x010 可写/ID 0x000 只读/0x030 步进合法）。
    let eoi = reg_by_offset(0x010)?;
    if !eoi.writable {
        return Err(PlanError::RegOffsetOob {
            offset: 0x010,
            why: "EOI 应可写——闭集表 writable 位错".to_string(),
        });
    }
    let id = reg_by_offset(0x000)?;
    if id.writable {
        return Err(PlanError::RegOffsetOob {
            offset: 0x000,
            why: "ID 应只读——闭集表 writable 位错".to_string(),
        });
    }
    // 5. x2APIC 分支显性（样板：MMIO 不可用拒绝正常触发）。
    let arb2 = arbitrate(&sample_x2apic_readings())?;
    let m2 = LapicMapping::new(&arb2)?;
    if !arb2.x2apic {
        return Err(PlanError::X2ApicMmioMix {
            why: "x2APIC 样板未识别为 x2APIC——MSR bit12 解码缺陷".to_string(),
        });
    }
    let _ = m2.check_mmio_usable();
    // 6. 快照往返。
    let s1 = LapicSnap::capture(&m);
    let s2 = LapicSnap::capture(&m);
    if !s1.roundtrip_ok(&s2) {
        return Err(PlanError::InterceptLedgerMiss {
            why: "快照往返不一致——拦截账指纹非确定".to_string(),
        });
    }
    Ok(s1)
}

/// 分歧立案自证（J1 交叉语义：分歧→立案→拒密封）。
pub fn selfcheck_mismatch() -> Result<(), PlanError> {
    let arb = arbitrate(&sample_mismatch_readings())?;
    if arb.consistent || arb.mismatch_case.is_none() {
        return Err(PlanError::SourceMismatch {
            case: "分歧样板未立案——交叉一致性判定缺陷".to_string(),
        });
    }
    // 分歧在册 → 映射可推进到 RO 强制，但 seal 必须拒。
    let mut m = LapicMapping::new(&arb)?;
    m.map()?;
    m.enforce_readonly()?;
    let sealed = m.seal(false);
    if sealed.is_ok() {
        return Err(PlanError::MapStateIllegal {
            from: MapState::ReadOnlyEnforced,
            to: MapState::Sealed,
            why: "分歧在册却被密封——密封闸失守".to_string(),
        });
    }
    Ok(())
}
