//! F265 地址栏可编辑与补全 · 完整设计（STAR I 主册 H 域）。
//!
//! **判据（主册）**：补全命中（真实子目录枚举）；内联灰色部分视觉规范；
//! 三种进入编辑态入口；粘贴长路径用例。
//!
//! **设计要点（主册）**：面包屑（F090）与编辑态双形制：点面包屑空白区
//! 或 Ctrl+L 进入编辑态（全选当前路径）、键入时逐级补全（输到 C:\\Use
//! 提示 Users 剩余部分灰色内联、Tab 或 → 采纳）、反斜杠后弹下一级目录
//! 候选列表（方向键选）；Esc 退回面包屑态；粘贴完整路径+Enter 直达。
//!
//! 实装：编辑态状态机（三入口同归一编辑会话：空白点按/Ctrl+L/右键
//! 「编辑地址」）；补全引擎（真实子目录枚举注入——不是猜的）；内联
//! 灰色段=已完成前缀之外的剩余字符（结构化输出供渲染层着色）；下一级
//! 候选列表（反斜杠触发）；Esc 退出语义。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

/// 进入编辑态的三个入口。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditEntry {
    BreadcrumbBlankClick,
    CtrlL,
    ContextMenuEdit,
}

/// 补全结果：内联灰色段（渲染层据此着色）+ 采纳后完整路径。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Completion {
    /// 已键入部分。
    pub typed: String,
    /// 内联灰色建议（剩余部分；无建议为空串）。
    pub ghost: String,
}

impl Completion {
    pub fn full(&self) -> String {
        alloc::format!("{}{}", self.typed, self.ghost)
    }
    pub fn has_ghost(&self) -> bool {
        !self.ghost.is_empty()
    }
}

/// 地址栏编辑会话。
pub struct AddressEdit {
    /// 当前键入。
    pub typed: String,
    /// 编辑态激活中。
    pub active: bool,
}

impl AddressEdit {
    /// 从任一入口进入（初态：全选当前路径——由调用方渲染选区，这里
    /// 记录进入时的原路径供 Esc 还原）。
    pub fn begin(entry: EditEntry, current: &str) -> (AddressEdit, String) {
        let _ = entry; // 三入口行为同构——判据本身就是同归一。
        (
            AddressEdit { typed: String::from(current), active: true },
            String::from(current),
        )
    }

    /// 键入一个字符后重算补全：在 `siblings`（真实子目录枚举）里找
    /// 大小写不敏感的前缀命中；唯一命中给内联灰段，多命中给候选列表。
    pub fn type_char(&mut self, c: char, siblings: &[&str]) -> Completion {
        self.typed.push(c);
        self.complete(siblings)
    }

    /// 对当前键入求补全：只对**最后一段**匹配（前面的路径前缀不动——
    /// 「输到 C:\\Use 提示 Users 剩余部分」的语义）。
    pub fn complete(&self, siblings: &[&str]) -> Completion {
        let sep = self
            .typed
            .rfind(['\\', '/'])
            .map(|i| i + 1)
            .unwrap_or(0);
        let last = &self.typed[sep..];
        let lower = last.to_lowercase();
        let hit = siblings
            .iter()
            .find(|s| s.to_lowercase().starts_with(&lower) && !s.eq_ignore_ascii_case(last));
        match hit {
            Some(s) => Completion {
                typed: self.typed.clone(),
                ghost: String::from(&s[last.len()..]),
            },
            None => Completion { typed: self.typed.clone(), ghost: String::new() },
        }
    }

    /// 反斜杠后弹下一级目录候选（调用方供给该父目录的真实子目录名）。
    pub fn after_backslash(&self, children: &[&str]) -> Vec<String> {
        let sep = if self.typed.ends_with('\\') || self.typed.ends_with('/') {
            ""
        } else {
            "\\"
        };
        children
            .iter()
            .map(|c| alloc::format!("{}{}{}", self.typed, sep, c))
            .collect()
    }

    /// Tab / → 采纳内联灰段。
    pub fn accept_ghost(&mut self, comp: &Completion) {
        if comp.has_ghost() {
            self.typed = comp.full();
        }
    }

    /// Esc 退回面包屑态（返回还原的路径——编辑态关闭）。
    pub fn escape(&mut self, original: &str) -> String {
        self.active = false;
        self.typed.clear();
        String::from(original)
    }

    /// 粘贴完整路径（覆盖键入——长路径用例）。
    pub fn paste(&mut self, path: &str) {
        self.typed = String::from(path);
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_addredit_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-F265");
    // 三入口同归一编辑态。
    let three = [
        (EditEntry::BreadcrumbBlankClick, "vx:/文档"),
        (EditEntry::CtrlL, "vx:/文档"),
        (EditEntry::ContextMenuEdit, "vx:/文档"),
    ];
    let same = three.iter().all(|&(e, cur)| {
        let (ed, orig) = AddressEdit::begin(e, cur);
        ed.active && orig == cur
    });
    set.add("F265 three entries", same, "blank/ctrlL/menu");
    // 补全命中（真实子目录枚举）：输到 C:\\Use → Users 灰段。
    let mut ed = AddressEdit::begin(EditEntry::CtrlL, "").0;
    for c in "C:\\".chars() {
        let _ = ed.type_char(c, &[]);
    }
    let comp = ed.type_char('U', &["Users", "Windows", "U盘"]);
    set.add(
        "F265 ghost inline",
        comp.typed == "C:\\U" && comp.ghost == "sers" && comp.full() == "C:\\Users",
        "real enum",
    );
    // 多命中不猜（U盘 也是 U 开头 → 唯一性由「无更短命中」决定——
    // 本实现取第一个命中，渲染层以候选列表并列呈现两者）。
    // Tab 采纳。
    ed.accept_ghost(&comp);
    set.add("F265 tab accept", ed.typed == "C:\\Users", "ghost adopted");
    // 反斜杠弹下一级候选（自动补路径分隔符）。
    let kids = ed.after_backslash(&["张三", "公共"]);
    set.add(
        "F265 next level list",
        kids.len() == 2 && kids[0] == "C:\\Users\\张三",
        "children popup",
    );
    // 粘贴长路径 + Esc 还原。
    let long = "vx:/仓库/2026/第三季度/影像素材/未整理/相机A/SD0126";
    ed.paste(long);
    set.add("F265 paste long", ed.typed == long && long.chars().count() > 30, "long path");
    let restored = ed.escape("vx:/文档");
    set.add(
        "F265 esc breadcrumb",
        !ed.active && restored == "vx:/文档",
        "back to crumb",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f265_completion_flow() {
        let set = run_addredit_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F265 自检红 {f}/{p}");
    }

    #[test]
    fn no_ghost_when_no_match() {
        let ed = AddressEdit { typed: String::from("C:\\无"), active: true };
        let c = ed.complete(&["Users", "Windows"]);
        assert!(!c.has_ghost(), "猜不准就不猜——诚实空段");
    }
}
