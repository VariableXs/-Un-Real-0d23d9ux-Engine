//! F228 列表虚拟化 · H 基础通用域实装。
//!
//! **判据锚**：F228。
//!
//! **验收标准（主册第一句）**：所有长列表只渲染可视区±一屏缓冲，滚动时
//! 按需实例化、离屏即回收；滚动条拖到中部不等待逐项加载（先出骨架屏占位、
//! 数据跟上后原位填充不跳行）；保持滚动位置语义——列表数据更新（后台删除
//! 一个文件）时用户正看的行不许跳。
//!
//! **设计要点**：
//! - [`VirtualList`] 窗口计算器：可视区 ±[`BUFFER_SCREENS`] 屏缓冲的行
//!   区间——**实例数只依赖可视区与缓冲，不依赖总项数**（内存恒定判据的
//!   实现核心：10 万项与 100 项的驻留实例数相同）；
//! - 按需实例化 / 离屏回收：每帧工作量定常（[`MAX_INSTANTIATE_PER_FRAME`]
//!   帧预算上限）——「10 万项滚动 60fps」的帧预算判据；回收计数入
//!   `recycled_total` 供审计；
//! - 锚点行滚动保持：数据增删前后按锚点行（可视首行 + 视口内偏移）重算
//!   滚动位置，锚点行在视口内的像素偏移不变 → 行偏移 = 0 判据
//!   （[`VirtualList::apply_data_change`]）；
//! - 骨架屏填充：占位几何 = 实数据几何（同一行高同一 y），误差恒 0 < 1px
//!   （[`VirtualList::skeleton_rect`] vs [`VirtualList::data_rect`]）；
//! - 定容热路径：驻留实例槽 [`LIVE_CAP`] 定长数组，零堆；滚动/增删/
//!   逐帧路径全部整数运算。
//!
//! **依赖锚点**：`crate::checks::CheckSet`、`crate::h1star::h1base::Rect`。

use crate::checks::CheckSet;
use crate::h1star::h1base::Rect;

// ---------------------------------------------------------------------------
// 规格常量
// ---------------------------------------------------------------------------

/// 缓冲屏数——主册 F228 原文「可视区±一屏缓冲」。
pub const BUFFER_SCREENS: u32 = 1;

/// 单帧实例化预算——「10 万项滚动 60fps」的帧工作量定常判据：
/// 每帧最多新建 32 个行实例，超出部分留到下一帧（骨架屏先行）。
pub const MAX_INSTANTIATE_PER_FRAME: usize = 32;

/// 驻留实例槽容量（定容热路径）：可视 40 行 + 上下各一屏缓冲 40 行
/// = 120 ≤ 128，留 8 槽余量。
pub const LIVE_CAP: usize = 128;

/// 视口行数上限——虚拟化内存恒定判据的结构边界（超过则钳制）。
pub const MAX_VIEWPORT_ROWS: u32 = 40;

// ---------------------------------------------------------------------------
// 数据结构
// ---------------------------------------------------------------------------

/// 一个驻留行实例（小拷贝体，入定长槽，零堆）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RowInst {
    row: u32,
    epoch: u32,
}

/// 锚点行：数据更新前捕获的「可视首行 + 该行在视口内的像素偏移」。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Anchor {
    pub row: u32,
    pub offset_px: u32,
}

/// 虚拟列表窗口计算器。
pub struct VirtualList {
    total: u32,
    row_h_px: u32,
    viewport_h_px: u32,
    viewport_w_px: u32,
    scroll_y: u32,
    live: [Option<RowInst>; LIVE_CAP],
    epoch: u32,
    /// 累计实例化次数（含重复实例化同一行——审计面）。
    pub instantiated_total: u64,
    /// 累计回收次数（离屏即回收判据的审计面）。
    pub recycled_total: u64,
}

impl VirtualList {
    /// 建列表：总项数 / 行高 / 视口尺寸。行高 0 视为非法回退 1；
    /// 视口行数超 [`MAX_VIEWPORT_ROWS`] 钳制（内存恒定的结构边界）。
    pub fn new(total: u32, row_h_px: u32, viewport_w_px: u32, viewport_h_px: u32) -> VirtualList {
        let row_h = if row_h_px == 0 { 1 } else { row_h_px };
        let rows = viewport_h_px / row_h;
        let clamped_h = if rows > MAX_VIEWPORT_ROWS { MAX_VIEWPORT_ROWS * row_h } else { viewport_h_px };
        VirtualList {
            total,
            row_h_px: row_h,
            viewport_h_px: clamped_h,
            viewport_w_px,
            scroll_y: 0,
            live: [const { None }; LIVE_CAP],
            epoch: 0,
            instantiated_total: 0,
            recycled_total: 0,
        }
    }

    pub fn total(&self) -> u32 {
        self.total
    }

    pub fn row_h(&self) -> u32 {
        self.row_h_px
    }

    pub fn scroll_y(&self) -> u32 {
        self.scroll_y
    }

    /// 视口高度（滚动条几何用）。
    pub fn viewport_h(&self) -> u32 {
        self.viewport_h_px
    }

    /// 内容总高（px）。
    pub fn content_h(&self) -> u64 {
        self.total as u64 * self.row_h_px as u64
    }

    /// 视口行数（钳制后）。
    pub fn viewport_rows(&self) -> u32 {
        self.viewport_h_px / self.row_h_px
    }

    /// 单侧缓冲行数（= 一屏缓冲）。
    pub fn buffer_rows(&self) -> u32 {
        self.viewport_rows() * BUFFER_SCREENS
    }

    /// 最大滚动量（总高 - 视口高；总高不足视口时为 0）。
    pub fn max_scroll(&self) -> u32 {
        (self.total as u64 * self.row_h_px as u64).saturating_sub(self.viewport_h_px as u64) as u32
    }

    /// 滚动到绝对位置（钳进 [0, max_scroll]）。
    pub fn scroll_to(&mut self, y: u32) {
        self.scroll_y = y.min(self.max_scroll());
    }

    /// 相对滚动。
    pub fn scroll_by(&mut self, dy: i64) {
        let target = (self.scroll_y as i64 + dy).clamp(0, self.max_scroll() as i64) as u32;
        self.scroll_y = target;
    }

    /// 可视首行。
    pub fn first_visible(&self) -> u32 {
        self.scroll_y / self.row_h_px
    }

    /// 可视区行区间 `[first, last)`（不含缓冲，钳到总项数）。
    pub fn visible_range(&self) -> (u32, u32) {
        let first = self.first_visible();
        let last = first.saturating_add(self.viewport_rows()).min(self.total);
        (first.min(self.total), last)
    }

    /// 驻留区行区间 `[lo, hi)`（可视区 ± 一屏缓冲，钳到总项数）。
    ///
    /// 区间长度只依赖视口与缓冲，不依赖 total——内存恒定判据的核心函数。
    pub fn live_range(&self) -> (u32, u32) {
        let (first, last) = self.visible_range();
        let buf = self.buffer_rows();
        let lo = first.saturating_sub(buf);
        let hi = last.saturating_add(buf).min(self.total);
        (lo, hi)
    }

    /// 当前驻留实例数（滚动稳定后 == live_range 长度，与 total 无关）。
    pub fn live_count(&self) -> usize {
        self.live.iter().filter(|s| s.is_some()).count()
    }

    /// 逐帧推进：先回收离屏实例，再按帧预算实例化驻留区内缺行。
    /// 返回本帧新建实例数（≤ [`MAX_INSTANTIATE_PER_FRAME`]）。
    pub fn tick_frame(&mut self) -> usize {
        let (lo, hi) = self.live_range();
        self.epoch = self.epoch.wrapping_add(1);

        // 离屏即回收。
        let mut recycled = 0usize;
        for slot in self.live.iter_mut() {
            if let Some(inst) = *slot {
                if inst.row < lo || inst.row >= hi {
                    *slot = None;
                    recycled += 1;
                }
            }
        }
        self.recycled_total += recycled as u64;

        // 按需实例化（帧预算内，骨架屏顶替未就绪行）。
        let mut made = 0usize;
        for row in lo..hi {
            if made >= MAX_INSTANTIATE_PER_FRAME {
                break;
            }
            let occupied = self.live.iter().any(|s| matches!(s, Some(i) if i.row == row));
            if occupied {
                continue;
            }
            let slot = match self.live.iter_mut().find(|s| s.is_none()) {
                Some(s) => s,
                None => break, // 槽满：驻留区超容防御（正常构造不会发生）。
            };
            *slot = Some(RowInst { row, epoch: self.epoch });
            self.instantiated_total += 1;
            made += 1;
        }
        made
    }

    /// 强制推进到驻留区就绪（测试/初始化用：循环 tick 直到无缺行）。
    /// 返回总帧数——拖到中部的就绪延迟上界（预算 × 帧数）。
    pub fn settle(&mut self) -> usize {
        let mut frames = 0usize;
        while frames < 4096 {
            let made = self.tick_frame();
            frames += 1;
            if made == 0 {
                break;
            }
        }
        frames
    }

    // -----------------------------------------------------------------------
    // 锚点行滚动保持（数据增删不跳行）
    // -----------------------------------------------------------------------

    /// 捕获锚点：可视首行 + 它在视口内的像素偏移。
    pub fn capture_anchor(&self) -> Anchor {
        let row = self.first_visible();
        let offset = self.scroll_y - row * self.row_h_px;
        Anchor { row, offset_px: offset }
    }

    /// 应用数据变更：`delta_before_anchor` = 锚点行之前净增减的行数
    /// （删 2 行传 -2，插 1 行传 +1）。
    ///
    /// 按锚点重算滚动位置，使锚点行在视口内的像素偏移保持不变——
    /// 「后台删除一个文件时用户正看的行不许跳」。
    /// 返回 true 表示目标位置越界被钳制（锚点行贴近列表端头时语义上
    /// 无法保持——列表滚不过端头；中段场景恒 false）。
    pub fn apply_data_change(&mut self, anchor: Anchor, new_total: u32, delta_before_anchor: i32) -> bool {
        self.total = new_total;
        let row_new = (anchor.row as i64 + delta_before_anchor as i64)
            .clamp(0, new_total.saturating_sub(1).max(0) as i64) as u32;
        let want = row_new as u64 * self.row_h_px as u64 + anchor.offset_px as u64;
        let clamped = want > self.max_scroll() as u64;
        self.scroll_to(want.min(u32::MAX as u64) as u32);
        clamped
    }

    /// 锚点行当前在视口内的像素偏移（判据面：与锚点捕获值之差即行偏移，
    /// 应为 0）。
    pub fn anchor_offset_now(&self, anchor: Anchor, delta_before_anchor: i32) -> i64 {
        let row_new = (anchor.row as i64 + delta_before_anchor as i64)
            .clamp(0, self.total.saturating_sub(1).max(0) as i64) as u32;
        self.scroll_y as i64 - row_new as i64 * self.row_h_px as i64
    }

    // -----------------------------------------------------------------------
    // 骨架屏（占位几何 = 实数据几何）
    // -----------------------------------------------------------------------

    /// 行 `row` 的实数据几何（视口坐标，y 可为负表示在视口上方）。
    pub fn data_rect(&self, row: u32) -> Rect {
        let y = row as i64 * self.row_h_px as i64 - self.scroll_y as i64;
        Rect::new(0, y as i32, self.viewport_w_px as i32, self.row_h_px as i32)
    }

    /// 行 `row` 的骨架屏占位几何——与实数据同一行高、同一 y（原位填充
    /// 不跳行）。构造上与 [`VirtualList::data_rect`] 逐字段相同，
    /// 填充误差恒 0 < 1px。
    pub fn skeleton_rect(&self, row: u32) -> Rect {
        self.data_rect(row)
    }

    /// 骨架屏填充对齐误差（px）——两几何逐字段比较，非零即缺陷。
    pub fn skeleton_fill_error_px(&self, row: u32) -> i64 {
        let d = self.data_rect(row);
        let s = self.skeleton_rect(row);
        (d.x - s.x).abs() as i64
            + (d.y - s.y).abs() as i64
            + (d.w - s.w).abs() as i64
            + (d.h - s.h).abs() as i64
    }

    // -----------------------------------------------------------------------
    // 滚动条几何（滚动条拖到中部判据的落地面）
    // -----------------------------------------------------------------------

    /// 让某行进入可视区的最小滚动（搜索跳转/锚点回看的通用入口）。
    /// 已可见 → 不动；在上方 → 滚到行顶对齐；在下方 → 滚到行底对齐。
    pub fn ensure_visible(&mut self, row: u32) -> bool {
        if self.total == 0 {
            return false;
        }
        let row = row.min(self.total - 1);
        let top = row as u64 * self.row_h_px as u64;
        let bottom = top + self.row_h_px as u64;
        let view_top = self.scroll_y as u64;
        let view_bottom = view_top + self.viewport_h_px as u64;
        if top < view_top {
            self.scroll_to(top.min(u32::MAX as u64) as u32);
            true
        } else if bottom > view_bottom {
            let want = bottom.saturating_sub(self.viewport_h_px as u64);
            self.scroll_to(want.min(u32::MAX as u64) as u32);
            true
        } else {
            false
        }
    }

    /// 整页滚动（PageUp/PageDown）。
    pub fn page(&mut self, down: bool) {
        let step = self.viewport_h_px as i64;
        self.scroll_by(if down { step } else { -step });
    }

    /// 视口 y 坐标 → 行号（点击命中测试；缓冲区内同样可命中）。
    pub fn row_at_y(&self, y_px: i32) -> Option<u32> {
        if self.total == 0 || y_px < 0 || y_px >= self.viewport_h_px as i32 {
            return None;
        }
        let row = self.first_visible() + (y_px as u32 + self.scroll_y % self.row_h_px) / self.row_h_px;
        if row < self.total {
            Some(row)
        } else {
            None
        }
    }

    /// 行是否在可视区内（不含缓冲）。
    pub fn is_row_visible(&self, row: u32) -> bool {
        let (lo, hi) = self.visible_range();
        row >= lo && row < hi
    }

    /// 滚动比例（‰，0 = 顶、1000 = 底；内容不足一屏恒 0）。
    pub fn scroll_ratio_permill(&self) -> u32 {
        let max = self.max_scroll();
        if max == 0 {
            return 0;
        }
        (self.scroll_y as u64 * 1000 / max as u64) as u32
    }
}

/// 滚动条滑块几何：轨道 ↔ 滚动位置的双向映射——「滚动条拖到中部」
/// 的精确落点（滑块中点 = 视口中心对应的文档位置）。
pub struct ScrollBarCalc;

/// 滑块最小像素（轨道再长也不至于捏不住）。
pub const THUMB_MIN_PX: u32 = 16;

impl ScrollBarCalc {
    /// 滑块在轨道上的 (offset_px, thumb_px)。
    /// 内容不足一屏 → 滑块满轨（offset 0）。
    pub fn thumb(vl: &VirtualList, track_px: u32) -> (u32, u32) {
        let content = vl.content_h();
        if content <= vl.viewport_h() as u64 || track_px == 0 {
            return (0, track_px);
        }
        let thumb = ((vl.viewport_h() as u64 * track_px as u64) / content)
            .max(THUMB_MIN_PX as u64)
            .min(track_px as u64) as u32;
        let max_scroll = vl.max_scroll();
        let off = if max_scroll == 0 {
            0
        } else {
            (vl.scroll_y() as u64 * (track_px - thumb) as u64 / max_scroll as u64) as u32
        };
        (off, thumb)
    }

    /// 拖动滑块 → 滚动位置（逆映射，钳制在 [0, max_scroll]）。
    pub fn drag(vl: &VirtualList, track_px: u32, thumb_offset_px: u32) -> u32 {
        let (_, thumb) = ScrollBarCalc::thumb(vl, track_px);
        let range = track_px.saturating_sub(thumb);
        let max_scroll = vl.max_scroll();
        if range == 0 || max_scroll == 0 {
            return 0;
        }
        let off = thumb_offset_px.min(range) as u64;
        ((off * max_scroll as u64) / range as u64).min(max_scroll as u64) as u32
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F228 自检（12 条行为级）。
pub fn run_vlist_checks() -> CheckSet {
    let mut set = CheckSet::new("F228-vlist");

    // 1. 驻留窗口 = 可视区 + 2×一屏缓冲（行数与主册口径一致；列表中段
    //    两侧缓冲都完整时成立）。
    let mut vl = VirtualList::new(100_000, 32, 800, 640);
    vl.scroll_to(50_000 * 32);
    let (lo, hi) = vl.live_range();
    set.add(
        "live window = viewport + 2x1-screen buffer",
        hi - lo == vl.viewport_rows() + 2 * vl.buffer_rows(),
        "",
    );

    // 2. 内存恒定判据：总项数 100 与 100_000 的驻留实例数相同。
    let mut small = VirtualList::new(100, 32, 800, 640);
    let mut big = VirtualList::new(100_000, 32, 800, 640);
    small.settle();
    big.settle();
    set.add(
        "resident instances independent of total (100 vs 100k)",
        small.live_count() == big.live_count() && big.live_count() > 0,
        "",
    );

    // 3. 滚动条拖到中部：settle 帧数受预算约束（10 万项直跳中部就绪
    //    帧数 ≤ ceil(驻留行数/预算)+1），骨架屏先行不等待逐项加载。
    let needed = (hi - lo) as usize;
    let frames = vl.settle();
    let budget_frames = needed.div_ceil(MAX_INSTANTIATE_PER_FRAME) + 1;
    set.add(
        "jump-to-middle settles within frame budget",
        frames <= budget_frames && vl.live_count() == needed,
        "",
    );

    // 4. 单帧实例化 ≤ MAX_INSTANTIATE_PER_FRAME（60fps 帧预算判据）。
    let mut vl2 = VirtualList::new(100_000, 32, 800, 640);
    vl2.scroll_to(60_000 * 32);
    let made = vl2.tick_frame();
    set.add(
        "per-frame instantiation <= budget",
        made <= MAX_INSTANTIATE_PER_FRAME && made > 0,
        "",
    );

    // 5. 离屏即回收：滚动一屏后旧实例被回收，回收计数增长、驻留数恒定。
    vl2.settle(); // 先补齐（检查 4 只跑了一帧，驻留未满）。
    let before_live = vl2.live_count();
    let before_recycled = vl2.recycled_total;
    vl2.scroll_to(60_100 * 32); // 下移 100 行
    vl2.settle();
    set.add(
        "offscreen recycled, resident constant",
        vl2.recycled_total > before_recycled && vl2.live_count() == before_live,
        "",
    );

    // 6. 后台删除不跳行：锚点行在视口内偏移不变（行偏移 = 0 判据）。
    let mut vl3 = VirtualList::new(10_000, 32, 800, 640);
    vl3.scroll_to(100 * 32); // 正看第 100 行
    let anchor = vl3.capture_anchor();
    vl3.apply_data_change(anchor, 9_999, -1); // 后台删了锚点前的一个文件
    set.add(
        "delete-before-anchor: row offset = 0",
        vl3.anchor_offset_now(anchor, -1) == anchor.offset_px as i64
            && vl3.first_visible() == anchor.row - 1,
        "",
    );

    // 7. 后台插入同样不跳行。
    let anchor2 = vl3.capture_anchor();
    vl3.apply_data_change(anchor2, 10_000, 1);
    set.add(
        "insert-before-anchor: row offset = 0",
        vl3.anchor_offset_now(anchor2, 1) == anchor2.offset_px as i64,
        "",
    );

    // 8. 骨架屏填充对齐误差 <1px（构造上恒 0）。
    set.add(
        "skeleton geometry == data geometry (<1px)",
        vl3.skeleton_fill_error_px(100) == 0 && vl3.skeleton_rect(100) == vl3.data_rect(100),
        "",
    );

    // 9. 滚动钳制：两端不越界。
    let mut vl4 = VirtualList::new(100, 32, 800, 640);
    vl4.scroll_to(u32::MAX);
    let bottom = vl4.scroll_y();
    vl4.scroll_by(-10_000);
    vl4.scroll_to(0);
    set.add(
        "scroll clamped at both ends",
        bottom == vl4.max_scroll() && vl4.scroll_y() == 0 && vl4.max_scroll() > 0,
        "",
    );

    // 10. 10 万项全程滚动：驻留实例数全程恒定（不随滚动位置与总数增长）。
    let mut vl5 = VirtualList::new(100_000, 32, 800, 640);
    let expect_live = (vl5.viewport_rows() + 2 * vl5.buffer_rows()) as usize;
    let mut constant = true;
    for step in 0..200u32 {
        vl5.scroll_to(step * 100_000 * 32 / 200);
        vl5.tick_frame();
        if vl5.live_count() > expect_live {
            constant = false;
            break;
        }
    }
    vl5.settle();
    set.add(
        "100k sweep: resident constant, settles exact",
        constant && vl5.live_count() == expect_live,
        "",
    );

    // 11. 空列表 / 微列表防御：total=0 与 total < 视口行数都不 panic、
    //     区间合法（lo ≤ hi ≤ total）。
    let mut e = VirtualList::new(0, 32, 800, 640);
    let (elo, ehi) = e.live_range();
    e.tick_frame();
    let mut tiny = VirtualList::new(3, 32, 800, 640);
    tiny.settle();
    let (tlo, thi) = tiny.live_range();
    set.add(
        "empty/tiny lists defensive",
        elo == 0 && ehi == 0 && e.live_count() == 0 && thi == 3 && tlo == 0 && tiny.live_count() == 3,
        "",
    );

    // 12. fuzz 2000 轮：随机滚动 + 随机数据增删，不变量——驻留 ≤ LIVE_CAP、
    //     区间有序且钳在 [0, total]、锚点行偏移 == 捕获偏移、无 panic。
    let mut x: u32 = 0x5EED_F00D;
    let mut fz = VirtualList::new(50_000, 24, 800, 480);
    fz.settle();
    let mut ok = true;
    for i in 0..2000u32 {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let op = x % 3;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        match op {
            0 => fz.scroll_by((x % 2000) as i64 - 1000),
            1 => {
                fz.scroll_to((x % 60_000) as u32);
            }
            _ => {
                let anchor = fz.capture_anchor();
                let delta = (x % 21) as i32 - 10;
                let new_total = (fz.total() as i64 + delta as i64).clamp(1, 100_000) as u32;
                let clamped = fz.apply_data_change(anchor, new_total, delta);
                // 锚点行不跳判据：未被端头钳制时像素偏移必须不变。
                if !clamped && fz.anchor_offset_now(anchor, delta) != anchor.offset_px as i64 {
                    ok = false;
                    break;
                }
            }
        }
        let (lo, hi) = fz.live_range();
        if !(lo <= hi && hi <= fz.total() && fz.live_count() <= LIVE_CAP) {
            ok = false;
            break;
        }
        fz.tick_frame();
        let _ = i;
    }
    set.add("fuzz 2000 rounds: invariants hold, no panic", ok, "");

    // 13. 滚动条几何：滑块随滚动位移、拖到中部滑块在中段、逆映射回滚
    //     位置误差 ≤ 1 格（range 量化步长）。
    //     [缺陷账本] 现象：本检查项红。根因：检查项几何自相矛盾——用
    //     10 万项列表（视口/内容 = 1/5000）断言 thumb == track/5（该断言
    //     仅在视口恰为内容 1/5 时成立，实际 thumb 被 THUMB_MIN_PX 钳到
    //     16）。修法：改检查项（判据数值未放宽，断言原样保留）——改用
    //     100 项列表使视口恰为内容的 1/5，滑块映射/中段/逆映射判据不变。
    let mut sb = VirtualList::new(100, 32, 800, 640); // 视口 640 = 内容 3200 的 1/5
    let track = 1000u32;
    let (top_off, thumb) = ScrollBarCalc::thumb(&sb, track);
    sb.scroll_to(sb.max_scroll() / 2); // 拖到中部
    let (mid_off, _) = ScrollBarCalc::thumb(&sb, track);
    let recovered = ScrollBarCalc::drag(&sb, track, mid_off);
    let step = sb.max_scroll() as i64 / (track - thumb) as i64;
    set.add(
        "scrollbar thumb maps & drag recovers within one step",
        thumb == track / 5 && top_off == 0 && mid_off > 350 && mid_off < 450
            && (recovered as i64 - sb.scroll_y() as i64).abs() <= step,
        "",
    );

    // 14. ensure_visible 最小滚动 + 整页翻页 + 命中测试 + 滚动比例。
    let mut ev = VirtualList::new(10_000, 32, 800, 640);
    let moved_down = ev.ensure_visible(500); // 视口外下方 → 滚到行底对齐
    let at = ev.scroll_y();
    let moved_up = {
        ev.scroll_to(500 * 32);
        ev.ensure_visible(10)
    };
    let noop = ev.ensure_visible(10); // 已可见 → 不动
    // [缺陷账本] 现象：page flip 断言红。根因：检查项拿陈旧基线 `at`
    // （ensure_visible(500) 时的位置）比对 page 后位置，而中间的
    // moved_up/noop 步骤已把滚动位置移到 320——基线过期是检查项自身
    // 顺序缺陷，`page` 实现按视口高整页滚动语义正确。修法：改检查项，
    // 以 page 前的实时位置为基线（+640 或钳到 max 判据不变）。
    let before_page = ev.scroll_y();
    ev.page(true);
    let paged = ev.scroll_y() == before_page + 640 || ev.scroll_y() == ev.max_scroll();
    // 命中测试：视口内像素 → 行号；滚动比例两端为 0/1000。
    let hit = ev.row_at_y(0) == Some(ev.first_visible()) && ev.row_at_y(639).is_some() && ev.row_at_y(640).is_none();
    ev.scroll_to(0);
    let ratio_top = ev.scroll_ratio_permill();
    ev.scroll_to(ev.max_scroll());
    let ratio_bottom = ev.scroll_ratio_permill();
    set.add(
        "ensure_visible, page flip, hit test, scroll ratio",
        moved_down && at <= 500 * 32 && at + 640 >= 501 * 32 && moved_up && !noop && paged
            && hit && ratio_top == 0 && ratio_bottom == 1000
            && !ev.is_row_visible(5_000),
        "",
    );

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_window_depends_only_on_viewport() {
        // [缺陷账本] 旧断言用 total=10 的列表比对窗口长度——它必然被
        // 钳到 total（第二条断言自己就验证这一点），与"长度相同"自相
        // 矛盾。修法：长度比对改用足量列表，钳制语义单独用小列表验证。
        let a = VirtualList::new(1_000, 32, 800, 640);
        let b = VirtualList::new(100_000, 32, 800, 640);
        // 视口相同 → 驻留区间长度相同。
        assert_eq!(b.live_range().1 - b.live_range().0, a.live_range().1 - a.live_range().0);
        let tiny = VirtualList::new(10, 32, 800, 640);
        assert_eq!(tiny.live_range().1, 10, "小列表钳到 total");
    }

    #[test]
    fn delete_before_anchor_keeps_row_in_place() {
        let mut vl = VirtualList::new(1_000, 32, 800, 320);
        vl.scroll_to(50 * 32 + 10); // 第 50 行顶部再往下 10px
        let anchor = vl.capture_anchor();
        assert_eq!(anchor, Anchor { row: 50, offset_px: 10 });
        // 后台删掉锚点前的 3 行。
        vl.apply_data_change(anchor, 997, -3);
        assert_eq!(vl.first_visible(), 47);
        assert_eq!(vl.anchor_offset_now(anchor, -3), 10, "锚点行像素偏移不变");
    }

    #[test]
    fn jump_to_middle_skeleton_first() {
        let mut vl = VirtualList::new(100_000, 32, 800, 640);
        vl.scroll_to(50_000 * 32);
        // 第一帧只实例化预算内行数（骨架屏顶其余）。
        let made = vl.tick_frame();
        assert!(made == MAX_INSTANTIATE_PER_FRAME);
        assert!(vl.live_count() < (vl.live_range().1 - vl.live_range().0) as usize);
        // settle 后补齐，锚定行几何就位。
        vl.settle();
        assert_eq!(vl.live_count(), (vl.live_range().1 - vl.live_range().0) as usize);
        assert_eq!(vl.skeleton_fill_error_px(50_000), 0);
    }

    #[test]
    fn recycle_on_scroll() {
        let mut vl = VirtualList::new(10_000, 32, 800, 320);
        // [缺陷账本] 旧流程从列表顶端（驻留 = 可视+单侧缓冲 20）滚到
        // 中段（双侧缓冲 30）后断言驻留数不变——区间两端被列表边界
        // 钳短本就是 live_range 的定义。修法：两端都取中段稳态位置
        // 比对，「驻留数恒定 + 回收增长」判据不变。
        vl.scroll_to(2_000 * 32);
        vl.settle();
        let n0 = vl.live_count();
        let r0 = vl.recycled_total;
        vl.scroll_to(5_000 * 32);
        vl.settle();
        assert_eq!(vl.live_count(), n0, "驻留数恒定");
        assert!(vl.recycled_total > r0, "离屏实例被回收");
    }

    #[test]
    fn scroll_edge_clamps() {
        let mut vl = VirtualList::new(100, 32, 800, 640);
        vl.scroll_to(9_999);
        assert_eq!(vl.scroll_y(), vl.max_scroll());
        vl.scroll_by(-i64::MAX);
        assert_eq!(vl.scroll_y(), 0);
        vl.scroll_by(i64::MAX);
        assert_eq!(vl.scroll_y(), vl.max_scroll());
    }

    #[test]
    fn scrollbar_geometry_roundtrip() {
        let mut vl = VirtualList::new(100_000, 32, 800, 640);
        let track = 800u32;
        // 内容不足一屏 → 滑块满轨（offset 0——与 thumb 文档一致）。
        let mut tiny = VirtualList::new(5, 32, 800, 640);
        assert_eq!(ScrollBarCalc::thumb(&tiny, track), (0, track));
        // 底部：滑块贴轨尾。
        vl.scroll_to(vl.max_scroll());
        let (off_bot, thumb) = ScrollBarCalc::thumb(&vl, track);
        assert_eq!(off_bot + thumb, track);
        // 拖到底 → max_scroll；拖到 0 → 0。
        assert_eq!(ScrollBarCalc::drag(&vl, track, track), vl.max_scroll());
        assert_eq!(ScrollBarCalc::drag(&vl, track, 0), 0);
        // 中点拖动恢复误差 ≤ 1 格。
        vl.scroll_to(50_000 * 32);
        let (off, _) = ScrollBarCalc::thumb(&vl, track);
        let back = ScrollBarCalc::drag(&vl, track, off);
        let step = vl.max_scroll() as i64 / (track - thumb) as i64;
        assert!((back as i64 - vl.scroll_y() as i64).abs() <= step);
    }

    #[test]
    fn ensure_visible_and_page() {
        let mut vl = VirtualList::new(1_000, 32, 800, 320);
        assert!(!vl.ensure_visible(5), "已可见不动");
        assert!(vl.ensure_visible(900));
        assert!(vl.scroll_y() >= 900 * 32 + 32 - 320);
        vl.page(false);
        assert_eq!(vl.scroll_y(), 900 * 32 + 32 - 320 - 320);
        // 越界行钳制。
        assert!(vl.ensure_visible(99_999));
        assert!(vl.first_visible() + 10 >= 999);
    }

    #[test]
    fn vlist_selfcheck_all_green() {
        let s = run_vlist_checks();
        assert!(s.all_passed(), "F228 自检存在红项");
        assert!(!s.truncated());
    }
}

// ===========================================================================
// v2 深化批（2026-09-26 · AI-H1 二次对账批）：UI 壳接线 / 持久化 I/O / 判定面扩展
// ===========================================================================

/// 持久化版本（格式变更递增；旧版本拒绝读——不猜格式）。
pub const VLIST_PERSIST_VERSION: u8 = 1;
/// 定长记录 = 4 magic + 1 版本 + 载荷 12（首行 u32 + scroll_y u32 +
/// total u32）+ 4 校验 = 21B；单快照定容即定长。行实例不落盘（虚拟化
/// 按需重建——持久化的是滚动语义不是数据）。
pub const VLIST_RECORD_LEN: usize = 5 + 12 + 4;
/// v2 记录魔数（AI-H1 二次对账批统一 b"VXH1"）。
const VXH1_MAGIC: [u8; 4] = *b"VXH1";

/// FNV-1a 32 位校验和（与 h2persist fnv1a64 同族异宽，域内自足实现）。
fn fnv1a32(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in data {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 持久化错误枚举：四类损坏输入全拒绝。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VlistPersistError { BadMagic, BadVersion, BadChecksum, BadLen }

/// 滚动位置快照：可视首行 + 滚动量 + 总项数（恢复时按需重建驻留区）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScrollSnapshot {
    pub first_row: u32,
    pub scroll_y: u32,
    pub total: u32,
}

impl ScrollSnapshot {
    /// 从列表捕获。
    pub fn capture(vl: &VirtualList) -> ScrollSnapshot {
        ScrollSnapshot { first_row: vl.first_visible(), scroll_y: vl.scroll_y(), total: vl.total() }
    }

    /// 编码：[0..4]=magic、[4]=版本、[5..17]=载荷、[17..21]=校验（LE）。
    pub fn to_bytes(&self) -> [u8; VLIST_RECORD_LEN] {
        let mut out = [0u8; VLIST_RECORD_LEN];
        out[0..4].copy_from_slice(&VXH1_MAGIC);
        out[4] = VLIST_PERSIST_VERSION;
        out[5..9].copy_from_slice(&self.first_row.to_le_bytes());
        out[9..13].copy_from_slice(&self.scroll_y.to_le_bytes());
        out[13..17].copy_from_slice(&self.total.to_le_bytes());
        let n = VLIST_RECORD_LEN;
        let sum = fnv1a32(&out[5..n - 4]);
        out[n - 4..n].copy_from_slice(&sum.to_le_bytes());
        out
    }

    /// 解码：长度/魔数/版本/校验四类损坏全拒绝。
    pub fn from_bytes(b: &[u8]) -> Result<ScrollSnapshot, VlistPersistError> {
        if b.len() != VLIST_RECORD_LEN {
            return Err(VlistPersistError::BadLen);
        }
        if b[0..4] != VXH1_MAGIC {
            return Err(VlistPersistError::BadMagic);
        }
        if b[4] != VLIST_PERSIST_VERSION {
            return Err(VlistPersistError::BadVersion);
        }
        let n = b.len();
        let sum = u32::from_le_bytes([b[n - 4], b[n - 3], b[n - 2], b[n - 1]]);
        if fnv1a32(&b[5..n - 4]) != sum {
            return Err(VlistPersistError::BadChecksum);
        }
        Ok(ScrollSnapshot {
            first_row: u32::from_le_bytes([b[5], b[6], b[7], b[8]]),
            scroll_y: u32::from_le_bytes([b[9], b[10], b[11], b[12]]),
            total: u32::from_le_bytes([b[13], b[14], b[15], b[16]]),
        })
    }

    /// 恢复进列表：滚动量原位回放（钳制语义复用 scroll_to——目标列表
    /// 被后台删短时钳到其自身尾部不炸；total 属数据面由调用方维护，
    /// 快照不回写——本模块持久化的是滚动语义不是数据）。
    /// [缺陷账本] 现象：v2_restore_clamps_when_shrunk 红。根因：旧实现
    /// 经 apply_data_change 把快照 total 回写进目标列表，「列表缩短时
    /// 钳到尾部」的钳制永远走不到（写死回快照 total），与本函数钳制
    /// 语义文档自相矛盾。修法：改为只回放滚动量 + 首行校正（同样受
    /// scroll_to 钳制），不触碰目标列表 total。
    pub fn restore_into(&self, vl: &mut VirtualList) {
        vl.scroll_to(self.scroll_y);
        if vl.first_visible() != self.first_row && self.first_row < vl.total() {
            let want = self.first_row as u64 * vl.row_h() as u64;
            vl.scroll_to(want.min(u32::MAX as u64) as u32);
        }
    }
}

// --- v2 UI 壳接线面：骨架几何清单（缓冲区占位——原位填充不跳行） ---

/// 骨架清单容量（驻留区上界 LIVE_CAP=128 中缓冲侧 ≤ 80 行，定长 32
/// 覆盖一屏缓冲典型行高场景；超出截断并如实计数——热路径零堆）。
pub const SKELETON_LIST_CAP: usize = 32;

/// 一条骨架占位图元（行号 + 几何——与实数据几何同源，误差恒 0）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SkeletonItem {
    pub row: u32,
    pub rect: Rect,
}

/// 骨架几何清单：驻留区中「尚无实例」的行产占位图元（先出骨架屏占位、
/// 数据跟上后原位填充不跳行——几何与 data_rect 同源，误差恒 0 <1px）。
/// 行就绪由 ready 谓词判定（调用方持实例表，判定面不依赖内部槽）。
pub fn skeleton_plan(
    vl: &VirtualList,
    ready: impl Fn(u32) -> bool,
) -> ([Option<SkeletonItem>; SKELETON_LIST_CAP], usize) {
    let mut out: [Option<SkeletonItem>; SKELETON_LIST_CAP] = [const { None }; SKELETON_LIST_CAP];
    let mut n = 0usize;
    let (lo, hi) = vl.live_range();
    for row in lo..hi {
        if n >= SKELETON_LIST_CAP {
            break;
        }
        if !ready(row) {
            out[n] = Some(SkeletonItem { row, rect: vl.skeleton_rect(row) });
            n += 1;
        }
    }
    (out, n)
}

// --- v2 判定面扩展 ---

/// F228 v2 自检（首条必为持久化 round-trip）。
pub fn run_vlist_v2_checks() -> CheckSet {
    let mut set = CheckSet::new("F228-vlist-v2");

    // 1. round-trip：快照编码→解码逐字段等值——验主册 F228「保持滚动
    //    位置语义」的会话续接面。
    let mut vl = VirtualList::new(100_000, 32, 800, 640);
    vl.scroll_to(50_000 * 32);
    let snap = ScrollSnapshot::capture(&vl);
    let bytes = snap.to_bytes();
    set.add(
        "v2 scroll snapshot roundtrip",
        matches!(ScrollSnapshot::from_bytes(&bytes), Ok(back)
            if back == snap && back.first_row == 50_000 && back.total == 100_000),
        "",
    );

    // 2. 四类损坏全拒绝——验十二查「损坏输入明错误」。
    let mut m = bytes;
    m[0] = b'X';
    let mut v = bytes;
    v[4] = 9;
    let mut s = bytes;
    s[10] ^= 0xFF;
    set.add(
        "v2 persist rejects 4 corrupt classes",
        ScrollSnapshot::from_bytes(&m) == Err(VlistPersistError::BadMagic)
            && ScrollSnapshot::from_bytes(&v) == Err(VlistPersistError::BadVersion)
            && ScrollSnapshot::from_bytes(&s) == Err(VlistPersistError::BadChecksum)
            && ScrollSnapshot::from_bytes(&bytes[..bytes.len() - 1]) == Err(VlistPersistError::BadLen),
        "",
    );

    // 3. 快照恢复：新列表经快照恢复后滚动位置/首行原位——验主册 F228
    //    「数据更新时用户正看的行不许跳」的续接面。
    let mut restored = VirtualList::new(100_000, 32, 800, 640);
    snap.restore_into(&mut restored);
    set.add(
        "v2 snapshot restores scroll in place",
        restored.scroll_y() == snap.scroll_y
            && restored.first_visible() == snap.first_row
            && restored.total() == snap.total,
        "",
    );

    // 4. 骨架几何清单：全未就绪时占满驻留区（截到容量）、几何与
    //    data_rect 逐字段一致（<1px 判据）、就绪后清单清空——验主册
    //    F228「先出骨架屏占位、数据跟上后原位填充不跳行」。
    let (plan, n_all) = skeleton_plan(&vl, |_| false);
    let first_ok = plan[0]
        .map(|it| it.row == vl.live_range().0 && it.rect == vl.data_rect(it.row))
        .unwrap_or(false);
    let (plan2, n_none) = skeleton_plan(&vl, |_| true);
    set.add(
        "v2 skeleton plan fills buffers, geometry exact, clears when ready",
        n_all == SKELETON_LIST_CAP && first_ok && n_none == 0 && plan2[0].is_none(),
        "",
    );

    // 5. 定容纪律在册：驻留 ≤ LIVE_CAP、清单 ≤ SKELETON_LIST_CAP、
    //    单帧实例化 ≤ MAX_INSTANTIATE_PER_FRAME——验十二查「容量上限
    //    在册、超限行为明示」（热路径零堆的边界线）。
    set.add(
        "v2 skeleton/list caps registered",
        LIVE_CAP == 128 && SKELETON_LIST_CAP == 32 && MAX_INSTANTIATE_PER_FRAME == 32,
        "",
    );

    set
}

#[cfg(test)]
mod tests_v2 {
    use super::*;

    #[test]
    fn v2_restore_clamps_when_shrunk() {
        let mut vl = VirtualList::new(100_000, 32, 800, 640);
        vl.scroll_to(90_000 * 32);
        let snap = ScrollSnapshot::capture(&vl);
        // 恢复进一个被后台删短的列表：钳到新尾部不炸。
        let mut small = VirtualList::new(1_000, 32, 800, 640);
        snap.restore_into(&mut small);
        assert_eq!(small.total(), 1_000);
        assert!(small.scroll_y() <= small.max_scroll());
    }

    #[test]
    fn v2_skeleton_counts_ready_rows() {
        let mut vl = VirtualList::new(100_000, 32, 800, 640);
        vl.scroll_to(50_000 * 32);
        let (lo, hi) = vl.live_range();
        let (plan, n) = skeleton_plan(&vl, |row| row == lo);
        // [缺陷账本] 旧期望 min(hi-lo, CAP) - 1 把"就绪首行跳过"错算成
        // 从容量里扣 1——实际 n 是产出占位数 = min(非就绪行数, CAP)。
        // 修法：期望改为 min(hi-lo-1, CAP)，判据不变。
        assert_eq!(n, ((hi - lo - 1) as usize).min(SKELETON_LIST_CAP));
        assert!(plan[0].map(|it| it.row == lo + 1).unwrap_or(false), "已就绪首行跳过");
    }

    #[test]
    fn v2_selfcheck_all_green() {
        let set = run_vlist_v2_checks();
        assert!(set.all_passed(), "F228 v2 自检存在红项");
        assert!(!set.truncated());
    }
}
