//! VE-F3561 · 令牌执行审查总纲 —— 四件两律 + 独立审查 + 违规零静默 + 编号处置。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3561`
//!
//! # 职责（锚点原文拆解）
//!
//! - **四件**（[`AuditSuite`]）：符合性检查器 / 对拍 / 扫描 / 报告
//!   四件——审查不是氛围，是工程。
//! - **两律**（[`AuditPrinciples`]）：审查独立（审查方与实现方
//!   分离）+ 违规零静默（每个违规都有编号有处置）。
//! - **编号处置**（[`Violation`]）：违规项带编号，处置动作可追踪。
//!
//! # 降级矩阵（锚点）
//!
//! | 故障 | 处置 |
//! |---|---|
//! | 审查失效 | 换道（备用审查路径） |
//! | 违规静默 | 红线 |
//! | 工具误报 | 调阈留痕 |
//!
//! # 复杂度
//!
//! O(1) 编排。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E20 段：6400 起）
// ---------------------------------------------------------------------------

/// 审查总纲诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AuditCode {
    /// 违规静默（红线——检出违规但未编号）。
    SilentViolation = 6400,
    /// 审查独立性被破坏。
    IndependenceBreach = 6401,
    /// 四件不齐。
    SuiteIncomplete = 6402,
    /// 工具误报（调阈留痕）。
    FalsePositive = 6403,
}

impl AuditCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            AuditCode::SilentViolation => "违规静默红线",
            AuditCode::IndependenceBreach => "审查独立性破坏",
            AuditCode::SuiteIncomplete => "审查四件不齐",
            AuditCode::FalsePositive => "工具误报已调阈",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        matches!(self, AuditCode::SilentViolation | AuditCode::IndependenceBreach)
    }
}

// ---------------------------------------------------------------------------
// 二、两律
// ---------------------------------------------------------------------------

/// 两律声明。
#[derive(Clone, Copy, Debug)]
pub struct AuditPrinciples {
    /// 审查独立（审查方 ≠ 实现方）。
    pub independent: bool,
    /// 违规零静默（违规必编号）。
    pub zero_silence: bool,
}

impl AuditPrinciples {
    /// 两律全守。
    pub const fn holds(&self) -> bool {
        self.independent && self.zero_silence
    }
}

// ---------------------------------------------------------------------------
// 三、四件
// ---------------------------------------------------------------------------

/// 审查四件。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AuditInstrument {
    /// 符合性检查器。
    Conformance,
    /// 对拍。
    Diff,
    /// 扫描。
    Sweep,
    /// 报告。
    Report,
}

impl AuditInstrument {
    /// 全部四件。
    pub const ALL: [AuditInstrument; 4] = [
        AuditInstrument::Conformance,
        AuditInstrument::Diff,
        AuditInstrument::Sweep,
        AuditInstrument::Report,
    ];

    /// 件短名。
    pub const fn label(self) -> &'static str {
        match self {
            AuditInstrument::Conformance => "符合性检查",
            AuditInstrument::Diff => "对拍",
            AuditInstrument::Sweep => "扫描",
            AuditInstrument::Report => "报告",
        }
    }
}

// ---------------------------------------------------------------------------
// 四、违规与处置
// ---------------------------------------------------------------------------

/// 处置动作。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Disposition {
    /// 修复。
    Fix,
    /// 登记在册。
    Register,
    /// 豁免（调阈留痕）。
    Exempt,
}

/// 违规项（编号必填——零静默）。
#[derive(Clone, Copy, Debug)]
pub struct Violation {
    /// 违规编号。
    pub id: u32,
    /// 哪件检出。
    pub instrument: AuditInstrument,
    /// 处置。
    pub action: Disposition,
}

// ---------------------------------------------------------------------------
// 五、审查套件
// ---------------------------------------------------------------------------

/// 审查套件（四件 + 两律 + 违规登记）。
pub struct AuditSuite {
    principles: AuditPrinciples,
    ready: u8,
    violations: Vec<Violation>,
    next_id: u32,
}

impl AuditSuite {
    /// 新建（审查方独立设定）。
    pub fn new(independent: bool) -> Self {
        Self {
            principles: AuditPrinciples {
                independent,
                zero_silence: true,
            },
            ready: 0,
            violations: Vec::new(),
            next_id: 1,
        }
    }

    /// 标记一件就绪。
    pub fn mark_ready(&mut self, inst: AuditInstrument) {
        let bit = match inst {
            AuditInstrument::Conformance => 1u8,
            AuditInstrument::Diff => 2,
            AuditInstrument::Sweep => 4,
            AuditInstrument::Report => 8,
        };
        self.ready |= bit;
    }

    /// 四件齐备。
    pub fn suite_complete(&self) -> bool {
        self.ready == 0b1111
    }

    /// 登记违规（编号自动分配——零静默保障）。
    pub fn report(&mut self, inst: AuditInstrument, action: Disposition) -> u32 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        self.violations.push(Violation {
            id,
            instrument: inst,
            action,
        });
        id
    }

    /// 两律核验。
    pub fn principles_hold(&self) -> bool {
        self.principles.holds()
    }

    /// 违规总数（有编号的）。
    pub fn violation_count(&self) -> usize {
        self.violations.len()
    }

    /// 四件齐 + 两律守 + 违规全编号 → 放行。
    pub fn can_close(&self) -> bool {
        self.suite_complete() && self.principles_hold()
    }
}

// ---------------------------------------------------------------------------
// 六、判据
// ---------------------------------------------------------------------------

/// 审查总纲判据集。
pub fn run_audit_checks() -> crate::checks::CheckSet {
    let mut chk = crate::checks::CheckSet::new("svstar2-ver11a");

    // 1. 两律独立。
    let a = AuditSuite::new(true);
    chk.add("6400-两律独立", a.principles_hold(), "");

    // 2. 非独立阻断。
    let b = AuditSuite::new(false);
    chk.add("6401-独立性红线", !b.principles_hold(), "");

    // 3. 四件齐备。
    let mut c = AuditSuite::new(true);
    for i in AuditInstrument::ALL {
        c.mark_ready(i);
    }
    chk.add("6402-四件齐备", c.suite_complete(), "");

    // 4. 违规必编号。
    let mut d = AuditSuite::new(true);
    let id1 = d.report(AuditInstrument::Sweep, Disposition::Fix);
    let id2 = d.report(AuditInstrument::Diff, Disposition::Register);
    chk.add("6403-违规编号", id1 == 1 && id2 == 2 && d.violation_count() == 2, "");

    // 5. 四件不齐不收口。
    let mut e = AuditSuite::new(true);
    e.mark_ready(AuditInstrument::Report);
    chk.add("6404-缺件阻断", !e.can_close(), "");

    // 6. 齐件可收口。
    chk.add("6405-齐件收口", c.can_close(), "");

    chk
}
