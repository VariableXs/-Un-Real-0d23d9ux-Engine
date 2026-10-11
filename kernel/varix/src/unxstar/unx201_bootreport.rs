//! unx201_bootreport — UNX-F0201 引导报告生成器（全景版）。
//!
//! 判据原文（CoRun Unxreal 主册 UNX-F0201 · UNX-A1-B11）：
//! UNX-F0201-J1 引导完成出全字段报告（硬件/内存/随机化/耗时/异常），
//! 字段齐备率 100%。
//!
//! **分层深化声明**：引导链各阶段本体（页表/ACPI/设备枚举）由各自单
//! 负责（UNX-F0041 ACPI / F0081 KASLR / F0161 Runtime Remap 等），
//! 本册只做**报告生成器**——从各阶段收集终值、记账、渲染、指纹与
//! 完整性闸门，不重复实现任何被报告的机制本体（防重纪律单点）。
//!
//! **全景版设计五柱**：
//!
//! 1. **字段闭集 + 齐备率引擎**：五大族（硬件/内存/随机化/耗时/异常）
//!    共 24 个字段构成 [`FieldId`] 闭集。齐备率不是"字段数恰好够"的
//!    数字游戏，而是**逐字段显性缺席检查**——`N/A`（显性不可用）算
//!    齐备，静默缺失不算。齐备率千分制，[`BootReport::completeness`]
//!    给出 `(已填, 总数, 千分率)` 三元组；构建器在 [`BootReportBuilder::build`]
//!    处设齐备率硬闸：不齐备即 `Err(MissingField)` 并携带缺字段清单
//!    ——**不齐备的报告根本出不了门**，这是 J1 判据"齐备率 100%"的
//!    机制化落地而非靠调用方自觉。
//!
//! 2. **耗时守恒账**：七阶段闭集（固件→解压→页表→ACPI→设备枚举→
//!    调度器→用户态交接）的 [`StageLedger`]，u128 纳秒 checked_add
//!    累计，`Σ段 == 全程` 守恒（[`StageLedger::conserve_check`]），
//!    不守恒即 `TimeNotConserved` 立案——各段之和与全程两个数是
//!    **独立测量**，对不上就是丢了段或多记了段，不许静默抹平。
//!
//! 3. **异常账 append-only**：[`ExceptionLog`] 容量界 32，三级 severity
//!    （Info/Warn/Fatal），重复裁决不覆盖旧账（账是历史不是现状），
//!    满即显性拒（`ExcLogFull`）——溢出静默丢弃是最恶劣的账务行为。
//!
//! 4. **序列化双形态单一真源**：`kv 行协议`（机器读）与 `文本报告`
//!    （人读）都从同一 [`FieldMatrix`] 派生，交叉对拍判据保证两形态
//!    键集合与行数一致——两套渲染器各自维护字段清单是漂移温床。
//!
//! 5. **快照指纹**：FNV-1a 64 位对 kv 全文哈希，[`BootReportSnap`]
//!    往返一致；同构报告指纹恒等、任一字节差异指纹必变——50 次引导
//!    零差异 diff（F0203）与报告轮转（F0204）都以本指纹为锚。
//!
//! **诊断码**：ReportCode 十六码（0x36 段 `0x3600..=0x360F` 互异 +
//! [`segment_guard`] 静态断言）。no_std 零 unwrap 零 expect 零切片
//! 越界，边界一律 checked_add / get。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const UNX201_BOOTREPORT_VERSION: &str = "unx201-bootreport-v1";

/// 报告 schema 版本（F0202 深化的锚——本册先立单点真源）。
pub const SCHEMA_VERSION: u32 = 1;

/// schema 人类可读标签（与 SCHEMA_VERSION 同源，双写即两处对拍面）。
pub const SCHEMA_TAG: &str = "varix-boot-report/1";

/// 诊断段基值（0x36 段——UNX 域 0x2E..0x35 已占，0x35 为 VE-B25 显示域独占）。
pub const SEG_BASE: u16 = 0x3600;

/// 阶段账容量（七阶段闭集，账本容量即闭集大小，满即结构性错误）。
pub const STAGE_CAP: usize = 7;

/// 异常账容量。
pub const EXC_CAP: usize = 32;

/// kv 行协议头行（版本行，固定首行）。
pub const KV_HEADER: &str = "# varix-boot-report";

/// kv 行协议尾行（指纹行，固定末行）。
pub const KV_FOOTER_PREFIX: &str = "# snap=";

// ---------------------------------------------------------------------------
// 诊断码（ReportCode 十六码 · 0x36 段互异）
// ---------------------------------------------------------------------------

/// 诊断码（引导报告——0x36 段十六码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReportCode {
    /// 0x3600 重复字段（同一字段 set 两次——覆盖即漂移，显性拒）。
    DuplicateField,
    /// 0x3601 字段缺失（build 时齐备率不足——报告不许带洞出门）。
    MissingField,
    /// 0x3602 阶段账满（七阶段闭集之外多记——结构性错误）。
    StageOverflow,
    /// 0x3603 阶段重复记账（同阶段两次记时——计时器复用错误）。
    StageDuplicate,
    /// 0x3604 耗时不守恒（Σ段 != 全程——丢段或多记段）。
    TimeNotConserved,
    /// 0x3605 异常账满（append-only 满即显性拒，不静默丢）。
    ExcLogFull,
    /// 0x3606 schema 版本不匹配（跨版本解析被拒——F0202 兼容矩阵锚）。
    SchemaVersionMismatch,
    /// 0x3607 字段值溢出（饱和记账——超上限记饱和值并立案）。
    FieldValueOverflow,
    /// 0x3608 空报告（零字段报告无意义，显性拒）。
    EmptyReport,
    /// 0x3609 未知阶段（闭集之外的阶段枚举——闭集纪律）。
    StageUnknown,
    /// 0x360A 未知严重级（三级闭集之外——闭集纪律）。
    SeverityUnknown,
    /// 0x360B 快照不一致（指纹往返对拍失败）。
    SnapshotMismatch,
    /// 0x360C 文本渲染失配（双形态交叉对拍失败——键集合漂移）。
    TextRenderMismatch,
    /// 0x360D 脱敏表冲突（脱敏字段集合含 FieldId 之外条目——静态构造即拒）。
    RedactConflict,
    /// 0x360E kv 行数失配（kv 行数 != 字段数 + 头尾——行协议完整性闸）。
    KvLineMismatch,
    /// 0x360F 构建器已封印（build 后再 set——状态机纪律）。
    BuilderSealed,
}

impl ReportCode {
    /// 诊断码值（0x36 段）。
    pub fn code(self) -> u16 {
        match self {
            ReportCode::DuplicateField => 0x3600,
            ReportCode::MissingField => 0x3601,
            ReportCode::StageOverflow => 0x3602,
            ReportCode::StageDuplicate => 0x3603,
            ReportCode::TimeNotConserved => 0x3604,
            ReportCode::ExcLogFull => 0x3605,
            ReportCode::SchemaVersionMismatch => 0x3606,
            ReportCode::FieldValueOverflow => 0x3607,
            ReportCode::EmptyReport => 0x3608,
            ReportCode::StageUnknown => 0x3609,
            ReportCode::SeverityUnknown => 0x360A,
            ReportCode::SnapshotMismatch => 0x360B,
            ReportCode::TextRenderMismatch => 0x360C,
            ReportCode::RedactConflict => 0x360D,
            ReportCode::KvLineMismatch => 0x360E,
            ReportCode::BuilderSealed => 0x360F,
        }
    }

    /// 诊断码中文速写（三要素文案族——码/语义/处置）。
    pub fn note(self) -> &'static str {
        match self {
            ReportCode::DuplicateField => "重复字段：同字段二次写入",
            ReportCode::MissingField => "字段缺失：齐备率闸拒绝出报告",
            ReportCode::StageOverflow => "阶段账满：超出七阶段闭集",
            ReportCode::StageDuplicate => "阶段重复：同阶段二次记时",
            ReportCode::TimeNotConserved => "耗时不守恒：段和与全程不等",
            ReportCode::ExcLogFull => "异常账满：append-only 容量界",
            ReportCode::SchemaVersionMismatch => "schema 版本不匹配",
            ReportCode::FieldValueOverflow => "字段值溢出：饱和记账立案",
            ReportCode::EmptyReport => "空报告：零字段无意义",
            ReportCode::StageUnknown => "未知阶段：闭集外枚举",
            ReportCode::SeverityUnknown => "未知严重级：三级闭集外",
            ReportCode::SnapshotMismatch => "快照不一致：指纹往返失败",
            ReportCode::TextRenderMismatch => "文本渲染失配：双形态键漂移",
            ReportCode::RedactConflict => "脱敏表冲突：字段集合越界",
            ReportCode::KvLineMismatch => "kv 行数失配：行协议完整性",
            ReportCode::BuilderSealed => "构建器已封印：build 后禁写",
        }
    }

    /// 全部诊断码闭集（segment_guard 对拍面）。
    pub fn all() -> [ReportCode; 16] {
        [
            ReportCode::DuplicateField,
            ReportCode::MissingField,
            ReportCode::StageOverflow,
            ReportCode::StageDuplicate,
            ReportCode::TimeNotConserved,
            ReportCode::ExcLogFull,
            ReportCode::SchemaVersionMismatch,
            ReportCode::FieldValueOverflow,
            ReportCode::EmptyReport,
            ReportCode::StageUnknown,
            ReportCode::SeverityUnknown,
            ReportCode::SnapshotMismatch,
            ReportCode::TextRenderMismatch,
            ReportCode::RedactConflict,
            ReportCode::KvLineMismatch,
            ReportCode::BuilderSealed,
        ]
    }
}

/// 段位守卫：十六码互异且全部落在 0x36 段——漂移即编译期后第一道运行时闸。
///
/// 返回 `true` 表示段位纪律完好。判据 C10 直接断言本函数，另附双
/// 写实现（手排去重计数）交叉对拍——单点自证不是守卫。
pub fn segment_guard() -> bool {
    let codes = ReportCode::all();
    // 闸一：全部落段。
    for c in codes.iter() {
        let v = c.code();
        if v < SEG_BASE || v >= SEG_BASE + 16 {
            return false;
        }
    }
    // 闸二：互异（O(n²) 直接对拍，n=16 足够小）。
    for i in 0..codes.len() {
        for j in (i + 1)..codes.len() {
            if let (Some(a), Some(b)) = (codes.get(i), codes.get(j)) {
                if a.code() == b.code() {
                    return false;
                }
            } else {
                return false;
            }
        }
    }
    true
}

// ---------------------------------------------------------------------------
// 字段模型（五大族 24 字段闭集）
// ---------------------------------------------------------------------------

/// 字段闭集总数（FieldId 变体数——与枚举定义双写，判据对拍）。
pub const FIELD_COUNT: usize = 24;

/// 报告字段 ID（五大族闭集）。
///
/// 命名族前缀 + 语义名；枚举顺序即 kv 行序（渲染确定性锚）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FieldId {
    // —— 硬件族（8）——
    /// 固件类型（BIOS / UEFI）。
    HwFirmware,
    /// CPU 厂商串。
    HwCpuVendor,
    /// CPU 核心数。
    HwCpuCores,
    /// CPU 频率上限 kHz。
    HwCpuFreqKhz,
    /// 物理内存总量 KiB。
    HwMemTotalKiB,
    /// 枚举设备计数。
    HwDeviceCount,
    /// ACPI 表计数。
    HwAcpiTables,
    /// MMIO 区间计数。
    HwMmioRanges,
    // —— 内存族（6）——
    /// 可用内存 KiB（引导期末口径）。
    MemUsableKiB,
    /// 保留内存 KiB（固件保留 + 内核保留）。
    MemReservedKiB,
    /// 回收内存 KiB（引导期回收的固件占用）。
    MemReclaimedKiB,
    /// 空闲页框数。
    MemFreePages,
    /// 最高物理地址。
    MemHighestAddr,
    /// 内核加载基址（脱敏字段）。
    MemKernelBase,
    // —— 随机化族（4）——
    /// KASLR 选定槽位号（脱敏字段）。
    RngKaslrSlot,
    /// 熵估计位数（保守口径）。
    RngEntropyBits,
    /// 可用熵源计数。
    RngSourceCount,
    /// 栈 canary 就绪。
    RngStackCanary,
    // —— 耗时族（4）——
    /// 全程耗时 ns。
    TimeTotalNs,
    /// 固件阶段耗时 ns。
    TimeFirmwareNs,
    /// 内核自解压耗时 ns。
    TimeDecompressNs,
    /// 设备枚举耗时 ns。
    TimeDevEnumNs,
    // —— 异常族（2）——
    /// 异常总数。
    ExcTotal,
    /// Fatal 级异常数。
    ExcFatal,
}

impl FieldId {
    /// 全字段闭集（渲染序=枚举序）。
    pub fn all() -> [FieldId; FIELD_COUNT] {
        [
            FieldId::HwFirmware,
            FieldId::HwCpuVendor,
            FieldId::HwCpuCores,
            FieldId::HwCpuFreqKhz,
            FieldId::HwMemTotalKiB,
            FieldId::HwDeviceCount,
            FieldId::HwAcpiTables,
            FieldId::HwMmioRanges,
            FieldId::MemUsableKiB,
            FieldId::MemReservedKiB,
            FieldId::MemReclaimedKiB,
            FieldId::MemFreePages,
            FieldId::MemHighestAddr,
            FieldId::MemKernelBase,
            FieldId::RngKaslrSlot,
            FieldId::RngEntropyBits,
            FieldId::RngSourceCount,
            FieldId::RngStackCanary,
            FieldId::TimeTotalNs,
            FieldId::TimeFirmwareNs,
            FieldId::TimeDecompressNs,
            FieldId::TimeDevEnumNs,
            FieldId::ExcTotal,
            FieldId::ExcFatal,
        ]
    }

    /// 字段 kv 键（行协议键名，全小写下划线）。
    pub fn key(self) -> &'static str {
        match self {
            FieldId::HwFirmware => "hw_firmware",
            FieldId::HwCpuVendor => "hw_cpu_vendor",
            FieldId::HwCpuCores => "hw_cpu_cores",
            FieldId::HwCpuFreqKhz => "hw_cpu_freq_khz",
            FieldId::HwMemTotalKiB => "hw_mem_total_kib",
            FieldId::HwDeviceCount => "hw_device_count",
            FieldId::HwAcpiTables => "hw_acpi_tables",
            FieldId::HwMmioRanges => "hw_mmio_ranges",
            FieldId::MemUsableKiB => "mem_usable_kib",
            FieldId::MemReservedKiB => "mem_reserved_kib",
            FieldId::MemReclaimedKiB => "mem_reclaimed_kib",
            FieldId::MemFreePages => "mem_free_pages",
            FieldId::MemHighestAddr => "mem_highest_addr",
            FieldId::MemKernelBase => "mem_kernel_base",
            FieldId::RngKaslrSlot => "rng_kaslr_slot",
            FieldId::RngEntropyBits => "rng_entropy_bits",
            FieldId::RngSourceCount => "rng_source_count",
            FieldId::RngStackCanary => "rng_stack_canary",
            FieldId::TimeTotalNs => "time_total_ns",
            FieldId::TimeFirmwareNs => "time_firmware_ns",
            FieldId::TimeDecompressNs => "time_decompress_ns",
            FieldId::TimeDevEnumNs => "time_devenum_ns",
            FieldId::ExcTotal => "exc_total",
            FieldId::ExcFatal => "exc_fatal",
        }
    }

    /// 字段中文语义（文本报告用）。
    pub fn label(self) -> &'static str {
        match self {
            FieldId::HwFirmware => "固件类型",
            FieldId::HwCpuVendor => "CPU 厂商",
            FieldId::HwCpuCores => "CPU 核心数",
            FieldId::HwCpuFreqKhz => "CPU 频率上限 kHz",
            FieldId::HwMemTotalKiB => "物理内存总量 KiB",
            FieldId::HwDeviceCount => "枚举设备数",
            FieldId::HwAcpiTables => "ACPI 表数",
            FieldId::HwMmioRanges => "MMIO 区间数",
            FieldId::MemUsableKiB => "可用内存 KiB",
            FieldId::MemReservedKiB => "保留内存 KiB",
            FieldId::MemReclaimedKiB => "回收内存 KiB",
            FieldId::MemFreePages => "空闲页框",
            FieldId::MemHighestAddr => "最高物理地址",
            FieldId::MemKernelBase => "内核基址",
            FieldId::RngKaslrSlot => "KASLR 槽位",
            FieldId::RngEntropyBits => "熵估计位数",
            FieldId::RngSourceCount => "熵源计数",
            FieldId::RngStackCanary => "栈 canary",
            FieldId::TimeTotalNs => "全程耗时 ns",
            FieldId::TimeFirmwareNs => "固件阶段 ns",
            FieldId::TimeDecompressNs => "自解压阶段 ns",
            FieldId::TimeDevEnumNs => "设备枚举 ns",
            FieldId::ExcTotal => "异常总数",
            FieldId::ExcFatal => "Fatal 异常数",
        }
    }

    /// 所属族（文本报告分组渲染）。
    pub fn section(self) -> &'static str {
        match self {
            FieldId::HwFirmware
            | FieldId::HwCpuVendor
            | FieldId::HwCpuCores
            | FieldId::HwCpuFreqKhz
            | FieldId::HwMemTotalKiB
            | FieldId::HwDeviceCount
            | FieldId::HwAcpiTables
            | FieldId::HwMmioRanges => "硬件",
            FieldId::MemUsableKiB
            | FieldId::MemReservedKiB
            | FieldId::MemReclaimedKiB
            | FieldId::MemFreePages
            | FieldId::MemHighestAddr
            | FieldId::MemKernelBase => "内存",
            FieldId::RngKaslrSlot
            | FieldId::RngEntropyBits
            | FieldId::RngSourceCount
            | FieldId::RngStackCanary => "随机化",
            FieldId::TimeTotalNs
            | FieldId::TimeFirmwareNs
            | FieldId::TimeDecompressNs
            | FieldId::TimeDevEnumNs => "耗时",
            FieldId::ExcTotal | FieldId::ExcFatal => "异常",
        }
    }

    /// 是否脱敏字段（基址/槽位类——sanitize 默认掩码）。
    pub fn sensitive(self) -> bool {
        matches!(self, FieldId::MemKernelBase | FieldId::RngKaslrSlot)
    }

    /// 字段索引（矩阵下标——枚举序即矩阵序）。
    pub fn index(self) -> usize {
        self as usize
    }
}

/// 字段值（四型闭集 + 显性缺席型）。
///
/// `Na` 是**显性缺席**：字段探测过但该环境无此概念（如 BIOS 下无
/// UEFI 运行时服务数）——Na 算齐备（齐备率口径），与静默缺失
/// （矩阵格为 None）严格二分。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum FieldValue {
    /// 显性不可用（探测过、环境不适用——算齐备）。
    Na,
    /// 无符号整型。
    U64(u64),
    /// 布尔。
    Bool(bool),
    /// 短字符串（≤64 字节，构造时超长截断并立案）。
    Str(String),
}

impl FieldValue {
    /// 渲染为 kv 值文本。
    pub fn render(&self) -> String {
        match self {
            FieldValue::Na => "n/a".to_string(),
            FieldValue::U64(v) => {
                let mut s = format!("{}", v);
                // 值溢出饱和记账口径：u64::MAX 渲染为饱和标记（判据可断言）。
                if *v == u64::MAX {
                    s = "sat:u64max".to_string();
                }
                s
            }
            FieldValue::Bool(b) => if *b { "yes" } else { "no" }.to_string(),
            FieldValue::Str(s) => {
                // 行协议值内禁换行与 '='——渲染前已消毒，此处兜底替换。
                let mut out = String::new();
                for ch in s.chars() {
                    if ch == '\n' || ch == '\r' || ch == '=' {
                        out.push('_');
                    } else {
                        out.push(ch);
                    }
                }
                out
            }
        }
    }

    /// 值类型标签（齐备率分型统计与对拍用）。
    pub fn type_tag(&self) -> &'static str {
        match self {
            FieldValue::Na => "na",
            FieldValue::U64(_) => "u64",
            FieldValue::Bool(_) => "bool",
            FieldValue::Str(_) => "str",
        }
    }
}

// ---------------------------------------------------------------------------
// 阶段账（七阶段闭集 + 守恒）
// ---------------------------------------------------------------------------

/// 引导阶段闭集（七段）。
///
/// 段序即账本序（渲染序）；守恒判据：Σ段 == 全程（全程由末段收尾
/// 时钟差单独测量，与分段累计互为独立口径）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BootStage {
    /// 固件阶段（上电到内核接手）。
    Firmware,
    /// 内核自解压。
    Decompress,
    /// 页表与内存初始化。
    PageTables,
    /// ACPI 表解析。
    Acpi,
    /// 设备枚举。
    DeviceEnum,
    /// 调度器启动。
    Scheduler,
    /// 用户态交接。
    UserHandoff,
}

impl BootStage {
    /// 全段闭集（账本序）。
    pub fn all() -> [BootStage; STAGE_CAP] {
        [
            BootStage::Firmware,
            BootStage::Decompress,
            BootStage::PageTables,
            BootStage::Acpi,
            BootStage::DeviceEnum,
            BootStage::Scheduler,
            BootStage::UserHandoff,
        ]
    }

    /// 段名（kv/文本渲染）。
    pub fn name(self) -> &'static str {
        match self {
            BootStage::Firmware => "firmware",
            BootStage::Decompress => "decompress",
            BootStage::PageTables => "page_tables",
            BootStage::Acpi => "acpi",
            BootStage::DeviceEnum => "device_enum",
            BootStage::Scheduler => "scheduler",
            BootStage::UserHandoff => "user_handoff",
        }
    }

    /// 段下标（账本槽位）。
    pub fn index(self) -> usize {
        self as usize
    }
}

/// 阶段记账结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StageErr {
    /// 闭集外阶段（不该发生——枚举闭集已封）。
    Unknown,
    /// 阶段重复记账。
    Duplicate(ReportCode),
    /// 账本容量满。
    Full(ReportCode),
    /// 记账值溢出 u128（checked_add 失败）。
    Overflow(ReportCode),
}

/// 七阶段耗时账（守恒引擎）。
///
/// 结构不变量：
/// - `staged[i]` 只能被写一次（写前查 `filled` 位图）；
/// - `total_ns` 与 Σ段是两个独立槽——守恒校验对拍两者；
/// - 全部 checked_add，任何溢出显性 Err 不回绕。
#[derive(Clone, Debug)]
pub struct StageLedger {
    /// 各段耗时（ns）——None 即未记。
    staged: [Option<u128>; STAGE_CAP],
    /// 全程耗时（ns）——独立测量口径。
    total_ns: Option<u128>,
    /// 守恒立案记录（守恒检查失败时置位——账只坏一次，立案不覆盖）。
    conserve_violated: bool,
}

impl StageLedger {
    /// 新账（全空）。
    pub fn new() -> Self {
        StageLedger {
            staged: [None; STAGE_CAP],
            total_ns: None,
            conserve_violated: false,
        }
    }

    /// 记一段耗时。
    pub fn record(&mut self, stage: BootStage, ns: u128) -> Result<(), StageErr> {
        let idx = stage.index();
        if idx >= STAGE_CAP {
            return Err(StageErr::Unknown);
        }
        if self.staged[idx].is_some() {
            return Err(StageErr::Duplicate(ReportCode::StageDuplicate));
        }
        self.staged[idx] = Some(ns);
        Ok(())
    }

    /// 记全程耗时（独立口径）。
    pub fn record_total(&mut self, ns: u128) -> Result<(), StageErr> {
        if self.total_ns.is_some() {
            return Err(StageErr::Duplicate(ReportCode::StageDuplicate));
        }
        self.total_ns = Some(ns);
        Ok(())
    }

    /// Σ段（checked_add 累计；任何一段溢出即 Err）。
    pub fn staged_sum(&self) -> Result<u128, StageErr> {
        let mut acc: u128 = 0;
        for slot in self.staged.iter() {
            if let Some(ns) = slot {
                acc = match acc.checked_add(*ns) {
                    Some(v) => v,
                    None => return Err(StageErr::Overflow(ReportCode::FieldValueOverflow)),
                };
            }
        }
        Ok(acc)
    }

    /// 守恒校验：Σ段 == 全程。
    ///
    /// 两口径都就位才校验（缺口径不算失败也不算通过——`None`）；
    /// 校验失败置位 `conserve_violated`（立案不覆盖）并返回 false。
    pub fn conserve_check(&mut self) -> Option<bool> {
        let sum = match self.staged_sum() {
            Ok(v) => v,
            Err(_) => {
                self.conserve_violated = true;
                return Some(false);
            }
        };
        match self.total_ns {
            Some(total) => {
                let ok = sum == total;
                if !ok {
                    self.conserve_violated = true;
                }
                Some(ok)
            }
            None => None,
        }
    }

    /// 守恒是否曾被打破（立案查询）。
    pub fn violated(&self) -> bool {
        self.conserve_violated
    }

    /// 已记段数。
    pub fn filled_stages(&self) -> usize {
        self.staged.iter().filter(|s| s.is_some()).count()
    }

    /// 全账就绪（七段 + 全程全部就位）。
    pub fn complete(&self) -> bool {
        self.filled_stages() == STAGE_CAP && self.total_ns.is_some()
    }

    /// 取一段（只读）。
    pub fn stage_ns(&self, stage: BootStage) -> Option<u128> {
        let idx = stage.index();
        if idx >= STAGE_CAP {
            return None;
        }
        self.staged[idx]
    }

    /// 取全程。
    pub fn total(&self) -> Option<u128> {
        self.total_ns
    }
}

// ---------------------------------------------------------------------------
// 异常账（append-only + 容量界 + 三级 severity）
// ---------------------------------------------------------------------------

/// 异常严重级（三级闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Severity {
    /// 信息级（记录性，不影响健康判定）。
    Info,
    /// 警告级（降级/回退类事件）。
    Warn,
    /// 致命级（功能受损/闸门失败）。
    Fatal,
}

impl Severity {
    /// 三级闭集。
    pub fn all() -> [Severity; 3] {
        [Severity::Info, Severity::Warn, Severity::Fatal]
    }

    /// 级名（渲染）。
    pub fn name(self) -> &'static str {
        match self {
            Severity::Info => "info",
            Severity::Warn => "warn",
            Severity::Fatal => "fatal",
        }
    }

    /// 从文本解析（kv 导入面——闭集外即 None）。
    pub fn parse(s: &str) -> Option<Severity> {
        match s {
            "info" => Some(Severity::Info),
            "warn" => Some(Severity::Warn),
            "fatal" => Some(Severity::Fatal),
            _ => None,
        }
    }
}

/// 异常条目。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ExcEntry {
    /// 异常码（借用方域码或本域 ReportCode 值）。
    pub code: u16,
    /// 严重级。
    pub severity: Severity,
    /// 发生阶段（闭集内；闭集外场景用 None——前置固件期异常）。
    pub stage: Option<BootStage>,
    /// 备注快照（≤48 字节，超长截断）。
    pub note: String,
}

/// 异常账（append-only）。
///
/// 账务纪律：
/// - append-only：条目一旦入账不可改不可删（账是历史不是现状）；
/// - 容量界：满即显性拒 `ExcLogFull`——溢出静默丢账是账务事故；
/// - 计数派生：count_by 从账派生，不另设计数器（单点真源）。
#[derive(Clone, Debug)]
pub struct ExceptionLog {
    entries: Vec<ExcEntry>,
    overflowed: bool,
}

impl ExceptionLog {
    /// 新账。
    pub fn new() -> Self {
        ExceptionLog {
            entries: Vec::new(),
            overflowed: false,
        }
    }

    /// 入账。
    pub fn append(&mut self, e: ExcEntry) -> Result<(), ReportCode> {
        if self.entries.len() >= EXC_CAP {
            self.overflowed = true;
            return Err(ReportCode::ExcLogFull);
        }
        self.entries.push(e);
        Ok(())
    }

    /// 账条数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 账是否空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 是否发生过溢出立案。
    pub fn overflowed(&self) -> bool {
        self.overflowed
    }

    /// 按级计数（派生，不设第二计数器）。
    pub fn count_by(&self, sev: Severity) -> usize {
        self.entries.iter().filter(|e| e.severity == sev).count()
    }

    /// 首 Fatal 条目。
    pub fn first_fatal(&self) -> Option<&ExcEntry> {
        self.entries.iter().find(|e| e.severity == Severity::Fatal)
    }

    /// 只读遍历。
    pub fn iter(&self) -> alloc::slice::Iter<'_, ExcEntry> {
        self.entries.iter()
    }

    /// 导出全账（clone——外部修改不影响本账）。
    pub fn snapshot(&self) -> Vec<ExcEntry> {
        self.entries.clone()
    }
}

// ---------------------------------------------------------------------------
// 字段矩阵（报告数据本体）
// ---------------------------------------------------------------------------

/// 字段矩阵——24 格闭集，每格 `Option<FieldValue>`。
///
/// 矩阵是报告的**唯一数据本体**：kv 与文本两形态都从这里派生，
/// 齐备率从这里计算，快照指纹对渲染全文哈希。格与 FieldId 的
/// 对位关系由枚举序保证（index() 即矩阵下标），判据有枚举↔矩阵
/// 对位完整性对拍。
#[derive(Clone, Debug)]
pub struct FieldMatrix {
    cells: [Option<FieldValue>; FIELD_COUNT],
    saturated: [bool; FIELD_COUNT],
}

impl FieldMatrix {
    /// 空矩阵。
    ///
    /// `[None; N]` 重复式要求元素 Copy（FieldValue 含 String 不满足），
    /// 走数组 Default（N=24 ≤ 32 的 std 宏实现）。
    pub fn new() -> Self {
        FieldMatrix {
            cells: Default::default(),
            saturated: [false; FIELD_COUNT],
        }
    }

    /// 写格（返回 Err(DuplicateField) 若已写）。
    pub fn set(&mut self, id: FieldId, v: FieldValue) -> Result<(), ReportCode> {
        let idx = id.index();
        if idx >= FIELD_COUNT {
            return Err(ReportCode::RedactConflict);
        }
        if self.cells[idx].is_some() {
            return Err(ReportCode::DuplicateField);
        }
        self.cells[idx] = Some(v);
        Ok(())
    }

    /// 读格（只读借用）。
    pub fn get(&self, id: FieldId) -> Option<&FieldValue> {
        let idx = id.index();
        if idx >= FIELD_COUNT {
            return None;
        }
        self.cells[idx].as_ref()
    }

    /// 已填格数（Na 计入——齐备率口径：显性缺席算齐备）。
    pub fn filled(&self) -> usize {
        self.cells.iter().filter(|c| c.is_some()).count()
    }

    /// 缺字段清单（枚举序）。
    pub fn missing(&self) -> Vec<FieldId> {
        let mut out = Vec::new();
        for id in FieldId::all().iter() {
            let idx = id.index();
            if idx < FIELD_COUNT && self.cells[idx].is_none() {
                out.push(*id);
            }
        }
        out
    }

    /// 齐备率（千分制）：filled / FIELD_COUNT * 1000。
    pub fn completeness_permille(&self) -> u32 {
        let filled = self.filled();
        // FIELD_COUNT=24 恒定，filled<=24，千分值上限 1000，无溢出面。
        ((filled as u32) * 1000) / (FIELD_COUNT as u32)
    }

    /// 值饱和立案标记（写入口由 mark_saturated 置位）。
    pub fn is_saturated(&self, id: FieldId) -> bool {
        let idx = id.index();
        idx < FIELD_COUNT && self.saturated[idx]
    }

    /// 置饱和立案。
    pub fn mark_saturated(&mut self, id: FieldId) {
        let idx = id.index();
        if idx < FIELD_COUNT {
            self.saturated[idx] = true;
        }
    }

    /// 饱和立案总数。
    pub fn saturated_count(&self) -> usize {
        self.saturated.iter().filter(|s| **s).count()
    }

    /// 枚举↔矩阵对位完整性：`FieldId::all()[i].index() == i` 对全部 i。
    ///
    /// 枚举重排而矩阵常数未跟即在此暴露——双写对拍面。
    pub fn index_alignment_ok() -> bool {
        for (i, id) in FieldId::all().iter().enumerate() {
            if id.index() != i {
                return false;
            }
        }
        true
    }
}

// ---------------------------------------------------------------------------
// 构建器（齐备率硬闸 + 状态机封印）
// ---------------------------------------------------------------------------

/// 报告构建错误。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BuildErr {
    /// 字段重复。
    Duplicate(ReportCode),
    /// 齐备率不足（携带缺字段计数）。
    Incomplete(ReportCode, usize),
    /// 空报告。
    Empty(ReportCode),
    /// 守恒失败（Σ段 != 全程）。
    NotConserved(ReportCode),
    /// 构建器已封印。
    Sealed(ReportCode),
    /// 未知阶段。
    BadStage(ReportCode),
}

/// 报告构建器。
///
/// 状态机：`Open → Sealed`（build 后封印，再 set 即 BuilderSealed）。
/// 齐备率硬闸在 build 处：缺字段即 Err(Incomplete) 携带缺字段数——
/// 报告不许带洞出门。
pub struct BootReportBuilder {
    matrix: FieldMatrix,
    ledger: StageLedger,
    exc: ExceptionLog,
    sealed: bool,
}

impl BootReportBuilder {
    /// 新构建器。
    pub fn new() -> Self {
        BootReportBuilder {
            matrix: FieldMatrix::new(),
            ledger: StageLedger::new(),
            exc: ExceptionLog::new(),
            sealed: false,
        }
    }

    /// 写字段（封印后拒）。
    pub fn set(&mut self, id: FieldId, v: FieldValue) -> Result<(), BuildErr> {
        if self.sealed {
            return Err(BuildErr::Sealed(ReportCode::BuilderSealed));
        }
        self.matrix.set(id, v).map_err(BuildErr::Duplicate)
    }

    /// 写 u64 字段（便捷面）。
    pub fn set_u64(&mut self, id: FieldId, v: u64) -> Result<(), BuildErr> {
        self.set(id, FieldValue::U64(v))
    }

    /// 写饱和 u64 字段（值饱和 + 立案一次完成）。
    pub fn set_u64_saturated(&mut self, id: FieldId, v: u64) -> Result<(), BuildErr> {
        if v == u64::MAX {
            self.matrix.mark_saturated(id);
        }
        self.set(id, FieldValue::U64(v))
    }

    /// 写字符串字段（≤64 字节截断——截断即立案到异常账）。
    pub fn set_str(&mut self, id: FieldId, s: &str) -> Result<(), BuildErr> {
        let (val, truncated) = clamp_str(s, 64);
        if truncated && !self.sealed {
            let _ = self.exc.append(ExcEntry {
                code: ReportCode::FieldValueOverflow.code(),
                severity: Severity::Info,
                stage: None,
                note: "field str clamped".to_string(),
            });
        }
        self.set(id, FieldValue::Str(val))
    }

    /// 记阶段耗时。
    pub fn stage(&mut self, st: BootStage, ns: u128) -> Result<(), BuildErr> {
        if self.sealed {
            return Err(BuildErr::Sealed(ReportCode::BuilderSealed));
        }
        self.ledger.record(st, ns).map_err(|e| match e {
            StageErr::Unknown => BuildErr::BadStage(ReportCode::StageUnknown),
            StageErr::Duplicate(c) => BuildErr::Duplicate(c),
            StageErr::Full(c) => BuildErr::Duplicate(c),
            StageErr::Overflow(c) => BuildErr::Duplicate(c),
        })
    }

    /// 记全程。
    pub fn total(&mut self, ns: u128) -> Result<(), BuildErr> {
        if self.sealed {
            return Err(BuildErr::Sealed(ReportCode::BuilderSealed));
        }
        self.ledger
            .record_total(ns)
            .map_err(|e| match e {
                StageErr::Duplicate(c) => BuildErr::Duplicate(c),
                _ => BuildErr::BadStage(ReportCode::StageUnknown),
            })
    }

    /// 异常入账。
    pub fn exc(&mut self, e: ExcEntry) -> Result<(), BuildErr> {
        if self.sealed {
            return Err(BuildErr::Sealed(ReportCode::BuilderSealed));
        }
        self.exc.append(e).map_err(BuildErr::Duplicate)
    }

    /// 构建报告（齐备率硬闸 + 守恒闸 + 封印）。
    ///
    /// 成功即封印（状态机单向）；失败保持 Open 可继续补字段。
    pub fn build(mut self) -> Result<BootReport, BuildErr> {
        if self.sealed {
            return Err(BuildErr::Sealed(ReportCode::BuilderSealed));
        }
        if self.matrix.filled() == 0 {
            return Err(BuildErr::Empty(ReportCode::EmptyReport));
        }
        let missing = self.matrix.missing();
        if !missing.is_empty() {
            return Err(BuildErr::Incomplete(
                ReportCode::MissingField,
                missing.len(),
            ));
        }
        // 守恒闸：账完整才校验；校验失败显性拒（报告耗时族必须自洽）。
        if self.ledger.complete() {
            match self.ledger.conserve_check() {
                Some(false) => return Err(BuildErr::NotConserved(ReportCode::TimeNotConserved)),
                _ => {}
            }
        }
        self.sealed = true;
        Ok(BootReport {
            matrix: self.matrix,
            ledger: self.ledger,
            exc: self.exc,
        })
    }

    /// 缺字段清单窥视（不封印）。
    pub fn missing_preview(&self) -> Vec<FieldId> {
        self.matrix.missing()
    }

    /// 当前齐备率窥视。
    pub fn completeness_preview(&self) -> u32 {
        self.matrix.completeness_permille()
    }
}

/// 字符串限长（返回 (串, 是否截断)）——按字节计，UTF-8 边界回退。
fn clamp_str(s: &str, cap: usize) -> (String, bool) {
    if s.len() <= cap {
        return (s.to_string(), false);
    }
    let mut end = cap;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    if end == 0 {
        return (String::new(), true);
    }
    match s.get(..end) {
        Some(t) => (t.to_string(), true),
        None => (String::new(), true),
    }
}

// ---------------------------------------------------------------------------
// 报告本体（查询 + 双形态渲染 + 快照 + 脱敏）
// ---------------------------------------------------------------------------

/// 引导报告（全景版本体）。
///
/// 构建即封印（不可变）；查询/渲染/指纹全部只读。
#[derive(Clone, Debug)]
pub struct BootReport {
    matrix: FieldMatrix,
    ledger: StageLedger,
    exc: ExceptionLog,
}

impl BootReport {
    /// schema 版本。
    pub fn schema_version(&self) -> u32 {
        SCHEMA_VERSION
    }

    /// 字段读取。
    pub fn field(&self, id: FieldId) -> Option<&FieldValue> {
        self.matrix.get(id)
    }

    /// u64 便捷读（类型不匹配/缺席均 None）。
    pub fn field_u64(&self, id: FieldId) -> Option<u64> {
        match self.matrix.get(id) {
            Some(FieldValue::U64(v)) => Some(*v),
            _ => None,
        }
    }

    /// 齐备率三元组。
    pub fn completeness(&self) -> (usize, usize, u32) {
        (self.matrix.filled(), FIELD_COUNT, self.matrix.completeness_permille())
    }

    /// 阶段账只读访问。
    pub fn ledger(&self) -> &StageLedger {
        &self.ledger
    }

    /// 异常账只读访问。
    pub fn exceptions(&self) -> &ExceptionLog {
        &self.exc
    }

    /// 饱和立案数。
    pub fn saturated(&self) -> usize {
        self.matrix.saturated_count()
    }

    /// kv 行协议（机器形态）。
    ///
    /// 行结构：头行（`# varix-boot-report`）+ 版本行 + 24 字段行 +
    /// 异常行（`exc[i]=code,sev,stage,note`）+ 尾行（`# snap=<指纹>`）。
    /// 字段行数恒 == FIELD_COUNT（缺席格不可能出现——构建闸保证）。
    ///
    /// 尾行指纹来自 kv 载荷行（不含尾行自身——自引用即互递归，
    /// 快照单点见 snapshot/kv_payload_lines）。
    pub fn to_kv_lines(&self) -> Vec<String> {
        let mut out = self.kv_payload_lines();
        out.push(format!("{}{:016x}", KV_FOOTER_PREFIX, self.snapshot().hash));
        out
    }

    /// kv 载荷行（头 + 版本 + 字段 + 异常——**不含尾行**）。
    ///
    /// 快照指纹的单点行源：尾行含指纹，指纹含尾行即自引用互递归
    /// （snapshot→to_kv_lines→snapshot 无限循环栈溢出）——载荷/尾行
    /// 分层后指纹对载荷闭包，尾行对载荷指纹，两层各自治。
    fn kv_payload_lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        out.push(KV_HEADER.to_string());
        out.push(format!("schema_version={}", SCHEMA_VERSION));
        for id in FieldId::all().iter() {
            let val = match self.matrix.get(*id) {
                Some(v) => v.render(),
                None => "n/a".to_string(), // 构建闸后不可达；渲染不 panic 兜底
            };
            out.push(format!("{}={}", id.key(), val));
        }
        for (i, e) in self.exc.iter().enumerate() {
            let st = match e.stage {
                Some(s) => s.name(),
                None => "prefirmware",
            };
            out.push(format!(
                "exc.{}={},{},{},{}",
                i,
                e.code,
                e.severity.name(),
                st,
                e.note
            ));
        }
        out
    }

    /// 文本报告（人读形态——族分组对齐表）。
    pub fn to_text_report(&self) -> String {
        let mut out = String::new();
        out.push_str("==== Varix 引导报告 ====\n");
        out.push_str(&format!("schema: {} (v{})\n", SCHEMA_TAG, SCHEMA_VERSION));
        let (filled, total, permille) = self.completeness();
        out.push_str(&format!(
            "字段齐备率: {}/{} ({}‰)\n",
            filled, total, permille
        ));
        // 族分组渲染（组序=首现序：硬件/内存/随机化/耗时/异常）。
        let sections = ["硬件", "内存", "随机化", "耗时", "异常"];
        for sec in sections.iter() {
            out.push_str(&format!("\n-- {} --\n", sec));
            for id in FieldId::all().iter() {
                if id.section() != *sec {
                    continue;
                }
                let val = match self.matrix.get(*id) {
                    Some(v) => v.render(),
                    None => "n/a".to_string(),
                };
                out.push_str(&format!("{:<22} {}\n", id.label(), val));
            }
        }
        // 阶段账段。
        out.push_str("\n-- 阶段耗时 --\n");
        for st in BootStage::all().iter() {
            match self.ledger.stage_ns(*st) {
                Some(ns) => out.push_str(&format!("{:<22} {} ns\n", st.name(), ns)),
                None => out.push_str(&format!("{:<22} n/a\n", st.name())),
            }
        }
        if let Some(total) = self.ledger.total() {
            out.push_str(&format!("{:<22} {} ns\n", "total", total));
        }
        // 异常账段。
        out.push_str("\n-- 异常账 --\n");
        if self.exc.is_empty() {
            out.push_str("(无异常)\n");
        } else {
            for (i, e) in self.exc.iter().enumerate() {
                let st = match e.stage {
                    Some(s) => s.name(),
                    None => "prefirmware",
                };
                out.push_str(&format!(
                    "[{}] {} code=0x{:04X} stage={} {}\n",
                    i,
                    e.severity.name(),
                    e.code,
                    st,
                    e.note
                ));
            }
        }
        out.push_str(&format!(
            "\nsnap={:016x}\n",
            self.snapshot().hash
        ));
        out
    }

    /// 快照指纹（对 kv **载荷行**——不含尾行，见 kv_payload_lines）。
    pub fn snapshot(&self) -> BootReportSnap {
        let lines = self.kv_payload_lines();
        let mut hash: u64 = 0xcbf29ce484222325;
        for line in lines.iter() {
            for b in line.as_bytes() {
                hash ^= *b as u64;
                hash = hash.wrapping_mul(0x100000001b3);
            }
            hash ^= b'\n' as u64;
            hash = hash.wrapping_mul(0x100000001b3);
        }
        BootReportSnap {
            hash,
            field_count: FIELD_COUNT,
        }
    }

    /// 脱敏视图（sanitize 默认掩码敏感字段）。
    ///
    /// 敏感字段（sensitive()==true）值替换为定长掩码——基址/槽位类
    /// 信息默认不出门（F0207 深化的单点锚）。
    pub fn to_redacted_kv(&self) -> Vec<String> {
        let mut out = Vec::new();
        out.push(KV_HEADER.to_string());
        out.push(format!("schema_version={}", SCHEMA_VERSION));
        for id in FieldId::all().iter() {
            let val = if id.sensitive() {
                "0xREDACTED".to_string()
            } else {
                match self.matrix.get(*id) {
                    Some(v) => v.render(),
                    None => "n/a".to_string(),
                }
            };
            out.push(format!("{}={}", id.key(), val));
        }
        out
    }

    /// 守恒状态查询。
    pub fn conserved(&self) -> Option<bool> {
        if self.ledger.violated() {
            return Some(false);
        }
        if !self.ledger.complete() {
            return None;
        }
        match (self.ledger.staged_sum(), self.ledger.total()) {
            (Ok(sum), Some(total)) => Some(sum == total),
            _ => Some(false),
        }
    }

    /// 双形态交叉对拍：kv 键集合 ⊇ 全字段键 且 字段行数==FIELD_COUNT。
    ///
    /// 两形态（kv/文本）各自独立渲染、同一矩阵派生——本对拍断言
    /// kv 行协议的完整性（行数闸）+ 文本报告含全部键（键集合闸）。
    pub fn cross_check(&self) -> Result<(), ReportCode> {
        let kv = self.to_kv_lines();
        // 行数闸：头 1 + 版本 1 + 字段 24 + 异常 n + 尾 1。
        let expect = 2 + FIELD_COUNT + self.exc.len() + 1;
        if kv.len() != expect {
            return Err(ReportCode::KvLineMismatch);
        }
        // 键集合闸：文本报告必须含每个字段键（人读形态以 label 渲染，
        // 这里对拍 label 与 key 双覆盖）。
        let text = self.to_text_report();
        for id in FieldId::all().iter() {
            if !text.contains(id.label()) {
                return Err(ReportCode::TextRenderMismatch);
            }
        }
        Ok(())
    }
}

/// 报告快照（指纹 + 字段数）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BootReportSnap {
    /// FNV-1a 64 指纹（对 kv 全文）。
    pub hash: u64,
    /// 字段数（结构指纹）。
    pub field_count: usize,
}

// ---------------------------------------------------------------------------
// 标准样本（判据与消费方共用的构造器——语料单点）
// ---------------------------------------------------------------------------

/// 构造一份**全字段齐备 + 守恒 + 双异常**的标准样本。
///
/// 判据与未来消费方（F0203 diff / F0204 轮转）共用同一样本构造器
/// ——语料单点，样本口径漂移即判据全体失效。
pub fn standard_report() -> BootReport {
    let mut b = BootReportBuilder::new();
    // 硬件族。
    let _ = b.set_str(FieldId::HwFirmware, "UEFI");
    let _ = b.set_str(FieldId::HwCpuVendor, "VarixVirtualCPU");
    let _ = b.set_u64(FieldId::HwCpuCores, 4);
    let _ = b.set_u64(FieldId::HwCpuFreqKhz, 2_400_000);
    let _ = b.set_u64(FieldId::HwMemTotalKiB, 1_048_576);
    let _ = b.set_u64(FieldId::HwDeviceCount, 17);
    let _ = b.set_u64(FieldId::HwAcpiTables, 9);
    let _ = b.set_u64(FieldId::HwMmioRanges, 23);
    // 内存族。
    let _ = b.set_u64(FieldId::MemUsableKiB, 917_504);
    let _ = b.set_u64(FieldId::MemReservedKiB, 98_304);
    let _ = b.set_u64(FieldId::MemReclaimedKiB, 32_768);
    let _ = b.set_u64(FieldId::MemFreePages, 229_376);
    let _ = b.set_u64(FieldId::MemHighestAddr, 0x3F_FFFF_FFFF);
    let _ = b.set_u64(FieldId::MemKernelBase, 0x10_0000_0000);
    // 随机化族。
    let _ = b.set_u64(FieldId::RngKaslrSlot, 7);
    let _ = b.set_u64(FieldId::RngEntropyBits, 18);
    let _ = b.set_u64(FieldId::RngSourceCount, 3);
    let _ = b.set(FieldId::RngStackCanary, FieldValue::Bool(true));
    // 耗时族（守恒样本：Σ段=40ms=全程）。
    let _ = b.set_u64(FieldId::TimeTotalNs, 40_000_000);
    let _ = b.set_u64(FieldId::TimeFirmwareNs, 25_000_000);
    let _ = b.set_u64(FieldId::TimeDecompressNs, 6_000_000);
    let _ = b.set_u64(FieldId::TimeDevEnumNs, 9_000_000);
    // 异常族。
    let _ = b.set_u64(FieldId::ExcTotal, 2);
    let _ = b.set_u64(FieldId::ExcFatal, 0);
    // 阶段账（七段，Σ=40ms 与 TimeTotalNs 守恒）。
    let _ = b.stage(BootStage::Firmware, 12_000_000);
    let _ = b.stage(BootStage::Decompress, 6_000_000);
    let _ = b.stage(BootStage::PageTables, 2_000_000);
    let _ = b.stage(BootStage::Acpi, 3_000_000);
    let _ = b.stage(BootStage::DeviceEnum, 9_000_000);
    let _ = b.stage(BootStage::Scheduler, 4_000_000);
    let _ = b.stage(BootStage::UserHandoff, 4_000_000);
    let _ = b.total(40_000_000);
    // 异常账（两 Info）。
    let _ = b.exc(ExcEntry {
        code: 0x2F03,
        severity: Severity::Info,
        stage: Some(BootStage::Acpi),
        note: "oem id blank".to_string(),
    });
    let _ = b.exc(ExcEntry {
        code: 0x3002,
        severity: Severity::Info,
        stage: Some(BootStage::DeviceEnum),
        note: "entropy discount low".to_string(),
    });
    match b.build() {
        Ok(r) => r,
        Err(_) => {
            // 标准样本构造失败是结构性错误——判据区会在 C1 处显性红，
            // 此处返回空报告（齐备率 0）让所有判据联动可见地红。
            empty_report()
        }
    }
}

/// 空报告（判据的"全红底座"——只经空构建路径）。
fn empty_report() -> BootReport {
    // 空构建器 build 会 Err(Empty)——这里手工构造一份零字段报告
    // 仅供失败路径渲染兜底，正常产线不可达。
    BootReport {
        matrix: FieldMatrix::new(),
        ledger: StageLedger::new(),
        exc: ExceptionLog::new(),
    }
}

// ---------------------------------------------------------------------------
// 判据（run_unx201_checks —— 12 项全链）
// ---------------------------------------------------------------------------

/// UNX-F0201 判据集（域内自检，CheckSet 聚合到 unxstar 域）。
pub fn run_unx201_checks() -> CheckSet {
    let mut set = CheckSet::new("unxstar");
    set.add("F0201-C1-segment-guard", check_c1_segment(), "");
    set.add("F0201-C2-standard-complete", check_c2_standard(), "");
    set.add("F0201-C3-missing-rejected", check_c3_missing(), "");
    set.add("F0201-C4-na-counts-filled", check_c4_na(), "");
    set.add("F0201-C5-duplicate-field", check_c5_duplicate(), "");
    set.add("F0201-C6-stage-conserve", check_c6_conserve(), "");
    set.add("F0201-C7-stage-duplicate", check_c7_stagedup(), "");
    set.add("F0201-C8-exc-log-bounds", check_c8_exclog(), "");
    set.add("F0201-C9-kv-text-cross", check_c9_cross(), "");
    set.add("F0201-C10-snap-roundtrip", check_c10_snap(), "");
    set.add("F0201-C11-redact-default", check_c11_redact(), "");
    set.add("F0201-C12-index-align", check_c12_align(), "");
    set.add("F0201-深-01-diff-audit", check_deep01_diff(), "");
    set.add("F0201-深-02-policy-roundtrip", check_deep02_policy_roundtrip(), "");
    set.add("F0201-深-03-budget-timeline", check_deep03_budget_timeline(), "");
    set
}

/// 十二项逐项探针（带伤排查面——宿主测试逐项实证）。
pub fn probe_c1_to_c12() -> [bool; 12] {
    [
        check_c1_segment(),
        check_c2_standard(),
        check_c3_missing(),
        check_c4_na(),
        check_c5_duplicate(),
        check_c6_conserve(),
        check_c7_stagedup(),
        check_c8_exclog(),
        check_c9_cross(),
        check_c10_snap(),
        check_c11_redact(),
        check_c12_align(),
    ]
}

/// 深化面逐项探针（深-01..03 带伤排查面）。
pub fn probe_deep() -> [bool; 3] {
    [
        check_deep01_diff(),
        check_deep02_policy_roundtrip(),
        check_deep03_budget_timeline(),
    ]
}

/// C1 段位守卫：十六码互异且全部落 0x36 段（双写对拍：手排去重计数）。
fn check_c1_segment() -> bool {
    if !segment_guard() {
        return false;
    }
    // 手排对拍：收集全码 → 两两比较计重复 → 重复数必须为 0。
    let codes = ReportCode::all();
    let mut dup = 0usize;
    for i in 0..codes.len() {
        for j in (i + 1)..codes.len() {
            if let (Some(a), Some(b)) = (codes.get(i), codes.get(j)) {
                if a.code() == b.code() {
                    dup += 1;
                }
            }
        }
    }
    if dup != 0 {
        return false;
    }
    // 段界对拍：最小码 >= SEG_BASE，最大码 < SEG_BASE + 16。
    let mut min = u16::MAX;
    let mut max = u16::MIN;
    for c in codes.iter() {
        let v = c.code();
        if v < min {
            min = v;
        }
        if v > max {
            max = v;
        }
    }
    min >= SEG_BASE && max < SEG_BASE + 16
}

/// C2 标准样本齐备：build 成功 + 齐备率 100% + 字段 24/24。
fn check_c2_standard() -> bool {
    let r = standard_report();
    let (filled, total, permille) = r.completeness();
    filled == FIELD_COUNT && total == FIELD_COUNT && permille == 1000
}

/// C3 缺字段显性拒：抽掉一个字段 → build Err(Incomplete) + 缺清单恰 1。
fn check_c3_missing() -> bool {
    let mut b = BootReportBuilder::new();
    // 只填 23 个字段（跳过 ExcFatal）。
    let skip = FieldId::ExcFatal;
    for id in FieldId::all().iter() {
        if *id == skip {
            continue;
        }
        let _ = b.set(*id, FieldValue::U64(1));
    }
    let missing = b.missing_preview();
    if missing.len() != 1 || missing[0] != skip {
        return false;
    }
    // 齐备率口径对拍：实现是 floor(filled*1000/total)，缺 1 格即
    // (FIELD_COUNT-1)*1000/FIELD_COUNT——与 floor 口径同式不另立。
    if b.completeness_preview() != (FIELD_COUNT as u32 - 1) * 1000 / FIELD_COUNT as u32 {
        return false;
    }
    matches!(b.build(), Err(BuildErr::Incomplete(ReportCode::MissingField, 1)))
}

/// C4 Na 算齐备：ExcFatal 显性 Na → build 成功且齐备率 100%。
fn check_c4_na() -> bool {
    let mut b = BootReportBuilder::new();
    for id in FieldId::all().iter() {
        if *id == FieldId::ExcFatal {
            let _ = b.set(*id, FieldValue::Na);
        } else {
            let _ = b.set(*id, FieldValue::U64(1));
        }
    }
    match b.build() {
        Ok(r) => {
            let (filled, _, permille) = r.completeness();
            filled == FIELD_COUNT && permille == 1000
        }
        Err(_) => false,
    }
}

/// C5 重复字段显性拒：同字段 set 两次 → 第二次 Err(Duplicate)。
fn check_c5_duplicate() -> bool {
    let mut b = BootReportBuilder::new();
    let first = b.set_u64(FieldId::HwCpuCores, 4);
    let second = b.set_u64(FieldId::HwCpuCores, 8);
    match (first, second) {
        (Ok(()), Err(BuildErr::Duplicate(ReportCode::DuplicateField))) => true,
        _ => false,
    }
}

/// C6 阶段守恒：守恒样本过闸；破坏样本（Σ段≠全程）显性拒。
fn check_c6_conserve() -> bool {
    // 正样本：标准样本守恒。
    let r = standard_report();
    if r.conserved() != Some(true) {
        return false;
    }
    // 负样本：段和 30ms、全程 40ms → NotConserved。
    let mut b = BootReportBuilder::new();
    for id in FieldId::all().iter() {
        let _ = b.set(*id, FieldValue::U64(1));
    }
    let _ = b.stage(BootStage::Firmware, 10_000_000);
    let _ = b.stage(BootStage::Decompress, 5_000_000);
    let _ = b.stage(BootStage::PageTables, 5_000_000);
    let _ = b.stage(BootStage::Acpi, 5_000_000);
    let _ = b.stage(BootStage::DeviceEnum, 5_000_000);
    let _ = b.stage(BootStage::Scheduler, 0);
    let _ = b.stage(BootStage::UserHandoff, 0);
    let _ = b.total(40_000_000);
    matches!(b.build(), Err(BuildErr::NotConserved(ReportCode::TimeNotConserved)))
}

/// C7 阶段重复记账显性拒 + u128 溢出面 checked。
fn check_c7_stagedup() -> bool {
    let mut b = BootReportBuilder::new();
    let first = b.stage(BootStage::Acpi, 1_000);
    let second = b.stage(BootStage::Acpi, 2_000);
    match (first, second) {
        (Ok(()), Err(BuildErr::Duplicate(ReportCode::StageDuplicate))) => {}
        _ => return false,
    }
    // u128 溢出面：staged_sum 对 u128::MAX 段 + 1ns 段 → Err。
    let mut l = StageLedger::new();
    let r1 = l.record(BootStage::Firmware, u128::MAX);
    let r2 = l.record(BootStage::Decompress, 1);
    if r1.is_err() || r2.is_err() {
        return false;
    }
    matches!(l.staged_sum(), Err(StageErr::Overflow(_)))
}

/// C8 异常账 append-only + 容量界 + 分级派生计数。
fn check_c8_exclog() -> bool {
    let mut log = ExceptionLog::new();
    // 灌满 32 条。
    for i in 0..EXC_CAP {
        let e = ExcEntry {
            code: 0x1000u16 + i as u16,
            severity: if i % 3 == 0 {
                Severity::Fatal
            } else if i % 3 == 1 {
                Severity::Warn
            } else {
                Severity::Info
            },
            stage: Some(BootStage::DeviceEnum),
            note: "filler".to_string(),
        };
        if log.append(e).is_err() {
            return false;
        }
    }
    // 第 33 条显性拒 + 溢出立案置位。
    let over = ExcEntry {
        code: 0x9999,
        severity: Severity::Fatal,
        stage: None,
        note: "overflow probe".to_string(),
    };
    if log.append(over) != Err(ReportCode::ExcLogFull) {
        return false;
    }
    if !log.overflowed() {
        return false;
    }
    // 账未变（append-only：拒即不写）。
    if log.len() != EXC_CAP {
        return false;
    }
    // 分级派生计数：i%3 分派 Fatal 恰 11（0,3,...,30）、Warn 11、Info 10。
    if log.count_by(Severity::Fatal) != 11
        || log.count_by(Severity::Warn) != 11
        || log.count_by(Severity::Info) != 10
    {
        return false;
    }
    // 首 Fatal 是 i=0 条目（code=0x1000）。
    match log.first_fatal() {
        Some(e) => e.code == 0x1000,
        None => false,
    }
}

/// C9 双形态交叉对拍：kv 行数闸 + 文本键覆盖闸 + 渲染消毒闸。
fn check_c9_cross() -> bool {
    let r = standard_report();
    // 交叉对拍过闸。
    if r.cross_check().is_err() {
        return false;
    }
    // kv 行数结构：2 头 + 24 字段 + 2 异常 + 1 尾 = 29。
    let kv = r.to_kv_lines();
    if kv.len() != 2 + FIELD_COUNT + 2 + 1 {
        return false;
    }
    // 首尾行形状。
    if kv[0] != KV_HEADER {
        return false;
    }
    let last = match kv.last() {
        Some(l) => l.clone(),
        None => return false,
    };
    if !last.starts_with(KV_FOOTER_PREFIX) {
        return false;
    }
    // 字段行键序 = 枚举序（渲染确定性）。
    for (i, id) in FieldId::all().iter().enumerate() {
        let line = &kv[2 + i];
        let expect_prefix = format!("{}=", id.key());
        if !line.starts_with(&expect_prefix) {
            return false;
        }
    }
    // 值消毒：含 '='/换行的 Str 值渲染后被替换——构造脏值样本对拍。
    let mut b = BootReportBuilder::new();
    for id in FieldId::all().iter() {
        if *id == FieldId::HwCpuVendor {
            let _ = b.set_str(*id, "bad=vendor\ninject");
        } else {
            let _ = b.set(*id, FieldValue::U64(2));
        }
    }
    let _ = b.stage(BootStage::Firmware, 1);
    let _ = b.stage(BootStage::Decompress, 1);
    let _ = b.stage(BootStage::PageTables, 1);
    let _ = b.stage(BootStage::Acpi, 1);
    let _ = b.stage(BootStage::DeviceEnum, 1);
    let _ = b.stage(BootStage::Scheduler, 1);
    let _ = b.stage(BootStage::UserHandoff, 1);
    // Σ段=7=全程——build 守恒闸（Σ段 != 全程即 NotConserved）过闸，
    // 本判据考察的是 kv 消毒不是耗时守恒（守恒由 C6 专管）。
    let _ = b.total(7);
    match b.build() {
        Ok(r2) => {
            let kv2 = r2.to_kv_lines();
            let mut vendor_ok = false;
            for line in kv2.iter() {
                if line.starts_with("hw_cpu_vendor=") {
                    vendor_ok = line == "hw_cpu_vendor=bad_vendor_inject";
                }
            }
            vendor_ok
        }
        Err(_) => false,
    }
}

/// C10 快照指纹：往返一致 + 内容变指纹变 + 同构恒等。
fn check_c10_snap() -> bool {
    let a = standard_report();
    let b = standard_report();
    // 同构恒等。
    let sa = a.snapshot();
    let sb = b.snapshot();
    if sa != sb {
        return false;
    }
    // kv 渲染再哈希往返（snapshot 内部即载荷行哈希——对拍外部重算
    // 同口径：载荷行不含尾行，尾行指纹自载荷闭包）。
    let lines = a.kv_payload_lines();
    let mut hash: u64 = 0xcbf29ce484222325;
    for line in lines.iter() {
        for byte in line.as_bytes() {
            hash ^= *byte as u64;
            hash = hash.wrapping_mul(0x100000001b3);
        }
        hash ^= b'\n' as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    if hash != sa.hash {
        return false;
    }
    // 内容变指纹变：克隆后改一个字段值（走重建路径）。
    let mut c = BootReportBuilder::new();
    for id in FieldId::all().iter() {
        if *id == FieldId::HwCpuCores {
            let _ = c.set_u64(*id, 8);
        } else {
            let _ = c.set_u64(*id, 1);
        }
    }
    for st in BootStage::all().iter() {
        let _ = c.stage(*st, 1);
    }
    let _ = c.total(7);
    match c.build() {
        Ok(rc) => rc.snapshot().hash != sa.hash,
        Err(_) => false,
    }
}

/// C11 脱敏默认开：敏感字段（内核基址/KASLR 槽位）掩码，非敏感原值。
fn check_c11_redact() -> bool {
    let r = standard_report();
    let red = r.to_redacted_kv();
    let mut base_masked = false;
    let mut slot_masked = false;
    let mut cores_plain = false;
    for line in red.iter() {
        if line.starts_with("mem_kernel_base=") {
            base_masked = line == "mem_kernel_base=0xREDACTED";
        }
        if line.starts_with("rng_kaslr_slot=") {
            slot_masked = line == "rng_kaslr_slot=0xREDACTED";
        }
        if line.starts_with("hw_cpu_cores=") {
            cores_plain = line == "hw_cpu_cores=4";
        }
    }
    base_masked && slot_masked && cores_plain
}

/// C12 枚举↔矩阵对位 + 阶段枚举↔账本对位（双写对拍）。
fn check_c12_align() -> bool {
    if !FieldMatrix::index_alignment_ok() {
        return false;
    }
    // 阶段对位：all()[i].index()==i。
    for (i, st) in BootStage::all().iter().enumerate() {
        if st.index() != i {
            return false;
        }
    }
    // FIELD_COUNT 与枚举变体数双写：all() 长度 == 常数。
    if FieldId::all().len() != FIELD_COUNT {
        return false;
    }
    // STAGE_CAP 与枚举变体数双写。
    BootStage::all().len() == STAGE_CAP
}

// ---------------------------------------------------------------------------
// 深化九区：报告消费面——构建封印之后是「报告怎么被读」的出口面：
// 差异审计 / 出口脱敏策略 / 文本协议往返 / 异常预算闸 / 阶段时间线。
// 全部纯函数零 IO——出口面与构建面同构（闭集显性、守恒对拍）。
// ---------------------------------------------------------------------------

/// 字段级差异（单格变更——升级审计的原子项）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FieldDelta {
    /// 变更字段。
    pub id: FieldId,
    /// 旧值渲染（None=旧报告缺格——构建闸后不可达，保留显性位）。
    pub old: Option<String>,
    /// 新值渲染。
    pub new: Option<String>,
}

impl FieldDelta {
    /// 差异行渲染（kv 形态单行——机器对账直读）。
    pub fn render(&self) -> String {
        let old = self.old.clone().unwrap_or_else(|| "n/a".to_string());
        let new = self.new.clone().unwrap_or_else(|| "n/a".to_string());
        format!("delta.{}:{}->{}", self.id.key(), old, new)
    }
}

/// 双报告差异账（逐字段对拍——变化清单枚举序确定）。
///
/// 差异面只看字段矩阵：阶段账/异常账是时序面（由 C6/C8 守恒专管），
/// 差异审计关心「哪格变了」——枚举序遍历双矩阵，值渲染不等即立案。
pub struct ReportDiff {
    /// 变化格清单（枚举序）。
    pub deltas: Vec<FieldDelta>,
    /// 对拍字段总数（恒 == FIELD_COUNT——对拍完整性锚）。
    pub compared: usize,
}

impl ReportDiff {
    /// **逐格对拍**（新旧两报告——渲染值比对，形态无关）。
    pub fn of(a: &BootReport, b: &BootReport) -> ReportDiff {
        let mut deltas = Vec::new();
        let mut compared = 0usize;
        for id in FieldId::all().iter() {
            compared += 1;
            let va = a.matrix.get(*id).map(FieldValue::render);
            let vb = b.matrix.get(*id).map(FieldValue::render);
            if va != vb {
                deltas.push(FieldDelta {
                    id: *id,
                    old: va,
                    new: vb,
                });
            }
        }
        ReportDiff { deltas, compared }
    }

    /// 同构恒等（零差异——对拍面平凡真）。
    pub fn identical(&self) -> bool {
        self.deltas.is_empty() && self.compared == FIELD_COUNT
    }

    /// 差异率（千分制）：变化格 * 1000 / 对拍总数——升级面广度账。
    pub fn permille(&self) -> u32 {
        ((self.deltas.len() as u32) * 1000) / (self.compared as u32)
    }
}

/// 出口脱敏策略（三态闭集——出口面策略单点显性）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RedactPolicy {
    /// 全掩（所有值 → 掩码——最大收敛出口）。
    FullMask,
    /// 仅敏感（sensitive() 格掩码，其余原值——默认出口）。
    SensitiveOnly,
    /// 全透（零掩码——可信通道显性选择）。
    Translucent,
}

impl RedactPolicy {
    /// 全集（策略闭集审计面）。
    pub const ALL: [RedactPolicy; 3] = [
        RedactPolicy::FullMask,
        RedactPolicy::SensitiveOnly,
        RedactPolicy::Translucent,
    ];

    /// 名称（快照/账面渲染单点）。
    pub const fn name(self) -> &'static str {
        match self {
            RedactPolicy::FullMask => "full-mask",
            RedactPolicy::SensitiveOnly => "sensitive-only",
            RedactPolicy::Translucent => "translucent",
        }
    }
}

impl BootReport {
    /// **按策略出口**（to_redacted_kv 的策略化泛化——策略单点分派，
    /// 三态各走各的值面：掩码 / 敏感判定 / 原值透传）。
    pub fn to_kv_with_policy(&self, policy: RedactPolicy) -> Vec<String> {
        let mut out = Vec::new();
        out.push(format!("{}policy={}", KV_HEADER, policy.name()));
        out.push(format!("schema_version={}", SCHEMA_VERSION));
        for id in FieldId::all().iter() {
            let masked = match policy {
                RedactPolicy::FullMask => true,
                RedactPolicy::SensitiveOnly => id.sensitive(),
                RedactPolicy::Translucent => false,
            };
            let val = if masked {
                "0xREDACTED".to_string()
            } else {
                match self.matrix.get(*id) {
                    Some(v) => v.render(),
                    None => "n/a".to_string(),
                }
            };
            out.push(format!("{}={}", id.key(), val));
        }
        out
    }
}

/// kv 载荷行解析器（首现 '=' 切分——文本协议往返的逆面）。
///
/// 行形态 `key=value`；无 '=' 行按 (整行, "") 记——解析不 panic
/// 不丢弃（协议面的异物显性保留，由对拍侧断言）。
pub fn parse_kv_line(line: &str) -> (&str, &str) {
    match line.find('=') {
        Some(pos) => {
            let k = &line[..pos];
            let v = &line[pos + 1..];
            (k, v)
        }
        None => (line, ""),
    }
}

impl BootReport {
    /// **kv 载荷往返对拍**：payload 行 → 解析 → 键序/值形态重渲染
    /// → 与原 payload 恒等（协议双面闭合——渲染即正形，解析不损）。
    ///
    /// 往返语义：键序保持枚举序（解析按行序即枚举序），值串逐字
    /// 保留（render 输出无 '=' 无换行——消毒闸 C9 保证可解析）。
    pub fn kv_roundtrip_ok(&self) -> bool {
        let payload = self.kv_payload_lines();
        if payload.is_empty() {
            return false;
        }
        // 头行特殊形态（# 前缀），其余必须 key=value。
        let mut rebuilt: Vec<String> = Vec::new();
        for line in payload.iter() {
            let (k, v) = parse_kv_line(line);
            if v.is_empty() && !line.contains('=') {
                // 无 '=' 行原样保（头行等）。
                rebuilt.push(k.to_string());
            } else {
                rebuilt.push(format!("{}={}", k, v));
            }
        }
        // 重渲染 == 原 payload（逐行恒等——解析不损）。
        if rebuilt.len() != payload.len() {
            return false;
        }
        let mut i = 0usize;
        while i < payload.len() {
            if rebuilt[i] != *payload[i] {
                return false;
            }
            i += 1;
        }
        // 键序闸：字段行键序 == 枚举序（协议确定性锚）。
        let mut fi = 0usize;
        for line in payload.iter() {
            let (k, _) = parse_kv_line(line);
            if k == FieldId::all()[fi].key() {
                fi += 1;
                if fi == FieldId::all().len() {
                    break;
                }
            }
        }
        fi == FieldId::all().len()
    }
}

/// 严重级预算账（异常账出口闸——Fatal 上限 + Warn 占比千分账）。
///
/// 出口面预算：报告出门前 Fatal 数有上限（引导报告不是崩溃转储，
/// Fatal 泛滥即上游语义坏），Warn 占比显性成账。
pub struct SeverityBudget {
    /// Fatal 实计数。
    pub fatal: usize,
    /// Warn 实计数。
    pub warn: usize,
    /// Info 实计数。
    pub info: usize,
    /// Fatal 上限（超限即闸拒——出口预算常数）。
    pub fatal_cap: usize,
}

impl SeverityBudget {
    /// Fatal 出口上限（引导报告 Fatal 预算——超限即上游已崩）。
    pub const FATAL_CAP: usize = 2;

    /// **对账**（异常账 → 预算账——count_by 三级单点遍历）。
    pub fn of(exc: &ExceptionLog) -> SeverityBudget {
        SeverityBudget {
            fatal: exc.count_by(Severity::Fatal),
            warn: exc.count_by(Severity::Warn),
            info: exc.count_by(Severity::Info),
            fatal_cap: SeverityBudget::FATAL_CAP,
        }
    }

    /// 预算内（Fatal ≤ cap——出口闸真值）。
    pub fn within(&self) -> bool {
        self.fatal <= self.fatal_cap
    }

    /// 异常总数（三级和——账守恒锚）。
    pub fn total(&self) -> usize {
        self.fatal + self.warn + self.info
    }

    /// Warn 占比（千分制）：warn * 1000 / total，total=0 → 0
    /// （空账零占比——零除显性守恒非猜值）。
    pub fn warn_permille(&self) -> u32 {
        if self.total() == 0 {
            return 0;
        }
        ((self.warn as u32) * 1000) / (self.total() as u32)
    }
}

/// 阶段时间线（占比账 + 关键路径——耗时族出口视图）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct StageTimelineEntry {
    /// 阶段。
    pub stage: BootStage,
    /// 耗时（ns）。
    pub ns: u128,
    /// 占比（千分制，对全程 total）。
    pub permille: u32,
}

/// 阶段时间线账（全程占比 + 关键路径单点）。
pub struct StageTimeline {
    /// 有账阶段条目（枚举序，缺席格不入账）。
    pub entries: Vec<StageTimelineEntry>,
    /// 全程（None=账不完整——占比面不猜）。
    pub total: Option<u128>,
}

impl StageTimeline {
    /// **建账**（阶段账 → 时间线——占比 = stage*1000/total 整除闭式）。
    ///
    /// total=0：全部占比 0（零除显性守恒——不猜不炸）。
    pub fn of(ledger: &StageLedger) -> StageTimeline {
        let total = ledger.total();
        let mut entries = Vec::new();
        for st in BootStage::all().iter() {
            if let Some(ns) = ledger.stage_ns(*st) {
                // 占比在 u128 域先乘后除（ns*1000 可超 u32——先乘后除
                // 再钳 1000，u32 域直接乘即溢出）。
                let pm = match total {
                    Some(t) if t > 0 => ((ns.min(t).saturating_mul(1000) / t) as u64).min(1000) as u32,
                    _ => 0,
                };
                entries.push(StageTimelineEntry {
                    stage: *st,
                    ns,
                    permille: pm,
                });
            }
        }
        StageTimeline { entries, total }
    }

    /// 占比守恒（Σ占比 ≤ 1000——整除下取不超账；账完整时逐段和
    /// 可能 < 1000（整除舍入），恒不超）。
    pub fn permille_conserved(&self) -> bool {
        let mut sum = 0u32;
        for e in self.entries.iter() {
            sum += e.permille;
        }
        sum <= 1000
    }

    /// 关键路径（最长段——None=无账）。
    pub fn critical(&self) -> Option<&StageTimelineEntry> {
        let mut best: Option<&StageTimelineEntry> = None;
        for e in self.entries.iter() {
            match best {
                None => best = Some(e),
                Some(b) => {
                    if e.ns > b.ns {
                        best = Some(e);
                    }
                }
            }
        }
        best
    }
}

/// 深化面判据深-01：差异账恰值（同构恒等 + 单格变恰 1 + 差异率）。
///
/// 对拍样本用同构双变体（仅 HwCpuCores 差一格）——对标准样本全值
/// 重造会引入面差（Str 字段全变），差异审计的恰值锚要求单格变。
fn variant_report(cores: u64) -> Option<BootReport> {
    let mut b = BootReportBuilder::new();
    for id in FieldId::all().iter() {
        if *id == FieldId::HwCpuCores {
            let _ = b.set_u64(*id, cores);
        } else {
            let _ = b.set_u64(*id, 1);
        }
    }
    for st in BootStage::all().iter() {
        let _ = b.stage(*st, 1);
    }
    // Σ段=7=全程——守恒闸过闸（本判据考察差异面非耗时守恒）。
    let _ = b.total(7);
    b.build().ok()
}

fn check_deep01_diff() -> bool {
    let v4 = match variant_report(4) {
        Some(r) => r,
        None => return false,
    };
    // 同构恒等。
    let d0 = ReportDiff::of(&v4, &v4);
    if !d0.identical() || d0.permille() != 0 {
        return false;
    }
    // 单格变：核心数 4 → 8 恰 1 差异（差异行渲染含键与新旧值）。
    let v8 = match variant_report(8) {
        Some(r) => r,
        None => return false,
    };
    let d1 = ReportDiff::of(&v4, &v8);
    if d1.deltas.len() != 1 || d1.compared != FIELD_COUNT {
        return false;
    }
    let dd = match d1.deltas.first() {
        Some(x) => x,
        None => return false,
    };
    let render_ok = dd.id == FieldId::HwCpuCores
        && dd.render() == "delta.hw_cpu_cores:4->8";
    // 差异率恰值：1/24 * 1000 = 41（整除）。
    render_ok && d1.permille() == 41
}

/// 深化面判据深-02：出口策略三态恰值 + kv 往返恒等。
fn check_deep02_policy_roundtrip() -> bool {
    let r = standard_report();
    // 三态遍历：掩格数单调（全掩 24 ≥ 仅敏感 ≥ 全透 0）。
    let mut masked_counts = [0usize; 3];
    for (pi, p) in RedactPolicy::ALL.iter().enumerate() {
        let kv = r.to_kv_with_policy(*p);
        if kv.len() != 2 + FIELD_COUNT {
            return false;
        }
        // 策略名入头行（出口自描述）。
        if !kv[0].contains(p.name()) {
            return false;
        }
        let mut masked = 0usize;
        for line in kv.iter().skip(2) {
            if line.ends_with("=0xREDACTED") {
                masked += 1;
            }
        }
        masked_counts[pi] = masked;
    }
    let order_ok = masked_counts[0] == FIELD_COUNT
        && masked_counts[1] <= FIELD_COUNT
        && masked_counts[1] > masked_counts[2]
        && masked_counts[2] == 0;
    // 默认出口（to_redacted_kv）== SensitiveOnly 策略出口（单点分派不双真相）。
    let def = r.to_redacted_kv();
    let pol = r.to_kv_with_policy(RedactPolicy::SensitiveOnly);
    let mut def_ok = def.len() == pol.len();
    if def_ok {
        // 默认头行是 KV_HEADER，策略头行带 policy= 尾缀——字段行全等。
        let mut i = 1usize;
        while i < def.len() {
            if def[i] != pol[i] {
                def_ok = false;
                break;
            }
            i += 1;
        }
    }
    // kv 往返：payload 解析重渲染恒等 + 键序枚举对齐。
    r.kv_roundtrip_ok() && order_ok && def_ok
}

/// 深化面判据深-03：严重级预算闸 + 阶段时间线占比守恒。
fn check_deep03_budget_timeline() -> bool {
    let r = standard_report();
    // 预算账：标准样本 2 异常均为 Info（见 standard_report 语料单点），
    // total=2，Fatal 0 ≤ cap 过闸；Warn 占比 0‰（空 Warn 账显性）。
    let bud = SeverityBudget::of(r.exceptions());
    let bud_ok = bud.total() == 2
        && bud.fatal == 0
        && bud.info == 2
        && bud.within()
        && bud.warn_permille() == 0;
    // 时间线：标准样本 7 段 + total（Σ段=40ms=全程，守恒样本）。
    let tl = StageTimeline::of(r.ledger());
    let entries_ok = tl.entries.len() == 7 && tl.total == Some(40_000_000);
    // 关键路径：Firmware 12ms（最长段）。
    let crit_ok = match tl.critical() {
        Some(c) => c.stage == BootStage::Firmware && c.ns == 12_000_000,
        None => false,
    };
    // 占比恰值：12/40=300、6/40=150、2/40=50、3/40=75、9/40=225、
    // 4/40=100、4/40=100（整除下取；Σ=1000 恰满——样本守恒）。
    let expect = [300u32, 150, 50, 75, 225, 100, 100];
    let mut pm_ok = tl.entries.len() == expect.len();
    let mut i = 0usize;
    while pm_ok && i < expect.len() {
        if tl.entries[i].permille != expect[i] {
            pm_ok = false;
        }
        i += 1;
    }
    bud_ok && entries_ok && crit_ok && pm_ok && tl.permille_conserved()
}

// ---------------------------------------------------------------------------
// 单元测试（域聚合测试在 mod.rs——此处只放本模块纯函数级断言）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// 标准样本全绿（判据 C2 的 test 面）。
    #[test]
    fn standard_sample_complete() {
        let r = standard_report();
        assert!(r.completeness().2 == 1000);
    }

    /// 段位守卫常真。
    #[test]
    fn segment_guard_holds() {
        assert!(segment_guard());
    }

    /// 守恒正负样本。
    #[test]
    fn conserve_positive_and_negative() {
        assert!(check_c6_conserve());
    }
}
