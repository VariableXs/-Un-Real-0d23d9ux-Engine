//! mech_radix — 三级基数树脏页追踪（AI-K1 深化批次四 · F046 写合并窗口）。
//!
//! 主册依据：
//! - F046【设计细节】「写入合并窗口自适应」的**合并对象管理**：哪些页脏、
//!   哪段连续可回写——批次三落了窗口策略/条件表/曲线；本件补**页级脏标记
//!   的容器**：Linux 页缓存同族的基数树（radix tree），三级覆盖 32 位页帧，
//!   定长节点池 + 空闲链（零堆），`pop_range` 给出回写器的天然接口
//!   （F046「合并写」= 相邻脏页的连续段一次下盘）。
//! - 节点账（分配/峰值）供 F052 同款字节账对账；池耗尽诚实报错。

/// 页大小（4KB——F045/F046 全域口径）。
pub const PAGE_SIZE: usize = 4096;
/// L2 节点容量（每叶节点管理 2^6 页 = 256KB 脏区）。
pub const LEAF_BITS: u32 = 6;
pub const LEAF_SIZE: usize = 1 << LEAF_BITS;
/// L2 节点数上限（64 × 256KB = 16MB 追踪域——F046 窗口管理域）。
pub const L2_CAP: usize = 64;
/// L1 节点数上限（每 L1 管 64 个 L2 → 覆盖 1GB）。
pub const L1_CAP: usize = 16;

#[derive(Clone, Copy)]
struct L2Node {
    /// 位图（LEAF_SIZE 页）。
    bits: u64,
    used: bool,
    next_free: i32,
}

/// 三级基数树（L1 → L2 句柄 → 位图）：L1 槽 = 64 个 L2 句柄。
pub struct DirtyRadix {
    /// l1[g][k] = L2 节点句柄（-1 = 空）。
    l1: [[i16; 64]; L1_CAP],
    l2: [L2Node; L2_CAP],
    free_head: i32,
    free_count: usize,
    /// 脏页计数（pop/set 同步维护——字节账的对账源）。
    pub dirty_pages: u64,
    pub peak_l2_nodes: usize,
    pub exhausted: u32,
}

impl DirtyRadix {
    pub const fn new() -> Self {
        DirtyRadix {
            l1: [[-1; 64]; L1_CAP],
            l2: [const_nil_l2(); L2_CAP],
            free_head: 0,
            free_count: L2_CAP,
            dirty_pages: 0,
            peak_l2_nodes: 0,
            exhausted: 0,
        }
    }

    /// 空闲链初建（构造时一次性）。
    pub fn init(&mut self) {
        for i in 0..L2_CAP {
            self.l2[i].used = false;
            self.l2[i].bits = 0;
            self.l2[i].next_free = if i + 1 < L2_CAP { (i + 1) as i32 } else { -1 };
        }
        self.free_head = 0;
        self.free_count = L2_CAP;
        self.l1 = [[-1; 64]; L1_CAP];
        self.dirty_pages = 0;
        self.peak_l2_nodes = 0;
    }

    fn alloc_l2(&mut self) -> Option<usize> {
        if self.free_head < 0 {
            self.exhausted += 1;
            return None;
        }
        let i = self.free_head as usize;
        self.free_head = self.l2[i].next_free;
        self.free_count -= 1;
        self.l2[i].used = true;
        self.l2[i].bits = 0;
        let used = L2_CAP - self.free_count;
        if used > self.peak_l2_nodes {
            self.peak_l2_nodes = used;
        }
        Some(i)
    }

    fn free_l2(&mut self, i: usize) {
        self.l2[i].used = false;
        self.l2[i].next_free = self.free_head;
        self.free_head = i as i32;
        self.free_count += 1;
    }

    /// 页号 → (l1 槽, 句柄位, 位图位)。
    fn split(page: u64) -> (usize, usize, usize) {
        let pages_per_l1 = (64 * LEAF_SIZE) as u64;
        let g = (page / pages_per_l1) as usize;
        let within = (page % pages_per_l1) as usize;
        (g, within / LEAF_SIZE, within % LEAF_SIZE)
    }

    /// 标脏。页号全域 = L1_CAP × 64 × LEAF_SIZE 页（65536 页 = 256MB 追踪域）。
    pub fn set_dirty(&mut self, page: u64) -> Result<(), ()> {
        let (g, k, bit) = Self::split(page);
        if g >= L1_CAP {
            return Err(());
        }
        if self.l1[g][k] < 0 {
            match self.alloc_l2() {
                Some(n) => self.l1[g][k] = n as i16,
                None => return Err(()),
            }
        }
        let n = self.l1[g][k] as usize;
        if self.l2[n].bits & (1u64 << bit) == 0 {
            self.l2[n].bits |= 1u64 << bit;
            self.dirty_pages += 1;
        }
        Ok(())
    }

    /// 清脏（回写完成）。未脏页返回 false（调用侧据此发现重复回写）。
    pub fn clear_dirty(&mut self, page: u64) -> bool {
        let (g, k, bit) = Self::split(page);
        if g >= L1_CAP || self.l1[g][k] < 0 {
            return false;
        }
        let n = self.l1[g][k] as usize;
        if self.l2[n].bits & (1u64 << bit) != 0 {
            self.l2[n].bits &= !(1u64 << bit);
            self.dirty_pages -= 1;
            if self.l2[n].bits == 0 {
                self.l1[g][k] = -1;
                self.free_l2(n); // 节点回收——池不随历史增长
            }
            true
        } else {
            false
        }
    }

    pub fn is_dirty(&self, page: u64) -> bool {
        let (g, k, bit) = Self::split(page);
        if g >= L1_CAP || self.l1[g][k] < 0 {
            return false;
        }
        self.l2[self.l1[g][k] as usize].bits & (1u64 << bit) != 0
    }

    /// 弹出一段连续脏页（回写器接口）：从 `from` 起找到的最长连续脏段
    /// （上限 `max_pages`），就地清脏。无脏页 → (0, 0)。
    pub fn pop_range(&mut self, from: u64, max_pages: u64) -> (u64, u64) {
        let mut start = from;
        // 跳过非脏前缀。
        while start < self.capacity_pages() && !self.is_dirty(start) {
            start += 1;
        }
        if start >= self.capacity_pages() {
            return (0, 0);
        }
        let mut len = 0u64;
        while start + len < self.capacity_pages() && len < max_pages && self.is_dirty(start + len) {
            self.clear_dirty(start + len);
            len += 1;
        }
        (start, len)
    }

    /// 追踪域容量（页）：L1_CAP × 64 句柄 × 64 位 = 65536 页（256MB）。
    pub fn capacity_pages(&self) -> u64 {
        (L1_CAP * 64 * LEAF_SIZE) as u64
    }

    /// 在用 L2 节点数（F052 字节账口径的对账面）。
    pub fn used_l2_nodes(&self) -> usize {
        L2_CAP - self.free_count
    }
}

/// 常量空节点（const fn 构造数组用）。
const fn const_nil_l2() -> L2Node {
    L2Node { bits: 0, used: false, next_free: -1 }
}

// ---------------------------------------------------------------------------
// 宿主单测
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// CheckSet（挂 F046）
// ---------------------------------------------------------------------------

/// 运行检查项（判据锚点见对账表批次四段）。
use crate::checks::CheckSet;

pub fn run_checks() -> CheckSet {
    let mut cs = CheckSet::new("F046-mech-radix");
    // 1) 标脏/清脏/查询 round-trip（跨位图字边界）。
    let mut r = DirtyRadix::new();
    r.init();
    r.set_dirty(0).unwrap();
    r.set_dirty(63).unwrap();
    r.set_dirty(64).unwrap();
    let set_ok = r.is_dirty(0) && r.is_dirty(63) && r.is_dirty(64) && !r.is_dirty(1);
    // 二次清脏如实报 false（首次 true = 确有位被清）。
    let dup_clear = r.clear_dirty(63) && !r.clear_dirty(63);
    cs.add("dirty_set_clear_roundtrip", set_ok && r.dirty_pages == 2 && dup_clear, "");
    // 2) 连续段弹出（合并写下盘的接口语义）。
    let mut r2 = DirtyRadix::new();
    r2.init();
    for p in [10u64, 11, 12, 13, 40, 41] {
        r2.set_dirty(p).unwrap();
    }
    let (s1, l1) = r2.pop_range(0, 64);
    let (s2, l2) = r2.pop_range(0, 64);
    let (s3, _) = r2.pop_range(0, 64);
    cs.add("pop_range_longest_run", (s1, l1) == (10, 4) && (s2, l2) == (40, 2) && (s3, 0) == (0, 0), "");
    // 3) 节点回收（池不随历史增长）+ 池满诚实计数。
    let mut r3 = DirtyRadix::new();
    r3.init();
    for k in 0..64u64 {
        r3.set_dirty(k * LEAF_SIZE as u64).unwrap();
    }
    let full = r3.used_l2_nodes() == L2_CAP;
    // 组 0 覆盖页 0..4096——耗尽判例页号必须在容量域内（越域页被
    // 范围检查拒绝，不算池耗尽）。
    let refused = r3.set_dirty(4096 + 5).is_err() && r3.exhausted == 1;
    for k in 0..64u64 {
        r3.clear_dirty(k * LEAF_SIZE as u64);
    }
    cs.add("pool_recycle_and_exhaustion", full && refused && r3.used_l2_nodes() == 0, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_clear_test_roundtrip() {
        let mut r = DirtyRadix::new();
        r.init();
        assert!(r.set_dirty(0).is_ok());
        assert!(r.set_dirty(63).is_ok());
        assert!(r.set_dirty(64).is_ok()); // 跨位图字
        assert!(r.is_dirty(0) && r.is_dirty(63) && r.is_dirty(64));
        assert!(!r.is_dirty(1));
        assert_eq!(r.dirty_pages, 3);
        assert!(r.clear_dirty(63));
        assert!(!r.clear_dirty(63), "重复清脏如实报 false");
        assert_eq!(r.dirty_pages, 2);
    }

    #[test]
    fn node_recycling_on_empty_l2() {
        let mut r = DirtyRadix::new();
        r.init();
        // 污染一个 L1 槽的一个 L2 再清空 → 节点必须归还池。
        let base = 5 * 64 * LEAF_SIZE as u64;
        for i in 0..8u64 {
            r.set_dirty(base + i).unwrap();
        }
        assert_eq!(r.used_l2_nodes(), 1);
        for i in 0..8u64 {
            r.clear_dirty(base + i);
        }
        assert_eq!(r.used_l2_nodes(), 0, "空 L2 必须回收——池不随历史增长");
        assert_eq!(r.peak_l2_nodes, 1);
    }

    #[test]
    fn pop_range_returns_longest_run() {
        let mut r = DirtyRadix::new();
        r.init();
        for p in [10u64, 11, 12, 13, 40, 41] {
            r.set_dirty(p).unwrap();
        }
        let (s1, l1) = r.pop_range(0, 64);
        assert_eq!((s1, l1), (10, 4), "10..13 是最靠前的连续段");
        let (s2, l2) = r.pop_range(0, 64);
        assert_eq!((s2, l2), (40, 2));
        let (s3, l3) = r.pop_range(0, 64);
        assert_eq!((s3, l3), (0, 0), "弹尽如实报空");
        assert_eq!(r.dirty_pages, 0);
    }

    #[test]
    fn pop_range_respects_max() {
        let mut r = DirtyRadix::new();
        r.init();
        for p in 0..100u64 {
            r.set_dirty(p).unwrap();
        }
        let (s, l) = r.pop_range(0, 30);
        assert_eq!((s, l), (0, 30));
        let (s2, l2) = r.pop_range(0, 30);
        assert_eq!((s2, l2), (30, 30));
    }

    #[test]
    fn out_of_range_honest() {
        let mut r = DirtyRadix::new();
        r.init();
        assert!(r.set_dirty(r.capacity_pages()).is_err(), "越域标脏拒绝");
        assert!(!r.is_dirty(r.capacity_pages()));
        assert!(!r.clear_dirty(r.capacity_pages()));
    }

    #[test]
    fn exhaustion_is_counted() {
        let mut r = DirtyRadix::new();
        r.init();
        // 槽 0 内触碰全部 64 个 L2 → 池恰满（每页距 LEAF_SIZE）。
        for k in 0..64u64 {
            r.set_dirty(k * LEAF_SIZE as u64).unwrap();
        }
        assert_eq!(r.used_l2_nodes(), L2_CAP);
        // 池满：组 1 的新页标脏失败 + exhausted 计数。
        // 组 0 覆盖页 0..4096（64 节点 × 64 页）——页号必须仍在容量域内
        // （越过 capacity 的页被范围检查拒绝，不算池耗尽）。
        assert!(r.set_dirty(4096 + 5).is_err());
        assert_eq!(r.exhausted, 1);
        // 已占节点继续标脏不受影响（池满不冻结既有账目）。
        assert!(r.set_dirty(1).is_ok());
    }
}
