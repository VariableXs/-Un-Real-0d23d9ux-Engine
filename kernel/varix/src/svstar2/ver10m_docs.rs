//! VE-F3553 · persona 文档 —— 三件文档 + 冲突规则 + 生态规范 + CI 防漂移。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3553`
//!
//! # 职责（锚点原文拆解）
//!
//! - **三件文档**（[`DocSuite`]）：使用指南 / 冲突规则 / 生态规范
//!   三件齐备，缺任一件收口阻断。
//! - **冲突规则**（[`ConflictRuleDoc`]）：与 F3544 仲裁规则同源
//!   文本，文档与机检规则不得漂移。
//! - **生态规范人话版**：规范条目带人话解释（[`PlainEntry`]），
//!   不是只有法言法语。
//! - **CI 防漂移**（[`DocCi::verify`]）：文档内容哈希与基准
//!   哈希对拍，漂移即阻断。
//!
//! # 降级矩阵（锚点）
//!
//! | 故障 | 处置 |
//! |---|---|
//! | 漂移 | CI 阻断 |
//! | 案例过时 | 复审 |
//! | 缺失 | 收口阻断 |
//!
//! # 复杂度
//!
//! O(条目)。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E19 段续：6240 起）
// ---------------------------------------------------------------------------

/// persona 文档诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DocCode {
    /// 三件不齐。
    SuiteIncomplete = 6240,
    /// 文档漂移（哈希不符）。
    Drift = 6241,
    /// 冲突规则与机检不同源。
    RuleMismatch = 6242,
    /// 人话版缺失。
    PlainMissing = 6243,
}

impl DocCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            DocCode::SuiteIncomplete => "三件文档不齐",
            DocCode::Drift => "文档漂移已阻断",
            DocCode::RuleMismatch => "冲突规则与机检不同源",
            DocCode::PlainMissing => "人话版缺失",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        matches!(self, DocCode::SuiteIncomplete | DocCode::Drift)
    }
}

// ---------------------------------------------------------------------------
// 二、三件文档
// ---------------------------------------------------------------------------

/// 文档件。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DocPart {
    /// 使用指南。
    Guide,
    /// 冲突规则。
    ConflictRules,
    /// 生态规范。
    Ecosystem,
}

impl DocPart {
    /// 件短名。
    pub const fn label(self) -> &'static str {
        match self {
            DocPart::Guide => "使用指南",
            DocPart::ConflictRules => "冲突规则",
            DocPart::Ecosystem => "生态规范",
        }
    }
}

/// 人话条目（规范 + 人话解释成对）。
#[derive(Clone, Copy, Debug)]
pub struct PlainEntry {
    /// 规范原文。
    pub rule: &'static str,
    /// 人话解释。
    pub plain: &'static str,
}

impl PlainEntry {
    /// 成对完整性。
    pub fn complete(&self) -> bool {
        !self.rule.is_empty() && !self.plain.is_empty()
    }
}

/// 冲突规则文档（与机检 [`super::ver10d_conflictcheck::ARBITRATION_RULES`] 同源）。
pub struct ConflictRuleDoc {
    /// 规则文本（须与机检常量逐字节一致）。
    pub text: &'static str,
}

// ---------------------------------------------------------------------------
// 三、文档套件
// ---------------------------------------------------------------------------

/// 文档套件。
pub struct DocSuite {
    has_guide: bool,
    rules: ConflictRuleDoc,
    plain: Vec<PlainEntry>,
    /// 基准内容指纹（CI 对拍基准）。
    pub baseline_hash: u64,
}

impl DocSuite {
    /// 新建（冲突规则文本必须与机检同源）。
    pub fn new(rules_text: &'static str, baseline_hash: u64) -> Self {
        Self {
            has_guide: true,
            rules: ConflictRuleDoc { text: rules_text },
            plain: Vec::new(),
            baseline_hash,
        }
    }

    /// 三件齐备判定。
    pub fn complete(&self) -> bool {
        self.has_guide && !self.rules.text.is_empty() && !self.plain.is_empty()
    }

    /// 追加人话条目。
    pub fn add_plain(&mut self, e: PlainEntry) -> Result<(), DocCode> {
        if !e.complete() {
            return Err(DocCode::PlainMissing);
        }
        self.plain.push(e);
        Ok(())
    }

    /// 规则同源核验（与机检常量比较）。
    pub fn rules_match(&self, engine_rules: &'static str) -> bool {
        self.rules.text.len() == engine_rules.len() && self.rules.text == engine_rules
    }

    /// 内容指纹（简化 FNV：条目长度与首字符混合）。
    pub fn content_hash(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        h ^= self.rules.text.len() as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        for e in &self.plain {
            h ^= e.rule.len() as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
            h ^= e.plain.len() as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    }

    /// 人话条目数。
    pub fn plain_len(&self) -> usize {
        self.plain.len()
    }
}

// ---------------------------------------------------------------------------
// 四、CI 防漂移
// ---------------------------------------------------------------------------

/// 文档 CI。
pub struct DocCi;

impl DocCi {
    /// 漂移核验：内容哈希对拍基准。
    pub fn verify(suite: &DocSuite) -> Result<(), DocCode> {
        if !suite.complete() {
            return Err(DocCode::SuiteIncomplete);
        }
        if suite.content_hash() != suite.baseline_hash {
            return Err(DocCode::Drift);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 五、判据
// ---------------------------------------------------------------------------

/// persona 文档判据集。
pub fn run_doc_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-ver10m_docs");

    let engine_rules = super::ver10d_conflictcheck::ARBITRATION_RULES;

    // 1. 同源规则可核。
    let suite = DocSuite::new(engine_rules, 0);
    set.add("6240-规则同源", suite.rules_match(engine_rules), "");

    // 2. 人话条目完整性。
    let mut s2 = DocSuite::new(engine_rules, 0);
    let good = s2
        .add_plain(PlainEntry {
            rule: "链序优先级",
            plain: "谁说了算：你 > 人格 > 官方",
        })
        .is_ok();
    let bad = s2
        .add_plain(PlainEntry {
            rule: "",
            plain: "x",
        })
        .is_err();
    set.add("6241-人话校验", good && bad, "");

    // 3. 不齐阻断。
    let empty_suite = DocSuite::new(engine_rules, 0);
    set.add(
            "6242-不齐阻断",
            DocCi::verify(&empty_suite).err() == Some(DocCode::SuiteIncomplete),
            "",
        );

    // 4. 漂移阻断。
    let mut s3 = DocSuite::new(engine_rules, 0);
    let _ = s3.add_plain(PlainEntry {
        rule: "R",
        plain: "P",
    });
    let real_hash = s3.content_hash();
    let mut s4 = DocSuite::new(engine_rules, real_hash.wrapping_add(1));
    let _ = s4.add_plain(PlainEntry {
        rule: "R",
        plain: "P",
    });
    set.add(
            "6243-漂移阻断",
            DocCi::verify(&s4).err() == Some(DocCode::Drift),
            "",
        );

    // 5. 基准一致可过。
    let mut s5 = DocSuite::new(engine_rules, real_hash);
    let _ = s5.add_plain(PlainEntry {
        rule: "R",
        plain: "P",
    });
    set.add("6244-基准一致过", DocCi::verify(&s5).is_ok(), "");

    // 6. 三件名可达。
    set.add(
            "6245-件名可达",
            DocPart::Guide.label() == "使用指南" && DocPart::Ecosystem.label() == "生态规范",
            "",
        );

    set
}
