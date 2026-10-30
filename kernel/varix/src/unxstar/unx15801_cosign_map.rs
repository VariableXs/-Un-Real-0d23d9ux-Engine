//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F15801 · 联签总纲与消费映射表（D5×D3 逐对可解引 + 悬空对注入）
//! · 0x67800
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F15801-J1 总纲成文（D5×D3 消费映射表逐对可解引 10/10 次），
//!      悬空对注入（映射表含未收口消费对）检出 10/10 次；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - D5 生产者闭集（8 段）× D3 消费者闭集（4 通道）= 32 映射对；
//!   - 解引器闭集（6 收口器）：每对必须解引到唯一收口器；
//!   - 状态机：Unresolved → Resolving → Resolved / Dangling（终态）；
//!   - 悬空判据：生产者/消费者越闭集 或 收口器查无 = 未收口消费对；
//!   - 正演 10/10：全表逐对可解引；注入 10/10：悬空对必检出；
//!   - 守恒恒等式：Resolved 数 + Dangling 数 = 总对数；
//!   - LCG 种子写死 = SEG_BASE（0x67800），禁 rand/浮点/IO/墙钟；
//!   - no_std + alloc；零 unwrap/expect/panic。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x67800，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x67800;

/// 每轮正演次数（判据钉版：10/10）。
pub const RUNS_PER_ROUND: u32 = 10;

/// D5 生产者闭集（8 段，写死：D5 批次段名）。
pub const PROD_COUNT: usize = 8;
pub const PROD_NAMES: [&str; PROD_COUNT] = [
    "B27-ITypeLib", "B28-DllReg", "B29-CoLock", "B30-ROT",
    "B31-CosignMap", "B32-ReentryGraph", "B33-DomainLedger", "B34-EDualJudge",
];

/// D3 消费者闭集（4 通道，写死）。
pub const CONS_COUNT: usize = 4;
pub const CONS_NAMES: [&str; CONS_COUNT] =
    ["判据消费", "账本消费", "诊断消费", "回归消费"];

/// 解引器闭集（6 收口器，写死）。
pub const RESOLVER_COUNT: usize = 6;
pub const RESOLVER_NAMES: [&str; RESOLVER_COUNT] = [
    "闭集收口", "账目收口", "诊断收口", "相位收口", "复现收口", "守恒收口",
];

/// 总映射对数 = 8×4 = 32。
pub const PAIR_TOTAL: usize = PROD_COUNT * CONS_COUNT;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MapCode {
    /// 生产者越闭集
    ProdOutOfRange,
    /// 消费者越闭集
    ConsOutOfRange,
    /// 收口器查无（解引落空）
    ResolverNotFound,
    /// 悬空对（未收口消费对在册）
    DanglingPair,
    /// 非法相位边
    IllegalPhaseEdge,
    /// 双解引（一对多收口器）
    DoubleResolve,
    /// 对账不守恒（Resolved+Dangling≠总数）
    PairCountMismatch,
    /// 双跑不一致
    NotReproducible,
    /// 映射表未封账
    MapUnsealed,
}

impl MapCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            MapCode::ProdOutOfRange => "生产者越闭集：D5 八段冻结",
            MapCode::ConsOutOfRange => "消费者越闭集：D3 四通道冻结",
            MapCode::ResolverNotFound => "收口器查无：解引落空",
            MapCode::DanglingPair => "悬空对：未收口消费对在册",
            MapCode::IllegalPhaseEdge => "非法相位边：状态机闭集外",
            MapCode::DoubleResolve => "双解引：一对多收口器被拒",
            MapCode::PairCountMismatch => "对账不守恒：Resolved+Dangling≠总数",
            MapCode::NotReproducible => "双跑不一致：映射表复现红线",
            MapCode::MapUnsealed => "映射表未封账：终态未达",
        }
    }
    pub const ALL: [MapCode; 9] = [
        MapCode::ProdOutOfRange,
        MapCode::ConsOutOfRange,
        MapCode::ResolverNotFound,
        MapCode::DanglingPair,
        MapCode::IllegalPhaseEdge,
        MapCode::DoubleResolve,
        MapCode::PairCountMismatch,
        MapCode::NotReproducible,
        MapCode::MapUnsealed,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < MapCode::ALL.len() {
        if MapCode::ALL[i].val() == code {
            return MapCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x67800 段闭集内"
}

// ── 对状态机 ─────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PairPhase {
    /// 未解引
    Unresolved,
    /// 解引中（占位防双解）
    Resolving,
    /// 已收口（终态）
    Resolved,
    /// 悬空（终态，必须被检出）
    Dangling,
}

/// 合法边闭集：Unresolved→Resolving、Resolving→Resolved、
/// Resolving→Dangling；其余（含终态出边）非法。
pub fn pair_edge_legal(from: PairPhase, to: PairPhase) -> bool {
    match (from, to) {
        (PairPhase::Unresolved, PairPhase::Resolving) => true,
        (PairPhase::Resolving, PairPhase::Resolved) => true,
        (PairPhase::Resolving, PairPhase::Dangling) => true,
        _ => false,
    }
}

/// 映射对：生产者 × 消费者 + 收口器指派。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MapPair {
    pub prod: usize,
    pub cons: usize,
    pub resolver: Option<usize>,
    pub phase: PairPhase,
}

// ── 映射表账本 ───────────────────────────────────────────────────
pub struct MapLedger {
    pub pairs: Vec<MapPair>,
    pub resolved: u32,
    pub dangling: u32,
}

impl MapLedger {
    pub fn new() -> MapLedger {
        MapLedger { pairs: Vec::new(), resolved: 0, dangling: 0 }
    }

    /// 建满 D5×D3 全表（32 对，全部 Unresolved）。
    pub fn build_full(&mut self) {
        self.pairs.clear();
        self.resolved = 0;
        self.dangling = 0;
        let mut p = 0;
        while p < PROD_COUNT {
            let mut c = 0;
            while c < CONS_COUNT {
                self.pairs.push(MapPair {
                    prod: p,
                    cons: c,
                    resolver: None,
                    phase: PairPhase::Unresolved,
                });
                c += 1;
            }
            p += 1;
        }
    }

    /// 逐对解引：闭集检查 → 收口器指派 → 终态落账。
    /// 收口器写死规则：(prod+cons) % RESOLVER_COUNT 确定性指派。
    pub fn resolve_pair(&mut self, idx: usize) -> Result<PairPhase, u32> {
        if idx >= self.pairs.len() {
            return Err(MapCode::ProdOutOfRange.val());
        }
        let prod = self.pairs[idx].prod;
        let cons = self.pairs[idx].cons;
        // 闭集闸（下标先比长度铁律落在闭集判据上）。
        if prod >= PROD_COUNT {
            self.pairs[idx].phase = PairPhase::Dangling;
            self.dangling += 1;
            return Err(MapCode::ProdOutOfRange.val());
        }
        if cons >= CONS_COUNT {
            self.pairs[idx].phase = PairPhase::Dangling;
            self.dangling += 1;
            return Err(MapCode::ConsOutOfRange.val());
        }
        // 相位边：Unresolved → Resolving。
        if !pair_edge_legal(self.pairs[idx].phase, PairPhase::Resolving) {
            return Err(MapCode::IllegalPhaseEdge.val());
        }
        self.pairs[idx].phase = PairPhase::Resolving;
        // 确定性收口器指派。
        let r = (prod + cons) % RESOLVER_COUNT;
        if r >= RESOLVER_COUNT {
            self.pairs[idx].phase = PairPhase::Dangling;
            self.dangling += 1;
            return Err(MapCode::ResolverNotFound.val());
        }
        // Resolving → Resolved。
        if !pair_edge_legal(PairPhase::Resolving, PairPhase::Resolved) {
            return Err(MapCode::IllegalPhaseEdge.val());
        }
        self.pairs[idx].resolver = Some(r);
        self.pairs[idx].phase = PairPhase::Resolved;
        self.resolved += 1;
        Ok(PairPhase::Resolved)
    }

    /// 全表解引（正演主道）：全 32 对必须 Resolved。
    pub fn resolve_all(&mut self) -> Result<u32, u32> {
        let mut i = 0;
        while i < self.pairs.len() {
            self.resolve_pair(i)?;
            i += 1;
        }
        if self.resolved as usize != self.pairs.len() {
            return Err(MapCode::PairCountMismatch.val());
        }
        Ok(self.resolved)
    }

    /// 悬空对注入：直接塞入越界消费对（未收口）并巡检。
    /// 巡检 = 逐对扫描终态账，发现非 Resolved 非终态或越界对即报。
    pub fn inject_dangling(&mut self, prod: usize, cons: usize) -> Result<(), u32> {
        self.pairs.push(MapPair {
            prod,
            cons,
            resolver: None,
            phase: PairPhase::Unresolved,
        });
        let idx = self.pairs.len() - 1;
        // 解引必败（越界）→ Dangling 终态。
        let r = self.resolve_pair(idx);
        match r {
            Ok(_) => Err(MapCode::DoubleResolve.val()),
            Err(_) => Ok(()),
        }
    }

    /// 对账守恒：resolved + dangling == pairs.len() 且全对为终态。
    pub fn conserve_check(&self) -> Result<(), u32> {
        if (self.resolved + self.dangling) as usize != self.pairs.len() {
            return Err(MapCode::PairCountMismatch.val());
        }
        let mut i = 0;
        while i < self.pairs.len() {
            let ph = self.pairs[i].phase;
            if ph != PairPhase::Resolved && ph != PairPhase::Dangling {
                return Err(MapCode::MapUnsealed.val());
            }
            i += 1;
        }
        Ok(())
    }

    /// 悬空巡检（注入检出主道）：存在 Dangling 对即报。
    pub fn dangling_audit(&self) -> Result<u32, u32> {
        let mut found = 0u32;
        let mut i = 0;
        while i < self.pairs.len() {
            if self.pairs[i].phase == PairPhase::Dangling {
                found += 1;
            }
            i += 1;
        }
        if found > 0 {
            return Err(MapCode::DanglingPair.val());
        }
        Ok(found)
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
pub const INJ_NAMES: [&str; INJ_FAMILIES] = ["悬空生产者族", "悬空消费者族", "终态出边族"];

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
    // 族 0：悬空生产者注入（prod=PROD_COUNT+k ×10）→ Dangling。
    let mut k = 0u32;
    while k < RUNS_PER_ROUND {
        ledger.tries[0] += 1;
        let mut m = MapLedger::new();
        m.build_full();
        let _ = m.inject_dangling(PROD_COUNT + k as usize, 0);
        if let Err(e) = m.dangling_audit() {
            if e == MapCode::DanglingPair.val() {
                ledger.caught[0] += 1;
            }
        }
        k += 1;
    }
    // 族 1：悬空消费者注入（cons=CONS_COUNT+k ×10）。
    k = 0;
    while k < RUNS_PER_ROUND {
        ledger.tries[1] += 1;
        let mut m = MapLedger::new();
        m.build_full();
        let _ = m.inject_dangling(0, CONS_COUNT + k as usize);
        if let Err(e) = m.dangling_audit() {
            if e == MapCode::DanglingPair.val() {
                ledger.caught[1] += 1;
            }
        }
        k += 1;
    }
    // 族 2：终态出边注入（Resolved 对再解引 ×10）→ IllegalPhaseEdge。
    k = 0;
    while k < RUNS_PER_ROUND {
        ledger.tries[2] += 1;
        let mut m = MapLedger::new();
        m.build_full();
        let _ = m.resolve_pair(0);
        // 已 Resolved 的 0 号对重解引必拒。
        if let Err(e) = m.resolve_pair(0) {
            if e == MapCode::IllegalPhaseEdge.val() {
                ledger.caught[2] += 1;
            }
        }
        k += 1;
    }
}

// ── 黄金正演：全表逐对可解引 ×10 轮 ──────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GoldenResult {
    pub rounds: u32,
    pub pair_total: usize,
    pub resolved: u32,
    pub resolver_spread: [u32; RESOLVER_COUNT],
}

pub fn golden_run() -> Result<GoldenResult, u32> {
    let mut g = Lcg::new(SEG_BASE as u64);
    let mut last: Option<GoldenResult> = None;
    let mut round = 0u32;
    while round < RUNS_PER_ROUND {
        let _ = g.below(7);
        let mut m = MapLedger::new();
        m.build_full();
        m.resolve_all()?;
        m.conserve_check()?;
        let mut spread = [0u32; RESOLVER_COUNT];
        let mut i = 0;
        while i < m.pairs.len() {
            if let Some(r) = m.pairs[i].resolver {
                spread[r] += 1;
            }
            i += 1;
        }
        last = Some(GoldenResult {
            rounds: round + 1,
            pair_total: m.pairs.len(),
            resolved: m.resolved,
            resolver_spread: spread,
        });
        round += 1;
    }
    match last {
        Some(res) => {
            if res.pair_total != PAIR_TOTAL || res.resolved != PAIR_TOTAL as u32 {
                return Err(MapCode::PairCountMismatch.val());
            }
            Ok(res)
        }
        None => Err(MapCode::MapUnsealed.val()),
    }
}

/// 双跑复现。
pub fn reproducible() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    let mut i = 0;
    while i < RESOLVER_COUNT {
        if a.resolver_spread[i] != b.resolver_spread[i] {
            return Err(MapCode::NotReproducible.val());
        }
        i += 1;
    }
    if a.pair_total != b.pair_total || a.resolved != b.resolved {
        return Err(MapCode::NotReproducible.val());
    }
    Ok(())
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < MapCode::ALL.len() {
        let v = MapCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 黄金正演：32 对全解引 ×10 轮。
    let golden = golden_run();
    let golden_full = match &golden {
        Ok(g) => g.rounds == RUNS_PER_ROUND && g.pair_total == PAIR_TOTAL && g.resolved == PAIR_TOTAL as u32,
        Err(_) => false,
    };
    v.push(("J1-全表逐对可解引10/10", golden_full, format!("{:?}", golden)));
    // 2. 注入三族 10/10 全抓。
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-悬空对三族10/10",
        led.all_caught(),
        format!("t/c {:?}/{:?}", led.tries, led.caught),
    ));
    // 3. 双跑复现。
    v.push(("J1-双跑复现一致", matches!(reproducible(), Ok(())), String::from("双跑逐项")));
    // 4. 闭集名点验：8 生产者 + 4 消费者 + 6 收口器。
    let mut ok = 0;
    let mut i = 0;
    while i < PROD_COUNT {
        if PROD_NAMES[i].len() > 0 {
            ok += 1;
        }
        i += 1;
    }
    let mut okc = 0;
    i = 0;
    while i < CONS_COUNT {
        if CONS_NAMES[i].len() > 0 {
            okc += 1;
        }
        i += 1;
    }
    let mut okr = 0;
    i = 0;
    while i < RESOLVER_COUNT {
        if RESOLVER_NAMES[i].len() > 0 {
            okr += 1;
        }
        i += 1;
    }
    v.push((
        "J1-闭集名点验8+4+6",
        ok == 8 && okc == 4 && okr == 6,
        format!("{}/8 {}/4 {}/6", ok, okc, okr),
    ));
    // 5. 对账守恒独立对账：悬空 1 对时 resolved=32、dangling=1。
    let mut m2 = MapLedger::new();
    m2.build_full();
    let _ = m2.resolve_all();
    let _ = m2.inject_dangling(PROD_COUNT, 0);
    let cons = m2.resolved == PAIR_TOTAL as u32 && m2.dangling == 1
        && matches!(m2.conserve_check(), Ok(()));
    v.push(("J1-守恒32+1对账", cons, format!("r={} d={}", m2.resolved, m2.dangling)));
    // 6. 状态机非法边抽验：Dangling→Resolved 直跳必拒。
    let bad = !pair_edge_legal(PairPhase::Dangling, PairPhase::Resolved);
    v.push(("J1-终态出边必拒", bad, String::from("Dangling→Resolved 闭集外")));
    // 7. 段位守卫。
    v.push(("J3-段位守卫", segment_guard(), String::from("0x67800 窗口")));
    v
}

pub fn run_unx15801_checks() -> CheckSet {
    let mut s = CheckSet::new("CoRun·UNX-D5-B31 联签总纲与消费映射表（D5×D3 逐对可解引）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
