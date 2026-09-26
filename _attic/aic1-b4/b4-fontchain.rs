
// ---------------------------------------------------------------------------
// F016 · 深化批次四：字体度量缓存（入字形图集管线）+ E7 换族整体失效
//
// 主册依据（G-A-16【数据与存储】）：「字体度量缓存入字形图集管线」；【设计
/// 细节】「E7 页可整体换族（映射跟随用户选择）」——换族的缓存语义：度量按
/// (族, 字号) 缓存，**换族即整体失效**（旧族度量对新族是错的，不清 = 静默错位）。
// ---------------------------------------------------------------------------

/// 度量缓存（键 = 族哈希 + 字号 px；容量 32——50 件采样常用组合远低于此）。
pub struct MetricsCache {
    keys: [(u64, u16); 32],
    n: usize,
    pub hits: u64,
    pub misses: u64,
}

impl MetricsCache {
    pub const fn new() -> MetricsCache {
        MetricsCache { keys: [(0, 0); 32], n: 0, hits: 0, misses: 0 }
    }

    /// 查度量（命中/未命中如实分账）。
    pub fn lookup(&mut self, family_hash: u64, size_px: u16) -> bool {
        let hit = self.keys[..self.n].contains(&(family_hash, size_px));
        if hit {
            self.hits += 1;
        } else {
            self.misses += 1;
        }
        hit
    }

    /// 插入（重复插入幂等；满容如实 false——调用方可 clear 重来）。
    pub fn insert(&mut self, family_hash: u64, size_px: u16) -> bool {
        if self.keys[..self.n].contains(&(family_hash, size_px)) {
            return true;
        }
        if self.n >= self.keys.len() {
            return false;
        }
        self.keys[self.n] = (family_hash, size_px);
        self.n += 1;
        true
    }

    /// E7 换族：度量整体失效（不清 = 静默错位——清零是换族的必经步骤）。
    pub fn invalidate_all(&mut self) -> usize {
        let cleared = self.n;
        self.n = 0;
        cleared
    }

    pub fn len(&self) -> usize {
        self.n
    }
}

/// F016 深化批次四自检。
pub fn run_fontchain_deep3_checks() -> CheckSet {
    let mut cs = CheckSet::new("F016-fontchain-deep3");
    // 1) 缓存行为：未命中 → 插入 → 命中；(族,字号) 二元组分档（同族不同字号
    //    是不同键——度量随字号变）。
    let mut mc = MetricsCache::new();
    let m1 = mc.lookup(0xFACE, 12);
    mc.insert(0xFACE, 12);
    mc.insert(0xFACE, 24);
    let h1 = mc.lookup(0xFACE, 12);
    let h2 = mc.lookup(0xFACE, 24);
    let h3 = mc.lookup(0xBABE, 12);
    cs.add(
        "metrics_cache_keyed_by_family_and_size",
        !m1 && h1 && h2 && !h3 && mc.len() == 2 && mc.hits == 2 && mc.misses == 2,
        "",
    );
    // 2) E7 换族失效：clear 后全部未命中（旧族度量不残留——静默错位的防线）。
    let cleared = mc.invalidate_all();
    let after = mc.lookup(0xFACE, 12);
    cs.add(
        "metrics_invalidate_on_family_swap",
        cleared == 2 && !after && mc.len() == 0,
        "",
    );
    // 3) 满容如实 false（不静默丢——调用方走 clear 重试）。
    let mut full = MetricsCache::new();
    let mut all_ok = true;
    for i in 0..32u64 {
        all_ok &= full.insert(0x1000 + i, 12);
    }
    let overflow = full.insert(0xFFFF, 12);
    cs.add("metrics_cache_cap_honest", all_ok && !overflow, "");
    cs
}
