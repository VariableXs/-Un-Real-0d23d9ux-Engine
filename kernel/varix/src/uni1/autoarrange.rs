//! F401 桌面自动排列 · 完整设计（STAR I 主册 G-I-01）。
//!
//! **判据（主册）**：四序排列正确性（各 20 项实测）；插入/删除补位；
//! 模式互斥与切换；临时移动弹回动画（F124 弹性档）。＋通12。
//!
//! **设计要点（主册）**：桌面图标两种排列哲学并存——自由网格（F084，
//! 拖哪算哪）与自动排列（按名称/大小/类型/日期四序自动归位、新增图标
//! 插入队尾、删图标后剩余的自动补位）；两模式互斥切换；自动排列下拖拽
//! 仍可临时移动但松手弹回序位。
//!
//! 本模块是排列**语义核**（纯状态机，不画像素）：网格几何、四序比较器、
//! 插入队尾、删除补位、临时移动弹回（F124 弹性档时长常量在此登记）。
//! 排序键的「大小/类型/日期」由调用方以 u64 键注入（字号/类型枚举/
//! 修改时间戳），模块不反查文件系统。
//!
//! 时间注入式（毫秒戳），无外部依赖。

use crate::checks::CheckSet;

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 网格单元（px）——与 F084 网格吸附同源口径。
pub const CELL_W: u32 = 96;
pub const CELL_H: u32 = 112;

/// 四序枚举。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortOrder {
    Name,
    Size,
    Type,
    Date,
}

/// 排列模式（互斥两态）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArrangeMode {
    /// 自由网格：拖哪算哪（F084）。
    Free,
    /// 自动排列：序位即位置。
    Auto,
}

/// 铺排方向（v7 深化）：列优先（默认——Windows 桌面纵向流动）与
/// 行优先（横向流动，宽屏偏好者）。切换即重排（Auto 模式）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlowAxis {
    /// 列优先：先填满一列再换列（默认）。
    Column,
    /// 行优先：先填满一行再换行。
    Row,
}

/// 铺排方向的索引↔格位换算（网格内回绕扫描的唯一实现）：
/// 列优先 k→(k%cols, k/cols)；行优先 k→(k/rows, k%rows)。逆算对称。
fn flow_index_to_cell(k: u32, cols: u32, rows: u32, flow: FlowAxis) -> (u32, u32) {
    match flow {
        FlowAxis::Column => (k % cols, k / cols),
        FlowAxis::Row => (k / rows, k % rows),
    }
}

fn cell_to_flow_index(c: (u32, u32), cols: u32, rows: u32, flow: FlowAxis) -> u32 {
    match flow {
        FlowAxis::Column => c.1 * cols + c.0,
        FlowAxis::Row => c.0 * rows + c.1,
    }
}

/// 选择集（v7 深化）：Ctrl 单击切换 + 框选（格位矩形区间）+
/// 全选/清空。选择集独立于排列模式——两种模式下都可多选。
pub struct Selection {
    ids: Vec<u64>,
}

impl Selection {
    pub fn new() -> Selection {
        Selection { ids: Vec::new() }
    }

    /// Ctrl 单击切换：返回切换后是否选中。
    pub fn toggle(&mut self, id: u64) -> bool {
        match self.ids.iter().position(|i| *i == id) {
            Some(pos) => {
                self.ids.remove(pos);
                false
            }
            None => {
                self.ids.push(id);
                true
            }
        }
    }

    /// 框选：把格位落在矩形区间（含边界）内的项全部加入选择。
    /// 返回本次新增数（已选中的不重复计）。
    pub fn marquee(&mut self, corner_a: (u32, u32), corner_b: (u32, u32), items: &[DeskItem]) -> usize {
        let (min_c, min_r) = (corner_a.0.min(corner_b.0), corner_a.1.min(corner_b.1));
        let (max_c, max_r) = (corner_a.0.max(corner_b.0), corner_a.1.max(corner_b.1));
        let mut added = 0;
        for it in items {
            let (c, r) = it.cell;
            if c >= min_c && c <= max_c && r >= min_r && r <= max_r && !self.ids.contains(&it.id) {
                self.ids.push(it.id);
                added += 1;
            }
        }
        added
    }

    pub fn select_all(&mut self, items: &[DeskItem]) -> usize {
        let mut added = 0;
        for it in items {
            if !self.ids.contains(&it.id) {
                self.ids.push(it.id);
                added += 1;
            }
        }
        added
    }

    pub fn clear(&mut self) {
        self.ids.clear();
    }

    pub fn contains(&self, id: u64) -> bool {
        self.ids.contains(&id)
    }

    pub fn len(&self) -> usize {
        self.ids.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    /// 选中 id 快照（登记序——框选/单击的先后序，稳定可复现）。
    pub fn ids(&self) -> &[u64] {
        &self.ids
    }
}

/// 一个桌面项：排序键 + 当前格位。#[derive(Clone, Debug)]
pub struct DeskItem {
    pub id: u64,
    pub name: &'static str,
    /// 排序键：名称序用名称，其余用调用方注入的数值键。
    pub size_key: u64,
    pub type_key: u64,
    pub date_key: u64,
    /// 当前格位（列, 行）。Free 模式下用户可拖到任意格。
    pub cell: (u32, u32),
}

/// 桌面排列状态机。
pub struct DeskArrange {
    pub mode: ArrangeMode,
    pub order: SortOrder,
    /// 铺排方向（v7：列优先默认，可切行优先——切换即重排）。
    pub flow: FlowAxis,
    items: Vec<DeskItem>,
    /// 网格列数（视口宽/CELL_W，由调用方随窗口变化更新）。
    pub cols: u32,
    /// 弹回动画进行中的项：松手时刻 + 目标格位（F124 弹性档）。
    pub snap_back_at_ms: Option<u64>,
    pub snap_back_target: Option<(u32, u32)>,
    /// 临时移动次数（诊断账）。
    pub temp_moves: u64,
}

impl DeskArrange {
    pub fn new(cols: u32) -> DeskArrange {
        DeskArrange {
            mode: ArrangeMode::Free,
            order: SortOrder::Name,
            flow: FlowAxis::Column,
            items: Vec::new(),
            cols: cols.max(1),
            snap_back_at_ms: None,
            snap_back_target: None,
            temp_moves: 0,
        }
    }

    /// 切模式（互斥切换判据）：切到 Auto 的瞬间立即按当前序重排。
    pub fn set_mode(&mut self, mode: ArrangeMode) {
        self.mode = mode;
        if mode == ArrangeMode::Auto {
            self.relayout();
        }
    }

    pub fn set_order(&mut self, order: SortOrder) {
        self.order = order;
        if self.mode == ArrangeMode::Auto {
            self.relayout();
        }
    }

    pub fn set_cols(&mut self, cols: u32) {
        self.cols = cols.max(1);
        if self.mode == ArrangeMode::Auto {
            self.relayout();
        }
    }

    /// 切铺排方向（v7）：Auto 模式下切换即重排；Free 模式只记偏好
    /// （下次切 Auto 时生效——自由网格的格位是用户的，不抢）。
    pub fn set_flow(&mut self, flow: FlowAxis) {
        self.flow = flow;
        if self.mode == ArrangeMode::Auto {
            self.relayout();
        }
    }

    /// 新增图标：Auto 模式插入队尾（序位由序决定）；Free 模式落指定格。
    pub fn insert(&mut self, item: DeskItem) {
        let mut it = item;
        if self.mode == ArrangeMode::Auto {
            it.cell = (0, 0);
            self.items.push(it);
            self.relayout();
        } else {
            self.items.push(it);
        }
    }

    /// 删除图标：Auto 模式剩余项自动补位（重排）；Free 模式原地留空。
    pub fn remove(&mut self, id: u64) -> bool {
        let before = self.items.len();
        self.items.retain(|i| i.id != id);
        let removed = self.items.len() != before;
        if removed && self.mode == ArrangeMode::Auto {
            self.relayout();
        }
        removed
    }

    /// 四序比较：返回在当前序下 a 是否应排在 b 前。
    /// 同键平局一律按 id 升序（稳定、可复现——判据「各 20 项实测」要求
    /// 结果确定，平局必须钉死）。
    fn less(&self, a: &DeskItem, b: &DeskItem) -> bool {
        let ord = match self.order {
            SortOrder::Name => a.name.cmp(b.name),
            SortOrder::Size => a.size_key.cmp(&b.size_key),
            SortOrder::Type => a.type_key.cmp(&b.type_key),
            SortOrder::Date => a.date_key.cmp(&b.date_key),
        };
        match ord {
            core::cmp::Ordering::Less => true,
            core::cmp::Ordering::Greater => false,
            core::cmp::Ordering::Equal => a.id < b.id,
        }
    }

    /// 稳定排序重排：Auto 模式下按当前序把 items 依次铺进网格
    /// （列优先换行——与 Windows 桌面纵向流动一致）。
    pub fn relayout(&mut self) {
        // 插入排序：n 小（桌面项 ≤ 数百）且要求稳定；不引 alloc::sort。
        for i in 1..self.items.len() {
            let mut j = i;
            while j > 0 {
                let should_swap = {
                    let (a, b) = (&self.items[j - 1], &self.items[j]);
                    self.less(b, a)
                };
                if should_swap {
                    self.items.swap(j - 1, j);
                    j -= 1;
                } else {
                    break;
                }
            }
        }
        let rows = self.rows();
        for (idx, it) in self.items.iter_mut().enumerate() {
            let i = idx as u32;
            // 列优先：先填满一列再换列（Windows 纵向流动）；行优先：先填满一行。
            it.cell = match self.flow {
                FlowAxis::Column => (i % self.cols, i / self.cols),
                FlowAxis::Row => (i / rows, i % rows),
            };
        }
    }

    /// 行数（行优先铺排的换行粒度——与 cols 对偶；规格上取 6 行一屏，
    /// 超出向下续——与列优先的「列满换列」对称）。
    fn rows(&self) -> u32 {
        6
    }

    /// 批量移动（v7）：Free 模式把选中组整体搬到目标格起的位置——
    /// 逐项从落点起按铺排方向网格内回绕找第一个可用格（被组外项占的
    /// 格跳过；本组项的原格视为即将腾出，可落——组内不互踩）。
    /// 查无此项诚实跳过。Auto 模式拒绝（序位即位置，返回 0）。
    pub fn batch_move(&mut self, ids: &[u64], to: (u32, u32)) -> usize {
        if self.mode != ArrangeMode::Free {
            return 0;
        }
        let (cols, rows, flow) = (self.cols, self.rows(), self.flow);
        let total = cols * rows;
        // 组外项占的格全部占用；本组格可落（腾出语义）。
        let mut occupied: Vec<(u32, u32)> = self
            .items
            .iter()
            .filter(|i| !ids.contains(&i.id))
            .map(|i| i.cell)
            .collect();
        let start = cell_to_flow_index(to, cols, rows, flow);
        let mut moved = 0;
        let mut k = 0u32;
        for &id in ids {
            // 查无此项：诚实跳过（不虚计）。
            if !self.items.iter().any(|i| i.id == id) {
                continue;
            }
            // 网格内回绕扫描（索引算术——终止有界、不出界落格）。
            // 可用格 ≥ 组员数恒成立（组员自身腾出等量格），全搬可达。
            while k < total {
                let cand = flow_index_to_cell((start + k) % total, cols, rows, flow);
                k += 1;
                if occupied.contains(&cand) {
                    continue;
                }
                if let Some(it) = self.items.iter_mut().find(|i| i.id == id) {
                    it.cell = cand;
                }
                occupied.push(cand);
                moved += 1;
                break;
            }
        }
        moved
    }

    /// 首个空格（新增图标的自由落点查询——F084 吸附的补充语义）。
    /// 全满返回 None。
    pub fn first_free_cell(&self) -> Option<(u32, u32)> {
        let (cols, rows, flow) = (self.cols, self.rows(), self.flow);
        let total = cols * rows;
        for k in 0..total {
            let cand = flow_index_to_cell(k, cols, rows, flow);
            if self.item_at(cand).is_none() {
                return Some(cand);
            }
        }
        None
    }

    /// 格位 → 项（反查：框选命中、碰撞检测共用）。
    pub fn item_at(&self, cell: (u32, u32)) -> Option<&DeskItem> {
        self.items.iter().find(|i| i.cell == cell)
    }

    /// 项 → 格位（正查）。
    pub fn cell_of(&self, id: u64) -> Option<(u32, u32)> {
        self.items.iter().find(|i| i.id == id).map(|i| i.cell)
    }

    /// 自动排列下的临时移动：拖拽可以挪，但松手登记弹回目标
    /// （F124 弹性档：320ms 弹性回弹——常量在此唯一登记）。
    /// Free 模式下此口拒绝（拖哪算哪，不需要弹回）。
    pub fn temp_move(&mut self, id: u64, to: (u32, u32), now_ms: u64) -> bool {
        if self.mode != ArrangeMode::Auto {
            return false;
        }
        if let Some(it) = self.items.iter_mut().find(|i| i.id == id) {
            it.cell = to;
            self.temp_moves += 1;
            self.snap_back_at_ms = Some(now_ms);
            self.snap_back_target = None; // 目标格由序位决定，relayout 时落定
            true
        } else {
            false
        }
    }

    /// 松手弹回：把临时移动的项放回序位。调用方在动画起点调用；
    /// 返回是否确有弹回（无临时移动 = false）。
    pub fn release_snap_back(&mut self) -> bool {
        if self.snap_back_at_ms.is_none() {
            return false;
        }
        self.snap_back_at_ms = None;
        self.relayout();
        true
    }

    /// 弹回动画时长（ms）——F124 弹性档，全模块唯一登记点。
    pub fn snap_back_duration_ms() -> u64 {
        320
    }

    /// 格位像素坐标（F084 吸附同源：格心对齐）。
    pub fn cell_origin(cell: (u32, u32)) -> (u32, u32) {
        (cell.0 * CELL_W, cell.1 * CELL_H)
    }

    pub fn items(&self) -> &[DeskItem] {
        &self.items
    }

    pub fn item(&self, id: u64) -> Option<&DeskItem> {
        self.items.iter().find(|i| i.id == id)
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 不变量：Auto 模式下 items 序 == 当前序序位，且格位无重复。
    fn invariant_ok(&self) -> bool {
        if self.mode == ArrangeMode::Free {
            return true;
        }
        for w in self.items.windows(2) {
            if self.less(&w[1], &w[0]) {
                return false;
            }
        }
        let mut seen: Vec<(u32, u32)> = Vec::new();
        for it in &self.items {
            if seen.contains(&it.cell) {
                return false;
            }
            seen.push(it.cell);
        }
        true
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F401 自检：判据逐条钉死。
pub fn run_autoarrange_checks() -> CheckSet {
    let mut set = CheckSet::new("uni1-F401");

    // 样本 20 项（判据「四序各 20 项实测」）。
    let mk = |i: u64| DeskItem {
        id: i,
        name: match i % 5 {
            0 => "alpha",
            1 => "bravo",
            2 => "charlie",
            3 => "delta",
            _ => "echo",
        },
        size_key: (i * 977) % 1000,
        type_key: i % 4,
        date_key: (i * 31) % 90,
        cell: (0, 0),
    };

    // 四序各 20 项：重排后格位序与序语义一致 + 无重复格位。
    for (order, key) in [
        (SortOrder::Name, 0u8),
        (SortOrder::Size, 1),
        (SortOrder::Type, 2),
        (SortOrder::Date, 3),
    ] {
        let mut d = DeskArrange::new(4);
        d.set_mode(ArrangeMode::Auto);
        for i in 0..20u64 {
            d.insert(mk(i));
        }
        d.set_order(order);
        let ok_shape = d.items().len() == 20
            && d.items().windows(2).all(|w| !d.less(&w[1], &w[0]))
            && d.items().iter().enumerate().all(|(i, it)| {
                it.cell == ((i as u32) % 4, (i as u32) / 4)
            });
        set.add(
            match key {
                0 => "f401-order-name-20",
                1 => "f401-order-size-20",
                2 => "f401-order-type-20",
                _ => "f401-order-date-20",
            },
            ok_shape && d.invariant_ok(),
            "",
        );
    }

    // 插入队尾：Auto 下新项排在语义序的正确位置（不是物理队尾 blindly）。
    let mut d = DeskArrange::new(4);
    d.set_mode(ArrangeMode::Auto);
    d.set_order(SortOrder::Name);
    d.insert(mk(2)); // bravo
    d.insert(mk(0)); // alpha
    d.insert(mk(3)); // charlie? id3%5=3 → delta
    set.add(
        "f401-insert-into-seq",
        d.item(0).map(|i| i.cell) == Some((0, 0))
            && d.item(2).map(|i| i.cell) == Some((1, 0))
            && d.item(3).map(|i| i.cell) == Some((2, 0)),
        "",
    );

    // 删除补位：删头项后其余整体前移一格，无空洞。
    d.remove(0);
    set.add(
        "f401-delete-reflow",
        d.len() == 2 && d.item(2).map(|i| i.cell) == Some((0, 0)) && d.item(3).map(|i| i.cell) == Some((1, 0)),
        "",
    );

    // 模式互斥与切换：Free 下 temp_move 不弹回；Auto 下弹回并复位。
    let mut f = DeskArrange::new(4);
    for i in 0..3u64 {
        f.insert(mk(i));
    }
    set.add("f401-free-no-snapback", !f.temp_move(1, (3, 3), 100), "");
    f.set_mode(ArrangeMode::Auto);
    let moved = f.temp_move(1, (3, 3), 200);
    set.add(
        "f401-auto-snapback",
        moved && DeskArrange::snap_back_duration_ms() == 320 && f.release_snap_back() && f.item(1).map(|i| i.cell) == Some((1, 0)),
        "",
    );

    // 互斥：两态只有两个值（编译期枚举保证，此处钉运行时切换往返）。
    f.set_mode(ArrangeMode::Free);
    f.set_mode(ArrangeMode::Auto);
    set.add("f401-mode-mutex-roundtrip", f.mode == ArrangeMode::Auto && f.invariant_ok(), "");

    // 格位像素：吸附口径锚定。
    set.add("f401-cell-origin", DeskArrange::cell_origin((2, 3)) == (192, 336), "");

    // ---- v7 深化：铺排方向 / 选择集 / 批量移动 / 序位反查 ----
    // （Auto 模式 relayout 按当前序排序——断言具体项须用单调 date_key
    //   锚定使序位 == id 序，杜绝「插入序 == 展示序」的错误假设。）
    let mkd = |i: u64| {
        let mut it = mk(i);
        it.date_key = i;
        it
    };

    // 行优先铺排：6 行粒度下前 6 项铺满首行，第 7/8 项换行。
    let mut r = DeskArrange::new(3);
    r.set_mode(ArrangeMode::Auto);
    r.set_order(SortOrder::Date);
    r.set_flow(FlowAxis::Row);
    for i in 0..8u64 {
        r.insert(mkd(i));
    }
    set.add(
        "f401-flow-row-major",
        r.flow == FlowAxis::Row
            && r.item(0).map(|i| i.cell) == Some((0, 0))
            && r.item(5).map(|i| i.cell) == Some((0, 5))
            && r.item(6).map(|i| i.cell) == Some((1, 0))
            && r.item(7).map(|i| i.cell) == Some((1, 1)),
        "",
    );

    // 切回列优先即重排：同一批项纵向回位（切换即重排判据）。
    r.set_flow(FlowAxis::Column);
    set.add(
        "f401-flow-switch-relayouts",
        r.flow == FlowAxis::Column
            && r.item(3).map(|i| i.cell) == Some((0, 1))
            && r.item(7).map(|i| i.cell) == Some((1, 2)),
        "",
    );

    // Free 模式切方向不抢用户格位（只记偏好）。
    let mut fr = DeskArrange::new(3);
    for i in 0..2u64 {
        fr.insert(mk(i));
    }
    let cell_before = fr.cell_of(0);
    fr.set_flow(FlowAxis::Row);
    set.add("f401-flow-free-preserved", fr.flow == FlowAxis::Row && fr.cell_of(0) == cell_before, "");

    // 选择集：Ctrl 单击切换（选 → 取消），含与不含有界。
    let mut sel = Selection::new();
    let t1 = sel.toggle(7);
    let t2 = sel.toggle(9);
    let t3 = sel.toggle(7);
    set.add(
        "f401-select-toggle",
        t1 && t2 && !t3 && sel.len() == 1 && sel.contains(9) && !sel.contains(7),
        "",
    );

    // 框选：矩形区间（含边界）命中项全部入选，已选不重复计；登记序
    // 快照与 contains 一致（items 遍历序——框选落账可回放）。
    let mut m = DeskArrange::new(3);
    m.set_mode(ArrangeMode::Auto);
    for i in 0..9u64 {
        m.insert(mk(i));
    }
    let mut mq = Selection::new();
    let added = mq.marquee((0, 0), (1, 1), m.items());
    let again = mq.marquee((0, 0), (1, 1), m.items());
    let snapshot_matches = mq.ids().len() == 4 && mq.ids().iter().all(|id| mq.contains(*id));
    set.add(
        "f401-marquee-rect",
        added == 4 && again == 0 && mq.len() == 4 && !mq.contains(8) && snapshot_matches,
        "",
    );

    // 全选/清空往返。
    mq.clear();
    let all = mq.select_all(m.items());
    mq.clear();
    set.add("f401-select-all-clear", all == 9 && mq.is_empty(), "");

    // 批量移动（Free）：组整体落位——列满换列推进 + 组外占格跳过
    // + 本组原格可落（腾出语义），全程无重复格位。
    let mut b = DeskArrange::new(3);
    for i in 0..6u64 {
        let mut it = mk(i);
        it.cell = ((i % 2) as u32, (i / 2) as u32); // (0,0)(0,1)(0,2)(1,0)(1,1)(1,2)
        b.insert(it);
    }
    let moved_n = b.batch_move(&[0, 1, 2], (2, 0));
    // 扫描序（cols=3 列优先，从 idx2 起）：(2,0)→id0；(0,1)→id1 原格
    // 腾出可落（原位）；(1,1) 组外占跳过 → (2,1)→id2。
    let mut cells: Vec<(u32, u32)> = b.items().iter().map(|i| i.cell).collect();
    cells.sort_unstable();
    let no_dup = {
        let mut d = cells.clone();
        d.dedup();
        d.len() == cells.len()
    };
    set.add(
        "f401-batch-move-yield",
        moved_n == 3
            && b.item_at((2, 0)).map(|i| i.id) == Some(0)
            && b.item_at((2, 1)).map(|i| i.id) == Some(2)
            && no_dup,
        "",
    );

    // 批量移动（Auto）：诚实拒绝——序位即位置。
    b.set_mode(ArrangeMode::Auto);
    set.add("f401-batch-move-auto-rejected", b.batch_move(&[3, 4], (2, 2)) == 0, "");

    // 查无此项诚实跳过：幽灵 id 不虚计。
    b.set_mode(ArrangeMode::Free);
    let ghost = b.batch_move(&[3, 999, 4], (2, 2));
    set.add("f401-batch-move-ghost-skip", ghost == 2, "");

    // 首空格查询：按铺排方向索引序找第一空（列优先扫描 (0,0)(1,0)(2,0)…，
    // 占用 (0,0)(1,0)(0,1)(1,1)(0,2) → 首空 (2,0)）。
    let mut f3 = DeskArrange::new(3);
    for i in 0..5u64 {
        let mut it = mk(i);
        it.cell = ((i % 2) as u32, (i / 2) as u32); // (0,0)(1,0)(0,1)(1,1)(0,2)
        f3.insert(it);
    }
    set.add("f401-first-free-cell", f3.first_free_cell() == Some((2, 0)), "");

    // 正反查对拍：cell_of(item_at(x)) == x（非空格）；查无此人归 None。
    let c = f3.cell_of(4).unwrap();
    set.add("f401-lookup-inverse", f3.item_at(c).map(|i| i.id) == Some(4) && f3.cell_of(99).is_none(), "");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk(i: u64) -> DeskItem {
        DeskItem {
            id: i,
            name: match i % 5 {
                0 => "alpha",
                1 => "bravo",
                2 => "charlie",
                3 => "delta",
                _ => "echo",
            },
            size_key: (i * 977) % 1000,
            type_key: i % 4,
            date_key: (i * 31) % 90,
            cell: (0, 0),
        }
    }

    #[test]
    fn four_orders_twenty_items_stable() {
        for order in [SortOrder::Name, SortOrder::Size, SortOrder::Type, SortOrder::Date] {
            let mut d = DeskArrange::new(4);
            d.set_mode(ArrangeMode::Auto);
            for i in 0..20u64 {
                d.insert(mk(i));
            }
            d.set_order(order);
            // 全序（无重复键的样本下）排列结果与「每次两两比较」一致。
            for w in d.items().windows(2) {
                assert!(!d.less(&w[1], &w[0]), "order={order:?} 出现逆序对");
            }
            // 格位铺满且无重复。
            let mut cells: Vec<(u32, u32)> = d.items().iter().map(|i| i.cell).collect();
            cells.sort();
            let n = cells.len();
            cells.dedup();
            assert_eq!(cells.len(), n, "格位重复");
        }
    }

    #[test]
    fn insert_and_delete_reflow() {
        let mut d = DeskArrange::new(4);
        d.set_mode(ArrangeMode::Auto);
        d.set_order(SortOrder::Date);
        for i in 0..8u64 {
            d.insert(mk(i));
        }
        assert_eq!(d.item(5).map(|i| i.cell), Some((3, 1)));
        assert_eq!(d.item(1).map(|i| i.cell), Some((3, 0)));
        d.remove(5);
        // 日期序补位：id2 从 (3,1) 前移一格，队尾之外各归其位。
        assert_eq!(d.item(2).map(|i| i.cell), Some((2, 1)));
        assert_eq!(d.item(0).map(|i| i.cell), Some((0, 0)));
        assert!(d.invariant_ok());
    }

    #[test]
    fn temp_move_snapback_elastic() {
        let mut d = DeskArrange::new(4);
        d.set_mode(ArrangeMode::Auto);
        for i in 0..6u64 {
            d.insert(mk(i));
        }
        // 临时拖到远角。
        assert!(d.temp_move(0, (3, 3), 1_000));
        assert_eq!(d.temp_moves, 1);
        assert_eq!(d.snap_back_at_ms, Some(1_000));
        assert_eq!(DeskArrange::snap_back_duration_ms(), 320, "F124 弹性档");
        // 松手弹回序位。
        assert!(d.release_snap_back());
        assert_eq!(d.item(0).map(|i| i.cell), Some((0, 0)));
        // Free 模式拒绝弹回语义。
        d.set_mode(ArrangeMode::Free);
        assert!(!d.temp_move(0, (3, 3), 2_000), "Free 模式拖哪算哪");
    }

    #[test]
    fn col_change_relayout_keeps_invariant() {
        let mut d = DeskArrange::new(2);
        d.set_mode(ArrangeMode::Auto);
        for i in 0..7u64 {
            d.insert(mk(i));
        }
        d.set_cols(5);
        assert!(d.invariant_ok());
        assert_eq!(d.item(4).map(|i| i.cell), Some((1, 1)));
        assert_eq!(d.item(5).map(|i| i.cell), Some((1, 0)));
    }

    // ---- v7 深化单测 ----

    #[test]
    fn row_major_wraps_at_row_grain() {
        let mut d = DeskArrange::new(3);
        d.set_mode(ArrangeMode::Auto);
        d.set_order(SortOrder::Date); // 单调键锚定：展示序 == id 序
        d.set_flow(FlowAxis::Row);
        for i in 0..14u64 {
            let mut it = mk(i);
            it.date_key = i;
            d.insert(it);
        }
        // 6 行粒度：第 13 项起换第 3 行。
        assert_eq!(d.item(12).map(|i| i.cell), Some((2, 0)));
        assert_eq!(d.item(13).map(|i| i.cell), Some((2, 1)));
        assert!(d.invariant_ok());
    }

    #[test]
    fn free_mode_keeps_cells_on_flow_switch() {
        let mut d = DeskArrange::new(3);
        for i in 0..3u64 {
            let mut it = mk(i);
            it.cell = (i as u32, 0);
            d.insert(it);
        }
        d.set_flow(FlowAxis::Row);
        assert_eq!(d.cell_of(1), Some((1, 0)), "Free 模式切方向不抢用户格位");
        // 切到 Auto 时记录的方向生效（行优先横向铺）。
        d.set_mode(ArrangeMode::Auto);
        assert_eq!(d.cell_of(1), Some((0, 1)));
    }

    #[test]
    fn selection_toggle_marquee_roundtrip() {
        let mut d = DeskArrange::new(3);
        d.set_mode(ArrangeMode::Auto);
        for i in 0..9u64 {
            d.insert(mk(i));
        }
        let mut sel = Selection::new();
        // 框选 (0,0)-(1,1)：名称序排序后前 5 项铺 (0,0)(0,1)(0,2)(1,0)(1,1)
        // → 命中 id0,5,1,2 四格。
        assert_eq!(sel.marquee((0, 0), (1, 1), d.items()), 4);
        assert!(sel.contains(0) && sel.contains(5) && sel.contains(6) && sel.contains(2));
        // Ctrl 单击取消其一。
        assert!(!sel.toggle(2));
        assert_eq!(sel.len(), 3);
        // 再框选同区域：只补回 1 个（2），不重复计。
        assert_eq!(sel.marquee((0, 0), (1, 1), d.items()), 1);
        assert_eq!(sel.len(), 4);
        sel.clear();
        assert!(sel.is_empty());
    }

    #[test]
    fn batch_move_group_never_overlaps() {
        let mut d = DeskArrange::new(4);
        for i in 0..12u64 {
            let mut it = mk(i);
            it.cell = ((i % 4) as u32, (i / 4) as u32);
            d.insert(it);
        }
        // 把三角上的三项搬到中心起——组外占格跳过、组内腾出。
        let n = d.batch_move(&[0, 5, 10], (1, 1));
        assert_eq!(n, 3);
        let mut cells: Vec<(u32, u32)> = d.items().iter().map(|i| i.cell).collect();
        cells.sort_unstable();
        cells.dedup();
        assert_eq!(cells.len(), 12, "批量落位后格位两两不同");
        // 幽灵 id 诚实跳过。
        assert_eq!(d.batch_move(&[0, 42], (0, 0)), 1);
        // Auto 模式拒绝。
        d.set_mode(ArrangeMode::Auto);
        assert_eq!(d.batch_move(&[0], (0, 0)), 0);
    }
}
