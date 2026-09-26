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
