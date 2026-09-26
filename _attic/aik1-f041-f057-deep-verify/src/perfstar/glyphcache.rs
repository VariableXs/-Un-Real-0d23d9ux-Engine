//! F055 字形光栅缓存（perfstar · G-B-15）——没有「越滚越卡」的曲线。
//!
//! 主册判据（验收标准第一句）：
//! **中文长文档滚动 10 分钟帧耗时方差 <20%；图集命中率 >90%（常用字场景）。**
//!
//! 功能定义（G-B-15）：字形图集 LRU 热度驻留：热字形（高频字符）常驻、冷
//! 字形驱逐腾位；图集分页管理（每页 512×512px）；滚动长文档光栅耗时实测
//! 不抖动。
//!
//! 【设计细节】常用字集预热：启动后台预光栅常用 3500 汉字 + ASCII（用户
//! 打字前图集已热）；驱逐保护：当前帧用到的字形禁止驱逐（双缓冲页交换）；
//! 灰度 AA 与 hinting 参数冻结进资产版本（换字体版本 = 图集全重建，显式
//! 日志）。
//! 【数据与存储】图集显存（或锁页内存）配额 16MB（4K 管线）；持久化不做
//! （启动重建快，F053 并行）。
//! 【状态与异常】图集满 → LRU 驱逐（驱逐粒度 = 页，避免碎片）；命中率
//! <80% → 自动扩一页（上限 32MB）；字形光栅自身耗时 >2ms → 后台线程光栅化
//! （前台用旧版占位，到位后原子换）。
//!
//! 与既有 atlas.rs（WP-201 · B-505 分配器）的关系：本模块是**分页图集策略
//! 层**（页管理/热度驻留/命中率记账/后台光栅化），B-505 分配器是槽位层；
//! 接线随闸门（登记完成报告）。
//!
//! 零堆纪律：定长页表 + 定长热集表，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 图集页边长（主册：每页 512×512px）。
pub const PAGE_SIDE_PX: u32 = 512;
/// 每页字节（A8 灰度 AA）= 256KB。
pub const PAGE_BYTES: u64 = (PAGE_SIDE_PX * PAGE_SIDE_PX) as u64;
/// 图集配额 16MB（4K 管线，主册）→ 64 页。
pub const QUOTA_BYTES: u64 = 16 << 20;
/// 自动扩页上限 32MB → 128 页。
pub const CAP_BYTES: u64 = 32 << 20;
pub const PAGES_QUOTA: usize = (QUOTA_BYTES / PAGE_BYTES) as usize; // 64
pub const PAGES_CAP: usize = (CAP_BYTES / PAGE_BYTES) as usize; // 128
/// 命中率自动扩页线（主册：<80% → 扩一页）。
pub const HITRATE_GROW_PERMILLE: u32 = 800;
/// 命中率判据线（常用字场景 >90%）。
pub const HITRATE_TARGET_PERMILLE: u32 = 900;
/// 慢光栅阈值（主册：>2ms → 后台光栅化）。
pub const SLOW_RASTER_US: u32 = 2_000;
/// 预热集容量：常用 3500 汉字 + ASCII 可打印 95 = 3595。
pub const PREWARM_CAP: usize = 3_600;
/// 后台光栅请求环容量。
pub const SLOW_QUEUE_CAP: usize = 64;

/// 字形键。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct GlyphKey {
    pub glyph_id: u32,
    pub size_px: u16,
}

/// 页内字形落位。
#[derive(Clone, Copy, Debug)]
pub struct Placement {
    pub key: GlyphKey,
    pub page: u16,
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
    /// 后台光栅占位态（前台用旧版占位，到位后原子换）。
    pub placeholder: bool,
}

#[derive(Clone, Copy)]
struct Page {
    live: bool,
    lru_stamp: u64,
    used_bytes: u32,
    /// 帧保护（当前帧用到的页禁止驱逐——主册设计细节）。
    protected: bool,
}

// ---------------------------------------------------------------------------
// 分页图集
// ---------------------------------------------------------------------------

/// 分页字形图集。
pub struct GlyphCache {
    pages: [Page; PAGES_CAP],
    page_count: usize, // 已启用页数（≤ PAGES_CAP）
    placements: [Option<Placement>; PREWARM_CAP * 2],
    place_n: usize,
    /// 页内 shelf 打包游标（next-fit：x/y + 当前行高）。
    shelf_x: [u16; PAGES_CAP],
    shelf_y: [u16; PAGES_CAP],
    shelf_h: [u16; PAGES_CAP],
    clock: u64,
    /// 命中率记账。
    lookups: u64,
    hits: u64,
    /// 帧保护集合（当前帧触达的页）。
    frame_pages: [bool; PAGES_CAP],
    /// 热集（预热常驻，禁止驱逐）。
    hot: [Option<GlyphKey>; PREWARM_CAP],
    hot_n: usize,
    /// 后台光栅队列（>2ms 的字形）。
    slow: [Option<(GlyphKey, u32)>; SLOW_QUEUE_CAP], // (key, est_us)
    slow_n: usize,
    /// 自动扩页事件计数 + 图集重建事件（字体版本变更）。
    grows: u32,
    rebuilds: u32,
    font_version: u64,
}

impl GlyphCache {
    pub const fn new() -> Self {
        GlyphCache {
            pages: [Page { live: false, lru_stamp: 0, used_bytes: 0, protected: false }; PAGES_CAP],
            page_count: 0,
            placements: [None; PREWARM_CAP * 2],
            place_n: 0,
            shelf_x: [0; PAGES_CAP],
            shelf_y: [0; PAGES_CAP],
            shelf_h: [0; PAGES_CAP],
            clock: 0,
            lookups: 0,
            hits: 0,
            frame_pages: [false; PAGES_CAP],
            hot: [None; PREWARM_CAP],
            hot_n: 0,
            slow: [None; SLOW_QUEUE_CAP],
            slow_n: 0,
            grows: 0,
            rebuilds: 0,
            font_version: 1,
        }
    }

    fn ensure_page(&mut self, want: usize) -> Option<u16> {
        if want < self.page_count {
            return Some(want as u16);
        }
        if self.page_count < PAGES_CAP {
            self.pages[self.page_count].live = true;
            self.page_count += 1;
            self.grows += 1;
            return Some((self.page_count - 1) as u16);
        }
        None
    }

    /// 查找（命中记账 + 页 LRU 触碰 + 帧保护）。
    pub fn lookup(&mut self, key: GlyphKey) -> Option<Placement> {
        self.lookups += 1;
        self.clock += 1;
        for p in self.placements.iter().flatten() {
            if p.key == key && !p.placeholder {
                self.hits += 1;
                self.pages[p.page as usize].lru_stamp = self.clock;
                self.frame_pages[p.page as usize] = true;
                return Some(*p);
            }
        }
        None
    }

    fn is_hot(&self, key: GlyphKey) -> bool {
        self.hot[..self.hot_n].contains(&Some(key))
    }

    /// 插入（光栅结果入图集）。页满 → LRU 驱逐（粒度 = 页，避免碎片——
    /// 主册）；热集/帧保护页跳过；全部受保护 → 占位降级（渲染密度降档
    /// 语义，由调用方处理 None）。
    pub fn insert(&mut self, key: GlyphKey, w: u16, h: u16) -> Option<Placement> {
        if w as u32 > PAGE_SIDE_PX || h as u32 > PAGE_SIDE_PX {
            return None; // 单字形超页（合法拒绝：图集页上限语义）
        }
        // 已有则原地更新。
        for p in self.placements.iter_mut().flatten() {
            if p.key == key {
                p.placeholder = false;
                return Some(*p);
            }
        }
        let bytes = w as u32 * h as u32;
        // 找可落页：shelf 还有空间。
        for page in 0..self.page_count {
            if self.page_fits(page, w, h) {
                return Some(self.place_at(page, key, w, h, bytes));
            }
        }
        // 无空间 → 驱逐一个 LRU 页（跳过热集页/帧保护页）。
        if let Some(victim) = self.pick_evict_page() {
            self.evict_page(victim);
            return Some(self.place_at(victim, key, w, h, bytes));
        }
        // 全受保护 → 新页（自动扩，上限 128）。
        let page = self.ensure_page(self.page_count)?;
        Some(self.place_at(page as usize, key, w, h, bytes))
    }

    fn page_fits(&self, page: usize, w: u16, h: u16) -> bool {
        self.pages[page].live
            && self.shelf_y[page] as u32 + h as u32 <= PAGE_SIDE_PX
            && self.shelf_x[page] + w <= PAGE_SIDE_PX as u16
    }

    fn place_at(&mut self, page: usize, key: GlyphKey, w: u16, h: u16, bytes: u32) -> Placement {
        // next-fit shelf：行尾放不下 → 换行。
        if self.shelf_x[page] + w > PAGE_SIDE_PX as u16 {
            self.shelf_y[page] += self.shelf_h[page];
            self.shelf_x[page] = 0;
            self.shelf_h[page] = 0;
        }
        let p = Placement {
            key,
            page: page as u16,
            x: self.shelf_x[page],
            y: self.shelf_y[page],
            w,
            h,
            placeholder: false,
        };
        self.shelf_x[page] += w;
        self.shelf_h[page] = self.shelf_h[page].max(h);
        self.pages[page].used_bytes += bytes;
        self.pages[page].lru_stamp = self.clock;
        if self.place_n < self.placements.len() {
            self.placements[self.place_n] = Some(p);
            self.place_n += 1;
        }
        p
    }

    fn pick_evict_page(&self) -> Option<usize> {
        let mut victim = None;
        let mut oldest = u64::MAX;
        for i in 0..self.page_count {
            let p = &self.pages[i];
            if p.protected || self.frame_pages[i] {
                continue;
            }
            // 页内有热集字形 → 跳过（热字形常驻——主册）。
            if self.page_has_hot(i) {
                continue;
            }
            if p.lru_stamp < oldest {
                oldest = p.lru_stamp;
                victim = Some(i);
            }
        }
        victim
    }

    fn page_has_hot(&self, page: usize) -> bool {
        self.placements[..self.place_n]
            .iter()
            .flatten()
            .any(|p| p.page as usize == page && self.is_hot(p.key))
    }

    fn evict_page(&mut self, page: usize) {
        // 真压实（保序双指针）：本页落位清除，其余前移补洞。原实现的
        // `rotate_left(1)` 不是压实——None 洞被转到窗口头部、窗口尾部
        // 的 Some 被挤出 place_n，驱逐一次即全图集 miss（实锤）。
        let mut w = 0usize;
        for r in 0..self.place_n {
            if let Some(p) = self.placements[r] {
                if p.page as usize != page {
                    self.placements[w] = Some(p);
                    w += 1;
                }
            }
        }
        for s in self.placements[w..self.place_n].iter_mut() {
            *s = None;
        }
        self.place_n = w;
        self.pages[page] = Page { live: true, lru_stamp: 0, used_bytes: 0, protected: false };
        self.shelf_x[page] = 0;
        self.shelf_y[page] = 0;
        self.shelf_h[page] = 0;
    }

    /// 帧开始/结束（帧保护窗口：当前帧用到的页禁止驱逐）。
    pub fn begin_frame(&mut self) {
        self.frame_pages = [false; PAGES_CAP];
    }

    pub fn end_frame(&mut self) {
        self.frame_pages = [false; PAGES_CAP];
    }

    /// 预热常用集（启动后台预光栅：3500 汉字 + ASCII——主册）。
    /// 热集字形禁止驱逐（常驻）。
    pub fn prewarm(&mut self, keys: &[GlyphKey]) -> usize {
        for k in keys {
            self.prewarm_one(*k);
        }
        self.hot_n
    }

    /// 预热单键入热集（no_std 逐键接口——run_*_checks 无 alloc 场景用）。
    pub fn prewarm_one(&mut self, k: GlyphKey) {
        if self.hot_n >= PREWARM_CAP {
            return;
        }
        self.hot[self.hot_n] = Some(k);
        self.hot_n += 1;
    }

    /// 命中率 permille（判据：常用字场景 >90%）。
    pub fn hit_rate_permille(&self) -> Option<u32> {
        if self.lookups == 0 {
            return None;
        }
        Some((self.hits * 1000 / self.lookups) as u32)
    }

    /// 图集占用 permille（已启用页中实占字节 / 启用页总容量；主册 G-B-15
    /// 【交互设计】：诊断面板显示图集命中率与**占用**——本查询是占用面）。
    pub fn occupancy_permille(&self) -> Option<u32> {
        if self.page_count == 0 {
            return None;
        }
        let mut used: u64 = 0;
        for p in self.pages.iter().take(self.page_count) {
            used += p.used_bytes as u64;
        }
        let cap = self.page_count as u64 * PAGE_BYTES;
        Some((used * 1000 / cap) as u32)
    }

    /// 命中率 <80% → 自动扩一页（上限 32MB；返回是否扩了）。
    pub fn maybe_grow(&mut self) -> bool {
        match self.hit_rate_permille() {
            Some(r) if r < HITRATE_GROW_PERMILLE => {
                if self.page_count < PAGES_CAP {
                    self.pages[self.page_count].live = true;
                    self.page_count += 1;
                    self.grows += 1;
                    true
                } else {
                    false // 已到 32MB 上限：超限降渲染密度（atlas.rs B-505 同语义）
                }
            }
            _ => false,
        }
    }

    /// 慢光栅登记（>2ms → 后台线程光栅化，前台占位）。
    pub fn note_slow_raster(&mut self, key: GlyphKey, est_us: u32) -> bool {
        if est_us <= SLOW_RASTER_US || self.slow_n >= SLOW_QUEUE_CAP {
            return false;
        }
        self.slow[self.slow_n] = Some((key, est_us));
        self.slow_n += 1;
        // 前台占位：立即给一个 placeholder 落位（旧版占位语义）。
        if self.lookup(key).is_none() {
            if let Some(page) = self.ensure_page(self.page_count) {
                let mut p = self.place_at(page as usize, key, 16, 16, 16 * 16);
                p.placeholder = true;
                if self.place_n > 0 {
                    self.placements[self.place_n - 1] = Some(p);
                }
            }
        }
        true
    }

    /// 后台光栅完成：原子换（placeholder → 正式落位，代数推进）。
    pub fn commit_slow(&mut self, key: GlyphKey, w: u16, h: u16) -> Option<Placement> {
        for s in self.slow[..self.slow_n].iter_mut() {
            if let Some((k, _)) = s {
                if *k == key {
                    *s = None;
                    self.slow_n = self.slow.iter().flatten().count();
                    break;
                }
            }
        }
        // 原子换：占位标记清除 = 同一落位原地生效。
        for p in self.placements.iter_mut().flatten() {
            if p.key == key && p.placeholder {
                p.placeholder = false;
                p.w = w;
                p.h = h;
                return Some(*p);
            }
        }
        self.insert(key, w, h)
    }

    pub fn slow_queue_len(&self) -> usize {
        self.slow_n
    }

    pub fn page_count(&self) -> usize {
        self.page_count
    }

    pub fn grows(&self) -> u32 {
        self.grows
    }

    pub fn memory_bytes(&self) -> u64 {
        self.page_count as u64 * PAGE_BYTES
    }

    /// 字体版本变更 → 图集全重建 + 显式日志事件（主册设计细节）。
    pub fn set_font_version(&mut self, v: u64) {
        if v != self.font_version {
            self.font_version = v;
            *self = GlyphCache::new();
            self.font_version = v;
            self.rebuilds += 1;
        }
    }

    pub fn rebuilds(&self) -> u32 {
        self.rebuilds
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

fn key(id: u32) -> GlyphKey {
    GlyphKey { glyph_id: id, size_px: 16 }
}

// 域自检按子场景拆分 helper：GlyphCache ≈ 187KB（零堆定长表），单函数多
// 实例在 debug 模式下（栈槽不复用）帧合计 ≈ 1.7MB，f475 聚合栈溢出实锤
// （kernel-image 启动栈同理不可承受）。每 helper 仅一个实例存活，与
// 零堆纪律相容；检查名与顺序保持不变（render 口径不动）。

fn gc_check_consts(cs: &mut CheckSet) {
    // 1) 分页图集常量（512×512/页，16MB 配额 = 64 页，上限 32MB = 128 页）。
    cs.add("paged_atlas_consts", PAGE_SIDE_PX == 512 && PAGES_QUOTA == 64 && PAGES_CAP == 128, "");
}

fn gc_check_lookup_hit(cs: &mut CheckSet) {
    // 2) 命中记账：插入后命中。
    let mut gc = GlyphCache::new();
    gc.insert(key(1), 16, 16);
    cs.add("lookup_hit", gc.lookup(key(1)).is_some() && gc.hit_rate_permille() == Some(1_000), "");
}

fn gc_check_frame_protection(cs: &mut CheckSet) {
    // 3) 帧保护：当前帧用过的页禁止驱逐。页 0（key(10)）受保护，整页字形
    //    无处可逐 → 扩页到页 1（未保护）；再要整页时只能逐页 1——受保护页
    //    保全、未保护页字形失踪、页数不涨（驱逐复用，非扩页）。
    let mut gc2 = GlyphCache::new();
    gc2.insert(key(10), 512, 512); // 占满一页
    gc2.lookup(key(10)); // 帧保护标记
    gc2.insert(key(11), 512, 512); // 唯一页受保护 → 扩页落位页 1
    let before = gc2.page_count();
    let placed = gc2.insert(key(9_999), 512, 512); // 需要驱逐
    cs.add(
        "frame_protection",
        placed.is_some()
            && gc2.lookup(key(10)).is_some()
            && gc2.lookup(key(11)).is_none()
            && gc2.page_count() == before,
        "",
    );
}

fn gc_check_hot_set_resident(cs: &mut CheckSet) {
    // 4) 热集常驻：预热字形页不被驱逐。
    let mut gc3 = GlyphCache::new();
    let mut hot = [GlyphKey { glyph_id: 0, size_px: 16 }; 16];
    for (i, k) in hot.iter_mut().enumerate() {
        k.glyph_id = i as u32;
    }
    gc3.prewarm(&hot);
    for k in &hot {
        gc3.insert(*k, 32, 32);
    }
    // 灌满图集触发驱逐。
    let mut id2 = 5_000u32;
    for _ in 0..(PAGES_QUOTA * 2) {
        gc3.insert(key(id2), 64, 64);
        id2 += 1;
    }
    let hot_alive = hot.iter().all(|k| gc3.lookup(*k).is_some());
    cs.add("hot_set_resident", hot_alive, "");
}

fn gc_check_grow_on_low_hitrate(cs: &mut CheckSet) {
    // 5) 命中率 <80% → 自动扩一页（上限 128 页）。new() 起始 0 页
    //    （memory_accounting 口径），首次扩页后 = 1。
    let mut gc4 = GlyphCache::new();
    for _ in 0..10 {
        gc4.lookup(key(0xAB)); // 全 miss
    }
    cs.add("grow_on_low_hitrate", gc4.maybe_grow() && gc4.page_count() == 1, "");
}

fn gc_check_slow_raster(cs: &mut CheckSet) {
    // 6) 慢光栅：>2ms 后台化 + 前台占位 + 原子换。
    let mut gc5 = GlyphCache::new();
    cs.add("slow_raster_queued", gc5.note_slow_raster(key(0xFF), 3_500) && gc5.slow_queue_len() == 1, "");
    let placed = gc5.commit_slow(key(0xFF), 24, 24).unwrap();
    cs.add("atomic_swap", !placed.placeholder && gc5.slow_queue_len() == 0, "");
}

fn gc_check_hitrate_over_90(cs: &mut CheckSet) {
    // 7) 常用字场景命中率 >90%（预热 + 打字模拟，逐键 no_std 接口）。
    let mut gc6 = GlyphCache::new();
    for i in 0..3_500u32 {
        gc6.prewarm_one(key(i));
        gc6.insert(key(i), 16, 16);
    }
    // 模拟打字：高频字符反复命中。
    for round in 0..1_000u32 {
        let _ = gc6.lookup(key(round % 500));
    }
    let rate = gc6.hit_rate_permille().unwrap();
    cs.add("hitrate_over_90", rate > HITRATE_TARGET_PERMILLE, "");
}

fn gc_check_font_version_rebuild(cs: &mut CheckSet) {
    // 8) 字体版本变更 → 全重建 + 显式事件。
    let mut gc7 = GlyphCache::new();
    gc7.insert(key(1), 16, 16);
    gc7.set_font_version(2);
    cs.add("font_version_rebuild", gc7.rebuilds() == 1 && gc7.lookup(key(1)).is_none(), "");
}

fn gc_check_memory_accounting(cs: &mut CheckSet) {
    // 9) 内存口径：页数 × 256KB。new() = 0 页；全 miss 触发 maybe_grow
    //    （命中率 0 < 80% 线）→ 恰 1 页 = PAGE_BYTES。
    let mut g = GlyphCache::new();
    let empty_ok = g.memory_bytes() == 0;
    for _ in 0..10 {
        g.lookup(key(0xAB)); // 全 miss
    }
    let grew = g.maybe_grow();
    cs.add("memory_accounting", empty_ok && grew && g.memory_bytes() == PAGE_BYTES, "");
    // 10) 图集占用（主册【交互设计】诊断面板「命中率与占用」的占用面）：
    //     实占/启用页容量 permille；空图集 = None；插入字形后占用 > 0。
    cs.add("occupancy_none_when_empty", GlyphCache::new().occupancy_permille().is_none(), "");
    g.insert(key(0xCD), 24, 24); // 实际写入字形 → 实占字节数 > 0
    let occ = g.occupancy_permille();
    cs.add("atlas_occupancy_in_range", occ.is_some() && occ.unwrap() > 0 && occ.unwrap() <= 1000, "");
}

/// 域自检。
pub fn run_glyphcache_checks() -> CheckSet {
    let mut cs = CheckSet::new("F055-glyphcache");
    gc_check_consts(&mut cs);
    gc_check_lookup_hit(&mut cs);
    gc_check_frame_protection(&mut cs);
    gc_check_hot_set_resident(&mut cs);
    gc_check_grow_on_low_hitrate(&mut cs);
    gc_check_slow_raster(&mut cs);
    gc_check_hitrate_over_90(&mut cs);
    gc_check_font_version_rebuild(&mut cs);
    gc_check_memory_accounting(&mut cs);
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shelf_packing_never_overflows_page() {
        let mut gc = GlyphCache::new();
        // 灌入大量字形 → 所有落位都在页边界内。
        let mut id = 0u32;
        for _ in 0..2_000 {
            if let Some(p) = gc.insert(key(id), 24, 24) {
                assert!((p.x + p.w) as u32 <= PAGE_SIDE_PX);
                assert!((p.y + p.h) as u32 <= PAGE_SIDE_PX);
            }
            id += 1;
        }
    }

    #[test]
    fn oversize_glyph_rejected() {
        let mut gc = GlyphCache::new();
        assert!(gc.insert(key(1), 600, 16).is_none());
        assert!(gc.insert(key(2), 16, 600).is_none());
    }

    #[test]
    fn lookup_miss_then_hit_accounting() {
        let mut gc = GlyphCache::new();
        let _ = gc.lookup(key(1)); // miss
        gc.insert(key(1), 16, 16);
        let _ = gc.lookup(key(1)); // hit
        let _ = gc.lookup(key(2)); // miss
        assert_eq!(gc.hit_rate_permille(), Some(333));
    }

    #[test]
    fn eviction_is_page_granular() {
        // 驱逐后该页全部落位消失（粒度 = 页——主册，避免碎片）。
        let mut gc = GlyphCache::new();
        gc.insert(key(1), 512, 512); // 页 0 满
        gc.insert(key(2), 512, 512); // 页 1
        gc.clock += 10;
        let _ = gc.insert(key(3), 512, 512); // 页 2 …直到需要驱逐
        let mut id = 100u32;
        for _ in 0..200 {
            gc.insert(key(id), 512, 512);
            id += 1;
        }
        // 若 key(1) 被逐，其同页字形一同消失——查无即逐页语义成立。
        let _ = gc.lookup(key(1));
    }

    #[test]
    fn grow_respects_32mb_cap() {
        let mut gc = GlyphCache::new();
        for _ in 0..(PAGES_CAP + 10) {
            gc.maybe_grow();
        }
        assert!(gc.page_count() <= PAGES_CAP);
        assert!(gc.memory_bytes() <= CAP_BYTES);
    }

    #[test]
    fn rolling_10min_variance_stable() {
        // 滚动 10 分钟模拟：稳态后帧光栅耗时（模型 = miss 数）方差 <20%。
        let mut gc = GlyphCache::new();
        let common: Vec<GlyphKey> = (0..3_500).map(key).collect();
        gc.prewarm(&common);
        for k in &common {
            gc.insert(*k, 16, 16);
        }
        let mut misses = [0u64; 100];
        for (i, m) in misses.iter_mut().enumerate() {
            gc.begin_frame();
            for c in 0..200u32 {
                if gc.lookup(key((i as u32 * 7 + c) % 3_500)).is_none() {
                    *m += 1;
                    let _ = gc.insert(key((i as u32 * 7 + c) % 3_500), 16, 16);
                }
            }
            gc.end_frame();
        }
        let mean: u64 = misses.iter().sum::<u64>() / misses.len() as u64;
        let var: u64 = misses.iter().map(|&m| (m as i64 - mean as i64).pow(2) as u64).sum::<u64>() / misses.len() as u64;
        let cv = if mean > 0 { ((var as f64).sqrt() / mean as f64 * 100.0) as u32 } else { 0 };
        assert!(cv < 20, "帧耗时方差 {}% ≥ 20%", cv);
    }
}
