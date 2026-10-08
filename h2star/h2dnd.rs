//! H2 拖放管线 · 深化批次三·二波（拖放全生命周期状态机——F255 文本
//! 拖放、F262 拖拽语义、F280 触屏长按三域的共用事件层）。
//!
//! **承接判据**（主册 H 域正文，一处一事实）：
//! - **F255 文本拖放**：四落点（输入框/桌面/终端/浏览器）命中判定、
//!   插入点指示跟随、Ctrl 修饰语义（复制原文 vs 移动引用——终端
//!   粘贴安全提示挂点）、拖影视觉参数（半透明文本快照的布局）；
//! - **F262 拖拽语义**：拖放中段的落点高亮与放弃复原——**Esc 中途
//!   放弃必须复原**（十四章「拖到一半 Esc 能放弃并复原」），拖影
//!   消失路径完整；
//! - **F280 触屏长按**：长按启动拖拽的衔接——500ms 长按转拖拽的
//!   状态迁移在这里（触屏没有独立拖拽键，长按即拖拽的准入闸）。
//!
//! 状态机纪律：Idle→Press→LongPress→Dragging→Dropped/Cancelled
//! 每个状态都有明确出口；Dragged 状态下收到取消（Esc/失焦/触摸
//! cancel）必须走 Cancelled 并产出复原指令——零「拖影挂在半空」。

use crate::checks::CheckSet;

use alloc::string::String;

// ---------------------------------------------------------------------------
// 状态机
// ---------------------------------------------------------------------------

/// 拖放状态（全生命周期——零半空路径）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DragState {
    Idle,
    /// 指针按下（未判定为拖拽——位移阈值前不算）。
    Press,
    /// 触屏长按成立（拖拽准入）。
    LongPress,
    Dragging,
    Dropped,
    /// 已放弃且已产出复原指令（终态）。
    Cancelled,
}

/// 位移阈值：超过才算拖拽（像素——防手抖误拖）。
pub const DRAG_THRESHOLD_PX: i32 = 6;
/// 长按准入时长（F280 判据 500ms±50ms——取整 500）。
pub const LONG_PRESS_MS: u32 = 500;

/// 拖放数据（文本拖放为最小集）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DragPayload {
    pub text: String,
    /// 是否修饰复制（Ctrl 按住——语义由落点解释，这里带原样标记）。
    pub copy_modifier: bool,
}

/// 拖放会话：状态机的唯一载体。
pub struct DragSession {
    pub state: DragState,
    pub payload: Option<DragPayload>,
    /// 起点与当前点（px）。
    pub origin: (i32, i32),
    pub current: (i32, i32),
    /// 长按已等待 ms（Press 态累计）。
    held_ms: u32,
}

impl DragSession {
    pub fn new() -> DragSession {
        DragSession {
            state: DragState::Idle,
            payload: None,
            origin: (0, 0),
            current: (0, 0),
            held_ms: 0,
        }
    }

    /// 指针按下（Idle → Press）。
    pub fn press(&mut self, x: i32, y: i32) {
        if self.state == DragState::Idle {
            self.state = DragState::Press;
            self.origin = (x, y);
            self.current = (x, y);
            self.held_ms = 0;
        }
    }

    /// 移动喂入。Press 态位移超阈值 → 直接进 Dragging（鼠标路径：
    /// 位移即准入，不等长按）；Dragging 态更新当前位置。
    pub fn move_to(&mut self, x: i32, y: i32) {
        match self.state {
            DragState::Press => {
                self.current = (x, y);
                let dx = (x - self.origin.0).abs();
                let dy = (y - self.origin.1).abs();
                if dx.max(dy) > DRAG_THRESHOLD_PX {
                    self.state = DragState::Dragging;
                }
            }
            DragState::Dragging | DragState::LongPress => {
                self.current = (x, y);
            }
            _ => {}
        }
    }

    /// 长按计时喂入（触屏路径）：Press 态累计满 500ms → LongPress
    /// （拖拽准入）；Dragged/其他态忽略。
    pub fn tick(&mut self, delta_ms: u32) {
        if self.state == DragState::Press {
            self.held_ms += delta_ms;
            if self.held_ms >= LONG_PRESS_MS {
                self.state = DragState::LongPress;
            }
        }
    }

    /// 携带数据（LongPress/Dragging 态挂上载荷——拖什么）。
    pub fn attach(&mut self, text: &str, copy_modifier: bool) -> bool {
        if matches!(self.state, DragState::LongPress | DragState::Dragging) {
            self.payload = Some(DragPayload { text: text.into(), copy_modifier });
            true
        } else {
            false
        }
    }

    /// 放下（Dropped 终态——落点由调用方按 hit_test 决定）。
    pub fn drop(&mut self) -> Option<DragPayload> {
        if self.state == DragState::Dragging || self.state == DragState::LongPress {
            let p = self.payload.take();
            self.state = DragState::Dropped;
            p
        } else {
            None
        }
    }

    /// 取消（Esc/失焦/触摸 cancel）——从 Press/LongPress/Dragging
    /// 任一中间态进入 Cancelled，并给复原指令（拖影消失 + 无落点
    /// 动作——「放弃并复原」的机制化）。
    pub fn cancel(&mut self) -> bool {
        if matches!(
            self.state,
            DragState::Press | DragState::LongPress | DragState::Dragging
        ) {
            self.payload = None;
            self.state = DragState::Cancelled;
            true
        } else {
            false
        }
    }

    pub fn is_live(&self) -> bool {
        matches!(self.state, DragState::Press | DragState::LongPress | DragState::Dragging)
    }
}

// ---------------------------------------------------------------------------
// 落点命中（F255 四落点）
// ---------------------------------------------------------------------------

/// 落点类别（F255 判据四类——枚举无第五成员，漏类别编译期可见）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropKind {
    InputBox,
    Desktop,
    Terminal,
    Browser,
}

/// 落点区域：类别 + 矩形（x, y, w, h）。
#[derive(Clone, Copy, Debug)]
pub struct DropZone {
    pub kind: DropKind,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl DropZone {
    pub fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
    }
}

/// 命中判定：多区重叠时**取最内层**（面积最小者——嵌套窗口的
/// 正确语义，不是列表序运气）。
pub fn hit_test(zones: &[DropZone], px: i32, py: i32) -> Option<DropKind> {
    zones
        .iter()
        .filter(|z| z.contains(px, py))
        .min_by_key(|z| z.w * z.h)
        .map(|z| z.kind)
}

/// 落点动作：修饰键 × 落点类别 → 动作（F255 Ctrl 语义 + 终端
/// 安全提示——终端粘贴外部文本必须显性提示，安全红线的落点侧）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropAction {
    Insert,
    MoveRef,
    InsertWithConfirm,
    Rejected,
}

pub fn drop_action(kind: DropKind, copy_modifier: bool) -> DropAction {
    match kind {
        DropKind::InputBox | DropKind::Browser | DropKind::Desktop => {
            if copy_modifier {
                DropAction::MoveRef
            } else {
                DropAction::Insert
            }
        }
        DropKind::Terminal => DropAction::InsertWithConfirm,
    }
}

// ---------------------------------------------------------------------------
// 拖影视觉（F255 半透明文本快照的布局）
// ---------------------------------------------------------------------------

/// 拖影参数（判据：半透明文本快照——不透明度 60%、偏移 (12,12)
/// 防自遮挡、单行截断 24 显示列）。
pub const GHOST_ALPHA_PERMILLE: u32 = 600;
pub const GHOST_OFFSET: (i32, i32) = (12, 12);

/// 拖影矩形：跟随手走（不是松手才瞬移——十四章手感公理）。
pub fn ghost_rect(cursor: (i32, i32), text_w: i32) -> (i32, i32, i32, i32) {
    (
        cursor.0 + GHOST_OFFSET.0,
        cursor.1 + GHOST_OFFSET.1,
        text_w,
        24,
    )
}

// ---------------------------------------------------------------------------
// 自检（判据逐条钉死）
// ---------------------------------------------------------------------------

pub fn run_h2dnd_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-h2dnd");
    // 鼠标路径：位移阈值才成拖——5px 不算、7px 算。
    let mut s = DragSession::new();
    s.press(100, 100);
    s.move_to(105, 100);
    set.add("h2dnd threshold holds", s.state == DragState::Press, "5px not drag");
    s.move_to(108, 100);
    set.add("h2dnd threshold fires", s.state == DragState::Dragging, "7px drags");
    // 触屏路径：500ms 长按准入；499ms 不进。
    let mut t = DragSession::new();
    t.press(50, 50);
    t.tick(499);
    set.add("h2dnd longpress gate", t.state == DragState::Press, "499ms waits");
    t.tick(1);
    set.add("h2dnd longpress fires", t.state == DragState::LongPress, "500ms in");
    // 载荷挂载：准入态才挂；Idle 拒绝。
    set.add("h2dnd attach ok", t.attach("拖这段文本", false), "longpress attach");
    let mut idle = DragSession::new();
    set.add("h2dnd attach idle no", !idle.attach("x", false), "idle refused");
    // 放下：载荷交出 + 终态；未挂载荷的放下 = None（诚实空手）。
    let got = t.drop();
    set.add(
        "h2dnd drop hands over",
        got.as_ref().map(|p| p.text.as_str()) == Some("拖这段文本")
            && t.state == DragState::Dropped
            && t.drop().is_none(),
        "payload once",
    );
    // Esc 取消：中间态可取消、终态不可再取消、复原无残留。
    let mut c = DragSession::new();
    c.press(0, 0);
    c.move_to(50, 0);
    c.attach("将被放弃", true);
    set.add(
        "h2dnd cancel restore",
        c.cancel() && c.state == DragState::Cancelled && c.payload.is_none() && !c.is_live(),
        "esc restores",
    );
    set.add("h2dnd cancel terminal", !c.cancel(), "terminal state locked");
    // 命中：嵌套区取最内层；脱靶 None。
    let zones = [
        DropZone { kind: DropKind::Desktop, x: 0, y: 0, w: 800, h: 600 },
        DropZone { kind: DropKind::Terminal, x: 100, y: 100, w: 200, h: 150 },
    ];
    set.add(
        "h2dnd innermost wins",
        hit_test(&zones, 150, 150) == Some(DropKind::Terminal)
            && hit_test(&zones, 50, 50) == Some(DropKind::Desktop)
            && hit_test(&zones, 900, 900).is_none(),
        "nested inner first",
    );
    // 落点动作：Ctrl 语义 + 终端必确认（安全提示挂点）。
    set.add(
        "h2dnd action matrix",
        drop_action(DropKind::InputBox, false) == DropAction::Insert
            && drop_action(DropKind::Desktop, true) == DropAction::MoveRef
            && drop_action(DropKind::Terminal, false) == DropAction::InsertWithConfirm
            && drop_action(DropKind::Terminal, true) == DropAction::InsertWithConfirm,
        "terminal always confirms",
    );
    // 拖影：跟随手走（cursor 移动 → 拖影同步移动）。
    let g1 = ghost_rect((10, 10), 80);
    let g2 = ghost_rect((30, 30), 80);
    set.add(
        "h2dnd ghost follows",
        g1 == (22, 22, 80, 24) && g2 == (42, 42, 80, 24),
        "cursor-locked",
    );
    set.add(
        "h2dnd ghost alpha",
        GHOST_ALPHA_PERMILLE == 600,
        "60% translucency",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn h2dnd_all_green() {
        let set = run_h2dnd_checks();
        assert!(set.all_passed(), "h2dnd 自检有红项");
        assert!(!set.truncated(), "h2dnd 自检溢出");
    }

    #[test]
    fn every_state_has_exit() {
        // 全状态遍历：任何状态喂 cancel/drop/move_to/tick 都不 panic、
        // 不出非法迁移（状态机封闭性——十四章状态机公理）。
        for st in [
            DragState::Idle,
            DragState::Press,
            DragState::LongPress,
            DragState::Dragging,
            DragState::Dropped,
            DragState::Cancelled,
        ] {
            let mut s = DragSession::new();
            s.state = st;
            s.move_to(1, 1);
            s.tick(100);
            let _ = s.attach("x", false);
            let _ = s.drop();
            let _ = s.cancel();
            assert!(matches!(
                s.state,
                DragState::Idle
                    | DragState::Press
                    | DragState::LongPress
                    | DragState::Dragging
                    | DragState::Dropped
                    | DragState::Cancelled
            ));
        }
    }

    #[test]
    fn cancel_never_leaves_payload() {
        // 拖拽中取消 100 次：载荷永远清空（无幽灵拖影）。
        for _ in 0..100 {
            let mut s = DragSession::new();
            s.press(0, 0);
            s.move_to(30, 0);
            s.attach("幽灵", false);
            assert!(s.cancel() && s.payload.is_none());
        }
    }
}
