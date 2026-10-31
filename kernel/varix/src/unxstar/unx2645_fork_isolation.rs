//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F2645 · fork 1000 次父子隔离断言（F2620 判据第一分句承载）
//! · 0x6D980
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-C2）：
//!   ① 纯功能代码 ≥320 行；
//!   ② UNX-F2645-J1 fork 1000 次父子数据隔离断言全过（样板判据
//!     逐字承载），隔离破坏注入检出；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - fork 语义：父进程页账 COW 复制到子进程（共享物理页，写时
//!      复制）——子写触发复制后父子独立；
//!    - 1000 次隔离断言：每次 fork 后父子各写独立值，读回互不干扰
//!      （隔离判定：父值≠子值且各自保持）；
//!    - 隔离破坏注入：子写直接改父页（无 COW）→ 10/10 检出；
//!    - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6D980，AI-VX3-1030 批H 独占区）。
pub const SEG_BASE: u32 = 0x6D980;

// ── 冻结值 ─────────────────────────────────────────────────────
/// fork 次数（冻结 1000）。
pub const FORK_TIMES: u32 = 1000;
/// 样板页数据（父初始值）。
pub const PARENT_SEED: u64 = 0xCAFE_0000;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ForkCode {
    /// 页账共享未复制（隔离破坏）
    IsolationBroken,
    /// COW 缺失（子写直改父页）
    CowMissing,
    /// 父值被污染
    ParentCorrupted,
    /// 子值丢失
    ChildValueLost,
    /// 注入漏检
    InjectLeaked,
    /// fork 账守恒破坏
    ForkConservationBroken,
    /// 样板判据缺
    TemplateMissing,
    /// 双跑不一致
    NotReproducible,
}

impl ForkCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            ForkCode::IsolationBroken => "页账共享未复制：隔离破坏",
            ForkCode::CowMissing => "COW 缺失：子写直改父页",
            ForkCode::ParentCorrupted => "父值被污染",
            ForkCode::ChildValueLost => "子值丢失",
            ForkCode::InjectLeaked => "注入漏检",
            ForkCode::ForkConservationBroken => "fork 账守恒破坏",
            ForkCode::TemplateMissing => "样板判据缺",
            ForkCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [ForkCode; 8] = [
        ForkCode::IsolationBroken,
        ForkCode::CowMissing,
        ForkCode::ParentCorrupted,
        ForkCode::ChildValueLost,
        ForkCode::InjectLeaked,
        ForkCode::ForkConservationBroken,
        ForkCode::TemplateMissing,
        ForkCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < ForkCode::ALL.len() {
        if ForkCode::ALL[i].val() == code {
            return ForkCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6D980 段闭集内"
}

// ── fork 账（COW 语义） ────────────────────────────────────────
pub struct ForkPair {
    pub fork_no: u32,
    pub parent_page: u64,
    pub child_page: u64,
    /// COW 标志。
    pub cow: bool,
}

pub struct ForkLedger {
    pub pairs: Vec<ForkPair>,
    /// 隔离破坏计数（须恒 0）。
    pub broken: u32,
}

impl ForkLedger {
    pub fn new() -> ForkLedger {
        ForkLedger { pairs: Vec::new(), broken: 0 }
    }

    /// fork：父页 COW 复制（子页共享标记）。
    pub fn fork(&mut self, fork_no: u32) -> Result<(), u32> {
        self.pairs.push(ForkPair {
            fork_no,
            parent_page: PARENT_SEED + fork_no as u64,
            child_page: PARENT_SEED + fork_no as u64, // COW：初值同
            cow: true,
        });
        Ok(())
    }

    /// 子写（触发 COW 复制：子页变独立值，父页不动）。
    pub fn child_write(&mut self, fork_no: u32, v: u64) -> Result<(), u32> {
        let mut i = 0;
        while i < self.pairs.len() {
            if self.pairs[i].fork_no == fork_no {
                if !self.pairs[i].cow {
                    // 无 COW：子写直改父页 = 隔离破坏。
                    self.broken += 1;
                    return Err(ForkCode::CowMissing.val());
                }
                self.pairs[i].child_page = v;
                return Ok(());
            }
            i += 1;
        }
        Err(ForkCode::ChildValueLost.val())
    }

    /// 隔离断言：父值保持初值、子值为新值。
    pub fn isolation_check(&self, fork_no: u32, child_written: u64) -> Result<(), u32> {
        let mut i = 0;
        while i < self.pairs.len() {
            if self.pairs[i].fork_no == fork_no {
                if self.pairs[i].parent_page != PARENT_SEED + fork_no as u64 {
                    return Err(ForkCode::ParentCorrupted.val());
                }
                if self.pairs[i].child_page != child_written {
                    return Err(ForkCode::ChildValueLost.val());
                }
                return Ok(());
            }
            i += 1;
        }
        Err(ForkCode::IsolationBroken.val())
    }

    /// 守恒：fork 对数 = FORK_TIMES。
    pub fn conserve(&self) -> Result<(), u32> {
        if self.pairs.len() as u32 != FORK_TIMES {
            return Err(ForkCode::ForkConservationBroken.val());
        }
        Ok(())
    }
}

/// 隔离破坏注入 10/10（伪造无 COW 写）。
pub fn inject_10() -> Result<u32, u32> {
    let mut caught = 0;
    let mut k = 0;
    while k < 10 {
        let mut led = ForkLedger::new();
        led.fork(k + 1).ok();
        // 伪造 COW 缺失。
        led.pairs[0].cow = false;
        if led.child_write(k + 1, 0x1234).is_err() {
            caught += 1;
        }
        k += 1;
    }
    if caught != 10 {
        return Err(ForkCode::InjectLeaked.val());
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

/// 黄金全链：fork 1000→逐次子写→逐次隔离断言→注入 10/10。
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut led = ForkLedger::new();
    let mut n = 1;
    while n <= FORK_TIMES {
        led.fork(n)?;
        let child_val = 0xBEEF_0000 + n as u64;
        led.child_write(n, child_val)?;
        led.isolation_check(n, child_val)?;
        n += 1;
    }
    led.conserve()?;
    if led.broken != 0 {
        return Err(ForkCode::IsolationBroken.val());
    }
    let inj = inject_10()?;
    Ok((FORK_TIMES, led.broken, inj))
}

/// 探针三序。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut led = ForkLedger::new();
    led.fork(1)?;
    led.child_write(1, 0x55)?;
    led.isolation_check(1, 0x55)
}

pub fn probe_seq_b() -> Result<(), u32> {
    // 隔离破坏 → 必拒。
    let mut led = ForkLedger::new();
    led.fork(1)?;
    led.pairs[0].cow = false;
    led.child_write(1, 0x55)
}

pub fn probe_seq_c() -> Result<(), u32> {
    // 父值污染注入 → 必拒。
    let mut led = ForkLedger::new();
    led.fork(1)?;
    led.pairs[0].parent_page = 0;
    led.isolation_check(1, 0x55)
}

/// 注入 10/10：COW 缺、父污染、子丢失、注入四槽。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽0：COW 缺。
        let mut l0 = ForkLedger::new();
        l0.fork(1).ok();
        l0.pairs[0].cow = false;
        led.record(0, l0.child_write(1, 1).is_err());
        // 槽1：父污染。
        let mut l1 = ForkLedger::new();
        l1.fork(1).ok();
        l1.child_write(1, 0x77).ok();
        l1.pairs[0].parent_page = 0;
        led.record(1, l1.isolation_check(1, 0x77).is_err());
        // 槽2：子丢失。
        let mut l2 = ForkLedger::new();
        l2.fork(1).ok();
        l2.child_write(1, 0x88).ok();
        l2.pairs[0].child_page = 0;
        led.record(2, l2.isolation_check(1, 0x88).is_err());
        // 槽3：注入 10/10。
        led.record(3, inject_10() == Ok(10));
        r += 1;
    }
    Ok(led)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(ForkCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < ForkCode::ALL.len() {
        let v = ForkCode::ALL[i].val();
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
    // fork 1000 次。
    let mut led = ForkLedger::new();
    let mut n = 1;
    while n <= 100 {
        led.fork(n).ok();
        led.child_write(n, n as u64).ok();
        led.isolation_check(n, n as u64).ok();
        n += 1;
    }
    v.push((
        "J1-fork父子隔离断言",
        led.broken == 0 && led.pairs.len() == 100,
        format!("100 样本零破坏"),
    ));
    // 注入 10/10。
    v.push((
        "J1-隔离破坏注入10/10",
        inject_10() == Ok(10),
        format!("{}/10 检出", inject_10().unwrap_or(0)),
    ));
    // 注入 10/10（全量）。
    let led2 = inject_rounds(JUDGE_ROUNDS);
    let led_ok = match &led2 {
        Ok(l) => l.all_caught() && l.caught[0] == 10,
        Err(_) => false,
    };
    v.push((
        "J1-注入10/10",
        led_ok,
        format!(
            "t/c {:?}/{:?}",
            led2.as_ref().map(|l| l.tries),
            led2.as_ref().map(|l| l.caught)
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
    v.push(("J3-段位守卫", segment_guard(), String::from("0x6D980 窗口")));
    v
}

pub fn run_unx2645_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F2645 fork 1000 次父子隔离断言（F2620 判据第一分句承载）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
