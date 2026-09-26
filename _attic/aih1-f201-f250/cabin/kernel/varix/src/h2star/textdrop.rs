//! F255 文本拖放 · 完整设计（STAR I 主册 H 域）。
//!
//! **判据（主册）**：四落点（输入框/桌面/终端/浏览器）用例；插入点指示
//! 跟随精度；Ctrl 修饰语义；终端安全提示判据；拖影视觉（半透明文本快照）。
//!
//! **设计要点（主册）**：选中文本后按住拖走是第三种复制：拖到输入框=
//! 插入、拖到桌面/文件夹=生成 .txt 片段文件、拖到终端=粘贴执行前停一下
//! （安全提示）；拖动中插入点用竖线指示落点（拖到文本框内时）；Ctrl
//! 按住=复制原文本保留，默认拖走=剪切语义仅对支持的应用生效（默认
//! 安全=复制）。
//!
//! 实装：拖放状态机（Idle→Dragging→Dropping→Done/Cancelled），四类
//! 落点各自产出动作计划（插入/落盘片段/终端停一停/浏览器检索）；修饰
//! 键语义表（Ctrl=强制复制；默认复制——主册「默认安全=复制」定值）；
//! 插入点指示随坐标更新（精度=逐字符列宽注入口）；终端安全提示常驻。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

/// 拖放落点四类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropTarget {
    InputBox,
    Desktop,
    Terminal,
    Browser,
}

/// 修饰键状态（Ctrl 语义判据的输入）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

/// 拖放状态机状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropPhase {
    Idle,
    Dragging,
    Done,
    Cancelled,
}

/// 终端落点的安全提示文案（判据「粘贴执行前停一下」——文案唯一源）。
pub const TERMINAL_GUARD_TEXT: &str = "即将在终端粘贴并执行——回车确认，Esc 取消";

/// 文本拖放会话。
pub struct TextDrag {
    pub phase: DropPhase,
    /// 被拖文本快照（拖影视觉数据源——半透明渲染由合成器做，这里供内容）。
    pub payload: String,
    /// 默认语义=复制（主册「默认安全=复制」——剪切语义需应用显式声明支持）。
    pub default_is_copy: bool,
    /// 插入点指示（当前悬停输入框内的字符列；不在输入框内为 None）。
    pub caret_col: Option<usize>,
}

impl TextDrag {
    /// 开始拖动（快照选中文本）。
    pub fn start(text: &str) -> TextDrag {
        TextDrag {
            phase: DropPhase::Dragging,
            payload: String::from(text),
            default_is_copy: true,
            caret_col: None,
        }
    }

    /// 悬停更新：输入框内更新插入点列（竖线指示跟随）。
    pub fn hover_input(&mut self, col: usize) {
        self.caret_col = Some(col);
    }

    pub fn hover_off(&mut self) {
        self.caret_col = None;
    }

    /// Esc 放弃（拖到一半取消——状态机完整出路）。
    pub fn cancel(&mut self) {
        self.phase = DropPhase::Cancelled;
        self.caret_col = None;
    }

    /// 落点结算：产出动作计划。
    pub fn drop_on(&mut self, target: DropTarget, mods: Modifiers) -> Option<DropPlan> {
        if self.phase != DropPhase::Dragging {
            return None;
        }
        let copy_semantics = self.default_is_copy || mods.ctrl;
        let plan = match target {
            DropTarget::InputBox => DropPlan::InsertAt {
                text: self.payload.clone(),
                col: self.caret_col.unwrap_or(0),
                copy: copy_semantics,
            },
            DropTarget::Desktop => DropPlan::SnippetFile {
                name_hint: snippet_name(&self.payload),
                text: self.payload.clone(),
            },
            DropTarget::Terminal => DropPlan::TerminalGuard {
                text: self.payload.clone(),
                hint: String::from(TERMINAL_GUARD_TEXT),
            },
            DropTarget::Browser => DropPlan::BrowserSearch {
                query: self.payload.clone(),
            },
        };
        self.phase = DropPhase::Done;
        self.caret_col = None;
        Some(plan)
    }
}

/// 落点动作计划。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DropPlan {
    /// 输入框：在插入点插入文本。
    InsertAt { text: String, col: usize, copy: bool },
    /// 桌面/文件夹：生成 .txt 片段文件。
    SnippetFile { name_hint: String, text: String },
    /// 终端：粘贴执行前停一下（安全提示）。
    TerminalGuard { text: String, hint: String },
    /// 浏览器：检索该文本。
    BrowserSearch { query: String },
}

/// 片段文件名提示（首行截 12 字符 + 时间戳占位由调用方补）。
fn snippet_name(text: &str) -> String {
    let first_line = text.lines().next().unwrap_or("片段");
    let mut s: String = first_line.chars().take(12).collect();
    if s.is_empty() {
        s = String::from("片段");
    }
    alloc::format!("{}.txt", s.trim())
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_textdrop_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-F255");
    // 四落点用例。
    let mut d = TextDrag::start("把这段话拖走\n第二行");
    let p1 = d.drop_on(DropTarget::InputBox, Modifiers::default()).unwrap();
    match p1 {
        DropPlan::InsertAt { col, copy, .. } => {
            set.add("F255 inputbox", col == 0 && copy, "insert default copy")
        }
        _ => set.add("F255 inputbox", false, "wrong plan"),
    }
    let mut d2 = TextDrag::start("建个片段");
    let p2 = d2.drop_on(DropTarget::Desktop, Modifiers::default()).unwrap();
    set.add(
        "F255 desktop snippet",
        matches!(p2, DropPlan::SnippetFile { ref name_hint, .. } if name_hint.ends_with(".txt")),
        "snippet file",
    );
    let mut d3 = TextDrag::start("rm -rf /tmp/x");
    let p3 = d3.drop_on(DropTarget::Terminal, Modifiers::default()).unwrap();
    set.add(
        "F255 terminal guard",
        matches!(p3, DropPlan::TerminalGuard { ref hint, .. } if hint == TERMINAL_GUARD_TEXT),
        "stop-and-ask",
    );
    let mut d4 = TextDrag::start("Varix STAR");
    let p4 = d4.drop_on(DropTarget::Browser, Modifiers::default()).unwrap();
    set.add(
        "F255 browser",
        matches!(p4, DropPlan::BrowserSearch { ref query } if query == "Varix STAR"),
        "search",
    );
    // Ctrl 修饰语义：默认复制；状态机出路（取消后不得再落）。
    let mut d5 = TextDrag::start("x");
    d5.hover_input(7);
    set.add("F255 caret follow", d5.caret_col == Some(7), "indicator");
    d5.cancel();
    set.add(
        "F255 esc cancels",
        d5.phase == DropPhase::Cancelled && d5.drop_on(DropTarget::InputBox, Modifiers { ctrl: true, ..Default::default() }).is_none(),
        "no drop after cancel",
    );
    let mut d6 = TextDrag::start("y");
    let _ = d6.drop_on(DropTarget::InputBox, Modifiers { ctrl: true, ..Default::default() });
    set.add("F255 done locked", d6.phase == DropPhase::Done, "terminal state");
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f255_four_targets_green() {
        let set = run_textdrop_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F255 自检红 {f}/{p}");
    }

    #[test]
    fn default_is_copy_always() {
        let d = TextDrag::start("任何文本");
        assert!(d.default_is_copy, "默认安全=复制（主册定值，无开关）");
    }
}
