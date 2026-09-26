
// ---------------------------------------------------------------------------
// F003 · 深化批次八：绑定缓存 LRU 淘汰面（会话缓存容量纪律——批次四做了
// 只读共享，本段补「满了谁让位」：最久未命中的条目先出，命中刷新热度）。
// ---------------------------------------------------------------------------

/// LRU 绑定缓存（容量 4——会话缓存的小模型；热度由最近命中位次决定）。
pub struct BindCacheLru {
    entries: [Option<(u64, u32)>; 4], // (模块哈希, 绑定结果代号)
    stamp: [u32; 4],                  // 最近使用序（越大越新）
    clock: u32,
    pub evictions: u32,
    pub hits: u32,
    pub misses: u32,
}

impl BindCacheLru {
    pub fn new() -> BindCacheLru {
        BindCacheLru {
            entries: [None; 4],
            stamp: [0; 4],
            clock: 0,
            evictions: 0,
            hits: 0,
            misses: 0,
        }
    }

    fn slot_of(&self, module_hash: u64) -> Option<usize> {
        self.entries.iter().position(|e| matches!(e, Some((h, _)) if *h == module_hash))
    }

    /// 命中返回绑定结果并刷新热度；未命中走 insert（满则淘汰最久未用）。
    pub fn lookup_or_insert(&mut self, module_hash: u64, bind_result: u32) -> u32 {
        self.clock += 1;
        if let Some(i) = self.slot_of(module_hash) {
            self.hits += 1;
            self.stamp[i] = self.clock;
            return match self.entries[i] {
                Some((_, v)) => v,
                None => 0,
            };
        }
        self.misses += 1;
        let slot = match self.entries.iter().position(|e| e.is_none()) {
            Some(i) => i,
            None => {
                // 淘汰最久未用（stamp 最小者）——LRU 语义本体。
                let mut victim = 0;
                for i in 1..4 {
                    if self.stamp[i] < self.stamp[victim] {
                        victim = i;
                    }
                }
                self.evictions += 1;
                victim
            }
        };
        self.entries[slot] = Some((module_hash, bind_result));
        self.stamp[slot] = self.clock;
        bind_result
    }
}

/// F003 深化批次八自检。
fn run_pebind_deep7_checks() -> CheckSet {
    let mut cs = CheckSet::new("F003-pebind-deep7");
    let mut cache = BindCacheLru::new();
    // 1) 填满 4 槽 + 命中刷新：A 命中后 A 不被淘汰，B（最旧未命中）先出。
    let _ = cache.lookup_or_insert(1, 10);
    let _ = cache.lookup_or_insert(2, 20);
    let _ = cache.lookup_or_insert(3, 30);
    let _ = cache.lookup_or_insert(4, 40);
    let a_again = cache.lookup_or_insert(1, 10); // 命中，热度刷新
    let _ = cache.lookup_or_insert(5, 50); // 淘汰 2（此时最久未用）
    cs.add(
        "lru_evicts_least_recently_used",
        a_again == 10
            && cache.hits == 1
            && cache.evictions == 1
            && cache.slot_of(2).is_none()
            && cache.slot_of(1).is_some()
            && cache.slot_of(5).is_some(),
        "",
    );
    // 2) 未命中如实计数；容量硬顶 4 槽不超。
    let _ = cache.lookup_or_insert(6, 60);
    let _ = cache.lookup_or_insert(7, 70);
    let occupied = cache.entries.iter().filter(|e| e.is_some()).count();
    cs.add(
        "lru_capacity_and_miss_accounting",
        cache.misses == 7 && occupied == 4 && cache.evictions == 3,
        "",
    );
    cs
}
