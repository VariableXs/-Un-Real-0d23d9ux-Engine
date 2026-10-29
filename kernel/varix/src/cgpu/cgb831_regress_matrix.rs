//! cgb831_regress_matrix — CGPU-F0831 Intel 回归矩阵。
//!
//! 判据原文（验收行摘录）：Intel 回归矩阵：维度交叉（驱动档 × 场景档 两
//! 维闭集交叉出格）；格执行状态机（待跑→通过/失败/跳过 三终态）；失败重
//! 跑（失败格允许一次重跑，重跑通过翻转状态并记账）；矩阵守恒（格总数 =
//! 通过+失败+跳过+待跑）。判据：交叉出格、三终态、一次重跑、守恒。
//!
//! 诊断段 0x7260（本单独占）。
//!
//! 设计纪律：no_std+alloc，禁 unwrap/expect/panic/浮点/std；失败路径
//! Result<_, u16> 闭集错误码；定容矩阵；守恒恒等式每步可验；drill() 自
//! 例程返回 0 表示全过。

#![allow(dead_code)]

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const MOD_VERSION: &str = "W3-cgb831-v1";

/// 诊断段基（本单独占 0x7260）。
pub const SEG_BASE: u16 = 0x7260;

/// 格不存在（行/列越界）。
pub const XC_CELL_MISS: u16 = SEG_BASE + 1;
/// 终态改写（通过/跳过格再执行——状态机违约）。
pub const XC_TERMINAL_LOCKED: u16 = SEG_BASE + 2;
/// 重跑超限（失败格第二次重跑）。
pub const XC_RERUN_EXCEEDED: u16 = SEG_BASE + 3;
/// 重复建格（矩阵已建好再次建）。
pub const XC_ALREADY_BUILT: u16 = SEG_BASE + 4;
/// 矩阵未建（执行前未出格）。
pub const XC_NOT_BUILT: u16 = SEG_BASE + 5;
/// 矩阵守恒破坏。
pub const XC_MATRIX_DRIFT: u16 = SEG_BASE + 6;
/// 汇总未齐（报告要求全格终态，仍有待跑）。
pub const XC_PENDING_LEFT: u16 = SEG_BASE + 7;

/// 驱动档（维 1 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DrvTier {
    /// 高性能档。
    XeHpc,
    /// 主流档。
    IrisXe,
    /// 基础档。
    Uhd,
    /// 保留档（老旧支持下限）。
    Legacy,
}

impl DrvTier {
    /// 档序号。
    pub fn idx(self) -> usize {
        match self {
            DrvTier::XeHpc => 0,
            DrvTier::IrisXe => 1,
            DrvTier::Uhd => 2,
            DrvTier::Legacy => 3,
        }
    }
    /// 档名。
    pub fn name(self) -> &'static str {
        match self {
            DrvTier::XeHpc => "xe_hpc",
            DrvTier::IrisXe => "iris_xe",
            DrvTier::Uhd => "uhd",
            DrvTier::Legacy => "legacy",
        }
    }
}

/// 场景档（维 2 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Scene {
    /// 共享内存。
    Shm,
    /// 媒体。
    Media,
    /// 功耗。
    Power,
    /// 遥测。
    Telemetry,
    /// 安全。
    Security,
    /// 基准。
    Bench,
}

impl Scene {
    /// 场景序号。
    pub fn idx(self) -> usize {
        match self {
            Scene::Shm => 0,
            Scene::Media => 1,
            Scene::Power => 2,
            Scene::Telemetry => 3,
            Scene::Security => 4,
            Scene::Bench => 5,
        }
    }
    /// 场景名。
    pub fn name(self) -> &'static str {
        match self {
            Scene::Shm => "shm",
            Scene::Media => "media",
            Scene::Power => "power",
            Scene::Telemetry => "telemetry",
            Scene::Security => "security",
            Scene::Bench => "bench",
        }
    }
}

/// 维 1 数。
pub const DRV_N: usize = 4;
/// 维 2 数。
pub const SCENE_N: usize = 6;

/// 格状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CellSt {
    /// 待跑。
    Pending,
    /// 通过。
    Passed,
    /// 失败。
    Failed,
    /// 跳过。
    Skipped,
}

impl CellSt {
    /// 是否终态。
    pub fn terminal(self) -> bool {
        matches!(self, CellSt::Passed | CellSt::Skipped)
    }
    /// 状态名。
    pub fn name(self) -> &'static str {
        match self {
            CellSt::Pending => "pending",
            CellSt::Passed => "passed",
            CellSt::Failed => "failed",
            CellSt::Skipped => "skipped",
        }
    }
}

/// 矩阵格。
#[derive(Clone, Copy, Debug)]
pub struct Cell {
    /// 行（驱动档）。
    pub drv: usize,
    /// 列（场景）。
    pub scene: usize,
    /// 状态。
    pub st: CellSt,
    /// 已重跑次数（失败后允许一次）。
    pub reruns: u32,
}

/// 回归矩阵。
pub struct Matrix {
    /// 矩阵名。
    pub name: String,
    /// 格账（定容 DRV_N * SCENE_N）。
    cells: Vec<Cell>,
    /// 已建位。
    built: bool,
    /// 重跑通过翻转数（记账项）。
    pub flip_pass: u64,
    /// 执行批次。
    pub rounds: u64,
}

/// 守恒恒等式：cells.len() == 通过数 + 失败数 + 跳过数 + 待跑数。
/// 每次状态迁移后 verify() 复核。
impl Matrix {
    /// 构造。
    pub fn new(name: &str) -> Self {
        Matrix { name: name.to_string(), cells: Vec::new(), built: false, flip_pass: 0, rounds: 0 }
    }

    /// 出格（判据"维度交叉"：闭集两维全积）。
    pub fn build(&mut self) -> Result<usize, u16> {
        if self.built {
            return Err(XC_ALREADY_BUILT);
        }
        self.cells.clear();
        for d in 0..DRV_N {
            for s in 0..SCENE_N {
                self.cells.push(Cell { drv: d, scene: s, st: CellSt::Pending, reruns: 0 });
            }
        }
        self.built = true;
        self.verify()?;
        Ok(self.cells.len())
    }

    /// 是否已建。
    pub fn is_built(&self) -> bool {
        self.built
    }

    /// 格总数。
    pub fn cell_count(&self) -> usize {
        self.cells.len()
    }

    /// 取格下标。
    fn at(&self, drv: usize, scene: usize) -> Result<usize, u16> {
        if !self.built {
            return Err(XC_NOT_BUILT);
        }
        if drv >= DRV_N || scene >= SCENE_N {
            return Err(XC_CELL_MISS);
        }
        Ok(drv * SCENE_N + scene)
    }

    /// 格状态只读。
    pub fn cell_st(&self, drv: usize, scene: usize) -> Result<CellSt, u16> {
        let i = self.at(drv, scene)?;
        Ok(self.cells[i].st)
    }

    /// 标跳过（待跑→跳过，终态）。
    pub fn skip(&mut self, drv: usize, scene: usize) -> Result<(), u16> {
        let i = self.at(drv, scene)?;
        if self.cells[i].st.terminal() {
            return Err(XC_TERMINAL_LOCKED);
        }
        self.cells[i].st = CellSt::Skipped;
        self.rounds += 1;
        self.verify()?;
        Ok(())
    }

    /// 执行一格（待跑→通过/失败；判据"三终态"）。
    pub fn execute(&mut self, drv: usize, scene: usize, ok: bool) -> Result<(), u16> {
        let i = self.at(drv, scene)?;
        if self.cells[i].st.terminal() {
            return Err(XC_TERMINAL_LOCKED);
        }
        if !ok {
            self.cells[i].st = CellSt::Failed;
        } else {
            self.cells[i].st = CellSt::Passed;
        }
        self.rounds += 1;
        self.verify()?;
        Ok(())
    }

    /// 重跑失败格（判据"失败重跑"——允许一次，通过则翻转）。
    pub fn rerun(&mut self, drv: usize, scene: usize, ok: bool) -> Result<(), u16> {
        let i = self.at(drv, scene)?;
        if self.cells[i].st != CellSt::Failed {
            return Err(XC_TERMINAL_LOCKED);
        }
        if self.cells[i].reruns >= 1 {
            return Err(XC_RERUN_EXCEEDED);
        }
        self.cells[i].reruns += 1;
        if ok {
            self.cells[i].st = CellSt::Passed;
            self.flip_pass += 1;
        }
        self.rounds += 1;
        self.verify()?;
        Ok(())
    }

    /// 各状态计数。
    pub fn count_of(&self, st: CellSt) -> Result<u32, u16> {
        if !self.built {
            return Err(XC_NOT_BUILT);
        }
        let mut n = 0u32;
        for c in self.cells.iter() {
            if c.st == st {
                n += 1;
            }
        }
        Ok(n)
    }

    /// 汇总门（判据"汇总未齐"：全格终态才可收口）。
    pub fn close_check(&self) -> Result<(), u16> {
        let pend = self.count_of(CellSt::Pending)?;
        if pend > 0 {
            return Err(XC_PENDING_LEFT);
        }
        Ok(())
    }

    /// 守恒校验。
    pub fn verify(&self) -> Result<(), u16> {
        if !self.built {
            return Ok(());
        }
        let p = self.count_of(CellSt::Passed)? as usize;
        let f = self.count_of(CellSt::Failed)? as usize;
        let s = self.count_of(CellSt::Skipped)? as usize;
        let w = self.count_of(CellSt::Pending)? as usize;
        if p + f + s + w != self.cells.len() {
            return Err(XC_MATRIX_DRIFT);
        }
        Ok(())
    }

    /// 审计摘要。
    pub fn audit(&self) -> String {
        let mut s = String::new();
        s.push_str("cgb831[");
        s.push_str(&self.name);
        s.push_str("] built=");
        s.push_str(if self.built { "1" } else { "0" });
        s.push_str(" cells=");
        s.push_str(&self.cells.len().to_string());
        s.push_str(" rounds=");
        s.push_str(&self.rounds.to_string());
        s.push_str(" flips=");
        s.push_str(&self.flip_pass.to_string());
        s
    }
}

/// 自例程：构造正/负样本走核心路径；全过返回 0，失败返回错误码。
pub fn drill() -> Result<u32, u16> {
    let mut m = Matrix::new("w3-drill-831");

    // 负样本 1：未建矩阵即执行——应拒。
    match m.execute(DrvTier::XeHpc.idx(), Scene::Shm.idx(), true) {
        Err(code) if code == XC_NOT_BUILT => {}
        Ok(_) => return Err(XC_NOT_BUILT),
        Err(other) => return Err(other),
    }
    let n = m.build()?;
    if n != DRV_N * SCENE_N || !m.is_built() {
        return Err(XC_MATRIX_DRIFT);
    }
    // 负样本 2：重复建格——应拒。
    match m.build() {
        Err(code) if code == XC_ALREADY_BUILT => {}
        Ok(_) => return Err(XC_ALREADY_BUILT),
        Err(other) => return Err(other),
    }
    // 负样本 3：越界格——应拒。
    match m.execute(DRV_N, 0, true) {
        Err(code) if code == XC_CELL_MISS => {}
        Ok(_) => return Err(XC_CELL_MISS),
        Err(other) => return Err(other),
    }
    match m.execute(0, SCENE_N, true) {
        Err(code) if code == XC_CELL_MISS => {}
        Ok(_) => return Err(XC_CELL_MISS),
        Err(other) => return Err(other),
    }

    // 批 1：通过 8 格、失败 2 格、跳过 6 格。
    for s in 0..SCENE_N {
        m.execute(DrvTier::XeHpc.idx(), s, true)?;
        m.execute(DrvTier::IrisXe.idx(), s, s % 2 == 0)?;
    }
    for s in 0..SCENE_N {
        m.skip(DrvTier::Uhd.idx(), s)?;
    }
    m.execute(DrvTier::Legacy.idx(), Scene::Shm.idx(), true)?;
    m.execute(DrvTier::Legacy.idx(), Scene::Media.idx(), false)?;
    m.execute(DrvTier::Legacy.idx(), Scene::Power.idx(), true)?;
    m.execute(DrvTier::Legacy.idx(), Scene::Telemetry.idx(), false)?;
    m.execute(DrvTier::Legacy.idx(), Scene::Security.idx(), true)?;
    m.execute(DrvTier::Legacy.idx(), Scene::Bench.idx(), true)?;
    m.verify()?;

    // 负样本 4：终态改写——通过格再执行。
    match m.execute(DrvTier::XeHpc.idx(), Scene::Shm.idx(), true) {
        Err(code) if code == XC_TERMINAL_LOCKED => {}
        Ok(_) => return Err(XC_TERMINAL_LOCKED),
        Err(other) => return Err(other),
    }
    // 负样本 5：跳过格重跑。
    match m.rerun(DrvTier::Uhd.idx(), Scene::Shm.idx(), true) {
        Err(code) if code == XC_TERMINAL_LOCKED => {}
        Ok(_) => return Err(XC_TERMINAL_LOCKED),
        Err(other) => return Err(other),
    }

    // 汇总门：尚有 2 个失败格（已终态但门要求全终态——失败也算终态，门应过）。
    m.close_check()?;

    // 重跑路径：2 个失败格重跑，1 过 1 不过，再第二次重跑应拒。
    m.rerun(DrvTier::Legacy.idx(), Scene::Media.idx(), true)?;
    m.rerun(DrvTier::Legacy.idx(), Scene::Telemetry.idx(), false)?;
    match m.rerun(DrvTier::Legacy.idx(), Scene::Telemetry.idx(), true) {
        Err(code) if code == XC_RERUN_EXCEEDED => {}
        Ok(_) => return Err(XC_RERUN_EXCEEDED),
        Err(other) => return Err(other),
    }
    if m.flip_pass != 1 {
        return Err(XC_MATRIX_DRIFT);
    }

    // 计数核验：全格终态。
    let pend = m.count_of(CellSt::Pending)?;
    let pass = m.count_of(CellSt::Passed)?;
    let fail = m.count_of(CellSt::Failed)?;
    let skip = m.count_of(CellSt::Skipped)?;
    if pend != 0 {
        return Err(XC_PENDING_LEFT);
    }
    if (pass + fail + skip) as usize != DRV_N * SCENE_N {
        return Err(XC_MATRIX_DRIFT);
    }
    m.verify()?;
    Ok(0)
}
