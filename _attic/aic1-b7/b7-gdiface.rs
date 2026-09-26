
// ---------------------------------------------------------------------------
// F006 · 深化批次七：Region 裁剪核（矩形列表 Region——contains/相交可见）
//
// 主册依据（G-A-06【设计细节】）：「BitBlt 光栅操作……直通合成器提交面」——
// 合成器侧的脏区裁剪前提是 Region 语义：矩形列表上的点包含与矩形相交可见
// 判定（GDI CreateRectRgn/CombineRgn 的最小核）。零堆：定长 8 矩形。
// ---------------------------------------------------------------------------

/// Region 容量（脏区矩形上限——合成器脏区深化 F056 的量级锚）。
pub const REGION_RECT_CAP: usize = 8;

/// 矩形列表 Region（x/y/w/h——w/h ≤0 的退化矩形不入表）。
#[derive(Clone, Copy, Debug)]
pub struct Region {
    rects: [(i32, i32, i32, i32); REGION_RECT_CAP],
    n: usize,
}

impl Region {
    pub const fn new() -> Region {
        Region { rects: [(0, 0, 0, 0); REGION_RECT_CAP], n: 0 }
    }

    pub fn add_rect(&mut self, x: i32, y: i32, w: i32, h: i32) -> bool {
        if w <= 0 || h <= 0 || self.n >= REGION_RECT_CAP {
            return false;
        }
        self.rects[self.n] = (x, y, w, h);
        self.n += 1;
        true
    }

    /// 点包含（任一矩形含点即真）。
    pub fn contains(&self, x: i32, y: i32) -> bool {
        self.rects[..self.n].iter().any(|&(rx, ry, rw, rh)| {
            x >= rx && x < rx + rw && y >= ry && y < ry + rh
        })
    }

    /// 矩形相交可见（任一矩形与目标矩形有正面积交叠即真——裁剪剔除的判定核）。
    pub fn intersects(&self, x: i32, y: i32, w: i32, h: i32) -> bool {
        if w <= 0 || h <= 0 {
            return false;
        }
        self.rects[..self.n].iter().any(|&(rx, ry, rw, rh)| {
            x < rx + rw && rx < x + w && y < ry + rh && ry < y + h
        })
    }

    pub fn len(&self) -> usize {
        self.n
    }
}

/// F006 深化批次七自检。
pub fn run_gdiface_deep6_checks() -> CheckSet {
    let mut cs = CheckSet::new("F006-gdiface-deep6");
    // 1) 点包含：两矩形各自命中、缝隙与界外不命中（半开区间语义）。
    let mut rgn = Region::new();
    rgn.add_rect(0, 0, 10, 10);
    rgn.add_rect(20, 20, 5, 5);
    cs.add(
        "region_contains_two_rects",
        rgn.contains(5, 5)
            && rgn.contains(22, 21)
            && !rgn.contains(15, 5)
            && !rgn.contains(25, 25)
            && !rgn.contains(10, 10),
        "",
    );
    // 2) 相交可见：目标矩形与第一矩形交叠 → 真；完全在缝隙 → 假；退化
    //    （零宽）目标 → 假（不产生不可见绘制）。
    cs.add(
        "region_intersects_visible",
        rgn.intersects(8, 8, 4, 4)
            && !rgn.intersects(12, 12, 6, 6)
            && !rgn.intersects(30, 30, 5, 5)
            && !rgn.intersects(5, 5, 0, 5),
        "",
    );
    // 3) 容量纪律：满 8 后 add 如实拒；退化矩形（负宽）不入表。
    let mut full = Region::new();
    let mut all = true;
    for i in 0..8i32 {
        all &= full.add_rect(i * 10, 0, 5, 5);
    }
    let over = full.add_rect(100, 0, 5, 5);
    let mut deg = Region::new();
    let neg = deg.add_rect(0, 0, -3, 5);
    cs.add(
        "region_cap_and_degenerate",
        all && !over && full.len() == 8 && !neg,
        "",
    );
    cs
}
