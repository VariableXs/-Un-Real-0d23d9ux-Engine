//! 任务52（AI-P）· 引擎盘 ramcache —— LRU 定容块缓存 + 关机清空断言 + 整盘 hash 零残留。
//!
//! 双域系统里引擎（Windows VM）盘数据在 VARIX 侧的**只读**缓存层：
//!
//! - **只读语义**：缓存永不持脏数据（引擎侧写回走引擎自身管线），淘汰与
//!   清空零回写——"清空即安全"由构造保证，不依赖调用方自觉；
//! - **LRU**：tick 单调计数，命中刷新，满逐出最久未用槽；
//! - **关机清空断言**（任务52 验收）：S5 关机序列 StopServices 步骤挂接
//!   [`RamCache::purge_and_verify`]——清空后对整个数据区 fnv1a64 hash
//!   留证 + 逐字节扫描，任何残留字节返回具名 `Residue`（块号+槽内偏移），
//!   绝不静默；
//! - **配额对接**（任务56 冻结契约）：实现 [`crate::quota::ReclaimStage`]
//!   级1（ReclaimStageId::RamCache）——reclaim 按 LRU 逐出 n 页（4KiB/页
//!   ＝一块），restore 账目回补（数据在盘，miss 重读即回）；stage 侧自持
//!   owed 账本，与 MemWatermark 记账两清，零旁路；
//! - **内核戒律**：目标态 256KiB 数据区 = static .bss（>64KB struct 禁
//!   栈上/Box 中转物化）；泛型 `CAP` 让宿主测试用 16KiB 小实例栈安全。

use crate::drivers::blk::fnv1a64;
use crate::quota::{ReclaimStage, ReclaimStageId};

/// 块大小 = 页大小（回收粒度与页账本同单位）。
pub const BLOCK_SIZE: usize = 4096;
/// 目标态容量：64 块 × 4KiB = 256KiB（kheap slab 边界内的 static .bss）。
pub const TARGET_CAP: usize = 64;

/// 槽元数据。
#[derive(Clone, Copy, PartialEq, Eq)]
struct SlotMeta {
    used: bool,
    /// 引擎盘标识（多盘隔离：SHARED/VHDX 各占键空间）。
    disk: u32,
    /// 盘内块号。
    block: u64,
    /// 最近使用 tick（LRU 序）。
    tick: u64,
}

impl SlotMeta {
    const EMPTY: SlotMeta = SlotMeta { used: false, disk: 0, block: 0, tick: 0 };
}

/// 零残留检出的具名残留（块号 = 槽序，off = 槽内偏移）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Residue {
    pub slot: usize,
    pub off: usize,
    pub byte: u8,
}

/// LRU 定容只读块缓存。
pub struct RamCache<const CAP: usize> {
    slots: [SlotMeta; CAP],
    data: [[u8; BLOCK_SIZE]; CAP],
    tick: u64,
    hits: u64,
    misses: u64,
    evictions: u64,
    purges: u64,
    /// reclaim 借走未还页数（restore 账目两清用；MemWatermark 另有总账）。
    owed: u64,
}

impl<const CAP: usize> RamCache<CAP> {
    pub const fn new() -> Self {
        RamCache {
            slots: [SlotMeta::EMPTY; CAP],
            data: [[0; BLOCK_SIZE]; CAP],
            tick: 0,
            hits: 0,
            misses: 0,
            evictions: 0,
            purges: 0,
            owed: 0,
        }
    }

    fn find(&self, disk: u32, block: u64) -> Option<usize> {
        self.slots.iter().position(|s| s.used && s.disk == disk && s.block == block)
    }

    /// fill 落槽：优先空闲槽；满载才逐出最久未用。
    fn free_or_lru(&self) -> usize {
        if let Some(free) = self.slots.iter().position(|s| !s.used) {
            return free;
        }
        let mut best = 0;
        let mut best_tick = u64::MAX;
        for i in 0..CAP {
            if self.slots[i].tick < best_tick {
                best_tick = self.slots[i].tick;
                best = i;
            }
        }
        best
    }

    /// reclaim 逐出目标：最久未用的**在用**槽（调用方保证 resident>0）。
    fn lru_used(&self) -> usize {
        let mut best = 0;
        let mut best_tick = u64::MAX;
        for i in 0..CAP {
            let s = &self.slots[i];
            if s.used && s.tick < best_tick {
                best_tick = s.tick;
                best = i;
            }
        }
        best
    }

    /// 查缓存：命中刷新 LRU 并返回块引用。
    pub fn get(&mut self, disk: u32, block: u64) -> Option<&[u8; BLOCK_SIZE]> {
        match self.find(disk, block) {
            Some(i) => {
                self.tick += 1;
                self.slots[i].tick = self.tick;
                self.hits += 1;
                Some(&self.data[i])
            }
            None => {
                self.misses += 1;
                None
            }
        }
    }

    /// 灌块：同键覆盖刷新；满则逐出 LRU 槽。返回被逐出的键（无逐出为 None）。
    pub fn fill(&mut self, disk: u32, block: u64, data: &[u8; BLOCK_SIZE]) -> Option<(u32, u64)> {
        let slot = match self.find(disk, block) {
            Some(i) => i,
            None => {
                let victim = self.free_or_lru();
                let out = match self.slots[victim].used {
                    true => {
                        self.evictions += 1;
                        Some((self.slots[victim].disk, self.slots[victim].block))
                    }
                    false => None,
                };
                // 逐出槽清零（只读缓存无回写；零化防旧块数据滞留误导后续 verify）。
                self.data[victim] = [0; BLOCK_SIZE];
                self.slots[victim] = SlotMeta { used: true, disk, block, tick: 0 };
                self.tick += 1;
                self.slots[victim].tick = self.tick;
                self.data[victim] = *data;
                return out;
            }
        };
        self.tick += 1;
        self.slots[slot].tick = self.tick;
        self.data[slot] = *data;
        None
    }

    /// 驻留块数。
    pub fn resident(&self) -> usize {
        self.slots.iter().filter(|s| s.used).count()
    }

    /// 命中/未命中/逐出/清空次数。
    pub fn stats(&self) -> (u64, u64, u64, u64) {
        (self.hits, self.misses, self.evictions, self.purges)
    }

    /// 整区 fnv1a64 hash（关机证据打点：purge 前后 hash 变化可复核）。
    pub fn hash_all(&self) -> u64 {
        // 元数据（used/disk/block）+ 数据区逐槽级联 FNV——顺序敏感，
        // 不拼接 256KiB 大缓冲（禁堆/栈中转）。
        let mut meta: alloc::vec::Vec<u8> = alloc::vec::Vec::new();
        for i in 0..CAP {
            if self.slots[i].used {
                meta.extend_from_slice(&self.slots[i].disk.to_le_bytes());
                meta.extend_from_slice(&self.slots[i].block.to_le_bytes());
            }
        }
        let mut h: u64 = fnv1a64(&meta);
        for chunk in self.data.iter() {
            h = h.rotate_left(1) ^ fnv1a64(chunk);
        }
        h
    }

    /// 逐字节扫描残留（不修改状态）。
    pub fn verify_zero(&self) -> Option<Residue> {
        for i in 0..CAP {
            for (off, &b) in self.data[i].iter().enumerate() {
                if b != 0 {
                    return Some(Residue { slot: i, off, byte: b });
                }
            }
        }
        None
    }

    /// 清空全部槽与数据区（volatile 逐字节零化），返回清除的驻留块数。
    pub fn purge_all(&mut self) -> usize {
        let n = self.resident();
        for i in 0..CAP {
            let d = &mut self.data[i] as *mut [u8; BLOCK_SIZE];
            // SAFETY: 目标态单核 syscall/关机语境独占本实例；volatile 防
            // 编译器把「全零写后不再读」优化掉（零化必须真实发生）。
            unsafe {
                for off in 0..BLOCK_SIZE {
                    core::ptr::write_volatile(&mut (*d)[off], 0u8);
                }
            }
            self.slots[i] = SlotMeta::EMPTY;
        }
        self.purges += 1;
        n
    }

    /// 关机清空断言（任务52 验收）：purge 后 hash 留证 + 逐字节扫描，
    /// 残留即 Err。S5 序列 StopServices 步骤挂接。
    pub fn purge_and_verify(&mut self) -> Result<usize, Residue> {
        let cleared = self.purge_all();
        let h = self.hash_all();
        if let Some(r) = self.verify_zero() {
            crate::kerror!(
                "ramcache: shutdown verify FAILED slot={} off={} byte={:#04x} hash={:#018x}",
                r.slot,
                r.off,
                r.byte,
                h
            );
            return Err(r);
        }
        crate::kinfo!("ramcache: shutdown purge cleared={} blocks hash={:#018x} — 零残留", cleared, h);
        Ok(cleared)
    }
}

impl<const CAP: usize> Default for RamCache<CAP> {
    fn default() -> Self {
        Self::new()
    }
}

/// 任务56 冻结契约对接：级1（RamCache）。
/// reclaim = LRU 逐出（每页一块，4KiB），restore = 账目回补（数据在盘）。
impl<const CAP: usize> ReclaimStage for RamCache<CAP> {
    fn id(&self) -> ReclaimStageId {
        ReclaimStageId::RamCache
    }

    fn as_str(&self) -> &'static str {
        "ram"
    }

    fn reclaim(&mut self, pages: u64) -> u64 {
        let mut n = 0u64;
        while n < pages && self.resident() > 0 {
            let slot = self.lru_used();
            self.data[slot] = [0; BLOCK_SIZE];
            self.slots[slot] = SlotMeta::EMPTY;
            self.evictions += 1;
            n += 1;
        }
        self.owed += n;
        n
    }

    fn restore(&mut self, pages: u64) -> u64 {
        let n = pages.min(self.owed);
        self.owed -= n;
        n
    }
}

// ---------------------------------------------------------------------------
// 目标态：static .bss 实例 + 实机探针
// ---------------------------------------------------------------------------

#[cfg(all(target_arch = "x86_64", target_os = "none"))]
pub mod target {
    use super::{RamCache, TARGET_CAP};

    static mut CACHE: RamCache<TARGET_CAP> = RamCache::new();

    /// 全局实例（目标态单核；关机序列与探针在同一执行流上，无并发）。
    pub fn cache() -> &'static mut RamCache<TARGET_CAP> {
        // SAFETY: 目标态单核演示语境，访问点互不重入（探针/关机序列）。
        unsafe { &mut *(&raw mut CACHE) }
    }

    /// 实机探针（ring3 演示链挂接）：填块→命中→LRU 逐出→回收账本→关机清空断言。
    pub fn target_probe() {
        let c = cache();
        let mut blk = [0u8; super::BLOCK_SIZE];
        // ① 填 3 块（disk=7 引擎盘，块号 100..102，内容含标记字节）。
        for b in 100..103u64 {
            blk[0] = b as u8;
            blk[1] = 0x5C;
            blk[4095] = (b as u8) ^ 0xA5;
            c.fill(7, b, &blk);
        }
        let resident_after_fill = c.resident();
        // ② 命中 + 未命中。
        let hit_ok = c.get(7, 101).map(|d| d[1] == 0x5C).unwrap_or(false);
        let miss_ok = c.get(7, 999).is_none();
        // ③ LRU：填满 64 槽（100..163 = 64 块，无逐出）→ 刷新 100 →
        //    再填一块 → 槽满逐出 tick 最小者 = 102（101 已被 get 刷新）。
        for b in 103..164u64 {
            blk[0] = b as u8;
            let _ = c.fill(7, b, &blk);
        }
        let _ = c.get(7, 100);
        blk[0] = 200;
        let evicted = c.fill(7, 200, &blk);
        let lru_ok = evicted == Some((7, 102)) && c.get(7, 100).is_some() && c.get(7, 102).is_none();
        // ④ 回收账本：reclaim 2 页 → resident-2；restore 全额两清。
        let before = c.resident() as u64;
        let got = crate::quota::ReclaimStage::reclaim(c, 2);
        let after = c.resident() as u64;
        let back = crate::quota::ReclaimStage::restore(c, 9);
        let ledger_ok = got == 2 && after + 2 == before && back == 2;
        // ⑤ 关机清空断言：塞脏 → purge_and_verify → 零残留。
        blk[0] = 0xEE;
        let _ = c.fill(7, 201, &blk);
        let shutdown = c.purge_and_verify();
        let (h, m, e, p) = c.stats();
        let ok = resident_after_fill == 3
            && hit_ok
            && miss_ok
            && lru_ok
            && ledger_ok
            && shutdown.is_ok();
        crate::kinfo!(
            "ramcache-probe: fill=3 hit={} miss={} lru_evict={:?} ledger={} shutdown={:?} stats(h={}/m={}/e={}/p={}) verdict={}",
            hit_ok,
            miss_ok,
            evicted,
            ledger_ok,
            shutdown,
            h,
            m,
            e,
            p,
            if ok { "ok" } else { "FAIL" }
        );
    }
}

// ---------------------------------------------------------------------------
// 宿主测试（CAP=2/4 小实例，16KiB 级，栈安全）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn blk(mark: u8) -> [u8; BLOCK_SIZE] {
        let mut b = [0u8; BLOCK_SIZE];
        b[0] = mark;
        b[BLOCK_SIZE - 1] = mark ^ 0xFF;
        b
    }

    #[test]
    fn lru_evicts_least_recently_used() {
        let mut c = RamCache::<2>::new();
        assert_eq!(c.fill(1, 10, &blk(1)), None);
        assert_eq!(c.fill(1, 11, &blk(2)), None);
        // 命中 10 刷新 → 11 成为最旧。
        assert!(c.get(1, 10).is_some());
        assert_eq!(c.fill(1, 12, &blk(3)), Some((1, 11)));
        assert!(c.get(1, 10).is_some(), "被刷新的 10 必须存活");
        assert!(c.get(1, 11).is_none(), "最久未用的 11 必须被逐出");
        assert!(c.get(1, 12).is_some());
    }

    #[test]
    fn same_key_overwrite_refreshes_without_eviction() {
        let mut c = RamCache::<2>::new();
        c.fill(1, 10, &blk(1));
        c.fill(1, 11, &blk(2));
        // 同键覆盖：不逐出、驻留不变。
        assert_eq!(c.fill(1, 10, &blk(9)), None);
        assert_eq!(c.resident(), 2);
        assert_eq!(c.get(1, 10).map(|d| d[0]), Some(9));
    }

    #[test]
    fn disk_keyspace_is_isolated() {
        let mut c = RamCache::<2>::new();
        c.fill(1, 10, &blk(1));
        // disk=2 同块号 ≠ 命中。
        assert!(c.get(2, 10).is_none());
        c.fill(2, 10, &blk(2));
        assert_eq!(c.get(1, 10).map(|d| d[0]), Some(1));
        assert_eq!(c.get(2, 10).map(|d| d[0]), Some(2));
    }

    #[test]
    fn purge_clears_all_and_counts() {
        let mut c = RamCache::<4>::new();
        for b in 0..4u64 {
            c.fill(1, b, &blk(b as u8 + 1));
        }
        assert_eq!(c.resident(), 4);
        assert_eq!(c.purge_all(), 4);
        assert_eq!(c.resident(), 0);
        assert!(c.get(1, 0).is_none());
        assert_eq!(c.purge_all(), 0, "重复清空幂等");
        let (_, _, _, purges) = c.stats();
        assert_eq!(purges, 2);
    }

    #[test]
    fn verify_detects_residue_without_purge() {
        let mut c = RamCache::<4>::new();
        // 块体全零、仅 [123]=0xAB：verify 必须精确报出槽与偏移。
        let mut b = [0u8; BLOCK_SIZE];
        b[123] = 0xAB;
        c.fill(3, 77, &b);
        let r = c.verify_zero().expect("有驻留数据必须检出残留");
        assert_eq!(r.slot, c_find(&c, 3, 77));
        assert_eq!(r.off, 123);
        assert_eq!(r.byte, 0xAB);
    }

    fn c_find<const CAP: usize>(c: &RamCache<CAP>, disk: u32, block: u64) -> usize {
        c.slots
            .iter()
            .position(|s| s.used && s.disk == disk && s.block == block)
            .unwrap()
    }

    #[test]
    fn purge_and_verify_zero_residue() {
        let mut c = RamCache::<4>::new();
        for b in 0..3u64 {
            c.fill(5, b, &blk(b as u8 + 3));
        }
        let cleared = c.purge_and_verify().expect("清空后必须零残留");
        assert_eq!(cleared, 3);
        assert_eq!(c.verify_zero(), None);
        // hash 证据：全零区 hash 稳定（两次 purge 后一致）。
        let h1 = c.hash_all();
        c.fill(5, 0, &blk(1));
        let _ = c.purge_and_verify();
        assert_eq!(c.hash_all(), h1, "全零区 hash 必须逐次一致");
    }

    #[test]
    fn reclaim_restore_ledger_two_sides_clear() {
        let mut c = RamCache::<4>::new();
        for b in 0..4u64 {
            c.fill(1, b, &blk(b as u8 + 1));
        }
        // reclaim 2 页：LRU 逐出 2 块，owed=2。
        let got = ReclaimStage::reclaim(&mut c, 2);
        assert_eq!(got, 2);
        assert_eq!(c.resident(), 2);
        // 超额 restore 只还欠账。
        let back = ReclaimStage::restore(&mut c, 9);
        assert_eq!(back, 2, "归还 = min(请求, 欠账)");
        // 再 restore 无欠账可还。
        assert_eq!(ReclaimStage::restore(&mut c, 1), 0);
    }

    #[test]
    fn reclaim_on_empty_is_zero_and_refuses_zero_pages() {
        let mut c = RamCache::<2>::new();
        assert_eq!(ReclaimStage::reclaim(&mut c, 3), 0);
        c.fill(1, 1, &blk(1));
        assert_eq!(ReclaimStage::reclaim(&mut c, 0), 0);
        assert_eq!(c.resident(), 1, "0 页请求不得逐出任何块");
    }

    #[test]
    fn stats_hit_miss_eviction_counting() {
        let mut c = RamCache::<1>::new();
        let _ = c.get(1, 1); // miss
        c.fill(1, 1, &blk(1));
        let _ = c.get(1, 1); // hit
        c.fill(1, 2, &blk(2)); // evict
        let (h, m, e, _) = c.stats();
        assert_eq!((h, m, e), (1, 1, 1));
    }

    #[test]
    fn hash_reflects_resident_state() {
        let mut c = RamCache::<2>::new();
        let empty = c.hash_all();
        c.fill(1, 1, &blk(1));
        let filled = c.hash_all();
        assert_ne!(empty, filled, "驻留状态必须反映进 hash");
        let _ = c.purge_all();
        assert_eq!(c.hash_all(), empty, "清空后 hash 回到全零基准");
    }
}
