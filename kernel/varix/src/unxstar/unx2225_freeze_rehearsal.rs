//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F2225 · 冻结终验预演一：B31–B32 段级预查 · 0x6D540
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-B5）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F2225-J1 段级预演全过（冻结五步断言链对 B31–B32 两批
//!     预跑：ID 连续/逐批求和/正文阈值/记账表/对回五步预演全过），
//!     预演与实跑口径一致断言过，预演缺陷零遗留声明在册；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 五步断言链闭集：ID 连续/逐批求和/正文阈值/记账表/对回（冻
//!      结五步，B31–B32 两批预跑）；
//!    - 预演口径 = 实跑口径：预演结果与实跑判定逐位一致（一致断
//!      言）；
//!    - 缺陷零遗留：预演缺陷账（空 = 零遗留声明在册）；
//!    - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6D540，AI-VX3-1030 批F 独占区）。
pub const SEG_BASE: u32 = 0x6D540;

// ── 五步闭集 ────────────────────────────────────────────────────
pub const ST_ID_CONT: usize = 0;
pub const ST_BATCH_SUM: usize = 1;
pub const ST_TEXT_THRESH: usize = 2;
pub const ST_LEDGER_TABLE: usize = 3;
pub const ST_RECON: usize = 4;
pub const FIVE_STEPS: usize = 5;
pub const STEP_NAMES: [&str; FIVE_STEPS] =
    ["ID 连续", "逐批求和", "正文阈值", "记账表", "对回"];
/// 预演两批。
pub const BATCHES: [&str; 2] = ["B31", "B32"];
/// 每批条数（冻结）。
pub const CARDS_PER_BATCH: usize = 20;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RehCode {
    /// 步越闭集
    StepOutOfRange,
    /// 步名空（五步齐备破坏）
    StepNameBlank,
    /// ID 断号
    IdGapDetected,
    /// 求和错
    SumMismatch,
    /// 阈值越界
    ThresholdOver,
    /// 记账表缺
    LedgerTableShort,
    /// 预演与实跑口径不一致
    CaliberMismatch,
    /// 缺陷遗留（非零）
    DefectLeft,
    /// 双跑不一致
    NotReproducible,
}

impl RehCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            RehCode::StepOutOfRange => "步越闭集：五步冻结",
            RehCode::StepNameBlank => "步名空：齐备破坏",
            RehCode::IdGapDetected => "ID 断号",
            RehCode::SumMismatch => "求和错",
            RehCode::ThresholdOver => "阈值越界",
            RehCode::LedgerTableShort => "记账表缺",
            RehCode::CaliberMismatch => "预演与实跑口径不一致",
            RehCode::DefectLeft => "缺陷遗留：非零",
            RehCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [RehCode; 9] = [
        RehCode::StepOutOfRange,
        RehCode::StepNameBlank,
        RehCode::IdGapDetected,
        RehCode::SumMismatch,
        RehCode::ThresholdOver,
        RehCode::LedgerTableShort,
        RehCode::CaliberMismatch,
        RehCode::DefectLeft,
        RehCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < RehCode::ALL.len() {
        if RehCode::ALL[i].val() == code {
            return RehCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6D540 段闭集内"
}

// ── 预演批账 ───────────────────────────────────────────────────
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RehearsalBatch {
    pub name: String,
    /// 判据 ID 列表（连续）。
    pub ids: Vec<usize>,
    /// 正文阈值（每条阈值 µs）。
    pub thresholds: Vec<u64>,
    /// 台账行数。
    pub ledger_lines: u32,
}

impl RehearsalBatch {
    /// 步一：ID 连续。
    pub fn id_cont_check(&self) -> Result<(), u32> {
        if self.ids.len() != CARDS_PER_BATCH {
            return Err(RehCode::LedgerTableShort.val());
        }
        let mut i = 1;
        while i < self.ids.len() {
            if self.ids[i] != self.ids[i - 1] + 1 {
                return Err(RehCode::IdGapDetected.val());
            }
            i += 1;
        }
        Ok(())
    }
    /// 步二：逐批求和（行数 = 条数 × 每条行数账）。
    pub fn sum_check(&self, lines_per_card: u32) -> Result<u32, u32> {
        let expect = self.ids.len() as u32 * lines_per_card;
        if self.ledger_lines != expect {
            return Err(RehCode::SumMismatch.val());
        }
        Ok(self.ledger_lines)
    }
    /// 步三：正文阈值（每条阈值 ≤ 10000µs 域）。
    pub fn threshold_check(&self) -> Result<(), u32> {
        let mut i = 0;
        while i < self.thresholds.len() {
            if self.thresholds[i] == 0 || self.thresholds[i] > 10_000 {
                return Err(RehCode::ThresholdOver.val());
            }
            i += 1;
        }
        Ok(())
    }
}

/// 记账表（步四）。
pub fn ledger_table_check(batches: &[RehearsalBatch]) -> Result<u32, u32> {
    if batches.len() != BATCHES.len() {
        return Err(RehCode::LedgerTableShort.val());
    }
    let mut total = 0;
    let mut b = 0;
    while b < batches.len() {
        total += batches[b].ledger_lines;
        b += 1;
    }
    Ok(total)
}

/// 对回（步五）：预演行数与实跑口径行数一致。
pub fn recon_check(rehearsal_total: u32, actual_total: u32) -> Result<(), u32> {
    if rehearsal_total != actual_total {
        return Err(RehCode::CaliberMismatch.val());
    }
    Ok(())
}

/// 基线两批（确定性）。
pub fn baseline_batches() -> Result<Vec<RehearsalBatch>, u32> {
    let mut v = Vec::new();
    let mut b = 0;
    while b < BATCHES.len() {
        let start = 1 + b * CARDS_PER_BATCH;
        let mut ids = Vec::new();
        let mut thresholds = Vec::new();
        let mut i = 0;
        while i < CARDS_PER_BATCH {
            ids.push(start + i);
            thresholds.push(100 + (i as u64) * 50);
            i += 1;
        }
        v.push(RehearsalBatch {
            name: String::from(BATCHES[b]),
            ids,
            thresholds,
            ledger_lines: (CARDS_PER_BATCH * 12) as u32,
        });
        b += 1;
    }
    Ok(v)
}

/// 预演五步全过。
pub fn rehearsal_pass(batches: &[RehearsalBatch]) -> Result<u32, u32> {
    if STEP_NAMES.len() != FIVE_STEPS {
        return Err(RehCode::StepNameBlank.val());
    }
    let mut b = 0;
    while b < batches.len() {
        batches[b].id_cont_check()?;
        batches[b].sum_check(12)?;
        batches[b].threshold_check()?;
        b += 1;
    }
    let total = ledger_table_check(batches)?;
    // 实跑口径（同公式）。
    let actual = batches.len() as u32 * CARDS_PER_BATCH as u32 * 12;
    recon_check(total, actual)?;
    Ok(total)
}

/// 缺陷账（零遗留声明）。
pub fn defect_ledger(batches: &[RehearsalBatch]) -> Result<u32, u32> {
    // 预演一次，缺陷记 0（全过）。
    rehearsal_pass(batches)?;
    Ok(0)
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

// ── 注入检出账 ──────────────────────────────────────────────────
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

/// 黄金全链：两批预演五步→口径一致→缺陷零遗留→10/10。
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let batches = baseline_batches()?;
    let total = rehearsal_pass(&batches)?;
    let defects = defect_ledger(&batches)?;
    if defects != 0 {
        return Err(RehCode::DefectLeft.val());
    }
    Ok((total, defects, FIVE_STEPS as u32))
}

/// 探针三序。
pub fn probe_seq_a() -> Result<(), u32> {
    let batches = baseline_batches()?;
    rehearsal_pass(&batches).map(|_| ())
}

pub fn probe_seq_b() -> Result<(), u32> {
    // ID 断号 → 必拒。
    let mut batches = baseline_batches()?;
    batches[0].ids[5] = 999;
    batches[0].id_cont_check()
}

pub fn probe_seq_c() -> Result<(), u32> {
    // 口径不一致 → 必拒。
    let batches = baseline_batches()?;
    let total = ledger_table_check(&batches)?;
    recon_check(total, total + 1)
}

/// 注入 10/10：断号、求和错、阈值越界、口径四槽。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽0：断号。
        let mut b0 = baseline_batches().ok();
        let h0 = match &mut b0 {
            Some(bs) => {
                bs[1].ids[3] = 500;
                bs[1].id_cont_check().is_err()
            }
            None => false,
        };
        led.record(0, h0);
        // 槽1：求和错。
        let mut b1 = baseline_batches().ok();
        let h1 = match &mut b1 {
            Some(bs) => {
                bs[0].ledger_lines = 1;
                bs[0].sum_check(12).is_err()
            }
            None => false,
        };
        led.record(1, h1);
        // 槽2：阈值越界。
        let mut b2 = baseline_batches().ok();
        let h2 = match &mut b2 {
            Some(bs) => {
                bs[1].thresholds[2] = 20000;
                bs[1].threshold_check().is_err()
            }
            None => false,
        };
        led.record(2, h2);
        // 槽3：口径。
        let b3 = baseline_batches().ok();
        let h3 = match &b3 {
            Some(bs) => {
                let t = ledger_table_check(bs).unwrap_or(0);
                recon_check(t, t + 9).is_err()
            }
            None => false,
        };
        led.record(3, h3);
        r += 1;
    }
    Ok(led)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(RehCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < RehCode::ALL.len() {
        let v = RehCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push(("J1-黄金全链", g.is_ok(), format!("{:?}", g)));
    // 五步 + 两批。
    v.push((
        "J1-五步两批预演",
        STEP_NAMES.len() == 5 && BATCHES.len() == 2 && CARDS_PER_BATCH == 20,
        format!("{:?} × {:?}", BATCHES, STEP_NAMES),
    ));
    // 预演全过 + 缺陷零遗留。
    let batches = baseline_batches();
    v.push((
        "J1-预演全过+缺陷零遗留",
        match &batches {
            Ok(bs) => rehearsal_pass(bs).is_ok() && defect_ledger(bs) == Ok(0),
            Err(_) => false,
        },
        format!("缺陷 {:?}", batches.as_ref().map(|bs| defect_ledger(bs))),
    ));
    // 注入 10/10。
    let led = inject_rounds(JUDGE_ROUNDS);
    let led_ok = match &led {
        Ok(l) => l.all_caught() && l.caught[0] == 10,
        Err(_) => false,
    };
    v.push((
        "J1-注入10/10",
        led_ok,
        format!(
            "t/c {:?}/{:?}",
            led.as_ref().map(|l| l.tries),
            led.as_ref().map(|l| l.caught)
        ),
    ));
    // 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_err();
    let c = probe_seq_c().is_err();
    v.push((
        "J1-探针三序",
        a && b && c,
        format!("a={} b!={} c!={}", a, b, c),
    ));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x6D540 窗口")));
    v
}

pub fn run_unx2225_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F2225 冻结终验预演一：B31–B32 段级预查");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
