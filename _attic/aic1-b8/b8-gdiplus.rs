
// ---------------------------------------------------------------------------
// F007 · 深化批次八：径向渐变像素核（RadialGradientBrush 的像素语义——
// 距中心沿半径线性插值；整数平方根避免浮点，确定性可对拍）。
// ---------------------------------------------------------------------------

/// 整数平方根（Newton 迭代，向下取整——u32 域）。
pub fn isqrt_u32(v: u32) -> u32 {
    if v < 2 {
        return v;
    }
    let mut x = v;
    let mut y = (x + 1) / 2;
    while y < x {
        x = y;
        y = (x + v / x) / 2;
    }
    x
}

/// 径向渐变取色（permille 整数插值）：t = dist/r，clamp 到 [0,1000]。
/// 返回 (color, t_permille)——t 一并暴露供对拍（渐变的几何可验证性）。
pub fn radial_sample(
    cx: i32,
    cy: i32,
    r: u32,
    inner: u32,
    outer: u32,
    px: i32,
    py: i32,
) -> (u32, u32) {
    let dx = (px - cx) as i64;
    let dy = (py - cy) as i64;
    let dist = isqrt_u32((dx * dx + dy * dy) as u32);
    let t = if r == 0 {
        1000
    } else {
        ((dist as u64 * 1000) / r as u64).min(1000) as u32
    };
    let mut color = 0u32;
    for shift in [0u32, 8, 16, 24] {
        let a = ((inner >> shift) & 0xFF) as u32;
        let b = ((outer >> shift) & 0xFF) as u32;
        let v = (a * (1000 - t as u32) + b * t as u32 + 500) / 1000;
        color |= (v.min(255) & 0xFF) << shift;
    }
    (color, t)
}

/// F007 深化批次八自检。
fn run_gdiplus_deep7_checks() -> CheckSet {
    let mut cs = CheckSet::new("F007-gdiplus-deep7");
    let inner = 0xFF00_0000u32; // 黑
    let outer = 0xFFFF_FFFFu32; // 白
    // 1) 中心点 t=0 → 纯内色；半径外 t=1000 → 纯外色。
    let (c0, t0) = radial_sample(50, 50, 40, inner, outer, 50, 50);
    let (cf, tf) = radial_sample(50, 50, 40, inner, outer, 200, 50);
    cs.add(
        "radial_endpoints",
        c0 == inner && t0 == 0 && cf == outer && tf == 1000,
        "",
    );
    // 2) 中点确定性：dist=20, r=40 → t=500；A 恒 255，RGB 通道线性中值 128。
    let (cm, tm) = radial_sample(50, 50, 40, inner, outer, 70, 50);
    cs.add(
        "radial_midpoint_deterministic",
        tm == 500 && cm == 0xFF80_8080,
        "",
    );
    // 3) isqrt 边界：完全平方数与邻近值（0/1/2/4/8/9/15/16/24/25）。
    cs.add(
        "isqrt_edges",
        isqrt_u32(0) == 0
            && isqrt_u32(1) == 1
            && isqrt_u32(2) == 1
            && isqrt_u32(4) == 2
            && isqrt_u32(8) == 2
            && isqrt_u32(9) == 3
            && isqrt_u32(15) == 3
            && isqrt_u32(16) == 4
            && isqrt_u32(24) == 4
            && isqrt_u32(25) == 5,
        "",
    );
    cs
}
