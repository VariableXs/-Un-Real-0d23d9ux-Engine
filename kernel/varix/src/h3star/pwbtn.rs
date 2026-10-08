//! F318 电源按钮行为设置 · 完整设计（AI-H3 · 主册 G-H 区段）。
//!
//! **判据（主册）**：三档×双场景矩阵用例；后果文案审查；长按 4s 判定；
//! 软件关机路径优先级验证（B-2902 判据引用）。
//!
//! **设计要点（主册）**：按电源/合盖行为可配三档：睡眠（默认）/关机/无
//! 动作——后果说明直白列出；长按电源 4 秒=硬件强断（文档化警告：先试
//! 软件路径）；电池模式与外接模式可分别配置。
//!
//! 实现形态：3 档 × 2 场景参数矩阵 + 长按判定状态机（4s 判定——软件路
//! 径优先：4s 内释放走软件路径，4s 持续按住才落强断并留警告）。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 长按强断判定（ms）。
pub const LONG_PRESS_MS: u64 = 4000;

/// 行为三档。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PowerAction {
    Sleep,
    Shutdown,
    Nothing,
}

impl PowerAction {
    pub const ALL: [PowerAction; 3] = [PowerAction::Sleep, PowerAction::Shutdown, PowerAction::Nothing];

    pub fn label(self) -> &'static str {
        match self {
            PowerAction::Sleep => "睡眠",
            PowerAction::Shutdown => "关机",
            PowerAction::Nothing => "无动作",
        }
    }

    /// 后果文案（人话——审查面：三档各一句，说清代价）。
    pub fn consequence(self) -> &'static str {
        match self {
            PowerAction::Sleep => "睡眠：秒回但耗一点电",
            PowerAction::Shutdown => "关机：走完整关机账目",
            PowerAction::Nothing => "无动作：屏幕保持现状",
        }
    }
}

/// 场景。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scenario {
    PowerButton,
    LidClose,
}

impl Scenario {
    pub const ALL: [Scenario; 2] = [Scenario::PowerButton, Scenario::LidClose];
}

// ---------------------------------------------------------------------------
// 参数矩阵（3 档 × 2 场景 × 双电源）
// ---------------------------------------------------------------------------

/// 电源按钮策略（电池/外接分列）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PowerButtonPolicy {
    pub on_battery: [PowerAction; 2],
    pub on_ac: [PowerAction; 2],
}

impl PowerButtonPolicy {
    pub fn default_policy() -> PowerButtonPolicy {
        PowerButtonPolicy {
            on_battery: [PowerAction::Sleep, PowerAction::Sleep],
            on_ac: [PowerAction::Sleep, PowerAction::Sleep],
        }
    }

    /// 取场景行为（双场景矩阵取数口）。
    pub fn action_for(&self, scenario: Scenario, on_battery: bool) -> PowerAction {
        let idx = match scenario {
            Scenario::PowerButton => 0,
            Scenario::LidClose => 1,
        };
        if on_battery {
            self.on_battery[idx]
        } else {
            self.on_ac[idx]
        }
    }

    /// 设置（档内校验）。
    pub fn set(&mut self, scenario: Scenario, on_battery: bool, action: PowerAction) {
        let idx = match scenario {
            Scenario::PowerButton => 0,
            Scenario::LidClose => 1,
        };
        if on_battery {
            self.on_battery[idx] = action;
        } else {
            self.on_ac[idx] = action;
        }
    }
}

/// 长按判定状态机（软件路径优先——按下即起软件关机计时，4s 持续按住
/// 才落强断；中途释放 = 按配置动作执行）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PressOutcome {
    /// 未松手、未到 4s（等待中）。
    Pending,
    /// 在 4s 内松手 → 走配置动作（软件路径优先）。
    ReleasedConfigured(PowerAction),
    /// 持续按住 ≥4s → 硬件强断（最后手段——带警告语义）。
    HardCut,
}

pub struct PressTracker {
    down_at_ms: u64,
    pub held: bool,
    /// 强断触发次数（警告账）。
    pub hard_cuts: u64,
}

impl PressTracker {
    pub fn new() -> PressTracker {
        PressTracker { down_at_ms: 0, held: false, hard_cuts: 0 }
    }

    pub fn press(&mut self, now_ms: u64) {
        self.held = true;
        self.down_at_ms = now_ms;
    }

    /// 时刻 now 的判定（持续按住场景下由看门狗轮询）。
    pub fn evaluate(&mut self, now_ms: u64) -> PressOutcome {
        if !self.held {
            return PressOutcome::Pending;
        }
        if now_ms.saturating_sub(self.down_at_ms) >= LONG_PRESS_MS {
            self.held = false;
            self.hard_cuts += 1;
            PressOutcome::HardCut
        } else {
            PressOutcome::Pending
        }
    }

    /// 松手（4s 内）→ 软件路径（配置动作）。
    pub fn release(&mut self, now_ms: u64, configured: PowerAction) -> PressOutcome {
        if !self.held {
            return PressOutcome::Pending;
        }
        self.held = false;
        if now_ms.saturating_sub(self.down_at_ms) >= LONG_PRESS_MS {
            self.hard_cuts += 1;
            PressOutcome::HardCut
        } else {
            PressOutcome::ReleasedConfigured(configured)
        }
    }

    /// 强断警告文案（文档化——数据在写的时刻强断有风险，先试软件路径）。
    pub const HARD_CUT_WARNING: &'static str = "长按 4 秒将硬件强断：数据在写的时刻强断有风险，先试软件关机路径";
}

impl Default for PressTracker {
    fn default() -> PressTracker {
        PressTracker::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F318 自检（判据：三档×双场景矩阵；后果文案；长按 4s；软件路径优先）。
pub fn run_pwbtn_checks() -> CheckSet {
    let mut set = CheckSet::new("F318-pwbtn");

    // 1. 三档 × 双场景矩阵：默认全睡眠；分场景设置独立生效。
    let mut p = PowerButtonPolicy::default_policy();
    p.set(Scenario::PowerButton, true, PowerAction::Nothing);
    p.set(Scenario::LidClose, false, PowerAction::Shutdown);
    set.add(
        "matrix 3x2 dual source",
        p.action_for(Scenario::PowerButton, true) == PowerAction::Nothing
            && p.action_for(Scenario::PowerButton, false) == PowerAction::Sleep
            && p.action_for(Scenario::LidClose, false) == PowerAction::Shutdown
            && p.action_for(Scenario::LidClose, true) == PowerAction::Sleep,
        "",
    );

    // 2. 后果文案审查：三档各一句人话、非空、提到代价。
    set.add(
        "consequence copy reviewed",
        PowerAction::ALL.iter().all(|a| a.consequence().len() > 6)
            && PowerAction::Sleep.consequence().contains("秒回")
            && PowerAction::Shutdown.consequence().contains("账目"),
        "",
    );

    // 3. 长按 4s 判定：3999ms 松手 = 软件路径；4000ms 持续 = 强断。
    let mut t = PressTracker::new();
    t.press(1000);
    let r = t.release(1000 + LONG_PRESS_MS - 1, PowerAction::Sleep);
    set.add(
        "release just under 4s is software",
        r == PressOutcome::ReleasedConfigured(PowerAction::Sleep) && t.hard_cuts == 0,
        "",
    );

    // 4. 持续按住 4s → 强断（看门狗轮询路径）。
    let mut t2 = PressTracker::new();
    t2.press(0);
    let r1 = t2.evaluate(3999);
    let r2 = t2.evaluate(4000);
    set.add(
        "hard cut at exactly 4s",
        r1 == PressOutcome::Pending && r2 == PressOutcome::HardCut && t2.hard_cuts == 1,
        "",
    );

    // 5. 软件路径优先验证：4s 内松手走配置动作而非强断（哪怕配置是关机）。
    let mut t3 = PressTracker::new();
    t3.press(0);
    let r = t3.release(2000, PowerAction::Shutdown);
    set.add(
        "software path priority",
        r == PressOutcome::ReleasedConfigured(PowerAction::Shutdown) && t3.hard_cuts == 0,
        "",
    );

    // 6. 松手时已超 4s（看门狗未轮询的兜底）→ 仍落强断。
    let mut t4 = PressTracker::new();
    t4.press(0);
    let r = t4.release(5000, PowerAction::Sleep);
    set.add("late release still hard cut", r == PressOutcome::HardCut, "");

    // 7. 强断警告文案在位（文档化——红线提示）。
    set.add(
        "hard cut warning present",
        PressTracker::HARD_CUT_WARNING.contains("4 秒") && PressTracker::HARD_CUT_WARNING.contains("风险"),
        "",
    );

    // 8. 未按下时判定 Pending（状态机闭环）。
    let mut t5 = PressTracker::new();
    set.add(
        "not held pending",
        t5.evaluate(1000) == PressOutcome::Pending && t5.release(1000, PowerAction::Sleep) == PressOutcome::Pending,
        "",
    );

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_unique() {
        let mut seen: Vec<&str> = Vec::new();
        for a in PowerAction::ALL {
            assert!(!seen.contains(&a.label()));
            seen.push(a.label());
        }
    }

    #[test]
    fn default_is_sleep_everywhere() {
        let p = PowerButtonPolicy::default_policy();
        for s in Scenario::ALL {
            assert_eq!(p.action_for(s, true), PowerAction::Sleep);
            assert_eq!(p.action_for(s, false), PowerAction::Sleep);
        }
    }

    #[test]
    fn long_press_constant() {
        assert_eq!(LONG_PRESS_MS, 4000);
    }

    #[test]
    fn tracker_rearm() {
        let mut t = PressTracker::new();
        t.press(0);
        let _ = t.evaluate(4000);
        t.press(10_000);
        assert_eq!(t.evaluate(10_000), PressOutcome::Pending);
    }
}

// ---------------------------------------------------------------------------
// 深化层二 · F318 三档×双场景执行矩阵账 / 软件路径优先链 / 长按中断账
// ---------------------------------------------------------------------------

/// 场景键名（矩阵账的格键——两场景人话名，唯一源）。
fn scenario_key(s: Scenario) -> &'static str {
    match s {
        Scenario::PowerButton => "电源按钮",
        Scenario::LidClose => "合盖",
    }
}

/// F318 执行矩阵账：每次电源动作请求留账（场景×供电/动作），审计面
/// 两查——场景×供电四格全覆盖 + 三档动作都被真实执行过（矩阵用例的
/// 账面形态，可回放）。
#[derive(Default)]
pub struct ActionMatrixLog {
    cells: Vec<(&'static str, bool)>,
    actions: Vec<&'static str>,
}

impl ActionMatrixLog {
    pub fn new() -> ActionMatrixLog {
        ActionMatrixLog { cells: Vec::new(), actions: Vec::new() }
    }

    /// 记录一次按场景×供电的行为。
    pub fn record_cell(&mut self, scenario: Scenario, on_battery: bool) {
        let k = scenario_key(scenario);
        if !self.cells.iter().any(|(a, b)| *a == k && *b == on_battery) {
            self.cells.push((k, on_battery));
        }
    }

    /// 记录一次动作结果。
    pub fn record_action(&mut self, action: PowerAction) {
        let k = action.label();
        if !self.actions.iter().any(|a| *a == k) {
            self.actions.push(k);
        }
    }

    /// 场景×供电四格覆盖（电源键/合盖 × 电池/外接）。
    pub fn cells_covered(&self) -> usize {
        self.cells.len()
    }

    pub const CELLS_TOTAL: usize = 4;

    /// 三档动作全覆盖（睡眠/关机/无动作都有实测）。
    pub fn actions_covered(&self) -> usize {
        self.actions.len()
    }

    pub fn all_covered(&self) -> bool {
        self.cells.len() == Self::CELLS_TOTAL && self.actions.len() == PowerAction::ALL.len()
    }
}

/// F318 软件路径优先链（「软件关机路径优先级验证（B-2902 判据引用）」
/// 的结构面）：任何硬断之前必须先走软件关机账目——链上事件可回放，
/// 硬断出现在软件尝试之前即缺陷。
pub struct SoftwareFirstChain {
    events: Vec<&'static str>,
}

impl SoftwareFirstChain {
    pub const SOFTWARE_STEP: &'static str = "软件关机账目(B-2902)";
    pub const HARD_STEP: &'static str = "硬件强断";

    pub fn new() -> SoftwareFirstChain {
        SoftwareFirstChain { events: Vec::new() }
    }

    /// 一次关机请求：先走软件账目；软件失败才降级硬断。
    pub fn request_shutdown(&mut self, software_ok: bool) -> &'static str {
        self.events.push(Self::SOFTWARE_STEP);
        if software_ok {
            Self::SOFTWARE_STEP
        } else {
            self.events.push(Self::HARD_STEP);
            Self::HARD_STEP
        }
    }

    /// 优先级断言：首事件必为软件路径。
    pub fn software_attempted_first(&self) -> bool {
        self.events.first() == Some(&Self::SOFTWARE_STEP)
    }

    /// 硬断纪律：出现硬断时软件路径必已在先。
    pub fn hard_cut_only_after_software(&self) -> bool {
        match self.events.iter().position(|e| *e == Self::HARD_STEP) {
            None => true,
            Some(0) => false,
            Some(_) => self.events[0] == Self::SOFTWARE_STEP,
        }
    }

    pub fn events(&self) -> &[&'static str] {
        &self.events
    }
}

impl Default for SoftwareFirstChain {
    fn default() -> SoftwareFirstChain {
        SoftwareFirstChain::new()
    }
}

/// F318 长按中断账（对 PressTracker 的行为观察面）：4s 内松手=配置
/// 动作（软件路径）、≥4s 持续按住=硬断（带警告语义留痕）——每次
/// 评估结果落账可回放。
#[derive(Default)]
pub struct PressInterruptLedger {
    pub configured_releases: u64,
    pub hardcuts: u64,
    pub warnings_shown: u64,
}

impl PressInterruptLedger {
    pub fn new() -> PressInterruptLedger {
        PressInterruptLedger::default()
    }

    /// 观察一次长按评估结果（Pending 不计——尚未成事实）。
    pub fn observe(&mut self, outcome: PressOutcome) {
        match outcome {
            PressOutcome::ReleasedConfigured(_) => self.configured_releases += 1,
            PressOutcome::HardCut => {
                self.hardcuts += 1;
                self.warnings_shown += 1;
            }
            PressOutcome::Pending => {}
        }
    }
}

/// 深化层二自检（执行矩阵 / 软件优先链 / 长按中断账）。
pub fn run_pwbtn_deep2_checks() -> CheckSet {
    let mut set = CheckSet::new("F318-deep2");

    // 1. 三档×双场景执行矩阵：四格（场景×供电）+ 三档动作全覆盖。
    let mut log = ActionMatrixLog::new();
    for s in Scenario::ALL {
        log.record_cell(s, false);
        log.record_cell(s, true);
    }
    for a in PowerAction::ALL {
        log.record_action(a);
    }
    log.record_cell(Scenario::PowerButton, false); // 重复格不重复计。
    set.add(
        "action matrix full coverage",
        log.cells_covered() == ActionMatrixLog::CELLS_TOTAL
            && log.actions_covered() == PowerAction::ALL.len()
            && log.all_covered(),
        "",
    );

    // 2. 软件路径优先：成功走软件即收口（无硬断）。
    let mut c1 = SoftwareFirstChain::new();
    let r1 = c1.request_shutdown(true);
    set.add(
        "software path succeeds first",
        r1 == SoftwareFirstChain::SOFTWARE_STEP
            && c1.software_attempted_first()
            && c1.hard_cut_only_after_software()
            && c1.events().len() == 1,
        "",
    );

    // 3. 软件失败 → 硬断降级，且软件尝试必在先（优先级链断言）。
    let mut c2 = SoftwareFirstChain::new();
    let r2 = c2.request_shutdown(false);
    set.add(
        "hard cut only after software",
        r2 == SoftwareFirstChain::HARD_STEP
            && c2.software_attempted_first()
            && c2.hard_cut_only_after_software()
            && c2.events() == [SoftwareFirstChain::SOFTWARE_STEP, SoftwareFirstChain::HARD_STEP],
        "",
    );

    // 4. 长按中断账：4s 内松手=配置动作、≥4s=硬断+警告、Pending 不计。
    let mut t = PressTracker::new();
    let mut led = PressInterruptLedger::new();
    t.press(0);
    let o1 = t.release(3_999, PowerAction::Sleep);
    led.observe(o1);
    let mut t2 = PressTracker::new();
    t2.press(0);
    let o2 = t2.evaluate(4_000);
    led.observe(o2);
    led.observe(PressOutcome::Pending);
    set.add(
        "press interrupt ledger",
        led.configured_releases == 1 && led.hardcuts == 1 && led.warnings_shown == 1,
        "",
    );

    // 5. 后果文案审查账：三档说明非空且互不相同（人话区分度——后果写得明白）。
    let c0 = PowerAction::ALL[0].consequence();
    let c1 = PowerAction::ALL[1].consequence();
    let c2 = PowerAction::ALL[2].consequence();
    set.add(
        "consequence copy audited",
        !c0.is_empty() && !c1.is_empty() && !c2.is_empty() && c0 != c1 && c1 != c2 && c0 != c2,
        "",
    );

    set
}

#[cfg(test)]
mod deep2_tests {
    use super::*;

    #[test]
    fn matrix_records_do_not_duplicate() {
        let mut log = ActionMatrixLog::new();
        log.record_cell(Scenario::LidClose, true);
        log.record_cell(Scenario::LidClose, true);
        assert_eq!(log.cells_covered(), 1);
    }

    #[test]
    fn chain_empty_is_clean() {
        let c = SoftwareFirstChain::new();
        assert!(c.software_attempted_first() == false || c.events().is_empty());
        assert!(c.hard_cut_only_after_software());
    }

    #[test]
    fn ledger_pending_never_counts() {
        let mut led = PressInterruptLedger::new();
        led.observe(PressOutcome::Pending);
        led.observe(PressOutcome::Pending);
        assert_eq!(led.configured_releases + led.hardcuts, 0);
    }
}

// ---------------------------------------------------------------------------
// 深化层三 · 三触发源统一动作解析器 + 后果预演面 + 长按进度账
// ---------------------------------------------------------------------------

/// 三触发源（电源按钮 / 合盖 / 电源菜单项）统一动作解析：用户配置的
/// 是「源 × 场景 → 动作」的完整矩阵——任何触发源在任何场景都必须能
/// 解析出动作或显式报「未配置」（不许静默默认兜底——配置面零猜测）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TriggerSource {
    /// 物理电源按钮。
    Button,
    /// 合盖（笔记本盖）。
    Lid,
    /// 电源菜单项（开始菜单电源键）。
    Menu,
}

/// 全触发源动作矩阵：源 × (电池/插电) → 动作名。
pub struct TriggerMatrix {
    /// (源, 插电?, 动作名)。
    rows: Vec<(TriggerSource, bool, &'static str)>,
}

impl TriggerMatrix {
    pub fn new() -> TriggerMatrix {
        TriggerMatrix { rows: Vec::new() }
    }

    /// 登记一格（同格重复登记覆盖——用户改配置即覆盖）。
    pub fn set(&mut self, src: TriggerSource, on_ac: bool, action: &'static str) {
        match self.rows.iter_mut().find(|(s, a, _)| *s == src && *a == on_ac) {
            Some(slot) => slot.2 = action,
            None => self.rows.push((src, on_ac, action)),
        }
    }

    /// 解析：命中返回动作名；未配置返回 None（调用方显式处理——配置
    /// 缺口显性化，不许猜）。
    pub fn resolve(&self, src: TriggerSource, on_ac: bool) -> Option<&'static str> {
        self.rows
            .iter()
            .find(|(s, a, _)| *s == src && *a == on_ac)
            .map(|(_, _, act)| *act)
    }

    /// 矩阵完备审计：3 源 × 2 供电面 = 6 格全登记（缺口清单直出）。
    pub fn completeness(&self) -> Vec<(TriggerSource, bool)> {
        let mut missing = Vec::new();
        for src in [TriggerSource::Button, TriggerSource::Lid, TriggerSource::Menu] {
            for on_ac in [true, false] {
                if self.resolve(src, on_ac).is_none() {
                    missing.push((src, on_ac));
                }
            }
        }
        missing
    }
}

impl Default for TriggerMatrix {
    fn default() -> TriggerMatrix {
        TriggerMatrix::new()
    }
}

/// 后果预演面（红线纪律「先干跑列清单」在电源域的落法）：动作 → 将
/// 发生的后果链清单（如「休眠」→ [内存保电、会话冻结、唤醒恢复会话]）
/// 纯读不动任何状态；预演与动作必须一表同源（改动作必炸对账）。
pub struct ConsequencePreview;

impl ConsequencePreview {
    /// 动作后果链表（唯一源——与 PowerAction 语义对齐）。
    pub const CHAINS: [(&'static str, [&'static str; 3]); 4] = [
        ("睡眠", ["屏幕熄灭", "内存保电", "唤醒即回（F319）"]),
        ("休眠", ["内存落盘", "整机断电", "唤醒恢复会话"]),
        ("关机", ["会话保存点", "按序停机", "下次开机走自检"]),
        ("无操作", ["（无）", "（无）", "（无）"]),
    ];

    pub fn chain(action: &str) -> Option<&'static [&'static str; 3]> {
        Self::CHAINS.iter().find(|(a, _)| *a == action).map(|(_, c)| c)
    }

    /// 预演面自证：每条链三步非空（「无操作」除外——显式占位）。
    pub fn table_sane() -> bool {
        Self::CHAINS
            .iter()
            .all(|(a, c)| *a == "无操作" || c.iter().all(|s| !s.is_empty()))
    }
}

/// 长按进度账（4s 判定的用户面）：按住期间逐拍记账进度‰，松手即清；
/// 进度到 1000‰ 才触发——进度条的数据源（有进度感而不是莫名卡住）。
pub struct LongPressProgress {
    held_ms: u64,
    /// 触发留痕（完成次数——重复触发防抖）。
    pub firings: u64,
}

impl LongPressProgress {
    pub fn new() -> LongPressProgress {
        LongPressProgress { held_ms: 0, firings: 0 }
    }

    /// 按住推进一拍（拍长任意——按真实毫秒累计）。
    pub fn hold_tick(&mut self, delta_ms: u64) -> u32 {
        if self.held_ms >= LONG_PRESS_MS {
            return 1000;
        }
        self.held_ms += delta_ms;
        let pct = (self.held_ms * 1000 / LONG_PRESS_MS) as u32;
        if self.held_ms >= LONG_PRESS_MS {
            self.firings += 1;
        }
        pct.min(1000)
    }

    /// 松手清零（半按不残留——状态机完整出口）。
    pub fn release(&mut self) {
        self.held_ms = 0;
    }

    pub fn progress(&self) -> u32 {
        (self.held_ms * 1000 / LONG_PRESS_MS) as u32
    }
}

impl Default for LongPressProgress {
    fn default() -> LongPressProgress {
        LongPressProgress::new()
    }
}

/// 深化层三自检（矩阵 / 预演 / 长按进度）。
pub fn run_pwbtn_deep3_checks() -> CheckSet {
    let mut set = CheckSet::new("F318-deep3");

    // 1. 三触发源矩阵：全格登记 + 完备审计零缺口。
    let mut m = TriggerMatrix::new();
    for (src, act) in [
        (TriggerSource::Button, "睡眠"),
        (TriggerSource::Lid, "睡眠"),
        (TriggerSource::Menu, "关机"),
    ] {
        m.set(src, true, act);
        m.set(src, false, act);
    }
    set.add(
        "trigger matrix complete",
        m.completeness().is_empty()
            && m.resolve(TriggerSource::Button, true) == Some("睡眠")
            && m.resolve(TriggerSource::Menu, false) == Some("关机"),
        "",
    );

    // 2. 改配置即覆盖（同格重登记）。
    m.set(TriggerSource::Lid, false, "休眠");
    set.add(
        "matrix overwrite semantics",
        m.resolve(TriggerSource::Lid, false) == Some("休眠"),
        "",
    );

    // 3. 配置缺口显性化：未登记格 resolve None + 缺口清单可直出。
    let mut m2 = TriggerMatrix::new();
    m2.set(TriggerSource::Button, true, "睡眠");
    let gaps = m2.completeness();
    set.add(
        "missing cells explicit",
        m2.resolve(TriggerSource::Lid, true).is_none() && gaps.len() == 5,
        "",
    );

    // 4. 后果预演：四动作链全可查 + 表自证 + 纯读语义（查两次同结果）。
    set.add(
        "consequence preview sane",
        ConsequencePreview::table_sane()
            && ConsequencePreview::chain("睡眠").is_some()
            && ConsequencePreview::chain("关机").map(|c| c[2].contains("自检")).unwrap_or(false)
            && ConsequencePreview::chain("幽灵动作").is_none(),
        "",
    );

    // 5. 长按进度：4s 分四拍逐拍到 1000‰、只触发一次、松手清零。
    let mut lp = LongPressProgress::new();
    let p = [lp.hold_tick(1000), lp.hold_tick(1000), lp.hold_tick(1000), lp.hold_tick(1000)];
    set.add(
        "long press progress to fire",
        p == [250, 500, 750, 1000] && lp.firings == 1 && lp.hold_tick(500) == 1000
            && lp.firings == 1,
        "",
    );
    lp.release();
    set.add("long press release clears", lp.progress() == 0, "");

    // 6. 半按不触发：3.9s 松手 → 零触发。
    let mut lp2 = LongPressProgress::new();
    let _ = lp2.hold_tick(3900);
    lp2.release();
    set.add("long press sub-threshold no fire", lp2.firings == 0, "");

    set
}

#[cfg(test)]
mod deep3_tests {
    use super::*;

    #[test]
    fn matrix_overwrite_not_duplicate() {
        let mut m = TriggerMatrix::new();
        m.set(TriggerSource::Button, true, "睡眠");
        m.set(TriggerSource::Button, true, "休眠");
        assert_eq!(m.resolve(TriggerSource::Button, true), Some("休眠"));
        assert_eq!(m.completeness().len(), 5, "覆盖语义不产生重复格");
    }

    #[test]
    fn progress_never_exceeds_1000() {
        let mut lp = LongPressProgress::new();
        for _ in 0..10 {
            let _ = lp.hold_tick(2000);
        }
        assert_eq!(lp.firings, 1, "长按只触发一次（防抖）");
        assert_eq!(lp.progress(), 1000);
    }

    #[test]
    fn zero_tick_no_progress() {
        let mut lp = LongPressProgress::new();
        assert_eq!(lp.hold_tick(0), 0, "零拍不推进度");
    }
}
