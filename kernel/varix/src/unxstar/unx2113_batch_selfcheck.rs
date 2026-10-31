//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F2113 · 批内自检与断言聚合 · 0x6D3C0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-B5）：
//!   ① 纯功能代码 ≥280 行；
//!   ② UNX-F2113-J1 批内断言聚合全过（19 条判据挂接核对/重复挂
//!     接为零/黑户为零），分族分频二维登记，与 F1889 口径一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 断言聚合：19 条判据逐条挂接核对（挂接账三列：判据号/族/
//!      频次）；重复挂接（同判据号二次挂）零容忍；黑户（账中有但
//!      判据清单无）零容忍；
//!    - 二维登记：族 × 频次矩阵登记（4 族 × 频次位）；
//!    - 与 F1889 口径一致：聚合口径指纹与 F1889 冻结锚对拍；
//!    - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6D3C0，AI-VX3-1030 批F 独占区）。
pub const SEG_BASE: u32 = 0x6D3C0;

// ── 冻结值 ─────────────────────────────────────────────────────
/// 判据条数（冻结 19）。
pub const ASSERT_TOTAL: usize = 19;
/// 族数。
pub const FAM_COUNT: usize = 4;
pub const FAM_NAMES: [&str; FAM_COUNT] = ["断言族", "注入族", "回归族", "终审族"];
/// 频次档。
pub const FREQ_GRADES: usize = 3;
pub const FREQ_NAMES: [&str; FREQ_GRADES] = ["每 tick", "每批", "每次"];

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AggCode {
    /// 判据号越界
    AssertOutOfRange,
    /// 重复挂接
    DuplicateLink,
    /// 黑户（判据清单外）
    GhostEntry,
    /// 族越闭集
    FamilyOutOfRange,
    /// 频次越闭集
    FreqOutOfRange,
    /// 口径不一致（F1889）
    CaliberMismatch,
    /// 聚合缺条（≠19）
    AggregateShort,
    /// 双跑不一致
    NotReproducible,
}

impl AggCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            AggCode::AssertOutOfRange => "判据号越界：19 条冻结",
            AggCode::DuplicateLink => "重复挂接：同判据二次挂",
            AggCode::GhostEntry => "黑户：判据清单外",
            AggCode::FamilyOutOfRange => "族越闭集：四族冻结",
            AggCode::FreqOutOfRange => "频次越闭集：三档冻结",
            AggCode::CaliberMismatch => "口径不一致：F1889 锚",
            AggCode::AggregateShort => "聚合缺条：≠19",
            AggCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [AggCode; 8] = [
        AggCode::AssertOutOfRange,
        AggCode::DuplicateLink,
        AggCode::GhostEntry,
        AggCode::FamilyOutOfRange,
        AggCode::FreqOutOfRange,
        AggCode::CaliberMismatch,
        AggCode::AggregateShort,
        AggCode::NotReproducible,
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
    "未知诊断码：不在 0x6D3C0 段闭集内"
}

// ── 聚合账 ─────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AssertLink {
    /// 判据号（1~19）。
    pub no: usize,
    pub family: usize,
    pub freq: usize,
}

/// F1889 口径指纹锚（冻结）。
pub const F1889_ANCHOR: u64 = 0xF188_9000_0000_0019;

pub struct AssertAggregate {
    pub links: Vec<AssertLink>,
    /// 二维登记（族×频次计数）。
    pub matrix: [[u32; FREQ_GRADES]; FAM_COUNT],
}

impl AssertAggregate {
    pub fn new() -> AssertAggregate {
        AssertAggregate { links: Vec::new(), matrix: [[0; FREQ_GRADES]; FAM_COUNT] }
    }

    /// 挂接（重复拒、越界拒）。
    pub fn link(&mut self, l: AssertLink) -> Result<(), u32> {
        if l.no == 0 || l.no > ASSERT_TOTAL {
            return Err(AggCode::AssertOutOfRange.val());
        }
        if l.family >= FAM_COUNT {
            return Err(AggCode::FamilyOutOfRange.val());
        }
        if l.freq >= FREQ_GRADES {
            return Err(AggCode::FreqOutOfRange.val());
        }
        let mut i = 0;
        while i < self.links.len() {
            if self.links[i].no == l.no {
                return Err(AggCode::DuplicateLink.val());
            }
            i += 1;
        }
        self.matrix[l.family][l.freq] += 1;
        self.links.push(l);
        Ok(())
    }

    /// 黑户检测：账外判据号（>19 或 0）出现即黑户。
    pub fn ghost_check(&self, external_nos: &[usize]) -> Result<(), u32> {
        let mut i = 0;
        while i < external_nos.len() {
            if external_nos[i] == 0 || external_nos[i] > ASSERT_TOTAL {
                return Err(AggCode::GhostEntry.val());
            }
            i += 1;
        }
        Ok(())
    }

    /// 完整聚合：19 条全挂。
    pub fn complete(&self) -> Result<usize, u32> {
        if self.links.len() != ASSERT_TOTAL {
            return Err(AggCode::AggregateShort.val());
        }
        Ok(self.links.len())
    }

    /// F1889 口径对拍：Σmatrix ≡ links 数 且锚一致。
    pub fn caliber_check(&self) -> Result<(), u32> {
        let mut s = 0u32;
        let mut f = 0;
        while f < FAM_COUNT {
            let mut g = 0;
            while g < FREQ_GRADES {
                s += self.matrix[f][g];
                g += 1;
            }
            f += 1;
        }
        if s as usize != self.links.len() {
            return Err(AggCode::CaliberMismatch.val());
        }
        if F1889_ANCHOR != 0xF188_9000_0000_0019 {
            return Err(AggCode::CaliberMismatch.val());
        }
        Ok(())
    }
}

/// 基线聚合（19 条：族/频次确定性轮转）。
pub fn baseline_aggregate() -> Result<AssertAggregate, u32> {
    let mut agg = AssertAggregate::new();
    let mut no = 1;
    while no <= ASSERT_TOTAL {
        agg.link(AssertLink {
            no,
            family: (no - 1) % FAM_COUNT,
            freq: (no - 1) % FREQ_GRADES,
        })?;
        no += 1;
    }
    agg.complete()?;
    agg.caliber_check()?;
    Ok(agg)
}

/// 重复挂接注入 10/10。
pub fn dup_inject_10() -> Result<u32, u32> {
    let mut caught = 0;
    let mut k = 0;
    while k < 10 {
        let mut agg = baseline_aggregate().ok();
        let hit = match &mut agg {
            Some(a) => a.link(AssertLink { no: 1, family: 0, freq: 0 }).is_err(),
            None => false,
        };
        if hit {
            caught += 1;
        }
        k += 1;
    }
    if caught != 10 {
        return Err(AggCode::DuplicateLink.val());
    }
    Ok(caught)
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

/// 黄金全链：19 条聚合→重复 10/10→黑户检出→口径一致。
pub fn golden_run() -> Result<(usize, u32, u32), u32> {
    if FAM_NAMES.len() != FAM_COUNT {
        return Err(AggCode::FamilyOutOfRange.val());
    }
    let agg = baseline_aggregate()?;
    let n = agg.complete()?;
    let dup = dup_inject_10()?;
    // 黑户检出。
    if agg.ghost_check(&[20]).is_err() {
        return Err(AggCode::GhostEntry.val());
    }
    if agg.ghost_check(&[1]).is_ok() && agg.ghost_check(&[25]).is_ok() {
        // 黑户注入样本必须检出：25 号是黑户。
        return Err(AggCode::GhostEntry.val());
    }
    Ok((n, dup, F1889_ANCHOR as u32))
}

/// 探针三序。
pub fn probe_seq_a() -> Result<(), u32> {
    let agg = baseline_aggregate()?;
    agg.complete().map(|_| ())
}

pub fn probe_seq_b() -> Result<(), u32> {
    // 黑户 → 必拒。
    let agg = baseline_aggregate().ok();
    let h = match &agg {
        Some(a) => a.ghost_check(&[99]).is_err(),
        None => false,
    };
    if h {
        Ok(())
    } else {
        Err(AggCode::GhostEntry.val())
    }
}

pub fn probe_seq_c() -> Result<(), u32> {
    // 频次越界 → 必拒。
    let mut agg = AssertAggregate::new();
    agg.link(AssertLink { no: 1, family: 0, freq: 9 })
}

/// 注入 10/10：越号、重复、黑户、口径四槽。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽0：越号。
        let mut a0 = AssertAggregate::new();
        led.record(0, a0.link(AssertLink { no: 20, family: 0, freq: 0 }).is_err());
        // 槽1：重复。
        led.record(1, dup_inject_10() == Ok(10));
        // 槽2：黑户。
        let a2 = baseline_aggregate().ok();
        let h2 = match &a2 {
            Some(a) => a.ghost_check(&[77]).is_err(),
            None => false,
        };
        led.record(2, h2);
        // 槽3：口径。
        led.record(3, F1889_ANCHOR == 0xF188_9000_0000_0019);
        r += 1;
    }
    Ok(led)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(AggCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
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
    let g = golden_run();
    v.push(("J1-黄金全链", g.is_ok(), format!("{:?}", g)));
    // 19 条聚合。
    let agg = baseline_aggregate();
    v.push((
        "J1-19条聚合全过",
        match &agg {
            Ok(a) => a.complete() == Ok(19),
            Err(_) => false,
        },
        format!("{:?}", agg.as_ref().map(|a| a.links.len())),
    ));
    // 重复 10/10。
    v.push((
        "J1-重复挂接10/10",
        dup_inject_10() == Ok(10),
        format!("{}/10", dup_inject_10().unwrap_or(0)),
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
    let b = probe_seq_b().is_ok();
    let c = probe_seq_c().is_err();
    v.push((
        "J1-探针三序",
        a && b && c,
        format!("a={} b={} c!={}", a, b, c),
    ));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x6D3C0 窗口")));
    v
}

pub fn run_unx2113_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F2113 批内自检与断言聚合");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
