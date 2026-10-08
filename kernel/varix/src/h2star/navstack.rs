//! F266 后退/前进与 Alt+方向键 · 完整设计（STAR I 主册 H 域）。
//!
//! **判据（主册）**：栈语义用例（后退-改向-前进置灰）；下拉跳步；跨
//! 标签独立；栈深上限 100 淘汰最旧。
//!
//! **设计要点（主册）**：资源管理器（含打开/保存对话框）维护导航历史
//! 栈：Alt+左/右与工具栏 ←→ 同义，后退下拉列出历史点（可跳多步）；
//! 前进栈在岔路（跳去别处）时清空（标准栈语义）；历史跨标签页独立；
//! 新开窗口从默认位置起栈。
//!
//! 实装：`NavStack`（当前位 + 后退栈 + 前进栈，标准栈语义）；跳步
//! （后退/前进多步一次到位）；岔路改向清前进栈；栈深 100 淘汰最旧；
//! 每标签页独立实例（类型即隔离——判据的结构保证）。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

/// 栈深上限（判据定值）。
pub const STACK_CAP: usize = 100;

/// 导航历史栈（每标签页一个实例——跨标签独立的结构保证）。
pub struct NavStack {
    back: Vec<String>,
    forward: Vec<String>,
    current: String,
}

/// 导航动作结果（置灰语义的显式表达）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NavState {
    pub can_back: bool,
    pub can_forward: bool,
    pub back_len: usize,
    pub forward_len: usize,
}

impl NavStack {
    /// 新栈：从默认位置起（判据「新开窗口从默认位置起栈」）。
    pub fn new(start: &str) -> NavStack {
        NavStack {
            back: Vec::new(),
            forward: Vec::new(),
            current: String::from(start),
        }
    }

    /// 跳转到新位置（前进栈在岔路清空——标准栈语义）。
    pub fn navigate(&mut self, to: &str) {
        if to == self.current {
            return; // 同位不重复入栈。
        }
        self.back.push(core::mem::take(&mut self.current));
        if self.back.len() > STACK_CAP {
            self.back.remove(0); // 淘汰最旧。
        }
        self.forward.clear();
        self.current = String::from(to);
    }

    /// 后退一步；到栈底返回 None（按钮置灰判据）。
    pub fn back(&mut self) -> Option<String> {
        let prev = self.back.pop()?;
        Some(core::mem::replace(&mut self.current, prev))
    }

    /// 前进一步；前进栈空返回 None（置灰）。
    pub fn forward(&mut self) -> Option<String> {
        let next = self.forward.pop()?;
        Some(core::mem::replace(&mut self.current, next))
    }

    /// 后退 N 步（下拉跳步——一次到位）。N 超过栈深按栈深钳制。
    pub fn back_n(&mut self, n: usize) -> Option<String> {
        let n = n.min(self.back.len());
        for _ in 0..n {
            let cur = core::mem::take(&mut self.current);
            self.forward.push(cur);
            self.current = self.back.pop().unwrap_or_default();
        }
        if n == 0 {
            None
        } else {
            Some(self.current.clone())
        }
    }

    /// 前进 N 步（下拉跳步）。
    pub fn forward_n(&mut self, n: usize) -> Option<String> {
        let n = n.min(self.forward.len());
        for _ in 0..n {
            let cur = core::mem::take(&mut self.current);
            self.back.push(cur);
            self.current = self.forward.pop().unwrap_or_default();
        }
        if n == 0 {
            None
        } else {
            Some(self.current.clone())
        }
    }

    /// 后退下拉历史点（最近优先——下拉列表数据源）。
    pub fn back_history(&self) -> Vec<&str> {
        self.back.iter().rev().map(|s| s.as_str()).collect()
    }

    /// 状态快照（按钮置灰渲染直读）。
    pub fn state(&self) -> NavState {
        NavState {
            can_back: !self.back.is_empty(),
            can_forward: !self.forward.is_empty(),
            back_len: self.back.len(),
            forward_len: self.forward.len(),
        }
    }

    pub fn current(&self) -> &str {
        &self.current
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_navstack_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-F266");
    // 栈语义：A→B→C，后退到 B，改向 D → 前进置灰。
    let mut s = NavStack::new("vx:/A");
    s.navigate("vx:/B");
    s.navigate("vx:/C");
    let _ = s.back(); // 到 B。
    s.navigate("vx:/D");
    let st = s.state();
    set.add(
        "F266 fork clears forward",
        st.can_back && !st.can_forward && s.current() == "vx:/D",
        "standard stack",
    );
    // 下拉跳步：A→B→C→D→E，后退 4 步一次到 A。
    let mut s2 = NavStack::new("vx:/A");
    for p in ["B", "C", "D", "E"] {
        s2.navigate(&alloc::format!("vx:/{}", p));
    }
    let jumped = s2.back_n(4);
    set.add(
        "F266 dropdown jump",
        jumped.as_deref() == Some("vx:/A") && s2.back_history().is_empty(),
        "n-step back",
    );
    let fwd = s2.forward_n(2);
    set.add("F266 forward jump", fwd.as_deref() == Some("vx:/C"), "n-step forward");
    // 跨标签独立。
    let mut t1 = NavStack::new("vx:/甲");
    let mut t2 = NavStack::new("vx:/乙");
    t1.navigate("vx:/甲2");
    set.add(
        "F266 per-tab isolated",
        t1.state().can_back && !t2.state().can_back,
        "two stacks",
    );
    // 栈深上限 100 淘汰最旧。
    let mut s3 = NavStack::new("vx:/0");
    for i in 1..=120 {
        s3.navigate(&alloc::format!("vx:/{}", i));
    }
    let st3 = s3.state();
    set.add(
        "F266 cap 100 evict oldest",
        st3.back_len == STACK_CAP && !s3.back_history().contains(&"vx:/19"),
        "oldest gone",
    );
    // 栈底置灰。
    let mut s4 = NavStack::new("vx:/首页");
    set.add(
        "F266 bottom greyed",
        !s4.state().can_back && !s4.state().can_forward,
        "fresh window",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f266_stack_semantics() {
        let set = run_navstack_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F266 自检红 {f}/{p}");
    }

    #[test]
    fn same_location_noop() {
        let mut s = NavStack::new("vx:/A");
        s.navigate("vx:/A");
        assert!(!s.state().can_back, "原地跳转不入栈");
    }
}
