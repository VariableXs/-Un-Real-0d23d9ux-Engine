# 批次九编译错三连修
import io

# 1) gdiface: 数组切片直接 &a[..1]
p = "kernel/varix/src/compatstar/gdiface.rs"
s = io.open(p, encoding="utf-8").read()
old = "let n3 = regions_intersect(&a[..1].as_slice(), &edge, &mut out);"
new = "let n3 = regions_intersect(&a[..1], &edge, &mut out);"
assert s.count(old) == 1, ("gdiface", s.count(old))
io.open(p, "w", encoding="utf-8", newline="").write(s.replace(old, new))
print("gdiface fixed")

# 2) winmgr: 三元组解构
p = "kernel/varix/src/compatstar/winmgr.rs"
s = io.open(p, encoding="utf-8").read()
old = "    let (_, ins) = adjust_window_rect(spec, 0, 0);"
new = "    let (_, _, ins) = adjust_window_rect(spec, 0, 0);"
assert s.count(old) == 1, ("winmgr", s.count(old))
io.open(p, "w", encoding="utf-8", newline="").write(s.replace(old, new))
print("winmgr fixed")

# 3) dragdrop: match 穷尽性（补三条正交臂——重按重锚/拖放中忽略/闲置移动忽略）
p = "kernel/varix/src/compatstar/dragdrop.rs"
s = io.open(p, encoding="utf-8").read()
old = """    match (state, ev) {
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
    }"""
new = """    match (state, ev) {
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
        (s @ PressState::Dragging, PressEv::Move) => s,
        (PressState::Idle, PressEv::Move) => PressState::Idle,
        // 重按重锚（按压中再次按下 = 新锚点）；拖放中的按下不改变拖放态。
        (PressState::Pressed { .. }, PressEv::Down) => PressState::Pressed { x, y },
        (PressState::Dragging, PressEv::Down) => PressState::Dragging,
        (_, PressEv::Up) => PressState::Idle,
    }"""
assert s.count(old) == 1, ("dragdrop", s.count(old))
io.open(p, "w", encoding="utf-8", newline="").write(s.replace(old, new))
print("dragdrop fixed")
