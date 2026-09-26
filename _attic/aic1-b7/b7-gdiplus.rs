
// ---------------------------------------------------------------------------
// F007 · 深化批次七：**像素旋转 90°**（图片查看器旋转判据的光栅变换面）
//
// 主册依据（G-A-07【功能定义】）：「图像绘制(Bitmap)」的几何变换面——F105
// 相册「旋转保存 EXIF 正确写回」的光栅前提：90° 旋转的像素级正确（w×h →
// h×w，src(x,y) → dst(h-1-y, x)）。
// ---------------------------------------------------------------------------

/// 90° 顺时针旋转（src w×h → dst h×w；零堆：调用方供输出缓冲，容量恰为
/// src.len()）。几何不符 → None。
pub fn rotate90_cw(src: &[u32], w: usize, h: usize, dst: &mut [u32]) -> Option<(usize, usize)> {
    if src.len() != w * h || dst.len() != src.len() || w == 0 || h == 0 {
        return None;
    }
    for sy in 0..h {
        for sx in 0..w {
            // 顺时针：src(sx, sy) → dst(h-1-sy, sx)。
            dst[(h - 1 - sy) * w + sx] = src[sy * w + sx];
        }
    }
    Some((h, w))
}

/// F007 深化批次七自检。
pub fn run_gdiplus_deep6_checks() -> CheckSet {
    let mut cs = CheckSet::new("F007-gdiplus-deep6");
    // 1) 3×2 图形（唯一标记）→ 2×3：逐像素位置对（顺时针几何恒等式）。
    //    src (0,0)=A (1,0)=B (2,0)=C / (0,1)=D (1,1)=E (2,1)=F
    let src = [1u32, 2, 3, 4, 5, 6]; // 3×2
    let mut dst = [0u32; 6];
    let dims = rotate90_cw(&src, 3, 2, &mut dst);
    // 顺时针后 dst 为 2×3：dst(x,y) = src(y, 2-x)
    // dst 行 0 = [D(4), A(1)]? 几何核对：dst(dx,dy) = src(h-1-dy? ...) 直接展开：
    // src(sx,sy) → dst(h-1-sy, sx) = dst(1-sy, sx)：
    //   A(0,0)→dst(1,0)  B(1,0)→dst(1,1)  C(2,0)→dst(1,2)
    //   D(0,1)→dst(0,0)  E(1,1)→dst(0,1)  F(2,1)→dst(0,2)
    cs.add(
        "rotate90_cw_pixel_map",
        dims == Some((2, 3))
            && dst[0] == 4
            && dst[1] == 1
            && dst[2] == 5
            && dst[3] == 2
            && dst[4] == 6
            && dst[5] == 3,
        "",
    );
    // 2) 双旋转 = 180°：src 元素逆序（w×h 任意矩形 180° 恰为逆序——几何恒等）。
    let mut once = [0u32; 6];
    let mut twice = [0u32; 6];
    let _ = rotate90_cw(&src, 3, 2, &mut once);
    let _ = rotate90_cw(&once, 2, 3, &mut twice);
    let rev = [src[5], src[4], src[3], src[2], src[1], src[0]];
    cs.add("rotate90_twice_is_180", twice == rev, "");
    // 3) 几何不符如实 None（容量/零维）。
    let mut tiny = [0u32; 4];
    cs.add(
        "rotate90_geometry_rejected",
        rotate90_cw(&src, 3, 2, &mut tiny).is_none()
            && rotate90_cw(&src, 0, 2, &mut dst).is_none(),
        "",
    );
    cs
}
