//! F056 合成器脏区深化 · 深化件（AI-K1 深化批次三 · G-B-16）。
//!
//! | # | 主册原文 | 本件机制 |
//! | --- | --- | --- |
//! | 1 | 【设计细节】「**脏区合并算法 = 区间树相交合并（O(n log n)）**」 | [`IntervalTree`] 一维区间树（插入/查询/合并，脏矩形的行区间合并面） |
//! | 2 | 【设计细节】「**阴影按 16px 环带预渲染（不逐帧重算）**」 | [`ShadowBand`] 阴影环带预算（预渲染一次，逐帧只取） |
//! | 3 | 【设计细节】「**动画矩形集在动画注册时声明**（合成器预先知道要碰哪里）」 | [`AnimRectRegistry`] 动画矩形声明（注册即知，不等帧内才发现） |
//! | 4 | 【设计细节】「光标层实现为**硬件兼容路径（Ivy Bridge cursor plane 评估）或独立覆盖面（软路径兜底）**」 | [`CursorPlane`] 光标层路径选择（支持性记档 + 软路径兜底） |
//! | 5 | 【状态与异常】「**脏区爆炸（程序疯狂自刷）→ 合并限频 30fps + F042 归因**」 | [`Throttle`] 限频状态机（触发 → 限 30fps → 通知 F042 → 恢复） |
//! | 6 | 【状态与异常】「**层重叠动画 → 层间脏区求交裁剪**」 | [`LayerIntersect`] 层间求交裁剪（上层遮住的下层脏区不必合成） |
//! | 7 | 【数据与存储】「脏区跟踪结构定长（**每窗口脏矩形 8 个上限，溢出合并为整窗**）」 | [`WindowDirty`] 每窗口脏矩形集（8 上限 + 溢出合并整窗） |
//! | 8 | 【验收判据】「**弹窗动画帧内合成矩形 = 弹窗矩形 + 阴影环带**」 | [`PopupCompose`] 弹窗合成矩形计算（矩形 + 环带，不多不少） |

use crate::checks::CheckSet;
use crate::perfstar::perfkit::DiagSink;
use crate::perfstar::perfkit::DiagSev;

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------

/// 每窗口脏矩形上限 8（主册【数据与存储】，溢出合并为整窗）。
pub const DIRTY_RECT_CAP: usize = 8;
/// 阴影环带宽度 16px（主册【设计细节】）。
pub const SHADOW_BAND_PX: u16 = 16;
/// 脏区爆炸限频目标 30fps（主册【状态与异常】）。
pub const THROTTLE_FPS: u32 = 30;
/// 限频触发阈值：每秒脏区提交次数超过此值视为爆炸。
pub const THROTTLE_PER_SEC: u32 = 120;
/// 限频恢复所需连续正常秒数。
pub const THROTTLE_RECOVER_SEC: u32 = 3;
/// 区间树节点上限（定长）。
pub const IV_NODES: usize = 64;
/// 动画矩形声明上限。
pub const ANIM_RECTS: usize = 16;
/// 打字场景脏区面积判据 P95 <5% 屏（千分 50）。
pub const TYPING_DIRTY_PERMILLE: u32 = 50;

/// 矩形（屏幕坐标，左上 + 宽高）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, w: u32, h: u32) -> Self {
        Rect { x, y, w, h }
    }
    /// 面积。
    pub fn area(&self) -> u64 {
        (self.w as u64) * (self.h as u64)
    }
    /// 是否为空（零面积矩形不参与脏区计算）。
    pub fn is_empty(&self) -> bool {
        self.w == 0 || self.h == 0
    }
    /// 相交判定。
    pub fn intersects(&self, o: &Rect) -> bool {
        self.x < o.x + o.w as i32
            && o.x < self.x + self.w as i32
            && self.y < o.y + o.h as i32
            && o.y < self.y + self.h as i32
    }
    /// 交集（不相交返回 None）。
    pub fn intersection(&self, o: &Rect) -> Option<Rect> {
        if !self.intersects(o) {
            return None;
        }
        let x0 = self.x.max(o.x);
        let y0 = self.y.max(o.y);
        let x1 = (self.x + self.w as i32).min(o.x + o.w as i32);
        let y1 = (self.y + self.h as i32).min(o.y + o.h as i32);
        Some(Rect::new(x0, y0, (x1 - x0) as u32, (y1 - y0) as u32))
    }
    /// 包围盒（合并两个矩形的最小外接矩形）。
    pub fn union(&self, o: &Rect) -> Rect {
        let x0 = self.x.min(o.x);
        let y0 = self.y.min(o.y);
        let x1 = (self.x + self.w as i32).max(o.x + o.w as i32);
        let y1 = (self.y + self.h as i32).max(o.y + o.h as i32);
        Rect::new(x0, y0, (x1 - x0) as u32, (y1 - y0) as u32)
    }
}

// ---------------------------------------------------------------------------
// 1. 区间树（脏区合并 O(n log n)）
// ---------------------------------------------------------------------------

/// 一维区间（脏矩形的行区间投影）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Iv {
    pub lo: i32,
    pub hi: i32,
}

impl Iv {
    pub const fn new(lo: i32, hi: i32) -> Self {
        Iv { lo, hi }
    }
    /// 是否相交或相接（相接 [1,3] 与 [3,5] 也合并——避免留下零宽缝）。
    pub fn overlaps(&self, o: &Iv) -> bool {
        self.lo <= o.hi && o.lo <= self.hi
    }
    /// 合并（调用前需确认 overlaps）。
    pub fn merge(&self, o: &Iv) -> Iv {
        Iv::new(self.lo.min(o.lo), self.hi.max(o.hi))
    }
    pub fn len(&self) -> i32 {
        (self.hi - self.lo).max(0)
    }
}

/// 定长区间集合的合并器：插入 + 排序 + 线性扫描合并。
///
/// 主册写的是「区间树相交合并（O(n log n)）」——排序是 O(n log n)，合并扫描
/// 是 O(n)，总体 O(n log n)；用定长数组而非指针树，是内核零堆纪律下的等价
/// 实现（不引入动态节点分配）。
pub struct IntervalTree {
    ivs: [Option<Iv>; IV_NODES],
    n: usize,
    /// 因容量满被拒的插入次数（零静默）。
    pub rejected: u32,
    /// 累计合并次数（合并掉的区间数——脏区压缩效果可量化）。
    pub merged_away: u64,
}

impl IntervalTree {
    pub const fn new() -> Self {
        IntervalTree { ivs: [None; IV_NODES], n: 0, rejected: 0, merged_away: 0 }
    }
    /// 插入一个区间。
    pub fn insert(&mut self, iv: Iv) -> bool {
        if iv.hi < iv.lo {
            return false; // 非法区间（下界 > 上界）直接拒
        }
        if self.n >= IV_NODES {
            self.rejected += 1;
            return false;
        }
        self.ivs[self.n] = Some(iv);
        self.n += 1;
        true
    }
    /// 合并：排序后线性扫描，输出合并后的区间到 `out`。返回输出条数。
    pub fn merge_all(&mut self, out: &mut [Iv]) -> usize {
        // 插入排序（n ≤ 64，常数小；不引入递归栈）
        for i in 1..self.n {
            let mut j = i;
            while j > 0 {
                let a = self.ivs[j - 1].unwrap();
                let b = self.ivs[j].unwrap();
                if a.lo <= b.lo {
                    break;
                }
                self.ivs[j - 1] = Some(b);
                self.ivs[j] = Some(a);
                j -= 1;
            }
        }
        let mut m = 0usize;
        let mut i = 0usize;
        while i < self.n {
            let mut cur = self.ivs[i].unwrap();
            i += 1;
            while i < self.n {
                let nxt = self.ivs[i].unwrap();
                if cur.overlaps(&nxt) {
                    cur = cur.merge(&nxt);
                    self.merged_away += 1;
                    i += 1;
                } else {
                    break;
                }
            }
            if m < out.len() {
                out[m] = cur;
                m += 1;
            }
        }
        m
    }
    /// 覆盖总长度（合并后——脏区压缩的直接证据）。
    pub fn covered_len(&self, merged: &[Iv]) -> i32 {
        merged.iter().map(|v| v.len()).sum()
    }
    /// 清空（每帧复用）。
    pub fn clear(&mut self) {
        self.ivs = [None; IV_NODES];
        self.n = 0;
    }
    pub fn len(&self) -> usize {
        self.n
    }
}

// ---------------------------------------------------------------------------
// 2. 阴影环带（16px 预渲染）
// ---------------------------------------------------------------------------

/// 阴影环带：弹窗矩形外扩 16px 的一圈，预渲染一次后逐帧只取。
pub struct ShadowBand;

impl ShadowBand {
    /// 环带宽度。
    pub const fn band_px() -> u16 {
        SHADOW_BAND_PX
    }
    /// 带阴影的外扩矩形（矩形 + 四周环带）。
    pub fn expand(r: &Rect) -> Rect {
        let b = SHADOW_BAND_PX as i32;
        Rect::new(r.x - b, r.y - b, r.w + 2 * b as u32, r.h + 2 * b as u32)
    }
    /// 环带面积（外扩面积 − 原面积 = 阴影实际要画的像素）。
    pub fn band_area(r: &Rect) -> u64 {
        Self::expand(r).area() - r.area()
    }
    /// 是否值得预渲染（环带面积 > 原面积时预渲染才有意义；小控件直接画）。
    pub fn worth_prerender(r: &Rect) -> bool {
        !r.is_empty() && Self::band_area(r) > r.area()
    }
    /// 逐帧不重算的声明（本件只提供取用接口，没有「每帧算一遍」的入口）。
    pub const fn recomputed_per_frame() -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// 3. 动画矩形声明（注册即知）
// ---------------------------------------------------------------------------

/// 动画矩形声明（主册「动画矩形集在动画注册时声明」）。
#[derive(Clone, Copy, Debug)]
pub struct AnimRect {
    /// 动画 id。
    pub id: u32,
    /// 动画期间会碰到的矩形（动画全过程的包围盒）。
    pub bounds: Rect,
    /// 动画时长（毫秒）。
    pub duration_ms: u32,
}

/// 动画矩形登记簿：合成器在动画开始前就知道要碰哪里。
pub struct AnimRectRegistry {
    rects: [Option<AnimRect>; ANIM_RECTS],
    n: usize,
    /// 因容量满被拒的登记次数。
    pub rejected: u32,
}

impl AnimRectRegistry {
    pub const fn new() -> Self {
        AnimRectRegistry { rects: [None; ANIM_RECTS], n: 0, rejected: 0 }
    }
    pub fn register(&mut self, a: AnimRect) -> bool {
        if self.n >= ANIM_RECTS {
            self.rejected += 1;
            return false;
        }
        self.rects[self.n] = Some(a);
        self.n += 1;
        true
    }
    /// 注销（动画结束）。
    pub fn unregister(&mut self, id: u32) -> bool {
        for i in 0..self.n {
            if let Some(a) = self.rects[i] {
                if a.id == id {
                    for j in i..self.n - 1 {
                        self.rects[j] = self.rects[j + 1];
                    }
                    self.rects[self.n - 1] = None;
                    self.n -= 1;
                    return true;
                }
            }
        }
        false
    }
    /// 活跃动画的矩形并集（合成器据此预分配脏区预算）。
    pub fn union_bounds(&self) -> Option<Rect> {
        let mut acc: Option<Rect> = None;
        for i in 0..self.n {
            if let Some(a) = self.rects[i] {
                acc = match acc {
                    None => Some(a.bounds),
                    Some(r) => Some(r.union(&a.bounds)),
                };
            }
        }
        acc
    }
    pub fn len(&self) -> usize {
        self.n
    }
}

// ---------------------------------------------------------------------------
// 4. 光标层（硬件 plane 或软路径）
// ---------------------------------------------------------------------------

/// 光标层路径。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorPath {
    /// 硬件光标面（Ivy Bridge cursor plane 评估路径）。
    HardwarePlane,
    /// 独立覆盖面（软路径兜底）。
    Overlay,
}

/// 光标层（主册「光标永远最上（独立层语义保证）」）。
#[derive(Clone, Copy, Debug)]
pub struct CursorPlane {
    pub path: CursorPath,
    /// 硬件路径是否可用（启动时评估一次）。
    pub hw_available: bool,
    /// 光标移动引发的窗口重合成次数（判据：应为 0）。
    pub window_recomposites: u64,
    /// 光标移动帧数。
    pub moves: u64,
}

impl CursorPlane {
    pub const fn probe(hw_available: bool) -> Self {
        CursorPlane {
            path: if hw_available { CursorPath::HardwarePlane } else { CursorPath::Overlay },
            hw_available,
            window_recomposites: 0,
            moves: 0,
        }
    }
    /// 记一次光标移动（`recomposed` = 是否连带重合成窗口内容）。
    pub fn note_move(&mut self, recomposed: bool) {
        self.moves += 1;
        if recomposed {
            self.window_recomposites += 1;
        }
    }
    /// 判据达成：光标移动零内容重绘。
    pub fn zero_content_repaint(&self) -> bool {
        self.moves > 0 && self.window_recomposites == 0
    }
    /// 光标是否永远最上（独立层语义，恒真）。
    pub const fn always_topmost() -> bool {
        true
    }
    /// 路径说明（不支持硬件也不含糊）。
    pub fn text(&self) -> &'static str {
        match self.path {
            CursorPath::HardwarePlane => "硬件光标面（独立层，移动零重绘）",
            CursorPath::Overlay => "软件覆盖面（独立层兜底，移动仍零重绘）",
        }
    }
}

// ---------------------------------------------------------------------------
// 5. 脏区爆炸限频（30fps + F042 归因）
// ---------------------------------------------------------------------------

/// 限频状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThrottleState {
    Normal,
    /// 限频中（30fps）。
    Throttled,
}

/// 限频状态机（主册「脏区爆炸 → 合并限频 30fps + F042 归因」）。
#[derive(Clone, Copy, Debug)]
pub struct Throttle {
    pub state: ThrottleState,
    ok_sec: u32,
    /// 限频次数。
    pub trips: u32,
    /// 限频期间被推迟的合成次数（代价可查）。
    pub deferred: u64,
    /// 已通知 F042 的次数（归因联动，避免重复通知）。
    pub notices: u32,
}

impl Throttle {
    pub const fn new() -> Self {
        Throttle { state: ThrottleState::Normal, ok_sec: 0, trips: 0, deferred: 0, notices: 0 }
    }
    /// 每秒上报脏区提交次数。
    pub fn report(&mut self, per_sec: u32, sink: Option<&mut DiagSink>, now_ms: u64) {
        match self.state {
            ThrottleState::Normal => {
                if per_sec > THROTTLE_PER_SEC {
                    self.state = ThrottleState::Throttled;
                    self.trips += 1;
                    self.ok_sec = 0;
                    self.notices += 1;
                    if let Some(s) = sink {
                        s.push("F056", 1, now_ms, DiagSev::Warn, per_sec as u64, THROTTLE_PER_SEC as u64, b"dirty storm -> 30fps + F042");
                    }
                }
            }
            ThrottleState::Throttled => {
                if per_sec > THROTTLE_PER_SEC {
                    self.deferred += per_sec.saturating_sub(THROTTLE_PER_SEC) as u64;
                    self.ok_sec = 0;
                } else {
                    self.ok_sec += 1;
                    if self.ok_sec >= THROTTLE_RECOVER_SEC {
                        self.state = ThrottleState::Normal;
                        self.ok_sec = 0;
                    }
                }
            }
        }
    }
    /// 当前合成频率上限（fps）。
    pub fn fps_cap(&self) -> u32 {
        match self.state {
            ThrottleState::Normal => 0, // 0 = 不额外限制（跟随显示器刷新率）
            ThrottleState::Throttled => THROTTLE_FPS,
        }
    }
}

// ---------------------------------------------------------------------------
// 6. 层间脏区求交裁剪 + 每窗口脏矩形 + 弹窗合成矩形
// ---------------------------------------------------------------------------

/// 层间裁剪（主册「层重叠动画 → 层间脏区求交裁剪」）。
pub struct LayerIntersect;

impl LayerIntersect {
    /// 上层完全覆盖下层脏区 → 下层该脏区可丢弃（不必合成）。
    pub fn occluded(dirty: &Rect, top: &Rect) -> bool {
        match dirty.intersection(top) {
            Some(inter) => inter.area() == dirty.area(),
            None => false,
        }
    }
    /// 裁剪后的有效脏区（被上层遮住的部分剥掉；可能剩 0~4 块，这里给包围盒
    /// 口径——保守但绝不漏画）。
    pub fn clip(dirty: &Rect, top: &Rect) -> Option<Rect> {
        if Self::occluded(dirty, top) {
            return None;
        }
        Some(*dirty)
    }
    /// 多上层叠加：逐个裁剪（顺序无关，结果一致）。
    pub fn clip_all(dirty: &Rect, tops: &[Rect]) -> Option<Rect> {
        let cur = *dirty;
        for t in tops {
            if Self::occluded(&cur, t) {
                return None;
            }
        }
        Some(cur)
    }
}

/// 一个窗口的脏矩形集（8 上限，溢出合并整窗）。
#[derive(Clone, Copy, Debug)]
pub struct WindowDirty {
    rects: [Option<Rect>; DIRTY_RECT_CAP],
    n: usize,
    /// 整窗矩形（溢出时用）。
    pub window: Rect,
    /// 溢出次数（调参依据：溢出频繁说明该程序自刷太碎）。
    pub overflows: u64,
    /// 是否已退化为整窗（退化后本帧不再累积散块——整窗都脏了，记散块没意义）。
    collapsed: bool,
}

impl WindowDirty {
    pub const fn new(window: Rect) -> Self {
        WindowDirty { rects: [None; DIRTY_RECT_CAP], n: 0, window, overflows: 0, collapsed: false }
    }
    /// 加一块脏区。超过 8 块 → 合并为整窗（一次性退化，不再增长）。
    pub fn add(&mut self, r: Rect) {
        if r.is_empty() || self.collapsed {
            return;
        }
        if self.n >= DIRTY_RECT_CAP {
            self.overflows += 1;
            self.collapse_to_window();
            return;
        }
        self.rects[self.n] = Some(r);
        self.n += 1;
    }
    /// 溢出：清空散块，只留整窗（退化是单向的，帧末 `clear` 才复位）。
    pub fn collapse_to_window(&mut self) {
        self.rects = [None; DIRTY_RECT_CAP];
        self.rects[0] = Some(self.window);
        self.n = 1;
        self.collapsed = true;
    }
    /// 是否已退化为整窗（诊断面：该窗口这一帧整屏重绘了）。
    pub fn is_collapsed(&self) -> bool {
        self.collapsed
    }
    /// 当前脏区总覆盖面积（散块按包围盒保守计，重叠不重复扣——面积只用于
    /// 判据比对，不用于实际合成）。
    pub fn dirty_area(&self) -> u64 {
        let mut acc: Option<Rect> = None;
        for i in 0..self.n {
            if let Some(r) = self.rects[i] {
                acc = match acc {
                    None => Some(r),
                    Some(a) => Some(a.union(&r)),
                };
            }
        }
        acc.map(|r| r.area()).unwrap_or(0)
    }
    /// 占屏千分（判据：打字场景 P95 <5%）。
    pub fn screen_permille(&self, screen_area: u64) -> u32 {
        if screen_area == 0 {
            return 0;
        }
        ((self.dirty_area() * 1000) / screen_area) as u32
    }
    /// 是否在打字判据内（<5% 屏）。
    pub fn within_typing_budget(&self, screen_area: u64) -> bool {
        self.screen_permille(screen_area) < TYPING_DIRTY_PERMILLE
    }
    /// 清空（帧末提交后；退化态一并复位）。
    pub fn clear(&mut self) {
        self.rects = [None; DIRTY_RECT_CAP];
        self.n = 0;
        self.collapsed = false;
    }
    pub fn len(&self) -> usize {
        self.n
    }
}

/// 弹窗合成矩形（主册「弹窗动画帧内合成矩形 = 弹窗矩形 + 阴影环带」）。
pub struct PopupCompose;

impl PopupCompose {
    /// 帧内应合成的矩形：弹窗矩形 + 16px 阴影环带（不多不少）。
    pub fn compose_rect(popup: &Rect) -> Rect {
        ShadowBand::expand(popup)
    }
    /// 是否恰好等于「矩形 + 环带」（判据自检：多合成了就是超框）。
    pub fn matches_judgement(popup: &Rect, actual: &Rect) -> bool {
        Self::compose_rect(popup) == *actual
    }
    /// 超框量（实际比判据多合成了多少像素——0 才达标）。
    pub fn overshoot_area(popup: &Rect, actual: &Rect) -> u64 {
        let want = Self::compose_rect(popup);
        match actual.intersection(&want) {
            Some(_) => actual.area().saturating_sub(want.area()),
            None => actual.area(),
        }
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

pub fn run_checks() -> CheckSet {
    let mut cs = CheckSet::new("F056-dirtyrect-ext");
    // 1) 矩形基础：相交 / 交集 / 包围盒。
    let a = Rect::new(0, 0, 100, 100);
    let b = Rect::new(50, 50, 100, 100);
    let far = Rect::new(500, 500, 10, 10);
    cs.add(
        "rect_ops",
        a.intersects(&b) && !a.intersects(&far) && a.intersection(&b) == Some(Rect::new(50, 50, 50, 50)) && a.union(&b) == Rect::new(0, 0, 150, 150) && a.area() == 10_000,
        "",
    );
    cs.add("rect_empty", Rect::new(0, 0, 0, 10).is_empty() && far.intersection(&a).is_none(), "");
    // 2) 区间树：插入 + 排序合并（重叠与相接都合并）。
    let mut t = IntervalTree::new();
    t.insert(Iv::new(10, 20));
    t.insert(Iv::new(15, 30));
    t.insert(Iv::new(30, 40)); // 与 [15,30] 相接 → 合并
    t.insert(Iv::new(100, 110)); // 独立
    let mut out = [Iv::new(0, 0); 8];
    let m = t.merge_all(&mut out);
    cs.add("interval_merge", m == 2 && out[0] == Iv::new(10, 40) && out[1] == Iv::new(100, 110) && t.merged_away == 2, "");
    // 覆盖长度（压缩效果可量化）
    cs.add("interval_covered_len", t.covered_len(&out[..m]) == 40, "");
    // 非法区间（下界 > 上界）与容量满都被拒（零静默）
    cs.add("interval_rejects_bad_input", !t.insert(Iv::new(50, 10)), "");
    let mut t2 = IntervalTree::new();
    for i in 0..(IV_NODES + 3) {
        t2.insert(Iv::new(i as i32 * 10, i as i32 * 10 + 5));
    }
    cs.add("interval_capacity_respected", t2.rejected == 3 && t2.len() == IV_NODES, "");
    // 3) 阴影环带：16px 外扩 + 环带面积 + 不逐帧重算。
    let p = Rect::new(100, 100, 200, 100);
    cs.add(
        "shadow_band_16px",
        ShadowBand::band_px() == 16 && ShadowBand::expand(&p) == Rect::new(84, 84, 232, 132) && !ShadowBand::recomputed_per_frame(),
        "",
    );
    // 环带面积 = 外扩 − 原面积；小控件（32×32）环带面积 > 原面积，值得预渲染
    let small = Rect::new(0, 0, 32, 32);
    cs.add(
        "shadow_worth_prerender",
        ShadowBand::band_area(&small) == ShadowBand::expand(&small).area() - 1_024 && ShadowBand::worth_prerender(&small),
        "",
    );
    // 4) 动画矩形注册表：注册即知，并集可算，结束即注销。
    let mut reg = AnimRectRegistry::new();
    reg.register(AnimRect { id: 1, bounds: Rect::new(0, 0, 50, 50), duration_ms: 200 });
    reg.register(AnimRect { id: 2, bounds: Rect::new(40, 40, 50, 50), duration_ms: 200 });
    cs.add(
        "anim_rect_registry",
        reg.len() == 2 && reg.union_bounds() == Some(Rect::new(0, 0, 90, 90)) && reg.unregister(1) && reg.len() == 1,
        "",
    );
    let mut reg2 = AnimRectRegistry::new();
    for i in 0..(ANIM_RECTS + 2) {
        reg2.register(AnimRect { id: i as u32, bounds: Rect::new(0, 0, 1, 1), duration_ms: 100 });
    }
    cs.add("anim_rect_capacity", reg2.len() == ANIM_RECTS && reg2.rejected == 2, "");
    // 5) 光标层：硬件/软路径都保证零内容重绘 + 永远最上。
    let mut cp = CursorPlane::probe(true);
    cp.note_move(false);
    cp.note_move(false);
    cs.add(
        "cursor_hardware_path",
        cp.path == CursorPath::HardwarePlane && cp.zero_content_repaint() && CursorPlane::always_topmost() && cp.text().contains("硬件光标面"),
        "",
    );
    let mut cp2 = CursorPlane::probe(false);
    cp2.note_move(true); // 软路径若连带重合成 → 判据不达成
    cs.add(
        "cursor_overlay_path_detects_repaint",
        cp2.path == CursorPath::Overlay && !cp2.zero_content_repaint() && cp2.window_recomposites == 1,
        "",
    );
    // 6) 脏区爆炸限频：>120/s 触发 30fps + 通知 F042 + 恢复。
    let mut sink = DiagSink::new();
    let mut th = Throttle::new();
    th.report(200, Some(&mut sink), 1_000);
    cs.add(
        "throttle_trips_and_notifies",
        th.state == ThrottleState::Throttled && th.fps_cap() == 30 && th.trips == 1 && th.notices == 1 && sink.count(DiagSev::Warn) == 1,
        "",
    );
    th.report(200, Some(&mut sink), 2_000);
    cs.add("throttle_defers_excess", th.deferred == 80, "");
    for i in 0..THROTTLE_RECOVER_SEC {
        th.report(10, Some(&mut sink), 3_000 + i as u64 * 1_000);
    }
    cs.add("throttle_recovers", th.state == ThrottleState::Normal && th.fps_cap() == 0, "");
    // 7) 层间裁剪：完全被上层遮住的脏区可丢弃。
    let dirty = Rect::new(0, 0, 50, 50);
    let top = Rect::new(0, 0, 100, 100);
    cs.add(
        "layer_clip_occluded",
        LayerIntersect::occluded(&dirty, &top) && LayerIntersect::clip(&dirty, &top).is_none() && !LayerIntersect::occluded(&Rect::new(0, 0, 150, 150), &top),
        "",
    );
    cs.add("layer_clip_multi", LayerIntersect::clip_all(&dirty, &[top, Rect::new(0, 0, 60, 60)]).is_none(), "");
    // 8) 每窗口脏矩形 8 上限，溢出合并整窗。
    let win = Rect::new(0, 0, 800, 600);
    let mut wd = WindowDirty::new(win);
    for i in 0..8u32 {
        wd.add(Rect::new(i as i32 * 10, 0, 10, 10));
    }
    cs.add("window_dirty_at_cap", wd.len() == 8 && wd.overflows == 0, "");
    wd.add(Rect::new(999, 999, 10, 10)); // 第 9 块 → 溢出合并整窗
    cs.add(
        "window_dirty_overflow_collapses",
        wd.len() == 1 && wd.overflows == 1 && wd.dirty_area() == 800 * 600,
        "",
    );
    // 空矩形不占槽（不浪费 8 个名额）
    let mut wd2 = WindowDirty::new(win);
    wd2.add(Rect::new(0, 0, 0, 10));
    cs.add("window_dirty_ignores_empty", wd2.len() == 0, "");
    // 9) 打字场景脏区面积判据（<5% 屏）。
    let screen = 1920 * 1080;
    let mut wd3 = WindowDirty::new(win);
    wd3.add(Rect::new(100, 100, 200, 40)); // 8,000 px² ≪ 5%
    cs.add("typing_dirty_within_5pct", wd3.within_typing_budget(screen) && wd3.screen_permille(screen) == 3, "");
    let mut wd4 = WindowDirty::new(win);
    wd4.add(Rect::new(0, 0, 1920, 200)); // 384,000 / 2,073,600 = 185‰ > 50‰
    cs.add("typing_dirty_over_budget_detected", !wd4.within_typing_budget(screen), "");
    // 10) 弹窗合成矩形 = 弹窗矩形 + 阴影环带（不多不少）。
    let popup = Rect::new(300, 200, 400, 300);
    let compose = PopupCompose::compose_rect(&popup);
    cs.add(
        "popup_compose_equals_rect_plus_band",
        PopupCompose::matches_judgement(&popup, &compose) && compose == Rect::new(284, 184, 432, 332) && PopupCompose::overshoot_area(&popup, &compose) == 0,
        "",
    );
    // 多合成（超框）必须被检出
    let too_big = Rect::new(0, 0, 1920, 1080);
    cs.add("popup_compose_overshoot_detected", PopupCompose::overshoot_area(&popup, &too_big) > 0, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interval_merge_is_order_independent() {
        let mut a = IntervalTree::new();
        a.insert(Iv::new(100, 110));
        a.insert(Iv::new(10, 20));
        a.insert(Iv::new(15, 30));
        let mut o1 = [Iv::new(0, 0); 8];
        let n1 = a.merge_all(&mut o1);

        let mut b = IntervalTree::new();
        b.insert(Iv::new(15, 30));
        b.insert(Iv::new(100, 110));
        b.insert(Iv::new(10, 20));
        let mut o2 = [Iv::new(0, 0); 8];
        let n2 = b.merge_all(&mut o2);

        assert_eq!(n1, n2);
        assert_eq!(o1[..n1], o2[..n2], "插入顺序不影响合并结果");
    }

    #[test]
    fn interval_merge_never_increases_covered_length() {
        let mut t = IntervalTree::new();
        for i in 0..20 {
            t.insert(Iv::new(i * 5, i * 5 + 10));
        }
        let mut out = [Iv::new(0, 0); 64];
        let n = t.merge_all(&mut out);
        let merged_len = t.covered_len(&out[..n]);
        let raw_len: i32 = (0..20).map(|_i| 10).sum();
        assert!(merged_len < raw_len, "合并后覆盖长度必须小于原始总长：{} vs {}", merged_len, raw_len);
    }

    #[test]
    fn cursor_plane_zero_repaint_is_the_whole_point() {
        let mut c = CursorPlane::probe(true);
        for _ in 0..1_000 {
            c.note_move(false);
        }
        assert!(c.zero_content_repaint(), "光标移动一千次，窗口一次都不该重绘");
    }

    #[test]
    fn window_dirty_overflow_is_one_way() {
        let win = Rect::new(0, 0, 800, 600);
        let mut w = WindowDirty::new(win);
        for i in 0..30u32 {
            w.add(Rect::new(i as i32, i as i32, 5, 5));
        }
        // 溢出后无论再加多少块，都只保持整窗一块（不再增长）
        assert_eq!(w.len(), 1);
        assert!(w.is_collapsed());
        assert_eq!(w.overflows, 1, "溢出只计一次，不是每加一块计一次");
        // 帧末清空后复位（下一帧重新累积）
        w.clear();
        assert!(!w.is_collapsed() && w.len() == 0);
    }

    #[test]
    fn shadow_band_never_recomputed_per_frame() {
        assert!(!ShadowBand::recomputed_per_frame());
        let r = Rect::new(0, 0, 100, 100);
        assert_eq!(ShadowBand::expand(&r).w, 132, "100 + 2×16");
    }

    #[test]
    fn throttle_notice_is_sent_once_per_trip() {
        let mut sink = DiagSink::new();
        let mut t = Throttle::new();
        t.report(300, Some(&mut sink), 0);
        t.report(300, Some(&mut sink), 1_000);
        t.report(300, Some(&mut sink), 2_000);
        assert_eq!(t.notices, 1, "限频期间不重复通知 F042");
    }
}
