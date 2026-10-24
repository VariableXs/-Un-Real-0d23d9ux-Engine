//! UNX-F1002 · TSC 特性探测（Invariant TSC 判定与降级声明）· 目标 300 行 · 已深化
//!
//! **锚点**：`docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II · Unxreal.md#UNX-F1002`
//!
//! **判据原文**：UNX-F1002-J1 特性判定正确（三环境 Invariant TSC 判定 10/10 次），
//! 非不变 TSC 降级声明 10/10 次，特性账入启动报告。
//!
//! ## 要点一：CPUID 注入口——零真实执行，全确定性
//!
//! 本模块绝不执行真实 CPUID/MSR 指令：CPUID 与 MSR 一律经可注入 trait
//! （`CpuidPort`/`MsrPort`）进入；默认实现 `SyntheticCpuid`/`SyntheticMsr`
//! 按三环境（物理机/hypervisor 虚拟机/Hyper-V 特定叶）确定性构造数据。
//! 判定器只对注入数据负责，注入什么判什么——零 panic 面、零墙钟
//! （时间一律逻辑 tick u64）、零浮点（千分比偏差全整数运算）。
//!
//! ## 要点二：Invariant TSC 判定位与频率叶
//!
//! 判定位：CPUID leaf 0x80000007 EDX[8]（Invariant TSC）；联动位：
//! leaf 0x1 ECX[24]（TSC-Deadline）；频率叶：leaf 0x15（EBX=分母/EAX=分子
//! 的核晶振比 + ECX=晶振频率 Hz）与 leaf 0x16（EAX=基础 TSC MHz）。
//! 三位三叶全部经注入口采样，叶缺失走诊断（不猜）。
//!
//! ## 要点三：三环境判定闭集与 10/10 对拍账
//!
//! 环境闭集三档：物理机（leaf 0x1 ECX[31]=0）/hypervisor 虚拟机
//! （ECX[31]=1 且 0x40000000 签名 KVMKVMKVM）/Hyper-V 特定叶
//! （0x40000001 EAX="Hv#1"=0x31237648）。每环境十轮独立判定，逐轮与
//! 确定性真值对拍，10/10 全中才判"特性判定正确"；任一轮翻转立案。
//! 默认构造中 Hyper-V 合成环境为非不变 TSC——降级声明路径由此常绿。
//!
//! ## 要点四：非不变 TSC 降级声明三档
//!
//! 档位闭集：不变可用（InvariantUsable）/仅校准可用（CalibOnlyUsable，
//! TSC 在走但跨核不同步，只可在校准修正后用）/不可用（Unusable，特性
//! 缺席，时间基座必须回退 PIT/HPET 语义模型）。每档带降级原因码与对
//! 时间基座的影响注记；非不变环境每轮强制出声明，10/10 落账。
//!
//! ## 要点五：频率校准三源一致性对拍（千分比整数运算）
//!
//! 三源：① CPUID 比值（leaf 0x15/0x16）；② 派生自 PIT 语义模型
//! （1.193182 MHz 基频 + 分频器 + 窗内 PIT 周计数 → 窗长 µs → TSC 增量
//! ÷ 窗长 = 频率）；③ 注入口给定值。三源两两千分比偏差（|a−b|×1000÷
//! max）越容差即立案；可用源 <2 拒签（CalibSourceMissing）。
//!
//! ## 要点六：MSR 0x10/0x11 语义模型 + Deadline 联动 + 跨核/幂等/报告
//!
//! MSR 0x10（IA32_TIME_STAMP_COUNTER）语义：随逻辑 tick 线性前进且
//! 增量恒等于频率口径；MSR 0x11（IA32_TSC_DEADLINE）语义：目标值必须
//! 严格大于当前 TSC 读数（已过期即语义破缺）。TSC-Deadline 位声明可用
//! 而 MSR 0x11 不可用→联动错位立案。跨核同步账：多核注入读数两两偏差
//! 越余量立案；拓扑值域账：每核采样值域须全局重叠（同步 TSC 特征）。
//! 幂等账：二次探测指纹与首次一致才接受，漂移立案。启动报告导出 kv
//! 闭集：invariant/tsc_deadline/freq_mhz/calib_source/degrade_tier/
//! degrade_reason/round_hit/degrade_hit/basis，缺键即 ReportKv 立案。
//!
//! ## 诊断码段
//!
//! 独占 `0x4cf0..=0x4cff` 十六码闭集（grep 同目录确认：0x4c00..0x4ceb
//! 已被 F008/F080/F081/F084/F088/F090..F099/F1021/F402/F403 等占用，
//! 0x4cf0 段空闲，先入库者占段）。

use alloc::vec;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 〇、版本与常量
// ---------------------------------------------------------------------------

/// 模块版本。
pub const TP_VERSION: &str = "unx1002_tscprobe-v1";

/// 诊断段基址（0x4cf0 段 · 本模块独占）。
pub const TP_SEG_BASE: u16 = 0x4cf0;

/// CPUID 标准叶上限（leaf 0x1 所在域）。
pub const CPUID_LEAF_STD_BASE: u32 = 0x0000_0001;
/// CPUID 扩展能效叶（Invariant TSC 所在叶）。
pub const CPUID_LEAF_EXT_EFF: u32 = 0x8000_0007;
/// CPUID hypervisor 品牌基叶。
pub const CPUID_LEAF_HV_BASE: u32 = 0x4000_0000;
/// CPUID Hyper-V 特定叶（"Hv#1" 签名所在）。
pub const CPUID_LEAF_HV_SIG: u32 = 0x4000_0001;
/// CPUID 核晶振比叶。
pub const CPUID_LEAF_RATIO: u32 = 0x0000_0015;
/// CPUID 基础频率叶（MHz）。
pub const CPUID_LEAF_BASE_MHZ: u32 = 0x0000_0016;

/// leaf 0x1 ECX[31]：hypervisor 在场位。
pub const BIT_HYPERVISOR: u32 = 31;
/// leaf 0x1 ECX[24]：TSC-Deadline 位。
pub const BIT_TSC_DEADLINE: u32 = 24;
/// leaf 0x80000007 EDX[8]：Invariant TSC 位。
pub const BIT_INVARIANT_TSC: u32 = 8;

/// Hyper-V 特定叶签名 "Hv#1"（小端 u32）。
pub const HV_SIG_HV1: u32 = 0x3123_7648;
/// KVM 品牌签名 EBX（"KVMK" 小端）。
pub const KVM_SIG_EBX: u32 = 0x4B4D_564B;
/// KVM 品牌签名 ECX（"VMKV" 小端）。
pub const KVM_SIG_ECX: u32 = 0x564B_4D56;
/// KVM 品牌签名 EDX（"M\0\0\0" 小端）。
pub const KVM_SIG_EDX: u32 = 0x0000_004D;

/// MSR 0x10：IA32_TIME_STAMP_COUNTER。
pub const MSR_TSC: u32 = 0x10;
/// MSR 0x11：IA32_TSC_DEADLINE。
pub const MSR_TSC_DEADLINE: u32 = 0x11;

/// PIT 基频（Hz，1.193182 MHz 整数口径）。
pub const PIT_BASE_HZ: u64 = 1_193_182;

/// 校准源两两千分比容差（50‰ = 5%）。
pub const TP_CALIB_TOLERANCE_PERMILLE: u64 = 50;

/// 跨核 TSC 读数偏差余量（tick）：两两同 tick 读数差不得超过此值。
pub const TP_CORE_SKEW_MARGIN_TICKS: u64 = 2_000;

/// 每环境判定轮数（判据 10/10）。
pub const TP_ROUNDS: u64 = 10;

/// TSC 语义模型换算：1 逻辑 tick（=1ms）内 TSC 增量 = freq_mhz × 1000。
pub const TSC_TICKS_PER_MHZ_MS: u64 = 1_000;

// ---------------------------------------------------------------------------
// 一、诊断码闭集（独占 0x4cf0 段）
// ---------------------------------------------------------------------------

/// TSC 特性探测诊断码。独占 `0x4cf0..=0x4cff` 段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TpCode {
    /// 0x4cf0 环境闭集判定失败（签名叶既非物理机口径也非已知 hypervisor）。
    EnvUnknown,
    /// 0x4cf1 必需 CPUID 叶缺失（0x80000007 或 0x1 不可达）。
    CpuidLeafMissing,
    /// 0x4cf2 MSR 注入口读不可达（返回 None）。
    MsrUnreachable,
    /// 0x4cf3 MSR 语义破缺（deadline ≤ 当前 TSC / 增量 ≠ 频率口径）。
    MsrSemanticsBad,
    /// 0x4cf4 十轮判定翻转（同环境两轮 Invariant 结论不一致）。
    VerdictFlip,
    /// 0x4cf5 轮次账破缺（轮数 ≠ 账条数）。
    RoundLedger,
    /// 0x4cf6 校准三源缺源（可用源 < 2，一致性无从对拍）。
    CalibSourceMissing,
    /// 0x4cf7 校准源发散（任一两两千分比偏差越容差）。
    CalibDiverge,
    /// 0x4cf8 频率为零（CPUID 比值零 / 注入口给定零）。
    FreqZero,
    /// 0x4cf9 TSC-Deadline 联动错位（位声明可用但 MSR 0x11 语义不可用）。
    DeadlineMismatch,
    /// 0x4cfa 跨核 TSC 读数偏差越余量。
    CrossCoreDrift,
    /// 0x4cfb 幂等账漂移立案（二次探测指纹 ≠ 首次）。
    IdemDrift,
    /// 0x4cfc 拓扑值域账破缺（每核值域不全局重叠——非同步 TSC 特征）。
    TopologyRange,
    /// 0x4cfd 状态机非法迁移（相位序错）。
    PhaseOrder,
    /// 0x4cfe 启动报告键集破缺（必备 kv 键缺失或空值）。
    ReportKv,
    /// 0x4cff 段位守卫失败（诊断码值越出 0x4cf0..=0x4cff）。
    SegmentGuard,
}

impl TpCode {
    /// 全量闭集（十六码）。
    pub const ALL: [TpCode; 16] = [
        TpCode::EnvUnknown,
        TpCode::CpuidLeafMissing,
        TpCode::MsrUnreachable,
        TpCode::MsrSemanticsBad,
        TpCode::VerdictFlip,
        TpCode::RoundLedger,
        TpCode::CalibSourceMissing,
        TpCode::CalibDiverge,
        TpCode::FreqZero,
        TpCode::DeadlineMismatch,
        TpCode::CrossCoreDrift,
        TpCode::IdemDrift,
        TpCode::TopologyRange,
        TpCode::PhaseOrder,
        TpCode::ReportKv,
        TpCode::SegmentGuard,
    ];

    /// wire 码值。
    pub const fn val(self) -> u16 {
        TP_SEG_BASE + self as u16
    }

    /// 段位守卫：码值必须落在 0x4cf0..=0x4cff。
    pub const fn in_segment(self) -> bool {
        let v = self.val();
        v >= TP_SEG_BASE && v <= TP_SEG_BASE + 15
    }

    /// 人话原因（中文诊断）。
    pub fn reason(self) -> String {
        match self {
            TpCode::EnvUnknown => {
                "环境闭集判定失败：hypervisor 签名叶既非物理机口径也非 KVM/Hyper-V 已知签名".into()
            }
            TpCode::CpuidLeafMissing => {
                "必需 CPUID 叶缺失：0x80000007（Invariant TSC）或 0x1 不可达，拒绝猜测".into()
            }
            TpCode::MsrUnreachable => "MSR 注入口读不可达：注入面返回 None（权限或虚拟化截获）".into(),
            TpCode::MsrSemanticsBad => {
                "MSR 语义破缺：deadline 目标不大于当前 TSC，或增量不等于频率口径".into()
            }
            TpCode::VerdictFlip => "十轮判定翻转：同环境两轮 Invariant 结论不一致，判定不可信".into(),
            TpCode::RoundLedger => "轮次账破缺：判定轮数与账条数不等，账本自洽性失守".into(),
            TpCode::CalibSourceMissing => "校准三源缺源：可用源不足 2 个，频率一致性无从对拍".into(),
            TpCode::CalibDiverge => "校准源发散：任一两两千分比偏差越容差，频率口径不可信".into(),
            TpCode::FreqZero => "频率为零：CPUID 比值叶读零或注入口给定零值，拒签".into(),
            TpCode::DeadlineMismatch => {
                "TSC-Deadline 联动错位：CPUID 位声明可用但 MSR 0x11 语义不可用".into()
            }
            TpCode::CrossCoreDrift => "跨核 TSC 读数偏差越余量：同逻辑 tick 两核读数差超界".into(),
            TpCode::IdemDrift => "幂等账漂移立案：二次探测指纹与首次不一致，探测不稳定".into(),
            TpCode::TopologyRange => "拓扑值域账破缺：每核采样值域不全局重叠，非同步 TSC 特征".into(),
            TpCode::PhaseOrder => "状态机非法迁移：相位序错，探测链路被跳步".into(),
            TpCode::ReportKv => "启动报告键集破缺：必备 kv 键缺失或空值，报告不可出口".into(),
            TpCode::SegmentGuard => "诊断码分配越段：核对本模块段基址 0x4cf0".into(),
        }
    }
}

// ---------------------------------------------------------------------------
// 二、环境闭集与 CPUID 注入口
// ---------------------------------------------------------------------------

/// 环境闭集三档。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvKind {
    /// 物理机：leaf 0x1 ECX[31] = 0。
    BareMetal,
    /// hypervisor 虚拟机：ECX[31] = 1 且 0x40000000 签名 = KVMKVMKVM。
    HypervisorVm,
    /// Hyper-V 特定叶：ECX[31] = 1 且 0x40000001 EAX = "Hv#1"。
    HyperV,
}

impl EnvKind {
    /// 全量闭集（判定次序固定：物理机 → Hyper-V → 虚拟机）。
    pub const ALL: [EnvKind; 3] = [EnvKind::BareMetal, EnvKind::HyperV, EnvKind::HypervisorVm];

    /// 人话名。
    pub const fn name(self) -> &'static str {
        match self {
            EnvKind::BareMetal => "bare_metal",
            EnvKind::HyperV => "hyper_v",
            EnvKind::HypervisorVm => "hypervisor_vm",
        }
    }
}

/// CPUID 四返回值（EAX/EBX/ECX/EDX）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CpuidQuad {
    /// EAX。
    pub eax: u32,
    /// EBX。
    pub ebx: u32,
    /// ECX。
    pub ecx: u32,
    /// EDX。
    pub edx: u32,
}

impl CpuidQuad {
    /// 构造。
    pub const fn new(eax: u32, ebx: u32, ecx: u32, edx: u32) -> Self {
        CpuidQuad { eax, ebx, ecx, edx }
    }

    /// 取 ECX 第 bit 位。
    pub const fn ecx_bit(&self, bit: u32) -> bool {
        (self.ecx >> bit) & 1 != 0
    }

    /// 取 EDX 第 bit 位。
    pub const fn edx_bit(&self, bit: u32) -> bool {
        (self.edx >> bit) & 1 != 0
    }
}

/// CPUID 注入口（零真实执行——一切叶读数经此进入）。
pub trait CpuidPort {
    /// 读指定叶/子叶；None = 叶不存在。
    fn cpuid(&self, leaf: u32, subleaf: u32) -> Option<CpuidQuad>;
    /// 标准叶上限。
    fn max_std_leaf(&self) -> u32;
    /// 扩展叶上限。
    fn max_ext_leaf(&self) -> u32;
}

/// 合成 CPU 确定性数据（三环境构造母体）。
///
/// 全部字段即"注入的硬件答案"；`ground_truth_invariant` 为对拍真值
/// （判定器正常不看它，仅 10/10 对拍账使用——判据闭环用）。
#[derive(Debug, Clone)]
pub struct SyntheticCpu {
    /// 环境档。
    pub env: EnvKind,
    /// Invariant TSC 注入答案（leaf 0x80000007 EDX[8]）。
    pub invariant: bool,
    /// TSC-Deadline 注入答案（leaf 0x1 ECX[24]）。
    pub deadline: bool,
    /// 对拍真值（判定正确性闭环用）。
    pub ground_truth_invariant: bool,
    /// leaf 0x15 EAX：核晶振比分子（0 = 叶缺席）。
    pub leaf15_num: u32,
    /// leaf 0x15 EBX：核晶振比分母（0 = 叶缺席）。
    pub leaf15_den: u32,
    /// leaf 0x15 ECX：核晶振频率 Hz（0 = 缺席）。
    pub crystal_hz: u32,
    /// leaf 0x16 EAX：基础 TSC MHz（0 = 叶缺席）。
    pub leaf16_mhz: u32,
    /// MSR 0x10 语义模型基值。
    pub msr_tsc_base: u64,
    /// MSR 0x11 语义模型目标值。
    pub msr_deadline_target: u64,
    /// PIT 分频器（语义模型用）。
    pub pit_divisor: u16,
    /// PIT 窗内周计数（语义模型用）。
    pub pit_window_ticks: u32,
    /// 校准窗长（逻辑 tick，1 tick = 1ms）。
    pub window_logic_ms: u64,
    /// 注入口给定频率（MHz，0 = 缺席）。
    pub injected_freq_mhz: u64,
    /// 每核 TSC 偏移（tick 口径，i64 允许负偏）。
    pub core_offsets: Vec<i64>,
}

impl SyntheticCpu {
    /// 物理机构造：Invariant + Deadline 全齐，三频率源齐备，四核微偏。
    pub fn bare_metal() -> Self {
        SyntheticCpu {
            env: EnvKind::BareMetal,
            invariant: true,
            deadline: true,
            ground_truth_invariant: true,
            leaf15_num: 96,
            leaf15_den: 1,
            crystal_hz: 25_000_000,
            leaf16_mhz: 2400,
            msr_tsc_base: 0x0001_0000_0000,
            msr_deadline_target: 0x0001_0000_0000u64 + 2400 * TSC_TICKS_PER_MHZ_MS * 100,
            pit_divisor: 0,
            pit_window_ticks: 11_932,
            window_logic_ms: 10,
            injected_freq_mhz: 2400,
            core_offsets: vec![0, 40, -30, 25],
        }
    }

    /// hypervisor 虚拟机构造（KVM 签名）：Invariant + Deadline 齐，
    /// leaf 0x15 缺席（虚拟化环境常见），频率走 0x16 + PIT + 注入。
    pub fn hypervisor_vm() -> Self {
        SyntheticCpu {
            env: EnvKind::HypervisorVm,
            invariant: true,
            deadline: true,
            ground_truth_invariant: true,
            leaf15_num: 0,
            leaf15_den: 0,
            crystal_hz: 0,
            leaf16_mhz: 2600,
            msr_tsc_base: 0x0002_0000_0000,
            msr_deadline_target: 0x0002_0000_0000u64 + 2600 * TSC_TICKS_PER_MHZ_MS * 100,
            pit_divisor: 0,
            pit_window_ticks: 11_932,
            window_logic_ms: 10,
            injected_freq_mhz: 2600,
            core_offsets: vec![0, 18, -12],
        }
    }

    /// Hyper-V 特定叶构造：非不变 TSC（降级声明路径常绿），Deadline 缺席，
    /// leaf 0x15/0x16 双缺——频率只走 PIT 模型 + 注入两源。
    pub fn hyper_v_no_invariant() -> Self {
        SyntheticCpu {
            env: EnvKind::HyperV,
            invariant: false,
            deadline: false,
            ground_truth_invariant: false,
            leaf15_num: 0,
            leaf15_den: 0,
            crystal_hz: 0,
            leaf16_mhz: 0,
            msr_tsc_base: 0x0003_0000_0000,
            msr_deadline_target: 0,
            pit_divisor: 0,
            pit_window_ticks: 11_932,
            window_logic_ms: 10,
            injected_freq_mhz: 2200,
            core_offsets: vec![0, -60, 45, -20],
        }
    }
}

/// 合成 CPUID 注入口（确定性——同输入恒同输出）。
pub struct SyntheticCpuid<'a> {
    /// 母体引用。
    pub cpu: &'a SyntheticCpu,
}

impl<'a> SyntheticCpuid<'a> {
    /// 构造。
    pub const fn new(cpu: &'a SyntheticCpu) -> Self {
        SyntheticCpuid { cpu }
    }
}

impl<'a> CpuidPort for SyntheticCpuid<'a> {
    fn cpuid(&self, leaf: u32, _subleaf: u32) -> Option<CpuidQuad> {
        let c = self.cpu;
        match leaf {
            // 标准 leaf 0x1：ECX[31]=hypervisor 位，ECX[24]=TSC-Deadline。
            CPUID_LEAF_STD_BASE => {
                let hv_bit: u32 = match c.env {
                    EnvKind::BareMetal => 0,
                    EnvKind::HypervisorVm | EnvKind::HyperV => 1,
                };
                let dl_bit: u32 = if c.deadline { 1 } else { 0 };
                Some(CpuidQuad::new(
                    0x0006_0F00,
                    0x0000_0800,
                    (hv_bit << BIT_HYPERVISOR) | (dl_bit << BIT_TSC_DEADLINE) | 0x0000_0001,
                    0x078B_FBF9,
                ))
            }
            // 核晶振比叶：den==0 即缺席（虚拟化常见）。
            CPUID_LEAF_RATIO => {
                if c.leaf15_den == 0 {
                    None
                } else {
                    Some(CpuidQuad::new(c.leaf15_num, c.leaf15_den, c.crystal_hz, 0))
                }
            }
            // 基础频率叶：mhz==0 即缺席。
            CPUID_LEAF_BASE_MHZ => {
                if c.leaf16_mhz == 0 {
                    None
                } else {
                    Some(CpuidQuad::new(c.leaf16_mhz, 0, 0, 0))
                }
            }
            // hypervisor 品牌基叶：物理机缺席；KVM/Hyper-V 给签名。
            CPUID_LEAF_HV_BASE => match c.env {
                EnvKind::BareMetal => None,
                EnvKind::HypervisorVm => Some(CpuidQuad::new(
                    0,
                    KVM_SIG_EBX,
                    KVM_SIG_ECX,
                    KVM_SIG_EDX,
                )),
                EnvKind::HyperV => Some(CpuidQuad::new(
                    0,
                    0x7263_694D, // "Micr"
                    0x666F_736F, // "osof"
                    0x7648_2074, // "t Hv"
                )),
            },
            // Hyper-V 特定叶：仅 Hyper-V 给 "Hv#1"。
            CPUID_LEAF_HV_SIG => match c.env {
                EnvKind::HyperV => Some(CpuidQuad::new(HV_SIG_HV1, 0, 0, 0)),
                _ => None,
            },
            // 扩展能效叶：EDX[8] = Invariant TSC。
            CPUID_LEAF_EXT_EFF => {
                let inv_bit: u32 = if c.invariant { 1 } else { 0 };
                Some(CpuidQuad::new(0, 0, 0, inv_bit << BIT_INVARIANT_TSC))
            }
            _ => None,
        }
    }

    fn max_std_leaf(&self) -> u32 {
        0x16
    }

    fn max_ext_leaf(&self) -> u32 {
        0x8000_0008
    }
}

/// 环境闭集判定：物理机 → Hyper-V 特定叶 → KVM 虚拟机，其余 EnvUnknown。
///
/// 规则：① leaf 0x1 不可达 → CpuidLeafMissing；② ECX[31]=0 → 物理机；
/// ③ ECX[31]=1 且品牌叶缺席 → EnvUnknown（声明在 hypervisor 却无签名）；
/// ④ 签名 = KVMKVMKVM → HypervisorVm；⑤ 0x40000001 EAX = "Hv#1" →
/// HyperV；⑥ 其余签名 → EnvUnknown。
pub fn classify_env(cpuid: &dyn CpuidPort) -> Result<EnvKind, TpCode> {
    let leaf1 = match cpuid.cpuid(CPUID_LEAF_STD_BASE, 0) {
        Some(q) => q,
        None => return Err(TpCode::CpuidLeafMissing),
    };
    if !leaf1.ecx_bit(BIT_HYPERVISOR) {
        return Ok(EnvKind::BareMetal);
    }
    let brand = match cpuid.cpuid(CPUID_LEAF_HV_BASE, 0) {
        Some(q) => q,
        None => return Err(TpCode::EnvUnknown),
    };
    if brand.ebx == KVM_SIG_EBX && brand.ecx == KVM_SIG_ECX && brand.edx == KVM_SIG_EDX {
        return Ok(EnvKind::HypervisorVm);
    }
    if let Some(sig) = cpuid.cpuid(CPUID_LEAF_HV_SIG, 0) {
        if sig.eax == HV_SIG_HV1 {
            return Ok(EnvKind::HyperV);
        }
    }
    Err(TpCode::EnvUnknown)
}

// ---------------------------------------------------------------------------
// 三、MSR 注入口与语义模型（0x10 / 0x11）
// ---------------------------------------------------------------------------

/// MSR 注入口（零真实执行——core 维度以支持跨核语义账）。
pub trait MsrPort {
    /// 读指定核的指定 MSR；None = 不可达。
    fn read_msr(&self, core: usize, msr: u32, tick: u64) -> Option<u64>;
}

/// 合成 MSR 注入口（语义模型：TSC 随逻辑 tick 线性前进 + 核偏移）。
pub struct SyntheticMsr<'a> {
    /// 母体引用。
    pub cpu: &'a SyntheticCpu,
}

impl<'a> SyntheticMsr<'a> {
    /// 构造。
    pub const fn new(cpu: &'a SyntheticCpu) -> Self {
        SyntheticMsr { cpu }
    }

    /// 语义模型单核 TSC 值：base + tick × freq_mhz × 1000 + 核偏移。
    ///
    /// 全 checked 算术：任一步溢出 → None（语义模型拒绝产值而非绕回）。
    fn model_tsc(&self, core: usize, tick: u64) -> Option<u64> {
        let c = self.cpu;
        let rate = core_freq_hz(c)?;
        let scaled = tick.checked_mul(rate / 1_000_000 * TSC_TICKS_PER_MHZ_MS)?;
        let base = c.msr_tsc_base.checked_add(scaled)?;
        let offset = *c.core_offsets.get(core)?;
        if offset >= 0 {
            base.checked_add(offset as u64)
        } else {
            base.checked_sub((offset.wrapping_neg()) as u64)
        }
    }
}

impl<'a> MsrPort for SyntheticMsr<'a> {
    fn read_msr(&self, core: usize, msr: u32, tick: u64) -> Option<u64> {
        let c = self.cpu;
        match msr {
            MSR_TSC => self.model_tsc(core, tick),
            MSR_TSC_DEADLINE => {
                if !c.deadline {
                    return None;
                }
                let cur = self.model_tsc(core, tick)?;
                // 语义前置：目标必须严格大于当前读数，否则注入面本身破缺。
                if c.msr_deadline_target > cur {
                    Some(c.msr_deadline_target)
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

/// 环境主频（Hz，整数口径——供 MSR 语义模型换算）。
///
/// 取注入答案优先级：CPUID 0x16 > CPUID 0x15 比值 > 注入口给定；全缺
/// → None（调用方翻译成 FreqZero / MsrUnreachable 语义）。
pub fn core_freq_hz(c: &SyntheticCpu) -> Option<u64> {
    if c.leaf16_mhz != 0 {
        return (c.leaf16_mhz as u64).checked_mul(1_000_000);
    }
    if c.leaf15_den != 0 && c.leaf15_num != 0 && c.crystal_hz != 0 {
        let prod = (c.crystal_hz as u64).checked_mul(c.leaf15_num as u64)?;
        return prod.checked_div(c.leaf15_den as u64);
    }
    if c.injected_freq_mhz != 0 {
        return c.injected_freq_mhz.checked_mul(1_000_000);
    }
    None
}

/// MSR 0x10 语义校验：同核相邻逻辑 tick 增量必须恒等于频率口径。
///
/// 规则：① 两次读数任一不可达 → MsrUnreachable；② t2 ≤ t1（倒退）→
/// MsrSemanticsBad；③ 增量 ≠ freq_mhz×1000（模型口径）→ MsrSemanticsBad。
pub fn check_msr10_semantics(
    msr: &dyn MsrPort,
    core: usize,
    tick: u64,
    expect_freq_mhz: u64,
) -> Result<(), TpCode> {
    let t1 = match msr.read_msr(core, MSR_TSC, tick) {
        Some(v) => v,
        None => return Err(TpCode::MsrUnreachable),
    };
    let t2 = match msr.read_msr(core, MSR_TSC, tick.wrapping_add(1)) {
        Some(v) => v,
        None => return Err(TpCode::MsrUnreachable),
    };
    if t2 <= t1 {
        return Err(TpCode::MsrSemanticsBad);
    }
    let expect = match expect_freq_mhz.checked_mul(TSC_TICKS_PER_MHZ_MS) {
        Some(v) => v,
        None => return Err(TpCode::MsrSemanticsBad),
    };
    if t2 - t1 != expect {
        return Err(TpCode::MsrSemanticsBad);
    }
    Ok(())
}

/// MSR 0x11 语义校验：目标值必须严格大于当前 TSC（未过期的 deadline）。
pub fn check_msr11_semantics(msr: &dyn MsrPort, core: usize, tick: u64) -> Result<(), TpCode> {
    let target = match msr.read_msr(core, MSR_TSC_DEADLINE, tick) {
        Some(v) => v,
        None => return Err(TpCode::MsrUnreachable),
    };
    let cur = match msr.read_msr(core, MSR_TSC, tick) {
        Some(v) => v,
        None => return Err(TpCode::MsrUnreachable),
    };
    if target <= cur {
        return Err(TpCode::MsrSemanticsBad);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 四、特性判定核心（Invariant 位 / Deadline 位 / 频率叶）
// ---------------------------------------------------------------------------

/// Invariant TSC 判定：leaf 0x80000007 EDX[8]。
///
/// 规则：① 扩展叶上限 < 0x80000007 → CpuidLeafMissing（不猜）；② 叶读
/// None → CpuidLeafMissing；③ EDX[8]=1 → true；否则 false。
pub fn probe_invariant(cpuid: &dyn CpuidPort) -> Result<bool, TpCode> {
    if cpuid.max_ext_leaf() < CPUID_LEAF_EXT_EFF {
        return Err(TpCode::CpuidLeafMissing);
    }
    let q = match cpuid.cpuid(CPUID_LEAF_EXT_EFF, 0) {
        Some(q) => q,
        None => return Err(TpCode::CpuidLeafMissing),
    };
    Ok(q.edx_bit(BIT_INVARIANT_TSC))
}

/// TSC-Deadline 位判定：leaf 0x1 ECX[24]。
pub fn probe_deadline_bit(cpuid: &dyn CpuidPort) -> Result<bool, TpCode> {
    let q = match cpuid.cpuid(CPUID_LEAF_STD_BASE, 0) {
        Some(q) => q,
        None => return Err(TpCode::CpuidLeafMissing),
    };
    Ok(q.ecx_bit(BIT_TSC_DEADLINE))
}

/// 频率源一：CPUID 比值（leaf 0x15 优先，缺席回退 leaf 0x16）。
///
/// 规则：① 0x15 在场且三值全非零 → crystal×num÷den → Hz → MHz（整除
/// 口径，余数即亚 MHz 精度损失，账面注明）；② 回退 0x16 EAX（MHz）；
/// ③ 两叶全缺或读零 → Err(FreqZero)。
pub fn freq_from_cpuid(cpuid: &dyn CpuidPort) -> Result<u64, TpCode> {
    if let Some(q) = cpuid.cpuid(CPUID_LEAF_RATIO, 0) {
        if q.eax != 0 && q.ebx != 0 && q.ecx != 0 {
            let prod = (q.ecx as u64).checked_mul(q.eax as u64).ok_or(TpCode::FreqZero)?;
            let hz = prod.checked_div(q.ebx as u64).ok_or(TpCode::FreqZero)?;
            let mhz = hz.checked_div(1_000_000).ok_or(TpCode::FreqZero)?;
            if mhz == 0 {
                return Err(TpCode::FreqZero);
            }
            return Ok(mhz);
        }
    }
    if let Some(q) = cpuid.cpuid(CPUID_LEAF_BASE_MHZ, 0) {
        if q.eax != 0 {
            return Ok(q.eax as u64);
        }
    }
    Err(TpCode::FreqZero)
}

/// 频率源二：派生自 PIT 语义模型。
///
/// 模型：PIT 基频 1.193182 MHz，窗内计 `pit_window_ticks` 周 → 窗长
/// µs = 周数×1e6÷基频；同窗 TSC 增量由 MSR 语义模型两 tick 读数差给
/// 出 → 频率 MHz = 增量×1e6÷(窗长µs×1000)（1ms=1 tick 口径）。
pub fn freq_from_pit_model(cpu: &SyntheticCpu, msr: &dyn MsrPort, tick: u64) -> Result<u64, TpCode> {
    if cpu.pit_window_ticks == 0 {
        return Err(TpCode::FreqZero);
    }
    let win_us = (cpu.pit_window_ticks as u64)
        .checked_mul(1_000_000)
        .ok_or(TpCode::FreqZero)?
        .checked_div(PIT_BASE_HZ)
        .ok_or(TpCode::FreqZero)?;
    if win_us == 0 {
        return Err(TpCode::FreqZero);
    }
    let t1 = match msr.read_msr(0, MSR_TSC, tick) {
        Some(v) => v,
        None => return Err(TpCode::MsrUnreachable),
    };
    let t2_tick = tick.checked_add(cpu.window_logic_ms).ok_or(TpCode::FreqZero)?;
    let t2 = match msr.read_msr(0, MSR_TSC, t2_tick) {
        Some(v) => v,
        None => return Err(TpCode::MsrUnreachable),
    };
    if t2 <= t1 {
        return Err(TpCode::MsrSemanticsBad);
    }
    let delta = t2 - t1;
    // delta tick × (1e6 µs / win_us) = 频率 µs 口径 → MHz 需再除 1e6，
    // 合并：mhz = delta × 1e6 ÷ (win_us × 1000 × window_logic_ms)。
    let denom = win_us
        .checked_mul(TSC_TICKS_PER_MHZ_MS)
        .ok_or(TpCode::FreqZero)?
        .checked_mul(cpu.window_logic_ms)
        .ok_or(TpCode::FreqZero)?;
    let mhz = delta.checked_mul(1_000_000).ok_or(TpCode::FreqZero)? / denom;
    if mhz == 0 {
        return Err(TpCode::FreqZero);
    }
    Ok(mhz)
}

/// 频率源三：注入口给定值（0 = 缺席 → FreqZero）。
pub fn freq_injected(cpu: &SyntheticCpu) -> Result<u64, TpCode> {
    if cpu.injected_freq_mhz == 0 {
        return Err(TpCode::FreqZero);
    }
    Ok(cpu.injected_freq_mhz)
}

/// 两值千分比偏差（全整数）：|a−b|×1000 ÷ max(a,b)。
pub fn permille_dev(a: u64, b: u64) -> Result<u64, TpCode> {
    if a == 0 && b == 0 {
        return Err(TpCode::FreqZero);
    }
    let diff = if a >= b { a - b } else { b - a };
    let m = if a >= b { a } else { b };
    diff.checked_mul(1000)
        .ok_or(TpCode::CalibDiverge)?
        .checked_div(m)
        .ok_or(TpCode::CalibDiverge)
}

// ---------------------------------------------------------------------------
// 五、三环境判定闭集与 10/10 对拍账
// ---------------------------------------------------------------------------

/// 单轮判定记录（一环境一轮）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoundRecord {
    /// 轮序（0 起单调）。
    pub round: u64,
    /// 环境档。
    pub env: EnvKind,
    /// 本轮 Invariant 判定。
    pub verdict_invariant: bool,
    /// 本轮 TSC-Deadline 判定。
    pub verdict_deadline: bool,
    /// 与确定性真值对拍是否命中。
    pub matches_truth: bool,
    /// 本轮逻辑 tick。
    pub tick: u64,
}

/// 十轮对拍账（一环境一册）。
#[derive(Debug, Clone)]
pub struct RoundLedger {
    /// 环境档。
    pub env: EnvKind,
    /// 逐轮记录。
    pub rounds: Vec<RoundRecord>,
    /// 命中数（与真值对拍全中数）。
    pub hits: u64,
    /// 轮数目标（10）。
    pub target: u64,
}

impl RoundLedger {
    /// 判定正确（判据 10/10）：账条数 = 轮数 = 命中数，且无翻转。
    pub fn is_correct(&self) -> bool {
        self.rounds.len() as u64 == self.target
            && self.hits == self.target
            && !self.verdict_flipped()
    }

    /// 翻转检测：任一两轮 Invariant 结论不一致即翻转。
    pub fn verdict_flipped(&self) -> bool {
        let mut iter = self.rounds.iter();
        let first = match iter.next() {
            Some(r) => r.verdict_invariant,
            None => return true,
        };
        self.rounds.iter().any(|r| r.verdict_invariant != first)
    }

    /// 账自洽校验：轮数 ≢ 条数 → RoundLedger。
    pub fn audit(&self) -> Result<(), TpCode> {
        if self.rounds.len() as u64 != self.target {
            return Err(TpCode::RoundLedger);
        }
        for (i, r) in self.rounds.iter().enumerate() {
            if r.round != i as u64 {
                return Err(TpCode::RoundLedger);
            }
        }
        Ok(())
    }
}

/// 三环境判定闭集：每环境十轮独立判定并逐轮与真值对拍。
///
/// 规则：① 环境先经 classify_env 定档（闭集外 → EnvUnknown）；② 每轮
/// 重读 CPUID 注入口（探测无状态——确定性下逐轮恒同）；③ 判定与
/// ground_truth_invariant 不符 → 该轮 miss；④ 返回三册账由调用方核
/// 10/10。
pub fn probe_three_envs(cpu: &SyntheticCpu, tick: u64) -> Result<Vec<RoundLedger>, TpCode> {
    let cpuid = SyntheticCpuid::new(cpu);
    let env = classify_env(&cpuid)?;
    if env != cpu.env {
        // 环境档与母体声明不一致：注入面自相矛盾，按 EnvUnknown 立案。
        return Err(TpCode::EnvUnknown);
    }
    let mut records: Vec<RoundRecord> = Vec::new();
    let mut round_tick = tick;
    for _ in 0..TP_ROUNDS {
        let inv = probe_invariant(&cpuid)?;
        let dl = probe_deadline_bit(&cpuid)?;
        records.push(RoundRecord {
            round: records.len() as u64,
            env,
            verdict_invariant: inv,
            verdict_deadline: dl,
            matches_truth: inv == cpu.ground_truth_invariant,
            tick: round_tick,
        });
        round_tick = round_tick.wrapping_add(1);
    }
    let hits = records.iter().filter(|r| r.matches_truth).count() as u64;
    let ledger = RoundLedger {
        env,
        rounds: records,
        hits,
        target: TP_ROUNDS,
    };
    ledger.audit()?;
    if ledger.verdict_flipped() {
        return Err(TpCode::VerdictFlip);
    }
    let mut out: Vec<RoundLedger> = Vec::new();
    out.push(ledger);
    Ok(out)
}

/// 三环境总账：对默认三环境母体各跑十轮，返回三册 + 命中总数。
///
/// 任一册非 10/10 → 返回 Err（判据不闭环即显性，不带病出账）。
pub fn probe_all_envs_closed_set(tick: u64) -> Result<(Vec<RoundLedger>, u64), TpCode> {
    let mats = [
        SyntheticCpu::bare_metal(),
        SyntheticCpu::hypervisor_vm(),
        SyntheticCpu::hyper_v_no_invariant(),
    ];
    let mut all: Vec<RoundLedger> = Vec::new();
    let mut total_hits: u64 = 0;
    let mut ledger_tick = tick;
    for m in mats.iter() {
        let mut ls = probe_three_envs(m, ledger_tick)?;
        let l = match ls.pop() {
            Some(l) => l,
            None => return Err(TpCode::RoundLedger),
        };
        if !l.is_correct() {
            return Err(TpCode::VerdictFlip);
        }
        total_hits = total_logits(total_hits, l.hits)?;
        ledger_tick = ledger_tick.wrapping_add(TP_ROUNDS);
        all.push(l);
    }
    if all.len() != EnvKind::ALL.len() {
        return Err(TpCode::EnvUnknown);
    }
    Ok((all, total_hits))
}

/// 命中数累加（checked——账面溢出即立案）。
fn total_logits(acc: u64, add: u64) -> Result<u64, TpCode> {
    acc.checked_add(add).ok_or(TpCode::RoundLedger)
}

// ---------------------------------------------------------------------------
// 六、非不变 TSC 降级声明三档
// ---------------------------------------------------------------------------

/// 降级档位闭集（三档）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DegradeTier {
    /// 档 0：不变可用——TSC 单调且跨核同步，可作单调基准。
    InvariantUsable,
    /// 档 1：仅校准可用——TSC 在走但非不变，须经校准/残差修正后使用。
    CalibOnlyUsable,
    /// 档 2：不可用——特性缺席，时间基座必须回退 PIT/HPET 语义模型。
    Unusable,
}

impl DegradeTier {
    /// 档位序号（0/1/2）。
    pub const fn rank(self) -> u8 {
        match self {
            DegradeTier::InvariantUsable => 0,
            DegradeTier::CalibOnlyUsable => 1,
            DegradeTier::Unusable => 2,
        }
    }

    /// kv 出口名。
    pub const fn key(self) -> &'static str {
        match self {
            DegradeTier::InvariantUsable => "invariant_usable",
            DegradeTier::CalibOnlyUsable => "calib_only_usable",
            DegradeTier::Unusable => "unusable",
        }
    }
}

/// 降级原因码闭集。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DegradeReason {
    /// 无降级（档 0 专属）。
    None,
    /// CPUID EDX[8] 位为 0——硬件明确声明非不变。
    CpuidBitClear,
    /// CPUID 0x80000007 叶缺席——特性不可知按不可用处理。
    CpuidLeafAbsent,
    /// hypervisor 在场且位为 0——虚拟化掩蔽（可能被截获伪装）。
    HypervisorMasking,
}

impl DegradeReason {
    /// kv 出口名。
    pub const fn key(self) -> &'static str {
        match self {
            DegradeReason::None => "none",
            DegradeReason::CpuidBitClear => "cpuid_bit_clear",
            DegradeReason::CpuidLeafAbsent => "cpuid_leaf_absent",
            DegradeReason::HypervisorMasking => "hypervisor_masking",
        }
    }
}

/// 降级声明册（档位 + 原因 + 对时间基座的影响注记）。
#[derive(Debug, Clone)]
pub struct DegradeDeclaration {
    /// 环境档。
    pub env: EnvKind,
    /// 降级档位。
    pub tier: DegradeTier,
    /// 降级原因码。
    pub reason: DegradeReason,
    /// 对时间基座的影响注记（人话，入启动报告）。
    pub impact_note: String,
    /// 声明轮数（连续声明计数——10/10 的"次"）。
    pub declared_rounds: u64,
}

/// 档位决策：判定结果（Ok(true)/Ok(false)/Err）→ 三档 + 原因。
///
/// 规则：① Ok(true) → InvariantUsable/None；② Ok(false) 且环境在
/// hypervisor → CalibOnlyUsable/HypervisorMasking；③ Ok(false) 物理机 →
/// CalibOnlyUsable/CpuidBitClear；④ Err(CpuidLeafMissing) →
/// Unusable/CpuidLeafAbsent；⑤ 其余 Err 原样上抛（不吞）。
pub fn decide_tier(env: EnvKind, probe: Result<bool, TpCode>) -> Result<(DegradeTier, DegradeReason), TpCode> {
    match probe {
        Ok(true) => Ok((DegradeTier::InvariantUsable, DegradeReason::None)),
        Ok(false) => match env {
            EnvKind::BareMetal => Ok((DegradeTier::CalibOnlyUsable, DegradeReason::CpuidBitClear)),
            EnvKind::HypervisorVm | EnvKind::HyperV => {
                Ok((DegradeTier::CalibOnlyUsable, DegradeReason::HypervisorMasking))
            }
        },
        Err(TpCode::CpuidLeafMissing) => Ok((DegradeTier::Unusable, DegradeReason::CpuidLeafAbsent)),
        Err(e) => Err(e),
    }
}

/// 档位 → 对时间基座的影响注记（全整数语义，无人话歧义）。
pub fn impact_note(tier: DegradeTier) -> String {
    match tier {
        DegradeTier::InvariantUsable => {
            "时间基座注记：TSC 可作单调基准；跨核时间戳直接可用；残差修正账可空转".to_string()
        }
        DegradeTier::CalibOnlyUsable => {
            "时间基座注记：TSC 非不变，跨核时间戳须经校准/残差修正后方可消费；单调基准须加修正层".to_string()
        }
        DegradeTier::Unusable => {
            "时间基座注记：TSC 特性缺席，单调基准回退 PIT/HPET 语义模型；TSC-Deadline 计时禁用".to_string()
        }
    }
}

/// 非不变 TSC 降级声明十轮账：非不变环境每轮强制出声明。
///
/// 规则：① 先跑一次判定 → 档位决策；② 非档 0 时连续声明 TP_ROUNDS
/// 次（每轮 tick 前进、决策复算，确定性下恒同）；③ 档 0 时声明账为
/// 空（无降级可言，账面 declared_rounds=0）；④ 返回声明册供启动报告。
pub fn declare_degradation(cpu: &SyntheticCpu, tick: u64) -> Result<DegradeDeclaration, TpCode> {
    let cpuid = SyntheticCpuid::new(cpu);
    let env = classify_env(&cpuid)?;
    let probe = probe_invariant(&cpuid);
    let (tier, reason) = decide_tier(env, probe)?;
    let mut rounds: u64 = 0;
    if tier != DegradeTier::InvariantUsable {
        while rounds < TP_ROUNDS {
            let p = probe_invariant(&cpuid)?;
            let (ti, re) = decide_tier(env, Ok(p))?;
            if ti != tier || re != reason {
                // 十轮内档位/原因漂移：声明不稳定，按 VerdictFlip 立案。
                return Err(TpCode::VerdictFlip);
            }
            rounds = rounds.checked_add(1).ok_or(TpCode::RoundLedger)?;
        }
    }
    Ok(DegradeDeclaration {
        env,
        tier,
        reason,
        impact_note: impact_note(tier),
        declared_rounds: rounds,
    })
}

// ---------------------------------------------------------------------------
// 七、TSC 频率校准三源一致性对拍
// ---------------------------------------------------------------------------

/// 频率源标识闭集。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalibSource {
    /// 源一：CPUID 比值（0x15/0x16）。
    CpuidRatio,
    /// 源二：PIT 语义模型派生。
    PitModel,
    /// 源三：注入口给定。
    Injected,
}

impl CalibSource {
    /// kv 出口名。
    pub const fn key(self) -> &'static str {
        match self {
            CalibSource::CpuidRatio => "cpuid_ratio",
            CalibSource::PitModel => "pit_model",
            CalibSource::Injected => "injected",
        }
    }
}

/// 单源读数账。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceReading {
    /// 源标识。
    pub source: CalibSource,
    /// 读数（MHz）。
    pub freq_mhz: u64,
}

/// 两两千分比对拍条目。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PairDeviation {
    /// 源 A。
    pub a: CalibSource,
    /// 源 B。
    pub b: CalibSource,
    /// 千分比偏差。
    pub permille: u64,
    /// 是否越容差。
    pub over: bool,
}

/// 三源一致性对拍账。
#[derive(Debug, Clone)]
pub struct CalibLedger {
    /// 可用源读数（≥2 才有对拍意义）。
    pub readings: Vec<SourceReading>,
    /// 两两偏差条目。
    pub pairs: Vec<PairDeviation>,
    /// 取中位数后的签收频率（MHz）。
    pub sealed_freq_mhz: u64,
    /// 签收源（中位数所属源）。
    pub sealed_source: CalibSource,
    /// 容差（千分比）。
    pub tolerance_permille: u64,
}

/// 三源采集：CPUID 比值 / PIT 模型 / 注入口，逐源可缺席。
pub fn collect_sources(
    cpu: &SyntheticCpu,
    cpuid: &dyn CpuidPort,
    msr: &dyn MsrPort,
    tick: u64,
) -> Result<Vec<SourceReading>, TpCode> {
    let mut out: Vec<SourceReading> = Vec::new();
    match freq_from_cpuid(cpuid) {
        Ok(v) => out.push(SourceReading { source: CalibSource::CpuidRatio, freq_mhz: v }),
        Err(TpCode::FreqZero) => { /* 源缺席——虚拟化常见，允许 */ }
        Err(e) => return Err(e),
    }
    match freq_from_pit_model(cpu, msr, tick) {
        Ok(v) => out.push(SourceReading { source: CalibSource::PitModel, freq_mhz: v }),
        Err(TpCode::FreqZero) | Err(TpCode::MsrUnreachable) => { /* 源缺席 */ }
        Err(e) => return Err(e),
    }
    match freq_injected(cpu) {
        Ok(v) => out.push(SourceReading { source: CalibSource::Injected, freq_mhz: v }),
        Err(TpCode::FreqZero) => { /* 源缺席 */ }
        Err(e) => return Err(e),
    }
    if out.len() < 2 {
        return Err(TpCode::CalibSourceMissing);
    }
    Ok(out)
}

/// 三源一致性对拍：两两千分比 + 容差 + 中位数签收。
///
/// 规则：① 源 <2 → CalibSourceMissing；② 任一两两偏差越容差 →
/// CalibDiverge；③ 取中位数（奇数取中间，偶数取较小者——全整数确定性）
/// 签收；④ 签收值 0 → FreqZero。
pub fn cross_check_sources(
    cpu: &SyntheticCpu,
    cpuid: &dyn CpuidPort,
    msr: &dyn MsrPort,
    tick: u64,
) -> Result<CalibLedger, TpCode> {
    let readings = collect_sources(cpu, cpuid, msr, tick)?;
    let mut pairs: Vec<PairDeviation> = Vec::new();
    let mut i = 0;
    while i < readings.len() {
        let mut j = i + 1;
        while j < readings.len() {
            let pm = permille_dev(readings[i].freq_mhz, readings[j].freq_mhz)?;
            let over = pm > TP_CALIB_TOLERANCE_PERMILLE;
            pairs.push(PairDeviation {
                a: readings[i].source,
                b: readings[j].source,
                permille: pm,
                over,
            });
            j += 1;
        }
        i += 1;
    }
    if pairs.iter().any(|p| p.over) {
        return Err(TpCode::CalibDiverge);
    }
    let mut vals: Vec<u64> = readings.iter().map(|r| r.freq_mhz).collect();
    vals.sort_unstable();
    let mid = vals.len() / 2;
    let sealed = match vals.get(mid) {
        Some(&v) => v,
        None => return Err(TpCode::FreqZero),
    };
    if sealed == 0 {
        return Err(TpCode::FreqZero);
    }
    let sealed_source = match readings.iter().find(|r| r.freq_mhz == sealed) {
        Some(r) => r.source,
        None => return Err(TpCode::FreqZero),
    };
    Ok(CalibLedger {
        readings,
        pairs,
        sealed_freq_mhz: sealed,
        sealed_source,
        tolerance_permille: TP_CALIB_TOLERANCE_PERMILLE,
    })
}

// ---------------------------------------------------------------------------
// 八、TSC-Deadline 可用性联动声明
// ---------------------------------------------------------------------------

/// TSC-Deadline 联动声明册。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeadlineDeclaration {
    /// CPUID 位判定（leaf 0x1 ECX[24]）。
    pub bit_claimed: bool,
    /// MSR 0x11 语义是否可用。
    pub msr_usable: bool,
    /// 联动结论：位声明与 MSR 语义一致才可用。
    pub usable: bool,
}

/// 联动核验：位声明可用 ⇔ MSR 0x11 语义可用，错位即立案。
///
/// 规则：① 位=1 而 MSR 读 None → DeadlineMismatch（位声明了却用不了）；
/// ② 位=0 而 MSR 有值 → DeadlineMismatch（未声明却有语义值——注入面
/// 撒谎）；③ 位=1 且 MSR 语义破缺 → 上抛 MsrSemanticsBad；④ 一致 →
/// usable = 位判定值。
pub fn check_deadline_linkage(
    cpu: &SyntheticCpu,
    msr: &dyn MsrPort,
    core: usize,
    tick: u64,
) -> Result<DeadlineDeclaration, TpCode> {
    let cpuid = SyntheticCpuid::new(cpu);
    let bit = probe_deadline_bit(&cpuid)?;
    let msr_ok = match check_msr11_semantics(msr, core, tick) {
        Ok(()) => true,
        Err(TpCode::MsrUnreachable) => false,
        Err(e) => return Err(e),
    };
    if bit && !msr_ok {
        return Err(TpCode::DeadlineMismatch);
    }
    if !bit && msr_ok {
        return Err(TpCode::DeadlineMismatch);
    }
    Ok(DeadlineDeclaration {
        bit_claimed: bit,
        msr_usable: msr_ok,
        usable: bit && msr_ok,
    })
}

// ---------------------------------------------------------------------------
// 九、跨核同步语义账 + CPU 拓扑值域账
// ---------------------------------------------------------------------------

/// 跨核两两偏差条目。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoreSkewRecord {
    /// 核 A id。
    pub core_a: usize,
    /// 核 B id。
    pub core_b: usize,
    /// 同逻辑 tick 读数差（tick 口径）。
    pub skew_ticks: u64,
    /// 是否越余量。
    pub over_margin: bool,
}

/// 跨核同步语义账。
#[derive(Debug, Clone)]
pub struct CrossCoreLedger {
    /// 参与核 id（注入面核序）。
    pub cores: Vec<usize>,
    /// 两两偏差条目。
    pub skews: Vec<CoreSkewRecord>,
    /// 最大偏差。
    pub max_skew_ticks: u64,
    /// 余量。
    pub margin_ticks: u64,
    /// 同步结论（全部条目未越余量）。
    pub synced: bool,
}

/// 跨核同步探测：同逻辑 tick 逐核读 MSR 0x10，两两求差对余量。
///
/// 规则：① 核数 <2 → CrossCoreDrift（单核无从跨核）；② 任一核读数
/// None → MsrUnreachable；③ 两两差越 TP_CORE_SKEW_MARGIN_TICKS →
/// CrossCoreDrift；④ 账面全收（越线条目也入账——账是账，立案是立案）。
pub fn probe_cross_core(cpu: &SyntheticCpu, msr: &dyn MsrPort, tick: u64) -> Result<CrossCoreLedger, TpCode> {
    let n = cpu.core_offsets.len();
    if n < 2 {
        return Err(TpCode::CrossCoreDrift);
    }
    let mut vals: Vec<u64> = Vec::new();
    for core in 0..n {
        match msr.read_msr(core, MSR_TSC, tick) {
            Some(v) => vals.push(v),
            None => return Err(TpCode::MsrUnreachable),
        }
    }
    let mut skews: Vec<CoreSkewRecord> = Vec::new();
    let mut max_skew: u64 = 0;
    let mut i = 0;
    while i < n {
        let mut j = i + 1;
        while j < n {
            let a = vals[i];
            let b = vals[j];
            let d = if a >= b { a - b } else { b - a };
            let over = d > TP_CORE_SKEW_MARGIN_TICKS;
            if d > max_skew {
                max_skew = d;
            }
            skews.push(CoreSkewRecord {
                core_a: i,
                core_b: j,
                skew_ticks: d,
                over_margin: over,
            });
            j += 1;
        }
        i += 1;
    }
    let synced = !skews.iter().any(|s| s.over_margin);
    if !synced {
        return Err(TpCode::CrossCoreDrift);
    }
    Ok(CrossCoreLedger {
        cores: (0..n).collect(),
        skews,
        max_skew_ticks: max_skew,
        margin_ticks: TP_CORE_SKEW_MARGIN_TICKS,
        synced,
    })
}

/// 每核 TSC 值域条目。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoreRange {
    /// 核 id。
    pub core: usize,
    /// 窗内最小读数。
    pub tsc_min: u64,
    /// 窗内最大读数。
    pub tsc_max: u64,
}

/// 拓扑值域账：每核窗内采样值域 + 全局重叠判定。
#[derive(Debug, Clone)]
pub struct TopologyLedger {
    /// 每核值域。
    pub ranges: Vec<CoreRange>,
    /// 全局重叠下界（各核 min 的最大者）。
    pub overlap_lo: u64,
    /// 全局重叠上界（各核 max 的最小者）。
    pub overlap_hi: u64,
    /// 同步特征：overlap_lo ≤ overlap_hi（值域全局重叠）。
    pub overlapped: bool,
}

/// 拓扑值域采样：每核取窗首/窗尾两读数构值域，核间重叠即同步特征。
///
/// 规则：① 任一核读数 None → MsrUnreachable；② 值域倒挂（min > max）
/// → MsrSemanticsBad；③ 不全局重叠 → TopologyRange（非同步 TSC 特征
/// ——与跨核账互为印证，双闸）。
pub fn probe_topology_ranges(
    cpu: &SyntheticCpu,
    msr: &dyn MsrPort,
    tick: u64,
) -> Result<TopologyLedger, TpCode> {
    let n = cpu.core_offsets.len();
    let mut ranges: Vec<CoreRange> = Vec::new();
    for core in 0..n {
        let v1 = match msr.read_msr(core, MSR_TSC, tick) {
            Some(v) => v,
            None => return Err(TpCode::MsrUnreachable),
        };
        let t2 = tick.checked_add(cpu.window_logic_ms).ok_or(TpCode::MsrSemanticsBad)?;
        let v2 = match msr.read_msr(core, MSR_TSC, t2) {
            Some(v) => v,
            None => return Err(TpCode::MsrUnreachable),
        };
        if v2 < v1 {
            return Err(TpCode::MsrSemanticsBad);
        }
        ranges.push(CoreRange {
            core,
            tsc_min: v1,
            tsc_max: v2,
        });
    }
    let lo = match ranges.iter().map(|r| r.tsc_min).max() {
        Some(v) => v,
        None => return Err(TpCode::TopologyRange),
    };
    let hi = match ranges.iter().map(|r| r.tsc_max).min() {
        Some(v) => v,
        None => return Err(TpCode::TopologyRange),
    };
    let overlapped = lo <= hi;
    if !overlapped {
        return Err(TpCode::TopologyRange);
    }
    Ok(TopologyLedger {
        ranges,
        overlap_lo: lo,
        overlap_hi: hi,
        overlapped,
    })
}

// ---------------------------------------------------------------------------
// 十、重复探测幂等账
// ---------------------------------------------------------------------------

/// 探测指纹（确定性折叠——同注入恒同指纹）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProbeFingerprint {
    /// 折叠值。
    pub fold: u64,
}

/// 特性快照（幂等账的对象——两次探测各出一快照）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeatureSnapshot {
    /// Invariant 判定。
    pub invariant: bool,
    /// Deadline 判定。
    pub deadline: bool,
    /// 签收频率（MHz）。
    pub freq_mhz: u64,
    /// 降级档位序号。
    pub tier_rank: u8,
    /// 环境档序号（EnvKind::ALL 下标）。
    pub env_rank: u8,
}

impl FeatureSnapshot {
    /// 指纹折叠（wrapping 乘加——溢出绕回但确定性不破）。
    pub fn fingerprint(&self) -> ProbeFingerprint {
        let mut f: u64 = 0x9E37_79B9_7F4A_7C15;
        f = f.wrapping_mul(31).wrapping_add(self.invariant as u64);
        f = f.wrapping_mul(31).wrapping_add(self.deadline as u64);
        f = f.wrapping_mul(31).wrapping_add(self.freq_mhz);
        f = f.wrapping_mul(31).wrapping_add(self.tier_rank as u64);
        f = f.wrapping_mul(31).wrapping_add(self.env_rank as u64);
        ProbeFingerprint { fold: f }
    }
}

/// 幂等账：首次指纹 + 复核指纹 + 立案记录。
#[derive(Debug, Clone)]
pub struct IdemLedger {
    /// 首次指纹。
    pub first: ProbeFingerprint,
    /// 复核指纹（None = 尚未复核）。
    pub second: Option<ProbeFingerprint>,
    /// 立案记录（漂移立案时写入）。
    pub cases: Vec<String>,
}

impl IdemLedger {
    /// 开账（首次探测）。
    pub fn open(snap: &FeatureSnapshot) -> Self {
        IdemLedger {
            first: snap.fingerprint(),
            second: None,
            cases: Vec::new(),
        }
    }

    /// 复核：指纹一致 → 接受；漂移 → 立案并 Err(IdemDrift)。
    pub fn recheck(&mut self, snap: &FeatureSnapshot) -> Result<(), TpCode> {
        let fp = snap.fingerprint();
        self.second = Some(fp);
        if fp.fold != self.first.fold {
            let case = format!(
                "idem drift: first={:#018x} second={:#018x}",
                self.first.fold, fp.fold
            );
            self.cases.push(case);
            return Err(TpCode::IdemDrift);
        }
        Ok(())
    }

    /// 幂等成立：两次指纹齐且相等。
    pub fn is_idempotent(&self) -> bool {
        match self.second {
            Some(s) => s.fold == self.first.fold,
            None => false,
        }
    }
}

// ---------------------------------------------------------------------------
// 十一、探测状态机
// ---------------------------------------------------------------------------

/// 探测相位（线性状态机——跳步即 PhaseOrder）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbePhase {
    /// 空闲。
    Idle,
    /// CPUID 已采样（环境定档 + 特性位判定）。
    CpuidProbed,
    /// MSR 已采样（0x10/0x11 语义过检）。
    MsrProbed,
    /// 三源校准已对拍。
    CalibCrossed,
    /// 跨核/拓扑已扫描。
    CoreScanned,
    /// 快照已封账（幂等复核完成）。
    Sealed,
}

impl ProbePhase {
    /// 相位序号。
    pub const fn rank(self) -> u8 {
        match self {
            ProbePhase::Idle => 0,
            ProbePhase::CpuidProbed => 1,
            ProbePhase::MsrProbed => 2,
            ProbePhase::CalibCrossed => 3,
            ProbePhase::CoreScanned => 4,
            ProbePhase::Sealed => 5,
        }
    }

    /// 唯一合法后继。
    pub const fn next(self) -> Option<ProbePhase> {
        match self {
            ProbePhase::Idle => Some(ProbePhase::CpuidProbed),
            ProbePhase::CpuidProbed => Some(ProbePhase::MsrProbed),
            ProbePhase::MsrProbed => Some(ProbePhase::CalibCrossed),
            ProbePhase::CalibCrossed => Some(ProbePhase::CoreScanned),
            ProbePhase::CoreScanned => Some(ProbePhase::Sealed),
            ProbePhase::Sealed => None,
        }
    }
}

/// 探测状态机（迁移守卫——非法迁移 PhaseOrder）。
#[derive(Debug, Clone)]
pub struct ProbeState {
    /// 当前相位。
    pub phase: ProbePhase,
    /// 迁移账（每次合法迁移追加一条）。
    pub transitions: Vec<String>,
}

impl ProbeState {
    /// 开机（Idle）。
    pub fn new() -> Self {
        ProbeState {
            phase: ProbePhase::Idle,
            transitions: Vec::new(),
        }
    }

    /// 前进一步：仅允许 Idle→CpuidProbed→…→Sealed 线性推进。
    pub fn advance(&mut self) -> Result<ProbePhase, TpCode> {
        let nxt = match self.phase.next() {
            Some(n) => n,
            None => return Err(TpCode::PhaseOrder),
        };
        let entry = format!("{:?}->{:?}", self.phase, nxt);
        self.phase = nxt;
        self.transitions.push(entry);
        Ok(self.phase)
    }

    /// 要求当前相位恰为 p，否则 PhaseOrder（各步执行前置守卫）。
    pub fn require(&self, p: ProbePhase) -> Result<(), TpCode> {
        if self.phase != p {
            return Err(TpCode::PhaseOrder);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 十二、启动报告导出（特性账 kv 格式）
// ---------------------------------------------------------------------------

/// 特性账行（kv 单行）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KvLine {
    /// 键。
    pub key: String,
    /// 值。
    pub value: String,
    /// 依据注记（判据闭环留痕）。
    pub basis: String,
}

/// 必备 kv 键闭集（缺一即 ReportKv）。
pub const TP_REQUIRED_KEYS: [&str; 9] = [
    "invariant_tsc",
    "tsc_deadline",
    "freq_mhz",
    "calib_source",
    "degrade_tier",
    "degrade_reason",
    "round_hit",
    "degrade_hit",
    "basis",
];

/// 特性账总册（启动报告素材）。
#[derive(Debug, Clone)]
pub struct FeatureLedger {
    /// kv 行（键序即出账序）。
    pub lines: Vec<KvLine>,
}

/// 导出启动报告 kv：特性账全键落账并自检。
///
/// 规则：① 九键全出，值域非空；② round_hit/degrade_hit 用 "n/10" 口径；
/// basis 汇总判定位依据；③ 缺键/空值 → ReportKv。
pub fn export_boot_report(
    snap: &FeatureSnapshot,
    calib: &CalibLedger,
    tier: DegradeTier,
    reason: DegradeReason,
    round_hits: u64,
    degrade_rounds: u64,
) -> Result<FeatureLedger, TpCode> {
    let inv_v = if snap.invariant { "1" } else { "0" };
    let dl_v = if snap.deadline { "1" } else { "0" };
    let basis = format!(
        "cpuid:0x80000007.EDX[8]={};cpuid:0x1.ECX[24]={};env={}",
        inv_v, dl_v, EnvKind::ALL[snap.env_rank as usize].name()
    );
    let lines = vec![
        KvLine {
            key: "invariant_tsc".to_string(),
            value: inv_v.to_string(),
            basis: "leaf 0x80000007 EDX[8]".to_string(),
        },
        KvLine {
            key: "tsc_deadline".to_string(),
            value: dl_v.to_string(),
            basis: "leaf 0x1 ECX[24] + MSR 0x11 语义".to_string(),
        },
        KvLine {
            key: "freq_mhz".to_string(),
            value: format!("{}", calib.sealed_freq_mhz),
            basis: format!("median of 3-source cross-check, tol {} permille", calib.tolerance_permille),
        },
        KvLine {
            key: "calib_source".to_string(),
            value: calib.sealed_source.key().to_string(),
            basis: "median source of {cpuid_ratio,pit_model,injected}".to_string(),
        },
        KvLine {
            key: "degrade_tier".to_string(),
            value: tier.key().to_string(),
            basis: "tier rank 0/1/2".to_string(),
        },
        KvLine {
            key: "degrade_reason".to_string(),
            value: reason.key().to_string(),
            basis: "closed-set reason code".to_string(),
        },
        KvLine {
            key: "round_hit".to_string(),
            value: format!("{}/{}", round_hits, TP_ROUNDS),
            basis: "ground-truth cross-check per round".to_string(),
        },
        KvLine {
            key: "degrade_hit".to_string(),
            value: format!("{}/{}", degrade_rounds, TP_ROUNDS),
            basis: "degradation declared per round when tier != 0".to_string(),
        },
        KvLine {
            key: "basis".to_string(),
            value: basis.clone(),
            basis: "bit-level evidence".to_string(),
        },
    ];
    // 自检：九键齐 + 值非空。
    for req in TP_REQUIRED_KEYS.iter() {
        let hit = lines.iter().any(|l| &l.key == req && !l.value.is_empty());
        if !hit {
            return Err(TpCode::ReportKv);
        }
    }
    Ok(FeatureLedger { lines })
}

// ---------------------------------------------------------------------------
// 十三、总入口：全链探测（串联一~十二）
// ---------------------------------------------------------------------------

/// 全链探测报告（一次 probe_all 产出——启动报告直付素材）。
#[derive(Debug, Clone)]
pub struct ProbeReport {
    /// 环境档。
    pub env: EnvKind,
    /// 特性快照（幂等复核后的封账快照）。
    pub snapshot: FeatureSnapshot,
    /// 三源校准账。
    pub calib: CalibLedger,
    /// 降级声明册。
    pub degrade: DegradeDeclaration,
    /// Deadline 联动声明。
    pub deadline: DeadlineDeclaration,
    /// 跨核账。
    pub cross_core: CrossCoreLedger,
    /// 拓扑值域账。
    pub topology: TopologyLedger,
    /// 幂等账。
    pub idem: IdemLedger,
    /// 启动报告 kv。
    pub report: FeatureLedger,
    /// 状态机迁移账。
    pub transitions: Vec<String>,
}

/// 环境档序号（EnvKind::ALL 下标；闭集外 3）。
fn env_rank(env: EnvKind) -> u8 {
    let mut i = 0;
    while i < EnvKind::ALL.len() {
        if EnvKind::ALL[i] == env {
            return i as u8;
        }
        i += 1;
    }
    3
}

/// 全链探测：环境定档 → 特性位判定 → MSR 语义 → 三源对拍 → Deadline
/// 联动 → 跨核/拓扑 → 快照封账 → 幂等复核 → kv 报告。
///
/// 状态机线性推进，任何一步失败即 Err（诊断码 + 中文诊断可 reason()），
/// 无半成品出账。
pub fn probe_all(cpu: &SyntheticCpu, tick: u64) -> Result<ProbeReport, TpCode> {
    let mut st = ProbeState::new();
    let cpuid = SyntheticCpuid::new(cpu);
    let msr = SyntheticMsr::new(cpu);

    // 步一：环境定档 + 特性位判定。
    st.require(ProbePhase::Idle)?;
    let env = classify_env(&cpuid)?;
    let inv = probe_invariant(&cpuid)?;
    let dl_bit = probe_deadline_bit(&cpuid)?;
    st.advance()?;

    // 步二：MSR 0x10/0x11 语义过检（频率口径先借 CPUID/注入粗值）。
    st.require(ProbePhase::CpuidProbed)?;
    let coarse_freq = match freq_from_cpuid(&cpuid) {
        Ok(v) => v,
        Err(TpCode::FreqZero) => freq_injected(cpu)?,
        Err(e) => return Err(e),
    };
    check_msr10_semantics(&msr, 0, tick, coarse_freq)?;
    if cpu.deadline {
        check_msr11_semantics(&msr, 0, tick)?;
    }
    st.advance()?;

    // 步三：三源校准对拍 + 签收频率。
    st.require(ProbePhase::MsrProbed)?;
    let calib = cross_check_sources(cpu, &cpuid, &msr, tick)?;
    st.advance()?;

    // 步四：Deadline 联动 + 跨核 + 拓扑。
    st.require(ProbePhase::CalibCrossed)?;
    let deadline = check_deadline_linkage(cpu, &msr, 0, tick)?;
    let cross_core = probe_cross_core(cpu, &msr, tick)?;
    let topology = probe_topology_ranges(cpu, &msr, tick)?;
    st.advance()?;

    // 步五：降级十轮声明 + 快照封账 + 幂等复核。
    st.require(ProbePhase::CoreScanned)?;
    let degrade = declare_degradation(cpu, tick)?;
    // 十轮特性对拍（判定正确性 10/10 账——判定与真值逐轮对拍）。
    let round_ledgers = probe_three_envs(cpu, tick)?;
    let ledger = match round_ledgers.last() {
        Some(l) => l,
        None => return Err(TpCode::RoundLedger),
    };
    let round_hits = ledger.hits;
    if round_hits != TP_ROUNDS {
        return Err(TpCode::VerdictFlip);
    }
    let snap = FeatureSnapshot {
        invariant: inv,
        deadline: dl_bit && deadline.usable,
        freq_mhz: calib.sealed_freq_mhz,
        tier_rank: degrade.tier.rank(),
        env_rank: env_rank(env),
    };
    let mut idem = IdemLedger::open(&snap);
    // 二次探测：全链只读复算（确定性下恒同），指纹漂移即立案。
    let inv2 = probe_invariant(&cpuid)?;
    let dl2 = probe_deadline_bit(&cpuid)?;
    if inv2 != inv || dl2 != dl_bit {
        return Err(TpCode::IdemDrift);
    }
    idem.recheck(&snap)?;
    st.advance()?;

    // 步六：kv 报告导出。
    let degrade_rounds = if degrade.tier == DegradeTier::InvariantUsable {
        0
    } else {
        degrade.declared_rounds
    };
    let report = export_boot_report(&snap, &calib, degrade.tier, degrade.reason, round_hits, degrade_rounds)?;
    let report = match ensure_sealed(st, report) {
        Ok(r) => r,
        Err(e) => return Err(e),
    };
    Ok(ProbeReport {
        env,
        snapshot: snap,
        calib,
        degrade,
        deadline,
        cross_core,
        topology,
        idem,
        report,
        transitions: Vec::new(),
    })
}

/// 封账守卫：相位必须已到 Sealed 才放行报告。
fn ensure_sealed(st: ProbeState, report: FeatureLedger) -> Result<FeatureLedger, TpCode> {
    if st.phase != ProbePhase::Sealed {
        return Err(TpCode::PhaseOrder);
    }
    Ok(report)
}

// ---------------------------------------------------------------------------
// 十四、段位守卫与闭集自检
// ---------------------------------------------------------------------------

/// 诊断码段自检：全闭集码值必须落在 0x4cf0..=0x4cff 且两两互异。
pub fn audit_codes() -> Result<(), TpCode> {
    for (i, c) in TpCode::ALL.iter().enumerate() {
        if !c.in_segment() {
            return Err(TpCode::SegmentGuard);
        }
        let mut j = i + 1;
        while j < TpCode::ALL.len() {
            if TpCode::ALL[j].val() == c.val() {
                return Err(TpCode::SegmentGuard);
            }
            j += 1;
        }
    }
    Ok(())
}

/// 环境闭集自检：三档名互异、档序覆盖 0..3。
pub fn audit_envs() -> Result<(), TpCode> {
    let mut i = 0;
    while i < EnvKind::ALL.len() {
        let mut j = i + 1;
        while j < EnvKind::ALL.len() {
            if EnvKind::ALL[i].name() == EnvKind::ALL[j].name() {
                return Err(TpCode::EnvUnknown);
            }
            j += 1;
        }
        i += 1;
    }
    Ok(())
}
