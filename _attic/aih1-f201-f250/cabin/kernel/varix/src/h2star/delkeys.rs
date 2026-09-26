//! F261 删除与 Shift+Delete · 完整设计（STAR I 主册 H 域）。
//!
//! **判据（主册）**：两路删除行为对照；确认框焦点判据；大文件直走流程
//! 提示；占用提示定位到应用（测试：记事本占用时删除）。
//!
//! **设计要点（主册）**：Delete=进回收站（F085，可还原）、Shift+Delete=
//! 永久删除且弹确认对话框（对话框显示件数与总大小、默认焦点在「取消」
//! ——F207 铁律）；大于回收站容量的删除直接走永久流程并提前告知；正在
//! 被占用的文件删除给「文件已在某应用中打开」人话提示（附哪个应用）。
//!
//! 实装：删除路由器（两路分流 + 容量越界直走永久 + 占用定位）；确认框
//! 计划（内容=件数+总大小、默认焦点=取消——F207 铁律结构化钉死）；
//! 占用表注入（哪个应用锁哪个文件——人话提示的数据源）。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

/// 删除路由结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeleteRoute {
    /// 进回收站（F085 可还原）。
    ToTrash { items: Vec<String> },
    /// 永久删除——先弹确认框。
    PermanentConfirm { items: Vec<String>, total_bytes: u64 },
    /// 占用受阻——提示定位到应用。
    BlockedByApp { file: String, holder: String },
    /// 容量越界直走永久的提前告知。
    OversizeNotice { items: Vec<String>, total_bytes: u64 },
}

/// 确认框计划（F207 铁律结构化：默认焦点恒为「取消」）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfirmPlan {
    pub headline: String,
    pub detail: String,
    /// 默认焦点按钮——恒为取消（保命铁律，写死无开关）。
    pub default_focus: ConfirmFocus,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfirmFocus {
    Cancel,
}

/// 删除服务。
pub struct DeleteRouter {
    /// 回收站容量（字节）。
    pub trash_cap_bytes: u64,
    /// 占用表：(文件, 占用应用)。
    locks: Vec<(String, String)>,
}

impl DeleteRouter {
    pub fn new(trash_cap_bytes: u64) -> DeleteRouter {
        DeleteRouter { trash_cap_bytes, locks: Vec::new() }
    }

    /// 注入占用登记（如记事本正在编辑某文件）。
    pub fn lock(&mut self, file: &str, app: &str) {
        self.locks.push((String::from(file), String::from(app)));
    }

    pub fn unlock(&mut self, file: &str) {
        self.locks.retain(|(f, _)| f != file);
    }

    fn holder_of(&self, file: &str) -> Option<&str> {
        self.locks.iter().find(|(f, _)| f == file).map(|(_, a)| a.as_str())
    }

    /// 删除路由：`permanent`=Shift 按住。先查占用（定位到应用），
    /// 再按容量与修饰键分流。
    pub fn route(
        &self,
        items: &[(String, u64)],
        permanent: bool,
    ) -> DeleteRoute {
        // 占用检查优先——任何一个被占用即受阻并定位应用。
        for (f, _) in items {
            if let Some(app) = self.holder_of(f) {
                return DeleteRoute::BlockedByApp {
                    file: f.clone(),
                    holder: String::from(app),
                };
            }
        }
        let names: Vec<String> = items.iter().map(|(f, _)| f.clone()).collect();
        let total: u64 = items.iter().map(|(_, s)| *s).sum();
        let over = total > self.trash_cap_bytes;
        if permanent || over {
            if over {
                // 容量越界：直走永久流程但给提前告知（不弹确认前的惊吓，
                // 告知本身即路由结果——调用方渲染 OversizeNotice 文案）。
                return DeleteRoute::OversizeNotice { items: names, total_bytes: total };
            }
            return DeleteRoute::PermanentConfirm { items: names, total_bytes: total };
        }
        DeleteRoute::ToTrash { items: names }
    }

    /// 永久确认框计划：件数 + 总大小 + 默认焦点取消。
    pub fn confirm_plan(items: usize, total_bytes: u64) -> ConfirmPlan {
        ConfirmPlan {
            headline: String::from("要永久删除这些文件吗？"),
            detail: alloc::format!("共 {} 项，{} 字节。此操作不可撤销。", items, total_bytes),
            default_focus: ConfirmFocus::Cancel,
        }
    }

    /// 占用人话提示（判据原文口径：附哪个应用）。
    pub fn blocked_text(file: &str, holder: &str) -> String {
        alloc::format!("「{}」已在 {} 中打开——请先在 {} 里关闭它再删除", file, holder, holder)
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_delkeys_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-F261");
    let mut r = DeleteRouter::new(100_000_000);
    // 两路对照：Delete 进回收站；Shift+Delete 走确认。
    let items = alloc::vec![
        (String::from("a.txt"), 10u64),
        (String::from("b.txt"), 20u64),
    ];
    set.add(
        "F261 two routes",
        matches!(r.route(&items, false), DeleteRoute::ToTrash { .. })
            && matches!(r.route(&items, true), DeleteRoute::PermanentConfirm { .. }),
        "delete vs shift-del",
    );
    // 确认框：件数+总大小+默认焦点取消。
    let plan = DeleteRouter::confirm_plan(2, 30);
    set.add(
        "F261 confirm plan",
        plan.default_focus == ConfirmFocus::Cancel
            && plan.detail.contains("2 项")
            && plan.detail.contains("30 字节"),
        "F207 focus cancel",
    );
    // 大文件越界直走永久并提前告知。
    let big = alloc::vec![(String::from("huge.bin"), 500_000_000u64)];
    set.add(
        "F261 oversize direct",
        matches!(r.route(&big, false), DeleteRoute::OversizeNotice { .. }),
        "skip trash",
    );
    // 占用定位到应用。
    r.lock("笔记.md", "记事本");
    set.add(
        "F261 blocked names app",
        matches!(
            r.route(&alloc::vec![(String::from("笔记.md"), 1u64)], false),
            DeleteRoute::BlockedByApp { ref holder, .. } if holder == "记事本"
        ),
        "记事本 case",
    );
    set.add(
        "F261 blocked text human",
        DeleteRouter::blocked_text("笔记.md", "记事本").contains("记事本"),
        "human wording",
    );
    // 解锁后恢复两路。
    r.unlock("笔记.md");
    set.add(
        "F261 unlock restores",
        matches!(r.route(&alloc::vec![(String::from("笔记.md"), 1u64)], false), DeleteRoute::ToTrash { .. }),
        "after close",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f261_routes_all_green() {
        let set = run_delkeys_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F261 自检红 {f}/{p}");
    }

    #[test]
    fn cancel_focus_is_structural() {
        // F207 铁律：确认框默认焦点没有第二种可能。
        let plan = DeleteRouter::confirm_plan(1, 1);
        assert_eq!(plan.default_focus, ConfirmFocus::Cancel);
    }
}
