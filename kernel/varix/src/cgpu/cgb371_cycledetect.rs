//! CGPU-F0371 · 周期模式识别（判据段 0x09B0）
//! 模式识别：周期检测（动画节奏/滚动节律的自相关统计识别）；周
//! 期注册（识别到的周期→预测先验）；失效检测（节奏变化→先验失
//! 效回退）；多周期并存（主周期+次周期）。判据：自相关、先验、
//! 失效回退、多周期、判据。
//!
//! 锚点原文摘录（施工书 1649 行）：
//! 「周期检测（自相关统计识别）；周期注册（识别到的周期→预测先
//! 验）；失效检测（节奏变化→先验失效回退）；多周期并存（主周期
//! +次周期）。判据：自相关、先验、失效回退、多周期、判据。」
//!
//! 诊断码段 0x09B0（本模块独占：0x09B0~0x09BF）。
//! no_std + alloc 零 unwrap（测试除外），全部 Result 化。

use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x09B0;

/// 自相关窗长（帧）：识别窗固定 64 帧样本。
pub const AUTOCORR_WINDOW: usize = 64;

/// 周期搜索范围（帧）：4..=32——动画节律典型域。
pub const PERIOD_MIN: u32 = 4;
pub const PERIOD_MAX: u32 = 32;

/// 周期强度达标线（千分比）：归一化自相关峰 ≥ 600‰ 才算周期。
pub const PERIOD_STRENGTH_PERMILLE: u32 = 600;

/// 失效判定容差（帧）：实测节律偏离先验超 ±1 帧即失效候选。
pub const DRIFT_TOLERANCE_FRAMES: u32 = 1;

/// 归一化自相关（判据「自相关」主判据核）：对 lag ∈ [PERIOD_MIN,
/// PERIOD_MAX] 计算 Pearson 式离散自相关（千分比，零浮点——整数
/// 交叉积 / 能量积）。返回（最佳周期, 强度千分比）。
pub fn autocorr_best(samples: &[u32]) -> Result<(u32, u32), u16> {
    if samples.len() < AUTOCORR_WINDOW {
        return Err(SEG_BASE);
    }
    let w = &samples[..AUTOCORR_WINDOW];
    let mean: u64 = w.iter().map(|&v| v as u64).sum::<u64>() / AUTOCORR_WINDOW as u64;
    // 去均值后的能量。
    let energy: i64 = w.iter().map(|&v| v as i64 - mean as i64).map(|d| d * d).sum();
    if energy == 0 {
        // 恒定信号：无周期可谈——诚实返回零强度。
        return Ok((0, 0));
    }
    let mut best_lag = 0u32;
    let mut best_score = 0i64;
    for lag in PERIOD_MIN..=PERIOD_MAX {
        let lag = lag as usize;
        let mut cross: i64 = 0;
        for i in 0..AUTOCORR_WINDOW - lag {
            let a = w[i] as i64 - mean as i64;
            let b = w[i + lag] as i64 - mean as i64;
            cross += a * b;
        }
        if cross > best_score {
            best_score = cross;
            best_lag = lag as u32;
        }
    }
    // 归一化：score / energy，千分比。
    let strength = ((best_score as i128 * 1000) / energy as i128) as u32;
    if best_lag == 0 {
        return Ok((0, strength.min(999)));
    }
    Ok((best_lag, strength.min(1000)))
}

/// 周期先验（判据「先验」一等公民）：识别结果冻结为预测先验。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CyclePrior {
    /// 周期（帧）。
    pub period: u32,
    /// 强度（千分比）。
    pub strength_permille: u32,
    /// 是否主周期。
    pub primary: bool,
}

impl CyclePrior {
    /// 注册合法性（判据「先验」守卫）：周期在搜索域内且强度达标。
    pub fn validate(&self) -> Result<(), u16> {
        if self.period < PERIOD_MIN || self.period > PERIOD_MAX {
            return Err(SEG_BASE + 1);
        }
        if self.strength_permille < PERIOD_STRENGTH_PERMILLE {
            return Err(SEG_BASE + 2);
        }
        Ok(())
    }
    /// 相位预测（先验的用途）：第 n 帧处于周期内的相位。
    pub fn phase_at(&self, frame: u64) -> u32 {
        (frame % self.period as u64) as u32
    }
}

/// 先验注册表（判据「多周期」主判据）：主周期恰一 + 次周期若干
/// （上限 3），主周期强度必须 ≥ 每个次周期。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PriorRegistry {
    /// 先验行。
    pub priors: Vec<CyclePrior>,
}

impl PriorRegistry {
    /// 空。
    pub fn new() -> Self {
        PriorRegistry { priors: Vec::new() }
    }
    /// 注册先验（判据「多周期」守卫）：总量 ≤4；主周期唯一——
    /// 再注册主周期时若新强度更高则顶替，否则拒绝。
    pub fn register(&mut self, p: CyclePrior) -> Result<(), u16> {
        p.validate()?;
        if p.primary {
            if let Some(cur) = self.priors.iter_mut().find(|c| c.primary) {
                if p.strength_permille > cur.strength_permille {
                    *cur = p;
                    return Ok(());
                }
                return Err(SEG_BASE + 3);
            }
        }
        if self.priors.len() >= 4 {
            return Err(SEG_BASE + 4);
        }
        // 次周期与主周期同频 = 重复注册拒绝。
        if self.priors.iter().any(|c| c.period == p.period) {
            return Err(SEG_BASE + 5);
        }
        self.priors.push(p);
        Ok(())
    }
    /// 完备核（判据「多周期」守卫）：主周期恰一且强度最大。
    pub fn consistent(&self) -> Result<(), u16> {
        let primaries: Vec<&CyclePrior> = self.priors.iter().filter(|c| c.primary).collect();
        if primaries.len() != 1 {
            return Err(SEG_BASE + 6);
        }
        let p = primaries[0];
        for c in &self.priors {
            if c.strength_permille > p.strength_permille {
                return Err(SEG_BASE + 7);
            }
        }
        Ok(())
    }
}

impl Default for PriorRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// 失效检测器（判据「失效回退」主判据）：逐帧实测节律间隔与先验
/// 比对——连续 DRIFT_CONFIRM 次超容差即判失效（先验作废回退被动）。
pub const DRIFT_CONFIRM: u32 = 3;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DriftDetector {
    /// 先验周期。
    pub prior_period: u32,
    /// 连续漂移计数。
    pub drift_streak: u32,
    /// 已判失效。
    pub invalidated: bool,
}

impl DriftDetector {
    /// 建（先验存在才可建）。
    pub fn new(prior_period: u32) -> Result<Self, u16> {
        if prior_period < PERIOD_MIN || prior_period > PERIOD_MAX {
            return Err(SEG_BASE + 8);
        }
        Ok(DriftDetector { prior_period, drift_streak: 0, invalidated: false })
    }
    /// 观测一个实测节律间隔（判据「失效回退」逐步核）。
    pub fn observe(&mut self, measured_interval: u32) -> Result<bool, u16> {
        if self.invalidated {
            return Ok(true);
        }
        if measured_interval > self.prior_period + DRIFT_TOLERANCE_FRAMES
            || self.prior_period > measured_interval + DRIFT_TOLERANCE_FRAMES
        {
            self.drift_streak += 1;
        } else {
            self.drift_streak = 0;
        }
        if self.drift_streak >= DRIFT_CONFIRM {
            self.invalidated = true;
        }
        Ok(self.invalidated)
    }
    /// 回退核（判据「失效回退」验收面）：失效即回退被动——先验
    /// 不可再用于预测。
    pub fn prior_usable(&self) -> bool {
        !self.invalidated
    }
}

/// 识别→先验→失效 全链（判据「自相关、先验、失效回退」联动）：
/// 对窗内样本识别；强度达标则出先验，否则显性「无周期」。
pub fn identify_and_register(samples: &[u32], reg: &mut PriorRegistry, primary: bool) -> Result<Option<CyclePrior>, u16> {
    let (lag, strength) = autocorr_best(samples)?;
    if lag == 0 || strength < PERIOD_STRENGTH_PERMILLE {
        return Ok(None);
    }
    let prior = CyclePrior { period: lag, strength_permille: strength, primary };
    reg.register(prior)?;
    Ok(Some(prior))
}

/// 诊断明细（错误码 → 中文可操作建议）。
pub fn diagnose(code: u16) -> &'static str {
    let d = code.wrapping_sub(SEG_BASE);
    match d {
        0 => "样本不足一个识别窗（64）",
        1 => "周期越搜索域 [4,32]",
        2 => "强度未达标（600‰）",
        3 => "主周期顶替失败——新强度不高于现值",
        4 => "先验表满（4 行）",
        5 => "同频重复注册",
        6 => "主周期不唯一",
        7 => "次周期强度超主周期",
        8 => "先验周期非法",
        _ => "未知诊断码——查本模块 SEG_BASE 0x09B0",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 合成周期信号：周期 8，方波节律。
    fn periodic(seed_amp: u32) -> Vec<u32> {
        (0..AUTOCORR_WINDOW).map(|i| if i % 8 < 4 { seed_amp } else { 0 }).collect()
    }

    #[test]
    fn autocorr_finds_period8() {
        let s = periodic(300);
        let (lag, strength) = autocorr_best(&s).unwrap();
        assert_eq!(lag, 8);
        assert!(strength >= PERIOD_STRENGTH_PERMILLE, "strength={}", strength);
        // 恒定信号：诚实零强度。
        let flat = vec![500u32; AUTOCORR_WINDOW];
        let (l2, s2) = autocorr_best(&flat).unwrap();
        assert_eq!(l2, 0);
        assert_eq!(s2, 0);
        assert_eq!(autocorr_best(&flat[..10]).unwrap_err(), SEG_BASE);
    }

    #[test]
    fn registry_multi_cycle() {
        let mut reg = PriorRegistry::new();
        let p1 = identify_and_register(&periodic(300), &mut reg, true).unwrap().unwrap();
        assert_eq!(p1.period, 8);
        // 次周期：周期 16 合成。
        let s16: Vec<u32> = (0..AUTOCORR_WINDOW).map(|i| if i % 16 < 8 { 200 } else { 0 }).collect();
        let p2 = identify_and_register(&s16, &mut reg, false).unwrap().unwrap();
        assert_eq!(p2.period, 16);
        assert!(reg.consistent().is_ok());
        // 同频重复拒绝。
        assert_eq!(reg.register(CyclePrior { period: 16, strength_permille: 700, primary: false }).unwrap_err(), SEG_BASE + 5);
        // 弱强度拒绝。
        assert_eq!(reg.register(CyclePrior { period: 12, strength_permille: 100, primary: false }).unwrap_err(), SEG_BASE + 2);
        assert_eq!(reg.priors.len(), 2);
        // 相位预测。
        assert_eq!(p1.phase_at(17), 1);
    }

    #[test]
    fn drift_invalidation() {
        let mut d = DriftDetector::new(8).unwrap();
        // 容差内：不漂。
        assert!(!d.observe(8).unwrap());
        assert!(!d.observe(9).unwrap());
        // 连续 3 次超容差：失效。
        assert!(!d.observe(12).unwrap());
        assert!(!d.observe(12).unwrap());
        assert!(d.observe(12).unwrap());
        assert!(!d.prior_usable());
        // 失效后恒报失效。
        assert!(d.observe(8).unwrap());
        assert_eq!(DriftDetector::new(3).unwrap_err(), SEG_BASE + 8);
        assert!(diagnose(SEG_BASE + 8).contains("非法"));
    }
}
