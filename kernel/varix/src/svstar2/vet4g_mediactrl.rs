#![allow(dead_code)]

extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码段（0xD900）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0xD900;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CD9Code(pub u16);

impl CD9Code {
    pub const CTRL_UNREACHABLE: Self = Self(SEG_BASE);  // 控制不可达
    pub const STATE_AMBIGUOUS: Self = Self(SEG_BASE + 1); // 状态不可辨
    pub const GESTURE_NOEQUIV: Self = Self(SEG_BASE + 2); // 手势无等价
    pub const FEEDBACK_MISSING: Self = Self(SEG_BASE + 3); // 反馈缺失
    pub const KEYBOARD_TRAP: Self = Self(SEG_BASE + 4);    // 键盘陷阱

    pub fn code(self) -> u16 { self.0 }
    pub fn name(self) -> &'static str {
        match self {
            Self::CTRL_UNREACHABLE => "控制不可达",
            Self::STATE_AMBIGUOUS => "状态不可辨",
            Self::GESTURE_NOEQUIV => "手势无等价",
            Self::FEEDBACK_MISSING => "反馈缺失",
            Self::KEYBOARD_TRAP => "键盘陷阱",
            _ => "未知",
        }
    }
    pub const ALL: [Self; 5] = [
        Self::CTRL_UNREACHABLE, Self::STATE_AMBIGUOUS, Self::GESTURE_NOEQUIV,
        Self::FEEDBACK_MISSING, Self::KEYBOARD_TRAP,
    ];
}

// ── 媒体播放状态 ────────────────────────────────────────────────────

/// 媒体播放态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackState { Playing, Paused, Stopped, Buffering, Error }

impl PlaybackState {
    /// 状态可辨：Playing vs Paused 渲染必须区分。
    pub fn discernible_pair(a: Self, b: Self) -> bool {
        match (a, b) {
            (Self::Playing, Self::Paused) | (Self::Paused, Self::Playing) => true,
            _ => a != b,
        }
    }
}

// ── 控制项 ──────────────────────────────────────────────────────────

/// 媒体控制项。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediaControl {
    pub id: u32,
    pub kind: ControlKind,
    /// 键盘/辅具可达。
    pub keyboard_reachable: bool,
    /// 反馈已渲染（复用 F3014）。
    pub feedback_rendered: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlKind { Play, Pause, Seek, Volume, Mute, Captions, Speed }

impl ControlKind {
    pub const ALL: [Self; 7] = [
        Self::Play, Self::Pause, Self::Seek, Self::Volume, Self::Mute, Self::Captions, Self::Speed,
    ];
}

impl MediaControl {
    /// 控制可达断言。
    pub fn reachable_ok(&self) -> bool { self.keyboard_reachable }
    /// 反馈完整。
    pub fn feedback_ok(&self) -> bool { self.feedback_rendered }
}

// ── 手势与键盘等价 ──────────────────────────────────────────────────

/// 手势动作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GestureAction { SwipeLeft, SwipeRight, SwipeUp, SwipeDown, DoubleTap, LongPress }

impl GestureAction {
    /// 键盘等价映射。
    pub fn keyboard_equiv(self) -> KeyCode {
        match self {
            Self::SwipeLeft => KeyCode::ArrowLeft,
            Self::SwipeRight => KeyCode::ArrowRight,
            Self::SwipeUp => KeyCode::ArrowUp,
            Self::SwipeDown => KeyCode::ArrowDown,
            Self::DoubleTap => KeyCode::Enter,
            Self::LongPress => KeyCode::Space,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyCode { ArrowLeft, ArrowRight, ArrowUp, ArrowDown, Enter, Space, Escape }

/// 手势等价注册表。
#[derive(Debug, Clone)]
pub struct GestureEquivMap {
    pub entries: Vec<(GestureAction, KeyCode)>,
}

impl GestureEquivMap {
    pub fn new() -> Self {
        let mut e = Vec::new();
        for g in [
            GestureAction::SwipeLeft, GestureAction::SwipeRight, GestureAction::SwipeUp,
            GestureAction::SwipeDown, GestureAction::DoubleTap, GestureAction::LongPress,
        ] {
            e.push((g, g.keyboard_equiv()));
        }
        Self { entries: e }
    }

    /// 全部手势有键盘等价。
    pub fn complete(&self) -> bool {
        self.entries.len() == 6 && self.entries.iter().all(|(g, k)| g.keyboard_equiv() == *k)
    }

    /// 查映射。
    pub fn lookup(&self, g: GestureAction) -> Option<KeyCode> {
        self.entries.iter().find(|(gg, _)| *gg == g).map(|(_, k)| *k)
    }
}

// ── 媒体控制面板 ────────────────────────────────────────────────────

/// 媒体控制无障碍面板。
#[derive(Debug, Clone)]
pub struct MediaControlPanel {
    pub controls: Vec<MediaControl>,
    pub state: PlaybackState,
    pub gesture_map: GestureEquivMap,
}

impl MediaControlPanel {
    pub fn new() -> Self {
        Self {
            controls: Vec::new(),
            state: PlaybackState::Stopped,
            gesture_map: GestureEquivMap::new(),
        }
    }

    /// 控制可达审计：全部控制键盘可达。
    pub fn all_reachable(&self) -> bool {
        !self.controls.is_empty() && self.controls.iter().all(|c| c.reachable_ok())
    }

    /// 反馈审计。
    pub fn all_feedback(&self) -> bool {
        self.controls.iter().all(|c| c.feedback_ok())
    }

    /// 手势等价审计。
    pub fn gesture_equiv_ok(&self) -> bool {
        self.gesture_map.complete()
    }

    /// 综合审计。
    pub fn audit(&self) -> CtrlAudit {
        CtrlAudit {
            reachable: self.all_reachable(),
            feedback: self.all_feedback(),
            gesture: self.gesture_equiv_ok(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CtrlAudit {
    pub reachable: bool,
    pub feedback: bool,
    pub gesture: bool,
}
impl CtrlAudit {
    pub fn all_green(&self) -> bool { self.reachable && self.feedback && self.gesture }
}

// ── 判据集 ──────────────────────────────────────────────────────────

pub fn run_f3869_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("vet4g_mediactrl");
    fn chk(set: &mut crate::checks::CheckSet, label: &'static str, cond: bool, note: &'static str) {
        set.add(label, cond, note);
    }

    // 1 状态可辨
    {
        chk(&mut set, "C3869-01", PlaybackState::discernible_pair(PlaybackState::Playing, PlaybackState::Paused), "err");
    }
    // 2 同状态不可辨
    {
        chk(&mut set, "C3869-02", !PlaybackState::discernible_pair(PlaybackState::Playing, PlaybackState::Playing), "err");
    }
    // 3 控制可达
    {
        let c = MediaControl { id: 1, kind: ControlKind::Play, keyboard_reachable: true, feedback_rendered: true };
        chk(&mut set, "C3869-03", c.reachable_ok(), "err");
    }
    // 4 控制不可达
    {
        let c = MediaControl { id: 1, kind: ControlKind::Play, keyboard_reachable: false, feedback_rendered: true };
        chk(&mut set, "C3869-04", !c.reachable_ok(), "err");
    }
    // 5 反馈完整
    {
        let c = MediaControl { id: 1, kind: ControlKind::Volume, keyboard_reachable: true, feedback_rendered: true };
        chk(&mut set, "C3869-05", c.feedback_ok(), "err");
    }
    // 6 手势等价（SwipeLeft→ArrowLeft）
    {
        chk(&mut set, "C3869-06", GestureAction::SwipeLeft.keyboard_equiv() == KeyCode::ArrowLeft, "err");
    }
    // 7 手势等价（DoubleTap→Enter）
    {
        chk(&mut set, "C3869-07", GestureAction::DoubleTap.keyboard_equiv() == KeyCode::Enter, "err");
    }
    // 8 等价映射完备
    {
        let m = GestureEquivMap::new();
        chk(&mut set, "C3869-08", m.complete(), "err");
    }
    // 9 映射查表
    {
        let m = GestureEquivMap::new();
        chk(&mut set, "C3869-09", m.lookup(GestureAction::LongPress) == Some(KeyCode::Space), "err");
    }
    // 10 面板可达
    {
        let mut p = MediaControlPanel::new();
        p.controls.push(MediaControl { id: 1, kind: ControlKind::Play, keyboard_reachable: true, feedback_rendered: true });
        chk(&mut set, "C3869-10", p.all_reachable(), "err");
    }
    // 11 面板不可达
    {
        let mut p = MediaControlPanel::new();
        p.controls.push(MediaControl { id: 1, kind: ControlKind::Play, keyboard_reachable: false, feedback_rendered: true });
        chk(&mut set, "C3869-11", !p.all_reachable(), "err");
    }
    // 12 控制种类齐全
    {
        chk(&mut set, "C3869-12", ControlKind::ALL.len() == 7, "err");
    }
    // 13 面板审计全绿
    {
        let mut p = MediaControlPanel::new();
        for k in ControlKind::ALL {
            p.controls.push(MediaControl { id: 1, kind: k, keyboard_reachable: true, feedback_rendered: true });
        }
        chk(&mut set, "C3869-13", p.audit().all_green(), "err");
    }
    // 14 面板反馈缺失
    {
        let mut p = MediaControlPanel::new();
        p.controls.push(MediaControl { id: 1, kind: ControlKind::Play, keyboard_reachable: true, feedback_rendered: false });
        chk(&mut set, "C3869-14", !p.audit().all_green(), "err");
    }
    // 15 空面板不可达
    {
        let p = MediaControlPanel::new();
        chk(&mut set, "C3869-15", !p.all_reachable(), "err");
    }
    // 16 键盘码互异
    {
        chk(&mut set, "C3869-16", KeyCode::ArrowLeft != KeyCode::ArrowRight, "err");
    }
    // 17 Buffering 态可辨
    {
        chk(&mut set, "C3869-17", PlaybackState::discernible_pair(PlaybackState::Buffering, PlaybackState::Playing), "err");
    }
    // 18 手势枚举六项
    {
        let m = GestureEquivMap::new();
        chk(&mut set, "C3869-18", m.entries.len() == 6, "err");
    }

    set
}
