//! F184 热插拔 · 批次七深化（v7）——卷标签缓存、弹出重试阶梯、
//! 挂载点分配器、设备到达去抖账。零堆、no_std。

use crate::checks::CheckSet;

/// 标签缓存容量。
pub const LABEL_CACHE_CAP: usize = 16;
/// 弹出重试上限。
pub const EJECT_RETRY_MAX: u32 = 3;
/// 重试间隔（ms——指数退避起点）。
pub const EJECT_RETRY_BASE_MS: u32 = 1_000;
/// 挂载点池容量（V0..V15）。
pub const MOUNT_POOL_CAP: usize = 16;
/// 到达去抖窗（ms——同设备抖动只报一次）。
pub const ARRIVAL_DEBOUNCE_MS: u32 = 500;

/// 卷标签缓存：设备序号 → 卷标（读盘面开销大——命中就免读）。
#[derive(Clone, Copy)]
pub struct LabelCache {
    dev_ids: [Option<u32>; LABEL_CACHE_CAP],
    labels: [[u8; 12]; LABEL_CACHE_CAP],
    lens: [u8; LABEL_CACHE_CAP],
    pub hits: u32,
    pub misses: u32,
}

impl LabelCache {
    pub const fn new() -> LabelCache {
        LabelCache { dev_ids: [None; LABEL_CACHE_CAP], labels: [[0; 12]; LABEL_CACHE_CAP], lens: [0; LABEL_CACHE_CAP], hits: 0, misses: 0 }
    }

    /// 查缓存：命中返回 Some(标签)。
    pub fn lookup(&self, dev_id: u32) -> Option<&[u8]> {
        for i in 0..LABEL_CACHE_CAP {
            if self.dev_ids[i] == Some(dev_id) {
                return Some(&self.labels[i][..self.lens[i] as usize]);
            }
        }
        None
    }

    /// 记录查询结果（miss 时由调用方读盘后回填）。
    pub fn fill(&mut self, dev_id: u32, label: &[u8]) -> bool {
        if label.len() > 12 || label.is_empty() {
            return false;
        }
        // 已存在 → 原位刷新。
        for i in 0..LABEL_CACHE_CAP {
            if self.dev_ids[i] == Some(dev_id) {
                self.labels[i][..label.len()].copy_from_slice(label);
                self.lens[i] = label.len() as u8;
                return true;
            }
        }
        // 新条目：找空位；满容驱逐 index 0（最旧）。
        let slot = (0..LABEL_CACHE_CAP).find(|&i| self.dev_ids[i].is_none()).unwrap_or_else(|| {
            for j in 1..LABEL_CACHE_CAP {
                self.dev_ids[j - 1] = self.dev_ids[j];
                self.labels[j - 1] = self.labels[j];
                self.lens[j - 1] = self.lens[j];
            }
            LABEL_CACHE_CAP - 1
        });
        self.dev_ids[slot] = Some(dev_id);
        self.labels[slot][..label.len()].copy_from_slice(label);
        self.lens[slot] = label.len() as u8;
        true
    }

    /// 带账查询：命中计 hit，未命中计 miss（命中率对账面）。
    pub fn lookup_accounted(&mut self, dev_id: u32) -> Option<&[u8]> {
        let found = self.lookup(dev_id).is_some();
        if found {
            self.hits += 1;
        } else {
            self.misses += 1;
        }
        self.lookup(dev_id)
    }

    /// 命中率 ‰。
    pub fn hit_rate_permille(&self) -> Option<u32> {
        let total = self.hits + self.misses;
        if total == 0 {
            return None;
        }
        Some(self.hits * 1_000 / total)
    }
}

/// 弹出重试阶梯：失败重试，间隔指数退避 1s/2s/4s，三败升级人工。
#[derive(Clone, Copy)]
pub struct EjectRetry {
    pub attempts: u32,
    pub escalated: bool,
}

impl EjectRetry {
    pub const fn new() -> EjectRetry {
        EjectRetry { attempts: 0, escalated: false }
    }

    /// 记一次失败：返回 Some(下次重试间隔 ms)；三败升级 → None。
    pub fn on_fail(&mut self) -> Option<u32> {
        self.attempts += 1;
        if self.attempts >= EJECT_RETRY_MAX {
            self.escalated = true;
            return None;
        }
        Some(EJECT_RETRY_BASE_MS << (self.attempts - 1))
    }

    pub fn on_success(&mut self) {
        self.attempts = 0;
        self.escalated = false;
    }
}

/// 挂载点分配器：V0..V15 位图池，分配取最低空闲、释放归还。
#[derive(Clone, Copy)]
pub struct MountPool {
    used: u32,
    pub allocations: u32,
}

impl MountPool {
    pub const fn new() -> MountPool {
        MountPool { used: 0, allocations: 0 }
    }

    /// 分配最低空闲槽（0..16），满 → None。
    pub fn alloc(&mut self) -> Option<usize> {
        if self.used == 0xFFFF {
            return None;
        }
        let slot = self.used.trailing_ones() as usize;
        self.used |= 1 << slot;
        self.allocations += 1;
        Some(slot)
    }

    /// 释放（未持有的槽释放 = 编程错误信号——拒绝）。
    pub fn release(&mut self, slot: usize) -> bool {
        if slot >= MOUNT_POOL_CAP || self.used & (1 << slot) == 0 {
            return false;
        }
        self.used &= !(1 << slot);
        true
    }

    pub fn used_count(&self) -> u32 {
        self.used.count_ones()
    }

    pub fn full(&self) -> bool {
        self.used == 0xFFFF
    }
}

/// 设备到达去抖：同设备在窗内重复到达事件合并（USB 抖动只报一次）。
#[derive(Clone, Copy)]
pub struct ArrivalDebouncer {
    last_ms: [u32; LABEL_CACHE_CAP],
    dev_ids: [Option<u32>; LABEL_CACHE_CAP],
    pub n: usize,
    pub merged: u32,
}

impl ArrivalDebouncer {
    pub const fn new() -> ArrivalDebouncer {
        ArrivalDebouncer { last_ms: [0; LABEL_CACHE_CAP], dev_ids: [None; LABEL_CACHE_CAP], n: 0, merged: 0 }
    }

    /// 到达事件裁决：true = 应上报（新设备或过窗）；false = 合并。
    pub fn arrival(&mut self, dev_id: u32, now_ms: u32) -> bool {
        for i in 0..self.n {
            if self.dev_ids[i] == Some(dev_id) {
                if now_ms.saturating_sub(self.last_ms[i]) < ARRIVAL_DEBOUNCE_MS {
                    self.merged += 1;
                    return false;
                }
                self.last_ms[i] = now_ms;
                return true;
            }
        }
        if self.n < LABEL_CACHE_CAP {
            self.dev_ids[self.n] = Some(dev_id);
            self.last_ms[self.n] = now_ms;
            self.n += 1;
        }
        true // 新设备必报
    }

    /// 设备移除清账（设备走了，去抖记忆也该走）。
    pub fn remove(&mut self, dev_id: u32) -> bool {
        for i in 0..self.n {
            if self.dev_ids[i] == Some(dev_id) {
                for j in i..self.n - 1 {
                    self.dev_ids[j] = self.dev_ids[j + 1];
                    self.last_ms[j] = self.last_ms[j + 1];
                }
                self.dev_ids[self.n - 1] = None;
                self.n -= 1;
                return true;
            }
        }
        false
    }
}

#[inline(never)]
pub fn run_hotplug_b7_checks() -> CheckSet {
    let mut cs = CheckSet::new("F184-b7");

    // 1) 标签缓存：fill 后命中、内容逐字节一致。
    let mut c = LabelCache::new();
    assert!(c.fill(7, b"BACKUP"));
    let got = c.lookup(7).unwrap();
    cs.add("label_cache_hit", got == b"BACKUP", "");

    // 2) 缓存账：1 hit 1 miss → 命中率 500‰；空账 None。
    let mut c2 = LabelCache::new();
    c2.fill(1, b"A");
    let _ = c2.lookup_accounted(1);
    let _ = c2.lookup_accounted(2);
    cs.add(
        "label_cache_rate",
        c2.hit_rate_permille() == Some(500) && LabelCache::new().hit_rate_permille().is_none(),
        "",
    );

    // 3) 标签守门：13B 超长拒、空标签拒（空标 = 读盘失败不是有效卷标）。
    let mut c3 = LabelCache::new();
    cs.add(
        "label_guards",
        !c3.fill(1, &[b'x'; 13]) && !c3.fill(1, b"") && c3.lookup(1).is_none(),
        "",
    );

    // 4) 弹出阶梯：1s → 2s → 升级（三败 None + escalated 旗）。
    let mut e = EjectRetry::new();
    let r1 = e.on_fail();
    let r2 = e.on_fail();
    let r3 = e.on_fail();
    cs.add(
        "eject_backoff_ladder",
        r1 == Some(1_000) && r2 == Some(2_000) && r3.is_none() && e.escalated && e.attempts == 3,
        "",
    );

    // 5) 弹出成功复位：成功后阶梯归零重计（新弹出是新战役）。
    e.on_success();
    let r4 = e.on_fail();
    cs.add("eject_success_resets", r4 == Some(1_000) && !e.escalated, "");

    // 6) 挂载池：顺序分配 0,1,2、释放 1 再分配得 1（最低空闲复用）。
    let mut p = MountPool::new();
    let s0 = p.alloc();
    let s1 = p.alloc();
    let s2 = p.alloc();
    p.release(s1.unwrap());
    let s3 = p.alloc();
    cs.add(
        "mount_lowest_free_reuse",
        s0 == Some(0) && s1 == Some(1) && s2 == Some(2) && s3 == Some(1),
        "",
    );

    // 7) 挂载池满容：16 槽占满 → None；释放持有槽成功、二次释放拒。
    let mut p2 = MountPool::new();
    for _ in 0..MOUNT_POOL_CAP {
        assert!(p2.alloc().is_some());
    }
    let full = p2.full() && p2.alloc().is_none();
    let rel_held = p2.release(5);
    let rel_twice_rejected = !p2.release(5);
    let refill = p2.alloc() == Some(5); // 释放出的槽被复用
    cs.add("mount_full_and_bad_release", full && rel_held && rel_twice_rejected && refill, "");

    // 8) 挂载池守恒：alloc N 次 release N 次 → used_count 归零（账平）。
    let mut p3 = MountPool::new();
    let mut slots = [0usize; MOUNT_POOL_CAP];
    for s in slots.iter_mut() {
        *s = p3.alloc().unwrap();
    }
    for &s in slots.iter() {
        assert!(p3.release(s));
    }
    cs.add("mount_conserved", p3.used_count() == 0 && p3.allocations == MOUNT_POOL_CAP as u32, "");

    // 9) 到达去抖：同设备 100ms 内二至 → 合并；600ms 后再至 → 上报。
    let mut d = ArrivalDebouncer::new();
    let a1 = d.arrival(9, 1_000);
    let a2 = d.arrival(9, 1_100);
    let a3 = d.arrival(9, 1_600);
    cs.add(
        "arrival_debounce",
        a1 && !a2 && a3 && d.merged == 1,
        "",
    );

    // 10) 不同设备互不合并：两设备同刻到达都上报（去抖按设备归因）。
    let mut d2 = ArrivalDebouncer::new();
    let b1 = d2.arrival(1, 500);
    let b2 = d2.arrival(2, 500);
    cs.add("arrival_per_device", b1 && b2, "");

    // 11) 移除清账：remove 后同设备再到达 = 新设备必报（记忆不留鬼）。
    d2.remove(1);
    let b3 = d2.arrival(1, 600);
    cs.add("arrival_remove_forgets", b3 && d2.n == 2, "");

    // 12) 常量自洽：重试 3 次、退避基 1s、去抖 500ms。
    cs.add(
        "b7_constants",
        EJECT_RETRY_MAX == 3 && EJECT_RETRY_BASE_MS == 1_000 && ARRIVAL_DEBOUNCE_MS == 500,
        "",
    );

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eject_backoff_sequence_exact() {
        // 全阶梯序列：1s, 2s, 4s——第 4 败才升级？不：三败即升级（attempts>=3）。
        let mut e = EjectRetry::new();
        assert_eq!(e.on_fail(), Some(1_000));
        assert_eq!(e.on_fail(), Some(2_000));
        assert_eq!(e.on_fail(), None); // 第三败直接升级
        assert!(e.escalated);
    }

    #[test]
    fn label_cache_eviction_keeps_newest() {
        // 16 满驱逐：第 17 设备入账、第 1 设备出账（最旧出）。
        let mut c = LabelCache::new();
        for i in 0..LABEL_CACHE_CAP as u32 {
            assert!(c.fill(i, b"dev"));
        }
        assert!(c.fill(99, b"new"));
        assert!(c.lookup(0).is_none() && c.lookup(99).is_some());
    }

    #[test]
    fn mount_pool_alternating_stress() {
        // 交替分配释放压力：任何时刻 used ≤ 16 且不越界。
        let mut p = MountPool::new();
        for round in 0..10 {
            let s = p.alloc().unwrap();
            assert!(s < MOUNT_POOL_CAP);
            if round % 2 == 1 {
                assert!(p.release(s));
            }
        }
    }
}
