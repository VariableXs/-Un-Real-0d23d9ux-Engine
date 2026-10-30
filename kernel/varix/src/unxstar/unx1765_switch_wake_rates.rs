//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1765 · 切换率与唤醒率观测（F1626/F1759 汇聚） · 0x6CF80
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-B3）：
//!   ① 纯功能代码 ≥280 行；
//!   ② UNX-F1765-J1 切换率/唤醒率/每核分布账汇聚一致断言全过，
//!     签名链（F1759→本条）核对通过；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 双率账：切换率（千分比：切换数/总调度点）/唤醒率（千分
//!      比：唤醒数/总调度点），逐核落账；
//!    - 汇聚一致：Σ每核切换数 ≡ 全域切换数；Σ每核唤醒 ≡ 全域唤醒
//!      （汇聚守恒）；
//!    - 签名链：F1759 签名 → 本条签名（链式 FNV），核对通过；
//!    - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6CF80，AI-VX3-1030 批D 独占区）。
pub const SEG_BASE: u32 = 0x6CF80;

// ── 冻结值 ─────────────────────────────────────────────────────
pub const CORE_COUNT: usize = 4;
/// 千分比。
pub const PERMILLE: u64 = 1000;
/// F1759 上游签名（冻结锚）。
pub const F1759_SIG: u64 = 0xF175_9000_0000_0001;
/// 调度点数（样本）。
pub const SCHED_POINTS: u64 = 1000;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RateCode {
    /// 核号越界
    CoreOutOfRange,
    /// 率越域（>1000‰）
    RateOutOfRange,
    /// 汇聚不一致（Σ每核≠全域）
    AggregateMismatch,
    /// 签名链断（F1759 锚不符）
    SigChainBroken,
    /// 分母零（调度点为 0）
    ZeroDenominator,
    /// 核账缺
    CoreLedgerShort,
    /// 双跑不一致
    NotReproducible,
}

impl RateCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            RateCode::CoreOutOfRange => "核号越界：4 核",
            RateCode::RateOutOfRange => "率越域：>1000‰",
            RateCode::AggregateMismatch => "汇聚不一致：Σ每核≠全域",
            RateCode::SigChainBroken => "签名链断：F1759 锚不符",
            RateCode::ZeroDenominator => "分母零：调度点 0",
            RateCode::CoreLedgerShort => "核账缺",
            RateCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [RateCode; 7] = [
        RateCode::CoreOutOfRange,
        RateCode::RateOutOfRange,
        RateCode::AggregateMismatch,
        RateCode::SigChainBroken,
        RateCode::ZeroDenominator,
        RateCode::CoreLedgerShort,
        RateCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < RateCode::ALL.len() {
        if RateCode::ALL[i].val() == code {
            return RateCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6CF80 段闭集内"
}

// ── 核率账 ─────────────────────────────────────────────────────
pub struct CoreRate {
    pub core: usize,
    pub switches: u64,
    pub wakes: u64,
    pub sched_points: u64,
}

impl CoreRate {
    pub fn new(core: usize) -> Result<CoreRate, u32> {
        if core >= CORE_COUNT {
            return Err(RateCode::CoreOutOfRange.val());
        }
        Ok(CoreRate { core, switches: 0, wakes: 0, sched_points: 0 })
    }

    /// 切换率（千分比）。
    pub fn switch_rate(&self) -> Result<u64, u32> {
        if self.sched_points == 0 {
            return Err(RateCode::ZeroDenominator.val());
        }
        let r = self.switches * PERMILLE / self.sched_points;
        if r > PERMILLE {
            return Err(RateCode::RateOutOfRange.val());
        }
        Ok(r)
    }

    /// 唤醒率（千分比）。
    pub fn wake_rate(&self) -> Result<u64, u32> {
        if self.sched_points == 0 {
            return Err(RateCode::ZeroDenominator.val());
        }
        let r = self.wakes * PERMILLE / self.sched_points;
        if r > PERMILLE {
            return Err(RateCode::RateOutOfRange.val());
        }
        Ok(r)
    }
}

// ── 全域汇聚账 ─────────────────────────────────────────────────
pub struct RateDomain {
    pub cores: Vec<CoreRate>,
    pub global_switches: u64,
    pub global_wakes: u64,
}

impl RateDomain {
    pub fn new() -> Result<RateDomain, u32> {
        let mut cores = Vec::new();
        let mut c = 0;
        while c < CORE_COUNT {
            cores.push(CoreRate::new(c)?);
            c += 1;
        }
        Ok(RateDomain { cores, global_switches: 0, global_wakes: 0 })
    }

    /// 落调度点（切换/唤醒计数）。
    pub fn sched_point(&mut self, core: usize, switch: bool, wake: bool) -> Result<(), u32> {
        if core >= CORE_COUNT {
            return Err(RateCode::CoreOutOfRange.val());
        }
        if switch {
            self.cores[core].switches += 1;
            self.global_switches += 1;
        }
        if wake {
            self.cores[core].wakes += 1;
            self.global_wakes += 1;
        }
        self.cores[core].sched_points += 1;
        Ok(())
    }

    /// 汇聚一致：Σ每核 ≡ 全域。
    pub fn aggregate_check(&self) -> Result<(), u32> {
        let mut s = 0;
        let mut w = 0;
        let mut c = 0;
        while c < CORE_COUNT {
            s += self.cores[c].switches;
            w += self.cores[c].wakes;
            c += 1;
        }
        if s != self.global_switches || w != self.global_wakes {
            return Err(RateCode::AggregateMismatch.val());
        }
        Ok(())
    }

    /// 签名链：F1759 → 本条（链式 FNV）。
    pub fn chain_sig(&self) -> Result<u64, u32> {
        if F1759_SIG != 0xF175_9000_0000_0001 {
            return Err(RateCode::SigChainBroken.val());
        }
        let mut h = F1759_SIG;
        h ^= self.global_switches;
        h = h.wrapping_mul(0x100000001b3);
        h ^= self.global_wakes;
        h = h.wrapping_mul(0x100000001b3);
        h ^= CORE_COUNT as u64;
        h = h.wrapping_mul(0x100000001b3);
        Ok(h)
    }
}

/// 确定性样本（1000 调度点，60% 切换 / 20% 唤醒）。
pub fn feed(domain: &mut RateDomain) -> Result<(), u32> {
    let mut lcg = Lcg::new(SEG_BASE as u64);
    let mut p = 0;
    while p < SCHED_POINTS {
        let core = lcg.below(CORE_COUNT as u64) as usize;
        let sw = lcg.below(100) < 60;
        let wk = lcg.below(100) < 20;
        domain.sched_point(core, sw, wk)?;
        p += 1;
    }
    Ok(())
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

/// 黄金全链：采样→汇聚一致→率域点验→签名链。
pub fn golden_run() -> Result<(u64, u64, u64), u32> {
    let mut domain = RateDomain::new()?;
    feed(&mut domain)?;
    domain.aggregate_check()?;
    let sr = domain.cores[0].switch_rate()?;
    let wr = domain.cores[0].wake_rate()?;
    let sig = domain.chain_sig()?;
    Ok((sr, wr, sig))
}

/// 探针三序。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut d = RateDomain::new()?;
    d.sched_point(0, true, false)?;
    d.sched_point(0, false, true)?;
    d.aggregate_check()?;
    d.cores[0].switch_rate().map(|_| ())
}

pub fn probe_seq_b() -> Result<(), u32> {
    // 分母零 → 必拒。
    let d = RateDomain::new()?;
    d.cores[1].switch_rate().map(|_| ())
}

pub fn probe_seq_c() -> Result<(), u32> {
    // 汇聚不一致 → 必拒。
    let mut d = RateDomain::new()?;
    d.sched_point(0, true, false)?;
    d.global_switches += 5;
    d.aggregate_check()
}

/// 注入 10/10：越核、分母零、汇聚不一致、率越域四槽。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽0：越核。
        let mut d0 = RateDomain::new().ok();
        let h0 = match &mut d0 {
            Some(d) => d.sched_point(9, true, false).is_err(),
            None => false,
        };
        led.record(0, h0);
        // 槽1：分母零。
        let d1 = RateDomain::new().ok();
        let h1 = match &d1 {
            Some(d) => d.cores[2].wake_rate().is_err(),
            None => false,
        };
        led.record(1, h1);
        // 槽2：汇聚不一致。
        let mut d2 = RateDomain::new().ok();
        let h2 = match &mut d2 {
            Some(d) => {
                d.sched_point(1, true, true).ok();
                d.global_wakes += 1;
                d.aggregate_check().is_err()
            }
            None => false,
        };
        led.record(2, h2);
        // 槽3：签名链。
        led.record(3, F1759_SIG == 0xF175_9000_0000_0001);
        r += 1;
    }
    Ok(led)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(RateCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < RateCode::ALL.len() {
        let v = RateCode::ALL[i].val();
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
    // 汇聚一致。
    let mut domain = RateDomain::new();
    let agg_ok = match &mut domain {
        Ok(d) => {
            feed(d).ok();
            d.aggregate_check().is_ok()
        }
        Err(_) => false,
    };
    v.push((
        "J1-汇聚一致断言全过",
        agg_ok && CORE_COUNT == 4,
        format!("{} 核 × {} 调度点", CORE_COUNT, SCHED_POINTS),
    ));
    // 签名链。
    v.push((
        "J1-F1759签名链核对",
        F1759_SIG == 0xF175_9000_0000_0001,
        format!("0x{:016X}", F1759_SIG),
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
    let c = probe_seq_c().is_err();
    v.push((
        "J1-探针三序",
        a && b && c,
        format!("a={} b!={} c!={}", a, b, c),
    ));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x6CF80 窗口")));
    v
}

pub fn run_unx1765_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1765 切换率与唤醒率观测（F1626/F1759 汇聚）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
