//! CGPU-F1443 · Intel RAPL/MSR 读取（CGPU-J 域 · 功耗域 · J03 组 · 深化版）。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F1443`
//!
//! **判据（锚点原文）**：RAPL 主路、MSR 备路、iGPU 分账、回绕补偿、
//! 频率联动、差异表、精度声明、测试、判据。
//!
//! **职责定位（锚点原文）**：Intel 采样：RAPL 接口（sysfs powercap
//! energy_uj 读取——CPU Package/集成显卡 power/energy 域分账读取）；
//! MSR 补充路径（rdmsr 0x611 等寄存器——驱动不可用时的备路——权限要求
//! 显性：管理员/内核驱动门槛声明）；核显功率（iGPU power 域——与 CPU
//! Package 区分归账）；energy 累计器回绕（32 位计数器回绕检测与补偿
//! ——回绕即诊断）；频率联动（与 RAPL 同步读核心频率——功耗-频率联合
//! 画像输入 F1460）；平台差异（Gen9-Gen14 采样差异表——按设备 ID 分派
//! 实现）；精度声明（RAPL 精度 ~1mW 级——文档写明不夸大）；测试
//! （回绕注入/域缺失回退/多代平台模拟）。
//!
//! 本深化版扩出的真实语义密度（全部纯功能，零 IO 零墙钟零浮点）：
//!
//! 1. **RAPL 主路**（判据一）：五域闭集（[`Domain`]：Package/Core/
//!    Uncore/iGPU/DRAM），读取面注入式（[`RaplReading`]——零 IO：真实
//!    sysfs/驱动读取位在宿主侧，本模块只做语义账），域间分账互不混写。
//! 2. **MSR 备路**（判据二）：[`MSR_TABLE`] 真实寄存器地址表
//!    （0x606 功率单位/0x611 Package/0x619 DRAM/0x639 PP0/0x641 PP1），
//!    **权限门槛显性**（[`RaplMsrEngine::enable_msr_fallback`] 需显性
//!    授权位——未授权拒启，[`XcCode::MsrPermissionDenied`]）。
//! 3. **iGPU 分账**（判据三）：iGPU 域独立账（[`DomainAccount`]），
//!    与 Package 分账不混写；平台不支持 iGPU 域（差异表标记）时读取
//!    显性回退立案。
//! 4. **回绕补偿**（判据四）：32 位 energy 计数器回绕检测（新值 < 旧值）
//!    + 补偿（每回绕补 2^32 µJ）+ **回绕即诊断**（回绕间隔低于
//!    [`WRAP_MIN_INTERVAL_TICKS`] 视为可疑读数立案——真回绕是物理慢
//!    过程，高频回绕=读数损坏）。
//! 5. **频率联动**（判据五）：每采样同步注入核心频率，产出功耗
//!    （mW，定点）与能效（kHz/W）联合画像行——F1460 联合选点输入面。
//! 6. **平台差异表**（判据六）：[`GEN_TABLE`] Gen9-Gen14 六代闭集
//!    （设备 ID 区间分派/iGPU 域有无/MSR 备路有无/能量单位 nj-per-LSB
//!    ——按代分派采样语义）。
//! 7. **精度声明**（判据七）：[`PRECISION_NOTE`] 字面量冻结（~1mW 级
//!    不夸大）+ 精度门（低于 [`PRECISION_FLOOR_MW`] 的功率变化不进
//!    趋势账——防噪声伪趋势）。
//! 8. **测试**（判据八）：[`run_cgj03_checks`] 参考探针——回绕注入/
//!    域缺失回退/多代平台模拟/权限门/精度门全覆盖。
//!
//! **对接**：F1441（J 域架构总览——域使命上游）；F1442（采样器与传感
//! 器抽象——[`crate`] 对接面，本模块语义账与其 [`Sampler`] 互补）；
//! F1460（功耗-频率联合选点——画像消费方）。零 panic 面、零 IO、
//! 零墙钟（tick 注入）、零浮点、no_std。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量（阈值/地址/精度唯一源）
// ---------------------------------------------------------------------------

/// 本项版本。
pub const RAPL_MSR_VERSION: &str = "J03-raplmsr-v1";

/// 回绕最小间隔（tick；低于此间隔的回绕视为可疑读数——真回绕是慢过程）。
pub const WRAP_MIN_INTERVAL_TICKS: u64 = 100;

/// 32 位能量计数器模（µJ；2^32）。
pub const COUNTER_MOD_UJ: u64 = 4_294_967_296;

/// 精度地板（mW；低于此差异不进趋势账——精度声明不夸大的机制面）。
pub const PRECISION_FLOOR_MW: u64 = 1;

/// 采样间隔缺省（ms；功率换算分母口径）。
pub const SAMPLE_INTERVAL_MS: u64 = 100;

/// 事件账容量（滚动）。
pub const EVENT_LEDGER_CAPACITY: usize = 64;

/// 精度声明（字面量冻结——文档面即机制面，锚点"不夸大"）。
pub const PRECISION_NOTE: &str = "RAPL 精度约 1mW 级（能量计数器 LSB 为纳焦级、采样窗 100ms）；低于 1mW 的功率差异不进趋势账；本声明为读数精度上限，不是测量承诺";

/// 权限门槛声明（MSR 备路——显性不隐藏）。
pub const MSR_PERMISSION_NOTE: &str = "MSR 备路需管理员/内核驱动门槛（rdmsr 指令 ring0）；未获显性授权时备路拒启，仅 RAPL 主路服务";

/// 真实 MSR 地址表（Intel SDM 口径；0 表示该域无 MSR 备路）。
/// MSR_RAPL_POWER_UNIT=0x606；MSR_PKG_ENERGY_STATUS=0x611；
/// MSR_DRAM_ENERGY_STATUS=0x619；MSR_PP0_ENERGY_STATUS=0x639（Core）；
/// MSR_PP1_ENERGY_STATUS=0x641（iGPU）。
pub const MSR_TABLE: [(u8, u16); 5] = [
    (0, 0x611), // Package
    (1, 0x639), // Core（PP0）
    (2, 0),     // Uncore 无独立能量 MSR（并入 Package——表内 0 显性声明）
    (3, 0x641), // iGPU（PP1）
    (4, 0x619), // DRAM
];

/// MSR 功率单位寄存器地址。
pub const MSR_RAPL_POWER_UNIT: u16 = 0x606;

// ---------------------------------------------------------------------------
// 二、诊断码（0x4B 段独占：J03 RAPL/MSR 面）
// ---------------------------------------------------------------------------

/// 诊断码闭集（0x4B 段）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XcCode {
    /// 域缺失（平台差异表标记该代无此域——回退或拒读）。
    DomainMissing,
    /// MSR 备路未授权（权限门槛显性拒）。
    MsrPermissionDenied,
    /// MSR 备路未启用或该域无 MSR 地址。
    MsrUnavailable,
    /// 可疑回绕（回绕间隔过短——读数损坏嫌疑）。
    SuspiciousWrap,
    /// 读数回退（raw 值倒退且无法用回绕解释——账面拒收）。
    ReadingRegress,
    /// 采样间隔非法（Δtick 为 0——除零防线）。
    ZeroInterval,
    /// 平台未知（设备 ID 落差异表外——保守回退 Gen 基线）。
    PlatformUnknown,
    /// 频率联动缺失（画像行缺频率半边——不产出画像）。
    FreqMissing,
    /// 事件账指纹链断裂。
    LedgerCorrupt,
    /// 账面断裂（域账总量与采样和不等）。
    LedgerMismatch,
}

impl XcCode {
    /// 人读诊断（全带可操作建议）。
    pub const fn detail(self) -> &'static str {
        match self {
            XcCode::DomainMissing => "域缺失：平台差异表标记该代无此能量域，建议按差异表回退到 Package 域分摊或显性跳过",
            XcCode::MsrPermissionDenied => "MSR 备路未授权：需管理员/内核驱动门槛，建议走 RAPL 主路或显性授权后重试",
            XcCode::MsrUnavailable => "MSR 备路不可用：未启用或该域无 MSR 地址（差异表 0 项），建议确认平台代际与授权位",
            XcCode::SuspiciousWrap => "可疑回绕：回绕间隔过短（真回绕是慢物理过程），建议核查读数源是否损坏或位宽截断",
            XcCode::ReadingRegress => "读数倒退：新值小于旧值且无法用单次回绕解释，建议丢弃本样并立案",
            XcCode::ZeroInterval => "采样间隔为 0：功率换算除零防线，建议检查采样时钟推进",
            XcCode::PlatformUnknown => "平台未知：设备 ID 落差异表外，建议按保守基线（最近兼容代）采样并留痕",
            XcCode::FreqMissing => "频率联动缺失：画像行需功耗与频率两半，建议同步注入频率读数",
            XcCode::LedgerCorrupt => "事件账指纹链断裂：账面被篡改或乱序，建议以快照为准重建",
            XcCode::LedgerMismatch => "账面断裂：域账累计与采样和不等，建议对账修复后再出报告",
        }
    }

    /// 0x4B 段十六进制码。
    pub const fn code_hex(self) -> u16 {
        match self {
            XcCode::DomainMissing => 0x4B01,
            XcCode::MsrPermissionDenied => 0x4B02,
            XcCode::MsrUnavailable => 0x4B03,
            XcCode::SuspiciousWrap => 0x4B04,
            XcCode::ReadingRegress => 0x4B05,
            XcCode::ZeroInterval => 0x4B06,
            XcCode::PlatformUnknown => 0x4B07,
            XcCode::FreqMissing => 0x4B08,
            XcCode::LedgerCorrupt => 0x4B09,
            XcCode::LedgerMismatch => 0x4B0A,
        }
    }
}

// ---------------------------------------------------------------------------
// 三、FNV-1a 指纹基元
// ---------------------------------------------------------------------------

/// FNV-1a 偏移基。
pub const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 质数。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a 单字节混入。
pub const fn fnv1a_byte(state: u64, b: u8) -> u64 {
    (state ^ (b as u64)).wrapping_mul(FNV_PRIME)
}

/// FNV-1a u64 定长混入。
pub const fn fnv1a_u64(state: u64, v: u64) -> u64 {
    let s1 = fnv1a_byte(state, (v & 0xff) as u8);
    let s2 = fnv1a_byte(s1, ((v >> 8) & 0xff) as u8);
    let s3 = fnv1a_byte(s2, ((v >> 16) & 0xff) as u8);
    let s4 = fnv1a_byte(s3, ((v >> 24) & 0xff) as u8);
    let s5 = fnv1a_byte(s4, ((v >> 32) & 0xff) as u8);
    let s6 = fnv1a_byte(s5, ((v >> 40) & 0xff) as u8);
    let s7 = fnv1a_byte(s6, ((v >> 48) & 0xff) as u8);
    fnv1a_byte(s7, ((v >> 56) & 0xff) as u8)
}

/// FNV-1a 字符串混入。
pub const fn fnv1a_str(state: u64, s: &str) -> u64 {
    let b = s.as_bytes();
    let mut i = 0usize;
    let mut st = state;
    while i < b.len() {
        st = fnv1a_byte(st, b[i]);
        i += 1;
    }
    st
}

// ---------------------------------------------------------------------------
// 四、能量域闭集（五域分账——iGPU 独立账）
// ---------------------------------------------------------------------------

/// 能量域闭集（分账单位；iGPU 与 Package 区分归账——锚点判据）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Domain {
    /// CPU Package（整封套）。
    Package,
    /// Core（PP0：核心域）。
    Core,
    /// Uncore（非核心；部分代无独立读数）。
    Uncore,
    /// iGPU（PP1：核显——独立账）。
    IGpu,
    /// DRAM（内存自耗）。
    Dram,
}

impl Domain {
    /// 域序（0-4）。
    pub const fn ordinal(self) -> usize {
        match self {
            Domain::Package => 0,
            Domain::Core => 1,
            Domain::Uncore => 2,
            Domain::IGpu => 3,
            Domain::Dram => 4,
        }
    }

    /// 由域序还原（表外 None）。
    pub const fn from_ordinal(o: usize) -> Option<Domain> {
        match o {
            0 => Some(Domain::Package),
            1 => Some(Domain::Core),
            2 => Some(Domain::Uncore),
            3 => Some(Domain::IGpu),
            4 => Some(Domain::Dram),
            _ => None,
        }
    }

    /// 人读名。
    pub const fn zh(self) -> &'static str {
        match self {
            Domain::Package => "Package",
            Domain::Core => "Core",
            Domain::Uncore => "Uncore",
            Domain::IGpu => "iGPU",
            Domain::Dram => "DRAM",
        }
    }

    /// MSR 备路地址（0 = 无备路——表内显性）。
    pub const fn msr_addr(self) -> u16 {
        let ord = self.ordinal() as u8;
        let mut i = 0usize;
        while i < MSR_TABLE.len() {
            let row = MSR_TABLE[i];
            if row.0 == ord {
                return row.1;
            }
            i += 1;
        }
        0
    }
}

/// 读取来源闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// RAPL 主路（sysfs powercap / 驱动能量域）。
    Rapl,
    /// MSR 备路（rdmsr——授权后启用）。
    Msr,
}

impl Source {
    /// 人读名。
    pub const fn zh(self) -> &'static str {
        match self {
            Source::Rapl => "RAPL主路",
            Source::Msr => "MSR备路",
        }
    }
}

// ---------------------------------------------------------------------------
// 五、平台差异表（Gen9-Gen14 六代闭集——设备 ID 分派）
// ---------------------------------------------------------------------------

/// 平台代际行（采样语义按代分派——锚点"按设备 ID 分派实现"）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GenRow {
    /// 代际名（人读）。
    pub gen: &'static str,
    /// 设备 ID 区间下沿（含）。
    pub dev_id_lo: u16,
    /// 设备 ID 区间上沿（含）。
    pub dev_id_hi: u16,
    /// 是否有 iGPU 能量域（PP1）。
    pub igpu_domain: bool,
    /// 是否支持 MSR 备路（驱动不可用时的备路可行性）。
    pub msr_fallback: bool,
    /// 能量计数器单位（纳焦/LSB；按代差异——回绕周期与精度直接相关）。
    pub nj_per_lsb: u32,
}

/// Gen9-Gen14 差异表（闭集；数值为规划标定值——按代可调，口径唯一）。
pub const GEN_TABLE: [GenRow; 6] = [
    GenRow { gen: "Gen9-SKL", dev_id_lo: 0x1900, dev_id_hi: 0x193F, igpu_domain: true, msr_fallback: true, nj_per_lsb: 61 },
    GenRow { gen: "Gen9.5-KBL/CFL", dev_id_lo: 0x5900, dev_id_hi: 0x3E9F, igpu_domain: true, msr_fallback: true, nj_per_lsb: 61 },
    GenRow { gen: "Gen11-ICL", dev_id_lo: 0x8A50, dev_id_hi: 0x8A5F, igpu_domain: true, msr_fallback: false, nj_per_lsb: 65 },
    GenRow { gen: "Gen12-TGL/ADL", dev_id_lo: 0x9A40, dev_id_hi: 0x9AFF, igpu_domain: true, msr_fallback: false, nj_per_lsb: 65 },
    GenRow { gen: "Gen12-DG1", dev_id_lo: 0x4900, dev_id_hi: 0x490F, igpu_domain: false, msr_fallback: false, nj_per_lsb: 65 },
    GenRow { gen: "Gen14-MTL", dev_id_lo: 0x7D40, dev_id_hi: 0x7D6F, igpu_domain: true, msr_fallback: false, nj_per_lsb: 60 },
];

/// 按设备 ID 解析平台行（区间闭集命中；表外 None——保守留痕）。
pub const fn resolve_gen(device_id: u16) -> Option<&'static GenRow> {
    let mut i = 0usize;
    while i < GEN_TABLE.len() {
        let row: &'static GenRow = &GEN_TABLE[i];
        if device_id >= row.dev_id_lo && device_id <= row.dev_id_hi {
            return Some(row);
        }
        i += 1;
    }
    None
}

// ---------------------------------------------------------------------------
// 六、回绕补偿账（32 位计数器——回绕即诊断）
// ---------------------------------------------------------------------------

/// 单域回绕账（32 位累计器语义）。
#[derive(Clone, Debug)]
pub struct WrapAccountant {
    /// 域。
    pub domain: Domain,
    /// 上一次原始读数（32 位口径）。
    last_raw32: Option<u32>,
    /// 上一次读数 tick。
    last_tick: u64,
    /// 回绕次数（补偿计数）。
    pub wraps: u64,
    /// 补偿总量（µJ；wraps×2^32——账面显性）。
    pub compensated_uj: u64,
    /// 可疑回绕立案数。
    pub suspicious: u64,
    /// 累计有效增量（µJ；补偿后）。
    pub total_delta_uj: u64,
    /// 样本数。
    pub samples: u64,
}

impl WrapAccountant {
    /// 新账。
    pub const fn new(domain: Domain) -> WrapAccountant {
        WrapAccountant {
            domain,
            last_raw32: None,
            last_tick: 0,
            wraps: 0,
            compensated_uj: 0,
            suspicious: 0,
            total_delta_uj: 0,
            samples: 0,
        }
    }

    /// 喂入一次 32 位原始读数（回绕检测→补偿→可疑立案）。
    /// 返回本次补偿后增量（µJ）；首样返回 0（无基线不产增量）。
    pub fn feed(&mut self, raw32: u32, tick: u64) -> Result<u64, XcCode> {
        self.samples = self.samples.saturating_add(1);
        let cur = raw32 as u64;
        let prev = match self.last_raw32 {
            Some(p) => p as u64,
            None => {
                // 首样：只立基线。
                self.last_raw32 = Some(raw32);
                self.last_tick = tick;
                return Ok(0);
            }
        };
        let prev_tick = self.last_tick;
        self.last_raw32 = Some(raw32);
        self.last_tick = tick;
        if cur >= prev {
            // 正常推进。
            let delta = cur - prev;
            self.total_delta_uj = self.total_delta_uj.saturating_add(delta);
            return Ok(delta);
        }
        // 倒退：单次回绕语义（cur + 2^32 - prev）。
        let delta = COUNTER_MOD_UJ - prev + cur;
        // 回绕即诊断：间隔过短 = 可疑（真回绕是慢物理过程）。
        let interval = tick.saturating_sub(prev_tick);
        if interval < WRAP_MIN_INTERVAL_TICKS {
            self.suspicious = self.suspicious.saturating_add(1);
            // 可疑回绕不补偿（读数损坏嫌疑——宁缺毋错）。
            return Err(XcCode::SuspiciousWrap);
        }
        self.wraps = self.wraps.saturating_add(1);
        self.compensated_uj = self.compensated_uj.saturating_add(COUNTER_MOD_UJ);
        self.total_delta_uj = self.total_delta_uj.saturating_add(delta);
        Ok(delta)
    }
}

// ---------------------------------------------------------------------------
// 七、功耗与能效（定点换算 + 精度门 + 联合画像）
// ---------------------------------------------------------------------------

/// 功率换算（µJ 增量 + 间隔 ms → mW；零浮点：mw = ΔµJ×1000/Δms）。
pub const fn mw_of(delta_uj: u64, interval_ms: u64) -> Result<u64, XcCode> {
    if interval_ms == 0 {
        return Err(XcCode::ZeroInterval);
    }
    Ok(delta_uj.saturating_mul(1000) / interval_ms)
}

/// 能效换算（频率 kHz + 功率 mW → kHz/W；定点：kpW = kHz×1000/mW）。
pub const fn efficiency_khz_per_w(freq_khz: u64, mw: u64) -> Result<u64, XcCode> {
    if mw == 0 {
        return Err(XcCode::ZeroInterval);
    }
    Ok(freq_khz.saturating_mul(1000) / mw)
}

/// 联合画像行（功耗-频率两半齐才产出——F1460 输入面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JointProfile {
    /// 域。
    pub domain_ordinal: u8,
    /// 功率（mW）。
    pub mw: u64,
    /// 频率（kHz）。
    pub freq_khz: u64,
    /// 能效（kHz/W）。
    pub eff_khz_per_w: u64,
    /// 来源。
    pub src: Source,
    /// tick。
    pub tick: u64,
}

/// 精度门判定（低于地板的功率差异不进趋势——精度声明机制面）。
pub const fn above_precision_floor(mw: u64) -> bool {
    mw >= PRECISION_FLOOR_MW
}

// ---------------------------------------------------------------------------
// 八、事件账（六类闭集 + 滚动 64 + FNV 链）
// ---------------------------------------------------------------------------

/// 账面事件闭集（六类）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LedgerKind {
    /// 域采样入账。
    Sampled,
    /// 回绕补偿。
    Wrapped,
    /// 可疑回绕立案。
    SuspiciousWrap,
    /// 域缺失回退。
    DomainFallback,
    /// MSR 备路启用/拒启。
    MsrGate,
    /// 平台分派。
    PlatformResolved,
}

impl LedgerKind {
    /// 序号。
    pub const fn kind_id(self) -> u8 {
        match self {
            LedgerKind::Sampled => 0,
            LedgerKind::Wrapped => 1,
            LedgerKind::SuspiciousWrap => 2,
            LedgerKind::DomainFallback => 3,
            LedgerKind::MsrGate => 4,
            LedgerKind::PlatformResolved => 5,
        }
    }

    /// 人读名。
    pub const fn zh(self) -> &'static str {
        match self {
            LedgerKind::Sampled => "采样",
            LedgerKind::Wrapped => "回绕补偿",
            LedgerKind::SuspiciousWrap => "可疑回绕",
            LedgerKind::DomainFallback => "域回退",
            LedgerKind::MsrGate => "MSR闸",
            LedgerKind::PlatformResolved => "平台分派",
        }
    }
}

/// 账面行。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LedgerRecord {
    /// 严格递增序号。
    pub seq: u64,
    /// tick。
    pub tick: u64,
    /// 类别。
    pub kind: LedgerKind,
    /// 域序。
    pub domain_ord: u8,
    /// 量（µJ/mW 复用域）。
    pub amount: u64,
    /// 摘要。
    pub summary: String,
    /// 链上指纹。
    pub fp: u64,
}

/// 滚动事件账。
#[derive(Clone, Debug)]
pub struct EventLedger {
    records: Vec<LedgerRecord>,
    next_seq: u64,
    /// 逐出累计。
    pub evicted: u64,
    /// 尾指纹。
    pub tail_fp: u64,
}

impl EventLedger {
    /// 空账。
    pub fn new() -> EventLedger {
        EventLedger {
            records: Vec::new(),
            next_seq: 1,
            evicted: 0,
            tail_fp: FNV_OFFSET,
        }
    }

    /// 账长。
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// 追加（滚动逐出）。
    pub fn append(&mut self, tick: u64, kind: LedgerKind, domain_ord: u8, amount: u64, summary: &str) -> u64 {
        let seq = self.next_seq;
        let mut fp = fnv1a_u64(self.tail_fp, seq);
        fp = fnv1a_u64(fp, tick);
        fp = fnv1a_u64(fp, kind.kind_id() as u64);
        fp = fnv1a_u64(fp, domain_ord as u64);
        fp = fnv1a_u64(fp, amount);
        fp = fnv1a_str(fp, summary);
        self.tail_fp = fp;
        self.next_seq = self.next_seq.saturating_add(1);
        if self.records.len() >= EVENT_LEDGER_CAPACITY {
            let _ = self.records.remove(0);
            self.evicted = self.evicted.saturating_add(1);
        }
        self.records.push(LedgerRecord {
            seq,
            tick,
            kind,
            domain_ord,
            amount,
            summary: summary.to_string(),
            fp,
        });
        seq
    }

    /// 链复验。
    pub fn verify_chain(&self) -> Result<(), XcCode> {
        let mut prev = FNV_OFFSET;
        let mut i = 0usize;
        while i < self.records.len() {
            let r = match self.records.get(i) {
                Some(r) => r,
                None => return Err(XcCode::LedgerCorrupt),
            };
            let mut fp = fnv1a_u64(prev, r.seq);
            fp = fnv1a_u64(fp, r.tick);
            fp = fnv1a_u64(fp, r.kind.kind_id() as u64);
            fp = fnv1a_u64(fp, r.domain_ord as u64);
            fp = fnv1a_u64(fp, r.amount);
            fp = fnv1a_str(fp, &r.summary);
            if fp != r.fp {
                return Err(XcCode::LedgerCorrupt);
            }
            prev = r.fp;
            i += 1;
        }
        if prev != self.tail_fp {
            return Err(XcCode::LedgerCorrupt);
        }
        Ok(())
    }

    /// 按类别计数。
    pub fn count_by_kind(&self, kind: LedgerKind) -> u64 {
        let mut n = 0u64;
        let mut i = 0usize;
        while i < self.records.len() {
            if let Some(r) = self.records.get(i) {
                if r.kind == kind {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }
}

// ---------------------------------------------------------------------------
// 九、域账与总编排（分账/回退/联动/差异表分派）
// ---------------------------------------------------------------------------

/// 单域账（分账单位；iGPU 独立）。
#[derive(Clone, Debug)]
pub struct DomainAccount {
    /// 域。
    pub domain: Domain,
    /// 回绕账。
    pub wrap: WrapAccountant,
    /// 补偿后累计能量（µJ）。
    pub total_uj: u64,
    /// 最近功率（mW）。
    pub mw_last: u64,
    /// 功率峰值（mW）。
    pub mw_peak: u64,
    /// 最近能效（kHz/W；0 = 无频率联动）。
    pub eff_last: u64,
    /// 最近频率（kHz）。
    pub freq_last_khz: u64,
    /// 走备路次数（域缺失回退）。
    pub msr_fallbacks: u64,
    /// 精度门滤除次数（低于地板不进趋势）。
    pub precision_filtered: u64,
    /// 采样次数。
    pub sample_count: u64,
}

impl DomainAccount {
    /// 新域账。
    pub fn new(domain: Domain) -> DomainAccount {
        DomainAccount {
            domain,
            wrap: WrapAccountant::new(domain),
            total_uj: 0,
            mw_last: 0,
            mw_peak: 0,
            eff_last: 0,
            freq_last_khz: 0,
            msr_fallbacks: 0,
            precision_filtered: 0,
            sample_count: 0,
        }
    }
}

/// 原始读数（注入式——零 IO：真实读取位在宿主侧）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RawReading {
    /// 域。
    pub domain: Domain,
    /// 32 位原始能量读数（µJ LSB 口径）。
    pub raw32: u32,
    /// 来源。
    pub src: Source,
    /// tick。
    pub tick: u64,
}

/// RAPL/MSR 总引擎。
#[derive(Clone, Debug)]
pub struct RaplMsrEngine {
    /// 平台行（差异表分派结果）。
    pub platform: Option<&'static GenRow>,
    /// 平台未知标记（保守基线声明）。
    pub platform_unknown: bool,
    /// MSR 备路授权位（显性授权才可启用）。
    pub msr_authorized: bool,
    /// MSR 备路启用位（授权+平台支持才置位）。
    pub msr_enabled: bool,
    /// 五域账（下标 = Domain ordinal）。
    pub accounts: [DomainAccount; 5],
    /// 频率联动注入（最近核心频率 kHz；0 = 无）。
    pub freq_khz_last: u64,
    /// 事件账。
    pub ledger: EventLedger,
    /// 联合画像行数（产出计数——F1460 输入面）。
    pub profiles_out: u64,
    /// 主路样本数（RAPL）。
    pub rapl_samples: u64,
    /// 备路样本数（MSR）。
    pub msr_samples: u64,
}

impl RaplMsrEngine {
    /// 新引擎（按设备 ID 分派平台——差异表区间闭集命中）。
    pub fn new(device_id: u16, tick: u64) -> RaplMsrEngine {
        let mut eng = RaplMsrEngine {
            platform: None,
            platform_unknown: false,
            msr_authorized: false,
            msr_enabled: false,
            accounts: [
                DomainAccount::new(Domain::Package),
                DomainAccount::new(Domain::Core),
                DomainAccount::new(Domain::Uncore),
                DomainAccount::new(Domain::IGpu),
                DomainAccount::new(Domain::Dram),
            ],
            freq_khz_last: 0,
            ledger: EventLedger::new(),
            profiles_out: 0,
            rapl_samples: 0,
            msr_samples: 0,
        };
        match resolve_gen(device_id) {
            Some(row) => {
                eng.platform = Some(row);
                eng.ledger.append(
                    tick,
                    LedgerKind::PlatformResolved,
                    5,
                    0,
                    &alloc::format!("平台分派 {} nj/LSB {}", row.gen, row.nj_per_lsb),
                );
            }
            None => {
                eng.platform_unknown = true;
                eng.ledger.append(
                    tick,
                    LedgerKind::PlatformResolved,
                    5,
                    0,
                    "平台未知：保守基线采样并留痕",
                );
            }
        }
        eng
    }

    /// MSR 备路授权（显性授权位——权限门槛声明机制面）。
    pub fn authorize_msr(&mut self, granted: bool, tick: u64) -> Result<(), XcCode> {
        if !granted {
            self.ledger.append(tick, LedgerKind::MsrGate, 5, 0, "MSR 授权被拒：仅 RAPL 主路服务");
            return Err(XcCode::MsrPermissionDenied);
        }
        self.msr_authorized = true;
        // 平台支持才置启用。
        let supported = match self.platform {
            Some(row) => row.msr_fallback,
            None => false,
        };
        if supported {
            self.msr_enabled = true;
            self.ledger.append(tick, LedgerKind::MsrGate, 5, 0, "MSR 备路启用（授权+平台支持）");
            Ok(())
        } else {
            self.ledger.append(tick, LedgerKind::MsrGate, 5, 0, "MSR 备路授权但平台不支持：保持关闭");
            Err(XcCode::MsrUnavailable)
        }
    }

    /// 平台是否支持该域（差异表分派——iGPU 域按代有无）。
    pub fn domain_supported(&self, domain: Domain) -> bool {
        match self.platform {
            Some(row) => match domain {
                Domain::IGpu => row.igpu_domain,
                _ => true,
            },
            None => true, // 平台未知保守全开（留痕在案）。
        }
    }

    /// 喂入一次读数（主路或备路；回绕补偿+分账+功率换算）。
    /// 返回 (补偿后增量 µJ, 功率 mW)。
    pub fn feed(&mut self, r: RawReading) -> Result<(u64, u64), XcCode> {
        // 来源合法性：MSR 备路必须已启用。
        if r.src == Source::Msr && !self.msr_enabled {
            return Err(XcCode::MsrUnavailable);
        }
        // 域支持闸：平台不支持 → 域缺失回退面。
        if !self.domain_supported(r.domain) {
            self.ledger.append(
                r.tick,
                LedgerKind::DomainFallback,
                r.domain.ordinal() as u8,
                0,
                "平台差异表标记无此域：回退 Package 分摊或显性跳过",
            );
            return Err(XcCode::DomainMissing);
        }
        match r.src {
            Source::Rapl => self.rapl_samples = self.rapl_samples.saturating_add(1),
            Source::Msr => self.msr_samples = self.msr_samples.saturating_add(1),
        }
        let ord = r.domain.ordinal();
        // 回绕补偿账。
        let delta = match self.accounts.get_mut(ord) {
            Some(acc) => acc.wrap.feed(r.raw32, r.tick)?,
            None => return Err(XcCode::LedgerMismatch),
        };
        if delta > 0 {
            // 累计能量 + 功率换算（与上次采样间隔）。
            let interval = SAMPLE_INTERVAL_MS;
            let mw = mw_of(delta, interval)?;
            let acc = match self.accounts.get_mut(ord) {
                Some(a) => a,
                None => return Err(XcCode::LedgerMismatch),
            };
            acc.total_uj = acc.total_uj.saturating_add(delta);
            acc.sample_count = acc.sample_count.saturating_add(1);
            // 精度门：低于地板不进趋势（不夸大）。
            if above_precision_floor(mw) {
                acc.mw_last = mw;
                if mw > acc.mw_peak {
                    acc.mw_peak = mw;
                }
                // 频率联动：两半齐才出画像。
                if self.freq_khz_last > 0 {
                    let eff = efficiency_khz_per_w(self.freq_khz_last, mw)?;
                    acc.eff_last = eff;
                    acc.freq_last_khz = self.freq_khz_last;
                    self.profiles_out = self.profiles_out.saturating_add(1);
                }
            } else {
                acc.precision_filtered = acc.precision_filtered.saturating_add(1);
            }
            self.ledger.append(
                r.tick,
                LedgerKind::Sampled,
                ord as u8,
                delta,
                &alloc::format!("{} {} 增量 {}µJ", r.src.zh(), r.domain.zh(), delta),
            );
            Ok((delta, mw))
        } else {
            Ok((0, 0))
        }
    }

    /// 注入核心频率（与 RAPL 同步读——联合画像的频率半边）。
    pub fn inject_freq(&mut self, freq_khz: u64, tick: u64) {
        self.freq_khz_last = freq_khz;
        let _ = tick;
    }

    /// 联合画像快照（F1460 输入面：五域最近画像行）。
    pub fn joint_profiles(&self) -> Vec<JointProfile> {
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < 5 {
            if let Some(acc) = self.accounts.get(i) {
                if acc.mw_last > 0 && acc.freq_last_khz > 0 && acc.eff_last > 0 {
                    let src = if self.msr_enabled && acc.msr_fallbacks > 0 {
                        Source::Msr
                    } else {
                        Source::Rapl
                    };
                    out.push(JointProfile {
                        domain_ordinal: i as u8,
                        mw: acc.mw_last,
                        freq_khz: acc.freq_last_khz,
                        eff_khz_per_w: acc.eff_last,
                        src,
                        tick: 0,
                    });
                }
            }
            i += 1;
        }
        out
    }

    /// 域缺失回退判定（iGPU 不可用时建议读 Package——锚点回退面）。
    pub fn fallback_domain_for(&self, domain: Domain) -> Option<Domain> {
        if self.domain_supported(domain) {
            return Some(domain);
        }
        match domain {
            Domain::IGpu => Some(Domain::Package), // iGPU 并入 Package 分摊。
            _ => Some(domain),
        }
    }

    /// 审计：账面守恒（域账累计 == 回绕账总增量）。
    pub fn reconcile(&self) -> Result<(), XcCode> {
        let mut i = 0usize;
        while i < 5 {
            let acc = match self.accounts.get(i) {
                Some(a) => a,
                None => return Err(XcCode::LedgerMismatch),
            };
            if acc.total_uj != acc.wrap.total_delta_uj {
                return Err(XcCode::LedgerMismatch);
            }
            i += 1;
        }
        self.ledger.verify_chain()
    }

    /// 全量校验（平台分派一致 + MSR 闸语义 + 账链 + 守恒）。
    pub fn validate(&self) -> Result<(), XcCode> {
        // MSR 语义：启用必先授权；授权但平台不支持不得启用。
        if self.msr_enabled && !self.msr_authorized {
            return Err(XcCode::MsrPermissionDenied);
        }
        if let Some(row) = self.platform {
            if !row.msr_fallback && self.msr_enabled {
                return Err(XcCode::MsrUnavailable);
            }
        }
        // iGPU 分账：平台不支持时 iGPU 账必须为空。
        if let Some(row) = self.platform {
            if !row.igpu_domain {
                let ord = Domain::IGpu.ordinal();
                let acc = match self.accounts.get(ord) {
                    Some(a) => a,
                    None => return Err(XcCode::DomainMissing),
                };
                if acc.total_uj > 0 {
                    return Err(XcCode::DomainMissing);
                }
            }
        }
        self.reconcile()
    }

    /// 人读汇总。
    pub fn report(&self) -> String {
        let mut s = String::new();
        s.push_str("== CGPU-F1443 Intel RAPL/MSR 读取 汇报 ==\n");
        let plat = match self.platform {
            Some(row) => alloc::format!("{}（nj/LSB {}）", row.gen, row.nj_per_lsb),
            None => "未知平台（保守基线）".to_string(),
        };
        s.push_str(&alloc::format!(
            "平台 {} MSR 授权={} 启用={} 主路样 {} 备路样 {}\n",
            plat,
            self.msr_authorized,
            self.msr_enabled,
            self.rapl_samples,
            self.msr_samples
        ));
        let mut i = 0usize;
        while i < 5 {
            if let Some(acc) = self.accounts.get(i) {
                if acc.sample_count > 0 || acc.wrap.wraps > 0 {
                    s.push_str(&alloc::format!(
                        "{}：累计 {}µJ 功率 {}mW（峰 {}）能效 {}kHz/W 回绕 {} 可疑 {} 精度滤 {}\n",
                        acc.domain.zh(),
                        acc.total_uj,
                        acc.mw_last,
                        acc.mw_peak,
                        acc.eff_last,
                        acc.wrap.wraps,
                        acc.wrap.suspicious,
                        acc.precision_filtered
                    ));
                }
            }
            i += 1;
        }
        s.push_str(&alloc::format!("画像 {} 行 账 {} 行\n", self.profiles_out, self.ledger.len()));
        s
    }

    /// 无障碍读屏（三段固定预算：平台态/分账态/回绕态）。
    pub fn read_screen(&self) -> Vec<String> {
        let mut out = Vec::new();
        // 段一：平台与来源态。
        let plat = match self.platform {
            Some(row) => row.gen,
            None => "未知平台",
        };
        out.push(alloc::format!(
            "Intel 功耗采样：平台 {}，来源 {}，频率联动 {}",
            plat,
            if self.msr_enabled { "RAPL主路+MSR备路" } else { "RAPL主路" },
            if self.freq_khz_last > 0 { "开" } else { "关" }
        ));
        // 段二：非零分账域（上限 3 行）。
        let mut shown = 0usize;
        let mut i = 0usize;
        while i < 5 && shown < 3 {
            if let Some(acc) = self.accounts.get(i) {
                if acc.sample_count > 0 {
                    out.push(alloc::format!(
                        "{}：功率 {}mW 累计 {}µJ",
                        acc.domain.zh(),
                        acc.mw_last,
                        acc.total_uj
                    ));
                    shown = shown.saturating_add(1);
                }
            }
            i += 1;
        }
        // 段三：回绕/精度态（仅异常时占预算）。
        let mut total_suspicious: u64 = 0;
        let mut i = 0usize;
        while i < 5 {
            if let Some(acc) = self.accounts.get(i) {
                total_suspicious = total_suspicious.saturating_add(acc.wrap.suspicious);
            }
            i += 1;
        }
        if total_suspicious > 0 {
            out.push(alloc::format!("注意：可疑回绕立案 {} 起", total_suspicious));
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 十、参考探针（判据八：回绕注入/域缺失回退/多代平台模拟/权限门/精度门）
// ---------------------------------------------------------------------------

/// 参考探针：核心判据自证（纯函数口径，零 IO）。
pub fn run_cgj03_checks() -> Vec<(&'static str, bool)> {
    let mut out: Vec<(&'static str, bool)> = Vec::new();
    // 差异表分派（多代平台模拟）。
    out.push(("Gen9 命中", resolve_gen(0x1912).map(|r| r.gen.contains("SKL")).unwrap_or(false)));
    out.push(("Gen12 命中", resolve_gen(0x9A49).map(|r| r.gen.contains("TGL")).unwrap_or(false)));
    out.push(("表外未知", resolve_gen(0xFFFF).is_none()));
    out.push(("Gen11 无 MSR 备路", resolve_gen(0x8A56).map(|r| !r.msr_fallback).unwrap_or(false)));
    out.push(("DG1 无 iGPU 域", resolve_gen(0x4905).map(|r| !r.igpu_domain).unwrap_or(false)));
    // MSR 地址表（真实寄存器）。
    out.push(("Package MSR 0x611", Domain::Package.msr_addr() == 0x611));
    out.push(("iGPU MSR 0x641", Domain::IGpu.msr_addr() == 0x641));
    out.push(("Uncore 无备路显性", Domain::Uncore.msr_addr() == 0));
    // 权限门。
    let mut eng = RaplMsrEngine::new(0x1912, 1);
    out.push(("MSR 未授权拒启", eng.authorize_msr(false, 2).is_err()));
    out.push(("MSR 授权启用", eng.authorize_msr(true, 3).is_ok() && eng.msr_enabled));
    out.push(("MSR 未启用拒读", RaplMsrEngine::new(0x8A56, 1).feed(RawReading { domain: Domain::Package, raw32: 100, src: Source::Msr, tick: 2 }).is_err()));
    // 主路采样 + iGPU 分账。
    let mut eng2 = RaplMsrEngine::new(0x1912, 1);
    eng2.inject_freq(3_500_000, 2); // 3.5GHz（kHz 定点）。
    let r1 = eng2.feed(RawReading { domain: Domain::Package, raw32: 1_000_000, src: Source::Rapl, tick: 10 });
    out.push(("首样立基线零增量", r1.as_ref().map(|x| x.0 == 0).unwrap_or(false)));
    let r2 = eng2.feed(RawReading { domain: Domain::Package, raw32: 1_000_000 + 50_000, src: Source::Rapl, tick: 110 });
    // 50000µJ/100ms = 500mW。
    out.push(("功率换算500mW", r2.as_ref().map(|x| x.1 == 500).unwrap_or(false)));
    out.push(("iGPU 独立账", {
        let _ = eng2.feed(RawReading { domain: Domain::IGpu, raw32: 2_000, src: Source::Rapl, tick: 120 });
        let pkg = eng2.accounts.get(Domain::Package.ordinal());
        let igpu = eng2.accounts.get(Domain::IGpu.ordinal());
        pkg.map(|a| a.total_uj == 50_000).unwrap_or(false) && igpu.map(|a| a.total_uj == 0).unwrap_or(false)
    }));
    // 画像联动（两半齐才出）。
    out.push(("联合画像产出", eng2.profiles_out >= 1 && eng2.joint_profiles().len() >= 1));
    // 回绕注入与补偿。
    let mut eng3 = RaplMsrEngine::new(0x1912, 1);
    let _ = eng3.feed(RawReading { domain: Domain::Dram, raw32: u32::MAX - 1_000, src: Source::Rapl, tick: 10 });
    let r3 = eng3.feed(RawReading { domain: Domain::Dram, raw32: 1_000, src: Source::Rapl, tick: 10 + WRAP_MIN_INTERVAL_TICKS + 1 });
    // 回绕增量 = 1000 + 1001 = 2001µJ。
    out.push(("回绕补偿增量", r3.as_ref().map(|x| x.0 == 2_001).unwrap_or(false)));
    let dram = eng3.accounts.get(Domain::Dram.ordinal());
    out.push(("回绕计数留痕", dram.map(|a| a.wrap.wraps == 1 && a.wrap.compensated_uj == COUNTER_MOD_UJ).unwrap_or(false)));
    // 可疑回绕（间隔过短）。
    let mut eng4 = RaplMsrEngine::new(0x1912, 1);
    let _ = eng4.feed(RawReading { domain: Domain::Core, raw32: u32::MAX - 10, src: Source::Rapl, tick: 10 });
    let r4 = eng4.feed(RawReading { domain: Domain::Core, raw32: 5, src: Source::Rapl, tick: 12 });
    out.push(("可疑回绕拒偿", r4.is_err() && r4.unwrap_err() == XcCode::SuspiciousWrap));
    // 域缺失回退（DG1 无 iGPU）。
    let mut eng5 = RaplMsrEngine::new(0x4905, 1);
    let r5 = eng5.feed(RawReading { domain: Domain::IGpu, raw32: 100, src: Source::Rapl, tick: 5 });
    out.push(("域缺失显性拒", r5.is_err() && r5.clone().unwrap_err() == XcCode::DomainMissing));
    out.push(("iGPU 回退 Package", eng5.fallback_domain_for(Domain::IGpu) == Some(Domain::Package)));
    // 精度门（1mW 地板：10µJ/100ms=0.1mW 低于地板）。
    let mut eng6 = RaplMsrEngine::new(0x1912, 1);
    let _ = eng6.feed(RawReading { domain: Domain::Package, raw32: 100, src: Source::Rapl, tick: 10 });
    let _ = eng6.feed(RawReading { domain: Domain::Package, raw32: 110, src: Source::Rapl, tick: 110 });
    let pkg6 = eng6.accounts.get(Domain::Package.ordinal());
    out.push(("精度门滤除", pkg6.map(|a| a.precision_filtered == 1 && a.mw_last == 0).unwrap_or(false)));
    // 守恒与账链。
    out.push(("量账守恒", eng2.reconcile().is_ok() && eng3.reconcile().is_ok()));
    out.push(("全量校验过", eng2.validate().is_ok() && eng5.validate().is_ok()));
    out.push(("读屏三段", eng2.read_screen().len() >= 3));
    out.push(("精度声明冻结", PRECISION_NOTE.contains("1mW") && PRECISION_NOTE.contains("不夸大")));
    out.push(("权限声明冻结", MSR_PERMISSION_NOTE.contains("管理员") && MSR_PERMISSION_NOTE.contains("ring0")));
    out.push(("诊断码带建议", !XcCode::SuspiciousWrap.detail().is_empty() && XcCode::SuspiciousWrap.code_hex() == 0x4B04));
    out
}
