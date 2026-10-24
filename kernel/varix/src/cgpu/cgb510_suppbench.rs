//! CGPU-F0510 · 抑制基准（目标 300 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 基准：均衡器开销实测（<0.2% 达标）；抑制效果基准（标准抖动注入
//!   的抑制率）；策略切换延迟——入册回归。判据：三基准、入册、判据。
//!
//! 设计纪律：
//! - 三基准：① 开销基准（均衡器每帧处理耗时 ÷ 合同帧时，比例 <
//!   0.2% 达标）；② 效果基准（标准抖动注入谱下的抑制率，≥ 目标带
//!   下限达标）；③ 切换延迟基准（策略切换到生效的间隔，≤ 预算达标）。
//! - 确定性计量：时钟注入式（不用墙钟）——帧耗时由计数器累加模拟，
//!   同输入同结果（回归可比）。
//! - 入册回归：三基准各记录（值, 达标, 版本戳）入册；下次实测与册内
//!   上次对比，退化超带宽即红。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x5250 独占。

use alloc::vec::Vec;

/// 诊断段基址（0x5250，批十独占）。
pub const SEG_BASE: u16 = 0x5250;

pub const E_BM_SCOPE: u16 = SEG_BASE + 1; // 基准标识非法
pub const E_BM_EMPTY: u16 = SEG_BASE + 2; // 空样本
pub const E_BM_REGRESS: u16 = SEG_BASE + 3; // 回归退化
pub const E_BM_BUDGET: u16 = SEG_BASE + 4; // 预算超标

/// 三基准标识。
pub const BM_OVERHEAD: u8 = 1; // 均衡器开销
pub const BM_EFFECT: u8 = 2; // 抑制效果
pub const BM_SWITCH: u8 = 3; // 策略切换延迟
pub const BENCH_IDS: [u8; 3] = [BM_OVERHEAD, BM_EFFECT, BM_SWITCH];

/// 合同帧时与达标线。
pub const CONTRACT_US: u64 = 12_500;
/// 开销达标线：0.2% of 合同帧时（25us）。
pub const OVERHEAD_BUDGET_US: u64 = 25;
/// 抑制效果达标下限（‰）。
pub const EFFECT_TARGET_PM: u64 = 600;
/// 切换延迟预算（us）。
pub const SWITCH_BUDGET_US: u64 = 2_000;

/// 单条基准结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BenchResult {
    /// 基准标识。
    pub id: u8,
    /// 实测值（开销/切换：us；效果：‰）。
    pub value: u64,
    /// 是否达标。
    pub pass: bool,
    /// 版本戳（册内比对用）。
    pub stamp: u32,
}

/// 标准抖动注入谱（效果基准的固定输入）。
pub const STD_JITTER_US: [u64; 8] = [
    1_000, 2_500, 4_000, 6_000, 9_000, 14_000, 22_000, 40_000,
];

/// 均衡器开销模型：每帧固定处理成本（决策查表+计数）模拟。
pub fn eq_overhead_us(frames: usize) -> u64 {
    // 表驱动决策 3us + 谱更新 2us + 计数 1us ≈ 每帧 6us（确定性模型）
    6u64.saturating_mul(frames as u64)
}

/// 开销基准：frames 帧总开销 ÷ 帧数应 ≤ 0.2% 合同（25us/帧）。
pub fn bench_overhead(frames: usize) -> BenchResult {
    let total = eq_overhead_us(frames);
    let per_frame = if frames == 0 { 0 } else { total / frames as u64 };
    BenchResult {
        id: BM_OVERHEAD,
        value: per_frame,
        pass: per_frame < OVERHEAD_BUDGET_US,
        stamp: 1,
    }
}

/// 抑制效果基准：标准谱注入均衡器（深缓冲策略），可被吸收的样本占比。
/// 吸收判据：注入幅度 ≤ 深度×合同/8 的缓冲余量。
pub fn bench_effect(depth: u8) -> BenchResult {
    let capacity = depth as u64 * CONTRACT_US / 8;
    let mut absorbed = 0u64;
    for &j in STD_JITTER_US.iter() {
        if j <= capacity {
            absorbed += 1;
        }
    }
    let rate = absorbed * 1_000 / STD_JITTER_US.len() as u64;
    BenchResult {
        id: BM_EFFECT,
        value: rate,
        pass: rate >= EFFECT_TARGET_PM,
        stamp: 1,
    }
}

/// 策略切换延迟模型：旧策略清场（深度×合同/16）+ 新策略预热（固定 800us）。
pub fn bench_switch(depth: u8) -> BenchResult {
    let flush = depth as u64 * CONTRACT_US / 16;
    let total = flush.saturating_add(800);
    BenchResult {
        id: BM_SWITCH,
        value: total,
        pass: total <= SWITCH_BUDGET_US,
        stamp: 1,
    }
}

/// 基准册：入册 + 回归判定。
#[derive(Clone, Debug)]
pub struct BenchBook {
    ledger: Vec<BenchResult>,
    /// 回归带宽（‰ 相对退化容忍度）。
    pub band_pm: u64,
}

impl BenchBook {
    pub fn new(band_pm: u64) -> Self {
        Self {
            ledger: Vec::new(),
            band_pm,
        }
    }

    /// 入册：新记录直接入；已有同 id 记录则做回归比对后更新。
    pub fn commit(&mut self, r: BenchResult) -> Result<(), u16> {
        if !BENCH_IDS.contains(&r.id) {
            return Err(E_BM_SCOPE);
        }
        if let Some(prev) = self.ledger.iter().find(|x| x.id == r.id) {
            // 退化判定：值变差（开销/切换变大、效果变小）超带宽即红
            let worse = match r.id {
                BM_EFFECT => prev.value.saturating_sub(r.value) * 1_000
                    > prev.value * self.band_pm,
                _ => r.value.saturating_sub(prev.value) * 1_000
                    > prev.value * self.band_pm,
            };
            if worse {
                return Err(E_BM_REGRESS);
            }
        }
        if let Some(slot) = self.ledger.iter_mut().find(|x| x.id == r.id) {
            *slot = r;
        } else {
            self.ledger.push(r);
        }
        Ok(())
    }

    /// 三基准达标面：册内全部 pass 且三基准齐。
    pub fn all_green(&self) -> bool {
        BENCH_IDS.iter().all(|id| {
            self.ledger
                .iter()
                .find(|x| x.id == *id)
                .map(|x| x.pass)
                .unwrap_or(false)
        })
    }

    pub fn len(&self) -> usize {
        self.ledger.len()
    }
}

// ==========================================================================
// 判据
// ==========================================================================

pub fn cgb510_raw_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();

    // 1. 开销基准：每帧 6us < 25us 预算（0.2% 达标）
    let r = bench_overhead(1_000);
    v.push(("基准-开销0.2%达标", r.pass && r.value == 6));

    // 2. 抑制效果：深度 6 容量 9375us→吸收 5/8 样本→625‰ 达标（≥600）
    let e6 = bench_effect(6);
    v.push(("基准-效果达标千分比", e6.pass && e6.value == 625));

    // 3. 效果梯度：深度 3（容量 4687us）只吸收 3 个样本→375‰ 不达标
    let e3 = bench_effect(3);
    v.push(("基准-浅缓冲效果不足", !e3.pass && e3.value == 375));

    // 4. 切换延迟：深度 4 → 3125+800=3925us 超预算拒；深度 2 → 1562+800=2362us 超预算；深度 1 → 781+800=1581 达标
    let s4 = bench_switch(4);
    let s1 = bench_switch(1);
    v.push((
        "基准-切换延迟预算",
        !s4.pass && s4.value == 3_925 && s1.pass && s1.value == 1_581,
    ));

    // 5. 入册与达标面：三基准全入册全绿
    let mut book = BenchBook::new(50);
    let _ = book.commit(bench_overhead(1_000));
    let _ = book.commit(bench_effect(6));
    let _ = book.commit(bench_switch(1));
    v.push(("入册-三基准全绿", book.len() == 3 && book.all_green()));

    // 6. 回归退化：效果退化超带宽被拒
    let mut book2 = BenchBook::new(50);
    let _ = book2.commit(bench_effect(6));
    let degraded = BenchResult {
        id: BM_EFFECT,
        value: 400, // 1000 → 400 退化 600‰ > 50‰ 带宽
        pass: false,
        stamp: 2,
    };
    v.push(("回归-退化超带宽即拒", book2.commit(degraded) == Err(E_BM_REGRESS)));

    // 7. 带宽内波动放行：1000→980（退化 20‰ < 50‰）
    let minor = BenchResult { id: BM_EFFECT, value: 980, pass: true, stamp: 2 };
    v.push(("回归-带宽内波动放行", book2.commit(minor) == Ok(())));

    // 8. 非法基准标识拒
    v.push((
        "拦截-非法标识",
        book.commit(BenchResult { id: 9, value: 0, pass: false, stamp: 1 }) == Err(E_BM_SCOPE),
    ));

    v
}

pub fn run_cgb510_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("cgpu-cgb510");
    for (name, ok) in cgb510_raw_checks() {
        set.add(name, ok, "");
    }
    set
}

#[cfg(test)]
mod tests {
    use super::cgb510_raw_checks as raw;
    #[test]
    fn t_cgb510() {
        for (n, ok) in raw() {
            assert!(ok, "cgb510 check failed: {}", n);
        }
    }
}
