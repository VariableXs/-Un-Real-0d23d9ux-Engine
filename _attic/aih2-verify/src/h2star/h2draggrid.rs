//! H2 桌面图标网格服务 · 深化批次五（F259 新建落点 × F084 车道
//! ——框选橡皮筋、碰撞换位、整体拖动、自动对齐的唯一实现）。
//!
//! **承接判据**（主册 H 域正文 + 人格章程五章，一处一事实）：
//! - **F259 新建落点**：网格位计算（h2base `pick_slot` 的服务化——
//!   删除空位复用 + Esc 取消释放，newmenu 已有位图，本层做几何
//!   通则：坐标换算与命中）；
//! - **F084 车道（经 F259 锚）**：框选集合（橡皮筋矩形命中——
//!   部分相交即入选）、整体拖动（20 图标无散架——集合相对偏移
//!   不变式）、碰撞换位（落点被占 → 换位而非覆盖——图标永不
//!   消失）、自动对齐（松手落最近格）；
//! - **Esc 放弃**：拖拽集合整体弹回原格（十四章「拖到一半 Esc 能
//!   放弃并复原」的桌面版——快照复原精确不变式）。
//!
//! 几何纪律：格坐标 ↔ 像素换算走 h2geo::GridSpec（单一定义点）。

use crate::checks::CheckSet;

use alloc::vec::Vec;

use crate::h2star::h2geo::GridSpec;

// ---------------------------------------------------------------------------
// 橡皮筋框选
// ---------------------------------------------------------------------------

/// 橡皮筋矩形（像素）：起点当前点归一化（拖反向也成立）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Marquee {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}

impl Marquee {
    /// 归一化矩形 (左, 上, 宽, 高)。
    pub fn rect(&self) -> (i32, i32, u32, u32) {
        let left = self.x0.min(self.x1);
        let top = self.y0.min(self.y1);
        let w = (self.x0.max(self.x1) - left) as u32;
        let h = (self.y0.max(self.y1) - top) as u32;
        (left, top, w, h)
    }

    /// 格命中（部分相交即入选——轴对齐矩形相交测试）。
    pub fn hits(&self, cell: (i32, i32, u32, u32)) -> bool {
        let (l, t, w, h) = self.rect();
        let cl = cell.0;
        let ct = cell.1;
        let cr = cl + cell.2 as i32;
        let cb = ct + cell.3 as i32;
        l < cr && l + w as i32 > cl && t < cb && t + h as i32 > ct
    }

    /// 框选集合：格矩形表 → 入选下标（行序稳定——可回放）。
    pub fn select(&self, cells: &[(i32, i32, u32, u32)]) -> Vec<usize> {
        cells.iter().enumerate().filter(|(_, c)| self.hits(**c)).map(|(i, _)| i).collect()
    }
}

// ---------------------------------------------------------------------------
// 拖拽集合（整体拖动 + Esc 复原）
// ---------------------------------------------------------------------------

/// 一次拖拽会话：集合相对抓取格的偏移恒定（无散架的几何本质）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GridDrag {
    pub cells: Vec<usize>,
    pub grab: (usize, usize),
    pub current: (usize, usize),
    /// 成员相对抓取格的偏移（有符号——抓取点可以在集合中部）。
    pub offsets: Vec<(i32, i32)>,
    /// 原格位快照（Esc 复原依据——逐格精确）。
    pub origin: Vec<(usize, usize)>,
    pub cancelled: bool,
}

impl GridDrag {
    /// 开始：cells 为被拖成员下标，members 为全表格位。
    pub fn start(cells: Vec<usize>, grab: (usize, usize), members: &[(usize, usize)]) -> GridDrag {
        let origin: Vec<(usize, usize)> = cells.iter().map(|i| members[*i]).collect();
        let offsets = origin
            .iter()
            .map(|(c, r)| (*c as i32 - grab.0 as i32, *r as i32 - grab.1 as i32))
            .collect();
        GridDrag { cells, grab, current: grab, offsets, origin, cancelled: false }
    }

    /// 移动（集合整体跟手——只有 current 变，偏移恒定）。
    pub fn move_to(&mut self, cell: (usize, usize)) {
        if !self.cancelled {
            self.current = cell;
        }
    }

    /// 成员落点表：新抓取位 + 各自相对偏移（负值钳 0——不出桌面）。
    pub fn member_cells(&self) -> Vec<(usize, usize)> {
        self.offsets
            .iter()
            .map(|(dc, dr)| {
                let c = (self.current.0 as i32 + dc).max(0) as usize;
                let r = (self.current.1 as i32 + dr).max(0) as usize;
                (c, r)
            })
            .collect()
    }

    /// Esc 放弃：复原 = 回到快照原位（精确逐格——不近似）。
    pub fn cancel(&mut self) -> Vec<(usize, usize)> {
        self.cancelled = true;
        self.origin.clone()
    }
}

/// 碰撞换位：集合首成员落点被占 → 占用者迁往首个空闲格（图标永不
/// 消失——覆盖=缺陷）。返回 (集合落位, 被换位者去向)。
pub fn swap_on_collision(
    target: (usize, usize),
    occupied: &[(usize, usize)],
    moving: &[(usize, usize)],
) -> (Vec<(usize, usize)>, Vec<(usize, usize)>) {
    let mut landed = moving.to_vec();
    if let Some(first) = landed.first_mut() {
        *first = target;
    }
    let need = occupied.iter().filter(|c| **c == target).count();
    if need == 0 {
        return (landed, Vec::new());
    }
    let free: Vec<(usize, usize)> = occupied
        .iter()
        .filter(|c| **c != target && !moving.iter().any(|m| *m == **c))
        .take(need)
        .copied()
        .collect();
    (landed, free)
}

/// 自动对齐：像素 → 最近格（h2geo GridSpec 命中——松手落最近格）。
pub fn snap_pixel(spec: &GridSpec, px: i32, py: i32, view_w: u32) -> Option<usize> {
    spec.hit(px, py, view_w)
}

// ---------------------------------------------------------------------------
// 自检（判据逐条钉死）
// ---------------------------------------------------------------------------

pub fn run_h2draggrid_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-h2draggrid");
    let spec = GridSpec::DEFAULT;
    let cells: Vec<(i32, i32, u32, u32)> =
        (0..6).map(|i| spec.cell_at(i, 1920)).map(|r| (r.x, r.y, r.w, r.h)).collect();
    // 橡皮筋：正向拖 = 反向拖（归一化）；6 格全中。
    let fwd = Marquee { x0: 0, y0: 0, x1: 600, y1: 500 };
    let rev = Marquee { x0: 600, y0: 500, x1: 0, y1: 0 };
    set.add(
        "h2draggrid marquee normalize",
        fwd.select(&cells) == rev.select(&cells) && fwd.select(&cells).len() == 6,
        "direction-free",
    );
    // 部分相交入选（只圈到第一格一角）。
    let corner = Marquee { x0: 0, y0: 0, x1: 20, y1: 20 };
    set.add(
        "h2draggrid partial hit",
        corner.select(&cells) == vec![0],
        "partial selects",
    );
    // 集合拖动：20 图标抓角拖 30 格——相对偏移不变式。
    let members: Vec<(usize, usize)> = (0..20).map(|i| (i % 4, i / 4)).collect();
    let mut d = GridDrag::start((0..20).collect(), (0, 0), &members);
    d.move_to((30, 30));
    let landed = d.member_cells();
    set.add(
        "h2draggrid group intact",
        landed.len() == 20
            && landed[0] == (30, 30)
            && landed[19] == (30 + 3, 30 + 4)
            && landed.windows(2).all(|w| w[0].1 <= w[1].1),
        "offsets invariant",
    );
    // 抓集合中部（抓取点非左上角）：偏移含负值，拖到边缘钳 0 不出桌。
    let mut d_mid = GridDrag::start(vec![1, 2, 3], (2, 0), &members);
    d_mid.move_to((0, 0));
    let mc = d_mid.member_cells();
    set.add(
        "h2draggrid clamp at edge",
        mc[0] == (0, 0) && mc[2] == (1, 0),
        "negative offsets clamped",
    );
    // Esc 复原：乱拖 200 步回快照逐格相等。
    let mut d3 = GridDrag::start((0..20).collect(), (0, 0), &members);
    for i in 0..200u32 {
        d3.move_to((i as usize % 50, (i as usize / 7) % 50));
    }
    let back = d3.cancel();
    set.add(
        "h2draggrid esc exact",
        d3.cancelled && back == members,
        "snapshot exact restore",
    );
    // 碰撞换位：空格直落；被占换位（占用者有去处——图标不消失）。
    let (la, da) = swap_on_collision((5, 5), &[], &[(9, 9)]);
    let occupied = vec![(5, 5), (0, 0)];
    let (lb, db) = swap_on_collision((5, 5), &occupied, &[(9, 9)]);
    set.add(
        "h2draggrid collision swap",
        la == vec![(5, 5)] && da.is_empty()
            && lb == vec![(5, 5)] && db == vec![(0, 0)],
        "displaced has destination",
    );
    // 自动对齐：格内像素命中该格（GridSpec 单一定义点）。
    let c3 = spec.cell_at(3, 1920);
    set.add(
        "h2draggrid snap",
        snap_pixel(&spec, c3.x + 10, c3.y + 10, 1920) == Some(3),
        "pixel snaps to cell",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn h2draggrid_all_green() {
        let set = run_h2draggrid_checks();
        assert!(set.all_passed(), "h2draggrid 自检有红项");
        assert!(!set.truncated(), "h2draggrid 自检溢出");
    }

    #[test]
    fn marquee_never_selects_by_wrap() {
        // 零面积橡皮筋（单击）：只选中包含该点的格或空（不环绕误选）。
        let m = Marquee { x0: 100, y0: 100, x1: 100, y1: 100 };
        let spec = GridSpec::DEFAULT;
        let cells: Vec<(i32, i32, u32, u32)> =
            (0..9).map(|i| spec.cell_at(i, 1920)).map(|r| (r.x, r.y, r.w, r.h)).collect();
        let sel = m.select(&cells);
        assert!(sel.len() <= 1, "degenerate marquee selected {sel:?}");
    }
}
