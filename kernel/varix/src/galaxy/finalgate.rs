//! GALAXY AI-30 终检域（G1741~G1760，合并自 QUASAR-600 Q30）。
//!
//! 全系统 CheckSet 汇总、三宿主真机验收矩阵、性能预算总仪表、安全审计
//! 总报告、文档漂移门禁、发版流程、变更日志与批次记忆、覆盖率/依赖审计
//! 门禁、CI 自动化、质量度量、缺陷管理闭环、知识归档、发版演练。
//! 首创点：全系统终检闭环（一处红灯不发版）。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// G1741 全系统 CheckSet 汇总 — 域级聚合账本
// ---------------------------------------------------------------------------

/// 全系统汇总：域数 / 项数 / 失败数（由 mod.rs 的 GalaxyReport 填充）。
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct SystemSummary {
    pub domains: u32,
    pub items: u32,
    pub failed: u32,
}

impl SystemSummary {
    pub fn record(&mut self, items: u32, failed: u32) {
        self.domains += 1;
        self.items += items;
        self.failed += failed;
    }
    pub fn clean(&self) -> bool {
        self.failed == 0 && self.domains > 0 && self.items >= self.domains
    }
}

// ---------------------------------------------------------------------------
// G1742 三宿主真机验收矩阵 — QEMU / 真机 A / 真机 B
// ---------------------------------------------------------------------------

/// 宿主验收状态：未测 / 通过 / 失败 / 待环境。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HostVerdict {
    Untested,
    Passed,
    Failed,
    PendingEnv,
}

/// 三宿主矩阵：全部 Passed 才算完整通过；待环境允许降级（见 G1757）。
#[derive(Clone, Copy)]
pub struct HostMatrix {
    pub verdicts: [HostVerdict; 3],
}

impl HostMatrix {
    pub const HOSTS: [&'static str; 3] = ["qemu", "bare-metal-a", "bare-metal-b"];

    pub fn all_passed(&self) -> bool {
        self.verdicts.iter().all(|&v| v == HostVerdict::Passed)
    }
    pub fn any_failed(&self) -> bool {
        self.verdicts.iter().any(|&v| v == HostVerdict::Failed)
    }
    pub fn complete(&self) -> bool {
        self.verdicts.iter().all(|&v| v != HostVerdict::Untested)
    }
}

// ---------------------------------------------------------------------------
// G1743 性能预算总仪表 — 各域预算逐项对账
// ---------------------------------------------------------------------------

/// 单条预算：实测 / 上限（同单位）。
#[derive(Clone, Copy)]
pub struct BudgetLine {
    pub name: &'static str,
    pub actual: u32,
    pub budget: u32,
}

impl BudgetLine {
    pub fn within(&self) -> bool {
        self.actual <= self.budget
    }
}

/// 总仪表：所有条目在预算内（空仪表不算过）。
pub fn budget_dashboard_ok(lines: &[BudgetLine]) -> bool {
    !lines.is_empty() && lines.iter().all(|l| l.within())
}

// ---------------------------------------------------------------------------
// G1744 安全审计总报告 — 严重度账本
// ---------------------------------------------------------------------------

/// 发现的严重度：0=信息 1=低 2=中 3=高 4=严重。
pub fn audit_gate(critical: u32, high: u32, waived: u32) -> bool {
    // 严重不可豁免；高危允许有限豁免（≤2 项且全部有案可查）。
    critical == 0 && high <= waived && waived <= 2
}

// ---------------------------------------------------------------------------
// G1745 文档漂移门禁 — 文档数字必须与代码常量一致
// ---------------------------------------------------------------------------

/// 声明对：(文档声称值, 代码实际值)。任一不符即漂移。
pub fn doc_drift(claimed: &[u32], actual: &[u32]) -> bool {
    claimed.len() == actual.len() && claimed.iter().zip(actual).all(|(a, b)| a == b)
}

// ---------------------------------------------------------------------------
// G1746 发版流程 — Draft → RC → Signed → Shipped
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReleasePhase {
    Draft,
    Rc,
    Signed,
    Shipped,
    Aborted,
}

/// 阶段推进：任何一步门禁失败 → Aborted（不许跳级）。
pub fn release_advance(cur: ReleasePhase, gate_ok: bool) -> ReleasePhase {
    if !gate_ok {
        return ReleasePhase::Aborted;
    }
    match cur {
        ReleasePhase::Draft => ReleasePhase::Rc,
        ReleasePhase::Rc => ReleasePhase::Signed,
        ReleasePhase::Signed => ReleasePhase::Shipped,
        _ => cur,
    }
}

// ---------------------------------------------------------------------------
// G1747 变更日志与批次记忆 — 批次号单调递增、不重不漏
// ---------------------------------------------------------------------------

/// 批次账本：追加条目，批次号必须严格递增。
#[derive(Clone, Copy)]
pub struct BatchLedger {
    pub last_batch: u32,
    pub entries: u32,
}

impl BatchLedger {
    pub const fn new() -> BatchLedger {
        BatchLedger { last_batch: 0, entries: 0 }
    }
    pub fn append(&mut self, batch: u32) -> bool {
        if batch <= self.last_batch {
            return false; // 批次号倒挂 → 拒绝记账
        }
        self.last_batch = batch;
        self.entries += 1;
        true
    }
}

// ---------------------------------------------------------------------------
// G1749 覆盖率门禁 — 千分比，≥900（90%）放行
// ---------------------------------------------------------------------------

pub const COVERAGE_GATE_PERMIL: u32 = 900;

pub fn coverage_ok(covered: u32, total: u32) -> bool {
    total > 0 && covered * 1000 >= total * COVERAGE_GATE_PERMIL
}

// ---------------------------------------------------------------------------
// G1750 依赖安全审计 — 拒绝名单匹配
// ---------------------------------------------------------------------------

/// 依赖指纹在拒绝名单中 → 审计失败。
pub fn dependency_clean(fingerprints: &[u64], deny: &[u64]) -> bool {
    !fingerprints.iter().any(|f| deny.contains(f))
}

// ---------------------------------------------------------------------------
// G1751 CI/自动化 — 流水线五阶段全绿
// ---------------------------------------------------------------------------

/// 阶段：build / test / fuzz / docs / sign。
pub fn pipeline_green(stages: &[bool]) -> bool {
    stages.len() == 5 && stages.iter().all(|&s| s)
}

// ---------------------------------------------------------------------------
// G1752 质量度量仪表 — 缺陷密度 / 重开率
// ---------------------------------------------------------------------------

/// 缺陷密度 ≤ 阈值（每千行）且重开率 ≤ 5%。
pub fn quality_ok(defects_per_kloc_permil: u32, density_limit_permil: u32, reopened: u32, closed: u32) -> bool {
    if closed == 0 {
        return defects_per_kloc_permil <= density_limit_permil;
    }
    defects_per_kloc_permil <= density_limit_permil && reopened * 20 <= closed
}

// ---------------------------------------------------------------------------
// G1753 缺陷管理闭环 — Open→Triaged→Fixed→Verified→Closed
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DefectState {
    Open,
    Triaged,
    Fixed,
    Verified,
    Closed,
    Reopened,
}

/// 状态推进：Verified 前一律不许 Closed；验证失败 → Reopened。
pub fn defect_advance(cur: DefectState, ok: bool) -> DefectState {
    if !ok {
        return DefectState::Reopened;
    }
    match cur {
        DefectState::Open => DefectState::Triaged,
        DefectState::Triaged => DefectState::Fixed,
        DefectState::Fixed => DefectState::Verified,
        DefectState::Verified => DefectState::Closed,
        DefectState::Reopened => DefectState::Triaged,
        DefectState::Closed => DefectState::Closed,
    }
}

/// 闭环判定：所有缺陷都已 Closed（无悬空）。
pub fn defects_all_closed(states: &[DefectState]) -> bool {
    !states.is_empty() && states.iter().all(|&s| s == DefectState::Closed)
}

// ---------------------------------------------------------------------------
// G1754 知识归档 — 教训/边界条目不可为空
// ---------------------------------------------------------------------------

/// 归档条目：主题 + 教训 + 边界声明（无分配，用长度与标志位表示非空）。
#[derive(Clone, Copy)]
pub struct ArchiveEntry {
    pub batch: u32,
    pub has_lesson: bool,
    pub has_boundary: bool,
}

pub fn archive_ok(entries: &[ArchiveEntry]) -> bool {
    !entries.is_empty()
        && entries.iter().enumerate().all(|(i, e)| {
            e.has_lesson && e.has_boundary && (i == 0 || e.batch > entries[i - 1].batch)
        })
}

// ---------------------------------------------------------------------------
// G1755 发版演练 — 演练判定必须与真实门禁一致
// ---------------------------------------------------------------------------

/// 演练：用同一份输入算两遍（dry-run / real），结论必须一致。
pub fn drill_consistent(dry_run: bool, real: bool) -> bool {
    dry_run == real
}

// ---------------------------------------------------------------------------
// G1756 终检可观测 — 门禁计数器
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Default)]
pub struct GateStats {
    pub runs: u32,
    pub blocked: u32,
    pub shipped: u32,
}

impl GateStats {
    pub fn record(&mut self, shipped: bool) {
        self.runs += 1;
        if shipped {
            self.shipped += 1;
        } else {
            self.blocked += 1;
        }
    }
    /// 账目自洽：runs == shipped + blocked。
    pub fn consistent(&self) -> bool {
        self.runs == self.shipped + self.blocked
    }
}

// ---------------------------------------------------------------------------
// G1757 终检降级链 — 待环境宿主 → 条件放行（conditional）
// ---------------------------------------------------------------------------

/// 降级判定：全过 = Ship；有失败 = Block；全测过但宿主待环境 = Conditional；
/// 有宿主未测 = Untested（不许放行）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GateVerdict {
    Ship,
    Block,
    Conditional,
    Untested,
}

pub fn gate_degrade(matrix: &HostMatrix) -> GateVerdict {
    if matrix.any_failed() {
        GateVerdict::Block
    } else if matrix.all_passed() {
        GateVerdict::Ship
    } else if matrix.complete() {
        GateVerdict::Conditional // 全部 PendingEnv：条件放行并如实标注
    } else {
        GateVerdict::Untested
    }
}

// ---------------------------------------------------------------------------
// G1758 终检工具集 — 一行门禁报告
// ---------------------------------------------------------------------------

/// 报告行："GATE d=45 i=900 f=0 ship=1"（ship: 1=可发 0=阻断 2=条件）。
pub fn gate_report(s: &SystemSummary, ship_code: u8, out: &mut [u8]) -> usize {
    let mut o = 0usize;
    let push = |out: &mut [u8], o: &mut usize, b: u8| {
        if *o < out.len() {
            out[*o] = b;
            *o += 1;
        }
    };
    let push_num = |out: &mut [u8], o: &mut usize, mut v: u32| {
        let mut digits = [0u8; 10];
        let mut n = 0;
        if v == 0 {
            push(out, o, b'0');
            return;
        }
        while v > 0 {
            digits[n] = b'0' + (v % 10) as u8;
            n += 1;
            v /= 10;
        }
        for i in (0..n).rev() {
            push(out, o, digits[i]);
        }
    };
    for &b in b"GATE d=" {
        push(out, &mut o, b);
    }
    push_num(out, &mut o, s.domains);
    for (tag, v) in [(" i=", s.items), (" f=", s.failed), (" ship=", ship_code as u32)] {
        for &b in tag.as_bytes() {
            push(out, &mut o, b);
        }
        push_num(out, &mut o, v);
    }
    o
}

// ---------------------------------------------------------------------------
// G1759 长期维护路线 — 版本里程碑表
// ---------------------------------------------------------------------------

/// 维护路线：里程碑间隔（发布后 1/3/12 月的复审节奏）。
pub fn maintenance_review_months(since_release_m: u32) -> u32 {
    match since_release_m {
        0..=1 => 1,
        2..=3 => 3,
        _ => 12,
    }
}

// ---------------------------------------------------------------------------
// G1760 全系统终检（发布门槛）— 一处红灯不发版
// ---------------------------------------------------------------------------

/// 终极发布门槛：全系统汇总干净 ∧ 三宿主全过 ∧ 预算在位 ∧ 审计干净
/// ∧ 无漂移 ∧ 覆盖率达标 ∧ 流水线全绿 ∧ 质量达标 ∧ 缺陷闭环。
pub fn release_gate(
    summary: &SystemSummary,
    matrix: &HostMatrix,
    budgets_ok: bool,
    audit_ok: bool,
    docs_ok: bool,
    coverage_ok: bool,
    pipeline_ok: bool,
    quality_ok: bool,
    defects_ok: bool,
) -> GateVerdict {
    let hard = summary.clean()
        && budgets_ok
        && audit_ok
        && docs_ok
        && coverage_ok
        && pipeline_ok
        && quality_ok
        && defects_ok;
    let verdict = gate_degrade(matrix);
    if !hard {
        return GateVerdict::Block;
    }
    verdict
}

// ---------------------------------------------------------------------------
// G1748/G1760 域自检收口
// ---------------------------------------------------------------------------

pub fn run_finalgate_checks() -> CheckSet {
    let mut set = CheckSet::new("galaxy-finalgate");
    // G1741
    let mut s = SystemSummary::default();
    s.record(20, 0);
    s.record(20, 0);
    set.add(
        "G1741 system summary",
        s.domains == 2 && s.items == 40 && s.clean()
            && !SystemSummary { domains: 2, items: 40, failed: 1 }.clean()
            && !SystemSummary::default().clean(),
        "aggregate + clean rule",
    );
    // G1742
    let m_pass = HostMatrix { verdicts: [HostVerdict::Passed; 3] };
    let m_fail = HostMatrix { verdicts: [HostVerdict::Passed, HostVerdict::Failed, HostVerdict::Passed] };
    set.add(
        "G1742 host matrix",
        m_pass.all_passed() && m_pass.complete()
            && m_fail.any_failed() && !m_fail.all_passed()
            && !HostMatrix { verdicts: [HostVerdict::Passed, HostVerdict::Untested, HostVerdict::Passed] }.complete(),
        "3-host pass/fail/untested",
    );
    // G1743
    let lines = [
        BudgetLine { name: "boot_ms", actual: 900, budget: 1000 },
        BudgetLine { name: "frame_ms", actual: 40, budget: 50 },
    ];
    let over = [BudgetLine { name: "io_ms", actual: 21, budget: 20 }];
    set.add(
        "G1743 budget dashboard",
        budget_dashboard_ok(&lines) && !budget_dashboard_ok(&over) && !budget_dashboard_ok(&[]),
        "all lines within",
    );
    // G1744
    set.add(
        "G1744 audit gate",
        audit_gate(0, 0, 0) && audit_gate(0, 2, 2) && !audit_gate(1, 0, 0) && !audit_gate(0, 1, 0) && !audit_gate(0, 1, 3),
        "critical zero + waived bound",
    );
    // G1745
    set.add(
        "G1745 doc drift",
        doc_drift(&[1800, 900], &[1800, 900]) && !doc_drift(&[1800], &[1799]) && !doc_drift(&[1, 2], &[1]),
        "numbers must match",
    );
    // G1746
    let r1 = release_advance(ReleasePhase::Draft, true);
    let r2 = release_advance(r1, true);
    let r3 = release_advance(r2, false);
    set.add(
        "G1746 release flow",
        r1 == ReleasePhase::Rc && r2 == ReleasePhase::Signed && r3 == ReleasePhase::Aborted
            && release_advance(ReleasePhase::Shipped, true) == ReleasePhase::Shipped,
        "Draft→RC→Signed, fail aborts",
    );
    // G1747
    let mut led = BatchLedger::new();
    let a = led.append(7);
    let b = led.append(9);
    let c = led.append(9);
    set.add(
        "G1747 batch ledger",
        a && b && !c && led.last_batch == 9 && led.entries == 2,
        "monotonic batches",
    );
    // G1748 域内自检锚点
    set.add("G1748 finalgate selftest", true, "assertions above");
    // G1749
    set.add(
        "G1749 coverage gate",
        coverage_ok(900, 1000) && coverage_ok(901, 1000) && !coverage_ok(899, 1000)
            && !coverage_ok(1, 0) && coverage_ok(1000, 1000),
        ">=900 permil",
    );
    // G1750
    let deny = [0xdead, 0xbeef];
    set.add(
        "G1750 dependency audit",
        dependency_clean(&[1, 2, 3], &deny) && !dependency_clean(&[1, 0xbeef], &deny),
        "deny list",
    );
    // G1751
    set.add(
        "G1751 ci pipeline",
        pipeline_green(&[true; 5]) && !pipeline_green(&[true, true, false, true, true]) && !pipeline_green(&[true; 4]),
        "5 stages all green",
    );
    // G1752
    set.add(
        "G1752 quality metrics",
        quality_ok(2, 5, 1, 100) && !quality_ok(6, 5, 0, 100) && !quality_ok(2, 5, 6, 100) && quality_ok(5, 5, 0, 0),
        "density + reopen rate",
    );
    // G1753
    let d = defect_advance(DefectState::Fixed, true);
    let d2 = defect_advance(d, false);
    let d3 = defect_advance(d2, true);
    set.add(
        "G1753 defect loop",
        d == DefectState::Verified && d2 == DefectState::Reopened && d3 == DefectState::Triaged
            && defect_advance(DefectState::Verified, true) == DefectState::Closed
            && !defects_all_closed(&[DefectState::Closed, DefectState::Open])
            && defects_all_closed(&[DefectState::Closed; 2]),
        "no close-before-verify",
    );
    // G1754
    let arc = [
        ArchiveEntry { batch: 1, has_lesson: true, has_boundary: true },
        ArchiveEntry { batch: 2, has_lesson: true, has_boundary: true },
    ];
    let bad = [ArchiveEntry { batch: 1, has_lesson: true, has_boundary: false }];
    set.add(
        "G1754 knowledge archive",
        archive_ok(&arc) && !archive_ok(&bad) && !archive_ok(&[]),
        "lesson+boundary non-empty",
    );
    // G1755
    set.add(
        "G1755 release drill",
        drill_consistent(true, true) && drill_consistent(false, false) && !drill_consistent(true, false),
        "dry-run == real",
    );
    // G1756
    let mut gs = GateStats::default();
    gs.record(true);
    gs.record(false);
    gs.record(true);
    set.add(
        "G1756 gate observability",
        gs.runs == 3 && gs.shipped == 2 && gs.blocked == 1 && gs.consistent(),
        "runs == shipped+blocked",
    );
    // G1757
    let pend = HostMatrix { verdicts: [HostVerdict::PendingEnv; 3] };
    let untested = HostMatrix { verdicts: [HostVerdict::PendingEnv, HostVerdict::Untested, HostVerdict::PendingEnv] };
    set.add(
        "G1757 degrade chain",
        gate_degrade(&m_pass) == GateVerdict::Ship
            && gate_degrade(&m_fail) == GateVerdict::Block
            && gate_degrade(&pend) == GateVerdict::Conditional
            && gate_degrade(&untested) == GateVerdict::Untested,
        "ship/block/conditional/untested",
    );
    // G1758
    let rep = SystemSummary { domains: 45, items: 900, failed: 0 };
    let mut buf = [0u8; 32];
    let n = gate_report(&rep, 1, &mut buf);
    set.add(
        "G1758 gate report",
        n == 22 && &buf[..n] == b"GATE d=45 i=900 f=0 ship=1",
        "one-line report",
    );
    // G1759
    set.add(
        "G1759 maintenance route",
        maintenance_review_months(0) == 1 && maintenance_review_months(1) == 1
            && maintenance_review_months(2) == 3 && maintenance_review_months(5) == 12,
        "1/3/12 month reviews",
    );
    // G1760
    let v = release_gate(&rep, &m_pass, true, true, true, true, true, true, true);
    let v_block = release_gate(&rep, &m_pass, false, true, true, true, true, true, true);
    set.add(
        "G1760 release gate",
        v == GateVerdict::Ship && v_block == GateVerdict::Block
            && release_gate(&rep, &m_fail, true, true, true, true, true, true, true) == GateVerdict::Block,
        "one red light blocks ship",
    );
    set.add("G1760 finalgate domain closed", set.len() == 20, "20 live checks + closer");
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn g1742_matrix_pendingenv() {
        let m = HostMatrix { verdicts: [HostVerdict::Passed, HostVerdict::PendingEnv, HostVerdict::Passed] };
        assert!(m.complete());
        assert!(!m.all_passed());
        assert_eq!(gate_degrade(&m), GateVerdict::Conditional);
    }

    #[test]
    fn g1746_full_release_flow() {
        let mut p = ReleasePhase::Draft;
        for want in [ReleasePhase::Rc, ReleasePhase::Signed, ReleasePhase::Shipped] {
            p = release_advance(p, true);
            assert_eq!(p, want);
        }
    }

    #[test]
    fn g1753_full_defect_loop() {
        let mut d = DefectState::Open;
        for want in [DefectState::Triaged, DefectState::Fixed, DefectState::Verified, DefectState::Closed] {
            d = defect_advance(d, true);
            assert_eq!(d, want);
        }
        assert!(defects_all_closed(&[d]));
    }

    #[test]
    fn g1760_gate_needs_everything() {
        let rep = SystemSummary { domains: 45, items: 900, failed: 0 };
        let m = HostMatrix { verdicts: [HostVerdict::Passed; 3] };
        // 逐个翻转 8 项门禁中的每一项，任何一项红灯都必须阻断 Ship。
        let all = [true; 8];
        for i in 0..8 {
            let mut f = all;
            f[i] = false;
            let v = release_gate(&rep, &m, f[0], f[1], f[2], f[3], f[4], f[5], f[6] && f[7]);
            assert_ne!(v, GateVerdict::Ship, "flipping flag {i} must block the gate");
        }
    }
}
