
// ---------------------------------------------------------------------------
// F006 · 深化批次八：Region 减法拆分核（矩形被另一个矩形切除后的剩余带——
// GDI 排除剪裁 ExcludeClipRect 的几何本体：上/下/左/右四带，最多 4 块）。
// ---------------------------------------------------------------------------

/// 拆分矩形（半开区间语义与 Region 既有面一致）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ClipRect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl ClipRect {
    pub fn right(&self) -> i32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> i32 {
        self.y + self.h
    }
    pub fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x && px < self.right() && py >= self.y && py < self.bottom()
    }
    pub fn area(&self) -> i64 {
        (self.w.max(0) as i64) * (self.h.max(0) as i64)
    }
}

/// a 减去 cut 后的剩余带（≤4 块；无交集返回原矩形单块；完全覆盖返回空表）。
pub fn rect_subtract(a: ClipRect, cut: ClipRect) -> alloc::vec::Vec<ClipRect> {
    let mut out = alloc::vec::Vec::new();
    let ix = a.x.max(cut.x);
    let iy = a.y.max(cut.y);
    let ir = a.right().min(cut.right());
    let ib = a.bottom().min(cut.bottom());
    if ix >= ir || iy >= ib {
        out.push(a); // 无交集
        return out;
    }
    // 交集上方带 / 下方带 / 左侧带 / 右侧带。
    if a.y < iy {
        out.push(ClipRect { x: a.x, y: a.y, w: a.w, h: iy - a.y });
    }
    if ib < a.bottom() {
        out.push(ClipRect { x: a.x, y: ib, w: a.w, h: a.bottom() - ib });
    }
    if a.x < ix {
        out.push(ClipRect { x: a.x, y: iy, w: ix - a.x, h: ib - iy });
    }
    if ir < a.right() {
        out.push(ClipRect { x: ir, y: iy, w: a.right() - ir, h: ib - iy });
    }
    out
}

/// F006 深化批次八自检。
fn run_gdiface_deep7_checks() -> CheckSet {
    let mut cs = CheckSet::new("F006-gdiface-deep7");
    let a = ClipRect { x: 0, y: 0, w: 100, h: 100 };
    // 1) 中央挖洞：四带覆盖除洞外全部点；洞内点全部脱离；面积守恒。
    let parts = rect_subtract(a, ClipRect { x: 40, y: 40, w: 20, h: 20 });
    let total: i64 = parts.iter().map(|p| p.area()).sum();
    let hole_free = !parts.iter().any(|p| p.contains(50, 50));
    let edge_kept = parts.iter().any(|p| p.contains(0, 0))
        && parts.iter().any(|p| p.contains(99, 99))
        && parts.iter().any(|p| p.contains(0, 50))
        && parts.iter().any(|p| p.contains(50, 0));
    cs.add(
        "rect_subtract_hole",
        parts.len() == 4
            && total == 100 * 100 - 20 * 20
            && hole_free
            && edge_kept,
        "",
    );
    // 2) 完全覆盖 → 空；无交集 → 原样单块；面积负值不炸（退化矩形）。
    let full = rect_subtract(a, ClipRect { x: -1, y: -1, w: 200, h: 200 });
    let none = rect_subtract(a, ClipRect { x: 200, y: 200, w: 5, h: 5 });
    cs.add(
        "rect_subtract_cover_and_disjoint",
        full.is_empty() && none.len() == 1 && none[0] == a,
        "",
    );
    // 3) 边缘切齐：切到右下角 → 只剩 2 带（上带 + 左带），无零面积块。
    let corner = rect_subtract(a, ClipRect { x: 60, y: 60, w: 100, h: 100 });
    cs.add(
        "rect_subtract_corner_two_bands",
        corner.len() == 2
            && corner.iter().all(|p| p.area() > 0)
            && corner.iter().map(|p| p.area()).sum::<i64>() == 100 * 100 - 40 * 40,
        "",
    );
    cs
}
