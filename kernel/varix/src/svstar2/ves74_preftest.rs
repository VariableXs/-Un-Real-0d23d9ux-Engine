//! VE-F3674 · 个性化测试（VE-S 域 · 批次 R04 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3674`
//!
//! **职责定位（锚点原文）**：个性化测试——
//! - **七类断言**：功能（四域正确）/ 性能（200ms+0.001ms 双线）/ 冲突（四源消解正确）/
//!   隐私（四权+最小化）/ 无障碍（覆盖禁令）/ 同步（范围+关闭）/ 失败（回滚）；
//! - **场景集**：单域/跨域/多源冲突/断网同步/覆盖攻击——五场景 × 四域 = 20 组合；
//! - **语料**：历史缺陷回放复述；
//! - **覆盖攻击**：无障碍覆盖注入断言（成功 = P0）。
//!
//! **判据（锚点原文）**：七类断言、覆盖攻击 P0、20 组合、双线性能、生成器单源、判据。
//!
//! **错误路径与降级矩阵**：
//! | 降级项 | 落位 |
//! |---|---|
//! | 覆盖攻击成功→P0（红线实测） | [`C74Code::COVER_ATTACK`] |
//! | 组合缺格→补跑 | [`C74Code::GRID_GAP`] |
//! | 生成器漂移→修复 | [`C74Code::GEN_DRIFT`] |
//! | 性能线破→立案 | [`C74Code::PERF_BREAK`] |
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3674 诊断码段（独占 0x7Axx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct C74Code(pub u16);

impl C74Code {
    /// 覆盖攻击成功（P0 红线实测）。
    pub const COVER_ATTACK: C74Code = C74Code(0x7A01);
    /// 场景组合缺格。
    pub const GRID_GAP: C74Code = C74Code(0x7A02);
    /// 生成器漂移。
    pub const GEN_DRIFT: C74Code = C74Code(0x7A03);
    /// 性能双线破。
    pub const PERF_BREAK: C74Code = C74Code(0x7A04);

    pub const fn value(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            C74Code::COVER_ATTACK => "无障碍覆盖攻击成功（P0 红线实测）。建议：覆盖禁令修复。",
            C74Code::GRID_GAP => "场景组合缺格。建议：补跑缺失组合。",
            C74Code::GEN_DRIFT => "生成器漂移。建议：按单源修复。",
            C74Code::PERF_BREAK => "性能双线破。建议：立案优化。",
            _ => "未知诊断码。",
        }
    }
}

// ── 七类断言 ────────────────────────────────────────────────────────

/// 断言类别（七类）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AssertClass {
    /// 功能（四域正确）。
    Function,
    /// 性能（200ms+0.001ms 双线）。
    Perf,
    /// 冲突（四源消解）。
    Conflict,
    /// 隐私（四权+最小化）。
    Privacy,
    /// 无障碍（覆盖禁令）。
    A11y,
    /// 同步（范围+关闭）。
    Sync,
    /// 失败（回滚）。
    Failure,
}

impl AssertClass {
    pub const ALL: [AssertClass; 7] = [
        AssertClass::Function,
        AssertClass::Perf,
        AssertClass::Conflict,
        AssertClass::Privacy,
        AssertClass::A11y,
        AssertClass::Sync,
        AssertClass::Failure,
    ];
}

/// 断言结果。
#[derive(Clone, Copy)]
pub struct AssertResult {
    pub class: AssertClass,
    pub ok: bool,
}

/// 七类生成器（单源复述）。
pub struct AssertGen {
    results: Vec<AssertResult>,
    drift_hits: u32,
    last_code: C74Code,
    has_error: bool,
}

impl AssertGen {
    pub fn new() -> Self {
        Self {
            results: Vec::new(),
            drift_hits: 0,
            last_code: C74Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C74Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    /// 生成一条断言结果。
    pub fn emit(&mut self, class: AssertClass, ok: bool) -> usize {
        self.results.push(AssertResult { class, ok });
        self.results.len()
    }

    /// 七类齐备。
    pub fn classes_covered(&self) -> bool {
        AssertClass::ALL.iter().all(|c| self.results.iter().any(|r| r.class == *c))
    }

    /// 全绿。
    pub fn all_green(&self) -> bool {
        self.results.iter().all(|r| r.ok)
    }

    /// 漂移上报（生成器输出与期望不符）。
    pub fn note_drift(&mut self) {
        self.drift_hits += 1;
        self.last_code = C74Code::GEN_DRIFT;
        self.has_error = true;
    }

    pub fn drift_hits(&self) -> u32 {
        self.drift_hits
    }

    pub fn count(&self) -> usize {
        self.results.len()
    }

    /// 按类计数。
    pub fn count_of(&self, class: AssertClass) -> usize {
        self.results.iter().filter(|r| r.class == class).count()
    }
}

// ── 性能双线 ────────────────────────────────────────────────────────

/// 双线预算（200ms 生效 + 0.001ms 读取，抽象 tick）。
pub struct PerfLines {
    /// 生效线（tick）。
    pub apply_line: u32,
    /// 读取线（tick）。
    pub read_line: u32,
    breaks: u32,
    last_code: C74Code,
    has_error: bool,
}

impl PerfLines {
    pub const fn standard() -> Self {
        Self {
            apply_line: 200,
            read_line: 1,
            breaks: 0,
            last_code: C74Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C74Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    /// 测双线。
    pub fn measure(&mut self, apply_tick: u32, read_tick: u32) -> Result<bool, C74Code> {
        if apply_tick <= self.apply_line && read_tick <= self.read_line {
            self.last_code = C74Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.breaks += 1;
            self.last_code = C74Code::PERF_BREAK;
            self.has_error = true;
            Err(C74Code::PERF_BREAK)
        }
    }

    pub fn breaks(&self) -> u32 {
        self.breaks
    }
}

// ── 场景组合网格 ────────────────────────────────────────────────────

/// 场景（五类）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Scene {
    /// 单域。
    SingleDomain,
    /// 跨域。
    CrossDomain,
    /// 多源冲突。
    MultiConflict,
    /// 断网同步。
    OfflineSync,
    /// 覆盖攻击。
    CoverAttack,
}

impl Scene {
    pub const ALL: [Scene; 5] = [
        Scene::SingleDomain,
        Scene::CrossDomain,
        Scene::MultiConflict,
        Scene::OfflineSync,
        Scene::CoverAttack,
    ];
}

/// 组合格（5×4=20）。
pub struct SceneGrid {
    /// (场景索引, 域索引) → 已跑。
    ran: Vec<(u8, u8)>,
    gap_hits: u32,
    last_code: C74Code,
    has_error: bool,
}

impl SceneGrid {
    pub fn new() -> Self {
        Self {
            ran: Vec::new(),
            gap_hits: 0,
            last_code: C74Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C74Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    /// 标记一格已跑。
    pub fn mark(&mut self, scene: u8, domain: u8) {
        if !self.ran.contains(&(scene, domain)) {
            self.ran.push((scene, domain));
        }
    }

    /// 20 格齐备。
    pub fn complete(&self) -> bool {
        self.ran.len() >= 20
    }

    /// 缺格审计。
    pub fn audit(&mut self) -> Result<bool, C74Code> {
        if self.complete() {
            self.last_code = C74Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.gap_hits += 1;
            self.last_code = C74Code::GRID_GAP;
            self.has_error = true;
            Err(C74Code::GRID_GAP)
        }
    }

    pub fn ran_count(&self) -> usize {
        self.ran.len()
    }

    pub fn gap_hits(&self) -> u32 {
        self.gap_hits
    }
}

// ── 覆盖攻击注入 ────────────────────────────────────────────────────

/// 覆盖攻击注入器（无障碍覆盖注入断言）。
pub struct CoverAttack {
    /// 攻击是否成功覆盖了无障碍项。
    succeeded: u32,
    blocked: u32,
    last_code: C74Code,
    has_error: bool,
}

impl CoverAttack {
    pub fn new() -> Self {
        Self {
            succeeded: 0,
            blocked: 0,
            last_code: C74Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C74Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    /// 注入一次覆盖攻击。
    /// `blocked` = 禁令矩阵是否拦下。
    pub fn inject(&mut self, blocked: bool) -> Result<bool, C74Code> {
        if blocked {
            self.blocked += 1;
            self.last_code = C74Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.succeeded += 1;
            self.last_code = C74Code::COVER_ATTACK;
            self.has_error = true;
            Err(C74Code::COVER_ATTACK)
        }
    }

    pub fn succeeded(&self) -> u32 {
        self.succeeded
    }

    pub fn blocked(&self) -> u32 {
        self.blocked
    }
}

// ── 语料回放 ────────────────────────────────────────────────────────

/// 历史缺陷语料条目。
#[derive(Clone, Copy)]
pub struct CorpusItem {
    pub id: u16,
    /// 是否回放通过。
    pub replayed_ok: bool,
}

/// 语料回放器（复述）。
pub struct CorpusReplay {
    items: Vec<CorpusItem>,
}

impl CorpusReplay {
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    pub fn add(&mut self, id: u16, replayed_ok: bool) {
        self.items.push(CorpusItem { id, replayed_ok });
    }

    /// 全部回放通过。
    pub fn all_replayed(&self) -> bool {
        !self.items.is_empty() && self.items.iter().all(|i| i.replayed_ok)
    }

    pub fn count(&self) -> usize {
        self.items.len()
    }

    pub fn failed_count(&self) -> usize {
        self.items.iter().filter(|i| !i.replayed_ok).count()
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves74_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves74_preftest");
    {
        let g = AssertGen::new();
        chk(&mut set, "C3674-01", g.count() == 0 && !g.classes_covered(), "err");
    }
    {
        let mut g = AssertGen::new();
        for c in AssertClass::ALL {
            g.emit(c, true);
        }
        chk(&mut set, "C3674-02", g.classes_covered() && g.all_green(), "err");
    }
    {
        let mut g = AssertGen::new();
        g.emit(AssertClass::Function, true);
        g.emit(AssertClass::Perf, false);
        chk(&mut set, "C3674-03", !g.all_green() && g.count_of(AssertClass::Perf) == 1, "err");
    }
    {
        let mut p = PerfLines::standard();
        let res = p.measure(100, 1);
        chk(&mut set, "C3674-04", res == Ok(true), "err");
    }
    {
        let mut p = PerfLines::standard();
        let res = p.measure(500, 1);
        chk(&mut set, "C3674-05", res.err() == Some(C74Code::PERF_BREAK), "err");
    }
    {
        let mut p = PerfLines::standard();
        let res = p.measure(100, 9);
        chk(&mut set, "C3674-06", res.err() == Some(C74Code::PERF_BREAK), "err");
    }
    {
        let mut g = SceneGrid::new();
        for s in 0..5u8 {
            for d in 0..4u8 {
                g.mark(s, d);
            }
        }
        chk(&mut set, "C3674-07", g.complete() && g.audit() == Ok(true), "err");
    }
    {
        let mut g = SceneGrid::new();
        g.mark(0, 0);
        let res = g.audit();
        chk(&mut set, "C3674-08", res.err() == Some(C74Code::GRID_GAP), "err");
    }
    {
        let mut a = CoverAttack::new();
        let res = a.inject(true);
        chk(&mut set, "C3674-09", res == Ok(true) && a.blocked() == 1, "err");
    }
    {
        let mut a = CoverAttack::new();
        let res = a.inject(false);
        chk(&mut set, "C3674-10", res.err() == Some(C74Code::COVER_ATTACK), "err");
    }
    {
        let mut c = CorpusReplay::new();
        c.add(1, true);
        c.add(2, true);
        chk(&mut set, "C3674-11", c.all_replayed() && c.count() == 2, "err");
    }
    {
        let mut c = CorpusReplay::new();
        c.add(1, true);
        c.add(2, false);
        chk(&mut set, "C3674-12", !c.all_replayed() && c.failed_count() == 1, "err");
    }
    set
}
