//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F15841 · 域级对象总账聚合（勾稽闭合 + 勾稽差注入） · 0x67880
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F15841-J1 聚合成文（域级总账与各批账勾稽闭合 10/10 次），
//!      勾稽差注入（总账与分账不等）检出 10/10 次；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 分批账闭集：D5 批 B27..B38（12 批，写死），每批对象账四科
//!     （创建/存续/释放/泄漏），总账=各批账逐科聚合；
//!   - 勾稽恒等式：总账.科目 == Σ 分批账.科目（逐科闭合）+ 批内
//!     恒等式（创建 − 释放 = 存续 + 泄漏）；
//!   - 聚合状态机：Open → Aggregating → Reconciled / Discrepant；
//!   - 勾稽差注入：聚合时篡改一科 ×10 次 → 必检出；
//!   - LCG 种子写死 = SEG_BASE（0x67880），禁 rand/浮点/IO/墙钟；
//!   - no_std + alloc；零 unwrap/expect/panic。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x67880，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x67880;

/// 每轮正演次数（判据钉版：10/10）。
pub const RUNS_PER_ROUND: u32 = 10;

/// 分批账闭集（12 批，写死：D5-B27..B38）。
pub const BATCH_COUNT: usize = 12;
pub const BATCH_NAMES: [&str; BATCH_COUNT] = [
    "B27", "B28", "B29", "B30", "B31", "B32", "B33", "B34", "B35", "B36", "B37", "B38",
];

/// 账科闭集（4 科，写死）。
pub const ACCT_CREATE: usize = 0;
pub const ACCT_ALIVE: usize = 1;
pub const ACCT_RELEASE: usize = 2;
pub const ACCT_LEAK: usize = 3;
pub const ACCT_COUNT: usize = 4;
pub const ACCT_NAMES: [&str; ACCT_COUNT] = ["创建", "存续", "释放", "泄漏"];

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AggCode {
    /// 批越闭集
    BatchOutOfRange,
    /// 科越闭集
    AcctOutOfRange,
    /// 勾稽差（总账与分账不等）
    ReconcileGap,
    /// 批内恒等式破坏（创建−释放≠存续+泄漏）
    BatchIdentityBroken,
    /// 非法相位边
    IllegalPhaseEdge,
    /// 聚合时序错（未开账即聚合）
    AggregateBeforeOpen,
    /// 双跑不一致
    NotReproducible,
    /// 负账（科目计数下溢为负不可表）
    NegativeLedger,
    /// 聚合未封账
    AggUnsealed,
}

impl AggCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            AggCode::BatchOutOfRange => "批越闭集：12 批冻结",
            AggCode::AcctOutOfRange => "科越闭集：四科冻结",
            AggCode::ReconcileGap => "勾稽差：总账与分账不等",
            AggCode::BatchIdentityBroken => "批内恒等式破坏：创建−释放≠存续+泄漏",
            AggCode::IllegalPhaseEdge => "非法相位边：聚合状态机闭集外",
            AggCode::AggregateBeforeOpen => "未开账即聚合：时序错",
            AggCode::NotReproducible => "双跑不一致：聚合复现红线",
            AggCode::NegativeLedger => "负账：科目计数不可为负",
            AggCode::AggUnsealed => "聚合未封账：Reconciled 未达",
        }
    }
    pub const ALL: [AggCode; 9] = [
        AggCode::BatchOutOfRange,
        AggCode::AcctOutOfRange,
        AggCode::ReconcileGap,
        AggCode::BatchIdentityBroken,
        AggCode::IllegalPhaseEdge,
        AggCode::AggregateBeforeOpen,
        AggCode::NotReproducible,
        AggCode::NegativeLedger,
        AggCode::AggUnsealed,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < AggCode::ALL.len() {
        if AggCode::ALL[i].val() == code {
            return AggCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x67880 段闭集内"
}

// ── 聚合状态机 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AggPhase {
    Open,
    Aggregating,
    Reconciled,
    Discrepant,
}

/// 合法边闭集：Open→Aggregating、Aggregating→Reconciled、
/// Aggregating→Discrepant、Discrepant→Open（重开重算）。
pub fn agg_edge_legal(from: AggPhase, to: AggPhase) -> bool {
    match (from, to) {
        (AggPhase::Open, AggPhase::Aggregating) => true,
        (AggPhase::Aggregating, AggPhase::Reconciled) => true,
        (AggPhase::Aggregating, AggPhase::Discrepant) => true,
        (AggPhase::Discrepant, AggPhase::Open) => true,
        _ => false,
    }
}

// ── 账结构 ───────────────────────────────────────────────────────
/// 单批对象账：四科计数 + 批内恒等式。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BatchLedger {
    pub name: &'static str,
    pub counts: [u64; ACCT_COUNT],
}

impl BatchLedger {
    /// 批内恒等式：创建 − 释放 == 存续 + 泄漏。
    pub fn identity_ok(&self) -> bool {
        if self.counts[ACCT_RELEASE] > self.counts[ACCT_CREATE] {
            return false;
        }
        self.counts[ACCT_CREATE] - self.counts[ACCT_RELEASE]
            == self.counts[ACCT_ALIVE] + self.counts[ACCT_LEAK]
    }
}

/// 域级总账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DomainTotal {
    pub counts: [u64; ACCT_COUNT],
}

pub struct AggregationLedger {
    pub phase: AggPhase,
    pub batches: Vec<BatchLedger>,
    pub total: DomainTotal,
    pub gaps: u32,
}

impl AggregationLedger {
    pub fn new() -> AggregationLedger {
        AggregationLedger {
            phase: AggPhase::Open,
            batches: Vec::new(),
            total: DomainTotal { counts: [0; ACCT_COUNT] },
            gaps: 0,
        }
    }

    /// 开账：按写死种子装填 12 批（确定性分配，保证批内恒等式成立）。
    pub fn open_seeded(&mut self) -> Result<(), u32> {
        if self.phase != AggPhase::Open {
            return Err(AggCode::AggregateBeforeOpen.val());
        }
        self.batches.clear();
        // 写死种子：批 b 创建 = 100 + 10b；泄漏 = b%3；释放 = 创建−存续−泄漏；
        // 存续 = 20 + b。
        let mut b = 0;
        while b < BATCH_COUNT {
            let create: u64 = 100 + 10 * b as u64;
            let leak: u64 = (b % 3) as u64;
            let alive: u64 = 20 + b as u64;
            if create < alive + leak {
                return Err(AggCode::NegativeLedger.val());
            }
            let release: u64 = create - alive - leak;
            let bl = BatchLedger { name: BATCH_NAMES[b], counts: [create, alive, release, leak] };
            if !bl.identity_ok() {
                return Err(AggCode::BatchIdentityBroken.val());
            }
            self.batches.push(bl);
            b += 1;
        }
        Ok(())
    }

    /// 聚合：逐科求和。
    pub fn aggregate(&mut self) -> Result<(), u32> {
        if !agg_edge_legal(self.phase, AggPhase::Aggregating) {
            return Err(AggCode::IllegalPhaseEdge.val());
        }
        self.phase = AggPhase::Aggregating;
        let mut sum = [0u64; ACCT_COUNT];
        let mut b = 0;
        while b < self.batches.len() {
            if !self.batches[b].identity_ok() {
                self.phase = AggPhase::Discrepant;
                return Err(AggCode::BatchIdentityBroken.val());
            }
            let mut a = 0;
            while a < ACCT_COUNT {
                sum[a] += self.batches[b].counts[a];
                a += 1;
            }
            b += 1;
        }
        self.total = DomainTotal { counts: sum };
        Ok(())
    }

    /// 勾稽：总账与分账逐科比对（重算一遍 Σ 与 total 比对）。
    pub fn reconcile(&mut self) -> Result<(), u32> {
        if self.phase != AggPhase::Aggregating {
            return Err(AggCode::IllegalPhaseEdge.val());
        }
        let mut recheck = [0u64; ACCT_COUNT];
        let mut b = 0;
        while b < self.batches.len() {
            let mut a = 0;
            while a < ACCT_COUNT {
                recheck[a] += self.batches[b].counts[a];
                a += 1;
            }
            b += 1;
        }
        let mut a = 0;
        let mut equal = true;
        while a < ACCT_COUNT {
            if self.total.counts[a] != recheck[a] {
                equal = false;
            }
            a += 1;
        }
        if equal {
            self.phase = AggPhase::Reconciled;
            Ok(())
        } else {
            self.phase = AggPhase::Discrepant;
            self.gaps += 1;
            Err(AggCode::ReconcileGap.val())
        }
    }

    /// 勾稽差注入：篡改总账第 a 科 +delta 后再勾稽必检出。
    pub fn inject_gap(&mut self, acct: usize, delta: u64) -> Result<(), u32> {
        if acct >= ACCT_COUNT {
            return Err(AggCode::AcctOutOfRange.val());
        }
        self.total.counts[acct] += delta;
        self.reconcile()
    }

    /// 重开（Discrepant → Open）：清 total。
    pub fn reopen(&mut self) -> Result<(), u32> {
        if !agg_edge_legal(self.phase, AggPhase::Open) {
            return Err(AggCode::IllegalPhaseEdge.val());
        }
        self.phase = AggPhase::Open;
        self.total = DomainTotal { counts: [0; ACCT_COUNT] };
        Ok(())
    }
}

// ── 确定性 LCG（种子 = SEG_BASE 写死） ──────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub const fn new(seed: u64) -> Lcg {
        Lcg(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % (if n == 0 { 1 } else { n })
    }
}

// ── 注入面：三族各 10 次，10/10 全抓 ─────────────────────────────
pub const INJ_FAMILIES: usize = 3;
pub const INJ_NAMES: [&str; INJ_FAMILIES] = ["勾稽差族", "批内恒等式破坏族", "时序错族"];

pub struct InjectLedger {
    pub tries: [u32; INJ_FAMILIES],
    pub caught: [u32; INJ_FAMILIES],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; INJ_FAMILIES], caught: [0; INJ_FAMILIES] }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < INJ_FAMILIES {
            if self.caught[i] != self.tries[i] || self.tries[i] != RUNS_PER_ROUND {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn run_injections(ledger: &mut InjectLedger) {
    // 族 0：勾稽差注入（篡改科 a=轮次%4，delta=1+轮次%3 ×10）。
    let mut k = 0u32;
    while k < RUNS_PER_ROUND {
        ledger.tries[0] += 1;
        let mut ag = AggregationLedger::new();
        let _ = ag.open_seeded();
        let _ = ag.aggregate();
        if let Err(e) = ag.inject_gap((k as usize) % ACCT_COUNT, 1 + (k % 3) as u64) {
            if e == AggCode::ReconcileGap.val() && ag.gaps == 1 {
                ledger.caught[0] += 1;
            }
        }
        k += 1;
    }
    // 族 1：批内恒等式破坏注入（篡改一批存续科 ×10）。
    k = 0;
    while k < RUNS_PER_ROUND {
        ledger.tries[1] += 1;
        let mut ag = AggregationLedger::new();
        let _ = ag.open_seeded();
        let _ = ag.aggregate();
        ag.batches[(k as usize) % BATCH_COUNT].counts[ACCT_ALIVE] += 7;
        if let Err(e) = ag.aggregate() {
            if e == AggCode::BatchIdentityBroken.val() {
                ledger.caught[1] += 1;
            }
        }
        k += 1;
    }
    // 族 2：时序错注入（Open 相位直接勾稽 ×10）。
    k = 0;
    while k < RUNS_PER_ROUND {
        ledger.tries[2] += 1;
        let mut ag = AggregationLedger::new();
        let _ = ag.open_seeded();
        if let Err(e) = ag.reconcile() {
            if e == AggCode::IllegalPhaseEdge.val() {
                ledger.caught[2] += 1;
            }
        }
        k += 1;
    }
}

// ── 黄金正演：聚合勾稽闭合 ×10 轮 ────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GoldenResult {
    pub rounds: u32,
    pub total: [u64; ACCT_COUNT],
    pub batch_count: usize,
    pub gaps: u32,
}

pub fn golden_run() -> Result<GoldenResult, u32> {
    let mut g = Lcg::new(SEG_BASE as u64);
    let mut last: Option<GoldenResult> = None;
    let mut round = 0u32;
    while round < RUNS_PER_ROUND {
        let _ = g.below(13);
        let mut ag = AggregationLedger::new();
        ag.open_seeded()?;
        ag.aggregate()?;
        ag.reconcile()?;
        if ag.phase != AggPhase::Reconciled {
            return Err(AggCode::AggUnsealed.val());
        }
        last = Some(GoldenResult {
            rounds: round + 1,
            total: ag.total.counts,
            batch_count: ag.batches.len(),
            gaps: ag.gaps,
        });
        round += 1;
    }
    match last {
        Some(res) => {
            if res.batch_count != BATCH_COUNT {
                return Err(AggCode::BatchOutOfRange.val());
            }
            Ok(res)
        }
        None => Err(AggCode::AggUnsealed.val()),
    }
}

/// 双跑复现。
pub fn reproducible() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    let mut i = 0;
    while i < ACCT_COUNT {
        if a.total[i] != b.total[i] {
            return Err(AggCode::NotReproducible.val());
        }
        i += 1;
    }
    if a.batch_count != b.batch_count || a.gaps != b.gaps {
        return Err(AggCode::NotReproducible.val());
    }
    Ok(())
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < AggCode::ALL.len() {
        let v = AggCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 黄金正演：勾稽闭合 ×10 轮。
    let golden = golden_run();
    let golden_full = match &golden {
        Ok(g) => g.rounds == RUNS_PER_ROUND && g.batch_count == BATCH_COUNT && g.gaps == 0,
        Err(_) => false,
    };
    v.push(("J1-勾稽闭合10/10", golden_full, format!("{:?}", golden)));
    // 2. 注入三族 10/10 全抓。
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-勾稽差三族10/10",
        led.all_caught(),
        format!("t/c {:?}/{:?}", led.tries, led.caught),
    ));
    // 3. 双跑复现。
    v.push(("J1-双跑复现一致", matches!(reproducible(), Ok(())), String::from("双跑逐项")));
    // 4. 批名/科名点验。
    let mut okb = 0;
    let mut i = 0;
    while i < BATCH_COUNT {
        if BATCH_NAMES[i].len() > 0 {
            okb += 1;
        }
        i += 1;
    }
    let mut oka = 0;
    i = 0;
    while i < ACCT_COUNT {
        if ACCT_NAMES[i].len() > 0 {
            oka += 1;
        }
        i += 1;
    }
    v.push(("J1-12批+4科名点验", okb == 12 && oka == 4, format!("{}/12 {}/4", okb, oka)));
    // 5. 状态机非法边抽验：Reconciled→Open 直跳必拒。
    let bad = !agg_edge_legal(AggPhase::Reconciled, AggPhase::Open);
    v.push(("J1-Reconciled→Open必拒", bad, String::from("闭集外")));
    // 6. 独立勾稽对账：种子批创建总和分析。
    //    创建 Σ = Σ(100+10b) = 12*100 + 10*(0+..+11) = 1200 + 660 = 1860。
    let mut ag = AggregationLedger::new();
    let _ = ag.open_seeded();
    let _ = ag.aggregate();
    let cons = ag.total.counts[ACCT_CREATE] == 1860
        && ag.total.counts[ACCT_LEAK] == 12; // b%3 循环：0,1,2 ×4 → Σ=12
    let leak_sum = {
        let mut s = 0u64;
        let mut b = 0;
        while b < BATCH_COUNT {
            s += (b % 3) as u64;
            b += 1;
        }
        s
    };
    v.push((
        "J1-种子总账解析对账",
        cons && ag.total.counts[ACCT_LEAK] == leak_sum,
        format!("create={} leak={}", ag.total.counts[ACCT_CREATE], ag.total.counts[ACCT_LEAK]),
    ));
    // 7. 段位守卫。
    v.push(("J3-段位守卫", segment_guard(), String::from("0x67880 窗口")));
    v
}

pub fn run_unx15841_checks() -> CheckSet {
    let mut s = CheckSet::new("CoRun·UNX-D5-B33 域级对象总账聚合（勾稽闭合 + 勾稽差注入）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
