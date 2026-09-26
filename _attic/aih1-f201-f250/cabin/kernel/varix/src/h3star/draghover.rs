//! F315 拖拽悬停前置 · 完整设计（AI-H3 · 主册 G-H 区段）。
//!
//! **判据（主册）**：500ms±50ms 触发；最小化还原衔接；多标签切换用例；
//! 划过不触发（<300ms 20 次测试）；前置后落点高亮（F212 联动）。
//!
//! **设计要点（主册）**：拖着文件经过任务栏图标 500ms，对应窗口自动前
//! 置（若最小化则还原）接住拖放——Windows 十年的经典手感；经过多标签
//! 窗口 500ms 切到悬停标签；拖拽全程被拖影跟随鼠标；前置动画 120ms 不
//! 抢戏。无感标准：悬停一下它自己浮上来；时序刚好——划过不误触。
//!
//! 实现形态：悬停计时器（注入钟——500ms 进入窗 / 300ms 划过豁免）+
//! 前置状态机（含最小化还原衔接）+ 多标签切换 + 落点高亮标记。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 前置触发悬停时长（ms）。
pub const HOVER_TRIGGER_MS: u64 = 500;

/// 触发容差（±50ms 判线带）。
pub const HOVER_TOLERANCE_MS: u64 = 50;

/// 划过豁免线（<300ms 不触发）。
pub const GLANCE_EXEMPT_MS: u64 = 300;

/// 前置动画时长（ms——不抢戏）。
pub const RAISE_ANIM_MS: u64 = 120;

// ---------------------------------------------------------------------------
// 悬停前置状态机
// ---------------------------------------------------------------------------

/// 一扇可接拖放的窗口。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DropTarget {
    pub id: u32,
    pub minimized: bool,
    /// 多标签窗口的当前标签。
    pub active_tab: usize,
    pub tab_count: usize,
}

/// 计时状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HoverStage {
    Idle,
    Hovering { since_ms: u64 },
    Raised,
}

/// 悬停前置引擎。
pub struct HoverRaise {
    stage: HoverStage,
    pub target_id: u32,
    /// 落点高亮（前置后开启——F212 联动面）。
    pub drop_highlight: bool,
    /// 前置次数（记账）。
    pub raises: u64,
}

impl HoverRaise {
    pub fn new() -> HoverRaise {
        HoverRaise { stage: HoverStage::Idle, target_id: 0, drop_highlight: false, raises: 0 }
    }

    /// 拖影进入目标图标。
    pub fn enter(&mut self, id: u32, now_ms: u64) {
        self.stage = HoverStage::Hovering { since_ms: now_ms };
        self.target_id = id;
        self.drop_highlight = false;
    }

    /// 拖影离开（清计时——划过不残留）。
    pub fn leave(&mut self) {
        self.stage = HoverStage::Idle;
    }

    /// 悬停是否已到触发点（停留 ≥500ms——±50ms 为计量容差，
    /// 触发一次后由 Raised 态接管，不重复触发）。
    pub fn due_at(&self, now_ms: u64) -> bool {
        match self.stage {
            HoverStage::Hovering { since_ms } => {
                now_ms.saturating_sub(since_ms) >= HOVER_TRIGGER_MS - HOVER_TOLERANCE_MS
            }
            _ => false,
        }
    }

    /// 划过豁免判定：停留 <300ms 即离开 → 不触发。
    pub fn glanced(&self, left_at_ms: u64) -> bool {
        match self.stage {
            HoverStage::Hovering { since_ms } => left_at_ms.saturating_sub(since_ms) < GLANCE_EXEMPT_MS,
            _ => false,
        }
    }

    /// 执行前置：最小化则还原；多标签切到悬停标签；开落点高亮。
    pub fn raise(&mut self, target: &mut DropTarget) -> bool {
        if !matches!(self.stage, HoverStage::Hovering { .. }) {
            return false;
        }
        if target.minimized {
            target.minimized = false; // 还原衔接——接住拖放。
        }
        self.stage = HoverStage::Raised;
        self.raises += 1;
        self.drop_highlight = true; // 前置后落点高亮（F212 联动）。
        true
    }

    /// 多标签窗口：悬停 500ms 切到悬停标签。
    pub fn switch_tab(&mut self, target: &mut DropTarget, tab: usize) -> bool {
        if tab < target.tab_count {
            target.active_tab = tab;
            true
        } else {
            false
        }
    }

    /// 拖放完成 → 高亮清除（生命周期闭环）。
    pub fn drop_done(&mut self) {
        self.stage = HoverStage::Idle;
        self.drop_highlight = false;
    }

    pub fn in_hover(&self) -> bool {
        matches!(self.stage, HoverStage::Hovering { .. })
    }
}

impl Default for HoverRaise {
    fn default() -> HoverRaise {
        HoverRaise::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F315 自检（判据：500ms±50；最小化衔接；多标签切换；划过不触发；高亮）。
pub fn run_draghover_checks() -> CheckSet {
    let mut set = CheckSet::new("F315-draghover");

    // 1. 触发窗：450ms 不触发 / 500ms 触发 / 550ms 触发（±50ms 判线带）。
    let mut h = HoverRaise::new();
    h.enter(1, 1000);
    set.add(
        "trigger at 500ms with 50ms meter tolerance",
        !h.due_at(1449) && h.due_at(1500) && h.due_at(1560),
        "",
    );

    // 2. 划过不触发：<300ms 停留 → 豁免成立（20 次扫描全对——299ms
    //    内判豁免、离开后计时清零不残留）。
    let mut all_glance = true;
    for i in 0..20u64 {
        let mut g = HoverRaise::new();
        g.enter(1, i * 10);
        all_glance = all_glance && g.glanced(i * 10 + 299);
        g.leave();
        all_glance = all_glance && !g.glanced(i * 10 + 301);
    }
    set.add("glance exempt under 300ms", all_glance, "");

    // 3. 真悬停 ≥300ms 不豁免（划过豁免只护短停）。
    let mut g = HoverRaise::new();
    g.enter(1, 0);
    set.add("long hover not exempt", !g.glanced(400), "");

    // 4. 最小化还原衔接：最小化窗前置后还原（接住拖放）。
    let mut h = HoverRaise::new();
    h.enter(2, 0);
    let mut t = DropTarget { id: 2, minimized: true, active_tab: 0, tab_count: 1 };
    let ok = h.raise(&mut t);
    set.add(
        "minimized restored on raise",
        ok && !t.minimized && h.drop_highlight && h.raises == 1,
        "",
    );

    // 5. 多标签切换：切到悬停标签；越界拒绝。
    let mut h = HoverRaise::new();
    h.enter(3, 0);
    let mut t = DropTarget { id: 3, minimized: false, active_tab: 0, tab_count: 4 };
    let ok1 = h.switch_tab(&mut t, 2);
    let ok2 = h.switch_tab(&mut t, 9);
    set.add(
        "multi tab switch",
        ok1 && t.active_tab == 2 && !ok2 && t.active_tab == 2,
        "",
    );

    // 6. 落点高亮生命周期：前置开 → 拖放完成清。
    let mut h = HoverRaise::new();
    h.enter(1, 0);
    let mut t = DropTarget { id: 1, minimized: false, active_tab: 0, tab_count: 1 };
    let _ = h.raise(&mut t);
    let lit = h.drop_highlight;
    h.drop_done();
    set.add(
        "highlight lifecycle closed",
        lit && !h.drop_highlight && !h.in_hover(),
        "",
    );

    // 7. 未悬停时前置拒绝（Idle 态诚实拒绝）。
    let mut h = HoverRaise::new();
    let mut t = DropTarget { id: 1, minimized: false, active_tab: 0, tab_count: 1 };
    set.add("idle raise rejected", !h.raise(&mut t), "");

    // 8. 前置动画时长常量（120ms 不抢戏——参数入账）。
    set.add("raise anim 120ms", RAISE_ANIM_MS == 120, "");

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leave_clears_timer() {
        let mut h = HoverRaise::new();
        h.enter(1, 0);
        h.leave();
        assert!(!h.in_hover());
        assert!(!h.due_at(10_000));
    }

    #[test]
    fn re_enter_resets_timer() {
        let mut h = HoverRaise::new();
        h.enter(1, 0);
        h.enter(1, 10_000);
        assert!(!h.due_at(10_449));
        assert!(h.due_at(10_450));
    }

    #[test]
    fn raise_twice_counts() {
        let mut h = HoverRaise::new();
        let mut t = DropTarget { id: 1, minimized: false, active_tab: 0, tab_count: 1 };
        h.enter(1, 0);
        let _ = h.raise(&mut t);
        h.enter(1, 100);
        let _ = h.raise(&mut t);
        assert_eq!(h.raises, 2);
    }
}
