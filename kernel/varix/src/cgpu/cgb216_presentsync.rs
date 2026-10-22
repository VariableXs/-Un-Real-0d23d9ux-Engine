//! CGPU-F0216 · 呈现同步优化（判据段 0x0720）
//! 任务完成与呈现时机的同步优化：呈现预测（GPU 完成时间预测——预
//! 测准确性驱动呈现调度：预测 80fps 帧在哪个 vsync 落地）、提前呈现
//! （GPU 提前完成时提前翻转——减少延迟）、延迟呈现（GPU 延迟完成
//! 时的丢帧策略——丢最老帧保持最新）。呈现与 F0048 三模式联动。
//! 判据：预测准确率、提前/延迟策略、丢帧语义、延迟改善、对拍。
//!
//! 诊断码段 0x0720（本模块独占：0x0720~0x072F）。
//! no_std + alloc 零 unwrap，全部 Result 化。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x0720;

/// vsync 周期（微秒）：60Hz = 16667µs。
pub const VSYNC_PERIOD_US: u32 = 16_667;

/// 呈现预测准确率门槛（千分比 800 = 80%）。
pub const PREDICT_ACCURACY_MIN_PERMILLE: u32 = 800;

/// 预测误差门槛（vsync 周期数）：误差 ≤ 1 周期算预测命中。
pub const PREDICT_TOLERANCE_TICKS: u32 = 1;

/// 一次呈现观测：GPU 实际完成时刻 vs 呈现时刻。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PresentObs {
    /// 帧号。
    pub frame: u32,
    /// GPU 完成时刻（微秒，帧起点相对）。
    pub gpu_done_us: u32,
    /// 实际呈现时刻（微秒）。
    pub present_us: u32,
}

impl PresentObs {
    /// 合法性：呈现不得早于 GPU 完成（翻转依赖帧就绪）。
    pub fn validate(&self) -> Result<(), u16> {
        if self.frame == 0 || self.gpu_done_us == 0 || self.present_us == 0 {
            return Err(SEG_BASE);
        }
        if self.present_us < self.gpu_done_us {
            return Err(SEG_BASE + 1);
        }
        Ok(())
    }
}

/// vsync 槽位计算（判据「呈现预测」底座）：时刻 t 落在第几个 vsync
/// 槽（向上取整——呈现必须等下一个 vsync 边沿）。
pub fn vsync_slot(t_us: u32) -> Result<u32, u16> {
    if t_us == 0 {
        return Err(SEG_BASE + 2);
    }
    Ok((t_us + VSYNC_PERIOD_US - 1) / VSYNC_PERIOD_US)
}

/// 呈现预测（判据「呈现预测」核心）：由 GPU 完成时刻预测呈现
/// vsync 槽——槽 = ceil(gpu_done / 周期)。
pub fn predict_present_slot(gpu_done_us: u32) -> Result<u32, u16> {
    vsync_slot(gpu_done_us)
}

/// 预测准确性（判据「预测准确率」主判据）：预测槽 vs 实际槽的
/// 命中率——误差 ≤ 容差周期数算命中。样本 < 5 拒绝下结论。
pub fn predict_accuracy_permille(preds: &[u32], actuals: &[u32]) -> Result<u32, u16> {
    if preds.len() != actuals.len() || preds.len() < 5 {
        return Err(SEG_BASE + 3);
    }
    let mut hits = 0u32;
    for (p, a) in preds.iter().zip(actuals.iter()) {
        if p.abs_diff(*a) <= PREDICT_TOLERANCE_TICKS {
            hits += 1;
        }
    }
    let rate = hits * 1000 / preds.len() as u32;
    if rate < PREDICT_ACCURACY_MIN_PERMILLE {
        return Err(SEG_BASE + 4);
    }
    Ok(rate)
}

/// 呈现策略（判据「提前/延迟策略」一等公民）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PresentPolicy {
    /// 提前呈现：GPU 提前完成 → 提前翻转（预测槽之前即就绪）。
    Early,
    /// 准时呈现：落在预测槽。
    OnTime,
    /// 延迟呈现：GPU 晚完成 → 丢最老帧保持最新。
    LateDropped,
}

impl PresentPolicy {
    /// 策略码。
    pub fn code(&self) -> u8 {
        match self {
            PresentPolicy::Early => 0,
            PresentPolicy::OnTime => 1,
            PresentPolicy::LateDropped => 2,
        }
    }
}

/// 呈现策略裁决（判据「提前/延迟策略」主判据）：GPU 完成时刻相对
/// vsync 槽边界的裕量驱动——裕量 > 50% 周期 → Early（下一槽就有
/// 富余）；裕量 < 20% → 准时；超槽（GPU 晚于预测槽）→ 丢帧策略。
pub fn decide_policy(gpu_done_us: u32, predicted_slot: u32) -> Result<PresentPolicy, u16> {
    if gpu_done_us == 0 || predicted_slot == 0 {
        return Err(SEG_BASE + 5);
    }
    let done_slot = vsync_slot(gpu_done_us)?;
    if done_slot > predicted_slot {
        return Ok(PresentPolicy::LateDropped);
    }
    let slot_start = (predicted_slot - 1) * VSYNC_PERIOD_US;
    let margin = slot_start.saturating_sub(gpu_done_us) as u64;
    let half = VSYNC_PERIOD_US as u64 / 2;
    let fifth = VSYNC_PERIOD_US as u64 / 5;
    if margin >= half {
        Ok(PresentPolicy::Early)
    } else if margin >= fifth {
        Ok(PresentPolicy::OnTime)
    } else {
        // 裕量过小但未超槽——保守准时。
        Ok(PresentPolicy::OnTime)
    }
}

/// 丢帧语义（判据「丢帧语义」核心）：延迟呈现时丢最老帧保持最新
/// ——帧队列深度 ≤ 2（当前 + 下一帧），入队第 3 帧必须淘汰队头。
pub fn drop_oldest(queue: &mut Vec<u32>, incoming: u32, max_depth: usize) -> Result<Option<u32>, u16> {
    if incoming == 0 || max_depth < 2 {
        return Err(SEG_BASE + 6);
    }
    let mut dropped = None;
    if queue.len() >= max_depth {
        dropped = Some(queue.remove(0));
    }
    queue.push(incoming);
    Ok(dropped)
}

/// 丢帧新鲜度断言（判据「丢帧语义」不变量）：丢帧后队列内的帧
/// 必须严格新于被丢帧（丢最老 = 保最新）。
pub fn queue_freshness(queue: &[u32], dropped: u32) -> Result<(), u16> {
    if dropped == 0 {
        return Err(SEG_BASE + 7);
    }
    if queue.iter().any(|&f| f <= dropped) {
        return Err(SEG_BASE + 8);
    }
    Ok(())
}

/// 延迟改善量化（判据「延迟改善」主判据）：提前呈现相对准时呈现
/// 的延迟改善（微秒）= 两者呈现时刻差。改善必须为正且 ≤ 一个
/// vsync 周期（提前超过一周期 = 语义错误）。
pub fn early_present_gain_us(ontime_present_us: u32, early_present_us: u32) -> Result<u32, u16> {
    if ontime_present_us == 0 || early_present_us == 0 {
        return Err(SEG_BASE + 9);
    }
    if early_present_us >= ontime_present_us {
        return Err(SEG_BASE + 10);
    }
    let gain = ontime_present_us - early_present_us;
    if gain > VSYNC_PERIOD_US {
        return Err(SEG_BASE + 11);
    }
    Ok(gain)
}

/// 呈现对拍（判据「对拍」主判据）：同一帧预测呈现时刻 vs 实际
/// 呈现时刻的一致性——差 ≤ 一个周期（预测语义内）。
pub fn present_parity(predicted_us: u32, actual_us: u32) -> Result<bool, u16> {
    if predicted_us == 0 || actual_us == 0 {
        return Err(SEG_BASE + 12);
    }
    Ok(predicted_us.abs_diff(actual_us) <= VSYNC_PERIOD_US)
}

/// 呈现遥测账（判据「预测准确率」容器）：滚动观测 + 策略分布。
#[derive(Default)]
pub struct PresentTelemetry {
    obs: Vec<PresentObs>,
    policy_counts: [u32; 3],
}

impl PresentTelemetry {
    /// 记录一次呈现。
    pub fn observe(&mut self, o: PresentObs, policy: PresentPolicy) -> Result<(), u16> {
        o.validate()?;
        if self.obs.iter().any(|x| x.frame == o.frame) {
            return Err(SEG_BASE + 13);
        }
        if self.obs.len() >= 256 {
            self.obs.remove(0);
        }
        self.policy_counts[policy.code() as usize] += 1;
        self.obs.push(o);
        Ok(())
    }
    /// 观测数。
    pub fn count(&self) -> usize {
        self.obs.len()
    }
    /// 策略分布（各策略计数）。
    pub fn policy_dist(&self) -> [u32; 3] {
        self.policy_counts
    }
    /// 平均呈现延迟（GPU 完成 → 呈现）。
    pub fn avg_present_latency_us(&self) -> Result<u32, u16> {
        if self.obs.is_empty() {
            return Err(SEG_BASE + 14);
        }
        let sum: u64 = self.obs.iter().map(|o| (o.present_us - o.gpu_done_us) as u64).sum();
        Ok((sum / self.obs.len() as u64) as u32)
    }
    /// 遥测指纹。
    pub fn telemetry_fp(&self) -> u64 {
        let mut acc: u64 = 0xcbf29ce484222325;
        for o in &self.obs {
            acc = acc.wrapping_mul(0x100000001b3) ^ o.frame as u64;
            acc = acc.wrapping_mul(0x100000001b3) ^ o.present_us as u64;
        }
        acc
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn obs(frame: u32, gpu: u32, pres: u32) -> PresentObs {
        PresentObs { frame, gpu_done_us: gpu, present_us: pres }
    }
    #[test]
    fn vsync_slot_math() {
        assert_eq!(vsync_slot(1).unwrap(), 1);
        assert_eq!(vsync_slot(16667).unwrap(), 1);
        assert_eq!(vsync_slot(16668).unwrap(), 2);
        assert_eq!(vsync_slot(33334).unwrap(), 2);
        assert!(vsync_slot(0).is_err());
    }
    #[test]
    fn prediction_accuracy_gate() {
        let preds = vec![1u32, 2, 3, 4, 5];
        let actuals = vec![1u32, 2, 3, 4, 5];
        assert_eq!(predict_accuracy_permille(&preds, &actuals).unwrap(), 1000);
        // 4/5 命中（80%）→ 过门槛。
        let actuals2 = vec![1u32, 2, 3, 4, 9];
        assert_eq!(predict_accuracy_permille(&preds, &actuals2).unwrap(), 800);
        // 3/5 → 不过。
        let actuals3 = vec![1u32, 2, 3, 9, 9];
        assert!(predict_accuracy_permille(&preds, &actuals3).is_err());
    }
    #[test]
    fn policy_decision_by_margin() {
        // GPU 在槽 2 开始前很久完成（裕量 > 50%）→ Early。
        // 槽 2 起点 16667；GPU 5000 完成 → 裕量 11667 > 8333。
        assert_eq!(decide_policy(5000, 2).unwrap(), PresentPolicy::Early);
        // GPU 在槽内完成但预测槽还没到——不可能（done_slot=predicted）。
        // GPU 超预测槽 → 丢帧。
        assert_eq!(decide_policy(34000, 1).unwrap(), PresentPolicy::LateDropped);
        // 裕量小 → 准时。
        assert_eq!(decide_policy(15500, 2).unwrap(), PresentPolicy::OnTime);
    }
    #[test]
    fn drop_oldest_keeps_fresh() {
        let mut q = vec![10u32, 11];
        let dropped = drop_oldest(&mut q, 12, 2).unwrap();
        assert_eq!(dropped, Some(10));
        assert_eq!(q, vec![11, 12]);
        queue_freshness(&q, 10).unwrap();
        // 深度不足拒绝。
        assert!(drop_oldest(&mut q, 13, 1).is_err());
        // 新鲜度断言失败样例。
        assert!(queue_freshness(&[9u32], 10).is_err());
    }
    #[test]
    fn early_gain_bounded() {
        assert_eq!(early_present_gain_us(34000, 20000).unwrap(), 14000);
        // 改善超过一周期 → 语义错误。
        assert!(early_present_gain_us(40000, 10000).is_err());
        assert!(early_present_gain_us(10000, 12000).is_err());
    }
    #[test]
    fn parity_one_vsync_window() {
        assert!(present_parity(16667, 20000).unwrap());
        assert!(!present_parity(16667, 40000).unwrap());
        assert!(present_parity(0, 1).is_err());
    }
    #[test]
    fn telemetry_lifecycle() {
        let mut t = PresentTelemetry::default();
        t.observe(obs(1, 5000, 17000), PresentPolicy::Early).unwrap();
        t.observe(obs(2, 15000, 17000), PresentPolicy::OnTime).unwrap();
        assert_eq!(t.count(), 2);
        assert_eq!(t.policy_dist(), [1, 1, 0]);
        assert_eq!(t.avg_present_latency_us().unwrap(), 7000);
        assert!(t.telemetry_fp() != 0);
        // 重复帧拒绝。
        assert!(t.observe(obs(1, 100, 200), PresentPolicy::Early).is_err());
    }
}
