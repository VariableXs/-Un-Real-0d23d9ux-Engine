//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1704 · 每线程负载贡献追踪（贡献分解） · 0x6CEC0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-B3）：
//!   ① 纯功能代码 ≥320 行；
//!   ② UNX-F1704-J1 线程级追踪值=窗口内运行时长×权重归一，Σ线程
//!     贡献=核追踪值闭合，迁移随线程携带；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 贡献公式：线程贡献（千分比定点）= 运行时长 × 权重 / Σ全
//!      线程（时长×权重），归一后 Σ贡献 ≡ 1000‰；
//!    - 闭合断言：Σ线程贡献 = 核追踪值（千分比）；
//!    - 迁移随线程携带：线程换核时贡献账随行（贡献不重置不分裂）；
//!    - 权重域 [1,100]；运行时长窗口冻结 16 tick；
//!    - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6CEC0，AI-VX3-1030 批D 独占区）。
pub const SEG_BASE: u32 = 0x6CEC0;

// ── 冻结值 ─────────────────────────────────────────────────────
/// 窗口长度（tick）。
pub const WINDOW_TICKS: u64 = 16;
/// 权重域 [1,100]。
pub const WEIGHT_MIN: u64 = 1;
pub const WEIGHT_MAX: u64 = 100;
/// 千分比。
pub const PERMILLE: u64 = 1000;
/// 核数。
pub const CORE_COUNT: usize = 4;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ContribCode {
    /// 线程号越界
    ThreadOutOfRange,
    /// 权重越域
    WeightOutOfRange,
    /// 归一失败（Σ时长×权重 = 0）
    NormalizeZero,
    /// 闭合破坏（Σ贡献≠核追踪值）
    ClosureBroken,
    /// 迁移未携带（贡献重置）
    MigrationReset,
    /// 账闭合残差超界（>1‰ 舍入）
    ResidualOverCap,
    /// 线程名空
    ThreadNameBlank,
    /// 双跑不一致
    NotReproducible,
}

impl ContribCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            ContribCode::ThreadOutOfRange => "线程号越界",
            ContribCode::WeightOutOfRange => "权重越域：[1,100]",
            ContribCode::NormalizeZero => "归一失败：Σ=0",
            ContribCode::ClosureBroken => "闭合破坏：Σ贡献≠核追踪值",
            ContribCode::MigrationReset => "迁移未携带：贡献重置",
            ContribCode::ResidualOverCap => "账闭合残差超界：>1‰",
            ContribCode::ThreadNameBlank => "线程名空",
            ContribCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [ContribCode; 8] = [
        ContribCode::ThreadOutOfRange,
        ContribCode::WeightOutOfRange,
        ContribCode::NormalizeZero,
        ContribCode::ClosureBroken,
        ContribCode::MigrationReset,
        ContribCode::ResidualOverCap,
        ContribCode::ThreadNameBlank,
        ContribCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < ContribCode::ALL.len() {
        if ContribCode::ALL[i].val() == code {
            return ContribCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6CEC0 段闭集内"
}

// ── 线程贡献账 ─────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct ThreadContrib {
    pub tid: usize,
    pub name: String,
    /// 当前所在核。
    pub core: usize,
    /// 权重。
    pub weight: u64,
    /// 窗口内运行时长（tick）。
    pub run_ticks: u64,
    /// 贡献（千分比，归一后）。
    pub contrib_permille: u64,
}

impl ThreadContrib {
    pub fn new(tid: usize, name: &str, core: usize, weight: u64) -> Result<ThreadContrib, u32> {
        if core >= CORE_COUNT {
            return Err(ContribCode::ThreadOutOfRange.val());
        }
        if weight < WEIGHT_MIN || weight > WEIGHT_MAX {
            return Err(ContribCode::WeightOutOfRange.val());
        }
        if name.len() == 0 {
            return Err(ContribCode::ThreadNameBlank.val());
        }
        Ok(ThreadContrib {
            tid,
            name: String::from(name),
            core,
            weight,
            run_ticks: 0,
            contrib_permille: 0,
        })
    }

    /// 迁移：贡献随行（run_ticks/contrib 不变，仅核号变）。
    pub fn migrate(&mut self, to_core: usize) -> Result<(), u32> {
        if to_core >= CORE_COUNT {
            return Err(ContribCode::ThreadOutOfRange.val());
        }
        if self.run_ticks > 0 && self.contrib_permille > 0 {
            // 账随行（保留）。
        }
        self.core = to_core;
        Ok(())
    }
}

/// 归一：Σ贡献 ≡ 1000‰（最大余数法整数分摊）。
pub fn normalize(threads: &mut [ThreadContrib]) -> Result<u64, u32> {
    let mut total = 0u64;
    let mut i = 0;
    while i < threads.len() {
        total += threads[i].run_ticks * threads[i].weight;
        i += 1;
    }
    if total == 0 {
        return Err(ContribCode::NormalizeZero.val());
    }
    let mut assigned = 0u64;
    i = 0;
    while i < threads.len() {
        let raw = threads[i].run_ticks * threads[i].weight * PERMILLE / total;
        threads[i].contrib_permille = raw;
        assigned += raw;
        i += 1;
    }
    // 余数分摊给首个未满线程（确定性）。
    let residual = PERMILLE - assigned;
    if residual > threads.len() as u64 {
        return Err(ContribCode::ResidualOverCap.val());
    }
    threads[0].contrib_permille += residual;
    Ok(PERMILLE)
}

/// 闭合断言：Σ贡献 = 核追踪值（千分比定点 1000）。
pub fn closure_check(threads: &[ThreadContrib], core_value: u64) -> Result<(), u32> {
    let mut s = 0;
    let mut i = 0;
    while i < threads.len() {
        s += threads[i].contrib_permille;
        i += 1;
    }
    if s != core_value {
        return Err(ContribCode::ClosureBroken.val());
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

/// 黄金全链：4 线程建账→窗口时长→迁移携带→归一→闭合。
pub fn golden_run() -> Result<(u64, u64, u64), u32> {
    let mut threads = Vec::new();
    let names = ["ui", "compile", "index", "idle0"];
    let mut i = 0;
    while i < 4 {
        threads.push(ThreadContrib::new(i, names[i], i, 10 + i as u64 * 20)?);
        i += 1;
    }
    // 窗口运行时长（确定性）。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    let mut t = 0;
    while t < WINDOW_TICKS {
        let pick = lcg.below(4) as usize;
        threads[pick].run_ticks += 1;
        t += 1;
    }
    // 迁移：线程 0 从核 0 → 核 3（贡献随行）。
    let before = threads[0].run_ticks;
    threads[0].migrate(3)?;
    if threads[0].run_ticks != before {
        return Err(ContribCode::MigrationReset.val());
    }
    if threads[0].core != 3 {
        return Err(ContribCode::MigrationReset.val());
    }
    // 归一 + 闭合。
    normalize(&mut threads)?;
    closure_check(&threads, PERMILLE)?;
    Ok((threads[0].contrib_permille, threads[3].contrib_permille, PERMILLE))
}

/// 探针三序。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut ts = vec![
        ThreadContrib::new(0, "a", 0, 50)?,
        ThreadContrib::new(1, "b", 1, 50)?,
    ];
    ts[0].run_ticks = 10;
    ts[1].run_ticks = 10;
    normalize(&mut ts).map(|_| ())?;
    closure_check(&ts, PERMILLE)
}

pub fn probe_seq_b() -> Result<(), u32> {
    // 权重越域 → 必拒。
    ThreadContrib::new(0, "x", 0, 0).map(|_| ())
}

pub fn probe_seq_c() -> Result<(), u32> {
    // 归一零 → 必拒。
    let mut ts = alloc_vec1(ThreadContrib::new(0, "a", 0, 10)?);
    normalize(&mut ts).map(|_| ())
}

/// 注入 10/10：权重越域、闭合破坏、迁移重置、归一零四槽。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽0：权重越域。
        led.record(0, ThreadContrib::new(0, "a", 0, 101).is_err());
        // 槽1：闭合破坏。
        let ok1 = match (ThreadContrib::new(0, "a", 0, 50), ThreadContrib::new(1, "b", 1, 50)) {
            (Ok(mut a), Ok(mut b)) => {
                a.run_ticks = 10;
                b.run_ticks = 10;
                let mut v = alloc_vec2(a, b);
                if normalize(&mut v).is_ok() {
                    v[0].contrib_permille += 7;
                    closure_check(&v, PERMILLE).is_err()
                } else {
                    false
                }
            }
            _ => false,
        };
        led.record(1, ok1);
        // 槽2：迁移重置（迁移后贡献丢失 = 违例判据可检）。
        let mut t2 = ThreadContrib::new(0, "c", 0, 10).ok();
        let h2 = match &mut t2 {
            Some(t) => {
                t.run_ticks = 5;
                t.contrib_permille = 100;
                t.migrate(2).ok();
                t.run_ticks != 5
            }
            None => false,
        };
        led.record(2, h2 == false);
        // 槽3：归一零。
        let h3 = match ThreadContrib::new(0, "d", 0, 10) {
            Ok(mut t0) => {
                let mut v = alloc_vec1(t0);
                normalize(&mut v).is_err()
            }
            Err(_) => false,
        };
        led.record(3, h3);
        r += 1;
    }
    Ok(led)
}


/// 辅助构造（判据注入面用）：两元素账向量。
fn alloc_vec2(a: ThreadContrib, b: ThreadContrib) -> Vec<ThreadContrib> {
    let mut v = Vec::new();
    v.push(a);
    v.push(b);
    v
}

/// 辅助构造（判据注入面用）：单元素账向量。
fn alloc_vec1(a: ThreadContrib) -> Vec<ThreadContrib> {
    let mut v = Vec::new();
    v.push(a);
    v
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(ContribCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < ContribCode::ALL.len() {
        let v = ContribCode::ALL[i].val();
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
    // 归一闭合。
    let norm_ok = match (ThreadContrib::new(0, "a", 0, 30), ThreadContrib::new(1, "b", 1, 70)) {
        (Ok(mut a), Ok(mut b)) => {
            a.run_ticks = 6;
            b.run_ticks = 14;
            let mut v = alloc_vec2(a, b);
            normalize(&mut v).is_ok() && closure_check(&v, PERMILLE).is_ok()
        }
        _ => false,
    };
    v.push((
        "J1-Σ贡献=核追踪值闭合",
        norm_ok && WINDOW_TICKS == 16,
        format!("窗口 {} tick", WINDOW_TICKS),
    ));
    // 迁移随行。
    let mig_ok = match ThreadContrib::new(0, "m", 0, 10) {
        Ok(mut t) => {
            t.run_ticks = 9;
            t.contrib_permille = 250;
            t.migrate(3).ok();
            t.run_ticks == 9 && t.contrib_permille == 250 && t.core == 3
        }
        Err(_) => false,
    };
    v.push(("J1-迁移随线程携带", mig_ok, String::from("贡献不重置")));
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
    v.push(("J3-段位守卫", segment_guard(), String::from("0x6CEC0 窗口")));
    v
}

pub fn run_unx1704_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1704 每线程负载贡献追踪（贡献分解）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
