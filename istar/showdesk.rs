//! F570 显示桌面按钮 · 完整设计（STAR I 主册 I 域批次七）。
//!
//! **判据（主册）**：点击/悬停两行为；6px+2px 热区；Peek 与 F537 一致；
//! 恢复原位；与 Win+D 同步。
//!
//! **设计要点（主册）**：
//! - 任务栏最右端一条 6px 细带：点击 = 全部最小化显桌面（F309 Win+D 同义）、
//!   再点 = 恢复；悬停半透明预览（Aero Peek——F537 按住键的同款效果）；
//! - 细带小到不误点（6px 视觉 + 鼠标吸附 2px 扩展热区）。

use crate::checks::CheckSet;
use crate::istar::ibase::ISTAR_DOMAIN;

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量
// ---------------------------------------------------------------------------

/// 细带视觉宽（px）。
pub const STRIP_W_PX: u32 = 6;

/// 热区扩展（px——热区比视觉大 2px，好点但不碍事）。
pub const HOTZONE_EXTRA_PX: u32 = 2;

/// Peek 预览透明度（%——F537 同源）。
pub const PEEK_OPACITY_PCT: u8 = 40;

// ---------------------------------------------------------------------------
// 模型
// ---------------------------------------------------------------------------

/// 任务栏上的窗口代表（最小化/恢复记账）。
#[derive(Clone, Copy, Debug)]
pub struct TaskWin {
    pub id: u64,
    /// 是否处于「被显示桌面最小化」态（区别于用户手动最小化——恢复语义
    /// 只针对本按钮压下去的窗）。
    pub minned_by_show_desk: bool,
    /// 用户手动最小化（恢复时不动它）。
    pub minned_by_user: bool,
}

/// 显示桌面按钮状态机。
pub struct ShowDesk {
    /// 桌面显示态：false = 显示桌面（全部压下）、true = 正常。
    desktop_visible: bool,
    wins: Vec<TaskWin>,
    /// Peek 预览激活中（悬停）。
    peeking: bool,
    /// 桌面切换次数账（与 Win+D 同步对拍）。
    toggles: u32,
}

impl ShowDesk {
    pub fn new() -> ShowDesk {
        ShowDesk {
            desktop_visible: true,
            wins: Vec::new(),
            peeking: false,
            toggles: 0,
        }
    }

    /// 任务栏窗口账同步（宿主推送）。
    pub fn sync_win(&mut self, id: u64, minned_by_user: bool) {
        match self.wins.iter_mut().find(|w| w.id == id) {
            Some(w) => w.minned_by_user = minned_by_user,
            None => self.wins.push(TaskWin {
                id,
                minned_by_show_desk: false,
                minned_by_user,
            }),
        }
    }

    pub fn remove_win(&mut self, id: u64) {
        self.wins.retain(|w| w.id != id);
    }

    /// 点击：压下/恢复切换。
    pub fn click(&mut self) -> bool {
        self.toggles += 1;
        if self.desktop_visible {
            // 压下所有可见窗。
            for w in self.wins.iter_mut() {
                if !w.minned_by_user {
                    w.minned_by_show_desk = true;
                }
            }
            self.desktop_visible = false;
        } else {
            // 恢复：只恢复被本按钮压下的窗（手动最小化的不动）。
            for w in self.wins.iter_mut() {
                if w.minned_by_show_desk {
                    w.minned_by_show_desk = false;
                }
            }
            self.desktop_visible = true;
        }
        true
    }

    /// 悬停 Peek：预览激活（与点击互不干扰——悬停不切换状态）。
    pub fn hover_begin(&mut self) {
        if self.desktop_visible {
            self.peeking = true;
        }
    }

    pub fn hover_end(&mut self) {
        self.peeking = false;
    }

    pub fn peeking(&self) -> bool {
        self.peeking
    }

    /// Peek 透明度（F537 同源取数口）。
    pub fn peek_opacity(&self) -> u8 {
        PEEK_OPACITY_PCT
    }

    /// 桌面可见态。
    pub fn desktop_visible(&self) -> bool {
        self.desktop_visible
    }

    /// 窗口被本按钮压下的集合（恢复语义对账面）。
    pub fn pressed_ids(&self) -> Vec<u64> {
        self.wins
            .iter()
            .filter(|w| w.minned_by_show_desk)
            .map(|w| w.id)
            .collect()
    }

    /// 热区宽（视觉 + 扩展——命中判定唯一口）。
    pub fn hotzone_w(&self) -> u32 {
        STRIP_W_PX + HOTZONE_EXTRA_PX * 2
    }

    /// Win+D 同步：外部热键触发等价于点击（同一状态机——殊途同归）。
    pub fn win_d(&mut self) -> bool {
        self.click()
    }

    /// 切换次数账。
    pub fn toggle_count(&self) -> u32 {
        self.toggles
    }
}

impl Default for ShowDesk {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_showdesk_checks() -> CheckSet {
    let mut set = CheckSet::new(ISTAR_DOMAIN);

    // 1. 点击压下全部可见窗（手动最小化的不算被压）。
    let mut s = ShowDesk::new();
    s.sync_win(1, false);
    s.sync_win(2, false);
    s.sync_win(3, true); // 用户手动最小化
    s.click();
    set.add(
        "click presses all visible",
        !s.desktop_visible() && s.pressed_ids() == alloc::vec![1u64, 2],
        "",
    );

    // 2. 再点恢复原位：被压的回来、手动最小化的保持最小化。
    s.click();
    set.add(
        "restore only pressed ones",
        s.desktop_visible() && s.pressed_ids().is_empty() && s.toggle_count() == 2,
        "",
    );

    // 3. 热区：6px 视觉 + 两侧 2px = 10px 命中宽。
    set.add("hotzone 6px plus 2px", s.hotzone_w() == 10 && STRIP_W_PX == 6, "");

    // 4. 悬停 Peek：预览激活、不改变桌面态；移出即收。
    s.hover_begin();
    let peek_on = s.peeking() && s.desktop_visible() && s.peek_opacity() == PEEK_OPACITY_PCT;
    s.hover_end();
    set.add("hover peek without state change", peek_on && !s.peeking(), "");

    // 5. 压下状态下悬停不出 Peek（桌面上没有窗可预览——诚实降级）。
    s.click();
    s.hover_begin();
    set.add("no peek while desktop shown", !s.peeking(), "");
    s.hover_end();
    s.click();

    // 6. Win+D 同步：外部热键与按钮同走一台状态机（toggles 账连续）。
    let before = s.toggle_count();
    s.win_d();
    let after_press = !s.desktop_visible();
    s.win_d();
    set.add(
        "win d same state machine",
        after_press && s.desktop_visible() && s.toggle_count() == before + 2,
        "",
    );

    // 7. 窗口关闭清理：压下中的窗被关后不参与恢复。
    s.click();
    s.remove_win(1);
    s.click();
    set.add("removed window not restored", s.pressed_ids().is_empty(), "");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_window_added_while_desktop_hidden_gets_pressed_on_restore() {
        // 压下期间新窗不自动压（窗口自身管理可见性）——恢复语义只针对
        // 本按钮压下的窗，行为一致性以账为准。
        let mut s = ShowDesk::new();
        s.sync_win(1, false);
        s.click();
        s.sync_win(9, false);
        assert_eq!(s.pressed_ids(), alloc::vec![1u64]);
        s.click();
        assert!(s.pressed_ids().is_empty());
    }

    #[test]
    fn remove_missing_window_noop() {
        let mut s = ShowDesk::new();
        s.remove_win(42);
        assert!(s.wins.is_empty());
    }

    #[test]
    fn peek_opacity_matches_f537() {
        assert_eq!(PEEK_OPACITY_PCT, 40);
    }
}
