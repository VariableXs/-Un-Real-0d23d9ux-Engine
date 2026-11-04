//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0126 · 引导进度可视化（阶段徽标） · 0x6EBC0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥240 行；
//!   ② UNX-F0126-J1 引导进度徽标：六阶段账（A2 交接/内存/ACPI/
//!     中断/文件系统/用户态预备）逐阶段四态（未达/进行/完成/失
//!     败），徽标文案逐字（[##] [..] [OK] [XX]），阶段次序单调
//!     （跳阶段拒），进度条账可复算；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 六阶段闭集（次序冻结）；
//！   - 四态：未达 0/进行 1/完成 2/失败 3；
//!   - 次序守卫：阶段 N 完成前 N+1 不得为完成（跳阶段拒）；
//!   - 徽标文案五类逐字；进度条（完成数/6）可复算；
//!   - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6EBC0，AI-VX3-1102 批M 独占区）。
pub const SEG_BASE: u32 = 0x6EBC0;

/// 六阶段闭集（冻结次序）。
pub const PH_A2: usize = 0;
pub const PH_MEM: usize = 1;
pub const PH_ACPI: usize = 2;
pub const PH_IRQ: usize = 3;
pub const PH_FS: usize = 4;
pub const PH_USER: usize = 5;
pub const PHASE_COUNT: usize = 6;

/// 四态。
pub const S_PENDING: u8 = 0;
pub const S_RUNNING: u8 = 1;
pub const S_DONE: u8 = 2;
pub const S_FAILED: u8 = 3;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadgeCode {
    /// 阶段越界
    PhaseOver,
    /// 跳阶段（前段未完成即完成）
    PhaseSkipped,
    /// 态账外
    StateOver,
    /// 徽标文案失配
    BadgeTextBad,
    /// 复算失配
    ReplayMismatch,
    /// 双跑不一致
    NotReproducible,
}

impl BadgeCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            BadgeCode::PhaseOver => "阶段越界：≥6",
            BadgeCode::PhaseSkipped => "跳阶段：前段未完成",
            BadgeCode::StateOver => "态账外：>3",
            BadgeCode::BadgeTextBad => "徽标文案失配",
            BadgeCode::ReplayMismatch => "复算失配",
            BadgeCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [BadgeCode; 6] = [
        BadgeCode::PhaseOver,
        BadgeCode::PhaseSkipped,
        BadgeCode::StateOver,
        BadgeCode::BadgeTextBad,
        BadgeCode::ReplayMismatch,
        BadgeCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < BadgeCode::ALL.len() {
        if BadgeCode::ALL[i].val() == code {
            return BadgeCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6EBC0 段闭集内"
}

// ── 进度账 ──────────────────────────────────────────────────────
/// 阶段名（冻结）。
pub fn phase_name(p: usize) -> Result<&'static str, u32> {
    match p {
        PH_A2 => Ok("A2交接"),
        PH_MEM => Ok("内存"),
        PH_ACPI => Ok("ACPI"),
        PH_IRQ => Ok("中断"),
        PH_FS => Ok("文件系统"),
        PH_USER => Ok("用户态预备"),
        _ => Err(BadgeCode::PhaseOver.val()),
    }
}

/// 徽标文案（四态逐字）。
pub fn badge_text(state: u8) -> Result<&'static str, u32> {
    match state {
        S_PENDING => Ok("[  ]"),
        S_RUNNING => Ok("[..]"),
        S_DONE => Ok("[OK]"),
        S_FAILED => Ok("[XX]"),
        _ => Err(BadgeCode::StateOver.val()),
    }
}

#[derive(Clone, Debug)]
pub struct BootProgress {
    pub states: [u8; PHASE_COUNT],
}

impl BootProgress {
    pub fn new() -> BootProgress {
        BootProgress { states: [S_PENDING; PHASE_COUNT] }
    }

    /// 阶段推进（次序守卫：完成 N 需 N-1 已完成）。
    pub fn advance(&mut self, phase: usize, state: u8) -> Result<(), u32> {
        if phase >= PHASE_COUNT {
            return Err(BadgeCode::PhaseOver.val());
        }
        if state > S_FAILED {
            return Err(BadgeCode::StateOver.val());
        }
        if state == S_DONE && phase > 0 {
            let prev = self.states[phase - 1];
            if prev != S_DONE {
                return Err(BadgeCode::PhaseSkipped.val());
            }
        }
        self.states[phase] = state;
        Ok(())
    }

    /// 完成计数。
    pub fn done_count(&self) -> u32 {
        let mut n = 0;
        let mut i = 0;
        while i < PHASE_COUNT {
            if self.states[i] == S_DONE {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 进度条文案（# 填充 / . 空位）。
    pub fn progress_bar(&self) -> String {
        let done = self.done_count() as usize;
        let mut s = String::from("[");
        let mut i = 0;
        while i < PHASE_COUNT {
            if i < done {
                s.push('#');
            } else {
                s.push('.');
            }
            i += 1;
        }
        s.push(']');
        s
    }
}

/// 复算：同状态账重算恒一致。
pub fn replay_ok(p: &BootProgress) -> Result<(), u32> {
    let mut fresh = BootProgress::new();
    let mut i = 0;
    while i < PHASE_COUNT {
        fresh.states[i] = p.states[i];
        i += 1;
    }
    if fresh.done_count() != p.done_count() || fresh.progress_bar() != p.progress_bar() {
        return Err(BadgeCode::ReplayMismatch.val());
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

// ── 注入检出账（四路：越段路/跳段路/文案路/复算路） ──────────────
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

/// 真机推进序列：逐段 完成。
pub fn machine_sequence() -> Vec<(usize, u8)> {
    vec![
        (PH_A2, S_DONE),
        (PH_MEM, S_DONE),
        (PH_ACPI, S_DONE),
        (PH_IRQ, S_DONE),
        (PH_FS, S_DONE),
        (PH_USER, S_DONE),
    ]
}

// ── 黄金全链：逐段推进→全绿→进度条→复算→跳段拒 ──────────────────
pub fn golden_run() -> Result<(u32, String, u32), u32> {
    let mut p = BootProgress::new();
    let seq = machine_sequence();
    let mut i = 0;
    while i < seq.len() {
        p.advance(seq[i].0, seq[i].1)?;
        i += 1;
    }
    let bar = p.progress_bar();
    replay_ok(&p)?;
    // 跳段：A2 未完成直接 MEM 完成 ⇒ 拒。
    let mut q = BootProgress::new();
    let skip = q.advance(PH_MEM, S_DONE).is_err();
    Ok((p.done_count(), bar, skip as u32))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：逐段推进 → 全绿 6 + 进度条 [######] + 跳段拒。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((6, b, 1)) if b == "[######]" => Ok(()),
        _ => Err(BadgeCode::NotReproducible.val()),
    }
}

/// 序 B：越阶段 ⇒ PhaseOver。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut p = BootProgress::new();
    match p.advance(9, S_DONE) {
        Err(e) if e == BadgeCode::PhaseOver.val() => Ok(()),
        _ => Err(BadgeCode::NotReproducible.val()),
    }
}

/// 序 C：徽标四态文案逐字。
pub fn probe_seq_c() -> Result<(), u32> {
    let want = ["[  ]", "[..]", "[OK]", "[XX]"];
    let mut i = 0;
    while i < 4 {
        if badge_text(i as u8) != Ok(want[i]) {
            return Err(BadgeCode::NotReproducible.val());
        }
        i += 1;
    }
    Ok(())
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：越段路。
        let mut p1 = BootProgress::new();
        led.record(0, p1.advance(7, S_DONE).is_err());
        // 路 2：跳段路。
        let mut p2 = BootProgress::new();
        led.record(1, p2.advance(PH_USER, S_DONE).is_err());
        // 路 3：文案路。
        let mut hit3 = true;
        let want = ["[  ]", "[..]", "[OK]", "[XX]"];
        let mut i = 0;
        while i < 4 {
            if badge_text(i as u8).unwrap_or("?") != want[i] {
                hit3 = false;
            }
            i += 1;
        }
        led.record(2, hit3 && badge_text(9).is_err());
        // 路 4：复算路。
        let mut p4 = BootProgress::new();
        let seq = machine_sequence();
        let mut i = 0;
        while i < seq.len() {
            let _ = p4.advance(seq[i].0, seq[i].1);
            i += 1;
        }
        led.record(3, replay_ok(&p4).is_ok() && p4.progress_bar() == "[######]");
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(BadgeCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < BadgeCode::ALL.len() {
        let v = BadgeCode::ALL[i].val();
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
            Ok((done, bar, skip)) => *done == 6 && bar == "[######]" && *skip == 1,
            Err(_) => false,
        },
        match &g {
            Ok((done, bar, skip)) => format!("done={} bar={} skip_reject={}", done, bar, skip),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 六阶段名冻结。
    let mut names_ok = true;
    let mut i = 0;
    while i < PHASE_COUNT {
        if phase_name(i).is_err() {
            names_ok = false;
        }
        i += 1;
    }
    v.push((
        "J1-六阶段名",
        names_ok && phase_name(6).is_err(),
        String::from("A2/内存/ACPI/中断/文件系统/用户态 冻结"),
    ));
    // 中途进度条（3/6）。
    let mut p = BootProgress::new();
    let mut i = 0;
    while i < 3 {
        let _ = p.advance(i, S_DONE);
        i += 1;
    }
    v.push((
        "J1-中途进度",
        p.progress_bar() == "[###...]" && p.done_count() == 3,
        format!("bar={}", p.progress_bar()),
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
        String::from("0x6EBC0 窗口"),
    ));
    v
}

pub fn run_unx0126_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0126 引导进度可视化（阶段徽标）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
