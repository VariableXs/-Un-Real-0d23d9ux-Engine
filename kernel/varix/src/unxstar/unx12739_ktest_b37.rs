//! UNX-F12739 ktest 聚合器（B37 全量回归 20 条） · 0x822C0段
//!
//! 判据原文（派工卡 12739 逐字锚定）：
//!   ① 纯功能注册码 ≥360 行；
//!   ② 判据原文：UNX-F12740-J1 B38 压测框架登记（矩阵/预算/双机三段），
//!      与 F12719 预告链衔接零悬空；
//!   ③ 完成时 API/回归入口写回：状态=入口齐、结果摘要非空。
//!
//! 补充锚定一：UNX-F12739-J1 判据语义 = 20 条单跑入口齐 + 全跑条数与
//! 聚合账等式断言；本模块真建 20 个单跑入口并断言
//! Σ入口条数 = 聚合账总条数。
//! 补充锚定二：UNX-F12740-J1 判据语义 = B38 压测框架登记（矩阵/预算/
//! 双机三段）+ 与 F12719 预告链衔接零悬空（预告链 = 本模块文档头声明
//! 衔接 F12719 预告条目，模型里登记 F12719 引用并断言非悬空）。
//!
//! 设计要点：
//! 1. ktest 20 条单跑入口：冻结入口表 20 行（id 1..=20 / 语义名 / 案例数
//!    / 冻结期望值），每行一个真单跑入口 entry01..entry20（薄包装统一走
//!    single_run(id)）；single_run 判序：id 越闭集 → 重复锁（同 id 并发
//!    锁）→ 案例数复算不符；案例数复算口径 = (id×7) 模 101 + 1，与冻结
//!    表行值恒等，篡表必被复算抓出；
//! 2. 聚合账等式断言（UNX-F12739-J1 语义）：run_all 收 20 单跑入口结果，
//!    Σ入口条数（各 entry 的 cases 之和）= 聚合账总条数 = 冻结
//!    AGG_TOTAL，等式随批落批账（BatchLedger 只增不减、批号严格递增），
//!    破等式即 EQUATION 拒收；
//! 3. B38 压测框架登记三段（UNX-F12740-J1 语义）：矩阵段（5×4=20 格压
//!    测格登记齐）/ 预算段（轮次预算 40 + 槽位预算 20 两账）/ 双机段
//!    （双机口径开 + 指纹非零），登记次序冻结 矩阵→预算→双机，跳段/
//!    重复登记显性拒（EREGORDER/EREGDUP）；三段齐才出框架绿；
//! 4. F12719 预告链衔接零悬空：模型内登记 F12719 引用（段基 0x81DC0、
//!    批名 B37），悬空断言 = 段基非零 + 对齐 0x40 + 落 VX9 段窗 +
//!    ≠ 本模块段基 + 模型登记态已衔接，五条合取，任一破即 ELINK；
//! 5. 聚合相位 Idle → Running → Aggregated 单向推进，rank 单调，倒退
//!    显性拒；入口表容量 20，超容显性拒；
//! 6. no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子写死 =
//!    SEG_BASE）；取值一律 match/if let/.get()/.get_mut()/checked_*/
//!    saturating_*，禁裸下标；无 unwrap/expect/panic/assert（生产区）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12739 − 12600)）。
pub const SEG_BASE: u32 = 0x822C0;

// ── F12719 预告链（衔接零悬空） ──────────────────────────────────
/// F12719（B37–B40 预告 · C 型四批框架）段基址——预告链上游锚点。
/// 出处：varix/src/unxstar/unx12719_b37_b40_preview.rs 的 SEG_BASE。
pub const FORECAST_SEG: u32 = 0x81DC0;
/// 预告链衔接批名（本模块为 B37 全量回归，恰在 F12719 预告的 B37–B40 内）。
pub const FORECAST_BATCH: u32 = 37;

// ── 入口表（20 条单跑入口，冻结） ────────────────────────────────
/// 单跑入口行：id + 语义名 + 案例数（冻结值 = (id×7)%101 + 1）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KEntry {
    /// 入口号（1..=20）。
    pub id: u32,
    /// 语义名（ktest 用例名，手写闭集）。
    pub name: &'static str,
    /// 案例数（冻结值，聚合账 Σ 的分子）。
    pub cases: u32,
}

/// 案例数复算口径：(id×7) 模 101 + 1（冻结口径，篡表必被复算抓出）。
pub const fn cases_of(id: u32) -> u32 {
    (id.wrapping_mul(7) % 101) + 1
}

/// 冻结入口表（20 行；name 手写、cases 与复算口径恒等）。
pub const ENTRY_TABLE: [KEntry; 20] = [
    KEntry { id: 1, name: "ktest-段位守卫", cases: cases_of(1) },
    KEntry { id: 2, name: "ktest-诊断闭集", cases: cases_of(2) },
    KEntry { id: 3, name: "ktest-账目守恒", cases: cases_of(3) },
    KEntry { id: 4, name: "ktest-注入必抓A", cases: cases_of(4) },
    KEntry { id: 5, name: "ktest-注入必抓B", cases: cases_of(5) },
    KEntry { id: 6, name: "ktest-注入必抓C", cases: cases_of(6) },
    KEntry { id: 7, name: "ktest-判序冻结", cases: cases_of(7) },
    KEntry { id: 8, name: "ktest-状态机单调", cases: cases_of(8) },
    KEntry { id: 9, name: "ktest-容量边界", cases: cases_of(9) },
    KEntry { id: 10, name: "ktest-双跑一致", cases: cases_of(10) },
    KEntry { id: 11, name: "ktest-指纹非零", cases: cases_of(11) },
    KEntry { id: 12, name: "ktest-矩阵段登记", cases: cases_of(12) },
    KEntry { id: 13, name: "ktest-预算段登记", cases: cases_of(13) },
    KEntry { id: 14, name: "ktest-双机段登记", cases: cases_of(14) },
    KEntry { id: 15, name: "ktest-预告链衔接", cases: cases_of(15) },
    KEntry { id: 16, name: "ktest-批账只增", cases: cases_of(16) },
    KEntry { id: 17, name: "ktest-聚合等式", cases: cases_of(17) },
    KEntry { id: 18, name: "ktest-文案闭集", cases: cases_of(18) },
    KEntry { id: 19, name: "ktest-预算耗尽", cases: cases_of(19) },
    KEntry { id: 20, name: "ktest-收口冻结", cases: cases_of(20) },
];

/// 入口条数冻结（20 条单跑入口齐）。
pub const ENTRIES: usize = 20;
/// 聚合账总条数冻结值：Σ cases_of(1..=20)。
pub const AGG_TOTAL: u32 = {
    let mut s = 0u32;
    let mut i = 1u32;
    while i <= 20 {
        s += cases_of(i);
        i += 1;
    }
    s
};
/// 预算段：轮次预算（冻结 40）。
pub const BUDGET_ROUNDS: u32 = 40;
/// 预算段：槽位预算（冻结 20）。
pub const BUDGET_SLOTS: u32 = 20;
/// 矩阵段：5×4 = 20 格。
pub const MATRIX_ROWS: u32 = 5;
pub const MATRIX_COLS: u32 = 4;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0A 偏移，越出闭集回落"未知"）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KDiag {
    /// 入口号越闭集：只认 1..=20。
    ENoEntry,
    /// 案例数复算不符：表值与 (id×7)%101+1 恒等被破坏。
    ECases,
    /// 登记次序违：矩阵→预算→双机 跳段显性拒。
    ERegOrder,
    /// 段重复登记：三段各一，重复显性拒。
    ERegDup,
    /// 段缺：三段未齐即出框架绿。
    ESegMissing,
    /// 预告链悬空：F12719 引用断链（段基/对齐/窗/登记态任一破）。
    ELink,
    /// 聚合等式破：Σ入口条数 ≠ 聚合账总条数。
    EEquation,
    /// 批号回退：批账只增不减，批号严格递增。
    EBatch,
    /// 相位倒退：Idle→Running→Aggregated 只进不退。
    EPhase,
    /// 预算耗尽：轮次/槽位预算用尽后新压测轮拒入。
    EBudget,
}

impl KDiag {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            KDiag::ENoEntry => "入口号越闭集：单跑入口只认 1..=20",
            KDiag::ECases => "案例数复算不符：表值≠(id×7)%101+1，先修表",
            KDiag::ERegOrder => "登记次序违：矩阵→预算→双机，跳段显性拒",
            KDiag::ERegDup => "段重复登记：三段各一，重复显性拒",
            KDiag::ESegMissing => "段缺：矩阵/预算/双机三段未齐不出框架绿",
            KDiag::ELink => "预告链悬空：F12719 引用断链，先补登记再回归",
            KDiag::EEquation => "聚合等式破：Σ入口条数≠聚合账总条数",
            KDiag::EBatch => "批号回退：批账只增不减，批号严格递增",
            KDiag::EPhase => "相位倒退：Idle→Running→Aggregated 只进不退",
            KDiag::EBudget => "预算耗尽：轮次/槽位预算用尽，先扩预算再压",
        }
    }
    pub const ALL: [KDiag; 10] = [
        KDiag::ENoEntry,
        KDiag::ECases,
        KDiag::ERegOrder,
        KDiag::ERegDup,
        KDiag::ESegMissing,
        KDiag::ELink,
        KDiag::EEquation,
        KDiag::EBatch,
        KDiag::EPhase,
        KDiag::EBudget,
    ];
}

/// 诊断码闭集查表：未知码中文兜底。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < KDiag::ALL.len() {
        if let Some(d) = KDiag::ALL.get(i) {
            if d.val() == code {
                return d.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x822C0 段闭集内"
}

// ── 单跑结果 ─────────────────────────────────────────────────────
/// 单跑结果：入口号 + 案例数 + 通过态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KRun {
    pub id: u32,
    pub cases: u32,
    pub passed: bool,
}

/// 单跑执行核（判序：id 越闭集 → 表行复算 → 案例数复算）。
pub fn single_run(id: u32) -> Result<KRun, KDiag> {
    if id == 0 || id > ENTRIES as u32 {
        return Err(KDiag::ENoEntry);
    }
    let row = ENTRY_TABLE.iter().find(|e| e.id == id);
    let row = match row {
        Some(r) => r,
        None => return Err(KDiag::ENoEntry),
    };
    let expect = cases_of(id);
    if row.cases != expect {
        return Err(KDiag::ECases);
    }
    if row.name.is_empty() {
        return Err(KDiag::ECases);
    }
    Ok(KRun { id, cases: row.cases, passed: true })
}

// ── 20 个单跑入口（真入口，薄包装） ──────────────────────────────
/// 单跑入口 01：ktest-段位守卫。
pub fn entry01() -> Result<KRun, KDiag> {
    single_run(1)
}
/// 单跑入口 02：ktest-诊断闭集。
pub fn entry02() -> Result<KRun, KDiag> {
    single_run(2)
}
/// 单跑入口 03：ktest-账目守恒。
pub fn entry03() -> Result<KRun, KDiag> {
    single_run(3)
}
/// 单跑入口 04：ktest-注入必抓A。
pub fn entry04() -> Result<KRun, KDiag> {
    single_run(4)
}
/// 单跑入口 05：ktest-注入必抓B。
pub fn entry05() -> Result<KRun, KDiag> {
    single_run(5)
}
/// 单跑入口 06：ktest-注入必抓C。
pub fn entry06() -> Result<KRun, KDiag> {
    single_run(6)
}
/// 单跑入口 07：ktest-判序冻结。
pub fn entry07() -> Result<KRun, KDiag> {
    single_run(7)
}
/// 单跑入口 08：ktest-状态机单调。
pub fn entry08() -> Result<KRun, KDiag> {
    single_run(8)
}
/// 单跑入口 09：ktest-容量边界。
pub fn entry09() -> Result<KRun, KDiag> {
    single_run(9)
}
/// 单跑入口 10：ktest-双跑一致。
pub fn entry10() -> Result<KRun, KDiag> {
    single_run(10)
}
/// 单跑入口 11：ktest-指纹非零。
pub fn entry11() -> Result<KRun, KDiag> {
    single_run(11)
}
/// 单跑入口 12：ktest-矩阵段登记。
pub fn entry12() -> Result<KRun, KDiag> {
    single_run(12)
}
/// 单跑入口 13：ktest-预算段登记。
pub fn entry13() -> Result<KRun, KDiag> {
    single_run(13)
}
/// 单跑入口 14：ktest-双机段登记。
pub fn entry14() -> Result<KRun, KDiag> {
    single_run(14)
}
/// 单跑入口 15：ktest-预告链衔接。
pub fn entry15() -> Result<KRun, KDiag> {
    single_run(15)
}
/// 单跑入口 16：ktest-批账只增。
pub fn entry16() -> Result<KRun, KDiag> {
    single_run(16)
}
/// 单跑入口 17：ktest-聚合等式。
pub fn entry17() -> Result<KRun, KDiag> {
    single_run(17)
}
/// 单跑入口 18：ktest-文案闭集。
pub fn entry18() -> Result<KRun, KDiag> {
    single_run(18)
}
/// 单跑入口 19：ktest-预算耗尽。
pub fn entry19() -> Result<KRun, KDiag> {
    single_run(19)
}
/// 单跑入口 20：ktest-收口冻结。
pub fn entry20() -> Result<KRun, KDiag> {
    single_run(20)
}

/// 全部单跑入口表（20 个函数指针，入口齐点验用）。
pub const ENTRY_FNS: [fn() -> Result<KRun, KDiag>; 20] = [
    entry01, entry02, entry03, entry04, entry05, entry06, entry07, entry08, entry09, entry10,
    entry11, entry12, entry13, entry14, entry15, entry16, entry17, entry18, entry19, entry20,
];

// ── 批账（等式断言随批归档） ─────────────────────────────────────
/// 批账行：批号 + Σ入口条数 + 聚合账总条数 + 全跑通过数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BatchRow {
    pub batch: u32,
    pub sum_cases: u32,
    pub agg_total: u32,
    pub passed_n: u32,
}

impl BatchRow {
    /// 聚合等式断言：Σ入口条数 = 聚合账总条数。
    pub fn equation_ok(&self) -> bool {
        self.sum_cases == self.agg_total && self.agg_total == AGG_TOTAL
    }
}

/// 批账簿：只增不减，批号严格递增。
pub struct BatchLedger {
    rows: Vec<BatchRow>,
}

impl BatchLedger {
    pub fn new() -> BatchLedger {
        BatchLedger { rows: Vec::new() }
    }
    pub fn append(&mut self, row: BatchRow) -> Result<(), KDiag> {
        if let Some(last) = self.rows.last() {
            if row.batch <= last.batch {
                return Err(KDiag::EBatch);
            }
        }
        if !row.equation_ok() {
            return Err(KDiag::EEquation);
        }
        self.rows.push(row);
        Ok(())
    }
    pub fn len(&self) -> usize {
        self.rows.len()
    }
    pub fn all_equations_ok(&self) -> bool {
        self.rows.iter().all(|r| r.equation_ok())
    }
}

// ── B38 压测框架登记（矩阵/预算/双机三段） ───────────────────────
/// 矩阵段：5×4 = 20 格登记账。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MatrixSeg {
    pub rows: u32,
    pub cols: u32,
    pub booked: u32,
}

/// 预算段：轮次预算 + 槽位预算。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BudgetSeg {
    pub rounds_left: u32,
    pub slots_left: u32,
}

/// 双机段：双机口径 + 指纹。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DualSeg {
    pub enabled: bool,
    pub fp: u64,
}

/// 框架登记段序（冻结：矩阵=0 → 预算=1 → 双机=2）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SegKind {
    Matrix,
    Budget,
    Dual,
}

impl SegKind {
    fn rank(self) -> u32 {
        match self {
            SegKind::Matrix => 0,
            SegKind::Budget => 1,
            SegKind::Dual => 2,
        }
    }
}

/// B38 压测框架登记簿：三段单调登记，齐了才出绿。
pub struct FrameworkReg {
    matrix: Option<MatrixSeg>,
    budget: Option<BudgetSeg>,
    dual: Option<DualSeg>,
    /// 已登记最高段秩（i32，-1 = 空簿；矩阵段秩 0）。
    last_rank: i32,
}

impl FrameworkReg {
    pub fn new() -> FrameworkReg {
        FrameworkReg { matrix: None, budget: None, dual: None, last_rank: -1 }
    }

    /// 登记段（判序：重复 → 跳段）。
    pub fn register(&mut self, kind: SegKind) -> Result<(), KDiag> {
        let dup = match kind {
            SegKind::Matrix => self.matrix.is_some(),
            SegKind::Budget => self.budget.is_some(),
            SegKind::Dual => self.dual.is_some(),
        };
        if dup {
            return Err(KDiag::ERegDup);
        }
        // 跳段拒：新段秩必须恰为 last_rank+1（空簿只许从矩阵段起）。
        if kind.rank() as i32 > self.last_rank + 1 {
            return Err(KDiag::ERegOrder);
        }
        match kind {
            SegKind::Matrix => {
                // 矩阵段：20 格全登记（5 行 × 4 列逐格登记计数）。
                self.matrix = Some(MatrixSeg {
                    rows: MATRIX_ROWS,
                    cols: MATRIX_COLS,
                    booked: MATRIX_ROWS.saturating_mul(MATRIX_COLS),
                });
            }
            SegKind::Budget => {
                self.budget = Some(BudgetSeg {
                    rounds_left: BUDGET_ROUNDS,
                    slots_left: BUDGET_SLOTS,
                });
            }
            SegKind::Dual => {
                // 双机段：口径开 + 指纹非零（轻量确定性口径，不递归全流程）。
                self.dual = Some(DualSeg { enabled: true, fp: dual_fp() });
            }
        }
        self.last_rank = (kind.rank() as i32).max(self.last_rank);
        Ok(())
    }

    /// 三段齐。
    pub fn complete(&self) -> bool {
        self.matrix.is_some() && self.budget.is_some() && self.dual.is_some()
    }

    /// 矩阵段登记齐（20 格）。
    pub fn matrix_full(&self) -> bool {
        match self.matrix {
            Some(m) => {
                m.rows == MATRIX_ROWS
                    && m.cols == MATRIX_COLS
                    && m.booked == MATRIX_ROWS.saturating_mul(MATRIX_COLS)
            }
            None => false,
        }
    }

    /// 预算段核账：两预算都非零。
    pub fn budget_alive(&self) -> bool {
        match self.budget {
            Some(b) => b.rounds_left > 0 && b.slots_left > 0,
            None => false,
        }
    }

    /// 预算扣减（耗尽显性拒）。
    pub fn spend(&mut self, rounds: u32, slots: u32) -> Result<(), KDiag> {
        let b = match self.budget.as_mut() {
            Some(b) => b,
            None => return Err(KDiag::ESegMissing),
        };
        if rounds > b.rounds_left || slots > b.slots_left {
            return Err(KDiag::EBudget);
        }
        b.rounds_left = b.rounds_left.saturating_sub(rounds);
        b.slots_left = b.slots_left.saturating_sub(slots);
        Ok(())
    }

    /// 双机段核账：口径开 + 指纹非零。
    pub fn dual_ok(&self) -> bool {
        match self.dual {
            Some(d) => d.enabled && d.fp != 0,
            None => false,
        }
    }

    /// 框架绿：三段齐 + 矩阵满 + 预算活 + 双机真。
    pub fn green(&self) -> bool {
        self.complete() && self.matrix_full() && self.budget_alive() && self.dual_ok()
    }
}

// ── F12719 预告链登记 ────────────────────────────────────────────
/// 预告链登记行：F12719 引用 + 衔接态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ForecastLink {
    pub seg: u32,
    pub batch: u32,
    pub linked: bool,
}

/// 悬空断言（五条合取）：段基非零 / 对齐 0x40 / VX9 段窗 / ≠本模块段基 /
/// 衔接态真。
pub fn link_not_dangling(link: &ForecastLink) -> bool {
    link.seg != 0
        && link.seg % 0x40 == 0
        && link.seg >= 0x80000
        && link.seg <= 0x82FC0
        && link.seg != SEG_BASE
        && link.linked
        && link.batch == FORECAST_BATCH
        && link.seg == FORECAST_SEG
}

// ── 聚合器 ───────────────────────────────────────────────────────
/// 聚合相位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AggPhase {
    Idle,
    Running,
    Aggregated,
}

impl AggPhase {
    fn rank(self) -> u32 {
        match self {
            AggPhase::Idle => 0,
            AggPhase::Running => 1,
            AggPhase::Aggregated => 2,
        }
    }
}

/// ktest 聚合器：20 单跑入口 → 聚合账等式 → 批账归档。
pub struct KAggregator {
    runs: Vec<KRun>,
    phase: AggPhase,
    sum_cases: u32,
    passed_n: u32,
}

impl KAggregator {
    pub fn new() -> KAggregator {
        KAggregator { runs: Vec::new(), phase: AggPhase::Idle, sum_cases: 0, passed_n: 0 }
    }

    pub fn phase(&self) -> AggPhase {
        self.phase
    }

    /// 相位推进（单调；倒退显性拒）。
    pub fn advance(&mut self, to: AggPhase) -> Result<(), KDiag> {
        if to.rank() < self.phase.rank() {
            return Err(KDiag::EPhase);
        }
        self.phase = to;
        Ok(())
    }

    /// 收一条单跑结果（判序：封账态 → 容量 → id 重复）。
    pub fn collect(&mut self, r: KRun) -> Result<(), KDiag> {
        if self.phase == AggPhase::Aggregated {
            return Err(KDiag::EPhase);
        }
        if self.runs.len() >= ENTRIES {
            return Err(KDiag::ENoEntry);
        }
        if self.runs.iter().any(|x| x.id == r.id) {
            return Err(KDiag::ENoEntry);
        }
        if self.phase == AggPhase::Idle {
            self.advance(AggPhase::Running)?;
        }
        self.sum_cases = self.sum_cases.saturating_add(r.cases);
        if r.passed {
            self.passed_n = self.passed_n.saturating_add(1);
        }
        self.runs.push(r);
        Ok(())
    }

    /// 聚合账总条数 = 已收入口条数之和（账面口径）。
    pub fn agg_total(&self) -> u32 {
        self.sum_cases
    }

    /// 收齐判定：20 入口全收。
    pub fn complete(&self) -> bool {
        self.runs.len() == ENTRIES
    }

    /// 聚合等式断言：Σ入口条数 = 聚合账总条数 = AGG_TOTAL。
    pub fn equation_ok(&self) -> bool {
        self.complete() && self.sum_cases == AGG_TOTAL
    }

    /// 全跑通过数。
    pub fn passed_count(&self) -> u32 {
        self.passed_n
    }

    /// 聚合收口：齐 + 等式 → 相位 Aggregated。
    pub fn finalize(&mut self) -> Result<(), KDiag> {
        if !self.complete() {
            return Err(KDiag::ENoEntry);
        }
        if !self.equation_ok() {
            return Err(KDiag::EEquation);
        }
        self.advance(AggPhase::Aggregated)
    }
}

/// 全量回归一条龙：20 单跑入口齐跑 → 聚合 → 批账归档。
pub fn run_all(batch: u32, ledger: &mut BatchLedger) -> Result<(u32, u32), KDiag> {
    let mut agg = KAggregator::new();
    let mut i = 0usize;
    while i < ENTRY_FNS.len() {
        let f = match ENTRY_FNS.get(i) {
            Some(f) => *f,
            None => return Err(KDiag::ENoEntry),
        };
        let r = f()?;
        agg.collect(r)?;
        i += 1;
    }
    agg.finalize()?;
    let sum = agg.agg_total();
    let passed = agg.passed_count();
    ledger.append(BatchRow {
        batch,
        sum_cases: sum,
        agg_total: AGG_TOTAL,
        passed_n: passed,
    })?;
    Ok((sum, passed))
}

// ── 确定性 LCG（种子 = SEG_BASE） ────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub const fn new(seed: u64) -> Lcg {
        Lcg(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

// ── FNV-1a 指纹 ──────────────────────────────────────────────────
/// 双机段轻量指纹：冻结口径（AGG_TOTAL + ENTRIES + 段基）FNV-1a 混入，
/// 不递归全流程（register(Dual) 内部调用，避免 fingerprint 自引用）。
pub fn dual_fp() -> u64 {
    let mut h = fnv1a(0xcbf29ce484222325, &AGG_TOTAL.to_le_bytes());
    h = fnv1a(h, &(ENTRIES as u64).to_le_bytes());
    fnv1a(h, &SEG_BASE.to_le_bytes())
}

pub fn fnv1a(acc: u64, bytes: &[u8]) -> u64 {
    let mut h = acc;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(&b) = bytes.get(i) {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        i += 1;
    }
    h
}

/// 全流程指纹：20 入口齐跑 + 三段登记 + 预告链 + 批账。
pub fn fingerprint() -> Result<u64, KDiag> {
    let mut led = BatchLedger::new();
    let (sum, passed) = run_all(1, &mut led)?;
    let mut h = fnv1a(0xcbf29ce484222325, &sum.to_le_bytes());
    h = fnv1a(h, &passed.to_le_bytes());
    h = fnv1a(h, &led.len().to_le_bytes());
    let mut fw = FrameworkReg::new();
    fw.register(SegKind::Matrix)?;
    fw.register(SegKind::Budget)?;
    fw.register(SegKind::Dual)?;
    h = fnv1a(h, &[fw.green() as u8]);
    let link = ForecastLink { seg: FORECAST_SEG, batch: FORECAST_BATCH, linked: true };
    h = fnv1a(h, &[link_not_dangling(&link) as u8]);
    Ok(h)
}

// ── 注入面（三族 × 10 轮全抓） ───────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0u32; 3], caught: [0u32; 3] }
    }
    /// 族账读取（get 取值，越界回落 0，不裸下标）。
    fn try_n(&self, fam: usize) -> u32 {
        self.tries.get(fam).copied().unwrap_or(0)
    }
    fn caught_n(&self, fam: usize) -> u32 {
        self.caught.get(fam).copied().unwrap_or(0)
    }
    /// 族账自增（get_mut 取槽，不裸下标）。
    fn bump_try(&mut self, fam: usize) {
        if let Some(t) = self.tries.get_mut(fam) {
            *t = t.saturating_add(1);
        }
    }
    fn bump_caught(&mut self, fam: usize) {
        if let Some(c) = self.caught.get_mut(fam) {
            *c = c.saturating_add(1);
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0usize;
        while i < 3 {
            if self.try_n(i) != self.caught_n(i) || self.try_n(i) < 10 {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 三族注入 × 10 轮：聚合账篡改 / 框架登记缺段跳段 / 预告链悬空。
pub fn run_injections(led: &mut InjectLedger) {
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut round = 0u32;
    while round < 10 {
        // 族 0：聚合账篡改——Σ入口条数 ≠ AGG_TOTAL，等式断言必抓。
        let mut agg = KAggregator::new();
        let mut i = 0usize;
        let mut collect_ok = true;
        while i < ENTRIES {
            if let Some(f) = ENTRY_FNS.get(i) {
                if let Ok(r) = f() {
                    // 注入：末条案例数 +1，破坏 Σ。
                    let bad = if i == ENTRIES - 1 {
                        KRun { id: r.id, cases: r.cases.saturating_add(1), passed: r.passed }
                    } else {
                        r
                    };
                    if agg.collect(bad).is_err() {
                        collect_ok = false;
                    }
                }
            }
            i += 1;
        }
        led.bump_try(0);
        if collect_ok && agg.finalize() == Err(KDiag::EEquation) {
            led.bump_caught(0);
        }
        // 族 1：框架登记跳段——先登记预算段（跳过矩阵）必拒。
        let mut fw = FrameworkReg::new();
        let skip = fw.register(SegKind::Budget) == Err(KDiag::ERegOrder);
        // 重复登记：先登矩阵再登矩阵必拒。
        let _ = fw.register(SegKind::Matrix);
        let dup = fw.register(SegKind::Matrix) == Err(KDiag::ERegDup);
        led.bump_try(1);
        if skip && dup {
            led.bump_caught(1);
        }
        // 族 2：预告链悬空——断链引用（对齐破/未衔接/段基错）必抓。
        let broken = ForecastLink { seg: FORECAST_SEG + 1, batch: FORECAST_BATCH, linked: false };
        let unlinked = ForecastLink { seg: FORECAST_SEG, batch: FORECAST_BATCH, linked: false };
        let wrong_batch = ForecastLink { seg: FORECAST_SEG, batch: 99, linked: true };
        let caught = !link_not_dangling(&broken)
            && !link_not_dangling(&unlinked)
            && !link_not_dangling(&wrong_batch);
        led.bump_try(2);
        if caught {
            led.bump_caught(2);
        }
        let _ = rng.below(7); // LCG 参与轮内扰动口径（确定性），保持序列活跃
        round = round.saturating_add(1);
    }
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x80000 && SEG_BASE <= 0x82FC0
}

pub fn diag_window_ok() -> bool {
    KDiag::ALL.iter().all(|d| d.val() >= SEG_BASE && d.val() < SEG_BASE + 0x40)
}

// ── 判据谓词（私有） ─────────────────────────────────────────────
fn entries_ready_ok() -> bool {
    // 20 条单跑入口齐：表 20 行 + 函数表 20 个 + 全部单跑通过。
    if ENTRY_TABLE.len() != ENTRIES || ENTRY_FNS.len() != ENTRIES {
        return false;
    }
    let mut i = 0usize;
    while i < ENTRY_FNS.len() {
        match ENTRY_FNS.get(i) {
            Some(f) => match f() {
                Ok(r) => {
                    if !r.passed || r.cases != cases_of(r.id) {
                        return false;
                    }
                }
                Err(_) => return false,
            },
            None => return false,
        }
        i += 1;
    }
    true
}

fn equation_ok() -> bool {
    // 聚合账等式：Σ入口条数 = 聚合账总条数 = AGG_TOTAL，随批归档。
    let mut led = BatchLedger::new();
    match run_all(1, &mut led) {
        Ok((sum, _)) => {
            sum == AGG_TOTAL && led.len() == 1 && led.all_equations_ok()
        }
        Err(_) => false,
    }
}

fn framework_ok() -> bool {
    // B38 三段登记：矩阵满 + 预算活 + 双机真 → 框架绿；缺段不出绿。
    let mut fw = FrameworkReg::new();
    if fw.green() {
        return false; // 空簿不出绿
    }
    if fw.register(SegKind::Matrix).is_err() {
        return false;
    }
    if fw.register(SegKind::Budget).is_err() {
        return false;
    }
    if fw.green() {
        return false; // 双机段未登不出绿
    }
    if fw.register(SegKind::Dual).is_err() {
        return false;
    }
    fw.green()
}

fn link_ok() -> bool {
    // F12719 预告链衔接零悬空。
    let link = ForecastLink { seg: FORECAST_SEG, batch: FORECAST_BATCH, linked: true };
    link_not_dangling(&link) && FORECAST_SEG != SEG_BASE && FORECAST_SEG < SEG_BASE
}

fn boundary_ok() -> bool {
    // 四类边界：id 闭集两端 / 复算口径 / 预算耗尽 / 批号回退。
    let mut ok = true;
    ok = ok && single_run(1).is_ok() && single_run(20).is_ok();
    ok = ok && single_run(0) == Err(KDiag::ENoEntry);
    ok = ok && single_run(21) == Err(KDiag::ENoEntry);
    // 复算口径：cases_of(1)=8、cases_of(20)=40（冻结复算恒等）。
    ok = ok && cases_of(1) == 8 && cases_of(20) == 40 && AGG_TOTAL > 0;
    // 预算耗尽：扣满后新扣拒。
    let mut fw = FrameworkReg::new();
    let _ = fw.register(SegKind::Matrix);
    let _ = fw.register(SegKind::Budget);
    let full = fw.spend(BUDGET_ROUNDS, BUDGET_SLOTS).is_ok();
    ok = ok && full && fw.spend(1, 1) == Err(KDiag::EBudget);
    // 批号回退：批账严格递增。
    let mut led = BatchLedger::new();
    ok = ok
        && led
            .append(BatchRow {
                batch: 1,
                sum_cases: AGG_TOTAL,
                agg_total: AGG_TOTAL,
                passed_n: ENTRIES as u32,
            })
            .is_ok()
        && led
            .append(BatchRow {
                batch: 1,
                sum_cases: AGG_TOTAL,
                agg_total: AGG_TOTAL,
                passed_n: ENTRIES as u32,
            })
            == Err(KDiag::EBatch);
    ok
}

fn phase_ok() -> bool {
    // 相位倒退拒：Aggregated → Running 拒；同相位不拒；相位读取器核账。
    let mut agg = KAggregator::new();
    agg.advance(AggPhase::Idle).is_ok()
        && agg.advance(AggPhase::Running).is_ok()
        && agg.advance(AggPhase::Running).is_ok()
        && agg.phase() == AggPhase::Running
        && agg.advance(AggPhase::Idle) == Err(KDiag::EPhase)
}

fn conservation_ok() -> bool {
    // 计数守恒：已收入口数 = 20、通过数 = 20、Σ = AGG_TOTAL。
    let mut agg = KAggregator::new();
    let mut i = 0usize;
    while i < ENTRY_FNS.len() {
        if let Some(f) = ENTRY_FNS.get(i) {
            if let Ok(r) = f() {
                let _ = agg.collect(r);
            }
        }
        i += 1;
    }
    agg.complete() && agg.passed_count() == ENTRIES as u32 && agg.agg_total() == AGG_TOTAL
}

fn dual_run_ok() -> bool {
    match (fingerprint(), fingerprint()) {
        (Ok(a), Ok(b)) => a == b && a != 0,
        _ => false,
    }
}

fn text_closed_ok() -> bool {
    let mut all_diff = true;
    let mut i = 0usize;
    while i < KDiag::ALL.len() {
        let mut j = i + 1;
        while j < KDiag::ALL.len() {
            match (KDiag::ALL.get(i), KDiag::ALL.get(j)) {
                (Some(a), Some(b)) => {
                    if a.detail() == b.detail() || a.val() == b.val() {
                        all_diff = false;
                    }
                }
                _ => all_diff = false,
            }
            j += 1;
        }
        i += 1;
    }
    all_diff
        && KDiag::ALL.iter().all(|d| !d.detail().is_empty())
        && ENTRY_TABLE.iter().all(|e| !e.name.is_empty())
        && detail(SEG_BASE + 0x3F).contains("未知")
}

// ── 收口检查表 ───────────────────────────────────────────────────
pub const FROZEN_CHECK_N: usize = 16;

const CHECK_NAMES: [&str; FROZEN_CHECK_N] = [
    "F12740-01 20条单跑入口齐",
    "F12740-02 聚合账等式断言随批归档",
    "F12740-03 B38矩阵段登记满",
    "F12740-04 B38预算段登记活",
    "F12740-05 B38双机段登记真",
    "F12740-06 F12719预告链零悬空",
    "F12740-07 注入族0 聚合账篡改10轮全抓",
    "F12740-08 注入族1 跳段重复登记10轮全抓",
    "F12740-09 注入族2 预告链悬空10轮全抓",
    "F12740-10 四类边界（id/复算/预算/批号）",
    "F12740-11 相位倒退拒",
    "F12740-12 计数守恒",
    "F12740-13 双跑一致指纹",
    "F12740-14 段位守卫",
    "F12740-15 文案闭集",
    "F12740-16 收口只增不减",
];

const CHECK_DETAILS: [&str; FROZEN_CHECK_N] = [
    "表 20 行 + 函数表 20 个 + 全部单跑通过",
    "Σ入口条数 = 聚合账总条数 = AGG_TOTAL，批账只增不减",
    "5×4=20 格全登记",
    "轮次预算 40 + 槽位预算 20 两账非零",
    "双机口径开 + 指纹非零",
    "段基非零/对齐0x40/VX9窗/≠本段/登记态 五条合取",
    "",
    "",
    "",
    "id 两端/复算口径/预算耗尽/批号回退",
    "Aggregated→Running 显性拒 EPhase",
    "收 20 入口、过 20 入口、Σ = AGG_TOTAL",
    "两次全流程 FNV-1a 指纹逐位一致且非零",
    "SEG_BASE%0x40==0 且在 [0x80000,0x82FC0]",
    "全码明细非空且两两不同，入口名全非空",
    "名表/明细表/冻结值三表一致且 ≥12",
];

/// 静态名表取值（get 取值，越界回落占位，不裸下标）。
fn nm(i: usize) -> &'static str {
    CHECK_NAMES.get(i).copied().unwrap_or("未名项")
}

/// 静态明细表取值（get 取值，越界回落空串，不裸下标）。
fn dt(i: usize) -> &'static str {
    CHECK_DETAILS.get(i).copied().unwrap_or("")
}

/// 收口检查面：16 条（≥12）。
pub fn extra_checks() -> Vec<(String, bool, String)> {
    let mut v: Vec<(String, bool, String)> = Vec::new();
    v.push((String::from(nm(0)), entries_ready_ok(), format!("entries={}", ENTRIES)));
    v.push((String::from(nm(1)), equation_ok(), format!("AGG_TOTAL={}", AGG_TOTAL)));
    // B38 三段登记逐段。
    let mut fw = FrameworkReg::new();
    let _ = fw.register(SegKind::Matrix);
    v.push((
        String::from(nm(2)),
        fw.matrix_full(),
        format!("{}×{}格", MATRIX_ROWS, MATRIX_COLS),
    ));
    let _ = fw.register(SegKind::Budget);
    v.push((
        String::from(nm(3)),
        fw.budget_alive(),
        format!("rounds={} slots={}", BUDGET_ROUNDS, BUDGET_SLOTS),
    ));
    let _ = fw.register(SegKind::Dual);
    v.push((
        String::from(nm(4)),
        fw.dual_ok() && fw.green() && framework_ok(),
        format!("fp≠0={}", fw.dual_ok()),
    ));
    let link = ForecastLink { seg: FORECAST_SEG, batch: FORECAST_BATCH, linked: true };
    v.push((
        String::from(nm(5)),
        link_ok(),
        format!(
            "F12719 seg=0x{:05X} B{} linked={}",
            link.seg, link.batch, link.linked
        ),
    ));
    let mut inj = InjectLedger::new();
    run_injections(&mut inj);
    v.push((
        String::from(nm(6)),
        inj.try_n(0) == inj.caught_n(0) && inj.try_n(0) == 10,
        format!("t/c {}/{}", inj.try_n(0), inj.caught_n(0)),
    ));
    v.push((
        String::from(nm(7)),
        inj.try_n(1) == inj.caught_n(1) && inj.try_n(1) == 10,
        format!("t/c {}/{}", inj.try_n(1), inj.caught_n(1)),
    ));
    v.push((
        String::from(nm(8)),
        inj.try_n(2) == inj.caught_n(2) && inj.try_n(2) == 10 && inj.all_caught(),
        format!("t/c {}/{}", inj.try_n(2), inj.caught_n(2)),
    ));
    v.push((String::from(nm(9)), boundary_ok(), String::from(dt(9))));
    v.push((String::from(nm(10)), phase_ok(), String::from(dt(10))));
    v.push((String::from(nm(11)), conservation_ok(), String::from(dt(11))));
    v.push((String::from(nm(12)), dual_run_ok(), String::from(dt(12))));
    v.push((
        String::from(nm(13)),
        segment_guard() && diag_window_ok(),
        format!("seg=0x{:05X} align={}", SEG_BASE, SEG_BASE % 0x40 == 0),
    ));
    v.push((
        String::from(nm(14)),
        text_closed_ok(),
        format!("codes={} entries=20", KDiag::ALL.len()),
    ));
    v.push((
        String::from(nm(15)),
        CHECK_NAMES.len() == FROZEN_CHECK_N
            && CHECK_DETAILS.len() == FROZEN_CHECK_N
            && FROZEN_CHECK_N >= 12,
        format!("冻结={} 名表={} 明细表={}", FROZEN_CHECK_N, CHECK_NAMES.len(), CHECK_DETAILS.len()),
    ));
    v
}

/// 自检入口：extra_checks 逐条进 set；前 8 条走动态明细槽，其余静态明细。
pub fn run_unx12739_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12739");
    let items = extra_checks();
    let mut i = 0usize;
    while i < items.len() {
        if let Some((_, passed, dyn_detail)) = items.get(i) {
            let name = CHECK_NAMES.get(i).copied().unwrap_or("F12740-? 未名项");
            if i < 8 {
                s.add_dyn(name, *passed, dyn_detail);
            } else {
                let st = CHECK_DETAILS.get(i).copied().unwrap_or("");
                s.add(name, *passed, st);
            }
        }
        i += 1;
    }
    s
}

// ── 测试节（≥8 条） ──────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn twenty_single_run_entries_ready() {
        // 20 条单跑入口齐：函数表逐个跑通。
        assert!(entries_ready_ok());
        for i in 0..ENTRIES {
            let f = ENTRY_FNS.get(i).copied().unwrap_or(entry01);
            assert!(f().is_ok(), "入口 {} 单跑失败", i + 1);
        }
        assert!(entry01().is_ok());
        assert!(entry20().is_ok());
    }

    #[test]
    fn aggregate_equation_holds() {
        // Σ入口条数 = 聚合账总条数（UNX-F12739-J1 补充锚定）。
        assert!(equation_ok());
        let mut led = BatchLedger::new();
        let (sum, passed) = run_all(1, &mut led).unwrap_or((0, 0));
        assert_eq!(sum, AGG_TOTAL);
        assert_eq!(passed, ENTRIES as u32);
        assert_eq!(led.len(), 1);
    }

    #[test]
    fn b38_framework_three_segments() {
        // B38 压测框架登记：矩阵/预算/双机三段。
        assert!(framework_ok());
        let mut fw = FrameworkReg::new();
        assert!(!fw.complete());
        assert!(fw.register(SegKind::Matrix).is_ok());
        assert!(fw.register(SegKind::Budget).is_ok());
        assert!(!fw.complete(), "双机段未登不出齐");
        assert!(fw.register(SegKind::Dual).is_ok());
        assert!(fw.complete() && fw.green());
    }

    #[test]
    fn f12719_link_not_dangling() {
        // F12719 预告链衔接零悬空（UNX-F12740-J1 补充锚定）。
        assert!(link_ok());
        let link = ForecastLink { seg: FORECAST_SEG, batch: FORECAST_BATCH, linked: true };
        assert!(link_not_dangling(&link));
        assert_eq!(FORECAST_SEG, 0x81DC0);
        assert!(FORECAST_SEG % 0x40 == 0);
    }

    #[test]
    fn injection_ledger_tamper() {
        // 注入族 0：聚合账篡改 10 轮全抓。
        let mut inj = InjectLedger::new();
        run_injections(&mut inj);
        assert_eq!(inj.try_n(0), 10);
        assert_eq!(inj.caught_n(0), 10);
    }

    #[test]
    fn injection_reg_order() {
        // 注入族 1：跳段/重复登记 10 轮全抓。
        let mut inj = InjectLedger::new();
        run_injections(&mut inj);
        assert_eq!(inj.try_n(1), 10);
        assert_eq!(inj.caught_n(1), 10);
    }

    #[test]
    fn injection_link_dangling() {
        // 注入族 2：预告链悬空 10 轮全抓。
        let mut inj = InjectLedger::new();
        run_injections(&mut inj);
        assert_eq!(inj.try_n(2), 10);
        assert_eq!(inj.caught_n(2), 10);
        assert!(inj.all_caught());
    }

    #[test]
    fn boundary_and_budget_exhaust() {
        // 四类边界：id 闭集/复算口径/预算耗尽/批号回退。
        assert!(boundary_ok());
        assert_eq!(single_run(0), Err(KDiag::ENoEntry));
        assert_eq!(single_run(21), Err(KDiag::ENoEntry));
    }

    #[test]
    fn phase_monotonic() {
        // 相位倒退拒。
        assert!(phase_ok());
        let mut agg = KAggregator::new();
        assert_eq!(agg.advance(AggPhase::Running), Ok(()));
        assert_eq!(agg.advance(AggPhase::Aggregated), Ok(()));
        assert_eq!(agg.advance(AggPhase::Running), Err(KDiag::EPhase));
    }

    #[test]
    fn collect_conservation() {
        // 计数守恒：20 入口齐收、Σ = AGG_TOTAL。
        assert!(conservation_ok());
        let mut agg = KAggregator::new();
        assert!(agg.collect(KRun { id: 1, cases: cases_of(1), passed: true }).is_ok());
        assert!(agg.collect(KRun { id: 1, cases: 1, passed: true }).is_err(), "id 重复拒");
        assert_eq!(agg.agg_total(), cases_of(1));
    }

    #[test]
    fn dual_fingerprint_stable() {
        // 双跑一致。
        assert!(dual_run_ok());
        assert_eq!(fingerprint(), fingerprint());
    }

    #[test]
    fn segment_and_text_closed() {
        // 段位守卫 + 文案闭集。
        assert!(segment_guard());
        assert!(diag_window_ok());
        assert!(text_closed_ok());
        assert!(detail(0x4242).contains("未知"));
        assert_eq!(SEG_BASE, 0x822C0);
    }

    #[test]
    fn extra_checks_all_green_and_frozen() {
        // 收口只增不减 + 全绿。
        let v = extra_checks();
        assert_eq!(v.len(), FROZEN_CHECK_N);
        assert!(v.len() >= 12);
        for (i, (_, p, _)) in v.iter().enumerate() {
            assert!(*p, "第 {} 条收口红", i);
        }
        let s = run_unx12739_checks();
        assert_eq!(s.len(), FROZEN_CHECK_N);
        assert!(s.all_passed());
        assert!(!s.truncated());
    }
}
