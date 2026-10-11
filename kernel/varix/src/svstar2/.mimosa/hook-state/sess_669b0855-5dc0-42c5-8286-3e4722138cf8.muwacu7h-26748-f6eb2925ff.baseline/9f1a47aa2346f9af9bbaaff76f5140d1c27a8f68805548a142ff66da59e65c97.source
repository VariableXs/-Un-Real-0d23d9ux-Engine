//! VE-F0402 · 语言规范文档体系（VE-C 域 · 着色器系统 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0402`
//!
//! **判据（锚点原文）**：三册分层、编号制、版本对齐、修订流程、判据。
//!
//! **职责定位（锚点原文）**：C 域语言规范文档体系——语法规范（文法产生式
//! 全集）、语义规范（类型与求值规则）、标准库规范（内建函数签名）三册分层；
//! 规范版本化与实现版本对齐声明；规范条目编号制（实现诊断可引用规范条款
//! 号）；变更走修订流程。
//!
//! **设计要点**：
//! - **三册分层**：语法册（VS-GR-）/ 语义册（VS-SE-）/ 标准库册（VS-SL-）
//!   三册各管一段，条目不得跨册混编号——分层是检索与引用的前提，混册
//!   等于没有分层；
//! - **编号制**：条目编号全局唯一，实现诊断可直接引用规范条款号（如
//!   "违反 VS-SE-07"）。编号冲突（同号两条/跨册撞号/格式非法）一律拒绝
//!   入册——错误码段先例：编号即主键，主键撞了检索即崩；
//! - **版本对齐**：规范版本与实现版本成对登记（`VersionAlignment`），对齐
//!   态显性（aligned 布尔 + 差异说明）——实现改了规范没升版（或反之）就
//!   是漂移，漂移必须可见，不许两处各说各话；
//! - **修订流程**：变更走修订记录（新增/修改/停用三类），修订必须公告
//!   （announced 标记）；发布门禁：存在未公告修订 → 拒绝发布。规范条目
//!   只增不改：修改 = 旧条目停用 + 新条目入册（编号不复用），语义修订
//!   溯源完整；
//! - **冲突显性二选一**：规范与实现冲突时，要么"以规范裁决"（实现改），
//!   要么"走修订改规范"——两条路都留裁决记录（谁裁的/裁了什么/依据），
//!   静默冲突（实现偏离规范没人管）是阻断级缺陷；
//! - **零静默**：所有拒绝与阻断产出结构化错误（现象/建议），检索未命中
//!   返回显性空结果而非 panic。
//!
//! **错误路径与降级矩阵**：规范与实现冲突→以规范裁决或修订（显性二选一）；
//! 编号冲突→拒绝发布；修订未公告→阻断。
//!
//! **跨批对接点**：上游 VE-B 移交包（能力位语境）；下游 F0403 起全编译器
//! 按规范实现；条款引用链对接 F0401 总纲（VS-G 条款）。
//!
//! 确定性：静态注册表 + 纯函数校验，无时钟无 IO。零外部依赖，只用 `alloc`
//! 与 `crate::checks`（自检侧）。

use crate::checks::CheckSet;

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、三册与条目（判据一：三册分层；判据二：编号制）
// ---------------------------------------------------------------------------

/// 规范册别（三册分层，锚点钦定）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SpecBook {
    /// 语法规范：文法产生式全集。
    Grammar,
    /// 语义规范：类型与求值规则。
    Semantics,
    /// 标准库规范：内建函数签名。
    StdLib,
}

impl SpecBook {
    /// 全部三册（顺序固定：文法 → 语义 → 标准库）。
    pub const ALL: [SpecBook; 3] = [SpecBook::Grammar, SpecBook::Semantics, SpecBook::StdLib];

    /// 册名（人话）。
    pub fn name(self) -> &'static str {
        match self {
            SpecBook::Grammar => "语法规范",
            SpecBook::Semantics => "语义规范",
            SpecBook::StdLib => "标准库规范",
        }
    }

    /// 本册编号前缀（编号制的册维度——混册编号在此被结构性排除）。
    pub fn id_prefix(self) -> &'static str {
        match self {
            SpecBook::Grammar => "VS-GR-",
            SpecBook::Semantics => "VS-SE-",
            SpecBook::StdLib => "VS-SL-",
        }
    }
}

/// 规范条目：一册之内的一条可引用规范条款。
#[derive(Clone, Debug)]
pub struct SpecEntry {
    /// 条款编号（册前缀 + 两位以上序号，如 `VS-GR-01`，全局唯一）。
    pub id: String,
    /// 条款标题。
    pub title: String,
    /// 条款正文摘要（实现诊断引用的锚文本，必须自足）。
    pub digest: String,
    /// 入册时的规范版本号（溯源：这条内容是哪个版本定的）。
    pub registered_in: String,
}

impl SpecEntry {
    /// 注册校验：编号格式（前缀对册 + 序号纯数字）、标题与摘要非空。
    /// 违反即拒——空条款与错位编号是占位符，占位符不许入册。
    pub fn new(
        id: &str,
        title: &str,
        digest: &str,
        registered_in: &str,
    ) -> Result<Self, SpecError> {
        // 形如 VS-GR-01：剥前缀 "VS-" 后按 '-' 拆出册代号与数字序号。
        let parts = id
            .strip_prefix("VS-")
            .and_then(|s| s.split_once('-'));
        let (book_code, rest) = match parts {
            Some(v) => v,
            None => {
                return Err(SpecError::BadEntryId {
                    id: id.to_string(),
                    suggestion: "编号必须形如 VS-GR-01 / VS-SE-01 / VS-SL-01".to_string(),
                })
            }
        };
        let expected = match book_code {
            "GR" => SpecBook::Grammar.id_prefix(),
            "SE" => SpecBook::Semantics.id_prefix(),
            "SL" => SpecBook::StdLib.id_prefix(),
            _ => {
                return Err(SpecError::BadEntryId {
                    id: id.to_string(),
                    suggestion: "册代号只认 GR（语法）/ SE（语义）/ SL（标准库）".to_string(),
                })
            }
        };
        if !id.starts_with(expected) || rest.is_empty() || !rest.chars().all(|c| c.is_ascii_digit())
        {
            return Err(SpecError::BadEntryId {
                id: id.to_string(),
                suggestion: format!("序号必须是纯数字且前缀与 {} 册一致", expected),
            });
        }
        if title.is_empty() || digest.is_empty() || registered_in.is_empty() {
            return Err(SpecError::EmptyEntry {
                id: id.to_string(),
                suggestion: "标题/摘要/入册版本都必填——占位条目不许入册".to_string(),
            });
        }
        Ok(Self {
            id: id.to_string(),
            title: title.to_string(),
            digest: digest.to_string(),
            registered_in: registered_in.to_string(),
        })
    }
}

/// 规范册：一册的条目集合 + 册版本。
pub struct SpecVolume {
    /// 册别。
    pub book: SpecBook,
    /// 条目集（按注册顺序；只增不改）。
    pub entries: Vec<SpecEntry>,
    /// 册版本（随修订升版）。
    pub version: String,
}

impl SpecVolume {
    /// 建空册（版本必填——没有版本的册无法做对齐声明）。
    pub fn new(book: SpecBook, version: &str) -> Result<Self, SpecError> {
        if version.is_empty() {
            return Err(SpecError::EmptyVersion {
                book: book.name().to_string(),
                suggestion: "册版本必填（如 spec-c-grammar-1.0）".to_string(),
            });
        }
        Ok(Self { book, entries: Vec::new(), version: version.to_string() })
    }

    /// 入册（编号唯一性在本册内校验；跨册唯一性由 SpecRegistry 负责）。
    pub fn add(&mut self, e: SpecEntry) -> Result<(), SpecError> {
        if !e.id.starts_with(self.book.id_prefix()) {
            return Err(SpecError::CrossBookId {
                id: e.id.clone(),
                book: self.book.name().to_string(),
                suggestion: format!("{} 册只收 {} 前缀条目", self.book.name(), self.book.id_prefix()),
            });
        }
        if self.entries.iter().any(|x| x.id == e.id) {
            return Err(SpecError::DuplicateEntryId {
                id: e.id.clone(),
                suggestion: "编号冲突即拒绝发布；换新序号，语义修订走修订流程".to_string(),
            });
        }
        self.entries.push(e);
        Ok(())
    }

    /// 按编号取条目（检索面：实现诊断引用条款号即查这里）。
    pub fn find(&self, id: &str) -> Option<&SpecEntry> {
        self.entries.iter().find(|e| e.id == id)
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 二、版本对齐（判据三：版本对齐）
// ---------------------------------------------------------------------------

/// 规范版本 ↔ 实现版本的对齐声明（成对登记，漂移显性）。
#[derive(Clone, Debug)]
pub struct VersionAlignment {
    /// 规范版本号（三册共同版本，册内另有细分）。
    pub spec_version: String,
    /// 实现版本号（编译器实现的版本）。
    pub impl_version: String,
    /// 对齐态：true = 实现声明按此规范版实现。
    pub aligned: bool,
    /// 差异说明（aligned=false 时必填——差异说不清等于没审计）。
    pub delta_note: String,
}

impl VersionAlignment {
    /// 登记对齐声明（不对齐时差异说明必填——零静默）。
    pub fn new(
        spec_version: &str,
        impl_version: &str,
        aligned: bool,
        delta_note: &str,
    ) -> Result<Self, SpecError> {
        if spec_version.is_empty() || impl_version.is_empty() {
            return Err(SpecError::IncompleteAlignment {
                missing: "规范版本或实现版本为空".to_string(),
                suggestion: "对齐声明必须成对登记两个版本号".to_string(),
            });
        }
        if !aligned && delta_note.is_empty() {
            return Err(SpecError::IncompleteAlignment {
                missing: "未对齐但差异说明为空".to_string(),
                suggestion: "漂移必须写明差异与收敛计划，不许静默不对齐".to_string(),
            });
        }
        Ok(Self {
            spec_version: spec_version.to_string(),
            impl_version: impl_version.to_string(),
            aligned,
            delta_note: delta_note.to_string(),
        })
    }

    /// 漂移检测：实现版本变化后未重新登记对齐 → 漂移。
    /// （比较当前登记的实现版本与运行期上报的实现版本。）
    pub fn drifted(&self, current_impl_version: &str) -> bool {
        self.impl_version != current_impl_version
    }
}

// ---------------------------------------------------------------------------
// 三、修订流程与发布门禁（判据四：修订流程）
// ---------------------------------------------------------------------------

/// 修订类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RevisionKind {
    /// 新增条目。
    Added,
    /// 修改条目（旧条目停用 + 新条目入册，编号不复用）。
    Amended,
    /// 停用条目（保留编号占位与停用原因，防止复用编号造成溯源断裂）。
    Retired,
}

/// 修订记录：一次规范变更的完整留痕。
pub struct Revision {
    /// 修订对象条款号。
    pub entry_id: String,
    /// 修订类别。
    pub kind: RevisionKind,
    /// 修订原因（为什么改——修订没理由等于没审计）。
    pub reason: String,
    /// 是否已公告（修订未公告 → 发布阻断）。
    pub announced: bool,
}

/// 冲突裁决：规范与实现冲突时的显性二选一。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Adjudication {
    /// 以规范裁决：实现改。
    SpecWins,
    /// 走修订改规范：规范改（改完升版重新对齐）。
    ReviseSpec,
}

/// 裁决记录（裁决三要素：裁了哪条/怎么裁/依据）。
pub struct AdjudicationRecord {
    /// 条款号。
    pub entry_id: String,
    /// 裁决方向。
    pub resolution: Adjudication,
    /// 依据（谁裁的与凭什么——静默冲突的反面）。
    pub basis: String,
}

/// 冲突上报：实现侧发现与规范不符时提交（未裁决前发布阻断）。
pub struct PendingConflict {
    /// 条款号。
    pub entry_id: String,
    /// 冲突现象。
    pub what: String,
}

/// 规范注册表：三册 + 版本对齐 + 修订账 + 冲突账 + 发布门禁。
pub struct SpecRegistry {
    /// 语法册。
    pub grammar: SpecVolume,
    /// 语义册。
    pub semantics: SpecVolume,
    /// 标准库册。
    pub stdlib: SpecVolume,
    /// 版本对齐声明。
    pub alignment: Option<VersionAlignment>,
    /// 修订账（只增不减）。
    pub revisions: Vec<Revision>,
    /// 待裁决冲突（发布前必须清空或裁决）。
    pub pending_conflicts: Vec<PendingConflict>,
    /// 裁决账（只增不减）。
    pub adjudications: Vec<AdjudicationRecord>,
    /// 已停用条目（编号保留，禁止复用）。
    pub retired_ids: Vec<String>,
}

impl SpecRegistry {
    /// 建三册注册表（三册版本各自登记）。
    pub fn new(
        grammar_version: &str,
        semantics_version: &str,
        stdlib_version: &str,
    ) -> Result<Self, SpecError> {
        Ok(Self {
            grammar: SpecVolume::new(SpecBook::Grammar, grammar_version)?,
            semantics: SpecVolume::new(SpecBook::Semantics, semantics_version)?,
            stdlib: SpecVolume::new(SpecBook::StdLib, stdlib_version)?,
            alignment: None,
            revisions: Vec::new(),
            pending_conflicts: Vec::new(),
            adjudications: Vec::new(),
            retired_ids: Vec::new(),
        })
    }

    /// 全册编号全局唯一性核查（跨册撞号也拒绝——编号制的主键纪律）。
    pub fn check_id_uniqueness(&self) -> Result<(), SpecError> {
        let mut seen: Vec<&str> = Vec::new();
        for e in self
            .grammar
            .entries
            .iter()
            .chain(self.semantics.entries.iter())
            .chain(self.stdlib.entries.iter())
        {
            if seen.contains(&e.id.as_str()) {
                return Err(SpecError::DuplicateEntryId {
                    id: e.id.clone(),
                    suggestion: "跨册撞号也是编号冲突——先修号再发布".to_string(),
                });
            }
            seen.push(&e.id);
        }
        Ok(())
    }

    /// 上报冲突（实现侧发现与规范不符——未裁决前发布阻断）。
    pub fn report_conflict(&mut self, entry_id: &str, what: &str) {
        self.pending_conflicts.push(PendingConflict {
            entry_id: entry_id.to_string(),
            what: what.to_string(),
        });
    }

    /// 裁决冲突（显性二选一：以规范裁决 或 走修订改规范）。
    pub fn adjudicate(
        &mut self,
        entry_id: &str,
        resolution: Adjudication,
        basis: &str,
    ) -> Result<(), SpecError> {
        if basis.is_empty() {
            return Err(SpecError::AdjudicationNoBasis {
                entry: entry_id.to_string(),
                suggestion: "裁决必须写明依据（谁裁的/凭什么）——无据裁决是静默冲突的变体".to_string(),
            });
        }
        if !self
            .pending_conflicts
            .iter()
            .any(|c| c.entry_id == entry_id)
        {
            return Err(SpecError::NoSuchConflict {
                entry: entry_id.to_string(),
                suggestion: "只裁决已上报的冲突——凭空裁决说明账目错位".to_string(),
            });
        }
        self.pending_conflicts.retain(|c| c.entry_id != entry_id);
        self.adjudications.push(AdjudicationRecord {
            entry_id: entry_id.to_string(),
            resolution,
            basis: basis.to_string(),
        });
        Ok(())
    }

    /// 登记修订（新增/修改/停用三类；修订未公告可入账但阻断发布）。
    pub fn add_revision(
        &mut self,
        entry_id: &str,
        kind: RevisionKind,
        reason: &str,
        announced: bool,
    ) -> Result<(), SpecError> {
        if reason.is_empty() {
            return Err(SpecError::RevisionNoReason {
                entry: entry_id.to_string(),
                suggestion: "修订必须带原因——没理由的变更是事故不是流程".to_string(),
            });
        }
        self.revisions.push(Revision {
            entry_id: entry_id.to_string(),
            kind,
            reason: reason.to_string(),
            announced,
        });
        Ok(())
    }

    /// 公告修订（把未公告的修订置为已公告——公告是发布的前置）。
    pub fn announce_revisions(&mut self) {
        for r in &mut self.revisions {
            r.announced = true;
        }
    }

    /// 停用条目（编号保留不复用——溯源完整性的结构性保障）。
    pub fn retire(&mut self, entry_id: &str, reason: &str) -> Result<(), SpecError> {
        if reason.is_empty() {
            return Err(SpecError::RevisionNoReason {
                entry: entry_id.to_string(),
                suggestion: "停用必须带原因".to_string(),
            });
        }
        let exists = self
            .grammar
            .entries
            .iter()
            .chain(self.semantics.entries.iter())
            .chain(self.stdlib.entries.iter())
            .any(|e| e.id == entry_id);
        if !exists {
            return Err(SpecError::NoSuchEntry {
                id: entry_id.to_string(),
                suggestion: "停用对象必须是在册条目".to_string(),
            });
        }
        if !self.retired_ids.iter().any(|id| id == entry_id) {
            self.retired_ids.push(entry_id.to_string());
        }
        Ok(())
    }

    /// 编号复用防护：停用过的编号不许再入册。
    pub fn check_retired_reuse(&self, id: &str) -> Result<(), SpecError> {
        if self.retired_ids.iter().any(|r| r == id) {
            return Err(SpecError::RetiredIdReuse {
                id: id.to_string(),
                suggestion: "停用编号永久保留不复用——复用会撕裂溯源链".to_string(),
            });
        }
        Ok(())
    }

    /// 全局检索：跨三册按编号 O(1) 语义查找（线性扫描，量级 ≤ 数百条）。
    /// 未命中返回显性空结果（不 panic、不瞎猜）。
    pub fn find(&self, id: &str) -> Option<&SpecEntry> {
        self.grammar
            .find(id)
            .or_else(|| self.semantics.find(id))
            .or_else(|| self.stdlib.find(id))
    }

    /// 发布门禁：五道闸全过才可发布。
    ///
    /// 闸序：编号全局唯一 → 停用编号未复用 → 无未公告修订 → 无待裁决冲突
    /// → 版本对齐已登记且对齐。任一闸红即拒绝，拒绝带具体阻断原因。
    pub fn publish_gate(&self) -> Result<(), SpecError> {
        self.check_id_uniqueness()?;
        // 停用编号未复用闸：在册条目不得落在停用清单里。
        for e in self
            .grammar
            .entries
            .iter()
            .chain(self.semantics.entries.iter())
            .chain(self.stdlib.entries.iter())
        {
            if self.retired_ids.iter().any(|r| r == &e.id) {
                return Err(SpecError::PublishBlocked {
                    stage: "停用编号复用".to_string(),
                    what: format!("条款 {} 已停用却又在册——溯源链断裂", e.id),
                    suggestion: "停用编号永久保留不复用，新内容换新序号".to_string(),
                });
            }
        }
        let aligned_ok = self.alignment.as_ref().map(|a| a.aligned).unwrap_or(false);
        if !aligned_ok {
            let what = match &self.alignment {
                Some(a) => format!(
                    "对齐声明为未对齐（规范 {} vs 实现 {}）：{}",
                    a.spec_version, a.impl_version, a.delta_note
                ),
                None => "对齐声明未登记".to_string(),
            };
            return Err(SpecError::PublishBlocked {
                stage: "版本对齐".to_string(),
                what,
                suggestion: "发布前必须成对登记规范版本与实现版本且为对齐态；漂移先收敛再升版".to_string(),
            });
        }
        if let Some(r) = self.revisions.iter().find(|r| !r.announced) {
            return Err(SpecError::PublishBlocked {
                stage: "修订公告".to_string(),
                what: format!("条款 {} 的修订未公告", r.entry_id),
                suggestion: "先公告全部修订再发布（修订未公告→阻断）".to_string(),
            });
        }
        if let Some(c) = self.pending_conflicts.first() {
            return Err(SpecError::PublishBlocked {
                stage: "冲突裁决".to_string(),
                what: format!("条款 {} 存在未裁决冲突：{}", c.entry_id, c.what),
                suggestion: "裁决只有两条路：以规范裁决（实现改）或走修订改规范——二选一必须留痕".to_string(),
            });
        }
        Ok(())
    }

    /// 三册条目总数。
    pub fn total_entries(&self) -> usize {
        self.grammar.len() + self.semantics.len() + self.stdlib.len()
    }
}

// ---------------------------------------------------------------------------
// 四、错误类型（零静默：每类拒绝带建议）
// ---------------------------------------------------------------------------

/// 规范体系错误（现象 + 建议，五要素精简为三段人话）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SpecError {
    /// 条款编号格式非法。
    BadEntryId { id: String, suggestion: String },
    /// 条目必填字段为空（占位条目）。
    EmptyEntry { id: String, suggestion: String },
    /// 册版本为空。
    EmptyVersion { book: String, suggestion: String },
    /// 条目跨册（编号前缀与册不符）。
    CrossBookId { id: String, book: String, suggestion: String },
    /// 条款编号冲突（册内或跨册）。
    DuplicateEntryId { id: String, suggestion: String },
    /// 对齐声明要素不全。
    IncompleteAlignment { missing: String, suggestion: String },
    /// 修订无原因。
    RevisionNoReason { entry: String, suggestion: String },
    /// 裁决无依据。
    AdjudicationNoBasis { entry: String, suggestion: String },
    /// 裁决对象不存在。
    NoSuchConflict { entry: String, suggestion: String },
    /// 停用对象不存在。
    NoSuchEntry { id: String, suggestion: String },
    /// 停用编号复用。
    RetiredIdReuse { id: String, suggestion: String },
    /// 发布阻断（阶段/现象/建议）。
    PublishBlocked { stage: String, what: String, suggestion: String },
}

impl SpecError {
    /// 人话呈现（发生了什么 + 怎么修）。
    pub fn describe(&self) -> String {
        match self {
            SpecError::BadEntryId { id, suggestion } => format!("条款编号非法：{id}。建议：{suggestion}"),
            SpecError::EmptyEntry { id, suggestion } => format!("条目 {id} 必填字段为空。建议：{suggestion}"),
            SpecError::EmptyVersion { book, suggestion } => format!("{book} 版本为空。建议：{suggestion}"),
            SpecError::CrossBookId { id, book, suggestion } => {
                format!("条目 {id} 与 {book} 册前缀不符。建议：{suggestion}")
            }
            SpecError::DuplicateEntryId { id, suggestion } => {
                format!("编号冲突：{id}。建议：{suggestion}")
            }
            SpecError::IncompleteAlignment { missing, suggestion } => {
                format!("对齐声明不全：{missing}。建议：{suggestion}")
            }
            SpecError::RevisionNoReason { entry, suggestion } => {
                format!("条款 {entry} 修订无原因。建议：{suggestion}")
            }
            SpecError::AdjudicationNoBasis { entry, suggestion } => {
                format!("条款 {entry} 裁决无依据。建议：{suggestion}")
            }
            SpecError::NoSuchConflict { entry, suggestion } => {
                format!("条款 {entry} 没有在案的冲突。建议：{suggestion}")
            }
            SpecError::NoSuchEntry { id, suggestion } => {
                format!("条目 {id} 不存在。建议：{suggestion}")
            }
            SpecError::RetiredIdReuse { id, suggestion } => {
                format!("停用编号 {id} 试图复用。建议：{suggestion}")
            }
            SpecError::PublishBlocked { stage, what, suggestion } => {
                format!("发布阻断（{stage}）：{what}。建议：{suggestion}")
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 五、标准注册表（预置条目，下游 F0403 起按此引用）
// ---------------------------------------------------------------------------

/// 标准三册预置（覆盖锚点点名面的种子条目：文法/语义/标准库各成段）。
pub fn standard_registry() -> Result<SpecRegistry, SpecError> {
    let mut r = SpecRegistry::new("spec-grammar-1.0", "spec-semantics-1.0", "spec-stdlib-1.0")?;
    let grammar = [
        ("VS-GR-01", "翻译单元", "源文件即翻译单元：全局声明（绑定/函数/类型别名）与模块属性构成，缺分号或括号失衡是语法错误"),
        ("VS-GR-02", "产生式：函数", "函数 = 签名 + 块体；签名含可见性/名称/参数表/返回类型/能力注记；块体为语句序列"),
        ("VS-GR-03", "产生式：语句", "语句分声明/表达式/控制流/分号空语句四类；控制流只认 if/loop/break/continue/return（定点循环界原则落点）"),
        ("VS-GR-04", "产生式：类型", "类型产生式覆盖标量/向量/矩阵/数组/结构体/绑定句柄；数组维数必须是常量表达式"),
    ];
    for (id, t, d) in grammar {
        r.grammar.add(SpecEntry::new(id, t, d, "spec-grammar-1.0")?)?;
    }
    let semantics = [
        ("VS-SE-01", "类型系统", "静态强类型：每个表达式有唯一编译期类型，隐式转换白名单外的转换必须显式"),
        ("VS-SE-02", "求值规则", "表达式求值顺序默认从左到右；浮点求值模式由模块注记显式声明（VS-G-02 联动）"),
        ("VS-SE-03", "绑定语义", "资源绑定在编译期与管线索引双向校验，不匹配即报错（VS-G-01 落点）"),
        ("VS-SE-04", "越界语义", "数组/向量越界是诊断报错并钳制读值，禁止未定义行为（VS-C-03 落点）"),
    ];
    for (id, t, d) in semantics {
        r.semantics.add(SpecEntry::new(id, t, d, "spec-semantics-1.0")?)?;
    }
    let stdlib = [
        ("VS-SL-01", "算术内建", "算术内建按 GLSL 语义全集对拍收编，跨后端确定性分级标注（VS-G-07 落点）"),
        ("VS-SL-02", "几何内建", "几何内建（点积/叉积/归一化等）签名与精度语义在册，直译类映射"),
        ("VS-SL-03", "采样内建", "纹理采样内建与采样器状态语义对齐（VE-F0018 采样器库同源）"),
    ];
    for (id, t, d) in stdlib {
        r.stdlib.add(SpecEntry::new(id, t, d, "spec-stdlib-1.0")?)?;
    }
    r.check_id_uniqueness()?;
    Ok(r)
}

/// 纯功能行数自证（正式门禁见 `vec02_checks.rs`）。
pub fn registry_smoke() -> usize {
    match standard_registry() {
        Ok(r) => r.total_entries(),
        Err(_) => 0,
    }
}

/// VE-F0402 域自检（判据逐条对应，见 `vec02_checks.rs`）。
pub fn run_vec02_checks() -> CheckSet {
    super::vec02_checks::run_vec02_checks()
}
