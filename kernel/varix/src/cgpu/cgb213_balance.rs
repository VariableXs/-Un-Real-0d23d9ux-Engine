//! CGPU-F0213 · 交互响应与吞吐平衡（判据段 0x06F0）
//! 交互响应（低延迟）与吞吐（高帧率）的动态平衡：平衡模式（交互
//! 优先模式：牺牲 5-10% 吞吐换 ≤20% 延迟改善；吞吐优先模式：最大
//! 化帧率但交互延迟上升；平衡模式：P95 延迟与帧率的加权最优）、模
//! 式自动切换（交互事件密度驱动——高交互期切交互优先，空闲期切吞
//! 吐优先）、切换平滑（模式切换不产生帧率跳变——渐变过渡）。判据：
//! 三模式定义与实测、自动切换正确、切换平滑、量化对比、用户可覆盖。
//!
//! 诊断码段 0x06F0（本模块独占：0x06F0~0x06FF）。
//! no_std + alloc 零 unwrap，全部 Result 化。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x06F0;

/// 交互优先模式容忍的吞吐损失上限（千分比 100 = 10%）。
pub const INTERACTIVE_THROUGHPUT_LOSS_MAX_PERMILLE: u32 = 100;

/// 交互优先模式的延迟改善下限（千分比 200 = 20%）。
pub const INTERACTIVE_LATENCY_GAIN_MIN_PERMILLE: u32 = 200;

/// 高交互期事件密度门槛（事件/秒）。
pub const BUSY_EVENT_DENSITY: u32 = 60;

/// 空闲期事件密度门槛（事件/秒）。
pub const IDLE_EVENT_DENSITY: u32 = 10;

/// 平衡模式（判据「三模式定义」一等公民）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BalanceMode {
    /// 交互优先：低延迟让吞吐。
    Interactive,
    /// 吞吐优先：高帧率容忍延迟。
    Throughput,
    /// 平衡：P95 延迟与帧率加权最优。
    Balanced,
}

impl BalanceMode {
    /// 模式序号。
    pub fn ordinal(&self) -> u8 {
        match self {
            BalanceMode::Interactive => 0,
            BalanceMode::Throughput => 1,
            BalanceMode::Balanced => 2,
        }
    }
}

/// 模式观测账（判据「三模式定义与实测」载体）：一帧的延迟与吞吐。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ModeObservation {
    /// 帧号。
    pub frame: u32,
    /// 交互延迟（微秒，输入到呈现）。
    pub latency_us: u32,
    /// 帧率（×100，如 6000 = 60fps）。
    pub fps_cent: u32,
    /// 所处模式。
    pub mode: BalanceMode,
}

impl ModeObservation {
    /// 合法性。
    pub fn validate(&self) -> Result<(), u16> {
        if self.frame == 0 || self.latency_us == 0 || self.fps_cent == 0 {
            return Err(SEG_BASE);
        }
        Ok(())
    }
}

/// 交互优先模式达标（判据「三模式定义与实测」面一）：相对吞吐
/// 优先基线，延迟改善 ≥ 20% 且吞吐损失 ≤ 10%。
pub fn interactive_mode_holds(
    base: &ModeObservation,
    obs: &ModeObservation,
) -> Result<(), u16> {
    base.validate()?;
    obs.validate()?;
    if obs.mode != BalanceMode::Interactive {
        return Err(SEG_BASE + 1);
    }
    if base.mode != BalanceMode::Throughput {
        return Err(SEG_BASE + 2);
    }
    let latency_gain = ((base.latency_us - obs.latency_us.min(base.latency_us)) as u64 * 1000
        / base.latency_us as u64) as u32;
    let fps_loss = base.fps_cent.abs_diff(obs.fps_cent) as u64 * 1000 / base.fps_cent as u64;
    if latency_gain < INTERACTIVE_LATENCY_GAIN_MIN_PERMILLE {
        return Err(SEG_BASE + 3);
    }
    if fps_loss > INTERACTIVE_THROUGHPUT_LOSS_MAX_PERMILLE as u64 {
        return Err(SEG_BASE + 4);
    }
    Ok(())
}

/// 模式自动切换裁决（判据「自动切换正确」核心）：事件密度驱动——
/// 密度 ≥ 60 → Interactive；≤ 10 → Throughput；中间 → Balanced。
pub fn auto_switch(event_density_per_sec: u32, current: BalanceMode) -> Result<BalanceMode, u16> {
    let next = if event_density_per_sec >= BUSY_EVENT_DENSITY {
        BalanceMode::Interactive
    } else if event_density_per_sec <= IDLE_EVENT_DENSITY {
        BalanceMode::Throughput
    } else {
        BalanceMode::Balanced
    };
    let _ = current;
    Ok(next)
}

/// 切换平滑性（判据「切换平滑」主判据）：模式切换帧的帧率跳变
/// 不得超过 ±10%（千分比 100）——渐变过渡的要求。
pub fn switch_smooth(prev_fps_cent: u32, next_fps_cent: u32) -> Result<(), u16> {
    if prev_fps_cent == 0 || next_fps_cent == 0 {
        return Err(SEG_BASE + 5);
    }
    let jump = prev_fps_cent.abs_diff(next_fps_cent) as u64 * 1000 / prev_fps_cent as u64;
    if jump > 100 {
        return Err(SEG_BASE + 6);
    }
    Ok(())
}

/// 渐变过渡计划（判据「切换平滑」执行面）：从当前帧率到目标帧率
/// 的 N 步渐变——每步变化量 = |target−current|/steps，每步都必须
/// 满足平滑带。返回渐变序列。
pub fn ramp_plan(current_fps_cent: u32, target_fps_cent: u32, steps: u32) -> Result<Vec<u32>, u16> {
    if steps == 0 || steps > 32 || current_fps_cent == 0 || target_fps_cent == 0 {
        return Err(SEG_BASE + 7);
    }
    let mut out: Vec<u32> = Vec::new();
    let diff = target_fps_cent as i64 - current_fps_cent as i64;
    for k in 1..=steps {
        let v = current_fps_cent as i64 + diff * k as i64 / steps as i64;
        out.push(v.max(1) as u32);
    }
    // 渐变末端必须精确到达目标。
    if *out.last().ok_or(SEG_BASE + 8)? != target_fps_cent {
        return Err(SEG_BASE + 9);
    }
    Ok(out)
}

/// 渐变序列全程平滑（判据「切换平滑」审计面）：相邻两步跳变均
/// 不超带。
pub fn ramp_smooth(plan: &[u32]) -> Result<(), u16> {
    if plan.len() < 2 {
        return Err(SEG_BASE + 10);
    }
    for w in plan.windows(2) {
        switch_smooth(w[0], w[1])?;
    }
    Ok(())
}

/// P95 延迟与帧率加权评分（判据「平衡模式」核心量）：score =
/// 1000 − (p95延迟/预算 ×500 + 帧率缺口/目标 ×500)×权重——分高者
/// 胜出。返回最优模式。
pub fn balanced_score(
    p95_latency_us: u32,
    latency_budget_us: u32,
    fps_cent: u32,
    target_fps_cent: u32,
) -> Result<i64, u16> {
    if latency_budget_us == 0 || target_fps_cent == 0 {
        return Err(SEG_BASE + 11);
    }
    let lat_term = (p95_latency_us as u64 * 1000 / latency_budget_us as u64).min(2000) as i64;
    let fps_term = if fps_cent >= target_fps_cent {
        0i64
    } else {
        ((target_fps_cent - fps_cent) as u64 * 1000 / target_fps_cent as u64).min(2000) as i64
    };
    Ok(1000 - (lat_term + fps_term) / 2)
}

/// 量化对比（判据「量化对比」主判据）：三模式在同一帧窗口的评分
/// 对拍，返回最优模式（评分最高）。
pub fn pick_best_mode(candidates: &[(BalanceMode, i64)]) -> Result<BalanceMode, u16> {
    if candidates.len() < 3 {
        return Err(SEG_BASE + 12);
    }
    let mut seen = [false; 3];
    for (m, s) in candidates {
        seen[m.ordinal() as usize] = true;
        if *s > 2000 || *s < -2000 {
            return Err(SEG_BASE + 13);
        }
    }
    if seen.iter().any(|s| !s) {
        return Err(SEG_BASE + 14);
    }
    Ok(candidates
        .iter()
        .max_by_key(|(_, s)| *s)
        .map(|(m, _)| *m)
        .ok_or(SEG_BASE + 15)?)
}

/// 用户覆盖（判据「用户可覆盖」语义面）：用户手动指定的模式覆盖
/// 自动裁决——返回 Some(用户模式)；None = 交给自动。
pub fn user_override(auto: BalanceMode, manual: Option<BalanceMode>) -> BalanceMode {
    manual.unwrap_or(auto)
}

/// 模式切换账指纹（可审计面）。
pub fn mode_obs_fp(o: &ModeObservation) -> u64 {
    let mut acc: u64 = 0xcbf29ce484222325;
    acc = acc.wrapping_mul(0x100000001b3) ^ o.frame as u64;
    acc = acc.wrapping_mul(0x100000001b3) ^ o.latency_us as u64;
    acc = acc.wrapping_mul(0x100000001b3) ^ o.fps_cent as u64;
    acc = acc.wrapping_mul(0x100000001b3) ^ o.mode.ordinal() as u64;
    acc
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn interactive_mode_bounds() {
        let base = ModeObservation { frame: 1, latency_us: 50_000, fps_cent: 6000, mode: BalanceMode::Throughput };
        // 延迟 40ms（20% 改善）、帧率 5900（1.7% 损失）→ 达标。
        let good = ModeObservation { frame: 2, latency_us: 40_000, fps_cent: 5900, mode: BalanceMode::Interactive };
        assert!(interactive_mode_holds(&base, &good).is_ok());
        // 延迟只改善 10% → 不达标。
        let weak = ModeObservation { frame: 2, latency_us: 45_000, fps_cent: 5900, mode: BalanceMode::Interactive };
        assert!(interactive_mode_holds(&base, &weak).is_err());
        // 吞吐损失 20% → 超限。
        let costly = ModeObservation { frame: 2, latency_us: 40_000, fps_cent: 4800, mode: BalanceMode::Interactive };
        assert!(interactive_mode_holds(&base, &costly).is_err());
    }
    #[test]
    fn auto_switch_three_zones() {
        assert_eq!(auto_switch(80, BalanceMode::Throughput).unwrap(), BalanceMode::Interactive);
        assert_eq!(auto_switch(5, BalanceMode::Interactive).unwrap(), BalanceMode::Throughput);
        assert_eq!(auto_switch(30, BalanceMode::Throughput).unwrap(), BalanceMode::Balanced);
    }
    #[test]
    fn switch_smoothness() {
        assert!(switch_smooth(6000, 5900).is_ok());
        assert!(switch_smooth(6000, 4000).is_err());
    }
    #[test]
    fn ramp_reaches_target_smoothly() {
        let plan = ramp_plan(6000, 5000, 10).unwrap();
        assert_eq!(*plan.last().unwrap(), 5000);
        assert_eq!(plan.len(), 10);
        assert!(ramp_smooth(&plan).is_ok());
        // 硬切（1 步 1000 跳变 > 10% 带）→ 拒绝。
        let hard = ramp_plan(6000, 5000, 1).unwrap();
        assert!(ramp_smooth(&hard).is_err());
    }
    #[test]
    fn balanced_score_ranks() {
        // 预算内低延迟高帧率 → 高分。
        let s1 = balanced_score(20_000, 50_000, 6000, 6000).unwrap();
        let s2 = balanced_score(50_000, 50_000, 5000, 6000).unwrap();
        assert!(s1 > s2);
    }
    #[test]
    fn best_mode_selection() {
        let cands = vec![
            (BalanceMode::Interactive, -100i64),
            (BalanceMode::Throughput, 50i64),
            (BalanceMode::Balanced, 300i64),
        ];
        assert_eq!(pick_best_mode(&cands).unwrap(), BalanceMode::Balanced);
        // 三模式不全 → 拒绝。
        let partial = vec![(BalanceMode::Interactive, 0i64), (BalanceMode::Throughput, 1i64)];
        assert!(pick_best_mode(&partial).is_err());
    }
    #[test]
    fn user_override_semantics() {
        assert_eq!(user_override(BalanceMode::Balanced, Some(BalanceMode::Interactive)), BalanceMode::Interactive);
        assert_eq!(user_override(BalanceMode::Balanced, None), BalanceMode::Balanced);
    }
}
