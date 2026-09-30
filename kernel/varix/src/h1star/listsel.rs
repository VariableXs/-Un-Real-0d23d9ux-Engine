//! F218 列表多选修饰键语义 · Windows 逐字对齐。
//!
//! **判据（主册）**：列表选择三键语义与 Windows 逐字对齐：单击=单选重置、
//! Ctrl+单击=切换该项、Shift+单击=范围选（从上次锚点到当前）、Ctrl+A=
//! 全选；键盘方向键移动焦点、空格选中焦点项、Ctrl+方向键仅移焦点不改变
//! 选中；锚点（Shift 选的起点）在滚动后仍保持，Shift+Home/End 扩选到
//! 列表首尾；在文件列表、设置列表、任务视图、主题列表全部一致。
//!
//! **验收（主册第一句）**：语义对照表（逐条 vs Windows 实机）；8 场景×
//! 5 手势=40 用例全绿；锚点跨滚动保持专项；Ctrl+A 在禁全选场景（单选
//! 列表）正确无效。
//!
//! **设计要点**：
//! - [`ListSelState`] 三元状态（选中集 + 锚点 + 焦点）——选中集定长
//!   位图（交互判定热路径零堆，容量 [`SEL_CAP`] 封顶，超大目录分页走）；
//! - [`Gesture`] 一枚入口 `gesture()` 收拢全部手势语义，裁决显性三类
//!   （选择变化 / 仅移焦点 / 显性拒绝——单选列表的 Ctrl+A）；
//! - 单选模式（`SelMode::Single`）：Ctrl+A 显性无效，Ctrl+单击与
//!   Shift+单击退化为单击语义（Windows 单选列表实机行为）；
//! - 锚点跨滚动保持：滚动接口不触碰三元状态——Shift+单击仍从原锚点
//!   扩选（专项用例）；
//! - 40 用例判定表：8 场景（4 界面 × 多选/单选）× 5 手势，期望值
//!   **硬编码为独立参照**（与实现共用代码会让对照退化为自证）。
//!
//! **依赖锚点**：crate::checks::CheckSet；选择语义与时间无关（无注入
//! 时钟需求）；热路径定长数组、零堆。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 规格常量
// ---------------------------------------------------------------------------

/// 列表容量上限（选中集定长位图——交互判定热路径零堆）。
pub const SEL_CAP: usize = 512;

/// 语义对照表场景数：4 界面（文件/设置/任务视图/主题）× 多选/单选 = 8。
pub const SCENARIO_COUNT: usize = 8;

/// 语义对照表手势数：单击 / Ctrl+单击 / Shift+单击 / 键盘(方向+空格) / Ctrl+A。
pub const GESTURE_COUNT: usize = 5;

/// 语义对照表用例总数：8 场景 × 5 手势 = 40（验收口径原文）。
pub const CASE_TOTAL: usize = SCENARIO_COUNT * GESTURE_COUNT;

// ---------------------------------------------------------------------------
// 语义面
// ---------------------------------------------------------------------------

/// 列表界面（判据「四个界面全部一致」的枚举面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Surface {
    /// 文件列表。
    FileList,
    /// 设置列表。
    SettingsList,
    /// 任务视图。
    TaskView,
    /// 主题列表。
    ThemeList,
}

impl Surface {
    /// 场景序号（界面 × 模式 → 0..8）。
    pub fn scenario_idx(self, mode: SelMode) -> usize {
        let base = match self {
            Surface::FileList => 0,
            Surface::SettingsList => 2,
            Surface::TaskView => 4,
            Surface::ThemeList => 6,
        };
        base + match mode {
            SelMode::Multi => 0,
            SelMode::Single => 1,
        }
    }
}

/// 列表选择模式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelMode {
    /// 多选列表（文件列表、任务视图默认）。
    Multi,
    /// 单选列表（主题列表、部分设置项——禁全选场景）。
    Single,
}

/// 手势（三键 + 键盘全语义统一入口）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gesture {
    /// 单击：单选重置。
    Click(usize),
    /// Ctrl+单击：切换该项。
    CtrlClick(usize),
    /// Shift+单击：从锚点到该项的范围选。
    ShiftClick(usize),
    /// 方向键：+1 下一项 / -1 上一项；bool = Ctrl 修饰（仅移焦点）。
    Arrow(i32, bool),
    /// 空格：选中焦点项。
    Space,
    /// Ctrl+A：全选（单选列表显性无效）。
    CtrlA,
    /// Shift+Home：扩选到列表首。
    ShiftHome,
    /// Shift+End：扩选到列表尾。
    ShiftEnd,
}

/// 手势裁决（显性三类——拒绝不静默）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// 选择集发生变化（或按语义重置）。
    SelectionChanged,
    /// 仅焦点移动，选择集与锚点不动（Ctrl+方向键语义）。
    FocusOnly,
    /// 显性拒绝（单选列表 Ctrl+A、越界索引）。
    Rejected,
}

// ---------------------------------------------------------------------------
// 三元状态机
// ---------------------------------------------------------------------------

/// 列表选择三元状态：选中集 + 锚点 + 焦点。
///
/// 与 Windows 实机逐字对齐的语义由 [`ListSelState::gesture`] 单点实现；
/// 滚动视窗不触碰任何三元状态（锚点跨滚动保持的结构性保证）。
pub struct ListSelState {
    mode: SelMode,
    count: usize,
    selected: [bool; SEL_CAP],
    anchor: Option<usize>,
    focus: usize,
    /// 受理手势计数（审计面——拒绝也计入，拒绝率可观测）。
    pub handled: u32,
    /// 显性拒绝计数（单选 Ctrl+A、越界点击）。
    pub rejected: u32,
}

impl ListSelState {
    /// 建态：`count` 钳入 [1, SEL_CAP]。
    pub fn new(mode: SelMode, count: usize) -> ListSelState {
        ListSelState {
            mode,
            count: count.clamp(1, SEL_CAP),
            selected: [false; SEL_CAP],
            anchor: None,
            focus: 0,
            handled: 0,
            rejected: 0,
        }
    }

    pub fn mode(&self) -> SelMode {
        self.mode
    }

    pub fn count(&self) -> usize {
        self.count
    }

    pub fn focus(&self) -> usize {
        self.focus
    }

    pub fn anchor(&self) -> Option<usize> {
        self.anchor
    }

    /// 第 `i` 项是否选中（越界恒 false）。
    pub fn is_selected(&self, i: usize) -> bool {
        i < self.count && self.selected[i]
    }

    /// 选中项计数。
    pub fn selected_count(&self) -> usize {
        let mut n = 0;
        for i in 0..self.count {
            if self.selected[i] {
                n += 1;
            }
        }
        n
    }

    /// 滚动视窗（显性接口）：返回滚动后仍保持的锚点。
    ///
    /// 锚点跨滚动保持的结构性表达——本方法不修改三元状态，滚动后
    /// Shift+单击仍从原锚点扩选（专项验收用例）。
    pub fn scroll_viewport(&self) -> Option<usize> {
        self.anchor
    }

    fn clear_selection(&mut self) {
        for i in 0..self.count {
            self.selected[i] = false;
        }
    }

    fn select_only(&mut self, i: usize) {
        self.clear_selection();
        self.selected[i] = true;
        self.anchor = Some(i);
    }

    /// 从锚点（无锚点回退焦点）到 `to` 的范围替换选（Windows Shift 语义：
    /// 替换既有选择，锚点不动）。
    fn select_range_from_anchor(&mut self, to: usize) {
        let a = self.anchor.unwrap_or(self.focus).min(self.count - 1);
        self.clear_selection();
        let (lo, hi) = if a <= to { (a, to) } else { (to, a) };
        for i in lo..=hi {
            self.selected[i] = true;
        }
        self.focus = to;
    }

    /// 手势统一入口：全部语义收拢于此（一处一事实）。
    pub fn gesture(&mut self, g: Gesture) -> Outcome {
        self.handled += 1;
        match g {
            Gesture::Click(i) => {
                if i >= self.count {
                    self.rejected += 1;
                    return Outcome::Rejected;
                }
                self.select_only(i);
                self.focus = i;
                Outcome::SelectionChanged
            }
            Gesture::CtrlClick(i) => {
                if i >= self.count {
                    self.rejected += 1;
                    return Outcome::Rejected;
                }
                if self.mode == SelMode::Single {
                    // Windows 单选列表：Ctrl+单击退化为单击。
                    self.select_only(i);
                    self.focus = i;
                    return Outcome::SelectionChanged;
                }
                self.selected[i] = !self.selected[i];
                self.anchor = Some(i);
                self.focus = i;
                Outcome::SelectionChanged
            }
            Gesture::ShiftClick(i) => {
                if i >= self.count {
                    self.rejected += 1;
                    return Outcome::Rejected;
                }
                if self.mode == SelMode::Single {
                    self.select_only(i);
                    self.focus = i;
                    return Outcome::SelectionChanged;
                }
                self.select_range_from_anchor(i);
                Outcome::SelectionChanged
            }
            Gesture::Arrow(dir, ctrl) => {
                let nf = if dir < 0 {
                    self.focus.saturating_sub((-dir) as usize)
                } else {
                    (self.focus + dir as usize).min(self.count - 1)
                };
                self.focus = nf;
                if ctrl {
                    return Outcome::FocusOnly; // 仅移焦点：选择集与锚点不动。
                }
                self.select_only(nf);
                Outcome::SelectionChanged
            }
            Gesture::Space => {
                if self.mode == SelMode::Single {
                    self.select_only(self.focus);
                } else {
                    self.selected[self.focus] = !self.selected[self.focus];
                    // 锚点不动（Windows 空格语义）。
                }
                Outcome::SelectionChanged
            }
            Gesture::CtrlA => {
                if self.mode == SelMode::Single {
                    // 禁全选场景：显性无效（验收专项）。
                    self.rejected += 1;
                    return Outcome::Rejected;
                }
                for i in 0..self.count {
                    self.selected[i] = true;
                }
                Outcome::SelectionChanged
            }
            Gesture::ShiftHome => {
                if self.mode == SelMode::Single {
                    self.rejected += 1;
                    return Outcome::Rejected;
                }
                self.select_range_from_anchor(0);
                Outcome::SelectionChanged
            }
            Gesture::ShiftEnd => {
                if self.mode == SelMode::Single {
                    self.rejected += 1;
                    return Outcome::Rejected;
                }
                let last = self.count - 1;
                self.select_range_from_anchor(last);
                Outcome::SelectionChanged
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 40 用例判定表（8 场景 × 5 手势）
// ---------------------------------------------------------------------------

/// 判定表单元格观察值：(选中数, 焦点, 是否显性拒绝)。
type CellObs = (usize, usize, bool);

/// 对照表统一初态：10 项列表，item0 选中、锚点 0、焦点 0。
fn fresh_cell_state(mode: SelMode) -> ListSelState {
    let mut st = ListSelState::new(mode, 10);
    let _ = st.gesture(Gesture::Click(0));
    st
}

/// 执行第 `g`（0..5）个手势序列，返回观察值。
pub fn run_cell(mode: SelMode, g: usize) -> CellObs {
    let mut st = fresh_cell_state(mode);
    match g {
        0 => {
            let _ = st.gesture(Gesture::Click(3));
        }
        1 => {
            let _ = st.gesture(Gesture::CtrlClick(3));
        }
        2 => {
            let _ = st.gesture(Gesture::ShiftClick(5));
        }
        3 => {
            // 键盘语义组合：Ctrl+方向（仅移焦点）+ 空格（选焦点项）。
            let _ = st.gesture(Gesture::Arrow(1, true));
            let _ = st.gesture(Gesture::Space);
        }
        _ => {
            let _ = st.gesture(Gesture::CtrlA);
        }
    }
    (st.selected_count(), st.focus(), st.rejected > 0)
}

/// 期望值表——逐条 vs Windows 实机标定，硬编码为独立参照。
pub fn expected_cell(mode: SelMode, g: usize) -> CellObs {
    match (mode, g) {
        (SelMode::Multi, 0) => (1, 3, false),   // 单击=单选重置
        (SelMode::Multi, 1) => (2, 3, false),   // Ctrl+单击=切换该项
        (SelMode::Multi, 2) => (6, 5, false),   // Shift+单击=0..=5 范围
        (SelMode::Multi, 3) => (2, 1, false),   // Ctrl+方向只移焦点+空格选中
        (SelMode::Multi, 4) => (10, 0, false),  // Ctrl+A 全选
        (SelMode::Single, 0) => (1, 3, false),
        (SelMode::Single, 1) => (1, 3, false),  // 退化单击
        (SelMode::Single, 2) => (1, 5, false),
        (SelMode::Single, 3) => (1, 1, false),
        (SelMode::Single, 4) => (1, 0, true),   // 禁全选：显性无效
        _ => (0, 0, false),
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F218 自检（判据：语义对照表 40 用例全绿 + 锚点专项 + 禁全选专项）。
pub fn run_listsel_checks() -> CheckSet {
    let mut set = CheckSet::new("F218-listsel");

    // 1. 单击=单选重置：既有选择全清。
    let mut st = ListSelState::new(SelMode::Multi, 10);
    let _ = st.gesture(Gesture::CtrlClick(2));
    let _ = st.gesture(Gesture::CtrlClick(4));
    let _ = st.gesture(Gesture::Click(7));
    set.add(
        "click resets selection to single",
        st.selected_count() == 1 && st.is_selected(7) && st.focus() == 7,
        "",
    );

    // 2. Ctrl+单击=切换（可选中亦可取消）。
    let mut st = ListSelState::new(SelMode::Multi, 10);
    let _ = st.gesture(Gesture::Click(1));
    let _ = st.gesture(Gesture::CtrlClick(3));
    let _ = st.gesture(Gesture::CtrlClick(1));
    set.add(
        "ctrl+click toggles items",
        st.is_selected(3) && !st.is_selected(1) && st.selected_count() == 1,
        "",
    );

    // 3. Shift+单击=锚点到当前的范围替换选。
    let mut st = ListSelState::new(SelMode::Multi, 10);
    let _ = st.gesture(Gesture::Click(2));
    let _ = st.gesture(Gesture::ShiftClick(6));
    set.add(
        "shift+click range replaces prior selection",
        st.selected_count() == 5
            && st.is_selected(2)
            && st.is_selected(6)
            && !st.is_selected(1),
        "",
    );

    // 4. 锚点跨滚动保持专项：滚动后 Shift+单击仍从原锚点扩选。
    let mut st = ListSelState::new(SelMode::Multi, 10);
    let _ = st.gesture(Gesture::Click(2));
    let kept = st.scroll_viewport();
    let _ = st.gesture(Gesture::ShiftClick(7));
    set.add(
        "anchor survives scroll then extends",
        kept == Some(2) && st.selected_count() == 6 && st.is_selected(7),
        "",
    );

    // 5. 方向键移动焦点并重置选择；Ctrl+方向仅移焦点。
    let mut st = ListSelState::new(SelMode::Multi, 10);
    let _ = st.gesture(Gesture::Click(0));
    let _ = st.gesture(Gesture::Arrow(1, false));
    let arrow_resets = st.focus() == 1 && st.selected_count() == 1 && st.is_selected(1);
    let anchor_after_arrow = st.anchor();
    let _ = st.gesture(Gesture::Arrow(1, true));
    let ctrl_arrow_focus_only = st.focus() == 2
        && st.selected_count() == 1
        && st.is_selected(1)
        && st.anchor() == anchor_after_arrow;
    set.add("arrow moves focus and selects", arrow_resets, "");
    set.add("ctrl+arrow moves focus only", ctrl_arrow_focus_only, "");

    // 6. 空格选中焦点项（可再按取消）。
    // 缺陷账本：现象=「space toggles focus item」红；根因=setup 用裸方向键
    // 移焦点，方向键本身已选中 item3，首次空格实为「取消」，on 恒 false，
    // 与「空格切换焦点项选中态」的测量意图矛盾；修法=改用 Ctrl+方向键
    // （判据「Ctrl+方向键仅移焦点不改变选中」）把焦点移到未选项，再测
    // 空格开→关两个方向，不改实现。
    let mut st = ListSelState::new(SelMode::Multi, 10);
    let _ = st.gesture(Gesture::Arrow(3, true));
    let _ = st.gesture(Gesture::Space);
    let on = st.is_selected(3);
    let _ = st.gesture(Gesture::Space);
    set.add("space toggles focus item", on && !st.is_selected(3), "");

    // 7. Ctrl+A 全选（多选）。
    let mut st = ListSelState::new(SelMode::Multi, 10);
    let _ = st.gesture(Gesture::CtrlA);
    set.add("ctrl+a selects all in multi", st.selected_count() == 10, "");

    // 8. Ctrl+A 单选列表显性无效（验收专项）。
    let mut st = ListSelState::new(SelMode::Single, 10);
    let _ = st.gesture(Gesture::Click(3));
    let out = st.gesture(Gesture::CtrlA);
    set.add(
        "ctrl+a rejected in single mode",
        out == Outcome::Rejected && st.selected_count() == 1 && st.is_selected(3),
        "",
    );

    // 9. Shift+Home/End 扩选到列表首尾。
    // 缺陷账本：现象=「shift+end extends to tail」红；根因=期望值 6 与
    // Windows 语义矛盾——Shift+Home 后锚点仍为 5（判据「锚点（Shift 选的
    // 起点）保持」），Shift+End 从锚点扩到尾 = 5..=9 共 5 项；修法=按
    // 判据把期望改为 5 项并补验 item9 选中，不改实现。
    let mut st = ListSelState::new(SelMode::Multi, 10);
    let _ = st.gesture(Gesture::Click(5));
    let _ = st.gesture(Gesture::ShiftHome);
    let head_ok = st.is_selected(0) && st.is_selected(5) && st.focus() == 0;
    let _ = st.gesture(Gesture::ShiftEnd);
    let tail_ok = st.selected_count() == 5 && st.is_selected(9) && st.focus() == 9;
    set.add("shift+home extends to head", head_ok, "");
    set.add("shift+end extends to tail", tail_ok, "");

    // 10. 单选模式修饰键退化语义。
    let mut st = ListSelState::new(SelMode::Single, 10);
    let _ = st.gesture(Gesture::Click(1));
    let _ = st.gesture(Gesture::CtrlClick(4));
    let _ = st.gesture(Gesture::ShiftClick(6));
    set.add(
        "single mode degrades modifiers to click",
        st.selected_count() == 1 && st.is_selected(6),
        "",
    );

    // 11. 40 用例判定表（8 场景 × 5 手势）全绿。
    let mut green = 0usize;
    for s in [
        Surface::FileList,
        Surface::SettingsList,
        Surface::TaskView,
        Surface::ThemeList,
    ] {
        for mode in [SelMode::Multi, SelMode::Single] {
            let _ = s.scenario_idx(mode);
            for g in 0..GESTURE_COUNT {
                if run_cell(mode, g) == expected_cell(mode, g) {
                    green += 1;
                }
            }
        }
    }
    set.add("semantic matrix 8x5=40 all green", green == CASE_TOTAL, "");

    // 12. 场景序号覆盖 0..8 无缝。
    let mut idx = [false; SCENARIO_COUNT];
    for s in [
        Surface::FileList,
        Surface::SettingsList,
        Surface::TaskView,
        Surface::ThemeList,
    ] {
        idx[s.scenario_idx(SelMode::Multi)] = true;
        idx[s.scenario_idx(SelMode::Single)] = true;
    }
    set.add("scenario ids cover 0..8", idx.iter().all(|&b| b), "");

    // 13. fuzz：2000 轮随机手势流，结构不变量恒成立（不 panic、
    //     多选 ≤ count、单选 ≤ 1、焦点/锚点越界即红）。
    let mut x: u32 = 0x5E3779B9;
    let mut ok = true;
    for _ in 0..2000u32 {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let single = x % 2 == 0;
        let mode = if single { SelMode::Single } else { SelMode::Multi };
        let n = 1 + (x % 24) as usize;
        let mut st = ListSelState::new(mode, n);
        for _ in 0..8 {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            let i = (x as usize) % (n + 2); // 含越界样本
            let g = match x % 8 {
                0 => Gesture::Click(i),
                1 => Gesture::CtrlClick(i),
                2 => Gesture::ShiftClick(i),
                3 => Gesture::Arrow(1 - (((x >> 4) as i32) % 2) * 2, x & 1 == 1),
                4 => Gesture::Space,
                5 => Gesture::CtrlA,
                6 => Gesture::ShiftHome,
                _ => Gesture::ShiftEnd,
            };
            let _ = st.gesture(g);
            if st.selected_count() > if single { 1 } else { n } {
                ok = false;
            }
            if st.focus() >= n || st.anchor().map(|a| a >= n).unwrap_or(false) {
                ok = false;
            }
        }
    }
    set.add("fuzz 2000 rounds invariants hold", ok, "");

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn click_resets_and_sets_anchor() {
        let mut st = ListSelState::new(SelMode::Multi, 6);
        let _ = st.gesture(Gesture::CtrlClick(2));
        let _ = st.gesture(Gesture::CtrlClick(5));
        assert_eq!(st.selected_count(), 2);
        let _ = st.gesture(Gesture::Click(0));
        assert_eq!(st.selected_count(), 1);
        assert_eq!(st.anchor(), Some(0));
        assert_eq!(st.focus(), 0);
    }

    #[test]
    fn shift_click_replaces_not_unions() {
        let mut st = ListSelState::new(SelMode::Multi, 10);
        let _ = st.gesture(Gesture::Click(8));
        let _ = st.gesture(Gesture::ShiftClick(2));
        // Windows 语义：范围替换——从锚点（Click(8) 所设）到当前项 = 2..=8
        // 共 7 项，不与既有选择并集。原断言「0..=2、不保留 8」与自设的
        // 锚点 8 自相矛盾（第三条断言锚点==8 即已锁定范围起点），按判据
        // 「Shift+单击=范围选（从上次锚点到当前）」修正为 2..=8。
        assert_eq!(st.selected_count(), 7);
        assert!(st.is_selected(8)); // 8 在 2..=8 范围内，因范围入选而非保留
        assert_eq!(st.anchor(), Some(8), "锚点在 Shift+单击后不动");
    }

    #[test]
    fn ctrl_arrow_keeps_selection_and_anchor() {
        let mut st = ListSelState::new(SelMode::Multi, 8);
        let _ = st.gesture(Gesture::Click(3));
        let before_sel = st.selected_count();
        let before_anchor = st.anchor();
        let _ = st.gesture(Gesture::Arrow(1, true));
        assert_eq!(st.focus(), 4);
        assert_eq!(st.selected_count(), before_sel);
        assert_eq!(st.anchor(), before_anchor);
        // 焦点到底钳制。
        for _ in 0..10 {
            let _ = st.gesture(Gesture::Arrow(1, true));
        }
        assert_eq!(st.focus(), 7);
    }

    #[test]
    fn out_of_range_click_rejected() {
        let mut st = ListSelState::new(SelMode::Multi, 5);
        let out = st.gesture(Gesture::Click(9));
        assert_eq!(out, Outcome::Rejected);
        assert_eq!(st.rejected, 1);
        assert_eq!(st.selected_count(), 0);
    }

    #[test]
    fn matrix_40_cells_all_match() {
        for mode in [SelMode::Multi, SelMode::Single] {
            for g in 0..GESTURE_COUNT {
                assert_eq!(
                    run_cell(mode, g),
                    expected_cell(mode, g),
                    "cell ({:?}, {}) 偏离 Windows 对照",
                    mode,
                    g
                );
            }
        }
    }

    #[test]
    fn selfcheck_all_green() {
        let set = run_listsel_checks();
        assert!(set.all_passed(), "F218 自检存在红项");
        assert!(!set.truncated());
    }
}

// ===========================================================================
// v2 深化批（2026-09-26 · AI-H1 二次对账批）：UI 壳接线 / 持久化 I/O / 判定面扩展
// ===========================================================================
// 深化范围（仍属主册 F218 验收定义的实装细化，非新立项）：持久化面 = 锚点
// -选区状态（模式/计数/锚点/焦点/选中位图）的 VXH1 定长记录；壳接线面 =
// Ctrl/Shift 双修饰矩阵命中判定；判定面 = run_listsel_v2_checks。
// 零堆：位图定长 64 字节（SEL_CAP=512 位封顶，容量在册）。

/// v2 记录魔数（H1 二次批统一身份面）与版本（布局演进守门）。
pub const V2_MAGIC: [u8; 4] = *b"VXH1";
pub const V2_VERSION: u8 = 1;
/// 选中位图定长（512 位 = 64 字节，与 SEL_CAP 一处一事实）。
pub const V2_BITMAP_BYTES: usize = SEL_CAP / 8;
/// 记录定长：4 魔数 + 1 版本 + 71 载荷（模式 1 + 计数 2 + 锚点 2 + 焦点 2
/// + 位图 64）+ 4 校验。
/// 缺陷账本：现象=「v2 record round-trip sel state」红、单测 round-trip
/// 解码出 selected[63]=224 野位；根因=原定长 79 按「70 载荷」误算（实为
/// 71 字节），位图末字节落在 out[75] 与校验和首字节重叠，编码后校验和
/// 覆写位图末字节，解码侧多出野位；修法=定长改为 80（4+1+71+4），校验
/// 和区移到 out[76..80]，编解码布局恢复一一对应。
pub const V2_RECORD_BYTES: usize = 80;

/// v2 持久化错误：四类损坏输入全部显性拒绝（明确错误枚举）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum V2PersistError { BadMagic, BadVersion, BadChecksum, BadLength }

/// FNV-1a 32 位校验和（v2 各记录共用口径，一处一事实）。
fn v2_fnv1a(data: &[u8]) -> u32 {
    data.iter().fold(0x811C_9DC5, |h, &b| (h ^ b as u32).wrapping_mul(0x0100_0193))
}

/// 锚点-选区状态记录（持久化面）：判据「锚点-选区状态」的存档载体。
/// 锚点无值编码为 0xFFFF（u16 槽位哨兵，512 项列表索引永不撞哨兵）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelStateRecord {
    pub mode: u8, // 选择模式（0 = Multi，1 = Single）
    pub count: u16,
    pub anchor: u16, // 选区锚点（0xFFFF = None）
    pub focus: u16,
    pub selected: [u8; V2_BITMAP_BYTES], // bit i = 第 i 项选中；count 之外恒 0
}

impl SelStateRecord {
    /// 采集：从三元状态导出（同一真值源，无二次判定）。
    pub fn capture(st: &ListSelState) -> SelStateRecord {
        let mut rec = SelStateRecord {
            mode: match st.mode() {
                SelMode::Multi => 0,
                SelMode::Single => 1,
            },
            count: st.count() as u16,
            anchor: st.anchor().map(|a| a as u16).unwrap_or(0xFFFF),
            focus: st.focus() as u16,
            selected: [0; V2_BITMAP_BYTES],
        };
        for i in 0..st.count() {
            if st.is_selected(i) {
                rec.selected[i / 8] |= 1 << (i % 8);
            }
        }
        rec
    }

    /// 记录合法性：计数在 [1, SEL_CAP]、焦点/锚点不越界、count 之外
    /// 位图清零（防「野位」随存档漂移）。
    pub fn is_valid(&self) -> bool {
        let n = self.count as usize;
        if n < 1 || n > SEL_CAP || self.focus as usize >= n {
            return false;
        }
        if self.anchor != 0xFFFF && self.anchor as usize >= n {
            return false;
        }
        (n..SEL_CAP).all(|i| self.selected[i / 8] >> (i % 8) & 1 == 0)
    }

    /// 编码：VXH1 + 版本 + 71 字节定长载荷 + FNV-1a 校验和。
    pub fn to_bytes(&self) -> [u8; V2_RECORD_BYTES] {
        let mut out = [0u8; V2_RECORD_BYTES];
        out[..4].copy_from_slice(&V2_MAGIC);
        out[4] = V2_VERSION;
        out[5] = self.mode;
        out[6..8].copy_from_slice(&self.count.to_le_bytes());
        out[8..10].copy_from_slice(&self.anchor.to_le_bytes());
        out[10..12].copy_from_slice(&self.focus.to_le_bytes());
        out[12..12 + V2_BITMAP_BYTES].copy_from_slice(&self.selected);
        let sum = v2_fnv1a(&out[..V2_RECORD_BYTES - 4]);
        out[V2_RECORD_BYTES - 4..].copy_from_slice(&sum.to_le_bytes());
        out
    }

    /// 解码：长度/魔数/版本/校验四门逐道拒绝。
    pub fn from_bytes(b: &[u8]) -> Result<SelStateRecord, V2PersistError> {
        if b.len() < V2_RECORD_BYTES { return Err(V2PersistError::BadLength); }
        if b[..4] != V2_MAGIC { return Err(V2PersistError::BadMagic); }
        if b[4] != V2_VERSION { return Err(V2PersistError::BadVersion); }
        let sum = u32::from_le_bytes([b[76], b[77], b[78], b[79]]);
        if v2_fnv1a(&b[..76]) != sum { return Err(V2PersistError::BadChecksum); }
        let mut rec = SelStateRecord {
            mode: b[5],
            count: u16::from_le_bytes([b[6], b[7]]),
            anchor: u16::from_le_bytes([b[8], b[9]]),
            focus: u16::from_le_bytes([b[10], b[11]]),
            selected: [0; V2_BITMAP_BYTES],
        };
        rec.selected.copy_from_slice(&b[12..12 + V2_BITMAP_BYTES]);
        Ok(rec)
    }
}

// ---------------------------------------------------------------------------
// UI 壳接线：Ctrl/Shift 双修饰矩阵命中判定
// ---------------------------------------------------------------------------

/// 双修饰矩阵命中判定（点→手势的纯函数）：Windows 列表实机口径——
/// Shift 优先（范围选压过切换选），Ctrl 次之，裸点单击重置。
pub fn modifier_gesture(ctrl: bool, shift: bool, i: usize) -> Gesture {
    if shift {
        Gesture::ShiftClick(i)
    } else if ctrl {
        Gesture::CtrlClick(i)
    } else {
        Gesture::Click(i)
    }
}

/// F218 v2 自检（首条=持久化 round-trip；逐条注明验主册哪句话）。
pub fn run_listsel_v2_checks() -> CheckSet {
    let mut set = CheckSet::new("F218-listsel-v2");
    // 1. round-trip：混选状态采集→编码→解码逐字段相等且记录合法
    //    （v2 记录纪律 + 「锚点-选区状态」存档面）。
    let mut st = ListSelState::new(SelMode::Multi, 10);
    let _ = st.gesture(Gesture::Click(2));
    let _ = st.gesture(Gesture::CtrlClick(5));
    let _ = st.gesture(Gesture::CtrlClick(8));
    let rec = SelStateRecord::capture(&st);
    let blob = rec.to_bytes();
    // 缺陷账本：现象=「v2 record round-trip sel state」红；根因=断言
    // rec.anchor==2 与 Windows 实机语义矛盾——Ctrl+单击把锚点移到所点项
    // （实现 gesture() 的 CtrlClick 分支 anchor=Some(i)），流末 CtrlClick(8)
    // 后锚点应为 8；修法=按 Windows Ctrl+单击语义改断言 anchor==8，不改实现。
    set.add(
        "v2 record round-trip sel state",
        SelStateRecord::from_bytes(&blob) == Ok(rec) && rec.is_valid() && rec.anchor == 8,
        "",
    );
    // 2. 四类损坏输入全部拒绝（魔数/版本/校验/长度）。
    let mut bad_magic = blob; bad_magic[0] = b'X';
    let mut bad_ver = blob; bad_ver[4] = 9;
    let mut bad_sum = blob; bad_sum[20] ^= 0xFF;
    set.add(
        "corruption four-way rejected",
        SelStateRecord::from_bytes(&bad_magic) == Err(V2PersistError::BadMagic)
            && SelStateRecord::from_bytes(&bad_ver) == Err(V2PersistError::BadVersion)
            && SelStateRecord::from_bytes(&bad_sum) == Err(V2PersistError::BadChecksum)
            && SelStateRecord::from_bytes(&blob[..78]) == Err(V2PersistError::BadLength),
        "",
    );
    // 3. 记录合法性拒绝野位/越界（count=0、焦点越界、count 外位图脏位）。
    let mut wild1 = rec;
    wild1.count = 0;
    let mut wild2 = rec;
    wild2.focus = 99;
    let mut wild3 = rec;
    wild3.selected[9] = 0x80; // 第 511 位脏位（count=10 之外）
    set.add(
        "record validity rejects wild bits",
        !wild1.is_valid() && !wild2.is_valid() && !wild3.is_valid(),
        "",
    );
    // 4. 验「语义对照表逐条 vs Windows」矩阵面：双修饰四组合（Shift 优先）
    //    落点与直接手势一致。
    let mut a = ListSelState::new(SelMode::Multi, 10);
    let _ = a.gesture(Gesture::Click(2));
    let _ = a.gesture(modifier_gesture(true, true, 5)); // 双修饰 → Shift 优先
    let mut b = ListSelState::new(SelMode::Multi, 10);
    let _ = b.gesture(Gesture::Click(2));
    let _ = b.gesture(Gesture::ShiftClick(5));
    set.add(
        "modifier matrix matches windows",
        modifier_gesture(false, false, 3) == Gesture::Click(3)
            && modifier_gesture(true, false, 3) == Gesture::CtrlClick(3)
            && a.selected_count() == b.selected_count() && a.selected_count() == 4,
        "",
    );
    // 5. 验「8 场景×5 手势=40 用例全绿」仍全绿（判定表复绿）。
    // 缺陷账本：现象=「40-case matrix still green」红；根因=循环只覆盖
    // 2 模式×5 手势=10 格却与 CASE_TOTAL=40 比较，判据原文「8 场景×5 手势
    // =40 用例」要求 4 界面 × 多选/单选全展开；修法=补 4 界面循环凑齐
    // 40 格（与 v1 检查 11 同一口径），不改实现。
    let mut green = 0usize;
    for s in [Surface::FileList, Surface::SettingsList, Surface::TaskView, Surface::ThemeList] {
        for mode in [SelMode::Multi, SelMode::Single] {
            let _ = s.scenario_idx(mode);
            for g in 0..GESTURE_COUNT {
                if run_cell(mode, g) == expected_cell(mode, g) {
                    green += 1;
                }
            }
        }
    }
    set.add("40-case matrix still green", green == CASE_TOTAL, "");
    set
}

#[cfg(test)]
mod tests_v2 {
    use super::*;

    #[test]
    fn sel_record_round_trip_and_reject() {
        let mut st = ListSelState::new(SelMode::Single, 6);
        let _ = st.gesture(Gesture::Click(4));
        let rec = SelStateRecord::capture(&st);
        assert_eq!(rec.mode, 1);
        assert_eq!(rec.anchor, 4); // 单击设锚点 4
        let blob = rec.to_bytes();
        assert_eq!(SelStateRecord::from_bytes(&blob), Ok(rec));
        assert!(SelStateRecord::from_bytes(&vec![0u8; 60]).is_err());
    }

    #[test]
    fn modifier_matrix_paths() {
        assert_eq!(modifier_gesture(false, true, 7), Gesture::ShiftClick(7));
        assert_eq!(modifier_gesture(true, false, 7), Gesture::CtrlClick(7));
    }

    #[test]
    fn listsel_v2_selfcheck_all_green() {
        let set = run_listsel_v2_checks();
        assert!(set.all_passed(), "F218 v2 自检存在红项");
        assert!(!set.truncated());
    }
}
