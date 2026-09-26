
// ---------------------------------------------------------------------------
// F006 · 深化批次五：**GDI 真光栅引擎核心**（软件光栅器——非记账模型）
//
// 主册依据（G-A-06【功能定义】）：FillRect/LineTo/Rectangle/Ellipse 的真渲染
// 核——批次一至四的绘制面为命令记账模型，本段把光栅本体落到像素级：矩形
// 填充（裁剪）、Bresenham 直线、中点椭圆（四分对称）、多边形扫描线填充
// （偶奇规则）。零堆：调用方供 `&mut [u32]` 表面（ARGB），无分配。
// 与既有合成器提交面（B-801）的关系：本核产出即提交前的帧缓冲内容。
// ---------------------------------------------------------------------------

/// 多边形顶点上限（零堆扫描线）。
pub const POLY_VERT_CAP: usize = 16;

/// 软件光栅表面（ARGB u32——w×h 像素，调用方持有缓冲）。
pub struct RasterSurface<'a> {
    pub px: &'a mut [u32],
    pub w: usize,
    pub h: usize,
}

impl<'a> RasterSurface<'a> {
    pub fn new(px: &'a mut [u32], w: usize, h: usize) -> Option<RasterSurface<'a>> {
        if px.len() != w * h || w == 0 || h == 0 {
            return None;
        }
        Some(RasterSurface { px, w, h })
    }

    pub fn clear(&mut self, c: u32) {
        self.px.fill(c);
    }

    /// 像素读（越界 None——调试/对拍用）。
    pub fn px_at(&self, x: i32, y: i32) -> Option<u32> {
        if x < 0 || y < 0 || x >= self.w as i32 || y >= self.h as i32 {
            return None;
        }
        Some(self.px[y as usize * self.w + x as usize])
    }

    fn set_px(&mut self, x: i32, y: i32, c: u32) {
        if x >= 0 && y >= 0 && x < self.w as i32 && y < self.h as i32 {
            self.px[y as usize * self.w + x as usize] = c;
        }
    }

    /// FillRect 真渲染（负宽高语义：按 min/max 规范化——GDI Rectangle 同义；
    /// 越界部分裁剪不报错）。
    pub fn fill_rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: u32) -> bool {
        if w == 0 || h == 0 {
            return false;
        }
        let (x0, x1) = if w > 0 { (x, x + w) } else { (x + w, x) };
        let (y0, y1) = if h > 0 { (y, y + h) } else { (y + h, y) };
        for yy in y0.max(0)..y1.min(self.h as i32) {
            for xx in x0.max(0)..x1.min(self.w as i32) {
                self.set_px(xx, yy, c);
            }
        }
        true
    }

    /// 逐行填充（扫描线核的行原语）。
    fn fill_span(&mut self, y: i32, x0: i32, x1: i32, c: u32) {
        let (a, b) = if x0 <= x1 { (x0, x1) } else { (x1, x0) };
        for xx in a.max(0)..=b.min(self.w as i32 - 1) {
            self.set_px(xx, y, c);
        }
    }
}

/// Bresenham 直线（全八向——整数增量，无浮点）。
pub fn raster_line(s: &mut RasterSurface, x0: i32, y0: i32, x1: i32, y1: i32, c: u32) -> bool {
    let dx = (x1 - x0).abs();
    let dy = -(y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;
    let (mut x, mut y) = (x0, y0);
    loop {
        s.set_px(x, y, c);
        if x == x1 && y == y1 {
            return true;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
    }
}

/// 中点椭圆轮廓（四分对称——rx/ry 为半径；(rx+1)×2 为直径口径）。
pub fn raster_ellipse(s: &mut RasterSurface, cx: i32, cy: i32, rx: i32, ry: i32, c: u32) -> bool {
    if rx <= 0 || ry <= 0 {
        return false;
    }
    let rx2 = (rx * rx) as i64;
    let ry2 = (ry * ry) as i64;
    let mut x = 0i32;
    let mut y = ry;
    let mut d1 = ry2 - rx2 * ry as i64 + rx2 / 4;
    let mut dx = 2 * ry2 * x as i64;
    let mut dy = 2 * rx2 * y as i64;
    while dx < dy {
        for (sx, sy) in [(x, y), (-x, y), (x, -y), (-x, -y)] {
            s.set_px(cx + sx, cy + sy, c);
        }
        if d1 < 0 {
            x += 1;
            dx += 2 * ry2;
            d1 += dx + ry2;
        } else {
            x += 1;
            y -= 1;
            dx += 2 * ry2;
            dy -= 2 * rx2;
            d1 += dx - dy + ry2;
        }
    }
    let mut d2 = ry2 * ((x + 1) * x) as i64 + rx2 * ((y - 1) * (y - 1)) as i64 - rx2 * ry2;
    while y >= 0 {
        for (sx, sy) in [(x, y), (-x, y), (x, -y), (-x, -y)] {
            s.set_px(cx + sx, cy + sy, c);
        }
        y -= 1;
        dy -= 2 * rx2;
        if d2 > 0 {
            d2 += rx2 - 2 * dy;
        } else {
            x += 1;
            dx += 2 * ry2;
            d2 += dx - dy + rx2;
        }
    }
    true
}

/// 多边形扫描线填充（偶奇规则；水平边跳过；扫描线用半开区间 [ymin, ymax)
/// 防顶点双计）。顶点超 16 如实拒。
pub fn raster_polygon_fill(s: &mut RasterSurface, pts: &[(i32, i32)], c: u32) -> bool {
    if pts.len() < 3 || pts.len() > POLY_VERT_CAP {
        return false;
    }
    let ymin = pts.iter().map(|p| p.1).min().unwrap();
    let ymax = pts.iter().map(|p| p.1).max().unwrap();
    let mut xs = [0i32; POLY_VERT_CAP * 2];
    for y in ymin..=ymax {
        let mut n = 0usize;
        for i in 0..pts.len() {
            let a = pts[i];
            let b = pts[(i + 1) % pts.len()];
            if a.1 == b.1 {
                continue; // 水平边不贡献交叉
            }
            let (y0, y1) = if a.1 < b.1 { (a.1, b.1) } else { (b.1, a.1) };
            if y >= y0 && y < y1 {
                let x = a.0 + (b.0 - a.0) * (y - a.1) / (b.1 - a.1);
                if n < xs.len() {
                    xs[n] = x;
                    n += 1;
                }
            }
        }
        if n >= 2 {
            xs[..n].sort_unstable();
            let mut k = 0;
            while k + 1 < n {
                s.fill_span(y, xs[k], xs[k + 1], c);
                k += 2;
            }
        }
    }
    true
}

/// F006 深化批次五自检（像素级断言——真光栅的对拍是逐像素的）。
pub fn run_gdiface_deep4_checks() -> CheckSet {
    let mut cs = CheckSet::new("F006-gdiface-deep4");
    // 1) 矩形填充：内部全色、边界外原色、负宽高规范化、越界裁剪不 panic。
    let mut buf = [0u32; 16 * 16];
    let mut sf = RasterSurface::new(&mut buf, 16, 16).unwrap();
    sf.clear(0xFF00_0000);
    sf.fill_rect(4, 4, 6, 6, 0xFF00_FF00);
    let inner = sf.px_at(6, 6) == Some(0xFF00_FF00);
    let outside = sf.px_at(3, 6) == Some(0xFF00_0000) && sf.px_at(10, 10) == Some(0xFF00_0000);
    let neg_ok = sf.fill_rect(8, 8, -3, -3, 0xFF00_00FF) && sf.px_at(6, 6) == Some(0xFF00_00FF);
    let clip_ok = sf.fill_rect(14, 14, 10, 10, 0xFFFFFFFF);
    let clipped = sf.px_at(15, 15) == Some(0xFFFFFFFF);
    cs.add(
        "raster_fill_rect_pixel_exact",
        inner && outside && neg_ok && clip_ok && clipped,
        "",
    );
    // 2) Bresenham：垂直/水平/主对角线的端点与中点逐像素对。
    let mut buf2 = [0u32; 16 * 16];
    let mut sf2 = RasterSurface::new(&mut buf2, 16, 16).unwrap();
    raster_line(&mut sf2, 2, 3, 2, 8, 1);
    raster_line(&mut sf2, 0, 0, 3, 3, 2);
    cs.add(
        "raster_line_bresenham",
        sf2.px_at(2, 3) == Some(1)
            && sf2.px_at(2, 5) == Some(1)
            && sf2.px_at(2, 8) == Some(1)
            && sf2.px_at(1, 1) == Some(2)
            && sf2.px_at(3, 3) == Some(2)
            && sf2.px_at(2, 1) == Some(0),
        "",
    );
    // 3) 多边形扫描线填充：右三角内部填充、斜边外不填充；顶点超限如实拒。
    let mut buf3 = [0u32; 16 * 16];
    let mut sf3 = RasterSurface::new(&mut buf3, 16, 16).unwrap();
    let tri = [(1i32, 1i32), (11, 1), (1, 11)];
    raster_polygon_fill(&mut sf3, &tri, 7);
    let in_tri = sf3.px_at(2, 2) == Some(7) && sf3.px_at(5, 5) == Some(7);
    let out_tri = sf3.px_at(11, 5) == Some(0) && sf3.px_at(12, 2) == Some(0);
    let too_few = !raster_polygon_fill(&mut sf3, &tri[..2], 7);
    let too_many = !raster_polygon_fill(&mut sf3, &[(0i32, 0i32); 17], 7);
    cs.add(
        "raster_polygon_scanline",
        in_tri && out_tri && too_few && too_many,
        "",
    );
    // 4) 中点椭圆：四分对称锚（上下左右极点在位）+ 轮廓非填充（内部原色）。
    let mut buf4 = [0u32; 24 * 24];
    let mut sf4 = RasterSurface::new(&mut buf4, 24, 24).unwrap();
    raster_ellipse(&mut sf4, 12, 12, 8, 6, 3);
    let poles = sf4.px_at(20, 12) == Some(3)
        && sf4.px_at(4, 12) == Some(3)
        && sf4.px_at(12, 18) == Some(3)
        && sf4.px_at(12, 6) == Some(3);
    let inside_clean = sf4.px_at(12, 12) == Some(0);
    let invalid = !raster_ellipse(&mut sf4, 12, 12, 0, 6, 3);
    cs.add(
        "raster_ellipse_midpoint_symmetry",
        poles && inside_clean && invalid,
        "",
    );
    cs
}
