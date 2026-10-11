//! unx402_tscinv —— UNX-F0402 TSC 不变量探测（invariant 标志）。
//! 锚点（CoRun Unxreal 主册 UNX-A1-B20）：UNX-F0402 · TSC 不变量探测（invariant 标志） · 目标 220 行 · 已深化
//! 判据原文：invariant TSC 标志三级探测一致（CPUID 0x80000007 EDX[8]/MSR/手册账 3 源），置信度入账
//! 组件：CPUID 探测器（0x80000007 EDX[8] 位采样）+ MSR 探测器（MSR_PLATFORM_INFO
//!      platform 系数可读性面）+ 手册账（型号族表驱动）+ 三源一致裁决器 + 置信度账
//! 判据落地：三源一致 + 置信度入账（不一致即降级显性，不谎报一致）
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，硬件读注入（零真实 IO）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const TI_VERSION: &str = "unx402_tscinv-v1";

/// 诊断段基址（0x4c20 段 · 本模块独占）。
pub const TI_SEG_BASE: u16 = 0x4c20;

/// CPUID invariant-TSC 叶号（0x80000007）。
pub const TI_CPUID_LEAF: u32 = 0x8000_0007;

/// invariant TSC 位（EDX[8]）。
pub const TI_CPUID_BIT: u32 = 1 << 8;

/// MSR_PLATFORM_INFO 地址（Intel SDM Vol.4：0xCE）。
pub const TI_MSR_PLATFORM_INFO: u32 = 0xCE;

/// IA32_MISC_ENABLE（TSC 调整位所在，0x1A0）。
pub const TI_MSR_MISC: u32 = 0x1A0;

/// 探测源（三源闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TiSource {
    /// CPUID 0x80000007 EDX[8]。
    CpuId,
    /// MSR 平台信息面（platform 系数可读 + TSC 频率系数存在）。
    Msr,
    /// 手册账（型号族表命中 invariant 声明）。
    Manual,
}

impl TiSource {
    /// 全量闭集。
    pub const ALL: [TiSource; 3] = [TiSource::CpuId, TiSource::Msr, TiSource::Manual];

    /// 中文名。
    pub const fn name(self) -> &'static str {
        match self {
            TiSource::CpuId => "CPUID 位采样",
            TiSource::Msr => "MSR 平台面",
            TiSource::Manual => "手册账",
        }
    }
}

/// 三源探测结论（二值，非 bool 别名——语义命名）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TiVerdict {
    /// 该源声称 invariant。
    Yes,
    /// 该源声称非 invariant 或不可探测。
    No,
    /// 该源不可达（虚拟化屏蔽、MSR 不可读等）——显性，不并 No。
    Unreachable,
}

/// 诊断码（0x4c20 段十三码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TiCode {
    /// 0x4c20 三源结论矛盾（有源 Yes 有源 No——不一致即显性）。
    Mismatch,
    /// 0x4c21 三源全不可达（探测无据，结论 Unknown）。
    AllUnreachable,
    /// 0x4c22 CPUID 叶不存在（最高扩展叶 < 0x80000007）。
    CpuIdLeafAbsent,
    /// 0x4c23 MSR 读不可达（权限或虚拟化截获）。
    MsrUnreachable,
    /// 0x4c24 手册账型号未登记（族/型号/步进无表项）。
    ManualMiss,
    /// 0x4c25 置信度账破缺（分位权重和不守恒）。
    ConfidenceBreak,
    /// 0x4c26 结论与置信度错位（Unknown 却高置信）。
    VerdictLied,
    /// 0x4c27 型号表条目重复（同键双写）。
    TableDup,
    /// 0x4c28 型号表坏条目（家族号全零之类平凡非法）。
    TableBad,
    /// 0x4c29 MSR 值域非法（保留位被置位）。
    MsrReserved,
    /// 0x4c2a 虚拟化截获显性（ hypervisor 叶在场，MSR 读可能被伪装）。
    HypervMask,
    /// 0x4c2b 探测账破缺（源数 ≢ 账条数）。
    LedgerBreak,
    /// 0x4c2c 段位守卫失败。
    SegmentGuard,
}

impl TiCode {
    /// 全量闭集。
    pub const ALL: [TiCode; 13] = [
        TiCode::Mismatch,
        TiCode::AllUnreachable,
        TiCode::CpuIdLeafAbsent,
        TiCode::MsrUnreachable,
        TiCode::ManualMiss,
        TiCode::ConfidenceBreak,
        TiCode::VerdictLied,
        TiCode::TableDup,
        TiCode::TableBad,
        TiCode::MsrReserved,
        TiCode::HypervMask,
        TiCode::LedgerBreak,
        TiCode::SegmentGuard,
    ];

    /// 码值。
    pub const fn val(self) -> u16 {
        TI_SEG_BASE + self as u16
    }

    /// 中文诊断。
    pub const fn text(self) -> &'static str {
        match self {
            TiCode::Mismatch => "三源结论矛盾：探测结果不一致，置信度降级并立案",
            TiCode::AllUnreachable => "三源全不可达：TSC invariant 结论 Unknown，不臆造",
            TiCode::CpuIdLeafAbsent => "CPUID 0x80000007 叶不存在：该处理器不支持扩展能效叶",
            TiCode::MsrUnreachable => "MSR 读不可达：权限不足或虚拟化截获",
            TiCode::ManualMiss => "手册账型号未登记：族/型号组合无表项",
            TiCode::ConfidenceBreak => "置信度账破缺：源权重分位不守恒",
            TiCode::VerdictLied => "结论与置信度错位：Unknown 结论不得配高置信度",
            TiCode::TableDup => "型号表条目重复：同族/型号双写",
            TiCode::TableBad => "型号表坏条目：家族号全零为平凡非法值",
            TiCode::MsrReserved => "MSR 值域非法：保留位被置位，读值不可信",
            TiCode::HypervMask => "虚拟化截获显性：hypervisor 在场，MSR 读可能被伪装，降权计账",
            TiCode::LedgerBreak => "探测账破缺：源数与账条数不一致",
            TiCode::SegmentGuard => "段位守卫失败：诊断码越出本模块段",
        }
    }

    /// 可操作建议。
    pub const fn advice(self) -> &'static str {
        match self {
            TiCode::Mismatch => "以最保守源为准（No 优先），标待人工复核后才能进时基决策",
            TiCode::AllUnreachable => "回退 TSC 变换常数路径：invariant 未知时禁止当恒定计时器用",
            TiCode::CpuIdLeafAbsent => "老平台按非 invariant 处理，计时回退 PIT/HPET 路线",
            TiCode::MsrUnreachable => "MSR 面失效不影响 CPUID 面：按二源裁决并降置信度",
            TiCode::ManualMiss => "补型号表条目或按 CPUID 单源降置信度走",
            TiCode::ConfidenceBreak => "权重表必须三源配平：核对 WEIGH 常量表",
            TiCode::VerdictLied => "置信度与结论绑定校验：Unknown 上限 20 分位",
            TiCode::TableDup => "登记前先查表去重，同键更新走替换不改账",
            TiCode::TableBad => "型号表录入校验：家族号零值一律拒",
            TiCode::MsrReserved => "按 MSR 文档核对保留位掩码，重读一次仍脏即判不可达",
            TiCode::HypervMask => "虚拟化下以 CPUID 直读为锚，MSR 面降权",
            TiCode::LedgerBreak => "三源恰好三条账：核对探测循环的记账路径",
            TiCode::SegmentGuard => "诊断码分配越段：核对本模块段基址 0x4c20",
        }
    }
}

/// 单源探测记录。
#[derive(Clone, Copy, Debug)]
pub struct TiProbe {
    /// 源。
    pub source: TiSource,
    /// 结论。
    pub verdict: TiVerdict,
    /// 原始采样值（CPUID EDX / MSR 读值 / 手册表标志位）。
    pub raw: u64,
}

/// 硬件面注入接口（纯功能：零真实 CPUID/MSR 调用，读值注入）。
pub trait TiHw {
    /// 读 CPUID 0x80000007 的 EDX；None = 叶不存在。
    fn cpuid_edx7(&self) -> Option<u32>;
    /// 读 MSR_PLATFORM_INFO；None = 不可达。
    fn msr_platform(&self) -> Option<u64>;
    /// hypervisor 叶（CPUID 0x1 ECX[31]）是否在场。
    fn hyperv_present(&self) -> bool;
}

/// 手册账表项（族/型号 → invariant 声明）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TiManualEntry {
    /// CPUID family。
    pub family: u8,
    /// CPUID model（不含扩展位时的原始值）。
    pub model: u8,
    /// 手册声明 invariant。
    pub invariant: bool,
}

/// 型号手册账（构造期校验：无重复无坏条目）。
pub struct TiManualBook {
    entries: Vec<TiManualEntry>,
}

impl TiManualBook {
    /// 建（构造期校验，坏条目显性拒）。
    pub fn new(entries: &[TiManualEntry]) -> Result<Self, (TiCode, String)> {
        for e in entries {
            if e.family == 0 {
                return Err((TiCode::TableBad, TiCode::TableBad.advice().to_string()));
            }
        }
        let mut book = TiManualBook {
            entries: Vec::new(),
        };
        for e in entries {
            if book.entries.iter().any(|x| x.family == e.family && x.model == e.model) {
                return Err((TiCode::TableDup, TiCode::TableDup.advice().to_string()));
            }
            book.entries.push(*e);
        }
        Ok(book)
    }

    /// 查表。
    pub fn lookup(&self, family: u8, model: u8) -> Option<bool> {
        self.entries
            .iter()
            .find(|e| e.family == family && e.model == model)
            .map(|e| e.invariant)
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

/// 源权重（分位，和 = 100 守恒）。
pub const TI_WEIGHT: [u16; 3] = [50, 30, 20];

/// 置信度分级（四档闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TiConf {
    /// ≥90 分位：三源一致 Yes。
    High,
    /// ≥60：二源一致 Yes（含权重）。
    Medium,
    /// >20：证据不足。
    Low,
    /// 结论 Unknown。
    Unknown,
}

impl TiConf {
    /// 中文名。
    pub const fn name(self) -> &'static str {
        match self {
            TiConf::High => "高置信",
            TiConf::Medium => "中置信",
            TiConf::Low => "低置信",
            TiConf::Unknown => "未知",
        }
    }
}

/// 探测总账（只读结论）。
#[derive(Clone, Debug)]
pub struct TiReport {
    /// 三源记录。
    pub probes: Vec<TiProbe>,
    /// 汇总结论：true= invariant，false= 非 invariant，None= Unknown。
    pub invariant: Option<bool>,
    /// 置信度。
    pub confidence: TiConf,
    /// 置信分位。
    pub score: u16,
    /// 诊断码。
    pub codes: Vec<TiCode>,
}

impl TiReport {
    /// 中文摘要。
    pub fn digest(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!(
            "TSC invariant：结论 {}，置信 {}（{} 分位），源数 {}",
            match self.invariant {
                Some(true) => "invariant",
                Some(false) => "非 invariant",
                None => "Unknown",
            },
            self.confidence.name(),
            self.score,
            self.probes.len()
        ));
        for p in &self.probes {
            s.push_str(&format!(
                "；{} → {}",
                p.source.name(),
                match p.verdict {
                    TiVerdict::Yes => "Yes",
                    TiVerdict::No => "No",
                    TiVerdict::Unreachable => "不可达",
                }
            ));
        }
        if !self.codes.is_empty() {
            s.push_str("；诊断：");
            for c in &self.codes {
                s.push_str(&format!("[0x{:04X} {}] ", c.val(), c.text()));
            }
        }
        s
    }
}

/// 三源探测主入口（硬件面注入 + 手册账 + 型号族三元组）。
pub fn probe<H: TiHw>(hw: &H, book: &TiManualBook, family: u8, model: u8) -> TiReport {
    let mut probes: Vec<TiProbe> = Vec::new();
    let mut codes: Vec<TiCode> = Vec::new();
    // 源 1：CPUID。
    match hw.cpuid_edx7() {
        Some(edx) => {
            let yes = edx & TI_CPUID_BIT != 0;
            probes.push(TiProbe { source: TiSource::CpuId, verdict: if yes { TiVerdict::Yes } else { TiVerdict::No }, raw: edx as u64 });
        }
        None => {
            codes.push(TiCode::CpuIdLeafAbsent);
            probes.push(TiProbe { source: TiSource::CpuId, verdict: TiVerdict::Unreachable, raw: 0 });
        }
    }
    // 源 2：MSR 平台面（platform 系数可读且 TSC 频率系数位型合法 → 面健在即倾向 invariant 声明；保留位脏 → 不可信）。
    const MSR_RESERVED_MASK: u64 = 0xFFFF_FFFF_FFFE_3C00;
    match hw.msr_platform() {
        Some(v) => {
            if v & MSR_RESERVED_MASK != 0 {
                codes.push(TiCode::MsrReserved);
                probes.push(TiProbe { source: TiSource::Msr, verdict: TiVerdict::Unreachable, raw: v });
            } else if v & 0x100 != 0 {
                probes.push(TiProbe { source: TiSource::Msr, verdict: TiVerdict::Yes, raw: v });
            } else {
                probes.push(TiProbe { source: TiSource::Msr, verdict: TiVerdict::No, raw: v });
            }
        }
        None => {
            codes.push(TiCode::MsrUnreachable);
            probes.push(TiProbe { source: TiSource::Msr, verdict: TiVerdict::Unreachable, raw: 0 });
        }
    }
    // 虚拟化降权显性。
    if hw.hyperv_present() {
        codes.push(TiCode::HypervMask);
    }
    // 源 3：手册账。
    match book.lookup(family, model) {
        Some(inv) => probes.push(TiProbe { source: TiSource::Manual, verdict: if inv { TiVerdict::Yes } else { TiVerdict::No }, raw: 1 }),
        None => {
            codes.push(TiCode::ManualMiss);
            probes.push(TiProbe { source: TiSource::Manual, verdict: TiVerdict::Unreachable, raw: 0 });
        }
    }
    // 三源一致裁决（权重计分：Yes 加权，No 减权，Unreachable 不计）。
    let mut score: i32 = 0;
    let mut reachable = 0usize;
    let mut yes_cnt = 0usize;
    let mut no_cnt = 0usize;
    for (i, p) in probes.iter().enumerate() {
        match p.verdict {
            TiVerdict::Yes => {
                score += TI_WEIGHT[i] as i32;
                yes_cnt += 1;
                reachable += 1;
            }
            TiVerdict::No => {
                score -= TI_WEIGHT[i] as i32;
                no_cnt += 1;
                reachable += 1;
            }
            TiVerdict::Unreachable => {}
        }
    }
    let invariant: Option<bool> = if reachable == 0 {
        codes.push(TiCode::AllUnreachable);
        None
    } else if yes_cnt > 0 && no_cnt > 0 {
        // 矛盾：以最保守为准（No），置信降级并立案。
        codes.push(TiCode::Mismatch);
        Some(false)
    } else if yes_cnt > 0 {
        Some(true)
    } else {
        Some(false)
    };
    let score_u = score.clamp(0, 100) as u16;
    let confidence = match invariant {
        Some(true) if score_u >= 90 => TiConf::High,
        Some(true) if score_u >= 60 => TiConf::Medium,
        Some(false) if score_u >= 40 => TiConf::Medium,
        Some(_) => TiConf::Low,
        None => TiConf::Unknown,
    };
    // 结论与置信度错位防线。
    if invariant.is_none() && score_u > 20 {
        codes.push(TiCode::VerdictLied);
    }
    // 权重表守恒（构造期不变量，运行期复核）。
    if TI_WEIGHT.iter().sum::<u16>() != 100 {
        codes.push(TiCode::ConfidenceBreak);
    }
    // 探测账破缺防线：恰好三源三条账。
    if probes.len() != 3 {
        codes.push(TiCode::LedgerBreak);
    }
    TiReport { probes, invariant, confidence, score: score_u, codes }
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    TiCode::ALL.iter().all(|c| c.val() >= TI_SEG_BASE && c.val() < TI_SEG_BASE + 0x100)
}
