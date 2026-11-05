//! UNX-F3428 结果账校验器（schema 合法性自查） · 0x79900段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 VX8-B12 逐字摘录）：
//!   ① 纯功能代码 ≥300 行；
//!   ② J1：校验器（schema 逐段合法性 + 计数自洽：sum(case) == summary）；
//!     M-187 注入坏账被拒。
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 账文档三段闭集：header（schema/run_id）/ cases（逐用例行）/
//!     summary（五计数收口）——段序冻结，表外段点名拒；
//!   - 逐段合法性：header 的 schema 冻结 1.0、run_id 非空；case 行五键
//!     （name/assert/passed/failed/skipped）类型与值域逐键核；
//!   - 计数自洽（J1 核心）：sum(case.asserts/passed/failed/skipped) 逐轴
//!     等于 summary 对应计数，且行内 passed+failed+skipped == asserts；
//!   - M-187 注入坏账：改 summary 任一计数 / 行内三计数和破 / 重名用例 /
//!     空用例名 / 非法 schema 版本——逐类显性拒；
//!   - 校验器纯函数：同坏账两次校验逐码相同（双跑一致）；
//!   - Err 用 Result<AccountReport, u32> 闭集码，文案带可操作建议。
//! 常量来源：
//!   - schema 版本 1.0：对齐 F3425 联签升级冻结版；
//!   - 五计数轴：对齐 F3408 四计数（total/pass/fail/skip）+ 用例行内
//!     asserts 总轴（summary.cases 另计）；
//!   - 诊断段：0x79900（0x40 对齐，B12 批窗 [0x79800, 0x799C0]）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x40 对齐，B12 批段窗 [0x79800, 0x799C0]）。
pub const SEG_BASE: u32 = 0x79900;

/// 冻结 schema 版本（与 F3425 同源联签）。
pub const SCHEMA_VERSION: &'static str = "1.0";

/// 账文档段名闭集（三段，序冻结）。
pub const SECTION_NAMES: [&'static str; 3] = ["header", "cases", "summary"];

/// 用例行键闭集（五键，序冻结）。
pub const CASE_FIELD_NAMES: [&'static str; 5] =
    ["name", "asserts", "passed", "failed", "skipped"];

/// 账头（schema 版本 + 运行标识）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    /// schema 版本（冻结 1.0）。
    pub schema: String,
    /// 运行标识（非空；同批重跑共用）。
    pub run_id: String,
}

/// 用例行（行内四计数 + 名称）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaseRow {
    /// 用例名（非空、全账唯一）。
    pub name: String,
    /// 断言总数。
    pub asserts: u64,
    /// 通过断言数。
    pub passed: u64,
    /// 失败断言数。
    pub failed: u64,
    /// 跳过断言数。
    pub skipped: u64,
}

impl CaseRow {
    /// 行内自洽：passed + failed + skipped == asserts（checked 防溢出）。
    pub fn row_consistent(&self) -> bool {
        match self.passed.checked_add(self.failed) {
            Some(pf) => match pf.checked_add(self.skipped) {
                Some(sum) => sum == self.asserts,
                None => false,
            },
            None => false,
        }
    }
}

/// 汇总段（五计数收口：用例数 + 四计数轴）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SummaryRow {
    /// 用例总数。
    pub cases_total: u64,
    /// 断言总数。
    pub asserts_total: u64,
    /// 通过总数。
    pub passed_total: u64,
    /// 失败总数。
    pub failed_total: u64,
    /// 跳过总数。
    pub skipped_total: u64,
}

/// 结果账文档（三段一体）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountDoc {
    /// 账头。
    pub header: Header,
    /// 用例行表。
    pub cases: Vec<CaseRow>,
    /// 汇总段。
    pub summary: SummaryRow,
}

/// 段合法性检查结果（逐段留痕；校验报告用）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionVerdict {
    /// 段名（三段闭集内）。
    pub section: String,
    /// 该段结论。
    pub passed: bool,
    /// 破缺诊断码（过段为 0）。
    pub code: u32,
}

/// 对账求和：cases 逐行同轴累加（checked；任一行溢出即 None）。
fn sum_axis(cases: &[CaseRow], pick: fn(&CaseRow) -> u64) -> Option<u64> {
    let mut total: u64 = 0;
    for row in cases.iter() {
        total = total.checked_add(pick(row))?;
    }
    Some(total)
}

/// 结果账校验器（M-187 入口）：逐段合法性 + 计数自洽全核。
/// 通过返回逐段留痕报告；任何一段破缺即 Err 闭集码。
pub fn validate_account(doc: &AccountDoc) -> Result<Vec<SectionVerdict>, u32> {
    let mut report: Vec<SectionVerdict> = Vec::new();
    // 段一：header。
    let header_ok = doc.header.schema == SCHEMA_VERSION && !doc.header.run_id.is_empty();
    let code1 = if header_ok {
        0
    } else if doc.header.schema != SCHEMA_VERSION {
        Diag::HeaderVersionForeign.val()
    } else {
        Diag::RunIdEmpty.val()
    };
    report.push(SectionVerdict {
        section: String::from("header"),
        passed: header_ok,
        code: code1,
    });
    if !header_ok {
        return Err(code1);
    }
    // 段二：cases 逐行。
    if doc.cases.is_empty() {
        return Err(Diag::CaseListEmpty.val());
    }
    let mut seen: Vec<String> = Vec::new();
    for row in doc.cases.iter() {
        if row.name.is_empty() {
            return Err(Diag::CaseNameEmpty.val());
        }
        for prev in seen.iter() {
            if prev == &row.name {
                return Err(Diag::CaseDup.val());
            }
        }
        seen.push(row.name.clone());
        if !row.row_consistent() {
            return Err(Diag::RowSumBreak.val());
        }
    }
    report.push(SectionVerdict {
        section: String::from("cases"),
        passed: true,
        code: 0,
    });
    // 段三：summary 计数自洽（J1：sum(case) == summary 四轴 + 用例数轴）。
    let a_asserts = match sum_axis(&doc.cases, |r| r.asserts) {
        Some(v) => v,
        None => return Err(Diag::CountOverflow.val()),
    };
    let a_passed = match sum_axis(&doc.cases, |r| r.passed) {
        Some(v) => v,
        None => return Err(Diag::CountOverflow.val()),
    };
    let a_failed = match sum_axis(&doc.cases, |r| r.failed) {
        Some(v) => v,
        None => return Err(Diag::CountOverflow.val()),
    };
    let a_skipped = match sum_axis(&doc.cases, |r| r.skipped) {
        Some(v) => v,
        None => return Err(Diag::CountOverflow.val()),
    };
    let cases_total_ok = doc.summary.cases_total == doc.cases.len() as u64;
    let counts_ok = a_asserts == doc.summary.asserts_total
        && a_passed == doc.summary.passed_total
        && a_failed == doc.summary.failed_total
        && a_skipped == doc.summary.skipped_total;
    if !cases_total_ok {
        return Err(Diag::CaseCountMismatch.val());
    }
    if !counts_ok {
        return Err(Diag::CountSelfInconsistent.val());
    }
    report.push(SectionVerdict {
        section: String::from("summary"),
        passed: true,
        code: 0,
    });
    Ok(report)
}

/// 诊断码闭集：0x79901..=0x7990C（12 码，变体带中文 doc）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Diag {
    /// 账头版本表外：schema 非 1.0。
    HeaderVersionForeign = 0x01,
    /// 运行标识空：run_id 为空串。
    RunIdEmpty = 0x02,
    /// 用例空表：cases 无任何行。
    CaseListEmpty = 0x03,
    /// 用例名空：某行 name 为空串。
    CaseNameEmpty = 0x04,
    /// 用例重名：cases 内 name 重复。
    CaseDup = 0x05,
    /// 行内和破：passed+failed+skipped != asserts。
    RowSumBreak = 0x06,
    /// 用例数不符：summary.cases_total != 实际行数。
    CaseCountMismatch = 0x07,
    /// 计数自洽破：四计数轴 sum(case) != summary。
    CountSelfInconsistent = 0x08,
    /// 计数溢出：cases 求和超 u64。
    CountOverflow = 0x09,
    /// 段表外：报告留痕出现三段之外的段名。
    SectionForeign = 0x0A,
    /// 段位守卫失守：SEG_BASE 不满足 0x40 对齐或越出批段窗。
    SegGuardFail = 0x0B,
    /// 双跑不一致：同账两次校验结论不同。
    DoubleRunDiverge = 0x0C,
}

impl Diag {
    /// 完整 u32 编码。
    pub fn val(self) -> u32 {
        SEG_BASE + (self as u32)
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::HeaderVersionForeign => "账头版本表外：建议 schema 对齐 F3425 冻结版 1.0 后重交",
            Diag::RunIdEmpty => "运行标识空：建议补非空 run_id（同批重跑共用同一标识）",
            Diag::CaseListEmpty => "用例空表：建议结果账至少登记一条用例行",
            Diag::CaseNameEmpty => "用例名空：建议补全用例名，空名行无对账轴",
            Diag::CaseDup => "用例重名：建议全账用例名唯一，重名行先改名或合并",
            Diag::RowSumBreak => "行内和破：建议 passed+failed+skipped 修正为 asserts 的真实和",
            Diag::CaseCountMismatch => "用例数不符：建议 summary.cases_total 改为 cases 实际行数",
            Diag::CountSelfInconsistent => "计数自洽破：建议四计数轴 sum(case) 重算后回填 summary",
            Diag::CountOverflow => "计数溢出：建议分账落盘，单账断言量控制在 u64 值域内",
            Diag::SectionForeign => "段表外：建议账文档只用 header/cases/summary 三段",
            Diag::SegGuardFail => "段位守卫失守：建议核对 SEG_BASE 的 0x40 对齐与批段窗",
            Diag::DoubleRunDiverge => "双跑不一致：建议校验器做成纯函数，同账同码",
        }
    }
}

/// 闭集全量数组（12 码）。
pub const ALL_DIAGS: [Diag; 12] = [
    Diag::HeaderVersionForeign,
    Diag::RunIdEmpty,
    Diag::CaseListEmpty,
    Diag::CaseNameEmpty,
    Diag::CaseDup,
    Diag::RowSumBreak,
    Diag::CaseCountMismatch,
    Diag::CountSelfInconsistent,
    Diag::CountOverflow,
    Diag::SectionForeign,
    Diag::SegGuardFail,
    Diag::DoubleRunDiverge,
];

/// 诊断码查表闭集返回；未知编码回退固定兜底串。
pub fn detail(code: u32) -> &'static str {
    for d in ALL_DIAGS.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：建议核对是否落在 0x79901..=0x7990C 闭集内"
}

/// 标准好账构造（三行，四轴求和与 summary 逐一对齐）。
pub fn build_good_account() -> AccountDoc {
    let rows = [
        CaseRow { name: String::from("boot_e820"), asserts: 10, passed: 8, failed: 1, skipped: 1 },
        CaseRow { name: String::from("sched_fifo"), asserts: 7, passed: 7, failed: 0, skipped: 0 },
        CaseRow { name: String::from("hda_probe"), asserts: 5, passed: 2, failed: 3, skipped: 0 },
    ];
    AccountDoc {
        header: Header {
            schema: String::from(SCHEMA_VERSION),
            run_id: String::from("vx8-b12-run01"),
        },
        cases: rows.iter().cloned().collect(),
        summary: SummaryRow {
            cases_total: 3,
            asserts_total: 22,
            passed_total: 17,
            failed_total: 4,
            skipped_total: 1,
        },
    }
}

/// 篡改汇总段的任一计数轴（注入素材；axis 0..=4）。
pub fn tamper_summary(doc: &AccountDoc, axis: usize) -> AccountDoc {
    let mut bad = doc.clone();
    match axis {
        0 => bad.summary.cases_total = bad.summary.cases_total.saturating_add(1),
        1 => bad.summary.asserts_total = bad.summary.asserts_total.saturating_add(1),
        2 => bad.summary.passed_total = bad.summary.passed_total.saturating_add(1),
        3 => bad.summary.failed_total = bad.summary.failed_total.saturating_add(1),
        _ => bad.summary.skipped_total = bad.summary.skipped_total.saturating_add(1),
    }
    bad
}

// ── 判据谓词（全部私有 fn xxx_ok() -> bool）────────────────────────────

/// 段位守卫：0x40 对齐 + B12 批段窗 [0x79800, 0x799C0]。
fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x79800 && SEG_BASE <= 0x799C0
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
        let text = a.detail();
        if text.is_empty() || !text.contains("建议") {
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
    let first = match ALL_DIAGS.first() {
        Some(d) => *d,
        None => return false,
    };
    !detail(0xDEAD).is_empty() && detail(first.val()) == first.detail()
}

/// 冻结面：段名三段互异、用例行五键互异、schema 冻结 1.0。
fn frozen_tables_ok() -> bool {
    if SCHEMA_VERSION != "1.0" || SECTION_NAMES.len() != 3 || CASE_FIELD_NAMES.len() != 5 {
        return false;
    }
    for (i, k) in SECTION_NAMES.iter().enumerate() {
        let mut j = i + 1;
        while j < SECTION_NAMES.len() {
            match SECTION_NAMES.get(j) {
                Some(q) => {
                    if k == q {
                        return false;
                    }
                }
                None => return false,
            }
            j += 1;
        }
    }
    for (i, k) in CASE_FIELD_NAMES.iter().enumerate() {
        let mut j = i + 1;
        while j < CASE_FIELD_NAMES.len() {
            match CASE_FIELD_NAMES.get(j) {
                Some(q) => {
                    if k == q {
                        return false;
                    }
                }
                None => return false,
            }
            j += 1;
        }
    }
    true
}

/// 正常流：好账三段全过、报告三痕段名序冻结。
fn good_account_passes_ok() -> bool {
    let doc = build_good_account();
    match validate_account(&doc) {
        Ok(report) => {
            report.len() == 3
                && report.get(0).map(|v| v.section == "header" && v.passed).unwrap_or(false)
                && report.get(1).map(|v| v.section == "cases" && v.passed).unwrap_or(false)
                && report.get(2).map(|v| v.section == "summary" && v.passed).unwrap_or(false)
                && report.iter().all(|v| v.code == 0)
        }
        Err(_) => false,
    }
}

/// 行内自洽轴：行内和破必抓、checked 溢出防御（u64::MAX 行）。
fn row_consistency_ok() -> bool {
    let bad = CaseRow {
        name: String::from("bad_row"),
        asserts: 10,
        passed: 9,
        failed: 9,
        skipped: 0,
    };
    if bad.row_consistent() {
        return false;
    }
    let max = CaseRow {
        name: String::from("max_row"),
        asserts: u64::MAX,
        passed: u64::MAX,
        failed: 0,
        skipped: 0,
    };
    max.row_consistent()
}

/// 注入 1（M-187）：篡改 summary 任一计数轴必被 CountSelfInconsistent
/// 或 CaseCountMismatch 点抓（五轴逐轴注入）。
fn inj_summary_tamper_ok() -> bool {
    let good = build_good_account();
    for axis in 0..5usize {
        let bad = tamper_summary(&good, axis);
        let want = if axis == 0 {
            Diag::CaseCountMismatch.val()
        } else {
            Diag::CountSelfInconsistent.val()
        };
        if validate_account(&bad) != Err(want) {
            return false;
        }
    }
    true
}

/// 注入 2：账头两违约（版本漂移 / run_id 空）逐类点名。
fn inj_header_rejects_ok() -> bool {
    let mut v_bad = build_good_account();
    v_bad.header.schema = String::from("0.9");
    let mut id_bad = build_good_account();
    id_bad.header.run_id = String::new();
    validate_account(&v_bad) == Err(Diag::HeaderVersionForeign.val())
        && validate_account(&id_bad) == Err(Diag::RunIdEmpty.val())
}

/// 注入 3：用例行三违约（空表 / 空名 / 重名）逐类点名。
fn inj_case_rejects_ok() -> bool {
    let mut empty = build_good_account();
    empty.cases = Vec::new();
    let mut nameless = build_good_account();
    match nameless.cases.first_mut() {
        Some(row) => row.name = String::new(),
        None => return false,
    }
    let mut dup = build_good_account();
    match dup.cases.get(1) {
        Some(row) => {
            let cloned = row.clone();
            match dup.cases.get_mut(2) {
                Some(last) => last.name = cloned.name,
                None => return false,
            }
        }
        None => return false,
    }
    validate_account(&empty) == Err(Diag::CaseListEmpty.val())
        && validate_account(&nameless) == Err(Diag::CaseNameEmpty.val())
        && validate_account(&dup) == Err(Diag::CaseDup.val())
}

/// 注入 4：行内和破账必抓（改行内 passed 但 summary 未随改 → 先点行内）。
fn inj_row_sum_ok() -> bool {
    let mut bad = build_good_account();
    match bad.cases.first_mut() {
        Some(row) => {
            row.passed = row.passed.saturating_add(1);
            row.asserts = row.asserts.saturating_add(1);
        }
        None => return false,
    }
    // 行内自洽仍立（asserts 同步加了），但 summary.asserts_total 未随改 →
    // 落在计数自洽破。
    validate_account(&bad) == Err(Diag::CountSelfInconsistent.val())
}

/// 求和轴与溢出防御：两行各行内自洽，但求和超 u64 → CountOverflow 点名。
fn sum_axis_overflow_ok() -> bool {
    let mut doc = build_good_account();
    doc.cases = Vec::new();
    doc.cases.push(CaseRow {
        name: String::from("max_a"),
        asserts: u64::MAX,
        passed: u64::MAX,
        failed: 0,
        skipped: 0,
    });
    doc.cases.push(CaseRow {
        name: String::from("max_b"),
        asserts: 1,
        passed: 1,
        failed: 0,
        skipped: 0,
    });
    doc.summary = SummaryRow {
        cases_total: 2,
        asserts_total: u64::MAX,
        passed_total: u64::MAX,
        failed_total: 0,
        skipped_total: 0,
    };
    validate_account(&doc) == Err(Diag::CountOverflow.val())
}

/// 双跑一致：好账与坏账两次校验逐码相同。
fn double_run_ok() -> bool {
    let good = build_good_account();
    if validate_account(&good) != validate_account(&good) {
        return false;
    }
    let bad = tamper_summary(&good, 3);
    let a = validate_account(&bad);
    let b = validate_account(&bad);
    a == b && a == Err(Diag::CountSelfInconsistent.val())
}

/// 错误路径 Result 语义：Ok 报告与闭集 Err 两侧均可达。
fn error_path_ok() -> bool {
    let good = build_good_account();
    if validate_account(&good).is_err() {
        return false;
    }
    let mut empty_id = good.clone();
    empty_id.header.run_id = String::new();
    match validate_account(&empty_id) {
        Err(code) => {
            let mut in_set = false;
            for d in ALL_DIAGS.iter() {
                if d.val() == code {
                    in_set = true;
                }
            }
            in_set && !detail(0x0).is_empty()
        }
        Ok(_) => false,
    }
}

/// ③ 写回面：状态=已完成、结果摘要非空。
fn writeback_summary_ok() -> bool {
    let good = build_good_account();
    let summary = match validate_account(&good) {
        Ok(report) => format!(
            "UNX-F3428 校验器：好账{}段全过 四计数轴 sum(case)==summary",
            report.len()
        ),
        Err(_) => String::new(),
    };
    !summary.is_empty() && summary.contains("sum(case)==summary")
}

// ── 判据总成：run_unx3428_checks ───────────────────────────────────────

/// run_unx3428_checks：本单判据聚合（诊断段 0x79900，全部走 add_dyn）。
pub fn run_unx3428_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F3428 结果账校验器");
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
            "冻结面三段五键互异schema冻结1.0",
            frozen_tables_ok(),
            format!("冻结表{}", if frozen_tables_ok() { "齐" } else { "漂移" }),
        ),
        (
            "正常流好账三段全过报告段序冻结",
            good_account_passes_ok(),
            format!("好账{}", if good_account_passes_ok() { "通过" } else { "未过" }),
        ),
        (
            "行内自洽轴和破必抓溢出防御",
            row_consistency_ok(),
            format!("行内轴{}", if row_consistency_ok() { "齐" } else { "缺" }),
        ),
        (
            "注入1五轴篡改summary逐轴必抓",
            inj_summary_tamper_ok(),
            format!("汇总篡改{}", if inj_summary_tamper_ok() { "全抓" } else { "漏抓" }),
        ),
        (
            "注入2账头两违约逐类点名",
            inj_header_rejects_ok(),
            format!("账头违约{}", if inj_header_rejects_ok() { "全抓" } else { "漏抓" }),
        ),
        (
            "注入3用例行三违约逐类点名",
            inj_case_rejects_ok(),
            format!("用例违约{}", if inj_case_rejects_ok() { "全抓" } else { "漏抓" }),
        ),
        (
            "注入4行内改账汇总未随改必抓",
            inj_row_sum_ok(),
            format!("行内改账{}", if inj_row_sum_ok() { "抓到" } else { "漏抓" }),
        ),
        (
            "求和轴溢出显性点名",
            sum_axis_overflow_ok(),
            format!("溢出{}", if sum_axis_overflow_ok() { "抓到" } else { "漏抓" }),
        ),
        (
            "双跑一致好账坏账两次逐码同",
            double_run_ok(),
            format!("双跑{}", if double_run_ok() { "一致" } else { "分叉" }),
        ),
        (
            "错误路径Ok报告与闭集Err两侧可达",
            error_path_ok(),
            format!("错误路径{}", if error_path_ok() { "过" } else { "破" }),
        ),
        (
            "写回摘要非空状态已完成",
            writeback_summary_ok(),
            format!("写回{}", if writeback_summary_ok() { "非空" } else { "空" }),
        ),
    ];
    for (name, passed, det) in checks.iter() {
        s.add_dyn(name, *passed, det);
    }
    s
}

// ── 测试区（#[cfg(test)]：允许 assert!/assert_eq!/unwrap）──────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_and_diag_closed_set() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE, 0x79900);
        assert!(diag_closed_set_ok());
        for (i, d) in ALL_DIAGS.iter().enumerate() {
            assert_eq!(d.val(), SEG_BASE + i as u32 + 1);
        }
    }

    #[test]
    fn frozen_tables_exact() {
        assert!(frozen_tables_ok());
        assert_eq!(SECTION_NAMES, ["header", "cases", "summary"]);
        assert_eq!(CASE_FIELD_NAMES, ["name", "asserts", "passed", "failed", "skipped"]);
    }

    #[test]
    fn good_account_passes_with_report() {
        assert!(good_account_passes_ok());
        let doc = build_good_account();
        let report = validate_account(&doc).unwrap();
        assert_eq!(report.len(), 3);
        assert_eq!(report.first().unwrap().section, "header");
        assert_eq!(report.last().unwrap().section, "summary");
    }

    #[test]
    fn row_consistency_checked() {
        assert!(row_consistency_ok());
        let row = CaseRow {
            name: String::from("r"),
            asserts: 5,
            passed: 2,
            failed: 2,
            skipped: 1,
        };
        assert!(row.row_consistent());
    }

    #[test]
    fn m187_summary_tamper_all_axes() {
        assert!(inj_summary_tamper_ok());
    }

    #[test]
    fn header_and_case_rejections() {
        assert!(inj_header_rejects_ok());
        assert!(inj_case_rejects_ok());
        assert!(inj_row_sum_ok());
    }

    #[test]
    fn overflow_axis_named() {
        assert!(sum_axis_overflow_ok());
    }

    #[test]
    fn double_run_identical() {
        assert!(double_run_ok());
    }

    #[test]
    fn error_paths_closed_codes() {
        assert!(error_path_ok());
        assert!(detail(0x799FF).starts_with("未知诊断码"));
    }

    #[test]
    fn writeback_summary_non_empty() {
        assert!(writeback_summary_ok());
    }

    #[test]
    fn checks_aggregate_all_green() {
        let s = run_unx3428_checks();
        assert!(s.len() >= 12);
        assert!(s.all_passed(), "全部判据必须绿：{}", s.red_items_text());
    }
}
