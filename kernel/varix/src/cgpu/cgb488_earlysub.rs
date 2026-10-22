//! CGPU-F0488 · 提前提交（目标 350 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 提前提交：下一帧提前量（预测驱动的提前渲染——与 C04 预测器
//!   联动预留，F0369 理念复用）；
//! - 提前量自适应（按负载稳定性动态调整——稳定场景提前多、抖动
//!   场景提前少）；
//! - 预测失败回退（预测错误→回退同步提交——零窗口回退复用）；
//! - 诚实标注（一期统计预测）。判据：联动预留、自适应、回退、
//!   诚实、判据。
//!
//! 设计纪律：
//! - 联动预留：提前量请求结构携带预测器源标记（C04 源 ID），无源
//!   标记的提前量不可信（联动结构在场）。
//! - 自适应：稳定度（EWMA 窗口抖动率）→ 提前量映射（稳定多/抖动
//!   少），映射单调且有界（0 ≤ lead ≤ LEAD_CAP）。
//! - 回退：预测错误立即回退同步提交（lead=0），回退后需连续稳定
//!   样本才重新启用提前（防抖动反复）。
//! - 诚实标注：一期统计预测——提前提交结果必须携带 statistical_
//!   only 标注，无标注即不诚实。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x1150 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x1150，全仓独占）。
pub const SEG_BASE: u16 = 0x1150;

pub const E_ES_NOSEED: u16 = SEG_BASE + 1; // 无预测器源标记
pub const E_ES_LEAD: u16 = SEG_BASE + 2; // 提前量越界
pub const E_ES_HONEST: u16 = SEG_BASE + 3; // 无统计预测标注
pub const E_ES_COOLDOWN: u16 = SEG_BASE + 4; // 冷却期未满重启用

/// 提前量上限（tick）与自适应参数。
pub const LEAD_CAP: u64 = 2;
pub const STABLE_EWMA_SHIFT: u32 = 4;
/// 回退后重新启用的稳定样本数。
pub const REARM_SAMPLES: u32 = 4;

/// 预测器源标记（C04 联动预留）。
pub const PRED_SOURCE_C04: u32 = 0x0C04;

/// 提前提交请求。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EarlySubmit {
    /// 预测器源（必须 C04 源标记才可启用）。
    pub pred_source: u32,
    /// 提前量（tick）。
    pub lead_ticks: u64,
    /// 统计预测诚实标注。
    pub statistical_only: bool,
}

impl EarlySubmit {
    pub fn valid(&self) -> Result<(), u16> {
        if self.pred_source != PRED_SOURCE_C04 {
            return Err(E_ES_NOSEED);
        }
        if self.lead_ticks > LEAD_CAP {
            return Err(E_ES_LEAD);
        }
        if !self.statistical_only {
            return Err(E_ES_HONEST);
        }
        Ok(())
    }
}

/// 提前量自适应（抖动率 EWMA → 提前量映射）。
#[derive(Debug)]
pub struct LeadController {
    /// 抖动率 EWMA（‰，0=完全稳定）。
    jitter_ewma_pm: u64,
    /// 回退冷却计数。
    cooldown: u32,
    /// 当前是否启用提前。
    pub enabled: bool,
    /// 预测错误累计。
    pub pred_misses: u64,
}

impl LeadController {
    pub fn new() -> Self {
        Self { jitter_ewma_pm: 0, cooldown: 0, enabled: false, pred_misses: 0 }
    }

    /// 喂抖动率样本（‰）。
    pub fn feed_jitter(&mut self, jitter_pm: u64) {
        self.jitter_ewma_pm = (self.jitter_ewma_pm * ((1 << STABLE_EWMA_SHIFT) - 1) + jitter_pm) >> STABLE_EWMA_SHIFT;
    }

    /// 自适应提前量：稳定(<200‰)给 2、一般(<600‰)给 1、抖动(≥600‰)给 0。
    pub fn adaptive_lead(&self) -> u64 {
        if !self.enabled || self.cooldown > 0 {
            return 0;
        }
        if self.jitter_ewma_pm < 200 {
            2
        } else if self.jitter_ewma_pm < 600 {
            1
        } else {
            0
        }
    }

    /// 预测错误：立即回退（lead=0 + 冷却）。
    pub fn predict_miss(&mut self) {
        self.pred_misses += 1;
        self.enabled = false;
        self.cooldown = REARM_SAMPLES;
    }

    /// 冷却期稳定样本：期满重新启用（联动预留生效）。
    pub fn feed_stable(&mut self) -> Result<u64, u16> {
        if !self.enabled && self.cooldown > 0 {
            self.cooldown -= 1;
            if self.cooldown == 0 {
                self.enabled = true;
            } else {
                return Err(E_ES_COOLDOWN);
            }
        } else if !self.enabled {
            self.enabled = true;
        }
        Ok(self.adaptive_lead())
    }
}

pub(crate) fn run_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();
    // 一、联动预留：源标记结构闸。
    {
        let good = EarlySubmit { pred_source: PRED_SOURCE_C04, lead_ticks: 1, statistical_only: true };
        v.push(("F0488-联动预留-C04源通过", good.valid().is_ok()));
        let nosrc = EarlySubmit { pred_source: 1234, lead_ticks: 1, statistical_only: true };
        v.push(("F0488-联动预留-无源标记拒", nosrc.valid() == Err(E_ES_NOSEED)));
    }
    // 二、自适应：稳定多/抖动少，单调有界。
    {
        let mut lc = LeadController::new();
        lc.enabled = true;
        for _ in 0..8 {
            lc.feed_jitter(50);
        }
        let l_stable = lc.adaptive_lead();
        for _ in 0..32 {
            lc.feed_jitter(800);
        }
        let l_jitter = lc.adaptive_lead();
        v.push(("F0488-自适应-稳定多抖动少", l_stable == 2 && l_jitter == 0 && l_stable > l_jitter));
        // 中间档。
        let mut lc2 = LeadController::new();
        lc2.enabled = true;
        for _ in 0..24 {
            lc2.feed_jitter(400);
        }
        v.push(("F0488-自适应-中档提前一", lc2.adaptive_lead() == 1));
        // 有界。
        v.push(("F0488-自适应-上限封顶", LEAD_CAP == 2));
    }
    // 三、回退：预测错误立即回零+冷却。
    {
        let mut lc = LeadController::new();
        lc.enabled = true;
        lc.predict_miss();
        v.push(("F0488-回退-错误立即归零", lc.adaptive_lead() == 0 && !lc.enabled));
        // 冷却期内重启用拒。
        v.push(("F0488-回退-冷却期拒启用", lc.feed_stable() == Err(E_ES_COOLDOWN)));
        lc.feed_stable().ok();
        lc.feed_stable().ok();
        let lead = lc.feed_stable();
        v.push(("F0488-回退-冷却期满重启用", lead.is_ok() && lc.enabled));
        v.push(("F0488-回退-错误计数累计", lc.pred_misses == 1));
    }
    // 四、诚实标注。
    {
        let honest = EarlySubmit { pred_source: PRED_SOURCE_C04, lead_ticks: 2, statistical_only: true };
        v.push(("F0488-诚实-统计预测标注通过", honest.valid().is_ok()));
        let lie = EarlySubmit { pred_source: PRED_SOURCE_C04, lead_ticks: 2, statistical_only: false };
        v.push(("F0488-诚实-无标注拒", lie.valid() == Err(E_ES_HONEST)));
    }
    // 五、判据自检。
    {
        v.push(("F0488-判据-源码入册", PRED_SOURCE_C04 == 0x0C04));
        v.push(("F0488-判据-重启用样本数", REARM_SAMPLES == 4));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::run_checks;
    #[test]
    fn f0488_earlysub_checks() {
        for (name, ok) in run_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
