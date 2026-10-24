//! CGPU-F0421 · 漂移检测（目标 360 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 漂移检测：性能回归早期预警（同负载消耗趋势持续上升=漂移——统计
//!   检出）；漂移归因（漂移定位到账户/资源维度）；误报控制（负载自然
//!   增长与真漂移的区分——归一化对比）；漂移报告（进 C 域告警体系
//!   F0391）。
//! - 判据：检出、归因、误报控制、报告、判据。
//!
//! 设计纪律：
//! - 检出：同负载档（轻/中/重）每窗消耗均值，连续 K 窗同向上升且累计
//!   升幅超带 ⇒ 漂移信号（单窗波动不报——持续性的形式化）。
//! - 误报控制：归一化对比——消耗除以产出（单位产出消耗）再判趋势；
//!   负载自然增长时分子分母同涨、归一化值平稳 ⇒ 不报。检出只认归一化
//!   曲线。
//! - 归因：对漂移账户按资源维分解，找贡献最大维（主嫌），归因链
//!   （账户 → 资源 → 责任窗）三段齐才出报告。
//! - 报告：进 C 域告警体系（F0391 规则语义——severity/摘要/证据链）。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0D80 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x0D80，全仓独占）。
pub const SEG_BASE: u16 = 0x0D80;

pub const E_DRIFT_NONE: u16 = SEG_BASE + 1; // 无漂移却出报告
pub const E_DRIFT_TRIB: u16 = SEG_BASE + 2; // 归因链缺段
pub const E_DRIFT_NORM: u16 = SEG_BASE + 3; // 归一化分母为零
pub const E_DRIFT_BAND: u16 = SEG_BASE + 4; // 检出带非法
pub const E_DRIFT_SEV: u16 = SEG_BASE + 5; // severity 越级
pub const E_DRIFT_ACCT: u16 = SEG_BASE + 6; // 账户未注册

/// 连续同向窗数阈值。
pub const DRIFT_STREAK: usize = 3;
/// 检出带（归一化值相对基线累计升幅，千分位）。
pub const DRIFT_BAND_PM: u32 = 150;

/// 资源维（归因坐标，与 F0418 同源）。
pub const DR_RES_CPU: u8 = 0;
pub const DR_RES_GPU: u8 = 1;
pub const DR_RES_BW: u8 = 2;
pub const DR_RES_MEM: u8 = 3;
pub const DR_RES_COUNT: usize = 4;

/// 负载档（归一化对比的同负载前提——跨档不可比）。
pub const DR_LOAD_LIGHT: u8 = 0;
pub const DR_LOAD_MID: u8 = 1;
pub const DR_LOAD_HEAVY: u8 = 2;

/// 一窗观测：负载档、产出量（分母）、四资源消耗（分子）。
#[derive(Clone, Copy, Debug)]
pub struct DriftSample {
    pub load: u8,
    /// 产出（帧数/请求数——归一化分母）。
    pub output: u64,
    pub used: [u64; DR_RES_COUNT],
}

/// 账户的漂移追踪器。
#[derive(Clone, Debug)]
pub struct DriftTracker {
    pub acct: u32,
    /// 基线归一化值（单位产出消耗，千分位精度 ×1000 保存）。
    pub baseline: Option<u64>,
    /// 各资源维归一化序列（×1000）。
    pub series: Vec<[u64; DR_RES_COUNT]>,
    /// 连续上升窗计数。
    pub up_streak: usize,
    /// 漂移已报标志（一段只报一次）。
    pub reported: bool,
    /// 累计升幅（‰，相对基线）。
    pub cum_rise_pm: i64,
}

impl DriftTracker {
    pub fn new(acct: u32) -> Self {
        DriftTracker {
            acct,
            baseline: None,
            series: Vec::new(),
            up_streak: 0,
            reported: false,
            cum_rise_pm: 0,
        }
    }

    /// 单资源维归一化（×1000 保精度；产出为 0 → 错误）。
    pub fn normalize(used: u64, output: u64) -> Result<u64, u16> {
        if output == 0 {
            return Err(E_DRIFT_NORM);
        }
        Ok(used * 1000 / output)
    }

    /// 灌入一窗：归一化 → 趋势判定（同负载档才可比）。
    /// 返回（是否检出漂移, 本窗归一化合计）。
    pub fn observe(&mut self, load: u8, s: &DriftSample) -> Result<(bool, u64), u16> {
        if load > DR_LOAD_HEAVY {
            return Err(E_DRIFT_BAND);
        }
        if s.load != load {
            return Err(E_DRIFT_BAND); // 跨档样本不可比
        }
        let mut norm = [0u64; DR_RES_COUNT];
        let mut sum = 0u64;
        for i in 0..DR_RES_COUNT {
            norm[i] = Self::normalize(s.used[i], s.output)?;
            sum += norm[i];
        }
        self.series.push(norm);
        // 基线未立 → 首窗立基线（不判趋势）。
        let base = match self.baseline {
            None => {
                self.baseline = Some(sum);
                return Ok((false, sum));
            }
            Some(b) => b,
        };
        // 同向上升判定：与上一窗比上升。
        let rising = match self.series.len() {
            n if n >= 2 => {
                let prev = self.series[n - 2].iter().sum::<u64>();
                sum > prev
            }
            _ => false,
        };
        if rising {
            self.up_streak += 1;
        } else {
            self.up_streak = 0;
            self.reported = false; // 趋势中断可重报
        }
        self.cum_rise_pm = if base == 0 {
            0
        } else {
            (sum as i64 - base as i64) * 1000 / base as i64
        };
        // 检出：连续 K 窗同向上升 + 累计升幅越带。一段只报一次
        // （首窗报警，其后持续漂移保持静默——趋势中断清锁可重报）。
        let fire = self.up_streak >= DRIFT_STREAK
            && self.cum_rise_pm >= DRIFT_BAND_PM as i64
            && !self.reported;
        if fire {
            self.reported = true;
        }
        Ok((fire, sum))
    }
}

/// 漂移归因：四资源维贡献分解 → 主嫌维。
#[derive(Clone, Debug)]
pub struct DriftAttrib {
    pub acct: u32,
    /// 各维升幅（千分位，相对基线）。
    pub rise_pm: [i64; DR_RES_COUNT],
    /// 责任窗（首窗号与末窗号）。
    pub first_win: u64,
    pub last_win: u64,
}

impl DriftAttrib {
    /// 归因链三段齐（账户 + 资源升幅 + 责任窗）才出报告。
    pub fn complete(&self) -> Result<(), u16> {
        if self.acct == 0 {
            return Err(E_DRIFT_TRIB);
        }
        if self.rise_pm.iter().all(|r| *r == 0) {
            return Err(E_DRIFT_NONE);
        }
        if self.last_win < self.first_win {
            return Err(E_DRIFT_TRIB);
        }
        Ok(())
    }

    /// 主嫌维：升幅最大的资源。
    pub fn prime(&self) -> u8 {
        let mut best = 0usize;
        for i in 1..DR_RES_COUNT {
            if self.rise_pm[i] > self.rise_pm[best] {
                best = i;
            }
        }
        best as u8
    }
}

/// 漂移报告（进 C 域告警体系——F0391 规则语义）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DriftReport {
    pub acct: u32,
    /// severity：1=提醒 2=警告 3=严重（升幅越大越严重）。
    pub severity: u8,
    /// 主嫌资源维。
    pub prime_res: u8,
    /// 累计升幅（‰）。
    pub rise_pm: i64,
    /// 证据链摘要（首末窗）。
    pub first_win: u64,
    pub last_win: u64,
}

/// 从归因生成报告（severity 分级 + 归因链完整校验）。
pub fn make_report(a: &DriftAttrib) -> Result<DriftReport, u16> {
    a.complete()?;
    let rise = a.rise_pm[a.prime() as usize];
    let severity: u8 = if rise >= 500 {
        3
    } else if rise >= 300 {
        2
    } else {
        1
    };
    if severity > 3 {
        return Err(E_DRIFT_SEV);
    }
    Ok(DriftReport {
        acct: a.acct,
        severity,
        prime_res: a.prime(),
        rise_pm: rise,
        first_win: a.first_win,
        last_win: a.last_win,
    })
}

/// 归一化误报对照：负载增长（分子分母同涨）平稳不报。
pub fn load_growth_not_drift(used0: u64, out0: u64, used1: u64, out1: u64) -> Result<bool, u16> {
    let n0 = DriftTracker::normalize(used0, out0)?;
    let n1 = DriftTracker::normalize(used1, out1)?;
    // 归一化平稳（±50‰ 内）⇒ 自然增长不是漂移。
    Ok((n1 as i64 - n0 as i64).abs() * 1000 <= 50 * n0 as i64)
}

// ==========================================================================
// 判据测试
// ==========================================================================
#[cfg(test)]
mod tests {
    use super::*;

    fn tracker() -> DriftTracker {
        DriftTracker::new(7)
    }

    fn sample(used: u64, output: u64) -> DriftSample {
        DriftSample { load: DR_LOAD_MID, output, used: [used, 0, 0, 0] }
    }

    #[test]
    fn a01_检出_连续三窗同向上升越带() {
        let mut t = tracker();
        // 基线窗：产出 100 用 100 → 归一 1000。
        assert_eq!(t.observe(DR_LOAD_MID, &sample(100, 100)), Ok((false, 1000)));
        // 升 1：1050（+50‰ 未越带）。
        assert_eq!(t.observe(DR_LOAD_MID, &sample(105, 100)), Ok((false, 1050)));
        // 升 2：1100（连续 2 窗，累计 +100‰ < 150‰ 未越带）。
        assert_eq!(t.observe(DR_LOAD_MID, &sample(110, 100)), Ok((false, 1100)));
        // 升 3：1200（连续 3 窗 + 累计 +200‰ ≥ 150‰ ⇒ 检出）。
        assert_eq!(t.observe(DR_LOAD_MID, &sample(120, 100)), Ok((true, 1200)));
        // 一段只报一次。
        assert_eq!(t.observe(DR_LOAD_MID, &sample(125, 100)), Ok((false, 1250)));
        // 趋势中断重置。
        let mut t2 = tracker();
        t2.observe(DR_LOAD_MID, &sample(100, 100)).ok();
        t2.observe(DR_LOAD_MID, &sample(110, 100)).ok();
        t2.observe(DR_LOAD_MID, &sample(90, 100)).ok(); // 下降中断
        assert_eq!(t2.up_streak, 0);
    }

    #[test]
    fn a02_误报控制_归一化滤掉自然增长() {
        // 负载翻倍：消耗 100→200、产出 100→200 ⇒ 归一值不变。
        assert_eq!(load_growth_not_drift(100, 100, 200, 200), Ok(true));
        // 真漂移：产出不变消耗涨 20% ⇒ 归一值涨 ⇒ 非平稳。
        assert_eq!(load_growth_not_drift(100, 100, 120, 100), Ok(false));
        // 归一化分母为零拒。
        assert_eq!(DriftTracker::normalize(50, 0), Err(E_DRIFT_NORM));
        // 跨档样本不可比。
        let mut t = tracker();
        t.observe(DR_LOAD_MID, &sample(100, 100)).ok();
        let heavy = DriftSample { load: DR_LOAD_HEAVY, output: 100, used: [110, 0, 0, 0] };
        assert_eq!(t.observe(DR_LOAD_MID, &heavy), Err(E_DRIFT_BAND));
    }

    #[test]
    fn a03_归因_主嫌维与三段链() {
        let a = DriftAttrib {
            acct: 7,
            rise_pm: [50, 400, 0, 0],
            first_win: 10,
            last_win: 13,
        };
        assert_eq!(a.complete(), Ok(()));
        assert_eq!(a.prime(), DR_RES_GPU); // GPU 升幅最大
        // 链缺段：全零升幅 → 无漂移。
        let zero = DriftAttrib { acct: 7, rise_pm: [0; 4], first_win: 1, last_win: 2 };
        assert_eq!(zero.complete(), Err(E_DRIFT_NONE));
        // 账户 0 保留。
        let a0 = DriftAttrib { acct: 0, rise_pm: [1, 0, 0, 0], first_win: 1, last_win: 2 };
        assert_eq!(a0.complete(), Err(E_DRIFT_TRIB));
        // 窗序颠倒。
        let bad = DriftAttrib { acct: 7, rise_pm: [1, 0, 0, 0], first_win: 5, last_win: 3 };
        assert_eq!(bad.complete(), Err(E_DRIFT_TRIB));
    }

    #[test]
    fn a04_报告_severity分级与告警语义() {
        // +200‰ → 提醒（1）。
        let r1 = make_report(&DriftAttrib {
            acct: 7, rise_pm: [200, 0, 0, 0], first_win: 1, last_win: 3,
        })
        .unwrap_or(DriftReport { acct: 0, severity: 0, prime_res: 0, rise_pm: 0, first_win: 0, last_win: 0 });
        assert_eq!(r1.severity, 1);
        assert_eq!(r1.prime_res, DR_RES_CPU);
        assert_eq!(r1.rise_pm, 200);
        // +400‰ → 警告（2）。
        let r2 = make_report(&DriftAttrib {
            acct: 7, rise_pm: [0, 400, 0, 0], first_win: 1, last_win: 3,
        })
        .unwrap_or(DriftReport { acct: 0, severity: 0, prime_res: 0, rise_pm: 0, first_win: 0, last_win: 0 });
        assert_eq!(r2.severity, 2);
        // +600‰ → 严重（3）。
        let r3 = make_report(&DriftAttrib {
            acct: 7, rise_pm: [0, 0, 600, 0], first_win: 1, last_win: 3,
        })
        .unwrap_or(DriftReport { acct: 0, severity: 0, prime_res: 0, rise_pm: 0, first_win: 0, last_win: 0 });
        assert_eq!(r3.severity, 3);
        assert_eq!(r3.prime_res, DR_RES_BW);
        // 无漂移出报告拒。
        assert_eq!(
            make_report(&DriftAttrib { acct: 7, rise_pm: [0; 4], first_win: 1, last_win: 2 }),
            Err(E_DRIFT_NONE)
        );
    }

    #[test]
    fn a05_多资源归一_全维序列与合计() {
        let mut t = tracker();
        let s = DriftSample { load: DR_LOAD_LIGHT, output: 100, used: [100, 50, 20, 30] };
        // 合计归一 = (100+50+20+30)×1000/100 = 2000。
        assert_eq!(t.observe(DR_LOAD_LIGHT, &s), Ok((false, 2000)));
        assert_eq!(t.series[0], [1000, 500, 200, 300]);
        // 四资源维常量与 F0418 立方同源（跨模块一致性自证）。
        assert_eq!(DR_RES_COUNT, crate::cgb418_staucube::RES_COUNT);
    }
}
