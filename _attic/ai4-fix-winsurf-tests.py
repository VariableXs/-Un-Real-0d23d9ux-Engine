# -*- coding: utf-8 -*-
"""AI-4 winsurf 测试修正落盘脚本（Edit 工具静默丢失，改用文件级替换并回读验证）。"""
import io, sys

PATH = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\kernel\varix\src\winsurf.rs"

with io.open(PATH, "r", encoding="utf-8") as f:
    src = f.read()

# ---- 修正 1：hit_test 测试的最小化断言（旧文本 → 新文本） ----
old1 = """        // 最小化窗不参与命中。
        svc.set_state(wa, WinState::Minimized);
        assert_eq!(svc.hit_test(1, 1), Some(wb), "最小化窗不可命中");
        assert_eq!(svc.hit_tests(), 5);"""
new1 = """        // 最小化窗不参与命中（点 (1,1) 只在 wa 内 → wa 隐身后无命中）。
        svc.set_state(wa, WinState::Minimized);
        assert_eq!(svc.hit_test(1, 1), None, "最小化窗不可命中且不误命中他人");
        assert_eq!(svc.hit_test(3, 3), Some(wb), "wb 区域命中不受影响");
        assert_eq!(svc.hit_tests(), 5);"""

# ---- 修正 2：two_windows 的 wb 整行红色 stage（旧 4 字节 → 整行填充） ----
old2 = """            assert!(svc.stage_row(wa, y, 0, &vec![0xFFu8; W as usize * BPP])); // 蓝
            assert!(svc.stage_row(wb, y, 0, &vec![0x00u8, 0x00, 0xFF, 0xFF])); // 红"""
new2 = """            assert!(svc.stage_row(wa, y, 0, &vec![0xFFu8; W as usize * BPP])); // 蓝（整行）
            // 红整行：每像素字节 [00 00 FF FF]（Bgr32 内存序 R=0xFF）。
            let mut red_row = vec![0u8; W as usize * BPP];
            for px in red_row.chunks_mut(BPP) {
                px.copy_from_slice(&[0x00, 0x00, 0xFF, 0xFF]);
            }
            assert!(svc.stage_row(wb, y, 0, &red_row));"""

n = 0
if old1 in src:
    src = src.replace(old1, new1, 1)
    n += 1
else:
    print("SKIP1: old1 not found (maybe already applied)")
if old2 in src:
    src = src.replace(old2, new2, 1)
    n += 1
else:
    print("SKIP2: old2 not found (maybe already applied)")

with io.open(PATH, "w", encoding="utf-8", newline="") as f:
    f.write(src)

# 回读验证（不信回执信回读）
with io.open(PATH, "r", encoding="utf-8") as f:
    back = f.read()
ok1 = new1 in back
ok2 = new2 in back
print("applied:", n, "| verify1(hit_test):", ok1, "| verify2(red_row):", ok2)
sys.exit(0 if (ok1 and ok2) else 1)
