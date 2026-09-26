//! F435 下拉框键盘操作 · 完整设计（STAR I 主册 G-I-35）。
//!
//! **判据（主册）**：三招行为矩阵；跳选循环；预览代值与 Esc 恢复；滚动
//! 跟随；展开收起时序 <100ms。＋通12。
//!
//! 设计：下拉框键盘核——三招（Alt+下/Enter 展开；输入首字跳选循环；
//! 方向键预览代值）；预览态（高亮即临时代值，确认才落定，Esc 恢复原值
//! ——无损反悔）；滚动跟随（预览位联动视窗顶）；展开 <100ms 记账。

use crate::checks::CheckSet;

use alloc::vec::Vec;

/// 展开收起判线（ms）。
pub const TOGGLE_BUDGET_MS: u64 = 100;

/// 下拉框核。
pub struct Dropdown {
    pub options: Vec<&'static str>,
    /// 已落定值。
    pub value: usize,
    pub open: bool,
    /// 预览位（展开后方向键高亮位；None = 未预览）。
    pub preview: Option<usize>,
    /// 展开前原值（Esc 恢复用）。
    pre_open_value: usize,
    pub last_toggle_ms: Option<u64>,
    pub over_budget: u64,
    /// 滚动视窗顶（跟随预览位）。
    pub scroll_top: usize,
    /// 视窗可见行数。
    pub visible_rows: usize,
}

impl Dropdown {
    pub fn new(options: Vec<&'static str>, visible_rows: usize) -> Dropdown {
        Dropdown {
            options,
            value: 0,
            open: false,
            preview: None,
            pre_open_value: 0,
            last_toggle_ms: None,
            over_budget: 0,
            scroll_top: 0,
            visible_rows: visible_rows.max(1),
        }
    }

    /// 第一招：Alt+下 / Enter = 展开收起（<100ms 记账）。
    pub fn toggle(&mut self, latency_ms: u64) -> bool {
        self.last_toggle_ms = Some(latency_ms);
        if latency_ms > TOGGLE_BUDGET_MS {
            self.over_budget += 1;
        }
        self.open = !self.open;
        if self.open {
            self.pre_open_value = self.value;
            self.preview = Some(self.value);
            self.follow();
        } else {
            self.preview = None;
        }
        self.open
    }

    /// 第二招：输入首字跳选（同字母循环命中；预览代值）。
    pub fn jump_letter(&mut self, ch: u8) -> bool {
        if !self.open || self.options.is_empty() {
            return false;
        }
        let start = self.preview.unwrap_or(self.value);
        let n = self.options.len();
        for step in 1..=n {
            let i = (start + step) % n;
            if self.options[i].as_bytes().first() == Some(&ch) {
                self.preview = Some(i);
                self.follow();
                return true;
            }
        }
        false
    }

    /// 第三招：方向键预览（上/下循环；滚动跟随）。
    pub fn preview_move(&mut self, delta: i32) {
        if !self.open || self.options.is_empty() {
            return;
        }
        let base = self.preview.unwrap_or(self.value) as i32;
        let n = self.options.len() as i32;
        self.preview = Some(((base + delta).rem_euclid(n)) as usize);
        self.follow();
    }

    /// 滚动跟随：预览到哪滚到哪。
    fn follow(&mut self) {
        if let Some(p) = self.preview {
            if p < self.scroll_top {
                self.scroll_top = p;
            } else if p >= self.scroll_top + self.visible_rows {
                self.scroll_top = p + 1 - self.visible_rows;
            }
        }
    }

    /// 确认（Enter）：预览值落定。
    pub fn confirm(&mut self) -> Option<&'static str> {
        if !self.open {
            return None;
        }
        let v = self.preview?;
        self.value = v;
        self.open = false;
        self.preview = None;
        Some(self.options[v])
    }

    /// Esc 恢复原值（无损反悔——落定值不动）。
    pub fn esc_revert(&mut self) -> bool {
        if !self.open {
            return false;
        }
        self.value = self.pre_open_value;
        self.open = false;
        self.preview = None;
        true
    }

    /// 预览代值读数（界面层显示用——预览态显示预览值）。
    pub fn displayed(&self) -> &str {
        match self.preview {
            Some(p) if self.open => self.options[p],
            _ => self.options[self.value],
        }
    }

    /// 展开时序判据。
    pub fn within_budget(&self) -> bool {
        self.last_toggle_ms.map(|m| m <= TOGGLE_BUDGET_MS).unwrap_or(false)
    }
}

pub fn run_dropdown_checks() -> CheckSet {
    let mut set = CheckSet::new("uni1-F435");
    let opts = alloc::vec![
        "UTC-8 洛杉矶", "UTC+0 伦敦", "UTC+8 北京", "UTC+9 东京", "UTC+10 悉尼", "UTC+12 奥克兰",
    ];
    let mut d = Dropdown::new(opts, 4);
    d.value = 2; // 北京
    // 第一招：展开 <100ms。
    set.add(
        "f435-toggle-fast",
        d.toggle(60) && d.open && d.within_budget() && d.over_budget == 0,
        "",
    );
    set.add("f435-preview-starts-at-value", d.preview == Some(2) && d.displayed() == "UTC+8 北京", "");
    // 第三招：方向键预览 + 滚动跟随。
    d.preview_move(1);
    set.add(
        "f435-preview-move",
        d.preview == Some(3) && d.displayed() == "UTC+9 东京" && d.scroll_top == 0,
        "",
    );
    d.preview_move(1);
    d.preview_move(1); // 3→4→5
    set.add(
        "f435-scroll-follow",
        d.preview == Some(5) && d.scroll_top == 2,
        "",
    );
    d.preview_move(1); // 5 → 0 循环
    set.add("f435-preview-wrap", d.preview == Some(0) && d.scroll_top == 0, "");
    // 第二招：首字跳选（六个时区项全以 UTC 开头——同字母跳选在全部
    // 命中项间循环：0→1 起，逐发步进，5 后回环 0）。
    set.add(
        "f435-jump-letter",
        d.jump_letter(b'U') && d.preview == Some(1),
        "",
    );
    let mut cycled = true;
    for expect in 2..=5 {
        cycled &= d.jump_letter(b'U') && d.preview == Some(expect);
    }
    cycled &= d.jump_letter(b'U') && d.preview == Some(0);
    set.add("f435-jump-cycle", cycled, "");
    // 确认落定（预览末项 → Enter：0 环回一步到 5）。
    d.preview_move(-1);
    set.add(
        "f435-confirm",
        d.confirm() == Some("UTC+12 奥克兰") && d.value == 5 && !d.open && d.displayed() == "UTC+12 奥克兰",
        "",
    );
    // Esc 恢复原值（无损反悔）。
    let mut e = Dropdown::new(alloc::vec!["甲", "乙", "丙"], 3);
    e.value = 1;
    let _ = e.toggle(50);
    e.preview_move(1); // 预览 丙
    set.add("f435-preview-temp", e.displayed() == "丙", "");
    set.add(
        "f435-esc-revert",
        e.esc_revert() && e.value == 1 && !e.open && e.displayed() == "乙",
        "",
    );
    // 收起 <100ms（开→关两拍都在判线内）+ 超线记账（再开一拍超线）。
    let _ = e.toggle(50);
    let _ = e.toggle(80);
    set.add("f435-collapse-fast", !e.open && e.within_budget(), "");
    let _ = e.toggle(200);
    set.add("f435-over-budget-logged", e.over_budget == 1 && e.open, "");
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_list_visible_window() {
        let opts: Vec<&'static str> = (0..40).map(|i| match i {
            0 => "a0", 1 => "a1", 2 => "a2", 3 => "a3", _ => "x",
        }).collect();
        let mut d = Dropdown::new(opts, 5);
        let _ = d.toggle(50);
        d.preview_move(3); // → 3（视窗 0..5 内）
        assert_eq!(d.scroll_top, 0);
        d.preview_move(1); // → 4（仍在视窗内，顶不变）
        assert_eq!(d.scroll_top, 0);
        d.preview_move(1); // → 5（出视窗 → 顶 = 1）
        assert_eq!(d.scroll_top, 1);
        d.preview_move(1); // → 6（出视窗 [1..6) → 顶 = 2）
        assert_eq!(d.scroll_top, 2);
    }
}
