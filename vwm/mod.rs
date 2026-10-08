//! TRINITY-500 · AI-09 VWM 窗口合成器域（F201~F225，W3）
//!
//! 实现在 [`compositor`]；本文件做 F217（VWM 自检）与 F225（域收口）。

pub mod compositor;

use crate::checks::CheckSet;
use crate::gfx::surface::Rect;
use crate::vwm::compositor::*;

/// AI-09 域自检：F201~F225 逐项登记。
pub fn run_vwm_checks() -> CheckSet {
    let mut set = CheckSet::new("vwm");
    let screen = Rect::new(0, 0, 1920, 1080);

    let mut tree = WindowTree::new();
    tree.add(Window::new(1, Rect::new(0, 0, 400, 300), "notes"));
    tree.add(Window::new(2, Rect::new(100, 100, 400, 300), "mind"));
    set.add(
        "F201 window tree",
        tree.len() == 2 && tree.get(0).unwrap().app == "notes" && tree.get(1).unwrap().z > tree.get(0).unwrap().z,
        "nodes with geometry and z",
    );

    let mut order = [0usize; MAX_WINDOWS];
    let n = tree.z_order(&mut order);
    set.add(
        "F202 z-order and occlusion",
        n == 2 && tree.get(order[1]).unwrap().id == 2 && tree.occluded_permille(tree.by_id(1).unwrap()) > 0,
        "sorted bottom to top",
    );

    set.add(
        "F203 hit test and focus",
        tree.hit(150, 150) == Some(2)
            && tree.hit(50, 50) == Some(1)
            && tree.hit(1900, 1000).is_none()
            && tree.focus(50, 50) == Some(1)
            && tree.get(tree.by_id(1).unwrap()).unwrap().focused,
        "topmost wins",
    );

    let r = Rect::new(10, 10, 400, 300);
    set.add(
        "F204 drag and resize",
        ResizeEdge::detect(&r, 10, 50) == ResizeEdge::Left
            && resize_rect(&r, ResizeEdge::Right, 50, 0).w == 450
            && resize_rect(&r, ResizeEdge::Right, -999, 0).w == MIN_W,
        "edge detection + minimum size",
    );

    set.add(
        "F205 snapping",
        SnapZone::detect(&Rect::new(0, 0, 960, 1080), &screen) == SnapZone::Left
            && SnapZone::detect(&Rect::new(0, 0, 1920, 1080), &screen) == SnapZone::Maximise
            && SnapZone::Left.apply(&screen).w == 960,
        "half / quarter / maximise",
    );

    let mut w = Window::new(9, Rect::new(20, 20, 200, 100), "x");
    let before = w.rect;
    apply_state(&mut w, WinState::Maximised, &screen);
    let maximised = w.rect;
    apply_state(&mut w, WinState::Normal, &screen);
    set.add(
        "F206 minimise/maximise/restore",
        maximised == screen && w.rect == before,
        "restore returns the saved geometry",
    );

    let mut splits = [Rect::new(0, 0, 0, 0); 4];
    set.add(
        "F207 split layouts",
        split_layout(SplitKind::Halves, &screen, &mut splits) == 2
            && splits[0].w + splits[1].w == 1920
            && split_layout(SplitKind::Quarters, &screen, &mut splits) == 4,
        "2/3/4 slot layouts",
    );

    let mut g1 = Window::new(11, Rect::new(0, 0, 10, 10), "a");
    g1.group = 5;
    let mut g2 = Window::new(12, Rect::new(0, 0, 10, 10), "b");
    g2.group = 5;
    let mut gt = WindowTree::new();
    gt.add(g1);
    gt.add(g2);
    let mut gids = [0u16; 8];
    set.add("F208 window groups", gt.group_ids(5, &mut gids) == 2, "grouped raise/minimise");

    set.add(
        "F209 alt+tab",
        tree.alt_tab(1) == Some(2) && tree.mru()[0] == 2,
        "most-recently-used cycle",
    );

    let mut staged = [Rect::new(0, 0, 0, 0); 4];
    set.add(
        "F210 stage orchestration",
        stage_layout(StageLayout::Grid, &screen, 4, &mut staged) == 4 && staged[1].x == 960,
        "grid/cascade/focus/stack",
    );

    let mut snap = [WindowSnapshot { id: 0, rect: Rect::new(0, 0, 0, 0), state: WinState::Normal, z: 0 }; MAX_WINDOWS];
    let sn = tree.snapshot(&mut snap);
    set.add("F211 snapshot/restore", sn == 2 && tree.restore(&snap[..1]) == 1, "session restore");

    let pip = pip_rect(&screen, 3, 200, 24);
    set.add(
        "F212 picture-in-picture",
        pip.w == pip.h && pip.x + pip.w == screen.right() - 24,
        "corner-anchored overlay",
    );

    let mut gw = Window::new(1, Rect::new(0, 0, 10, 10), "a");
    gw.material = WindowMaterial::Glass;
    gw.alpha = 10;
    set.add("F213 window material", effective_alpha(&gw) == min_alpha(WindowMaterial::Glass), "glass keeps translucency");

    let (p, _v) = elastic_step(200.0, 0.0, 0.0, 16.0);
    set.add("F214 elastic animation", p < 200.0 && p > 0.0, "bounce spring moves toward target");

    set.add(
        "F215 multi-instance",
        tree.instances_of("notes") == 1 && tree.can_open("notes", 2) && !tree.can_open("notes", 1),
        "instance cap",
    );

    let mut bus = EventBus::new();
    bus.push(WinEvent { kind: WinEventKind::Closed, id: 1, stamp_ms: 0 });
    set.add("F216 close forensics", bus.closed_total == 1 && bus.get(0).unwrap().id == 1, "close is recorded");

    set.add("F217 vwm self-check", set.all_passed(), "entry point");

    bus.push(WinEvent { kind: WinEventKind::Moved, id: 2, stamp_ms: 5 });
    set.add("F218 event bus", bus.len() == 2 && bus.get(1).unwrap().kind == WinEventKind::Moved, "ring buffer");

    set.add(
        "F219 vwm perf budget",
        vwm_budget_ok(8, 200) && !vwm_budget_ok(20, 200),
        "3ms window budget",
    );

    let far = Rect::new(9000, 9000, 400, 300);
    set.add(
        "F220 clamp to screen",
        clamp_to_screen(&far, &screen).x + MIN_VISIBLE <= screen.right()
            && clamp_to_screen(&Rect::new(10, 10, 100, 100), &screen) == Rect::new(10, 10, 100, 100),
        "always grabbable",
    );

    set.add(
        "F221 fullscreen protocol",
        fullscreen_allowed(100) && !fullscreen_allowed(900),
        "exclusive only when host is idle",
    );

    set.add(
        "F222 thumbnails/dock",
        thumb_rect(0, 900).w == 160 && thumb_rect(1, 900).x == 8 + 168,
        "dock strip geometry",
    );

    set.add(
        "F223 scroll sync",
        scroll_sync(250, 1000, 4000) == 1000,
        "proportional follow",
    );

    let mut dbg = [0u8; 256];
    let dn = render_tree(&tree, &mut dbg);
    set.add(
        "F224 layer debugger",
        dn > 0 && core::str::from_utf8(&dbg[..dn]).unwrap().contains("z="),
        "human-readable tree",
    );

    set.add("F225 vwm domain closure", set.all_passed(), "all above green");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f217_domain_self_test_is_green() {
        let set = run_vwm_checks();
        if !set.all_passed() {
            let mut buf = [0u8; 2048];
            let n = set.render(&mut buf);
            panic!("vwm self-test://n{}", core::str::from_utf8(&buf[..n]).unwrap());
        }
        assert_eq!(set.len(), 25);
    }
}
