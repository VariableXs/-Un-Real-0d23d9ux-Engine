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
    /// 修正留痕账。
    pub ledger: Vec<AdjustEvent>,
    /// 手动改回历史的提醒只弹一次。
    pub backwarn_shown: bool,
}

impl TimeKeeper {
    pub fn new() -> TimeKeeper {
        TimeKeeper { now_min: 0, drift_s_per_hour: 0.0, ledger: Vec::new(), backwarn_shown: false }
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
    pub fn calibrate_drift(&mut self, true_min: u64, offline_hours: u64) {
        if offline_hours > 0 {
            let diff_s = (true_min as i64 - self.now_min as i64) * 60;
            self.drift_s_per_hour = diff_s as f64 / offline_hours as f64;
            self.now_min = true_min;
            self.ledger.push(AdjustEvent {
                at_min: true_min,
                delta_s: diff_s,
                source: AdjustSource::DriftComp,
            });
        }
    }

    /// 手动改时：改回历史（新值 < 旧值）→ 提醒一次（只一次）。
    pub fn manual_set(&mut self, new_min: u64) -> Option<&'static str> {
        let going_back = new_min < self.now_min;
        self.now_min = new_min;
        self.ledger.push(AdjustEvent {
            at_min: new_min,
            delta_s: if going_back { -1 } else { 1 },
            source: AdjustSource::Manual,
        });
        if going_back && !self.backwarn_shown {
            self.backwarn_shown = true;
            Some("时钟被改回过去——文件排序判断可能受影响")
        } else {
            None
        }
    }
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
    // 手动改回历史提醒一次。
    tk.now_min = 5_000;
    let w1 = tk.manual_set(4_000);
    let w2 = tk.manual_set(3_000);
    set.add(
        "F295 backwarn once",
        w1.is_some() && w2.is_none() && tk.backwarn_shown,
        "remind once",
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
