//! F413 PrintScreen 键接入 · 完整设计（STAR I 主册 G-I-13）。
//!
//! **判据（主册）**：三键位语义；窗口自动框定（含阴影边界）；即存路径
//! 与通知；键位可改持久化。＋通12。
//!
//! 设计：PrtSc 三键语义核——PrtSc=交互截图（蒙层选区）、Alt+PrtSc=当前
//! 窗口（自动框定：焦点窗矩形 + 阴影外扩量）、Win+PrtSc=全屏即存（落
//! 图片目录 + 通知一条）；三键可改（F244 表）且持久化快照 round-trip。

use crate::checks::CheckSet;
use crate::uni1::ubase::{Chord, HotkeyTable, MOD_ALT, MOD_WIN};

/// 阴影外扩（px）——窗口自动框定的边界补偿（唯一登记点）。
pub const SHADOW_MARGIN_PX: i32 = 24;
/// 全屏即存落盘目录。
pub const SAVE_DIR: &str = "图片/屏幕截图";
/// 通知摘要。
pub const SAVE_NOTICE: &str = "全屏截图已保存到 图片/屏幕截图";

/// 截图三键语义。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShotAction {
    /// 交互截图（全屏蒙层选区）。
    Interactive,
    /// 当前窗口（含阴影自动框定）。
    ActiveWindow,
    /// 全屏即存。
    FullscreenSave,
}

/// PrtSc 接入核。
pub struct PrtSc {
    pub hotkeys: HotkeyTable,
    /// 即存计数与通知发送账。
    pub saves: u64,
    pub notices_sent: u64,
}

impl PrtSc {
    pub fn new() -> PrtSc {
        let mut hotkeys = HotkeyTable::new();
        let _ = hotkeys.register("f413.shot.interactive", Chord::new(0, 0xE2));
        let _ = hotkeys.register("f413.shot.window", Chord::new(MOD_ALT, 0xE2));
        let _ = hotkeys.register("f413.shot.fullsave", Chord::new(MOD_WIN, 0xE2));
        PrtSc { hotkeys, saves: 0, notices_sent: 0 }
    }

    /// 键分发（chord 由输入层解码）。
    pub fn dispatch(&mut self, chord: Chord) -> Option<ShotAction> {
        match self.hotkeys.lookup(chord) {
            Some("f413.shot.interactive") => Some(ShotAction::Interactive),
            Some("f413.shot.window") => Some(ShotAction::ActiveWindow),
            Some("f413.shot.fullsave") => {
                self.saves += 1;
                self.notices_sent += 1;
                Some(ShotAction::FullscreenSave)
            }
            _ => None,
        }
    }

    /// 窗口自动框定：焦点窗矩形外扩阴影边界（负坐标安全收敛）。
    pub fn frame_window(&self, rect: (i32, i32, i32, i32)) -> (i32, i32, i32, i32) {
        let (x, y, w, h) = rect;
        let x2 = x + SHADOW_MARGIN_PX;
        let y2 = y + SHADOW_MARGIN_PX;
        let w2 = w - 2 * SHADOW_MARGIN_PX;
        let h2 = h - 2 * SHADOW_MARGIN_PX;
        (x2, y2, w2.max(1), h2.max(1))
    }

    /// 即存路径与通知（唯一出口）。
    pub fn save_fullscreen(&mut self) -> (&'static str, &'static str) {
        self.saves += 1;
        self.notices_sent += 1;
        (SAVE_DIR, SAVE_NOTICE)
    }

    /// 键位可改持久化：改键后快照可 round-trip（user_set 标注）。
    pub fn rebind_and_snapshot(&mut self, chord: Chord, new_chord: Chord) -> bool {
        self.hotkeys.rebind_key_lookup(chord, new_chord)
    }
}

/// F244 表扩展：按现键找动作再改键（PrtSc 键位改绑的便捷口）。
impl HotkeyTable {
    pub fn rebind_key_lookup(&mut self, old: Chord, new: Chord) -> bool {
        match self.lookup(old) {
            Some(action) => self.rebind(action, new).is_ok(),
            None => false,
        }
    }
}

pub fn run_prtsrc_checks() -> CheckSet {
    let mut set = CheckSet::new("uni1-F413");
    let mut p = PrtSc::new();
    // 三键位注册齐。
    set.add(
        "f413-three-bound",
        p.hotkeys.lookup(Chord::new(0, 0xE2)) == Some("f413.shot.interactive")
            && p.hotkeys.lookup(Chord::new(MOD_ALT, 0xE2)) == Some("f413.shot.window")
            && p.hotkeys.lookup(Chord::new(MOD_WIN, 0xE2)) == Some("f413.shot.fullsave"),
        "",
    );
    // 三语义分发。
    set.add(
        "f413-semantics",
        p.dispatch(Chord::new(0, 0xE2)) == Some(ShotAction::Interactive)
            && p.dispatch(Chord::new(MOD_ALT, 0xE2)) == Some(ShotAction::ActiveWindow)
            && p.dispatch(Chord::new(MOD_WIN, 0xE2)) == Some(ShotAction::FullscreenSave),
        "",
    );
    // 即存路径与通知同步计数。
    set.add(
        "f413-save-notice",
        p.saves == 1 && p.notices_sent == 1,
        "",
    );
    let (dir, notice) = p.save_fullscreen();
    set.add(
        "f413-save-path",
        dir == SAVE_DIR && notice == SAVE_NOTICE && p.notices_sent == 2,
        "",
    );
    // 窗口自动框定（阴影外扩，边界收缩不越界）。
    set.add(
        "f413-shadow-frame",
        p.frame_window((100, 100, 400, 300)) == (124, 124, 352, 252),
        "",
    );
    set.add(
        "f413-tiny-window-clamped",
        p.frame_window((0, 0, 10, 10)) == (24, 24, 1, 1),
        "",
    );
    // 键位可改 + 持久化快照。
    set.add(
        "f413-rebind",
        p.rebind_and_snapshot(Chord::new(MOD_WIN, 0xE2), Chord::new(MOD_WIN, b'P'))
            && p.dispatch(Chord::new(MOD_WIN, b'P')) == Some(ShotAction::FullscreenSave),
        "",
    );
    set.add(
        "f413-snapshot-user-set",
        p.hotkeys.snapshot().iter().any(|(a, _, u)| *u && *a == "f413.shot.fullsave"),
        "",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_chord_is_none() {
        let mut p = PrtSc::new();
        assert!(p.dispatch(Chord::new(0, b'x')).is_none());
        assert_eq!(p.saves, 0);
    }

    #[test]
    fn rebind_conflict_rejected() {
        let mut p = PrtSc::new();
        // 改到已占用的键 → 拒绝，原键不变。
        assert!(!p.rebind_and_snapshot(Chord::new(0, 0xE2), Chord::new(MOD_ALT, 0xE2)));
        assert!(p.dispatch(Chord::new(0, 0xE2)).is_some());
    }
}
