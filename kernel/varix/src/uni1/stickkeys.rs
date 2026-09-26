//! F450 粘滞键与筛选键 · 完整设计（STAR I 主册 G-I-50）。
//!
//! **判据（主册）**：粘滞键逐键等效用例（五组合键）；五次触发与确认
//! 框；筛选键阈值参数与效果；指示器；永不再提醒。＋通12。
//!
//! 设计：键盘无障碍双件核——**粘滞键**：修饰键锁存状态机（按下松开
//! 即锁存，再按实体键时合成组合键——Ctrl 按下松开再按 C 等效 Ctrl+C；
//! 五组合键逐键等效用例机检）；连按 5 次 Shift 触发启用（带确认框防
//! 误启——确认前不生效）；**筛选键**：最短有效按下时长阈值（短促误击
//! 忽略）+ 重复抑制（长按不重复）；指示器（任一开启即显示——任务栏
//! 指示位账）；提示纪律（启用提示一次，「不再提醒」登记——Windows
//! 历史上弹窗烦人的教训吸收）；两件默认关。

use crate::checks::CheckSet;

use alloc::vec::Vec;

/// 粘滞键触发：Shift 连按次数。
pub const STICKY_TOGGLE_PRESSES: usize = 5;
/// 筛选键默认阈值：短于此的按下视为误击（ms）。
pub const FILTER_MIN_HOLD_MS: u64 = 50;

/// 修饰键。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Modifier {
    Ctrl,
    Alt,
    Shift,
    Win,
}

impl Modifier {
    pub fn key_code(self) -> u8 {
        match self {
            Modifier::Ctrl => 0x11,
            Modifier::Alt => 0x12,
            Modifier::Shift => 0x10,
            Modifier::Win => 0x5B,
        }
    }
}

/// 粘滞键锁存状态机。
pub struct StickyKeys {
    pub enabled: bool,
    /// 确认框待决（连按 5 次后弹出——确认前不生效）。
    pub pending_confirm: bool,
    /// 「不再提醒」登记。
    pub never_remind: bool,
    /// 锁存的修饰键（按顺序——一次一个，再按同键取消）。
    pub latched: Vec<Modifier>,
    /// Shift 连按计数（触发判定窗内）。
    pub shift_presses: usize,
    /// 启用提示账（每次启用提示一次）。
    pub enable_notices: u64,
}

impl StickyKeys {
    pub fn new() -> StickyKeys {
        StickyKeys {
            enabled: false,
            pending_confirm: false,
            never_remind: false,
            latched: Vec::new(),
            shift_presses: 0,
            enable_notices: 0,
        }
    }

    /// 连按 5 次 Shift：弹确认框（防误启）。已启用时连按 = 快捷关闭
    /// （Windows 惯例：关闭不需要确认）。
    pub fn shift_press(&mut self) {
        if self.enabled {
            self.disable();
            return;
        }
        if self.never_remind {
            // 已登记不再提醒：直接启用（用户已表达过意愿）。
            self.enable();
            return;
        }
        self.shift_presses += 1;
        if self.shift_presses >= STICKY_TOGGLE_PRESSES {
            self.pending_confirm = true;
            self.shift_presses = 0;
        }
    }

    /// 确认框：接受 → 启用 + 提示一次。
    pub fn confirm_enable(&mut self) -> bool {
        if !self.pending_confirm {
            return false;
        }
        self.pending_confirm = false;
        self.enable();
        true
    }

    pub fn dismiss_confirm(&mut self) {
        self.pending_confirm = false;
    }

    fn enable(&mut self) {
        self.enabled = true;
        self.enable_notices += 1; // 每次启用提示一次（温和、可关）
    }

    fn disable(&mut self) {
        self.enabled = false;
        self.latched.clear();
    }

    /// 修饰键按下+松开（粘滞语义：松开时锁存）。
    pub fn modifier_tap(&mut self, m: Modifier) {
        if !self.enabled {
            return;
        }
        if let Some(pos) = self.latched.iter().position(|l| *l == m) {
            self.latched.remove(pos); // 再按同键 = 取消锁存
        } else {
            self.latched.push(m);
        }
    }

    /// 实体键到达：合成组合键 = 锁存修饰键 + 该键；随后锁存清空
    /// （一次合成后修饰键回弹——逐键等效的落点）。
    pub fn key_press(&mut self, key: u8) -> (Vec<Modifier>, u8) {
        let combo = (self.latched.clone(), key);
        self.latched.clear();
        combo
    }

    /// 逐键等效判据：粘滞路径合成结果 == 直按组合键语义
    /// （修饰键集合 + 键码相等）。
    pub fn equivalent_to(direct: &[Modifier], sticky: &[Modifier], key: u8, sticky_key: u8) -> bool {
        key == sticky_key && direct.len() == sticky.len() && direct.iter().all(|m| sticky.contains(m))
    }
}

/// 筛选键。
pub struct FilterKeys {
    pub enabled: bool,
    /// 最短有效按下时长（ms，可调参数）。
    pub min_hold_ms: u64,
    /// 忽略的短促误击计数。
    pub ignored_taps: u64,
    /// 抑制的长按重复计数。
    pub suppressed_repeats: u64,
}

impl FilterKeys {
    pub fn new(min_hold_ms: u64) -> FilterKeys {
        FilterKeys { enabled: false, min_hold_ms: min_hold_ms.max(1), ignored_taps: 0, suppressed_repeats: 0 }
    }

    /// 按下判定：时长 < 阈值 → 误击忽略（手抖用户的救星）。
    pub fn press(&mut self, hold_ms: u64) -> bool {
        if !self.enabled {
            return true; // 关闭时全通
        }
        if hold_ms < self.min_hold_ms {
            self.ignored_taps += 1;
            return false;
        }
        true
    }

    /// 长按重复抑制：开启时键盘自动重复不生效（逐键确认输入）。
    pub fn auto_repeat(&mut self) -> bool {
        if self.enabled {
            self.suppressed_repeats += 1;
            return false;
        }
        true
    }
}

/// 指示器：任一开启即显示。
pub fn accessibility_indicator(sticky_on: bool, filter_on: bool) -> bool {
    sticky_on || filter_on
}

pub fn run_stickkeys_checks() -> CheckSet {
    let mut set = CheckSet::new("uni1-F450");
    // 粘滞键：连按 5 次 → 确认框（防误启）→ 确认才启用。
    let mut s = StickyKeys::new();
    for _ in 0..(STICKY_TOGGLE_PRESSES - 1) {
        s.shift_press();
    }
    set.add("f450-four-presses-no-popup", !s.pending_confirm && !s.enabled, "");
    s.shift_press();
    set.add("f450-five-presses-confirm", s.pending_confirm && !s.enabled, "");
    set.add("f450-confirm-enables", s.confirm_enable() && s.enabled && s.enable_notices == 1, "");
    // 逐键等效五组合键（判据主用例）。
    let combos: [(Vec<Modifier>, u8); 5] = [
        (alloc::vec![Modifier::Ctrl], b'C'),
        (alloc::vec![Modifier::Ctrl, Modifier::Shift], b'N'),
        (alloc::vec![Modifier::Alt], 0xF4),
        (alloc::vec![Modifier::Ctrl, Modifier::Alt], b'D'),
        (alloc::vec![Modifier::Ctrl, Modifier::Shift, Modifier::Alt], 0x53),
    ];
    let mut all_equiv = true;
    for (direct, key) in combos.iter() {
        let mut sticky = StickyKeys::new();
        sticky.enabled = true;
        for m in direct.iter() {
            sticky.modifier_tap(*m);
        }
        let (latched, got_key) = sticky.key_press(*key);
        all_equiv &= StickyKeys::equivalent_to(direct, &latched, *key, got_key) && sticky.latched.is_empty();
    }
    set.add("f450-five-combos-equivalent", all_equiv, "");
    // 锁存取消：再按同键取消锁存（不合成）。
    let mut t = StickyKeys::new();
    t.enabled = true;
    t.modifier_tap(Modifier::Ctrl);
    t.modifier_tap(Modifier::Ctrl);
    set.add("f450-relatch-cancels", t.latched.is_empty(), "");
    // 筛选键：阈值参数 + 误击忽略 + 重复抑制。
    let mut f = FilterKeys::new(FILTER_MIN_HOLD_MS);
    f.enabled = true;
    set.add(
        "f450-filter-taps-ignored",
        !f.press(20) && !f.press(49) && f.ignored_taps == 2 && f.press(50),
        "",
    );
    set.add("f450-filter-repeat-suppressed", !f.auto_repeat() && f.suppressed_repeats == 1, "");
    // 阈值参数可调（唯一登记点改值 → 行为跟随）。
    f.min_hold_ms = 120;
    set.add("f450-filter-threshold-adjustable", f.press(60) == false && f.press(120), "");
    // 指示器：任一开启即显示；全关不占位。
    set.add(
        "f450-indicator",
        accessibility_indicator(true, false)
            && accessibility_indicator(false, true)
            && accessibility_indicator(true, true)
            && !accessibility_indicator(false, false),
        "",
    );
    // 永不再提醒：登记后连按直接启用（不再弹框）。
    let mut n = StickyKeys::new();
    n.never_remind = true;
    n.shift_press();
    set.add(
        "f450-never-remind",
        n.enabled && !n.pending_confirm && n.enable_notices == 1,
        "",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sticky_off_keys_pass_through() {
        let mut s = StickyKeys::new();
        s.modifier_tap(Modifier::Ctrl); // 关闭时修饰键点击无效果
        let (latched, key) = s.key_press(b'C');
        assert!(latched.is_empty() && key == b'C', "关闭时纯透传");
        // 已启用时连按 Shift = 快捷关闭。
        s.enabled = true;
        s.shift_press();
        assert!(!s.enabled, "连按 Shift 关闭（无需确认）");
    }

    #[test]
    fn dismiss_confirm_leaves_disabled() {
        let mut s = StickyKeys::new();
        for _ in 0..STICKY_TOGGLE_PRESSES {
            s.shift_press();
        }
        assert!(s.pending_confirm);
        s.dismiss_confirm();
        assert!(!s.enabled && !s.pending_confirm, "取消确认 = 不启用");
        assert_eq!(s.enable_notices, 0);
    }
}
