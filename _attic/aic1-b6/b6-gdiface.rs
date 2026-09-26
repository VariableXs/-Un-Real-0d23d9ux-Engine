
// ---------------------------------------------------------------------------
// F006 · 深化批次六：**光栅引擎扩面**（渐变填充 + 圆角矩形——真渲染续批）
//
// 主册依据（G-A-07【功能定义】）：「Brush(实心/渐变)」的像素级落点——线性
// 渐变逐像素插值填充；G-A-05 视觉词典的圆角矩形（VARIX 控件圆角 4px 基线，
// 乙节基线表）。基于批次五 RasterSurface（一处一事实）。
// ---------------------------------------------------------------------------

/// 圆角矩形填充（corner 为圆角半径；角部 1/4 圆内剔除——中点判据 x²+y²≤r²）。
pub fn raster_round_rect(s: &mut RasterSurface, x: i32, y: i32, w: i32, h: i32, r: i32, c: u32) -> bool {
    if w <= 0 || h <= 0 || r < 0 {
        return false;
    }
    let r = r.min(w / 2).min(h / 2); // 半径钳制（超尺寸圆角按最大可行值）
    let r2 = (r * r) as i64;
    for yy in y..y + h {
        for xx in x..x + w {
            // 角部判定：到本角圆心的距离平方 > r² → 剔除。
            let in_corner_x = xx < x + r || xx >= x + w - r;
            let in_corner_y = yy < y + r || yy >= y + h - r;
            if in_corner_x && in_corner_y {
                let cx = if xx < x + r { x + r } else { x + w - 1 - r };
                let cy = if yy < y + r { y + r } else { y + h - 1 - r };
                let dx = (xx - cx) as i64;
                let dy = (yy - cy) as i64;
                if dx * dx + dy * dy > r2 {
                    continue;
                }
            }
            s.set_px(xx, yy, c);
        }
    }
    true
}

/// 线性渐变矩形填充（水平向：颜色 c0→c1 逐列插值——G-A-07 LinearGradient 的
/// 像素落点；插值核与既有 lerp_color 同族，permille 确定性）。
pub fn raster_gradient_h(s: &mut RasterSurface, x: i32, y: i32, w: i32, h: i32, c0: u32, c1: u32) -> bool {
    if w <= 0 || h <= 0 {
        return false;
    }
    for col in 0..w {
        let t = ((col * 1000) / w.max(1)) as u32; // 0..=1000 permille
        let color = {
            let mut out = 0u32;
            for shift in [0u32, 8, 16, 24] {
                let a = ((c0 >> shift) & 0xFF) as u32;
                let b = ((c1 >> shift) & 0xFF) as u32;
                out |= (a + (b as i32 - a as i32) * t as i32 / 1000).clamp(0, 255) as u32 << shift;
            }
            out
        };
        for yy in y.max(0)..(y + h).min(s.h as i32) {
            s.set_px(x + col, yy, color);
        }
    }
    true
}

/// F006 深化批次六自检（像素级）。
pub fn run_gdiface_deep5_checks() -> CheckSet {
    let mut cs = CheckSet::new("F006-gdiface-deep5");
    // 1) 圆角矩形：中心填充、角部剔除（r=2 的 (0,0) 角点空）、边线在位。
    let mut buf = [0u32; 16 * 16];
    let mut sf = RasterSurface::new(&mut buf, 16, 16).unwrap();
    raster_round_rect(&mut sf, 2, 2, 12, 12, 2, 9);
    let center = sf.px_at(8, 8) == Some(9);
    let corner_cut = sf.px_at(2, 2) == Some(0) && sf.px_at(13, 2) == Some(0);
    let edge = sf.px_at(8, 2) == Some(9) && sf.px_at(2, 8) == Some(9);
    let bad = !raster_round_rect(&mut sf, 0, 0, 0, 5, 1, 9);
    cs.add(
        "raster_round_rect_corners",
        center && corner_cut && edge && bad,
        "",
    );
    // 2) 渐变填充：两端色精确（左 c0 右 c1）+ 中点通道中值（±1 舍入）。
    let mut buf2 = [0u32; 16 * 16];
    let mut sf2 = RasterSurface::new(&mut buf2, 16, 16).unwrap();
    raster_gradient_h(&mut sf2, 0, 4, 16, 2, 0xFF00_0000, 0xFF00_00FF);
    let left = sf2.px_at(0, 4) == Some(0xFF00_0000);
    let right = sf2.px_at(15, 4) == Some(0xFF00_00FF);
    let mid_px = sf2.px_at(8, 4).unwrap_or(0);
    let mid_b = (mid_px & 0xFF) as i32;
    cs.add(
        "raster_gradient_endpoints_and_mid",
        left && right && (mid_b - 128).abs() <= 2,
        "",
    );
    // 3) 渐变单调性：B 通道逐列单调不减（无回跳——确定性插值核）。
    let mut mono = true;
    let mut prev = -1i32;
    for xx in 0..16 {
        let v = (sf2.px_at(xx, 5).unwrap_or(0) & 0xFF) as i32;
        mono &= v >= prev;
        prev = v;
    }
    cs.add("raster_gradient_monotonic", mono, "");
    cs
}
