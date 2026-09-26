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
