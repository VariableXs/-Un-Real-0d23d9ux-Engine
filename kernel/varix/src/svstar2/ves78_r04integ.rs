//! VE-F3678 · R04 联调（VE-S 域 · 批次 R04 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3678`
//!
//! **职责定位（锚点原文）**：R04 联调——
//! - **组合矩阵**：四域 × 五场景（变更/预设/同步/冲突/导出）× 三设备档 = 60 格分层抽样
//!   （覆盖底线复述）；
//! - **真实剧本**：20 分钟个性化剧本（调外观→场景规则→换设备同步→导出迁移混流）
//!   ——重演确定性 + 信号采集；
//! - **故障注入**：同步断 / 覆盖攻击 / 画像上云三故障——红线实测场（复述）；
//! - **三分流**复述。
//!
//! **判据（锚点原文）**：60 格矩阵、剧本确定性、三故障注入、处置清单、判据。
//!
//! **错误路径与降级矩阵**：
//! | 降级项 | 落位 |
//! |---|---|
//! | 覆盖攻击成功→P0（复述红线实测） | [`C78Code::COVER_P0`] |
//! | 覆盖不达标→补跑（复述） | [`C78Code::COVERAGE_MISS`] |
//! | 重演分歧→立案（复述） | [`C78Code::REPLAY_DIVERGE`] |
//! | 处置无责→兜底（复述） | [`C78Code::NO_OWNER`] |
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3678 诊断码段（独占 0x7Exx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct C78Code(pub u16);

impl C78Code {
    /// 覆盖攻击成功（P0 红线实测）。
    pub const COVER_P0: C78Code = C78Code(0x7E01);
    /// 覆盖不达标。
    pub const COVERAGE_MISS: C78Code = C78Code(0x7E02);
    /// 重演分歧。
    pub const REPLAY_DIVERGE: C78Code = C78Code(0x7E03);
    /// 处置无责。
    pub const NO_OWNER: C78Code = C78Code(0x7E04);

    pub const fn value(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            C78Code::COVER_P0 => "覆盖攻击成功（P0 红线实测）。建议：覆盖禁令修复。",
            C78Code::COVERAGE_MISS => "矩阵覆盖不达标。建议：补跑缺失格。",
            C78Code::REPLAY_DIVERGE => "剧本重演分歧。建议：立案比对。",
            C78Code::NO_OWNER => "处置无责任人。建议：兜底指派。",
            _ => "未知诊断码。",
        }
    }
}

// ── 组合矩阵 ────────────────────────────────────────────────────────

/// 设备档（三档）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DeviceTier {
    /// 高档。
    High,
    /// 中档。
    Mid,
    /// 低档。
    Low,
}

impl DeviceTier {
    pub const ALL: [DeviceTier; 3] = [DeviceTier::High, DeviceTier::Mid, DeviceTier::Low];
}

/// 场景（五类：变更/预设/同步/冲突/导出）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum R04Scene {
    /// 变更。
    Change,
    /// 预设。
    Preset,
    /// 同步。
    Sync,
    /// 冲突。
    Conflict,
    /// 导出。
    Export,
}

impl R04Scene {
    pub const ALL: [R04Scene; 5] = [
        R04Scene::Change,
        R04Scene::Preset,
        R04Scene::Sync,
        R04Scene::Conflict,
        R04Scene::Export,
    ];
}

/// 60 格矩阵（4 域 × 5 场景 × 3 设备档）。
pub struct ComboMatrix {
    /// 已跑格数（扁平索引）。
    ran: Vec<u16>,
    coverage_miss: u32,
    last_code: C78Code,
    has_error: bool,
}

impl ComboMatrix {
    pub fn new() -> Self {
        Self {
            ran: Vec::new(),
            coverage_miss: 0,
            last_code: C78Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C78Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    /// 标记一格（域 0-3 × 场景 0-4 × 档 0-2）。
    pub fn mark(&mut self, domain: u8, scene: u8, tier: u8) {
        let idx = (domain as u16) * 15 + (scene as u16) * 3 + tier as u16;
        if !self.ran.contains(&idx) {
            self.ran.push(idx);
        }
    }

    /// 覆盖底线：至少 60 格。
    pub fn complete(&self) -> bool {
        self.ran.len() >= 60
    }

    /// 覆盖审计。
    pub fn audit(&mut self) -> Result<bool, C78Code> {
        if self.complete() {
            self.last_code = C78Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.coverage_miss += 1;
            self.last_code = C78Code::COVERAGE_MISS;
            self.has_error = true;
            Err(C78Code::COVERAGE_MISS)
        }
    }

    pub fn ran_count(&self) -> usize {
        self.ran.len()
    }

    pub fn coverage_miss(&self) -> u32 {
        self.coverage_miss
    }
}

// ── 剧本执行器 ──────────────────────────────────────────────────────

/// 剧本步骤。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScriptStep {
    /// 调外观。
    TuneVisual,
    /// 场景规则。
    SceneRule,
    /// 换设备同步。
    DeviceSync,
    /// 导出迁移。
    ExportMigrate,
}

impl ScriptStep {
    pub const CHRONO: [ScriptStep; 4] = [
        ScriptStep::TuneVisual,
        ScriptStep::SceneRule,
        ScriptStep::DeviceSync,
        ScriptStep::ExportMigrate,
    ];
}

/// 剧本执行器（重演确定性 + 信号采集）。
pub struct ScriptRunner {
    executed: Vec<ScriptStep>,
    signals: Vec<u32>,
    diverge_hits: u32,
    last_code: C78Code,
    has_error: bool,
}

impl ScriptRunner {
    pub fn new() -> Self {
        Self {
            executed: Vec::new(),
            signals: Vec::new(),
            diverge_hits: 0,
            last_code: C78Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C78Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    /// 执行一步并采信号。
    pub fn run_step(&mut self, step: ScriptStep, signal: u32) -> usize {
        self.executed.push(step);
        self.signals.push(signal);
        self.executed.len()
    }

    /// 重演确定性：两轮序列一致。
    pub fn assert_deterministic(&mut self, other: &[ScriptStep]) -> Result<bool, C78Code> {
        let same = self.executed.len() == other.len()
            && self.executed.iter().zip(other.iter()).all(|(a, b)| a == b);
        if same {
            self.last_code = C78Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.diverge_hits += 1;
            self.last_code = C78Code::REPLAY_DIVERGE;
            self.has_error = true;
            Err(C78Code::REPLAY_DIVERGE)
        }
    }

    /// 完整剧本（四步按序）。
    pub fn script_complete(&self) -> bool {
        self.executed.len() >= 4
            && ScriptStep::CHRONO
                .iter()
                .enumerate()
                .all(|(i, s)| self.executed.get(i).map(|e| e == s).unwrap_or(false))
    }

    /// 信号采集齐备。
    pub fn signals_collected(&self) -> bool {
        self.signals.len() == self.executed.len() && !self.signals.is_empty()
    }

    pub fn step_count(&self) -> usize {
        self.executed.len()
    }

    pub fn diverge_hits(&self) -> u32 {
        self.diverge_hits
    }
}

// ── 故障注入 ────────────────────────────────────────────────────────

/// 三故障。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fault {
    /// 同步断。
    SyncBreak,
    /// 覆盖攻击。
    CoverAttack,
    /// 画像上云。
    ProfileCloud,
}

impl Fault {
    pub const ALL: [Fault; 3] = [Fault::SyncBreak, Fault::CoverAttack, Fault::ProfileCloud];
}

/// 故障注入集（红线实测场）。
pub struct FaultInjector {
    injected: Vec<Fault>,
    /// 覆盖攻击是否被拦（P0）。
    cover_blocked: bool,
    p0_hits: u32,
    last_code: C78Code,
    has_error: bool,
}

impl FaultInjector {
    pub fn new() -> Self {
        Self {
            injected: Vec::new(),
            cover_blocked: true,
            p0_hits: 0,
            last_code: C78Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C78Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    /// 注入一个故障。
    pub fn inject(&mut self, fault: Fault) -> usize {
        self.injected.push(fault);
        self.injected.len()
    }

    /// 覆盖攻击结果上报。
    pub fn report_cover(&mut self, blocked: bool) -> Result<bool, C78Code> {
        self.cover_blocked = blocked;
        if blocked {
            self.last_code = C78Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.p0_hits += 1;
            self.last_code = C78Code::COVER_P0;
            self.has_error = true;
            Err(C78Code::COVER_P0)
        }
    }

    /// 三故障齐备注入。
    pub fn all_injected(&self) -> bool {
        Fault::ALL.iter().all(|f| self.injected.contains(f))
    }

    pub fn injected_count(&self) -> usize {
        self.injected.len()
    }

    pub fn p0_hits(&self) -> u32 {
        self.p0_hits
    }

    pub fn cover_blocked(&self) -> bool {
        self.cover_blocked
    }
}

// ── 处置清单 ────────────────────────────────────────────────────────

/// 处置项。
#[derive(Clone, Copy)]
pub struct DisposalItem {
    pub id: u16,
    /// 责任人 ID（0=无责）。
    pub owner: u8,
}

/// 处置清单（三分流复述）。
pub struct DisposalList {
    items: Vec<DisposalItem>,
    no_owner_hits: u32,
    last_code: C78Code,
    has_error: bool,
}

impl DisposalList {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            no_owner_hits: 0,
            last_code: C78Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C78Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    pub fn add(&mut self, id: u16, owner: u8) -> usize {
        self.items.push(DisposalItem { id, owner });
        self.items.len()
    }

    /// 兜底审计：每项必须有责。
    pub fn audit(&mut self) -> Result<bool, C78Code> {
        if self.items.iter().all(|i| i.owner != 0) {
            self.last_code = C78Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.no_owner_hits += 1;
            self.last_code = C78Code::NO_OWNER;
            self.has_error = true;
            Err(C78Code::NO_OWNER)
        }
    }

    /// 兜底指派。
    pub fn assign_fallback(&mut self, fallback_owner: u8) -> usize {
        let mut n = 0;
        for i in &mut self.items {
            if i.owner == 0 {
                i.owner = fallback_owner;
                n += 1;
            }
        }
        n
    }

    pub fn item_count(&self) -> usize {
        self.items.len()
    }

    pub fn no_owner_hits(&self) -> u32 {
        self.no_owner_hits
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves78_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves78_r04integ");
    {
        let m = ComboMatrix::new();
        chk(&mut set, "C3678-01", m.ran_count() == 0 && !m.complete(), "err");
    }
    {
        let mut m = ComboMatrix::new();
        for d in 0..4u8 {
            for s in 0..5u8 {
                for t in 0..3u8 {
                    m.mark(d, s, t);
                }
            }
        }
        chk(&mut set, "C3678-02", m.complete() && m.audit() == Ok(true), "err");
    }
    {
        let mut m = ComboMatrix::new();
        m.mark(0, 0, 0);
        let res = m.audit();
        chk(&mut set, "C3678-03", res.err() == Some(C78Code::COVERAGE_MISS), "err");
    }
    {
        let mut r = ScriptRunner::new();
        for s in ScriptStep::CHRONO {
            r.run_step(s, 1);
        }
        chk(&mut set, "C3678-04", r.script_complete() && r.signals_collected(), "err");
    }
    {
        let mut r = ScriptRunner::new();
        r.run_step(ScriptStep::TuneVisual, 1);
        r.run_step(ScriptStep::SceneRule, 2);
        let other = [ScriptStep::TuneVisual, ScriptStep::SceneRule];
        chk(&mut set, "C3678-05", r.assert_deterministic(&other) == Ok(true), "err");
    }
    {
        let mut r = ScriptRunner::new();
        r.run_step(ScriptStep::TuneVisual, 1);
        let other = [ScriptStep::ExportMigrate];
        let res = r.assert_deterministic(&other);
        chk(&mut set, "C3678-06", res.err() == Some(C78Code::REPLAY_DIVERGE), "err");
    }
    {
        let mut f = FaultInjector::new();
        f.inject(Fault::SyncBreak);
        f.inject(Fault::CoverAttack);
        f.inject(Fault::ProfileCloud);
        chk(&mut set, "C3678-07", f.all_injected() && f.injected_count() == 3, "err");
    }
    {
        let mut f = FaultInjector::new();
        let res = f.report_cover(true);
        chk(&mut set, "C3678-08", res == Ok(true) && f.cover_blocked(), "err");
    }
    {
        let mut f = FaultInjector::new();
        let res = f.report_cover(false);
        chk(&mut set, "C3678-09", res.err() == Some(C78Code::COVER_P0), "err");
    }
    {
        let mut d = DisposalList::new();
        d.add(1, 10);
        d.add(2, 11);
        chk(&mut set, "C3678-10", d.audit() == Ok(true), "err");
    }
    {
        let mut d = DisposalList::new();
        d.add(1, 0);
        let res = d.audit();
        chk(&mut set, "C3678-11", res.err() == Some(C78Code::NO_OWNER), "err");
    }
    {
        let mut d = DisposalList::new();
        d.add(1, 0);
        d.add(2, 5);
        let n = d.assign_fallback(99);
        chk(&mut set, "C3678-12", n == 1 && d.audit() == Ok(true), "err");
    }
    set
}
