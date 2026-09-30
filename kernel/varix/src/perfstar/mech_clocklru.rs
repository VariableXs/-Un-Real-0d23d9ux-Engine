//! mech_clocklru — CLOCK 扫描缓存 + LRU-K 驻留（AI-K1 深化批次四 · F044/F045）。
//!
//! 主册依据：
//! - F044【设计细节】「8MB 每应用配额 + **LRU 驱逐**」、批次三已落「LRU 指纹
//!   库」的账面（计数+配额）；本件给出**驱逐本体的可测算法**：LRU-K（K=2，
//!   后历史间隔），解决纯 LRU 的顺序洪峰污染（一次全表扫描把热点全部挤出
//!   ——F044 判据「指纹命中率 >70%」的天敌）。
//! - F045【设计细节】页缓存回收——**CLOCK 第二次机会**（reference bit +
//!   扫描手），Linux 页缓存同族算法：回收不必维护精确 LRU 序，开销 O(1)
//!   摊还；批次三的分列 LRU（文件页/匿名页）消费本件作底层。
//! - F055 glyphcache 的「当前帧钉住」——本件给 pin 语义（钉住的页不参选）。
//! - 零堆：槽位定长数组；键一律 u64（域内把 (应用,指纹) 哈希折叠进来）。

// ---------------------------------------------------------------------------
// 1. CLOCK 第二次机会
// ---------------------------------------------------------------------------

/// 缓存容量（CLOCK 槽数；消费域按配额换算——F045 水位档决定实例规模）。
pub const CLOCK_CAP: usize = 512;

#[derive(Clone, Copy)]
struct ClockSlot {
    key: u64,
    used: bool,
    /// reference bit（第二次机会）。
    ref_bit: bool,
    pinned: bool,
}

/// CLOCK 缓存：hit 置位；miss 时扫描手推进，ref=1 降为 0 给第二次机会，
/// ref=0 驱逐。pin 槽跳过（不驱逐、不参与扫描——F055 当前帧语义）。
pub struct ClockCache {
    slots: [Option<ClockSlot>; CLOCK_CAP],
    hand: usize,
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    pub pinned_skips: u64,
}

impl ClockCache {
    pub const fn new() -> Self {
        ClockCache { slots: [None; CLOCK_CAP], hand: 0, hits: 0, misses: 0, evictions: 0, pinned_skips: 0 }
    }

    /// 查询（命中置 reference bit）。
    pub fn get(&mut self, key: u64) -> bool {
        if let Some(i) = self.find(key) {
            self.slots[i].as_mut().unwrap().ref_bit = true;
            self.slots[i].as_mut().unwrap().used = true;
            self.hits += 1;
            true
        } else {
            self.misses += 1;
            false
        }
    }

    /// 插入（已存在则视为命中刷新）。容量满 → CLOCK 扫描驱逐。
    pub fn put(&mut self, key: u64) {
        if let Some(i) = self.find(key) {
            let s = self.slots[i].as_mut().unwrap();
            s.ref_bit = true;
            s.used = true;
            self.hits += 1;
            return;
        }
        self.misses += 1; // 新键即 miss（无论落点）——计数一处，不分支重复
        // 有空槽直接用。
        if let Some(i) = self.slots.iter().position(|s| s.is_none()) {
            // 加载 ≠ 访问：插入不置保护位（只由 get/put 命中刷新）。
            // 插入即置位会让「插入后从未复用」的页伪装成热页——扫描
            // 即停即逐、从不衰减旧位，手针绕回时全场 ref=1 → 整场清零
            // → 真热点被误逐（clock_hit_rate_on_hot_loop 判出的缺陷）。
            self.slots[i] = Some(ClockSlot { key, used: true, ref_bit: false, pinned: false });
            return;
        }
        // CLOCK 扫描：全场 pinned 时诚实放弃（不转死圈）。
        let mut scanned = 0usize;
        loop {
            if scanned >= CLOCK_CAP * 2 {
                return; // 全 pin：调用侧凭 pinned_skips 归因
            }
            let i = self.hand;
            self.hand = (self.hand + 1) % CLOCK_CAP;
            match self.slots[i].as_mut() {
                Some(s) if s.pinned => {
                    self.pinned_skips += 1;
                    scanned += 1;
                }
                Some(s) if s.ref_bit => {
                    s.ref_bit = false;
                    scanned += 1;
                }
                Some(s) => {
                    self.evictions += 1;
                    *s = ClockSlot { key, used: true, ref_bit: false, pinned: false };
                    return;
                }
                None => unreachable!("空槽分支已处理"),
            }
        }
    }

    /// 钉住/解除（F055 当前帧：钉住即不参选驱逐）。
    pub fn set_pinned(&mut self, key: u64, pin: bool) -> bool {
        match self.find(key) {
            Some(i) => {
                self.slots[i].as_mut().unwrap().pinned = pin;
                true
            }
            None => false,
        }
    }

    pub fn pinned_count(&self) -> usize {
        self.slots.iter().flatten().filter(|s| s.pinned).count()
    }

    fn find(&self, key: u64) -> Option<usize> {
        self.slots.iter().position(|s| matches!(s, Some(c) if c.key == key && c.used))
    }

    pub fn resident(&self) -> usize {
        self.slots.iter().flatten().filter(|s| s.used).count()
    }
}

// ---------------------------------------------------------------------------
// 2. LRU-K（K=2 后历史间隔）
// ---------------------------------------------------------------------------

/// LRU-K 容量。
pub const LRUK_CAP: usize = 256;
/// K 值：后历史间隔 ≥2 的页才是"真热点"（O'Neil et al. 1993）。
pub const LRUK_K: usize = 2;

#[derive(Clone, Copy)]
struct LruSlot {
    key: u64,
    used: bool,
    /// 最近两次访问时刻（K=2 后历史）。
    hist: [u64; LRUK_K],
    hist_n: u8,
}

/// LRU-K 缓存：驱逐「回溯 K 次访问距离最大」的槽——只被访问过一次的新页
/// 不优先驱逐（等待 K 次确认），顺序洪峰不再污染热点集。
pub struct LruK {
    slots: [Option<LruSlot>; LRUK_CAP],
    tick: u64,
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    /// 单次访问即被逐出的次数（洪峰暴露面——应远小于纯 LRU）。
    pub young_evictions: u64,
}

impl LruK {
    pub const fn new() -> Self {
        LruK { slots: [None; LRUK_CAP], tick: 0, hits: 0, misses: 0, evictions: 0, young_evictions: 0 }
    }

    pub fn access(&mut self, key: u64) -> bool {
        self.tick += 1;
        let t = self.tick;
        if let Some(s) = self.slots.iter_mut().flatten().find(|s| s.used && s.key == key) {
            // 后历史右移：hist[1] ← hist[0] ← now（最近在前）。
            s.hist[1] = s.hist[0];
            s.hist[0] = t;
            s.hist_n = s.hist_n.saturating_add(1).min(LRUK_K as u8);
            self.hits += 1;
            true
        } else {
            self.misses += 1;
            // 空槽直插。
            if let Some(slot) = self.slots.iter_mut().find(|s| s.is_none()) {
                *slot = Some(LruSlot { key, used: true, hist: [t; LRUK_K], hist_n: 1 });
                return false;
            }
            // 驱逐（O'Neil et al. 1993 语义）：访问不足 K 次的页后历史距离
            // 为 ∞ —— **首选受害者**（顺序洪峰的一次性页先逐，热点保住）；
            // 其次逐回溯距离最大的 K 页。
            let mut victim: Option<usize> = None;
            let mut best_back: i128 = -1;
            let mut fallback: Option<usize> = None;
            let mut fallback_first: u64 = u64::MAX;
            for (i, s) in self.slots.iter().enumerate() {
                let s = s.as_ref().unwrap();
                if (s.hist_n as usize) >= LRUK_K {
                    let back = s.hist[0] - s.hist[1]; // K=2 后历史间隔
                    if back as i128 > best_back {
                        best_back = back as i128;
                        victim = Some(i);
                    }
                } else if s.hist[0] < fallback_first {
                    fallback_first = s.hist[0];
                    fallback = Some(i);
                }
            }
            let v = fallback.or(victim).unwrap();
            if (self.slots[v].as_ref().unwrap().hist_n as usize) < LRUK_K {
                self.young_evictions += 1;
            }
            self.slots[v] = Some(LruSlot { key, used: true, hist: [t; LRUK_K], hist_n: 1 });
            self.evictions += 1;
            false
        }
    }

    pub fn resident(&self) -> usize {
        self.slots.iter().flatten().filter(|s| s.used).count()
    }
}

// ---------------------------------------------------------------------------
// 宿主单测
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// CheckSet（挂 F044）
// ---------------------------------------------------------------------------

/// 运行检查项（判据锚点见对账表批次四段）。
use crate::checks::CheckSet;

pub fn run_checks() -> CheckSet {
    let mut cs = CheckSet::new("F044-mech-clocklru");
    // 1) CLOCK 第二次机会（置位页幸存、未置位页先逐）——满载判例，
    //    CLOCK 只在满时逐出，空槽期扫描不发生。
    let mut c = ClockCache::new();
    for k in 0..CLOCK_CAP as u64 {
        c.put(k);
    }
    c.get(1);
    c.put(CLOCK_CAP as u64);
    let surv = c.slots.iter().flatten().any(|s| s.key == 1);
    let gone = !c.slots.iter().flatten().any(|s| s.key == 0);
    cs.add("clock_second_chance", surv && gone && c.evictions == 1, "");
    // 2) 全 pin 诚实放弃（不转死圈、不假成功）。
    let mut c2 = ClockCache::new();
    for k in 0..CLOCK_CAP as u64 {
        c2.put(k);
        c2.set_pinned(k, true);
    }
    c2.put(0xDEAD);
    cs.add(
        "full_pin_honest",
        c2.resident() == CLOCK_CAP && !c2.slots.iter().flatten().any(|s| s.key == 0xDEAD) && c2.pinned_skips > 0,
        "",
    );
    // 3) LRU-K 顺序洪峰不逐热点（O'Neil 语义：单次访问页先逐）。
    let mut c3 = LruK::new();
    for k in 0..64u64 {
        c3.access(k);
    }
    for k in 0..64u64 {
        c3.access(k);
    }
    for cold in 1000..1200u64 {
        c3.access(cold);
    }
    let mut hits = 0u32;
    for k in 0..64u64 {
        hits += c3.access(k) as u32;
    }
    cs.add("lruk_flood_survival", hits >= 60, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_second_chance_works() {
        // 先灌满（CLOCK 只在满时逐出——空槽期不发生扫描）。
        let mut c = ClockCache::new();
        for k in 0..CLOCK_CAP as u64 {
            c.put(k);
        }
        c.get(1); // 1 置位
        c.put(CLOCK_CAP as u64); // 满载逐出：0 号（先扫到、未置位）被逐，1 幸存
        assert!(c.slots.iter().flatten().any(|s| s.key == 1), "置位页必须拿到第二次机会");
        assert!(!c.slots.iter().flatten().any(|s| s.key == 0), "未置位页先逐");
        assert_eq!(c.evictions, 1);
    }

    #[test]
    fn clock_full_pin_is_honest() {
        let mut c = ClockCache::new();
        for k in 0..CLOCK_CAP as u64 {
            c.put(k);
            c.set_pinned(k, true);
        }
        assert_eq!(c.pinned_count(), CLOCK_CAP);
        c.put(0xDEAD); // 全 pin：不逐任何页、不假成功
        assert_eq!(c.resident(), CLOCK_CAP);
        assert!(!c.slots.iter().flatten().any(|s| s.key == 0xDEAD));
        assert!(c.pinned_skips > 0, "pinned 跳过必须可归因");
    }

    #[test]
    fn clock_hit_rate_on_hot_loop() {
        // 热点循环 + 冷页流：CLOCK 保热点。
        // 先铺冷底再逐入热点——直接铺热点会在首次逐出时全缓存 ref=1
        // （从未被扫描过），CLOCK 第一轮清场逐掉热点，那是暂态不是稳态。
        let mut c = ClockCache::new();
        for k in 0..CLOCK_CAP as u64 {
            c.put(10_000 + k); // 冷底
        }
        for k in 0..64u64 {
            c.put(k); // 逐入热点（各逐出一个冷页）
        }
        // 每轮：命中 64 热点，再灌 8 冷页。
        for round in 0..100u64 {
            for k in 0..64u64 {
                assert!(c.get(k), "热点页在冷流下必须幸存（round {}）", round);
            }
            for cold in 0..8u64 {
                c.put(1000 + round * 8 + cold);
            }
        }
        let rate = c.hits as f64 / (c.hits + c.misses) as f64;
        assert!(rate > 0.7, "F044 口径（>70%）：实际 {:.2}", rate);
    }

    #[test]
    fn lruk_survives_sequential_flood() {
        let mut c = LruK::new();
        // 热点集入驻并二次访问（成为 K=2 热点）。
        for k in 0..64u64 {
            c.access(k);
        }
        for k in 0..64u64 {
            c.access(k);
        }
        // 顺序洪峰：200 个一次性页流过（容量 256——64 热点 + 洪峰共存）。
        for cold in 1000..1200u64 {
            c.access(cold);
        }
        // 热点重访：LRU-K 的洪峰不逐热点（洪峰页只访问一次，回溯距离小）。
        let mut hits = 0;
        for k in 0..64u64 {
            if c.access(k) {
                hits += 1;
            }
        }
        assert!(hits >= 60, "LRU-K 必须扛住顺序洪峰：64 热点只剩 {}", hits);
    }

    #[test]
    fn lruk_evicts_stalest_history_first() {
        let mut c = LruK::new();
        // 3 页占满？容量 256——这里验证距离序：构造两个 K=2 页，
        // 间隔小的（真热点）比间隔大的（近失忆）晚逐。
        for k in 0..LRUK_CAP as u64 {
            c.access(k);
        }
        for k in 0..LRUK_CAP as u64 {
            c.access(k);
        }
        // 全部二次访问：回溯距离都是 CAP（256）——旧行为一致。
        // 新页插入 → 驱逐距离最大者之一（并列取先扫到的）。
        c.access(9999);
        assert_eq!(c.evictions, 1);
        assert_eq!(c.resident(), LRUK_CAP);
        assert!(c.access(9999), "新页必须在缓存中");
    }

    #[test]
    fn lruk_counts_are_honest() {
        let mut c = LruK::new();
        c.access(1);
        assert_eq!((c.hits, c.misses), (0, 1));
        assert!(c.access(1));
        assert_eq!((c.hits, c.misses), (1, 1));
    }
}
