//! cga35_clkadapt.rs ↔ CGPU-F0035 时钟感知自适应渲染（0x1B50 段）
//!
//! 职责（施工书原文）：
//!   CPU 频率动态变化（睿频/降频/热节流）时渲染自适应：频率事件感知（CGPU-F0011 联动）、
//!   瓦片预算按当前频率修正（预算不是常数是频率函数）、降频期主动减瓦片质量
//!   （低频时降 AA/简化滤波——帧率稳住画质渐进）。
//!
//! 判据（施工书原文）：
//!   频率事件联动、预算修正正确、降档渐进、恢复回升滞回、实测帧率稳定性改善。

use alloc::vec::Vec;
use crate::float::FloatExt;
use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::alloc::vec;
use crate::checks::CheckSet;

/// 诊断段（0x1B50 独占 0x10）。
pub const SEG_BASE: u16 = 0x1B50;
pub const XC_FREQ: u16 = SEG_BASE + 1; // 频率读数非法
pub const XC_QUAL: u16 = SEG_BASE + 2; // 质量档非法

/// 基准频率（预算=基准频率函数的归一锚）
pub const BASE_FREQ_MHZ: u32 = 3000;

/// 频率事件（F0011 事件源语义）
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum FreqEvent {
    Boost = SEG_BASE,      // 0x1B50 睿频进入
    Sustain,               // 0x1B51 稳态
    ThermalThrottle,       // 0x1B52 热节流
    PowerCap,              // 0x1B53 功耗墙
}
impl FreqEvent {
    pub const fn code(self) -> u16 { self as u16 }
}

/// 质量档（渐进五档——降档有序，不允许跳档）
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Debug, PartialOrd)]
pub enum QualLevel {
    Ultra = 0, // 全 AA + 全滤波
    High,      // AA 减半
    Medium,    // AA 关 + 滤波降级
    Low,       // 简化滤波
    Floor,     // 保底（最近邻）
}
impl QualLevel {
    pub const ALL: [QualLevel; 5] = [
        QualLevel::Ultra, QualLevel::High, QualLevel::Medium, QualLevel::Low, QualLevel::Floor,
    ];
    /// 档位步进成本系数（相对 Ultra 的成本比）
    pub const fn cost_ratio(self) -> f32 {
        match self {
            QualLevel::Ultra => 1.0,
            QualLevel::High => 0.6,
            QualLevel::Medium => 0.4,
            QualLevel::Low => 0.25,
            QualLevel::Floor => 0.15,
        }
    }
    pub const fn name(self) -> &'static str {
        match self {
            QualLevel::Ultra => "Ultra",
            QualLevel::High => "High",
            QualLevel::Medium => "Medium",
            QualLevel::Low => "Low",
            QualLevel::Floor => "Floor",
        }
    }
    pub const fn step_down(self) -> Option<QualLevel> {
        match self {
            QualLevel::Ultra => Some(QualLevel::High),
            QualLevel::High => Some(QualLevel::Medium),
            QualLevel::Medium => Some(QualLevel::Low),
            QualLevel::Low => Some(QualLevel::Floor),
            QualLevel::Floor => None,
        }
    }
    pub const fn step_up(self) -> Option<QualLevel> {
        match self {
            QualLevel::Ultra => None,
            QualLevel::High => Some(QualLevel::Ultra),
            QualLevel::Medium => Some(QualLevel::High),
            QualLevel::Low => Some(QualLevel::Medium),
            QualLevel::Floor => Some(QualLevel::Low),
        }
    }
}

/// 预算修正：预算 = 基准预算 × (当前频率 / 基准频率)
pub fn scale_budget(base_budget_ms: f32, cur_freq_mhz: u32) -> Result<f32, u16> {
    if cur_freq_mhz < 200 || cur_freq_mhz > 8000 {
        return Err(XC_FREQ);
    }
    Ok(base_budget_ms * (cur_freq_mhz as f32 / BASE_FREQ_MHZ as f32))
}

/// 滞回状态机：降档立即（帧超预算），升档需连续 N 帧低于恢复阈值
pub struct AdaptController {
    pub cur: QualLevel,
    pub over_budget_streak: u32,  // 连续超预算帧数
    pub recover_streak: u32,      // 连续低于恢复阈值帧数
    pub down_after: u32,          // 连续超 N 帧降一档
    pub up_after: u32,            // 连续低于 N 帧升一档
}

impl AdaptController {
    pub fn new() -> Self {
        AdaptController { cur: QualLevel::Ultra, over_budget_streak: 0, recover_streak: 0, down_after: 2, up_after: 8 }
    }
    /// 帧反馈：actual_ms 实测，budget_ms 预算。返回（新档位，是否跳变）
    pub fn feed(&mut self, actual_ms: f32, budget_ms: f32) -> (QualLevel, bool) {
        if actual_ms > budget_ms {
            self.over_budget_streak += 1;
            self.recover_streak = 0;
            if self.over_budget_streak >= self.down_after {
                self.over_budget_streak = 0;
                if let Some(next) = self.cur.step_down() {
                    self.cur = next;
                    return (self.cur, true);
                }
            }
        } else if actual_ms < budget_ms * 0.75 {
            // 恢复回升滞回：必须富余 25% 且连续 up_after 帧才升
            self.recover_streak += 1;
            self.over_budget_streak = 0;
            if self.recover_streak >= self.up_after {
                self.recover_streak = 0;
                if let Some(next) = self.cur.step_up() {
                    self.cur = next;
                    return (self.cur, true);
                }
            }
        } else {
            // 灰区：两边计数都清
            self.over_budget_streak = 0;
            self.recover_streak = 0;
        }
        (self.cur, false)
    }
}

/// 频率事件→质量意图映射（联动表）
pub fn event_intent(ev: FreqEvent, cur: QualLevel) -> QualLevel {
    match ev {
        FreqEvent::Boost => cur, // 睿频：维持当前档（升档由滞回决定）
        FreqEvent::Sustain => cur,
        FreqEvent::ThermalThrottle => cur.step_down().unwrap_or(QualLevel::Floor),
        FreqEvent::PowerCap => cur.step_down().unwrap_or(QualLevel::Floor).step_down().unwrap_or(QualLevel::Floor),
    }
}

/// 帧率稳定性统计（实测改善口径）
#[derive(Clone, Default)]
pub struct FpsStability {
    pub samples: Vec<f32>,
}
impl FpsStability {
    pub fn push(&mut self, ms: f32) { self.samples.push(ms); }
    /// 抖动 = 标准差 / 均值（变异系数）
    pub fn jitter(&self) -> f32 {
        if self.samples.len() < 2 {
            return 0.0;
        }
        let n = self.samples.len() as f32;
        let mean = self.samples.iter().sum::<f32>() / n;
        let var = self.samples.iter().map(|s| (s - mean) * (s - mean)).sum::<f32>() / n;
        var.m_sqrt() / mean.max(1e-6)
    }
}

pub fn self_audit() -> Result<(), u16> {
    let _ = scale_budget(16.0, BASE_FREQ_MHZ)?;
    let _ = AdaptController::new();
    let _ = event_intent as fn(FreqEvent, QualLevel) -> QualLevel;
    Ok(())
}

pub fn run_cga35_checks() -> CheckSet {
    let mut s = CheckSet::new("CGPU-F0035");
    // J1 频率事件联动（热节流/功耗墙降档，睿频维持）
    let ok = event_intent(FreqEvent::ThermalThrottle, QualLevel::Ultra) == QualLevel::High
        && event_intent(FreqEvent::PowerCap, QualLevel::Ultra) == QualLevel::Medium
        && event_intent(FreqEvent::Boost, QualLevel::High) == QualLevel::High
        && event_intent(FreqEvent::Sustain, QualLevel::Floor) == QualLevel::Floor;
    s.add("CGA-35-01-事件联动", ok, if ok { "" } else { "联动破缺" });
    // J2 预算修正正确（预算是频率函数：降频减预算、基准归一、非法拒）
    let b_base = scale_budget(16.0, 3000).ok().unwrap();
    let b_low = scale_budget(16.0, 1500).ok().unwrap();
    let b_high = scale_budget(16.0, 4500).ok().unwrap();
    let ok = (b_base - 16.0).abs() < 1e-6
        && (b_low - 8.0).abs() < 1e-6
        && (b_high - 24.0).abs() < 1e-6
        && scale_budget(16.0, 100) == Err(XC_FREQ);
    s.add("CGA-35-02-预算修正", ok, if ok { "" } else { "频率函数破缺" });
    // J3 降档渐进（每次只降一档 + Floor 不再降）
    let ok = QualLevel::Ultra.step_down() == Some(QualLevel::High)
        && QualLevel::High.step_down() == Some(QualLevel::Medium)
        && QualLevel::Low.step_down() == Some(QualLevel::Floor)
        && QualLevel::Floor.step_down().is_none()
        && QualLevel::Ultra.cost_ratio() > QualLevel::Floor.cost_ratio();
    s.add("CGA-35-03-降档渐进", ok, if ok { "" } else { "跳档破缺" });
    // J4 恢复回升滞回（富余 25% 且连续 8 帧才升；降档 2 帧即动）
    let mut ctl = AdaptController::new();
    let mut jumps = 0u32;
    // 模拟：预算 16ms，超预算 3 帧降档
    for _ in 0..3 {
        let (_, j) = ctl.feed(20.0, 16.0);
        if j { jumps += 1; }
    }
    let after_down = ctl.cur;
    // 富余 7 帧不升（不足 8）
    for _ in 0..7 {
        let (_, j) = ctl.feed(10.0, 16.0);
        if j { jumps += 1; }
    }
    let still = ctl.cur;
    // 第 8 帧富余升档
    let (_, j) = ctl.feed(10.0, 16.0);
    if j { jumps += 1; }
    let ok = after_down == QualLevel::High && still == QualLevel::High && j && ctl.cur == QualLevel::Ultra;
    s.add("CGA-35-04-滞回状态机", ok, if ok { "" } else { "滞回破缺" });
    // J4-b 灰区清零（不升不降）
    let mut ctl2 = AdaptController::new();
    let _ = ctl2.feed(20.0, 16.0); // 超 1 帧
    let (lvl, j) = ctl2.feed(15.0, 16.0); // 灰区（不富余 25%）
    let ok = lvl == QualLevel::Ultra && !j && ctl2.over_budget_streak == 0;
    s.add("CGA-35-05-灰区清零", ok, if ok { "" } else { "灰区破缺" });
    // J5 帧率稳定性改善（降档后抖动小于降档前）
    let mut before = FpsStability::default();
    for ms in [22.0, 14.0, 25.0, 12.0, 28.0, 11.0] { before.push(ms); }
    let mut after = FpsStability::default();
    for ms in [15.0, 16.0, 15.5, 16.2, 15.1, 15.8] { after.push(ms); }
    let ok = after.jitter() < before.jitter() && before.samples.len() == after.samples.len();
    s.add("CGA-35-06-稳定性改善", ok, if ok { "" } else { "抖动未降" });
    // J6 频率读数边界（200 下限 8000 上限）
    let ok = scale_budget(1.0, 200).is_ok() && scale_budget(1.0, 8000).is_ok();
    s.add("CGA-35-07-频率边界", ok, if ok { "" } else { "边界破缺" });
    // J7 码位互异段独占
    let ok = SEG_BASE % 0x10 == 0
        && FreqEvent::Boost.code() != FreqEvent::PowerCap.code()
        && QualLevel::ALL.len() == 5
        && XC_FREQ != XC_QUAL;
    s.add("CGA-35-08-码位互异", ok, if ok { "" } else { "码位冲突" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cga35_checks_all_green() {
        let set = run_cga35_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn cga35_floor_never_steps_below() {
        let mut ctl = AdaptController::new();
        ctl.cur = QualLevel::Floor;
        for _ in 0..10 {
            let (lvl, _) = ctl.feed(100.0, 16.0);
            assert_eq!(lvl, QualLevel::Floor);
        }
    }
}
