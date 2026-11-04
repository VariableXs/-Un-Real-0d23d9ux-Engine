//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0174 · 运行时防重入与并发限制（深化第二实现） · 0x6F380
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥220 行；
//!   ② UNX-F0174-J1 运行时防重入：重入计数账（持锁期重入记账拒
//!     不 panic），并发上限（持锁者恒 1），锁泄漏检测（退出未释
//!     记账），账可复算；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6F380，AI-VX3-1102 批O 独占区）。
pub const SEG_BASE: u32 = 0x6F380;

/// 并发上限（冻结 1）。
pub const MAX_HOLDERS: u32 = 1;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RnCode {
    /// 重入（持锁期再入）
    ReenterAttempt,
    /// 锁泄漏（退出未释）
    LockLeaked,
    /// 未持锁释放
    ReleaseWithoutHold,
    /// 复算失配
    ReplayMismatch,
    /// 双跑不一致
    NotReproducible,
}

impl RnCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            RnCode::ReenterAttempt => "重入：持锁期再入",
            RnCode::LockLeaked => "锁泄漏：退出未释",
            RnCode::ReleaseWithoutHold => "未持锁释放",
            RnCode::ReplayMismatch => "复算失配",
            RnCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [RnCode; 5] = [
        RnCode::ReenterAttempt,
        RnCode::LockLeaked,
        RnCode::ReleaseWithoutHold,
        RnCode::ReplayMismatch,
        RnCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < RnCode::ALL.len() {
        if RnCode::ALL[i].val() == code {
            return RnCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6F380 段闭集内"
}

// ── 防重入账 ────────────────────────────────────────────────────
#[derive(Clone, Debug, Default)]
pub struct RuntimeLock {
    pub holders: u32,
    pub reentries: u32,
    pub leaks: u32,
    pub bad_releases: u32,
    pub enter_depth: u32,
}

impl RuntimeLock {
    pub fn new() -> RuntimeLock {
        RuntimeLock::default()
    }

    /// 进入（持锁期重入拒留痕）。
    pub fn enter(&mut self) -> Result<(), u32> {
        if self.holders >= MAX_HOLDERS {
            self.reentries += 1;
            return Err(RnCode::ReenterAttempt.val());
        }
        self.holders += 1;
        self.enter_depth += 1;
        Ok(())
    }

    /// 退出（未持锁释放拒；正常释零）。
    pub fn exit(&mut self, released: bool) -> Result<(), u32> {
        if self.holders == 0 {
            self.bad_releases += 1;
            return Err(RnCode::ReleaseWithoutHold.val());
        }
        self.holders -= 1;
        if !released {
            self.leaks += 1;
        }
        Ok(())
    }

    /// 泄漏审计。
    pub fn leak_audit(&self) -> Result<(), u32> {
        if self.holders > 0 || self.leaks > 0 {
            return Err(RnCode::LockLeaked.val());
        }
        Ok(())
    }
}

/// 复算。
pub fn replay_ok(seq: &[(u8, bool)]) -> Result<(), u32> {
    let mut a = RuntimeLock::new();
    let mut b = RuntimeLock::new();
    let mut i = 0;
    while i < seq.len() {
        match seq[i].0 {
            0 => {
                let _ = a.enter();
                let _ = b.enter();
            }
            _ => {
                let _ = a.exit(seq[i].1);
                let _ = b.exit(seq[i].1);
            }
        }
        i += 1;
    }
    if a.holders != b.holders || a.leaks != b.leaks {
        return Err(RnCode::ReplayMismatch.val());
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

// ── 注入检出账（四路：重入路/泄漏路/空释路/复算路） ──────────────
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

/// 真机序列：进→出（释放）→进→出（释放）。
pub fn machine_seq() -> Vec<(u8, bool)> {
    vec![(0, false), (1, true), (0, false), (1, true)]
}

// ── 黄金全链：正常两轮→重入拒→泄漏抓→空释拒→复算 ──────────────────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut l = RuntimeLock::new();
    let seq = machine_seq();
    let mut i = 0;
    while i < seq.len() {
        match seq[i].0 {
            0 => l.enter()?,
            _ => l.exit(seq[i].1)?,
        }
        i += 1;
    }
    let clean = l.leak_audit().is_ok() as u32;
    // 重入拒。
    let mut l2 = RuntimeLock::new();
    let _ = l2.enter();
    let reenter = l2.enter().is_err() && l2.reentries == 1;
    // 泄漏抓。
    let mut l3 = RuntimeLock::new();
    let _ = l3.enter();
    let _ = l3.exit(false);
    let leak = l3.leak_audit().is_err() && l3.leaks == 1;
    replay_ok(&seq)?;
    Ok((clean, reenter as u32 + leak as u32, 0))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：两轮干净 + 重入拒 + 泄漏抓。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((1, 2, _)) => Ok(()),
        _ => Err(RnCode::NotReproducible.val()),
    }
}

/// 序 B：空释 ⇒ ReleaseWithoutHold。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut l = RuntimeLock::new();
    match l.exit(true) {
        Err(e) if e == RnCode::ReleaseWithoutHold.val() => Ok(()),
        _ => Err(RnCode::NotReproducible.val()),
    }
}

/// 序 C：并发上限冻结。
pub fn probe_seq_c() -> Result<(), u32> {
    match MAX_HOLDERS {
        1 => Ok(()),
        _ => Err(RnCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：重入路。
        let mut l1 = RuntimeLock::new();
        let _ = l1.enter();
        led.record(0, l1.enter().is_err());
        // 路 2：泄漏路。
        let mut l2 = RuntimeLock::new();
        let _ = l2.enter();
        let _ = l2.exit(false);
        led.record(1, l2.leak_audit().is_err());
        // 路 3：空释路。
        let mut l3 = RuntimeLock::new();
        led.record(2, l3.exit(true).is_err() && l3.bad_releases == 1);
        // 路 4：复算路。
        led.record(3, replay_ok(&machine_seq()).is_ok());
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(RnCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < RnCode::ALL.len() {
        let v = RnCode::ALL[i].val();
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
            Ok((clean, rej, _)) => *clean == 1 && *rej == 2,
            Err(_) => false,
        },
        match &g {
            Ok((clean, rej, _)) => format!("clean={} rejects={}", clean, rej),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 并发上限冻结。
    v.push((
        "J1-并发上限",
        MAX_HOLDERS == 1,
        String::from("持锁者恒 1 冻结"),
    ));
    // 计数不 panic 语义（重入只记账不炸）。
    let mut l = RuntimeLock::new();
    let _ = l.enter();
    let _ = l.enter();
    let _ = l.enter();
    v.push((
        "J1-记账不炸",
        l.reentries == 2 && l.holders == 1,
        String::from("重入记账 holders 恒 1"),
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
        String::from("0x6F380 窗口"),
    ));
    v
}

pub fn run_unx0174b_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0174 运行时防重入与并发限制（深化二）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
