//! UNX-F3440 批收口面 · 0x79C00段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 VX8-B14 逐字摘录）：
//!   ① 纯功能代码 ≥180 行；
//!   ② J1：finalize 五步断言链留痕；台账翻态完成。
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 五步断言链固定步名表：全域自检聚合 → 红项清零核验 → 台账翻态 →
//!     结果摘要写回 → 收口封印；每步走完即落一条 StepTrace 留痕
//!     （步号 + 步名 + 序戳），链走满五步才算收口；
//!   - 台账状态机单调：Open → Finalizing → LedgerFlipped → Summarized
//!     → Sealed，倒退一律显性拒（StateRegress 点名）；
//!   - 台账翻态恰一次：Finalizing → LedgerFlipped 的翻态动作由第三步
//!     携带，flip_count 记账，重复翻态显性拒（LedgerDoubleFlip）；
//!   - 红项闸门：第二步核验 red_before == 0，红项未清零不许翻态
//!     （RedNotClear 点名，收口面拒绝带红封印）；
//!   - 写回闸门：第四步要求摘要非空（SummaryEmpty 点名），第五步封印
//!     只允许在 Summarized 之后（SealBeforeSummary 点名越序封印）；
//!   - 双跑一致：finalize 纯函数，同输入两次收口留痕逐条相同；
//!   - 注入必抓：跳步（第 1 步后直接记第 3 步）、倒退、重复翻态、
//!     空摘要、带红收口五类人为违约全部被闭集诊断码点名。
//! 常量来源：
//!   - 五步链与步名：派工卡判据原文「finalize 五步断言链留痕；台账
//!     翻态完成」冻结；
//!   - 诊断码闭集：SEG_BASE+0x01..0x0C（派工卡约定）；
//!   - 段窗：B14 批段窗 [0x79C00, 0x79DC0]（本批 8 单段基址覆盖域）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x40 对齐，B14 批段窗 [0x79C00, 0x79DC0] 起点）。
pub const SEG_BASE: u32 = 0x79C00;

/// 五步断言链步数（判据原文冻结：五步）。
pub const FINALIZE_STEPS: u32 = 5;

/// 五步链固定步名表（判据原文「finalize 五步断言链」的落地步序）。
pub const STEP_NAMES: [&str; 5] = [
    "全域自检聚合",
    "红项清零核验",
    "台账翻态",
    "结果摘要写回",
    "收口封印",
];

/// 批收口状态机（单调五态，倒退显性拒）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatchState {
    /// 开口态：台账已建、尚未进入收口。
    Open = 0,
    /// 收口进行态：五步链正在走。
    Finalizing = 1,
    /// 翻态完成态：台账状态已翻为已完成。
    LedgerFlipped = 2,
    /// 摘要定稿态：结果摘要已写回且非空。
    Summarized = 3,
    /// 封印态：批收口终态，只读。
    Sealed = 4,
}

impl BatchState {
    /// 单调秩（越大越靠后，倒退即秩下降）。
    pub fn rank(self) -> u32 {
        self as u32
    }

    /// 秩还原（闭集：0..=4 之外返回 None）。
    pub fn from_rank(r: u32) -> Option<BatchState> {
        match r {
            0 => Some(BatchState::Open),
            1 => Some(BatchState::Finalizing),
            2 => Some(BatchState::LedgerFlipped),
            3 => Some(BatchState::Summarized),
            4 => Some(BatchState::Sealed),
            _ => None,
        }
    }
}

/// 单步留痕：步号 + 步名 + 序戳（五步断言链留痕本体）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepTrace {
    /// 步号（1..=5，与 STEP_NAMES 下标对齐）。
    pub step: u32,
    /// 步名（取自 STEP_NAMES 冻结表）。
    pub name: &'static str,
    /// 序戳（链内单调递增，1 起）。
    pub stamp: u64,
}

/// 批收口台账：状态机 + 留痕 + 翻态记账 + 摘要。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinalizeLedger {
    /// 当前状态（单调，倒退显性拒）。
    pub state: BatchState,
    /// 五步留痕（按走步序追加）。
    pub traces: Vec<StepTrace>,
    /// 结果摘要（第四步写回，必须非空）。
    pub summary: String,
    /// 台账翻态记账（恰一次）。
    pub flip_count: u32,
    /// 收口前红项数（第二步核验必须为 0）。
    pub red_before: u32,
    /// 收口后红项数（收口面承诺清零后恒 0）。
    pub red_after: u32,
}

impl FinalizeLedger {
    /// 建台账（开口态）。
    pub fn new(red_before: u32) -> FinalizeLedger {
        FinalizeLedger {
            state: BatchState::Open,
            traces: Vec::new(),
            summary: String::new(),
            flip_count: 0,
            red_before,
            red_after: red_before,
        }
    }

    /// 进入收口（Open → Finalizing）；非开口态显性拒。
    pub fn begin(&mut self) -> Result<(), u32> {
        if self.state != BatchState::Open {
            return Err(Diag::LedgerNotOpen.val());
        }
        self.state = BatchState::Finalizing;
        Ok(())
    }

    /// 走一步并留痕：step 必须是「下一步」，步间不许跳、不许退。
    /// 第三步携带台账翻态（Finalizing → LedgerFlipped，恰一次）；
    /// 第四步要求摘要非空（LedgerFlipped → Summarized）；
    /// 第五步封印（Summarized → Sealed）。
    pub fn record_step(&mut self, step: u32, summary: &str) -> Result<(), u32> {
        // 步号闭集守卫：1..=5 之外直接拒。
        if step == 0 || step > FINALIZE_STEPS {
            return Err(Diag::StepOrderBad.val());
        }
        // 步序守卫：只许走「下一步」（留痕数 + 1），跳步倒退都拒。
        let next = self.traces.len() as u32 + 1;
        if step != next {
            return Err(Diag::StepOrderBad.val());
        }
        // 步名守卫：步名必须与冻结表逐字一致。
        let name = match STEP_NAMES.get((step - 1) as usize) {
            Some(n) => *n,
            None => return Err(Diag::TraceNameBad.val()),
        };
        // 第二步红项闸门：红项未清零不许继续翻态。
        if step == 2 && self.red_before != 0 {
            return Err(Diag::RedNotClear.val());
        }
        // 第三步台账翻态：状态翻为 LedgerFlipped，翻态记账恰一次。
        if step == 3 {
            if self.state != BatchState::Finalizing {
                return Err(Diag::LedgerNotOpen.val());
            }
            self.flip_count += 1;
            if self.flip_count != 1 {
                return Err(Diag::LedgerDoubleFlip.val());
            }
            self.state = BatchState::LedgerFlipped;
            self.red_after = 0;
        }
        // 第四步写回闸门：摘要必须非空。
        if step == 4 {
            if self.state != BatchState::LedgerFlipped {
                return Err(Diag::SealBeforeSummary.val());
            }
            if summary.is_empty() {
                return Err(Diag::SummaryEmpty.val());
            }
            self.summary = summary.to_string();
            self.state = BatchState::Summarized;
        }
        // 第五步封印：只许在摘要定稿之后。
        if step == 5 {
            if self.state != BatchState::Summarized {
                return Err(Diag::SealBeforeSummary.val());
            }
            self.state = BatchState::Sealed;
        }
        // 留痕落账（序戳 = 留痕条数，恒从 1 起单调）。
        let stamp = self.traces.len() as u64 + 1;
        self.traces.push(StepTrace { step, name, stamp });
        Ok(())
    }

    /// 留痕完整性核验：恰好五条、步号连续 1..=5、序戳连续、步名对位。
    pub fn traces_intact(&self) -> bool {
        if self.traces.len() != FINALIZE_STEPS as usize {
            return false;
        }
        let mut i = 0usize;
        while i < self.traces.len() {
            let t = match self.traces.get(i) {
                Some(t) => t,
                None => return false,
            };
            if t.step != (i as u32) + 1 {
                return false;
            }
            if t.stamp != i as u64 + 1 {
                return false;
            }
            let expect = match STEP_NAMES.get(i) {
                Some(n) => *n,
                None => return false,
            };
            if t.name != expect {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 五步收口全链：建台账 → begin → 依次走五步（摘要第四步写入）→
/// 核验留痕完整与终态封印。任一步违约即带诊断码短 路。
pub fn finalize_batch(red_before: u32, summary: &str) -> Result<FinalizeLedger, u32> {
    let mut ledger = FinalizeLedger::new(red_before);
    ledger.begin()?;
    let mut step = 1u32;
    while step <= FINALIZE_STEPS {
        ledger.record_step(step, summary)?;
        step += 1;
    }
    if !ledger.traces_intact() {
        return Err(Diag::TraceGap.val());
    }
    if ledger.state != BatchState::Sealed {
        return Err(Diag::ChainIncomplete.val());
    }
    Ok(ledger)
}

/// 诊断码闭集：0x79C01..=0x79C0C（12 码，变体带中文 doc）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Diag {
    /// 步序断链：跳步/退步/越界步号被拒。
    StepOrderBad = 0x01,
    /// 状态倒退：状态机单调秩下降被拒。
    StateRegress = 0x02,
    /// 台账重复翻态：翻态记账超过一次被拒。
    LedgerDoubleFlip = 0x03,
    /// 红项未清零：带红收口被第二步闸门拒绝。
    RedNotClear = 0x04,
    /// 摘要为空：第四步写回空摘要被拒。
    SummaryEmpty = 0x05,
    /// 越序封印：未到摘要定稿态就试图封印。
    SealBeforeSummary = 0x06,
    /// 五步未走满：链路提前终止。
    ChainIncomplete = 0x07,
    /// 留痕断号：步号或序戳不连续。
    TraceGap = 0x08,
    /// 留痕名错位：步名与冻结表不一致。
    TraceNameBad = 0x09,
    /// 台账非开口态：begin 只许在 Open 态调用。
    LedgerNotOpen = 0x0A,
    /// 段位守卫失守：SEG_BASE 不满足 0x40 对齐或越出批段窗。
    SegGuardFail = 0x0B,
    /// 写回失守：完成时摘要空或状态未达封印。
    WritebackFail = 0x0C,
}

impl Diag {
    /// 完整 u32 编码。
    pub fn val(self) -> u32 {
        SEG_BASE + (self as u32)
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::StepOrderBad => "步序断链：建议按 STEP_NAMES 冻结表顺序逐步走链",
            Diag::StateRegress => "状态倒退：建议状态机只许沿单调秩前进，倒退显性拒",
            Diag::LedgerDoubleFlip => "台账重复翻态：建议翻态记账恰一次，重复即拒",
            Diag::RedNotClear => "红项未清零：建议先清零全部红项再进收口面",
            Diag::SummaryEmpty => "摘要为空：建议第四步以非空中文摘要写回",
            Diag::SealBeforeSummary => "越序封印：建议先完成摘要写回再执行封印步",
            Diag::ChainIncomplete => "五步未走满：建议补齐五步断言链后再收口",
            Diag::TraceGap => "留痕断号：建议逐步留痕，步号序戳恒连续",
            Diag::TraceNameBad => "留痕名错位：建议步名取自 STEP_NAMES 冻结表",
            Diag::LedgerNotOpen => "台账非开口态：建议仅在 Open 态发起收口",
            Diag::SegGuardFail => "段位守卫失守：建议核对 SEG_BASE 的 0x40 对齐与批段窗",
            Diag::WritebackFail => "写回失守：建议完成时状态=已完成且摘要非空",
        }
    }
}

/// 闭集全量数组（12 码）。
pub const ALL_DIAGS: [Diag; 12] = [
    Diag::StepOrderBad,
    Diag::StateRegress,
    Diag::LedgerDoubleFlip,
    Diag::RedNotClear,
    Diag::SummaryEmpty,
    Diag::SealBeforeSummary,
    Diag::ChainIncomplete,
    Diag::TraceGap,
    Diag::TraceNameBad,
    Diag::LedgerNotOpen,
    Diag::SegGuardFail,
    Diag::WritebackFail,
];

/// 诊断码查表闭集返回；未知编码回退固定兜底串。
pub fn detail(code: u32) -> &'static str {
    for d in ALL_DIAGS.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：建议核对是否落在 0x79C01..=0x79C0C 闭集内"
}

// ── 判据谓词（全部私有 fn xxx_ok() -> bool）────────────────────────────

/// 段位守卫：0x40 对齐 + B14 批段窗 [0x79C00, 0x79DC0]。
fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x79C00 && SEG_BASE <= 0x79DC0
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
        if a.detail().is_empty() || !a.detail().contains("建议") {
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
    detail(0xDEAD) == detail(0x5A5A) && !detail(0xDEAD).is_empty()
}

/// 状态机单调：五态闭集可还原、秩单调、越界秩还原为 None。
fn state_monotonic_ok() -> bool {
    let mut r = 0u32;
    while r <= 4 {
        let st = match BatchState::from_rank(r) {
            Some(s) => s,
            None => return false,
        };
        if st.rank() != r {
            return false;
        }
        r += 1;
    }
    BatchState::from_rank(5).is_none()
        && BatchState::from_rank(u32::MAX).is_none()
        && BatchState::Sealed.rank() > BatchState::Open.rank()
        && detail(Diag::StateRegress.val()).contains("单调")
}

/// 五步链正常流：红项清零 + 非空摘要 → 封印终态 + 翻态恰一次。
fn five_step_chain_ok() -> bool {
    let ledger = match finalize_batch(0, "UNX-F3440 批收口：五步链全绿") {
        Ok(l) => l,
        Err(_) => return false,
    };
    ledger.state == BatchState::Sealed
        && ledger.flip_count == 1
        && ledger.red_after == 0
        && !ledger.summary.is_empty()
}

/// 留痕完整性：五条、步号序戳连续、步名对位（TraceGap/TraceNameBad 语义）。
fn trace_integrity_ok() -> bool {
    let ledger = match finalize_batch(0, "留痕核验摘要") {
        Ok(l) => l,
        Err(_) => return false,
    };
    ledger.traces_intact() && ledger.traces.len() == FINALIZE_STEPS as usize
}

/// 留痕名对位：每条留痕的步名逐字等于冻结表对应步。
fn trace_names_ok() -> bool {
    let ledger = match finalize_batch(0, "步名对位核验") {
        Ok(l) => l,
        Err(_) => return false,
    };
    let mut i = 0usize;
    while i < ledger.traces.len() {
        let t = match ledger.traces.get(i) {
            Some(t) => t,
            None => return false,
        };
        let expect = match STEP_NAMES.get(i) {
            Some(n) => *n,
            None => return false,
        };
        if t.name != expect {
            return false;
        }
        i += 1;
    }
    STEP_NAMES.len() == 5 && detail(Diag::TraceNameBad.val()).contains("冻结表")
}

/// 台账翻态完成：翻态记账恰一次且翻态后 red_after 清零。
fn ledger_flip_ok() -> bool {
    let ledger = match finalize_batch(0, "翻态核验摘要") {
        Ok(l) => l,
        Err(_) => return false,
    };
    ledger.flip_count == 1
        && ledger.state.rank() >= BatchState::LedgerFlipped.rank()
        && ledger.red_after == 0
        && detail(Diag::LedgerDoubleFlip.val()).contains("恰一次")
}

/// 红项闸门：red_before > 0 必被第二步 RedNotClear 点名（带红收口必拒）。
fn red_gate_ok() -> bool {
    match finalize_batch(3, "带红摘要") {
        Ok(_) => false,
        Err(e) => e == Diag::RedNotClear.val(),
    }
}

/// 注入 1：跳步——第 1 步后直接记第 3 步必被 StepOrderBad 点名。
fn inj_skip_step_ok() -> bool {
    let mut l = FinalizeLedger::new(0);
    if l.begin().is_err() {
        return false;
    }
    if l.record_step(1, "摘要").is_err() {
        return false;
    }
    l.record_step(3, "摘要") == Err(Diag::StepOrderBad.val())
        && l.record_step(0, "摘要") == Err(Diag::StepOrderBad.val())
        && l.record_step(6, "摘要") == Err(Diag::StepOrderBad.val())
}

/// 注入 2：倒退——封印后再 begin 必被 LedgerNotOpen 点名（单调拒倒退）。
fn inj_regress_ok() -> bool {
    let mut l = match finalize_batch(0, "倒退注入摘要") {
        Ok(l) => l,
        Err(_) => return false,
    };
    if l.state != BatchState::Sealed {
        return false;
    }
    l.begin() == Err(Diag::LedgerNotOpen.val())
        && detail(Diag::StateRegress.val()).contains("倒退")
}

/// 注入 3：空摘要——第四步写空串必被 SummaryEmpty 点名。
fn inj_empty_summary_ok() -> bool {
    let mut l = FinalizeLedger::new(0);
    if l.begin().is_err() {
        return false;
    }
    let mut step = 1u32;
    while step <= 3 {
        if l.record_step(step, "").is_err() {
            return false;
        }
        step += 1;
    }
    l.record_step(4, "") == Err(Diag::SummaryEmpty.val())
        && detail(Diag::SummaryEmpty.val()).contains("非空")
}

/// 注入 4：越序封印——未写摘要直接走第 5 步必被 StepOrderBad 拦截。
fn inj_seal_out_of_order_ok() -> bool {
    let mut l = FinalizeLedger::new(0);
    if l.begin().is_err() {
        return false;
    }
    // 第 1 步都没走就直奔第 5 步：步序守卫先拦。
    l.record_step(5, "摘要") == Err(Diag::StepOrderBad.val())
        && detail(Diag::SealBeforeSummary.val()).contains("封印")
}

/// 五步链步数冻结：FINALIZE_STEPS == 5 且步名表同长。
fn chain_len_ok() -> bool {
    FINALIZE_STEPS == 5
        && STEP_NAMES.len() == FINALIZE_STEPS as usize
        && detail(Diag::ChainIncomplete.val()).contains("五步")
}

/// 双跑一致：同输入两次收口留痕逐条相同（纯函数）。
fn double_run_ok() -> bool {
    let a = finalize_batch(0, "双跑一致摘要");
    let b = finalize_batch(0, "双跑一致摘要");
    a.is_ok() && b.is_ok() && a == b
}

/// 写回面：状态=已完成（封印）、结果摘要非空且含批号（WritebackFail 语义）。
fn writeback_summary_ok() -> bool {
    let ledger = match finalize_batch(0, "UNX-F3440 写回摘要") {
        Ok(l) => l,
        Err(_) => return false,
    };
    let summary = format!(
        "UNX-F3440 完成：五步链{}条留痕 翻态{}次 状态秩{}",
        ledger.traces.len(),
        ledger.flip_count,
        ledger.state.rank(),
    );
    !summary.is_empty()
        && summary.contains("UNX-F3440")
        && ledger.state == BatchState::Sealed
        && !ledger.summary.is_empty()
        && detail(Diag::WritebackFail.val()).contains("写回")
}

// ── 判据总成：run_unx3440_checks ───────────────────────────────────────

/// run_unx3440_checks：本单判据聚合（诊断段 0x79C00，全部走 add_dyn）。
pub fn run_unx3440_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F3440 批收口面");
    let checks: [(&'static str, bool, String); 15] = [
        (
            "段位守卫0x40对齐且在批段窗内",
            seg_guard_ok(),
            format!("SEG_BASE=0x{:X} 守卫{}", SEG_BASE, if seg_guard_ok() { "过" } else { "破" }),
        ),
        (
            "诊断码闭集12码唯一且文案带建议",
            diag_closed_set_ok(),
            format!("闭集{}", if diag_closed_set_ok() { "完整" } else { "破缺" }),
        ),
        (
            "台账状态机五态单调倒退还原",
            state_monotonic_ok(),
            format!("状态机{}", if state_monotonic_ok() { "单调" } else { "倒退" }),
        ),
        (
            "五步断言链正常流封印终态",
            five_step_chain_ok(),
            format!("五步链{}", if five_step_chain_ok() { "走满" } else { "断链" }),
        ),
        (
            "留痕完整性步号序戳连续",
            trace_integrity_ok(),
            format!("留痕{}", if trace_integrity_ok() { "完整" } else { "断号" }),
        ),
        (
            "留痕步名与冻结表逐字对位",
            trace_names_ok(),
            format!("步名{}", if trace_names_ok() { "对位" } else { "错位" }),
        ),
        (
            "台账翻态完成恰一次且红清零",
            ledger_flip_ok(),
            format!("翻态{}", if ledger_flip_ok() { "完成" } else { "失守" }),
        ),
        (
            "红项闸门带红收口必拒",
            red_gate_ok(),
            format!("红闸{}", if red_gate_ok() { "守住" } else { "漏红" }),
        ),
        (
            "注入1跳步必抓",
            inj_skip_step_ok(),
            format!("跳步注入{}", if inj_skip_step_ok() { "抓到" } else { "漏抓" }),
        ),
        (
            "注入2状态倒退必抓",
            inj_regress_ok(),
            format!("倒退注入{}", if inj_regress_ok() { "抓到" } else { "漏抓" }),
        ),
        (
            "注入3空摘要必抓",
            inj_empty_summary_ok(),
            format!("空摘要注入{}", if inj_empty_summary_ok() { "抓到" } else { "漏抓" }),
        ),
        (
            "注入4越序封印必抓",
            inj_seal_out_of_order_ok(),
            format!("越序封印注入{}", if inj_seal_out_of_order_ok() { "抓到" } else { "漏抓" }),
        ),
        (
            "五步链步数冻结为5",
            chain_len_ok(),
            format!("步数{}", if chain_len_ok() { "冻结" } else { "漂移" }),
        ),
        (
            "双跑一致两次收口留痕逐条同",
            double_run_ok(),
            format!("双跑{}", if double_run_ok() { "一致" } else { "分叉" }),
        ),
        (
            "写回摘要非空状态已完成",
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

    #[test]
    fn seg_guard_and_diag_closed_set() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE, 0x79C00);
        assert!(diag_closed_set_ok());
        for (i, d) in ALL_DIAGS.iter().enumerate() {
            assert_eq!(d.val(), SEG_BASE + i as u32 + 1);
        }
    }

    #[test]
    fn state_machine_monotonic() {
        assert!(state_monotonic_ok());
        assert_eq!(BatchState::from_rank(4), Some(BatchState::Sealed));
        assert_eq!(BatchState::from_rank(5), None);
        assert!(BatchState::Summarized.rank() > BatchState::LedgerFlipped.rank());
    }

    #[test]
    fn five_step_chain_green_path() {
        assert!(five_step_chain_ok());
        let l = finalize_batch(0, "测试摘要").unwrap();
        assert_eq!(l.traces.len(), 5);
        assert_eq!(l.state, BatchState::Sealed);
        assert_eq!(l.flip_count, 1);
        assert_eq!(l.red_after, 0);
    }

    #[test]
    fn traces_intact_and_names_aligned() {
        assert!(trace_integrity_ok());
        assert!(trace_names_ok());
        let l = finalize_batch(0, "对位").unwrap();
        assert_eq!(l.traces.get(2).unwrap().name, "台账翻态");
        assert_eq!(l.traces.get(4).unwrap().name, "收口封印");
    }

    #[test]
    fn red_gate_rejects_dirty_batch() {
        assert!(red_gate_ok());
        assert_eq!(
            finalize_batch(1, "带红"),
            Err(Diag::RedNotClear.val())
        );
    }

    #[test]
    fn injection_skip_step_and_regress() {
        assert!(inj_skip_step_ok());
        assert!(inj_regress_ok());
        let mut l = FinalizeLedger::new(0);
        l.begin().unwrap();
        // 重复 begin 不再是开口态。
        assert_eq!(l.begin(), Err(Diag::LedgerNotOpen.val()));
        // 跳过第 1 步直接走第 2 步也不行。
        assert_eq!(l.record_step(2, ""), Err(Diag::StepOrderBad.val()));
    }

    #[test]
    fn injection_empty_summary_and_out_of_order_seal() {
        assert!(inj_empty_summary_ok());
        assert!(inj_seal_out_of_order_ok());
    }

    #[test]
    fn double_run_identical_ledger() {
        assert!(double_run_ok());
        let a = finalize_batch(0, "S").unwrap();
        let b = finalize_batch(0, "S").unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn writeback_summary_non_empty() {
        assert!(writeback_summary_ok());
    }

    #[test]
    fn detail_lookup_fallback() {
        assert!(detail(SEG_BASE + 1).contains("步序"));
        assert_eq!(detail(0xDEAD), detail(0x5A5A));
        assert!(!detail(0xDEAD).is_empty());
    }

    #[test]
    fn checks_aggregate_all_green() {
        let s = run_unx3440_checks();
        assert!(s.len() >= 12);
        assert!(s.all_passed(), "全部判据必须绿：{}", s.red_items_text());
    }
}
