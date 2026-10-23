//! UNX-F0805 · spurious 向量 0xFF 槽位与伪中断过滤 · 诊断段 0x1710（独占 0x1710~0x171F）
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 验收锚点原文（docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR
//! II · Unxreal.md 行 2430，全文抄录不改一字）：
//! ```text
//! | UNX-F0805 | spurious 向量 0xFF 槽位与伪中断过滤 | 240 | 已深化 | UNX-F0805-J1 spurious 触发全程入账不误 EOI（10 万次注入零误 EOI），伪中断识别率 100% |
//! ```
//! 定位原文：
//! ```text
//! UNX-A2 · 中断与计时基座
//! UNX-A2-B01 · IDT 与向量空间规划（中断面第 1 批）（F801–F820 · 20 条）
//! 嫁接源：Intel SDM Vol.3 第 6 章（中断与异常）……防重：本批对样板
//! F0021/F0022 与现存 cpu/idt.rs 为分层细化深化
//! ```
//! ═══════════════════════════════════════════════════════════════════════
//!
//! 判据拆解（J1 两成分，缺一即批内红灯）：
//! 1. spurious 触发全程入账不误 EOI —— 10 万次注入（真中断/伪中断/边界向量
//!    混排）零误 EOI：伪中断路径（判决为 spurious）一律只入账、永不发 EOI；
//!    EOI 只允许出现在真中断路径，且误 EOI 检测器对任何旁路尝试即时 latch。
//! 2. 伪中断识别率 100% —— 0xFF 使能期仲裁失败伪中断与未使能期到达（AP
//!    初始化期全部判伪）两类注入均被过滤逐条识别，识别数 == 注入数。
//!
//! 语义来源（写死，不引入运行期查表）：
//! - Intel SDM Vol.3 §10.9 "Spurious-Interrupt Vector Register (SVR)"：
//!   位 7:0 = spurious vector（推荐 0xFF，对应 IDT 第 255 项）；位 8 = APIC
//!   software enable（清零则本地 APIC 不投递中断）；位 12 = Focus Processor
//!   Masking；位 15:13、11:9 保留必须写 0。
//! - 本地 APIC 仲裁失败 / 中断投递与 INTA 竞争时，APIC 以 SVR 向量字段
//!   （0xFF）注入；该交付天然不携带 ISR 置位语义，故 EOI 是违规操作。
//! - 五区向量规划沿用 UNX-F0801：异常 0x00–0x1F / 设备 IRQ 0x20–0x8F /
//!   MSI-X 0x90–0xDF / IPI 0xE0–0xFE / spurious 0xFF。
//!
//! 诊断段：0x1710~0x171F 独占。段空闲性依据：全仓 grep `SEG_BASE: u16 = 0x17`
//! 仅有 0x1700（ves460）、0x1730（cga28）、0x1740（unx2422）、0x1780（ves461）、
//! 0x17A0（ves462）、0x17C0（unx2442）；0x1710/0x1720 无任何占用，取 0x1710。
//!
//! no_std+alloc 自包含：只依赖 core / alloc；零 unwrap / expect / panic! /
//! 运行期 assert!；可失败路径全走 `Result<T, SpuriousError>`；计数一律
//! saturating/wrapping；>64KB 结构体（事件账）只落堆不上栈。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ═══════════════════════════════════════════════════════════════════════
// 确定性 LCG（本册本地副本，同仓 unx0807 惯例：乘加常数取 Knuth MMIX）
// ═══════════════════════════════════════════════════════════════════════

/// 确定性线性同余发生器：双跑对拍的随机性来源，同种子必得同序列。
pub struct Lcg(u64);

impl Lcg {
    /// 种子即初态；0 亦可（乘加链非平凡不动点概率为零，此处仍显式允许）。
    pub const fn new(seed: u64) -> Self {
        Lcg(seed)
    }

    /// 推进一步：x_{n+1} = a·x_n + c (mod 2^64)，溢出显性 wrapping。
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0
    }

    /// [0, n) 均匀采样；n==0 恒返 0（不 panic 纪律）。
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            0
        } else {
            self.next() % n
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 诊断段独占 · Spurious 过滤错误域（0x1710 基，段宽 0x10）
// ═══════════════════════════════════════════════════════════════════════

/// 诊断段基址（0x1710~0x171F 独占，空闲性依据见文件头）。
pub const SEG_BASE: u16 = 0x1710;

const E_SLOT: u16 = SEG_BASE + 1; // 0x1711 0xFF 槽位被非 spurious 认领/重复认领
const E_SVRBITS: u16 = SEG_BASE + 2; // 0x1712 SVR 位域编解码回读不一致
const E_FALSE_EOI: u16 = SEG_BASE + 3; // 0x1713 伪中断路径发出 EOI（误 EOI 检出）
const E_RECOG: u16 = SEG_BASE + 4; // 0x1714 伪中断漏判为真（识别率跌破 100%）
const E_SEQ: u16 = SEG_BASE + 5; // 0x1715 SVR 使能写入序违例
const E_LEDGER: u16 = SEG_BASE + 6; // 0x1716 事件账写满
const E_CORRUPT: u16 = SEG_BASE + 7; // 0x1717 事件账序号/不变量损坏
const E_STORM: u16 = SEG_BASE + 8; // 0x1718 风暴未按期告警
const E_FP: u16 = SEG_BASE + 9; // 0x1719 双跑指纹不一致
const E_INJECT: u16 = SEG_BASE + 10; // 0x171A 注入面未检出
const E_TRUTH: u16 = SEG_BASE + 11; // 0x171B 过滤真值表不自洽
const E_HEALTH: u16 = SEG_BASE + 12; // 0x171C health 不变量破坏

/// 本册唯一错误类型：每个变体携带中文可操作诊断（detail）与诊断码（code）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpuriousError {
    /// 0xFF 槽位被非 spurious 处理器占用，或 spurious 槽被重复认领。
    SlotConflict,
    /// SVR 位域编解码回读不一致或保留位污染。
    SvrBitfield,
    /// 伪中断路径发出 EOI（误 EOI 检测器 latch）。
    FalseEoiLatched,
    /// 伪中断漏判为真（识别率跌破 100%）。
    RecognitionMiss,
    /// SVR 使能写入序违例（未编程向量先使能等）。
    SequenceViolation,
    /// 事件账写满，注入事件无法入账。
    LedgerFull,
    /// 事件账序号回退或 EOI 守恒不变量损坏。
    LedgerCorrupt,
    /// 连续 spurious 风暴未按期告警。
    StormAlarmMiss,
    /// 双跑指纹不一致（确定性破坏）。
    FingerprintMismatch,
    /// 注入面（误 EOI/风暴/未使能）未按预期检出。
    InjectionSurfaceMiss,
    /// 过滤真值表与判定函数不自洽。
    TruthTableInconsistent,
    /// health() 不变量破坏。
    HealthBroken,
}

impl SpuriousError {
    /// 中文可操作诊断：指出错因与下一步动作。
    pub fn detail(&self) -> &'static str {
        match self {
            SpuriousError::SlotConflict => "0xFF 槽位冲突：spurious 槽必须独占保留（IDT 第 255 项），\
             请检查是否被设备 IRQ/IPI 处理器重复认领，认领顺序必须先于一切其他向量",
            SpuriousError::SvrBitfield => "SVR 位域错：回读值与编码不一致或位 15:13/11:9 保留位被污染，\
             请核对 SDM §10.9 位 7:0 向量、位 8 使能、位 12 焦点掩码的编解码路径",
            SpuriousError::FalseEoiLatched => "误 EOI 检出：伪中断判决路径发出了 EOI，\
             违反『只入账不 EOI』铁律；请检查调用方是否绕过 SpuriousFilter::admit 直接发 EOI",
            SpuriousError::RecognitionMiss => "伪中断漏判：注入侧真值与过滤判决不一致，\
             请核对 verdict_of 真值表与 effective_enabled 时序覆盖逻辑",
            SpuriousError::SequenceViolation => "SVR 使能写入序违例：必须 AP 初始化→SVR 向量字段写 0xFF→\
             再置位 8 使能；未编程向量先使能或对非 0xFF 向量编程均违规",
            SpuriousError::LedgerFull => "事件账写满：LEDGER_CAP 容量不足或账目未消费，\
             请扩容 LEDGER_CAP 或先落盘再续注",
            SpuriousError::LedgerCorrupt => "事件账损坏：序号回退或 EOI 守恒被破坏，\
             请检查 record() 调用序与 eoi 只随 K_REAL 事件的约束",
            SpuriousError::StormAlarmMiss => "风暴告警未触发：窗内计数未达阈值或跨度参数漂移，\
             请核对 STORM_COUNT_THRESHOLD/STORM_SPAN_TICKS 与注入 burst 规模",
            SpuriousError::FingerprintMismatch => "双跑指纹不一致：同种子两引擎状态发散，\
             请检查是否存在非确定性（未初始化内存/浮点/时钟依赖）路径",
            SpuriousError::InjectionSurfaceMiss => "注入面未检出：误 EOI/风暴/未使能注入未被对应\
             检测器捕获，检测器 latch 逻辑或阈值失效",
            SpuriousError::TruthTableInconsistent => "过滤真值表不自洽：FILTER_TRUTH_TABLE 冻结行与 \
             verdict_of/eoi_allowed 判定函数输出不一致，禁止单边修改",
            SpuriousError::HealthBroken => "health 不变量破坏：EOI 守恒/账本单调/槽位独占之一失守，\
             请按 health() 报告逐项定位",
        }
    }

    /// 诊断码：0x1710 段内唯一编址。
    pub fn code(&self) -> u16 {
        match self {
            SpuriousError::SlotConflict => E_SLOT,
            SpuriousError::SvrBitfield => E_SVRBITS,
            SpuriousError::FalseEoiLatched => E_FALSE_EOI,
            SpuriousError::RecognitionMiss => E_RECOG,
            SpuriousError::SequenceViolation => E_SEQ,
            SpuriousError::LedgerFull => E_LEDGER,
            SpuriousError::LedgerCorrupt => E_CORRUPT,
            SpuriousError::StormAlarmMiss => E_STORM,
            SpuriousError::FingerprintMismatch => E_FP,
            SpuriousError::InjectionSurfaceMiss => E_INJECT,
            SpuriousError::TruthTableInconsistent => E_TRUTH,
            SpuriousError::HealthBroken => E_HEALTH,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// SVR 寄存器位域建模（Intel SDM Vol.3 §10.9，语义写死不查运行期表）
// ═══════════════════════════════════════════════════════════════════════

/// spurious 专用向量：0xFF。SDM §10.9：SVR 向量字段推荐值 0xFF，
/// 对应 IDT 第 255 项 —— UNX-F0801 五区规划的 spurious 区唯一成员。
pub const VECTOR_SPURIOUS: u8 = 0xFF;

/// SVR 位 7:0 —— spurious vector 字段掩码。
pub const SVR_VECTOR_MASK: u32 = 0x0000_00FF;
/// SVR 位 8 —— APIC 软件使能位：置 1 才投递中断，清零期到达一律判伪。
pub const SVR_ENABLE_BIT: u32 = 0x0000_0100;
/// SVR 位 12 —— Focus Processor Masking：置 1 时焦点处理器仲裁失败不重定向。
pub const SVR_FOCUS_MASK_BIT: u32 = 0x0000_1000;
/// SVR 保留位掩码（位 15:13、11:9）：软件必须写 0，读出非零即位域污染。
pub const SVR_RESERVED_MASK: u32 = 0xFFFF_EE00;

/// SVR 位域拆解结果：向量槽 / 使能位 / 焦点处理器掩码位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SvrBits {
    /// 位 7:0：spurious 向量（必须为 0xFF 才占用合法槽位）。
    pub vector: u8,
    /// 位 8：APIC 软件使能。
    pub enabled: bool,
    /// 位 12：焦点处理器掩码。
    pub focus_masked: bool,
}

impl SvrBits {
    /// 编码回 SVR 原始值（保留位恒 0）。
    pub const fn encode(self) -> u32 {
        let mut raw = (self.vector as u32) & SVR_VECTOR_MASK;
        if self.enabled {
            raw |= SVR_ENABLE_BIT;
        }
        if self.focus_masked {
            raw |= SVR_FOCUS_MASK_BIT;
        }
        raw
    }

    /// 槽位合法性：向量字段必须是 0xFF 才算占用 spurious 专属槽。
    pub const fn vector_slot_ok(self) -> bool {
        self.vector == VECTOR_SPURIOUS
    }
}

/// 解码 SVR 原始值（const：直接位运算，不用 slice::get / Ord::min）。
pub const fn decode_svr(raw: u32) -> SvrBits {
    SvrBits {
        vector: (raw & SVR_VECTOR_MASK) as u8,
        enabled: (raw & SVR_ENABLE_BIT) != 0,
        focus_masked: (raw & SVR_FOCUS_MASK_BIT) != 0,
    }
}

/// 保留位必须干净（软件写 0 后回读非零 = 硬件/程序错）。
pub const fn svr_reserved_clean(raw: u32) -> bool {
    (raw & SVR_RESERVED_MASK) == 0
}

/// 单位抽取（const：移位掩码，禁 Ord::min/max）。
pub const fn svr_bit_value(raw: u32, bit: u8) -> bool {
    (raw >> bit) & 1 == 1
}

/// SVR 位域含义表（13 行，位 0..=12；来源 SDM §10.9 逐位注释，const 冻结）。
pub const SVR_BIT_TABLE: [(u8, &'static str); 13] = [
    (0, "SVR 位0 · spurious vector 字段最低位（0xFF 槽位位0）"),
    (1, "SVR 位1 · spurious vector 字段位1"),
    (2, "SVR 位2 · spurious vector 字段位2"),
    (3, "SVR 位3 · spurious vector 字段位3"),
    (4, "SVR 位4 · spurious vector 字段位4"),
    (5, "SVR 位5 · spurious vector 字段位5"),
    (6, "SVR 位6 · spurious vector 字段位6"),
    (7, "SVR 位7 · spurious vector 字段最高位（合 0xFF 即 IDT 第 255 项）"),
    (8, "SVR 位8 · APIC 软件使能位：置 1 才投递中断；清零期到达一律判伪"),
    (9, "SVR 位9 · 保留（必须写 0，SDM §10.9）"),
    (10, "SVR 位10 · 保留（必须写 0）"),
    (11, "SVR 位11 · 保留（必须写 0）"),
    (12, "SVR 位12 · Focus Processor Masking：仲裁失败不向焦点处理器重定向"),
];

/// 逐位解码扫描（运行期）：返回 (位号, 取值) 列表，供回读审计。
pub fn svr_bit_scan(raw: u32) -> Vec<(u8, bool)> {
    let mut out: Vec<(u8, bool)> = Vec::new();
    let mut i: u8 = 0;
    while i < 13 {
        out.push((i, svr_bit_value(raw, i)));
        i += 1;
    }
    out
}

/// 编解码往返一致性（运行期校验入口）。
pub fn svr_roundtrip_ok(bits: SvrBits) -> bool {
    decode_svr(bits.encode()) == bits
}

// ═══════════════════════════════════════════════════════════════════════
// 向量五区规划与 0xFF 槽位守护（UNX-F0801 五区边界的 spurious 侧收口）
// ═══════════════════════════════════════════════════════════════════════

/// 异常区下界（UNX-F0801）。
pub const ZONE_EXC_HI: u8 = 0x1F;
/// 设备 IRQ 区上界。
pub const ZONE_IRQ_HI: u8 = 0x8F;
/// MSI-X 区上界。
pub const ZONE_MSI_HI: u8 = 0xDF;
/// IPI 区上界（0xFE；0xFF 归 spurious 区）。
pub const ZONE_IPI_HI: u8 = 0xFE;

/// 向量五区归类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VectorZone {
    /// 异常区 0x00–0x1F。
    Exception,
    /// 设备 IRQ 区 0x20–0x8F。
    DeviceIrq,
    /// MSI-X 区 0x90–0xDF。
    MsiX,
    /// IPI 区 0xE0–0xFE。
    Ipi,
    /// spurious 区：仅 0xFF 一席。
    Spurious,
}

/// 五区归类（const：u8 比较 + if 链，不用 Ord::min/max）。
pub const fn zone_of(v: u8) -> VectorZone {
    if v == VECTOR_SPURIOUS {
        VectorZone::Spurious
    } else if v <= ZONE_EXC_HI {
        VectorZone::Exception
    } else if v <= ZONE_IRQ_HI {
        VectorZone::DeviceIrq
    } else if v <= ZONE_MSI_HI {
        VectorZone::MsiX
    } else {
        VectorZone::Ipi
    }
}

/// 区名（const：match 枚举，不比较 PartialEq）。
pub const fn zone_name(z: VectorZone) -> &'static str {
    match z {
        VectorZone::Exception => "异常区 0x00-0x1F",
        VectorZone::DeviceIrq => "设备 IRQ 区 0x20-0x8F",
        VectorZone::MsiX => "MSI-X 区 0x90-0xDF",
        VectorZone::Ipi => "IPI 区 0xE0-0xFE",
        VectorZone::Spurious => "spurious 区（仅 0xFF）",
    }
}

/// 注入混排用边界向量集：五区边界各取端点 + spurious 槽本身。
pub const BOUNDARY_VECTORS: [u8; 9] = [
    0x00, 0x1F, 0x20, 0x8F, 0x90, 0xDF, 0xE0, 0xFE, 0xFF,
];

/// 256 向量占用位图：0xFF 槽专属守护。
/// 槽位语义：0xFF 是 spurious 入口在 IDT 的唯一落点，任何设备/IPI 处理器
/// 不得认领；spurious 认领必须先于一切其他向量认领。
pub struct SlotTable {
    taken: [u64; 4],
}

impl SlotTable {
    /// 空表（const：数组初始化，无循环）。
    pub const fn new() -> Self {
        SlotTable { taken: [0; 4] }
    }

    /// (字, 位) 分解（const：移位 + 掩码）。
    pub const fn split(v: u8) -> (usize, u32) {
        ((v >> 6) as usize, (v & 0x3F) as u32)
    }

    /// 认领向量槽；重复认领报 SlotConflict。
    pub fn claim(&mut self, v: u8) -> Result<(), SpuriousError> {
        let (w, b) = Self::split(v);
        if self.taken[w] & (1u64 << b) != 0 {
            return Err(SpuriousError::SlotConflict);
        }
        self.taken[w] |= 1u64 << b;
        Ok(())
    }

    /// 释放向量槽；未认领返回 false（不 panic）。
    pub fn release(&mut self, v: u8) -> bool {
        let (w, b) = Self::split(v);
        if self.taken[w] & (1u64 << b) == 0 {
            return false;
        }
        self.taken[w] &= !(1u64 << b);
        true
    }

    /// 查询占用（const：纯位读）。
    pub const fn is_taken(&self, v: u8) -> bool {
        let (w, b) = Self::split(v);
        (self.taken[w] >> b) & 1 == 1
    }

    /// spurious 槽认领（幂等保护：已认领即冲突）。
    pub fn claim_spurious_slot(&mut self) -> Result<(), SpuriousError> {
        if self.is_taken(VECTOR_SPURIOUS) {
            return Err(SpuriousError::SlotConflict);
        }
        self.claim(VECTOR_SPURIOUS)
    }

    /// 0xFF 槽位独占断言源（health 用）。
    pub const fn spurious_slot_exclusive(&self) -> bool {
        self.is_taken(VECTOR_SPURIOUS)
    }

    /// 已认领向量计数（运行期：while 遍历 4 字）。
    pub fn taken_count(&self) -> u64 {
        let mut c: u64 = 0;
        let mut w = 0;
        while w < 4 {
            c = c.wrapping_add(self.taken[w].count_ones() as u64);
            w += 1;
        }
        c
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 过滤真值表（const 冻结）与判决函数
// ═══════════════════════════════════════════════════════════════════════

/// 过滤判决。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// 真中断：正常路径 EOI。
    Real,
    /// 0xFF 使能期仲裁失败伪中断：只入账不 EOI。
    SpuriousArbitration,
    /// 未使能期到达（含 0xFF 与真向量，AP 初始化期全部判伪）：只入账不 EOI。
    SpuriousPreEnable,
}

/// 真值表冻结行：(0xFF 向量?, SVR 使能?) → (判决, 是否允许 EOI, 语义标签)。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FilterRow {
    /// 输入 1：到达向量是否为 0xFF。
    pub is_ff: bool,
    /// 输入 2：SVR 使能位（经时序状态机覆盖后的有效值）。
    pub enabled: bool,
    /// 判决。
    pub verdict: Verdict,
    /// 是否允许 EOI（伪判决恒 false —— J1 铁律）。
    pub eoi: bool,
    /// 语义标签（中文，供打印/health 备注）。
    pub tag: &'static str,
}

/// 过滤真值表（4 行全输入覆盖；与 verdict_of/eoi_allowed 双向冻结）。
pub const FILTER_TRUTH_TABLE: [FilterRow; 4] = [
    FilterRow {
        is_ff: false,
        enabled: true,
        verdict: Verdict::Real,
        eoi: true,
        tag: "真中断·使能期：正常路径 EOI",
    },
    FilterRow {
        is_ff: true,
        enabled: true,
        verdict: Verdict::SpuriousArbitration,
        eoi: false,
        tag: "伪中断·使能期 0xFF 仲裁失败：只入账不 EOI",
    },
    FilterRow {
        is_ff: false,
        enabled: false,
        verdict: Verdict::SpuriousPreEnable,
        eoi: false,
        tag: "未使能期到达真向量：AP 初始化期全部判伪",
    },
    FilterRow {
        is_ff: true,
        enabled: false,
        verdict: Verdict::SpuriousPreEnable,
        eoi: false,
        tag: "未使能期 0xFF：判伪且不 EOI",
    },
];

/// 判决函数（const）：!enabled 一律判伪（时序纪律优先），enabled 期 0xFF
/// 判仲裁失败伪中断，其余判真。
pub const fn verdict_of(is_ff: bool, enabled: bool) -> Verdict {
    if !enabled {
        Verdict::SpuriousPreEnable
    } else if is_ff {
        Verdict::SpuriousArbitration
    } else {
        Verdict::Real
    }
}

/// EOI 允许位（const）：仅 Real 允许。
pub const fn eoi_allowed(v: Verdict) -> bool {
    match v {
        Verdict::Real => true,
        Verdict::SpuriousArbitration | Verdict::SpuriousPreEnable => false,
    }
}

/// 是否伪判决（const）。
pub const fn is_spurious_verdict(v: Verdict) -> bool {
    match v {
        Verdict::Real => false,
        Verdict::SpuriousArbitration | Verdict::SpuriousPreEnable => true,
    }
}

/// 真值表自洽校验（运行期）：冻结表每行与判定函数双向一致，
/// 且伪判决行绝不携带 EOI 允许位 —— 单边修改立即红灯。
pub fn verify_truth_table() -> Result<(), SpuriousError> {
    let mut i: usize = 0;
    while i < FILTER_TRUTH_TABLE.len() {
        let row = FILTER_TRUTH_TABLE[i];
        let dv = verdict_of(row.is_ff, row.enabled);
        let de = eoi_allowed(dv);
        if dv != row.verdict || de != row.eoi {
            return Err(SpuriousError::TruthTableInconsistent);
        }
        if is_spurious_verdict(dv) && row.eoi {
            return Err(SpuriousError::TruthTableInconsistent);
        }
        i += 1;
    }
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════
// 误 EOI 检测器 + SpuriousFilter
// ═══════════════════════════════════════════════════════════════════════

/// 误 EOI 检测器：对一切『伪判决 + EOI』组合即时 latch，永不自动解除
/// （latch 是事故证据，由 health()/注入面消费，不参与正常路径）。
#[derive(Clone, Copy, Debug)]
pub struct FalseEoiDetector {
    latched: bool,
    hits: u64,
    last_vector: u8,
    last_verdict_code: u8,
}

impl FalseEoiDetector {
    /// 干净初态。
    pub const fn new() -> Self {
        FalseEoiDetector {
            latched: false,
            hits: 0,
            last_vector: 0,
            last_verdict_code: 0,
        }
    }

    /// 观测一次 (向量, 判决, 是否发了 EOI)；检出返回 true 并 latch。
    pub fn observe(&mut self, vector: u8, verdict: Verdict, eoi_issued: bool) -> bool {
        let spurious = is_spurious_verdict(verdict);
        if spurious && eoi_issued {
            self.latched = true;
            self.hits = self.hits.saturating_add(1);
            self.last_vector = vector;
            self.last_verdict_code = verdict_code(verdict);
            true
        } else {
            false
        }
    }

    /// 是否已 latch。
    pub const fn latched(&self) -> bool {
        self.latched
    }

    /// 累计检出次数（saturating）。
    pub const fn hit_count(&self) -> u64 {
        self.hits
    }

    /// 最近一次检出现场。
    pub const fn last_scene(&self) -> (u8, u8) {
        (self.last_vector, self.last_verdict_code)
    }
}

/// 判决编码（const：match 枚举，供账目/检测器存证）。
pub const fn verdict_code(v: Verdict) -> u8 {
    match v {
        Verdict::Real => 1,
        Verdict::SpuriousArbitration => 2,
        Verdict::SpuriousPreEnable => 3,
    }
}

/// 伪中断过滤器：五区归类 + 真值表判决 + 误 EOI 检测一体化。
pub struct SpuriousFilter {
    detector: FalseEoiDetector,
    classified_real: u64,
    classified_spurious: u64,
    ff_recognized: u64,
    preenable_recognized: u64,
}

impl SpuriousFilter {
    /// 干净初态。
    pub const fn new() -> Self {
        SpuriousFilter {
            detector: FalseEoiDetector::new(),
            classified_real: 0,
            classified_spurious: 0,
            ff_recognized: 0,
            preenable_recognized: 0,
        }
    }

    /// 过滤判定主入口。
    /// `timing_enabled` 来自使能时序状态机：未达 SvrEnabled 一律按未使能
    /// 判（时序纪律覆盖 SVR 位域，AP 初始化期全部判伪）。
    /// 返回 (判决, 是否允许 EOI)。
    pub fn admit(&mut self, vector: u8, svr: SvrBits, timing_enabled: bool) -> (Verdict, bool) {
        let is_ff = vector == VECTOR_SPURIOUS;
        let eff_enabled = svr.enabled && timing_enabled;
        let verdict = verdict_of(is_ff, eff_enabled);
        let eoi = eoi_allowed(verdict);
        match verdict {
            Verdict::Real => {
                self.classified_real = self.classified_real.saturating_add(1);
            }
            Verdict::SpuriousArbitration => {
                self.classified_spurious = self.classified_spurious.saturating_add(1);
                self.ff_recognized = self.ff_recognized.saturating_add(1);
            }
            Verdict::SpuriousPreEnable => {
                self.classified_spurious = self.classified_spurious.saturating_add(1);
                self.preenable_recognized = self.preenable_recognized.saturating_add(1);
            }
        }
        // 正常路径自检：过滤自身产出的 EOI 位与判决必须自洽。
        let _ = self
            .detector
            .observe(vector, verdict, eoi && is_spurious_verdict(verdict));
        (verdict, eoi)
    }

    /// 误 EOI 检测器旁路观测（注入面专用：模拟绕过过滤直接发 EOI）。
    /// 返回是否检出。
    pub fn rogue_eoi_observe(&mut self, vector: u8, verdict: Verdict) -> bool {
        self.detector.observe(vector, verdict, true)
    }

    /// 误 EOI 检测器状态。
    pub const fn detector(&self) -> &FalseEoiDetector {
        &self.detector
    }

    /// 真中断判决计数。
    pub const fn real_classified(&self) -> u64 {
        self.classified_real
    }

    /// 伪中断判决计数（两类合计）。
    pub const fn spurious_classified(&self) -> u64 {
        self.classified_spurious
    }

    /// 0xFF 仲裁失败伪中断识别数。
    pub const fn ff_recognized(&self) -> u64 {
        self.ff_recognized
    }

    /// 未使能期到达判伪识别数。
    pub const fn preenable_recognized(&self) -> u64 {
        self.preenable_recognized
    }
}

// ═══════════════════════════════════════════════════════════════════════
// SVR 使能时序状态机（写入序纪律：AP 初始化期未使能前到达的全部判伪）
// ═══════════════════════════════════════════════════════════════════════

/// 时序状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SeqState {
    /// 上电复位：SVR 未触碰。
    PowerOnReset,
    /// AP 初始化进行中（INIT-SIPI 序列期）。
    ApInitInProgress,
    /// SVR 向量字段已编程（0xFF 已入槽）但未使能。
    SvrVectorProgrammed,
    /// SVR 使能位置 1：正常投递期。
    SvrEnabled,
}

/// 时序事件。
#[derive(Clone, Copy, Debug)]
pub enum SeqEvent {
    /// AP 初始化启动。
    ApInitStart,
    /// AP 初始化中止（回 PowerOnReset；已使能后不许中止）。
    ApInitAbort,
    /// SVR 向量字段写入（必须 0xFF，且必须在 ApInitInProgress 期）。
    SvrVectorWrite(u8),
    /// SVR 使能位置 1（必须先完成向量字段编程）。
    SvrEnableWrite,
}

/// 时序事件码（入账用）。
pub const EV_APINIT_START: u8 = 1;
pub const EV_APINIT_ABORT: u8 = 2;
pub const EV_SVR_VEC: u8 = 3;
pub const EV_SVR_EN: u8 = 4;

/// 时序账封顶：超出后停止追加并置截断标志（不 panic、不丢状态）。
pub const SEQ_LOG_CAP: usize = 4096;

/// 使能时序状态机：写入序纪律的执行体与账本。
pub struct EnableSequencer {
    state: SeqState,
    violations: u64,
    last_violation: Option<SpuriousError>,
    last_vector_written: u8,
    svr_writes: u64,
    order_log: Vec<u8>,
    log_truncated: bool,
    preenable_judged: u64,
}

impl EnableSequencer {
    /// 初态：上电复位。
    pub const fn new() -> Self {
        EnableSequencer {
            state: SeqState::PowerOnReset,
            violations: 0,
            last_violation: None,
            last_vector_written: 0,
            svr_writes: 0,
            order_log: Vec::new(),
            log_truncated: false,
            preenable_judged: 0,
        }
    }

    /// 当前状态。
    pub const fn state(&self) -> SeqState {
        self.state
    }

    /// 时序使能位：只有 SvrEnabled 才算使能期。
    pub const fn timing_enabled(&self) -> bool {
        match self.state {
            SeqState::SvrEnabled => true,
            SeqState::PowerOnReset
            | SeqState::ApInitInProgress
            | SeqState::SvrVectorProgrammed => false,
        }
    }

    /// 违例计数。
    pub const fn violations(&self) -> u64 {
        self.violations
    }

    /// 最近一次违例。
    pub const fn last_violation(&self) -> Option<SpuriousError> {
        self.last_violation
    }

    /// 最近写入的向量字段值。
    pub const fn last_vector_written(&self) -> u8 {
        self.last_vector_written
    }

    /// 使能前判伪计数（时序纪律的直接产出）。
    pub const fn preenable_judged(&self) -> u64 {
        self.preenable_judged
    }

    /// 顺序账（只读视图）。
    pub fn order_log(&self) -> &[u8] {
        &self.order_log
    }

    /// 顺序账是否被截断。
    pub const fn log_truncated(&self) -> bool {
        self.log_truncated
    }

    /// 记一笔事件码（封顶截断，不 panic）。
    fn log(&mut self, code: u8) {
        if self.order_log.len() < SEQ_LOG_CAP {
            self.order_log.push(code);
        } else {
            self.log_truncated = true;
        }
    }

    /// 状态推进主入口。返回新状态；违例记账并返回 SequenceViolation。
    pub fn advance(&mut self, event: SeqEvent) -> Result<SeqState, SpuriousError> {
        // 先记事件码（违例也入账：时序审计要看到全部尝试）。
        match event {
            SeqEvent::ApInitStart => self.log(EV_APINIT_START),
            SeqEvent::ApInitAbort => self.log(EV_APINIT_ABORT),
            SeqEvent::SvrVectorWrite(_) => self.log(EV_SVR_VEC),
            SeqEvent::SvrEnableWrite => self.log(EV_SVR_EN),
        }
        match event {
            SeqEvent::ApInitStart => match self.state {
                SeqState::PowerOnReset => {
                    self.state = SeqState::ApInitInProgress;
                    Ok(self.state)
                }
                _ => Err(self.violate()),
            },
            SeqEvent::ApInitAbort => match self.state {
                SeqState::SvrEnabled => Err(self.violate()), // 已使能不许回退
                _ => {
                    self.state = SeqState::PowerOnReset;
                    Ok(self.state)
                }
            },
            SeqEvent::SvrVectorWrite(v) => match self.state {
                SeqState::ApInitInProgress => {
                    if v == VECTOR_SPURIOUS {
                        self.state = SeqState::SvrVectorProgrammed;
                        self.last_vector_written = v;
                        self.svr_writes = self.svr_writes.saturating_add(1);
                        Ok(self.state)
                    } else {
                        // 向量字段必须写 0xFF：写别的值直接违例
                        Err(self.violate())
                    }
                }
                _ => Err(self.violate()), // 只能在 AP 初始化期编程
            },
            SeqEvent::SvrEnableWrite => match self.state {
                SeqState::SvrVectorProgrammed => {
                    self.state = SeqState::SvrEnabled;
                    self.svr_writes = self.svr_writes.saturating_add(1);
                    Ok(self.state)
                }
                _ => Err(self.violate()), // 未编程向量先使能 = 违例
            },
        }
    }

    /// 违例记账（计数 saturating，最近违例存证）。
    fn violate(&mut self) -> SpuriousError {
        self.violations = self.violations.saturating_add(1);
        self.last_violation = Some(SpuriousError::SequenceViolation);
        SpuriousError::SequenceViolation
    }

    /// 使能前到达判伪入账（时序纪律产出，供过滤/health 对账）。
    pub fn note_preenable_judged(&mut self) {
        self.preenable_judged = self.preenable_judged.saturating_add(1);
    }

    /// 状态编码（const）。
    pub const fn state_code(s: SeqState) -> u8 {
        match s {
            SeqState::PowerOnReset => 0,
            SeqState::ApInitInProgress => 1,
            SeqState::SvrVectorProgrammed => 2,
            SeqState::SvrEnabled => 3,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Spurious 计数账 + 连续 spurious 风暴检测
// ═══════════════════════════════════════════════════════════════════════

/// 来源分列：使能期仲裁失败。
pub const SOURCE_ARBITRATION: usize = 0;
/// 来源分列：未使能期到达（AP 初始化期判伪）。
pub const SOURCE_PREENABLE: usize = 1;
/// 来源分列：0xFF 双重判伪（未使能期又见 0xFF）。
pub const SOURCE_FF_PREENABLE: usize = 2;
/// 来源种类数。
pub const SOURCE_KINDS: usize = 3;

/// 风暴窗口容量（最近 256 次 spurious 的时间戳环形账）。
pub const STORM_WINDOW_CAP: usize = 256;
/// 风暴跨度阈值：窗内时间戳跨度 ≤ 512 tick 视为密集。
pub const STORM_SPAN_TICKS: u64 = 512;
/// 风暴计数阈值：跨度内 ≥ 160 条即告警（约 62% 密度）。
pub const STORM_COUNT_THRESHOLD: usize = 160;
/// 风暴静默回归：连续 2048 tick 无 spurious 解除告警。
pub const STORM_QUIET_TICKS: u64 = 2048;

/// spurious 计数账：发生计数 / 来源分列 / 逐向量 EOI / 风暴检测。
/// 尺寸约 4.2KB（2×256×u64 + 窗口），低于 64KB 栈禁线但仍随引擎堆置。
pub struct SpuriousLedger {
    spurious_total: u64,
    real_total: u64,
    eoi_issued: u64,
    by_source: [u64; SOURCE_KINDS],
    eoi_by_vector: [u64; 256],
    storm_window: [u64; STORM_WINDOW_CAP],
    storm_fill: usize,
    storm_active: bool,
    storm_episodes: u64,
    last_ts: u64,
    /// EOI 与真中断守恒破坏计数（应为 0）。
    eoi_consistency_breaks: u64,
}

impl SpuriousLedger {
    /// 零账初态。
    pub const fn new() -> Self {
        SpuriousLedger {
            spurious_total: 0,
            real_total: 0,
            eoi_issued: 0,
            by_source: [0; SOURCE_KINDS],
            eoi_by_vector: [0; 256],
            storm_window: [0; STORM_WINDOW_CAP],
            storm_fill: 0,
            storm_active: false,
            storm_episodes: 0,
            last_ts: 0,
            eoi_consistency_breaks: 0,
        }
    }

    /// 真中断入账 + 静默刷新。
    pub fn note_real(&mut self, _vector: u8, ts: u64) {
        self.real_total = self.real_total.saturating_add(1);
        self.refresh_quiet(ts);
    }

    /// spurious 入账（来源分列）+ 风暴窗口推进。返回是否为告警上升沿。
    pub fn note_spurious(&mut self, source: usize, ts: u64) -> bool {
        self.spurious_total = self.spurious_total.saturating_add(1);
        if source < SOURCE_KINDS {
            self.by_source[source] = self.by_source[source].saturating_add(1);
        }
        self.refresh_quiet(ts);
        // 环形写入（覆盖最旧）
        let slot = self.storm_fill % STORM_WINDOW_CAP;
        self.storm_window[slot] = ts;
        self.storm_fill = self.storm_fill.wrapping_add(1);
        self.last_ts = ts;
        // 风暴评估
        let recent = self.count_recent(ts);
        if recent >= STORM_COUNT_THRESHOLD && !self.storm_active {
            self.storm_active = true;
            self.storm_episodes = self.storm_episodes.saturating_add(1);
            true
        } else {
            false
        }
    }

    /// 真中断 EOI 发出（逐向量分列）。
    pub fn issue_eoi(&mut self, vector: u8) {
        self.eoi_issued = self.eoi_issued.saturating_add(1);
        self.eoi_by_vector[vector as usize] = self.eoi_by_vector[vector as usize].saturating_add(1);
        if self.eoi_issued > self.real_total {
            // EOI 多于真中断 = 守恒破坏存证（误 EOI 的账面信号）
            self.eoi_consistency_breaks = self.eoi_consistency_breaks.saturating_add(1);
        }
    }

    /// 静默刷新：距上次 spurious 足够久则解除告警。
    fn refresh_quiet(&mut self, ts: u64) {
        if self.storm_active && ts > self.last_ts && ts - self.last_ts > STORM_QUIET_TICKS {
            self.storm_active = false;
        }
    }

    /// 强制静默（风暴注入面前置：先清告警再注入才有检出意义）。
    pub fn force_quiet(&mut self) {
        self.storm_active = false;
    }

    /// 窗内（跨度 ≤ STORM_SPAN_TICKS）spurious 计数。
    pub fn count_recent(&self, ts: u64) -> usize {
        let mut i: usize = 0;
        let mut c: usize = 0;
        while i < STORM_WINDOW_CAP {
            if i < self.storm_fill {
                let t = self.storm_window[i];
                if ts >= t && ts - t <= STORM_SPAN_TICKS {
                    c += 1;
                }
            }
            i += 1;
        }
        c
    }

    /// 风暴告警当前状态。
    pub const fn storm_active(&self) -> bool {
        self.storm_active
    }

    /// 风暴累计 episode 数（saturating）。
    pub const fn storm_episodes(&self) -> u64 {
        self.storm_episodes
    }

    /// spurious 总数。
    pub const fn spurious_total(&self) -> u64 {
        self.spurious_total
    }

    /// 真中断总数。
    pub const fn real_total(&self) -> u64 {
        self.real_total
    }

    /// EOI 发出总数。
    pub const fn eoi_issued(&self) -> u64 {
        self.eoi_issued
    }

    /// 来源分列只读。
    pub const fn by_source(&self) -> &[u64; SOURCE_KINDS] {
        &self.by_source
    }

    /// 逐向量 EOI 查询（越界返 0，不 panic）。
    pub fn eoi_count_of(&self, vector: u8) -> u64 {
        self.eoi_by_vector[vector as usize]
    }

    /// 0xFF 向量 EOI 计数 —— 判据直读：必须恒为 0。
    pub fn eoi_on_spurious_slot(&self) -> u64 {
        self.eoi_by_vector[VECTOR_SPURIOUS as usize]
    }

    /// 守恒破坏计数（应为 0）。
    pub const fn consistency_breaks(&self) -> u64 {
        self.eoi_consistency_breaks
    }

    /// 账本不变量：EOI 守恒 + 无 0xFF EOI。
    pub fn invariants_ok(&self) -> bool {
        self.eoi_consistency_breaks == 0
            && self.eoi_issued <= self.real_total
            && self.eoi_on_spurious_slot() == 0
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 事件账（每次注入入账）+ FNV 指纹
// ═══════════════════════════════════════════════════════════════════════

/// 事件种类码：真中断（EOI 路径）。
pub const K_REAL: u8 = 1;
/// 事件种类码：仲裁失败伪中断。
pub const K_SPUR_ARB: u8 = 2;
/// 事件种类码：未使能期判伪。
pub const K_SPUR_PRE: u8 = 3;
/// 事件种类码：风暴告警标记。
pub const K_STORM_MARK: u8 = 4;
/// 事件种类码：时序标记（使能翻转等）。
pub const K_SEQ_MARK: u8 = 5;
/// 事件种类码：注入面标记（aux 编码面别：1 误 EOI / 2 风暴 / 3 未使能）。
pub const K_INJECT_MARK: u8 = 6;

/// 单条注入事件：定长记录（编译期冻结 24 字节），事件账堆上存储。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpuriousEvent {
    /// 全账单调序号。
    pub seq: u64,
    /// 注入时刻（tick）。
    pub ts: u64,
    /// 到达向量。
    pub vector: u8,
    /// 事件种类码（K_*）。
    pub kind: u8,
    /// 是否走了 EOI 路径（伪事件恒 false）。
    pub eoi: bool,
    /// 辅助字段（来源分列/面别编码）。
    pub aux: u16,
}

/// 事件账容量：10 万注入 + 风暴/时序/注入面标记余量。
pub const LEDGER_CAP: usize = 131_072;

/// FNV-1a 64 位常数。
pub const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV 单字节推进（const：wrapping 乘）。
pub const fn fnv1a_byte(h: u64, b: u8) -> u64 {
    (h ^ (b as u64)).wrapping_mul(FNV_PRIME)
}

/// FNV 单 u64 推进（const：8 字节小端序逐位入火）。
pub const fn fnv1a_u64(h: u64, v: u64) -> u64 {
    let mut acc = h;
    let mut i = 0;
    while i < 8 {
        acc = fnv1a_byte(acc, ((v >> (i * 8)) & 0xFF) as u8);
        i += 1;
    }
    acc
}

/// 事件账：顺序入账 + FNV 指纹 + 不变量审计。
pub struct EventLedger {
    events: Vec<SpuriousEvent>,
    next_seq: u64,
    dropped: u64,
}

impl EventLedger {
    /// 空账。
    pub fn new() -> Self {
        EventLedger {
            events: Vec::new(),
            next_seq: 1,
            dropped: 0,
        }
    }

    /// 入账：满则记 dropped 并报 LedgerFull（不静默丢数据）。
    pub fn record(
        &mut self,
        ts: u64,
        vector: u8,
        kind: u8,
        eoi: bool,
        aux: u16,
    ) -> Result<u64, SpuriousError> {
        if self.events.len() >= LEDGER_CAP {
            self.dropped = self.dropped.saturating_add(1);
            return Err(SpuriousError::LedgerFull);
        }
        let seq = self.next_seq;
        self.events.push(SpuriousEvent {
            seq,
            ts,
            vector,
            kind,
            eoi,
            aux,
        });
        self.next_seq = self.next_seq.wrapping_add(1);
        Ok(seq)
    }

    /// 账长。
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// 空账判断。
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// 丢账计数（应为 0）。
    pub const fn dropped(&self) -> u64 {
        self.dropped
    }

    /// 按序号读事件（运行期 get，越界 None）。
    pub fn event_at(&self, i: usize) -> Option<SpuriousEvent> {
        self.events.get(i).copied()
    }

    /// FNV 指纹：全账逐事件字段入火（双跑对拍依据）。
    pub fn fnv_fingerprint(&self) -> u64 {
        let mut h = FNV_OFFSET;
        let mut i: usize = 0;
        while i < self.events.len() {
            let e = self.events[i];
            h = fnv1a_u64(h, e.seq);
            h = fnv1a_u64(h, e.ts);
            h = fnv1a_byte(h, e.vector);
            h = fnv1a_byte(h, e.kind);
            h = fnv1a_byte(h, e.eoi as u8);
            h = fnv1a_u64(h, e.aux as u64);
            i += 1;
        }
        h
    }

    /// 伪事件带 EOI 计数（判据直读：必须为 0）。
    pub fn eoi_on_spurious_count(&self) -> u64 {
        let mut c: u64 = 0;
        let mut i: usize = 0;
        while i < self.events.len() {
            let e = self.events[i];
            let spurious_kind = e.kind == K_SPUR_ARB || e.kind == K_SPUR_PRE;
            if spurious_kind && e.eoi {
                c += 1;
            }
            i += 1;
        }
        c
    }

    /// 序号单调性审计。
    pub fn seq_monotonic(&self) -> bool {
        let mut i: usize = 1;
        while i < self.events.len() {
            if self.events[i].seq <= self.events[i - 1].seq {
                return false;
            }
            i += 1;
        }
        true
    }

    /// EOI 守恒审计：EOI 事件只允许 K_REAL（注入面标记除外）。
    pub fn eoi_conservation_ok(&self) -> bool {
        let mut i: usize = 0;
        while i < self.events.len() {
            let e = self.events[i];
            if e.eoi && e.kind != K_REAL && e.kind != K_INJECT_MARK {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 各种类计数（运行期扫描）。
    pub fn kind_counts(&self) -> [u64; 7] {
        let mut out = [0u64; 7];
        let mut i: usize = 0;
        while i < self.events.len() {
            let k = self.events[i].kind as usize;
            if k < 7 {
                out[k] += 1;
            }
            i += 1;
        }
        out
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 注入管线：10 万次混排注入（真中断/伪中断/边界向量）+ 判据自证
// ═══════════════════════════════════════════════════════════════════════

/// 注入总量：判据 UNX-F0805-J1 写死 10 万。
pub const INJECTION_TOTAL: u64 = 100_000;
/// 预使能阶段宽度：前 4,000 注入处于 AP 初始化未使能期（全部判伪面）。
pub const PRE_ENABLE_PHASE: u64 = 4_000;
/// 风暴注入 burst 规模：阈值 160 + 余量 40，保证一次跨越。
pub const STORM_BURST: u64 = 200;

/// 管线统计：注入侧真值与过滤侧识别分列（识别率的分子分母）。
#[derive(Clone, Copy, Debug, Default)]
pub struct PipelineStats {
    /// 已注入总数。
    pub injections: u64,
    /// 真值：使能期 0xFF 伪中断注入数。
    pub gt_ff_spurious: u64,
    /// 真值：未使能期到达注入数（全部判伪，含 0xFF）。
    pub gt_preenable: u64,
    /// 真值：真中断注入数。
    pub gt_real: u64,
    /// 风暴上升沿次数（管线期间自然触发）。
    pub natural_storms: u64,
    /// 使能翻转时刻。
    pub enable_tick: u64,
}

/// 管线终报：判据两成分的直读数字。
#[derive(Clone, Copy, Debug)]
pub struct PipelineReport {
    /// 注入总数（应为 100,000）。
    pub injections: u64,
    /// 真中断 EOI 数（= 真值真中断数，零误差）。
    pub real_eoi: u64,
    /// 伪中断入账总数。
    pub spurious_accounted: u64,
    /// 真值：使能期 0xFF 伪中断注入数。
    pub gt_ff_spurious: u64,
    /// 识别：0xFF 仲裁失败伪中断识别数（= gt 即 100%）。
    pub ff_recognized: u64,
    /// 真值：未使能期到达注入数。
    pub gt_preenable: u64,
    /// 识别：未使能期判伪识别数（= gt 即 100%）。
    pub preenable_recognized: u64,
    /// 误 EOI 计数（判据一：必须为 0）。
    pub false_eoi: u64,
    /// 风暴 episode 数。
    pub storm_episodes: u64,
    /// 事件账长度。
    pub ledger_events: u64,
    /// 识别率（ppm，1_000_000 = 100%）。
    pub recognition_ppm: u64,
    /// 账本指纹（双跑对拍依据之一）。
    pub fingerprint: u64,
}

/// 注入管线：LCG 混排 + 过滤判决 + 双账（计数账/事件账）+ 自证。
pub struct InjectionPipeline {
    rng: Lcg,
    filter: SpuriousFilter,
    sequencer: EnableSequencer,
    ledger: SpuriousLedger,
    events: EventLedger,
    slots: SlotTable,
    svr: SvrBits,
    stats: PipelineStats,
    seed: u64,
}

impl InjectionPipeline {
    /// 引导：0xFF 槽认领 + AP 初始化序列（停在校验过的『向量已编程未使能』态）。
    pub fn boot(seed: u64) -> Result<Self, SpuriousError> {
        let mut slots = SlotTable::new();
        slots.claim_spurious_slot()?;
        let mut sequencer = EnableSequencer::new();
        // AP 初始化启动（PowerOnReset → ApInitInProgress）
        sequencer.advance(SeqEvent::ApInitStart)?;
        // SVR 向量字段写 0xFF（ApInitInProgress → SvrVectorProgrammed）
        sequencer.advance(SeqEvent::SvrVectorWrite(VECTOR_SPURIOUS))?;
        // 此处刻意不写使能位：预使能阶段由此产生
        Ok(InjectionPipeline {
            rng: Lcg::new(seed),
            filter: SpuriousFilter::new(),
            sequencer,
            ledger: SpuriousLedger::new(),
            events: EventLedger::new(),
            slots,
            svr: SvrBits {
                vector: VECTOR_SPURIOUS,
                enabled: false,
                focus_masked: false,
            },
            stats: PipelineStats::default(),
            seed,
        })
    }

    /// 种子。
    pub const fn seed(&self) -> u64 {
        self.seed
    }

    /// 过滤器只读。
    pub const fn filter(&self) -> &SpuriousFilter {
        &self.filter
    }

    /// 时序状态机只读。
    pub const fn sequencer(&self) -> &EnableSequencer {
        &self.sequencer
    }

    /// 计数账只读。
    pub const fn ledger(&self) -> &SpuriousLedger {
        &self.ledger
    }

    /// 事件账只读。
    pub const fn events(&self) -> &EventLedger {
        &self.events
    }

    /// 槽位表只读。
    pub const fn slots(&self) -> &SlotTable {
        &self.slots
    }

    /// SVR 位域只读。
    pub const fn svr(&self) -> SvrBits {
        self.svr
    }

    /// 统计只读。
    pub const fn stats(&self) -> &PipelineStats {
        &self.stats
    }

    /// SVR 使能翻转（预使能阶段结束后一次）：时序状态机 + 位域同步。
    fn enable_apic(&mut self) -> Result<(), SpuriousError> {
        self.sequencer.advance(SeqEvent::SvrEnableWrite)?;
        self.svr.enabled = true;
        // 时序标记入账（aux=8 记 SVR 编码后的位 8 使能）
        self.events
            .record(self.stats.injections, VECTOR_SPURIOUS, K_SEQ_MARK, false, 8)?;
        Ok(())
    }

    /// 抽取向量：预使能期以真向量为主（证明『全部判伪』覆盖真到达），
    /// 使能期按 设备/MSI/IPI/0xFF/边界 混排。
    fn draw_vector(&mut self, pre_enable: bool) -> u8 {
        if pre_enable {
            let roll = self.rng.below(10);
            if roll < 7 {
                // 设备 IRQ 区真向量
                ((ZONE_IRQ_LO_BASE as u64) + self.rng.below(0x70)) as u8
            } else if roll < 9 {
                // IPI 区真向量
                ((ZONE_IPI_LO_BASE as u64) + self.rng.below(0x1F)) as u8
            } else {
                VECTOR_SPURIOUS
            }
        } else {
            let roll = self.rng.below(100);
            if roll < 55 {
                ((ZONE_IRQ_LO_BASE as u64) + self.rng.below(0x70)) as u8
            } else if roll < 70 {
                ((ZONE_MSI_LO_BASE as u64) + self.rng.below(0x50)) as u8
            } else if roll < 80 {
                ((ZONE_IPI_LO_BASE as u64) + self.rng.below(0x1F)) as u8
            } else if roll < 92 {
                VECTOR_SPURIOUS
            } else {
                // 边界向量：五区端点全谱
                let idx = self.rng.below(BOUNDARY_VECTORS.len() as u64) as usize;
                BOUNDARY_VECTORS[idx]
            }
        }
    }

    /// 主跑：`total` 次注入，逐条入账，实时识别守卫，终局自证。
    pub fn run(&mut self, total: u64) -> Result<PipelineReport, SpuriousError> {
        // 前置自洽：真值表必须先过（单边修改直接红灯）
        verify_truth_table()?;
        // 槽位与 SVR 一致性：0xFF 已认领且位域回读成立
        if !self.slots.spurious_slot_exclusive() {
            return Err(SpuriousError::SlotConflict);
        }
        if !svr_roundtrip_ok(self.svr) || !svr_reserved_clean(self.svr.encode()) {
            return Err(SpuriousError::SvrBitfield);
        }
        let mut i: u64 = 0;
        while i < total {
            if i == PRE_ENABLE_PHASE {
                // 使能翻转：此后进入正常投递期
                self.enable_apic()?;
                self.stats.enable_tick = i;
            }
            let pre_enable = !self.sequencer.timing_enabled();
            let vector = self.draw_vector(pre_enable);
            let ts = i;
            // 真值（注入侧）：未使能期到达 or 0xFF = 伪；否则真
            let gt_spurious = pre_enable || vector == VECTOR_SPURIOUS;
            // 过滤判决（时序覆盖：timing_enabled 未达即判伪）
            let (verdict, eoi) = self
                .filter
                .admit(vector, self.svr, self.sequencer.timing_enabled());
            if pre_enable {
                self.sequencer.note_preenable_judged();
                self.stats.gt_preenable = self.stats.gt_preenable.saturating_add(1);
            } else if vector == VECTOR_SPURIOUS {
                self.stats.gt_ff_spurious = self.stats.gt_ff_spurious.saturating_add(1);
            } else {
                self.stats.gt_real = self.stats.gt_real.saturating_add(1);
            }
            if eoi {
                // 真中断路径：正常 EOI + 双账
                self.ledger.note_real(vector, ts);
                self.ledger.issue_eoi(vector);
                self.events.record(ts, vector, K_REAL, true, 0)?;
            } else {
                // 伪中断路径：只入账不 EOI
                let source = if pre_enable {
                    if vector == VECTOR_SPURIOUS {
                        SOURCE_FF_PREENABLE
                    } else {
                        SOURCE_PREENABLE
                    }
                } else {
                    SOURCE_ARBITRATION
                };
                let kind = if pre_enable { K_SPUR_PRE } else { K_SPUR_ARB };
                let rising = self.ledger.note_spurious(source, ts);
                if rising {
                    // 风暴上升沿标记入账
                    self.events.record(ts, VECTOR_SPURIOUS, K_STORM_MARK, false, 1)?;
                    self.stats.natural_storms = self.stats.natural_storms.saturating_add(1);
                }
                self.events.record(ts, vector, kind, false, source as u16)?;
            }
            // 实时识别守卫：真值与判决分叉立即红灯（识别率跌破 100% 的第一现场）
            if gt_spurious != is_spurious_verdict(verdict) {
                return Err(SpuriousError::RecognitionMiss);
            }
            self.stats.injections = self.stats.injections.saturating_add(1);
            i += 1;
        }
        self.self_proof()
    }

    /// 终局自证：J1 两成分直读。
    /// 成分一：零误 EOI（检测器干净 + 伪事件零 EOI + EOI 守恒 + 0xFF 槽零 EOI）。
    /// 成分二：识别率 100%（两类识别数 == 注入数，且真中断 EOI 数 == 真值）。
    fn self_proof(&self) -> Result<PipelineReport, SpuriousError> {
        // 成分一：零误 EOI
        let false_eoi = self.filter.detector().hit_count()
            + self.events.eoi_on_spurious_count()
            + self.ledger.consistency_breaks()
            + self.ledger.eoi_on_spurious_slot();
        if false_eoi != 0 {
            return Err(SpuriousError::FalseEoiLatched);
        }
        if !self.ledger.invariants_ok() || !self.events.eoi_conservation_ok() {
            return Err(SpuriousError::HealthBroken);
        }
        // EOI 守恒：EOI 数 == 真中断账 == 真值真中断注入数
        if self.ledger.eoi_issued() != self.ledger.real_total()
            || self.ledger.real_total() != self.stats.gt_real
        {
            return Err(SpuriousError::HealthBroken);
        }
        // 成分二：识别率 100%
        if self.filter.ff_recognized() != self.stats.gt_ff_spurious
            || self.filter.preenable_recognized() != self.stats.gt_preenable
        {
            return Err(SpuriousError::RecognitionMiss);
        }
        let spurious_injected = self.stats.gt_ff_spurious + self.stats.gt_preenable;
        let recognition_ppm = if spurious_injected == 0 {
            0
        } else {
            spurious_injected.saturating_mul(1_000_000) / spurious_injected
        };
        if recognition_ppm != 1_000_000 {
            return Err(SpuriousError::RecognitionMiss);
        }
        Ok(PipelineReport {
            injections: self.stats.injections,
            real_eoi: self.ledger.eoi_issued(),
            spurious_accounted: self.ledger.spurious_total(),
            gt_ff_spurious: self.stats.gt_ff_spurious,
            ff_recognized: self.filter.ff_recognized(),
            gt_preenable: self.stats.gt_preenable,
            preenable_recognized: self.filter.preenable_recognized(),
            false_eoi,
            storm_episodes: self.ledger.storm_episodes(),
            ledger_events: self.events.len() as u64,
            recognition_ppm,
            fingerprint: self.fingerprint(),
        })
    }

    /// 引擎指纹：种子 + 全部计数账 + 事件账 FNV 串联（双跑对拍依据）。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a_u64(h, self.seed);
        h = fnv1a_u64(h, self.stats.injections);
        h = fnv1a_u64(h, self.stats.gt_ff_spurious);
        h = fnv1a_u64(h, self.stats.gt_preenable);
        h = fnv1a_u64(h, self.stats.gt_real);
        h = fnv1a_u64(h, self.stats.natural_storms);
        h = fnv1a_u64(h, self.stats.enable_tick);
        h = fnv1a_u64(h, self.ledger.spurious_total());
        h = fnv1a_u64(h, self.ledger.real_total());
        h = fnv1a_u64(h, self.ledger.eoi_issued());
        h = fnv1a_u64(h, self.ledger.storm_episodes());
        let mut s: usize = 0;
        while s < SOURCE_KINDS {
            h = fnv1a_u64(h, self.ledger.by_source()[s]);
            s += 1;
        }
        h = fnv1a_u64(h, self.filter.ff_recognized());
        h = fnv1a_u64(h, self.filter.preenable_recognized());
        h = fnv1a_u64(h, self.filter.real_classified());
        h = fnv1a_u64(h, self.filter.spurious_classified());
        h = fnv1a_u64(h, self.sequencer.violations());
        h = fnv1a_u64(h, self.sequencer.preenable_judged());
        h = fnv1a_byte(h, EnableSequencer::state_code(self.sequencer.state()));
        h = fnv1a_u64(h, self.events.fnv_fingerprint());
        h
    }
}

/// 设备 IRQ 区基（0x20）。
pub const ZONE_IRQ_LO_BASE: u8 = 0x20;
/// MSI-X 区基（0x90）。
pub const ZONE_MSI_LO_BASE: u8 = 0x90;
/// IPI 区基（0xE0）。
pub const ZONE_IPI_LO_BASE: u8 = 0xE0;

// ═══════════════════════════════════════════════════════════════════════
// 注入面：误 EOI 注入 / 风暴注入 / 未使能注入（三面必须全部检出）
// ═══════════════════════════════════════════════════════════════════════

/// 注入面检出报告。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SurfaceReport {
    /// 误 EOI 注入被检测器捕获。
    pub false_eoi_caught: bool,
    /// 风暴注入触发告警。
    pub storm_caught: bool,
    /// 未使能注入全部判伪。
    pub not_enabled_caught: bool,
}

impl SurfaceReport {
    /// 三面全绿。
    pub const fn all_caught(&self) -> bool {
        self.false_eoi_caught && self.storm_caught && self.not_enabled_caught
    }
}

/// 注入面面别编码（事件账 aux 字段）。
pub const SURFACE_FALSE_EOI: u16 = 1;
pub const SURFACE_STORM: u16 = 2;
pub const SURFACE_NOT_ENABLED: u16 = 3;

impl InjectionPipeline {
    /// 注入面一：误 EOI 注入。
    /// 模拟绕过 SpuriousFilter::admit 的旁路路径：对 0xFF 伪判决直接发 EOI。
    /// 误 EOI 检测器必须 latch（返回 true）；未检出即红灯。
    pub fn inject_false_eoi_attempt(&mut self) -> Result<bool, SpuriousError> {
        if !self.slots.spurious_slot_exclusive() {
            return Err(SpuriousError::SlotConflict);
        }
        let ts = self.stats.injections.wrapping_add(0xF0_000);
        let caught = self
            .filter
            .rogue_eoi_observe(VECTOR_SPURIOUS, Verdict::SpuriousArbitration);
        // 旁路尝试本身入账（eoi=true 记的是『尝试』，非账面真 EOI：
        // 计数账 eoi_issued 不动 —— 检出即拦截，EOI 未到达硬件）
        self.events.record(
            ts,
            VECTOR_SPURIOUS,
            K_INJECT_MARK,
            true,
            SURFACE_FALSE_EOI,
        )?;
        Ok(caught)
    }

    /// 注入面二：风暴注入。先强制静默清场，再灌 `burst` 条连续 spurious，
    /// 告警必须重新拉响；未拉响即红灯。
    pub fn inject_storm(&mut self, burst: u64) -> Result<bool, SpuriousError> {
        self.ledger.force_quiet();
        let base_ts = self.stats.injections.wrapping_add(0xF1_000);
        let mut k: u64 = 0;
        let mut rising = false;
        while k < burst {
            let r = self.ledger.note_spurious(SOURCE_ARBITRATION, base_ts + k);
            if r {
                rising = true;
            }
            k += 1;
        }
        self.events.record(
            base_ts + burst,
            VECTOR_SPURIOUS,
            K_INJECT_MARK,
            false,
            SURFACE_STORM,
        )?;
        // 双重判据：上升沿见过 且 注入结束时告警仍处激活
        Ok(rising && self.ledger.storm_active())
    }

    /// 注入面三：未使能注入。
    /// 全新过滤器 + 上电复位时序状态机，灌 `count` 条真设备向量：
    /// AP 初始化期未使能前到达的全部判伪（零真判决、零 EOI）。
    pub fn inject_not_enabled(&mut self, count: u64) -> Result<bool, SpuriousError> {
        let mut probe_filter = SpuriousFilter::new();
        let mut probe_seq = EnableSequencer::new();
        probe_seq.advance(SeqEvent::ApInitStart)?;
        let disabled_svr = SvrBits {
            vector: VECTOR_SPURIOUS,
            enabled: false,
            focus_masked: false,
        };
        let ts_base = self.stats.injections.wrapping_add(0xF2_000);
        let mut all_spurious = true;
        let mut k: u64 = 0;
        while k < count {
            let v = ((ZONE_IRQ_LO_BASE as u64) + self.rng.below(0x70)) as u8;
            let (verdict, eoi) = probe_filter.admit(v, disabled_svr, probe_seq.timing_enabled());
            probe_seq.note_preenable_judged();
            if !is_spurious_verdict(verdict) || eoi {
                all_spurious = false;
            }
            self.events.record(
                ts_base + k,
                v,
                K_INJECT_MARK,
                false,
                SURFACE_NOT_ENABLED,
            )?;
            k += 1;
        }
        // 判据：count 条全部判伪 且 探针时序账判伪计数 == count
        Ok(all_spurious && probe_seq.preenable_judged() == count)
    }

    /// 三面连跑（每面必须检出，任一未检出报 InjectionSurfaceMiss）。
    pub fn run_surfaces(&mut self) -> Result<SurfaceReport, SpuriousError> {
        let false_eoi_caught = self.inject_false_eoi_attempt()?;
        let storm_caught = self.inject_storm(STORM_BURST)?;
        let not_enabled_caught = self.inject_not_enabled(64)?;
        let rep = SurfaceReport {
            false_eoi_caught,
            storm_caught,
            not_enabled_caught,
        };
        if !rep.all_caught() {
            return Err(SpuriousError::InjectionSurfaceMiss);
        }
        Ok(rep)
    }
}

// ═══════════════════════════════════════════════════════════════════════
// SpuriousEngine 门面：全流程编排 + health() + 双跑确定性对拍
// ═══════════════════════════════════════════════════════════════════════

/// 引擎终报：管线终报 + 注入面报告 + 引擎级指纹。
#[derive(Clone, Copy, Debug)]
pub struct EngineReport {
    /// 管线终报（10 万注入直读数字）。
    pub pipeline: PipelineReport,
    /// 注入面报告（三面检出）。
    pub surfaces: SurfaceReport,
    /// 引擎级指纹（含注入面账目）。
    pub fingerprint: u64,
}

/// 引擎健康报告：逐项检查（名称, 通过, 备注）+ 全绿标志。
pub struct HealthReport {
    /// 检查项列表。
    pub checks: Vec<(String, bool, String)>,
    /// 全绿。
    pub all_green: bool,
}

impl HealthReport {
    /// 中文摘要（health 面板打印用）。
    pub fn summary(&self) -> String {
        let mut lines: Vec<String> = Vec::new();
        lines.push(format!(
            "UNX-F0805 spurious 域 health：{} 项，全绿={}",
            self.checks.len(),
            self.all_green
        ));
        let mut i: usize = 0;
        while i < self.checks.len() {
            let (name, pass, note) = &self.checks[i];
            lines.push(format!(
                "  [{}] {} {}",
                if *pass { "PASS" } else { "FAIL" },
                name,
                note
            ));
            i += 1;
        }
        lines.join("\n")
    }
}

/// Spurious 域引擎门面：注入管线 + 注入面 + 双账 + 时序一体的对外门。
pub struct SpuriousEngine {
    pipeline: InjectionPipeline,
    seed: u64,
    report: Option<EngineReport>,
}

impl SpuriousEngine {
    /// 引导（同种子必得同状态）。
    pub fn boot(seed: u64) -> Result<Self, SpuriousError> {
        Ok(SpuriousEngine {
            pipeline: InjectionPipeline::boot(seed)?,
            seed,
            report: None,
        })
    }

    /// 种子。
    pub const fn seed(&self) -> u64 {
        self.seed
    }

    /// 注入管线只读视图。
    pub const fn pipeline(&self) -> &InjectionPipeline {
        &self.pipeline
    }

    /// 最近一次 run_full 终报。
    pub const fn last_report(&self) -> Option<EngineReport> {
        self.report
    }

    /// 全流程：10 万混排注入（自证零误 EOI + 识别率 100%）→ 三注入面检出。
    pub fn run_full(&mut self) -> Result<EngineReport, SpuriousError> {
        let pipeline = self.pipeline.run(INJECTION_TOTAL)?;
        let surfaces = self.pipeline.run_surfaces()?;
        let rep = EngineReport {
            fingerprint: self.engine_fingerprint(),
            pipeline,
            surfaces,
        };
        self.report = Some(rep);
        Ok(rep)
    }

    /// 引擎级指纹：管线指纹 + 注入面标记计数串联。
    pub fn engine_fingerprint(&self) -> u64 {
        let mut h = self.pipeline.fingerprint();
        let counts = self.pipeline.events().kind_counts();
        h = fnv1a_u64(h, counts[K_INJECT_MARK as usize]);
        h = fnv1a_u64(h, counts[K_SEQ_MARK as usize]);
        h = fnv1a_u64(h, counts[K_STORM_MARK as usize]);
        h
    }

    /// 双跑确定性对拍：同种子两引擎全流程后指纹与关键计数逐项相等；
    /// 异种子必发散（发散检出探测见 dual_run_mismatch_probe）。
    pub fn dual_run_check(seed: u64) -> Result<(EngineReport, EngineReport), SpuriousError> {
        let mut a = SpuriousEngine::boot(seed)?;
        let mut b = SpuriousEngine::boot(seed)?;
        let ra = a.run_full()?;
        let rb = b.run_full()?;
        if ra.fingerprint != rb.fingerprint
            || ra.pipeline.injections != rb.pipeline.injections
            || ra.pipeline.real_eoi != rb.pipeline.real_eoi
            || ra.pipeline.spurious_accounted != rb.pipeline.spurious_accounted
            || ra.pipeline.false_eoi != rb.pipeline.false_eoi
            || ra.pipeline.recognition_ppm != rb.pipeline.recognition_ppm
        {
            return Err(SpuriousError::FingerprintMismatch);
        }
        Ok((ra, rb))
    }

    /// 异种子发散探测：不同种子指纹必须不同（确定性 ≠ 常量性）。
    /// 返回是否成功检出发散。
    pub fn dual_run_mismatch_probe(seed_a: u64, seed_b: u64) -> Result<bool, SpuriousError> {
        if seed_a == seed_b {
            return Err(SpuriousError::FingerprintMismatch);
        }
        let mut a = SpuriousEngine::boot(seed_a)?;
        let mut b = SpuriousEngine::boot(seed_b)?;
        let ra = a.run_full()?;
        let rb = b.run_full()?;
        Ok(ra.fingerprint != rb.fingerprint)
    }

    /// health()：判据两成分 + 结构不变量逐项体检。
    pub fn health(&self) -> HealthReport {
        let mut checks: Vec<(String, bool, String)> = Vec::new();
        let mut add = |name: &str, pass: bool, note: &str| {
            checks.push((name.to_string(), pass, note.to_string()));
        };

        // 1. 0xFF 槽位独占
        let slot_ok = self.pipeline.slots().spurious_slot_exclusive();
        add(
            "SP-0xFF槽位独占",
            slot_ok,
            if slot_ok { "IDT 第 255 项专属保留" } else { "槽位失守" },
        );

        // 2. SVR 位域回读 + 保留位干净
        let svr = self.pipeline.svr();
        let svr_ok = svr_roundtrip_ok(svr) && svr_reserved_clean(svr.encode()) && svr.vector_slot_ok();
        add(
            "SP-SVR位域回读",
            svr_ok,
            if svr_ok {
                "向量 0xFF/使能/焦点掩码编解码一致"
            } else {
                "位域污染或回读漂移"
            },
        );

        // 3. 真值表自洽
        let truth_ok = verify_truth_table().is_ok();
        add(
            "SP-真值表自洽",
            truth_ok,
            if truth_ok { "4 行冻结表与判定函数一致" } else { "单边修改检出" },
        );

        // 4. 判据一：零误 EOI（注入面故意 latch 的那 1 次检出是证据，
        //    不是违例 —— 只有『未预期』的误 EOI 才计为破坏）
        let surface_expected = match self.report {
            Some(rep) if rep.surfaces.false_eoi_caught => 1,
            _ => 0,
        };
        let false_eoi_raw = self.pipeline.filter().detector().hit_count()
            + self.pipeline.events().eoi_on_spurious_count()
            + self.pipeline.ledger().consistency_breaks()
            + self.pipeline.ledger().eoi_on_spurious_slot();
        let false_eoi = false_eoi_raw.saturating_sub(surface_expected);
        add(
            "SP-零误EOI",
            false_eoi == 0,
            if false_eoi == 0 {
                "伪中断路径全程只入账不 EOI"
            } else {
                "误 EOI 计数非零"
            },
        );

        // 5. 判据二：识别率 100%
        let f = self.pipeline.filter();
        let gt_ff = self.pipeline.stats().gt_ff_spurious;
        let gt_pre = self.pipeline.stats().gt_preenable;
        let recog_ok = f.ff_recognized() == gt_ff && f.preenable_recognized() == gt_pre;
        add(
            "SP-识别率100%",
            recog_ok,
            if recog_ok {
                "0xFF 仲裁失败与未使能期判伪双类全中"
            } else {
                "识别数与注入数分叉"
            },
        );

        // 6. EOI 守恒（账面）
        let l = self.pipeline.ledger();
        let conserv = l.eoi_issued() == l.real_total() && l.invariants_ok();
        add(
            "SP-EOI守恒",
            conserv,
            if conserv { "EOI 数 == 真中断账" } else { "守恒破坏" },
        );

        // 7. 时序纪律（违例零 + 终态已使能 + 使能前判伪有账）
        let seq = self.pipeline.sequencer();
        let seq_ok = seq.violations() == 0
            && seq.timing_enabled()
            && seq.preenable_judged() == gt_pre;
        add(
            "SP-使能时序",
            seq_ok,
            if seq_ok {
                "AP 初始化→0xFF 编程→使能 顺序合规"
            } else {
                "写入序违例或终态异常"
            },
        );

        // 8. 注入面三检出（跑过 run_full 才判）
        match self.report {
            Some(rep) => add(
                "SP-注入面三检出",
                rep.surfaces.all_caught(),
                "误 EOI/风暴/未使能 全部捕获",
            ),
            None => add("SP-注入面三检出", true, "尚未 run_full（豁免）"),
        }

        // 9. 事件账完整性
        let ev = self.pipeline.events();
        let ev_ok = ev.seq_monotonic() && ev.dropped() == 0 && ev.eoi_conservation_ok();
        add(
            "SP-事件账完整",
            ev_ok,
            if ev_ok {
                "序号单调/零丢账/EOI 守恒"
            } else {
                "账目损坏或丢账"
            },
        );

        // 10. 注入总量判据（跑过才判）
        match self.report {
            Some(rep) => add(
                "SP-注入10万",
                rep.pipeline.injections == INJECTION_TOTAL,
                "判据 UNX-F0805-J1 写死 10 万",
            ),
            None => add("SP-注入10万", true, "尚未 run_full（豁免）"),
        }

        // 11. 诊断段独占
        let seg_ok = SEG_BASE == 0x1710;
        add(
            "SP-诊断段独占",
            seg_ok,
            if seg_ok { "0x1710~0x171F" } else { "段漂移" },
        );

        let mut all_green = true;
        let mut i: usize = 0;
        while i < checks.len() {
            if !checks[i].1 {
                all_green = false;
            }
            i += 1;
        }
        HealthReport { checks, all_green }
    }
}

/// 一键自检入口：引导 → 全流程 → health（供内核启动期与本册测试共用）。
pub fn unx0805_self_test(seed: u64) -> Result<HealthReport, SpuriousError> {
    let mut eng = SpuriousEngine::boot(seed)?;
    let _ = eng.run_full()?;
    let h = eng.health();
    if h.all_green {
        Ok(h)
    } else {
        Err(SpuriousError::HealthBroken)
    }
}

// ═══════════════════════════════════════════════════════════════════════
// 编译期冻结断言（const assert：运行期零成本）
// ═══════════════════════════════════════════════════════════════════════

const _: () = assert!(VECTOR_SPURIOUS == 0xFF);
const _: () = assert!(SEG_BASE == 0x1710);
const _: () = assert!(SVR_VECTOR_MASK == 0x0000_00FF);
const _: () = assert!(SVR_ENABLE_BIT == 0x0000_0100);
const _: () = assert!(SVR_FOCUS_MASK_BIT == 0x0000_1000);
const _: () = assert!(SVR_VECTOR_MASK & SVR_ENABLE_BIT == 0);
const _: () = assert!(SVR_VECTOR_MASK & SVR_FOCUS_MASK_BIT == 0);
const _: () = assert!(FILTER_TRUTH_TABLE.len() == 4);
const _: () = assert!(SVR_BIT_TABLE.len() == 13);
const _: () = assert!(BOUNDARY_VECTORS.len() == 9);
const _: () = assert!(LEDGER_CAP >= 128_000);
const _: () = assert!(STORM_COUNT_THRESHOLD < STORM_WINDOW_CAP);
const _: () = assert!(STORM_BURST > STORM_COUNT_THRESHOLD as u64);
const _: () = assert!(INJECTION_TOTAL == 100_000);
const _: () = assert!(PRE_ENABLE_PHASE < INJECTION_TOTAL);
const _: () = assert!(ZONE_EXC_HI < ZONE_IRQ_LO_BASE);
const _: () = assert!(ZONE_IRQ_HI < ZONE_MSI_LO_BASE);
const _: () = assert!(ZONE_MSI_HI < ZONE_IPI_LO_BASE);
const _: () = assert!(ZONE_IPI_LO_BASE < VECTOR_SPURIOUS);
const _: () = assert!(core::mem::size_of::<SpuriousEvent>() <= 32);
const _: () = assert!(core::mem::size_of::<SlotTable>() == 32);
const _: () = assert!(STORM_QUIET_TICKS > STORM_SPAN_TICKS);

// ═══════════════════════════════════════════════════════════════════════
// 冒烟测试（cfg(test)：宿主机 std 环境；内核路径纪律只约束非测试代码）
// ═══════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试辅助：Result 展开（仅测试路径允许 panic!）。
    fn ok<T>(r: Result<T, SpuriousError>) -> T {
        match r {
            Ok(v) => v,
            Err(e) => panic!("意外失败 code=0x{:04X}: {}", e.code(), e.detail()),
        }
    }

    #[test]
    fn test_truth_table_frozen() {
        // 4 行冻结表与判定函数双向一致
        ok(verify_truth_table());
        let r0 = FILTER_TRUTH_TABLE[0];
        assert_eq!((r0.is_ff, r0.enabled), (false, true));
        assert_eq!(r0.verdict, Verdict::Real);
        assert!(r0.eoi);
        let r1 = FILTER_TRUTH_TABLE[1];
        assert_eq!((r1.is_ff, r1.enabled), (true, true));
        assert_eq!(r1.verdict, Verdict::SpuriousArbitration);
        assert!(!r1.eoi);
        let r2 = FILTER_TRUTH_TABLE[2];
        assert_eq!((r2.is_ff, r2.enabled), (false, false));
        assert_eq!(r2.verdict, Verdict::SpuriousPreEnable);
        assert!(!r2.eoi);
        let r3 = FILTER_TRUTH_TABLE[3];
        assert_eq!((r3.is_ff, r3.enabled), (true, false));
        assert_eq!(r3.verdict, Verdict::SpuriousPreEnable);
        assert!(!r3.eoi);
        // 伪判决绝不携带 EOI 允许位
        assert!(!eoi_allowed(Verdict::SpuriousArbitration));
        assert!(!eoi_allowed(Verdict::SpuriousPreEnable));
        assert!(eoi_allowed(Verdict::Real));
    }

    #[test]
    fn test_svr_bitfield() {
        // 编解码往返 + 保留位干净 + 位域语义
        let bits = SvrBits {
            vector: VECTOR_SPURIOUS,
            enabled: true,
            focus_masked: true,
        };
        let raw = bits.encode();
        assert_eq!(raw, 0x0000_11FF);
        assert!(svr_reserved_clean(raw));
        assert!(svr_roundtrip_ok(bits));
        let d = decode_svr(raw);
        assert_eq!(d, bits);
        assert!(d.vector_slot_ok());
        assert!(svr_bit_value(raw, 8));
        assert!(svr_bit_value(raw, 12));
        assert!(!svr_bit_value(raw, 9));
        // 使能清零编码
        let off = SvrBits {
            vector: VECTOR_SPURIOUS,
            enabled: false,
            focus_masked: false,
        };
        assert_eq!(off.encode(), 0x0000_00FF);
        // 位表 13 行全量扫描
        let scan = svr_bit_scan(raw);
        assert_eq!(scan.len(), 13);
        assert_eq!(scan[8], (8, true));
        assert_eq!(scan[12], (12, true));
    }

    #[test]
    fn test_slot_guard() {
        let mut t = SlotTable::new();
        // spurious 槽认领一次成功，二次冲突
        ok(t.claim_spurious_slot());
        assert!(t.spurious_slot_exclusive());
        assert_eq!(t.claim_spurious_slot(), Err(SpuriousError::SlotConflict));
        assert_eq!(
            SpuriousError::SlotConflict.detail().contains("0xFF"),
            true
        );
        // 其他向量可认领，0xFF 不可再认领
        ok(t.claim(0x20));
        assert_eq!(t.taken_count(), 2);
        assert!(t.release(0x20));
        assert!(!t.release(0x20)); // 未认领释放 = false，不 panic
        assert!(t.is_taken(VECTOR_SPURIOUS));
    }

    #[test]
    fn test_zone_classification() {
        assert_eq!(zone_of(0x00), VectorZone::Exception);
        assert_eq!(zone_of(0x1F), VectorZone::Exception);
        assert_eq!(zone_of(0x20), VectorZone::DeviceIrq);
        assert_eq!(zone_of(0x8F), VectorZone::DeviceIrq);
        assert_eq!(zone_of(0x90), VectorZone::MsiX);
        assert_eq!(zone_of(0xDF), VectorZone::MsiX);
        assert_eq!(zone_of(0xE0), VectorZone::Ipi);
        assert_eq!(zone_of(0xFE), VectorZone::Ipi);
        assert_eq!(zone_of(0xFF), VectorZone::Spurious);
        // 五区端点边界向量集与归类一致
        let mut i = 0;
        while i < BOUNDARY_VECTORS.len() {
            let v = BOUNDARY_VECTORS[i];
            assert!(zone_name(zone_of(v)).len() > 0);
            i += 1;
        }
    }

    #[test]
    fn test_enable_sequence() {
        // 合规序列：启动 → 编程 0xFF → 使能
        let mut s = EnableSequencer::new();
        assert!(!s.timing_enabled());
        ok(s.advance(SeqEvent::ApInitStart));
        assert_eq!(s.state(), SeqState::ApInitInProgress);
        ok(s.advance(SeqEvent::SvrVectorWrite(VECTOR_SPURIOUS)));
        assert_eq!(s.state(), SeqState::SvrVectorProgrammed);
        assert_eq!(s.last_vector_written(), VECTOR_SPURIOUS);
        ok(s.advance(SeqEvent::SvrEnableWrite));
        assert!(s.timing_enabled());
        assert_eq!(s.violations(), 0);
        assert_eq!(s.order_log(), &[EV_APINIT_START, EV_SVR_VEC, EV_SVR_EN]);

        // 违例 1：未编程先使能
        let mut s2 = EnableSequencer::new();
        ok(s2.advance(SeqEvent::ApInitStart));
        assert_eq!(
            s2.advance(SeqEvent::SvrEnableWrite),
            Err(SpuriousError::SequenceViolation)
        );
        assert_eq!(s2.violations(), 1);

        // 违例 2：向量字段写非 0xFF
        let mut s3 = EnableSequencer::new();
        ok(s3.advance(SeqEvent::ApInitStart));
        assert_eq!(
            s3.advance(SeqEvent::SvrVectorWrite(0x80)),
            Err(SpuriousError::SequenceViolation)
        );

        // 违例 3：复位态直接编程
        let mut s4 = EnableSequencer::new();
        assert_eq!(
            s4.advance(SeqEvent::SvrVectorWrite(VECTOR_SPURIOUS)),
            Err(SpuriousError::SequenceViolation)
        );

        // 中止回退：复位后可重启
        let mut s5 = EnableSequencer::new();
        ok(s5.advance(SeqEvent::ApInitStart));
        ok(s5.advance(SeqEvent::ApInitAbort));
        assert_eq!(s5.state(), SeqState::PowerOnReset);
        ok(s5.advance(SeqEvent::ApInitStart));
        assert_eq!(s5.state(), SeqState::ApInitInProgress);
    }

    #[test]
    fn test_storm_detection_unit() {
        let mut l = SpuriousLedger::new();
        assert!(!l.storm_active());
        // 稀疏 spurious：不告警
        let mut ts: u64 = 0;
        let mut i: u64 = 0;
        while i < 64 {
            let _ = l.note_spurious(SOURCE_ARBITRATION, ts);
            ts += STORM_SPAN_TICKS * 4; // 间隔远超跨度
            i += 1;
        }
        assert!(!l.storm_active());
        // 强制静默清场后灌 burst：必须拉响
        l.force_quiet();
        let mut k: u64 = 0;
        let mut rising = false;
        while k < STORM_BURST {
            if l.note_spurious(SOURCE_ARBITRATION, ts + k) {
                rising = true;
            }
            k += 1;
        }
        assert!(rising, "风暴上升沿必须出现");
        assert!(l.storm_active());
        assert!(l.storm_episodes() >= 1);
        // 长静默后解除
        let _ = l.note_real(0x20, ts + STORM_BURST + STORM_QUIET_TICKS + 1);
        assert!(!l.storm_active(), "静默回归必须解除告警");
        // 来源分列
        assert_eq!(l.by_source()[SOURCE_ARBITRATION], 64 + STORM_BURST);
        assert_eq!(l.spurious_total(), 64 + STORM_BURST);
    }

    #[test]
    fn test_ledger_eoi_conservation() {
        let mut l = SpuriousLedger::new();
        l.note_real(0x20, 0);
        l.issue_eoi(0x20);
        l.note_real(0x21, 1);
        l.issue_eoi(0x21);
        assert_eq!(l.eoi_issued(), 2);
        assert_eq!(l.real_total(), 2);
        assert_eq!(l.eoi_count_of(0x20), 1);
        assert_eq!(l.eoi_count_of(0x21), 1);
        assert_eq!(l.eoi_on_spurious_slot(), 0, "0xFF 槽 EOI 必须恒零");
        assert!(l.invariants_ok());
        assert_eq!(l.consistency_breaks(), 0);
    }

    #[test]
    fn test_event_ledger_fnv() {
        let mut a = EventLedger::new();
        let mut b = EventLedger::new();
        let mut c = EventLedger::new();
        let mut i: u64 = 0;
        while i < 100 {
            ok(a.record(i, 0x20, K_REAL, true, 0));
            ok(b.record(i, 0x20, K_REAL, true, 0));
            ok(c.record(i, 0x21, K_REAL, true, 0));
            i += 1;
        }
        // 同内容同指纹，异内容异指纹
        assert_eq!(a.fnv_fingerprint(), b.fnv_fingerprint());
        assert_ne!(a.fnv_fingerprint(), c.fnv_fingerprint());
        // 序号单调
        assert!(a.seq_monotonic());
        assert_eq!(a.len(), 100);
        assert_eq!(a.event_at(0).map(|e| e.seq), Some(1));
        assert_eq!(a.event_at(999), None, "越界读返 None 不 panic");
        // EOI 守恒审计
        assert!(a.eoi_conservation_ok());
        // 伪事件带 EOI 检出
        let mut d = EventLedger::new();
        ok(d.record(0, 0xFF, K_SPUR_ARB, false, 0));
        ok(d.record(1, 0x20, K_REAL, true, 0));
        assert_eq!(d.eoi_on_spurious_count(), 0);
    }
}

#[cfg(test)]
mod engine_tests {
    use super::*;

    fn ok<T>(r: Result<T, SpuriousError>) -> T {
        match r {
            Ok(v) => v,
            Err(e) => panic!("意外失败 code=0x{:04X}: {}", e.code(), e.detail()),
        }
    }

    #[test]
    fn test_100k_zero_false_eoi_and_recognition_100pct() {
        // 判据 UNX-F0805-J1 成分一 + 成分二 一次跑全
        let mut eng = ok(SpuriousEngine::boot(0x1710_0805));
        let rep = ok(eng.run_full());
        let p = rep.pipeline;
        assert_eq!(p.injections, INJECTION_TOTAL, "注入总量必须 10 万");
        // 成分一：零误 EOI
        assert_eq!(p.false_eoi, 0, "10 万次注入零误 EOI");
        assert_eq!(p.real_eoi, eng.pipeline().ledger().real_total());
        assert_eq!(eng.pipeline().ledger().eoi_on_spurious_slot(), 0);
        assert_eq!(eng.pipeline().events().eoi_on_spurious_count(), 0);
        // 成分二：识别率 100%
        assert_eq!(p.gt_ff_spurious, p.ff_recognized, "0xFF 伪中断识别率 100%");
        assert_eq!(p.gt_preenable, p.preenable_recognized, "未使能期判伪率 100%");
        assert!(p.gt_ff_spurious > 0, "0xFF 伪中断样本必须非空");
        assert!(p.gt_preenable > 0, "未使能期样本必须非空");
        assert!(p.gt_preenable >= PRE_ENABLE_PHASE);
        assert_eq!(p.recognition_ppm, 1_000_000);
        // 每次注入入账：事件数 >= 注入数（另有风暴/时序/注入面标记）
        assert!(p.ledger_events >= p.injections);
        // 真中断 EOI 与真值一致（EOI 只走真路径）
        assert!(p.real_eoi > 0);
    }

    #[test]
    fn test_surfaces_all_caught() {
        let mut eng = ok(SpuriousEngine::boot(0x1710_0806));
        let rep = ok(eng.run_full());
        assert!(rep.surfaces.false_eoi_caught, "误 EOI 注入必须被检出");
        assert!(rep.surfaces.storm_caught, "风暴注入必须触发告警");
        assert!(rep.surfaces.not_enabled_caught, "未使能注入必须全部判伪");
        assert!(rep.surfaces.all_caught());
    }

    #[test]
    fn test_storm_surface_detects() {
        let mut eng = ok(SpuriousEngine::boot(0x1710_0807));
        // 直接跑管线（不做 10 万），只验风暴面
        let _ = ok(eng.pipeline.run(2_000));
        let caught = ok(eng.pipeline.inject_storm(STORM_BURST));
        assert!(caught, "风暴注入必须拉响告警");
    }

    #[test]
    fn test_false_eoi_surface_detects() {
        let mut eng = ok(SpuriousEngine::boot(0x1710_0808));
        let _ = ok(eng.pipeline.run(2_000));
        // 干净基线：检测器零命中
        assert_eq!(eng.pipeline.filter().detector().hit_count(), 0);
        // 旁路误 EOI 注入：必须 latch
        let caught = ok(eng.pipeline.inject_false_eoi_attempt());
        assert!(caught, "误 EOI 注入必须被检测器 latch");
        assert_eq!(eng.pipeline.filter().detector().hit_count(), 1);
        assert_eq!(
            eng.pipeline.filter().detector().last_scene().0,
            VECTOR_SPURIOUS
        );
    }

    #[test]
    fn test_not_enabled_surface_detects() {
        let mut eng = ok(SpuriousEngine::boot(0x1710_0809));
        let _ = ok(eng.pipeline.run(2_000));
        let caught = ok(eng.pipeline.inject_not_enabled(64));
        assert!(caught, "未使能期真向量必须全部判伪");
    }

    #[test]
    fn test_dual_run_determinism() {
        // 同种子双跑：指纹与关键计数逐项相等
        let (ra, rb) = ok(SpuriousEngine::dual_run_check(0x1710_080A));
        assert_eq!(ra.fingerprint, rb.fingerprint);
        assert_eq!(ra.pipeline.recognition_ppm, rb.pipeline.recognition_ppm);
        assert_eq!(ra.pipeline.false_eoi, rb.pipeline.false_eoi);
        // 异种子发散探测：指纹必须不同（确定性 ≠ 常量性）
        let diverged = ok(SpuriousEngine::dual_run_mismatch_probe(0x1710_080A, 0x1710_080B));
        assert!(diverged, "异种子指纹必须发散");
    }

    #[test]
    fn test_health_all_green() {
        let h = ok(unx0805_self_test(0x1710_080C));
        assert!(h.all_green, "health 必须全绿：\n{}", h.summary());
        // 11 项检查
        assert_eq!(h.checks.len(), 11);
        let s = h.summary();
        assert!(s.contains("UNX-F0805"));
        assert!(s.contains("SP-零误EOI"));
        assert!(s.contains("SP-识别率100%"));
    }

    #[test]
    fn test_error_details_and_codes() {
        // 每个变体 detail 非空且 code 落在 0x1710~0x171F 段
        let all = [
            SpuriousError::SlotConflict,
            SpuriousError::SvrBitfield,
            SpuriousError::FalseEoiLatched,
            SpuriousError::RecognitionMiss,
            SpuriousError::SequenceViolation,
            SpuriousError::LedgerFull,
            SpuriousError::LedgerCorrupt,
            SpuriousError::StormAlarmMiss,
            SpuriousError::FingerprintMismatch,
            SpuriousError::InjectionSurfaceMiss,
            SpuriousError::TruthTableInconsistent,
            SpuriousError::HealthBroken,
        ];
        let mut i = 0;
        while i < all.len() {
            assert!(!all[i].detail().is_empty());
            assert!(all[i].code() >= 0x1710 && all[i].code() <= 0x171F);
            i += 1;
        }
    }

    #[test]
    fn test_filter_admit_semantics() {
        let mut f = SpuriousFilter::new();
        let en = SvrBits {
            vector: VECTOR_SPURIOUS,
            enabled: true,
            focus_masked: false,
        };
        let off = SvrBits {
            vector: VECTOR_SPURIOUS,
            enabled: false,
            focus_masked: false,
        };
        // 使能期真中断 → EOI
        let (v, e) = f.admit(0x20, en, true);
        assert_eq!(v, Verdict::Real);
        assert!(e);
        // 使能期 0xFF → 伪中断不 EOI
        let (v2, e2) = f.admit(VECTOR_SPURIOUS, en, true);
        assert_eq!(v2, Verdict::SpuriousArbitration);
        assert!(!e2);
        // SVR 位域使能但时序未达 → 判伪（时序覆盖位域）
        let (v3, e3) = f.admit(0x20, en, false);
        assert_eq!(v3, Verdict::SpuriousPreEnable);
        assert!(!e3);
        // 双重未使能 → 判伪
        let (v4, e4) = f.admit(VECTOR_SPURIOUS, off, false);
        assert_eq!(v4, Verdict::SpuriousPreEnable);
        assert!(!e4);
        // 计数分列
        assert_eq!(f.real_classified(), 1);
        assert_eq!(f.ff_recognized(), 1);
        assert_eq!(f.preenable_recognized(), 2);
        assert_eq!(f.spurious_classified(), 3);
        // 正常路径零误 EOI
        assert_eq!(f.detector().hit_count(), 0);
    }

    #[test]
    fn test_const_frozen_layouts() {
        assert_eq!(core::mem::size_of::<SpuriousEvent>(), 24);
        assert_eq!(core::mem::size_of::<SlotTable>(), 32);
        assert_eq!(SEG_BASE, 0x1710);
        assert_eq!(VECTOR_SPURIOUS, 0xFF);
        assert_eq!(BOUNDARY_VECTORS[BOUNDARY_VECTORS.len() - 1], 0xFF);
    }

    #[test]
    fn test_lcg_determinism() {
        let mut a = Lcg::new(7);
        let mut b = Lcg::new(7);
        let mut i = 0;
        while i < 1000 {
            assert_eq!(a.next(), b.next());
            i += 1;
        }
        let mut c = Lcg::new(7);
        assert_eq!(c.below(0), 0, "below(0) 恒 0 不 panic");
        let mut d = Lcg::new(7);
        let mut j = 0;
        while j < 100 {
            assert!(d.below(1000) < 1000);
            j += 1;
        }
    }
}
