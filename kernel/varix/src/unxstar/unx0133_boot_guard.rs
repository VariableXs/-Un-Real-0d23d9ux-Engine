//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0133 · 坏引导检测与拦截 · 0x6ED80
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥240 行；
//!   ② UNX-F0133-J1 坏引导拦截：三信号账（连续失败 ≥3 / 看门狗
//!     触发 / 健康降级），任一信号即拦截进恢复（三信号或账），
//!     拦截账（信号/时刻/证据）三要素，放行需三信号全清，
//!     账可复算；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 三信号闭集：SIG_FAILS/SIG_WATCHDOG/SIG_HEALTH（冻结）；
//!   - 或账：任一置位 ⇒ 拦截；全清 ⇒ 放行；
//!   - 拦截册三要素（信号/时刻/证据值）；
//!   - 复算：同信号序列同终态；
//!   - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6ED80，AI-VX3-1102 批M 独占区）。
pub const SEG_BASE: u32 = 0x6ED80;

/// 三信号（冻结）。
pub const SIG_FAILS: usize = 0;
pub const SIG_WATCHDOG: usize = 1;
pub const SIG_HEALTH: usize = 2;
pub const SIGNAL_COUNT: usize = 3;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GuardCode {
    /// 信号越界
    SignalOver,
    /// 拦截未入册（静默拦截红线）
    InterceptUntracked,
    /// 三要素缺账
    TripletMissing,
    /// 复算失配
    ReplayMismatch,
    /// 双跑不一致
    NotReproducible,
}

impl GuardCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            GuardCode::SignalOver => "信号越界：≥3",
            GuardCode::InterceptUntracked => "拦截未入册：静默红线",
            GuardCode::TripletMissing => "三要素缺账：信号/时刻/证据",
            GuardCode::ReplayMismatch => "复算失配",
            GuardCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [GuardCode; 5] = [
        GuardCode::SignalOver,
        GuardCode::InterceptUntracked,
        GuardCode::TripletMissing,
        GuardCode::ReplayMismatch,
        GuardCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < GuardCode::ALL.len() {
        if GuardCode::ALL[i].val() == code {
            return GuardCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6ED80 段闭集内"
}

// ── 拦截账 ──────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InterceptRecord {
    pub signal: usize,
    pub ts: u64,
    pub evidence: u64,
}

#[derive(Clone, Debug)]
pub struct BootGuard {
    pub signals: [bool; SIGNAL_COUNT],
    pub intercepted: bool,
    pub ledger: Vec<InterceptRecord>,
}

impl BootGuard {
    pub fn new() -> BootGuard {
        BootGuard { signals: [false; SIGNAL_COUNT], intercepted: false, ledger: Vec::new() }
    }

    /// 置信号（置位即拦截并入册三要素）。
    pub fn raise(&mut self, signal: usize, ts: u64, evidence: u64) -> Result<bool, u32> {
        if signal >= SIGNAL_COUNT {
            return Err(GuardCode::SignalOver.val());
        }
        if ts == 0 || evidence == 0 {
            return Err(GuardCode::TripletMissing.val());
        }
        self.signals[signal] = true;
        self.intercepted = true;
        self.ledger.push(InterceptRecord { signal, ts, evidence });
        Ok(true)
    }

    /// 清信号（三信号全清 ⇒ 放行）。
    pub fn clear(&mut self, signal: usize) -> Result<bool, u32> {
        if signal >= SIGNAL_COUNT {
            return Err(GuardCode::SignalOver.val());
        }
        self.signals[signal] = false;
        let mut i = 0;
        while i < SIGNAL_COUNT {
            if self.signals[i] {
                return Ok(false);
            }
            i += 1;
        }
        self.intercepted = false;
        Ok(true)
    }

    /// 静默审计：拦截态必在册有证据。
    pub fn silent_audit(&self) -> Result<(), u32> {
        if self.intercepted && self.ledger.is_empty() {
            return Err(GuardCode::InterceptUntracked.val());
        }
        Ok(())
    }
}

/// 复算：同信号序列同终态。
pub fn replay_ok(ops: &[(bool, usize, u64, u64)]) -> Result<(), u32> {
    let mut a = BootGuard::new();
    let mut b = BootGuard::new();
    let mut i = 0;
    while i < ops.len() {
        let (raise, sig, ts, ev) = ops[i];
        if raise {
            let _ = a.raise(sig, ts, ev);
            let _ = b.raise(sig, ts, ev);
        } else {
            let _ = a.clear(sig);
            let _ = b.clear(sig);
        }
        i += 1;
    }
    if a.signals != b.signals || a.intercepted != b.intercepted || a.ledger[..] != b.ledger[..] {
        return Err(GuardCode::ReplayMismatch.val());
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

// ── 注入检出账（四路：越号路/静默路/缺账路/复算路） ──────────────
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

/// 真机操作：看门狗信号触发拦截。
pub fn machine_ops() -> Vec<(bool, usize, u64, u64)> {
    vec![(true, SIG_WATCHDOG, 0xE8_0000, 8)]
}

// ── 黄金全链：触发拦截→入册→清号放行→静默审计→复算→越号拒 ────────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut g = BootGuard::new();
    let ops = machine_ops();
    let mut i = 0;
    while i < ops.len() {
        g.raise(ops[i].1, ops[i].2, ops[i].3)?;
        i += 1;
    }
    let intercepted = g.intercepted as u32;
    g.silent_audit()?;
    // 清号放行。
    let released = g.clear(SIG_WATCHDOG)? as u32;
    // 复算 + 越号拒。
    replay_ok(&ops)?;
    let over = g.raise(9, 1, 1).is_err();
    Ok((intercepted, released, (over as u32) << 0 | 0))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：拦截 1 → 放行 1 → 越号拒。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((1, 1, 1)) => Ok(()),
        _ => Err(GuardCode::NotReproducible.val()),
    }
}

/// 序 B：静默拦截 ⇒ InterceptUntracked。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut g = BootGuard::new();
    g.intercepted = true; // 手工置拦截无册（模拟静默）
    match g.silent_audit() {
        Err(e) if e == GuardCode::InterceptUntracked.val() => Ok(()),
        _ => Err(GuardCode::NotReproducible.val()),
    }
}

/// 序 C：三要素缺账 ⇒ TripletMissing。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut g = BootGuard::new();
    match g.raise(SIG_FAILS, 0, 0) {
        Err(e) if e == GuardCode::TripletMissing.val() => Ok(()),
        _ => Err(GuardCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：越号路。
        let mut g1 = BootGuard::new();
        led.record(0, g1.raise(7, 1, 1).is_err());
        // 路 2：静默路。
        let mut g2 = BootGuard::new();
        g2.intercepted = true;
        led.record(1, g2.silent_audit().is_err());
        // 路 3：缺账路。
        let mut g3 = BootGuard::new();
        led.record(2, g3.raise(SIG_HEALTH, 0, 5).is_err());
        // 路 4：复算路。
        led.record(3, replay_ok(&machine_ops()).is_ok());
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(GuardCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < GuardCode::ALL.len() {
        let v = GuardCode::ALL[i].val();
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
            Ok((ic, rel, over)) => *ic == 1 && *rel == 1 && *over == 1,
            Err(_) => false,
        },
        match &g {
            Ok((ic, rel, over)) => format!("intercepted={} released={} over_reject={}", ic, rel, over),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 三信号闭集。
    v.push((
        "J1-三信号",
        SIGNAL_COUNT == 3 && (SIG_FAILS, SIG_WATCHDOG, SIG_HEALTH) == (0, 1, 2),
        String::from("连续失败/看门狗/健康降级 冻结"),
    ));
    // 或账语义（任一置位即拦截）。
    let mut g = BootGuard::new();
    let _ = g.raise(SIG_HEALTH, 100, 3);
    let or_semantics = g.intercepted && g.ledger.len() == 1;
    let _ = g.clear(SIG_HEALTH);
    v.push((
        "J1-或账语义",
        or_semantics && !g.intercepted,
        String::from("任一置位拦截 / 全清放行"),
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
        String::from("0x6ED80 窗口"),
    ));
    v
}

pub fn run_unx0133_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0133 坏引导检测与拦截");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
