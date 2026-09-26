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

/// 一个桌面项：排序键 + 当前格位。
#[derive(Clone, Debug)]
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
        for (idx, it) in self.items.iter_mut().enumerate() {
            let col = (idx as u32) % self.cols;
            let row = (idx as u32) / self.cols;
            it.cell = (col, row);
        }
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
}
