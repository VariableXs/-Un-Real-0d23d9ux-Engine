
// ---------------------------------------------------------------------------
// F014 · 深化批次四：图标缓存键（文件哈希+尺寸 入缩略图库）+ 版本信息不缓存
// 纪律钉值
//
// 主册依据（G-A-14【数据与存储】）：「图标缓存按 (文件哈希+尺寸) 入缩略图库」
// ；「版本信息不缓存（属性页现取现显）」。既有面：pick_group_member/pick_icon
// _ladder（选择策略）不重复——本段补缓存键面与纪律锚。
// ---------------------------------------------------------------------------

/// 图标缓存键（FNV-1a 混合：文件哈希 × 尺寸档——同文件不同尺寸档各占一槽，
/// 4K 管线多档消费的前提）。
pub fn icon_cache_key_of(file_hash: u64, size_px: u16) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for b in file_hash.to_le_bytes().iter().chain(size_px.to_le_bytes().iter()) {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 图标缓存（键 → 命中记账；容量 32 档——50 件清单 × 4 尺寸档远低于此）。
pub struct IconCache {
    keys: [Option<u64>; 32],
    n: usize,
    pub hits: u64,
    pub misses: u64,
}

impl IconCache {
    pub const fn new() -> IconCache {
        IconCache { keys: [None; 32], n: 0, hits: 0, misses: 0 }
    }

    /// 查缓存（命中/未命中如实分账）。
    pub fn lookup(&mut self, file_hash: u64, size_px: u16) -> bool {
        let k = icon_cache_key_of(file_hash, size_px);
        let hit = self.keys[..self.n].contains(&Some(k));
        if hit {
            self.hits += 1;
        } else {
            self.misses += 1;
        }
        hit
    }

    /// 插入（同键重复插入不重复占槽——幂等）。
    pub fn insert(&mut self, file_hash: u64, size_px: u16) -> bool {
        let k = icon_cache_key_of(file_hash, size_px);
        if self.keys[..self.n].contains(&Some(k)) {
            return true;
        }
        if self.n >= self.keys.len() {
            return false;
        }
        self.keys[self.n] = Some(k);
        self.n += 1;
        true
    }

    pub fn len(&self) -> usize {
        self.n
    }
}

/// 版本信息缓存纪律：**恒不缓存**（属性页现取现显——一处一事实钉值）。
pub const VERSION_INFO_CACHED: bool = false;

/// F014 深化批次四自检。
pub fn run_persrc_deep3_checks() -> CheckSet {
    let mut cs = CheckSet::new("F014-persrc-deep3");
    // 1) 缓存键：同文件同尺寸 = 同键；同文件不同尺寸 ≠ 同键（多档消费前提）。
    let k1 = icon_cache_key_of(0xFEED, 256);
    let k2 = icon_cache_key_of(0xFEED, 32);
    cs.add(
        "icon_cache_key_dims_distinct",
        k1 == icon_cache_key_of(0xFEED, 256) && k1 != k2,
        "",
    );
    // 2) 缓存行为：未命中 → 插入 → 命中；分账准确；重复插入幂等不占双槽。
    let mut cache = IconCache::new();
    let m1 = cache.lookup(0xFEED, 256);
    let i1 = cache.insert(0xFEED, 256);
    let i2 = cache.insert(0xFEED, 256);
    let h1 = cache.lookup(0xFEED, 256);
    cs.add(
        "icon_cache_miss_insert_hit",
        !m1 && i1 && i2 && h1 && cache.len() == 1 && cache.hits == 1 && cache.misses == 1,
        "",
    );
    // 3) 版本信息不缓存纪律钉值（现取现显——无缓存写路径）。
    cs.add("version_info_never_cached", !VERSION_INFO_CACHED, "");
    cs
}
