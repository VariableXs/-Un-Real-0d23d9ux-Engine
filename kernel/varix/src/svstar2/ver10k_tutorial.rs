//! VE-F3551 · persona 教程与发现 —— 首次引导 + 空态展示 + 示例链接 + 只出现一次可找回。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3551`
//!
//! # 职责（锚点原文拆解）
//!
//! - **首次引导**（[`Tutorial::maybe_show`]）：第一次用才弹，
//!   频控保证只出现一次。
//! - **空态展示**（[`EmptyState`]）：没配置时展示空态卡
//!   （含示例链接），不是空白页。
//! - **可找回**（[`Tutorial::restore`]）：跳过/看过后仍能从
//!   帮助入口找回——跳过不等于永久丢失。
//!
//! # 降级矩阵（锚点）
//!
//! | 故障 | 处置 |
//! |---|---|
//! | 引导重复 | 频控（只一次） |
//! | 内容缺失 | 补 |
//! | 跳过不可找回 | 修复（restore 通道） |
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
// 一、诊断码（E19 段续：6200 起）
// ---------------------------------------------------------------------------

/// 教程发现诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TutorialCode {
    /// 引导已展示过（频控拦）。
    AlreadyShown = 6200,
    /// 内容缺失。
    ContentMissing = 6201,
    /// 找回通道失效。
    RestoreBroken = 6202,
}

impl TutorialCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            TutorialCode::AlreadyShown => "引导已展示过被频控",
            TutorialCode::ContentMissing => "教程内容缺失",
            TutorialCode::RestoreBroken => "找回通道失效",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// 二、教程内容三件
// ---------------------------------------------------------------------------

/// 教程三件（缺任一件判 [`TutorialCode::ContentMissing`]）。
pub struct TutorialContent {
    /// 首次使用引导正文。
    pub guide_text: &'static str,
    /// 空态展示文案。
    pub empty_text: &'static str,
    /// 示例链接 id（0 = 无）。
    pub example_id: u32,
}

impl TutorialContent {
    /// 内容完整性。
    pub fn complete(&self) -> bool {
        !self.guide_text.is_empty() && !self.empty_text.is_empty() && self.example_id != 0
    }
}

// ---------------------------------------------------------------------------
// 三、空态卡
// ---------------------------------------------------------------------------

/// 空态卡。
#[derive(Clone, Copy, Debug)]
pub struct EmptyState {
    /// 是否有配置（false = 空态）。
    pub has_config: bool,
    /// 示例链接 id。
    pub example_id: u32,
}

impl EmptyState {
    /// 应展示空态卡。
    pub const fn should_show(&self) -> bool {
        !self.has_config
    }
}

// ---------------------------------------------------------------------------
// 四、教程（频控状态机）
// ---------------------------------------------------------------------------

/// 引导展示状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShownState {
    /// 从未展示。
    Never,
    /// 已展示（含跳过）。
    Shown,
}

/// 教程控制器。
pub struct Tutorial {
    content: TutorialContent,
    state: ShownState,
    /// 找回次数（审计）。
    pub restores: u32,
}

impl Tutorial {
    /// 新建（内容必须完整）。
    pub fn new(content: TutorialContent) -> Result<Self, TutorialCode> {
        if !content.complete() {
            return Err(TutorialCode::ContentMissing);
        }
        Ok(Self {
            content,
            state: ShownState::Never,
            restores: 0,
        })
    }

    /// 首次引导（只放行一次）。
    pub fn maybe_show(&mut self) -> Result<&'static str, TutorialCode> {
        match self.state {
            ShownState::Never => {
                self.state = ShownState::Shown;
                Ok(self.content.guide_text)
            }
            ShownState::Shown => Err(TutorialCode::AlreadyShown),
        }
    }

    /// 找回（帮助入口；不受频控限制）。
    pub fn restore(&mut self) -> &'static str {
        self.restores = self.restores.saturating_add(1);
        self.content.guide_text
    }

    /// 空态卡渲染。
    pub fn empty_card(&self, s: EmptyState) -> Option<&'static str> {
        if s.should_show() {
            Some(self.content.empty_text)
        } else {
            None
        }
    }

    /// 当前展示状态。
    pub fn state(&self) -> ShownState {
        self.state
    }

    /// 示例链接 id。
    pub fn example_id(&self) -> u32 {
        self.content.example_id
    }
}

// ---------------------------------------------------------------------------
// 五、判据
// ---------------------------------------------------------------------------

/// 教程发现判据集。
pub fn run_tutorial_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-ver10k_tutorial");

    let content = TutorialContent {
        guide_text: "三步换上你的人格：选预览、看效果、点应用。",
        empty_text: "还没有配置——看看示例找灵感。",
        example_id: 42,
    };

    // 1. 内容完整可建。
    let t = Tutorial::new(content);
    set.add("6200-内容完整", t.is_ok(), "");

    // 2. 首次引导放行、二次拦。
    let mut t2 = match Tutorial::new(TutorialContent {
        guide_text: "引导",
        empty_text: "空态",
        example_id: 1,
    }) {
        Ok(t) => t,
        Err(_) => return set,
    };
    // 上面两段新建必然成功（内容非空）；演示 maybe_show 频控：
    let first = t2.maybe_show().is_ok();
    let second = t2.maybe_show().is_err();
    set.add("6201-只出现一次", first && second, "");

    // 3. 找回不受频控。
    let restored = !t2.restore().is_empty() && t2.restores == 1;
    set.add("6202-可找回", restored, "");

    // 4. 空态展示。
    let card = t2.empty_card(EmptyState {
        has_config: false,
        example_id: 1,
    });
    set.add("6203-空态展示", card == Some("空态"), "");

    // 5. 有配置不弹空态。
    let no_card = t2.empty_card(EmptyState {
        has_config: true,
        example_id: 1,
    });
    set.add("6204-有配置不弹", no_card.is_none(), "");

    // 6. 内容不全拒建。
    let bad = Tutorial::new(TutorialContent {
        guide_text: "",
        empty_text: "x",
        example_id: 1,
    });
    set.add("6205-内容缺失拒绝", bad.is_err(), "");

    // 7. 示例链接 id 有效。
    set.add("6206-示例链接", t2.example_id() == 1, "");

    set
}
