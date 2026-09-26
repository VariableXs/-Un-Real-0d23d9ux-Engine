
// ---------------------------------------------------------------------------
// F018 · 深化批次九：拖拽启动阈值（系统拖拽灵敏度 SM_CXDRAG/SM_CYDRAG
// 钉值 4px——按下后移动不足阈值是「点歪」不是「拖」，零误拖纪律；超阈值
// 才进入拖放会话）。
// ---------------------------------------------------------------------------

/// 系统拖拽阈值（GetSystemMetrics SM_CXDRAG/SM_CYDRAG——Windows 缺省 4）。
pub const SM_CXDRAG: i32 = 36;
pub const SM_CYDRAG: i32 = 37;
pub const DRAG_THRESHOLD_PX: i32 = 4;

/// 按压状态机（idle → pressed → dragging；阈值判定用切比雪夫距离——
/// x/y 任一超出阈值即启动，与系统语义一致）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PressState {
    Idle,
    Pressed { x: i32, y: i32 },
    Dragging,
}

/// 步进：按下记录锚点；移动超阈值启动拖放；松手根据状态归位。
pub fn press_step(state: PressState, ev: PressEv, x: i32, y: i32) -> PressState {
    match (state, ev) {
        (PressState::Idle, PressEv::Down) => PressState::Pressed { x, y },
        (PressState::Pressed { x: ax, y: ay }, PressEv::Move) => {
            let dx = (x - ax).abs();
            let dy = (y - ay).abs();
            if dx >= DRAG_THRESHOLD_PX || dy >= DRAG_THRESHOLD_PX {
                PressState::Dragging
            } else {
                state
            }
        }
        (PressState::Dragging, PressEv::Move) => PressState::Dragging,
        (_, PressEv::Up) => PressState::Idle,
    }
}

/// 按压事件。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PressEv {
    Down,
    Move,
    Up,
}

/// F018 深化批次九自检。
fn run_dragdrop_deep8_checks() -> CheckSet {
    let mut cs = CheckSet::new("F018-dragdrop-deep8");
    // 1) 阈值内微动不启动（3px 抖动仍是按压——零误拖纪律）。
    let s1 = press_step(press_step(PressState::Idle, PressEv::Down, 100, 100), PressEv::Move, 103, 100);
    // 2) 任一轴达 4px 启动（x 静止、y 4px 也算——切比雪夫语义）。
    let s2 = press_step(s1, PressEv::Move, 100, 104);
    cs.add(
        "drag_threshold_chebyshev",
        s1 == PressState::Pressed { x: 100, y: 100 }
            && s2 == PressState::Dragging,
        "",
    );
    // 3) 拖放中松手归 Idle；按压中松手归 Idle（点击完成——不残留状态）。
    let s3 = press_step(s2, PressEv::Up, 100, 104);
    let s4 = press_step(PressState::Pressed { x: 5, y: 5 }, PressEv::Up, 5, 5);
    cs.add(
        "press_release_returns_idle",
        s3 == PressState::Idle && s4 == PressState::Idle,
        "",
    );
    // 4) 阈值钉值 4（与 GetSystemMetrics 缺省一致——一个字节都不要漂）。
    cs.add(
        "drag_threshold_pinned",
        DRAG_THRESHOLD_PX == 4,
        "",
    );
    cs
}
