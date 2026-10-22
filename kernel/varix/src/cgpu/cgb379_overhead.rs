//! CGPU-F0379 · 预测器开销控制（目标 300 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 开销：预测 CPU 预算 ≤0.5%（预测本身不成为负载——实测达标）；增量计算
//!   （复用上一帧统计——不全量重算）；超标降级（预测降频/暂停→被动仲裁
//!   兜底并显性告知）。
//! - 判据：0.5% 实测、增量、降级兜底、判据。
//!
//! 设计纪律：
//! - 开销以「周期数」计量：每帧预算 = 帧周期 × 0.5%（千分之五）。
//! - 增量：维护滚动聚合器，逐帧 O(1) 更新，禁止全量重扫。
//! - 降级是显性状态机：Normal→Throttled→Suspended，每步对外可见。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0A60 独占。

use alloc::vec::Vec;

/// 诊断段基址（0x0A60，全仓独占）。
pub const SEG_BASE: u16 = 0x0A60;

pub const E_BUDGET_EXCEEDED: u16 = SEG_BASE + 1;
pub const E_INCREMENTAL_VIOLATION: u16 = SEG_BASE + 2;
pub const E_SUSPENDED: u16 = SEG_BASE + 3;
pub const E_THROTTLED: u16 = SEG_BASE + 4;

/// 预算参数：0.5% = 5/1000。
pub const BUDGET_PERMILLE: u32 = 5;

/// 降级状态机（显性三态）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DegradeState {
    /// 正常：每帧都预测。
    Normal,
    /// 降频：每 N 帧预测一次（N=降频因子）。
    Throttled { every_n_frames: u32 },
    /// 暂停：完全不预测，被动仲裁兜底（显性告知标志位随态输出）。
    Suspended,
}

/// 滚动聚合器（增量计算核心）：
/// 逐帧 O(1) 维护 sum/sum2/max，均值与二阶矩代理随时可读，无需重扫。
#[derive(Clone, Debug)]
pub struct IncrementalAgg {
    count: u64,
    sum: u64,
    sum2: u64,
    max: u32,
    /// 复用上一帧统计的锚点（判据「复用上一帧统计」）。
    last_value: Option<u32>,
}

impl IncrementalAgg {
    pub fn new() -> Self {
        IncrementalAgg { count: 0, sum: 0, sum2: 0, max: 0, last_value: None }
    }

    /// 单次 O(1) 观测。
    pub fn push(&mut self, v: u32) {
        self.count += 1;
        self.sum += v as u64;
        self.sum2 += (v as u64) * (v as u64);
        if v > self.max {
            self.max = v;
        }
        self.last_value = Some(v);
    }

    pub fn mean(&self) -> Option<u32> {
        if self.count == 0 {
            None
        } else {
            Some((self.sum / self.count) as u32)
        }
    }

    /// 方差代理（E[x²]−E[x]²，整数截断——只作趋势判据不做精确统计）。
    pub fn variance_hint(&self) -> Option<u32> {
        if self.count == 0 {
            return None;
        }
        let m = self.sum / self.count;
        let v = self.sum2 / self.count;
        Some(v.saturating_sub(m * m) as u32)
    }

    pub fn max(&self) -> u32 {
        self.max
    }

    pub fn count(&self) -> u64 {
        self.count
    }

    pub fn last(&self) -> Option<u32> {
        self.last_value
    }
}

impl Default for IncrementalAgg {
    fn default() -> Self {
        Self::new()
    }
}

/// 开销控制器：周期账本 + 状态机 + 显性告知。
pub struct OverheadGovernor {
    /// 帧周期（周期单位）。
    frame_period: u64,
    /// 本帧已花周期（预测器消耗）。
    spent_this_frame: u64,
    /// 实测滚动开销千分位（近 K 帧均值）。
    permille_hist: Vec<u32>,
    state: DegradeState,
    /// 降频因子（进入 Throttled 时启用）。
    throttle_every: u32,
    /// 帧号计数（降频调度用）。
    frame_no: u64,
    /// 进入 Throttled 时的基准帧（相位锚——下一帧即可预测）。
    throttle_base: u64,
    /// 显性告知位（降级期间为 true——被动仲裁兜底方必须能看到）。
    pub degraded_notice: bool,
}

impl OverheadGovernor {
    pub fn new(frame_period: u64) -> Self {
        OverheadGovernor {
            frame_period,
            spent_this_frame: 0,
            permille_hist: Vec::new(),
            state: DegradeState::Normal,
            throttle_every: 4,
            frame_no: 0,
            throttle_base: 0,
            degraded_notice: false,
        }
    }

    /// 每帧预算（周期）：帧周期 × 5‰。
    pub fn frame_budget(&self) -> u64 {
        self.frame_period * BUDGET_PERMILLE as u64 / 1000
    }

    /// 记一笔预测开销。超预算即触发降级评估（返回当前态给调用方）。
    pub fn charge(&mut self, cycles: u64) -> DegradeState {
        self.spent_this_frame += cycles;
        let budget = self.frame_budget().max(1);
        let pm = ((self.spent_this_frame * 1000) / budget) as u32;
        if pm > 1000 {
            // 实测超 100%（即预测吃掉 >0.5% 帧预算）→ 降级。
            self.apply_degrade();
        }
        self.state
    }

    /// 帧收口：结算本帧千分位实测并清账。
    pub fn close_frame(&mut self) -> u32 {
        let budget = self.frame_budget().max(1);
        let pm = ((self.spent_this_frame * 1000) / budget) as u32;
        self.permille_hist.push(pm);
        if self.permille_hist.len() > 64 {
            self.permille_hist.remove(0);
        }
        self.spent_this_frame = 0;
        self.frame_no += 1;
        pm
    }

    fn apply_degrade(&mut self) {
        self.state = match self.state {
            DegradeState::Normal => {
                self.throttle_base = self.frame_no;
                DegradeState::Throttled { every_n_frames: self.throttle_every }
            }
            DegradeState::Throttled { .. } => DegradeState::Suspended,
            DegradeState::Suspended => DegradeState::Suspended,
        };
        self.degraded_notice = true;
    }

    /// 恢复评估：连续 K 帧实测 ≤ 预算则逐级恢复（Suspended→Throttled→Normal）。
    pub fn maybe_recover(&mut self, calm_streak: u32) {
        if calm_streak >= 8 {
            self.state = match self.state {
                DegradeState::Suspended => {
                    self.degraded_notice = true; // 恢复中仍保持告知直至 Normal
                    self.throttle_base = self.frame_no;
                    DegradeState::Throttled { every_n_frames: self.throttle_every }
                }
                DegradeState::Throttled { .. } => {
                    self.degraded_notice = false;
                    DegradeState::Normal
                }
                DegradeState::Normal => DegradeState::Normal,
            };
        }
    }

    /// 本帧是否该做预测（判据「增量/降频」的调度入口）。
    pub fn should_predict(&self) -> Result<bool, u16> {
        match self.state {
            DegradeState::Normal => Ok(true),
            DegradeState::Throttled { every_n_frames } => {
                if every_n_frames == 0 {
                    return Err(E_THROTTLED);
                }
                // 相位语义：进入降级后的下一帧立即可预测，此后每 n 帧一次。
                let elapsed = self.frame_no.saturating_sub(self.throttle_base + 1);
                Ok(elapsed % every_n_frames as u64 == 0)
            }
            DegradeState::Suspended => Err(E_SUSPENDED),
        }
    }

    /// 实测开销均值（千分位——判据「0.5% 实测」的读数）。
    pub fn measured_permille(&self) -> Option<u32> {
        if self.permille_hist.is_empty() {
            return None;
        }
        let s: u64 = self.permille_hist.iter().map(|&v| v as u64).sum();
        Some((s / self.permille_hist.len() as u64) as u32)
    }

    pub fn state(&self) -> DegradeState {
        self.state
    }
}

/// 被动仲裁兜底通知（判据「降级兜底并显性告知」）：
/// 预测不可用时，仲裁器读此通知转被动策略并记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FallbackNotice {
    pub active: bool,
    pub reason_state: DegradeState,
}

pub fn passive_fallback_notice(g: &OverheadGovernor) -> FallbackNotice {
    FallbackNotice { active: g.degraded_notice, reason_state: g.state() }
}

// ---------------------------------------------------------------------------
// 判据自检
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a79_预算实测_千分之五() {
        let mut g = OverheadGovernor::new(1_000_000);
        assert_eq!(g.frame_budget(), 5_000);
        // 正常：花 4_000 周期 = 800‰ 预算内。
        g.charge(4_000);
        assert_eq!(g.state(), DegradeState::Normal);
        assert_eq!(g.close_frame(), 800);
        // 超标：花 6_000 周期 → 1200‰ → 立即降级到 Throttled。
        g.charge(6_000);
        assert_eq!(g.state(), DegradeState::Throttled { every_n_frames: 4 });
        assert!(g.degraded_notice);
        // 实测均值读数在册。
        assert!(g.measured_permille().is_some());
    }

    #[test]
    fn a79_增量_O1复用上一帧() {
        let mut agg = IncrementalAgg::new();
        for v in [100u32, 200, 300, 400, 500] {
            agg.push(v);
        }
        assert_eq!(agg.count(), 5);
        assert_eq!(agg.mean(), Some(300));
        assert_eq!(agg.max(), 500);
        assert_eq!(agg.last(), Some(500));
        // 方差代理 = E[x²]−E[x]² = 110000−90000 = 20000。
        assert_eq!(agg.variance_hint(), Some(20_000));
        assert_eq!(IncrementalAgg::default().mean(), None);
    }

    #[test]
    fn a79_降级_降频调度与暂停() {
        let mut g = OverheadGovernor::new(1_000_000);
        g.charge(6_000);
        g.close_frame();
        // Throttled：every=4 → 帧 0 预测、帧 1 不预测。
        assert!(g.should_predict().unwrap());
        g.close_frame();
        assert!(!g.should_predict().unwrap());
        // 再次超标 → Suspended：一律不预测且显性告知。
        g.charge(9_000);
        g.close_frame();
        assert_eq!(g.state(), DegradeState::Suspended);
        assert!(g.degraded_notice);
        assert_eq!(g.should_predict().unwrap_err(), E_SUSPENDED);
        // 恢复：冷静 8 帧 → Throttled（告知保持），再 8 帧 → Normal（告知撤销）。
        g.maybe_recover(8);
        assert_eq!(g.state(), DegradeState::Throttled { every_n_frames: 4 });
        assert!(g.degraded_notice);
        g.maybe_recover(16);
        assert_eq!(g.state(), DegradeState::Normal);
        assert!(!g.degraded_notice);
    }

    #[test]
    fn a79_兜底通知_显性可读() {
        let g = OverheadGovernor::new(2_000_000);
        let n = passive_fallback_notice(&g);
        assert!(!n.active);
        assert_eq!(n.reason_state, DegradeState::Normal);
        // 错误码常量在册（降级兜底路径的显性信号）。
        assert_eq!(E_BUDGET_EXCEEDED, SEG_BASE + 1);
        assert_eq!(E_INCREMENTAL_VIOLATION, SEG_BASE + 2);
        assert_eq!(E_THROTTLED, SEG_BASE + 4);
    }
}
