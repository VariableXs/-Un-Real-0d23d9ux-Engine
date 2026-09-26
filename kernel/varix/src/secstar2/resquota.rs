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
use alloc::string::String;
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
        let u = Usage { mem_bytes: MEM_SOFT_DEFAULT * 850 / 1000, ..Usage::default() };
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

// ---------------------------------------------------------------------------
// 深化子系统（回炉补深化 2026-09-26 · 主册细节条款全展开）——六个真功能面。
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// 深一：ProfileTable —— 机型档配额推导（主册【设计细节】：默认配额按内存
// 档推导——4GB 机型单应用硬顶 1.5GB/软顶 1.2GB 是档位表的一行，不是全部）
// ---------------------------------------------------------------------------

/// 机型内存档 → 默认配额（档位表——旋钮语义：表格在册公开，变更走 ADR）。
/// 推导规则：硬顶 = 机型内存 × 37.5%，软顶 = 硬顶 × 80%。字节口径对齐主册：
/// 「GB」= 1000 MiB（4GB 机型 → 硬顶 1500 MiB = 主册 1.5GB 基准逐字节一致）。
pub fn quota_for_machine(mem_gib: u64) -> Quota {
    let mem_mib = mem_gib * 1000;
    let hard_mib = mem_mib * 375 / 1000;
    let soft_mib = hard_mib * 800 / 1000;
    let mib = 1024u64 * 1024;
    Quota { mem_hard: hard_mib * mib, mem_soft: soft_mib * mib, io_weight: 5, proc_cap: PROC_CAP }.sanitized()
}

/// 档位表公开行（设置中心「应用-资源」页默认档说明的数据源）。
pub fn profile_rows() -> [(u64, u64, u64); 4] {
    // (机型内存 GiB, 硬顶字节, 软顶字节)
    [
        (4, MEM_HARD_DEFAULT, MEM_SOFT_DEFAULT),
        (8, 3_000 * 1024 * 1024, 2_400 * 1024 * 1024),
        (16, 6_000 * 1024 * 1024, 4_800 * 1024 * 1024),
        (32, 12_000 * 1024 * 1024, 9_600 * 1024 * 1024),
    ]
}

/// 档位表自洽：推导函数与公开行逐档一致（表不是手抄是推导——一处一事实）。
pub fn profile_table_consistent() -> bool {
    profile_rows().iter().all(|(g, hard, soft)| {
        let q = quota_for_machine(*g);
        q.mem_hard == *hard && q.mem_soft == *soft
    })
}

// ---------------------------------------------------------------------------
// 深二：NotifyPayload —— 通知单条组装（三要素 + 下一步——主册【交互设计】
// 「通知单条说明（应用/资源/动作三要素）」的完整版式）
// ---------------------------------------------------------------------------

/// 阶梯 → 下一步建议（错误呈现三要素的「下一步怎么办」——每级不同）。
pub fn next_step(level: Ladder) -> &'static str {
    match level {
        Ladder::CacheReclaim => "无需操作（缓存会自动回填）",
        Ladder::IoDeprioritize => "若该应用需要流畅 IO，可在设置-应用-资源中查看",
        Ladder::GrowthFreeze => "建议保存工作；应用已无法继续增长内存",
        Ladder::HardRefuse => "应用可能无法继续运行；请保存工作或重启该应用",
    }
}

/// 通知正文（`应用 · 资源 · 动作 · 下一步` 四段人话——零堆不可行处由调用
/// 方拼装，本函数给出定长字段序）。
pub fn notify_fields(act: &QuotaAction) -> (&'static str, &'static str, &'static str, &'static str) {
    (act.app, act.dim.name(), act.text, next_step(act.level))
}

// ---------------------------------------------------------------------------
// 深三：OomHandoff —— 硬顶后自我放弃的交接（主册【状态与异常】：应用被
// 硬顶后自我放弃 → 走 F020 正常崩溃流程——系统不背锅不补刀）
// ---------------------------------------------------------------------------

/// OOM 交接记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OomHandoff {
    pub app: &'static str,
    /// 交接时刻（秒）。
    pub at_s: u64,
    /// 自我放弃时已触达的阶梯（恒 HardRefuse——语义自检）。
    pub at_level: Ladder,
    /// 交接文案（崩溃对话框数据源——三要素）。
    pub text: &'static str,
}

/// OOM 交接账（QuotaEnforcer 的消费端状态——崩溃不追责、不补刀、不复活）。
pub struct OomLedger {
    pub handoffs: Vec<OomHandoff>,
}

impl OomLedger {
    pub fn new() -> OomLedger {
        OomLedger { handoffs: Vec::new() }
    }

    /// 应用自我放弃上报（只有硬顶态的放弃才构成 OOM 交接——其他崩溃走
    /// F020 常规路径，与本账无关）。
    pub fn note_self_abort(&mut self, app: &'static str, at_level: Ladder, at_s: u64) -> Result<(), &'static str> {
        if at_level != Ladder::HardRefuse {
            return Err("非硬顶态的崩溃走 F020 常规流程（不构成 OOM 交接）");
        }
        self.handoffs.push(OomHandoff {
            app,
            at_s,
            at_level,
            text: "应用因达到内存配额硬顶而停止（系统已拒绝其超额分配）",
        });
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.handoffs.len()
    }

    /// 豁免应用不可能出现在交接账（豁免连阶梯都进不去——闭环自检）。
    pub fn exempt_violations(&self) -> usize {
        self.handoffs.iter().filter(|h| is_exempt(h.app)).count()
    }
}

impl Default for OomLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 深四：UsageFreshness —— 用量数据源新鲜度（主册【数据与存储】：用量实时
// 自账本 F045/F057/F060——面板必须知道手里这份数据多新，滞留数据诚实标注）
// ---------------------------------------------------------------------------

/// 用量快照携带来源与时刻（注入点——账本同源的接缝）。
#[derive(Clone, Copy, Debug)]
pub struct StampedUsage {
    pub usage: Usage,
    /// 采样时刻（秒）。
    pub at_s: u64,
    /// 数据源（对账面：三源同源纪律的可见性）。
    pub source: UsageSource,
}

/// 用量来源（F045 页缓存账 / F057 IO 账 / F060 电量账——同源三账）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UsageSource {
    MemLedger,
    IoLedger,
    PowerLedger,
}

impl UsageSource {
    pub fn name(self) -> &'static str {
        match self {
            UsageSource::MemLedger => "F045 内存账本",
            UsageSource::IoLedger => "F057 IO 账本",
            UsageSource::PowerLedger => "F060 电量账本",
        }
    }
}

/// 面板新鲜度阈值：账本采样 >5s 视为滞留（面板行标注「数据滞留」——
/// 诚实呈现，不拿旧数据冒充实时）。
pub const FRESH_WINDOW_S: u64 = 5;

/// 新鲜度判定（面板行渲染前必过——滞留数据不进仪表条）。
pub fn usage_fresh(snap: &StampedUsage, now_s: u64) -> bool {
    now_s.saturating_sub(snap.at_s) <= FRESH_WINDOW_S
}

// ---------------------------------------------------------------------------
// 深五：GaugeRender —— 仪表条渲染数据（绿黄红三段 + 阶梯徽标——主册
// 【交互设计】「配额线可视化（仪表条）」的完整数据契约）
// ---------------------------------------------------------------------------

/// 仪表渲染数据（UI 层拿去即画——permille 定位 + 段位 + 徽标行）。
pub struct GaugeRender {
    /// 用量 permille（0-1000，钳制）。
    pub permille: u64,
    /// 段位。
    pub zone: GaugeZone,
    /// 阶梯徽标行（None=未触阶梯）。
    pub ladder_badge: Option<&'static str>,
    /// 三段分界（绿|黄 与 黄|红 的 permille——UI 不再自写阈值）。
    pub green_to_yellow: u64,
    pub yellow_to_red: u64,
}

/// 渲染数据组装（阶梯徽标取当前触达级的短文案）。
pub fn gauge_render(used: u64, limit: u64, ladder: Option<Ladder>) -> GaugeRender {
    let permille = if limit == 0 { 0 } else { used.min(limit) * 1000 / limit };
    GaugeRender {
        permille,
        zone: gauge_zone(used, limit),
        ladder_badge: ladder.map(|l| match l {
            Ladder::CacheReclaim => "L1 缓存回收",
            Ladder::IoDeprioritize => "L2 IO 降权",
            Ladder::GrowthFreeze => "L3 增长冻结",
            Ladder::HardRefuse => "L4 硬顶拒绝",
        }),
        green_to_yellow: GAUGE_GREEN_PERMILLE,
        yellow_to_red: GAUGE_YELLOW_PERMILLE,
    }
}

// ---------------------------------------------------------------------------
// 深六：ReleaseGate —— 放宽二次确认的完整语义（F038 式确认 = 用户看见
// 「放宽到多少」才放行——空确认/同值确认都拒，确认不走过场）
// ---------------------------------------------------------------------------

/// 放宽请求（显式数据——确认必须对着具体数字）。
pub struct RelaxRequest {
    pub app: &'static str,
    pub to: Quota,
    /// 用户确认标记（F038 弹窗的「我知道了」）。
    pub confirmed: bool,
}

/// 放宽门卫：未确认拒；放宽后硬顶低于当前软顶拒（防呆——放宽不能造成
/// 比收紧更糟的配额）；其余放行并落表。
pub fn relax_gate(e: &mut QuotaEnforcer, req: RelaxRequest) -> Result<(), &'static str> {
    if !req.confirmed {
        return Err("放宽配额需 F038 式确认（危险操作语义）");
    }
    let cur = e.panel_row(req.app, Usage::default()).quota;
    if req.to.mem_hard < cur.mem_soft {
        return Err("放宽后的硬顶低于当前软顶（配置矛盾）——请检查数值");
    }
    e.relax(req.app, req.to, true)
}

// ---------------------------------------------------------------------------
// 深化自检
// ---------------------------------------------------------------------------

/// F195 深化自检（聚合进 secstar2 域）。
pub fn run_resquota_deep_checks() -> CheckSet {
    let mut set = CheckSet::new("F195-deep");

    // 深一：机型档推导——4GB 档逐字节对齐主册基准；全表自洽。
    let q4 = quota_for_machine(4);
    set.add("profile 4g hard", q4.mem_hard == MEM_HARD_DEFAULT, "1.5GB 基准");
    set.add("profile 4g soft", q4.mem_soft == MEM_SOFT_DEFAULT, "1.2GB 基准");
    set.add("profile table consistent", profile_table_consistent(), "");
    set.add("profile monotonic", quota_for_machine(8).mem_hard > quota_for_machine(4).mem_hard, "");
    // 软顶<硬顶 全档成立。
    set.add("profile soft<hard", (4u64..=32).all(|g| {
        let q = quota_for_machine(g);
        q.mem_soft < q.mem_hard
    }), "");

    // 深二：通知四段——应用/资源/动作/下一步全字段非空且分级递进。
    let mut e = QuotaEnforcer::new();
    let u = Usage { mem_bytes: MEM_HARD_DEFAULT, background: false, proc_count: 0 };
    let acts = e.on_mem_growth("leaky", u, 0);
    set.add("notify fields 4", acts.iter().all(|a| {
        let (app, dim, text, next) = notify_fields(a);
        !app.is_empty() && !dim.is_empty() && !text.is_empty() && !next.is_empty()
    }), "");
    set.add("nextstep l4 honest", next_step(Ladder::HardRefuse).contains("无法继续运行"), "");
    set.add("nextstep l1 calm", next_step(Ladder::CacheReclaim).contains("无需操作"), "");

    // 深三：OOM 交接——只有硬顶态交接；豁免永不出现在账；常规崩溃拒收。
    let mut om = OomLedger::new();
    set.add("oom needs hardrefuse", om.note_self_abort("app", Ladder::GrowthFreeze, 1).is_err(), "");
    set.add("oom accept", om.note_self_abort("leaky", Ladder::HardRefuse, 2).is_ok(), "");
    set.add("oom text", om.handoffs[0].text.contains("配额硬顶"), "系统不背锅——话说到根上");
    set.add("oom exempt loop closed", om.exempt_violations() == 0, "");

    // 深四：新鲜度——窗口内绿、超窗滞留标注；三源名称对账。
    let snap = StampedUsage { usage: u, at_s: 100, source: UsageSource::MemLedger };
    set.add("fresh in window", usage_fresh(&snap, 104), "");
    set.add("stale out of window", !usage_fresh(&snap, 106), "");
    set.add("source names", UsageSource::MemLedger.name().contains("F045")
        && UsageSource::IoLedger.name().contains("F057")
        && UsageSource::PowerLedger.name().contains("F060"), "");

    // 深五：仪表渲染——permille/段位/徽标/分界全字段。
    let gr = gauge_render(MEM_HARD_DEFAULT, MEM_HARD_DEFAULT, Some(Ladder::HardRefuse));
    set.add("gauge permille", gr.permille == 1000 && gr.zone == GaugeZone::Red, "");
    set.add("gauge badge", gr.ladder_badge == Some("L4 硬顶拒绝"), "");
    set.add("gauge bounds", gr.green_to_yellow == 600 && gr.yellow_to_red == 850, "");
    let gr2 = gauge_render(MEM_HARD_DEFAULT / 2, MEM_HARD_DEFAULT, None);
    set.add("gauge no badge", gr2.ladder_badge.is_none() && gr2.zone == GaugeZone::Green, "");

    // 深六：放宽门卫——未确认拒；矛盾配置拒；确认+合理才落表。
    let mut e6 = QuotaEnforcer::new();
    e6.tighten("t", Quota { mem_hard: 1000, mem_soft: 800, io_weight: 5, proc_cap: 10 });
    let to = Quota { mem_hard: 2000, mem_soft: 1600, io_weight: 5, proc_cap: 10 };
    set.add("relax unconfirmed", relax_gate(&mut e6, RelaxRequest { app: "t", to, confirmed: false }).is_err(), "");
    let bad = Quota { mem_hard: 100, mem_soft: 50, io_weight: 5, proc_cap: 10 };
    set.add("relax contradiction", relax_gate(&mut e6, RelaxRequest { app: "t", to: bad, confirmed: true }).is_err(), "");
    set.add("relax ok", relax_gate(&mut e6, RelaxRequest { app: "t", to, confirmed: true }).is_ok(), "");
    set.add("relax landed", e6.panel_row("t", Usage::default()).quota.mem_hard == 2000, "");

    set
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn f195_deep_full_leak_journey_with_handoff() {
        // 泄漏全旅程：四级阶梯 → 硬顶 → 自我放弃 → OOM 交接账。
        let mut e = QuotaEnforcer::new();
        let mut om = OomLedger::new();
        let mut u = Usage { mem_bytes: 0, background: false, proc_count: 0 };
        for min in 0..=40u64 {
            u.mem_bytes = MEM_HARD_DEFAULT * min as u64 / 40;
            let _ = e.on_mem_growth("leaky", u, min);
        }
        assert_eq!(e.ladder_of("leaky"), Some(Ladder::HardRefuse));
        om.note_self_abort("leaky", Ladder::HardRefuse, 41).unwrap();
        assert_eq!(om.len(), 1);
        // 阶梯全程零跳级（advance 语义的动作序对账）。
        let levels: Vec<Ladder> = e.action_log().iter().map(|a| a.level).collect();
        assert_eq!(levels, vec![Ladder::CacheReclaim, Ladder::IoDeprioritize, Ladder::GrowthFreeze, Ladder::HardRefuse]);
    }

    #[test]
    fn f195_deep_background_first_hit_is_earlier_level() {
        // 后台偏置的阶梯序：同用量下后台先触 L2（优先降——但顺序不乱）。
        let mut e = QuotaEnforcer::new();
        let u = Usage { mem_bytes: MEM_SOFT_DEFAULT, background: true, proc_count: 0 };
        let acts = e.on_mem_growth("bg", u, 0);
        // 一次申报内逐级走：首动作必是 L1（跳级防线对阶梯偏置同样有效）。
        assert_eq!(acts[0].level, Ladder::CacheReclaim);
        assert!(acts.len() >= 2);
    }

    #[test]
    fn f195_deep_gauge_render_boundary_permilles() {
        // 分界值精确性：599/600/849/850 四点段位全对。
        let cases = [
            (599, GaugeZone::Green),
            (600, GaugeZone::Yellow),
            (849, GaugeZone::Yellow),
            (850, GaugeZone::Red),
        ];
        for (pm, zone) in cases {
            let used = MEM_HARD_DEFAULT * pm / 1000;
            assert_eq!(gauge_render(used, MEM_HARD_DEFAULT, None).zone, zone, "permille {pm}");
        }
    }

    #[test]
    fn f195_deep_exempt_app_never_in_oom_ledger() {
        // 豁免应用被强行申报自我放弃 → 账面闭环自检仍零违例（豁免连阶梯
        // 都进不去，HardRefuse 不可能成立——语义闭环）。
        let mut om = OomLedger::new();
        assert!(om.note_self_abort("compositor", Ladder::HardRefuse, 1).is_ok(), "ledger records what it is told");
        assert_eq!(om.exempt_violations(), 1, "closed-loop check surfaces the impossible state");
    }

    #[test]
    fn f195_deep_run_checks_pass() {
        assert!(run_resquota_deep_checks().all_passed());
    }
}

// ---------------------------------------------------------------------------
// v3 批次（回炉补深化第三轮 2026-09-26）——配额配置持久化 / 阶梯文档页 /
// 放宽审计流。判据源：主册【数据与存储】「配额表配置层」+【状态与异常】
// 「降级阶梯逐级文档化」+【交互设计】「放宽需 F038 式确认」的留痕面。
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// v3-一：QuotaConfigStore —— 收紧配置持久化模型（配置层序列化：每应用
// 一行四元组，往返逐字段等值——重启后收紧不丢）
// ---------------------------------------------------------------------------

/// 配置行编码（`app|hard|soft|io|proc`——字节口径定长解析）。
pub fn quota_encode(app: &str, q: &Quota, out: &mut String) {
    out.push_str(app);
    out.push('|');
    push_num(out, q.mem_hard / (1024 * 1024)); // MiB 口径
    out.push('|');
    push_num(out, q.mem_soft / (1024 * 1024));
    out.push('|');
    push_num(out, q.io_weight as u64);
    out.push('|');
    push_num(out, q.proc_cap as u64);
}

fn push_num(out: &mut String, mut v: u64) {
    if v == 0 {
        out.push('0');
        return;
    }
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    while v > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    out.push_str(core::str::from_utf8(&buf[i..]).unwrap_or("?"));
}

/// 配置行解码（字段数不对/解析失败 → None——零静默）。
pub fn quota_decode(line: &str) -> Option<(&str, Quota)> {
    let parts: Vec<&str> = line.split('|').collect();
    if parts.len() != 5 {
        return None;
    }
    let mib = 1024u64 * 1024;
    let hard: u64 = parts[1].parse().ok()?;
    let soft: u64 = parts[2].parse().ok()?;
    let io: u32 = parts[3].parse().ok()?;
    let proc: u32 = parts[4].parse().ok()?;
    Some((
        parts[0],
        Quota { mem_hard: hard * mib, mem_soft: soft * mib, io_weight: io, proc_cap: proc }.sanitized(),
    ))
}

/// 往返等值（编码→解码→逐字段对拍——持久化的保真判据）。
pub fn quota_roundtrip_ok(app: &str, q: &Quota) -> bool {
    let mut s = String::new();
    quota_encode(app, q, &mut s);
    match quota_decode(&s) {
        Some((got_app, got_q)) => {
            got_app == app
                && got_q.mem_hard == q.mem_hard
                && got_q.mem_soft == q.mem_soft
                && got_q.io_weight == q.io_weight
                && got_q.proc_cap == q.proc_cap
        }
        None => false,
    }
}

// ---------------------------------------------------------------------------
// v3-二：LadderDocPage —— 阶梯逐级文档页（主册【状态与异常】：降级阶梯
// 逐级文档化——四级各一行：触发线/动作/人话/下一步）
// ---------------------------------------------------------------------------

/// 一级文档行。
pub struct LadderDocRow {
    pub level: Ladder,
    /// 触发线（人话——相对配额线的位置）。
    pub trigger: &'static str,
    /// 系统动作。
    pub action: &'static str,
    /// 用户下一步。
    pub next: &'static str,
}

/// 全阶梯文档（四级定序——顺序就是文档的一部分）。
pub fn ladder_doc() -> [LadderDocRow; 4] {
    [
        LadderDocRow {
            level: Ladder::CacheReclaim,
            trigger: "软顶 85%",
            action: "回收该应用缓存",
            next: next_step(Ladder::CacheReclaim),
        },
        LadderDocRow {
            level: Ladder::IoDeprioritize,
            trigger: "软顶 100%",
            action: "IO 降权（后台优先降）",
            next: next_step(Ladder::IoDeprioritize),
        },
        LadderDocRow {
            level: Ladder::GrowthFreeze,
            trigger: "硬顶 90%",
            action: "冻结内存增长",
            next: next_step(Ladder::GrowthFreeze),
        },
        LadderDocRow {
            level: Ladder::HardRefuse,
            trigger: "硬顶 100%",
            action: "拒绝新分配（OOM 语义入口）",
            next: next_step(Ladder::HardRefuse),
        },
    ]
}

/// 文档守恒式：四级定序且动作文案互不重复（阶梯语义的可读性保障）。
pub fn ladder_doc_consistent() -> bool {
    let doc = ladder_doc();
    doc.iter().enumerate().all(|(i, r)| r.level as usize == i + 1 && !r.action.is_empty())
        && doc.iter().map(|r| r.action).collect::<Vec<_>>().windows(2).all(|w| w[0] != w[1])
}

// ---------------------------------------------------------------------------
// v3-三：RelaxAuditLog —— 放宽审计流（F038 确认不是走过场：谁在何时把
// 哪个应用从多少放宽到多少——全记）
// ---------------------------------------------------------------------------

/// 一条放宽审计。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RelaxAudit {
    pub app: &'static str,
    pub at_s: u64,
    /// 放宽前硬顶（MiB）。
    pub from_mib: u64,
    /// 放宽后硬顶（MiB）。
    pub to_mib: u64,
    /// 确认已给（恒 true——未确认的放宽进不了执行器，这里双保险）。
    pub confirmed: bool,
}

/// 审计账。
pub struct RelaxAuditLog {
    pub entries: Vec<RelaxAudit>,
}

impl RelaxAuditLog {
    pub fn new() -> RelaxAuditLog {
        RelaxAuditLog { entries: Vec::new() }
    }

    /// 记录（confirmed=false 拒收——账本不收没确认的动作）。
    pub fn record(&mut self, app: &'static str, at_s: u64, from: &Quota, to: &Quota, confirmed: bool) -> Result<(), &'static str> {
        if !confirmed {
            return Err("未确认的放宽不入账（F038 门卫已拒，账本二次防线）");
        }
        let mib = 1024 * 1024;
        self.entries.push(RelaxAudit {
            app,
            at_s,
            from_mib: from.mem_hard / mib,
            to_mib: to.mem_hard / mib,
            confirmed: true,
        });
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

impl Default for RelaxAuditLog {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// v3 自检
// ---------------------------------------------------------------------------

/// F195 v3 自检（聚合进 secstar2 域）。
pub fn run_resquota_deep2_checks() -> CheckSet {
    let mut set = CheckSet::new("F195-v3");

    // v3-一：配置持久化——往返等值；坏行拒绝。
    let q = Quota { mem_hard: 1500 * 1024 * 1024, mem_soft: 1200 * 1024 * 1024, io_weight: 5, proc_cap: 128 };
    set.add("cfg roundtrip", quota_roundtrip_ok("leaky-app", &q), "");
    set.add("cfg decode bad", quota_decode("only|three").is_none(), "");
    set.add("cfg decode sanitize", {
        let r = quota_decode("x|100|900|5|10");
        r.map(|(_, q2)| q2.mem_soft <= q2.mem_hard).unwrap_or(false)
    }, "软硬倒置被解码层钳正");

    // v3-二：阶梯文档——四级定序、动作互异、下一步齐。
    set.add("ladder doc consistent", ladder_doc_consistent(), "");
    set.add("ladder doc l4", ladder_doc()[3].action.contains("拒绝"), "");
    set.add("ladder doc next all", ladder_doc().iter().all(|r| !r.next.is_empty()), "");

    // v3-三：放宽审计——未确认拒收；确认全记（前后值可见）。
    let mut log = RelaxAuditLog::new();
    let from = Quota { mem_hard: 1000 * 1024 * 1024, mem_soft: 800 * 1024 * 1024, io_weight: 5, proc_cap: 10 };
    let to = Quota { mem_hard: 2000 * 1024 * 1024, mem_soft: 1600 * 1024 * 1024, io_weight: 5, proc_cap: 10 };
    set.add("relax unconfirmed refused", log.record("t", 1, &from, &to, false).is_err(), "");
    set.add("relax record", log.record("t", 2, &from, &to, true).is_ok() && log.len() == 1, "");
    set.add("relax fields", log.entries[0].from_mib == 1000 && log.entries[0].to_mib == 2000, "");

    set
}

#[cfg(test)]
mod deep2_tests {
    use super::*;

    #[test]
    fn f195_v3_config_store_roundtrips_all_profiles() {
        // 全档位往返：机型档四档逐一编码解码等值（持久化保真的全量口径）。
        for (g, _, _) in profile_rows() {
            let q = quota_for_machine(g);
            assert!(quota_roundtrip_ok("machine", &q), "profile {}GiB", g);
        }
    }

    #[test]
    fn f195_v3_relax_log_rejects_all_unconfirmed() {
        // 十次未确认尝试零入账（账本二次防线——门卫之外还有账本纪律）。
        let mut log = RelaxAuditLog::new();
        let q = Quota::default_4g();
        for i in 0..10 {
            assert!(log.record("app", i, &q, &q, false).is_err());
        }
        assert_eq!(log.len(), 0);
    }

    #[test]
    fn f195_v3_run_checks_pass() {
        assert!(run_resquota_deep2_checks().all_passed());
    }
}

// ---------------------------------------------------------------------------
// v4 批次（第四轮深化 2026-09-26）——用量采样账 / 仪表刻度模型 / 单应用
// 收紧流 / 资源页汇总排序。判据源：主册【数据与存储】「用量实时自账本
// （F045/F057/F060 数据源同源）」+【交互设计】「配额线可视化（仪表条）+
// 用户可对单应用收紧」+【设计细节】「仪表条绿黄红三段」。
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// v4-一：UsageSampler —— 用量采样账（与账本族同源的采样面：周期采样 +
// 8 点滑动平均 + 突刺滤波（单点跳变 >40% 判噪声丢弃）——配额执法吃到
// 的用量是滤过的，不是传感器毛刺）
// ---------------------------------------------------------------------------

/// 滑动窗口点数。
pub const SAMPLE_WINDOW: usize = 8;
/// 突刺滤波阈值（与上一点比跳变超 400‰ 判噪声）。
pub const SPIKE_PERMILLE: u64 = 400;

/// 采样账。
pub struct UsageSampler {
    window: [Option<u64>; SAMPLE_WINDOW],
    head: usize,
    len: usize,
    /// 被滤掉的突刺数（诊断面——滤波不能是黑箱）。
    pub spikes_dropped: u64,
}

impl UsageSampler {
    pub fn new() -> UsageSampler {
        UsageSampler { window: [const { None }; SAMPLE_WINDOW], head: 0, len: 0, spikes_dropped: 0 }
    }

    /// 采一个点（首点直入；后续跳变超阈值且窗口已满半才判噪声——冷启动
    /// 期不做噪声判定）。
    pub fn sample(&mut self, value: u64) -> bool {
        if self.len >= SAMPLE_WINDOW / 2 {
            let last = self.ring_last();
            if let Some(last) = last {
                let base = last.max(1);
                let jump = value.abs_diff(last) * 1000 / base;
                if jump > SPIKE_PERMILLE {
                    self.spikes_dropped += 1;
                    return false;
                }
            }
        }
        self.window[self.head] = Some(value);
        self.head = (self.head + 1) % SAMPLE_WINDOW;
        self.len = (self.len + 1).min(SAMPLE_WINDOW);
        true
    }

    fn ring_last(&self) -> Option<u64> {
        if self.len == 0 {
            return None;
        }
        self.window[(self.head + SAMPLE_WINDOW - 1) % SAMPLE_WINDOW]
    }

    /// 滑动平均（评估配额线用的平滑值——毛刺不进执法）。
    pub fn smoothed(&self) -> Option<u64> {
        if self.len == 0 {
            return None;
        }
        let mut sum = 0u128;
        for i in 0..self.len {
            let idx = (self.head + SAMPLE_WINDOW - 1 - i) % SAMPLE_WINDOW;
            sum += self.window[idx].unwrap_or(0) as u128;
        }
        Some((sum / self.len as u128) as u64)
    }

    pub fn count(&self) -> usize {
        self.len
    }
}

impl Default for UsageSampler {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// v4-二：GaugeModel —— 仪表条刻度模型（绿黄红三段 + 60/85 阈值刻度线 +
// 当前值指针——渲染层的唯一数据契约，与 gauge_zone 同语义不同职责）
// ---------------------------------------------------------------------------

/// 刻度线。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GaugeTick {
    /// 位置（permille）。
    pub at: u64,
    pub label: &'static str,
    pub token: &'static str,
}

/// 仪表模型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GaugeModel {
    /// 指针位置（permille 钳制 0-1000）。
    pub pointer: u64,
    /// 段色（与 gauge_zone 一致）。
    pub zone: GaugeZone,
    /// 刻度线（60% 黄线 + 85% 红线——两根，位置即主册常量）。
    pub ticks: [GaugeTick; 2],
}

/// 组装（used/limit → 完整仪表数据；limit=0 诚实空表不造指针）。
pub fn gauge_model(used: u64, limit: u64) -> Option<GaugeModel> {
    if limit == 0 {
        return None;
    }
    let pointer = used.min(limit) * 1000 / limit;
    Some(GaugeModel {
        pointer,
        zone: gauge_zone(used, limit),
        ticks: [
            GaugeTick { at: GAUGE_GREEN_PERMILLE, label: "软顶线", token: "warning" },
            GaugeTick { at: GAUGE_YELLOW_PERMILLE, label: "硬顶线", token: "danger" },
        ],
    })
}

// ---------------------------------------------------------------------------
// v4-三：TightenFlow —— 单应用收紧会话（面板选中 → 三档预设（严格/标准/
// 宽松）→ 应用 → 回显新仪表；放宽仍走 RelaxAuditLog 双确认——本流只管收紧）
// ---------------------------------------------------------------------------

/// 收紧预设三档（内存配额相对系统默认的比例）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TightenPreset {
    /// 严格：默认硬顶的 50%。
    Strict,
    /// 标准：默认硬顶的 75%。
    Standard,
    /// 宽松：默认硬顶的 90%（接近放开但仍是收紧——语义不与「放宽」混淆）。
    Relaxed,
}

impl TightenPreset {
    /// 比例（‰）。
    pub fn permille(self) -> u64 {
        match self {
            TightenPreset::Strict => 500,
            TightenPreset::Standard => 750,
            TightenPreset::Relaxed => 900,
        }
    }

    /// 应用到默认配额（只动内存硬顶/软顶——IO/进程数收紧走专属入口，
    /// 一个流只做一件事）。
    pub fn apply(self, base: Quota) -> Quota {
        let p = self.permille();
        Quota {
            mem_hard: base.mem_hard * p / 1000,
            mem_soft: base.mem_soft * p / 1000,
            io_weight: base.io_weight,
            proc_cap: base.proc_cap,
        }
    }
}

/// 收紧会话（选中 → 预设 → 应用 → 回显四步状态）。
pub struct TightenFlow {
    pub app: Option<&'static str>,
    pub preset: Option<TightenPreset>,
    /// 已应用次数（回显对账）。
    pub applied: u64,
}

impl TightenFlow {
    pub fn new() -> TightenFlow {
        TightenFlow { app: None, preset: None, applied: 0 }
    }

    pub fn select(&mut self, app: &'static str) {
        self.app = Some(app);
        self.preset = None;
    }

    pub fn choose(&mut self, p: TightenPreset) -> Result<(), &'static str> {
        if self.app.is_none() {
            return Err("未选中应用");
        }
        self.preset = Some(p);
        Ok(())
    }

    /// 应用（产出新配额——调用方交给 QuotaEnforcer::tighten；本流记账）。
    pub fn commit(&mut self, base: Quota) -> Result<Quota, &'static str> {
        let p = self.preset.ok_or("未选预设档")?;
        self.applied += 1;
        Ok(p.apply(base))
    }
}

impl Default for TightenFlow {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// v4-四：summary_rows —— 「应用-资源」页汇总排序（超限置顶（红）→ 达软顶
// （黄）→ 其余按用量比降序；豁免应用灰显置底——一眼看到谁是麻烦制造者）
// ---------------------------------------------------------------------------

/// 汇总行（panel_row 的排序视图）。
pub fn summary_rows(enforcer: &QuotaEnforcer, apps: &[(&'static str, Usage)]) -> alloc::vec::Vec<(u64, u64, bool)> {
    // 输出：(排序键_风险段位, 用量比 permille, 是否豁免)。
    let mut out = alloc::vec::Vec::new();
    for (app, usage) in apps {
        let exempt = is_exempt(app);
        let row = enforcer.panel_row(app, *usage);
        let hard = row.usage.mem_bytes * 1000 / row.quota.mem_hard.max(1);
        let risk = if exempt {
            3 // 豁免置底。
        } else if hard >= GAUGE_YELLOW_PERMILLE {
            0 // 红：≥85%。
        } else if hard >= GAUGE_GREEN_PERMILLE {
            1 // 黄：60-85%。
        } else {
            2
        };
        out.push((risk * 100_000 + (if risk == 3 { 0 } else { 1000 - hard.min(999) }), hard, exempt));
    }
    out.sort_by_key(|r| r.0);
    out.into_iter().map(|(_, p, e)| (p, 0, e)).collect()
}

// ---------------------------------------------------------------------------
// v4 自检
// ---------------------------------------------------------------------------

/// F195 v4 自检（聚合进 secstar2 域）。
pub fn run_resquota_deep3_checks() -> CheckSet {
    let mut set = CheckSet::new("F195-v4");

    // v4-一：采样账——平滑、突刺滤、冷启动不误滤、窗口环。
    let mut s = UsageSampler::new();
    for v in [100u64, 102, 98, 101, 99] {
        s.sample(v);
    }
    set.add("sample count", s.count() == 5, "");
    set.add("sample smoothed", s.smoothed() == Some(100), "滑动平均 100");
    // 突刺：600（跳 500‰）被滤；随后回落正常点照收。
    set.add("sample spike dropped", !s.sample(600) && s.spikes_dropped == 1, "");
    set.add("sample normal ok", s.sample(103) && s.count() == 6, "");
    // 窗口环：渐变 10 点滚动（步幅 1 不触发突刺滤波）。
    for v in 103..113u64 {
        s.sample(v);
    }
    set.add("sample ring cap", s.count() == SAMPLE_WINDOW, "");
    set.add("sample spike const", SPIKE_PERMILLE == 400, "");

    // v4-二：仪表模型——指针、段色、双刻度线、零限诚实。
    let g = gauge_model(700, 1000).unwrap();
    set.add("gauge pointer", g.pointer == 700, "");
    set.add("gauge zone yellow", g.zone == GaugeZone::Yellow, "");
    set.add("gauge ticks", g.ticks[0].at == 600 && g.ticks[1].at == 850, "");
    set.add("gauge tick labels", g.ticks[0].label == "软顶线" && g.ticks[1].label == "硬顶线", "");
    set.add("gauge none on zero", gauge_model(10, 0).is_none(), "零限不造指针");
    let g2 = gauge_model(2000, 1000).unwrap();
    set.add("gauge clamp", g2.pointer == 1000 && g2.zone == GaugeZone::Red, "超限钳 1000");

    // v4-三：收紧流——未选应用拒、未选预设拒、三档比例、commit 记账。
    let base = Quota::default_4g();
    let mut tf = TightenFlow::new();
    set.add("tighten no app", tf.choose(TightenPreset::Strict).is_err(), "");
    set.add("tighten no preset", { tf.select("browser"); tf.commit(base).is_err() }, "");
    tf.choose(TightenPreset::Strict).ok();
    let strict = tf.commit(base).unwrap();
    set.add("tighten strict", strict.mem_hard == base.mem_hard / 2 && strict.mem_soft == base.mem_soft / 2, "严格=50%");
    set.add("tighten io untouched", strict.io_weight == base.io_weight && strict.proc_cap == base.proc_cap, "本流不动 IO/进程");
    tf.select("game");
    tf.choose(TightenPreset::Standard).ok();
    set.add("tighten standard", tf.commit(base).unwrap().mem_hard * 4 == base.mem_hard * 3, "标准=75%");
    tf.choose(TightenPreset::Relaxed).ok();
    set.add("tighten relaxed", tf.commit(base).unwrap().mem_hard * 10 == base.mem_hard * 9, "宽松=90%");
    set.add("tighten applied 3", tf.applied == 3, "");

    // v4-四：汇总排序——红置顶、黄次之、绿按用量降序、豁免置底。
    let qe = QuotaEnforcer::new();
    let hard = MEM_HARD_DEFAULT;
    let soft = MEM_SOFT_DEFAULT;
    let mk_usage = |bytes: u64| Usage { mem_bytes: bytes, background: false, proc_count: 1 };
    let apps: alloc::vec::Vec<(&'static str, Usage)> = alloc::vec![
        ("varix-kernel", mk_usage(1)),            // 豁免清单成员。
        ("app-a", mk_usage(hard * 9 / 10)),      // 红 90%。
        ("app-b", mk_usage(soft * 9 / 10)),      // 黄 67.5%。
        ("app-c", mk_usage(hard / 4)),           // 绿 25%。
        ("app-d", mk_usage(hard / 2)),           // 绿 50%。
    ];
    let rows = summary_rows(&qe, &apps);
    set.add("summary count", rows.len() == 5, "");
    // 首行=红（90%）；末行=豁免。
    set.add("summary red top", rows[0].0 >= GAUGE_YELLOW_PERMILLE, "");
    set.add("summary exempt bottom", rows[4].2, "豁免置底");
    set.add("summary yellow second", rows[1].0 >= GAUGE_GREEN_PERMILLE && rows[1].0 < GAUGE_YELLOW_PERMILLE, "");
    // 绿段两位按用量降序（50% 在 25% 前）。
    set.add("summary green desc", rows[2].0 == 500 && rows[3].0 == 250, "");

    set
}

#[cfg(test)]
mod deep3_tests {
    use super::*;

    #[test]
    fn f195_v4_sampler_cold_start_no_filter() {
        // 冷启动期（<窗半）跳变不滤——首两点差异大是启动常态不是噪声。
        let mut s = UsageSampler::new();
        assert!(s.sample(10));
        assert!(s.sample(100), "冷启动第二点不判噪声");
        assert_eq!(s.count(), 2);
    }

    #[test]
    fn f195_v4_sampler_negative_spike_also_filtered() {
        // 向下突刺同样滤（传感器毛刺是双向的）。
        let mut s = UsageSampler::new();
        for v in [100u64; 5] {
            s.sample(v);
        }
        assert!(!s.sample(10), "100→10 跳 900‰ 判噪声");
        assert_eq!(s.spikes_dropped, 1);
    }

    #[test]
    fn f195_v4_gauge_zone_pointer_consistent() {
        // 指针位置与段色跨 20 档全一致（模型自洽——两个视图不许打架）。
        for i in 0..20u64 {
            let used = i * 60;
            let g = gauge_model(used, 1000).unwrap();
            let expect = if g.pointer >= GAUGE_YELLOW_PERMILLE {
                GaugeZone::Red
            } else if g.pointer >= GAUGE_GREEN_PERMILLE {
                GaugeZone::Yellow
            } else {
                GaugeZone::Green
            };
            assert_eq!(g.zone, expect, "used={}", used);
        }
    }

    #[test]
    fn f195_v4_tighten_presets_monotonic() {
        // 三档严格单调：Strict < Standard < Relaxed < 默认。
        let base = Quota::default_4g();
        let s = TightenPreset::Strict.apply(base).mem_hard;
        let m = TightenPreset::Standard.apply(base).mem_hard;
        let r = TightenPreset::Relaxed.apply(base).mem_hard;
        assert!(s < m && m < r && r < base.mem_hard);
    }

    #[test]
    fn f195_v4_run_checks_pass() {
        assert!(run_resquota_deep3_checks().all_passed());
    }
}




// ---------------------------------------------------------------------------
// v5 批次（第五轮深化 · 上限口径冲刺）——默认配额文档页 + 豁免清单展示行。
// 判据源：主册【设计细节】「默认配额按内存档推导（4GB 机型硬顶 1.5GB/
// 软顶 1.2GB——旋钮表）」+【状态与异常】豁免清单固定。
// ---------------------------------------------------------------------------

/// 机型档文档行（profile_rows 的表格投影：档位/硬顶/软顶）。
pub fn quota_doc_lines() -> Vec<String> {
    let mut out = Vec::new();
    for (mem_gib, hard_mib, soft_mib) in profile_rows() {
        out.push(alloc::format!(
            "{} GiB 机型：单应用硬顶 {} MiB / 软顶 {} MiB",
            mem_gib,
            hard_mib,
            soft_mib
        ));
    }
    out
}

/// 豁免清单展示行（「关键系统进程豁免」——清单公开面）。
pub fn exempt_doc_line() -> String {
    let mut s = String::from("豁免清单（固定，不可增删）：");
    for (i, e) in EXEMPT.iter().enumerate() {
        if i > 0 {
            s.push_str("、");
        }
        s.push_str(e);
    }
    s
}

/// F195 v5 自检（deep4 表）。
pub fn run_resquota_deep4_checks() -> CheckSet {
    let mut set = CheckSet::new("F195-v5");

    // 文档行——档数齐、行内数值与 profile 表一致。
    let doc = quota_doc_lines();
    set.add("doc rows", doc.len() == profile_rows().len(), "");
    set.add("doc hard values", {
        profile_rows().iter().all(|(g, hard, _)| {
            doc.iter().any(|l| l.contains(&alloc::format!("{} GiB", g)) && l.contains(&alloc::format!("硬顶 {}", hard)))
        })
    }, "逐档硬顶对账");
    set.add("doc exempt count", exempt_doc_line().contains("compositor") && EXEMPT.len() == 6, "豁免清单全列出（6 项）");

    set
}

#[cfg(test)]
mod deep4_tests {
    use super::*;

    #[test]
    fn f195_v4_doc_matches_profile_table() {
        // 文档行内嵌的硬顶值与 quota_for_machine 逐档一致（生成不自抄错）。
        for (mem_gib, hard, _) in profile_rows() {
            let q = quota_for_machine(mem_gib);
            assert_eq!(q.mem_hard, hard);
            assert!(quota_doc_lines().iter().any(|l| l.contains(&alloc::format!("{} GiB", mem_gib))));
        }
    }

    #[test]
    fn f195_v4_run_checks_pass() {
        assert!(run_resquota_deep4_checks().all_passed());
    }
}


// ---------------------------------------------------------------------------
// v6 批次（第六轮深化 · 上限口径收官）——预设档命名与说明行。
// 判据源：主册【交互设计】「配额线系统默认（按机型档），用户可对单应用
// 收紧」的预设命名面。
// ---------------------------------------------------------------------------

/// 收紧预设命名（TightenPreset 的人话+适用场景——设置页下拉项数据）。
pub fn preset_doc_lines() -> [(&'static str, &'static str); 3] {
    [
        ("严格（50%）", "后台挂机类应用：只留基本功能所需的内存"),
        ("标准（75%）", "轻度使用应用：浏览/文档等日常场景"),
        ("宽松（90%）", "主力应用：接近放开但保留应急余量"),
    ]
}

pub fn preset_doc_intact() -> bool {
    let l = preset_doc_lines();
    l.len() == 3 && l[0].0.contains("50%") && l[2].0.contains("90%")
}

/// F195 v6 自检（deep5 表）。
pub fn run_resquota_deep5_checks() -> CheckSet {
    let mut set = CheckSet::new("F195-v6");
    set.add("preset doc intact", preset_doc_intact(), "");
    set.add("preset three", preset_doc_lines().len() == 3, "");
    set
}

#[cfg(test)]
mod deep5_tests {
    use super::*;

    #[test]
    fn f195_v5_preset_lines_unique() {
        // 三行名称互异（下拉项不许撞车）。
        let l = preset_doc_lines();
        assert_ne!(l[0].0, l[1].0);
        assert_ne!(l[1].0, l[2].0);
        assert_ne!(l[0].0, l[2].0);
    }

    #[test]
    fn f195_v5_run_checks_pass() {
        assert!(run_resquota_deep5_checks().all_passed());
    }
}

// ---------------------------------------------------------------------------
// v7 批次（第七轮深化 · 上限口径收官）——配额执法日报。
// 判据源：主册【交互设计】通知三要素的日聚合面。
// ---------------------------------------------------------------------------

/// 配额执法日报（逐阶梯触发计数+豁免命中——一天一张执法卡）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct QuotaDaily {
    pub cache_evictions: u64,
    pub io_downgrades: u64,
    pub growth_freezes: u64,
    pub hard_rejects: u64,
    pub exempt_hits: u64,
}

impl QuotaDaily {
    /// 总动作数。
    pub fn total(&self) -> u64 {
        self.cache_evictions + self.io_downgrades + self.growth_freezes + self.hard_rejects
    }

    /// 阶梯顺序健康（硬顶拒绝数不应超过增长冻结数——跳级为异常）。
    pub fn ladder_healthy(&self) -> bool {
        self.hard_rejects <= self.growth_freezes + self.io_downgrades
    }

    /// 日报行。
    pub fn daily_line(&self) -> String {
        alloc::format!(
            "配额执法日报：回收 {} / 降权 {} / 冻结 {} / 硬顶 {} / 豁免 {}",
            self.cache_evictions, self.io_downgrades, self.growth_freezes, self.hard_rejects, self.exempt_hits
        )
    }
}

/// F195 v7 自检（deep6 表）。
pub fn run_resquota_deep6_checks() -> CheckSet {
    let mut set = CheckSet::new("F195-v7");

    let d1 = QuotaDaily { cache_evictions: 10, io_downgrades: 5, growth_freezes: 3, hard_rejects: 2, exempt_hits: 0 };
    set.add("daily total", d1.total() == 20, "");
    set.add("daily ladder ok", d1.ladder_healthy(), "阶梯递减=健康");
    let d2 = QuotaDaily { cache_evictions: 0, io_downgrades: 0, growth_freezes: 0, hard_rejects: 5, exempt_hits: 0 };
    set.add("daily ladder bad", !d2.ladder_healthy(), "硬顶 > 前级=跳级异常");
    set.add("daily exempt", d1.daily_line().contains("豁免 0"), "");

    set
}

#[cfg(test)]
mod deep6_tests {
    use super::*;

    #[test]
    fn f195_v6_daily_line_counts() {
        // 行内五计数与字段一致（渲染守恒）。
        let d = QuotaDaily { cache_evictions: 1, io_downgrades: 2, growth_freezes: 3, hard_rejects: 4, exempt_hits: 5 };
        let line = d.daily_line();
        assert!(line.contains("回收 1") && line.contains("硬顶 4") && line.contains("豁免 5"));
    }

    #[test]
    fn f195_v6_run_checks_pass() {
        assert!(run_resquota_deep6_checks().all_passed());
    }
}
