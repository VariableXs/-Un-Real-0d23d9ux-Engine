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

/// 回弹键默认再武装窗（ms）：同键在此窗内的重复按下被抑制
/// （手抖连击用户的救星——异键不受影响）。
pub const BOUNCE_REARM_MS: u64 = 200;

/// 粘滞键锁存默认超时（ms）：锁存后无后续键自动清锁（Windows 同款
/// 可选项——忘了关修饰键不至于污染后续输入）。None = 不超时。
pub const STICKY_LATCH_TIMEOUT_MS: u64 = 5_000;

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
    /// 锁存超时（v7 深化）：Some = 锁存后 N ms 无后续键自动清锁。
    pub latch_timeout_ms: Option<u64>,
    /// 最近一次锁存时刻（超时判定基准；None = 当前无锁存计时）。
    last_latch_at_ms: Option<u64>,
    /// 超时自动清锁次数（体验账）。
    pub latch_timeouts: u64,
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
            latch_timeout_ms: Some(STICKY_LATCH_TIMEOUT_MS),
            last_latch_at_ms: None,
            latch_timeouts: 0,
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

    /// 修饰键按下+松开（粘滞语义：松开时锁存）。无时刻注入——锁存
    /// 不参与超时判定（老调用方零意外）。
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

    /// 带时刻的修饰键锁存（v7 深化）：首次锁存登记时刻，参与超时判定。
    pub fn modifier_tap_at(&mut self, m: Modifier, now_ms: u64) {
        self.modifier_tap(m);
        if !self.latched.is_empty() && self.last_latch_at_ms.is_none() {
            self.last_latch_at_ms = Some(now_ms);
        } else if self.latched.is_empty() {
            self.last_latch_at_ms = None;
        }
    }

    /// 实体键到达：合成组合键 = 锁存修饰键 + 该键；随后锁存清空
    /// （一次合成后修饰键回弹——逐键等效的落点）。
    pub fn key_press(&mut self, key: u8) -> (Vec<Modifier>, u8) {
        let combo = (self.latched.clone(), key);
        self.latched.clear();
        self.last_latch_at_ms = None;
        combo
    }

    /// 锁存超时判定（v7 深化）：带时刻锁存后超过超时窗且期间无实体键
    /// 到达 → 自动清锁（计数留痕）。返回本次清掉的修饰键数。
    pub fn expire_latch(&mut self, now_ms: u64) -> usize {
        let timeout = match self.latch_timeout_ms {
            Some(t) => t,
            None => return 0,
        };
        if let Some(t0) = self.last_latch_at_ms {
            if now_ms.saturating_sub(t0) > timeout && !self.latched.is_empty() {
                let n = self.latched.len();
                self.latched.clear();
                self.last_latch_at_ms = None;
                self.latch_timeouts += 1;
                return n;
            }
        }
        0
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

// ---------------------------------------------------------------------------
// v7 深化：回弹键（同键连击抑制）
// ---------------------------------------------------------------------------

/// 回弹键状态机：同键在再武装窗内的重复按下被抑制（手抖连击救星）；
/// 异键立即放行（打 "ab" 不受 "a" 连击抑制影响）；窗口过后同键恢复。
pub struct BounceKeys {
    pub enabled: bool,
    /// 再武装窗（ms，可调——无障碍参数入册纪律）。
    pub rearm_ms: u64,
    /// 各键最近按下时刻（小表——键盘键数有限，线性扫足够）。
    last_at: Vec<(u8, u64)>,
    /// 抑制的连击数（诊断面）。
    pub suppressed: u64,
}

impl BounceKeys {
    pub fn new(rearm_ms: u64) -> BounceKeys {
        BounceKeys { enabled: false, rearm_ms: rearm_ms.max(1), last_at: Vec::new(), suppressed: 0 }
    }

    /// 按键判定：开启时同键窗内重复 → 抑制（false）；异键/窗外 → 放行
    /// 并登记时刻。关闭时全通。
    pub fn press(&mut self, key: u8, now_ms: u64) -> bool {
        if !self.enabled {
            return true;
        }
        if let Some((_, t0)) = self.last_at.iter().find(|(k, _)| *k == key) {
            if now_ms.saturating_sub(*t0) <= self.rearm_ms {
                self.suppressed += 1;
                return false;
            }
        }
        match self.last_at.iter_mut().find(|(k, _)| *k == key) {
            Some(e) => e.1 = now_ms,
            None => self.last_at.push((key, now_ms)),
        }
        true
    }
}

// ---------------------------------------------------------------------------
// v7 深化：提示音事件账（无障碍反馈的可观测面）
// ---------------------------------------------------------------------------

/// 一条提示音事件：时刻 + 类别（accept/reject/latch/timeout）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BeepEvent {
    pub at_ms: u64,
    pub kind: &'static str,
}

/// 提示音事件账：接受/拒绝/锁存/超时清锁各有其声（无障碍用户靠听觉
/// 确认状态——反馈不只走视觉）；静音 = 事件记为 skip（诚实计数，
/// 不当无事发生）。环容量 32，满则淘汰最旧。
pub struct SoundLedger {
    pub muted: bool,
    events: alloc::collections::vec_deque::VecDeque<BeepEvent>,
    pub muted_skips: u64,
}

impl SoundLedger {
    pub fn new() -> SoundLedger {
        SoundLedger { muted: false, events: alloc::collections::vec_deque::VecDeque::new(), muted_skips: 0 }
    }

    /// 记一条提示音。静音期间 → muted_skips 计数（不进环——回放的是
    /// 「真实响过的」）。
    pub fn record(&mut self, at_ms: u64, kind: &'static str) {
        if self.muted {
            self.muted_skips += 1;
            return;
        }
        if self.events.len() == 32 {
            self.events.pop_front();
        }
        self.events.push_back(BeepEvent { at_ms, kind });
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// 时间序快照。
    pub fn snapshot(&self) -> Vec<BeepEvent> {
        self.events.iter().copied().collect()
    }
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

    // ---- v7 深化：回弹键 / 提示音事件账 / 锁存超时 ----

    // 回弹键：同键窗内连击抑制、异键放行、窗外恢复。
    let mut b = BounceKeys::new(BOUNCE_REARM_MS);
    b.enabled = true;
    set.add(
        "f450-bounce-same-key-suppressed",
        b.press(b'A', 1_000) && !b.press(b'A', 1_050) && !b.press(b'A', 1_150) && b.suppressed == 2,
        "",
    );
    set.add(
        "f450-bounce-other-key-passes",
        b.press(b'B', 1_060) && b.press(b'C', 1_070),
        "",
    );
    set.add(
        "f450-bounce-rearm-after-window",
        b.press(b'A', 1_250) && b.press(b'A', 1_500),
        "",
    );
    let mut off = BounceKeys::new(BOUNCE_REARM_MS);
    set.add(
        "f450-bounce-disabled-passthrough",
        off.press(b'A', 1) && off.press(b'A', 1) && off.suppressed == 0,
        "",
    );

    // 再武装窗可调（参数入册纪律——改值行为跟随）。
    b.rearm_ms = 500;
    set.add("f450-bounce-rearm-adjustable", !b.press(b'B', 1_400) && b.press(b'B', 1_950), "");

    // 提示音事件账：响过的进环可回放；静音期记 skip（不当无事发生）。
    let mut snd = SoundLedger::new();
    snd.record(10, "latch");
    snd.record(20, "accept");
    snd.muted = true;
    snd.record(30, "reject");
    snd.muted = false;
    snd.record(40, "accept");
    let beep_snap = snd.snapshot();
    set.add(
        "f450-sound-ledger",
        snd.len() == 3 && snd.muted_skips == 1 && beep_snap[0].kind == "latch" && beep_snap[2].kind == "accept",
        "",
    );

    // 环容量 32：满则淘汰最旧（可回放最近）。
    let mut full = SoundLedger::new();
    for i in 0..40u64 {
        full.record(i * 10, "accept");
    }
    let f_snap = full.snapshot();
    set.add(
        "f450-sound-ring-evicts",
        full.len() == 32 && f_snap[0].at_ms == 80 && full.snapshot()[31].at_ms == 390,
        "",
    );

    // 锁存超时：带时刻锁存 → 超窗自动清锁（计数留痕）；窗内不误清。
    let mut t = StickyKeys::new();
    t.enabled = true;
    t.modifier_tap_at(Modifier::Ctrl, 1_000);
    set.add(
        "f450-latch-no-expiry-in-window",
        t.expire_latch(4_000) == 0 && t.latched.len() == 1,
        "",
    );
    set.add(
        "f450-latch-expires-after-window",
        t.expire_latch(6_100) == 1 && t.latched.is_empty() && t.latch_timeouts == 1,
        "",
    );
    // 实体键到达复位计时（合成后清锁本来就走 key_press）。
    let mut t2 = StickyKeys::new();
    t2.enabled = true;
    t2.modifier_tap_at(Modifier::Shift, 1_000);
    let (latched, key) = t2.key_press(b'S');
    set.add(
        "f450-latch-reset-on-keypress",
        latched == alloc::vec![Modifier::Shift] && key == b'S' && t2.expire_latch(99_999) == 0,
        "",
    );
    // 超时可关（None = 永不超时——用户意愿优先）。
    let mut t3 = StickyKeys::new();
    t3.enabled = true;
    t3.latch_timeout_ms = None;
    t3.modifier_tap_at(Modifier::Alt, 1_000);
    set.add("f450-latch-timeout-disableable", t3.expire_latch(99_999_999) == 0 && t3.latched.len() == 1, "");

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

    // ---- v7 深化单测 ----

    #[test]
    fn bounce_keys_full_matrix() {
        let mut b = BounceKeys::new(200);
        b.enabled = true;
        // 各键窗口独立：B 的穿插不延长/缩短 A 的窗口（互不干扰）。
        assert!(b.press(b'A', 0));
        assert!(b.press(b'B', 10));
        assert!(!b.press(b'A', 20), "同键窗内穿插异键仍抑制本键");
        assert!(!b.press(b'B', 150), "B 自己的窗同样生效");
        // 窗口边界：恰达线仍抑制（≤），线外 1ms 放行并重起窗口。
        assert!(!b.press(b'A', 200), "恰达再武装线仍算连击");
        assert!(b.press(b'A', 201));
        assert!(!b.press(b'A', 300), "重起后的窗内再现连击");
        // 关闭后全通，抑制账保留（诊断面不清零）。
        b.enabled = false;
        assert!(b.press(b'A', 310));
        assert_eq!(b.suppressed, 4);
    }

    #[test]
    fn sound_ledger_mute_resumes_honestly() {
        let mut s = SoundLedger::new();
        s.record(0, "latch");
        s.muted = true;
        for i in 0..5u64 {
            s.record(100 + i, "reject");
        }
        assert_eq!(s.len(), 1, "静音期不进环");
        assert_eq!(s.muted_skips, 5);
        s.muted = false;
        s.record(999, "accept");
        assert_eq!(s.len(), 2, "解除静音即恢复记录");
        // 空账诚实：is_empty 对拍。
        let mut e = SoundLedger::new();
        assert!(e.is_empty());
        e.muted = true;
        e.record(1, "accept");
        assert!(e.is_empty() && e.muted_skips == 1);
    }

    #[test]
    fn latch_timeout_full_cycle() {
        let mut s = StickyKeys::new();
        s.enabled = true;
        // 带时刻锁存两键 → 超时一次清两键。
        s.modifier_tap_at(Modifier::Ctrl, 0);
        s.modifier_tap_at(Modifier::Shift, 100);
        assert_eq!(s.latched.len(), 2);
        assert_eq!(s.expire_latch(STICKY_LATCH_TIMEOUT_MS), 0, "恰达线不清（> 判定）");
        assert_eq!(s.expire_latch(STICKY_LATCH_TIMEOUT_MS + 1), 2);
        assert_eq!(s.latch_timeouts, 1);
        // 清锁后可再锁（超时窗从新锁存时刻起算）。
        s.modifier_tap_at(Modifier::Alt, 10_000);
        assert_eq!(s.expire_latch(12_000), 0, "新窗内不误清");
        assert_eq!(s.expire_latch(15_001), 1);
        // 老口（无时刻）不参与超时：锁存挂着永不超时。
        let mut legacy = StickyKeys::new();
        legacy.enabled = true;
        legacy.modifier_tap(Modifier::Win);
        assert_eq!(legacy.expire_latch(99_999_999), 0);
        assert_eq!(legacy.latched.len(), 1);
    }
}
