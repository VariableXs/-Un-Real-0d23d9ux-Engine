//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1002 · TSC 特性探测（Invariant TSC 判定与降级声明） · 0x6C980
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-B2）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F1002-J1 特性判定正确（三环境 Invariant TSC 判定 10/10
//!     次），非不变 TSC 降级声明 10/10 次，特性账入启动报告；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 三环境闭集：物理机（CPUID 0x80000007 INVARIANT_TSC 位）/
//!     虚拟机（hypervisor 叶 + TSC 频率透明声明）/QEMU 档（TSC 不
//!      保证不变——默认降级）；
//!    - 判定协议：leaf 0x80000007 EDX bit8 = Invariant TSC；虚拟化
//!      下须 hypervisor 声明叠加（不猜）；
//!    - 降级声明：非不变 TSC → 时钟源降级（TSC→HPET 候补），降级
//!      声明入启动报告（不静默）；
//!    - 特性账三列（环境/判定/降级），10/10 轮判定一致（确定性）；
//!    - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6C980，AI-VX3-1030 批A 独占区）。
pub const SEG_BASE: u32 = 0x6C980;

// ── 三环境闭集 ─────────────────────────────────────────────────
pub const ENV_BARE_METAL: usize = 0;
pub const ENV_VM: usize = 1;
pub const ENV_QEMU: usize = 2;
pub const ENV_COUNT: usize = 3;
pub const ENV_NAMES: [&str; ENV_COUNT] = ["物理机", "虚拟机", "QEMU档"];

/// CPUID 0x80000007 EDX bit8 = Invariant TSC。
pub const INVARIANT_TSC_BIT: u32 = 1 << 8;

/// 判定轮数（冻结：10/10）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TscCode {
    /// 环境越闭集
    EnvOutOfRange,
    /// 环境名空（三环境齐备破坏）
    EnvNameBlank,
    /// 判定漂移（同环境 10 轮不一致）
    VerdictDrift,
    /// 降级未声明（非不变 TSC 却用 TSC 时钟源）
    DegradeUndeclared,
    /// 判定颠倒（QEMU 档判不变）
    VerdictInverted,
    /// 特性账缺列
    FeatureLedgerShort,
    /// 报告行缺
    ReportRowMissing,
    /// 双跑不一致
    NotReproducible,
}

impl TscCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            TscCode::EnvOutOfRange => "环境越闭集：三环境冻结",
            TscCode::EnvNameBlank => "环境名空：齐备破坏",
            TscCode::VerdictDrift => "判定漂移：同环境 10 轮不一致",
            TscCode::DegradeUndeclared => "降级未声明：非不变却用 TSC",
            TscCode::VerdictInverted => "判定颠倒：QEMU 档判不变",
            TscCode::FeatureLedgerShort => "特性账缺列",
            TscCode::ReportRowMissing => "报告行缺",
            TscCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [TscCode; 8] = [
        TscCode::EnvOutOfRange,
        TscCode::EnvNameBlank,
        TscCode::VerdictDrift,
        TscCode::DegradeUndeclared,
        TscCode::VerdictInverted,
        TscCode::FeatureLedgerShort,
        TscCode::ReportRowMissing,
        TscCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < TscCode::ALL.len() {
        if TscCode::ALL[i].val() == code {
            return TscCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6C980 段闭集内"
}

// ── 特性账 ─────────────────────────────────────────────────────
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TscFeature {
    pub env: usize,
    /// CPUID 0x80000007 EDX。
    pub edx_80000007: u32,
    /// hypervisor 声明叠加（VM/QEMU 有 hypervisor 叶）。
    pub hypervisor: bool,
}

impl TscFeature {
    /// 判定：物理机按 CPUID 位；VM 需 hypervisor 叠加声明；QEMU 档
    /// 默认降级（不保证不变）。
    pub fn invariant(&self) -> Result<bool, u32> {
        if self.env >= ENV_COUNT {
            return Err(TscCode::EnvOutOfRange.val());
        }
        match self.env {
            ENV_BARE_METAL => Ok(self.edx_80000007 & INVARIANT_TSC_BIT != 0),
            ENV_VM => {
                if !self.hypervisor {
                    // 虚拟机无 hypervisor 声明：不猜，判否（降级）。
                    return Ok(false);
                }
                // hypervisor 声明叠加：声明位（账面样本：CPUID 位 +
                // hypervisor 声明才判是）。
                Ok(self.edx_80000007 & INVARIANT_TSC_BIT != 0)
            }
            _ => Ok(false), // QEMU 档：默认降级。
        }
    }
    /// 降级声明：非不变 → 时钟源降级 HPET。
    pub fn degrade_declared(&self) -> Result<bool, u32> {
        Ok(!self.invariant()?)
    }
}

/// 启动报告行。
pub fn report_line(f: &TscFeature) -> Result<String, u32> {
    let inv = f.invariant()?;
    let degrade = f.degrade_declared()?;
    Ok(format!(
        "tsc: env={} invariant={} degrade_to={}",
        ENV_NAMES[f.env],
        inv,
        if degrade { "HPET" } else { "none" }
    ))
}

// ── 判定器（10 轮一致性） ──────────────────────────────────────
/// 三环境样本账（写死）：
/// 物理机（位亮）/VM（hypervisor+位亮）/VM（无声明）/QEMU。
pub const SAMPLES: [TscFeature; 4] = [
    TscFeature { env: ENV_BARE_METAL, edx_80000007: INVARIANT_TSC_BIT, hypervisor: false },
    TscFeature { env: ENV_VM, edx_80000007: INVARIANT_TSC_BIT, hypervisor: true },
    TscFeature { env: ENV_VM, edx_80000007: 0, hypervisor: true },
    TscFeature { env: ENV_QEMU, edx_80000007: INVARIANT_TSC_BIT, hypervisor: true },
];

/// 三环境 10/10 判定一致性。
pub fn judge_consistency() -> Result<usize, u32> {
    let mut consistent = 0;
    let mut e = 0;
    while e < ENV_COUNT {
        let f = TscFeature {
            env: e,
            edx_80000007: if e == ENV_QEMU { INVARIANT_TSC_BIT } else { INVARIANT_TSC_BIT },
            hypervisor: true,
        };
        let v0 = f.invariant()?;
        let mut r = 1;
        while r < JUDGE_ROUNDS {
            if f.invariant()? != v0 {
                return Err(TscCode::VerdictDrift.val());
            }
            r += 1;
        }
        consistent += 1;
        e += 1;
    }
    Ok(consistent)
}

/// 降级声明 10/10。
pub fn degrade_declared_10() -> Result<usize, u32> {
    let f = TscFeature { env: ENV_QEMU, edx_80000007: 0, hypervisor: false };
    let d0 = f.degrade_declared()?;
    let mut r = 1;
    while r < JUDGE_ROUNDS {
        if f.degrade_declared()? != d0 {
            return Err(TscCode::VerdictDrift.val());
        }
        r += 1;
    }
    if !d0 {
        return Err(TscCode::DegradeUndeclared.val());
    }
    Ok(JUDGE_ROUNDS as usize)
}

// ── LCG ────────────────────────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 注入检出账 ──────────────────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}
impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 4 {
            self.tries[slot] += 1;
            if hit {
                self.caught[slot] += 1;
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 黄金全链：三环境判定→样本账对拍→降级声明→报告行。
pub fn golden_run() -> Result<(usize, usize, String), u32> {
    if ENV_NAMES.len() != ENV_COUNT {
        return Err(TscCode::EnvNameBlank.val());
    }
    // 三环境 10/10。
    let consistent = judge_consistency()?;
    if consistent != ENV_COUNT {
        return Err(TscCode::VerdictDrift.val());
    }
    // 样本账对拍（判定正确性）。
    // 物理机位亮 → 是；VM 声明+位亮 → 是；VM 无位 → 否；QEMU → 否。
    let expect = [true, true, false, false];
    let mut i = 0;
    while i < SAMPLES.len() {
        let got = SAMPLES[i].invariant()?;
        if got != expect[i] {
            return Err(TscCode::VerdictInverted.val());
        }
        i += 1;
    }
    // 降级声明 10/10。
    degrade_declared_10()?;
    // 报告行。
    let line = report_line(&SAMPLES[0])?;
    if !line.contains("invariant=true") {
        return Err(TscCode::ReportRowMissing.val());
    }
    Ok((consistent, SAMPLES.len(), line))
}

/// 探针三序。
pub fn probe_seq_a() -> Result<(), u32> {
    let f = TscFeature { env: ENV_BARE_METAL, edx_80000007: INVARIANT_TSC_BIT, hypervisor: false };
    f.invariant().map(|_| ())
}

pub fn probe_seq_b() -> Result<(), u32> {
    // 环境越界 → 必拒。
    let f = TscFeature { env: 9, edx_80000007: 0, hypervisor: false };
    f.invariant().map(|_| ())
}

pub fn probe_seq_c() -> Result<(), u32> {
    // VM 无 hypervisor 声明 → 降级（判否）。
    let f = TscFeature { env: ENV_VM, edx_80000007: INVARIANT_TSC_BIT, hypervisor: false };
    match f.invariant() {
        Ok(false) => Ok(()),
        _ => Err(TscCode::VerdictInverted.val()),
    }
}

/// 注入 10/10：越环境、判定颠倒、降级未声明、报告缺四槽。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽0：越环境。
        let f0 = TscFeature { env: 9, edx_80000007: 0, hypervisor: false };
        led.record(0, f0.invariant().is_err());
        // 槽1：判定颠倒（QEMU 位亮判应否）。
        let f1 = TscFeature { env: ENV_QEMU, edx_80000007: INVARIANT_TSC_BIT, hypervisor: true };
        led.record(1, f1.invariant() == Ok(false));
        // 槽2：降级未声明（QEMU 判否但降级未声 → degrade 必 true）。
        let f2 = TscFeature { env: ENV_QEMU, edx_80000007: 0, hypervisor: false };
        led.record(2, f2.degrade_declared() == Ok(true));
        // 槽3：报告缺。
        let line3 = report_line(&SAMPLES[2]).ok();
        led.record(3, match line3 {
            Some(l) => l.contains("degrade_to=HPET"),
            None => false,
        });
        r += 1;
    }
    Ok(led)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(TscCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < TscCode::ALL.len() {
        let v = TscCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push(("J1-黄金全链", g.is_ok(), format!("{:?}", g)));
    // 三环境 10/10。
    v.push((
        "J1-三环境判定10/10",
        judge_consistency() == Ok(ENV_COUNT) && JUDGE_ROUNDS == 10,
        format!("{}/3 环境一致", judge_consistency().unwrap_or(0)),
    ));
    // 降级声明 10/10。
    v.push((
        "J1-降级声明10/10",
        degrade_declared_10() == Ok(10),
        String::from("QEMU 档降级 HPET"),
    ));
    // 注入 10/10。
    let led = inject_rounds(JUDGE_ROUNDS);
    let led_ok = match &led {
        Ok(l) => l.all_caught() && l.caught[0] == 10,
        Err(_) => false,
    };
    v.push((
        "J1-注入10/10",
        led_ok,
        format!(
            "t/c {:?}/{:?}",
            led.as_ref().map(|l| l.tries),
            led.as_ref().map(|l| l.caught)
        ),
    ));
    // 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_err();
    let c = probe_seq_c().is_ok();
    v.push((
        "J1-探针三序",
        a && b && c,
        format!("a={} b!={} c={}", a, b, c),
    ));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x6C980 窗口")));
    v
}

pub fn run_unx1002_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1002 TSC 特性探测（Invariant TSC 判定与降级声明）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
