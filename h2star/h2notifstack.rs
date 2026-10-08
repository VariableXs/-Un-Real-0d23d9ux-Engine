//! H2 通知堆叠几何 · 深化批次五（F281 呈现侧——横幅位置、堆叠
//! 上限、退出方向、免打扰联动的布局引擎）。
//!
//! **承接判据**（主册 H 域正文 + 人格章程七章/二章，一处一事实）：
//! - **F281 横幅**：右下角堆叠（不遮任务栏、不抢焦点）、堆叠上限
//!   （超限最旧先走——通知风暴不糊屏）、进出场沿右缘滑动（出向
//!   右——桌面公理：通知从边缘来，回边缘去）；
//! - **免打扰（F115 车道，经 F281 锚）**：免打扰开着 = 横幅不呈现、
//!   全部直接入中心（呈现层静默、中心层照常——通知不丢只不弹）；
//! - **十二章**：堆叠上限驱逐与 F077 中心完整性联动——被驱逐的
//!   横幅仍进通知中心（横幅只是呈现，不是消息本体）。
//!
//! 几何纪律：纯函数布局（同状态同布局）；锚 h2geo::Rect。

use crate::checks::CheckSet;

use crate::h2star::h2geo::Rect;

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 堆叠模型
// ---------------------------------------------------------------------------

/// 横幅尺寸与堆叠参数（域内唯一）。
pub const BANNER_W: u32 = 360;
pub const BANNER_H: u32 = 84;
pub const STACK_GAP: u32 = 10;
/// 堆叠上限（可见横幅最多 3 条——第 4 条起直接入中心）。
pub const STACK_VISIBLE_CAP: usize = 3;
/// 与屏幕右缘/下缘的安全边距（避开任务栏 48px 再留 16px）。
pub const EDGE_MARGIN: u32 = 16;
/// 进出场滑动距离（右缘外——出向右）。
pub const SLIDE_DISTANCE: i32 = 420;

/// 堆叠布局：给可见横幅序（0 = 最新在下沿），产出逐条矩形。
/// 锚点 = 屏幕右下角，向上堆叠；不遮任务栏（调用方传的 screen
/// 已扣掉任务栏高度）。
pub fn stack_layout(count: usize, screen_w: u32, work_h: u32) -> Vec<Rect> {
    let count = count.min(STACK_VISIBLE_CAP);
    (0..count)
        .map(|i| {
            let y64 = work_h as i64
                - EDGE_MARGIN as i64
                - ((i as u64 + 1) * (BANNER_H + STACK_GAP) as u64) as i64
                + STACK_GAP as i64;
            let y = y64.max(0) as i32;
            Rect::new(
                screen_w as i32 - EDGE_MARGIN as i32 - BANNER_W as i32,
                y,
                BANNER_W,
                BANNER_H,
            )
        })
        .collect()
}

/// 进出场位移动画参数：进场从右侧滑入（x = 终点 + SLIDE_DISTANCE），
/// 退场滑出（同向——进出场对称，八章/六章公理）。
pub fn slide_offset(progress_permille: u32, entering: bool) -> i32 {
    let p = progress_permille.min(1000) as i64;
    // 进场：位移 = (1-p) × 距离（p=0 全距、p=1000 归位）；
    // 退场：位移 = p × 距离（反向使用同一曲线——对称）。
    let shift = if entering { 1000 - p } else { p };
    (SLIDE_DISTANCE as i64 * shift / 1000) as i32
}

// ---------------------------------------------------------------------------
// 呈现决策（免打扰 + 上限驱逐）
// ---------------------------------------------------------------------------

/// 呈现决策。
#[derive(Debug, PartialEq, Eq)]
pub struct PresentPlan {
    /// 横幅呈现的条数。
    pub shown: usize,
    /// 直接入中心的条数（超出上限 + 免打扰期间全部）。
    pub to_center: usize,
    /// 被上限驱逐的（最新三条之外的旧条——仍进中心）。
    pub evicted_to_center: usize,
}

/// 呈现决策：免打扰开着 → 全部入中心；否则新横幅入栈（每条新
/// 横幅占一槽、挤出的最旧横幅进中心——**零丢弃**：通知本体永不
/// 丢，丢的只是横幅呈现形态）。
pub fn present(dnd: bool, incoming: usize, already_shown: usize) -> PresentPlan {
    if dnd {
        return PresentPlan { shown: 0, to_center: incoming, evicted_to_center: 0 };
    }
    let shown = incoming.min(STACK_VISIBLE_CAP);
    let visible_before = already_shown.min(STACK_VISIBLE_CAP);
    let evicted = (visible_before + shown).saturating_sub(STACK_VISIBLE_CAP);
    let to_center = incoming - shown;
    PresentPlan { shown, to_center, evicted_to_center: evicted }
}

// ---------------------------------------------------------------------------
// 自检（判据逐条钉死）
// ---------------------------------------------------------------------------

pub fn run_h2notifstack_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-h2notifstack");
    // 布局：右下锚、向上堆叠、不遮任务栏（y ≥ 0）、间距正确。
    let l3 = stack_layout(3, 1920, 1000);
    set.add(
        "h2notifstack anchor",
        l3.len() == 3
            && l3[0].x == (1920 - 16 - BANNER_W) as i32
            && l3[0].y + BANNER_H as i32 <= 1000
            && l3[1].y < l3[0].y,
        "bottom-right, stacked up",
    );
    // 上限：第 4 条起不布局（布局只画 ≤3）。
    let l9 = stack_layout(9, 1920, 1000);
    set.add(
        "h2notifstack cap",
        l9.len() == STACK_VISIBLE_CAP,
        "layout ≤ 3",
    );
    // 呈现决策：免打扰全入中心；正常最新 3 条上横幅。
    let p1 = present(true, 5, 0);
    let p2 = present(false, 5, 0);
    set.add(
        "h2notifstack dnd routes all",
        p1 == PresentPlan { shown: 0, to_center: 5, evicted_to_center: 0 }
            && p2 == PresentPlan { shown: 3, to_center: 2, evicted_to_center: 0 },
        "dnd silent, normal capped",
    );
    // 已占满再进新条：挤出最旧（驱逐也进中心——零丢弃）。
    let p3 = present(false, 1, 3);
    set.add(
        "h2notifstack evict oldest",
        p3 == PresentPlan { shown: 1, to_center: 0, evicted_to_center: 1 },
        "oldest slides out to center",
    );
    // 零丢弃守恒：shown + to_center = incoming（dnd 关时再 + 驱逐回流）。
    let p4 = present(false, 7, 2);
    set.add(
        "h2notifstack zero loss",
        p4.shown + p4.to_center == 7 && p4.evicted_to_center <= 2,
        "nothing dropped",
    );
    // 滑动对称：进场 0‰ 全距、1000‰ 归位；退场镜像。
    set.add(
        "h2notifstack slide symmetric",
        slide_offset(0, true) == SLIDE_DISTANCE
            && slide_offset(1000, true) == 0
            && slide_offset(0, false) == 0
            && slide_offset(1000, false) == SLIDE_DISTANCE
            && slide_offset(500, true) == slide_offset(500, false),
        "enter/exit mirrored",
    );
    set.add(
        "h2notifstack consts",
        STACK_VISIBLE_CAP == 3 && BANNER_W == 360 && EDGE_MARGIN == 16,
        "single definition",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn h2notifstack_all_green() {
        let set = run_h2notifstack_checks();
        assert!(set.all_passed(), "h2notifstack 自检有红项");
        assert!(!set.truncated(), "h2notifstack 自检溢出");
    }

    #[test]
    fn storm_never_overflows_screen() {
        // 通知风暴 50 条灌入：可见恒 ≤3、全部消息有归宿（零丢弃压测）。
        let mut shown = 0usize;
        let mut total_accounted = 0usize;
        for _ in 0..50 {
            let p = present(false, 1, shown);
            shown = shown.saturating_sub(p.evicted_to_center) + p.shown;
            total_accounted += p.to_center + p.shown;
            assert!(shown <= STACK_VISIBLE_CAP);
        }
        assert_eq!(total_accounted, 50);
    }

    #[test]
    fn layout_never_covers_taskbar() {
        // 小屏（工作区 300px 高）：布局钳到 0 以上——不画出屏。
        for i in 0..3 {
            let l = stack_layout(i + 1, 1024, 300);
            assert!(l.iter().all(|r| r.y >= 0));
        }
    }
}
