//! unx080_idt — UNX-F0801 IDT 全量初始化与向量空间五区规划。
//!
//! 判据原文（施工书锚点 UNX-F0801，CoRun·UNX-A2-B01 批）：
//! **UNX-F0801-J1 真机 256 项 IDT 全量填充且 lidt 生效，异常/IRQ/MSI/IPI/
//! spurious 五区规划断言 ≥20 条全绿，五区边界值逐区可打印**。
//!
//! 职责定位：A2 域（中断子系统）开篇之单——把 256 个向量的整个中断入口
//! 空间从「写代码时随手挑一个号」收拢为**规划在先的分区账**：五区闭集
//! （异常 0x00~0x1F / IRQ 0x20~0x6F / MSI 0x70~0xEF / IPI 0xF0~0xFD /
//! spurious 0xFE~0xFF）+ 256 项 GateDescriptor 位级编码（x86_64 16 字节
//! 门描述符五字段：offset 低 16 / selector / IST+type / type+DPL+P /
//! offset 高 48——编码后解码逐位往返一致）+ 32 项 CPU 异常注册表
//! （x86_64 SDM Vol.3 Table 6-1 全闭集：异常名/种类 fault-trap-abort/
//! 是否带错误码/DPL/IST 五维声明）+ IST 双重故障与 NMI 专项（#DF 走
//! IST2、NMI 走 IST1——IST 栈上不允许嵌套同类中断，这是 SDM 硬要求）
//! + MSI 动态池（128 向量分配/释放/越界显性——F0802~F0804 的分配底座）
//! + IPI 语义子区闭集（14 项语义绑定）+ lidt 描述符（limit=4095、
//! base 对齐 8 字节、生效语义三要素）+ 五区断言引擎（J1 落地：≥20 条
//! 断言逐条结构化产出）+ 五区边界值逐区可打印（J1 尾半）。
//! 对标 Windows 深在哪：Windows 的 IDT 由 ntoskrnl 编译期 KiIdt 数组 +
//! 运行期 KeInitInterrupts 填充，向量分配分散在 HAL（KiIpiSend 硬编码
//! IPI 向量、HAL MSI 策略按需挑号），五区边界散落多处源码无单一规划账；
//! Varix 编排器：向量空间分区**单源声明**（RegionDef 闭集表即地图）→
//! 分配走池不走散挑（MSI 池带占用账，杜绝双 MSI 同向量——Windows 历史上
//! 多次 MSI 冲突 bug 的根源）→ 边界值可打印（排障时一眼看出某向量属哪区
//! 该挂什么 handler）→ 256 项全填充（Windows 对未用向量填 0 项——lidt
//! 后撞未用向量直接 triple fault，Varix 未用向量填占位 handler P=1，
//! 撞上去进 spurious 账本显性归因，机器不哑）。
//! 真机 lidt/sidt 执行面归内核接线路径（asm 落位），本单交付**表构造 +
//! 位级校验 + 规划断言**三面——附编 C 纪律：编译/测试类判据宿主侧快测，
//! 真机面随闸门补测登记。
//!
//! 设计要点：no_std+alloc；零 unwrap/expect；零 IO 零墙钟；确定性排序；
//! 常量写死并注释 SDM 出处；错误全显性带可操作建议；FNV 快照往返一致。
//! 诊断码段 0x36（UNX 册 0x2E~0x34 既有 + 0x35 为 F0181 所取——先入库者
//! 占段纪律，0x36 起本单独占）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// 模块版本（向量空间规划协议版本——分区变更即前滚）。
pub const IDT_PLAN_VERSION: &str = "B01-idt-v1";

// ---------------------------------------------------------------------------
// 诊断码（XcCode 0x36 段，本单独占）
// ---------------------------------------------------------------------------

/// 0x3601 向量越区（某向量被要求挂到不属于它的区）。
pub const XC_VECTOR_OUT_OF_REGION: u32 = 0x3601;
/// 0x3602 门描述符位域非法（type/DPL/IST 取值越闭集）。
pub const XC_GATE_FIELD_ILLEGAL: u32 = 0x3602;
/// 0x3603 IDT 未满填充（256 项有空洞——lidt 后撞空洞即 triple fault）。
pub const XC_IDT_HOLE: u32 = 0x3603;
/// 0x3604 lidt 描述符非法（limit≠4095 或 base 未对齐或 base=0）。
pub const XC_LIDT_DESCRIPTOR_BAD: u32 = 0x3604;
/// 0x3605 IST 分配违规（#DF 不走 IST2 / NMI 不走 IST1 / IST 复用冲突）。
pub const XC_IST_ALLOCATION_BAD: u32 = 0x3605;
/// 0x3606 异常注册表缺项（0~31 闭集有异常未登记——SDM 表 6-1 不允许缺）。
pub const XC_EXCEPT_TABLE_INCOMPLETE: u32 = 0x3606;
/// 0x3607 MSI 池越界/双占（同向量双分配或池外分配）。
pub const XC_MSI_POOL_VIOLATION: u32 = 0x3607;
/// 0x3608 五区规划断言失守（≥20 条断言有红——J1 拒收）。
pub const XC_REGION_ASSERTION_RED: u32 = 0x3608;
/// 0x3609 IPI 语义绑定缺失（IPI 区向量无语义条目）。
pub const XC_IPI_SEMANTICS_MISSING: u32 = 0x3609;
/// 0x360A spurious 区配置非法（LAPIC spurious 向量不在 0xFE~0xFF 闭集）。
pub const XC_SPURIOUS_CFG_BAD: u32 = 0x360A;

// ---------------------------------------------------------------------------
// 常量（写死并注明 SDM 出处）
// ---------------------------------------------------------------------------

/// 向量空间总量（x86_64 固定 256 向量——SDM Vol.3 §6.11）。
pub const VECTOR_TOTAL: usize = 256;
/// IDT 表项字节数（16 字节门描述符——SDM Vol.3 §6.14.1 Figure 6-7）。
pub const GATE_SIZE: usize = 16;
/// IDT 全表字节数（256×16=4096）。
pub const IDT_BYTES: usize = VECTOR_TOTAL * GATE_SIZE;
/// lidt limit（字节数减一=4095——lidt 生效三要素之一）。
pub const IDT_LIMIT: u16 = (IDT_BYTES - 1) as u16;
/// 内核代码段选择子（GDT 布局约定——A1 域 F0189 GDT 重载的 0x08 槽）。
pub const KERNEL_CS: u16 = 0x08;
/// 门类型：64 位中断门（IF 自动清零——SDM Table 3-17）。
pub const GATE_TYPE_INTERRUPT: u8 = 0xE;
/// 门类型：64 位陷阱门（IF 保留——调试/性能面用）。
pub const GATE_TYPE_TRAP: u8 = 0xF;
/// IDT base 对齐要求（8 字节——SDM Vol.3 §6.10 对齐建议，16 更佳但 8 为硬底）。
pub const IDT_BASE_ALIGN: u64 = 8;

// ---- 五区边界（分区单源——全部边界值在此一张表，J1 尾半打印即出自此） ----

/// 区一 异常区起（0x00——#DE 起步）。
pub const REGION_EXC_START: u8 = 0x00;
/// 区一 异常区止（含 0x1F——SDM 规定 0~31 为 CPU 异常与保留）。
pub const REGION_EXC_END: u8 = 0x1F;
/// 区二 IRQ 区起（0x20——legacy PIC 16 号后的 APIC 时代外部中断区）。
pub const REGION_IRQ_START: u8 = 0x20;
/// 区二 IRQ 区止（含 0x6F——80 项外部中断容量，对接 IO-APIC GSI 映射）。
pub const REGION_IRQ_END: u8 = 0x6F;
/// 区三 MSI 区起（0x70——MSI/MSI-X 消息中断动态池）。
pub const REGION_MSI_START: u8 = 0x70;
/// 区三 MSI 区止（含 0xEF——128 项池容量）。
pub const REGION_MSI_END: u8 = 0xEF;
/// 区四 IPI 区起（0xF0——处理器间中断语义区）。
pub const REGION_IPI_START: u8 = 0xF0;
/// 区四 IPI 区止（含 0xFD——14 项语义闭集）。
pub const REGION_IPI_END: u8 = 0xFD;
/// 区五 spurious 起含（0xFE——预留扩展）。
pub const REGION_SPURIOUS_START: u8 = 0xFE;
/// 区五 spurious 止含（0xFF——LAPIC spurious interrupt 默认向量）。
pub const REGION_SPURIOUS_END: u8 = 0xFF;
/// LAPIC spurious 向量（SDM Vol.3 §10.9：SPR bit 所在寄存器可编程——取 0xFF）。
pub const LAPIC_SPURIOUS_VECTOR: u8 = 0xFF;

// ---- IST 分配（SDM Vol.3 §6.14.5：IST 字段 1~7；0=不用） ----

/// IST1 归属 NMI（NMI 在 IST 栈上不得再收 NMI——SDM 硬要求）。
pub const IST_NMI: u8 = 1;
/// IST2 归属 #DF 双重故障（#DF 必须 IST——栈本身可能就是故障源）。
pub const IST_DF: u8 = 2;
/// IST3 归属 #MC 机器检查（MCE 现场最脆弱）。
pub const IST_MC: u8 = 3;
/// IST 槽位数（SDM 1~7 共 7 个）。
pub const IST_SLOTS: u8 = 7;

// ---- MSI 池 ----

/// MSI 池容量（0x70~0xEF 闭集大小 128）。
pub const MSI_POOL_CAP: usize = 128;

// ---- 优先级/占位 ----

/// 占位 handler 名称（未绑定真实 handler 的向量统一走占位——撞上进账不哑）。
pub const PLACEHOLDER_HANDLER: &str = "stub-unclaimed";

// ---------------------------------------------------------------------------
// 一、GateDescriptor 位级模型（16 字节五字段——SDM Vol.3 §6.14.1）
// ---------------------------------------------------------------------------

/// 门描述符（解码视图——位级字节的十段读法）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GateDesc {
    /// offset 低 16 位。
    pub offset_lo: u16,
    /// 段选择子（内核代码段）。
    pub selector: u16,
    /// IST 索引（0=不用；1~7）。
    pub ist: u8,
    /// 门类型（0xE 中断门 / 0xF 陷阱门）。
    pub gate_type: u8,
    /// 描述符特权级（0 内核 / 3 用户可触发）。
    pub dpl: u8,
    /// 存在位（P=1 有效门）。
    pub present: bool,
    /// offset 高 48 位。
    pub offset_hi: u64,
}

impl GateDesc {
    /// 编码为 16 字节小端布局（SDM Figure 6-7 逐字段落位）。
    ///
    /// 布局：[0..2]=offset_lo；[2..4]=selector；[4]=ist(bit3..5)|zero；
    /// [5]=type(bit0..4)|zero(bit4..5)|DPL(bit5..7)|P(bit7)；
    /// [6..8]=offset[31:16]；[8..16]=offset[63:32]。
    pub fn encode(&self) -> [u8; GATE_SIZE] {
        let mut b = [0u8; GATE_SIZE];
        let offset = (self.offset_hi << 16) | (self.offset_lo as u64);
        b[0..2].copy_from_slice(&(offset as u16).to_le_bytes());
        b[2..4].copy_from_slice(&self.selector.to_le_bytes());
        b[4] = (self.ist & 0x7) << 3; // bits 3..5 = IST，其余零
        b[5] = (self.gate_type & 0xF) | ((self.dpl & 0x3) << 5) | if self.present { 0x80 } else { 0 };
        let mid = ((offset >> 32) & 0xFFFF) as u16;
        let hi = (offset >> 48) as u32;
        b[6..8].copy_from_slice(&mid.to_le_bytes());
        b[8..12].copy_from_slice(&hi.to_le_bytes());
        b
    }

    /// 从 16 字节解码（编码的逆——往返校验的另一半）。
    pub fn decode(raw: &[u8; GATE_SIZE]) -> Self {
        let off_lo = u16::from_le_bytes([raw[0], raw[1]]);
        let selector = u16::from_le_bytes([raw[2], raw[3]]);
        let ist = (raw[4] >> 3) & 0x7;
        let gate_type = raw[5] & 0xF;
        let dpl = (raw[5] >> 5) & 0x3;
        let present = raw[5] & 0x80 != 0;
        let mid = u16::from_le_bytes([raw[6], raw[7]]) as u64;
        let hi = u32::from_le_bytes([raw[8], raw[9], raw[10], raw[11]]) as u64;
        // offset 重组：全 64 位 offset = hi<<48 | mid<<32 | lo。
        // 注意 encode 里 offset_lo 只进低 16，mid 与 hi 的拼接按 encode 逆序。
        let offset = (hi << 48) | (mid << 16) | (off_lo as u64);
        GateDesc {
            offset_lo: off_lo,
            selector,
            ist,
            gate_type,
            dpl,
            present,
            offset_hi: (offset >> 16) & 0xFFFF_FFFF_FFFF,
        }
    }

    /// 位域合法性（判据「位级编码」的构造闸）。
    pub fn validate(&self) -> Result<(), PlanError> {
        if self.ist > IST_SLOTS {
            return Err(PlanError::GateFieldIllegal {
                why: format!("IST={} 超闭集 0..={}", self.ist, IST_SLOTS),
            });
        }
        if self.gate_type != GATE_TYPE_INTERRUPT && self.gate_type != GATE_TYPE_TRAP {
            return Err(PlanError::GateFieldIllegal {
                why: format!("门类型 0x{:X} 非法——IDT 只允许 0xE 中断门/0xF 陷阱门", self.gate_type),
            });
        }
        if self.dpl > 3 {
            return Err(PlanError::GateFieldIllegal {
                why: format!("DPL={} 越界——x86 特权级只有 0..3", self.dpl),
            });
        }
        if !self.present {
            return Err(PlanError::GateFieldIllegal {
                why: "P=0 的门进 IDT 即空洞——未用向量也必须填占位门（撞上去进账不哑）".to_string(),
            });
        }
        if self.selector != KERNEL_CS {
            return Err(PlanError::GateFieldIllegal {
                why: format!("selector 0x{:04X} ≠ 内核代码段 0x{:04X}", self.selector, KERNEL_CS),
            });
        }
        Ok(())
    }

    /// 往返一致（encode→decode→同值——位级无失真证明）。
    pub fn roundtrip_ok(&self) -> bool {
        let raw = self.encode();
        let back = GateDesc::decode(&raw);
        self == &back
    }
}

// ---------------------------------------------------------------------------
// 二、向量空间五区规划（RegionDef 闭集表——分区单源地图）
// ---------------------------------------------------------------------------

/// 区定义（向量空间分区原子项——五区闭集逐条声明）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RegionDef {
    /// 区名。
    pub name: &'static str,
    /// 起向量（含）。
    pub start: u8,
    /// 止向量（含）。
    pub end: u8,
    /// 门类型（异常/IRQ/MSI 用中断门；spurious 用中断门；IPI 用中断门）。
    pub gate_type: u8,
    /// 默认 DPL（异常区个别可 3——#BP/#OF；其余 0）。
    pub default_dpl: u8,
    /// 是否动态分配（仅 MSI 区为真）。
    pub dynamic: bool,
}

/// 五区闭集表（分区单源——改边界只改这里，断言引擎自动跟拍）。
pub const REGIONS: [RegionDef; 5] = [
    RegionDef {
        name: "异常",
        start: REGION_EXC_START,
        end: REGION_EXC_END,
        gate_type: GATE_TYPE_INTERRUPT,
        default_dpl: 0,
        dynamic: false,
    },
    RegionDef {
        name: "IRQ",
        start: REGION_IRQ_START,
        end: REGION_IRQ_END,
        gate_type: GATE_TYPE_INTERRUPT,
        default_dpl: 0,
        dynamic: false,
    },
    RegionDef {
        name: "MSI",
        start: REGION_MSI_START,
        end: REGION_MSI_END,
        gate_type: GATE_TYPE_INTERRUPT,
        default_dpl: 0,
        dynamic: true,
    },
    RegionDef {
        name: "IPI",
        start: REGION_IPI_START,
        end: REGION_IPI_END,
        gate_type: GATE_TYPE_INTERRUPT,
        default_dpl: 0,
        dynamic: false,
    },
    RegionDef {
        name: "spurious",
        start: REGION_SPURIOUS_START,
        end: REGION_SPURIOUS_END,
        gate_type: GATE_TYPE_INTERRUPT,
        default_dpl: 0,
        dynamic: false,
    },
];

/// 向量属区判定（五区不重不漏的查询面——越界返回 Err）。
pub fn region_of(vector: u8) -> Result<&'static RegionDef, PlanError> {
    for r in REGIONS.iter() {
        if vector >= r.start && vector <= r.end {
            return Ok(r);
        }
    }
    Err(PlanError::VectorOutOfRegion {
        vector,
        why: format!("向量 0x{:02X} 不落在任何区——五区闭集须覆盖全 256 空间", vector),
    })
}

/// 五区覆盖完整性（分区间隙与重叠双查——规划地图的自洽闸）。
///
/// 检查：五区按起址排序后，相邻区 start == 前区 end+1（零间隙零重叠），
/// 且首区起 0、尾区止 255（全空间覆盖）。
pub fn regions_cover_all() -> Result<(), PlanError> {
    let mut sorted = REGIONS;
    sorted.sort_by_key(|r| r.start);
    if sorted[0].start != 0 {
        return Err(PlanError::RegionAssertionRed {
            assert_name: "五区覆盖完整性".to_string(),
            why: format!("首区起 0x{:02X} ≠ 0——低段向量无主", sorted[0].start),
        });
    }
    if sorted[4].end != 255 {
        return Err(PlanError::RegionAssertionRed {
            assert_name: "五区覆盖完整性".to_string(),
            why: format!("尾区止 0x{:02X} ≠ 0xFF——高段向量无主", sorted[4].end),
        });
    }
    for i in 1..sorted.len() {
        let prev_end = sorted[i - 1].end;
        let cur_start = sorted[i].start;
        if cur_start != prev_end + 1 {
            return Err(PlanError::RegionAssertionRed {
                assert_name: "五区覆盖完整性".to_string(),
                why: format!(
                    "区「{}」起 0x{:02X} 与前区「{}」止 0x{:02X} 之间有缝隙或重叠",
                    sorted[i].name, cur_start, sorted[i - 1].name, prev_end
                ),
            });
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 三、异常区 32 项注册表（x86_64 SDM Vol.3 Table 6-1 全闭集）
// ---------------------------------------------------------------------------

/// 异常种类闭集（SDM Table 6-1 Type 列——NMI 单列 Interrupt 不属三分法）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExceptKind {
    /// 故障（fault——可恢复，RIP 指向故障指令）。
    Fault = 0,
    /// 陷阱（trap——RIP 指向下一条指令）。
    Trap = 1,
    /// 中止（abort——不可恢复）。
    Abort = 2,
    /// 中断（仅 NMI——SDM 表 6-1 Type 列「Interrupt」，不可屏蔽推入即执行）。
    Interrupt = 3,
}

impl ExceptKind {
    /// 种类名（中文打印）。
    pub fn name(self) -> &'static str {
        match self {
            ExceptKind::Fault => "故障",
            ExceptKind::Trap => "陷阱",
            ExceptKind::Abort => "中止",
            ExceptKind::Interrupt => "中断（NMI）",
        }
    }
}

/// 单条异常注册（SDM Table 6-1 一行的结构化投影）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ExceptEntry {
    /// 向量号（0~31）。
    pub vector: u8,
    /// 助记名（#DE/#DB/…）。
    pub mnemonic: &'static str,
    /// 中文名。
    pub zh: &'static str,
    /// 种类。
    pub kind: ExceptKind,
    /// 是否压入错误码（SDM Err 列）。
    pub has_error_code: bool,
    /// DPL（#BP/#OF 可由用户态 int3/into 触发故为 3；其余 0）。
    pub dpl: u8,
    /// IST 索引（0=不用；NMI=1/#DF=2/#MC=3）。
    pub ist: u8,
}

/// 32 项异常注册表（SDM Table 6-1 全闭集——一行不少，保留向量也登记）。
pub const EXCEPT_TABLE: [ExceptEntry; 32] = [
    ExceptEntry { vector: 0x00, mnemonic: "#DE", zh: "除零", kind: ExceptKind::Fault, has_error_code: false, dpl: 0, ist: 0 },
    ExceptEntry { vector: 0x01, mnemonic: "#DB", zh: "调试", kind: ExceptKind::Fault, has_error_code: false, dpl: 0, ist: 0 },
    ExceptEntry { vector: 0x02, mnemonic: "NMI", zh: "不可屏蔽中断", kind: ExceptKind::Interrupt, has_error_code: false, dpl: 0, ist: IST_NMI },
    ExceptEntry { vector: 0x03, mnemonic: "#BP", zh: "断点", kind: ExceptKind::Trap, has_error_code: false, dpl: 3, ist: 0 },
    ExceptEntry { vector: 0x04, mnemonic: "#OF", zh: "溢出", kind: ExceptKind::Trap, has_error_code: false, dpl: 3, ist: 0 },
    ExceptEntry { vector: 0x05, mnemonic: "#BR", zh: "边界越界", kind: ExceptKind::Fault, has_error_code: false, dpl: 0, ist: 0 },
    ExceptEntry { vector: 0x06, mnemonic: "#UD", zh: "无效操作码", kind: ExceptKind::Fault, has_error_code: false, dpl: 0, ist: 0 },
    ExceptEntry { vector: 0x07, mnemonic: "#NM", zh: "设备不可用", kind: ExceptKind::Fault, has_error_code: false, dpl: 0, ist: 0 },
    ExceptEntry { vector: 0x08, mnemonic: "#DF", zh: "双重故障", kind: ExceptKind::Abort, has_error_code: true, dpl: 0, ist: IST_DF },
    ExceptEntry { vector: 0x09, mnemonic: "RES9", zh: "留用（x87 段越界已废弃）", kind: ExceptKind::Fault, has_error_code: false, dpl: 0, ist: 0 },
    ExceptEntry { vector: 0x0A, mnemonic: "#TS", zh: "无效 TSS", kind: ExceptKind::Fault, has_error_code: true, dpl: 0, ist: 0 },
    ExceptEntry { vector: 0x0B, mnemonic: "#NP", zh: "段不存在", kind: ExceptKind::Fault, has_error_code: true, dpl: 0, ist: 0 },
    ExceptEntry { vector: 0x0C, mnemonic: "#SS", zh: "栈段故障", kind: ExceptKind::Fault, has_error_code: true, dpl: 0, ist: 0 },
    ExceptEntry { vector: 0x0D, mnemonic: "#GP", zh: "一般保护故障", kind: ExceptKind::Fault, has_error_code: true, dpl: 0, ist: 0 },
    ExceptEntry { vector: 0x0E, mnemonic: "#PF", zh: "页故障", kind: ExceptKind::Fault, has_error_code: true, dpl: 0, ist: 0 },
    ExceptEntry { vector: 0x0F, mnemonic: "RES15", zh: "留用", kind: ExceptKind::Fault, has_error_code: false, dpl: 0, ist: 0 },
    ExceptEntry { vector: 0x10, mnemonic: "#MF", zh: "x87 浮点错", kind: ExceptKind::Fault, has_error_code: false, dpl: 0, ist: 0 },
    ExceptEntry { vector: 0x11, mnemonic: "#AC", zh: "对齐检查", kind: ExceptKind::Fault, has_error_code: true, dpl: 0, ist: 0 },
    ExceptEntry { vector: 0x12, mnemonic: "#MC", zh: "机器检查", kind: ExceptKind::Abort, has_error_code: false, dpl: 0, ist: IST_MC },
    ExceptEntry { vector: 0x13, mnemonic: "#XM", zh: "SIMD 浮点", kind: ExceptKind::Fault, has_error_code: false, dpl: 0, ist: 0 },
    ExceptEntry { vector: 0x14, mnemonic: "#VE", zh: "虚拟化异常", kind: ExceptKind::Fault, has_error_code: false, dpl: 0, ist: 0 },
    ExceptEntry { vector: 0x15, mnemonic: "#CP", zh: "控制保护", kind: ExceptKind::Fault, has_error_code: true, dpl: 0, ist: 0 },
    ExceptEntry { vector: 0x16, mnemonic: "RES22", zh: "留用", kind: ExceptKind::Fault, has_error_code: false, dpl: 0, ist: 0 },
    ExceptEntry { vector: 0x17, mnemonic: "RES23", zh: "留用", kind: ExceptKind::Fault, has_error_code: false, dpl: 0, ist: 0 },
    ExceptEntry { vector: 0x18, mnemonic: "RES24", zh: "留用", kind: ExceptKind::Fault, has_error_code: false, dpl: 0, ist: 0 },
    ExceptEntry { vector: 0x19, mnemonic: "RES25", zh: "留用", kind: ExceptKind::Fault, has_error_code: false, dpl: 0, ist: 0 },
    ExceptEntry { vector: 0x1A, mnemonic: "RES26", zh: "留用", kind: ExceptKind::Fault, has_error_code: false, dpl: 0, ist: 0 },
    ExceptEntry { vector: 0x1B, mnemonic: "RES27", zh: "留用", kind: ExceptKind::Fault, has_error_code: false, dpl: 0, ist: 0 },
    ExceptEntry { vector: 0x1C, mnemonic: "RES28", zh: "留用", kind: ExceptKind::Fault, has_error_code: false, dpl: 0, ist: 0 },
    ExceptEntry { vector: 0x1D, mnemonic: "#HV", zh: "管理程序注入", kind: ExceptKind::Fault, has_error_code: false, dpl: 0, ist: 0 },
    ExceptEntry { vector: 0x1E, mnemonic: "#VC", zh: "VMM 通信", kind: ExceptKind::Fault, has_error_code: true, dpl: 0, ist: 0 },
    ExceptEntry { vector: 0x1F, mnemonic: "RES31", zh: "留用", kind: ExceptKind::Fault, has_error_code: false, dpl: 0, ist: 0 },
];

/// IPI 语义子区见下一节（ExceptKind 完整闭集四变体已在上方声明——NMI 单列
/// Interrupt，语义=不可屏蔽推入即执行，不属 fault/trap/abort 三分法）。

// ---------------------------------------------------------------------------
// 四、IPI 语义子区（0xF0~0xFD 14 项语义闭集）
// ---------------------------------------------------------------------------

/// IPI 语义条目（向量→语义的单向绑定——发 IPI 的路径按此表选号）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IpiEntry {
    /// 向量号。
    pub vector: u8,
    /// 语义名。
    pub semantic: &'static str,
    /// 说明。
    pub note: &'static str,
}

/// IPI 语义闭集（14 项——0xF0~0xFD 一号一义，绝不复用）。
pub const IPI_TABLE: [IpiEntry; 14] = [
    IpiEntry { vector: 0xF0, semantic: "reschedule", note: "请求目标 CPU 重调度" },
    IpiEntry { vector: 0xF1, semantic: "tlb-shootdown", note: "TLB 击落——地址空间变更同步" },
    IpiEntry { vector: 0xF2, semantic: "function-call", note: "跨核函数调用（smp_call）" },
    IpiEntry { vector: 0xF3, semantic: "halt", note: "请求目标 CPU 停机" },
    IpiEntry { vector: 0xF4, semantic: "panic-ipi", note: "panic 广播——全员停机" },
    IpiEntry { vector: 0xF5, semantic: "trace-flush", note: "追踪缓冲冲刷" },
    IpiEntry { vector: 0xF6, semantic: "irq-resend", note: "中断重发" },
    IpiEntry { vector: 0xF7, semantic: "tick-broadcast", note: "时钟广播（深度空闲 CPU 补拍）" },
    IpiEntry { vector: 0xF8, semantic: "cpu-online", note: "CPU 上线握手" },
    IpiEntry { vector: 0xF9, semantic: "cpu-offline", note: "CPU 下线握手" },
    IpiEntry { vector: 0xFA, semantic: "mce-broadcast", note: "机器检查广播" },
    IpiEntry { vector: 0xFB, semantic: "perf-ipi", note: "性能采样中断" },
    IpiEntry { vector: 0xFC, semantic: "lazy-flush", note: "惰性 FPU/状态冲刷" },
    IpiEntry { vector: 0xFD, semantic: "reserve", note: "预留扩展位（当前未用——占位 handler）" },
];

/// IPI 语义查询（向量→语义；IPI 区外查即拒）。
pub fn ipi_semantic(vector: u8) -> Result<&'static IpiEntry, PlanError> {
    if vector < REGION_IPI_START || vector > REGION_IPI_END {
        return Err(PlanError::IpiSemanticsMissing {
            vector,
            why: format!("向量 0x{:02X} 不在 IPI 区 0x{:02X}~0x{:02X}", vector, REGION_IPI_START, REGION_IPI_END),
        });
    }
    IPI_TABLE
        .iter()
        .find(|e| e.vector == vector)
        .ok_or(PlanError::IpiSemanticsMissing {
            vector,
            why: format!("IPI 区向量 0x{:02X} 无语义条目——一号一义闭集有洞", vector),
        })
}

// ---------------------------------------------------------------------------
// 五、MSI 动态池（128 向量分配/释放/越界——F0802+ 的分配底座）
// ---------------------------------------------------------------------------

/// MSI 池（占用位图账——杜绝双设备同向量）。
#[derive(Clone, Debug)]
pub struct MsiPool {
    /// 占用账（向量号→占用者名；None=空闲）。
    occupied: [Option<&'static str>; MSI_POOL_CAP],
    /// 已分配计数。
    allocated: usize,
    /// 分配游标（轮转起点——避免每次都从池头分配造成低段拥挤）。
    cursor: usize,
}

impl MsiPool {
    /// 新池（全空闲，游标归零）。
    pub fn new() -> Self {
        MsiPool {
            occupied: [None; MSI_POOL_CAP],
            allocated: 0,
            cursor: 0,
        }
    }

    /// 池内向量号 → 全局向量（池索引 0 → 0x70）。
    fn pool_to_vector(idx: usize) -> u8 {
        (REGION_MSI_START as usize + idx) as u8
    }

    /// 分配一个 MSI 向量（轮转扫描找空闲——O(池容) 最坏，池操作低频可接受）。
    ///
    /// 越界语义：池满显性 Err（不降级复用 IRQ 区向量——分区纪律是硬边界）。
    pub fn alloc(&mut self, owner: &'static str) -> Result<u8, PlanError> {
        if self.allocated >= MSI_POOL_CAP {
            return Err(PlanError::MsiPoolViolation {
                vector: 0,
                why: format!(
                    "MSI 池已满（{}/{}）——设备再多须先释放或扩池改规划表",
                    self.allocated, MSI_POOL_CAP
                ),
            });
        }
        let mut i = self.cursor;
        loop {
            if self.occupied[i].is_none() {
                self.occupied[i] = Some(owner);
                self.allocated += 1;
                self.cursor = (i + 1) % MSI_POOL_CAP;
                return Ok(Self::pool_to_vector(i));
            }
            i = (i + 1) % MSI_POOL_CAP;
            if i == self.cursor {
                return Err(PlanError::MsiPoolViolation {
                    vector: 0,
                    why: "轮转一周无空闲——allocated 与占用账失同步（内部缺陷）".to_string(),
                });
            }
        }
    }

    /// 释放（未占用即显性拒——重复释放是账面事故不是可忽略警告）。
    pub fn free(&mut self, vector: u8) -> Result<(), PlanError> {
        let idx = Self::vector_to_index(vector)?;
        match self.occupied[idx] {
            Some(_) => {
                self.occupied[idx] = None;
                self.allocated -= 1;
                Ok(())
            }
            None => Err(PlanError::MsiPoolViolation {
                vector,
                why: format!("向量 0x{:02X} 本就空闲——重复释放", vector),
            }),
        }
    }

    /// 全局向量 → 池索引（区外即拒）。
    fn vector_to_index(vector: u8) -> Result<usize, PlanError> {
        if vector < REGION_MSI_START || vector > REGION_MSI_END {
            return Err(PlanError::MsiPoolViolation {
                vector,
                why: format!("向量 0x{:02X} 不在 MSI 区 0x{:02X}~0x{:02X}", vector, REGION_MSI_START, REGION_MSI_END),
            });
        }
        Ok((vector as usize) - (REGION_MSI_START as usize))
    }

    /// 查占用者。
    pub fn owner_of(&self, vector: u8) -> Result<Option<&'static str>, PlanError> {
        let idx = Self::vector_to_index(vector)?;
        Ok(self.occupied[idx])
    }

    /// 占用计数（对账用：allocated == occupied 非空数恒等）。
    pub fn allocated_count(&self) -> usize {
        self.allocated
    }

    /// 占用账自洽（allocated == 实际非空槽位——池的守恒断言）。
    pub fn ledger_ok(&self) -> bool {
        self.allocated == self.occupied.iter().filter(|o| o.is_some()).count()
    }
}

impl Default for MsiPool {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 六、IDT 构建引擎（256 项全填充——零空洞）
// ---------------------------------------------------------------------------

/// 单向量绑定（handler 地址 + 来源标签——构建账的原子项）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct VectorBinding {
    /// 向量号。
    pub vector: u8,
    /// handler 地址（占位=PLACEHOLDER_ADDR）。
    pub handler_addr: u64,
    /// 绑定来源标签（"exc#DE"/"irq"/"msi:owner"/"ipi:reschedule"/"stub"）。
    pub tag: &'static str,
    /// 门描述符字段（type/dpl/ist——构建时按区规则+异常表逐项定）。
    pub gate: GateDesc,
}

/// 占位 handler 地址（确定性哨兵值——真 handler 由接线面回填，占位面
/// 保证 P=1 与格式合法，撞上后 spurious 账本可归因）。
pub const PLACEHOLDER_ADDR: u64 = 0xFFFF_8000_0000_0000;

/// IDT 构建器（256 项全填充状态机）。
#[derive(Clone, Debug)]
pub struct IdtBuilder {
    /// 绑定表（索引即向量号）。
    bindings: Vec<Option<VectorBinding>>,
    /// 填充计数。
    filled: usize,
}

impl IdtBuilder {
    /// 空构建器。
    pub fn new() -> Self {
        IdtBuilder {
            bindings: vec![None; VECTOR_TOTAL],
            filled: 0,
        }
    }

    /// 绑定一个向量（区规则闸：type 按、DPL 异常区按表、IST 异常区按表）。
    pub fn bind(&mut self, vector: u8, handler_addr: u64, tag: &'static str) -> Result<(), PlanError> {
        let region = region_of(vector)?;
        // 异常区：查注册表定 DPL/IST；其余区：区默认。
        let (dpl, ist) = if region.start == REGION_EXC_START {
            let e = EXCEPT_TABLE
                .iter()
                .find(|e| e.vector == vector)
                .ok_or(PlanError::ExceptTableIncomplete {
                    vector,
                    why: format!("异常区向量 0x{:02X} 不在注册表——SDM 表 6-1 闭集有洞", vector),
                })?;
            (e.dpl, e.ist)
        } else {
            (region.default_dpl, 0)
        };
        let gate = GateDesc {
            offset_lo: (handler_addr & 0xFFFF) as u16,
            selector: KERNEL_CS,
            ist,
            gate_type: region.gate_type,
            dpl,
            present: true,
            offset_hi: (handler_addr >> 16) & 0xFFFF_FFFF_FFFF,
        };
        gate.validate()?;
        let slot = &mut self.bindings[vector as usize];
        if slot.is_some() {
            return Err(PlanError::GateFieldIllegal {
                why: format!("向量 0x{:02X} 已绑定（{}）——重复绑定", vector, tag),
            });
        }
        *slot = Some(VectorBinding {
            vector,
            handler_addr,
            tag,
            gate,
        });
        self.filled += 1;
        Ok(())
    }

    /// 全量填充（异常区 32 项按表 + IRQ/MSI/IPI/spurious 全区占位——
    /// 256 项无一空洞：未绑定真 handler 的向量填占位门 P=1）。
    pub fn fill_all_placeholder(&mut self) -> Result<(), PlanError> {
        for v in 0u8..=255u8 {
            if self.bindings[v as usize].is_some() {
                continue;
            }
            self.bind(v, PLACEHOLDER_ADDR, PLACEHOLDER_HANDLER)?;
        }
        if self.filled != VECTOR_TOTAL {
            return Err(PlanError::IdtHole {
                missing: self
                    .bindings
                    .iter()
                    .enumerate()
                    .filter(|(_, b)| b.is_none())
                    .map(|(i, _)| i as u8)
                    .collect(),
            });
        }
        Ok(())
    }

    /// 编码全表（256×16 字节——lidt 的落盘形态）。
    pub fn encode_all(&self) -> Result<Vec<[u8; GATE_SIZE]>, PlanError> {
        if self.filled != VECTOR_TOTAL {
            return Err(PlanError::IdtHole {
                missing: self
                    .bindings
                    .iter()
                    .enumerate()
                    .filter(|(_, b)| b.is_none())
                    .map(|(i, _)| i as u8)
                    .collect(),
            });
        }
        let mut out: Vec<[u8; GATE_SIZE]> = Vec::with_capacity(VECTOR_TOTAL);
        for b in self.bindings.iter() {
            match b {
                Some(vb) => out.push(vb.gate.encode()),
                None => {
                    return Err(PlanError::IdtHole { missing: Vec::new() });
                }
            }
        }
        Ok(out)
    }

    /// 查绑定。
    pub fn binding(&self, vector: u8) -> Option<&VectorBinding> {
        self.bindings.get(vector as usize).and_then(|b| b.as_ref())
    }

    /// 填充计数。
    pub fn filled_count(&self) -> usize {
        self.filled
    }

    /// IST 分配账复核（#DF=IST2、NMI=IST1、#MC=IST3——SDM 硬要求逐项查）。
    pub fn verify_ist(&self) -> Result<(), PlanError> {
        let df = self.binding(0x08).ok_or(PlanError::IstAllocationBad {
            why: "#DF(0x08) 未绑定".to_string(),
        })?;
        if df.gate.ist != IST_DF {
            return Err(PlanError::IstAllocationBad {
                why: format!("#DF IST={} ≠ {}——双重故障必须走独立栈（SDM 硬要求）", df.gate.ist, IST_DF),
            });
        }
        let nmi = self.binding(0x02).ok_or(PlanError::IstAllocationBad {
            why: "NMI(0x02) 未绑定".to_string(),
        })?;
        if nmi.gate.ist != IST_NMI {
            return Err(PlanError::IstAllocationBad {
                why: format!("NMI IST={} ≠ {}——NMI 必须走独立栈防嵌套", nmi.gate.ist, IST_NMI),
            });
        }
        let mc = self.binding(0x12).ok_or(PlanError::IstAllocationBad {
            why: "#MC(0x12) 未绑定".to_string(),
        })?;
        if mc.gate.ist != IST_MC {
            return Err(PlanError::IstAllocationBad {
                why: format!("#MC IST={} ≠ {}——机器检查走独立栈", mc.gate.ist, IST_MC),
            });
        }
        // 其余异常区 IST 必须为 0（不占 IST 槽——IST 是稀缺资源 7 个）。
        for e in EXCEPT_TABLE.iter() {
            if e.ist == 0 {
                if let Some(b) = self.binding(e.vector) {
                    if b.gate.ist != 0 {
                        return Err(PlanError::IstAllocationBad {
                            why: format!(
                                "异常 0x{:02X}({}) 声明 IST=0 但门上 IST={}——注册表与门失同步",
                                e.vector, e.mnemonic, b.gate.ist
                            ),
                        });
                    }
                }
            }
        }
        Ok(())
    }
}

impl Default for IdtBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 七、lidt 描述符（生效三要素——limit/base/对齐）
// ---------------------------------------------------------------------------

/// lidt 操作数（10 字节：2 字节 limit + 8 字节 base——SDM Vol.3 §6.10）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LidtPtr {
    /// 表限长（字节数-1 = 4095）。
    pub limit: u16,
    /// 表基址（物理/线性地址）。
    pub base: u64,
}

impl LidtPtr {
    /// 由表基址构造（三要素校验：limit 恰 4095、base 对齐、base 非 0）。
    pub fn new(base: u64) -> Result<Self, PlanError> {
        if base == 0 {
            return Err(PlanError::LidtDescriptorBad {
                why: "base=0——IDT 无处安放（0 页是空指针哨兵）".to_string(),
            });
        }
        if base % IDT_BASE_ALIGN != 0 {
            return Err(PlanError::LidtDescriptorBad {
                why: format!("base 0x{:X} 未 {} 字节对齐——CPU 取门描述符按 8 字节界", base, IDT_BASE_ALIGN),
            });
        }
        let lp = LidtPtr {
            limit: IDT_LIMIT,
            base,
        };
        if lp.limit != IDT_LIMIT {
            return Err(PlanError::LidtDescriptorBad {
                why: format!("limit {} ≠ {}（256×16-1）", lp.limit, IDT_LIMIT),
            });
        }
        Ok(lp)
    }

    /// 编码 10 字节小端（内存布局：limit 在前）。
    pub fn encode(&self) -> [u8; 10] {
        let mut b = [0u8; 10];
        b[0..2].copy_from_slice(&self.limit.to_le_bytes());
        b[2..10].copy_from_slice(&self.base.to_le_bytes());
        b
    }

    /// 生效语义自查（判据「lidt 生效」的构造面：limit 覆盖全表 + base 对齐）。
    pub fn effective(&self) -> Result<(), PlanError> {
        let need = IDT_BYTES as u64;
        if (self.limit as u64) + 1 != need {
            return Err(PlanError::LidtDescriptorBad {
                why: format!("limit+1={} ≠ 表字节数 {}", self.limit as u64 + 1, need),
            });
        }
        if self.base % IDT_BASE_ALIGN != 0 {
            return Err(PlanError::LidtDescriptorBad {
                why: "base 未对齐——lidt 后取门按 8 字节界会撕裂".to_string(),
            });
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 八、五区断言引擎（J1 落地：≥20 条断言逐条结构化）
// ---------------------------------------------------------------------------

/// 单条断言结果（结构化——不查文案，查事实字段）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RegionAssertion {
    /// 断言名。
    pub name: String,
    /// 通过与否。
    pub passed: bool,
    /// 证据（数值/事实串）。
    pub evidence: String,
}

/// 断言引擎（≥20 条清单——J1 的逐条落地，全绿才收单）。
pub struct RegionAssertEngine;

impl RegionAssertEngine {
    /// 全量断言（20+ 条——每条独立事实核对，零文案裁决）。
    pub fn run_all(builder: &IdtBuilder, pool: &MsiPool) -> Vec<RegionAssertion> {
        let mut out: Vec<RegionAssertion> = Vec::new();
        let mut a = |name: &str, passed: bool, evidence: String| {
            out.push(RegionAssertion {
                name: name.to_string(),
                passed,
                evidence,
            })
        };

        // 断言 1：全表 256 项填充（零空洞）。
        a(
            "A01_256项全填充",
            builder.filled_count() == VECTOR_TOTAL,
            format!("filled={}/256", builder.filled_count()),
        );
        // 断言 2：五区覆盖全空间（零缝隙零重叠）。
        let cover = regions_cover_all();
        a(
            "A02_五区覆盖全空间",
            cover.is_ok(),
            match &cover {
                Ok(_) => "五区闭集覆盖 0x00~0xFF 零缝隙零重叠".to_string(),
                Err(e) => e.detail(),
            },
        );
        // 断言 3~7：五区边界值逐区核对（区定义表 vs 常量——两写法一致）。
        let bounds_expect: [(&str, u8, u8); 5] = [
            ("异常", REGION_EXC_START, REGION_EXC_END),
            ("IRQ", REGION_IRQ_START, REGION_IRQ_END),
            ("MSI", REGION_MSI_START, REGION_MSI_END),
            ("IPI", REGION_IPI_START, REGION_IPI_END),
            ("spurious", REGION_SPURIOUS_START, REGION_SPURIOUS_END),
        ];
        for (i, (nm, s, e)) in bounds_expect.iter().enumerate() {
            let r = &REGIONS[i];
            a(
                &format!("A{:02}_区{}_边界", i + 3, nm),
                r.name == *nm && r.start == *s && r.end == *e,
                format!("表:{} 0x{:02X}~0x{:02X} / 常量:{} 0x{:02X}~0x{:02X}", r.name, r.start, r.end, nm, s, e),
            );
        }
        // 断言 8：异常区 32 项注册表闭集（0~31 全登记）。
        let exc_complete = (0u8..=31).all(|v| EXCEPT_TABLE.iter().any(|e| e.vector == v));
        a(
            "A08_异常注册表32项闭集",
            exc_complete,
            format!("0..=31 全登记={}", exc_complete),
        );
        // 断言 9：#DF 走 IST2。
        let df_ok = builder
            .binding(0x08)
            .map(|b| b.gate.ist == IST_DF)
            .unwrap_or(false);
        a("A09_DF走IST2", df_ok, format!("#DF IST={:?}", builder.binding(0x08).map(|b| b.gate.ist)));
        // 断言 10：NMI 走 IST1。
        let nmi_ok = builder
            .binding(0x02)
            .map(|b| b.gate.ist == IST_NMI)
            .unwrap_or(false);
        a("A10_NMI走IST1", nmi_ok, format!("NMI IST={:?}", builder.binding(0x02).map(|b| b.gate.ist)));
        // 断言 11：#BP/#OF DPL=3（用户态可触发）。
        let dpl3_ok = [0x03u8, 0x04]
            .iter()
            .all(|v| builder.binding(*v).map(|b| b.gate.dpl == 3).unwrap_or(false));
        a(
            "A11_BP与OF_DPL3",
            dpl3_ok,
            format!("#BP DPL={:?} #OF DPL={:?}", builder.binding(0x03).map(|b| b.gate.dpl), builder.binding(0x04).map(|b| b.gate.dpl)),
        );
        // 断言 12：异常区全部 P=1（无空洞门）。
        let exc_p1 = (0u8..=REGION_EXC_END)
            .all(|v| builder.binding(v).map(|b| b.gate.present).unwrap_or(false));
        a("A12_异常区全P1", exc_p1, format!("0x00~0x1F present 全真={}", exc_p1));
        // 断言 13：全表 selector 恒 = KERNEL_CS。
        let sel_ok = (0u8..=255u8).all(|v| {
            builder
                .binding(v)
                .map(|b| b.gate.selector == KERNEL_CS)
                .unwrap_or(false)
        });
        a("A13_全表selector内核段", sel_ok, format!("256 项 selector==0x08={}", sel_ok));
        // 断言 14：全表门类型合法（0xE/0xF 闭集）。
        let type_ok = (0u8..=255u8).all(|v| {
            builder
                .binding(v)
                .map(|b| b.gate.gate_type == GATE_TYPE_INTERRUPT || b.gate.gate_type == GATE_TYPE_TRAP)
                .unwrap_or(false)
        });
        a("A14_全表门类型闭集", type_ok, format!("256 项 type∈{{0xE,0xF}}={}", type_ok));
        // 断言 15：MSI 池账自洽（allocated==非空槽）。
        a(
            "A15_MSI池账自洽",
            pool.ledger_ok(),
            format!("allocated={} 非空={}", pool.allocated_count(), pool.occupied_count()),
        );
        // 断言 16：MSI 池分配一轮后向量落区内。
        let mut probe = MsiPool::new();
        let v = probe.alloc("probe-dev").ok();
        let in_region = v
            .map(|vv| vv >= REGION_MSI_START && vv <= REGION_MSI_END)
            .unwrap_or(false);
        a("A16_MSI分配落区内", in_region, format!("首分配={:?}", v.map(|vv| format!("0x{:02X}", vv))));
        // 断言 17：MSI 双占拦截。
        let dup = if let Some(vv) = v {
            // 释放后再释放应拒——重复释放闸。
            let _ = probe.free(vv);
            probe.free(vv).is_err()
        } else {
            false
        };
        a("A17_MSI重复释放拦截", dup, format!("释放后再释放被拒={}", dup));
        // 断言 18：IPI 区 14 项语义闭集（每向量有语义）。
        let ipi_complete = (REGION_IPI_START..=REGION_IPI_END)
            .all(|v| IPI_TABLE.iter().any(|e| e.vector == v));
        a("A18_IPI语义14项闭集", ipi_complete, format!("0xF0~0xFD 全绑定={}", ipi_complete));
        // 断言 19：IPI 区外查语义被拒。
        let ipi_oob = ipi_semantic(0x20).is_err();
        a("A19_IPI区外查拒", ipi_oob, format!("IRQ 区向量查 IPI 语义被拒={}", ipi_oob));
        // 断言 20：spurious 区含 LAPIC 默认向量 0xFF。
        let spur_ok = LAPIC_SPURIOUS_VECTOR == REGION_SPURIOUS_END
            && builder.binding(LAPIC_SPURIOUS_VECTOR).is_some();
        a(
            "A20_spurious默认向量",
            spur_ok,
            format!("LAPIC spurious=0x{:02X} 在区且已绑定", LAPIC_SPURIOUS_VECTOR),
        );
        // 断言 21：位级往返——抽 4 个代表向量 encode→decode 逐位一致。
        let rt_ok = [0x00u8, 0x08, 0x70, 0xFF]
            .iter()
            .all(|v| builder.binding(*v).map(|b| b.gate.roundtrip_ok()).unwrap_or(false));
        a("A21_位级编码往返", rt_ok, format!("代表向量 0x00/0x08/0x70/0xFF 往返一致={}", rt_ok));
        // 断言 22：lidt 三要素（limit 4095/对齐/非零）。
        let lidt = LidtPtr::new(0xFFFF_8000_0000_0000);
        a(
            "A22_lidt三要素",
            lidt.as_ref().map(|l| l.effective().is_ok()).unwrap_or(false),
            match &lidt {
                Ok(l) => format!("limit={} base=0x{:X} 对齐 OK", l.limit, l.base),
                Err(e) => e.detail(),
            },
        );
        // 断言 23：IST 复核全绿（#DF/#NMI/#MC 三硬要求+其余异常 IST=0）。
        let ist_ok = builder.verify_ist().is_ok();
        a("A23_IST分配账复核", ist_ok, match builder.verify_ist() {
            Ok(_) => "NMI=IST1/#DF=IST2/#MC=IST3/其余 IST=0 全绿".to_string(),
            Err(e) => e.detail(),
        });
        // 断言 24：占位面覆盖（未绑定真 handler 的向量全是 stub 标签）。
        let stub_ok = (0u8..=255u8).all(|v| {
            builder
                .binding(v)
                .map(|b| !b.tag.is_empty())
                .unwrap_or(false)
        });
        a("A24_全表绑定标签非空", stub_ok, format!("256 项 tag 非空={}", stub_ok));

        out
    }

    /// 断言汇总（红项清单——全绿返回 Ok，有红返回首红诊断）。
    pub fn verdict(asserts: &[RegionAssertion]) -> Result<(), PlanError> {
        if asserts.len() < 20 {
            return Err(PlanError::RegionAssertionRed {
                assert_name: "断言清单规模".to_string(),
                why: format!("断言 {} 条 < 20 条——J1 要求 ≥20", asserts.len()),
            });
        }
        for a in asserts.iter() {
            if !a.passed {
                return Err(PlanError::RegionAssertionRed {
                    assert_name: a.name.clone(),
                    why: a.evidence.clone(),
                });
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 九、五区边界值打印（J1 尾半——逐区可打印）
// ---------------------------------------------------------------------------

/// 五区边界打印（每区一行：名/起止/项数/门类型/动态性——排障即查）。
pub fn describe_regions() -> String {
    let mut s = String::from("向量空间五区规划（0x00~0xFF 256 向量）：\n");
    for r in REGIONS.iter() {
        s.push_str(&format!(
            "  {:<8} 0x{:02X}~0x{:02X} 共 {} 项 门类型 0x{:X} DPL={} {}\n",
            r.name,
            r.start,
            r.end,
            (r.end as usize) - (r.start as usize) + 1,
            r.gate_type,
            r.default_dpl,
            if r.dynamic { "动态分配池" } else { "静态分区" },
        ));
    }
    s
}

/// 单向量归属打印（某向量属哪区/绑了谁——排障点查）。
pub fn describe_vector(vector: u8, builder: &IdtBuilder) -> String {
    let region = match region_of(vector) {
        Ok(r) => r.name,
        Err(_) => "无主",
    };
    match builder.binding(vector) {
        Some(b) => format!(
            "向量 0x{:02X}：区={} 绑定={} handler=0x{:016X} IST={} DPL={} P={}",
            vector, region, b.tag, b.handler_addr, b.gate.ist, b.gate.dpl, b.gate.present
        ),
        None => format!("向量 0x{:02X}：区={} 未绑定", vector, region),
    }
}

// ---------------------------------------------------------------------------
// 十、错误与快照
// ---------------------------------------------------------------------------

/// 编排失败（全显性——每个变体带诊断与建议）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlanError {
    /// 向量越区。
    VectorOutOfRegion { vector: u8, why: String },
    /// 门字段非法。
    GateFieldIllegal { why: String },
    /// IDT 空洞。
    IdtHole { missing: Vec<u8> },
    /// lidt 描述符非法。
    LidtDescriptorBad { why: String },
    /// IST 分配违规。
    IstAllocationBad { why: String },
    /// 异常注册表缺项。
    ExceptTableIncomplete { vector: u8, why: String },
    /// MSI 池违规。
    MsiPoolViolation { vector: u8, why: String },
    /// 五区断言红。
    RegionAssertionRed { assert_name: String, why: String },
    /// IPI 语义缺失。
    IpiSemanticsMissing { vector: u8, why: String },
    /// spurious 配置非法。
    SpuriousCfgBad { why: String },
}

impl PlanError {
    /// 人话诊断（含建议）。
    pub fn detail(&self) -> String {
        match self {
            PlanError::VectorOutOfRegion { vector, why } => format!(
                "[0x3601] 向量 0x{:02X} 越区：{}。建议：改五区规划表或改用区内向量",
                vector, why
            ),
            PlanError::GateFieldIllegal { why } => format!("[0x3602] 门描述符字段非法：{}。建议：按 SDM §6.14.1 位域约束修正", why),
            PlanError::IdtHole { missing } => format!(
                "[0x3603] IDT 有空洞（{} 项未填：{:?}）。建议：fill_all_placeholder 全量填充——撞空洞即 triple fault",
                missing.len(), missing
            ),
            PlanError::LidtDescriptorBad { why } => format!("[0x3604] lidt 描述符非法：{}。建议：limit=4095、base 8 字节对齐、非零", why),
            PlanError::IstAllocationBad { why } => format!("[0x3605] IST 分配违规：{}。建议：NMI=IST1/#DF=IST2/#MC=IST3，其余异常 IST=0", why),
            PlanError::ExceptTableIncomplete { vector, why } => format!("[0x3606] 异常注册表缺项 0x{:02X}：{}。建议：SDM Table 6-1 32 行一行不缺", vector, why),
            PlanError::MsiPoolViolation { vector, why } => format!("[0x3607] MSI 池违规（向量 0x{:02X}）：{}。建议：走池分配不走散挑，满池先释放", vector, why),
            PlanError::RegionAssertionRed { assert_name, why } => format!("[0x3608] 五区断言红「{}」：{}。建议：修规划表到断言过——断言是 J1 收单线", assert_name, why),
            PlanError::IpiSemanticsMissing { vector, why } => format!("[0x3609] IPI 语义缺失（0x{:02X}）：{}。建议：IPI_TABLE 一号一义补条目", vector, why),
            PlanError::SpuriousCfgBad { why } => format!("[0x360A] spurious 配置非法：{}。建议：LAPIC spurious 向量取 0xFE~0xFF 闭集内", why),
        }
    }
}

impl MsiPool {
    /// 非空槽计数（断言 A15 对账面）。
    fn occupied_count(&self) -> usize {
        self.occupied.iter().filter(|o| o.is_some()).count()
    }
}

/// IDT 规划快照（FNV 三指纹——表/池/lidt 任一漂移即变）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdtSnap {
    /// 全表字节流指纹。
    pub table_fp: u32,
    /// 池占用指纹。
    pub pool_fp: u32,
    /// 填充数。
    pub filled: usize,
}

impl IdtSnap {
    /// 取快照。
    pub fn capture(builder: &IdtBuilder, pool: &MsiPool) -> Self {
        let mut tb: Vec<u8> = Vec::with_capacity(IDT_BYTES);
        if let Ok(all) = builder.encode_all() {
            for row in all {
                tb.extend_from_slice(&row);
            }
        }
        let mut pb: Vec<u8> = Vec::new();
        for i in 0..MSI_POOL_CAP {
            pb.push(match pool.occupied[i] {
                Some(_) => 1,
                None => 0,
            });
        }
        IdtSnap {
            table_fp: fnv32(&tb, FNV_OFFSET),
            pool_fp: fnv32(&pb, FNV_OFFSET),
            filled: builder.filled_count(),
        }
    }

    /// 往返一致。
    pub fn roundtrip_ok(&self, other: &IdtSnap) -> bool {
        self == other
    }
}

/// FNV-1a 32 位（家族统一口径）。
pub fn fnv32(data: &[u8], seed: u32) -> u32 {
    let mut h = seed;
    for b in data.iter() {
        h ^= *b as u32;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

/// FNV 素数与偏移。
pub const FNV_PRIME: u32 = 0x0100_0193;
pub const FNV_OFFSET: u32 = 0x811C_9DC5;

// ---------------------------------------------------------------------------
// 十一、样板构建入口（全链自证——J1 的完整走线）
// ---------------------------------------------------------------------------

/// 默认 IDT 全链构建（异常表绑定 + 全量占位填充 + IST 复核）。
pub fn build_default_idt() -> Result<IdtBuilder, PlanError> {
    let mut b = IdtBuilder::new();
    // 异常区 32 项：以异常注册表为准逐项绑定（占位 handler——真 handler
    // 由内核接线面回填，地址面本单保证 P=1 与格式合法）。
    for e in EXCEPT_TABLE.iter() {
        b.bind(e.vector, PLACEHOLDER_ADDR, "exc-table")?;
    }
    // 全量填充剩余 224 项占位（零空洞纪律）。
    b.fill_all_placeholder()?;
    // IST 分配账复核。
    b.verify_ist()?;
    Ok(b)
}

/// 样板 MSI 池（分配 2 个演示占用——账面非平凡）。
pub fn sample_msi_pool() -> Result<MsiPool, PlanError> {
    let mut p = MsiPool::new();
    let v1 = p.alloc("nvme-queue0")?;
    let v2 = p.alloc("gpu-msi0")?;
    if v1 < REGION_MSI_START || v1 > REGION_MSI_END || v2 < REGION_MSI_START || v2 > REGION_MSI_END {
        return Err(PlanError::MsiPoolViolation {
            vector: v1,
            why: "样板分配落区外——池向量映射缺陷".to_string(),
        });
    }
    Ok(p)
}

/// 全链自证（J1 走线：构建→断言≥20→lidt→快照往返——四步全绿）。
pub fn selfcheck_full() -> Result<(IdtSnap, String), PlanError> {
    // 1. 构建。
    let builder = build_default_idt()?;
    let pool = sample_msi_pool()?;
    // 2. 断言 ≥20 条全绿。
    let asserts = RegionAssertEngine::run_all(&builder, &pool);
    RegionAssertEngine::verdict(&asserts)?;
    // 3. lidt 描述符（表自省地址哨兵——真机由接线面传入实址）。
    let lidt = LidtPtr::new(0xFFFF_8000_0000_0000)?;
    lidt.effective()?;
    // 4. 快照往返。
    let s1 = IdtSnap::capture(&builder, &pool);
    let s2 = IdtSnap::capture(&builder, &pool);
    if !s1.roundtrip_ok(&s2) {
        return Err(PlanError::RegionAssertionRed {
            assert_name: "快照往返".to_string(),
            why: "同产物两次 capture 指纹不一致——确定性破约".to_string(),
        });
    }
    Ok((s1, describe_regions()))
}

/// 断言条数（J1「≥20 条」的规模自证——常量在册）。
pub const ASSERTION_COUNT: usize = 24;
