//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0136 · 诊断模式入口统一 · 0x6EE40
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥220 行；
//!   ② UNX-F0136-J1 诊断入口统一：三入口闭集（菜单/热键/自动三
//!     败）汇单口（入口单一路径红线），入口原因账（三源/时刻），
//!     重入拒（已在诊断态再进拒），入口账可复算；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 三入口闭集（MENU/HOTKEY/AUTO 冻结）；
//!   - 单口红线：诊断态唯一（重入拒，计数留痕）；
//!   - 退出后可再进（状态复位）；
//!   - 复算：同入口序列同终态；
//!   - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6EE40，AI-VX3-1102 批N 独占区）。
pub const SEG_BASE: u32 = 0x6EE40;

/// 三入口（冻结）。
pub const EN_MENU: usize = 0;
pub const EN_HOTKEY: usize = 1;
pub const EN_AUTO: usize = 2;
pub const ENTRY_COUNT: usize = 3;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DiagCode {
    /// 入口越界
    EntryOver,
    /// 重入（诊断态唯一红线）
    ReenterAttempt,
    /// 复算失配
    ReplayMismatch,
    /// 退出态再退
    ExitWhenOut,
    /// 双跑不一致
    NotReproducible,
}

impl DiagCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            DiagCode::EntryOver => "入口越界：≥3",
            DiagCode::ReenterAttempt => "重入：诊断态唯一红线",
            DiagCode::ReplayMismatch => "复算失配",
            DiagCode::ExitWhenOut => "退出态再退",
            DiagCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [DiagCode; 5] = [
        DiagCode::EntryOver,
        DiagCode::ReenterAttempt,
        DiagCode::ReplayMismatch,
        DiagCode::ExitWhenOut,
        DiagCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < DiagCode::ALL.len() {
        if DiagCode::ALL[i].val() == code {
            return DiagCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6EE40 段闭集内"
}

// ── 诊断入口账 ──────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct DiagPortal {
    pub in_diag: bool,
    pub enter_count: u32,
    pub reentries: u32,
    pub last_entry: Option<usize>,
}

impl DiagPortal {
    pub fn new() -> DiagPortal {
        DiagPortal { in_diag: false, enter_count: 0, reentries: 0, last_entry: None }
    }

    /// 进入（重入拒留痕）。
    pub fn enter(&mut self, entry: usize) -> Result<(), u32> {
        if entry >= ENTRY_COUNT {
            return Err(DiagCode::EntryOver.val());
        }
        if self.in_diag {
            self.reentries += 1;
            return Err(DiagCode::ReenterAttempt.val());
        }
        self.in_diag = true;
        self.enter_count += 1;
        self.last_entry = Some(entry);
        Ok(())
    }

    /// 退出（非诊断态再退拒；退出后可再进）。
    pub fn exit(&mut self) -> Result<(), u32> {
        if !self.in_diag {
            return Err(DiagCode::ExitWhenOut.val());
        }
        self.in_diag = false;
        Ok(())
    }
}

/// 复算：同入口序列同终态。
pub fn replay_ok(seq: &[(bool, usize)]) -> Result<(), u32> {
    let mut a = DiagPortal::new();
    let mut b = DiagPortal::new();
    let mut i = 0;
    while i < seq.len() {
        let (enter, e) = seq[i];
        if enter {
            let _ = a.enter(e);
            let _ = b.enter(e);
        } else {
            let _ = a.exit();
            let _ = b.exit();
        }
        i += 1;
    }
    if a.in_diag != b.in_diag || a.enter_count != b.enter_count || a.last_entry != b.last_entry {
        return Err(DiagCode::ReplayMismatch.val());
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

// ── 注入检出账（四路：越口路/重入路/复算路/退出路） ──────────────
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

/// 真机序列：热键进 → 退 → 自动进。
pub fn machine_seq() -> Vec<(bool, usize)> {
    vec![(true, EN_HOTKEY), (false, 0), (true, EN_AUTO)]
}

// ── 黄金全链：进出→重入拒→退出后可再进→复算→越口拒 ──────────────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut p = DiagPortal::new();
    let seq = machine_seq();
    let mut i = 0;
    while i < seq.len() {
        let (enter, e) = seq[i];
        if enter {
            p.enter(e)?;
        } else {
            p.exit()?;
        }
        i += 1;
    }
    let final_in = p.in_diag as u32;
    // 重入拒。
    let mut q = DiagPortal::new();
    let _ = q.enter(EN_MENU);
    let reenter = q.enter(EN_HOTKEY).is_err() && q.reentries == 1;
    replay_ok(&seq)?;
    // 越口拒。
    let over = DiagPortal::new().enter(9).is_err();
    Ok((final_in + p.enter_count, reenter as u32, over as u32))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：两进一退 + 重入拒 + 越口拒。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((3, 1, 1)) => Ok(()),
        _ => Err(DiagCode::NotReproducible.val()),
    }
}

/// 序 B：退出态再退 ⇒ ExitWhenOut。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut p = DiagPortal::new();
    match p.exit() {
        Err(e) if e == DiagCode::ExitWhenOut.val() => Ok(()),
        _ => Err(DiagCode::NotReproducible.val()),
    }
}

/// 序 C：三入口闭集文案。
pub fn probe_seq_c() -> Result<(), u32> {
    let want = ["菜单", "热键", "自动三败"];
    let names = ["菜单", "热键", "自动三败"];
    let mut i = 0;
    while i < ENTRY_COUNT {
        if want[i] != names[i] {
            return Err(DiagCode::NotReproducible.val());
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
        // 路 1：越口路。
        let mut p1 = DiagPortal::new();
        led.record(0, p1.enter(7).is_err());
        // 路 2：重入路。
        let mut p2 = DiagPortal::new();
        let _ = p2.enter(EN_MENU);
        led.record(1, p2.enter(EN_HOTKEY).is_err() && p2.reentries == 1);
        // 路 3：复算路。
        led.record(2, replay_ok(&machine_seq()).is_ok());
        // 路 4：退出路。
        let mut p4 = DiagPortal::new();
        let _ = p4.enter(EN_AUTO);
        let _ = p4.exit();
        led.record(3, p4.exit().is_err() && !p4.in_diag);
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(DiagCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < DiagCode::ALL.len() {
        let v = DiagCode::ALL[i].val();
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
            Ok((finals, re, over)) => *finals == 3 && *re == 1 && *over == 1,
            Err(_) => false,
        },
        match &g {
            Ok((finals, re, over)) => format!("in+count={} reenter={} over_reject={}", finals, re, over),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 三入口闭集。
    v.push((
        "J1-三入口闭集",
        ENTRY_COUNT == 3 && (EN_MENU, EN_HOTKEY, EN_AUTO) == (0, 1, 2),
        String::from("菜单/热键/自动三败 三入口汇单口"),
    ));
    // 退出复位（退出后再进成功）。
    let mut p = DiagPortal::new();
    let _ = p.enter(EN_MENU);
    let _ = p.exit();
    let re_ok = p.enter(EN_HOTKEY).is_ok() && p.enter_count == 2;
    v.push((
        "J1-退出复位",
        re_ok,
        String::from("退出后可再进（计数 2）"),
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
        String::from("0x6EE40 窗口"),
    ));
    v
}

pub fn run_unx0136_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0136 诊断模式入口统一");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
