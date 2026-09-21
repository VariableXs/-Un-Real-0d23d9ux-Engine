# -*- coding: utf-8 -*-
"""AI-4：winsurf 补 S2.09「窗口开关 ×100 无泄漏」内核侧水位用例。"""
import io

P = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\kernel\varix\src\winsurf.rs"

CASE = '''    #[test]
    fn window_toggle_100_no_leak() {
        // S2.09 验收口径「窗口开关 ×100 无泄漏」内核侧水位：100 轮
        // register+submit+composite+unregister 后槽位全空、账本守恒。
        let mut svc = WinService::new_host();
        for round in 0..100u32 {
            let wid = svc
                .register(0x1000 + round as u32, 8, 6, PixelFormat::Bgr32)
                .unwrap_or_else(|| panic!("round {round}: register"));
            assert!(svc.attach_block(wid, 0, leak_buf(H as usize, W as usize * BPP)));
            for y in 0..H as i64 {
                assert!(svc.stage_row(wid, y, 0, &test_row(1)));
            }
            svc.mark_window_dirty(wid, Rect::new(0, 0, W as i64, H as i64));
            assert!(svc.end_submit(wid));
            let mut disp = mk_display();
            svc.composite(&mut disp);
            assert!(svc.unregister(wid), "round {round}: unregister");
            assert_eq!(svc.focus_wid(), None);
            // 水位：合成帧计数单调（每轮恰 1 帧）。
            assert_eq!(svc.stats().frames, round as u64 + 1, "frames monotonic");
        }
        // 100 轮后：任意注册仍成功（槽位零泄漏的直接证据）。
        let wid = svc.register(0xDEAD, W, H, PixelFormat::Bgr32).expect("slots must be free");
        assert!(svc.unregister(wid));
        // 行账本守恒：100 轮 × 6 行 = 600。
        assert_eq!(svc.submit_rows(), 600);
    }

'''

with io.open(P, "r", encoding="utf-8", newline="") as f:
    src = f.read()

anchor = "    #[test]\n    fn clip_window_region_unit() {"
assert anchor in src, "anchor missing"
assert "window_toggle_100_no_leak" not in src, "already applied"
src = src.replace(anchor, CASE + anchor, 1)

with io.open(P, "w", encoding="utf-8", newline="") as f:
    f.write(src)

with io.open(P, "r", encoding="utf-8") as f:
    back = f.read()
print("verify:", "window_toggle_100_no_leak" in back)
