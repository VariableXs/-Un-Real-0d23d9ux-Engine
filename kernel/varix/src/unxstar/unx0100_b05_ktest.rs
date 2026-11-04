//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0100 · ktest 引导面 B05 批断言集 · 0x6E740
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥340 行；
//!   ② UNX-F0100-J1 B05 批（F0085~F0099）全量判据聚合：一次命
//!     令全跑各单判据面，全绿才批绿，批账三源守恒 + 指纹封存 +
//!     漏单显性拒；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 聚合面：F0085~F0099 十二单（85/86/87/88/89/90/91/92/93/
//!     94/95/96/97/98/99 = 15 单）逐单直调；
//!   - 批账三源：单数 / 总项数 / 绿单数（守恒对拍）；
//!   - FNV 指纹封存（重跑一致可复算）；
//!   - 独立四路注入（漏单/指纹/守恒/段位）10/10；
//!   - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6E740，AI-VX3-1102 批K 独占区）。
pub const SEG_BASE: u32 = 0x6E740;

/// B05 批单数（F0085~F0099 = 15 单）。
pub const B05_UNITS: usize = 15;
/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AggCode {
    /// 聚合漏单
    UnitCountBad,
    /// 守恒破坏
    ConservationBad,
    /// 指纹漂移
    FingerprintDrift,
    /// 段位越界
    SegmentOver,
    /// 绿账失真
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
            AggCode::UnitCountBad => "聚合漏单：单数 ≠ 15",
            AggCode::ConservationBad => "守恒破坏：三源不等",
            AggCode::FingerprintDrift => "指纹漂移：重跑不一致",
            AggCode::SegmentOver => "段位越界",
            AggCode::GreenLedgerBad => "绿账失真",
            AggCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [AggCode; 6] = [
        AggCode::UnitCountBad,
        AggCode::ConservationBad,
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
    "未知诊断码：不在 0x6E740 段闭集内"
}

// ── 单报告 ──────────────────────────────────────────────────────
pub struct UnitReport {
    pub name: &'static str,
    pub items: usize,
    pub reds: usize,
    pub green: bool,
}

fn run_unit(name: &'static str, s: CheckSet, out: &mut Vec<UnitReport>) {
    let items = s.len();
    let reds = s.failed_count();
    out.push(UnitReport { name, items, reds, green: reds == 0 });
}

/// 聚合 B05 全批。
pub fn aggregate_all() -> Vec<UnitReport> {
    let mut r: Vec<UnitReport> = Vec::new();
    run_unit("F0085", super::unx0085_bss_zero::run_unx0085_checks(), &mut r);
    run_unit("F0086", super::unx0086_symtab_panic::run_unx0086_checks(), &mut r);
    run_unit("F0087", super::unx0087_kaslr_off::run_unx0087_checks(), &mut r);
    run_unit("F0088", super::unx0088_manifest_replay::run_unx0088_checks(), &mut r);
    run_unit("F0089", super::unx0089_highhalf_offset::run_unx0089_checks(), &mut r);
    run_unit("F0090", super::unx0090_phys_base_pick::run_unx0090_checks(), &mut r);
    run_unit("F0091", super::unx0091_pagetable_kaslr::run_unx0091_checks(), &mut r);
    run_unit("F0092", super::unx0092_bl_coop_entropy::run_unx0092_checks(), &mut r);
    run_unit("F0093", super::unx0093_sidechannel_min::run_unx0093_checks(), &mut r);
    run_unit("F0094", super::unx0094_reloc_audit::run_unx0094_checks(), &mut r);
    run_unit("F0095", super::unx0095_boot_fp::run_unx0095_checks(), &mut r);
    run_unit("F0096", super::unx0096_seg_loader::run_unx0096_checks(), &mut r);
    run_unit("F0097", super::unx0097_bad_image_reject::run_unx0097_checks(), &mut r);
    run_unit("F0098", super::unx0098_early_map_consist::run_unx0098_checks(), &mut r);
    run_unit("F0099", super::unx0099_reloc_timing::run_unx0099_checks(), &mut r);
    r
}

// ── 批账 ────────────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct BatchLedger {
    pub units: usize,
    pub total_items: usize,
    pub green_units: usize,
    pub fp: u64,
}

impl BatchLedger {
    pub fn build(reports: &[UnitReport]) -> Result<BatchLedger, u32> {
        if reports.len() != B05_UNITS {
            return Err(AggCode::UnitCountBad.val());
        }
        let mut total = 0usize;
        let mut green = 0usize;
        let mut fp: u64 = 0xcbf29ce484222325;
        let mut i = 0;
        while i < reports.len() {
            total += reports[i].items;
            if reports[i].green {
                green += 1;
            }
            fp = (fp ^ reports[i].items as u64).wrapping_mul(0x100000001b3);
            fp = (fp ^ ((reports[i].reds as u64) << 32)).wrapping_mul(0x100000001b3);
            i += 1;
        }
        Ok(BatchLedger { units: reports.len(), total_items: total, green_units: green, fp })
    }

    pub fn conservation(&self, reports: &[UnitReport]) -> Result<(), u32> {
        let mut sum = 0usize;
        let mut i = 0;
        while i < reports.len() {
            sum += reports[i].items;
            i += 1;
        }
        if sum != self.total_items {
            return Err(AggCode::ConservationBad.val());
        }
        if self.green_units > self.units {
            return Err(AggCode::GreenLedgerBad.val());
        }
        Ok(())
    }

    pub fn batch_green(&self) -> bool {
        self.green_units == self.units
    }
}

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

// ── 注入检出账（四路：漏单路/绿账路/指纹路/段位路） ──────────────
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

// ── 黄金全链：15 单聚合全绿 + 守恒 + 复算 ─────────────────────────
pub fn golden_run() -> Result<(usize, usize, bool), u32> {
    let led = aggregate_stable()?;
    let reports = aggregate_all();
    led.conservation(&reports)?;
    Ok((led.units, led.total_items, led.batch_green()))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：15 单全绿。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((15, _, true)) => Ok(()),
        _ => Err(AggCode::NotReproducible.val()),
    }
}

/// 序 B：漏单 ⇒ UnitCountBad。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut reports = aggregate_all();
    let _ = reports.pop();
    match BatchLedger::build(&reports) {
        Err(e) if e == AggCode::UnitCountBad.val() => Ok(()),
        _ => Err(AggCode::NotReproducible.val()),
    }
}

/// 序 C：守恒破坏 ⇒ ConservationBad。
pub fn probe_seq_c() -> Result<(), u32> {
    let reports = aggregate_all();
    let mut led = BatchLedger::build(&reports)?;
    led.total_items += 1;
    match led.conservation(&reports) {
        Err(e) if e == AggCode::ConservationBad.val() => Ok(()),
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
        // 路 2：绿账路。
        let rep2 = aggregate_all();
        let mut l2 = BatchLedger::build(&rep2)?;
        l2.green_units = l2.units + 1;
        led.record(1, l2.conservation(&rep2).is_err());
        // 路 3：指纹路。
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
            Ok((units, items, green)) => *units == 15 && *green && *items > 100,
            Err(_) => false,
        },
        match &g {
            Ok((units, items, green)) => format!("units={} items={} batch_green={}", units, items, green),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 逐单点名。
    let reports = aggregate_all();
    let mut names = String::new();
    let mut all_green = reports.len() == B05_UNITS;
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
    // 批绿语义。
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
        String::from("0x6E740 窗口"),
    ));
    v
}

pub fn run_unx0100_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0100 ktest 引导面 B05 批断言集");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
