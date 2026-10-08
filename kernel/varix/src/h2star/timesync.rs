//! F295 时间同步与准确性 · 完整设计（STAR I 主册 H 域）。
//!
//! **判据（主册）**：NTP 修正 <2s 阈值与留痕；漂移补偿用例（模拟 48h
//! 离线偏差）；跨机插拔时钟连续性；手动改时提醒。
//!
//! **设计要点（主册）**：系统时钟三重保障：RTC 断电保持、联网时 NTP
//! 校准（偏差 >2s 静默修正，修正事件留痕）、离线时本地时钟漂移按上次
//! 校准斜率补偿（U 盘系统跨机插拔时钟不乱）；时区设置与区域格式解耦；
//! 时间被手动改回历史时提醒一次。
//!
//! 实装：校准器（NTP 注入偏差——>2s 修正+留痕，≤2s 忽略）；漂移斜率
//! 补偿（上次校准记录的 ppm 斜率外推——48h 离线用例）；手动改回历史
//! 提醒（一次性——提醒后不再烦）；留痕账（修正事件全记）。

use crate::checks::CheckSet;

use alloc::vec::Vec;

/// NTP 静默修正阈值（s——判据定值）。
pub const NTP_THRESHOLD_S: i64 = 2;

/// 一次修正留痕。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AdjustEvent {
    pub at_min: u64,
    /// 修正量（秒，带符号）。
    pub delta_s: i64,
    pub source: AdjustSource,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdjustSource {
    Ntp,
    DriftComp,
    Manual,
}

/// 时钟服务。
pub struct TimeKeeper {
    /// 当前系统时刻（分钟戳）。
    pub now_min: u64,
    /// 漂移斜率（秒/小时——上次校准测得，负=慢）。
    pub drift_s_per_hour: f64,
    /// 校准斜率滚动窗（最近 N 次测定——中位数去抖，防单点噪声）。
    pub slope_window: Vec<f64>,
    /// 修正留痕账。
    pub ledger: Vec<AdjustEvent>,
    /// 跨机插拔连续性账（拔出时刻——插入时对账）。
    pub unplugged_at_min: Option<u64>,
}

impl TimeKeeper {
    pub fn new() -> TimeKeeper {
        TimeKeeper {
            now_min: 0,
            drift_s_per_hour: 0.0,
            slope_window: Vec::new(),
            ledger: Vec::new(),
            unplugged_at_min: None,
        }
    }

    /// NTP 校准：`ntp_min` 为权威时刻。偏差 >2s 静默修正+留痕；
    /// ≤2s 忽略（不制造无意义写入）。
    pub fn ntp_sync(&mut self, ntp_min: u64) -> bool {
        let diff = ntp_min as i64 - self.now_min as i64;
        if diff.abs() * 60 > NTP_THRESHOLD_S {
            self.now_min = ntp_min;
            self.ledger.push(AdjustEvent { at_min: ntp_min, delta_s: diff * 60, source: AdjustSource::Ntp });
            true
        } else {
            false
        }
    }

    /// 离线漂移补偿：离线 `offline_hours` 小时后的预测时刻 =
    /// 当前时刻 + 离线时长 + 漂移量（上次校准斜率外推——跨机插拔
    /// 时钟连续性判据；钟走慢为负斜率）。
    pub fn drift_adjusted(&self, offline_hours: u64) -> u64 {
        let adj_s = self.drift_s_per_hour * offline_hours as f64;
        (self.now_min as f64 + offline_hours as f64 * 60.0 + adj_s / 60.0).max(0.0) as u64
    }

    /// 测定并更新漂移斜率：权威源给出真实时刻 → 斜率 = 偏差/离线时长。
    /// 斜率进滚动窗，生效斜率取**窗口中位数**（单点噪声不直接生效——
    /// 深化：两次测量中一次异常不会把时钟带偏）。
    pub fn calibrate_drift(&mut self, true_min: u64, offline_hours: u64) {
        if offline_hours > 0 {
            let diff_s = (true_min as i64 - self.now_min as i64) * 60;
            let slope = diff_s as f64 / offline_hours as f64;
            self.slope_window.push(slope);
            if self.slope_window.len() > 5 {
                let _ = self.slope_window.remove(0);
            }
            self.drift_s_per_hour = median(&self.slope_window).unwrap_or(slope);
            self.now_min = true_min;
            self.ledger.push(AdjustEvent {
                at_min: true_min,
                delta_s: diff_s,
                source: AdjustSource::DriftComp,
            });
        }
    }

    /// 手动改时：改回历史（新值 < 旧值）→ **该次操作**提醒一次
    /// （主册语义判定：「提醒一次」=每次改回历史的操作各提醒一次，
    /// 不是永久静音——每次都可能影响文件排序判断，用户每次都该知道；
    /// 深化二修正首批的永久一次实现，记缺陷账 #22）。
    pub fn manual_set(&mut self, new_min: u64) -> Option<&'static str> {
        let going_back = new_min < self.now_min;
        self.now_min = new_min;
        self.ledger.push(AdjustEvent {
            at_min: new_min,
            delta_s: if going_back { -1 } else { 1 },
            source: AdjustSource::Manual,
        });
        if going_back {
            Some("时钟被改回过去——文件排序判断可能受影响")
        } else {
            None
        }
    }

    // ---------------------------------------------------------------
    // 深化批次二：跨机插拔时钟连续性
    // ---------------------------------------------------------------

    /// 拔出（U 盘整机宿主换机）：记录拔出时刻。
    pub fn unplug(&mut self, at_min: u64) {
        self.unplugged_at_min = Some(at_min);
    }

    /// 插入：RTC 断电保持 + 漂移斜率外推的对账口。插入时刻早于拔出
    /// 时刻超过容差（`tolerance_min`）= 时钟回拨异常——返回提醒并把
    /// 异常留痕（「跨机插拔时钟不乱」的机判：正常路径静默，异常路径
    /// 显性化——异常零静默）。
    pub fn replug(&mut self, rtc_min: u64, tolerance_min: i64) -> Result<(), &'static str> {
        let unplugged = self.unplugged_at_min.take().unwrap_or(self.now_min);
        self.now_min = rtc_min;
        let back = unplugged as i64 - rtc_min as i64;
        if back > tolerance_min {
            self.ledger.push(AdjustEvent {
                at_min: rtc_min,
                delta_s: -back * 60,
                source: AdjustSource::Manual,
            });
            return Err("时钟比拔出时还早——RTC 可能掉电，建议联网校准");
        }
        Ok(())
    }
}

/// 滚动窗中位数（奇偶双口径——去抖的中枢）。
fn median(v: &[f64]) -> Option<f64> {
    if v.is_empty() {
        return None;
    }
    let mut s = v.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap_or(core::cmp::Ordering::Equal));
    let n = s.len();
    Some(if n % 2 == 1 { s[n / 2] } else { (s[n / 2 - 1] + s[n / 2]) / 2.0 })
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_timesync_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-F295");
    let mut tk = TimeKeeper::new();
    tk.now_min = 1_000; // 当前 1000 分钟。
    // NTP 阈值：偏差 1s（<1 分钟）忽略；偏差 3s 修正+留痕。
    set.add(
        "F295 within 2s ignored",
        !tk.ntp_sync(1_000), // 1s 偏差按分钟戳同值——≤2s。
        "<=2s skip",
    );
    let fixed = tk.ntp_sync(1_000 + 1); // 60s 偏差。
    set.add(
        "F295 over 2s fixed+ledger",
        fixed && tk.now_min == 1_001 && tk.ledger.len() == 1 && tk.ledger[0].source == AdjustSource::Ntp,
        "silent fix, kept trace",
    );
    // 漂移补偿：斜率 -50s/h（钟走慢）× 48h → 预测 = 48h 时长 - 40 分钟当量。
    tk.now_min = 2_001;
    tk.drift_s_per_hour = -50.0;
    let predicted = tk.drift_adjusted(48);
    set.add(
        "F295 48h drift comp",
        predicted == 2_001 + 48 * 60 - 40,
        "slope extrapolated",
    );
    // 校准测定：真实时刻落后 40 分钟（8h 离线）→ 斜率 -300s/h。
    let mut tk2 = TimeKeeper::new();
    tk2.now_min = 1_001;
    tk2.calibrate_drift(961, 8);
    set.add(
        "F295 drift measured",
        (tk2.drift_s_per_hour - (-300.0)).abs() < 0.01 && tk2.ledger.len() == 1,
        "from calibration",
    );
    // 跨机插拔连续性：斜率补偿后时刻单调不减。
    let p1 = tk.drift_adjusted(0);
    let p2 = tk.drift_adjusted(10);
    set.add("F295 monotonic", p1 <= p2, "clock never jumps back via drift");
    // 手动改回历史：每次操作提醒一次（主册语义判定——操作级而非永久级）。
    tk.now_min = 5_000;
    let w1 = tk.manual_set(4_000);
    let w2 = tk.manual_set(3_000);
    let w3 = tk.manual_set(6_000); // 改向未来不提醒。
    set.add(
        "F295 backwarn per-op",
        w1.is_some() && w2.is_some() && w3.is_none(),
        "remind each backwards op",
    );
    // --- 深化二：斜率滚动窗中位数去抖（单点噪声不生效）。 ---
    // 三次校准：正常 3600 → 野值 60000 → 正常 3600；生效斜率取窗口
    // 中位数 = 3600（野值被压住——两次测量一次异常不会带偏时钟）。
    let mut tk3 = TimeKeeper::new();
    tk3.now_min = 1_000;
    tk3.calibrate_drift(1_060, 1); // 正常：+60min/1h。
    tk3.now_min = 1_060;
    tk3.calibrate_drift(2_060, 1); // 野值：+1000min/1h。
    set.add(
        "F295 two-point window",
        (tk3.drift_s_per_hour - 31_800.0).abs() < 1.0,
        "median of two = mean",
    );
    tk3.now_min = 2_060;
    tk3.calibrate_drift(2_120, 1); // 回归正常。
    set.add(
        "F295 median wins",
        (tk3.drift_s_per_hour - 3_600.0).abs() < 1.0,
        "outlier suppressed",
    );
    // --- 深化二：跨机插拔连续性（异常显性化——回拨超容差提醒）。 ---
    let mut tk4 = TimeKeeper::new();
    tk4.now_min = 10_000;
    tk4.unplug(10_000);
    set.add(
        "F295 replug normal",
        tk4.replug(11_000, 30).is_ok() && tk4.now_min == 11_000,
        "clock moved forward silently",
    );
    let mut tk5 = TimeKeeper::new();
    tk5.now_min = 10_000;
    tk5.unplug(10_000);
    let back = tk5.replug(9_000, 30);
    set.add(
        "F295 replug rollback flagged",
        back.is_err() && tk5.ledger.last().map(|e| e.delta_s).unwrap_or(0) == -60 * 1_000,
        "rollback surfaced",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f295_time_keeper() {
        let set = run_timesync_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F295 自检红 {f}/{p}");
    }

    #[test]
    fn drift_never_negative_time() {
        let tk = TimeKeeper { now_min: 10, drift_s_per_hour: -1e9, ..TimeKeeper::new() };
        assert!(tk.drift_adjusted(100) == 0, "极端斜率钳到 0——时间不倒流成负数");
    }
}
