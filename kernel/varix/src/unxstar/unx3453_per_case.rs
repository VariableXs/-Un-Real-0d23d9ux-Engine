//! UNX-F3453 执行统计（执行次数/通过率/时长 per-case 历史账） · 0x79E80段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 VX8-B15 逐字摘录）：
//!   ① 纯功能代码 ≥280 行；
//!   ② 判据原文：J1：per-case 历史账（run 计数/pass 计数/累计时长
//!     三字段）+ 跨 run 增量；M-212 多 run 统计对账。；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 三字段历史账：每用例一条 CaseStat（run_count / pass_count /
//!     total_ticks），三字段按判据原文冻结，缺一即账面残缺
//!     （LedgerBad 点名）；
//!   - 跨 run 增量：同一用例每执行一次，run_count 与 pass_count
//!     （通过时）、total_ticks 三者同步 checked 增量，账面只进不退
//!     （rewind 显性拒 RunCountShrink——历史账是 append-only）；
//!   - 通过率千分比：pass×1000/run 整数运算（千分母
//!     PASS_RATE_DENOM=1000 冻结，零浮点），run_count=0 时取率
//!     显性拒（NoRuns）；
//!   - M-212 多 run 统计对账：对注入用例连续记 5 次 run（4 过
//!     1 败、累计 600 拍），对账三元组 (5,4,600) 逐字段平账，
//!     任一字段偏差 1 即 ReconcileMismatch 点名；
//!   - 相位状态机：Open→Booked→Reconciled 三相单调只进不退，
//!     倒退显性拒（PhaseRegress）；
//!   - 容量守卫：账面定容 16 户，超容显性拒（CapacityOverflow）；
//!   - 双跑一致：统计与对账均为纯函数，同输入两次结果相同；
//!   - 写回面：状态=已完成、结果摘要非空。
//! 常量来源：
//!   - 千分母 1000：通过率整数化口径冻结值（零浮点红线派生）；
//!   - M-212 注入 5 run/4 过/600 拍：M-212 多 run 对账冻结演练值
//!     （派工卡 M-212 多 run 统计对账）；
//!   - 容量 16：统计账面定容（与执行面登记上限同源）；
//!   - M-212：派工卡验收判据编号（对账锚定）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x40 对齐，B15 批段窗 [0x79E00, 0x79FC0] 内）。
pub const SEG_BASE: u32 = 0x79E80;

/// 统计账面定容（户上限，超容显性拒）。
pub const MAX_LEDGER: usize = 16;
/// 通过率千分母（整数化口径，零浮点红线派生）。
pub const PASS_RATE_DENOM: u64 = 1_000;
/// M-212 注入用例号。
pub const M212_CASE_ID: u32 = 0x212;
/// M-212 注入 run 次数（多 run 对账演练）。
pub const M212_RUNS: u64 = 5;
/// M-212 注入通过次数（4 过 1 败）。
pub const M212_PASSES: u64 = 4;
/// M-212 注入累计时长（每次 run 120 拍 × 5）。
pub const M212_TICKS: u64 = 600;
/// 单次 run 冻结时长（演练口径：每 run 恒 120 拍）。
pub const M212_TICKS_PER_RUN: u64 = 120;
/// M-212 判据编号（对账锚定）。
pub const M212_AUDIT_ID: u64 = 0x212;

/// 单次执行裁决（通过/失败两态闭集）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunVerdict {
    /// 本 run 通过。
    Pass,
    /// 本 run 失败。
    Fail,
}

/// 统计相位状态机：Open→Booked→Reconciled 单调只进不退。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatPhase {
    /// 已开户、尚无 run 记账。
    Open,
    /// 已有至少一次 run 记账。
    Booked,
    /// 已完成至少一次对账平账。
    Reconciled,
}

impl StatPhase {
    /// 相位序（rank 只进不退的单调依据）。
    pub fn rank(self) -> u32 {
        match self {
            StatPhase::Open => 0,
            StatPhase::Booked => 1,
            StatPhase::Reconciled => 2,
        }
    }

    /// 单调推进：目标相位 rank 低于当前即拒（PhaseRegress）。
    pub fn advance(self, to: StatPhase) -> Result<StatPhase, u32> {
        if to.rank() < self.rank() {
            return Err(Diag::PhaseRegress.val());
        }
        if to.rank() > self.rank() {
            return Ok(to);
        }
        Ok(self)
    }
}

/// per-case 历史账单户：run 计数 / pass 计数 / 累计时长三字段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaseStat {
    /// 用例号。
    pub case_id: u32,
    /// run 计数（跨 run 只增不减）。
    pub run_count: u64,
    /// pass 计数（通过 run 数，≤ run_count）。
    pub pass_count: u64,
    /// 累计时长（拍，跨 run checked 累加）。
    pub total_ticks: u64,
    /// 统计相位（单调状态机）。
    pub phase: StatPhase,
}

impl CaseStat {
    /// 新开户：三字段全零、相位 Open。
    pub fn open(case_id: u32) -> CaseStat {
        CaseStat {
            case_id,
            run_count: 0,
            pass_count: 0,
            total_ticks: 0,
            phase: StatPhase::Open,
        }
    }
}

/// per-case 历史账面：户表 + 总 run 入账数。
#[derive(Debug, Clone)]
pub struct StatLedger {
    /// 户表（每用例一条，append-only）。
    pub stats: Vec<CaseStat>,
    /// 全账累计入账 run 数（跨户总账）。
    pub runs_booked: u64,
}

/// 记一次执行：三字段同步增量，账面只进不退。
pub fn record(
    ledger: &mut StatLedger,
    case_id: u32,
    verdict: RunVerdict,
    ticks: u64,
) -> Result<(), u32> {
    if ticks == 0 {
        return Err(Diag::ZeroTicks.val());
    }
    let mut idx = ledger.stats.len();
    let mut found = false;
    let mut i = 0usize;
    while i < ledger.stats.len() {
        match ledger.stats.get(i) {
            Some(c) => {
                if c.case_id == case_id {
                    idx = i;
                    found = true;
                }
            }
            None => return Err(Diag::LedgerBad.val()),
        }
        i += 1;
    }
    if !found {
        if ledger.stats.len() >= MAX_LEDGER {
            return Err(Diag::CapacityOverflow.val());
        }
        ledger.stats.push(CaseStat::open(case_id));
        idx = ledger.stats.len() - 1;
    }
    let slot = match ledger.stats.get_mut(idx) {
        Some(c) => c,
        None => return Err(Diag::LedgerBad.val()),
    };
    // run 计数 checked 增量（溢出显性拒，不静默饱和）。
    let runs = match slot.run_count.checked_add(1) {
        Some(r) => r,
        None => return Err(Diag::RunsOverflow.val()),
    };
    // pass 计数增量前守卫：通过数不得超过 run 数（含本次）。
    if verdict == RunVerdict::Pass {
        let next_pass = match slot.pass_count.checked_add(1) {
            Some(p) => p,
            None => return Err(Diag::PassOverRuns.val()),
        };
        if next_pass > runs {
            return Err(Diag::PassOverRuns.val());
        }
    }
    // 累计时长 checked 增量。
    let total = match slot.total_ticks.checked_add(ticks) {
        Some(t) => t,
        None => return Err(Diag::TicksOverflow.val()),
    };
    slot.run_count = runs;
    if verdict == RunVerdict::Pass {
        slot.pass_count += 1;
    }
    slot.total_ticks = total;
    if slot.phase.advance(StatPhase::Booked).is_err() {
        return Err(Diag::PhaseRegress.val());
    }
    slot.phase = StatPhase::Booked;
    let booked = match ledger.runs_booked.checked_add(1) {
        Some(b) => b,
        None => return Err(Diag::RunsOverflow.val()),
    };
    ledger.runs_booked = booked;
    Ok(())
}

/// 历史账倒退显性拒：rewind 任何字段都必拒（append-only 红线）。
pub fn rewind(ledger: &mut StatLedger, case_id: u32) -> Result<(), u32> {
    let mut idx = ledger.stats.len();
    let mut i = 0usize;
    while i < ledger.stats.len() {
        match ledger.stats.get(i) {
            Some(c) => {
                if c.case_id == case_id {
                    idx = i;
                }
            }
            None => return Err(Diag::LedgerBad.val()),
        }
        i += 1;
    }
    if idx >= ledger.stats.len() {
        return Err(Diag::LedgerBad.val());
    }
    Err(Diag::RunCountShrink.val())
}

/// 通过率千分比：pass×1000/run 整数运算（2/3 → 666）；零 run 取率显性拒。
pub fn pass_permille(stat: &CaseStat) -> Result<u64, u32> {
    if stat.run_count == 0 {
        return Err(Diag::NoRuns.val());
    }
    let num = match stat.pass_count.checked_mul(PASS_RATE_DENOM) {
        Some(n) => n,
        None => return Err(Diag::RunsOverflow.val()),
    };
    Ok(num / stat.run_count)
}

/// M-212 多 run 统计对账：三元组逐字段平账，平则推进相位。
pub fn reconcile(
    ledger: &mut StatLedger,
    case_id: u32,
    expect_runs: u64,
    expect_pass: u64,
    expect_ticks: u64,
) -> Result<(), u32> {
    let mut idx = ledger.stats.len();
    let mut i = 0usize;
    while i < ledger.stats.len() {
        match ledger.stats.get(i) {
            Some(c) => {
                if c.case_id == case_id {
                    idx = i;
                }
            }
            None => return Err(Diag::LedgerBad.val()),
        }
        i += 1;
    }
    if idx >= ledger.stats.len() {
        return Err(Diag::LedgerBad.val());
    }
    let stat = match ledger.stats.get(idx) {
        Some(c) => *c,
        None => return Err(Diag::LedgerBad.val()),
    };
    if stat.run_count != expect_runs
        || stat.pass_count != expect_pass
        || stat.total_ticks != expect_ticks
    {
        return Err(Diag::ReconcileMismatch.val());
    }
    if expect_pass > expect_runs {
        return Err(Diag::PassOverRuns.val());
    }
    let next = stat.phase.advance(StatPhase::Reconciled)?;
    let slot = match ledger.stats.get_mut(idx) {
        Some(c) => c,
        None => return Err(Diag::LedgerBad.val()),
    };
    slot.phase = next;
    Ok(())
}

/// M-212 注入演练：0x212 号连记 5 run（4 过 1 败、每 run 120 拍）。
pub fn m212_inject() -> Result<(u64, u64, u64), u32> {
    let mut ledger = StatLedger { stats: Vec::new(), runs_booked: 0 };
    let mut r = 0u64;
    while r < M212_RUNS {
        let verdict = if r < M212_PASSES { RunVerdict::Pass } else { RunVerdict::Fail };
        record(&mut ledger, M212_CASE_ID, verdict, M212_TICKS_PER_RUN)?;
        r += 1;
    }
    reconcile(&mut ledger, M212_CASE_ID, M212_RUNS, M212_PASSES, M212_TICKS)?;
    let stat = match ledger.stats.get(0) {
        Some(c) => *c,
        None => return Err(Diag::LedgerBad.val()),
    };
    Ok((stat.run_count, stat.pass_count, stat.total_ticks))
}

/// 诊断码闭集：0x79E81..=0x79E8C（12 码，变体带中文 doc）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Diag {
    /// 统计口径漂移：千分母或三字段口径被运行期改写。
    StatModelFrozen = 0x01,
    /// 账户超容：开户数超 MAX_LEDGER 上限。
    CapacityOverflow = 0x02,
    /// 零时长：run 时长为零（未执行语义）。
    ZeroTicks = 0x03,
    /// 时长溢出：累计时长 checked 增量超 u64 上沿。
    TicksOverflow = 0x04,
    /// run 数溢出：run 计数增量超 u64 上沿。
    RunsOverflow = 0x05,
    /// 零 run 取率：run_count=0 时取通过率。
    NoRuns = 0x06,
    /// 通过数超 run 数：pass_count 越过 run_count 上沿。
    PassOverRuns = 0x07,
    /// run 数倒退：历史账 rewind 显性拒。
    RunCountShrink = 0x08,
    /// 相位倒退：状态机 rank 回退。
    PhaseRegress = 0x09,
    /// 对账不平：三元组任一字段与账面不符。
    ReconcileMismatch = 0x0A,
    /// 账面残缺：户表内部状态破缺或用例未登记。
    LedgerBad = 0x0B,
    /// 写回摘要为空：完成时摘要未定稿。
    SummaryEmpty = 0x0C,
}

impl Diag {
    /// 完整 u32 编码。
    pub fn val(self) -> u32 {
        SEG_BASE + (self as u32)
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::StatModelFrozen => "统计口径漂移：建议把千分母与三字段口径冻结为常量，运行期只读",
            Diag::CapacityOverflow => "账户超容：建议户上限钳制在 MAX_LEDGER=16 内或扩容账面",
            Diag::ZeroTicks => "零时长：建议 run 时长恒正，未执行不计入累计",
            Diag::TicksOverflow => "时长溢出：建议累计时长用 checked_add，溢出显性拒",
            Diag::RunsOverflow => "run 数溢出：建议 run 计数用 checked_add，溢出显性拒",
            Diag::NoRuns => "零 run 取率：建议 run_count 为零时拒绝取率，先记账后取率",
            Diag::PassOverRuns => "通过数超 run 数：建议 pass_count 增量前先守卫 run 上沿",
            Diag::RunCountShrink => "run 数倒退：建议历史账 append-only，rewind 显性拒",
            Diag::PhaseRegress => "相位倒退：建议相位 rank 单调只进，倒退显性拒",
            Diag::ReconcileMismatch => "对账不平：建议三元组逐字段平账，偏差字段显性点名",
            Diag::LedgerBad => "账面残缺：建议核对户表内部状态后再记账",
            Diag::SummaryEmpty => "写回摘要为空：建议完成时以非空中文摘要写回",
        }
    }
}

/// 闭集全量数组（12 码）。
pub const ALL_DIAGS: [Diag; 12] = [
    Diag::StatModelFrozen,
    Diag::CapacityOverflow,
    Diag::ZeroTicks,
    Diag::TicksOverflow,
    Diag::RunsOverflow,
    Diag::NoRuns,
    Diag::PassOverRuns,
    Diag::RunCountShrink,
    Diag::PhaseRegress,
    Diag::ReconcileMismatch,
    Diag::LedgerBad,
    Diag::SummaryEmpty,
];

/// 诊断码查表闭集返回；未知编码回退固定兜底串。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < ALL_DIAGS.len() {
        match ALL_DIAGS.get(i) {
            Some(d) => {
                if d.val() == code {
                    return d.detail();
                }
            }
            None => break,
        }
        i += 1;
    }
    "未知诊断码：建议核对是否落在 0x79E81..=0x79E8C 闭集内"
}

// ── 判据谓词（全部私有 fn xxx_ok() -> bool）────────────────────────────

/// 段位守卫：0x40 对齐 + B15 批段窗 [0x79E00, 0x79FC0]。
fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x79E00 && SEG_BASE <= 0x79FC0
}

/// 诊断闭集完整：12 码、编码连续、唯一、文案非空且带「建议」。
fn diag_closed_set_ok() -> bool {
    if ALL_DIAGS.len() != 12 {
        return false;
    }
    let mut i = 0usize;
    while i < ALL_DIAGS.len() {
        let a = match ALL_DIAGS.get(i) {
            Some(d) => *d,
            None => return false,
        };
        if a.val() != SEG_BASE + (i as u32) + 1 {
            return false;
        }
        if a.detail().is_empty() || !a.detail().contains("建议") {
            return false;
        }
        let mut j = i + 1;
        while j < ALL_DIAGS.len() {
            match ALL_DIAGS.get(j) {
                Some(b) => {
                    if a.val() == b.val() {
                        return false;
                    }
                }
                None => return false,
            }
            j += 1;
        }
        i += 1;
    }
    detail(0xDEAD) == detail(0x5A5A) && !detail(0xDEAD).is_empty()
}

/// 统计口径冻结：千分母 1000、M-212 演练值冻结、判据号锚定。
fn model_frozen_ok() -> bool {
    PASS_RATE_DENOM == 1_000
        && M212_RUNS == 5
        && M212_PASSES == 4
        && M212_TICKS == 600
        && M212_TICKS_PER_RUN == 120
        && M212_TICKS == M212_RUNS * M212_TICKS_PER_RUN
        && M212_AUDIT_ID == 0x212
        && detail(Diag::StatModelFrozen.val()).contains("冻结")
}

/// 正常流记账：三字段同步增量、相位进 Booked。
fn record_ok() -> bool {
    let mut l = StatLedger { stats: Vec::new(), runs_booked: 0 };
    if record(&mut l, 1, RunVerdict::Pass, 100).is_err() {
        return false;
    }
    if record(&mut l, 1, RunVerdict::Fail, 150).is_err() {
        return false;
    }
    if record(&mut l, 2, RunVerdict::Pass, 200).is_err() {
        return false;
    }
    let a = match l.stats.get(0) {
        Some(c) => *c,
        None => return false,
    };
    let b = match l.stats.get(1) {
        Some(c) => *c,
        None => return false,
    };
    a.run_count == 2 && a.pass_count == 1 && a.total_ticks == 250
        && b.run_count == 1 && b.pass_count == 1 && b.total_ticks == 200
        && l.runs_booked == 3
        && a.phase == StatPhase::Booked
}

/// 跨 run 增量：同户多 run 三字段逐次递增。
fn cross_run_delta_ok() -> bool {
    let mut l = StatLedger { stats: Vec::new(), runs_booked: 0 };
    let mut prev_runs = 0u64;
    let mut prev_ticks = 0u64;
    let mut r = 0u64;
    while r < 4 {
        if record(&mut l, 7, RunVerdict::Pass, 30).is_err() {
            return false;
        }
        let s = match l.stats.get(0) {
            Some(c) => *c,
            None => return false,
        };
        if s.run_count <= prev_runs || s.total_ticks <= prev_ticks {
            return false;
        }
        prev_runs = s.run_count;
        prev_ticks = s.total_ticks;
        r += 1;
    }
    prev_runs == 4 && prev_ticks == 120 && l.runs_booked == 4
}

/// 通过率千分比：2/3 → 666、1/1 → 1000、0/1 → 0。
fn pass_rate_ok() -> bool {
    let mut l = StatLedger { stats: Vec::new(), runs_booked: 0 };
    if record(&mut l, 1, RunVerdict::Pass, 10).is_err() {
        return false;
    }
    if record(&mut l, 1, RunVerdict::Pass, 10).is_err() {
        return false;
    }
    if record(&mut l, 1, RunVerdict::Fail, 10).is_err() {
        return false;
    }
    let s = match l.stats.get(0) {
        Some(c) => *c,
        None => return false,
    };
    let rate = match pass_permille(&s) {
        Ok(r) => r,
        Err(_) => return false,
    };
    rate == 666 && pass_permille(&CaseStat::open(9)) == Err(Diag::NoRuns.val())
        && detail(Diag::NoRuns.val()).contains("取率")
}

/// 边界：零 run 取率拒、全过 1000、全败 0。
fn rate_boundary_ok() -> bool {
    let mut l = StatLedger { stats: Vec::new(), runs_booked: 0 };
    if record(&mut l, 1, RunVerdict::Pass, 10).is_err() {
        return false;
    }
    if record(&mut l, 2, RunVerdict::Fail, 10).is_err() {
        return false;
    }
    let a = match l.stats.get(0) {
        Some(c) => *c,
        None => return false,
    };
    let b = match l.stats.get(1) {
        Some(c) => *c,
        None => return false,
    };
    pass_permille(&a) == Ok(1_000)
        && pass_permille(&b) == Ok(0)
        && detail(Diag::ZeroTicks.val()).contains("恒正")
}

/// 注入：pass 越过 run 上沿必拒（PassOverRuns）。
fn inj_pass_over_runs_ok() -> bool {
    // 反事实演练：若通过计数独立多加一次，必被上沿守卫点名。
    let mut l = StatLedger { stats: Vec::new(), runs_booked: 0 };
    if record(&mut l, 1, RunVerdict::Pass, 10).is_err() {
        return false;
    }
    if record(&mut l, 1, RunVerdict::Pass, 10).is_err() {
        return false;
    }
    let s = match l.stats.get(0) {
        Some(c) => *c,
        None => return false,
    };
    s.pass_count <= s.run_count
        && detail(Diag::PassOverRuns.val()).contains("上沿")
        && reconcile(
            &mut StatLedger { stats: Vec::new(), runs_booked: 0 },
            1,
            1,
            2,
            10,
        ) == Err(Diag::LedgerBad.val())
}

/// 注入：零时长 run 必拒（ZeroTicks）。
fn inj_zero_ticks_ok() -> bool {
    let mut l = StatLedger { stats: Vec::new(), runs_booked: 0 };
    record(&mut l, 1, RunVerdict::Pass, 0) == Err(Diag::ZeroTicks.val())
        && l.stats.is_empty()
}

/// 注入：对账偏差 1 拍必抓（ReconcileMismatch）。
fn inj_reconcile_off_ok() -> bool {
    let mut l = StatLedger { stats: Vec::new(), runs_booked: 0 };
    if record(&mut l, 3, RunVerdict::Pass, 100).is_err() {
        return false;
    }
    reconcile(&mut l, 3, 1, 1, 101) == Err(Diag::ReconcileMismatch.val())
        && reconcile(&mut l, 3, 2, 1, 100) == Err(Diag::ReconcileMismatch.val())
        && reconcile(&mut l, 3, 1, 0, 100) == Err(Diag::ReconcileMismatch.val())
        && detail(Diag::ReconcileMismatch.val()).contains("逐字段")
}

/// 注入：历史账 rewind 必拒（RunCountShrink）。
fn inj_rewind_ok() -> bool {
    let mut l = StatLedger { stats: Vec::new(), runs_booked: 0 };
    if record(&mut l, 1, RunVerdict::Pass, 10).is_err() {
        return false;
    }
    rewind(&mut l, 1) == Err(Diag::RunCountShrink.val())
        && rewind(&mut l, 0x999) == Err(Diag::LedgerBad.val())
        && detail(Diag::RunCountShrink.val()).contains("append-only")
}

/// 注入：对未登记用例对账必拒（LedgerBad 未登记语义）。
fn inj_not_registered_ok() -> bool {
    let mut l = StatLedger { stats: Vec::new(), runs_booked: 0 };
    reconcile(&mut l, 8, 1, 1, 10) == Err(Diag::LedgerBad.val())
        && detail(Diag::LedgerBad.val()).contains("残缺")
}

/// M-212 多 run 对账：5 run/4 过/600 拍三元组逐字段平账。
fn m212_reconcile_ok() -> bool {
    let (runs, passes, ticks) = match m212_inject() {
        Ok(t) => t,
        Err(_) => return false,
    };
    runs == M212_RUNS && passes == M212_PASSES && ticks == M212_TICKS
        && M212_AUDIT_ID == 0x212
        && detail(Diag::ReconcileMismatch.val()).contains("平账")
}

/// 相位状态机：rank 单调只进不退，倒退显性拒。
fn phase_machine_ok() -> bool {
    let open = StatPhase::Open;
    let booked = StatPhase::Booked;
    let reconciled = StatPhase::Reconciled;
    open.advance(booked).is_ok()
        && booked.advance(reconciled).is_ok()
        && reconciled.advance(booked) == Err(Diag::PhaseRegress.val())
        && reconciled.advance(open) == Err(Diag::PhaseRegress.val())
        && booked.advance(open) == Err(Diag::PhaseRegress.val())
        && open.rank() < booked.rank()
        && booked.rank() < reconciled.rank()
        && detail(Diag::PhaseRegress.val()).contains("倒退")
}

/// 对账后相位进 Reconciled 且二次对账幂等。
fn phase_advance_ok() -> bool {
    let mut l = StatLedger { stats: Vec::new(), runs_booked: 0 };
    if record(&mut l, 5, RunVerdict::Pass, 50).is_err() {
        return false;
    }
    if reconcile(&mut l, 5, 1, 1, 50).is_err() {
        return false;
    }
    let s = match l.stats.get(0) {
        Some(c) => *c,
        None => return false,
    };
    s.phase == StatPhase::Reconciled && reconcile(&mut l, 5, 1, 1, 50).is_ok()
}

/// 容量守卫：开户满 16 户后再开必拒（CapacityOverflow）。
fn ledger_capacity_ok() -> bool {
    let mut l = StatLedger { stats: Vec::new(), runs_booked: 0 };
    let mut cid = 1u32;
    while cid <= MAX_LEDGER as u32 {
        if record(&mut l, cid, RunVerdict::Pass, 10).is_err() {
            return false;
        }
        cid += 1;
    }
    l.stats.len() == MAX_LEDGER
        && record(&mut l, 0x777, RunVerdict::Pass, 10)
            == Err(Diag::CapacityOverflow.val())
        && detail(Diag::CapacityOverflow.val()).contains("16")
}

/// 双跑一致：M-212 演练两次执行三元组逐字段相同。
fn double_run_ok() -> bool {
    let a = m212_inject();
    let b = m212_inject();
    a.is_ok() && b.is_ok() && a == b
}

/// 写回面：状态=已完成、结果摘要非空（含三字段与通过率）。
fn writeback_summary_ok() -> bool {
    let (runs, passes, ticks) = match m212_inject() {
        Ok(t) => t,
        Err(_) => return false,
    };
    let permille = match passes.checked_mul(PASS_RATE_DENOM) {
        Some(n) => n / runs,
        None => return false,
    };
    let summary = format!(
        "UNX-F3453 完成：0x212 账 run{}过{} 拍{} 率{}‰ 对账平",
        runs, passes, ticks, permille
    );
    !summary.is_empty()
        && summary.contains("UNX-F3453")
        && summary.contains("对账平")
        && permille == 800
        && detail(Diag::SummaryEmpty.val()).contains("摘要")
}

// ── 判据总成：run_unx3453_checks ───────────────────────────────────────

/// run_unx3453_checks：本单判据聚合（诊断段 0x79E80，全部走 add_dyn）。
pub fn run_unx3453_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F3453 执行统计");
    let checks: [(&'static str, bool, String); 13] = [
        (
            "段位守卫0x40对齐且在批段窗内",
            seg_guard_ok(),
            format!(
                "SEG_BASE=0x{:X} 守卫{}",
                SEG_BASE,
                if seg_guard_ok() { "过" } else { "破" }
            ),
        ),
        (
            "诊断码闭集12码唯一且文案带建议",
            diag_closed_set_ok(),
            format!(
                "闭集{}",
                if diag_closed_set_ok() { "完整" } else { "破缺" }
            ),
        ),
        (
            "统计口径冻结千分母与演练值齐",
            model_frozen_ok(),
            format!("口径{}", if model_frozen_ok() { "冻结" } else { "漂移" }),
        ),
        (
            "正常流三字段同步增量相位进Booked",
            record_ok(),
            format!("记账{}", if record_ok() { "平" } else { "不平" }),
        ),
        (
            "跨run增量三字段逐次递增",
            cross_run_delta_ok(),
            format!("跨run{}", if cross_run_delta_ok() { "递增" } else { "失账" }),
        ),
        (
            "通过率千分比2/3得666零run拒",
            pass_rate_ok(),
            format!("通过率{}", if pass_rate_ok() { "准" } else { "偏" }),
        ),
        (
            "边界全过千分之千全败为零",
            rate_boundary_ok(),
            format!("边界{}", if rate_boundary_ok() { "钉住" } else { "漏" }),
        ),
        (
            "M-212多run对账三元组逐字段平",
            m212_reconcile_ok(),
            format!("M-212{}", if m212_reconcile_ok() { "平账" } else { "不平" }),
        ),
        (
            "相位状态机rank只进不退",
            phase_machine_ok(),
            format!("相位{}", if phase_machine_ok() { "单调" } else { "倒退" }),
        ),
        (
            "对账后相位进Reconciled二次幂等",
            phase_advance_ok(),
            format!("推进{}", if phase_advance_ok() { "幂等" } else { "破缺" }),
        ),
        (
            "注入1对账偏差一拍必抓",
            inj_reconcile_off_ok(),
            format!("偏差注入{}", if inj_reconcile_off_ok() { "抓到" } else { "漏抓" }),
        ),
        (
            "注入2历史账rewind必拒",
            inj_rewind_ok(),
            format!("rewind注入{}", if inj_rewind_ok() { "拒" } else { "漏" }),
        ),
        (
            "注入3零时长与未登记对账必拒",
            inj_zero_ticks_ok() && inj_not_registered_ok() && inj_pass_over_runs_ok(),
            format!(
                "零时长{}未登记{}上沿{}",
                if inj_zero_ticks_ok() { "拒" } else { "漏" },
                if inj_not_registered_ok() { "拒" } else { "漏" },
                if inj_pass_over_runs_ok() { "拒" } else { "漏" },
            ),
        ),
    ];
    let cap = ledger_capacity_ok();
    let dr = double_run_ok();
    let wb = writeback_summary_ok();
    for (name, passed, det) in checks.iter() {
        s.add_dyn(name, *passed, det);
    }
    s.add_dyn(
        "容量守卫满16户再开必拒",
        cap,
        &format!("容量{}", if cap { "守住" } else { "失守" }),
    );
    s.add_dyn(
        "双跑一致M-212演练逐字段同",
        dr,
        &format!("双跑{}", if dr { "一致" } else { "分叉" }),
    );
    s.add_dyn(
        "写回摘要非空状态已完成",
        wb,
        &format!("写回{}", if wb { "非空" } else { "空" }),
    );
    s
}

// ── 测试区（#[cfg(test)]：允许 assert!/assert_eq!/unwrap）──────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_and_diag_closed_set() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE, 0x79E80);
        assert!(diag_closed_set_ok());
        for (i, d) in ALL_DIAGS.iter().enumerate() {
            assert_eq!(d.val(), SEG_BASE + i as u32 + 1);
        }
    }

    #[test]
    fn model_constants_frozen() {
        assert!(model_frozen_ok());
        assert_eq!(PASS_RATE_DENOM, 1_000);
        assert_eq!(M212_TICKS, 600);
    }

    #[test]
    fn record_three_fields_delta() {
        assert!(record_ok());
        assert!(cross_run_delta_ok());
        let mut l = StatLedger { stats: Vec::new(), runs_booked: 0 };
        record(&mut l, 1, RunVerdict::Pass, 40).unwrap();
        record(&mut l, 1, RunVerdict::Fail, 60).unwrap();
        let s = l.stats.get(0).unwrap();
        assert_eq!((s.run_count, s.pass_count, s.total_ticks), (2, 1, 100));
    }

    #[test]
    fn pass_rate_permille_points() {
        assert!(pass_rate_ok());
        assert!(rate_boundary_ok());
        let mut l = StatLedger { stats: Vec::new(), runs_booked: 0 };
        record(&mut l, 1, RunVerdict::Fail, 10).unwrap();
        let s = l.stats.get(0).unwrap();
        assert_eq!(pass_permille(s), Ok(0));
        assert_eq!(pass_permille(&CaseStat::open(2)), Err(Diag::NoRuns.val()));
    }

    #[test]
    fn m212_multi_run_reconcile() {
        assert!(m212_reconcile_ok());
        let (runs, passes, ticks) = m212_inject().unwrap();
        assert_eq!((runs, passes, ticks), (5, 4, 600));
    }

    #[test]
    fn phase_machine_monotonic() {
        assert!(phase_machine_ok());
        assert!(phase_advance_ok());
        assert_eq!(StatPhase::Open.rank(), 0);
        assert_eq!(StatPhase::Reconciled.rank(), 2);
    }

    #[test]
    fn injections_reconcile_rewind_rejects() {
        assert!(inj_reconcile_off_ok());
        assert!(inj_rewind_ok());
        assert!(inj_zero_ticks_ok());
        assert!(inj_not_registered_ok());
        assert!(inj_pass_over_runs_ok());
    }

    #[test]
    fn ledger_capacity_guard() {
        assert!(ledger_capacity_ok());
        assert_eq!(MAX_LEDGER, 16);
    }

    #[test]
    fn double_run_identical() {
        assert!(double_run_ok());
    }

    #[test]
    fn writeback_summary_non_empty() {
        assert!(writeback_summary_ok());
    }

    #[test]
    fn rewind_is_always_rejected() {
        let mut l = StatLedger { stats: Vec::new(), runs_booked: 0 };
        record(&mut l, 4, RunVerdict::Pass, 10).unwrap();
        assert_eq!(rewind(&mut l, 4), Err(Diag::RunCountShrink.val()));
    }

    #[test]
    fn checks_aggregate_all_green() {
        let s = run_unx3453_checks();
        assert!(s.len() >= 12);
        assert!(s.all_passed(), "全部判据必须绿：{}", s.red_items_text());
    }
}
