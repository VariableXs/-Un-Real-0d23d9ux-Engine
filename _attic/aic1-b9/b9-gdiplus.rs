
// ---------------------------------------------------------------------------
// F007 · 深化批次九：椭圆域径向渐变（椭圆 RadialGradientBrush——x/y 各按
// rx/ry 归一后取最大值：t = max(|dx|/rx, |dy|/ry)，整数 permille 可对拍；
// 圆形域是 rx==ry 的特例）。
// ---------------------------------------------------------------------------

/// 椭圆径向取色：t = clamp(max(|dx|/rx, |dy|/ry), 0, 1000) permille，
/// 逐通道线性插值（+500 四舍五入）。返回 (color, t)。
pub fn radial_ellipse_sample(
    cx: i32,
    cy: i32,
    rx: u32,
    ry: u32,
    inner: u32,
    outer: u32,
    px: i32,
    py: i32,
) -> (u32, u32) {
    let dx = (px - cx).abs() as u64;
    let dy = (py - cy).abs() as u64;
    let tx = if rx == 0 { 1000u64 } else { (dx * 1000 / rx as u64).min(1000) };
    let ty = if ry == 0 { 1000u64 } else { (dy * 1000 / ry as u64).min(1000) };
    let t = tx.max(ty) as u32;
    let mut color = 0u32;
    for shift in [0u32, 8, 16, 24] {
        let a = ((inner >> shift) & 0xFF) as u32;
        let b = ((outer >> shift) & 0xFF) as u32;
        let v = (a * (1000 - t) + b * t + 500) / 1000;
        color |= (v.min(255) & 0xFF) << shift;
    }
    (color, t)
}

/// F007 深化批次九自检。
fn run_gdiplus_deep8_checks() -> CheckSet {
    let mut cs = CheckSet::new("F007-gdiplus-deep8");
    let inner = 0xFF00_0000u32;
    let outer = 0xFFFF_FFFFu32;
    // 1) 椭圆各向异性：rx=40, ry=20 → 点 (90,60) 距中心 (50,50)：
    //    tx=1000, ty=500 → t=1000（x 主导——外色）。
    let (c1, t1) = radial_ellipse_sample(50, 50, 40, 20, inner, outer, 90, 60);
    cs.add(
        "radial_ellipse_x_dominant",
        t1 == 1000 && c1 == outer,
        "",
    );
    // 2) y 主导对照：(50,80) → dx=0, dy=30 → ty=1000（ry=20）→ 外色；
    //    若 ry=40 → ty=750 → t=750。
    let (_, ty40) = radial_ellipse_sample(50, 50, 40, 40, inner, outer, 50, 80);
    let (_, ty20) = radial_ellipse_sample(50, 50, 40, 20, inner, outer, 50, 80);
    cs.add(
        "radial_ellipse_y_dominant_scales",
        ty20 == 1000 && ty40 == 750,
        "",
    );
    // 3) 圆形退化 = 批次八径向（rx==ry 时 t 与圆一致——跨面一致性锚）。
    let (_, t_circle) = radial_ellipse_sample(50, 50, 40, 40, inner, outer, 70, 50);
    cs.add(
        "radial_circle_degenerate_consistent",
        t_circle == 500,
        "",
    );
    cs
}
