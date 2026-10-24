//! VE-F3679 · 个性化 fuzz（VE-S 域 · 批次 R04 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3679`
//!
//! **职责定位（锚点原文）**：个性化 fuzz——
//! - **数据 fuzz**：值变异 / 键变异 / 来源伪造 / 同步包畸形；
//! - **变异三级**：值变异（越界/类型错/巨量）、结构变异（键风暴/四源交叉写/重置风暴）、
//!   时序变异（同步×手设×场景竞态）；
//! - **四不变量**：主权（手设不被越）/ 可逆（可还原）/ 隐私（不上云）/ 禁令（无障碍不被覆盖）
//!   ——违例分级：主权/隐私/禁令 = P0（复述分级）；
//! - **最小化 + 闭环**复述。
//!
//! **判据（锚点原文）**：三变异、四不变量、违例分级、竞态检出、最小化、判据。
//!
//! **错误路径与降级矩阵**：
//! | 降级项 | 落位 |
//! |---|---|
//! | 不变量违例→分级立案（复述红线实测） | [`C79Code::INVARIANT_BREAK`] |
//! | 竞态检出→最小化（复述） | [`C79Code::RACE_FOUND`] |
//! | 挂起→看门狗（复述） | [`C79Code::HANG`] |
//! | 闭环缺→阻断（复述） | [`C79Code::LOOP_OPEN`] |
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3679 诊断码段（独占 0x7Fxx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct C79Code(pub u16);

impl C79Code {
    /// 不变量违例（分级立案）。
    pub const INVARIANT_BREAK: C79Code = C79Code(0x7F01);
    /// 竞态检出。
    pub const RACE_FOUND: C79Code = C79Code(0x7F02);
    /// 挂起（看门狗）。
    pub const HANG: C79Code = C79Code(0x7F03);
    /// 闭环缺。
    pub const LOOP_OPEN: C79Code = C79Code(0x7F04);

    pub const fn value(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            C79Code::INVARIANT_BREAK => "不变量违例（分级立案）。建议：按 P 级处置。",
            C79Code::RACE_FOUND => "竞态检出。建议：最小化复现。",
            C79Code::HANG => "挂起。建议：看门狗回收。",
            C79Code::LOOP_OPEN => "闭环缺。建议：阻断放行。",
            _ => "未知诊断码。",
        }
    }
}

// ── 变异三级 ────────────────────────────────────────────────────────

/// 变异级。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MutationLevel {
    /// 值变异（越界/类型错/巨量）。
    Value,
    /// 结构变异（键风暴/四源交叉写/重置风暴）。
    Structure,
    /// 时序变异（同步×手设×场景竞态）。
    Timing,
}

impl MutationLevel {
    pub const ALL: [MutationLevel; 3] = [
        MutationLevel::Value,
        MutationLevel::Structure,
        MutationLevel::Timing,
    ];
}

/// fuzz 案例。
#[derive(Clone, Copy)]
pub struct FuzzCase {
    pub id: u32,
    pub level: MutationLevel,
    /// 是否触发竞态。
    pub race: bool,
    /// 是否挂起。
    pub hang: bool,
}

// ── 语料库 ──────────────────────────────────────────────────────────

/// fuzz 语料库。
pub struct Corpus {
    cases: Vec<FuzzCase>,
    next_id: u32,
}

impl Corpus {
    pub fn new() -> Self {
        Self { cases: Vec::new(), next_id: 1 }
    }

    /// 生成一条案例。
    pub fn spawn(&mut self, level: MutationLevel, race: bool, hang: bool) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        self.cases.push(FuzzCase { id, level, race, hang });
        id
    }

    /// 三级齐备。
    pub fn levels_covered(&self) -> bool {
        MutationLevel::ALL
            .iter()
            .all(|l| self.cases.iter().any(|c| c.level == *l))
    }

    pub fn count(&self) -> usize {
        self.cases.len()
    }

    pub fn race_count(&self) -> usize {
        self.cases.iter().filter(|c| c.race).count()
    }

    pub fn hang_count(&self) -> usize {
        self.cases.iter().filter(|c| c.hang).count()
    }

    pub fn get(&self, id: u32) -> Option<&FuzzCase> {
        self.cases.iter().find(|c| c.id == id)
    }

    /// 最小化：去掉重复 level+race+hang 签名的冗余。
    pub fn minimize(&mut self) -> usize {
        let before = self.cases.len();
        let mut seen: Vec<(u8, bool, bool)> = Vec::new();
        self.cases.retain(|c| {
            let sig = (c.level as u8, c.race, c.hang);
            if seen.contains(&sig) {
                false
            } else {
                seen.push(sig);
                true
            }
        });
        before - self.cases.len()
    }
}

// ── 四不变量 ────────────────────────────────────────────────────────

/// 不变量。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Invariant {
    /// 主权（手设不被越）——P0。
    Sovereignty,
    /// 可逆（可还原）——P1。
    Reversible,
    /// 隐私（不上云）——P0。
    Privacy,
    /// 禁令（无障碍不被覆盖）——P0。
    BanA11y,
}

impl Invariant {
    pub const ALL: [Invariant; 4] = [
        Invariant::Sovereignty,
        Invariant::Reversible,
        Invariant::Privacy,
        Invariant::BanA11y,
    ];

    /// 违例分级：主权/隐私/禁令 = P0，可逆 = P1。
    pub const fn severity(self) -> u8 {
        match self {
            Invariant::Sovereignty => 0,
            Invariant::Reversible => 1,
            Invariant::Privacy => 0,
            Invariant::BanA11y => 0,
        }
    }
}

/// 违例记录。
#[derive(Clone, Copy)]
pub struct Violation {
    pub inv: Invariant,
    pub case_id: u32,
}

/// 不变量守卫（违例分级）。
pub struct InvariantGuard {
    violations: Vec<Violation>,
    last_code: C79Code,
    has_error: bool,
}

impl InvariantGuard {
    pub fn new() -> Self {
        Self {
            violations: Vec::new(),
            last_code: C79Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C79Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    /// 上报违例。
    pub fn violate(&mut self, inv: Invariant, case_id: u32) -> Result<u8, C79Code> {
        self.violations.push(Violation { inv, case_id });
        self.last_code = C79Code::INVARIANT_BREAK;
        self.has_error = true;
        Err(C79Code::INVARIANT_BREAK)
    }

    /// 无违例断言。
    pub fn clean(&mut self) -> Result<bool, C79Code> {
        if self.violations.is_empty() {
            self.last_code = C79Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.last_code = C79Code::INVARIANT_BREAK;
            self.has_error = true;
            Err(C79Code::INVARIANT_BREAK)
        }
    }

    /// P0 违例计数。
    pub fn p0_count(&self) -> usize {
        self.violations.iter().filter(|v| v.inv.severity() == 0).count()
    }

    /// P1 违例计数。
    pub fn p1_count(&self) -> usize {
        self.violations.iter().filter(|v| v.inv.severity() == 1).count()
    }

    pub fn violation_count(&self) -> usize {
        self.violations.len()
    }
}

// ── 竞态与看门狗 ────────────────────────────────────────────────────

/// 竞态检测器 + 看门狗。
pub struct RaceWatchdog {
    races: u32,
    hangs: u32,
    /// 看门狗预算（步）。
    budget: u32,
    steps: u32,
    last_code: C79Code,
    has_error: bool,
}

impl RaceWatchdog {
    pub fn new(budget: u32) -> Self {
        Self {
            races: 0,
            hangs: 0,
            budget,
            steps: 0,
            last_code: C79Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C79Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    /// 步进（看门狗计数）。
    pub fn step(&mut self) -> Result<bool, C79Code> {
        self.steps += 1;
        if self.steps > self.budget {
            self.hangs += 1;
            self.last_code = C79Code::HANG;
            self.has_error = true;
            return Err(C79Code::HANG);
        }
        self.last_code = C79Code(0);
        self.has_error = false;
        Ok(true)
    }

    /// 上报竞态。
    pub fn note_race(&mut self) {
        self.races += 1;
        self.last_code = C79Code::RACE_FOUND;
        self.has_error = true;
    }

    /// 重置步计数（回收）。
    pub fn reset(&mut self) {
        self.steps = 0;
    }

    pub fn races(&self) -> u32 {
        self.races
    }

    pub fn hangs(&self) -> u32 {
        self.hangs
    }

    pub fn steps(&self) -> u32 {
        self.steps
    }
}

// ── 闭环 ────────────────────────────────────────────────────────────

/// fuzz 闭环（最小化+闭环复述）。
pub struct FuzzLoop {
    open: bool,
    cases_run: u32,
    loop_open_hits: u32,
    last_code: C79Code,
    has_error: bool,
}

impl FuzzLoop {
    pub fn new() -> Self {
        Self {
            open: false,
            cases_run: 0,
            loop_open_hits: 0,
            last_code: C79Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C79Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    /// 打开闭环。
    pub fn open(&mut self) {
        self.open = true;
    }

    /// 关闭闭环。
    pub fn close(&mut self) {
        self.open = false;
    }

    /// 跑一例（闭环必须开）。
    pub fn run_case(&mut self) -> Result<u32, C79Code> {
        if !self.open {
            self.loop_open_hits += 1;
            self.last_code = C79Code::LOOP_OPEN;
            self.has_error = true;
            return Err(C79Code::LOOP_OPEN);
        }
        self.cases_run += 1;
        self.last_code = C79Code(0);
        self.has_error = false;
        Ok(self.cases_run)
    }

    pub fn cases_run(&self) -> u32 {
        self.cases_run
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn loop_open_hits(&self) -> u32 {
        self.loop_open_hits
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves79_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves79_prefuzz");
    {
        let mut c = Corpus::new();
        c.spawn(MutationLevel::Value, false, false);
        c.spawn(MutationLevel::Structure, true, false);
        c.spawn(MutationLevel::Timing, true, true);
        chk(&mut set, "C3679-01", c.levels_covered() && c.count() == 3, "err");
    }
    {
        let mut c = Corpus::new();
        c.spawn(MutationLevel::Value, true, true);
        c.spawn(MutationLevel::Value, true, true);
        c.spawn(MutationLevel::Value, true, true);
        let n = c.minimize();
        chk(&mut set, "C3679-02", n == 2 && c.count() == 1, "err");
    }
    {
        let mut c = Corpus::new();
        c.spawn(MutationLevel::Value, true, false);
        c.spawn(MutationLevel::Value, false, true);
        chk(&mut set, "C3679-03", c.race_count() == 1 && c.hang_count() == 1, "err");
    }
    {
        let mut g = InvariantGuard::new();
        let res = g.clean();
        chk(&mut set, "C3679-04", res == Ok(true), "err");
    }
    {
        let mut g = InvariantGuard::new();
        let res = g.violate(Invariant::Sovereignty, 1);
        chk(
            &mut set,
            "C3679-05",
            res.is_err() && g.p0_count() == 1 && g.p1_count() == 0,
            "err",
        );
    }
    {
        let mut g = InvariantGuard::new();
        g.violate(Invariant::Reversible, 1);
        chk(&mut set, "C3679-06", g.p1_count() == 1 && g.p0_count() == 0, "err");
    }
    {
        let mut g = InvariantGuard::new();
        g.violate(Invariant::Privacy, 1);
        g.violate(Invariant::BanA11y, 2);
        chk(&mut set, "C3679-07", g.p0_count() == 2, "err");
    }
    {
        let mut w = RaceWatchdog::new(3);
        w.step();
        w.step();
        let res = w.step();
        chk(&mut set, "C3679-08", res == Ok(true) && w.steps() == 3, "err");
    }
    {
        let mut w = RaceWatchdog::new(2);
        w.step();
        w.step();
        let res = w.step();
        chk(&mut set, "C3679-09", res.err() == Some(C79Code::HANG) && w.hangs() == 1, "err");
    }
    {
        let mut w = RaceWatchdog::new(10);
        w.note_race();
        chk(&mut set, "C3679-10", w.races() == 1 && w.has_error(), "err");
    }
    {
        let mut l = FuzzLoop::new();
        let res = l.run_case();
        chk(&mut set, "C3679-11", res.err() == Some(C79Code::LOOP_OPEN), "err");
    }
    {
        let mut l = FuzzLoop::new();
        l.open();
        l.run_case();
        l.run_case();
        chk(&mut set, "C3679-12", l.cases_run() == 2 && l.is_open(), "err");
    }
    set
}
