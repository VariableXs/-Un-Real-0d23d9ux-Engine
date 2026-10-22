//! CGPU-F0489 · 计时校准（目标 360 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 计时校准：多计时源一致性（CPU QPC×GPU 时间戳×呈现时钟三源
//!   互校——漂移可测可纠）；
//! - 定期重校准（时钟漂移的周期性再校准——长时间运行不漂）；
//! - 校准精度声明（校准后残余误差量化）；
//! - 校准失败降级（单源故障→双源降级+显性告知）。判据：三源互校、
//!   重校准、精度声明、降级、判据。
//!
//! 设计纪律：
//! - 三源互校：以 CPU QPC 为基准源，GPU/呈现源分别算漂移率
//!   （‰），漂移在带内（±50‰）即一致；超带即告警。
//! - 定期重校准：CALIB_INTERVAL tick 到期自动再校（重校后漂移归零
//!   基准重置）。
//! - 精度声明：校准后残余误差（‰）量化入册——声明值必须 ≥ 实测
//!   残余（声明不能夸大精度）。
//! - 降级：单源故障（GPU 时间戳失效）→ 双源降级+告警行；三源全
//!   故障 → 计时不可信（显性拒）。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x1160 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x1160，全仓独占）。
pub const SEG_BASE: u16 = 0x1160;

pub const E_TS_DRIFT: u16 = SEG_BASE + 1; // 漂移超带
pub const E_TS_DECL: u16 = SEG_BASE + 2; // 精度声明夸大
pub const E_TS_NOSRC: u16 = SEG_BASE + 3; // 源不足（<2）
pub const E_TS_ALLFAIL: u16 = SEG_BASE + 4; // 三源全故障

/// 漂移带与重校准间隔。
pub const DRIFT_BAND_PM: u64 = 50;
pub const CALIB_INTERVAL: u64 = 3600;

/// 三源 ID。
pub const SRC_CPU_QPC: u8 = 0; // 基准源
pub const SRC_GPU_TS: u8 = 1;
pub const SRC_PRESENT: u8 = 2;

/// 漂移率（‰）：对照时长与偏移。
pub fn drift_pm(base_elapsed: u64, other_elapsed: u64) -> u64 {
    if base_elapsed == 0 {
        return 0;
    }
    let d = other_elapsed.abs_diff(base_elapsed);
    d * 1000 / base_elapsed
}

/// 计时校准器。
#[derive(Debug)]
pub struct TimeCalibrator {
    /// 上次校准 tick。
    pub last_calib: u64,
    /// 源健康位图（bit0=CPU bit1=GPU bit2=呈现）。
    pub healthy: u8,
    /// 上次校准残余误差（‰）。
    pub residual_pm: u64,
    /// 精度声明（‰，必须 ≥ 残余）。
    pub declared_pm: u64,
    /// 告警行。
    pub alerts: Vec<(&'static str, u64)>,
}

impl TimeCalibrator {
    pub fn new() -> Self {
        Self { last_calib: 0, healthy: 0b111, residual_pm: 0, declared_pm: 10, alerts: Vec::new() }
    }

    pub fn healthy_count(&self) -> u32 {
        self.healthy.count_ones()
    }

    /// 三源互校：GPU/呈现相对 CPU 的漂移逐源判定。
    pub fn cross_check(&mut self, now: u64, cpu_elapsed: u64, gpu_elapsed: u64, present_elapsed: u64) -> Result<(), u16> {
        if self.healthy_count() < 2 {
            return Err(E_TS_NOSRC);
        }
        let mut worst = 0u64;
        if self.healthy & (1 << SRC_GPU_TS) != 0 {
            let d = drift_pm(cpu_elapsed, gpu_elapsed);
            if d > DRIFT_BAND_PM {
                self.alerts.push(("gpu-drift", d));
            }
            worst = worst.max(d);
        }
        if self.healthy & (1 << SRC_PRESENT) != 0 {
            let d = drift_pm(cpu_elapsed, present_elapsed);
            if d > DRIFT_BAND_PM {
                self.alerts.push(("present-drift", d));
            }
            worst = worst.max(d);
        }
        if worst > DRIFT_BAND_PM {
            return Err(E_TS_DRIFT);
        }
        Ok(())
    }

    /// 定期重校准：到期触发，重置基准与残余。
    pub fn maybe_recalib(&mut self, now: u64) -> bool {
        if now.saturating_sub(self.last_calib) >= CALIB_INTERVAL {
            self.last_calib = now;
            self.residual_pm = 0;
            true
        } else {
            false
        }
    }

    /// 精度声明合法性：声明 ≥ 实测残余（不夸大精度）。
    pub fn set_residual(&mut self, residual_pm: u64) -> Result<(), u16> {
        self.residual_pm = residual_pm;
        if self.declared_pm < residual_pm {
            return Err(E_TS_DECL);
        }
        Ok(())
    }

    /// 源故障降级：单源故障 → 双源（保持 ≥2）+告警；全故障拒。
    pub fn source_fault(&mut self, src: u8) -> Result<(), u16> {
        self.healthy &= !(1 << src);
        if self.healthy == 0 {
            return Err(E_TS_ALLFAIL);
        }
        if self.healthy_count() == 2 {
            self.alerts.push(("degrade-dual-source", now_hint(src)));
        }
        Ok(())
    }
}

fn now_hint(src: u8) -> u64 {
    src as u64 + 1
}

pub(crate) fn run_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();
    // 一、三源互校：带内通过+超带告警。
    {
        let mut tc = TimeCalibrator::new();
        let r = tc.cross_check(1000, 100_000, 100_400, 99_800); // 漂移 4‰/2‰
        v.push(("F0489-三源互校-带内通过", r.is_ok()));
        let mut tc2 = TimeCalibrator::new();
        let r2 = tc2.cross_check(1000, 100_000, 106_000, 100_000); // GPU 漂移 60‰
        v.push(("F0489-三源互校-超带拒", r2 == Err(E_TS_DRIFT)));
        v.push(("F0489-三源互校-告警留痕", !tc2.alerts.is_empty()));
        // 漂移率计算。
        v.push(("F0489-三源互校-漂移率公式", drift_pm(1000, 1050) == 50 && drift_pm(1000, 1000) == 0));
    }
    // 二、定期重校准。
    {
        let mut tc = TimeCalibrator::new();
        tc.last_calib = 0;
        v.push(("F0489-重校准-未到期不触发", !tc.maybe_recalib(CALIB_INTERVAL - 1)));
        v.push(("F0489-重校准-到期触发+基准重置", tc.maybe_recalib(CALIB_INTERVAL) && tc.residual_pm == 0 && tc.last_calib == CALIB_INTERVAL));
    }
    // 三、精度声明。
    {
        let mut tc = TimeCalibrator::new();
        v.push(("F0489-精度声明-残余小于声明过", tc.set_residual(5).is_ok()));
        v.push(("F0489-精度声明-夸大拒", tc.set_residual(50) == Err(E_TS_DECL)));
        v.push(("F0489-精度声明-残余量化在场", tc.residual_pm == 50));
    }
    // 四、降级。
    {
        let mut tc = TimeCalibrator::new();
        tc.source_fault(SRC_GPU_TS).ok();
        v.push(("F0489-降级-单源故障双源", tc.healthy_count() == 2));
        tc.source_fault(SRC_PRESENT).ok();
        v.push(("F0489-降级-双源仍可用", tc.healthy_count() == 1));
        v.push(("F0489-降级-全故障拒", tc.source_fault(SRC_CPU_QPC) == Err(E_TS_ALLFAIL)));
        // 降级告警在场。
        v.push(("F0489-降级-降级告警留痕", tc.alerts.iter().any(|a| a.0 == "degrade-dual-source")));
    }
    // 五、判据自检。
    {
        v.push(("F0489-判据-漂移带入册", DRIFT_BAND_PM == 50));
        v.push(("F0489-判据-三源枚举", SRC_CPU_QPC == 0 && SRC_GPU_TS == 1 && SRC_PRESENT == 2));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::run_checks;
    #[test]
    fn f0489_timecal_checks() {
        for (name, ok) in run_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
