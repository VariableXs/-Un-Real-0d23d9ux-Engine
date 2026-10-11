//! unx081_kaslr_entropy — UNX-F0081 KASLR 熵源采集器（深化版）。
//!
//! 判据原文（CoRun Unxreal 主册 UNX-F0081 · UNX-A1-B04）：RDRAND/TSC/
//! 内存图自由度三源混合熵估计落账，熵低于阈值时降级固定基址并告警。
//!
//! **分层深化声明**：采集接口定义 + 判读逻辑纯函数化——真实硬件读数
//! （RDRAND 指令/TSC 寄存器/内存图扫描）由引导主链注入，本册零 IO
//! 零特权指令（no_std 纪律），只做**熵的语义学**：三源质量分级、混合
//! 估计的保守口径、低于阈值降级决策、告警落账。随机性要真，判读要纯
//! ——两层分开，注入面可测，判读面可证。
//!
//! **三源语义**：
//! 1. **RDRAND**（硬件 CSPRNG）：质量最高——CPUID 可用位 + 读数成功
//!    计数 + 重试上限（RDRAND 可能连续失败，重试穷尽即该源缺席显性
//!    记账不静默装作有熵）；
//! 2. **TSC 低位**：时戳计数器低位抖动是熵、高位是可预测的钟——取低
//!    N 位（TSC_LOW_BITS=12），读数即注入；
//! 3. **内存图自由度**：可用基址数 = 空洞总容量 / 对齐粒度——log2 估
//!    熵位（手写 const 半步 log2 复用 cga04 语义——此处独立实现零跨域
//!    依赖）；空洞越多基址自由度越大。
//!
//! **混合估计保守口径**：三源不独立时线性相加会高估——混合熵 = Σ
//! (min(源熵位, 源上限) × 源权重) / Σ权重（权重表万分制守恒——与
//! cgo01 复用五家族同款纪律），再打独立折扣（三源齐备 100%、缺 RDRAND
//! 80%、只 TSC 60%——折扣表闭集显性不拍脑袋）。
//!
//! **降级决策**：混合熵 < KASLR_BITS_MIN（12 位——4096 槽下限，低于
//! 此随机化形同虚设）→ 降级固定基址（槽 0）+ 告警项入账（Severity
//! Warn——不阻塞引导，与 UNX-F0041 不阻塞语义同源）；熵充足 → 种子
//! 确定性哈希选槽（FNV-1a 种子扩散——同种子同槽可复现可审计）。
//!
//! 诊断枚举 KaslrCode 十六码（0x30 段 0x3000..=0x300F 互异 +
//! [`segment_guard`] 对账）。no_std 零 unwrap 零 panic 面。

use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const UNX081_KASLR_VERSION: &str = "unx081-kaslr-v1";

/// 诊断段基值（0x30 段——UNX 域 0x2E 速写/0x2F 深化已占，此占 0x30）。
pub const SEG_BASE: u16 = 0x3000;

/// KASLR 最低熵阈值（位——4096 槽下限，低于此随机化形同虚设）。
pub const KASLR_BITS_MIN: u64 = 12;

/// TSC 低位掩码位宽（低 12 位抖动是熵、高位是可预测的钟）。
pub const TSC_LOW_BITS: u32 = 12;

/// RDRAND 重试上限（连续失败穷尽即源缺席显性记账）。
pub const RDRAND_MAX_RETRIES: u32 = 8;

/// 对齐粒度（基址槽距——4 MiB，页表大粒度）。
pub const SLOT_GRANULARITY: u64 = 4 * 1024 * 1024;

/// 基址槽上限（策略值——1024 槽覆盖 4 GiB 空间）。
pub const MAX_SLOTS: u64 = 1024;

// ---------------------------------------------------------------------------
// 诊断码（KaslrCode 十六码 · 0x30 段互异）
// ---------------------------------------------------------------------------

/// 诊断码（KASLR 熵源——0x30 段十六码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KaslrCode {
    /// 0x3000 三源全缺席（无任何可用熵源——必须降级并告警）。
    AllSourcesAbsent,
    /// 0x3001 RDRAND 可用位未置（CPUID 报告不可用——源缺席）。
    RdrandUnavailable,
    /// 0x3002 RDRAND 重试穷尽（连续失败超上限——本次采集缺席）。
    RdrandExhausted,
    /// 0x3003 TSC 读数为零（零不是熵——采集链路疑故障）。
    TscZero,
    /// 0x3004 内存图空（无空洞无自由度——源缺席）。
    MemMapEmpty,
    /// 0x3005 内存图自由度为零（空洞容量 < 对齐粒度——无处可挪）。
    NoFreedom,
    /// 0x3006 权重表守恒破坏（三源权重和 ≠ 10000——估计前提崩塌）。
    WeightSumBroken,
    /// 0x3007 熵上限越界（源熵位 > 64——估计面事故）。
    BitsOverflow,
    /// 0x3008 混合熵低于阈值（触发降级——这不是错误是决策输入）。
    BelowThreshold,
    /// 0x3009 槽位越界（选槽 ≥ 槽总数或槽总数 > MAX_SLOTS）。
    SlotOutOfRange,
    /// 0x300A 告警账满（告警项超容量——旧账保留）。
    AlertFull,
    /// 0x300B 种子为零（零种子使槽选择退化——显性记账）。
    SeedZero,
    /// 0x300C 折扣表非法（独立折扣不在闭集——估计面污染）。
    DiscountInvalid,
    /// 0x300D 熵账守恒破坏（采集总数 ≠ 成功 + 缺席 + 拒收和）。
    LedgerBroken,
    /// 0x300E 降级基址为空（降级时无固定基址可退——配置事故）。
    FallbackMissing,
    /// 0x300F 段守恒对账失败（十六码校验不过——段位事故）。
    SegmentGuard,
}

impl KaslrCode {
    /// 十六码闭集。
    pub const ALL: [KaslrCode; 16] = [
        KaslrCode::AllSourcesAbsent,
        KaslrCode::RdrandUnavailable,
        KaslrCode::RdrandExhausted,
        KaslrCode::TscZero,
        KaslrCode::MemMapEmpty,
        KaslrCode::NoFreedom,
        KaslrCode::WeightSumBroken,
        KaslrCode::BitsOverflow,
        KaslrCode::BelowThreshold,
        KaslrCode::SlotOutOfRange,
        KaslrCode::AlertFull,
        KaslrCode::SeedZero,
        KaslrCode::DiscountInvalid,
        KaslrCode::LedgerBroken,
        KaslrCode::FallbackMissing,
        KaslrCode::SegmentGuard,
    ];

    /// 段内码值（连续——guard 对账面）。
    pub const fn code(self) -> u16 {
        match self {
            KaslrCode::AllSourcesAbsent => 0x3000,
            KaslrCode::RdrandUnavailable => 0x3001,
            KaslrCode::RdrandExhausted => 0x3002,
            KaslrCode::TscZero => 0x3003,
            KaslrCode::MemMapEmpty => 0x3004,
            KaslrCode::NoFreedom => 0x3005,
            KaslrCode::WeightSumBroken => 0x3006,
            KaslrCode::BitsOverflow => 0x3007,
            KaslrCode::BelowThreshold => 0x3008,
            KaslrCode::SlotOutOfRange => 0x3009,
            KaslrCode::AlertFull => 0x300A,
            KaslrCode::SeedZero => 0x300B,
            KaslrCode::DiscountInvalid => 0x300C,
            KaslrCode::LedgerBroken => 0x300D,
            KaslrCode::FallbackMissing => 0x300E,
            KaslrCode::SegmentGuard => 0x300F,
        }
    }

    /// 人读诊断（互异非空）。
    pub const fn detail(self) -> &'static str {
        match self {
            KaslrCode::AllSourcesAbsent => {
                "三源全缺席：无任何可用熵源——必须降级固定基址并告警，建议核查 CPUID 与内存图"
            }
            KaslrCode::RdrandUnavailable => {
                "RDRAND 可用位未置：CPUID 报告硬件 CSPRNG 不可用，建议双源混合补熵"
            }
            KaslrCode::RdrandExhausted => {
                "RDRAND 重试穷尽：连续失败超上限，本次采集缺席记账，建议下次重扫"
            }
            KaslrCode::TscZero => {
                "TSC 读数为零：零不是熵，采集链路疑故障，建议核查时戳计数器使能"
            }
            KaslrCode::MemMapEmpty => {
                "内存图空：无空洞无自由度，建议核查引导期内存图注入面"
            }
            KaslrCode::NoFreedom => {
                "自由度为零：空洞容量不足一个对齐粒度——无处可挪，建议缩小粒度评审"
            }
            KaslrCode::WeightSumBroken => {
                "权重表守恒破坏：三源权重和须恒 10000，建议校准权重表"
            }
            KaslrCode::BitsOverflow => {
                "源熵位越界：单源熵位不得超过 64——估计面事故，建议钳制后重估"
            }
            KaslrCode::BelowThreshold => {
                "混合熵低于阈值：随机化形同虚设——降级固定基址并告警，不阻塞引导"
            }
            KaslrCode::SlotOutOfRange => {
                "槽位越界：选槽须在槽总数内且槽总数不超上限，建议核查种子与槽表"
            }
            KaslrCode::AlertFull => {
                "告警账满：旧账保留新告警拒入，建议归档旧账"
            }
            KaslrCode::SeedZero => {
                "种子为零：零种子使槽选择退化固定，建议核查混合器输出"
            }
            KaslrCode::DiscountInvalid => {
                "折扣表非法：独立折扣不在闭集，建议回归三档闭集"
            }
            KaslrCode::LedgerBroken => {
                "熵账守恒破坏：采集总数应等于成功加缺席加拒收，建议审计采集流水"
            }
            KaslrCode::FallbackMissing => {
                "降级基址缺失：降级时无固定基址可退是配置事故，建议登记槽 0 基址"
            }
            KaslrCode::SegmentGuard => {
                "段守恒对账失败：十六码须连续互异界内——段位事故立即立案"
            }
        }
    }
}

/// **段守恒对账**（连续即互异——code() u16 const 判等）。
pub const fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < KaslrCode::ALL.len() {
        let c = KaslrCode::ALL[i].code();
        if c < SEG_BASE || c != SEG_BASE + i as u16 {
            return false;
        }
        let mut j = i + 1;
        while j < KaslrCode::ALL.len() {
            if KaslrCode::ALL[i].code() == KaslrCode::ALL[j].code() {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 三源闭集 + 权重表（守恒纪律）
// ---------------------------------------------------------------------------

/// 熵源三闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EntropySource {
    /// 硬件 CSPRNG（质量最高）。
    Rdrand,
    /// 时戳计数器低位。
    TscLow,
    /// 内存图基址自由度。
    MemMap,
}

impl EntropySource {
    /// 三闭集。
    pub const ALL: [EntropySource; 3] = [
        EntropySource::Rdrand,
        EntropySource::TscLow,
        EntropySource::MemMap,
    ];

    /// 中文名（读屏面——互异）。
    pub const fn zh(self) -> &'static str {
        match self {
            EntropySource::Rdrand => "硬件随机",
            EntropySource::TscLow => "时戳低位",
            EntropySource::MemMap => "内存自由度",
        }
    }

    /// 单源熵位上限（估计钳制面——RDRAND 理论 64、TSC 低位 12、内存图
    /// 由槽位数决定上限 10（MAX_SLOTS=1024 → log2=10））。
    pub const fn cap_bits(self) -> u64 {
        match self {
            EntropySource::Rdrand => 64,
            EntropySource::TscLow => TSC_LOW_BITS as u64,
            EntropySource::MemMap => 10,
        }
    }
}

/// 权重预设闭集（万分制和恒 10000——RDRAND 主导/TSC 补充/内存图兜底）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WeightPreset {
    /// 均衡档（三源等权）。
    Balanced,
    /// 硬件优先（RDRAND 6000）。
    HwFirst,
    /// 无硬件档（RDRAND 缺席时——TSC 与内存图分摊）。
    NoHw,
}

impl WeightPreset {
    /// 三档闭集。
    pub const ALL: [WeightPreset; 3] = [
        WeightPreset::Balanced,
        WeightPreset::HwFirst,
        WeightPreset::NoHw,
    ];

    /// 中文名（互异）。
    pub const fn zh(self) -> &'static str {
        match self {
            WeightPreset::Balanced => "均衡",
            WeightPreset::HwFirst => "硬件优先",
            WeightPreset::NoHw => "无硬件",
        }
    }

    /// 三源权重（按 EntropySource::ALL 序——万分制）。
    pub const fn weights(self) -> [u64; 3] {
        match self {
            WeightPreset::Balanced => [4_000, 3_000, 3_000],
            WeightPreset::HwFirst => [6_000, 2_500, 1_500],
            WeightPreset::NoHw => [0, 5_500, 4_500],
        }
    }
}

/// 权重表（守恒校验单点）。
#[derive(Clone, Copy, Debug)]
pub struct WeightSet {
    /// [RDRAND, TSC, MemMap] 万分制。
    pub per_source: [u64; 3],
}

impl WeightSet {
    /// 预设构造。
    pub const fn preset(p: WeightPreset) -> WeightSet {
        WeightSet { per_source: p.weights() }
    }

    /// 直构。
    pub const fn from_parts(per_source: [u64; 3]) -> WeightSet {
        WeightSet { per_source }
    }

    /// 守恒校验（和 ≠ 10000 即 WeightSumBroken）。
    pub const fn validate(&self) -> Option<KaslrCode> {
        let mut sum: u64 = 0;
        let mut i = 0usize;
        while i < 3 {
            sum += self.per_source[i];
            i += 1;
        }
        if sum == 10_000 {
            None
        } else {
            Some(KaslrCode::WeightSumBroken)
        }
    }

    /// 源权重。
    pub const fn weight_of(&self, s: EntropySource) -> u64 {
        match s {
            EntropySource::Rdrand => self.per_source[0],
            EntropySource::TscLow => self.per_source[1],
            EntropySource::MemMap => self.per_source[2],
        }
    }

    /// 三档全守恒（静态面）。
    pub const fn presets_conserved() -> bool {
        let mut i = 0usize;
        while i < WeightPreset::ALL.len() {
            if WeightSet::preset(WeightPreset::ALL[i]).validate().is_some() {
                return false;
            }
            i += 1;
        }
        true
    }
}

// ---------------------------------------------------------------------------
// 源读数注入面（上游采集——本册只判读）
// ---------------------------------------------------------------------------

/// RDRAND 采集结果（可用性 + 读数 + 重试消耗）。
#[derive(Clone, Copy, Debug)]
pub struct RdrandSample {
    /// CPUID 可用位（上游注入）。
    pub available: bool,
    /// 读数（None=本次采集失败——重试穷尽）。
    pub value: Option<u64>,
    /// 本次消耗的重试次数（≤ RDRAND_MAX_RETRIES）。
    pub retries_used: u32,
}

/// TSC 采集结果。
#[derive(Clone, Copy, Debug)]
pub struct TscSample {
    /// 全宽读数（上游注入——本册取低 TSC_LOW_BITS 位）。
    pub raw: u64,
}

/// 内存图自由度采集结果。
#[derive(Clone, Copy, Debug)]
pub struct MemMapSample {
    /// 空洞总容量（字节）。
    pub hole_bytes: u64,
    /// 空洞数。
    pub holes: u32,
}

impl MemMapSample {
    /// 自由槽位数 = 空洞容量 / 对齐粒度（钳 MAX_SLOTS）。
    pub const fn free_slots(&self) -> u64 {
        if SLOT_GRANULARITY == 0 {
            return 0;
        }
        let n = self.hole_bytes / SLOT_GRANULARITY;
        if n > MAX_SLOTS {
            MAX_SLOTS
        } else {
            n
        }
    }

    /// 自由度熵位 = log2(槽位数)（0 槽 0 位——log2_floor 手写）。
    pub const fn freedom_bits(&self) -> u64 {
        log2_floor(self.free_slots())
    }
}

/// log2 下取整（const 手写——零依赖）。
pub const fn log2_floor(v: u64) -> u64 {
    if v == 0 {
        return 0;
    }
    let mut x = v;
    let mut r: u64 = 0;
    while x > 1 {
        x >>= 1;
        r += 1;
    }
    r
}

// ---------------------------------------------------------------------------
// 混合估计（保守口径——折扣表闭集）
// ---------------------------------------------------------------------------

/// 独立折扣三档闭集（万分制——三源齐备 100%/缺 RDRAND 80%/只 TSC 60%）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IndependenceDiscount {
    /// 三源齐备（10000）。
    AllThree,
    /// 缺 RDRAND（8000）。
    NoRdrand,
    /// 仅 TSC+内存图之外单源（6000）。
    SingleSource,
}

impl IndependenceDiscount {
    /// 三档闭集。
    pub const ALL: [IndependenceDiscount; 3] = [
        IndependenceDiscount::AllThree,
        IndependenceDiscount::NoRdrand,
        IndependenceDiscount::SingleSource,
    ];

    /// 万分制值。
    pub const fn permille(self) -> u64 {
        match self {
            IndependenceDiscount::AllThree => 10_000,
            IndependenceDiscount::NoRdrand => 8_000,
            IndependenceDiscount::SingleSource => 6_000,
        }
    }

    /// 按可用源集合选档（闭集判定单点——RdrandAbsent 无 TscZero 语义：
    /// TSC 读零是故障不是缺席，缺席按 available 标志）。
    pub const fn select(rdrand_ok: bool, tsc_ok: bool, mem_ok: bool) -> IndependenceDiscount {
        let n = (rdrand_ok as u64) + (tsc_ok as u64) + (mem_ok as u64);
        if n >= 3 {
            IndependenceDiscount::AllThree
        } else if rdrand_ok {
            // 有 RDRAND 但缺别的——两源组合。
            IndependenceDiscount::NoRdrand
        } else if n == 2 {
            IndependenceDiscount::NoRdrand
        } else {
            IndependenceDiscount::SingleSource
        }
    }
}

/// 混合估计输入（三源样本 + 权重表）。
pub struct MixInput<'a> {
    /// RDRAND 样本（None=源缺席）。
    pub rdrand: Option<&'a RdrandSample>,
    /// TSC 样本（None=源缺席）。
    pub tsc: Option<&'a TscSample>,
    /// 内存图样本（None=源缺席）。
    pub memmap: Option<&'a MemMapSample>,
    /// 权重表。
    pub weights: &'a WeightSet,
}

/// 混合估计结果（各源贡献 + 折扣 + 总熵——全链可见）。
#[derive(Clone, Copy, Debug)]
pub struct MixEstimate {
    /// RDRAND 贡献熵位（缺席 0）。
    pub bits_rdrand: u64,
    /// TSC 贡献熵位。
    pub bits_tsc: u64,
    /// 内存图贡献熵位。
    pub bits_memmap: u64,
    /// 独立折扣档。
    pub discount: IndependenceDiscount,
    /// 混合总熵位（0..=64 钳）。
    pub total_bits: u64,
    /// 是否低于 KASLR_BITS_MIN（降级决策输入）。
    pub below_min: bool,
}

/// **混合熵估计**（保守口径单点：Σ min(源熵位, 源上限) × 权重 / Σ权重
/// × 折扣 / 10000，钳 [0, 64]）。
pub fn mix_estimate(inp: &MixInput) -> Result<MixEstimate, KaslrCode> {
    if let Some(c) = inp.weights.validate() {
        return Err(c);
    }
    // 各源熵位（钳上限——RDRAND 取读数非零即 64 位满额语义简化为 cap，
    // TSC 取低位宽，内存图取自由度 log2）。
    let (b_r, r_ok) = match inp.rdrand {
        Some(s) => {
            if !s.available {
                (0u64, false)
            } else if s.value.is_none() {
                (0u64, false) // 重试穷尽——缺席
            } else {
                (EntropySource::Rdrand.cap_bits(), true)
            }
        }
        None => (0, false),
    };
    let (b_t, t_ok) = match inp.tsc {
        Some(s) => {
            if s.raw == 0 {
                (0, false) // 零读数——故障非缺席，熵贡献 0
            } else {
                (EntropySource::TscLow.cap_bits(), true)
            }
        }
        None => (0, false),
    };
    let (b_m, m_ok) = match inp.memmap {
        Some(s) => {
            let fb = s.freedom_bits();
            if fb == 0 {
                (0, false)
            } else {
                (fb.min(EntropySource::MemMap.cap_bits()), true)
            }
        }
        None => (0, false),
    };
    if b_r > 64 || b_t > 64 || b_m > 64 {
        return Err(KaslrCode::BitsOverflow);
    }
    if !r_ok && !t_ok && !m_ok {
        return Err(KaslrCode::AllSourcesAbsent);
    }
    let disc = IndependenceDiscount::select(r_ok, t_ok, m_ok);
    // 加权混合（分母只算可用源权重——没参与的源不送分）。
    let mut num: u64 = 0;
    let mut den: u64 = 0;
    if r_ok {
        num += b_r * inp.weights.weight_of(EntropySource::Rdrand);
        den += inp.weights.weight_of(EntropySource::Rdrand);
    }
    if t_ok {
        num += b_t * inp.weights.weight_of(EntropySource::TscLow);
        den += inp.weights.weight_of(EntropySource::TscLow);
    }
    if m_ok {
        num += b_m * inp.weights.weight_of(EntropySource::MemMap);
        den += inp.weights.weight_of(EntropySource::MemMap);
    }
    if den == 0 {
        // 可用源权重全 0（NoHw 档遇上 RDRAND 在场）——按满权均值退化。
        den = 1;
    }
    let weighted = num / den;
    let discounted = weighted * disc.permille() / 10_000;
    let total = if discounted > 64 { 64 } else { discounted };
    Ok(MixEstimate {
        bits_rdrand: b_r,
        bits_tsc: b_t,
        bits_memmap: b_m,
        discount: disc,
        total_bits: total,
        below_min: total < KASLR_BITS_MIN,
    })
}

// ---------------------------------------------------------------------------
// 基址槽选择（种子确定性哈希 + 降级固定基址）
// ---------------------------------------------------------------------------

/// FNV-1a 64（种子扩散——手写 const 零依赖）。
pub const fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0usize;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    h
}

/// 种子打包（三源读数小端串接——同读数同种子可复现可审计）。
pub fn seed_pack(rdrand: Option<u64>, tsc: Option<u64>, mem: Option<u64>) -> [u8; 24] {
    let mut out = [0u8; 24];
    let mut off = 0usize;
    let chunks: [Option<u64>; 3] = [rdrand, tsc, mem];
    let mut i = 0usize;
    while i < 3 {
        if let Some(v) = chunks[i] {
            out[off..off + 8].copy_from_slice(&v.to_le_bytes());
        }
        off += 8;
        i += 1;
    }
    out
}

/// 槽选择决策（降级 vs 哈希选槽——判据面单点）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SlotDecision {
    /// 降级固定基址（槽 0）。
    FallbackSlot0,
    /// 熵充足——哈希选槽。
    SeededSlot(u64),
}

/// **槽选择**（熵足 → FNV(种子) mod 槽数（非 0——槽 0 留给降级，避免
/// 随机化偶尔撞上降级位址混淆审计）；熵不足 → 槽 0 + 告警）。
pub fn pick_slot(est: &MixEstimate, seed: &[u8], total_slots: u64) -> Result<SlotDecision, KaslrCode> {
    if total_slots == 0 || total_slots > MAX_SLOTS {
        return Err(KaslrCode::SlotOutOfRange);
    }
    if est.below_min {
        return Ok(SlotDecision::FallbackSlot0);
    }
    if seed.iter().all(|b| *b == 0) {
        return Err(KaslrCode::SeedZero);
    }
    let h = fnv1a64(seed);
    // 槽 0 保留给降级——随机选槽从 1 起。
    let usable = total_slots - 1;
    if usable == 0 {
        return Ok(SlotDecision::FallbackSlot0);
    }
    Ok(SlotDecision::SeededSlot(1 + h % usable))
}

/// 槽→基址换算（槽 n = SLOT_GRANULARITY × n——0 槽即固定基址 0 偏移）。
pub const fn slot_base(slot: u64) -> u64 {
    slot * SLOT_GRANULARITY
}

// ---------------------------------------------------------------------------
// 告警账 + 熵账本（落账面——判据"落账"的本体）
// ---------------------------------------------------------------------------

/// 告警账容量。
pub const ALERT_CAPACITY: usize = 32;

/// 告警账（append-only——低于阈值告警/源缺席告警逐码入账）。
pub struct AlertLog {
    /// 告警行（码值|人读）。
    pub rows: Vec<String>,
    /// 逐码计数（十六槽）。
    pub per_code: [u32; 16],
    /// 拒入计数（账满）。
    pub rejected_full: u32,
}

impl AlertLog {
    /// 空账。
    pub fn new() -> AlertLog {
        AlertLog {
            rows: Vec::new(),
            per_code: [0; 16],
            rejected_full: 0,
        }
    }

    /// 追加告警（码 → 行；账满拒入旧账保留）。
    pub fn push(&mut self, c: KaslrCode, note: &str) {
        if self.rows.len() >= ALERT_CAPACITY {
            self.rejected_full += 1;
            return;
        }
        let idx = (c.code() - SEG_BASE) as usize;
        self.per_code[idx] += 1;
        let mut line = String::from(note);
        line.push_str(" | ");
        line.push_str(c.detail());
        self.rows.push(alloc::format!("{:#06x}|{}", c.code(), line));
    }

    /// 在账数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 空判。
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// 一行摘要（读屏面）。
    pub fn summary(&self) -> String {
        alloc::format!(
            "U081 KASLR 告警账: 条目={} 拒入={} 段守恒={}",
            self.len(),
            self.rejected_full,
            if segment_guard() { "真" } else { "假" }
        )
    }
}

impl Default for AlertLog {
    fn default() -> Self {
        AlertLog::new()
    }
}

/// 熵账本（逐源采集计数——守恒式见 conserved）。
#[derive(Clone, Copy, Debug, Default)]
pub struct EntropyLedger {
    /// 采集总次（进账面）。
    pub seen: u64,
    /// 成功次（至少一源有产出）。
    pub produced: u64,
    /// 缺席次（全源无产出——AllSourcesAbsent 或空输入）。
    pub absent: u64,
    /// 降级次（低于阈值走槽 0）。
    pub fallbacks: u64,
    /// 哈希选槽次。
    pub seeded: u64,
    /// 拒收次（权重破/槽越界/种子零）。
    pub rejected: u64,
}

impl EntropyLedger {
    /// 守恒（seen == produced + absent + rejected——降级与选槽是 produced
    /// 的后续分流不计入守恒式）。
    pub const fn conserved(&self) -> bool {
        self.seen == self.produced + self.absent + self.rejected
    }
}

// ---------------------------------------------------------------------------
// 主编排：采集判读全链（三源注入 → 混合估计 → 降级决策 → 落账）
// ---------------------------------------------------------------------------

/// 全链结果（估计 + 决策 + 基址 + 告警账 + 账本增量——编排面单点）。
pub struct KaslrOutcome {
    /// 混合估计（成功时 Some）。
    pub estimate: Option<MixEstimate>,
    /// 槽决策。
    pub decision: Option<SlotDecision>,
    /// 基址（字节）。
    pub base: Option<u64>,
    /// 告警账（本轮增量）。
    pub alerts: AlertLog,
    /// 账本增量（调用侧并入总账）。
    pub ledger_delta: EntropyLedger,
}

/// **全链判读**（三源样本注入 → mix_estimate → pick_slot → base 换算
/// → 告警落账——零 IO，判读纯函数化）。
pub fn walk_entropy(
    rdrand: Option<&RdrandSample>,
    tsc: Option<&TscSample>,
    memmap: Option<&MemMapSample>,
    weights: &WeightSet,
) -> KaslrOutcome {
    let mut alerts = AlertLog::new();
    let mut ledger = EntropyLedger::default();
    ledger.seen = 1;
    // 源缺席显性记账（不静默装作有熵）。
    let mut inp = MixInput {
        rdrand: None,
        tsc: None,
        memmap: None,
        weights,
    };
    if let Some(s) = rdrand {
        if !s.available {
            alerts.push(KaslrCode::RdrandUnavailable, "RDRAND 缺席");
        } else if s.value.is_none() {
            alerts.push(KaslrCode::RdrandExhausted, "RDRAND 重试穷尽");
        } else if s.retries_used > RDRAND_MAX_RETRIES {
            alerts.push(KaslrCode::RdrandExhausted, "重试计数越界");
        } else {
            inp.rdrand = Some(s);
        }
    }
    if let Some(s) = tsc {
        if s.raw == 0 {
            alerts.push(KaslrCode::TscZero, "TSC 读零");
        } else {
            inp.tsc = Some(s);
        }
    }
    if let Some(s) = memmap {
        if s.holes == 0 || s.hole_bytes == 0 {
            alerts.push(KaslrCode::MemMapEmpty, "内存图空");
        } else if s.freedom_bits() == 0 {
            alerts.push(KaslrCode::NoFreedom, "自由度不足一粒度");
        } else {
            inp.memmap = Some(s);
        }
    }
    match mix_estimate(&inp) {
        Ok(est) => {
            if est.below_min {
                alerts.push(KaslrCode::BelowThreshold, "熵不足降级");
                ledger.fallbacks += 1;
                ledger.produced += 1;
                KaslrOutcome {
                    estimate: Some(est),
                    decision: Some(SlotDecision::FallbackSlot0),
                    base: Some(slot_base(0)),
                    alerts,
                    ledger_delta: ledger,
                }
            } else {
                // 种子打包（可用源读数——RDRAND 读数/TSC 低位/自由槽位）。
                let r_val = inp.rdrand.and_then(|s| s.value);
                let t_val = inp.tsc.map(|s| s.raw & ((1u64 << TSC_LOW_BITS) - 1));
                let m_val = inp.memmap.map(|s| s.free_slots());
                let seed = seed_pack(r_val, t_val, m_val);
                let slots = match memmap {
                    Some(s) => s.free_slots().max(2), // 至少 2 槽（0 降级 + 1 随机）
                    None => 2,
                };
                match pick_slot(&est, &seed, slots) {
                    Ok(d) => {
                        ledger.seeded += 1;
                        ledger.produced += 1;
                        let base = match d {
                            SlotDecision::FallbackSlot0 => slot_base(0),
                            SlotDecision::SeededSlot(n) => slot_base(n),
                        };
                        KaslrOutcome {
                            estimate: Some(est),
                            decision: Some(d),
                            base: Some(base),
                            alerts,
                            ledger_delta: ledger,
                        }
                    }
                    Err(c) => {
                        alerts.push(c, "槽选择失败");
                        ledger.rejected += 1;
                        KaslrOutcome {
                            estimate: Some(est),
                            decision: None,
                            base: None,
                            alerts,
                            ledger_delta: ledger,
                        }
                    }
                }
            }
        }
        Err(c) => {
            if c == KaslrCode::AllSourcesAbsent {
                ledger.absent += 1;
            } else {
                ledger.rejected += 1;
            }
            alerts.push(c, "混合估计失败");
            // 全缺席必须降级——固定基址兜底 + FallbackMissing 检查（槽 0
            // 恒可用，此处不会缺——缺的是配置面校验：槽表空才 Missing）。
            KaslrOutcome {
                estimate: None,
                decision: Some(SlotDecision::FallbackSlot0),
                base: Some(slot_base(0)),
                alerts,
                ledger_delta: ledger,
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 快照（告警账 '|' 序列化 + FNV + 往返）
// ---------------------------------------------------------------------------

/// 告警账快照。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KaslrSnap {
    /// 版本戳。
    pub version: String,
    /// 告警行。
    pub rows: Vec<String>,
    /// 逐码计数。
    pub per_code: [u32; 16],
    /// 拒入数。
    pub rejected_full: u32,
}

impl KaslrSnap {
    /// 定格。
    pub fn capture(a: &AlertLog) -> KaslrSnap {
        // 快照行换基纪律：行内 '|' → ';'——'|' 是快照串分隔符，告警行
        // （"码|注 | 详情"）残留分隔符会让 parse 段数错位。
        let mut rows = Vec::new();
        let mut i = 0usize;
        while i < a.rows.len() {
            rows.push(a.rows[i].replace('|', ";"));
            i += 1;
        }
        KaslrSnap {
            version: String::from(UNX081_KASLR_VERSION),
            rows,
            per_code: a.per_code,
            rejected_full: a.rejected_full,
        }
    }

    /// 序列化。
    pub fn serialize(&self) -> String {
        let mut s = alloc::format!(
            "{}|{}|{}|{}",
            self.version,
            self.rows.len(),
            self.rejected_full,
            dist_str(&self.per_code)
        );
        let mut i = 0usize;
        while i < self.rows.len() {
            s.push('|');
            s.push_str(&self.rows[i]);
            i += 1;
        }
        s
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        fnv1a64(self.serialize().as_bytes())
    }

    /// 往返恒等。
    pub fn roundtrip(&self) -> Result<(), KaslrCode> {
        match parse_snap(&self.serialize()) {
            Some(p) => {
                if p == *self && p.fingerprint() == self.fingerprint() {
                    Ok(())
                } else {
                    Err(KaslrCode::SegmentGuard)
                }
            }
            None => Err(KaslrCode::AlertFull),
        }
    }
}

/// 十六槽分布串。
fn dist_str(d: &[u32; 16]) -> String {
    let mut s = String::new();
    let mut i = 0usize;
    while i < 16 {
        if i > 0 {
            s.push(',');
        }
        s.push_str(&alloc::format!("{}", d[i]));
        i += 1;
    }
    s
}

/// 十六槽分布解析。
fn parse_dist(s: &str) -> Option<[u32; 16]> {
    let parts: Vec<&str> = s.split(',').collect();
    if parts.len() != 16 {
        return None;
    }
    let mut out = [0u32; 16];
    let mut i = 0usize;
    while i < 16 {
        let v = parse_u64(parts[i])?;
        if v > u32::MAX as u64 {
            return None;
        }
        out[i] = v as u32;
        i += 1;
    }
    Some(out)
}

/// 十进制解析。
fn parse_u64(s: &str) -> Option<u64> {
    if s.is_empty() {
        return None;
    }
    let mut v: u64 = 0;
    let b = s.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        let d = (b[i] as u64).wrapping_sub(b'0' as u64);
        if d > 9 {
            return None;
        }
        v = v.wrapping_mul(10).wrapping_add(d);
        i += 1;
    }
    Some(v)
}

/// 快照反序列化。
pub fn parse_snap(s: &str) -> Option<KaslrSnap> {
    let parts: Vec<&str> = s.split('|').collect();
    if parts.len() < 4 {
        return None;
    }
    let version = String::from(parts[0]);
    let rows_len = parse_u64(parts[1])? as usize;
    let rejected_full = parse_u64(parts[2])? as u32;
    let per_code = parse_dist(parts[3])?;
    if parts.len() != 4 + rows_len {
        return None;
    }
    let mut rows = Vec::new();
    let mut i = 0usize;
    while i < rows_len {
        rows.push(String::from(parts[4 + i]));
        i += 1;
    }
    Some(KaslrSnap {
        version,
        rows,
        per_code,
        rejected_full,
    })
}

// ---------------------------------------------------------------------------
// 多口径参考探针（四口径）
// ---------------------------------------------------------------------------

/// 口径一：混合估计手算——三源齐备 Balanced 档：RDRAND 64×4000 + TSC
/// 12×3000 + 内存 log2(32)=5×3000 = 256000+36000+15000 = 307000 /
/// 10000 = 30.7 → 30 × 10000/10000 折扣 = 30 位 ≥ 12 不降级；仅 TSC
/// （SingleSource 6000）：12×3000/3000=12 ×6000/10000=7.2→7 < 12 降级。
fn probe_mix_math() -> bool {
    let w = WeightSet::preset(WeightPreset::Balanced);
    let rd = RdrandSample { available: true, value: Some(42), retries_used: 0 };
    let ts = TscSample { raw: 0xA5A5_1234 };
    let mm = MemMapSample { hole_bytes: 32 * SLOT_GRANULARITY, holes: 4 };
    let est = mix_estimate(&MixInput { rdrand: Some(&rd), tsc: Some(&ts), memmap: Some(&mm), weights: &w });
    let ok_full = match est {
        Ok(e) => e.total_bits == 30 && !e.below_min && e.discount == IndependenceDiscount::AllThree,
        Err(_) => false,
    };
    let est2 = mix_estimate(&MixInput { rdrand: None, tsc: Some(&ts), memmap: None, weights: &w });
    let ok_single = match est2 {
        Ok(e) => e.total_bits == 7 && e.below_min && e.discount == IndependenceDiscount::SingleSource,
        Err(_) => false,
    };
    ok_full && ok_single
}

/// 口径二：降级决策——熵足哈希选槽同种子同槽可复现 + 非槽 0；熵不足
/// 恒槽 0；零种子显性拒。
pub fn probe_slot_decision() -> bool {
    let good = MixEstimate {
        bits_rdrand: 64, bits_tsc: 12, bits_memmap: 10,
        discount: IndependenceDiscount::AllThree, total_bits: 32, below_min: false,
    };
    let bad = MixEstimate {
        bits_rdrand: 0, bits_tsc: 12, bits_memmap: 0,
        discount: IndependenceDiscount::SingleSource, total_bits: 7, below_min: true,
    };
    let seed = seed_pack(Some(0xDEAD_BEEF), Some(0x1234), Some(16));
    let s1 = pick_slot(&good, &seed, 64);
    let s2 = pick_slot(&good, &seed, 64);
    let repro = s1 == s2
        && matches!(s1, Ok(SlotDecision::SeededSlot(n)) if n >= 1 && n < 64);
    let fb = pick_slot(&bad, &seed, 64) == Ok(SlotDecision::FallbackSlot0);
    let zero_seed = pick_slot(&good, &[0u8; 24], 64) == Err(KaslrCode::SeedZero);
    let oor = pick_slot(&good, &seed, 0) == Err(KaslrCode::SlotOutOfRange)
        && pick_slot(&good, &seed, MAX_SLOTS + 1) == Err(KaslrCode::SlotOutOfRange);
    repro && fb && zero_seed && oor
}

/// 口径三：账本守恒——混合流（成功/缺席/拒收）逐流对账。
pub fn probe_ledger() -> bool {
    let w = WeightSet::preset(WeightPreset::Balanced);
    let o1 = walk_entropy(
        Some(&RdrandSample { available: true, value: Some(7), retries_used: 0 }),
        Some(&TscSample { raw: 99 }),
        Some(&MemMapSample { hole_bytes: 8 * SLOT_GRANULARITY, holes: 2 }),
        &w,
    );
    // 全好流：produced+seeded。
    let ok1 = o1.ledger_delta.seen == 1
        && o1.ledger_delta.produced == 1
        && o1.ledger_delta.seeded == 1
        && o1.ledger_delta.conserved();
    // 全缺席流。
    let o2 = walk_entropy(None, None, None, &w);
    let ok2 = o2.ledger_delta.absent == 1 && o2.ledger_delta.conserved()
        && o2.decision == Some(SlotDecision::FallbackSlot0)
        && o2.base == Some(0);
    // TSC 零流（故障→TSC 缺席语义，RDRAND 在场仍 produced）。
    let o3 = walk_entropy(
        Some(&RdrandSample { available: true, value: Some(9), retries_used: 0 }),
        Some(&TscSample { raw: 0 }),
        None,
        &w,
    );
    let ok3 = o3.ledger_delta.produced == 1 && o3.ledger_delta.conserved()
        && !o3.alerts.is_empty();
    ok1 && ok2 && ok3
}

/// 口径四：段守恒 + 快照往返 + 权重三档守恒。
pub fn probe_snapshot_segment() -> bool {
    if !segment_guard() || !WeightSet::presets_conserved() {
        return false;
    }
    let mut a = AlertLog::new();
    a.push(KaslrCode::BelowThreshold, "降级告警");
    a.push(KaslrCode::TscZero, "TSC 零");
    let snap = KaslrSnap::capture(&a);
    snap.roundtrip().is_ok() && snap.rows.len() == 2
}

/// 汇总参考探针。
pub fn reference_budget_check() -> bool {
    probe_mix_math() && probe_slot_decision() && probe_ledger() && probe_snapshot_segment()
}

// ---------------------------------------------------------------------------
// 判据（CheckSet——判据面零 panic）
// ---------------------------------------------------------------------------

/// UNX-F0081 域自检（判据四组：三源×3、混合×3、降级×3、判据元×1）。
pub fn run_unx081_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0081");

    // 三-01：三源闭集 + 上限语义（RDRAND 64/TSC 12/内存 10）+ 中文名互异。
    let mut zh_ok = true;
    let mut i = 0usize;
    while i < EntropySource::ALL.len() {
        let mut j = i + 1;
        while j < EntropySource::ALL.len() {
            if EntropySource::ALL[i].zh() == EntropySource::ALL[j].zh() {
                zh_ok = false;
            }
            j += 1;
        }
        i += 1;
    }
    let cap_ok = EntropySource::Rdrand.cap_bits() == 64
        && EntropySource::TscLow.cap_bits() == TSC_LOW_BITS as u64
        && EntropySource::MemMap.cap_bits() == 10;
    let slot_ok = MemMapSample { hole_bytes: 4 * SLOT_GRANULARITY, holes: 1 }.free_slots() == 4
        && MemMapSample { hole_bytes: (MAX_SLOTS + 8) * SLOT_GRANULARITY, holes: 2 }.free_slots() == MAX_SLOTS
        && MemMapSample { hole_bytes: SLOT_GRANULARITY / 2, holes: 1 }.freedom_bits() == 0
        && MemMapSample { hole_bytes: 16 * SLOT_GRANULARITY, holes: 3 }.freedom_bits() == 4;
    if zh_ok && cap_ok && slot_ok {
        s.ok("U081-三-01-三源闭集与上限");
    } else {
        s.fail("U081-三-01-三源闭集与上限", "源面失效");
    }

    // 三-02：折扣三档选档恰值（三源/缺 RDRAND/单源）+ 权重三档守恒。
    let d3 = IndependenceDiscount::select(true, true, true);
    let d2 = IndependenceDiscount::select(false, true, true);
    let d1 = IndependenceDiscount::select(false, true, false);
    let disc_ok = d3 == IndependenceDiscount::AllThree
        && d2 == IndependenceDiscount::NoRdrand
        && d1 == IndependenceDiscount::SingleSource
        && IndependenceDiscount::AllThree.permille() == 10_000
        && IndependenceDiscount::NoRdrand.permille() == 8_000
        && IndependenceDiscount::SingleSource.permille() == 6_000
        && WeightSet::presets_conserved();
    if disc_ok {
        s.ok("U081-三-02-折扣三档与权重守恒");
    } else {
        s.fail("U081-三-02-折扣三档与权重守恒", "折扣或权重失效");
    }

    // 三-03：坏权重显性拒（和 ≠ 10000 → WeightSumBroken 不静默钳）。
    let bad = WeightSet::from_parts([5_000, 5_000, 5_000]);
    let bad_ok = bad.validate() == Some(KaslrCode::WeightSumBroken);
    let est_bad = matches!(
        mix_estimate(&MixInput {
            rdrand: None,
            tsc: Some(&TscSample { raw: 5 }),
            memmap: None,
            weights: &bad,
        }),
        Err(KaslrCode::WeightSumBroken)
    );
    if bad_ok && est_bad {
        s.ok("U081-三-03-坏权重显性拒");
    } else {
        s.fail("U081-三-03-坏权重显性拒", "权重闸失效");
    }

    // 混-01：混合手算恰值（32 位不降级 / 单源 7 位降级）。
    if probe_mix_math() {
        s.ok("U081-混-01-混合估计手算");
    } else {
        s.fail("U081-混-01-混合估计手算", "估计公式失效");
    }

    // 混-02：TSC 低位掩码——高位置 1 不影响低位取值（12 位掩码语义；
    // 0x1234 低 12 位 = 0x0234，判据侧独立手算）。
    let mask = (1u64 << TSC_LOW_BITS) - 1;
    let t_hi = TscSample { raw: 0xFFFF_FFFF_FFFF_1234 };
    let t_lo_val = t_hi.raw & mask;
    let mask_ok = t_lo_val == 0x0234 && mask == 0x0FFF;
    if mask_ok {
        s.ok("U081-混-02-TSC低位掩码");
    } else {
        s.fail("U081-混-02-TSC低位掩码", "掩码面失效");
    }

    // 混-03：源越界钳制——内存图自由度 > 上限钳 10 位（cap_bits 语义）。
    let big = MemMapSample { hole_bytes: (MAX_SLOTS + 100) * SLOT_GRANULARITY, holes: 9 };
    let w3 = WeightSet::preset(WeightPreset::Balanced);
    let est_big = mix_estimate(&MixInput { rdrand: None, tsc: None, memmap: Some(&big), weights: &w3 });
    let clamp_ok = match est_big {
        Ok(e) => e.bits_memmap == 10 && e.below_min, // 10×3000/3000×6000/10000=6 <12
        Err(_) => false,
    };
    if clamp_ok {
        s.ok("U081-混-03-自由度上限钳制");
    } else {
        s.fail("U081-混-03-自由度上限钳制", "钳制面失效");
    }

    // 降-01：槽决策恰值（复现/非零槽/降级恒槽0/零种子拒/越界拒）。
    if probe_slot_decision() {
        s.ok("U081-降-01-槽决策恰值");
    } else {
        s.fail("U081-降-01-槽决策恰值", "槽面失效");
    }

    // 降-02：账本守恒三流对账。
    if probe_ledger() {
        s.ok("U081-降-02-账本守恒三流");
    } else {
        s.fail("U081-降-02-账本守恒三流", "账面失效");
    }

    // 降-03：告警账容量界——灌爆拒入旧账保留 + 摘要含守恒态。
    let mut full = AlertLog::new();
    let mut i2 = 0usize;
    while i2 < ALERT_CAPACITY + 5 {
        full.push(KaslrCode::BelowThreshold, "灌");
        i2 += 1;
    }
    let full_ok = full.len() == ALERT_CAPACITY && full.rejected_full == 5
        && full.summary().contains("段守恒=真");
    if full_ok {
        s.ok("U081-降-03-告警账容量界");
    } else {
        s.fail("U081-降-03-告警账容量界", "容量闸失效");
    }

    // 组-01：十六码段守恒 + 详情互异非空。
    let codes = KaslrCode::ALL;
    let mut codes_ok = codes.len() == 16;
    let mut i3 = 0usize;
    while i3 < codes.len() && codes_ok {
        if codes[i3].detail().is_empty() {
            codes_ok = false;
        }
        let mut j = i3 + 1;
        while j < codes.len() {
            if codes[i3].code() == codes[j].code() || codes[i3].detail() == codes[j].detail() {
                codes_ok = false;
            }
            j += 1;
        }
        i3 += 1;
    }
    if codes_ok && segment_guard() {
        s.ok("U081-组-01-十六码段守恒");
    } else {
        s.fail("U081-组-01-十六码段守恒", "码面漂移");
    }

    // 组-02：参考探针四口径全真。
    if reference_budget_check() {
        s.ok("U081-组-02-四口径探针全真");
    } else {
        s.fail("U081-组-02-四口径探针全真", "探针失效");
    }

    // 元-01：零 panic 面 + 空态平凡 + 快照往返 + log2 恰值。
    let src = include_str!("unx081_kaslr_entropy.rs");
    let clean = strip_lexical_noise(src);
    let mut hits = 0usize;
    for pat in [".expect(", "panic!", "unreachable!", "unsafe"].iter() {
        if clean.contains(pat) {
            hits += 1;
        }
    }
    let empty_a = AlertLog::new();
    let snap0 = KaslrSnap::capture(&empty_a);
    let log2_ok = log2_floor(0) == 0
        && log2_floor(1) == 0
        && log2_floor(2) == 1
        && log2_floor(1024) == 10
        && log2_floor(1023) == 9;
    let empty_ok = empty_a.is_empty()
        && empty_a.summary().contains("U081")
        && snap0.roundtrip().is_ok()
        && !UNX081_KASLR_VERSION.is_empty()
        && slot_base(0) == 0
        && slot_base(3) == 3 * SLOT_GRANULARITY
        && log2_ok;
    if hits == 0 && empty_ok {
        s.ok("U081-元-01-零panic空态平凡");
    } else {
        s.fail("U081-元-01-零panic空态平凡", "panic 面或空态失效");
    }

    s
}

/// 单遍词法剥除（注释剥除、字符串整体剥空——判据模式串不自 hit）。
fn strip_lexical_noise(src: &str) -> String {
    let b = src.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    while i < b.len() {
        if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'/' {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'*' {
            let mut depth = 1usize;
            i += 2;
            while i < b.len() && depth > 0 {
                if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'*' {
                    depth += 1;
                    i += 2;
                } else if i + 1 < b.len() && b[i] == b'*' && b[i + 1] == b'/' {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            continue;
        }
        if b[i] == b'"' {
            i += 1;
            while i < b.len() {
                if b[i] == b'\\' {
                    i += 2;
                    continue;
                }
                let closed = b[i] == b'"';
                i += 1;
                if closed {
                    break;
                }
            }
            continue;
        }
        if b[i] == b'\'' {
            i += 1;
            while i < b.len() {
                if b[i] == b'\\' {
                    i += 2;
                    continue;
                }
                let closed = b[i] == b'\'';
                i += 1;
                if closed {
                    break;
                }
            }
            continue;
        }
        if let Some(c) = src.get(i..i + 1) {
            out.push_str(c);
        }
        i += 1;
    }
    out
}
