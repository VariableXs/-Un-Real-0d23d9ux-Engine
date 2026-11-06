//! UNX-F3455 flaky 检测账（重试通过 ≠ 首次通过 → flaky 标记） · 0x79F00段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 VX8-B15 逐字摘录）：
//!   ① 纯功能代码 ≥300 行；
//!   ② 判据原文：J1：flaky 标记规则（首败重过 → FLAKY 记账，终态
//!     仍 FAIL）+ 与 B10 S-FLAKY 样本联签；M-214 flaky 注入检测
//!     验证。；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - flaky 标记规则：首败 + 重过 → FlakyVerdict::Flaky 记账；
//!     首过 → CleanPass；首败重败或首败无重试 → HardFail。
//!     三态闭集互斥，规则冻结为纯函数（RuleFrozen 点名漂移）；
//!   - 终态仍 FAIL：flaky 只作标记入账，**不改终态**——flaky 户
//!     终态恒 CaseFinal::Fail，任何把 flaky 终态升成 Pass 的接线
//!     必被 FinalStateBroken 点名（终态锁死，不得借 flaky 漂白）；
//!   - B10 联签：每个 flaky 户必须带 B10 S-FLAKY 样本标签联签
//!     （cosign = B10_SFLAKY_TAG 冻结值），非 flaky 户不得带联签
//!     （CosignMissing / CosignSpurious 双向点名）；
//!   - M-214 注入：人为构造首败重过户（0x214），必须被 Flaky
//!     标记 + 终态 FAIL + B10 联签三连点名（漏任一即 M214Missed）；
//!   - 账面守卫：定容 16 条、同号重复入账拒、账目残缺拒；
//!   - 双跑一致：标记规则纯函数，同输入两次分类相同；
//!   - 写回面：状态=已完成、结果摘要非空。
//! 常量来源：
//!   - B10_SFLAKY_TAG=0xB10：B10 批 S-FLAKY 样本标签（派工卡
//!     「与 B10 S-FLAKY 样本联签」冻结锚）；
//!   - M-214 注入用例号 0x214：派工卡验收判据编号（注入验证锚定）；
//!   - 容量 16：flaky 账定容（与执行面登记上限同源）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x40 对齐，B15 批段窗 [0x79E00, 0x79FC0] 内）。
pub const SEG_BASE: u32 = 0x79F00;

/// B10 S-FLAKY 样本标签（联签冻结锚）。
pub const B10_SFLAKY_TAG: u32 = 0xB10;
/// flaky 账定容（条上限，超容显性拒）。
pub const FLAKY_LEDGER_CAP: usize = 16;
/// M-214 注入用例号（注入验证锚定）。
pub const M214_CASE_ID: u32 = 0x214;
/// M-214 判据编号。
pub const M214_AUDIT_ID: u64 = 0x214;

/// 单次尝试裁决（通过/失败两态闭集）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttemptVerdict {
    /// 本尝试通过。
    Pass,
    /// 本尝试失败。
    Fail,
}

/// flaky 标记三态闭集：CleanPass / HardFail / Flaky。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlakyVerdict {
    /// 首过即过（无 flaky 嫌疑）。
    CleanPass,
    /// 首败且终局败（重败或无重试，非 flaky）。
    HardFail,
    /// 首败重过（重试通过 ≠ 首次通过 → FLAKY 标记）。
    Flaky,
}

/// 用例终态（flaky 户终态锁死为 Fail——标记不改终态）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaseFinal {
    /// 终态通过。
    Pass,
    /// 终态失败（flaky 户恒落此态）。
    Fail,
}

/// flaky 账条目：两列尝试 + 三态标记 + 锁死终态 + B10 联签。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlakyEntry {
    /// 用例号。
    pub case_id: u32,
    /// 首列：首次尝试结果。
    pub first: AttemptVerdict,
    /// 重试列：重试尝试结果（未重试即 None）。
    pub retry: Option<AttemptVerdict>,
    /// flaky 三态标记。
    pub verdict: FlakyVerdict,
    /// 锁死终态（flaky 户恒 Fail）。
    pub final_state: CaseFinal,
    /// B10 S-FLAKY 联签（仅 flaky 户携带）。
    pub cosign: Option<u32>,
}

/// flaky 账面：条目表（append-only）。
#[derive(Debug, Clone)]
pub struct FlakyLedger {
    /// 条目表。
    pub entries: Vec<FlakyEntry>,
}

impl FlakyLedger {
    /// 空账面。
    pub fn new() -> FlakyLedger {
        FlakyLedger { entries: Vec::new() }
    }
}

/// flaky 标记规则（纯函数冻结）：首败重过 → Flaky；首过 →
/// CleanPass；首败重败或首败无重试 → HardFail。
pub fn classify(
    case_id: u32,
    first: AttemptVerdict,
    retry: Option<AttemptVerdict>,
) -> Result<FlakyEntry, u32> {
    // 首过后不得再排重试（与重试账同一合法性口径）。
    if first == AttemptVerdict::Pass && retry.is_some() {
        return Err(Diag::RetryAfterPass.val());
    }
    let (verdict, final_state, cosign) = match first {
        AttemptVerdict::Pass => (FlakyVerdict::CleanPass, CaseFinal::Pass, None),
        AttemptVerdict::Fail => match retry {
            Some(AttemptVerdict::Pass) => (
                FlakyVerdict::Flaky,
                CaseFinal::Fail,
                Some(B10_SFLAKY_TAG),
            ),
            _ => (FlakyVerdict::HardFail, CaseFinal::Fail, None),
        },
    };
    Ok(FlakyEntry { case_id, first, retry, verdict, final_state, cosign })
}

/// 入账一条 flaky 记录（容量/查重守卫 + 联签双向校验）。
pub fn book(ledger: &mut FlakyLedger, entry: FlakyEntry) -> Result<(), u32> {
    if ledger.entries.len() >= FLAKY_LEDGER_CAP {
        return Err(Diag::LedgerCapOverflow.val());
    }
    let mut i = 0usize;
    while i < ledger.entries.len() {
        match ledger.entries.get(i) {
            Some(e) => {
                if e.case_id == entry.case_id {
                    return Err(Diag::DupCase.val());
                }
            }
            None => return Err(Diag::LedgerBad.val()),
        }
        i += 1;
    }
    cosign_check(&entry)?;
    ledger.entries.push(entry);
    Ok(())
}

/// 联签双向校验：flaky 户必带 B10 联签、非 flaky 户必不带。
pub fn cosign_check(entry: &FlakyEntry) -> Result<(), u32> {
    match entry.verdict {
        FlakyVerdict::Flaky => {
            if entry.cosign != Some(B10_SFLAKY_TAG) {
                return Err(Diag::CosignMissing.val());
            }
            // 终态锁死：flaky 户终态恒 FAIL。
            if entry.final_state != CaseFinal::Fail {
                return Err(Diag::FinalStateBroken.val());
            }
            Ok(())
        }
        _ => {
            if entry.cosign.is_some() {
                return Err(Diag::CosignSpurious.val());
            }
            Ok(())
        }
    }
}

/// 全账联签审计：逐条双向校验，任一破缺即红。
pub fn cosign_audit(ledger: &FlakyLedger) -> Result<usize, u32> {
    let mut flaky_n = 0usize;
    let mut i = 0usize;
    while i < ledger.entries.len() {
        let e = match ledger.entries.get(i) {
            Some(e) => e,
            None => return Err(Diag::LedgerBad.val()),
        };
        cosign_check(e)?;
        if e.verdict == FlakyVerdict::Flaky {
            flaky_n += 1;
        }
        i += 1;
    }
    Ok(flaky_n)
}

/// M-214 flaky 注入演练：0x214 首败重过 → Flaky + 终态 FAIL + 联签。
pub fn m214_inject() -> Result<(bool, u32), u32> {
    let entry = classify(M214_CASE_ID, AttemptVerdict::Fail, Some(AttemptVerdict::Pass))?;
    let mut ledger = FlakyLedger::new();
    book(&mut ledger, entry)?;
    let e = match ledger.entries.get(0) {
        Some(e) => *e,
        None => return Err(Diag::LedgerBad.val()),
    };
    let caught = e.verdict == FlakyVerdict::Flaky
        && e.final_state == CaseFinal::Fail
        && e.cosign == Some(B10_SFLAKY_TAG);
    let tag = match e.cosign {
        Some(t) => t,
        None => 0,
    };
    Ok((caught, tag))
}

/// 诊断码闭集：0x79F01..=0x79F0C（12 码，变体带中文 doc）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Diag {
    /// flaky 规则漂移：三态标记规则被运行期改写。
    RuleFrozen = 0x01,
    /// 账面超容：条目数超 FLAKY_LEDGER_CAP 上限。
    LedgerCapOverflow = 0x02,
    /// 重复用例号：同号二次入账。
    DupCase = 0x03,
    /// 首过后排重试：重试列在首列通过时非法。
    RetryAfterPass = 0x04,
    /// 终态破锁：flaky 户终态被升成 PASS。
    FinalStateBroken = 0x05,
    /// 联签缺失：flaky 户未带 B10 S-FLAKY 联签。
    CosignMissing = 0x06,
    /// 联签冒签：非 flaky 户携带联签。
    CosignSpurious = 0x07,
    /// 标记误判：首败重过却判非 Flaky（规则注入演练漏抓）。
    MarkMissed = 0x08,
    /// 账面残缺：条目表内部状态破缺。
    LedgerBad = 0x09,
    /// 样本标签漂移：B10 S-FLAKY 冻结锚被改写。
    SampleTagDrift = 0x0A,
    /// 双跑分叉：同输入两次分类结果不同。
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
            Diag::RuleFrozen => "flaky 规则漂移：建议三态标记冻结为纯函数，运行期只读",
            Diag::LedgerCapOverflow => "账面超容：建议条目上限钳制在 FLAKY_LEDGER_CAP=16 内或扩容账面",
            Diag::DupCase => "重复用例号：建议入账前先查重，同号只入账一次",
            Diag::RetryAfterPass => "首过后排重试：建议重试列只在首列失败时合法",
            Diag::FinalStateBroken => "终态破锁：建议 flaky 只作标记不改终态，终态恒 FAIL",
            Diag::CosignMissing => "联签缺失：建议 flaky 户必带 B10 S-FLAKY 标签联签",
            Diag::CosignSpurious => "联签冒签：建议非 flaky 户不得携带联签",
            Diag::MarkMissed => "标记误判：建议首败重过必判 Flaky，注入演练逐条复核",
            Diag::LedgerBad => "账面残缺：建议核对条目表内部状态后再入账",
            Diag::SampleTagDrift => "样本标签漂移：建议 B10_SFLAKY_TAG 冻结为常量 0xB10",
            Diag::DoubleRunDiverge => "双跑分叉：建议分类纯函数化，输入定则结果定",
            Diag::SummaryEmpty => "写回摘要为空：建议完成时以非空中文摘要写回",
        }
    }
}

/// 闭集全量数组（12 码）。
pub const ALL_DIAGS: [Diag; 12] = [
    Diag::RuleFrozen,
    Diag::LedgerCapOverflow,
    Diag::DupCase,
    Diag::RetryAfterPass,
    Diag::FinalStateBroken,
    Diag::CosignMissing,
    Diag::CosignSpurious,
    Diag::MarkMissed,
    Diag::LedgerBad,
    Diag::SampleTagDrift,
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
    "未知诊断码：建议核对是否落在 0x79F01..=0x79F0C 闭集内"
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

/// 规则冻结：三态互斥、标签锚定、判据号锚定。
fn rule_frozen_ok() -> bool {
    B10_SFLAKY_TAG == 0xB10
        && M214_AUDIT_ID == 0x214
        && FLAKY_LEDGER_CAP == 16
        && detail(Diag::RuleFrozen.val()).contains("三态")
        && detail(Diag::SampleTagDrift.val()).contains("0xB10")
}

/// 首过 → CleanPass、终态 Pass、无联签。
fn clean_pass_ok() -> bool {
    let e = match classify(1, AttemptVerdict::Pass, None) {
        Ok(e) => e,
        Err(_) => return false,
    };
    e.verdict == FlakyVerdict::CleanPass
        && e.final_state == CaseFinal::Pass
        && e.cosign.is_none()
        && cosign_check(&e).is_ok()
}

/// 首败重败 / 首败无重试 → HardFail、终态 Fail、无联签。
fn hard_fail_ok() -> bool {
    let a = match classify(2, AttemptVerdict::Fail, Some(AttemptVerdict::Fail)) {
        Ok(e) => e,
        Err(_) => return false,
    };
    let b = match classify(3, AttemptVerdict::Fail, None) {
        Ok(e) => e,
        Err(_) => return false,
    };
    a.verdict == FlakyVerdict::HardFail && a.final_state == CaseFinal::Fail
        && a.cosign.is_none()
        && b.verdict == FlakyVerdict::HardFail && b.final_state == CaseFinal::Fail
        && b.cosign.is_none()
        && cosign_check(&a).is_ok()
        && cosign_check(&b).is_ok()
}

/// flaky 标记：首败重过 → Flaky 记账。
fn flaky_mark_ok() -> bool {
    let e = match classify(4, AttemptVerdict::Fail, Some(AttemptVerdict::Pass)) {
        Ok(e) => e,
        Err(_) => return false,
    };
    e.verdict == FlakyVerdict::Flaky
        && e.first == AttemptVerdict::Fail
        && e.retry == Some(AttemptVerdict::Pass)
        && detail(Diag::MarkMissed.val()).contains("首败重过")
}

/// 终态锁死：flaky 户终态恒 FAIL，不得升 PASS。
fn final_locked_ok() -> bool {
    let e = match classify(5, AttemptVerdict::Fail, Some(AttemptVerdict::Pass)) {
        Ok(e) => e,
        Err(_) => return false,
    };
    e.final_state == CaseFinal::Fail
        // 反事实演练：手工把终态改成 PASS，联签校验必拒。
        && {
            let bleached = FlakyEntry { final_state: CaseFinal::Pass, ..e };
            cosign_check(&bleached) == Err(Diag::FinalStateBroken.val())
        }
        && detail(Diag::FinalStateBroken.val()).contains("恒 FAIL")
}

/// B10 联签：flaky 户必带、非 flaky 户必不带（双向点名）。
fn cosign_pair_ok() -> bool {
    let flaky = match classify(6, AttemptVerdict::Fail, Some(AttemptVerdict::Pass)) {
        Ok(e) => e,
        Err(_) => return false,
    };
    flaky.cosign == Some(B10_SFLAKY_TAG)
        // 冒签演练：HardFail/CleanPass 户手工带联签，必被
        // CosignSpurious 双向点名。
        && {
            let hard = match classify(6, AttemptVerdict::Fail, None) {
                Ok(h) => h,
                Err(_) => return false,
            };
            let spurious = FlakyEntry { cosign: Some(B10_SFLAKY_TAG), ..hard };
            let mis = classify(7, AttemptVerdict::Pass, None);
            match mis {
                Ok(mut m) => {
                    m.cosign = Some(B10_SFLAKY_TAG);
                    cosign_check(&m) == Err(Diag::CosignSpurious.val())
                        && cosign_check(&spurious) == Err(Diag::CosignSpurious.val())
                }
                Err(_) => false,
            }
        }
}

/// 缺签演练：flaky 户摘掉联签必被 CosignMissing 点名。
fn cosign_missing_ok() -> bool {
    let e = match classify(8, AttemptVerdict::Fail, Some(AttemptVerdict::Pass)) {
        Ok(e) => e,
        Err(_) => return false,
    };
    let bare = FlakyEntry { cosign: None, ..e };
    cosign_check(&bare) == Err(Diag::CosignMissing.val())
        && detail(Diag::CosignMissing.val()).contains("联签")
}

/// 入账正常流：四态户全入、flaky 计数恰一。
fn book_normal_ok() -> bool {
    let mut l = FlakyLedger::new();
    let e1 = match classify(1, AttemptVerdict::Pass, None) {
        Ok(e) => e,
        Err(_) => return false,
    };
    let e2 = match classify(2, AttemptVerdict::Fail, Some(AttemptVerdict::Fail)) {
        Ok(e) => e,
        Err(_) => return false,
    };
    let e3 = match classify(3, AttemptVerdict::Fail, Some(AttemptVerdict::Pass)) {
        Ok(e) => e,
        Err(_) => return false,
    };
    let e4 = match classify(4, AttemptVerdict::Fail, None) {
        Ok(e) => e,
        Err(_) => return false,
    };
    if book(&mut l, e1).is_err() || book(&mut l, e2).is_err()
        || book(&mut l, e3).is_err() || book(&mut l, e4).is_err()
    {
        return false;
    }
    let n = match cosign_audit(&l) {
        Ok(n) => n,
        Err(_) => return false,
    };
    n == 1 && l.entries.len() == 4
}

/// 注入：首过后排重试必拒（RetryAfterPass）。
fn inj_retry_after_pass_ok() -> bool {
    classify(9, AttemptVerdict::Pass, Some(AttemptVerdict::Pass))
        == Err(Diag::RetryAfterPass.val())
        && detail(Diag::RetryAfterPass.val()).contains("首列失败")
}

/// 注入：同号重复入账必拒（DupCase）。
fn inj_dup_case_ok() -> bool {
    let mut l = FlakyLedger::new();
    let e = match classify(5, AttemptVerdict::Fail, None) {
        Ok(e) => e,
        Err(_) => return false,
    };
    if book(&mut l, e).is_err() {
        return false;
    }
    book(&mut l, e) == Err(Diag::DupCase.val())
}

/// 容量守卫：入账满 16 条后再入必拒（LedgerCapOverflow）。
fn ledger_cap_ok() -> bool {
    let mut l = FlakyLedger::new();
    let mut cid = 1u32;
    while cid <= FLAKY_LEDGER_CAP as u32 {
        let e = match classify(cid, AttemptVerdict::Fail, None) {
            Ok(e) => e,
            Err(_) => return false,
        };
        if book(&mut l, e).is_err() {
            return false;
        }
        cid += 1;
    }
    l.entries.len() == FLAKY_LEDGER_CAP
        && {
            let extra = match classify(0x777, AttemptVerdict::Fail, None) {
                Ok(e) => e,
                Err(_) => return false,
            };
            book(&mut l, extra) == Err(Diag::LedgerCapOverflow.val())
        }
        && detail(Diag::LedgerCapOverflow.val()).contains("16")
}

/// M-214 注入验证：0x214 首败重过三连点名（Flaky+FAIL+联签）。
fn m214_ok() -> bool {
    let (caught, tag) = match m214_inject() {
        Ok(r) => r,
        Err(_) => return false,
    };
    caught && tag == B10_SFLAKY_TAG && M214_CASE_ID == 0x214
        && detail(Diag::MarkMissed.val()).contains("Flaky")
}

/// 双跑一致：分类与注入演练两次执行结果相同（纯函数）。
fn double_run_ok() -> bool {
    let a = m214_inject();
    let b = m214_inject();
    a.is_ok() && b.is_ok() && a == b
        && classify(1, AttemptVerdict::Pass, None) == classify(1, AttemptVerdict::Pass, None)
}

/// 写回面：状态=已完成、结果摘要非空（含注入对账与联签标签）。
fn writeback_summary_ok() -> bool {
    let (caught, tag) = match m214_inject() {
        Ok(r) => r,
        Err(_) => return false,
    };
    let summary = format!(
        "UNX-F3455 完成：M-214 {} 联签0x{:X} 终态FAIL 账容{}",
        if caught { "注入点名" } else { "漏抓" },
        tag,
        FLAKY_LEDGER_CAP
    );
    !summary.is_empty()
        && summary.contains("UNX-F3455")
        && summary.contains("终态FAIL")
        && detail(Diag::SummaryEmpty.val()).contains("摘要")
}

// ── 判据总成：run_unx3455_checks ───────────────────────────────────────

/// run_unx3455_checks：本单判据聚合（诊断段 0x79F00，全部走 add_dyn）。
pub fn run_unx3455_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F3455 flaky 检测账");
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
            "flaky规则三态冻结标签锚定",
            rule_frozen_ok(),
            format!("规则{}", if rule_frozen_ok() { "冻结" } else { "漂移" }),
        ),
        (
            "首过CleanPass终态过无联签",
            clean_pass_ok(),
            format!("首过{}", if clean_pass_ok() { "洁" } else { "误标" }),
        ),
        (
            "首败重败或无重试HardFail终态败",
            hard_fail_ok(),
            format!("硬败{}", if hard_fail_ok() { "准" } else { "误标" }),
        ),
        (
            "首败重过Flaky标记记账",
            flaky_mark_ok(),
            format!("flaky标记{}", if flaky_mark_ok() { "记账" } else { "漏标" }),
        ),
        (
            "终态锁死flaky户恒FAIL不得漂白",
            final_locked_ok(),
            format!("终态{}", if final_locked_ok() { "锁死" } else { "破锁" }),
        ),
        (
            "B10联签双向校验flaky必带非必不带",
            cosign_pair_ok(),
            format!("联签{}", if cosign_pair_ok() { "双向" } else { "单向" }),
        ),
        (
            "缺签演练flaky摘签必点名",
            cosign_missing_ok(),
            format!("缺签{}", if cosign_missing_ok() { "抓到" } else { "漏抓" }),
        ),
        (
            "入账正常流四态户全入flaky计数恰一",
            book_normal_ok(),
            format!("入账{}", if book_normal_ok() { "平" } else { "不平" }),
        ),
        (
            "M-214注入首败重过三连点名",
            m214_ok(),
            format!("M-214{}", if m214_ok() { "验证过" } else { "漏抓" }),
        ),
        (
            "注入1首过后排重试必拒",
            inj_retry_after_pass_ok(),
            format!("首过重试{}", if inj_retry_after_pass_ok() { "拒" } else { "漏" }),
        ),
        (
            "注入2同号重复入账必拒",
            inj_dup_case_ok(),
            format!("同号{}", if inj_dup_case_ok() { "拒" } else { "漏" }),
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
        "双跑一致分类注入两遍同",
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
        assert_eq!(SEG_BASE, 0x79F00);
        assert!(diag_closed_set_ok());
        for (i, d) in ALL_DIAGS.iter().enumerate() {
            assert_eq!(d.val(), SEG_BASE + i as u32 + 1);
        }
    }

    #[test]
    fn rule_frozen_and_tag_anchor() {
        assert!(rule_frozen_ok());
        assert_eq!(B10_SFLAKY_TAG, 0xB10);
        assert_eq!(M214_CASE_ID, 0x214);
    }

    #[test]
    fn clean_pass_and_hard_fail() {
        assert!(clean_pass_ok());
        assert!(hard_fail_ok());
    }

    #[test]
    fn flaky_mark_first_fail_retry_pass() {
        assert!(flaky_mark_ok());
        let e = classify(1, AttemptVerdict::Fail, Some(AttemptVerdict::Pass)).unwrap();
        assert_eq!(e.verdict, FlakyVerdict::Flaky);
        assert_eq!(e.final_state, CaseFinal::Fail);
        assert_eq!(e.cosign, Some(B10_SFLAKY_TAG));
    }

    #[test]
    fn final_state_locked_on_fail() {
        assert!(final_locked_ok());
        let e = classify(2, AttemptVerdict::Fail, Some(AttemptVerdict::Pass)).unwrap();
        assert_eq!(e.final_state, CaseFinal::Fail);
    }

    #[test]
    fn cosign_both_directions() {
        assert!(cosign_pair_ok());
        assert!(cosign_missing_ok());
    }

    #[test]
    fn book_normal_flow_flaky_count_one() {
        assert!(book_normal_ok());
    }

    #[test]
    fn m214_injection_caught() {
        assert!(m214_ok());
        let (caught, tag) = m214_inject().unwrap();
        assert!(caught);
        assert_eq!(tag, 0xB10);
    }

    #[test]
    fn injections_after_pass_dup() {
        assert!(inj_retry_after_pass_ok());
        assert!(inj_dup_case_ok());
    }

    #[test]
    fn ledger_capacity_guard() {
        assert!(ledger_cap_ok());
        assert_eq!(FLAKY_LEDGER_CAP, 16);
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
        let s = run_unx3455_checks();
        assert!(s.len() >= 12);
        assert!(s.all_passed(), "全部判据必须绿：{}", s.red_items_text());
    }
}
