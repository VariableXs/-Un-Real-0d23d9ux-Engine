//! F260 行内重命名 · 完整设计（STAR I 主册 H 域）。
//!
//! **判据（主册）**：三入口（F2/单击/慢双击）用例；扩展名隔离判据；
//! 非法字符即时拒绝 8 字符全测；失败行内提示与不弹窗审计。
//!
//! **设计要点（主册）**：F2 或单击已选中项的名或慢双击名区进入行内
//! 编辑：主文件名全选、扩展名不选不参与（改「报告」不会动「.docx」）、
//! Esc 还原退出、Enter 提交、非法字符（\\ / : * ? " < > |）输入时即时
//! 抖动拒绝不等到提交；重命名成功无动画打扰，失败（重名/权限）行内
//! 红字提示不弹窗。
//!
//! 实装：入口枚举三态同归一编辑会话；扩展名隔离（复用
//! [`h2base::ext_split`]）；按键流处理器（可打印字符入段、非法字符
//! 即时抖动拒绝、Backspace、Esc/Enter 语义）；提交校验（重名/非法/
//! 空名）失败返回行内错误——无任何弹窗出口（类型层面就没有对话框
//! 交互，审计=代码结构本身）。

use crate::checks::CheckSet;
use crate::h2star::h2base::{ext_split, has_invalid_char};

use alloc::string::String;

/// 重命名入口（三态同归一编辑会话——判据「三入口用例」）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenameEntry {
    F2Key,
    SingleClickName,
    SlowDoubleClick,
}

/// 按键处理结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyOutcome {
    /// 字符入段。
    Accepted,
    /// 非法字符——即时抖动拒绝（不等到提交）。
    ShakeRejected,
    /// 提交成功。
    Committed,
    /// 提交失败（行内错误，附原因；不弹窗）。
    Failed(InlineError),
    /// Esc 还原退出。
    Reverted,
}

/// 行内错误（无弹窗——只进编辑框下方的红字区）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InlineError {
    Duplicate,
    Empty,
    InvalidChar,
    Permission,
}

/// 行内重命名编辑会话。
pub struct InlineRename {
    /// 主名段（扩展名不在此段——隔离）。
    stem: String,
    /// 扩展名（只读随行，Enter 提交时拼回）。
    ext: String,
    /// 原主名（Esc 还原用）。
    original: String,
    /// 初态全选标志：选中态下键入=整段替换（主册「全选、直接打字即
    /// 替换」判据的机制实现）。
    selected: bool,
    /// 最近一次抖动拒绝是否激活（视觉层读此位）。
    pub shaking: bool,
    /// 当前行内错误（无错为 None）。
    pub error: Option<InlineError>,
}

impl InlineRename {
    /// 从三入口任一开始（初态：主名全选——由调用方按 `stem` 渲染选区）。
    pub fn begin(filename: &str, _entry: RenameEntry) -> InlineRename {
        let (s, e) = ext_split(filename);
        InlineRename {
            stem: String::from(s),
            ext: String::from(e),
            original: String::from(s),
            selected: true,
            shaking: false,
            error: None,
        }
    }

    /// 主名段（选区=整段全选）。
    pub fn stem(&self) -> &str {
        &self.stem
    }

    /// 扩展名（不参与编辑——隔离判据）。
    pub fn ext(&self) -> &str {
        &self.ext
    }

    /// 输入一个字符：非法字符即时抖动拒绝；选中态下合法键入=整段替换。
    pub fn type_char(&mut self, c: char) -> KeyOutcome {
        if has_invalid_char(&alloc::format!("{}", c)) {
            self.shaking = true;
            return KeyOutcome::ShakeRejected;
        }
        if c == '\n' || c == '\t' {
            self.shaking = true;
            return KeyOutcome::ShakeRejected;
        }
        if self.selected {
            self.stem.clear();
            self.selected = false;
        }
        self.shaking = false;
        self.stem.push(c);
        KeyOutcome::Accepted
    }

    /// 退格：选中态=整段清除，否则删末字符。
    pub fn backspace(&mut self) {
        self.shaking = false;
        if self.selected {
            self.stem.clear();
            self.selected = false;
        } else {
            let _ = self.stem.pop();
        }
    }

    /// Esc：还原退出（主名回到原值，重新进入全选态）。
    pub fn revert(&mut self) -> KeyOutcome {
        self.stem = self.original.clone();
        self.selected = true;
        self.shaking = false;
        self.error = None;
        KeyOutcome::Reverted
    }

    /// Enter：提交。`name_exists` 由调用方供给现存名比对（重名判据），
    /// `writable` 供给权限态。失败=行内错误（不弹窗）。
    pub fn commit(
        &mut self,
        name_exists: impl Fn(&str) -> bool,
        writable: bool,
    ) -> KeyOutcome {
        if self.stem.is_empty() {
            self.error = Some(InlineError::Empty);
            return KeyOutcome::Failed(InlineError::Empty);
        }
        if has_invalid_char(&self.stem) {
            self.error = Some(InlineError::InvalidChar);
            return KeyOutcome::Failed(InlineError::InvalidChar);
        }
        if !writable {
            self.error = Some(InlineError::Permission);
            return KeyOutcome::Failed(InlineError::Permission);
        }
        let full = alloc::format!("{}{}", self.stem, self.ext);
        if name_exists(&full) {
            self.error = Some(InlineError::Duplicate);
            return KeyOutcome::Failed(InlineError::Duplicate);
        }
        self.error = None;
        KeyOutcome::Committed
    }

    /// 提交后的完整文件名（成功时供文件系统调用）。
    pub fn full_name(&self) -> String {
        alloc::format!("{}{}", self.stem, self.ext)
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_rename_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-F260");
    // 三入口同归一编辑会话。
    let same = [
        RenameEntry::F2Key,
        RenameEntry::SingleClickName,
        RenameEntry::SlowDoubleClick,
    ]
    .iter()
    .all(|&e| {
        let r = InlineRename::begin("报告.docx", e);
        r.stem() == "报告" && r.ext() == ".docx"
    });
    set.add("F260 three entries", same, "F2/click/slow-dbl");
    // 扩展名隔离：改「报告」动不了「.docx」。
    let mut r = InlineRename::begin("报告.docx", RenameEntry::F2Key);
    for c in "年度总结".chars() {
        let _ = r.type_char(c);
    }
    let out = r.commit(|_| false, true);
    set.add(
        "F260 ext isolated",
        out == KeyOutcome::Committed && r.full_name() == "年度总结.docx",
        "ext untouched",
    );
    // 非法字符 8 个即时拒绝（不等到提交）。
    let mut r2 = InlineRename::begin("新名", RenameEntry::F2Key);
    let eight = ['\\', '/', ':', '*', '?', '"', '<', '>'];
    let all_shake = eight.iter().all(|&c| r2.type_char(c) == KeyOutcome::ShakeRejected);
    set.add(
        "F260 invalid 8 immediate",
        all_shake && r2.shaking && r2.stem() == "新名",
        "shake not commit",
    );
    // 重名 → 行内错误不弹窗（初态全选下键入 b=整段替换 → 候选 b.txt 撞名）。
    let mut r3 = InlineRename::begin("a.txt", RenameEntry::F2Key);
    let _ = r3.type_char('b');
    let fail = r3.commit(|n| n == "b.txt", true);
    set.add(
        "F260 duplicate inline",
        fail == KeyOutcome::Failed(InlineError::Duplicate) && r3.error == Some(InlineError::Duplicate),
        "inline red text",
    );
    // 权限失败 + 空名失败 + Esc 还原。
    let mut r4 = InlineRename::begin("x.log", RenameEntry::F2Key);
    set.add(
        "F260 permission+empty",
        r4.commit(|_| false, false) == KeyOutcome::Failed(InlineError::Permission)
            && {
                let mut e = InlineRename::begin("y.md", RenameEntry::F2Key);
                let _ = e.backspace();
                e.commit(|_| false, true) == KeyOutcome::Failed(InlineError::Empty)
            },
        "perm + empty",
    );
    let mut r5 = InlineRename::begin("keep.me", RenameEntry::F2Key);
    let _ = r5.type_char('Z');
    let _ = r5.revert();
    set.add(
        "F260 esc reverts",
        r5.full_name() == "keep.me" && r5.error.is_none(),
        "restore on esc",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f260_rename_flow() {
        let set = run_rename_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F260 自检红 {f}/{p}");
    }

    #[test]
    fn shake_resets_on_valid_input() {
        let mut r = InlineRename::begin("a", RenameEntry::F2Key);
        let _ = r.type_char(':');
        assert!(r.shaking);
        let _ = r.type_char('好');
        assert!(!r.shaking, "合法输入清除抖动态");
    }
}
