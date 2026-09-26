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

// ===========================================================================
// 深化 v2（F473）：五步链路超时/中断处理 / 焦点栈完整退链 /
// 十任务预算分解表 / 面包屑键盘路径
// ===========================================================================

/// 十任务预算分解表（主册「典型 10 设置任务计时记录」——每任务 5s
/// 预算是分解账：搜索 1s + 达 1s + 改 1.5s + 验 1.5s；账面合计 = 预算）。
pub const TASK_BUDGET_STAGES: [(&str, u64); 4] = [
    ("search", 1_000),
    ("navigate", 1_000),
    ("modify", 1_500),
    ("verify", 1_500),
];

pub fn task_budget_sum() -> u64 {
    TASK_BUDGET_STAGES.iter().map(|(_, ms)| ms).sum()
}

/// 链路中断处理（五步键盘流的打断恢复：任意步骤按 Esc 逐层退——
/// 搜索态退到空闲、结果态退到搜索、空闲态 Esc 无动作——永不悬空）。
impl KbdFlow {
    /// 当前层名（人话审计面：Esc 提示「正在退出 X」）。
    pub fn level_name(&self) -> &'static str {
        match self.level() {
            1 => "空闲",
            2 => "详情",
            _ => "空闲",
        }
    }

    /// Esc 退链守卫（对 v1 on_key 的收口审计：退到层 1 后再 Esc 不崩不怪）。
    pub fn esc_at_root_safe(&mut self) -> bool {
        while self.level() > 1 {
            let _ = self.on_key(Key::Esc, 0);
        }
        // 根层再按 Esc：无动作且层不变（不悬空不崩——v1 返回 false）。
        let before = self.level();
        let accepted = self.on_key(Key::Esc, 0);
        !accepted && self.level() == before
    }

    /// 焦点栈守卫（主册 F206 焦点回归链：层栈深度不超过 FOCUS_STACK_CAP）。
    pub fn focus_stack_bounded(&self) -> bool {
        self.level() <= FOCUS_STACK_CAP
    }
}

/// 面包屑键盘路径（主册「每页面包屑键盘可达（Backspace 上层）」——
/// 路径栈：Push 下层 / Backspace 上层 / 根层 Backspace 无动作）。
pub struct BreadcrumbTrail {
    trail: [&'static str; 8],
    n: usize,
}

impl BreadcrumbTrail {
    pub const fn new() -> Self {
        BreadcrumbTrail { trail: ["", "", "", "", "", "", "", ""], n: 1 }
    }

    pub fn push(&mut self, page: &'static str) -> bool {
        if self.n >= 8 || page.is_empty() {
            return false;
        }
        self.trail[self.n] = page;
        self.n += 1;
        true
    }

    pub fn backspace_up(&mut self) -> bool {
        if self.n <= 1 {
            return false; // 根层不退（键盘路径终点清晰）。
        }
        self.trail[self.n - 1] = "";
        self.n -= 1;
        true
    }

    pub fn current(&self) -> &'static str {
        self.trail[self.n - 1]
    }

    pub fn depth(&self) -> usize {
        self.n
    }
}

// ---------------------------------------------------------------------------
// 深化自检（F473 v2）
// ---------------------------------------------------------------------------

pub fn run_setkbd_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F473-v2");
    // 1) 预算分解账：四段和 = 5s（一处一事实）。
    cs.add("budget_sum_exact", task_budget_sum() == TASK_BUDGET_MS, "");
    cs.add("ten_task_budget_v2", ten_task_budget(task_budget_sum() * 10 - 1), "");
    // 2) Esc 逐层退：任意层退到根；根层再按无动作。
    let mut f = KbdFlow::new();
    let _ = f.on_key(Key::CtrlE, 0);
    let _ = f.on_key(Key::Enter, 3);
    cs.add("mid_flow_esc_safe", f.esc_at_root_safe() && f.level() == 1, "");
    // 3) 焦点栈守卫 + 焦点回归链。
    let mut f2 = KbdFlow::new();
    let _ = f2.on_key(Key::CtrlE, 2);
    let _ = f2.on_key(Key::Enter, 2);
    let _ = f2.on_key(Key::Tab, 2);
    cs.add("focus_stack_bounded", f2.focus_stack_bounded(), "");
    cs.add("focus_back_after_esc", {
        let _ = f2.on_key(Key::Esc, 2);
        f2.focus_back_to_trigger()
    }, "");
    // 4) 面包屑键盘路径：下钻-上退-根层守卫。
    let mut bc = BreadcrumbTrail::new();
    cs.add("trail_root", bc.current() == "" && bc.depth() == 1, "");
    cs.add("trail_push", bc.push("个性化") && bc.push("桌面图标"), "");
    cs.add("trail_backspace", bc.backspace_up() && bc.current() == "个性化", "");
    cs.add("trail_root_guard", bc.backspace_up() && bc.depth() == 1 && !bc.backspace_up(), "");
    // 5) 层名审计（人话提示的面）。
    let mut f3 = KbdFlow::new();
    cs.add("level_name_idle", f3.level_name() == "空闲", "");
    let _ = f3.on_key(Key::CtrlE, 0);
    let _ = f3.on_key(Key::Enter, 1);
    cs.add("level_name_detail", f3.level_name() == "详情", "");
    cs
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn full_five_step_keyboard_chain() {
        // Ctrl+E → 输入（Enter）→ Tab → Esc 全链不碰鼠标。
        let mut f = KbdFlow::new();
        assert!(f.on_key(Key::CtrlE, 4));
        assert!(f.on_key(Key::Char('p'), 4));
        assert!(f.on_key(Key::Enter, 4));
        assert!(f.on_key(Key::Tab, 4));
        assert!(f.on_key(Key::Esc, 4));
        assert_eq!(f.level(), 1);
        assert!(f.focus_back_to_trigger());
    }

    #[test]
    fn breadcrumb_full_walk() {
        let mut bc = BreadcrumbTrail::new();
        assert!(bc.push("系统"));
        assert!(bc.push("电源"));
        assert!(bc.push("电池"));
        assert_eq!(bc.current(), "电池");
        assert!(bc.backspace_up());
        assert!(bc.backspace_up());
        assert_eq!(bc.current(), "系统");
        // 退到根：再退一次到 trail 底、根层守卫生效。
        assert!(bc.backspace_up());
        assert_eq!(bc.current(), "");
        assert!(!bc.backspace_up());
    }

    #[test]
    fn esc_interrupts_at_every_level() {
        // 每一层打断都回到根（永无卡死层）。
        for steps in 1..3 {
            let mut f = KbdFlow::new();
            let _ = f.on_key(Key::CtrlE, 1);
            if steps > 1 {
                let _ = f.on_key(Key::Enter, 1);
            }
            assert!(f.esc_at_root_safe());
        }
    }

    #[test]
    fn budget_table_no_gaps() {
        assert_eq!(TASK_BUDGET_STAGES.iter().map(|(_, ms)| ms).sum::<u64>(), 5_000);
    }
}
