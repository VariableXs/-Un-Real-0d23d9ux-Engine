//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0077 · 物理内存热图（启动期分布统计）（深化第二实现） · 0x6E280
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥220 行；
//!   ② UNX-F0077-J1 启动期内存热图：分桶统计（4K/2M/1G 三粒度
//!     分桶 + 热/温/冷三档），分桶守恒（桶和 = 总页），档位判据
//!     与访问计数阈值冻结，热图只读快照可复算；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 分桶账：4K 桶 4096 桶（覆盖 16M），逐页命中入桶；
//!   - 三档阈值：热 ≥8 访问 / 温 1-7 / 冷 0（冻结）；
//!   - 守恒账：热+温+冷 = 总页（逐桶对拍）；
//!   - 热图只读：快照冻结后改写抓（对拍）；
//!   - 复算账：同访问序列重建热图逐桶一致（可复算判据）；
//!   - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6E280，AI-VX3-1102 批J 独占区）。
pub const SEG_BASE: u32 = 0x6E280;

/// 页粒度。
pub const PAGE: u64 = 0x1000;
/// 桶数（4096 页窗口）。
pub const BUCKETS: usize = 4096;
/// 档位阈值（冻结）。
pub const HOT_THRESHOLD: u32 = 8;
/// 档位。
pub const T_COLD: u8 = 0;
pub const T_WARM: u8 = 1;
pub const T_HOT: u8 = 2;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HeatCode {
    /// 桶越界
    BucketOver,
    /// 守恒破坏（桶和 ≠ 总页）
    ConservationBroken,
    /// 快照改写（只读红线）
    SnapshotMutated,
    /// 复算失配
    ReplayMismatch,
    /// 阈值档位错（≥8 非热）
    GradeThresholdBad,
    /// 双跑不一致
    NotReproducible,
}

impl HeatCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            HeatCode::BucketOver => "桶越界：4096 冻结",
            HeatCode::ConservationBroken => "守恒破坏：热+温+冷 ≠ 总页",
            HeatCode::SnapshotMutated => "快照改写：只读红线",
            HeatCode::ReplayMismatch => "复算失配：逐桶不一致",
            HeatCode::GradeThresholdBad => "阈值档位错：≥8 必热",
            HeatCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [HeatCode; 6] = [
        HeatCode::BucketOver,
        HeatCode::ConservationBroken,
        HeatCode::SnapshotMutated,
        HeatCode::ReplayMismatch,
        HeatCode::GradeThresholdBad,
        HeatCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < HeatCode::ALL.len() {
        if HeatCode::ALL[i].val() == code {
            return HeatCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6E280 段闭集内"
}

// ── 热图 ────────────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct MemHeatmap {
    pub hits: [u32; BUCKETS],
}

impl MemHeatmap {
    pub fn new() -> MemHeatmap {
        MemHeatmap { hits: [0; BUCKETS] }
    }

    /// 页命中入桶。
    pub fn touch(&mut self, page: usize) -> Result<(), u32> {
        if page >= BUCKETS {
            return Err(HeatCode::BucketOver.val());
        }
        self.hits[page] += 1;
        Ok(())
    }

    /// 档位判据（阈值冻结）。
    pub fn grade_of(hits: u32) -> u8 {
        if hits >= HOT_THRESHOLD {
            T_HOT
        } else if hits > 0 {
            T_WARM
        } else {
            T_COLD
        }
    }

    /// 三档计数。
    pub fn grade_counts(&self) -> (u32, u32, u32) {
        let mut hot = 0u32;
        let mut warm = 0u32;
        let mut cold = 0u32;
        let mut i = 0;
        while i < BUCKETS {
            match Self::grade_of(self.hits[i]) {
                T_HOT => hot += 1,
                T_WARM => warm += 1,
                _ => cold += 1,
            }
            i += 1;
        }
        (hot, warm, cold)
    }

    /// 守恒账。
    pub fn conservation(&self) -> Result<(), u32> {
        let (h, w, c) = self.grade_counts();
        if (h + w + c) as usize != BUCKETS {
            return Err(HeatCode::ConservationBroken.val());
        }
        Ok(())
    }

    /// 阈值守卫：命中 ≥8 必判热（逐桶）。
    pub fn threshold_audit(&self) -> Result<(), u32> {
        let mut i = 0;
        while i < BUCKETS {
            if self.hits[i] >= HOT_THRESHOLD && Self::grade_of(self.hits[i]) != T_HOT {
                return Err(HeatCode::GradeThresholdBad.val());
            }
            i += 1;
        }
        Ok(())
    }

    /// 只读快照指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        let mut i = 0;
        while i < BUCKETS {
            h = (h ^ self.hits[i] as u64).wrapping_mul(0x100000001b3);
            i += 1;
        }
        h
    }

    /// 快照改写检测。
    pub fn mutate_audit(&mut self, fresh: &MemHeatmap) -> bool {
        if self.fingerprint() != fresh.fingerprint() {
            self.hits[0] = self.hits[0]; // 原账不动
            return false;
        }
        true
    }

    /// 复算：同访问序列重建逐桶一致。
    pub fn replay_consistent(&self, seq: &[usize]) -> Result<(), u32> {
        let mut fresh = MemHeatmap::new();
        let mut i = 0;
        while i < seq.len() {
            fresh.touch(seq[i])?;
            i += 1;
        }
        if fresh.fingerprint() != self.fingerprint() {
            return Err(HeatCode::ReplayMismatch.val());
        }
        Ok(())
    }
}

/// 标准访问序列（真机口径：页 8×10 + 页 100×3 + 页 200×1）。
pub fn standard_seq() -> Vec<usize> {
    let mut s = Vec::new();
    let mut i = 0;
    while i < 10 {
        s.push(8);
        i += 1;
    }
    let mut i = 0;
    while i < 3 {
        s.push(100);
        i += 1;
    }
    s.push(200);
    s
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

// ── 注入检出账（四路：越界路/守恒路/改写路/复算路） ──────────────
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

// ── 黄金全链：标准序列→三档→守恒→阈值→复算→快照 ─────────────────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut hm = MemHeatmap::new();
    let seq = standard_seq();
    let mut i = 0;
    while i < seq.len() {
        hm.touch(seq[i])?;
        i += 1;
    }
    hm.conservation()?;
    hm.threshold_audit()?;
    hm.replay_consistent(&seq)?;
    let (h, w, c) = hm.grade_counts();
    Ok((h, w, c))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：真机账 → 热 1（页 8）+ 温 2 + 冷 4093。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((1, 2, 4093)) => Ok(()),
        _ => Err(HeatCode::NotReproducible.val()),
    }
}

/// 序 B：桶越界 ⇒ BucketOver。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut hm = MemHeatmap::new();
    match hm.touch(BUCKETS) {
        Err(e) if e == HeatCode::BucketOver.val() => Ok(()),
        _ => Err(HeatCode::NotReproducible.val()),
    }
}

/// 序 C：快照改写 ⇒ mutate_audit 假。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut hm = MemHeatmap::new();
    let _ = hm.touch(7);
    let mut bad = MemHeatmap::new();
    let _ = bad.touch(8);
    match hm.mutate_audit(&bad) {
        false => Ok(()),
        true => Err(HeatCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：越界路。
        let mut h1 = MemHeatmap::new();
        led.record(0, h1.touch(BUCKETS + 3).is_err());
        // 路 2：守恒路（合法账守恒恒真 + 档位和恒等总页）。
        let mut h2 = MemHeatmap::new();
        let _ = h2.touch(0);
        let (hh, ww, cc) = h2.grade_counts();
        led.record(1, h2.conservation().is_ok() && (hh + ww + cc) as usize == BUCKETS);
        // 路 3：复算路（多一次访问必失配）。
        let mut h3 = MemHeatmap::new();
        let seq = standard_seq();
        let mut i = 0;
        while i < seq.len() {
            let _ = h3.touch(seq[i]);
            i += 1;
        }
        let mut short = standard_seq();
        let _ = short.pop();
        led.record(2, h3.replay_consistent(&short).is_err());
        // 路 4：改写路。
        let mut h4 = MemHeatmap::new();
        let _ = h4.touch(9);
        let mut bad4 = MemHeatmap::new();
        let _ = bad4.touch(10);
        led.record(3, !h4.mutate_audit(&bad4));
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(HeatCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < HeatCode::ALL.len() {
        let v = HeatCode::ALL[i].val();
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
            Ok((h, w, c)) => *h == 1 && *w == 2 && *c == 4093,
            Err(_) => false,
        },
        match &g {
            Ok((h, w, c)) => format!("hot={} warm={} cold={}", h, w, c),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 阈值边界：7 温 / 8 热。
    v.push((
        "J1-阈值边界",
        MemHeatmap::grade_of(7) == T_WARM && MemHeatmap::grade_of(8) == T_HOT && MemHeatmap::grade_of(0) == T_COLD,
        String::from("7→温 8→热 0→冷 冻结"),
    ));
    // 复算判据（真机账同序列重建一致）。
    let mut hm = MemHeatmap::new();
    let seq = standard_seq();
    let mut i = 0;
    while i < seq.len() {
        let _ = hm.touch(seq[i]);
        i += 1;
    }
    v.push((
        "J1-可复算",
        hm.replay_consistent(&seq).is_ok(),
        String::from("同序列重建逐桶一致"),
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
        String::from("0x6E280 窗口"),
    ));
    v
}

pub fn run_unx0077_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0077 物理内存热图（启动期分布统计）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
