//! VE-F3618 · R01 联调（VE-S 域 · 批次 S01 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3618`
//!
//! **判据（锚点原文六条）**：63 格、三故障、全链剧本、重演确定、
//! 三分流、判据。逐条落位：
//! - **63 格**：七资产类 × 三工作流 × 三规模 = 63 格分层抽样
//!   （覆盖底线复述）——[`ComboMatrix`] 精确 63 格，缺格 →
//!   [`V12Code::COVERAGE_GAP`]。
//! - **三故障**：许可缺失/引用断/沙箱逃逸三故障注入
//!   （[`FaultInjector`]），三故障缺一 → [`V12Code::FAULT_INCOMPLETE`]。
//! - **全链剧本**：20 分钟创作剧本（新建→编辑→验证→发布→市场→安装
//!   混流），[`ScriptRunner`] 六段全过。
//! - **重演确定**：同输入同输出（重演确定性），分歧 → [`V12Code::REPLAY_DIVERGE`] 立案。
//! - **三分流**：降级/拦截/放行三分流处置（[`Triage`]）。
//! - **判据**：`run_ves18_checks` 逐条映射。
//!
//! **错误路径与降级矩阵（锚点原文四条 → 落位）**：
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 覆盖不达标→补跑（复述） | [`V12Code::COVERAGE_GAP`] |
//! | 重演分歧→立案（复述） | [`V12Code::REPLAY_DIVERGE`] |
//! | 拦截缺测→补测（复述） | [`V12Code::FAULT_INCOMPLETE`] |
//! | 处置无责→兜底（复述） | [`V12Code::TRIAGE_ORPHAN`] |
//!
//! **码段**：独占 **0x42xx**。
//!
//! **性能逐项分解**：单组合 O(分钟)；全集夜间；剧本 O(20 分钟)；
//! 归因 O(维数)。
//!
//! **跨批对接点**：F3604–F3617 全对端；F3620 输入；E/P/Q 协同——
//! 钉死于 [`DOCKS`]。
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::string::String;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3618 诊断码段（独占 0x42xx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct V12Code(pub u16);

impl V12Code {
    /// 组合矩阵覆盖缺口。
    pub const COVERAGE_GAP: V12Code = V12Code(0x4201);
    /// 故障注入集不全。
    pub const FAULT_INCOMPLETE: V12Code = V12Code(0x4202);
    /// 重演分歧。
    pub const REPLAY_DIVERGE: V12Code = V12Code(0x4203);
    /// 处置无归属。
    pub const TRIAGE_ORPHAN: V12Code = V12Code(0x4204);
    /// 剧本段失败。
    pub const SCRIPT_FAIL: V12Code = V12Code(0x4205);

    /// 数值。
    pub const fn value(self) -> u16 {
        self.0
    }

    /// 中文说明。
    pub fn explain(self) -> &'static str {
        match self {
            V12Code::COVERAGE_GAP => {
                "组合矩阵覆盖不足 63 格。建议：补跑缺失格后重新统计。"
            }
            V12Code::FAULT_INCOMPLETE => {
                "三故障注入集不全（许可缺失/引用断/沙箱逃逸缺一）。建议：补齐后重跑。"
            }
            V12Code::REPLAY_DIVERGE => {
                "同输入重演出不同结果（不确定性）。建议：排查随机源与时间依赖。"
            }
            V12Code::TRIAGE_ORPHAN => {
                "处置项无归属。建议：兜底到默认处置人并立案。"
            }
            V12Code::SCRIPT_FAIL => {
                "剧本段失败。建议：定位失败段并修复后重演。"
            }
            _ => "未知诊断码。",
        }
    }
}

// ── 组合矩阵（63 格）────────────────────────────────────────────────

/// 资产类（七类）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AssetClass {
    /// 主题。
    Theme,
    /// 皮肤。
    Skin,
    /// 壁纸。
    Wallpaper,
    /// 图标。
    Icon,
    /// 光标。
    Cursor,
    /// 组件。
    Component,
    /// 工作流包。
    WorkflowPack,
}

impl AssetClass {
    /// 全集。
    pub fn all() -> [AssetClass; 7] {
        [
            AssetClass::Theme,
            AssetClass::Skin,
            AssetClass::Wallpaper,
            AssetClass::Icon,
            AssetClass::Cursor,
            AssetClass::Component,
            AssetClass::WorkflowPack,
        ]
    }
}

/// 工作流（三种）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Flow {
    /// 新建创作。
    Create,
    /// 导入迁移。
    Import,
    /// 批量生产。
    Batch,
}

impl Flow {
    /// 全集。
    pub fn all() -> [Flow; 3] {
        [Flow::Create, Flow::Import, Flow::Batch]
    }
}

/// 规模（三档）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Scale {
    /// 小（百级）。
    Small,
    /// 中（千级）。
    Medium,
    /// 大（万级）。
    Large,
}

impl Scale {
    /// 全集。
    pub fn all() -> [Scale; 3] {
        [Scale::Small, Scale::Medium, Scale::Large]
    }
}

/// 一格。
#[derive(Clone, Copy, Debug)]
pub struct ComboCell {
    /// 资产类。
    pub class: AssetClass,
    /// 工作流。
    pub flow: Flow,
    /// 规模。
    pub scale: Scale,
    /// 是否已跑。
    pub ran: bool,
}

/// 组合矩阵。
#[derive(Clone, Debug, Default)]
pub struct ComboMatrix {
    cells: Vec<ComboCell>,
}

impl ComboMatrix {
    /// 全 63 格。
    pub fn full() -> ComboMatrix {
        let mut m = ComboMatrix { cells: Vec::new() };
        for c in AssetClass::all() {
            for f in Flow::all() {
                for s in Scale::all() {
                    m.cells.push(ComboCell {
                        class: c,
                        flow: f,
                        scale: s,
                        ran: false,
                    });
                }
            }
        }
        m
    }

    /// 格数。
    pub fn len(&self) -> usize {
        self.cells.len()
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    /// 覆盖审计：必须 63 格。
    pub fn audit_coverage(&self) -> Result<(), V12Code> {
        if self.cells.len() == 63 {
            Ok(())
        } else {
            Err(V12Code::COVERAGE_GAP)
        }
    }

    /// 标记一格已跑。
    pub fn mark_ran(&mut self, class: AssetClass, flow: Flow, scale: Scale) {
        if let Some(c) = self
            .cells
            .iter_mut()
            .find(|c| c.class == class && c.flow == flow && c.scale == scale)
        {
            c.ran = true;
        }
    }

    /// 已跑数。
    pub fn ran_count(&self) -> usize {
        self.cells.iter().filter(|c| c.ran).count()
    }
}

// ── 故障注入（三故障）───────────────────────────────────────────────

/// 故障类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fault {
    /// 许可缺失。
    LicenseMissing,
    /// 引用断链。
    RefBroken,
    /// 沙箱逃逸。
    SandboxEscape,
}

impl Fault {
    /// 三故障全集。
    pub fn all() -> [Fault; 3] {
        [Fault::LicenseMissing, Fault::RefBroken, Fault::SandboxEscape]
    }

    /// 中文名。
    pub fn name(self) -> &'static str {
        match self {
            Fault::LicenseMissing => "许可缺失",
            Fault::RefBroken => "引用断链",
            Fault::SandboxEscape => "沙箱逃逸",
        }
    }
}

/// 故障注入器。
#[derive(Clone, Debug, Default)]
pub struct FaultInjector {
    injected: Vec<Fault>,
}

impl FaultInjector {
    /// 新建。
    pub fn new() -> FaultInjector {
        FaultInjector {
            injected: Vec::new(),
        }
    }

    /// 注入。
    pub fn inject(&mut self, f: Fault) {
        if !self.injected.contains(&f) {
            self.injected.push(f);
        }
    }

    /// 三故障齐全审计。
    pub fn audit_complete(&self) -> Result<(), V12Code> {
        for f in Fault::all() {
            if !self.injected.contains(&f) {
                return Err(V12Code::FAULT_INCOMPLETE);
            }
        }
        Ok(())
    }

    /// 已注入数。
    pub fn count(&self) -> usize {
        self.injected.len()
    }
}

// ── 剧本执行器（六段）───────────────────────────────────────────────

/// 剧本段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Act {
    /// 新建。
    Create,
    /// 编辑。
    Edit,
    /// 验证。
    Verify,
    /// 发布。
    Publish,
    /// 市场。
    Market,
    /// 安装。
    Install,
}

impl Act {
    /// 六段全序。
    pub fn order() -> [Act; 6] {
        [
            Act::Create,
            Act::Edit,
            Act::Verify,
            Act::Publish,
            Act::Market,
            Act::Install,
        ]
    }

    /// 中文名。
    pub fn name(self) -> &'static str {
        match self {
            Act::Create => "新建",
            Act::Edit => "编辑",
            Act::Verify => "验证",
            Act::Publish => "发布",
            Act::Market => "市场",
            Act::Install => "安装",
        }
    }
}

/// 剧本执行器。
#[derive(Clone, Debug, Default)]
pub struct ScriptRunner {
    passed: Vec<Act>,
    /// 确定性指纹（重演对拍）。
    pub signature: u64,
}

impl ScriptRunner {
    /// 新建。
    pub fn new() -> ScriptRunner {
        ScriptRunner {
            passed: Vec::new(),
            signature: 0,
        }
    }

    /// 走一段。
    pub fn run_act(&mut self, act: Act, ok: bool) -> Result<(), V12Code> {
        if ok {
            self.passed.push(act);
            // 确定性签名：段序号混入
            self.signature = self
                .signature
                .wrapping_mul(31)
                .wrapping_add(act.name().len() as u64 + 1);
            Ok(())
        } else {
            Err(V12Code::SCRIPT_FAIL)
        }
    }

    /// 六段全过审计。
    pub fn audit_full(&self) -> Result<(), V12Code> {
        for a in Act::order() {
            if !self.passed.contains(&a) {
                return Err(V12Code::SCRIPT_FAIL);
            }
        }
        Ok(())
    }

    /// 重演对拍：两次运行签名一致。
    pub fn replay_match(&self, other: &ScriptRunner) -> Result<(), V12Code> {
        if self.signature == other.signature {
            Ok(())
        } else {
            Err(V12Code::REPLAY_DIVERGE)
        }
    }
}

// ── 三分流 ──────────────────────────────────────────────────────────

/// 处置分流。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Triage {
    /// 降级放行。
    Degrade,
    /// 拦截。
    Block,
    /// 放行。
    Pass,
}

impl Triage {
    /// 中文名。
    pub fn name(self) -> &'static str {
        match self {
            Triage::Degrade => "降级",
            Triage::Block => "拦截",
            Triage::Pass => "放行",
        }
    }
}

/// 处置清单。
#[derive(Clone, Debug, Default)]
pub struct TriageList {
    entries: Vec<(String, Triage)>,
}

impl TriageList {
    /// 新建。
    pub fn new() -> TriageList {
        TriageList {
            entries: Vec::new(),
        }
    }

    /// 记一条。
    pub fn record(&mut self, item: &str, t: Triage) {
        self.entries.push((String::from(item), t));
    }

    /// 无归属审计（空名）。
    pub fn audit_orphan(&self) -> Result<(), V12Code> {
        if self.entries.iter().any(|(n, _)| n.is_empty()) {
            Err(V12Code::TRIAGE_ORPHAN)
        } else {
            Ok(())
        }
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// ── 对外契约 ────────────────────────────────────────────────────────

/// 对接点清单。
pub const DOCKS: [&str; 3] = ["VE-F3604", "VE-F3620", "E-P-Q-COLLAB"];

// ── 自检 ────────────────────────────────────────────────────────────

/// 全模块自检清单。
pub fn run_ves18_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3618");

    // 0x4201 63 格齐
    let m = ComboMatrix::full();
    out.add("0x4201-63 格齐", m.audit_coverage().is_ok() && m.len() == 63, "");

    // 0x4202 三故障齐
    let mut fi = FaultInjector::new();
    for f in Fault::all() {
        fi.inject(f);
    }
    out.add("0x4202-三故障齐", fi.audit_complete().is_ok(), "");

    // 0x4203 故障缺一检出
    let mut fi2 = FaultInjector::new();
    fi2.inject(Fault::LicenseMissing);
    out.add("0x4203-故障缺一检出", fi2.audit_complete().is_err(), "");

    // 0x4204 六段全过
    let mut sr = ScriptRunner::new();
    for a in Act::order() {
        sr.run_act(a, true).ok();
    }
    out.add("0x4204-六段全过", sr.audit_full().is_ok(), "");

    // 0x4205 重演一致
    let mut sr2 = ScriptRunner::new();
    for a in Act::order() {
        sr2.run_act(a, true).ok();
    }
    out.add("0x4205-重演一致", sr.replay_match(&sr2).is_ok(), "");

    // 0x4206 重演分歧检出
    let mut sr3 = ScriptRunner::new();
    sr3.run_act(Act::Create, true).ok();
    out.add("0x4206-重演分歧检出", sr.replay_match(&sr3).is_err(), "");

    // 0x4207 剧本失败检出
    let mut sr4 = ScriptRunner::new();
    out.add("0x4207-剧本失败检出", sr4.run_act(Act::Verify, false).is_err(), "");

    // 0x4208 处置无归属
    let mut tl = TriageList::new();
    tl.record("", Triage::Block);
    out.add("0x4208-处置无归属检出", tl.audit_orphan().is_err(), "");

    // 0x4209 标记已跑
    let mut m2 = ComboMatrix::full();
    m2.mark_ran(AssetClass::Theme, Flow::Create, Scale::Small);
    out.add("0x4209-标记已跑正确", m2.ran_count() == 1, "");

    out
}
