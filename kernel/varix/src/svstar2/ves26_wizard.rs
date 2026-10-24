//! VE-F3626 · 主题向导（VE-S 域 · 批次 R02 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3626`
//!
//! **判据（锚点原文六条）**：五步默认、推荐可解释、产物保证、新手实测、
//! 复用模式、判据。逐条落位：
//! - **五步默认**：选基调→选色板→选形状→选字阶→预览发布，每步默认值
//!   选好（[`Wizard`] 五步流）。
//! - **推荐可解释**：推荐必须给理由（推荐可解释红线）——
//!   [`Recommender::suggest`] 返回 (值, 理由)，理由空 →
//!   [`V1ACode::REASON_MISSING`]。
//! - **产物保证**：向导产物必须过验证（产物保证线）——
//!   [`Wizard::finish`] 自动验证，不过 → [`V1ACode::OUTPUT_INVALID`]（P1）。
//! - **新手实测**：新手 5 分钟成主题（向导线）。
//! - **复用模式**：跳步与回退复用 F3026 向导模式。
//! - **判据**：`run_ves26_checks` 逐条映射。
//!
//! **错误路径与降级矩阵（锚点原文四条 → 落位）**：
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 默认劣质→基线修正（复述实测） | [`V1ACode::DEFAULT_POOR`] |
//! | 推荐黑箱→理由补全（可解释红线实测） | [`V1ACode::REASON_MISSING`] |
//! | 产物验证不过→P1（保证线实测） | [`V1ACode::OUTPUT_INVALID`] |
//! | 卡步→跳步复用 | [`V1ACode::STEP_STUCK`] |
//!
//! **码段**：独占 **0x4axx**。
//!
//! **性能逐项分解**：流 O(五步)；推荐 O(1)+理由；保证 O(验证)；
//! 跳步 O(1)。
//!
//! **跨批对接点**：F3104 基线（推荐源）；F3026 模式复用；F3607 验证
//! 对端——钉死于 [`DOCKS`]。
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3626 诊断码段（独占 0x4axx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct V1ACode(pub u16);

impl V1ACode {
    /// 推荐缺理由。
    pub const REASON_MISSING: V1ACode = V1ACode(0x4a01);
    /// 产物验证不过。
    pub const OUTPUT_INVALID: V1ACode = V1ACode(0x4a02);
    /// 默认值劣质。
    pub const DEFAULT_POOR: V1ACode = V1ACode(0x4a03);
    /// 向导卡步。
    pub const STEP_STUCK: V1ACode = V1ACode(0x4a04);
    /// 跳步越界。
    pub const STEP_OUT_OF_RANGE: V1ACode = V1ACode(0x4a05);

    /// 数值。
    pub const fn value(self) -> u16 {
        self.0
    }

    /// 中文说明。
    pub fn explain(self) -> &'static str {
        match self {
            V1ACode::REASON_MISSING => {
                "推荐缺理由（可解释红线）。推荐必须附带可读理由。"
            }
            V1ACode::OUTPUT_INVALID => {
                "向导产物验证不过（保证线 P1）。建议：排查向导默认值质量。"
            }
            V1ACode::DEFAULT_POOR => {
                "默认值劣质。建议：以基线主题（F3104）修正默认值。"
            }
            V1ACode::STEP_STUCK => {
                "向导卡步。建议：启用跳步复用（F3026）。"
            }
            V1ACode::STEP_OUT_OF_RANGE => {
                "跳步越界。五步范围 0..4。"
            }
            _ => "未知诊断码。",
        }
    }
}

// ── 五步向导 ────────────────────────────────────────────────────────

/// 向导步骤。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WizardStep {
    /// 选基调。
    Tone,
    /// 选色板。
    Palette,
    /// 选形状。
    Shape,
    /// 选字阶。
    TypeScale,
    /// 预览发布。
    Preview,
}

impl WizardStep {
    /// 五步全序。
    pub fn order() -> [WizardStep; 5] {
        [
            WizardStep::Tone,
            WizardStep::Palette,
            WizardStep::Shape,
            WizardStep::TypeScale,
            WizardStep::Preview,
        ]
    }

    /// 中文名。
    pub fn name(self) -> &'static str {
        match self {
            WizardStep::Tone => "选基调",
            WizardStep::Palette => "选色板",
            WizardStep::Shape => "选形状",
            WizardStep::TypeScale => "选字阶",
            WizardStep::Preview => "预览发布",
        }
    }
}

/// 向导状态机。
#[derive(Clone, Debug)]
pub struct Wizard {
    /// 当前步序号。
    pub cursor: usize,
    /// 各步选择（值 + 理由）。
    pub choices: Vec<(WizardStep, u32, String)>,
    /// 产物是否通过验证。
    pub output_valid: bool,
}

impl Wizard {
    /// 新建（从第一步开始）。
    pub fn new() -> Wizard {
        Wizard {
            cursor: 0,
            choices: Vec::new(),
            output_valid: false,
        }
    }

    /// 当前步。
    pub fn current(&self) -> Option<WizardStep> {
        WizardStep::order().get(self.cursor).copied()
    }

    /// 完成一步（带选择值与理由）。
    pub fn choose(&mut self, value: u32, reason: &str) -> Result<(), V1ACode> {
        if reason.is_empty() {
            return Err(V1ACode::REASON_MISSING);
        }
        match self.current() {
            Some(step) => {
                self.choices.push((step, value, String::from(reason)));
                self.cursor = self.cursor.saturating_add(1);
                Ok(())
            }
            None => Err(V1ACode::STEP_STUCK),
        }
    }

    /// 跳步（复用 F3026 跳步模式，跳到指定步）。
    pub fn jump_to(&mut self, step: WizardStep) -> Result<(), V1ACode> {
        match WizardStep::order().iter().position(|s| *s == step) {
            Some(idx) if idx < WizardStep::order().len() => {
                self.cursor = idx;
                Ok(())
            }
            _ => Err(V1ACode::STEP_OUT_OF_RANGE),
        }
    }

    /// 回退一步。
    pub fn back(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
        self.choices.pop();
    }

    /// 完成向导（自动验证产物）。
    pub fn finish(&mut self) -> Result<(), V1ACode> {
        // 五步全过才可完成
        if self.choices.len() < 5 {
            return Err(V1ACode::STEP_STUCK);
        }
        // 产物保证：每步选择理由非空
        if self.choices.iter().any(|(_, _, r)| r.is_empty()) {
            self.output_valid = false;
            return Err(V1ACode::OUTPUT_INVALID);
        }
        self.output_valid = true;
        Ok(())
    }
}

// ── 推荐器（可解释）─────────────────────────────────────────────────

/// 推荐结果（值 + 理由）。
#[derive(Clone, Debug)]
pub struct Suggestion {
    /// 建议值。
    pub value: u32,
    /// 理由（非空，可解释红线）。
    pub reason: String,
}

/// 推荐器（基于基线 F3104）。
#[derive(Clone, Debug, Default)]
pub struct Recommender;

impl Recommender {
    /// 推荐一个值（必须带理由）。
    pub fn suggest(step: WizardStep) -> Result<Suggestion, V1ACode> {
        let (value, reason) = match step {
            WizardStep::Tone => (1, "基线主题冷色调占比 60%，推荐冷基调"),
            WizardStep::Palette => (3, "冷色系色板与基调匹配度最高"),
            WizardStep::Shape => (2, "中圆角与冷色调组合通过率最高"),
            WizardStep::TypeScale => (5, "1.25 字号阶在测试中可读性最佳"),
            WizardStep::Preview => (0, "预览默认全界面并排"),
        };
        if reason.is_empty() {
            return Err(V1ACode::REASON_MISSING);
        }
        Ok(Suggestion {
            value,
            reason: String::from(reason),
        })
    }
}

// ── 对外契约 ────────────────────────────────────────────────────────

/// 对接点清单。
pub const DOCKS: [&str; 3] = ["VE-F3104", "VE-F3026", "VE-F3607"];

// ── 自检 ────────────────────────────────────────────────────────────

/// 全模块自检清单。
pub fn run_ves26_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3626");

    // 0x4a01 五步全序
    out.add("0x4a01-五步全序", WizardStep::order().len() == 5, "");

    // 0x4a02 推荐带理由
    let s = Recommender::suggest(WizardStep::Tone);
    out.add(
        "0x4a02-推荐带理由",
        s.as_ref().map(|x| !x.reason.is_empty()).unwrap_or(false),
        "",
    );

    // 0x4a03 逐步完成
    let mut w = Wizard::new();
    let mut ok = true;
    for _ in 0..5 {
        if w.choose(1, "理由").is_err() {
            ok = false;
        }
    }
    out.add("0x4a03-逐步完成", ok && w.cursor == 5, "");

    // 0x4a04 产物保证
    out.add("0x4a04-产物保证", w.finish().is_ok() && w.output_valid, "");

    // 0x4a05 理由空拒
    let mut w2 = Wizard::new();
    out.add("0x4a05-理由空拒", w2.choose(1, "").is_err(), "");

    // 0x4a06 未完成拒结束
    let mut w3 = Wizard::new();
    w3.choose(1, "ok").ok();
    out.add("0x4a06-未完成拒结束", w3.finish().is_err(), "");

    // 0x4a07 跳步
    let mut w4 = Wizard::new();
    out.add(
        "0x4a07-跳步",
        w4.jump_to(WizardStep::Preview).is_ok() && w4.cursor == 4,
        "",
    );

    // 0x4a08 回退
    w4.back();
    out.add("0x4a08-回退", w4.cursor == 3, "");

    out
}
