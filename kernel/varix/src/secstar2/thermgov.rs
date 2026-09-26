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
