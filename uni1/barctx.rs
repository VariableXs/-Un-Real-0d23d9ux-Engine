//! F419 任务栏右键应用菜单 · 完整设计（STAR I 主册 G-I-19）。
//!
//! **判据（主册）**：两形制清单；最近 3 条与 F074 引擎同源；关闭所有
//! 确认链；固定项语义；菜单项数 ≤8 审计。＋通12。
//!
//! 设计：任务栏图标右键菜单核——未运行形制（打开/固定/从开始菜单取消
//! 固定）与运行中形制（最近 3 条/新窗口/固定/关闭所有窗口）；最近文件
//! 由 F074 引擎注入口送入（同源——同一 Vec 切片）；「关闭所有」多窗
//! 未保存时逐窗 F310 三问（确认链状态机）；菜单项数 ≤8 审计。

use crate::checks::CheckSet;

use alloc::vec::Vec;

/// 菜单项数上限（F215 两项纪律的应用面）。
pub const MENU_MAX: usize = 8;

/// 关闭所有确认链的逐窗裁决。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloseAllVerdict {
    Saved,
    ProceedAfterAsk,
    Cancelled,
}

/// 任务栏右键菜单核。
pub struct BarCtxMenu {
    /// 最近文件注入位（F074 同源——调用方直接送引擎切片）。
    pub recent: Vec<&'static str>,
    /// 关闭所有：待审窗口清单（id, dirty）。
    pub close_all_pending: Vec<(u64, bool)>,
    pub close_all_done: u64,
    pub close_all_cancelled: u64,
    /// 中断未竟账（v6：begin_close_all 覆盖未清空的前链——被弃窗口数）。
    pub close_all_abandoned: u64,
    /// 最近清单重复项剔除账（v6：引擎侧抖动产生的同名去重数）。
    pub recent_deduped: u64,
}

impl BarCtxMenu {
    pub fn new() -> BarCtxMenu {
        BarCtxMenu {
            recent: Vec::new(),
            close_all_pending: Vec::new(),
            close_all_done: 0,
            close_all_cancelled: 0,
            close_all_abandoned: 0,
            recent_deduped: 0,
        }
    }

    /// F074 同源注入口（同一切片，不复制第二份语义）。v6：同名去重——
    /// 引擎侧抖动不该在菜单里出现两行同一文件。
    pub fn sync_recent(&mut self, from_engine: &[&'static str]) {
        self.recent.clear();
        for name in from_engine {
            if self.recent.len() >= 3 {
                break;
            }
            if self.recent.contains(name) {
                self.recent_deduped += 1;
                continue;
            }
            self.recent.push(name);
        }
    }

    /// 最近项激活（v6）：按位返回打开动作 id（渲染层路由用）。
    pub fn activate_recent(&self, idx: usize) -> Option<&'static str> {
        self.recent.get(idx).copied()
    }

    /// 未运行形制清单（固定项语义：已固定则换「取消固定」）。
    pub fn menu_idle(&self, pinned: bool) -> Vec<&'static str> {
        if pinned {
            alloc::vec!["打开", "从任务栏取消固定", "从开始菜单取消固定"]
        } else {
            alloc::vec!["打开", "固定到任务栏", "从开始菜单取消固定"]
        }
    }

    /// 运行中形制清单（最近 3 条动态插入 + 三常驻项；≤8 审计）。
    pub fn menu_running(&self, pinned: bool) -> Vec<&'static str> {
        let mut v: Vec<&'static str> = Vec::new();
        for r in self.recent.iter().take(3) {
            v.push(r);
        }
        v.push("新窗口");
        v.push(if pinned { "从任务栏取消固定" } else { "固定到任务栏" });
        v.push("关闭所有窗口");
        v
    }

    /// 菜单项数 ≤8 审计（两形制都过）。
    pub fn menu_within_cap(&self, pinned: bool) -> bool {
        self.menu_idle(pinned).len() <= MENU_MAX && self.menu_running(pinned).len() <= MENU_MAX
    }

    /// 关闭所有：登记待审窗（含脏标记）。v6：覆盖未清空的前链时，
    /// 被弃窗口如实入账（不静默丢弃）。
    pub fn begin_close_all(&mut self, windows: Vec<(u64, bool)>) {
        if !self.close_all_pending.is_empty() {
            self.close_all_abandoned += self.close_all_pending.len() as u64;
        }
        self.close_all_pending = windows;
    }

    /// 逐窗裁决：干净窗直接关；脏窗三问——ProceedAfterAsk 才关，
    /// Cancelled 中止整链（剩余窗保留）。
    pub fn resolve_window(&mut self, pid: u64, verdict: CloseAllVerdict) -> bool {
        let pos = match self.close_all_pending.iter().position(|(i, _)| *i == pid) {
            Some(p) => p,
            None => return false,
        };
        match verdict {
            CloseAllVerdict::Saved | CloseAllVerdict::ProceedAfterAsk => {
                self.close_all_pending.remove(pos);
                self.close_all_done += 1;
                true
            }
            CloseAllVerdict::Cancelled => {
                self.close_all_cancelled += 1;
                false
            }
        }
    }

    /// 链结束判定：全部窗口处理完毕。
    pub fn close_all_settled(&self) -> bool {
        self.close_all_pending.is_empty()
    }
}

pub fn run_barctx_checks() -> CheckSet {
    let mut set = CheckSet::new("uni1-F419");
    let mut m = BarCtxMenu::new();
    // 未运行形制（两固定态）。
    set.add(
        "f419-idle-unpinned",
        m.menu_idle(false) == alloc::vec!["打开", "固定到任务栏", "从开始菜单取消固定"],
        "",
    );
    set.add(
        "f419-idle-pinned",
        m.menu_idle(true) == alloc::vec!["打开", "从任务栏取消固定", "从开始菜单取消固定"],
        "",
    );
    // 最近 3 条同源注入（引擎给 5 条 → 截 3）。
    m.sync_recent(&["a.docx", "b.txt", "c.md", "d.log", "e.csv"]);
    set.add(
        "f419-recent-3-same-source",
        m.recent == alloc::vec!["a.docx", "b.txt", "c.md"],
        "",
    );
    // 最近项激活按位路由（v6）。
    set.add(
        "f419-activate-recent-by-index",
        m.activate_recent(0) == Some("a.docx") && m.activate_recent(2) == Some("c.md") && m.activate_recent(3).is_none(),
        "",
    );
    // 同名去重（v6）：引擎抖动同名 → 剔除并记账，不占清单位。
    let mut d = BarCtxMenu::new();
    d.sync_recent(&["x.md", "x.md", "y.md", "z.md"]);
    set.add(
        "f419-recent-dedupe",
        d.recent == alloc::vec!["x.md", "y.md", "z.md"] && d.recent_deduped == 1,
        "",
    );
    // 空清单如实。
    let mut e0 = BarCtxMenu::new();
    e0.sync_recent(&[]);
    set.add("f419-recent-empty", e0.recent.is_empty() && e0.recent_deduped == 0, "");
    // 清单不足 3 条如实（去重后更少也不虚补）——sync_recent 是整表
    // 替换语义：重同步后只含本轮有效项。
    d.sync_recent(&["x.md", "x.md"]);
    set.add(
        "f419-dedupe-shrinks-honestly",
        d.recent == alloc::vec!["x.md"] && d.recent_deduped == 2,
        "",
    );
    // 运行中形制：3 最近 + 3 常驻 = 6 ≤ 8。
    let run_menu = m.menu_running(false);
    set.add(
        "f419-running-menu",
        run_menu.len() == 6
            && run_menu[3] == "新窗口"
            && run_menu[4] == "固定到任务栏"
            && run_menu[5] == "关闭所有窗口",
        "",
    );
    set.add(
        "f419-menu-cap-8",
        m.menu_within_cap(false) && m.menu_within_cap(true),
        "",
    );
    // 关闭所有确认链：多窗未保存逐窗三问。
    m.begin_close_all(alloc::vec![(1, false), (2, true), (3, true)]);
    set.add(
        "f419-closeall-clean-direct",
        m.resolve_window(1, CloseAllVerdict::Saved),
        "",
    );
    set.add(
        "f419-closeall-cancel-stops",
        !m.resolve_window(2, CloseAllVerdict::Cancelled) && !m.close_all_settled(),
        "",
    );
    set.add(
        "f419-closeall-proceed",
        m.resolve_window(2, CloseAllVerdict::ProceedAfterAsk) && m.resolve_window(3, CloseAllVerdict::ProceedAfterAsk),
        "",
    );
    set.add(
        "f419-closeall-ledger",
        m.close_all_settled() && m.close_all_done == 3 && m.close_all_cancelled == 1,
        "",
    );
    // 未知窗口裁决拒绝。
    set.add("f419-unknown-window", !m.resolve_window(99, CloseAllVerdict::Saved), "");
    // 中断未竟账（v6）：取消后重开链 → 剩余窗（含被取消裁决留下、
    // 未移出待审的窗）如实务入弃置账，不静默消失。
    let mut n2 = BarCtxMenu::new();
    n2.begin_close_all(alloc::vec![(1, false), (2, true), (3, true)]);
    let _ = n2.resolve_window(1, CloseAllVerdict::Saved);
    let _ = n2.resolve_window(2, CloseAllVerdict::Cancelled); // 2 留在待审（取消≠移除）
    n2.begin_close_all(alloc::vec![(3, true)]); // 重开链只含剩余窗
    set.add(
        "f419-abandoned-ledger",
        n2.close_all_abandoned == 2 && n2.close_all_pending == alloc::vec![(3, true)],
        "",
    );
    // 重开链收口：剩余窗正常裁决完成。
    set.add(
        "f419-resume-after-cancel",
        n2.resolve_window(3, CloseAllVerdict::ProceedAfterAsk)
            && n2.close_all_settled()
            && n2.close_all_done == 2,
        "",
    );
    // 空清单再开链：无弃置（前链已清）。
    n2.begin_close_all(alloc::vec![(9, false)]);
    set.add("f419-clean-rebegin", n2.close_all_abandoned == 2 && n2.close_all_pending.len() == 1, "");
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_never_exceeds_three() {
        let mut m = BarCtxMenu::new();
        m.sync_recent(&["1", "2"]);
        assert_eq!(m.recent.len(), 2, "不足 3 条如实");
        m.sync_recent(&["a", "b", "c", "d"]);
        assert_eq!(m.recent.len(), 3);
    }

    #[test]
    fn pinned_variant_of_running_menu() {
        let mut m = BarCtxMenu::new();
        m.sync_recent(&["r1"]);
        assert_eq!(m.menu_running(true)[2], "从任务栏取消固定");
    }
}
