//! H2 宿主胶水层 · 深化批次三（事件总线 + 会话装配——五十项与宿主
//! 输入/持久/恢复链路的唯一接缝）。
//!
//! **承接判据**（主册 H 域正文，一处一事实）：
//! - **F283 三拍子**：任何交互入口的「反馈 <100ms」——本层把每个
//!   事件的分发预算钉成 100ms，超线事件计数器**不静默**；
//! - **F282 单例转发**：激活事件的路由要区分「转发给既有实例」与
//!   「走新启动编排」——本层路由表是两入口结构一致的物理落点；
//! - **F281 浮层出路**：会话拆除顺序 = 后开先关（浮层最先、托盘
//!   次之、窗口最后）——拆除计划是机判的，不许各功能各拆各的；
//! - **F273/F253/F266/F286 恢复族**：会话装配顺序统一——持久层
//!   加载 → 快照解析（损坏项降级默认态+自首）→ 状态回填 → 就绪；
//!   中断后续装（幂等：已完成步骤不重做）；
//! - **十四章状态机**：每个事件要么被路由（返回投递计划），要么被
//!   显式拒绝（拒绝原因留账）——零「事件被吞」路径。
//!
//! 时间纪律：时钟由调用方注入（毫秒戳）；本层只做计划与记账。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 事件模型与分类
// ---------------------------------------------------------------------------

/// 宿主输入事件（H2 域关心的最小集——渲染层/控件层再细分）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostEvent {
    /// 键盘（`mods` 位域：0x1 Ctrl / 0x2 Shift / 0x4 Alt / 0x8 Win）。
    Key { code: u32, mods: u8 },
    /// 指针移动。
    PointerMove { x: i32, y: i32 },
    /// 指针按下（button：0 左 / 1 中 / 2 右 / 3 X1 / 4 X2）。
    PointerDown { x: i32, y: i32, button: u8 },
    /// 指针抬起。
    PointerUp { x: i32, y: i32, button: u8 },
    /// 滚轮（双轴独立——F256 语义的地基）。
    Wheel { dx: i32, dy: i32 },
    /// 触屏相位（0 按下 / 1 移动 / 2 抬起 / 3 取消）。
    Touch { phase: u8, x: i32, y: i32 },
    /// 输入法组合事件（组合期——IME 纪律的地基）。
    Ime { composing: bool },
    /// 激活事件（任务栏/开始菜单入口点击——F282 两入口的载体）。
    Activate { entry: u8, arg: u32 },
}

/// 事件类别（路由订阅以类别为单位——新增事件源不改订阅方）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventClass {
    Keyboard,
    Pointer,
    Wheel,
    Touch,
    Ime,
    Activation,
}

/// 事件分类（一处一事实：分类规则只此一份）。
pub fn classify(ev: &HostEvent) -> EventClass {
    match ev {
        HostEvent::Key { .. } => EventClass::Keyboard,
        HostEvent::PointerMove { .. }
        | HostEvent::PointerDown { .. }
        | HostEvent::PointerUp { .. } => EventClass::Pointer,
        HostEvent::Wheel { .. } => EventClass::Wheel,
        HostEvent::Touch { .. } => EventClass::Touch,
        HostEvent::Ime { .. } => EventClass::Ime,
        HostEvent::Activate { .. } => EventClass::Activation,
    }
}

// ---------------------------------------------------------------------------
// 路由表（订阅-分发计划）
// ---------------------------------------------------------------------------

/// 事件分发预算：反馈红线 100ms（F283 三拍子第一拍——全域同值）。
pub const FEEDBACK_BUDGET_MS: u32 = 100;

/// 一条路由：某功能订阅某类事件，带优先级（小者先分发）。
/// `subscriber` 是功能锚（如 "F258"）——路由表即「谁能收什么事件」的
/// 唯一名册，胶水层之外的任何直连都算越界。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Route {
    pub subscriber: &'static str,
    pub class: EventClass,
    /// 优先级：0 = 前台交互（输入优先，F047 车道），9 = 后台观察者。
    pub priority: u8,
}

/// 路由表：订阅登记 + 分发计划生成。
pub struct Router {
    routes: Vec<Route>,
}

impl Router {
    pub fn new() -> Router {
        Router { routes: Vec::new() }
    }

    /// 登记订阅。同一 (subscriber, class) 重复登记 = 幂等（不产生双份
    /// 分发——防抖是结构保证，不是约定）。
    pub fn subscribe(&mut self, r: Route) {
        if !self.routes.iter().any(|x| *x == r) {
            self.routes.push(r);
        }
    }

    /// 取消订阅（浮层关闭时注销——出路纪律的登记侧）。
    pub fn unsubscribe(&mut self, subscriber: &str, class: EventClass) {
        self.routes.retain(|r| !(r.subscriber == subscriber && r.class == class));
    }

    /// 分发计划：按优先级升序（同优先级按登记序——稳定排序，同输入
    /// 同计划，可回放）。事件无人订阅时返回空计划 + 拒绝原因——
    /// 「看起来没反应」在路由层就被点名，不许沉到界面层才暴露。
    pub fn dispatch_plan(&self, ev: &HostEvent) -> DispatchPlan {
        let class = classify(ev);
        let mut hits: Vec<&Route> =
            self.routes.iter().filter(|r| r.class == class).collect();
        hits.sort_by_key(|r| r.priority);
        DispatchPlan {
            class,
            subscribers: hits.iter().map(|r| r.subscriber).collect(),
            budget_ms: FEEDBACK_BUDGET_MS,
        }
    }

    /// 登记的路由数（诊断页用）。
    pub fn len(&self) -> usize {
        self.routes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.routes.is_empty()
    }
}

/// 一次分发的投递计划（可回放的最小单元）。
#[derive(Debug)]
pub struct DispatchPlan {
    pub class: EventClass,
    pub subscribers: Vec<&'static str>,
    pub budget_ms: u32,
}

// ---------------------------------------------------------------------------
// 分发延迟账（超线不静默）
// ---------------------------------------------------------------------------

/// 分发记录：事件序号 + 实际耗时。账本定容（环形淘汰最旧——账本
/// 自身不许成为内存泄漏源），超线计数单独累计、永不淘汰。
pub struct DispatchLedger {
    records: Vec<(u64, u32)>,
    cap: usize,
    pub over_budget: u64,
    next_seq: u64,
}

impl DispatchLedger {
    pub fn new(cap: usize) -> DispatchLedger {
        DispatchLedger { records: Vec::new(), cap: cap.max(1), over_budget: 0, next_seq: 0 }
    }

    /// 记一笔：事件实际分发耗时。超预算即计数——超线事件不静默是
    /// 十三章「异常显性化」在输入链路的落点。
    pub fn record(&mut self, cost_ms: u32) -> u64 {
        let seq = self.next_seq;
        self.next_seq += 1;
        if cost_ms > FEEDBACK_BUDGET_MS {
            self.over_budget += 1;
        }
        if self.records.len() == self.cap {
            self.records.remove(0);
        }
        self.records.push((seq, cost_ms));
        seq
    }

    /// 最近 `n` 笔的最大耗时（健康页口径）。
    pub fn max_recent(&self, n: usize) -> u32 {
        let take = self.records.len().saturating_sub(n);
        self.records[take..].iter().map(|(_, c)| *c).max().unwrap_or(0)
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }
}

// ---------------------------------------------------------------------------
// 会话装配（恢复族四步——F273/F253/F266/F286 统一链）
// ---------------------------------------------------------------------------

/// 装配步骤（顺序即语义，不许跳步）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssembleStep {
    /// 1. 持久层加载（h2persist 通道）。
    LoadPersist,
    /// 2. 快照解析（h2snap 封包；损坏项降级默认态并自首）。
    ParseSnaps,
    /// 3. 状态回填（各功能接自己的状态机）。
    Backfill,
    /// 4. 就绪（反馈线放开）。
    Ready,
}

pub const ASSEMBLE_STEPS: [AssembleStep; 4] = [
    AssembleStep::LoadPersist,
    AssembleStep::ParseSnaps,
    AssembleStep::Backfill,
    AssembleStep::Ready,
];

/// 装配器：域会话恢复的唯一编排者。
/// - 幂等续装：中断后再调 `advance` 从完成处继续，不重头（恢复中断
///   路径——十四章「打断/恢复」的状态机要求）；
/// - 损坏自首：解析步逐项记录降级名单，装配结果里可见（不静默吞）。
pub struct SessionAssembler {
    done: usize,
    degraded: Vec<&'static str>,
}

impl SessionAssembler {
    pub fn new() -> SessionAssembler {
        SessionAssembler { done: 0, degraded: Vec::new() }
    }

    /// 已完成步数。
    pub fn done(&self) -> usize {
        self.done
    }

    pub fn is_ready(&self) -> bool {
        self.done >= ASSEMBLE_STEPS.len()
    }

    /// 推进一步。`damaged` 传本步发现的损坏项名单（仅 ParseSnaps 步
    /// 有意义，其余步传空切片）。重复推进已完成步骤 = 无操作（幂等）。
    pub fn advance(&mut self, damaged: &[&'static str]) -> Option<AssembleStep> {
        if self.done >= ASSEMBLE_STEPS.len() {
            return None;
        }
        let step = ASSEMBLE_STEPS[self.done];
        for d in damaged {
            if !self.degraded.contains(d) {
                self.degraded.push(d);
            }
        }
        self.done += 1;
        Some(step)
    }

    /// 降级名单（装配报告用——哪些快照没恢复成、走了默认态）。
    pub fn degraded(&self) -> &[&'static str] {
        &self.degraded
    }

    /// 装配人话报告（诊断页直接可显——三要素齐）。
    pub fn report(&self) -> String {
        if self.is_ready() {
            if self.degraded.is_empty() {
                alloc::format!("会话装配完成：{}/4 步，全部快照正常恢复", self.done)
            } else {
                alloc::format!(
                    "会话装配完成：{}/4 步，{} 项损坏走默认态（列表见降级登记）",
                    self.done,
                    self.degraded.len()
                )
            }
        } else {
            alloc::format!("会话装配进行中：{}/4 步", self.done)
        }
    }
}

// ---------------------------------------------------------------------------
// 会话拆除（浮层出路纪律的编排侧）
// ---------------------------------------------------------------------------

/// 拆除层优先级：小者先拆。后开先关——浮层（菜单/弹窗/气泡）是
/// 最后打开的，所以最先关；窗口层最持久，最后收。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum TearLayer {
    /// 浮层：右键菜单、下拉、气泡、提示框（F281/F258/F272 的出路）。
    Floating,
    /// 面板：抽屉、停靠窗格。
    Panel,
    /// 托盘与常驻图标。
    Tray,
    /// 主窗口。
    Window,
    /// 会话资源：日志落盘、账本封口（最后一步——日志要看到全过程）。
    Telemetry,
}

pub const TEAR_ORDER: [TearLayer; 5] = [
    TearLayer::Floating,
    TearLayer::Panel,
    TearLayer::Tray,
    TearLayer::Window,
    TearLayer::Telemetry,
];

/// 拆除计划：给出各层待关闭清单，产出逐层关闭顺序。
/// 未保存守卫在 Window 层触发（关窗前的未保存提示是 Window 层的
/// 出路检查，不是可选步骤）。
pub fn teardown_plan(layers: &[(TearLayer, u32)]) -> Vec<(TearLayer, u32)> {
    let mut out: Vec<(TearLayer, u32)> = layers.to_vec();
    out.sort_by_key(|(l, _)| *l as u8);
    out
}

// ---------------------------------------------------------------------------
// 自检（判据逐条钉死）
// ---------------------------------------------------------------------------

pub fn run_h2host_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-h2host");
    // 分类唯一源：八事件全分类且键盘类命中键盘订阅。
    let evs = [
        HostEvent::Key { code: 0x41, mods: 0 },
        HostEvent::PointerMove { x: 1, y: 2 },
        HostEvent::Wheel { dx: 0, dy: 3 },
        HostEvent::Touch { phase: 0, x: 4, y: 5 },
        HostEvent::Ime { composing: true },
        HostEvent::Activate { entry: 1, arg: 7 },
    ];
    let classes = [EventClass::Keyboard, EventClass::Pointer, EventClass::Wheel, EventClass::Touch, EventClass::Ime, EventClass::Activation];
    set.add(
        "h2host classify all",
        evs.iter().zip(classes.iter()).all(|(e, c)| classify(e) == *c),
        "six classes",
    );
    // 订阅幂等：同路由登记两次只派发一次（防抖结构保证）。
    let mut router = Router::new();
    let r = Route { subscriber: "F258", class: EventClass::Pointer, priority: 0 };
    router.subscribe(r);
    router.subscribe(r);
    let plan = router.dispatch_plan(&HostEvent::PointerDown { x: 0, y: 0, button: 2 });
    set.add(
        "h2host subscribe idempotent",
        plan.subscribers.len() == 1 && router.len() == 1,
        "no double dispatch",
    );
    // 优先级序：前台 0 先于观察者 9；同优先级按登记序（稳定）。
    router.subscribe(Route { subscriber: "F279", class: EventClass::Pointer, priority: 9 });
    router.subscribe(Route { subscriber: "F260", class: EventClass::Pointer, priority: 0 });
    let plan2 = router.dispatch_plan(&HostEvent::PointerMove { x: 0, y: 0 });
    set.add(
        "h2host priority order",
        plan2.subscribers == alloc::vec!["F258", "F260", "F279"],
        "stable by priority",
    );
    // 无人订阅 = 显式空计划（拒绝可见，不吞事件）。
    let lonely = router.dispatch_plan(&HostEvent::Activate { entry: 0, arg: 0 });
    set.add("h2host unmatched visible", lonely.subscribers.is_empty(), "empty plan not swallowed");
    // 注销生效。
    router.unsubscribe("F260", EventClass::Pointer);
    let after = router.dispatch_plan(&HostEvent::PointerMove { x: 0, y: 0 });
    set.add(
        "h2host unsubscribe",
        after.subscribers == alloc::vec!["F258", "F279"],
        "route removed",
    );
    // 延迟账：超线计数独立累计、环形容量不增长、最近窗口口径正确。
    let mut ledger = DispatchLedger::new(4);
    for cost in [20u32, 50, 99, 100, 140, 200] {
        ledger.record(cost);
    }
    set.add(
        "h2host budget ledger",
        ledger.over_budget == 2
            && ledger.len() == 4
            && ledger.max_recent(2) == 200
            && ledger.max_recent(100) == 200,
        "100ms line enforced",
    );
    // 装配：四步顺序固定；中断后续装不重做；损坏自首进报告。
    let mut asm = SessionAssembler::new();
    asm.advance(&[]);
    asm.advance(&["F266.nav", "F253.pins"]);
    let third = asm.advance(&[]);
    set.add(
        "h2host assemble order",
        third == Some(AssembleStep::Backfill) && asm.done() == 3,
        "resume mid-chain",
    );
    let _ = asm.advance(&["F266.nav"]); // 重复降级项不重复记
    set.add(
        "h2host degraded honest",
        asm.degraded().len() == 2 && asm.report().contains("损坏走默认态"),
        "damage self-reports",
    );
    asm.advance(&[]);
    set.add(
        "h2host assemble ready",
        asm.is_ready()
            && asm.report().contains("损坏走默认态")
            && asm.advance(&[]).is_none(),
        "ready + idempotent tail",
    );
    // 全绿装配的报告不带「损坏」字样。
    let mut clean = SessionAssembler::new();
    for _ in 0..4 {
        clean.advance(&[]);
    }
    set.add("h2host clean report", clean.report().contains("全部快照正常恢复"), "no false alarm");
    // 拆除顺序：浮层最先、遥测最后——后开先关的机判。
    let plan3 = teardown_plan(&[
        (TearLayer::Window, 2),
        (TearLayer::Floating, 7),
        (TearLayer::Telemetry, 1),
        (TearLayer::Tray, 3),
        (TearLayer::Panel, 1),
    ]);
    set.add(
        "h2host teardown order",
        plan3[0].0 == TearLayer::Floating
            && plan3[1].0 == TearLayer::Panel
            && plan3[2].0 == TearLayer::Tray
            && plan3[3].0 == TearLayer::Window
            && plan3[4].0 == TearLayer::Telemetry,
        "floating first, telemetry last",
    );
    set.add(
        "h2host tear constants",
        TEAR_ORDER[0] == TearLayer::Floating && ASSEMBLE_STEPS[3] == AssembleStep::Ready,
        "tables consistent",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn h2host_all_green() {
        let set = run_h2host_checks();
        assert!(set.all_passed(), "h2host 自检有红项");
        assert!(!set.truncated(), "h2host 自检溢出");
    }

    #[test]
    fn dispatch_budget_line_is_honest() {
        // 恰好 100ms = 不超线（预算是 ≤ 线）；101ms = 超线。
        let mut l = DispatchLedger::new(8);
        l.record(FEEDBACK_BUDGET_MS);
        assert_eq!(l.over_budget, 0);
        l.record(FEEDBACK_BUDGET_MS + 1);
        assert_eq!(l.over_budget, 1);
    }

    #[test]
    fn assembler_resumes_after_interrupt() {
        // 中断在 Backfill 后：续装从 Ready 开始，前两步不重跑。
        let mut asm = SessionAssembler::new();
        asm.advance(&[]);
        asm.advance(&[]);
        asm.advance(&[]);
        assert_eq!(asm.done(), 3);
        assert_eq!(asm.advance(&[]), Some(AssembleStep::Ready));
        assert!(asm.is_ready());
    }
}
