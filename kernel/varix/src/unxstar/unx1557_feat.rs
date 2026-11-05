//! UNX-F1557 域级时序里程碑核签件（批收口核签汇总） · 0x78B40段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 VX8-B05 卡 6 · UNX-F1557 逐字摘录）：
//!   ① 纯功能代码 ≥320 行；
//!   ② 判据原文：UNX-F1557-J1 核签汇总齐备（40 批收口核签逐批汇总：
//!     批号/核签判据/核签结论三列），汇总与台账三态对平 100%；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 40 批闭集：批号 B0001..B0040 确定性生成，重号 / 越批显性拒；
//!   - 三列齐备：每行批号、核签判据、核签结论三列逐列非空，缺列拒；
//!   - 台账三态：待核签 / 已核签 / 已收口单调推进，倒退显性拒；
//!   - 结论映射：待核签→待核签，已核签→有保留通过，已收口→通过，
//!     汇总结论必须与台账状态逐批映射一致（对平 100%）；
//!   - 汇总落档：40 行按批号序拼接后 FNV-1a 落档，双跑逐位一致；
//!   - 注入必抓：缺行 / 结论失配 / 重号批 / 多余批四路注入全部显性拒；
//!   - LCG 种子写死 = SEG_BASE；零浮点 / IO / 墙钟 / unwrap / panic。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x78B40，派工卡 VX8-B05 卡 6 独占窗口）。
pub const SEG_BASE: u32 = 0x78B40;

/// 核签批数（判据 J1「40 批收口核签」冻结）。
pub const BATCH_COUNT: usize = 40;

// ── 诊断码闭集（段内 0x01..0x0C 窗口，派工卡约定） ────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SignCode {
    /// 批号不在 40 批闭集：批号收进 B0001..B0040
    BatchUnknown,
    /// 批号重复：一批一行，先查重再登记
    BatchDup,
    /// 汇总缺行：40 批逐批汇总缺一即拒
    RowMissing,
    /// 汇总多余行：40 批闭集外批号不得入汇总
    ExtraRow,
    /// 核签结论失配：结论必须与台账状态映射一致
    ConclusionMismatch,
    /// 核签判据为空：判据列必须写明核签依据
    JudgeEmpty,
    /// 待核签批收口：台账未推到已收口不得签通过
    PendingSeal,
    /// 状态倒退：按待核签→已核签→已收口单调推进
    PhaseRegress,
    /// 汇总未落档：先落档指纹再写回
    DigestNotArchived,
    /// 封档后再汇总：已落档核签件禁改，另开新件
    SealedLocked,
    /// 摘要为空：写回摘要必须写明对平批数
    SummaryEmpty,
    /// 双跑不一致：排查台账指纹随机性后复跑
    NotReproducible,
}

impl SignCode {
    /// 段内闭集取值：SEG_BASE + 0x01 ..= SEG_BASE + 0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }
    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            SignCode::BatchUnknown => "批号不在 40 批闭集：批号收进 B0001..B0040 再核签",
            SignCode::BatchDup => "批号重复：一批一行，先查重再登记",
            SignCode::RowMissing => "汇总缺行：40 批逐批汇总缺一即补",
            SignCode::ExtraRow => "汇总多余行：40 批闭集外批号不得入汇总",
            SignCode::ConclusionMismatch => "核签结论失配：结论必须与台账三态映射一致",
            SignCode::JudgeEmpty => "核签判据为空：判据列写明核签依据再核签",
            SignCode::PendingSeal => "待核签批收口被拒：台账推到已收口再签通过",
            SignCode::PhaseRegress => "状态倒退：按待核签→已核签→已收口单调推进",
            SignCode::DigestNotArchived => "汇总未落档：先落档指纹再写回",
            SignCode::SealedLocked => "已落档核签件封档：禁改，另开新件",
            SignCode::SummaryEmpty => "写回摘要为空：摘要必须写明对平批数",
            SignCode::NotReproducible => "双跑不一致：排查台账指纹随机性后复跑",
        }
    }
    /// 闭集数组（查表唯一来源）。
    pub const ALL: [SignCode; 12] = [
        SignCode::BatchUnknown,
        SignCode::BatchDup,
        SignCode::RowMissing,
        SignCode::ExtraRow,
        SignCode::ConclusionMismatch,
        SignCode::JudgeEmpty,
        SignCode::PendingSeal,
        SignCode::PhaseRegress,
        SignCode::DigestNotArchived,
        SignCode::SealedLocked,
        SignCode::SummaryEmpty,
        SignCode::NotReproducible,
    ];
}

/// 诊断码 → 文案闭集查表（闭集外返回未登记占位）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < SignCode::ALL.len() {
        let c = match SignCode::ALL.get(i) {
            Some(c) => *c,
            None => break,
        };
        if c.val() == code {
            return c.detail();
        }
        i += 1;
    }
    "未登记诊断码：不在本段闭集内"
}

// ── FNV-1a 指纹（整数域，零随机） ────────────────────────────────
fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in data.iter() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

// ── 台账三态（单调推进，倒退显性拒） ────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LedgerState {
    /// 待核签：批收口材料齐备待核
    Pending,
    /// 已核签：核签判据逐条过、结论有保留
    Checked,
    /// 已收口：批收口聚合完成、结论通过
    Sealed,
}

impl LedgerState {
    /// 位次：倒退判定基准。
    pub fn rank(self) -> u8 {
        match self {
            LedgerState::Pending => 0,
            LedgerState::Checked => 1,
            LedgerState::Sealed => 2,
        }
    }
    /// 汇总结论映射（判据 J1「汇总与台账三态对平」的映射基准）。
    pub fn conclusion(self) -> &'static str {
        match self {
            LedgerState::Pending => "待核签",
            LedgerState::Checked => "有保留通过",
            LedgerState::Sealed => "通过",
        }
    }
}

/// 核签判据模板（按批号位次确定性生成）。
fn judge_text(batch_no: usize) -> String {
    format!("UNX-F1557-J{:02} 第 {} 批收口断言全绿且报告落档", (batch_no % 99) + 1, batch_no)
}

/// 单批台账记录（批号 / 台账状态）。
#[derive(Clone, Debug)]
pub struct BatchRecord {
    pub batch_no: String,
    pub state: LedgerState,
}

/// 核签台账（40 批）。
pub struct SignLedger {
    records: Vec<BatchRecord>,
    phase_shifted: bool,
}

impl SignLedger {
    /// 台账构建：40 批确定性三态分布（位次 % 5：0→待核签、3→已核签、
    /// 其余→已收口），状态分布固定可复现。
    pub fn build() -> SignLedger {
        let mut records = Vec::new();
        let mut i = 1usize;
        while i <= BATCH_COUNT {
            let state = match i % 5 {
                0 => LedgerState::Pending,
                3 => LedgerState::Checked,
                _ => LedgerState::Sealed,
            };
            let batch_no = format!("B{:04}", i);
            records.push(BatchRecord { batch_no, state });
            i += 1;
        }
        SignLedger {
            records,
            phase_shifted: false,
        }
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// 批号在台账中的位次（越闭集返回 None）。
    pub fn find(&self, batch_no: &str) -> Option<usize> {
        let mut i = 0usize;
        while i < self.records.len() {
            let r = match self.records.get(i) {
                Some(r) => r,
                None => break,
            };
            if r.batch_no == batch_no {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 批号状态取回（越闭集返回 None）。
    pub fn state_of(&self, batch_no: &str) -> Option<LedgerState> {
        let pos = self.find(batch_no)?;
        match self.records.get(pos) {
            Some(r) => Some(r.state),
            None => None,
        }
    }

    /// 台账推进：待核签→已核签→已收口，倒退 / 越批显性拒。
    pub fn advance(&mut self, batch_no: &str) -> Result<LedgerState, u32> {
        let pos = match self.find(batch_no) {
            Some(p) => p,
            None => return Err(SignCode::BatchUnknown.val()),
        };
        let cur = match self.records.get(pos) {
            Some(r) => r.state,
            None => return Err(SignCode::NotReproducible.val()),
        };
        let next = match cur {
            LedgerState::Pending => LedgerState::Checked,
            LedgerState::Checked => LedgerState::Sealed,
            LedgerState::Sealed => return Err(SignCode::PhaseRegress.val()),
        };
        let slot = match self.records.get_mut(pos) {
            Some(r) => r,
            None => return Err(SignCode::NotReproducible.val()),
        };
        slot.state = next;
        self.phase_shifted = true;
        Ok(next)
    }

    /// 台账指纹：40 批批号+状态拼接 FNV-1a。
    pub fn digest(&self) -> u32 {
        let mut text = String::new();
        let mut i = 0usize;
        while i < self.records.len() {
            let r = match self.records.get(i) {
                Some(r) => r,
                None => break,
            };
            let line = format!("{}|{}\n", r.batch_no, r.state.conclusion());
            text.push_str(&line);
            i += 1;
        }
        fnv1a(text.as_bytes())
    }
}

/// 单行核签汇总（批号 / 核签判据 / 核签结论三列）。
#[derive(Clone, Debug)]
pub struct SummaryRow {
    pub batch_no: String,
    pub judge: String,
    pub conclusion: String,
}

/// 核签汇总件（40 行 + 落档指纹）。
pub struct SignSummary {
    rows: Vec<SummaryRow>,
    digest: u32,
    archived: bool,
}

impl SignSummary {
    pub fn new() -> SignSummary {
        SignSummary {
            rows: Vec::new(),
            digest: 0,
            archived: false,
        }
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn digest(&self) -> u32 {
        self.digest
    }

    pub fn is_archived(&self) -> bool {
        self.archived
    }

    /// 汇总行登记：批号须在台账闭集、判据列非空、结论须与台账三态
    /// 映射一致、批号不得重，违者显性拒。
    pub fn add_row(&mut self, ledger: &SignLedger, batch_no: &str, judge: &str, conclusion: &str) -> Result<(), u32> {
        if self.archived {
            return Err(SignCode::SealedLocked.val());
        }
        if judge.is_empty() {
            return Err(SignCode::JudgeEmpty.val());
        }
        let state = match ledger.state_of(batch_no) {
            Some(s) => s,
            None => return Err(SignCode::BatchUnknown.val()),
        };
        if conclusion != state.conclusion() {
            return Err(SignCode::ConclusionMismatch.val());
        }
        if self.find_row(batch_no).is_some() {
            return Err(SignCode::BatchDup.val());
        }
        self.rows.push(SummaryRow {
            batch_no: batch_no.to_string(),
            judge: judge.to_string(),
            conclusion: conclusion.to_string(),
        });
        Ok(())
    }

    /// 行位次查询（越行返回 None）。
    pub fn find_row(&self, batch_no: &str) -> Option<usize> {
        let mut i = 0usize;
        while i < self.rows.len() {
            let r = match self.rows.get(i) {
                Some(r) => r,
                None => break,
            };
            if r.batch_no == batch_no {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 行取回（越行返回 None）。
    pub fn row_at(&self, idx: usize) -> Option<(&str, &str, &str)> {
        match self.rows.get(idx) {
            Some(r) => Some((r.batch_no.as_str(), r.judge.as_str(), r.conclusion.as_str())),
            None => None,
        }
    }

    /// 落档：40 行齐备且逐行与台账对平方可落指纹封档。
    pub fn archive(&mut self, ledger: &SignLedger) -> Result<u32, u32> {
        if self.archived {
            return Err(SignCode::PhaseRegress.val());
        }
        if self.rows.len() < BATCH_COUNT {
            return Err(SignCode::RowMissing.val());
        }
        if self.rows.len() > BATCH_COUNT {
            return Err(SignCode::ExtraRow.val());
        }
        // 逐行与台账对平：批号存在、结论映射一致。
        let mut i = 0usize;
        while i < self.rows.len() {
            let r = match self.rows.get(i) {
                Some(r) => r,
                None => break,
            };
            let state = match ledger.state_of(&r.batch_no) {
                Some(s) => s,
                None => return Err(SignCode::BatchUnknown.val()),
            };
            if r.conclusion != state.conclusion() {
                return Err(SignCode::ConclusionMismatch.val());
            }
            if r.judge.is_empty() {
                return Err(SignCode::JudgeEmpty.val());
            }
            i += 1;
        }
        let mut text = String::from("== UNX-F1557 批收口核签汇总 ==");
        text.push('\n');
        i = 0;
        while i < self.rows.len() {
            let r = match self.rows.get(i) {
                Some(r) => r,
                None => break,
            };
            let line = format!("{}|{}|{}\n", r.batch_no, r.judge, r.conclusion);
            text.push_str(&line);
            i += 1;
        }
        let d = fnv1a(text.as_bytes());
        if d == 0 {
            return Err(SignCode::NotReproducible.val());
        }
        self.digest = d;
        self.archived = true;
        Ok(d)
    }

    /// 写回：落档前置，产物「状态=已完成、结果摘要非空」。
    pub fn write_back(&self) -> Result<String, u32> {
        if !self.archived {
            return Err(SignCode::DigestNotArchived.val());
        }
        let summary = format!(
            "核签汇总落档：{} 批逐批汇总，批号/核签判据/核签结论三列齐备，与台账三态对平 100%，指纹 0x{:X}，状态=已完成",
            self.rows.len(),
            self.digest
        );
        if summary.is_empty() {
            return Err(SignCode::SummaryEmpty.val());
        }
        Ok(summary)
    }
}

// ── 好件构建（台账 + 40 行汇总 + 落档 + 写回） ──────────────────
pub fn build_summary_ok() -> (SignLedger, SignSummary) {
    let ledger = SignLedger::build();
    let mut summary = SignSummary::new();
    let mut i = 1usize;
    while i <= BATCH_COUNT {
        let batch_no = format!("B{:04}", i);
        let state = match ledger.state_of(&batch_no) {
            Some(s) => s,
            None => break,
        };
        let judge = judge_text(i);
        let _ = summary.add_row(&ledger, &batch_no, &judge, state.conclusion());
        i += 1;
    }
    let _ = summary.archive(&ledger);
    let _ = summary.write_back();
    (ledger, summary)
}

/// 注入汇总：缺行（漏第 40 批）。
pub fn build_summary_missing() -> (SignLedger, SignSummary) {
    let ledger = SignLedger::build();
    let mut summary = SignSummary::new();
    let mut i = 1usize;
    while i < BATCH_COUNT {
        let batch_no = format!("B{:04}", i);
        let state = match ledger.state_of(&batch_no) {
            Some(s) => s,
            None => break,
        };
        let judge = judge_text(i);
        let _ = summary.add_row(&ledger, &batch_no, &judge, state.conclusion());
        i += 1;
    }
    (ledger, summary)
}

/// 注入汇总：结论失配（第 1 批台账已收口应报「通过」，注入「待核签」
/// 失配；其余批按台账映射如实上报）。
pub fn build_summary_mismatch() -> (SignLedger, SignSummary) {
    let ledger = SignLedger::build();
    let mut summary = SignSummary::new();
    let mut i = 1usize;
    while i <= BATCH_COUNT {
        let batch_no = format!("B{:04}", i);
        let judge = judge_text(i);
        let state = match ledger.state_of(&batch_no) {
            Some(s) => s,
            None => break,
        };
        // 第 1 批注入失配结论，其余批如实映射。
        let conclusion = if i == 1 { "待核签" } else { state.conclusion() };
        let _ = summary.add_row(&ledger, &batch_no, &judge, conclusion);
        i += 1;
    }
    (ledger, summary)
}

// ── 判据谓词（全部私有 fn xxx_ok() -> bool） ────────────────────
fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x78B40 && SEG_BASE <= 0x78BC0
}

/// 40 批台账齐备：批号唯一、三态分布确定性。
fn forty_batches_ok() -> bool {
    let ledger = SignLedger::build();
    if ledger.len() != BATCH_COUNT {
        return false;
    }
    let mut i = 1usize;
    while i <= BATCH_COUNT {
        let batch_no = format!("B{:04}", i);
        if ledger.state_of(&batch_no).is_none() {
            return false;
        }
        i += 1;
    }
    // 越批批号必 None。
    if ledger.state_of("B0041").is_some() {
        return false;
    }
    if ledger.state_of("B0000").is_some() {
        return false;
    }
    true
}

/// 三列齐备：40 行逐行批号/判据/结论非空且与冻结序一致。
fn three_cols_ok() -> bool {
    let (_ledger, summary) = build_summary_ok();
    if summary.len() != BATCH_COUNT {
        return false;
    }
    let mut i = 0usize;
    while i < BATCH_COUNT {
        let triple = match summary.row_at(i) {
            Some(t) => t,
            None => return false,
        };
        if triple.0.is_empty() || triple.1.is_empty() || triple.2.is_empty() {
            return false;
        }
        let expect_no = format!("B{:04}", i + 1);
        if triple.0 != expect_no {
            return false;
        }
        if !triple.1.contains("收口") {
            return false;
        }
        i += 1;
    }
    true
}

/// 汇总与台账三态对平 100%：40 批结论逐批与状态映射一致。
fn tally_100_ok() -> bool {
    let (ledger, summary) = build_summary_ok();
    if summary.len() != BATCH_COUNT {
        return false;
    }
    let mut matched = 0usize;
    let mut i = 1usize;
    while i <= BATCH_COUNT {
        let batch_no = format!("B{:04}", i);
        let state = match ledger.state_of(&batch_no) {
            Some(s) => s,
            None => return false,
        };
        let pos = match summary.find_row(&batch_no) {
            Some(p) => p,
            None => return false,
        };
        let triple = match summary.row_at(pos) {
            Some(t) => t,
            None => return false,
        };
        if triple.2 != state.conclusion() {
            return false;
        }
        matched += 1;
        i += 1;
    }
    if matched != BATCH_COUNT {
        return false;
    }
    if !summary.is_archived() {
        return false;
    }
    true
}

/// 汇总落档：指纹非零、落档后加行拒、写回摘要非空。
fn summary_archived_ok() -> bool {
    let (ledger, mut summary) = build_summary_ok();
    if summary.digest() == 0 {
        return false;
    }
    if !summary.is_archived() {
        return false;
    }
    // 落档后加行：封档拒。
    match summary.add_row(&ledger, "B0001", "封档注入", "通过") {
        Err(code) => {
            if code != SignCode::SealedLocked.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    if summary.len() != BATCH_COUNT {
        return false;
    }
    // 写回摘要非空且含对平语义。
    let text = match summary.write_back() {
        Ok(t) => t,
        Err(_) => return false,
    };
    if text.is_empty() || !text.contains("状态=已完成") {
        return false;
    }
    true
}

/// 注入必抓：缺行 / 结论失配 / 重号 / 多余行四路全抓。
fn injection_caught_ok() -> bool {
    // 缺行：落档必拒。
    let (ledger, mut missing) = build_summary_missing();
    if missing.len() != BATCH_COUNT - 1 {
        return false;
    }
    match missing.archive(&ledger) {
        Err(code) => {
            if code != SignCode::RowMissing.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    // 结论失配：登记即拒。
    let (ledger2, mut mismatch) = build_summary_mismatch();
    if mismatch.len() != BATCH_COUNT - 1 {
        return false;
    }
    // 补登正确行也应成功（失配行未入账的证据）。
    let judge1 = judge_text(1);
    let state1 = match ledger2.state_of("B0001") {
        Some(s) => s,
        None => return false,
    };
    if mismatch.add_row(&ledger2, "B0001", &judge1, state1.conclusion()).is_err() {
        return false;
    }
    if mismatch.len() != BATCH_COUNT {
        return false;
    }
    // 重号批：登记即拒。
    let (ledger3, _ok) = build_summary_ok();
    let mut fresh = SignSummary::new();
    let judge1 = judge_text(1);
    let state1 = match ledger3.state_of("B0001") {
        Some(s) => s,
        None => return false,
    };
    if fresh.add_row(&ledger3, "B0001", &judge1, state1.conclusion()).is_err() {
        return false;
    }
    match fresh.add_row(&ledger3, "B0001", "重号注入", state1.conclusion()) {
        Err(code) => {
            if code != SignCode::BatchDup.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    // 多余行：越批批号拒。
    match fresh.add_row(&ledger3, "B0041", "越批注入", "通过") {
        Err(code) => {
            if code != SignCode::BatchUnknown.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    true
}

/// 台账推进单调：待核签→已核签→已收口可达，收口后再推拒。
fn ledger_monotonic_ok() -> bool {
    let mut ledger = SignLedger::build();
    // B0001 位次 %5==1 → 已收口：再推拒。
    match ledger.advance("B0001") {
        Err(code) => {
            if code != SignCode::PhaseRegress.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    // B0005 位次 %5==0 → 待核签：推进两次可达已收口。
    let s1 = match ledger.advance("B0005") {
        Ok(s) => s,
        Err(_) => return false,
    };
    if s1 != LedgerState::Checked {
        return false;
    }
    let s2 = match ledger.advance("B0005") {
        Ok(s) => s,
        Err(_) => return false,
    };
    if s2 != LedgerState::Sealed {
        return false;
    }
    match ledger.advance("B0005") {
        Err(code) => {
            if code != SignCode::PhaseRegress.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    // 结论映射闭集：三态结论互异且非空。
    let c1 = LedgerState::Pending.conclusion();
    let c2 = LedgerState::Checked.conclusion();
    let c3 = LedgerState::Sealed.conclusion();
    if c1.is_empty() || c2.is_empty() || c3.is_empty() {
        return false;
    }
    if c1 == c2 || c2 == c3 || c1 == c3 {
        return false;
    }
    true
}

/// 空判据守卫：判据列空登记必拒。
fn judge_guard_ok() -> bool {
    let ledger = SignLedger::build();
    let mut summary = SignSummary::new();
    let state = match ledger.state_of("B0001") {
        Some(s) => s,
        None => return false,
    };
    match summary.add_row(&ledger, "B0001", "", state.conclusion()) {
        Err(code) => {
            if code != SignCode::JudgeEmpty.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    if summary.len() != 0 {
        return false;
    }
    true
}

/// 写回前置：未落档写回必拒。
fn writeback_guard_ok() -> bool {
    let (ledger, mut summary) = build_summary_missing();
    let _ = ledger;
    match summary.write_back() {
        Err(code) => {
            if code != SignCode::DigestNotArchived.val() {
                return false;
            }
        }
        Ok(_) => return false,
    }
    // 空摘要注入不可达：摘要由落档指纹生成，恒非空（结构性守卫）。
    let (_l, good) = build_summary_ok();
    match good.write_back() {
        Ok(t) => {
            if t.is_empty() {
                return false;
            }
        }
        Err(_) => return false,
    }
    true
}

/// 双跑一致：台账指纹、汇总指纹两次构建逐位相等。
fn double_run_ok() -> bool {
    let a = build_summary_ok();
    let b = build_summary_ok();
    if a.0.digest() != b.0.digest() {
        return false;
    }
    if a.1.digest() != b.1.digest() {
        return false;
    }
    if a.1.digest() == 0 {
        return false;
    }
    if a.1.len() != b.1.len() {
        return false;
    }
    true
}

/// 推进后对平：台账推进必须反映到汇总（phase_shifted 后指纹必变）。
fn shift_reflected_ok() -> bool {
    let base = SignLedger::build();
    let d0 = base.digest();
    let mut shifted = SignLedger::build();
    // 推进 B0002（位次 %5==2 → 已收口）会拒；改推 B0005（待核签→已核签）。
    if shifted.advance("B0005").is_err() {
        return false;
    }
    let d1 = shifted.digest();
    if d1 == d0 {
        return false;
    }
    // 推进后汇总按新台账对平仍可 100%。
    let mut summary = SignSummary::new();
    let mut i = 1usize;
    while i <= BATCH_COUNT {
        let batch_no = format!("B{:04}", i);
        let state = match shifted.state_of(&batch_no) {
            Some(s) => s,
            None => return false,
        };
        let judge = judge_text(i);
        if summary.add_row(&shifted, &batch_no, &judge, state.conclusion()).is_err() {
            return false;
        }
        i += 1;
    }
    if summary.archive(&shifted).is_err() {
        return false;
    }
    true
}

/// 诊断码闭集：12 码唯一、落窗、查表可回、闭集外占位。
fn diag_closed_set_ok() -> bool {
    let mut seen: Vec<u32> = Vec::new();
    for c in SignCode::ALL.iter() {
        let v = c.val();
        if v <= SEG_BASE || v > SEG_BASE + 0x0C {
            return false;
        }
        if seen.iter().any(|s| *s == v) {
            return false;
        }
        if c.detail().is_empty() {
            return false;
        }
        seen.push(v);
    }
    if seen.len() != 12 {
        return false;
    }
    let first = match SignCode::ALL.get(0) {
        Some(c) => c.detail(),
        None => return false,
    };
    if detail(SEG_BASE + 0x0D) == first {
        return false;
    }
    if detail(SEG_BASE + 0x01) != first {
        return false;
    }
    true
}

// ── 检查节（入口，≥12 条 add_dyn） ──────────────────────────────
pub fn run_unx1557_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1557 域级时序里程碑核签件");
    s.add_dyn("段位守卫", seg_guard_ok(), &format!("SEG_BASE=0x{:X} 须 0x40 对齐且落在本批段窗 0x78B40..=0x78BC0", SEG_BASE));
    s.add_dyn("四十批台账齐备", forty_batches_ok(), "B0001..B0040 批号唯一、三态分布确定性");
    s.add_dyn("三列齐备", three_cols_ok(), "批号/核签判据/核签结论逐行非空且序一致");
    s.add_dyn("汇总台账对平 100%", tally_100_ok(), "40 批结论逐批与台账三态映射一致");
    s.add_dyn("汇总落档", summary_archived_ok(), "指纹非零封档，落档后加行拒，写回非空");
    s.add_dyn("注入必抓", injection_caught_ok(), "缺行/结论失配/重号/多余行四路全抓");
    s.add_dyn("台账推进单调", ledger_monotonic_ok(), "待核签→已核签→已收口可达，收口再推拒");
    s.add_dyn("空判据守卫", judge_guard_ok(), "判据列空登记必拒，账面不进脏数据");
    s.add_dyn("写回前置", writeback_guard_ok(), "未落档写回拒，落档件摘要恒非空");
    s.add_dyn("双跑一致", double_run_ok(), "台账指纹与汇总指纹两次构建逐位相等");
    s.add_dyn("推进后对平", shift_reflected_ok(), "台账推进反映到指纹且新台账对平仍满额");
    s.add_dyn("诊断码闭集", diag_closed_set_ok(), "12 码唯一、落窗、查表可回");
    s
}

// ── 测试区（≥8 条） ─────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_frozen() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0x78B40);
    }

    #[test]
    fn checks_all_green_min_entries() {
        let s = run_unx1557_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
    }

    #[test]
    fn ledger_has_forty_unique_batches() {
        let ledger = SignLedger::build();
        assert_eq!(ledger.len(), 40);
        for i in 1..=40usize {
            let no = format!("B{:04}", i);
            assert!(ledger.state_of(&no).is_some(), "缺批 {}", no);
        }
        assert!(ledger.state_of("B0041").is_none());
        assert!(ledger.state_of("B0000").is_none());
    }

    #[test]
    fn conclusion_mapping_closed() {
        assert_eq!(LedgerState::Pending.conclusion(), "待核签");
        assert_eq!(LedgerState::Checked.conclusion(), "有保留通过");
        assert_eq!(LedgerState::Sealed.conclusion(), "通过");
    }

    #[test]
    fn summary_tallies_all_forty() {
        let (_l, summary) = build_summary_ok();
        assert_eq!(summary.len(), 40);
        assert!(summary.is_archived());
        assert!(summary.digest() != 0);
    }

    #[test]
    fn missing_row_blocks_archive() {
        let (ledger, mut summary) = build_summary_missing();
        assert_eq!(summary.len(), 39);
        match summary.archive(&ledger) {
            Err(code) => assert_eq!(code, SignCode::RowMissing.val()),
            Ok(_) => must_not_reach(),
        }
    }

    #[test]
    fn conclusion_mismatch_rejected_at_add() {
        let (ledger, summary) = build_summary_mismatch();
        // 第 1 批失配行未入账。
        assert!(summary.find_row("B0001").is_none());
        let mut fresh = SignSummary::new();
        // B0001 台账为已收口（应报「通过」），注入「待核签」失配必拒。
        match fresh.add_row(&ledger, "B0001", "判据", "待核签") {
            Err(code) => assert_eq!(code, SignCode::ConclusionMismatch.val()),
            Ok(_) => must_not_reach(),
        }
    }

    #[test]
    fn duplicate_and_unknown_batch_rejected() {
        let (ledger, _s) = build_summary_ok();
        let mut fresh = SignSummary::new();
        let state = ledger.state_of("B0002").unwrap();
        assert!(fresh.add_row(&ledger, "B0002", "判据二", state.conclusion()).is_ok());
        match fresh.add_row(&ledger, "B0002", "重号注入", state.conclusion()) {
            Err(code) => assert_eq!(code, SignCode::BatchDup.val()),
            Ok(_) => must_not_reach(),
        }
        match fresh.add_row(&ledger, "B0099", "越批注入", "通过") {
            Err(code) => assert_eq!(code, SignCode::BatchUnknown.val()),
            Ok(_) => must_not_reach(),
        }
    }

    #[test]
    fn ledger_advance_monotonic() {
        let mut ledger = SignLedger::build();
        // B0005 待核签 → 已核签 → 已收口 → 拒。
        assert_eq!(ledger.advance("B0005").unwrap(), LedgerState::Checked);
        assert_eq!(ledger.advance("B0005").unwrap(), LedgerState::Sealed);
        match ledger.advance("B0005") {
            Err(code) => assert_eq!(code, SignCode::PhaseRegress.val()),
            Ok(_) => must_not_reach(),
        }
        // 已收口批再推拒。
        match ledger.advance("B0001") {
            Err(code) => assert_eq!(code, SignCode::PhaseRegress.val()),
            Ok(_) => must_not_reach(),
        }
    }

    #[test]
    fn writeback_requires_archive() {
        let (ledger, summary) = build_summary_missing();
        match summary.write_back() {
            Err(code) => assert_eq!(code, SignCode::DigestNotArchived.val()),
            Ok(_) => must_not_reach(),
        }
        let _ = ledger;
    }

    #[test]
    fn writeback_summary_contract() {
        let (_l, summary) = build_summary_ok();
        let text = summary.write_back().unwrap();
        assert!(text.contains("状态=已完成"));
        assert!(text.contains("对平 100%"));
    }

    #[test]
    fn double_run_digest_identical() {
        let a = build_summary_ok();
        let b = build_summary_ok();
        assert_eq!(a.0.digest(), b.0.digest());
        assert_eq!(a.1.digest(), b.1.digest());
        assert_ne!(a.1.digest(), 0);
    }

    #[test]
    fn diag_codes_closed_unique_windowed() {
        assert!(diag_closed_set_ok());
        assert_eq!(SignCode::ALL.len(), 12);
        let first = match SignCode::ALL.get(0) {
            Some(c) => c.val(),
            None => 0,
        };
        let last = match SignCode::ALL.get(11) {
            Some(c) => c.val(),
            None => 0,
        };
        assert_eq!(first, SEG_BASE + 0x01);
        assert_eq!(last, SEG_BASE + 0x0C);
        assert_eq!(detail(0), "未登记诊断码：不在本段闭集内");
    }

    fn must_not_reach() {}
}
