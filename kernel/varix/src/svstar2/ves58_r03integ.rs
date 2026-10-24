//! VE-F3658 · R03 联调（VE-S 域 · 批次 R03 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3658`
//!
//! **职责定位（锚点原文）**：视觉创作组合矩阵联调——
//! - **24 格矩阵**：四工坊 × 三资产规模 × 两目标（应用/市场）（覆盖底线）；
//! - **真实剧本**：20 分钟工坊剧本（壁纸创作→取色→导出→上架→应用混流
//!   ——重演确定性 + 信号采集）；
//! - **故障注入**：功耗破门/隐写注入/伪 4K 三故障（门禁实测）；
//! - **跨组**：E 域产消 + Q 导出双联动实测；三分流复述。
//!
//! **判据（锚点原文六条）**：24 格、三门禁、跨组双联、
//! 重演确定、三分流、判据。
//!
//! **错误路径与降级矩阵（锚点原文四条 → 落位）**：
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 门禁漏检→P0（复述红线实测） | [`R58Code::GATE_MISS`] |
//! | 覆盖不达标→补跑（复述） | [`R58Code::COVERAGE_SHORT`] |
//! | 重演分歧→立案（复述） | [`R58Code::REPLAY_DIVERGED`] |
//! | 处置无责→兜底（复述） | [`R58Code::NO_OWNER`] |
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3658 诊断码段（独占 0x6Axx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct R58Code(pub u16);

impl R58Code {
    /// 门禁漏检（P0）。
    pub const GATE_MISS: R58Code = R58Code(0x6A01);
    /// 覆盖不达标（补跑）。
    pub const COVERAGE_SHORT: R58Code = R58Code(0x6A02);
    /// 重演分歧（立案）。
    pub const REPLAY_DIVERGED: R58Code = R58Code(0x6A03);
    /// 处置无责（兜底）。
    pub const NO_OWNER: R58Code = R58Code(0x6A04);

    pub const fn value(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            R58Code::GATE_MISS => "门禁漏检（P0 红线实测）。建议：三故障全部过门才放行。",
            R58Code::COVERAGE_SHORT => "覆盖不达标。建议：补跑缺失格。",
            R58Code::REPLAY_DIVERGED => "重演分歧。建议：立案归因剧本漂移。",
            R58Code::NO_OWNER => "处置无责。建议：兜底责任组。",
            _ => "未知诊断码。",
        }
    }
}

// ── 24 格矩阵 ───────────────────────────────────────────────────────

/// 工坊（四）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shop {
    /// 壁纸。
    Wall,
    /// 图标。
    Icon,
    /// 光标。
    Cursor,
    /// 锁屏。
    Lock,
}

impl Shop {
    pub const ALL: [Shop; 4] = [Shop::Wall, Shop::Icon, Shop::Cursor, Shop::Lock];
}

/// 资产规模（三）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Scale {
    /// 百级。
    S100,
    /// 千级。
    S1k,
    /// 万级。
    S10k,
}

impl Scale {
    pub const ALL: [Scale; 3] = [Scale::S100, Scale::S1k, Scale::S10k];
}

/// 目标（两）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Goal {
    /// 应用。
    App,
    /// 市场。
    Market,
}

impl Goal {
    pub const ALL: [Goal; 2] = [Goal::App, Goal::Market];
}

/// 矩阵格。
#[derive(Clone, Copy)]
pub struct Cell {
    pub shop: Shop,
    pub scale: Scale,
    pub goal: Goal,
    pub ran: bool,
}

// ── 故障注入 ────────────────────────────────────────────────────────

/// 三故障。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fault {
    /// 功耗破门。
    PowerBreak,
    /// 隐写注入。
    StegoInject,
    /// 伪 4K。
    Fake4k,
}

impl Fault {
    pub const ALL: [Fault; 3] = [Fault::PowerBreak, Fault::StegoInject, Fault::Fake4k];
}

/// 门禁结果。
#[derive(Clone, Copy)]
pub struct GateResult {
    pub fault: Fault,
    pub caught: bool,
}

// ── 联调执行器 ──────────────────────────────────────────────────────

/// R03 联调执行器。
pub struct R03Integration {
    cells: Vec<Cell>,
    gates: Vec<GateResult>,
    /// 剧本重演指纹（确定性对拍）。
    replay_fp: u32,
    runs: u32,
    owners_set: bool,
    last_code: R58Code,
    has_error: bool,
}

impl R03Integration {
    pub fn new() -> Self {
        Self {
            cells: Vec::new(),
            gates: Vec::new(),
            replay_fp: 0,
            runs: 0,
            owners_set: false,
            last_code: R58Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> R58Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    fn fail(&mut self, code: R58Code) {
        self.last_code = code;
        self.has_error = true;
    }

    /// 跑一格。
    pub fn run_cell(&mut self, shop: Shop, scale: Scale, goal: Goal) -> usize {
        self.cells.push(Cell { shop, scale, goal, ran: true });
        self.runs += 1;
        self.cells.len()
    }

    /// 24 格覆盖底线。
    pub fn coverage_ok(&mut self) -> Result<u32, R58Code> {
        let n = self.cells.iter().filter(|c| c.ran).count() as u32;
        if n >= 24 {
            self.last_code = R58Code(0);
            self.has_error = false;
            Ok(n)
        } else {
            self.fail(R58Code::COVERAGE_SHORT);
            Err(R58Code::COVERAGE_SHORT)
        }
    }

    /// 故障注入 + 门禁执法（漏检 = P0）。
    pub fn inject(&mut self, fault: Fault, caught: bool) -> Result<bool, R58Code> {
        self.gates.push(GateResult { fault, caught });
        if !caught {
            self.fail(R58Code::GATE_MISS);
            return Err(R58Code::GATE_MISS);
        }
        self.last_code = R58Code(0);
        self.has_error = false;
        Ok(true)
    }

    /// 三门禁全过。
    pub fn gates_green(&self) -> bool {
        Fault::ALL
            .iter()
            .all(|f| self.gates.iter().any(|g| g.fault == *f && g.caught))
    }

    /// 剧本重演：两次指纹必须一致。
    pub fn replay(&mut self, fp_a: u32, fp_b: u32) -> Result<u32, R58Code> {
        if fp_a != fp_b {
            self.fail(R58Code::REPLAY_DIVERGED);
            return Err(R58Code::REPLAY_DIVERGED);
        }
        self.replay_fp = fp_a;
        self.last_code = R58Code(0);
        self.has_error = false;
        Ok(fp_a)
    }

    pub fn replay_fp(&self) -> u32 {
        self.replay_fp
    }

    /// 跨组双联：E 产消 + Q 导出。
    pub fn cross_link(&mut self, eco_ok: bool, export_ok: bool) -> Result<bool, R58Code> {
        if eco_ok && export_ok {
            self.last_code = R58Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.fail(R58Code::GATE_MISS);
            Err(R58Code::GATE_MISS)
        }
    }

    /// 处置责任登记（无责兜底）。
    pub fn set_owner(&mut self, assigned: bool) -> Result<bool, R58Code> {
        self.owners_set = assigned;
        if assigned {
            self.last_code = R58Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.fail(R58Code::NO_OWNER);
            Err(R58Code::NO_OWNER)
        }
    }

    pub fn cell_count(&self) -> usize {
        self.cells.len()
    }

    pub fn gate_count(&self) -> usize {
        self.gates.len()
    }

    pub fn runs(&self) -> u32 {
        self.runs
    }

    /// 三分流总评：覆盖 + 门禁 + 有责。
    pub fn triage_green(&mut self) -> bool {
        self.coverage_ok().is_ok() && self.gates_green() && self.owners_set
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves58_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves58_r03integ");
    {
        let r = R03Integration::new();
        chk(&mut set, "C3658-01", r.cell_count() == 0 && r.gate_count() == 0, "err");
    }
    {
        let mut r = R03Integration::new();
        let n = r.run_cell(Shop::Wall, Scale::S100, Goal::App);
        chk(&mut set, "C3658-02", n == 1 && r.runs() == 1, "err");
    }
    {
        let mut r = R03Integration::new();
        for s in Shop::ALL.iter() {
            for sc in Scale::ALL.iter() {
                for g in Goal::ALL.iter() {
                    r.run_cell(*s, *sc, *g);
                }
            }
        }
        let n = r.coverage_ok();
        chk(&mut set, "C3658-03", n == Ok(24), "err");
    }
    {
        let mut r = R03Integration::new();
        r.run_cell(Shop::Wall, Scale::S100, Goal::App);
        let e = r.coverage_ok();
        chk(&mut set, "C3658-04", e.err() == Some(R58Code::COVERAGE_SHORT), "err");
    }
    {
        let mut r = R03Integration::new();
        let g = r.inject(Fault::PowerBreak, true);
        chk(&mut set, "C3658-05", g == Ok(true), "err");
    }
    {
        let mut r = R03Integration::new();
        let g = r.inject(Fault::StegoInject, false);
        chk(&mut set, "C3658-06", g.err() == Some(R58Code::GATE_MISS), "err");
    }
    {
        let mut r = R03Integration::new();
        for f in Fault::ALL.iter() {
            r.inject(*f, true);
        }
        chk(&mut set, "C3658-07", r.gates_green(), "err");
    }
    {
        let mut r = R03Integration::new();
        let res = r.replay(0xAA, 0xAA);
        chk(&mut set, "C3658-08", res == Ok(0xAA) && r.replay_fp() == 0xAA, "err");
    }
    {
        let mut r = R03Integration::new();
        let res = r.replay(0xAA, 0xBB);
        chk(&mut set, "C3658-09", res.err() == Some(R58Code::REPLAY_DIVERGED), "err");
    }
    {
        let mut r = R03Integration::new();
        let c = r.cross_link(true, true);
        chk(&mut set, "C3658-10", c == Ok(true), "err");
    }
    {
        let mut r = R03Integration::new();
        let c = r.cross_link(true, false);
        chk(&mut set, "C3658-11", c.err() == Some(R58Code::GATE_MISS), "err");
    }
    {
        let mut r = R03Integration::new();
        for s in Shop::ALL.iter() {
            for sc in Scale::ALL.iter() {
                for g in Goal::ALL.iter() {
                    r.run_cell(*s, *sc, *g);
                }
            }
        }
        for f in Fault::ALL.iter() {
            r.inject(*f, true);
        }
        r.set_owner(true);
        chk(&mut set, "C3658-12", r.triage_green(), "err");
    }
    set
}
