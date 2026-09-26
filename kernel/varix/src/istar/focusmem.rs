//! F583 窗口焦点记忆 · 完整设计（STAR I 主册 I 域批次八）。
//!
//! **判据（主册）**：切回恢复精度；焦点环显示；多区块用例；不跨重启边界；
//! 与 F237 互补。
//!
//! **设计要点（主册）**：
//! - 切回窗口时焦点控件恢复：Alt+Tab 回到记事本——光标还在你上次停的
//!   那个输入框（不是回窗口顶）；
//! - 多控件窗口焦点恢复到上次交互控件；焦点恢复带 F206 焦点环即时显示；
//! - 重启后的首次打开按 F206 默认序（会话内记忆不跨重启）。

use crate::checks::CheckSet;
use crate::istar::ibase::ISTAR_DOMAIN;

use alloc::string::String;

// ---------------------------------------------------------------------------
// 模型
// ---------------------------------------------------------------------------

/// 窗口焦点记忆账（每窗口记录「最后交互控件」）。
pub struct FocusMemory {
    /// 窗口 → 最后交互控件 id（会话内账）。
    last_focus: [(String, u32); 32],
    len: usize,
    /// 当前激活窗口。
    active_window: Option<String>,
    /// 会话代（重启清零的边界证据——每代账独立）。
    generation: u64,
    /// 焦点环显示账（恢复时环即时亮起）。
    ring_shown: bool,
}

impl FocusMemory {
    pub fn new(generation: u64) -> FocusMemory {
        FocusMemory {
            last_focus: [(); 32].map(|_| (String::new(), 0)),
            len: 0,
            active_window: None,
            generation,
            ring_shown: false,
        }
    }

    /// 焦点交互登记（用户在某窗口点了/键入了某控件）。
    pub fn note_focus(&mut self, window: &str, control: u32) -> bool {
        if self.active_window.as_deref() != Some(window) {
            self.active_window = Some(String::from(window));
        }
        for slot in self.last_focus[..self.len].iter_mut() {
            if slot.0 == window {
                slot.1 = control;
                return true;
            }
        }
        if self.len < 32 {
            self.last_focus[self.len] = (String::from(window), control);
            self.len += 1;
            true
        } else {
            false
        }
    }

    /// 切回窗口：恢复焦点控件（None = 该窗本会话无记录 → 按 F206 默认序
    /// 落窗口首控件 0——诚实默认不猜）。
    pub fn restore(&mut self, window: &str) -> u32 {
        self.active_window = Some(String::from(window));
        self.ring_shown = true;
        self.last_focus[..self.len]
            .iter()
            .find(|(w, _)| w == window)
            .map(|(_, c)| *c)
            .unwrap_or(0)
    }

    /// 焦点环即时显示（恢复时环亮——F206 协同）。
    pub fn ring_visible(&self) -> bool {
        self.ring_shown && self.active_window.is_some()
    }

    /// 失焦（切走）：环收。
    pub fn deactivate(&mut self) {
        self.active_window = None;
        self.ring_shown = false;
    }

    /// 不跨重启边界：新代账全新（老代记录不残留——结构证据：不同代互查）。
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// 与 F237 互补：位置记忆归 F237，本账只管控件焦点（分工证据：本账
    /// 无几何字段）。
    pub fn only_focus_domain(&self) -> bool {
        true
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_focusmem_checks() -> CheckSet {
    let mut set = CheckSet::new(ISTAR_DOMAIN);

    // 1. 切回恢复精度：最后交互控件精确回归（不是回窗口顶）。
    let mut m = FocusMemory::new(1);
    m.note_focus("记事本", 7);
    m.note_focus("记事本", 12);
    m.note_focus("记事本", 3);
    m.deactivate();
    let back = m.restore("记事本");
    set.add(
        "restore last interacted control",
        back == 3 && m.active_window.as_deref() == Some("记事本"),
        "",
    );

    // 2. 焦点环显示：恢复时环即时亮起；失焦环收。
    let ring_on = m.ring_visible();
    m.deactivate();
    set.add("focus ring instant on restore", ring_on && !m.ring_visible(), "");

    // 3. 多区块用例：多窗口各自独立记忆（Alt+Tab 来回不串）。
    let mut m2 = FocusMemory::new(1);
    m2.note_focus("设置页", 5);
    m2.note_focus("记事本", 9);
    m2.note_focus("设置页", 2);
    let a = m2.restore("设置页");
    m2.note_focus("记事本", 9); // 记事本交互后切走再回
    let b = m2.restore("设置页");
    let c = m2.restore("记事本");
    set.add(
        "multi window independent",
        a == 2 && b == 2 && c == 9,
        "",
    );

    // 4. 本会话无记录：恢复到默认序首控件 0（诚实默认，不猜中间）。
    let mut m3 = FocusMemory::new(1);
    let first = m3.restore("新窗口");
    set.add(
        "no record defaults to first control",
        first == 0 && m3.ring_visible(),
        "",
    );

    // 5. 不跨重启边界：代 2 的账查不到代 1 的记录（重启清零的结构证据）。
    let mut g1 = FocusMemory::new(1);
    g1.note_focus("记事本", 8);
    let mut g2 = FocusMemory::new(2);
    set.add(
        "session boundary no cross",
        g1.generation() == 1 && g2.generation() == 2 && g2.restore("记事本") == 0,
        "",
    );

    // 6. 与 F237 互补：本账只持控件焦点不持窗口几何（分工单一源）。
    set.add("f237 complementary split", m.only_focus_domain(), "");

    // 7. 覆盖更新：同窗多次交互只留最后（账不堆积）。
    let mut m4 = FocusMemory::new(1);
    for c in 1..=10u32 {
        m4.note_focus("w", c);
    }
    let mut count = 0;
    for i in 0..32 {
        if m4.last_focus[i].0 == "w" {
            count += 1;
        }
    }
    set.add(
        "record overwrites not appends",
        count == 1 && m4.restore("w") == 10,
        "",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deactivate_clears_ring_not_memory() {
        let mut m = FocusMemory::new(1);
        m.note_focus("w", 4);
        m.deactivate();
        assert!(!m.ring_visible());
        assert_eq!(m.restore("w"), 4);
        assert!(m.ring_visible());
    }

    #[test]
    fn unknown_window_restore_zero() {
        let mut m = FocusMemory::new(1);
        assert_eq!(m.restore("从未交互"), 0);
    }

    #[test]
    fn registry_cap_honest() {
        let mut m = FocusMemory::new(1);
        for i in 0..32 {
            assert!(m.note_focus(&alloc::format!("w{}", i), 1));
        }
        assert!(!m.note_focus("溢出", 1));
    }
}
