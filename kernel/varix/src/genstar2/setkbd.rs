//! F473 设置中心键盘流（genstar2 · I 域通用·二分队 · AI-U2）。
//!
//! 主册判据（验收标准第一句）：
//! **五步全键盘链路录屏；就地卡 Enter 切换；面包屑键盘；焦点回归链
//! （F206）；典型 10 设置任务计时记录。**
//!
//! 功能定义（主册批次三）：设置中心全程键盘化——Ctrl+E 聚焦搜索→输入→
//! Enter 开第一结果（F301 就地操作卡可直接 Enter 切换开关）→Tab 走控件
//! （F434 键位全套）→Esc 逐层退（F424 语义表）；每页面包屑（F090 同源）
//! 键盘可达（Backspace 上层）。
//!
//! 零堆纪律：定长焦点栈与搜索索引，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 五步链路（主册：Ctrl+E→输入→Enter→Tab→Esc）。
pub const KEYBOARD_STEPS: usize = 5;
/// 典型设置任务计时预算（主册：改一个设置最快路径 5 秒纯键盘）。
pub const TASK_BUDGET_MS: u64 = 5_000;
/// 搜索结果索引容量。
pub const RESULT_CAP: usize = 16;
/// 焦点回归链深度（F206：浮层关后焦点还给触发元素——逐层退栈）。
pub const FOCUS_STACK_CAP: usize = 8;

/// 键盘流状态机。
pub struct KbdFlow {
    /// 搜索聚焦态（Ctrl+E 置位）。
    pub search_focused: bool,
    /// 页层级栈（Esc 逐层退）。
    levels: [u8; FOCUS_STACK_CAP],
    level_n: usize,
    /// 结果索引内游标（下键跳过结果卡——F457 共存排序）。
    result_cursor: usize,
    result_n: usize,
}

/// 键事件。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Key {
    CtrlE,
    Char(char),
    Enter,
    Tab,
    Esc,
    Backspace,
    ArrowDown,
}

impl KbdFlow {
    pub const fn new() -> Self {
        KbdFlow {
            search_focused: false,
            levels: [0; FOCUS_STACK_CAP],
            level_n: 1, // 根页
            result_cursor: 0,
            result_n: 0,
        }
    }

    pub fn level(&self) -> usize {
        self.level_n
    }

    /// 键处理（五步链路核心）。
    pub fn on_key(&mut self, k: Key, results: usize) -> bool {
        match k {
            Key::CtrlE => {
                self.search_focused = true; // 第一步：聚焦搜索
                self.result_n = 0;
                true
            }
            Key::Char(_) => self.search_focused, // 第二步：输入
            Key::Enter => {
                if self.search_focused && results > 0 {
                    self.result_n = results; // 第三步：开第一结果
                    self.result_cursor = 0;
                    self.search_focused = false;
                    // 进入详情层。
                    if self.level_n < FOCUS_STACK_CAP {
                        self.level_n += 1;
                    }
                    true
                } else {
                    false
                }
            }
            Key::ArrowDown => {
                // 结果卡可下键跳过（共存排序判据）。
                if self.result_cursor + 1 < self.result_n {
                    self.result_cursor += 1;
                    true
                } else {
                    false
                }
            }
            Key::Tab => {
                self.result_n > 0 // 第四步：Tab 走控件
            }
            Key::Esc => {
                // 第五步：Esc 逐层退（F424 语义：浮层→面板→页）。
                if self.level_n > 1 {
                    self.level_n -= 1;
                    true
                } else {
                    false // 根页 Esc 无动作（无副作用桌面态）
                }
            }
            Key::Backspace => {
                // 面包屑键盘：Backspace 上层（搜索聚焦外才生效）。
                !self.search_focused && self.level_n > 1 && {
                    self.level_n -= 1;
                    true
                }
            }
        }
    }

    pub fn result_cursor(&self) -> usize {
        self.result_cursor
    }

    /// 焦点回归链（F206）：逐层退栈到根后焦点回到触发元素（Ctrl+E 来源）。
    pub fn focus_back_to_trigger(&self) -> bool {
        self.level_n == 1
    }
}

/// 典型 10 设置任务计时模型（主册：典型 10 设置任务计时记录——每任务
/// 五步路径预算 5s，10 任务 ≤50s 总账）。
pub fn ten_task_budget(total_ms: u64) -> bool {
    total_ms < 10 * TASK_BUDGET_MS
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

pub fn run_setkbd_checks() -> CheckSet {
    let mut cs = CheckSet::new("F473-setkbd");
    // 1) 五步全键盘链路（不碰鼠标走完：聚焦→输入→开→走→退）。
    let mut f = KbdFlow::new();
    let s1 = f.on_key(Key::CtrlE, 0);
    let s2 = f.on_key(Key::Char('贴'), 0);
    let s3 = f.on_key(Key::Enter, 3);
    let s4 = f.on_key(Key::Tab, 0);
    let s5 = f.on_key(Key::Esc, 0);
    cs.add("five_step_chain", s1 && s2 && s3 && s4 && s5 && f.focus_back_to_trigger(), "");
    // 2) 就地卡 Enter 切换（第一结果即操作卡——Enter 后层级+1）。
    let mut f2 = KbdFlow::new();
    f2.on_key(Key::CtrlE, 0);
    f2.on_key(Key::Char('w'), 0);
    let lv0 = f2.level();
    f2.on_key(Key::Enter, 2);
    cs.add("enter_opens_first", f2.level() == lv0 + 1 && f2.result_cursor() == 0, "");
    // 3) 结果卡下键跳过（共存排序：置顶可跳；两个结果只可跳一次）。
    cs.add("arrow_skips_card", f2.on_key(Key::ArrowDown, 0) && f2.result_cursor() == 1 && !f2.on_key(Key::ArrowDown, 0), "");
    // 4) Esc 逐层退 + 根页无动作。
    let mut f3 = KbdFlow::new();
    cs.add("esc_root_noop", !f3.on_key(Key::Esc, 0), "");
    f3.on_key(Key::CtrlE, 0);
    f3.on_key(Key::Enter, 2);
    f3.on_key(Key::CtrlE, 0); // 详情页再搜索 → 再进一层
    f3.on_key(Key::Enter, 2);
    let deep = f3.level();
    f3.on_key(Key::Esc, 0);
    f3.on_key(Key::Esc, 0);
    cs.add("esc_peels_layers", deep == 3 && f3.level() == 1 && f3.focus_back_to_trigger(), "");
    // 5) 面包屑键盘：搜索态 Backspace 不劫持（输入优先）；页面态才退层。
    let mut f4 = KbdFlow::new();
    f4.on_key(Key::CtrlE, 0);
    cs.add("backspace_in_search_noop", !f4.on_key(Key::Backspace, 0), "");
    let mut f5 = KbdFlow::new();
    f5.on_key(Key::Enter, 1); // 未聚焦搜索 Enter 无效（层级不变）
    cs.add("enter_without_search_noop", f5.level() == 1, "");
    f5.on_key(Key::CtrlE, 0);
    f5.on_key(Key::Enter, 2);
    cs.add("backspace_breadcrumb", f5.on_key(Key::Backspace, 0) && f5.level() == 1, "");
    // 6) 典型 10 设置任务计时记录（总账预算）。
    cs.add("ten_task_budget", ten_task_budget(49_999) && !ten_task_budget(50_000), "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_flow_never_loses_focus() {
        let mut f = KbdFlow::new();
        f.on_key(Key::CtrlE, 0);
        f.on_key(Key::Char('a'), 0);
        f.on_key(Key::Enter, 4);
        assert_eq!(f.level(), 2);
        // 连退两层到根（Esc 层层剥，焦点链不断）。
        f.on_key(Key::Esc, 0);
        f.on_key(Key::Esc, 0);
        assert!(f.focus_back_to_trigger());
    }

    #[test]
    fn tab_requires_results() {
        let mut f = KbdFlow::new();
        assert!(!f.on_key(Key::Tab, 0));
        f.on_key(Key::CtrlE, 0);
        f.on_key(Key::Enter, 1);
        assert!(f.on_key(Key::Tab, 0));
    }

    #[test]
    fn ten_tasks_within_budget() {
        // 每任务 4.8s（键路径纯键盘）× 10 = 48s 总账绿。
        assert!(ten_task_budget(48_000));
    }
}
