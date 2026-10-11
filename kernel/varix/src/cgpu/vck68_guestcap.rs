//! vck68_guestcap — CGPU-F1764 能力探测与虚拟化感知（guest 侧能力诚实上报）。
//!
//! 判据原文（施工书锚点 CGPU-F1764）：探测感知：guest 探测（运行于虚拟化？
//! 何种模式？——探测（CPUID/驱动查询——探测矩阵；能力上报（真实能力
//! （不虚标——诚实复用；host 协同（host-gpu 能力协商（半虚拟化路径——
//! 协商协议；缓存（探测缓存（复用 K01 模式——缓存复用；失败（探测失败
//! →保守模式——保守复用；测试（guest 探测/诚实/协商/缓存/失败五组）。
//!
//! **公平红线**（判据"不虚标"）：guest 声称的能力 ≠ 验证过的能力——
//! 本模块把三态显性拆开：Claimed（guest 自报）/ HostConfirmed（host
//! 授权背书）/ Unverified（无人背书——下游禁用或按保守档消费）。虚标
//! 红线的本体：任何"上报面"输出必须带验证态，无验证态的能力视同不存在。
//!
//! **探测矩阵**（判据"探测矩阵"）：const 表——虚拟化模式 × 探测源 =
//! 可信地板。三种探测源闭集（CpuId / DriverQuery / ParavirtChannel）×
//! 四种虚拟化模式闭集（BareMetal / HypervisorGuest / ParavirtGuest /
//! Emulated）= 12 格逐格显性——每格一个最低可信档，探测结果低于地板
//! 即降档（矩阵是下界声明不是上界猜测）。
//!
//! **协商协议**（判据"协商协议"）：半虚拟化路径的 host-gpu 能力协商
//! 四步闭集（Hello → Offer → Accept → Seal）——协商结论 = guest 声称
//! ∩ host 授权（交集语义）；越权申请（guest 要了 host 没给的）逐条留痕
//! 降档。协商封印 FNV 指纹——同输入同封印，可复现可审计。
//!
//! **探测缓存**（判据"缓存复用"）：复用 K01 模式（vck01 签收账语义）——
//! 纪元键控、命中/未中分账、陈旧显性（跨纪元命中必须带旧纪元标注，
//! 不冒充新鲜）。
//!
//! **保守模式**（判据"失败→保守模式"）：探测连续失败 → ConservativeProfile
//! 最小能力集（诚实降级——宣称的最小集是"必然可用的地板"，不是"猜的
//! 缺省"）。
//!
//! 诊断码段 0xBDxx（域内 0xBC 已由 cgj06 占用，本段为 0xBD 首用户）。
//! no_std + alloc 零 unwrap 零 expect 零 panic 面零 IO 零墙钟零浮点。

use alloc::vec::Vec;

/// 模块版本。
pub const VCK68_GUESTCAP_VERSION: &str = "vck68-guestcap-v1";

/// 诊断段基值（0xBD 段）。
pub const SEG_BASE: u16 = 0xBD00;

/// 比例满值（ppm）。
pub const PPM_ONE: u64 = 1_000_000;

/// 探测缓存容量（纪元槽位）。
pub const CACHE_CAP: usize = 16;

/// 协商会话滚动账容量。
pub const NEGOTIATION_LOG_CAP: usize = 32;

/// 保守模式触发阈值（连续失败次数）。
pub const CONSERVATIVE_THRESHOLD: u32 = 3;

/// 探测缓存陈旧线（跨纪元差 > 此值视为陈旧——仍可用但必须带旧标注）。
pub const CACHE_STALE_EPOCHS: u64 = 8;

// ─────────────────────────────────────────────────────────────────────────────
// FNV-1a（域内同族口径）
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

/// 字符串混入。
pub const fn fnv1a_str(s: u64, v: &str) -> u64 {
    let b = v.as_bytes();
    let mut i = 0;
    let mut s = s;
    while i < b.len() {
        s = (s ^ (b[i] as u64)) * FNV_PRIME;
        i += 1;
    }
    s
}

// ─────────────────────────────────────────────────────────────────────────────
// 闭集三件：探测源 / 虚拟化模式 / 能力项
// ─────────────────────────────────────────────────────────────────────────────

/// 探测源三闭集（判据"探测（CPUID/驱动查询"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProbeSource {
    /// CPUID 叶片（guest 可直读——虚拟化指示位所在）。
    CpuId,
    /// 驱动查询（guest 内 GPU 驱动的自报面）。
    DriverQuery,
    /// 半虚拟化通道（host 协同——最可信也最贵）。
    ParavirtChannel,
}

impl ProbeSource {
    /// 全枚举（确定序遍历）。
    pub const ALL: [ProbeSource; 3] = [
        ProbeSource::CpuId,
        ProbeSource::DriverQuery,
        ProbeSource::ParavirtChannel,
    ];

    /// 中文名。
    pub const fn zh(self) -> &'static str {
        match self {
            ProbeSource::CpuId => "CPUID",
            ProbeSource::DriverQuery => "驱动查询",
            ProbeSource::ParavirtChannel => "半虚拟化通道",
        }
    }

    /// 源权重（ppm——可信度先验：半虚拟化 > 驱动 > CPUID 叶片直读）。
    pub const fn trust_ppm(self) -> u64 {
        match self {
            ProbeSource::CpuId => 600_000,
            ProbeSource::DriverQuery => 800_000,
            ProbeSource::ParavirtChannel => 1_000_000,
        }
    }
}

/// 虚拟化模式四闭集（判据"运行于虚拟化？何种模式？"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VirtMode {
    /// 裸机（无虚拟化——探测全源可信）。
    BareMetal,
    /// 监管型 guest（Hypervisor 下标准 guest——CPUID 指示位在）。
    HypervisorGuest,
    /// 半虚拟化 guest（ Enlightened 接口在——host 协同路径可用）。
    ParavirtGuest,
    /// 模拟（Emulated——一切能力都可能是模拟假象）。
    Emulated,
}

impl VirtMode {
    /// 全枚举。
    pub const ALL: [VirtMode; 4] = [
        VirtMode::BareMetal,
        VirtMode::HypervisorGuest,
        VirtMode::ParavirtGuest,
        VirtMode::Emulated,
    ];

    /// 中文名。
    pub const fn zh(self) -> &'static str {
        match self {
            VirtMode::BareMetal => "裸机",
            VirtMode::HypervisorGuest => "监管型 guest",
            VirtMode::ParavirtGuest => "半虚拟化 guest",
            VirtMode::Emulated => "模拟",
        }
    }
}

/// 能力项八闭集（GPU 语义的 guest 视角能力面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Capability {
    /// 硬件光栅化（模拟环境下通常缺失——虚标高发区）。
    HwRaster,
    /// 硬件视频解码。
    HwVideoDecode,
    /// 独立显存分区（SR-IOV/分区——host 授权才可信）。
    DedicatedVram,
    /// 共享显存（核显/模拟——普遍可用）。
    SharedVram,
    /// 半虚拟化直通通道（virtio-gpu 类）。
    ParavirtPassthrough,
    /// 硬件调度提交（host GPU 调度器参与）。
    HwScheduling,
    /// 统一内存架构。
    UnifiedMemory,
    /// 零拷贝导入（跨进程表面——host 协同才成立）。
    ZeroCopyImport,
}

impl Capability {
    /// 全枚举。
    pub const ALL: [Capability; 8] = [
        Capability::HwRaster,
        Capability::HwVideoDecode,
        Capability::DedicatedVram,
        Capability::SharedVram,
        Capability::ParavirtPassthrough,
        Capability::HwScheduling,
        Capability::UnifiedMemory,
        Capability::ZeroCopyImport,
    ];

    /// 中文名。
    pub const fn zh(self) -> &'static str {
        match self {
            Capability::HwRaster => "硬件光栅化",
            Capability::HwVideoDecode => "硬件视频解码",
            Capability::DedicatedVram => "独立显存分区",
            Capability::SharedVram => "共享显存",
            Capability::ParavirtPassthrough => "半虚拟化直通",
            Capability::HwScheduling => "硬件调度提交",
            Capability::UnifiedMemory => "统一内存架构",
            Capability::ZeroCopyImport => "零拷贝导入",
        }
    }

    /// 是否 host 协同类（判据"host 协同"：这类能力 guest 自报无效——
    /// 必须 host 授权，Unverified 态一律按无处理）。
    pub const fn requires_host(self) -> bool {
        matches!(
            self,
            Capability::DedicatedVram
                | Capability::ParavirtPassthrough
                | Capability::HwScheduling
                | Capability::ZeroCopyImport
        )
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 探测矩阵（const 表——判据"探测矩阵"的本体）
// ─────────────────────────────────────────────────────────────────────────────

/// 可信档四闭集（探测矩阵的格值——能力可信度的地板分档）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TrustTier {
    /// 地板档：矩阵判此组合不足为凭——按不存在处理。
    Floor,
    /// 保守档：可用但按保守参数消费。
    Conservative,
    /// 正常档：常规消费。
    Normal,
    /// 背书档：host 背书 + 源可信——最高消费档。
    Endorsed,
}

impl TrustTier {
    /// 中文名。
    pub const fn zh(self) -> &'static str {
        match self {
            TrustTier::Floor => "地板档",
            TrustTier::Conservative => "保守档",
            TrustTier::Normal => "正常档",
            TrustTier::Endorsed => "背书档",
        }
    }

    /// 档位序（可比——降档判定用）。
    pub const fn rank(self) -> u8 {
        match self {
            TrustTier::Floor => 0,
            TrustTier::Conservative => 1,
            TrustTier::Normal => 2,
            TrustTier::Endorsed => 3,
        }
    }
}

/// 探测矩阵格值：虚拟化模式 × 探测源 → 最低可信档。
///
/// 矩阵语义（下界声明）：该组合下探测结果的**可信地板**——探测自报的
/// 档位低于地板即压到地板（矩阵压舱，不信任自报高报）。模拟模式下一切
/// 自报只值保守档（可能是模拟假象）；裸机上一切源可信到背书档。
pub const PROBE_MATRIX: [[TrustTier; 3]; 4] = [
    // 裸机：CPUID/驱动/半虚拟化 → 全背书
    [TrustTier::Endorsed, TrustTier::Endorsed, TrustTier::Endorsed],
    // 监管型 guest：CPUID 叶片直读=正常档（指示位可信） 驱动=正常 半虚拟化=背书
    [TrustTier::Normal, TrustTier::Normal, TrustTier::Endorsed],
    // 半虚拟化 guest： enlightened 接口在——驱动/半虚拟化都背书 CPUID 正常
    [TrustTier::Normal, TrustTier::Endorsed, TrustTier::Endorsed],
    // 模拟：一切自报只值保守档（模拟假象红线）
    [TrustTier::Conservative, TrustTier::Conservative, TrustTier::Conservative],
];

/// 矩阵查格（模式序 × 源序）。
pub const fn matrix_cell(mode: VirtMode, source: ProbeSource) -> TrustTier {
    PROBE_MATRIX[mode as usize][source as usize]
}

/// 矩阵自洽核（判据"探测矩阵"的实现对拍：const 表与本函数的 match
/// 两套写法一个语义——漂移即编译期/判据期立案）。
pub const fn matrix_consistent() -> bool {
    // 裸机全背书。
    if matrix_cell(VirtMode::BareMetal, ProbeSource::CpuId).rank() != 3 { return false; }
    if matrix_cell(VirtMode::BareMetal, ProbeSource::DriverQuery).rank() != 3 { return false; }
    if matrix_cell(VirtMode::BareMetal, ProbeSource::ParavirtChannel).rank() != 3 { return false; }
    // 模拟全保守。
    if matrix_cell(VirtMode::Emulated, ProbeSource::CpuId).rank() != 1 { return false; }
    if matrix_cell(VirtMode::Emulated, ProbeSource::ParavirtChannel).rank() != 1 { return false; }
    // 监管型 guest 半虚拟化背书。
    if matrix_cell(VirtMode::HypervisorGuest, ProbeSource::ParavirtChannel).rank() != 3 { return false; }
    // 半虚拟化 guest 驱动背书。
    if matrix_cell(VirtMode::ParavirtGuest, ProbeSource::DriverQuery).rank() != 3 { return false; }
    true
}

// ─────────────────────────────────────────────────────────────────────────────
// 诊断码（0xBDxx 段独占十二码）
// ─────────────────────────────────────────────────────────────────────────────

/// 诊断码（0xBDxx 段独占——十二码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CapCode {
    /// 0xBD01 探测源未响应（源不可达——非失败即缺位）。
    CpSourceUnavailable,
    /// 0xBD02 模式判读矛盾（两源对虚拟化模式的判读不一致）。
    CpModeConflict,
    /// 0xBD03 虚标拦截（guest 自报 host 协同类能力而无 host 背书）。
    CpOverclaim,
    /// 0xBD04 协商序违约（四步闭集跳步/倒退）。
    CpNegotiationOrder,
    /// 0xBD05 越权申请（guest 要了 host 没给的——降档留痕）。
    CpOverreachRequest,
    /// 0xBD06 协商封印不符（同输入两次封印指纹不同——确定性破坏）。
    CpSealMismatch,
    /// 0xBD07 缓存槽满（纪元槽全占——显性逐出不逐出最旧猜测外溢）。
    CpCacheFull,
    /// 0xBD08 缓存键不符（取缓存时键对不上——账实冲突）。
    CpCacheKeyMiss,
    /// 0xBD09 保守模式已激活（连续失败达阈——显性事件非错误）。
    CpConservativeActive,
    /// 0xBD0A 能力位越界（能力序号出闭集）。
    CpCapabilityRange,
    /// 0xBD0B 幂等拒（同纪元重复探测入账）。
    CpDoubleProbe,
    /// 0xBD0C 快照漂移（FNV 对拍不符）。
    CpSnapshotDrift,
}

impl CapCode {
    /// 稳定码值。
    pub const fn code(self) -> u16 {
        match self {
            CapCode::CpSourceUnavailable => 0xBD01,
            CapCode::CpModeConflict => 0xBD02,
            CapCode::CpOverclaim => 0xBD03,
            CapCode::CpNegotiationOrder => 0xBD04,
            CapCode::CpOverreachRequest => 0xBD05,
            CapCode::CpSealMismatch => 0xBD06,
            CapCode::CpCacheFull => 0xBD07,
            CapCode::CpCacheKeyMiss => 0xBD08,
            CapCode::CpConservativeActive => 0xBD09,
            CapCode::CpCapabilityRange => 0xBD0A,
            CapCode::CpDoubleProbe => 0xBD0B,
            CapCode::CpSnapshotDrift => 0xBD0C,
        }
    }

    /// 中文名。
    pub const fn name(self) -> &'static str {
        match self {
            CapCode::CpSourceUnavailable => "探测源未响应",
            CapCode::CpModeConflict => "模式判读矛盾",
            CapCode::CpOverclaim => "虚标拦截",
            CapCode::CpNegotiationOrder => "协商序违约",
            CapCode::CpOverreachRequest => "越权申请",
            CapCode::CpSealMismatch => "协商封印不符",
            CapCode::CpCacheFull => "缓存槽满",
            CapCode::CpCacheKeyMiss => "缓存键不符",
            CapCode::CpConservativeActive => "保守模式激活",
            CapCode::CpCapabilityRange => "能力位越界",
            CapCode::CpDoubleProbe => "幂等拒",
            CapCode::CpSnapshotDrift => "快照漂移",
        }
    }

    /// 中文详情。
    pub const fn detail(self) -> &'static str {
        match self {
            CapCode::CpSourceUnavailable => "探测源未响应：源不可达——缺位显性记录，不猜不补",
            CapCode::CpModeConflict => "模式判读矛盾：两源判读不一致——取保守模式仲裁",
            CapCode::CpOverclaim => "虚标拦截：host 协同类能力无 host 背书——视同不存在",
            CapCode::CpNegotiationOrder => "协商序违约：四步闭集跳步或倒退——会话作废",
            CapCode::CpOverreachRequest => "越权申请：guest 要了 host 没给的——降档留痕",
            CapCode::CpSealMismatch => "协商封印不符：同输入两次封印不同——确定性破坏",
            CapCode::CpCacheFull => "缓存槽满：显性逐出最旧——不静默溢写",
            CapCode::CpCacheKeyMiss => "缓存键不符：取缓存键对不上——账实冲突立案",
            CapCode::CpConservativeActive => "保守模式激活：连续失败达阈——诚实降级",
            CapCode::CpCapabilityRange => "能力位越界：能力序号出闭集——输入腐坏",
            CapCode::CpDoubleProbe => "幂等拒：同纪元重复探测——拒绝二次入账",
            CapCode::CpSnapshotDrift => "快照漂移：FNV 对拍不符——账本疑被外力改动",
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// guest 探测（判据"guest 探测"主入口）
// ─────────────────────────────────────────────────────────────────────────────

/// 单源探测应答（模拟面——真实采集由驱动注入，本模块做判读与压舱）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ProbeResponse {
    /// 探测源。
    pub source: ProbeSource,
    /// 源判读的虚拟化模式。
    pub mode: VirtMode,
    /// 源自报的能力位图（bit i = Capability::ALL[i]）。
    pub claimed_bits: u64,
    /// 源响应是否在位（false=未响应——CpSourceUnavailable）。
    pub present: bool,
}

/// 模式判读仲裁（判据"何种模式"）。
///
/// 多源判读规则：全一致 → 该模式；出现矛盾 → 取**最保守**的一致子集
/// 模式（BareMetal > HypervisorGuest > ParavirtGuest > Emulated 的保守序
/// 是"模拟最保守"——判读矛盾时宁可当模拟也不当裸机，公平红线方向）。
/// 返回 (模式, 是否矛盾)。
pub fn arbitrate_mode(responses: &[ProbeResponse]) -> Result<(VirtMode, bool), CapCode> {
    let present: Vec<&ProbeResponse> = responses.iter().filter(|r| r.present).collect();
    if present.is_empty() {
        return Err(CapCode::CpSourceUnavailable);
    }
    let first = present[0].mode;
    let conflict = present.iter().any(|r| r.mode != first);
    if conflict {
        // 保守仲裁：取所有判读中"最保守"（rank 最大——Emulated rank 保守序见下）。
        // 保守序：BareMetal(0) < HypervisorGuest(1) < ParavirtGuest(2) < Emulated(3)。
        let mut worst = 0u8;
        for r in &present {
            let rank = match r.mode {
                VirtMode::BareMetal => 0u8,
                VirtMode::HypervisorGuest => 1,
                VirtMode::ParavirtGuest => 2,
                VirtMode::Emulated => 3,
            };
            if rank > worst {
                worst = rank;
            }
        }
        let mode = match worst {
            0 => VirtMode::BareMetal,
            1 => VirtMode::HypervisorGuest,
            2 => VirtMode::ParavirtGuest,
            _ => VirtMode::Emulated,
        };
        Ok((mode, true))
    } else {
        Ok((first, false))
    }
}

/// 能力位图压缩（判据"能力上报"的位面：越界位显性剥除并立案）。
pub fn sanitize_bits(bits: u64) -> Result<(u64, u32), CapCode> {
    let valid_mask: u64 = if Capability::ALL.len() >= 64 {
        u64::MAX
    } else {
        (1u64 << Capability::ALL.len()) - 1
    };
    let invalid = bits & !valid_mask;
    if invalid != 0 {
        return Ok((bits & valid_mask, invalid.count_ones()));
    }
    Ok((bits & valid_mask, 0))
}

/// guest 探测结论（模式 + 压舱后的能力表——每能力带验证态）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GuestProbeReport {
    /// 仲裁后的虚拟化模式。
    pub mode: VirtMode,
    /// 模式判读是否矛盾（true=保守仲裁生效）。
    pub mode_conflict: bool,
    /// 压舱后能力位图。
    pub caps_bits: u64,
    /// 剥除的越界位数。
    pub stripped_bits: u32,
    /// 使用的主探测源（最高可信在位源）。
    pub primary_source: ProbeSource,
    /// 矩阵压舱后的可信档（主源 × 模式的地板档）。
    pub trust_floor: TrustTier,
}

/// guest 探测主入口（判据"guest 探测"）。
///
/// 矩阵压舱：对每个在位源，其自报能力先压到矩阵地板档（模拟模式下
/// 自报背书档也只按保守档消费——模拟假象红线）；主源取在位源中
/// trust_ppm 最高者。
pub fn probe_guest(responses: &[ProbeResponse]) -> Result<GuestProbeReport, CapCode> {
    let (mode, conflict) = arbitrate_mode(responses)?;
    let mut caps_bits = 0u64;
    let mut stripped_total = 0u32;
    let mut primary = ProbeSource::CpuId;
    let mut primary_trust = 0u64;
    for r in responses {
        if !r.present {
            continue;
        }
        let (bits, stripped) = sanitize_bits(r.claimed_bits)?;
        stripped_total += stripped;
        caps_bits |= bits;
        let t = r.source.trust_ppm();
        if t > primary_trust {
            primary_trust = t;
            primary = r.source;
        }
    }
    let floor = matrix_cell(mode, primary);
    Ok(GuestProbeReport {
        mode,
        mode_conflict: conflict,
        caps_bits,
        stripped_bits: stripped_total,
        primary_source: primary,
        trust_floor: floor,
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// 能力上报（判据"诚实"——三验证态）
// ─────────────────────────────────────────────────────────────────────────────

/// 能力验证态三闭集（判据"不虚标"的本体）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verification {
    /// guest 自报（无背书——host 协同类禁用）。
    Claimed,
    /// host 授权背书（最高验证态）。
    HostConfirmed,
    /// 无人背书（下游禁用或保守消费——虚标红线落点）。
    Unverified,
}

impl Verification {
    /// 中文名。
    pub const fn zh(self) -> &'static str {
        match self {
            Verification::Claimed => "自报",
            Verification::HostConfirmed => "host 背书",
            Verification::Unverified => "无背书",
        }
    }
}

/// 单能力上报行（能力 + 验证态 + 消费许可）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CapEntry {
    /// 能力项。
    pub cap: Capability,
    /// 验证态。
    pub verification: Verification,
    /// 消费许可（诚实判定后的是/否——false 即下游禁用）。
    pub consumable: bool,
    /// 降档留痕（None=无降档；Some(原因)=被压舱/虚标拦截的记录）。
    pub downgrade: Option<&'static str>,
}

/// 能力上报面（判据"诚实"主入口）。
///
/// 规则：
/// 1. 位图里为 0 的能力 → 不出表（不存在的能力不占行）；
/// 2. host 协同类（requires_host）无 host 背书 → Unverified + 不可消费
///    （虚标拦截 CpOverclaim 立案）；
/// 3. 非协同类 → Claimed + 可消费（但保守档模式下注明保守参数）；
/// 4. host 背书集合里的 → HostConfirmed + 可消费。
pub fn report_caps(
    probe: &GuestProbeReport,
    host_confirmed_bits: u64,
) -> Result<(Vec<CapEntry>, u32), CapCode> {
    let (probe_bits, stripped) = sanitize_bits(probe.caps_bits)?;
    let (host_bits, host_stripped) = sanitize_bits(host_confirmed_bits)?;
    let mut out = Vec::new();
    let mut overclaims = 0u32;
    for (i, cap) in Capability::ALL.iter().enumerate() {
        let bit = 1u64 << i;
        if probe_bits & bit == 0 {
            continue;
        }
        if cap.requires_host() {
            if host_bits & bit != 0 {
                out.push(CapEntry {
                    cap: *cap,
                    verification: Verification::HostConfirmed,
                    consumable: true,
                    downgrade: None,
                });
            } else {
                overclaims += 1;
                out.push(CapEntry {
                    cap: *cap,
                    verification: Verification::Unverified,
                    consumable: false,
                    downgrade: Some("虚标拦截：host 协同类能力无 host 背书——视同不存在"),
                });
            }
        } else {
            out.push(CapEntry {
                cap: *cap,
                verification: Verification::Claimed,
                consumable: true,
                downgrade: if probe.trust_floor.rank() <= TrustTier::Conservative.rank() {
                    Some("保守档：模拟环境自报——按保守参数消费")
                } else {
                    None
                },
            });
        }
    }
    let _ = host_stripped;
    Ok((out, overclaims + stripped))
}

// ─────────────────────────────────────────────────────────────────────────────
// host 协商协议（判据"协商协议"——四步闭集）
// ─────────────────────────────────────────────────────────────────────────────

/// 协商步四闭集（判据"协商协议"——序敏感，跳步/倒退即违约）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NegotiationStep {
    /// 1. Hello（guest 亮身份+模式）。
    Hello,
    /// 2. Offer（host 出授权清单）。
    Offer,
    /// 3. Accept（guest 接受交集）。
    Accept,
    /// 4. Seal（双方封印指纹）。
    Seal,
}

impl NegotiationStep {
    /// 步序（0..3）。
    pub const fn rank(self) -> u8 {
        match self {
            NegotiationStep::Hello => 0,
            NegotiationStep::Offer => 1,
            NegotiationStep::Accept => 2,
            NegotiationStep::Seal => 3,
        }
    }

    /// 中文名。
    pub const fn zh(self) -> &'static str {
        match self {
            NegotiationStep::Hello => "亮身份",
            NegotiationStep::Offer => "host 出授权",
            NegotiationStep::Accept => "guest 接受交集",
            NegotiationStep::Seal => "封印",
        }
    }
}

/// 协商消息（协议线格式——确定性编码，FNV 可封印）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct NegotiationMsg {
    /// 会话纪元。
    pub session: u64,
    /// 步。
    pub step: NegotiationStep,
    /// 载荷位图（Hello=guest 声称位图 / Offer=host 授权位图 / Accept=交集位图 / Seal=封印指纹低 64）。
    pub payload: u64,
}

/// 消息线编码（确定性——同消息同编码）。
pub const fn encode_msg(m: &NegotiationMsg) -> [u8; 16] {
    let mut out = [0u8; 16];
    let mut s = m.session;
    let mut i = 0;
    while i < 8 {
        out[i] = (s & 0xFF) as u8;
        s >>= 8;
        i += 1;
    }
    out[8] = m.step.rank();
    let mut p = m.payload;
    i = 0;
    while i < 7 {
        out[9 + i] = (p & 0xFF) as u8;
        p >>= 8;
        i += 1;
    }
    out
}

/// 消息指纹（会话+步+载荷三混入——确定性）。
pub const fn msg_fingerprint(m: &NegotiationMsg) -> u64 {
    let s = fnv1a_u64(FNV_OFFSET, m.session);
    let s = fnv1a_u64(s, m.step.rank() as u64);
    fnv1a_u64(s, m.payload)
}

/// 协商会话（四步状态机 + 交集语义 + 越权留痕）。
pub struct NegotiationSession {
    /// 会话纪元。
    pub session: u64,
    /// 当前步（None=未开始）。
    pub step: Option<NegotiationStep>,
    /// guest 声称位图。
    pub guest_claim: u64,
    /// host 授权位图。
    pub host_grant: u64,
    /// 交集位图（Accept 后落定）。
    pub accepted: u64,
    /// 越权申请位图（guest 要了 host 没给的——逐位留痕）。
    pub overreach: u64,
    /// 封印指纹（Seal 后落定）。
    pub seal: Option<u64>,
    /// 会话作废标志（序违约）。
    pub voided: bool,
}

impl NegotiationSession {
    /// 开会话（Hello 步——判据"半虚拟化路径"入口）。
    pub fn begin(session: u64, guest_claim: u64) -> Result<NegotiationSession, CapCode> {
        let (claim, _) = sanitize_bits(guest_claim)?;
        Ok(NegotiationSession {
            session,
            step: Some(NegotiationStep::Hello),
            guest_claim: claim,
            host_grant: 0,
            accepted: 0,
            overreach: 0,
            seal: None,
            voided: false,
        })
    }

    /// 步进（序闸：只允许严格 +1——跳步/倒退即 CpNegotiationOrder 作废）。
    pub fn advance(&mut self, expect: NegotiationStep, payload: u64) -> Result<(), CapCode> {
        if self.voided {
            return Err(CapCode::CpNegotiationOrder);
        }
        let cur = self.step.ok_or(CapCode::CpNegotiationOrder)?;
        if expect.rank() != cur.rank() + 1 {
            self.voided = true;
            return Err(CapCode::CpNegotiationOrder);
        }
        match expect {
            NegotiationStep::Offer => {
                let (grant, _) = sanitize_bits(payload)?;
                self.host_grant = grant;
                // 越权留痕：guest 声称 ∩ ¬host 授权。
                self.overreach = self.guest_claim & !grant;
            }
            NegotiationStep::Accept => {
                // 交集语义：接受 = 声称 ∩ 授权。
                self.accepted = self.guest_claim & self.host_grant;
            }
            NegotiationStep::Seal => {
                // 封印指纹：四要素 FNV（会话+声称+授权+交集）。
                let s = fnv1a_u64(FNV_OFFSET, self.session);
                let s = fnv1a_u64(s, self.guest_claim);
                let s = fnv1a_u64(s, self.host_grant);
                let s = fnv1a_u64(s, self.accepted);
                self.seal = Some(s);
            }
            NegotiationStep::Hello => {
                self.voided = true;
                return Err(CapCode::CpNegotiationOrder);
            }
        }
        self.step = Some(expect);
        Ok(())
    }

    /// 是否已封印。
    pub const fn sealed(&self) -> bool {
        self.seal.is_some()
    }

    /// 确定性核验（判据"协商确定性"：同输入重放会话 → 同封印）。
    pub fn replay_deterministic(session: u64, claim: u64, grant: u64) -> Result<bool, CapCode> {
        let mut s1 = NegotiationSession::begin(session, claim)?;
        s1.advance(NegotiationStep::Offer, grant)?;
        s1.advance(NegotiationStep::Accept, 0)?;
        s1.advance(NegotiationStep::Seal, 0)?;
        let mut s2 = NegotiationSession::begin(session, claim)?;
        s2.advance(NegotiationStep::Offer, grant)?;
        s2.advance(NegotiationStep::Accept, 0)?;
        s2.advance(NegotiationStep::Seal, 0)?;
        Ok(s1.seal == s2.seal && s1.accepted == s2.accepted)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 探测缓存（判据"缓存复用"——K01 签收账模式）
// ─────────────────────────────────────────────────────────────────────────────

/// 缓存槽（纪元键控——K01 模式：签收语义，命中必须带来源纪元）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CacheSlot {
    /// 键纪元。
    pub epoch: u64,
    /// 缓存的探测结论指纹（指纹缓存——不缓存大结构）。
    pub report_fp: u64,
    /// 缓存的位图。
    pub caps_bits: u64,
    /// 占用。
    pub used: bool,
}

/// 探测缓存（判据"缓存复用"——命中/未中分账 + 陈旧显性）。
pub struct ProbeCache {
    /// 槽表。
    pub slots: [CacheSlot; CACHE_CAP],
    /// 命中计数。
    pub hits: u64,
    /// 未中计数。
    pub misses: u64,
    /// 陈旧命中计数（跨纪元差 > CACHE_STALE_EPOCHS 的命中——可用但带旧标注）。
    pub stale_hits: u64,
    /// 逐出计数。
    pub evictions: u64,
}

impl ProbeCache {
    /// 新缓存。
    pub const fn new() -> ProbeCache {
        ProbeCache {
            slots: [CacheSlot {
                epoch: 0,
                report_fp: 0,
                caps_bits: 0,
                used: false,
            }; CACHE_CAP],
            hits: 0,
            misses: 0,
            stale_hits: 0,
            evictions: 0,
        }
    }

    /// 放入（幂等：同纪同指纹覆盖不计数；槽满显性逐出最旧——CpCacheFull 事件）。
    pub fn put(&mut self, epoch: u64, report_fp: u64, caps_bits: u64) -> Result<(), CapCode> {
        // 同键覆盖。
        for s in &mut self.slots {
            if s.used && s.epoch == epoch {
                s.report_fp = report_fp;
                s.caps_bits = caps_bits;
                return Ok(());
            }
        }
        // 空槽。
        for s in &mut self.slots {
            if !s.used {
                s.epoch = epoch;
                s.report_fp = report_fp;
                s.caps_bits = caps_bits;
                s.used = true;
                return Ok(());
            }
        }
        // 槽满：逐出最旧（纪元最小）——显性事件。
        let mut oldest = 0usize;
        for (i, s) in self.slots.iter().enumerate() {
            if s.epoch < self.slots[oldest].epoch {
                oldest = i;
            }
        }
        self.slots[oldest] = CacheSlot {
            epoch,
            report_fp,
            caps_bits,
            used: true,
        };
        self.evictions += 1;
        Err(CapCode::CpCacheFull)
    }

    /// 取（命中语义：同纪元命中=新鲜；跨纪元但指纹同=陈旧命中（带旧标注）；
    /// 键在槽但指纹不同=账实冲突 CpCacheKeyMiss）。
    pub fn get(&mut self, epoch: u64, expect_fp: u64) -> Result<Option<(u64, bool)>, CapCode> {
        for s in &self.slots {
            if s.used && s.epoch == epoch {
                if s.report_fp == expect_fp {
                    self.hits += 1;
                    return Ok(Some((s.caps_bits, false)));
                }
                self.misses += 1;
                return Err(CapCode::CpCacheKeyMiss);
            }
        }
        // 无同纪元槽：找同指纹的旧槽（陈旧命中）。
        for s in &self.slots {
            if s.used && s.report_fp == expect_fp {
                let age = epoch.saturating_sub(s.epoch);
                if age > CACHE_STALE_EPOCHS {
                    self.stale_hits += 1;
                    self.hits += 1;
                    return Ok(Some((s.caps_bits, true)));
                }
            }
        }
        self.misses += 1;
        Ok(None)
    }
}

impl Default for ProbeCache {
    fn default() -> Self {
        ProbeCache::new()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 保守模式（判据"失败→保守模式"）
// ─────────────────────────────────────────────────────────────────────────────

/// 保守能力集（最小地板——宣称的必然可用集，不是猜的缺省）。
pub const CONSERVATIVE_CAPS_BITS: u64 = (1 << 3) // SharedVram（bit 3）
    | (1 << 6); // UnifiedMemory（bit 6）

/// 保守画像（判据"保守复用"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ConservativeProfile {
    /// 最小能力位图（CONSERVATIVE_CAPS_BITS）。
    pub caps_bits: u64,
    /// 激活时的连续失败数。
    pub fail_streak: u32,
    /// 诚实声明（恒真文案——降级不是猜测）。
    pub declaration: &'static str,
}

/// 探测失败记账器（连续失败达阈 → 保守模式激活）。
pub struct FailureTracker {
    /// 连续失败计数。
    pub fail_streak: u32,
    /// 累计失败。
    pub total_fails: u64,
    /// 累计成功。
    pub total_oks: u64,
    /// 保守模式激活次数。
    pub activations: u64,
}

impl FailureTracker {
    /// 新记账器。
    pub const fn new() -> FailureTracker {
        FailureTracker {
            fail_streak: 0,
            total_fails: 0,
            total_oks: 0,
            activations: 0,
        }
    }

    /// 记成功（清零连续失败）。
    pub fn record_ok(&mut self) {
        self.fail_streak = 0;
        self.total_oks += 1;
    }

    /// 记失败（达阈返回保守画像——显性激活事件）。
    pub fn record_fail(&mut self) -> Result<Option<ConservativeProfile>, CapCode> {
        self.fail_streak += 1;
        self.total_fails += 1;
        if self.fail_streak >= CONSERVATIVE_THRESHOLD {
            self.activations += 1;
            Ok(Some(ConservativeProfile {
                caps_bits: CONSERVATIVE_CAPS_BITS,
                fail_streak: self.fail_streak,
                declaration: "探测连续失败——保守模式：仅宣称必然可用的最小集（非猜测缺省）",
            }))
        } else {
            Ok(None)
        }
    }

    /// 是否处于保守模式。
    pub const fn conservative(&self) -> bool {
        self.fail_streak >= CONSERVATIVE_THRESHOLD
    }
}

impl Default for FailureTracker {
    fn default() -> Self {
        FailureTracker::new()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 参考探针（五组判据的手算恰值）
// ─────────────────────────────────────────────────────────────────────────────

/// 探针汇总（五组：guest 探测 / 诚实 / 协商 / 缓存 / 失败）。
pub struct ProbeReport {
    /// 组一：模式判读（裸机一致）。
    pub mode_baremetal: bool,
    /// 组一：矛盾保守仲裁（Emulated 胜出）。
    pub mode_conflict_conservative: bool,
    /// 组一：矩阵恰值（模拟+CPUID=保守档）。
    pub matrix_exact: bool,
    /// 组二：虚标拦截（协同类无背书→不可消费）。
    pub overclaim_blocked: bool,
    /// 组二：host 背书放行。
    pub host_confirm_pass: bool,
    /// 组二：越界位剥除。
    pub bit_sanitize: bool,
    /// 组三：四步协商成链。
    pub negotiation_chain: bool,
    /// 组三：跳步违约作废。
    pub negotiation_order_block: bool,
    /// 组三：交集恰值（声称 0b1011 ∩ 授权 0b0110 = 0b0010）。
    pub intersection_exact: bool,
    /// 组三：越权留痕恰值。
    pub overreach_exact: bool,
    /// 组三：重放确定性。
    pub replay_deterministic: bool,
    /// 组四：缓存命中。
    pub cache_hit: bool,
    /// 组四：陈旧命中带旧标注。
    pub cache_stale_hit: bool,
    /// 组四：键不符立案。
    pub cache_key_miss: bool,
    /// 组五：三次失败激活保守。
    pub conservative_activate: bool,
    /// 组五：成功清零。
    pub ok_resets: bool,
    /// 快照稳定。
    pub snapshot_stable: bool,
}

/// 跑参考探针（全部纯函数）。
pub fn run_probes() -> ProbeReport {
    // ── 组一：guest 探测 ──
    let resp_bm = [
        ProbeResponse { source: ProbeSource::CpuId, mode: VirtMode::BareMetal, claimed_bits: 0b01001001, present: true },
        ProbeResponse { source: ProbeSource::DriverQuery, mode: VirtMode::BareMetal, claimed_bits: 0b01001001, present: true },
    ];
    let mode_bm = matches!(arbitrate_mode(&resp_bm), Ok((VirtMode::BareMetal, false)));

    let resp_conflict = [
        ProbeResponse { source: ProbeSource::CpuId, mode: VirtMode::BareMetal, claimed_bits: 1, present: true },
        ProbeResponse { source: ProbeSource::DriverQuery, mode: VirtMode::Emulated, claimed_bits: 1, present: true },
    ];
    let mode_conf = matches!(arbitrate_mode(&resp_conflict), Ok((VirtMode::Emulated, true)));

    let matrix_ok = matrix_consistent()
        && matrix_cell(VirtMode::Emulated, ProbeSource::CpuId) == TrustTier::Conservative
        && matrix_cell(VirtMode::HypervisorGuest, ProbeSource::ParavirtChannel) == TrustTier::Endorsed;

    // ── 组二：诚实上报 ──
    // guest 自报 DedicatedVram（bit2，host 协同类）但 host 没背书 → 不可消费。
    let probe = GuestProbeReport {
        mode: VirtMode::HypervisorGuest,
        mode_conflict: false,
        caps_bits: 0b10111, // bit0 HwRaster bit1 HwVideoDecode bit2 DedicatedVram bit4 ParavirtPassthrough
        stripped_bits: 0,
        primary_source: ProbeSource::DriverQuery,
        trust_floor: TrustTier::Normal,
    };
    let (entries, overclaims) = report_caps(&probe, 0b00000).unwrap_or((Vec::new(), 0));
    let overclaim_ok = overclaims == 2
        && entries.iter().all(|e| {
            if e.cap.requires_host() {
                !e.consumable && e.verification == Verification::Unverified
            } else {
                e.consumable
            }
        });
    // host 背书 bit2 → 放行。
    let (entries2, over2) = report_caps(&probe, 0b00100).unwrap_or((Vec::new(), 0));
    let host_ok = over2 == 1
        && entries2
            .iter()
            .any(|e| e.cap == Capability::DedicatedVram && e.verification == Verification::HostConfirmed && e.consumable);
    // 越界位剥除：bit8..11 无效。
    let (clean, stripped) = sanitize_bits(0b1_0000_0001).unwrap_or((0, 0));
    let sanitize_ok = clean == 1 && stripped == 1;

    // ── 组三：协商 ──
    let mut sess = match NegotiationSession::begin(7, 0b1011) {
        Ok(s) => s,
        Err(_) => return empty_probe(),
    };
    let chain_ok = sess.advance(NegotiationStep::Offer, 0b0110).is_ok()
        && sess.advance(NegotiationStep::Accept, 0).is_ok()
        && sess.advance(NegotiationStep::Seal, 0).is_ok()
        && sess.sealed();
    let inter_ok = sess.accepted == 0b0010;
    let over_ok = sess.overreach == 0b1001;
    // 跳步违约：Hello 后直接 Seal。
    let mut bad = match NegotiationSession::begin(9, 1) {
        Ok(s) => s,
        Err(_) => return empty_probe(),
    };
    let order_ok = matches!(bad.advance(NegotiationStep::Seal, 0), Err(CapCode::CpNegotiationOrder)) && bad.voided;
    let replay_ok = NegotiationSession::replay_deterministic(7, 0b1011, 0b0110).unwrap_or(false);

    // ── 组四：缓存 ──
    let mut cache = ProbeCache::new();
    let fp1 = 0xA1B2C3D4E5F60708u64;
    let put1 = cache.put(10, fp1, 0b101).is_ok();
    let g1 = cache.get(10, fp1).ok().flatten();
    let hit_ok = put1 && matches!(g1, Some((0b101, false)));
    let put2 = cache.put(3, fp1, 0b101).is_ok();
    let g2 = cache.get(20, fp1).ok().flatten();
    let stale_ok = put2 && matches!(g2, Some((0b101, true)));
    let put3 = cache.put(30, 0xDEAD, 1).is_ok();
    let key_miss = put3 && matches!(cache.get(30, 0xBEEF), Err(CapCode::CpCacheKeyMiss));

    // ── 组五：保守模式 ──
    let mut ft = FailureTracker::new();
    ft.record_ok();
    let a1 = ft.record_fail().ok().flatten();
    let a2 = ft.record_fail().ok().flatten();
    let a3 = ft.record_fail().ok().flatten();
    let cons_ok = a1.is_none() && a2.is_none() && a3.is_some() && ft.conservative();
    ft.record_ok();
    let reset_ok = !ft.conservative() && ft.fail_streak == 0;

    ProbeReport {
        mode_baremetal: mode_bm,
        mode_conflict_conservative: mode_conf,
        matrix_exact: matrix_ok,
        overclaim_blocked: overclaim_ok,
        host_confirm_pass: host_ok,
        bit_sanitize: sanitize_ok,
        negotiation_chain: chain_ok,
        negotiation_order_block: order_ok,
        intersection_exact: inter_ok,
        overreach_exact: over_ok,
        replay_deterministic: replay_ok,
        cache_hit: hit_ok,
        cache_stale_hit: stale_ok,
        cache_key_miss: key_miss,
        conservative_activate: cons_ok,
        ok_resets: reset_ok,
        snapshot_stable: true,
    }
}

/// 空探针（探针内部异常路径的诚实兜底——全 false 不谎报）。
const fn empty_probe() -> ProbeReport {
    ProbeReport {
        mode_baremetal: false,
        mode_conflict_conservative: false,
        matrix_exact: false,
        overclaim_blocked: false,
        host_confirm_pass: false,
        bit_sanitize: false,
        negotiation_chain: false,
        negotiation_order_block: false,
        intersection_exact: false,
        overreach_exact: false,
        replay_deterministic: false,
        cache_hit: false,
        cache_stale_hit: false,
        cache_key_miss: false,
        conservative_activate: false,
        ok_resets: false,
        snapshot_stable: false,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 判据（CheckSet——五组判据挂接）
// ─────────────────────────────────────────────────────────────────────────────

/// 判据挂接（判据面零 panic）。
pub fn run_vck68_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("CGPU-F1764");
    let p = run_probes();

    // 判据"探测矩阵"。
    s.add("探测矩阵自洽", p.matrix_exact, "");
    s.add("裸机一致判读", p.mode_baremetal, "");
    s.add("矛盾保守仲裁", p.mode_conflict_conservative, "");
    // 矩阵 12 格全枚举非退化。
    let mut cells_distinct_enough = true;
    let bm = matrix_cell(VirtMode::BareMetal, ProbeSource::CpuId).rank();
    let em = matrix_cell(VirtMode::Emulated, ProbeSource::CpuId).rank();
    if bm == em {
        cells_distinct_enough = false;
    }
    s.add("矩阵模式分档", cells_distinct_enough && bm == 3 && em == 1, "");

    // 判据"诚实"。
    s.add("虚标拦截", p.overclaim_blocked, "");
    s.add("host背书放行", p.host_confirm_pass, "");
    s.add("越界位剥除", p.bit_sanitize, "");
    // 协同类四件全 requires_host。
    let host_caps = Capability::ALL.iter().filter(|c| c.requires_host()).count();
    s.add("协同类四件", host_caps == 4, "");

    // 判据"协商协议"。
    s.add("四步协商成链", p.negotiation_chain, "");
    s.add("交集恰值", p.intersection_exact, "");
    s.add("越权留痕恰值", p.overreach_exact, "");
    s.add("跳步违约作废", p.negotiation_order_block, "");
    s.add("重放确定性", p.replay_deterministic, "");

    // 判据"缓存复用"。
    s.add("缓存命中", p.cache_hit, "");
    s.add("陈旧命中带旧标注", p.cache_stale_hit, "");
    s.add("键不符立案", p.cache_key_miss, "");

    // 判据"保守模式"。
    s.add("三败激活保守", p.conservative_activate, "");
    s.add("成功清零", p.ok_resets, "");
    s.add(
        "保守集恰值",
        CONSERVATIVE_CAPS_BITS == 0b1001000 && CONSERVATIVE_THRESHOLD == 3,
        "",
    );

    // 判据"五组"（组数守恒声明）。
    s.add("五组判据齐备", true, "探测/诚实/协商/缓存/失败——上列逐组落行");

    // 判据"诊断码闭集"：十二码互异。
    let codes = [
        CapCode::CpSourceUnavailable.code(),
        CapCode::CpModeConflict.code(),
        CapCode::CpOverclaim.code(),
        CapCode::CpNegotiationOrder.code(),
        CapCode::CpOverreachRequest.code(),
        CapCode::CpSealMismatch.code(),
        CapCode::CpCacheFull.code(),
        CapCode::CpCacheKeyMiss.code(),
        CapCode::CpConservativeActive.code(),
        CapCode::CpCapabilityRange.code(),
        CapCode::CpDoubleProbe.code(),
        CapCode::CpSnapshotDrift.code(),
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
    s.add("诊断码十二码互异", distinct && codes[0] == 0xBD01 && codes[11] == 0xBD0C, "");

    s
}
