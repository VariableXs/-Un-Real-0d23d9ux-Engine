//! VE-F4269 · 一致性度量趋势（U04 治理组）
//! 职责：指标族四类（违例密度/修复时长/复发率/覆盖度）+ 趋势三粒度
//! （周/月/季）+ 断点插值显式标记不伪造连续 + 聚合冲突以细粒度重算为准。
//! 段位：0x2760（续表 idx503）。行数预算 300。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x2760;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TrCode {
    /// 断点未标记（伪造连续）。
    FakeContinuity = SEG_BASE + 0x01,
    /// 粒度聚合冲突。
    GrainConflict = SEG_BASE + 0x02,
    /// 趋势异常未联动预警。
    AnomalySilent = SEG_BASE + 0x03,
}

/// 指标族四类。
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Metric {
    Density = 0,
    FixTime = 1,
    Recur = 2,
    Cover = 3,
}
pub const METRIC_N: usize = 4;

/// 粒度。
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
pub enum Grain {
    Week = 0,
    Month = 1,
    Quarter = 2,
}

/// 时序点：(时点, 值, 插值标记)。
#[derive(Clone, Copy, Debug)]
pub struct TsPoint {
    pub t: u32,
    pub val: u32,
    pub interpolated: bool,
}

/// 趋势库。
pub struct TrendBook {
    /// 各指标时序。
    pub series: [Vec<TsPoint>; METRIC_N],
    /// 越限异常计数（联动 F4270 预警）。
    pub anomalies: u32,
}

impl TrendBook {
    pub fn new() -> Self {
        TrendBook { series: [Vec::new(), Vec::new(), Vec::new(), Vec::new()], anomalies: 0 }
    }

    /// 写入（O(1) 追加，须时点递增）。
    pub fn push(&mut self, m: Metric, t: u32, val: u32) -> Result<(), TrCode> {
        let s = &mut self.series[m as usize];
        if let Some(last) = s.last() {
            if t <= last.t {
                return Err(TrCode::GrainConflict);
            }
        }
        s.push(TsPoint { t, val, interpolated: false });
        Ok(())
    }

    /// 断点插值：t 缺口处补线性插值点并显式标记。
    pub fn fill_gaps(&mut self, m: Metric, step: u32) -> usize {
        let s = &mut self.series[m as usize];
        let mut out = Vec::new();
        let mut filled = 0;
        for i in 0..s.len() {
            if i > 0 {
                let prev = s[i - 1];
                let cur = s[i];
                let mut t = prev.t + step;
                while t < cur.t {
                    // 线性插值（整数运算，no_std 无 f64::round）
                    let span = cur.t - prev.t;
                    let dv = cur.val as i64 - prev.val as i64;
                    let v = prev.val as i64 + dv * (t - prev.t) as i64 / span as i64;
                    out.push(TsPoint { t, val: v.max(0) as u32, interpolated: true });
                    filled += 1;
                    t += step;
                }
            }
            out.push(s[i]);
        }
        *s = out;
        filled
    }

    /// 插值完整性：无相邻未标记断点。
    pub fn no_fake_continuity(&self, m: Metric, step: u32) -> bool {
        let s = &self.series[m as usize];
        for w in s.windows(2) {
            if w[1].t - w[0].t > step && !w[1].interpolated && !w[0].interpolated {
                // 中间应有插值点
                return false;
            }
        }
        true
    }

    /// 粒度聚合：细→粗求均值；与粗粒度已存值冲突→以细粒度重算为准。
    pub fn aggregate(&self, m: Metric, from: usize, to: usize) -> Option<u32> {
        let s = &self.series[m as usize];
        if from >= to || to > s.len() {
            return None;
        }
        let sum: u64 = s[from..to].iter().map(|p| p.val as u64).sum();
        Some((sum / (to - from) as u64) as u32)
    }

    /// 异常检：越限计数+联动标记。
    pub fn anomaly_check(&mut self, m: Metric, cap: u32) -> bool {
        let over = self.series[m as usize].iter().any(|p| p.val > cap);
        if over {
            self.anomalies += 1;
        }
        over
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

/// 判据组：C503-01..12。
pub fn run_ves503_checks() -> CheckSet {
    let mut set = CheckSet::new("ves503");

    let mut tb = TrendBook::new();
    // 乱序拒收
    let _ = tb.push(Metric::Density, 10, 100);
    chk(&mut set, "C503-01", tb.push(Metric::Density, 10, 90) == Err(TrCode::GrainConflict), "时点重复拒收");
    chk(&mut set, "C503-02", tb.push(Metric::Density, 5, 90) == Err(TrCode::GrainConflict), "时点回退拒收");

    // 断点插值
    let _ = tb.push(Metric::Density, 30, 200); // 20 缺口
    chk(&mut set, "C503-03", tb.no_fake_continuity(Metric::Density, 10) == false, "缺口未补=伪造连续风险");
    let n = tb.fill_gaps(Metric::Density, 10);
    chk(&mut set, "C503-04", n == 1, "插值补 1 点（t=20）");
    chk(&mut set, "C503-05", tb.no_fake_continuity(Metric::Density, 10), "补后连续性成立");
    chk(&mut set, "C503-06", tb.series[0][1].interpolated && tb.series[0][1].val == 150, "插值点显式标记且线性值 150");

    // 粒度聚合
    chk(&mut set, "C503-07", tb.aggregate(Metric::Density, 0, 3) == Some(150), "细→粗聚合均值 150");
    chk(&mut set, "C503-08", tb.aggregate(Metric::Density, 2, 2) == None && tb.aggregate(Metric::Density, 0, 9) == None, "越界/空窗聚合拒绝");

    // 四族指标独立
    for m in [Metric::FixTime, Metric::Recur, Metric::Cover] {
        let _ = tb.push(m, 1, 50);
    }
    chk(&mut set, "C503-09", tb.series.iter().enumerate().all(|(i, s)| if i == 0 { s.len() == 3 } else { s.len() == 1 }), "四族时序独立");

    // 异常联动
    chk(&mut set, "C503-10", tb.anomaly_check(Metric::Density, 180), "越限异常检出");
    chk(&mut set, "C503-11", tb.anomalies == 1, "预警联动计数");
    chk(&mut set, "C503-12", !tb.anomaly_check(Metric::Cover, 100) && tb.anomalies == 1, "未越限不误报");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trend_interpolate() {
        let mut tb = TrendBook::new();
        let _ = tb.push(Metric::Cover, 1, 10);
        let _ = tb.push(Metric::Cover, 3, 20);
        assert_eq!(tb.fill_gaps(Metric::Cover, 1), 1);
        assert!(tb.series[3][1].interpolated && tb.series[3][1].val == 15);
    }
}
