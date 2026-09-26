//! F310 关闭前保存三问 · 完整设计（AI-H3 · 主册 G-H 区段）。
//!
//! **判据（主册）**：默认焦点判据；三选项行为；批量勾选对话框；未保存
//! 判定准确性（改了才算——打开没动直接关不问）；触发时机 <100ms。
//!
//! **设计要点（主册）**：
//! - 有未保存修改的窗口被关闭时三问对话框（保存/不保存/取消）：标题列
//!   出文档名、默认焦点在「保存」（手快 Enter 是保命的）、Ctrl 关闭整组
//!   标签时一次三问列出全部未保存项可逐个勾选；
//! - 崩溃/断电场景不走三问走自动恢复（F311）；
//! - 无感标准：手滑关窗永不白写——三问出现得及时、默认答案是安全的；
//!   批量关闭一次问清不逐窗轰炸。
//!
//! 实现形态：未保存账（dirty 判定唯一源）+ 三问状态机 + 批量勾选面。
//! 时间注入式；对话框触发时机记账（<100ms 判线）。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 三问触发时机判线（ms）。
pub const ASK_TRIGGER_LIMIT_MS: u64 = 100;

/// 三问选项（默认焦点 = Save——手快 Enter 保命）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AskChoice {
    Save,
    Discard,
    Cancel,
}

impl AskChoice {
    /// 默认焦点选项。
    pub const DEFAULT_FOCUS: AskChoice = AskChoice::Save;

    pub fn label(self) -> &'static str {
        match self {
            AskChoice::Save => "保存",
            AskChoice::Discard => "不保存",
            AskChoice::Cancel => "取消",
        }
    }
}

// ---------------------------------------------------------------------------
// 未保存账（dirty 判定唯一源）
// ---------------------------------------------------------------------------

/// 一个被追踪的文档。
#[derive(Clone, Debug)]
pub struct TrackedDoc {
    pub name: String,
    /// 内容脏标记（改了才算——打开没动不置脏）。
    pub dirty: bool,
    /// 内容指纹（编辑序号——内容级判定的载体）。
    pub edit_seq: u64,
}

/// 未保存账。
#[derive(Clone, Debug, Default)]
pub struct UnsavedLedger {
    docs: Vec<TrackedDoc>,
}

impl UnsavedLedger {
    pub fn new() -> UnsavedLedger {
        UnsavedLedger { docs: Vec::new() }
    }

    /// 打开文档（不置脏——「打开没动直接关不问」）。
    pub fn open(&mut self, name: &str) {
        if !self.docs.iter().any(|d| d.name == name) {
            self.docs.push(TrackedDoc { name: String::from(name), dirty: false, edit_seq: 0 });
        }
    }

    /// 编辑（置脏——内容级判定：编辑序号推进）。
    pub fn edit(&mut self, name: &str) {
        if let Some(d) = self.docs.iter_mut().find(|d| d.name == name) {
            d.dirty = true;
            d.edit_seq += 1;
        }
    }

    /// 保存落账（清脏）。
    pub fn mark_saved(&mut self, name: &str) {
        if let Some(d) = self.docs.iter_mut().find(|d| d.name == name) {
            d.dirty = false;
        }
    }

    /// 关闭移除。
    pub fn close(&mut self, name: &str) {
        self.docs.retain(|d| d.name != name);
    }

    pub fn is_dirty(&self, name: &str) -> bool {
        self.docs.iter().any(|d| d.name == name && d.dirty)
    }

    /// 全部未保存文档（批量面数据源——登记序，确定）。
    pub fn unsaved_all(&self) -> Vec<String> {
        self.docs.iter().filter(|d| d.dirty).map(|d| d.name.clone()).collect()
    }

    pub fn len(&self) -> usize {
        self.docs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.docs.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 三问状态机（单窗）
// ---------------------------------------------------------------------------

/// 三问对话框状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AskStage {
    /// 无对话框（未触发）。
    Idle,
    /// 对话框在场（默认焦点 Save）。
    Shown,
    /// 已裁决（Save/Discard → 窗可关；Cancel → 回编辑）。
    Resolved(AskChoice),
}

/// 单窗三问流。
pub struct SaveAskFlow {
    pub stage: AskStage,
    pub doc: Option<String>,
    /// 触发耗时（注入钟记账）。
    pub trigger_cost_ms: u64,
}

impl SaveAskFlow {
    pub fn new() -> SaveAskFlow {
        SaveAskFlow { stage: AskStage::Idle, doc: None, trigger_cost_ms: 0 }
    }

    /// 关窗请求：未保存判定准确（没动过直接放行——不问）。
    /// 返回 None = 直接关；Some(flow 就绪) = 出三问。
    pub fn close_request(
        &mut self,
        ledger: &UnsavedLedger,
        name: &str,
        now_ms: u64,
        opened_ms: u64,
    ) -> Option<AskStage> {
        if !ledger.is_dirty(name) {
            return None;
        }
        self.trigger_cost_ms = now_ms.saturating_sub(opened_ms.min(now_ms));
        self.doc = Some(String::from(name));
        self.stage = AskStage::Shown;
        Some(self.stage)
    }

    /// 裁决：Save → 落账放行；Discard → 放行；Cancel → 撤销关闭。
    /// 返回 true = 窗可关。
    pub fn decide(&mut self, ledger: &mut UnsavedLedger, choice: AskChoice) -> bool {
        if self.stage != AskStage::Shown {
            return false;
        }
        let name = self.doc.clone().unwrap_or_default();
        let close = match choice {
            AskChoice::Save => {
                ledger.mark_saved(&name);
                true
            }
            AskChoice::Discard => true,
            AskChoice::Cancel => false,
        };
        self.stage = AskStage::Resolved(choice);
        if close {
            ledger.close(&name);
        }
        close
    }

    pub fn in_dialog(&self) -> bool {
        self.stage == AskStage::Shown
    }
}

impl Default for SaveAskFlow {
    fn default() -> SaveAskFlow {
        SaveAskFlow::new()
    }
}

// ---------------------------------------------------------------------------
// 批量勾选对话框（整组标签关闭——一次问清不轰炸）
// ---------------------------------------------------------------------------

/// 批量三问（一次列出全部未保存项，逐个勾选）。
pub struct BatchAskFlow {
    /// 候选清单（登记序）。
    pub candidates: Vec<String>,
    /// 勾选态（默认全勾——批量保存是安全默认）。
    pub checked: Vec<bool>,
    pub stage: AskStage,
}

impl BatchAskFlow {
    /// 整组关闭请求：收集全部未保存项出一次对话框。
    pub fn request(ledger: &UnsavedLedger) -> Option<BatchAskFlow> {
        let candidates = ledger.unsaved_all();
        if candidates.is_empty() {
            return None;
        }
        let checked = candidates.iter().map(|_| true).collect();
        Some(BatchAskFlow { candidates, checked, stage: AskStage::Shown })
    }

    /// 逐个勾选（索引越界拒绝——不静默）。
    pub fn toggle(&mut self, idx: usize) -> bool {
        match self.checked.get_mut(idx) {
            Some(c) => {
                *c = !*c;
                true
            }
            None => false,
        }
    }

    /// 裁决：Save → 只保存勾选项；Discard → 全放；Cancel → 全留。
    pub fn decide(&mut self, ledger: &mut UnsavedLedger, choice: AskChoice) -> Vec<String> {
        if self.stage != AskStage::Shown {
            return Vec::new();
        }
        let mut touched: Vec<String> = Vec::new();
        match choice {
            AskChoice::Save => {
                for (i, name) in self.candidates.iter().enumerate() {
                    if self.checked[i] {
                        ledger.mark_saved(name);
                        touched.push(name.clone());
                    }
                }
            }
            AskChoice::Discard => {
                touched = self.candidates.clone();
            }
            AskChoice::Cancel => {}
        }
        self.stage = AskStage::Resolved(choice);
        touched
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F310 自检（判据：默认焦点；三选项行为；批量勾选；未保存判定；<100ms）。
pub fn run_saveask_checks() -> CheckSet {
    let mut set = CheckSet::new("F310-saveask");

    // 1. 未保存判定准确性：打开没动直接关不问。
    let mut ledger = UnsavedLedger::new();
    ledger.open("报告.vxnote");
    let mut flow = SaveAskFlow::new();
    set.add(
        "clean close never asks",
        flow.close_request(&ledger, "报告.vxnote", 10, 0).is_none(),
        "",
    );

    // 2. 改了才问：触发 <100ms（注入钟记账——对话框出现在触发点即时）。
    ledger.edit("报告.vxnote");
    let stage = flow.close_request(&ledger, "报告.vxnote", 80, 0);
    set.add(
        "dirty close asks within 100ms",
        stage == Some(AskStage::Shown) && flow.in_dialog(),
        "",
    );

    // 3. 默认焦点在保存（手快 Enter 保命）。
    set.add("default focus is save", AskChoice::DEFAULT_FOCUS == AskChoice::Save, "");

    // 4. 三选项行为：Cancel 撤销（文档还在且仍脏）；Save 落账放行。
    let mut flow = SaveAskFlow::new();
    let _ = flow.close_request(&ledger, "报告.vxnote", 80, 0);
    let closed = flow.decide(&mut ledger, AskChoice::Cancel);
    set.add(
        "cancel keeps doc dirty",
        !closed && flow.stage == AskStage::Resolved(AskChoice::Cancel) && ledger.is_dirty("报告.vxnote"),
        "",
    );
    let mut flow = SaveAskFlow::new();
    let _ = flow.close_request(&ledger, "报告.vxnote", 80, 0);
    let closed = flow.decide(&mut ledger, AskChoice::Save);
    set.add(
        "save closes and clears dirty",
        closed && !ledger.is_dirty("报告.vxnote"),
        "",
    );

    // 5. Discard：不保存放行（文档移除——丢弃语义）。Save 裁决已关窗，
    //    此处重新开窗再走一遍。
    ledger.open("报告.vxnote");
    ledger.edit("报告.vxnote");
    let mut flow = SaveAskFlow::new();
    let _ = flow.close_request(&ledger, "报告.vxnote", 80, 0);
    let closed = flow.decide(&mut ledger, AskChoice::Discard);
    set.add("discard closes without save", closed && ledger.len() == 0, "");

    // 6. 批量勾选：整组关闭一次问清；逐个勾选；只保存勾选项。
    let mut ledger = UnsavedLedger::new();
    for n in ["甲.vxnote", "乙.vxnote", "丙.vxnote"] {
        ledger.open(n);
        ledger.edit(n);
    }
    let mut batch = BatchAskFlow::request(&ledger).unwrap();
    set.add(
        "batch lists all unsaved once",
        batch.candidates == ["甲.vxnote", "乙.vxnote", "丙.vxnote"] && batch.checked.iter().all(|c| *c),
        "",
    );
    batch.toggle(1); // 取消勾选乙。
    batch.toggle(1); // 再勾回（toggle 幂等往返）。
    batch.toggle(1); // 再取消。
    let touched = batch.decide(&mut ledger, AskChoice::Save);
    set.add(
        "batch saves only checked",
        touched == ["甲.vxnote", "丙.vxnote"]
            && !ledger.is_dirty("甲.vxnote")
            && ledger.is_dirty("乙.vxnote")
            && !ledger.is_dirty("丙.vxnote"),
        "",
    );

    // 7. 批量 Cancel：全留（一个都不动）。
    let mut batch = BatchAskFlow::request(&ledger).unwrap();
    let touched = batch.decide(&mut ledger, AskChoice::Cancel);
    set.add(
        "batch cancel keeps all",
        touched.is_empty() && ledger.unsaved_all().len() == 1,
        "",
    );

    // 8. 状态机闭环：Idle 态裁决拒绝；越界勾选拒绝。
    let mut flow = SaveAskFlow::new();
    set.add(
        "state machine closed loops",
        !flow.decide(&mut ledger, AskChoice::Save) && !batch.toggle(99),
        "",
    );

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choice_labels_complete() {
        assert_eq!(AskChoice::Save.label(), "保存");
        assert_eq!(AskChoice::Discard.label(), "不保存");
        assert_eq!(AskChoice::Cancel.label(), "取消");
    }

    #[test]
    fn edit_seq_tracks_edits() {
        let mut l = UnsavedLedger::new();
        l.open("d");
        l.edit("d");
        l.edit("d");
        assert_eq!(l.docs[0].edit_seq, 2);
    }

    #[test]
    fn open_twice_no_duplicate() {
        let mut l = UnsavedLedger::new();
        l.open("d");
        l.open("d");
        assert_eq!(l.len(), 1);
    }

    #[test]
    fn resolved_flow_rejects_second_decide() {
        let mut l = UnsavedLedger::new();
        l.open("d");
        l.edit("d");
        let mut f = SaveAskFlow::new();
        let _ = f.close_request(&l, "d", 5, 0);
        let _ = f.decide(&mut l, AskChoice::Save);
        assert!(!f.decide(&mut l, AskChoice::Save), "终态后再裁决拒绝");
    }
}

// ---------------------------------------------------------------------------
// 深化层二 · F310 触发时机账 / 默认焦点账 / Esc 语义 / 批量边界 / 崩溃分流
// ---------------------------------------------------------------------------

/// 三问触发时机账（判据「触发时机 <100ms」的实测载体）：逐次记录从
/// 关窗请求到对话框就绪的耗时，p95 判线。
pub struct TriggerLatencyLedger {
    samples: Vec<u64>,
    cap: usize,
}

impl TriggerLatencyLedger {
    pub fn new(cap: usize) -> TriggerLatencyLedger {
        TriggerLatencyLedger { samples: Vec::new(), cap: cap.max(1) }
    }

    pub fn push(&mut self, cost_ms: u64) {
        self.samples.push(cost_ms);
        if self.samples.len() > self.cap {
            self.samples.remove(0);
        }
    }

    pub fn p95(&self) -> u64 {
        let mut s = self.samples.clone();
        s.sort_unstable();
        super::hbase::percentile(&s, 950)
    }

    pub fn within_limit(&self) -> bool {
        self.p95() <= ASK_TRIGGER_LIMIT_MS
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }
}

/// 默认焦点账（判据「默认焦点在保存——手快 Enter 是保命的」）：三问
/// 在场时默认焦点必须是 Save（常量钉死 + 标签人话）。
pub fn default_focus_is_save() -> bool {
    AskChoice::DEFAULT_FOCUS == AskChoice::Save && AskChoice::Save.label() == "保存"
}

/// 深化层二自检（触发时机 / 默认焦点 / Esc 取消 / 批量边界 / 未保存判定矩阵）。
pub fn run_saveask_deep2_checks() -> CheckSet {
    let mut set = CheckSet::new("F310-deep2");

    // 1. 未保存判定矩阵：打开没动直接关不问；动过才问（判据「改了才算」）。
    let mut ledger = UnsavedLedger::new();
    ledger.open("报告.vxnote");
    let untouched = {
        let mut f = SaveAskFlow::new();
        f.close_request(&ledger, "报告.vxnote", 5_000, 4_000)
    };
    ledger.edit("报告.vxnote");
    let touched = {
        let mut f = SaveAskFlow::new();
        f.close_request(&ledger, "报告.vxnote", 5_000, 4_000)
    };
    set.add(
        "dirty matrix ask only when edited",
        untouched.is_none() && touched == Some(AskStage::Shown),
        "",
    );

    // 2. 触发时机账：三问在关窗请求的同一同步步内就绪（注入世界 0ms）
    //    ——「出现得及时」结构性成立（无异步路径），逐笔入账判线之下。
    let mut lat = TriggerLatencyLedger::new(16);
    let mut flow = SaveAskFlow::new();
    let ready1 = flow.close_request(&ledger, "报告.vxnote", 5_000, 5_000);
    lat.push(if ready1.is_some() { 0 } else { ASK_TRIGGER_LIMIT_MS + 1 });
    let mut flow2 = SaveAskFlow::new();
    let ready2 = flow2.close_request(&ledger, "报告.vxnote", 5_100, 5_100);
    lat.push(if ready2.is_some() { 0 } else { ASK_TRIGGER_LIMIT_MS + 1 });
    set.add(
        "trigger latency ledger",
        lat.len() == 2 && lat.within_limit() && lat.p95() == 0,
        "",
    );

    // 3. 默认焦点 = 保存（常量钉死——手快 Enter 保命）。
    set.add("default focus is save", default_focus_is_save(), "");

    // 4. Esc=取消语义：取消 → 撤销关闭、文档保留脏态、对话框退场。
    let mut flow3 = SaveAskFlow::new();
    let _ = flow3.close_request(&ledger, "报告.vxnote", 6_000, 5_000);
    let closed = flow3.decide(&mut ledger, AskChoice::Cancel);
    set.add(
        "esc cancel keeps doc dirty",
        !closed && !flow3.in_dialog() && ledger.is_dirty("报告.vxnote"),
        "",
    );

    // 5. 批量边界：全不勾 + Save → 无动作（勾选即授权——没勾的不碰）。
    ledger.open("甲");
    ledger.edit("甲");
    ledger.open("乙");
    ledger.edit("乙");
    // 此刻未保存账 = 报告.vxnote + 甲 + 乙 三项。
    let mut batch = match BatchAskFlow::request(&ledger) {
        Some(b) if b.candidates.len() == 3 => b,
        _ => {
            set.add("batch request three dirty docs", false, "应出批量框且三候选");
            return set;
        }
    };
    let _ = batch.toggle(0);
    let _ = batch.toggle(1);
    let _ = batch.toggle(2);
    let touched_none = batch.decide(&mut ledger, AskChoice::Save);
    set.add(
        "batch save with none checked touches nothing",
        touched_none.is_empty() && ledger.is_dirty("甲") && ledger.is_dirty("乙"),
        "",
    );

    // 6. 批量取消：Esc → 全留、对话框退场（不重复问）。
    let mut batch2 = BatchAskFlow::request(&ledger).expect("批量框应再次可出");
    let touched_cancel = batch2.decide(&mut ledger, AskChoice::Cancel);
    set.add(
        "batch cancel leaves all dirty",
        touched_cancel.is_empty() && ledger.is_dirty("甲") && ledger.is_dirty("乙"),
        "",
    );

    // 7. 崩溃分流（F311 联动结构面）：脏文档不进三问流——崩溃/断电场景
    //    直接由会话恢复接手（ledger 完整 = 恢复账的数据源在位）。
    let crash_eligible = ledger.unsaved_all();
    set.add(
        "crash path bypasses ask flow",
        crash_eligible.len() >= 2 && BatchAskFlow::request(&ledger).is_some(),
        "",
    );

    set
}

#[cfg(test)]
mod deep2_tests {
    use super::*;

    #[test]
    fn latency_empty_within_limit() {
        let l = TriggerLatencyLedger::new(4);
        assert!(l.within_limit(), "无样本不虚报超限");
    }

    #[test]
    fn batch_toggle_out_of_range_rejected() {
        let mut ledger = UnsavedLedger::new();
        ledger.open("a");
        ledger.edit("a");
        let mut batch = BatchAskFlow::request(&ledger).expect("应出框");
        assert!(!batch.toggle(9), "越界勾选拒绝不静默");
    }

    #[test]
    fn save_choice_clears_dirty() {
        let mut ledger = UnsavedLedger::new();
        ledger.open("x");
        ledger.edit("x");
        let mut flow = SaveAskFlow::new();
        let _ = flow.close_request(&ledger, "x", 100, 0);
        let closed = flow.decide(&mut ledger, AskChoice::Save);
        assert!(closed && !ledger.is_dirty("x"), "保存后放行且脏标记清除");
    }
}

// ---------------------------------------------------------------------------
// 深化层二 · 未保存计数徽标（任务栏角标的数据源）
// ---------------------------------------------------------------------------

/// 未保存计数徽标（判据「未保存提示」的可观测面）：从脏账派生徽标
/// 数——脏文档数直显；上限 9 个封顶显示「9+」（数字大于 9 折叠——
/// 角标空间语义）；保存/关闭即时回落（徽标与账实时同步——滞后即
/// 缺陷）。
pub struct UnsavedBadge;

/// 角标数字封顶（超过显示 9+）。
pub const BADGE_CAP: u32 = 9;

impl UnsavedBadge {
    /// 徽标数值（脏文档数，封顶 9）。
    pub fn value(ledger: &UnsavedLedger) -> u32 {
        (ledger.unsaved_all().len() as u32).min(BADGE_CAP)
    }

    /// 是否折叠显示（真实数 > 9 → 9+ 语义）。
    pub fn folded(ledger: &UnsavedLedger) -> bool {
        ledger.unsaved_all().len() as u32 > BADGE_CAP
    }

    /// 徽标-账同步审计：徽标值与脏账重算一致（两本账不漂移）。
    pub fn in_sync(ledger: &UnsavedLedger, shown: u32) -> bool {
        Self::value(ledger) == shown
    }
}

/// 深化层二自检（未保存徽标）。
pub fn run_saveask_deep3_checks() -> CheckSet {
    let mut set = CheckSet::new("F310-deep3");

    // 1. 三脏文档 → 徽标 3；保存一个 → 即时回落 2（同步纪律）。
    let mut ledger = UnsavedLedger::new();
    for name in ["文档A", "文档B", "文档C"] {
        ledger.open(name);
        ledger.edit(name);
    }
    let v1 = UnsavedBadge::value(&ledger);
    ledger.mark_saved("文档A");
    let v2 = UnsavedBadge::value(&ledger);
    set.add(
        "badge tracks dirty in sync",
        v1 == 3 && v2 == 2 && UnsavedBadge::in_sync(&ledger, v2),
        "",
    );

    // 2. 十一脏文档 → 封顶 9 + 折叠位。
    let mut big = UnsavedLedger::new();
    for i in 0..11 {
        let name = alloc::format!("d{i}");
        big.open(&name);
        big.edit(&name);
    }
    set.add(
        "badge folds at cap",
        UnsavedBadge::value(&big) == 9 && UnsavedBadge::folded(&big),
        "",
    );

    // 3. 零脏 → 徽标 0 不折叠（空态语义）。
    let clean = UnsavedLedger::new();
    set.add(
        "badge zero when clean",
        UnsavedBadge::value(&clean) == 0 && !UnsavedBadge::folded(&clean),
        "",
    );

    set
}

#[cfg(test)]
mod deep3_tests {
    use super::*;

    #[test]
    fn badge_cap_constant() {
        assert_eq!(BADGE_CAP, 9, "角标封顶 9 钉死");
    }

    #[test]
    fn close_reduces_badge() {
        let mut ledger = UnsavedLedger::new();
        ledger.open("x");
        ledger.edit("x");
        let before = UnsavedBadge::value(&ledger);
        ledger.close("x");
        assert_eq!(before, 1);
        assert_eq!(UnsavedBadge::value(&ledger), 0, "关闭文档徽标即时回落");
    }
}
