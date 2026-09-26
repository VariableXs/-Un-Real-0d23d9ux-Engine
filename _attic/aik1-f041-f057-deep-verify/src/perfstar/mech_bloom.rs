//! mech_bloom — 布隆过滤器与计数布隆本体（AI-K1 深化批次五 · F046）。
//!
//! 主册依据：
//! - F046【设计细节】「写合并」——域内有写曲线与脏页基数树
//!   （mech_radix 管精确账），但**廉价的"见过没有"判定**缺席：写合并
//!   需要快速回答"这块是否已在合并窗里"而付出 O(1)、空间几个 KB——
//!   这正是布隆过滤器的语义（可能有假阳性、绝无假阴性）。本件实现：
//!   ① 经典布隆（4096 位 × 3 哈希、双重哈希构造 K 个独立位、FPR
//!   实测 vs 理论公式 (1-e^{-kn/m})^k 对拍）；② 计数布隆（4 位计数器，
//!   支持删除、饱和 15 诚实钳制）。
//! - 锚点：F046 写合并窗的成员查询底盘；零假阴性是判据的硬面
//!   （漏合并 = 数据风险），假阳性只损失效率——判例必须分开验两面。
//! - 零堆、零浮点（理论 FPR 用定点/查表近似比较）。

// ---------------------------------------------------------------------------
// 1. 哈希与布隆
// ---------------------------------------------------------------------------

/// 位数组大小（位）。
pub const M_BITS: usize = 4096;
/// 哈希函数个数。
pub const K: usize = 3;

/// FNV-1a 64 双哈希对（h1 直接、h2 折叠——Kirsch-Mitzenmacher 构造）。
pub fn hash_pair(key: u64) -> (u64, u64) {
    let mut h1 = 0xcbf2_9ce4_8422_2325u64;
    for b in key.to_le_bytes() {
        h1 ^= b as u64;
        h1 = h1.wrapping_mul(0x100_0000_01b3);
    }
    let h2 = (h1 >> 33) | 1; // 奇数步长，保证遍历周期覆盖
    (h1, h2)
}

/// 经典布隆过滤器。
pub struct Bloom {
    bits: [u64; M_BITS / 64],
    pub inserted: u64,
    /// 查询计数（真阳/假阳拆账由调用方比对真值表）。
    pub queries: u64,
    pub positives: u64,
}

impl Bloom {
    pub fn new() -> Self {
        Bloom { bits: [0; M_BITS / 64], inserted: 0, queries: 0, positives: 0 }
    }

    fn set_bit(&mut self, i: usize) {
        self.bits[i / 64] |= 1u64 << (i % 64);
    }

    fn get_bit(&self, i: usize) -> bool {
        (self.bits[i / 64] >> (i % 64)) & 1 == 1
    }

    pub fn insert(&mut self, key: u64) {
        let (h1, h2) = hash_pair(key);
        for k in 0..K {
            let idx = ((h1.wrapping_add((k as u64).wrapping_mul(h2))) % M_BITS as u64) as usize;
            self.set_bit(idx);
        }
        self.inserted += 1;
    }

    pub fn might_contain(&mut self, key: u64) -> bool {
        let (h1, h2) = hash_pair(key);
        let mut all = true;
        for k in 0..K {
            let idx = ((h1.wrapping_add((k as u64).wrapping_mul(h2))) % M_BITS as u64) as usize;
            if !self.get_bit(idx) {
                all = false;
                break;
            }
        }
        self.queries += 1;
        if all {
            self.positives += 1;
        }
        all
    }

    /// 置位密度（‰）。
    pub fn density_permille(&self) -> u64 {
        let set: u64 = self.bits.iter().map(|w| w.count_ones() as u64).sum();
        set * 1000 / M_BITS as u64
    }
}

impl Default for Bloom {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 2. 计数布隆（4 位计数器，支持删除）
// ---------------------------------------------------------------------------

/// 计数器宽度 4 位（饱和 15）。
pub const COUNTER_CAP: u8 = 15;

pub struct CountingBloom {
    /// 4 位计数器 × M_BITS 个（打包：每 u64 存 16 个计数器）。
    counters: [u64; M_BITS / 16],
    pub insertions: u64,
}

impl CountingBloom {
    pub fn new() -> Self {
        CountingBloom { counters: [0; M_BITS / 16], insertions: 0 }
    }

    #[inline]
    fn get(&self, i: usize) -> u8 {
        let word = self.counters[i / 16];
        ((word >> ((i % 16) * 4)) & 0xF) as u8
    }

    #[inline]
    fn bump(&mut self, i: usize, delta: i8) {
        let cur = self.get(i);
        let next = if delta > 0 { cur.saturating_add(1).min(COUNTER_CAP) } else { cur.saturating_sub(1) };
        let shift = (i % 16) * 4;
        let mask = 0xFu64 << shift;
        self.counters[i / 16] = (self.counters[i / 16] & !mask) | ((next as u64) << shift);
    }

    pub fn insert(&mut self, key: u64) {
        let (h1, h2) = hash_pair(key);
        for k in 0..K {
            let idx = ((h1.wrapping_add((k as u64).wrapping_mul(h2))) % M_BITS as u64) as usize;
            self.bump(idx, 1);
        }
        self.insertions += 1;
    }

    pub fn remove(&mut self, key: u64) -> bool {
        // 预检：任一计数器为 0 → 从未插入（诚实拒绝，不产生负计数）。
        let (h1, h2) = hash_pair(key);
        for k in 0..K {
            let idx = ((h1.wrapping_add((k as u64).wrapping_mul(h2))) % M_BITS as u64) as usize;
            if self.get(idx) == 0 {
                return false;
            }
        }
        for k in 0..K {
            let idx = ((h1.wrapping_add((k as u64).wrapping_mul(h2))) % M_BITS as u64) as usize;
            self.bump(idx, -1);
        }
        true
    }

    pub fn might_contain(&self, key: u64) -> bool {
        let (h1, h2) = hash_pair(key);
        for k in 0..K {
            let idx = ((h1.wrapping_add((k as u64).wrapping_mul(h2))) % M_BITS as u64) as usize;
            if self.get(idx) == 0 {
                return false;
            }
        }
        true
    }

    /// 全零校验（删除完整性用）。
    pub fn is_empty(&self) -> bool {
        self.counters.iter().all(|&w| w == 0)
    }
}

impl Default for CountingBloom {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 3. CheckSet
// ---------------------------------------------------------------------------

pub fn run_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    let mut cs = CheckSet::new("mech_bloom");

    // 1) 零假阴性硬面：全部已插入键必须命中（一个都不能漏）。
    {
        let mut b = Bloom::new();
        for k in 0..500u64 {
            b.insert(k * 7919);
        }
        let mut miss = 0;
        for k in 0..500u64 {
            if !b.might_contain(k * 7919) {
                miss += 1;
            }
        }
        cs.add("bloom_zero_false_negative", miss == 0, "");
    }

    // 2) FPR 实测 ≤ 理论上界 + 余量：n=500, m=4096, k=3 →
    //    p = (1-e^{-kn/m})^k ≈ (1-e^{-0.366})^3 ≈ 0.128。
    {
        let mut b = Bloom::new();
        for k in 0..500u64 {
            b.insert(k);
        }
        let mut fp = 0u64;
        let mut probes = 0u64;
        // 用与插入集不相交的键域探测。
        for k in 100_000..102_000u64 {
            probes += 1;
            if b.might_contain(k) {
                fp += 1;
            }
        }
        let fpr_bp = fp * 10_000 / probes; // 基点（0.01%）
        cs.add("bloom_fpr_bounded", fpr_bp <= 2_500, "");
    }

    // 3) 密度账：置位数 ≈ m(1-e^{-kn/m})，密度应落在理论 ±30% 内。
    {
        let mut b = Bloom::new();
        for k in 0..500u64 {
            b.insert(k);
        }
        let d = b.density_permille();
        // 理论：1-e^{-1500/4096} ≈ 0.306 → 306‰。
        cs.add("bloom_density_near_theory", d >= 214 && d <= 398, "");
    }

    // 4) 计数布隆删除：插入→删除→查否（计数归零）；未插入的删除被拒。
    {
        let mut cb = CountingBloom::new();
        cb.insert(42);
        cb.insert(42);
        cb.insert(42);
        assert!(cb.might_contain(42));
        assert!(cb.remove(42));
        assert!(cb.remove(42));
        assert!(cb.remove(42));
        cs.add(
            "cbloom_remove_exact",
            !cb.might_contain(42) && !cb.remove(42) && !cb.remove(99_999),
            "",
        );
    }

    // 5) 计数器饱和诚实：同一键塞 20 次钳在 15，删除 15 次后拒绝第 16 次。
    {
        let mut cb = CountingBloom::new();
        for _ in 0..20 {
            cb.insert(7);
        }
        // 饱和语义：might_contain 仍真。
        let sat_true = cb.might_contain(7);
        let mut removed = 0;
        while cb.remove(7) {
            removed += 1;
        }
        cs.add(
            "cbloom_saturation_honest",
            sat_true && removed == 15 && cb.is_empty(),
            "",
        );
    }

    // 6) 空过滤器全否（无误报起点）。
    {
        let mut b = Bloom::new();
        let mut any = false;
        for k in 0..1_000u64 {
            if b.might_contain(k) {
                any = true;
            }
        }
        cs.add("bloom_empty_all_negative", !any && b.queries == 1_000, "");
    }

    cs
}

// ---------------------------------------------------------------------------
// 4. 宿主单测
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_false_negatives_across_loads() {
        for n in [10u64, 100, 500, 1500] {
            let mut b = Bloom::new();
            for k in 0..n {
                b.insert(k.wrapping_mul(0x9E37_79B9_7F4A_7C15));
            }
            for k in 0..n {
                assert!(b.might_contain(k.wrapping_mul(0x9E37_79B9_7F4A_7C15)), "n={} k={}", n, k);
            }
        }
    }

    #[test]
    fn fpr_measured_matches_formula() {
        // n=500: 理论 FPR ≈ 12.8%。测 2000 个负例，假阳应 < 25%（>2×余量）
        // 且 > 0（假阳性确实存在——否则哈希退化，检不出真布隆行为）。
        let mut b = Bloom::new();
        for k in 0..500u64 {
            b.insert(k);
        }
        let mut fp = 0;
        for k in 200_000..202_000u64 {
            if b.might_contain(k) {
                fp += 1;
            }
        }
        assert!(fp > 0, "完全零假阳说明哈希或位操作退化");
        assert!(fp < 500, "fp={} 超出理论 12.8% 的 2 倍", fp);
    }

    #[test]
    fn density_tracks_fill() {
        let mut b = Bloom::new();
        assert_eq!(b.density_permille(), 0);
        for k in 0..100u64 {
            b.insert(k);
        }
        let d_low = b.density_permille();
        for k in 100..1500u64 {
            b.insert(k);
        }
        let d_high = b.density_permille();
        assert!(d_high > d_low, "密度随负载上升");
        assert!(d_high <= 1000);
    }

    #[test]
    fn counting_bloom_exact_remove_semantics() {
        let mut cb = CountingBloom::new();
        let keys = [11u64, 22, 33];
        for k in keys {
            cb.insert(k);
            cb.insert(k);
        }
        // 删一次后仍 contains（还有 1 份）。
        assert!(cb.remove(11));
        assert!(cb.might_contain(11));
        // 再删一次后 exact-zero。
        assert!(cb.remove(11));
        // 其他键不受影响（共享位的计数仍 > 0——若被借走则说明计数
        // 冲突，查否即可，不崩溃）。
        assert!(cb.might_contain(22));
        assert!(cb.might_contain(33));
        // 全删干净 → 过滤器空。
        cb.remove(22);
        cb.remove(22);
        cb.remove(33);
        cb.remove(33);
        assert!(cb.is_empty());
    }

    #[test]
    fn counting_bloom_rejects_never_inserted() {
        let mut cb = CountingBloom::new();
        assert!(!cb.remove(123));
        cb.insert(123);
        assert!(!cb.remove(456));
        assert!(cb.remove(123));
        assert!(!cb.remove(123));
    }

    #[test]
    fn hash_pair_is_stable_and_odd_step() {
        let (a1, a2) = hash_pair(42);
        let (b1, b2) = hash_pair(42);
        assert_eq!((a1, a2), (b1, b2));
        assert_eq!(a2 % 2, 1);
        let (c1, _) = hash_pair(43);
        assert_ne!(a1, c1);
    }
}
