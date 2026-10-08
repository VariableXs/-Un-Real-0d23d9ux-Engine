//! F028 深化批次四 · DPI 资源选型面（compatstar2/deep3 · G-A-28）。
//!
//! 批次一~三覆盖三态执行/DPICHANGED 宽限/坐标虚拟化等程序可见面；本批
//! 补齐主册【功能定义】「全语义对齐」的序列化/账本/容错面：资产倍率桶
//! 选型（1x/1.25x/1.5x/2x 四桶，就近舍入——0.5 步进四舍五入、边界值
//! 标定、越界钳制）、位图缓存键（桶 × 尺寸定长 16 键表，命中/未命中/
//! 替换三账）、桶变更重渲染触发（dpi 跨桶才触发，同桶跳过记账）、
//! 逐桶内存预算账（超预算 LRU 逐出计数，超大单件显性拒绝）。
//!
//! 判据对账：主册 G-A-28【设计细节】缩放档 100/125/150/175/200 五档 +
//! MS GetDpiForMonitor 文档语义对拍（96 DPI = 100% 基准；倍率 = dpi/96，
//! ‰ 口径存储）。零堆纪律：定长键表，无 Vec/String/Box/format!，错误
//! 一律 Err 或计数账面，零静默。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 资产倍率四桶（‰ 口径：1000 = 1x——对齐主册缩放档族）。
pub const BUCKETS_PERMILLE: [u32; 4] = [1000, 1250, 1500, 2000];
/// 位图缓存键表容量（定长 16——域内口径）。
pub const CACHE_SLOTS: usize = 16;
/// 逐桶内存预算字节（256 KiB/桶——域内口径）。
pub const BUCKET_BUDGET_BYTES: u32 = 256 * 1024;
/// 桶号索引（BUCKETS_PERMILLE 下标）。
pub const BUCKET_1X: usize = 0;

// ---------------------------------------------------------------------------
// 桶选型（就近舍入）
// ---------------------------------------------------------------------------

/// 倍率桶选型：就近舍入（恰在两桶中点 → 四舍五入进位到高档；越界钳到
/// 端桶）。边界标定：1125→1250、1375→1500、1750→2000。
pub fn select_bucket(scale_permille: u32) -> u32 {
    if scale_permille <= BUCKETS_PERMILLE[BUCKET_1X] {
        return BUCKETS_PERMILLE[BUCKET_1X];
    }
    for i in 0..3 {
        let lo = BUCKETS_PERMILLE[i];
        let hi = BUCKETS_PERMILLE[i + 1];
        let mid = (lo + hi) / 2;
        if scale_permille < mid {
            return lo;
        }
        if scale_permille <= mid {
            return hi;
        }
    }
    BUCKETS_PERMILLE[3]
}

/// 桶号 → 倍率值（‰）。
pub fn bucket_value(idx: usize) -> u32 {
    BUCKETS_PERMILLE[idx]
}

// ---------------------------------------------------------------------------
// 位图缓存键表（桶 × 尺寸）
// ---------------------------------------------------------------------------

/// 一个位图缓存键（桶 × 宽 × 高 + 字节数 + LRU 时钟戳）。
#[derive(Clone, Copy)]
pub struct CacheKey {
    pub bucket: u32,
    pub w: u16,
    pub h: u16,
    pub bytes: u32,
    pub stamp: u64,
}

/// 定长 16 键表：命中/未命中/替换三账 + 逐桶预算 + LRU 逐出账。
pub struct BitmapCache {
    pub entries: [Option<CacheKey>; CACHE_SLOTS],
    pub clock: u64,
    pub hits: u32,
    pub misses: u32,
    pub replacements: u32,
    pub evictions: u32,
    /// 逐桶占用字节（下标 = 桶在 BUCKETS_PERMILLE 中的位次）。
    pub used: [u32; 4],
}

impl BitmapCache {
    pub const fn new() -> Self {
        BitmapCache {
            entries: [None; CACHE_SLOTS],
            clock: 0,
            hits: 0,
            misses: 0,
            replacements: 0,
            evictions: 0,
            used: [0; 4],
        }
    }

    fn bucket_idx(bucket: u32) -> usize {
        for (i, b) in BUCKETS_PERMILLE.iter().enumerate() {
            if *b == bucket {
                return i;
            }
        }
        BUCKET_1X
    }

    fn find_slot(&self, bucket: u32, w: u16, h: u16) -> Option<usize> {
        self.entries.iter().position(|s| matches!(s, Some(k) if k.bucket == bucket && k.w == w && k.h == h))
    }

    /// 全表 LRU 槽（stamp 最小者）——替换语义用。
    fn lru_slot(&self) -> usize {
        let mut oldest = 0usize;
        for i in 1..CACHE_SLOTS {
            if self.entries[i].unwrap().stamp < self.entries[oldest].unwrap().stamp {
                oldest = i;
            }
        }
        oldest
    }

    /// 指定桶内 LRU 槽（预算逐出语义用）。
    fn lru_slot_in(&self, bucket: u32) -> Option<usize> {
        let mut best: Option<usize> = None;
        for i in 0..CACHE_SLOTS {
            if let Some(k) = self.entries[i] {
                if k.bucket == bucket && best.map_or(true, |b| k.stamp < self.entries[b].unwrap().stamp) {
                    best = Some(i);
                }
            }
        }
        best
    }

    /// 查找：命中提新（LRU stamp）；未命中入账。
    pub fn lookup(&mut self, bucket: u32, w: u16, h: u16) -> bool {
        self.clock += 1;
        match self.find_slot(bucket, w, h) {
            Some(i) => {
                if let Some(k) = self.entries[i].as_mut() {
                    k.stamp = self.clock;
                }
                self.hits += 1;
                true
            }
            None => {
                self.misses += 1;
                false
            }
        }
    }

    /// 插入键：重复 → 提新；有空位 → 直插；全满 → 替换全表 LRU（记账）。
    /// 之后逐桶预算：超预算 → 逐出桶内 LRU（记账）；单件超大 → 显性拒绝。
    pub fn insert(&mut self, bucket: u32, w: u16, h: u16, bytes: u32) -> Result<(), &'static str> {
        self.clock += 1;
        if let Some(i) = self.find_slot(bucket, w, h) {
            if let Some(k) = self.entries[i].as_mut() {
                k.stamp = self.clock; // 重复键：提新语义（LRU 位置刷新）
            }
            return Ok(());
        }
        if bytes > BUCKET_BUDGET_BYTES {
            return Err("oversize");
        }
        let idx = match self.entries.iter().position(|s| s.is_none()) {
            Some(i) => i,
            None => {
                let old = self.lru_slot();
                let victim = self.entries[old].unwrap();
                self.used[Self::bucket_idx(victim.bucket)] -= victim.bytes;
                self.replacements += 1;
                old
            }
        };
        let bi = Self::bucket_idx(bucket);
        self.entries[idx] = Some(CacheKey { bucket, w, h, bytes, stamp: self.clock });
        self.used[bi] += bytes;
        while self.used[bi] > BUCKET_BUDGET_BYTES {
            match self.lru_slot_in(bucket) {
                Some(i) => {
                    let victim = self.entries[i].take().unwrap();
                    self.used[bi] -= victim.bytes;
                    self.evictions += 1;
                }
                None => break,
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 桶变更重渲染触发
// ---------------------------------------------------------------------------

/// 重渲染闸：dpi 跨桶才触发重渲染；同桶跳过并记账（不撕裂不空转）。
pub struct RerenderGate {
    pub last_bucket: u32,
    pub triggers: u32,
    pub skips: u32,
}

impl RerenderGate {
    pub const fn new() -> Self {
        RerenderGate { last_bucket: BUCKETS_PERMILLE[BUCKET_1X], triggers: 0, skips: 0 }
    }

    /// dpi 变更通知（scale_permille = dpi×1000/96）。跨桶返回 true。
    pub fn on_dpi(&mut self, scale_permille: u32) -> bool {
        let nb = select_bucket(scale_permille);
        if nb == self.last_bucket {
            self.skips += 1;
            false
        } else {
            self.last_bucket = nb;
            self.triggers += 1;
            true
        }
    }
}

// ---------------------------------------------------------------------------
// 域自检（深化批次四）
// ---------------------------------------------------------------------------

/// 域自检（F028 深化批次四 · 桶选型/缓存/触发/预算）。
pub fn run_f028f_checks() -> CheckSet {
    let mut cs = CheckSet::new("F028-dpiasst-d4");
    // 1) 端点标定：四桶倍率原值各自命中本桶。
    cs.add(
        "bucket_exact_hit",
        select_bucket(1000) == 1000 && select_bucket(1250) == 1250 && select_bucket(1500) == 1500 && select_bucket(2000) == 2000,
        "",
    );
    // 2) 中点四舍五入进位：1125/1375/1750 → 高档（0.5 步进规则）。
    cs.add(
        "bucket_midpoint_round_up",
        select_bucket(1125) == 1250 && select_bucket(1375) == 1500 && select_bucket(1750) == 2000,
        "",
    );
    // 3) 中点之下归低档：1124/1374/1749 → 低档。
    cs.add(
        "bucket_below_mid_keeps_low",
        select_bucket(1124) == 1000 && select_bucket(1374) == 1250 && select_bucket(1749) == 1500,
        "",
    );
    // 4) 越界钳制：800 → 1x；2400 → 2x。
    cs.add("bucket_clamp_out_of_range", select_bucket(800) == 1000 && select_bucket(2400) == 2000, "");
    // 5) 缓存命中/未命中三账（同键命中、异键未命中）。
    let mut cache = BitmapCache::new();
    let _ = cache.insert(1000, 32, 32, 4096);
    let hit = cache.lookup(1000, 32, 32);
    let miss = cache.lookup(1000, 64, 64);
    cs.add("cache_hit_miss_ledger", hit && !miss && cache.hits == 1 && cache.misses == 1, "");
    // 6) 表满替换 LRU：16 键占满 → 第 17 键替换最旧（首个未提新者）。
    let mut full = BitmapCache::new();
    for i in 0..CACHE_SLOTS as u16 {
        let _ = full.insert(1000, 16 + i, 16 + i, 1024);
    }
    let _ = full.insert(1000, 9999, 1, 1024);
    cs.add(
        "cache_replace_lru_when_full",
        full.replacements == 1 && !full.lookup(1000, 16, 16) && full.lookup(1000, 9999, 1),
        "",
    );
    // 7) 跨桶触发：1000‰ → 1250‰ 触发重渲染。
    let mut gate = RerenderGate::new();
    let t = gate.on_dpi(1250);
    cs.add("rerender_cross_bucket_triggers", t && gate.triggers == 1, "");
    // 8) 同桶跳过：1250‰ → 1300‰ 仍 1250 桶 → 跳过记账。
    let skip = gate.on_dpi(1300);
    cs.add("rerender_same_bucket_skips", !skip && gate.skips == 1, "");
    // 9) 逐桶预算：96 KiB × 3 = 288 KiB > 256 KiB → 第三笔逐出桶内 LRU 一件。
    let mut bud = BitmapCache::new();
    let _ = bud.insert(1000, 1, 1, 96 * 1024);
    let _ = bud.insert(1000, 2, 2, 96 * 1024);
    let _ = bud.insert(1000, 3, 3, 96 * 1024);
    cs.add(
        "budget_lru_evicts",
        bud.evictions == 1 && bud.used[0] == 192 * 1024 && !bud.lookup(1000, 1, 1) && bud.lookup(1000, 3, 3),
        "",
    );
    // 10) 单件超大显性拒绝：300 KiB > 256 KiB → Err，账面不动。
    let mut big = BitmapCache::new();
    let r = big.insert(1000, 1, 1, 300 * 1024);
    cs.add("budget_oversize_refused", r == Err("oversize") && big.used[0] == 0 && big.evictions == 0, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hit_bumps_lru_order() {
        // 命中提新改变替换对象：16 键占满后提新最旧者，第 17 键替换次旧。
        let mut c = BitmapCache::new();
        for i in 0..CACHE_SLOTS as u16 {
            let _ = c.insert(1000, 16 + i, 16 + i, 16);
        }
        assert!(c.lookup(1000, 16, 16)); // 提新最旧者 (16,16)
        let _ = c.insert(1000, 9999, 1, 16);
        assert_eq!(c.replacements, 1);
        assert!(c.lookup(1000, 16, 16), "被提新者应存活");
        assert!(!c.lookup(1000, 17, 17), "次旧者应被替换");
        assert!(c.lookup(1000, 9999, 1));
    }

    #[test]
    fn rerender_sequence_two_buckets() {
        // 1000→1250→1500 触发两次；随后 1750 仍在 2x? 不——1750 归 2000 桶
        // 之前 1500 桶内 1750>1500 → 实为 2000 桶？1500/2000 中点 1750 →
        // 1750 恰中点进位 2000 桶，仍触发；1800 同桶跳过。
        let mut g = RerenderGate::new();
        assert!(g.on_dpi(1250));
        assert!(g.on_dpi(1500));
        assert!(g.on_dpi(1750));
        assert!(!g.on_dpi(1800));
        assert_eq!(g.triggers, 3);
        assert_eq!(g.skips, 1);
    }

    #[test]
    fn deep4_checks_all_green() {
        let cs = run_f028f_checks();
        assert!(cs.all_passed() && !cs.truncated());
    }
}
