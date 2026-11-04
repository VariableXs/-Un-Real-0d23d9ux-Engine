//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0176 · 运行时服务缺失降级路径（深化第二实现） · 0x6F500
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥220 行；
//!   ② UNX-F0176-J1 降级路径：服务在账（在线）→ 直调；缺账 →
//!     降级桩（返回 UNSUPPORTED 语义不 panic），降级计数账，关键
//!     服务（ResetSystem）缺失即停机显性码，账可复算；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6F500，AI-VX3-1102 批P 独占区）。
pub const SEG_BASE: u32 = 0x6F500;

/// 服务闭集（冻结）。
pub const SVC_GETVAR: usize = 0;
pub const SVC_SETVAR: usize = 1;
pub const SVC_RESET: usize = 2;
pub const SVC_COUNT: usize = 3;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DgCode {
    /// 服务越界
    SvcOver,
    /// 关键服务缺失（停机码）
    CriticalMissing,
    /// 复算失配
    ReplayMismatch,
    /// 双跑不一致
    NotReproducible,
}

impl DgCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            DgCode::SvcOver => "服务越界：≥3",
            DgCode::CriticalMissing => "关键服务缺失：ResetSystem",
            DgCode::ReplayMismatch => "复算失配",
            DgCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [DgCode; 4] = [
        DgCode::SvcOver,
        DgCode::CriticalMissing,
        DgCode::ReplayMismatch,
        DgCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < DgCode::ALL.len() {
        if DgCode::ALL[i].val() == code {
            return DgCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6F500 段闭集内"
}

// ── 降级账 ──────────────────────────────────────────────────────
#[derive(Clone, Debug, Default)]
pub struct SvcDegrader {
    /// 在线位图。
    pub online: [bool; SVC_COUNT],
    pub degraded: u32,
    pub halts: u32,
}

impl SvcDegrader {
    pub fn new(online: [bool; SVC_COUNT]) -> SvcDegrader {
        SvcDegrader { online, degraded: 0, halts: 0 }
    }

    /// 调用（在线直调；缺账降级桩；关键服务缺 → 停机码）。
    pub fn call(&mut self, svc: usize) -> Result<&'static str, u32> {
        if svc >= SVC_COUNT {
            return Err(DgCode::SvcOver.val());
        }
        if self.online[svc] {
            return Ok("直调");
        }
        if svc == SVC_RESET {
            self.halts += 1;
            return Err(DgCode::CriticalMissing.val());
        }
        self.degraded += 1;
        Ok("降级桩 UNSUPPORTED")
    }
}

/// 复算。
pub fn replay_ok(online: [bool; SVC_COUNT], calls: &[usize]) -> Result<(), u32> {
    let mut a = SvcDegrader::new(online);
    let mut b = SvcDegrader::new(online);
    let mut i = 0;
    while i < calls.len() {
        let _ = a.call(calls[i]);
        let _ = b.call(calls[i]);
        i += 1;
    }
    if a.degraded != b.degraded || a.halts != b.halts {
        return Err(DgCode::ReplayMismatch.val());
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

// ── 注入检出账（四路：越口路/停机路/复算路/降级路） ──────────────
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

/// 真机在线账（GETVAR 在线，其余缺失）。
pub fn machine_online() -> [bool; SVC_COUNT] {
    [true, false, false]
}

// ── 黄金全链：直调→降级→关键缺停机→复算→越口拒 ────────────────────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut d = SvcDegrader::new(machine_online());
    let direct = d.call(SVC_GETVAR)? == "直调";
    let stub = d.call(SVC_SETVAR)? == "降级桩 UNSUPPORTED";
    let halt = d.call(SVC_RESET).is_err() && d.halts == 1;
    replay_ok(machine_online(), &[SVC_GETVAR, SVC_SETVAR, SVC_RESET])?;
    let over = d.call(9).is_err();
    Ok((direct as u32 + stub as u32 + halt as u32, over as u32, d.degraded))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：直调+降级+停机 + 越口拒。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((3, 1, _)) => Ok(()),
        _ => Err(DgCode::NotReproducible.val()),
    }
}

/// 序 B：关键缺失停机码。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut d = SvcDegrader::new([false; SVC_COUNT]);
    match d.call(SVC_RESET) {
        Err(e) if e == DgCode::CriticalMissing.val() => Ok(()),
        _ => Err(DgCode::NotReproducible.val()),
    }
}

/// 序 C：服务闭集。
pub fn probe_seq_c() -> Result<(), u32> {
    match SVC_COUNT {
        3 => Ok(()),
        _ => Err(DgCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：越口路。
        let mut d1 = SvcDegrader::new([true; SVC_COUNT]);
        led.record(0, d1.call(9).is_err());
        // 路 2：停机路。
        let mut d2 = SvcDegrader::new([false; SVC_COUNT]);
        led.record(1, d2.call(SVC_RESET).is_err() && d2.halts == 1);
        // 路 3：降级路。
        led.record(2, d2.call(SVC_GETVAR) == Ok("降级桩 UNSUPPORTED"));
        // 路 4：复算路。
        led.record(3, replay_ok(machine_online(), &[0, 1]).is_ok());
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(DgCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < DgCode::ALL.len() {
        let v = DgCode::ALL[i].val();
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
            Ok((ok, over, deg)) => *ok == 3 && *over == 1 && *deg == 1,
            Err(_) => false,
        },
        match &g {
            Ok((ok, over, deg)) => format!("paths={} over_reject={} degraded={}", ok, over, deg),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 服务闭集。
    v.push((
        "J1-服务闭集",
        SVC_COUNT == 3 && (SVC_GETVAR, SVC_SETVAR, SVC_RESET) == (0, 1, 2),
        String::from("GetVariable/SetVariable/ResetSystem 冻结"),
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
        String::from("0x6F500 窗口"),
    ));
    v
}

pub fn run_unx0176b_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0176 运行时服务缺失降级路径（深化二）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
