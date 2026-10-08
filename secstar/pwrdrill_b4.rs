//! F180 断电演练自动化 · 批次四深化（secstar · G-G-10）。
//!
//! 批次四功能面（与批次三互补：批次三管「漏跑与配置」，本批管
//! 「节奏与统计」）：
//! - [`RecoveryTimer`]：恢复计时统计——P50/P95/最大三读数（恢复均时
//!   5s 线的分布面：均时达标不代表 P95 达标）；
//! - [`NightCalendar`]：28 夜格子账——出勤/全绿/中止三色格，缺失格
//!   定位（4 周总账的日历可视化数据面）；
//! - [`BlindPair`]：双盲配对器——机器判/人工判两序列配对一致率
//!   ‰（20 轮双盲对拍的配对面：序对齐才算数）；
//! - [`PhaseCoverage`]：三态覆盖验证——Init/WritePeak/Idle 各档轮数
//!   计数（分布覆盖三态的执行面）。
//!
//! 零堆纪律：定长统计 + 定长日历，无 alloc。

use super::pwrdrill::{BLIND_AUDIT_ROUNDS, PowerPhase, RECOVERY_MEAN_TARGET_MS, ROUNDS_PER_NIGHT};
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 恢复计时统计
// ---------------------------------------------------------------------------

/// 样本容量（一夜 100 轮）。
pub const RECOVERY_SAMPLES: usize = ROUNDS_PER_NIGHT;

#[derive(Clone, Copy, Debug)]
pub struct RecoveryTimer {
    samples: [u32; RECOVERY_SAMPLES],
    n: usize,
}

impl RecoveryTimer {
    pub const fn new() -> RecoveryTimer {
        RecoveryTimer { samples: [0; RECOVERY_SAMPLES], n: 0 }
    }

    pub fn record(&mut self, ms: u32) -> bool {
        if self.n >= RECOVERY_SAMPLES {
            return false;
        }
        self.samples[self.n] = ms;
        self.n += 1;
        true
    }

    /// 排序副本（插入排序——100 元素足够）。
    fn sorted(&self) -> [u32; RECOVERY_SAMPLES] {
        let mut out = self.samples;
        for i in 1..self.n {
            let mut j = i;
            while j > 0 && out[j] < out[j - 1] {
                out.swap(j, j - 1);
                j -= 1;
            }
        }
        out
    }

    /// P50（中位数——偶数取下中位，诚实不插值）。
    pub fn p50(&self) -> Option<u32> {
        if self.n == 0 {
            return None;
        }
        let s = self.sorted();
        Some(s[self.n / 2 - 1 + self.n % 2])
    }

    /// P95（上 5% 分位——ceil 语义：95% 样本 ≤ 此值；idx = ceil(0.95n)-1）。
    pub fn p95(&self) -> Option<u32> {
        if self.n == 0 {
            return None;
        }
        let s = self.sorted();
        let idx = ((self.n * 95 + 99) / 100).saturating_sub(1);
        Some(s[idx.min(self.n - 1)])
    }

    pub fn max(&self) -> Option<u32> {
        self.samples[..self.n].iter().max().copied()
    }

    /// 均时（P99 口径外的均值——主册 RECOVERY_MEAN_TARGET 对照）。
    pub fn mean(&self) -> Option<u64> {
        if self.n == 0 {
            return None;
        }
        let sum: u64 = self.samples[..self.n].iter().map(|v| *v as u64).sum();
        Some(sum / self.n as u64)
    }

    /// 均时达标（主册 5s 线直算）。
    pub fn mean_on_target(&self) -> bool {
        self.mean().map(|m| m <= RECOVERY_MEAN_TARGET_MS).unwrap_or(false)
    }
}

// ---------------------------------------------------------------------------
// 28 夜日历
// ---------------------------------------------------------------------------

/// 日历周数。
pub const CAL_WEEKS: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NightCell {
    Missed,
    RanGreen,
    RanFailed,
    Aborted,
}

/// 4 周 × 7 夜格子账。
#[derive(Clone, Copy)]
pub struct NightCalendar {
    cells: [NightCell; CAL_WEEKS * 7],
}

impl NightCalendar {
    pub const fn all_missed() -> NightCalendar {
        NightCalendar { cells: [NightCell::Missed; CAL_WEEKS * 7] }
    }

    pub fn set(&mut self, week: usize, night: usize, c: NightCell) -> bool {
        if week >= CAL_WEEKS || night >= 7 {
            return false;
        }
        self.cells[week * 7 + night] = c;
        true
    }

    pub fn get(&self, week: usize, night: usize) -> Option<NightCell> {
        if week >= CAL_WEEKS || night >= 7 {
            return None;
        }
        Some(self.cells[week * 7 + night])
    }

    /// 缺失格位图（u32 低 28 位——漏跑定位面）。
    pub fn missing_bitmap(&self) -> u32 {
        let mut bm = 0u32;
        for (i, c) in self.cells.iter().enumerate() {
            if *c == NightCell::Missed {
                bm |= 1 << i;
            }
        }
        bm
    }

    /// 全勤判定：零缺失格（4 周 400 轮零漏跑的日历直算）。
    pub fn all_present(&self) -> bool {
        self.missing_bitmap() == 0
    }

    pub fn count(&self, c: NightCell) -> usize {
        self.cells.iter().filter(|x| **x == c).count()
    }
}

// ---------------------------------------------------------------------------
// 双盲配对器
// ---------------------------------------------------------------------------

/// 双盲配对：机器判/人工判两序列逐位对拍 → 一致率 ‰。
/// 序长不等 = 配对失败（None——不拿半截序列冒充一致率）。
pub fn blind_pair(machine: &[bool], human: &[bool]) -> Option<u32> {
    if machine.len() != human.len() || machine.is_empty() {
        return None;
    }
    let agree = machine.iter().zip(human.iter()).filter(|(a, b)| a == b).count();
    Some((agree * 1_000 / machine.len()) as u32)
}

/// 双盲批次常量（20 轮——与主层 BLIND_AUDIT_ROUNDS 同源）。
pub const BLIND_BATCH: usize = BLIND_AUDIT_ROUNDS;

// ---------------------------------------------------------------------------
// 三态覆盖验证
// ---------------------------------------------------------------------------

/// 三态覆盖账：Init/WritePeak/Idle 各档实际轮数。
#[derive(Clone, Copy, Debug, Default)]
pub struct PhaseCoverage {
    counts: [u32; 3],
}

impl PhaseCoverage {
    pub const fn new() -> PhaseCoverage {
        PhaseCoverage { counts: [0; 3] }
    }

    pub fn record(&mut self, p: PowerPhase) {
        self.counts[p.ord() as usize % 3] += 1;
    }

    pub fn count(&self, p: PowerPhase) -> u32 {
        self.counts[p.ord() as usize % 3]
    }

    /// 主册三态覆盖：每档 ≥ DIRECTED_PER_STATE（20）轮。
    pub fn all_directed_covered(&self, per_state: u32) -> bool {
        self.counts.iter().all(|c| *c >= per_state)
    }
}

// ---------------------------------------------------------------------------
// 批次四自检
// ---------------------------------------------------------------------------

#[inline(never)]
pub fn run_pwrdrill_b4_checks() -> CheckSet {
    let mut cs = CheckSet::new("F180-b4");

    // 1) 恢复统计四读数：已知样本 P50/P95/Max/Mean 全对（分布面）。
    let mut t = RecoveryTimer::new();
    for ms in [4_000u32, 5_000, 6_000, 5_200, 4_800] {
        assert!(t.record(ms));
    }
    cs.add(
        "recovery_stats",
        t.p50() == Some(5_000) && t.p95() == Some(6_000) && t.max() == Some(6_000) && t.mean() == Some(5_000),
        "",
    );

    // 2) 均时线：5s 恰好达标、5.2s 不达标（主册线直算两面）。
    let mut t2 = RecoveryTimer::new();
    t2.record(5_000);
    let at_line = t2.mean_on_target();
    t2.record(5_400);
    cs.add("recovery_target_line", at_line && !t2.mean_on_target(), "");

    // 3) 空统计不猜：无样本四读数全 None（不出数）。
    let e = RecoveryTimer::new();
    cs.add("recovery_empty_none", e.p50().is_none() && e.p95().is_none() && e.mean().is_none(), "");

    // 4) 满样本拒收：100 满后第 101 拒（一夜一轮纪律）。
    let mut t3 = RecoveryTimer::new();
    let mut all = true;
    for _ in 0..RECOVERY_SAMPLES {
        all &= t3.record(1_000);
    }
    cs.add("recovery_cap", all && !t3.record(1_000), "");

    // 5) 日历：登记/读取/越界拒（格子账三面）。
    let mut cal = NightCalendar::all_missed();
    let set_ok = cal.set(0, 0, NightCell::RanGreen) && cal.set(3, 6, NightCell::Aborted);
    let bad = cal.set(4, 0, NightCell::RanGreen) || cal.set(0, 7, NightCell::RanGreen);
    cs.add(
        "calendar_bounds",
        set_ok && !bad && cal.get(0, 0) == Some(NightCell::RanGreen) && cal.get(3, 6) == Some(NightCell::Aborted),
        "",
    );

    // 6) 缺失位图：28 全缺 = 低 28 位全 1；补 2 格后恰好缺 26（定位面）。
    let bm0 = NightCalendar::all_missed().missing_bitmap();
    let mut cal2 = NightCalendar::all_missed();
    cal2.set(0, 0, NightCell::RanGreen);
    cal2.set(2, 3, NightCell::RanFailed);
    let bm2 = cal2.missing_bitmap();
    cs.add(
        "calendar_missing_bitmap",
        bm0 == 0x0FFF_FFFF && bm2.count_ones() == 26 && !cal2.all_present(),
        "",
    );

    // 7) 全勤判定：28 格全填 → all_present（4 周零漏跑直算）。
    let mut cal3 = NightCalendar::all_missed();
    for w in 0..CAL_WEEKS {
        for n in 0..7 {
            cal3.set(w, n, NightCell::RanGreen);
        }
    }
    cs.add("calendar_all_present", cal3.all_present() && cal3.count(NightCell::RanGreen) == 28, "");

    // 8) 双盲配对：全对 1000‰ / 全错 0‰ / 半对 500‰（三锚）。
    let m = [true; BLIND_BATCH];
    let h = [true; BLIND_BATCH];
    let h_bad = [false; BLIND_BATCH];
    let h_half = [true, false, true, false, true, false, true, false, true, false, true, false, true, false, true, false, true, false, true, false];
    cs.add(
        "blind_pair_anchors",
        blind_pair(&m, &h) == Some(1_000)
            && blind_pair(&m, &h_bad) == Some(0)
            && blind_pair(&m, &h_half) == Some(500),
        "",
    );

    // 9) 双盲序长不等拒收：不拿半截序列冒充一致率。
    cs.add("blind_pair_len_guard", blind_pair(&[true, true], &[true]).is_none() && blind_pair(&[], &[]).is_none(), "");

    // 10) 三态覆盖：三档各 20 → 达标；缺一档 → 不达标（分布覆盖面）。
    let mut cov = PhaseCoverage::new();
    for _ in 0..20 {
        cov.record(PowerPhase::Init);
        cov.record(PowerPhase::WritePeak);
        cov.record(PowerPhase::Idle);
    }
    let full = cov.all_directed_covered(20);
    cov.record(PowerPhase::Init); // 只加不影响其他档
    let mut cov2 = PhaseCoverage::new();
    cov2.record(PowerPhase::Init);
    cs.add(
        "phase_coverage",
        full && cov.count(PowerPhase::Init) == 21 && !cov2.all_directed_covered(20),
        "",
    );

    // 11) 主册常量贯通：恢复均时线 5s / 一夜 100 轮 / 双盲 20 轮一处一事实。
    cs.add(
        "consts_aligned",
        RECOVERY_MEAN_TARGET_MS == 5_000 && ROUNDS_PER_NIGHT == 100 && BLIND_BATCH == 20,
        "",
    );

    cs
}

// ---------------------------------------------------------------------------
// 宿主单测（批次四）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests_b4 {
    use super::*;

    #[test]
    fn recovery_p95_ceil_semantics() {
        // P95 ceil 语义：20 样本 → 第 19 位（95% 样本 ≤ 此值）。
        let mut t = RecoveryTimer::new();
        for i in 1..=20u32 {
            t.record(i * 100);
        }
        // ceil(0.95*20)=19 → idx 18 → 值 1900。
        assert_eq!(t.p95(), Some(1_900));
        assert_eq!(t.p50(), Some(1_000)); // 下中位：idx 10 → 1100? n=20 → n/2-1+0 = 9 → 1000
    }

    #[test]
    fn calendar_three_colors() {
        // 三色格计数：绿/黄(失败)/中止各归各（日历不洗色）。
        let mut cal = NightCalendar::all_missed();
        cal.set(0, 0, NightCell::RanGreen);
        cal.set(0, 1, NightCell::RanFailed);
        cal.set(0, 2, NightCell::Aborted);
        assert_eq!((cal.count(NightCell::RanGreen), cal.count(NightCell::RanFailed), cal.count(NightCell::Aborted)), (1, 1, 1));
        assert_eq!(cal.count(NightCell::Missed), 25);
    }

    #[test]
    fn blind_pair_realistic_drift() {
        // 真实场景：20 轮中 19 对 1 错 = 950‰（双盲一致率非满分常态）。
        let m = [true; 20];
        let mut h = [true; 20];
        h[7] = false;
        assert_eq!(blind_pair(&m, &h), Some(950));
    }
}
