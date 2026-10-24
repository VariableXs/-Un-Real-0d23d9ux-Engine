//! VE-F3419 · 令牌文档 —— 四件指南 + 类型手册 + 迁移说明 + 默认可信 + CI 防漂移
//! + 反例对照教学。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3419`
//!
//! # 职责（锚点原文拆解）
//!
//! - **四件指南**：入门（Getting Started）、令牌参考（Token Reference）、
//!   迁移（Migration）、排障（Troubleshooting）。四件缺一即收口阻断——
//!   文档不是「有就行」，是**四件齐备才算有**；
//! - **类型手册**：六类令牌（颜色/长度/时长/比例/计数/字体）的写法、
//!   单位、取值范围、常见错法。类型手册与 [`TokenType`] 同源，新增类型
//!   时手册缺条目会被 [`DocSet::audit`] 直接点名；
//! - **迁移说明**：旧格式 → 新格式的逐条对照。迁移章节必须给出
//!   「旧写法 / 新写法 / 为什么」三列，只给新写法等于让读者自己猜；
//! - **默认值可信标准**：文档里写的默认值必须与代码里的默认值逐字相同。
//!   [`DefaultClaim`] 把两侧都拿在手上比对，不一致记
//!   [`DocCode::DefaultDrift`]——文档写错默认值比不写更坏，读者会信；
//! - **CI 防漂移**：整份文档的「默认值指纹」参与 CI 比对，指纹变了必须
//!   有人复核（[`DocSet::fingerprint`]）。指纹只覆盖可比对的部分（默认值
//!   与类型表），不覆盖散文——否则改个错别字就红一次，CI 会被绕过；
//! - **反例对照**：每条反例必须带「错误写法 + 后果」两段，且错误写法
//!   必须真的会被验证器拒绝（调用
//!   [`crate::svstar2::ver05c_validator::validate`] 实测）。教人写对的
//!   前提是先证明错的确实错。
//!
//! # 为什么文档要能被机器审
//!
//! 文档腐坏的方式不是「写错」，是「过期」：代码改了默认值，文档还写着
//! 旧的，读者照着写，界面上就差 2px。这类问题人工复核抓不住（谁会为了
//! 一个默认值去翻代码），只能让机器每天比一次。所以文档在本模块里
//! 不是字符串，是**带断言的结构**：每条默认值声明都带着它声称的代码值，
//! 每条反例都带着它的验证结论。
//!
//! # 复杂度
//!
//! - `audit`：O(章节数 + 反例数)；
//! - `fingerprint`：O(默认值声明数)；
//! - `drift_check`：O(1) 比较指纹。
//!
//! # 零 panic 面
//!
//! 全部遍历用 `iter()` / `get()`，无索引；`format!` 不失败。测试区除外。

use alloc::vec;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::svstar2::ver05a_appread::TokenType;

// ---------------------------------------------------------------------------
// 一、诊断码（E17 段续：5343 起）
// ---------------------------------------------------------------------------

/// 文档层诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DocCode {
    /// 四件指南缺件。
    GuideMissing = 5343,
    /// 类型手册缺类型条目。
    TypeManualGap = 5344,
    /// 迁移说明缺列（旧写法/新写法/为什么）。
    MigrationColumnGap = 5345,
    /// 文档默认值与代码默认值不一致。
    DefaultDrift = 5346,
    /// 文档指纹与基线不一致（需人工复核）。
    FingerprintDrift = 5347,
    /// 反例缺少后果说明。
    CounterExampleIncomplete = 5348,
    /// 反例的错误写法竟然通过了验证器（教错了）。
    CounterExamplePasses = 5349,
    /// 标题层级跳跃（h1 直接到 h3，读屏会乱）。
    HeadingSkip = 5350,
    /// 章节缺读屏摘要。
    MissingSummary = 5351,
    /// 代码块缺语言标记。
    UnlabeledCodeBlock = 5352,
    /// 章节正文为空。
    EmptySection = 5353,
}

impl DocCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            DocCode::GuideMissing => "四件指南缺件",
            DocCode::TypeManualGap => "类型手册缺类型条目",
            DocCode::MigrationColumnGap => "迁移说明缺列",
            DocCode::DefaultDrift => "文档默认值与代码不一致",
            DocCode::FingerprintDrift => "文档指纹与基线不一致",
            DocCode::CounterExampleIncomplete => "反例缺后果说明",
            DocCode::CounterExamplePasses => "反例的错误写法通过了验证器",
            DocCode::HeadingSkip => "标题层级跳跃",
            DocCode::MissingSummary => "章节缺读屏摘要",
            DocCode::UnlabeledCodeBlock => "代码块缺语言标记",
            DocCode::EmptySection => "章节正文为空",
        }
    }

    /// 是否阻断收口（反例通过验证器是「教错」，比缺文档更严重）。
    pub fn blocking(self) -> bool {
        !matches!(self, DocCode::FingerprintDrift)
    }
}

/// 四件指南的封闭全集。缺任何一件都是 [`DocCode::GuideMissing`]。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum GuideKind {
    /// 入门。
    GettingStarted,
    /// 令牌参考。
    TokenReference,
    /// 迁移。
    Migration,
    /// 排障。
    Troubleshooting,
}

impl GuideKind {
    /// 四件全量（顺序即推荐阅读顺序）。
    pub fn all() -> [GuideKind; 4] {
        [
            GuideKind::GettingStarted,
            GuideKind::TokenReference,
            GuideKind::Migration,
            GuideKind::Troubleshooting,
        ]
    }

    /// 章节 id 前缀。
    pub fn id(self) -> &'static str {
        match self {
            GuideKind::GettingStarted => "guide.start",
            GuideKind::TokenReference => "guide.reference",
            GuideKind::Migration => "guide.migration",
            GuideKind::Troubleshooting => "guide.troubleshooting",
        }
    }

    /// 中文标题。
    pub fn title(self) -> &'static str {
        match self {
            GuideKind::GettingStarted => "入门指南",
            GuideKind::TokenReference => "令牌参考",
            GuideKind::Migration => "迁移说明",
            GuideKind::Troubleshooting => "排障指南",
        }
    }
}

// ---------------------------------------------------------------------------
// 二、文档模型
// ---------------------------------------------------------------------------

/// 一段代码示例。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CodeBlock {
    /// 语言标记（`text` / `rust` / `toml`）。空标记即违规。
    pub lang: String,
    /// 内容。
    pub body: String,
}

impl CodeBlock {
    /// 构造。
    pub fn new(lang: &str, body: &str) -> CodeBlock {
        CodeBlock {
            lang: lang.to_string(),
            body: body.to_string(),
        }
    }
}

/// 一条默认值声明：文档说的 + 代码里的真值。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DefaultClaim {
    /// 令牌路径。
    pub path: String,
    /// 文档里写的默认值。
    pub documented: String,
    /// 代码里的默认值。
    pub actual: String,
}

impl DefaultClaim {
    /// 构造。
    pub fn new(path: &str, documented: &str, actual: &str) -> DefaultClaim {
        DefaultClaim {
            path: path.to_string(),
            documented: documented.to_string(),
            actual: actual.to_string(),
        }
    }

    /// 是否一致。
    pub fn coherent(&self) -> bool {
        self.documented == self.actual
    }
}

/// 一条反例：错误写法 + 后果 + （可选的）期望诊断码。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CounterExample {
    /// 错误写法（一段令牌文本）。
    pub wrong: String,
    /// 后果说明（人话）。
    pub consequence: String,
    /// 期望被验证器拒绝（实测结论）。
    pub rejected_in_practice: bool,
}

impl CounterExample {
    /// 构造并保留实测结论由调用方填。
    pub fn new(wrong: &str, consequence: &str) -> CounterExample {
        CounterExample {
            wrong: wrong.to_string(),
            consequence: consequence.to_string(),
            rejected_in_practice: false,
        }
    }

    /// 用验证器实测一次，把结论写回。返回是否被拒。
    pub fn rehearse(&mut self) -> bool {
        let report = crate::svstar2::ver05c_validator::validate(&self.wrong, false);
        self.rejected_in_practice = !report.clean();
        self.rejected_in_practice
    }
}

/// 迁移对照的一行（三列齐全）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MigrationRow {
    /// 旧写法。
    pub old_form: String,
    /// 新写法。
    pub new_form: String,
    /// 为什么改。
    pub reason: String,
}

impl MigrationRow {
    /// 三列齐备。
    pub fn complete(&self) -> bool {
        !self.old_form.trim().is_empty()
            && !self.new_form.trim().is_empty()
            && !self.reason.trim().is_empty()
    }
}

/// 一个章节。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Section {
    /// 章节 id。
    pub id: String,
    /// 标题。
    pub title: String,
    /// 层级（1 = 一级标题）。
    pub level: u8,
    /// 读屏摘要（一句话，必须非空）。
    pub summary: String,
    /// 正文段落。
    pub paragraphs: Vec<String>,
    /// 代码块。
    pub code: Vec<CodeBlock>,
}

impl Section {
    /// 构造。
    pub fn new(id: &str, title: &str, level: u8, summary: &str) -> Section {
        Section {
            id: id.to_string(),
            title: title.to_string(),
            level,
            summary: summary.to_string(),
            paragraphs: Vec::new(),
            code: Vec::new(),
        }
    }

    /// 追加段落。
    pub fn with_paragraph(mut self, text: &str) -> Section {
        self.paragraphs.push(text.to_string());
        self
    }

    /// 追加代码块。
    pub fn with_code(mut self, lang: &str, body: &str) -> Section {
        self.code.push(CodeBlock::new(lang, body));
        self
    }
}

/// 一份文档集。
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct DocSet {
    /// 章节。
    pub sections: Vec<Section>,
    /// 类型手册覆盖的类型（必须覆盖六类全集）。
    pub type_manual: Vec<TokenType>,
    /// 迁移对照。
    pub migration: Vec<MigrationRow>,
    /// 默认值声明。
    pub defaults: Vec<DefaultClaim>,
    /// 反例。
    pub counter_examples: Vec<CounterExample>,
}

/// 一条审计发现。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DocFinding {
    /// 诊断码。
    pub code: DocCode,
    /// 章节 id（可为空）。
    pub section: String,
    /// 说明。
    pub detail: String,
}

impl DocFinding {
    fn new(code: DocCode, section: &str, detail: &str) -> DocFinding {
        DocFinding {
            code,
            section: section.to_string(),
            detail: detail.to_string(),
        }
    }

    /// 读屏渲染。
    pub fn render(&self) -> String {
        if self.section.is_empty() {
            format!("E{} {}（{}）", self.code.code(), self.code.text(), self.detail)
        } else {
            format!(
                "E{} {}［{}］（{}）",
                self.code.code(),
                self.code.text(),
                self.section,
                self.detail
            )
        }
    }
}

/// 审计报告。
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct DocReport {
    /// 发现。
    pub findings: Vec<DocFinding>,
    /// 章节数。
    pub sections: u32,
    /// 反例数。
    pub counter_examples: u32,
    /// 默认值声明数。
    pub defaults: u32,
}

impl DocReport {
    /// 是否无阻断项。
    pub fn clean(&self) -> bool {
        !self.findings.iter().any(|f| f.code.blocking())
    }

    /// 阻断项计数。
    pub fn blocking_count(&self) -> u32 {
        self.findings
            .iter()
            .filter(|f| f.code.blocking())
            .count() as u32
    }

    /// 读屏清单。
    pub fn reading_lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        out.push(format!(
            "文档审计：章节 {}，反例 {}，默认值声明 {}，阻断项 {}，判定 {}",
            self.sections,
            self.counter_examples,
            self.defaults,
            self.blocking_count(),
            if self.clean() { "通过" } else { "阻断" }
        ));
        for f in self.findings.iter() {
            out.push(f.render());
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 三、审计
// ---------------------------------------------------------------------------

impl DocSet {
    /// 空文档集。
    pub fn new() -> DocSet {
        DocSet::default()
    }

    /// 加章节。
    pub fn push_section(&mut self, section: Section) {
        self.sections.push(section);
    }

    /// 加默认值声明。
    pub fn push_default(&mut self, claim: DefaultClaim) {
        self.defaults.push(claim);
    }

    /// 加反例（自动实测一次，结论写进 `rejected_in_practice`）。
    pub fn push_counter_example(&mut self, wrong: &str, consequence: &str) {
        let mut c = CounterExample::new(wrong, consequence);
        c.rehearse();
        self.counter_examples.push(c);
    }

    /// 四件指南是否齐备：按章节 id 前缀判定。
    pub fn guides_present(&self) -> Vec<GuideKind> {
        let mut out = Vec::new();
        for kind in GuideKind::all().iter() {
            let prefix = kind.id();
            if self
                .sections
                .iter()
                .any(|s| s.id == prefix || s.id.starts_with(&format!("{}.", prefix)))
            {
                out.push(*kind);
            }
        }
        out
    }

    /// 审计全量。
    pub fn audit(&self) -> DocReport {
        let mut report = DocReport {
            findings: Vec::new(),
            sections: self.sections.len() as u32,
            counter_examples: self.counter_examples.len() as u32,
            defaults: self.defaults.len() as u32,
        };
        // 1. 四件指南
        let present = self.guides_present();
        for kind in GuideKind::all().iter() {
            if !present.contains(kind) {
                report
                    .findings
                    .push(DocFinding::new(DocCode::GuideMissing, "", kind.title()));
            }
        }
        // 2. 类型手册覆盖
        for ty in [
            TokenType::Color,
            TokenType::Length,
            TokenType::Duration,
            TokenType::Ratio,
            TokenType::Count,
            TokenType::Font,
        ]
        .iter()
        {
            if !self.type_manual.contains(ty) {
                report.findings.push(DocFinding::new(
                    DocCode::TypeManualGap,
                    "guide.reference",
                    ty.label(),
                ));
            }
        }
        // 3. 迁移三列
        for row in self.migration.iter() {
            if !row.complete() {
                report.findings.push(DocFinding::new(
                    DocCode::MigrationColumnGap,
                    "guide.migration",
                    &row.old_form,
                ));
            }
        }
        // 4. 默认值一致
        for claim in self.defaults.iter() {
            if !claim.coherent() {
                let detail = format!(
                    "{}：文档 {} ≠ 代码 {}",
                    claim.path, claim.documented, claim.actual
                );
                report
                    .findings
                    .push(DocFinding::new(DocCode::DefaultDrift, "", &detail));
            }
        }
        // 5. 反例完备 + 实测
        for cx in self.counter_examples.iter() {
            if cx.consequence.trim().is_empty() {
                report.findings.push(DocFinding::new(
                    DocCode::CounterExampleIncomplete,
                    "",
                    &cx.wrong,
                ));
            }
            if !cx.rejected_in_practice {
                report.findings.push(DocFinding::new(
                    DocCode::CounterExamplePasses,
                    "",
                    &cx.wrong,
                ));
            }
        }
        // 6. 无障碍：层级不跳、摘要有、代码有语言标记、正文非空
        let mut last_level: u8 = 1;
        for s in self.sections.iter() {
            if s.level > last_level.saturating_add(1) {
                report
                    .findings
                    .push(DocFinding::new(DocCode::HeadingSkip, &s.id, &s.title));
            }
            last_level = s.level.max(1);
            if s.summary.trim().is_empty() {
                report
                    .findings
                    .push(DocFinding::new(DocCode::MissingSummary, &s.id, ""));
            }
            for cb in s.code.iter() {
                if cb.lang.trim().is_empty() {
                    report
                        .findings
                        .push(DocFinding::new(DocCode::UnlabeledCodeBlock, &s.id, ""));
                }
            }
            if s.paragraphs.is_empty() && s.code.is_empty() {
                report
                    .findings
                    .push(DocFinding::new(DocCode::EmptySection, &s.id, &s.title));
            }
        }
        report
    }

    /// 文档指纹：只覆盖可比对的结构（默认值 + 类型手册），不覆盖散文。
    /// 返回值直接进 CI 基线文件。
    pub fn fingerprint(&self) -> u64 {
        let mut hash: u64 = 0xCBF2_9CE4_8422_2325;
        // 默认值按路径排序后参与，保证声明顺序不影响指纹。
        let mut pairs: Vec<(&str, &str)> = self
            .defaults
            .iter()
            .map(|d| (d.path.as_str(), d.actual.as_str()))
            .collect();
        pairs.sort();
        for (path, value) in pairs {
            for byte in path.as_bytes().iter().chain(value.as_bytes()) {
                hash ^= *byte as u64;
                hash = hash.wrapping_mul(0x0000_0100_0000_01B3);
            }
        }
        for ty in self.type_manual.iter() {
            hash ^= ty.short() as u64;
            hash = hash.wrapping_mul(0x0000_0100_0000_01B3);
        }
        hash
    }

    /// CI 防漂移：与基线指纹比对。
    pub fn drift_check(&self, baseline: u64) -> Result<(), DocCode> {
        if self.fingerprint() == baseline {
            Ok(())
        } else {
            Err(DocCode::FingerprintDrift)
        }
    }

    /// 读屏友好的整册导出：每个章节先念标题与摘要，再念代码。
    pub fn reading_dump(&self) -> Vec<String> {
        let mut out = Vec::new();
        for s in self.sections.iter() {
            out.push(format!(
                "h{} {}（{}）",
                s.level, s.title, s.summary
            ));
            for p in s.paragraphs.iter() {
                out.push(p.clone());
            }
            for cb in s.code.iter() {
                out.push(format!("代码[{}]：{}", cb.lang, cb.body));
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 四、标准文档骨架（四件齐备，供测试与出厂文档复用）
// ---------------------------------------------------------------------------

/// 构造一份**通过审计**的标准文档集。这不是示例数据，是出厂文档的骨架：
/// 真实产品文档在此基础上补正文，四件指南与类型手册的结构不动。
pub fn standard_doc() -> DocSet {
    let mut doc = DocSet::new();
    doc.push_section(
        Section::new(
            GuideKind::GettingStarted.id(),
            GuideKind::GettingStarted.title(),
            1,
            "三步上手：写令牌文件、跑验证器、接读门面",
        )
        .with_paragraph("先写一份最小令牌文件，再用验证器确认三查全绿。")
        .with_code("text", "color.accent = #4C8DFF\nspacing.gap = 12px\n"),
    );
    doc.push_section(
        Section::new(
            GuideKind::TokenReference.id(),
            GuideKind::TokenReference.title(),
            2,
            "六类令牌的写法、单位与取值范围",
        )
        .with_paragraph("类型手册覆盖颜色、长度、时长、比例、计数、字体六类。")
        .with_code("text", "motion.fast = 120ms\n"),
    );
    doc.push_section(
        Section::new(
            GuideKind::Migration.id(),
            GuideKind::Migration.title(),
            2,
            "旧格式到新格式的逐条对照",
        )
        .with_paragraph("迁移只改写法，不改语义；语义变更另立条目。"),
    );
    doc.push_section(
        Section::new(
            GuideKind::Troubleshooting.id(),
            GuideKind::Troubleshooting.title(),
            2,
            "按诊断码查原因与修法",
        )
        .with_paragraph("先看行号，再看码，最后看反例对照。"),
    );
    doc.type_manual = alloc::vec![
        TokenType::Color,
        TokenType::Length,
        TokenType::Duration,
        TokenType::Ratio,
        TokenType::Count,
        TokenType::Font,
    ];
    doc.migration = alloc::vec![MigrationRow {
        old_form: "accent #4C8DFF".to_string(),
        new_form: "color.accent = #4C8DFF".to_string(),
        reason: "旧格式没有等号，验证器无法定位行内错误".to_string(),
    }];
    doc.push_default(DefaultClaim::new("spacing.gap", "12px", "12px"));
    doc.push_default(DefaultClaim::new("motion.fast", "120ms", "120ms"));
    doc.push_counter_example(
        "motion.fast = 12px\n",
        "时长写成 px，验证器报类型后缀冲突，运行期取不到值",
    );
    doc.push_counter_example(
        "color.a = {color.missing}\n",
        "引用断链，验证器报断链并给出行号",
    );
    doc
}

// ---------------------------------------------------------------------------
// 五、测试支撑
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_doc_passes_audit() {
        let doc = standard_doc();
        let r = doc.audit();
        assert!(r.clean(), "标准文档必须零阻断：{:?}", r.reading_lines());
        assert_eq!(doc.guides_present().len(), 4);
        assert_eq!(r.counter_examples, 2);
    }

    #[test]
    fn missing_guide_blocks_closure() {
        let mut doc = standard_doc();
        doc.sections.retain(|s| !s.id.starts_with("guide.troubleshooting"));
        let r = doc.audit();
        assert!(!r.clean());
        assert!(r
            .findings
            .iter()
            .any(|f| f.code == DocCode::GuideMissing && f.detail == "排障指南"));
    }

    #[test]
    fn default_drift_is_flagged_with_both_values() {
        let mut doc = standard_doc();
        doc.push_default(DefaultClaim::new("radius.card", "8px", "6px"));
        let r = doc.audit();
        let f = r
            .findings
            .iter()
            .find(|f| f.code == DocCode::DefaultDrift)
            .expect("默认值漂移被发现");
        assert!(f.detail.contains("8px"));
        assert!(f.detail.contains("6px"));
    }

    #[test]
    fn counter_example_that_passes_is_worse_than_missing() {
        let mut doc = standard_doc();
        // 这条其实是合法令牌——教它做反例就是教错。
        doc.push_counter_example("color.ok = #FFFFFF\n", "这条并不错");
        let r = doc.audit();
        assert!(r.findings.iter().any(|f| f.code == DocCode::CounterExamplePasses));
        assert!(!r.clean());
    }

    #[test]
    fn fingerprint_is_order_independent_and_detects_drift() {
        let a = standard_doc();
        let base = a.fingerprint();
        assert!(a.drift_check(base).is_ok());
        // 顺序变换不影响指纹。
        let mut b = standard_doc();
        b.defaults.reverse();
        assert_eq!(b.fingerprint(), base);
        // 改一个默认值 -> 漂移。
        let mut c = standard_doc();
        c.defaults.push(DefaultClaim::new("radius.card", "8px", "9px"));
        assert_eq!(c.drift_check(base).err(), Some(DocCode::FingerprintDrift));
    }

    #[test]
    fn heading_skip_and_missing_summary_are_accessibility_errors() {
        let mut doc = standard_doc();
        doc.push_section(
            Section::new("guide.extra", "附加章节", 5, "")
                .with_paragraph("层级从 2 直接跳到 5。"),
        );
        let r = doc.audit();
        assert!(r.findings.iter().any(|f| f.code == DocCode::HeadingSkip));
        assert!(r.findings.iter().any(|f| f.code == DocCode::MissingSummary));
    }

    #[test]
    fn migration_row_needs_all_three_columns() {
        let mut doc = standard_doc();
        doc.migration.push(MigrationRow {
            old_form: "accent #4C8DFF".to_string(),
            new_form: String::new(),
            reason: "缺新写法".to_string(),
        });
        let r = doc.audit();
        assert!(r.findings.iter().any(|f| f.code == DocCode::MigrationColumnGap));
    }

    #[test]
    fn unlabeled_code_block_is_flagged() {
        let mut doc = standard_doc();
        doc.push_section(
            Section::new("guide.sample", "示例", 2, "示例章节")
                .with_code("", "color.a = #FFFFFF\n"),
        );
        let r = doc.audit();
        assert!(r.findings.iter().any(|f| f.code == DocCode::UnlabeledCodeBlock));
    }

    #[test]
    fn reading_dump_starts_each_section_with_title_and_summary() {
        let doc = standard_doc();
        let dump = doc.reading_dump();
        assert!(dump
            .first()
            .map(|l| l.contains("入门指南") && l.contains("三步上手"))
            .unwrap_or(false));
    }
}
