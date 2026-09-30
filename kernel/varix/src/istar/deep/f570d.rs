//! 深化层 · F570 显示桌面按钮（回炉补深主册【设计要点】未实装机制）。
//!
//! 补深四条（判据唯一源：主册 F570 节）：
//! ①「悬停半透明预览（Aero Peek——F537 同款效果）」的 **Peek 预览状态机**
//!   ——悬停进入/移出/点击三通路的预览层生命周期账：进入快照窗集合、
//!   移出收层无残留、点击交账压窗，桌面已显示态诚实不出预览；
//! ②「热区比视觉大 2px（6px+2px 吸附扩展）」的 **热区几何引擎**——视觉
//!   细带 6px 之内必中、吸附带（视觉外 2px）吸附命中、吸附带外诚实脱靶；
//! ③「与 Win+D/晃动（F425）三路同效殊途同归」的 **三路同效对账**——
//!   点击/Win+D/晃动三个触发源汇入同一最小化账，触发总数与状态机切换
//!   数逐一对账（有旁路即红）；
//! ④「细带小到不误点」的 **双击防误触**——窗口期内第二击不重复触发
//!   （一次按压一次生效）。

use crate::checks::CheckSet;
use crate::istar::ibase::ISTAR_DOMAIN;
use crate::istar::showdesk::{HOTZONE_EXTRA_PX, PEEK_OPACITY_PCT, STRIP_W_PX, ShowDesk};

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// Peek 预览状态机（预览层生命周期账）
// ---------------------------------------------------------------------------

/// Peek 预览层账：悬停三通路的层生命周期（层账与桌面账分离——悬停
/// 永不改桌面态，压窗只发生在点击通路）。
pub struct PeekLedger {
    open: bool,
    /// 预览层展示的窗集合（进入时快照——移出/点击即收，无残留）。
    shown: Vec<u64>,
}

impl PeekLedger {
    pub fn new() -> PeekLedger {
        PeekLedger { open: false, shown: Vec::new() }
    }

    /// 通路一：悬停进入——开层并快照窗集合。层已开（重复进入）或桌面
    /// 已显示（无窗可预览——诚实降级）时拒。
    pub fn enter(&mut self, ids: &[u64], desktop_visible: bool) -> bool {
        if self.open || !desktop_visible {
            return false;
        }
        self.open = true;
        self.shown = ids.to_vec();
        true
    }

    /// 通路二：移出——收层清快照（未开层返回 false）。
    pub fn leave(&mut self) -> bool {
        if !self.open {
            return false;
        }
        self.open = false;
        self.shown.clear();
        true
    }

    /// 通路三：点击——预览层交账收层（压窗由 ShowDesk 负责，本账只管层）。
    pub fn consume_by_click(&mut self) -> bool {
        self.leave()
    }

    pub fn open(&self) -> bool {
        self.open
    }

    pub fn shown(&self) -> &[u64] {
        &self.shown
    }
}

impl Default for PeekLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 热区几何引擎
// ---------------------------------------------------------------------------

/// 热区命中判定（任务栏横向一维坐标 x / 任务栏总宽 w）：视觉细带
/// STRIP_W_PX 之内必中；两侧各 HOTZONE_EXTRA_PX 吸附扩展带内吸附命中；
/// 吸附带外诚实脱靶。
pub fn hotzone_hit(x: i32, w: i32) -> bool {
    let left = w - STRIP_W_PX as i32 - HOTZONE_EXTRA_PX as i32;
    let right = w + HOTZONE_EXTRA_PX as i32;
    x >= left && x < right
}

/// 视觉带严格命中（吸附扩展之外不算视觉命中——几何账分层口径）。
pub fn visual_hit(x: i32, w: i32) -> bool {
    x >= w - STRIP_W_PX as i32 && x < w
}

// ---------------------------------------------------------------------------
// 三路同效对账
// ---------------------------------------------------------------------------

/// 三路触发源对账器：点击/Win+D/晃动各自入账，与状态机切换账核对
/// （殊途同归 = 三源汇入同一台 ShowDesk，无一路旁路）。
pub struct ThreeWayLedger {
    events: [u32; 3],
}

impl ThreeWayLedger {
    pub const SRC_CLICK: usize = 0;
    pub const SRC_WIN_D: usize = 1;
    pub const SRC_SHAKE: usize = 2;

    pub fn new() -> ThreeWayLedger {
        ThreeWayLedger { events: [0; 3] }
    }

    /// 触发入账（源号越界拒——账不收野路）。
    pub fn note(&mut self, src: usize) -> bool {
        if src < 3 {
            self.events[src] += 1;
            true
        } else {
            false
        }
    }

    pub fn per_source(&self, src: usize) -> u32 {
        if src < 3 {
            self.events[src]
        } else {
            0
        }
    }

    pub fn total(&self) -> u32 {
        self.events[0] + self.events[1] + self.events[2]
    }

    /// 一致性：账面触发总数 == 状态机切换总数（旁路一击即红）。
    pub fn consistent_with(&self, toggles: u32) -> bool {
        self.total() == toggles
    }
}

impl Default for ThreeWayLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 双击防误触
// ---------------------------------------------------------------------------

/// 防误触窗口（ms）：窗口期内第二击拒收。
pub const DOUBLE_CLICK_GUARD_MS: u64 = 400;

/// 双击防误触闸：一次按压一次生效——距上次受击不足窗口期的第二击拒。
pub struct ClickGuard {
    last_accepted: Option<u64>,
}

impl ClickGuard {
    pub fn new() -> ClickGuard {
        ClickGuard { last_accepted: None }
    }

    /// 时刻 now_ms 的一击是否收下（收下即记账）。
    pub fn admit(&mut self, now_ms: u64) -> bool {
        match self.last_accepted {
            Some(t) if now_ms.saturating_sub(t) < DOUBLE_CLICK_GUARD_MS => false,
            _ => {
                self.last_accepted = Some(now_ms);
                true
            }
        }
    }
}

impl Default for ClickGuard {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 深化自检
// ---------------------------------------------------------------------------

pub fn run_f570_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new(ISTAR_DOMAIN);

    // 1) 热区几何：视觉带内必中；两侧吸附带命中；吸附带外脱靶。
    let w = 1_000i32;
    let in_visual = hotzone_hit(w - 3, w) && visual_hit(w - 3, w);
    let in_snap = hotzone_hit(w + 1, w) && hotzone_hit(w - STRIP_W_PX as i32 - 1, w)
        && !visual_hit(w + 1, w);
    let out_miss = !hotzone_hit(w + 3, w)
        && !hotzone_hit(w - STRIP_W_PX as i32 - HOTZONE_EXTRA_PX as i32 - 1, w);
    cs.add("hotzone 6px visual 2px snap", in_visual && in_snap && out_miss, "");

    // 2) Peek 生命周期：悬停进入开层快照；桌面账不动；移出收层无残留。
    let mut sd = ShowDesk::new();
    sd.sync_win(1, false);
    sd.sync_win(2, false);
    let mut peek = PeekLedger::new();
    let opened = peek.enter(&[1, 2], sd.desktop_visible());
    let desk_untouched = sd.desktop_visible() && sd.pressed_ids().is_empty();
    let closed = peek.leave();
    cs.add(
        "peek hover open snapshot close",
        opened && desk_untouched && closed && !peek.open() && peek.shown().is_empty(),
        "",
    );

    // 3) 点击通路 + 恢复原位账：预览层交账、压窗；恢复只还本按钮压下的
    //    窗（用户手动最小化的窗跨压/恢复两轮始终不参与）。
    let mut sd3 = ShowDesk::new();
    sd3.sync_win(1, false);
    sd3.sync_win(3, true); // 用户手动最小化
    let mut pk = PeekLedger::new();
    let _ = pk.enter(&[1], true);
    let path = pk.consume_by_click() && sd3.click();
    let first = sd3.pressed_ids() == alloc::vec![1u64];
    let back = sd3.click();
    let again = sd3.click() && sd3.pressed_ids() == alloc::vec![1u64];
    cs.add("restore ledger spares user minned", path && first && back && again, "");

    // 4) 桌面已显示（全部压下态）时悬停不出预览（诚实降级）。
    let mut sd4 = ShowDesk::new();
    sd4.sync_win(1, false);
    sd4.click();
    let mut pk4 = PeekLedger::new();
    cs.add(
        "no peek while desktop shown",
        !pk4.enter(&[1], sd4.desktop_visible()) && !pk4.open(),
        "",
    );

    // 5) 三路同效：点击/Win+D/晃动（F425 走同一 click 通路入账）三源
    //    汇入同一台 ShowDesk——触发总数与切换账一致。
    let mut sd5 = ShowDesk::new();
    sd5.sync_win(1, false);
    let mut tw = ThreeWayLedger::new();
    let _ = tw.note(ThreeWayLedger::SRC_CLICK);
    let _ = sd5.click();
    let _ = tw.note(ThreeWayLedger::SRC_WIN_D);
    let _ = sd5.win_d();
    let _ = tw.note(ThreeWayLedger::SRC_SHAKE);
    let _ = sd5.click();
    cs.add(
        "three sources one ledger",
        tw.total() == 3
            && tw.per_source(ThreeWayLedger::SRC_WIN_D) == 1
            && sd5.toggle_count() == 3
            && tw.consistent_with(sd5.toggle_count()),
        "",
    );

    // 6) 三路对账诚实：旁路一击（状态机切了、账没记）即红。
    let _ = sd5.click();
    cs.add("bypass detected inconsistent", !tw.consistent_with(sd5.toggle_count()), "");

    // 7) 双击防误触：窗口期内第二击拒；窗口边界上（恰 400ms）新击收。
    let mut g = ClickGuard::new();
    let a = g.admit(0);
    let b = g.admit(DOUBLE_CLICK_GUARD_MS - 1);
    let c = g.admit(DOUBLE_CLICK_GUARD_MS);
    let d = g.admit(DOUBLE_CLICK_GUARD_MS + 1);
    cs.add("double click guard window", a && !b && c && !d, "");

    // 8) Peek 透明度与 F537 同源（深化不漂移基础常量）。
    cs.add("peek opacity f537 aligned", PEEK_OPACITY_PCT == 40, "");

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hotzone_boundaries_exact() {
        let w = 500i32;
        let left = w - STRIP_W_PX as i32 - HOTZONE_EXTRA_PX as i32;
        let right = w + HOTZONE_EXTRA_PX as i32;
        assert!(hotzone_hit(left, w));
        assert!(!hotzone_hit(left - 1, w));
        assert!(hotzone_hit(right - 1, w));
        assert!(!hotzone_hit(right, w));
    }

    #[test]
    fn peek_enter_twice_second_rejected() {
        let mut p = PeekLedger::new();
        assert!(p.enter(&[1], true));
        assert!(!p.enter(&[2], true));
        assert_eq!(p.shown(), &[1u64]);
    }

    #[test]
    fn guard_zero_gap_rejected() {
        let mut g = ClickGuard::new();
        assert!(g.admit(100));
        assert!(!g.admit(100));
    }
}
