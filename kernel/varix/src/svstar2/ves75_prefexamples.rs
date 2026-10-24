//! VE-F3675 · 个性化示例（VE-S 域 · 批次 R04 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3675`
//!
//! **职责定位（锚点原文）**：个性化示例——
//! - **示例库 10 个**：预设创建 2 / 场景规则 2 / 同步配置 2 / 导出迁移 2 / 推荐理解 2——五主题；
//! - **三段文档 + 反模式对照**：每主题一反例（默认全同步 / 静默覆盖 / 画像上云——复述诊断）；
//! - **示例即测试**复述；完整工程复述；任务索引复用（复述视角红线）。
//!
//! **判据（锚点原文）**：10 示例、任务索引、完整工程、视角复用、家族同构、判据。
//!
//! **错误路径与降级矩阵**：
//! | 降级项 | 落位 |
//! |---|---|
//! | 示例失效→CI（复述） | [`C75Code::SAMPLE_STALE`] |
//! | 反例误用→标注（复述） | [`C75Code::ANTI_MISUSE`] |
//! | 工程缺件→断言（复述） | [`C75Code::PROJECT_GAP`] |
//! | 漂移→对账（复述） | [`C75Code::INDEX_DRIFT`] |
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3675 诊断码段（独占 0x7Bxx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct C75Code(pub u16);

impl C75Code {
    /// 示例失效（CI 复述）。
    pub const SAMPLE_STALE: C75Code = C75Code(0x7B01);
    /// 反例误用。
    pub const ANTI_MISUSE: C75Code = C75Code(0x7B02);
    /// 完整工程缺件。
    pub const PROJECT_GAP: C75Code = C75Code(0x7B03);
    /// 任务索引漂移。
    pub const INDEX_DRIFT: C75Code = C75Code(0x7B04);

    pub const fn value(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            C75Code::SAMPLE_STALE => "示例失效（CI 复述）。建议：修复示例。",
            C75Code::ANTI_MISUSE => "反例误用。建议：反模式标注。",
            C75Code::PROJECT_GAP => "完整工程缺件。建议：补齐断言。",
            C75Code::INDEX_DRIFT => "任务索引漂移。建议：对账修正。",
            _ => "未知诊断码。",
        }
    }
}

// ── 示例主题 ────────────────────────────────────────────────────────

/// 五主题。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Topic {
    /// 预设创建。
    Preset,
    /// 场景规则。
    Scene,
    /// 同步配置。
    Sync,
    /// 导出迁移。
    Export,
    /// 推荐理解。
    Recommend,
}

impl Topic {
    pub const ALL: [Topic; 5] = [
        Topic::Preset,
        Topic::Scene,
        Topic::Sync,
        Topic::Export,
        Topic::Recommend,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Topic::Preset => "预设创建",
            Topic::Scene => "场景规则",
            Topic::Sync => "同步配置",
            Topic::Export => "导出迁移",
            Topic::Recommend => "推荐理解",
        }
    }
}

/// 示例条目（三段文档 + 反例标注）。
#[derive(Clone, Copy)]
pub struct Sample {
    pub id: u16,
    pub topic: Topic,
    /// 是否带反模式对照。
    pub has_anti: bool,
    /// 是否通过 CI（示例即测试）。
    pub ci_ok: bool,
}

// ── 示例库 ──────────────────────────────────────────────────────────

/// 个性化示例库（10 项，每主题 2）。
pub struct SampleLib {
    samples: Vec<Sample>,
    anti_misuse: u32,
    stale_hits: u32,
    last_code: C75Code,
    has_error: bool,
}

impl SampleLib {
    pub fn new() -> Self {
        Self {
            samples: Vec::new(),
            anti_misuse: 0,
            stale_hits: 0,
            last_code: C75Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C75Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    /// 注册一条示例。
    pub fn register(&mut self, s: Sample) -> usize {
        self.samples.push(s);
        self.samples.len()
    }

    /// 10 项齐备且每主题 2。
    pub fn lib_complete(&self) -> bool {
        self.samples.len() == 10
            && Topic::ALL
                .iter()
                .all(|t| self.samples.iter().filter(|s| s.topic == *t).count() == 2)
    }

    /// CI 复述：全部示例 ci_ok。
    pub fn audit_ci(&mut self) -> Result<bool, C75Code> {
        if self.samples.iter().all(|s| s.ci_ok) {
            self.last_code = C75Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.stale_hits += 1;
            self.last_code = C75Code::SAMPLE_STALE;
            self.has_error = true;
            Err(C75Code::SAMPLE_STALE)
        }
    }

    /// 反例标注齐备：每主题至少一条带反例。
    pub fn anti_tagged(&self) -> bool {
        Topic::ALL
            .iter()
            .all(|t| self.samples.iter().any(|s| s.topic == *t && s.has_anti))
    }

    /// 反例误用上报。
    pub fn note_anti_misuse(&mut self) {
        self.anti_misuse += 1;
        self.last_code = C75Code::ANTI_MISUSE;
        self.has_error = true;
    }

    pub fn sample_count(&self) -> usize {
        self.samples.len()
    }

    pub fn stale_hits(&self) -> u32 {
        self.stale_hits
    }

    pub fn anti_misuse(&self) -> u32 {
        self.anti_misuse
    }

    /// 按主题取示例数。
    pub fn count_of(&self, topic: Topic) -> usize {
        self.samples.iter().filter(|s| s.topic == topic).count()
    }
}

// ── 完整工程 ────────────────────────────────────────────────────────

/// 完整工程件（复述）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProjectPart {
    /// 源码。
    Source,
    /// 构建脚本。
    Build,
    /// 测试。
    Test,
    /// 文档。
    Docs,
}

impl ProjectPart {
    pub const ALL: [ProjectPart; 4] = [
        ProjectPart::Source,
        ProjectPart::Build,
        ProjectPart::Test,
        ProjectPart::Docs,
    ];
}

/// 完整工程断言器。
pub struct ProjectCompleteness {
    present: Vec<ProjectPart>,
    gap_hits: u32,
    last_code: C75Code,
    has_error: bool,
}

impl ProjectCompleteness {
    pub fn new() -> Self {
        Self {
            present: Vec::new(),
            gap_hits: 0,
            last_code: C75Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C75Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    pub fn add(&mut self, part: ProjectPart) {
        if !self.present.contains(&part) {
            self.present.push(part);
        }
    }

    /// 四件齐备。
    pub fn complete(&self) -> bool {
        ProjectPart::ALL.iter().all(|p| self.present.contains(p))
    }

    /// 缺件审计。
    pub fn audit(&mut self) -> Result<bool, C75Code> {
        if self.complete() {
            self.last_code = C75Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.gap_hits += 1;
            self.last_code = C75Code::PROJECT_GAP;
            self.has_error = true;
            Err(C75Code::PROJECT_GAP)
        }
    }

    pub fn present_count(&self) -> usize {
        self.present.len()
    }

    pub fn gap_hits(&self) -> u32 {
        self.gap_hits
    }
}

// ── 任务索引 ────────────────────────────────────────────────────────

/// 索引项（视角复用红线：每示例挂任务号）。
#[derive(Clone, Copy)]
pub struct IndexEntry {
    pub sample_id: u16,
    pub task_id: u16,
}

/// 任务索引（复用）。
pub struct TaskIndex {
    entries: Vec<IndexEntry>,
    drift_hits: u32,
    last_code: C75Code,
    has_error: bool,
}

impl TaskIndex {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            drift_hits: 0,
            last_code: C75Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C75Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    pub fn link(&mut self, sample_id: u16, task_id: u16) -> usize {
        self.entries.push(IndexEntry { sample_id, task_id });
        self.entries.len()
    }

    /// 对账：索引与示例库数量一致（复述）。
    pub fn reconcile(&mut self, sample_count: usize) -> Result<bool, C75Code> {
        if self.entries.len() == sample_count {
            self.last_code = C75Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.drift_hits += 1;
            self.last_code = C75Code::INDEX_DRIFT;
            self.has_error = true;
            Err(C75Code::INDEX_DRIFT)
        }
    }

    pub fn lookup(&self, sample_id: u16) -> Option<u16> {
        self.entries
            .iter()
            .find(|e| e.sample_id == sample_id)
            .map(|e| e.task_id)
    }

    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    pub fn drift_hits(&self) -> u32 {
        self.drift_hits
    }
}

// ── 家族同构 ────────────────────────────────────────────────────────

/// 家族同构检查（复述）。
pub struct FamilyIsomorphism {
    /// 家族成员数（十九口径）。
    members: u8,
}

impl FamilyIsomorphism {
    pub fn new(members: u8) -> Self {
        Self { members }
    }

    /// 同构断言：成员数在家族口径内。
    pub fn assert_iso(&self) -> bool {
        self.members > 0 && self.members <= 32
    }

    pub fn members(&self) -> u8 {
        self.members
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves75_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves75_prefexamples");
    {
        let lib = SampleLib::new();
        chk(&mut set, "C3675-01", lib.sample_count() == 0 && !lib.lib_complete(), "err");
    }
    {
        let mut lib = SampleLib::new();
        let mut id = 1u16;
        for t in Topic::ALL {
            for _ in 0..2 {
                lib.register(Sample { id, topic: t, has_anti: true, ci_ok: true });
                id += 1;
            }
        }
        chk(&mut set, "C3675-02", lib.lib_complete() && lib.anti_tagged(), "err");
    }
    {
        let mut lib = SampleLib::new();
        for i in 0..2 {
            lib.register(Sample { id: i + 1, topic: Topic::Preset, has_anti: false, ci_ok: true });
        }
        chk(&mut set, "C3675-03", !lib.lib_complete() && lib.count_of(Topic::Preset) == 2, "err");
    }
    {
        let mut lib = SampleLib::new();
        lib.register(Sample { id: 1, topic: Topic::Sync, has_anti: true, ci_ok: false });
        let res = lib.audit_ci();
        chk(&mut set, "C3675-04", res.err() == Some(C75Code::SAMPLE_STALE), "err");
    }
    {
        let mut lib = SampleLib::new();
        lib.note_anti_misuse();
        chk(&mut set, "C3675-05", lib.anti_misuse() == 1 && lib.has_error(), "err");
    }
    {
        let mut p = ProjectCompleteness::new();
        p.add(ProjectPart::Source);
        p.add(ProjectPart::Build);
        p.add(ProjectPart::Test);
        p.add(ProjectPart::Docs);
        chk(&mut set, "C3675-06", p.complete() && p.audit() == Ok(true), "err");
    }
    {
        let mut p = ProjectCompleteness::new();
        p.add(ProjectPart::Source);
        let res = p.audit();
        chk(&mut set, "C3675-07", res.err() == Some(C75Code::PROJECT_GAP), "err");
    }
    {
        let mut idx = TaskIndex::new();
        idx.link(1, 100);
        idx.link(2, 101);
        let res = idx.reconcile(2);
        chk(&mut set, "C3675-08", res == Ok(true) && idx.lookup(2) == Some(101), "err");
    }
    {
        let mut idx = TaskIndex::new();
        idx.link(1, 100);
        let res = idx.reconcile(3);
        chk(&mut set, "C3675-09", res.err() == Some(C75Code::INDEX_DRIFT), "err");
    }
    {
        let f = FamilyIsomorphism::new(19);
        chk(&mut set, "C3675-10", f.assert_iso() && f.members() == 19, "err");
    }
    {
        let mut lib = SampleLib::new();
        let mut id = 1u16;
        for t in Topic::ALL {
            for k in 0..2 {
                lib.register(Sample {
                    id,
                    topic: t,
                    has_anti: k == 0,
                    ci_ok: true,
                });
                id += 1;
            }
        }
        chk(&mut set, "C3675-11", lib.lib_complete() && lib.audit_ci() == Ok(true), "err");
    }
    {
        let name_ok = Topic::ALL.iter().all(|t| !t.name().is_empty());
        chk(&mut set, "C3675-12", name_ok && Topic::ALL.len() == 5, "err");
    }
    set
}
