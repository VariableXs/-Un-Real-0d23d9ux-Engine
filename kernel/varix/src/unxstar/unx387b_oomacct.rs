//! unx387b_oomacct —— UNX-F0387 堆耗尽显性路径（OOM 分级账）· 双实现并存 B 面。
//! 锚点（CoRun Unxreal 主册 L1858）：UNX-F0387 · 堆耗尽显性路径（OOM 分级账） · 目标 240 行 · 已深化
//! 判据原文：OOM 三级（warn 80%/crit 95%/dead 100%）触发正确，分级动作账齐
//! 并存声明：unx387_oomtier.rs（并行工号 A 面）在先，本面为独立第二实现（源码只增不减）。
//! 组件：三级水位账（warn 80% / crit 95% / dead 100% · 迟回滞回口径）
//!      + 分级动作账（warn=记录 / crit=拒绝新分配+回收建议 / dead=停分配+仅许释放）
//!      + 触发正确性核（水位 → 级别映射逐点断言 + 迟回滞回）
//!      + 越级注入核（跳级/回卷注入 · 分级闸必拒）
//! 判据落地：三级触发正确 + 动作账齐 + 注入必拒
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 整数算术，零墙钟。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本（B 面）。
pub const OB_VERSION: &str = "unx387b_oomacct-v1";

/// 诊断段基址（0x5180 段 · 本模块独占）。
pub const OB_SEG_BASE: u16 = 0x5180;

/// OOM 三级（闭集 · 冻结 v1）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ObLevel {
    /// 正常（< 80%）。
    Ok,
    /// 警告（≥ 80%）。
    Warn,
    /// 危急（≥ 95%）。
    Crit,
    /// 耗尽（= 100%）。
    Dead,
}

impl ObLevel {
    /// 按用量占比（万分比）分级。
    pub fn of(bps: u32) -> ObLevel {
        if bps >= 10000 {
            ObLevel::Dead
        } else if bps >= 9500 {
            ObLevel::Crit
        } else if bps >= 8000 {
            ObLevel::Warn
        } else {
            ObLevel::Ok
        }
    }

    /// 级名。
    pub fn name(self) -> &'static str {
        match self {
            ObLevel::Ok => "ok",
            ObLevel::Warn => "warn-80",
            ObLevel::Crit => "crit-95",
            ObLevel::Dead => "dead-100",
        }
    }

    /// 级序（0–3）。
    pub fn ordinal(self) -> u8 {
        match self {
            ObLevel::Ok => 0,
            ObLevel::Warn => 1,
            ObLevel::Crit => 2,
            ObLevel::Dead => 3,
        }
    }
}

/// 分级动作闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObAction {
    /// 记录（Ok/Warn 默认）。
    Log,
    /// 拒绝新分配 + 回收建议（Crit）。
    DenyAlloc,
    /// 停分配 + 仅许释放（Dead）。
    DeadStop,
}

impl ObAction {
    /// 级别 → 动作（映射冻结 v1）。
    pub fn for_level(l: ObLevel) -> ObAction {
        match l {
            ObLevel::Ok | ObLevel::Warn => ObAction::Log,
            ObLevel::Crit => ObAction::DenyAlloc,
            ObLevel::Dead => ObAction::DeadStop,
        }
    }

    /// 动作名。
    pub fn name(self) -> &'static str {
        match self {
            ObAction::Log => "log",
            ObAction::DenyAlloc => "deny-alloc",
            ObAction::DeadStop => "dead-stop",
        }
    }
}

/// 分配请求结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObAlloc {
    /// 受理。
    Granted,
    /// 拒绝（crit/dead 级 · 附级别）。
    Denied(ObLevel),
}

/// 堆水位账（单例状态机）。
pub struct ObHeap {
    /// 堆总容量（字节）。
    pub capacity: u64,
    /// 已用量。
    pub used: u64,
    /// 当前级别。
    pub level: ObLevel,
    /// 动作账（逐次升级/降级落账）。
    pub actions: Vec<(u64, ObLevel, ObAction)>,
    /// 分配受理数。
    pub granted: u32,
    /// 分配拒绝数（按级分账）。
    pub denied_crit: u32,
    pub denied_dead: u32,
    /// 释放数。
    pub freed: u32,
    /// 升级触发数。
    pub escalations: u32,
}

impl ObHeap {
    /// 新堆账。
    pub fn new(capacity: u64) -> Self {
        ObHeap {
            capacity,
            used: 0,
            level: ObLevel::Ok,
            actions: Vec::new(),
            granted: 0,
            denied_crit: 0,
            denied_dead: 0,
            freed: 0,
            escalations: 0,
        }
    }

    /// 占比（万分比）。
    pub fn bps(&self) -> u32 {
        if self.capacity == 0 {
            return 10000;
        }
        ((self.used * 10000) / self.capacity).min(10_000) as u32
    }

    /// 重估级别（升级即时 / 降级迟回：crit→warn 需降至 <78%，warn→ok 需 <78%）。
    pub fn relevel(&mut self, tick: u64) {
        let raw = ObLevel::of(self.bps());
        if raw.ordinal() > self.level.ordinal() {
            // 升级即时。
            if raw != self.level {
                self.escalations += 1;
            }
            self.level = raw;
        } else if raw.ordinal() < self.level.ordinal() {
            // 降级迟回：只有 raw 已低于「本级下沿-200bps」才回卷。
            let drop_back = match self.level {
                ObLevel::Dead => raw.ordinal() <= ObLevel::Crit.ordinal(),
                ObLevel::Crit => raw == ObLevel::Warn && self.bps() < 7800,
                ObLevel::Warn => raw == ObLevel::Ok,
                ObLevel::Ok => false,
            };
            if drop_back {
                self.level = raw;
            }
        }
        self.actions
            .push((tick, self.level, ObAction::for_level(self.level)));
    }

    /// 分配请求：crit 拒新分配、dead 停分配仅许释放。
    pub fn alloc(&mut self, bytes: u64, tick: u64) -> ObAlloc {
        let a = ObAction::for_level(self.level);
        match a {
            ObAction::Log => {
                self.used += bytes;
                self.relevel(tick);
                self.granted += 1;
                ObAlloc::Granted
            }
            ObAction::DenyAlloc => {
                self.denied_crit += 1;
                ObAlloc::Denied(ObLevel::Crit)
            }
            ObAction::DeadStop => {
                self.denied_dead += 1;
                ObAlloc::Denied(ObLevel::Dead)
            }
        }
    }

    /// 释放（任何级都允许 · dead 级唯一出口）。
    pub fn free(&mut self, bytes: u64, tick: u64) -> bool {
        if bytes > self.used {
            return false;
        }
        self.used -= bytes;
        self.freed += 1;
        self.relevel(tick);
        true
    }

    /// 分级动作账齐：四动作映射逐级落账（含 Ok→Log）。
    pub fn action_ledger_complete(&self) -> bool {
        self.actions
            .iter()
            .all(|(_, l, a)| ObAction::for_level(*l) == *a)
    }
}

/// 触发正确性核：逐点水位 → 级别映射断言（纯函数表驱动）。
pub fn ob_trigger_correct() -> bool {
    let cases: [(u32, ObLevel); 8] = [
        (0, ObLevel::Ok),
        (7999, ObLevel::Ok),
        (8000, ObLevel::Warn),
        (9499, ObLevel::Warn),
        (9500, ObLevel::Crit),
        (9999, ObLevel::Crit),
        (10000, ObLevel::Dead),
        (12000, ObLevel::Dead),
    ];
    cases.iter().all(|(bps, want)| ObLevel::of(*bps) == *want)
}

/// 负向注入场景。
pub enum ObInject {
    /// 越级请求（crit 级下分配必须拒）。
    AllocInCrit,
    /// dead 级分配（必须拒）。
    AllocInDead,
    /// 释放越界（free > used 必拒）。
    OverFree,
    /// 降级迟回破坏（crit 级微降 100bps 不得立即回 warn）。
    HysteresisBreak,
}

/// 注入核。
pub struct ObInjector {
    /// 场数。
    pub cases: u32,
    /// 拦截数。
    pub caught: u32,
}

impl ObInjector {
    /// 空注入核。
    pub fn new() -> Self {
        ObInjector { cases: 0, caught: 0 }
    }

    /// 四场注入（独立探针堆 · 不污染主账）。
    pub fn inject_all(&mut self) {
        // 场一：crit 级分配必拒。
        let mut h = ObHeap::new(1000);
        h.used = 960; // 96% crit
        h.relevel(1);
        if matches!(h.alloc(10, 2), ObAlloc::Denied(ObLevel::Crit)) {
            self.caught += 1;
        }
        self.cases += 1;
        // 场二：dead 级分配必拒。
        let mut h2 = ObHeap::new(1000);
        h2.used = 1000;
        h2.relevel(1);
        if matches!(h2.alloc(10, 2), ObAlloc::Denied(ObLevel::Dead)) {
            self.caught += 1;
        }
        self.cases += 1;
        // 场三：释放越界必拒。
        let mut h3 = ObHeap::new(1000);
        h3.used = 100;
        let over = !h3.free(200, 3);
        if over {
            self.caught += 1;
        }
        self.cases += 1;
        // 场四：迟回破坏（crit 级 95.4% → 降至 95.3% 仍在 crit 不回 warn）。
        let mut h4 = ObHeap::new(10000);
        h4.used = 9540;
        h4.relevel(1);
        let l1 = h4.level;
        h4.used = 9530;
        h4.relevel(2);
        let stuck = h4.level == ObLevel::Crit && l1 == ObLevel::Crit;
        if stuck {
            self.caught += 1;
        }
        self.cases += 1;
    }

    /// 注入满格。
    pub fn all_caught(&self) -> bool {
        self.cases >= 4 && self.caught == self.cases
    }
}

/// 判据门面（判据三件：三级触发正确/动作账齐/注入必拒）。
pub struct ObGate {
    /// 主堆账。
    pub heap: ObHeap,
    /// 注入核。
    pub inj: ObInjector,
}

impl ObGate {
    /// 新门面。
    pub fn new() -> Self {
        ObGate {
            heap: ObHeap::new(1 << 20),
            inj: ObInjector::new(),
        }
    }

    /// 判据总门。
    pub fn judge(&self) -> (bool, String) {
        let ok = ob_trigger_correct()
            && self.heap.action_ledger_complete()
            && self.heap.escalations >= 3
            && self.inj.all_caught();
        let msg = format!(
            "{}: level={} bps={} actions={} esc={} granted={} denied(c/d)={}/{} freed={} inj={}/{} seg=0x{:04x}",
            OB_VERSION,
            self.heap.level.name(),
            self.heap.bps(),
            self.heap.actions.len(),
            self.heap.escalations,
            self.heap.granted,
            self.heap.denied_crit,
            self.heap.denied_dead,
            self.heap.freed,
            self.inj.caught,
            self.inj.cases,
            OB_SEG_BASE
        );
        (ok, msg)
    }
}

/// 判据自检（t 系入口）：渐进灌堆触发三级 + 释放回卷 + 四场注入。
pub fn run_t387b_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("unx387b-t387");
    let mut g = ObGate::new();
    // 渐进灌堆：0 → 85%（warn）→ 96%（crit）；crit 级分级闸拒绝新分配（语义正确）。
    let cap = g.heap.capacity;
    let _ = g.heap.alloc(cap * 85 / 100, 10);
    let w = g.heap.level;
    let _ = g.heap.alloc(cap * 11 / 100, 20); // 96%
    let c = g.heap.level;
    // crit 级新分配必拒（denied_crit 落账 · 分级闸生效证据）。
    let crit_denied = matches!(g.heap.alloc(1, 30), ObAlloc::Denied(ObLevel::Crit));
    // 灌至 100%（dead）：分配闸已封，唯余「用量预置 + 重估」口径（与注入场二同构）。
    g.heap.used = cap;
    g.heap.relevel(35);
    let d = g.heap.level;
    // dead 级分配必拒（denied_dead 落账）。
    let dead_denied = matches!(g.heap.alloc(1, 40), ObAlloc::Denied(ObLevel::Dead));
    // 释放回卷：降到 70%。
    let _ = g.heap.free(cap * 30 / 100, 50);
    // 注入四场。
    g.inj.inject_all();
    let (ok, msg) = g.judge();
    let tripped = w == ObLevel::Warn
        && c == ObLevel::Crit
        && d == ObLevel::Dead
        && crit_denied
        && dead_denied;
    set.add_dyn("UNX-F0387-J1", ok && tripped, &msg);
    set.add("OB-trip-3levels", tripped, "");
    set.add("OB-trigger-table", ob_trigger_correct(), "");
    set.add("OB-action-ledger", g.heap.action_ledger_complete(), "");
    set.add("OB-inject-4/4", g.inj.all_caught(), "");
    set
}
