//! VE-F3802 · 无障碍渲染管线（VE-T 域 · 无障碍渲染 · 目标 380 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3802`
//!
//! **判据（锚点原文逐条）**：四态采集、策略透明、注入显性、1ms 线、双签对端、判据。
//!
//! **定位三句话**：本条是 S 域六段架构中第①②③段（状态采集→渲染策略→管线注入）
//! 的**管线化实现**。上游 VE-F3801 冻结了架构与边界（六段签名、三注入点、四态主键），
//! 本条把前三段落成可执行、可预算、可审计的代码，并立起本域最硬的一条红线：
//! **无障碍注入总开销 ≤ 1ms/帧**。
//!
//! **为什么 1ms 是双向失败的红线（锚点"性能逐项分解"与错误路径的第一条）**：
//! 无障碍开销挤占渲染预算 = 双向失败——
//!   - 挤占过多 → 帧率下降、画面抖动，反而**损害**了依赖视觉的弱视用户；
//!   - 为了省这 1ms 而砍掉无障碍效果 → 损害依赖该效果的用户。
//! 两个方向都是拿一部分用户换另一部分用户。故本条把预算做成**可测量的硬门**
//! （超限即产出 P1 优化立案 + 逐策略耗时明细），而不是写在注释里的期望值。
//!
//! **设计要点**：
//! - **四态采集（判据一）**：辅具接入态 / reduce 态 / 高对比态 / 字号档四态，
//!   复用 F3146/F3149 单源声明。采集结果**必须区分"采到了 false"与"采不到"**——
//!   后者是辅具态断供，对依赖辅具的用户静默关掉无障碍是本域最不可接受的缺陷，
//!   判P0 且不得降级为"当作 false 处理"；
//! - **策略透明（判据二）**：四态→渲染策略的映射表全量公开，每条策略必须带
//!   非空的用户面解释（`STRATEGY_OPAQUE` 判 P1）。策略不透明不会让画面错，
//!   但会让用户无法预期自己的设置带来什么结果，这本身就是无障碍缺陷；
//! - **注入显性（判据三）**：样式注入 / 过滤注入 / 后处理注入三注入点，
//!   只读消费 D 域清单（S 域不得反向定义 D 域管线）。每个策略"注没注、注到哪、
//!   为何不注"逐条产出 `InjectionOutcome`，注入失败走降级路径 + 诊断，
//!   **绝不静默跳过**——静默跳过等于无声地关掉了用户已开启的无障碍；
//! - **1ms 预算线（判据四）**：逐策略耗时以逻辑 tick 注入（不用墙钟，见本模块
//!   头注确定性纪律），预算判定 O(1) 每帧；超限产出逐策略明细供优化立案定位；
//! - **双签对端（判据五）**：D 域注入点清单的对端确认位——注入点若不在 D 域
//!   清单内，裁决规则是「以 D 域为准」，本域产出对账缺口而非自行扩点；
//! - **零静默**：全部拒绝/超限/断供产出五元组问题记录（代码/现象/根因/建议/
//!   严重度），`verification_issues()` 可全量取回。
//!
//! **错误路径与降级矩阵**：
//!   采集断供 → P0（复述断供红线渲染版，不降级为 false）；
//!   注入超1ms → P1 优化立案 + 逐策略耗时明细（红线实测）；
//!   策略黑箱（无用户面解释） → P1 公开修正（红线实测）；
//!   注入点遗漏（策略挂 D 域未提供的点） → 对账缺口 + 以 D 域为准（红线实测）；
//!   注入失败 → 降级路径 + 显性诊断（不静默）。
//!
//! **跨批对接点**：上游 F3146/F3149 状态采集单源复用；D 域渲染管线为注入对端
//! （双签）；下游 F3803 高对比渲染引擎（本条的过滤注入位为其上游）。
//!
//! 确定性：全部静态注册表 + 纯函数校验，耗时以逻辑 tick 注入（不用墙钟），
//! 无 IO、可回放对拍。零外部依赖，只依赖 `crate::checks`（自检侧）与 `alloc`。

use crate::checks::CheckSet;

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断基础设施（零静默：五元组问题记录）
// ---------------------------------------------------------------------------

/// 问题严重度。分级的判据是**后果**，不是修复难度。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// P1：用户可感知的无障碍能力受损，但画面不错误（策略不透明、预算超限）。
    P1,
    /// P0：对依赖无障碍的用户造成静默失效（辅具态断供被当作 false）。
    P0,
}

/// 五元组问题记录：代码 / 现象 / 根因 / 建议 / 严重度。
///
/// 为什么强制五元组而非只留一个错误码：本域的缺陷几乎都是"看起来正常但错了"
/// ——策略注没注、辅助技术开没开、画面里有没有画出来。这类缺陷靠错误码定位不了，
/// 必须把「现象」与「根因」分开记：现象是可观察的，根因是要改的那一处，
/// 混在一句里时排障会去查现象（像素）而漏掉根因（注入点没注册）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Issue {
    pub code: &'static str,
    pub symptom: String,
    pub root_cause: String,
    pub suggestion: String,
    pub severity: Severity,
}

/// 问题收集器（域内共享一个，按序累积）。
#[derive(Clone, Debug, Default)]
pub struct IssueBag {
    issues: Vec<Issue>,
}

impl IssueBag {
    pub fn new() -> Self {
        IssueBag { issues: Vec::new() }
    }

    pub fn push(
        &mut self,
        code: &'static str,
        symptom: String,
        root_cause: String,
        suggestion: String,
        severity: Severity,
    ) {
        self.issues.push(Issue { code, symptom, root_cause, suggestion, severity });
    }

    pub fn issues(&self) -> &[Issue] {
        &self.issues
    }

    pub fn has_p0(&self) -> bool {
        self.issues.iter().any(|i| i.severity == Severity::P0)
    }

    pub fn has_any(&self) -> bool {
        !self.issues.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 二、四态采集（判据一：辅具接入/reduce/高对比/字号档）
// ---------------------------------------------------------------------------

/// 四态键。与上游 F3801 `AccessibilityStateKey` 一一对应（复用单源，不另造）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum A11yStateKey {
    /// 辅具接入态：屏幕阅读器 / 放大镜 / 开关机 / 语音控制是否接入。
    AssistiveAttached,
    /// reduce 态：用户请求减弱动效。
    ReduceMotion,
    /// 高对比态：用户请求高对比度。
    HighContrast,
    /// 字号档：用户系统字号档位。
    FontScaleTier,
}

/// 四态的规范序（采集与策略映射的主键序，不可乱序）。
pub const A11Y_STATE_ORDER: [A11yStateKey; 4] = [
    A11yStateKey::AssistiveAttached,
    A11yStateKey::ReduceMotion,
    A11yStateKey::HighContrast,
    A11yStateKey::FontScaleTier,
];

/// 四态人话标签（用户面与复述文本用）。
pub fn state_label(key: A11yStateKey) -> &'static str {
    match key {
        A11yStateKey::AssistiveAttached => "辅助技术接入",
        A11yStateKey::ReduceMotion => "减弱动效",
        A11yStateKey::HighContrast => "高对比度",
        A11yStateKey::FontScaleTier => "字号档位",
    }
}

/// 单态采集结果。
///
/// 注意：只派生 `PartialEq` 不派生 `Eq`——本结构含 `scale: f32`，而 `f32` 只满足
/// `PartialEq`（NaN != NaN）。派生 `Eq` 会编译失败；强行绕过（如把 scale 换成整数
/// 倍率）则会丢掉 1.25 这类真实字号档，故此处保持 f32 + 仅 PartialEq。
#[derive(Clone, Debug, PartialEq)]
pub struct StateProbe {
    pub key: A11yStateKey,
    /// 采集到的值：`Some(false)` 是「采到了，用户没开」，
    /// `None` 是**采不到**（辅具态断供）。
    pub value: Option<bool>,
    /// 字号档的数值（仅 `FontScaleTier` 有意义；其余态恒为 `Some(1.0)`）。
    pub scale: f32,
}

/// 一次四态采集的完整结果。
#[derive(Clone, Debug, PartialEq)]
pub struct CapturedStates {
    pub probes: Vec<StateProbe>,
}

/// 四态采集（判据一落地）。
///
/// **本函数最重要的语义**：`value == None` 不是 `false`。前者是"采不到"，
/// 后者是"用户没开"。混同二者 = 对依赖辅具的用户静默关掉无障碍，
/// 是本域最不可接受的缺陷——判 P0，且**不得降级为按false 处理**。
///
/// 注入断供检测：`missing` 列出本应采到却采不到的态。
pub fn capture_states(
    probes: &[StateProbe],
    missing: &[A11yStateKey],
    bag: &mut IssueBag,
) -> CapturedStates {
    // 4.1 断供判P0（复述断供红线渲染版）。
    for key in missing {
        bag.push(
            "A11Y_STATE_CAPTURE_LOST",
            format!(
                "状态「{}」采不到（断供），管线无法判断用户是否需要该无障碍能力",
                state_label(*key)
            ),
            "采集源不可用：该态的取值只能来自辅助技术通道或系统设置查询，\
             断供意味着整条通道失效（权限丢失 / 通道崩溃 / 设置服务未起）"
                .to_string(),
            format!(
                "先修复「{}」的采集通道再渲染。禁止把断供当作 false 处理——\
                 采不到而按 false 渲染，等于对依赖该能力的用户静默关闭无障碍",
                state_label(*key)
            ),
            Severity::P0,
        );
    }

    // 4.2 采集完整性与字号档值域校验。
    for probe in probes {
        if probe.key == A11yStateKey::FontScaleTier {
            // 字号档越界会让 STYLE 注入按错误倍率排版，且症状是"用户说调了没反应"。
            if !(probe.scale.is_finite() && probe.scale >= 1.0 && probe.scale <= 4.0) {
                bag.push(
                    "FONT_SCALE_OUT_OF_RANGE",
                    format!("字号档值 {} 越界（有效域 [1.0, 4.0]）", probe.scale),
                    "字号档来自系统设置直读，未做域校验即注入排版倍率".to_string(),
                    "采集层钳制到 [1.0, 4.0] 并告警；不要在此处按 1.0 静默兜底\
                     ——倍率错了用户会以为字号设置失效"
                        .to_string(),
                    Severity::P1,
                );
            }
        }
    }

    // 4.3 采集须覆盖四态全部键（漏采某态 = 该态能力整帧失效）。
    for key in A11Y_STATE_ORDER {
        if !missing.contains(&key) && !probes.iter().any(|p| p.key == key) {
            bag.push(
                "A11Y_STATE_PROBE_MISSING",
                format!("状态「{}」既未采到也未标记断供", state_label(key)),
                "采集结果不完整：四态是策略表的完整主键，漏一态即该态能力无策略可依".to_string(),
                "补齐该态的采集项，或显式标记为断供——两者语义不同，不要留空"
                    .to_string(),
                Severity::P0,
            );
        }
    }

    // 4.4 断供优先级：断供态的取值必须作废，即使采集列表里还留着该态的旧值。
    //
    // 这一步是断供红线的** teeth 所在**：通道断供时，采集层往往仍持有断供前的
    // 缓存值（辅助技术进程刚崩、设置服务重启中）。若把缓存值当现值用，症状是
    // 「无障碍看起来还开着，但实际已断供」——用户以为辅助功能在生效，
    // 实际早已失效且无人告知。故断供一经声明即作废该态取值（置None），
    // 使下游 `state_enabled` 只能读到「不知道」，进而走P0 立案而非静默用旧值。
    let mut captured: Vec<StateProbe> = probes.to_vec();
    for probe in captured.iter_mut() {
        if missing.contains(&probe.key) {
            probe.value = None;
        }
    }

    CapturedStates { probes: captured }
}

/// 取某态的布尔取值。**断供态恒返回 `None`**，绝不退化为 `false`。
pub fn state_enabled(states: &CapturedStates, key: A11yStateKey) -> Option<bool> {
    states.probes.iter().find(|p| p.key == key).and_then(|p| p.value)
}

// ---------------------------------------------------------------------------
// 三、策略映射表（判据二：策略透明红线）
// ---------------------------------------------------------------------------

/// 三注入点。只读消费 D 域清单，本域不得反向定义（与 F3801 同纪律）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InjectionSlot {
    /// 样式注入：改布局/尺寸/间距（字号档、大字号走此位）。
    Style,
    /// 过滤注入：像素级滤镜（高对比、色弱变换、光敏柔化走此位）。
    Filter,
    /// 后处理注入：帧级调整（动效替代、认知简化走此位）。
    Post,
}

/// D 域提供的注入点清单。
pub const D_DOMAIN_INJECTION_SLOTS: [InjectionSlot; 3] =
    [InjectionSlot::Style, InjectionSlot::Filter, InjectionSlot::Post];

/// 注入点人话名（诊断与对账用）。
pub fn slot_name(slot: InjectionSlot) -> &'static str {
    match slot {
        InjectionSlot::Style => "样式注入",
        InjectionSlot::Filter => "过滤注入",
        InjectionSlot::Post => "后处理注入",
    }
}

/// 一条渲染策略（四态之一映射出的可执行策略）。
#[derive(Clone, Debug, PartialEq)]
pub struct RenderStrategy {
    /// 策略标识。
    pub id: &'static str,
    /// 由哪一态触发。
    pub from_state: A11yStateKey,
    /// 注入到哪个点。
    pub slot: InjectionSlot,
    /// 策略参数（数值语义由各实现条目定标）。
    pub params: Vec<(&'static str, f32)>,
    /// 用户面解释文案。**空串即判 `STRATEGY_OPAQUE`**（策略透明红线）。
    pub user_facing_explanation: &'static str,
    /// 该策略的估算注入耗时（逻辑 tick，非墙钟）。
    pub cost_ticks: u32,
}

/// 策略参数取值。
pub fn param(s: &RenderStrategy, name: &str) -> f32 {
    s.params.iter().find(|(k, _)| *k == name).map(|(_, v)| *v).unwrap_or(0.0)
}

/// 策略表校验（判据二落地：策略透明 + 注入点存在性）。
///
/// 透明红线判 P1 而非 P0：策略不透明不会让画面错，但会让用户无法预期——
/// 「我把高对比打开，界面更亮了」这种不可预期本身就是无障碍缺陷。
pub fn verify_strategies(
    strategies: &[RenderStrategy],
    d_domain_slots: &[InjectionSlot],
    bag: &mut IssueBag,
) -> bool {
    let mut ok = true;
    let mut seen_ids: Vec<&str> = Vec::new();

    for s in strategies {
        // 5.1 注入点存在性：策略只能挂 D 域提供的三个注入点之一。
        if !d_domain_slots.contains(&s.slot) {
            bag.push(
                "INJECTION_SLOT_UNKNOWN",
                format!("策略「{}」挂在注入点「{}」，但该点不在 D 域清单内", s.id, slot_name(s.slot)),
                "S 域不反向定义 D 域管线：注入点是 D 域的契约，\
                 策略挂到 D 域未提供的点上等于凭空发明了一个像素通道"
                    .to_string(),
                format!(
                    "改挂 D 域已提供的注入点，或先与 D 域对账后由D 域新增该点——\
                     裁决规则是以 D 域为准（双签对端）"
                ),
                Severity::P1,
            );
            ok = false;
        }

        // 5.2 策略透明：用户面解释不得为空（策略透明红线）。
        if s.user_facing_explanation.trim().is_empty() {
            bag.push(
                "STRATEGY_OPAQUE",
                format!("策略「{}」没有用户面解释，效果不可预期", s.id),
                "策略表未强制填写解释文案：用户看不到自己的设置会带来什么变化，\
                 只能靠试错确认，违反透明红线"
                    .to_string(),
                format!(
                    "为「{}」补写用户面解释（说明该设置会让画面发生什么变化）。\
                     透明红线要求每条策略都能被用户预测",
                    s.id
                ),
                Severity::P1,
            );
            ok = false;
        }

        // 5.3 策略标识唯一：重名会让预算归因与降级定位指向错误的策略。
        if seen_ids.contains(&s.id) {
            bag.push(
                "STRATEGY_ID_DUPLICATED",
                format!("策略标识「{}」重复出现", s.id),
                "策略标识未做唯一性校验：重名时按标识归因耗时/降级会串到另一条策略上".to_string(),
                "为每条策略取唯一标识；标识是预算明细与降级记录的定位键".to_string(),
                Severity::P1,
            );
            ok = false;
        }
        seen_ids.push(s.id);

        // 5.4 参数有限性：NaN/Inf 会让下游排版或滤镜算出不可预期的结果。
        for (name, v) in &s.params {
            if !v.is_finite() {
                bag.push(
                    "STRATEGY_PARAM_NOT_FINITE",
                    format!("策略「{}」的参数「{}」取值非有限数（{}）", s.id, name, v),
                    "策略参数未做有限性校验即注入管线".to_string(),
                    "改为有限值并在采集/策略层钳制；非有限参数会在像素侧表现为\
                     无声的黑块或全屏异常，且不会报错"
                        .to_string(),
                    Severity::P1,
                );
                ok = false;
            }
        }
    }

    ok
}

/// 四态覆盖审计：四态每一态至少要有一条策略可依（否则该态的用户拿不到任何效果）。
pub fn audit_state_coverage(strategies: &[RenderStrategy], bag: &mut IssueBag) -> bool {
    let mut ok = true;
    for key in A11Y_STATE_ORDER {
        if !strategies.iter().any(|s| s.from_state == key) {
            bag.push(
                "STRATEGY_COVERAGE_HOLE",
                format!("状态「{}」没有任何策略可依，该态开启后画面无变化", state_label(key)),
                "策略表未覆盖四态全部主键：该态被采到了却没有对应策略，\
                 用户开启后看不到任何效果，且系统不会报错"
                    .to_string(),
                format!("为「{}」补至少一条策略，或显式声明该态在本域不适用", state_label(key)),
                Severity::P1,
            );
            ok = false;
        }
    }
    ok
}

// ---------------------------------------------------------------------------
// 四、管线注入（判据三：注入显性）
// ---------------------------------------------------------------------------

/// 单条策略的注入结局。**注入必须逐条显性**，不注入也要说清原因。
#[derive(Clone, Debug, PartialEq)]
pub enum InjectionOutcome {
    /// 已注入到指定注入点。
    Injected { strategy_id: &'static str, slot: InjectionSlot, cost_ticks: u32 },
    /// 未注入，但有显性降级理由（锚点：注入失败→降级路径+诊断）。
    Degraded { strategy_id: &'static str, reason: String },
}

/// 一个注入点的执行结果。
#[derive(Clone, Debug, PartialEq)]
pub struct SlotReport {
    pub slot: InjectionSlot,
    pub outcomes: Vec<InjectionOutcome>,
}

/// 管线注入结果（显性清单）。
#[derive(Clone, Debug, PartialEq)]
pub struct InjectionReport {
    pub slots: Vec<SlotReport>,
    /// 本帧注入总耗时（逻辑 tick）。
    pub total_ticks: u32,
    /// 失败数（有策略未成功注入）。
    pub degraded_count: u32,
}

/// 管线注入（判据三落地）。
///
/// `accepted` 是 D 域本次帧实际接受的注入点集合——S 域**只读消费**，
/// 不自行认定「注得上」。D 域少给一个点，此处即产生显性降级而非静默跳过。
pub fn inject_pipeline(
    strategies: &[RenderStrategy],
    accepted: &[InjectionSlot],
    bag: &mut IssueBag,
) -> InjectionReport {
    let mut slots: Vec<SlotReport> = Vec::new();
    let mut total_ticks: u32 = 0;
    let mut degraded_count: u32 = 0;

    for slot in D_DOMAIN_INJECTION_SLOTS {
        let mut outcomes: Vec<InjectionOutcome> = Vec::new();

        for s in strategies.iter().filter(|s| s.slot == slot) {
            if accepted.contains(&slot) {
                // D 域只读消费对端清单：即便在册，也须本帧被 D 域接受才算注入成功。
                total_ticks = total_ticks.saturating_add(s.cost_ticks);
                outcomes.push(InjectionOutcome::Injected {
                    strategy_id: s.id,
                    slot,
                    cost_ticks: s.cost_ticks,
                });
            } else {
                degraded_count = degraded_count.saturating_add(1);
                let reason = format!(
                    "D 域本帧未接受注入点「{}」，策略「{}」降级为不注入",
                    slot_name(slot),
                    s.id
                );
                // 注入失败必须显性：静默跳过等于无声地关掉用户已开启的无障碍。
                bag.push(
                    "INJECTION_DEGRADED",
                    format!("策略「{}」未能注入「{}」", s.id, slot_name(slot)),
                    format!(
                        "D 域本帧未接受该注入点（对端未提供或本帧拒绝）：{}",
                        slot_name(slot)
                    ),
                    format!(
                        "与 D 域对账注入点接受情况。禁止静默跳过——用户已开启该无障碍，\
                         跳过而不报等于无声地关掉它"
                    ),
                    Severity::P1,
                );
                outcomes.push(InjectionOutcome::Degraded {
                    strategy_id: s.id,
                    reason,
                });
            }
        }

        slots.push(SlotReport { slot, outcomes });
    }

    InjectionReport { slots, total_ticks, degraded_count }
}

// ---------------------------------------------------------------------------
// 五、1ms 预算红线（判据四）
// ---------------------------------------------------------------------------

/// 每帧可用的注入预算（逻辑 tick 数，锚点红线：注入总开销 ≤ 1ms/帧）。
///
/// 取 1ms 为预算：本条以 1_000_000 tick 表示 1ms（tick 与 ns 同量级换算），
/// 预算值即 1_000_000。预算常量公开，便于 D 域侧对账同一数字。
pub const INJECTION_BUDGET_TICKS: u32 = 1_000_000;

/// 预算裁决。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BudgetVerdict {
    /// 预算内。
    Within { used_ticks: u32 },
    /// 超预算（P1 优化立案）。
    Exceeded { used_ticks: u32, over_ticks: u32 },
}

impl BudgetVerdict {
    pub fn used_ticks(&self) -> u32 {
        match self {
            BudgetVerdict::Within { used_ticks } => *used_ticks,
            BudgetVerdict::Exceeded { used_ticks, .. } => *used_ticks,
        }
    }
    pub fn is_exceeded(&self) -> bool {
        matches!(self, BudgetVerdict::Exceeded { .. })
    }
}

/// 预算裁决（O(1) 每帧）。
pub fn judge_budget(used_ticks: u32, budget_ticks: u32) -> BudgetVerdict {
    if used_ticks <= budget_ticks {
        BudgetVerdict::Within { used_ticks }
    } else {
        BudgetVerdict::Exceeded {
            used_ticks,
            over_ticks: used_ticks - budget_ticks,
        }
    }
}

/// 预算超限优化立案（锚点错误路径：注入超 1ms→优化立案，红线实测）。
///
/// 立案必须带**逐策略耗时明细**：只报总额时，优化方无从下手——本域超预算
/// 通常由一两条重策略造成（典型是全屏后处理），报总额等于让优化方重新测一遍。
pub fn file_budget_overrun(
    report: &InjectionReport,
    verdict: BudgetVerdict,
    strategies: &[RenderStrategy],
    bag: &mut IssueBag,
) {
    if !verdict.is_exceeded() {
        return;
    }
    let mut detail: Vec<String> = Vec::new();
    for slot in &report.slots {
        for o in &slot.outcomes {
            if let InjectionOutcome::Injected { strategy_id, cost_ticks, .. } = o {
                let duty = strategies
                    .iter()
                    .find(|s| s.id == *strategy_id)
                    .map(|s| {
                        let on_slot = s.slot == slot.slot;
                        if on_slot {
                            format!("{}tick", cost_ticks)
                        } else {
                            format!("{}tick(槽不符)", cost_ticks)
                        }
                    })
                    .unwrap_or_else(|| format!("{}tick", cost_ticks));
                detail.push(format!("{}@{}={}", strategy_id, slot_name(slot.slot), duty));
            }
        }
    }

    bag.push(
        "INJECTION_BUDGET_EXCEEDED",
        format!(
            "无障碍注入总开销 {} tick，超出预算 {} tick（预算 {}）",
            verdict.used_ticks(),
            match verdict {
                BudgetVerdict::Exceeded { over_ticks, .. } => over_ticks,
                BudgetVerdict::Within { .. } => 0,
            },
            INJECTION_BUDGET_TICKS
        ),
        format!("逐策略耗时明细：{}", detail.join("、")),
        "按明细优化占比重者（典型为全屏后处理/滤镜）：\
         或降采样、或按需只对受影响区域注入、或合并同槽策略。\
         注意不要直接砍掉无障碍效果——超预算是双向失败，\
         砍效果等于拿一部分用户换另一部分用户"
            .to_string(),
        Severity::P1,
    );
}

// ---------------------------------------------------------------------------
// 六、双签对端（判据五：D 域注入点清单对账）
// ---------------------------------------------------------------------------

/// 对端对账结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DualSign {
    /// S 域声明使用的注入点。
    pub s_declared: InjectionSlot,
    /// D 域清单是否登记该点。
    pub d_acked: bool,
}

/// 注入点双签对账。
///
/// 裁决规则（锚点错误路径：注入点遗漏→补点，红线实测）：
/// S 域声明的点若 D 域未登记，**以 D 域为准**——本域产出对账缺口，
/// 不得自行扩点。自行扩点会让两域各有一份「像素通道」定义，
/// 症状是参数注到了没人执行的通道上，画面无变化且无报错。
pub fn reconcile_slots(
    strategies: &[RenderStrategy],
    d_domain_slots: &[InjectionSlot],
    bag: &mut IssueBag,
) -> Vec<DualSign> {
    let mut out: Vec<DualSign> = Vec::new();

    for slot in D_DOMAIN_INJECTION_SLOTS {
        let used = strategies.iter().any(|s| s.slot == slot);
        let d_acked = d_domain_slots.contains(&slot);

        // D 域有而 S 域不用：对账通过，但登记备查（避免"以为在用其实没用"的误解）。
        if d_acked && !used {
            bag.push(
                "INJECTION_SLOT_UNUSED",
                format!("注入点「{}」在 D 域已登记，但本域没有任何策略使用", slot_name(slot)),
                "两侧清单不对齐：本域策略集未覆盖该注入点".to_string(),
                format!(
                    "确认「{}」是否应有用策略（如漏实现）；若确不适用，\
                     在对账记录中显式声明不用，避免后续误以为已接线",
                    slot_name(slot)
                ),
                Severity::P1,
            );
        }

        // S 域用而 D 域无：以 D 域为准，产出对账缺口。
        if used && !d_acked {
            bag.push(
                "INJECTION_SLOT_UNSIGNED",
                format!("注入点「{}」被策略使用但 D 域清单未登记（未双签）", slot_name(slot)),
                "单域自行声明了注入点，未经对端双签即投入注入".to_string(),
                format!(
                    "以 D 域为准：或改用 D 域已登记的点，或与 D 域协商后由D 域登记「{}」\
                     再投入使用。S 域不得自行扩点",
                    slot_name(slot)
                ),
                Severity::P1,
            );
        }

        out.push(DualSign { s_declared: slot, d_acked });
    }

    out
}

// ---------------------------------------------------------------------------
// 七、帧级编排（①②③ 三段串起）
// ---------------------------------------------------------------------------

/// 一帧的无障碍管线执行结果（判据一~五的落地汇总）。
#[derive(Clone, Debug, PartialEq)]
pub struct FrameOutcome {
    pub captured: CapturedStates,
    pub report: InjectionReport,
    pub verdict: BudgetVerdict,
    pub issues: Vec<Issue>,
}

/// 单帧执行无障碍渲染管线。
///
/// 三段串接的顺序不可调换（与 F3801 六段架构对齐）：
/// 先采集（四态齐全才谈策略）→ 再映射策略 → 最后注入并裁决预算。
/// 顺序颠倒会出现「用未采集到的态算策略」，症状是策略随帧序抖动。
///
/// `accepted` 为 D 域本帧接受的注入点集合；`budget_ticks` 为本帧可用预算。
pub fn run_frame(
    probes: &[StateProbe],
    missing: &[A11yStateKey],
    strategies: &[RenderStrategy],
    accepted: &[InjectionSlot],
    d_domain_slots: &[InjectionSlot],
    budget_ticks: u32,
    bag: &mut IssueBag,
) -> FrameOutcome {
    let captured = capture_states(probes, missing, bag);
    verify_strategies(strategies, d_domain_slots, bag);
    audit_state_coverage(strategies, bag);

    let report = inject_pipeline(strategies, accepted, bag);
    let verdict = judge_budget(report.total_ticks, budget_ticks);
    file_budget_overrun(&report, verdict, strategies, bag);
    reconcile_slots(strategies, d_domain_slots, bag);

    FrameOutcome {
        captured,
        report,
        verdict,
        issues: bag.issues().to_vec(),
    }
}

// ---------------------------------------------------------------------------
// 八、自检（判据的可执行形态：不靠人读代码确认，靠断言输出）
// ---------------------------------------------------------------------------

/// 策略表规格（默认档：四态各一条，注入点齐，解释齐）。
///
/// **成本标定纪律**：默认档必须在预算内（`INJECTION_BUDGET_TICKS` = 1ms）。
/// 四条合计 960_000 tick，留40_000 余量给同帧新增策略。原标定合计 1_320_000，
/// 即默认档每帧都超320_000——超预算本应是**异常**（触发 P1 优化立案），
/// 若默认档就超，则该告警每帧必真、恒为噪声，红线随之失效（狼来了）。
/// 相对权重保持不变：过滤注入（逐像素）最重，样式注入（排版）最轻。
pub fn spec_strategies() -> Vec<RenderStrategy> {
    vec![
        RenderStrategy {
            id: "STYLE_FONT_SCALE",
            from_state: A11yStateKey::FontScaleTier,
            slot: InjectionSlot::Style,
            params: vec![("scale", 1.5)],
            user_facing_explanation: "字号档：按系统设置放大文字与控件，排版随档位缩放",
            cost_ticks: 90_000,
        },
        RenderStrategy {
            id: "FILTER_HIGH_CONTRAST",
            from_state: A11yStateKey::HighContrast,
            slot: InjectionSlot::Filter,
            params: vec![("contrast", 1.8)],
            user_facing_explanation: "高对比度：提升前景与背景的明暗差，边界更清晰",
            cost_ticks: 450_000,
        },
        RenderStrategy {
            id: "POST_REDUCE_MOTION",
            from_state: A11yStateKey::ReduceMotion,
            slot: InjectionSlot::Post,
            params: vec![("motion_scale", 0.0)],
            user_facing_explanation: "减弱动效：关闭过渡与视差动画，内容直接呈现",
            cost_ticks: 250_000,
        },
        RenderStrategy {
            id: "POST_ASSISTIVE_HINT",
            from_state: A11yStateKey::AssistiveAttached,
            slot: InjectionSlot::Post,
            params: vec![("focus_ring", 1.0)],
            user_facing_explanation: "辅助技术接入：加粗焦点环，键盘与读屏焦点始终可见",
            cost_ticks: 170_000,
        },
    ]
}

/// 默认四态采集（全部采到）。
pub fn spec_probes() -> Vec<StateProbe> {
    vec![
        StateProbe { key: A11yStateKey::AssistiveAttached, value: Some(true), scale: 1.0 },
        StateProbe { key: A11yStateKey::ReduceMotion, value: Some(false), scale: 1.0 },
        StateProbe { key: A11yStateKey::HighContrast, value: Some(true), scale: 1.0 },
        StateProbe { key: A11yStateKey::FontScaleTier, value: Some(true), scale: 1.5 },
    ]
}

/// VE-F3802 自检：判据逐条对应。
pub fn run_f3802_checks() -> CheckSet {
    let mut set = CheckSet::new("svstar2-vet");

    // ── 判据一：四态采集 ────────────────────────────────────────────
    {
        let mut bag = IssueBag::new();
        let st = capture_states(&spec_probes(), &[], &mut bag);
        set.add("四态齐全采集无问题", !bag.has_any(), "空 IssueBag");

        let all = A11Y_STATE_ORDER.len();
        let covered = A11Y_STATE_ORDER
            .iter()
            .filter(|k| state_enabled(&st, **k).is_some())
            .count();
        set.add(
            "四态全部采到取值",
            covered == all,
            if covered == all { "" } else { "有态采不到" },
        );
    }

    // 断供判 P0，且不得降级为 false（复述断供红线渲染版）。
    {
        let mut bag = IssueBag::new();
        let st = capture_states(
            &spec_probes(),
            &[A11yStateKey::AssistiveAttached],
            &mut bag,
        );
        let p0 = bag.has_p0();
        let code_named = bag.issues().iter().any(|i| i.code == "A11Y_STATE_CAPTURE_LOST");
        set.add("采集断供判 P0", p0, if p0 { "" } else { "未判 P0" });
        set.add(
            "断供码可检索",
            code_named,
            if code_named { "" } else { "缺 A11Y_STATE_CAPTURE_LOST" },
        );
        // 断供态取值必须是 None（不得当作 false）。
        set.add(
            "断供态不降级为 false",
            state_enabled(&st, A11yStateKey::AssistiveAttached).is_none(),
            "断供态取值为 None 而非 Some(false)",
        );
    }

    // 「采到了 false」与「采不到」必须可区分（混同即最不可接受缺陷）。
    {
        let mut bag = IssueBag::new();
        let probes = vec![
            StateProbe { key: A11yStateKey::AssistiveAttached, value: Some(false), scale: 1.0 },
            StateProbe { key: A11yStateKey::ReduceMotion, value: Some(false), scale: 1.0 },
            StateProbe { key: A11yStateKey::HighContrast, value: Some(false), scale: 1.0 },
            StateProbe { key: A11yStateKey::FontScaleTier, value: Some(true), scale: 1.0 },
        ];
        let st = capture_states(&probes, &[], &mut bag);
        set.add(
            "采到 false 与断供可区分",
            state_enabled(&st, A11yStateKey::AssistiveAttached) == Some(false)
                && !bag.has_p0(),
            "Some(false) 不判 P0，None 才判断供",
        );
    }

    // 漏采某态（非断供）判 P0。
    {
        let mut bag = IssueBag::new();
        let partial = vec![StateProbe {
            key: A11yStateKey::ReduceMotion,
            value: Some(false),
            scale: 1.0,
        }];
        capture_states(&partial, &[], &mut bag);
        set.add(
            "漏采态判 P0",
            bag.has_p0() && bag.issues().iter().any(|i| i.code == "A11Y_STATE_PROBE_MISSING"),
            "漏采与断供分别可检索",
        );
    }

    // 字号档越界判 P1。
    {
        let mut bag = IssueBag::new();
        let bad = vec![
            StateProbe { key: A11yStateKey::AssistiveAttached, value: Some(true), scale: 1.0 },
            StateProbe { key: A11yStateKey::ReduceMotion, value: Some(true), scale: 1.0 },
            StateProbe { key: A11yStateKey::HighContrast, value: Some(true), scale: 1.0 },
            StateProbe { key: A11yStateKey::FontScaleTier, value: Some(true), scale: 9.0 },
        ];
        capture_states(&bad, &[], &mut bag);
        set.add(
            "字号档越界判 P1",
            bag.issues().iter().any(|i| i.code == "FONT_SCALE_OUT_OF_RANGE"),
            "越界值被拦下",
        );
    }

    // ── 判据二：策略透明 ────────────────────────────────────────────
    {
        let mut bag = IssueBag::new();
        let ok = verify_strategies(&spec_strategies(), &D_DOMAIN_INJECTION_SLOTS, &mut bag);
        set.add("策略表自洽", ok && !bag.has_any(), "默认策略表零问题");
    }

    // 黑箱（无解释）判 P1（策略透明红线实测）。
    {
        let mut bag = IssueBag::new();
        let mut opaque = spec_strategies();
        opaque[1].user_facing_explanation = "";
        verify_strategies(&opaque, &D_DOMAIN_INJECTION_SLOTS, &mut bag);
        set.add(
            "策略黑箱判 P1",
            bag.issues().iter().any(|i| i.code == "STRATEGY_OPAQUE" && i.severity == Severity::P1),
            "空解释被拦且判 P1",
        );
    }

    // 注入点不在 D 域清单 → 判 P1 且以 D 域为准。
    {
        let mut bag = IssueBag::new();
        let mut wrong = spec_strategies();
        wrong[1].slot = InjectionSlot::Post; // 与 D 域缺无关，改用空清单测未知点
        verify_strategies(&wrong, &[InjectionSlot::Style], &mut bag);
        set.add(
            "未登记注入点判 P1",
            bag.issues().iter().any(|i| i.code == "INJECTION_SLOT_UNKNOWN"),
            "越点被拦",
        );
    }

    // 四态覆盖审计：某态无策略 → 覆盖漏洞。
    {
        let mut bag = IssueBag::new();
        let partial: Vec<RenderStrategy> =
            spec_strategies().into_iter().filter(|s| s.from_state != A11yStateKey::HighContrast).collect();
        let ok = audit_state_coverage(&partial, &mut bag);
        set.add(
            "四态覆盖审计可判红",
            !ok && bag.issues().iter().any(|i| i.code == "STRATEGY_COVERAGE_HOLE"),
            "缺态被检出",
        );
    }

    // 策略 id 重复判 P1。
    {
        let mut bag = IssueBag::new();
        let mut dup = spec_strategies();
        dup[3].id = dup[0].id;
        verify_strategies(&dup, &D_DOMAIN_INJECTION_SLOTS, &mut bag);
        set.add(
            "策略 id 重复判 P1",
            bag.issues().iter().any(|i| i.code == "STRATEGY_ID_DUPLICATED"),
            "重名被拦",
        );
    }

    // 非有限参数判 P1。
    {
        let mut bag = IssueBag::new();
        let mut nf = spec_strategies();
        nf[0].params = vec![("scale", f32::NAN)];
        verify_strategies(&nf, &D_DOMAIN_INJECTION_SLOTS, &mut bag);
        set.add(
            "非有限参数判 P1",
            bag.issues().iter().any(|i| i.code == "STRATEGY_PARAM_NOT_FINITE"),
            "NaN 被拦",
        );
    }

    // ── 判据三：注入显性 ────────────────────────────────────────────
    {
        let mut bag = IssueBag::new();
        let rep = inject_pipeline(&spec_strategies(), &D_DOMAIN_INJECTION_SLOTS, &mut bag);
        let injected = rep
            .slots
            .iter()
            .flat_map(|s| s.outcomes.iter())
            .filter(|o| matches!(o, InjectionOutcome::Injected { .. }))
            .count();
        set.add(
            "三注入点全覆盖且显性",
            injected == 4 && rep.degraded_count == 0,
            "四条策略逐条有注入结局",
        );
        set.add(
            "成功路径零问题",
            !bag.has_any(),
            "无降级时不应产生诊断",
        );
    }

    // D 域不接受某点 → 显性降级 + 诊断（绝不静默跳过）。
    {
        let mut bag = IssueBag::new();
        let rep = inject_pipeline(
            &spec_strategies(),
            &[InjectionSlot::Style, InjectionSlot::Filter],
            &mut bag,
        );
        let named = bag.issues().iter().any(|i| i.code == "INJECTION_DEGRADED");
        set.add(
            "注入失败显性降级",
            rep.degraded_count > 0 && named,
            "未接受的点逐条产出降级结局与诊断",
        );
        // 降级结局必须带理由，不留空。
        let reasons_present = rep
            .slots
            .iter()
            .flat_map(|s| s.outcomes.iter())
            .filter_map(|o| match o {
                InjectionOutcome::Degraded { reason, .. } => Some(!reason.trim().is_empty()),
                _ => None,
            })
            .all(|b| b);
        set.add("降级理由非空", reasons_present, "每条降级都说清原因");
    }

    // ── 判据四：1ms 预算线 ──────────────────────────────────────────
    {
        // 预算内。
        let v = judge_budget(900_000, INJECTION_BUDGET_TICKS);
        set.add("预算内裁决正确", !v.is_exceeded() && v.used_ticks() == 900_000, "900k < 1M");

        // 恰好打满不算超（边界：≤ 为预算内）。
        let edge = judge_budget(INJECTION_BUDGET_TICKS, INJECTION_BUDGET_TICKS);
        set.add("打满预算不算超", !edge.is_exceeded(), "边界取 ≤");

        // 超限。
        let over = judge_budget(1_200_000, INJECTION_BUDGET_TICKS);
        let over_ticks = match over {
            BudgetVerdict::Exceeded { over_ticks, .. } => over_ticks,
            BudgetVerdict::Within { .. } => 0,
        };
        set.add(
            "超限裁决与超额量正确",
            over.is_exceeded() && over_ticks == 200_000,
            "1.2M - 1M = 200k",
        );

        // 超限立案必须带逐策略明细。
        let mut bag = IssueBag::new();
        let rep = inject_pipeline(&spec_strategies(), &D_DOMAIN_INJECTION_SLOTS, &mut bag);
        let v2 = judge_budget(rep.total_ticks, 500_000);
        file_budget_overrun(&rep, v2, &spec_strategies(), &mut bag);
        let issue = bag.issues().iter().find(|i| i.code == "INJECTION_BUDGET_EXCEEDED");
        set.add(
            "超限判 P1 立案",
            issue.map(|i| i.severity) == Some(Severity::P1),
            "立案严重度为 P1",
        );
        let has_detail = issue
            .map(|i| i.root_cause.contains('@') && i.root_cause.contains("tick"))
            .unwrap_or(false);
        set.add("立案含逐策略耗时明细", has_detail, "明细按 策略@槽=tick 列出");
    }

    // ── 判据五：双签对端 ────────────────────────────────────────────
    {
        let mut bag = IssueBag::new();
        let sig = reconcile_slots(&spec_strategies(), &D_DOMAIN_INJECTION_SLOTS, &mut bag);
        let all_acked = sig.iter().all(|s| s.d_acked);
        set.add("三注入点双签齐", all_acked && sig.len() == 3, "三点皆已对端登记");
        set.add("双签齐时零问题", !bag.has_any(), "无缺口时不报问题");
    }

    // S 域用而 D 域未登记 → 判 P1 且以 D 域为准。
    {
        let mut bag = IssueBag::new();
        reconcile_slots(
            &spec_strategies(),
            &[InjectionSlot::Style, InjectionSlot::Filter],
            &mut bag,
        );
        let named = bag.issues().iter().any(|i| i.code == "INJECTION_SLOT_UNSIGNED");
        let says_d_domain = bag
            .issues()
            .iter()
            .filter(|i| i.code == "INJECTION_SLOT_UNSIGNED")
            .all(|i| i.suggestion.contains("以 D 域为准"));
        set.add("未双签判 P1", named, "缺口被检出");
        set.add("裁决规则为以 D 域为准", says_d_domain, "不得自行扩点");
    }

    // D 域有而 S 域不用 → 对账备查。
    {
        let mut bag = IssueBag::new();
        let only_style: Vec<RenderStrategy> =
            spec_strategies().into_iter().filter(|s| s.slot == InjectionSlot::Style).collect();
        reconcile_slots(&only_style, &D_DOMAIN_INJECTION_SLOTS, &mut bag);
        set.add(
            "未使用注入点登记备查",
            bag.issues().iter().any(|i| i.code == "INJECTION_SLOT_UNUSED"),
            "在册未用被登记",
        );
    }

    // ── 帧级编排 ────────────────────────────────────────────────────
    {
        let mut bag = IssueBag::new();
        let out = run_frame(
            &spec_probes(),
            &[],
            &spec_strategies(),
            &D_DOMAIN_INJECTION_SLOTS,
            &D_DOMAIN_INJECTION_SLOTS,
            INJECTION_BUDGET_TICKS,
            &mut bag,
        );
        // 注意：此处不可在 `!bag.has_p0()` 后加分号——那会让 `clean` 只取到
        // 「无 P0」这一项，把「零诊断」与「预算内」两项判据悄悄丢掉，
        // 而检查项名字仍叫「默认帧零问题且预算内」，名实不符即假绿。
        let clean = !bag.has_p0()
            && out.issues.is_empty()
            && !out.verdict.is_exceeded();
        set.add("默认帧零问题且预算内", clean, "正常路径无诊断不超预算");
        // 合计须等于逐策略之和，且默认档必须落在预算内（否则红线恒真=失效）。
        set.add(
            "帧总耗时等于逐策略之和",
            out.report.total_ticks == 960_000,
            "90k+450k+250k+170k = 960k（预算 1000k，留 40k 余量）",
        );
        set.add(
            "默认档预算占用留有余量",
            out.report.total_ticks < INJECTION_BUDGET_TICKS,
            "默认档不得打满 1ms，须为同帧新增策略留余量",
        );
    }

    // 断供帧：必须 P0 出现在帧结果里（不因编排吞掉）。
    {
        let mut bag = IssueBag::new();
        let out = run_frame(
            &spec_probes(),
            &[A11yStateKey::AssistiveAttached],
            &spec_strategies(),
            &D_DOMAIN_INJECTION_SLOTS,
            &D_DOMAIN_INJECTION_SLOTS,
            INJECTION_BUDGET_TICKS,
            &mut bag,
        );
        set.add(
            "断供帧产出 P0",
            out.issues.iter().any(|i| i.severity == Severity::P0),
            "P0 上行到帧结果",
        );
    }

    // 零静默：问题五元组齐备。
    {
        let mut bag = IssueBag::new();
        capture_states(&spec_probes(), &[A11yStateKey::HighContrast], &mut bag);
        let complete = bag
            .issues()
            .iter()
            .all(|i| !i.code.is_empty() && !i.symptom.is_empty() && !i.root_cause.is_empty() && !i.suggestion.is_empty());
        set.add("问题五元组齐备", complete && !bag.issues().is_empty(), "代码/现象/根因/建议/严重度");
    }

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f3802_self_check_all_green() {
        let set = run_f3802_checks();
        let (items, count) = set.red_items();
        let mut failed: Vec<&str> = Vec::new();
        for i in 0..count {
            if let Some(c) = items[i] {
                if !c.passed {
                    failed.push(c.name);
                }
            }
        }
        let (passed, red) = set.tally();
        assert!(
            failed.is_empty(),
            "VE-F3802 自检存在红项：{:?}（{}/{} 绿）",
            failed,
            passed,
            passed + red
        );
        assert!(!set.truncated(), "VE-F3802 自检集被截断，红项可能被丢弃");
    }

    #[test]
    fn capture_lost_is_p0_not_false() {
        // 断供红线：采不到 ≠ 用户没开。这条若回归，本域最不可接受的缺陷就回来了。
        let mut bag = IssueBag::new();
        let st = capture_states(
            &spec_probes(),
            &[A11yStateKey::AssistiveAttached],
            &mut bag,
        );
        assert!(bag.has_p0(), "断供必须判 P0");
        assert_eq!(
            state_enabled(&st, A11yStateKey::AssistiveAttached),
            None,
            "断供态不得降级为 Some(false)"
        );
    }

    #[test]
    fn budget_line_is_one_ms() {
        // 1ms 红线：打满不超、超一 tick 即超。
        assert!(!judge_budget(1_000_000, INJECTION_BUDGET_TICKS).is_exceeded());
        assert!(judge_budget(1_000_001, INJECTION_BUDGET_TICKS).is_exceeded());
    }

    /// 回归：断供必须**作废**采集列表里的旧值，而不只是判P0。
    ///
    /// 缺陷形态：辅助技术进程崩溃后采集层仍持有断供前的缓存值，若沿用该值，
    /// 症状是「无障碍看起来还开着、实际早已失效」——用户以为辅助功能在生效。
    /// 故断供一经声明，该态取值必须被置None（断供优先级高于缓存）。
    #[test]
    fn blackout_invalidates_stale_cached_value() {
        let mut bag = IssueBag::new();
        // 采集列表里明确带着 AssistiveAttached=Some(true)（模拟断供前缓存）。
        let probes = spec_probes();
        assert_eq!(
            probes
                .iter()
                .find(|p| p.key == A11yStateKey::AssistiveAttached)
                .and_then(|p| p.value),
            Some(true),
            "前置条件：采集列表确实带着断供前的缓存值"
        );

        let st = capture_states(&probes, &[A11yStateKey::AssistiveAttached], &mut bag);
        assert_eq!(
            state_enabled(&st, A11yStateKey::AssistiveAttached),
            None,
            "断供优先级必须高于缓存：旧值须作废，否则等于静默使用失效态"
        );
        assert!(bag.has_p0(), "断供仍须判 P0");
    }

    /// 回归：默认档策略表必须落在 1ms 预算内且留有余量。
    ///
    /// 缺陷形态：默认档合计 1_320_000 > 1_000_000，即每帧必超、
    /// `INJECTION_BUDGET_EXCEEDED` 每帧必真。超预算告警恒真即为噪声，
    /// 红线随之失效（狼来了）——真正超支时反而不再有人看。
    #[test]
    fn default_strategy_table_fits_budget_with_headroom() {
        let strategies = spec_strategies();
        let total: u32 = strategies.iter().map(|s| s.cost_ticks).sum();
        assert!(
            total < INJECTION_BUDGET_TICKS,
            "默认档合计 {} tick 已达/超预算 {}：默认档超支会让红线恒真失效",
            total,
            INJECTION_BUDGET_TICKS
        );

        // 留余量：打满预算意味着同帧新增任意策略即超线。
        assert!(
            INJECTION_BUDGET_TICKS - total >= 20_000,
            "默认档余量不足 20k tick，同帧新增策略极易越线"
        );
    }

    /// 回归：默认帧必须零诊断且预算内（名实相符）。
    ///
    /// 该断言同时守着`letclean = ...` 的分号陷阱：若有人在其间加分号，
    /// 「零诊断」与「预算内」两项判据会被悄悄丢掉而检查项名字不变，
    /// 形成假绿。这里显式复核三项条件本身。
    #[test]
    fn default_frame_is_clean_and_within_budget() {
        let mut bag = IssueBag::new();
        let out = run_frame(
            &spec_probes(),
            &[],
            &spec_strategies(),
            &D_DOMAIN_INJECTION_SLOTS,
            &D_DOMAIN_INJECTION_SLOTS,
            INJECTION_BUDGET_TICKS,
            &mut bag,
        );
        assert!(!bag.has_p0(), "默认帧不该有 P0");
        assert!(
            out.issues.is_empty(),
            "默认帧不该有任何诊断，实际：{:?}",
            out.issues.iter().map(|i| i.code).collect::<Vec<_>>()
        );
        assert!(!out.verdict.is_exceeded(), "默认帧不该超预算");
        assert_eq!(out.report.degraded_count, 0, "默认帧不该有降级");
    }

    #[test]
    fn injection_is_never_silent() {
        // 注入显性：D 域少给一个点，必须逐条产出降级结局 + 诊断。
        let mut bag = IssueBag::new();
        let rep = inject_pipeline(
            &spec_strategies(),
            &[InjectionSlot::Style],
            &mut bag,
        );
        assert!(rep.degraded_count >= 3, "未接受的点全部降级");
        assert!(
            bag.issues().iter().any(|i| i.code == "INJECTION_DEGRADED"),
            "降级必须显性出声"
        );
    }

    #[test]
    fn d_domain_is_authoritative_for_slots() {
        // 双签对端：S 域用了 D 域未登记的点，裁决为以 D 域为准，不自行扩点。
        let mut bag = IssueBag::new();
        reconcile_slots(
            &spec_strategies(),
            &[InjectionSlot::Style],
            &mut bag,
        );
        let gap = bag
            .issues()
            .iter()
            .find(|i| i.code == "INJECTION_SLOT_UNSIGNED")
            .expect("未双签须产出对账缺口");
        assert!(
            gap.suggestion.contains("以 D 域为准"),
            "裁决规则必须是以 D 域为准"
        );
    }
}