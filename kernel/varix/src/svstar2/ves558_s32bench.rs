//! VE-F4324 · 一致性基准套件（U07 性能与预算组）
//! 职责：一致性性能基准套件——基准集（小/中/大/超大四规模工程样本 + 极端
//! 形态样本）+ 测量协议三定式（预热 / 多轮 / 去离群）+ 基准结果与预算对表
//! 发布（对表报告随发版）+ 样本损坏哈希校验重取 + 离群未去协议断言拦截
//! + 结果漂移环境指纹比对归因。
//! 段位：0x3420（续表 idx558）。行数预算 360。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x3420;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BnCode {
    /// 样本损坏（哈希校验失败重取）。
    SampleCorrupt = SEG_BASE,
    /// 离群未去（协议断言拦截）。
    OutlierLeft = SEG_BASE + 0x01,
    /// 预热缺失（协议定式一）。
    NoWarmup = SEG_BASE + 0x02,
    /// 轮数不足（协议定式二）。
    TooFewRounds = SEG_BASE + 0x03,
    /// 环境漂移（指纹不符）。
    EnvDrift = SEG_BASE + 0x04,
}

/// 四规模 + 极端形态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Scale {
    Small,
    Medium,
    Large,
    XLarge,
    Extreme,
}

/// 基准样本。
#[derive(Clone, Debug, PartialEq)]
pub struct Sample {
    pub scale: Scale,
    pub files: u32,
    /// 内容哈希（损坏检测）。
    pub hash: u32,
}

/// 一轮测量结果（ms）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Round {
    pub ms: u32,
}

/// 基准台。
#[derive(Default)]
pub struct BenchDesk {
    pub samples: Vec<Sample>,
    /// 环境指纹（测量环境单源）。
    pub env_fp: u32,
    pub rounds: Vec<Round>,
    pub warmed: bool,
    pub min_rounds: u32,
    /// 对表报告：(规模, 均值 ms)。
    pub report: Vec<(Scale, u32)>,
    pub intercepts: u32,
}

impl BenchDesk {
    pub fn new(env_fp: u32, min_rounds: u32) -> Self {
        BenchDesk { env_fp, min_rounds: min_rounds.max(3), ..Default::default() }
    }

    /// 样本登记（哈希自洽校验：hash 与 FNV(内容) 不符 = 损坏拒收）。
    pub fn add_sample(&mut self, scale: Scale, files: u32, hash: u32) -> Result<(), BnCode> {
        if hash != fnv1a(files) {
            return Err(BnCode::SampleCorrupt);
        }
        if let Some(s) = self.samples.iter_mut().find(|s| s.scale == scale) {
            s.files = files;
            s.hash = hash;
        } else {
            self.samples.push(Sample { scale, files, hash });
        }
        Ok(())
    }

    /// 预热定式。
    pub fn warmup(&mut self) {
        self.warmed = true;
    }

    /// 多轮采集：未预热 / 轮数不足拒绝。
    pub fn measure(&mut self, rounds: &[u32]) -> Result<(), BnCode> {
        if !self.warmed {
            return Err(BnCode::NoWarmup);
        }
        if rounds.len() < self.min_rounds as usize {
            return Err(BnCode::TooFewRounds);
        }
        self.rounds = rounds.iter().map(|m| Round { ms: *m }).collect();
        Ok(())
    }

    /// 去离群：四分位法（< Q1 − 1.5·IQR 或 > Q3 + 1.5·IQR 剔除）。
    /// 返回 (去离群后均值, 剔除数)。
    pub fn trim_mean(&mut self) -> Result<(u32, usize), BnCode> {
        if self.rounds.is_empty() {
            return Err(BnCode::OutlierLeft);
        }
        let mut vals: Vec<u32> = self.rounds.iter().map(|r| r.ms).collect();
        vals.sort_unstable();
        let n = vals.len();
        let q1 = vals[n / 4];
        let q3 = vals[n * 3 / 4];
        let iqr = q3.saturating_sub(q1);
        let lo = q1.saturating_sub(iqr / 2 * 3);
        let hi = q3.saturating_add(iqr / 2 * 3);
        let kept: Vec<u32> = vals.iter().copied().filter(|v| *v >= lo && *v <= hi).collect();
        let removed = n - kept.len();
        self.rounds = kept.iter().map(|ms| Round { ms: *ms }).collect();
        if kept.is_empty() {
            return Err(BnCode::OutlierLeft);
        }
        let sum: u64 = kept.iter().map(|v| *v as u64).sum();
        Ok(((sum / kept.len() as u64) as u32, removed))
    }

    /// 对表报告发布（随发版）：基准均值入报告。
    pub fn publish(&mut self, scale: Scale, mean_ms: u32) {
        if let Some(r) = self.report.iter_mut().find(|r| r.0 == scale) {
            r.1 = mean_ms;
        } else {
            self.report.push((scale, mean_ms));
        }
    }

    /// 结果漂移归因：环境指纹不符 = 环境漂移（结果不可比——拒绝直接对表）。
    pub fn compare_env(&self, now_fp: u32) -> Result<(), BnCode> {
        if now_fp != self.env_fp {
            return Err(BnCode::EnvDrift);
        }
        Ok(())
    }
}

fn fnv1a(mut x: u32) -> u32 {
    let mut h: u32 = 0x811C9DC5;
    for _ in 0..4 {
        h ^= x & 0xFF;
        h = h.wrapping_mul(0x01000193);
        x >>= 8;
    }
    h
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves558_checks() -> CheckSet {
    let mut set = CheckSet::new("ves558");
    let mut bd = BenchDesk::new(0xABCD, 3);

    // C558-01 四规模 + 极端样本登记（哈希自洽）。
    let scales = [Scale::Small, Scale::Medium, Scale::Large, Scale::XLarge, Scale::Extreme];
    let files = [10, 100, 1000, 10000, 100000];
    let mut ok = true;
    for (s, f) in scales.iter().zip(files.iter()) {
        if bd.add_sample(*s, *f, fnv1a(*f)).is_err() {
            ok = false;
        }
    }
    chk(&mut set, "C558-01", ok && bd.samples.len() == 5, "五样本哈希自洽");

    // C558-02 样本损坏哈希拒收。
    chk(&mut set, "C558-02", bd.add_sample(Scale::Small, 10, 0xDEAD) == Err(BnCode::SampleCorrupt), "损坏哈希拒收");

    // C558-03 预热缺失拒绝测量（定式一）。
    chk(&mut set, "C558-03", bd.measure(&[100, 100, 100]) == Err(BnCode::NoWarmup), "预热缺失拒");

    // C558-04 轮数不足拒绝（定式二）。
    bd.warmup();
    chk(&mut set, "C558-04", bd.measure(&[100, 100]) == Err(BnCode::TooFewRounds), "轮数不足拒");

    // C558-05 多轮采集通过（定式三采集）。
    chk(&mut set, "C558-05", bd.measure(&[100, 102, 98, 101]).is_ok() && bd.rounds.len() == 4, "多轮采集通过");

    // C558-06 去离群（注入极端值后剔除并重均值）。
    let mut vals = [100, 102, 98, 101, 9000];
    let _ = bd.measure(&vals);
    let (mean, removed) = bd.trim_mean().unwrap_or((0, 0));
    chk(&mut set, "C558-06", removed >= 1 && mean <= 110 && mean >= 90, "离群剔除重均值");

    // C558-07 无离群零剔除（全等值样本——IQR=0 时等值不被误剔）。
    let _ = bd.measure(&[100, 100, 100, 100]);
    let (mean2, removed2) = bd.trim_mean().unwrap_or((0, 0));
    chk(&mut set, "C558-07", removed2 == 0 && mean2 == 100, "无离群零剔除");

    // C558-08 对表报告发布。
    bd.publish(Scale::Small, mean2);
    chk(&mut set, "C558-08", bd.report == [(Scale::Small, 100)], "对表报告发布");

    // C558-09 环境指纹一致可比。
    chk(&mut set, "C558-09", bd.compare_env(0xABCD).is_ok(), "同环境可比");

    // C558-10 环境漂移拒绝直接对表。
    chk(&mut set, "C558-10", bd.compare_env(0x1111) == Err(BnCode::EnvDrift), "环境漂移拒");

    // C558-11 样本覆盖更新幂等。
    let _ = bd.add_sample(Scale::Small, 20, fnv1a(20));
    chk(&mut set, "C558-11", bd.samples.len() == 5 && bd.samples[0].files == 20, "样本覆盖幂等");

    // C558-12 最小轮数下限钳制（<3 钳 3）。
    let bd2 = BenchDesk::new(1, 1);
    chk(&mut set, "C558-12", bd2.min_rounds == 3, "轮数下限钳制");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_selfcheck() {
        let mut bd = BenchDesk::new(1, 3);
        assert!(bd.add_sample(Scale::Small, 50, fnv1a(50)).is_ok());
        assert_eq!(bd.add_sample(Scale::Small, 50, 1), Err(BnCode::SampleCorrupt));
    }

    #[test]
    fn protocol_gate() {
        let mut bd = BenchDesk::new(1, 3);
        assert_eq!(bd.measure(&[1, 1, 1]), Err(BnCode::NoWarmup));
        bd.warmup();
        assert_eq!(bd.measure(&[1, 1]), Err(BnCode::TooFewRounds));
        assert!(bd.measure(&[1, 1, 1]).is_ok());
    }

    #[test]
    fn trim_outliers() {
        let mut bd = BenchDesk::new(1, 3);
        bd.warmup();
        let _ = bd.measure(&[10, 11, 12, 1000]);
        let (mean, removed) = bd.trim_mean().unwrap_or((0, 0));
        assert_eq!(removed, 1);
        assert!(mean <= 12);
    }
}
