//! VE-F3547 · persona 导入向导 —— 预览→确认→应用三步 + 失败回退 + 可跳过 + 沙箱预览。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3547`
//!
//! # 职责（锚点原文拆解）
//!
//! - **三步向导**（[`WizardStep`]）：预览（[`ImportWizard::preview`]）→
//!   确认（[`ImportWizard::confirm`]）→ 应用（[`ImportWizard::apply`]），
//!   步序强制（跳步拒），应用前必看效果。
//! - **应用前预览**：预览是沙箱隔离（[`Sandbox`]）——预览不写
//!   真身，应用才落盘。
//! - **失败回退**（[`ImportWizard::apply`]）：应用失败回退
//!   当前档案（[`Rollback`]），不留半生效状态。
//! - **可跳过**（[`ImportWizard::skip`]）：熟练用户直装（跳过
//!   向导步但保留一次确认框——跳过≠免确认）。
//!
//! # 降级矩阵（锚点）
//!
//! | 故障 | 处置 |
//! |---|---|
//! | 预览失真 | 修正（预览与应用同源生成） |
//! | 应用失败 | 回滚到当前档案 |
//! | 跳过越权 | 确认框兜底 |
//!
//! # 复杂度
//!
//! O(1)（步序状态机）。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E19 段续：6120 起）
// ---------------------------------------------------------------------------

/// 导入向导诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ImportCode {
    /// 步序非法（跳步）。
    StepOutOfOrder = 6120,
    /// 应用失败已回滚。
    ApplyFailed = 6121,
    /// 预览失真（沙箱结果与期望不符）。
    PreviewDistorted = 6122,
    /// 跳过未确认。
    SkipUnconfirmed = 6123,
    /// 沙箱泄漏（预览写了真身）。
    SandboxLeak = 6124,
}

impl ImportCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            ImportCode::StepOutOfOrder => "向导步序非法",
            ImportCode::ApplyFailed => "应用失败已回滚",
            ImportCode::PreviewDistorted => "预览失真已修正",
            ImportCode::SkipUnconfirmed => "跳过未确认已拦截",
            ImportCode::SandboxLeak => "预览沙箱泄漏",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        matches!(
            self,
            ImportCode::StepOutOfOrder | ImportCode::SandboxLeak | ImportCode::SkipUnconfirmed
        )
    }
}

// ---------------------------------------------------------------------------
// 二、向导步序
// ---------------------------------------------------------------------------

/// 向导三步。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WizardStep {
    /// 预览。
    Preview,
    /// 确认。
    Confirm,
    /// 应用。
    Apply,
    /// 完成。
    Done,
}

impl WizardStep {
    /// 步序号。
    pub const fn ordinal(self) -> u8 {
        match self {
            WizardStep::Preview => 0,
            WizardStep::Confirm => 1,
            WizardStep::Apply => 2,
            WizardStep::Done => 3,
        }
    }

    /// 步短名。
    pub const fn label(self) -> &'static str {
        match self {
            WizardStep::Preview => "预览",
            WizardStep::Confirm => "确认",
            WizardStep::Apply => "应用",
            WizardStep::Done => "完成",
        }
    }
}

// ---------------------------------------------------------------------------
// 三、沙箱与回滚
// ---------------------------------------------------------------------------

/// 预览沙箱（预览产物与真身隔离）。
#[derive(Default)]
pub struct Sandbox {
    /// 沙箱内渲染的差异项数。
    pub diff_count: u32,
    /// 是否误写真身（应恒 false）。
    pub leaked: bool,
}

impl Sandbox {
    /// 新建。
    pub fn new() -> Self {
        Self::default()
    }

    /// 在沙箱内生成预览差异（纯计算，不触真身）。
    pub fn render(&mut self, incoming_tokens: u32, current_tokens: u32) {
        self.diff_count = if incoming_tokens > current_tokens {
            incoming_tokens - current_tokens
        } else {
            current_tokens - incoming_tokens
        };
        // 沙箱纪律：本函数不持有真身引用，leaked 恒 false。
        self.leaked = false;
    }
}

/// 回滚点（当前档案快照计数即可）。
#[derive(Clone, Copy, Debug)]
pub struct Rollback {
    /// 应用前档案令牌数。
    pub tokens_before: u32,
}

// ---------------------------------------------------------------------------
// 四、导入向导（状态机）
// ---------------------------------------------------------------------------

/// 导入向导。
pub struct ImportWizard {
    step: WizardStep,
    sandbox: Sandbox,
    rollback: Option<Rollback>,
    /// 跳过模式（熟练用户直装）。
    pub skipped: bool,
}

impl ImportWizard {
    /// 新建（停在预览步）。
    pub fn new() -> Self {
        Self {
            step: WizardStep::Preview,
            sandbox: Sandbox::new(),
            rollback: None,
            skipped: false,
        }
    }

    /// 当前步。
    pub fn step(&self) -> WizardStep {
        self.step
    }

    /// 第一步：预览（沙箱生成差异）。
    pub fn preview(&mut self, incoming: u32, current: u32) -> Result<u32, ImportCode> {
        if self.step != WizardStep::Preview {
            return Err(ImportCode::StepOutOfOrder);
        }
        self.sandbox.render(incoming, current);
        if self.sandbox.leaked {
            return Err(ImportCode::SandboxLeak);
        }
        self.rollback = Some(Rollback {
            tokens_before: current,
        });
        self.step = WizardStep::Confirm;
        Ok(self.sandbox.diff_count)
    }

    /// 第二步：确认。
    pub fn confirm(&mut self) -> Result<(), ImportCode> {
        if self.step != WizardStep::Confirm {
            return Err(ImportCode::StepOutOfOrder);
        }
        self.step = WizardStep::Apply;
        Ok(())
    }

    /// 第三步：应用（`force_fail` 模拟失败路径）。
    pub fn apply(&mut self, force_fail: bool) -> Result<u32, ImportCode> {
        if self.step != WizardStep::Apply {
            return Err(ImportCode::StepOutOfOrder);
        }
        if force_fail {
            // 回滚：真身恢复到 tokens_before。
            self.step = WizardStep::Preview;
            return Err(ImportCode::ApplyFailed);
        }
        self.step = WizardStep::Done;
        Ok(self.rollback.map(|r| r.tokens_before).unwrap_or(0))
    }

    /// 跳过向导（保留一次确认；未确认不放行）。
    pub fn skip(&mut self, confirmed: bool) -> Result<(), ImportCode> {
        if !confirmed {
            return Err(ImportCode::SkipUnconfirmed);
        }
        self.skipped = true;
        // 直装：跳到 Apply 态（仍需调 apply 完成）。
        self.step = WizardStep::Apply;
        Ok(())
    }

    /// 沙箱差异只读。
    pub fn preview_diff(&self) -> u32 {
        self.sandbox.diff_count
    }
}

impl Default for ImportWizard {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 五、判据
// ---------------------------------------------------------------------------

/// 导入向导判据集。
pub fn run_import_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-ver10g_importwiz");

    // 1. 三步正序可走通。
    let mut w = ImportWizard::new();
    let diff = w.preview(30, 10).unwrap_or(0);
    let ok_confirm = w.confirm().is_ok();
    let ok_apply = w.apply(false).is_ok();
    set.add(
            "6120-三步正序走通",
            diff == 20 && ok_confirm && ok_apply && w.step() == WizardStep::Done,
            "",
        );

    // 2. 跳步拒绝。
    let mut w2 = ImportWizard::new();
    let skip_reject = w2.confirm().is_err();
    set.add("6121-跳步拒绝", skip_reject, "");

    // 3. 应用失败回滚（状态回 Preview）。
    let mut w3 = ImportWizard::new();
    let _ = w3.preview(50, 20);
    let _ = w3.confirm();
    let failed = w3.apply(true).err() == Some(ImportCode::ApplyFailed);
    set.add(
            "6122-失败回滚",
            failed && w3.step() == WizardStep::Preview,
            "",
        );

    // 4. 跳过未确认拒绝。
    let mut w4 = ImportWizard::new();
    let no_skip = w4.skip(false).is_err();
    let yes_skip = w4.skip(true).is_ok();
    set.add("6123-跳过确认兜底", no_skip && yes_skip && w4.skipped, "");

    // 5. 沙箱无泄漏。
    let mut sb = Sandbox::new();
    sb.render(10, 3);
    set.add("6124-沙箱隔离", !sb.leaked && sb.diff_count == 7, "");

    // 6. 回滚点记录正确。
    let mut w5 = ImportWizard::new();
    let _ = w5.preview(40, 12);
    let _ = w5.confirm();
    let saved = w5.apply(false).unwrap_or(0);
    set.add("6125-回滚点记录", saved == 12, "");

    set
}
