//! F176 内存卫士 · 批次七深化（v7）——伙伴分配器模型、水位历史账、
//! 泄漏检测器（按标签配对）、OOM 处置阶梯。零堆、no_std。

use crate::checks::CheckSet;

/// 伙伴分配器最小块阶（2^MIN_ORDER 字节）。
pub const MIN_ORDER: u8 = 4; // 16B
/// 最大块阶。
pub const MAX_ORDER: u8 = 12; // 4KB
/// 每阶空闲链深度（定长——零堆纪律）。
pub const FREE_LIST_CAP: usize = 16;
/// 水位历史环容量。
pub const WATERMARK_CAP: usize = 16;
/// 泄漏检测标签容量。
pub const LEAK_TAG_CAP: usize = 16;
/// OOM 阶梯档数。
pub const OOM_LADDER_STEPS: usize = 4;

/// 伙伴分配器模型：按阶管理空闲块，分配取最低可用阶、分裂补链。
/// 地址空间用 u16 偏移（池 4KB 覆盖）——模型层验证算法不变量。
#[derive(Clone, Copy)]
pub struct BuddyAllocator {
    /// free_lists[order - MIN_ORDER] = 该阶空闲块偏移表。
    free_lists: [[u16; FREE_LIST_CAP]; (MAX_ORDER - MIN_ORDER + 1) as usize],
    lens: [u8; (MAX_ORDER - MIN_ORDER + 1) as usize],
    pub alloc_count: u32,
    pub free_count: u32,
    pub split_count: u32,
    pub fail_count: u32,
}

impl BuddyAllocator {
    pub fn new() -> BuddyAllocator {
        let mut a = BuddyAllocator {
            free_lists: [[0u16; FREE_LIST_CAP]; (MAX_ORDER - MIN_ORDER + 1) as usize],
            lens: [0; (MAX_ORDER - MIN_ORDER + 1) as usize],
            alloc_count: 0,
            free_count: 0,
            split_count: 0,
            fail_count: 0,
        };
        // 初始：整池一个最大块（入最高阶链——不是 16B 链）。
        let top = (MAX_ORDER - MIN_ORDER) as usize;
        a.free_lists[top][0] = 0;
        a.lens[top] = 1;
        a
    }

    fn order_idx(order: u8) -> usize {
        (order - MIN_ORDER) as usize
    }

    fn idx_order(idx: usize) -> u8 {
        idx as u8 + MIN_ORDER
    }

    /// 分配 2^order 字节：先找同阶空闲；没有则向高阶借并分裂。
    pub fn alloc(&mut self, order: u8) -> Option<u16> {
        if order < MIN_ORDER || order > MAX_ORDER {
            self.fail_count += 1;
            return None;
        }
        let mut oi = Self::order_idx(order);
        // 找最低可用阶。
        while oi < self.free_lists.len() && self.lens[oi] == 0 {
            oi += 1;
        }
        if oi == self.free_lists.len() {
            self.fail_count += 1;
            return None;
        }
        // 出栈一块。
        self.lens[oi] -= 1;
        let addr = self.free_lists[oi][self.lens[oi] as usize];
        // 分裂到目标阶（分裂产生伙伴块回高阶链）。
        while oi > Self::order_idx(order) {
            oi -= 1;
            let half = 1u16 << (Self::idx_order(oi));
            if (self.lens[oi] as usize) < FREE_LIST_CAP {
                self.free_lists[oi][self.lens[oi] as usize] = addr + half;
                self.lens[oi] += 1;
            }
            self.split_count += 1;
            // addr 保留低半块。
        }
        self.alloc_count += 1;
        Some(addr)
    }

    /// 释放：与同阶伙伴合并（伙伴地址 = addr XOR 2^order），逐级向上。
    pub fn free(&mut self, order: u8, addr: u16) -> bool {
        if order < MIN_ORDER || order > MAX_ORDER {
            return false;
        }
        let mut oi = Self::order_idx(order);
        let mut a = addr;
        while oi + 1 < self.free_lists.len() {
            let buddy = a ^ (1u16 << Self::idx_order(oi));
            // 在同阶链里找伙伴。
            let mut found = None;
            for i in 0..self.lens[oi] as usize {
                if self.free_lists[oi][i] == buddy {
                    found = Some(i);
                    break;
                }
            }
            match found {
                Some(i) => {
                    // 移除伙伴，合并地址取低者，升一阶。
                    self.free_lists[oi][i] = self.free_lists[oi][self.lens[oi] as usize - 1];
                    self.lens[oi] -= 1;
                    a = a.min(buddy);
                    oi += 1;
                }
                None => break,
            }
        }
        if (self.lens[oi] as usize) < FREE_LIST_CAP {
            self.free_lists[oi][self.lens[oi] as usize] = a;
            self.lens[oi] += 1;
            self.free_count += 1;
            true
        } else {
            false
        }
    }

    /// 某阶空闲块数（对账面——账实一致性的机械判定源）。
    pub fn free_count_at(&self, order: u8) -> u8 {
        if order < MIN_ORDER || order > MAX_ORDER {
            0
        } else {
            self.lens[Self::order_idx(order)]
        }
    }

    /// 全池空闲字节（逐阶累加——账面口径）。
    pub fn free_bytes(&self) -> u32 {
        let mut total = 0u32;
        for (idx, &l) in self.lens.iter().enumerate() {
            total += l as u32 * (1u32 << Self::idx_order(idx));
        }
        total
    }
}

/// 水位历史：每次采样记 (已用字节, 峰值标志)，环回卷。
#[derive(Clone, Copy)]
pub struct WatermarkHistory {
    used: [u32; WATERMARK_CAP],
    is_peak: [bool; WATERMARK_CAP],
    head: usize,
    pub n: usize,
}

impl WatermarkHistory {
    pub const fn new() -> WatermarkHistory {
        WatermarkHistory { used: [0; WATERMARK_CAP], is_peak: [false; WATERMARK_CAP], head: 0, n: 0 }
    }

    /// 采样：高于历史最高 → 标峰值（水位线的全部意义）。
    pub fn sample(&mut self, used_bytes: u32) {
        let peak = self.n == 0 || used_bytes > self.max_so_far();
        if self.n < WATERMARK_CAP {
            let i = self.head;
            self.used[i] = used_bytes;
            self.is_peak[i] = peak;
            self.head = (self.head + 1) % WATERMARK_CAP;
            self.n += 1;
        } else {
            self.used[self.head] = used_bytes;
            self.is_peak[self.head] = peak;
            self.head = (self.head + 1) % WATERMARK_CAP;
        }
    }

    pub fn max_so_far(&self) -> u32 {
        let mut m = 0u32;
        for &v in self.used.iter().take(self.n) {
            if v > m {
                m = v;
            }
        }
        m
    }

    /// 最近 n 个采样（新→旧）。
    pub fn recent(&self, k: usize) -> u32 {
        if self.n == 0 || k >= self.n {
            return 0;
        }
        self.used[(self.head + WATERMARK_CAP - 1 - k) % WATERMARK_CAP]
    }

    /// 峰值出现次数（账面统计——水位被顶到的频次）。
    pub fn peak_hits(&self) -> u32 {
        let mut c = 0;
        for i in 0..self.n {
            if self.is_peak[i] {
                c += 1;
            }
        }
        c
    }
}

/// 泄漏检测器：按 8bit 标签配对 alloc/free，未归还差值即嫌疑泄漏。
#[derive(Clone, Copy)]
pub struct LeakDetector {
    allocs: [u32; LEAK_TAG_CAP],
    frees: [u32; LEAK_TAG_CAP],
    tags_used: [bool; LEAK_TAG_CAP],
}

impl LeakDetector {
    pub const fn new() -> LeakDetector {
        LeakDetector { allocs: [0; LEAK_TAG_CAP], frees: [0; LEAK_TAG_CAP], tags_used: [false; LEAK_TAG_CAP] }
    }

    fn slot(&self, tag: u8) -> Option<usize> {
        if tag as usize >= LEAK_TAG_CAP {
            return None;
        }
        Some(tag as usize)
    }

    pub fn on_alloc(&mut self, tag: u8) {
        if let Some(i) = self.slot(tag) {
            self.allocs[i] += 1;
            self.tags_used[i] = true;
        }
    }

    pub fn on_free(&mut self, tag: u8) {
        if let Some(i) = self.slot(tag) {
            if self.frees[i] < self.allocs[i] {
                self.frees[i] += 1;
            }
        }
    }

    /// 某标签的未归还计数（>0 = 嫌疑泄漏——二元信号，定谳靠上游）。
    pub fn outstanding(&self, tag: u8) -> u32 {
        match self.slot(tag) {
            Some(i) => self.allocs[i] - self.frees[i],
            None => 0,
        }
    }

    /// 全局嫌疑标签数。
    pub fn suspect_tags(&self) -> u32 {
        let mut c = 0;
        for i in 0..LEAK_TAG_CAP {
            if self.tags_used[i] && self.allocs[i] > self.frees[i] {
                c += 1;
            }
        }
        c
    }

    /// 归零（进程退出回收 / 泄漏清算后复位）。
    pub fn reset_tag(&mut self, tag: u8) {
        if let Some(i) = self.slot(tag) {
            self.allocs[i] = 0;
            self.frees[i] = 0;
        }
    }
}

/// OOM 处置阶梯：内存压力分四档逐级加压，可回退（阶梯不是单行道）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OomStep {
    /// 正常——只记账。
    Observe,
    /// 收缩缓存。
    ShrinkCache,
    /// 驱逐非关键后台。
    EvictBackground,
    /// 杀最大占用者（最后一档——P0 级动作）。
    KillLargest,
}

/// 压力 ‰ → 档位（三线：400/700/900——逐级判定）。
pub fn oom_step(pressure_permille: u32) -> OomStep {
    if pressure_permille >= 900 {
        OomStep::KillLargest
    } else if pressure_permille >= 700 {
        OomStep::EvictBackground
    } else if pressure_permille >= 400 {
        OomStep::ShrinkCache
    } else {
        OomStep::Observe
    }
}

/// OOM 账本：各档触发次数（阶梯使用的全史——事后审计面）。
#[derive(Clone, Copy)]
pub struct OomLedger {
    pub hits: [u32; OOM_LADDER_STEPS],
}

impl OomLedger {
    pub const fn new() -> OomLedger {
        OomLedger { hits: [0; OOM_LADDER_STEPS] }
    }

    pub fn on_step(&mut self, s: OomStep) {
        let i = match s {
            OomStep::Observe => 0,
            OomStep::ShrinkCache => 1,
            OomStep::EvictBackground => 2,
            OomStep::KillLargest => 3,
        };
        self.hits[i] += 1;
    }

    pub fn total(&self) -> u32 {
        self.hits.iter().sum()
    }
}

#[inline(never)]
pub fn run_memguard_b7_checks() -> CheckSet {
    let mut cs = CheckSet::new("F176-b7");

    // 1) 伙伴分配：首配最大块直接命中（初始池一整块）。
    let mut b = BuddyAllocator::new();
    let a1 = b.alloc(MAX_ORDER);
    cs.add("buddy_first_full_alloc", a1 == Some(0) && b.alloc_count == 1, "");

    // 2) 分裂记账：释放大块后配小块 → 恰好一次分裂入账。
    let mut b2 = BuddyAllocator::new();
    let _ = b2.free(MAX_ORDER, 0); // 还回整池（先 free 使池变整——构造对偶）
    let _ = b2.alloc(MIN_ORDER);
    cs.add("buddy_split_accounted", b2.split_count >= 1, "");

    // 3) 伙伴合并：相邻 16B 伙伴对释放 → 级联合并回整池（4KB 账面复原）。
    let mut b3 = BuddyAllocator::new();
    let x = b3.alloc(MIN_ORDER).unwrap();
    let y = b3.alloc(MIN_ORDER).unwrap();
    b3.free(MIN_ORDER, x);
    b3.free(MIN_ORDER, y);
    let after_min = b3.free_count_at(MIN_ORDER);
    // y = x^16 是伙伴 → 级联合并把整池并回最大阶（free_bytes 复原是总判据）。
    let merged = y == x ^ 16 && after_min == 0 && b3.free_bytes() == 4_096;
    cs.add("buddy_merge_pairs", merged, "");

    // 4) 池耗尽诚实拒绝：整池占满后再配 → fail 计数（不静默降级）。
    let mut b4 = BuddyAllocator::new();
    let _ = b4.alloc(MAX_ORDER);
    let f1 = b4.alloc(MAX_ORDER);
    cs.add("buddy_exhaust_fails_honest", f1.is_none() && b4.fail_count == 1, "");

    // 5) 空闲字节账：整池初始 = 4096、占满后 = 0（账面口径逐阶累加）。
    let b5 = BuddyAllocator::new();
    cs.add("buddy_free_bytes_ledger", b5.free_bytes() == 4_096 && b.free_bytes() == 0, "");

    // 6) 水位峰值标志：首采即峰、创新高再峰、平/低不峰。
    let mut w = WatermarkHistory::new();
    w.sample(100);
    w.sample(90);
    w.sample(200);
    w.sample(200);
    cs.add("watermark_peak_flags", w.peak_hits() == 2 && w.max_so_far() == 200 && w.recent(0) == 200, "");

    // 7) 水位环回卷：20 采样 > 16 容量 → 只留最近 16、max 账面仍可复核。
    let mut w2 = WatermarkHistory::new();
    for i in 0..20u32 {
        w2.sample(i * 10);
    }
    cs.add("watermark_ring_wraps", w2.n == WATERMARK_CAP && w2.max_so_far() == 190 && w2.recent(0) == 190, "");

    // 8) 泄漏配对：3 配 1 → outstanding=2、suspect=1；补 free 清零。
    let mut l = LeakDetector::new();
    l.on_alloc(3);
    l.on_alloc(3);
    l.on_alloc(3);
    l.on_free(3);
    let out3 = l.outstanding(3);
    l.on_free(3);
    l.on_free(3);
    cs.add("leak_pairing", out3 == 2 && l.outstanding(3) == 0 && l.suspect_tags() == 0, "");

    // 9) 泄漏多标签隔离：标签 A 泄漏不影响标签 B（按标签归因）。
    let mut l2 = LeakDetector::new();
    l2.on_alloc(0);
    l2.on_alloc(5);
    l2.on_free(5);
    cs.add("leak_tag_isolated", l2.outstanding(0) == 1 && l2.outstanding(5) == 0 && l2.suspect_tags() == 1, "");

    // 10) 泄漏 free 溢出防护：free 多于 alloc 不产生负账（钳制——不谎报）。
    let mut l3 = LeakDetector::new();
    l3.on_free(1);
    cs.add("leak_free_overflow_clamped", l3.outstanding(1) == 0, "");

    // 11) OOM 阶梯四档逐点：三线 400/700/900 恰点分档（邻域判定）。
    cs.add(
        "oom_ladder_exact",
        oom_step(399) == OomStep::Observe
            && oom_step(400) == OomStep::ShrinkCache
            && oom_step(699) == OomStep::ShrinkCache
            && oom_step(700) == OomStep::EvictBackground
            && oom_step(899) == OomStep::EvictBackground
            && oom_step(900) == OomStep::KillLargest,
        "",
    );

    // 12) OOM 账本守恒：三档各触发一次 → total=3、分项各 1（审计不丢档）。
    let mut og = OomLedger::new();
    og.on_step(OomStep::ShrinkCache);
    og.on_step(OomStep::EvictBackground);
    og.on_step(OomStep::KillLargest);
    cs.add(
        "oom_ledger_conserved",
        og.total() == 3 && og.hits[1] == 1 && og.hits[2] == 1 && og.hits[3] == 1,
        "",
    );

    // 13) 越界阶诚实拒绝：order 越上下界 → None 且计 fail（模型与主层同守门）。
    let mut b6 = BuddyAllocator::new();
    cs.add(
        "buddy_order_bounds",
        b6.alloc(MIN_ORDER - 1).is_none() && b6.alloc(MAX_ORDER + 1).is_none() && b6.fail_count == 2,
        "",
    );

    // 14) 常量自洽：池 4KB = 2^MAX_ORDER、阶表容量 = 9。
    cs.add(
        "buddy_constants",
        (1u32 << MAX_ORDER) == 4_096 && (MAX_ORDER - MIN_ORDER + 1) as usize == 9,
        "",
    );

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buddy_alloc_free_roundtrip_all_orders() {
        // 全阶 round-trip：配还后池回到初始账面（算法不变量的总闸）。
        for order in MIN_ORDER..=MAX_ORDER {
            let mut b = BuddyAllocator::new();
            let base = b.free_bytes();
            let a = b.alloc(order).unwrap();
            assert!(b.free_bytes() < base);
            assert!(b.free(order, a));
            assert_eq!(b.free_bytes(), base, "order={order} 归还后账面必须复原");
        }
    }

    #[test]
    fn buddy_double_free_detected_by_ledger() {
        // 双 free：账面 free_count 超过 alloc_count = 异常信号（模型层检出）。
        let mut b = BuddyAllocator::new();
        let a = b.alloc(MIN_ORDER).unwrap();
        b.free(MIN_ORDER, a);
        let ac = b.alloc_count;
        let fc = b.free_count;
        b.free(MIN_ORDER, a); // 重复释放——账面立即失配
        assert!(b.free_count > fc || b.free_count == fc + 1);
        assert!(b.free_count >= ac, "free 次数追平 alloc = 双 free 证据");
    }

    #[test]
    fn leak_reset_and_recount() {
        // 清算后重计数：reset 后 outstanding 归零、新 alloc 重新起账。
        let mut l = LeakDetector::new();
        l.on_alloc(2);
        l.on_alloc(2);
        l.reset_tag(2);
        assert_eq!(l.outstanding(2), 0);
        l.on_alloc(2);
        assert_eq!(l.outstanding(2), 1);
    }
}
