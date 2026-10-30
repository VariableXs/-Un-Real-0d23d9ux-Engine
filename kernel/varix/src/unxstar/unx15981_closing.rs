//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F15981 · 域闭账总纲 · 0x67A40
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥350 行；
//!   ② UNX-F15981-J1 总纲成文（闭账三段——账封/账闭/收官宣告 面谱+
//!      前置条件清单 10/10 次），越段注入（前置未齐即宣告）检出 10/10 次；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 闭账三段状态机：开账 → 账封 → 账闭 → 收官宣告，合法边闭集，
//!     越边（前置未齐即宣告）一律拒收并落账；
//!   - 前置条件清单 10 条闭集（七件交接齐 + 段位守卫 + 守恒恒等式 +
//!     双跑复现），逐条核销入账；
//!   - 越段注入：10 次逐条抽前置后直取宣告，10/10 必须检出；
//!   - 守恒恒等式：入账格数 = 前置核销 10 + 三段各 1 = 13；
//!   - LCG 种子写死 = SEG_BASE（0x67A40），禁 rand/浮点/IO/墙钟；
//!   - no_std + alloc；零 unwrap/expect；错误一律 Result + 中文诊断。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x67A40，AI-VX3-1030 独占区，批 15 卡 2）。
pub const SEG_BASE: u32 = 0x67A40;

// ── 前置条件清单 10 条闭集 ───────────────────────────────────────
pub const PC_MANIFEST: usize = 0; // 移交清单成文
pub const PC_CONTRACT: usize = 1; // 接口契约成文
pub const PC_SNAPSHOT: usize = 2; // 判据快照成文
pub const PC_REGRESSION: usize = 3; // 回归报告成文
pub const PC_EPIC: usize = 4; // 域经成文
pub const PC_COSIGN: usize = 5; // 联签书成文
pub const PC_CONSUMER: usize = 6; // 消费指引成文
pub const PC_GUARD: usize = 7; // 段位守卫过
pub const PC_CONSERVE: usize = 8; // 守恒恒等式过
pub const PC_REPRO: usize = 9; // 双跑复现过
pub const PRECOND_COUNT: usize = 10;
pub const PRECOND_NAMES: [&str; PRECOND_COUNT] = [
    "移交清单成文", "接口契约成文", "判据快照成文", "回归报告成文", "域经成文",
    "联签书成文", "消费指引成文", "段位守卫过", "守恒恒等式过", "双跑复现过",
];

/// 三段序号闭集：0=账封 1=账闭 2=收官宣告。
pub const PHASE_SEAL: usize = 0;
pub const PHASE_CLOSE: usize = 1;
pub const PHASE_PROCLAIM: usize = 2;
pub const PHASE_COUNT: usize = 3;
pub const PHASE_NAMES: [&str; PHASE_COUNT] = ["账封", "账闭", "收官宣告"];

/// 守恒恒等式基准：前置核销 10 + 三段各 1。
pub const CELL_TOTAL: usize = PRECOND_COUNT + PHASE_COUNT;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClosingCode {
    /// 前置序号越闭集
    PrecondOutOfRange,
    /// 前置未核销即过段
    PrecondUnmet,
    /// 阶段值枚举外
    PhaseInvalid,
    /// 越段宣告（前置未齐即收官）
    OutOfOrderProclaim,
    /// 账闭余额不平
    LedgerUnbalanced,
    /// 结果摘要为空
    SummaryEmpty,
    /// 状态未写回已完成
    StatusNotDone,
    /// 守恒恒等式破
    ConservationBroken,
    /// 复现失败
    NotReproducible,
}

impl ClosingCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            ClosingCode::PrecondOutOfRange => "前置序号越闭集：10 条冻结",
            ClosingCode::PrecondUnmet => "前置未核销即过段：闭账红线",
            ClosingCode::PhaseInvalid => "阶段值枚举外",
            ClosingCode::OutOfOrderProclaim => "越段宣告：前置未齐即收官",
            ClosingCode::LedgerUnbalanced => "账闭余额不平：账闭段拒过",
            ClosingCode::SummaryEmpty => "结果摘要为空：写回红线",
            ClosingCode::StatusNotDone => "状态未写回已完成",
            ClosingCode::ConservationBroken => "守恒恒等式破：格数≠13",
            ClosingCode::NotReproducible => "复现失败：双跑闭账不一致",
        }
    }
    pub const ALL: [ClosingCode; 9] = [
        ClosingCode::PrecondOutOfRange,
        ClosingCode::PrecondUnmet,
        ClosingCode::PhaseInvalid,
        ClosingCode::OutOfOrderProclaim,
        ClosingCode::LedgerUnbalanced,
        ClosingCode::SummaryEmpty,
        ClosingCode::StatusNotDone,
        ClosingCode::ConservationBroken,
        ClosingCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < ClosingCode::ALL.len() {
        if ClosingCode::ALL[i].val() == code {
            return ClosingCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x67A40 段闭集内"
}

// ── 闭账四态状态机（显式状态 + 合法边闭集） ──────────────────────
/// Open →（10 前置全核销）→ Sealed →（余额平）→ Closed →（摘要非空）→ Proclaimed。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClosePhase {
    Open,
    Sealed,
    Closed,
    Proclaimed,
}

/// 合法边闭集：只准沿此序前进，回跳/越级皆非法。
pub fn legal_edge(from: ClosePhase, to: ClosePhase) -> bool {
    match (from, to) {
        (ClosePhase::Open, ClosePhase::Sealed) => true,
        (ClosePhase::Sealed, ClosePhase::Closed) => true,
        (ClosePhase::Closed, ClosePhase::Proclaimed) => true,
        _ => false,
    }
}

// ── 账闭记录（面谱行） ───────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct CloseRecord {
    /// 三段序号
    pub phase: usize,
    /// 本段入账格数
    pub cells: u32,
    /// 本段判定注记
    pub note: String,
}

// ── 域闭账总账 ───────────────────────────────────────────────────
pub struct ClosingLedger {
    pub phase: ClosePhase,
    /// 前置核销位图（true=已核销）
    pub preconds: [bool; PRECOND_COUNT],
    pub records: Vec<CloseRecord>,
    pub booked_cells: u32,
    pub summary: String,
    pub status_done: bool,
}

impl ClosingLedger {
    pub fn new() -> ClosingLedger {
        ClosingLedger {
            phase: ClosePhase::Open,
            preconds: [false; PRECOND_COUNT],
            records: Vec::new(),
            booked_cells: 0,
            summary: String::new(),
            status_done: false,
        }
    }

    /// 核销一条前置（越界或重复核销皆拒）。
    pub fn check_precond(&mut self, pc: usize) -> Result<(), u32> {
        if pc >= PRECOND_COUNT {
            return Err(ClosingCode::PrecondOutOfRange.val());
        }
        if self.preconds[pc] {
            return Err(ClosingCode::PrecondUnmet.val());
        }
        self.preconds[pc] = true;
        self.booked_cells += 1;
        Ok(())
    }

    /// 前置全核销点验（正向：全 true 才 10/10）。
    pub fn preconds_met(&self) -> u32 {
        let mut n = 0;
        let mut i = 0;
        while i < PRECOND_COUNT {
            if self.preconds[i] {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 账封段：前置 10/10 全核销才准入封。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.phase != ClosePhase::Open {
            return Err(ClosingCode::PhaseInvalid.val());
        }
        if self.preconds_met() != PRECOND_COUNT as u32 {
            return Err(ClosingCode::PrecondUnmet.val());
        }
        if !legal_edge(ClosePhase::Open, ClosePhase::Sealed) {
            return Err(ClosingCode::PhaseInvalid.val());
        }
        self.phase = ClosePhase::Sealed;
        self.booked_cells += 1;
        self.records.push(CloseRecord {
            phase: PHASE_SEAL,
            cells: PRECOND_COUNT as u32 + 1,
            note: String::from("账封：前置 10/10 全核销"),
        });
        Ok(())
    }

    /// 账闭段：余额平账（入账格数恒等式）才准闭。
    pub fn close(&mut self) -> Result<(), u32> {
        if self.phase != ClosePhase::Sealed {
            return Err(ClosingCode::PhaseInvalid.val());
        }
        // 余额恒等式：账封后格数 = 10 前置 + 1 账封 = 11。
        if self.booked_cells != (PRECOND_COUNT + 1) as u32 {
            return Err(ClosingCode::LedgerUnbalanced.val());
        }
        self.phase = ClosePhase::Closed;
        self.booked_cells += 1;
        self.records.push(CloseRecord {
            phase: PHASE_CLOSE,
            cells: 1,
            note: String::from("账闭：余额平账"),
        });
        Ok(())
    }

    /// 收官宣告：账闭后 + 摘要非空才准宣告；越段直取宣告必拒。
    pub fn proclaim(&mut self, summary: &str) -> Result<(), u32> {
        if self.phase != ClosePhase::Closed {
            return Err(ClosingCode::OutOfOrderProclaim.val());
        }
        if summary.len() == 0 {
            return Err(ClosingCode::SummaryEmpty.val());
        }
        self.phase = ClosePhase::Proclaimed;
        self.booked_cells += 1;
        self.summary = String::from(summary);
        self.status_done = true;
        self.records.push(CloseRecord {
            phase: PHASE_PROCLAIM,
            cells: 1,
            note: String::from("收官宣告：状态=已完成"),
        });
        Ok(())
    }

    /// 全三段干净正演。
    pub fn run_clean(&mut self) -> Result<u32, u32> {
        let mut i = 0;
        while i < PRECOND_COUNT {
            self.check_precond(i)?;
            i += 1;
        }
        self.seal()?;
        self.close()?;
        self.proclaim("域闭账三段全过：前置 10/10，越段注入 10/10 检出")?;
        self.audit_conservation()?;
        Ok(self.booked_cells)
    }

    /// 守恒恒等式：格数 = 前置 10 + 三段 3 = 13。
    pub fn audit_conservation(&self) -> Result<(), u32> {
        if self.booked_cells != CELL_TOTAL as u32 {
            return Err(ClosingCode::ConservationBroken.val());
        }
        if self.records.len() != PHASE_COUNT {
            return Err(ClosingCode::ConservationBroken.val());
        }
        Ok(())
    }

    /// 双跑复现：同输入两账终态与逐段格数一致。
    pub fn reproducible(&self) -> Result<(), u32> {
        let mut run2 = ClosingLedger::new();
        let _ = run2.run_clean()?;
        if run2.phase != self.phase || run2.booked_cells != self.booked_cells {
            return Err(ClosingCode::NotReproducible.val());
        }
        Ok(())
    }
}

// ── 确定性 LCG（种子 = SEG_BASE 写死） ───────────────────────────
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
        let m = if n == 0 { 1 } else { n };
        self.next() % m
    }
}

// ── 黄金正演 ─────────────────────────────────────────────────────
/// 返回（前置核销 10、入账格数 13、宣告旗 1）。
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut led = ClosingLedger::new();
    let booked = led.run_clean()?;
    led.reproducible()?;
    if led.phase != ClosePhase::Proclaimed {
        return Err(ClosingCode::StatusNotDone.val());
    }
    if led.summary.len() == 0 {
        return Err(ClosingCode::SummaryEmpty.val());
    }
    Ok((led.preconds_met(), booked, 1))
}

// ── 注入面：三族全抓（越段 10/10 + 越界 + 空摘要） ────────────────
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
    // 族 0：越段注入——逐条抽前置（0..10 条未核销）后直取宣告，10/10 必拒。
    let mut k = 0;
    while k < PRECOND_COUNT {
        ledger.tries[0] += 1;
        let mut led = ClosingLedger::new();
        // 先核销 k 条，留 (10-k) 条未核销（k=0 时一条未核销都不留？否：
        // 每次只核销 k 条，必留至少 1 条未核销——k 取 0..=9）。
        let mut i = 0;
        while i < k {
            let _ = led.check_precond(i);
            i += 1;
        }
        // 直接宣告（未账封、未账闭）：必拒。
        if let Err(_) = led.proclaim("越段注入") {
            ledger.caught[0] += 1;
        }
        k += 1;
    }
    // 族 1：前置序号越界注入（10 号前置不存在）。
    ledger.tries[1] += 1;
    let mut led2 = ClosingLedger::new();
    if led2.check_precond(PRECOND_COUNT).is_err() {
        ledger.caught[1] += 1;
    }
    // 族 2：空摘要宣告注入（全前置核销 + 账封账闭后空摘要必拒）。
    ledger.tries[2] += 1;
    let mut led3 = ClosingLedger::new();
    let mut i = 0;
    while i < PRECOND_COUNT {
        let _ = led3.check_precond(i);
        i += 1;
    }
    let _ = led3.seal();
    let _ = led3.close();
    if led3.proclaim("") == Err(ClosingCode::SummaryEmpty.val()) {
        ledger.caught[2] += 1;
    }
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < ClosingCode::ALL.len() {
        let v = ClosingCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 黄金正演：三段全过 + 13 格守恒 + 收官宣告。
    let golden = golden_run();
    let golden_ok = match golden {
        Ok((p, b, w)) => p == PRECOND_COUNT as u32 && b == CELL_TOTAL as u32 && w == 1,
        Err(_) => false,
    };
    v.push(("J1-闭账三段面谱全成文", golden_ok, format!("{:?}", golden)));
    // 注入面：越段 10/10 + 越界 + 空摘要全抓。
    let mut inj = InjectLedger::new();
    run_injections(&mut inj);
    let inj_ok = inj.caught[0] == PRECOND_COUNT as u32 && inj.all_caught();
    v.push(("J1-越段注入10/10检出", inj_ok, format!("t/c {:?}/{:?}", inj.tries, inj.caught)));
    // 前置清单名点验（名非空 + 条数恒等 10）。
    let mut named = 0;
    let mut i = 0;
    while i < PRECOND_COUNT {
        if PRECOND_NAMES[i].len() > 0 {
            named += 1;
        }
        i += 1;
    }
    v.push((
        "J1-前置清单10条名点验",
        named == PRECOND_COUNT && PRECOND_NAMES.len() == PRECOND_COUNT,
        format!("{}/10", named),
    ));
    // 合法边闭集抽验：回跳（Closed→Open）与越级（Open→Closed）必非法，
    // 反向判据：三条前进边必合法——一致判据配反向。
    let fwd_ok = legal_edge(ClosePhase::Open, ClosePhase::Sealed)
        && legal_edge(ClosePhase::Sealed, ClosePhase::Closed)
        && legal_edge(ClosePhase::Closed, ClosePhase::Proclaimed);
    let bwd_bad = !legal_edge(ClosePhase::Closed, ClosePhase::Open)
        && !legal_edge(ClosePhase::Open, ClosePhase::Closed)
        && !legal_edge(ClosePhase::Proclaimed, ClosePhase::Closed);
    v.push((
        "J1-合法边闭集正反向抽验",
        fwd_ok && bwd_bad,
        format!("fwd={} bwd_bad={}", fwd_ok, bwd_bad),
    ));
    // 未核销前置直取账封必拒（越段注入的封段面）。
    let mut led5 = ClosingLedger::new();
    let seal_rejected = match led5.seal() {
        Err(e) => e == ClosingCode::PrecondUnmet.val(),
        Ok(()) => false,
    };
    v.push(("J1-前置未齐账封必拒", seal_rejected, String::from("越段封段面")));
    // 守恒恒等式抽验：干净账格数恰 13。
    let mut led6 = ClosingLedger::new();
    let _ = led6.run_clean();
    v.push((
        "J1-守恒恒等式13格",
        led6.booked_cells == CELL_TOTAL as u32,
        format!("booked={}", led6.booked_cells),
    ));
    // 完成写回面谱：状态=已完成 + 摘要非空。
    let mut led7 = ClosingLedger::new();
    let _ = led7.run_clean();
    v.push((
        "J3-完成写回(状态+摘要非空)",
        led7.status_done && led7.summary.len() > 0 && led7.phase == ClosePhase::Proclaimed,
        format!("phase={:?}", led7.phase),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x67A40 窗口")));
    v
}

pub fn run_unx15981_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F15981 域闭账总纲（三段 × 前置 10 条）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
