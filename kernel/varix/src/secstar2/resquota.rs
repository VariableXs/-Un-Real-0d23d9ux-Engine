//! F195 资源配额执行（secstar2 · G-G-25）——处理失控应用像教练处理犯规：吹哨罚下，不动拳头。
//!
//! **判据（主册）**：泄漏样本实测：四级阶梯逐级触发顺序正确；桌面帧率全程
//! 不掉（F041）；豁免清单零误伤。
//!
//! **功能定义（主册 G-G-25）**：每应用配额三维：内存（工作集上限）/IO（带宽
//! 权重）/进程数——超限降级不终止（后台应用优先降）；配额面板可查；失控
//! 应用掐流量不掐命。
//!
//! 【交互设计】通知单条说明（应用/资源/动作三要素）；设置中心「应用-资源」
//! 页：每应用三维当前用量+配额线可视化（仪表条）；配额线系统默认（按机型
//! 档），用户可对单应用收紧（放宽需 F038 式确认）。
//! 【数据与存储】配额表配置层；用量实时自账本（F045/F057/F060 数据源同源）。
//! 【状态与异常】降级阶梯（缓存回收→IO 降权→增长冻结→硬顶拒绝）逐级文档
//! 化；应用被硬顶后自我放弃（OOM 语义）→ 走 F020 正常崩溃流程（系统不背锅
//! 不补刀）；关键系统进程豁免清单固定。
//! 【设计细节】默认配额按内存档推导（4GB 机型：单应用硬顶 1.5GB/软顶
//! 1.2GB——旋钮表）；IO 权重 1-10（映射 F057 三级内细分）；进程数上限
//! 128/应用；仪表条绿黄红三段（配额的 60%/85%/100%）；通知合并（同应用
//! 5min 窗）。
//!
//! 阶梯思路参照 Linux cgroup 压力反馈与 Android OOM adj（简化版）；实现自研。
//! 依赖锚点：F020（崩溃流程）、F038（确认语义）、F041（帧率对账）、F057（IO 映射）、F060（用量同源）。

use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 常量（主册数值，一处一事实）
// ---------------------------------------------------------------------------

/// 应用数上限（配额表容量）。
pub const APP_CAP: usize = 64;

/// 默认内存硬顶：1.5GB（4GB 机型档——旋钮表）。
pub const MEM_HARD_DEFAULT: u64 = 1_500 * 1024 * 1024;
/// 默认内存软顶：1.2GB。
pub const MEM_SOFT_DEFAULT: u64 = 1_200 * 1024 * 1024;
/// IO 权重界：1-10。
pub const IO_WEIGHT_MIN: u32 = 1;
pub const IO_WEIGHT_MAX: u32 = 10;
/// 进程数上限：128/应用。
pub const PROC_CAP: u32 = 128;
/// 仪表条三段位：绿 60% / 黄 85% / 红 100%。
pub const GAUGE_GREEN_PERMILLE: u64 = 600;
pub const GAUGE_YELLOW_PERMILLE: u64 = 850;
/// 通知合并窗口：5 分钟（同应用同资源）。
pub const NOTIFY_MERGE_MIN: u64 = 5;

/// 降级阶梯四级（主册【状态与异常】顺序——逐级文档化）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Ladder {
    /// L1 缓存回收（无感）。
    CacheReclaim = 1,
    /// L2 IO 降权（后台优先降）。
    IoDeprioritize = 2,
    /// L3 增长冻结（分配冻结在软顶）。
    GrowthFreeze = 3,
    /// L4 硬顶拒绝（分配失败——OOM 语义入口）。
    HardRefuse = 4,
}

/// 配额维度。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dim {
    Mem,
    Io,
    Proc,
}

impl Dim {
    pub fn name(self) -> &'static str {
        match self {
            Dim::Mem => "内存",
            Dim::Io => "IO",
            Dim::Proc => "进程数",
        }
    }
}

// ---------------------------------------------------------------------------
// 豁免清单
// ---------------------------------------------------------------------------

/// 关键系统进程豁免清单（固定——零误伤判据的依据源）。
pub const EXEMPT: [&str; 6] = [
    "varix-kernel",
    "compositor",
    "input-svc",
    "power-svc",
    "security-svc",
    "log-svc",
];

pub fn is_exempt(app: &str) -> bool {
    EXEMPT.contains(&app)
}

// ---------------------------------------------------------------------------
// 配额与用量
// ---------------------------------------------------------------------------

/// 单应用配额（三维）。
#[derive(Clone, Copy, Debug)]
pub struct Quota {
    /// 内存硬顶（字节）。
    pub mem_hard: u64,
    /// 内存软顶（字节）。
    pub mem_soft: u64,
    /// IO 权重（1-10；F057 三级内细分）。
    pub io_weight: u32,
    /// 进程数上限。
    pub proc_cap: u32,
}

impl Quota {
    /// 4GB 机型档默认（主册推导）。
    pub fn default_4g() -> Quota {
        Quota { mem_hard: MEM_HARD_DEFAULT, mem_soft: MEM_SOFT_DEFAULT, io_weight: 5, proc_cap: PROC_CAP }
    }

    /// 校正：软顶不得超硬顶（配置层防呆——钳制语义）。
    pub fn sanitized(mut self) -> Quota {
        if self.mem_soft > self.mem_hard {
            self.mem_soft = self.mem_hard;
        }
        self.io_weight = self.io_weight.clamp(IO_WEIGHT_MIN, IO_WEIGHT_MAX);
        self
    }
}

/// 用量快照（实时自账本注入）。
#[derive(Clone, Copy, Debug, Default)]
pub struct Usage {
    pub mem_bytes: u64,
    /// 后台标记（后台应用优先降——阶梯的次序输入）。
    pub background: bool,
    pub proc_count: u32,
}

/// 单条降级动作记录（通知三要素+仪表条的数据源）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QuotaAction {
    pub app: &'static str,
    pub dim: Dim,
    pub level: Ladder,
    /// 三要素文案（发生了什么/为什么/下一步——通知单条说明）。
    pub text: &'static str,
}

/// 仪表条段位（绿黄红——配额的 60%/85%/100%）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GaugeZone {
    Green,
    Yellow,
    Red,
}

pub fn gauge_zone(used: u64, limit: u64) -> GaugeZone {
    if limit == 0 {
        return GaugeZone::Red;
    }
    let permille = used.saturating_mul(1000) / limit;
    if permille >= GAUGE_YELLOW_PERMILLE {
        GaugeZone::Red
    } else if permille >= GAUGE_GREEN_PERMILLE {
        GaugeZone::Yellow
    } else {
        GaugeZone::Green
    }
}

// ---------------------------------------------------------------------------
// 配额执行主体
// ---------------------------------------------------------------------------

/// 配额执行器。
pub struct QuotaEnforcer {
    /// 应用名 → 配额（收紧项——未列应用用默认档）。
    quotas: Vec<(&'static str, Quota)>,
    /// 已触达的最高阶梯（每应用——阶梯顺序对账）。
    reached: Vec<(&'static str, Ladder)>,
    /// 动作流水（审计面——每次阶梯跨越全记，配额面板回放用）。
    actions: Vec<QuotaAction>,
    /// 通知流（通知中心面——同应用 5min 窗合并，只记窗内首条）。
    notify_log: Vec<QuotaAction>,
    /// 默认配额（机型档）。
    pub default_quota: Quota,
    /// 通知合并窗（app + 窗首分钟戳）。
    last_notify_min: Vec<(&'static str, u64)>,
    /// 豁免误伤计数（恒 0 才是绿）。
    pub exempt_violations: u64,
    /// 硬顶拒绝计数（OOM 语义入口——F020 崩溃流程由调用方衔接）。
    pub hard_refusals: u64,
}

impl QuotaEnforcer {
    pub fn new() -> QuotaEnforcer {
        QuotaEnforcer {
            quotas: Vec::new(),
            reached: Vec::new(),
            actions: Vec::new(),
            notify_log: Vec::new(),
            default_quota: Quota::default_4g(),
            last_notify_min: Vec::new(),
            exempt_violations: 0,
            hard_refusals: 0,
        }
    }

    /// 用户收紧单应用配额（放宽需 F038 式确认——本 API 只收不放；
    /// 放宽走显式 confirm 入口 `relax`）。
    pub fn tighten(&mut self, app: &'static str, q: Quota) {
        let q = q.sanitized();
        if let Some(pos) = self.quotas.iter().position(|(a, _)| *a == app) {
            self.quotas[pos].1 = q;
        } else {
            self.quotas.push((app, q));
        }
    }

    /// 放宽（F038 式确认入口——`confirmed=false` 拒绝，零静默）。
    pub fn relax(&mut self, app: &'static str, q: Quota, confirmed: bool) -> Result<(), &'static str> {
        if !confirmed {
            return Err("放宽配额需 F038 式确认（危险操作语义）");
        }
        self.tighten(app, q);
        Ok(())
    }

    fn quota_of(&self, app: &str) -> Quota {
        self.quotas.iter().find(|(a, _)| *a == app).map(|(_, q)| *q).unwrap_or(self.default_quota)
    }

    /// 阶梯推进登记（逐级触发——跳级视为缺陷，返回 false）。
    fn advance(&mut self, app: &'static str, level: Ladder) -> bool {
        let cur = self.reached.iter().find(|(a, _)| *a == app).map(|(_, l)| *l);
        let ok = match cur {
            None => level == Ladder::CacheReclaim,
            Some(c) => level as u32 == c as u32 + 1,
        };
        if ok {
            if let Some(pos) = self.reached.iter().position(|(a, _)| *a == app) {
                self.reached[pos].1 = level;
            } else {
                self.reached.push((app, level));
            }
        }
        ok
    }

    /// 应用阶梯复位（内存压力解除/应用重启——L1 重新开始）。
    pub fn reset_ladder(&mut self, app: &'static str) {
        self.reached.retain(|(a, _)| *a != app);
    }

    /// **内存增长申报**（判据一核心）：按四级阶梯逐级触发，返回动作列表。
    ///
    /// 顺序断言（阶梯语义）：软顶 85%→L1、软顶 100%→L2、硬顶 90%→L3、
    /// 硬顶 100%→L4；后台应用在同用量下阶梯前移（优先降）。
    pub fn on_mem_growth(&mut self, app: &'static str, usage: Usage, now_min: u64) -> Vec<QuotaAction> {
        // 豁免清单零误伤：豁免应用不进阶梯。
        if is_exempt(app) {
            return Vec::new();
        }
        let q = self.quota_of(app);
        let mut out = Vec::new();

        // 触发线（字节）：L1/L2/L3 对后台应用 ×0.8（优先降——同用量先触发）；
        // L4 硬顶不偏置（硬顶就是硬顶——安全边界不因前后台松动）。
        let bias = if usage.background { 800 } else { 1000 };
        let line_l1 = q.mem_soft * GAUGE_YELLOW_PERMILLE / 1000 * bias / 1000;
        let line_l2 = q.mem_soft * bias / 1000;
        let line_l3 = q.mem_hard * 900 / 1000 * bias / 1000;
        let line_l4 = q.mem_hard;

        let steps = [
            (Ladder::CacheReclaim, line_l1, "缓存已被回收（内存配额）", Dim::Mem),
            (Ladder::IoDeprioritize, line_l2, "IO 已降权（内存配额）", Dim::Mem),
            (Ladder::GrowthFreeze, line_l3, "内存增长已冻结（接近硬顶）", Dim::Mem),
            (Ladder::HardRefuse, line_l4, "分配已被拒绝（内存配额硬顶）", Dim::Mem),
        ];
        for (level, line, text, dim) in steps {
            if usage.mem_bytes >= line && self.advance(app, level) {
                if level == Ladder::HardRefuse {
                    self.hard_refusals += 1;
                }
                let act = QuotaAction { app, dim, level, text };
                // 动作全记（审计零静默）；通知按应用 5min 窗合并。
                self.actions.push(act);
                if !self.merged(app, now_min) {
                    self.notify_log.push(act);
                    self.last_notify_min.retain(|(a, _)| *a != app);
                    self.last_notify_min.push((app, now_min));
                }
                out.push(act);
            }
        }
        out
    }

    /// 通知合并（同应用 5min 窗——窗内不重复通知，动作照记）。
    fn merged(&self, app: &str, now_min: u64) -> bool {
        self.last_notify_min
            .iter()
            .any(|(a, t)| *a == app && now_min.saturating_sub(*t) < NOTIFY_MERGE_MIN)
    }

    /// **IO 权重申报**：越界钳制（1-10——F057 三级内细分）。
    pub fn io_weight(&self, app: &str) -> u32 {
        self.quota_of(app).io_weight
    }

    /// **进程数申报**：超 128 拒绝 fork（不终止已有进程——掐增量不掐存量）。
    pub fn proc_spawn_allowed(&mut self, app: &'static str, usage: Usage) -> bool {
        if is_exempt(app) {
            return true;
        }
        let q = self.quota_of(app);
        usage.proc_count < q.proc_cap
    }

    /// 动作流水（配额面板回放——全量阶梯跨越）。
    pub fn action_log(&self) -> &[QuotaAction] {
        &self.actions
    }

    /// 通知流（通知中心——合并后）。
    pub fn notification_log(&self) -> &[QuotaAction] {
        &self.notify_log
    }

    /// 应用当前触达阶梯（面板仪表条叠加显示）。
    pub fn ladder_of(&self, app: &str) -> Option<Ladder> {
        self.reached.iter().find(|(a, _)| *a == app).map(|(_, l)| *l)
    }

    /// 面板行：三维用量+配额线（设置中心「应用-资源」页数据源）。
    pub fn panel_row(&self, app: &'static str, usage: Usage) -> PanelRow {
        let q = self.quota_of(app);
        PanelRow {
            app,
            usage,
            quota: q,
            mem_zone: gauge_zone(usage.mem_bytes, q.mem_hard),
            ladder: self.ladder_of(app),
        }
    }
}

impl Default for QuotaEnforcer {
    fn default() -> Self {
        Self::new()
    }
}

/// 面板行。
#[derive(Clone, Copy, Debug)]
pub struct PanelRow {
    pub app: &'static str,
    pub usage: Usage,
    pub quota: Quota,
    pub mem_zone: GaugeZone,
    pub ladder: Option<Ladder>,
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F195 自检（聚合进 secstar2 域）。
pub fn run_resquota_checks() -> CheckSet {
    let mut set = CheckSet::new("F195-resquota");

    // 判据一：泄漏样本——四级阶梯逐级触发顺序正确。
    let mut e = QuotaEnforcer::new();
    let soft = MEM_SOFT_DEFAULT;
    let hard = MEM_HARD_DEFAULT;
    let mut u = Usage { mem_bytes: 0, background: false, proc_count: 0 };
    // L1：软顶 85%。
    u.mem_bytes = soft * 850 / 1000;
    let a1 = e.on_mem_growth("leaky-app", u, 0);
    set.add("l1 fires", a1.len() == 1 && a1[0].level == Ladder::CacheReclaim, "");
    // L2：软顶 100%。
    u.mem_bytes = soft;
    let a2 = e.on_mem_growth("leaky-app", u, 10);
    set.add("l2 fires", a2.len() == 1 && a2[0].level == Ladder::IoDeprioritize, "");
    // L3：硬顶 90%。
    u.mem_bytes = hard * 900 / 1000;
    let a3 = e.on_mem_growth("leaky-app", u, 20);
    set.add("l3 fires", a3.len() == 1 && a3[0].level == Ladder::GrowthFreeze, "");
    // L4：硬顶 100%。
    u.mem_bytes = hard;
    let a4 = e.on_mem_growth("leaky-app", u, 30);
    set.add("l4 fires", a4.len() == 1 && a4[0].level == Ladder::HardRefuse, "");
    set.add("hard counted", e.hard_refusals == 1, "");
    set.add("ladder order", e.ladder_of("leaky-app") == Some(Ladder::HardRefuse), "");

    // 阶梯顺序的跳级保护：顶格（L4）后再观察 → 零新动作（不重复触发）。
    let mut e2 = QuotaEnforcer::new();
    u.mem_bytes = hard;
    let walk = e2.on_mem_growth("x", u, 0);
    set.add("ladder walk 4 steps", walk.len() == 4, "");
    set.add("no refire at top", e2.on_mem_growth("x", u, 100).is_empty(), "");

    // 后台优先降：同用量触线更早（85% 软顶用量：前台只 L1，后台 L1+L2）。
    let mut e3 = QuotaEnforcer::new();
    u.mem_bytes = soft * 850 / 1000;
    let fg = e3.on_mem_growth("fg-app", u, 0).len();
    let bg = Usage { background: true, ..u };
    set.add("background biased", e3.on_mem_growth("bg-app", bg, 0).len() > fg, "");

    // 判据三：豁免清单零误伤。
    let mut e4 = QuotaEnforcer::new();
    u.mem_bytes = u64::MAX / 2;
    for &app in EXEMPT.iter() {
        let acts = e4.on_mem_growth(app, u, 0);
        if !acts.is_empty() {
            e4.exempt_violations += 1;
        }
    }
    set.add("exempt zero hit", e4.exempt_violations == 0, "");
    set.add("exempt proc ok", e4.proc_spawn_allowed("varix-kernel", Usage { proc_count: 9999, ..u }), "");

    // 进程数上限：128 拒增量不掐存量。
    let mut e5 = QuotaEnforcer::new();
    let mut full = Usage::default();
    full.proc_count = PROC_CAP;
    set.add("proc cap refuse", !e5.proc_spawn_allowed("forky", full), "");
    full.proc_count = PROC_CAP - 1;
    set.add("proc cap allow", e5.proc_spawn_allowed("forky", full), "");

    // 通知合并：动作全记、通知按应用 5min 窗去重。
    let mut e6 = QuotaEnforcer::new();
    u.mem_bytes = soft;
    // 一次申报跨 L1+L2：动作两条，通知只第一条（同窗合并）。
    let _ = e6.on_mem_growth("m", u, 0);
    set.add("actions all logged", e6.action_log().len() == 2, "");
    set.add("notify merged", e6.notification_log().len() == 1, "");
    // 窗内复检无新跨越 → 通知不增加。
    let _ = e6.on_mem_growth("m", u, 3);
    set.add("no refire no notify", e6.notification_log().len() == 1, "");
    // 窗外（5min 后）新泄漏周期：复位阶梯后再跨越 → 新通知。
    e6.reset_ladder("m");
    let _ = e6.on_mem_growth("m", u, 6);
    set.add("notify after window", e6.notification_log().len() == 2, "");

    // 配额面板行：仪表条三段。
    let e7 = QuotaEnforcer::new();
    set.add("gauge green", gauge_zone(hard / 2, hard) == GaugeZone::Green, "");
    set.add("gauge yellow", gauge_zone(hard * 700 / 1000, hard) == GaugeZone::Yellow, "");
    set.add("gauge red", gauge_zone(hard, hard) == GaugeZone::Red, "");
    let row = e7.panel_row("app", Usage { mem_bytes: hard / 2, ..Usage::default() });
    set.add("panel row", row.mem_zone == GaugeZone::Green, "");

    // 收紧/放宽（F038 确认语义）。
    let mut e8 = QuotaEnforcer::new();
    e8.tighten("t", Quota { mem_hard: 1000, mem_soft: 900, io_weight: 3, proc_cap: 10 });
    set.add("tighten eff", e8.io_weight("t") == 3, "");
    set.add("relax needs confirm", e8.relax("t", Quota::default_4g(), false).is_err(), "");
    set.add("relax confirmed ok", e8.relax("t", Quota::default_4g(), true).is_ok(), "");
    // 软顶>硬顶防呆。
    e8.tighten("t2", Quota { mem_hard: 100, mem_soft: 900, io_weight: 1, proc_cap: 1 });
    let q = e8.panel_row("t2", Usage::default()).quota;
    set.add("sanitize", q.mem_soft <= q.mem_hard, "");

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f195_ladder_reset_after_relief() {
        let mut e = QuotaEnforcer::new();
        let mut u = Usage { mem_bytes: MEM_SOFT_DEFAULT * 850 / 1000, ..Usage::default() };
        assert!(!e.on_mem_growth("app", u, 0).is_empty());
        e.reset_ladder("app");
        assert_eq!(e.ladder_of("app"), None);
        // 复位后 L1 可再次触发（新泄漏周期）。
        assert!(!e.on_mem_growth("app", u, 100).is_empty());
    }

    #[test]
    fn f195_background_never_terminates() {
        // 阶梯顶格 = HardRefuse（拒绝分配），不存在「终止应用」动作。
        let mut e = QuotaEnforcer::new();
        let u = Usage { mem_bytes: MEM_HARD_DEFAULT * 2, background: true, proc_count: 0 };
        let acts = e.on_mem_growth("bg", u, 0);
        assert!(acts.iter().all(|a| a.level != Ladder::HardRefuse || a.text.contains("拒绝")));
        assert!(acts.iter().any(|a| a.level == Ladder::HardRefuse));
        // 四级全部触发（后台偏置下同用量走完全程）。
        assert_eq!(acts.len(), 4);
    }

    #[test]
    fn f195_notify_three_element_text() {
        let mut e = QuotaEnforcer::new();
        let u = Usage { mem_bytes: MEM_SOFT_DEFAULT, ..Usage::default() };
        let acts = e.on_mem_growth("a", u, 0);
        // 通知三要素：应用名在 app 字段、资源在 dim 字段、动作在 text 字段。
        assert!(acts.iter().all(|a| !a.text.is_empty() && a.dim == Dim::Mem));
    }

    #[test]
    fn f195_io_weight_clamped() {
        let mut e = QuotaEnforcer::new();
        e.tighten("w", Quota { mem_hard: 1, mem_soft: 1, io_weight: 99, proc_cap: 1 });
        assert_eq!(e.io_weight("w"), IO_WEIGHT_MAX, "99 clamps to 10");
        e.tighten("w2", Quota { mem_hard: 1, mem_soft: 1, io_weight: 0, proc_cap: 1 });
        assert_eq!(e.io_weight("w2"), IO_WEIGHT_MIN, "0 clamps to 1");
    }

    #[test]
    fn f195_default_4g_profile() {
        let q = Quota::default_4g();
        assert_eq!(q.mem_hard, 1_500 * 1024 * 1024);
        assert_eq!(q.mem_soft, 1_200 * 1024 * 1024);
        assert_eq!(q.proc_cap, 128);
    }

    #[test]
    fn f195_run_checks_pass() {
        assert!(run_resquota_checks().all_passed());
    }
}
