//! 深化层 · F583 窗口焦点记忆（回炉补深主册【设计要点】未实装机制）。
//!
//! 补深四条（判据唯一源：主册 F583 节）：
//! ①「焦点账本」——窗口→控件交互路径栈：交互即压栈（基础件只存
//!   「最后一件」，栈保全序），切回即从栈顶回放；
//! ②「多区块精确恢复」——长页面分区记账：每区块独立锚点，互不串扰；
//! ③「失效控件降级」——记忆的控件已被删/禁用时沿栈回退到最近合法
//!   控件；全失效诚实落默认序 0（不硬猜中间）；
//! ④「重启清零边界」——重启后首开标志位：首开走 F206 默认序（消费
//!   一次），此后按账回放；会话代账隔离不跨重启。

use crate::checks::CheckSet;
use crate::istar::ibase::ISTAR_DOMAIN;
use crate::istar::focusmem::FocusMemory;

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 焦点交互路径栈
// ---------------------------------------------------------------------------

/// 焦点交互路径栈（会话内全序账——补深基础件「只存最后一件」的单槽）。
pub struct FocusPathStack {
    entries: Vec<(String, u32)>,
    pushes: u32,
}

impl FocusPathStack {
    pub fn new() -> FocusPathStack {
        FocusPathStack { entries: Vec::new(), pushes: 0 }
    }

    /// 交互即压栈。
    pub fn push(&mut self, window: &str, control: u32) {
        self.entries.push((String::from(window), control));
        self.pushes += 1;
    }

    /// 栈顶回放：该窗最近一次交互控件。
    pub fn replay(&self, window: &str) -> Option<u32> {
        self.entries.iter().rev().find(|(w, _)| w == window).map(|(_, c)| *c)
    }

    /// 失效降级回放：从栈顶往回找该窗最近一个合法控件；
    /// 全失效 → None（诚实默认，不硬猜）。
    pub fn replay_valid(&self, window: &str, is_valid: impl Fn(u32) -> bool) -> Option<u32> {
        self.entries
            .iter()
            .rev()
            .find(|(w, c)| w == window && is_valid(*c))
            .map(|(_, c)| *c)
    }

    pub fn pushes(&self) -> u32 {
        self.pushes
    }

    pub fn depth(&self) -> usize {
        self.entries.len()
    }
}

impl Default for FocusPathStack {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 多区块独立锚
// ---------------------------------------------------------------------------

/// 多区块独立锚（每区块一锚——长页面分区精确恢复）。
pub struct SectionAnchors {
    slots: [(String, u32); 8],
    len: usize,
}

impl SectionAnchors {
    pub fn new() -> SectionAnchors {
        SectionAnchors { slots: [(); 8].map(|_| (String::new(), 0)), len: 0 }
    }

    /// 区块锚记账（同区块覆盖更新）。
    pub fn note(&mut self, section: &str, control: u32) -> bool {
        for slot in self.slots[..self.len].iter_mut() {
            if slot.0 == section {
                slot.1 = control;
                return true;
            }
        }
        if self.len < 8 {
            self.slots[self.len] = (String::from(section), control);
            self.len += 1;
            true
        } else {
            false
        }
    }

    /// 区块精确恢复（各锚独立——互不串扰）。
    pub fn restore(&self, section: &str) -> Option<u32> {
        self.slots[..self.len].iter().find(|(s, _)| s == section).map(|(_, c)| *c)
    }

    pub fn sections(&self) -> usize {
        self.len
    }
}

impl Default for SectionAnchors {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 会话焦点账（基础件 FocusMemory + 路径栈 + 重启边界三位一体）
// ---------------------------------------------------------------------------

pub struct SessionBook {
    mem: FocusMemory,
    stack: FocusPathStack,
    /// 重启后首开标志（消费一次——首开走默认序）。
    first_open_pending: bool,
    first_open_used: bool,
}

impl SessionBook {
    /// 新会话（重启后第一本账——旧代栈不随行）。
    pub fn new_session() -> SessionBook {
        SessionBook {
            mem: FocusMemory::new(1),
            stack: FocusPathStack::new(),
            first_open_pending: true,
            first_open_used: false,
        }
    }

    /// 交互记账（栈压一笔 + 基础件覆盖更新——两账同源）。
    pub fn interact(&mut self, window: &str, control: u32) {
        self.stack.push(window, control);
        let _ = self.mem.note_focus(window, control);
    }

    /// 切回：重启后首开走默认序（标志消费一次）；此后基础件账回放。
    pub fn switch_back(&mut self, window: &str) -> u32 {
        let _ = self.mem.restore(window); // 环与激活窗副作用走基础件
        if self.first_open_pending {
            self.first_open_pending = false;
            self.first_open_used = true;
            return 0; // F206 默认序首控件
        }
        self.mem.restore(window)
    }

    /// 失效降级切回：栈上最近合法控件；全失效落默认序 0。
    pub fn switch_back_valid(&mut self, window: &str, is_valid: impl Fn(u32) -> bool) -> u32 {
        let _ = self.mem.restore(window);
        if self.first_open_pending {
            self.first_open_pending = false;
            self.first_open_used = true;
            return 0;
        }
        self.stack.replay_valid(window, is_valid).unwrap_or(0)
    }

    pub fn stack(&self) -> &FocusPathStack {
        &self.stack
    }

    pub fn first_open_used(&self) -> bool {
        self.first_open_used
    }

    pub fn ring_visible(&self) -> bool {
        self.mem.ring_visible()
    }
}

// ---------------------------------------------------------------------------
// 深化自检
// ---------------------------------------------------------------------------

pub fn run_f583_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new(ISTAR_DOMAIN);

    // 1) 焦点账本：交互全序压栈（3 笔）——栈顶回放 = 最后交互控件。
    let mut book = SessionBook::new_session();
    book.interact("记事本", 7);
    book.interact("记事本", 12);
    book.interact("记事本", 3);
    cs.add(
        "path stack full order",
        book.stack().pushes() == 3
            && book.stack().depth() == 3
            && book.stack().replay("记事本") == Some(3),
        "",
    );

    // 2) 重启清零边界：首开走默认序 0（标志消费一次），此后回放栈顶。
    let first = book.switch_back("记事本");
    let second = book.switch_back("记事本");
    cs.add(
        "restart first open default then replay",
        first == 0 && second == 3 && book.first_open_used(),
        "",
    );

    // 3) 多区块精确恢复：三区块独立锚——同区块覆盖、跨区块不串。
    let mut sec = SectionAnchors::new();
    let _ = sec.note("顶部工具区", 2);
    let _ = sec.note("正文编辑区", 9);
    let _ = sec.note("侧栏目录区", 4);
    let _ = sec.note("正文编辑区", 11);
    cs.add(
        "section anchors independent",
        sec.restore("顶部工具区") == Some(2)
            && sec.restore("正文编辑区") == Some(11)
            && sec.restore("侧栏目录区") == Some(4)
            && sec.sections() == 3,
        "",
    );

    // 4) 失效降级：栈序 [5,9,3]，末两笔已失效 → 沿栈回退到 5。
    let mut book4 = SessionBook::new_session();
    book4.interact("设置页", 5);
    book4.interact("设置页", 9);
    book4.interact("设置页", 3);
    let _ = book4.switch_back("设置页"); // 消费首开标志
    let degraded = book4.switch_back_valid("设置页", |c| c != 9 && c != 3);
    cs.add("invalid control degrades along stack", degraded == 5, "");

    // 5) 全失效 → 默认序 0（诚实默认，不硬猜）。
    let none_valid = book4.switch_back_valid("设置页", |_| false);
    cs.add("all invalid falls to default", none_valid == 0, "");

    // 6) 两账同源：栈顶回放与基础件最后交互一致（复用 FocusMemory）。
    let mut book6 = SessionBook::new_session();
    book6.interact("浏览器", 21);
    book6.interact("浏览器", 8);
    let _ = book6.switch_back("浏览器");
    let again = book6.switch_back("浏览器");
    cs.add(
        "stack replay matches basic memory",
        again == 8 && book6.stack().replay("浏览器") == Some(8),
        "",
    );

    // 7) F206 焦点环：切回即亮（复用基础件环账）。
    cs.add("focus ring on restore", book6.ring_visible(), "");

    // 8) 区块锚容量诚实：8 区块封顶、溢出如实报告。
    let mut sec8 = SectionAnchors::new();
    let names = ["s0", "s1", "s2", "s3", "s4", "s5", "s6", "s7", "s8", "s9", "s10", "s11"];
    let mut all = true;
    for n in names {
        all &= sec8.note(n, 1);
    }
    cs.add("section cap honest", !all && sec8.sections() == 8, "");

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replay_unknown_window_none() {
        let mut st = FocusPathStack::new();
        st.push("w", 1);
        assert_eq!(st.replay("other"), None);
    }

    #[test]
    fn degrade_picks_most_recent_valid() {
        let mut st = FocusPathStack::new();
        st.push("w", 4);
        st.push("w", 2);
        st.push("w", 6);
        assert_eq!(st.replay_valid("w", |c| c % 2 == 0), Some(6));
        assert_eq!(st.replay_valid("w", |c| c > 100), None);
    }

    #[test]
    fn first_open_flag_consumed_once() {
        let mut b = SessionBook::new_session();
        b.interact("w", 9);
        assert_eq!(b.switch_back("w"), 0);
        assert_eq!(b.switch_back("w"), 9);
    }
}
