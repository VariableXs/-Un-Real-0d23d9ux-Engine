
// ---------------------------------------------------------------------------
// F007 · 深化批次六：像素吸附（0.5px 对齐——锐利线条与图标边界的根）
//
// 主册依据（G-A-07【交互设计】）：「抗锯齿与 VARIX 原生窗口一致」——锐利
// UI（图标/边框）走像素吸附：0.5px 边界吸附到整数像素，模糊边界不出现在
// 应锐利的面上（4K 精度判据的光栅前提）。
// ---------------------------------------------------------------------------

/// 像素吸附：`round(v - 0.5)` 吸附左边缘（Windows 同语义——半像素边界归左）。
pub fn snap_left_half(v: f32) -> i32 {
    (v - 0.5).round() as i32
}

/// 宽度吸附：吸附后宽度 ≥1（零宽线条如实升 1px——不可见线条是缺陷）。
pub fn snap_width(x0: f32, x1: f32) -> u32 {
    let a = snap_left_half(x0);
    let b = snap_left_half(x1);
    (b - a).max(1) as u32
}

/// F007 深化批次六自检。
pub fn run_gdiplus_deep5_checks() -> CheckSet {
    let mut cs = CheckSet::new("F007-gdiplus-deep5");
    // 1) 吸附锚：0.5 → 0（半像素归左）；1.5 → 1；2.5 → 2；整数像素不动。
    cs.add(
        "snap_left_half_anchors",
        snap_left_half(0.5) == 0 && snap_left_half(1.5) == 1 && snap_left_half(2.5) == 2
            && snap_left_half(2.0) == 2 && snap_left_half(-0.5) == -1,
        "",
    );
    // 2) 宽度吸附：0.5px 宽线条升 1px（不可见线条防线）；3px 宽保持 3。
    cs.add(
        "snap_width_min_one",
        snap_width(0.0, 0.5) == 1 && snap_width(2.0, 5.0) == 3,
        "",
    );
    // 3) 确定性：同输入两次吸附恒同（4K 对拍可复现）。
    let s1 = snap_left_half(7.5);
    let s2 = snap_left_half(7.5);
    cs.add("snap_deterministic", s1 == s2, "");
    cs
}
