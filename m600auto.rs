//! m600auto — VARIX-M600 AI-16 自动化与脚本域 (F376~F400)
//!
//! 全局自动化引擎/事件总线公开/触发器词典/动作库/可视化流程编辑/
//! 脚本沙盒/定时任务指挥/文件 watcher 服务/热键总机/快捷指令库/
//! 跨应用编排/宏录制器/自动化审计日志/失败重试策略/自动化市场/
//! 模板画廊/条件逻辑画布/变量系统/自动化测试器/系统状态查询 API/
//! 无人值守模式/自动化安全门/速率限制器/示例配方集/自动化年报。
//!
//! 硬约束：no_std / 无 alloc / 无浮点（全部 permille/定点）/ 纯逻辑。

use crate::checks::CheckSet;

// ===========================================================================
// F376 — 全局自动化引擎：规则匹配→执行
// ===========================================================================

#[derive(Clone, Copy)]
pub struct Rule {
    pub trigger_index: u16,
    pub action_index: u16,
    pub enabled: bool,
}

/// 引擎步进：给定事件匹配出将要执行的动作下标（最多 4 条/步）。
pub fn engine_step(rules: &[Rule], event_trigger: u16, out: &mut [u16; 4]) -> usize {
    let mut n = 0usize;
    for r in rules {
        if r.enabled && r.trigger_index == event_trigger && n < out.len() {
            out[n] = r.action_index;
            n += 1;
        }
    }
    n
}

// ===========================================================================
// F377 — 事件总线公开：订阅/发布
// ===========================================================================

pub const EVENT_KINDS: [&str; 8] =
    ["boot", "app-open", "app-close", "file-change", "timer", "hotkey", "net-up", "net-down"];

/// 总线容量：最多 8 个事件槽，环形覆盖。
pub const BUS_SLOTS: usize = 8;

#[derive(Clone, Copy)]
pub struct EventBus {
    ring: [u16; BUS_SLOTS],
    head: usize,
    len: usize,
    dropped: usize,
}

impl EventBus {
    pub const fn new() -> EventBus {
        EventBus { ring: [0; BUS_SLOTS], head: 0, len: 0, dropped: 0 }
    }
    pub fn publish(&mut self, kind_index: u16) {
        self.ring[self.head] = kind_index;
        self.head = (self.head + 1) % BUS_SLOTS;
        if self.len < BUS_SLOTS {
            self.len += 1;
        } else {
            self.dropped += 1;
        }
    }
    /// 最新（最后发布）的事件。
    pub fn latest(&self) -> Option<u16> {
        if self.len == 0 {
            None
        } else {
            Some(self.ring[(self.head + BUS_SLOTS - 1) % BUS_SLOTS])
        }
    }
    pub fn dropped(&self) -> usize {
        self.dropped
    }
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn is_full(&self) -> bool {
        self.len == BUS_SLOTS
    }
}

// ===========================================================================
// F378 — 触发器词典：触发器标识统一
// ===========================================================================

pub const TRIGGERS: [&str; 6] = ["on-event", "on-schedule", "on-file", "on-hotkey", "on-boot", "on-battery"];

pub fn trigger_known(name: &str) -> bool {
    TRIGGERS.contains(&name)
}

// ===========================================================================
// F379 — 动作库：动作标识统一
// ===========================================================================

pub const ACTIONS: [&str; 6] =
    ["open-app", "notify", "run-script", "set-var", "wait", "abort"];

pub fn action_known(name: &str) -> bool {
    ACTIONS.contains(&name)
}

// ===========================================================================
// F380 — 可视化流程编辑：节点连线成环检测
// ===========================================================================

/// 有向图（邻接为 fixed 2-out）是否含环：函数式三色染色（0 白 1 灰 2 黑）。
pub const FLOW_MAX_NODES: usize = 16;

/// edges: 每节点最多 2 条出边，u8::MAX 表示无。
pub fn flow_has_cycle(edges: &[[u8; 2]; FLOW_MAX_NODES], node_count: usize) -> bool {
    #[derive(Clone, Copy, PartialEq)]
    enum Color { White, Gray, Black }
    let mut color = [Color::White; FLOW_MAX_NODES];

    // 迭代式 DFS，避免递归（no_std 友好）。
    for start in 0..node_count {
        if color[start] != Color::White {
            continue;
        }
        let mut stack = [0u8; FLOW_MAX_NODES * 2];
        let mut sp = 0usize;
        stack[sp] = start as u8;
        sp += 1;
        color[start] = Color::Gray;
        while sp > 0 {
            sp -= 1;
            let cur = stack[sp] as usize;
            let mut pushed = false;
            for &e in &edges[cur] {
                if e == u8::MAX {
                    continue;
                }
                let e = e as usize;
                if color[e] == Color::Gray {
                    return true;
                }
                if color[e] == Color::White && sp < stack.len() {
                    color[e] = Color::Gray;
                    stack[sp] = e as u8;
                    sp += 1;
                    pushed = true;
                }
            }
            if !pushed {
                color[cur] = Color::Black;
                // 重新压回父级语义：直接标黑继续。
            }
        }
    }
    false
}

// ===========================================================================
// F381 — 脚本沙盒：指令数/内存/调用面三限
// ===========================================================================

pub const SCRIPT_MAX_STEPS: u32 = 100_000;
pub const SCRIPT_MAX_MEM_KIB: u32 = 256;
pub const SCRIPT_ALLOW_SYS环: u8 = 0; // 系统调用白标数（0=纯计算）

/// 沙盒放行：步数、内存双限内，且未请求越权调用面。
pub fn script_sandbox_ok(steps: u32, mem_kib: u32, syscalls_used: u8) -> bool {
    steps <= SCRIPT_MAX_STEPS && mem_kib <= SCRIPT_MAX_MEM_KIB && syscalls_used <= SCRIPT_ALLOW_SYS环
}

// ===========================================================================
// F382 — 定时任务指挥：cron 摘要（分/时/日）
// ===========================================================================

#[derive(Clone, Copy)]
pub struct Schedule {
    pub minute: u8, // 0~59
    pub hour: u8,   // 0~23
    pub day_mask: u16, // bit0=周日 … bit6=周六
}

impl Schedule {
    pub fn sane(&self) -> bool {
        self.minute < 60 && self.hour < 24 && self.day_mask & !0x7F == 0 && self.day_mask != 0
    }
    /// 某时刻是否命中。
    pub fn hits(&self, minute: u8, hour: u8, weekday: u8) -> bool {
        self.sane()
            && self.minute == minute
            && self.hour == hour
            && weekday < 7
            && self.day_mask & (1 << weekday) != 0
    }
}

pub const SCHEDULE_DAILY_NOON: Schedule = Schedule { minute: 0, hour: 12, day_mask: 0x7F };

// ===========================================================================
// F383 — 文件 watcher 服务：去抖 + 过滤
// ===========================================================================

pub const WATCH_DEBOUNCE_MS: u32 = 250;

#[derive(Clone, Copy)]
pub struct WatchEvent {
    pub path_hash: u32,
    pub at_ms: u32,
}

/// 去抖：同一路径在窗口内只算一次。
pub fn watch_debounce(events: &[WatchEvent], window_ms: u32) -> usize {
    // (hash, last_ms) 槽表，容量 8。
    let mut last_by_slot: [(u32, u32); 8] = [(0, 0); 8];
    let mut used = 0usize;
    let mut kept = 0usize;
    for e in events {
        let mut found = false;
        let mut dup = false;
        for i in 0..used {
            if last_by_slot[i].0 == e.path_hash {
                found = true;
                if e.at_ms.saturating_sub(last_by_slot[i].1) < window_ms {
                    dup = true;
                } else {
                    last_by_slot[i].1 = e.at_ms;
                }
                break;
            }
        }
        if dup {
            continue;
        }
        if !found && used < last_by_slot.len() {
            last_by_slot[used] = (e.path_hash, e.at_ms);
            used += 1;
        }
        kept += 1;
    }
    kept
}

// ===========================================================================
// F384 — 热键总机：冲突检测
// ===========================================================================

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Hotkey {
    pub mods: u8, // bit0=ctrl bit1=alt bit2=shift bit3=meta
    pub key: u8,
}

/// 无冲突：任何两条热键不完全同键。
pub fn hotkey_conflict_free(keys: &[Hotkey]) -> bool {
    for i in 0..keys.len() {
        for j in (i + 1)..keys.len() {
            if keys[i] == keys[j] {
                return false;
            }
        }
    }
    true
}

// ===========================================================================
// F385 — 快捷指令库：指令由动作序列组成
// ===========================================================================

#[derive(Clone, Copy)]
pub struct Shortcut {
    pub name: &'static str,
    pub steps: usize,
}

pub fn shortcut_valid(s: &Shortcut) -> bool {
    !s.name.is_empty() && s.steps > 0 && s.steps <= 32
}

// ===========================================================================
// F386 — 跨应用编排：编排深度上限
// ===========================================================================

pub const ORCHESTRATION_MAX_DEPTH: u8 = 8;

/// 编排计划合法：深度不超限且至少涉及两个应用。
pub fn orchestration_ok(depth: u8, apps_involved: u8) -> bool {
    depth <= ORCHESTRATION_MAX_DEPTH && apps_involved >= 2
}

// ===========================================================================
// F387 — 宏录制器：录制事件回放一致性
// ===========================================================================

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct MacroStep {
    pub key: u8,
    pub delay_ms: u16,
}

/// 回放一致：序列逐位相等（长度也一致）。
pub fn macro_replay_matches(recorded: &[MacroStep], replayed: &[MacroStep]) -> bool {
    recorded.len() == replayed.len()
        && recorded.iter().zip(replayed.iter()).all(|(a, b)| a == b)
}

// ===========================================================================
// F388 — 自动化审计日志：环形审计
// ===========================================================================

pub const AUDIT_SLOTS: usize = 16;

#[derive(Clone, Copy)]
pub struct AuditLog {
    ring: [(u32, bool); AUDIT_SLOTS], // (rule_id, allowed)
    head: usize,
    len: usize,
}

impl AuditLog {
    pub const fn new() -> AuditLog {
        AuditLog { ring: [(0, false); AUDIT_SLOTS], head: 0, len: 0 }
    }
    pub fn record(&mut self, rule_id: u32, allowed: bool) {
        self.ring[self.head] = (rule_id, allowed);
        self.head = (self.head + 1) % AUDIT_SLOTS;
        if self.len < AUDIT_SLOTS {
            self.len += 1;
        }
    }
    pub fn len(&self) -> usize {
        self.len
    }
    /// 拒绝计数。
    pub fn denied(&self) -> usize {
        (0..self.len).filter(|&i| !self.ring[i].1).count()
    }
}

// ===========================================================================
// F389 — 失败重试策略：指数退避（定点）
// ===========================================================================

pub const RETRY_BASE_MS: u32 = 500;
pub const RETRY_MAX_ATTEMPTS: u32 = 5;

/// 第 attempt 次失败后的退避：base * 2^attempt，封顶 8s。
pub fn retry_backoff_ms(attempt: u32) -> u32 {
    let cap = 8_000u32;
    let mut ms = RETRY_BASE_MS;
    let mut i = 0;
    while i < attempt && ms < cap {
        ms = ms.saturating_mul(2);
        i += 1;
    }
    if ms > cap {
        cap
    } else {
        ms
    }
}

pub fn retry_give_up(attempt: u32) -> bool {
    attempt >= RETRY_MAX_ATTEMPTS
}

// ===========================================================================
// F390 — 自动化市场：上架安全审查门
// ===========================================================================

#[derive(Clone, Copy)]
pub struct MarketListing {
    pub manifest_ok: bool,
    pub sandbox_ok: bool,
    pub audit_clean: bool,
}

pub fn market_admittable(l: MarketListing) -> bool {
    l.manifest_ok && l.sandbox_ok && l.audit_clean
}

// ===========================================================================
// F391 — 模板画廊：内置模板齐备
// ===========================================================================

pub const AUTOMATION_TEMPLATES: [&str; 6] =
    ["morning-routine", "backup-nightly", "screenshot-organize", "focus-timer", "battery-saver", "meeting-mute"];

pub fn templates_complete(present: &[&str]) -> bool {
    AUTOMATION_TEMPLATES.iter().all(|t| present.contains(t))
}

// ===========================================================================
// F392 — 条件逻辑画布：条件求值（与/或/非）
// ===========================================================================

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Cond {
    True,
    False,
    And(u8, u8), // 子条件下标
    Or(u8, u8),
    Not(u8),
}

/// 在固定容量条件表上求值（防环：深度上限 8，深度超限判 false）。
pub fn eval_cond(tree: &[Cond], idx: u8) -> bool {
    eval_cond_depth(tree, idx, 0)
}

fn eval_cond_depth(tree: &[Cond], idx: u8, depth: u8) -> bool {
    if depth > 8 || (idx as usize) >= tree.len() {
        return false;
    }
    match tree[idx as usize] {
        Cond::True => true,
        Cond::False => false,
        Cond::And(a, b) => eval_cond_depth(tree, a, depth + 1) && eval_cond_depth(tree, b, depth + 1),
        Cond::Or(a, b) => eval_cond_depth(tree, a, depth + 1) || eval_cond_depth(tree, b, depth + 1),
        Cond::Not(a) => !eval_cond_depth(tree, a, depth + 1),
    }
}

// ===========================================================================
// F393 — 变量系统：固定容量变量表
// ===========================================================================

pub const VARS_MAX: usize = 16;

#[derive(Clone, Copy)]
pub struct VarTable {
    names: [&'static str; VARS_MAX],
    values: [i32; VARS_MAX],
    count: usize,
}

impl VarTable {
    pub const fn new() -> VarTable {
        VarTable { names: [""; VARS_MAX], values: [0; VARS_MAX], count: 0 }
    }
    pub fn set(&mut self, name: &'static str, value: i32) -> bool {
        for i in 0..self.count {
            if self.names[i] == name {
                self.values[i] = value;
                return true;
            }
        }
        if self.count < VARS_MAX {
            self.names[self.count] = name;
            self.values[self.count] = value;
            self.count += 1;
            true
        } else {
            false
        }
    }
    pub fn get(&self, name: &str) -> Option<i32> {
        for i in 0..self.count {
            if self.names[i] == name {
                return Some(self.values[i]);
            }
        }
        None
    }
    pub fn len(&self) -> usize {
        self.count
    }
}

// ===========================================================================
// F394 — 自动化测试器：干跑（不落副作用）
// ===========================================================================

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum StepKind {
    Query,
    Mutate,
}

/// 干跑放行：只允许 Query 类步骤进入干跑，Mutate 必须被拦下。
pub fn dry_run_allowed(kinds: &[StepKind]) -> bool {
    kinds.iter().all(|k| *k == StepKind::Query)
}

// ===========================================================================
// F395 — 系统状态查询 API：状态键统一
// ===========================================================================

pub const STATE_KEYS: [&str; 8] =
    ["battery", "cpu-load", "mem-free", "net-up", "front-app", "uptime", "storage", "volume"];

pub fn state_key_known(key: &str) -> bool {
    STATE_KEYS.contains(&key)
}

/// 负载 permille 合法范围。
pub fn load_permille_sane(v: u16) -> bool {
    v <= 1000
}

// ===========================================================================
// F396 — 无人值守模式：无人值守时的运行约束
// ===========================================================================

#[derive(Clone, Copy)]
pub struct UnattendedPolicy {
    pub allow_network: bool,
    pub allow_notifications: bool,
    pub max_runtime_min: u32,
}

/// 无人值守：不允许打扰（通知关）且有运行时长上限。
pub fn unattended_compliant(p: UnattendedPolicy) -> bool {
    !p.allow_notifications && p.max_runtime_min > 0 && p.max_runtime_min <= 24 * 60
}

// ===========================================================================
// F397 — 自动化安全门：危险动作需确认
// ===========================================================================

pub const DANGEROUS_ACTIONS: [&str; 4] = ["delete-files", "send-out", "spend", "grant-perm"];

/// 安全门裁决：危险动作必须带确认标记才放行。
pub fn safety_gate(action: &str, confirmed: bool) -> bool {
    if DANGEROUS_ACTIONS.contains(&action) {
        confirmed
    } else {
        true
    }
}

// ===========================================================================
// F398 — 速率限制器：令牌桶（定点）
// ===========================================================================

#[derive(Clone, Copy)]
pub struct TokenBucket {
    pub capacity: u32,
    pub tokens: u32,
    pub refill_per_min: u32,
}

impl TokenBucket {
    pub const fn new(capacity: u32, refill_per_min: u32) -> TokenBucket {
        TokenBucket { capacity, tokens: capacity, refill_per_min }
    }
    pub fn try_take(&mut self) -> bool {
        if self.tokens > 0 {
            self.tokens -= 1;
            true
        } else {
            false
        }
    }
    /// 每分钟补币（不超过容量）。
    pub fn refill_tick(&mut self) {
        self.tokens = if self.tokens + self.refill_per_min > self.capacity {
            self.capacity
        } else {
            self.tokens + self.refill_per_min
        };
    }
}

// ===========================================================================
// F399 — 示例配方集：配方清单齐备
// ===========================================================================

pub const SAMPLE_RECIPES: [&str; 5] =
    ["rename-batch", "wifi-switch-at-desk", "quiet-hours", "clipboard-chain", "low-power-alert"];

pub fn recipes_complete(present: &[&str]) -> bool {
    SAMPLE_RECIPES.iter().all(|r| present.contains(r))
}

// ===========================================================================
// F400 — 自动化年报：运行统计板块
// ===========================================================================

pub const AUTO_REPORT_SECTIONS: [&str; 4] = ["runs", "success-rate", "top-rules", "denied"];

/// 年报就绪：板块齐 + 成功率 permille ≥ 900。
pub fn auto_report_ready(sections: &[&str], success_permille: u16) -> bool {
    AUTO_REPORT_SECTIONS.iter().all(|s| sections.contains(s)) && success_permille >= 900
}

// ===========================================================================
// 域自检
// ===========================================================================

pub fn run_m600auto_checks() -> CheckSet {
    let mut set = CheckSet::new("m600auto");

    // F376 引擎
    let rules = [
        Rule { trigger_index: 3, action_index: 0, enabled: true },
        Rule { trigger_index: 3, action_index: 1, enabled: true },
        Rule { trigger_index: 3, action_index: 2, enabled: false },
        Rule { trigger_index: 5, action_index: 1, enabled: true },
    ];
    let mut out = [0u16; 4];
    let n = engine_step(&rules, 3, &mut out);
    set.add("F376 engine match", n == 2 && out[0] == 0 && out[1] == 1, "enabled only");

    // F377 事件总线
    let mut bus = EventBus::new();
    bus.publish(0);
    bus.publish(4);
    let mid = bus.latest();
    bus.publish(6);
    set.add(
        "F377 bus latest",
        mid == Some(4) && bus.latest() == Some(6),
        "ring order",
    );
    let mut bus2 = EventBus::new();
    for i in 0..(BUS_SLOTS as u16 + 3) {
        bus2.publish(i);
    }
    set.add(
        "F377 bus overwrite",
        bus2.dropped() == 3 && bus2.is_full() && bus2.latest() == Some(BUS_SLOTS as u16 + 2),
        "ring covers",
    );

    // F378 触发器词典
    set.add(
        "F378 triggers",
        trigger_known("on-event") && trigger_known("on-battery") && !trigger_known("on-magic"),
        "6 triggers",
    );

    // F379 动作库
    set.add(
        "F379 actions",
        action_known("open-app") && action_known("abort") && !action_known("format-disk"),
        "6 actions",
    );

    // F380 流程编辑成环检测
    let mut edges = [[u8::MAX; 2]; FLOW_MAX_NODES];
    edges[0] = [1, u8::MAX];
    edges[1] = [2, u8::MAX];
    edges[2] = [u8::MAX, u8::MAX];
    let acyclic = flow_has_cycle(&edges, 3);
    edges[2] = [0, u8::MAX];
    let cyclic = flow_has_cycle(&edges, 3);
    set.add("F380 flow cycle", !acyclic && cyclic, "dfs detect");

    // F381 脚本沙盒
    set.add(
        "F381 script sandbox",
        script_sandbox_ok(1_000, 64, 0)
            && !script_sandbox_ok(SCRIPT_MAX_STEPS + 1, 8, 0)
            && !script_sandbox_ok(8, 8, 1),
        "3 limits",
    );

    // F382 定时任务
    set.add(
        "F382 schedule sane",
        SCHEDULE_DAILY_NOON.sane()
            && SCHEDULE_DAILY_NOON.hits(0, 12, 3)
            && !SCHEDULE_DAILY_NOON.hits(1, 12, 3)
            && !SCHEDULE_DAILY_NOON.hits(0, 13, 3),
        "daily noon",
    );
    set.add(
        "F382 schedule bounds",
        !Schedule { minute: 60, hour: 12, day_mask: 0x7F }.sane()
            && !Schedule { minute: 0, hour: 24, day_mask: 0x7F }.sane()
            && !Schedule { minute: 0, hour: 12, day_mask: 0x1FF }.sane(),
        "bounds",
    );

    // F383 watcher 去抖
    let evs = [
        WatchEvent { path_hash: 7, at_ms: 0 },
        WatchEvent { path_hash: 7, at_ms: 100 },  // 窗口内 → 去抖
        WatchEvent { path_hash: 9, at_ms: 150 },
        WatchEvent { path_hash: 7, at_ms: 300 },  // 超窗 → 保留
    ];
    set.add("F383 watcher debounce", watch_debounce(&evs, WATCH_DEBOUNCE_MS) == 3, "250ms window");

    // F384 热键
    let hks = [
        Hotkey { mods: 0b0001, key: 0x41 },
        Hotkey { mods: 0b0010, key: 0x41 },
        Hotkey { mods: 0b0001, key: 0x41 },
    ];
    set.add(
        "F384 hotkey conflict",
        hotkey_conflict_free(&hks[..2]) && !hotkey_conflict_free(&hks),
        "dup detect",
    );

    // F385 快捷指令
    set.add(
        "F385 shortcuts",
        shortcut_valid(&Shortcut { name: "focus", steps: 4 })
            && !shortcut_valid(&Shortcut { name: "", steps: 1 })
            && !shortcut_valid(&Shortcut { name: "x", steps: 0 })
            && !shortcut_valid(&Shortcut { name: "y", steps: 33 }),
        "1..=32 steps",
    );

    // F386 跨应用编排
    set.add(
        "F386 orchestration",
        orchestration_ok(3, 2) && !orchestration_ok(9, 2) && !orchestration_ok(1, 1),
        "depth+apps",
    );

    // F387 宏录制
    let rec = [MacroStep { key: 1, delay_ms: 50 }, MacroStep { key: 2, delay_ms: 80 }];
    set.add(
        "F387 macro replay",
        macro_replay_matches(&rec, &rec)
            && !macro_replay_matches(&rec, &[MacroStep { key: 1, delay_ms: 50 }]),
        "exact match",
    );

    // F388 审计日志
    let mut audit = AuditLog::new();
    audit.record(1, true);
    audit.record(2, false);
    audit.record(3, true);
    let denied_mid = audit.denied();
    set.add("F388 audit log", audit.len() == 3 && denied_mid == 1, "denied count");

    // F389 重试退避
    set.add(
        "F389 retry backoff",
        retry_backoff_ms(0) == 500
            && retry_backoff_ms(1) == 1000
            && retry_backoff_ms(2) == 2000
            && retry_backoff_ms(5) == 8000
            && retry_give_up(5)
            && !retry_give_up(4),
        "exp cap 8s",
    );

    // F390 自动化市场
    set.add(
        "F390 market gate",
        market_admittable(MarketListing { manifest_ok: true, sandbox_ok: true, audit_clean: true })
            && !market_admittable(MarketListing { manifest_ok: true, sandbox_ok: false, audit_clean: true }),
        "3 gates",
    );

    // F391 模板画廊
    set.add("F391 templates", templates_complete(&AUTOMATION_TEMPLATES), "6 templates");

    // F392 条件画布
    let tree = [
        Cond::And(1, 2), // 0: T && F = false
        Cond::True,      // 1
        Cond::Not(3),    // 2: Not(True) = false
        Cond::True,      // 3
    ];
    let tree2 = [
        Cond::Or(1, 2),
        Cond::False,
        Cond::True,
        Cond::False,
    ];
    set.add(
        "F392 condition canvas",
        !eval_cond(&tree, 0) && eval_cond(&tree2, 0) && eval_cond(&tree, 1) && !eval_cond(&tree, 2),
        "and/or/not",
    );

    // F393 变量系统
    let mut vars = VarTable::new();
    vars.set("count", 3);
    vars.set("mode", 7);
    let first = vars.get("count");
    vars.set("count", 9);
    set.add(
        "F393 vars",
        first == Some(3) && vars.get("count") == Some(9) && vars.len() == 2,
        "set/get/overwrite",
    );
    let mut full = VarTable::new();
    let names = ["v0", "v1", "v2", "v3", "v4", "v5", "v6", "v7", "v8", "v9", "va", "vb", "vc", "vd", "ve", "vf"];
    let mut filled_all = true;
    for (i, n) in names.iter().enumerate() {
        filled_all = filled_all && full.set(n, i as i32);
    }
    set.add(
        "F393 vars capacity",
        filled_all && !full.set("x", 1) && full.len() == VARS_MAX,
        "16 cap",
    );

    // F394 干跑
    set.add(
        "F394 dry run",
        dry_run_allowed(&[StepKind::Query, StepKind::Query])
            && !dry_run_allowed(&[StepKind::Query, StepKind::Mutate]),
        "query only",
    );

    // F395 状态查询
    set.add(
        "F395 state api",
        state_key_known("battery") && state_key_known("uptime") && !state_key_known("secret"),
        "8 keys",
    );
    set.add("F395 load range", load_permille_sane(0) && load_permille_sane(1000) && !load_permille_sane(1001), "permille");

    // F396 无人值守
    set.add(
        "F396 unattended",
        unattended_compliant(UnattendedPolicy { allow_network: true, allow_notifications: false, max_runtime_min: 120 })
            && !unattended_compliant(UnattendedPolicy { allow_network: true, allow_notifications: true, max_runtime_min: 120 })
            && !unattended_compliant(UnattendedPolicy { allow_network: true, allow_notifications: false, max_runtime_min: 0 }),
        "quiet+budget",
    );

    // F397 安全门
    set.add(
        "F397 safety gate",
        safety_gate("notify", false) && !safety_gate("delete-files", false) && safety_gate("delete-files", true),
        "confirm dangerous",
    );

    // F398 速率限制
    let mut bucket = TokenBucket::new(3, 1);
    let t1 = bucket.try_take();
    let t2 = bucket.try_take();
    let t3 = bucket.try_take();
    let t4 = bucket.try_take();
    set.add(
        "F398 rate limit take",
        t1 && t2 && t3 && !t4,
        "3 tokens",
    );
    bucket.refill_tick();
    let t5 = bucket.try_take();
    set.add("F398 rate limit refill", t5, "refill");

    // F399 配方集
    set.add("F399 recipes", recipes_complete(&SAMPLE_RECIPES), "5 recipes");

    // F400 年报
    set.add(
        "F400 auto report",
        auto_report_ready(&AUTO_REPORT_SECTIONS, 950)
            && !auto_report_ready(&AUTO_REPORT_SECTIONS, 899)
            && !auto_report_ready(&["runs"], 999),
        "sections+rate",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f376_engine_disabled_skipped() {
        let rules = [Rule { trigger_index: 1, action_index: 0, enabled: false }];
        let mut out = [0u16; 4];
        assert_eq!(engine_step(&rules, 1, &mut out), 0);
    }

    #[test]
    fn f377_bus_ring_wrap() {
        let mut bus = EventBus::new();
        for i in 0..BUS_SLOTS as u16 {
            bus.publish(i);
        }
        assert_eq!(bus.dropped(), 0);
        bus.publish(99);
        assert_eq!(bus.dropped(), 1);
        assert_eq!(bus.latest(), Some(99));
    }

    #[test]
    fn f382_schedule_weekday_mask() {
        let wk = Schedule { minute: 30, hour: 8, day_mask: 0b0111110 }; // 周一~周五
        assert!(wk.hits(30, 8, 1));
        assert!(wk.hits(30, 8, 5));
        assert!(!wk.hits(30, 8, 0));
        assert!(!wk.hits(30, 8, 6));
    }

    #[test]
    fn f389_backoff_cap() {
        assert_eq!(retry_backoff_ms(3), 4000);
        assert_eq!(retry_backoff_ms(4), 8000);
        assert_eq!(retry_backoff_ms(9), 8000);
        assert!(retry_give_up(RETRY_MAX_ATTEMPTS));
    }

    #[test]
    fn f392_cond_deep_not_chain() {
        let tree = [
            Cond::Not(1),
            Cond::Not(2),
            Cond::Not(3),
            Cond::False,
        ];
        assert!(eval_cond(&tree, 0)); // Not(Not(Not(False)))
        assert!(!eval_cond(&tree, 1)); // Not(Not(False))
    }

    #[test]
    fn f400_domain_selfcheck_all_pass() {
        let set = run_m600auto_checks();
        assert!(set.len() >= 25, "got {}", set.len());
        assert!(set.all_passed());
    }
}
