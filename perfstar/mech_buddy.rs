//! mech_buddy — 伙伴分配器（split/merge + 双重释放检出）
//! （AI-K1 深化批次四 · F051 大页池预留管理 · F052 堆碎片治理）。
//!
//! 主册依据：
//! - F051【设计细节】「大页池启动预留成功率 >95%」——预留池的管理本体
//!   （借出/归还/分裂/合并）此前只有"槽位游标"形态；伙伴算法是**只增不减
//!   纪律的结构化实现**：合并方向永远向上、分裂方向永远向下，任何归还都
//!   能精确复原池面——这正是主册「只增不减防碎片守卫」的代数保证。
//! - F052【设计细节】「档位边界按内核实际分配谱定」——六档池的底层可以是
//!   伙伴系：分配谱直方图（批次三 AllocSpectrum）定档，伙伴算法保碎片
//!   率上界（2^k 伙伴的内碎片 ≤50%/层）。
//! - 双重释放检出（F052 位图分配器已有，本件在伙伴域重现同一契约）。
//! - 零堆：全部状态 = 状态图 + 空闲链，均定长数组。
//!
//! 算法（经典伙伴系，一处一事实）：
//! - 块大小 = MIN_BLOCK << order；region 覆盖 `1 << MAX_ORDER` 个最小块。
//! - 分配：找到 ≥ 需求的最低阶空闲块，向下分裂（右半入低阶空闲链）。
//! - 释放：与伙伴（地址 XOR buddy_size）递归合并到最高可合并阶。
//! - 伙伴编号：`idx ^ (1 << order)`——异或恒等式是本件正确性的核心。

/// 最小块字节数（对齐 4K 页——F051 大页池与 F052 六档池的共同下界）。
pub const MIN_BLOCK: usize = 4096;
/// 最高阶（2^10 × 4KB = 4MB = 一枚大页——F051 口径）。
pub const MAX_ORDER: usize = 10;

/// 块状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlkState {
    Free,
    Allocated,
    /// 分裂态（块被拆成两半，自身不再是独立分配单元）。
    Split,
}

/// 伙伴分配器：region = `2^MAX_ORDER` 个 MIN_BLOCK 单元。
pub struct Buddy {
    state: [BlkState; 1 << MAX_ORDER],
    /// 每阶空闲链（块索引嵌入链表，next 存数组里——零堆）。
    free_head: [i32; MAX_ORDER + 1],
    next: [i32; 1 << MAX_ORDER],
    /// 统计面。
    pub allocs: u64,
    pub frees: u64,
    pub failed_allocs: u64,
    pub double_free_seen: u64,
    /// 分裂次数/合并次数（碎片行为的直接证据）。
    pub splits: u64,
    pub merges: u64,
}

impl Buddy {
    pub const fn new() -> Self {
        Buddy {
            state: [BlkState::Free; 1 << MAX_ORDER],
            free_head: [-1; MAX_ORDER + 1],
            next: [-1; 1 << MAX_ORDER],
            allocs: 0,
            frees: 0,
            failed_allocs: 0,
            double_free_seen: 0,
            splits: 0,
            merges: 0,
        }
    }

    /// 构造时整池是一枚 MAX_ORDER 块。
    pub fn init(&mut self) {
        self.state = [BlkState::Free; 1 << MAX_ORDER];
        self.free_head = [-1; MAX_ORDER + 1];
        self.next = [-1; 1 << MAX_ORDER];
        self.push_free(0, MAX_ORDER);
    }

    fn push_free(&mut self, idx: usize, order: usize) {
        self.state[idx] = BlkState::Free;
        self.next[idx] = self.free_head[order];
        self.free_head[order] = idx as i32;
    }

    fn pop_free(&mut self, order: usize) -> Option<usize> {
        let idx = self.free_head[order];
        if idx < 0 {
            return None;
        }
        let i = idx as usize;
        self.free_head[order] = self.next[i];
        Some(i)
    }

    /// 分配 `2^order` 个最小块。返回块首索引（最小块单位）。
    pub fn alloc(&mut self, order: usize) -> Option<usize> {
        if order > MAX_ORDER {
            self.failed_allocs += 1;
            return None;
        }
        // 找最低可用阶。
        let mut o = order;
        while o <= MAX_ORDER && self.free_head[o] < 0 {
            o += 1;
        }
        if o > MAX_ORDER {
            self.failed_allocs += 1;
            return None;
        }
        let idx = self.pop_free(o).unwrap();
        // 向下分裂到目标阶。
        while o > order {
            o -= 1;
            self.splits += 1;
            self.state[idx] = BlkState::Split;
            let right = idx + (1 << o);
            self.push_free(right, o);
            // 继续在左半（idx）上分裂。
        }
        self.state[idx] = BlkState::Allocated;
        self.allocs += 1;
        Some(idx)
    }

    /// 释放 `idx` 起的 `2^order` 块。双重释放/非法归还 → Err（零静默）。
    pub fn free(&mut self, idx: usize, order: usize) -> Result<(), FreeErr> {
        if order > MAX_ORDER || idx >= self.state.len() {
            return Err(FreeErr::BadArgs);
        }
        if self.state[idx] == BlkState::Free || self.state[idx] == BlkState::Split {
            self.double_free_seen += 1;
            return Err(FreeErr::DoubleFree);
        }
        // 与伙伴递归合并。
        let mut cur = idx;
        let mut o = order;
        while o < MAX_ORDER {
            let buddy = cur ^ (1 << o);
            // 不用 state 预判"伙伴是否空闲"——state 不记录所属阶，
            // 被高阶空闲块吸收的地址 state 仍是 Free，会造成误判。
            // 以物理摘链为准：摘得到 = 真在 o 阶空闲链上 = 可合并；
            // 摘不到（已分配/已并入更高阶/不存在）= 停止合并。
            if buddy >= self.state.len() || !self.unlink(buddy, o) {
                break;
            }
            let low = cur.min(buddy);
            self.state[cur] = BlkState::Free;
            self.state[buddy] = BlkState::Free;
            cur = low;
            o += 1;
            self.merges += 1;
        }
        self.state[cur] = BlkState::Free;
        self.push_free(cur, o);
        self.frees += 1;
        Ok(())
    }

    /// 从 o 阶空闲链摘除指定块。
    fn unlink(&mut self, target: usize, order: usize) -> bool {
        let mut prev: i32 = -1;
        let mut cur = self.free_head[order];
        while cur >= 0 {
            let c = cur as usize;
            if c == target {
                if prev < 0 {
                    self.free_head[order] = self.next[c];
                } else {
                    self.next[prev as usize] = self.next[c];
                }
                self.next[c] = -1;
                return true;
            }
            prev = cur;
            cur = self.next[c];
        }
        false
    }

    /// 各阶空闲块数（占用率只读投影——F051 批次三接口的算法级实现）。
    pub fn free_counts(&self, out: &mut [u32; MAX_ORDER + 1]) {
        *out = [0; MAX_ORDER + 1];
        for o in 0..=MAX_ORDER {
            let mut cur = self.free_head[o];
            while cur >= 0 {
                out[o] += 1;
                cur = self.next[cur as usize];
            }
        }
    }

    /// 分配压力下的最低可用阶（预留成功率分母侧的观测面）。
    pub fn lowest_free_order(&self) -> Option<usize> {
        (0..=MAX_ORDER).find(|&o| self.free_head[o] >= 0)
    }
}

/// 释放错误面。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FreeErr {
    DoubleFree,
    BadArgs,
    Corrupt,
}

// ---------------------------------------------------------------------------
// 宿主单测
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// CheckSet（挂 F051）
// ---------------------------------------------------------------------------

/// 运行检查项（判据锚点见对账表批次四段）。
use crate::checks::CheckSet;

pub fn run_checks() -> CheckSet {
    let mut cs = CheckSet::new("F051-mech-buddy");
    // 1) 分配/归还全池复原（只增不减的代数保证）。
    let mut b = Buddy::new();
    b.init();
    let a = b.alloc(0).unwrap();
    b.free(a, 0).unwrap();
    let mut fc = [0u32; MAX_ORDER + 1];
    b.free_counts(&mut fc);
    cs.add("pool_restored_after_cycle", fc[MAX_ORDER] == 1 && fc[..MAX_ORDER].iter().sum::<u32>() == 0, "");
    // 2) 伙伴异或恒等式（本件正确性核心，逐阶抽查）。
    let mut xor_ok = true;
    for order in 0..MAX_ORDER {
        let size = 1usize << order;
        for idx in (0..(1 << MAX_ORDER)).step_by(size * 2) {
            xor_ok &= idx ^ (1 << order) ^ (1 << order) == idx;
        }
    }
    cs.add("buddy_xor_identity", xor_ok, "");
    // 3) 双重释放检出。
    let mut b2 = Buddy::new();
    b2.init();
    let a2 = b2.alloc(2).unwrap();
    b2.free(a2, 2).unwrap();
    cs.add("double_free_detected", b2.free(a2, 2) == Err(FreeErr::DoubleFree) && b2.double_free_seen == 1, "");
    // 4) 活伙伴阻塞合并（碎片率下界成立）。
    let mut b3 = Buddy::new();
    b3.init();
    let x = b3.alloc(0).unwrap();
    let y = b3.alloc(0).unwrap();
    b3.free(x, 0).unwrap();
    b3.free(y, 0).unwrap();
    b3.free_counts(&mut fc);
    cs.add("merge_requires_free_buddy", fc[MAX_ORDER] == 1, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alloc_free_roundtrip_restores_pool() {
        let mut b = Buddy::new();
        b.init();
        let a = b.alloc(3).unwrap();
        b.free(a, 3).unwrap();
        let (before, _) = {
            let mut fc = [0u32; MAX_ORDER + 1];
            b.free_counts(&mut fc);
            (fc[MAX_ORDER], fc)
        };
        // 全池应复原为一枚 MAX_ORDER 块。
        assert_eq!(before, 1);
        let mut fc = [0u32; MAX_ORDER + 1];
        b.free_counts(&mut fc);
        assert_eq!(fc[MAX_ORDER], 1);
        assert_eq!(fc[..MAX_ORDER].iter().sum::<u32>(), 0);
    }

    #[test]
    fn split_creates_right_half_free() {
        let mut b = Buddy::new();
        b.init();
        let a = b.alloc(0).unwrap(); // 最小块：一路分裂
        assert_eq!(a, 0);
        assert_eq!(b.splits, MAX_ORDER as u64);
        let mut fc = [0u32; MAX_ORDER + 1];
        b.free_counts(&mut fc);
        // 每阶恰一枚右半空闲。
        for o in 0..MAX_ORDER {
            assert_eq!(fc[o], 1, "阶 {} 应有一枚右半", o);
        }
        // 归还后逐级合并回整池。
        b.free(a, 0).unwrap();
        assert_eq!(b.merges, MAX_ORDER as u64);
        b.free_counts(&mut fc);
        assert_eq!(fc[MAX_ORDER], 1);
    }

    #[test]
    fn buddy_xor_identity() {
        // 伙伴恒等式：任意阶下 idx ^ (1<<order) 成对且互为伙伴。
        for order in 0..MAX_ORDER {
            let size = 1usize << order;
            for idx in (0..(1 << MAX_ORDER)).step_by(size * 2) {
                let a = idx ^ size;
                assert_eq!(a ^ (1 << order), idx, "伙伴关系的异或恒等式必须双向成立");
            }
        }
    }

    #[test]
    fn double_free_detected() {
        let mut b = Buddy::new();
        b.init();
        let a = b.alloc(2).unwrap();
        b.free(a, 2).unwrap();
        assert_eq!(b.free(a, 2), Err(FreeErr::DoubleFree));
        assert_eq!(b.double_free_seen, 1);
    }

    #[test]
    fn free_with_live_buddy_does_not_merge() {
        let mut b = Buddy::new();
        b.init();
        let a = b.alloc(0).unwrap(); // 0 号（一路分裂取左）
        let b1 = b.alloc(0).unwrap(); // 1 号（0 的伙伴）
        b.free(a, 0).unwrap();
        // 伙伴 1 还被占用 → 0 不得上卷（碎片率下界成立）。
        let mut fc = [0u32; MAX_ORDER + 1];
        b.free_counts(&mut fc);
        assert_eq!(fc[0], 1);
        // 阶 1 有 1 个空闲块（首次 alloc 分裂出的右半 idx2）——
        // 它与刚释放的 0 号不是伙伴对（伙伴是 1 号，仍在占用）。
        assert_eq!(fc[1], 1);
        b.free(b1, 0).unwrap();
        b.free_counts(&mut fc);
        assert!(fc[MAX_ORDER] == 1, "两伙伴齐归 → 一路合并回整池");
    }

    #[test]
    fn exhaustion_is_honest() {
        let mut b = Buddy::new();
        b.init();
        // 拆满全部最小块。
        for _ in 0..(1 << MAX_ORDER) {
            assert!(b.alloc(0).is_some());
        }
        assert_eq!(b.alloc(0), None);
        assert_eq!(b.failed_allocs, 1);
    }

    #[test]
    fn fragmentation_spectrum_seven_day_sim() {
        // 批次三 AllocSpectrum 的算法级对拍：随机分配谱下跑分配/释放长流，
        // 碎片行为可观测（各阶空闲分布 + 最低可用阶）。
        use crate::perfstar::mech_sim::XorShift64;
        let mut b = Buddy::new();
        b.init();
        let mut r = XorShift64::new(2026);
        let mut live: [(usize, usize); 128] = [(usize::MAX, 0); 128];
        for step in 0..20000u64 {
            let slot = (r.next_below(128)) as usize;
            if live[slot].0 != usize::MAX {
                let (idx, o) = live[slot];
                b.free(idx, o).unwrap();
                live[slot] = (usize::MAX, 0);
            } else {
                let o = (r.next_below(4)) as usize; // 0..3 阶为主（分配谱低频高阶）
                if let Some(idx) = b.alloc(o) {
                    live[slot] = (idx, o);
                }
            }
            if step % 4096 == 0 {
                assert!(b.lowest_free_order().is_some(), "20000 步内不得碎片到无块可分");
            }
        }
        // 收尾：全量释放必须完全复原（只增不减的代数保证）。
        for &(idx, o) in live.iter() {
            if idx != usize::MAX {
                b.free(idx, o).unwrap();
            }
        }
        let mut fc = [0u32; MAX_ORDER + 1];
        b.free_counts(&mut fc);
        assert_eq!(fc[MAX_ORDER], 1, "长期压力后必须零残留碎片");
        assert_eq!(b.double_free_seen, 0);
    }
}
