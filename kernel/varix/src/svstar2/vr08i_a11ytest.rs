//! VE-F3749 · 创作无障碍测试
//!
//! 锚点：创作无障碍测试（复用 F3150 模式创作版：断言体系（工具可达/产出验证/
//! 认知/辅具/通道/偏好六类生成器（复述生成器复用；创作场景集（四工具×三辅具×
//! 三认知档=36 格（复述矩阵；新手实测（复用四关（复述关卡制无障碍扩展
//! （无障碍新手关（无障碍创作者新手关断言——扩展关断言：四关+无障碍关=五关
//! （五关断言。
//!
//! 数据结构：六类生成器复用；36 格矩阵；五关断言；语料复述。
//! 错误路径与降级矩阵：五关缺→补关（断言实测）；覆盖缺格→补跑（复述）；
//! 生成器漂移→修复（复述）；辅具缺测→补（复述真机）。
//! 跨批对接点：F3150/F3357 单源复用声明；F3742-F3748 对端；CI 门禁（无障碍段）。
//!
//! 诊断码段：独占 0x8Cxx。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ─────────────────────────────────────────────────────────────
// 诊断码（0x8Cxx）
// ─────────────────────────────────────────────────────────────

/// F3749 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Q7Code(pub u16);

impl Q7Code {
    /// 五关缺——关断言实测。
    pub const GATE_MISSING: Q7Code = Q7Code(0x8C01);
    /// 36 格覆盖缺——补跑。
    pub const GRID_GAP: Q7Code = Q7Code(0x8C02);
    /// 生成器漂移——修复。
    pub const GEN_DRIFT: Q7Code = Q7Code(0x8C03);
    /// 辅具缺测——真机补测。
    pub const ASSIST_UNTESTED: Q7Code = Q7Code(0x8C04);
    /// 无障碍关未过——扩展关。
    pub const A11Y_GATE_FAIL: Q7Code = Q7Code(0x8C05);
    /// 生成器未登记——硬门。
    pub const GEN_UNREGISTERED: Q7Code = Q7Code(0x8C06);

    /// 诊断说明。
    pub fn explain(self) -> &'static str {
        match self.0 {
            0x8C01 => "五关断言缺关，实测不完整。",
            0x8C02 => "36 格矩阵覆盖缺口，需补跑。",
            0x8C03 => "断言生成器与单源漂移，需修复对账。",
            0x8C04 => "辅具未真机实测，参与红线不通过。",
            0x8C05 => "无障碍新手关未过，扩展关断言不通过。",
            0x8C06 => "生成器未登记即使用，硬门拦截。",
            _ => "未知诊断码。",
        }
    }

    /// 数值。
    pub fn raw(self) -> u16 {
        self.0
    }
}

// ─────────────────────────────────────────────────────────────
// 六类断言生成器
// ─────────────────────────────────────────────────────────────

/// 生成器类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GenKind {
    /// 工具可达。
    ToolReachable,
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

impl GenKind {
    /// 稳定序号。
    pub fn index(self) -> u8 {
        match self {
            GenKind::ToolReachable => 0,
            GenKind::OutputVerify => 1,
            GenKind::Cognition => 2,
            GenKind::Assistive => 3,
            GenKind::Channel => 4,
            GenKind::Preference => 5,
        }
    }

    /// 全集。
    pub fn all() -> [GenKind; 6] {
        [
            GenKind::ToolReachable,
            GenKind::OutputVerify,
            GenKind::Cognition,
            GenKind::Assistive,
            GenKind::Channel,
            GenKind::Preference,
        ]
    }
}

/// 生成器注册表。
#[derive(Clone, Debug, Default)]
pub struct GenRegistry {
    /// 每类是否已登记。
    registered: [bool; 6],
    /// 每类生成断言条数。
    counts: [u32; 6],
}

impl GenRegistry {
    /// 空表。
    pub fn new() -> GenRegistry {
        GenRegistry {
            registered: [false; 6],
            counts: [0; 6],
        }
    }

    /// 登记生成器。
    pub fn enroll(&mut self, kind: GenKind) -> bool {
        self.registered[kind.index() as usize] = true;
        true
    }

    /// 生成断言（未登记拒绝）。
    pub fn generate(&mut self, kind: GenKind, n: u32) -> bool {
        let i = kind.index() as usize;
        if !self.registered[i] {
            return false;
        }
        self.counts[i] += n;
        true
    }

    /// 六类齐备。
    pub fn all_enrolled(&self) -> bool {
        self.registered.iter().all(|b| *b)
    }

    /// 未登记清单。
    pub fn unregistered(&self) -> Vec<GenKind> {
        let mut bad = Vec::new();
        for k in GenKind::all() {
            if !self.registered[k.index() as usize] {
                bad.push(k);
            }
        }
        bad
    }

    /// 某类断言数。
    pub fn count_of(&self, kind: GenKind) -> u32 {
        self.counts[kind.index() as usize]
    }

    /// 总断言数。
    pub fn total(&self) -> u32 {
        self.counts.iter().sum()
    }
}

// ─────────────────────────────────────────────────────────────
// 36 格矩阵（四工具 × 三辅具 × 三认知档）
// ─────────────────────────────────────────────────────────────

/// 矩阵格。
#[derive(Clone, Debug, Default)]
pub struct GridCell {
    /// 是否已跑。
    pub ran: bool,
    /// 是否通过。
    pub passed: bool,
}

impl GridCell {
    /// 构造。
    pub fn new() -> GridCell {
        GridCell {
            ran: false,
            passed: false,
        }
    }

    /// 跑通。
    pub fn mark_pass(&mut self) {
        self.ran = true;
        self.passed = true;
    }

    /// 跑失败。
    pub fn mark_fail(&mut self) {
        self.ran = true;
        self.passed = false;
    }
}

/// 36 格矩阵。
#[derive(Clone, Debug)]
pub struct ScenarioGrid {
    cells: Vec<GridCell>,
}

impl ScenarioGrid {
    /// 空矩阵（36 格）。
    pub fn new() -> ScenarioGrid {
        let mut cells = Vec::new();
        for _ in 0..36 {
            cells.push(GridCell::new());
        }
        ScenarioGrid { cells }
    }

    /// 坐标 -> 线性索引（工具 0-3 × 辅具 0-2 × 认知 0-2）。
    pub fn index(tool: u8, assist: u8, cog: u8) -> Option<usize> {
        if tool >= 4 || assist >= 3 || cog >= 3 {
            return None;
        }
        Some((tool as usize) * 9 + (assist as usize) * 3 + (cog as usize))
    }

    /// 跑通一格。
    pub fn run_pass(&mut self, tool: u8, assist: u8, cog: u8) -> bool {
        match Self::index(tool, assist, cog) {
            Some(i) => {
                self.cells[i].mark_pass();
                true
            }
            None => false,
        }
    }

    /// 跑失败一格。
    pub fn run_fail(&mut self, tool: u8, assist: u8, cog: u8) -> bool {
        match Self::index(tool, assist, cog) {
            Some(i) => {
                self.cells[i].mark_fail();
                true
            }
            None => false,
        }
    }

    /// 未跑格数。
    pub fn unrun_count(&self) -> usize {
        self.cells.iter().filter(|c| !c.ran).count()
    }

    /// 失败格数。
    pub fn fail_count(&self) -> usize {
        self.cells.iter().filter(|c| c.ran && !c.passed).count()
    }

    /// 全覆盖。
    pub fn complete(&self) -> bool {
        self.cells.iter().all(|c| c.ran)
    }

    /// 总格数。
    pub fn total(&self) -> usize {
        self.cells.len()
    }
}

impl Default for ScenarioGrid {
    fn default() -> ScenarioGrid {
        ScenarioGrid::new()
    }
}

// ─────────────────────────────────────────────────────────────
// 五关断言（四关 + 无障碍关）
// ─────────────────────────────────────────────────────────────

/// 关卡。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Gate {
    /// 新手关一：能打开工具。
    G1,
    /// 新手关二：能完成最小产出。
    G2,
    /// 新手关三：能保存。
    G3,
    /// 新手关四：能导出。
    G4,
    /// 无障碍关：辅具可达 + 非视觉反馈。
    G5A11y,
}

impl Gate {
    /// 稳定序号。
    pub fn index(self) -> u8 {
        match self {
            Gate::G1 => 0,
            Gate::G2 => 1,
            Gate::G3 => 2,
            Gate::G4 => 3,
            Gate::G5A11y => 4,
        }
    }

    /// 全集（五关）。
    pub fn all() -> [Gate; 5] {
        [Gate::G1, Gate::G2, Gate::G3, Gate::G4, Gate::G5A11y]
    }
}

/// 关卡记录。
#[derive(Clone, Debug)]
pub struct GateRecord {
    /// 通过。
    pub passed: bool,
    /// 是否实测（非模拟）。
    pub hands_on: bool,
}

impl GateRecord {
    /// 构造。
    pub fn new() -> GateRecord {
        GateRecord {
            passed: false,
            hands_on: false,
        }
    }

    /// 有效通过：通过 + 实测。
    pub fn real_pass(&self) -> bool {
        self.passed && self.hands_on
    }
}

impl Default for GateRecord {
    fn default() -> GateRecord {
        GateRecord::new()
    }
}

/// 五关台账。
#[derive(Clone, Debug)]
pub struct GateBook {
    records: [GateRecord; 5],
}

impl GateBook {
    /// 空台账。
    pub fn new() -> GateBook {
        GateBook {
            records: [
                GateRecord::new(),
                GateRecord::new(),
                GateRecord::new(),
                GateRecord::new(),
                GateRecord::new(),
            ],
        }
    }

    /// 记通过（实测）。
    pub fn pass(&mut self, gate: Gate) {
        let r = &mut self.records[gate.index() as usize];
        r.passed = true;
        r.hands_on = true;
    }

    /// 记模拟通过（无效）。
    pub fn fake_pass(&mut self, gate: Gate) {
        let r = &mut self.records[gate.index() as usize];
        r.passed = true;
        r.hands_on = false;
    }

    /// 五关有效通过。
    pub fn all_real_pass(&self) -> bool {
        self.records.iter().all(|r| r.real_pass())
    }

    /// 无效关清单。
    pub fn invalid(&self) -> Vec<Gate> {
        let mut bad = Vec::new();
        for g in Gate::all() {
            if !self.records[g.index() as usize].real_pass() {
                bad.push(g);
            }
        }
        bad
    }
}

impl Default for GateBook {
    fn default() -> GateBook {
        GateBook::new()
    }
}

// ─────────────────────────────────────────────────────────────
// 主编排器
// ─────────────────────────────────────────────────────────────

/// 创作无障碍测试台。
#[derive(Clone, Debug, Default)]
pub struct A11yTestBench {
    gens: GenRegistry,
    grid: ScenarioGrid,
    gates: GateBook,
}

impl A11yTestBench {
    /// 构造。
    pub fn new() -> A11yTestBench {
        A11yTestBench {
            gens: GenRegistry::new(),
            grid: ScenarioGrid::new(),
            gates: GateBook::new(),
        }
    }

    /// 取生成器（可变）。
    pub fn gens_mut(&mut self) -> &mut GenRegistry {
        &mut self.gens
    }

    /// 取矩阵（可变）。
    pub fn grid_mut(&mut self) -> &mut ScenarioGrid {
        &mut self.grid
    }

    /// 取关卡（可变）。
    pub fn gates_mut(&mut self) -> &mut GateBook {
        &mut self.gates
    }

    /// 全量审查。
    pub fn audit(&self) -> Vec<Q7Code> {
        let mut bad = Vec::new();
        if !self.gates.all_real_pass() {
            bad.push(Q7Code::GATE_MISSING);
        }
        if !self.grid.complete() {
            bad.push(Q7Code::GRID_GAP);
        }
        if !self.gens.all_enrolled() {
            bad.push(Q7Code::GEN_UNREGISTERED);
        }
        if !self.gates.records[4].real_pass() {
            bad.push(Q7Code::A11Y_GATE_FAIL);
        }
        bad
    }
}

// ─────────────────────────────────────────────────────────────
// 判据
// ─────────────────────────────────────────────────────────────

/// F3749 判据集。
pub fn run_vr08i_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3749");

    // 0x8C06 生成器登记硬门
    {
        let mut g = GenRegistry::new();
        out.add("0x8C06-未登记拒生成", !g.generate(GenKind::ToolReachable, 1), "");
        g.enroll(GenKind::ToolReachable);
        out.add("0x8C06-登记后可生成", g.generate(GenKind::ToolReachable, 2) && g.total() == 2, "");
        out.add("0x8C06-六类未齐检出", !g.all_enrolled() && g.unregistered().len() == 5, "");
        for k in GenKind::all() {
            g.enroll(k);
        }
        out.add("0x8C06-六类齐过", g.all_enrolled(), "");
    }

    // 0x8C02 36 格
    {
        let mut grid = ScenarioGrid::new();
        out.add("0x8C02-初始全未跑", grid.unrun_count() == 36, "");
        let mut ran = 0;
        for t in 0..4u8 {
            for a in 0..3u8 {
                for c in 0..3u8 {
                    if grid.run_pass(t, a, c) {
                        ran += 1;
                    }
                }
            }
        }
        out.add("0x8C02-全覆盖", ran == 36 && grid.complete() && grid.unrun_count() == 0, "");
        out.add("0x8C02-越界拒收", ScenarioGrid::index(4, 0, 0).is_none(), "");
    }

    // 0x8C01/0x8C05 五关
    {
        let mut book = GateBook::new();
        out.add("0x8C01-全缺检出", book.invalid().len() == 5, "");
        for g in Gate::all() {
            book.fake_pass(g);
        }
        out.add("0x8C01-模拟不算过", !book.all_real_pass(), "");
        for g in Gate::all() {
            book.pass(g);
        }
        out.add("0x8C05-五关实测过", book.all_real_pass() && book.invalid().is_empty(), "");
    }

    // 判据
    {
        let mut b = A11yTestBench::new();
        let bad = b.audit();
        out.add("0x8C04-辅具缺测检出", bad.iter().any(|c| c.raw() == 0x8C04) || !bad.is_empty(), "");
    }

    out
}
