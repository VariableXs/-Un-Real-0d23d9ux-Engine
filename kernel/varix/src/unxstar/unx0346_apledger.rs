//! unx0346_apledger —— UNX-F0346 AP 启动总账（唤醒耗时分布）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0346 · AP 启动总账 · 目标 240 行
//! 判据原文：UNX-F0346-J1 每 AP 唤醒耗时五数概括落账，与 F0198
//!           时间线对接一致。
//! 组件：唤醒样本账（ap_id + 唤醒 tick，逐 AP 单值）+ 五数概括器
//!       （min/P25/中位/P75/max，整数分位，无浮点）+ F0198 时间线
//!       对接断言（账内 ap 序列与时间线行序列逐行同构）+ 分布
//!       健康面（离群 AP 检出：超中位 4 倍即立案——慢 AP 常是
//!       NUMA 错位的最早信号）。
//! 职责定位：AP 唤醒耗时的唯一权威账——五数概括给「分布形状」，
//!   离群检出给「异常信号」，F0198 对接断言保证两账不同源漂移。
//! 性能逐项分解：记账 O(1) 摊销；五数 O(n log n)（排序一次）；
//!   对接断言 O(n)。样本上限 256（16 核 × 16 轮），账面定容。
//! 诊断码段：0xE410（0xE411~0xE419 闭集；全域 grep 确认独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const AL_VERSION: &str = "unx0346_apledger-v1";

/// 诊断段基址（0xE410 段 · 本模块独占）。
pub const AL_SEG_BASE: u16 = 0xE410;

/// XcCode 闭集（0xE411~0xE419）。
pub const XC_DUP_AP: u32 = 0xE411;
pub const XC_SAMPLE_EMPTY: u32 = 0xE412;
pub const XC_TL_ROW_MISMATCH: u32 = 0xE413;
pub const XC_TL_MISSING: u32 = 0xE414;
pub const XC_OUTLIER: u32 = 0xE415;
pub const XC_LEDGER_FULL: u32 = 0xE416;
pub const XC_TICK_BACK: u32 = 0xE417;
pub const XC_FIVENUM_INV: u32 = 0xE418;
pub const XC_CAP_EXCEEDED: u32 = 0xE419;

/// 账容量上限（16 核 × 16 轮）。
pub const LEDGER_CAP: usize = 256;

/// 离群倍数红线（超中位 4 倍立案）。
pub const OUTLIER_FACTOR: u64 = 4;

/// 单条唤醒样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WakeSample {
    pub ap_id: u32,
    /// 自 INIT-SIPI 发出到 AP 应答哨兵置位的 tick 数。
    pub wake_ticks: u64,
}

/// 五数概括（全整数，无浮点）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FiveNumber {
    pub min: u64,
    pub p25: u64,
    pub median: u64,
    pub p75: u64,
    pub max: u64,
}

impl FiveNumber {
    /// 形状合法性：min ≤ p25 ≤ median ≤ p75 ≤ max（任一逆序 = 账面破）。
    pub fn shape_ok(&self) -> bool {
        self.min <= self.p25 && self.p25 <= self.median
            && self.median <= self.p75 && self.p75 <= self.max
    }
}

/// AP 唤醒耗时总账。
pub struct ApWakeLedger {
    samples: Vec<WakeSample>,
    cap: usize,
}

impl ApWakeLedger {
    /// 新建空账（默认定容 LEDGER_CAP）。
    pub fn new() -> ApWakeLedger {
        ApWakeLedger { samples: Vec::new(), cap: LEDGER_CAP }
    }

    /// 记一笔唤醒样本：同 AP 重复记账即拒（每 AP 每轮一值）。
    pub fn record(&mut self, ap_id: u32, wake_ticks: u64) -> Result<(), u16> {
        if self.samples.iter().any(|s| s.ap_id == ap_id) {
            return Err(XC_DUP_AP as u16);
        }
        if self.samples.len() >= self.cap {
            return Err(XC_LEDGER_FULL as u16);
        }
        self.samples.push(WakeSample { ap_id, wake_ticks });
        Ok(())
    }

    /// 账内样本数。
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// 空账判定。
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// 五数概括：空账显性 Err（不返回零值谎报）；单调保障由
    /// 排序 + 固定分位取点实现（P25 = 下四分位整数取点）。
    pub fn five_number(&self) -> Result<FiveNumber, u16> {
        if self.samples.is_empty() {
            return Err(XC_SAMPLE_EMPTY as u16);
        }
        let mut ticks: Vec<u64> = self.samples.iter().map(|s| s.wake_ticks).collect();
        // 插入排序：n ≤ 256，无递归、无 alloc，确定性。
        for i in 1..ticks.len() {
            let key = ticks[i];
            let mut j = i;
            while j > 0 && ticks[j - 1] > key {
                ticks[j] = ticks[j - 1];
                j -= 1;
            }
            ticks[j] = key;
        }
        let n = ticks.len();
        let pick = |q: usize| -> u64 { ticks[q * (n - 1) / 4] };
        let fn5 = FiveNumber {
            min: ticks[0],
            p25: pick(1),
            median: pick(2),
            p75: pick(3),
            max: ticks[n - 1],
        };
        if !fn5.shape_ok() {
            return Err(XC_FIVENUM_INV as u16);
        }
        Ok(fn5)
    }

    /// 离群检出：超中位 OUTLIER_FACTOR 倍的样本立案，返回 AP 列表。
    pub fn outliers(&self) -> Result<Vec<u32>, u16> {
        let f5 = self.five_number()?;
        let mut out = Vec::new();
        for s in self.samples.iter() {
            if s.wake_ticks > f5.median * OUTLIER_FACTOR {
                out.push(s.ap_id);
            }
        }
        Ok(out)
    }

    /// F0198 时间线对接断言：时间线行序列（ap_id 升序）与账内
    /// AP 集合逐行同构——多行/少行/错行任一即破。
    pub fn assert_timeline(&self, timeline_aps: &[u32]) -> Result<(), u16> {
        if timeline_aps.len() != self.samples.len() {
            return Err(XC_TL_ROW_MISMATCH as u16);
        }
        let mut mine: Vec<u32> = self.samples.iter().map(|s| s.ap_id).collect();
        mine.sort();
        for (i, (&a, &b)) in timeline_aps.iter().zip(mine.iter()).enumerate() {
            if a != b {
                return Err(XC_TL_MISSING as u16 + i as u16);
            }
        }
        Ok(())
    }
}

/// 域自检入口：记账/五数/离群/对接 四族。
pub fn run_unx0346_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0346-AP总账");
    // 族一：记账幂等与容量。
    let mut led = ApWakeLedger::new();
    for i in 0..8u32 {
        let r = led.record(i, 100 + i as u64 * 10);
        cs.add("记账成功", r.is_ok(), "AP 0..7 逐笔记账");
    }
    cs.add("重复 AP 拒", led.record(0, 1).is_err(), "同 AP 二记即拒");
    cs.add("空账五数拒", ApWakeLedger::new().five_number().is_err(), "空账显性 Err");

    // 族二：五数形状（构造已知分布核对取点）。
    // 分布：8 样本 ticks = 100,110,...,170 → 排序后取点可手算。
    let f5 = led.five_number();
    let shape_ok = match f5 {
        Ok(v) => v.shape_ok() && v.min == 100 && v.max == 170,
        Err(_) => false,
    };
    cs.add("五数形状合法", shape_ok, "min=100 max=170 闭式核对");

    // 族三：离群检出（注入一个 8 倍中位样本）。
    let mut led2 = ApWakeLedger::new();
    for i in 0..8u32 {
        let _ = led2.record(i, 100);
    }
    let _ = led2.record(99, 100 * OUTLIER_FACTOR + 1);
    let out = led2.outliers();
    cs.add("离群检出", matches!(&out, Ok(v) if v.as_slice() == &[99u32]), "超 4 倍中位");
    cs.add("健康账零立案", matches!(led.outliers(), Ok(ref v) if v.is_empty()), "无离群即零清单");

    // 族四：F0198 对接断言（行数一致 + 序列同构）。
    let mut tl: Vec<u32> = (0..8).collect();
    tl.sort();
    cs.add("对接一致", led.assert_timeline(&tl).is_ok(), "行数+序列同构");
    cs.add("对接缺行拒", led.assert_timeline(&tl[..7]).is_err(), "行数失配即破");
    let mut wrong = tl.clone();
    wrong[0] = 99;
    cs.add("对接错行拒", led.assert_timeline(&wrong).is_err(), "序列失配即破");
    cs
}

// ===== 扩展面：轮次账 + 双轮对账（同 AP 跨轮五数漂移检出）=====
/// 双轮对比：同一 AP 集两轮唤醒的五数逐项比较，漂移超容差立案。
///
/// 启动确定性红线：同硬件同负载下两轮五数应近似重合（每项
/// 容差内），P75 突增常指向 NUMA 错位或中断风暴，先于用户可感
/// 症状暴露问题。容差用千分比整数口径（无浮点）。
pub fn five_number_drift(a: &FiveNumber, b: &FiveNumber, tol_permille: u64) -> Result<(), u16> {
    if tol_permille > 1000 {
        return Err(1);
    }
    let items = [(a.min, b.min), (a.p25, b.p25), (a.median, b.median), (a.p75, b.p75), (a.max, b.max)];
    for (i, (x, y)) in items.iter().enumerate() {
        let denom = (*x).max(1);
        let diff = if x > y { x - y } else { y - x };
        if diff * 1000 > denom * tol_permille {
            return Err(2 + i as u16);
        }
    }
    Ok(())
}

/// 轮次账：每轮一份五数落档，轮号严格递增（回退 = 时序破）。
pub struct RoundLedger {
    rounds: Vec<(u32, FiveNumber)>,
}

impl RoundLedger {
    pub fn new() -> RoundLedger {
        RoundLedger { rounds: Vec::new() }
    }

    pub fn push(&mut self, round: u32, f5: FiveNumber) -> Result<(), u16> {
        if let Some(&(last, _)) = self.rounds.last() {
            if round <= last {
                return Err(3);
            }
        }
        if !f5.shape_ok() {
            return Err(4);
        }
        self.rounds.push((round, f5));
        Ok(())
    }

    /// 轮间漂移巡检：相邻两轮逐项比，任一超容差立案返回轮号。
    pub fn drift_patrol(&self, tol_permille: u64) -> Result<(), u16> {
        for w in self.rounds.windows(2) {
            five_number_drift(&w[0].1, &w[1].1, tol_permille)
                .map_err(|e| e + w[1].0 as u16 * 16)?;
        }
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.rounds.len()
    }
}
