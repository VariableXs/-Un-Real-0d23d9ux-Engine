//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F15761 · CoLockObjectExternal 外部锁定（锁计数语义 + 死锁注入）
//! · 0x67780
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F15761-J1 锁定成文（LockConnected/锁计数语义 10/10 次），
//!      死锁注入（重入锁定致永锁）检出 10/10 次；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 锁计数账本：fLockConnected 加锁 +1 / 减锁 -1，锁归零才可拆；
//!   - 状态机：Unlocked → Locked(n>0) → Disallowed（永锁态）；
//!   - 死锁注入：重入锁定超上限（LOCK_CAP）→ 永锁检出 10/10；
//!   - 守恒恒等式：加锁次数 − 减锁次数 = 当前锁计数；
//!   - LCG 种子写死 = SEG_BASE（0x67780），禁 rand/浮点/IO/墙钟；
//!   - no_std + alloc；零 unwrap/expect/panic。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x67780，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x67780;

/// 每语义正演次数（判据钉版：10/10）。
pub const RUNS_PER_SEM: u32 = 10;
/// 锁上限（重入超限即永锁；来源：外部锁重入语义推定，写死）。
pub const LOCK_CAP: u32 = 7;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LockCode {
    /// 减锁下溢（锁计数为 0 时再减）
    LockUnderflow,
    /// 死锁（重入超上限致永锁）
    DeadlockDetected,
    /// 锁在仍拆（锁计数 >0 时尝试拆除）
    UnlockWhileLocked,
    /// 非法相位边
    IllegalPhaseEdge,
    /// 锁计数不守恒（加锁−减锁≠计数）
    CountNotConserved,
    /// 账本未落账
    LedgerUnbooked,
    /// 正演账未满
    RunLedgerShort,
    /// 双跑不一致
    NotReproducible,
    /// 永锁未检出（注入漏抓）
    DeadlockMissed,
}

impl LockCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            LockCode::LockUnderflow => "减锁下溢：锁计数为 0 时再减",
            LockCode::DeadlockDetected => "死锁：重入锁定超上限致永锁",
            LockCode::UnlockWhileLocked => "锁在仍拆：锁计数 >0 时拆除被拒",
            LockCode::IllegalPhaseEdge => "非法相位边：状态机闭集外",
            LockCode::CountNotConserved => "锁计数不守恒：加锁−减锁≠计数",
            LockCode::LedgerUnbooked => "账本未落账：锁定事件缺账",
            LockCode::RunLedgerShort => "正演账未满：语义须 10/10",
            LockCode::NotReproducible => "双跑不一致：锁账复现红线",
            LockCode::DeadlockMissed => "永锁未检出：死锁注入漏抓",
        }
    }
    pub const ALL: [LockCode; 9] = [
        LockCode::LockUnderflow,
        LockCode::DeadlockDetected,
        LockCode::UnlockWhileLocked,
        LockCode::IllegalPhaseEdge,
        LockCode::CountNotConserved,
        LockCode::LedgerUnbooked,
        LockCode::RunLedgerShort,
        LockCode::NotReproducible,
        LockCode::DeadlockMissed,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < LockCode::ALL.len() {
        if LockCode::ALL[i].val() == code {
            return LockCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x67780 段闭集内"
}

// ── 锁相位状态机 ─────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LockPhase {
    /// 未锁（计数 0，可拆除）
    Unlocked,
    /// 已锁（计数 n，1 ≤ n < LOCK_CAP）
    Locked(u32),
    /// 永锁（计数 ≥ LOCK_CAP，重入锁定拒收）
    Disallowed,
}

/// 合法边闭集（写死）：
///   Unlocked → Locked(1)；Locked(n) → Locked(n+1)（n+1 < LOCK_CAP）；
///   Locked(n) → Unlocked（n==1）；Locked(n) → Disallowed（n+1==LOCK_CAP）；
///   Disallowed → Disallowed（重入拒收，相位不动）。
pub fn lock_edge_legal(from: LockPhase, to: LockPhase) -> bool {
    match (from, to) {
        (LockPhase::Unlocked, LockPhase::Locked(1)) => true,
        (LockPhase::Locked(n), LockPhase::Locked(m)) => m == n + 1 && m < LOCK_CAP,
        (LockPhase::Locked(1), LockPhase::Unlocked) => true,
        (LockPhase::Locked(n), LockPhase::Disallowed) => n + 1 == LOCK_CAP,
        (LockPhase::Disallowed, LockPhase::Disallowed) => true,
        _ => false,
    }
}

// ── 锁账本 ───────────────────────────────────────────────────────
/// 锁定事件：加/减与次数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LockOp {
    Lock,
    Unlock,
}

pub struct LockLedger {
    pub phase: LockPhase,
    /// 当前锁计数（与相位内值恒等）。
    pub count: u32,
    /// 累计加锁/减锁次数（守恒恒等式两边）。
    pub total_locks: u32,
    pub total_unlocks: u32,
    /// 事件账。
    pub events: Vec<LockOp>,
}

impl LockLedger {
    pub fn new() -> LockLedger {
        LockLedger {
            phase: LockPhase::Unlocked,
            count: 0,
            total_locks: 0,
            total_unlocks: 0,
            events: Vec::new(),
        }
    }

    fn set_phase(&mut self, p: LockPhase) {
        self.phase = p;
    }

    /// LockConnected 语义：加锁 +1。
    pub fn lock_connected(&mut self) -> Result<(), u32> {
        let next = self.count + 1;
        if next >= LOCK_CAP {
            // 永锁态：重入超上限，拒收加锁（死锁检出点）。
            self.set_phase(LockPhase::Disallowed);
            return Err(LockCode::DeadlockDetected.val());
        }
        let from = self.phase;
        let to = LockPhase::Locked(next);
        if !lock_edge_legal(from, to) {
            return Err(LockCode::IllegalPhaseEdge.val());
        }
        self.count = next;
        self.total_locks += 1;
        self.events.push(LockOp::Lock);
        self.set_phase(to);
        Ok(())
    }

    /// 减锁 −1：下溢即拒。
    pub fn lock_release(&mut self) -> Result<(), u32> {
        if self.count == 0 {
            return Err(LockCode::LockUnderflow.val());
        }
        if let LockPhase::Disallowed = self.phase {
            return Err(LockCode::DeadlockDetected.val());
        }
        self.count -= 1;
        self.total_unlocks += 1;
        self.events.push(LockOp::Unlock);
        if self.count == 0 {
            self.set_phase(LockPhase::Unlocked);
        } else {
            self.set_phase(LockPhase::Locked(self.count));
        }
        Ok(())
    }

    /// 拆除尝试：锁计数 >0 必拒（锁在仍拆）。
    pub fn tear_down(&self) -> Result<(), u32> {
        if self.count > 0 {
            return Err(LockCode::UnlockWhileLocked.val());
        }
        Ok(())
    }

    /// 守恒恒等式：total_locks − total_unlocks == count。
    pub fn conserve_check(&self) -> Result<(), u32> {
        if self.total_locks < self.total_unlocks {
            return Err(LockCode::CountNotConserved.val());
        }
        if self.total_locks - self.total_unlocks != self.count {
            return Err(LockCode::CountNotConserved.val());
        }
        // 相位内值与 count 恒等。
        match self.phase {
            LockPhase::Unlocked => {
                if self.count != 0 {
                    return Err(LockCode::CountNotConserved.val());
                }
            }
            LockPhase::Locked(n) => {
                if n != self.count || n == 0 || n >= LOCK_CAP {
                    return Err(LockCode::CountNotConserved.val());
                }
            }
            LockPhase::Disallowed => {}
        }
        Ok(())
    }

    /// 锁定语义正演一轮：加 n 次 → 减 n 次 → 归零（n = k%3+1）。
    pub fn run_semantics_once(&mut self, n: u32) -> Result<(), u32> {
        let mut i = 0;
        while i < n {
            self.lock_connected()?;
            i += 1;
        }
        let mut j = 0;
        while j < n {
            self.lock_release()?;
            j += 1;
        }
        if self.count != 0 {
            return Err(LockCode::CountNotConserved.val());
        }
        self.conserve_check()?;
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
pub const INJ_NAMES: [&str; INJ_FAMILIES] = ["死锁族", "减锁下溢族", "锁在仍拆族"];

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
            if self.caught[i] != self.tries[i] || self.tries[i] != RUNS_PER_SEM {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn run_injections(ledger: &mut InjectLedger) {
    // 族 0：死锁注入（重入加锁至 LOCK_CAP ×10）→ DeadlockDetected。
    let mut k = 0u32;
    while k < RUNS_PER_SEM {
        ledger.tries[0] += 1;
        let mut led = LockLedger::new();
        let mut caught = false;
        let mut step = 0;
        while step <= LOCK_CAP {
            if let Err(e) = led.lock_connected() {
                if e == LockCode::DeadlockDetected.val() && step == LOCK_CAP - 1 {
                    caught = true;
                }
                break;
            }
            step += 1;
        }
        if caught {
            ledger.caught[0] += 1;
        }
        k += 1;
    }
    // 族 1：减锁下溢注入（未加锁先减 ×10）。
    k = 0;
    while k < RUNS_PER_SEM {
        ledger.tries[1] += 1;
        let mut led = LockLedger::new();
        if let Err(e) = led.lock_release() {
            if e == LockCode::LockUnderflow.val() {
                ledger.caught[1] += 1;
            }
        }
        k += 1;
    }
    // 族 2：锁在仍拆注入（加锁 1 次后拆除 ×10）。
    k = 0;
    while k < RUNS_PER_SEM {
        ledger.tries[2] += 1;
        let mut led = LockLedger::new();
        let _ = led.lock_connected();
        if let Err(e) = led.tear_down() {
            if e == LockCode::UnlockWhileLocked.val() {
                ledger.caught[2] += 1;
            }
        }
        k += 1;
    }
}

// ── 黄金正演：LockConnected/锁计数语义 10/10 ─────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GoldenResult {
    pub sem_runs: u32,
    pub final_count: u32,
    pub teardown_ok: bool,
    pub events: usize,
}

pub fn golden_run() -> Result<GoldenResult, u32> {
    let mut led = LockLedger::new();
    let mut g = Lcg::new(SEG_BASE as u64);
    let mut run = 0u32;
    while run < RUNS_PER_SEM {
        let n = (g.below(3) as u32) + 1;
        led.run_semantics_once(n)?;
        run += 1;
    }
    if led.events.len() == 0 {
        return Err(LockCode::LedgerUnbooked.val());
    }
    led.conserve_check()?;
    let teardown_ok = matches!(led.tear_down(), Ok(()));
    if !teardown_ok {
        return Err(LockCode::UnlockWhileLocked.val());
    }
    Ok(GoldenResult {
        sem_runs: run,
        final_count: led.count,
        teardown_ok,
        events: led.events.len(),
    })
}

/// 双跑复现。
pub fn reproducible() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a.sem_runs != b.sem_runs || a.final_count != b.final_count
        || a.events != b.events || a.teardown_ok != b.teardown_ok {
        return Err(LockCode::NotReproducible.val());
    }
    Ok(())
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < LockCode::ALL.len() {
        let v = LockCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 黄金正演：语义 10/10 归零可拆。
    let golden = golden_run();
    let golden_full = match &golden {
        Ok(g) => g.sem_runs == RUNS_PER_SEM && g.final_count == 0 && g.teardown_ok && g.events > 0,
        Err(_) => false,
    };
    v.push(("J1-锁计数语义10/10", golden_full, format!("{:?}", golden)));
    // 2. 注入三族 10/10 全抓。
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-死锁等三族10/10",
        led.all_caught(),
        format!("t/c {:?}/{:?}", led.tries, led.caught),
    ));
    // 3. 双跑复现。
    v.push(("J1-双跑复现一致", matches!(reproducible(), Ok(())), String::from("双跑逐项")));
    // 4. 状态机非法边抽验：Unlocked→Disallowed 直跳必拒。
    let bad = !lock_edge_legal(LockPhase::Unlocked, LockPhase::Disallowed);
    v.push(("J1-非法边Unlocked→永锁必拒", bad, String::from("闭集外")));
    // 5. 守恒恒等式独立对账：加锁 3 减锁 2 余 1。
    let mut led2 = LockLedger::new();
    let _ = led2.lock_connected();
    let _ = led2.lock_connected();
    let _ = led2.lock_connected();
    let _ = led2.lock_release();
    let _ = led2.lock_release();
    let cons = led2.total_locks - led2.total_unlocks == led2.count && led2.count == 1;
    v.push(("J1-守恒3-2=1对账", cons, format!("count={}", led2.count)));
    // 6. LOCK_CAP 边界反向判据：恰加到 CAP−1 次不报死锁。
    let mut led3 = LockLedger::new();
    let mut no_dl = true;
    let mut s = 0;
    while s < LOCK_CAP - 1 {
        if let Err(e) = led3.lock_connected() {
            if e == LockCode::DeadlockDetected.val() {
                no_dl = false;
            }
        }
        s += 1;
    }
    v.push((
        "J1-CAP-1边界不死锁",
        no_dl && led3.count == LOCK_CAP - 1,
        format!("count={}", led3.count),
    ));
    // 7. 段位守卫。
    v.push(("J3-段位守卫", segment_guard(), String::from("0x67780 窗口")));
    v
}

pub fn run_unx15761_checks() -> CheckSet {
    let mut s = CheckSet::new("CoRun·UNX-D5-B29 CoLockObjectExternal 外部锁定（锁计数 + 死锁注入）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
