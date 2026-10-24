//! VE-F3838 · S02 联调
//!
//! 54 格分层抽样（六域×三平台×三系统态）+ 20 分钟高对比剧本 + 三故障注入。
//! 缺色检出 P0。重演确定性。三分流处置。

#![allow(dead_code)]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

// ── 诊断码段（0xBAxx）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0xBA00;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CBACode(pub u16);

impl CBACode {
    pub const OK: Self = Self(SEG_BASE);
    pub const COLOR_MISSING_P0: Self = Self(SEG_BASE + 1);
    pub const COVERAGE_SHORT: Self = Self(SEG_BASE + 2);
    pub const REPLAY_DIVERGE: Self = Self(SEG_BASE + 3);
    pub const DISPATCH_ORPHAN: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::COLOR_MISSING_P0 => "缺色检出（P0）：必需色位缺失",
            Self::COVERAGE_SHORT => "覆盖不达标，需补跑",
            Self::REPLAY_DIVERGE => "重演分歧，立案",
            Self::DISPATCH_ORPHAN => "处置无责，兜底",
            _ => "未知",
        }
    }
}

// ── 数据结构 ────────────────────────────────────────────────────────

/// 六域。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Domain {
    /// 对比度。
    Contrast,
    /// 主题映射。
    Theme,
    /// 色弱适配。
    Cvd,
    /// 文字强化。
    Text,
    /// 图形替代。
    Graphic,
    /// 性能。
    Perf,
}

impl Domain {
    pub const ALL: [Domain; 6] = [
        Domain::Contrast,
        Domain::Theme,
        Domain::Cvd,
        Domain::Text,
        Domain::Graphic,
        Domain::Perf,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Contrast => "对比度",
            Self::Theme => "主题映射",
            Self::Cvd => "色弱适配",
            Self::Text => "文字强化",
            Self::Graphic => "图形替代",
            Self::Perf => "性能",
        }
    }
}

/// 三平台。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    /// 桌面。
    Desktop,
    /// 移动。
    Mobile,
    /// 嵌入。
    Embedded,
}

impl Platform {
    pub const ALL: [Platform; 3] = [Platform::Desktop, Platform::Mobile, Platform::Embedded];

    pub fn name(self) -> &'static str {
        match self {
            Self::Desktop => "桌面",
            Self::Mobile => "移动",
            Self::Embedded => "嵌入",
        }
    }
}

/// 三系统态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SysState {
    /// 默认。
    Default,
    /// 高对比。
    HighContrast,
    /// forced-colors。
    Forced,
}

impl SysState {
    pub const ALL: [SysState; 3] = [SysState::Default, SysState::HighContrast, SysState::Forced];

    pub fn name(self) -> &'static str {
        match self {
            Self::Default => "默认",
            Self::HighContrast => "高对比",
            Self::Forced => "forced",
        }
    }
}

/// 抽样格。
#[derive(Debug, Clone, Copy)]
pub struct Cell {
    /// 域。
    pub domain: Domain,
    /// 平台。
    pub platform: Platform,
    /// 系统态。
    pub state: SysState,
    /// 是否已跑。
    pub executed: bool,
    /// 是否通过。
    pub passed: bool,
    /// 缺色检出。
    pub color_missing: bool,
}

impl Cell {
    pub fn key(self) -> u16 {
        (self.domain as u16) * 100 + (self.platform as u16) * 10 + (self.state as u16)
    }
}

/// 故障注入三类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    /// 缺色。
    MissingColor,
    /// 覆写风暴。
    OverwriteStorm,
    /// 过渡破门。
    TransitionBreach,
}

impl Fault {
    pub const ALL: [Fault; 3] = [
        Fault::MissingColor,
        Fault::OverwriteStorm,
        Fault::TransitionBreach,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::MissingColor => "缺色",
            Self::OverwriteStorm => "覆写风暴",
            Self::TransitionBreach => "过渡破门",
        }
    }
}

/// 剧本步骤。
#[derive(Debug, Clone)]
pub struct ScriptStep {
    /// 序号。
    pub seq: u16,
    /// 动作描述。
    pub action: &'static str,
    /// 期望信号。
    pub expect_signal: &'static str,
    /// 是否命中。
    pub hit: bool,
}

/// 20 分钟剧本。
#[derive(Debug, Clone)]
pub struct DramaScript {
    /// 步骤。
    pub steps: Vec<ScriptStep>,
    /// 重演确定性（同输入同信号）。
    pub deterministic: bool,
}

impl DramaScript {
    pub fn new() -> Self {
        Self {
            steps: Vec::new(),
            deterministic: true,
        }
    }

    pub fn push(&mut self, seq: u16, action: &'static str, expect: &'static str) {
        self.steps.push(ScriptStep {
            seq,
            action,
            expect_signal: expect,
            hit: false,
        });
    }

    /// 全步骤命中。
    pub fn all_hit(&self) -> bool {
        self.steps.iter().all(|s| s.hit)
    }
}

impl Default for DramaScript {
    fn default() -> Self {
        Self::new()
    }
}

/// 处置分流。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Triage {
    /// 立案。
    Case,
    /// 补跑。
    Rerun,
    /// 兜底。
    Fallback,
}

impl Triage {
    pub fn name(self) -> &'static str {
        match self {
            Self::Case => "立案",
            Self::Rerun => "补跑",
            Self::Fallback => "兜底",
        }
    }
}

/// 处置项。
#[derive(Debug, Clone)]
pub struct DispatchItem {
    /// 来源。
    pub source: String,
    /// 分流。
    pub triage: Triage,
    /// 是否有责。
    pub owned: bool,
}

/// 联调器。
#[derive(Debug, Clone)]
pub struct S02Integrator {
    /// 54 格。
    pub cells: Vec<Cell>,
    /// 剧本。
    pub script: DramaScript,
    /// 处置清单。
    pub dispatch: Vec<DispatchItem>,
}

impl S02Integrator {
    pub fn new() -> Self {
        let mut cells = Vec::new();
        for d in Domain::ALL.iter() {
            for p in Platform::ALL.iter() {
                for s in SysState::ALL.iter() {
                    cells.push(Cell {
                        domain: *d,
                        platform: *p,
                        state: *s,
                        executed: false,
                        passed: false,
                        color_missing: false,
                    });
                }
            }
        }
        Self {
            cells,
            script: DramaScript::new(),
            dispatch: Vec::new(),
        }
    }

    /// 覆盖底线：≥54 格、已跑 ≥54。
    pub fn coverage_ok(&self) -> bool {
        self.cells.len() >= 54 && self.cells.iter().filter(|c| c.executed).count() >= 54
    }

    /// 缺色 P0 扫描。
    pub fn color_missing_p0(&self) -> bool {
        self.cells.iter().any(|c| c.color_missing)
    }

    /// 重演确定。
    pub fn replay_stable(&self) -> bool {
        self.script.deterministic && self.script.all_hit()
    }

    /// 处置有责。
    pub fn dispatch_owned(&self) -> bool {
        self.dispatch.iter().all(|d| d.owned)
    }

    /// 全检。
    pub fn full_check(&self) -> Result<(), CBACode> {
        if self.color_missing_p0() {
            return Err(CBACode::COLOR_MISSING_P0);
        }
        if !self.coverage_ok() {
            return Err(CBACode::COVERAGE_SHORT);
        }
        if !self.replay_stable() {
            return Err(CBACode::REPLAY_DIVERGE);
        }
        if !self.dispatch_owned() {
            return Err(CBACode::DISPATCH_ORPHAN);
        }
        Ok(())
    }

    /// 标记执行。
    pub fn mark_executed(&mut self, idx: usize, passed: bool, color_missing: bool) {
        if idx < self.cells.len() {
            self.cells[idx].executed = true;
            self.cells[idx].passed = passed;
            self.cells[idx].color_missing = color_missing;
        }
    }

    /// 故障注入：登记处置。
    pub fn inject_fault(&mut self, fault: Fault) {
        let (triage, owned) = match fault {
            Fault::MissingColor => (Triage::Case, true),
            Fault::OverwriteStorm => (Triage::Rerun, true),
            Fault::TransitionBreach => (Triage::Fallback, true),
        };
        self.dispatch.push(DispatchItem {
            source: String::from(fault.name()),
            triage,
            owned,
        });
    }
}

impl Default for S02Integrator {
    fn default() -> Self {
        Self::new()
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves138_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves138_s02integ");

    fn ready() -> S02Integrator {
        let mut it = S02Integrator::new();
        for i in 0..54 {
            it.mark_executed(i, true, false);
        }
        it.script.push(1, "开启 forced", "forced-on");
        it.script.push(2, "主题映射", "theme-mapped");
        it.script.push(3, "色弱叠加", "cvd-on");
        it.script.push(4, "文字强化", "text-boost");
        for s in it.script.steps.iter_mut() {
            s.hit = true;
        }
        it.inject_fault(Fault::MissingColor);
        it.inject_fault(Fault::OverwriteStorm);
        it.inject_fault(Fault::TransitionBreach);
        it
    }

    // 01 54 格覆盖
    {
        let it = ready();
        chk(&mut set, "C3838-01", it.cells.len() == 54 && it.coverage_ok(), "err");
    }
    // 02 覆盖不足
    {
        let mut it = ready();
        it.cells[0].executed = false;
        chk(&mut set, "C3838-02", !it.coverage_ok(), "err");
    }
    // 03 缺色 P0
    {
        let mut it = ready();
        it.mark_executed(0, true, true);
        chk(
            &mut set,
            "C3838-03",
            it.full_check() == Err(CBACode::COLOR_MISSING_P0),
            "err",
        );
    }
    // 04 无缺色
    {
        let it = ready();
        chk(&mut set, "C3838-04", !it.color_missing_p0(), "err");
    }
    // 05 重演确定
    {
        let it = ready();
        chk(&mut set, "C3838-05", it.replay_stable(), "err");
    }
    // 06 重演分歧
    {
        let mut it = ready();
        it.script.deterministic = false;
        chk(&mut set, "C3838-06", !it.replay_stable(), "err");
    }
    // 07 剧本未命中
    {
        let mut it = ready();
        it.script.steps[0].hit = false;
        chk(
            &mut set,
            "C3838-07",
            it.full_check() == Err(CBACode::REPLAY_DIVERGE),
            "err",
        );
    }
    // 08 三故障齐
    {
        let it = ready();
        chk(&mut set, "C3838-08", it.dispatch.len() == 3, "err");
    }
    // 09 处置有责
    {
        let it = ready();
        chk(&mut set, "C3838-09", it.dispatch_owned(), "err");
    }
    // 10 处置无责
    {
        let mut it = ready();
        it.dispatch[0].owned = false;
        chk(
            &mut set,
            "C3838-10",
            it.full_check() == Err(CBACode::DISPATCH_ORPHAN),
            "err",
        );
    }
    // 11 全检通过
    {
        let it = ready();
        chk(&mut set, "C3838-11", it.full_check().is_ok(), "err");
    }
    // 12 故障分流语义
    {
        let mut it = S02Integrator::new();
        it.inject_fault(Fault::MissingColor);
        let ok = it.dispatch[0].triage == Triage::Case
            && it.dispatch[0].source.as_str() == "缺色";
        chk(&mut set, "C3838-12", ok, "err");
    }

    set
}
