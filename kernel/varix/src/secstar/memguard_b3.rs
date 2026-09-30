//! F176 内存守卫 · 批次三深化（secstar · G-G-06）。
//!
//! 批次三功能面（主册判据「六类越界全捕获 / 开销 <3%」的实现纵深）：
//! - [`Quarantine`]：UAF 隔离区——释放后的块冻结 N 个纪元（epoch），
//!   冻结期内触碰即报 UAF；纪元推进后真正回收（use-after-free 的
//!   确定性捕获面，不靠运气）；
//! - [`canary_write/canary_verify`]：金丝雀模型——块尾 8B 模式写入与
//!   校验（堆溢出的第一道指纹）；
//! - [`SizeClass`]：分配尺寸分级——六级 slab 尺寸档（分配延迟 P99 <1μs
//!   的分级地基，主层六档判据的实现面）；
//! - [`FragmentationEstimator`]：碎片率估计——空闲段/总量比的滑动估计
//!   （7 天烤机碎片率 <15% 判据的测量面）。
//!
//! 零堆纪律：定长隔离环 + 定长块表，无 alloc。

use super::memguard::{FaultClass, GUARD_PAGE_BYTES, POISON_BYTE};
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// UAF 隔离区（冻结纪元模型）
// ---------------------------------------------------------------------------

/// 隔离冻结纪元数（释放后冻结 4 纪元——足够盖住典型 UAF 窗口）。
pub const QUARANTINE_EPOCHS: u32 = 4;
/// 隔离环容量。
pub const QUARANTINE_CAP: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct QuaranSlot {
    base: u64,
    size: u64,
    freed_epoch: u32,
}

/// 隔离区：环满时驱逐最老槽（冻结最久的块——UAF 风险随时间衰减）。
pub struct Quarantine {
    slots: [Option<QuaranSlot>; QUARANTINE_CAP],
    pub epoch: u32,
}

impl Quarantine {
    pub const fn new() -> Quarantine {
        Quarantine { slots: [const { None }; QUARANTINE_CAP], epoch: 0 }
    }

    /// 释放块入隔离（冻结起点=当前纪元）。同块冻结期内重复冻结 = 双
    /// free 信号（拒）；冻结已过期的同块 = 新一轮释放（替换旧账）。
    pub fn freeze(&mut self, base: u64, size: u64) -> bool {
        if let Some(s) = self.slots.iter().flatten().find(|s| s.base == base) {
            if self.epoch.saturating_sub(s.freed_epoch) < QUARANTINE_EPOCHS {
                return false; // 冻结期内再冻 = 双 free
            }
        }
        // 先替换过期同块槽，再找空槽。
        for slot in self.slots.iter_mut().flatten() {
            if slot.base == base {
                slot.size = size;
                slot.freed_epoch = self.epoch;
                return true;
            }
        }
        if let Some(slot) = self.slots.iter_mut().find(|s| s.is_none()) {
            *slot = Some(QuaranSlot { base, size, freed_epoch: self.epoch });
            return true;
        }
        // 环满：驱逐冻结最久槽。
        let mut oldest = 0;
        for i in 1..QUARANTINE_CAP {
            let a = self.slots[i].unwrap().freed_epoch;
            let b = self.slots[oldest].unwrap().freed_epoch;
            if a < b {
                oldest = i;
            }
        }
        self.slots[oldest] = Some(QuaranSlot { base, size, freed_epoch: self.epoch });
        true
    }

    /// 访问审查：冻结期内触碰 → UAF 报告（主层 FaultClass::UseAfterFree
    /// 语义面）；纪元走满 → 放行（块已回收，归属新分配方）。
    pub fn touch(&self, addr: u64) -> Result<(), FaultClass> {
        for s in self.slots.iter().flatten() {
            if addr >= s.base && addr < s.base + s.size {
                if self.epoch.saturating_sub(s.freed_epoch) < QUARANTINE_EPOCHS {
                    return Err(FaultClass::UseAfterFree);
                }
            }
        }
        Ok(())
    }

    /// 纪元推进（ GC 拍）。
    pub fn advance_epoch(&mut self) {
        self.epoch += 1;
    }

    pub fn frozen_count(&self) -> usize {
        self.slots.iter().flatten().count()
    }
}

// ---------------------------------------------------------------------------
// 金丝雀模型（块尾 8B 模式）
// ---------------------------------------------------------------------------

/// 金丝雀长度。
pub const CANARY_LEN: usize = 8;
/// 金丝雀模式（与毒值 0xDD 区分——溢出探到哪一侧可归因）。
pub const CANARY_PATTERN: [u8; CANARY_LEN] = [0xC1, 0xA5, 0xC1, 0xA5, 0xC1, 0xA5, 0xC1, 0xA5];

/// 在块尾写金丝雀（调用方持有块尾切片——布局归主层，模式归本层）。
pub fn canary_write(tail: &mut [u8; CANARY_LEN]) {
    *tail = CANARY_PATTERN;
}

/// 校验金丝雀：逐字节对——发现被改 = 堆溢出指纹（返回损坏字节位）。
pub fn canary_verify(tail: &[u8; CANARY_LEN]) -> Option<usize> {
    tail.iter().zip(CANARY_PATTERN.iter()).position(|(a, p)| a != p)
}

// ---------------------------------------------------------------------------
// 分配尺寸分级（六级 slab）
// ---------------------------------------------------------------------------

/// 六级尺寸档上限（字节）：16/64/256/1K/4K/超大。
pub const SIZE_CLASS_EDGES: [u64; 6] = [16, 64, 256, 1_024, 4_096, u64::MAX];

/// 尺寸分级：落在哪一档（0-5；超大块独立走页分配不走 slab）。
pub fn size_class(size: u64) -> usize {
    for (i, e) in SIZE_CLASS_EDGES.iter().enumerate() {
        if size <= *e {
            return i;
        }
    }
    SIZE_CLASS_EDGES.len() - 1
}

/// 档位是否走 slab 快径（0-4 档 slab，第 5 档超大直配）。
pub fn is_slab_class(class: usize) -> bool {
    class < 5
}

// ---------------------------------------------------------------------------
// 碎片率估计
// ---------------------------------------------------------------------------

/// 碎片估计器：外部碎片率 = 1 - 最大空闲段/总空闲（经典口径的整数面）。
pub struct FragmentationEstimator {
    total_free: u64,
    max_free: u64,
}

impl FragmentationEstimator {
    pub const fn new() -> FragmentationEstimator {
        FragmentationEstimator { total_free: 0, max_free: 0 }
    }

    /// 汇入一个空闲段。
    pub fn add_free_segment(&mut self, bytes: u64) {
        self.total_free += bytes;
        if bytes > self.max_free {
            self.max_free = bytes;
        }
    }

    /// 碎片率千分比：0=零碎片；总空闲 0 → 0（无空闲即无外部碎片）。
    pub fn permille(&self) -> u32 {
        if self.total_free == 0 {
            return 0;
        }
        // u128 中间量——大池（u64::MAX 级）不溢出。
        let usable = ((self.max_free as u128 * 1_000) / self.total_free as u128).min(1_000) as u32;
        1_000 - usable
    }

    /// 主册线：碎片率 <15% 判据直算。
    pub fn under_budget(&self) -> bool {
        self.permille() < 150
    }
}

// ---------------------------------------------------------------------------
// 批次三自检
// ---------------------------------------------------------------------------

#[inline(never)]
pub fn run_memguard_b3_checks() -> CheckSet {
    let mut cs = CheckSet::new("F176-b3");

    // 1) 隔离区冻结期触碰 → UAF（确定性捕获——不靠运气）。
    let mut q = Quarantine::new();
    q.advance_epoch();
    assert!(q.freeze(0x2000, 0x100));
    cs.add("quarantine_uaf_caught", q.touch(0x2050) == Err(FaultClass::UseAfterFree), "");

    // 2) 隔离区外地址不受牵连（守卫不开空白支票）。
    cs.add("quarantine_outside_ok", q.touch(0x9999).is_ok(), "");

    // 3) 冻结期满放行：4 纪元后同地址触碰不再误报（回收语义）。
    for _ in 0..QUARANTINE_EPOCHS {
        q.advance_epoch();
    }
    cs.add("quarantine_expires", q.touch(0x2050).is_ok(), "");

    // 4) 双 free 留痕：同块二次冻结被拒（隔离区面的一致性）。
    let mut q2 = Quarantine::new();
    let first = q2.freeze(0x3000, 0x40);
    let second = q2.freeze(0x3000, 0x40);
    cs.add("quarantine_double_free_rejected", first && !second && q2.frozen_count() == 1, "");

    // 5) 环满驱逐最老槽：16 满后第 17 块入环、最老块让位（内存上限纪律）。
    let mut q3 = Quarantine::new();
    for i in 0..(QUARANTINE_CAP as u64 + 1) {
        q3.advance_epoch();
        q3.freeze(0x10000 + i * 0x100, 0x100);
    }
    cs.add(
        "quarantine_ring_evicts",
        q3.frozen_count() == QUARANTINE_CAP && q3.touch(0x10000).is_ok() && q3.touch(0x10000 + QUARANTINE_CAP as u64 * 0x100).is_err(),
        "",
    );

    // 6) 金丝雀写入与校验：写后验零损坏（无溢出无报告）。
    let mut tail = [0u8; CANARY_LEN];
    canary_write(&mut tail);
    cs.add("canary_intact", canary_verify(&tail).is_none(), "");

    // 7) 金丝雀被改 → 定位损坏位（堆溢出指纹落在第几字节）。
    let mut tail2 = CANARY_PATTERN;
    tail2[3] = 0xFF;
    cs.add("canary_pinpoints", canary_verify(&tail2) == Some(3), "");

    // 8) 金丝雀与毒值分色：0xC1A5 ≠ 0xDD（溢出方向可归因）。
    cs.add("canary_not_poison", CANARY_PATTERN[0] != POISON_BYTE, "");

    // 9) 尺寸分级六级：边界值落档全对（16→0、17→1、4K→4、超大→5）。
    cs.add(
        "size_class_edges",
        size_class(1) == 0 && size_class(16) == 0 && size_class(17) == 1 && size_class(4_096) == 4 && size_class(1_000_000) == 5,
        "",
    );

    // 10) slab 快径面：0-4 档走 slab、第 5 档直配（延迟分级的地基）。
    cs.add(
        "slab_vs_direct",
        (0..5).all(is_slab_class) && !is_slab_class(5),
        "",
    );

    // 11) 碎片率：均匀大段 ≈ 低碎片、碎渣化 → 高碎片（测量面两态）。
    let mut f1 = FragmentationEstimator::new();
    f1.add_free_segment(1_000);
    let mut f2 = FragmentationEstimator::new();
    for _ in 0..10 {
        f2.add_free_segment(100);
    }
    cs.add(
        "fragmentation_two_states",
        f1.permille() == 0 && f2.permille() == 900 && !f2.under_budget() && f1.under_budget(),
        "",
    );

    // 12) 碎片线：15% 内即预算内（主册 <15% 线的直算口径）。
    let mut f3 = FragmentationEstimator::new();
    f3.add_free_segment(900);
    f3.add_free_segment(100);
    cs.add("fragmentation_budget_line", f3.permille() == 100 && f3.under_budget(), "");

    // 13) 空池零碎片：总空闲 0 → 0（无空闲无外部碎片——除零诚实）。
    cs.add("fragmentation_empty_zero", FragmentationEstimator::new().permille() == 0, "");

    // 14) 页宽贯通：护栏页宽与主层一处一事实（4KB）。
    cs.add("page_width_aligned", GUARD_PAGE_BYTES == 4_096, "");

    cs
}

// ---------------------------------------------------------------------------
// 宿主单测（批次三）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests_b3 {
    use super::*;

    #[test]
    fn quarantine_full_lifecycle() {
        // 冻结→冻结期触碰报 UAF→期满放行→新块复用同地址不受旧账牵连。
        let mut q = Quarantine::new();
        q.freeze(0x5000, 0x80);
        assert!(q.touch(0x5010).is_err());
        for _ in 0..QUARANTINE_EPOCHS {
            q.advance_epoch();
        }
        assert!(q.touch(0x5010).is_ok());
        // 重冻结（新分配又释放）→ 再次受保护。
        assert!(q.freeze(0x5000, 0x80));
        assert!(q.touch(0x5010).is_err());
    }

    #[test]
    fn canary_every_byte_position() {
        // 每个字节位被改都能定位（8 位全覆盖——指纹无盲区）。
        for pos in 0..CANARY_LEN {
            let mut t = CANARY_PATTERN;
            t[pos] ^= 0xFF;
            assert_eq!(canary_verify(&t), Some(pos), "pos={pos}");
        }
    }

    #[test]
    fn fragmentation_never_negative() {
        // 单段池碎片率恒 0；比例恒在 0..=1000（估计器不吐负数）。
        let mut f = FragmentationEstimator::new();
        f.add_free_segment(u64::MAX / 2);
        assert_eq!(f.permille(), 0);
        assert!(f.under_budget());
    }
}
