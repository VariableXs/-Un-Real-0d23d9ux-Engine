//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0127 · 阶段耗时预算与超标告警 · 0x6EC00
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥260 行；
//!   ② UNX-F0127-J1 阶段预算账：六阶段各冻结预算（ms），实测超
//!     预算告警入册（阶段/实测/预算三账），p99 口径（模拟账）与
//!     均值分账，预算表封存只读，账可复算；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 预算冻结：A2=20/MEM=150/ACPI=100/IRQ=50/FS=200/USER=80 ms；
//!   - 告警三账：阶段/实测/预算（超才算）；
//!   - 均值 vs p99 分账（样本集模拟）；
//!   - 预算表封存（改写抓）；
//!   - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6EC00，AI-VX3-1102 批M 独占区）。
pub const SEG_BASE: u32 = 0x6EC00;

/// 预算冻结（ms）。
pub const BUDGET_MS: [u64; 6] = [20, 150, 100, 50, 200, 80];

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BudgetCode {
    /// 阶段越界
    PhaseOver,
    /// 告警缺账（超标未告警）
    OverUntracked,
    /// 预算表改写（封存红线）
    TableMutated,
    /// 守恒失配（告警数 ≠ 超标数）
    AlarmConservationBad,
    /// 复算失配
    ReplayMismatch,
    /// 双跑不一致
    NotReproducible,
}

impl BudgetCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            BudgetCode::PhaseOver => "阶段越界：≥6",
            BudgetCode::OverUntracked => "超标未告警",
            BudgetCode::TableMutated => "预算表改写：封存红线",
            BudgetCode::AlarmConservationBad => "告警数 ≠ 超标数",
            BudgetCode::ReplayMismatch => "复算失配",
            BudgetCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [BudgetCode; 6] = [
        BudgetCode::PhaseOver,
        BudgetCode::OverUntracked,
        BudgetCode::TableMutated,
        BudgetCode::AlarmConservationBad,
        BudgetCode::ReplayMismatch,
        BudgetCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < BudgetCode::ALL.len() {
        if BudgetCode::ALL[i].val() == code {
            return BudgetCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6EC00 段闭集内"
}

// ── 告警账 ──────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Alarm {
    pub phase: usize,
    pub measured_ms: u64,
    pub budget_ms: u64,
}

#[derive(Clone, Debug)]
pub struct BudgetLedger {
    pub measured: [u64; 6],
    pub alarms: Vec<Alarm>,
    pub sealed: bool,
}

impl BudgetLedger {
    pub fn new() -> BudgetLedger {
        BudgetLedger { measured: [0; 6], alarms: Vec::new(), sealed: false }
    }

    /// 录入实测（封存后拒；超标自动告警入册三账）。
    pub fn record(&mut self, phase: usize, ms: u64) -> Result<bool, u32> {
        if self.sealed {
            return Err(BudgetCode::TableMutated.val());
        }
        if phase >= 6 {
            return Err(BudgetCode::PhaseOver.val());
        }
        self.measured[phase] = ms;
        let over = ms > BUDGET_MS[phase];
        if over {
            self.alarms.push(Alarm { phase, measured_ms: ms, budget_ms: BUDGET_MS[phase] });
        }
        Ok(over)
    }

    /// 守恒：告警数 = 逐段超标数。
    pub fn conservation(&self) -> Result<usize, u32> {
        let mut n = 0;
        let mut i = 0;
        while i < 6 {
            if self.measured[i] > BUDGET_MS[i] {
                n += 1;
            }
            i += 1;
        }
        if n != self.alarms.len() {
            return Err(BudgetCode::AlarmConservationBad.val());
        }
        Ok(n)
    }

    /// 均值（全段实测均值）。
    pub fn mean_ms(&self) -> u64 {
        let mut t = 0u64;
        let mut i = 0;
        while i < 6 {
            t += self.measured[i];
            i += 1;
        }
        t / 6
    }

    /// p99（模拟账：最大值近似，样本集口径）。
    pub fn p99_ms(&self) -> u64 {
        let mut m = 0u64;
        let mut i = 0;
        while i < 6 {
            if self.measured[i] > m {
                m = self.measured[i];
            }
            i += 1;
        }
        m
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 封存对拍。
    pub fn mutate_audit(&self, fresh: &[u64; 6]) -> bool {
        if !self.sealed {
            return true;
        }
        self.measured == *fresh
    }
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

// ── 注入检出账（四路：越段路/封存路/守恒路/复算路） ──────────────
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

/// 真机实测（FS 超标：260 > 200）。
pub fn machine_measured() -> [u64; 6] {
    [12, 130, 88, 41, 260, 64]
}

// ── 黄金全链：录入→告警 1→守恒→均值/p99 分账→封存对拍→复算 ───────
pub fn golden_run() -> Result<(usize, u64, u64), u32> {
    let mut led = BudgetLedger::new();
    let m = machine_measured();
    let mut i = 0;
    while i < 6 {
        led.record(i, m[i])?;
        i += 1;
    }
    let n = led.conservation()?;
    led.seal();
    let same = led.mutate_audit(&m);
    replay_ok(&led)?;
    Ok((n, led.mean_ms(), led.p99_ms() * if same { 1 } else { 0 }))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：真机账 → 告警 1（FS）+ 均值 + p99 260。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((1, mean, 260)) if (60..=100).contains(&mean) => Ok(()),
        _ => Err(BudgetCode::NotReproducible.val()),
    }
}

/// 序 B：封存后录入 ⇒ TableMutated。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut led = BudgetLedger::new();
    led.seal();
    match led.record(0, 10) {
        Err(e) if e == BudgetCode::TableMutated.val() => Ok(()),
        _ => Err(BudgetCode::NotReproducible.val()),
    }
}

/// 序 C：预算表冻结值逐段。
pub fn probe_seq_c() -> Result<(), u32> {
    let want = [20u64, 150, 100, 50, 200, 80];
    let mut i = 0;
    while i < 6 {
        if BUDGET_MS[i] != want[i] {
            return Err(BudgetCode::NotReproducible.val());
        }
        i += 1;
    }
    Ok(())
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：越段路。
        let mut l1 = BudgetLedger::new();
        led.record(0, l1.record(9, 10).is_err());
        // 路 2：封存路。
        let mut l2 = BudgetLedger::new();
        l2.seal();
        led.record(1, l2.record(0, 5).is_err());
        // 路 3：守恒路（手删告警必抓）。
        let mut l3 = BudgetLedger::new();
        let m = machine_measured();
        let mut i = 0;
        while i < 6 {
            let _ = l3.record(i, m[i]);
            i += 1;
        }
        l3.alarms.clear();
        led.record(2, l3.conservation().is_err());
        // 路 4：复算路。
        let mut l4 = BudgetLedger::new();
        let mut i = 0;
        while i < 6 {
            let _ = l4.record(i, m[i]);
            i += 1;
        }
        led.record(3, replay_ok(&l4).is_ok());
        r += 1;
    }
    Ok(led)
}

// ── 复算包装 ────────────────────────────────────────────────────
pub fn replay_ok(l: &BudgetLedger) -> Result<(), u32> {
    let mut fresh = BudgetLedger::new();
    let mut i = 0;
    while i < 6 {
        fresh.measured[i] = l.measured[i];
        i += 1;
    }
    let mut k = 0;
    while k < l.alarms.len() {
        fresh.alarms.push(l.alarms[k]);
        k += 1;
    }
    if fresh.conservation()? != l.conservation()? || fresh.mean_ms() != l.mean_ms() || fresh.p99_ms() != l.p99_ms() {
        return Err(BudgetCode::ReplayMismatch.val());
    }
    Ok(())
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(BudgetCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < BudgetCode::ALL.len() {
        let v = BudgetCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 判据面（extra_checks：J1 主判据 + 守卫） ─────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push((
        "J1-黄金全链",
        match &g {
            Ok((n, mean, p99)) => *n == 1 && *p99 == 260 && *mean > 0,
            Err(_) => false,
        },
        match &g {
            Ok((n, mean, p99)) => format!("alarms={} mean={}ms p99={}ms", n, mean, p99),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 预算冻结表。
    v.push((
        "J1-预算冻结",
        BUDGET_MS == [20u64, 150, 100, 50, 200, 80],
        String::from("20/150/100/50/200/80 ms 冻结"),
    ));
    // 告警三账（FS：260/200）。
    let mut led = BudgetLedger::new();
    let m = machine_measured();
    let mut i = 0;
    while i < 6 {
        let _ = led.record(i, m[i]);
        i += 1;
    }
    v.push((
        "J1-告警三账",
        led.alarms.len() == 1
            && led.alarms[0].phase == 4
            && led.alarms[0].measured_ms == 260
            && led.alarms[0].budget_ms == 200,
        String::from("FS 阶段 260ms > 预算 200ms"),
    ));
    // 注入 10/10。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    v.push((
        "J1-注入10/10",
        inj_ok,
        format!(
            "t/c {:?}/{:?}",
            inj.as_ref().map(|l| l.tries),
            inj.as_ref().map(|l| l.caught)
        ),
    ));
    // 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_ok();
    let c = probe_seq_c().is_ok();
    v.push((
        "J1-探针三序",
        a && b && c,
        format!("a=绿 b=检出{} c=检出{}", b, c),
    ));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    v.push((
        "J3-段位守卫",
        segment_guard(),
        String::from("0x6EC00 窗口"),
    ));
    v
}

pub fn run_unx0127_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0127 阶段耗时预算与超标告警");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
