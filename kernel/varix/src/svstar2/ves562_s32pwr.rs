//! VE-F4328 · 一致性功耗治理（U07 性能与预算组）
//! 职责：一致性检查功耗治理——功耗策略三件（后台扫描限频 / 批量合并 /
//! 电池模式降档）+ 功耗度量（能耗采样 + 单位扫描能耗指标——采样中值滤波
//! 去噪）+ 降档过度体验预警联动 + 策略冲突优先级仲裁 + Q 域续航联动声明。
//! 段位：0x3520（续表 idx562）。行数预算 340。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x3520;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PwCode {
    /// 限频拒绝（后台扫描频次超阈）。
    RateLimited = SEG_BASE,
    /// 采样偏差（中值滤波兜底）。
    SampleNoise = SEG_BASE + 0x01,
    /// 策略冲突（仲裁裁决）。
    PolicyClash = SEG_BASE + 0x02,
    /// 电源档越界。
    PowerRange = SEG_BASE + 0x03,
}

/// 电源档。
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug, PartialOrd, Ord)]
pub enum PowerTier {
    #[default]
    Plugged,
    Balanced,
    Battery,
    Saver,
}

/// 档位扫描间隔（tick 抽象单位——电池档最疏）。
pub fn tier_interval(t: PowerTier) -> u32 {
    match t {
        PowerTier::Plugged => 1,
        PowerTier::Balanced => 2,
        PowerTier::Battery => 5,
        PowerTier::Saver => 10,
    }
}

/// 中值滤波：奇数窗口取中位（偶数取下中位——整数安全）。
pub fn median_filter(vals: &[u32]) -> u32 {
    if vals.is_empty() {
        return 0;
    }
    let mut v = vals.to_vec();
    v.sort_unstable();
    v[v.len() / 2]
}

/// 功耗台。
#[derive(Default)]
pub struct PowerDesk {
    pub tier: PowerTier,
    /// 限频：窗口内已扫描次数 / 上限。
    pub scanned: u32,
    pub rate_cap: u32,
    /// 采样流。
    pub samples: Vec<u32>,
    /// 批量合并待扫队列。
    pub backlog: Vec<u32>,
    /// 仲裁记录。
    pub verdicts: Vec<u8>,
}

impl PowerDesk {
    pub fn new(tier: PowerTier, rate_cap: u32) -> Self {
        PowerDesk { tier, rate_cap: rate_cap.max(1), ..Default::default() }
    }

    /// 后台扫描请求：限频核验（窗口内超阈拒绝）。
    pub fn request_scan(&mut self) -> Result<(), PwCode> {
        if self.scanned >= self.rate_cap {
            return Err(PwCode::RateLimited);
        }
        self.scanned += 1;
        Ok(())
    }

    /// 电池降档：档位切换（越界防护在类型层）+ 队列合并强度随之增强。
    pub fn set_tier(&mut self, t: PowerTier) {
        self.tier = t;
    }

    /// 批量合并：待扫项满合并强度即批量放行（返回本批数量）。
    pub fn coalesce(&mut self) -> usize {
        let strength = match self.tier {
            PowerTier::Plugged => 4,
            PowerTier::Balanced => 8,
            PowerTier::Battery => 16,
            PowerTier::Saver => 32,
        };
        if self.backlog.len() < strength {
            return 0;
        }
        let n = self.backlog.len();
        self.backlog.clear();
        n
    }

    /// 能耗采样入流。
    pub fn feed(&mut self, mj: u32) {
        self.samples.push(mj);
    }

    /// 单位扫描能耗指标：中值滤波去噪后取均值。
    pub fn unit_cost(&self) -> u32 {
        if self.samples.is_empty() {
            return 0;
        }
        let mut clean: Vec<u32> = Vec::new();
        let w = 3usize;
        for i in 0..self.samples.len() {
            let lo = i.saturating_sub(w / 2);
            let hi = (i + w / 2 + 1).min(self.samples.len());
            clean.push(median_filter(&self.samples[lo..hi]));
        }
        let sum: u64 = clean.iter().map(|v| *v as u64).sum();
        (sum / clean.len() as u64) as u32
    }

    /// 采样噪声检出：原始均值与滤波均值偏差超阈 = 噪声污染。
    pub fn noisy(&self, tol_bp: u32) -> bool {
        if self.samples.is_empty() {
            return false;
        }
        let raw: u64 = self.samples.iter().map(|v| *v as u64).sum();
        let raw_avg = (raw / self.samples.len() as u64) as u32;
        let clean = self.unit_cost();
        let d = raw_avg.abs_diff(clean);
        raw_avg > 0 && d * 10000 / raw_avg > tol_bp
    }

    /// 策略冲突仲裁：省电档 vs 全量扫描需求——优先级裁决（省电赢=限频，
    /// 业务红线赢=放行并计数留痕）。
    pub fn arbitrate(&mut self, redline: bool) -> u8 {
        let v = if redline { 1 } else { 0 }; // 1=红线放行 0=省电限频
        self.verdicts.push(v);
        v
    }

    /// 降档过度预警：档位过低且积压未消 = 体验受损（预警联动）。
    pub fn over_throttled(&self) -> bool {
        self.tier >= PowerTier::Battery && !self.backlog.is_empty()
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves562_checks() -> CheckSet {
    let mut set = CheckSet::new("ves562");
    let mut pd = PowerDesk::new(PowerTier::Plugged, 3);

    // C562-01 限频窗口内放行。
    chk(&mut set, "C562-01", pd.request_scan().is_ok() && pd.scanned == 1, "限频内放行");

    // C562-02 超阈限频拒绝。
    let _ = pd.request_scan();
    let _ = pd.request_scan();
    chk(&mut set, "C562-02", pd.request_scan() == Err(PwCode::RateLimited), "超阈限频拒");

    // C562-03 档位间隔梯度（插电最密省电最疏）。
    chk(&mut set, "C562-03", tier_interval(PowerTier::Plugged) < tier_interval(PowerTier::Battery)
        && tier_interval(PowerTier::Battery) < tier_interval(PowerTier::Saver), "档位间隔梯度");

    // C562-04 电池降档批量合并强度增强。
    let mut pd2 = PowerDesk::new(PowerTier::Plugged, 100);
    for i in 0..4u32 {
        pd2.backlog.push(i);
    }
    chk(&mut set, "C562-04", pd2.coalesce() == 4, "插电档合并强度 4");
    pd2.set_tier(PowerTier::Battery);
    for i in 0..16u32 {
        pd2.backlog.push(i);
    }
    chk(&mut set, "C562-04b", pd2.coalesce() == 16, "电池档合并强度 16");

    // C562-05 积压不足合并强度不放行。
    pd2.set_tier(PowerTier::Saver);
    for i in 0..10u32 {
        pd2.backlog.push(i);
    }
    chk(&mut set, "C562-05", pd2.coalesce() == 0 && pd2.backlog.len() == 10, "积压不足不放行");

    // C562-06 中值滤波去噪（野点剔除）。
    let mut pd3 = PowerDesk::new(PowerTier::Balanced, 10);
    for v in [100, 102, 9900, 101, 103] {
        pd3.feed(v);
    }
    let uc = pd3.unit_cost();
    chk(&mut set, "C562-06", uc <= 110 && uc >= 90, "中值滤波剔野点");

    // C562-07 采样噪声检出。
    chk(&mut set, "C562-07", pd3.noisy(1000), "噪声检出");

    // C562-08 干净采样不误报。
    let mut pd4 = PowerDesk::new(PowerTier::Balanced, 10);
    for v in [100, 100, 100, 100] {
        pd4.feed(v);
    }
    chk(&mut set, "C562-08", !pd4.noisy(1000) && pd4.unit_cost() == 100, "干净采样不误报");

    // C562-09 策略冲突仲裁留痕（省电 vs 红线两向）。
    chk(&mut set, "C562-09", pd.arbitrate(true) == 1 && pd.arbitrate(false) == 0
        && pd.verdicts == [1, 0], "仲裁两向留痕");

    // C562-10 降档过度预警（低档+积压）。
    pd2.set_tier(PowerTier::Saver);
    chk(&mut set, "C562-10", pd2.over_throttled(), "降档过度预警");

    // C562-11 消积压后预警解除。
    for _ in 0..4 {
        pd2.backlog.pop();
    }
    chk(&mut set, "C562-11", pd2.backlog.len() == 6, "积压递减");
    for _ in 0..6 {
        pd2.backlog.pop();
    }
    chk(&mut set, "C562-11b", !pd2.over_throttled(), "消积压解除");

    // C562-12 空采样指标恒零（不臆测）。
    let pd5 = PowerDesk::new(PowerTier::Plugged, 1);
    chk(&mut set, "C562-12", pd5.unit_cost() == 0 && !pd5.noisy(100), "空采样不臆测");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_limit() {
        let mut pd = PowerDesk::new(PowerTier::Plugged, 1);
        assert!(pd.request_scan().is_ok());
        assert_eq!(pd.request_scan(), Err(PwCode::RateLimited));
    }

    #[test]
    fn median() {
        assert_eq!(median_filter(&[3, 1, 2]), 2);
        assert_eq!(median_filter(&[9, 1]), 1); // 偶数取下中位
        assert_eq!(median_filter(&[]), 0);
    }

    #[test]
    fn coalesce_by_tier() {
        let mut pd = PowerDesk::new(PowerTier::Saver, 10);
        for i in 0..32u32 {
            pd.backlog.push(i);
        }
        assert_eq!(pd.coalesce(), 32);
        assert!(pd.backlog.is_empty());
    }
}
