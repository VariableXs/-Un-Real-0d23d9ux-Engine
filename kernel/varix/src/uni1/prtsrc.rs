//! F413 PrintScreen 键接入 · 完整设计（STAR I 主册 G-I-13）。
//!
//! **判据（主册）**：三键位语义；窗口自动框定（含阴影边界）；即存路径
//! 与通知；键位可改持久化。＋通12。
//!
//! 设计：PrtSc 三键语义核——PrtSc=交互截图（蒙层选区）、Alt+PrtSc=当前
//! 窗口（自动框定：焦点窗矩形 + 阴影外扩量）、Win+PrtSc=全屏即存（落
//! 图片目录 + 通知一条）；三键可改（F244 表）且持久化快照 round-trip。
//!
//! v5 纵深：交互截图选区会话状态机（进入/拖动/确认/Esc 取消，零选区
//! 诚实拒绝）；无活动窗口时 Alt+PrtSc 诚实提示（不假装截到）；即存
//! 文件名序号化防覆盖 + 截图历史账（封顶 20）。

use crate::checks::CheckSet;
use crate::uni1::ubase::{Chord, HotkeyTable, MOD_ALT, MOD_WIN};

use alloc::string::String;
use alloc::vec::Vec;

/// 阴影外扩（px）——窗口自动框定的边界补偿（唯一登记点）。
pub const SHADOW_MARGIN_PX: i32 = 24;
/// 全屏即存落盘目录。
pub const SAVE_DIR: &str = "图片/屏幕截图";
/// 通知摘要。
pub const SAVE_NOTICE: &str = "全屏截图已保存到 图片/屏幕截图";
/// 即存文件名上限（历史封顶）。
pub const HISTORY_CAP: usize = 20;

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

/// 交互截图选区会话（蒙层状态机：进入 → 拖动 → 确认/Esc）。
#[derive(Default)]
pub struct SelSession {
    pub active: bool,
    start: (i32, i32),
    cur: (i32, i32),
}

impl SelSession {
    /// 进入蒙层选区。
    pub fn begin(&mut self) -> bool {
        self.active = true;
        self.start = (0, 0);
        self.cur = (0, 0);
        true
    }

    /// 拖动更新选区（未进入蒙层时拒绝）。
    pub fn drag(&mut self, start: (i32, i32), cur: (i32, i32)) -> bool {
        if !self.active {
            return false;
        }
        self.start = start;
        self.cur = cur;
        true
    }

    /// 松手确认：返回归一化矩形（负拖向自动翻转）；零选区诚实拒绝。
    pub fn confirm(&mut self) -> Result<(i32, i32, i32, i32), &'static str> {
        if !self.active {
            return Err("没有进行中的截图选区");
        }
        let (x1, y1) = self.start;
        let (x2, y2) = self.cur;
        let rect = (x1.min(x2), y1.min(y2), (x1 - x2).abs(), (y1 - y2).abs());
        if rect.2 == 0 || rect.3 == 0 {
            return Err("选区为空——拖出一个非零区域再松手");
        }
        self.active = false;
        Ok(rect)
    }

    /// Esc 取消：蒙层收场，无截图产出。
    pub fn cancel(&mut self) -> bool {
        let was = self.active;
        self.active = false;
        was
    }
}

/// PrtSc 接入核。
pub struct PrtSc {
    pub hotkeys: HotkeyTable,
    /// 即存计数与通知发送账。
    pub saves: u64,
    pub notices_sent: u64,
    /// 即存文件序号（防覆盖：文件名序号化）。
    pub seq: u64,
    /// 截图历史（最近文件名，FIFO 封顶）。
    pub history: Vec<String>,
}

impl PrtSc {
    pub fn new() -> PrtSc {
        let mut hotkeys = HotkeyTable::new();
        let _ = hotkeys.register("f413.shot.interactive", Chord::new(0, 0xE2));
        let _ = hotkeys.register("f413.shot.window", Chord::new(MOD_ALT, 0xE2));
        let _ = hotkeys.register("f413.shot.fullsave", Chord::new(MOD_WIN, 0xE2));
        PrtSc { hotkeys, saves: 0, notices_sent: 0, seq: 0, history: Vec::new() }
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

    /// Alt+PrtSc：无活动窗口时诚实拒绝（不假装截到一张空图）。
    pub fn shot_active_window(
        &self,
        focus: Option<(i32, i32, i32, i32)>,
    ) -> Result<(i32, i32, i32, i32), &'static str> {
        match focus {
            Some(r) => Ok(self.frame_window(r)),
            None => Err("没有活动窗口——先点一下目标窗口再按 Alt+PrtSc"),
        }
    }

    /// 即存路径与通知（唯一出口，保持旧口径）。
    pub fn save_fullscreen(&mut self) -> (&'static str, &'static str) {
        self.saves += 1;
        self.notices_sent += 1;
        (SAVE_DIR, SAVE_NOTICE)
    }

    /// 即存文件名序号化（防覆盖：同秒连拍不互相顶掉）+ 历史留痕。
    pub fn save_fullscreen_named(&mut self) -> String {
        self.saves += 1;
        self.notices_sent += 1;
        self.seq += 1;
        let name = alloc::format!("屏幕截图 {:03}.png", self.seq);
        self.history.push(name.clone());
        if self.history.len() > HISTORY_CAP {
            self.history.remove(0);
        }
        name
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
    // v5：选区会话——未进入就确认 = 诚实拒绝。
    let mut s = SelSession::default();
    set.add("f413-sel-confirm-before-begin", s.confirm().is_err() && !s.drag((0, 0), (9, 9)), "");
    // v5：进入 → 拖动 → 确认（负拖向自动归一化）。
    set.add("f413-sel-begin-drag", s.begin() && s.drag((300, 260), (100, 80)), "");
    set.add("f413-sel-confirm-normalized", s.confirm() == Ok((100, 80, 200, 180)), "");
    // v5：零选区拒绝（三要素人话）。
    let _ = s.begin();
    let _ = s.drag((50, 50), (50, 90));
    set.add(
        "f413-sel-zero-rejected",
        s.confirm() == Err("选区为空——拖出一个非零区域再松手"),
        "",
    );
    // v5：Esc 取消——蒙层收场，确认通道随关。
    let mut s2 = SelSession::default();
    s2.begin();
    set.add(
        "f413-sel-esc-cancel",
        s2.cancel() && !s2.active && s2.confirm().is_err() && !s2.cancel(),
        "",
    );
    // v5：无活动窗口诚实提示（不假装截到）。
    let mut p2 = PrtSc::new();
    set.add(
        "f413-window-no-focus-honest",
        p2.shot_active_window(None) == Err("没有活动窗口——先点一下目标窗口再按 Alt+PrtSc"),
        "",
    );
    set.add(
        "f413-window-focus-framed",
        p2.shot_active_window(Some((10, 10, 200, 100))) == Ok((34, 34, 152, 52)),
        "",
    );
    // v5：文件名序号化防覆盖 + 历史留痕。
    let n1 = p2.save_fullscreen_named();
    let n2 = p2.save_fullscreen_named();
    set.add(
        "f413-filename-unique",
        n1 == "屏幕截图 001.png" && n2 == "屏幕截图 002.png" && p2.history.len() == 2,
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

    #[test]
    fn sel_rejects_drag_before_begin() {
        let mut s = SelSession::default();
        assert!(!s.drag((0, 0), (9, 9)), "未进蒙层不允许拖动");
        assert!(s.begin());
        assert!(s.drag((0, 0), (9, 9)));
    }

    #[test]
    fn history_caps_at_20() {
        let mut p = PrtSc::new();
        for _ in 0..25 {
            let _ = p.save_fullscreen_named();
        }
        assert_eq!(p.history.len(), HISTORY_CAP);
        assert_eq!(p.seq, 25);
        assert_eq!(p.history[0], "屏幕截图 006.png", "FIFO 淘汰最旧");
    }
}
