//! H2 磁盘空间滚动账 · 深化批次六（F268 深化——每日空间采样、
//! 趋势斜率、预计满盘日期；F060 车道经 F268 锚落位）。
//!
//! **承接判据**（主册 H 域正文，一处一事实）：
//! - **F268 磁盘空间预警**：每日一次提醒节流——本层是节流的数据
//!   面补充：每日一采（同日重复采样覆盖旧值——一天一条不膨胀）、
//!   趋势斜率（最小二乘——h2ledger 车道同法）、预计满盘日期；
//! - **十二章「诚实预测」**：样本 <2 不预测（两点才成线——不编
//!   造趋势）；斜率为负或零 → 无满盘风险（如实说「无增长」）；
//!   预测窗口外（>365 天）→ 「暂无风险」不精确到天。
//!
//! 时间纪律：天序戳（天 = 分 / 1440）注入；账本定容（90 天——
//! 趋势窗口够用，账本不无限膨胀）。

use crate::checks::CheckSet;

use alloc::vec::Vec;

/// 账本容量（天）。
pub const ROLL_CAP_DAYS: usize = 90;
/// 预测显示上限（天——超出按「暂无风险」口径）。
pub const FORECAST_CAP_DAYS: u64 = 365;

/// 一天一条采样：天序 + 已用字节。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DaySample {
    pub day: u64,
    pub used_bytes: u64,
}

/// 空间滚动账。
pub struct DiskRoll {
    samples: Vec<DaySample>,
    pub capacity: u64,
}

impl DiskRoll {
    pub fn new(capacity: u64) -> DiskRoll {
        DiskRoll { samples: Vec::new(), capacity: capacity.max(1) }
    }

    /// 记一笔：同日覆盖（每日一条），满容淘汰最旧。
    pub fn record(&mut self, day: u64, used: u64) {
        self.samples.retain(|s| s.day != day);
        self.samples.push(DaySample { day, used_bytes: used.min(self.capacity) });
        self.samples.sort_by_key(|s| s.day);
        if self.samples.len() > ROLL_CAP_DAYS {
            let drop = self.samples.len() - ROLL_CAP_DAYS;
            self.samples.drain(..drop);
        }
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// 最近 `n` 天用量（诊断页口径）。
    pub fn recent(&self, n: usize) -> &[DaySample] {
        let take = self.samples.len().saturating_sub(n);
        &self.samples[take..]
    }

    /// 趋势预测：最小二乘斜率（字节/天）+ 预计满盘天数。
    /// 返回 None = 样本不足或无增长（诚实——不编数）。
    pub fn forecast(&self) -> Option<(u64, u64)> {
        let n = self.samples.len();
        if n < 2 {
            return None;
        }
        let first = self.samples[0].day as i64;
        // x = 天序偏移，y = 用量。
        let sx: i64 = self.samples.iter().map(|s| s.day as i64 - first).sum();
        let sy: i64 = self.samples.iter().map(|s| s.used_bytes as i64).sum();
        let sxx: i64 = self.samples.iter().map(|s| {
            let x = s.day as i64 - first;
            x * x
        }).sum();
        let sxy: i64 = self.samples.iter().map(|s| {
            (s.day as i64 - first) * s.used_bytes as i64
        }).sum();
        let denom = (n as i64) * sxx - sx * sx;
        if denom == 0 {
            return None;
        }
        let slope = ((n as i64) * sxy - sx * sy) as f64 / denom as f64;
        if slope <= 0.0 {
            return None; // 无增长或回落——无满盘风险
        }
        let last = self.samples.last()?;
        let remaining = self.capacity.saturating_sub(last.used_bytes) as f64;
        let days = (remaining / slope) as u64;
        if days > FORECAST_CAP_DAYS {
            return None; // 窗口外——「暂无风险」不精确到天
        }
        Some((days, slope as u64))
    }

    /// 今日是否已采（F268 每日节流的查询面）。
    pub fn sampled_today(&self, day: u64) -> bool {
        self.samples.iter().any(|s| s.day == day)
    }
}

// ---------------------------------------------------------------------------
// 自检（判据逐条钉死）
// ---------------------------------------------------------------------------

pub fn run_h2diskroll_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-h2diskroll");
    let cap = 100 * 1024 * 1024 * 1024; // 100GiB
    let mut roll = DiskRoll::new(cap);
    // 每日一条：同日重复覆盖（不膨胀）。
    let mut ov = DiskRoll::new(cap);
    ov.record(100, 10 * 1024 * 1024 * 1024);
    ov.record(100, 11 * 1024 * 1024 * 1024);
    set.add(
        "h2diskroll same-day overwrite",
        ov.len() == 1 && ov.recent(1)[0].used_bytes == 11 * 1024 * 1024 * 1024,
        "one sample per day",
    );
    // 线性增长：干净账每天 +1GiB（50→59），斜率恰 1 → 满盘 ≈41 天。
    for i in 0..10u64 {
        roll.record(100 + i, (50 + i) * 1024 * 1024 * 1024);
    }
    let f = roll.forecast();
    set.add(
        "h2diskroll linear forecast",
        f.map(|(days, _)| (40..=60).contains(&days)).unwrap_or(false),
        "1GiB/day slope",
    );
    // 无增长：斜率 ≤0 → None（不编趋势）。
    let mut flat = DiskRoll::new(cap);
    flat.record(1, 10);
    flat.record(2, 10);
    flat.record(3, 10);
    set.add(
        "h2diskroll flat honest",
        flat.forecast().is_none(),
        "no growth no risk",
    );
    // 样本不足：单点不预测。
    let mut one = DiskRoll::new(cap);
    one.record(1, 10);
    set.add(
        "h2diskroll single point",
        one.forecast().is_none(),
        "two points or nothing",
    );
    // 窗口外：增长极缓（每天 1 字节）→ None（「暂无风险」口径）。
    let mut slow = DiskRoll::new(cap);
    slow.record(1, 1000);
    slow.record(2, 1001);
    slow.record(3, 1002);
    set.add(
        "h2diskroll far horizon",
        slow.forecast().is_none(),
        ">365d not predicted",
    );
    // 今日已采查询（节流面）。
    set.add(
        "h2diskroll sampled today",
        roll.sampled_today(109) && !roll.sampled_today(999),
        "throttle query",
    );
    // 满容淘汰最旧（90 天窗）。
    let mut big = DiskRoll::new(cap);
    for d in 0..120u64 {
        big.record(d, 100);
    }
    set.add(
        "h2diskroll cap eviction",
        big.len() == ROLL_CAP_DAYS && big.recent(1)[0].day == 119,
        "90-day window",
    );
    set.add(
        "h2diskroll consts",
        ROLL_CAP_DAYS == 90 && FORECAST_CAP_DAYS == 365,
        "window constants",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn h2diskroll_all_green() {
        let set = run_h2diskroll_checks();
        assert!(set.all_passed(), "h2diskroll 自检有红项");
        assert!(!set.truncated(), "h2diskroll 自检溢出");
    }

    #[test]
    fn noisy_data_never_panics() {
        // 锯齿数据（交替涨落）500 天：斜率可算则算、不可算则 None——不炸。
        let mut roll = DiskRoll::new(1024);
        for d in 0..500u64 {
            roll.record(d, if d % 2 == 0 { 500 } else { 100 });
        }
        let _ = roll.forecast();
    }
}
