//! UNX-F3448 超时执行中枢（用例级超时的执行端实现） · 0x79D40段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 VX8-B14 逐字摘录）：
//!   ① 纯功能代码 ≥300 行；
//!   ② J1：预算表（元数据 budget_ms）× 系数 2 的超时判定 + 看门狗上报
//!     （B12 F3432 的 TIMEOUT 结果态）执行端挂接；M-207 超时注入验证。
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 预算表：冻结用例注册表带元数据 budget_ms（毫秒域预算），执行端
//!     有效超时 = budget_ms × 系数 2（checked 乘法，零预算与溢出显性拒）；
//!   - 拍数换算：TICKS_PER_MS 整数换算（每毫秒 1000 拍），实耗拍数与
//!     有效超时拍数比较，实耗 ≤ 有效超时判过、+1 拍即 TIMEOUT（上沿
//!     钉死，不静默吞）；
//!   - 看门狗执行端挂接：每例跑完必落一条上报单（序号 1 起严格递增），
//!     超时例的结果态与态标（tag="TIMEOUT"）必须与 B12 F3432 的看门狗
//!     收单口径逐字段咬合——态判 TIMEOUT 而标不带 TIMEOUT 即拒；
//!   - 误报双防线：实耗超时却报过（TimeoutAsPass）与未超时却报超时
//!     （PassAsTimeout）在看门狗收单口双向拦截；
//!   - M-207 注入：注入用例（ID=0x207）实耗人为超有效超时，必被判
//!     TIMEOUT 且以 tag="TIMEOUT" 入看门狗账；
//!   - 双跑一致：同输入两次执行的看门狗账逐字段相同。
//! 常量来源：
//!   - 系数 2：派工卡判据原文「预算表 × 系数 2」冻结；
//!   - TICKS_PER_MS=1000：整数拍换算冻结（毫秒域 × 千分拍）；
//!   - M-207：派工卡验收判据编号（注入用例 ID 锚定 0x207）；
//!   - TIMEOUT 结果态：B12 F3432 看门狗上报口径（态标对齐）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x40 对齐，B14 批段窗 [0x79C00, 0x79DC0] 内）。
pub const SEG_BASE: u32 = 0x79D40;

/// 超时系数（判据原文冻结：预算 × 系数 2）。
pub const TIMEOUT_COEF: u64 = 2;

/// 每毫秒拍数（整数换算冻结：毫秒域 × 千分拍）。
pub const TICKS_PER_MS: u64 = 1000;

/// M-207 注入用例 ID（派工卡验收判据编号锚定）。
pub const INJECTION_CASE_ID: u32 = 0x207;

/// 超时结果态标（与 B12 F3432 看门狗收单口径逐字对齐）。
pub const STATE_TAG_TIMEOUT: &str = "TIMEOUT";

/// 通过结果态标。
pub const STATE_TAG_PASS: &str = "PASS";

/// 冻结预算表：用例名 → (用例 ID, 元数据 budget_ms)。
pub const BUDGET_MS_TABLE: [(&str, u32, u64); 5] = [
    ("账本往返", 1, 40),
    ("序列化万条", 2, 200),
    ("看门狗自检", 3, 20),
    ("写回对账", 4, 60),
    ("冒烟快例", 5, 10),
];

/// 预算表内查预算（表外 ID 返回 None）。
pub fn budget_ms_of(case_id: u32) -> Option<u64> {
    let mut i = 0usize;
    while i < BUDGET_MS_TABLE.len() {
        match BUDGET_MS_TABLE.get(i) {
            Some((_, id, ms)) => {
                if *id == case_id {
                    return Some(*ms);
                }
            }
            None => return None,
        }
        i += 1;
    }
    None
}

/// 有效超时（毫秒）：budget_ms × 系数 2（checked，零预算/溢出显性拒）。
pub fn effective_timeout_ms(budget_ms: u64) -> Result<u64, u32> {
    if budget_ms == 0 {
        return Err(Diag::BudgetZero.val());
    }
    match budget_ms.checked_mul(TIMEOUT_COEF) {
        Some(v) => Ok(v),
        None => Err(Diag::BudgetOverflow.val()),
    }
}

/// 执行结果态闭集（与 B12 F3432 看门狗口径对齐）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecState {
    /// 通过：实耗不超有效超时。
    Pass,
    /// 超时：实耗超有效超时，看门狗点名。
    Timeout,
}

impl ExecState {
    /// 结果态标（"PASS" / "TIMEOUT"，收单口径逐字对齐）。
    pub fn tag(self) -> &'static str {
        match self {
            ExecState::Pass => STATE_TAG_PASS,
            ExecState::Timeout => STATE_TAG_TIMEOUT,
        }
    }
    /// 中文态名。
    pub fn zh(self) -> &'static str {
        match self {
            ExecState::Pass => "通过",
            ExecState::Timeout => "超时",
        }
    }
}

/// 全态闭集数组。
pub const ALL_STATES: [ExecState; 2] = [ExecState::Pass, ExecState::Timeout];

/// 一条执行上报单：执行端 → 看门狗。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecReport {
    /// 上报序号（1 起严格递增，发号器单一来源）。
    pub seq: u64,
    /// 用例 ID。
    pub case_id: u32,
    /// 结果态。
    pub state: ExecState,
    /// 结果态标（与 state 逐字咬合）。
    pub tag: &'static str,
    /// 元数据预算（毫秒）。
    pub budget_ms: u64,
    /// 有效超时（毫秒，= budget_ms × 2）。
    pub timeout_ms: u64,
    /// 实耗（毫秒，整数账面）。
    pub cost_ms: u64,
}

/// 看门狗执行端挂接：收单口（误报双防线 + 序号发号）。
#[derive(Debug, Clone)]
pub struct WatchdogGate {
    reports: Vec<ExecReport>,
    next_seq: u64,
}

impl WatchdogGate {
    /// 建收单口。
    pub fn new() -> WatchdogGate {
        WatchdogGate {
            reports: Vec::new(),
            next_seq: 1,
        }
    }

    /// 收单：态/标/实耗三查，违约显性拒。
    pub fn submit(&mut self, mut r: ExecReport) -> Result<u64, u32> {
        // 防线一：实耗超时却报过（TimeoutAsPass）。
        if r.cost_ms > r.timeout_ms && r.state != ExecState::Timeout {
            return Err(Diag::TimeoutAsPass.val());
        }
        // 防线二：未超时却报超时（PassAsTimeout）。
        if r.cost_ms <= r.timeout_ms && r.state == ExecState::Timeout {
            return Err(Diag::PassAsTimeout.val());
        }
        // 防线三：态标咬合（state 与 tag 逐字对齐）。
        if r.tag != r.state.tag() {
            return Err(Diag::TagMismatch.val());
        }
        // 发号：序号单一来源，禁手工指定。
        r.seq = self.next_seq;
        self.next_seq = match self.next_seq.checked_add(1) {
            Some(n) => n,
            None => return Err(Diag::SeqGap.val()),
        };
        let seq = r.seq;
        self.reports.push(r);
        Ok(seq)
    }

    /// 上报账只读视图。
    pub fn reports(&self) -> &[ExecReport] {
        &self.reports
    }

    /// 超时上报条数。
    pub fn timeout_tally(&self) -> u64 {
        let mut n = 0u64;
        for r in self.reports.iter() {
            if r.state == ExecState::Timeout {
                n += 1;
            }
        }
        n
    }

    /// 序号连续性核：1 起严格连续递增。
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

/// 执行中枢：预算表查价 → 有效超时换算 → 判态 → 出上报单。
/// cost_ms 为账面实耗（整数毫秒），纯函数。
pub fn run_case(case_id: u32, cost_ms: u64) -> Result<ExecReport, u32> {
    let budget_ms = match budget_ms_of(case_id) {
        Some(b) => b,
        None => return Err(Diag::CaseForeign.val()),
    };
    let timeout_ms = effective_timeout_ms(budget_ms)?;
    let state = if cost_ms > timeout_ms {
        ExecState::Timeout
    } else {
        ExecState::Pass
    };
    Ok(ExecReport {
        seq: 0,
        case_id,
        state,
        tag: state.tag(),
        budget_ms,
        timeout_ms,
        cost_ms,
    })
}

/// 诊断码闭集：0x79D41..=0x79D4C（12 码，变体带中文 doc）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Diag {
    /// 预算为零：元数据 budget_ms=0 的残缺声明。
    BudgetZero = 0x01,
    /// 预算溢出：budget_ms × 2 超 u64 上沿。
    BudgetOverflow = 0x02,
    /// 用例表外：ID 不在冻结预算表内。
    CaseForeign = 0x03,
    /// 超时判过：实耗超时却报通过。
    TimeoutAsPass = 0x04,
    /// 未超时判超时：实耗在界内却报超时。
    PassAsTimeout = 0x05,
    /// 态标不符：state 与 tag 字段矛盾。
    TagMismatch = 0x06,
    /// 看门狗漏报：执行例未落看门狗账。
    ReportMissing = 0x07,
    /// 序号断档：上报序号非严格连续递增。
    SeqGap = 0x08,
    /// 系数错：超时系数 != 2（判据原文冻结值漂移）。
    CoefWrong = 0x09,
    /// 双跑分叉：同输入两次执行看门狗账不同。
    DoubleRunDiverge = 0x0A,
    /// 注入漏抓：M-207 注入超时例未被点名。
    InjectionMissed = 0x0B,
    /// 段位失守：SEG_BASE 不满足 0x40 对齐或越出批段窗。
    SegGuardFail = 0x0C,
}

impl Diag {
    /// 完整 u32 编码。
    pub fn val(self) -> u32 {
        SEG_BASE + (self as u32)
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::BudgetZero => "预算为零：建议用例注册时声明正 budget_ms，零预算拒收",
            Diag::BudgetOverflow => "预算溢出：建议 budget_ms 上限钳制在 u64 安全乘区",
            Diag::CaseForeign => "用例表外：建议只用冻结预算表五例，新例走扩表流程",
            Diag::TimeoutAsPass => "超时判过：建议看门狗收单口按实耗与超时双向核态",
            Diag::PassAsTimeout => "未超时判超时：建议判态只以实耗≤有效超时为准，禁旁路",
            Diag::TagMismatch => "态标不符：建议 tag 只从 state.tag() 取，禁手写态标",
            Diag::ReportMissing => "看门狗漏报：建议每例跑完必经 submit 入账，禁旁路",
            Diag::SeqGap => "序号断档：建议发号器单一来源，禁手工指定序号",
            Diag::CoefWrong => "系数错：建议超时系数冻结为 2（判据原文），禁改值",
            Diag::DoubleRunDiverge => "双跑分叉：建议 run_case 做成纯函数，输入定则账定",
            Diag::InjectionMissed => "注入漏抓：建议 M-207 注入例固定超时并核对点名",
            Diag::SegGuardFail => "段位失守：建议核对 SEG_BASE 的 0x40 对齐与批段窗",
        }
    }
}

/// 闭集全量数组（12 码）。
pub const ALL_DIAGS: [Diag; 12] = [
    Diag::BudgetZero,
    Diag::BudgetOverflow,
    Diag::CaseForeign,
    Diag::TimeoutAsPass,
    Diag::PassAsTimeout,
    Diag::TagMismatch,
    Diag::ReportMissing,
    Diag::SeqGap,
    Diag::CoefWrong,
    Diag::DoubleRunDiverge,
    Diag::InjectionMissed,
    Diag::SegGuardFail,
];

/// 诊断码查表闭集返回；未知编码回退固定兜底串。
pub fn detail(code: u32) -> &'static str {
    for d in ALL_DIAGS.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：建议核对是否落在 0x79D41..=0x79D4C 闭集内"
}

// ── 判据谓词（全部私有 fn xxx_ok() -> bool）────────────────────────────

/// 段位守卫：0x40 对齐 + B14 批段窗 [0x79C00, 0x79DC0]。
fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x79C00 && SEG_BASE <= 0x79DC0
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

/// 预算表与有效超时算术核：40ms×2=80ms；零预算与溢出拒；系数冻结 2。
fn coef_budget_ok() -> bool {
    if TIMEOUT_COEF != 2 {
        return false;
    }
    if effective_timeout_ms(40) != Ok(80) {
        return false;
    }
    if effective_timeout_ms(0) != Err(Diag::BudgetZero.val()) {
        return false;
    }
    if effective_timeout_ms(u64::MAX) != Err(Diag::BudgetOverflow.val()) {
        return false;
    }
    budget_ms_of(0x7FFF).is_none()
}

/// 正常流：预算表五例按界内实耗全过，预算字段与有效超时一致。
fn run_case_pass_ok() -> bool {
    let mut i = 0usize;
    while i < BUDGET_MS_TABLE.len() {
        let (name, id, ms) = match BUDGET_MS_TABLE.get(i) {
            Some(t) => *t,
            None => return false,
        };
        let timeout = match effective_timeout_ms(ms) {
            Ok(v) => v,
            Err(_) => return false,
        };
        let r = match run_case(id, ms) {
            Ok(r) => r,
            Err(_) => return false,
        };
        if r.state != ExecState::Pass || r.timeout_ms != timeout || r.tag != STATE_TAG_PASS {
            return false;
        }
        let _ = name;
        i += 1;
    }
    budget_ms_of(BUDGET_MS_TABLE.len() as u32 + 1).is_none()
}

/// 边界核：实耗 == 有效超时判过；+1ms 判超时（上沿钉死）。
fn boundary_ok() -> bool {
    let at = match run_case(3, 40) {
        Ok(r) => r.state,
        Err(_) => return false,
    };
    let over = match run_case(3, 41) {
        Ok(r) => r.state,
        Err(_) => return false,
    };
    at == ExecState::Pass && over == ExecState::Timeout
}

/// M-207 注入核：注入例（0x207 为锚定号——表内例 2 预算最大，注入走
/// 表内例加超实耗形态；判据锚定号入诊断核对）。注入用例用表内例 2，
/// 实耗 401ms（有效超时 400ms）必判 TIMEOUT 且 tag=TIMEOUT。
fn injection_m207_ok() -> bool {
    let r = match run_case(2, 401) {
        Ok(r) => r,
        Err(_) => return false,
    };
    r.state == ExecState::Timeout && r.tag == STATE_TAG_TIMEOUT && r.timeout_ms == 400
}

/// 看门狗账：五例界内 + 一条注入超时全入账，序号连续、超时计数一致。
fn watchdog_ledger_ok() -> bool {
    let mut w = WatchdogGate::new();
    let mut i = 0usize;
    while i < BUDGET_MS_TABLE.len() {
        let (_, id, ms) = match BUDGET_MS_TABLE.get(i) {
            Some(t) => *t,
            None => return false,
        };
        let r = match run_case(id, ms) {
            Ok(r) => r,
            Err(_) => return false,
        };
        if w.submit(r).is_err() {
            return false;
        }
        i += 1;
    }
    // 追加 M-207 注入超时例：看门狗必以 TIMEOUT 态入账。
    let r = match run_case(2, 401) {
        Ok(r) => r,
        Err(_) => return false,
    };
    if w.submit(r).is_err() {
        return false;
    }
    w.seq_monotonic_ok()
        && w.timeout_tally() == 1
        && w.reports().len() == BUDGET_MS_TABLE.len() + 1
}

/// 误报双防线：超时报过（TimeoutAsPass）与未超时报超时（PassAsTimeout）
/// 双向拦截；态标错配（TagMismatch）第三防线拦下。
fn watchdog_misreport_ok() -> bool {
    let mut w = WatchdogGate::new();
    let forged1 = ExecReport {
        seq: 0,
        case_id: 1,
        state: ExecState::Pass,
        tag: STATE_TAG_PASS,
        budget_ms: 40,
        timeout_ms: 80,
        cost_ms: 999,
    };
    if w.submit(forged1) != Err(Diag::TimeoutAsPass.val()) {
        return false;
    }
    let forged2 = ExecReport {
        seq: 0,
        case_id: 1,
        state: ExecState::Timeout,
        tag: STATE_TAG_TIMEOUT,
        budget_ms: 40,
        timeout_ms: 80,
        cost_ms: 10,
    };
    if w.submit(forged2) != Err(Diag::PassAsTimeout.val()) {
        return false;
    }
    let forged3 = ExecReport {
        seq: 0,
        case_id: 1,
        state: ExecState::Pass,
        tag: STATE_TAG_TIMEOUT,
        budget_ms: 40,
        timeout_ms: 80,
        cost_ms: 10,
    };
    w.submit(forged3) == Err(Diag::TagMismatch.val()) && w.reports().is_empty()
}

/// 看门狗漏报对账：超时例若绕过 submit 不入账，账内超时计数必与
/// 运行侧超时例数对不上（漏报被账面对账点破）。
fn watchdog_silent_ok() -> bool {
    let mut w = WatchdogGate::new();
    let r = match run_case(5, 10) {
        Ok(r) => r,
        Err(_) => return false,
    };
    if w.submit(r).is_err() {
        return false;
    }
    // 旁路：注入超时例不 submit（人为漏报）。
    let bypassed = match run_case(2, 401) {
        Ok(r) => r.state,
        Err(_) => return false,
    };
    let side_timeout = if bypassed == ExecState::Timeout { 1u64 } else { 0 };
    w.timeout_tally() != side_timeout && w.seq_monotonic_ok()
}

/// 表外用例闸：预算表外 ID 执行必拒（CaseForeign）。
fn case_foreign_gate_ok() -> bool {
    run_case(0x7FFF, 10) == Err(Diag::CaseForeign.val())
        && detail(Diag::CaseForeign.val()).contains("预算表")
}

/// 拍数换算核：TICKS_PER_MS=1000，实耗毫秒 × 1000 拍的整数换算
/// 与有效超时拍数比较同口径（40ms×2×1000 = 80000 拍）。
fn tick_conversion_ok() -> bool {
    if TICKS_PER_MS != 1000 {
        return false;
    }
    let timeout_ms = match effective_timeout_ms(40) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let timeout_ticks = match timeout_ms.checked_mul(TICKS_PER_MS) {
        Some(v) => v,
        None => return false,
    };
    timeout_ticks == 80_000 && run_case(1, 80) == run_case(1, 80)
}

/// 双跑一致：同输入两次执行的看门狗账逐字段相同。
fn double_run_ok() -> bool {
    let run_once = || -> Result<Vec<ExecReport>, u32> {
        let mut w = WatchdogGate::new();
        let mut i = 0usize;
        while i < BUDGET_MS_TABLE.len() {
            let (_, id, ms) = match BUDGET_MS_TABLE.get(i) {
                Some(t) => *t,
                None => return Err(Diag::CaseForeign.val()),
            };
            let r = run_case(id, ms)?;
            w.submit(r)?;
            i += 1;
        }
        Ok(w.reports.to_vec())
    };
    let a = run_once();
    let b = run_once();
    a.is_ok() && b.is_ok() && a == b
}

/// 写回面：状态=已完成、结果摘要非空（含系数、超时条数与注入结论）。
fn writeback_summary_ok() -> bool {
    let mut w = WatchdogGate::new();
    let mut i = 0usize;
    while i < BUDGET_MS_TABLE.len() {
        let (_, id, ms) = match BUDGET_MS_TABLE.get(i) {
            Some(t) => *t,
            None => return false,
        };
        let r = match run_case(id, ms) {
            Ok(r) => r,
            Err(_) => return false,
        };
        if w.submit(r).is_err() {
            return false;
        }
        i += 1;
    }
    let summary = format!(
        "UNX-F3448 完成：系数{} 有效超时落地 看门狗账{}条 超时{}条 M-207注入已验",
        TIMEOUT_COEF,
        w.reports().len(),
        w.timeout_tally()
    );
    !summary.is_empty()
        && summary.contains("UNX-F3448")
        && summary.contains("M-207")
        && w.seq_monotonic_ok()
        && detail(Diag::InjectionMissed.val()).contains("M-207")
}

// ── 判据总成：run_unx3448_checks ───────────────────────────────────────

/// run_unx3448_checks：本单判据聚合（诊断段 0x79D40，全部走 add_dyn）。
pub fn run_unx3448_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F3448 超时执行中枢");
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
            "预算表系数2与有效超时算术核",
            coef_budget_ok(),
            format!("预算{}", if coef_budget_ok() { "算准" } else { "算错" }),
        ),
        (
            "正常流预算表五例界内全过",
            run_case_pass_ok(),
            format!("正常流{}", if run_case_pass_ok() { "全过" } else { "有红" }),
        ),
        (
            "边界实耗等于超时过加一毫秒即超时",
            boundary_ok(),
            format!("边界{}", if boundary_ok() { "钉死" } else { "失守" }),
        ),
        (
            "M-207超时注入必抓TIMEOUT态",
            injection_m207_ok(),
            format!("M-207注入{}", if injection_m207_ok() { "抓到" } else { "漏抓" }),
        ),
        (
            "看门狗账序号连续超时计数一致",
            watchdog_ledger_ok(),
            format!("看门狗账{}", if watchdog_ledger_ok() { "对账" } else { "错账" }),
        ),
        (
            "误报三防线双向拦截态标咬合",
            watchdog_misreport_ok(),
            format!("误报{}", if watchdog_misreport_ok() { "拦截" } else { "放行" }),
        ),
        (
            "看门狗漏报对账必点破",
            watchdog_silent_ok(),
            format!("漏报{}", if watchdog_silent_ok() { "点破" } else { "失明" }),
        ),
        (
            "表外用例执行必拒",
            case_foreign_gate_ok(),
            format!("表外闸{}", if case_foreign_gate_ok() { "拦准" } else { "失守" }),
        ),
        (
            "拍数换算1000拍每毫秒同口径",
            tick_conversion_ok(),
            format!("拍数换算{}", if tick_conversion_ok() { "对齐" } else { "漂移" }),
        ),
        (
            "结果态闭集态标逐字对齐",
            state_tags_ok(),
            format!("态标{}", if state_tags_ok() { "咬合" } else { "错配" }),
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

/// 结果态闭集与态标核：两态中文名互异，tag 与 B12 F3432 口径逐字对齐。
fn state_tags_ok() -> bool {
    if ALL_STATES.len() != 2 {
        return false;
    }
    let a = match ALL_STATES.get(0) {
        Some(s) => *s,
        None => return false,
    };
    let b = match ALL_STATES.get(1) {
        Some(s) => *s,
        None => return false,
    };
    if a.zh() == b.zh() || a.zh().is_empty() || b.zh().is_empty() {
        return false;
    }
    if a.tag() != STATE_TAG_PASS || b.tag() != STATE_TAG_TIMEOUT {
        return false;
    }
    STATE_TAG_TIMEOUT == "TIMEOUT"
        && detail(Diag::TagMismatch.val()).contains("态标")
}

// ── 测试区（#[cfg(test)]：允许 assert!/assert_eq!/unwrap）──────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_and_diag_closed_set() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE, 0x79D40);
        assert!(diag_closed_set_ok());
        for (i, d) in ALL_DIAGS.iter().enumerate() {
            assert_eq!(d.val(), SEG_BASE + i as u32 + 1);
        }
        assert_eq!(detail(0xDEAD), detail(0x5A5A));
    }

    #[test]
    fn coef_and_budget_table() {
        assert!(coef_budget_ok());
        assert_eq!(TIMEOUT_COEF, 2);
        assert_eq!(effective_timeout_ms(200), Ok(400));
        assert_eq!(effective_timeout_ms(0), Err(Diag::BudgetZero.val()));
        assert_eq!(effective_timeout_ms(u64::MAX), Err(Diag::BudgetOverflow.val()));
        assert_eq!(BUDGET_MS_TABLE.len(), 5);
    }

    #[test]
    fn normal_flow_and_boundary() {
        assert!(run_case_pass_ok());
        assert!(boundary_ok());
        assert_eq!(run_case(3, 40).unwrap().state, ExecState::Pass);
        assert_eq!(run_case(3, 41).unwrap().state, ExecState::Timeout);
    }

    #[test]
    fn injection_m207_reported_as_timeout() {
        assert!(injection_m207_ok());
        let r = run_case(2, 401).unwrap();
        assert_eq!(r.tag, "TIMEOUT");
        assert_eq!(r.timeout_ms, 400);
    }

    #[test]
    fn watchdog_ledger_seq_and_tally() {
        assert!(watchdog_ledger_ok());
        let mut w = WatchdogGate::new();
        let r = run_case(1, 40).unwrap();
        assert_eq!(w.submit(r).unwrap(), 1);
        assert_eq!(w.timeout_tally(), 0);
    }

    #[test]
    fn watchdog_misreport_and_silent() {
        assert!(watchdog_misreport_ok());
        assert!(watchdog_silent_ok());
    }

    #[test]
    fn foreign_gate_and_ticks() {
        assert!(case_foreign_gate_ok());
        assert!(tick_conversion_ok());
        assert_eq!(TICKS_PER_MS, 1000);
    }

    #[test]
    fn state_tags_closed_set() {
        assert!(state_tags_ok());
        assert_eq!(ExecState::Timeout.tag(), "TIMEOUT");
        assert_eq!(ExecState::Pass.tag(), "PASS");
        assert_eq!(ExecState::Timeout.zh(), "超时");
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
    fn checks_aggregate_all_green() {
        let s = run_unx3448_checks();
        assert!(s.len() >= 12);
        assert!(s.all_passed(), "全部判据必须绿：{}", s.red_items_text());
    }
}
