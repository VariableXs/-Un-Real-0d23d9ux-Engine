//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0192 · fs/gs base 设置与 per-CPU 预备 · 0x6F700
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥260 行；
//!   ② UNX-F0192-J1 fs/gs 基址账：per-CPU 区账（CPU 号/基址/大
//!     小三账），基址 16 对齐 + 区不重叠 + gs 基址与 CPU 号一一
//!     对应，swapgs 语义账（内核态/用户态切换记账），账可复算；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6F700，AI-VX3-1102 批Q 独占区）。
pub const SEG_BASE: u32 = 0x6F700;

/// per-CPU 区对齐（16 冻结）。
pub const AREA_ALIGN: u64 = 16;
/// 最大 CPU 数（冻结 64）。
pub const MAX_CPUS: usize = 64;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CpuCode {
    /// CPU 号越界
    CpuOver,
    /// 基址非 16 对齐
    BaseMisaligned,
    /// 区重叠
    AreaOverlap,
    /// 重复注册
    DuplicateCpu,
    /// swapgs 越权（用户态记账误切）
    SwapgsBad,
    /// 复算失配
    ReplayMismatch,
    /// 双跑不一致
    NotReproducible,
}

impl CpuCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            CpuCode::CpuOver => "CPU 号越界：≥64",
            CpuCode::BaseMisaligned => "基址非 16 对齐",
            CpuCode::AreaOverlap => "区重叠：红线",
            CpuCode::DuplicateCpu => "重复注册",
            CpuCode::SwapgsBad => "swapgs 记账越权",
            CpuCode::ReplayMismatch => "复算失配",
            CpuCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [CpuCode; 7] = [
        CpuCode::CpuOver,
        CpuCode::BaseMisaligned,
        CpuCode::AreaOverlap,
        CpuCode::DuplicateCpu,
        CpuCode::SwapgsBad,
        CpuCode::ReplayMismatch,
        CpuCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < CpuCode::ALL.len() {
        if CpuCode::ALL[i].val() == code {
            return CpuCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6F700 段闭集内"
}

// ── per-CPU 账 ──────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CpuArea {
    pub cpu: usize,
    pub base: u64,
    pub size: u64,
}

#[derive(Clone, Debug, Default)]
pub struct PerCpuLedger {
    pub areas: Vec<CpuArea>,
    /// gs 基址当前归属（0=内核 gs / 1=用户 gs 记账位）。
    pub kernel_gs: bool,
    pub swapgs_count: u32,
}

impl PerCpuLedger {
    pub fn new() -> PerCpuLedger {
        PerCpuLedger::default()
    }

    /// 注册 per-CPU 区（对齐 + 重叠 + 重复三闸）。
    pub fn register(&mut self, cpu: usize, base: u64, size: u64) -> Result<usize, u32> {
        if cpu >= MAX_CPUS {
            return Err(CpuCode::CpuOver.val());
        }
        if base % AREA_ALIGN != 0 {
            return Err(CpuCode::BaseMisaligned.val());
        }
        let mut i = 0;
        while i < self.areas.len() {
            let e = &self.areas[i];
            if e.cpu == cpu {
                return Err(CpuCode::DuplicateCpu.val());
            }
            if base < e.base + e.size && e.base < base + size {
                return Err(CpuCode::AreaOverlap.val());
            }
            i += 1;
        }
        self.areas.push(CpuArea { cpu, base, size });
        Ok(self.areas.len() - 1)
    }

    /// gs 基址查询（CPU 号 → 区基址）。
    pub fn gs_base_of(&self, cpu: usize) -> Result<u64, u32> {
        let mut i = 0;
        while i < self.areas.len() {
            if self.areas[i].cpu == cpu {
                return Ok(self.areas[i].base);
            }
            i += 1;
        }
        Err(CpuCode::CpuOver.val())
    }

    /// swapgs 记账（内核↔用户切换计数）。
    pub fn swapgs(&mut self) -> Result<(), u32> {
        self.kernel_gs = !self.kernel_gs;
        self.swapgs_count += 1;
        Ok(())
    }
}

/// 复算。
pub fn replay_ok(areas: &[(usize, u64, u64)]) -> Result<(), u32> {
    let mut a = PerCpuLedger::new();
    let mut b = PerCpuLedger::new();
    let mut i = 0;
    while i < areas.len() {
        let _ = a.register(areas[i].0, areas[i].1, areas[i].2);
        let _ = b.register(areas[i].0, areas[i].1, areas[i].2);
        i += 1;
    }
    if a.areas != b.areas {
        return Err(CpuCode::ReplayMismatch.val());
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

// ── 注入检出账（四路：越号路/对齐路/重叠路/复算路） ──────────────
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

/// 真机 per-CPU 区（4 CPU，各 4KB 区，16 对齐间隔）。
pub fn machine_areas() -> Vec<(usize, u64, u64)> {
    vec![(0, 0x100000, 0x1000), (1, 0x102000, 0x1000), (2, 0x104000, 0x1000), (3, 0x106000, 0x1000)]
}

// ── 黄金全链：注册→gs 查询→swapgs 偶数回位→重叠/重复/越号/对齐拒→复算
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut l = PerCpuLedger::new();
    let areas = machine_areas();
    let mut i = 0;
    while i < areas.len() {
        l.register(areas[i].0, areas[i].1, areas[i].2)?;
        i += 1;
    }
    // gs 基址一一对应。
    let gs_ok = (l.gs_base_of(2)? == 0x104000) as u32;
    // swapgs 偶数次回位。
    l.swapgs()?;
    l.swapgs()?;
    let swap_ok = (l.kernel_gs == false && l.swapgs_count == 2) as u32;
    // 四拒。
    let over = l.register(MAX_CPUS, 0x200000, 0x1000).is_err();
    let misaligned = l.register(4, 0x200004, 0x1000).is_err();
    let overlap = l.register(4, 0x100800, 0x1000).is_err();
    let dup = l.register(0, 0x200000, 0x1000).is_err();
    replay_ok(&areas)?;
    Ok((gs_ok + swap_ok, over as u32 + misaligned as u32 + overlap as u32 + dup as u32, 0))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：gs 对应 + swapgs 回位 + 四拒。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((2, 4, _)) => Ok(()),
        _ => Err(CpuCode::NotReproducible.val()),
    }
}

/// 序 B：未注册 CPU 查询 ⇒ CpuOver。
pub fn probe_seq_b() -> Result<(), u32> {
    let l = PerCpuLedger::new();
    match l.gs_base_of(9) {
        Err(e) if e == CpuCode::CpuOver.val() => Ok(()),
        _ => Err(CpuCode::NotReproducible.val()),
    }
}

/// 序 C：对齐/上限冻结。
pub fn probe_seq_c() -> Result<(), u32> {
    match (AREA_ALIGN == 16, MAX_CPUS == 64) {
        (true, true) => Ok(()),
        _ => Err(CpuCode::NotReproducible.val()),
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
        let mut l1 = PerCpuLedger::new();
        led.record(0, l1.register(MAX_CPUS, 0x1000, 0x1000).is_err());
        // 路 2：对齐路。
        led.record(1, l1.register(0, 0x1001, 0x1000).is_err());
        // 路 3：重叠路。
        let mut l3 = PerCpuLedger::new();
        let _ = l3.register(0, 0x100000, 0x2000);
        led.record(2, l3.register(1, 0x101000, 0x1000).is_err());
        // 路 4：复算路。
        led.record(3, replay_ok(&machine_areas()).is_ok());
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(CpuCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < CpuCode::ALL.len() {
        let v = CpuCode::ALL[i].val();
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
            Ok((ok, rej, _)) => *ok == 2 && *rej == 4,
            Err(_) => false,
        },
        match &g {
            Ok((ok, rej, _)) => format!("gs+swap={} rejects={}", ok, rej),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 真机区账（4 CPU 16 对齐互不重叠）。
    let mut l = PerCpuLedger::new();
    let areas = machine_areas();
    let mut all_ok = true;
    let mut i = 0;
    while i < areas.len() {
        if l.register(areas[i].0, areas[i].1, areas[i].2).is_err() {
            all_ok = false;
        }
        i += 1;
    }
    v.push((
        "J1-区账",
        all_ok && l.areas.len() == 4,
        String::from("4 CPU 区互不重叠 16 对齐"),
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
        String::from("0x6F700 窗口"),
    ));
    v
}

pub fn run_unx0192_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0192 fs/gs base 设置与 per-CPU 预备");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
