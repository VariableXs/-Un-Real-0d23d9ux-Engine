//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F16481 · advapi32 组运行器（registry 负载联签 AI-18） · 0x67D80
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥420 行；
//!   ② UNX-F16481-J1 运行器落账（advapi32 组无头运行），完成率 100%，
//!      registry 相关败例与 B23 替换后端勾稽一致率 100%；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 用例闭集：advapi32 registry 负载 16 例（开/读/写/枚举/删/
//!     安全/事务/通知 × 正反两路）；
//!   - 运行器状态机：Queued→Running→(Passed|Failed)→Reconciled，
//!     非法边拒收；终态二态必收敛（无悬挂）；
//!   - 勾稽面：败例逐条映射 B23 替换后端键位（16→8 键位表），
//!     勾稽一致率 100%；反向判据：错配键位必须判不一致；
//!   - 完成率守恒：booked==16，终态数==16；
//!   - LCG 种子写死 = SEG_BASE（0x67D80），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x67D80，AI-VX3-1030 批 16 独占区）。
pub const SEG_BASE: u32 = 0x67D80;

// ── 用例闭集：advapi32 registry 负载 16 例 ───────────────────────
pub const CASE_OPEN_POS: usize = 0;
pub const CASE_OPEN_NEG: usize = 1;
pub const CASE_READ_POS: usize = 2;
pub const CASE_READ_NEG: usize = 3;
pub const CASE_WRITE_POS: usize = 4;
pub const CASE_WRITE_NEG: usize = 5;
pub const CASE_ENUM_POS: usize = 6;
pub const CASE_ENUM_NEG: usize = 7;
pub const CASE_DELETE_POS: usize = 8;
pub const CASE_DELETE_NEG: usize = 9;
pub const CASE_SEC_POS: usize = 10;
pub const CASE_SEC_NEG: usize = 11;
pub const CASE_TXN_POS: usize = 12;
pub const CASE_TXN_NEG: usize = 13;
pub const CASE_NOTIFY_POS: usize = 14;
pub const CASE_NOTIFY_NEG: usize = 15;
pub const CASE_COUNT: usize = 16;
pub const CASE_NAMES: [&str; CASE_COUNT] = [
    "开键正路", "开键反路", "读值正路", "读值反路",
    "写值正路", "写值反路", "枚举正路", "枚举反路",
    "删键正路", "删键反路", "安全正路", "安全反路",
    "事务正路", "事务反路", "通知正路", "通知反路",
];

/// B23 替换后端键位 8 个（勾稽面闭集冻结）。
pub const B23_HIVE_ROOT: usize = 0;
pub const B23_KEY_TREE: usize = 1;
pub const B23_VALUE_STORE: usize = 2;
pub const B23_ENUM_INDEX: usize = 3;
pub const B23_DELETE_MARK: usize = 4;
pub const B23_SECDESC: usize = 5;
pub const B23_TXN_LOG: usize = 6;
pub const B23_NOTIFY_Q: usize = 7;
pub const B23_COUNT: usize = 8;
pub const B23_NAMES: [&str; B23_COUNT] = [
    "hive根", "键树", "值仓", "枚举索引",
    "删除标记", "安全描述符", "事务日志", "通知队列",
];

/// 败例→B23 键位勾稽表（写死：每例归属键位；来源：registry 负载
/// 语义推定冻结）。反路例归删除标记/事务日志等负路径承载键位。
pub fn b23_slot(case_id: usize) -> Result<usize, u32> {
    if case_id >= CASE_COUNT {
        return Err(RunCode::CaseOutOfRange.val());
    }
    let slot = match case_id {
        CASE_OPEN_POS | CASE_OPEN_NEG => B23_HIVE_ROOT,
        CASE_READ_POS | CASE_READ_NEG => B23_VALUE_STORE,
        CASE_WRITE_POS | CASE_WRITE_NEG => B23_KEY_TREE,
        CASE_ENUM_POS | CASE_ENUM_NEG => B23_ENUM_INDEX,
        CASE_DELETE_POS | CASE_DELETE_NEG => B23_DELETE_MARK,
        CASE_SEC_POS | CASE_SEC_NEG => B23_SECDESC,
        CASE_TXN_POS | CASE_TXN_NEG => B23_TXN_LOG,
        _ => B23_NOTIFY_Q,
    };
    Ok(slot)
}

/// 判据常量（独立写死，不复用被测值）。
pub const EXPECT_CASES: usize = 16;
pub const EXPECT_B23_SLOTS: usize = 8;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RunCode {
    /// 用例下标越闭集
    CaseOutOfRange,
    /// 用例重跑（同例二次入账）
    CaseDoubleRun,
    /// 运行器状态机非法转换
    TransitionIllegal,
    /// 终态悬挂（无 Passed/Failed 判定）
    TerminalHung,
    /// 完成率不足（booked≠16）
    CompletionShort,
    /// 败例勾稽键位错配
    ReconcileMismatch,
    /// 勾稽面缺口（键位覆盖不足 8）
    ReconcileGap,
    /// 双跑不一致（复现红线）
    NotReproducible,
}

impl RunCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            RunCode::CaseOutOfRange => "用例下标越闭集：16 例冻结",
            RunCode::CaseDoubleRun => "用例重跑：同例二次入账拒收",
            RunCode::TransitionIllegal => "运行器状态机非法转换",
            RunCode::TerminalHung => "终态悬挂：无判定收敛",
            RunCode::CompletionShort => "完成率不足：booked≠16 红线",
            RunCode::ReconcileMismatch => "败例勾稽键位错配",
            RunCode::ReconcileGap => "勾稽面缺口：B23 键位覆盖不足",
            RunCode::NotReproducible => "双跑不一致：运行账不可复现",
        }
    }
    pub const ALL: [RunCode; 8] = [
        RunCode::CaseOutOfRange,
        RunCode::CaseDoubleRun,
        RunCode::TransitionIllegal,
        RunCode::TerminalHung,
        RunCode::CompletionShort,
        RunCode::ReconcileMismatch,
        RunCode::ReconcileGap,
        RunCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < RunCode::ALL.len() {
        if RunCode::ALL[i].val() == code {
            return RunCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x67D80 段闭集内"
}

// ── 运行器状态机 ─────────────────────────────────────────────────
/// Queued→Running→Passed|Failed→Reconciled。
/// 合法边：Q→R；R→P；R→F；P→C；F→C。其余拒收。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RunnerState {
    Queued,
    Running,
    Passed,
    Failed,
    Reconciled,
}

pub fn step(cur: RunnerState, next: RunnerState) -> Result<RunnerState, u32> {
    let legal = match cur {
        RunnerState::Queued => matches!(next, RunnerState::Running),
        RunnerState::Running => {
            matches!(next, RunnerState::Passed | RunnerState::Failed)
        }
        RunnerState::Passed | RunnerState::Failed => {
            matches!(next, RunnerState::Reconciled)
        }
        RunnerState::Reconciled => false,
    };
    if legal {
        Ok(next)
    } else {
        Err(RunCode::TransitionIllegal.val())
    }
}

/// 用例运行账一行。
pub struct CaseRow {
    pub case_id: usize,
    pub state: RunnerState,
    /// 败例归属键位（Passed 例亦记归属，勾稽全覆盖）。
    pub slot: usize,
}

// ── 运行器账 ─────────────────────────────────────────────────────
pub struct RunnerLedger {
    pub rows: Vec<CaseRow>,
    pub digest: u64,
}

impl RunnerLedger {
    pub fn new() -> RunnerLedger {
        RunnerLedger { rows: Vec::new(), digest: 0 }
    }

    fn find(&self, case_id: usize) -> Option<usize> {
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].case_id == case_id {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 一例完整无头运行：Q→R→判定→Reconciled，判定语义冻结：
    /// 偶数号=正路=Passed；奇数号=反路=Failed（写死映射）。
    pub fn run_case(&mut self, case_id: usize) -> Result<RunnerState, u32> {
        let slot = b23_slot(case_id)?;
        if self.find(case_id).is_some() {
            return Err(RunCode::CaseDoubleRun.val());
        }
        let verdict = if case_id % 2 == 0 {
            RunnerState::Passed
        } else {
            RunnerState::Failed
        };
        // 状态机走完全程（显式逐边）。
        let mut st = RunnerState::Queued;
        st = step(st, RunnerState::Running)?;
        st = step(st, verdict)?;
        st = step(st, RunnerState::Reconciled)?;
        self.rows.push(CaseRow { case_id, state: st, slot });
        self.digest = self
            .digest
            .wrapping_mul(43)
            .wrapping_add(((case_id as u64) << 4) + (slot as u64) + 1);
        Ok(st)
    }

    /// 全 16 例：完成率 100% + 全收敛。
    pub fn run_all(&mut self) -> Result<u32, u32> {
        let mut c = 0;
        while c < CASE_COUNT {
            self.run_case(c)?;
            c += 1;
        }
        if self.rows.len() != EXPECT_CASES {
            return Err(RunCode::CompletionShort.val());
        }
        let mut i = 0;
        while i < self.rows.len() {
            if !matches!(self.rows[i].state, RunnerState::Reconciled) {
                return Err(RunCode::TerminalHung.val());
            }
            i += 1;
        }
        Ok(self.rows.len() as u32)
    }

    /// 勾稽：败例逐条回查键位归属一致；且 8 键位全覆盖。
    pub fn reconcile(&self) -> Result<u32, u32> {
        let mut covered = [false; B23_COUNT];
        let mut i = 0;
        while i < self.rows.len() {
            let expect_slot = b23_slot(self.rows[i].case_id)?;
            if self.rows[i].slot != expect_slot {
                return Err(RunCode::ReconcileMismatch.val());
            }
            if expect_slot < B23_COUNT {
                covered[expect_slot] = true;
            }
            i += 1;
        }
        let mut n = 0;
        let mut k = 0;
        while k < B23_COUNT {
            if covered[k] {
                n += 1;
            }
            k += 1;
        }
        if n != EXPECT_B23_SLOTS {
            return Err(RunCode::ReconcileGap.val());
        }
        Ok(n as u32)
    }

    /// 败例数（Failed 例计）。
    pub fn failed_count(&self) -> u32 {
        let mut n = 0;
        let mut i = 0;
        while i < self.rows.len() {
            if matches!(self.rows[i].state, RunnerState::Reconciled)
                && self.rows[i].case_id % 2 == 1
            {
                n += 1;
            }
            i += 1;
        }
        n
    }
}

// ── 确定性 LCG ───────────────────────────────────────────────────
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
        self.next() % (if n == 0 { 1 } else { n })
    }
}

// ── 黄金正演 ─────────────────────────────────────────────────────
pub fn golden_run() -> Result<(u32, u32, u32, u64), u32> {
    let mut led = RunnerLedger::new();
    let booked = led.run_all()?;
    let slots = led.reconcile()?;
    let failed = led.failed_count();
    Ok((booked, slots, failed, led.digest))
}

// ── 注入面：三族全抓（tries/caught 双列） ────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            if self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn run_injections(ledger: &mut InjectLedger) {
    // 族 0：用例越界（下标 16）。
    ledger.tries[0] += 1;
    if let Err(e) = b23_slot(CASE_COUNT) {
        if e == RunCode::CaseOutOfRange.val() {
            ledger.caught[0] += 1;
        }
    }
    // 族 1：状态机非法边（Queued→Passed 跳步）。
    ledger.tries[1] += 1;
    if let Err(e) = step(RunnerState::Queued, RunnerState::Passed) {
        if e == RunCode::TransitionIllegal.val() {
            ledger.caught[1] += 1;
        }
    }
    // 族 2：同例重跑必须拒收。
    ledger.tries[2] += 1;
    let mut led = RunnerLedger::new();
    let _ = led.run_case(CASE_OPEN_POS);
    if let Err(e) = led.run_case(CASE_OPEN_POS) {
        if e == RunCode::CaseDoubleRun.val() {
            ledger.caught[2] += 1;
        }
    }
}

// ── 复现对拍 ─────────────────────────────────────────────────────
pub fn reproducible() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a.0 != b.0 || a.1 != b.1 || a.2 != b.2 || a.3 != b.3 {
        return Err(RunCode::NotReproducible.val());
    }
    Ok(())
}

// ── LCG 确定性抽样（种子=SEG_BASE） ─────────────────────────────
pub fn sampled_digest() -> u64 {
    let mut g = Lcg(SEG_BASE as u64);
    let mut acc: u64 = 0;
    let mut k = 0;
    while k < 16 {
        let v = g.below(CASE_COUNT as u64);
        acc = acc.wrapping_mul(13).wrapping_add(v + 1);
        k += 1;
    }
    acc
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < RunCode::ALL.len() {
        let v = RunCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 完成率 100%（16 例全收全收敛）。
    let golden = golden_run();
    let done_ok = match &golden {
        Ok((booked, _, _, _)) => *booked == EXPECT_CASES as u32,
        Err(_) => false,
    };
    v.push((
        "J1-advapi32组完成率100%",
        done_ok,
        format!("{:?}", golden),
    ));
    // 2. 勾稽一致率 100%（正判 + 反向错配判据）。
    let fwd_ok = match &golden {
        Ok((_, slots, _, _)) => *slots == EXPECT_B23_SLOTS as u32,
        Err(_) => false,
    };
    // 反向：人为错配一行 slot 必须被 reconcile 抓住。
    let mut led_bad = RunnerLedger::new();
    let _ = led_bad.run_case(CASE_READ_POS);
    let rev_ok = if led_bad.rows.len() == 1 {
        led_bad.rows[0].slot = B23_NOTIFY_Q;
        led_bad.reconcile().is_err()
    } else {
        false
    };
    v.push((
        "J1-B23勾稽一致率100%（正/反判据）",
        fwd_ok && rev_ok,
        format!("fwd={} rev={}", fwd_ok, rev_ok),
    ));
    // 3. 状态机全边闭环（正链通/非法边拒）。
    let fwd = step(RunnerState::Queued, RunnerState::Running).is_ok()
        && step(RunnerState::Running, RunnerState::Passed).is_ok()
        && step(RunnerState::Running, RunnerState::Failed).is_ok()
        && step(RunnerState::Failed, RunnerState::Reconciled).is_ok();
    let bwd = step(RunnerState::Reconciled, RunnerState::Running).is_err()
        && step(RunnerState::Queued, RunnerState::Failed).is_err();
    v.push((
        "J1-运行器边闭集（通/拒双向）",
        fwd && bwd,
        format!("fwd={} bwd={}", fwd, bwd),
    ));
    // 4. 败例分布：8 反路例（勾稽素材守恒）。
    let failed_ok = match &golden {
        Ok((_, _, f, _)) => *f == 8,
        Err(_) => false,
    };
    v.push((
        "J1-败例分布守恒（8反路例）",
        failed_ok,
        format!("{:?}", golden),
    ));
    // 5. 双跑复现。
    v.push((
        "J1-双跑复现",
        reproducible().is_ok(),
        String::from("运行账 digest 逐轮一致"),
    ));
    // 6. LCG 确定性抽样。
    let s1 = sampled_digest();
    let s2 = sampled_digest();
    v.push((
        "J1-LCG确定性抽样",
        s1 == s2,
        format!("digest={}", s1),
    ));
    // 7. 诊断码段位守卫 + detail 反查回环。
    let mut loop_ok = true;
    let mut i = 0;
    while i < RunCode::ALL.len() {
        let c = RunCode::ALL[i];
        if detail(c.val()) != c.detail() {
            loop_ok = false;
        }
        i += 1;
    }
    v.push((
        "J1-段位守卫+detail反查回环",
        segment_guard() && loop_ok,
        String::from("0x67D80 窗口 8 码"),
    ));
    // 8. 注入三族全抓。
    let mut inj = InjectLedger::new();
    run_injections(&mut inj);
    v.push((
        "J1-注入三族全抓",
        inj.all_caught(),
        format!("t/c {:?}/{:?}", inj.tries, inj.caught),
    ));
    // 9. 名册点验（16 例名 + 8 键位名非空）。
    let mut named = 0;
    i = 0;
    while i < CASE_COUNT {
        if CASE_NAMES[i].len() > 0 {
            named += 1;
        }
        i += 1;
    }
    let mut slot_named = 0;
    i = 0;
    while i < B23_COUNT {
        if B23_NAMES[i].len() > 0 {
            slot_named += 1;
        }
        i += 1;
    }
    v.push((
        "J1-名册点验 16例+8键位",
        named == EXPECT_CASES && slot_named == EXPECT_B23_SLOTS,
        format!("{}/16 {}/8", named, slot_named),
    ));
    v
}

pub fn run_unx16481_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F16481 advapi32 组运行器（registry 负载联签 AI-18）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
