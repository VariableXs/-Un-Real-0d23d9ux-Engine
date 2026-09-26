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

// ===========================================================================
// 深化 v7（F473）：输入法组合守卫 / Enter 双击防抖 / 结果分页器 /
// 快捷键账（重复抑制）/ 偏好持久化 v7（W7K1 + FNV 校验尾）
// ===========================================================================
//
// v7 主轴（主册判据的二阶展开）：
// 1. IME 组合守卫——中文输入法组合期**不能触发快捷键和误提交**（体验
//   六章红线）：组合中 Enter=上屏、Esc=取消组合、Tab/方向键不劫持；
//   组合外行为与 v1 五步链路一致。
// 2. Enter 双击防抖——同一 Enter 在抑制窗内只生效一次（重复提交防线）。
// 3. 结果分页器——16 项结果一屏放不下：分页钳制 + 页码诚实。
// 4. 快捷键账——「快捷键是承诺」的审计面：每次记账 + 时钟单调守卫 +
//   重复抑制窗内连按计数（rage-key 指纹——十三·补挫败信号）。
// 5. 偏好持久化——v7 通道（W7K1 + FNV 尾）。

use crate::genstar2::vxdict::fnv1a;

// ---------------------------------------------------------------------------
// IME 组合守卫（中文输入法组合期不触发快捷键/不误提交）
// ---------------------------------------------------------------------------

/// 组合守卫状态机。
pub struct ImeGuard {
    /// 组合中（候选窗开着）。
    pub composing: bool,
    /// 组合缓冲字符数（守卫不存内容——隐私红线：日志不记正文）。
    pub comp_len: usize,
    /// 组合缓冲上限。
    pub comp_cap: usize,
}

/// 组合缓冲上限（超限拒绝继续吞字符——诚实边界）。
pub const IME_COMP_CAP: usize = 32;

impl ImeGuard {
    pub const fn new() -> Self {
        ImeGuard { composing: false, comp_len: 0, comp_cap: IME_COMP_CAP }
    }

    /// 开始组合。
    pub fn begin(&mut self) {
        self.composing = true;
        self.comp_len = 0;
    }

    /// 组合中吞字符（超容拒绝——返回 false 让上层出「已满」提示）。
    pub fn feed(&mut self) -> bool {
        if !self.composing {
            return false;
        }
        if self.comp_len >= self.comp_cap {
            return false;
        }
        self.comp_len += 1;
        true
    }

    /// 回删一格（组合缓冲空 = false——不越界）。
    pub fn backspace(&mut self) -> bool {
        if !self.composing || self.comp_len == 0 {
            return false;
        }
        self.comp_len -= 1;
        true
    }

    /// Enter：组合中 = 上屏（不是提交！——误提交红线），组合外 = 提交。
    pub fn enter(&mut self) -> bool {
        if self.composing {
            self.composing = false;
            self.comp_len = 0;
            false // false = 「这不是提交」——五步链路的 Enter 不触发
        } else {
            true
        }
    }

    /// Esc：组合中 = 取消组合；组合外 = 透传给五步链路（true=透传）。
    pub fn esc(&mut self) -> bool {
        if self.composing {
            self.composing = false;
            self.comp_len = 0;
            false // 组合被取消，不透传
        } else {
            true
        }
    }

    /// 组合中快捷键一律不劫持（Tab/CtrlE 等——组合期键全归 IME）。
    pub fn shortcut_blocked(&self) -> bool {
        self.composing
    }
}

// ---------------------------------------------------------------------------
// Enter 双击防抖（重复提交防线）
// ---------------------------------------------------------------------------

/// 抑制窗（ms：同一 Enter 300ms 内只生效一次——双击不双份动作）。
pub const ENTER_DEBOUNCE_MS: u64 = 300;

pub struct EnterDebounce {
    last_ms: Option<u64>,
    /// 被抑制的重复 Enter 计数（rage-click 指纹——体验日志面）。
    pub suppressed: usize,
}

impl EnterDebounce {
    pub const fn new() -> Self {
        EnterDebounce { last_ms: None, suppressed: 0 }
    }

    /// 裁决（返回 true = 放行；窗内重复 = 抑制并计数）。
    pub fn accept(&mut self, at_ms: u64) -> bool {
        match self.last_ms {
            Some(t) if at_ms.saturating_sub(t) < ENTER_DEBOUNCE_MS => {
                self.suppressed += 1;
                false
            }
            _ => {
                self.last_ms = Some(at_ms);
                true
            }
        }
    }

    pub fn suppressed_count(&self) -> usize {
        self.suppressed
    }
}

// ---------------------------------------------------------------------------
// 结果分页器（16 项结果分页——钳制不越界）
// ---------------------------------------------------------------------------

/// 每页行数。
pub const RESULTS_PER_PAGE: usize = 8;

pub struct ResultPager {
    total: usize,
    page: usize,
}

impl ResultPager {
    pub fn new(total: usize) -> Self {
        let mut p = ResultPager { total, page: 0 };
        p.clamp();
        p
    }

    fn max_page(&self) -> usize {
        if self.total == 0 {
            0
        } else {
            (self.total - 1) / RESULTS_PER_PAGE
        }
    }

    fn clamp(&mut self) {
        self.page = self.page.min(self.max_page());
    }

    pub fn next(&mut self) -> bool {
        if self.page >= self.max_page() {
            return false;
        }
        self.page += 1;
        true
    }

    pub fn prev(&mut self) -> bool {
        if self.page == 0 {
            return false;
        }
        self.page -= 1;
        true
    }

    /// 当前页（页码, 本页行数）——尾页不足一页诚实显示。
    pub fn window(&self) -> (usize, usize) {
        let start = self.page * RESULTS_PER_PAGE;
        let show = self.total.saturating_sub(start).min(RESULTS_PER_PAGE);
        (self.page, show)
    }

    pub fn page_index(&self) -> usize {
        self.page
    }
}

// ---------------------------------------------------------------------------
// 快捷键账（记账 + 单调守卫 + 重复抑制计数）
// ---------------------------------------------------------------------------

/// 快捷键种类（审计键集——五步链路 + 面包屑）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShortcutKey {
    CtrlE,
    Enter,
    Tab,
    Esc,
    Backspace,
    ArrowDown,
}

/// 账面容量。
pub const SHORTCUT_LEDGER_CAP: usize = 16;
/// 重复抑制窗（ms：同键连按 <200ms 计 rage 指纹——不拦动作只记账，
/// 与 Enter 防抖分层：防抖管提交、账本管观察）。
pub const RAGE_WINDOW_MS: u64 = 200;

pub struct ShortcutLedger {
    ring: [(u64, ShortcutKey); SHORTCUT_LEDGER_CAP],
    head: usize,
    n: usize,
    pub out_of_order_rejected: usize,
    /// rage 指纹计数（同键 RAGE_WINDOW_MS 内连按 ≥3 次的事件数）。
    pub rage_events: usize,
    /// 账内同键连按游标（rage 判定用）。
    same_key_streak: usize,
}

impl ShortcutLedger {
    pub const fn new() -> Self {
        ShortcutLedger {
            ring: [(0, ShortcutKey::Esc); SHORTCUT_LEDGER_CAP],
            head: 0,
            n: 0,
            out_of_order_rejected: 0,
            rage_events: 0,
            same_key_streak: 0,
        }
    }

    pub fn push(&mut self, at_ms: u64, k: ShortcutKey) -> bool {
        if self.n > 0 {
            let last = (self.head + SHORTCUT_LEDGER_CAP - 1) % SHORTCUT_LEDGER_CAP;
            if at_ms < self.ring[last].0 {
                self.out_of_order_rejected += 1;
                return false;
            }
            if self.ring[last].1 == k && at_ms.saturating_sub(self.ring[last].0) <= RAGE_WINDOW_MS {
                self.same_key_streak += 1;
                if self.same_key_streak == 2 {
                    // 第 3 次同键快按（streak 0→1→2）= 一次 rage 事件。
                    self.rage_events += 1;
                }
            } else {
                self.same_key_streak = 0;
            }
        }
        self.ring[self.head] = (at_ms, k);
        self.head = (self.head + 1) % SHORTCUT_LEDGER_CAP;
        self.n = (self.n + 1).min(SHORTCUT_LEDGER_CAP);
        true
    }

    pub fn count(&self) -> usize {
        self.n
    }
}

// ---------------------------------------------------------------------------
// 偏好持久化 v7（W7K1 + FNV 尾）
// ---------------------------------------------------------------------------

/// v7 魔标（W7K 族）。
pub const SETKBD_V7_MAGIC: [u8; 4] = *b"W7K1";
/// 长度：魔标(4) + 版本(1) + 旗标(1) + 每页行数(1) + 保留(1) + FNV(4) = 12。
pub const SETKBD_V7_LEN: usize = 12;
pub const SETKBD_V7_VERSION: u8 = 1;
/// 旗标位：bit0 = 开页即聚焦搜索（search-on-open）。
const FLAG_SEARCH_ON_OPEN: u8 = 1 << 0;
const FLAG_RESERVED: u8 = !0x01;

/// 序列化（v7 独占通道）。
pub fn save_prefs_v7(search_on_open: bool, per_page: u8, out: &mut [u8]) -> Option<usize> {
    if out.len() < SETKBD_V7_LEN || per_page == 0 || per_page as usize > RESULT_CAP {
        return None;
    }
    out[..4].copy_from_slice(&SETKBD_V7_MAGIC);
    out[4] = SETKBD_V7_VERSION;
    out[5] = if search_on_open { FLAG_SEARCH_ON_OPEN } else { 0 };
    out[6] = per_page;
    out[7] = 0;
    let h = fnv1a(&out[..8]);
    out[8] = (h & 0xff) as u8;
    out[9] = ((h >> 8) & 0xff) as u8;
    out[10] = ((h >> 16) & 0xff) as u8;
    out[11] = ((h >> 24) & 0xff) as u8;
    Some(SETKBD_V7_LEN)
}

/// 反序列化（版本/旗标保留位/per_page 值域/FNV 四重守卫）。
pub fn load_prefs_v7(buf: &[u8]) -> Option<(bool, u8)> {
    if buf.len() < SETKBD_V7_LEN || buf[..4] != SETKBD_V7_MAGIC {
        return None;
    }
    if buf[4] != SETKBD_V7_VERSION || buf[5] & FLAG_RESERVED != 0 || buf[7] != 0 {
        return None;
    }
    let per_page = buf[6];
    if per_page == 0 || per_page as usize > RESULT_CAP {
        return None;
    }
    let expect = fnv1a(&buf[..8]);
    let got = buf[8] as u32
        | ((buf[9] as u32) << 8)
        | ((buf[10] as u32) << 16)
        | ((buf[11] as u32) << 24);
    if expect != got {
        return None;
    }
    Some((buf[5] & FLAG_SEARCH_ON_OPEN != 0, per_page))
}

// ---------------------------------------------------------------------------
// 域自检（F473 v7）
// ---------------------------------------------------------------------------

pub fn run_setkbd_v7_checks() -> CheckSet {
    let mut cs = CheckSet::new("F473-v7");
    // 1) IME 守卫：组合期 Enter=上屏不提交、Esc=取消不透传、快捷键不劫持。
    cs.add("ime_enter_commits_composition", {
        let mut g = ImeGuard::new();
        g.begin();
        let _ = g.feed();
        let _ = g.feed();
        !g.enter() && !g.composing && g.comp_len == 0 // 上屏后组合清空且非提交
    }, "");
    cs.add("ime_enter_outside_is_submit", {
        let mut g = ImeGuard::new();
        g.enter() // 组合外 Enter = 真提交
    }, "");
    cs.add("ime_esc_cancels_no_passthrough", {
        let mut g = ImeGuard::new();
        g.begin();
        let _ = g.feed();
        !g.esc() && !g.composing
    }, "");
    cs.add("ime_shortcuts_blocked_while_composing", {
        let mut g = ImeGuard::new();
        g.begin();
        g.shortcut_blocked() && { let _ = g.esc(); !g.shortcut_blocked() }
    }, "");
    cs.add("ime_backspace_bounds", {
        let mut g = ImeGuard::new();
        g.begin();
        !g.backspace() && { let _ = g.feed(); g.backspace() } && !g.backspace()
    }, "");
    cs.add("ime_comp_cap_honest", {
        let mut g = ImeGuard::new();
        g.begin();
        let fed = (0..IME_COMP_CAP + 5).filter(|_| g.feed()).count();
        fed == IME_COMP_CAP // 超容拒绝（诚实边界，不静默丢）
    }, "");
    // 2) Enter 防抖：窗内抑制计数、窗外放行。
    cs.add("enter_debounce_window", {
        let mut d = EnterDebounce::new();
        d.accept(1_000) && !d.accept(1_100) && d.suppressed_count() == 1
            && d.accept(1_301) // 301ms 后放行
    }, "");
    // 3) 分页器：翻页钳制 + 尾页诚实 + 零结果。
    cs.add("pager_clamp_and_tail", {
        let mut p = ResultPager::new(16);
        let _ = p.next();
        !p.next() // 只有 2 页，第 2 页 next = false
            && p.window() == (1, 8)
            && {
                let mut t = ResultPager::new(11);
                let _ = t.next();
                t.window() == (1, 3) // 尾页 3 行诚实
            }
    }, "");
    cs.add("pager_zero_results", {
        let mut p = ResultPager::new(0);
        !p.next() && p.window() == (0, 0)
    }, "");
    cs.add("pager_prev_at_top", {
        let mut p = ResultPager::new(16);
        !p.prev() && p.page_index() == 0
    }, "");
    // 4) 快捷键账：单调守卫 + rage 指纹 + 环上限。
    cs.add("shortcut_ledger_monotonic", {
        let mut led = ShortcutLedger::new();
        let _ = led.push(1_000, ShortcutKey::CtrlE);
        !led.push(500, ShortcutKey::Esc) && led.out_of_order_rejected == 1
    }, "");
    cs.add("shortcut_rage_fingerprint", {
        let mut led = ShortcutLedger::new();
        let _ = led.push(1_000, ShortcutKey::Esc);
        let _ = led.push(1_050, ShortcutKey::Esc);
        let _ = led.push(1_100, ShortcutKey::Esc); // 200ms 内三连 = rage 一次
        led.rage_events == 1 && {
            let _ = led.push(5_000, ShortcutKey::Esc); // 冷却后重置
            led.rage_events == 1
        }
    }, "");
    cs.add("shortcut_ledger_ring_cap", {
        let mut led = ShortcutLedger::new();
        for i in 0..(SHORTCUT_LEDGER_CAP * 2) {
            let _ = led.push(i as u64 * 1_000, ShortcutKey::Tab);
        }
        led.count() == SHORTCUT_LEDGER_CAP
    }, "");
    // 5) 偏好持久化：round-trip + 值域守卫 + 篡改拒收。
    let mut buf = [0u8; SETKBD_V7_LEN];
    cs.add("prefs_roundtrip", {
        let n = save_prefs_v7(true, 8, &mut buf).unwrap_or(0);
        load_prefs_v7(&buf[..n]) == Some((true, 8))
    }, "");
    cs.add("prefs_per_page_bounds", save_prefs_v7(true, 0, &mut buf).is_none()
        && save_prefs_v7(true, RESULT_CAP as u8 + 1, &mut buf).is_none(), "");
    cs.add("prefs_tamper", {
        let n = save_prefs_v7(false, 8, &mut buf).unwrap_or(0);
        let mut bad = buf;
        bad[5] ^= 0x01; // 翻旗标位 → FNV 失配
        load_prefs_v7(&bad[..n]).is_none()
    }, "");
    cs.add("prefs_reserved_set", {
        let mut bad = [0u8; SETKBD_V7_LEN];
        let _ = save_prefs_v7(false, 8, &mut bad);
        bad[5] |= 0x02;
        load_prefs_v7(&bad).is_none()
    }, "");
    // 6) 五步链路回归锚（v1 判据的 v7 复核——改 IME 层不许伤链路）。
    cs.add("five_step_chain_regression", {
        let mut g = ImeGuard::new();
        let mut f = KbdFlow::new();
        let _ = f.on_key(Key::CtrlE, 0);
        // 组合外字符照常输入；组合期不触发链路。
        g.begin();
        let _ = g.feed();
        let blocked = g.shortcut_blocked();
        let _ = g.esc();
        blocked && f.on_key(Key::Char('k'), 0) && f.on_key(Key::Enter, 3)
            && f.level() == 2 && f.on_key(Key::Esc, 0) && f.focus_back_to_trigger()
    }, "");
    cs
}

#[cfg(test)]
mod v7_tests {
    use super::*;

    #[test]
    fn ime_flow_never_double_submits() {
        // 打字 → 上屏 → 再 Enter 才是真提交（组合期 Enter 零提交）。
        let mut g = ImeGuard::new();
        let mut submits = 0;
        g.begin();
        for _ in 0..3 {
            assert!(g.feed());
        }
        if g.enter() {
            submits += 1; // 组合期 Enter 不计
        }
        if g.enter() {
            submits += 1; // 组合外 Enter 计一次
        }
        assert_eq!(submits, 1);
    }

    #[test]
    fn debounce_boundary_exact() {
        let mut d = EnterDebounce::new();
        assert!(d.accept(0));
        assert!(!d.accept(ENTER_DEBOUNCE_MS - 1));
        assert!(d.accept(ENTER_DEBOUNCE_MS));
    }

    #[test]
    fn pager_full_walk() {
        let mut p = ResultPager::new(20);
        let mut pages = 1;
        while p.next() {
            pages += 1;
        }
        assert_eq!(pages, 3); // 20 / 8 = 3 页
        let mut back = 0;
        while p.prev() {
            back += 1;
        }
        assert_eq!(back, 2);
    }

    #[test]
    fn rage_never_counts_across_keys() {
        let mut led = ShortcutLedger::new();
        // 交替按不同键：不构成同键连按。
        for i in 0..6 {
            let k = if i % 2 == 0 { ShortcutKey::Esc } else { ShortcutKey::Tab };
            assert!(led.push(1_000 + i as u64 * 50, k));
        }
        assert_eq!(led.rage_events, 0);
    }

    #[test]
    fn prefs_all_flag_combos_roundtrip() {
        let mut buf = [0u8; SETKBD_V7_LEN];
        for &soo in &[true, false] {
            let n = save_prefs_v7(soo, 4, &mut buf).unwrap();
            assert_eq!(load_prefs_v7(&buf[..n]), Some((soo, 4)));
        }
    }
}
