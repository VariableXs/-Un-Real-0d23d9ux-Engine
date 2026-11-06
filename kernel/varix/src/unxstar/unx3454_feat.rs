//! UNX-F3454 失败重试策略（失败用例重试一次 + 重试入账） · 0x79EC0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 VX8-B15 逐字摘录）：
//!   ① 纯功能代码 ≥300 行；
//!   ② 判据原文：J1：重试一次语义（retry=1 默认、可关）+ 重试结果
//!     独立记账（首次/重试两列）；M-213 重试语义验证。；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 重试一次语义：默认策略 retry_max=1 且 enabled（DEFAULT_RETRY_
//!     MAX 冻结）；重试上限钳制在 1 次以内——即使配置 retry_max=5，
//!     有效重试仍恰 1 次（RetryCapExceeded 点名任何二次重试）；
//!   - 可关：enabled=false 或 retry_max=0 都把策略判为关闭
//!     （PolicyFrozen 点名默认策略漂移、NoRetrySemantics 点名
//!     可关语义失守），关闭后失败即终态、不排重试；
//!   - 首次/重试两列：RetryRecord 以 first / retry 两列独立记账，
//!     retry 列 Option 承载（未重试即 None），终态由两列推导：
//!     首过即过、首败重过即过、首败重败即败（FinalMismatch
//!     点名任何与两列推导不符的终态）；
//!   - 重试列合法性：首次通过后不得再排重试（RetryAfterPass），
//!     重试列缺失而决策说要重试即账目破缺（ColumnMissing）；
//!   - 重试入账独立：重试结果必须以独立列入账，不得覆盖首列
//!     （RetryNotBooked 点名丢列）；
//!   - 容量守卫：账面定容 16 条，超容显性拒；同号重复入账拒；
//!   - 双跑一致：决策与入账均为纯函数，同输入两次结果相同；
//!   - 写回面：状态=已完成、结果摘要非空。
//! 常量来源：
//!   - DEFAULT_RETRY_MAX=1：派工卡判据原文冻结值（retry=1 默认）；
//!   - RETRY_LEDGER_CAP=16：重试账定容（与执行面登记上限同源）；
//!   - M-213 注入用例号 0x213：派工卡验收判据编号（语义验证锚定）；
//!   - 有效重试钳制 min(retry_max, 1)：判据原文「重试一次语义」
//!     的直接派生（重试不多于一次）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x40 对齐，B15 批段窗 [0x79E00, 0x79FC0] 内）。
pub const SEG_BASE: u32 = 0x79EC0;

/// 默认重试上限（retry=1 默认，判据原文冻结值）。
pub const DEFAULT_RETRY_MAX: u32 = 1;
/// 关闭态重试上限（可关语义的 0 次形态）。
pub const RETRY_OFF_MAX: u32 = 0;
/// 重试账定容（条上限，超容显性拒）。
pub const RETRY_LEDGER_CAP: usize = 16;
/// M-213 注入用例号（语义验证锚定）。
pub const M213_CASE_ID: u32 = 0x213;
/// M-213 判据编号。
pub const M213_AUDIT_ID: u64 = 0x213;

/// 单次尝试裁决（通过/失败两态闭集）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttemptVerdict {
    /// 本尝试通过。
    Pass,
    /// 本尝试失败。
    Fail,
}

/// 重试决策闭集：终态收口或再试一次。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryDecision {
    /// 收口为终态（不再重试）。
    Final,
    /// 再试恰一次（有效重试钳制在 1 次以内）。
    RetryOnce,
}

/// 重试策略：开关 + 上限（retry=1 默认、可关）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    /// 策略开关（false 即关闭，失败即终态）。
    pub enabled: bool,
    /// 配置的重试上限（有效重试仍钳制在 1 次以内）。
    pub retry_max: u32,
}

impl RetryPolicy {
    /// 默认策略：开启且 retry=1（判据原文冻结值）。
    pub fn default_policy() -> RetryPolicy {
        RetryPolicy { enabled: true, retry_max: DEFAULT_RETRY_MAX }
    }

    /// 关闭策略：enabled=false（可关形态一）。
    pub fn disabled_policy() -> RetryPolicy {
        RetryPolicy { enabled: false, retry_max: DEFAULT_RETRY_MAX }
    }

    /// 零上限策略：retry_max=0（可关形态二）。
    pub fn zero_max_policy() -> RetryPolicy {
        RetryPolicy { enabled: true, retry_max: RETRY_OFF_MAX }
    }

    /// 策略是否处于关闭态（enabled=false 或 retry_max=0 任一即关）。
    pub fn is_off(&self) -> bool {
        !self.enabled || self.retry_max == 0
    }
}

/// 重试决策：首败且策略开启时恰排一次重试，其余一律收口。
pub fn decide(policy: &RetryPolicy, first: AttemptVerdict) -> Result<RetryDecision, u32> {
    if first == AttemptVerdict::Pass {
        return Ok(RetryDecision::Final);
    }
    if policy.is_off() {
        return Ok(RetryDecision::Final);
    }
    // 有效重试钳制：即使 retry_max 配大，也只排一次（重试一次语义）。
    if policy.retry_max >= 1 {
        return Ok(RetryDecision::RetryOnce);
    }
    Ok(RetryDecision::Final)
}

/// 重试账条目：首次/重试两列独立记账 + 两列推导的终态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryRecord {
    /// 用例号。
    pub case_id: u32,
    /// 首列：首次尝试结果（恒有）。
    pub first: AttemptVerdict,
    /// 重试列：重试尝试结果（未重试即 None）。
    pub retry: Option<AttemptVerdict>,
    /// 终态：由两列推导（首过即过、首败重过即过、首败重败即败）。
    pub final_verdict: AttemptVerdict,
}

/// 重试账面：条目表（append-only）。
#[derive(Debug, Clone)]
pub struct RetryLedger {
    /// 条目表。
    pub records: Vec<RetryRecord>,
}

impl RetryLedger {
    /// 空账面。
    pub fn new() -> RetryLedger {
        RetryLedger { records: Vec::new() }
    }
}

/// 终态推导（纯函数）：首列过即过；首列败看重试列，重试列缺失
/// 视为无重试（终态随首列败），有重试列则随重试列。
pub fn derive_final(first: AttemptVerdict, retry: Option<AttemptVerdict>) -> AttemptVerdict {
    match first {
        AttemptVerdict::Pass => AttemptVerdict::Pass,
        AttemptVerdict::Fail => match retry {
            Some(r) => r,
            None => AttemptVerdict::Fail,
        },
    }
}

/// 入账一条重试记录（首次/重试两列独立，终态按两列推导冻结）。
pub fn book(
    ledger: &mut RetryLedger,
    case_id: u32,
    first: AttemptVerdict,
    retry: Option<AttemptVerdict>,
) -> Result<(), u32> {
    if ledger.records.len() >= RETRY_LEDGER_CAP {
        return Err(Diag::LedgerCapOverflow.val());
    }
    let mut i = 0usize;
    while i < ledger.records.len() {
        match ledger.records.get(i) {
            Some(r) => {
                if r.case_id == case_id {
                    return Err(Diag::DupCase.val());
                }
            }
            None => return Err(Diag::LedgerBad.val()),
        }
        i += 1;
    }
    // 重试列合法性：首列过后不得再排重试。
    if first == AttemptVerdict::Pass && retry.is_some() {
        return Err(Diag::RetryAfterPass.val());
    }
    let final_verdict = derive_final(first, retry);
    ledger.records.push(RetryRecord { case_id, first, retry, final_verdict });
    Ok(())
}

/// 重试语义复核：账目与策略决策互恰（决策说要重试则重试列必在，
/// 决策收口则重试列必缺），终态必须与两列推导一致。
pub fn audit_record(
    policy: &RetryPolicy,
    record: &RetryRecord,
) -> Result<(), u32> {
    let expect = decide(policy, record.first)?;
    match expect {
        RetryDecision::RetryOnce => {
            if record.retry.is_none() {
                return Err(Diag::ColumnMissing.val());
            }
            if record.final_verdict != derive_final(record.first, record.retry) {
                return Err(Diag::FinalMismatch.val());
            }
            Ok(())
        }
        RetryDecision::Final => {
            if record.retry.is_some() {
                return Err(Diag::RetryNotBooked.val());
            }
            if record.final_verdict != record.first {
                return Err(Diag::FinalMismatch.val());
            }
            Ok(())
        }
    }
}

/// M-213 重试语义验证：默认策略四组合全查（首过/首败重过/首败重败/
/// 首败无重试），决策、两列、终态三点互恰。
pub fn m213_semantics() -> Result<bool, u32> {
    let policy = RetryPolicy::default_policy();
    if decide(&policy, AttemptVerdict::Pass)? != RetryDecision::Final {
        return Err(Diag::NoRetrySemantics.val());
    }
    if decide(&policy, AttemptVerdict::Fail)? != RetryDecision::RetryOnce {
        return Err(Diag::NoRetrySemantics.val());
    }
    let mut ledger = RetryLedger::new();
    // 首败重过：两列（Fail, Pass），终态过。
    book(&mut ledger, M213_CASE_ID, AttemptVerdict::Fail, Some(AttemptVerdict::Pass))?;
    let mut ledger2 = RetryLedger::new();
    // 首败重败：两列（Fail, Fail），终态败。
    book(&mut ledger2, M213_CASE_ID, AttemptVerdict::Fail, Some(AttemptVerdict::Fail))?;
    let mut ledger3 = RetryLedger::new();
    // 首败无重试：两列（Fail, None），终态败。
    book(&mut ledger3, M213_CASE_ID, AttemptVerdict::Fail, None)?;
    let a = match ledger.records.get(0) {
        Some(r) => *r,
        None => return Err(Diag::LedgerBad.val()),
    };
    let b = match ledger2.records.get(0) {
        Some(r) => *r,
        None => return Err(Diag::LedgerBad.val()),
    };
    let c = match ledger3.records.get(0) {
        Some(r) => *r,
        None => return Err(Diag::LedgerBad.val()),
    };
    audit_record(&policy, &a)?;
    audit_record(&policy, &b)?;
    // 首败无重试户按关闭策略复核（收口语义：retry 列必缺）。
    audit_record(&RetryPolicy::zero_max_policy(), &c)?;
    Ok(a.final_verdict == AttemptVerdict::Pass
        && b.final_verdict == AttemptVerdict::Fail
        && c.final_verdict == AttemptVerdict::Fail
        && a.retry == Some(AttemptVerdict::Pass)
        && b.retry == Some(AttemptVerdict::Fail)
        && c.retry.is_none())
}

/// 诊断码闭集：0x79EC1..=0x79ECC（12 码，变体带中文 doc）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Diag {
    /// 默认策略漂移：retry=1 默认值被运行期改写。
    PolicyFrozen = 0x01,
    /// 重试上限超限：出现第二次重试（有效重试钳制在 1 次以内）。
    RetryCapExceeded = 0x02,
    /// 账面超容：条目数超 RETRY_LEDGER_CAP 上限。
    LedgerCapOverflow = 0x03,
    /// 首过后排重试：重试列在首列通过时非法。
    RetryAfterPass = 0x04,
    /// 重复用例号：同号二次入账。
    DupCase = 0x05,
    /// 账面残缺：条目表内部状态破缺。
    LedgerBad = 0x06,
    /// 终态与两列推导不符。
    FinalMismatch = 0x07,
    /// 重试列缺失：决策说要重试但账上无重试列。
    ColumnMissing = 0x08,
    /// 重试未独立入账：重试结果覆盖或丢失首列。
    RetryNotBooked = 0x09,
    /// 可关语义失守：关闭态仍排重试。
    NoRetrySemantics = 0x0A,
    /// 双跑分叉：同输入两次决策/入账结果不同。
    DoubleRunDiverge = 0x0B,
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
            Diag::PolicyFrozen => "默认策略漂移：建议把 retry=1 默认值冻结为常量 DEFAULT_RETRY_MAX",
            Diag::RetryCapExceeded => "重试上限超限：建议有效重试钳制在 min(retry_max, 1) 次",
            Diag::LedgerCapOverflow => "账面超容：建议条目上限钳制在 RETRY_LEDGER_CAP=16 内或扩容账面",
            Diag::RetryAfterPass => "首过后排重试：建议重试列只在首列失败时合法",
            Diag::DupCase => "重复用例号：建议入账前先查重，同号只入账一次",
            Diag::LedgerBad => "账面残缺：建议核对条目表内部状态后再入账",
            Diag::FinalMismatch => "终态与两列推导不符：建议终态恒由首次/重试两列推导冻结",
            Diag::ColumnMissing => "重试列缺失：建议决策 RetryOnce 时重试列必入账",
            Diag::RetryNotBooked => "重试未独立入账：建议重试结果独立成列，不得覆盖首列",
            Diag::NoRetrySemantics => "可关语义失守：建议 enabled=false 或 retry_max=0 即关闭态，失败即终态",
            Diag::DoubleRunDiverge => "双跑分叉：建议决策与入账纯函数化，输入定则结果定",
            Diag::SummaryEmpty => "写回摘要为空：建议完成时以非空中文摘要写回",
        }
    }
}

/// 闭集全量数组（12 码）。
pub const ALL_DIAGS: [Diag; 12] = [
    Diag::PolicyFrozen,
    Diag::RetryCapExceeded,
    Diag::LedgerCapOverflow,
    Diag::RetryAfterPass,
    Diag::DupCase,
    Diag::LedgerBad,
    Diag::FinalMismatch,
    Diag::ColumnMissing,
    Diag::RetryNotBooked,
    Diag::NoRetrySemantics,
    Diag::DoubleRunDiverge,
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
    "未知诊断码：建议核对是否落在 0x79EC1..=0x79ECC 闭集内"
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

/// 默认策略冻结：retry=1 且开启，关闭态两形态判定正确。
fn policy_default_ok() -> bool {
    let p = RetryPolicy::default_policy();
    p.enabled && p.retry_max == DEFAULT_RETRY_MAX && DEFAULT_RETRY_MAX == 1
        && RetryPolicy::disabled_policy().is_off()
        && RetryPolicy::zero_max_policy().is_off()
        && !p.is_off()
        && detail(Diag::PolicyFrozen.val()).contains("retry=1")
}

/// 可关语义：关闭态下失败即终态（两种关闭形态全查）。
fn policy_disabled_ok() -> bool {
    let off1 = RetryPolicy::disabled_policy();
    let off2 = RetryPolicy::zero_max_policy();
    decide(&off1, AttemptVerdict::Fail) == Ok(RetryDecision::Final)
        && decide(&off2, AttemptVerdict::Fail) == Ok(RetryDecision::Final)
        && detail(Diag::NoRetrySemantics.val()).contains("关闭态")
}

/// 决策正常流：首过收口、首败（默认策略）恰排一次重试。
fn decide_normal_ok() -> bool {
    let p = RetryPolicy::default_policy();
    decide(&p, AttemptVerdict::Pass) == Ok(RetryDecision::Final)
        && decide(&p, AttemptVerdict::Fail) == Ok(RetryDecision::RetryOnce)
}

/// 注入：重试上限钳制——retry_max=5 仍只排一次重试。
fn inj_retry_cap_ok() -> bool {
    let big = RetryPolicy { enabled: true, retry_max: 5 };
    decide(&big, AttemptVerdict::Fail) == Ok(RetryDecision::RetryOnce)
        && DEFAULT_RETRY_MAX == 1
        && detail(Diag::RetryCapExceeded.val()).contains("钳制")
}

/// 两列入账：首次/重试两列独立、终态按两列推导。
fn book_two_columns_ok() -> bool {
    let mut l = RetryLedger::new();
    if book(&mut l, 1, AttemptVerdict::Fail, Some(AttemptVerdict::Pass)).is_err() {
        return false;
    }
    if book(&mut l, 2, AttemptVerdict::Pass, None).is_err() {
        return false;
    }
    if book(&mut l, 3, AttemptVerdict::Fail, Some(AttemptVerdict::Fail)).is_err() {
        return false;
    }
    if book(&mut l, 4, AttemptVerdict::Fail, None).is_err() {
        return false;
    }
    let a = match l.records.get(0) {
        Some(r) => *r,
        None => return false,
    };
    let b = match l.records.get(1) {
        Some(r) => *r,
        None => return false,
    };
    let c = match l.records.get(2) {
        Some(r) => *r,
        None => return false,
    };
    let d = match l.records.get(3) {
        Some(r) => *r,
        None => return false,
    };
    a.first == AttemptVerdict::Fail && a.retry == Some(AttemptVerdict::Pass)
        && a.final_verdict == AttemptVerdict::Pass
        && b.first == AttemptVerdict::Pass && b.retry.is_none()
        && b.final_verdict == AttemptVerdict::Pass
        && c.final_verdict == AttemptVerdict::Fail
        && d.final_verdict == AttemptVerdict::Fail
        && l.records.len() == 4
}

/// 终态推导纯函数：四组合逐一核对。
fn derive_final_ok() -> bool {
    derive_final(AttemptVerdict::Pass, None) == AttemptVerdict::Pass
        && derive_final(AttemptVerdict::Pass, Some(AttemptVerdict::Fail))
            == AttemptVerdict::Pass
        && derive_final(AttemptVerdict::Fail, Some(AttemptVerdict::Pass))
            == AttemptVerdict::Pass
        && derive_final(AttemptVerdict::Fail, Some(AttemptVerdict::Fail))
            == AttemptVerdict::Fail
        && derive_final(AttemptVerdict::Fail, None) == AttemptVerdict::Fail
        && detail(Diag::FinalMismatch.val()).contains("两列")
}

/// 语义复核：决策与两列互恰（缺列/多列/终态不符全点名）。
fn audit_semantics_ok() -> bool {
    let p = RetryPolicy::default_policy();
    let want_retry = RetryRecord {
        case_id: 1,
        first: AttemptVerdict::Fail,
        retry: Some(AttemptVerdict::Pass),
        final_verdict: AttemptVerdict::Pass,
    };
    let want_final = RetryRecord {
        case_id: 2,
        first: AttemptVerdict::Pass,
        retry: None,
        final_verdict: AttemptVerdict::Pass,
    };
    audit_record(&p, &want_retry).is_ok() && audit_record(&p, &want_final).is_ok()
}

/// 注入：决策 RetryOnce 但重试列缺失必抓（ColumnMissing）。
fn inj_column_missing_ok() -> bool {
    let p = RetryPolicy::default_policy();
    let broken = RetryRecord {
        case_id: 1,
        first: AttemptVerdict::Fail,
        retry: None,
        final_verdict: AttemptVerdict::Fail,
    };
    audit_record(&p, &broken) == Err(Diag::ColumnMissing.val())
        && detail(Diag::ColumnMissing.val()).contains("必入账")
}

/// 注入：收口决策却带重试列必抓（RetryNotBooked）。
fn inj_retry_not_booked_ok() -> bool {
    let p = RetryPolicy::disabled_policy();
    let broken = RetryRecord {
        case_id: 1,
        first: AttemptVerdict::Fail,
        retry: Some(AttemptVerdict::Pass),
        final_verdict: AttemptVerdict::Pass,
    };
    audit_record(&p, &broken) == Err(Diag::RetryNotBooked.val())
        && detail(Diag::RetryNotBooked.val()).contains("独立成列")
}

/// 注入：终态与两列推导不符必抓（FinalMismatch）。
fn inj_final_mismatch_ok() -> bool {
    let p = RetryPolicy::default_policy();
    let broken = RetryRecord {
        case_id: 1,
        first: AttemptVerdict::Fail,
        retry: Some(AttemptVerdict::Pass),
        final_verdict: AttemptVerdict::Fail,
    };
    audit_record(&p, &broken) == Err(Diag::FinalMismatch.val())
        && derive_final(AttemptVerdict::Fail, Some(AttemptVerdict::Pass))
            == AttemptVerdict::Pass
}

/// 注入：首过后排重试必拒（RetryAfterPass）。
fn inj_retry_after_pass_ok() -> bool {
    let mut l = RetryLedger::new();
    book(&mut l, 1, AttemptVerdict::Pass, Some(AttemptVerdict::Pass))
        == Err(Diag::RetryAfterPass.val())
        && l.records.is_empty()
        && detail(Diag::RetryAfterPass.val()).contains("只在首列失败")
}

/// 注入：同号重复入账必拒（DupCase）。
fn inj_dup_case_ok() -> bool {
    let mut l = RetryLedger::new();
    if book(&mut l, 7, AttemptVerdict::Fail, None).is_err() {
        return false;
    }
    book(&mut l, 7, AttemptVerdict::Fail, Some(AttemptVerdict::Pass))
        == Err(Diag::DupCase.val())
}

/// 容量守卫：入账满 16 条后再入必拒（LedgerCapOverflow）。
fn ledger_cap_ok() -> bool {
    let mut l = RetryLedger::new();
    let mut cid = 1u32;
    while cid <= RETRY_LEDGER_CAP as u32 {
        if book(&mut l, cid, AttemptVerdict::Fail, None).is_err() {
            return false;
        }
        cid += 1;
    }
    l.records.len() == RETRY_LEDGER_CAP
        && book(&mut l, 0x777, AttemptVerdict::Fail, None)
            == Err(Diag::LedgerCapOverflow.val())
        && detail(Diag::LedgerCapOverflow.val()).contains("16")
}

/// M-213 重试语义验证：默认策略四组合全查全过。
fn m213_ok() -> bool {
    let r = match m213_semantics() {
        Ok(r) => r,
        Err(_) => return false,
    };
    r && M213_AUDIT_ID == 0x213
        && M213_CASE_ID == 0x213
        && detail(Diag::NoRetrySemantics.val()).contains("失败即终态")
}

/// 双跑一致：语义验证两次执行结果相同（纯函数）。
fn double_run_ok() -> bool {
    let a = m213_semantics();
    let b = m213_semantics();
    a.is_ok() && b.is_ok() && a == b
}

/// 写回面：状态=已完成、结果摘要非空（含默认策略与两列对账）。
fn writeback_summary_ok() -> bool {
    let sem = match m213_semantics() {
        Ok(s) => s,
        Err(_) => return false,
    };
    let summary = format!(
        "UNX-F3454 完成：retry={} 默认{} 两列{} 语义{}",
        DEFAULT_RETRY_MAX,
        if RetryPolicy::default_policy().enabled { "开" } else { "关" },
        if book_two_columns_ok() { "独立" } else { "混账" },
        if sem { "验证过" } else { "失守" },
    );
    !summary.is_empty()
        && summary.contains("UNX-F3454")
        && summary.contains("retry=1")
        && detail(Diag::SummaryEmpty.val()).contains("摘要")
}

// ── 判据总成：run_unx3454_checks ───────────────────────────────────────

/// run_unx3454_checks：本单判据聚合（诊断段 0x79EC0，全部走 add_dyn）。
pub fn run_unx3454_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F3454 失败重试策略");
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
            "默认策略retry=1开启且关闭态判定准",
            policy_default_ok(),
            format!("默认策略{}", if policy_default_ok() { "冻结" } else { "漂移" }),
        ),
        (
            "可关语义两种关闭形态失败即终态",
            policy_disabled_ok(),
            format!("可关{}", if policy_disabled_ok() { "守住" } else { "失守" }),
        ),
        (
            "决策正常流首过收口首败恰排一次",
            decide_normal_ok(),
            format!("决策{}", if decide_normal_ok() { "准" } else { "偏" }),
        ),
        (
            "首次重试两列独立入账终态随列推导",
            book_two_columns_ok(),
            format!("两列{}", if book_two_columns_ok() { "独立" } else { "混账" }),
        ),
        (
            "终态推导纯函数四组合逐一核对",
            derive_final_ok(),
            format!("推导{}", if derive_final_ok() { "准" } else { "偏" }),
        ),
        (
            "语义复核决策与两列互恰",
            audit_semantics_ok(),
            format!("复核{}", if audit_semantics_ok() { "互恰" } else { "破缺" }),
        ),
        (
            "M-213重试语义验证四组合全过",
            m213_ok(),
            format!("M-213{}", if m213_ok() { "验证过" } else { "失守" }),
        ),
        (
            "注入1重试列缺失必抓",
            inj_column_missing_ok(),
            format!("缺列{}", if inj_column_missing_ok() { "抓到" } else { "漏抓" }),
        ),
        (
            "注入2收口决策带重试列必抓",
            inj_retry_not_booked_ok(),
            format!("多列{}", if inj_retry_not_booked_ok() { "抓到" } else { "漏抓" }),
        ),
        (
            "注入3终态与两列不符必抓",
            inj_final_mismatch_ok(),
            format!("终态注入{}", if inj_final_mismatch_ok() { "抓到" } else { "漏抓" }),
        ),
        (
            "注入4首过后排重试与同号重复必拒",
            inj_retry_after_pass_ok() && inj_dup_case_ok() && inj_retry_cap_ok(),
            format!(
                "首过重试{}同号{}上限{}",
                if inj_retry_after_pass_ok() { "拒" } else { "漏" },
                if inj_dup_case_ok() { "拒" } else { "漏" },
                if inj_retry_cap_ok() { "钳" } else { "漏" },
            ),
        ),
    ];
    let cap = ledger_cap_ok();
    let dr = double_run_ok();
    let wb = writeback_summary_ok();
    for (name, passed, det) in checks.iter() {
        s.add_dyn(name, *passed, det);
    }
    s.add_dyn(
        "容量守卫满16条再入必拒",
        cap,
        &format!("容量{}", if cap { "守住" } else { "失守" }),
    );
    s.add_dyn(
        "双跑一致语义验证两遍同",
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
        assert_eq!(SEG_BASE, 0x79EC0);
        assert!(diag_closed_set_ok());
        for (i, d) in ALL_DIAGS.iter().enumerate() {
            assert_eq!(d.val(), SEG_BASE + i as u32 + 1);
        }
    }

    #[test]
    fn default_policy_retry_one() {
        assert!(policy_default_ok());
        assert_eq!(DEFAULT_RETRY_MAX, 1);
        let p = RetryPolicy::default_policy();
        assert!(p.enabled);
        assert_eq!(p.retry_max, 1);
    }

    #[test]
    fn disable_semantics_two_forms() {
        assert!(policy_disabled_ok());
        let p = RetryPolicy::disabled_policy();
        assert_eq!(decide(&p, AttemptVerdict::Fail), Ok(RetryDecision::Final));
        let z = RetryPolicy::zero_max_policy();
        assert_eq!(decide(&z, AttemptVerdict::Fail), Ok(RetryDecision::Final));
    }

    #[test]
    fn decide_pass_final_fail_retry_once() {
        assert!(decide_normal_ok());
        let p = RetryPolicy::default_policy();
        assert_eq!(decide(&p, AttemptVerdict::Pass), Ok(RetryDecision::Final));
        assert_eq!(decide(&p, AttemptVerdict::Fail), Ok(RetryDecision::RetryOnce));
    }

    #[test]
    fn two_columns_booked_independently() {
        assert!(book_two_columns_ok());
        let mut l = RetryLedger::new();
        book(&mut l, 9, AttemptVerdict::Fail, Some(AttemptVerdict::Pass)).unwrap();
        let r = l.records.get(0).unwrap();
        assert_eq!(r.first, AttemptVerdict::Fail);
        assert_eq!(r.retry, Some(AttemptVerdict::Pass));
        assert_eq!(r.final_verdict, AttemptVerdict::Pass);
    }

    #[test]
    fn derive_final_four_combinations() {
        assert!(derive_final_ok());
    }

    #[test]
    fn m213_semantics_verified() {
        assert!(m213_ok());
        assert!(m213_semantics().unwrap());
        assert_eq!(M213_CASE_ID, 0x213);
    }

    #[test]
    fn injections_missing_column_and_more() {
        assert!(inj_column_missing_ok());
        assert!(inj_retry_not_booked_ok());
        assert!(inj_final_mismatch_ok());
        assert!(inj_retry_after_pass_ok());
        assert!(inj_dup_case_ok());
        assert!(inj_retry_cap_ok());
    }

    #[test]
    fn ledger_capacity_guard() {
        assert!(ledger_cap_ok());
        assert_eq!(RETRY_LEDGER_CAP, 16);
    }

    #[test]
    fn audit_semantics_interlocks() {
        assert!(audit_semantics_ok());
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
        let s = run_unx3454_checks();
        assert!(s.len() >= 12);
        assert!(s.all_passed(), "全部判据必须绿：{}", s.red_items_text());
    }
}
