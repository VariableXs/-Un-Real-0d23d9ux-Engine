//! 阶段 6 · S3.8 ramcache（三体 AI-2）——引擎热数据宿主内存缓存。
//!
//! 职责（施工总案 6.8 / 三体分工图 S3.8）：
//!   SHARED/WIN_ENGINE 热数据 LRU 缓存进宿主 RAM。
//!
//! 硬语义（拔盘无痕红线）：
//!   - **只缓不落盘**：本模块对文件系统零写入（除调用方提供的 loader 读盘外，
//!     模块自身不做任何 I/O）——"关机后 U 盘字节级零残留"由构造保证：
//!     数据只存在于进程内存，进程结束即物理消失，没有任何可残留的落盘点。
//!   - **关机即清**：进程退出 = 内存归还；引擎会话停止/拔盘时布线层显式
//!     `clear()`（双保险）。
//!   - **缓存一致性**：以 (mtime_ms, size) 元组为有效性凭据——盘上文件被
//!     另一系统改写后凭据失配 → 自动失效并重新读盘（总案"盘上文件被另一
//!     系统改后失效"）。元数据由调用方在读盘时提供，缓存本体不做 I/O。
//!   - **内存预算**：缓存尺寸随性能档位联动（`set_budget`，布线层按 perf
//!     档下发）；超预算按 LRU 逐出；单条超预算整条旁路（不缓存，防挤兑）。
//!   - **独立分配**：数据存自有 `Vec<u8>`（堆分配），不碰系统文件缓存语义。
//!   - **命中率公示**：hits/misses/evictions/bytes 统计经命令可查（完善性）。
//!
//! 实现注记：LRU 逐出用"最近使用时钟 + O(n) 扫描"——缓存条目数预期为
//! 百级（热文件集合），O(n) 扫描换实现简洁可审计；条目量级膨胀时再换
//! 双向链表（开放性：接口不变）。
//!
//! 零 unwrap：生产路径全部显式处理。

use serde::Serialize;
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::OnceLock;

/// 默认内存预算：256 MiB（性能档位联动的初值；布线层可按档位覆盖）。
pub const DEFAULT_BUDGET_BYTES: usize = 256 * 1024 * 1024;

/// 单条上限 = 预算的 1/4（防单文件挤兑热集合）。
const MAX_ENTRY_RATIO: usize = 4;

/// 缓存条目（数据 + 一致性凭据 + LRU 时钟）。
struct Entry {
    data: Vec<u8>,
    mtime_ms: u64,
    #[allow(dead_code)]
    size: u64,
    last_used: u64,
}

/// 命中率公示快照（EngineTab/诊断可消费）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    pub entries: u64,
    pub bytes: u64,
    pub budget_bytes: usize,
    /// 命中率 0.0-1.0（零查询时恒 0，不虚构）。
    pub hit_rate: f64,
}

/// 引擎热数据 LRU 缓存（纯内存；I/O 由调用方 loader 承担）。
pub struct RamCache {
    max_bytes: usize,
    entries: HashMap<String, Entry>,
    bytes: u64,
    clock: u64,
    hits: u64,
    misses: u64,
    evictions: u64,
}

impl RamCache {
    pub fn new(max_bytes: usize) -> Self {
        RamCache {
            max_bytes: max_bytes.max(1),
            entries: HashMap::new(),
            bytes: 0,
            clock: 0,
            hits: 0,
            misses: 0,
            evictions: 0,
        }
    }

    fn max_entry_bytes(&self) -> usize {
        (self.max_bytes / MAX_ENTRY_RATIO).max(1)
    }

    /// 读缓存：凭据 (mtime_ms, size) 失配 = 自动失效并记 miss（一致性语义）。
    /// 命中返回数据引用并刷新 LRU 时钟。
    pub fn get(&mut self, path: &str, mtime_ms: u64, size: u64) -> Option<&[u8]> {
        self.clock += 1;
        // 1) 一致性判定：凭据失配（盘上文件被另一系统改写）→ 先失效再 miss。
        let stale = matches!(
            self.entries.get(path),
            Some(e) if e.mtime_ms != mtime_ms || e.size != size
        );
        if stale {
            self.entries.remove(path);
            self.misses += 1;
            return None;
        }
        // 2) 命中路径：刷新 LRU 时钟并返回数据。
        let clock = self.clock;
        match self.entries.get_mut(path) {
            Some(e) => {
                e.last_used = clock;
                self.hits += 1;
                Some(e.data.as_slice())
            }
            None => {
                self.misses += 1;
                None
            }
        }
    }

    /// 写缓存：超预算整条旁路（不缓存不挤兑）；超出预算按 LRU 逐出。
    /// 同 path 已有旧条目先移除（凭据刷新语义）。
    pub fn put(&mut self, path: &str, data: Vec<u8>, mtime_ms: u64) {
        let len = data.len();
        if len > self.max_entry_bytes() {
            // 单条超限：旁路（记 miss 已在 get 侧；此处只保证不挤兑）。
            self.entries.remove(path);
            return;
        }
        if let Some(old) = self.entries.remove(path) {
            self.bytes -= old.data.len() as u64;
        }
        // 逐出到能放下为止。
        while self.bytes as usize + len > self.max_bytes {
            if !self.evict_one() {
                break; // 空表防御（理论上 len <= max_entry 不会走到）
            }
        }
        self.clock += 1;
        self.entries.insert(
            path.to_string(),
            Entry {
                data,
                mtime_ms,
                size: len as u64,
                last_used: self.clock,
            },
        );
        self.bytes += len as u64;
    }

    /// LRU 逐出一条；返回是否逐出（空表 = false）。
    fn evict_one(&mut self) -> bool {
        let victim = self
            .entries
            .iter()
            .min_by_key(|(_, e)| e.last_used)
            .map(|(k, _)| k.clone());
        match victim {
            Some(k) => {
                if let Some(e) = self.entries.remove(&k) {
                    self.bytes -= e.data.len() as u64;
                }
                self.evictions += 1;
                true
            }
            None => false,
        }
    }

    /// 显式失效单条（另一系统改写通知路径用）。
    pub fn invalidate(&mut self, path: &str) {
        if let Some(e) = self.entries.remove(path) {
            self.bytes -= e.data.len() as u64;
        }
    }

    /// 全清（关机/引擎停止/拔盘联动；"关机即清"的显式双保险路径）。
    pub fn clear(&mut self) {
        self.entries.clear();
        self.bytes = 0;
    }

    /// 内存预算调整（性能档位联动）：调小立即按 LRU 逐出至合规。
    pub fn set_budget(&mut self, max_bytes: usize) {
        self.max_bytes = max_bytes.max(1);
        while self.bytes as usize > self.max_bytes {
            if !self.evict_one() {
                break;
            }
        }
    }

    pub fn budget(&self) -> usize {
        self.max_bytes
    }

    /// 统计快照（命中率公示；零查询恒 0 不虚构）。
    pub fn stats(&self) -> CacheStats {
        let queries = self.hits + self.misses;
        CacheStats {
            hits: self.hits,
            misses: self.misses,
            evictions: self.evictions,
            entries: self.entries.len() as u64,
            bytes: self.bytes,
            budget_bytes: self.max_bytes,
            hit_rate: if queries == 0 {
                0.0
            } else {
                self.hits as f64 / queries as f64
            },
        }
    }
}

// ---------------------------------------------------------------- 全局实例与布线

static CACHE: OnceLock<Mutex<RamCache>> = OnceLock::new();

/// 全局缓存实例（布线层用；进程生命周期即缓存生命周期）。
pub fn global() -> &'static Mutex<RamCache> {
    CACHE.get_or_init(|| Mutex::new(RamCache::new(DEFAULT_BUDGET_BYTES)))
}

/// 会话收束清空（引擎停止/拔盘联动调用；失败静默——缓存清不掉也只是多占内存，
/// 绝不在收束路径上制造新故障）。
pub fn global_clear() {
    if let Ok(mut c) = global().lock() {
        c.clear();
    }
}

// ---------------------------------------------------------------- 命令层

/// 命中率与占用公示（完善性验收：命中率统计与公示）。
#[tauri::command(async)]
pub fn ramcache_stats() -> CacheStats {
    match global().lock() {
        Ok(c) => c.stats(),
        Err(_) => CacheStats {
            hits: 0,
            misses: 0,
            evictions: 0,
            entries: 0,
            bytes: 0,
            budget_bytes: 0,
            hit_rate: 0.0,
        },
    }
}

/// 手动全清（设置页/诊断；关机清空的显式入口）。
#[tauri::command(async)]
pub fn ramcache_clear() -> CacheStats {
    match global().lock() {
        Ok(mut c) => {
            c.clear();
            c.stats()
        }
        Err(_) => CacheStats {
            hits: 0,
            misses: 0,
            evictions: 0,
            entries: 0,
            bytes: 0,
            budget_bytes: 0,
            hit_rate: 0.0,
        },
    }
}

// ---------------------------------------------------------------- 测试（纯逻辑全覆盖）

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn put_get_roundtrip_and_hit_miss_counts() {
        let mut c = RamCache::new(1024);
        assert!(c.get("p1", 100, 3).is_none(), "空缓存必 miss");
        c.put("p1", vec![1, 2, 3], 100);
        let hit = c.get("p1", 100, 3).unwrap();
        assert_eq!(hit, &[1, 2, 3]);
        let s = c.stats();
        assert_eq!((s.hits, s.misses) , (1, 1));
        assert!((s.hit_rate - 0.5).abs() < 1e-9);
    }

    #[test]
    fn lru_eviction_order() {
        // 预算 200，单条上限 = 200/4 = 50 → 用 50B 条目（合规尺寸）。
        let mut c = RamCache::new(200);
        c.put("a", vec![0u8; 50], 1);
        c.put("b", vec![0u8; 50], 1);
        c.put("c", vec![0u8; 50], 1);
        // 触碰 a（a 变最新，b 成最旧）。
        let _ = c.get("a", 1, 50);
        c.put("d", vec![0u8; 50], 1); // 200 = 预算，恰好放下
        c.put("e", vec![0u8; 50], 1); // 250 > 200 → 逐出最旧的 b
        assert!(c.get("b", 1, 50).is_none(), "最久未用的 b 应被逐出");
        assert!(c.get("a", 1, 50).is_some(), "刚触碰过的 a 必须存活");
        assert!(c.get("c", 1, 50).is_some());
        assert!(c.get("d", 1, 50).is_some());
        assert!(c.get("e", 1, 50).is_some());
        assert_eq!(c.stats().evictions, 1);
    }

    #[test]
    fn consistency_invalidate_on_mtime_change() {
        let mut c = RamCache::new(1024);
        c.put("p", vec![1, 2, 3], 100);
        // 另一系统改写盘上文件：mtime 变了 → 凭据失配自动失效。
        assert!(c.get("p", 200, 3).is_none());
        // 失效后重新读盘写入新版本。
        c.put("p", vec![9, 9, 9], 200);
        assert_eq!(c.get("p", 200, 3).unwrap(), &[9, 9, 9]);
    }

    #[test]
    fn consistency_invalidate_on_size_change() {
        let mut c = RamCache::new(1024);
        c.put("p", vec![1, 2, 3], 100);
        assert!(c.get("p", 100, 4).is_none(), "size 变化也必须失效");
    }

    #[test]
    fn oversized_entry_bypasses_cache() {
        let mut c = RamCache::new(100);
        c.put("big", vec![0u8; 500], 1); // 单条 > 预算/4 → 旁路
        assert!(c.get("big", 1, 500).is_none());
        assert_eq!(c.stats().entries, 0, "旁路不得占位");
        assert_eq!(c.stats().bytes, 0);
    }

    #[test]
    fn clear_empties_everything_but_keeps_budget() {
        let mut c = RamCache::new(1024);
        c.put("a", vec![0u8; 100], 1);
        c.put("b", vec![0u8; 100], 1);
        c.clear();
        let s = c.stats();
        assert_eq!((s.entries, s.bytes), (0, 0));
        assert_eq!(s.budget_bytes, 1024, "清空不清预算");
        assert!(c.get("a", 1, 100).is_none());
    }

    #[test]
    fn set_budget_shrinks_and_evicts() {
        let mut c = RamCache::new(1024);
        c.put("a", vec![0u8; 200], 1);
        c.put("b", vec![0u8; 200], 1);
        c.put("c", vec![0u8; 200], 1);
        c.set_budget(500); // 600B > 500B → 逐出最旧
        assert!(c.bytes as usize <= 500);
        assert!(c.get("a", 1, 200).is_none(), "最旧的 a 被逐出");
        assert_eq!(c.budget(), 500);
    }

    #[test]
    fn invalidate_removes_single_entry_only() {
        let mut c = RamCache::new(1024);
        c.put("a", vec![1], 1);
        c.put("b", vec![2], 1);
        c.invalidate("a");
        assert!(c.get("a", 1, 1).is_none());
        assert!(c.get("b", 1, 1).is_some());
    }

    #[test]
    fn hit_rate_zero_division_safe() {
        let c = RamCache::new(1);
        assert_eq!(c.stats().hit_rate, 0.0, "零查询恒 0，不虚构");
    }

    #[test]
    fn zero_budget_clamped_to_one() {
        let mut c = RamCache::new(0);
        c.put("a", vec![1], 1); // max_entry = max(1/4,1)=1 → len 1 可入
        let s = c.stats();
        assert!(s.entries <= 1);
    }
}
