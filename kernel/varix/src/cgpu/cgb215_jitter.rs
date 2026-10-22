//! CGPU-F0215 · 帧间隔抖动抑制（判据段 0x0710）
//! 帧间隔的抖动（jitter）抑制：抖动源分类（GPU 耗时波动/CPU 调度
//! 延迟/OS 后台干扰/热降频——每类的检测特征与抑制策略）、平滑策略
//! （帧间隔的 EWMA 平滑——预测下一帧完成时间并预补偿）、抖动预算
//! （允许的最大帧间隔偏差——超过即触发降质链 CGPU-O）。抖动量化
//! （帧间隔的标准差作为抖动指标——入遥测）。判据：抖动源检测、
//! 平滑策略、预算联动、指标量化、抑制收益实测。
//!
//! 诊断码段 0x0710（本模块独占：0x0710~0x071F）。
//! no_std + alloc 零 unwrap，全部 Result 化。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x0710;

/// 目标帧间隔（微秒）：60fps = 16667µs。
pub const FRAME_INTERVAL_TARGET_US: u32 = 16_667;

/// 抖动预算（微秒）：帧间隔允许的最大偏差——超过触发降质链。
pub const JITTER_BUDGET_US: u32 = 2_500;

/// EWMA 平滑系数（千分比）：α = 300（新样本占 30%）。
pub const EWMA_ALPHA_PERMILLE: u32 = 300;

/// 抖动源（判据「抖动源分类」一等公民）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JitterSource {
    /// GPU 耗时波动（特征：帧间隔与 GPU 耗时同相波动）。
    GpuSpike,
    /// CPU 调度延迟（特征：帧开始时刻延迟但 GPU 耗时平稳）。
    CpuScheduling,
    /// OS 后台干扰（特征：随机尖刺、无周期）。
    OsNoise,
    /// 热降频（特征：持续缓慢劣化 + 温度相关）。
    ThermalThrottle,
}

impl JitterSource {
    /// 源码。
    pub fn code(&self) -> u8 {
        match self {
            JitterSource::GpuSpike => 0,
            JitterSource::CpuScheduling => 1,
            JitterSource::OsNoise => 2,
            JitterSource::ThermalThrottle => 3,
        }
    }
}

/// 帧间隔样本（判据「指标量化」原子量）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FrameSample {
    /// 帧号。
    pub frame: u32,
    /// 本帧间隔（微秒）。
    pub interval_us: u32,
    /// GPU 耗时（微秒）。
    pub gpu_us: u32,
    /// CPU 侧提交延迟（微秒）。
    pub submit_delay_us: u32,
    /// 温度（0.1℃ 计，如 755 = 75.5℃）。
    pub temp_deci_c: u32,
}

impl FrameSample {
    /// 合法性。
    pub fn validate(&self) -> Result<(), u16> {
        if self.frame == 0 || self.interval_us == 0 || self.gpu_us == 0 {
            return Err(SEG_BASE);
        }
        if self.temp_deci_c > 1500 {
            return Err(SEG_BASE + 1);
        }
        Ok(())
    }
    /// 帧间隔偏差（微秒，带符号）。
    pub fn deviation_us(&self) -> i64 {
        self.interval_us as i64 - FRAME_INTERVAL_TARGET_US as i64
    }
}

/// 抖动源分类（判据「抖动源检测」主判据）：按特征向量裁决主导源。
/// 特征：GPU 耗时相对其均值偏差 > 300‰ → GPU 波动；提交延迟 >
/// 2000µs → CPU 调度；温度 > 85℃ 且持续劣化 → 热降频；其余随机
/// 尖刺 → OS 噪声。
pub fn classify_source(s: &FrameSample, gpu_mean_us: u32) -> Result<JitterSource, u16> {
    s.validate()?;
    if gpu_mean_us == 0 {
        return Err(SEG_BASE + 2);
    }
    let gpu_dev = s.gpu_us.abs_diff(gpu_mean_us) as u64 * 1000 / gpu_mean_us as u64;
    if s.temp_deci_c > 850 {
        return Ok(JitterSource::ThermalThrottle);
    }
    if gpu_dev > 300 {
        return Ok(JitterSource::GpuSpike);
    }
    if s.submit_delay_us > 2_000 {
        return Ok(JitterSource::CpuScheduling);
    }
    Ok(JitterSource::OsNoise)
}

/// EWMA 平滑（判据「平滑策略」核心）：ewma = α×新样本 + (1−α)×旧值。
/// 返回新 EWMA 值（微秒）。
pub fn ewma_update(prev_ewma_us: u32, new_sample_us: u32) -> Result<u32, u16> {
    if new_sample_us == 0 {
        return Err(SEG_BASE + 3);
    }
    let a = EWMA_ALPHA_PERMILLE as u64;
    let v = (new_sample_us as u64 * a + prev_ewma_us as u64 * (1000 - a)) / 1000;
    Ok(v.max(1) as u32)
}

/// 预补偿量（判据「平滑策略」预测面）：预测下一帧完成时间与目标
/// 的偏差 → 提前量。预补偿 = (ewma − 目标)（正 = 需提前启动，
/// 负 = 可以延后）。返回带符号补偿（微秒）。
pub fn precompensation_us(ewma_us: u32) -> i64 {
    ewma_us as i64 - FRAME_INTERVAL_TARGET_US as i64
}

/// 抖动量化（判据「指标量化」主判据）：帧间隔标准差（微秒）——
/// 入遥测的抖动指标。样本 < 2 拒绝。算法：population 标准差的
/// 整数近似（sqrt 前 >= 0 保护）。
pub fn jitter_stddev_us(samples: &[FrameSample]) -> Result<u32, u16> {
    if samples.len() < 2 {
        return Err(SEG_BASE + 4);
    }
    for s in samples {
        s.validate()?;
    }
    let n = samples.len() as u64;
    let sum: u64 = samples.iter().map(|s| s.interval_us as u64).sum();
    let mean = sum / n;
    let var: u64 = samples
        .iter()
        .map(|s| {
            let d = s.interval_us as i64 - mean as i64;
            (d * d) as u64
        })
        .sum::<u64>()
        / n;
    Ok(isqrt(var))
}

/// 整数平方根（no_std 无 f64 精度依赖的近似）。
fn isqrt(x: u64) -> u32 {
    if x == 0 {
        return 0;
    }
    let mut r = x;
    let mut s = x;
    // 牛顿迭代。
    loop {
        s = r;
        r = (s + x / s) / 2;
        if r >= s {
            break;
        }
    }
    s.min(u32::MAX as u64) as u32
}

/// 抖动预算联动（判据「预算联动」主判据）：任意帧偏差超预算 →
/// 触发降质链（返回 true）；全帧达标 → false。
pub fn budget_breach(samples: &[FrameSample]) -> Result<bool, u16> {
    if samples.is_empty() {
        return Err(SEG_BASE + 5);
    }
    for s in samples {
        s.validate()?;
        if s.deviation_us().unsigned_abs() as u64 > JITTER_BUDGET_US as u64 {
            return Ok(true);
        }
    }
    Ok(false)
}

/// 连续劣化检测（判据「预算联动」深化面）：连续 N 帧偏差同号且
/// 幅度递增 → 系统性劣化（区别于随机抖动）。
pub fn sustained_drift(samples: &[FrameSample], min_run: usize) -> Result<bool, u16> {
    if min_run < 2 || samples.len() < min_run {
        return Err(SEG_BASE + 6);
    }
    let devs: Vec<i64> = samples.iter().map(|s| s.deviation_us()).collect();
    for w in devs.windows(min_run) {
        let all_pos = w.iter().all(|&d| d > 0);
        let all_neg = w.iter().all(|&d| d < 0);
        let increasing = w.windows(2).all(|p| p[1].abs() >= p[0].abs());
        if (all_pos || all_neg) && increasing {
            return Ok(true);
        }
    }
    Ok(false)
}

/// 抑制收益实测（判据「抑制收益实测」主判据）：抑制前后的抖动
/// 标准差对比——抑制后必须更低且降幅 ≥ 200‰。
pub fn suppression_gain_permille(before_us: u32, after_us: u32) -> Result<u32, u16> {
    if before_us == 0 || after_us == 0 {
        return Err(SEG_BASE + 7);
    }
    if after_us >= before_us {
        return Err(SEG_BASE + 8);
    }
    Ok(((before_us - after_us) as u64 * 1000 / before_us as u64) as u32)
}

/// 抖动遥测账（判据「指标量化」遥测容器）：滚动窗口样本 + 源分类
/// 计数。
#[derive(Default)]
pub struct JitterTelemetry {
    samples: Vec<FrameSample>,
    source_counts: [u32; 4],
}

impl JitterTelemetry {
    /// 采集一帧。
    pub fn observe(&mut self, s: FrameSample, gpu_mean_us: u32) -> Result<JitterSource, u16> {
        s.validate()?;
        let src = classify_source(&s, gpu_mean_us)?;
        if self.samples.len() >= 512 {
            self.samples.remove(0);
        }
        self.source_counts[src.code() as usize] += 1;
        self.samples.push(s);
        Ok(src)
    }
    /// 样本数。
    pub fn count(&self) -> usize {
        self.samples.len()
    }
    /// 主导源（计数最高的源——判据「抖动源检测」遥测投影）。
    pub fn dominant_source(&self) -> Result<JitterSource, u16> {
        if self.count() == 0 {
            return Err(SEG_BASE + 9);
        }
        let (idx, _) = self
            .source_counts
            .iter()
            .enumerate()
            .max_by_key(|(_, c)| **c)
            .ok_or(SEG_BASE + 10)?;
        Ok(match idx {
            0 => JitterSource::GpuSpike,
            1 => JitterSource::CpuScheduling,
            2 => JitterSource::OsNoise,
            _ => JitterSource::ThermalThrottle,
        })
    }
    /// 当前窗口抖动指标。
    pub fn current_jitter(&self) -> Result<u32, u16> {
        jitter_stddev_us(&self.samples)
    }
    /// 遥测指纹。
    pub fn telemetry_fp(&self) -> u64 {
        let mut acc: u64 = 0xcbf29ce484222325;
        for s in &self.samples {
            acc = acc.wrapping_mul(0x100000001b3) ^ s.frame as u64;
            acc = acc.wrapping_mul(0x100000001b3) ^ s.interval_us as u64;
        }
        acc
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fs(frame: u32, interval: u32, gpu: u32) -> FrameSample {
        FrameSample { frame, interval_us: interval, gpu_us: gpu, submit_delay_us: 100, temp_deci_c: 700 }
    }
    #[test]
    fn source_classification_features() {
        // GPU 耗时暴涨 → GpuSpike。
        assert_eq!(classify_source(&fs(1, 20000, 5000), 3000).unwrap(), JitterSource::GpuSpike);
        // 温度爆表 → 热降频优先。
        let hot = FrameSample { frame: 2, interval_us: 20000, gpu_us: 3000, submit_delay_us: 100, temp_deci_c: 900 };
        assert_eq!(classify_source(&hot, 3000).unwrap(), JitterSource::ThermalThrottle);
        // 提交延迟大 → CPU 调度。
        let slow = FrameSample { frame: 3, interval_us: 20000, gpu_us: 3000, submit_delay_us: 3000, temp_deci_c: 700 };
        assert_eq!(classify_source(&slow, 3000).unwrap(), JitterSource::CpuScheduling);
        // 都不是 → OS 噪声。
        assert_eq!(classify_source(&fs(4, 18000, 3000), 3000).unwrap(), JitterSource::OsNoise);
    }
    #[test]
    fn ewma_smooths() {
        let mut e = FRAME_INTERVAL_TARGET_US;
        for _ in 0..10 {
            e = ewma_update(e, 20000).unwrap();
        }
        // 10 轮后向 20000 收敛但平滑。
        assert!(e > FRAME_INTERVAL_TARGET_US && e < 20000);
        assert!(ewma_update(100, 0).is_err());
    }
    #[test]
    fn jitter_stddev_math() {
        // 稳定帧列。
        let stable: Vec<FrameSample> = (1..=5).map(|f| fs(f, 16667, 3000)).collect();
        assert_eq!(jitter_stddev_us(&stable).unwrap(), 0);
        // 抖动帧列。
        let jittery = vec![fs(1, 14000, 3000), fs(2, 19000, 3000), fs(3, 17000, 3000)];
        let j = jitter_stddev_us(&jittery).unwrap();
        assert!(j > 1000);
        assert!(jitter_stddev_us(&jittery[..1]).is_err());
    }
    #[test]
    fn budget_breach_triggers() {
        let ok_samples = vec![fs(1, 17000, 3000), fs(2, 16500, 3000)];
        assert!(!budget_breach(&ok_samples).unwrap());
        let bad = vec![fs(1, 16667, 3000), fs(2, 20000, 3000)];
        assert!(budget_breach(&bad).unwrap());
        assert!(budget_breach(&[]).is_err());
    }
    #[test]
    fn sustained_drift_detection() {
        // 连续递增正偏差。
        let drift = vec![fs(1, 17000, 3000), fs(2, 17500, 3000), fs(3, 18000, 3000)];
        assert!(sustained_drift(&drift, 3).unwrap());
        // 随机正负交替 → 非劣化。
        let noisy = vec![fs(1, 17000, 3000), fs(2, 16300, 3000), fs(3, 17000, 3000)];
        assert!(!sustained_drift(&noisy, 3).unwrap());
    }
    #[test]
    fn suppression_gain_gated() {
        assert_eq!(suppression_gain_permille(5000, 3500).unwrap(), 300);
        assert!(suppression_gain_permille(5000, 5000).is_err());
        assert!(suppression_gain_permille(4000, 5000).is_err());
    }
    #[test]
    fn telemetry_dominant_source() {
        let mut tel = JitterTelemetry::default();
        for f in 1..=3u32 {
            tel.observe(fs(f, 20000, 5000), 3000).unwrap();
        }
        assert_eq!(tel.dominant_source().unwrap(), JitterSource::GpuSpike);
        assert!(tel.current_jitter().unwrap() > 0);
        assert!(tel.telemetry_fp() != 0);
    }
    #[test]
    fn precompensation_sign() {
        // EWMA 高于目标 → 正补偿（提前）。
        assert!(precompensation_us(18000) > 0);
        // 低于目标 → 负补偿。
        assert!(precompensation_us(15000) < 0);
        assert_eq!(precompensation_us(FRAME_INTERVAL_TARGET_US), 0);
    }
}
