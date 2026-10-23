//! VE-F0111 · 适配器能力差异均衡器（VE-A 域 · A05 多 GPU 与异构组 · 目标 360 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0111`
//!
//! **职责定位（锚点原文）**：多卡能力差异的负载均衡（弱卡不拖强卡），均衡的公平
//! 与效率取舍，能力差异量化表；含均衡器的切换迟滞（负载抖动不引起乒乓）。
//!
//! **铁律**：
//! 1. **按能力分配**——强卡多分、弱卡少分，用量化表。
//! 2. **弱卡保护**——弱卡不过载。
//! 3. **取舍声明**——公平与效率取舍显式。
//! 4. **迟滞**——负载抖动不引起乒乓切换。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址 0xE8B5（能力均衡）。
pub const SEG_BASE: u16 = 0xE8B5;

/// 能力差异误判。
pub const XC_CAP_MISJUDGE: u16 = SEG_BASE | 0x01;
/// 弱卡过载。
pub const XC_WEAK_OVERLOAD: u16 = SEG_BASE | 0x02;
/// 负载失衡。
pub const XC_LOAD_IMBALANCE: u16 = SEG_BASE | 0x03;
/// 迟滞未生效。
pub const XC_HYSTERESIS: u16 = SEG_BASE | 0x04;

/// 适配器能力档。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CapTier {
    /// 弱卡。
    Weak,
    /// 中卡。
    Mid,
    /// 强卡。
    Strong,
}

impl CapTier {
    /// 能力权重（分配比例基数）。
    pub fn weight(&self) -> u32 {
        match self {
            CapTier::Weak => 1,
            CapTier::Mid => 2,
            CapTier::Strong => 4,
        }
    }

    /// 弱卡负载上限（千分比）。
    pub fn load_cap_permille(&self) -> u32 {
        match self {
            CapTier::Weak => 600,
            CapTier::Mid => 850,
            CapTier::Strong => 1000,
        }
    }
}

/// 卡记录。
#[derive(Clone, Debug)]
pub struct Adapter {
    pub id: u16,
    pub tier: CapTier,
    /// 当前负载（千分比）。
    pub load_permille: u32,
}

/// 量化表（能力差异数据化）。
#[derive(Clone, Debug)]
pub struct CapMatrix {
    /// (弱,mid,strong) 两两比值（千分比）。
    pub weak_vs_mid: u32,
    pub mid_vs_strong: u32,
    pub weak_vs_strong: u32,
}

impl CapMatrix {
    pub fn derive(adapters: &[Adapter]) -> Result<Self, u16> {
        let w = adapters.iter().find(|a| a.tier == CapTier::Weak).map(|a| a.tier.weight());
        let m = adapters.iter().find(|a| a.tier == CapTier::Mid).map(|a| a.tier.weight());
        let s = adapters.iter().find(|a| a.tier == CapTier::Strong).map(|a| a.tier.weight());
        if w.is_none() && m.is_none() && s.is_none() {
            return Err(XC_CAP_MISJUDGE);
        }
        let ratio = |a: u32, b: u32| -> u32 {
            if b == 0 {
                1000
            } else {
                a * 1000 / b
            }
        };
        Ok(CapMatrix {
            weak_vs_mid: ratio(w.unwrap_or(1), m.unwrap_or(2)),
            mid_vs_strong: ratio(m.unwrap_or(2), s.unwrap_or(4)),
            weak_vs_strong: ratio(w.unwrap_or(1), s.unwrap_or(4)),
        })
    }
}

/// 均衡取舍（公平 vs 效率）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BalanceBias {
    /// 偏公平。
    Fair,
    /// 偏效率。
    Efficient,
}

/// 切换迟滞（防乒乓）。
#[derive(Clone, Debug)]
pub struct Hysteresis {
    /// 触发阈值（千分比差）。
    pub threshold: u32,
    /// 需连续满足次数。
    pub sustain: u32,
    streak: u32,
}

impl Hysteresis {
    pub fn new(threshold: u32, sustain: u32) -> Self {
        Hysteresis {
            threshold: if threshold == 0 { 100 } else { threshold },
            sustain: if sustain == 0 { 3 } else { sustain },
            streak: 0,
        }
    }

    /// 喂一次负载差；返回 true 表示应切换。
    pub fn feed(&mut self, delta_permille: u32) -> bool {
        if delta_permille >= self.threshold {
            self.streak += 1;
        } else {
            self.streak = 0;
        }
        self.streak >= self.sustain
    }

    pub fn reset(&mut self) {
        self.streak = 0;
    }
}

/// 均衡器。
pub struct LoadBalancer {
    adapters: Vec<Adapter>,
    bias: BalanceBias,
    hyst: Hysteresis,
    matrix: CapMatrix,
    last_code: u16,
    /// 重均衡次数。
    rebalances: u32,
    /// 弱卡保护触发次数。
    weak_rescues: u32,
    /// 乒乓拦截次数。
    pingpong_blocked: u32,
}

impl LoadBalancer {
    pub fn new(
        adapters: Vec<Adapter>,
        bias: BalanceBias,
        hyst: Hysteresis,
    ) -> Result<Self, u16> {
        if adapters.is_empty() {
            return Err(XC_CAP_MISJUDGE);
        }
        let matrix = CapMatrix::derive(&adapters)?;
        Ok(LoadBalancer {
            adapters,
            bias,
            hyst,
            matrix,
            last_code: 0,
            rebalances: 0,
            weak_rescues: 0,
            pingpong_blocked: 0,
        })
    }

    pub fn last_code(&self) -> u16 {
        self.last_code
    }

    pub fn matrix(&self) -> &CapMatrix {
        &self.matrix
    }

    /// 按能力分配负载份额。
    pub fn allocate(&mut self, total_work: u32) -> Vec<(u16, u32)> {
        let total_w: u32 = self.adapters.iter().map(|a| a.tier.weight()).sum();
        let mut out = Vec::new();
        for a in &self.adapters {
            let share = total_work * a.tier.weight() / total_w.max(1);
            out.push((a.id, share));
        }
        out
    }

    /// 弱卡保护：检测过载并减载。
    pub fn protect_weak(&mut self) -> Result<(), u16> {
        let mut overloaded = false;
        for a in self.adapters.iter_mut() {
            let cap = a.tier.load_cap_permille();
            if a.load_permille > cap {
                overloaded = true;
                a.load_permille = cap; // 减载到上限
                self.weak_rescues += 1;
            }
        }
        if overloaded {
            self.last_code = XC_WEAK_OVERLOAD;
            Err(XC_WEAK_OVERLOAD)
        } else {
            self.last_code = 0;
            Ok(())
        }
    }

    /// 重均衡决策（迟滞防抖）。
    pub fn consider_rebalance(&mut self, delta_permille: u32) -> bool {
        if self.hyst.feed(delta_permille) {
            self.rebalances += 1;
            self.hyst.reset();
            true
        } else {
            if delta_permille > 0 {
                self.pingpong_blocked += 1;
            }
            false
        }
    }

    /// 失衡检测。
    pub fn detect_imbalance(&mut self) -> Result<(), u16> {
        let max = self
            .adapters
            .iter()
            .map(|a| a.load_permille)
            .max()
            .unwrap_or(0);
        let min = self
            .adapters
            .iter()
            .map(|a| a.load_permille)
            .min()
            .unwrap_or(0);
        if max > min * 2 && max > 500 {
            self.last_code = XC_LOAD_IMBALANCE;
            Err(XC_LOAD_IMBALANCE)
        } else {
            self.last_code = 0;
            Ok(())
        }
    }

    /// 差异误判（实测与量化表偏差大）重估。
    pub fn recalibrate(&mut self, measured_weights: &[u32]) -> Result<(), u16> {
        if measured_weights.len() != self.adapters.len() {
            self.last_code = XC_CAP_MISJUDGE;
            return Err(XC_CAP_MISJUDGE);
        }
        for (i, w) in measured_weights.iter().enumerate() {
            let expect = self.adapters[i].tier.weight();
            // 偏差超 2 倍判误判。
            if *w > expect * 2 || (*w > 0 && expect > *w * 2) {
                self.last_code = XC_CAP_MISJUDGE;
                return Err(XC_CAP_MISJUDGE);
            }
        }
        self.last_code = 0;
        Ok(())
    }

    pub fn rebalances(&self) -> u32 {
        self.rebalances
    }

    pub fn weak_rescues(&self) -> u32 {
        self.weak_rescues
    }

    pub fn pingpong_blocked(&self) -> u32 {
        self.pingpong_blocked
    }

    pub fn bias(&self) -> BalanceBias {
        self.bias
    }
}

/// 自检：按能力分配、弱卡保护、取舍声明、差异量化。
pub fn run_vea111_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;

    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }

    fn adapters() -> Vec<Adapter> {
        vec![
            Adapter {
                id: 0,
                tier: CapTier::Weak,
                load_permille: 100,
            },
            Adapter {
                id: 1,
                tier: CapTier::Mid,
                load_permille: 200,
            },
            Adapter {
                id: 2,
                tier: CapTier::Strong,
                load_permille: 300,
            },
        ]
    }

    let mut set = CheckSet::new("vea111_balance");

    // C111-01 量化表
    {
        let m = CapMatrix::derive(&adapters());
        chk(
            &mut set,
            "C111-01 量化表",
            m.is_ok() && m.unwrap().weak_vs_strong == 250,
            "量化错",
        ); // 1*1000/4 = 250
    }

    // C111-02 按能力分配
    {
        let mut b = LoadBalancer::new(adapters(), BalanceBias::Efficient, Hysteresis::new(100, 3)).unwrap();
        let alloc = b.allocate(700);
        let total: u32 = alloc.iter().map(|a| a.1).sum();
        chk(&mut set, "C111-02 按能力", total > 600 && total < 750, "分配失真");
    }

    // C111-03 弱卡保护减载
    {
        let mut ads = adapters();
        ads[0].load_permille = 900; // 弱卡过载
        let mut b = LoadBalancer::new(ads, BalanceBias::Fair, Hysteresis::new(100, 3)).unwrap();
        let r = b.protect_weak();
        chk(
            &mut set,
            "C111-03 弱卡保护",
            r.is_err() && b.weak_rescues() == 1,
            "未保护",
        );
    }

    // C111-04 迟滞防乒乓
    {
        let mut b = LoadBalancer::new(adapters(), BalanceBias::Fair, Hysteresis::new(100, 3)).unwrap();
        let c1 = b.consider_rebalance(200); // 1st
        let c2 = b.consider_rebalance(200); // 2nd
        let c3 = b.consider_rebalance(200); // 3rd → 触发
        chk(&mut set, "C111-04 迟滞", !c1 && !c2 && c3, "迟滞失效");
    }

    // C111-05 抖动不切换
    {
        let mut b = LoadBalancer::new(adapters(), BalanceBias::Fair, Hysteresis::new(100, 3)).unwrap();
        let _ = b.consider_rebalance(200);
        let _ = b.consider_rebalance(200);
        let c = b.consider_rebalance(50); // 抖动回落清零
        chk(&mut set, "C111-05 抖动阻断", !c && b.pingpong_blocked() >= 1, "抖动触发");
    }

    // C111-06 失衡检测
    {
        let mut ads = adapters();
        ads[2].load_permille = 900;
        ads[0].load_permille = 100;
        let mut b = LoadBalancer::new(ads, BalanceBias::Fair, Hysteresis::new(100, 3)).unwrap();
        let r = b.detect_imbalance();
        chk(
            &mut set,
            "C111-06 失衡检测",
            r.is_err() && b.last_code() == XC_LOAD_IMBALANCE,
            "失衡漏检",
        );
    }

    // C111-07 取舍声明
    {
        let b = LoadBalancer::new(adapters(), BalanceBias::Fair, Hysteresis::new(100, 3)).unwrap();
        chk(&mut set, "C111-07 取舍声明", b.bias() == BalanceBias::Fair, "取舍未声明");
    }

    // C111-08 差异误判检出
    {
        let mut b = LoadBalancer::new(adapters(), BalanceBias::Fair, Hysteresis::new(100, 3)).unwrap();
        let r = b.recalibrate(&[1, 2, 100]); // 强卡实测 100 vs 预期 4
        chk(
            &mut set,
            "C111-08 误判检出",
            r.is_err() && b.last_code() == XC_CAP_MISJUDGE,
            "误判漏检",
        );
    }

    // C111-09 空适配器拒绝
    {
        let r = LoadBalancer::new(vec![], BalanceBias::Fair, Hysteresis::new(100, 3));
        chk(&mut set, "C111-09 空拒绝", r.is_err(), "空被放");
    }

    // C111-10 校准通过
    {
        let mut b = LoadBalancer::new(adapters(), BalanceBias::Fair, Hysteresis::new(100, 3)).unwrap();
        let r = b.recalibrate(&[1, 2, 4]);
        chk(&mut set, "C111-10 校准过", r.is_ok(), "校准误拒");
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vea111_all_green() {
        let s = run_vea111_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
