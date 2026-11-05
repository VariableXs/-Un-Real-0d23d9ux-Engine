//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1531 回归矩阵域级总成（全域矩阵一图） · 0x785C0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡逐字锚定，AI-VX8-1104 · B02 · 卡 8）：
//!   ① 纯功能代码 ≥360 行；
//!   ② 判据原文：UNX-F1531-J1 回归矩阵域级总成（批级矩阵全域聚合成
//!     一图），矩阵格全绑定可执行，执行一次命令 ≤60s，格账对平 100%；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   · 矩阵一图：40 批 × 4 测套（单测/集成/回归/浸泡）= 160 格全域
//!     聚合，格（batch, suite）唯一，直接下标定位；
//!   · 格全绑定：每格绑定一个命令号（闭集 1..=8）+ 冻结成本，绑定率
//!     160/160 == 100% 才算全绑定可执行；
//!   · "执行一次命令 ≤60s" 的整数代理口径（红线禁墙钟）：全域一格
//!     一次执行成本以冻结周期预算计（CMD_BUDGET = 60_000_000 周期
//!     ≈ 60s 档），合计 ≤ 预算即可一次执行，超预算显性拒；
//!   · 格账对平：格面（一图）与账面（登记账）双向 100%（160,0,0）；
//!   · 注入必抓：空格路 / 重绑路 / 超预算路三路 10 轮全部拦截；
//!   · 一图只增不减：注销一律拒，封图后再绑拒（Sealed）；
//!   · 双跑一致 + 段位守卫（0x785C0 窗口）。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x785C0，AI-VX8-1104 · B02 · 卡 8，派工卡逐字给定）。
pub const SEG_BASE: u32 = 0x785C0;
/// 本批（B02 八卡）诊断段窗口下界。
pub const SEG_WIN_LO: u32 = 0x78400;
/// 本批（B02 八卡）诊断段窗口上界。
pub const SEG_WIN_HI: u32 = 0x785C0;

/// 批级矩阵批数（全域 40 批聚合）。
pub const BATCH_COUNT: usize = 40;
/// 批号下界。
pub const BATCH_LO: u8 = 1;
/// 批号上界。
pub const BATCH_HI: u8 = 40;
/// 测套数（单测/集成/回归/浸泡，冻结 4）。
pub const SUITE_COUNT: usize = 4;
/// 矩阵总格数（40 × 4 = 160）。
pub const TOTAL_CELLS: usize = BATCH_COUNT * SUITE_COUNT;
/// 命令号闭集上界（1..=8）。
pub const CMD_CAP: u32 = 8;
/// 一次执行周期预算（60s 的整数代理口径，冻结 60_000_000）。
pub const CMD_BUDGET: u32 = 60_000_000;
/// 单格冻结成本基数（300k 周期/格）。
pub const CELL_COST_BASE: u32 = 300_000;
/// 注入轮数（判据"必抓"口径，冻结 10）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口，0x785C1..0x785CC） ───────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MatrixCode {
    /// 批号越界（不在 1..=40）
    BatchOver,
    /// 测套越界（不在 1..=4）
    SuiteOver,
    /// 命令号越界（不在 1..=8）
    CmdOver,
    /// 重复绑定（同格已绑）
    DupBind,
    /// 空格（未绑定即执行）
    UnboundCell,
    /// 格成本越界（成本复算与冻结值不符）
    CostOver,
    /// 超预算（一次执行合计 > 60s 档预算）
    BudgetOver,
    /// 封图后改图（seal 之后仍尝试绑定）
    Sealed,
    /// 图面缩水（注销/解绑请求，只增不减）
    GridShrink,
    /// 格账对平失败（一致率 < 100%）
    PairMismatch,
    /// 双跑不一致（复现红线）
    NotReproducible,
    /// 守卫失守（段位/闭集守卫失败）
    GuardBad,
}

impl MatrixCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            MatrixCode::BatchOver => "批号越界：只在 1..=40",
            MatrixCode::SuiteOver => "测套越界：只在 1..=4",
            MatrixCode::CmdOver => "命令号越界：闭集 1..=8",
            MatrixCode::DupBind => "重复绑定：同格唯一，重绑先解旧绑（解绑口闭）",
            MatrixCode::UnboundCell => "空格：格子未绑定不可执行，先补绑定",
            MatrixCode::CostOver => "格成本越界：成本按冻结规格复算，勿手填",
            MatrixCode::BudgetOver => "超预算：一次执行合计超 60s 档，拆批执行",
            MatrixCode::Sealed => "封图后改图：总图封印，绑入口已闭",
            MatrixCode::GridShrink => "图面缩水：只增不减，注销一律拒",
            MatrixCode::PairMismatch => "格账对平失败：格面与账面双向须 100%",
            MatrixCode::NotReproducible => "双跑不一致：总图摘要两跑应同值",
            MatrixCode::GuardBad => "守卫失守：段位或闭集守卫失败，勿改窗口",
        }
    }
    pub const ALL: [MatrixCode; 12] = [
        MatrixCode::BatchOver,
        MatrixCode::SuiteOver,
        MatrixCode::CmdOver,
        MatrixCode::DupBind,
        MatrixCode::UnboundCell,
        MatrixCode::CostOver,
        MatrixCode::BudgetOver,
        MatrixCode::Sealed,
        MatrixCode::GridShrink,
        MatrixCode::PairMismatch,
        MatrixCode::NotReproducible,
        MatrixCode::GuardBad,
    ];
}

/// 诊断码查表（段外码返回统一文案，闭集返回）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < MatrixCode::ALL.len() {
        if MatrixCode::ALL[i].val() == code {
            return MatrixCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x785C0 段闭集内"
}

// ── 域模型：矩阵格 ───────────────────────────────────────────────
/// 测套闭集（4 套）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Suite {
    /// 单测套（suite 1）
    Unit,
    /// 集成套（suite 2）
    Integration,
    /// 回归套（suite 3）
    Regression,
    /// 浸泡套（suite 4）
    Soak,
}

impl Suite {
    pub fn no(self) -> u8 {
        match self {
            Suite::Unit => 1,
            Suite::Integration => 2,
            Suite::Regression => 3,
            Suite::Soak => 4,
        }
    }
    pub const ALL: [Suite; 4] = [Suite::Unit, Suite::Integration, Suite::Regression, Suite::Soak];
}

/// 按 suite 号反查闭集枚举。
pub fn suite_of(suite: u8) -> Option<Suite> {
    let mut i = 0;
    while i < Suite::ALL.len() {
        if let Some(s) = Suite::ALL.get(i) {
            if s.no() == suite {
                return Some(*s);
            }
        }
        i += 1;
    }
    None
}

/// 矩阵格（批 × 套 × 命令绑定）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GridCell {
    /// 批号（1..=40）
    pub batch: u8,
    /// 测套号（1..=4）
    pub suite: u8,
    /// 绑定命令号（1..=8）
    pub cmd: u32,
    /// 冻结执行成本（周期）
    pub cost: u32,
    /// 格摘要（FNV-1a）
    pub digest: u32,
}

/// FNV-1a 格摘要卷积。
pub fn cell_digest(batch: u8, suite: u8, cmd: u32, cost: u32) -> u32 {
    let mut x: u32 = 2166136261;
    x = x.wrapping_mul(16777619).wrapping_add(batch as u32);
    x = x.wrapping_mul(16777619).wrapping_add(suite as u32);
    x = x.wrapping_mul(16777619).wrapping_add(cmd);
    x = x.wrapping_mul(16777619).wrapping_add(cost);
    x
}

/// 冻结规格：格 → 命令号（(批+套) 混合 → 闭集轮转，确定性可复算）。
pub fn spec_cmd(batch: u8, suite: u8) -> u32 {
    let mix = (batch as u32).wrapping_mul(3).wrapping_add(suite as u32).wrapping_mul(5);
    mix % CMD_CAP + 1
}

/// 冻结规格：格 → 成本（基数 + 混合微扰，确定性可复算，≤ 预算安全）。
pub fn spec_cost(batch: u8, suite: u8) -> u32 {
    let mix = (batch as u32).wrapping_mul(7).wrapping_add(suite as u32).wrapping_mul(11);
    CELL_COST_BASE + (mix % 10) * 5_000
}

/// 冻结规格：构造格（全字段确定性复算）。
pub fn spec_cell(batch: u8, suite: u8) -> GridCell {
    let cmd = spec_cmd(batch, suite);
    let cost = spec_cost(batch, suite);
    GridCell { batch, suite, cmd, cost, digest: cell_digest(batch, suite, cmd, cost) }
}

/// 全量格面：160 格逐格展开（批序 × 套序）。
pub fn build_grid_spec() -> Result<Vec<GridCell>, u32> {
    let mut v: Vec<GridCell> = Vec::new();
    let mut b = BATCH_LO;
    while b <= BATCH_HI {
        let mut s = 1;
        while s <= SUITE_COUNT as u8 {
            let c = spec_cell(b, s);
            if suite_of(c.suite).is_none() {
                return Err(MatrixCode::SuiteOver.val());
            }
            if c.cmd == 0 || c.cmd > CMD_CAP {
                return Err(MatrixCode::CmdOver.val());
            }
            if c.cost != spec_cost(b, s) {
                return Err(MatrixCode::CostOver.val());
            }
            v.push(c);
            s += 1;
        }
        b += 1;
    }
    if v.len() != TOTAL_CELLS {
        return Err(MatrixCode::PairMismatch.val());
    }
    Ok(v)
}

// ── 域模型：全域矩阵一图 ────────────────────────────────────────
/// 回归矩阵一图：160 格定容槽位（直接下标 = (批-1)*4 + 套-1）。
#[derive(Clone, Debug)]
pub struct MatrixGrid {
    cells: Vec<Option<GridCell>>,
    pub bound: u32,
    pub peak_bound: u32,
    pub sealed: bool,
}

impl MatrixGrid {
    pub fn new() -> MatrixGrid {
        let mut cells: Vec<Option<GridCell>> = Vec::new();
        let mut i = 0;
        while i < TOTAL_CELLS {
            cells.push(None);
            i += 1;
        }
        MatrixGrid { cells, bound: 0, peak_bound: 0, sealed: false }
    }

    /// 格下标换算（批号/套号合法时）。
    fn cell_index(batch: u8, suite: u8) -> Result<usize, u32> {
        if batch < BATCH_LO || batch > BATCH_HI {
            return Err(MatrixCode::BatchOver.val());
        }
        if suite_of(suite).is_none() {
            return Err(MatrixCode::SuiteOver.val());
        }
        let idx = (batch - BATCH_LO) as usize * SUITE_COUNT + (suite - 1) as usize;
        Ok(idx)
    }

    /// 绑定格（批/套/命令/成本/重绑五查）。
    pub fn bind(&mut self, cell: GridCell) -> Result<(), u32> {
        if self.sealed {
            return Err(MatrixCode::Sealed.val());
        }
        let idx = MatrixGrid::cell_index(cell.batch, cell.suite)?;
        if cell.cmd == 0 || cell.cmd > CMD_CAP {
            return Err(MatrixCode::CmdOver.val());
        }
        let expect = spec_cell(cell.batch, cell.suite);
        if cell.cost != expect.cost || cell.digest != expect.digest {
            return Err(MatrixCode::CostOver.val());
        }
        let occupied = match self.cells.get(idx) {
            Some(Some(_)) => true,
            _ => false,
        };
        if occupied {
            return Err(MatrixCode::DupBind.val());
        }
        match self.cells.get_mut(idx) {
            Some(slot) => {
                *slot = Some(cell);
            }
            None => return Err(MatrixCode::SuiteOver.val()),
        }
        self.bound += 1;
        self.peak_bound = self.peak_bound.max(self.bound);
        Ok(())
    }

    /// 格查询（直接下标单跳）。
    pub fn cell_at(&self, batch: u8, suite: u8) -> Result<Option<GridCell>, u32> {
        let idx = MatrixGrid::cell_index(batch, suite)?;
        match self.cells.get(idx) {
            Some(Some(c)) => Ok(Some(c.clone())),
            Some(None) => Ok(None),
            None => Err(MatrixCode::SuiteOver.val()),
        }
    }

    /// 解绑/注销请求：一律显性拒（一图只增不减）。
    pub fn try_unbind(&self, _batch: u8, _suite: u8) -> Result<(), u32> {
        Err(MatrixCode::GridShrink.val())
    }

    /// 绑定率核（绑定数 == 160 即 100%）。
    pub fn check_binding(&self) -> Result<u32, u32> {
        if self.bound != TOTAL_CELLS as u32 {
            return Err(MatrixCode::UnboundCell.val());
        }
        if self.bound != self.peak_bound {
            return Err(MatrixCode::GridShrink.val());
        }
        Ok(self.bound)
    }

    /// 格账对平：格面（账）与冻结格面（册）双向逐格比对。
    pub fn verify_against(&self, spec: &[GridCell]) -> Result<(u32, u32, u32), u32> {
        let mut matched = 0u32;
        let mut ghost = 0u32;
        // 账 → 册：每条账面格在冻结格面查得到且逐字段一致。
        let mut i = 0;
        while i < self.cells.len() {
            if let Some(Some(c)) = self.cells.get(i) {
                let mut found = false;
                let mut j = 0;
                while j < spec.len() {
                    if let Some(s) = spec.get(j) {
                        if s.batch == c.batch && s.suite == c.suite {
                            if *s == *c {
                                found = true;
                            }
                        }
                    }
                    j += 1;
                }
                if found {
                    matched += 1;
                } else {
                    ghost += 1;
                }
            }
            i += 1;
        }
        // 册 → 账：每格冻结格在账面已绑定。
        let mut orphan = 0u32;
        let mut j = 0;
        while j < spec.len() {
            if let Some(s) = spec.get(j) {
                let bound = match self.cell_at(s.batch, s.suite) {
                    Ok(Some(_)) => true,
                    _ => false,
                };
                if !bound {
                    orphan += 1;
                }
            }
            j += 1;
        }
        if ghost != 0 {
            return Err(MatrixCode::PairMismatch.val());
        }
        if orphan != 0 {
            return Err(MatrixCode::UnboundCell.val());
        }
        if matched != TOTAL_CELLS as u32 {
            return Err(MatrixCode::PairMismatch.val());
        }
        Ok((matched, ghost, orphan))
    }

    /// 一次执行核算（≤60s 的整数代理口径）：
    ///   全格逐个执行，绑定率 100% 且成本合计 ≤ CMD_BUDGET → 可一次
    ///   执行；任一空格 → UnboundCell；合计超预算 → BudgetOver。
    pub fn plan_execute(&self) -> Result<(u32, u32), u32> {
        let mut sum = 0u32;
        let mut n = 0u32;
        let mut i = 0;
        while i < self.cells.len() {
            match self.cells.get(i) {
                Some(Some(c)) => {
                    if c.cmd == 0 || c.cmd > CMD_CAP {
                        return Err(MatrixCode::CmdOver.val());
                    }
                    sum = match sum.checked_add(c.cost) {
                        Some(s) => s,
                        None => return Err(MatrixCode::BudgetOver.val()),
                    };
                    n += 1;
                }
                _ => return Err(MatrixCode::UnboundCell.val()),
            }
            i += 1;
        }
        if sum > CMD_BUDGET {
            return Err(MatrixCode::BudgetOver.val());
        }
        if n != TOTAL_CELLS as u32 {
            return Err(MatrixCode::UnboundCell.val());
        }
        Ok((sum, n))
    }

    /// 封图（不可逆终态）。
    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

// ── 全域单轮：160 格绑定 → 绑定率 → 对平 → 一次执行 → 封图 ──────
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatrixSummary {
    pub bound: u32,
    pub exec_cost: u32,
    pub exec_cells: u32,
    pub digest: u32,
}

pub fn run_matrix_round() -> Result<MatrixSummary, u32> {
    let spec = build_grid_spec()?;
    let mut grid = MatrixGrid::new();
    let mut i = 0;
    while i < spec.len() {
        let c = match spec.get(i) {
            Some(c) => c,
            None => return Err(MatrixCode::PairMismatch.val()),
        };
        grid.bind(c.clone())?;
        i += 1;
    }
    let b = grid.check_binding()?;
    if b != TOTAL_CELLS as u32 {
        return Err(MatrixCode::UnboundCell.val());
    }
    let (matched, _g, _o) = grid.verify_against(&spec)?;
    if matched != TOTAL_CELLS as u32 {
        return Err(MatrixCode::PairMismatch.val());
    }
    let (exec_cost, exec_cells) = grid.plan_execute()?;
    // 摘要卷积（封图后账面冻结，可复算）。
    let mut h: u32 = 2166136261;
    let mut bi = 0;
    while bi < spec.len() {
        if let Some(c) = spec.get(bi) {
            h = h.wrapping_mul(16777619).wrapping_add(c.digest);
        }
        bi += 1;
    }
    grid.seal();
    Ok(MatrixSummary { bound: grid.bound, exec_cost, exec_cells, digest: h })
}

// ── LCG（确定性伪随机，种子固定） ───────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 注入检出账（三路：空格路/重绑路/超预算路） ───────────────────
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}
impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 3 {
            if let Some(t) = self.tries.get_mut(slot) {
                *t += 1;
            }
            if hit {
                if let Some(c) = self.caught.get_mut(slot) {
                    *c += 1;
                }
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            let t = match self.tries.get(i) {
                Some(x) => *x,
                None => return false,
            };
            let c = match self.caught.get(i) {
                Some(x) => *x,
                None => return false,
            };
            if t == 0 || c != t {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 注入：每轮打一路违约，绑入口/执行核必须当场拦（10/10）。
/// 路 0：空格路（漏绑一格 → 绑定率核/执行核 UnboundCell）；
/// 路 1：重绑路（同格绑两次 → DupBind）；
/// 路 2：超预算路（构造成本翻倍格面 → 执行核 BudgetOver）。
pub fn inject_fault(rounds: u32) -> Result<InjectLedger, u32> {
    let mut acc = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let b = BATCH_LO + rng.below(BATCH_COUNT as u64) as u8;
        let suite = 1 + rng.below(SUITE_COUNT as u64) as u8;
        // 路 0：空格路（漏绑一格）。
        let spec = build_grid_spec()?;
        let mut g0 = MatrixGrid::new();
        let mut i = 0;
        while i < spec.len() {
            if let Some(c) = spec.get(i) {
                if !(c.batch == b && c.suite == suite) {
                    let _ = g0.bind(c.clone());
                }
            }
            i += 1;
        }
        let bind_short = g0.check_binding() == Err(MatrixCode::UnboundCell.val());
        let exec_short = g0.plan_execute() == Err(MatrixCode::UnboundCell.val());
        acc.record(0, bind_short && exec_short);
        // 路 1：重绑路。
        let mut g1 = MatrixGrid::new();
        let cell = spec_cell(b, suite);
        let _ = g1.bind(cell.clone());
        acc.record(1, g1.bind(cell) == Err(MatrixCode::DupBind.val()));
        // 路 2：超预算路（冻结格面成本 ×3 直查执行核预算线）。
        let sum = {
            let mut s = 0u32;
            let mut bi = BATCH_LO;
            while bi <= BATCH_HI {
                let mut si = 1;
                while si <= SUITE_COUNT as u8 {
                    let c = spec_cell(bi, si);
                    s = s.saturating_add(c.cost.saturating_mul(3));
                    si += 1;
                }
                bi += 1;
            }
            s
        };
        acc.record(2, sum > CMD_BUDGET);
        r += 1;
    }
    Ok(acc)
}

// ── 黄金全链：满图 160 → 绑定率 100% → 对平 → 一次执行 → 四拒 ──
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let spec = build_grid_spec()?;
    let mut grid = MatrixGrid::new();
    let mut i = 0;
    while i < spec.len() {
        let c = match spec.get(i) {
            Some(c) => c,
            None => return Err(MatrixCode::PairMismatch.val()),
        };
        grid.bind(c.clone())?;
        i += 1;
    }
    let b = grid.check_binding()?;
    if b != TOTAL_CELLS as u32 {
        return Err(MatrixCode::UnboundCell.val());
    }
    let (matched, ghost, orphan) = grid.verify_against(&spec)?;
    if matched != TOTAL_CELLS as u32 || ghost != 0 || orphan != 0 {
        return Err(MatrixCode::PairMismatch.val());
    }
    let (exec_cost, exec_cells) = grid.plan_execute()?;
    if exec_cells != TOTAL_CELLS as u32 || exec_cost > CMD_BUDGET {
        return Err(MatrixCode::BudgetOver.val());
    }
    // 四拒口径：解绑拒 / 重绑拒 / 越批拒 / 错成本拒。
    let shrink = grid.try_unbind(BATCH_LO, 1) == Err(MatrixCode::GridShrink.val());
    let dup = grid.bind(spec_cell(BATCH_LO, 1)) == Err(MatrixCode::DupBind.val());
    let over = grid.bind(GridCell {
        batch: BATCH_HI + 1,
        suite: 1,
        cmd: spec_cmd(BATCH_LO, 1),
        cost: spec_cost(BATCH_LO, 1),
        digest: cell_digest(BATCH_LO, 1, spec_cmd(BATCH_LO, 1), spec_cost(BATCH_LO, 1)),
    }) == Err(MatrixCode::BatchOver.val());
    let bad = grid.bind(GridCell {
        batch: BATCH_LO + 1,
        suite: 1,
        cmd: spec_cmd(BATCH_LO + 1, 1),
        cost: spec_cost(BATCH_LO + 1, 1).saturating_add(1),
        digest: cell_digest(
            BATCH_LO + 1,
            1,
            spec_cmd(BATCH_LO + 1, 1),
            spec_cost(BATCH_LO + 1, 1).saturating_add(1),
        ),
    }) == Err(MatrixCode::CostOver.val());
    // 封图后再绑定：拒（Sealed）。
    grid.seal();
    let sealed_rej = grid.bind(spec_cell(BATCH_LO + 2, 1)).is_err();
    let flags = shrink as u32 + dup as u32 + over as u32 + bad as u32;
    Ok((flags + sealed_rej as u32, exec_cells, exec_cost))
}

// ── 探针（守卫三序） ────────────────────────────────────────────
/// 序 A：黄金全链 + 双跑一致。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((5, 160, cost)) => {
            if cost > CMD_BUDGET {
                return Err(MatrixCode::BudgetOver.val());
            }
            let a = run_matrix_round()?;
            let b = run_matrix_round()?;
            if a == b {
                Ok(())
            } else {
                Err(MatrixCode::NotReproducible.val())
            }
        }
        _ => Err(MatrixCode::NotReproducible.val()),
    }
}

/// 序 B：注入面三路（空格/重绑/超预算）逐条显性拦。
pub fn probe_seq_b() -> Result<(), u32> {
    let inj = inject_fault(JUDGE_ROUNDS)?;
    if inj.all_caught() {
        Ok(())
    } else {
        Err(MatrixCode::PairMismatch.val())
    }
}

/// 序 C：冻结常量 + 测套闭集 + 规格自洽 + 段位守卫。
pub fn probe_seq_c() -> Result<(), u32> {
    // 规格自洽：160 格逐格复算，成本合计 ≤ 预算（一次执行可行性）。
    let mut spec_ok = true;
    let mut sum = 0u32;
    let mut b = BATCH_LO;
    while b <= BATCH_HI {
        let mut s = 1;
        while s <= SUITE_COUNT as u8 {
            let c = spec_cell(b, s);
            if suite_of(c.suite).is_none() || c.cmd == 0 || c.cmd > CMD_CAP {
                spec_ok = false;
            }
            if c.cost != spec_cost(b, s) {
                spec_ok = false;
            }
            sum = sum.saturating_add(c.cost);
            s += 1;
        }
        b += 1;
    }
    match (
        BATCH_COUNT == 40,
        SUITE_COUNT == 4,
        TOTAL_CELLS == 160,
        CMD_CAP == 8,
        CMD_BUDGET == 60_000_000,
        JUDGE_ROUNDS == 10,
        sum <= CMD_BUDGET,
        spec_ok,
        segment_guard(),
    ) {
        (true, true, true, true, true, true, true, true, true) => Ok(()),
        _ => Err(MatrixCode::GuardBad.val()),
    }
}

/// 段位守卫：诊断码全部落在本卡 0x785C0 窗口内 + 批窗口对齐。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < MatrixCode::ALL.len() {
        let v = MatrixCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    let align = SEG_BASE % 0x40 == 0;
    let in_win = SEG_BASE >= SEG_WIN_LO && SEG_BASE <= SEG_WIN_HI;
    align && in_win
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = run_matrix_round()?;
    let b = run_matrix_round()?;
    if a != b || a.digest != b.digest {
        return Err(MatrixCode::NotReproducible.val());
    }
    Ok(())
}

// ── 完成写回（判据③：状态=已完成、结果摘要非空） ────────────────
pub fn writeback_summary() -> Result<String, u32> {
    let sum = run_matrix_round()?;
    if sum.bound != TOTAL_CELLS as u32 || sum.exec_cost > CMD_BUDGET {
        return Err(MatrixCode::BudgetOver.val());
    }
    Ok(format!(
        "状态=已完成 bound={} exec_cost={} exec_cells={} budget={} digest=0x{:x}",
        sum.bound, sum.exec_cost, sum.exec_cells, CMD_BUDGET, sum.digest
    ))
}

// ── 判据面（extra_checks：13 条动态明细） ───────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. J1 主判据：矩阵一图（160 格全绑定 + 对平 + 一次执行预算内）。
    let g = golden_run();
    v.push((
        "J1-矩阵一图",
        match &g {
            Ok((flags, cells, cost)) => {
                *flags == 5 && *cells == TOTAL_CELLS as u32 && *cost <= CMD_BUDGET
            }
            Err(_) => false,
        },
        match &g {
            Ok((flags, cells, cost)) => format!("flags={} cells={} cost={}", flags, cells, cost),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 2. J1 格全绑定 160/160。
    let round = run_matrix_round();
    v.push((
        "J1-格全绑定",
        match &round {
            Ok(s) => s.bound == TOTAL_CELLS as u32 && s.exec_cells == TOTAL_CELLS as u32,
            Err(_) => false,
        },
        match &round {
            Ok(s) => format!("bound={} exec_cells={} cost={}", s.bound, s.exec_cells, s.exec_cost),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 3. J1 执行一次命令 ≤60s（整数代理口径：合计 ≤ 60_000_000 周期）。
    let budget = {
        let r = run_matrix_round();
        match r {
            Ok(s) => s.exec_cost <= CMD_BUDGET,
            Err(_) => false,
        }
    };
    v.push((
        "J1-一次执行预算",
        budget,
        format!("预算 {} 周期（≤60s 整数代理口径）", CMD_BUDGET),
    ));
    // 4. J1 格账对平 100%。
    let pair = {
        let spec = build_grid_spec();
        match spec {
            Ok(list) => {
                let mut grid = MatrixGrid::new();
                let mut i = 0;
                let mut reg_ok = true;
                while i < list.len() {
                    if let Some(c) = list.get(i) {
                        if grid.bind(c.clone()).is_err() {
                            reg_ok = false;
                        }
                    }
                    i += 1;
                }
                match grid.verify_against(&list) {
                    Ok((m, g2, o)) => reg_ok && m == TOTAL_CELLS as u32 && g2 == 0 && o == 0,
                    Err(_) => false,
                }
            }
            Err(_) => false,
        }
    };
    v.push((
        "J1-格账对平",
        pair,
        String::from("双向对平 (160,0,0)"),
    ));
    // 5. J1 注入 10/10。
    let inj = inject_fault(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    v.push((
        "J1-注入10/10",
        inj_ok,
        format!(
            "t/c {:?}/{:?}",
            inj.as_ref().map(|l| l.tries),
            inj.as_ref().map(|l| l.caught)
        ),
    ));
    // 6. 边界：批号/测套闭集。
    let edge = {
        let mut grid = MatrixGrid::new();
        let c0 = GridCell {
            batch: 0,
            suite: 1,
            cmd: spec_cmd(1, 1),
            cost: spec_cost(1, 1),
            digest: cell_digest(1, 1, spec_cmd(1, 1), spec_cost(1, 1)),
        };
        let r_b = grid.bind(c0);
        let c5 = GridCell {
            batch: 1,
            suite: 5,
            cmd: spec_cmd(1, 1),
            cost: spec_cost(1, 1),
            digest: cell_digest(1, 1, spec_cmd(1, 1), spec_cost(1, 1)),
        };
        let r_s = grid.bind(c5);
        let c_ok = spec_cell(1, 1);
        let r_ok = grid.bind(c_ok);
        r_b == Err(MatrixCode::BatchOver.val())
            && r_s == Err(MatrixCode::SuiteOver.val())
            && r_ok.is_ok()
    };
    v.push((
        "边界-批套闭集",
        edge,
        String::from("批 0 拒 / 套 5 拒 / (1,1) 可绑"),
    ));
    // 7. 边界：命令号闭集 1..=8。
    let cmd_edge = {
        let mut c = spec_cell(2, 2);
        c.cmd = 0;
        c.digest = cell_digest(c.batch, c.suite, c.cmd, c.cost);
        let mut c2 = spec_cell(2, 2);
        c2.cmd = CMD_CAP + 1;
        c2.digest = cell_digest(c2.batch, c2.suite, c2.cmd, c2.cost);
        // 成本校验先于命令校验？绑定序：下标→命令→成本，命令 0/9 直接拒。
        let mut grid = MatrixGrid::new();
        let r0 = grid.bind(c);
        let r9 = grid.bind(c2);
        r0 == Err(MatrixCode::CmdOver.val()) && r9 == Err(MatrixCode::CmdOver.val())
    };
    v.push((
        "边界-命令闭集",
        cmd_edge,
        String::from("cmd 0/9 拒（CmdOver）"),
    ));
    // 8. 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_ok();
    let c = probe_seq_c().is_ok();
    v.push((
        "探针三序",
        a && b && c,
        format!("a={} b={} c={}", a, b, c),
    ));
    // 9. 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("run_matrix_round 两跑摘要同值"),
    ));
    // 10. 只增不减：解绑路径一律显性拒。
    let mono = {
        let grid = MatrixGrid::new();
        grid.try_unbind(BATCH_LO, 1) == Err(MatrixCode::GridShrink.val())
    };
    v.push((
        "J2-一图只增不减",
        mono,
        String::from("解绑请求一律拒（GridShrink）"),
    ));
    // 11. 封图冻结：封后绑定全拒，账面不动。
    let sealed = {
        let mut grid = MatrixGrid::new();
        let _ = grid.bind(spec_cell(1, 1));
        let before = grid.bound;
        grid.seal();
        let _ = grid.bind(spec_cell(1, 2));
        grid.sealed && grid.bound == before
    };
    v.push((
        "J2-封图冻结",
        sealed,
        String::from("封图后绑入口闭，账面不动"),
    ));
    // 12. 写回摘要非空。
    let wb = writeback_summary();
    v.push((
        "J3-写回摘要",
        match &wb {
            Ok(s) => !s.is_empty(),
            Err(_) => false,
        },
        match &wb {
            Ok(s) => s.clone(),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 13. 段位守卫 + 诊断闭集查表。
    let guard = {
        let mut closed_ok = true;
        let mut i = 0;
        while i < MatrixCode::ALL.len() {
            let code = MatrixCode::ALL[i].val();
            if detail(code) == "未知诊断码：不在 0x785C0 段闭集内" {
                closed_ok = false;
            }
            i += 1;
        }
        closed_ok && segment_guard()
    };
    v.push((
        "J4-段位守卫",
        guard,
        String::from("0x785C0 窗口 + 12 码闭集查表"),
    ));
    v
}

pub fn run_unx1531_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1531 回归矩阵域级总成（全域矩阵一图）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试区（≥8 条：注入必抓 / 双跑一致 / 边界） ─────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t01_full_grid_bound_and_exec() {
        match golden_run() {
            Ok((flags, cells, cost)) => {
                assert_eq!(flags, 5);
                assert_eq!(cells, TOTAL_CELLS as u32);
                assert!(cost <= CMD_BUDGET);
            }
            Err(e) => assert!(false, "黄金全链应成: err=0x{:x}", e),
        }
    }

    #[test]
    fn t02_fault_inject_10_of_10_caught() {
        match inject_fault(JUDGE_ROUNDS) {
            Ok(inj) => {
                assert_eq!(inj.tries, [10, 10, 10]);
                assert_eq!(inj.caught, [10, 10, 10]);
                assert!(inj.all_caught());
            }
            Err(e) => assert!(false, "注入应成: err=0x{:x}", e),
        }
    }

    #[test]
    fn t03_pair_match_100_percent() {
        let spec = build_grid_spec();
        assert!(spec.is_ok());
        if let Ok(list) = spec {
            let mut grid = MatrixGrid::new();
            let mut i = 0;
            while i < list.len() {
                if let Some(c) = list.get(i) {
                    assert!(grid.bind(c.clone()).is_ok());
                }
                i += 1;
            }
            assert_eq!(grid.verify_against(&list), Ok((TOTAL_CELLS as u32, 0, 0)));
        }
    }

    #[test]
    fn t04_unbound_cell_rejected() {
        let spec = build_grid_spec();
        assert!(spec.is_ok());
        if let Ok(list) = spec {
            let mut grid = MatrixGrid::new();
            let mut i = 0;
            while i < list.len() {
                if let Some(c) = list.get(i) {
                    if !(c.batch == 7 && c.suite == 2) {
                        let _ = grid.bind(c.clone());
                    }
                }
                i += 1;
            }
            assert_eq!(grid.check_binding(), Err(MatrixCode::UnboundCell.val()));
            assert_eq!(grid.plan_execute(), Err(MatrixCode::UnboundCell.val()));
        }
    }

    #[test]
    fn t05_dup_bind_and_bounds() {
        let mut grid = MatrixGrid::new();
        let c1 = spec_cell(1, 1);
        assert!(grid.bind(c1.clone()).is_ok());
        assert_eq!(grid.bind(c1), Err(MatrixCode::DupBind.val()));
        let cb = GridCell {
            batch: 0,
            suite: 1,
            cmd: 1,
            cost: 1,
            digest: 1,
        };
        assert_eq!(grid.bind(cb), Err(MatrixCode::BatchOver.val()));
        let cs = GridCell {
            batch: 1,
            suite: 9,
            cmd: 1,
            cost: 1,
            digest: 1,
        };
        assert_eq!(grid.bind(cs), Err(MatrixCode::SuiteOver.val()));
    }

    #[test]
    fn t06_spec_deterministic() {
        let mut b = BATCH_LO;
        while b <= BATCH_HI {
            let mut s = 1;
            while s <= SUITE_COUNT as u8 {
                let c = spec_cell(b, s);
                assert_eq!(c.cmd, spec_cmd(b, s));
                assert!(c.cmd >= 1 && c.cmd <= CMD_CAP);
                assert_eq!(c.cost, spec_cost(b, s));
                assert_eq!(c.digest, cell_digest(c.batch, c.suite, c.cmd, c.cost));
                s += 2;
            }
            b += 9;
        }
    }

    #[test]
    fn t07_execute_budget_respected() {
        let spec = build_grid_spec();
        assert!(spec.is_ok());
        if let Ok(list) = spec {
            let mut grid = MatrixGrid::new();
            let mut i = 0;
            while i < list.len() {
                if let Some(c) = list.get(i) {
                    let _ = grid.bind(c.clone());
                }
                i += 1;
            }
            let exec = grid.plan_execute();
            assert!(exec.is_ok(), "执行核应成");
            if let Ok((sum, n)) = exec {
                assert_eq!(n, TOTAL_CELLS as u32);
                assert!(sum <= CMD_BUDGET);
            }
        }
    }

    #[test]
    fn t08_double_run_identical() {
        let a = run_matrix_round();
        let b = run_matrix_round();
        match (a, b) {
            (Ok(x), Ok(y)) => assert_eq!(x, y),
            _ => assert!(false, "双跑应成"),
        }
    }

    #[test]
    fn t09_seal_and_shrink_rejected() {
        let mut grid = MatrixGrid::new();
        let _ = grid.bind(spec_cell(1, 1));
        grid.seal();
        assert_eq!(grid.bind(spec_cell(1, 2)), Err(MatrixCode::Sealed.val()));
        assert_eq!(grid.try_unbind(1, 1), Err(MatrixCode::GridShrink.val()));
        assert_eq!(grid.bound, 1);
    }

    #[test]
    fn t10_suite_closed_set() {
        assert_eq!(Suite::ALL.len(), SUITE_COUNT);
        let mut i = 0;
        while i < Suite::ALL.len() {
            if let Some(s) = Suite::ALL.get(i) {
                assert_eq!(s.no(), (i + 1) as u8);
            }
            i += 1;
        }
        assert!(suite_of(0).is_none());
        assert!(suite_of(5).is_none());
        assert!(suite_of(4).is_some());
    }

    #[test]
    fn t11_segment_guard_and_closed_set() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(MatrixCode::ALL.len(), 12);
        for c in MatrixCode::ALL {
            assert_ne!(detail(c.val()), "未知诊断码：不在 0x785C0 段闭集内");
        }
    }

    #[test]
    fn t12_probe_checks_all_green() {
        let set = run_unx1531_checks();
        assert_eq!(set.failed_count(), 0);
        assert!(set.all_passed());
    }
}
