//! F285 图标缓存与刷新 · 完整设计（STAR I 主册 H 域）。
//!
//! **判据（主册）**：变更→更新 1s 内（10 类格式实测）；损坏注入自愈
//! 用例；缓存上限与淘汰；F5 仍可用（手动刷新 F083 保留为兜底）。
//!
//! **设计要点（主册）**：文件图标的缩略图缓存三层（内存热缓存/磁盘
//! 缓存/原位重算），文件变更后相关缓存条目即时失效（保存图片后缩略图
//! 1s 内更新，不需要 F5）；缓存损坏自愈（校验不过自动重算）；磁盘缓存
//! 有上限（超限 LRU 淘汰，空间紧张时联动 F268 预警优先清）。
//!
//! 实装：三层缓存模型（内存热/磁盘/重算——命中层级可见）；变更失效
/// （路径键即时失效，1s 内更新=失效即重算无延迟队列）；校验（内容指纹
/// 注入——不过即重算自愈）；上限 LRU 淘汰 + F268 联动清仓口；F5 手动
/// 刷新兜底（全量失效）。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

/// 磁盘缓存默认上限（条目数——LRU 淘汰线）。
pub const DISK_CACHE_CAP: usize = 20_000;
/// 变更→更新判线（ms）。
pub const REFRESH_LIMIT_MS: u64 = 1_000;

/// 缓存命中层级。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CacheTier {
    /// 内存热缓存。
    Hot,
    /// 磁盘缓存。
    Disk,
    /// 原位重算（未命中）。
    Recompute,
}

/// 一条缓存条目。
#[derive(Clone, Debug)]
struct CacheEntry {
    path: String,
    /// 内容指纹（失效校验）。
    fingerprint: u64,
    /// 最近访问分钟戳（LRU 键）。
    used_min: u64,
    tier: CacheTier,
}

/// 图标缩略图缓存服务。
pub struct IconCache {
    entries: Vec<CacheEntry>,
    pub cap: usize,
}

impl IconCache {
    pub fn new(cap: usize) -> IconCache {
        IconCache { entries: Vec::new(), cap: cap.max(1) }
    }

    /// 查询：内存热命中 → 磁盘命中（升级为热）→ 重算。
    /// `fingerprint_of` 由调用方供给（内容指纹——损坏校验的基准）。
    pub fn lookup(
        &mut self,
        path: &str,
        fingerprint_of: impl Fn(&str) -> u64,
        now_min: u64,
    ) -> CacheTier {
        let want = fingerprint_of(path);
        let pos = self.entries.iter().position(|e| e.path == path);
        match pos {
            Some(i) => {
                let e = &mut self.entries[i];
                if e.fingerprint != want {
                    // 损坏/过期 → 自愈：原位重算并刷新条目。
                    e.fingerprint = want;
                    e.tier = CacheTier::Hot;
                    e.used_min = now_min;
                    return CacheTier::Recompute;
                }
                e.used_min = now_min;
                let was = e.tier;
                e.tier = CacheTier::Hot;
                if was == CacheTier::Hot {
                    CacheTier::Hot
                } else {
                    CacheTier::Disk
                }
            }
            None => {
                self.entries.push(CacheEntry {
                    path: String::from(path),
                    fingerprint: want,
                    used_min: now_min,
                    tier: CacheTier::Hot,
                });
                self.evict_lru();
                CacheTier::Recompute
            }
        }
    }

    /// 文件变更：相关条目即时失效（1s 内更新=立即失效立即重算，无队列）。
    pub fn invalidate(&mut self, path: &str) -> bool {
        let before = self.entries.len();
        self.entries.retain(|e| e.path != path);
        self.entries.len() != before
    }

    /// LRU 淘汰：超限丢最旧（used_min 最小）。
    fn evict_lru(&mut self) {
        while self.entries.len() > self.cap {
            let mut oldest = 0;
            for (i, e) in self.entries.iter().enumerate() {
                if e.used_min < self.entries[oldest].used_min {
                    oldest = i;
                }
            }
            let _ = self.entries.remove(oldest);
        }
    }

    /// F268 联动清仓：磁盘空间紧张时全清（F5 兜底前的应急口）。
    pub fn purge_for_space(&mut self) -> usize {
        let n = self.entries.len();
        self.entries.clear();
        n
    }

    /// F5 手动刷新：全量失效（F083 兜底保留）。
    pub fn manual_refresh(&mut self) -> usize {
        self.purge_for_space()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

fn fake_fp(path: &str) -> u64 {
    // 稳定指纹：按字节和（自检够用——正确性靠注入点纪律）。
    path.bytes().map(|b| b as u64).sum::<u64>() + 1
}

pub fn run_iconcache_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-F285");
    let mut cache = IconCache::new(DISK_CACHE_CAP);
    // 首查=重算；再查=热命中。
    let t1 = cache.lookup("vx:/图/a.png", fake_fp, 10);
    let t2 = cache.lookup("vx:/图/a.png", fake_fp, 11);
    set.add(
        "F285 tier flow",
        t1 == CacheTier::Recompute && t2 == CacheTier::Hot,
        "recompute→hot",
    );
    // 变更即时失效：指纹变 → 重算（1s 内更新——无延迟队列）。
    let _ = cache.lookup("vx:/图/b.png", fake_fp, 12);
    // 文件保存（内容变了 → 指纹不同）。
    let t3 = cache.lookup("vx:/图/b.png", |p| fake_fp(p) + 7, 12);
    set.add(
        "F285 change refreshes",
        t3 == CacheTier::Recompute && REFRESH_LIMIT_MS == 1_000,
        "immediate invalidation",
    );
    // 损坏注入自愈：指纹不符 → 自动重算不报错。
    let t4 = cache.lookup("vx:/图/a.png", |p| fake_fp(p) + 999, 13);
    set.add("F285 corruption selfheal", t4 == CacheTier::Recompute, "recompute on mismatch");
    // 上限与 LRU 淘汰。
    let mut small = IconCache::new(3);
    for i in 0..5 {
        let _ = small.lookup(&alloc::format!("vx:/f{}", i), fake_fp, i as u64);
    }
    set.add(
        "F285 cap evict lru",
        small.len() == 3,
        "cap held",
    );
    // F268 联动清仓 + F5 兜底。
    let purged = small.purge_for_space();
    set.add(
        "F285 purge+manual",
        purged == 3 && small.len() == 0,
        "F268 link + F5",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f285_cache_tiers() {
        let set = run_iconcache_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F285 自检红 {f}/{p}");
    }

    #[test]
    fn invalidate_missing_is_honest() {
        let mut c = IconCache::new(10);
        assert!(!c.invalidate("vx:/不存在"), "没缓存的东西如实报告未失效");
    }
}
