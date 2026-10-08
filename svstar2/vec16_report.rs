//! VE-F0416 · 词法错误报告（VE-C 域 · 着色器系统 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0416`
//!
//! **判据（锚点原文）**：四族分类、三要素、双侧定位、三级分级、判据。
//!
//! 锚点职责定位原文：
//! > 词法错误报告体系：错误分类（字符类、字面量类、括号类、指令类四族）、三要素
//! > 呈现（发生了什么带位置、为什么带规则引用、下一步带修正建议）、双侧定位支持
//! > （宏展开场景）；报告按严重度分级（错误、警告、注记）；位置信息（行列加字节
//! > 偏移三元式）。
//!
//! 本条只做「把各上游错误源的一条原始错误，变成一行作者能照着改的诊断」这一个
//! 动作，并把四件容易做错的事显式钉死：
//!
//! 1. **四族分类**（判据一）。字符类、字面量类、括号类、指令类**各有各的修法**，
//!    混成一族就没法给建议。分类不能靠**猜**（看错误码字符串 `contains("paren")`
//!    之类），必须**查表**（`ErrorFamily` 表 + 码前缀族标记）——`str::contains`
//!    猜族是本条最容易埋雷的地方：`VE-F0415-UTF8-TRUNCATED` 与
//!    `VE-F0415-UTF16-ODD` 都含 "ODD"/"TRUNCATED" 式的词，猜错族给出的建议就
//!    指错方向。`classify()` 只认**权威族标记**：上游错误码尾段的族标识 +
//!    登记表的显式条目二者取一，猜不中就落 `Other` 族并**如实说「未归族」**，
//!    不硬塞进某一族。
//!
//! 2. **三要素齐备**（判据二）。「发生了什么（带位置）/ 为什么（带规则引用）/
//!    下一步（修正建议）」。锚点错误路径第一条：**模板缺失→最小告知不静默**。
//!    所以三要素任一为空都不许静默出报告：走 `TemplateMissing` 路径，产出
//!    「已知错误码 X，但本域无该码的规则模板」的最小告知 + 待回填登记，
//!    绝不输出半空的三要素假装完整。
//!
//! 3. **双侧定位**（判据三）。宏展开场景下错误同时属于「调用处」与「定义处」，
//!    只报一侧作者无法修：只报调用处他不知道改哪张表，只报定义处他不知道哪个
//!    调用触发的。`Loc` 支持双侧（`primary` + `secondary`），`dual_side()`
//!    判定是否双侧，报告文本双侧都渲染。**两侧都不可省**——本条不提供
//!    「只报一侧」的省事选项，因为省掉的那一侧正是修不了的原因。
//!
//! 4. **三级分级**（判据四）。错误 / 警告 / 注记。锚点错误路径第三条：**严重度
//!    误判→以规范裁定为准**。故严重度**不由调用方随手指定**，而由「码 + 族」
//!    经 `SeverityPolicy` 表裁定；表里查不到 → 按**保守最严**（`Error`）处置并
//!    出注记说明「未登记，按最严裁定」——把选择权交给表，不交给笔误。
//!
//! 零静默纪律：模板缺失→最小告知 + 待回填登记（不静默）；未归族→如实标注
//!   「未归族」；严重度未登记→按最严并注记；位置计算越界→不 panic，钳到合法域
//!   并出注记；三要素任一为空→不输出完整报告。零 panic 面、零 IO、无全局可变状态。
//!
//! 性能逐项分解：报告 O(1)/条（模板查表 + 三段文本拼接）；分类 O(1) 查表；位置
//!   O(1)（三元组随错误同行携带，不重扫源码——**位置计算错误由测试断言拦截**，
//!   见 `position_is_consistent` 自检：行/列/字节偏移必须互相自洽）。
//!
//! 上游消费：F0412 `MacroError`（含 `def_pos`/`def_line` 双侧）、F0413 `CondError`、
//!   F0414 `IncludeError`、F0415 `EncodingFault`（本条把四者统一成一条诊断）。
//!   下游 F0417 恢复策略消费 `Diagnostic` 的 `family`（选恢复策略）与
//!   `severity`（决定是否阻断）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、位置三元式（行列 + 字节偏移）
// ---------------------------------------------------------------------------

/// 位置三元式：行、列、字节偏移（判据四的核心数据结构）。
///
/// 三个分量必须**互相自洽**：`byte_offset` 是权威值，行列是由它派生的展示值。
/// 上游若给出越界行列（例如把 UTF-16 的列号当 UTF-8 用），本条不 panic 也不
/// 照单全收——`normalized()` 把行列钳到与字节偏移相容的合法域并**如实标注**
/// `clamped`，让「位置算错了」这件事本身可见，而不是被静默美化。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Loc {
    /// 行号（1 起）。
    pub line: usize,
    /// 列号（1 起）。
    pub col: usize,
    /// 字节偏移（0 起，权威值）。
    pub byte_offset: usize,
    /// 行列是否被钳制过（上游位置不自洽时为真）。
    pub clamped: bool,
}

impl Loc {
    /// 构造一个自洽位置（行列均 ≥1）。
    pub const fn new(line: usize, col: usize, byte_offset: usize) -> Loc {
        Loc { line, col, byte_offset, clamped: false }
    }

    /// 仅由字节偏移构造（行列未知时置 1，由 `with_line_col` 补齐）。
    pub const fn from_offset(byte_offset: usize) -> Loc {
        Loc { line: 1, col: 1, byte_offset, clamped: false }
    }

    /// 补齐行列（`clamped` 标志清零——调用者自称自洽时才清）。
    pub const fn with_line_col(mut self, line: usize, col: usize) -> Loc {
        self.line = line;
        self.col = col;
        self
    }

    /// 规范化：行列至少为 1（0 行 0 列在诊断里没有意义）。
    ///
    /// 返回 `(规范化位置, 是否被钳制)`。**不 panic**——上游给 0 或给出与字节
    /// 偏移根本不相容的行列时，钳到最小合法值并让调用方出注记告知。
    pub fn normalized(self) -> (Loc, bool) {
        let line = if self.line == 0 { 1 } else { self.line };
        let col = if self.col == 0 { 1 } else { self.col };
        // 行列不得超过字节偏移所能支撑的上界：第 N 行第 M 列至少要 M-1 个字节。
        let col_cap = self.byte_offset + 1;
        let (col, mut clamped) = if col > col_cap { (col_cap, true) } else { (col, false) };
        if line != self.line || col != self.col {
            clamped = true;
        }
        (Loc { line, col, byte_offset: self.byte_offset, clamped }, clamped)
    }

    /// 位置自洽性核验（锚点「位置计算错误→测试断言拦截」的运行时侧）。
    ///
    /// 判据：行 ≥ 1、列 ≥ 1、列不超过 `byte_offset + 1`（第 M 列之前至少要有
    /// M-1 个字节）。
    pub fn is_consistent(self) -> bool {
        self.line >= 1 && self.col >= 1 && self.col <= self.byte_offset + 1
    }

    /// 人话呈现（`行:列` 形态，附字节偏移）。
    pub fn render(&self) -> String {
        let base = format!("{}:{}（字节偏移 {}）", self.line, self.col, self.byte_offset);
        if self.clamped {
            format!("{base}，位置已钳制")
        } else {
            base
        }
    }
}

/// 双侧定位：主侧（出错处）+ 副侧（定义处/来源处，判据三）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span2 {
    /// 主侧：作者直接要改的位置。
    pub primary: Loc,
    /// 副侧：宏定义处 / include 链来源处等（无副侧则 `None`）。
    pub secondary: Option<Loc>,
}

impl Span2 {
    /// 仅主侧。
    pub const fn primary_only(primary: Loc) -> Span2 {
        Span2 { primary, secondary: None }
    }

    /// 双侧（宏展开场景：调用处 + 定义处）。
    pub const fn dual(primary: Loc, secondary: Loc) -> Span2 {
        Span2 { primary, secondary: Some(secondary) }
    }

    /// 是否双侧。
    pub const fn is_dual(&self) -> bool {
        self.secondary.is_some()
    }

    /// 取副侧（`Option`）。
    pub const fn secondary(&self) -> Option<Loc> {
        self.secondary
    }

    /// 规范化两侧（各自独立钳制）。
    pub fn normalized(&self) -> Span2 {
        let (p, _) = self.primary.normalized();
        let s = self.secondary.map(|l| l.normalized().0);
        Span2 { primary: p, secondary: s }
    }

    /// 人话呈现：双侧两侧都渲染（判据三：省掉的那一侧正是修不了的原因）。
    pub fn render(&self) -> String {
        match self.secondary {
            Some(s) => format!("{} ← 定义于 {}", self.primary.render(), s.render()),
            None => self.primary.render(),
        }
    }
}

// ---------------------------------------------------------------------------
// 二、错误族与严重度（判据一、判据四）
// ---------------------------------------------------------------------------

/// 错误族（判据一：字符类、字面量类、括号类、指令类四族）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorFamily {
    /// 字符类：非法字符、续字节、不可识别符号。
    Char,
    /// 字面量类：数值/字符串/转义/编码。
    Literal,
    /// 括号类：配对失配、作用域标记。
    Bracket,
    /// 指令类：预处理指令、宏、条件编译、include。
    Directive,
    /// 未归族（如实标注，不硬塞进上面四族）。
    Other,
}

impl ErrorFamily {
    /// 人话标签（诊断抬头用）。
    pub const fn label(self) -> &'static str {
        match self {
            ErrorFamily::Char => "字符类",
            ErrorFamily::Literal => "字面量类",
            ErrorFamily::Bracket => "括号类",
            ErrorFamily::Directive => "指令类",
            ErrorFamily::Other => "未归族",
        }
    }

    /// 该族的通用修法方向（三要素之「下一步」的族级兜底）。
    pub const fn remedy_hint(self) -> &'static str {
        match self {
            ErrorFamily::Char => "检查该处字符是否属于本语言允许的字符集",
            ErrorFamily::Literal => "检查字面量写法与后缀是否合法",
            ErrorFamily::Bracket => "检查括号是否配对、是否交叉嵌套",
            ErrorFamily::Directive => "检查预处理指令拼写、参数个数与条件编译分支",
            ErrorFamily::Other => "该错误未归入四族，需回填族归属后补族级建议",
        }
    }

    /// 是否为已归族（四族之一）。
    pub const fn is_classified(self) -> bool {
        !matches!(self, ErrorFamily::Other)
    }
}

/// 严重度分级（判据四：错误、警告、注记）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// 注记：不影响编译通过，仅告知（例：按假定处理、缓存跳过）。
    Note,
    /// 警告：可疑但可继续。
    Warning,
    /// 错误：阻断编译。
    Error,
}

impl Severity {
    /// 人话标签。
    pub const fn label(self) -> &'static str {
        match self {
            Severity::Note => "注记",
            Severity::Warning => "警告",
            Severity::Error => "错误",
        }
    }

    /// 是否阻断编译。
    pub const fn is_blocking(self) -> bool {
        matches!(self, Severity::Error)
    }

    /// 全序比较用的序号（`Note < Warning < Error`）。
    pub const fn rank(self) -> u8 {
        match self {
            Severity::Note => 0,
            Severity::Warning => 1,
            Severity::Error => 2,
        }
    }
}

/// 严重度裁定表条目。
#[derive(Clone, Copy, Debug)]
pub struct SeverityRule {
    /// 匹配的码前缀（`starts_with` 判定）。
    pub code_prefix: &'static str,
    /// 裁定严重度。
    pub severity: Severity,
}

/// 严重度裁定表（判据四：严重度由表裁定，不由调用方随手指定）。
///
/// 表里查不到 → **按最严**（`Error`）处置并出注记（锚点错误路径第三条：以规范
/// 裁定为准；无规范条目时保守最严是唯一安全侧）。
pub const SEVERITY_TABLE: &[SeverityRule] = &[
    SeverityRule { code_prefix: "VE-F0415-BOM", severity: Severity::Warning },
    SeverityRule { code_prefix: "VE-F0415-UTF8-CONTINUATION", severity: Severity::Error },
    SeverityRule { code_prefix: "VE-F0415-UTF8", severity: Severity::Error },
    SeverityRule { code_prefix: "VE-F0415-UTF16", severity: Severity::Error },
    SeverityRule { code_prefix: "VE-F0415-UTF32", severity: Severity::Error },
    SeverityRule { code_prefix: "VE-F0413", severity: Severity::Error },
    SeverityRule { code_prefix: "VE-F0414", severity: Severity::Error },
    SeverityRule { code_prefix: "VE-F0412-RECURSION", severity: Severity::Error },
    SeverityRule { code_prefix: "VE-F0412", severity: Severity::Error },
    SeverityRule { code_prefix: "VE-F0411", severity: Severity::Error },
    SeverityRule { code_prefix: "VE-F0410", severity: Severity::Error },
    SeverityRule { code_prefix: "VE-F0409", severity: Severity::Error },
    SeverityRule { code_prefix: "VE-F0408", severity: Severity::Warning },
    SeverityRule { code_prefix: "VE-F0407", severity: Severity::Error },
    SeverityRule { code_prefix: "VE-F0406", severity: Severity::Error },
    SeverityRule { code_prefix: "VE-F0405", severity: Severity::Error },
    SeverityRule { code_prefix: "VE-F0404", severity: Severity::Error },
    SeverityRule { code_prefix: "VE-F0403", severity: Severity::Error },
    SeverityRule { code_prefix: "NOTE", severity: Severity::Note },
    SeverityRule { code_prefix: "WARN", severity: Severity::Warning },
];

/// 按裁定表定严重度。返回 `(严重度, 是否查表命中)`。
pub fn severity_of(code: &str) -> (Severity, bool) {
    for r in SEVERITY_TABLE.iter() {
        if code.starts_with(r.code_prefix) {
            return (r.severity, true);
        }
    }
    // 未登记 → 保守最严（并由调用方出注记说明）。
    (Severity::Error, false)
}

/// 族归属裁定表条目。
#[derive(Clone, Copy, Debug)]
pub struct FamilyRule {
    /// 匹配的码前缀。
    pub code_prefix: &'static str,
    /// 裁定族。
    pub family: ErrorFamily,
}

/// 族归属裁定表（判据一：查表归族，不猜）。
///
/// 顺序敏感：先匹配到的赢，故更长的前缀排在前面。
pub const FAMILY_TABLE: &[FamilyRule] = &[
    // 指令类：预处理与其指令族（F0411 指令词法、F0412 宏、F0413 条件、F0414 include）
    FamilyRule { code_prefix: "VE-F0411", family: ErrorFamily::Directive },
    FamilyRule { code_prefix: "VE-F0412", family: ErrorFamily::Directive },
    FamilyRule { code_prefix: "VE-F0413", family: ErrorFamily::Directive },
    FamilyRule { code_prefix: "VE-F0414", family: ErrorFamily::Directive },
    // 括号类：F0410 括号配对与作用域标记
    FamilyRule { code_prefix: "VE-F0410", family: ErrorFamily::Bracket },
    // 字面量类：F0406 数值、F0407 字符串与转义、F0405 标识符
    FamilyRule { code_prefix: "VE-F0406", family: ErrorFamily::Literal },
    FamilyRule { code_prefix: "VE-F0407", family: ErrorFamily::Literal },
    FamilyRule { code_prefix: "VE-F0405", family: ErrorFamily::Literal },
    // 字符类：F0415 编码字节类、F0408 注释类
    FamilyRule { code_prefix: "VE-F0415-UTF8", family: ErrorFamily::Char },
    FamilyRule { code_prefix: "VE-F0415-UTF16", family: ErrorFamily::Char },
    FamilyRule { code_prefix: "VE-F0415-UTF32", family: ErrorFamily::Char },
    FamilyRule { code_prefix: "VE-F0415", family: ErrorFamily::Char },
    FamilyRule { code_prefix: "VE-F0408", family: ErrorFamily::Char },
    // 兜底：F0403 词法主路（字符类默认）、F0404 关键字、F0409 运算符
    FamilyRule { code_prefix: "VE-F0403", family: ErrorFamily::Char },
    FamilyRule { code_prefix: "VE-F0404", family: ErrorFamily::Char },
    FamilyRule { code_prefix: "VE-F0409", family: ErrorFamily::Char },
];

/// 查表归族（判据一）。查不中 → `Other` 族（**如实说未归族**，不硬塞）。
pub fn classify(code: &str) -> ErrorFamily {
    for r in FAMILY_TABLE.iter() {
        if code.starts_with(r.code_prefix) {
            return r.family;
        }
    }
    ErrorFamily::Other
}

// ---------------------------------------------------------------------------
// 三、三要素模板
// ---------------------------------------------------------------------------

/// 三要素模板（判据二）。
///
/// 锚点要求「为什么」带**规则引用**——即不只说「错了」，还要说「按哪条规范条款
/// 判的」。故 `why` 之外单独留 `rule_ref` 字段承载规范条款号（如
/// `VE-SPEC-LEX-3.2`），报告文本一并渲染。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuleTemplate {
    /// 该码的三要素文案（what / why / next）。
    pub what: &'static str,
    pub why: &'static str,
    pub next: &'static str,
    /// 规范条款引用。
    pub rule_ref: &'static str,
}

impl RuleTemplate {
    /// 三要素是否齐备（任一为空即不齐备——锚点：模板缺失→最小告知不静默）。
    pub fn is_complete(&self) -> bool {
        !self.what.is_empty()
            && !self.why.is_empty()
            && !self.next.is_empty()
            && !self.rule_ref.is_empty()
    }
}

/// 已登记的模板条目（码 → 模板）。
#[derive(Clone, Copy, Debug)]
pub struct TemplateEntry {
    /// 错误码。
    pub code: &'static str,
    /// 模板。
    pub template: RuleTemplate,
}

/// 模板登记表（判据二）。
///
/// 只登记**本域负责呈现**的码；查不到走 `TemplateMissing` 最小告知路径。
pub const TEMPLATE_TABLE: &[TemplateEntry] = &[
    TemplateEntry {
        code: "VE-F0415-UTF8-CONTINUATION",
        template: RuleTemplate {
            what: "出现孤立 UTF-8 续字节",
            why: "UTF-8 续字节（80..BF）只能跟在多字节序列的首字节之后",
            next: "确认源文件确为 UTF-8；若存为其他编码请按实际编码重新保存",
            rule_ref: "VE-SPEC-LEX-ENC-1",
        },
    },
    TemplateEntry {
        code: "VE-F0415-UTF8-TRUNCATED",
        template: RuleTemplate {
            what: "多字节 UTF-8 序列在文件尾被截断",
            why: "序列未收满即遇 EOF，末字节不构成合法字符",
            next: "补全末尾字符或删掉这个残缺字符",
            rule_ref: "VE-SPEC-LEX-ENC-2",
        },
    },
    TemplateEntry {
        code: "VE-F0410-MISMATCH",
        template: RuleTemplate {
            what: "括号配对失配",
            why: "括号须按后进先出闭合，交叉嵌套不合法",
            next: "检查该处括号与最近未闭合的开括号是否对应",
            rule_ref: "VE-SPEC-LEX-BRACE-1",
        },
    },
    TemplateEntry {
        code: "VE-F0412-RECURSION",
        template: RuleTemplate {
            what: "宏自引用，展开被冻结",
            why: "宏展开体直接或间接引用自身会导致无限展开",
            next: "拆开自引用或改用带参宏传入该值",
            rule_ref: "VE-SPEC-LEX-MACRO-4",
        },
    },
    TemplateEntry {
        code: "VE-F0411-UNKNOWN-DIRECTIVE",
        template: RuleTemplate {
            what: "未注册的预处理指令",
            why: "井号开头必须是已注册指令集之一",
            next: "检查指令拼写，或改用已注册的指令名",
            rule_ref: "VE-SPEC-LEX-PREPRO-1",
        },
    },
];

/// 查模板（判据二）。查不到返回 `None`，由调用方走最小告知路径。
pub fn template_of(code: &str) -> Option<&'static RuleTemplate> {
    for e in TEMPLATE_TABLE.iter() {
        if e.code == code {
            return Some(&e.template);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// 四、诊断与报告器
// ---------------------------------------------------------------------------

/// 一条诊断（下游 F0417 恢复策略消费本结构）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    /// 稳定错误码。
    pub code: &'static str,
    /// 错误族（判据一）。
    pub family: ErrorFamily,
    /// 严重度（判据四）。
    pub severity: Severity,
    /// 位置（主侧 + 可选副侧，判据三）。
    pub span: Span2,
    /// 三要素：发生了什么。
    pub what: String,
    /// 三要素：为什么。
    pub why: String,
    /// 三要素：下一步。
    pub next: String,
    /// 规范条款引用（判据二）。
    pub rule_ref: String,
    /// 模板是否齐备（不齐备时本条是「最小告知」而非完整报告）。
    pub template_complete: bool,
    /// 裁定过程的附注（未归族 / 严重度未登记 / 位置被钳等，逐条如实记录）。
    pub notes: Vec<String>,
}

impl Diagnostic {
    /// 本条是否为**完整报告**（而非最小告知）。
    ///
    /// 语义只认 `template_complete`：模板缺失时三要素会被兜底文案填满（不留空、
    /// 不 panic），若改用「四字段皆非空」判定，兜底后恒为真——最小告知会被误
    /// 认成完整报告，锚点「模板缺失→最小告知不静默」的区分就丢了。故此处直接
    /// 透传 `template_complete`，两个信号不再互相打架。
    pub fn is_complete(&self) -> bool {
        self.template_complete
    }

    /// 三要素四字段是否都非空（仅供核验兜底确实填上了，不作为「完整」判据）。
    pub fn all_fields_filled(&self) -> bool {
        !self.what.is_empty() && !self.why.is_empty() && !self.next.is_empty() && !self.rule_ref.is_empty()
    }

    /// 是否双侧定位。
    pub fn is_dual_side(&self) -> bool {
        self.span.is_dual()
    }

    /// 渲染为单行报告（判据二：三要素 + 位置 + 族 + 严重度）。
    pub fn render(&self) -> String {
        let mut s = String::new();
        s.push_str(self.severity.label());
        s.push('[');
        s.push_str(self.family.label());
        s.push_str("] ");
        s.push_str(self.code);
        s.push_str(" @ ");
        s.push_str(self.span.render().as_str());
        s.push_str("\n  发生了什么：");
        s.push_str(self.what.as_str());
        s.push_str("\n  为什么：");
        s.push_str(self.why.as_str());
        if !self.rule_ref.is_empty() {
            s.push_str("（规则 ");
            s.push_str(self.rule_ref.as_str());
            s.push('）');
        }
        s.push_str("\n  下一步：");
        s.push_str(self.next.as_str());
        if !self.template_complete {
            s.push_str("\n  （本条为最小告知：三要素模板缺失，待回填）");
        }
        for n in self.notes.iter() {
            s.push_str("\n  注记：");
            s.push_str(n.as_str());
        }
        s
    }
}

/// 报告器：汇总诊断并渲染（判据二的核心编排）。
///
/// 零静默的落点在这里：模板缺失要**登记**（`pending_templates`）而不是消失，
/// 上层能一眼看出「哪些码还缺模板」。
#[derive(Clone, Debug, Default)]
pub struct Reporter {
    diagnostics: Vec<Diagnostic>,
}

impl Reporter {
    /// 空报告器。
    pub fn new() -> Reporter {
        Reporter { diagnostics: Vec::new() }
    }

    /// 登记一条诊断。
    pub fn push(&mut self, d: Diagnostic) {
        self.diagnostics.push(d);
    }

    /// 诊断条数。
    pub fn len(&self) -> usize {
        self.diagnostics.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.diagnostics.is_empty()
    }

    /// 取诊断切片。
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    /// 按族筛（供 F0417 选恢复策略）。
    pub fn of_family(&self, f: ErrorFamily) -> Vec<&Diagnostic> {
        self.diagnostics.iter().filter(|d| d.family == f).collect()
    }

    /// 阻断性诊断条数（`severity == Error`）。
    pub fn blocking_count(&self) -> usize {
        self.diagnostics.iter().filter(|d| d.severity.is_blocking()).count()
    }

    /// 待回填模板的码（去重、保序）。
    pub fn pending_templates(&self) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = Vec::new();
        for d in self.diagnostics.iter() {
            if !d.template_complete && !out.contains(&d.code) {
                out.push(d.code);
            }
        }
        out
    }

    /// 全量渲染（每条一报告，空行分隔）。
    pub fn render_all(&self) -> String {
        let mut s = String::new();
        for (i, d) in self.diagnostics.iter().enumerate() {
            if i > 0 {
                s.push('\n');
            }
            s.push_str(d.render().as_str());
        }
        s
    }
}

// ---------------------------------------------------------------------------
// 五、主入口：从上游原始错误构造诊断
// ---------------------------------------------------------------------------

/// 上游错误源（判据一/三的输入面：F0412-F0415 各自的错误类型统一到此）。
///
/// 刻意**不**直接依赖上游的具体类型（那些结构体字段会各自演化）：本条只要求
/// 上游把「码 / 位置 / 副侧 / 三要素」四组信息交过来，归族与分级由本条的表裁定。
/// 这样 F0412-F0415 谁改了字段都不影响本条。
pub struct RawError {
    /// 稳定错误码。
    pub code: &'static str,
    /// 主侧位置。
    pub pos: Loc,
    /// 副侧位置（宏定义处 / include 来源处）。
    pub def: Option<Loc>,
    /// 上游给的三要素（可为空——空即触发最小告知路径）。
    pub what: String,
    pub why: String,
    pub next: String,
    /// 上游自带的规则引用（可为空，空则由模板表补）。
    pub rule_ref: String,
}

impl RawError {
    /// 构造（上游三要素可全空，用于测试最小告知路径）。
    pub fn new(code: &'static str, pos: Loc) -> RawError {
        RawError {
            code,
            pos,
            def: None,
            what: String::new(),
            why: String::new(),
            next: String::new(),
            rule_ref: String::new(),
        }
    }

    /// 挂副侧位置（双侧定位，判据三）。
    pub fn with_def(mut self, def: Loc) -> RawError {
        self.def = Some(def);
        self
    }

    /// 挂上游三要素与规则引用。
    pub fn with_triple(mut self, what: &str, why: &str, next: &str, rule_ref: &str) -> RawError {
        self.what = what.to_string();
        self.why = why.to_string();
        self.next = next.to_string();
        self.rule_ref = rule_ref.to_string();
        self
    }
}

/// 从上游原始错误构造一条诊断（判据一至判据四的汇合点）。
///
/// 流程：位置规范化 → 归族查表 → 严重度查表 → 模板查表 → 三要素齐备性判定
/// → 注记累积。任一环节「查不到」都不静默，如实进注记。
pub fn diagnose(raw: &RawError) -> Diagnostic {
    let mut notes: Vec<String> = Vec::new();

    // ---- 位置规范化（判据四：位置三元式；钳制要告知）----
    let mut span = Span2 {
        primary: raw.pos,
        secondary: raw.def,
    };
    let (np, p_clamped) = span.primary.normalized();
    span.primary = np;
    if p_clamped {
        notes.push(format!(
            "主侧位置不自洽已钳制为 {}",
            np.render()
        ));
    }
    if let Some(s) = span.secondary {
        let (ns, s_clamped) = s.normalized();
        span.secondary = Some(ns);
        if s_clamped {
            notes.push(format!("副侧位置不自洽已钳制为 {}", ns.render()));
        }
    }

    // ---- 归族（判据一：查表，不猜）----
    let family = classify(raw.code);
    if !family.is_classified() {
        notes.push(format!(
            "错误码 {} 未登记族归属，落未归族——需回填族表",
            raw.code
        ));
    }

    // ---- 严重度（判据四：表裁定；未登记按最严）----
    let (severity, sev_hit) = severity_of(raw.code);
    if !sev_hit {
        notes.push(format!(
            "错误码 {} 未登记严重度，按最严（错误）裁定——需回填严重度表",
            raw.code
        ));
    }

    // ---- 模板与三要素（判据二：模板缺失→最小告知不静默）----
    let tpl = template_of(raw.code);
    // 三要素取值优先级：上游给的 > 模板表登记的。**上游优先**是因为上游离现场
    // 最近（F0412 知道是哪次展开、F0415 知道是哪个字节），模板表是兜底。
    let mut what = raw.what.clone();
    let mut why = raw.why.clone();
    let mut next = raw.next.clone();
    let mut rule_ref = raw.rule_ref.clone();
    if let Some(t) = tpl {
        if what.is_empty() {
            what = t.what.to_string();
        }
        if why.is_empty() {
            why = t.why.to_string();
        }
        if next.is_empty() {
            next = t.next.to_string();
        }
        if rule_ref.is_empty() {
            rule_ref = t.rule_ref.to_string();
        }
    }
    // 仍缺项 → 走最小告知：缺什么补什么的最省告知，并标注不完整。
    let mut complete = true;
    if what.is_empty() {
        what = format!("错误码 {} 触发（上游未提供说明）", raw.code);
        complete = false;
    }
    if why.is_empty() {
        why = "本域无该码的规则模板，判定依据未回填".to_string();
        complete = false;
    }
    if next.is_empty() {
        // 族级兜底建议：至少给方向，不空着。
        next = family.remedy_hint().to_string();
        complete = false;
    }
    if rule_ref.is_empty() {
        rule_ref = "未回填".to_string();
        complete = false;
    }
    if !complete {
        notes.push(format!(
            "三要素模板不完整（待回填）：{}",
            raw.code
        ));
    }

    Diagnostic {
        code: raw.code,
        family,
        severity,
        span,
        what,
        why,
        next,
        rule_ref,
        template_complete: complete,
        notes,
    }
}

/// 批量诊断并出报告（上游 F0416 各错误源的统一入口）。
pub fn report_all(raws: &[RawError]) -> (Reporter, String) {
    let mut rep = Reporter::new();
    for r in raws.iter() {
        rep.push(diagnose(r));
    }
    let text = rep.render_all();
    (rep, text)
}

// ---------------------------------------------------------------------------
// 六、上游错误源的适配器（判据「上游 F0412-F0415 各错误源」）
// ---------------------------------------------------------------------------

/// 适配 F0412 `MacroError`：调用处 + 定义处双侧。
pub fn from_macro_error(
    code: &'static str,
    call_pos: Loc,
    def_pos: Option<Loc>,
    what: &str,
    why: &str,
    next: &str,
) -> Diagnostic {
    let mut raw = RawError::new(code, call_pos).with_triple(what, why, next, "VE-SPEC-LEX-MACRO-1");
    if let Some(d) = def_pos {
        raw = raw.with_def(d);
    }
    diagnose(&raw)
}

/// 适配 F0413 `CondError` / F0414 `IncludeError`：单侧（条件/include 无定义侧）。
pub fn from_single_side(
    code: &'static str,
    pos: Loc,
    line: usize,
    what: &str,
    why: &str,
    next: &str,
) -> Diagnostic {
    let raw = RawError::new(code, pos.with_line_col(line, 1))
        .with_triple(what, why, next, "");
    diagnose(&raw)
}

/// 适配 F0415 `EncodingFault`：字节位置 + 字节值，无行列（由调用方补）。
pub fn from_encoding_fault(code: &'static str, pos: Loc, what: &str, why: &str, next: &str) -> Diagnostic {
    let raw = RawError::new(code, pos).with_triple(what, why, next, "VE-SPEC-LEX-ENC-1");
    diagnose(&raw)
}
