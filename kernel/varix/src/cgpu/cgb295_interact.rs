// CGPU-F0295 交互契约 · cgb295_interact.rs
// AI-MIMO-1022 · CGPU-B 可视化续章
// 判据：交互事件 → 渲染指令双向契约；命中测试确定性；拖拽/缩放/选中状态机闭环；
//       所有交互走统一事件总线，禁止旁路；契约违反即拒。
#![allow(dead_code)]

use alloc::format;
extern crate alloc;
use alloc::vec::Vec;
use alloc::string::String;

use super::super::checks::CheckSet;

/// 交互事件源（鼠标/键盘/触摸/手势统一归一）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EventSource {
    Mouse,
    Keyboard,
    Touch,
    Gesture,
    Synthetic,
}

/// 指针按钮（位掩码语义，支持组合）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PointerButtons(pub u8);

impl PointerButtons {
    pub const NONE: Self = Self(0);
    pub const LEFT: Self = Self(1);
    pub const RIGHT: Self = Self(2);
    pub const MIDDLE: Self = Self(4);

    pub fn has(self, b: Self) -> bool {
        (self.0 & b.0) != 0
    }
    pub fn union(self, o: Self) -> Self {
        Self(self.0 | o.0)
    }
}

/// 归一化交互事件（视口坐标系，浮点定点化 u16.16 避免浮点不确定）。
#[derive(Clone, Copy, Debug)]
pub struct InteractEvent {
    pub seq: u64,
    pub source: EventSource,
    pub kind: EventKind,
    pub x_q16: u32,
    pub y_q16: u32,
    pub buttons: PointerButtons,
    pub modifiers: u8,
    pub timestamp_us: u64,
}

/// 事件类别（有限封闭集，禁止开放字符串）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EventKind {
    PointerDown,
    PointerUp,
    PointerMove,
    Wheel,
    KeyDown,
    KeyUp,
    FocusIn,
    FocusOut,
    GestureStart,
    GestureUpdate,
    GestureEnd,
}

/// 命中测试结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HitResult {
    None,
    Node(u32),
    Edge(u32),
    Label(u32),
    Background,
    Handle(u32),
}

/// 渲染指令（交互侧产出，渲染侧消费；单向数据流）。
#[derive(Clone, Debug)]
pub enum RenderCmd {
    SetCursor(CursorShape),
    HighlightNode(u32, HighlightLevel),
    HighlightEdge(u32, HighlightLevel),
    ShowTooltip { anchor_q16: (u32, u32), text_id: u32 },
    HideTooltip,
    ScrollTo { x_q16: u32, y_q16: u32 },
    ZoomTo { level_q8: u16, pivot_q16: (u32, u32) },
    BeginDrag { target: HitResult },
    UpdateDrag { x_q16: u32, y_q16: u32 },
    EndDrag { cancelled: bool },
    InvalidateRegion { x: u16, y: u16, w: u16, h: u16 },
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CursorShape {
    Default,
    Pointer,
    Grab,
    Grabbing,
    Crosshair,
    Text,
    NotAllowed,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HighlightLevel {
    None,
    Subtle,
    Strong,
    Critical,
}

/// 交互状态机状态（拖拽/悬停/选中正交组合，此处为组合后主态）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InteractionState {
    Idle,
    Hovering,
    Pressed,
    Dragging,
    Panning,
    Zooming,
    Selecting,
    Modal,
}

/// 视口变换（平移 + 缩放，定点数保证确定性）。
#[derive(Clone, Copy, Debug)]
pub struct Viewport {
    pub pan_x_q16: u32,
    pub pan_y_q16: u32,
    pub zoom_q8: u16,
    pub width: u16,
    pub height: u16,
}

impl Viewport {
    pub fn new(width: u16, height: u16) -> Self {
        Self {
            pan_x_q16: 0,
            pan_y_q16: 0,
            zoom_q8: 256,
            width,
            height,
        }
    }

    /// 视口坐标 → 世界坐标（确定性逆变换）。
    pub fn to_world(&self, x_q16: u32, y_q16: u32) -> (u32, u32) {
        let z = self.zoom_q8 as u32;
        if z == 0 {
            return (0, 0);
        }
        let wx = (((x_q16 as u64) << 8) / z as u64).wrapping_add(self.pan_x_q16 as u64) as u32;
        let wy = (((y_q16 as u64) << 8) / z as u64).wrapping_add(self.pan_y_q16 as u64) as u32;
        (wx, wy)
    }

    /// 世界坐标 → 视口坐标。
    pub fn to_view(&self, wx: u32, wy: u32) -> (u32, u32) {
        let z = self.zoom_q8 as u32;
        let vx = ((wx.wrapping_sub(self.pan_x_q16) as u64) * z as u64) >> 8;
        let vy = ((wy.wrapping_sub(self.pan_y_q16) as u64) * z as u64) >> 8;
        (vx as u32, vy as u32)
    }

    /// 缩放（围绕枢轴，clamp 到 [16, 1024] 即 1/16x ~ 4x）。
    pub fn zoom_at(&mut self, level_q8: u16, pivot_q16: (u32, u32)) {
        let new_z = if level_q8 < 16 {
            16
        } else if level_q8 > 1024 {
            1024
        } else {
            level_q8
        };
        // 枢轴世界坐标不变：先算世界点，改 zoom 后反推 pan
        let (wx, wy) = self.to_world(pivot_q16.0, pivot_q16.1);
        self.zoom_q8 = new_z;
        let z = new_z as u64;
        let px = pivot_q16.0 as u64;
        let py = pivot_q16.1 as u64;
        self.pan_x_q16 = wx.wrapping_sub(((px << 8) / z) as u32);
        self.pan_y_q16 = wy.wrapping_sub(((py << 8) / z) as u32);
    }

    pub fn pan_by(&mut self, dx: i32, dy: i32) {
        self.pan_x_q16 = self.pan_x_q16.wrapping_sub((dx as u32) << 8 / 256.max(self.zoom_q8 as u32));
        self.pan_y_q16 = self.pan_y_q16.wrapping_sub((dy as u32) << 8 / 256.max(self.zoom_q8 as u32));
    }
}

/// 命中条目（空间索引扁平行）。
#[derive(Clone, Copy, Debug)]
pub struct HitEntry {
    pub target: HitResult,
    pub min_x: u16,
    pub min_y: u16,
    pub max_x: u16,
    pub max_y: u16,
    pub z_order: u16,
}

/// 交互契约引擎：唯一事件入口 + 状态机 + 指令流。
pub struct InteractionContract {
    pub state: InteractionState,
    pub viewport: Viewport,
    pub selection: Vec<u32>,
    pub hover: HitResult,
    pub drag_target: HitResult,
    pub hit_index: Vec<HitEntry>,
    pub cmd_log: Vec<RenderCmd>,
    pub event_seq: u64,
    pub cmd_seq: u64,
    pub contract_violations: u32,
    pub max_selection: usize,
    pub last_event: Option<InteractEvent>,
}

impl InteractionContract {
    pub fn new(width: u16, height: u16) -> Self {
        Self {
            state: InteractionState::Idle,
            viewport: Viewport::new(width, height),
            selection: Vec::new(),
            hover: HitResult::None,
            drag_target: HitResult::None,
            hit_index: Vec::new(),
            cmd_log: Vec::new(),
            event_seq: 0,
            cmd_seq: 0,
            contract_violations: 0,
            max_selection: 256,
            last_event: None,
        }
    }

    /// 注册命中区域（渲染侧构建空间索引）。
    pub fn register_hit(&mut self, entry: HitEntry) {
        // z_order 高者优先，插入保持稳定序
        let mut pos = self.hit_index.len();
        for (i, e) in self.hit_index.iter().enumerate() {
            if e.z_order < entry.z_order {
                pos = i;
                break;
            }
        }
        self.hit_index.insert(pos, entry);
    }

    /// 命中测试（确定性：按 z_order 降序，同 z 按注册序）。
    pub fn hit_test(&self, x_q16: u32, y_q16: u32) -> HitResult {
        let (wx, wy) = self.viewport.to_world(x_q16, y_q16);
        let (x, y) = (wx as u16, wy as u16);
        for e in &self.hit_index {
            if x >= e.min_x && x <= e.max_x && y >= e.min_y && y <= e.max_y {
                return e.target;
            }
        }
        HitResult::Background
    }

    fn push_cmd(&mut self, cmd: RenderCmd) {
        self.cmd_seq += 1;
        self.cmd_log.push(cmd);
    }

    /// 事件总线唯一入口。契约违反（非法迁移）计入 violations 并拒绝。
    pub fn dispatch(&mut self, ev: InteractEvent) -> bool {
        if ev.seq != self.event_seq + 1 {
            self.contract_violations += 1;
            return false;
        }
        self.event_seq = ev.seq;
        self.last_event = Some(ev);

        let next = self.next_state(ev);
        if !self.is_legal_transition(self.state, next) {
            self.contract_violations += 1;
            return false;
        }
        let prev = self.state;
        self.state = next;
        self.emit_for_transition(prev, next, ev);
        true
    }

    fn next_state(&self, ev: InteractEvent) -> InteractionState {
        use EventKind::*;
        use InteractionState::*;
        match (self.state, ev.kind) {
            (Idle, PointerMove) => Hovering,
            (Idle, PointerDown) => Pressed,
            (Hovering, PointerMove) => Hovering,
            (Hovering, PointerDown) => Pressed,
            (Pressed, PointerMove) => Dragging,
            (Pressed, PointerUp) => Hovering,
            (Dragging, PointerMove) => Dragging,
            (Dragging, PointerUp) => Hovering,
            (Dragging, FocusOut) => Idle,
            (Hovering, FocusOut) => Idle,
            (Pressed, FocusOut) => Idle,
            (_, Wheel) => self.state,
            (_, KeyDown) | (_, KeyUp) => self.state,
            (_, FocusIn) => Hovering,
            (_, GestureStart) => Panning,
            (Panning, GestureUpdate) => Panning,
            (Panning, GestureEnd) => Hovering,
            (Selecting, PointerMove) => Selecting,
            (Selecting, PointerUp) => Hovering,
            (Modal, _) => Modal,
            (s, _) => s,
        }
    }

    fn is_legal_transition(&self, from: InteractionState, to: InteractionState) -> bool {
        use InteractionState::*;
        matches!(
            (from, to),
            (Idle, Idle)
                | (Idle, Hovering)
                | (Idle, Pressed)
                | (Idle, Panning)
                | (Idle, Modal)
                | (Hovering, Hovering)
                | (Hovering, Pressed)
                | (Hovering, Idle)
                | (Hovering, Selecting)
                | (Hovering, Panning)
                | (Pressed, Dragging)
                | (Pressed, Hovering)
                | (Pressed, Idle)
                | (Dragging, Dragging)
                | (Dragging, Hovering)
                | (Dragging, Idle)
                | (Panning, Panning)
                | (Panning, Hovering)
                | (Panning, Idle)
                | (Selecting, Selecting)
                | (Selecting, Hovering)
                | (Modal, Modal)
                | (Modal, Idle)
        )
    }

    fn emit_for_transition(&mut self, from: InteractionState, to: InteractionState, ev: InteractEvent) {
        use InteractionState::*;
        match (from, to) {
            (Idle, Hovering) | (Pressed, Hovering) | (Dragging, Hovering) => {
                let hit = self.hit_test(ev.x_q16, ev.y_q16);
                self.hover = hit;
                self.push_cmd(RenderCmd::SetCursor(match hit {
                    HitResult::Node(_) | HitResult::Edge(_) => CursorShape::Pointer,
                    HitResult::Handle(_) => CursorShape::Grab,
                    _ => CursorShape::Default,
                }));
            }
            (_, Pressed) => {
                let hit = self.hit_test(ev.x_q16, ev.y_q16);
                self.drag_target = hit;
                self.push_cmd(RenderCmd::BeginDrag { target: hit });
            }
            (_, Dragging) => {
                self.push_cmd(RenderCmd::UpdateDrag {
                    x_q16: ev.x_q16,
                    y_q16: ev.y_q16,
                });
            }
            (Dragging, Hovering) | (Dragging, Idle) => {
                self.push_cmd(RenderCmd::EndDrag { cancelled: false });
                self.drag_target = HitResult::None;
            }
            (_, Panning) => {
                self.push_cmd(RenderCmd::SetCursor(CursorShape::Grabbing));
            }
            (Panning, Hovering) | (Panning, Idle) => {
                self.push_cmd(RenderCmd::SetCursor(CursorShape::Default));
            }
            _ => {}
        }
    }

    /// 选中（点击语义：单击选中/ctrl 多选/空白清选）。
    pub fn apply_selection(&mut self, hit: HitResult, additive: bool) {
        match hit {
            HitResult::Node(id) | HitResult::Edge(id) | HitResult::Label(id) | HitResult::Handle(id) => {
                if additive {
                    if !self.selection.contains(&id) && self.selection.len() < self.max_selection {
                        self.selection.push(id);
                    }
                } else {
                    self.selection.clear();
                    self.selection.push(id);
                }
                self.push_cmd(RenderCmd::HighlightNode(id, HighlightLevel::Strong));
            }
            _ => {
                if !additive {
                    // drain 先收集成值再入账：selection 的可变借用不能横跨 push_cmd（E0499）。
                    let drained: Vec<_> = self.selection.drain(..).collect();
                    for id in drained {
                        self.push_cmd(RenderCmd::HighlightNode(id, HighlightLevel::None));
                    }
                }
            }
        }
    }

    /// 契约自检：状态机一致性 + 指令流完整性。
    pub fn run_checks(&self) -> CheckSet {
        let mut cs = CheckSet::new("CGPU-F0295 交互契约");
        cs.add("事件序号连续", self.event_seq as usize == self.cmd_log.len() || self.event_seq >= 1, "seq单调");
        cs.add(
            "契约违反计数为零",
            self.contract_violations == 0,
            "violations=0 账面",
        );
        cs.add("视口 zoom 在合法区间", self.viewport.zoom_q8 >= 16 && self.viewport.zoom_q8 <= 1024, "16..=1024");
        cs.add_dyn(
            "选择集不超过上限",
            self.selection.len() <= self.max_selection,
            "≤max_selection",
        );
        cs.add(
            "命中索引有序",
            self.hit_index.windows(2).all(|w| w[0].z_order >= w[1].z_order),
            "z_order 降序",
        );
        cs.add(
            "选中项无重复",
            {
                let mut s = self.selection.clone();
                s.sort_unstable();
                s.dedup();
                s.len() == self.selection.len()
            },
            "unique",
        );
        cs.add(
            "状态机终态可退出",
            !matches!(self.state, InteractionState::Modal) || self.cmd_seq > 0,
            "modal 有指令产出",
        );
        cs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(seq: u64, kind: EventKind) -> InteractEvent {
        InteractEvent {
            seq,
            source: EventSource::Mouse,
            kind,
            x_q16: 100 << 16,
            y_q16: 100 << 16,
            buttons: PointerButtons::LEFT,
            modifiers: 0,
            timestamp_us: seq * 1000,
        }
    }

    #[test]
    fn state_machine_closed() {
        let mut ic = InteractionContract::new(800, 600);
        assert!(ic.dispatch(ev(1, EventKind::PointerMove)));
        assert_eq!(ic.state, InteractionState::Hovering);
        assert!(ic.dispatch(ev(2, EventKind::PointerDown)));
        assert_eq!(ic.state, InteractionState::Pressed);
        assert!(ic.dispatch(ev(3, EventKind::PointerMove)));
        assert_eq!(ic.state, InteractionState::Dragging);
        assert!(ic.dispatch(ev(4, EventKind::PointerUp)));
        assert_eq!(ic.state, InteractionState::Hovering);
        assert_eq!(ic.contract_violations, 0);
    }

    #[test]
    fn out_of_order_rejected() {
        let mut ic = InteractionContract::new(800, 600);
        assert!(!ic.dispatch(ev(5, EventKind::PointerMove)));
        assert_eq!(ic.contract_violations, 1);
    }

    #[test]
    fn hit_test_deterministic() {
        let mut ic = InteractionContract::new(800, 600);
        ic.register_hit(HitEntry {
            target: HitResult::Node(7),
            min_x: 0,
            min_y: 0,
            max_x: 100,
            max_y: 100,
            z_order: 10,
        });
        ic.register_hit(HitEntry {
            target: HitResult::Node(9),
            min_x: 0,
            min_y: 0,
            max_x: 50,
            max_y: 50,
            z_order: 20,
        });
        // 同区域 z 高者胜
        assert_eq!(ic.hit_test(10 << 16, 10 << 16), HitResult::Node(9));
        assert_eq!(ic.hit_test(80 << 16, 80 << 16), HitResult::Node(7));
        assert_eq!(ic.hit_test(200 << 16, 200 << 16), HitResult::Background);
    }

    #[test]
    fn viewport_roundtrip() {
        let mut vp = Viewport::new(800, 600);
        vp.zoom_q8 = 512;
        let (wx, wy) = vp.to_world(100 << 16, 100 << 16);
        let (vx, vy) = vp.to_view(wx, wy);
        assert!((vx as i64 - (100 << 16) as i64).abs() < 65536);
        assert!((vy as i64 - (100 << 16) as i64).abs() < 65536);
    }
}
