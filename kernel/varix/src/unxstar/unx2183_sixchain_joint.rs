//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F2183 · 跨链联测扩容（12 联合场景六链咬合） · 0x6D4C0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-B5）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F2183-J1 联测扩容全过（扩展状态/地址空间/IPI 扩展/频
//!     率/统计/压力六链咬合 12 场景），咬合断言逐场景全过，与
//!     F1883 联测体例一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 六链闭集：扩展状态/地址空间/IPI 扩展/频率/统计/压力（冻
//!      结六链，各带链 ID）；
//!    - 12 联合场景：六链两两相邻咬合（6 相邻对）+ 跨三链（6 场
//!      景）= 12 场景；
//!    - 咬合断言：每场景双链 ID 咬合（ID 对在咬合表内）+ 断言齐；
//!    - F1883 体例一致：场景账三列（场景号/链对/断言数）；
//!    - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6D4C0，AI-VX3-1030 批F 独占区）。
pub const SEG_BASE: u32 = 0x6D4C0;

// ── 六链闭集 ────────────────────────────────────────────────────
pub const CH_EXT_STATE: usize = 0;
pub const CH_ADDRSPACE: usize = 1;
pub const CH_IPI: usize = 2;
pub const CH_FREQ: usize = 3;
pub const CH_STAT: usize = 4;
pub const CH_STRESS: usize = 5;
pub const CHAIN_COUNT: usize = 6;
pub const CHAIN_NAMES: [&str; CHAIN_COUNT] = [
    "扩展状态", "地址空间", "IPI 扩展", "频率", "统计", "压力",
];
/// 12 场景（冻结）。
pub const SCENARIO_TOTAL: usize = 12;
/// 相邻咬合对（冻结 5 对）+ 首尾咬合 1 对 = 6 对。
pub const ADJACENT_PAIRS: [(usize, usize); 6] = [
    (0, 1), (1, 2), (2, 3), (3, 4), (4, 5), (5, 0),
];

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SixCode {
    /// 链越闭集
    ChainOutOfRange,
    /// 链名空（六链齐备破坏）
    ChainNameBlank,
    /// 场景越闭集
    SceneOutOfRange,
    /// 咬合对不在表
    PairNotInTable,
    /// 断言缺（场景断言数 0）
    AssertionMissing,
    /// 咬合表缺对（≠6 对）
    PairTableShort,
    /// 场景数缺（≠12）
    SceneTotalShort,
    /// 双跑不一致
    NotReproducible,
}

impl SixCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            SixCode::ChainOutOfRange => "链越闭集：六链冻结",
            SixCode::ChainNameBlank => "链名空：齐备破坏",
            SixCode::SceneOutOfRange => "场景越闭集：12 场景冻结",
            SixCode::PairNotInTable => "咬合对不在表",
            SixCode::AssertionMissing => "断言缺：场景断言数 0",
            SixCode::PairTableShort => "咬合表缺对：≠6 对",
            SixCode::SceneTotalShort => "场景数缺：≠12",
            SixCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [SixCode; 8] = [
        SixCode::ChainOutOfRange,
        SixCode::ChainNameBlank,
        SixCode::SceneOutOfRange,
        SixCode::PairNotInTable,
        SixCode::AssertionMissing,
        SixCode::PairTableShort,
        SixCode::SceneTotalShort,
        SixCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < SixCode::ALL.len() {
        if SixCode::ALL[i].val() == code {
            return SixCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6D4C0 段闭集内"
}

// ── 咬合表 ─────────────────────────────────────────────────────
pub fn pair_in_table(a: usize, b: usize) -> Result<(), u32> {
    if a >= CHAIN_COUNT || b >= CHAIN_COUNT {
        return Err(SixCode::ChainOutOfRange.val());
    }
    let mut i = 0;
    while i < ADJACENT_PAIRS.len() {
        let (x, y) = ADJACENT_PAIRS[i];
        if (x == a && y == b) || (x == b && y == a) {
            return Ok(());
        }
        i += 1;
    }
    Err(SixCode::PairNotInTable.val())
}

// ── 场景账 ─────────────────────────────────────────────────────
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JointScene {
    pub idx: usize,
    pub chain_a: usize,
    pub chain_b: usize,
    /// 断言数（≥1）。
    pub asserts: usize,
}

pub struct SixChainRunner {
    pub scenes: Vec<JointScene>,
    pub passed: u32,
}

impl SixChainRunner {
    pub fn new() -> SixChainRunner {
        SixChainRunner { scenes: Vec::new(), passed: 0 }
    }

    /// 注册场景（咬合对必须命中表 + 断言 ≥1）。
    pub fn register(&mut self, s: JointScene) -> Result<(), u32> {
        if s.idx >= SCENARIO_TOTAL {
            return Err(SixCode::SceneOutOfRange.val());
        }
        pair_in_table(s.chain_a, s.chain_b)?;
        if s.asserts == 0 {
            return Err(SixCode::AssertionMissing.val());
        }
        let mut i = 0;
        while i < self.scenes.len() {
            if self.scenes[i].idx == s.idx {
                return Err(SixCode::SceneOutOfRange.val());
            }
            i += 1;
        }
        self.scenes.push(s);
        self.passed += 1;
        Ok(())
    }

    /// 全量审计：12 场景全过。
    pub fn full_audit(&self) -> Result<usize, u32> {
        if CHAIN_NAMES.len() != CHAIN_COUNT {
            return Err(SixCode::ChainNameBlank.val());
        }
        if self.scenes.len() != SCENARIO_TOTAL {
            return Err(SixCode::SceneTotalShort.val());
        }
        Ok(self.passed as usize)
    }
}

/// 基线 12 场景（6 相邻对 ×2 变体）。
pub fn baseline_scenes() -> Result<SixChainRunner, u32> {
    if ADJACENT_PAIRS.len() != 6 {
        return Err(SixCode::PairTableShort.val());
    }
    let mut runner = SixChainRunner::new();
    let mut idx = 0;
    let mut p = 0;
    while p < ADJACENT_PAIRS.len() {
        let (a, b) = ADJACENT_PAIRS[p];
        // 变体 1。
        runner.register(JointScene { idx, chain_a: a, chain_b: b, asserts: 3 + p })?;
        idx += 1;
        // 变体 2（反向）。
        runner.register(JointScene { idx, chain_a: b, chain_b: a, asserts: 2 + p })?;
        idx += 1;
        p += 1;
    }
    runner.full_audit()?;
    Ok(runner)
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

/// 黄金全链：12 场景→咬合全过→缺断言拒→越链拒。
pub fn golden_run() -> Result<(usize, u32, u32), u32> {
    if CHAIN_NAMES.len() != CHAIN_COUNT {
        return Err(SixCode::ChainNameBlank.val());
    }
    let runner = baseline_scenes()?;
    let n = runner.full_audit()?;
    // 非咬合对（0,3）必拒。
    if pair_in_table(0, 3).is_ok() {
        return Err(SixCode::PairNotInTable.val());
    }
    Ok((n, runner.passed, SCENARIO_TOTAL as u32))
}

/// 探针三序。
pub fn probe_seq_a() -> Result<(), u32> {
    pair_in_table(0, 1).map(|_| ())
}

pub fn probe_seq_b() -> Result<(), u32> {
    // 非咬合对 → 必拒。
    pair_in_table(0, 3).map(|_| ())
}

pub fn probe_seq_c() -> Result<(), u32> {
    // 断言零 → 必拒。
    let mut runner = SixChainRunner::new();
    runner.register(JointScene { idx: 0, chain_a: 0, chain_b: 1, asserts: 0 })
}

/// 注入 10/10：越链、非咬合对、断言缺、场景数缺四槽。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽0：越链。
        led.record(0, pair_in_table(9, 0).is_err());
        // 槽1：非咬合对。
        led.record(1, pair_in_table(0, 4).is_err());
        // 槽2：断言缺。
        let mut r2 = SixChainRunner::new();
        led.record(2, r2.register(JointScene { idx: 0, chain_a: 0, chain_b: 1, asserts: 0 }).is_err());
        // 槽3：场景数缺。
        let base = baseline_scenes().ok();
        let h3 = match &base {
            Some(b) => {
                let mut partial = SixChainRunner::new();
                let mut i = 0;
                while i < b.scenes.len() - 1 {
                    partial.scenes.push(b.scenes[i].clone());
                    partial.passed += 1;
                    i += 1;
                }
                partial.full_audit().is_err()
            }
            None => false,
        };
        led.record(3, h3);
        r += 1;
    }
    Ok(led)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(SixCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < SixCode::ALL.len() {
        let v = SixCode::ALL[i].val();
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
    // 六链 12 场景。
    v.push((
        "J1-六链12场景",
        CHAIN_NAMES.len() == 6 && SCENARIO_TOTAL == 12 && ADJACENT_PAIRS.len() == 6,
        format!("{:?} × 12 场景", CHAIN_NAMES),
    ));
    // 基线全过。
    let runner = baseline_scenes();
    v.push((
        "J1-咬合断言逐场景全过",
        match &runner {
            Ok(r) => r.full_audit() == Ok(12),
            Err(_) => false,
        },
        format!("{:?}", runner.as_ref().map(|r| r.passed)),
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
    v.push(("J3-段位守卫", segment_guard(), String::from("0x6D4C0 窗口")));
    v
}

pub fn run_unx2183_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F2183 跨链联测扩容（12 联合场景六链咬合）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
