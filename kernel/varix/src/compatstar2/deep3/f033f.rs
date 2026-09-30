//! F033 深化批次四 · 构建缓存面（compatstar2/deep3 · G-A-33）。
//!
//! 批次一/二/三已覆盖 F033 的工具链判例面、执行治理面与构建图面；本批补
//! 主册【功能定义】「全语义对齐」的序列化/账本/容错面：内容寻址缓存
//! （FNV-1a 32 位哈希 → 产物槽定长 32，线性探测冲突计数）、命中/未命中/
//! 写入三账与命中率 permille、工具链版本指纹失效（指纹变更 → 全缓存失效
//! 标记，逐条记因）、缓存字节预算（超预算 LRU 逐出并记账，热条目按命中
//! 计数保护——全热时退化为最旧逐出，零死锁）。
//!
//! 判据对账：深化以主册【设计细节】「增量构建判据：二次构建仅重编变更
//! 文件（时间戳比对实测）」未落地面为源，一处一事实（FNV-1a 参考参数
//! offset=2166136261 / prime=16777619 对拍、ccache 命中率统计语义对拍）。
//!
//! 零堆纪律：定长槽表/逐出账/记因表，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量与哈希（FNV-1a 32 位参考参数）
// ---------------------------------------------------------------------------

/// FNV-1a 偏移基数（参考向量对拍锚点）。
pub const FNV_OFFSET: u32 = 2166136261;
/// FNV-1a 素数。
pub const FNV_PRIME: u32 = 16777619;
/// 产物槽容量（定长 32）。
pub const SLOTS: usize = 32;
/// 热条目命中阈值（命中计数达到即免逐出）。
pub const HOT_HITS: u32 = 3;
/// 逐出账容量。
pub const EVICT_LOG_N: usize = 16;
/// 失效记因表容量。
pub const INVALID_LOG_N: usize = 8;

/// FNV-1a 32 位（逐字节 xor-then-mul，参考测试向量见自检）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h = FNV_OFFSET;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

/// 一个缓存条目：内容哈希 + 命中计数 + 字节量 + LRU 时间戳 + 记因名。
#[derive(Clone, Copy)]
pub struct CacheEntry {
    pub hash: u32,
    pub hits: u32,
    pub bytes: u32,
    pub stamp: u64,
    pub name: &'static str,
}

// ---------------------------------------------------------------------------
// 构建缓存（内容寻址 + 三账 + 预算逐出 + 指纹失效）
// ---------------------------------------------------------------------------

/// 构建缓存：miss→write→hit 全账面；线性探测冲突显性计数。
pub struct BuildCache {
    entries: [Option<CacheEntry>; SLOTS],
    /// 命中计数。
    pub hits: u32,
    /// 未命中计数。
    pub misses: u32,
    /// 写入计数。
    pub writes: u32,
    /// 线性探测冲突计数（探过的非目标槽）。
    pub probes: u32,
    /// 当前占用字节。
    pub used_bytes: u64,
    /// 字节预算。
    pub budget_bytes: u64,
    /// 逐出计数。
    pub evictions: u32,
    evict_log: [&'static str; EVICT_LOG_N],
    pub evict_n: usize,
    tick: u64,
    fingerprint: u64,
    /// 失效标记（失效期间拒绝读写）。
    pub invalidated: bool,
    /// 失效记因表。
    invalid_reasons: [&'static str; INVALID_LOG_N],
    pub invalid_n: usize,
    /// 最近一次失效冲刷的条目数。
    pub invalidated_entries: u32,
}

/// 冷度比较：命中少者优先，平手取最旧。
fn colder(a: CacheEntry, b: CacheEntry) -> bool { a.hits < b.hits || (a.hits == b.hits && a.stamp < b.stamp) }

impl BuildCache {
    pub const fn new() -> Self {
        BuildCache {
            entries: [None; SLOTS], hits: 0, misses: 0, writes: 0, probes: 0,
            used_bytes: 0, budget_bytes: 4096, evictions: 0,
            evict_log: [""; EVICT_LOG_N], evict_n: 0, tick: 0, fingerprint: 0,
            invalidated: false, invalid_reasons: [""; INVALID_LOG_N],
            invalid_n: 0, invalidated_entries: 0,
        }
    }
    /// 查找：哈希命中 → 记 hit 并触摸；空槽 → 记 miss 显性 Err；
    /// 冲突槽逐个探测计数。
    pub fn lookup(&mut self, key: &[u8]) -> Result<usize, &'static str> {
        if self.invalidated {
            return Err("cache-invalidated");
        }
        let h = fnv1a(key);
        let mut slot = (h as usize) % SLOTS;
        for _ in 0..SLOTS {
            match self.entries[slot] {
                Some(e) if e.hash == h => {
                    self.hits += 1;
                    self.tick += 1;
                    self.entries[slot] = Some(CacheEntry { hits: e.hits + 1, stamp: self.tick, ..e });
                    return Ok(slot);
                }
                Some(_) => {
                    self.probes += 1;
                    slot = (slot + 1) % SLOTS;
                }
                None => {
                    self.misses += 1;
                    return Err("cache-miss");
                }
            }
        }
        self.misses += 1;
        Err("cache-miss")
    }
    /// 写入：同哈希原位更新；否则预算内腾位（逐出最冷）后插入；
    /// 超预算单项/表满显性 Err。
    pub fn write(&mut self, key: &[u8], bytes: u32, name: &'static str) -> Result<usize, &'static str> {
        if self.invalidated {
            return Err("cache-invalidated");
        }
        if bytes as u64 > self.budget_bytes {
            return Err("budget-exceeded");
        }
        let h = fnv1a(key);
        let mut slot = (h as usize) % SLOTS;
        for _ in 0..SLOTS {
            match self.entries[slot] {
                Some(e) if e.hash == h => {
                    self.used_bytes = self.used_bytes + bytes as u64 - e.bytes as u64;
                    self.entries[slot] = Some(CacheEntry { hash: h, hits: e.hits, bytes, stamp: self.tick, name });
                    self.tick += 1;
                    self.writes += 1;
                    return Ok(slot);
                }
                Some(_) => {
                    self.probes += 1;
                    slot = (slot + 1) % SLOTS;
                }
                None => break,
            }
        }
        while self.used_bytes + bytes as u64 > self.budget_bytes {
            if !self.evict_coldest() {
                return Err("budget-exceeded");
            }
        }
        let mut free = usize::MAX;
        let mut s = (h as usize) % SLOTS;
        for _ in 0..SLOTS {
            if self.entries[s].is_none() {
                free = s;
                break;
            }
            self.probes += 1;
            s = (s + 1) % SLOTS;
        }
        if free == usize::MAX {
            return Err("cache-full");
        }
        self.entries[free] = Some(CacheEntry { hash: h, hits: 0, bytes, stamp: self.tick, name });
        self.tick += 1;
        self.used_bytes += bytes as u64;
        self.writes += 1;
        Ok(free)
    }
    /// 逐出最冷：优先 hits < HOT_HITS 中（hits, stamp）最小者；全热退化
    /// 为整体最旧（零死锁）。返回是否发生逐出。
    fn evict_coldest(&mut self) -> bool {
        let mut victim = usize::MAX;
        let mut any = usize::MAX;
        for i in 0..SLOTS {
            if let Some(e) = self.entries[i] {
                if e.hits < HOT_HITS && (victim == usize::MAX || colder(e, self.entries[victim].unwrap())) {
                    victim = i;
                }
                if any == usize::MAX || colder(e, self.entries[any].unwrap()) {
                    any = i;
                }
            }
        }
        let v = if victim != usize::MAX { victim } else { any };
        if v == usize::MAX { return false; }
        let e = self.entries[v].unwrap();
        self.entries[v] = None;
        self.used_bytes -= e.bytes as u64;
        self.evictions += 1;
        if self.evict_n < EVICT_LOG_N {
            self.evict_log[self.evict_n] = e.name;
            self.evict_n += 1;
        }
        true
    }
    /// 命中率 permille（无访问时为 0）。
    pub fn hit_rate_permille(&self) -> u32 {
        let total = self.hits + self.misses;
        if total == 0 { return 0; }
        self.hits * 1000 / total
    }
    /// 工具链版本指纹登记：变更 → 全缓存失效标记 + 逐条冲刷记因；
    /// 同值 → 原样（无操作）。
    pub fn set_fingerprint(&mut self, fp: u64) {
        if fp == self.fingerprint {
            return;
        }
        let mut flushed = 0u32;
        for i in 0..SLOTS {
            if self.entries[i].is_some() { flushed += 1; }
            self.entries[i] = None;
        }
        self.used_bytes = 0;
        self.invalidated = true;
        self.invalidated_entries = flushed;
        if self.invalid_n < INVALID_LOG_N { self.invalid_reasons[self.invalid_n] = "toolchain-fingerprint-changed"; self.invalid_n += 1; }
        self.fingerprint = fp;
    }
    /// 失效标记解除（重新预热入口）。
    pub fn resume(&mut self) { self.invalidated = false; }
    /// 记因表视图。
    pub fn invalid_reasons(&self) -> &[&'static str] { &self.invalid_reasons[..self.invalid_n] }
    /// 逐出账视图。
    pub fn evict_log(&self) -> &[&'static str] { &self.evict_log[..self.evict_n] }
}

/// 域自检（深化批次四）。
pub fn run_f033f_checks() -> CheckSet {
    let mut cs = CheckSet::new("F033-build-cache-d4");
    // 1) FNV-1a 32 位参考向量（"" / "a" / "foobar"）。
    cs.add("fnv_reference_vectors", fnv1a(b"") == 2166136261 && fnv1a(b"a") == 0xE40C_292C && fnv1a(b"foobar") == 0xBF9C_F968, "");
    // 2) miss → write → hit 三账各一，命中率 500‰。
    let mut c = BuildCache::new();
    let miss = c.lookup(b"obj/main.o");
    let wslot = c.write(b"obj/main.o", 64, "main.o");
    let hit = c.lookup(b"obj/main.o");
    cs.add("miss_write_hit_ledgers", miss == Err("cache-miss") && wslot.is_ok() && hit.is_ok() && c.hits == 1 && c.misses == 1 && c.writes == 1 && c.hit_rate_permille() == 500, "");
    // 3) 命中率 750‰（3 命中 1 未命中）。
    let mut c2 = BuildCache::new();
    let _ = c2.lookup(b"x");
    let _ = c2.write(b"x", 16, "x");
    for _ in 0..3 {
        let _ = c2.lookup(b"x");
    }
    cs.add("hit_rate_permille_750", c2.hit_rate_permille() == 750, "");
    // 4) 槽满显性 Err；33 键入 32 槽鸽笼必冲突 → 探测计数 > 0。
    let mut c3 = BuildCache::new();
    c3.budget_bytes = 1000;
    let mut last = Ok(0usize);
    for i in 0..34u8 {
        last = c3.write(&[b'k', i], 10, "k");
    }
    cs.add("cache_full_probe_accounted", last == Err("cache-full") && c3.writes == 32 && c3.probes > 0, "");
    // 5) 超预算 LRU 逐出最旧并记账。
    let mut c4 = BuildCache::new();
    c4.budget_bytes = 100;
    let _ = c4.write(b"e0", 40, "e0");
    let _ = c4.write(b"e1", 40, "e1");
    let _ = c4.write(b"e2", 30, "e2");
    cs.add("budget_lru_evicts_oldest", c4.evict_log() == ["e0"] && c4.evictions == 1 && c4.used_bytes == 70 && c4.lookup(b"e1").is_ok(), "");
    // 6) 热条目按命中计数保护：冷邻居被逐出，热者存活。
    let mut c5 = BuildCache::new();
    c5.budget_bytes = 100;
    let _ = c5.write(b"h1", 40, "h1");
    let _ = c5.write(b"h2", 40, "h2");
    for _ in 0..3 {
        let _ = c5.lookup(b"h1");
    }
    let _ = c5.write(b"h3", 30, "h3");
    cs.add("hot_entries_survive_eviction", c5.evict_log() == ["h2"] && c5.lookup(b"h1").is_ok(), "");
    // 7) 超预算单项显性拒绝。
    let mut c6 = BuildCache::new();
    c6.budget_bytes = 100;
    cs.add("oversize_budget_rejected", c6.write(b"big", 150, "big") == Err("budget-exceeded") && c6.writes == 0, "");
    // 8) 指纹变更 → 全缓存失效 + 记因；失效期读写拒绝。
    let mut c7 = BuildCache::new();
    let _ = c7.write(b"a", 16, "a");
    let _ = c7.write(b"b", 16, "b");
    c7.set_fingerprint(2);
    let inv = c7.invalidated && c7.invalidated_entries == 2 && c7.invalid_reasons() == ["toolchain-fingerprint-changed"];
    cs.add("fingerprint_change_invalidates", inv && c7.lookup(b"a") == Err("cache-invalidated") && c7.write(b"c", 8, "c") == Err("cache-invalidated"), "");
    // 9) 同指纹登记 → 原样（无操作、无重复记因）。
    c7.set_fingerprint(2);
    cs.add("same_fingerprint_noop", c7.invalid_n == 1 && c7.invalidated, "");
    // 10) 失效解除 → 重新预热可写可命中。
    c7.resume();
    let w = c7.write(b"fresh", 32, "fresh");
    let h = c7.lookup(b"fresh");
    cs.add("resume_after_invalidation", !c7.invalidated && w.is_ok() && h.is_ok() && c7.hits == 1, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn write_updates_in_place_keeps_hits() {
        let mut c = BuildCache::new();
        let _ = c.write(b"art", 64, "art");
        for _ in 0..2 {
            let _ = c.lookup(b"art");
        }
        let slot = c.write(b"art", 128, "art").expect("同哈希原位更新必成");
        assert_eq!(c.entries[slot].unwrap().hits, 2, "更新保持命中计数");
        assert_eq!(c.used_bytes, 128, "字节账随更新回落/上调");
        assert_eq!(c.writes, 2);
    }
    #[test]
    fn eviction_log_is_ordered() {
        let mut c = BuildCache::new();
        c.budget_bytes = 60;
        let _ = c.write(b"p", 30, "p");
        let _ = c.write(b"q", 30, "q");
        let _ = c.write(b"r", 30, "r");
        let _ = c.write(b"s", 30, "s");
        assert_eq!(c.evict_log(), ["p", "q"], "逐出账按最旧序如实排列");
        assert_eq!(c.used_bytes, 60);
        assert!(c.lookup(b"r").is_ok() && c.lookup(b"s").is_ok());
    }
    #[test]
    fn deep4_checks_all_green() {
        let cs = run_f033f_checks();
        assert!(cs.all_passed() && !cs.truncated());
    }
}
