//! VE-F3549 · persona 与通知样式联动 —— 样式可换 + 行为不换 + 越界拒绝。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3549`
//!
//! # 职责（锚点原文拆解）
//!
//! - **样式可换**（[`StyleSlot`]）：音效 / 横幅风格两槽可个性化。
//! - **行为不换**（[`BehaviorSlot`]）：勿扰 / 优先级 / 超时行为
//!   三槽**不可被 Persona 修改**——通知行为是系统事不是装扮事。
//! - **越界拒绝**（[`NotifyBoundary::apply`]）：越界调用一律拒
//!   [`NotifyCode::BehaviorOverride`]，并有边界读屏说明。
//!
//! # 降级矩阵（锚点）
//!
//! | 故障 | 处置 |
//! |---|---|
//! | 越界改行为 | 拒绝 |
//! | 样式违例 | 回退默认 |
//! | 边界漂移 | 修正 |
//!
//! # 复杂度
//!
//! O(1)。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E19 段续：6160 起）
// ---------------------------------------------------------------------------

/// 通知联动诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NotifyCode {
    /// 越界改行为（已拒）。
    BehaviorOverride = 6160,
    /// 样式违例（已回退）。
    StyleViolation = 6161,
    /// 边界漂移（已修正）。
    BoundaryDrift = 6162,
}

impl NotifyCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            NotifyCode::BehaviorOverride => "越界修改通知行为已拒绝",
            NotifyCode::StyleViolation => "样式违例已回退默认",
            NotifyCode::BoundaryDrift => "边界漂移已修正",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        matches!(self, NotifyCode::BehaviorOverride)
    }
}

// ---------------------------------------------------------------------------
// 二、样式槽与行为槽
// ---------------------------------------------------------------------------

/// 可个性化样式槽。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StyleSlot {
    /// 通知音效。
    Sound,
    /// 横幅风格。
    Banner,
}

impl StyleSlot {
    /// 槽短名。
    pub const fn label(self) -> &'static str {
        match self {
            StyleSlot::Sound => "通知音效",
            StyleSlot::Banner => "横幅风格",
        }
    }
}

/// 不可个性化行为槽（系统事）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BehaviorSlot {
    /// 勿扰行为。
    DoNotDisturb,
    /// 优先级规则。
    Priority,
    /// 超时消失时机。
    Timeout,
}

impl BehaviorSlot {
    /// 槽短名。
    pub const fn label(self) -> &'static str {
        match self {
            BehaviorSlot::DoNotDisturb => "勿扰行为",
            BehaviorSlot::Priority => "优先级规则",
            BehaviorSlot::Timeout => "超时时机",
        }
    }
}

/// 样式取值 id（0 = 默认）。
pub type StyleValue = u32;

// ---------------------------------------------------------------------------
// 三、边界器
// ---------------------------------------------------------------------------

/// 通知边界器（样式可换、行为不换）。
pub struct NotifyBoundary {
    sound: StyleValue,
    banner: StyleValue,
    /// 越界拦截计数。
    pub blocked: u32,
    /// 样式回退计数。
    pub style_rollbacks: u32,
}

impl NotifyBoundary {
    /// 新建（全默认）。
    pub fn new() -> Self {
        Self {
            sound: 0,
            banner: 0,
            blocked: 0,
            style_rollbacks: 0,
        }
    }

    /// 应用样式（允许）。
    pub fn apply_style(&mut self, slot: StyleSlot, value: StyleValue) -> Result<(), NotifyCode> {
        // 样式值域 0..=99；越界回退默认。
        if value >= 100 {
            self.style_rollbacks = self.style_rollbacks.saturating_add(1);
            match slot {
                StyleSlot::Sound => self.sound = 0,
                StyleSlot::Banner => self.banner = 0,
            }
            return Err(NotifyCode::StyleViolation);
        }
        match slot {
            StyleSlot::Sound => self.sound = value,
            StyleSlot::Banner => self.banner = value,
        }
        Ok(())
    }

    /// 尝试改行为（一律拒绝）。
    pub fn apply_behavior(
        &mut self,
        _slot: BehaviorSlot,
        _value: StyleValue,
    ) -> Result<(), NotifyCode> {
        self.blocked = self.blocked.saturating_add(1);
        Err(NotifyCode::BehaviorOverride)
    }

    /// 当前样式快照（读屏可达）。
    pub fn snapshot(&self) -> (StyleValue, StyleValue) {
        (self.sound, self.banner)
    }

    /// 边界读屏说明。
    pub fn explain(&self) -> &'static str {
        "音效与横幅风格可个性化；勿扰、优先级、超时行为由系统管理不可装扮"
    }
}

impl Default for NotifyBoundary {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 四、判据
// ---------------------------------------------------------------------------

/// 通知联动判据集。
pub fn run_notify_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-ver10i_notify");
    let mut b = NotifyBoundary::new();

    // 1. 样式可换。
    let ok_style = b.apply_style(StyleSlot::Sound, 7).is_ok()
        && b.apply_style(StyleSlot::Banner, 3).is_ok();
    set.add("6160-样式可换", ok_style && b.snapshot() == (7, 3), "");

    // 2. 行为不换。
    let ok_block = b.apply_behavior(BehaviorSlot::DoNotDisturb, 1).is_err();
    set.add("6161-行为不换", ok_block && b.blocked == 1, "");

    // 3. 样式违例回退。
    let viol = b.apply_style(StyleSlot::Sound, 500).is_err();
    set.add(
            "6162-样式违例回退",
            viol && b.snapshot().0 == 0 && b.style_rollbacks == 1,
            "",
        );

    // 4. 三行为槽全拒。
    let mut b2 = NotifyBoundary::new();
    let all_reject = b2.apply_behavior(BehaviorSlot::Priority, 1).is_err()
        && b2.apply_behavior(BehaviorSlot::Timeout, 1).is_err()
        && b2.apply_behavior(BehaviorSlot::DoNotDisturb, 1).is_err();
    set.add("6163-行为槽全拒", all_reject && b2.blocked == 3, "");

    // 5. 边界说明可达。
    set.add("6164-边界说明", !b.explain().is_empty(), "");

    // 6. 槽名读屏可达。
    set.add(
            "6165-槽名可达",
            StyleSlot::Sound.label() == "通知音效" && BehaviorSlot::Timeout.label() == "超时时机",
            "",
        );

    set
}
