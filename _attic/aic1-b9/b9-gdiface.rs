
// ---------------------------------------------------------------------------
// F006 · 深化批次九：Region 两两求交（两个矩形列表 Region 的交——所有
// 跨列表矩形对的非空交收集；SelectClipRgn 双区域相交裁剪的几何核）。
// ---------------------------------------------------------------------------

/// 两个矩形求交（None = 不相交；半开区间语义）。
pub fn rects_intersect(a: ClipRect, b: ClipRect) -> Option<ClipRect> {
    let x = a.x.max(b.x);
    let y = a.y.max(b.y);
    let r = a.right().min(b.right());
    let bt = a.bottom().min(b.bottom());
    if x < r && y < bt {
        Some(ClipRect { x, y, w: r - x, h: bt - y })
    } else {
        None
    }
}

/// 两 Region 的交（列表内矩形互不相交的规范形下，交也互不相交——
/// 返回收集到的全部非空交，按 a 列表序再 b 列表序）。
pub fn regions_intersect(
    a: &[ClipRect],
    b: &[ClipRect],
    out: &mut alloc::vec::Vec<ClipRect>,
) -> usize {
    out.clear();
    for ra in a {
        for rb in b {
            if let Some(ix) = rects_intersect(*ra, *rb) {
                out.push(ix);
            }
        }
    }
    out.len()
}

/// F006 深化批次九自检。
fn run_gdiface_deep8_checks() -> CheckSet {
    let mut cs = CheckSet::new("F006-gdiface-deep8");
    let a = [
        ClipRect { x: 0, y: 0, w: 10, h: 10 },
        ClipRect { x: 20, y: 20, w: 10, h: 10 },
    ];
    let b = [
        ClipRect { x: 5, y: 5, w: 10, h: 10 },
        ClipRect { x: 100, y: 100, w: 5, h: 5 },
    ];
    let mut out = alloc::vec::Vec::new();
    // 1) 2×2 全配对：仅 a0×b0 相交于 (5,5,5,5)；b1 双双脱靶。
    let n = regions_intersect(&a, &b, &mut out);
    cs.add(
        "region_intersect_pairs",
        n == 1 && out[0] == ClipRect { x: 5, y: 5, w: 5, h: 5 },
        "",
    );
    // 2) 交的自反：与自身求交 = 原矩形（恒等判据）。
    let n2 = regions_intersect(&a, &a, &mut out);
    cs.add(
        "region_intersect_self_identity",
        n2 == 2 && out.contains(&a[0]) && out.contains(&a[1]),
        "",
    );
    // 3) 零面积接触（边贴边）不算交——半开区间下 (10,y) 不属于 (0,0,10,10)。
    let edge = [ClipRect { x: 10, y: 0, w: 5, h: 5 }];
    let n3 = regions_intersect(&a[..1].as_slice(), &edge, &mut out);
    cs.add(
        "region_intersect_edge_touch_empty",
        n3 == 0,
        "",
    );
    cs
}
