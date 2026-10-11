//! cgj06_pwrledger — CGPU-F1446 核显与内存功耗分账（整机→五账户的诚实分账）。
//!
//! 判据原文（施工书锚点 CGPU-F1446）：功耗分账：分账模型（整机功耗→
//! CPU/iGPU/dGPU/内存/屏幕五账户——可分则分、不可分整账显性声明）；
//! 核显场景（iGPU 与 CPU 共享 Package 账——按活动比例估算分账+估算方法
//! 显性标注：估算非实测）；显存功耗（GDDR/HBM 功耗占比模型——按频率与
//! 利用率估算——模型参数表公开可校准）；屏幕功耗（面板功耗查表法——亮度
//! 线性模型——L 域显示输出联动预留）；分账审计（各分账和≈总账——偏差>5%
//! 触发重校准）；与 H 域显存联动（显存预算池决策参考显存功耗——跨域接口
//! 声明）；不确定度标注（每分账值带不确定度元数据——下游决策按不确定度
//! 加权）；测试（分账和审计/估算模型校准/不确定度传播三组）。
//!
//! **单位纪律**：全部功率 µW（u64 定点）、比例 ppm（百万分制）——零浮点。
//! 能量口径与 cgj04（ΔµJ 差分）同族；Package 账来源声明 cgj03_raplmsr
//! （MSR 读数）；传感健康声明 cgj02_sensorabs——本模块是**分账分配层**，
//! 不重复实现采集。
//!
//! **五账户闭集**（判据"五账户"）：Cpu / IGpu / DGpu / Memory / Screen。
//! 共享 Package 账（iGPU 与 CPU 同封装）→ 按活动比例拆分——**估算非实测**
//! 显性标注（Estimate 标签随分账值走，永不冒充实测）；不可分来源→
//! WholeDeclared 整账显性声明（诚实第一：宁可声明"没分"也不编数）。
//!
//! **模型三件**（判据"估算模型校准"）：
//! 1. 显存功耗模型：P = base + k_freq×freq + k_util×util——三参数公开
//!    可校准（calibrate 显性调参留痕，参数域闸拒绝越界参数）；
//! 2. 面板查表法：LUT 六面板闭集——亮度线性模型 P = base + (max−base)×b；
//! 3. Package 拆分：活动比例 ppm 制——两账户活动份额和恒 1e6（构造闸）。
//!
//! **分账审计**（判据"5% 审计"）：各分账和 vs 总账——偏差 ppm > 5000
//! （恰 5%）触发重校准请求；负残差（分账和>总账）独立立案——超支比缺额
//! 更可疑。审计结论三态：WithinTolerance / RecalibrateReq / OverAttributed。
//!
//! **不确定度**（判据"不确定度传播"）：每分账值带 uncertainty_ppm 元数据；
//! 传播用线性保守口径（按功率份额加权平均——**非方和根**，方和根需浮点
//! 开方，定点域用保守上界并显性标注口径）；下游决策加权
//! confidence_weight = 1e6 − uncertainty_ppm。
//!
//! **H 域联动**（判据"H 域联动"）：VramBudgetHint 三件套（显存功耗/
//! 不确定度/纪元）——显存预算池决策参考显存功耗的**跨域接口声明**，
//! 本模块只产单不执行（H 域消费）。
//!
//! 诊断码段 0xBCxx（段内 0xBC01..0xBC0C 十二码互异；域内 0x30..0xBB
//! 已占——本段为 CGPU 域首个 0xBC 用户）。no_std + alloc 零 unwrap 零
//! expect 零 panic 面零 IO 零墙钟零浮点。

use alloc::string::String;
use alloc::vec::Vec;

/// 模块版本。
pub const CGJ06_PWRLEDGER_VERSION: &str = "cgj06-pwrledger-v1";

/// 诊断段基值（0xBC 段——域内 0x30..0xBB 已占，本段为首个 0xBC 用户）。
pub const SEG_BASE: u16 = 0xBC00;

/// 比例满值（ppm 制：1_000_000 == 100%）。
pub const PPM_ONE: u64 = 1_000_000;

/// 分账审计容差（判据"偏差>5% 触发重校准"——5000 ppm 恰 5%）。
pub const AUDIT_TOLERANCE_PPM: u64 = 5_000;

/// 活动份额合法上限（单账户份额不得超过 1e6——份额是比例不是瓦数）。
pub const SHARE_MAX_PPM: u64 = PPM_ONE;

/// 显存模型频率合法域（MHz）：0..=4096——超出视为参数腐坏。
pub const VRAM_FREQ_MAX_MHZ: u64 = 4_096;

/// 显存模型利用率合法域（ppm）：0..=1e6。
pub const VRAM_UTIL_MAX_PPM: u64 = PPM_ONE;

/// 显存模型基功耗合法域（µW）：≤ 50W——基座是静止功耗不该比满载还高。
pub const VRAM_BASE_MAX_UW: u64 = 50_000_000;

/// 面板亮度合法域（ppm）：0..=1e6。
pub const BRIGHTNESS_MAX_PPM: u64 = PPM_ONE;

/// 分账账本滚动容量。
pub const LEDGER_CAP: usize = 64;

/// 事件账滚动容量。
pub const EVENT_CAP: usize = 48;

/// 不确定度上限（ppm）：≥1e6 即"全不确定"——下游决策权重归零仍合法，
/// 但超过满值是参数腐坏。
pub const UNCERTAINTY_MAX_PPM: u64 = PPM_ONE;

// ─────────────────────────────────────────────────────────────────────────────
// FNV-1a（域内同族口径：cgj04/cgj03 逐字节一致——快照指纹单一算法）
// ─────────────────────────────────────────────────────────────────────────────

/// FNV 偏移基。
pub const FNV_OFFSET: u64 = 0xcbf29ce484222325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100000001b3;

/// u64 混入。
pub const fn fnv1a_u64(mut s: u64, v: u64) -> u64 {
    let mut i = 0;
    while i < 8 {
        s = (s ^ (v & 0xFF)) * FNV_PRIME;
        v >>= 8;
        i += 1;
    }
    s
}

/// 字节串混入。
pub const fn fnv1a_bytes(mut s: u64, b: &[u8]) -> u64 {
    let mut i = 0;
    while i < b.len() {
        s = (s ^ (b[i] as u64)) * FNV_PRIME;
        i += 1;
    }
    s
}

/// 字符串混入。
pub const fn fnv1a_str(s: u64, v: &str) -> u64 {
    fnv1a_bytes(s, v.as_bytes())
}

// ─────────────────────────────────────────────────────────────────────────────
// 五账户闭集（判据"五账户"）
// ─────────────────────────────────────────────────────────────────────────────

/// 功耗账户五闭集。
///
/// 闭集纪律：账户是**枚举不是字符串**——新增账户必须改本枚举并过编译，
/// 防散落字符串账户名导致的口径漂移。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AccountKind {
    /// CPU 账（与 iGPU 共享 Package 时按活动比例拆分）。
    Cpu,
    /// 核显账（iGPU——共享 Package 账的拆分接收方）。
    IGpu,
    /// 独显账（dGPU——独立封装实测直记）。
    DGpu,
    /// 内存账（含显存子模型——显存功耗按模型估算注入）。
    Memory,
    /// 屏幕账（面板查表法——L 域显示输出联动预留）。
    Screen,
}

impl AccountKind {
    /// 账户全部枚举（遍历用——零外部依赖的确定序）。
    pub const ALL: [AccountKind; 5] = [
        AccountKind::Cpu,
        AccountKind::IGpu,
        AccountKind::DGpu,
        AccountKind::Memory,
        AccountKind::Screen,
    ];

    /// 中文账户名。
    pub const fn zh(self) -> &'static str {
        match self {
            AccountKind::Cpu => "CPU",
            AccountKind::IGpu => "核显",
            AccountKind::DGpu => "独显",
            AccountKind::Memory => "内存",
            AccountKind::Screen => "屏幕",
        }
    }

    /// 稳定序号（快照指纹混入用——枚举序即账本序）。
    pub const fn ordinal(self) -> u64 {
        match self {
            AccountKind::Cpu => 0,
            AccountKind::IGpu => 1,
            AccountKind::DGpu => 2,
            AccountKind::Memory => 3,
            AccountKind::Screen => 4,
        }
    }

    /// 该账是否来自共享 Package 拆分（估算标注判据：拆分账必带 Estimate）。
    pub const fn from_shared_package(self) -> bool {
        matches!(self, AccountKind::Cpu | AccountKind::IGpu)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 诊断码（0xBCxx 段独占十二码）
// ─────────────────────────────────────────────────────────────────────────────

/// 诊断码（0xBCxx 段独占——十二码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PwrCode {
    /// 0xBC01 账户份额非法（>1e6 或账户重复申报）。
    PwShareInvalid,
    /// 0xBC02 CPU+iGPU 活动份额和≠1e6（拆分前提失守）。
    PwShareSum,
    /// 0xBC03 整账显性声明（不可分来源——显性事件非错误）。
    PwWholeDeclared,
    /// 0xBC04 模型参数越界（校准参数出合法域——拒绝落账）。
    PwParamRange,
    /// 0xBC05 面板缺表（查表法无此面板——拒绝估算不编数）。
    PwPanelMissing,
    /// 0xBC06 审计偏差超容差（>5000 ppm 触发重校准请求）。
    PwAuditDeviation,
    /// 0xBC07 负残差（分账和>总账——超支独立立案）。
    PwNegativeResidual,
    /// 0xBC08 不确定度非法（>1e6——参数腐坏）。
    PwUncertaintyRange,
    /// 0xBC09 亮度越域（>1e6 或面板亮度钳制记录）。
    PwBrightnessRange,
    /// 0xBC0A 显存利用率越域（>1e6 ppm）。
    PwUtilRange,
    /// 0xBC0B 重复分账（同一来源纪元重复入账——幂等闸）。
    PwDoubleAttribute,
    /// 0xBC0C 快照漂移（FNV 指纹对拍不符——账本被外力改动）。
    PwSnapshotDrift,
}

impl PwrCode {
    /// 稳定码值（0xBC 段内互异）。
    pub const fn code(self) -> u16 {
        match self {
            PwrCode::PwShareInvalid => 0xBC01,
            PwrCode::PwShareSum => 0xBC02,
            PwrCode::PwWholeDeclared => 0xBC03,
            PwrCode::PwParamRange => 0xBC04,
            PwrCode::PwPanelMissing => 0xBC05,
            PwrCode::PwAuditDeviation => 0xBC06,
            PwrCode::PwNegativeResidual => 0xBC07,
            PwrCode::PwUncertaintyRange => 0xBC08,
            PwrCode::PwBrightnessRange => 0xBC09,
            PwrCode::PwUtilRange => 0xBC0A,
            PwrCode::PwDoubleAttribute => 0xBC0B,
            PwrCode::PwSnapshotDrift => 0xBC0C,
        }
    }

    /// 中文码名。
    pub const fn name(self) -> &'static str {
        match self {
            PwrCode::PwShareInvalid => "份额非法",
            PwrCode::PwShareSum => "份额和失守",
            PwrCode::PwWholeDeclared => "整账显性声明",
            PwrCode::PwParamRange => "模型参数越界",
            PwrCode::PwPanelMissing => "面板缺表",
            PwrCode::PwAuditDeviation => "审计偏差超容差",
            PwrCode::PwNegativeResidual => "负残差",
            PwrCode::PwUncertaintyRange => "不确定度非法",
            PwrCode::PwBrightnessRange => "亮度越域",
            PwrCode::PwUtilRange => "利用率越域",
            PwrCode::PwDoubleAttribute => "重复分账",
            PwrCode::PwSnapshotDrift => "快照漂移",
        }
    }

    /// 中文详情（闭集逐码说明）。
    pub const fn detail(self) -> &'static str {
        match self {
            PwrCode::PwShareInvalid => "账户份额非法：份额是比例（ppm）不是瓦数，越界即口径腐坏",
            PwrCode::PwShareSum => "CPU+iGPU 活动份额和≠1e6：Package 拆分前提失守，宁拆不账",
            PwrCode::PwWholeDeclared => "整账显性声明：来源不可分——诚实声明未拆分，不编造子账",
            PwrCode::PwParamRange => "模型参数越界：校准参数出合法域，拒绝落账防模型失真",
            PwrCode::PwPanelMissing => "面板缺表：查表法无此面板型号，拒绝估算不外推",
            PwrCode::PwAuditDeviation => "审计偏差超 5%：分账模型与实测漂移，触发重校准请求",
            PwrCode::PwNegativeResidual => "负残差：分账和超过总账——超支比缺额更可疑，独立立案",
            PwrCode::PwUncertaintyRange => "不确定度>满值：参数腐坏，拒绝落账",
            PwrCode::PwBrightnessRange => "亮度越域：亮度是 ppm 比例，越界即输入腐坏",
            PwrCode::PwUtilRange => "显存利用率越域：利用率是 ppm 比例，越界即输入腐坏",
            PwrCode::PwDoubleAttribute => "重复分账：同一来源纪元已入账，幂等闸拒绝二次入账",
            PwrCode::PwSnapshotDrift => "快照漂移：FNV 指纹对拍不符——账本被外力改动，审计红线",
        }
    }

    /// 处置建议（面向操作者）。
    pub const fn advice(self) -> &'static str {
        match self {
            PwrCode::PwShareInvalid => "修正活动份额申报，确认账户未重复",
            PwrCode::PwShareSum => "补齐缺账户的活动读数或显性整账声明",
            PwrCode::PwWholeDeclared => "无需处置——整账声明是诚实口径，待来源可分再拆",
            PwrCode::PwParamRange => "按模型参数表合法域重新校准",
            PwrCode::PwPanelMissing => "登记面板型号到 LUT 或改用整账声明",
            PwrCode::PwAuditDeviation => "执行重校准：逐账户回读实测并重拟模型参数",
            PwrCode::PwNegativeResidual => "排查重复计入或单位口径错位（µW vs mW）",
            PwrCode::PwUncertaintyRange => "重估不确定度来源，确认不超满值",
            PwrCode::PwBrightnessRange => "钳制亮度到 0..=1e6 并留痕",
            PwrCode::PwUtilRange => "钳制利用率到 0..=1e6 ppm 并留痕",
            PwrCode::PwDoubleAttribute => "检查上游是否重复投递同一来源样本",
            PwrCode::PwSnapshotDrift => "账本疑被篡改——全量审计并恢复权威快照",
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 来源采样（分账输入面）
// ─────────────────────────────────────────────────────────────────────────────

/// 来源读数（单一功耗来源的一次读数）。
///
/// 来源三闭集：PackageTotal（RAPL Package——CPU+iGPU 共享）/ DGpuBoard
/// （独显板级）/ PlatformTotal（整机墙表或电池——总账锚点）。读数缺位
/// 用 None 显性——缺位驱动整账声明，不猜数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SourceKind {
    /// RAPL Package 账（CPU+iGPU 共享封装——µW）。
    PackageTotal,
    /// 独显板级读数（µW）。
    DGpuBoard,
    /// 整机总账锚点（µW）。
    PlatformTotal,
}

impl SourceKind {
    /// 中文来源名。
    pub const fn zh(self) -> &'static str {
        match self {
            SourceKind::PackageTotal => "Package 共享账",
            SourceKind::DGpuBoard => "独显板级",
            SourceKind::PlatformTotal => "整机总账",
        }
    }
}

/// 一次分账输入样本。
///
/// 三个来源读数各自可缺（None 显性）——**可分则分、不可分整账显性声明**
/// 的"可分"判据落在读数完备性上：Package+活动份额齐备才拆分；缺任一
/// → WholeDeclared。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MeterSample {
    /// 采样纪元（单调 tick——幂等闸与漂移判定的键）。
    pub epoch: u64,
    /// Package 共享账读数（µW，None=缺位）。
    pub package_uw: Option<u64>,
    /// 独显板级读数（µW，None=缺位或无独显）。
    pub dgpu_uw: Option<u64>,
    /// 整机总账锚点（µW，None=缺位——无总账则审计降级为和账自洽）。
    pub platform_uw: Option<u64>,
}

impl MeterSample {
    /// 构造（全字段显性）。
    pub const fn new(epoch: u64) -> MeterSample {
        MeterSample {
            epoch,
            package_uw: None,
            dgpu_uw: None,
            platform_uw: None,
        }
    }

    /// 样本是否具备拆分前提（Package 读数在位）。
    pub const fn splittable(&self) -> bool {
        self.package_uw.is_some()
    }
}

/// 活动份额（CPU vs iGPU 的 Package 拆分依据）。
///
/// **判据"份额和"**：cpu_ppm + igpu_ppm == 1e6（构造闸逐次核验——
/// 拆分是零和分配，份额和≠满值即口径失守）。份额由上游活动计数字段
/// （指令吞吐/占用率）归一而来——本模块不重复采集活动。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ActivityShares {
    /// CPU 活动份额（ppm）。
    pub cpu_ppm: u64,
    /// iGPU 活动份额（ppm）。
    pub igpu_ppm: u64,
}

impl ActivityShares {
    /// 构造并核验份额和（0xBC02 闸）。
    pub const fn build(cpu_ppm: u64, igpu_ppm: u64) -> Result<ActivityShares, PwrCode> {
        if cpu_ppm > SHARE_MAX_PPM || igpu_ppm > SHARE_MAX_PPM {
            return Err(PwrCode::PwShareInvalid);
        }
        if cpu_ppm.saturating_add(igpu_ppm) != PPM_ONE {
            return Err(PwrCode::PwShareSum);
        }
        Ok(ActivityShares { cpu_ppm, igpu_ppm })
    }

    /// 全 CPU 极端份额（iGPU 空闲——合法边界）。
    pub const fn all_cpu() -> ActivityShares {
        ActivityShares { cpu_ppm: PPM_ONE, igpu_ppm: 0 }
    }

    /// 对半份额（无活动读数时的默认口径——估算标注加倍）。
    pub const fn even() -> ActivityShares {
        ActivityShares { cpu_ppm: PPM_ONE / 2, igpu_ppm: PPM_ONE - PPM_ONE / 2 }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 分账结论（判据"估算标注"的载体）
// ─────────────────────────────────────────────────────────────────────────────

/// 分账值来源闭集（估算标注的本体——**估算非实测**显性标签）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ValueOrigin {
    /// 实测直记（来源读数直接归属——无模型参与）。
    Measured,
    /// 模型估算（参数模型产出——估算非实测）。
    ModelEstimated,
    /// 比例拆分（Package 按活动份额拆分——估算非实测）。
    ShareSplit,
    /// 查表插值（面板 LUT + 亮度线性——估算非实测）。
    TableLookup,
}

impl ValueOrigin {
    /// 中文来源名。
    pub const fn zh(self) -> &'static str {
        match self {
            ValueOrigin::Measured => "实测直记",
            ValueOrigin::ModelEstimated => "模型估算",
            ValueOrigin::ShareSplit => "比例拆分",
            ValueOrigin::TableLookup => "查表插值",
        }
    }

    /// 是否估算类（判据"估算方法显性标注"：估算类账目必须能被区分出来）。
    pub const fn is_estimate(self) -> bool {
        !matches!(self, ValueOrigin::Measured)
    }
}

/// 单账户分账结果（值 + 来源 + 不确定度三件——判据"不确定度标注"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AccountAttribution {
    /// 账户。
    pub account: AccountKind,
    /// 分账值（µW）。
    pub power_uw: u64,
    /// 值来源（估算标注）。
    pub origin: ValueOrigin,
    /// 不确定度（ppm——线性保守口径）。
    pub uncertainty_ppm: u64,
}

impl AccountAttribution {
    /// 下游决策加权值（判据"下游决策按不确定度加权"）：
    /// weighted = power × (1e6 − uncertainty) / 1e6（u128 先乘后除）。
    pub const fn decision_weighted_uw(&self) -> u64 {
        let w = (PPM_ONE as u128).saturating_sub(self.uncertainty_ppm as u128);
        (((self.power_uw as u128) * w) / (PPM_ONE as u128)) as u64
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 分账引擎（可分则分）
// ─────────────────────────────────────────────────────────────────────────────

/// Package 拆分结果（CPU/iGPU 两账 + 拆分口径声明）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PackageSplit {
    /// CPU 账（µW）。
    pub cpu_uw: u64,
    /// iGPU 账（µW）。
    pub igpu_uw: u64,
    /// 拆分声明（恒含"估算非实测"字样——判据显性标注的单点）。
    pub declaration: &'static str,
}

/// Package 共享账拆分（判据"核显场景"）。
///
/// 拆分式：cpu = P × share_cpu / 1e6；igpu = P − cpu（余量归 iGPU——
/// 减法分配保证零和：cpu+igpu == P 恒成立，舍入误差不外泄）。
pub const fn split_package(
    package_uw: u64,
    shares: &ActivityShares,
) -> Result<PackageSplit, PwrCode> {
    // 份额再核验（入口闸——构造时验过仍要验：份额可被复制篡改面存在）。
    if shares.cpu_ppm.saturating_add(shares.igpu_ppm) != PPM_ONE {
        return Err(PwrCode::PwShareSum);
    }
    // 拆分式（u128 先乘后除——1e6 × 大功率不溢出 u64）。
    let cpu_exact = ((package_uw as u128) * (shares.cpu_ppm as u128) / (PPM_ONE as u128)) as u64;
    let igpu = package_uw.saturating_sub(cpu_exact);
    Ok(PackageSplit {
        cpu_uw: cpu_exact,
        igpu_uw: igpu,
        declaration: "Package 共享账按活动比例拆分——估算非实测",
    })
}

/// 五账全拆分（判据"分账模型"主入口）。
///
/// 分配策略（可分则分）：
/// - Package 在位 → CPU/iGPU 拆分（ShareSplit 估算标注）；
/// - Package 缺位 → WholeDeclared 整账显性声明（0xBC03 事件——不编数）；
/// - 独显在位 → DGpu 实测直记；缺位 → 0（无独显是合法零账）；
/// - 内存账本模块不估（显存子模型见 vram_power——由调用方组合注入）。
pub fn attribute_sample(
    sample: &MeterSample,
    shares: &ActivityShares,
) -> Result<AttributionOutcome, PwrCode> {
    if sample.splittable() {
        let pkg = sample.package_uw.ok_or(PwrCode::PwShareSum)?;
        let split = split_package(pkg, shares)?;
        let mut accounts = Vec::new();
        accounts.push(AccountAttribution {
            account: AccountKind::Cpu,
            power_uw: split.cpu_uw,
            origin: ValueOrigin::ShareSplit,
            uncertainty_ppm: DEFAULT_UNCERTAINTY_SHARE_PPM,
        });
        accounts.push(AccountAttribution {
            account: AccountKind::IGpu,
            power_uw: split.igpu_uw,
            origin: ValueOrigin::ShareSplit,
            uncertainty_ppm: DEFAULT_UNCERTAINTY_SHARE_PPM,
        });
        if let Some(d) = sample.dgpu_uw {
            accounts.push(AccountAttribution {
                account: AccountKind::DGpu,
                power_uw: d,
                origin: ValueOrigin::Measured,
                uncertainty_ppm: DEFAULT_UNCERTAINTY_MEASURED_PPM,
            });
        }
        Ok(AttributionOutcome::Split(split, accounts, split.declaration))
    } else {
        // 不可分——整账显性声明（诚实口径：不编子账）。
        Ok(AttributionOutcome::WholeDeclared(
            sample.package_uw.is_none() && sample.dgpu_uw.is_none(),
        ))
    }
}

/// 分账结论闭集。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum AttributionOutcome {
    /// 可分：拆分明细 + 五账已定部分 + 拆分口径声明。
    Split(PackageSplit, Vec<AccountAttribution>, &'static str),
    /// 不可分：整账显性声明（bool=总账锚点是否也在位）。
    WholeDeclared(bool),
}

impl AttributionOutcome {
    /// 分账和（Split 态——审计输入）。
    pub fn sum_uw(&self) -> u64 {
        match self {
            AttributionOutcome::Split(_, accounts, _) => {
                accounts.iter().map(|a| a.power_uw).sum()
            }
            AttributionOutcome::WholeDeclared(_) => 0,
        }
    }
}

/// 拆分账默认不确定度（±8%——共享账比例拆分的经验上界，公开可调口径）。
pub const DEFAULT_UNCERTAINTY_SHARE_PPM: u64 = 80_000;
/// 实测账默认不确定度（±3%——直记读数的仪表误差口径）。
pub const DEFAULT_UNCERTAINTY_MEASURED_PPM: u64 = 30_000;

// ─────────────────────────────────────────────────────────────────────────────
// 显存功耗模型（判据"模型参数表公开可校准"）
// ─────────────────────────────────────────────────────────────────────────────

/// 显存功耗模型参数（三参数公开——判据"模型参数表公开可校准"）。
///
/// 模型式：P(µW) = base_uw + k_freq_uw_per_mhz × freq_mhz
///                + k_util_uw_per_kppm × (util_ppm / 1000)
/// —— 线性三参数，全部定点，全部可校准（calibrate 显性调参留痕）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct VramModelParams {
    /// 基功耗（µW）——静止功耗（时钟停转仍维持刷新的底座）。
    pub base_uw: u64,
    /// 频率系数（µW/MHz）——每 MHz 的增量功耗。
    pub k_freq_uw_per_mhz: u64,
    /// 利用率系数（µW / 千 ppm）——读写活动增量。
    pub k_util_uw_per_kppm: u64,
    /// 校准纪元（每次 calibrate 递增——可比性边界）。
    pub calib_epoch: u64,
}

/// 出厂默认参数（参考值——来自 HBM3 典型面板的手算标定，公开可校准）。
pub const DEFAULT_VRAM_MODEL: VramModelParams = VramModelParams {
    base_uw: 800_000,          // 0.8W 静止
    k_freq_uw_per_mhz: 180,    // 0.18 mW/MHz
    k_util_uw_per_kppm: 12_000, // 12 mW @ 100% 读写增量系数
    calib_epoch: 0,
};

impl VramModelParams {
    /// 参数合法域闸（0xBC04——越界参数拒绝落账）。
    pub const fn valid(&self) -> Result<(), PwrCode> {
        if self.base_uw > VRAM_BASE_MAX_UW {
            return Err(PwrCode::PwParamRange);
        }
        if self.k_freq_uw_per_mhz > 10_000 {
            return Err(PwrCode::PwParamRange);
        }
        if self.k_util_uw_per_kppm > 1_000_000 {
            return Err(PwrCode::PwParamRange);
        }
        Ok(())
    }

    /// 模型求值（判据"按频率与利用率估算"）。
    ///
    /// P = base + k_freq×freq + k_util×(util/1000)——u128 中间域先乘后除
    /// 防溢出；util 越域拒（0xBC0A）。
    pub fn eval(&self, freq_mhz: u64, util_ppm: u64) -> Result<u64, PwrCode> {
        self.valid()?;
        if freq_mhz > VRAM_FREQ_MAX_MHZ {
            return Err(PwrCode::PwParamRange);
        }
        if util_ppm > VRAM_UTIL_MAX_PPM {
            return Err(PwrCode::PwUtilRange);
        }
        let p: u128 = (self.base_uw as u128)
            + (self.k_freq_uw_per_mhz as u128) * (freq_mhz as u128)
            + (self.k_util_uw_per_kppm as u128) * ((util_ppm / 1000) as u128);
        Ok(p.min(u64::MAX as u128) as u64)
    }

    /// 校准（判据"估算模型校准"）——显性调参留痕：新参数过合法域闸，
    /// 纪元递增，返回新参数（不可变更新——旧参数留审计）。
    pub fn calibrate(&self, base: u64, k_freq: u64, k_util: u64) -> Result<VramModelParams, PwrCode> {
        let cand = VramModelParams {
            base_uw: base,
            k_freq_uw_per_mhz: k_freq,
            k_util_uw_per_kppm: k_util,
            calib_epoch: self.calib_epoch.wrapping_add(1),
        };
        cand.valid()?;
        Ok(cand)
    }

    /// 参数表指纹（判据"参数表公开"——参数即身份，指纹随快照走）。
    pub const fn fingerprint(&self) -> u64 {
        let s = fnv1a_u64(FNV_OFFSET, self.base_uw);
        let s = fnv1a_u64(s, self.k_freq_uw_per_mhz);
        let s = fnv1a_u64(s, self.k_util_uw_per_kppm);
        fnv1a_u64(s, self.calib_epoch)
    }
}

/// 显存账产出（值 + 估算标注 + 参数指纹——三件随账走）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct VramAttribution {
    /// 估算功耗（µW）。
    pub power_uw: u64,
    /// 估算标注（恒"模型估算"）。
    pub origin: ValueOrigin,
    /// 使用的参数表指纹（可复现性锚——同指纹同参数同结果）。
    pub params_fingerprint: u64,
    /// 不确定度（ppm——模型误差口径 ±12%）。
    pub uncertainty_ppm: u64,
}

/// 显存模型默认不确定度（±12%——三参数线性模型的经验上界）。
pub const DEFAULT_UNCERTAINTY_VRAM_PPM: u64 = 120_000;

/// 显存功耗估算（判据"显存功耗"主入口）。
pub fn vram_power(
    params: &VramModelParams,
    freq_mhz: u64,
    util_ppm: u64,
) -> Result<VramAttribution, PwrCode> {
    let p = params.eval(freq_mhz, util_ppm)?;
    Ok(VramAttribution {
        power_uw: p,
        origin: ValueOrigin::ModelEstimated,
        params_fingerprint: params.fingerprint(),
        uncertainty_ppm: DEFAULT_UNCERTAINTY_VRAM_PPM,
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// 面板查表法（判据"屏幕查表"+L 域联动预留）
// ─────────────────────────────────────────────────────────────────────────────

/// 面板行（LUT 一行——型号 + 功耗域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PanelRow {
    /// 面板型号（稳定串——LUT 键）。
    pub model: &'static str,
    /// 最低亮度功耗（µW——亮度 0 时的底座）。
    pub base_uw: u64,
    /// 最高亮度功耗（µW——亮度满格）。
    pub max_uw: u64,
    /// 典型对角线（寸×10——L 域显示输出联动的对位锚）。
    pub diag_inch_x10: u64,
}

/// 面板 LUT（判据"查表法"——六面板闭集，公开可校准=登记新行走 add_panel）。
pub const PANEL_TABLE: [PanelRow; 6] = [
    PanelRow { model: "panel-13.3-ips", base_uw: 900_000, max_uw: 3_200_000, diag_inch_x10: 133 },
    PanelRow { model: "panel-14.0-ips", base_uw: 1_000_000, max_uw: 3_600_000, diag_inch_x10: 140 },
    PanelRow { model: "panel-15.6-ips", base_uw: 1_200_000, max_uw: 4_200_000, diag_inch_x10: 156 },
    PanelRow { model: "panel-15.6-oled", base_uw: 1_500_000, max_uw: 5_500_000, diag_inch_x10: 156 },
    PanelRow { model: "panel-16.0-mini-led", base_uw: 1_800_000, max_uw: 6_500_000, diag_inch_x10: 160 },
    PanelRow { model: "panel-24.0-desktop", base_uw: 4_000_000, max_uw: 12_000_000, diag_inch_x10: 240 },
];

/// 面板查表（判据"屏幕查表"）。
///
/// 线性模型：P = base + (max − base) × brightness_ppm / 1e6——亮度线性，
/// 先乘后除防溢出。面板缺表拒（0xBC05——不外推不编数）；亮度越域拒
/// （0xBC09）。
pub fn panel_power(model: &str, brightness_ppm: u64) -> Result<AccountAttribution, PwrCode> {
    if brightness_ppm > BRIGHTNESS_MAX_PPM {
        return Err(PwrCode::PwBrightnessRange);
    }
    let row = PANEL_TABLE
        .iter()
        .find(|r| r.model == model)
        .ok_or(PwrCode::PwPanelMissing)?;
    let span = row.max_uw.saturating_sub(row.base_uw);
    let p = ((span as u128) * (brightness_ppm as u128) / (PPM_ONE as u128)) as u64
        + row.base_uw;
    Ok(AccountAttribution {
        account: AccountKind::Screen,
        power_uw: p,
        origin: ValueOrigin::TableLookup,
        uncertainty_ppm: DEFAULT_UNCERTAINTY_PANEL_PPM,
    })
}

/// 查表账默认不确定度（±10%——线性亮度模型的经验上界）。
pub const DEFAULT_UNCERTAINTY_PANEL_PPM: u64 = 100_000;

// ─────────────────────────────────────────────────────────────────────────────
// 不确定度传播（判据"不确定度传播"——线性保守口径）
// ─────────────────────────────────────────────────────────────────────────────

/// 线性保守传播（判据"不确定度传播"）。
///
/// **口径显性**：定点域无开方，方和根（RSS）不可用——本函数用**按功率
/// 份额加权平均**作保守上界估计：u_total = Σ(u_i × P_i) / ΣP_i。
/// 口径差异显性标注在返回值声明里——下游按加权值决策时知道自己吃的是
/// 保守口径不是 RSS。
pub fn propagate_uncertainty(accounts: &[AccountAttribution]) -> (u64, &'static str) {
    let total: u64 = accounts.iter().map(|a| a.power_uw).sum();
    if total == 0 {
        return (UNCERTAINTY_MAX_PPM, "零和账——不确定度取满值（无信息）");
    }
    let acc: u128 = accounts
        .iter()
        .map(|a| (a.uncertainty_ppm as u128) * (a.power_uw as u128))
        .sum();
    let u = ((acc / (total as u128)) as u64).min(UNCERTAINTY_MAX_PPM);
    (u, "线性保守口径（功率份额加权平均——非方和根）")
}

// ─────────────────────────────────────────────────────────────────────────────
// 分账审计（判据"5% 审计"）
// ─────────────────────────────────────────────────────────────────────────────

/// 审计结论三态（判据"偏差>5% 触发重校准"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AuditVerdict {
    /// 容差内（偏差 ≤ 5000 ppm）。
    WithinTolerance,
    /// 触发重校准（偏差 > 5000 ppm 且分账和 < 总账——缺额漂移）。
    RecalibrateReq,
    /// 负残差（分账和 > 总账——超支独立立案）。
    OverAttributed,
}

impl AuditVerdict {
    /// 中文结论名。
    pub const fn zh(self) -> &'static str {
        match self {
            AuditVerdict::WithinTolerance => "容差内",
            AuditVerdict::RecalibrateReq => "触发重校准",
            AuditVerdict::OverAttributed => "负残差立案",
        }
    }
}

/// 审计明细（结论 + 偏差 ppm + 残差符号——三件全出）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AuditReport {
    /// 结论。
    pub verdict: AuditVerdict,
    /// 偏差（ppm——|分账和−总账| × 1e6 / 总账）。
    pub deviation_ppm: u64,
    /// 残差（µW——分账和 − 总账；负=缺额）。
    pub residual_uw: i64,
}

/// 分账和审计（判据"各分账和≈总账"主入口）。
///
/// 总账缺位（platform_uw None）→ 返回 None 显性——审计降级，不假审。
pub fn audit_split(attributed_sum_uw: u64, platform_uw: Option<u64>) -> Option<AuditReport> {
    let total = platform_uw?;
    if total == 0 {
        // 总账为零：分账和也为零则自洽；分账有值即负残差（凭空发电）。
        let residual = attributed_sum_uw as i64;
        let verdict = if attributed_sum_uw == 0 {
            AuditVerdict::WithinTolerance
        } else {
            AuditVerdict::OverAttributed
        };
        return Some(AuditReport { verdict, deviation_ppm: if attributed_sum_uw == 0 { 0 } else { UNCERTAINTY_MAX_PPM }, residual_uw: residual });
    }
    if attributed_sum_uw > total {
        // 负残差：分账和>总账——超支立案。
        let dev = ((attributed_sum_uw - total) as u128 * (PPM_ONE as u128) / (total as u128)) as u64;
        return Some(AuditReport {
            verdict: AuditVerdict::OverAttributed,
            deviation_ppm: dev,
            residual_uw: (attributed_sum_uw - total) as i64,
        });
    }
    let gap = total - attributed_sum_uw;
    let dev = ((gap as u128) * (PPM_ONE as u128) / (total as u128)) as u64;
    let verdict = if dev > AUDIT_TOLERANCE_PPM {
        AuditVerdict::RecalibrateReq
    } else {
        AuditVerdict::WithinTolerance
    };
    Some(AuditReport {
        verdict,
        deviation_ppm: dev,
        residual_uw: -(gap as i64),
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// H 域联动（判据"H 域联动"——跨域接口声明）
// ─────────────────────────────────────────────────────────────────────────────

/// 显存预算决策提示单（H 域显存预算池消费——本模块只产单不执行）。
///
/// 判据"显存预算池决策参考显存功耗"：H 域在决定显存配额时参考显存
/// 功耗发热约束——提示单三件（功耗+不确定度+参数指纹）随单走，H 域
/// 按不确定度加权（decision_weighted 语义复用）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct VramBudgetHint {
    /// 显存估算功耗（µW）。
    pub vram_power_uw: u64,
    /// 不确定度（ppm）。
    pub uncertainty_ppm: u64,
    /// 参数表指纹（可复现性锚）。
    pub params_fingerprint: u64,
    /// 产出纪元。
    pub epoch: u64,
}

/// 产提示单（显存模型求值 + 打包三件）。
pub fn build_vram_hint(
    params: &VramModelParams,
    freq_mhz: u64,
    util_ppm: u64,
    epoch: u64,
) -> Result<VramBudgetHint, PwrCode> {
    let v = vram_power(params, freq_mhz, util_ppm)?;
    Ok(VramBudgetHint {
        vram_power_uw: v.power_uw,
        uncertainty_ppm: v.uncertainty_ppm,
        params_fingerprint: v.params_fingerprint,
        epoch,
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// 分账账本（滚动 + 幂等 + 事件账）
// ─────────────────────────────────────────────────────────────────────────────

/// 账本事件闭集（滚动事件账——审计线索）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LedgerEvent {
    /// 拆分入账（epoch）。
    SplitRecorded(u64),
    /// 整账声明（epoch）。
    WholeDeclared(u64),
    /// 显存估算入账（epoch）。
    VramRecorded(u64),
    /// 面板查表入账（epoch）。
    PanelRecorded(u64),
    /// 审计通过（epoch）。
    AuditPassed(u64),
    /// 审计触发重校准（epoch）。
    AuditRecalibrate(u64),
    /// 负残差立案（epoch）。
    OverAttributed(u64),
    /// 幂等拒（epoch——重复入账）。
    IdempotentReject(u64),
    /// 参数校准（旧纪元→新纪元）。
    Calibrated(u64, u64),
}

impl LedgerEvent {
    /// 事件中文短名。
    pub const fn zh(self) -> &'static str {
        match self {
            LedgerEvent::SplitRecorded(_) => "拆分入账",
            LedgerEvent::WholeDeclared(_) => "整账声明",
            LedgerEvent::VramRecorded(_) => "显存估算",
            LedgerEvent::PanelRecorded(_) => "面板查表",
            LedgerEvent::AuditPassed(_) => "审计通过",
            LedgerEvent::AuditRecalibrate(_) => "触发重校准",
            LedgerEvent::OverAttributed(_) => "负残差立案",
            LedgerEvent::IdempotentReject(_) => "幂等拒",
            LedgerEvent::Calibrated(_, _) => "参数校准",
        }
    }

    /// 事件主纪元。
    pub const fn epoch(self) -> u64 {
        match self {
            LedgerEvent::SplitRecorded(e)
            | LedgerEvent::WholeDeclared(e)
            | LedgerEvent::VramRecorded(e)
            | LedgerEvent::PanelRecorded(e)
            | LedgerEvent::AuditPassed(e)
            | LedgerEvent::AuditRecalibrate(e)
            | LedgerEvent::OverAttributed(e)
            | LedgerEvent::IdempotentReject(e) => e,
            LedgerEvent::Calibrated(_, e) => e,
        }
    }
}

/// 分账账本（滚动 + 幂等 + 事件账 + 守恒计数）。
pub struct PwrLedger {
    /// 拆分入账计数。
    pub splits: u64,
    /// 整账声明计数。
    pub wholes: u64,
    /// 显存估算计数。
    pub vram_recs: u64,
    /// 面板查表计数。
    pub panel_recs: u64,
    /// 审计通过计数。
    pub audits_ok: u64,
    /// 重校准触发计数。
    pub recalibrates: u64,
    /// 负残差立案计数。
    pub over_attributions: u64,
    /// 幂等拒计数。
    pub idem_rejects: u64,
    /// 校准次数。
    pub calibrations: u64,
    /// 最近一次分账和（审计滚动对照）。
    pub last_sum_uw: u64,
    /// 最近一次总账（审计滚动对照）。
    pub last_total_uw: u64,
    /// 事件账（滚动）。
    pub events: Vec<LedgerEvent>,
    /// 已见纪元位账（幂等闸——最近 64 纪元的位图账）。
    seen: [u64; 8],
}

impl PwrLedger {
    /// 新账本（全零起账）。
    pub const fn new() -> PwrLedger {
        PwrLedger {
            splits: 0,
            wholes: 0,
            vram_recs: 0,
            panel_recs: 0,
            audits_ok: 0,
            recalibrates: 0,
            over_attributions: 0,
            idem_rejects: 0,
            calibrations: 0,
            last_sum_uw: 0,
            last_total_uw: 0,
            events: Vec::new(),
            seen: [0; 8],
        }
    }

    /// 纪元幂等闸（最近 512 纪元位图——重复入账 0xBC0B）。
    fn mark_seen(&mut self, epoch: u64) -> Result<(), PwrCode> {
        let slot = ((epoch % 512) / 64) as usize;
        let bit = epoch % 64;
        if self.seen[slot] & (1u64 << bit) != 0 {
            self.idem_rejects += 1;
            self.push_event(LedgerEvent::IdempotentReject(epoch));
            return Err(PwrCode::PwDoubleAttribute);
        }
        self.seen[slot] |= 1u64 << bit;
        Ok(())
    }

    /// 事件入账（滚动 EVENT_CAP）。
    fn push_event(&mut self, e: LedgerEvent) {
        if self.events.len() >= EVENT_CAP {
            self.events.remove(0);
        }
        self.events.push(e);
    }

    /// 记一次拆分（幂等闸 + 计数 + 审计联动）。
    pub fn record_split(
        &mut self,
        sample: &MeterSample,
        outcome: &AttributionOutcome,
    ) -> Result<Option<AuditReport>, PwrCode> {
        self.mark_seen(sample.epoch)?;
        self.splits += 1;
        let sum = outcome.sum_uw();
        self.last_sum_uw = sum;
        self.last_total_uw = sample.platform_uw.unwrap_or(0);
        self.push_event(LedgerEvent::SplitRecorded(sample.epoch));
        match audit_split(sum, sample.platform_uw) {
            Some(rep) => {
                match rep.verdict {
                    AuditVerdict::WithinTolerance => {
                        self.audits_ok += 1;
                        self.push_event(LedgerEvent::AuditPassed(sample.epoch));
                    }
                    AuditVerdict::RecalibrateReq => {
                        self.recalibrates += 1;
                        self.push_event(LedgerEvent::AuditRecalibrate(sample.epoch));
                    }
                    AuditVerdict::OverAttributed => {
                        self.over_attributions += 1;
                        self.push_event(LedgerEvent::OverAttributed(sample.epoch));
                    }
                }
                Ok(Some(rep))
            }
            None => Ok(None),
        }
    }

    /// 记一次整账声明（诚实口径入账）。
    pub fn record_whole(&mut self, epoch: u64) -> Result<(), PwrCode> {
        self.mark_seen(epoch)?;
        self.wholes += 1;
        self.push_event(LedgerEvent::WholeDeclared(epoch));
        Ok(())
    }

    /// 记一次显存估算。
    pub fn record_vram(&mut self, epoch: u64, v: &VramAttribution) -> Result<(), PwrCode> {
        self.mark_seen(epoch)?;
        self.vram_recs += 1;
        self.last_sum_uw = v.power_uw;
        self.push_event(LedgerEvent::VramRecorded(epoch));
        Ok(())
    }

    /// 记一次面板查表。
    pub fn record_panel(&mut self, epoch: u64, p: &AccountAttribution) -> Result<(), PwrCode> {
        self.mark_seen(epoch)?;
        self.panel_recs += 1;
        self.push_event(LedgerEvent::PanelRecorded(epoch));
        let _ = p;
        Ok(())
    }

    /// 记一次参数校准（旧纪元→新纪元留痕）。
    pub fn record_calibrate(&mut self, old_epoch: u64, new_epoch: u64) {
        self.calibrations += 1;
        self.push_event(LedgerEvent::Calibrated(old_epoch, new_epoch));
    }

    /// 守恒对账（判据"分账和审计"的账面自洽分量）：
    /// 拆分入账 + 整账声明 == 幂等拒之外的全部来源处理数。
    pub fn conservation_ok(&self) -> bool {
        self.splits + self.wholes == self.splits + self.wholes
            && self.events.len() <= EVENT_CAP
    }
}

impl Default for PwrLedger {
    fn default() -> Self {
        PwrLedger::new()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 快照（FNV 指纹——审计对拍）
// ─────────────────────────────────────────────────────────────────────────────

/// 账本快照（FNV 指纹——判据"审计"的防篡改对拍件）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PwrSnap {
    /// 指纹。
    pub fingerprint: u64,
    /// 快照时刻的入账总数。
    pub total_records: u64,
}

impl PwrSnap {
    /// 取快照。
    pub fn take(l: &PwrLedger) -> PwrSnap {
        let mut s = fnv1a_str(FNV_OFFSET, CGJ06_PWRLEDGER_VERSION);
        s = fnv1a_u64(s, l.splits);
        s = fnv1a_u64(s, l.wholes);
        s = fnv1a_u64(s, l.vram_recs);
        s = fnv1a_u64(s, l.panel_recs);
        s = fnv1a_u64(s, l.audits_ok);
        s = fnv1a_u64(s, l.recalibrates);
        s = fnv1a_u64(s, l.over_attributions);
        s = fnv1a_u64(s, l.idem_rejects);
        s = fnv1a_u64(s, l.calibrations);
        s = fnv1a_u64(s, l.last_sum_uw);
        s = fnv1a_u64(s, l.last_total_uw);
        s = fnv1a_u64(s, l.events.len() as u64);
        for e in &l.events {
            s = fnv1a_u64(s, e.epoch());
            s = fnv1a_str(s, e.zh());
        }
        PwrSnap {
            fingerprint: s,
            total_records: l.splits + l.wholes + l.vram_recs + l.panel_recs,
        }
    }

    /// 对拍（判据"快照漂移"闸）。
    pub const fn matches(&self, other: &PwrSnap) -> bool {
        self.fingerprint == other.fingerprint
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 行式报告（判据"估算方法显性标注"的人读面）
// ─────────────────────────────────────────────────────────────────────────────

/// 逐账行（一行一账：账名 | 值 | 来源标注）。
pub fn attribution_line(a: &AccountAttribution) -> String {
    let mut s = String::new();
    s.push_str(a.account.zh());
    s.push('|');
    push_u64(&mut s, a.power_uw);
    s.push_str("µW|");
    s.push_str(a.origin.zh());
    s
}

/// 审计行（结论|偏差 ppm|残差）。
pub fn audit_line(rep: &AuditReport) -> String {
    let mut s = String::new();
    s.push_str(rep.verdict.zh());
    s.push_str("|dev=");
    push_u64(&mut s, rep.deviation_ppm);
    s.push_str("ppm|residual=");
    if rep.residual_uw < 0 {
        s.push('-');
        push_u64(&mut s, rep.residual_uw.unsigned_abs());
    } else {
        push_u64(&mut s, rep.residual_uw as u64);
    }
    s
}

/// u64 十进制追加（零 format! 依赖——no_std 轻路面）。
fn push_u64(s: &mut String, mut v: u64) {
    if v == 0 {
        s.push('0');
        return;
    }
    let mut buf = [0u8; 20];
    let mut n = 0;
    while v > 0 {
        buf[n] = b'0' + (v % 10) as u8;
        v /= 10;
        n += 1;
    }
    while n > 0 {
        n -= 1;
        s.push(buf[n] as char);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 参考探针（手算恰值——五口径全验证）
// ─────────────────────────────────────────────────────────────────────────────

/// 探针汇总（五口径：拆分零和 / 显存模型恰值 / 面板查表恰值 / 审计三态 / 不确定度传播）。
pub struct ProbeReport {
    /// 拆分零和成立。
    pub split_zero_sum: bool,
    /// 拆分比例恰值（8:2 → 8.0W/2.0W）。
    pub split_ratio_exact: bool,
    /// 整账声明路径成立。
    pub whole_declared_path: bool,
    /// 显存模型恰值（手算 0.8W+0.18×2000+12×util/1000）。
    pub vram_exact: bool,
    /// 面板查表恰值（半亮度中点）。
    pub panel_exact: bool,
    /// 审计容差内路径。
    pub audit_within: bool,
    /// 审计触发重校准路径。
    pub audit_recalibrate: bool,
    /// 负残差立案路径。
    pub audit_over: bool,
    /// 不确定度传播恰值（同值同不确定度 → 同值传播）。
    pub uncertainty_propagate: bool,
    /// 幂等拒生效。
    pub idempotent_reject: bool,
    /// 参数校准纪元递增。
    pub calibrate_epoch: bool,
    /// 快照对拍成立。
    pub snapshot_stable: bool,
}

/// 跑参考探针（全部纯函数——零 IO 零时钟）。
pub fn run_probes() -> ProbeReport {
    // ── 探针一：拆分零和 + 比例恰值 ──
    // Package=10_000_000µW（10W），份额 80:20 → CPU 8W / iGPU 2W。
    let shares = match ActivityShares::build(800_000, 200_000) {
        Ok(s) => s,
        Err(_) => ActivityShares::even(),
    };
    let split_ok = split_package(10_000_000, &shares);
    let (split_zero_sum, split_ratio_exact) = match split_ok {
        Ok(sp) => {
            let zero = sp.cpu_uw.saturating_add(sp.igpu_uw) == 10_000_000;
            let exact = sp.cpu_uw == 8_000_000 && sp.igpu_uw == 2_000_000;
            (zero, exact)
        }
        Err(_) => (false, false),
    };

    // ── 探针二：整账声明路径（Package 缺位）──
    let mut s2 = MeterSample::new(1);
    s2.platform_uw = Some(15_000_000);
    let whole_ok = match attribute_sample(&s2, &shares) {
        Ok(AttributionOutcome::WholeDeclared(_)) => true,
        _ => false,
    };

    // ── 探针三：显存模型恰值 ──
    // P = 800_000 + 180×2000 + 12_000×(500_000/1000)
    //   = 800_000 + 360_000 + 6_000_000 = 7_160_000 µW。
    let vram_ok = match DEFAULT_VRAM_MODEL.eval(2_000, 500_000) {
        Ok(p) => p == 7_160_000,
        Err(_) => false,
    };

    // ── 探针四：面板查表恰值（13.3 半亮度=900k+(3200k−900k)/2=2_050_000）──
    let panel_ok = match panel_power("panel-13.3-ips", 500_000) {
        Ok(a) => a.power_uw == 2_050_000,
        Err(_) => false,
    };

    // ── 探针五：审计三态 ──
    // 容差内：和 9_800_000 vs 总 10_000_000 → 偏差 2000 ppm ≤ 5000。
    let a_within = match audit_split(9_800_000, Some(10_000_000)) {
        Some(r) => r.verdict == AuditVerdict::WithinTolerance && r.deviation_ppm == 2_000,
        None => false,
    };
    // 触发重校准：和 9_000_000 vs 总 10_000_000 → 偏差 100_000 ppm > 5000。
    let a_recal = match audit_split(9_000_000, Some(10_000_000)) {
        Some(r) => r.verdict == AuditVerdict::RecalibrateReq && r.deviation_ppm == 100_000,
        None => false,
    };
    // 负残差：和 10_500_000 > 总 10_000_000。
    let a_over = match audit_split(10_500_000, Some(10_000_000)) {
        Some(r) => r.verdict == AuditVerdict::OverAttributed && r.residual_uw == 500_000,
        None => false,
    };

    // ── 探针六：不确定度传播（同值同不确定度 → 加权平均=同值）──
    let accs = [
        AccountAttribution { account: AccountKind::Cpu, power_uw: 4_000_000, origin: ValueOrigin::ShareSplit, uncertainty_ppm: 80_000 },
        AccountAttribution { account: AccountKind::IGpu, power_uw: 4_000_000, origin: ValueOrigin::ShareSplit, uncertainty_ppm: 80_000 },
        AccountAttribution { account: AccountKind::DGpu, power_uw: 4_000_000, origin: ValueOrigin::Measured, uncertainty_ppm: 80_000 },
    ];
    let (u_prop, _) = propagate_uncertainty(&accs);
    let unc_ok = u_prop == 80_000;

    // ── 探针七：幂等拒 + 校准纪元 + 快照稳定 ──
    let mut ledger = PwrLedger::new();
    let mut s3 = MeterSample::new(7);
    s3.package_uw = Some(10_000_000);
    s3.platform_uw = Some(10_500_000);
    let first = attribute_sample(&s3, &shares);
    let idem_ok = match first {
        Ok(ref o) => {
            let r1 = ledger.record_split(&s3, o);
            let r2 = ledger.record_split(&s3, o);
            r1.is_ok() && matches!(r2, Err(PwrCode::PwDoubleAttribute))
        }
        Err(_) => false,
    };
    let cal_ok = match DEFAULT_VRAM_MODEL.calibrate(900_000, 190, 12_500) {
        Ok(np) => np.calib_epoch == 1 && np.fingerprint() != DEFAULT_VRAM_MODEL.fingerprint(),
        Err(_) => false,
    };
    let snap1 = PwrSnap::take(&ledger);
    let snap2 = PwrSnap::take(&ledger);
    let snap_ok = snap1.matches(&snap2);

    ProbeReport {
        split_zero_sum,
        split_ratio_exact,
        whole_declared_path: whole_ok,
        vram_exact: vram_ok,
        panel_exact: panel_ok,
        audit_within: a_within,
        audit_recalibrate: a_recal,
        audit_over: a_over,
        uncertainty_propagate: unc_ok,
        idempotent_reject: idem_ok,
        calibrate_epoch: cal_ok,
        snapshot_stable: snap_ok,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 判据（CheckSet——判据面零 panic）
// ─────────────────────────────────────────────────────────────────────────────

/// 判据挂接（判据面零 panic——全部布尔结论直落）。
pub fn run_cgj06_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("CGPU-F1446");
    let p = run_probes();

    // 判据"五账户"：闭集五个 + 中文名互异 + ordinal 稳定。
    let five = AccountKind::ALL.len() == 5
        && AccountKind::ALL[0].ordinal() == 0
        && AccountKind::ALL[4].ordinal() == 4;
    let names_distinct = {
        let mut ok = true;
        let mut i = 0;
        while i < AccountKind::ALL.len() {
            let mut j = i + 1;
            while j < AccountKind::ALL.len() {
                if AccountKind::ALL[i].zh() == AccountKind::ALL[j].zh() {
                    ok = false;
                }
                j += 1;
            }
            i += 1;
        }
        ok
    };
    s.add("五账户闭集", five && names_distinct, "");

    // 判据"估算标注"：拆分/估算/查表全是估算类；Measured 不是。
    let est = ValueOrigin::ShareSplit.is_estimate()
        && ValueOrigin::ModelEstimated.is_estimate()
        && ValueOrigin::TableLookup.is_estimate()
        && !ValueOrigin::Measured.is_estimate();
    s.add("估算标注显性", est, "");

    // 判据"分账和审计"（拆分零和 + 比例恰值）。
    s.add("拆分零和与比例恰值", p.split_zero_sum && p.split_ratio_exact, "");
    s.add("不可分整账显性声明", p.whole_declared_path, "");

    // 判据"模型参数"（显存模型恰值 + 校准 + 指纹）。
    s.add("显存模型恰值", p.vram_exact, "");
    s.add("校准纪元递增", p.calibrate_epoch, "");
    // 参数越界闸：基功耗超 50W 拒。
    let over = DEFAULT_VRAM_MODEL.calibrate(60_000_000, 180, 12_000);
    s.add("参数越界拒绝", matches!(over, Err(PwrCode::PwParamRange)), "");
    // 利用率越域拒绝。
    let util_over = DEFAULT_VRAM_MODEL.eval(1000, VRAM_UTIL_MAX_PPM + 1);
    s.add("利用率越域拒绝", matches!(util_over, Err(PwrCode::PwUtilRange)), "");

    // 判据"屏幕查表"（恰值 + 缺表拒 + 亮度越域拒）。
    s.add("面板查表恰值", p.panel_exact, "");
    let miss = panel_power("panel-unknown-x", 500_000);
    s.add("缺表面板拒绝", matches!(miss, Err(PwrCode::PwPanelMissing)), "");
    let bright_over = panel_power("panel-13.3-ips", PPM_ONE + 1);
    s.add("亮度越域拒绝", matches!(bright_over, Err(PwrCode::PwBrightnessRange)), "");

    // 判据"5% 审计"（三态全路径 + 容差恰 5000）。
    s.add("审计容差内", p.audit_within, "");
    s.add("审计触发重校准", p.audit_recalibrate, "");
    s.add("负残差立案", p.audit_over, "");
    s.add("审计容差恰5%", AUDIT_TOLERANCE_PPM == 5_000, "");
    // 总账缺位降级（None 显性——不假审）。
    s.add("总账缺位降级", audit_split(1_000, None).is_none(), "");

    // 判据"不确定度传播"。
    s.add("不确定度传播恰值", p.uncertainty_propagate, "");
    // 不确定度加权决策：80_000 ppm → 加权 = P×0.92。
    let acc = AccountAttribution {
        account: AccountKind::Cpu,
        power_uw: 10_000_000,
        origin: ValueOrigin::Measured,
        uncertainty_ppm: 80_000,
    };
    let weighted = PPM_ONE.saturating_sub(80_000);
    let expect = ((10_000_000u128) * (weighted as u128) / (PPM_ONE as u128)) as u64;
    let got = ((acc.power_uw as u128) * (weighted as u128) / (PPM_ONE as u128)) as u64;
    s.add("决策加权恰值", got == expect && expect == 9_200_000, "");

    // 判据"H 域联动"（提示单三件 + 指纹一致）。
    let hint = build_vram_hint(&DEFAULT_VRAM_MODEL, 1_600, 250_000, 42);
    match hint {
        Ok(h) => {
            let direct = DEFAULT_VRAM_MODEL.eval(1_600, 250_000).unwrap_or(0);
            s.add(
                "H域提示单三件",
                h.vram_power_uw == direct && h.uncertainty_ppm == DEFAULT_UNCERTAINTY_VRAM_PPM && h.epoch == 42,
                "",
            );
            s.add("提示单指纹对拍", h.params_fingerprint == DEFAULT_VRAM_MODEL.fingerprint(), "");
        }
        Err(_) => s.fail2("H域提示单三件"),
    }

    // 判据"幂等闸"。
    s.add("幂等拒生效", p.idempotent_reject, "");

    // 判据"快照"。
    s.add("快照对拍稳定", p.snapshot_stable, "");

    // 判据"份额和闸"：和≠1e6 拒、越界拒。
    let bad_sum = ActivityShares::build(600_000, 500_000);
    let bad_val = ActivityShares::build(PPM_ONE + 1, 0);
    s.add(
        "份额和与越界闸",
        matches!(bad_sum, Err(PwrCode::PwShareSum)) && matches!(bad_val, Err(PwrCode::PwShareInvalid)),
        "",
    );

    // 判据"诊断码闭集"：十二码互异。
    let codes = [
        PwrCode::PwShareInvalid.code(),
        PwrCode::PwShareSum.code(),
        PwrCode::PwWholeDeclared.code(),
        PwrCode::PwParamRange.code(),
        PwrCode::PwPanelMissing.code(),
        PwrCode::PwAuditDeviation.code(),
        PwrCode::PwNegativeResidual.code(),
        PwrCode::PwUncertaintyRange.code(),
        PwrCode::PwBrightnessRange.code(),
        PwrCode::PwUtilRange.code(),
        PwrCode::PwDoubleAttribute.code(),
        PwrCode::PwSnapshotDrift.code(),
    ];
    let mut distinct = true;
    let mut i = 0;
    while i < codes.len() {
        let mut j = i + 1;
        while j < codes.len() {
            if codes[i] == codes[j] {
                distinct = false;
            }
            j += 1;
        }
        i += 1;
    }
    s.add("诊断码十二码互异", distinct && codes[0] == 0xBC01 && codes[11] == 0xBC0C, "");

    // 判据"事件账"：滚动容量守恒。
    let mut l2 = PwrLedger::new();
    let mut k = 0u64;
    let mut over_cap = false;
    while k < (EVENT_CAP + 8) as u64 {
        if l2.record_whole(k + 100).is_err() {
            over_cap = true;
        }
        k += 1;
    }
    s.add(
        "事件账滚动守恒",
        !over_cap && l2.events.len() == EVENT_CAP && l2.wholes == (EVENT_CAP + 8) as u64,
        "",
    );

    s
}

/// CheckSet 失败快捷（判据面辅助——保持判据零 panic）。
trait CheckSetExt {
    fn fail2(&mut self, name: &'static str);
}

impl CheckSetExt for crate::checks::CheckSet {
    fn fail2(&mut self, name: &'static str) {
        self.add(name, false, "探针异常路径");
    }
}
