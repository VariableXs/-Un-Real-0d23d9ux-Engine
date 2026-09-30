//! F585 焦点跟随鼠标（可选） · 完整设计（STAR I 主册 I 域批次八）。
//!
//! **判据（主册）**：跟随触发时序；透传点击判据（首击生效）；浮层豁免
//! 清单；默认关；与既有焦点链不冲突。
//!
//! **设计要点（主册）**：
//! - 进阶选项（默认关）：焦点跟随鼠标（鼠标进哪个窗口哪个窗自动获焦
//!   ——X11 党的习惯）；
//! - 开启后点击仍是点击（不会「先点聚焦再点生效」的二次点击——首次
//!   点击直接透传动作）；
//! - 输入法/托盘浮层豁免（不因划过抢焦点）。

use crate::checks::CheckSet;
use crate::istar::ibase::ISTAR_DOMAIN;

use alloc::vec::Vec;

/// 跟随触发驻留时间（ms——进入窗口后需驻留此时长才转移焦点，防扫过抢焦）。
pub const DWELL_MS: u64 = 120;

/// 浮层豁免清单（枚举即清单——划过不抢焦的窗口类）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExemptKind {
    /// 输入法候选浮窗。
    ImeCandidate,
    /// 托盘弹出面板。
    TrayFlyout,
    /// 通知横幅。
    ToastBanner,
}

pub const EXEMPT_KINDS: [ExemptKind; 3] = [
    ExemptKind::ImeCandidate,
    ExemptKind::TrayFlyout,
    ExemptKind::ToastBanner,
];

/// 焦点跟随鼠标引擎（默认关）。
pub struct FocusFollow {
    enabled: bool,
    /// 窗口几何（id → (x, y, w, h)）。
    wins: Vec<(u64, i32, i32, u32, u32)>,
    /// 浮层豁免账（正在显示的浮层类）。
    overlays: Vec<ExemptKind>,
    /// 当前焦点窗。
    focus: Option<u64>,
    /// 悬停驻留态（窗 id, 进入时刻）。
    dwell: Option<(u64, u64)>,
    now_ms: u64,
    /// 首击透传账（透传动作次数——判据对账）。
    pass_through: u32,
}

impl FocusFollow {
    pub fn new() -> FocusFollow {
        FocusFollow {
            enabled: false, // 默认关——判据钉死
            wins: Vec::new(),
            overlays: Vec::new(),
            focus: None,
            dwell: None,
            now_ms: 0,
            pass_through: 0,
        }
    }

    /// 开关（默认关——开启才生效）。
    pub fn set_enabled(&mut self, on: bool) {
        self.enabled = on;
        if !on {
            self.dwell = None;
        }
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// 窗口几何同步。
    pub fn sync_win(&mut self, id: u64, x: i32, y: i32, w: u32, h: u32) {
        match self.wins.iter_mut().find(|(i, ..)| *i == id) {
            Some(slot) => *slot = (id, x, y, w, h),
            None => self.wins.push((id, x, y, w, h)),
        }
    }

    /// 浮层显示/关闭（豁免窗口在显期间不跟焦）。
    pub fn overlay(&mut self, kind: ExemptKind, shown: bool) {
        if shown {
            if !self.overlays.contains(&kind) {
                self.overlays.push(kind);
            }
        } else {
            self.overlays.retain(|k| *k != kind);
        }
    }

    fn hit(&self, x: i32, y: i32) -> Option<u64> {
        self.wins
            .iter()
            .rev() // 顶层优先
            .find(|(_, wx, wy, w, h)| x >= *wx && y >= *wy && (x - *wx) < *w as i32 && (y - *wy) < *h as i32)
            .map(|(id, ..)| *id)
    }

    /// 鼠标移动：驻留时序判定（进入窗后驻留 DWELL_MS 才转焦点）。
    /// 返回新焦点窗（若有转移）。
    pub fn mouse_move(&mut self, x: i32, y: i32, ms: u64) -> Option<u64> {
        self.now_ms = ms;
        if !self.enabled {
            return None;
        }
        if !self.overlays.is_empty() {
            return None; // 浮层豁免：任何浮层在显期间不跟焦。
        }
        let id = self.hit(x, y)?;
        match self.dwell {
            Some((same, since)) if same == id => {
                if ms.saturating_sub(since) >= DWELL_MS && self.focus != Some(id) {
                    self.focus = Some(id);
                    self.dwell = None;
                    return Some(id);
                }
                None
            }
            _ => {
                self.dwell = Some((id, ms));
                None
            }
        }
    }

    /// 首击透传：点击命中窗即执行动作（不在跟随引擎内吞点击——
    /// 焦点转移与动作执行互不阻塞）。返回命中窗。
    pub fn click(&mut self, x: i32, y: i32) -> Option<u64> {
        let id = self.hit(x, y)?;
        self.focus = Some(id); // 点击即焦点（与跟随无冲突——既有焦点链同款）
        self.pass_through += 1;
        Some(id)
    }

    pub fn focus(&self) -> Option<u64> {
        self.focus
    }

    pub fn pass_through_count(&self) -> u32 {
        self.pass_through
    }
}

impl Default for FocusFollow {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_focusfollow_checks() -> CheckSet {
    let mut set = CheckSet::new(ISTAR_DOMAIN);

    // 1. 默认关：不开启时鼠标移动零焦点转移（Windows 习惯党零感知）。
    let mut f = FocusFollow::new();
    f.sync_win(1, 0, 0, 800, 600);
    f.sync_win(2, 800, 0, 800, 600);
    let off = f.enabled() == false && f.mouse_move(900, 300, 1_000).is_none();
    set.add("default off zero transfers", off, "");

    // 2. 跟随触发时序：驻留 119ms 不转、120ms 转（防扫过抢焦）。
    f.set_enabled(true);
    f.mouse_move(100, 100, 2_000);
    let early = f.mouse_move(100, 100, 2_119).is_none();
    let on_time = f.mouse_move(100, 100, 2_120) == Some(1);
    set.add("dwell 120ms timing", early && on_time && f.focus() == Some(1), "");

    // 3. 移到第二窗：重新驻留再转。
    f.mouse_move(900, 300, 3_000);
    let early2 = f.mouse_move(900, 300, 3_100).is_none();
    let later = f.mouse_move(900, 300, 3_121);
    set.add(
        "second window dwell again",
        early2 && later == Some(2) && f.focus() == Some(2),
        "",
    );

    // 4. 首击透传：点击直接动作（不因焦点转移吞首击）。
    let hit = f.click(900, 300);
    set.add(
        "first click passes through",
        hit == Some(2) && f.pass_through_count() == 1 && f.focus() == Some(2),
        "",
    );

    // 5. 浮层豁免：输入法候选浮窗在显期间划过不抢焦。
    let mut f2 = FocusFollow::new();
    f2.set_enabled(true);
    f2.sync_win(1, 0, 0, 800, 600);
    f2.sync_win(2, 800, 0, 800, 600);
    f2.mouse_move(100, 100, 0);
    f2.mouse_move(100, 100, 200); // 焦点到 1
    f2.overlay(ExemptKind::ImeCandidate, true);
    f2.mouse_move(900, 300, 1_000);
    f2.mouse_move(900, 300, 1_300);
    let shielded = f2.focus() == Some(1);
    f2.overlay(ExemptKind::ImeCandidate, false);
    f2.mouse_move(900, 300, 1_400);
    f2.mouse_move(900, 300, 1_600);
    set.add(
        "overlay exempt while shown",
        shielded && f2.focus() == Some(2),
        "",
    );

    // 6. 豁免清单三类齐（输入法/托盘/横幅——枚举即清单）。
    set.add(
        "exempt list three kinds",
        EXEMPT_KINDS.len() == 3
            && EXEMPT_KINDS.contains(&ExemptKind::TrayFlyout)
            && EXEMPT_KINDS.contains(&ExemptKind::ToastBanner),
        "",
    );

    // 7. 与既有焦点链不冲突：点击置焦与跟随置焦同走 focus 唯一账。
    let mut f3 = FocusFollow::new();
    f3.set_enabled(true);
    f3.sync_win(5, 0, 0, 100, 100);
    f3.click(50, 50);
    let after_click = f3.focus() == Some(5);
    f3.mouse_move(10, 10, 0);
    f3.mouse_move(10, 10, 200);
    set.add(
        "click and follow share ledger",
        after_click && f3.focus() == Some(5),
        "",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn move_without_dwell_reset() {
        let mut f = FocusFollow::new();
        f.set_enabled(true);
        f.sync_win(1, 0, 0, 800, 600);
        // 扫过：快速穿过窗口不驻留 → 零转移。
        f.mouse_move(10, 10, 0);
        f.mouse_move(700, 500, 50);
        f.mouse_move(10, 10, 100);
        assert!(f.focus().is_none());
    }

    #[test]
    fn click_outside_no_hit() {
        let mut f = FocusFollow::new();
        f.sync_win(1, 0, 0, 100, 100);
        assert!(f.click(500, 500).is_none());
        assert_eq!(f.pass_through_count(), 0);
    }

    #[test]
    fn overlay_cleared_allows_follow() {
        let mut f = FocusFollow::new();
        f.set_enabled(true);
        f.sync_win(1, 0, 0, 800, 600);
        f.sync_win(2, 800, 0, 800, 600);
        for k in EXEMPT_KINDS {
            f.overlay(k, true);
        }
        f.overlay(ExemptKind::ImeCandidate, false);
        f.overlay(ExemptKind::TrayFlyout, false);
        f.overlay(ExemptKind::ToastBanner, false);
        f.mouse_move(100, 100, 0);
        assert_eq!(f.mouse_move(100, 100, 200), Some(1));
    }
}
