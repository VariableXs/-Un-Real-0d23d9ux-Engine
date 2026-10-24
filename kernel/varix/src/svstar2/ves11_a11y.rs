//! VE-F3611 · 创作无障碍（VE-S 域 · 批次 S01 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3611`
//!
//! **判据（锚点原文六条）**：工具即产品、辅助不阻塞、自举复述、
//! WCAG 执法、双维声明、判据。逐条落位：
//! - **工具即产品**：编辑器无障碍清单（键盘全通/读屏/对比度）按
//!   产品级标准执法——工具不可达 → 阻断（[`V0BCode::TOOL_UNREACHABLE`]，
//!   复述红线实测）：创作工具本身就是产品，无障碍不是附加项。
//! - **辅助不阻塞**：对比度实时提示/语义标注助手是**提示不是强制**
//!   （辅助不阻塞红线）——[`AssistHint`] 通道只出建议，**绝不拦截**
//!   创作操作；提示挡操作 → [`V0BCode::HINT_BLOCKS`] 修正（红线实测）。
//! - **自举复述**：教程/文档无障碍（复述自举）——[`SelfBoot::audit`] 
//!   审计教程无障碍基线。
//! - **WCAG 执法**：创作工具 WCAG 矩阵（复述制度）——[`WcagMatrix`] 
//!   逐条款核验，矩阵虚登 → [`V0BCode::MATRIX_FALSE_CLAIM`]（伪证）。
//! - **双维声明**：无障碍与 DX 双维同时达标（[`DualDim::audit`]）。
//! - **判据**：`run_ves11_checks` 逐条映射。
//!
//! **错误路径与降级矩阵（锚点原文四条 → 落位）**：
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 工具不可达→阻断（复述红线实测） | [`V0BCode::TOOL_UNREACHABLE`]：键盘不可达即阻断发布 |
//! | 辅助阻塞创作→修正（红线实测） | [`V0BCode::HINT_BLOCKS`]：提示通道必须非阻塞 |
//! | 自举不过→修（复述） | [`V0BCode::SELFBOOT_FAIL`]：教程无障碍审计不过 |
//! | 矩阵虚登→伪证（复述） | [`V0BCode::MATRIX_FALSE_CLAIM`]：声称过而未核验 |
//!
//! **码段**：独占 **0x3bxx**。
//!
//! **性能逐项分解（锚点口径）**：清单 O(核验)；提示 O(1) 非阻塞；
//! 矩阵 O(条款)；自举 O(审查)。
//!
//! **跨批对接点**：F3146/F3153 模式复用；F3622 编辑器对端；创作社区
//! （无障碍创作者对端）——钉死于 [`DOCKS`]。
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::string::String;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3611 诊断码段（独占 0x3bxx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct V0BCode(pub u16);

impl V0BCode {
    /// 工具不可达（键盘/读屏）。
    pub const TOOL_UNREACHABLE: V0BCode = V0BCode(0x3b01);
    /// 辅助提示阻塞创作。
    pub const HINT_BLOCKS: V0BCode = V0BCode(0x3b02);
    /// 自举（教程无障碍）不过。
    pub const SELFBOOT_FAIL: V0BCode = V0BCode(0x3b03);
    /// WCAG 矩阵虚登。
    pub const MATRIX_FALSE_CLAIM: V0BCode = V0BCode(0x3b04);
    /// 对比度低于门槛。
    pub const CONTRAST_LOW: V0BCode = V0BCode(0x3b05);
    /// 语义标注缺失。
    pub const SEMANTIC_MISSING: V0BCode = V0BCode(0x3b06);
    /// 双维缺一。
    pub const DUAL_DIM_MISS: V0BCode = V0BCode(0x3b07);

    /// 数值。
    pub const fn value(self) -> u16 {
        self.0
    }

    /// 中文说明。
    pub fn explain(self) -> &'static str {
        match self {
            V0BCode::TOOL_UNREACHABLE => {
                "创作工具对无障碍用户不可达（键盘/读屏）。建议：补齐键盘通路，工具即产品红线不容缺格。"
            }
            V0BCode::HINT_BLOCKS => {
                "无障碍辅助提示阻塞了创作操作。提示必须非阻塞——建议：改为旁路气泡或延后展示。"
            }
            V0BCode::SELFBOOT_FAIL => {
                "教程/文档无障碍审计不过。建议：按 WCAG 基线修订教程后重审。"
            }
            V0BCode::MATRIX_FALSE_CLAIM => {
                "WCAG 条款声称通过但无核验记录（伪证）。建议：每条声称都必须有核验证据。"
            }
            V0BCode::CONTRAST_LOW => {
                "对比度低于 4.5:1 门槛。建议：调整前景/背景色值。"
            }
            V0BCode::SEMANTIC_MISSING => {
                "语义标注缺失。建议：为控件补 ARIA/等价语义标签。"
            }
            V0BCode::DUAL_DIM_MISS => {
                "无障碍与 DX 双维未同时达标。建议：补齐短板维度后重审。"
            }
            _ => "未知诊断码。",
        }
    }
}

// ── 编辑器无障碍清单 ────────────────────────────────────────────────

/// 清单项。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChecklistItem {
    /// 键盘全通。
    KeyboardFull,
    /// 读屏支持。
    ScreenReader,
    /// 对比度达标。
    Contrast,
    /// 焦点可见。
    FocusVisible,
    /// 色值数值通道。
    ColorNumeric,
}

impl ChecklistItem {
    /// 全集。
    pub fn all() -> [ChecklistItem; 5] {
        [
            ChecklistItem::KeyboardFull,
            ChecklistItem::ScreenReader,
            ChecklistItem::Contrast,
            ChecklistItem::FocusVisible,
            ChecklistItem::ColorNumeric,
        ]
    }

    /// 中文名。
    pub fn name(self) -> &'static str {
        match self {
            ChecklistItem::KeyboardFull => "键盘全通",
            ChecklistItem::ScreenReader => "读屏支持",
            ChecklistItem::Contrast => "对比度",
            ChecklistItem::FocusVisible => "焦点可见",
            ChecklistItem::ColorNumeric => "色值数值通道",
        }
    }
}

/// 编辑器无障碍清单。
#[derive(Clone, Debug)]
pub struct EditorChecklist {
    pass: Vec<ChecklistItem>,
    fail: Vec<(ChecklistItem, V0BCode)>,
}

impl EditorChecklist {
    /// 空清单。
    pub fn new() -> EditorChecklist {
        EditorChecklist {
            pass: Vec::new(),
            fail: Vec::new(),
        }
    }

    /// 核验一项。
    pub fn verify(&mut self, item: ChecklistItem, ok: bool) {
        if ok {
            self.pass.push(item);
        } else {
            let code = match item {
                ChecklistItem::KeyboardFull | ChecklistItem::ScreenReader => {
                    V0BCode::TOOL_UNREACHABLE
                }
                ChecklistItem::Contrast => V0BCode::CONTRAST_LOW,
                ChecklistItem::FocusVisible => V0BCode::SEMANTIC_MISSING,
                ChecklistItem::ColorNumeric => V0BCode::TOOL_UNREACHABLE,
            };
            self.fail.push((item, code));
        }
    }

    /// 是否全部通过。
    pub fn all_pass(&self) -> bool {
        self.fail.is_empty() && self.pass.len() >= ChecklistItem::all().len()
    }

    /// 失败清单。
    pub fn failures(&self) -> &[(ChecklistItem, V0BCode)] {
        &self.fail
    }

    /// 通过数。
    pub fn pass_count(&self) -> usize {
        self.pass.len()
    }
}

// ── 非阻塞辅助提示 ──────────────────────────────────────────────────

/// 提示通道（只出建议，绝不拦截）。
#[derive(Clone, Debug, Default)]
pub struct AssistHint {
    /// 是否误装了阻塞钩子（审计用，正常应恒 false）。
    blocking_hook_installed: bool,
    /// 已发提示数。
    issued: u32,
}

impl AssistHint {
    /// 新建。
    pub fn new() -> AssistHint {
        AssistHint {
            blocking_hook_installed: false,
            issued: 0,
        }
    }

    /// 发一条提示（非阻塞，返回 true 表示已送达）。
    pub fn issue(&mut self, _text: &str) -> bool {
        self.issued = self.issued.wrapping_add(1);
        // 提示通道设计为永不拦截：返回 true 即送达，调用方流程不受影响
        !self.blocking_hook_installed
    }

    /// 审计：通道是否被误装阻塞钩子。
    pub fn audit_non_blocking(&self) -> Result<(), V0BCode> {
        if self.blocking_hook_installed {
            Err(V0BCode::HINT_BLOCKS)
        } else {
            Ok(())
        }
    }

    /// 诊断用：模拟误装（仅供判据反向语料，正常代码永不调用）。
    pub fn install_blocking_hook_for_test(&mut self) {
        self.blocking_hook_installed = true;
    }

    /// 已发数。
    pub fn issued_count(&self) -> u32 {
        self.issued
    }
}

// ── WCAG 矩阵 ───────────────────────────────────────────────────────

/// WCAG 条款。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WcagClause(pub u16);

impl WcagClause {
    /// 1.4.3 对比度（最低）。
    pub const CONTRAST_MIN: WcagClause = WcagClause(143);
    /// 2.1.1 键盘。
    pub const KEYBOARD: WcagClause = WcagClause(211);
    /// 4.1.2 名称/角色/值。
    pub const NAME_ROLE_VALUE: WcagClause = WcagClause(412);
}

/// 矩阵一行。
#[derive(Clone, Debug)]
pub struct ClauseRecord {
    /// 条款。
    pub clause: WcagClause,
    /// 声称通过。
    pub claimed: bool,
    /// 核验证据（虚登检测：claimed 而 evidence 空 = 伪证）。
    pub evidence: String,
}

/// WCAG 矩阵。
#[derive(Clone, Debug, Default)]
pub struct WcagMatrix {
    rows: Vec<ClauseRecord>,
}

impl WcagMatrix {
    /// 空矩阵。
    pub fn new() -> WcagMatrix {
        WcagMatrix { rows: Vec::new() }
    }

    /// 登记一条。
    pub fn record(&mut self, clause: WcagClause, claimed: bool, evidence: &str) {
        self.rows.push(ClauseRecord {
            clause,
            claimed,
            evidence: String::from(evidence),
        });
    }

    /// 虚登审计：声称通过而无证据 → 伪证。
    pub fn audit_false_claims(&self) -> Vec<WcagClause> {
        self.rows
            .iter()
            .filter(|r| r.claimed && r.evidence.is_empty())
            .map(|r| r.clause)
            .collect()
    }

    /// 是否全部声称都有据。
    pub fn honest(&self) -> bool {
        self.audit_false_claims().is_empty()
    }

    /// 条款数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

// ── 双维声明（无障碍 × DX）───────────────────────────────────────────

/// 双维。
#[derive(Clone, Copy, Debug)]
pub struct DualDim {
    /// 无障碍达标。
    pub a11y_ok: bool,
    /// DX 达标。
    pub dx_ok: bool,
}

impl DualDim {
    /// 双维审计：必须同时达标。
    pub fn audit(&self) -> Result<(), V0BCode> {
        if self.a11y_ok && self.dx_ok {
            Ok(())
        } else {
            Err(V0BCode::DUAL_DIM_MISS)
        }
    }
}

// ── 自举审计 ────────────────────────────────────────────────────────

/// 教程/文档无障碍自举。
#[derive(Clone, Debug, Default)]
pub struct SelfBoot {
    /// 教程无障碍达标。
    pub tutorial_ok: bool,
    /// 文档无障碍达标。
    pub doc_ok: bool,
}

impl SelfBoot {
    /// 自举审计。
    pub fn audit(&self) -> Result<(), V0BCode> {
        if self.tutorial_ok && self.doc_ok {
            Ok(())
        } else {
            Err(V0BCode::SELFBOOT_FAIL)
        }
    }
}

// ── 对外契约 ────────────────────────────────────────────────────────

/// 对接点清单。
pub const DOCKS: [&str; 3] = ["VE-F3146", "VE-F3153", "VE-F3622"];

/// 对比度门（4.5:1，独立写死）。
pub const CONTRAST_THRESHOLD_X10: u32 = 45;

/// 对比度检查（放大 10 倍的整数比）。
pub fn check_contrast(ratio_x10: u32) -> Result<(), V0BCode> {
    if ratio_x10 >= CONTRAST_THRESHOLD_X10 {
        Ok(())
    } else {
        Err(V0BCode::CONTRAST_LOW)
    }
}

// ── 自检 ────────────────────────────────────────────────────────────

/// 全模块自检清单。
pub fn run_ves11_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3611");

    // 0x3b01 全过清单
    let mut cl = EditorChecklist::new();
    for item in ChecklistItem::all() {
        cl.verify(item, true);
    }
    out.add("0x3b01-无障碍清单全过", cl.all_pass(), "");

    // 0x3b02 键盘缺失即失败
    let mut cl2 = EditorChecklist::new();
    cl2.verify(ChecklistItem::KeyboardFull, false);
    out.add(
        "0x3b02-键盘缺失被拦",
        !cl2.all_pass() && cl2.failures().len() == 1,
        "",
    );

    // 0x3b03 辅助非阻塞
    let hint = AssistHint::new();
    out.add("0x3b03-提示通道非阻塞", hint.audit_non_blocking().is_ok(), "");

    // 0x3b04 阻塞钩子反向语料
    let mut hint2 = AssistHint::new();
    hint2.install_blocking_hook_for_test();
    out.add(
        "0x3b04-误装阻塞钩子被审计检出",
        hint2.audit_non_blocking().is_err(),
        "",
    );

    // 0x3b05 对比度门
    out.add("0x3b05-对比度 4.5:1 卡两侧", check_contrast(45).is_ok() && check_contrast(44).is_err(), "");

    // 0x3b06 WCAG 诚实
    let mut m = WcagMatrix::new();
    m.record(WcagClause::CONTRAST_MIN, true, "实测 5.2:1");
    m.record(WcagClause::KEYBOARD, true, "键盘走查记录 #12");
    out.add("0x3b06-声称均有据", m.honest(), "");

    // 0x3b07 虚登检出
    let mut m2 = WcagMatrix::new();
    m2.record(WcagClause::NAME_ROLE_VALUE, true, "");
    out.add("0x3b07-虚登伪证被检出", !m2.honest(), "");

    // 0x3b08 双维
    out.add("0x3b08-双维同时达标", DualDim { a11y_ok: true, dx_ok: true }.audit().is_ok(), "");

    // 0x3b09 双维缺一
    out.add("0x3b09-双维缺一报错", DualDim { a11y_ok: true, dx_ok: false }.audit().is_err(), "");

    // 0x3b0a 自举
    out.add("0x3b0a-自举过", SelfBoot { tutorial_ok: true, doc_ok: true }.audit().is_ok(), "");

    out
}
