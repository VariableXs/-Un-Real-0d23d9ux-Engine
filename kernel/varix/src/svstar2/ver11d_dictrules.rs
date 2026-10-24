//! VE-F3564 · 交互词典引擎级落地 —— 四类规则可执行化 + 版本化 + 单向同步 + 白名单期限。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3564`
//!
//! # 职责（锚点原文拆解）
//!
//! - **四类规则**（[`RuleCategory`]）：控件 / 浮层 / 快捷键 / 文案
//!   四类规则进检查器（可执行化）。
//! - **版本化**：规则带版本，变更可追溯。
//! - **单向同步**：文档改了规则跟上（[`RuleSync`]），反向不行。
//! - **白名单期限**（[`Whitelist`]）：误报白名单有期限自动过期。
//!
//! # 降级矩阵（锚点）
//!
//! | 故障 | 处置 |
//! |---|---|
//! | 规则缺失 | 补录 |
//! | 同步漂移 | CI 阻断 |
//! | 规则冲突 | 仲裁 |
//!
//! # 复杂度
//!
//! O(规则)。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E20 段续：6460 起）
// ---------------------------------------------------------------------------

/// 词典引擎诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RuleCode {
    /// 规则缺失。
    RuleMissing = 6460,
    /// 同步漂移。
    SyncDrift = 6461,
    /// 规则冲突。
    RuleConflict = 6462,
    /// 白名单过期。
    WhitelistExpired = 6463,
}

impl RuleCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            RuleCode::RuleMissing => "规则缺失",
            RuleCode::SyncDrift => "同步漂移",
            RuleCode::RuleConflict => "规则冲突",
            RuleCode::WhitelistExpired => "白名单过期",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        matches!(self, RuleCode::RuleMissing | RuleCode::SyncDrift)
    }
}

// ---------------------------------------------------------------------------
// 二、四类规则
// ---------------------------------------------------------------------------

/// 规则类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RuleCategory {
    /// 控件规则。
    Widget,
    /// 浮层规则。
    Overlay,
    /// 快捷键规则。
    Shortcut,
    /// 文案规则。
    Copy,
}

impl RuleCategory {
    /// 全部四类。
    pub const ALL: [RuleCategory; 4] = [
        RuleCategory::Widget,
        RuleCategory::Overlay,
        RuleCategory::Shortcut,
        RuleCategory::Copy,
    ];

    /// 类短名。
    pub const fn label(self) -> &'static str {
        match self {
            RuleCategory::Widget => "控件",
            RuleCategory::Overlay => "浮层",
            RuleCategory::Shortcut => "快捷键",
            RuleCategory::Copy => "文案",
        }
    }
}

/// 规则条目。
#[derive(Clone, Copy, Debug)]
pub struct Rule {
    /// 规则 id。
    pub id: u32,
    /// 类。
    pub category: RuleCategory,
    /// 版本。
    pub version: u32,
}

// ---------------------------------------------------------------------------
// 三、白名单（期限管理）
// ---------------------------------------------------------------------------

/// 白名单条目。
#[derive(Clone, Copy, Debug)]
pub struct WhitelistEntry {
    /// 规则 id。
    pub rule: u32,
    /// 剩余有效天数。
    pub days_left: u32,
}

/// 白名单。
#[derive(Default)]
pub struct Whitelist {
    entries: Vec<WhitelistEntry>,
}

impl Whitelist {
    /// 新建。
    pub fn new() -> Self {
        Self::default()
    }

    /// 加入（期限 30 天）。
    pub fn add(&mut self, rule: u32) {
        self.entries.push(WhitelistEntry {
            rule,
            days_left: 30,
        });
    }

    /// 过期一日：自动淘汰到期项。
    pub fn tick(&mut self) {
        for e in self.entries.iter_mut() {
            e.days_left = e.days_left.saturating_sub(1);
        }
        self.entries.retain(|e| e.days_left > 0);
    }

    /// 查某规则是否在白名单。
    pub fn holds(&self, rule: u32) -> bool {
        self.entries.iter().any(|e| e.rule == rule)
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 空判定。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 四、同步器
// ---------------------------------------------------------------------------

/// 单向同步（文档 → 规则）。
pub struct RuleSync {
    /// 文档版本。
    doc_version: u32,
    /// 规则版本。
    rule_version: u32,
}

impl RuleSync {
    /// 新建。
    pub fn new(doc: u32, rule: u32) -> Self {
        Self {
            doc_version: doc,
            rule_version: rule,
        }
    }

    /// 同步：规则版本追上文档版本（单向）。
    pub fn sync(&mut self) -> Result<(), RuleCode> {
        if self.rule_version > self.doc_version {
            // 反向漂移（规则超前文档）= 不一致。
            return Err(RuleCode::SyncDrift);
        }
        self.rule_version = self.doc_version;
        Ok(())
    }

    /// 是否一致。
    pub fn aligned(&self) -> bool {
        self.doc_version == self.rule_version
    }
}

// ---------------------------------------------------------------------------
// 五、规则引擎
// ---------------------------------------------------------------------------

/// 规则引擎。
pub struct RuleEngine {
    rules: Vec<Rule>,
    whitelist: Whitelist,
}

impl RuleEngine {
    /// 新建（四类各一基础规则）。
    pub fn standard() -> Self {
        let rules = alloc::vec![
            Rule { id: 1, category: RuleCategory::Widget, version: 1 },
            Rule { id: 2, category: RuleCategory::Overlay, version: 1 },
            Rule { id: 3, category: RuleCategory::Shortcut, version: 1 },
            Rule { id: 4, category: RuleCategory::Copy, version: 1 },
        ];
        Self {
            rules,
            whitelist: Whitelist::new(),
        }
    }

    /// 补录规则。
    pub fn add_rule(&mut self, r: Rule) {
        self.rules.push(r);
    }

    /// 四类覆盖。
    pub fn covers_all(&self) -> bool {
        RuleCategory::ALL
            .iter()
            .all(|c| self.rules.iter().any(|r| r.category == *c))
    }

    /// 白名单只读。
    pub fn whitelist(&self) -> &Whitelist {
        &self.whitelist
    }

    /// 白名单可变。
    pub fn whitelist_mut(&mut self) -> &mut Whitelist {
        &mut self.whitelist
    }

    /// 规则数。
    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }
}

// ---------------------------------------------------------------------------
// 六、判据
// ---------------------------------------------------------------------------

/// 词典引擎判据集。
pub fn run_rule_checks() -> crate::checks::CheckSet {
    let mut chk = crate::checks::CheckSet::new("svstar2-ver11d");

    // 1. 四类覆盖。
    let e = RuleEngine::standard();
    chk.add("6460-四类覆盖", e.covers_all() && e.rule_count() == 4, "");

    // 2. 单向同步。
    let mut s = RuleSync::new(3, 3);
    chk.add("6461-同步一致", s.aligned(), "");
    let mut s2 = RuleSync::new(5, 3);
    let ok_sync = s2.sync().is_ok() && s2.aligned();
    chk.add("6462-单向追上", ok_sync, "");

    // 3. 反向漂移拦截。
    let mut s3 = RuleSync::new(2, 5);
    chk.add("6463-反向拦截", s3.sync().err() == Some(RuleCode::SyncDrift), "");

    // 4. 白名单期限。
    let mut w = Whitelist::new();
    w.add(99);
    chk.add("6464-白名单在", w.holds(99), "");
    for _ in 0..31 {
        w.tick();
    }
    chk.add("6465-白名单过期", !w.holds(99) && w.is_empty(), "");

    // 5. 类名可达。
    chk.add(
        "6466-类名可达",
        RuleCategory::Shortcut.label() == "快捷键",
        "",
    );

    chk
}
