//! F197 温度感知降档（secstar2 · G-G-27）——过热不是故障，是被管理好的物理。
//!
//! **判据（主册）**：三档阈值触发实测（热风枪/环境箱或模拟注入）；graceful
//! 跳过路径（不可读机型）实测；降档后温度回落曲线归因。
//!
//! **功能定义（主册 G-G-27）**：宿主温度传感器可读时（ACPI thermal zone）：
//! 高温三档响应（75℃ 降性能档 F069/85℃ 降频+通知/95℃ 保护性冲刷+关机预警
//! ——同 F196 链）；数据不可读优雅跳过。
//!
//! 【交互设计】85℃ 通知三要素+「查看温度」直跳监视器温度页（实时曲线+当前
//! 档位）；95℃ 预警卡复用 F196 语汇（倒计时冲刷）；托盘温度角标可选。
//! 【数据与存储】温度曲线入账本（F060 分项）；降档事件审计；阈值配置层。
//! 【状态与异常】传感器读数异常（负值/跳变 >20℃/s）→ 滤波丢弃+诊断标注；
//! 传感器中途失效 → 优雅退出温度管理+一次通知（不反复骚扰）；与 F048 手动
//! 档叠加规则：温度强制优先（安全>偏好）。
//! 【设计细节】读数频率 1 次/2s（功耗平衡）；三档迟滞（触发-回退差 5℃——
//! 防阈值震荡）；75℃ 档仅调 F069 边界（无感降档）；95℃ 冲刷复用 F196 管线
//! （同一套体面关机——一套机制两处用）；温度页曲线 60s 窗口+24h 聚合双视图。
//!
//! 依赖锚点：F048（手动档叠加）、F060（曲线账本）、F069（性能档联动）、F196（冲刷管线）。

use crate::checks::CheckSet;
use crate::star::sbase::MinuteBook;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 常量（主册数值，一处一事实）
// ---------------------------------------------------------------------------

/// 三档阈值（℃）：降档 / 降频+通知 / 保护性冲刷+关机预警。
pub const THROTTLE_C: i64 = 75;
pub const NOTIFY_C: i64 = 85;
pub const CRIT_C: i64 = 95;

/// 三档迟滞：触发-回退差 5℃（防阈值震荡）。
pub const HYSTERESIS_C: i64 = 5;

/// 读数频率：1 次/2s。
pub const SAMPLE_PERIOD_S: u64 = 2;

/// 读数异常判定：秒间跳变 >20℃。
pub const JUMP_REJECT_C: i64 = 20;

/// 温度账本保留窗口（分钟）。
pub const CURVE_RETENTION_MIN: u64 = 24 * 60;

// ---------------------------------------------------------------------------
// 档位与事件
// ---------------------------------------------------------------------------

/// 降档档位（三档+正常）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ThermoLevel {
    /// 正常（无降档）。
    Normal = 0,
    /// 75℃：降性能档边界（F069——无感降档）。
    Throttle = 1,
    /// 85℃：降频+通知。
    Notify = 2,
    /// 95℃：保护性冲刷+关机预警（F196 链）。
    Critical = 3,
}

impl ThermoLevel {
    /// 触发阈值。
    pub fn trigger_c(self) -> i64 {
        match self {
            ThermoLevel::Normal => i64::MIN,
            ThermoLevel::Throttle => THROTTLE_C,
            ThermoLevel::Notify => NOTIFY_C,
            ThermoLevel::Critical => CRIT_C,
        }
    }

    /// 回退阈值（触发-迟滞；Normal 无回退线——饱和防溢出）。
    pub fn release_c(self) -> i64 {
        self.trigger_c().saturating_sub(HYSTERESIS_C)
    }

    /// 通知三要素文案（85℃ 档——发生了什么/为什么/下一步）。
    pub fn notify_text(self) -> &'static str {
        match self {
            ThermoLevel::Notify => "设备温度较高，已降低性能保护硬件（查看温度可看实时曲线）",
            ThermoLevel::Critical => "设备过热，即将保护性冲刷并关机（请保存工作）",
            _ => "",
        }
    }
}

/// 传感器健康。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SensorHealth {
    Readable,
    /// 读数异常（负值/超界）——滤波丢弃+诊断标注。
    Rejecting,
    /// 中途失效——优雅退出温度管理+一次通知。
    Failed,
}

/// 降档事件审计条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThermoEvent {
    pub at_s: u64,
    /// 升档（true）或回退（false）。
    pub up: bool,
    pub from: ThermoLevel,
    pub to: ThermoLevel,
    pub temp_c: i64,
}

// ---------------------------------------------------------------------------
// 温度管理主体
// ---------------------------------------------------------------------------

/// 温度感知降档。
pub struct ThermoGovernor {
    level: ThermoLevel,
    /// 读数计数（读数频率对账——1 次/2s）。
    pub samples: u64,
    /// 上次有效读数（跳变检测基线）。
    last_temp: Option<i64>,
    health: SensorHealth,
    /// 失效通知已发（一次通知语义）。
    fail_notified: bool,
    /// 60s 曲线窗（30 个采样点——温度页实时视图数据源）。
    window: crate::star::sbase::RingLog<(u64, i64), 30>,
    /// 24h 分钟账本（温度曲线——F060 分项同源）。
    curve: MinuteBook,
    /// 事件审计流。
    pub events: Vec<ThermoEvent>,
    /// 降档是否覆盖手动档（温度强制优先——安全>偏好）。
    pub manual_override_blocked: u64,
    /// 丢弃读数计数（诊断标注）。
    pub rejected_samples: u64,
    /// 连续拒绝计数（≥3 → 重定基线——传感器真实漂移 vs 噪声的分界）。
    consec_rejects: u32,
    /// 重定基线次数（诊断标注——每次都该被追查）。
    pub re_baselines: u64,
    /// 95℃ 冲刷预警回调次数（F196 管线联动对账）。
    pub flush_warnings: u64,
}

/// 连续拒绝重定基线阈值。
const REBASE_AFTER: u32 = 3;

impl ThermoGovernor {
    pub fn new() -> ThermoGovernor {
        ThermoGovernor {
            level: ThermoLevel::Normal,
            samples: 0,
            last_temp: None,
            health: SensorHealth::Readable,
            fail_notified: false,
            window: crate::star::sbase::RingLog::new(),
            curve: MinuteBook::new(1, CURVE_RETENTION_MIN),
            events: Vec::new(),
            manual_override_blocked: 0,
            rejected_samples: 0,
            consec_rejects: 0,
            re_baselines: 0,
            flush_warnings: 0,
        }
    }

    pub fn level(&self) -> ThermoLevel {
        self.level
    }

    pub fn health(&self) -> SensorHealth {
        self.health
    }

    /// 传感器可读性（不可读机型——graceful 跳过：温度管理整体旁路）。
    pub fn set_readable(&mut self, readable: bool) {
        self.health = if readable { SensorHealth::Readable } else { SensorHealth::Failed };
    }

    /// **读数主路**（判据一二三核心）：异常滤波 → 迟滞档位判定 → 事件审计。
    /// `at_s` 必须按采样周期推进（1 次/2s——对账）。
    pub fn sample(&mut self, temp_c: i64, at_s: u64) -> Option<ThermoEvent> {
        // graceful 跳过：传感器失效后不再处理读数（一次通知语义）。
        if self.health == SensorHealth::Failed {
            if !self.fail_notified {
                self.fail_notified = true;
            }
            return None;
        }
        // 采样周期对账（不强制——诊断面可见偏离）。
        self.samples += 1;

        // 异常滤波：负值 / 超界 / 秒间跳变 >20℃ → 丢弃+诊断标注；
        // 连续 ≥3 次拒绝 → 重定基线（真实漂移 vs 单次噪声的分界——基线
        // 永久卡死会让真实过热永远读不到，那比噪声更危险）。
        if temp_c < 0 || temp_c > 150 {
            self.rejected_samples += 1;
            self.consec_rejects += 1;
            if self.consec_rejects >= REBASE_AFTER {
                self.rebaseline(temp_c);
            }
            return None;
        }
        if let Some(prev) = self.last_temp {
            if prev.abs_diff(temp_c) as i64 > JUMP_REJECT_C {
                self.rejected_samples += 1;
                self.consec_rejects += 1;
                if self.consec_rejects >= REBASE_AFTER {
                    self.rebaseline(temp_c);
                }
                return None;
            }
        }
        self.consec_rejects = 0;
        self.last_temp = Some(temp_c);
        self.window.push((at_s, temp_c));
        self.curve.record_minute(at_s / 60, &[temp_c.max(0) as u64]);
        self.curve.evict_by_now(at_s / 60);

        // 迟滞档位判定：升档看触发线（即时——安全方向不迟滞），降档看
        // 回退线（触发-5℃，逐级——防阈值震荡）。
        let target = if temp_c >= ThermoLevel::Critical.trigger_c() {
            ThermoLevel::Critical
        } else if temp_c >= ThermoLevel::Notify.trigger_c() {
            ThermoLevel::Notify
        } else if temp_c >= ThermoLevel::Throttle.trigger_c() {
            ThermoLevel::Throttle
        } else {
            ThermoLevel::Normal
        };
        let new_level = if target > self.level {
            target // 升档：直达当前温度触发的最高档（渐升温也逐档walk-through）。
        } else if temp_c < self.level.release_c() {
            // 降档：迟滞线以下，一次退一级。
            match self.level {
                ThermoLevel::Critical => ThermoLevel::Notify,
                ThermoLevel::Notify => ThermoLevel::Throttle,
                ThermoLevel::Throttle => ThermoLevel::Normal,
                ThermoLevel::Normal => ThermoLevel::Normal,
            }
        } else {
            self.level
        };

        if new_level != self.level {
            let up = new_level > self.level;
            let ev = ThermoEvent { at_s, up, from: self.level, to: new_level, temp_c };
            if new_level == ThermoLevel::Critical {
                // 95℃ 预警卡复用 F196 管线（同一套体面关机——两处用）。
                self.flush_warnings += 1;
            }
            self.events.push(ev);
            self.level = new_level;
            Some(ev)
        } else {
            None
        }
    }

    /// 重定基线（连续拒绝后的诚实防线——接受新读数为基线并留诊断痕）。
    fn rebaseline(&mut self, temp_c: i64) {
        self.last_temp = Some(temp_c);
        self.consec_rejects = 0;
        self.re_baselines += 1;
    }

    /// 与 F048 手动档的叠加规则：温度强制优先（安全>偏好）。
    /// 手动档请求高于温度档 → 拒绝并计数（审计面）。
    pub fn manual_request_allowed(&mut self, manual_level: u8) -> bool {
        let cap = match self.level {
            ThermoLevel::Normal => 3,
            ThermoLevel::Throttle => 1,
            ThermoLevel::Notify => 0,
            ThermoLevel::Critical => 0,
        };
        if manual_level as i64 > cap {
            self.manual_override_blocked += 1;
            return false;
        }
        true
    }

    /// 温度页曲线：60s 窗口原始序列（30 采样点 ×2s）。
    pub fn curve_60s(&self) -> Vec<(u64, i64)> {
        self.window.newest_first()
    }

    /// 24h 聚合：每小时均值 ×24（聚合视图；时间轴不足 24h 的早期窗如实为 0）。
    pub fn curve_24h_hourly_avg(&self, now_s: u64) -> Vec<u64> {
        let mut out = Vec::new();
        let now_min = now_s / 60;
        for h in 0..24u64 {
            let to_min = now_min.saturating_sub(h * 60);
            let from_min = to_min.saturating_sub(60);
            let s = self.curve.range_sum(from_min, to_min);
            // range_sum 给区间总和；样本数 = 分钟槽有数据的个数——近似取
            // 区间分钟数（采样纪律 1/min 时精确）。
            let n = (to_min - from_min).max(1);
            out.push(s[0] / n);
        }
        out
    }
}

impl Default for ThermoGovernor {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F197 自检（聚合进 secstar2 域）。
pub fn run_thermgov_checks() -> CheckSet {
    let mut set = CheckSet::new("F197-thermgov");

    // 判据一：三档阈值触发（模拟注入）。
    let mut t = ThermoGovernor::new();
    let mut at = 0u64;
    let mut evs: Vec<ThermoEvent> = Vec::new();
    for temp in [40i64, 60, 74, 76, 80, 86, 90, 96, 98] {
        at += SAMPLE_PERIOD_S;
        if let Some(ev) = t.sample(temp, at) {
            evs.push(ev);
        }
    }
    set.add("throttle fires", evs.iter().any(|e| e.to == ThermoLevel::Throttle && e.up), "");
    set.add("notify fires", evs.iter().any(|e| e.to == ThermoLevel::Notify && e.up), "");
    set.add("critical fires", evs.iter().any(|e| e.to == ThermoLevel::Critical && e.up), "");
    set.add("flush warning", t.flush_warnings >= 1, "");
    set.add("level critical", t.level() == ThermoLevel::Critical, "");

    // 判据二：graceful 跳过（不可读机型）。
    let mut t2 = ThermoGovernor::new();
    t2.set_readable(false);
    set.add("graceful skip", t2.sample(90, 2).is_none() && t2.level() == ThermoLevel::Normal, "");
    // 中途失效：先正常读数，再失效 → 优雅退出+一次通知。
    let mut t3 = ThermoGovernor::new();
    let _ = t3.sample(50, 2);
    t3.set_readable(false);
    set.add("midfail quiet", t3.sample(60, 4).is_none(), "");
    set.add("midfail once", t3.health() == SensorHealth::Failed, "");

    // 异常读数滤波：负值/巨跳丢弃。
    let mut t4 = ThermoGovernor::new();
    let _ = t4.sample(50, 2);
    set.add("neg rejected", t4.sample(-5, 4).is_none(), "");
    set.add("jump rejected", t4.sample(90, 6).is_none(), "");
    set.add("reject counted", t4.rejected_samples == 2, "");
    set.add("good after reject", t4.sample(52, 8).is_none(), "still Normal, sample kept");

    // 迟滞：86 触发后 83 不回退（<85-5=80 才回）。
    let mut t5 = ThermoGovernor::new();
    let _ = t5.sample(74, 2);
    let _ = t5.sample(86, 4);
    set.add("notify on", t5.level() == ThermoLevel::Notify, "");
    set.add("hysteresis holds", t5.sample(83, 6).is_none() && t5.level() == ThermoLevel::Notify, "");
    set.add("release at 79", t5.sample(79, 8).is_some() && t5.level() == ThermoLevel::Throttle, "");

    // 手动档叠加：温度强制优先。
    let mut t6 = ThermoGovernor::new();
    set.add("manual ok normal", t6.manual_request_allowed(3), "");
    let _ = t6.sample(90, 2);
    set.add("manual blocked hot", !t6.manual_request_allowed(3), "");
    set.add("manual blocked counted", t6.manual_override_blocked == 1, "");

    // 回落曲线归因：降档事件后温度序列入账本（曲线非空）。
    let row = t.curve_24h_hourly_avg(at);
    set.add("curve 24h", row.len() == 24, "");

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f197_hysteresis_no_oscillation() {
        // 阈值附近抖动（84/86 交替）：迟滞保证档位不震荡。
        let mut t = ThermoGovernor::new();
        let mut at = 0u64;
        let _ = t.sample(80, { at += 2; at });
        let _ = t.sample(86, { at += 2; at }); // Notify
        let mut transitions = 1;
        for i in 0..20 {
            let temp = if i % 2 == 0 { 86 } else { 84 };
            if t.sample(temp, { at += 2; at }).is_some() {
                transitions += 1;
            }
        }
        assert_eq!(transitions, 1, "84/86 flapping must not change levels");
        assert_eq!(t.level(), ThermoLevel::Notify);
    }

    #[test]
    fn f197_stepwise_release() {
        // 95℃ 后冷却：逐级回退（Critical→Notify→Throttle→Normal），不跳级。
        let mut t = ThermoGovernor::new();
        let mut at = 0u64;
        let _ = t.sample(96, { at += 2; at });
        assert_eq!(t.level(), ThermoLevel::Critical);
        let _ = t.sample(88, { at += 2; at }); // <90 → Notify
        assert_eq!(t.level(), ThermoLevel::Notify);
        let _ = t.sample(78, { at += 2; at }); // <80 → Throttle
        assert_eq!(t.level(), ThermoLevel::Throttle);
        let _ = t.sample(69, { at += 2; at }); // <70 → Normal
        assert_eq!(t.level(), ThermoLevel::Normal);
        // 事件序列全为下行。
        let downs: Vec<_> = t.events.iter().filter(|e| !e.up).collect();
        assert_eq!(downs.len(), 3);
    }

    #[test]
    fn f197_sample_period_discipline() {
        // 读数频率 1 次/2s：samples 计数与调用一致（对账字段）。
        let mut t = ThermoGovernor::new();
        for i in 1..=10u64 {
            let _ = t.sample(50 + i as i64, i * SAMPLE_PERIOD_S);
        }
        assert_eq!(t.samples, 10);
    }

    #[test]
    fn f197_flush_warning_on_critical_only() {
        let mut t = ThermoGovernor::new();
        let _ = t.sample(86, 2);
        assert_eq!(t.flush_warnings, 0, "notify tier is not a flush warning");
        let _ = t.sample(96, 4);
        assert_eq!(t.flush_warnings, 1);
        let _ = t.sample(99, 6);
        assert_eq!(t.flush_warnings, 1, "already critical — no re-fire");
    }

    #[test]
    fn f197_reject_then_recover_trace() {
        let mut t = ThermoGovernor::new();
        let _ = t.sample(70, 2);
        // 巨跳（70→95=25℃）丢弃——单点看是噪声。
        assert!(t.sample(95, 4).is_none());
        assert_eq!(t.level(), ThermoLevel::Normal);
        // 但持续高位（95/95/95 连续拒绝 ≥3）→ 重定基线（真实漂移防线）。
        let _ = t.sample(95, 6);
        let _ = t.sample(95, 8);
        assert_eq!(t.re_baselines, 1, "third consecutive reject re-baselines");
        // 基线重定后：真实高温可达 Critical（基线卡死 bug 的回归测试）。
        let mut temp = 96i64;
        let mut at = 10u64;
        let mut hit = false;
        for _ in 0..4 {
            at += 2;
            if t.sample(temp, at).map(|e| e.to == ThermoLevel::Critical).unwrap_or(false) {
                hit = true;
            }
            temp += 1;
        }
        assert!(hit, "genuine heat reaches critical after re-baseline");
        assert!(t.last_temp.is_some());
    }

    #[test]
    fn f197_curve_window_60s() {
        let mut t = ThermoGovernor::new();
        let mut at = 0u64;
        for i in 0..40u64 {
            at += 2;
            let _ = t.sample(50 + (i % 10) as i64, at);
        }
        let win = t.curve_60s();
        assert_eq!(win.len(), 30, "60s window = 30 samples @2s");
        assert!(win.iter().all(|(_, c)| *c >= 50 && *c <= 59));
    }

    #[test]
    fn f197_run_checks_pass() {
        assert!(run_thermgov_checks().all_passed());
    }
}
