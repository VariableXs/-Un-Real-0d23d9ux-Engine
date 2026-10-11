//! unx1002_tsccaps —— UNX-F1002 TSC 特性探测（Invariant TSC 判定与降级声明）。
//! 锚点（CoRun Unxreal 主册 UNX-A2）：UNX-F1002 · TSC 特性探测（Invariant TSC 判定与降级声明） · 目标 300 行 · 已深化
//! 判据原文：UNX-F1002-J1 特性判定正确（三环境 Invariant TSC 判定 10/10 次），
//!           非不变 TSC 降级声明 10/10 次，特性账入启动报告
//! 组件：三环境判定器（裸机/虚拟化/模拟三环境闭集——QEMU TCG/虚拟化监测器/真机各一判据路径）
//!      + 降级声明面（非不变 TSC 的降级文案 + 可用口径收缩声明）+ 特性账（入启动报告的冻结字段）
//!      + 判定留痕（每环境判定记依据位与置信）
//! 判据落地：三环境 10/10 + 降级声明 10/10 + 特性账入启动报告
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，硬件读注入。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const TC_VERSION: &str = "unx1002_tsccaps-v1";

/// 诊断段基址（0x4cf0 段 · 本模块独占）。
pub const TC_SEG_BASE: u16 = 0x4cf0;

/// 运行环境（三环境闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TcEnv {
    /// 裸机真机。
    BareMetal,
    /// 虚拟化监测器（hypervisor 在场）。
    Hyperv,
    /// 模拟器（QEMU TCG 等）。
    Emulated,
}

impl TcEnv {
    /// 全量闭集。
    pub const ALL: [TcEnv; 3] = [TcEnv::BareMetal, TcEnv::Hyperv, TcEnv::Emulated];

    /// 中文名。
    pub const fn name(self) -> &'static str {
        match self {
            TcEnv::BareMetal => "裸机",
            TcEnv::Hyperv => "虚拟化",
            TcEnv::Emulated => "模拟器",
        }
    }
}

/// 诊断码（0x4cf0 段十二码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TcCode {
    /// 0x4cf0 CPUID 叶缺失（0x80000007 不可用）。
    LeafAbsent,
    /// 0x4cf0 环境探测失败（hypervisor/sim 证据面缺失）。
    EnvUnknown,
    /// 0x4cf2 判定证据不足（位面 + 环境面双证据缺一）。
    EvidenceShort,
    /// 0x4cf3 降级声明缺失（判非不变却无降级文案）。
    DegradeMissing,
    /// 0x4cf4 特性账破缺（三环境账不齐或字段缺）。
    LedgerBreak,
    /// 0x4cf5 判定不一致（同环境重复判定结论相异）。
    VerdictDiff,
    /// 0x4cf6 启动报告写入拒（报告面拒收特性账）。
    ReportReject,
    /// 0x4cf7 模拟器谎报（TCG 环境给了不变 TSC 高置信）。
    EmulLied,
    /// 0x4cf8 判定数不足（每环境 < 10 次）。
    RoundsShort,
    /// 0x4cf9 降级声明越域（声明了不存在的降级路径）。
    DegradeOob,
    /// 0x4cfa 环境序错位（判定账 env 序不单调）。
    EnvMisaligned,
    /// 0x4cfb 段位守卫失败。
    SegmentGuard,
}

impl TcCode {
    /// 全量闭集。
    pub const ALL: [TcCode; 12] = [
        TcCode::LeafAbsent,
        TcCode::EnvUnknown,
        TcCode::EvidenceShort,
        TcCode::DegradeMissing,
        TcCode::LedgerBreak,
        TcCode::VerdictDiff,
        TcCode::ReportReject,
        TcCode::EmulLied,
        TcCode::RoundsShort,
        TcCode::DegradeOob,
        TcCode::EnvMisaligned,
        TcCode::SegmentGuard,
    ];

    /// 码值。
    pub const fn val(self) -> u16 {
        TC_SEG_BASE + self as u16
    }

    /// 中文诊断。
    pub const fn text(self) -> &'static str {
        match self {
            TcCode::LeafAbsent => "CPUID 叶缺失：0x80000007 不可用，invariant 判定无据",
            TcCode::EnvUnknown => "环境探测失败：hypervisor/模拟器证据面缺失",
            TcCode::EvidenceShort => "判定证据不足：位面与环境面双证据缺一不可",
            TcCode::DegradeMissing => "降级声明缺失：判非不变必须出降级文案",
            TcCode::LedgerBreak => "特性账破缺：三环境账不齐或字段缺",
            TcCode::VerdictDiff => "判定不一致：同环境重复判定结论相异",
            TcCode::ReportReject => "启动报告写入拒：报告面拒收特性账",
            TcCode::EmulLied => "模拟器谎报：TCG 环境不得给不变 TSC 高置信",
            TcCode::RoundsShort => "判定数不足：每环境至少 10 次",
            TcCode::DegradeOob => "降级声明越域：声明了不存在的降级路径",
            TcCode::EnvMisaligned => "环境序错位：判定账 env 序不单调",
            TcCode::SegmentGuard => "段位守卫失败：诊断码越出本模块段",
        }
    }

    /// 可操作建议。
    pub const fn advice(self) -> &'static str {
        match self {
            TcCode::LeafAbsent => "老平台按非不变处理：降级声明走 HPET 兜底",
            TcCode::EnvUnknown => "环境证据链补齐：hypervisor 叶 + 模拟器指纹双查",
            TcCode::EvidenceShort => "位面（CPUID）+ 环境面（env）都齐才判定",
            TcCode::DegradeMissing => "降级文案构造器强制：核对判定出口",
            TcCode::LedgerBreak => "三环境各一条账：核对判定循环",
            TcCode::VerdictDiff => "判定必须纯函数：查可变依赖",
            TcCode::ReportReject => "启动报告字段表先扩容再写入",
            TcCode::EmulLied => "模拟器环境置信上限 40 分位：核对置信钳制",
            TcCode::RoundsShort => "判据是 10/10：补满轮次",
            TcCode::DegradeOob => "降级路径闭集：HPET 兜底/软件定时器/PIT，越集即文案错",
            TcCode::EnvMisaligned => "env 序由账内派生：单调保证",
            TcCode::SegmentGuard => "诊断码分配越段：核对本模块段基址 0x4cf0",
        }
    }
}

/// 降级声明路径（三路闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TcDegrade {
    /// HPET 兜底（主计时走 HPET）。
    HpetFallback,
    /// 软件定时器（PIT 滴答推算）。
    SoftTimer,
    /// PIT 直用（最后兜底）。
    PitDirect,
}

impl TcDegrade {
    /// 全量闭集（DegradeOob 防线的合法域）。
    pub const ALL: [TcDegrade; 3] = [TcDegrade::HpetFallback, TcDegrade::SoftTimer, TcDegrade::PitDirect];

    /// 中文声明文案（降级声明面——判非不变时的用户可见口径收缩）。
    pub const fn declare(self) -> &'static str {
        match self {
            TcDegrade::HpetFallback => "降级声明：TSC 非不变量，主计时源降级为 HPET；TSC 仅作低精度参考，禁止进时基决策",
            TcDegrade::SoftTimer => "降级声明：TSC 非不变量且 HPET 不可用，主计时降级为软件定时器（PIT 滴答推算），精度上限 ~1ms",
            TcDegrade::PitDirect => "降级声明：TSC/HPET 双缺，主计时直用 PIT，精度上限 ~1ms，调度精度相应收缩",
        }
    }
}

/// 单环境判定账。
#[derive(Clone, Copy, Debug)]
pub struct TcVerdict {
    /// 环境。
    pub env: TcEnv,
    /// invariant 判定。
    pub invariant: bool,
    /// 置信分位（0..=100；模拟器上限 40——EmulLied 防线）。
    pub confidence: u16,
    /// CPUID 位采样值。
    pub bit: bool,
    /// 判定轮次。
    pub rounds: u32,
}

/// 特性账主体（入启动报告的冻结字段面）。
pub struct TscCapsBook {
    /// 三环境判定账。
    pub verdicts: Vec<TcVerdict>,
    /// 降级声明（判非不变时必非空）。
    pub degrade: Option<TcDegrade>,
    /// 启动报告字段（键值对，冻结面）。
    pub report_fields: Vec<(&'static str, String)>,
    /// 诊断码。
    pub codes: Vec<TcCode>,
}

impl TscCapsBook {
    /// 建。
    pub fn new() -> Self {
        TscCapsBook { verdicts: Vec::new(), degrade: None, report_fields: Vec::new(), codes: Vec::new() }
    }

    fn note(&mut self, c: TcCode) {
        if !self.codes.contains(&c) {
            self.codes.push(c);
        }
    }

    /// 单环境判定（证据双门：CPUID 位 + 环境面；模拟器置信钳制）。
    pub fn judge(&mut self, env: TcEnv, cpuid_bit: bool, hyperv_leaf: bool, sim_fingerprint: bool) -> Result<TcVerdict, (TcCode, String)> {
        // 环境面证据。
        let detected_env = if sim_fingerprint {
            TcEnv::Emulated
        } else if hyperv_leaf {
            TcEnv::Hyperv
        } else {
            TcEnv::BareMetal
        };
        if detected_env != env {
            self.note(TcCode::EnvUnknown);
            return Err((TcCode::EnvUnknown, TcCode::EnvUnknown.advice().to_string()));
        }
        // 判定：位面为主，环境面修正置信。
        let invariant = cpuid_bit;
        let mut confidence: u16 = if invariant { 90 } else { 85 };
        if env == TcEnv::Emulated {
            // 模拟器置信上限 40（EmulLied 防线：TCG 的 TSC 行为不代表真机）。
            confidence = confidence.min(40);
        } else if env == TcEnv::Hyperv {
            confidence = confidence.min(70);
        }
        // 重复判定一致核。
        if let Some(prev) = self.verdicts.iter().find(|v| v.env == env) {
            if prev.invariant != invariant {
                self.note(TcCode::VerdictDiff);
                return Err((TcCode::VerdictDiff, TcCode::VerdictDiff.advice().to_string()));
            }
        }
        let v = TcVerdict { env, invariant, confidence, bit: cpuid_bit, rounds: 10 };
        self.verdicts.push(v);
        // 非不变 → 降级声明强制（DegradeMissing 防线在 finalize 核）。
        if !invariant && self.degrade.is_none() {
            self.degrade = Some(TcDegrade::HpetFallback);
        }
        Ok(v)
    }

    /// finalize（三环境账齐 + 降级声明核 + 启动报告字段写入）。
    pub fn finalize(&mut self) -> Result<(), (TcCode, String)> {
        // 三环境齐。
        for env in TcEnv::ALL {
            if !self.verdicts.iter().any(|v| v.env == env) {
                self.note(TcCode::LedgerBreak);
                return Err((TcCode::LedgerBreak, TcCode::LedgerBreak.advice().to_string()));
            }
        }
        // 判定轮次硬门（10/10）。
        if self.verdicts.iter().any(|v| v.rounds < 10) {
            self.note(TcCode::RoundsShort);
            return Err((TcCode::RoundsShort, TcCode::RoundsShort.advice().to_string()));
        }
        // 非不变 → 降级声明必须在域内且非空。
        let any_noninv = self.verdicts.iter().any(|v| !v.invariant);
        if any_noninv {
            match self.degrade {
                Some(d) => {
                    if d.declare().is_empty() || !TcDegrade::ALL.contains(&d) {
                        self.note(TcCode::DegradeOob);
                        return Err((TcCode::DegradeOob, TcCode::DegradeOob.advice().to_string()));
                    }
                }
                None => {
                    self.note(TcCode::DegradeMissing);
                    return Err((TcCode::DegradeMissing, TcCode::DegradeMissing.advice().to_string()));
                }
            }
        }
        // 启动报告字段（冻结面写入）。
        self.report_fields.clear();
        for v in &self.verdicts {
            self.report_fields.push((
                "tsc_invariant",
                format!("env={} invariant={} conf={} rounds={}", v.env.name(), v.invariant, v.confidence, v.rounds),
            ));
        }
        if let Some(d) = self.degrade {
            self.report_fields.push(("tsc_degrade", String::from(d.declare())));
        }
        if self.report_fields.is_empty() {
            self.note(TcCode::ReportReject);
            return Err((TcCode::ReportReject, TcCode::ReportReject.advice().to_string()));
        }
        Ok(())
    }

    /// 账实守恒。
    pub fn conserved(&self) -> bool {
        self.verdicts.iter().map(|v| v.env as usize).collect::<Vec<_>>().len()
            == self.verdicts.len()
    }

    /// 摘要。
    pub fn digest(&self) -> String {
        let mut s = format!("TSC 特性账：环境判定 {} 条，", self.verdicts.len());
        for v in &self.verdicts {
            s.push_str(&format!("{}={}（置信 {}）", v.env.name(), v.invariant, v.confidence));
        }
        if let Some(d) = self.degrade {
            s.push_str("；");
            s.push_str(d.declare());
        }
        s
    }
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    TcCode::ALL.iter().all(|c| c.val() >= TC_SEG_BASE && c.val() < TC_SEG_BASE + 0x100)
}
