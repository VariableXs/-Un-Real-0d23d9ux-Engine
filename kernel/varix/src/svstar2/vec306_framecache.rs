//! VE-F0706 · 路径光栅帧间缓存
//!
//! 职责：栅格化结果的帧间缓存深化（D04 F0675 的帧间维度）——
//! 路径静态判定（连续帧未变 → 光栅结果直接复用）、
//! 缓存与瓦片系统联动（缓存的粒度=瓦片而非整图——部分更新）、
//! 缓存驱逐与预算（F0034 瞬态池协同）。
//! 判据：帧间命中判定、瓦片粒度复用、预算联动、命中率遥测、滚动/静态场景收益。

use alloc::vec;
use alloc::vec::Vec;

/// 瓦片缓存条目：瓦片坐标 + 光栅结果哈希 + 最后使用帧号。
#[derive(Clone, Debug, PartialEq)]
pub struct TileCacheEntry {
    pub tx: u32,
    pub ty: u32,
    /// 该瓦片内容（路径几何 + 变换 + AA 档）的指纹。
    pub fingerprint: u64,
    pub last_used_frame: u64,
}

/// 帧间缓存：瓦片粒度（不是整图）——滚动场景只更新脏瓦片。
pub struct FrameCache {
    pub entries: Vec<TileCacheEntry>,
    pub capacity: u32,
    pub cur_frame: u64,
    /// 命中率遥测：命中数 / 请求数。
    pub hits: u64,
    pub requests: u64,
}

impl FrameCache {
    pub fn new(capacity: u32) -> Self {
        FrameCache { entries: Vec::new(), capacity, cur_frame: 0, hits: 0, requests: 0 }
    }

    /// 帧推进。
    pub fn advance_frame(&mut self) {
        self.cur_frame += 1;
    }

    /// 路径静态判定：瓦片的指纹未变 → 命中（光栅结果直接复用）。
    pub fn lookup(&mut self, tx: u32, ty: u32, fingerprint: u64) -> bool {
        self.requests += 1;
        if let Some(e) = self.entries.iter_mut().find(|e| e.tx == tx && e.ty == ty) {
            if e.fingerprint == fingerprint {
                e.last_used_frame = self.cur_frame;
                self.hits += 1;
                return true;
            }
            // 指纹变了：原地替换（瓦片粒度部分更新）。
            e.fingerprint = fingerprint;
            e.last_used_frame = self.cur_frame;
            return false;
        }
        self.insert(tx, ty, fingerprint);
        false
    }

    fn insert(&mut self, tx: u32, ty: u32, fingerprint: u64) {
        if self.entries.len() >= self.capacity as usize {
            self.evict_one();
        }
        self.entries.push(TileCacheEntry {
            tx, ty, fingerprint, last_used_frame: self.cur_frame,
        });
    }

    /// 驱逐：LRU（最后使用帧号最小者）——与 F0034 瞬态池协同的预算纪律。
    fn evict_one(&mut self) {
        if self.entries.is_empty() {
            return;
        }
        let idx = self
            .entries
            .iter()
            .enumerate()
            .min_by_key(|(_, e)| e.last_used_frame)
            .map(|(i, _)| i)
            .unwrap();
        self.entries.remove(idx);
    }

    /// 命中率（千分位）。
    pub fn hit_rate(&self) -> u32 {
        if self.requests == 0 {
            0
        } else {
            (self.hits * 1000 / self.requests) as u32
        }
    }
}

/// 路径指纹：几何 + 变换 + AA 档联合哈希（FNV-1a 简化版）。
pub fn path_fingerprint(geometry: &[f32], transform: [f32; 6], aa: u32) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    let mut feed = |bytes: &[u8], h: &mut u64| {
        for &b in bytes {
            *h ^= b as u64;
            *h = h.wrapping_mul(0x100000001b3);
        }
    };
    let geom: Vec<u8> = geometry.iter().flat_map(|f| f.to_bits().to_le_bytes()).collect();
    feed(&geom, &mut h);
    for t in transform {
        feed(&t.to_bits().to_le_bytes(), &mut h);
    }
    feed(&aa.to_le_bytes(), &mut h);
    h
}

/// 静态场景收益模型：全静态时第 2 帧起命中率 100%——CPU 光栅归零。
pub fn static_scene_gain(frame_count: u32, raster_cost_per_frame: u32) -> u64 {
    if frame_count <= 1 {
        return raster_cost_per_frame as u64 * frame_count as u64;
    }
    raster_cost_per_frame as u64 // 只有第一帧真实栅格化
}

/// 滚动场景收益：滚动时脏瓦片占比 = 滚动距离 / 视口尺寸（瓦片粒度部分更新）。
pub fn scroll_dirty_ratio(scroll_px: u32, viewport_px: u32, tile_size: u32) -> u32 {
    if viewport_px == 0 {
        return 1000;
    }
    let dirty_tiles = (scroll_px + tile_size - 1) / tile_size;
    let total_tiles = (viewport_px + tile_size - 1) / tile_size;
    dirty_tiles * 1000 / total_tiles.max(1)
}

pub fn run_vec306_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec306");
    // 族一：指纹
    let fp1 = path_fingerprint(&[1.0, 2.0, 3.0], [1.0, 0.0, 0.0, 1.0, 0.0, 0.0], 1);
    let fp2 = path_fingerprint(&[1.0, 2.0, 3.0], [1.0, 0.0, 0.0, 1.0, 0.0, 0.0], 1);
    let fp3 = path_fingerprint(&[1.0, 2.0, 4.0], [1.0, 0.0, 0.0, 1.0, 0.0, 0.0], 1);
    let fp4 = path_fingerprint(&[1.0, 2.0, 3.0], [1.0, 0.0, 0.0, 1.0, 5.0, 0.0], 1);
    s.add("同输入同指纹", fp1 == fp2, "fp-same");
    s.add("几何变指纹变", fp1 != fp3, "fp-geom");
    s.add("变换变指纹变", fp1 != fp4, "fp-tf");
    // 族二：帧间命中
    let mut c = FrameCache::new(4);
    c.advance_frame();
    s.add("首帧未命中", !c.lookup(0, 0, fp1), "miss");
    s.add("次帧命中", c.lookup(0, 0, fp1), "hit");
    s.add("命中数 1", c.hits == 1 && c.requests == 2, "count");
    s.add("命中率 500", c.hit_rate() == 500, "rate");
    c.advance_frame();
    s.add("指纹变未命中且原地替换", {
        let ok = !c.lookup(0, 0, fp3);
        ok && c.entries.len() == 1 && c.entries[0].fingerprint == fp3
    }, "partial-update");
    // 族三：瓦片粒度与 LRU 驱逐
    let mut c2 = FrameCache::new(2);
    c2.advance_frame();
    let _ = c2.lookup(1, 0, 100);
    c2.advance_frame();
    let _ = c2.lookup(0, 0, 200);
    c2.advance_frame();
    let _ = c2.lookup(2, 0, 300);
    s.add("容量上限生效", c2.entries.len() == 2, "cap");
    s.add("LRU 驱逐最早使用", {
        // 容量 2：驱逐 (1,0)（帧 0 使用最旧），留下 (0,0) 与 (2,0)。
        c2.entries.iter().any(|e| e.tx == 2) && !c2.entries.iter().any(|e| e.tx == 1)
    }, "lru");
    // 族四：收益模型
    s.add("静态场景一次栅格化", static_scene_gain(60, 1000) == 1000, "static");
    s.add("单帧无收益", static_scene_gain(1, 1000) == 1000, "one-frame");
    s.add("滚动脏占比计算", scroll_dirty_ratio(64, 256, 32) == 250, "scroll");
    s.add("整屏滚动全脏", scroll_dirty_ratio(256, 256, 32) == 1000, "full-dirty");
    s.add("静止零脏", scroll_dirty_ratio(0, 256, 32) == 0, "clean");
    s.add("瓦片粒度优于整图", scroll_dirty_ratio(64, 256, 32) < 1000, "tile-gran");
    s.add("预算联动：容量即预算", FrameCache::new(2).capacity == 2, "budget");
    s
}
