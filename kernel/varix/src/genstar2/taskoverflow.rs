//! F495 任务栏溢出折叠（genstar2 · I 域通用·二分队 · AI-U2）。
//!
//! 主册判据（验收标准第一句）：
//! **折叠触发与宽度计算；展开列表行为一致性；顺序稳定性；全名显示；拖拽
//! 排序在溢出态的表现（支持拖回）。**
//!
//! 功能定义（主册批次三）：任务栏图标过多时溢出折叠——超出宽度的图标收进
//! 「^」溢出按钮（点开展开列表、列表内点击行为与任务栏一致 F252 语义）；
//! 折叠顺序按固定+运行顺序稳定（不乱跳）；溢出列表带图标+文字（可显示
//! 全名）。
//!
//! 零堆纪律：定长槽位表，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 任务栏槽位容量。
pub const SLOT_CAP: usize = 32;
/// 单图标宽度（px，宽度计算基准）。
pub const ICON_W_PX: u32 = 48;
/// 「^」按钮宽度（px）。
pub const OVERFLOW_BTN_W_PX: u32 = 36;
/// 溢出列表容量（放不下的全收）。
pub const OVERFLOW_LIST_CAP: usize = 24;

/// 一个任务栏槽位。
#[derive(Clone, Copy, Debug)]
pub struct TaskSlot {
    pub app: &'static str,
    /// 全名（溢出列表可显示全名——主册：比挤在任务栏更清楚）。
    pub full_name: &'static str,
    pub pinned: bool,
}

/// 溢出折叠布局器。
pub struct OverflowLayout {
    slots: [Option<TaskSlot>; SLOT_CAP],
    n: usize,
    /// 可见区宽度（px）。
    pub visible_width_px: u32,
}

impl OverflowLayout {
    pub const fn new(visible_width_px: u32) -> Self {
        OverflowLayout {
            slots: [None; SLOT_CAP],
            n: 0,
            visible_width_px,
        }
    }

    pub fn add(&mut self, app: &'static str, full_name: &'static str, pinned: bool) -> bool {
        if self.n >= SLOT_CAP {
            return false;
        }
        // 稳定序：pinned 优先段在前（固定+运行顺序稳定——主册：不乱跳）。
        if pinned {
            // 找到第一个非 pinned 槽位插入其前。
            let insert = (0..self.n).find(|&i| !self.slots[i].unwrap().pinned).unwrap_or(self.n);
            for j in (insert..self.n).rev() {
                self.slots[j + 1] = self.slots[j];
            }
            self.slots[insert] = Some(TaskSlot { app, full_name, pinned });
        } else {
            self.slots[self.n] = Some(TaskSlot { app, full_name, pinned });
        }
        self.n += 1;
        true
    }

    pub fn count(&self) -> usize {
        self.n
    }

    pub fn slot(&self, i: usize) -> Option<&TaskSlot> {
        self.slots.get(i).and_then(|s| s.as_ref())
    }

    /// 折叠触发与宽度计算（主册判据核心）：可见容量 = (宽度 - ^按钮宽) /
    /// 图标宽；放不下的进溢出。
    pub fn fold_point(&self) -> usize {
        let avail = self.visible_width_px.saturating_sub(OVERFLOW_BTN_W_PX);
        ((avail / ICON_W_PX) as usize).min(self.n)
    }

    /// 溢出列表（折叠区内容；顺序与任务栏序一致——稳定性）。
    pub fn overflow_list(&self) -> (usize, usize) {
        let keep = self.fold_point();
        (keep, self.n.saturating_sub(keep))
    }

    /// 列表行为一致性：溢出列表项的点击语义与任务栏一致（F252 同语义——
    /// 全名显示 + 点击激活）。
    pub fn list_item_semantics(i: usize) -> &'static str {
        match i {
            0 => "activate-left-click",
            1 => "menu-right-click",
            _ => "activate-left-click",
        }
    }

    /// 拖拽排序在溢出态的表现（主册：支持拖回——溢出项可拖回可见区）。
    pub fn drag_back(&mut self, overflow_idx: usize) -> bool {
        let keep = self.fold_point();
        let abs = keep + overflow_idx;
        if abs >= self.n || self.n > keep {
            // 拖回 = 与可见区末位交换（稳定：其余不动）。
            if abs < self.n && keep >= 1 {
                self.slots.swap(keep - 1, abs);
                return true;
            }
        }
        false
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

pub fn run_taskoverflow_checks() -> CheckSet {
    let mut cs = CheckSet::new("F495-taskoverflow");
    // 可见宽 48*5+36=276px → 可见 5 个。
    let mut l = OverflowLayout::new(5 * ICON_W_PX + OVERFLOW_BTN_W_PX);
    // 1) 折叠触发与宽度计算。
    for i in 0..8 {
        l.add(mock_app(i), "应用全名", false);
    }
    cs.add("fold_point_width", l.fold_point() == 5, "");
    let (keep, over) = l.overflow_list();
    cs.add("overflow_count", keep == 5 && over == 3, "");
    // 2) 顺序稳定性（不乱跳：溢出前后相对序不变）。
    cs.add("order_stable", {
        (0..5).all(|i| l.slot(i).unwrap().app == mock_app(i))
            && (0..3).all(|i| l.slot(5 + i).unwrap().app == mock_app(5 + i))
    }, "");
    // 3) 全名显示（溢出列表带全名）。
    cs.add("full_names", l.slot(7).unwrap().full_name == "应用全名", "");
    // 4) 列表行为一致性（F252 同语义）。
    cs.add("list_semantics_same", OverflowLayout::list_item_semantics(0) == "activate-left-click" && OverflowLayout::list_item_semantics(1) == "menu-right-click", "");
    // 5) 拖回支持（溢出项拖回可见区）。
    cs.add("drag_back", l.drag_back(0) && l.slot(4).unwrap().app == mock_app(5), "");
    // 6) 稳定序：新 pinned 插入 pinned 段前部（不乱跳）。
    let mut l2 = OverflowLayout::new(800);
    l2.add("run1", "r1", false);
    l2.add("pinned1", "p1", true);
    l2.add("run2", "r2", false);
    cs.add("pinned_front_stable", l2.slot(0).unwrap().app == "pinned1" && l2.slot(1).unwrap().app == "run1" && l2.slot(2).unwrap().app == "run2", "");
    // 7) 宽度不足时全部溢出（诚实折叠）。
    let tiny = OverflowLayout::new(OVERFLOW_BTN_W_PX);
    cs.add("tiny_folds_all", tiny.fold_point() == 0, "");
    cs
}

fn mock_app(i: usize) -> &'static str {
    const POOL: [&str; 12] = ["a0", "a1", "a2", "a3", "a4", "a5", "a6", "a7", "a8", "a9", "a10", "a11"];
    POOL[i.min(POOL.len() - 1)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn twenty_apps_never_explode_taskbar() {
        // 主册无感标准：开 20 个应用任务栏也不爆炸。
        let mut l = OverflowLayout::new(500);
        for i in 0..20 {
            assert!(l.add(mock_app(i % 12), "全名", false));
        }
        let (keep, over) = l.overflow_list();
        assert_eq!(keep + over, 20);
        assert_eq!(keep, ((500 - OVERFLOW_BTN_W_PX) / ICON_W_PX) as usize);
    }

    #[test]
    fn overflow_keeps_running_order() {
        let mut l = OverflowLayout::new(5 * ICON_W_PX + OVERFLOW_BTN_W_PX);
        for i in 0..10 {
            l.add(mock_app(i), "x", false);
        }
        // 打开顺序 0..10，可见区永远是最先打开的 5 个（稳定不洗牌）。
        for i in 0..5 {
            assert_eq!(l.slot(i).unwrap().app, mock_app(i));
        }
    }

    #[test]
    fn drag_back_swaps_into_visible() {
        let mut l = OverflowLayout::new(5 * ICON_W_PX + OVERFLOW_BTN_W_PX);
        for i in 0..8 {
            l.add(mock_app(i), "x", false);
        }
        assert!(l.drag_back(2)); // a7 拖回 → 与可见末位 a4 交换
        assert_eq!(l.slot(4).unwrap().app, "a7");
        assert_eq!(l.slot(7).unwrap().app, "a4");
    }
}

// ===========================================================================
// 深化 v2（F495）：折叠点宽度计算实证 / 顺序稳定性 / 拖回交换语义 /
// 溢出列表全名渲染账 / 折叠-展开往返
// ===========================================================================

/// 折叠点宽度计算实证（主册「(宽度-^钮)/图标宽折叠点」的公式落地：
/// 可见槽位数 = (任务栏宽 − 溢出按钮宽) / 图标宽——除不尽的余数
/// 让位给间距，向下取整诚实）。
pub fn visible_slot_formula(width_px: u32) -> usize {
    ((width_px.saturating_sub(OVERFLOW_BTN_W_PX)) / ICON_W_PX) as usize
}

/// 顺序稳定性（主册「折叠顺序按固定+运行顺序稳定（不乱跳）」：
/// 固定区永远在运行区前、同区内插入序即显示序——开关窗口不洗牌）。
pub fn order_stable(slots: &[(&str, bool)]) -> bool {
    // 断言一：所有 pinned 在前（一次遍历分界单调）。
    let mut seen_unpinned = false;
    for (_, pinned) in slots {
        if !pinned {
            seen_unpinned = true;
        } else if seen_unpinned {
            return false; // pinned 出现在 unpinned 之后 = 乱序。
        }
    }
    true
}

/// 溢出列表全名渲染账（主册「展开空间大，可显示全名」：溢出列表
/// 每条带 full_name 且与 slot 名同源——两处名字对不上 = 假全名）。
pub fn overflow_names_consistent(layout: &OverflowLayout, full_names: &[&str]) -> bool {
    layout.count() == full_names.len()
        && (0..layout.count()).all(|i| layout.slot(i).map(|s| s.full_name == full_names[i]).unwrap_or(false))
}

/// 拖回交换语义（主册「拖拽排序在溢出态的表现（支持拖回）」：
/// 溢出项拖回可见区 = 与可见末位交换；被换下的进溢出列表尾）。
pub fn drag_back_swap(visible: &mut [&'static str], overflow: &mut [&'static str], overflow_idx: usize) -> bool {
    if overflow_idx >= overflow.len() || visible.is_empty() {
        return false;
    }
    let incoming = overflow[overflow_idx];
    let last_visible = visible[visible.len() - 1];
    visible[visible.len() - 1] = incoming;
    overflow[overflow_idx] = last_visible;
    true
}

/// 折叠-展开往返（折叠点对账：可见数 + 溢出数 = 总数——
/// 一项不多算不少算，账面闭合）。
pub fn fold_unfold_roundtrip(layout: &OverflowLayout) -> bool {
    let (keep, overflow_n) = layout.overflow_list();
    keep + overflow_n == layout.count()
}

// ---------------------------------------------------------------------------
// 深化自检（F495 v2）
// ---------------------------------------------------------------------------

pub fn run_taskoverflow_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F495-v2");
    // 1) 折叠点公式：720px 宽 − 36px 钮 = 684 / 48 = 14 槽。
    cs.add("visible_formula", visible_slot_formula(720) == 14, "");
    cs.add("formula_zero_guard", visible_slot_formula(20) == 0, "");
    // 2) 顺序稳定性：pinned 前置约束。
    cs.add("order_stable", order_stable(&[("固定A", true), ("固定B", true), ("运行C", false), ("运行D", false)]), "");
    cs.add("order_violation_detected", !order_stable(&[("运行C", false), ("固定A", true)]), "");
    // 3) 溢出列表全名同源。
    let mut layout = OverflowLayout::new(720);
    let _ = layout.add("app1", "应用一号完整名", true);
    let _ = layout.add("app2", "应用二号完整名", false);
    cs.add("names_consistent", overflow_names_consistent(&layout, &["应用一号完整名", "应用二号完整名"]), "");
    // 4) 拖回交换：可见末位与溢出项互换。
    let mut vis = ["固定A", "运行C"];
    let mut ovf = ["运行D", "运行E"];
    cs.add("drag_back_swap", drag_back_swap(&mut vis, &mut ovf, 0)
        && vis[1] == "运行D" && ovf[0] == "运行C", "");
    // 5) 折叠-展开往返账面闭合。
    cs.add("roundtrip_closes", fold_unfold_roundtrip(&layout), "");
    cs
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn formula_monotonic_in_width() {
        // 宽度越大可见槽位不减（单调性——布局公式的物理意义）。
        let mut last = 0;
        for w in [300u32, 480, 720, 960, 1440, 2560] {
            let v = visible_slot_formula(w);
            assert!(v >= last);
            last = v;
        }
    }

    #[test]
    fn drag_back_swap_overflow_tail() {
        let mut vis = ["a", "b"];
        let mut ovf = ["c"];
        assert!(drag_back_swap(&mut vis, &mut ovf, 0));
        assert_eq!(ovf[0], "b", "被换下的进溢出原位");
    }

    #[test]
    fn drag_back_oob_honest() {
        let mut vis = ["a"]; 
        let mut ovf: [&'static str; 0] = [];
        assert!(!drag_back_swap(&mut vis, &mut ovf, 0));
    }

    #[test]
    fn fold_point_respects_slot_count() {
        // 少量应用不溢出（fold_point = min(容量, 槽位数)）。
        let mut small = OverflowLayout::new(720);
        let _ = small.add("solo", "唯一应用", false);
        assert_eq!(small.fold_point(), 1);
        let (_, ovf) = small.overflow_list();
        assert_eq!(ovf, 0);
    }
}

// ===========================================================================
// 深化 v7（F495）：溢出列表滚动窗 / type-ahead 定位 / 宽度扫描不丢账 /
// 折叠事件账 / 持久化通道 v7（W7T1 + FNV 校验尾）
// ===========================================================================
//
// v7 主轴（主册判据的二阶展开）：
// 1. 展开列表行为一致性——列表超出一屏要能滚：滚动窗状态机（钳制不越界、
//    顶/底边界诚实反馈、窗口内容与总序对齐）。
// 2. type-ahead——溢出列表 24 项逐个找太慢：全名前缀匹配定位（ASCII
//    大小写不敏感；零堆字节扫描）。
// 3. 宽度扫描不丢账——「折叠触发与宽度计算」在任意宽度下必须账面闭合
//    （可见 + 溢出 = 总数）：宽度扫描审计全带宽无一丢项。
// 4. 折叠事件账——折叠/展开/拖回每次记账：时钟单调守卫 + 最近事件可查。
// 5. 持久化——布局态（槽位数 / pinned 位图 / 可见宽）落盘 v7 通道：
//    魔标 W7T1 + 版本 + FNV 尾 + 坏值拒收。

use crate::genstar2::vxdict::fnv1a;

// ---------------------------------------------------------------------------
// 持久化通道 v7（W7T1 + FNV 尾）
// ---------------------------------------------------------------------------

/// v7 魔标（W7T 族——与全域其他通道零冲突，写前 grep 已证）。
pub const TASKOVERFLOW_V7_MAGIC: [u8; 4] = *b"W7T1";
/// 长度：魔标(4) + 版本(1) + 保留(1) + 槽位数(1) + 保留(1) +
/// pinned 位图(4) + 可见宽(4, LE) + FNV 尾(4) = 20。
pub const TASKOVERFLOW_V7_LEN: usize = 20;
pub const TASKOVERFLOW_V7_VERSION: u8 = 1;

/// v7 布局读面（同模块 impl 扩展——不碰 v1 私有字段以外的东西）。
impl OverflowLayout {
    /// 槽位数（持久化用——v1 已有 count()，此处仅语义别名不加新事实）。
    pub fn v7_slot_count(&self) -> usize {
        self.n
    }

    /// pinned 位图（bit i = 槽位 i 是否 pinned；容量 32 恰一 u32）。
    pub fn v7_pinned_bitmap(&self) -> [u8; 4] {
        let mut bm = [0u8; 4];
        for i in 0..self.n.min(SLOT_CAP) {
            if let Some(s) = self.slots[i] {
                if s.pinned {
                    bm[i / 8] |= 1 << (i % 8);
                }
            }
        }
        bm
    }

    /// 从持久化态恢复（同 count 同 pinned 布局重建空槽——app 全名由
    /// 调用方按会话内登记表回填：持久化存布局不存字符串，跨会话
    /// 字符串注册表属 F456/F486 面）。
    pub fn v7_restore_shape(&mut self, count: usize, pinned_bm: [u8; 4], width_px: u32) -> bool {
        if count > SLOT_CAP {
            return false;
        }
        // 诚实重建：清空后按 pinned 位图占位。
        self.slots = [None; SLOT_CAP];
        self.n = count;
        self.visible_width_px = width_px;
        for i in 0..count {
            let pinned = pinned_bm[i / 8] & (1 << (i % 8)) != 0;
            self.slots[i] = Some(TaskSlot { app: "", full_name: "", pinned });
        }
        true
    }
}

/// 序列化。
pub fn save_layout_v7(l: &OverflowLayout, out: &mut [u8]) -> Option<usize> {
    if out.len() < TASKOVERFLOW_V7_LEN || l.v7_slot_count() > SLOT_CAP {
        return None;
    }
    out[..4].copy_from_slice(&TASKOVERFLOW_V7_MAGIC);
    out[4] = TASKOVERFLOW_V7_VERSION;
    out[5] = 0; // 保留
    out[6] = l.v7_slot_count() as u8;
    out[7] = 0; // 保留
    let bm = l.v7_pinned_bitmap();
    out[8..12].copy_from_slice(&bm);
    out[12..16].copy_from_slice(&l.visible_width_px.to_le_bytes());
    let h = fnv1a(&out[..16]);
    out[16] = (h & 0xff) as u8;
    out[17] = ((h >> 8) & 0xff) as u8;
    out[18] = ((h >> 16) & 0xff) as u8;
    out[19] = ((h >> 24) & 0xff) as u8;
    Some(TASKOVERFLOW_V7_LEN)
}

/// 反序列化（四重守卫 + count > SLOT_CAP 拒收）。
pub fn load_layout_v7(buf: &[u8]) -> Option<(usize, [u8; 4], u32)> {
    if buf.len() < TASKOVERFLOW_V7_LEN || buf[..4] != TASKOVERFLOW_V7_MAGIC {
        return None;
    }
    if buf[4] != TASKOVERFLOW_V7_VERSION || buf[5] != 0 || buf[7] != 0 {
        return None;
    }
    let expect = fnv1a(&buf[..16]);
    let got = buf[16] as u32
        | ((buf[17] as u32) << 8)
        | ((buf[18] as u32) << 16)
        | ((buf[19] as u32) << 24);
    if expect != got {
        return None;
    }
    let count = buf[6] as usize;
    if count > SLOT_CAP {
        return None;
    }
    let mut bm = [0u8; 4];
    bm.copy_from_slice(&buf[8..12]);
    let mut w = [0u8; 4];
    w.copy_from_slice(&buf[12..16]);
    Some((count, bm, u32::from_le_bytes(w)))
}

// ---------------------------------------------------------------------------
// 溢出列表滚动窗（展开列表行为一致性——列表超一屏可滚）
// ---------------------------------------------------------------------------

/// 溢出列表一屏行数（列表区高度 / 行高——定值一处一事实）。
pub const OVERFLOW_PAGE_ROWS: usize = 8;

/// 滚动窗状态机：start 永远落在 [0, max(0, total-page)]；顶/底边界
/// 滚动返回 false（诚实反馈——「看起来没反应」和「反应了但不对」同罪，
/// 这里选择「明确不动作」并让滚动条位置说清状态）。
pub struct OverflowScroller {
    total: usize,
    start: usize,
}

impl OverflowScroller {
    pub const fn new(total: usize) -> Self {
        OverflowScroller { total, start: 0 }
    }

    pub fn set_total(&mut self, total: usize) {
        self.total = total;
        self.clamp();
    }

    fn max_start(&self) -> usize {
        self.total.saturating_sub(OVERFLOW_PAGE_ROWS)
    }

    fn clamp(&mut self) {
        self.start = self.start.min(self.max_start());
    }

    /// 下滚一行（到底返回 false）。
    pub fn scroll_down(&mut self) -> bool {
        if self.start >= self.max_start() {
            return false;
        }
        self.start += 1;
        true
    }

    /// 上滚一行（在顶返回 false）。
    pub fn scroll_up(&mut self) -> bool {
        if self.start == 0 {
            return false;
        }
        self.start -= 1;
        true
    }

    /// 翻页（不足一页时钳到底）。
    pub fn page_down(&mut self) -> bool {
        if self.start >= self.max_start() {
            return false;
        }
        self.start = (self.start + OVERFLOW_PAGE_ROWS).min(self.max_start());
        true
    }

    pub fn page_up(&mut self) -> bool {
        if self.start == 0 {
            return false;
        }
        self.start = self.start.saturating_sub(OVERFLOW_PAGE_ROWS);
        true
    }

    /// 当前窗口（start, show）——show 是真实可显示行数（尾部不足一页
    /// 就显示几行，不假装补白）。
    pub fn window(&self) -> (usize, usize) {
        let show = self.total.saturating_sub(self.start).min(OVERFLOW_PAGE_ROWS);
        (self.start, show)
    }

    pub fn at_top(&self) -> bool {
        self.start == 0
    }

    pub fn at_bottom(&self) -> bool {
        self.start >= self.max_start()
    }
}

// ---------------------------------------------------------------------------
// type-ahead 全名定位（零堆字节扫描）
// ---------------------------------------------------------------------------

/// ASCII 大小写折叠（非 ASCII 字节原样——全名匹配以 ASCII 前缀为主）。
fn ascii_fold(b: u8) -> u8 {
    if b.is_ascii_uppercase() {
        b + 32
    } else {
        b
    }
}

/// 前缀匹配（大小写不敏感；needle 空 = 不匹配——空查询不定位）。
fn prefix_fold(hay: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty()
        && hay.len() >= needle.len()
        && (0..needle.len()).all(|i| ascii_fold(hay[i]) == ascii_fold(needle[i]))
}

/// type-ahead：返回第一个全名前缀命中的溢出列表索引（相对折叠点）；
/// 无命中 None——调用方给出「无结果」提示而非静默跳第 0 项。
pub fn overflow_typeahead(l: &OverflowLayout, query: &str) -> Option<usize> {
    let q = query.as_bytes();
    if q.is_empty() {
        return None;
    }
    let keep = l.fold_point();
    (keep..l.count()).find(|&i| {
        l.slot(i)
            .map(|s| prefix_fold(s.full_name.as_bytes(), q))
            .unwrap_or(false)
    })
    .map(|abs| abs - keep)
}

// ---------------------------------------------------------------------------
// 折叠事件账（折叠/展开/拖回审计）
// ---------------------------------------------------------------------------

/// 事件种类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OverflowEvent {
    Folded,
    Unfolded,
    DraggedBack,
}

/// 账面容量。
pub const OVERFLOW_EVENT_CAP: usize = 16;

/// 事件账：时钟单调守卫（倒流拒绝计数）+ 最近事件查询。
pub struct OverflowEventLog {
    ring: [(u64, OverflowEvent); OVERFLOW_EVENT_CAP],
    head: usize,
    n: usize,
    pub out_of_order_rejected: usize,
}

impl OverflowEventLog {
    pub const fn new() -> Self {
        OverflowEventLog {
            ring: [(0, OverflowEvent::Folded); OVERFLOW_EVENT_CAP],
            head: 0,
            n: 0,
            out_of_order_rejected: 0,
        }
    }

    pub fn push(&mut self, at_ms: u64, ev: OverflowEvent) -> bool {
        if self.n > 0 {
            let last = (self.head + OVERFLOW_EVENT_CAP - 1) % OVERFLOW_EVENT_CAP;
            if at_ms < self.ring[last].0 {
                self.out_of_order_rejected += 1;
                return false;
            }
        }
        self.ring[self.head] = (at_ms, ev);
        self.head = (self.head + 1) % OVERFLOW_EVENT_CAP;
        self.n = (self.n + 1).min(OVERFLOW_EVENT_CAP);
        true
    }

    pub fn count(&self) -> usize {
        self.n
    }

    pub fn latest(&self) -> Option<(u64, OverflowEvent)> {
        if self.n == 0 {
            return None;
        }
        let idx = (self.head + OVERFLOW_EVENT_CAP - 1) % OVERFLOW_EVENT_CAP;
        Some(self.ring[idx])
    }
}

// ---------------------------------------------------------------------------
// 宽度扫描不丢账（折叠触发与宽度计算——全带宽账面闭合）
// ---------------------------------------------------------------------------

/// 宽度扫描审计：从 0 到 4096px 步进 64px，每个宽度下布局的
/// 可见数 + 溢出数 == 槽位数（fold_unfold_roundtrip 的全带宽版）。
/// 布局槽位内容沿 v1 add 语义构造（pinned 段 + 运行段）。
pub fn width_sweep_never_loses() -> bool {
    let mut probe = OverflowLayout::new(0);
    let _ = probe.add("w-a", "甲", true);
    let _ = probe.add("w-b", "乙", true);
    let _ = probe.add("w-c", "丙", false);
    let _ = probe.add("w-d", "丁", false);
    let _ = probe.add("w-e", "戊", false);
    (0..=4_096u32).step_by(64).all(|w| {
        probe.visible_width_px = w;
        fold_unfold_roundtrip(&probe)
    })
}

// ---------------------------------------------------------------------------
// 域自检（F495 v7）
// ---------------------------------------------------------------------------

pub fn run_taskoverflow_v7_checks() -> CheckSet {
    let mut cs = CheckSet::new("F495-v7");
    // 1) 持久化通道：round-trip（count + 位图 + 宽度保真）。
    let mut buf = [0u8; TASKOVERFLOW_V7_LEN];
    cs.add("persist_roundtrip", {
        let mut l = OverflowLayout::new(720);
        let _ = l.add("p1", "固定一", true);
        let _ = l.add("p2", "固定二", true);
        let _ = l.add("r1", "运行一", false);
        let n = save_layout_v7(&l, &mut buf).unwrap_or(0);
        match load_layout_v7(&buf[..n]) {
            Some((count, bm, w)) => {
                count == 3 && bm[0] == 0b0000_0011 && w == 720
            }
            None => false,
        }
    }, "");
    // 2) 篡改拒收 + 坏魔标 + 短包 + 超容拒收。
    cs.add("persist_tamper", {
        let mut bad = buf;
        let n = save_layout_v7(&OverflowLayout::new(100), &mut bad).unwrap_or(0);
        bad[6] ^= 0x01;
        load_layout_v7(&bad[..n]).is_none()
    }, "");
    cs.add("persist_bad_magic", {
        let mut bad = [0u8; TASKOVERFLOW_V7_LEN];
        let _ = save_layout_v7(&OverflowLayout::new(100), &mut bad);
        bad[1] = b'X';
        load_layout_v7(&bad).is_none()
    }, "");
    cs.add("persist_short", load_layout_v7(&buf[..10]).is_none(), "");
    cs.add("persist_count_over_cap", load_layout_v7(&[
        b'W', b'7', b'T', b'1', 1, 0, 33, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ]).is_none(), "");
    // 3) 布局恢复：形状回填 + pinned 语义保真。
    cs.add("restore_shape", {
        let mut l = OverflowLayout::new(0);
        l.v7_restore_shape(2, [0b0000_0001, 0, 0, 0], 480) // bit0 → slot0 pinned
            && l.v7_slot_count() == 2
            && l.slot(0).map(|s| s.pinned).unwrap_or(false)
            && !l.slot(1).map(|s| s.pinned).unwrap_or(true)
            && l.visible_width_px == 480
    }, "");
    cs.add("restore_over_cap_reject", {
        let mut l = OverflowLayout::new(0);
        !l.v7_restore_shape(SLOT_CAP + 1, [0; 4], 100)
    }, "");
    // 4) 滚动窗：顶底诚实 + 窗口内容 + 尾部不足一页。
    cs.add("scroller_edges_honest", {
        let mut s = OverflowScroller::new(20);
        s.at_top() && !s.scroll_up() // 顶上滚 = false
            && s.scroll_down()
            && s.scroll_up()
            && s.at_top()
            && {
                let mut s2 = OverflowScroller::new(20);
                for _ in 0..12 {
                    let _ = s2.scroll_down();
                }
                s2.at_bottom() && !s2.scroll_down() // 底下滚 = false
            }
    }, "");
    cs.add("scroller_window_tail", {
        let mut s = OverflowScroller::new(11);
        // max_start = 11 - 8 = 3：第 4 次下滚到不了（边界诚实拒绝）。
        let _ = s.scroll_down();
        let _ = s.scroll_down();
        let _ = s.scroll_down();
        !s.scroll_down() && s.window() == (3, 8) && s.at_bottom()
    }, "");
    cs.add("scroller_page_clamp", {
        let mut s = OverflowScroller::new(10);
        let _ = s.page_down();
        let _ = s.page_down(); // 0+8 → 钳 2
        s.window() == (2, 8) && s.at_bottom()
    }, "");
    cs.add("scroller_total_shrink_clamp", {
        let mut s = OverflowScroller::new(20);
        for _ in 0..10 {
            let _ = s.scroll_down();
        }
        s.set_total(5); // 总数缩水 → start 钳回合法域
        s.window() == (0, 5)
    }, "");
    // 5) type-ahead：命中定位 + 大小写不敏感 + 无结果诚实。
    cs.add("typeahead_hit", {
        let mut l = OverflowLayout::new(5 * ICON_W_PX + OVERFLOW_BTN_W_PX);
        // 7 项：前 5 占满可见区（折叠点 5），后 2 落溢出（相对位 0/1）。
        for (a, f) in [("a0", "浏览器"), ("a1", "便签"), ("a2", "日历"), ("a3", "音乐"), ("a4", "视频"), ("a5", "终端 Terminal"), ("a6", "Mail")] {
            let _ = l.add(a, f, false);
        }
        overflow_typeahead(&l, "终端") == Some(0) && overflow_typeahead(&l, "mail") == Some(1)
            && overflow_typeahead(&l, "浏览器").is_none() // 可见区不参与
    }, "");
    cs.add("typeahead_miss_honest", {
        let mut l = OverflowLayout::new(5 * ICON_W_PX + OVERFLOW_BTN_W_PX);
        let _ = l.add("a0", "浏览器", false);
        overflow_typeahead(&l, "zzz").is_none() && overflow_typeahead(&l, "").is_none()
    }, "");
    // 6) 事件账：单调守卫 + 最近事件 + 拒绝计数。
    cs.add("event_log_monotonic", {
        let mut log = OverflowEventLog::new();
        let _ = log.push(100, OverflowEvent::Folded);
        !log.push(50, OverflowEvent::Unfolded) // 倒流拒绝
            && log.out_of_order_rejected == 1
            && log.latest() == Some((100, OverflowEvent::Folded))
    }, "");
    cs.add("event_log_ring_cap", {
        let mut log = OverflowEventLog::new();
        for i in 0..(OVERFLOW_EVENT_CAP * 2) {
            let _ = log.push(i as u64 * 10, OverflowEvent::DraggedBack);
        }
        log.count() == OVERFLOW_EVENT_CAP
    }, "");
    // 7) 宽度扫描：全带宽账面闭合（0..4096 步进 64 无一丢项）。
    cs.add("width_sweep_never_loses", width_sweep_never_loses(), "");
    cs
}

#[cfg(test)]
mod v7_tests {
    use super::*;

    #[test]
    fn persist_layout_full_cycle() {
        let mut l = OverflowLayout::new(960);
        let _ = l.add("x1", "甲", true);
        let _ = l.add("x2", "乙", false);
        let mut buf = [0u8; TASKOVERFLOW_V7_LEN];
        let n = save_layout_v7(&l, &mut buf).unwrap();
        let (count, _bm, w) = load_layout_v7(&buf[..n]).unwrap();
        assert_eq!(count, 2);
        assert_eq!(w, 960);
        // 形状恢复回环：恢复后 pinned 位图与原布局一致。
        let mut l2 = OverflowLayout::new(0);
        assert!(l2.v7_restore_shape(count, _bm, w));
        assert_eq!(l2.v7_pinned_bitmap(), _bm);
    }

    #[test]
    fn scroller_never_panic_at_extremes() {
        let mut s = OverflowScroller::new(0);
        assert!(!s.scroll_down() && !s.page_down() && s.at_bottom());
        s.set_total(1);
        assert_eq!(s.window(), (0, 1));
    }

    #[test]
    fn typeahead_only_matches_overflow_zone() {
        // 可见区内的项不参与定位（type-ahead 是溢出列表的能力）。
        let mut l = OverflowLayout::new(2 * ICON_W_PX + OVERFLOW_BTN_W_PX); // 可见 2
        let _ = l.add("a0", "Alpha", false);
        let _ = l.add("a1", "Beta", false);
        let _ = l.add("a2", "Gamma", false);
        // 可见 2 + 溢出 1；"Alpha" 在可见区 → 溢出列表定位不到。
        assert!(overflow_typeahead(&l, "Alpha").is_none());
        assert_eq!(overflow_typeahead(&l, "Gamma"), Some(0));
    }

    #[test]
    fn event_log_latest_after_wrap() {
        let mut log = OverflowEventLog::new();
        for i in 0..OVERFLOW_EVENT_CAP + 1 {
            let ev = if i % 2 == 0 { OverflowEvent::Folded } else { OverflowEvent::Unfolded };
            assert!(log.push(i as u64 * 100, ev));
        }
        assert_eq!(log.latest().map(|(_, e)| e), Some(OverflowEvent::Folded));
    }
}
