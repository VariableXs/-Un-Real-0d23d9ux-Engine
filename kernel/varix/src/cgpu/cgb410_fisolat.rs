//! CGPU-F0410 · 集成故障隔离（目标 340 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 故障隔离：单点故障不扩散（任一集成域故障→仲裁降级到无该维度
//!   运行——其余维度照常）；降级显性（降级事件+影响面三要素告知）；
//!   恢复回流（故障域恢复→维度自动回归+状态对账）；隔离测试（逐域
//!   注入故障验证隔离边界）。
//! - 判据：降级运行、显性、回归对账、隔离测试、判据。
//!
//! 设计纪律：
//! - 维度化仲裁：六集成维度各自可降级；任一维故障 → 该维退出仲裁
//!   （权重归零/中位兜底），其余五维照常——不扩散的形式化 = 剩余
//!   维度权重和仍为 1000 且仲裁照常出决策。
//! - 降级显性三要素：事件（哪个域）、影响面（哪些能力失去）、
//!   兜底策略（用什么替代）——三要素齐才允许生效降级。
//! - 恢复回流：故障域恢复 → 维度回归（权重恢复 + 状态对账：恢复期
//!   内的配置差异逐项核对后入列）。
//! - 隔离测试：逐域注入故障 → 验证其余维度决策不变（隔离边界）。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0CD0 独占。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x0CD0，全仓独占）。
pub const SEG_BASE: u16 = 0x0CD0;

pub const E_ISO_SPREAD: u16 = SEG_BASE + 1; // 故障扩散（其余维决策被扰动）
pub const E_ISO_Triad: u16 = SEG_BASE + 2; // 降级三要素缺件
pub const E_ISO_STATE: u16 = SEG_BASE + 3; // 状态机非法（降级域直接回归/重复恢复）
pub const E_ISO_RECON: u16 = SEG_BASE + 4; // 回流对账差异未清
pub const E_ISO_POINT: u16 = SEG_BASE + 5; // 维度号非法
pub const E_ISO_WEIGHT: u16 = SEG_BASE + 6; // 权重和越界

/// 六集成维度（复用矩阵坐标）。
pub const DIM_COUNT: usize = 6;

/// 维度状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DimState {
    Normal,
    Degraded,
    Recovering,
}

/// 降级事件三要素。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DegradeEvent {
    /// 域号。
    pub dim: u8,
    /// 影响面（失去的能力清单）。
    pub impacts: Vec<String>,
    /// 兜底策略号。
    pub fallback: u8,
    pub frame: u64,
}

/// 决策输入（仲裁仿真）：各维的权重贡献。
#[derive(Clone, Debug, PartialEq)]
pub struct ArbDecision {
    pub frame: u64,
    /// 各维权重‰（降级维为 0，其余和恒 1000）。
    pub weights: [u32; DIM_COUNT],
    /// 决策摘要（选中动作号）。
    pub action: u32,
}

/// 故障隔离控制器。
#[derive(Clone, Debug)]
pub struct FaultIsolator {
    pub states: [DimState; DIM_COUNT],
    pub weights_normal: [u32; DIM_COUNT],
    pub events: Vec<DegradeEvent>,
    /// 回流对账差异（恢复期残留配置差）。
    pub recon_pending: Vec<(u8, u32)>,
    /// 基线动作（隔离测试的对照）。
    pub baseline_action: u32,
}

impl FaultIsolator {
    pub fn new(weights_normal: [u32; DIM_COUNT], baseline: u32) -> Self {
        FaultIsolator {
            states: [DimState::Normal; DIM_COUNT],
            weights_normal,
            events: Vec::new(),
            recon_pending: Vec::new(),
            baseline_action: baseline,
        }
    }

    /// 当前生效权重：降级/恢复维归零，其余等比放大回 1000。
    pub fn effective_weights(&self) -> Result<[u32; DIM_COUNT], u16> {
        if self.weights_normal.iter().sum::<u32>() != 1000 {
            return Err(E_ISO_WEIGHT);
        }
        let live: Vec<usize> = self
            .states
            .iter()
            .enumerate()
            .filter(|(_, s)| **s == DimState::Normal)
            .map(|(i, _)| i)
            .collect();
        if live.is_empty() {
            return Err(E_ISO_SPREAD);
        }
        let sum: u32 = live.iter().map(|i| self.weights_normal[*i]).sum();
        let mut w = [0u32; DIM_COUNT];
        let mut acc = 0u32;
        for (k, i) in live.iter().enumerate() {
            if k + 1 == live.len() {
                w[*i] = 1000 - acc;
            } else {
                w[*i] = self.weights_normal[*i] * 1000 / sum;
                acc += w[*i];
            }
        }
        Ok(w)
    }

    /// 降级：三要素齐才生效；维度号合法 + 状态机合法。
    pub fn degrade(&mut self, dim: u8, impacts: &[&str], fallback: u8, frame: u64) -> Result<(), u16> {
        if dim as usize >= DIM_COUNT {
            return Err(E_ISO_POINT);
        }
        if impacts.is_empty() || fallback == 0 {
            return Err(E_ISO_Triad);
        }
        if self.states[dim as usize] != DimState::Normal {
            return Err(E_ISO_STATE);
        }
        self.states[dim as usize] = DimState::Degraded;
        self.events.push(DegradeEvent {
            dim,
            impacts: impacts.iter().map(|s| s.to_string()).collect(),
            fallback,
            frame,
        });
        Ok(())
    }

    /// 恢复回流：降级 → Recovering，差异项进对账池。
    pub fn begin_recover(&mut self, dim: u8, diffs: Vec<(u8, u32)>) -> Result<(), u16> {
        if dim as usize >= DIM_COUNT {
            return Err(E_ISO_POINT);
        }
        if self.states[dim as usize] != DimState::Degraded {
            return Err(E_ISO_STATE);
        }
        self.states[dim as usize] = DimState::Recovering;
        self.recon_pending.extend(diffs);
        Ok(())
    }

    /// 对账清账：差异逐项核销后维度回归 Normal。
    pub fn finish_recover(&mut self, dim: u8) -> Result<(), u16> {
        if dim as usize >= DIM_COUNT {
            return Err(E_ISO_POINT);
        }
        if self.states[dim as usize] != DimState::Recovering {
            return Err(E_ISO_STATE);
        }
        if self.recon_pending.is_empty() {
            self.states[dim as usize] = DimState::Normal;
            Ok(())
        } else {
            Err(E_ISO_RECON)
        }
    }

    /// 对账核销：按差异键消除一项。
    pub fn recon_settle(&mut self, key: u8) -> bool {
        if let Some(p) = self.recon_pending.iter().position(|(k, _)| *k == key) {
            self.recon_pending.remove(p);
            true
        } else {
            false
        }
    }

    /// 仲裁仿真：给定各维局部决策，输出总决策 = 权重最大维的动作；
    /// 隔离边界验证：故障维的动作不得影响结果。
    pub fn decide(&self, dim_actions: [u32; DIM_COUNT], frame: u64) -> Result<ArbDecision, u16> {
        let w = self.effective_weights()?;
        // 取权重最大的活维动作；活维动作一致时等于基线（隔离不扩散）。
        let mut best = 0usize;
        for i in 1..DIM_COUNT {
            if w[i] > w[best] {
                best = i;
            }
        }
        if w[best] == 0 {
            return Err(E_ISO_SPREAD);
        }
        Ok(ArbDecision { frame, weights: w, action: dim_actions[best] })
    }

    /// 隔离测试：注入故障维后，总决策必须仍等于基线。
    pub fn isolation_test(&self, dim_actions: [u32; DIM_COUNT], frame: u64) -> Result<(), u16> {
        let d = self.decide(dim_actions, frame)?;
        if d.action != self.baseline_action {
            return Err(E_ISO_SPREAD);
        }
        Ok(())
    }
}

// ==========================================================================
// 判据测试
// ==========================================================================
#[cfg(test)]
mod tests {
    use super::*;
    use crate::cgpu::cgb409_regmatrix::{POINT_D, POINT_E, POINT_K};

    fn iso() -> FaultIsolator {
        FaultIsolator::new([200, 200, 150, 150, 150, 150], 7)
    }

    fn actions() -> [u32; DIM_COUNT] {
        [7, 7, 7, 7, 7, 7]
    }

    #[test]
    fn a01_降级运行_单域故障其余照常() {
        let mut i = iso();
        // K 维故障降级。
        i.degrade(POINT_K, &["异构预算通道", "NPU 校准"], 1, 10).ok();
        assert_eq!(i.states[POINT_K as usize], DimState::Degraded);
        // 其余五维权重和回 1000。
        let w = i.effective_weights().ok();
        assert_eq!(w.map(|w| w.iter().sum::<u32>()), Some(1000));
        assert_eq!(w.map(|w| w[POINT_K as usize]), Some(0));
        // 仲裁照常出决策。
        let d = i.decide(actions(), 11).ok();
        assert_eq!(d.map(|d| d.action), Some(7));
    }

    #[test]
    fn a02_显性_三要素齐才生效() {
        let mut i = iso();
        // 缺影响面拒。
        assert_eq!(i.degrade(POINT_D, &[], 1, 1), Err(E_ISO_Triad));
        // 缺兜底策略拒。
        assert_eq!(i.degrade(POINT_D, &["帧闭环"], 0, 1), Err(E_ISO_Triad));
        // 齐件生效且事件入账。
        i.degrade(POINT_D, &["帧闭环"], 2, 5).ok();
        assert_eq!(i.events.len(), 1);
        let e = &i.events[0];
        assert_eq!(e.dim, POINT_D);
        assert_eq!(e.impacts, vec!["帧闭环".to_string()]);
        assert_eq!(e.fallback, 2);
        assert_eq!(e.frame, 5);
        // 重复降级拒。
        assert_eq!(i.degrade(POINT_D, &["x"], 1, 6), Err(E_ISO_STATE));
    }

    #[test]
    fn a03_回归对账_差异清账才回流() {
        let mut i = iso();
        i.degrade(POINT_E, &["表面联动"], 1, 1).ok();
        // 带差异进入恢复期。
        i.begin_recover(POINT_E, vec![(3, 100), (4, 200)]).ok();
        assert_eq!(i.states[POINT_E as usize], DimState::Recovering);
        // 差异未清就回归 ⇒ 拒。
        assert_eq!(i.finish_recover(POINT_E), Err(E_ISO_RECON));
        // 逐项核销。
        assert!(i.recon_settle(3));
        assert!(i.recon_settle(4));
        assert!(!i.recon_settle(3)); // 重复核销失败
        i.finish_recover(POINT_E).ok();
        assert_eq!(i.states[POINT_E as usize], DimState::Normal);
        // 状态机跳步：Normal 直接恢复拒。
        assert_eq!(i.begin_recover(POINT_D, vec![]), Err(E_ISO_STATE));
    }

    #[test]
    fn a04_隔离测试_逐域注入决策不变() {
        let mut i = iso();
        // 基线决策 7（六维动作全 7）。
        for d in 0..DIM_COUNT as u8 {
            let mut t = iso();
            t.degrade(d, &["x"], 1, 1).ok();
            // 故障维注入扰动动作 99，其余维保持基线。
            let mut acts = actions();
            acts[d as usize] = 99;
            assert_eq!(t.isolation_test(acts, 1), Ok(()));
        }
        // 对照：活维动作被扰动 ⇒ 决策变 ⇒ 扩散红。
        let mut acts = actions();
        acts[0] = 8;
        let mut t = iso();
        t.degrade(POINT_K, &["x"], 1, 1).ok();
        assert_eq!(t.isolation_test(acts, 1), Err(E_ISO_SPREAD));
    }

    #[test]
    fn a05_权重防线_基线和不为1000即拒() {
        let bad = FaultIsolator::new([500, 500, 100, 0, 0, 0], 7);
        assert_eq!(bad.effective_weights(), Err(E_ISO_WEIGHT));
        // 全维故障 ⇒ 无活维可仲裁。
        let mut i = iso();
        for d in 0..DIM_COUNT as u8 {
            i.degrade(d, &["x"], 1, 1).ok();
        }
        assert_eq!(i.effective_weights(), Err(E_ISO_SPREAD));
        assert_eq!(i.decide(actions(), 1), Err(E_ISO_SPREAD));
    }
}
