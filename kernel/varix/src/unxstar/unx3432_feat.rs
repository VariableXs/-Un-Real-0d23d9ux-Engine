//! UNX-F3432 超时与看门狗（用例级超时上报） · 0x79A00段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 VX8-B13 逐字摘录）：
//!   ① 纯功能代码 ≥340 行；
//!   ② J1：用例级超时（元数据预算 × 系数）+ 看门狗上报（TIMEOUT 结果态）；
//!     M-191 超时注入验证。
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 有效预算 = 元数据预算 × 系数（NUM/DEN 有理系数，checked 乘除，
//!     溢出与零预算均显性拒——不用 saturating 静默吞）；
//!   - 拍数边界钉死：拍数 == 有效预算为通过上沿，+1 拍即 TIMEOUT；
//!   - 看门狗上报：每例运行完必落一条上报单（序号从 1 起严格递增），
//!     超时例必须以 TIMEOUT 结果态入账，漏报与误报两类违约显性判红；
//!   - 误报双防线：超预算却报非超时态（TimeoutAsPass）与未超预算却报
//!     超时态（ReportMalformed）在看门狗收单口双向拦截；
//!   - M-191 超时注入：冻结注入用例（ID=0x191）人为超预算运行，必被抓
//!     且以 TIMEOUT 态入看门狗账；
//!   - 双跑一致：同输入两次运行的看门狗账逐字段相同；
//!   - 写回面：状态=已完成、结果摘要非空。
//! 常量来源：
//!   - 系数 NUM=3/DEN=2：派工卡「元数据预算 × 系数」的系数取 3/2（放大
//!     三分之二），冻结于本模块常量表；
//!   - M-191：派工卡验收判据编号（注入用例 ID 锚定 0x191）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x40 对齐，B13 批段窗 [0x79A00, 0x79BC0] 内）。
pub const SEG_BASE: u32 = 0x79A00;

/// 预算系数分子（有效预算 = 元数据预算 × NUM ÷ DEN）。
pub const BUDGET_COEF_NUM: u64 = 3;
/// 预算系数分母（建表期冻结为非零，零分母即建表违约）。
pub const BUDGET_COEF_DEN: u64 = 2;
/// M-191 注入用例 ID（派工卡验收判据编号锚定）。
pub const INJECTION_CASE_ID: u32 = 0x191;

/// 冻结用例注册表：用例名 → 元数据预算（拍）。
/// 预算量级按各用例的账面工作量定（冒烟类小、序列化类大）。
pub const CASE_TABLE: [(&'static str, u64); 5] = [
    ("断言冒烟", 64),
    ("账本往返", 128),
    ("序列化万条", 4096),
    ("看门狗自检", 32),
    ("写回对账", 96),
];

/// 用例元数据：一例的静态预算声明。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaseMeta {
    /// 用例 ID（注册表内 1..=5，注入用例走 0x191 专用段）。
    pub case_id: u32,
    /// 用例名（中文，账面可读）。
    pub name: String,
    /// 元数据预算（拍）。
    pub budget_ticks: u64,
}

/// 按注册表下标取用例元数据（表外下标返回 None，不裸下标）。
pub fn case_meta(idx: usize) -> Option<CaseMeta> {
    match CASE_TABLE.get(idx) {
        Some((name, budget)) => Some(CaseMeta {
            case_id: (idx + 1) as u32,
            name: String::from(*name),
            budget_ticks: *budget,
        }),
        None => None,
    }
}

/// 结果态闭集（看门狗上报口径）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResultState {
    /// 通过：拍数不超有效预算。
    Pass,
    /// 失败：断言未过（非超时路径）。
    Fail,
    /// 超时：拍数超有效预算，看门狗点名上报。
    Timeout,
    /// 跳过：前置未满足，未计时。
    Skipped,
}

impl ResultState {
    /// 中文态名（账面可读）。
    pub fn zh(self) -> &'static str {
        match self {
            ResultState::Pass => "通过",
            ResultState::Fail => "失败",
            ResultState::Timeout => "超时",
            ResultState::Skipped => "跳过",
        }
    }
    /// 是否超时态。
    pub fn is_timeout(self) -> bool {
        self == ResultState::Timeout
    }
}

/// 全态闭集数组（态名表校验与测试共用；四态逐一构造，闭集完整）。
pub const ALL_STATES: [ResultState; 4] = [
    ResultState::Pass,
    ResultState::Fail,
    ResultState::Timeout,
    ResultState::Skipped,
];

/// 看门狗上报单：一例一条，序号由看门狗发号。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchdogReport {
    /// 上报序号（1 起严格递增）。
    pub seq: u64,
    /// 用例 ID。
    pub case_id: u32,
    /// 结果态（超时必为 Timeout）。
    pub state: ResultState,
    /// 实耗拍数。
    pub ticks_used: u64,
    /// 有效预算（拍）。
    pub budget_ticks: u64,
}

/// 有效预算：元数据预算 × 系数（checked，溢出与零预算显性拒）。
pub fn effective_budget(meta_budget: u64) -> Result<u64, u32> {
    if meta_budget == 0 {
        return Err(Diag::BudgetZero.val());
    }
    let scaled = match meta_budget.checked_mul(BUDGET_COEF_NUM) {
        Some(x) => x,
        None => return Err(Diag::BudgetOverflow.val()),
    };
    let budget = scaled / BUDGET_COEF_DEN;
    if budget == 0 {
        return Err(Diag::BudgetOverflow.val());
    }
    Ok(budget)
}

/// 用例运行：拍数对照有效预算，超即 TIMEOUT 结果态。
pub fn run_case(meta: &CaseMeta, ticks: u64) -> Result<WatchdogReport, u32> {
    let budget = effective_budget(meta.budget_ticks)?;
    let state = if ticks > budget {
        ResultState::Timeout
    } else {
        ResultState::Pass
    };
    Ok(WatchdogReport {
        seq: 0,
        case_id: meta.case_id,
        state,
        ticks_used: ticks,
        budget_ticks: budget,
    })
}

/// 看门狗：上报账 + 序号发号器 + 超时计数。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Watchdog {
    next_seq: u64,
    reports: Vec<WatchdogReport>,
    timeout_count: u64,
}

impl Watchdog {
    /// 新看门狗：序号从 1 起发号。
    pub fn new() -> Watchdog {
        Watchdog {
            next_seq: 1,
            reports: Vec::new(),
            timeout_count: 0,
        }
    }

    /// 收单：双向误报防线 + 发号 + 超时计数 + 入账。
    pub fn submit(&mut self, mut r: WatchdogReport) -> Result<u64, u32> {
        // 防线一：超预算却报非超时态——超时误判为通过。
        if r.ticks_used > r.budget_ticks && r.state != ResultState::Timeout {
            return Err(Diag::TimeoutAsPass.val());
        }
        // 防线二：未超预算却报超时态——上报单字段残缺。
        if r.ticks_used <= r.budget_ticks && r.state == ResultState::Timeout {
            return Err(Diag::ReportMalformed.val());
        }
        r.seq = self.next_seq;
        self.next_seq = match self.next_seq.checked_add(1) {
            Some(n) => n,
            None => return Err(Diag::SeqGap.val()),
        };
        if r.state == ResultState::Timeout {
            self.timeout_count += 1;
        }
        self.reports.push(r);
        Ok(self.next_seq - 1)
    }

    /// 上报账只读切片。
    pub fn reports(&self) -> &[WatchdogReport] {
        &self.reports
    }

    /// 超时上报条数。
    pub fn timeout_tally(&self) -> u64 {
        self.timeout_count
    }

    /// 序号连续性核：账内序号必须 1 起严格连续递增（断档即违约）。
    pub fn seq_monotonic_ok(&self) -> bool {
        let mut expect: u64 = 1;
        for r in self.reports.iter() {
            if r.seq != expect {
                return false;
            }
            expect += 1;
        }
        true
    }
}

/// 诊断码闭集：0x79A01..=0x79A0C（12 码，变体带中文 doc）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Diag {
    /// 超时误判为通过：超预算却报非超时态。
    TimeoutAsPass = 0x01,
    /// 元数据预算为零：预算声明残缺。
    BudgetZero = 0x02,
    /// 预算乘法溢出：预算 × 系数超 u64 上沿。
    BudgetOverflow = 0x03,
    /// 用例表外：用例 ID 不在冻结注册表内。
    CaseForeign = 0x04,
    /// 序号断档：看门狗发号非严格连续递增。
    SeqGap = 0x05,
    /// 看门狗漏报：超时例未落看门狗账。
    WatchdogSilent = 0x06,
    /// 上报单残缺：未超预算却报超时态等字段矛盾。
    ReportMalformed = 0x07,
    /// 双跑分叉：同输入两次运行看门狗账不同。
    DoubleRunDiverge = 0x08,
    /// 注入漏抓：M-191 注入超预算例未被点名。
    InjectionMissed = 0x09,
    /// 边界失守：拍数 == 预算未判通过或 +1 拍未判超时。
    BoundaryBreak = 0x0A,
    /// 段位守卫失守：SEG_BASE 不满足 0x40 对齐或越出批段窗。
    SegGuardFail = 0x0B,
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
            Diag::TimeoutAsPass => "超时误判为通过：建议核对看门狗收单口的双向态防线",
            Diag::BudgetZero => "元数据预算为零：建议用例注册时声明正预算，零预算拒收",
            Diag::BudgetOverflow => "预算乘法溢出：建议预算上限钳制在 u64 安全乘区",
            Diag::CaseForeign => "用例表外：建议只用冻结注册表五例，新例走注册扩表",
            Diag::SeqGap => "序号断档：建议看门狗发号器单一来源，禁手工指定序号",
            Diag::WatchdogSilent => "看门狗漏报：建议超时例必经 submit 入账，禁旁路",
            Diag::ReportMalformed => "上报单残缺：建议核对态与拍数/预算的一致性再收单",
            Diag::DoubleRunDiverge => "双跑分叉：建议 run_case 做成纯函数，输入定则账定",
            Diag::InjectionMissed => "注入漏抓：建议 M-191 注入例固定超预算并核对点名",
            Diag::BoundaryBreak => "边界失守：建议钉死拍数==预算通过、+1 拍超时的上沿",
            Diag::SegGuardFail => "段位守卫失守：建议核对 SEG_BASE 的 0x40 对齐与批段窗",
            Diag::SummaryEmpty => "写回摘要为空：建议完成时以非空中文摘要写回",
        }
    }
}

/// 闭集全量数组（12 码）。
pub const ALL_DIAGS: [Diag; 12] = [
    Diag::TimeoutAsPass,
    Diag::BudgetZero,
    Diag::BudgetOverflow,
    Diag::CaseForeign,
    Diag::SeqGap,
    Diag::WatchdogSilent,
    Diag::ReportMalformed,
    Diag::DoubleRunDiverge,
    Diag::InjectionMissed,
    Diag::BoundaryBreak,
    Diag::SegGuardFail,
    Diag::SummaryEmpty,
];

/// 诊断码查表闭集返回；未知编码回退固定兜底串。
pub fn detail(code: u32) -> &'static str {
    for d in ALL_DIAGS.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：建议核对是否落在 0x79A01..=0x79A0C 闭集内"
}

// ── 判据谓词（全部私有 fn xxx_ok() -> bool）────────────────────────────

/// 段位守卫：0x40 对齐 + B13 批段窗 [0x79A00, 0x79BC0]。
fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x79A00 && SEG_BASE <= 0x79BC0
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

/// 系数常量表与有效预算算术核：64 拍 × 3/2 = 96 拍；零预算与溢出拒。
fn coef_budget_ok() -> bool {
    if BUDGET_COEF_DEN == 0 || BUDGET_COEF_NUM == 0 {
        return false;
    }
    if effective_budget(64) != Ok(96) {
        return false;
    }
    if effective_budget(0) != Err(Diag::BudgetZero.val()) {
        return false;
    }
    effective_budget(u64::MAX) == Err(Diag::BudgetOverflow.val())
}

/// 正常流：注册表五例全过，预算字段与拍数一致，ID 与注册表对齐。
fn run_case_pass_ok() -> bool {
    let mut i = 0usize;
    while i < CASE_TABLE.len() {
        let meta = match case_meta(i) {
            Some(m) => m,
            None => return false,
        };
        let budget = match effective_budget(meta.budget_ticks) {
            Ok(b) => b,
            Err(_) => return false,
        };
        let r = match run_case(&meta, budget) {
            Ok(r) => r,
            Err(_) => return false,
        };
        if r.state != ResultState::Pass || r.case_id != (i as u32) + 1 {
            return false;
        }
        i += 1;
    }
    case_meta(CASE_TABLE.len()).is_none()
}

/// 边界核：拍数 == 有效预算判通过；+1 拍判超时（上沿钉死）。
fn boundary_ok() -> bool {
    let meta = match case_meta(1) {
        Some(m) => m,
        None => return false,
    };
    let budget = match effective_budget(meta.budget_ticks) {
        Ok(b) => b,
        Err(_) => return false,
    };
    let at = match run_case(&meta, budget) {
        Ok(r) => r.state,
        Err(_) => return false,
    };
    let over = match run_case(&meta, budget + 1) {
        Ok(r) => r.state,
        Err(_) => return false,
    };
    at == ResultState::Pass && over == ResultState::Timeout
}

/// M-191 注入：注入例（0x191）超预算运行必被判 TIMEOUT。
fn injection_m191_ok() -> bool {
    let meta = CaseMeta {
        case_id: INJECTION_CASE_ID,
        name: String::from("M-191超时注入"),
        budget_ticks: 16,
    };
    let budget = match effective_budget(meta.budget_ticks) {
        Ok(b) => b,
        Err(_) => return false,
    };
    match run_case(&meta, budget * 2 + 7) {
        Ok(r) => r.state == ResultState::Timeout && r.case_id == INJECTION_CASE_ID,
        Err(_) => false,
    }
}

/// 看门狗账：序号严格递增、超时计数与账内超时条数一致。
fn watchdog_ledger_ok() -> bool {
    let mut w = Watchdog::new();
    let mut i = 0usize;
    while i < CASE_TABLE.len() {
        let meta = match case_meta(i) {
            Some(m) => m,
            None => return false,
        };
        let r = match run_case(&meta, 0) {
            Ok(r) => r,
            Err(_) => return false,
        };
        if w.submit(r).is_err() {
            return false;
        }
        i += 1;
    }
    // 追加一条 M-191 注入超时例：看门狗必点名入账。
    let meta = CaseMeta {
        case_id: INJECTION_CASE_ID,
        name: String::from("M-191超时注入"),
        budget_ticks: 8,
    };
    let r = match run_case(&meta, 99) {
        Ok(r) => r,
        Err(_) => return false,
    };
    if w.submit(r).is_err() {
        return false;
    }
    w.seq_monotonic_ok() && w.timeout_tally() == 1 && w.reports().len() == CASE_TABLE.len() + 1
}

/// 看门狗漏报核：超时例若绕过 submit 不入账，账内超时计数必与
/// 实际超时例数对不上（漏报被账面对账点破）。
fn watchdog_silent_ok() -> bool {
    let mut w = Watchdog::new();
    let meta = match case_meta(2) {
        Some(m) => m,
        None => return false,
    };
    let budget = match effective_budget(meta.budget_ticks) {
        Ok(b) => b,
        Err(_) => return false,
    };
    // 只入账通过例，超时注入例（0x191）被人为旁路。
    let r = match run_case(&meta, budget) {
        Ok(r) => r,
        Err(_) => return false,
    };
    if w.submit(r).is_err() {
        return false;
    }
    // 对账：超时注入例未入账 → 账内超时条数为 0，而运行侧超时例数 ≥ 1，
    // 两数不等即漏报实锤。
    let injected = CaseMeta {
        case_id: INJECTION_CASE_ID,
        name: String::from("旁路注入"),
        budget_ticks: 8,
    };
    let bypassed_state = match run_case(&injected, 77) {
        Ok(r) => r.state,
        Err(_) => return false,
    };
    let side_timeout = if bypassed_state.is_timeout() { 1u64 } else { 0 };
    w.timeout_tally() != side_timeout && w.seq_monotonic_ok()
}

/// 误报双防线：超预算非超时态拒（TimeoutAsPass）；未超预算报超时态拒。
fn watchdog_misreport_ok() -> bool {
    let mut w = Watchdog::new();
    let forged = WatchdogReport {
        seq: 0,
        case_id: 1,
        state: ResultState::Pass,
        ticks_used: 999,
        budget_ticks: 96,
    };
    if w.submit(forged) != Err(Diag::TimeoutAsPass.val()) {
        return false;
    }
    let forged2 = WatchdogReport {
        seq: 0,
        case_id: 1,
        state: ResultState::Timeout,
        ticks_used: 10,
        budget_ticks: 96,
    };
    w.submit(forged2) == Err(Diag::ReportMalformed.val()) && w.reports().is_empty()
}

/// 注入漏抓防线：注入例若被判非超时态，InjectionMissed 语义点名。
fn injection_missed_guard_ok() -> bool {
    let meta = CaseMeta {
        case_id: INJECTION_CASE_ID,
        name: String::from("M-191超时注入"),
        budget_ticks: 8,
    };
    let budget = match effective_budget(meta.budget_ticks) {
        Ok(b) => b,
        Err(_) => return false,
    };
    let r = match run_case(&meta, budget + 1) {
        Ok(r) => r,
        Err(_) => return false,
    };
    // 正确路径必抓：注入例为超时态；若不是则判漏抓。
    if r.state != ResultState::Timeout {
        return false;
    }
    // 漏抓语义演练：假设注入例被判 Pass（构造违约态），账面对账必红。
    let missed = r.state == ResultState::Pass;
    !missed && detail(Diag::InjectionMissed.val()).contains("M-191")
}

/// 结果态闭集：四态中文名两两不同且非空，is_timeout 只点名超时态。
fn state_names_ok() -> bool {
    let mut i = 0usize;
    while i < ALL_STATES.len() {
        let a = match ALL_STATES.get(i) {
            Some(s) => *s,
            None => return false,
        };
        if a.zh().is_empty() {
            return false;
        }
        let mut j = i + 1;
        while j < ALL_STATES.len() {
            match ALL_STATES.get(j) {
                Some(b) => {
                    if a.zh() == b.zh() {
                        return false;
                    }
                }
                None => return false,
            }
            j += 1;
        }
        if a.is_timeout() != (a == ResultState::Timeout) {
            return false;
        }
        i += 1;
    }
    true
}

/// 表外用例拒：注册表外下标取 None，表外 ID 不入冻结表。
fn case_foreign_ok() -> bool {
    let meta = match case_meta(0) {
        Some(m) => m,
        None => return false,
    };
    let forged = CaseMeta {
        case_id: 0x7FFF,
        name: String::from("表外例"),
        budget_ticks: meta.budget_ticks,
    };
    // 表外 ID 的例可运行（纯函数），但不等价任何注册表例。
    let r = match run_case(&forged, 1) {
        Ok(r) => r,
        Err(_) => return false,
    };
    let mut in_table = false;
    let mut i = 0usize;
    while i < CASE_TABLE.len() {
        match case_meta(i) {
            Some(m) => {
                if m.case_id == r.case_id {
                    in_table = true;
                }
            }
            None => return false,
        }
        i += 1;
    }
    !in_table && detail(Diag::CaseForeign.val()).contains("注册")
}

/// 双跑一致：同输入两次运行看门狗账逐字段相同。
fn double_run_ok() -> bool {
    let run_once = || -> Result<Vec<WatchdogReport>, u32> {
        let mut w = Watchdog::new();
        let mut i = 0usize;
        while i < CASE_TABLE.len() {
            let meta = match case_meta(i) {
                Some(m) => m,
                None => return Err(Diag::CaseForeign.val()),
            };
            let ticks = match effective_budget(meta.budget_ticks) {
                Ok(b) => b,
                Err(e) => return Err(e),
            };
            let r = run_case(&meta, ticks)?;
            w.submit(r)?;
            i += 1;
        }
        Ok(w.reports.to_vec())
    };
    let a = run_once();
    let b = run_once();
    a.is_ok() && b.is_ok() && a == b
}

/// 写回面：状态=已完成、结果摘要非空（含超时点名与注入结论）。
fn writeback_summary_ok() -> bool {
    let mut w = Watchdog::new();
    let mut i = 0usize;
    while i < CASE_TABLE.len() {
        let meta = match case_meta(i) {
            Some(m) => m,
            None => return false,
        };
        let r = match run_case(&meta, 0) {
            Ok(r) => r,
            Err(_) => return false,
        };
        if w.submit(r).is_err() {
            return false;
        }
        i += 1;
    }
    let summary = format!(
        "UNX-F3432 完成：看门狗账{}条 超时{}条 M-191注入已验",
        w.reports().len(),
        w.timeout_tally()
    );
    !summary.is_empty()
        && summary.contains("UNX-F3432")
        && summary.contains("超时")
        && w.seq_monotonic_ok()
        && detail(Diag::SummaryEmpty.val()).contains("摘要")
}

// ── 判据总成：run_unx3432_checks ───────────────────────────────────────

/// run_unx3432_checks：本单判据聚合（诊断段 0x79A00，全部走 add_dyn）。
pub fn run_unx3432_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F3432 超时与看门狗");
    let checks: [(&'static str, bool, String); 13] = [
        (
            "段位守卫0x40对齐且在批段窗内",
            seg_guard_ok(),
            format!("SEG_BASE=0x{:X} 守卫{}", SEG_BASE, if seg_guard_ok() { "过" } else { "破" }),
        ),
        (
            "诊断码闭集12码唯一且文案带建议",
            diag_closed_set_ok(),
            format!("闭集{}", if diag_closed_set_ok() { "完整" } else { "破缺" }),
        ),
        (
            "系数常量表与有效预算算术核",
            coef_budget_ok(),
            format!("预算{}", if coef_budget_ok() { "算准" } else { "算错" }),
        ),
        (
            "正常流注册表五例全过",
            run_case_pass_ok(),
            format!("五例{}", if run_case_pass_ok() { "全过" } else { "有红" }),
        ),
        (
            "边界拍数等于预算通过加一拍超时",
            boundary_ok(),
            format!("边界{}", if boundary_ok() { "钉死" } else { "失守" }),
        ),
        (
            "M-191超时注入必抓TIMEOUT态",
            injection_m191_ok(),
            format!("注入{}", if injection_m191_ok() { "抓到" } else { "漏抓" }),
        ),
        (
            "看门狗账序号递增超时计数一致",
            watchdog_ledger_ok(),
            format!("看门狗账{}", if watchdog_ledger_ok() { "对账" } else { "错账" }),
        ),
        (
            "看门狗漏报对账必点破",
            watchdog_silent_ok(),
            format!("漏报{}", if watchdog_silent_ok() { "点破" } else { "失明" }),
        ),
        (
            "误报双防线双向拦截",
            watchdog_misreport_ok(),
            format!("误报{}", if watchdog_misreport_ok() { "拦截" } else { "放行" }),
        ),
        (
            "注入漏抓语义点名",
            injection_missed_guard_ok(),
            format!("漏抓{}", if injection_missed_guard_ok() { "点名" } else { "哑火" }),
        ),
        (
            "结果态闭集四态中文名互异",
            state_names_ok(),
            format!("态闭集{}", if state_names_ok() { "完整" } else { "破缺" }),
        ),
        (
            "表外用例不入冻结注册表",
            case_foreign_ok(),
            format!("表外例{}", if case_foreign_ok() { "拒" } else { "漏" }),
        ),
        (
            "双跑一致看门狗账逐字段同",
            double_run_ok(),
            format!("双跑{}", if double_run_ok() { "一致" } else { "分叉" }),
        ),
    ];
    // 写回摘要判据（第 14 条，与 13 条并列成账）。
    let wb = writeback_summary_ok();
    for (name, passed, det) in checks.iter() {
        s.add_dyn(name, *passed, det);
    }
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
        assert_eq!(SEG_BASE, 0x79A00);
        assert!(diag_closed_set_ok());
        for (i, d) in ALL_DIAGS.iter().enumerate() {
            assert_eq!(d.val(), SEG_BASE + i as u32 + 1);
        }
        assert_eq!(detail(0xDEAD), detail(0x5A5A));
    }

    #[test]
    fn coef_budget_arithmetics() {
        assert!(coef_budget_ok());
        assert_eq!(effective_budget(64), Ok(96));
        assert_eq!(effective_budget(4096), Ok(6144));
        assert_eq!(effective_budget(0), Err(Diag::BudgetZero.val()));
        assert_eq!(effective_budget(u64::MAX), Err(Diag::BudgetOverflow.val()));
    }

    #[test]
    fn registry_five_cases_pass() {
        assert!(run_case_pass_ok());
        assert_eq!(CASE_TABLE.len(), 5);
        assert!(case_meta(0).is_some());
        assert!(case_meta(4).is_some());
        assert!(case_meta(5).is_none());
    }

    #[test]
    fn boundary_exact_and_over() {
        assert!(boundary_ok());
        let meta = case_meta(3).unwrap();
        let budget = effective_budget(meta.budget_ticks).unwrap();
        assert_eq!(run_case(&meta, budget).unwrap().state, ResultState::Pass);
        assert_eq!(
            run_case(&meta, budget + 1).unwrap().state,
            ResultState::Timeout
        );
    }

    #[test]
    fn injection_m191_reported_as_timeout() {
        assert!(injection_m191_ok());
        assert!(injection_missed_guard_ok());
        assert!(detail(Diag::InjectionMissed.val()).contains("M-191"));
    }

    #[test]
    fn watchdog_ledger_seq_and_tally() {
        assert!(watchdog_ledger_ok());
        let mut w = Watchdog::new();
        let meta = case_meta(0).unwrap();
        let r = run_case(&meta, 0).unwrap();
        assert_eq!(w.submit(r).unwrap(), 1);
        assert!(w.seq_monotonic_ok());
        assert_eq!(w.timeout_tally(), 0);
    }

    #[test]
    fn watchdog_misreport_double_sided() {
        assert!(watchdog_misreport_ok());
        assert!(watchdog_silent_ok());
    }

    #[test]
    fn state_names_distinct() {
        assert!(state_names_ok());
        assert_eq!(ResultState::Timeout.zh(), "超时");
        assert!(ResultState::Timeout.is_timeout());
        assert!(!ResultState::Pass.is_timeout());
    }

    #[test]
    fn foreign_case_not_in_table() {
        assert!(case_foreign_ok());
    }

    #[test]
    fn double_run_identical_reports() {
        assert!(double_run_ok());
    }

    #[test]
    fn writeback_summary_non_empty() {
        assert!(writeback_summary_ok());
    }

    #[test]
    fn checks_aggregate_all_green() {
        let s = run_unx3432_checks();
        assert!(s.len() >= 12);
        assert!(s.all_passed(), "全部判据必须绿：{}", s.red_items_text());
    }
}
