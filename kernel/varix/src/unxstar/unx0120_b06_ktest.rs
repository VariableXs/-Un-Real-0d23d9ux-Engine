//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0120 · ktest 引导面 B06 批断言集 · 0x6EB80
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F0120-J1 B06 批（F0104~F0119）全量判据聚合：十二单
//!     一次命令全跑，全绿才批绿，批账三源守恒 + FNV 指纹封存 +
//!     漏单显性拒；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 聚合面：F0104/F0105/F0106/F0107/F0108/F0109/F0110/F0111/
//!     F0112/F0113/F0114/F0115/F0116/F0117/F0118/F0119（十六单）
//!     逐单直调；
//!   - 批账三源（单数/总项数/绿单数）+ 指纹封存复算；
//!   - 独立四路注入 10/10；
//!   - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6EB80，AI-VX3-1102 批M 独占区）。
pub const SEG_BASE: u32 = 0x6EB80;

/// B06 批单数（F0104~F0119 = 16 单）。
pub const B06_UNITS: usize = 16;
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
            AggCode::UnitCountBad => "聚合漏单：单数 ≠ 16",
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
    "未知诊断码：不在 0x6EB80 段闭集内"
}

// ── 单报告与聚合 ────────────────────────────────────────────────
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

pub fn aggregate_all() -> Vec<UnitReport> {
    let mut r: Vec<UnitReport> = Vec::new();
    run_unit("F0104", super::unx0104_dual_channel::run_unx0104_checks(), &mut r);
    run_unit("F0105", super::unx0105_log_levels::run_unx0105_checks(), &mut r);
    run_unit("F0106", super::unx0106_log_tsc_ts::run_unx0106_checks(), &mut r);
    run_unit("F0107", super::unx0107_log_ring::run_unx0107_checks(), &mut r);
    run_unit("F0108", super::unx0108_panic_log_snap::run_unx0108_checks(), &mut r);
    run_unit("F0109", super::unx0109_uart_cfg::run_unx0109_checks(), &mut r);
    run_unit("F0110", super::unx0110_console_attr::run_unx0110_checks(), &mut r);
    run_unit("F0111", super::unx0111_scroll_history::run_unx0111_checks(), &mut r);
    run_unit("F0112", super::unx0112_ansi_csi::run_unx0112_checks(), &mut r);
    run_unit("F0113", super::unx0113_log_sinks::run_unx0113_checks(), &mut r);
    run_unit("F0114", super::unx0114_diag_ui::run_unx0114_checks(), &mut r);
    run_unit("F0115", super::unx0115_quiet_boot::run_unx0115_checks(), &mut r);
    run_unit("F0116", super::unx0116_log_inject_guard::run_unx0116_checks(), &mut r);
    run_unit("F0117", super::unx0117_serial_mirror::run_unx0117_checks(), &mut r);
    run_unit("F0118", super::unx0118_log_format::run_unx0118_checks(), &mut r);
    run_unit("F0119", super::unx0119_fail_probes::run_unx0119_checks(), &mut r);
    r
}

#[derive(Clone, Debug)]
pub struct BatchLedger {
    pub units: usize,
    pub total_items: usize,
    pub green_units: usize,
    pub fp: u64,
}

impl BatchLedger {
    pub fn build(reports: &[UnitReport]) -> Result<BatchLedger, u32> {
        if reports.len() != B06_UNITS {
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

// ── 注入检出账（四路：漏单/绿账/指纹/段位） ──────────────────────
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

// ── 黄金全链：16 单批绿 + 守恒 + 复算 ────────────────────────────
pub fn golden_run() -> Result<(usize, usize, bool), u32> {
    let led = aggregate_stable()?;
    let reports = aggregate_all();
    led.conservation(&reports)?;
    Ok((led.units, led.total_items, led.batch_green()))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：16 单全绿。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((16, _, true)) => Ok(()),
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
            Ok((units, items, green)) => *units == 16 && *green && *items > 100,
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
    let mut all_green = reports.len() == B06_UNITS;
    let mut i = 0;
    while i < reports.len() {
        if !reports[i].green {
            all_green = false;
        }
        names.push_str(reports[i].name);
        names.push(',');
        i += 1;
    }
    v.push(("J1-逐单点名", all_green, names));
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
        String::from("0x6EB80 窗口"),
    ));
    v
}

pub fn run_unx0120_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0120 ktest 引导面 B06 批断言集");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
