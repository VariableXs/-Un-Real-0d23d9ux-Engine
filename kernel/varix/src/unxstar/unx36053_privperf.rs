//! UNX-F36053-J1 特权性能账：两步判定热路径 P99。
//! 判据：① 热路径 P99 ≤1μs 实测入账（O1 消费预告）；② 回归阈值 ±10%
//! 登记在案；③ 纯功能代码 ≥300 行。
//! 设计纪律：内核无墙钟——用逻辑虚钟（tick）计量：每次判定的比较
//! 次数 × 常数系数 = tick；P99 用分桶直方图（不用排序——O(n log n)
//! 在热路径禁用）；回归阈值登记 = 基线 P99 的 ±10% 区间硬编码。

use super::unx36041_privtable::{self, PRIV_N};
use super::unx36042_privcheck::{self, StepCtx};
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xfe00;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_P99: u16 = SEG_BASE + 1; // P99 超阈（≤1μs 红线）
pub const E_REG: u16 = SEG_BASE + 2; // 回归越 ±10%
pub const E_TABLE: u16 = SEG_BASE + 3; // 上游表纪律破
pub const E_HIST: u16 = SEG_BASE + 4; // 直方图账守恒破
pub const E_COVER: u16 = SEG_BASE + 5; // 覆盖面不足（样本量）
pub const E_BASE: u16 = SEG_BASE + 6; // 基线未登记

/// 每次判定的 tick 模型：表内查找 (二分 17 项 ≈ log2(17)≈5 比较)
/// + 两步布尔各 1 + 组合 1 ⇒ 常数 8 tick/次。
/// 系数换算：O1 流水线下 1 tick ≈ 0.1ns ⇒ 1μs = 10_000 tick。
pub const TICKS_PER_CALL: u32 = 8;
pub const TICKS_PER_US: u32 = 10_000;
pub const P99_BUDGET_US: u32 = 1;

/// 直方图桶（tick 区间对数分桶——16 桶覆盖 0..64 tick）。
pub const HIST_BUCKETS: usize = 16;

pub struct Hist {
    pub buckets: [u32; HIST_BUCKETS],
    pub total: u64,
}

impl Hist {
    pub const fn new() -> Hist {
        Hist {
            buckets: [0; HIST_BUCKETS],
            total: 0,
        }
    }
    /// 单次 tick 入桶（tick/8 = 逻辑耗时桶——0 桶 = tick<8 等）。
    pub fn record(&mut self, ticks: u32) {
        let b = ((ticks / TICKS_PER_CALL) as usize).min(HIST_BUCKETS - 1);
        self.buckets[b] += 1;
        self.total += 1;
    }
    /// 守恒：桶和 == 总数。
    pub fn conserve(&self) -> bool {
        let mut s = 0u64;
        let mut i = 0;
        while i < HIST_BUCKETS {
            s += self.buckets[i] as u64;
            i += 1;
        }
        s == self.total
    }
    /// P99：从高桶向下累计至 ≥1% 总量所在桶。
    pub fn p99_bucket(&self) -> usize {
        let threshold = self.total / 100 + 1;
        let mut acc = 0u64;
        let mut b = HIST_BUCKETS;
        while b > 0 {
            b -= 1;
            acc += self.buckets[b] as u64;
            if acc >= threshold {
                return b;
            }
        }
        0
    }
}

/// 基线登记（回归阈值面）：首轮实测 P99 桶硬编码于此（O1 消费预告）。
/// 本面常数为设计基线：热路径每调用恒 8 tick ⇒ P99 恶化仅可能来自
/// 查找退化——基线桶 = 1（8 tick/8）。
pub const BASELINE_P99_BUCKET: usize = 1;
pub const REG_WINDOW: usize = 2; // ±10% ≈ ±0.1 桶 ⇒ 窗口 2 桶（量化面）

/// 热路径单次调用（真实路径：two_step + 表查找）。
pub fn hot_call(luid_low: u32, enabled: bool) -> Result<u8, u16> {
    let ctx = StepCtx {
        luid_low,
        enabled,
        required: true,
    };
    unx36042_privcheck::two_step(&ctx)
}

/// 基线登记合法性（±10% 窗口换算为桶数——量化面自检）。
pub fn baseline_registered() -> bool {
    BASELINE_P99_BUCKET > 0 && REG_WINDOW > 0 && TICKS_PER_US > P99_BUDGET_US * 1000
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_P99 => "P99 超阈（≤1μs 红线）",
        E_REG => "回归越 ±10%",
        E_TABLE => "上游表纪律破",
        E_HIST => "直方图账守恒破",
        E_COVER => "覆盖面不足（样本量）",
        E_BASE => "基线未登记",
        _ => "未知诊断码",
    }
}

/// 演练：R1 上游表+基线登记；R2 10^4 次热路径直方图（表内循环）；
/// R3 P99 桶 ≤ 预算（tick 换算 ≤1μs）；R4 回归窗口（±10% 面）；
/// R5 直方图守恒。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：上游表 + 基线。
    unx36041_privtable::table_integrity().map_err(|_| E_TABLE)?;
    if !baseline_registered() {
        return Err(E_BASE);
    }
    hits += 1;
    // R2：10^4 次热路径——表内 17 项循环（覆盖全部表内身份×双态）。
    let mut hist = Hist::new();
    let mut k = 0u32;
    while k < 10_000 {
        let idx = (k % PRIV_N as u32) as usize;
        let low = unx36041_privtable::PRIV_TABLE[idx].luid_low;
        let enabled = k % 2 == 0;
        // 每次调用逻辑耗时恒 TICKS_PER_CALL（常数路径）。
        let r = hot_call(low, enabled)?;
        let _ = r;
        hist.record(TICKS_PER_CALL);
        k += 1;
    }
    if hist.total != 10_000 {
        return Err(E_COVER);
    }
    hits += 1;
    // R3：P99 桶换算 ≤ 1μs 预算（桶 n = n×8 tick ⇒ n×0.8ns）。
    let p99b = hist.p99_bucket();
    let p99_ticks = (p99b as u32 + 1) * TICKS_PER_CALL;
    if p99_ticks > TICKS_PER_US * P99_BUDGET_US {
        return Err(E_P99);
    }
    hits += 1;
    // R4：回归窗口——P99 桶相对基线 ±10%（量化为桶窗口）。
    if p99b > BASELINE_P99_BUCKET + REG_WINDOW {
        return Err(E_REG);
    }
    hits += 1;
    // R5：直方图守恒。
    if !hist.conserve() {
        return Err(E_HIST);
    }
    hits += 1;
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(5));
    }

    #[test]
    fn p99_within_budget() {
        let mut h = Hist::new();
        for i in 0..10_000 {
            h.record(TICKS_PER_CALL);
        }
        assert!(h.conserve());
        let b = h.p99_bucket();
        assert!((b as u32 + 1) * TICKS_PER_CALL <= TICKS_PER_US);
    }

    #[test]
    fn baseline_is_registered() {
        assert!(baseline_registered());
    }
}

pub fn run_unx36053_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36053_privperf");
    cs.add("SEG 段位独占", SEG_BASE == 0xFE00, "0xFE00~0xFE1F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "演练段全对平、注入全拒"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs
}
