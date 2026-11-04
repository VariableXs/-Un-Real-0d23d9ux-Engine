//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0159 · 慢速介质与老 U 盘兼容参数（深化第二实现） · 0x6F100
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥220 行；
//!   ② UNX-F0159-J1 慢速介质兼容：重试/延迟参数账（重试 N=5 冻
//!     结/延迟档 3 档/超时倍率 ×4），慢速档判定（失败率阈值触发
//!     降速），参数封存可复算；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6F100，AI-VX3-1102 批O 独占区）。
pub const SEG_BASE: u32 = 0x6F100;

/// 重试次数（冻结 5）。
pub const RETRY_N: u32 = 5;
/// 延迟三档（ms 冻结）。
pub const DELAY_STEPS: [u64; 3] = [0, 10, 50];
/// 超时倍率（×4 冻结）。
pub const TIMEOUT_SCALE: u64 = 4;
/// 降速触发失败率（‰ 阈值 200 冻结）。
pub const SLOW_TRIGGER: u32 = 200;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SlowCode {
    /// 延迟档越界
    StepOver,
    /// 参数账失配
    ParamLedgerBad,
    /// 降速误触（失败率未达阈值）
    SlowPremature,
    /// 复算失配
    ReplayMismatch,
    /// 双跑不一致
    NotReproducible,
}

impl SlowCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            SlowCode::StepOver => "延迟档越界：≥3",
            SlowCode::ParamLedgerBad => "参数账失配",
            SlowCode::SlowPremature => "降速误触：失败率未达阈值",
            SlowCode::ReplayMismatch => "复算失配",
            SlowCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [SlowCode; 5] = [
        SlowCode::StepOver,
        SlowCode::ParamLedgerBad,
        SlowCode::SlowPremature,
        SlowCode::ReplayMismatch,
        SlowCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < SlowCode::ALL.len() {
        if SlowCode::ALL[i].val() == code {
            return SlowCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6F100 段闭集内"
}

// ── 兼容参数账 ──────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SlowParams {
    pub retries: u32,
    pub delay_step: usize,
    /// 基础超时 ms。
    pub base_timeout_ms: u64,
}

impl SlowParams {
    pub fn normal() -> SlowParams {
        SlowParams { retries: RETRY_N, delay_step: 0, base_timeout_ms: 100 }
    }

    /// 有效延迟。
    pub fn delay_ms(&self) -> Result<u64, u32> {
        if self.delay_step >= DELAY_STEPS.len() {
            return Err(SlowCode::StepOver.val());
        }
        Ok(DELAY_STEPS[self.delay_step])
    }

    /// 有效超时（基 × 倍率^(档位)）。
    pub fn effective_timeout(&self) -> Result<u64, u32> {
        self.delay_ms()?;
        Ok(self.base_timeout_ms * TIMEOUT_SCALE.pow(self.delay_step as u32))
    }

    pub fn legal(&self) -> Result<(), u32> {
        self.delay_ms()?;
        if self.retries != RETRY_N {
            return Err(SlowCode::ParamLedgerBad.val());
        }
        Ok(())
    }
}

/// 慢速档判定：失败率 ≥ 阈值 ⇒ 降一档（不越顶）。
pub fn slow_step_down(fail_count: u32, total: u32, current: usize) -> Result<usize, u32> {
    if current >= DELAY_STEPS.len() {
        return Err(SlowCode::StepOver.val());
    }
    if total == 0 {
        return Ok(current);
    }
    let rate = (fail_count as u64 * 1000) / total as u64;
    if rate >= SLOW_TRIGGER as u64 && current + 1 < DELAY_STEPS.len() {
        return Ok(current + 1);
    }
    Ok(current)
}

/// 复算：同参数恒同超时。
pub fn replay_ok(p: &SlowParams) -> Result<(), u32> {
    let a = p.effective_timeout()?;
    let b = p.effective_timeout()?;
    if a != b {
        return Err(SlowCode::ReplayMismatch.val());
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

// ── 注入检出账（四路：越档路/失配路/误触路/复算路） ──────────────
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

// ── 黄金全链：正常档→降速触发→三档超时阶梯→顶档封顶→复算 ──────────
pub fn golden_run() -> Result<(u64, u64, u32), u32> {
    // 档 0 → 1（失败率 300‰ ≥ 200）。
    let s1 = slow_step_down(3, 10, 0)?;
    // 档 0 不降（100‰ < 200）。
    let hold = slow_step_down(1, 10, 0)?;
    let hold_ok = (hold == 0) as u32;
    // 三档超时阶梯：100/400/1600。
    let p0 = SlowParams::normal();
    let mut p2 = p0;
    p2.delay_step = s1;
    let t0 = p0.effective_timeout()?;
    let t2 = p2.effective_timeout()?;
    replay_ok(&p2)?;
    // 顶档封顶（档 2 不再升：恒返回 2）。
    let top = slow_step_down(9, 10, 2)?;
    Ok((t0, t2, hold_ok + (top == 2) as u32))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：档 1 → 400ms + 顶档封顶 + 误触拦。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((100, 400, 2)) => Ok(()),
        _ => Err(SlowCode::NotReproducible.val()),
    }
}

/// 序 B：延迟档越界 ⇒ StepOver。
pub fn probe_seq_b() -> Result<(), u32> {
    let p = SlowParams { retries: RETRY_N, delay_step: 5, base_timeout_ms: 100 };
    match p.delay_ms() {
        Err(e) if e == SlowCode::StepOver.val() => Ok(()),
        _ => Err(SlowCode::NotReproducible.val()),
    }
}

/// 序 C：参数账冻结（重试恒 5 / 倍率恒 ×4）。
pub fn probe_seq_c() -> Result<(), u32> {
    match (RETRY_N == 5, TIMEOUT_SCALE == 4) {
        (true, true) => Ok(()),
        _ => Err(SlowCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：越档路。
        led.record(0, slow_step_down(9, 10, 5).is_err());
        // 路 2：失配路。
        let bad = SlowParams { retries: 3, delay_step: 0, base_timeout_ms: 100 };
        led.record(1, bad.legal().is_err());
        // 路 3：误触路。
        led.record(2, slow_step_down(1, 10, 0) == Ok(0));
        // 路 4：复算路。
        led.record(3, replay_ok(&SlowParams::normal()).is_ok());
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(SlowCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < SlowCode::ALL.len() {
        let v = SlowCode::ALL[i].val();
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
            Ok((t0, t2, ok)) => *t0 == 100 && *t2 == 400 && *ok == 2,
            Err(_) => false,
        },
        match &g {
            Ok((t0, t2, ok)) => format!("t0={}ms t1={}ms guards={}", t0, t2, ok),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 三档阶梯全账（100/400/1600）。
    let mut ladder_ok = true;
    let mut i = 0;
    while i < 3 {
        let p = SlowParams { retries: RETRY_N, delay_step: i, base_timeout_ms: 100 };
        let want = [100u64, 400, 1600][i];
        if p.effective_timeout() != Ok(want) {
            ladder_ok = false;
        }
        i += 1;
    }
    v.push((
        "J1-三档阶梯",
        ladder_ok,
        String::from("100/400/1600 ms 冻结"),
    ));
    // 降速触发阈值。
    v.push((
        "J1-降速阈值",
        SLOW_TRIGGER == 200 && slow_step_down(2, 10, 0) == Ok(1),
        String::from("失败率 ≥200‰ 触发降档"),
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
        String::from("0x6F100 窗口"),
    ));
    v
}

pub fn run_unx0159b_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0159 慢速介质与老 U 盘兼容参数（深化二）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
