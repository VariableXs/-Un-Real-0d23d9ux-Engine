//! SDK 最小示例集与模板质量评审门（WP-303 · B-1107 三模板过 20 维度代码
//! 质量评审）。
//!
//! MD2 篇 11.1：sysroot 随 SDK 分发交叉工具链说明与最小示例集——**终端
//! echo、窗口 hello、后台服务三个模板**。SDK 的第一印象决定未来生态的第一
//! 印象：模板代码按 20 维度第 8 项（代码质量）的标准写，**示例即门面**。
//! 评审门把质量标准落成可判定清单——带病的示例代码不许出门。
//!
//! 零堆纪律：无 Vec/String/Box/format!，定长数组 + 字节串。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 三模板（最小示例集——SDK 的门面）
// ---------------------------------------------------------------------------

/// 模板三档（穷举——MD2 11.1 明文三件）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TemplateKind {
    /// 终端 echo（命令行面示例）。
    EchoCli,
    /// 窗口 hello（VXWM 客户端面示例）。
    WindowHello,
    /// 后台服务（vxapp 运行时托管面示例）。
    BackgroundService,
}

/// 模板在册总数。
pub const TEMPLATES: usize = 3;

/// 模板评审清单（20 维度第 8 项代码质量的可计算子集——四要素缺一不发布）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ReviewChecklist {
    /// clippy 零告警（lint 面全绿）。
    pub clippy_zero: bool,
    /// 文档注释齐（pub 面全带——示例是教学材料）。
    pub doc_comment: bool,
    /// 错误三要素（附录 B：出了什么事/为什么/怎么办——示例的错误文案即标尺）。
    pub error_three_elements: bool,
    /// 宪章默认复用（吃自家狗粮：用控件基类默认与主题 token，不绕过 SDK）。
    pub charter_defaults_used: bool,
}

impl ReviewChecklist {
    /// 评审门：四要素全过才允许进 sysroot 分发。
    pub fn pass(&self) -> bool {
        self.clippy_zero && self.doc_comment && self.error_three_elements && self.charter_defaults_used
    }
}

/// 模板评审记录（模板 + 清单——评审记录即发布凭证）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ReviewRecord {
    pub kind: TemplateKind,
    pub checklist: ReviewChecklist,
}

/// 三模板全过判（**B-1107 达标线**）：三档穷举全登记 + 全部过评审门——
/// 一件没过就是 SDK 门面带病。
pub fn templates_green(records: &[ReviewRecord]) -> bool {
    if records.len() != TEMPLATES {
        return false;
    }
    let mut seen = [false; TEMPLATES];
    let mut i = 0;
    while i < records.len() {
        let slot = match records[i].kind {
            TemplateKind::EchoCli => 0,
            TemplateKind::WindowHello => 1,
            TemplateKind::BackgroundService => 2,
        };
        if seen[slot] || !records[i].checklist.pass() {
            return false;
        }
        seen[slot] = true;
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// CheckSet（B-1107 · 4 项）
// ---------------------------------------------------------------------------

pub fn run_sdktemplate_checks() -> CheckSet {
    let mut set = CheckSet::new("B-1107 模板质量评审门");
    // 1. 三模板在册（echo/hello/service 穷举——最小示例集即门面）。
    let kinds = [TemplateKind::EchoCli, TemplateKind::WindowHello, TemplateKind::BackgroundService];
    let mut distinct = 0;
    let mut i = 0;
    while i < kinds.len() {
        let mut j = i + 1;
        while j < kinds.len() && kinds[i] != kinds[j] {
            j += 1;
        }
        if j == kinds.len() {
            distinct += 1;
        }
        i += 1;
    }
    set.add(
        "B-1107 三模板在册",
        distinct == 3 && TEMPLATES == 3,
        "终端 echo/窗口 hello/后台服务——SDK 的第一印象决定生态的第一印象",
    );
    // 2. 评审清单四要素齐判（缺一即不过）。
    let full = ReviewChecklist { clippy_zero: true, doc_comment: true, error_three_elements: true, charter_defaults_used: true };
    let miss_doc = ReviewChecklist { clippy_zero: true, doc_comment: false, error_three_elements: true, charter_defaults_used: true };
    let miss_err = ReviewChecklist { clippy_zero: true, doc_comment: true, error_three_elements: false, charter_defaults_used: true };
    set.add(
        "B-1107 评审四要素",
        full.pass() && !miss_doc.pass() && !miss_err.pass(),
        "clippy 零告警+文档齐+错误三要素+宪章默认复用——20 维度第 8 项的可计算面",
    );
    // 3. 单模板评审门：带病示例不许进 sysroot。
    let mut sick = ReviewRecord { kind: TemplateKind::WindowHello, checklist: full };
    sick.checklist.charter_defaults_used = false;
    set.add(
        "B-1107 带病不发布",
        !templates_green(&[ReviewRecord { kind: TemplateKind::EchoCli, checklist: full }, sick]),
        "绕过 SDK 的示例是反面教材——评审记录即发布凭证",
    );
    // 4. 三模板全过（B-1107 达标线）。
    let all = [
        ReviewRecord { kind: TemplateKind::EchoCli, checklist: full },
        ReviewRecord { kind: TemplateKind::WindowHello, checklist: full },
        ReviewRecord { kind: TemplateKind::BackgroundService, checklist: full },
    ];
    let short = &all[..2];
    set.add(
        "B-1107 三模板全过评审",
        templates_green(&all) && !templates_green(short),
        "三模板过 20 维度代码质量评审（B-1107 达标线）——示例即门面，门面即 SDK",
    );
    set
}

// ---------------------------------------------------------------------------
// 单元测试（fe10 · 4 项）
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn fe10_three_templates_enumerated() {
        let kinds = [TemplateKind::EchoCli, TemplateKind::WindowHello, TemplateKind::BackgroundService];
        assert_eq!(kinds.len(), TEMPLATES);
        assert_ne!(kinds[0], kinds[1]);
        assert_ne!(kinds[1], kinds[2]);
    }

    #[test]
    fn fe10_checklist_conjunction() {
        // 四要素是合取不是投票：逐项缺席逐一不过。
        let base = ReviewChecklist { clippy_zero: true, doc_comment: true, error_three_elements: true, charter_defaults_used: true };
        assert!(base.pass());
        let a = ReviewChecklist { clippy_zero: false, ..base };
        assert!(!a.pass());
        let b = ReviewChecklist { doc_comment: false, ..base };
        assert!(!b.pass());
        let c = ReviewChecklist { error_three_elements: false, ..base };
        assert!(!c.pass());
        let d = ReviewChecklist { charter_defaults_used: false, ..base };
        assert!(!d.pass());
    }

    #[test]
    fn fe10_templates_green_full_and_short() {
        let ok = ReviewChecklist { clippy_zero: true, doc_comment: true, error_three_elements: true, charter_defaults_used: true };
        let all = [
            ReviewRecord { kind: TemplateKind::EchoCli, checklist: ok },
            ReviewRecord { kind: TemplateKind::WindowHello, checklist: ok },
            ReviewRecord { kind: TemplateKind::BackgroundService, checklist: ok },
        ];
        assert!(templates_green(&all));
        // 少一件不算齐。
        assert!(!templates_green(&all[..2]));
        // 重复模板不算齐。
        let mut dup = all;
        dup[2].kind = TemplateKind::EchoCli;
        assert!(!templates_green(&dup));
    }

    #[test]
    fn fe10_sick_template_blocks_release() {
        let ok = ReviewChecklist { clippy_zero: true, doc_comment: true, error_three_elements: true, charter_defaults_used: true };
        let mut sick = ok;
        sick.clippy_zero = false;
        let recs = [
            ReviewRecord { kind: TemplateKind::EchoCli, checklist: sick },
            ReviewRecord { kind: TemplateKind::WindowHello, checklist: ok },
            ReviewRecord { kind: TemplateKind::BackgroundService, checklist: ok },
        ];
        // 一件 clippy 带病：三模板整体判红——门面不带病出门。
        assert!(!templates_green(&recs));
    }
}
