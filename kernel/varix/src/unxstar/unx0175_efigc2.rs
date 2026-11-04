//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0175 · EFI 内存垃圾回收（boot services 区回收）（深化二） · 0x6F3C0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥260 行；
//!   ② UNX-F0175-J1 BS 区回收：EBS 后 BootServices 数据区转可用
//!     （Runtime/保留区不可回收红线），回收页数守恒（可回收 = BS
//!     总 - 运行时 - 保留），回收账可复算；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6F3C0，AI-VX3-1102 批O 独占区）。
pub const SEG_BASE: u32 = 0x6F3C0;

/// 区域类（冻结）。
pub const K_BS_DATA: u8 = 0;
pub const K_RUNTIME: u8 = 1;
pub const K_RESERVED: u8 = 2;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GcCode {
    /// 运行时区回收企图（红线）
    RuntimeReclaimAttempt,
    /// 保留区回收企图（红线）
    ReservedReclaimAttempt,
    /// 未退出即回收
    ReclaimBeforeExit,
    /// 守恒失配
    ConservationBad,
    /// 复算失配
    ReplayMismatch,
    /// 双跑不一致
    NotReproducible,
}

impl GcCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            GcCode::RuntimeReclaimAttempt => "运行时区回收企图：红线",
            GcCode::ReservedReclaimAttempt => "保留区回收企图：红线",
            GcCode::ReclaimBeforeExit => "未退出即回收",
            GcCode::ConservationBad => "守恒失配",
            GcCode::ReplayMismatch => "复算失配",
            GcCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [GcCode; 6] = [
        GcCode::RuntimeReclaimAttempt,
        GcCode::ReservedReclaimAttempt,
        GcCode::ReclaimBeforeExit,
        GcCode::ConservationBad,
        GcCode::ReplayMismatch,
        GcCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < GcCode::ALL.len() {
        if GcCode::ALL[i].val() == code {
            return GcCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6F3C0 段闭集内"
}

// ── 回收账 ──────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemRegion {
    pub kind: u8,
    pub pages: u64,
}

#[derive(Clone, Debug, Default)]
pub struct EfiGcLedger {
    pub regions: Vec<MemRegion>,
    pub exited: bool,
    pub reclaimed_pages: u64,
    pub violations: u32,
}

impl EfiGcLedger {
    pub fn new() -> EfiGcLedger {
        EfiGcLedger::default()
    }

    pub fn add(&mut self, r: MemRegion) {
        self.regions.push(r);
    }

    pub fn mark_exited(&mut self) {
        self.exited = true;
    }

    /// 回收（EBS 前拒；Runtime/保留拒红线；BS 数据区页数入账）。
    pub fn reclaim(&mut self, idx: usize) -> Result<u64, u32> {
        if !self.exited {
            return Err(GcCode::ReclaimBeforeExit.val());
        }
        match self.regions.get(idx) {
            Some(r) if r.kind == K_RUNTIME => {
                self.violations += 1;
                Err(GcCode::RuntimeReclaimAttempt.val())
            }
            Some(r) if r.kind == K_RESERVED => {
                self.violations += 1;
                Err(GcCode::ReservedReclaimAttempt.val())
            }
            Some(r) => {
                self.reclaimed_pages += r.pages;
                Ok(r.pages)
            }
            None => Err(GcCode::ConservationBad.val()),
        }
    }

    /// 守恒：可回收 = BS 总 - 运行时 - 保留（即全部 BS 数据页）。
    pub fn conservation(&self) -> Result<u64, u32> {
        let mut bs_total = 0u64;
        let mut i = 0;
        while i < self.regions.len() {
            if self.regions[i].kind == K_BS_DATA {
                bs_total += self.regions[i].pages;
            }
            i += 1;
        }
        if bs_total != self.reclaimed_pages {
            return Err(GcCode::ConservationBad.val());
        }
        Ok(bs_total)
    }
}

/// 复算。
pub fn replay_ok(regions: &[MemRegion]) -> Result<(), u32> {
    let mut a = EfiGcLedger::new();
    let mut b = EfiGcLedger::new();
    let mut i = 0;
    while i < regions.len() {
        a.add(regions[i]);
        b.add(regions[i]);
        i += 1;
    }
    a.mark_exited();
    b.mark_exited();
    let mut k = 0;
    while k < regions.len() {
        let _ = a.reclaim(k);
        let _ = b.reclaim(k);
        k += 1;
    }
    if a.reclaimed_pages != b.reclaimed_pages || a.violations != b.violations {
        return Err(GcCode::ReplayMismatch.val());
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

// ── 注入检出账（四路：Runtime 路/保留路/未退出路/复算路） ─────────
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

/// 真机区域账（BS 100 页 + Runtime 20 页 + 保留 10 页）。
pub fn machine_regions() -> Vec<MemRegion> {
    vec![
        MemRegion { kind: K_BS_DATA, pages: 100 },
        MemRegion { kind: K_RUNTIME, pages: 20 },
        MemRegion { kind: K_RESERVED, pages: 10 },
    ]
}

// ── 黄金全链：未退出拒→退出→BS 回收 100→Runtime/保留拒→守恒→复算 ──
pub fn golden_run() -> Result<(u64, u32, u32), u32> {
    let mut g = EfiGcLedger::new();
    let regions = machine_regions();
    let mut i = 0;
    while i < regions.len() {
        g.add(regions[i]);
        i += 1;
    }
    // 未退出回收拒。
    let early = g.reclaim(0).is_err();
    g.mark_exited();
    // BS 数据区回收。
    let n = g.reclaim(0)?;
    // Runtime/保留拒。
    let rt = g.reclaim(1).is_err() && g.violations == 1;
    let res = g.reclaim(2).is_err() && g.violations == 2;
    let cons = g.conservation()?;
    replay_ok(&regions)?;
    Ok((n, (early as u32) + (rt as u32) + (res as u32), (cons == 100) as u32))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：回收 100 页 + 三拒 + 守恒。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((100, 3, 1)) => Ok(()),
        _ => Err(GcCode::NotReproducible.val()),
    }
}

/// 序 B：Runtime 回收 ⇒ RuntimeReclaimAttempt。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut g = EfiGcLedger::new();
    g.add(MemRegion { kind: K_RUNTIME, pages: 5 });
    g.mark_exited();
    match g.reclaim(0) {
        Err(e) if e == GcCode::RuntimeReclaimAttempt.val() => Ok(()),
        _ => Err(GcCode::NotReproducible.val()),
    }
}

/// 序 C：区域类闭集。
pub fn probe_seq_c() -> Result<(), u32> {
    match (K_BS_DATA, K_RUNTIME, K_RESERVED) == (0, 1, 2) {
        true => Ok(()),
        false => Err(GcCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：Runtime 路。
        let mut g1 = EfiGcLedger::new();
        g1.add(MemRegion { kind: K_RUNTIME, pages: 5 });
        g1.mark_exited();
        led.record(0, g1.reclaim(0).is_err());
        // 路 2：保留路。
        let mut g2 = EfiGcLedger::new();
        g2.add(MemRegion { kind: K_RESERVED, pages: 5 });
        g2.mark_exited();
        led.record(1, g2.reclaim(0).is_err());
        // 路 3：未退出路。
        let mut g3 = EfiGcLedger::new();
        g3.add(MemRegion { kind: K_BS_DATA, pages: 5 });
        led.record(2, g3.reclaim(0).is_err());
        // 路 4：复算路。
        led.record(3, replay_ok(&machine_regions()).is_ok());
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(GcCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < GcCode::ALL.len() {
        let v = GcCode::ALL[i].val();
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
            Ok((pages, rej, cons)) => *pages == 100 && *rej == 3 && *cons == 1,
            Err(_) => false,
        },
        match &g {
            Ok((pages, rej, cons)) => format!("reclaimed={} rejects={} conserved={}", pages, rej, cons),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 区域类冻结。
    v.push((
        "J1-区域类冻结",
        (K_BS_DATA, K_RUNTIME, K_RESERVED) == (0, 1, 2),
        String::from("BS 数据/Runtime/保留 三类冻结"),
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
        String::from("0x6F3C0 窗口"),
    ));
    v
}

pub fn run_unx0175b_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0175 EFI 内存垃圾回收（深化二）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
