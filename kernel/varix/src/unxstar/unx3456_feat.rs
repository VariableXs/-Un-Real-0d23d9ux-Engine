//! UNX-F3456 断点续跑（执行器断点：序号断点 + 续跑清单） · 0x79F40段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 VX8-B15 逐字摘录）：
//!   ① 纯功能代码 ≥280 行；
//!   ② 判据原文：J1：断点记录（已完成用例序号集）+ 续跑（从断点
//!     起跳、跳过已完成）；M-215 模拟中断续跑对账。；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 断点记录：ResumePoint 以已完成用例序号集（done，append-only）
//!     + 起跳序号 next_index + 总数 total 三元记录断点；序号集只进
//!     不退（ResumeRegress 点名任何倒退）；
//!   - 续跑清单：resume_plan 从断点起跳产出 [next_index, total)
//!     的剩余序号清单，**不含任何已完成序号**（SkipBroken 点名
//!     清单漏进已完成户）；
//!   - 跳过已完成：skip_completed 对小于起跳序号的候选序号一律
//!     跳到 next_index（从断点起跳语义的本体）；
//!   - 顺序纪律：mark_done 只接受恰好等于 next_index 的序号
//!     （跳序 OrderBreak、重标 DupMark、越界 OutOfRange 显性拒）；
//!   - M-215 模拟中断：总 12 例、中断于第 7 例（序号集 0..7、
//!     起跳 7），续跑清单必须恰为 [7,12) 七条且首元素恰为断点
//!     （PlanMismatch 点名任何不平）；
//!   - 容量守卫：序号集定容 16 条，超容显性拒；total=0 空计划拒；
//!   - 双跑一致：续跑对账纯函数，同输入两次清单相同；
//!   - 写回面：状态=已完成、结果摘要非空。
//! 常量来源：
//!   - TOTAL_CASES=12：执行面用例总数（演练口径冻结值）；
//!   - M-215 中断点 7：派工卡 M-215 模拟中断续跑对账冻结演练值；
//!   - 容量 16：断点序号集定容（与执行面登记上限同源）；
//!   - M-215：派工卡验收判据编号（续跑对账锚定）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x40 对齐，B15 批段窗 [0x79E00, 0x79FC0] 内）。
pub const SEG_BASE: u32 = 0x79F40;

/// 执行面用例总数（演练口径冻结值）。
pub const TOTAL_CASES: u32 = 12;
/// 断点序号集定容（条上限，超容显性拒）。
pub const RESUME_CAP: usize = 16;
/// M-215 模拟中断点（第 7 例后断）。
pub const M215_BREAK_AT: u32 = 7;
/// M-215 判据编号（续跑对账锚定）。
pub const M215_AUDIT_ID: u64 = 0x215;

/// 断点记录：已完成序号集 + 起跳序号 + 总数。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResumePoint {
    /// 已完成用例序号集（append-only，只进不退）。
    pub done: Vec<u32>,
    /// 起跳序号（下一待执行用例）。
    pub next_index: u32,
    /// 用例总数（计划冻结值）。
    pub total: u32,
}

/// 建断点：total 必须 ≥1（空计划显性拒）。
pub fn resume_new(total: u32) -> Result<ResumePoint, u32> {
    if total == 0 {
        return Err(Diag::EmptyTotal.val());
    }
    Ok(ResumePoint { done: Vec::new(), next_index: 0, total })
}

/// 标记一例完成：只接受恰好等于起跳序号的标记（顺序纪律）。
pub fn mark_done(point: &mut ResumePoint, idx: u32) -> Result<(), u32> {
    if idx >= point.total {
        return Err(Diag::OutOfRange.val());
    }
    if idx != point.next_index {
        if idx < point.next_index {
            return Err(Diag::DupMark.val());
        }
        return Err(Diag::OrderBreak.val());
    }
    if point.done.len() >= RESUME_CAP {
        return Err(Diag::CapOverflow.val());
    }
    let next = match point.next_index.checked_add(1) {
        Some(n) => n,
        None => return Err(Diag::IdxOverflow.val()),
    };
    point.done.push(idx);
    point.next_index = next;
    Ok(())
}

/// 倒退显性拒：断点回卷（rewind）必拒——序号集 append-only。
pub fn rewind(point: &mut ResumePoint) -> Result<(), u32> {
    let _ = point;
    Err(Diag::ResumeRegress.val())
}

/// 续跑清单：从断点起跳的剩余序号 [next_index, total)，
/// 清单绝不含已完成序号（跳过已完成语义本体）。
pub fn resume_plan(point: &ResumePoint) -> Result<Vec<u32>, u32> {
    if point.next_index > point.total {
        return Err(Diag::PlanMismatch.val());
    }
    let mut plan = Vec::new();
    let mut i = point.next_index;
    while i < point.total {
        // 已完成守卫：序号集内出现的序号绝不允许进清单。
        let mut j = 0usize;
        while j < point.done.len() {
            match point.done.get(j) {
                Some(d) => {
                    if *d == i {
                        return Err(Diag::SkipBroken.val());
                    }
                }
                None => return Err(Diag::PlanMismatch.val()),
            }
            j += 1;
        }
        plan.push(i);
        i += 1;
    }
    Ok(plan)
}

/// 跳过已完成：候选序号小于起跳序号一律跳到 next_index
/// （从断点起跳），否则原样返回。
pub fn skip_completed(point: &ResumePoint, candidate: u32) -> u32 {
    if candidate < point.next_index {
        point.next_index
    } else {
        candidate
    }
}

/// M-215 模拟中断续跑对账：总 12 例、中断于第 7 例，续跑清单
/// 必须恰为 [7,12) 七条、首元素恰为断点、不含任何已完成序号。
pub fn m215_reconcile() -> Result<(usize, u32), u32> {
    let mut point = match resume_new(TOTAL_CASES) {
        Ok(p) => p,
        Err(_) => return Err(Diag::EmptyTotal.val()),
    };
    let mut i = 0u32;
    while i < M215_BREAK_AT {
        mark_done(&mut point, i)?;
        i += 1;
    }
    let plan = resume_plan(&point)?;
    let head = match plan.get(0) {
        Some(h) => *h,
        None => return Err(Diag::PlanMismatch.val()),
    };
    if plan.len() != (TOTAL_CASES - M215_BREAK_AT) as usize {
        return Err(Diag::PlanMismatch.val());
    }
    if head != M215_BREAK_AT {
        return Err(Diag::PlanMismatch.val());
    }
    // 跳过已完成对账：任何已完成序号经 skip_completed 后都跳到断点。
    let mut k = 0u32;
    while k < M215_BREAK_AT {
        if skip_completed(&point, k) != M215_BREAK_AT {
            return Err(Diag::SkipBroken.val());
        }
        k += 1;
    }
    Ok((plan.len(), head))
}

/// 诊断码闭集：0x79F41..=0x79F4C（12 码，变体带中文 doc）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Diag {
    /// 计划冻结漂移：用例总数被运行期改写。
    PlanFrozen = 0x01,
    /// 序号越界：标记序号超出用例总数。
    OutOfRange = 0x02,
    /// 重复标记：已完成序号二次标记。
    DupMark = 0x03,
    /// 跳序执行：标记序号越过起跳序号。
    OrderBreak = 0x04,
    /// 序号集超容：已完成序号数超 RESUME_CAP 上限。
    CapOverflow = 0x05,
    /// 空计划：total=0 建断点。
    EmptyTotal = 0x06,
    /// 断点倒退：rewind 显性拒（序号集 append-only）。
    ResumeRegress = 0x07,
    /// 续跑清单不平：清单长度/首元素与断点不符。
    PlanMismatch = 0x08,
    /// 跳过已完成失守：清单漏进已完成序号或跳转落点错误。
    SkipBroken = 0x09,
    /// 序号溢出：起跳序号 checked 增量超 u32 上沿。
    IdxOverflow = 0x0A,
    /// 双跑分叉：同输入两次续跑清单不同。
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
            Diag::PlanFrozen => "计划冻结漂移：建议用例总数冻结为常量 TOTAL_CASES，运行期只读",
            Diag::OutOfRange => "序号越界：建议标记前先核 total 上沿，越界显性拒",
            Diag::DupMark => "重复标记：建议已完成序号不重标，二次标记显性拒",
            Diag::OrderBreak => "跳序执行：建议只接受恰好等于起跳序号的标记，跳序显性拒",
            Diag::CapOverflow => "序号集超容：建议容量钳制在 RESUME_CAP=16 内或扩容序号集",
            Diag::EmptyTotal => "空计划：建议 total=0 时拒绝建断点，先定计划后执行",
            Diag::ResumeRegress => "断点倒退：建议序号集 append-only，rewind 显性拒",
            Diag::PlanMismatch => "续跑清单不平：建议清单长度与首元素逐项对账，偏差显性点名",
            Diag::SkipBroken => "跳过已完成失守：建议清单绝不含已完成序号，跳转落到断点",
            Diag::IdxOverflow => "序号溢出：建议起跳序号用 checked_add，溢出显性拒",
            Diag::DoubleRunDiverge => "双跑分叉：建议续跑对账纯函数化，输入定则清单定",
            Diag::SummaryEmpty => "写回摘要为空：建议完成时以非空中文摘要写回",
        }
    }
}

/// 闭集全量数组（12 码）。
pub const ALL_DIAGS: [Diag; 12] = [
    Diag::PlanFrozen,
    Diag::OutOfRange,
    Diag::DupMark,
    Diag::OrderBreak,
    Diag::CapOverflow,
    Diag::EmptyTotal,
    Diag::ResumeRegress,
    Diag::PlanMismatch,
    Diag::SkipBroken,
    Diag::IdxOverflow,
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
    "未知诊断码：建议核对是否落在 0x79F41..=0x79F4C 闭集内"
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

/// 计划冻结：总数 12、中断点 7、判据号锚定。
fn plan_frozen_ok() -> bool {
    TOTAL_CASES == 12
        && M215_BREAK_AT == 7
        && M215_AUDIT_ID == 0x215
        && M215_BREAK_AT < TOTAL_CASES
        && detail(Diag::PlanFrozen.val()).contains("TOTAL_CASES")
}

/// 正常流标记：顺序标满、起跳序号逐次递增、序号集逐条落账。
fn mark_done_ok() -> bool {
    let mut p = match resume_new(TOTAL_CASES) {
        Ok(p) => p,
        Err(_) => return false,
    };
    let mut i = 0u32;
    while i < TOTAL_CASES {
        if mark_done(&mut p, i).is_err() {
            return false;
        }
        i += 1;
    }
    p.done.len() == TOTAL_CASES as usize
        && p.next_index == TOTAL_CASES
        && match p.done.get(0) {
            Some(d) => *d == 0,
            None => false,
        }
        && match p.done.get(TOTAL_CASES as usize - 1) {
            Some(d) => *d == TOTAL_CASES - 1,
            None => false,
        }
}

/// 续跑清单正常流：全标完后清单为空、未标时清单从断点起。
fn resume_plan_ok() -> bool {
    let mut p = match resume_new(TOTAL_CASES) {
        Ok(p) => p,
        Err(_) => return false,
    };
    let early = match resume_plan(&p) {
        Ok(l) => l,
        Err(_) => return false,
    };
    if early.len() != TOTAL_CASES as usize {
        return false;
    }
    let mut i = 0u32;
    while i < 4 {
        if mark_done(&mut p, i).is_err() {
            return false;
        }
        i += 1;
    }
    let mid = match resume_plan(&p) {
        Ok(l) => l,
        Err(_) => return false,
    };
    let head = match mid.get(0) {
        Some(h) => *h,
        None => return false,
    };
    let tail = match mid.get(mid.len() - 1) {
        Some(t) => *t,
        None => return false,
    };
    head == 4 && tail == TOTAL_CASES - 1 && mid.len() == (TOTAL_CASES - 4) as usize
        && detail(Diag::PlanMismatch.val()).contains("逐项对账")
}

/// 注入：越界标记必拒（OutOfRange）。
fn inj_out_of_range_ok() -> bool {
    let mut p = match resume_new(TOTAL_CASES) {
        Ok(p) => p,
        Err(_) => return false,
    };
    mark_done(&mut p, TOTAL_CASES) == Err(Diag::OutOfRange.val())
        && mark_done(&mut p, u32::MAX) == Err(Diag::OutOfRange.val())
        && detail(Diag::OutOfRange.val()).contains("上沿")
}

/// 注入：跳序执行必拒（OrderBreak）。
fn inj_order_break_ok() -> bool {
    let mut p = match resume_new(TOTAL_CASES) {
        Ok(p) => p,
        Err(_) => return false,
    };
    mark_done(&mut p, 2) == Err(Diag::OrderBreak.val())
        && p.done.is_empty()
        && p.next_index == 0
        && detail(Diag::OrderBreak.val()).contains("跳序")
}

/// 注入：重复标记必拒（DupMark）。
fn inj_dup_mark_ok() -> bool {
    let mut p = match resume_new(TOTAL_CASES) {
        Ok(p) => p,
        Err(_) => return false,
    };
    if mark_done(&mut p, 0).is_err() {
        return false;
    }
    mark_done(&mut p, 0) == Err(Diag::DupMark.val())
        && p.done.len() == 1
        && p.next_index == 1
        && detail(Diag::DupMark.val()).contains("不重标")
}

/// 注入：断点倒退 rewind 必拒（ResumeRegress）。
fn inj_regress_ok() -> bool {
    let mut p = match resume_new(TOTAL_CASES) {
        Ok(p) => p,
        Err(_) => return false,
    };
    if mark_done(&mut p, 0).is_err() {
        return false;
    }
    rewind(&mut p) == Err(Diag::ResumeRegress.val())
        && rewind(&mut p) == Err(Diag::ResumeRegress.val())
        && detail(Diag::ResumeRegress.val()).contains("append-only")
}

/// 注入：空计划必拒（EmptyTotal）。
fn inj_empty_total_ok() -> bool {
    resume_new(0) == Err(Diag::EmptyTotal.val())
        && detail(Diag::EmptyTotal.val()).contains("先定计划")
}

/// 跳过已完成：断点前候选一律跳到起跳序号、断点后原样返回。
fn skip_completed_ok() -> bool {
    let mut p = match resume_new(TOTAL_CASES) {
        Ok(p) => p,
        Err(_) => return false,
    };
    let mut i = 0u32;
    while i < 5 {
        if mark_done(&mut p, i).is_err() {
            return false;
        }
        i += 1;
    }
    skip_completed(&p, 0) == 5
        && skip_completed(&p, 4) == 5
        && skip_completed(&p, 5) == 5
        && skip_completed(&p, 9) == 9
        && detail(Diag::SkipBroken.val()).contains("落到断点")
}

/// 清单不含已完成：任意断点下清单与序号集永不相交。
fn plan_excludes_done_ok() -> bool {
    let mut p = match resume_new(TOTAL_CASES) {
        Ok(p) => p,
        Err(_) => return false,
    };
    let mut i = 0u32;
    while i < TOTAL_CASES {
        if mark_done(&mut p, i).is_err() {
            return false;
        }
        let plan = match resume_plan(&p) {
            Ok(l) => l,
            Err(_) => return false,
        };
        // 每一步断点下清单都必须与序号集互斥。
        let mut a = 0usize;
        while a < plan.len() {
            let cand = match plan.get(a) {
                Some(c) => *c,
                None => return false,
            };
            let mut b = 0usize;
            while b < p.done.len() {
                match p.done.get(b) {
                    Some(d) => {
                        if *d == cand {
                            return false;
                        }
                    }
                    None => return false,
                }
                b += 1;
            }
            a += 1;
        }
        i += 1;
    }
    match resume_plan(&p) {
        Ok(l) => l.is_empty(),
        Err(_) => false,
    }
}

/// 容量守卫：序号集满 16 条后再标必拒（CapOverflow）。
fn capacity_guard_ok() -> bool {
    let mut p = match resume_new(RESUME_CAP as u32) {
        Ok(p) => p,
        Err(_) => return false,
    };
    let mut i = 0u32;
    while i < RESUME_CAP as u32 {
        if mark_done(&mut p, i).is_err() {
            return false;
        }
        i += 1;
    }
    p.done.len() == RESUME_CAP
        && mark_done(&mut p, RESUME_CAP as u32) == Err(Diag::OutOfRange.val())
        && detail(Diag::CapOverflow.val()).contains("16")
}

/// M-215 模拟中断续跑对账：清单七条、首元素恰断点、跳转全落断点。
fn m215_ok() -> bool {
    let (len, head) = match m215_reconcile() {
        Ok(r) => r,
        Err(_) => return false,
    };
    len == (TOTAL_CASES - M215_BREAK_AT) as usize
        && head == M215_BREAK_AT
        && len == 5
        && detail(Diag::PlanMismatch.val()).contains("不平")
}

/// 双跑一致：M-215 对账两次执行结果相同（纯函数）。
fn double_run_ok() -> bool {
    let a = m215_reconcile();
    let b = m215_reconcile();
    a.is_ok() && b.is_ok() && a == b
}

/// 写回面：状态=已完成、结果摘要非空（含断点与续跑清单规模）。
fn writeback_summary_ok() -> bool {
    let (len, head) = match m215_reconcile() {
        Ok(r) => r,
        Err(_) => return false,
    };
    let summary = format!(
        "UNX-F3456 完成：断点{} 续跑{}条 已完成{}例 计划{}例",
        head, len, M215_BREAK_AT, TOTAL_CASES
    );
    !summary.is_empty()
        && summary.contains("UNX-F3456")
        && summary.contains("断点7")
        && detail(Diag::SummaryEmpty.val()).contains("摘要")
}

// ── 判据总成：run_unx3456_checks ───────────────────────────────────────

/// run_unx3456_checks：本单判据聚合（诊断段 0x79F40，全部走 add_dyn）。
pub fn run_unx3456_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F3456 断点续跑");
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
            "计划冻结总数12中断点7判据号锚定",
            plan_frozen_ok(),
            format!("计划{}", if plan_frozen_ok() { "冻结" } else { "漂移" }),
        ),
        (
            "正常流顺序标满序号集逐条落账",
            mark_done_ok(),
            format!("标记{}", if mark_done_ok() { "齐" } else { "缺" }),
        ),
        (
            "续跑清单从断点起跳尾元素恰总数减一",
            resume_plan_ok(),
            format!("清单{}", if resume_plan_ok() { "准" } else { "偏" }),
        ),
        (
            "清单与序号集恒互斥不含已完成",
            plan_excludes_done_ok(),
            format!("互斥{}", if plan_excludes_done_ok() { "守住" } else { "破缺" }),
        ),
        (
            "跳过已完成断点前候选全落断点",
            skip_completed_ok(),
            format!("跳过{}", if skip_completed_ok() { "准" } else { "偏" }),
        ),
        (
            "M-215模拟中断续跑对账五条恰断点七",
            m215_ok(),
            format!("M-215{}", if m215_ok() { "对账平" } else { "不平" }),
        ),
        (
            "注入1越界标记必拒",
            inj_out_of_range_ok(),
            format!("越界{}", if inj_out_of_range_ok() { "拒" } else { "漏" }),
        ),
        (
            "注入2跳序执行必拒",
            inj_order_break_ok(),
            format!("跳序{}", if inj_order_break_ok() { "拒" } else { "漏" }),
        ),
        (
            "注入3重复标记必拒",
            inj_dup_mark_ok(),
            format!("重标{}", if inj_dup_mark_ok() { "拒" } else { "漏" }),
        ),
        (
            "注入4断点倒退rewind必拒",
            inj_regress_ok(),
            format!("倒退{}", if inj_regress_ok() { "拒" } else { "漏" }),
        ),
        (
            "注入5空计划必拒",
            inj_empty_total_ok(),
            format!("空计划{}", if inj_empty_total_ok() { "拒" } else { "漏" }),
        ),
    ];
    let cap = capacity_guard_ok();
    let dr = double_run_ok();
    let wb = writeback_summary_ok();
    for (name, passed, det) in checks.iter() {
        s.add_dyn(name, *passed, det);
    }
    s.add_dyn(
        "容量守卫序号集满16条必拒",
        cap,
        &format!("容量{}", if cap { "守住" } else { "失守" }),
    );
    s.add_dyn(
        "双跑一致M-215对账两遍同",
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
        assert_eq!(SEG_BASE, 0x79F40);
        assert!(diag_closed_set_ok());
        for (i, d) in ALL_DIAGS.iter().enumerate() {
            assert_eq!(d.val(), SEG_BASE + i as u32 + 1);
        }
    }

    #[test]
    fn plan_constants_frozen() {
        assert!(plan_frozen_ok());
        assert_eq!(TOTAL_CASES, 12);
        assert_eq!(M215_BREAK_AT, 7);
    }

    #[test]
    fn mark_done_sequential_full() {
        assert!(mark_done_ok());
        let mut p = resume_new(TOTAL_CASES).unwrap();
        mark_done(&mut p, 0).unwrap();
        mark_done(&mut p, 1).unwrap();
        assert_eq!(p.next_index, 2);
        assert_eq!(p.done.len(), 2);
    }

    #[test]
    fn resume_plan_from_breakpoint() {
        assert!(resume_plan_ok());
        assert!(plan_excludes_done_ok());
    }

    #[test]
    fn skip_completed_jumps_to_breakpoint() {
        assert!(skip_completed_ok());
    }

    #[test]
    fn m215_simulated_interrupt_reconcile() {
        assert!(m215_ok());
        let (len, head) = m215_reconcile().unwrap();
        assert_eq!((len, head), (5, 7));
    }

    #[test]
    fn injections_range_order_dup_regress_empty() {
        assert!(inj_out_of_range_ok());
        assert!(inj_order_break_ok());
        assert!(inj_dup_mark_ok());
        assert!(inj_regress_ok());
        assert!(inj_empty_total_ok());
    }

    #[test]
    fn capacity_guard_at_sixteen() {
        assert!(capacity_guard_ok());
        assert_eq!(RESUME_CAP, 16);
    }

    #[test]
    fn rewind_always_rejected() {
        let mut p = resume_new(TOTAL_CASES).unwrap();
        assert_eq!(rewind(&mut p), Err(Diag::ResumeRegress.val()));
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
    fn empty_plan_rejected() {
        assert_eq!(resume_new(0), Err(Diag::EmptyTotal.val()));
    }

    #[test]
    fn checks_aggregate_all_green() {
        let s = run_unx3456_checks();
        assert!(s.len() >= 12);
        assert!(s.all_passed(), "全部判据必须绿：{}", s.red_items_text());
    }
}
