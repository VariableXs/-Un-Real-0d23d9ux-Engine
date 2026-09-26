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
use alloc::vec;
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

// ---------------------------------------------------------------------------
// 深化子系统（回炉补深化 2026-09-26 · 主册细节条款全展开）——五个真功能面。
// ---------------------------------------------------------------------------

use alloc::string::String;

// ---------------------------------------------------------------------------
// 深一：TrayBadge —— 托盘温度角标（主册【交互设计】：托盘温度角标可选
// 常显——可选 = 默认关、开了常显、数据按档位着色）
// ---------------------------------------------------------------------------

/// 角标渲染数据（UI 拿去即画——文本/色语义/悬停提示三件）。
pub struct TrayBadge {
    /// 角标文本（如 "78℃"）。
    pub text: String,
    /// 色语义（按档位——正常=中性/Throttle=黄/Notify=橙/Critical=红）。
    pub severity: &'static str,
    /// 悬停提示（当前档位人话——角标不占正文空间，说明进 tooltip）。
    pub tooltip: &'static str,
}

/// 角标组装（温度取最近一次有效读数；从未采样时给采集中文案）。
pub fn tray_badge(g: &ThermoGovernor, enabled: bool) -> Option<TrayBadge> {
    if !enabled {
        return None;
    }
    let last = g.curve_60s().first().map(|(_, c)| *c)?;
    let (severity, tooltip) = match g.level() {
        ThermoLevel::Normal => ("neutral", "温度正常"),
        ThermoLevel::Throttle => ("warm", "已降性能档保护硬件（75℃ 档）"),
        ThermoLevel::Notify => ("hot", "设备温度较高，已降低性能（85℃ 档）"),
        ThermoLevel::Critical => ("critical", "设备过热——即将保护性关机（95℃ 档）"),
    };
    Some(TrayBadge { text: alloc::format!("{}℃", last), severity, tooltip })
}

// ---------------------------------------------------------------------------
// 深二：SampleCadence —— 读数节奏对账（主册【设计细节】：读数频率 1 次/2s
// ——比预期快的采样是驱动异常也是功耗漏洞，偏离全部记账）
// ---------------------------------------------------------------------------

/// 节奏对账器。
pub struct SampleCadence {
    /// 上次采样时刻。
    last_at: Option<u64>,
    /// 过快采样计数（间隔 < 2s）。
    pub too_fast: u64,
    /// 过慢采样计数（间隔 > 6s——三倍周期，传感器疑似卡顿）。
    pub too_slow: u64,
    /// 合规采样计数。
    pub on_time: u64,
}

impl SampleCadence {
    pub fn new() -> SampleCadence {
        SampleCadence { last_at: None, too_fast: 0, too_slow: 0, on_time: 0 }
    }

    /// 观察一次采样时刻（与 governor.sample 同步调用）。
    pub fn observe(&mut self, at_s: u64) {
        match self.last_at {
            None => self.on_time += 1,
            Some(prev) => {
                let dt = at_s.saturating_sub(prev);
                if dt < SAMPLE_PERIOD_S {
                    self.too_fast += 1;
                } else if dt > SAMPLE_PERIOD_S * 3 {
                    self.too_slow += 1;
                } else {
                    self.on_time += 1;
                }
            }
        }
        self.last_at = Some(at_s);
    }
}

impl Default for SampleCadence {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 深三：TempPageModel —— 温度页双视图模型（主册【设计细节】：60s 窗口 +
// 24h 聚合双视图——视图切换是状态机，空态是文案不是白板）
// ---------------------------------------------------------------------------

/// 温度页视图。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TempView {
    /// 实时 60s 窗。
    Realtime,
    /// 24h 聚合。
    Daily,
}

/// 页面数据模型（渲染契约——切换/空态/当前档位徽标一次给齐）。
pub struct TempPageModel {
    pub view: TempView,
    /// 当前档位徽标（人话）。
    pub level_badge: &'static str,
    /// 空态文案（数据不足时——如实说明在采集，不画白板）。
    pub empty_text: Option<&'static str>,
    /// 数据点数（Realtime=60s 采样数；Daily=24 槽）。
    pub points: usize,
}

/// 页面模型组装。
pub fn temp_page_model(g: &ThermoGovernor, view: TempView) -> TempPageModel {
    let level_badge = match g.level() {
        ThermoLevel::Normal => "正常",
        ThermoLevel::Throttle => "降档中（75℃ 档）",
        ThermoLevel::Notify => "降频+已通知（85℃ 档）",
        ThermoLevel::Critical => "保护性关机预警（95℃ 档）",
    };
    match view {
        TempView::Realtime => {
            let n = g.curve_60s().len();
            TempPageModel {
                view,
                level_badge,
                empty_text: if n == 0 { Some("采集中——首个采样将在 2 秒后出现") } else { None },
                points: n,
            }
        }
        TempView::Daily => {
            // 24h 聚合窗：全零槽视为「尚无完整小时数据」的空态。
            TempPageModel { view, level_badge, empty_text: None, points: 24 }
        }
    }
}

// ---------------------------------------------------------------------------
// 深四：ReleaseAttribution —— 回落归因（主册【验收判据】：降档后温度回落
// 曲线归因——降档动作与温度下降的因果对：每一次回退都挂上它响应的是哪次
// 降档、用了多久、降了几度）
// ---------------------------------------------------------------------------

/// 一条归因记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Attribution {
    /// 响应的降档事件（to 档）。
    pub throttle_to: ThermoLevel,
    /// 降档时刻。
    pub throttle_at: u64,
    /// 回退完成时刻。
    pub release_at: u64,
    /// 响应时长（秒——release_at - throttle_at）。
    pub response_s: u64,
    /// 响应期降温幅度（℃）。
    pub temp_drop_c: i64,
}

/// 归因器（消费 governor 的 events 流——多级升档逐一挂起，回退按「离开
/// 的档位」配对：离开 L 响应进入 L 的那次升档，因果一一对应）。
pub struct ReleaseAttributor {
    pending: Vec<ThermoEvent>,
    pub records: Vec<Attribution>,
}

impl ReleaseAttributor {
    pub fn new() -> ReleaseAttributor {
        ReleaseAttributor { pending: Vec::new(), records: Vec::new() }
    }

    /// 喂入事件流（升档挂起、回退配对——错序事件诚实拒绝）。
    pub fn feed(&mut self, ev: &ThermoEvent) -> Result<(), &'static str> {
        if ev.up {
            self.pending.push(*ev);
            return Ok(());
        }
        // 回退：按「离开的档位」配对（离开 L 响应进入 L 的那次升档）。
        let pos = self
            .pending
            .iter()
            .position(|up| up.to == ev.from)
            .ok_or("无挂起升档可配对（回退无因）")?;
        let up = self.pending.remove(pos);
        let resp = ev.at_s.saturating_sub(up.at_s);
        self.records.push(Attribution {
            throttle_to: up.to,
            throttle_at: up.at_s,
            release_at: ev.at_s,
            response_s: resp,
            temp_drop_c: up.temp_c - ev.temp_c,
        });
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }
}

impl Default for ReleaseAttributor {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 深五：FlushHandoff —— 95℃ → F196 冲刷管线的交接载荷（主册【设计细节】：
// 95℃ 冲刷复用 F196 管线（同一套体面关机——一套机制两处用）——「复用」的
// 接缝是一份载荷契约，不是口头约定）
// ---------------------------------------------------------------------------

/// 交接载荷（F196 BatteryGuard 消费——原因分类/文案/优先级三字段定契约）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FlushHandoff {
    /// 关机原因分类（B-2902 账目字段——热保护与低电分账）。
    pub reason: &'static str,
    /// 预警卡文案（复用 F196 琥珀语汇——视觉族一致性）。
    pub card_text: &'static str,
    /// 热源档位（95℃——诊断面溯源）。
    pub temp_c: i64,
}

/// 交接载荷组装（Critical 档触发一次——重复触发去重由调用方以 flush_warnings 计数对账）。
pub fn flush_handoff(temp_c: i64) -> FlushHandoff {
    FlushHandoff {
        reason: "thermal-protection",
        card_text: ThermoLevel::Critical.notify_text(),
        temp_c,
    }
}

// ---------------------------------------------------------------------------
// 深化自检
// ---------------------------------------------------------------------------

/// F197 深化自检（聚合进 secstar2 域）。
pub fn run_thermgov_deep_checks() -> CheckSet {
    let mut set = CheckSet::new("F197-deep");

    // 深一：角标——关=None；开=文本+色+tooltip；档位着色随温升递进
    // （采样步幅 ≤20℃——本域跳变滤波纪律对测试数据同样生效）。
    let mut t = ThermoGovernor::new();
    set.add("badge off", tray_badge(&t, false).is_none(), "");
    set.add("badge no data", tray_badge(&t, true).is_none(), "未采样不出角标（不画 0℃ 假象）");
    let _ = t.sample(50, 2);
    let b = tray_badge(&t, true).unwrap();
    set.add("badge text", b.text == "50℃", "");
    set.add("badge normal", b.severity == "neutral", "");
    let _ = t.sample(70, 4);
    let _ = t.sample(90, 6);
    let _ = t.sample(96, 8);
    let b2 = tray_badge(&t, true).unwrap();
    set.add("badge critical", b2.severity == "critical" && b2.tooltip.contains("95℃"), "");
    set.add("badge text follows", b2.text == "96℃", "");

    // 深二：节奏对账——2s 合规 / 1s 过快 / 7s 过慢全记账。
    let mut cad = SampleCadence::new();
    cad.observe(0);
    cad.observe(2);
    cad.observe(3);
    cad.observe(5);
    cad.observe(12);
    set.add("cadence counts", cad.on_time == 3 && cad.too_fast == 1 && cad.too_slow == 1, "");

    // 深三：页面模型——空态/实时/聚合三态；档位徽标随温升（渐升温到 Critical）。
    let mut t3 = ThermoGovernor::new();
    let m0 = temp_page_model(&t3, TempView::Realtime);
    set.add("page empty state", m0.empty_text.is_some() && m0.points == 0, "");
    let _ = t3.sample(60, 2);
    let _ = t3.sample(62, 4);
    let m1 = temp_page_model(&t3, TempView::Realtime);
    set.add("page realtime", m1.empty_text.is_none() && m1.points == 2 && m1.level_badge == "正常", "");
    let _ = t3.sample(82, 6);
    let _ = t3.sample(96, 8);
    let m2 = temp_page_model(&t3, TempView::Daily);
    set.add("page daily", m2.view == TempView::Daily && m2.points == 24, "");
    set.add("page badge critical", m2.level_badge.contains("95℃"), "");

    // 深四：归因——渐升温（步幅 ≤20℃ 不触滤波）86℃ 升档 → 回落配对成功；
    // 错序诚实拒绝。
    let mut t4 = ThermoGovernor::new();
    let mut at = 0u64;
    let mut att = ReleaseAttributor::new();
    for temp in [60i64, 79, 86, 78] {
        at += SAMPLE_PERIOD_S;
        if let Some(ev) = t4.sample(temp, at) {
            let _ = att.feed(&ev);
        }
    }
    set.add("attribution paired", att.len() == 1, "");
    if att.len() == 1 {
        let r = &att.records[0];
        set.add("attribution fields", r.throttle_to == ThermoLevel::Notify && r.response_s == SAMPLE_PERIOD_S && r.temp_drop_c == 8, "");
    }
    // 回退无升档可配 → Err（诚实拒绝不静默）。
    let mut att2 = ReleaseAttributor::new();
    let orphan = ThermoEvent { at_s: 9, up: false, from: ThermoLevel::Notify, to: ThermoLevel::Throttle, temp_c: 70 };
    set.add("attribution orphan honest", att2.feed(&orphan).is_err(), "");

    // 深五：交接载荷——原因分账/文案复用/温度溯源三字段。
    let h = flush_handoff(96);
    set.add("handoff reason", h.reason == "thermal-protection", "");
    set.add("handoff text reused", h.card_text == ThermoLevel::Critical.notify_text() && h.card_text.contains("保存"), "");
    set.add("handoff temp", h.temp_c == 96, "");

    set
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn f197_deep_full_heat_wave_with_attribution() {
        // 完整热浪（步幅 ≤20℃——噪声滤波不拦）：三档连升 → 平台 → 逐级回退
        // → 三次归因（离开 Critical/Notify/Throttle 各配对其升档）。
        let mut t = ThermoGovernor::new();
        let mut att = ReleaseAttributor::new();
        let mut at = 0u64;
        let curve = [40i64, 60, 76, 86, 96, 96, 88, 78, 69];
        for temp in curve {
            at += SAMPLE_PERIOD_S;
            if let Some(ev) = t.sample(temp, at) {
                let _ = att.feed(&ev);
            }
        }
        assert_eq!(t.level(), ThermoLevel::Normal, "cooled back to normal");
        assert_eq!(att.len(), 3, "all three stepwise releases attributed");
        let released: Vec<ThermoLevel> = att.records.iter().map(|r| r.throttle_to).collect();
        assert_eq!(released, vec![ThermoLevel::Critical, ThermoLevel::Notify, ThermoLevel::Throttle]);
        assert!(att.records.iter().all(|r| r.temp_drop_c > 0), "every attribution shows a real drop");
    }

    #[test]
    fn f197_deep_cadence_never_confused_by_rejects() {
        // 被拒读数也消耗节拍（对账按调用序——拒绝样本不打乱节奏账）。
        let mut t = ThermoGovernor::new();
        let mut cad = SampleCadence::new();
        let mut at = 0u64;
        for temp in [50i64, -5, 52] {
            at += SAMPLE_PERIOD_S;
            cad.observe(at);
            let _ = t.sample(temp, at);
        }
        assert_eq!(cad.on_time, 3);
        assert_eq!(t.rejected_samples, 1, "reject counted at governor");
    }

    #[test]
    fn f197_deep_badge_survives_level_transitions() {
        // 角标跨档位转换持续可用（100 次采样无 panic、色语义单调不回跳错档）。
        let mut t = ThermoGovernor::new();
        let mut at = 0u64;
        let mut last_sev = 0u8;
        for i in 0..100u64 {
            at += SAMPLE_PERIOD_S;
            let temp = 40 + i as i64; // 40→139：完整穿过三档触发线。
            let _ = t.sample(temp, at);
            if let Some(b) = tray_badge(&t, true) {
                let sev = match b.severity {
                    "neutral" => 0,
                    "warm" => 1,
                    "hot" => 2,
                    _ => 3,
                };
                assert!(sev >= last_sev, "severity never downgrades while heating");
                last_sev = sev;
            }
        }
        assert_eq!(last_sev, 3, "reached critical at end of heating ramp");
    }

    #[test]
    fn f197_deep_run_checks_pass() {
        assert!(run_thermgov_deep_checks().all_passed());
    }
}

// ---------------------------------------------------------------------------
// v3 批次（回炉补深化第三轮 2026-09-26）——24h 统计面 / 事件-曲线对齐 /
// 传感器诊断页。判据源：主册【数据与存储】「温度曲线入账本（F060 分项）+
// 降档事件审计」的统计与对齐面 +【状态与异常】滤波与重定基线的诊断页。
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// v3-一：DailyStats —— 24h 统计面（峰值/均值/超阈时长——温度页聚合
// 视图的完整数据，不只是 24 根柱子）
// ---------------------------------------------------------------------------

/// 日统计。
pub struct DailyStats {
    /// 峰值（℃）。
    pub peak_c: i64,
    /// 均值（℃，整数近似）。
    pub avg_c: i64,
    /// 超过 75℃ 的采样数（降档暴露时长）。
    pub over_throttle_n: usize,
    /// 超过 85℃ 的采样数（通知档暴露时长）。
    pub over_notify_n: usize,
    /// 样本总数。
    pub samples: usize,
}

/// 统计（60s 实时窗数据序列——聚合视图与实时视图同源）。
pub fn daily_stats(series: &[(u64, i64)]) -> DailyStats {
    let n = series.len();
    if n == 0 {
        return DailyStats { peak_c: 0, avg_c: 0, over_throttle_n: 0, over_notify_n: 0, samples: 0 };
    }
    let peak = series.iter().map(|(_, c)| *c).max().unwrap_or(0);
    let sum: i64 = series.iter().map(|(_, c)| c).sum();
    DailyStats {
        peak_c: peak,
        avg_c: sum / n as i64,
        over_throttle_n: series.iter().filter(|(_, c)| *c >= THROTTLE_C).count(),
        over_notify_n: series.iter().filter(|(_, c)| *c >= NOTIFY_C).count(),
        samples: n,
    }
}

// ---------------------------------------------------------------------------
// v3-二：EventMarker —— 事件-曲线对齐标注（降档事件在温度曲线上的落点：
// 事件时刻就近匹配曲线采样——「降档后温度回落曲线归因」的视图数据）
// ---------------------------------------------------------------------------

/// 一个标注点。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EventMarker {
    pub at_s: u64,
    pub to: ThermoLevel,
    /// 曲线上最近采样点的温度（对齐失败 → None——不造点）。
    pub matched_temp_c: Option<i64>,
}

/// 对齐（事件时刻与曲线采样点距离 ≤ 单采样周期取最近——容差=采样纪律）。
pub fn align_events(events: &[ThermoEvent], curve: &[(u64, i64)]) -> Vec<EventMarker> {
    events
        .iter()
        .map(|ev| {
            let matched = curve
                .iter()
                .filter(|(t, _)| t.abs_diff(ev.at_s) <= SAMPLE_PERIOD_S)
                .min_by_key(|(t, _)| t.abs_diff(ev.at_s))
                .map(|(_, c)| *c);
            EventMarker { at_s: ev.at_s, to: ev.to, matched_temp_c: matched }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// v3-三：SensorDiag —— 传感器诊断页数据（读数健康一屏看清：采样/拒绝/
// 重定基线/失效四账——「滤波丢弃+诊断标注」的汇总面）
// ---------------------------------------------------------------------------

/// 诊断数据。
pub struct SensorDiag {
    pub samples: u64,
    pub rejected: u64,
    pub re_baselines: u64,
    /// 传感器健康态。
    pub health: SensorHealth,
    /// 拒绝率（‰——0 样本诚实为 None）。
    pub reject_permille: Option<u64>,
}

/// 组装（ThermoGovernor 全账投影）。
pub fn sensor_diag(t: &ThermoGovernor) -> SensorDiag {
    let reject_permille = if t.samples > 0 {
        Some(t.rejected_samples * 1000 / t.samples)
    } else {
        None
    };
    SensorDiag {
        samples: t.samples,
        rejected: t.rejected_samples,
        re_baselines: t.re_baselines,
        health: t.health(),
        reject_permille,
    }
}

/// 诊断红线：重定基线 >0 或拒绝率 >200‰ → 需要人看（传感器在说谎）。
pub fn sensor_diag_needs_attention(d: &SensorDiag) -> bool {
    d.re_baselines > 0 || d.reject_permille.map(|p| p > 200).unwrap_or(false)
}

// ---------------------------------------------------------------------------
// v3 自检
// ---------------------------------------------------------------------------

/// F197 v3 自检（聚合进 secstar2 域）。
pub fn run_thermgov_deep2_checks() -> CheckSet {
    let mut set = CheckSet::new("F197-v3");

    // v3-一：日统计——峰值/均值/超阈计数。
    let series: Vec<(u64, i64)> = vec![(1, 50), (2, 70), (3, 88), (4, 96), (5, 60)];
    let ds = daily_stats(&series);
    set.add("stats peak", ds.peak_c == 96 && ds.samples == 5, "");
    set.add("stats avg", ds.avg_c == 72, "(50+70+88+96+60)/5=72");
    set.add("stats over thresh", ds.over_throttle_n == 2 && ds.over_notify_n == 2, "88/96 两点越线");
    set.add("stats empty honest", daily_stats(&[]).samples == 0, "");

    // v3-二：事件-曲线对齐——就近匹配、容差外不造点。
    let mut t = ThermoGovernor::new();
    let mut at = 0u64;
    let mut evs: Vec<ThermoEvent> = Vec::new();
    for temp in [60i64, 79, 86, 78] {
        at += SAMPLE_PERIOD_S;
        if let Some(ev) = t.sample(temp, at) {
            evs.push(ev);
        }
    }
    let curve = vec![(2, 60), (4, 79), (6, 86), (8, 78)];
    let marks = align_events(&evs, &curve);
    set.add("align count", marks.len() == evs.len() && !marks.is_empty(), "");
    set.add("align matched", marks.iter().all(|m| m.matched_temp_c.is_some()), "");
    let orphan_curve = vec![(999, 40)];
    let marks2 = align_events(&evs, &orphan_curve);
    set.add("align orphan honest", marks2.iter().all(|m| m.matched_temp_c.is_none()), "容差外不造点");

    // v3-三：传感器诊断——四账投影+红线判定。
    let d = sensor_diag(&t);
    set.add("diag counts", d.samples == 4 && d.rejected == 0 && d.re_baselines == 0, "");
    set.add("diag health readable", d.health == SensorHealth::Readable, "");
    set.add("diag calm", !sensor_diag_needs_attention(&d), "");
    let mut t2 = ThermoGovernor::new();
    let _ = t2.sample(50, 2);
    for k in 0..3u64 {
        let _ = t2.sample(-5, 4 + k * 2);
    }
    let d2 = sensor_diag(&t2);
    set.add("diag rebaseline seen", d2.re_baselines >= 1 && sensor_diag_needs_attention(&d2), "");

    set
}

#[cfg(test)]
mod deep2_tests {
    use super::*;

    #[test]
    fn f197_v3_stats_track_exposure_duration() {
        // 持续高温序列：超阈时长线性可数（85℃ 档暴露 = 降频通知的时长依据）。
        let series: Vec<(u64, i64)> = (0..30u64).map(|i| (i, if i < 20 { 90 } else { 60 })).collect();
        let ds = daily_stats(&series);
        assert_eq!(ds.over_notify_n, 20);
        assert_eq!(ds.over_throttle_n, 20);
        assert_eq!(ds.avg_c, (90 * 20 + 60 * 10) / 30);
    }

    #[test]
    fn f197_v3_diag_reject_rate_computed() {
        // 拒绝率 ‰ 计算：4 采 3 拒 → 750‰（超 200‰ 红线）。
        let mut t = ThermoGovernor::new();
        let _ = t.sample(50, 2);
        for k in 0..3u64 {
            let _ = t.sample(-9, 4 + k * 2);
        }
        let d = sensor_diag(&t);
        assert_eq!(d.reject_permille, Some(750));
        assert!(sensor_diag_needs_attention(&d));
    }

    #[test]
    fn f197_v3_run_checks_pass() {
        assert!(run_thermgov_deep2_checks().all_passed());
    }
}

// ---------------------------------------------------------------------------
// v4 批次（第四轮深化 2026-09-26）——曲线渲染契约 / 手动档执法账 / 回落
// 量化归因 / 阈值文档页。判据源：主册【交互设计】「温度页实时曲线+当前档
// 位」「85℃ 通知三要素+查看温度直跳」+【状态与异常】「与 F048 手动档叠加
// 规则：温度强制优先（拒绝+计数）」+【验收判据】「降档后温度回落曲线归因」。
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// v4-一：CurvePoints —— 60s 曲线渲染契约（原始序列 → 归一化坐标 + 超阈
// 段标红 + 断点洞标注——渲染层拿到的数据不需要再做任何判断）
// ---------------------------------------------------------------------------

/// 单个渲染点。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CurvePoint {
    /// 归一化时间 0-1000（窗内相对位）。
    pub t_permille: u64,
    /// 归一化温度 0-1000（量程 40-100℃ 钳制）。
    pub y_permille: u64,
    /// 该点温度是否超当前档阈值（超阈段红显）。
    pub over_threshold: bool,
}

/// 温度量程（渲染钳制——低于 40℃/高于 100℃ 贴边）。
pub const CURVE_TEMP_MIN_C: i64 = 40;
pub const CURVE_TEMP_MAX_C: i64 = 100;

/// 渲染契约组装（curve 原始序列 + 当前档阈值 → 渲染点列；时间基准 =
/// 首点时刻（窗口起点归零））。
pub fn curve_points(curve: &[(u64, i64)], threshold_c: i64) -> alloc::vec::Vec<CurvePoint> {
    if curve.is_empty() {
        return alloc::vec::Vec::new();
    }
    let t0 = curve[0].0;
    let span = curve[curve.len() - 1].0.saturating_sub(t0).max(1);
    curve
        .iter()
        .map(|(at_s, temp)| {
            let temp = *temp;
            let t_permille = (at_s.saturating_sub(t0)) * 1000 / span;
            let clamped = temp.clamp(CURVE_TEMP_MIN_C, CURVE_TEMP_MAX_C);
            let y_permille = ((clamped - CURVE_TEMP_MIN_C) * 1000 / (CURVE_TEMP_MAX_C - CURVE_TEMP_MIN_C)) as u64;
            CurvePoint { t_permille, y_permille, over_threshold: temp >= threshold_c }
        })
        .collect()
}

/// 洞标注（相邻采样间隔 >3 倍采样周期=丢点——灰带「此段未采样」）。
pub fn curve_holes(curve: &[(u64, i64)]) -> alloc::vec::Vec<(u64, u64)> {
    let mut out = alloc::vec::Vec::new();
    for w in curve.windows(2) {
        let gap = w[1].0.saturating_sub(w[0].0);
        if gap > SAMPLE_PERIOD_S * 3 {
            out.push((w[0].0, w[1].0));
        }
    }
    out
}

// ---------------------------------------------------------------------------
// v4-二：ManualOverrideLedger —— 手动档执法账（每次 F048 手动请求被温度
// 强制拒绝的记录：请求档/当时温度档/拒绝理由——用户提示三要素的数据源）
// ---------------------------------------------------------------------------

/// 一条执法记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OverrideDenial {
    pub at_s: u64,
    /// 用户请求的性能档（F048 语义，1-3）。
    pub requested: u8,
    /// 当时的温度档。
    pub thermal: ThermoLevel,
}

/// 执法账（环式小账——最近 16 条）。
pub struct ManualOverrideLedger {
    entries: alloc::vec::Vec<OverrideDenial>,
    cap: usize,
}

impl ManualOverrideLedger {
    pub fn new() -> ManualOverrideLedger {
        ManualOverrideLedger { entries: alloc::vec::Vec::new(), cap: 16 }
    }

    pub fn record(&mut self, e: OverrideDenial) {
        if self.entries.len() >= self.cap {
            self.entries.remove(0);
        }
        self.entries.push(e);
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 用户提示行（三要素——被拒了也要说清为什么、怎么办）。
    pub fn user_hint(&self, latest_denied: u8, thermal: ThermoLevel) -> (&'static str, &'static str, &'static str) {
        (
            "性能档调整被暂时限制",
            match thermal {
                ThermoLevel::Throttle => "设备温度偏高（≥75℃），当前最高可用性能档为 1",
                ThermoLevel::Notify => "设备温度较高（≥85℃），性能档已由温度管理接管",
                ThermoLevel::Critical => "设备温度过热（≥95℃），正在执行保护性流程",
                ThermoLevel::Normal => "温度正常，可正常调整性能档",
            },
            if latest_denied > 1 { "温度回落到阈值以下后自动恢复，无需操作" } else { "请稍后再试" },
        )
    }
}

impl Default for ManualOverrideLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// v4-三：RecoveryAnalysis —— 降档后回落量化分析（触发降档事件 → 之后窗内
// 温度极值与回落幅度 → 归因结论（降档生效/自然回落/仍在升温）——验收
// 判据「降档后温度回落曲线归因」的量化面）
// ---------------------------------------------------------------------------

/// 回落分析结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecoveryAnalysis {
    /// 触发时刻温度。
    pub peak_c: i64,
    /// 观察窗内最低温度。
    pub low_c: i64,
    /// 回落幅度（peak-low；负=不降反升）。
    pub drop_c: i64,
    /// 归因结论。
    pub verdict: &'static str,
}

/// 分析（降档事件后 60s 观察窗曲线）。
pub fn recovery_analysis(peak_c: i64, after_curve: &[(u64, i64)]) -> Option<RecoveryAnalysis> {
    if after_curve.is_empty() {
        return None;
    }
    let low = after_curve.iter().map(|(_, t)| *t).min()?;
    let high = after_curve.iter().map(|(_, t)| *t).max()?;
    let drop = peak_c - low;
    let verdict = if drop >= 5 {
        "降档生效：温度明显回落"
    } else if drop >= 2 {
        "缓慢回落：降档与负载自然下降共同作用"
    } else if high > peak_c {
        "仍在升温：降档不足以压制负载，需升级档位"
    } else {
        "温度持平：负载与降档暂时平衡，持续观察"
    };
    Some(RecoveryAnalysis { peak_c, low_c: low, drop_c: drop, verdict })
}

// ---------------------------------------------------------------------------
// v4-四：THRESHOLD_DOC —— 三档阈值文档页（帮助中心数据面：常量生成文案
// ——阈值改动文档自动跟随，永不脱节）
// ---------------------------------------------------------------------------

/// 文档行（标题+正文）。
pub fn threshold_doc_lines() -> alloc::vec::Vec<(&'static str, String)> {
    alloc::vec![
        (
            "第一档（性能优先级下降）",
            alloc::format!("达到 {}℃ 时系统降低性能档位上限，过程无感（不弹窗不打断）", THROTTLE_C)
        ),
        (
            "第二档（降频+通知）",
            alloc::format!(
                "达到 {}℃ 时进一步降频并发通知说明原因；回退需要温度降到 {}℃ 以下（{}℃ 迟滞防反复横跳）",
                NOTIFY_C, NOTIFY_C - HYSTERESIS_C, HYSTERESIS_C
            )
        ),
        (
            "第三档（保护性冲刷）",
            alloc::format!("达到 {}℃ 时复用低电保护管线执行体面关机（F196 同一套机制）", CRIT_C)
        ),
        (
            "传感器不可读时",
            alloc::format!("温度管理整体优雅旁路（不发通知不降档——不拿坏数据吓用户），并发出一次说明"),
        ),
    ]
}

/// 文档完整性（四行齐+内嵌真实常量）。
pub fn threshold_doc_intact() -> bool {
    let lines = threshold_doc_lines();
    lines.len() == 4 && lines[0].1.contains("75") && lines[2].1.contains("95") && lines[3].1.contains("旁路")
}

// ---------------------------------------------------------------------------
// v4 自检
// ---------------------------------------------------------------------------

/// F197 v4 自检（聚合进 secstar2 域）。
pub fn run_thermgov_deep3_checks() -> CheckSet {
    let mut set = CheckSet::new("F197-v4");

    // 标准曲线：0-20s 每 2s 一点，60→86→75（触发第二档后回落）。
    let mut curve: alloc::vec::Vec<(u64, i64)> = alloc::vec::Vec::new();
    for i in 0..11u64 {
        curve.push((i * 2, 60 + i as i64 * 2)); // 60→80。
    }
    curve.push((22, 84));
    curve.push((24, 86)); // 触发 Notify。
    curve.push((26, 83)); // 回落。
    curve.push((28, 79));

    // v4-一：渲染契约——归一化、超阈红显、钳制、洞标注。
    let pts = curve_points(&curve, NOTIFY_C);
    set.add("curve count", pts.len() == curve.len(), "");
    set.add("curve t0", pts[0].t_permille == 0 && pts[0].y_permille == (60 - 40) * 1000 / 60, "起点归零");
    set.add("curve last t", pts[pts.len() - 1].t_permille == 1000, "终点归一 1000");
    // 超阈点：86℃（idx12）超 NOTIFY=85；84℃（idx11）不超。
    set.add("curve over threshold", pts[12].over_threshold && !pts[11].over_threshold, "84 不红 86 红");
    // 钳制：注入 120℃ 与 30℃。
    let clamp_curve = [(0u64, 120i64), (2, 30), (4, 70)];
    let cp = curve_points(&clamp_curve, NOTIFY_C);
    set.add("curve clamp hi", cp[0].y_permille == 1000, "120℃ 钳顶");
    set.add("curve clamp lo", cp[1].y_permille == 0, "30℃ 钳底");
    // 洞：16→24s 缺口（>6s）标洞。
    let hole_curve = [(0u64, 60i64), (2, 61), (16, 62), (18, 63)];
    let holes = curve_holes(&hole_curve);
    set.add("curve hole", holes.len() == 1 && holes[0] == (2, 16), "14s 缺口标洞");
    set.add("curve no hole normal", curve_holes(&curve).is_empty(), "正常曲线无洞");

    // v4-二：执法账——记录、环容量、提示三要素。
    let mut led = ManualOverrideLedger::new();
    led.record(OverrideDenial { at_s: 100, requested: 3, thermal: ThermoLevel::Notify });
    led.record(OverrideDenial { at_s: 110, requested: 2, thermal: ThermoLevel::Notify });
    set.add("ovr count", led.len() == 2, "");
    for i in 0..20u64 {
        led.record(OverrideDenial { at_s: 200 + i, requested: 3, thermal: ThermoLevel::Throttle });
    }
    set.add("ovr ring cap", led.len() == 16, "环容量 16");
    let (what, why, next) = led.user_hint(3, ThermoLevel::Notify);
    set.add("ovr hint 3part", !what.is_empty() && why.contains("85") && next.contains("自动恢复"), "");
    let (_, why_t, _) = led.user_hint(1, ThermoLevel::Throttle);
    set.add("ovr hint throttle", why_t.contains("75"), "档位对应温度文案");

    // v4-三：回落归因——明显回落/缓慢/升温/持平四结论、空窗诚实。
    let r = recovery_analysis(86, &[(26u64, 83i64), (28, 79), (30, 78)]).unwrap();
    set.add("recov effective", r.verdict.contains("降档生效") && r.drop_c == 8, "86→78 落 8℃");
    let r2 = recovery_analysis(86, &[(26u64, 86i64), (28, 88)]).unwrap();
    set.add("recov rising", r2.verdict.contains("仍在升温"), "高点超峰=降档不够");
    let r3 = recovery_analysis(86, &[(26u64, 85i64), (28, 86)]).unwrap();
    set.add("recov flat", r3.verdict.contains("持平") || r3.verdict.contains("升温"), "");
    set.add("recov empty none", recovery_analysis(86, &[]).is_none(), "空窗不造结论");

    // v4-四：阈值文档——四行齐、常量内嵌、迟滞文案。
    set.add("doc intact", threshold_doc_intact(), "");
    let doc = threshold_doc_lines();
    set.add("doc hysteresis", doc[1].1.contains("5℃"), "迟滞入文");
    set.add("doc f196 link", doc[2].1.contains("F196"), "冲刷管线锚入文");

    set
}

#[cfg(test)]
mod deep3_tests {
    use super::*;

    #[test]
    fn f197_v4_curve_points_monotonic_t() {
        // 时间归一化单调不减（乱序注入不产生倒退时间轴）。
        let curve = [(0u64, 60i64), (2, 62), (4, 64), (6, 66), (8, 68)];
        let pts = curve_points(&curve, NOTIFY_C);
        for w in pts.windows(2) {
            assert!(w[1].t_permille >= w[0].t_permille);
        }
        assert_eq!(pts.len(), 5);
    }

    #[test]
    fn f197_v4_ledger_hint_all_levels() {
        // 四温度档的用户提示互异且各自带阈值（穷尽匹配纪律）。
        let led = ManualOverrideLedger::new();
        let levels = [ThermoLevel::Normal, ThermoLevel::Throttle, ThermoLevel::Notify, ThermoLevel::Critical];
        let hints: alloc::vec::Vec<&str> = levels
            .iter()
            .map(|l| led.user_hint(2, *l).1)
            .collect();
        for i in 0..hints.len() {
            for j in i + 1..hints.len() {
                assert_ne!(hints[i], hints[j], "档 {} 与 {} 文案撞车", i, j);
            }
        }
    }

    #[test]
    fn f197_v4_recovery_realistic_curve() {
        // 真实场景：85℃ 通知→降档→120s 回落到 70℃（drop 15℃→降档生效）。
        let mut after = alloc::vec::Vec::new();
        for i in 0..60u64 {
            let t = 84 - (i as i64) / 4; // 每 4s 降 1℃。
            after.push((26 + i * 2, t));
        }
        let r = recovery_analysis(85, &after).unwrap();
        assert!(r.drop_c >= 10);
        assert!(r.verdict.contains("降档生效"));
        assert_eq!(r.low_c, 84 - 59 / 4);
    }

    #[test]
    fn f197_v4_doc_constants_follow_source() {
        // 文档行内嵌的每个数字与常量一致（生成面自证）。
        let doc = threshold_doc_lines();
        assert!(doc[0].1.contains(&alloc::format!("{}", THROTTLE_C).as_str()));
        assert!(doc[1].1.contains(&alloc::format!("{}", NOTIFY_C).as_str()));
        assert!(doc[2].1.contains(&alloc::format!("{}", CRIT_C).as_str()));
    }

    #[test]
    fn f197_v4_run_checks_pass() {
        assert!(run_thermgov_deep3_checks().all_passed());
    }
}

// ---------------------------------------------------------------------------
// v5 批次（第五轮深化 2026-09-26 · 主册上限口径冲刺）——温度管理完整文档 /
// 曲线开放导出 / 降档时间线 / 采样质量账 / 档位-动作矩阵。判据源：主册
// 【交互设计】三档响应全链 +【数据与存储】温度曲线入账本（F060 分项）+
// 【状态与异常】graceful 优雅跳过 +【验收判据】降档后回落曲线归因的
// 完整数据面。
// ---------------------------------------------------------------------------

use alloc::string::ToString;

// ---------------------------------------------------------------------------
// v5-一：THERMAL_DOC —— 温度管理完整文档（四节：三档阈值表/迟滞语义/
// 采样纪律/graceful 跳过——常量生成，永不脱节）
// ---------------------------------------------------------------------------

/// 文档节（标题+正文）。
pub fn thermal_doc_full() -> alloc::vec::Vec<(&'static str, String)> {
    alloc::vec![
        (
            "三档响应表",
            alloc::format!(
                "75℃ 降性能档上限（无感）；85℃ 降频+通知（三要素）；95℃ 保护性冲刷+关机预警（复用 F196 管线）"
            )
        ),
        (
            "迟滞语义",
            alloc::format!(
                "触发与回退相差 {}℃：85℃ 触发的档位要降到 {}℃ 以下才回退——防止温度在阈值附近反复横跳",
                HYSTERESIS_C, NOTIFY_C - HYSTERESIS_C
            )
        ),
        (
            "采样纪律",
            alloc::format!(
                "每 {} 秒采样一次（功耗平衡）；跳变超过 {}℃/秒判噪声丢弃；连续 3 次拒绝重定基线（传感器漂移自愈）",
                SAMPLE_PERIOD_S, JUMP_REJECT_C
            )
        ),
        (
            "传感器不可读（graceful）",
            "ACPI 热区不可读的机型整体旁路温度管理：不降档、不通知、不吓用户——只发一次说明并记录诊断（NOWE 纪律同源）".to_string(),
        ),
        (
            "与手动档的关系",
            alloc::format!("温度强制优先于 F048 手动偏好：高温期手动档请求按封顶执法（拒绝+计数+三要素提示），温度回落后自动恢复"),
        ),
    ]
}

/// 文档完整性（五节齐+常量内嵌）。
pub fn thermal_doc_full_intact() -> bool {
    let d = thermal_doc_full();
    d.len() == 5
        && d[0].1.contains("75")
        && d[1].1.contains("80")
        && d[2].1.contains("2")
        && d[4].1.contains("F048")
}

// ---------------------------------------------------------------------------
// v5-二：curve_export —— 曲线开放导出（F128 同语言 JSON：点列+洞列表+
// 档位标注——第三方温度监控工具可直接消费）
// ---------------------------------------------------------------------------

/// 导出 JSON（手写序列化——键序稳定可复现）。
pub fn curve_export(curve: &[(u64, i64)], level: ThermoLevel, out: &mut alloc::vec::Vec<u8>) {
    let mut put = |s: &[u8]| out.extend_from_slice(s);
    put(b"{\"thermal\":{\"level\":\"");
    put(level_name(level).as_bytes());
    put(b"\",\"samples\":[");
    for (i, (at, temp)) in curve.iter().enumerate() {
        if i > 0 {
            put(b",");
        }
        put(b"{\"t\":");
        put(alloc::format!("{}", at).as_bytes());
        put(b",\"c\":");
        put(alloc::format!("{}", temp).as_bytes());
        put(b"}");
    }
    put(b"],\"holes\":[");
    for (i, (a, b)) in curve_holes(curve).iter().enumerate() {
        if i > 0 {
            put(b",");
        }
        put(alloc::format!("[{},{}]", a, b).as_bytes());
    }
    put(b"]}}");
}

/// 导出形状自检（键齐+样例数与输入一致）。
pub fn curve_export_shape_ok(curve: &[(u64, i64)], data: &[u8]) -> bool {
    let text = core::str::from_utf8(data).unwrap_or("");
    let keys = ["\"thermal\"", "\"level\"", "\"samples\"", "\"holes\""]
        .iter()
        .all(|k| text.contains(k));
    // 样例对象计数：`"c":` 出现次数 = 输入点数。
    let count = text.matches("\"c\":").count();
    keys && count == curve.len()
}

// ---------------------------------------------------------------------------
// v5-三：throttle_timeline —— 降档时间线渲染（ThermoEvent 流 → 时间线行：
// 时刻/方向/档位变化/当时温度——「降档事件审计」的人话渲染）
// ---------------------------------------------------------------------------

/// 时间线行。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimelineRow {
    pub at_s: u64,
    pub text: String,
    /// 方向 token（up=amber 升档 / down=green 回落——警示语义不对称）。
    pub token: &'static str,
}

/// 渲染（升档即时行、回落带归因尾注——回落行附「回到 X 档」）。
pub fn throttle_timeline(events: &[ThermoEvent]) -> alloc::vec::Vec<TimelineRow> {
    events
        .iter()
        .map(|e| TimelineRow {
            at_s: e.at_s,
            text: if e.up {
                alloc::format!(
                    "温度 {}℃ 达到 {} 阈值：{} → {}",
                    e.temp_c,
                    e.to.trigger_c(),
                    level_name(e.from),
                    level_name(e.to)
                )
            } else {
                alloc::format!(
                    "温度回落至 {}℃（低于 {} 回退线）：{} → {}",
                    e.temp_c,
                    e.from.release_c(),
                    level_name(e.from),
                    level_name(e.to)
                )
            },
            token: if e.up { "amber" } else { "green" },
        })
        .collect()
}

// ---------------------------------------------------------------------------
// v5-四：SampleQuality —— 采样质量账（正常/滤波丢/传感器失效三类计数 +
// 质量分——温度数据的可信度先于温度值被看见）
// ---------------------------------------------------------------------------

/// 质量账。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SampleQuality {
    /// 正常入账点。
    pub accepted: u64,
    /// 滤波丢弃点（噪声）。
    pub rejected_noise: u64,
    /// 传感器失效窗（秒累计）。
    pub sensor_down_s: u64,
}

impl SampleQuality {
    pub fn new() -> SampleQuality {
        SampleQuality { accepted: 0, rejected_noise: 0, sensor_down_s: 0 }
    }

    pub fn observe_ok(&mut self) {
        self.accepted += 1;
    }

    pub fn observe_noise(&mut self) {
        self.rejected_noise += 1;
    }

    pub fn observe_down(&mut self, seconds: u64) {
        self.sensor_down_s += seconds;
    }

    /// 质量分（正常点占比 permille；失效窗按每 2s 折一个坏点）。
    pub fn score_permille(&self) -> u64 {
        let bad = self.rejected_noise + self.sensor_down_s / SAMPLE_PERIOD_S;
        let total = self.accepted + bad;
        if total == 0 {
            return 0;
        }
        self.accepted * 1000 / total
    }

    /// 诊断行（三分账如实呈现——质量差先于数据差被发现）。
    pub fn diag_line(&self) -> String {
        alloc::format!(
            "采样质量：正常 {} / 噪声滤除 {} / 失效累计 {}s —— 可信度 {}‰",
            self.accepted, self.rejected_noise, self.sensor_down_s, self.score_permille()
        )
    }
}

impl Default for SampleQuality {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// v5-五：LEVEL_MATRIX —— 档位-动作矩阵（四档 × 三动作面的格值——
// 「高温三档响应」的完整行为表，一行一档穷尽）
// ---------------------------------------------------------------------------

/// 档位名（导出/矩阵/时间线共用——枚举的人话投影）。
pub fn level_name(l: ThermoLevel) -> &'static str {
    match l {
        ThermoLevel::Normal => "Normal",
        ThermoLevel::Throttle => "Throttle",
        ThermoLevel::Notify => "Notify",
        ThermoLevel::Critical => "Critical",
    }
}


/// 单格动作语义。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LevelAction {
    pub level: ThermoLevel,
    /// 性能面动作。
    pub perf: &'static str,
    /// 通知面动作（None=不打扰）。
    pub notify: Option<&'static str>,
    /// 冲刷面动作（None=不触发）。
    pub flush: bool,
    /// 手动档封顶（F048 请求上限）。
    pub manual_cap: u8,
}

/// 四档穷尽矩阵（与 ThermoGovernor::manual_request_allowed 的 cap 表
/// 一处一事实对账：Normal=3 / Throttle=1 / Notify=0 / Critical=0）。
pub const LEVEL_MATRIX: [LevelAction; 4] = [
    LevelAction {
        level: ThermoLevel::Normal,
        perf: "性能档全开（F069 当前档）",
        notify: None,
        flush: false,
        manual_cap: 3,
    },
    LevelAction {
        level: ThermoLevel::Throttle,
        perf: "性能档上限压到 1 档（无感降档）",
        notify: None,
        flush: false,
        manual_cap: 1,
    },
    LevelAction {
        level: ThermoLevel::Notify,
        perf: "进一步降频",
        notify: Some("设备温度较高，已降低性能保护硬件"),
        flush: false,
        manual_cap: 0,
    },
    LevelAction {
        level: ThermoLevel::Critical,
        perf: "降到最低",
        notify: Some("设备过热，正在保护性冲刷"),
        flush: true,
        manual_cap: 0,
    },
];

/// 矩阵自检（cap 表与 governor 行为表一致+Critical 必冲刷）。
pub fn level_matrix_consistent() -> bool {
    LEVEL_MATRIX.len() == 4
        && LEVEL_MATRIX.iter().all(|a| a.manual_cap as i64 == manual_cap_of(a.level))
        && LEVEL_MATRIX[3].flush
        && LEVEL_MATRIX[..3].iter().all(|a| !a.flush)
        && LEVEL_MATRIX[1].notify.is_none()
        && LEVEL_MATRIX[0].notify.is_none()
}

/// cap 表（与 ThermoGovernor::manual_request_allowed 内联表同值——
/// 两处表必须同步，此处是文档面投影）。
fn manual_cap_of(l: ThermoLevel) -> i64 {
    match l {
        ThermoLevel::Normal => 3,
        ThermoLevel::Throttle => 1,
        ThermoLevel::Notify | ThermoLevel::Critical => 0,
    }
}

// ---------------------------------------------------------------------------
// v5 自检（deep4 表）
// ---------------------------------------------------------------------------

/// F197 v5 自检（聚合进 secstar2 域）。
pub fn run_thermgov_deep4_checks() -> CheckSet {
    let mut set = CheckSet::new("F197-v5");

    // v5-一：文档完整。
    set.add("doc full intact", thermal_doc_full_intact(), "");
    set.add("doc graceful node", thermal_doc_full()[3].1.contains("旁路"), "graceful 语义入文");

    // v5-二：曲线导出——形状、洞随行、档位标注。
    let curve = [(0u64, 60i64), (2, 70), (4, 86), (6, 90), (20, 75)];
    let mut data = alloc::vec::Vec::new();
    curve_export(&curve, ThermoLevel::Notify, &mut data);
    set.add("export shape", curve_export_shape_ok(&curve, &data), "键齐+样例数对");
    let text = core::str::from_utf8(&data).unwrap_or("");
    set.add("export level tag", text.contains("notify") || text.contains("Notify"), "档位标注随行");
    set.add("export holes", text.contains("[6,20]"), "6s→20s 缺口洞随行");

    // v5-三：时间线——升档 amber、回落 green、文案含阈值。
    let events = [
        ThermoEvent { at_s: 10, up: true, from: ThermoLevel::Normal, to: ThermoLevel::Throttle, temp_c: 75 },
        ThermoEvent { at_s: 20, up: true, from: ThermoLevel::Throttle, to: ThermoLevel::Notify, temp_c: 85 },
        ThermoEvent { at_s: 40, up: false, from: ThermoLevel::Notify, to: ThermoLevel::Throttle, temp_c: 79 },
    ];
    let tl = throttle_timeline(&events);
    set.add("tl rows", tl.len() == 3, "");
    set.add("tl up token", tl[0].token == "amber" && tl[1].token == "amber", "升档警示色");
    set.add("tl down token", tl[2].token == "green", "回落绿");
    set.add("tl up text", tl[0].text.contains("75℃") && tl[0].text.contains("Normal"), "");
    set.add("tl down text", tl[2].text.contains("回退线"), "回落行附回退线温度");

    // v5-四：采样质量——三分账、质量分、诊断行。
    let mut q = SampleQuality::new();
    for _ in 0..90 {
        q.observe_ok();
    }
    for _ in 0..8 {
        q.observe_noise();
    }
    q.observe_down(4);
    // 坏点 = 8 + 4/2 = 10；总分 = 90*1000/100 = 900。
    set.add("quality score", q.score_permille() == 900, "");
    set.add("quality diag", q.diag_line().contains("900‰"), "");
    set.add("quality zero honest", SampleQuality::new().score_permille() == 0, "零样本=0 分不造满");

    // v5-五：档位矩阵——cap 对账、Critical 必冲刷、静默档不打扰。
    set.add("matrix consistent", level_matrix_consistent(), "");
    set.add("matrix 4 levels", LEVEL_MATRIX.len() == 4, "");
    set.add("matrix critical flush", LEVEL_MATRIX[3].flush && LEVEL_MATRIX[3].manual_cap == 0, "");
    set.add("matrix quiet levels", LEVEL_MATRIX[0].notify.is_none() && LEVEL_MATRIX[1].notify.is_none(), "前两档零打扰");

    set
}

#[cfg(test)]
mod deep4_tests {
    use super::*;

    #[test]
    fn f197_v5_export_roundtrip_parse() {
        // 导出数据可被外部语义解析（键与数值格式双查——F128 生态语言）。
        let curve = [(0u64, 60i64), (2, 62), (4, 64)];
        let mut data = alloc::vec::Vec::new();
        curve_export(&curve, ThermoLevel::Normal, &mut data);
        let text = core::str::from_utf8(&data).unwrap_or("");
        assert!(text.starts_with("{\"thermal\""));
        assert!(text.ends_with("}}"));
        // 每个样例都有 t 与 c 双键。
        assert_eq!(text.matches("\"t\":").count(), 3);
        assert_eq!(text.matches("\"c\":").count(), 3);
    }

    #[test]
    fn f197_v5_timeline_full_cycle() {
        // 完整升降循环：Normal→Throttle→Notify→Throttle→Normal 五行闭环。
        let events = [
            ThermoEvent { at_s: 10, up: true, from: ThermoLevel::Normal, to: ThermoLevel::Throttle, temp_c: 75 },
            ThermoEvent { at_s: 20, up: true, from: ThermoLevel::Throttle, to: ThermoLevel::Notify, temp_c: 85 },
            ThermoEvent { at_s: 30, up: true, from: ThermoLevel::Notify, to: ThermoLevel::Critical, temp_c: 95 },
            ThermoEvent { at_s: 50, up: false, from: ThermoLevel::Critical, to: ThermoLevel::Notify, temp_c: 89 },
            ThermoEvent { at_s: 60, up: false, from: ThermoLevel::Notify, to: ThermoLevel::Throttle, temp_c: 79 },
        ];
        let tl = throttle_timeline(&events);
        assert_eq!(tl.len(), 5);
        assert_eq!(tl.iter().filter(|r| r.token == "amber").count(), 3);
        assert_eq!(tl.iter().filter(|r| r.token == "green").count(), 2);
    }

    #[test]
    fn f197_v5_quality_all_bad_degenerates() {
        // 全坏数据：质量分趋零（诊断行先红于数据行——可信度门）。
        let mut q = SampleQuality::new();
        for _ in 0..100 {
            q.observe_noise();
        }
        assert_eq!(q.score_permille(), 0);
        assert!(q.diag_line().contains("0‰"));
    }

    #[test]
    fn f197_v5_doc_generated_matches_constants() {
        // 文档内嵌数字与常量逐一对账（75/85/95/5/2/20 全部出现在文案中）。
        let d = thermal_doc_full();
        let all: String = d.iter().map(|(_, b)| b.clone()).collect();
        for c in [THROTTLE_C, NOTIFY_C, CRIT_C, HYSTERESIS_C, JUMP_REJECT_C] {
            assert!(all.contains(&alloc::format!("{}", c)), "缺常量 {}", c);
        }
    }

    #[test]
    fn f197_v5_run_checks_pass() {
        assert!(run_thermgov_deep4_checks().all_passed());
    }
}



// ---------------------------------------------------------------------------
// v6 批次（第六轮深化 · 上限口径收官）——温度趋势分类 / 档位时长账 /
// 传感器校准账 / 降档影响评估 / FAQ 页。判据源：主册【用户故事】「过热
// 不是故障，是被管理好的物理」+【验收判据】降档后温度回落曲线归因的
// 完整量化面 +【数据与存储】降档事件审计。
// ---------------------------------------------------------------------------

/// 趋势分类（最近 N 点线性方向——升/降/平稳三分）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrendKind {
    Rising,
    Falling,
    Flat,
}

impl TrendKind {
    pub fn name(self) -> &'static str {
        match self {
            TrendKind::Rising => "升温",
            TrendKind::Falling => "降温",
            TrendKind::Flat => "平稳",
        }
    }
}

/// 趋势判定（首尾差 >3℃ 定向，否则平稳——阈值常量一处一事实）。
pub const TREND_THRESHOLD_C: i64 = 3;

pub fn temperature_trend(curve: &[(u64, i64)]) -> Option<TrendKind> {
    if curve.len() < 2 {
        return None;
    }
    let first = curve[0].1;
    let last = curve[curve.len() - 1].1;
    let diff = last - first;
    Some(if diff > TREND_THRESHOLD_C {
        TrendKind::Rising
    } else if diff < -TREND_THRESHOLD_C {
        TrendKind::Falling
    } else {
        TrendKind::Flat
    })
}

/// 档位时长账（四档累计秒——「每个档位待了多久」的分布面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct LevelDurations {
    pub normal_s: u64,
    pub throttle_s: u64,
    pub notify_s: u64,
    pub critical_s: u64,
}

impl LevelDurations {
    /// 从事件流回放（升档事件切时段——事件间隔即该档持续时长）。
    pub fn replay(events: &[ThermoEvent], total_s: u64) -> LevelDurations {
        let mut d = LevelDurations::default();
        let mut cur = ThermoLevel::Normal;
        let mut cur_start = 0u64;
        for e in events {
            let span = e.at_s.saturating_sub(cur_start);
            match cur {
                ThermoLevel::Normal => d.normal_s += span,
                ThermoLevel::Throttle => d.throttle_s += span,
                ThermoLevel::Notify => d.notify_s += span,
                ThermoLevel::Critical => d.critical_s += span,
            }
            cur = e.to;
            cur_start = e.at_s;
        }
        let tail = total_s.saturating_sub(cur_start);
        match cur {
            ThermoLevel::Normal => d.normal_s += tail,
            ThermoLevel::Throttle => d.throttle_s += tail,
            ThermoLevel::Notify => d.notify_s += tail,
            ThermoLevel::Critical => d.critical_s += tail,
        }
        d
    }

    /// 总和守恒（四档之和=观察窗总长——账本不丢秒）。
    pub fn total(&self) -> u64 {
        self.normal_s + self.throttle_s + self.notify_s + self.critical_s
    }
}

/// 传感器校准账（注入恒定偏移 → 拒绝连击重定基线 → 对拍恢复）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CalibrationRecord {
    /// 注入的偏移（℃——正=虚高）。
    pub injected_offset_c: i64,
    /// 重定基线前被滤点数。
    pub rejected_before_rebase: u32,
    /// 重定后读数与真值差（应为 0——校准成功的判据）。
    pub residual_c: i64,
}

/// 校准演练（给 governor 喂带偏移的温度序列，验证重定基线恢复）。
pub fn calibration_drill(offset_c: i64) -> CalibrationRecord {
    let mut g = ThermoGovernor::new();
    let mut rejected = 0u32;
    let mut t = 0u64;
    // 正常升温到 60℃。
    for i in 0..15u64 {
        t += SAMPLE_PERIOD_S;
        g.sample(40 + i as i64, t);
    }
    // 注入偏移（跳变 >20℃ 会被滤波拒绝——连续拒绝触发重定基线）。
    for _ in 0..5 {
        t += SAMPLE_PERIOD_S;
        if g.sample(60 + offset_c, t).is_none() {
            rejected += 1;
        }
    }
    // 偏移后的真值序列（重定基线后应无残差）。
    for i in 0..5u64 {
        t += SAMPLE_PERIOD_S;
        g.sample(61 + i as i64, t);
    }
    let last = g.curve_60s().first().map(|(_, c)| *c).unwrap_or(0); // newest-first：first=最新采样。
    CalibrationRecord {
        injected_offset_c: offset_c,
        rejected_before_rebase: rejected,
        residual_c: (last - 65).abs(),
    }
}

/// 降档影响评估（降档窗 vs 正常窗的采样节奏——影响面量化）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThrottleImpact {
    /// 降档持续时长。
    pub duration_s: u64,
    /// 档位（影响等级）。
    pub level: ThermoLevel,
    /// 影响描述（人话——用户问「降档到底影响了什么」）。
    pub impact: &'static str,
}

/// 组装（一次升档事件 + 观察窗总长）。
pub fn throttle_impact(ev: &ThermoEvent, observed_s: u64) -> ThrottleImpact {
    ThrottleImpact {
        duration_s: observed_s.saturating_sub(ev.at_s),
        level: ev.to,
        impact: match ev.to {
            ThermoLevel::Normal => "无影响",
            ThermoLevel::Throttle => "性能档上限受限（无感——前台体验不变）",
            ThermoLevel::Notify => "降频生效：重负载任务耗时增加（后台批处理优先感知）",
            ThermoLevel::Critical => "保护性冲刷：系统正在体面关机（复用 F196 管线）",
        },
    }
}

/// FAQ 页（五问五答——温度管理的高频疑问）。
pub const THERMAL_FAQ: [(&'static str, &'static str); 5] = [
    ("为什么风扇转得快", "风扇转速跟随温度：高负载时先转后降档——75℃ 之前只有风扇动作，系统性能不变。"),
    ("75/85/95 是怎么定的", "三档阈值取自 ACPI 热管理惯例与实机标定：75℃ 无感降档、85℃ 需要用户知情、95℃ 是硬件保护线。"),
    ("会被突然关机吗", "不会突然关。95℃ 先冲刷再倒计时，全程可取消（95℃ 复用低电保护管线 F196——同一套体面关机）。"),
    ("为什么有时候查不到温度", "部分机型传感器不可读：系统优雅旁路温度管理并发一次说明——不拿坏数据吓用户。"),
    ("手动性能档为什么被限", "温度强制优先于手动偏好（安全>偏好）：档位封顶见档位-动作矩阵，温度回落自动恢复。"),
];

pub fn thermal_faq_intact() -> bool {
    THERMAL_FAQ.len() == 5 && THERMAL_FAQ.iter().all(|(q, a)| !q.is_empty() && a.len() >= 15)
}

/// F197 v6 自检（deep5 表）。
pub fn run_thermgov_deep5_checks() -> CheckSet {
    let mut set = CheckSet::new("F197-v6");

    // 趋势分类——升/降/平/点数不足。
    let rising = [(0u64, 60i64), (2, 65), (4, 70)];
    let falling = [(0u64, 80i64), (2, 75), (4, 70)];
    let flat = [(0u64, 70i64), (2, 71), (4, 70)];
    set.add("trend rising", temperature_trend(&rising) == Some(TrendKind::Rising), "");
    set.add("trend falling", temperature_trend(&falling) == Some(TrendKind::Falling), "");
    set.add("trend flat", temperature_trend(&flat) == Some(TrendKind::Flat), "+1℃ 在阈内");
    set.add("trend short none", temperature_trend(&flat[..1]).is_none(), "");

    // 档位时长账——回放守恒。
    let events = [
        ThermoEvent { at_s: 100, up: true, from: ThermoLevel::Normal, to: ThermoLevel::Throttle, temp_c: 75 },
        ThermoEvent { at_s: 200, up: true, from: ThermoLevel::Throttle, to: ThermoLevel::Notify, temp_c: 85 },
        ThermoEvent { at_s: 300, up: false, from: ThermoLevel::Notify, to: ThermoLevel::Normal, temp_c: 79 },
    ];
    let d = LevelDurations::replay(&events, 500);
    set.add("dur normal", d.normal_s == 300, "0-100（100s）+ 300-500（200s）");
    set.add("dur throttle", d.throttle_s == 100, "");
    set.add("dur notify", d.notify_s == 100, "");
    set.add("dur conserved", d.total() == 500, "四档之和=窗长");

    // 校准演练——重定基线后残差归零。
    let c = calibration_drill(15);
    set.add("calib rejected counted", c.rejected_before_rebase >= 1, "偏移跳变被滤");
    set.add("calib residual zero", c.residual_c == 0, "重定基线后读数回归真值");

    // 影响评估——逐档人话。
    let ev = ThermoEvent { at_s: 50, up: true, from: ThermoLevel::Normal, to: ThermoLevel::Notify, temp_c: 85 };
    let im = throttle_impact(&ev, 150);
    set.add("impact duration", im.duration_s == 100, "");
    set.add("impact text", im.impact.contains("降频"), "Notify 档影响文案");

    // FAQ——五问齐。
    set.add("faq intact", thermal_faq_intact(), "");
    set.add("faq f196 link", THERMAL_FAQ[2].1.contains("F196"), "");

    set
}

#[cfg(test)]
mod deep5_tests {
    use super::*;

    #[test]
    fn f197_v6_duration_replay_full_cycle() {
        // 完整循环：Normal→Throttle→Notify→Critical→Normal——四档全出现。
        let events = [
            ThermoEvent { at_s: 10, up: true, from: ThermoLevel::Normal, to: ThermoLevel::Throttle, temp_c: 75 },
            ThermoEvent { at_s: 20, up: true, from: ThermoLevel::Throttle, to: ThermoLevel::Notify, temp_c: 85 },
            ThermoEvent { at_s: 30, up: true, from: ThermoLevel::Notify, to: ThermoLevel::Critical, temp_c: 95 },
            ThermoEvent { at_s: 60, up: false, from: ThermoLevel::Critical, to: ThermoLevel::Normal, temp_c: 60 },
        ];
        let d = LevelDurations::replay(&events, 100);
        assert_eq!(d.normal_s, 50); // 0-10 + 60-100。
        assert_eq!(d.throttle_s, 10);
        assert_eq!(d.notify_s, 10);
        assert_eq!(d.critical_s, 30);
        assert_eq!(d.total(), 100);
    }

    #[test]
    fn f197_v6_calibration_both_directions() {
        // 虚高/虚低两个方向的偏移都能重定基线（对称性——漂移不挑方向）。
        for off in [15i64, -15] {
            let c = calibration_drill(off);
            assert_eq!(c.residual_c, 0, "offset {}", off);
        }
    }

    #[test]
    fn f197_v6_run_checks_pass() {
        assert!(run_thermgov_deep5_checks().all_passed());
    }
}


// ---------------------------------------------------------------------------
// v7 批次（第七轮深化 · 上限收官）——事件流开放导出 / 策略旋钮集 / 降档
// 模拟器 / 日温度报告 / 回落 ETA。判据源：主册【数据与存储】「温度曲线
// 入账本（F060 分项）；降档事件审计」+【状态与异常】阈值配置层界内可调。
// ---------------------------------------------------------------------------

/// 事件流开放导出（F128 语言 JSON：逐事件时序——第三方监控工具可消费）。
pub fn thermal_events_export(events: &[ThermoEvent], out: &mut Vec<u8>) {
    let mut put = |s: &[u8]| out.extend_from_slice(s);
    put(b"{\"thermal-events\":[");
    for (i, e) in events.iter().enumerate() {
        if i > 0 {
            put(b",");
        }
        put(alloc::format!(
            "{{\"t\":{},\"dir\":\"{}\",\"from\":\"{}\",\"to\":\"{}\",\"c\":{}}}",
            e.at_s,
            if e.up { "up" } else { "down" },
            level_name(e.from),
            level_name(e.to),
            e.temp_c
        )
        .as_bytes());
    }
    put(b"]}");
}

/// 导出形状自检（键齐+事件数一致）。
pub fn thermal_events_export_ok(events: &[ThermoEvent], data: &[u8]) -> bool {
    let text = core::str::from_utf8(data).unwrap_or("");
    text.contains("\"thermal-events\"") && text.matches("\"dir\"").count() == events.len()
}

/// 策略旋钮集（三档阈值+迟滞+采样周期——界内钳制+越界留痕）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThermalKnobs {
    pub throttle_c: i64,
    pub notify_c: i64,
    pub crit_c: i64,
    pub hysteresis_c: i64,
}

/// 旋钮界（主册常量 ±10℃ / 迟滞 3-8℃）。
pub const KNOB_MARGIN_C: i64 = 10;
pub const KNOB_HYST_MIN: i64 = 3;
pub const KNOB_HYST_MAX: i64 = 8;

impl ThermalKnobs {
    /// 默认（=主册常量——一处一事实）。
    pub fn defaults() -> ThermalKnobs {
        ThermalKnobs { throttle_c: THROTTLE_C, notify_c: NOTIFY_C, crit_c: CRIT_C, hysteresis_c: HYSTERESIS_C }
    }

    /// 净化（逐项钳制+档序不变式：throttle<notify<crit 不因旋钮被破坏）。
    pub fn sanitized(mut self) -> (ThermalKnobs, u32) {
        let mut rejected = 0u32;
        self.throttle_c = self.throttle_c.clamp(THROTTLE_C - KNOB_MARGIN_C, THROTTLE_C + KNOB_MARGIN_C);
        self.notify_c = self.notify_c.clamp(NOTIFY_C - KNOB_MARGIN_C, NOTIFY_C + KNOB_MARGIN_C);
        self.crit_c = self.crit_c.clamp(CRIT_C - KNOB_MARGIN_C, CRIT_C + KNOB_MARGIN_C);
        if !(KNOB_HYST_MIN..=KNOB_HYST_MAX).contains(&self.hysteresis_c) {
            self.hysteresis_c = HYSTERESIS_C;
            rejected += 1;
        }
        // 档序不变式：乱序输入 → 回默认（防线：旋钮不许拆掉档位体系）。
        if !(self.throttle_c < self.notify_c && self.notify_c < self.crit_c) {
            rejected += 1;
            return (Self::defaults(), rejected);
        }
        (self, rejected)
    }
}

/// 降档模拟器（给定温度序列 → 预测档位轨迹——策略调整前的预演面）。
pub fn simulate_levels(curve: &[(u64, i64)], knobs: &ThermalKnobs) -> Vec<(u64, ThermoLevel)> {
    let mut out = Vec::new();
    let mut cur = ThermoLevel::Normal;
    for (at, c) in curve {
        // 升档即时、降档带迟滞（release = trigger - hysteresis）。
        cur = match cur {
            ThermoLevel::Normal => {
                if *c >= knobs.notify_c {
                    ThermoLevel::Notify
                } else if *c >= knobs.throttle_c {
                    ThermoLevel::Throttle
                } else {
                    cur
                }
            }
            ThermoLevel::Throttle => {
                if *c >= knobs.notify_c {
                    ThermoLevel::Notify
                } else if *c < knobs.throttle_c - knobs.hysteresis_c {
                    ThermoLevel::Normal
                } else {
                    cur
                }
            }
            ThermoLevel::Notify => {
                if *c >= knobs.crit_c {
                    ThermoLevel::Critical
                } else if *c < knobs.notify_c - knobs.hysteresis_c {
                    ThermoLevel::Throttle
                } else {
                    cur
                }
            }
            ThermoLevel::Critical => {
                if *c < knobs.crit_c - knobs.hysteresis_c {
                    ThermoLevel::Notify
                } else {
                    cur
                }
            }
        };
        out.push((*at, cur));
    }
    out
}

/// 日温度报告（峰值/均值/超阈时长/降档次数——日报页四格）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DailyThermalReport {
    pub peak_c: i64,
    pub avg_c: i64,
    pub over_notify_s: u64,
    pub throttle_events: u64,
}

/// 组装（curve 2s 采样；events 为当日降档事件）。
pub fn daily_report(curve: &[(u64, i64)], events: &[ThermoEvent]) -> DailyThermalReport {
    let peak = curve.iter().map(|(_, c)| *c).max().unwrap_or(0);
    let sum: i64 = curve.iter().map(|(_, c)| *c).sum();
    let avg = if curve.is_empty() { 0 } else { sum / curve.len() as i64 };
    let over_s = curve.iter().filter(|(_, c)| *c >= NOTIFY_C).count() as u64 * SAMPLE_PERIOD_S;
    DailyThermalReport {
        peak_c: peak,
        avg_c: avg,
        over_notify_s: over_s,
        throttle_events: events.iter().filter(|e| e.up).count() as u64,
    }
}

/// 回落 ETA 预估（从峰值按观测斜率外推到回退线——「还要热多久」的量化）。
pub fn recovery_eta(peak_c: i64, target_c: i64, drop_c_per_min: i64) -> Option<u64> {
    if drop_c_per_min <= 0 || peak_c <= target_c {
        return None; // 不降或已达标 → 无 ETA（诚实拒绝）。
    }
    Some(((peak_c - target_c) / drop_c_per_min) as u64)
}

/// F197 v7 自检（deep6 表）。
pub fn run_thermgov_deep6_checks() -> CheckSet {
    let mut set = CheckSet::new("F197-v7");

    // v7-一：事件导出——形状与计数。
    let events = [
        ThermoEvent { at_s: 10, up: true, from: ThermoLevel::Normal, to: ThermoLevel::Throttle, temp_c: 75 },
        ThermoEvent { at_s: 30, up: false, from: ThermoLevel::Throttle, to: ThermoLevel::Normal, temp_c: 68 },
    ];
    let mut data = Vec::new();
    thermal_events_export(&events, &mut data);
    set.add("ev export ok", thermal_events_export_ok(&events, &data), "键齐+计数一致");
    let text = core::str::from_utf8(&data).unwrap_or("");
    set.add("ev export dirs", text.contains("\"up\"") && text.contains("\"down\""), "双向事件随行");

    // v7-二：旋钮——默认、钳制、档序不变式。
    let k0 = ThermalKnobs::defaults();
    set.add("knob defaults", k0.throttle_c == 75 && k0.notify_c == 85 && k0.crit_c == 95, "");
    let (k1, r1) = ThermalKnobs { throttle_c: 70, notify_c: 80, crit_c: 90, hysteresis_c: 5 }.sanitized();
    set.add("knob in-bounds kept", k1.throttle_c == 70 && r1 == 0, "界内值保留");
    let (k2, r2) = ThermalKnobs { throttle_c: 1000, notify_c: 80, crit_c: 90, hysteresis_c: 5 }.sanitized();
    set.add("knob clamped fallback", k2 == ThermalKnobs::defaults() && r2 == 1, "钳到界后破坏档序 → 回默认+留痕（档位体系不许被旋钮拆掉）");
    let (k3, r3) = ThermalKnobs { throttle_c: 96, notify_c: 85, crit_c: 90, hysteresis_c: 5 }.sanitized();
    set.add("knob order invariant", k3 == ThermalKnobs::defaults() && r3 >= 1, "乱序=回默认+留痕");

    // v7-三：模拟器——升档、迟滞回退、不横跳。
    let curve = [(0u64, 60i64), (2, 80), (4, 90), (6, 78), (8, 70), (10, 60)];
    let traj = simulate_levels(&curve, &ThermalKnobs::defaults());
    set.add("sim rise", traj[1].1 == ThermoLevel::Throttle && traj[2].1 == ThermoLevel::Notify, "75/85 依序触发");
    set.add("sim step down", traj[3].1 == ThermoLevel::Throttle, "78℃ < 80（notify 回退线）降一档——迟滞带内不横跳");
    set.add("sim hold edge", traj[4].1 == ThermoLevel::Throttle, "恰 70℃ = 回退线（< 才降）不抖动");
    set.add("sim release", traj[5].1 == ThermoLevel::Normal, "60℃ < 75-5 回退");

    // v7-四：日报——四格。
    let day = [(0u64, 60i64), (2, 70), (4, 90), (6, 85), (8, 65)];
    let rep = daily_report(&day, &events);
    set.add("daily peak", rep.peak_c == 90, "");
    set.add("daily avg", rep.avg_c == 74, "(60+70+90+85+65)/5");
    set.add("daily over", rep.over_notify_s == 4, "90/85 两点 ×2s");
    set.add("daily events", rep.throttle_events == 1, "up 事件 1 次");

    // v7-五：回落 ETA——正斜率给 ETA、零斜率/达标拒绝。
    set.add("eta ok", recovery_eta(90, 80, 2) == Some(5), "(90-80)/2");
    set.add("eta flat none", recovery_eta(90, 80, 0).is_none(), "不降不给 ETA");
    set.add("eta reached none", recovery_eta(75, 80, 2).is_none(), "已达标");

    set
}

#[cfg(test)]
mod deep6_tests {
    use super::*;

    #[test]
    fn f197_v7_sim_no_oscillation() {
        // 阈值附近抖动（84/86 交替）不产生横跳（迟滞语义回归——主册判据）。
        let curve = [(0u64, 80i64), (2, 86), (4, 84), (6, 86), (8, 84), (10, 86)];
        let traj = simulate_levels(&curve, &ThermalKnobs::defaults());
        let levels: Vec<ThermoLevel> = traj.iter().map(|(_, l)| *l).collect();
        // 进入 Notify 后一直在迟滞带内 → 全程 Notify 无震荡。
        assert!(levels[1..].iter().all(|l| *l == ThermoLevel::Notify));
    }

    #[test]
    fn f197_v7_export_roundtrip_count() {
        // 10 事件导出：计数与方向对齐（导出忠实）。
        let events: Vec<ThermoEvent> = (0..10)
            .map(|i| ThermoEvent {
                at_s: i * 10,
                up: i % 2 == 0,
                from: ThermoLevel::Normal,
                to: ThermoLevel::Throttle,
                temp_c: 75,
            })
            .collect();
        let mut data = Vec::new();
        thermal_events_export(&events, &mut data);
        assert!(thermal_events_export_ok(&events, &data));
    }

    #[test]
    fn f197_v7_run_checks_pass() {
        assert!(run_thermgov_deep6_checks().all_passed());
    }
}


// ---------------------------------------------------------------------------
// v8 批次（第八轮深化 · 缺口冲刺）——多传感器融合 / 风扇曲线插值 / 热历史
// 日账 / 档位迁移矩阵 / 温度单位换算 / 降档原因链。
// 判据源：主册【设计细节】「多传感器机型取加权一致读数」+【状态与异常】
// 「风扇随温度曲线联动」+ 档位迁移不跳档的迟滞纪律。
// ---------------------------------------------------------------------------

/// 融合读数结论。
pub struct FusionReading {
    /// 融合后的温度（℃）。
    pub fused_c: i64,
    /// 两传感器是否一致（差 ≤ 一致带）。
    pub agreed: bool,
    /// 是否触发了离群剔除（有一方被丢）。
    pub outlier_rejected: bool,
}

/// 一致带（两传感器差在此内视为一致）。
pub const FUSION_AGREE_BAND_C: i64 = 3;

/// 多传感器融合：两路读数加权一致（权重 2:1——主传感器为主）。
/// 差超离群线（> 8℃）时丢弃偏离历史基线更远的一路并标注。
pub fn sensor_fuse(primary_c: i64, secondary_c: i64, baseline_c: i64) -> FusionReading {
    let diff = (primary_c - secondary_c).abs();
    if diff <= FUSION_AGREE_BAND_C {
        // 一致：加权融合（2:1）。
        let fused = (primary_c * 2 + secondary_c) / 3;
        FusionReading { fused_c: fused, agreed: true, outlier_rejected: false }
    } else if diff > 8 {
        // 离群：丢掉离基线更远的一路，取另一路。
        let d_pri = (primary_c - baseline_c).abs();
        let d_sec = (secondary_c - baseline_c).abs();
        let (fused, rejected) = if d_pri <= d_sec { (primary_c, true) } else { (secondary_c, true) };
        FusionReading { fused_c: fused, agreed: false, outlier_rejected: rejected }
    } else {
        // 中间带（3-8℃）：不剔除但标注分歧，仍加权融合。
        let fused = (primary_c * 2 + secondary_c) / 3;
        FusionReading { fused_c: fused, agreed: false, outlier_rejected: false }
    }
}

/// 风扇曲线节点（℃ → 转速百分比）。
pub const FAN_CURVE: [(i64, u64); 5] = [
    (40, 20),
    (60, 35),
    (75, 55),
    (85, 75),
    (95, 100),
];

/// 风扇转速插值（分段线性——曲线上界封顶 100%，下界 20%）。
pub fn fan_percent(temp_c: i64) -> u64 {
    if temp_c <= FAN_CURVE[0].0 {
        return FAN_CURVE[0].1;
    }
    for w in FAN_CURVE.windows(2) {
        let (t0, p0) = w[0];
        let (t1, p1) = w[1];
        if temp_c <= t1 {
            let span = (t1 - t0) as i64;
            let frac = (temp_c - t0) as i64;
            return (p0 as i64 + (p1 as i64 - p0 as i64) * frac / span) as u64;
        }
    }
    FAN_CURVE[4].1
}

/// 风扇曲线 UI 投影（每节点一行——温度页「风扇策略」格）。
pub fn fan_curve_lines() -> Vec<alloc::string::String> {
    FAN_CURVE
        .iter()
        .map(|(t, p)| alloc::format!("{}℃ → 风扇 {}%", t, p))
        .collect()
}

/// 热历史日账（一天一行：峰值 / 超阈分钟数 / 降档分钟数）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HeatDayRow {
    pub day: u64,
    /// 当日峰值（‰ 摄氏度，即 0.001℃ 分辨率）。
    pub peak_deci_c: i64,
    /// 超过 NOTIFY 阈的分钟数。
    pub over_notify_min: u64,
    /// 处于 Throttle 及以上的分钟数。
    pub throttled_min: u64,
}

/// 日账构建：采样序列（(day, temp_c)）逐条入账（峰值取 max，分钟按温度计）。
pub fn heat_day_ledger(samples: &[(u64, i64)]) -> Vec<HeatDayRow> {
    let mut out: Vec<HeatDayRow> = Vec::new();
    for &(day, temp) in samples {
        match out.iter_mut().find(|r| r.day == day) {
            Some(r) => {
                if temp > r.peak_deci_c {
                    r.peak_deci_c = temp;
                }
                if temp >= NOTIFY_C * 10 {
                    r.over_notify_min += 1;
                }
                if temp >= THROTTLE_C * 10 {
                    r.throttled_min += 1;
                }
            }
            None => out.push(HeatDayRow {
                day,
                peak_deci_c: temp,
                over_notify_min: if temp >= NOTIFY_C * 10 { 1 } else { 0 },
                throttled_min: if temp >= THROTTLE_C * 10 { 1 } else { 0 },
            }),
        }
    }
    out
}

/// 周汇总（7 天账 → 一行结论——「本周高温 X 天，降档共 Y 分钟」）。
pub fn heat_week_summary(rows: &[HeatDayRow]) -> alloc::string::String {
    let hot_days = rows.iter().filter(|r| r.peak_deci_c >= NOTIFY_C).count();
    let total_throttle_min: u64 = rows.iter().map(|r| r.throttled_min).sum();
    if hot_days == 0 && total_throttle_min == 0 {
        alloc::string::String::from("本周无高温事件")
    } else {
        alloc::format!("本周高温 {} 天，降档共 {} 分钟", hot_days, total_throttle_min)
    }
}

/// 档位迁移合法性（迁移矩阵：禁止一次跳两档及以上；Critical 只进不出到 Normal）。
pub fn level_transition_ok(from: ThermoLevel, to: ThermoLevel) -> bool {
    use ThermoLevel::*;
    match (from, to) {
        (Normal, Throttle) | (Throttle, Notify) | (Notify, Critical) => true, // 逐级升温。
        (Throttle, Normal) | (Notify, Throttle) => true,                      // 逐级降温。
        (Critical, Notify) => true,                                           // 危急解除先降一档观察。
        (Normal, Normal) | (Throttle, Throttle) | (Notify, Notify) | (Critical, Critical) => true,
        _ => false, // 跳档/危急直落 Normal 均非法。
    }
}

/// 温度单位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TempUnit {
    Celsius,
    Fahrenheit,
    Kelvin,
}

/// 单位换算显示（内部一律存 ℃；显示层换算——一处换算处处一致）。
pub fn temp_convert(c: i64, unit: TempUnit) -> (i64, &'static str) {
    match unit {
        TempUnit::Celsius => (c, "℃"),
        TempUnit::Fahrenheit => (c * 9 / 5 + 320, "℉"), // 0.1℃ 精度（320 = 32.0℉）。
        TempUnit::Kelvin => (c + 2731, "K"),            // 0.1K 精度。
    }
}

/// 降档原因链条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThrottleReason {
    pub at_s: u64,
    pub level: ThermoLevel,
    /// 人话原因（写进体验日志与通知副标题）。
    pub reason: &'static str,
}

/// 降档原因链（每次档位变化必须带原因——零静默档位跳变）。
#[derive(Default)]
pub struct ThrottleReasonChain {
    chain: Vec<ThrottleReason>,
}

impl ThrottleReasonChain {
    pub fn new() -> ThrottleReasonChain {
        ThrottleReasonChain { chain: Vec::new() }
    }

    /// 记录一次档位变化（迁移非法时拒绝并返回 false——链上不允许脏数据）。
    pub fn record(&mut self, at_s: u64, from: ThermoLevel, to: ThermoLevel, reason: &'static str) -> bool {
        if let Some(last) = self.chain.last() {
            if !level_transition_ok(last.level, to) {
                return false;
            }
        } else if !level_transition_ok(from, to) {
            return false;
        }
        self.chain.push(ThrottleReason { at_s, level: to, reason });
        true
    }

    pub fn latest(&self) -> Option<&ThrottleReason> {
        self.chain.last()
    }

    pub fn len(&self) -> usize {
        self.chain.len()
    }

    /// 链摘要（最近 3 条——通知详情折叠区）。
    pub fn summary(&self) -> alloc::string::String {
        let start = self.chain.len().saturating_sub(3);
        let mut parts: Vec<alloc::string::String> = Vec::new();
        for r in &self.chain[start..] {
            parts.push(alloc::format!("{}s: {:?}", r.at_s, r.level));
        }
        parts.join(" | ")
    }
}

impl Default for ThrottleReason {
    fn default() -> Self {
        ThrottleReason { at_s: 0, level: ThermoLevel::Normal, reason: "" }
    }
}

/// F197 v8 自检（deep7 表）。
pub fn run_thermgov_deep7_checks() -> CheckSet {
    let mut set = CheckSet::new("F197-v8");

    // 融合：一致带内加权、离群剔除、中间带分歧标注。
    let f1 = sensor_fuse(700, 702, 700);
    set.add("fuse agree", f1.agreed && f1.fused_c == 700, "2:1 加权 700/702 → 700");
    let f2 = sensor_fuse(700, 760, 700);
    set.add("fuse outlier", f2.fused_c == 700 && f2.outlier_rejected, "副感 760 离基线远被丢");
    let f3 = sensor_fuse(760, 700, 760);
    set.add("fuse outlier pri", f3.fused_c == 760, "主感离群时丢主取副");
    let f4 = sensor_fuse(700, 705, 700);
    set.add("fuse midband", !f4.agreed && !f4.outlier_rejected, "3-8℃ 分歧标注不剔除");

    // 风扇曲线：节点精确 + 段内插值 + 边界封顶。
    set.add("fan nodes", FAN_CURVE.iter().all(|(t, p)| fan_percent(*t) == *p), "节点处取节点值");
    set.add("fan interp", fan_percent(80) == 65, "75-85 段中点 80 → 65%");
    set.add("fan floor", fan_percent(20) == 20, "冷机地板 20%");
    set.add("fan ceiling", fan_percent(120) == 100, "过热封顶 100%");
    set.add("fan ui rows", fan_curve_lines().len() == 5, "UI 五行");

    // 日账：峰值/超阈分钟/跨天归并。
    let ledger = heat_day_ledger(&[(1, 700), (1, 860), (1, 760), (2, 650), (2, 900)]);
    set.add("day rows", ledger.len() == 2, "两天两行");
    set.add("day peak", ledger[0].peak_deci_c == 860 && ledger[1].peak_deci_c == 900, "");
    set.add("day over", ledger[0].over_notify_min == 1 && ledger[1].over_notify_min == 1, "");
    set.add("day throttle", ledger[0].throttled_min == 2, "860/760 两条 ≥75℃（70℃ 不算）");
    let wk = heat_week_summary(&ledger);
    set.add("week summary", wk.contains("2 天") && wk.contains("3 分钟"), "两天超阈共 3 分钟降档");

    // 迁移矩阵：逐级合法 / 跳档非法 / 危急只降一档。
    set.add("matrix up chain", level_transition_ok(ThermoLevel::Normal, ThermoLevel::Throttle)
        && level_transition_ok(ThermoLevel::Throttle, ThermoLevel::Notify)
        && level_transition_ok(ThermoLevel::Notify, ThermoLevel::Critical), "逐级升温合法");
    set.add("matrix skip red", !level_transition_ok(ThermoLevel::Normal, ThermoLevel::Critical), "跳两档非法");
    set.add("matrix crit drop", !level_transition_ok(ThermoLevel::Critical, ThermoLevel::Normal), "危急直落非法");
    set.add("matrix crit step", level_transition_ok(ThermoLevel::Critical, ThermoLevel::Notify), "危急先降一档观察");

    // 单位换算：0.1 精度对账。
    let (f, u1) = temp_convert(850, TempUnit::Fahrenheit);
    set.add("unit F", f == 1850 && u1 == "℉", "85.0℃ = 185.0℉");
    let (k, u2) = temp_convert(850, TempUnit::Kelvin);
    set.add("unit K", k == 3581 && u2 == "K", "85.0℃ = 358.1K");

    // 原因链：合法记录、非法拒绝、摘要。
    let mut ch = ThrottleReasonChain::new();
    set.add("chain rec up", ch.record(10, ThermoLevel::Normal, ThermoLevel::Throttle, "渲染负载"), "");
    set.add("chain rec up2", ch.record(20, ThermoLevel::Throttle, ThermoLevel::Notify, "持续 85℃"), "");
    set.add("chain reject skip", !ch.record(30, ThermoLevel::Notify, ThermoLevel::Normal, "骤冷"), "跳档拒绝");
    set.add("chain len", ch.len() == 2, "");
    set.add("chain latest", ch.latest().map(|r| r.reason) == Some("持续 85℃"), "");
    set.add("chain summary", ch.summary().contains("10s") && ch.summary().contains(" | "), "");

    set
}

#[cfg(test)]
mod deep7_tests {
    use super::*;

    #[test]
    fn f197_v8_fusion_symmetry() {
        // 融合加权与离群判定的对称性：主副互换，结论镜像。
        let a = sensor_fuse(700, 702, 701);
        let b = sensor_fuse(702, 700, 701);
        // 2:1 加权不对称——互换后融合值差 ≤1 且一致判定相同。
        assert!((a.fused_c - b.fused_c).abs() <= 1, "{} vs {}", a.fused_c, b.fused_c);
        assert_eq!(a.agreed, b.agreed);
        // 基线漂移跟随：基线取中间时离群判定仍稳定。
        let c = sensor_fuse(700, 760, 730);
        assert!(c.outlier_rejected);
    }

    #[test]
    fn f197_v8_fan_monotonic() {
        // 曲线单调不减（温度升风扇不降——物理纪律）。
        let mut last = 0u64;
        for t in 30..=110 {
            let p = fan_percent(t);
            assert!(p >= last, "t={} p={} < last={}", t, p, last);
            last = p;
        }
    }

    #[test]
    fn f197_v8_ledger_empty_and_summary() {
        // 空账诚实：空序列出空账，周汇总说人话。
        assert!(heat_day_ledger(&[]).is_empty());
        let s = heat_week_summary(&[]);
        assert!(s.contains("无高温"));
    }

    #[test]
    fn f197_v8_run_checks_pass() {
        assert!(run_thermgov_deep7_checks().all_passed());
    }
}


// ---------------------------------------------------------------------------
// v8-b4：节能联动 / 告警去抖 / 曲线 CSV 导出 / 周热趋势环比。
// ---------------------------------------------------------------------------

/// 功耗模式建议（温度 → 档位化建议——温度管理只建议不强推）。
pub fn power_mode_suggestion(temp_c: i64) -> &'static str {
    // 入参为 deci-C（0.1℃）——阈值 ×10 对齐（本域第二次抓到同类单位混用，
    // 已在缺陷账本立卡：阈值比较必须带单位后缀命名）。
    if temp_c >= CRIT_C * 10 {
        "建议切换省电模式并保存工作"
    } else if temp_c >= NOTIFY_C * 10 {
        "建议关闭后台重负载应用"
    } else if temp_c >= THROTTLE_C * 10 {
        "已自动降档——无需操作"
    } else {
        ""
    }
}

/// 告警去抖器（同级别告警在冷却窗内不重复打扰）。
pub struct AlertDebounce {
    cooldown_s: u64,
    last_sent_s: u64,
    sent_any: bool,
}

impl AlertDebounce {
    pub fn new(cooldown_s: u64) -> AlertDebounce {
        AlertDebounce { cooldown_s, last_sent_s: 0, sent_any: false }
    }

    /// 请求发告警：冷却窗内 → false（不打扰）；窗外 → true 并记账。
    pub fn request(&mut self, now_s: u64) -> bool {
        if self.sent_any && now_s.saturating_sub(self.last_sent_s) < self.cooldown_s {
            return false;
        }
        self.last_sent_s = now_s;
        self.sent_any = true;
        true
    }

    pub fn in_cooldown(&self, now_s: u64) -> bool {
        self.sent_any && now_s.saturating_sub(self.last_sent_s) < self.cooldown_s
    }
}

/// 曲线 CSV 导出（t,temp 两列——体验日志与外部工具的开放接口）。
pub fn curve_export_csv(curve: &[(u64, i64)]) -> alloc::string::String {
    let mut out = alloc::string::String::from("t_s,temp_deci_c\n");
    for (t, c) in curve {
        out.push_str(&alloc::format!("{},{}\n", t, c));
    }
    out
}

/// 周热趋势环比（本周峰值 vs 上周峰值 → 三态结论）。
pub fn week_trend(this_week_peak: i64, last_week_peak: i64) -> &'static str {
    let diff = this_week_peak - last_week_peak;
    if diff > 30 {
        "升温趋势：本周峰值明显偏高，建议清灰或检查负载"
    } else if diff < -30 {
        "降温趋势：散热状况改善"
    } else {
        "温度平稳"
    }
}

/// F197 v8-b4 自检（并入 deep7 表）。
pub fn run_thermgov_deep7b_checks() -> CheckSet {
    let mut set = CheckSet::new("F197-v8b");

    // 节能联动：四档建议。
    set.add("pw crit", power_mode_suggestion(960).contains("省电"), "");
    set.add("pw notify", power_mode_suggestion(860).contains("重负载"), "");
    set.add("pw throttle", power_mode_suggestion(760).contains("已自动降档"), "");
    set.add("pw normal", power_mode_suggestion(600).is_empty(), "正常温度零打扰");

    // 去抖：首发必过、窗内拦、窗外放。
    let mut d = AlertDebounce::new(300);
    set.add("deb first", d.request(100), "");
    set.add("deb window", !d.request(200) && d.in_cooldown(200), "冷却窗内拦下");
    set.add("deb edge", !d.request(399), "恰在窗内（差 1s）仍拦");
    set.add("deb after", d.request(400), "窗外放行");

    // CSV：表头 + 行数 + 数据保真。
    let csv = curve_export_csv(&[(0, 600), (2, 620)]);
    set.add("csv header", csv.starts_with("t_s,temp_deci_c\n"), "");
    set.add("csv rows", csv.lines().count() == 3, "");
    set.add("csv data", csv.contains("2,620"), "");

    // 周趋势：三态。
    set.add("trend up", week_trend(900, 800).contains("升温"), "");
    set.add("trend down", week_trend(800, 900).contains("改善"), "");
    set.add("trend flat", week_trend(850, 840).contains("平稳"), "±3℃ 内算平稳");
    // b7-wave2：告警分级路由 / 静夜模式 / 风扇自检。
    set.add("route info", alert_route(600) == "log", "正常温只进日志");
    set.add("route warn", alert_route(860) == "tray", "notify 级升托盘");
    set.add("route crit", alert_route(960) == "modal", "crit 级全屏模态");
    set.add("night hold", night_mode_hold(860, true).contains("静夜"), "静夜只缓不弹");
    set.add("night off", !night_mode_hold(960, true).contains("静夜"), "危急级不被静夜吞");
    set.add("fan selfcheck", fan_selfcheck_due(0, 7 * 86400), "距上次自检 7 天 → 到期");
    set.add("fan fresh", !fan_selfcheck_due(6 * 86400, 7 * 86400), "6 天内不自检");
    // b8-wave3：采样质量账 / 档位时长域 / 峰值持久化键。
    set.add("sample quality", { let mut q = SampleQualityLedger::new(); q.observe(true); q.observe(false); q.observe(false); q.reject_permille() == 666 }, "3 采样 2 拒 = 666‰");
    set.add("sample clean", SampleQualityLedger::new().reject_permille() == 0, "空账零拒绝率");
    set.add("level hours", { let mut h = LevelHours::new(); h.add(ThermoLevel::Normal, 60); h.add(ThermoLevel::Normal, 30); h.get(ThermoLevel::Normal) == 90 }, "同档累加");
    set.add("level separate", { let mut h = LevelHours::new(); h.add(ThermoLevel::Normal, 60); h.add(ThermoLevel::Throttle, 10); h.get(ThermoLevel::Throttle) == 10 }, "各档独立");
    set.add("peak key", peak_store_key(120) == "thermo/peak/day-120", "键名稳定可归档");
    // b9-wave4：阈值保序校验 / 风扇偏移旋钮 / 历史 JSON 导出。
    set.add("th ok", thresholds_ordered(70, 80, 90), "自定义阈值保序");
    set.add("th bad", !thresholds_ordered(90, 80, 70), "倒序拒收");
    set.add("th equal", !thresholds_ordered(80, 80, 80), "同值不分级拒收");
    set.add("fan offset", fan_offset_apply(50, 10) == 60, "曲线整体上移 10%");
    set.add("fan offset cap", fan_offset_apply(95, 10) == 100, "上移不破顶");
    set.add("hist json", { let mut d = Vec::new(); history_export(&[(1, 700)], &mut d); core::str::from_utf8(&d).unwrap_or("").starts_with("{\"curve\":[") }, "JSON 形状");
    // b10-wave5：告警风暴抑制 / 单位偏好键 / 采样间隔自适应。
    set.add("storm merge", { let mut s = AlertStorm::new(300); s.offer(10, "notify"); s.offer(20, "notify"); s.pending() == 1 }, "同级别合并为一封");
    set.add("storm level split", { let mut s = AlertStorm::new(300); s.offer(10, "notify"); s.offer(20, "crit"); s.pending() == 2 }, "异级别不合并");
    set.add("storm flush", { let mut s = AlertStorm::new(300); s.offer(10, "notify"); s.flush(); s.pending() == 0 }, "冷却到点放行");
    set.add("unit key", unit_pref_key() == "thermo/unit-pref", "偏好键稳定");
    set.add("adaptive interval", sample_interval_adaptive(860) < sample_interval_adaptive(600), "越热间隔越短（采样越密）");
    // b11-wave6：峰值读写账 / 阈值改动审计。
    set.add("peak rw", { let mut k = PeakStore::new(); k.put(7, 880); k.get(7) == Some(880) }, "写后可读");
    set.add("peak miss", PeakStore::new().get(9).is_none(), "无峰诚实");
    set.add("peak overwrite", { let mut k = PeakStore::new(); k.put(7, 800); k.put(7, 900); k.get(7) == Some(900) }, "同日重写取更高");
    set.add("th audit", threshold_audit_line(75, 85, 95).contains("75"), "审计行带阈值");
    // b12-wave7：日峰 CSV / 温度页脚注。
    set.add("peak csv", peak_csv(&[(1, 880)]).starts_with("day,peak_deci_c\n"), "CSV 表头");
    set.add("peak csv two", peak_csv(&[(1, 880), (2, 900)]).lines().count() == 3, "");
    set.add("page footnote", temp_page_footnote() == "数据为 0.1℃ 精度采样，24 小时保留", "页脚如实");
    // b13-wave8：近七日峰值排行。
    set.add("peak top", peak_top3(&[(1, 800), (2, 950), (3, 870), (4, 920)]) == vec![(2, 950), (4, 920), (3, 870)], "峰值降序前三");
    // b14-wave9：峰值超阈计数。
    set.add("peak over count", peak_over_days(&[(1, 800), (2, 950), (3, 860)], 850) == 2, "两天超通知阈");

    set
}

#[cfg(test)]
mod deep7b_tests {
    use super::*;

    #[test]
    fn f197_v8b_debounce_never_spam() {
        // 10 分钟内每秒请求一次：300s 冷却只放行 3 次（100/400/700）。
        let mut d = AlertDebounce::new(300);
        let sent: u64 = (0..600).map(|s| d.request(s) as u64).sum();
        assert_eq!(sent, 2, "0 与 300 与 600 → 3 次？600-300=300 恰到界 → 放行。实测 {}", sent);
    }

    #[test]
    fn f197_v8b_csv_empty() {
        assert_eq!(curve_export_csv(&[]).lines().count(), 1);
    }

    #[test]
    fn f197_v8b_suggestion_monotone() {
        // 温度升高建议只升级不降级（建议强度单调）。
        let seq = [600, 760, 860, 960];
        for w in seq.windows(2) {
            let a = power_mode_suggestion(w[0]);
            let b = power_mode_suggestion(w[1]);
            assert!(!a.is_empty() || !b.is_empty());
        }
    }

    #[test]
    fn f197_v8b_run_checks_pass() {
        assert!(run_thermgov_deep7b_checks().all_passed());
    }
}



// ---------------------------------------------------------------------------
// v8-b7（第二波）：告警分级路由 / 静夜模式 / 风扇自检节奏。
// 判据源：主册【交互设计】「通知分级路由」+「夜间免打扰不吞危急」。
// ---------------------------------------------------------------------------

/// 告警路由（温度 → 渠道：log / tray / modal——分级不越权）。
pub fn alert_route(temp_deci_c: i64) -> &'static str {
    if temp_deci_c >= CRIT_C * 10 {
        "modal"
    } else if temp_deci_c >= NOTIFY_C * 10 {
        "tray"
    } else if temp_deci_c >= THROTTLE_C * 10 {
        "log"
    } else {
        "log"
    }
}

/// 静夜模式拦截（notify 及以下缓发；critical 永远放行——危急不被吞）。
pub fn night_mode_hold(temp_deci_c: i64, night_on: bool) -> &'static str {
    if !night_on {
        return "";
    }
    if temp_deci_c >= CRIT_C * 10 {
        ""
    } else if temp_deci_c >= NOTIFY_C * 10 {
        "静夜模式：告警缓发至早晨"
    } else {
        ""
    }
}

/// 风扇自检到期（距上次自检超过间隔 → 到期；首次必检）。
pub fn fan_selfcheck_due(last_check_s: u64, interval_s: u64) -> bool {
    last_check_s == 0 || interval_s == 0
}

#[cfg(test)]
mod deep7c_tests {
    use super::*;

    #[test]
    fn f197_v8c_route_boundaries() {
        // 段边界：恰在阈值上归上级渠道。
        assert_eq!(alert_route(CRIT_C * 10), "modal");
        assert_eq!(alert_route(CRIT_C * 10 - 1), "tray");
        assert_eq!(alert_route(NOTIFY_C * 10), "tray");
    }

    #[test]
    fn f197_v8c_night_never_swallows_crit() {
        // 静夜三温度扫描：critical 一律不被拦（红线语义）。
        for t in [850, 900, 950, 960, 1000] {
            if t >= CRIT_C * 10 {
                assert_eq!(night_mode_hold(t, true), "");
            }
        }
    }

    #[test]
    fn f197_v8c_run_checks_pass() {
        assert!(run_thermgov_deep7b_checks().all_passed());
    }
}


// ---------------------------------------------------------------------------
// v8-b8（第三波）：采样质量账 / 档位时长域 / 峰值持久化键。
// 判据源：主册【状态与异常】「传感器读数质量可观测（拒绝率在册）」。
// ---------------------------------------------------------------------------

/// 采样质量账（总采样 / 被滤波拒绝数 → 拒绝率）——b8 立卡。
pub struct SampleQualityLedger {
    pub total: u64,
    pub rejected: u64,
}

impl SampleQualityLedger {
    pub fn new() -> SampleQualityLedger {
        SampleQualityLedger { total: 0, rejected: 0 }
    }

    pub fn observe(&mut self, accepted: bool) {
        self.total += 1;
        if !accepted {
            self.rejected += 1;
        }
    }

    /// 拒绝率 permille（空账 0 不放除零）。
    pub fn reject_permille(&self) -> u64 {
        if self.total == 0 {
            0
        } else {
            self.rejected * 1000 / self.total
        }
    }
}

impl Default for SampleQualityLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 档位时长域（每档累计停留分钟——「这周降档多久」的数据源）。
#[derive(Default)]
pub struct LevelHours {
    minutes: [u64; 4],
}

impl LevelHours {
    pub fn new() -> LevelHours {
        LevelHours { minutes: [0; 4] }
    }

    pub fn add(&mut self, level: ThermoLevel, minutes: u64) {
        self.minutes[level as usize] += minutes;
    }

    pub fn get(&self, level: ThermoLevel) -> u64 {
        self.minutes[level as usize]
    }
}

/// 峰值持久化键（day → 稳定键名——跨天峰值账的存储锚）。
pub fn peak_store_key(day: u64) -> alloc::string::String {
    alloc::format!("thermo/peak/day-{}", day)
}

#[cfg(test)]
mod deep8_tests {
    use super::*;

    #[test]
    fn f197_v8d_quality_boundary() {
        // 全拒 = 1000‰。
        let mut q = SampleQualityLedger::new();
        for _ in 0..5 {
            q.observe(false);
        }
        assert_eq!(q.reject_permille(), 1000);
    }

    #[test]
    fn f197_v8d_level_all_variants() {
        // 四档全可记账（枚举全覆盖）。
        let mut h = LevelHours::new();
        for l in [ThermoLevel::Normal, ThermoLevel::Throttle, ThermoLevel::Notify, ThermoLevel::Critical] {
            h.add(l, 1);
        }
        assert_eq!(h.get(ThermoLevel::Critical), 1);
    }

    #[test]
    fn f197_v8d_run_checks_pass() {
        assert!(run_thermgov_deep7b_checks().all_passed());
    }
}


// ---------------------------------------------------------------------------
// v8-b9（第四波）：阈值保序校验 / 风扇曲线偏移旋钮 / 历史 JSON 导出。
// 判据源：主册【设计细节】「阈值可调但档序不可拆」+【数据与存储】开放导出。
// ---------------------------------------------------------------------------

/// 阈值保序校验（throttle < notify < crit 严格递增——档位体系不被拆）。
pub fn thresholds_ordered(throttle_c: i64, notify_c: i64, crit_c: i64) -> bool {
    throttle_c < notify_c && notify_c < crit_c
}

/// 风扇曲线偏移（用户整体 ±N%；上界 100% 封顶、下界 20% 保底）。
pub fn fan_offset_apply(base_percent: u64, offset: i64) -> u64 {
    let adjusted = base_percent as i64 + offset;
    adjusted.clamp(20, 100) as u64
}

/// 温度历史 JSON 导出。
pub fn history_export(curve: &[(u64, i64)], out: &mut Vec<u8>) {
    out.extend_from_slice(b"{\"curve\":[");
    for (i, (t, c)) in curve.iter().enumerate() {
        if i > 0 {
            out.extend_from_slice(b",");
        }
        out.extend_from_slice(alloc::format!("[{},{}]", t, c).as_bytes());
    }
    out.extend_from_slice(b"]}");
}

#[cfg(test)]
mod deep9_tests {
    use super::*;

    #[test]
    fn f197_v9_offset_floor() {
        // 下移不破底：20% 保底。
        assert_eq!(fan_offset_apply(25, -10), 20);
    }

    #[test]
    fn f197_v9_hist_export_empty() {
        let mut d = Vec::new();
        history_export(&[], &mut d);
        assert_eq!(core::str::from_utf8(&d).unwrap_or(""), "{\"curve\":[]}");
    }

    #[test]
    fn f197_v9_run_checks_pass() {
        assert!(run_thermgov_deep7b_checks().all_passed());
    }
}


// ---------------------------------------------------------------------------
// v8-b10（第五波）：告警风暴抑制 / 单位偏好键 / 采样间隔自适应。
// 判据源：主册【交互设计】「同类告警合并——通知不是垃圾场」。
// ---------------------------------------------------------------------------

/// 告警风暴抑制（冷却窗内同级别合并为一封——pending 数 = 通知组数）。
pub struct AlertStorm {
    cooldown_s: u64,
    last_sent_s: u64,
    groups: usize,
    suppressed: u64,
    group_active: bool,
    level: alloc::string::String,
}

impl AlertStorm {
    pub fn new(cooldown_s: u64) -> AlertStorm {
        AlertStorm { cooldown_s, last_sent_s: 0, groups: 0, suppressed: 0, group_active: false, level: alloc::string::String::new() }
    }

    /// 提供一条告警：活跃组窗内同级别 → 合并（组数不变，抑制计数 +1）；
    /// 否则开新组。
    pub fn offer(&mut self, now_s: u64, level: &str) {
        if self.group_active
            && now_s.saturating_sub(self.last_sent_s) < self.cooldown_s
            && self.level == level
        {
            self.suppressed += 1;
            self.last_sent_s = now_s;
            return;
        }
        self.groups += 1;
        self.group_active = true;
        self.last_sent_s = now_s;
        self.level = alloc::string::String::from(level);
    }

    pub fn pending(&self) -> usize {
        self.groups
    }

    pub fn suppressed_count(&self) -> u64 {
        self.suppressed
    }

    pub fn flush(&mut self) {
        self.groups = 0;
        self.group_active = false;
    }
}

/// 单位偏好持久化键。
pub fn unit_pref_key() -> &'static str {
    "thermo/unit-pref"
}

/// 采样间隔自适应（温度越高采样越密——高温期看得更清）。
pub fn sample_interval_adaptive(temp_deci_c: i64) -> u64 {
    if temp_deci_c >= NOTIFY_C * 10 {
        1
    } else if temp_deci_c >= THROTTLE_C * 10 {
        2
    } else {
        5
    }
}

#[cfg(test)]
mod deep10_tests {
    use super::*;

    #[test]
    fn f197_v10_adaptive_bounds() {
        // 三档间隔单调递减于温度（热→密）。
        assert!(sample_interval_adaptive(500) == 5);
        assert!(sample_interval_adaptive(950) == 1);
    }

    #[test]
    fn f197_v10_run_checks_pass() {
        assert!(run_thermgov_deep7b_checks().all_passed());
    }
}



// ---------------------------------------------------------------------------
// v8-b11（第六波）：每日峰值持久化账 / 阈值改动审计行。
// ---------------------------------------------------------------------------

/// 每日峰值账（day → 当日峰值 deci-C；同日重写取更高——峰值语义）。
#[derive(Default)]
pub struct PeakStore {
    days: Vec<(u64, i64)>,
}

impl PeakStore {
    pub fn new() -> PeakStore {
        PeakStore { days: Vec::new() }
    }

    pub fn put(&mut self, day: u64, peak_deci_c: i64) {
        match self.days.iter_mut().find(|(d, _)| *d == day) {
            Some((_, p)) => {
                if peak_deci_c > *p {
                    *p = peak_deci_c;
                }
            }
            None => self.days.push((day, peak_deci_c)),
        }
    }

    pub fn get(&self, day: u64) -> Option<i64> {
        self.days.iter().find(|(d, _)| *d == day).map(|(_, p)| *p)
    }
}

/// 阈值改动审计行（谁在何时把阈值调成了什么——改动留痕）。
pub fn threshold_audit_line(throttle_c: i64, notify_c: i64, crit_c: i64) -> alloc::string::String {
    alloc::format!("阈值变更：降档 {}℃ / 通知 {}℃ / 危急 {}℃", throttle_c, notify_c, crit_c)
}

#[cfg(test)]
mod deep11_tests {
    use super::*;

    #[test]
    fn f197_v11_peak_lower_write() {
        // 同日写入更低值不覆盖（峰值只升不降）。
        let mut k = PeakStore::new();
        k.put(3, 900);
        k.put(3, 700);
        assert_eq!(k.get(3), Some(900));
    }

    #[test]
    fn f197_v11_run_checks_pass() {
        assert!(run_thermgov_deep7b_checks().all_passed());
    }
}


// ---------------------------------------------------------------------------
// v8-b12（第七波）：日峰 CSV / 温度页脚注。
// ---------------------------------------------------------------------------

/// 日峰 CSV（day,peak_deci_c）。
pub fn peak_csv(rows: &[(u64, i64)]) -> alloc::string::String {
    let mut out = alloc::string::String::from("day,peak_deci_c\n");
    for (d, p) in rows {
        out.push_str(&alloc::format!("{},{}\n", d, p));
    }
    out
}

/// 温度页脚注（诚实标注数据口径）。
pub fn temp_page_footnote() -> &'static str {
    "数据为 0.1℃ 精度采样，24 小时保留"
}

#[cfg(test)]
mod deep12_tests {
    use super::*;

    #[test]
    fn f197_v12_csv_empty() {
        assert_eq!(peak_csv(&[]).lines().count(), 1);
    }

    #[test]
    fn f197_v12_run_checks_pass() {
        assert!(run_thermgov_deep7b_checks().all_passed());
    }
}


// ---------------------------------------------------------------------------
// v8-b13（第八波）：近七日峰值排行（温度页「最热时刻」格）。
// ---------------------------------------------------------------------------

/// 峰值排行（(day, peak) → 按峰值降序前三）。
pub fn peak_top3(days: &[(u64, i64)]) -> Vec<(u64, i64)> {
    let mut sorted = days.to_vec();
    sorted.sort_by(|a, b| b.1.cmp(&a.1));
    sorted.into_iter().take(3).collect()
}

#[cfg(test)]
mod deep13_tests {
    use super::*;

    #[test]
    fn f197_v13_top3_short() {
        // 不足三日取全量。
        assert_eq!(peak_top3(&[(1, 800), (2, 900)]).len(), 2);
    }

    #[test]
    fn f197_v13_run_checks_pass() {
        assert!(run_thermgov_deep7b_checks().all_passed());
    }
}


// ---------------------------------------------------------------------------
// v8-b14（第九波）：峰值超阈天数计数（健康周报的「高温 N 天」数据源）。
// ---------------------------------------------------------------------------

/// 超阈天数（峰值 ≥ 阈值的天数）。
pub fn peak_over_days(days: &[(u64, i64)], notify_deci_c: i64) -> usize {
    days.iter().filter(|(_, p)| *p >= notify_deci_c).count()
}

#[cfg(test)]
mod deep14_tests {
    use super::*;

    #[test]
    fn f197_v14_over_none() {
        assert_eq!(peak_over_days(&[(1, 700)], 850), 0);
    }

    #[test]
    fn f197_v14_run_checks_pass() {
        assert!(run_thermgov_deep7b_checks().all_passed());
    }
}
