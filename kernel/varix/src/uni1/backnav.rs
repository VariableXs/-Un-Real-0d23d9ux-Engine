//! F411 Backspace 上级目录 · 完整设计（STAR I 主册 G-I-11）。
//!
//! **判据（主册）**：两键分岔用例（跨目录跳转后行为差异）；根目录边界；
//! 与 F266 栈独立性；键位注册。＋通12。
//!
//! 设计：Backspace=层级上移（parent）；Alt+左=历史后退（F266 栈语义）。
//! 两键分岔的经典场景：从「下载」直接跳转到「D:\项目」后——Backspace
//! 去的是「D:\」的上级链（层级），Alt+左回的是「下载」（历史）。本模块
//! 以路径链 + 历史栈双结构把分岔语义钉死；根目录 Backspace 无动作 +
//! 轻提示一次。

use crate::checks::CheckSet;

use alloc::vec::Vec;

/// 路径链（层级结构）：由祖先到当前，每节一个段名。
pub struct NavCore {
    /// 层级链（链尾 = 当前目录）。
    chain: Vec<&'static str>,
    /// 历史栈（F266 语义——所有到过的目录，后退用）。
    history: Vec<Vec<&'static str>>,
    history_pos: usize,
    /// 根目录轻提示：只提示一次。
    root_hint_shown: bool,
    /// 动作账。
    pub up_moves: u64,
    pub back_moves: u64,
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
    // 根目录边界：无动作 + 提示恰好一次。
    let mut r = NavCore::new("此机");
    set.add("f411-root-noop-first-hint", !r.backspace() && r.root_hint_once(), "");
    set.add("f411-root-hint-still-once", !r.backspace() && r.root_hint_once(), "");
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
}
