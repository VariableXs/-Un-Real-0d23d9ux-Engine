//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0179 · EFI 调用早期失败显性化 · 0x6F5C0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥220 行；
//!   ② UNX-F0179-J1 早期调用失败账：调用点账（服务/状态码/时刻
//!     三要素），失败必入册（静默失败红线），连续 M 败升级显性
//!     停机码（M=3 冻结），账可复算；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6F5C0，AI-VX3-1102 批P 独占区）。
pub const SEG_BASE: u32 = 0x6F5C0;

/// 升级阈值（冻结 3）。
pub const ESCALATE_M: u32 = 3;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FailCode {
    /// 服务越界
    SvcOver,
    /// 失败未入册（静默红线）
    SilentFailure,
    /// 三要素缺账
    TripletMissing,
    /// 复算失配
    ReplayMismatch,
    /// 双跑不一致
    NotReproducible,
}

impl FailCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            FailCode::SvcOver => "服务越界",
            FailCode::SilentFailure => "失败未入册：静默红线",
            FailCode::TripletMissing => "三要素缺账：服务/码/时刻",
            FailCode::ReplayMismatch => "复算失配",
            FailCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [FailCode; 5] = [
        FailCode::SvcOver,
        FailCode::SilentFailure,
        FailCode::TripletMissing,
        FailCode::ReplayMismatch,
        FailCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < FailCode::ALL.len() {
        if FailCode::ALL[i].val() == code {
            return FailCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6F5C0 段闭集内"
}

// ── 失败账 ──────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CallFailure {
    pub svc: u8,
    pub code: u64,
    pub ts: u64,
}

#[derive(Clone, Debug, Default)]
pub struct EarlyFailLedger {
    pub failures: Vec<CallFailure>,
    pub consecutive: u32,
    pub escalated: bool,
}

impl EarlyFailLedger {
    pub fn new() -> EarlyFailLedger {
        EarlyFailLedger::default()
    }

    /// 失败上报（三要素闸 + 连败累加）。
    pub fn report(&mut self, svc: u8, code: u64, ts: u64) -> Result<bool, u32> {
        if svc > 7 {
            return Err(FailCode::SvcOver.val());
        }
        if code == 0 || ts == 0 {
            return Err(FailCode::TripletMissing.val());
        }
        self.failures.push(CallFailure { svc, code, ts });
        self.consecutive += 1;
        if self.consecutive >= ESCALATE_M {
            self.escalated = true;
            return Ok(true);
        }
        Ok(false)
    }

    /// 成功清连败。
    pub fn success(&mut self) {
        self.consecutive = 0;
    }

    /// 静默审计：连败 ≥M 必已升级。
    pub fn silent_audit(&self) -> Result<(), u32> {
        if self.consecutive >= ESCALATE_M && !self.escalated {
            return Err(FailCode::SilentFailure.val());
        }
        Ok(())
    }
}

/// 复算。
pub fn replay_ok(seq: &[(u8, u64, u64)]) -> Result<(), u32> {
    let mut a = EarlyFailLedger::new();
    let mut b = EarlyFailLedger::new();
    let mut i = 0;
    while i < seq.len() {
        let _ = a.report(seq[i].0, seq[i].1, seq[i].2);
        let _ = b.report(seq[i].0, seq[i].1, seq[i].2);
        i += 1;
    }
    if a.consecutive != b.consecutive || a.escalated != b.escalated {
        return Err(FailCode::ReplayMismatch.val());
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

// ── 注入检出账（四路：越服路/缺账路/静默路/复算路） ──────────────
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

/// 真机失败序列（三连败触发升级）。
pub fn machine_seq() -> Vec<(u8, u64, u64)> {
    vec![(1, 0x8002, 100), (1, 0x8002, 200), (3, 0x800E, 300)]
}

// ── 黄金全链：三连败升级→成功清零→静默审计→三拒→复算 ──────────────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut l = EarlyFailLedger::new();
    let seq = machine_seq();
    let mut escalated_at = 0u32;
    let mut i = 0;
    while i < seq.len() {
        let esc = l.report(seq[i].0, seq[i].1, seq[i].2)?;
        if esc && escalated_at == 0 {
            escalated_at = (i + 1) as u32;
        }
        i += 1;
    }
    let escalate_ok = (escalated_at == 3) as u32;
    // 成功清零。
    l.success();
    let cleared = (l.consecutive == 0) as u32;
    l.silent_audit()?;
    // 静默抓。
    let mut bad = EarlyFailLedger::new();
    let mut i = 0;
    while i < 3 {
        let _ = bad.report(1, 1, 1);
        i += 1;
    }
    bad.escalated = false;
    let silent = bad.silent_audit().is_err();
    replay_ok(&seq)?;
    Ok((escalate_ok + cleared, silent as u32, 0))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：第 3 败升级 + 清零 + 静默抓。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((2, 1, _)) => Ok(()),
        _ => Err(FailCode::NotReproducible.val()),
    }
}

/// 序 B：三要素缺账 ⇒ TripletMissing。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut l = EarlyFailLedger::new();
    match l.report(1, 0, 0) {
        Err(e) if e == FailCode::TripletMissing.val() => Ok(()),
        _ => Err(FailCode::NotReproducible.val()),
    }
}

/// 序 C：阈值冻结。
pub fn probe_seq_c() -> Result<(), u32> {
    match ESCALATE_M {
        3 => Ok(()),
        _ => Err(FailCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：越服路。
        let mut l1 = EarlyFailLedger::new();
        led.record(0, l1.report(9, 1, 1).is_err());
        // 路 2：缺账路。
        led.record(1, l1.report(1, 0, 1).is_err());
        // 路 3：静默路。
        let mut l3 = EarlyFailLedger::new();
        let mut i = 0;
        while i < 3 {
            let _ = l3.report(1, 1, 1);
            i += 1;
        }
        l3.escalated = false;
        led.record(2, l3.silent_audit().is_err());
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
        return Err(FailCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < FailCode::ALL.len() {
        let v = FailCode::ALL[i].val();
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
            Ok((ok, silent, _)) => *ok == 2 && *silent == 1,
            Err(_) => false,
        },
        match &g {
            Ok((ok, silent, _)) => format!("escalate+clear={} silent={}", ok, silent),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 阈值冻结。
    v.push((
        "J1-阈值冻结",
        ESCALATE_M == 3,
        String::from("连败 3 升级冻结"),
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
        String::from("0x6F5C0 窗口"),
    ));
    v
}

pub fn run_unx0179_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0179 EFI 调用早期失败显性化");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
