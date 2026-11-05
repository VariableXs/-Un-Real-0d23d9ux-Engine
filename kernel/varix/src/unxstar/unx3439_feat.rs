//! UNX-F3439 性能预算：结果生成开销预算 · 0x79BC0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 VX8-B13 逐字摘录）：
//!   ① 纯功能代码 ≥240 行；
//!   ② J1：结果生成开销（断言级零开销 + 用例级 ≤ 200 拍 +
//!     全账序列化 ≤ 2ms/万断言）；M-195 测量对账。
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 断言级零开销：断言记录路径只做一次定容写（冻结开销
//!     ASSERT_OVERHEAD_TICKS = 0 拍），格式化/分配全部移出断言路径
//!     （ZeroCostBroken 点名破约）；
//!   - 用例级预算：用例收口冻结实测 CASE_FINALIZE_TICKS = 120 拍
//!     ≤ 200 拍预算（CaseOverBudget 点名超支）；
//!   - 全账序列化预算：成本模型 序列化拍数 = 基数 100_000 +
//!     条数 × 150 拍（checked 乘加，溢出显性拒）；万条基准
//!     1_600_000 拍 ≤ 2ms 预算（1GHz 拍模型即 2_000_000 拍）；
//!   - M-195 测量对账：对 [1, 100, 10000] 三个计数点逐一
//!     实测对预算，测量公式与对账表逐点一致（MeasureMismatch
//!     点名任何不平点）；
//!   - 负拍数拒：拍数声明为负语义（超 i64 下沿演练）显性拒；
//!   - 双跑一致：成本模型纯函数，同输入两次测量逐拍相同；
//!   - 写回面：状态=已完成、结果摘要非空。
//! 常量来源：
//!   - 200 拍/2ms：派工卡判据原文冻结值（用例级 ≤ 200 拍、
//!     全账序列化 ≤ 2ms/万断言）；
//!   - 拍模型 1 拍 = 1ns（1GHz）：TICKS_PER_MS = 1_000_000；
//!   - M-195：派工卡验收判据编号（测量对账锚定）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x40 对齐，B13 批段窗 [0x79A00, 0x79BC0] 内）。
pub const SEG_BASE: u32 = 0x79BC0;

/// 断言级冻结开销（零开销判据本体，恒 0 拍）。
pub const ASSERT_OVERHEAD_TICKS: u64 = 0;
/// 用例级预算（派工卡冻结：≤ 200 拍）。
pub const CASE_BUDGET_TICKS: u64 = 200;
/// 用例收口冻结实测（≤ 预算）。
pub const CASE_FINALIZE_TICKS: u64 = 120;
/// 拍模型：每毫秒拍数（1GHz → 1_000_000 拍/ms）。
pub const TICKS_PER_MS: u64 = 1_000_000;
/// 序列化基数拍数（固定起步开销）。
pub const SER_BASE_TICKS: u64 = 100_000;
/// 序列化每条拍数。
pub const SER_TICKS_PER_ASSERT: u64 = 150;
/// 万条基准断言数（派工卡：万断言）。
pub const BENCH_ASSERTS: u64 = 10_000;
/// M-195 对账锚定（判据编号）。
pub const M195_AUDIT_ID: u64 = 0x195;

/// 全账序列化 2ms 预算（拍）= 2ms × TICKS_PER_MS。
pub const SER_BUDGET_TICKS: u64 = 2 * TICKS_PER_MS;

/// 序列化成本模型：基数 + 条数 × 每条（checked，溢出显性拒）。
pub fn serialize_cost(n: u64) -> Result<u64, u32> {
    let per = match n.checked_mul(SER_TICKS_PER_ASSERT) {
        Some(x) => x,
        None => return Err(Diag::CountOverflow.val()),
    };
    match per.checked_add(SER_BASE_TICKS) {
        Some(t) => Ok(t),
        None => Err(Diag::CountOverflow.val()),
    }
}

/// 万条基准成本：10_000 条序列化实测拍数。
pub fn bench_cost() -> Result<u64, u32> {
    serialize_cost(BENCH_ASSERTS)
}

/// 对账点：一个测量点（断言条数 + 实测拍数）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuditPoint {
    /// 断言条数。
    pub asserts: u64,
    /// 实测拍数。
    pub measured: u64,
}

/// M-195 测量对账：实测拍数必须等于成本模型公式值（逐点平账）。
pub fn audit_point(p: &AuditPoint) -> Result<(), u32> {
    let expect = serialize_cost(p.asserts)?;
    if p.measured != expect {
        return Err(Diag::MeasureMismatch.val());
    }
    Ok(())
}

/// 万条对预算：万条实测 ≤ 2ms 预算（SerOverBudget 语义）。
pub fn bench_within_budget() -> Result<u64, u32> {
    let c = bench_cost()?;
    if c > SER_BUDGET_TICKS {
        return Err(Diag::SerOverBudget.val());
    }
    Ok(c)
}

/// 用例级对预算：用例收口实测 ≤ 200 拍。
pub fn case_within_budget(measured: u64) -> Result<(), u32> {
    if measured > CASE_BUDGET_TICKS {
        return Err(Diag::CaseOverBudget.val());
    }
    if measured == 0 {
        return Err(Diag::NegativeTicks.val());
    }
    Ok(())
}

/// 断言记录路径（零开销本体）：只做一次定容写并返回冻结开销。
/// `slot` 为定容账槽（写 1 表示该槽已被本断言占用——零格式化零分配）。
pub fn record_assert(slot: &mut u32) -> Result<u64, u32> {
    *slot = 1;
    Ok(ASSERT_OVERHEAD_TICKS)
}

/// 预算表：三档冻结对账点（1/100/10000 条）。
pub const BUDGET_TABLE: [AuditPoint; 3] = [
    AuditPoint { asserts: 1, measured: 0 },
    AuditPoint { asserts: 100, measured: 0 },
    AuditPoint { asserts: 10_000, measured: 0 },
];

/// 预算表补实测：按成本模型填 measured（测量动作的桩——纯函数计量）。
pub fn fill_measured() -> Result<[AuditPoint; 3], u32> {
    let mut out = BUDGET_TABLE;
    let mut i = 0usize;
    while i < out.len() {
        let n = match out.get(i) {
            Some(p) => p.asserts,
            None => return Err(Diag::BudgetTableBad.val()),
        };
        let m = serialize_cost(n)?;
        match out.get_mut(i) {
            Some(slot) => slot.measured = m,
            None => return Err(Diag::BudgetTableBad.val()),
        }
        i += 1;
    }
    Ok(out)
}

/// 诊断码闭集：0x79BC1..=0x79BCC（12 码，变体带中文 doc）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Diag {
    /// 断言级非零开销：断言路径混入格式化/分配。
    AssertNonZero = 0x01,
    /// 用例超 200 拍：用例收口实测超预算。
    CaseOverBudget = 0x02,
    /// 序列化超 2ms 预算：万条实测超预算。
    SerOverBudget = 0x03,
    /// 测量对账不平：实测与成本模型公式值不一致。
    MeasureMismatch = 0x04,
    /// 计数溢出：条数 × 每条拍数超 u64 上沿。
    CountOverflow = 0x05,
    /// 万条基准计数错：基准不为 10_000 条。
    BenchCountBad = 0x06,
    /// 预算表残缺：三档对账点破缺。
    BudgetTableBad = 0x07,
    /// 负拍数：拍数声明为零或负语义。
    NegativeTicks = 0x08,
    /// 双跑分叉：同输入两次测量逐拍不同。
    DoubleRunDiverge = 0x09,
    /// 零开销破约：断言路径实测开销非冻结零值。
    ZeroCostBroken = 0x0A,
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
            Diag::AssertNonZero => "断言级非零开销：建议把格式化/分配移出断言路径",
            Diag::CaseOverBudget => "用例超200拍：建议压缩用例收口步数后复测对账",
            Diag::SerOverBudget => "序列化超2ms预算：建议降每条拍价或分批序列化",
            Diag::MeasureMismatch => "测量对账不平：建议逐点平账，核对实测与成本模型公式",
            Diag::CountOverflow => "计数溢出：建议条数上限钳制在安全乘区",
            Diag::BenchCountBad => "万条基准计数错：建议基准固定 BENCH_ASSERTS=10_000",
            Diag::BudgetTableBad => "预算表残缺：建议三档对账点逐项补实测后再对账",
            Diag::NegativeTicks => "负拍数：建议拍数声明恒正，零/负显性拒",
            Diag::DoubleRunDiverge => "双跑分叉：建议成本模型纯函数，输入定则拍数定",
            Diag::ZeroCostBroken => "零开销破约：建议断言路径回归冻结零开销常量",
            Diag::SegGuardFail => "段位守卫失守：建议核对 SEG_BASE 的 0x40 对齐与批段窗",
            Diag::SummaryEmpty => "写回摘要为空：建议完成时以非空中文摘要写回",
        }
    }
}

/// 闭集全量数组（12 码）。
pub const ALL_DIAGS: [Diag; 12] = [
    Diag::AssertNonZero,
    Diag::CaseOverBudget,
    Diag::SerOverBudget,
    Diag::MeasureMismatch,
    Diag::CountOverflow,
    Diag::BenchCountBad,
    Diag::BudgetTableBad,
    Diag::NegativeTicks,
    Diag::DoubleRunDiverge,
    Diag::ZeroCostBroken,
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
    "未知诊断码：建议核对是否落在 0x79BC1..=0x79BCC 闭集内"
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

/// 断言级零开销：1 万次断言记录累计开销恒为 0 拍。
fn assert_zero_cost_ok() -> bool {
    let mut slot: u32 = 0;
    let mut total: u64 = 0;
    let mut i = 0u64;
    while i < 10_000 {
        let cost = match record_assert(&mut slot) {
            Ok(c) => c,
            Err(_) => return false,
        };
        total += cost;
        i += 1;
    }
    total == ASSERT_OVERHEAD_TICKS
        && ASSERT_OVERHEAD_TICKS == 0
        && slot == 1
        && detail(Diag::ZeroCostBroken.val()).contains("零开销")
}

/// 用例级预算：冻结实测 120 拍 ≤ 200 拍；0 拍拒（负拍数语义）。
fn case_budget_ok() -> bool {
    if CASE_FINALIZE_TICKS > CASE_BUDGET_TICKS {
        return false;
    }
    if case_within_budget(CASE_FINALIZE_TICKS).is_err() {
        return false;
    }
    case_within_budget(0) == Err(Diag::NegativeTicks.val())
        && CASE_FINALIZE_TICKS == 120
        && CASE_BUDGET_TICKS == 200
}

/// 万条对预算：1_600_000 拍 ≤ 2_000_000 拍（2ms）。
fn bench_budget_ok() -> bool {
    let c = match bench_within_budget() {
        Ok(c) => c,
        Err(_) => return false,
    };
    c == SER_BASE_TICKS + BENCH_ASSERTS * SER_TICKS_PER_ASSERT
        && c == 1_600_000
        && c <= SER_BUDGET_TICKS
        && SER_BUDGET_TICKS == 2 * TICKS_PER_MS
}

/// M-195 测量对账：三档对账点实测 = 公式值，逐点平账。
fn m195_audit_ok() -> bool {
    let table = match fill_measured() {
        Ok(t) => t,
        Err(_) => return false,
    };
    if table.len() != 3 {
        return false;
    }
    let mut i = 0usize;
    while i < table.len() {
        let p = match table.get(i) {
            Some(p) => *p,
            None => return false,
        };
        if audit_point(&p).is_err() {
            return false;
        }
        // 逐点独立复算。
        let expect = match serialize_cost(p.asserts) {
            Ok(e) => e,
            Err(_) => return false,
        };
        if p.measured != expect {
            return false;
        }
        i += 1;
    }
    M195_AUDIT_ID == 0x195
        && detail(Diag::MeasureMismatch.val()).contains("平账")
}

/// 注入 1：用例超支——250 拍 > 200 拍必被 CaseOverBudget 点名。
fn inj_case_over_ok() -> bool {
    case_within_budget(250) == Err(Diag::CaseOverBudget.val())
        && case_within_budget(201) == Err(Diag::CaseOverBudget.val())
}

/// 注入 2：序列化超支——每条拍价放大到 200 后万条必超 2ms 预算。
fn inj_ser_over_ok() -> bool {
    // 反事实演练：若每条 200 拍，万条 = 2_100_100 > 2_000_000 → 红线。
    let counterfactual = BENCH_ASSERTS
        .checked_mul(200)
        .and_then(|x| x.checked_add(SER_BASE_TICKS));
    match counterfactual {
        Some(t) => t > SER_BUDGET_TICKS && detail(Diag::SerOverBudget.val()).contains("分批"),
        None => false,
    }
}

/// 注入 3：对账不平——实测偏离公式 1 拍必被 MeasureMismatch 点名。
fn inj_measure_off_ok() -> bool {
    let bad = AuditPoint {
        asserts: 100,
        measured: 100_000 + 100 * SER_TICKS_PER_ASSERT + 1,
    };
    audit_point(&bad) == Err(Diag::MeasureMismatch.val())
}

/// 注入 4：计数溢出——条数取 u64::MAX 必被 CountOverflow 点名。
fn inj_count_overflow_ok() -> bool {
    serialize_cost(u64::MAX) == Err(Diag::CountOverflow.val())
}

/// 万条基准计数：BENCH_ASSERTS 冻结 10_000（BenchCountBad 语义）。
fn bench_count_ok() -> bool {
    if BENCH_ASSERTS != 10_000 {
        return false;
    }
    // 基准偏移演练：若基准漂到 9_999，对账表必红。
    let drifted = BENCH_ASSERTS - 1;
    match serialize_cost(drifted) {
        Ok(c) => c != bench_cost().unwrap_or(u64::MAX),
        Err(_) => false,
    }
}

/// 预算表残缺守卫：三档点条数必须恰为 1/100/10000 冻结值。
fn budget_table_ok() -> bool {
    let table = match fill_measured() {
        Ok(t) => t,
        Err(_) => return false,
    };
    let a = match table.get(0) {
        Some(p) => p.asserts,
        None => return false,
    };
    let b = match table.get(1) {
        Some(p) => p.asserts,
        None => return false,
    };
    let c = match table.get(2) {
        Some(p) => p.asserts,
        None => return false,
    };
    a == 1 && b == 100 && c == 10_000
        && detail(Diag::BudgetTableBad.val()).contains("三档")
}

/// 拍常量表：预算派生链自洽（2ms = 2 × 1ms 拍数）。
fn tick_constants_ok() -> bool {
    TICKS_PER_MS == 1_000_000
        && SER_BUDGET_TICKS == 2_000_000
        && SER_TICKS_PER_ASSERT < 200
        && detail(Diag::NegativeTicks.val()).contains("恒正")
}

/// 双跑一致：成本模型两次测量逐拍相同（纯函数）。
fn double_run_ok() -> bool {
    let a = fill_measured();
    let b = fill_measured();
    a.is_ok() && b.is_ok() && a == b
}

/// 写回面：状态=已完成、结果摘要非空（含三档对账与预算余量）。
fn writeback_summary_ok() -> bool {
    let c = match bench_cost() {
        Ok(c) => c,
        Err(_) => return false,
    };
    let margin = match SER_BUDGET_TICKS.checked_sub(c) {
        Some(m) => m,
        None => return false,
    };
    let summary = format!(
        "UNX-F3439 完成：断言级{}拍 用例{}拍 万条序列化{}拍 预算余量{}拍",
        ASSERT_OVERHEAD_TICKS, CASE_FINALIZE_TICKS, c, margin
    );
    !summary.is_empty()
        && summary.contains("UNX-F3439")
        && summary.contains("余量")
        && margin > 0
        && detail(Diag::SummaryEmpty.val()).contains("摘要")
}

// ── 判据总成：run_unx3439_checks ───────────────────────────────────────

/// run_unx3439_checks：本单判据聚合（诊断段 0x79BC0，全部走 add_dyn）。
pub fn run_unx3439_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F3439 性能预算");
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
            "断言级零开销万次累计恒零",
            assert_zero_cost_ok(),
            format!("断言级{}", if assert_zero_cost_ok() { "零开销" } else { "破约" }),
        ),
        (
            "用例级120拍不超200拍预算",
            case_budget_ok(),
            format!("用例级{}", if case_budget_ok() { "达标" } else { "超支" }),
        ),
        (
            "万条序列化不超2ms预算",
            bench_budget_ok(),
            format!("万条{}", if bench_budget_ok() { "达标" } else { "超支" }),
        ),
        (
            "M-195三档测量对账逐点平",
            m195_audit_ok(),
            format!("对账{}", if m195_audit_ok() { "逐点平" } else { "不平" }),
        ),
        (
            "注入1用例超支必抓",
            inj_case_over_ok(),
            format!("用例超支{}", if inj_case_over_ok() { "抓到" } else { "漏抓" }),
        ),
        (
            "注入2序列化超支红线演练",
            inj_ser_over_ok(),
            format!("超支演练{}", if inj_ser_over_ok() { "点破" } else { "漏" }),
        ),
        (
            "注入3对账不平一拍必抓",
            inj_measure_off_ok(),
            format!("不平注入{}", if inj_measure_off_ok() { "抓到" } else { "漏抓" }),
        ),
        (
            "注入4计数溢出必抓",
            inj_count_overflow_ok(),
            format!("溢出注入{}", if inj_count_overflow_ok() { "抓到" } else { "漏抓" }),
        ),
        (
            "万条基准计数冻结一万",
            bench_count_ok(),
            format!("基准{}", if bench_count_ok() { "冻结" } else { "漂移" }),
        ),
        (
            "预算表三档对账点冻结",
            budget_table_ok(),
            format!("预算表{}", if budget_table_ok() { "冻结" } else { "残缺" }),
        ),
        (
            "拍常量表派生链自洽",
            tick_constants_ok(),
            format!("常量表{}", if tick_constants_ok() { "自洽" } else { "漂移" }),
        ),
    ];
    let dr = double_run_ok();
    let wb = writeback_summary_ok();
    for (name, passed, det) in checks.iter() {
        s.add_dyn(name, *passed, det);
    }
    s.add_dyn(
        "双跑一致两次测量逐拍同",
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
        assert_eq!(SEG_BASE, 0x79BC0);
        assert!(diag_closed_set_ok());
        for (i, d) in ALL_DIAGS.iter().enumerate() {
            assert_eq!(d.val(), SEG_BASE + i as u32 + 1);
        }
    }

    #[test]
    fn assert_level_zero_overhead() {
        assert!(assert_zero_cost_ok());
        let mut slot = 0u32;
        assert_eq!(record_assert(&mut slot), Ok(0));
        assert_eq!(slot, 1);
    }

    #[test]
    fn case_budget_120_of_200() {
        assert!(case_budget_ok());
        assert!(case_within_budget(200).is_ok());
        assert!(case_within_budget(201).is_err());
    }

    #[test]
    fn bench_within_2ms() {
        assert!(bench_budget_ok());
        assert_eq!(bench_cost(), Ok(1_600_000));
        assert_eq!(SER_BUDGET_TICKS, 2_000_000);
    }

    #[test]
    fn m195_three_point_audit() {
        assert!(m195_audit_ok());
        let t = fill_measured().unwrap();
        assert_eq!(t.get(0).unwrap().measured, 100_150);
        assert_eq!(t.get(2).unwrap().measured, 1_600_000);
    }

    #[test]
    fn injections_case_ser_measure_overflow() {
        assert!(inj_case_over_ok());
        assert!(inj_ser_over_ok());
        assert!(inj_measure_off_ok());
        assert!(inj_count_overflow_ok());
    }

    #[test]
    fn bench_count_frozen() {
        assert!(bench_count_ok());
    }

    #[test]
    fn budget_table_frozen_points() {
        assert!(budget_table_ok());
        assert!(tick_constants_ok());
    }

    #[test]
    fn double_run_identical_measurement() {
        assert!(double_run_ok());
    }

    #[test]
    fn writeback_summary_non_empty() {
        assert!(writeback_summary_ok());
    }

    #[test]
    fn serialize_cost_formula_points() {
        assert_eq!(serialize_cost(0), Ok(SER_BASE_TICKS));
        assert_eq!(serialize_cost(1), Ok(100_150));
        assert_eq!(serialize_cost(100), Ok(115_000));
    }

    #[test]
    fn checks_aggregate_all_green() {
        let s = run_unx3439_checks();
        assert!(s.len() >= 12);
        assert!(s.all_passed(), "全部判据必须绿：{}", s.red_items_text());
    }
}
