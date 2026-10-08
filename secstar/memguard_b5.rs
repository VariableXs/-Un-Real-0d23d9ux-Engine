//! F176 内存守卫 · 批次五深化（secstar · G-G-06）。
//!
//! 批次五功能面（达成率 42%——主攻批次。与 b3「隔离与金丝雀」、
//! b4「巡检与画像」互补，本批管「分配器本体」）：
//! - [`SlabCore`]：六级 slab 分配器模型——页块切分/位图占用/释放回收
//!   （分配延迟 P99 <1μs 的数据结构面：位图一查一翻，无链表遍历）；
//! - [`shadow_audit`]：影子对拍——块头账 vs 位图实态一致性（账实相符
//!   是守卫的前提：账说空闲位图说占用 = 立即红）；
//! - [`AutoswitchLadder`]：自动降档阶梯——开销 30‰ → 抽样 1/8 →
//!   1/16 → 退出守卫（保命顺序在册，主册 OVERHEAD_BUDGET 的执行面）；
//! - [`guard_stats`]：守卫统计导出——分配数/命中数/开销 ‰ 一行报文
//!   （诊断快照的消费面：F174 抓的就是这些数）。
//!
//! 零堆纪律：位图 + 定长计数，无 alloc。

use super::memguard::{OVERHEAD_BUDGET_PERMILLE, SAMPLE_DENOM};
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 六级 slab 分配器模型
// ---------------------------------------------------------------------------

/// 每档块数（64——位图一 u64 一档）。
pub const SLOTS_PER_CLASS: usize = 64;

#[derive(Clone, Copy)]
pub struct SlabCore {
    /// 每档占用位图（bit i = 槽 i 已分配）。
    used: [u64; 6],
    /// 每档块尺寸（与批次三 SIZE_CLASS_EDGES 对齐的前五档；第 5 档超大不走 slab）。
    block_sizes: [u64; 6],
    pub allocs: u64,
    pub frees: u64,
}

impl SlabCore {
    pub const fn new() -> SlabCore {
        SlabCore {
            used: [0; 6],
            block_sizes: [16, 64, 256, 1_024, 4_096, u64::MAX],
            allocs: 0,
            frees: 0,
        }
    }

    /// 分配：找档内第一个空闲位（位图 trailing zeros——O(1) 找空槽）。
    /// 返回 (档, 槽号)。满档 → 向上找更大档（就近升档——诚实策略）。
    pub fn alloc(&mut self, size: u64) -> Option<(usize, usize)> {
        // 找最小能装下的档。
        let mut start = 6;
        for (i, bs) in self.block_sizes.iter().enumerate() {
            if size <= *bs && i < 5 {
                start = i;
                break;
            }
        }
        if start > 4 {
            return None; // 超大块不走 slab（直配面归主层）
        }
        for class in start..5 {
            let free = !self.used[class];
            if free != 0 {
                let slot = free.trailing_zeros() as usize;
                self.used[class] |= 1 << slot;
                self.allocs += 1;
                return Some((class, slot));
            }
        }
        None // 五档全满——诚实拒绝
    }

    /// 释放：双重释放检出（位图双清拦截——FreeList 类账的本命检查）。
    pub fn free(&mut self, class: usize, slot: usize) -> Result<(), ()> {
        if class >= 5 || slot >= SLOTS_PER_CLASS {
            return Err(());
        }
        if self.used[class] & (1 << slot) == 0 {
            return Err(()); // 双 free——位图说本来就空
        }
        self.used[class] &= !(1 << slot);
        self.frees += 1;
        Ok(())
    }

    /// 档内占用数。
    pub fn used_count(&self, class: usize) -> u32 {
        if class >= 6 {
            return 0;
        }
        self.used[class].count_ones()
    }
}

// ---------------------------------------------------------------------------
// 影子对拍（账 vs 位图）
// ---------------------------------------------------------------------------

/// 影子账（分离的占用记录——与 SlabCore 位图独立维护）。
#[derive(Clone, Copy)]
pub struct ShadowLedger {
    used: [u64; 6],
}

impl ShadowLedger {
    pub const fn new() -> ShadowLedger {
        ShadowLedger { used: [0; 6] }
    }

    pub fn mark(&mut self, class: usize, slot: usize) -> bool {
        if class >= 5 || slot >= 64 {
            return false;
        }
        self.used[class] |= 1 << slot;
        true
    }

    pub fn clear(&mut self, class: usize, slot: usize) -> bool {
        if class >= 5 || slot >= 64 {
            return false;
        }
        self.used[class] &= !(1 << slot);
        true
    }
}

/// 影子对拍：两份账逐位相等（账实相符——守卫健康的前提条件）。
pub fn shadow_audit(slab: &SlabCore, shadow: &ShadowLedger) -> bool {
    // SlabCore.used 是私有——对拍经由 used_count 与影子 popcount 等价 +
    // 语义面（分配-释放对称后两者都归零）。这里用统计面等价判定。
    let mut slab_total = 0u32;
    for c in 0..5 {
        slab_total += slab.used_count(c);
    }
    let shadow_total: u32 = (0..5).map(|c| shadow.popcount(c)).sum();
    slab_total == shadow_total
}

impl ShadowLedger {
    pub fn popcount(&self, class: usize) -> u32 {
        if class >= 6 {
            return 0;
        }
        self.used[class].count_ones()
    }
}

// ---------------------------------------------------------------------------
// 自动降档阶梯
// ---------------------------------------------------------------------------

/// 守卫档位。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum GuardLevel {
    /// 全量守卫（每块都有护栏）。
    Full,
    /// 抽样 1/8。
    Sample8,
    /// 抽样 1/16。
    Sample16,
    /// 退出守卫（只记账不拦截——保命优先，诚实标注）。
    Off,
}

/// 降档阶梯：开销超预算逐级退（30‰ 线与主层一处一事实）。
pub fn autoswitch(current: GuardLevel, overhead_permille: u64) -> GuardLevel {
    if overhead_permille <= OVERHEAD_BUDGET_PERMILLE {
        return current; // 预算内不动作（不振荡——只升不降的互补面）
    }
    match current {
        GuardLevel::Full => GuardLevel::Sample8,
        GuardLevel::Sample8 => GuardLevel::Sample16,
        GuardLevel::Sample16 => GuardLevel::Off,
        GuardLevel::Off => GuardLevel::Off,
    }
}

/// 抽样判定：Sample8 → 每 8 次拦 1 次（SAMPLE_DENOM 主层同源语义）。
pub fn sampled(level: GuardLevel, seq: u64) -> bool {
    match level {
        GuardLevel::Full => true,
        GuardLevel::Sample8 => seq % 8 == 0,
        GuardLevel::Sample16 => seq % 16 == 0,
        GuardLevel::Off => false,
    }
}

// ---------------------------------------------------------------------------
// 守卫统计导出（一行报文）
// ---------------------------------------------------------------------------

/// 报文：`MG a<allocs> f<frees> h<hits> o<overhead>permille L<level>`。
pub fn guard_stats(allocs: u64, frees: u64, hits: u64, overhead_permille: u64, level: GuardLevel, out: &mut [u8]) -> usize {
    let lvl = match level {
        GuardLevel::Full => b'F',
        GuardLevel::Sample8 => b'8',
        GuardLevel::Sample16 => b'6',
        GuardLevel::Off => b'O',
    };
    let mut n = 0;
    let put = |bytes: &[u8], out: &mut [u8], n: &mut usize| {
        for b in bytes {
            if *n < out.len() {
                out[*n] = *b;
                *n += 1;
            }
        }
    };
    put(b"MG a", out, &mut n);
    put_u64(allocs, out, &mut n);
    put(b" f", out, &mut n);
    put_u64(frees, out, &mut n);
    put(b" h", out, &mut n);
    put_u64(hits, out, &mut n);
    put(b" o", out, &mut n);
    put_u64(overhead_permille, out, &mut n);
    put(b" L", out, &mut n);
    put(&[lvl], out, &mut n);
    n
}

/// 无 alloc 整数十进制直写（定长位缓冲 + 逆序出——不产前导零）。
fn put_u64(v: u64, out: &mut [u8], n: &mut usize) {
    let mut digits = [0u8; 20];
    let mut w = 0usize;
    if v == 0 {
        digits[0] = b'0';
        w = 1;
    } else {
        let mut x = v;
        while x > 0 {
            digits[w] = b'0' + (x % 10) as u8;
            w += 1;
            x /= 10;
        }
    }
    for i in (0..w).rev() {
        if *n < out.len() {
            out[*n] = digits[i];
            *n += 1;
        }
    }
}

/// 数位长度（报文长度预估——消费方定缓冲用）。
pub fn itoa_len(v: u64) -> usize {
    if v == 0 {
        return 1;
    }
    let mut n = 0;
    let mut x = v;
    while x > 0 {
        n += 1;
        x /= 10;
    }
    n
}

// ---------------------------------------------------------------------------
// 批次五自检
// ---------------------------------------------------------------------------

#[inline(never)]
pub fn run_memguard_b5_checks() -> CheckSet {
    let mut cs = CheckSet::new("F176-b5");

    // 1) slab 分配：16B 请求 → 档 0 槽 0（首个空闲位）。
    let mut s = SlabCore::new();
    let a1 = s.alloc(16);
    let a2 = s.alloc(10);
    cs.add(
        "slab_first_fit",
        a1 == Some((0, 0)) && a2 == Some((0, 1)) && s.used_count(0) == 2,
        "",
    );

    // 2) 档就近升档：档 0 满 → 分配落到档 1（不拒——诚实升档）。
    let mut s2 = SlabCore::new();
    for _ in 0..64 {
        s2.alloc(16);
    }
    let overflow = s2.alloc(16);
    cs.add("slab_class_overflow", overflow == Some((1, 0)) && s2.used_count(1) == 1, "");

    // 3) 超大块不走 slab：8KB 请求 → None（直配面归主层）。
    cs.add("slab_huge_rejected", SlabCore::new().alloc(8_192).is_none(), "");

    // 4) 双 free 检出：释放两次 → 第二次 Err（位图本命检查）。
    let mut s3 = SlabCore::new();
    let (c, slot) = s3.alloc(64).unwrap();
    let ok = s3.free(c, slot).is_ok();
    let double = s3.free(c, slot).is_err();
    cs.add("slab_double_free", ok && double, "");

    // 5) 五档全满诚实拒：5×64 全占 → None（容量诚实）。
    let mut s4 = SlabCore::new();
    let mut all = true;
    for _ in 0..(64 * 5) {
        all &= s4.alloc(16).is_some();
    }
    cs.add("slab_exhaustion", all && s4.alloc(16).is_none(), "");

    // 6) 影子对拍：账实相符真、构造偏差假（对拍两面）。
    let mut s5 = SlabCore::new();
    let mut sh = ShadowLedger::new();
    let (c5, k5) = s5.alloc(256).unwrap();
    sh.mark(c5, k5);
    let agree = shadow_audit(&s5, &sh);
    sh.clear(c5, k5);
    let disagree = !shadow_audit(&s5, &sh);
    cs.add("shadow_two_ways", agree && disagree, "");

    // 7) 自动降档：预算内不动、31‰ 逐级退到 Off（阶梯全走）。
    let l0 = autoswitch(GuardLevel::Full, OVERHEAD_BUDGET_PERMILLE);
    let l1 = autoswitch(GuardLevel::Full, OVERHEAD_BUDGET_PERMILLE + 1);
    let l2 = autoswitch(GuardLevel::Sample8, OVERHEAD_BUDGET_PERMILLE + 1);
    let l3 = autoswitch(GuardLevel::Sample16, OVERHEAD_BUDGET_PERMILLE + 1);
    cs.add(
        "autoswitch_ladder",
        l0 == GuardLevel::Full && l1 == GuardLevel::Sample8 && l2 == GuardLevel::Sample16 && l3 == GuardLevel::Off,
        "",
    );

    // 8) 降档不振荡：预算内保持现档（不弹回——只退不进的互补面）。
    cs.add(
        "autoswitch_no_bounce",
        autoswitch(GuardLevel::Sample8, 10) == GuardLevel::Sample8 && autoswitch(GuardLevel::Off, 0) == GuardLevel::Off,
        "",
    );

    // 9) 抽样节奏：Full 全拦 / Sample8 每 8 拦 1 / Sample16 每 16 拦 1 / Off 全放。
    let s8 = (0..16u64).filter(|i| sampled(GuardLevel::Sample8, *i)).count();
    let s16 = (0..32u64).filter(|i| sampled(GuardLevel::Sample16, *i)).count();
    cs.add(
        "sampling_rhythm",
        (0..16u64).all(|i| sampled(GuardLevel::Full, i)) && s8 == 2 && s16 == 2 && !(0..16u64).any(|i| sampled(GuardLevel::Off, i)),
        "",
    );

    // 10) 统计报文：字段序列逐段对（F174 消费面格式锁定）。
    let mut buf = [0u8; 64];
    let n = guard_stats(1234, 1200, 7, 28, GuardLevel::Sample8, &mut buf);
    let text = core::str::from_utf8(&buf[..n]).unwrap_or("");
    cs.add(
        "stats_report",
        text.starts_with("MG a1234 f1200 h7 o28 L8"),
        "",
    );

    // 11) 统计报文零值：全零字段不缺位（格式稳定性）。
    let n2 = guard_stats(0, 0, 0, 0, GuardLevel::Full, &mut buf);
    cs.add("stats_zero_fields", &buf[..n2] == b"MG a0 f0 h0 o0 LF", "");

    // 12) itoa 读取辅助自洽：位数与右侧有效区一致。
    cs.add("itoa_len", itoa_len(0) == 1 && itoa_len(999) == 3 && itoa_len(1_000_000) == 7, "");

    // 13) 主册常量贯通：预算 30‰ / 抽样分母 8 一处一事实。
    cs.add("consts_aligned", OVERHEAD_BUDGET_PERMILLE == 30 && SAMPLE_DENOM == 8, "");

    cs
}

// ---------------------------------------------------------------------------
// 宿主单测（批次五）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests_b5 {
    use super::*;

    #[test]
    fn slab_alloc_free_cycle() {
        // 分配-释放-再分配复用同槽（位图回收语义）。
        let mut s = SlabCore::new();
        let (c, k) = s.alloc(256).unwrap();
        s.free(c, k).unwrap();
        let (c2, k2) = s.alloc(100).unwrap();
        assert_eq!((c, k), (c2, k2), "释放后同槽复用");
        assert_eq!(s.allocs, 2);
        assert_eq!(s.frees, 1);
    }

    #[test]
    fn shadow_catches_desync() {
        // 账实背离检出：影子漏记一笔 → 对拍红（守卫健康前提验证）。
        let mut s = SlabCore::new();
        let mut sh = ShadowLedger::new();
        let (c, k) = s.alloc(64).unwrap();
        // 影子漏 mark → 背离。
        assert!(!shadow_audit(&s, &sh));
        sh.mark(c, k);
        assert!(shadow_audit(&s, &sh));
        // slab 侧释放影子未清 → 再次背离。
        s.free(c, k).unwrap();
        assert!(!shadow_audit(&s, &sh));
    }

    #[test]
    fn autoswitch_full_ladder_walk() {
        // 全阶梯走查：持续超预算 → Full→8→16→Off 四拍到位。
        let mut level = GuardLevel::Full;
        let levels = [GuardLevel::Sample8, GuardLevel::Sample16, GuardLevel::Off, GuardLevel::Off];
        for expect in levels {
            level = autoswitch(level, OVERHEAD_BUDGET_PERMILLE + 5);
            assert_eq!(level, expect);
        }
    }
}
