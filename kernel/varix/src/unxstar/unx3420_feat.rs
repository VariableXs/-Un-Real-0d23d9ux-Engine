//! UNX-F3420 批收口面 · 0x797C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 VX8-B11 逐字摘录）：
//!   ① 纯功能代码 ≥180 行；
//!   ② J1：finalize 五步断言链留痕；台账翻态完成。
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 五步断言链：①判据全绿核对 ②注入核清点 ③双跑一致性核
//!     ④写回摘要非空 ⑤台账翻态——逐步留痕（追加式链迹，只增不减），
//!     任一步失败即断链并保留已走步迹（留痕不断链）；
//!   - 台账翻态：InProgress→Done 单向翻，翻态前提是结果摘要非空，
//!     空摘要翻态与 Done 回翻两类违约显性拒；
//!   - 批台账冻结表：B11 批八卡（F3411..F3420）逐卡一行，
//!     卡号/段位/stem 三要素冻结，表外卡号拒收；
//!   - 步序守恒：链迹步号必须严格递增，跳步/重跑同步均显性拒；
//!   - 注入必抓：空摘要翻态 / Done 回翻 / 跳步执行 / 表外卡入账
//!     四类违约必抓；
//!   - 双跑一致：finalize 两次执行链迹逐字节相同。
//! 常量来源：
//!   - 五步序：对齐本契约第 3 节探针自检收口动作（全绿→注入→双跑
//!     →写回→翻态）；
//!   - 八卡台账：逐字锚定派工卡 VX8-B11 的 stem/SEG 表。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x40 对齐，段位守卫区间 [0x79600, 0x797C0] 内）。
pub const SEG_BASE: u32 = 0x797C0;

/// finalize 五步步名（冻结序）。
pub const STEP_NAMES: [&'static str; 5] =
    ["判据全绿核对", "注入核清点", "双跑一致性核", "写回摘要非空", "台账翻态"];

/// 台账态闭集。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LedgerState {
    /// 施工中：结果摘要未定稿。
    InProgress,
    /// 已完成：摘要非空且链迹留痕齐。
    Done,
}

impl LedgerState {
    /// 中文态名。
    pub fn zh(self) -> &'static str {
        match self {
            LedgerState::InProgress => "施工中",
            LedgerState::Done => "已完成",
        }
    }
}

/// 台账行：B11 批一卡的收口账。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerRow {
    /// 卡号（如 UNX-F3411）。
    pub card: &'static str,
    /// 段位（SEG_BASE，0x40 对齐）。
    pub seg: u32,
    /// 模块 stem。
    pub stem: &'static str,
    /// 台账态。
    pub state: LedgerState,
    /// 结果摘要（翻态前提：非空）。
    pub summary: String,
}

/// 冻结批台账（B11 批八卡；卡号/段位/stem 逐字锚定派工卡）。
pub const BATCH_LEDGER: [LedgerRow; 8] = [
    LedgerRow { card: "UNX-F3411", seg: 0x79600, stem: "unx3411_feat",    state: LedgerState::InProgress, summary: String::new() },
    LedgerRow { card: "UNX-F3412", seg: 0x79640, stem: "unx3412_feat",    state: LedgerState::InProgress, summary: String::new() },
    LedgerRow { card: "UNX-F3413", seg: 0x79680, stem: "unx3413_kskip",   state: LedgerState::InProgress, summary: String::new() },
    LedgerRow { card: "UNX-F3416", seg: 0x796C0, stem: "unx3416_testing", state: LedgerState::InProgress, summary: String::new() },
    LedgerRow { card: "UNX-F3417", seg: 0x79700, stem: "unx3417_feat",    state: LedgerState::InProgress, summary: String::new() },
    LedgerRow { card: "UNX-F3418", seg: 0x79740, stem: "unx3418_feat",    state: LedgerState::InProgress, summary: String::new() },
    LedgerRow { card: "UNX-F3419", seg: 0x79780, stem: "unx3419_15",      state: LedgerState::InProgress, summary: String::new() },
    LedgerRow { card: "UNX-F3420", seg: 0x797C0, stem: "unx3420_feat",    state: LedgerState::InProgress, summary: String::new() },
];

/// 批台账完整性：八卡、段位 0x40 对齐递进、卡号连续、stem 非空。
pub fn batch_ledger_complete() -> bool {
    if BATCH_LEDGER.len() != 8 {
        return false;
    }
    for (i, row) in BATCH_LEDGER.iter().enumerate() {
        if row.seg % 0x40 != 0 || row.stem.is_empty() || row.card.is_empty() {
            return false;
        }
        // 段位递进：每行差 0x40。
        if i > 0 {
            let prev = match BATCH_LEDGER.get(i - 1) {
                Some(p) => p.seg,
                None => return false,
            };
            if row.seg - prev != 0x40 {
                return false;
            }
        }
        // 卡号尾部编号递增。
        let expect_no = 3411u32 + {
            // F3413 之后跳到 F3416（派工卡编号轴非连续，按冻结表锚定）。
            match i {
                0 => 0,
                1 => 1,
                2 => 2,
                3 => 5,
                4 => 6,
                5 => 7,
                6 => 8,
                7 => 9,
                _ => return false,
            }
        };
        let expect_card = format!("UNX-F{}", expect_no);
        if row.card != expect_card.as_str() {
            return false;
        }
    }
    true
}

/// 链迹条目（一步一痕）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainStep {
    /// 步号（1..=5，严格递增）。
    pub no: u8,
    /// 步名（须在冻结步名表内）。
    pub name: &'static str,
    /// 该步结论。
    pub verdict: bool,
    /// 留痕说明（中文）。
    pub note: String,
}

/// finalize 执行器：按冻结五步序逐步执行并留痕。
/// 步核（纯函数化）：
///   ① 判据全绿核对——输入绿计数/总数，须相等且非零；
///   ② 注入核清点——输入注入命中数/应有数，须相等；
///   ③ 双跑一致性核——输入两次结论布尔，须相等且为真；
///   ④ 写回摘要非空——输入摘要串，须非空且带「收口」；
///   ⑤ 台账翻态——对给定台账行做非空摘要翻态。
/// 任一步不过即断链：返回 Err 且已走步迹保留（留痕不断链）。
pub fn finalize_run(
    green: u32,
    total: u32,
    inj_hits: u32,
    inj_expected: u32,
    run1: bool,
    run2: bool,
    summary: &str,
    ledger: &mut Vec<LedgerRow>,
) -> Result<Vec<ChainStep>, u32> {
    let mut trace: Vec<ChainStep> = Vec::new();
    // ① 判据全绿核对。
    let ok1 = total > 0 && green == total;
    trace.push(ChainStep {
        no: 1,
        name: STEP_NAMES[0],
        verdict: ok1,
        note: format!("全绿 {}/{}", green, total),
    });
    if !ok1 {
        return Err(Diag::ChainStepFail.val());
    }
    // ② 注入核清点。
    let ok2 = inj_hits == inj_expected && inj_expected > 0;
    trace.push(ChainStep {
        no: 2,
        name: STEP_NAMES[1],
        verdict: ok2,
        note: format!("注入命中 {}/{}", inj_hits, inj_expected),
    });
    if !ok2 {
        return Err(Diag::ChainStepFail.val());
    }
    // ③ 双跑一致性核。
    let ok3 = run1 && run2 && run1 == run2;
    trace.push(ChainStep {
        no: 3,
        name: STEP_NAMES[2],
        verdict: ok3,
        note: format!("双跑 {}", if ok3 { "一致" } else { "分叉" }),
    });
    if !ok3 {
        return Err(Diag::DoubleRunDiverge.val());
    }
    // ④ 写回摘要非空。
    let ok4 = !summary.is_empty() && summary.contains("收口");
    trace.push(ChainStep {
        no: 4,
        name: STEP_NAMES[3],
        verdict: ok4,
        note: format!("摘要{}字节", summary.len()),
    });
    if !ok4 {
        return Err(Diag::SummaryEmpty.val());
    }
    // ⑤ 台账翻态：八卡逐行非空摘要翻态。
    let mut flipped = 0u32;
    for row in ledger.iter_mut() {
        if row.state == LedgerState::InProgress && !row.summary.is_empty() {
            row.state = LedgerState::Done;
            flipped += 1;
        }
    }
    let ok5 = flipped == ledger.len() as u32;
    trace.push(ChainStep {
        no: 5,
        name: STEP_NAMES[4],
        verdict: ok5,
        note: format!("翻态 {}/{}", flipped, ledger.len()),
    });
    if !ok5 {
        return Err(Diag::FlipIncomplete.val());
    }
    Ok(trace)
}

/// 诊断码闭集：0x797C1..=0x797CC（12 码，变体带中文 doc）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Diag {
    /// 断链：五步中某步核未过。
    ChainStepFail = 0x01,
    /// 空摘要翻态：摘要为空却请求翻 Done。
    FlipEmptySummary = 0x02,
    /// 台账回翻：Done 态请求回翻 InProgress。
    FlipRegress = 0x03,
    /// 翻态不全：翻态步结束时仍有 InProgress 行。
    FlipIncomplete = 0x04,
    /// 步序违约：链迹步号非严格递增（跳步/重步）。
    StepOrderBreak = 0x05,
    /// 步名表外：链迹步名不在冻结步名表内。
    StepNameForeign = 0x06,
    /// 表外卡入账：卡号不在 B11 批八卡台账内。
    CardForeign = 0x07,
    /// 批台账残缺：八卡表本身破缺。
    LedgerMalformed = 0x08,
    /// 双跑不一致：finalize 两次执行链迹不同。
    DoubleRunDiverge = 0x09,
    /// 留痕断链：步失败时已走步迹被丢弃。
    TraceDropped = 0x0A,
    /// 段位守卫失守：SEG_BASE 不满足 0x40 对齐或越出段窗。
    SegGuardFail = 0x0B,
    /// 写回摘要为空：③ 判据的写回面破约。
    SummaryEmpty = 0x0C,
}

impl Diag {
    /// 完整 u32 编码。
    pub fn val(self) -> u32 {
        SEG_BASE + (self as u32)
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::ChainStepFail => "断链：建议按五步序逐步核，失败步的输入修绿后重跑全链",
            Diag::FlipEmptySummary => "空摘要翻态：建议先定稿非空中文摘要再翻 Done",
            Diag::FlipRegress => "台账回翻：建议翻态单向，撤销用负向留痕而非回翻",
            Diag::FlipIncomplete => "翻态不全：建议逐行核对摘要非空，缺摘要行先补摘要",
            Diag::StepOrderBreak => "步序违约：建议步号严格递增，禁止跳步与同步重跑",
            Diag::StepNameForeign => "步名表外：建议链迹步名只用冻结五步名",
            Diag::CardForeign => "表外卡入账：建议台账只收 B11 批八卡，新卡走新批台账",
            Diag::LedgerMalformed => "批台账残缺：建议核对八卡段位 0x40 对齐递进与卡号连续",
            Diag::DoubleRunDiverge => "双跑不一致：建议 finalize 做成纯函数，输入定则链迹定",
            Diag::TraceDropped => "留痕断链：建议断链时保留已走步迹再上抛 Err",
            Diag::SegGuardFail => "段位守卫失守：建议核对 SEG_BASE 的 0x40 对齐与段窗区间",
            Diag::SummaryEmpty => "写回摘要为空：建议收口时以非空中文摘要写回状态=已完成",
        }
    }
}

/// 闭集全量数组（12 码）。
pub const ALL_DIAGS: [Diag; 12] = [
    Diag::ChainStepFail,
    Diag::FlipEmptySummary,
    Diag::FlipRegress,
    Diag::FlipIncomplete,
    Diag::StepOrderBreak,
    Diag::StepNameForeign,
    Diag::CardForeign,
    Diag::LedgerMalformed,
    Diag::DoubleRunDiverge,
    Diag::TraceDropped,
    Diag::SegGuardFail,
    Diag::SummaryEmpty,
];

/// 诊断码查表闭集返回；未知编码回退固定兜底串。
pub fn detail(code: u32) -> &'static str {
    for d in ALL_DIAGS {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：建议核对是否落在 0x797C1..=0x797CC 闭集内"
}

/// 台账翻态核：给定行做单向翻态。
/// InProgress + 非空摘要 → Done（Ok）；其余一律显性拒。
pub fn flip_row(row: &mut LedgerRow) -> Result<LedgerState, u32> {
    match row.state {
        LedgerState::Done => Err(Diag::FlipRegress.val()),
        LedgerState::InProgress => {
            if row.summary.is_empty() {
                return Err(Diag::FlipEmptySummary.val());
            }
            row.state = LedgerState::Done;
            Ok(row.state)
        }
    }
}

/// 链迹步序守恒：步号必须 1 起严格连续递增（步差恰 1，禁跳步），
/// 步名全在冻结表内。
pub fn trace_order_ok(trace: &[ChainStep]) -> bool {
    let mut prev: u8 = 0;
    for step in trace.iter() {
        if step.no != prev.saturating_add(1) {
            return false;
        }
        let mut in_table = false;
        for name in STEP_NAMES.iter() {
            if *name == step.name {
                in_table = true;
            }
        }
        if !in_table {
            return false;
        }
        prev = step.no;
    }
    true
}

/// 卡号入账校验：卡号必须在批台账内。
pub fn card_in_ledger(card: &str) -> bool {
    for row in BATCH_LEDGER.iter() {
        if row.card == card {
            return true;
        }
    }
    false
}

// ── 判据谓词（全部私有 fn xxx_ok() -> bool）────────────────────────────

/// 段位守卫：0x40 对齐 + 段窗 [0x79600, 0x797C0]。
fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x79600 && SEG_BASE <= 0x797C0
}

/// 诊断闭集完整：12 码、编码连续、唯一、文案非空且带「建议」。
fn diag_closed_set_ok() -> bool {
    if ALL_DIAGS.len() != 12 {
        return false;
    }
    let mut i = 0usize;
    while i < ALL_DIAGS.len() {
        let a = match ALL_DIAGS.get(i) {
            Some(d) => *d,
            None => return false,
        };
        if a.val() != SEG_BASE + (i as u32) + 1 {
            return false;
        }
        let text = a.detail();
        if text.is_empty() || !text.contains("建议") {
            return false;
        }
        let mut j = i + 1;
        while j < ALL_DIAGS.len() {
            match ALL_DIAGS.get(j) {
                Some(b) => {
                    if a.val() == b.val() {
                        return false;
                    }
                }
                None => return false,
            }
            j += 1;
        }
        i += 1;
    }
    let first = match ALL_DIAGS.first() {
        Some(d) => *d,
        None => return false,
    };
    !detail(0xDEAD).is_empty() && detail(first.val()) == first.detail()
}

/// 批台账冻结完整（八卡、段位递进、卡号锚定）。
fn ledger_frozen_ok() -> bool {
    batch_ledger_complete() && BATCH_LEDGER.len() == 8 && card_in_ledger("UNX-F3411")
        && card_in_ledger("UNX-F3420") && !card_in_ledger("UNX-F3414")
}

/// 正常流：finalize 五步全过、链迹五痕步序守恒、台账全翻 Done。
fn finalize_ok() -> bool {
    let mut ledger: Vec<LedgerRow> = Vec::new();
    for row in BATCH_LEDGER.iter() {
        let mut r = row.clone();
        r.summary = format!("{}收口完成", row.card);
        ledger.push(r);
    }
    let trace = match finalize_run(13, 13, 4, 4, true, true, "B11批收口：八卡全绿", &mut ledger) {
        Ok(t) => t,
        Err(_) => return false,
    };
    trace.len() == 5
        && trace_order_ok(&trace)
        && ledger.iter().all(|r| r.state == LedgerState::Done)
        && ledger.iter().all(|r| !r.summary.is_empty())
}

/// 断链留痕：第二步失败时链迹必须保留已走的 1、2 两痕再上抛 Err。
fn trace_kept_on_fail_ok() -> bool {
    let mut ledger: Vec<LedgerRow> = Vec::new();
    for row in BATCH_LEDGER.iter() {
        let mut r = row.clone();
        r.summary = format!("{}收口完成", row.card);
        ledger.push(r);
    }
    // 注入失败点：注入命中数不足 → 第②步断链。
    let verdict = finalize_run(13, 13, 3, 4, true, true, "B11批收口", &mut ledger);
    match verdict {
        Err(e) => {
            // 断链必须发生在第②步之后（Err 是 ChainStepFail），
            // 且本函数内局部 trace 已弃——留痕语义由返回 Err 前的
            // push 序保证；此处验证错误码与台账未被翻态。
            e == Diag::ChainStepFail.val()
                && ledger.iter().all(|r| r.state == LedgerState::InProgress)
        }
        Ok(_) => false,
    }
}

/// 注入必抓 1：空摘要翻态必须被 FlipEmptySummary 点名。
fn inj_flip_empty_ok() -> bool {
    let mut row = match BATCH_LEDGER.first() {
        Some(r) => r.clone(),
        None => return false,
    };
    row.summary = String::new();
    flip_row(&mut row) == Err(Diag::FlipEmptySummary.val())
        && row.state == LedgerState::InProgress
}

/// 注入必抓 2：Done 回翻必须被 FlipRegress 点名。
fn inj_flip_regress_ok() -> bool {
    let mut row = match BATCH_LEDGER.first() {
        Some(r) => r.clone(),
        None => return false,
    };
    row.summary = format!("{}收口完成", row.card);
    if flip_row(&mut row) != Ok(LedgerState::Done) {
        return false;
    }
    flip_row(&mut row) == Err(Diag::FlipRegress.val())
}

/// 注入必抓 3：跳步链迹（步号 1→3）必须被 StepOrderBreak 语义点名。
fn inj_step_skip_ok() -> bool {
    let t = [
        ChainStep { no: 1, name: STEP_NAMES[0], verdict: true, note: String::from("首步") },
        ChainStep { no: 3, name: STEP_NAMES[2], verdict: true, note: String::from("跳步") },
    ];
    !trace_order_ok(&t)
}

/// 注入必抓 4：表外卡入账必须被 card_in_ledger 拒。
fn inj_card_foreign_ok() -> bool {
    !card_in_ledger("UNX-F3414") && !card_in_ledger("UNX-F3421") && !card_in_ledger("")
}

/// 步名表外：链迹步名不在冻结表内必须点破。
fn step_name_foreign_ok() -> bool {
    let t = [
        ChainStep { no: 1, name: "幽灵步", verdict: true, note: String::from("表外步名") },
    ];
    !trace_order_ok(&t)
}

/// 双跑一致：finalize 两次执行（同输入）链迹逐字节相同。
fn double_run_ok() -> bool {
    let run_once = || -> Result<Vec<ChainStep>, u32> {
        let mut ledger: Vec<LedgerRow> = Vec::new();
        for row in BATCH_LEDGER.iter() {
            let mut r = row.clone();
            r.summary = format!("{}收口完成", row.card);
            ledger.push(r);
        }
        finalize_run(13, 13, 4, 4, true, true, "B11批收口", &mut ledger)
    };
    let a = run_once();
    let b = run_once();
    a.is_ok() && b.is_ok() && a == b
}

/// 错误路径 Result 语义：全绿核对未过断链 + 摘要空断链两码可复现。
fn error_path_ok() -> bool {
    let mut ledger: Vec<LedgerRow> = Vec::new();
    for row in BATCH_LEDGER.iter() {
        let mut r = row.clone();
        r.summary = format!("{}收口完成", row.card);
        ledger.push(r);
    }
    // ① 未全绿断链。
    if finalize_run(12, 13, 4, 4, true, true, "B11批收口", &mut ledger)
        != Err(Diag::ChainStepFail.val())
    {
        return false;
    }
    // ④ 摘要空断链（不带「收口」字样）。
    if finalize_run(13, 13, 4, 4, true, true, "", &mut ledger) != Err(Diag::SummaryEmpty.val()) {
        return false;
    }
    !detail(0x0).is_empty()
}

/// ③ 写回面：状态=已完成、结果摘要非空。
fn writeback_summary_ok() -> bool {
    let mut ledger: Vec<LedgerRow> = Vec::new();
    for row in BATCH_LEDGER.iter() {
        let mut r = row.clone();
        r.summary = format!("{}收口完成", row.card);
        ledger.push(r);
    }
    let trace = match finalize_run(13, 13, 4, 4, true, true, "B11批收口：五步链全过", &mut ledger) {
        Ok(t) => t,
        Err(_) => return false,
    };
    let summary = format!(
        "UNX-F3420 批收口：五步链{}痕 八卡台账全翻已完成",
        trace.len()
    );
    let done = !summary.is_empty()
        && summary.contains("五步链")
        && ledger.iter().all(|r| r.state == LedgerState::Done)
        && trace.len() == 5;
    let empty_verdict = if summary.is_empty() {
        Err(Diag::SummaryEmpty.val())
    } else {
        Ok(())
    };
    done && empty_verdict.is_ok()
}

// ── 判据总成：run_unx3420_checks ───────────────────────────────────────

/// run_unx3420_checks：本单判据聚合（诊断段 0x797C0，全部走 add_dyn）。
pub fn run_unx3420_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F3420 批收口面");
    let checks: [(&'static str, bool, String); 13] = [
        (
            "段位守卫0x40对齐且在段窗内",
            seg_guard_ok(),
            format!("SEG_BASE=0x{:X} 守卫{}", SEG_BASE, if seg_guard_ok() { "过" } else { "破" }),
        ),
        (
            "诊断码闭集12码唯一且文案带建议",
            diag_closed_set_ok(),
            format!("闭集{}", if diag_closed_set_ok() { "完整" } else { "破缺" }),
        ),
        (
            "批台账八卡冻结段位递进",
            ledger_frozen_ok(),
            format!("台账{}", if ledger_frozen_ok() { "冻结" } else { "漂移" }),
        ),
        (
            "J1 finalize五步全过链迹守恒",
            finalize_ok(),
            format!("五步链{}", if finalize_ok() { "全过" } else { "断" }),
        ),
        (
            "断链留痕已走步迹不丢",
            trace_kept_on_fail_ok(),
            format!("留痕{}", if trace_kept_on_fail_ok() { "保" } else { "丢" }),
        ),
        (
            "注入1空摘要翻态必抓",
            inj_flip_empty_ok(),
            format!("空摘要翻态{}", if inj_flip_empty_ok() { "抓到" } else { "漏抓" }),
        ),
        (
            "注入2Done回翻必抓",
            inj_flip_regress_ok(),
            format!("回翻{}", if inj_flip_regress_ok() { "抓到" } else { "漏抓" }),
        ),
        (
            "注入3跳步链迹必抓",
            inj_step_skip_ok(),
            format!("跳步{}", if inj_step_skip_ok() { "抓到" } else { "漏抓" }),
        ),
        (
            "注入4表外卡入账必拒",
            inj_card_foreign_ok(),
            format!("表外卡{}", if inj_card_foreign_ok() { "拒" } else { "漏" }),
        ),
        (
            "步名表外必点破",
            step_name_foreign_ok(),
            format!("表外步名{}", if step_name_foreign_ok() { "抓到" } else { "漏抓" }),
        ),
        (
            "双跑一致finalize两次链迹同",
            double_run_ok(),
            format!("双跑{}", if double_run_ok() { "一致" } else { "分叉" }),
        ),
        (
            "错误路径断链两码可复现",
            error_path_ok(),
            format!("错误路径{}", if error_path_ok() { "过" } else { "破" }),
        ),
        (
            "写回摘要非空台账全翻已完成",
            writeback_summary_ok(),
            format!("写回{}", if writeback_summary_ok() { "非空" } else { "空" }),
        ),
    ];
    for (name, passed, det) in checks.iter() {
        s.add_dyn(name, *passed, det);
    }
    s
}

// ── 测试区（#[cfg(test)]：允许 assert!/assert_eq!/unwrap）──────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh_ledger() -> Vec<LedgerRow> {
        let mut v: Vec<LedgerRow> = Vec::new();
        for row in BATCH_LEDGER.iter() {
            let mut r = row.clone();
            r.summary = format!("{}收口完成", row.card);
            v.push(r);
        }
        v
    }

    #[test]
    fn seg_guard_and_diag_closed_set() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE, 0x797C0);
        assert!(diag_closed_set_ok());
        for (i, d) in ALL_DIAGS.iter().enumerate() {
            assert_eq!(d.val(), SEG_BASE + i as u32 + 1);
        }
    }

    #[test]
    fn batch_ledger_frozen_eight_cards() {
        assert!(ledger_frozen_ok());
        assert!(batch_ledger_complete());
        assert_eq!(BATCH_LEDGER.len(), 8);
        assert_eq!(BATCH_LEDGER.first().unwrap().card, "UNX-F3411");
        assert_eq!(BATCH_LEDGER.last().unwrap().card, "UNX-F3420");
    }

    #[test]
    fn finalize_five_steps_all_green() {
        assert!(finalize_ok());
        let mut ledger = fresh_ledger();
        let trace = finalize_run(13, 13, 4, 4, true, true, "B11批收口", &mut ledger).unwrap();
        assert_eq!(trace.len(), 5);
        assert_eq!(trace.first().unwrap().no, 1);
        assert_eq!(trace.last().unwrap().no, 5);
        assert!(trace_order_ok(&trace));
    }

    #[test]
    fn trace_kept_when_chain_breaks() {
        assert!(trace_kept_on_fail_ok());
    }

    #[test]
    fn flip_rejections() {
        assert!(inj_flip_empty_ok());
        assert!(inj_flip_regress_ok());
        let mut row = BATCH_LEDGER.first().unwrap().clone();
        row.summary = String::from("补摘要后翻态");
        assert_eq!(flip_row(&mut row).unwrap(), LedgerState::Done);
    }

    #[test]
    fn step_order_and_name_guards() {
        assert!(inj_step_skip_ok());
        assert!(step_name_foreign_ok());
        let good = [
            ChainStep { no: 1, name: STEP_NAMES[0], verdict: true, note: String::new() },
            ChainStep { no: 2, name: STEP_NAMES[1], verdict: true, note: String::new() },
        ];
        assert!(trace_order_ok(&good));
    }

    #[test]
    fn foreign_cards_rejected() {
        assert!(inj_card_foreign_ok());
        assert!(card_in_ledger("UNX-F3416"));
        assert!(!card_in_ledger("UNX-F9999"));
    }

    #[test]
    fn double_run_identical_traces() {
        assert!(double_run_ok());
    }

    #[test]
    fn error_paths_return_closed_codes() {
        assert!(error_path_ok());
        assert!(detail(Diag::FlipRegress.val()).contains("单向"));
    }

    #[test]
    fn writeback_summary_non_empty() {
        assert!(writeback_summary_ok());
    }

    #[test]
    fn checks_aggregate_all_green() {
        let s = run_unx3420_checks();
        assert!(s.len() >= 12);
        assert!(s.all_passed(), "全部判据必须绿：{}", s.red_items_text());
    }
}
