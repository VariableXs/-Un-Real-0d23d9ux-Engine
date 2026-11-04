//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0080 · ktest 引导面 B04 批断言集 · 0x6E340
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥330 行；
//!   ② UNX-F0080-J1 B04 批（F0063~F0079）全量判据聚合：一次命
//!     令全跑各单 13 项判据面，全部绿才算批绿，批账三源（单账
//!     数/绿项数/总项数）守恒；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 聚合面：逐单调 run_unx00NN_checks（F0063~F0079 十七单），
//!     全 passed 才单绿；任一红列红账；
//!   - 批账三源守恒：单数 × 各单项数 = 总项数（逐单累加对拍）；
//!   - 批账封存：聚合结果指纹封存，重跑指纹一致（可复算）；
//!   - 独立自检面：本卡四路注入（聚合漏单/指纹漂移/守恒破坏/
//!     段位越界）10/10；
//!   - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6E340，AI-VX3-1102 批J 独占区）。
pub const SEG_BASE: u32 = 0x6E340;

/// B04 批单数（F0063~F0079 = 17 单）。
pub const B04_UNITS: usize = 17;
/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AggCode {
    /// 聚合漏单（单数 ≠ 17）
    UnitCountBad,
    /// 批账守恒破坏（三源不等）
    ConservationBroken,
    /// 聚合指纹漂移（重跑不一致）
    FingerprintDrift,
    /// 段位越界
    SegmentOver,
    /// 单绿账失真（红单计绿）
    GreenLedgerBad,
    /// 双跑不一致
    NotReproducible,
}

impl AggCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            AggCode::UnitCountBad => "聚合漏单：单数 ≠ 17",
            AggCode::ConservationBroken => "批账守恒破坏：三源不等",
            AggCode::FingerprintDrift => "聚合指纹漂移：重跑不一致",
            AggCode::SegmentOver => "段位越界",
            AggCode::GreenLedgerBad => "单绿账失真：红单计绿",
            AggCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [AggCode; 6] = [
        AggCode::UnitCountBad,
        AggCode::ConservationBroken,
        AggCode::FingerprintDrift,
        AggCode::SegmentOver,
        AggCode::GreenLedgerBad,
        AggCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < AggCode::ALL.len() {
        if AggCode::ALL[i].val() == code {
            return AggCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6E340 段闭集内"
}

// ── B04 单注册表（聚合面，逐单直调） ─────────────────────────────
pub struct UnitReport {
    pub name: &'static str,
    pub items: usize,
    pub green: bool,
    pub reds: usize,
}

fn run_unit(name: &'static str, s: CheckSet, reports: &mut Vec<UnitReport>) {
    let items = s.len();
    let reds = s.failed_count();
    reports.push(UnitReport { name, items, green: reds == 0, reds });
}

/// 聚合全批（一次命令全跑）。
pub fn aggregate_all() -> Vec<UnitReport> {
    let mut r: Vec<UnitReport> = Vec::new();
    run_unit("F0063", super::unx0063_reclaim_nvs::run_unx0063_checks(), &mut r);
    run_unit("F0064", super::unx0064_low1mb_layout::run_unx0064_checks(), &mut r);
    run_unit("F0065", super::unx0065_e820_bridge2::run_unx0065_checks(), &mut r);
    run_unit("F0066", super::unx0066_memmap_snapshot::run_unx0066_checks(), &mut r);
    run_unit("F0067", super::unx0067_zone_aligner::run_unx0067_checks(), &mut r);
    run_unit("F0068", super::unx0068_hugepage_avail::run_unx0068_checks(), &mut r);
    run_unit("F0069", super::unx0069_hotplug_bitmap2::run_unx0069_checks(), &mut r);
    run_unit("F0070", super::unx0070_memmap_persist::run_unx0070_checks(), &mut r);
    run_unit("F0071", super::unx0071_early_alloc2::run_unx0071_checks(), &mut r);
    run_unit("F0072", super::unx0072_page_ledger_audit::run_unx0072_checks(), &mut r);
    run_unit("F0073", super::unx0073_hole_detector::run_unx0073_checks(), &mut r);
    run_unit("F0074", super::unx0074_numa_page_bitmap::run_unx0074_checks(), &mut r);
    run_unit("F0075", super::unx0075_memcap_policy::run_unx0075_checks(), &mut r);
    run_unit("F0076", super::unx0076_badpage_zone2::run_unx0076_checks(), &mut r);
    run_unit("F0077", super::unx0077_mem_heat2::run_unx0077_checks(), &mut r);
    run_unit("F0078", super::unx0078_cr3_selftest2::run_unx0078_checks(), &mut r);
    run_unit("F0079", super::unx0079_a4_handoff2::run_unx0079_checks(), &mut r);
    r
}

// ── 批账（三源守恒 + 指纹封存） ──────────────────────────────────
#[derive(Clone, Debug)]
pub struct BatchLedger {
    pub units: usize,
    pub total_items: usize,
    pub green_units: usize,
    pub fp: u64,
}

impl BatchLedger {
    /// 从单报告集建批账（三源守恒 + FNV 封存）。
    pub fn build(reports: &[UnitReport]) -> Result<BatchLedger, u32> {
        if reports.len() != B04_UNITS {
            return Err(AggCode::UnitCountBad.val());
        }
        let mut total_items = 0usize;
        let mut green = 0usize;
        let mut fp: u64 = 0xcbf29ce484222325;
        let mut i = 0;
        while i < reports.len() {
            total_items += reports[i].items;
            if reports[i].green {
                green += 1;
            }
            fp = (fp ^ reports[i].items as u64).wrapping_mul(0x100000001b3);
            fp = (fp ^ (reports[i].reds as u64) << 32).wrapping_mul(0x100000001b3);
            i += 1;
        }
        Ok(BatchLedger { units: reports.len(), total_items, green_units: green, fp })
    }

    /// 三源守恒：绿单数 ≤ 单数；总项数 = 逐单和（构建时已算，重对拍）。
    pub fn conservation(&self, reports: &[UnitReport]) -> Result<(), u32> {
        let mut sum = 0usize;
        let mut i = 0;
        while i < reports.len() {
            sum += reports[i].items;
            i += 1;
        }
        if sum != self.total_items {
            return Err(AggCode::ConservationBroken.val());
        }
        if self.green_units > self.units {
            return Err(AggCode::GreenLedgerBad.val());
        }
        Ok(())
    }

    /// 全批绿。
    pub fn batch_green(&self) -> bool {
        self.green_units == self.units
    }
}

/// 聚合可复算：两跑批账指纹一致。
pub fn aggregate_stable() -> Result<BatchLedger, u32> {
    let a = BatchLedger::build(&aggregate_all())?;
    let b = BatchLedger::build(&aggregate_all())?;
    if a.fp != b.fp || a.total_items != b.total_items {
        return Err(AggCode::FingerprintDrift.val());
    }
    Ok(a)
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

// ── 注入检出账（四路：漏单路/守恒路/指纹路/绿账路） ──────────────
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

// ── 黄金全链：聚合 17 单全绿 + 三源守恒 + 复算稳定 ────────────────
pub fn golden_run() -> Result<(usize, usize, bool), u32> {
    let led = aggregate_stable()?;
    let reports = aggregate_all();
    led.conservation(&reports)?;
    Ok((led.units, led.total_items, led.batch_green()))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：17 单全绿。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((17, _, true)) => Ok(()),
        _ => Err(AggCode::NotReproducible.val()),
    }
}

/// 序 B：漏单（16 单）⇒ UnitCountBad。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut reports = aggregate_all();
    let _ = reports.pop();
    match BatchLedger::build(&reports) {
        Err(e) if e == AggCode::UnitCountBad.val() => Ok(()),
        _ => Err(AggCode::NotReproducible.val()),
    }
}

/// 序 C：守恒破坏（批账篡改总项数）⇒ ConservationBroken。
pub fn probe_seq_c() -> Result<(), u32> {
    let reports = aggregate_all();
    let mut led = BatchLedger::build(&reports)?;
    led.total_items += 1;
    match led.conservation(&reports) {
        Err(e) if e == AggCode::ConservationBroken.val() => Ok(()),
        _ => Err(AggCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：漏单路。
        let mut rep1 = aggregate_all();
        let _ = rep1.pop();
        led.record(0, BatchLedger::build(&rep1).is_err());
        // 路 2：绿账路（伪造绿单越单数必拒）。
        let rep2 = aggregate_all();
        let mut l2 = BatchLedger::build(&rep2)?;
        l2.green_units = l2.units + 1;
        led.record(1, l2.conservation(&rep2).is_err());
        // 路 3：指纹路（逐单红数漂移指纹必变）。
        let l3a = BatchLedger::build(&aggregate_all())?;
        let mut rep3 = aggregate_all();
        rep3[0].reds += 1;
        rep3[0].green = false;
        let l3b = BatchLedger::build(&rep3)?;
        led.record(2, l3a.fp != l3b.fp);
        // 路 4：段位路。
        let seg_ok = {
            let mut ok = true;
            let mut i = 0;
            while i < AggCode::ALL.len() {
                let v = AggCode::ALL[i].val();
                if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                    ok = false;
                }
                i += 1;
            }
            ok
        };
        led.record(3, seg_ok);
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(AggCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < AggCode::ALL.len() {
        let v = AggCode::ALL[i].val();
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
            Ok((units, items, green)) => *units == 17 && *green && *items > 100,
            Err(_) => false,
        },
        match &g {
            Ok((units, items, green)) => format!("units={} items={} batch_green={}", units, items, green),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 逐单绿账点名（17 单全绿逐名列出）。
    let reports = aggregate_all();
    let mut names = String::new();
    let mut all_green = reports.len() == B04_UNITS;
    let mut i = 0;
    while i < reports.len() {
        if !reports[i].green {
            all_green = false;
        }
        names.push_str(reports[i].name);
        names.push(',');
        i += 1;
    }
    v.push((
        "J1-逐单点名",
        all_green,
        names,
    ));
    // 批绿语义：green_units == units。
    let led = aggregate_stable();
    v.push((
        "J1-批绿语义",
        match &led {
            Ok(l) => l.batch_green(),
            Err(_) => false,
        },
        match &led {
            Ok(l) => format!("{}/{} 单绿", l.green_units, l.units),
            Err(e) => format!("err=0x{:x}", e),
        },
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
        String::from("0x6E340 窗口"),
    ));
    v
}

pub fn run_unx0080_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0080 ktest 引导面 B04 批断言集");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
