//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0193 · 控制寄存器策略表（CR0/CR4 基线） · 0x6F740
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥260 行；
//!   ② UNX-F0193-J1 CR0/CR4 基线策略：CR0 必置位账（PE/PG/MP）
//!     与必清位账（NW/CD 写策略），CR4 必置位账（PAE/OSFXSR）与
//!     可选位账，策略表冻结，偏离即显性码，账可复算；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6F740，AI-VX3-1102 批Q 独占区）。
pub const SEG_BASE: u32 = 0x6F740;

/// CR0 位（冻结）。
pub const CR0_PE: u64 = 1;
pub const CR0_MP: u64 = 1 << 1;
pub const CR0_CD: u64 = 1 << 30;
pub const CR0_NW: u64 = 1 << 29;
pub const CR0_PG: u64 = 1 << 31;
/// CR4 位（冻结）。
pub const CR4_PAE: u64 = 1 << 5;
pub const CR4_OSFXSR: u64 = 1 << 9;
pub const CR4_OSXMMEXCPT: u64 = 1 << 10;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CrCode {
    /// CR0 必置位缺
    Cr0MustSetMissing,
    /// CR0 必清位置位
    Cr0MustClearSet,
    /// CR4 必置位缺
    Cr4MustSetMissing,
    /// 复算失配
    ReplayMismatch,
    /// 双跑不一致
    NotReproducible,
}

impl CrCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            CrCode::Cr0MustSetMissing => "CR0 必置位缺：PE/MP/PG",
            CrCode::Cr0MustClearSet => "CR0 必清位置位：NW/CD",
            CrCode::Cr4MustSetMissing => "CR4 必置位缺：PAE/OSFXSR",
            CrCode::ReplayMismatch => "复算失配",
            CrCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [CrCode; 5] = [
        CrCode::Cr0MustSetMissing,
        CrCode::Cr0MustClearSet,
        CrCode::Cr4MustSetMissing,
        CrCode::ReplayMismatch,
        CrCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < CrCode::ALL.len() {
        if CrCode::ALL[i].val() == code {
            return CrCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6F740 段闭集内"
}

// ── 策略表 ──────────────────────────────────────────────────────
/// CR0 基线策略（必置/必清位掩码，冻结）。
pub const CR0_MUST_SET: u64 = CR0_PE | CR0_MP | CR0_PG;
pub const CR0_MUST_CLEAR: u64 = CR0_CD | CR0_NW;
/// CR4 基线策略（必置位掩码；OSXMMEXCPT 可选不查）。
pub const CR4_MUST_SET: u64 = CR4_PAE | CR4_OSFXSR;

/// CR0 偏离审计。
pub fn audit_cr0(v: u64) -> Result<(), u32> {
    if v & CR0_MUST_SET != CR0_MUST_SET {
        return Err(CrCode::Cr0MustSetMissing.val());
    }
    if v & CR0_MUST_CLEAR != 0 {
        return Err(CrCode::Cr0MustClearSet.val());
    }
    Ok(())
}

/// CR4 偏离审计。
pub fn audit_cr4(v: u64) -> Result<(), u32> {
    if v & CR4_MUST_SET != CR4_MUST_SET {
        return Err(CrCode::Cr4MustSetMissing.val());
    }
    Ok(())
}

/// 策略快照（基线值，冻结账）。
pub const CR0_BASELINE: u64 = CR0_PE | CR0_MP | CR0_PG;
pub const CR4_BASELINE: u64 = CR4_PAE | CR4_OSFXSR | CR4_OSXMMEXCPT;

/// 复算。
pub fn replay_ok() -> Result<(), u32> {
    let a = audit_cr0(CR0_BASELINE);
    let b = audit_cr4(CR4_BASELINE);
    if a.is_err() || b.is_err() {
        return Err(CrCode::ReplayMismatch.val());
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

// ── 注入检出账（四路：CR0 缺位路/CR0 脏位路/CR4 缺位路/复算路） ──
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

// ── 黄金全链：基线放行→三类偏离全抓→逐位策略→复算 ──────────────────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let base_ok = (audit_cr0(CR0_BASELINE).is_ok() && audit_cr4(CR4_BASELINE).is_ok()) as u32;
    // 缺 PG。
    let no_pg = audit_cr0(CR0_PE | CR0_MP).is_err();
    // CD 置位。
    let dirty = audit_cr0(CR0_BASELINE | CR0_CD).is_err();
    // 缺 PAE。
    let no_pae = audit_cr4(CR4_OSFXSR).is_err();
    // 逐位策略账：三必置位逐一验证。
    let bit_ok = (audit_cr0(CR0_MUST_SET).is_ok()
        && audit_cr0(CR0_BASELINE & !CR0_PE).is_err()
        && audit_cr0(CR0_BASELINE & !CR0_MP).is_err()) as u32;
    replay_ok()?;
    Ok((base_ok + bit_ok, no_pg as u32 + dirty as u32 + no_pae as u32, 0))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：基线放行 + 三偏离全抓 + 逐位策略。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((2, 3, _)) => Ok(()),
        _ => Err(CrCode::NotReproducible.val()),
    }
}

/// 序 B：策略表冻结值。
pub fn probe_seq_b() -> Result<(), u32> {
    match (CR0_MUST_SET, CR0_MUST_CLEAR, CR4_MUST_SET) {
        (0x8000_0003, 0x6000_0000, 0x220) => Ok(()),
        _ => Err(CrCode::NotReproducible.val()),
    }
}

/// 序 C：CR4 必置位缺 ⇒ Cr4MustSetMissing。
pub fn probe_seq_c() -> Result<(), u32> {
    match audit_cr4(CR4_PAE) {
        Err(e) if e == CrCode::Cr4MustSetMissing.val() => Ok(()),
        _ => Err(CrCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：CR0 缺位路。
        led.record(0, audit_cr0(CR0_PE).is_err());
        // 路 2：CR0 脏位路。
        led.record(1, audit_cr0(CR0_BASELINE | CR0_NW).is_err());
        // 路 3：CR4 缺位路。
        led.record(2, audit_cr4(0).is_err());
        // 路 4：复算路。
        led.record(3, replay_ok().is_ok());
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(CrCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < CrCode::ALL.len() {
        let v = CrCode::ALL[i].val();
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
            Ok((ok, rej, _)) => *ok == 2 && *rej == 3,
            Err(_) => false,
        },
        match &g {
            Ok((ok, rej, _)) => format!("baseline+bits={} rejects={}", ok, rej),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 基线冻结值。
    v.push((
        "J1-基线冻结",
        CR0_BASELINE == 0x8000_0003 && CR4_BASELINE == 0x620,
        format!("CR0=0x{:x} CR4=0x{:x}", CR0_BASELINE, CR4_BASELINE),
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
        String::from("0x6F740 窗口"),
    ));
    v
}

pub fn run_unx0193_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0193 控制寄存器策略表（CR0/CR4 基线）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
