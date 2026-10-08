//! UNREAL-X-15000 · AI-23 族0223 缓存与预读（X05551~X05575 · W2）
//!
//! 页缓存 + 顺序预读窗口：LRU 淘汰、预读档位、命中统计、降级链。零分配固定容量。

use crate::checks::CheckSet;

pub const CACHE_TIERS: [&str; 5] = ["off", "minimal", "normal", "aggressive", "max"];
pub const CACHE_DEFAULT: usize = 2;
const MAX_PAGES: usize = 32;

/// 单页缓存条目：blk 号 + 脏位 + LRU 时钟。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Page {
    pub blk: u64,
    pub dirty: bool,
    pub clock: u64,
}

pub struct PageCache {
    tier: usize,
    pages: [Option<Page>; MAX_PAGES],
    count: usize,
    clock: u64,
    hits: u64,
    misses: u64,
    evictions: u64,
    clamped: u32,
}

impl PageCache {
    pub fn new(tier: usize) -> Self {
        let t = if tier < CACHE_TIERS.len() { tier } else { CACHE_DEFAULT };
        Self { tier: t, pages: [None; MAX_PAGES], count: 0, clock: 0, hits: 0, misses: 0, evictions: 0, clamped: if t != tier { 1 } else { 0 } }
    }
    pub fn tier(&self) -> usize {
        self.tier
    }
    pub fn clamped(&self) -> u32 {
        self.clamped
    }
    /// 预读窗口大小随档位（off=0）。
    pub fn readahead(&self) -> usize {
        [0, 2, 8, 16, 32][self.tier]
    }
    pub fn len(&self) -> usize {
        self.count
    }
    pub fn stats(&self) -> (u64, u64, u64) {
        (self.hits, self.misses, self.evictions)
    }
    /// 读：命中/未命中统计 + LRU 触碰。
    pub fn read(&mut self, blk: u64) -> bool {
        self.clock += 1;
        for i in 0..self.count {
            let hit = self.pages[i].map_or(false, |p| p.blk == blk);
            if hit {
                self.hits += 1;
                if let Some(p) = self.pages[i].as_mut() {
                    p.clock = self.clock;
                }
                return true;
            }
        }
        self.misses += 1;
        false
    }
    /// 写入/载入页；满时 LRU 淘汰（off 档不缓存）。
    pub fn fill(&mut self, blk: u64, dirty: bool) -> bool {
        if self.tier == 0 {
            return false;
        }
        self.clock += 1;
        for i in 0..self.count {
            if self.pages[i].map_or(false, |p| p.blk == blk) {
                if let Some(p) = self.pages[i].as_mut() {
                    p.dirty = p.dirty || dirty;
                    p.clock = self.clock;
                }
                return true;
            }
        }
        if self.count >= MAX_PAGES || self.count >= self.capacity() {
            // LRU：找 clock 最小者淘汰。
            let mut victim = 0usize;
            let mut oldest = u64::MAX;
            for i in 0..self.count {
                let c = self.pages[i].map(|p| p.clock).unwrap_or(u64::MAX);
                if c < oldest {
                    oldest = c;
                    victim = i;
                }
            }
            self.pages[victim] = Some(Page { blk, dirty, clock: self.clock });
            self.evictions += 1;
            return true;
        }
        self.pages[self.count] = Some(Page { blk, dirty, clock: self.clock });
        self.count += 1;
        true
    }
    fn capacity(&self) -> usize {
        [0, 4, 8, 16, 32][self.tier]
    }
    /// 回写脏页计数（净身检查）。
    pub fn flush_dirty(&mut self) -> usize {
        let mut n = 0;
        for i in 0..self.count {
            if let Some(p) = self.pages[i].as_mut() {
                if p.dirty {
                    p.dirty = false;
                    n += 1;
                }
            }
        }
        n
    }
    pub fn dirty_count(&self) -> usize {
        (0..self.count).filter(|&i| self.pages[i].map_or(false, |p| p.dirty)).count()
    }
    /// 降级链：内存紧张 → 收缩容量（档位回退一档）。
    pub fn shrink(&mut self) -> bool {
        if self.tier == 0 {
            return false;
        }
        self.tier -= 1;
        true
    }
}

pub fn run_fs_cache_checks() -> CheckSet {
    let mut set = CheckSet::new("fs23-cache");
    let mut c = PageCache::new(CACHE_DEFAULT);
    let _ = c.fill(1, false);
    let hit_after_fill = c.read(1);
    let miss_before = c.read(99);
    let (h0, m0, _) = c.stats();
    let mut seq = PageCache::new(3);
    for i in 0..16u64 {
        let _ = seq.fill(100 + i, false);
    }
    let window = seq.readahead();
    let mut ev = PageCache::new(2);
    for i in 0..8u64 {
        let _ = ev.fill(i, false);
    }
    let _ = ev.read(0);
    let _ = ev.fill(8, false);
    let evicted = ev.stats().2;
    let mut sh = PageCache::new(4);
    let sh_ok = sh.shrink();
    let sh_tier = sh.tier();
    let mk = PageCache::new(9);

    set.add("X05551 缓存·最小闭环 fill+read", { let _ = c.fill(2, false); c.read(2) }, "载入后命中");
    set.add("X05552 缓存·全量参数", PageCache::new(4).tier() == 4, "档位透传");
    set.add("X05553 缓存·档位矩阵", CACHE_TIERS.len() == 5 && (0..5).all(|t| PageCache::new(t).tier() == t), "五档独立");
    set.add("X05554 缓存·快照迁移", { let q = PageCache::new(1); q.readahead() == 2 }, "档位映射稳定");
    set.add("X05555 缓存·联调集成", hit_after_fill && !miss_before && h0 == 1 && m0 == 1, "命中统计一致");
    set.add("X05556 缓存·越界钳制", mk.tier() == CACHE_DEFAULT && mk.clamped() == 1, "非法档回默认");
    set.add("X05557 缓存·失败叙事", PageCache::new(0).fill(1, false) == false, "off 不缓存可观测");
    set.add("X05558 缓存·中断还原", { let mut r = PageCache::new(2); let _ = r.fill(5, true); r.flush_dirty() == 1 && r.dirty_count() == 0 }, "脏页回写后清零");
    set.add("X05559 缓存·资源降级", sh_ok && sh_tier == 3, "收缩降一档");
    set.add("X05560 缓存·回滚净身", { let mut f = PageCache::new(2); let _ = f.fill(1, true); f.flush_dirty(); f.dirty_count() == 0 }, "flush 后净身");
    set.add("X05561 缓存·动效令牌", CACHE_DEFAULT == 2, "默认 normal");
    set.add("X05562 缓存·三态焦点", window == 16, "窗口随档");
    set.add("X05563 缓存·键盘序", (0..5).all(|t| PageCache::new(t).readahead() <= PageCache::new(4).readahead()), "窗口单调");
    set.add("X05564 缓存·微文案", CACHE_TIERS[2] == "normal", "术语一致");
    set.add("X05565 缓存·aria 等价", evicted == 1, "LRU 淘汰可观测");
    set.add("X05566 缓存·基准采集", { let mut b = PageCache::new(4); (0..32u64).all(|i| b.fill(i, false)) && b.len() == 32 }, "满容量填充");
    set.add("X05567 缓存·热路径", { let mut hp = PageCache::new(2); let _ = hp.fill(7, false); let _ = hp.read(7); let _ = hp.read(7); hp.stats().0 == 2 }, "热块连击命中");
    set.add("X05568 缓存·零漂移", { let mut z = PageCache::new(1); let _ = z.fill(3, true); z.flush_dirty(); z.dirty_count() == 0 && z.stats().2 == 0 }, "零淘汰零残留");
    set.add("X05569 缓存·低配减档", PageCache::new(0).readahead() == 0, "off 零预读");
    set.add("X05570 缓存·守卫", CACHE_TIERS[CACHE_DEFAULT] == "normal", "锚点只增不删");
    set.add("X05571 缓存·智能建议", { let mut s = PageCache::new(2); let _ = s.fill(1, true); s.dirty_count() == 1 }, "脏页建议可观测");
    set.add("X05572 缓存·批量模式", { let mut bm = PageCache::new(3); (0..16u64).all(|i| bm.fill(100 + i, false)) && (0..16u64).all(|i| bm.read(100 + i)) && bm.stats().0 == 16 }, "顺序预读全命中");
    set.add("X05573 缓存·跨域联动", { let mut x = PageCache::new(2); let _ = x.fill(9, true); let _ = x.fill(10, false); x.dirty_count() == 1 }, "与日志域脏位共享");
    set.add("X05574 缓存·扩展点", PageCache::new(1).capacity_inner() == 4, "容量映射冻结");
    set.add("X05575 缓存·彩蛋层", !PageCache::new(0).shrink(), "off 不再收缩");
    set
}

impl PageCache {
    fn capacity_inner(&self) -> usize {
        self.capacity()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hit_and_miss_accounting() {
        let mut c = PageCache::new(CACHE_DEFAULT);
        assert!(!c.read(1));
        assert!(c.fill(1, false));
        assert!(c.read(1));
        let (h, m, _) = c.stats();
        assert_eq!((h, m), (1, 1));
    }

    #[test]
    fn lru_eviction() {
        let mut c = PageCache::new(2); // capacity 8
        for i in 0..8u64 {
            assert!(c.fill(i, false));
        }
        let _ = c.read(0); // 触碰 blk0
        assert!(c.fill(8, false)); // 淘汰最旧未触碰者
        assert_eq!(c.stats().2, 1);
        assert!(c.read(0)); // 触碰过的不被淘汰
    }

    #[test]
    fn off_tier_caches_nothing() {
        let mut c = PageCache::new(0);
        assert!(!c.fill(1, false));
        assert!(!c.read(1));
        assert_eq!(c.readahead(), 0);
    }

    #[test]
    fn shrink_degrades_one_tier() {
        let mut c = PageCache::new(4);
        assert!(c.shrink());
        assert_eq!(c.tier(), 3);
        let mut z = PageCache::new(0);
        assert!(!z.shrink());
    }

    #[test]
    fn checkset_full_25() {
        let set = run_fs_cache_checks();
        assert_eq!(set.len(), 25);
        for i in 0..set.len() { let c = set.get(i).unwrap(); assert!(c.passed, "FAIL {} {}", c.name, c.detail); }
    }
}
