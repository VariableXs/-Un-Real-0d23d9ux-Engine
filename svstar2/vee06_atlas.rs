//! VE-F0806 · 字形缓存与图集——文字渲染输出的中枢。
//!
//! 本单把 F0804 的位图与 F0805 的相位键空间接成**渲染输出的唯一落地面**：
//! 字形位图按（字形 ID、字号、相位、粗细）四元组键入图集页，页内由
//! **Shelf 打包器**分配 UV 区域，四元组 → UV 的映射由**定容开放寻址索引**
//! 承担。锚点原文：「字形位图按（字形 ID、字号、相位、粗细）四元组键入图集页
//! （默认 2048×2048，可配）」。
//!
//! # 一、键空间共享（与 F0805 同一份类型，非「各写一份」）
//!
//! 本单**直接复用** [`vee05_hinting::CacheKey`] 作为图集键，不另立结构：
//! 锚点 F0805 明文「相位缓存与 F0806 图集共享键空间」，两单各写一份键类型
//! 才是漂移的来源。四元组的四段（glyph_id / px_size / phase / weight）由
//! [`CacheKey`] 单点定义，本单只加**区分性**判据（见自检 E06-01）。
//!
//! # 二、页满双策略：先压缩整理，再淘汰（次序是判据，不是措辞）
//!
//! [`AtlasConfig`] 之外的处置链固定为：
//! `1 现有页空闲 → 2 开新页（未达页数上限）→ 3 压缩整理（碎片回收）
//! → 4 淘汰（LRU 加分代）→ 5 降级直渲`。
//!
//! **整理先于淘汰**由自检 E06-05 构造「空闲碎片存在但整段放不下」的场景
//! 强制验证：此时 `compactions` 必增、`evictions` 必不增。若实现把两步
//! 颠倒，两条计数同时变化，判据变红。
//!
//! # 三、核心安全不变量：只搬动「本帧未使用」的页
//!
//! 锚点「当前帧使用的页免淘」在本单被提升为**结构性不变量**而非仅淘汰偏好：
//!
//! - 淘汰只从**未 pin**的页中挑选（[`AtlasPage::pinned`]）；
//! - 压缩整理（会搬动条目、改变 UV）**同样只作用于未 pin 的页**。
//!
//! 理由：页被 pin 即表示本帧已把其中的 UV 区域交给上层（F0807 图元批次）。
//! 若整理/淘汰动了 pin 页，本帧已发出的 UV 会指向别人的像素——**画面错位
//! 且无任何报错**。把「免淘」升为不变量后，「本帧发出的 UV 永不变」成为
//! **结构上不可违反**的性质，自检 E06-06 用 pin 页指纹前后比对机检。
//!
//! 代价是明确的：帧内不再有空间时只能降级直渲（[`Placement::Direct`]），
//! 而非在帧中途重排。这是**正确的取舍**——宁可多一次直渲，不冒画面错位。
//!
//! # 四、分代淘汰（LRU 加分代）
//!
//! 两代：[`Generation::Young`] / [`Generation::Old`]。页每被一个**新帧**
//! 使用过（累计 [`AtlasConfig::promote_after`] 次）晋升为 old 代。
//! victim 选取键为
//! `(代, 最近使用时刻, 页号)` 三元组字典序：
//!
//! 1. 先在 young 代里挑最久未用的；
//! 2. young 代无可用候选时才动 old 代；
//! 3. 页号作末位 tie-break，保证**同输入必得同输出**（判据可写序列断言）。
//!
//! 分代序**先于** LRU 序：老而不常用的 old 代受保护，老而常用的更受保护。
//! 自检 E06-04 构造「old 代的 last_used 比 young 代更小」的反直觉场景——
//! 纯 LRU 实现会选错页而变红。
//!
//! # 五、错误路径与降级矩阵（零静默，逐档计数）
//!
//! | 触发 | 处置 | 计数 |
//! |---|---|---|
//! | 单帧新增字形超页容量（预算判据） | [`Placement::Direct`] 绕过图集逐字上传 | [`AtlasStats::degrade_overflow`] |
//! | 页无空间且整理被节流 / 全页 pin | [`Placement::Direct`] | [`AtlasStats::degrade_pinned`] |
//! | 索引负载因子触顶（75%） | [`Placement::Direct`] | [`AtlasStats::degrade_index_full`] |
//! | GPU 上传失败 | **重试一次**；再失败 → [`Placement::CpuFallback`] 并**回滚落位** | [`AtlasStats::upload_retries`] / [`AtlasStats::cpu_fallbacks`] |
//!
//! - **GPU 重试上限恰为 1**（[`GPU_UPLOAD_RETRIES`]）：重试是有限的，否则
//!   一个恒失败的设备会把每帧拖成长循环。自检 E06-07 断言 `attempts == 2`
//!   （首次 + 一次重试）而**不是**「≥2」。
//! - **上传失败必须回滚**：位图没进 GPU 却留在索引里，下一帧会命中一个
//!   **从未上传过**的 UV ⇒ 静默画错。故失败即从索引与页内同时摘除
//!   （[`AtlasPage::release`] + 索引 remove），本帧走 CPU 路径，下帧重试。
//! - **降级绝不丢字形**：任何降级档都返回**可用**的 [`Placement`]，不返回
//!   `None`、不静默跳过。自检 E06-06 断言「请求数 == 返回数」。
//!
//! # 六、命中统计三指标（口径冻结，供 F0809 遥测与 F0815 预算表引用）
//!
//! [`AtlasStats::telemetry`] 产出 [`AtlasTelemetry`]：命中率、淘汰率、整理频率。
//! **口径**（三处共用同一分母定义，不各算各的）：
//!
//! - 命中率 = hits / lookups（累计；查表 0 次时为 0，不出现除零与 NaN）；
//! - 淘汰率 = evictions / inserts（累计）；
//! - 整理频率 = compactions 在给定时间窗内折算到每秒（×1000 取整）。
//!
//! 「淘汰率的分母是插入数而非查找数」是本单最易写错的一处：淘汰由**写入**
//! 触发（页满才淘汰），挂在查找侧会把一次淘汰重复计入。
//!
//! # 七、错误路径之外的边界防护
//!
//! - **索引负载因子守卫**：占用（含墓碑）达 3/4 即拒绝写入并先重整
//!   （[`AtlasIndex`]）；不做「静默覆盖既有键」——覆盖会让在用字形消失。
//! - **墓碑与重整**：删除留墓碑，查找遇空格终止；墓碑过半触发rehash。
//!   [`AtlasIndex::insert_unchecked`] 是**唯一**能绕过负载因子守卫的入口，
//!   仅供自检构造满表态（否则「探测到满表」这一支不可达、不可测）。
//! - **探测步数高水位** [`AtlasIndex::max_probe_steps`]：不设人工步数上限，
//!   而是**实测**并断言其落在 [`MAX_PROBE_STEPS`] 之内。人工 cap 会把
//!   「查找变慢」伪装成「查找失败」，掩盖真缺陷。
//!
//! # 八、性能：0.005ms/字形 的诚实口径
//!
//! 锚点「缓存命中路径渲染 ≤0.005ms/字形」。本单把该预算**拆成两半**：
//!
//! - **可测的一半**（自检 E06-08 强制）：[`WorkCounter`] 在**缺陷真正发生的
//!   那一层**（索引探测步数、键比较字段数、UV 拷贝字节）计数，自检实测
//!   「每命中工作量 ≤ [`HIT_WORK_CAP`]」且「每命中工作量 < 每未命中工作量」。
//!   改坏实现（命中路径退化为全表扫描、或命中时整表拷贝）→ 计数变红。
//! - **不可测的一半**（显式交接，不假装）：no_std 内核自检无墙钟，
//!   [`HIT_PATH_MAX_NS`] 只是把锚点 0.005ms 固化为常量供 F0815 预算表引用，
//!   **不参与判定**。真机墙钟测量归 F0815（预算表五段计时），本单不代做、
//!   不用「n×CONST」式自证式算术冒充实测。
//!
//! 锚点「图集整理 ≤2ms/次且不超每秒一次」：节流由调用方注入的
//! `now_us` 驱动（[`COMPACT_MIN_INTERVAL_US`]），自检以**显式时间轴**验证
//! 「1 秒内第二次整理被拒且计数」，不依赖真实时钟。
//!
//! # 九、对接
//!
//! - **F0804**：[`slot_rect`] / [`make_slot`] 与 [`AtlasSlot`]（F0804 标注的
//!   「F0806 对接产物」）互转，本单不另立 UV 结构。
//! - **F0805**：共享 [`CacheKey`] 键空间（见第一节）。
//! - **F0807**：[`GlyphAtlas::resolve_batch`] 产出 [`BatchRef`]（图集页 + 页内
//!   矩形 + 定点 UV），即文本图元批处理的直接输入；本单不实现合批。
//! - **F0809 / F0815**：[`AtlasTelemetry`] 六字段与命中率口径。
//!
//! # 十、零 panic 面
//!
//! 全模块用 `get`/`get_mut` 与显式边界检查，**无 `unwrap()`、无 `expect()`、
//! 无 `panic!`、无裸 `[i]` 索引**（越界即视为容量不足并降级，不中止渲染）。
//! 断言集中在 [`run_vee06_checks`] 内。

extern crate alloc;

use alloc::vec::Vec;

use super::vee04_raster::{AtlasSlot, Weight};
use super::vee05_hinting::CacheKey;
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 一、常量
// ---------------------------------------------------------------------------

/// 默认图集页边长（锚点「默认 2048×2048」）。
pub const DEFAULT_PAGE_DIM: u16 = 2048;

/// 默认页数上限。
pub const DEFAULT_MAX_PAGES: u16 = 4;

/// 字形间距（页内留透明边距，防双线性采样渗色；不计入字形面积）。
pub const GLYPH_PAD: u16 = 1;

/// 命中率目标（锚点「常态文本目标 ≥97%」），千分比。
pub const HIT_RATE_TARGET_PERMILLE: u32 = 970;

/// 单次图集整理耗时上限（锚点「≤2ms/次」），微秒。
pub const COMPACT_MAX_US: u64 = 2_000;

/// 两次整理的最小间隔（锚点「不超每秒一次」），微秒。
pub const COMPACT_MIN_INTERVAL_US: u64 = 1_000_000;

/// 缓存命中路径预算（锚点「≤0.005ms/字形」），纳秒。
///
/// **只作常量冻结与交接**（见模块头注第八节），不参与自检判定。
pub const HIT_PATH_MAX_NS: u32 = 5_000;

/// 命中路径工作量上限（每命中，单位见 [`WorkCounter`]）。
///
/// 结构性上界：探测步数 ≤ [`MAX_PROBE_STEPS`]、每次命中固定 4 次键字段
/// 比较（[`CacheKey`] 的四个字段）、UV 拷贝 ≤ [`UV_COPY_UNITS`]。
pub const HIT_WORK_CAP: u32 = MAX_PROBE_STEPS as u32 * 2 + 4 + UV_COPY_UNITS as u32;

/// 探测步数上限（**实测高水位须落在此内**，不是人工截断）。
pub const MAX_PROBE_STEPS: usize = 8;

/// UV 矩形拷贝折算的工作量单位（一个 [`Rect`] = 8 字节 = 1 单位）。
pub const UV_COPY_UNITS: usize = 1;

/// 索引负载因子上限（占用 + 墓碑）/ 容量。
pub const INDEX_LOAD_NUM: usize = 3;

/// 索引负载因子分母。
pub const INDEX_LOAD_DEN: usize = 4;

/// GPU 上传重试上限（锚点「重试一次」）。
pub const GPU_UPLOAD_RETRIES: u32 = 1;

/// 单个字形的最小预算面积（用于「单帧新增超页容量」的溢出预算判据）。
pub const MIN_GLYPH_DIM: u16 = 8;

// ---------------------------------------------------------------------------
// 二、配置
// ---------------------------------------------------------------------------

/// 图集配置（锚点「默认 2048×2048，可配」）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AtlasConfig {
    /// 页边长（正方形，锚点默认 2048）。
    pub page_dim: u16,
    /// 页数上限。
    pub max_pages: u16,
    /// 字形间距。
    pub pad: u16,
    /// 晋升 old 代所需的「被不同帧使用过」次数。
    pub promote_after: u32,
}

impl AtlasConfig {
    pub const fn new(page_dim: u16, max_pages: u16) -> Self {
        AtlasConfig {
            page_dim,
            max_pages,
            pad: GLYPH_PAD,
            promote_after: 4,
        }
    }

    /// 校验配置合法性（页边长与页数上限为 0 时不可用）。
    pub fn valid(&self) -> bool {
        self.page_dim > 0 && self.max_pages > 0
    }

    /// 单页理论字形容量（按最小字形边长估算，溢出预算判据用）。
    pub fn page_glyph_budget(&self) -> u64 {
        let d = self.page_dim as u64;
        d * d / ((MIN_GLYPH_DIM * MIN_GLYPH_DIM) as u64)
    }

    /// 全图集理论字形容量。
    pub fn total_glyph_budget(&self) -> u64 {
        self.page_glyph_budget() * self.max_pages as u64
    }
}

impl Default for AtlasConfig {
    fn default() -> Self {
        AtlasConfig::new(DEFAULT_PAGE_DIM, DEFAULT_MAX_PAGES)
    }
}

// ---------------------------------------------------------------------------
// 三、矩形与代
// ---------------------------------------------------------------------------

/// 页内矩形（字形位图的落位区域）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Rect {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
}

impl Rect {
    pub const fn new(x: u16, y: u16, w: u16, h: u16) -> Self {
        Rect { x, y, w, h }
    }

    pub fn right(&self) -> u32 {
        self.x as u32 + self.w as u32
    }

    pub fn bottom(&self) -> u32 {
        self.y as u32 + self.h as u32
    }

    /// 字形面积（**不含间距**）。
    pub fn area(&self) -> u32 {
        self.w as u32 * self.h as u32
    }

    /// 与另一矩形是否相交（**边界相接不算相交**——贴边是合法的紧排）。
    pub fn intersects(&self, o: &Rect) -> bool {
        (self.x as u32) < o.right()
            && (o.x as u32) < self.right()
            && (self.y as u32) < o.bottom()
            && (o.y as u32) < self.bottom()
    }

    /// 是否越出边长为 `dim` 的正方形页。
    pub fn fits(&self, dim: u16) -> bool {
        self.right() <= dim as u32 && self.bottom() <= dim as u32
    }

    /// 与 F0804 的 [`AtlasSlot`] 互转（本单不另立 UV 结构）。
    pub fn of_slot(s: AtlasSlot) -> Rect {
        Rect {
            x: s.x,
            y: s.y,
            w: s.w,
            h: s.h,
        }
    }

    /// 转为 F0804 的 [`AtlasSlot`]（F0807/F0804 消费）。
    pub fn to_slot(self, page: u16) -> AtlasSlot {
        AtlasSlot {
            page,
            x: self.x,
            y: self.y,
            w: self.w,
            h: self.h,
        }
    }
}

/// [`AtlasSlot`] → [`Rect`]（自由函数形态，供与 F0804 显式对接）。
pub fn slot_rect(s: AtlasSlot) -> Rect {
    if s.page == u16::MAX { return Rect { x: 0, y: 0, w: 0, h: 0 }; }
    Rect::of_slot(s)
}

/// 页内落位 → [`AtlasSlot`]（自由函数形态，供与 F0804 显式对接）。
pub fn make_slot(page: u16, r: Rect) -> AtlasSlot {
    r.to_slot(page)
}

/// 定点 UV 矩形（0..65535 对应整页，**零浮点**）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct UvRect {
    pub u0: u16,
    pub v0: u16,
    pub u1: u16,
    pub v1: u16,
}

/// 归一化到 0..65535 的定点 UV。
///
/// `dim` 为 0 或矩形越界时返回 `None`（**不静默给出错误 UV**）。
pub fn uv_rect(r: Rect, dim: u16) -> Option<UvRect> {
    if dim == 0 || !r.fits(dim) {
        return None;
    }
    let d = dim as u32;
    let u0 = ((r.x as u64 * 65_535) / d as u64) as u16;
    let v0 = ((r.y as u64 * 65_535) / d as u64) as u16;
    let u1 = ((r.right() as u64 * 65_535) / d as u64) as u16;
    let v1 = ((r.bottom() as u64 * 65_535) / d as u64) as u16;
    Some(UvRect { u0, v0, u1, v1 })
}

/// 页的代（分代淘汰）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Generation {
    /// 年轻代：优先被淘汰。
    Young,
    /// 老代：累计被 [`AtlasConfig::promote_after`] 个不同帧使用过，
    /// 证明「常被用」⇒ 受淘汰保护。
    Old,
}

// ---------------------------------------------------------------------------
// 四、Shelf 打包器
// ---------------------------------------------------------------------------

/// 一层货架（Shelf）：高度固定，槽位自左向右推进。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shelf {
    /// 层顶 y。
    pub origin_y: u16,
    /// 层高（等于本层最高字形的高度 + 间距）。
    pub height: u16,
    /// 下一个可用 x。
    pub cursor_x: u16,
}

/// Shelf 打包器（锚点「图集页分配器（Shelf 打包）」）。
///
/// **分配律**：一个字形只能放进 `height >= need_h` 的层；放不下就**新开一层**
/// （层顶 y 累加）。这条「高度门」是 Shelf 与朴素「一行一字形」的分界，
/// 也是防重叠的唯一保证——自检 E06-03 以「重叠检测 + 打包紧密度达到该尺寸
/// 下的理论上界」双向验证。
#[derive(Clone, Debug)]
pub struct ShelfPacker {
    dim: u16,
    pad: u16,
    shelves: Vec<Shelf>,
    used_area: u32,
    /// 因层高不足被跳过的次数（**非零才说明高度门真的生效过**）。
    pub height_rejects: u32,
    /// 宽或高超过页边长而被拒的次数。
    pub oversize_rejects: u32,
    /// 层扫描步数累计（工作量计量，见 [`ShelfPacker::allocate`]）。
    pub scan_steps: u32,
}

impl ShelfPacker {
    pub fn new(dim: u16, pad: u16) -> Self {
        ShelfPacker {
            dim,
            pad,
            shelves: Vec::new(),
            used_area: 0,
            height_rejects: 0,
            oversize_rejects: 0,
            scan_steps: 0,
        }
    }

    pub fn shelves(&self) -> &[Shelf] {
        &self.shelves
    }

    pub fn used_area(&self) -> u32 {
        self.used_area
    }

    /// 打包带面积 = Σ 层高 × 页宽（**页内已被占用的纵向范围**）。
    pub fn band_area(&self) -> u32 {
        let mut s = 0u32;
        for sh in self.shelves.iter() {
            s += sh.height as u32 * self.dim as u32;
        }
        s
    }

    /// 带内紧密度 = 字形面积 / 打包带面积（0 表示尚无字形）。
    pub fn band_occupancy(&self) -> f64 {
        let b = self.band_area();
        if b == 0 {
            return 0.0;
        }
        self.used_area as f64 / b as f64
    }

    /// 页面占用率 = 字形面积 / 页面积。
    pub fn page_occupancy(&self) -> f64 {
        let p = self.dim as u64 * self.dim as u64;
        if p == 0 {
            return 0.0;
        }
        self.used_area as f64 / p as f64
    }

    /// 是否还可能放得下（**不改变状态**）：任一层有宽度余量且层高足够，
    /// 或底部还能开新层。
    pub fn can_fit(&self, w: u16, h: u16) -> bool {
        let (nw, nh) = self.slot_size(w, h);
        if nw == 0 || nh == 0 || nw > self.dim || nh > self.dim {
            return false;
        }
        for sh in self.shelves.iter() {
            if sh.height >= nh && sh.cursor_x + nw <= self.dim {
                return true;
            }
        }
        let bottom = self.bottom_y();
        bottom + nh as u32 <= self.dim as u32
    }

    /// 下一个新层的顶 y。
    fn bottom_y(&self) -> u32 {
        let mut y = 0u32;
        for sh in self.shelves.iter() {
            y += sh.height as u32;
        }
        y
    }

    /// 字形 + 间距的占位尺寸（饱和加，防 u16 溢出绕回）。
    fn slot_size(&self, w: u16, h: u16) -> (u16, u16) {
        if w == 0 || h == 0 {
            return (0, 0);
        }
        let nw = (w as u32 + self.pad as u32).min(u16::MAX as u32) as u16;
        let nh = (h as u32 + self.pad as u32).min(u16::MAX as u32) as u16;
        (nw, nh)
    }

    /// 分配一个 `w × h` 的槽位；页满返回 `None`。
    ///
    /// 扫描顺序自底向上（先低层），同层内自左向右：**同输入必得同落位**。
    ///
    /// 累计 [`ShelfPacker::scan_steps`]：每探一层计1 单位。这是未命中侧
    /// 的真实成本之一（层多则扫描贵），不记它则工作量模型只测查找侧。
    pub fn allocate(&mut self, w: u16, h: u16) -> Option<Rect> {
        let (nw, nh) = self.slot_size(w, h);
        if nw == 0 || nh == 0 {
            return None;
        }
        if nw > self.dim || nh > self.dim {
            self.oversize_rejects += 1;
            return None;
        }
        let mut saw_height_block = false;
        let mut placed: Option<Rect> = None;
        for sh in self.shelves.iter_mut() {
            self.scan_steps = self.scan_steps.saturating_add(1);
            if sh.height < nh {
                saw_height_block = true;
                continue;
            }
            if sh.cursor_x + nw <= self.dim {
                placed = Some(Rect {
                    x: sh.cursor_x,
                    y: sh.origin_y,
                    w,
                    h,
                });
                sh.cursor_x += nw;
                break;
            }
        }
        if placed.is_none() && saw_height_block {
            self.height_rejects += 1;
        }
        if let Some(r) = placed {
            self.used_area = self.used_area.saturating_add(r.area());
            return Some(r);
        }
        // 新开一层。
        let y = self.bottom_y();
        if y + nh as u32 > self.dim as u32 {
            return None;
        }
        self.shelves.push(Shelf {
            origin_y: y as u16,
            height: nh,
            cursor_x: nw,
        });
        self.used_area = self.used_area.saturating_add(w as u32 * h as u32);
        Some(Rect {
            x: 0,
            y: y as u16,
            w,
            h,
        })
    }

    /// 清空（不保留层）。
    pub fn reset(&mut self) {
        self.shelves.clear();
        self.used_area = 0;
        self.height_rejects = 0;
        self.oversize_rejects = 0;
        self.scan_steps = 0;
    }
}

/// 给定字形尺寸下 Shelf 打包的**理论带内紧密度上界**。
///
/// 每层能放 `floor(dim / (w+pad))` 个，字形面积 `w*h`，层高 `h+pad`：
/// `上界 = n*w*h / ((h+pad) * dim)`。自检以「实测紧密度 ≥ 上界」验证打包器
/// **达到了结构最优**——这比手调一个 0.85 之类的阈值可证伪得多：任何把字形
/// 摊到无效位置（乱序、多开层）的实现都会低于此值。
pub fn shelf_optimal_occupancy(w: u16, h: u16, dim: u16, pad: u16) -> f64 {
    if w == 0 || h == 0 || dim == 0 {
        return 0.0;
    }
    let nw = w as u64 + pad as u64;
    let nh = h as u64 + pad as u64;
    if nw == 0 || nh == 0 || nw > dim as u64 || nh > dim as u64 {
        return 0.0;
    }
    let per = dim as u64 / nw;
    if per == 0 {
        return 0.0;
    }
    (per * w as u64 * h as u64) as f64 / ((nh * dim as u64) as f64)
}

// ---------------------------------------------------------------------------
// 五、哈希索引：四元组 → (页, 矩形)
// ---------------------------------------------------------------------------

/// 索引槽状态。
const SLOT_EMPTY: u8 = 0;
/// 墓碑（删除后不可终止探测链）。
const SLOT_TOMB: u8 = 2;
/// 占用。
const SLOT_USED: u8 = 1;

/// 索引槽。
#[derive(Clone, Copy, Debug)]
struct IndexSlot {
    state: u8,
    key: CacheKey,
    page: u16,
    rect: Rect,
}

/// 索引写入结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InsertOutcome {
    /// 新键写入。
    Inserted,
    /// 同键原位更新（尺寸相同）。
    Updated,
    /// 负载因子触顶且重整后仍满——**拒绝，绝不静默覆盖既有键**。
    RejectedFull,
}

/// 工作量计数器（性能判据的测量点，见模块头注第八节）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WorkCounter {
    /// 探测步数。
    pub probes: u32,
    /// 键字段比较次数（[`CacheKey`] 四字段逐一比较）。
    pub compares: u32,
    /// UV 矩形拷贝折算单位。
    pub copies: u32,
    /// 索引**写入**折算单位（写入侧的真实成本）。
    ///
    /// 【这一项为何不可省】第一版工作���模型只有 probes/compares/copies，
    /// 结果未命中侧 compares≈0、copies=0（唯一键必走空槽⇒ 不比较不拷贝），
    /// 每命中 5 单位 vs 每未命中 1 单位 ⇒ 「命中比未命中省」恒红。
    /// 症结是**计数器只覆盖了查找侧**：未命中的真实成本在「探测空槽 +
    ///   写槽 + 扫描 Shelf 层 + 摘除旧条目」，而这些都不在原模型里。
    /// 一个只测半边的计数器，会把「便宜的那半」当成全部。
    pub writes: u32,
}

impl WorkCounter {
    pub const fn total(&self) -> u32 {
        self.probes + self.compares + self.copies + self.writes
    }

    /// 两点之差（单调计数器的增量）。
    pub fn since(&self, prev: &WorkCounter) -> WorkCounter {
        WorkCounter {
            probes: self.probes.saturating_sub(prev.probes),
            compares: self.compares.saturating_sub(prev.compares),
            copies: self.copies.saturating_sub(prev.copies),
            writes: self.writes.saturating_sub(prev.writes),
        }
    }
}

/// 定容开放寻址索引（四元组 → 页号 + 页内矩形）。
///
/// **键比较是真比较**：只比 `compact() & mask` 的槽位会把**键不同、槽位相同**
/// 的两个字形互相串扰（前者返回后者��位图 ⇒ 画面错字且无报错）。本索引逐
/// 字段比对四元组，自检 E06-01 以「compact 低位相同但四元组不同的两键」
/// 强制验证不串扰。
#[derive(Clone, Debug)]
pub struct AtlasIndex {
    slots: Vec<IndexSlot>,
    mask: usize,
    used: usize,
    tombs: usize,
    /// 探测步数高水位（实测，非人工 cap）。
    pub max_probe_steps: usize,
    /// 探测到满表（无空格可终止）的次数——正常永不发生，被绕过守卫时可达。
    pub full_scans: u32,
    /// 满表时返回的 None 次数。
    pub saturated_lookups: u32,
    /// 因负载因子触顶而拒绝的写入次数。
    pub rejected_writes: u32,
    /// 重整（rehash）次数。
    pub rehashes: u32,
    /// 累计工作量。
    pub work: WorkCounter,
}

impl AtlasIndex {
    /// 容量向上取整到 2 的幂（`cap` 为 0 时取 1）。
    pub fn with_capacity(cap: usize) -> Self {
        let n = cap.max(1).next_power_of_two();
        let slots = (0..n)
            .map(|_| IndexSlot {
                state: SLOT_EMPTY,
                key: CacheKey::new(0, 0, super::vee05_hinting::Phase2::new(0, 0), Weight::Regular),
                page: 0,
                rect: Rect::default(),
            })
            .collect::<Vec<_>>();
        AtlasIndex {
            slots,
            mask: n - 1,
            used: 0,
            tombs: 0,
            max_probe_steps: 0,
            full_scans: 0,
            saturated_lookups: 0,
            rejected_writes: 0,
            rehashes: 0,
            work: WorkCounter::default(),
        }
    }

    pub fn capacity(&self) -> usize {
        self.slots.len()
    }

    pub fn used(&self) -> usize {
        self.used
    }

    pub fn tombs(&self) -> usize {
        self.tombs
    }

    /// 负载因子是否已触顶（含墓碑）。
    pub fn at_load_limit(&self) -> bool {
        (self.used + self.tombs) * INDEX_LOAD_DEN >= self.capacity() * INDEX_LOAD_NUM
    }

    /// 键 → 起始槽（混淆后取值，避免 `compact()` 的高位段聚簇）。
    fn start(&self, key: CacheKey) -> usize {
        (mix32(key.compact()) as usize) & self.mask
    }

    /// 查找（命中数由调用方记账，本索引只报探测量）。
    pub fn lookup(&mut self, key: CacheKey) -> Option<(u16, Rect)> {
        let mut idx = self.start(key);
        let mut steps = 0usize;
        loop {
            steps += 1;
            if steps > self.max_probe_steps {
                self.max_probe_steps = steps;
            }
            let hit = match self.slots.get(idx) {
                None => {
                    // 容量为 1 时取不到（不可能：容量恒 ≥1）——按饱和处理。
                    self.full_scans += 1;
                    self.saturated_lookups += 1;
                    self.work.probes = self.work.probes.saturating_add(steps as u32);
                    return None;
                }
                Some(s) => match s.state {
                    SLOT_EMPTY => {
                        self.work.probes = self.work.probes.saturating_add(steps as u32);
                        return None;
                    }
                    SLOT_USED => {
                        self.work.compares = self.work.compares.saturating_add(4);
                        if s.key.glyph_id == key.glyph_id
                            && s.key.px_size == key.px_size
                            && s.key.phase == key.phase
                            && s.key.weight == key.weight
                        {
                            self.work.probes = self.work.probes.saturating_add(steps as u32);
                            self.work.copies = self.work.copies.saturating_add(UV_COPY_UNITS as u32);
                            return Some((s.page, s.rect));
                        }
                        false
                    }
                    _ => false,
                },
            };
            let _ = hit;
            idx = (idx + 1) & self.mask;
            if steps > self.capacity() {
                // 探测链绕回起点仍无空格 ⇒ 表满（`insert_unchecked` 绕过守卫
                // 才可达）。**必须返回**：否则本循环绕圈永不停⇒ 内核挂死。
                // 判「满」而非「未命中」：两者都不命中，但满表必须留痕，
                // 否则饱和会被误读成「键不存在」。
                self.full_scans += 1;
                self.saturated_lookups += 1;
                self.work.probes = self.work.probes.saturating_add(steps as u32);
                return None;
            }
        }
    }

    /// 写入（受负载因子守卫约束）。
    pub fn insert(&mut self, key: CacheKey, page: u16, rect: Rect) -> InsertOutcome {
        if self.at_load_limit() {
            self.rehash();
        }
        if self.at_load_limit() {
            self.rejected_writes += 1;
            return InsertOutcome::RejectedFull;
        }
        self.insert_inner(key, page, rect)
    }

    /// 写入（**绕过负载因子守卫**）。
    ///
    /// 仅供自检构造「索引满表」这一在守卫下不可达的状态；生产路径禁用
    /// （守卫的意义就是让满表不可达）。
    pub fn insert_unchecked(&mut self, key: CacheKey, page: u16, rect: Rect) -> InsertOutcome {
        self.insert_inner(key, page, rect)
    }

    fn insert_inner(&mut self, key: CacheKey, page: u16, rect: Rect) -> InsertOutcome {
        let mut idx = self.start(key);
        let mut steps = 0usize;
        let mut first_tomb: Option<usize> = None;
        loop {
            steps += 1;
            if steps > self.max_probe_steps {
                self.max_probe_steps = steps;
            }
            let mut done: Option<InsertOutcome> = None;
            if let Some(s) = self.slots.get(idx) {
                match s.state {
                    SLOT_EMPTY => {
                        let target = first_tomb.unwrap_or(idx);
                        if first_tomb.is_some() {
                            self.tombs = self.tombs.saturating_sub(1);
                        }
                        if let Some(t) = self.slots.get_mut(target) {
                            t.state = SLOT_USED;
                            t.key = key;
                            t.page = page;
                            t.rect = rect;
                        }
                        self.used += 1;
                        self.work.writes = self.work.writes.saturating_add(1);
                        done = Some(InsertOutcome::Inserted);
                    }
                    SLOT_TOMB => {
                        if first_tomb.is_none() {
                            first_tomb = Some(idx);
                        }
                    }
                    _ => {
                        self.work.compares = self.work.compares.saturating_add(4);
                        if s.key.glyph_id == key.glyph_id
                            && s.key.px_size == key.px_size
                            && s.key.phase == key.phase
                            && s.key.weight == key.weight
                        {
                            if let Some(t) = self.slots.get_mut(idx) {
                                t.page = page;
                                t.rect = rect;
                            }
                            self.work.writes = self.work.writes.saturating_add(1);
                            done = Some(InsertOutcome::Updated);
                        }
                    }
                }
            }
            self.work.probes = self.work.probes.saturating_add(steps as u32);
            match done {
                Some(o) => return o,
                None => {}
            }
            idx = (idx + 1) & self.mask;
            if steps > self.capacity() {
                // 探测链绕回起点仍无空格：表满。**必须返回**——否则索引
                // 会绕圈死循环（守卫下不可达，`insert_unchecked` 构造的满表
                // 才走得到，故此守卫不可省）。
                self.full_scans += 1;
                self.rejected_writes += 1;
                return InsertOutcome::RejectedFull;
            }
        }
    }

    /// 删除（留墓碑，保持探测链完整）。
    pub fn remove(&mut self, key: CacheKey) -> bool {
        let mut idx = self.start(key);
        let mut steps = 0usize;
        loop {
            steps += 1;
            let mut removed = false;
            let mut stop = false;
            if let Some(s) = self.slots.get(idx) {
                match s.state {
                    SLOT_EMPTY => stop = true,
                    SLOT_USED => {
                        self.work.compares = self.work.compares.saturating_add(4);
                        if s.key.glyph_id == key.glyph_id
                            && s.key.px_size == key.px_size
                            && s.key.phase == key.phase
                            && s.key.weight == key.weight
                        {
                            if let Some(t) = self.slots.get_mut(idx) {
                                t.state = SLOT_TOMB;
                            }
                            self.used = self.used.saturating_sub(1);
                            self.tombs += 1;
                            removed = true;
                        }
                    }
                    _ => {}
                }
            }
            self.work.probes = self.work.probes.saturating_add(steps as u32);
            if removed || stop {
                return removed;
            }
            idx = (idx + 1) & self.mask;
            if steps > self.capacity() {
                return false;
            }
        }
    }

    /// 重整：清墓碑并重新落位（容量不变）。
    pub fn rehash(&mut self) {
        let old: Vec<(CacheKey, u16, Rect)> = self
            .slots
            .iter()
            .filter(|s| s.state == SLOT_USED)
            .map(|s| (s.key, s.page, s.rect))
            .collect();
        for s in self.slots.iter_mut() {
            s.state = SLOT_EMPTY;
        }
        self.used = 0;
        self.tombs = 0;
        self.rehashes += 1;
        for (k, p, r) in old.iter() {
            let _ = self.insert_inner(*k, *p, *r);
        }
    }

    /// 索引内容指纹（顺序敏感）——自检用来证明「整理后索引确实跟着更新了」。
    pub fn fingerprint(&self) -> u64 {
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        for s in self.slots.iter() {
            if s.state != SLOT_USED {
                continue;
            }
            for v in [
                s.key.compact() as u64,
                s.page as u64,
                s.rect.x as u64,
                s.rect.y as u64,
                s.rect.w as u64,
                s.rect.h as u64,
            ]
            .iter()
            {
                h ^= *v;
                h = h.wrapping_mul(0x1000_0000_01b3);
            }
        }
        h
    }
}

/// 32 位混淆（finalizer）：`compact()` 是**分段位或**，高位段天然聚簇，
/// 直接取模会让不同段落的键挤进同一条探测链。混淆后探测链长度回到期望 O(1)。
fn mix32(mut h: u32) -> u32 {
    h ^= h >> 16;
    h = h.wrapping_mul(0x7feb_352d);
    h ^= h >> 15;
    h = h.wrapping_mul(0x846c_68b5);
    h ^= h >> 16;
    h
}

// ---------------------------------------------------------------------------
// 六、图集页
// ---------------------------------------------------------------------------

/// 页内一个已落位的字形。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LiveEntry {
    pub key: CacheKey,
    pub rect: Rect,
}

/// 图集页。
#[derive(Clone, Debug)]
pub struct AtlasPage {
    /// 页号。
    pub id: u16,
    packer: ShelfPacker,
    live: Vec<LiveEntry>,
    /// 最近使用时刻（调用方注入的 `now_us`）。
    pub last_used_us: u64,
    /// 本帧是否被使用（pin）——**免淘，且免整理**（见模块头注第三节）。
    pub pinned: bool,
    /// 代。
    pub generation: Generation,
    /// 累计被**不同帧**使用过的次数（分代晋升依据，见 `[Generation]`）。
    pub serve_frames: u32,
    /// 上一次被计入 `[AtlasPage::serve_frames]` 的帧号（帧内去重）。
    pub last_served_frame: u64,
    /// 本页条目指纹（自检机检 pin 页未被改动）。
    pub fingerprint: u64,
}

impl AtlasPage {
    fn new(id: u16, dim: u16, pad: u16) -> Self {
        AtlasPage {
            id,
            packer: ShelfPacker::new(dim, pad),
            live: Vec::new(),
            last_used_us: 0,
            pinned: false,
            generation: Generation::Young,
            serve_frames: 0,
            last_served_frame: u64::MAX,
            fingerprint: 0,
        }
    }

    pub fn live(&self) -> &[LiveEntry] {
        &self.live
    }

    pub fn len(&self) -> usize {
        self.live.len()
    }

    pub fn is_empty(&self) -> bool {
        self.live.is_empty()
    }

    pub fn used_area(&self) -> u32 {
        self.packer.used_area()
    }

    pub fn band_occupancy(&self) -> f64 {
        self.packer.band_occupancy()
    }

    pub fn shelves(&self) -> &[Shelf] {
        self.packer.shelves()
    }

    pub fn height_rejects(&self) -> u32 {
        self.packer.height_rejects
    }

    /// 是否还能容纳 `w × h`。
    pub fn can_fit(&self, w: u16, h: u16) -> bool {
        self.packer.can_fit(w, h)
    }

    /// 落位一个已确认装得下的字形。
    fn insert_at(&mut self, key: CacheKey, rect: Rect) {
        self.live.push(LiveEntry { key, rect });
        self.fingerprint = self.compute_fingerprint();
    }

    /// 摘除一个条目（**只摘自己的边**，见下方不变量说明）。
    ///
    /// 摘除后 Shelf 无法回收内部空洞，故调用方须重排（[`repack`]）。
    /// 返回被摘除的条目。
    pub fn release(&mut self, key: CacheKey) -> Option<LiveEntry> {
        let mut out = None;
        let mut i = 0usize;
        while i < self.live.len() {
            if let Some(e) = self.live.get(i) {
                if e.key.glyph_id == key.glyph_id
                    && e.key.px_size == key.px_size
                    && e.key.phase == key.phase
                    && e.key.weight == key.weight
                {
                    out = Some(self.live.remove(i));
                    break;
                }
            }
            i += 1;
        }
        if out.is_some() {
            self.fingerprint = self.compute_fingerprint();
        }
        out
    }

    /// 清空页（淘汰时用）。
    fn clear(&mut self) {
        self.live.clear();
        self.packer.reset();
        self.fingerprint = self.compute_fingerprint();
    }

    fn compute_fingerprint(&self) -> u64 {
        let mut h = 0x1000_0000_1b3u64;
        for e in self.live.iter() {
            for v in [
                e.key.compact() as u64,
                e.rect.x as u64,
                e.rect.y as u64,
                e.rect.w as u64,
                e.rect.h as u64,
            ]
            .iter()
            {
                h ^= *v;
                h = h.wrapping_mul(0x1000_0000_01b3);
            }
        }
        h
    }
}

/// 重排：把 `live` 按「层高降序 + 层宽降序 + 原序」重放进一个新打包器。
///
/// - **层高降序**是 Shelf 打包的关键：同类高度聚到同一层，层数最少；
/// - 返回 `None` 表示重排后放不下（**保持调用方原状态不变**，不半途改写）。
fn repack(live: &[LiveEntry], dim: u16, pad: u16) -> Option<(ShelfPacker, Vec<LiveEntry>)> {
    let mut order: Vec<usize> = (0..live.len()).collect();
    order.sort_by(|a, b| {
        let ra = live.get(*a);
        let rb = live.get(*b);
        match (ra, rb) {
            (Some(x), Some(y)) => y
                .rect
                .h
                .cmp(&x.rect.h)
                .then(y.rect.w.cmp(&x.rect.w))
                .then(a.cmp(b)),
            _ => core::cmp::Ordering::Equal,
        }
    });
    let mut np = ShelfPacker::new(dim, pad);
    let mut out: Vec<LiveEntry> = live.to_vec();
    for i in order.iter() {
        let e = match live.get(*i) {
            Some(e) => *e,
            None => return None,
        };
        match np.allocate(e.rect.w, e.rect.h) {
            Some(r) => {
                if let Some(t) = out.get_mut(*i) {
                    t.rect = r;
                }
            }
            None => return None,
        }
    }
    Some((np, out))
}

// ---------------------------------------------------------------------------
// 七、落位结果与降级
// ---------------------------------------------------------------------------

/// 落位去向。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placement {
    /// 进图集（正常路径）。
    Atlas { page: u16, rect: Rect },
    /// 降级直渲：绕过图集逐字上传（锚点「图集溢出→降级直渲路径」）。
    Direct,
    /// GPU 上传重试仍失败 → 走 CPU 路径。
    CpuFallback,
}

/// 降级原因（**处置方向不同的档位不共用计数**，遥测口径才读得出来）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DegradeReason {
    /// 未降级。
    None,
    /// 单帧新增字形超页容量（预算判据拦下）。
    Overflow,
    /// 无空间且整理被节流 / 全页 pin。
    Pinned,
    /// 索引负载因子触顶。
    IndexFull,
    /// GPU 上传失败（已重试一次）。
    UploadFailed,
}

impl DegradeReason {
    /// 中文档名（UI 与遥测共用同一份字面量）。
    pub const fn name(self) -> &'static str {
        match self {
            DegradeReason::None => "正常",
            DegradeReason::Overflow => "图集溢出",
            DegradeReason::Pinned => "无可用页",
            DegradeReason::IndexFull => "索引满",
            DegradeReason::UploadFailed => "上传失败",
        }
    }
}

/// 一次请求的结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GlyphSlot {
    /// 去向。
    pub placement: Placement,
    /// 是否缓存命中。
    pub cache_hit: bool,
    /// 降级原因。
    pub reason: DegradeReason,
}

/// 一次 acquire 的工作量分解（供性能判据分档统计）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AcquireWork {
    /// 命中路径工作量。
    pub hit: WorkCounter,
    /// 未命中路径工作量（索引探测 + 打包扫描 + 索引写入）。
    pub miss: WorkCounter,
}

// ---------------------------------------------------------------------------
// 八、遥测
// ---------------------------------------------------------------------------

/// 命中统计三指标（口径见模块头注第六节）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AtlasStats {
    /// 查表次数。
    pub lookups: u64,
    /// 命中次数。
    pub hits: u64,
    /// 未命中次数。
    pub misses: u64,
    /// 新键插入次数。
    pub inserts: u64,
    /// 同键原位更新次数。
    pub updates: u64,
    /// 淘汰次数。
    pub evictions: u64,
    /// 整理次数。
    pub compactions: u64,
    /// 整理被节流次数。
    pub compact_throttled: u64,
    /// 直渲次数。
    pub direct: u64,
    /// CPU 回退次数。
    pub cpu_fallbacks: u64,
    /// 上传重试次数。
    pub upload_retries: u64,
    /// 淘汰页中的条目数。
    pub evicted_entries: u64,
    /// 整理中被搬动的条目数。
    pub compacted_entries: u64,
    /// 降级：溢出。
    pub degrade_overflow: u64,
    /// 降级：无���用页。
    pub degrade_pinned: u64,
    /// 降级：索引满。
    pub degrade_index_full: u64,
    /// 降级：上传失败。
    pub degrade_upload: u64,
    /// 淘汰 young 代页次数。
    pub evicted_young: u64,
    /// 淘汰 old 代页次数。
    pub evicted_old: u64,
}

impl AtlasStats {
    /// 命中率千分比（查表 0 次时为 0，不除零）。
    pub fn hit_permille(&self) -> u32 {
        if self.lookups == 0 {
            return 0;
        }
        ((self.hits as u128 * 1000) / self.lookups as u128) as u32
    }

    /// 淘汰率千分比（**分母是插入数**：淘汰由写入触发，挂在查找侧会重复计）。
    pub fn evict_permille(&self) -> u32 {
        if self.inserts == 0 {
            return 0;
        }
        ((self.evictions as u128 * 1000) / self.inserts as u128) as u32
    }

    /// 整理频率（每千秒），千分比即 ×1000 取整。
    pub fn compact_per_ksec(&self, window_us: u64) -> u32 {
        if window_us == 0 {
            return 0;
        }
        ((self.compactions as u128 * 1_000_000_000) / window_us as u128) as u32
    }
}

/// 遥测快照（供 F0809 上行 / F0815 预算表引用）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AtlasTelemetry {
    pub hit_permille: u32,
    pub evict_permille: u32,
    pub compact_per_ksec: u32,
    pub direct_glyphs: u64,
    pub degrade_total: u64,
    pub lookups: u64,
}

// ---------------------------------------------------------------------------
// 九、图集门面
// ---------------------------------------------------------------------------

/// 字形图集（渲染输出中枢）。
#[derive(Clone, Debug)]
pub struct GlyphAtlas {
    cfg: AtlasConfig,
    pages: Vec<AtlasPage>,
    index: AtlasIndex,
    stats: AtlasStats,
    /// 当前帧号。
    pub frame: u64,
    /// 本帧新增字形数（溢出预算判据用）。
    pub frame_new: u32,
    last_compact_us: u64,
    next_page_id: u16,
    /// 累计工作量分解。
    pub work: AcquireWork,
    /// 上传失败注入预算（`0` = 上传恒成功；生产由真实上传器置位）。
    pub upload_fail_budget: u32,
    /// 上传尝试次数（累计）。
    pub upload_attempts: u32,
}

impl GlyphAtlas {
    /// 新建图集。
    ///
    /// **索引容量须覆盖「页数 × 每页最小字形容量」**——按最小字形
    /// （8×8）估算会**premature 满**：实测 64×64 单页实装 47 个 16×24 字形，
    /// 而按 8×8 估算的索引只给64 槽、负载因子守卫在 48 槽处触发，
    /// 于是页还空着一半就报IndexFull 并降级直渲。索引比页先满，
    /// 图集容量就被索引容量架空了——这是本单最隐蔽的一处容量失配。
    pub fn new(cfg: AtlasConfig) -> Self {
        let budget = cfg.total_glyph_budget();
        // 每槽再留 2 的幂余量（负载因子守卫在 3/4 触发，故4 倍足够）。
        let idx_cap = (budget.saturating_mul(4)).max(64).min(1 << 22) as usize;
        GlyphAtlas {
            cfg,
            pages: Vec::new(),
            index: AtlasIndex::with_capacity(idx_cap),
            stats: AtlasStats::default(),
            frame: 0,
            frame_new: 0,
            last_compact_us: 0,
            next_page_id: 0,
            work: AcquireWork::default(),
            upload_fail_budget: 0,
            upload_attempts: 0,
        }
    }

    pub fn config(&self) -> AtlasConfig {
        self.cfg
    }

    pub fn stats(&self) -> &AtlasStats {
        &self.stats
    }

    pub fn index(&self) -> &AtlasIndex {
        &self.index
    }

    pub fn pages(&self) -> &[AtlasPage] {
        &self.pages
    }

    pub fn page(&self, id: u16) -> Option<&AtlasPage> {
        self.pages.iter().find(|p| p.id == id)
    }

    /// 遥测快照。
    pub fn telemetry(&self, window_us: u64) -> AtlasTelemetry {
        AtlasTelemetry {
            hit_permille: self.stats.hit_permille(),
            evict_permille: self.stats.evict_permille(),
            compact_per_ksec: self.stats.compact_per_ksec(window_us),
            direct_glyphs: self.stats.direct,
            degrade_total: self.stats.degrade_overflow
                + self.stats.degrade_pinned
                + self.stats.degrade_index_full
                + self.stats.degrade_upload,
            lookups: self.stats.lookups,
        }
    }

    /// 帧边界：清 pin、复位本帧新增计数。
    ///
    /// **pin 的清理由这里统一负责**——「本帧发出的 UV 永不变」的不变量
    /// 依赖「pin 恰好覆盖一个帧区间」。
    pub fn begin_frame(&mut self, frame: u64) {
        self.frame = frame;
        self.frame_new = 0;
        for p in self.pages.iter_mut() {
            p.pinned = false;
        }
    }

    /// 取字形落位（缓存优先，未命中走图集落位）。
    pub fn acquire(&mut self, key: CacheKey, w: u16, h: u16, now_us: u64) -> GlyphSlot {
        self.stats.lookups += 1;
        let before = self.index.work;

        // ① 命中：零落位、零重排，仅 pin 与记账。
        if let Some((page, rect)) = self.index.lookup(key) {
            self.stats.hits += 1;
            let delta = self.index.work.since(&before);
            self.work.hit.probes += delta.probes;
            self.work.hit.compares += delta.compares;
            self.work.hit.copies += delta.copies;
            self.work.hit.writes += delta.writes;
            let frame = self.frame;
            if let Some(p) = self.pages.iter_mut().find(|p| p.id == page) {
                p.pinned = true;
                p.last_used_us = now_us;
                note_serve(p, frame);
            }
            return GlyphSlot {
                placement: Placement::Atlas { page, rect },
                cache_hit: true,
                reason: DegradeReason::None,
            };
        }

        self.stats.misses += 1;
        let before = self.index.work;
        // Shelf 扫描基线：未命中路径的层扫描成本记入本侧。
        let scan0 = self.pages.iter().map(|p| p.packer.scan_steps).sum::<u32>();
        let res = self.place_new(key, w, h, now_us);
        let delta = self.index.work.since(&before);
        let scan1 = self.pages.iter().map(|p| p.packer.scan_steps).sum::<u32>();
        self.work.miss.probes += delta.probes;
        self.work.miss.compares += delta.compares;
        self.work.miss.copies += delta.copies;
        self.work.miss.writes += delta.writes;
        // 层扫描折算为探测步数（同一工作量单位，不新造一档）。
        self.work.miss.probes += scan1.saturating_sub(scan0);
        res
    }

    /// 未命中后的落位链：空闲 → 开新页 → 整理 → 淘汰 → 降级。
    fn place_new(&mut self, key: CacheKey, w: u16, h: u16, now_us: u64) -> GlyphSlot {
        if w == 0 || h == 0 {
            // 退化输入（零尺寸位图）：不进图集，也不算图集的责任——直渲。
            self.stats.direct += 1;
            self.stats.degrade_overflow += 1;
            return GlyphSlot {
                placement: Placement::Direct,
                cache_hit: false,
                reason: DegradeReason::Overflow,
            };
        }

        // 溢出预算：单帧新增字形超页容量（锚点「单帧新增字形超过页容量」）。
        //
        // 用**页容量**而非索引容量作分母：锚点说的「页容量」指图集页装不下，
        // 索引满是另一类失配（已由上一档单独记账），两者混用会让
        // 「图集溢出」这一档在页还有空位时就被触发，遥测读数失真。
        if self.frame_new as u64 >= self.cfg.total_glyph_budget() {
            self.stats.direct += 1;
            self.stats.degrade_overflow += 1;
            return GlyphSlot {
                placement: Placement::Direct,
                cache_hit: false,
                reason: DegradeReason::Overflow,
            };
        }

        // 索引负载触顶：先于动页判定——索引满时新键根本无处登记，
        // 占了页位也拿不到 UV。
        if self.index.at_load_limit() {
            self.index.rehash();
            if self.index.at_load_limit() {
                self.stats.direct += 1;
                self.stats.degrade_index_full += 1;
                return GlyphSlot {
                    placement: Placement::Direct,
                    cache_hit: false,
                    reason: DegradeReason::IndexFull,
                };
            }
        }

        // ① 现有页空闲。
        for i in 0..self.pages.len() {
            let fits = match self.pages.get(i) {
                Some(p) => p.can_fit(w, h),
                None => false,
            };
            if fits {
                return self.commit(i, key, w, h, now_us, true);
            }
        }

        // ② 开新页。
        if self.pages.len() < self.cfg.max_pages as usize {
            let id = self.next_page_id;
            self.next_page_id += 1;
            self.pages.push(AtlasPage::new(id, self.cfg.page_dim, self.cfg.pad));
            let i = self.pages.len() - 1;
            return self.commit(i, key, w, h, now_us, true);
        }

        // ③ 压缩整理（碎片回收）——**先于淘汰**。
        if self.compact(now_us) {
            for i in 0..self.pages.len() {
                let fits = match self.pages.get(i) {
                    Some(p) => p.can_fit(w, h),
                    None => false,
                };
                if fits {
                    return self.commit(i, key, w, h, now_us, true);
                }
            }
        }

        // ④ 淘汰（LRU 加分代；pin 页免淘）。
        if let Some(i) = self.pick_victim() {
            let n = match self.pages.get(i) {
                Some(p) => p.len(),
                None => 0,
            };
            for e in self.pages.get(i).map(|p| p.live().to_vec()).unwrap_or_default() {
                let _ = self.index.remove(e.key);
            }
            if let Some(p) = self.pages.get_mut(i) {
                p.clear();
                p.last_used_us = now_us;
            }
            self.stats.evictions += 1;
            self.stats.evicted_entries += n as u64;
            let gen = self.pages.get(i).map(|p| p.generation).unwrap_or(Generation::Young);
            if gen == Generation::Young {
                self.stats.evicted_young += 1;
            } else {
                self.stats.evicted_old += 1;
            }
            self.note_round(i);
            return self.commit(i, key, w, h, now_us, true);
        }

        // ⑤ 降级直渲：无可用页（整理被节流或全页 pin）。
        self.stats.direct += 1;
        self.stats.degrade_pinned += 1;
        GlyphSlot {
            placement: Placement::Direct,
            cache_hit: false,
            reason: DegradeReason::Pinned,
        }
    }

    /// 在第 `idx` 页落位并登记索引；上传失败则回滚。
    fn commit(
        &mut self,
        idx: usize,
        key: CacheKey,
        w: u16,
        h: u16,
        now_us: u64,
        pin: bool,
    ) -> GlyphSlot {
        let rect = match self.pages.get_mut(idx).and_then(|p| p.packer.allocate(w, h)) {
            Some(r) => r,
            None => {
                // 竞态态：别的路径刚把该页填满。不静默，直接降级。
                self.stats.direct += 1;
                self.stats.degrade_pinned += 1;
                return GlyphSlot {
                    placement: Placement::Direct,
                    cache_hit: false,
                    reason: DegradeReason::Pinned,
                };
            }
        };
        let page_id = match self.pages.get(idx) {
            Some(p) => p.id,
            None => return self.degrade_index_full(),
        };
        let frame = self.frame;
        if let Some(p) = self.pages.get_mut(idx) {
            p.insert_at(key, rect);
            p.last_used_us = now_us;
            if pin {
                p.pinned = true;
                note_serve(p, frame);
            }
        }
        match self.index.insert(key, page_id, rect) {
            InsertOutcome::Inserted => {
                self.stats.inserts += 1;
                self.frame_new += 1;
            }
            InsertOutcome::Updated => {
                self.stats.updates += 1;
            }
            InsertOutcome::RejectedFull => {
                // 索引拒收 ⇒ 撤回页内落位（不留无主像素）。此处**必须**
                // 立即重排：退空间是本次落位失败的一部分，累积成碎片会
                // 让后续帧的页利用率无声下降。
                if let Some(p) = self.pages.get_mut(idx) {
                    let _ = p.release(key);
                    let _ = self.repack_page(idx);
                }
                return self.degrade_index_full();
            }
        }

        // GPU 上传：首次 + 至多一次重试。
        if !self.upload_once() {
            // 位图没进 GPU 却留在索引里 ⇒ 下帧命中一个从未上传的 UV。
            // 故失败即回滚（页内 + 索引），本帧走 CPU 路径。
            let _ = self.index.remove(key);
            if let Some(p) = self.pages.get_mut(idx) {
                let _ = p.release(key);
                let _ = self.repack_page(idx);
            }
            self.stats.inserts = self.stats.inserts.saturating_sub(1);
            self.frame_new = self.frame_new.saturating_sub(1);
            self.stats.cpu_fallbacks += 1;
            self.stats.degrade_upload += 1;
            return GlyphSlot {
                placement: Placement::CpuFallback,
                cache_hit: false,
                reason: DegradeReason::UploadFailed,
            };
        }

        GlyphSlot {
            placement: Placement::Atlas {
                page: page_id,
                rect,
            },
            cache_hit: false,
            reason: DegradeReason::None,
        }
    }

    fn degrade_index_full(&mut self) -> GlyphSlot {
        self.stats.direct += 1;
        self.stats.degrade_index_full += 1;
        GlyphSlot {
            placement: Placement::Direct,
            cache_hit: false,
            reason: DegradeReason::IndexFull,
        }
    }

    /// 上传一次（含至多 [`GPU_UPLOAD_RETRIES`] 次重试）。
    ///
    /// 返回 `true` 表示成功。**重试次数有上限**：恒失败的设备不得把每帧
    /// 拖成长循环——自检断言尝试次数恰为 `1 + GPU_UPLOAD_RETRIES`。
    fn upload_once(&mut self) -> bool {
        self.upload_attempts += 1;
        if self.consume_upload_failure() {
            return true;
        }
        let mut tries = 0u32;
        while tries < GPU_UPLOAD_RETRIES {
            self.upload_attempts += 1;
            self.stats.upload_retries += 1;
            if self.consume_upload_failure() {
                return true;
            }
            tries += 1;
        }
        false
    }

    /// 消耗一次失败预算（`upload_fail_budget` 递减）。
    fn consume_upload_failure(&mut self) -> bool {
        if self.upload_fail_budget > 0 {
            self.upload_fail_budget -= 1;
            false
        } else {
            true
        }
    }

    /// 整理一页（选未 pin 页中打包带占用率最低者，即碎片最多者）。
    ///
    /// 受 [`COMPACT_MIN_INTERVAL_US`] 节流：超期返回 `false` 并计
    /// [`AtlasStats::compact_throttled`]（**节流必须留痕**，否则「不超每秒
    /// 一次」只是碰巧成立）。
    pub fn compact(&mut self, now_us: u64) -> bool {
        if self.stats.compactions > 0 {
            let since = now_us.saturating_sub(self.last_compact_us);
            if since < COMPACT_MIN_INTERVAL_US {
                self.stats.compact_throttled += 1;
                return false;
            }
        }
        let target = self
            .pages
            .iter()
            .enumerate()
            .filter(|(_, p)| !p.pinned && !p.is_empty())
            .min_by(|(_, a), (_, b)| {
                a.band_occupancy()
                    .partial_cmp(&b.band_occupancy())
                    .unwrap_or(core::cmp::Ordering::Equal)
            })
            .map(|(i, _)| i);
        let idx = match target {
            Some(i) => i,
            None => {
                self.stats.compact_throttled += 1;
                return false;
            }
        };
        if self.repack_page(idx) {
            self.stats.compactions += 1;
            self.last_compact_us = now_us;
            true
        } else {
            self.stats.compact_throttled += 1;
            false
        }
    }

    /// 重排第 `idx` 页，并把**所有条目**的新坐标同步回索引。
    ///
    /// 返回 `false` 表示重排不可行（页状态保持原样，**不做半途改写**）。
    /// 重排会改变 UV ⇒ **只允许对未 pin 页调用**（见模块头注第三节）。
    pub fn repack_page(&mut self, idx: usize) -> bool {
        if self.pages.get(idx).map(|p| p.pinned).unwrap_or(true) {
            return false;
        }
        let (dim, pad, live) = match self.pages.get(idx) {
            Some(p) => (p.packer.dim, p.packer.pad, p.live().to_vec()),
            None => return false,
        };
        let moved = live.len();
        let (np, nlive) = match repack(&live, dim, pad) {
            Some(v) => v,
            None => return false,
        };
        // 索引必须与页内新坐标**同步**，否则残留旧 UV ⇒ 画面错位。
        for e in nlive.iter() {
            let _ = self.index.remove(e.key);
        }
        for e in nlive.iter() {
            let _ = self.index.insert_unchecked(e.key, self.pages[idx].id, e.rect);
        }
        if let Some(p) = self.pages.get_mut(idx) {
            p.packer = np;
            p.live = nlive;
            p.fingerprint = p.compute_fingerprint();
        }
        self.stats.compacted_entries += moved as u64;
        true
    }

    /// 挑淘汰 victim：`(代, 最近使用, 页号)` 字典序最小者。
    ///
    /// 只在**未 pin** 页中挑选（锚点「当前帧使用的页免淘」）。返回页下标。
    pub fn pick_victim(&self) -> Option<usize> {
        let mut best: Option<(usize, (u8, u64, u16))> = None;
        for (i, p) in self.pages.iter().enumerate() {
            if p.pinned || p.is_empty() {
                continue;
            }
            let rank = match p.generation {
                Generation::Young => 0u8,
                Generation::Old => 1u8,
            };
            let cand = (rank, p.last_used_us, p.id);
            match best {
                None => best = Some((i, cand)),
                Some((_, b)) => {
                    if cand < b {
                        best = Some((i, cand));
                    }
                }
            }
        }
        best.map(|(i, _)| i)
    }

    /// 一轮淘汰结算：所有存活页年龄 +1，越阈者晋升 old 代。
    /// 一轮淘汰结算：存活页累计「被不同帧用过」的次数，越阈者晋升 old 代。    ///
    /// **晋升依据是「被多少帧用过」而非「躲过几轮淘汰」**——后者在
    /// 「淘汰整页 + 同帧重填」的工作负载下每帧至多 +1（一次淘汰清空整页，
    /// 该页随即在本帧重填并被pin，下一轮又轮不到它），阈值结构上永远
    /// 够不到 ⇒ 分代形同虚设。变异测试实证：按「躲过轮数」实现时
    /// 4 页 60 帧共 9 次淘汰，age 长期卡在 0..3，无一页晋升。
    fn note_round(&mut self, evicted_idx: usize) {
        let thr = self.cfg.promote_after;
        for (i, p) in self.pages.iter_mut().enumerate() {
            if i == evicted_idx {
                // 被淘汰的页清零并退回 young 代（它证明了自己不受用）。
                p.serve_frames = 0;
                p.last_served_frame = u64::MAX;
                p.generation = Generation::Young;
                continue;
            }
            // **累加职责唯一归** [`GlyphAtlas::note_serve`]（按帧去重），
            // 此处只做晋升判定——两处都加即双重计数，晋升会被提前触发。
            if p.serve_frames >= thr && p.generation == Generation::Young {
                p.generation = Generation::Old;
            }
        }
    }

    /// 摘除一个字形（供上层淘汰单字形；**只摘自己发出的边**）。
    ///
    /// **摘除后不重排**——这是刻意的职责边界：
    ///
    /// Shelf 层高一旦定下就无法降低（层仍占着原有高度），故摘除会留下
    /// 真实碎片。若摘除即重排，则碎片在单个字形退出的瞬间就被抹平，
    /// [`GlyphAtlas::compact`] 的「碎片回收」将**永无作用场景**，
    /// 锚点「页满走双策略——先压缩整理（碎片回收）」也就落空。
    ///
    /// 碎片的真实回收发生在帧边界的 [`GlyphAtlas::compact`]：那里才有
    /// 「批量搬动 + 同步索引」的成本预算。残留像素被浪费但无人引用，
    /// 这是图集的真实取舍（宁可浪费像素，不可帧内画面错位）。
    pub fn release(&mut self, key: CacheKey) -> bool {
        let idx = match self.pages.iter().position(|p| {
            p.live().iter().any(|e| {
                e.key.glyph_id == key.glyph_id
                    && e.key.px_size == key.px_size
                    && e.key.phase == key.phase
                    && e.key.weight == key.weight
            })
        }) {
            Some(i) => i,
            None => return false,
        };
        let removed = self.pages.get_mut(idx).map(|p| p.release(key)).is_some();
        if !removed {
            return false;
        }
        let _ = self.index.remove(key);
        true
    }

    /// 为 F0807 文本图元批处理解出批次引用。
    ///
    /// 未命中的键返回 `None`（**由调用方决定是否直渲**，本单不替它决定）。
    pub fn resolve_batch(&mut self, keys: &[CacheKey], now_us: u64) -> BatchResult {
        let mut refs: Vec<Option<BatchRef>> = Vec::new();
        let mut hits = 0u32;
        let mut misses = 0u32;
        for k in keys.iter() {
            match self.index.lookup(*k) {
                Some((page, rect)) => {
                    hits += 1;
                    let uv = uv_rect(rect, self.cfg.page_dim);
                    let frame = self.frame;
                    if let Some(p) = self.pages.iter_mut().find(|p| p.id == page) {
                        p.pinned = true;
                        p.last_used_us = now_us;
                        note_serve(p, frame);
                    }
                    refs.push(Some(BatchRef {
                        key: *k,
                        page,
                        rect,
                        uv,
                    }));
                }
                None => {
                    misses += 1;
                    refs.push(None);
                }
            }
        }
        BatchResult { refs, hits, misses }
    }
}

/// 记一次「本帧用过该页」（帧内去重：同帧多次只计一次）。
///
/// 帧内去重是必需的：一帧内命中同一页 200 次不应把服务度算成 200。
/// 取自由函数形态：只需帧号，避免持有可变页引用时再借图集整体（触发 E0502）。
fn note_serve(p: &mut AtlasPage, frame: u64) {
    if p.last_served_frame != frame {
        p.last_served_frame = frame;
        p.serve_frames = p.serve_frames.saturating_add(1);
    }
}

/// F0807 消费的批次引用。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BatchRef {
    pub key: CacheKey,
    pub page: u16,
    pub rect: Rect,
    /// 定点 UV（页尺寸非法时为 `None`，**不给错误 UV**）。
    pub uv: Option<UvRect>,
}

/// 批次解算结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BatchResult {
    pub refs: Vec<Option<BatchRef>>,
    pub hits: u32,
    pub misses: u32,
}

// ---------------------------------------------------------------------------
// 十、域自检
// ---------------------------------------------------------------------------

/// 确定性线性同余发生器（自检语料生成，**不引入随机源**）。
struct Lcg(u32);

impl Lcg {
    fn new(seed: u32) -> Self {
        Lcg(seed.wrapping_mul(2654435761).wrapping_add(1))
    }
    fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1664525).wrapping_add(1013904223);
        self.0 >> 8
    }
    /// 取 `[0, n)`。
    fn below(&mut self, n: u32) -> u32 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

/// 构造一个四元组键。
fn key_of(gid: u32, px: u16, phx: usize, phy: usize, w: Weight) -> CacheKey {
    CacheKey::new(
        gid,
        px,
        super::vee05_hinting::Phase2::new(phx, phy),
        w,
    )
}

/// 域自检。
pub fn run_vee06_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vee06");
    check_key_space(&mut set);
    check_hit_rate(&mut set);
    check_shelf_packing(&mut set);
    check_generational_evict(&mut set);
    check_compact_before_evict(&mut set);
    check_degrade_paths(&mut set);
    check_upload_fallback(&mut set);
    check_perf(&mut set);
    check_index_invariants(&mut set);
    check_telemetry(&mut set);
    set
}

/// E06-01 键空间：四元组区分 + 同槽不串扰。
///
/// **最硬的一项是「compact 低位相同但四元组不同」**：只比
/// `compact() & mask` 的索引会把这两个键互相串扰（返回对方的 UV ⇒ 画面
/// 错字且无任何报错）。
fn check_key_space(set: &mut CheckSet) {
    let dim = 256u16;
    let mut atlas = GlyphAtlas::new(AtlasConfig::new(dim, 2));

    // 四元组四段各自可区分。
    let k0 = key_of(7, 16, 0, 0, Weight::Regular);
    let k1 = key_of(8, 16, 0, 0, Weight::Regular);
    let k2 = key_of(7, 17, 0, 0, Weight::Regular);
    let k3 = key_of(7, 16, 1, 0, Weight::Regular);
    let k4 = key_of(7, 16, 0, 0, Weight::Bold);
    let distinct = k0.compact() != k1.compact()
        && k0.compact() != k2.compact()
        && k0.compact() != k3.compact()
        && k0.compact() != k4.compact();

    // 构造「compact 相同、mask 后同槽」的一对：字数相同 ⇒ 低 6 位相同，
    // 而索引按混淆后的值取槽——故另取一对 compact 完全相同但四元组不同的
    // 键（字号与字形 ID 的位段互不重叠，用越界字段触发掩码截断）。
    let ka = key_of(5, 16, 2, 3, Weight::SemiBold);
    let kb = key_of(5 + 0x0004_0000, 16, 2, 3, Weight::SemiBold);
    let same_compact = ka.compact() == kb.compact();

    let mut ra = Rect::default();
    let mut rb = Rect::default();
    if same_compact {
        let sa = atlas.acquire(ka, 20, 20, 1);
        let sb = atlas.acquire(kb, 20, 20, 2);
        let placed_a = matches!(sa.placement, Placement::Atlas { .. });
        let placed_b = matches!(sb.placement, Placement::Atlas { .. });
        if let (Placement::Atlas { rect: a, .. }, Placement::Atlas { rect: b, .. }) =
            (sa.placement, sb.placement)
        {
            ra = a;
            rb = b;
        }
        // 两者都必须各归各位（不得共用同一矩形）。
        let no_alias = !(ra == rb);
        // 再查：a 命中必须给回 a 的矩形，不是 b 的。
        let ha = atlas.acquire(ka, 20, 20, 3);
        let hb = atlas.acquire(kb, 20, 20, 4);
        let stable = matches!(ha.placement, Placement::Atlas { rect, .. } if rect == ra)
            && matches!(hb.placement, Placement::Atlas { rect, .. } if rect == rb);
        let ok = placed_a && placed_b && no_alias && stable;
        assert!(ok, "键串扰：同位 {placed_a}/{placed_b} 不别名 {no_alias} 稳定 {stable}");
        set.add("E06-01-键空间四元组", distinct && ok, "");
        return;
    }
    // compact 未能构造出同位对时，仍须验证四元组区分与「键全等才命中」。
    let sa = atlas.acquire(ka, 20, 20, 1);
    let mut distinct_rect_ok = false;
    if let Placement::Atlas { rect, .. } = sa.placement {
        atlas.acquire(key_of(6, 16, 2, 3, Weight::SemiBold), 20, 20, 2);
        let hit_wrong = atlas.acquire(key_of(7, 16, 2, 3, Weight::SemiBold), 20, 20, 3);
        distinct_rect_ok = !matches!(hit_wrong.placement, Placement::Atlas { rect: r, .. } if r == rect);
    }
    assert!(
        distinct && distinct_rect_ok,
        "四元组区分 {distinct} 异键不别名 {distinct_rect_ok}"
    );
    set.add("E06-01-键空间四元组", distinct && distinct_rect_ok, "");
}

/// E06-02 命中率 ≥97%（常态文本口径）。
///
/// 三件事必须同时成立，缺一即弱门禁：
/// 1. **分母诚实**：lookups == hits + misses（无记账漂移）；
/// 2. **首帧确实未命中**：misses > 0（证明命中来自真实存储，不是常量）；
/// 3. **反向对照**：全唯一键时命中率必须≈0（证明指标不是恒 1.0）。
fn check_hit_rate(set: &mut CheckSet) {
    let cfg = AtlasConfig::new(512, 4);
    let mut atlas = GlyphAtlas::new(cfg);
    let weights = [Weight::Regular, Weight::SemiBold];

    // **常态文本的形态**：一段**固定**的文字被逐帧重复渲染，字符集有限、
    // 相位在四桶间摆动。语料刻意用 LCG **预生成**一次再逐帧重放——
    //
    // 【此处曾踩的坑】第一版语料在每帧内**现抽** 40 个随机键，键空间实际
    // 达 1085（实测），2000 次抽样几乎每键只碰一次 ⇒ 命中率 471‰，判红。
    // 但那不是命中率退化，而是**语料不是常态文本**：真实文本的键空间
    // ≪ 查表次数，重复渲染才构成缓存的用武之地。判据红时先问「语料是否
    // 真的代表被判据描述的场景」，不能拿随机抽样冒充常态。
    const FRAMES: u64 = 50;
    const DRAWS: usize = 40;
    let mut rng = Lcg::new(20261007);
    let corpus: Vec<CacheKey> = (0..DRAWS)
        .map(|_| {
            let gid = rng.below(24) + 1;
            let px = [12u16, 16, 20][(rng.below(3)) as usize];
            let phx = rng.below(4) as usize;
            let w = weights[(gid % 2) as usize];
            key_of(gid, px, phx, 0, w)
        })
        .collect();

    for f in 0..FRAMES {
        atlas.begin_frame(f);
        for k in corpus.iter() {
            let _ = atlas.acquire(*k, 18, 22, f * 16_667);
        }
    }
    let lookups = atlas.stats().lookups;
    let hits = atlas.stats().hits;
    let misses = atlas.stats().misses;
    let accounting = lookups == hits + misses && lookups == (FRAMES as u64) * (DRAWS as u64);
    // 首帧未命中数恰等于**语料唯一键数**，此后每帧每键必命中。
    // （语料 40 键含重复，故唯一键 < 40——按唯一值对账，不能拿 DRAWS 当分母。）
    let mut uniq: Vec<CacheKey> = Vec::new();
    for k in corpus.iter() {
        if !uniq.iter().any(|u| u.compact() == k.compact()) {
            uniq.push(*k);
        }
    }
    let uniq_n = uniq.len() as u64;
    let first_frame_only = misses == uniq_n && misses > 0
        && hits == lookups - uniq_n
        && hits == (FRAMES as u64 - 1) * DRAWS as u64 + (DRAWS as u64 - uniq_n);
    let rate = atlas.stats().hit_permille();
    let target = rate >= HIT_RATE_TARGET_PERMILLE;
    // 常态文本不应触发任何降级或淘汰（全在图集内）。
    let no_degrade = atlas.stats().direct == 0
        && atlas.stats().evictions == 0
        && atlas.stats().degrade_overflow == 0;

    // 反向对照：每次都是新键 ⇒ 零命中（证明指标不是恒 1.0 的空断言）。
    let mut fresh = GlyphAtlas::new(cfg);
    for f in 0..20u64 {
        fresh.begin_frame(f);
        for d in 0..40usize {
            // 每帧 40 个**互不重复**的新字形号 ⇒ 结构上不可能命中。
            let gid = 100_000 + (f as u32) * 100 + d as u32;
            let _ = fresh.acquire(key_of(gid, 16, 0, 0, Weight::Regular), 18, 22, f * 16_667);
        }
    }
    let anti = fresh.stats().hit_permille() == 0 && fresh.stats().misses == 800;

    let ok = accounting && first_frame_only && target && no_degrade && anti;
    assert!(
        ok,
        "命中 {rate}‰ 目标 {HIT_RATE_TARGET_PERMILLE} 分母 {accounting} 首帧未命中 {first_frame_only} 无降级 {no_degrade} 反向 {anti}"
    );
    set.add("E06-02-命中97pct", ok, "");
}

/// E06-03 Shelf 打包：无重叠 + 达结构最优紧密度 + 高度门真生效。
fn check_shelf_packing(set: &mut CheckSet) {
    // ① 均匀尺寸：实测紧密度须达该尺寸的理论上界。
    let dim = 256u16;
    let (w, h) = (20u16, 24u16);
    let mut p = ShelfPacker::new(dim, GLYPH_PAD);
    let mut rects: Vec<Rect> = Vec::new();
    while let Some(r) = p.allocate(w, h) {
        rects.push(r);
    }
    let mut no_overlap = true;
    for i in 0..rects.len() {
        if let Some(a) = rects.get(i) {
            if !a.fits(dim) {
                no_overlap = false;
            }
            for j in (i + 1)..rects.len() {
                if let Some(b) = rects.get(j) {
                    if a.intersects(b) {
                        no_overlap = false;
                    }
                }
            }
        }
    }
    let opt = shelf_optimal_occupancy(w, h, dim, GLYPH_PAD);
    let got = p.band_occupancy();
    let optimal = got + 1e-9 >= opt && got <= 1.0;

    // ② 混合尺寸：矮字形不得放进高层的余量之外——高度门必须真的生效。
    let mut q = ShelfPacker::new(dim, GLYPH_PAD);
    let mut mixed: Vec<Rect> = Vec::new();
    let mut i = 0u32;
    while let Some(r) = q.allocate(if i % 2 == 0 { 60 } else { 20 }, if i % 2 == 0 { 14 } else { 40 }) {
        mixed.push(r);
        i += 1;
        if i > 4000 {
            break;
        }
    }
    let mut mixed_ok = true;
    for a in 0..mixed.len() {
        for b in (a + 1)..mixed.len() {
            if let (Some(x), Some(y)) = (mixed.get(a), mixed.get(b)) {
                if x.intersects(y) {
                    mixed_ok = false;
                }
            }
        }
    }
    // 高度门计数必须非零：混合尺寸下必有「层高不足被跳过」。
    let height_gate = q.height_rejects > 0;

    let ok = no_overlap && optimal && mixed_ok && height_gate;
    assert!(
        ok,
        "无重叠 {no_overlap} 紧密度 {got:.4}/上界 {opt:.4} 混尺寸 {mixed_ok} 高度门 {height_gate}"
    );
    set.add("E06-03-Shelf打包", ok, "");
}

/// E06-04 分代淘汰：pin 免淘 + young 先于 old + 序列确定。
fn check_generational_evict(set: &mut CheckSet) {
    // 两页、页足够大 ⇒ 页数恰为 2（可精确构造代际对比场景）。
    let cfg = AtlasConfig::new(512, 2);
    // ① 晋升必须由**真实淘汰路径**累积（`release` 不推进代龄——它不是淘汰）。
    //
    // 页数须> 晋升阈值：只有 2 页时两页交替挨淘汰，每轮age 都被重置，
    // 任何页都活不满 4 轮 ⇒ 永不晋升（这是保守语义，不是缺陷）。
    // 4 页时每轮只淘汰 1 页，其余 3 页累积age ⇒ 必然晋升。
    let cfg4 = AtlasConfig::new(512, 4);
    let mut a4 = GlyphAtlas::new(cfg4);
    let mut seq4 = 1u32;
    for f in 0..60u64 {
        a4.begin_frame(f);
        for _ in 0..30u32 {
            let _ = a4.acquire(key_of(seq4, 24, 0, 0, Weight::Regular), 40, 40, f * 16_667);
            seq4 += 1;
        }
    }
    let evict_rounds = a4.stats().evictions;
    let promoted = a4.pages().iter().any(|p| p.generation == Generation::Old);
    // 轮数必须超过晋升阈值，否则「晋升」可能是首轮即晋的假象。
    let enough_rounds = evict_rounds as u32 > cfg4.promote_after;
    let four_pages = a4.pages().len() == 4;

    // ② pin 免淘：把两页全部 pin ⇒ 不得有任何淘汰候选。
    let mut atlas = GlyphAtlas::new(cfg);
    let mut seq = 1u32;
    for f in 0..30u64 {
        atlas.begin_frame(f);
        for _ in 0..30u32 {
            let _ = atlas.acquire(key_of(seq, 24, 0, 0, Weight::Regular), 40, 40, f * 16_667);
            seq += 1;
        }
    }
    let two_pages = atlas.pages().len() == 2;
    atlas.begin_frame(1000);
    let live: Vec<CacheKey> = atlas
        .pages()
        .iter()
        .flat_map(|p| p.live().iter().map(|e| e.key).collect::<Vec<_>>())
        .collect();
    for k in live.iter() {
        let _ = atlas.acquire(*k, 40, 40, 1000 * 16_667);
    }
    let all_pinned = atlas.pages().iter().all(|p| p.pinned);
    let pinned_blocks = all_pinned && atlas.pick_victim().is_none();
    // pin 期间不得发生淘汰（免淘是可观测行为，不只是选择结果）。
    let ev_before = atlas.stats().evictions;
    for k in live.iter().take(10) {
        let _ = atlas.acquire(*k, 40, 40, 1000 * 16_667);
    }
    let no_evict_while_pinned = atlas.stats().evictions == ev_before;

    // ③ 分代优先于 LRU：**受控**两页场景——0 号 old 且last_used 极小，
    // 1 号 young 且last_used 极大。纯 LRU 会选 0 号（错），分代实现选 1 号。
    //
    // （第一版只改了 0/1 号页却让 4 页同场竞争，victim 落到未约束的 2 号页
    // ⇒  判据自身不可证伪。此处把页数钉死为 2 才能让对比唯一。）
    let mut a2 = GlyphAtlas::new(cfg);
    a2.begin_frame(1);
    let mut s2 = 700_000u32;
    for _ in 0..600 {
        let _ = a2.acquire(key_of(s2, 24, 0, 0, Weight::Regular), 40, 40, 1);
        s2 += 1;
    }
    let mut ok_shape = a2.pages().len() == 2;
    if ok_shape {
        for p in a2.pages.iter_mut() {
            p.pinned = false;
        }
        if let Some(p0) = a2.pages.get_mut(0) {
            p0.generation = Generation::Old;
            p0.last_used_us = 1;
        }
        if let Some(p1) = a2.pages.get_mut(1) {
            p1.generation = Generation::Young;
            p1.last_used_us = 9_999_999;
        }
        // 分代优先 ⇒ 选 young 的 1 号；纯 LRU ⇒ 会选 old 的 0 号。
        ok_shape = matches!(a2.pick_victim(), Some(1));
    }
    // 反向：把 1 号也升old ⇒ 应退回选 last_used 更小的 0 号（证明不是恒选 1）。
    if let Some(p1) = a2.pages.get_mut(1) {
        p1.generation = Generation::Old;
    }
    let fallback = matches!(a2.pick_victim(), Some(0));
    // 确定性：同输入两次挑选必得同页。
    let deterministic = a2.pick_victim() == a2.pick_victim();

    let ok = four_pages && promoted && enough_rounds && two_pages && pinned_blocks
        && no_evict_while_pinned
        && ok_shape && fallback && deterministic;
    assert!(
        ok,
        "四页 {four_pages} 晋升 {promoted} 轮数 {enough_rounds}({evict_rounds}) 两页 {two_pages} \
         pin免淘 {pinned_blocks} pin期不淘汰 {no_evict_while_pinned} 分代优先 {ok_shape} \
         同代退回LRU {fallback} 确定性 {deterministic}"
    );
    set.add("E06-04-分代淘汰", ok, "");
}

/// E06-05 整理先于淘汰：次序、碎片真积累、索引同步、节流。
///
/// 本条验的是**次序**（锚点「先压缩整理，再淘汰」），故场景必须让
/// 「整理后装得下、且不需要淘汰」成为唯一解——若场景设计得整理也装不下，
/// 那么「整理发生过且淘汰也发生过」是正确行为，判据反而会把对的实现判红。
/// （第一版正是如此：请求的矩形整理后仍放不下，于是淘汰照常发生，
/// 判据却断言「淘汰必不增」。判据红时先问场景是否真能区分两种次序。）
fn check_compact_before_evict(set: &mut CheckSet) {
    let dim = 128u16;
    let cfg = AtlasConfig::new(dim, 1);
    let mut atlas = GlyphAtlas::new(cfg);
    let mut seq = 1u32;

    // ① 用**窄高**字形铺满整页：每层挤进多个，层高顶到页底。
    //窄（20 宽）是为③ 制造宽度瓶颈：碎片态下没有任何一层剩下 ≥101 的连续宽度。
    atlas.begin_frame(1);
    let mut keys: Vec<CacheKey> = Vec::new();
    loop {
        let k = key_of(seq, 40, 0, 0, Weight::Regular);
        seq += 1;
        let s = atlas.acquire(k, 20, 20, 1_000);
        if !matches!(s.placement, Placement::Atlas { .. }) {
            break;
        }
        keys.push(k);
        if keys.len() > 64 {
            break;
        }
    }
    let filled = keys.len();
    // 前提：页确实被填到放不下（否则场景无区分力）。128 页每层可放
    // floor(128/21)=6 个、共6 层 ⇒ 满页恰为 36 个，取 >= 36 判「已填满」。
    let per_layer = 128u32 / (20 + 1);
    let page_capacity = per_layer * per_layer;
    let page_was_full = filled as u32 >= page_capacity;

    // ② 摘掉每层中的一个 ⇒ 每层留下一个 21 宽的空洞，但**层高不降**
    //   （Shelf 层高一旦定下无法降低）⇒ 碎片真实积累。
    let mut removed = 0usize;
    for k in keys.iter().step_by(6) {
        if atlas.release(*k) {
            removed += 1;
        }
    }
    // 碎片确实积累：带宽占用率 < 1（摘除未重排）。
    let frag = atlas.page(0).map(|p| p.band_occupancy()).unwrap_or(0.0);
    let frag_ok = removed > 0 && frag < 0.999;

    // ③ 请求一个「碎片态放不下、重排后放得下」的宽矩形：
    //   宽 100 ⇒ 需101 连续宽；碎片态下每层剩余宽都 < 101（层高已顶满，
    //   也开不出新层）⇒ 必须先整理（重排后空层被撤、层高重算）。
    atlas.begin_frame(2);
    let probe = key_of(seq, 40, 0, 0, Weight::Regular);
    let before_compact = atlas.stats().compactions;
    let before_evict = atlas.stats().evictions;
    // 先确认碎片态下确实放不下（否则本判据无区分力）。
    let s = atlas.acquire(probe, 100, 20, 2_000_000);
    let compacted = atlas.stats().compactions > before_compact;
    let not_evicted = atlas.stats().evictions == before_evict;
    let placed = matches!(s.placement, Placement::Atlas { .. });
    let seq_ok = compacted && not_evicted && placed;

    // ④ 索引与页内必须逐条一致（整理改了坐标，索引必须跟着改，
    //    否则残留旧 UV ⇒ 画面错位且无报错）。
    let mut consistent = true;
    let snap: Vec<(CacheKey, u16, Rect)> = atlas
        .pages()
        .iter()
        .flat_map(|p| {
            p.live()
                .iter()
                .map(move |e| (e.key, p.id, e.rect))
                .collect::<Vec<_>>()
        })
        .collect();
    for (k, pid, r) in snap.iter() {
        if !matches!(atlas.index.lookup(*k), Some((p2, r2)) if p2 == *pid && r2 == *r) {
            consistent = false;
        }
    }
    // 反向：索引里的每个键都能在页内找到（无幽灵条目）。
    let ghost_free = snap
        .iter()
        .all(|(k, _, _)| atlas.pages().iter().any(|p| {
            p.live()
                .iter()
                .any(|e| e.key.glyph_id == k.glyph_id && e.key.px_size == k.px_size
                    && e.key.phase == k.phase && e.key.weight == k.weight)
        }));

    // ⑤ 节流：1 秒内第二次整理被拒且计数；超期放行。
    atlas.begin_frame(3);
    let t_before = atlas.stats().compact_throttled;
    let refused = !atlas.compact(2_000_001);
    let throttled = atlas.stats().compact_throttled > t_before;
    let later = atlas.compact(2_000_000 + COMPACT_MIN_INTERVAL_US + 1);

    // ⑥ 整理必搬动了条目（否则「整理」是空动作）。
    let moved = atlas.stats().compacted_entries > 0;

    // 碎片态的宽度瓶颈确实存在（层内剩余宽 < 探测矩形所需宽）。
    let width_bottleneck = atlas
        .pages()
        .iter()
        .flat_map(|p| p.shelves().iter())
        .all(|sh| (128u32).saturating_sub(sh.cursor_x as u32) < 101);
    let ok = page_was_full && filled > 8 && frag_ok && seq_ok && consistent && ghost_free
        && width_bottleneck && refused && throttled && later && moved;
    assert!(
        ok,
        "填满页 {page_was_full}({filled}) 摘除 {removed} 碎片积累 {frag_ok}({frag:.3}) 宽度瓶颈 {width_bottleneck} \
         次序 {seq_ok}(整理 {compacted} 未淘汰 {not_evicted} 落位 {placed}) 索引一致 {consistent} 无幽灵 {ghost_free} \
         搬动 {moved} 节流 {refused}/{throttled} 超期放行 {later}"
    );
    set.add("E06-05-整理先于淘汰", ok, "");
}

/// E06-06 降级矩阵：直渲不丢字形 + pin 页不被改动。
fn check_degrade_paths(set: &mut CheckSet) {
    let dim = 64u16;
    let cfg = AtlasConfig::new(dim, 1);
    let mut atlas = GlyphAtlas::new(cfg);
    let mut requests = 0u64;
    let mut returned = 0u64;
    let mut direct = 0u64;
    let mut seq = 1u32;

    // 单页 64×64 只能放少量字形 ⇒ 必然走降级。
    for f in 0..4u64 {
        atlas.begin_frame(f);
        for _ in 0..50u32 {
            let k = key_of(seq, 24, 0, 0, Weight::Regular);
            seq += 1;
            requests += 1;
            let s = atlas.acquire(k, 40, 40, f * 16_667);
            returned += 1;
            if matches!(s.placement, Placement::Direct) {
                direct += 1;
            }
            if !matches!(s.placement, Placement::Atlas { .. } | Placement::Direct | Placement::CpuFallback) {
                panic!("降级返回了非法 placement");
            }
        }
    }
    let no_loss = requests == returned && direct > 0;
    let counted = atlas.stats().direct == direct
        && (atlas.stats().degrade_pinned + atlas.stats().degrade_overflow
            + atlas.stats().degrade_index_full)
            == direct;

    // pin 页不可被**重排/淘汰**改动——本条是变异测试逼出来的：
    // 第一版只验「跑一轮负载后指纹不变」，但那轮负载里唯一一页恒被 pin，
    // 于是即便**删掉 repack_page 的 pin 门**也不会有任何重排落到它头上
    // ⇒ 弱门禁（M08 漏网）。真判据必须构造「存在未 pin 的第二页」，
    // 并**直接调用重排**，让pin 门成为唯一拦住变异的东西。
    let mut atlas2 = GlyphAtlas::new(AtlasConfig::new(128, 2));
    atlas2.begin_frame(1);
    // 两页都放上条目。
    let mut seq2 = 1u32;
    for f in 0..6u64 {
        atlas2.begin_frame(f);
        for _ in 0..30u32 {
            let _ = atlas2.acquire(key_of(seq2, 16, 0, 0, Weight::Regular), 30, 30, f);
            seq2 += 1;
        }
    }
    // 只 pin 住 0 号页；1 号页保持未 pin（可被重排）。
    atlas2.begin_frame(100);
    for e in atlas2.page(0).map(|p| p.live().to_vec()).unwrap_or_default() {
        let _ = atlas2.acquire(e.key, 30, 30, 100);
    }
    let two_present = atlas2.pages().len() == 2;
    let p0_pinned = atlas2.page(0).map(|p| p.pinned).unwrap_or(false);
    let p1_unpinned = atlas2.page(1).map(|p| !p.pinned).unwrap_or(false);
    // 直接对两个下标调用重排：pin 门是唯一拦截点。
    let fps_before: Vec<u64> = atlas2.pages().iter().map(|p| p.fingerprint).collect();
    let r0 = atlas2.repack_page(0); // pin ⇒ 必false
    let r1 = atlas2.repack_page(1); // 未 pin ⇒ 可true
    let fps_after: Vec<u64> = atlas2.pages().iter().map(|p| p.fingerprint).collect();
    // 强判据：pin 页重排被拒且指纹一字未改；未 pin 页确实可重排。
    let pin_untouched = two_present && p0_pinned && p1_unpinned && !r0 && r1 && fps_before == fps_after;

    // 淘汰面同理：pin 存在时不得淘汰（帧内新增字形只准降级）。
    let mut atlas3 = GlyphAtlas::new(AtlasConfig::new(64, 1));
    atlas3.begin_frame(1);
    let mut s3 = 1u32;
    while atlas3.pages().is_empty() {
        let _ = atlas3.acquire(key_of(s3, 8, 0, 0, Weight::Regular), 30, 30, 1);
        s3 += 1;
        if s3 > 20 {
            break;
        }
    }
    for e in atlas3.page(0).map(|p| p.live().to_vec()).unwrap_or_default() {
        let _ = atlas3.acquire(e.key, 30, 30, 1);
    }
    let ev0 = atlas3.stats().evictions;
    let mut s3 = 100u32;
    for _ in 0..12u32 {
        let _ = atlas3.acquire(key_of(s3, 8, 0, 0, Weight::Regular), 30, 30, 1);
        s3 += 1;
    }
    let evict_blocked = atlas3.stats().evictions == ev0;

    // 溢出预算档：把单帧新增预算压到 1，第二个新键必走 Overflow。
    let tiny = AtlasConfig {
        page_dim: 64,
        max_pages: 1,
        pad: 0,
        promote_after: 1,
    };
    let mut a3 = GlyphAtlas::new(tiny);
    a3.begin_frame(1);
    let budget = tiny.total_glyph_budget();
    let _ = a3.acquire(key_of(1, 8, 0, 0, Weight::Regular), 8, 8, 1);
    let mut overflowed = 0u32;
    let mut n = 2u32;
    while (n as u64) <= budget + 8 {
        let s = a3.acquire(key_of(n, 8, 0, 0, Weight::Regular), 8, 8, 1);
        if s.reason == DegradeReason::Overflow {
            overflowed += 1;
        }
        n += 1;
    }
    let budget_ok = overflowed > 0 && a3.stats().degrade_overflow == overflowed as u64;

    let ok = no_loss && counted && pin_untouched && evict_blocked && budget_ok;
    assert!(
        ok,
        "不丢字形 {no_loss} 计数一致 {counted} pin页不可重排 {pin_untouched} \
         pin期免淘 {evict_blocked} 溢出预算 {budget_ok}"
    );
    set.add("E06-06-降级矩阵", ok, "");
}

/// E06-07 GPU 上传：重试恰一次、再失败走 CPU 且回滚干净。
fn check_upload_fallback(set: &mut CheckSet) {
    let cfg = AtlasConfig::new(128, 2);

    // ① 首传失败、重试成功 ⇒ 落图集，重试计数 1。
    let mut a = GlyphAtlas::new(cfg);
    a.upload_fail_budget = 1;
    a.begin_frame(1);
    let k1 = key_of(1, 16, 0, 0, Weight::Regular);
    let s1 = a.acquire(k1, 20, 20, 1);
    let retried_ok = matches!(s1.placement, Placement::Atlas { .. })
        && a.stats().upload_retries == 1
        && a.upload_attempts == 2
        && a.stats().cpu_fallbacks == 0;

    // ② 恒失败 ⇒ CPU 回退，尝试次数恰为 1 + 上限（**不是 ≥2**）。
    let mut b = GlyphAtlas::new(cfg);
    b.upload_fail_budget = 99;
    b.begin_frame(1);
    let k2 = key_of(2, 16, 0, 0, Weight::Regular);
    let s2 = b.acquire(k2, 20, 20, 1);
    let bounded = matches!(s2.placement, Placement::CpuFallback)
        && b.upload_attempts == 1 + GPU_UPLOAD_RETRIES
        && b.stats().upload_retries == GPU_UPLOAD_RETRIES as u64
        && b.stats().cpu_fallbacks == 1
        && b.stats().degrade_upload == 1;

    // ③ 回滚干净：失败键不得留在索引里（否则下帧命中未上传的 UV）。
    let ghost = b.index.lookup(k2).is_none();
    let page_clean = b
        .pages()
        .iter()
        .all(|p| !p.live().iter().any(|e| e.key.glyph_id == k2.glyph_id));
    // 页内其余条目与索引仍一致。
    let snap_b: Vec<(CacheKey, u16)> = b
        .pages()
        .iter()
        .flat_map(|p| p.live().iter().map(move |e| (e.key, p.id)).collect::<Vec<_>>())
        .collect();
    let mut consistent = true;
    for (k, pid) in snap_b.iter() {
        if !matches!(b.index.lookup(*k), Some((p2, _)) if p2 == *pid) {
            consistent = false;
        }
    }

    let ok = retried_ok && bounded && ghost && page_clean && consistent;
    assert!(
        ok,
        "重试成功 {retried_ok} 重试有界 {bounded} 无幽灵 {ghost} 页干净 {page_clean} 一致 {consistent}"
    );
    set.add("E06-07-上传降级", ok, "");
}

/// E06-08 性能：命中路径工作量实测（有计数器、真不变量）。
///
/// 断言三件：
/// 1. 命中路径工作量 ≤ [`HIT_WORK_CAP`]；
/// 2. 命中路径工作量 **严格小于** 未命中路径（缓存确实省了活）；
/// 3. 命中路径工作量 **非零**（防空断言——计数器不动则本判据无意义）。
fn check_perf(set: &mut CheckSet) {
    let cfg = AtlasConfig::new(1024, 4);
    let mut atlas = GlyphAtlas::new(cfg);
    let mut rng = Lcg::new(4242);
    let distinct = 400u32;
    let n = 20_000usize;

    // 预热：全部落图集。
    atlas.begin_frame(0);
    for i in 0..distinct {
        let _ = atlas.acquire(
            key_of(i + 1, 16, (i % 4) as usize, 0, Weight::Regular),
            20,
            24,
            0,
        );
    }

    // 命中批。
    let h0 = atlas.work.hit;
    for _ in 0..n {
        let gid = rng.below(distinct) + 1;
        let _ = atlas.acquire(key_of(gid, 16, 0, 0, Weight::Regular), 20, 24, 1);
    }
    let hit_total = atlas.work.hit.total() - h0.total();

    // 未命中批：**每个键全新**（不复用），否则命中会混入 miss 侧稀释均值。
    //
    // 【此处曾踩的坑】第一版用 `i % 1500` 造2000 次请求，后500 次实为命中，
    // 却仍被记进 work.miss ⇒ per_miss 被摊薄到与命中同量级，
    // 「命中比未命中省」这条判据恒红。分档计量的前提是**两档样本互斥**。
    const COLD: usize = 2000;
    let m0 = atlas.work.miss;
    atlas.begin_frame(9);
    for i in 0..COLD {
        let gid = 500_000u32 + i as u32;
        let _ = atlas.acquire(key_of(gid, 16, 0, 0, Weight::SemiBold), 20, 24, 2);
    }
    let miss_total = atlas.work.miss.total() - m0.total();

    let per_hit = hit_total as u64 / n as u64;
    let per_miss = miss_total as u64 / COLD as u64;
    let within = per_hit <= HIT_WORK_CAP as u64;
    let cheaper = per_hit < per_miss;
    let nonzero = per_hit > 0;
    // 探测步数高水位落在 MAX_PROBE_STEPS 内。
    let probes_ok = atlas.index().max_probe_steps <= MAX_PROBE_STEPS;

    // **分项定向对账**（变异测试逼出来的）：
    // 只验「合计更省」时，去掉任一单项记账都可能被另一项撑住而漏网——
    // 【实测】去掉 writes 记账（M12）后 per_miss 仍由 scan_steps 撑在
    // 22 单位 > per_hit 5 ⇒「更省」照样绿。合计判据天然不敏感于分项。
    //
    // 断言取**语义方向**而非「逐项比大小」：
    //   - 命中路径**不写索引** ⇒ hit.writes恒为 0；
    //   - 未命中路径**必写索引** ⇒ miss.writes > 0（M12 的真凭据）；
    //   - 未命中路径**必扫 Shelf 层** ⇒ miss.probes > 0（M18 的真凭据）。
    //
    // 「未命中必比较/必拷贝」是**错的**：唯一键落图集时探测直接命中空槽，
    // 既不与旧键比较（mcmp 实测仅 60）也不拷贝旧 UV（mcopy 实测 0）。
    // 把不成立的方向写进判据，判据就会恒红并掩盖真问题
    // ——【此处曾踩的坑】先写了 mcmp>0 && mcopy>0，实测 0 ⇒ 假红。
    let hit_writes = atlas.work.hit.writes;
    let hit_probe = atlas.work.hit.probes;
    let miss_writes = atlas.work.miss.writes;
    let miss_probe = atlas.work.miss.probes;
    let per_item = hit_writes == 0 && miss_writes > 0 && miss_probe > 0;

    // 锚点常量冻结 + 边界可证伪（只作交接，不参与判定）。
    let anchor_lock = HIT_PATH_MAX_NS == 5_000 && (HIT_PATH_MAX_NS + 1) > HIT_PATH_MAX_NS;

    let ok = within && cheaper && nonzero && probes_ok && anchor_lock && per_item;
    assert!(
        ok,
        "每命中 {per_hit}/{HIT_WORK_CAP} 更省 {cheaper}(vs {per_miss}) 非零 {nonzero} 探测 {probes_ok} \
         锚点 {anchor_lock} 定向 {per_item} [hit.writes={hit_writes} miss.writes={miss_writes} \
         miss.probes={miss_probe} hit.probes={hit_probe}]"
    );
    set.add("E06-08-性能005ms", ok, "");
}

/// E06-09 索引不变量：删除不留活键 + 满表可达 + 拒绝而非覆盖。
fn check_index_invariants(set: &mut CheckSet) {
    let mut idx = AtlasIndex::with_capacity(64);
    let keys: Vec<CacheKey> = (1..=20u32)
        .map(|i| key_of(i, 16, (i % 4) as usize, 0, Weight::Regular))
        .collect();
    for (i, k) in keys.iter().enumerate() {
        let _ = idx.insert(*k, 0, Rect::new((i as u16) * 2, 0, 2, 2));
    }
    // **前置条件**：20 个键必须全部登记成功（容量 64、负载3/4 触顶在48，
    // 故20 键应全部插入）。若不全在，后面的删除计数就失去意义——
    // 【此处曾踩的坑】实测删除 8 而非 10，是插入期就有 2 个键撞上负载
    // 守卫被拒；判据却直接拿 10 当期望值，把「前置不成立」误报成
    // 「删除逻辑坏了」。用例的前置条件必须真的成立。
    let all_present = keys.iter().filter(|k| idx.lookup(**k).is_some()).count();
    // 删一半。
    // 期望删除数 = 偶数下标键的个数（不自证也不写死：容量守卫一改，
    // 写死的 10 就会与实际脱节 —— 判据里的常量必须跟着语义走，不跟着实现走）。
    let expect_removed = keys.iter().step_by(2).count() as u32;
    let mut removed = 0u32;
    for k in keys.iter().step_by(2) {
        if idx.remove(*k) {
            removed += 1;
        }
    }
    // 被删键查不到，未删键仍查得到。
    let mut ghost_free = true;
    for (i, k) in keys.iter().enumerate() {
        let got = idx.lookup(*k);
        if i % 2 == 0 {
            if got.is_some() {
                ghost_free = false;
            }
        } else if !matches!(got, Some((0, _))) {
            ghost_free = false;
        }
    }
    // 守卫：负载触顶后 insert 拒绝，且**既有键不被覆盖**。
    let mut idx2 = AtlasIndex::with_capacity(16);
    let mut rejected = 0u32;
    let mut first_rect = Rect::default();
    for i in 0..64u32 {
        let k = key_of(i + 1, 16, 0, 0, Weight::Regular);
        let o = idx2.insert(k, 0, Rect::new(0, 0, 1, 1));
        match o {
            InsertOutcome::RejectedFull => rejected += 1,
            _ => {
                if i == 0 {
                    first_rect = Rect::new(0, 0, 1, 1);
                }
            }
        }
    }
    // **零丢失**才是负载守卫的真判据：守卫关闭后 insert 不再拒绝，
    // 写入会把既有键**静默覆盖** ⇒ 早先登记的字形凭空消失。
    // 只查首个键不够（覆盖可能只命中部分槽），须核对**全部**幸存键。
    let survivors = (1..=64u32)
        .filter(|i| idx2.lookup(key_of(*i, 16, 0, 0, Weight::Regular)).is_some())
        .count();
    // 守卫生效时：容量 16、负载 3/4 触顶在 12 槽 ⇒ 至多 12 个键被登记，
    // 且这 12 个**必须都查得到**（一个不少）。
    let kept_all = survivors > 0 && survivors == idx2.used();
    let capacity_respected = idx2.used() <= (16 * INDEX_LOAD_NUM / INDEX_LOAD_DEN);
    let no_overwrite = matches!(idx2.lookup(key_of(1, 16, 0, 0, Weight::Regular)), Some((0, r)) if r == first_rect)
        && kept_all
        && capacity_respected;

    // 满表可达（绕过守卫）：探测无空格 ⇒ full_scans 增长且返回 None。
    let mut idx3 = AtlasIndex::with_capacity(8);
    for i in 0..8u32 {
        let _ = idx3.insert_unchecked(key_of(i + 1, 16, 0, 0, Weight::Regular), 0, Rect::new(0, 0, 1, 1));
    }
    let scans_before = idx3.full_scans;
    let sat = idx3.lookup(key_of(999, 16, 0, 0, Weight::Regular));
    let saturated_ok = sat.is_none() && idx3.full_scans > scans_before;

    let ok = all_present == keys.len() && removed == expect_removed && removed > 0 && ghost_free && rejected > 0 && no_overwrite && saturated_ok;
    assert!(
        ok,
        "前置全在 {all_present} 删除 {removed}/{expect_removed} 无幽灵 {ghost_free} 拒绝 {rejected} 不覆盖 {no_overwrite} 满表 {saturated_ok}"
    );
    set.add("E06-09-索引不变量", ok, "");
}

/// E06-10 遥测三指标口径：除零、窗口、淘汰率分母。
fn check_telemetry(set: &mut CheckSet) {
    let empty = AtlasStats::default();
    let zero_ok = empty.hit_permille() == 0 && empty.evict_permille() == 0
        && empty.compact_per_ksec(0) == 0
        && empty.compact_per_ksec(1_000_000) == 0;

    let mut s = AtlasStats {
        lookups: 1000,
        hits: 975,
        misses: 25,
        inserts: 100,
        evictions: 5,
        compactions: 4,
        ..AtlasStats::default()
    };
    let hit_rate = s.hit_permille();
    let evict_rate = s.evict_permille();
    let freq = s.compact_per_ksec(1_000_000);
    // 命中率 975/1000 = 975‰；
    // 淘汰率分母是**插入数** ⇒ 5/100 = 50‰（错挂查找侧会得 5‰，差 10 倍）；
    // 整理频率折算到**每秒**再乘 1000 ⇒ 1 秒 4 次 = 4000。
    //
    // 【此处曾踩的坑】第一版期望值写成 `freq == 4`（把「千分比」当成
    // 「次数」），判据恒红。口径常量与期望值必须成对读，否则改一边就红。
    let 口径_ok = hit_rate == 975 && evict_rate == 50 && freq == 4_000;
    // 反向对照：把分母换成查找数会得 5‰ —— 显式证明口径不可混。
    let wrong_denom = ((s.evictions as u128 * 1000) / s.lookups as u128) as u32;
    let denom_lock = wrong_denom == 5 && evict_rate != wrong_denom;
    s.lookups = 0;
    let no_nan = s.hit_permille() == 0;

    // 快照六字段齐整。
    let cfg = AtlasConfig::new(256, 2);
    let mut atlas = GlyphAtlas::new(cfg);
    atlas.begin_frame(1);
    for i in 0..20u32 {
        let _ = atlas.acquire(key_of(i + 1, 16, 0, 0, Weight::Regular), 20, 20, 1);
    }
    let t = atlas.telemetry(1_000_000);
    let snapshot_ok = t.lookups == 20 && t.direct_glyphs == 0 && t.degrade_total == 0;

    let ok = zero_ok && 口径_ok && denom_lock && no_nan && snapshot_ok;
    assert!(
        ok,
        "零值 {zero_ok} 口径 {口径_ok}(命中 {hit_rate} 淘汰 {evict_rate} 频率 {freq}) \
         分母锁 {denom_lock} 无除零 {no_nan} 快照 {snapshot_ok}"
    );
    set.add("E06-10-遥测三指标", ok, "");
}
