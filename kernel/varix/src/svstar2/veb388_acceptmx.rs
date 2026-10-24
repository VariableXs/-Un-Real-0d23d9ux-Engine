//! veb388_acceptmx —— VE-F0388 B 域验收矩阵。
//! 锚点（CoRun Unxreal 主册）：VE-F0388 · B 域验收矩阵 · 目标 360 行
//! 判据原文：VE-F0388-J1 验收矩阵 = 验收项 × 验收档（冒烟/全量/
//!           长稳）；每格记三态：过/败/未跑；矩阵裁决：全量档
//!           有败格 → 拒收；冒烟档有败格 → 拒收（冒烟是底线）；
//!           未跑格在终验中视为败（零信任）；长稳档败格不拒收
//!           但出风险单；格覆盖率核（跑过的格 / 总格数）。
//! 组件：矩阵（行 = 验收项，列 = 档）+ 三态格 + 裁决器 + 覆盖率。
//! 职责定位：验收矩阵是「B 域的考场答题卡」——每道题在三个难度
//!           档各有一格；冒烟是及格线，全量是毕业线，长稳是健
//!           康档案；未跑视败堵住「少跑几格混过关」的空子；长
//!           稳败不拒收但留风险单，是为了不因长周期问题卡交付。
//! 性能逐项分解：记格 O(1)；裁决 O(行列)；覆盖率 O(行列)。
//! 诊断码段：0x30680（0x30681~0x3068A 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const AM_VERSION: &str = "veb388_acceptmx-v1";

/// 诊断段基址（0x30680 段 · 本模块独占）。
pub const AM_SEG_BASE: u32 = 0x30680;
pub const XC_SMOKE_FAIL: u32 = 0x30681; // 冒烟败拒收
pub const XC_FULL_FAIL: u32 = 0x30682; // 全量败拒收
pub const XC_UNRAN: u32 = 0x30683; // 未跑视为败（终验）
pub const XC_CELL_OOR: u32 = 0x30684; // 格越界
pub const XC_MAGIC: u32 = 0x30685; // 账魔数破
pub const XC_ROW_DUP: u32 = 0x30686; // 行重名
pub const XC_RISK: u32 = 0x30687; // 风险单
pub const XC_DIM: u32 = 0x30688; // 维度不合法
pub const XC_RECELL: u32 = 0x30689; // 格重记（终验后）

/// 档位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    /// 冒烟。
    Smoke = 0,
    /// 全量。
    Full = 1,
    /// 长稳。
    Soak = 2,
}

/// 格三态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cell {
    /// 未跑。
    Unran = 0,
    /// 过。
    Pass = 1,
    /// 败。
    Fail = 2,
}

/// 风险单。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RiskRec {
    pub item: &'static str,
    pub tier: Tier,
}

/// 验收矩阵。
pub struct AcceptMx {
    rows: Vec<&'static str>,
    cells: Vec<Cell>, // rows.len() * 3
    risks: Vec<RiskRec>,
    sealed: bool,
    magic: u32,
}

impl AcceptMx {
    /// 建。
    pub fn new() -> AcceptMx {
        AcceptMx { rows: Vec::new(), cells: Vec::new(), risks: Vec::new(), sealed: false, magic: 0x0680_AC07 }
    }

    /// 加验收项（行）。
    pub fn add_row(&mut self, name: &'static str) -> Result<(), u32> {
        if self.magic != 0x0680_AC07 {
            return Err(XC_MAGIC);
        }
        for r in self.rows.iter() {
            if *r == name {
                return Err(XC_ROW_DUP);
            }
        }
        self.rows.push(name);
        self.cells.push(Cell::Unran);
        self.cells.push(Cell::Unran);
        self.cells.push(Cell::Unran);
        Ok(())
    }

    fn idx(&self, row: usize, tier: Tier) -> usize {
        row * 3 + tier as usize
    }

    /// 记格。
    pub fn mark(&mut self, item: &'static str, tier: Tier, c: Cell) -> Result<(), u32> {
        if self.sealed {
            return Err(XC_RECELL);
        }
        let r = match self.rows.iter().position(|x| *x == item) {
            Some(r) => r,
            None => return Err(XC_CELL_OOR),
        };
        let i = self.idx(r, tier);
        self.cells[i] = c;
        // 长稳败 → 风险单。
        if c == Cell::Fail && tier == Tier::Soak {
            self.risks.push(RiskRec { item, tier });
            return Err(XC_RISK);
        }
        Ok(())
    }

    /// 终验裁决：sealed 前未跑格视为败。
    pub fn seal(&mut self) -> Result<bool, u32> {
        if self.magic != 0x0680_AC07 {
            return Err(XC_MAGIC);
        }
        if self.rows.is_empty() {
            return Err(XC_DIM);
        }
        self.sealed = true;
        // 冒烟 + 全量：有败格或未跑格 → 拒收。
        for r in 0..self.rows.len() {
            let smoke = self.cells[self.idx(r, Tier::Smoke)];
            let full = self.cells[self.idx(r, Tier::Full)];
            if smoke != Cell::Pass {
                return Err(XC_SMOKE_FAIL);
            }
            if full != Cell::Pass {
                return Err(XC_FULL_FAIL);
            }
        }
        Ok(true)
    }

    /// 覆盖率（千分比）：非未跑格 / 总格数。
    pub fn coverage_milli(&self) -> Result<u32, u32> {
        if self.magic != 0x0680_AC07 {
            return Err(XC_MAGIC);
        }
        if self.cells.is_empty() {
            return Err(XC_DIM);
        }
        let mut ran = 0u32;
        for c in self.cells.iter() {
            if *c != Cell::Unran {
                ran += 1;
            }
        }
        Ok(ran * 1000 / self.cells.len() as u32)
    }

    /// 风险单数。
    pub fn risk_count(&self) -> usize {
        self.risks.len()
    }

    /// 格状态读。
    pub fn cell(&self, item: &str, tier: Tier) -> Result<Cell, u32> {
        let r = match self.rows.iter().position(|x| *x == item) {
            Some(r) => r,
            None => return Err(XC_CELL_OOR),
        };
        Ok(self.cells[self.idx(r, tier)])
    }

    /// 行数。
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }
}

pub fn run_veb388_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0388");
    // 族一：全绿收。
    let mut a = AcceptMx::new();
    let _ = a.add_row("boot");
    let _ = a.add_row("io");
    let _ = a.add_row("mem");
    s.add("三行", a.row_count() == 3, "行数");
    for it in ["boot", "io", "mem"] {
        let _ = a.mark(it, Tier::Smoke, Cell::Pass);
        let _ = a.mark(it, Tier::Full, Cell::Pass);
        let _ = a.mark(it, Tier::Soak, Cell::Pass);
    }
    s.add("覆盖率千满", a.coverage_milli() == Ok(1000), "9/9");
    s.add("全绿收", a.seal() == Ok(true), "过验");
    s.add("封卷后重记拒", a.mark("boot", Tier::Smoke, Cell::Fail) == Err(XC_RECELL), "终验后");
    // 族二：冒烟败拒。
    let mut b = AcceptMx::new();
    let _ = b.add_row("x");
    let _ = b.mark("x", Tier::Smoke, Cell::Fail);
    let _ = b.mark("x", Tier::Full, Cell::Pass);
    let _ = b.mark("x", Tier::Soak, Cell::Pass);
    s.add("冒烟败拒收", b.seal() == Err(XC_SMOKE_FAIL), "底线");
    // 族三：全量败拒。
    let mut c = AcceptMx::new();
    let _ = c.add_row("x");
    let _ = c.mark("x", Tier::Smoke, Cell::Pass);
    let _ = c.mark("x", Tier::Full, Cell::Fail);
    let _ = c.mark("x", Tier::Soak, Cell::Pass);
    s.add("全量败拒收", c.seal() == Err(XC_FULL_FAIL), "毕业线");
    // 族四：未跑视为败（零信任）。
    let mut d = AcceptMx::new();
    let _ = d.add_row("x");
    let _ = d.mark("x", Tier::Smoke, Cell::Pass);
    let _ = d.mark("x", Tier::Full, Cell::Pass);
    // Soak 不记（未跑）。
    s.add("未跑不挡 seal 前读", d.cell("x", Tier::Soak) == Ok(Cell::Unran), "格态");
    let _ = d.mark("x", Tier::Soak, Cell::Pass);
    s.add("补跑后收", d.seal() == Ok(true), "齐格");
    let mut e = AcceptMx::new();
    let _ = e.add_row("x");
    let _ = e.mark("x", Tier::Smoke, Cell::Pass);
    // Full 未跑 → 拒。
    s.add("全量未跑拒", e.seal() == Err(XC_FULL_FAIL), "未跑视败");
    // 族五：长稳败 → 风险单不拒收。
    let mut f = AcceptMx::new();
    let _ = f.add_row("x");
    let _ = f.mark("x", Tier::Smoke, Cell::Pass);
    let _ = f.mark("x", Tier::Full, Cell::Pass);
    let rf = f.mark("x", Tier::Soak, Cell::Fail);
    s.add("长稳败出风险单", rf == Err(XC_RISK), "留档");
    s.add("风险单 1", f.risk_count() == 1, "一单");
    s.add("长稳败仍收", f.seal() == Ok(true), "不卡交付");
    // 覆盖率部分。
    let mut g = AcceptMx::new();
    let _ = g.add_row("x");
    let _ = g.add_row("y");
    let _ = g.mark("x", Tier::Smoke, Cell::Pass);
    // 6 格跑了 1 格 → 166。
    s.add("覆盖率 166", g.coverage_milli() == Ok(166), "1/6");
    s.add("重行拒", g.add_row("x") == Err(XC_ROW_DUP), "主键");
    s.add("野格拒", g.mark("ghost", Tier::Smoke, Cell::Pass) == Err(XC_CELL_OOR), "无行");
    s.add("魔数核", AM_SEG_BASE == 0x30680, "段基址");
    s
}
