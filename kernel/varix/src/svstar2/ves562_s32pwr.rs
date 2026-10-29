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
    /// 能耗预算击穿（消耗超分配）。
    BudgetBroke = SEG_BASE + 0x04,
    /// 滞回锁定（档位切换冷却中）。
    HysteresisLock = SEG_BASE + 0x05,
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

// ─── 深化块：能耗预算账本（allocated ≡ spent + remaining 守恒）───

/// 能耗预算账本：每窗分配 mj 预算，消耗留痕，结余可查。
/// 守恒恒等式 allocated ≡ spent + remaining 是本块核心性质。
#[derive(Default)]
pub struct BudgetBook {
    /// 累计分配（mj）。
    pub allocated: u64,
    /// 累计消耗（mj）。
    pub spent: u64,
    /// 每窗消耗流（供守恒对账）。
    pub ledger: Vec<u64>,
}

impl BudgetBook {
    pub fn new() -> Self {
        BudgetBook { allocated: 0, spent: 0, ledger: Vec::new() }
    }

    /// 分配一窗预算。
    pub fn alloc(&mut self, mj: u64) {
        self.allocated += mj;
    }

    /// 记一笔消耗；超结余即预算击穿（显性码，不静默钳制）。
    pub fn spend(&mut self, mj: u64) -> Result<(), PwCode> {
        let remaining = self.allocated - self.spent.min(self.allocated);
        if mj > remaining {
            return Err(PwCode::BudgetBroke);
        }
        self.spent += mj;
        self.ledger.push(mj);
        Ok(())
    }

    /// 结余。
    pub fn remaining(&self) -> u64 {
        self.allocated.saturating_sub(self.spent)
    }

    /// 守恒对账：账面 spent ≡ 流水合计（篡改即检出）。
    pub fn conserves(&self) -> bool {
        let flow: u64 = self.ledger.iter().sum();
        flow == self.spent && self.spent <= self.allocated
    }
}

// ─── 深化块：降档滞回（防档位振荡）───

/// 滞回切换器：升/降档需连续 need tick 同向请求才生效，
/// 冷却期内拒绝切换（HysteresisLock），防负载抖动导致档位振荡。
#[derive(Clone, Copy)]
pub struct Hysteresis {
    pub need: u8,
    up_run: u8,
    down_run: u8,
    pub cooldown: u8,
}

impl Hysteresis {
    pub fn new(need: u8, cooldown: u8) -> Self {
        Hysteresis { need: need.max(1), up_run: 0, down_run: 0, cooldown }
    }

    /// 请求升档：连续 need 次即放行并进入冷却。
    pub fn request_up(&mut self) -> Result<(), PwCode> {
        if self.cooldown > 0 {
            return Err(PwCode::HysteresisLock);
        }
        self.up_run += 1;
        self.down_run = 0;
        if self.up_run >= self.need {
            self.up_run = 0;
            self.cooldown = self.cooldown.max(1);
            return Ok(());
        }
        Err(PwCode::HysteresisLock)
    }

    /// 请求降档：同向计数互斥（升档计数清零）。
    pub fn request_down(&mut self) -> Result<(), PwCode> {
        if self.cooldown > 0 {
            return Err(PwCode::HysteresisLock);
        }
        self.down_run += 1;
        self.up_run = 0;
        if self.down_run >= self.need {
            self.down_run = 0;
            self.cooldown = self.cooldown.max(1);
            return Ok(());
        }
        Err(PwCode::HysteresisLock)
    }

    /// 每 tick 推进冷却。
    pub fn tick(&mut self) {
        self.cooldown = self.cooldown.saturating_sub(1);
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

    // ── 深化判据：预算账本 ──
    let mut bb = BudgetBook::new();
    bb.alloc(1000);
    bb.alloc(500);
    chk(&mut set, "C562-13", bb.spend(600).is_ok() && bb.spend(400).is_ok(),
        "预算内消耗两笔放行");
    chk(&mut set, "C562-14", bb.spend(1) == Err(PwCode::BudgetBroke), "结余尽即击穿显性");
    chk(&mut set, "C562-15", bb.conserves() && bb.allocated == 1500
        && bb.spent == 1000 && bb.remaining() == 500, "allocated≡spent+remaining 守恒");

    // 篡改检出：流水改一笔即守恒破。
    let mut bb2 = BudgetBook::new();
    bb2.alloc(100);
    let _ = bb2.spend(30);
    bb2.ledger[0] = 40;
    chk(&mut set, "C562-16", !bb2.conserves(), "流水篡改守恒即破");

    // ── 深化判据：降档滞回 ──
    let mut hys = Hysteresis::new(3, 2);
    chk(&mut set, "C562-17", hys.request_up().is_err() && hys.request_up().is_err()
        && hys.request_up().is_ok(), "连续 3 tick 升档放行");
    chk(&mut set, "C562-18", hys.request_down() == Err(PwCode::HysteresisLock),
        "冷却期内切换锁定");
    hys.tick();
    hys.tick();
    chk(&mut set, "C562-19", hys.request_down().is_err() && hys.request_down().is_ok(),
        "冷却解除后连续 2 tick 降档放行");
    // 反向计数互斥：升 2 后降 1 → 升计数清零需重计。
    let mut hys2 = Hysteresis::new(3, 0);
    let _ = hys2.request_up();
    let _ = hys2.request_up();
    let _ = hys2.request_down();
    chk(&mut set, "C562-20", hys2.request_up().is_err(), "反向请求打断同向连击");

    // ── 深化判据：野点注入回归（LCG 驱动，必抓 100%）──
    let mut rng: u64 = 0x5620_4155;
    let mut caught = 0u32;
    let mut rounds = 0u32;
    for _ in 0..64 {
        rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let mut pdi = PowerDesk::new(PowerTier::Balanced, 100);
        let spike = 9000 + (rng % 1000) as u32;
        for v in [100u32, 101, 100, 102] {
            pdi.feed(v);
        }
        pdi.feed(spike);
        for v in [100u32, 101, 99] {
            pdi.feed(v);
        }
        rounds += 1;
        if pdi.noisy(1000) {
            caught += 1;
        }
    }
    chk(&mut set, "C562-21", caught == rounds, "64 轮野点注入必抓率 100%");

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

    #[test]
    fn budget_conserves() {
        let mut bb = BudgetBook::new();
        bb.alloc(100);
        assert!(bb.spend(60).is_ok());
        assert!(bb.spend(60).is_err());
        assert!(bb.conserves());
        assert_eq!(bb.remaining(), 40);
    }

    #[test]
    fn hysteresis_no_oscillation() {
        let mut h = Hysteresis::new(2, 1);
        assert!(h.request_up().is_err());
        assert!(h.request_up().is_ok());
        assert!(h.request_up().is_err()); // 冷却
        h.tick();
        assert!(h.request_down().is_err());
        assert!(h.request_down().is_ok());
    }
}
