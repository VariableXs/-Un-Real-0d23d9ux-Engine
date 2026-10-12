//! unx404_hpetcap_deep —— UNX-F0404 · HPET 探测与能力账（只读·深化并存版）。
//!
//! **锚点**：`docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II · Unxreal.md#UNX-F0404`
//!
//! **判据原文**：HPET 探测结果与 ACPI HPET 表对照一致，能力三要素
//! （位宽 32/64/定时器数/周期）双读 2 次一致账齐。
//!
//! **与并行速写版 unx404_hpetcap 的关系**：同目录并存、模块天然隔离，
//! 符号零复用零 `use` 对方；本版为深化版——诊断码段独取 `0x82xx`
//! （全域 grep 确认空闲：0x80/0x83 归他册、0x85~0x8C 归 CGPU vck 册、
//! 0x84 仅有 FNV/汉字巧合常量无段主，0x82 无主），速写版取 0x52xx，
//! 两段互不相犯。
//!
//! ## 深化令逐条落点（本文件章节索引）
//!
//! 1. **ACPI HPET 表解析**：签名 HPET / 表头九字段 / 事件定时器块
//!    （硬件修订 ID / 比较器数 / 计数器位宽标志 / legacy 替换 /
//!    PCI 厂商 ID）/ 基址 64 位 GAS 结构逐字段账（§四）。
//! 2. **能力三要素账**：位宽 32/64 判定（ACPI counter_size_cap 与
//!    GCAP bit12:13 双源对拍）/ 定时器数 = 比较器数 + 1 语义 /
//!    周期 femtoseconds 值域校验（10^15/10^8 Hz = 10^7 fs 下界，
//!    10^15/10^7 Hz = 10^8 fs 上界——10~100 MHz 合理频带）（§五、§六）。
//! 3. **双读一致性**：能力寄存器读 2 次逐字段比对（修订/厂商/位宽/
//!    legacy/周期五字段 + 逐比较器能力字），不一致即立案（§七）。
//! 4. **MMIO 基址账**：ACPI 表基址 vs 探测基址对拍 + 4 KiB 页对齐
//!    校验（§八）。
//! 5. **比较器能力细账**：每比较器 64 位支持 / 周期允许 /
//!    set-accumulate / FSB 中断映射能力 / 路由位图非零性逐项入账
//!    （§九）。
//! 6. **编号唯一性**：比较器编号闭集 0..=31 内互异且连续（§十）。
//! 7. **有效定时器数闭式**：vendorID 字段双读（ACPI PCI 厂商 vs
//!    GCAP 厂商位段）一致则闭式生效，不一致立案取保守 min（§十）。
//! 8. **HPET 使能位只读语义**：探测全程零写面，使能位改动记账为
//!    违规立案（0x8210），写请求显性拒并非静默（§十一）。
//! 9. **频率换算**：周期 fs → Hz 全整数 `10^15 / fs` 商余双记，
//!    零浮点（§六）。
//! 10. **增量重扫对账**：首轮账 FNV 指纹 vs 重扫账指纹对拍，
//!     漂移即 0x8212 立案（§十二）。
//! 11. **启动报告 kv 导出**：探测账全量键值化，诊断项随账导出
//!     （§十四）。
//! 12. **诊断码闭集**：0x8201~0x8216 二十二位命名（段内互异自证），
//!     全域 grep 确认不撞（§一）。
//!
//! ## 纯功能线
//!
//! no_std + alloc；零 `std`；零 `unwrap`/`expect`/`panic!`；错误
//! 全走 `Result<_, HcdCode>` + 中文诊断；HPET 寄存器访问全走可注入
//! trait `HcdMmio`（默认确定性构造口 `HcdDeterministicMmio`，禁真实
//! MMIO）；逻辑 tick `u64` 贯穿（禁墙钟）；全整数算术禁浮点；输入
//! 注入式确定性——同输入序列恒同账。

#![forbid(unsafe_code)]

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 〇、模块级常量
// ---------------------------------------------------------------------------

/// 模块版本（深化版独立命名——与速写版版本号互不相扰）。
pub const HCD_VERSION: &str = "unx404_hpetcap_deep-v1";

/// 诊断段基座（0x82 段 · 本模块独占 · 全域 grep 确认空闲后占用）。
pub const HCD_SEG_BASE: u16 = 0x8200;

/// 诊断段上界（开区间——判码越界自查用）。
pub const HCD_SEG_END: u16 = 0x8300;

/// ACPI HPET 表签名 "HPET"（小端读出 0x54455045）。
pub const HCD_SIG_HPET: u32 = 0x5445_5045;

/// SDT 表头长度：36 字节（签名 4 + 长度 4 + 修订 1 + 校验和 1 +
/// OEM ID 6 + OEM 表 ID 8 + OEM 修订 4 + 创建者 ID 4 + 创建者修订 4）。
pub const HCD_SDT_HEADER_LEN: usize = 36;

/// HPET 事件定时器块偏移（紧跟 36 字节表头之后）。
pub const HCD_EVENT_BLOCK_OFF: usize = 36;

/// HPET 事件定时器块长度：8 字节（修订 1 + 比较器数 1 + 位宽标志 1
/// + 保留 1 + PCI 厂商 ID 2 + 保留 2）。
pub const HCD_EVENT_BLOCK_LEN: usize = 8;

/// ACPI HPET 基址 GAS（Generic Address Structure）长度：12 字节。
pub const HCD_GAS_LEN: usize = 12;

/// 最小合法表长：表头 36 + 事件块 8 + GAS 12 = 56 字节。
pub const HCD_TABLE_MIN_LEN: usize = 56;

/// 单个定时器信息块长度：20 字节（IA-PC HPET 规范 v1.0a §3.2.2——
/// 长度足够性账用）。
pub const HCD_TIMER_BLOCK_LEN: usize = 20;

/// 比较器数上限（规范最少 3 台事件定时器；账面闭集 32 防注入爆账
/// ——0 或超 32 即 0x8206）。
pub const HCD_COMPARATOR_MAX: usize = 32;

/// MMIO 寄存窗口大小：0x400 字节（规范 HPET 寄存器空间 1024 字节，
/// 覆盖定时器块尾 0x100 + 32*16 = 0x400 整）。
pub const HCD_MMIO_WINDOW: u64 = 0x400;

/// 双读轮数（判据点名：双读 2 次——账面闭集常量）。
pub const HCD_DUAL_ROUNDS: usize = 2;

/// 4 KiB 页掩码（MMIO 基址页对齐校验用）。
pub const HCD_PAGE_MASK: u64 = 0xFFF;

/// 判据点名的周期合理频带下界：10 MHz = 10^7 Hz。
pub const HCD_FREQ_BAND_LO_HZ: u64 = 10_000_000;

/// 判据点名的周期合理频带上界：100 MHz = 10^8 Hz。
pub const HCD_FREQ_BAND_HI_HZ: u64 = 100_000_000;

/// 飞秒换算常量：1 秒 = 10^15 fs。
pub const HCD_FS_PER_SEC: u64 = 1_000_000_000_000_000;

/// 周期合理下界 fs：10^15 / 10^8 Hz = 10^7 fs（= 10 ns，100 MHz 档）。
pub const HCD_PERIOD_FS_MIN: u64 = 10_000_000;

/// 周期合理上界 fs：10^15 / 10^7 Hz = 10^8 fs（= 100 ns，10 MHz 档）。
pub const HCD_PERIOD_FS_MAX: u64 = 100_000_000;

// ---------------------------------------------------------------------------
// 一、诊断码闭集（0x8201~0x8216 · 全域 grep 确认独占）
// ---------------------------------------------------------------------------

/// HPET 深化探测诊断码闭集。
///
/// 取码纪律：① 全落 `0x8200..0x8300` 开区间（`hcd_diag_codes_sane`
/// 自查）；② 与速写版 0x52xx 段零交集；③ 每码配中文诊断（`reason`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HcdCode {
    /// 0x8201 表签名不匹配（非 "HPET"）。
    BadSignature = 0x8201,
    /// 0x8202 全表校验和不过（字节和 mod 256 非零）。
    BadChecksum = 0x8202,
    /// 0x8203 表长度越界（小于 56 字节最小面或长度字段自相矛盾）。
    TableLenBad = 0x8203,
    /// 0x8204 事件块硬件修订 ID 为零（固件偷懒可检）。
    HwRevZero = 0x8204,
    /// 0x8205 基址 GAS 结构非法（空间 ID 越闭集/位宽非 64/地址零）。
    GasBad = 0x8205,
    /// 0x8206 比较器数越界（0 或超 32——定时器数 = 比较器数 + 1 落界外）。
    ComparatorCountBad = 0x8206,
    /// 0x8207 能力寄存器双读不一致（GCAP 标量字段任一漂移）。
    CapDualMismatch = 0x8207,
    /// 0x8208 主计数器周期字段双读不一致。
    PeriodDualMismatch = 0x8208,
    /// 0x8209 位宽判定矛盾（ACPI counter_size_cap 与 GCAP 编码对拍不合）。
    WidthConflict = 0x8209,
    /// 0x820A 定时器数语义不符（探测比较器数 ≠ 表比较器数 + 1）。
    TimerCountConflict = 0x820A,
    /// 0x820B 周期值域越界（fs 为 0 或落 10^7..10^8 合理区间外）。
    PeriodOutOfRange = 0x820B,
    /// 0x820C MMIO 基址对拍偏差（ACPI 表基址 ≠ 探测基址）。
    BaseMismatch = 0x820C,
    /// 0x820D MMIO 基址页对齐不过（基址低 12 位非零）。
    BaseNotPageAligned = 0x820D,
    /// 0x820E 比较器能力字双读不一致（任一比较器任一轮漂移）。
    TimerCapDualMismatch = 0x820E,
    /// 0x820F 比较器编号不唯一或越界（闭集 0..=31 外/重复/断档）。
    TimerIndexBad = 0x820F,
    /// 0x8210 使能位写红线违规（探测期出现使能写意图——立案非执行）。
    EnableWriteViolation = 0x8210,
    /// 0x8211 频率换算失守（fs 为零除零防护/商余守恒失配）。
    FreqConvBad = 0x8211,
    /// 0x8212 增量重扫账漂移（重扫指纹与首轮指纹失配）。
    RescanDrift = 0x8212,
    /// 0x8213 vendorID 双读不符（ACPI PCI 厂商 ≠ GCAP 厂商位段）。
    VendorDualMismatch = 0x8213,
    /// 0x8214 MMIO 读越界（偏移落 0x0..0x400 寄存窗口外）。
    MmioReadOob = 0x8214,
    /// 0x8215 状态机非法跃迁（探测阶段未按序推进）。
    PhaseIllegal = 0x8215,
    /// 0x8216 封账时账未齐（报告导出前置项缺一）。
    SealIncomplete = 0x8216,
}

impl HcdCode {
    /// wire 数值。
    pub const fn num(self) -> u16 {
        self as u16
    }

    /// 段内自查：判码必落 0x8200..0x8300 开区间（段越界即账面自爆）。
    pub const fn in_segment(self) -> bool {
        let n = self as u16;
        n >= HCD_SEG_BASE && n < HCD_SEG_END
    }

    /// 人话诊断（中文——直接可入引导报告）。
    pub fn reason(self) -> String {
        match self {
            HcdCode::BadSignature => "HPET 表签名不匹配：目标表头签名非 \"HPET\"，非 HPET 表误入".into(),
            HcdCode::BadChecksum => "HPET 全表校验和不过：全表字节和 mod 256 非零，表体可疑".into(),
            HcdCode::TableLenBad => "HPET 表长度越界：小于 56 字节最小面或长度字段与读面自相矛盾".into(),
            HcdCode::HwRevZero => "HPET 事件块硬件修订 ID 为零：固件未填，表账可信度降级".into(),
            HcdCode::GasBad => "HPET 基址 GAS 非法：地址空间 ID 越闭集 {0,1} 或位宽非 64 或地址为零".into(),
            HcdCode::ComparatorCountBad => "HPET 比较器数越界：0 或超 32，定时器数 = 比较器数 + 1 落闭集外".into(),
            HcdCode::CapDualMismatch => "HPET 能力寄存器双读不一致：GCAP 修订/厂商/位宽/legacy 字段漂移".into(),
            HcdCode::PeriodDualMismatch => "HPET 主计数器周期双读不一致：bit32:63 周期字段两轮读数失配".into(),
            HcdCode::WidthConflict => "HPET 位宽判定矛盾：ACPI counter_size_cap 与 GCAP bit12:13 对拍不合".into(),
            HcdCode::TimerCountConflict => "HPET 定时器数语义不符：探测比较器数 ≠ 表比较器数 + 1".into(),
            HcdCode::PeriodOutOfRange => "HPET 周期值域越界：fs 周期为 0 或落 10^7..10^8 合理频带外".into(),
            HcdCode::BaseMismatch => "HPET MMIO 基址对拍偏差：ACPI 表基址与探测基址不一致".into(),
            HcdCode::BaseNotPageAligned => "HPET MMIO 基址页对齐不过：基址低 12 位非零".into(),
            HcdCode::TimerCapDualMismatch => "HPET 比较器能力字双读不一致：任一比较器能力位两轮读数失配".into(),
            HcdCode::TimerIndexBad => "HPET 比较器编号不唯一或越界：闭集 0..=31 外或重复或断档".into(),
            HcdCode::EnableWriteViolation => "HPET 使能位写红线违规：探测期出现使能写意图，只读语义被试探已拒".into(),
            HcdCode::FreqConvBad => "HPET 频率换算失守：fs 为零除零防护触发或商余守恒失配".into(),
            HcdCode::RescanDrift => "HPET 增量重扫账漂移：重扫 FNV 指纹与首轮指纹失配".into(),
            HcdCode::VendorDualMismatch => "HPET vendorID 双读不符：ACPI PCI 厂商字段与 GCAP 厂商位段不一致".into(),
            HcdCode::MmioReadOob => "HPET MMIO 读越界：偏移落 0x0..0x400 寄存窗口外，读面闸拦截".into(),
            HcdCode::PhaseIllegal => "HPET 探测状态机非法跃迁：阶段未按序推进，探测账作废".into(),
            HcdCode::SealIncomplete => "HPET 封账时账未齐：报告导出前置项（表账/双读/对拍/细账）缺一".into(),
        }
    }
}

// ---------------------------------------------------------------------------
// 二、寄存器偏移与位段常量（IA-PC HPET 规范 v1.0a）
// ---------------------------------------------------------------------------

/// GENERAL_CAPABILITIES_ID 寄存器偏移（0x100，只读）。
pub const HCD_REG_GCAP: u64 = 0x100;
/// GENERAL_CONFIGURATION 寄存器偏移（0x010，含使能位——只读红线靶点）。
pub const HCD_REG_CONF: u64 = 0x010;
/// GENERAL_INTERRUPT_STATUS 寄存器偏移（0x020，只读）。
pub const HCD_REG_INTSTS: u64 = 0x020;
/// MAIN_COUNTER_VALUE 寄存器偏移（0x0F0，只读）。
pub const HCD_REG_MAINCTR: u64 = 0x0F0;
/// 定时器 N 配置与能力寄存器基偏移（0x100 + 16*N——规范布局：定时器
/// 块从 0x100 起每块 16 字节，能力字在块内低 8 字节）。
pub const HCD_REG_TIMER_BASE: u64 = 0x100;

/// GCAP：修订版本字段位段（bit 0:7）。
pub const HCD_CAP_REVISION_SHIFT: u32 = 0;
/// GCAP：修订版本字段掩码。
pub const HCD_CAP_REVISION_MASK: u64 = 0xFF;
/// GCAP：厂商 ID 字段位段（bit 16:31）。
pub const HCD_CAP_VENDOR_SHIFT: u32 = 16;
/// GCAP：厂商 ID 字段掩码。
pub const HCD_CAP_VENDOR_MASK: u64 = 0xFFFF;
/// GCAP：legacy 替换路由位（bit 15）。
pub const HCD_CAP_LEGACY_BIT: u32 = 15;
/// GCAP：主计数器位宽字段位段（bit 12:13，1=32 位、2=64 位——0/3 非法）。
pub const HCD_CAP_WIDTH_SHIFT: u32 = 12;
/// GCAP：主计数器位宽字段掩码（2 位）。
pub const HCD_CAP_WIDTH_MASK: u64 = 0b11;
/// GCAP：主计数器周期字段位段（bit 32:63，单位飞秒）。
pub const HCD_CAP_PERIOD_SHIFT: u32 = 32;

/// CONF：HPET 使能位（bit 0——只读红线靶点，写即 0x8210 立案）。
pub const HCD_CONF_ENABLE_BIT: u32 = 0;
/// CONF：legacy 替换路由使能位（bit 1——同样只读不写）。
pub const HCD_CONF_LEGACY_BIT: u32 = 1;

/// 定时器能力字：周期中断允许位（bit 4，只读——周期允许判据位）。
pub const HCD_TIMCAP_PERIODIC_BIT: u32 = 4;
/// 定时器能力字：64 位宽度位（bit 5，只读——64 位支持判据位）。
pub const HCD_TIMCAP_64BIT_BIT: u32 = 5;
/// 定时器能力字：set-accumulate 置累加位（bit 6，读写——中断触发模式账项）。
pub const HCD_TIMCAP_SETACC_BIT: u32 = 6;
/// 定时器能力字：强制 32 位模式位（bit 7，读写）。
pub const HCD_TIMCAP_FORCE32_BIT: u32 = 7;
/// 定时器能力字：FSB 中断投递能力位（bit 15，只读——FSB 映射判据位）。
pub const HCD_TIMCAP_FSB_BIT: u32 = 15;
/// 定时器能力字：中断路由位图位段（bit 32:63，32 位路由位图）。
pub const HCD_TIMCAP_ROUTE_SHIFT: u32 = 32;

/// 位宽编码（GCAP bit12:13）：1 = 32 位计数器。
pub const HCD_WIDTH_CODE_32: u64 = 0b01;
/// 位宽编码（GCAP bit12:13）：2 = 64 位计数器。
pub const HCD_WIDTH_CODE_64: u64 = 0b10;

// ---------------------------------------------------------------------------
// 三、可注入 MMIO 面（禁真实 MMIO——一切寄存器读经 trait）
// ---------------------------------------------------------------------------

/// HPET MMIO 注入面。
///
/// 探测全程只读：`read64` 是唯一取数口；`write64` 恒拒（返回
/// `EnableWriteViolation`）——使能位只读语义由注入面契约强制，任何
/// 实现都不得在探测语境写通。窗口闸：偏移落 `0x0..0x400` 外一律
/// `MmioReadOob`。
pub trait HcdMmio {
    /// 窗口内 64 位读（偏移为 HPET 寄存空间内偏移）。
    fn read64(&mut self, offset: u64) -> Result<u64, HcdCode>;
    /// 窗口内 64 位写——探测语境恒拒（只读红线由 trait 契约强制）。
    fn write64(&mut self, offset: u64, value: u64) -> Result<(), HcdCode>;
    /// 探测基址（与 ACPI 表基址对拍用）。
    fn probe_base(&self) -> u64;
}

/// 偏移窗口闸：偏移落 `0x0..0x400` 内为合法（64 位读不越窗）。
pub fn hcd_offset_in_window(offset: u64) -> bool {
    offset < HCD_MMIO_WINDOW
}

/// 确定性构造口：由账面种子拼装一台虚拟 HPET。
///
/// 字段全显式注入——同种子恒同读数，探测链全确定性。零真实 IO。
#[derive(Debug, Clone)]
pub struct HcdDeterministicMmio {
    /// 探测基址。
    base: u64,
    /// GENERAL_CAPABILITIES_ID 读数。
    gcap: u64,
    /// GENERAL_CONFIGURATION 读数（含使能位现值）。
    conf: u64,
    /// GENERAL_INTERRUPT_STATUS 读数。
    intsts: u64,
    /// MAIN_COUNTER 读数。
    mainctr: u64,
    /// 比较器能力字表（下标即比较器编号——账面顺序即编号闭集）。
    timer_caps: Vec<u64>,
    /// 读计数（账面留痕——双读核可对账轮次）。
    read_count: u64,
}

impl HcdDeterministicMmio {
    /// 由种子拼装：基址/GCAP/CONF/INTSTS/主计数器/比较器能力表全显式。
    ///
    /// 比较器表超 32 台按 32 台截账（注入爆账防护）。
    pub fn new(base: u64, gcap: u64, conf: u64, intsts: u64, mainctr: u64, timer_caps: Vec<u64>) -> Self {
        let mut caps = timer_caps;
        if caps.len() > HCD_COMPARATOR_MAX {
            caps.truncate(HCD_COMPARATOR_MAX);
        }
        HcdDeterministicMmio { base, gcap, conf, intsts, mainctr, timer_caps: caps, read_count: 0 }
    }

    /// 读计数留痕（账面对账用）。
    pub fn read_count(&self) -> u64 {
        self.read_count
    }

    /// 由三要素一键拼装标准虚拟 HPET（常用构造捷径）。
    ///
    /// 位宽编码 `width_code`（1=32 位 2=64 位）、周期 fs、比较器数
    /// `comparator_count`（表语义——定时器数 = 之 + 1）、每台周期允许
    /// `periodic_ok`。set-accumulate 逐台置位、路由位图逐台独占项、
    /// 64 位支持随位宽编码联动。
    pub fn standard(
        base: u64,
        width_code: u64,
        period_fs: u64,
        vendor_id: u16,
        revision: u8,
        comparator_count: usize,
        legacy_route: bool,
        periodic_ok: bool,
    ) -> Self {
        let mut gcap: u64 = (revision as u64) & HCD_CAP_REVISION_MASK;
        gcap |= ((vendor_id as u64) & HCD_CAP_VENDOR_MASK) << HCD_CAP_VENDOR_SHIFT;
        gcap |= (width_code & HCD_CAP_WIDTH_MASK) << HCD_CAP_WIDTH_SHIFT;
        if legacy_route {
            gcap |= 1u64 << HCD_CAP_LEGACY_BIT;
        }
        gcap |= period_fs << HCD_CAP_PERIOD_SHIFT;
        let mut timer_caps: Vec<u64> = Vec::new();
        let mut i: usize = 0;
        while i < comparator_count {
            let mut cap: u64 = (i as u64) & 0x1F;
            if periodic_ok {
                cap |= 1u64 << HCD_TIMCAP_PERIODIC_BIT;
            }
            if width_code == HCD_WIDTH_CODE_64 {
                cap |= 1u64 << HCD_TIMCAP_64BIT_BIT;
            }
            cap |= 1u64 << HCD_TIMCAP_SETACC_BIT;
            cap |= 1u64 << (HCD_TIMCAP_ROUTE_SHIFT + ((i as u32) % 32));
            timer_caps.push(cap);
            i += 1;
        }
        let conf: u64 = 1u64 << HCD_CONF_ENABLE_BIT;
        HcdDeterministicMmio::new(base, gcap, conf, 0, 0, timer_caps)
    }

    /// 定时器能力读内部口（编号越界返回 None——探测面据此判台数）。
    fn timer_cap_at(&self, index: usize) -> Option<u64> {
        if index < self.timer_caps.len() {
            Some(self.timer_caps[index])
        } else {
            None
        }
    }
}

impl HcdMmio for HcdDeterministicMmio {
    fn read64(&mut self, offset: u64) -> Result<u64, HcdCode> {
        self.read_count = self.read_count.wrapping_add(1);
        if !hcd_offset_in_window(offset) {
            return Err(HcdCode::MmioReadOob);
        }
        if offset == HCD_REG_GCAP {
            return Ok(self.gcap);
        }
        if offset == HCD_REG_CONF {
            return Ok(self.conf);
        }
        if offset == HCD_REG_INTSTS {
            return Ok(self.intsts);
        }
        if offset == HCD_REG_MAINCTR {
            return Ok(self.mainctr);
        }
        if offset >= HCD_REG_TIMER_BASE {
            let rel = offset - HCD_REG_TIMER_BASE;
            if rel % 16 == 0 {
                // 定时器块内低 8 字节：能力字。
                let index = (rel / 16) as usize;
                return match self.timer_cap_at(index) {
                    Some(cap) => Ok(cap),
                    None => Ok(0),
                };
            }
            // 块内高 8 字节：比较器值寄存器——探测不读（写语义），读回零。
            return Ok(0);
        }
        Ok(0)
    }

    fn write64(&mut self, _offset: u64, _value: u64) -> Result<(), HcdCode> {
        // 只读红线：探测语境任何写意图恒拒——由调用侧立案 0x8210。
        Err(HcdCode::EnableWriteViolation)
    }

    fn probe_base(&self) -> u64 {
        self.base
    }
}

// ---------------------------------------------------------------------------
// 四、ACPI HPET 表解析——逐字段账
// ---------------------------------------------------------------------------

/// 单字段审计项（逐字段账的原子单元）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HcdFieldAudit {
    /// 字段名（账面键）。
    pub field: String,
    /// 字段原始值（整数字面——复合字段拆位前的原值）。
    pub value: u64,
    /// 字段级校验是否通过。
    pub ok: bool,
    /// 一句话账注。
    pub note: String,
}

impl HcdFieldAudit {
    /// 构造一条通过项。
    fn pass(field: &str, value: u64, note: &str) -> Self {
        HcdFieldAudit { field: field.to_string(), value, ok: true, note: note.to_string() }
    }

    /// 构造一条不过项。
    fn fail(field: &str, value: u64, note: &str) -> Self {
        HcdFieldAudit { field: field.to_string(), value, ok: false, note: note.to_string() }
    }
}

/// 字节序列按小端拼 u64（复合字段拼装入账助手——报告可读化）。
fn hcd_le_bytes_to_u64(bytes: &[u8]) -> u64 {
    let mut v: u64 = 0;
    let mut i = 0;
    while i < bytes.len() && i < 8 {
        v |= (bytes[i] as u64) << (i * 8);
        i += 1;
    }
    v
}

/// SDT 表头九字段账（本模块自带解析——不依赖他册头结构，自洽封账）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HcdSdtHeaderAccount {
    /// 签名 4 字节（ASCII 原样）。
    pub signature: [u8; 4],
    /// 长度字段（表自述总长）。
    pub length: u32,
    /// 修订。
    pub revision: u8,
    /// 校验和字段原值。
    pub checksum: u8,
    /// OEM ID 6 字节。
    pub oem_id: [u8; 6],
    /// OEM 表 ID 8 字节。
    pub oem_table_id: [u8; 8],
    /// OEM 修订。
    pub oem_revision: u32,
    /// 创建者 ID。
    pub creator_id: u32,
    /// 创建者修订。
    pub creator_revision: u32,
    /// 全表校验和是否通过（字节和 mod 256 == 0）。
    pub checksum_ok: bool,
    /// 逐字段审计账。
    pub audits: Vec<HcdFieldAudit>,
}

impl HcdSdtHeaderAccount {
    /// 签名 ASCII 化（非可打印字节以 '?' 替——报告可读性）。
    pub fn signature_str(&self) -> String {
        hcd_bytes_to_ascii(&self.signature)
    }

    /// OEM ID ASCII 化（同上替字规则）。
    pub fn oem_id_str(&self) -> String {
        hcd_bytes_to_ascii(&self.oem_id)
    }
}

/// 字节序列 ASCII 化：0x20..0x7F 原样、其余 '?' 替。
fn hcd_bytes_to_ascii(bytes: &[u8]) -> String {
    let mut s = String::new();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b >= 0x20 && b < 0x7F {
            s.push(b as char);
        } else {
            s.push('?');
        }
        i += 1;
    }
    s
}

/// 全表校验和：字节和 mod 256 == 0（ACPI 通用规则）。
pub fn hcd_checksum_ok(table: &[u8]) -> bool {
    let mut sum: u64 = 0;
    let mut i = 0;
    while i < table.len() {
        sum = sum.wrapping_add(table[i] as u64);
        i += 1;
    }
    (sum & 0xFF) == 0
}

/// SDT 表头九字段逐项解析入账。
///
/// 规则：① 读面不足 36 字节 → `TableLenBad`；② 签名非 "HPET" →
/// `BadSignature`（签名先校——非 HPET 表不进后续账）；③ 校验和
/// 不过 → `BadChecksum`（账照记、错误照回——坏表不 panic）；④ 通过
/// → 九字段账齐。
pub fn hcd_parse_sdt_header(table: &[u8]) -> Result<HcdSdtHeaderAccount, HcdCode> {
    if table.len() < HCD_SDT_HEADER_LEN {
        return Err(HcdCode::TableLenBad);
    }
    let mut signature = [0u8; 4];
    let mut i = 0;
    while i < 4 {
        signature[i] = table[i];
        i += 1;
    }
    let sig_u32 = u32::from_le_bytes([signature[0], signature[1], signature[2], signature[3]]);
    if sig_u32 != HCD_SIG_HPET {
        return Err(HcdCode::BadSignature);
    }
    let length = u32::from_le_bytes([table[4], table[5], table[6], table[7]]);
    let revision = table[8];
    let checksum = table[9];
    let oem_id: [u8; 6] = [table[10], table[11], table[12], table[13], table[14], table[15]];
    let oem_table_id: [u8; 8] =
        [table[16], table[17], table[18], table[19], table[20], table[21], table[22], table[23]];
    let oem_revision = u32::from_le_bytes([table[24], table[25], table[26], table[27]]);
    let creator_id = u32::from_le_bytes([table[28], table[29], table[30], table[31]]);
    let creator_revision = u32::from_le_bytes([table[32], table[33], table[34], table[35]]);
    let checksum_ok = hcd_checksum_ok(table);
    let mut audits: Vec<HcdFieldAudit> = Vec::new();
    audits.push(HcdFieldAudit::pass("signature", sig_u32 as u64, "表头签名 = HPET，签名关通过"));
    audits.push(if checksum_ok {
        HcdFieldAudit::pass("checksum", checksum as u64, "全表字节和 mod 256 为零")
    } else {
        HcdFieldAudit::fail("checksum", checksum as u64, "全表字节和 mod 256 非零")
    });
    audits.push(HcdFieldAudit::pass("length", length as u64, "表自述总长入账"));
    audits.push(HcdFieldAudit::pass("revision", revision as u64, "表修订"));
    audits.push(HcdFieldAudit::pass("oem_id", hcd_le_bytes_to_u64(&oem_id), "OEM ID 六字节拼装入账"));
    audits.push(HcdFieldAudit::pass("oem_table_id", hcd_le_bytes_to_u64(&oem_table_id), "OEM 表 ID 八字节拼装入账"));
    audits.push(HcdFieldAudit::pass("oem_revision", oem_revision as u64, "OEM 修订"));
    audits.push(HcdFieldAudit::pass("creator_id", creator_id as u64, "创建者 ID"));
    audits.push(HcdFieldAudit::pass("creator_revision", creator_revision as u64, "创建者修订"));
    Ok(HcdSdtHeaderAccount {
        signature,
        length,
        revision,
        checksum,
        oem_id,
        oem_table_id,
        oem_revision,
        creator_id,
        creator_revision,
        checksum_ok,
        audits,
    })
}

/// ACPI HPET 事件定时器块账（规范 §3.2.4 五字段）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HcdEventBlockAccount {
    /// 硬件修订 ID（块内字节 0）。
    pub hardware_rev: u8,
    /// 比较器数（块内字节 1——定时器数 = 之 + 1）。
    pub comparator_count: u8,
    /// 计数器位宽标志（块内字节 2 bit0——1 = 64 位能力）。
    pub counter_size_cap: bool,
    /// legacy 替换路由标志（块内字节 2 bit1）。
    pub legacy_replacement: bool,
    /// PCI 厂商 ID（块内字节 4:5——vendorID 双读左源）。
    pub pci_vendor_id: u16,
    /// 逐字段审计账。
    pub audits: Vec<HcdFieldAudit>,
}

/// 事件定时器块解析：偏移 36 起 8 字节逐字段拆账。
///
/// 校验：硬件修订 ID 为零 → `HwRevZero`（固件偷懒可检）；其余字段
/// 只入账不判错（语义判定归能力账/对照账）。
pub fn hcd_parse_event_block(table: &[u8]) -> Result<HcdEventBlockAccount, HcdCode> {
    if table.len() < HCD_EVENT_BLOCK_OFF + HCD_EVENT_BLOCK_LEN {
        return Err(HcdCode::TableLenBad);
    }
    let b = HCD_EVENT_BLOCK_OFF;
    let hardware_rev = table[b];
    let comparator_count = table[b + 1];
    let flags = table[b + 2];
    let counter_size_cap = (flags & 0b0000_0001) != 0;
    let legacy_replacement = (flags & 0b0000_0010) != 0;
    let pci_vendor_id = u16::from_le_bytes([table[b + 4], table[b + 5]]);
    let mut audits: Vec<HcdFieldAudit> = Vec::new();
    audits.push(if hardware_rev == 0 {
        HcdFieldAudit::fail("hardware_rev", 0, "硬件修订 ID 为零，固件未填")
    } else {
        HcdFieldAudit::pass("hardware_rev", hardware_rev as u64, "硬件修订 ID 入账")
    });
    audits.push(HcdFieldAudit::pass("comparator_count", comparator_count as u64, "比较器数入账（定时器数 = 之 + 1）"));
    audits.push(HcdFieldAudit::pass(
        "counter_size_cap",
        counter_size_cap as u64,
        if counter_size_cap { "计数器位宽标志 = 64 位能力" } else { "计数器位宽标志 = 仅 32 位" },
    ));
    audits.push(HcdFieldAudit::pass(
        "legacy_replacement",
        legacy_replacement as u64,
        if legacy_replacement { "legacy 替换路由标志置位" } else { "legacy 替换路由标志清零" },
    ));
    audits.push(HcdFieldAudit::pass("pci_vendor_id", pci_vendor_id as u64, "PCI 厂商 ID 入账（vendorID 双读左源）"));
    if hardware_rev == 0 {
        return Err(HcdCode::HwRevZero);
    }
    Ok(HcdEventBlockAccount {
        hardware_rev,
        comparator_count,
        counter_size_cap,
        legacy_replacement,
        pci_vendor_id,
        audits,
    })
}

/// 基址 GAS（Generic Address Structure）账——64 位结构逐字段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HcdGasAccount {
    /// 地址空间 ID（0 = 系统内存、1 = 系统 I/O——HPET 合法闭集 {0,1}）。
    pub address_space_id: u8,
    /// 寄存器位宽（HPET 合法值 64）。
    pub register_bit_width: u8,
    /// 寄存器位偏移（HPET 合法值 0）。
    pub register_bit_offset: u8,
    /// 保留字节（应为 0——非零账注不判错）。
    pub reserved: u8,
    /// 64 位物理地址。
    pub address: u64,
    /// 字段闭集是否全过（审计账归表账总集持有——Copy 结构不带 Vec）。
    pub ok: bool,
}

/// GAS 解析：事件块后 12 字节逐字段拆账。
///
/// 闭集：address_space_id ∈ {0,1}、register_bit_width == 64、
/// register_bit_offset == 0、address 非零——任一不过 → `GasBad`。
pub fn hcd_parse_gas(table: &[u8]) -> Result<HcdGasAccount, HcdCode> {
    let off = HCD_EVENT_BLOCK_OFF + HCD_EVENT_BLOCK_LEN;
    if table.len() < off + HCD_GAS_LEN {
        return Err(HcdCode::TableLenBad);
    }
    let address_space_id = table[off];
    let register_bit_width = table[off + 1];
    let register_bit_offset = table[off + 2];
    let reserved = table[off + 3];
    let addr_b: [u8; 8] =
        [table[off + 4], table[off + 5], table[off + 6], table[off + 7], table[off + 8], table[off + 9], table[off + 10], table[off + 11]];
    let address = u64::from_le_bytes(addr_b);
    let ok = (address_space_id == 0 || address_space_id == 1)
        && register_bit_width == 64
        && register_bit_offset == 0
        && address != 0;
    if !ok {
        return Err(HcdCode::GasBad);
    }
    Ok(HcdGasAccount { address_space_id, register_bit_width, register_bit_offset, reserved, address, ok })
}

/// ACPI HPET 表总账（表头 + 事件块 + GAS 三账合一）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HcdAcpiTableAccount {
    /// 表头九字段账。
    pub header: HcdSdtHeaderAccount,
    /// 事件定时器块账。
    pub event_block: HcdEventBlockAccount,
    /// 基址 GAS 账。
    pub gas: HcdGasAccount,
    /// GAS 逐字段审计账（GAS 为 Copy 不带 Vec，审计账在此总集）。
    pub gas_audits: Vec<HcdFieldAudit>,
    /// 长度字段与最小面自洽（length >= 56 且 length <= 读面）。
    pub length_consistent: bool,
    /// 表长度足以容纳声明比较器数所需的定时器信息块
    /// （56 + 20*N <= length——不够即账面残表立案）。
    pub length_fits_timers: bool,
}

/// ACPI HPET 表全量解析入口：三账合一 + 长度自洽双检。
pub fn hcd_parse_acpi_hpet_table(table: &[u8]) -> Result<HcdAcpiTableAccount, HcdCode> {
    let header = hcd_parse_sdt_header(table)?;
    let event_block = hcd_parse_event_block(table)?;
    let gas = hcd_parse_gas(table)?;
    let mut gas_audits: Vec<HcdFieldAudit> = Vec::new();
    gas_audits.push(HcdFieldAudit::pass("gas.address_space_id", gas.address_space_id as u64, "地址空间 ID 落闭集 {0,1}"));
    gas_audits.push(HcdFieldAudit::pass("gas.register_bit_width", gas.register_bit_width as u64, "GAS 位宽 = 64（规范值）"));
    gas_audits.push(HcdFieldAudit::pass("gas.register_bit_offset", gas.register_bit_offset as u64, "GAS 位偏移 = 0（规范值）"));
    gas_audits.push(if gas.reserved == 0 {
        HcdFieldAudit::pass("gas.reserved", 0, "GAS 保留字节为零")
    } else {
        HcdFieldAudit::fail("gas.reserved", gas.reserved as u64, "GAS 保留字节非零（账注不立案）")
    });
    gas_audits.push(HcdFieldAudit::pass("gas.address", gas.address, "GAS 64 位物理地址入账"));
    let length_consistent =
        (header.length as usize) >= HCD_TABLE_MIN_LEN && (header.length as usize) <= table.len();
    if !length_consistent {
        return Err(HcdCode::TableLenBad);
    }
    // 长度足够性：56 + 20*N <= length（N = 比较器数）。不够 → 立案。
    let need = HCD_TABLE_MIN_LEN + HCD_TIMER_BLOCK_LEN * (event_block.comparator_count as usize);
    let length_fits_timers = (header.length as usize) >= need;
    if !length_fits_timers {
        return Err(HcdCode::TableLenBad);
    }
    Ok(HcdAcpiTableAccount {
        header,
        event_block,
        gas,
        gas_audits,
        length_consistent,
        length_fits_timers,
    })
}

// ---------------------------------------------------------------------------
// 五、能力三要素账——位宽判定 / 定时器数语义 / 周期
// ---------------------------------------------------------------------------

/// 主计数器位宽判定（三要素之一）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HcdCounterWidth {
    /// 32 位计数器。
    Width32,
    /// 64 位计数器。
    Width64,
}

impl HcdCounterWidth {
    /// 账面宽度数值。
    pub const fn bits(self) -> u32 {
        match self {
            HcdCounterWidth::Width32 => 32,
            HcdCounterWidth::Width64 => 64,
        }
    }

    /// 账面名。
    pub fn name(self) -> String {
        match self {
            HcdCounterWidth::Width32 => "32".into(),
            HcdCounterWidth::Width64 => "64".into(),
        }
    }
}

/// GCAP bit12:13 位宽编码判定：1 → 32 位、2 → 64 位；0/3 非法编码
/// → 位宽矛盾立案（0x8209——规范外编码不猜）。
pub fn hcd_width_from_gcap(gcap: u64) -> Result<HcdCounterWidth, HcdCode> {
    let code = (gcap >> HCD_CAP_WIDTH_SHIFT) & HCD_CAP_WIDTH_MASK;
    if code == HCD_WIDTH_CODE_32 {
        Ok(HcdCounterWidth::Width32)
    } else if code == HCD_WIDTH_CODE_64 {
        Ok(HcdCounterWidth::Width64)
    } else {
        Err(HcdCode::WidthConflict)
    }
}

/// 位宽双源对拍：ACPI counter_size_cap（1 = 64 位能力）vs GCAP 编码。
///
/// 不合 → `WidthConflict`（三要素之一双源失守——判据直接靶点）。
pub fn hcd_width_crosscheck(counter_size_cap: bool, gcap_width: HcdCounterWidth) -> Result<(), HcdCode> {
    let table_says_64 = counter_size_cap;
    let gcap_says_64 = gcap_width == HcdCounterWidth::Width64;
    if table_says_64 == gcap_says_64 {
        Ok(())
    } else {
        Err(HcdCode::WidthConflict)
    }
}

/// 能力三要素账（位宽 / 定时器数 / 周期——判据点名三件套）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HcdCaps {
    /// 位宽判定（32/64）。
    pub width: HcdCounterWidth,
    /// 定时器数（= 比较器数 + 1，含主定时器语义）。
    pub timer_count: u32,
    /// 主计数器周期（飞秒）。
    pub period_fs: u64,
    /// GCAP 修订（三要素外围账项——报告导出用）。
    pub revision: u8,
    /// GCAP 厂商 ID（vendorID 双读右源）。
    pub vendor_id: u16,
    /// legacy 替换路由能力位。
    pub legacy_route: bool,
}

/// 由 GCAP 原值提取能力账外围字段（位宽判定单源快照；timer_count
/// 由对照账语义闭式另填——此处记 0 占位）。
pub fn hcd_caps_from_gcap(gcap: u64) -> Result<HcdCaps, HcdCode> {
    let width = hcd_width_from_gcap(gcap)?;
    let revision = (gcap >> HCD_CAP_REVISION_SHIFT) & HCD_CAP_REVISION_MASK;
    let vendor = (gcap >> HCD_CAP_VENDOR_SHIFT) & HCD_CAP_VENDOR_MASK;
    let period_fs = gcap >> HCD_CAP_PERIOD_SHIFT;
    let legacy = (gcap >> HCD_CAP_LEGACY_BIT) & 0x1;
    Ok(HcdCaps {
        width,
        timer_count: 0,
        period_fs,
        revision: revision as u8,
        vendor_id: vendor as u16,
        legacy_route: legacy != 0,
    })
}

/// 定时器数语义判定：定时器数 = 比较器数 + 1。
///
/// 闭集：比较器数落 3..=32（规范最少 3 台事件定时器；账面上限 32），
/// 定时器数随之落 4..=33。越界 → `ComparatorCountBad`。
pub fn hcd_timer_count_from_comparators(comparator_count: u8) -> Result<u32, HcdCode> {
    if (comparator_count as usize) < 3 || (comparator_count as usize) > HCD_COMPARATOR_MAX {
        return Err(HcdCode::ComparatorCountBad);
    }
    Ok(comparator_count as u32 + 1)
}

// ---------------------------------------------------------------------------
// 六、周期值域校验与频率换算（全整数 · 零浮点）
// ---------------------------------------------------------------------------

/// 周期值域校验（三要素之三）。
///
/// 合理区间（判据点名）：10^15/10^8 Hz = 10^7 fs（100 MHz 档）至
/// 10^15/10^7 Hz = 10^8 fs（10 MHz 档）。fs 为 0 → 除零隐患直接
/// `PeriodOutOfRange`；落区间外 → 同码（值域关不过不猜）。
pub fn hcd_period_in_band(period_fs: u64) -> Result<(), HcdCode> {
    if period_fs == 0 || period_fs < HCD_PERIOD_FS_MIN || period_fs > HCD_PERIOD_FS_MAX {
        return Err(HcdCode::PeriodOutOfRange);
    }
    Ok(())
}

/// 频率换算账：周期 fs → Hz 全整数（商余双记）。
///
/// 规则：Hz = 10^15 / fs（整数商）；余数 fs 落账披露（非整除时
/// 频率有亚赫兹残差——账面显性不藏）。fs 为 0 → `FreqConvBad`
/// （除零防护——永不 panic）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HcdFreqConv {
    /// 整数商 Hz。
    pub hz: u64,
    /// 余数 fs（10^15 mod fs——非整除残差披露）。
    pub rem_fs: u64,
}

/// 执行换算：fs → Hz（商余双记 + 商余守恒复核）。
pub fn hcd_freq_from_period(period_fs: u64) -> Result<HcdFreqConv, HcdCode> {
    if period_fs == 0 {
        return Err(HcdCode::FreqConvBad);
    }
    let hz = HCD_FS_PER_SEC / period_fs;
    let rem_fs = HCD_FS_PER_SEC % period_fs;
    // 商余守恒：hz * fs + rem == 10^15（复核失配即立案——账面自洽网）。
    let recon = hz.wrapping_mul(period_fs).wrapping_add(rem_fs);
    if recon != HCD_FS_PER_SEC {
        return Err(HcdCode::FreqConvBad);
    }
    Ok(HcdFreqConv { hz, rem_fs })
}

/// 频带对拍：换算所得 Hz 是否落判据点名合理频带（10 MHz..100 MHz）。
///
/// 与 `hcd_period_in_band` 互为镜像——周期关过则频带必过、反之亦然；
/// 双检双记防单侧账漏。
pub fn hcd_freq_in_band(conv: &HcdFreqConv) -> bool {
    conv.hz >= HCD_FREQ_BAND_LO_HZ && conv.hz <= HCD_FREQ_BAND_HI_HZ
}

// ---------------------------------------------------------------------------
// 七、双读一致性核——能力寄存器读 2 次逐字段比对
// ---------------------------------------------------------------------------

/// 单字段双读账项。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HcdDualField {
    /// 字段名。
    pub field: String,
    /// 第一轮读数。
    pub first: u64,
    /// 第二轮读数。
    pub second: u64,
    /// 两轮是否一致。
    pub consistent: bool,
}

/// 双读账项构造助手（两轮同字段比对打包）。
fn hcd_dual_field(name: &str, a: u64, b: u64) -> HcdDualField {
    HcdDualField { field: name.to_string(), first: a, second: b, consistent: a == b }
}

/// 能力寄存器双读总账（GCAP 五字段 + 逐比较器能力字）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HcdDualReadLedger {
    /// GCAP 原值两轮快照。
    pub gcap_first: u64,
    /// GCAP 第二轮快照。
    pub gcap_second: u64,
    /// 逐字段双读账（revision / vendor / width / legacy / period）。
    pub fields: Vec<HcdDualField>,
    /// 比较器能力字双读账（下标即比较器编号）。
    pub timer_fields: Vec<HcdDualField>,
    /// 全字段是否一致（判据"双读 2 次一致"总闸位）。
    pub all_consistent: bool,
}

/// 双读核执行：能力寄存器经注入面读 2 次，逐字段比对。
///
/// 比对字段闭集：修订（bit0:7）/ 厂商（bit16:31）/ 位宽编码
/// （bit12:13）/ legacy 位（bit15）/ 周期（bit32:63）；比较器能力字
/// 逐台整值比对。字段拆位先于比对——位段级漂移定位到字段粒度。
pub fn hcd_dual_read(mmio: &mut dyn HcdMmio, comparator_count: usize) -> Result<HcdDualReadLedger, HcdCode> {
    let gcap_first = mmio.read64(HCD_REG_GCAP)?;
    let gcap_second = mmio.read64(HCD_REG_GCAP)?;
    let ext_a = (gcap_first >> HCD_CAP_WIDTH_SHIFT) & HCD_CAP_WIDTH_MASK;
    let ext_b = (gcap_second >> HCD_CAP_WIDTH_SHIFT) & HCD_CAP_WIDTH_MASK;
    let ven_a = (gcap_first >> HCD_CAP_VENDOR_SHIFT) & HCD_CAP_VENDOR_MASK;
    let ven_b = (gcap_second >> HCD_CAP_VENDOR_SHIFT) & HCD_CAP_VENDOR_MASK;
    let leg_a = (gcap_first >> HCD_CAP_LEGACY_BIT) & 0x1;
    let leg_b = (gcap_second >> HCD_CAP_LEGACY_BIT) & 0x1;
    let per_a = gcap_first >> HCD_CAP_PERIOD_SHIFT;
    let per_b = gcap_second >> HCD_CAP_PERIOD_SHIFT;
    let mut fields: Vec<HcdDualField> = Vec::new();
    fields.push(hcd_dual_field(
        "gcap.revision",
        gcap_first & HCD_CAP_REVISION_MASK,
        gcap_second & HCD_CAP_REVISION_MASK,
    ));
    fields.push(hcd_dual_field("gcap.vendor", ven_a, ven_b));
    fields.push(hcd_dual_field("gcap.width_code", ext_a, ext_b));
    fields.push(hcd_dual_field("gcap.legacy_route", leg_a, leg_b));
    fields.push(hcd_dual_field("gcap.period_fs", per_a, per_b));
    fields.push(hcd_dual_field("gcap.raw", gcap_first, gcap_second));
    // 比较器能力字逐台双读（编号闭集 0..count-1——账面顺序即编号）。
    let mut timer_fields: Vec<HcdDualField> = Vec::new();
    let mut i: usize = 0;
    while i < comparator_count {
        let off = HCD_REG_TIMER_BASE + (i as u64) * 16;
        let a = mmio.read64(off)?;
        let b = mmio.read64(off)?;
        timer_fields.push(hcd_dual_field(&format!("timer{}.cap", i), a, b));
        i += 1;
    }
    let mut all = true;
    let mut fi = 0;
    while fi < fields.len() {
        if !fields[fi].consistent {
            all = false;
        }
        fi += 1;
    }
    let mut ti = 0;
    while ti < timer_fields.len() {
        if !timer_fields[ti].consistent {
            all = false;
        }
        ti += 1;
    }
    Ok(HcdDualReadLedger { gcap_first, gcap_second, fields, timer_fields, all_consistent: all })
}

/// 双读总闸判定：任一字段失配 → 对应诊断码。
///
/// 归因顺序：GCAP 标量字段失配 → 0x8207；其中周期字段单独归因
/// → 0x8208；比较器能力字失配 → 0x820E。全一致 → Ok。
pub fn hcd_dual_read_gate(ledger: &HcdDualReadLedger) -> Result<(), HcdCode> {
    let mut fi = 0;
    while fi < ledger.fields.len() {
        let f = &ledger.fields[fi];
        if !f.consistent {
            if f.field == "gcap.period_fs" {
                return Err(HcdCode::PeriodDualMismatch);
            }
            return Err(HcdCode::CapDualMismatch);
        }
        fi += 1;
    }
    let mut ti = 0;
    while ti < ledger.timer_fields.len() {
        if !ledger.timer_fields[ti].consistent {
            return Err(HcdCode::TimerCapDualMismatch);
        }
        ti += 1;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 八、MMIO 基址账——表基址 vs 探测基址对拍 + 页对齐
// ---------------------------------------------------------------------------

/// MMIO 基址账（对拍 + 页对齐双检）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HcdMmioBaseLedger {
    /// ACPI 表 GAS 所载基址。
    pub table_base: u64,
    /// 注入面探测基址。
    pub probe_base: u64,
    /// 两基址是否一致。
    pub matched: bool,
    /// 表基址是否 4 KiB 页对齐。
    pub table_page_aligned: bool,
    /// 探测基址是否 4 KiB 页对齐。
    pub probe_page_aligned: bool,
}

/// MMIO 基址账执行：对拍 + 双侧页对齐。
///
/// 对拍不合 → `BaseMismatch`；任一侧页对齐不过 → `BaseNotPageAligned`。
pub fn hcd_mmio_base_ledger(table_base: u64, mmio: &dyn HcdMmio) -> Result<HcdMmioBaseLedger, HcdCode> {
    let probe_base = mmio.probe_base();
    let matched = table_base == probe_base;
    let table_page_aligned = (table_base & HCD_PAGE_MASK) == 0;
    let probe_page_aligned = (probe_base & HCD_PAGE_MASK) == 0;
    if !matched {
        return Err(HcdCode::BaseMismatch);
    }
    if !table_page_aligned || !probe_page_aligned {
        return Err(HcdCode::BaseNotPageAligned);
    }
    Ok(HcdMmioBaseLedger { table_base, probe_base, matched, table_page_aligned, probe_page_aligned })
}

// ---------------------------------------------------------------------------
// 九、比较器能力细账——每台逐位段拆解
// ---------------------------------------------------------------------------

/// 单台比较器能力细账。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HcdComparatorLedger {
    /// 比较器编号（账面顺序即编号——唯一性另账 §十）。
    pub index: usize,
    /// 64 位支持（bit 5）。
    pub supports_64bit: bool,
    /// 周期中断允许（bit 4——周期允许判据位）。
    pub periodic_allowed: bool,
    /// set-accumulate 置累加能力（bit 6——中断触发模式账项）。
    pub set_accumulate: bool,
    /// 强制 32 位模式位现值（bit 7——账项不判错）。
    pub force_32bit: bool,
    /// FSB 中断映射能力（bit 15）。
    pub fsb_capable: bool,
    /// 中断路由位图（bit 32:63 截 32 位）。
    pub route_bitmap: u32,
    /// 路由位图是否非零（零位图 = 无可路由项——账注不立案，
    /// FSB 能力缺位时规范允许）。
    pub route_nonzero: bool,
    /// 能力字原值（留痕）。
    pub raw: u64,
}

/// 单台比较器能力字拆账（位段闭集逐项：bit4/5/6/7/15 + bit32:63）。
pub fn hcd_comparator_ledger_one(index: usize, raw: u64) -> HcdComparatorLedger {
    let bit = |n: u32| ((raw >> n) & 0x1) != 0;
    let route_bitmap = ((raw >> HCD_TIMCAP_ROUTE_SHIFT) & 0xFFFF_FFFF) as u32;
    HcdComparatorLedger {
        index,
        supports_64bit: bit(HCD_TIMCAP_64BIT_BIT),
        periodic_allowed: bit(HCD_TIMCAP_PERIODIC_BIT),
        set_accumulate: bit(HCD_TIMCAP_SETACC_BIT),
        force_32bit: bit(HCD_TIMCAP_FORCE32_BIT),
        fsb_capable: bit(HCD_TIMCAP_FSB_BIT),
        route_bitmap,
        route_nonzero: route_bitmap != 0,
        raw,
    }
}

/// 全部比较器细账：逐台读能力字拆解入账。
pub fn hcd_comparator_ledgers(mmio: &mut dyn HcdMmio, comparator_count: usize) -> Result<Vec<HcdComparatorLedger>, HcdCode> {
    let mut out: Vec<HcdComparatorLedger> = Vec::new();
    let mut i: usize = 0;
    while i < comparator_count {
        let off = HCD_REG_TIMER_BASE + (i as u64) * 16;
        let raw = mmio.read64(off)?;
        out.push(hcd_comparator_ledger_one(i, raw));
        i += 1;
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// 十、编号唯一性与有效定时器数闭式（vendorID 双读）
// ---------------------------------------------------------------------------

/// 编号唯一性账：比较器编号闭集 0..=31 内互异且连续（账面顺序即
/// 编号——对账面做显性闸以防账序被外破坏）。
///
/// 检查：① 台数不超 32（越界 → `TimerIndexBad`）；② 编号序列
/// 恰为 0..=n-1 连续互异（断档/重复/越闭集 → `TimerIndexBad`）。
pub fn hcd_timer_index_gate(ledgers: &[HcdComparatorLedger]) -> Result<(), HcdCode> {
    if ledgers.len() > HCD_COMPARATOR_MAX {
        return Err(HcdCode::TimerIndexBad);
    }
    let mut i = 0;
    while i < ledgers.len() {
        if ledgers[i].index != i {
            return Err(HcdCode::TimerIndexBad);
        }
        i += 1;
    }
    Ok(())
}

/// vendorID 双读账：ACPI PCI 厂商字段（左源）vs GCAP 厂商位段（右源）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HcdVendorDualLedger {
    /// 左源：ACPI 事件块 PCI 厂商 ID。
    pub acpi_vendor: u16,
    /// 右源：GCAP bit16:31 厂商 ID。
    pub gcap_vendor: u16,
    /// 双源是否一致。
    pub matched: bool,
    /// 双源是否皆非零（零值源 = 未填——账注）。
    pub both_nonzero: bool,
}

/// vendorID 双读执行：一致 → 闭式生效；不一致 → 立案
/// `VendorDualMismatch`（探测继续——有效定时器数走保守 min 闭式）。
pub fn hcd_vendor_dual(acpi_vendor: u16, gcap_vendor: u16) -> (HcdVendorDualLedger, Result<(), HcdCode>) {
    let matched = acpi_vendor == gcap_vendor;
    let both_nonzero = acpi_vendor != 0 && gcap_vendor != 0;
    let ledger = HcdVendorDualLedger { acpi_vendor, gcap_vendor, matched, both_nonzero };
    if matched {
        (ledger, Ok(()))
    } else {
        (ledger, Err(HcdCode::VendorDualMismatch))
    }
}

/// 有效定时器数闭式：vendorID 双读一致 → 有效定时器数 = 表比较器数 + 1
/// （闭式权威）；双读失配（已立案但探测继续——诊断不中止语义）→
/// 有效定时器数取表比较器数与探测比较器数的保守 min + 1。
pub fn hcd_effective_timer_count(vendor_matched: bool, table_comparators: u8, probe_comparators: usize) -> u32 {
    if vendor_matched {
        table_comparators as u32 + 1
    } else {
        let t = table_comparators as usize;
        let p = probe_comparators;
        let min = if t < p { t } else { p };
        min as u32 + 1
    }
}

/// 探测台数发现：逐台读能力字至零读（账面模型：越台读回零）——
/// 非零能力字计数即探测比较器数。上限 32 台封顶（读满即停不越窗）。
pub fn hcd_probe_comparator_count(mmio: &mut dyn HcdMmio) -> Result<usize, HcdCode> {
    let mut count: usize = 0;
    let mut i: usize = 0;
    while i < HCD_COMPARATOR_MAX {
        let off = HCD_REG_TIMER_BASE + (i as u64) * 16;
        let raw = mmio.read64(off)?;
        if raw == 0 {
            break;
        }
        count += 1;
        i += 1;
    }
    Ok(count)
}

// ---------------------------------------------------------------------------
// 十一、HPET 使能位只读语义——禁写红线
// ---------------------------------------------------------------------------

/// 使能位只读红线账。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HcdEnableRoLedger {
    /// 探测期间出现的写意图次数（每次显性计数——禁静默吞没）。
    pub write_attempts: u64,
    /// 使能位现值（只读快照）。
    pub enable_bit: bool,
    /// legacy 替换位现值（只读快照）。
    pub legacy_bit: bool,
    /// 是否发生违规立案（写意图 > 0）。
    pub violated: bool,
}

/// 使能位只读核：① 双读 CONF 取使能位/legacy 位快照并核一致；
/// ② 试写使能位（bit0=1）——注入面恒拒，拒绝即红线有效；
/// ③ 写意图记账为违规立案（0x8210）。
///
/// 注意：试写是对"写通路存在性"的显性检验，不是真写——注入面
/// 契约保证写意图不落任何状态。返回的账含违规事实，调用侧将
/// 违规写入诊断流（探测继续——诊断是报告不是中止）。
pub fn hcd_enable_ro_audit(mmio: &mut dyn HcdMmio) -> Result<HcdEnableRoLedger, HcdCode> {
    let conf_a = mmio.read64(HCD_REG_CONF)?;
    let conf_b = mmio.read64(HCD_REG_CONF)?;
    if conf_a != conf_b {
        return Err(HcdCode::CapDualMismatch);
    }
    let enable_bit = ((conf_a >> HCD_CONF_ENABLE_BIT) & 0x1) != 0;
    let legacy_bit = ((conf_a >> HCD_CONF_LEGACY_BIT) & 0x1) != 0;
    // 写意图试探：恒拒即红线有效；拒・达皆记 1 次违规（显性非静默）。
    let write_result = mmio.write64(HCD_REG_CONF, 1u64 << HCD_CONF_ENABLE_BIT);
    let (write_attempts, violated) = match write_result {
        // 不可能达此分支（trait 契约恒拒）——若达即红线被绕过，
        // 写通路存在本身即违规事实，照记立案。
        Ok(()) => (1u64, true),
        Err(HcdCode::EnableWriteViolation) => (1u64, true),
        // 越窗等其它错——写意图未达红线靶点，红线账按错误回传。
        Err(_) => return Err(HcdCode::EnableWriteViolation),
    };
    Ok(HcdEnableRoLedger { write_attempts, enable_bit, legacy_bit, violated })
}

// ---------------------------------------------------------------------------
// 十二、FNV 指纹与增量重扫对账
// ---------------------------------------------------------------------------

/// FNV-1a 64 位偏移基。
const HCD_FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 64 位素数。
const HCD_FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// 单 u64 喂入 FNV（8 字节逐字节吸收）。
fn hcd_fnv_u64(h: u64, v: u64) -> u64 {
    let mut acc = h;
    let mut i = 0;
    while i < 8 {
        acc ^= (v >> (i * 8)) & 0xFF;
        acc = acc.wrapping_mul(HCD_FNV_PRIME);
        i += 1;
    }
    acc
}

/// 能力账指纹：三要素 + 外围字段 + GCAP 原值 + 逐台能力字级联哈希。
pub fn hcd_caps_fingerprint(caps: &HcdCaps, gcap: u64, timer_caps: &[u64]) -> u64 {
    let mut h = HCD_FNV_OFFSET;
    h = hcd_fnv_u64(h, caps.width.bits() as u64);
    h = hcd_fnv_u64(h, caps.timer_count as u64);
    h = hcd_fnv_u64(h, caps.period_fs);
    h = hcd_fnv_u64(h, caps.revision as u64);
    h = hcd_fnv_u64(h, caps.vendor_id as u64);
    h = hcd_fnv_u64(h, gcap);
    let mut i = 0;
    while i < timer_caps.len() {
        h = hcd_fnv_u64(h, timer_caps[i]);
        i += 1;
    }
    h
}

/// 重扫对账账目。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HcdRescanLedger {
    /// 首轮指纹。
    pub first_fp: u64,
    /// 重扫指纹。
    pub rescan_fp: u64,
    /// 指纹是否一致。
    pub matched: bool,
    /// 重扫逻辑 tick。
    pub rescan_tick: u64,
}

/// 增量重扫对账：重扫一轮能力读并拼指纹与首轮对拍。
///
/// 漂移 → `RescanDrift`（0x8212）。重扫只读不改账——对账即封账
/// 前最后一张一致性网。
pub fn hcd_rescan_ledger(mmio: &mut dyn HcdMmio, comparator_count: usize, first_fp: u64, rescan_tick: u64) -> Result<HcdRescanLedger, HcdCode> {
    let gcap = mmio.read64(HCD_REG_GCAP)?;
    let mut caps = hcd_caps_from_gcap(gcap)?;
    caps.timer_count = comparator_count as u32 + 1;
    let mut timer_caps: Vec<u64> = Vec::new();
    let mut i: usize = 0;
    while i < comparator_count {
        let off = HCD_REG_TIMER_BASE + (i as u64) * 16;
        timer_caps.push(mmio.read64(off)?);
        i += 1;
    }
    let rescan_fp = hcd_caps_fingerprint(&caps, gcap, &timer_caps);
    let matched = rescan_fp == first_fp;
    if !matched {
        return Err(HcdCode::RescanDrift);
    }
    Ok(HcdRescanLedger { first_fp, rescan_fp, matched, rescan_tick })
}

// ---------------------------------------------------------------------------
// 十三、探测状态机与会话账
// ---------------------------------------------------------------------------

/// 探测阶段状态机（严格线序——跳序即 0x8215）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HcdPhase {
    /// 空闲：账未开。
    Idle,
    /// ACPI 表解析已落账。
    TableParsed,
    /// MMIO 注入面已绑定（基址对拍已过）。
    MmioBound,
    /// 能力双读已落账且一致。
    DualReadDone,
    /// ACPI 表账 vs 探测账对照已过。
    CrossChecked,
    /// 比较器细账与编号闸已过。
    ComparatorAudited,
    /// 只读红线核已过。
    RoAudited,
    /// 重扫对账已过——可封账。
    RescanDone,
}

impl HcdPhase {
    /// 线序下一阶段（终态返回自身）。
    const fn next(self) -> HcdPhase {
        match self {
            HcdPhase::Idle => HcdPhase::TableParsed,
            HcdPhase::TableParsed => HcdPhase::MmioBound,
            HcdPhase::MmioBound => HcdPhase::DualReadDone,
            HcdPhase::DualReadDone => HcdPhase::CrossChecked,
            HcdPhase::CrossChecked => HcdPhase::ComparatorAudited,
            HcdPhase::ComparatorAudited => HcdPhase::RoAudited,
            HcdPhase::RoAudited => HcdPhase::RescanDone,
            HcdPhase::RescanDone => HcdPhase::RescanDone,
        }
    }

    /// 账面名。
    pub fn name(self) -> String {
        match self {
            HcdPhase::Idle => "idle".into(),
            HcdPhase::TableParsed => "table_parsed".into(),
            HcdPhase::MmioBound => "mmio_bound".into(),
            HcdPhase::DualReadDone => "dual_read_done".into(),
            HcdPhase::CrossChecked => "cross_checked".into(),
            HcdPhase::ComparatorAudited => "comparator_audited".into(),
            HcdPhase::RoAudited => "ro_audited".into(),
            HcdPhase::RescanDone => "rescan_done".into(),
        }
    }
}

/// 阶段推进闸：当前阶段必须是 `expected` 才许推进到其下一阶段，
/// 否则 `PhaseIllegal`（跳序/回退皆拒）。
pub fn hcd_phase_gate(current: HcdPhase, expected: HcdPhase) -> Result<HcdPhase, HcdCode> {
    if current == expected {
        Ok(current.next())
    } else {
        Err(HcdCode::PhaseIllegal)
    }
}

/// 诊断项（探测会话全程累积——报告随账导出）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HcdDiag {
    /// 诊断码。
    pub code: HcdCode,
    /// 触发时的逻辑 tick。
    pub tick: u64,
    /// 中文诊断。
    pub detail: String,
}

/// 探测会话：状态机 + 全账 + 诊断流（探测链唯一持有者）。
#[derive(Debug, Clone)]
pub struct HcdSession {
    /// 当前阶段。
    pub phase: HcdPhase,
    /// 会话起始逻辑 tick。
    pub start_tick: u64,
    /// ACPI 表总账（TableParsed 后有值）。
    pub table: Option<HcdAcpiTableAccount>,
    /// MMIO 基址账（MmioBound 后有值）。
    pub base_ledger: Option<HcdMmioBaseLedger>,
    /// 双读总账（DualReadDone 后有值）。
    pub dual: Option<HcdDualReadLedger>,
    /// 能力三要素账（CrossChecked 后有值）。
    pub caps: Option<HcdCaps>,
    /// 比较器细账（ComparatorAudited 后有值）。
    pub comparators: Option<Vec<HcdComparatorLedger>>,
    /// 只读红线账（RoAudited 后有值）。
    pub ro: Option<HcdEnableRoLedger>,
    /// vendorID 双读账（CrossChecked 后有值）。
    pub vendor: Option<HcdVendorDualLedger>,
    /// 重扫对账账目（RescanDone 后有值）。
    pub rescan: Option<HcdRescanLedger>,
    /// 诊断流（全程累积——含违规立案与降级账注）。
    pub diags: Vec<HcdDiag>,
}

impl HcdSession {
    /// 新会话：Idle 起，起始 tick 入账。
    pub fn new(start_tick: u64) -> Self {
        HcdSession {
            phase: HcdPhase::Idle,
            start_tick,
            table: None,
            base_ledger: None,
            dual: None,
            caps: None,
            comparators: None,
            ro: None,
            vendor: None,
            rescan: None,
            diags: Vec::new(),
        }
    }

    /// 记一条诊断（不中止——诊断是报告）。
    pub fn record(&mut self, code: HcdCode, tick: u64) {
        self.diags.push(HcdDiag { code, tick, detail: code.reason() });
    }
}

// ---------------------------------------------------------------------------
// 十四、启动报告 kv 导出
// ---------------------------------------------------------------------------

/// kv 报告项（键值对——键序即账序）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HcdKv {
    /// 键。
    pub key: String,
    /// 值。
    pub val: String,
}

/// kv 拼装助手（账序即键序——一处 helper 管全账键值化）。
fn hcd_kv_add(kv: &mut Vec<HcdKv>, key: &str, val: String) {
    kv.push(HcdKv { key: key.to_string(), val });
}

/// HPET 探测启动报告（封账产物——kv 全量导出）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HcdReport {
    /// 模块版本。
    pub module: String,
    /// 报告逻辑 tick。
    pub tick: u64,
    /// 探测终态阶段名。
    pub phase: String,
    /// 是否封账成功（全前置齐 + 零阻断诊断）。
    pub sealed: bool,
    /// kv 账目（账序导出）。
    pub kv: Vec<HcdKv>,
    /// 诊断流导出（code:detail 行）。
    pub diag_lines: Vec<String>,
}

impl HcdReport {
    /// 渲染为多行文本（`k=v` 每行一项，诊断段随后）。
    pub fn render(&self) -> String {
        let mut out = String::new();
        let mut i = 0;
        while i < self.kv.len() {
            out.push_str(&self.kv[i].key);
            out.push('=');
            out.push_str(&self.kv[i].val);
            out.push('\n');
            i += 1;
        }
        let mut d = 0;
        while d < self.diag_lines.len() {
            out.push_str("diag=");
            out.push_str(&self.diag_lines[d]);
            out.push('\n');
            d += 1;
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 十五、顶层探测链——表账 → 绑定 → 双读 → 对照 → 细账 → 红线 → 重扫 → 封账
// ---------------------------------------------------------------------------

/// 探测链各步产物总装账（run 内部聚合——报告导出的数据源）。
#[derive(Debug, Clone)]
struct HcdChain {
    /// ACPI 表总账。
    table: HcdAcpiTableAccount,
    /// MMIO 基址账。
    base_ledger: HcdMmioBaseLedger,
    /// 双读总账。
    dual: HcdDualReadLedger,
    /// 能力三要素账。
    caps: HcdCaps,
    /// 比较器细账。
    comparators: Vec<HcdComparatorLedger>,
    /// 只读红线账。
    ro: HcdEnableRoLedger,
    /// vendorID 双读账。
    vendor: HcdVendorDualLedger,
    /// 重扫对账账目。
    rescan: HcdRescanLedger,
    /// 有效定时器数（vendorID 双读闭式或保守 min）。
    effective_timer_count: u32,
    /// 频率换算账。
    freq: HcdFreqConv,
}

/// ACPI 表账 vs 探测账逐字段对照（判据：探测结果与 ACPI HPET 表对照一致）。
///
/// 对照项闭集：① 位宽（counter_size_cap vs GCAP 编码判定）；② 定时器数
/// 语义（探测台数 vs 表比较器数 + 1）；③ 周期（表无周期字段——周期账
/// 单源 GCAP 值域闸，账注显性）；④ 基址（表 GAS vs 探测基址——§八已
/// 闸，此处复核留账）；⑤ vendorID 双读（失配立案但探测继续）。
fn hcd_crosscheck(
    session: &mut HcdSession,
    table: &HcdAcpiTableAccount,
    dual: &HcdDualReadLedger,
    probe_comparators: usize,
    tick: u64,
) -> Result<HcdCaps, HcdCode> {
    // ① 位宽双源对拍。
    let gcap_width = hcd_width_from_gcap(dual.gcap_first)?;
    hcd_width_crosscheck(table.event_block.counter_size_cap, gcap_width)?;
    // ② 定时器数语义：表比较器数闭集闸 → 探测台数 = 表比较器数 + 1。
    let table_timers = hcd_timer_count_from_comparators(table.event_block.comparator_count)?;
    if probe_comparators as u32 + 1 != table_timers {
        return Err(HcdCode::TimerCountConflict);
    }
    // ③ 周期账：GCAP 周期值域闸（表无周期字段——单源账注显性）。
    let mut caps = hcd_caps_from_gcap(dual.gcap_first)?;
    hcd_period_in_band(caps.period_fs)?;
    caps.timer_count = table_timers;
    // ④ 基址复核留账（§八已闸过——此处账面自证）。
    match &session.base_ledger {
        Some(bl) => {
            if !bl.matched {
                return Err(HcdCode::BaseMismatch);
            }
        }
        None => return Err(HcdCode::PhaseIllegal),
    }
    // ⑤ vendorID 双读（表 PCI 厂商 vs GCAP 厂商）。
    let (vledger, vres) = hcd_vendor_dual(table.event_block.pci_vendor_id, caps.vendor_id);
    session.vendor = Some(vledger);
    if let Err(code) = vres {
        // 失配立案但探测继续——有效定时器数走保守 min 闭式。
        session.record(code, tick);
    }
    Ok(caps)
}

/// 封账闸：报告导出前置项闭集自查。
///
/// 前置闭集：表账（校验和/长度自洽/长度足够）/ 基址账（对拍+页对齐）/
/// 双读账（全一致）/ 三要素账（定时器数非零）/ 比较器细账（台数 =
/// 定时器数 - 1）/ 会话账位（细账/红线/重扫有值）/ 重扫指纹一致 /
/// 周期频带双检。缺一 → `SealIncomplete`。
fn hcd_seal_gate(chain: &HcdChain, session: &HcdSession) -> Result<(), HcdCode> {
    if !chain.table.length_consistent || !chain.table.length_fits_timers || !chain.table.header.checksum_ok {
        return Err(HcdCode::SealIncomplete);
    }
    if !chain.base_ledger.matched || !chain.base_ledger.table_page_aligned || !chain.base_ledger.probe_page_aligned {
        return Err(HcdCode::SealIncomplete);
    }
    if !chain.dual.all_consistent {
        return Err(HcdCode::SealIncomplete);
    }
    if chain.caps.timer_count == 0 {
        return Err(HcdCode::SealIncomplete);
    }
    if chain.comparators.len() as u32 + 1 != chain.caps.timer_count {
        return Err(HcdCode::SealIncomplete);
    }
    if session.comparators.is_none() || session.ro.is_none() || session.rescan.is_none() {
        return Err(HcdCode::SealIncomplete);
    }
    if !chain.rescan.matched {
        return Err(HcdCode::SealIncomplete);
    }
    hcd_period_in_band(chain.caps.period_fs)?;
    if !hcd_freq_in_band(&chain.freq) {
        return Err(HcdCode::SealIncomplete);
    }
    Ok(())
}

/// kv 拼装：全账键值化（账序即键序——表账 → 基址账 → 能力账 →
/// 比较器汇总 → 红线账 → 重扫账 → 诊断计数）。
fn hcd_build_kv(chain: &HcdChain, session: &HcdSession, tick: u64) -> Vec<HcdKv> {
    let mut kv: Vec<HcdKv> = Vec::new();
    hcd_kv_add(&mut kv, "module", HCD_VERSION.into());
    hcd_kv_add(&mut kv, "tick", format!("{}", tick));
    hcd_kv_add(&mut kv, "phase", session.phase.name());
    hcd_kv_add(&mut kv, "table.signature", chain.table.header.signature_str());
    hcd_kv_add(&mut kv, "table.length", format!("{}", chain.table.header.length));
    hcd_kv_add(&mut kv, "table.revision", format!("{}", chain.table.header.revision));
    hcd_kv_add(&mut kv, "table.checksum_ok", format!("{}", chain.table.header.checksum_ok));
    hcd_kv_add(&mut kv, "table.oem_id", chain.table.header.oem_id_str());
    hcd_kv_add(&mut kv, "table.hw_rev", format!("{}", chain.table.event_block.hardware_rev));
    hcd_kv_add(&mut kv, "table.comparator_count", format!("{}", chain.table.event_block.comparator_count));
    hcd_kv_add(&mut kv, "table.counter_size_cap", format!("{}", chain.table.event_block.counter_size_cap as u8));
    hcd_kv_add(&mut kv, "table.legacy_replacement", format!("{}", chain.table.event_block.legacy_replacement as u8));
    hcd_kv_add(&mut kv, "table.pci_vendor_id", format!("0x{:04X}", chain.table.event_block.pci_vendor_id));
    hcd_kv_add(&mut kv, "gas.space_id", format!("{}", chain.table.gas.address_space_id));
    hcd_kv_add(&mut kv, "gas.bit_width", format!("{}", chain.table.gas.register_bit_width));
    hcd_kv_add(&mut kv, "gas.address", format!("0x{:016X}", chain.table.gas.address));
    hcd_kv_add(&mut kv, "mmio.table_base", format!("0x{:016X}", chain.base_ledger.table_base));
    hcd_kv_add(&mut kv, "mmio.probe_base", format!("0x{:016X}", chain.base_ledger.probe_base));
    hcd_kv_add(&mut kv, "mmio.base_matched", format!("{}", chain.base_ledger.matched as u8));
    hcd_kv_add(&mut kv, "mmio.page_aligned", format!("{}", (chain.base_ledger.table_page_aligned && chain.base_ledger.probe_page_aligned) as u8));
    hcd_kv_add(&mut kv, "caps.width_bits", chain.caps.width.name());
    hcd_kv_add(&mut kv, "caps.timer_count", format!("{}", chain.caps.timer_count));
    hcd_kv_add(&mut kv, "caps.effective_timer_count", format!("{}", chain.effective_timer_count));
    hcd_kv_add(&mut kv, "caps.period_fs", format!("{}", chain.caps.period_fs));
    hcd_kv_add(&mut kv, "caps.freq_hz", format!("{}", chain.freq.hz));
    hcd_kv_add(&mut kv, "caps.freq_rem_fs", format!("{}", chain.freq.rem_fs));
    hcd_kv_add(&mut kv, "caps.freq_in_band", format!("{}", hcd_freq_in_band(&chain.freq) as u8));
    hcd_kv_add(&mut kv, "caps.revision", format!("{}", chain.caps.revision));
    hcd_kv_add(&mut kv, "caps.vendor_id", format!("0x{:04X}", chain.caps.vendor_id));
    hcd_kv_add(&mut kv, "caps.legacy_route", format!("{}", chain.caps.legacy_route as u8));
    hcd_kv_add(&mut kv, "vendor.dual_matched", format!("{}", chain.vendor.matched as u8));
    hcd_kv_add(&mut kv, "vendor.both_nonzero", format!("{}", chain.vendor.both_nonzero as u8));
    hcd_kv_add(&mut kv, "dual.all_consistent", format!("{}", chain.dual.all_consistent as u8));
    hcd_kv_add(&mut kv, "dual.fields", format!("{}", chain.dual.fields.len()));
    hcd_kv_add(&mut kv, "dual.timer_fields", format!("{}", chain.dual.timer_fields.len()));
    // 比较器四能力汇总：单遍扫描累计（周期允许/FSB/64 位/set-accumulate/路由非零）。
    let mut periodic_count: u32 = 0;
    let mut fsb_count: u32 = 0;
    let mut cap64_count: u32 = 0;
    let mut setacc_count: u32 = 0;
    let mut route_nz_count: u32 = 0;
    let mut ci = 0;
    while ci < chain.comparators.len() {
        let c = &chain.comparators[ci];
        if c.periodic_allowed {
            periodic_count += 1;
        }
        if c.fsb_capable {
            fsb_count += 1;
        }
        if c.supports_64bit {
            cap64_count += 1;
        }
        if c.set_accumulate {
            setacc_count += 1;
        }
        if c.route_nonzero {
            route_nz_count += 1;
        }
        ci += 1;
    }
    hcd_kv_add(&mut kv, "timers.periodic_allowed_count", format!("{}", periodic_count));
    hcd_kv_add(&mut kv, "timers.fsb_capable_count", format!("{}", fsb_count));
    hcd_kv_add(&mut kv, "timers.cap64_count", format!("{}", cap64_count));
    hcd_kv_add(&mut kv, "timers.setacc_count", format!("{}", setacc_count));
    hcd_kv_add(&mut kv, "timers.route_nonzero_count", format!("{}", route_nz_count));
    hcd_kv_add(&mut kv, "ro.write_attempts", format!("{}", chain.ro.write_attempts));
    hcd_kv_add(&mut kv, "ro.enable_bit", format!("{}", chain.ro.enable_bit as u8));
    hcd_kv_add(&mut kv, "ro.legacy_bit", format!("{}", chain.ro.legacy_bit as u8));
    hcd_kv_add(&mut kv, "ro.violated", format!("{}", chain.ro.violated as u8));
    hcd_kv_add(&mut kv, "rescan.fingerprint", format!("0x{:016X}", chain.rescan.rescan_fp));
    hcd_kv_add(&mut kv, "rescan.matched", format!("{}", chain.rescan.matched as u8));
    hcd_kv_add(&mut kv, "rescan.tick", format!("{}", chain.rescan.rescan_tick));
    hcd_kv_add(&mut kv, "diag.count", format!("{}", session.diags.len()));
    kv
}

/// HPET 探测与能力账顶层入口（判据全链执行）。
///
/// 链序（状态机强制，跳序即 0x8215）：
/// 1. ACPI HPET 表解析（签名/校验和/长度/事件块/GAS 逐字段账）；
/// 2. MMIO 基址对拍 + 页对齐（表基址 vs 探测基址）；
/// 3. 能力寄存器双读 2 次逐字段比对（GCAP 六字段 + 逐比较器能力字）；
/// 4. 表账 vs 探测账对照（位宽双源 / 定时器数语义 / 周期值域 /
///    vendorID 双读）；
/// 5. 比较器能力细账（64 位/周期/set-accumulate/FSB/路由逐台）+ 编号闸；
/// 6. 使能位只读红线核（写意图试探 → 违规立案）；
/// 7. 增量重扫对账（指纹对拍）；
/// 8. 封账闸 + kv 报告导出。
///
/// 诊断语义：`VendorDualMismatch` 与红线违规立案但探测继续
/// （诊断是报告不是中止）；其余诊断码为阻断——`Err` 返回（未封账
/// 不发报告——账面自洽）。
pub fn hcd_run_probe(table: &[u8], table_base: u64, mmio: &mut dyn HcdMmio, tick: u64) -> Result<HcdReport, HcdCode> {
    let mut session = HcdSession::new(tick);

    // 1. 表解析（Idle → TableParsed）。
    session.phase = hcd_phase_gate(session.phase, HcdPhase::Idle)?;
    let table_acc = hcd_parse_acpi_hpet_table(table)?;
    session.table = Some(table_acc.clone());
    session.phase = hcd_phase_gate(session.phase, HcdPhase::TableParsed)?;

    // 2. MMIO 基址账（TableParsed → MmioBound）。
    let base_ledger = hcd_mmio_base_ledger(table_base, mmio)?;
    session.base_ledger = Some(base_ledger.clone());
    session.phase = hcd_phase_gate(session.phase, HcdPhase::MmioBound)?;

    // 3. 双读核（MmioBound → DualReadDone）。
    let comparator_count = table_acc.event_block.comparator_count as usize;
    let dual = hcd_dual_read(mmio, comparator_count)?;
    hcd_dual_read_gate(&dual)?;
    session.dual = Some(dual.clone());
    session.phase = hcd_phase_gate(session.phase, HcdPhase::DualReadDone)?;

    // 4. 表账 vs 探测账对照（DualReadDone → CrossChecked）。
    let probe_comparators = hcd_probe_comparator_count(mmio)?;
    let caps = hcd_crosscheck(&mut session, &table_acc, &dual, probe_comparators, tick)?;
    session.phase = hcd_phase_gate(session.phase, HcdPhase::CrossChecked)?;

    // 5. 比较器细账 + 编号闸（CrossChecked → ComparatorAudited）。
    let comparators = hcd_comparator_ledgers(mmio, comparator_count)?;
    hcd_timer_index_gate(&comparators)?;
    session.comparators = Some(comparators.clone());
    session.phase = hcd_phase_gate(session.phase, HcdPhase::ComparatorAudited)?;

    // 6. 只读红线核（ComparatorAudited → RoAudited）。违规立案但继续。
    let ro = hcd_enable_ro_audit(mmio)?;
    session.ro = Some(ro.clone());
    if ro.violated {
        session.record(HcdCode::EnableWriteViolation, tick);
    }
    session.phase = hcd_phase_gate(session.phase, HcdPhase::RoAudited)?;

    // 7. 增量重扫对账（RoAudited → RescanDone）。
    let timer_caps_snap: Vec<u64> = {
        let mut v: Vec<u64> = Vec::new();
        let mut i = 0;
        while i < comparators.len() {
            v.push(comparators[i].raw);
            i += 1;
        }
        v
    };
    let first_fp = hcd_caps_fingerprint(&caps, dual.gcap_first, &timer_caps_snap);
    let rescan = hcd_rescan_ledger(mmio, comparator_count, first_fp, tick.wrapping_add(1))?;
    session.rescan = Some(rescan.clone());
    session.phase = hcd_phase_gate(session.phase, HcdPhase::RescanDone)?;

    // 8. 频率换算 + 封账闸 + kv 报告。
    let freq = hcd_freq_from_period(caps.period_fs)?;
    let vendor_matched = match &session.vendor {
        Some(v) => v.matched,
        None => false,
    };
    let effective = hcd_effective_timer_count(vendor_matched, table_acc.event_block.comparator_count, probe_comparators);
    let vendor = match &session.vendor {
        Some(v) => v.clone(),
        None => HcdVendorDualLedger { acpi_vendor: 0, gcap_vendor: 0, matched: false, both_nonzero: false },
    };
    let chain = HcdChain {
        table: table_acc,
        base_ledger,
        dual,
        caps,
        comparators,
        ro,
        vendor,
        rescan,
        effective_timer_count: effective,
        freq,
    };
    hcd_seal_gate(&chain, &session)?;
    let diag_lines: Vec<String> = {
        let mut v: Vec<String> = Vec::new();
        let mut i = 0;
        while i < session.diags.len() {
            let d = &session.diags[i];
            v.push(format!("0x{:04X}:{}", d.code.num(), d.detail));
            i += 1;
        }
        v
    };
    Ok(HcdReport {
        module: HCD_VERSION.into(),
        tick,
        phase: session.phase.name(),
        sealed: true,
        kv: hcd_build_kv(&chain, &session, tick),
        diag_lines,
    })
}

// ---------------------------------------------------------------------------
// 十六、账面自洽收尾——段自查与镜像不变量
// ---------------------------------------------------------------------------

/// 诊断码闭集自查：全码落段且互异（账面不变量——调用侧可随时核）。
pub fn hcd_diag_codes_sane() -> bool {
    let codes: [u16; 22] = [
        HcdCode::BadSignature.num(),
        HcdCode::BadChecksum.num(),
        HcdCode::TableLenBad.num(),
        HcdCode::HwRevZero.num(),
        HcdCode::GasBad.num(),
        HcdCode::ComparatorCountBad.num(),
        HcdCode::CapDualMismatch.num(),
        HcdCode::PeriodDualMismatch.num(),
        HcdCode::WidthConflict.num(),
        HcdCode::TimerCountConflict.num(),
        HcdCode::PeriodOutOfRange.num(),
        HcdCode::BaseMismatch.num(),
        HcdCode::BaseNotPageAligned.num(),
        HcdCode::TimerCapDualMismatch.num(),
        HcdCode::TimerIndexBad.num(),
        HcdCode::EnableWriteViolation.num(),
        HcdCode::FreqConvBad.num(),
        HcdCode::RescanDrift.num(),
        HcdCode::VendorDualMismatch.num(),
        HcdCode::MmioReadOob.num(),
        HcdCode::PhaseIllegal.num(),
        HcdCode::SealIncomplete.num(),
    ];
    let mut i = 0;
    while i < codes.len() {
        if codes[i] < HCD_SEG_BASE || codes[i] >= HCD_SEG_END {
            return false;
        }
        let mut j = i + 1;
        while j < codes.len() {
            if codes[i] == codes[j] {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 周期频带与频率换算镜像自洽：周期关过 ⇔ 频带关过（账面不变量）。
pub fn hcd_band_mirror_sane(period_fs: u64) -> bool {
    match hcd_period_in_band(period_fs) {
        Ok(()) => match hcd_freq_from_period(period_fs) {
            Ok(conv) => hcd_freq_in_band(&conv),
            Err(_) => false,
        },
        Err(_) => false,
    }
}
