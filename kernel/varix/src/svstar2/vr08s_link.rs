//! VE-F3759 · 创作无障碍联调
//!
//! 锚点：创作无障碍联调（F3160 模式创作版复刻：联调矩阵（六维×四工具=24 格
//! （复述矩阵）；联调门禁（CI 门禁无障碍段（复述门禁）；联调报告（证据同源
//! （复述证据红线）；联调回归（夜间回归（复述回归）。
//!
//! 数据结构：24 格联调矩阵；门禁复述；证据同源；回归复述。
//! 错误路径与降级矩阵：矩阵缺格→补跑（复述）；门禁失效→阻断（红线实测）；
//! 证据异源→立案（复述红线）；回归漏→补（复述）。
//! 跨批对接点：F3160/F3357 单源复用声明；F3742-F3758 对端；F3760 收口输入。
//!
//! 诊断码段：独占 0x96xx。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ─────────────────────────────────────────────────────────────
// 诊断码（0x96xx）
// ─────────────────────────────────────────────────────────────

/// F3759 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct G8Code(pub u16);

impl G8Code {
    /// 联调矩阵缺格——补跑。
    pub const MATRIX_GAP: G8Code = G8Code(0x9601);
    /// 门禁失效——阻断红线。
    pub const GATE_BROKEN: G8Code = G8Code(0x9602);
    /// 证据异源——立案红线。
    pub const EVIDENCE_FORK: G8Code = G8Code(0x9603);
    /// 回归漏——补。
    pub const REGRESSION_MISS: G8Code = G8Code(0x9604);
    /// 六维缺——复述。
    pub const DIM_MISSING: G8Code = G8Code(0x9605);
    /// 夜间任务未挂——调度复述。
    pub const NIGHTLY_UNSET: G8Code = G8Code(0x9606);

    /// 诊断说明。
    pub fn explain(self) -> &'static str {
        match self.0 {
            0x9601 => "联调矩阵缺格，需补跑。",
            0x9602 => "CI 门禁无障碍段失效，阻断红线不通过。",
            0x9603 => "联调证据异源，证据红线不通过。",
            0x9604 => "回归覆盖漏项，需补。",
            0x9605 => "六维联调缺维，复述不完整。",
            0x9606 => "夜间回归任务未挂载，调度复述不完整。",
            _ => "未知诊断码。",
        }
    }

    /// 数值。
    pub fn raw(self) -> u16 {
        self.0
    }
}

// ─────────────────────────────────────────────────────────────
// 六维联调
// ─────────────────────────────────────────────────────────────

/// 联调维度。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LinkDim {
    /// 工具可达。
    ToolReach,
    /// 产出验证。
    OutputVerify,
    /// 认知。
    Cognition,
    /// 辅具。
    Assistive,
    /// 通道。
    Channel,
    /// 偏好。
    Preference,
}

impl LinkDim {
    /// 稳定序号。
    pub fn index(self) -> u8 {
        match self {
            LinkDim::ToolReach => 0,
            LinkDim::OutputVerify => 1,
            LinkDim::Cognition => 2,
            LinkDim::Assistive => 3,
            LinkDim::Channel => 4,
            LinkDim::Preference => 5,
        }
    }

    /// 全集（六维）。
    pub fn all() -> [LinkDim; 6] {
        [
            LinkDim::ToolReach,
            LinkDim::OutputVerify,
            LinkDim::Cognition,
            LinkDim::Assistive,
            LinkDim::Channel,
            LinkDim::Preference,
        ]
    }
}

/// 24 格矩阵（六维 × 四工具）。
#[derive(Clone, Debug)]
pub struct LinkGrid {
    /// 是否已跑（6×4）。
    ran: [[bool; 4]; 6],
    /// 是否通过。
    passed: [[bool; 4]; 6],
}

impl LinkGrid {
    /// 空矩阵。
    pub fn new() -> LinkGrid {
        LinkGrid {
            ran: [[false; 4]; 6],
            passed: [[false; 4]; 6],
        }
    }

    /// 跑通一格。
    pub fn pass(&mut self, dim: LinkDim, tool: u8) -> bool {
        if tool >= 4 {
            return false;
        }
        let i = dim.index() as usize;
        self.ran[i][tool as usize] = true;
        self.passed[i][tool as usize] = true;
        true
    }

    /// 跑失败一格。
    pub fn fail(&mut self, dim: LinkDim, tool: u8) -> bool {
        if tool >= 4 {
            return false;
        }
        let i = dim.index() as usize;
        self.ran[i][tool as usize] = true;
        self.passed[i][tool as usize] = false;
        true
    }

    /// 未跑格数。
    pub fn unrun_count(&self) -> u32 {
        let mut n = 0;
        for i in 0..6 {
            for j in 0..4 {
                if !self.ran[i][j] {
                    n += 1;
                }
            }
        }
        n
    }

    /// 全覆盖。
    pub fn complete(&self) -> bool {
        self.unrun_count() == 0
    }

    /// 总格数。
    pub fn total(&self) -> u32 {
        24
    }
}

impl Default for LinkGrid {
    fn default() -> LinkGrid {
        LinkGrid::new()
    }
}

// ─────────────────────────────────────────────────────────────
// 门禁与证据
// ─────────────────────────────────────────────────────────────

/// CI 门禁无障碍段。
#[derive(Clone, Debug, Default)]
pub struct CiGate {
    /// 是否启用。
    pub enabled: bool,
    /// 是否阻断级（失效即全红）。
    pub blocking: bool,
}

impl CiGate {
    /// 构造。
    pub fn new() -> CiGate {
        CiGate {
            enabled: false,
            blocking: true,
        }
    }

    /// 启用。
    pub fn enable(&mut self) {
        self.enabled = true;
    }

    /// 合规：启用 + 阻断。
    pub fn ok(&self) -> bool {
        self.enabled && self.blocking
    }
}

/// 证据源。
#[derive(Clone, Debug, Default)]
pub struct EvidenceSource {
    /// 源 id。
    pub source_id: u64,
    /// 证据条数。
    pub count: u32,
}

impl EvidenceSource {
    /// 构造。
    pub fn new(source_id: u64) -> EvidenceSource {
        EvidenceSource {
            source_id,
            count: 0,
        }
    }

    /// 记证据。
    pub fn record(&mut self) -> u32 {
        self.count += 1;
        self.count
    }

    /// 同源判断。
    pub fn same_as(&self, other: &EvidenceSource) -> bool {
        self.source_id == other.source_id
    }
}

// ─────────────────────────────────────────────────────────────
// 夜间回归
// ─────────────────────────────────────────────────────────────

/// 回归调度。
#[derive(Clone, Debug, Default)]
pub struct NightlySchedule {
    /// 是否已挂夜间任务。
    pub scheduled: bool,
    /// 覆盖维度数。
    pub dims_covered: u32,
}

impl NightlySchedule {
    /// 构造。
    pub fn new() -> NightlySchedule {
        NightlySchedule {
            scheduled: false,
            dims_covered: 0,
        }
    }

    /// 挂载。
    pub fn mount(&mut self) {
        self.scheduled = true;
    }

    /// 覆盖一维。
    pub fn cover_dim(&mut self) -> u32 {
        self.dims_covered += 1;
        self.dims_covered
    }

    /// 合规：已挂 + 六维全覆盖。
    pub fn ok(&self) -> bool {
        self.scheduled && self.dims_covered >= 6
    }
}

// ─────────────────────────────────────────────────────────────
// 主编排器
// ─────────────────────────────────────────────────────────────

/// 联调台。
#[derive(Clone, Debug, Default)]
pub struct LinkBench {
    grid: LinkGrid,
    gate: CiGate,
    evidence: EvidenceSource,
    nightly: NightlySchedule,
}

impl LinkBench {
    /// 构造。
    pub fn new(source_id: u64) -> LinkBench {
        LinkBench {
            grid: LinkGrid::new(),
            gate: CiGate::new(),
            evidence: EvidenceSource::new(source_id),
            nightly: NightlySchedule::new(),
        }
    }

    /// 取矩阵（可变）。
    pub fn grid_mut(&mut self) -> &mut LinkGrid {
        &mut self.grid
    }

    /// 取门禁（可变）。
    pub fn gate_mut(&mut self) -> &mut CiGate {
        &mut self.gate
    }

    /// 取夜间（可变）。
    pub fn nightly_mut(&mut self) -> &mut NightlySchedule {
        &mut self.nightly
    }

    /// 全量审查。
    pub fn audit(&self) -> Vec<G8Code> {
        let mut bad = Vec::new();
        if !self.grid.complete() {
            bad.push(G8Code::MATRIX_GAP);
        }
        if !self.gate.ok() {
            bad.push(G8Code::GATE_BROKEN);
        }
        if !self.nightly.ok() {
            bad.push(G8Code::NIGHTLY_UNSET);
        }
        if self.evidence.count == 0 {
            bad.push(G8Code::REGRESSION_MISS);
        }
        bad
    }
}

// ─────────────────────────────────────────────────────────────
// 判据
// ─────────────────────────────────────────────────────────────

/// F3759 判据集。
pub fn run_vr08s_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3759");

    // 0x9601 24 格
    {
        let mut g = LinkGrid::new();
        out.add("0x9601-初始全未跑", g.unrun_count() == 24, "");
        let mut ran = 0;
        for d in LinkDim::all() {
            for t in 0..4u8 {
                if g.pass(d, t) {
                    ran += 1;
                }
            }
        }
        out.add("0x9601-全覆盖", ran == 24 && g.complete(), "");
        out.add("0x9601-越界拒收", !g.pass(LinkDim::ToolReach, 4), "");
    }

    // 0x9602 门禁
    {
        let mut gate = CiGate::new();
        out.add("0x9602-未启用拦", !gate.ok(), "");
        gate.enable();
        out.add("0x9602-启用后过", gate.ok(), "");
    }

    // 0x9603 证据同源
    {
        let a = EvidenceSource::new(100);
        let b = EvidenceSource::new(100);
        let c = EvidenceSource::new(200);
        out.add("0x9603-同源过", a.same_as(&b), "");
        out.add("0x9603-异源检出", !a.same_as(&c), "");
    }

    // 0x9606 夜间
    {
        let mut n = NightlySchedule::new();
        out.add("0x9606-未挂拦", !n.ok(), "");
        n.mount();
        for _ in 0..6 {
            n.cover_dim();
        }
        out.add("0x9606-挂载覆盖过", n.ok(), "");
    }

    // 判据
    {
        let mut b = LinkBench::new(1);
        let bad = b.audit();
        out.add("0x9604-联调台检出", bad.iter().any(|c| c.raw() == 0x9604), "");
    }

    out
}
