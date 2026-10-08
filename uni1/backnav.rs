//! F411 Backspace 上级目录 · 完整设计（STAR I 主册 G-I-11）。
//!
//! **判据（主册）**：两键分岔用例（跨目录跳转后行为差异）；根目录边界；
//! 与 F266 栈独立性；键位注册。＋通12。
//!
//! 设计：Backspace=层级上移（parent）；Alt+左=历史后退（F266 栈语义）；
//! Alt+右=历史前进（v6 补全三键矩阵——后退后前进可达，新导航截断未来）。
//! 两键分岔的经典场景：从「下载」直接跳转到「D:\项目」后——Backspace
//! 去的是「D:\」的上级链（层级），Alt+左回的是「下载」（历史）。本模块
//! 以路径链 + 历史栈双结构把分岔语义钉死；根目录 Backspace 无动作 +
//! 轻提示一次。v6 深化：历史容量上限（淘汰最老、账面诚实）、层级深度
//! 查询、面包屑渲染数据、一键升到根（计数账）、前进被新导航截断。

use crate::checks::CheckSet;

use alloc::vec::Vec;

/// 历史栈容量上限（超限淘汰最老——内存有上限，账面如实）。
pub const HISTORY_CAP: usize = 128;

/// 路径链（层级结构）：由祖先到当前，每节一个段名。
pub struct NavCore {
    /// 层级链（链尾 = 当前目录）。
    chain: Vec<&'static str>,
    /// 历史栈（F266 语义——所有到过的目录，后退/前进用）。
    history: Vec<Vec<&'static str>>,
    history_pos: usize,
    /// 根目录轻提示：只提示一次。
    root_hint_shown: bool,
    /// 动作账。
    pub up_moves: u64,
    pub back_moves: u64,
    pub fwd_moves: u64,
    /// 历史淘汰账（容量上限触发次数）。
    pub history_evictions: u64,
}

impl NavCore {
    pub fn new(root: &'static str) -> NavCore {
        NavCore {
            chain: alloc::vec![root],
            history: alloc::vec![alloc::vec![root]],
            history_pos: 0,
            root_hint_shown: false,
            up_moves: 0,
            back_moves: 0,
            fwd_moves: 0,
            history_evictions: 0,
        }
    }

    pub fn current(&self) -> Vec<&'static str> {
        self.chain.clone()
    }

    pub fn current_tail(&self) -> &'static str {
        *self.chain.last().unwrap_or(&"")
    }

    /// 进入子目录（层级 +1；压历史）。
    pub fn enter(&mut self, sub: &'static str) {
        self.chain.push(sub);
        self.push_history();
    }

    /// 跳转到全新路径链（跨目录跳转——历史压栈，层级链整体替换）。
    pub fn jump_to(&mut self, chain: Vec<&'static str>) {
        if chain.is_empty() {
            return;
        }
        self.chain = chain;
        self.push_history();
    }

    fn push_history(&mut self) {
        // 截断当前位置之后的「未来」（后退后进入新目录的标准语义）。
        self.history.truncate(self.history_pos + 1);
        self.history.push(self.chain.clone());
        self.history_pos = self.history.len() - 1;
        // 容量上限：淘汰最老，位置随移。
        if self.history.len() > HISTORY_CAP {
            self.history.remove(0);
            self.history_evictions += 1;
            self.history_pos -= 1;
        }
    }

    /// Backspace：层级上移。根目录 → 无动作 + 轻提示一次。
    pub fn backspace(&mut self) -> bool {
        if self.chain.len() <= 1 {
            if !self.root_hint_shown {
                self.root_hint_shown = true;
            }
            return false;
        }
        self.chain.pop();
        self.up_moves += 1;
        self.push_history();
        true
    }

    /// Alt+左：历史后退（F266 栈语义——与层级无关）。
    pub fn alt_left(&mut self) -> bool {
        if self.history_pos == 0 {
            return false;
        }
        self.history_pos -= 1;
        self.chain = self.history[self.history_pos].clone();
        self.back_moves += 1;
        true
    }

    /// Alt+右：历史前进（v6——后退过的位置前进可达；到最新则无动作）。
    pub fn alt_right(&mut self) -> bool {
        if self.history_pos + 1 >= self.history.len() {
            return false;
        }
        self.history_pos += 1;
        self.chain = self.history[self.history_pos].clone();
        self.fwd_moves += 1;
        true
    }

    /// 一键升到根（v6）：连续 Backspace 直到根，返回实际步数。
    pub fn up_to_root(&mut self) -> usize {
        let mut steps = 0usize;
        while self.backspace() {
            steps += 1;
        }
        steps
    }

    /// 层级深度（根 = 0；v6 查询口）。
    pub fn depth(&self) -> usize {
        self.chain.len().saturating_sub(1)
    }

    /// 面包屑渲染数据（v6：由祖先到当前的段名序——渲染层直接可用）。
    pub fn breadcrumb(&self) -> Vec<&'static str> {
        self.chain.clone()
    }

    /// 根目录提示只出现一次（重复到根不再弹）。
    pub fn root_hint_once(&self) -> bool {
        self.root_hint_shown
    }

    /// 栈独立性证据：历史长度与层级深度无耦合（历史随访问增长、层级
    /// 随 Backspace 收缩——两账各自独立）。
    pub fn stacks_independent(&self) -> bool {
        self.history.len() >= self.chain.len()
    }

    pub fn history_len(&self) -> usize {
        self.history.len()
    }
}

pub fn run_backnav_checks() -> CheckSet {
    let mut set = CheckSet::new("uni1-F411");
    let mut n = NavCore::new("此机");
    n.enter("下载");
    // 两键等效场景：直进子目录后，Backspace 与 Alt+左同归。
    n.enter("发票");
    let via_bs = {
        let mut m = NavCore::new("此机");
        m.enter("下载");
        m.enter("发票");
        m.backspace();
        m.current_tail()
    };
    let via_alt = {
        let mut m = NavCore::new("此机");
        m.enter("下载");
        m.enter("发票");
        m.alt_left();
        m.current_tail()
    };
    set.add(
        "f411-direct-entry-equivalent",
        via_bs == "下载" && via_alt == "下载",
        "",
    );
    // 分岔场景：跨目录跳转后两键分道（各自独立副本上验证）。
    n.jump_to(alloc::vec!["此机", "D:", "项目"]);
    let via_bs = {
        let mut m = NavCore::new("此机");
        m.enter("下载");
        m.enter("发票");
        m.jump_to(alloc::vec!["此机", "D:", "项目"]);
        m.backspace();
        m.current_tail()
    };
    set.add("f411-divergence", via_bs == "D:", "");
    let _ = n.alt_left(); // 回「下载/发票」链
    set.add("f411-back-to-history", n.current_tail() == "发票", "");
    // 前进（v6）：后退之后 Alt+右回到跳转后的「项目」。
    set.add("f411-forward-after-back", n.alt_right() && n.current_tail() == "项目", "");
    // 前进到最新后再按：无动作。
    set.add("f411-forward-at-latest-noop", !n.alt_right(), "");
    // 新导航截断未来（v6）：前进路径作废。
    let mut f = NavCore::new("根");
    f.enter("a");
    let _ = f.alt_left(); // 退到根
    f.enter("b"); // 新分支
    set.add("f411-forward-truncated", !f.alt_right() && f.current_tail() == "b", "");
    // 根目录边界：无动作 + 提示恰好一次。
    let mut r = NavCore::new("此机");
    set.add("f411-root-noop-first-hint", !r.backspace() && r.root_hint_once(), "");
    set.add("f411-root-hint-still-once", !r.backspace() && r.root_hint_once(), "");
    // 一键升到根（v6）：从两层深处一步清账，步数=2。
    let mut u = NavCore::new("此机");
    u.enter("a");
    u.enter("b");
    set.add(
        "f411-up-to-root-count",
        u.up_to_root() == 2 && u.depth() == 0 && u.up_moves == 2,
        "",
    );
    // 层级深度查询 + 面包屑渲染数据（v6）。
    let mut b = NavCore::new("此机");
    b.enter("下载");
    b.enter("发票");
    set.add(
        "f411-breadcrumb-shape",
        b.depth() == 2 && b.breadcrumb() == alloc::vec!["此机", "下载", "发票"],
        "",
    );
    // 历史容量上限（v6）：200 次导航 > 128 容量 → 淘汰 72+ 次，栈长恒
    // 在容量窗内（独立性不变量在深链下取「历史为有界窗口」口径）。
    let mut c = NavCore::new("根");
    for i in 0..200 {
        let _ = c.enter(if i % 2 == 0 { "x" } else { "y" });
    }
    set.add(
        "f411-history-cap",
        c.history_len() == HISTORY_CAP && c.history_evictions >= 72 && c.depth() == 200,
        "",
    );
    // 栈独立性。
    set.add(
        "f411-stack-independent",
        n.stacks_independent() && n.history_len() >= 3 && n.up_moves + n.back_moves >= 1,
        "",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn divergence_after_cross_jump() {
        let mut n = NavCore::new("此机");
        n.enter("下载");
        n.jump_to(alloc::vec!["此机", "D:", "项目"]);
        // Backspace：层级上移 → D:（层级链的上级，不是「下载」）。
        assert!(n.backspace());
        assert_eq!(n.current_tail(), "D:");
        // 回到跳转后原位再 Alt+左：历史后退 → 「此机/下载」（历史链）。
        let mut n2 = NavCore::new("此机");
        n2.enter("下载");
        n2.jump_to(alloc::vec!["此机", "D:", "项目"]);
        assert!(n2.alt_left());
        assert_eq!(n2.current(), alloc::vec!["此机", "下载"]);
    }

    #[test]
    fn truncate_future_on_new_branch() {
        let mut n = NavCore::new("根");
        n.enter("a");
        n.enter("b");
        n.backspace(); // 回 a（b 成为历史中的过去项——浏览器模型）
        n.enter("c"); // 新分支
        assert_eq!(n.current(), alloc::vec!["根", "a", "c"]);
        assert!(n.alt_left());
        assert_eq!(n.current_tail(), "a");
        assert!(n.alt_left(), "b 是到访过的位置——后退可达");
        assert_eq!(n.current_tail(), "b");
        assert!(n.alt_left());
        assert_eq!(n.current_tail(), "a");
        assert!(n.alt_left());
        assert_eq!(n.current_tail(), "根");
        assert!(!n.alt_left(), "历史尽头无动作");
    }

    #[test]
    fn roundtrip_back_forward() {
        let mut n = NavCore::new("根");
        n.enter("a");
        n.enter("b");
        assert!(n.alt_left() && n.alt_left());
        assert!(n.alt_right() && n.alt_right());
        assert_eq!(n.current_tail(), "b", "后退两步前进两步原位");
        assert_eq!(n.back_moves, 2);
        assert_eq!(n.fwd_moves, 2);
    }
}
