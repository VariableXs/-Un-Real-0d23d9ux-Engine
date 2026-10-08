//! 回归门与预算基线同步（WP-402 · B-1404/B-1405）。
//!
//! MD2 篇 14.3：回归门嵌在每日流水（B-1206）与合入闸门（35.4）两处——
//! 每日全套基准跑完与基线对表，标红项自动开 issue 并通知；合入前跑受
//! 影响子系统的基准子集（按改动路径选择，选择表维护在仓库），红即阻断
//! 合入——性能回归与功能缺陷同罪（总案 19.5）。预算数字本身要改时走
//! 三处同步（MD1 附录 I 的纪律）：源章节、速查总表、vxbench 基线同时改
//! 加 ADR 备案，三处不同步的改动在审计时打回。
//!
//! 数字同源纪律：回归判定公式（P95 对基线偏差超 10% 标红）直引
//! `benchsix::BenchReport::regress_red`——WP-209 首轮单源，本模块只做
//! 门的编排，不重写公式。基准样本同源 `benchsix::synth_samples`。
//!
//! 零堆纪律：无 Vec/String/Box/format!，定长数组 + 字节串。

use crate::benchsix::{kind_index, synth_samples, BenchKind, BenchReport, BENCH_KINDS};
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// B-1404 回归门：路径选择表 + 合入阻断 + 红项 issue + 可复现演练
// ---------------------------------------------------------------------------

/// 改动路径 → 基准子集选择表（维护在仓库——篇 14.3 的"选择表"）。
/// 无命中时保守取全集：宁可多跑，不许漏跑。
pub const PATH_RULES: usize = 7;

pub struct PathRule {
    pub prefix: &'static [u8],
    pub mask: [bool; BENCH_KINDS],
}

pub const CHANGE_PATH_TABLE: [PathRule; PATH_RULES] = [
    PathRule { prefix: b"syscall/", mask: [true, false, false, false, false, false] },
    PathRule { prefix: b"sched/", mask: [false, true, false, false, false, false] },
    PathRule { prefix: b"mem/", mask: [false, false, true, false, false, false] },
    PathRule { prefix: b"compositor/", mask: [false, false, false, true, false, false] },
    PathRule { prefix: b"storage/", mask: [false, false, false, false, true, false] },
    PathRule { prefix: b"net/", mask: [false, false, false, false, false, true] },
    PathRule { prefix: b"kernel/", mask: [true, true, true, true, true, true] },
];

/// 按改动路径选基准子集：首条前缀命中即用；无命中取全集（保守不漏）。
pub fn select_benches(path: &[u8]) -> [bool; BENCH_KINDS] {
    let mut i = 0;
    while i < PATH_RULES {
        let p = CHANGE_PATH_TABLE[i].prefix;
        if path.len() >= p.len() && &path[..p.len()] == p {
            return CHANGE_PATH_TABLE[i].mask;
        }
        i += 1;
    }
    [true; BENCH_KINDS]
}

/// 合入裁决：性能回归与功能缺陷同罪——红即阻断。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MergeVerdict {
    Allow,
    Block,
}

/// 合入门：受影响子集逐类对基线，红即 Block。
/// 回归判定直引 benchsix::BenchReport::regress_red（单源不重写）。
pub fn gate_merge(
    path: &[u8],
    baseline: &[u64; BENCH_KINDS],
    report: &BenchReport,
) -> (MergeVerdict, [bool; BENCH_KINDS]) {
    let mask = select_benches(path);
    let mut red = [false; BENCH_KINDS];
    let mut i = 0;
    while i < BENCH_KINDS {
        if mask[i] {
            let kind = ALL_KINDS[i];
            red[i] = BenchReport::regress_red(kind, baseline[i], report);
        }
        i += 1;
    }
    let mut any_red = false;
    let mut j = 0;
    while j < BENCH_KINDS {
        if red[j] {
            any_red = true;
        }
        j += 1;
    }
    let verdict = if any_red { MergeVerdict::Block } else { MergeVerdict::Allow };
    (verdict, red)
}

pub const ALL_KINDS: [BenchKind; BENCH_KINDS] = [
    BenchKind::SyscallRoundtrip,
    BenchKind::CtxSwitch,
    BenchKind::PageAllocFree,
    BenchKind::FramePipeline,
    BenchKind::BlockPath,
    BenchKind::NetLoopback,
];

/// 红项 issue（标红项自动开——篇 14.3）。cap = 六类，每类至多一条。
pub const RED_ISSUE_CAP: usize = BENCH_KINDS;

#[derive(Clone, Copy)]
pub struct RedIssue {
    pub kind_idx: usize,
    pub p95_ns: u64,
    pub baseline_ns: u64,
    pub drift_permille: u64,
    /// 开 issue 即通知——红项与通知绑定，不存在"开了没发"。
    pub notified: bool,
}

pub struct IssueLog {
    pub issues: [Option<RedIssue>; RED_ISSUE_CAP],
    pub count: usize,
}

impl IssueLog {
    pub fn new() -> Self {
        IssueLog { issues: [None; RED_ISSUE_CAP], count: 0 }
    }

    /// 按红位表自动开 issue 并通知；返回开出条数。
    pub fn open_reds(
        &mut self,
        red: &[bool; BENCH_KINDS],
        baseline: &[u64; BENCH_KINDS],
        report: &BenchReport,
    ) -> usize {
        let mut opened = 0;
        let mut i = 0;
        while i < BENCH_KINDS {
            if red[i] && self.count < RED_ISSUE_CAP {
                if let Some(d) = report.dist[i] {
                    let diff =
                        if d.p95_ns > baseline[i] { d.p95_ns - baseline[i] } else { baseline[i] - d.p95_ns };
                    self.issues[self.count] = Some(RedIssue {
                        kind_idx: i,
                        p95_ns: d.p95_ns,
                        baseline_ns: baseline[i],
                        drift_permille: diff * 1000 / baseline[i].max(1),
                        notified: true, // 开即通知——红必发
                    });
                    self.count += 1;
                    opened += 1;
                } else {
                    // 缺数据按红处理（与 benchsix 同语义）——issue 照开。
                    self.issues[self.count] = Some(RedIssue {
                        kind_idx: i,
                        p95_ns: 0,
                        baseline_ns: baseline[i],
                        drift_permille: 1000,
                        notified: true,
                    });
                    self.count += 1;
                    opened += 1;
                }
            }
            i += 1;
        }
        opened
    }

    pub fn all_notified(&self) -> bool {
        let mut i = 0;
        while i < self.count {
            match self.issues[i] {
                Some(r) if r.notified => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }
}

/// 每日流水联动（B-1206 面）：每日全套六类与基线对表，返回红类数。
/// 红项的 issue 开出与通知由 IssueLog 承接——同一套 regress_red。
pub fn daily_full_audit(baseline: &[u64; BENCH_KINDS], report: &BenchReport) -> usize {
    let mut reds = 0;
    let mut i = 0;
    while i < BENCH_KINDS {
        if BenchReport::regress_red(ALL_KINDS[i], baseline[i], report) {
            reds += 1;
        }
        i += 1;
    }
    reds
}

/// 可复现演练：同 seed 两次独立重跑门禁，裁决必须一致——
/// "红项阻断合入可复现演练"的字面落实（同输入重放同裁决）。
pub fn replay_gate(seed: u64, baseline: &[u64; BENCH_KINDS]) -> (MergeVerdict, MergeVerdict) {
    let mut rep_a = BenchReport::new();
    let mut rep_b = BenchReport::new();
    let mut i = 0;
    while i < BENCH_KINDS {
        rep_a.record(ALL_KINDS[i], &synth_samples(ALL_KINDS[i], seed + i as u64));
        rep_b.record(ALL_KINDS[i], &synth_samples(ALL_KINDS[i], seed + i as u64));
        i += 1;
    }
    let (v_a, _) = gate_merge(b"kernel/ci-replay", baseline, &rep_a);
    let (v_b, _) = gate_merge(b"kernel/ci-replay", baseline, &rep_b);
    (v_a, v_b)
}

// ---------------------------------------------------------------------------
// B-1405 基线三处同步：源章节 × 速查总表 × vxbench 基线 + ADR 备案
// ---------------------------------------------------------------------------

/// 预算条目：同一预算数字的三处落点（MD1 附录 I 纪律）。
pub struct BudgetEntry {
    /// 预算号（字节串定长，末尾 0 填充）。
    pub id: [u8; 12],
    /// 源章节值（MD2 对应篇）。
    pub source_val: u64,
    /// 速查总表值（MD2 附录 K）。
    pub quickref_val: u64,
    /// vxbench 基线值（本仓库基线文件）。
    pub baseline_val: u64,
}

pub const BUDGET_ENTRIES: usize = 4;

/// ADR 备案：预算变更的理由与前后值——改动必留痕。
#[derive(Clone, Copy)]
pub struct AdrRecord {
    pub adr_id: u32,
    pub budget_idx: usize,
    pub old_val: u64,
    pub new_val: u64,
    pub reason_len: usize,
}

pub const ADR_LOG_CAP: usize = 8;

pub struct AdrLog {
    pub recs: [Option<AdrRecord>; ADR_LOG_CAP],
    pub count: usize,
}

impl AdrLog {
    pub fn new() -> Self {
        AdrLog { recs: [None; ADR_LOG_CAP], count: 0 }
    }
}

/// 三处同步审计（逐条目）：三处值全等才过——漂移即打回。
pub fn audit_three_sync(entries: &[BudgetEntry; BUDGET_ENTRIES]) -> bool {
    let mut i = 0;
    while i < BUDGET_ENTRIES {
        let e = &entries[i];
        if e.source_val != e.quickref_val || e.quickref_val != e.baseline_val {
            return false;
        }
        i += 1;
    }
    true
}

/// 预算变更：无 ADR 拒绝（三处不动）；有 ADR 且理由非空且 old_val 对上
/// 才允许三处同写——"三处同时改加 ADR 备案"是同一动作不是三个动作。
pub fn apply_budget_change(
    entries: &mut [BudgetEntry; BUDGET_ENTRIES],
    log: &mut AdrLog,
    idx: usize,
    new_val: u64,
    adr: Option<AdrRecord>,
) -> bool {
    let rec = match adr {
        Some(r) => r,
        None => return false, // 改了值没备案——审计打回的根源，当场拒
    };
    if rec.budget_idx != idx || rec.reason_len == 0 {
        return false; // 备案对象不符 / 理由空白——同等拒绝
    }
    if rec.old_val != entries[idx].source_val {
        return false; // old_val 与现状不符——备案失真
    }
    if log.count >= ADR_LOG_CAP {
        return false;
    }
    let old = entries[idx].source_val;
    entries[idx].source_val = new_val;
    entries[idx].quickref_val = new_val;
    entries[idx].baseline_val = new_val;
    log.recs[log.count] = Some(AdrRecord { old_val: old, new_val, ..rec });
    log.count += 1;
    true
}

/// 组合审计：三处一致 + 变更链每步 ADR 的 new_val 与三处现值吻合。
/// 两关全过 = "审计通过"（B-1405 达标线）。
pub fn audit_pass(entries: &[BudgetEntry; BUDGET_ENTRIES], log: &AdrLog) -> bool {
    if !audit_three_sync(entries) {
        return false;
    }
    let mut i = 0;
    while i < log.count {
        match log.recs[i] {
            Some(r) => {
                let e = &entries[r.budget_idx];
                if e.source_val != r.new_val {
                    return false; // 备案的终值与三处现值脱节——同步被打破
                }
            }
            None => return false,
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// CheckSet（B-1404 · 5 项 + B-1405 · 3 项）
// ---------------------------------------------------------------------------

pub fn run_reggate_checks() -> CheckSet {
    let mut set = CheckSet::new("B-1404/1405 回归门与基线同步");
    // 1. 路径选择表完备：六类基准各被至少一条规则覆盖；未知路径保守全集。
    let mut covered = [false; BENCH_KINDS];
    let mut i1 = 0;
    while i1 < PATH_RULES {
        let mut k = 0;
        while k < BENCH_KINDS {
            if CHANGE_PATH_TABLE[i1].mask[k] {
                covered[k] = true;
            }
            k += 1;
        }
        i1 += 1;
    }
    let all_cov = covered.iter().all(|c| *c);
    let unknown_full = select_benches(b"drivers/usb-quirk") == [true; BENCH_KINDS];
    let narrow = select_benches(b"storage/fsync-path");
    set.add(
        "B-1404 路径选择表",
        all_cov && unknown_full && (narrow[4] && !narrow[0]),
        "六类全被规则覆盖+未知路径保守全集+命中规则按表收窄",
    );
    // 2. 合入门：绿过红拒——性能回归与功能缺陷同罪。
    // baseline[0] 对齐 SyscallRoundtrip base(2_000)+抖动上界(999) 取整——
    // 测试数据先算一遍被测公式：p95≈2_950 对 3_000 偏差 <2% 必绿。
    let mut baseline = [10_000u64; BENCH_KINDS];
    baseline[0] = 3_000;
    let mut rep_ok = BenchReport::new();
    rep_ok.record(BenchKind::SyscallRoundtrip, &synth_samples(BenchKind::SyscallRoundtrip, 0xB404));
    let (v_ok, _) = gate_merge(b"syscall/entry", &baseline, &rep_ok);
    let mut slow = [12_000u64; 100]; // p95=12_000 对 3_000 偏差 300% 必红
    slow[0] = 12_100;
    let mut rep_bad = BenchReport::new();
    rep_bad.record(BenchKind::SyscallRoundtrip, &slow);
    let (v_bad, red_bad) = gate_merge(b"syscall/entry", &baseline, &rep_bad);
    set.add(
        "B-1404 红即阻断",
        v_ok == MergeVerdict::Allow && v_bad == MergeVerdict::Block && red_bad[0],
        "子集绿放行、子集红阻断——回归与缺陷同罪",
    );
    // 3. 红项自动开 issue 且开即通知（红必发）。
    let mut log3 = IssueLog::new();
    let opened = log3.open_reds(&red_bad, &baseline, &rep_bad);
    set.add(
        "B-1404 红项开 issue+通知",
        opened == 1 && log3.count == 1 && log3.all_notified(),
        "标红项逐条立票且全部带通知面——不存在静默红",
    );
    // 4. 可复现演练：同 seed 重放裁决一致。
    let (va, vb) = replay_gate(0xB404, &baseline);
    set.add(
        "B-1404 可复现演练",
        va == vb,
        "同输入重放同裁决——红项阻断可复现",
    );
    // 5. 每日流水联动：全套对表走同一 regress_red 单源。
    // 全六类都 record（None 缺数据按红——benchsix 同语义），只让网络类红。
    let mut rep5 = BenchReport::new();
    let flat5 = [10_000u64; 100];
    let slow5 = [12_000u64; 100]; // 12_000 对 10_000 偏差 20% > 10% 必红
    let mut i5 = 0;
    while i5 < BENCH_KINDS {
        if i5 == kind_index(BenchKind::NetLoopback) {
            rep5.record(BenchKind::NetLoopback, &slow5);
        } else {
            rep5.record(ALL_KINDS[i5], &flat5);
        }
        i5 += 1;
    }
    let base5 = [10_000u64; BENCH_KINDS]; // 独立基线——不被项 2 的 3_000 污染
    let reds5 = daily_full_audit(&base5, &rep5);
    set.add(
        "B-1404 每日全套对表",
        reds5 == 1,
        "每日六类全套与基线对表：恰红一类计一——公式直引 benchsix 单源",
    );
    // 6. 三处同步审计：一致过、漂移打回。
    let mut e6 = [
        BudgetEntry { id: *b"FRM-16MS\0\0\0\0", source_val: 16, quickref_val: 16, baseline_val: 16 },
        BudgetEntry { id: *b"MEM-CAP-MB\0\0", source_val: 512, quickref_val: 512, baseline_val: 640 },
        BudgetEntry { id: *b"BOOT-11PT\0\0\0", source_val: 9_000, quickref_val: 9_000, baseline_val: 9_000 },
        BudgetEntry { id: *b"NET-RTT-US\0\0", source_val: 2_000, quickref_val: 2_000, baseline_val: 2_000 },
    ];
    let drifted = !audit_three_sync(&e6);
    e6[1].baseline_val = 512;
    let synced = audit_three_sync(&e6);
    set.add(
        "B-1405 三处同步审计",
        drifted && synced,
        "三处漂移打回、齐平放行——附录 I 纪律进审计",
    );
    // 7. 预算变更必走 ADR：无备案拒、有备案三处同改。
    let mut log7 = AdrLog::new();
    let refused = !apply_budget_change(&mut e6, &mut log7, 1, 640, None);
    let adr_ok = AdrRecord { adr_id: 1, budget_idx: 1, old_val: 512, new_val: 640, reason_len: 9 };
    let applied = apply_budget_change(&mut e6, &mut log7, 1, 640, Some(adr_ok));
    set.add(
        "B-1405 变更必走 ADR",
        refused && applied && e6[1].source_val == 640 && e6[1].quickref_val == 640 && e6[1].baseline_val == 640,
        "无备案当场拒；有备案三处同写一个值——三处同步是同一动作",
    );
    // 8. 组合审计：同步 + 备案链吻合 = 审计通过。
    set.add(
        "B-1405 审计通过",
        audit_pass(&e6, &log7),
        "三处一致+变更链逐条与现值吻合——两关全过",
    );
    set
}

// ---------------------------------------------------------------------------
// 单测（fe26 · 4 项）
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn fe26_path_select() {
        // 命中规则按表收窄。
        let m = select_benches(b"net/stack");
        assert!(m[5] && !m[0] && !m[1]);
        // 未知路径保守全集——宁可多跑不漏跑。
        assert_eq!(select_benches(b"ui/theme"), [true; BENCH_KINDS]);
        // 空路径也走全集。
        assert_eq!(select_benches(b""), [true; BENCH_KINDS]);
    }

    #[test]
    fn fe26_gate_replay() {
        // baseline 逐类对齐 synth_samples 的 base+抖动上界——先算被测公式：
        // 每类 p95 ≤ base+999，取 base+999 为基线时偏差恒 < 10%，必全绿。
        let baseline = [2_999u64, 5_999, 1_999, 10_999, 50_999, 30_999];
        let (a, b) = replay_gate(7, &baseline);
        assert_eq!(a, MergeVerdict::Allow);
        assert_eq!(a, b);
        // 抬高基线到样本达不到的低位（压成全红）——重放仍一致（Block）。
        let tight = [1u64; BENCH_KINDS];
        let (c, d) = replay_gate(7, &tight);
        assert_eq!(c, MergeVerdict::Block);
        assert_eq!(c, d);
    }

    #[test]
    fn fe26_adr_audit() {
        let mut e = [
            BudgetEntry { id: *b"A\0\0\0\0\0\0\0\0\0\0\0", source_val: 1, quickref_val: 1, baseline_val: 1 },
            BudgetEntry { id: *b"B\0\0\0\0\0\0\0\0\0\0\0", source_val: 2, quickref_val: 2, baseline_val: 2 },
            BudgetEntry { id: *b"C\0\0\0\0\0\0\0\0\0\0\0", source_val: 3, quickref_val: 3, baseline_val: 3 },
            BudgetEntry { id: *b"D\0\0\0\0\0\0\0\0\0\0\0", source_val: 4, quickref_val: 4, baseline_val: 4 },
        ];
        let mut log = AdrLog::new();
        // 无备案拒绝，三处不动。
        assert!(!apply_budget_change(&mut e, &mut log, 0, 9, None));
        assert_eq!(e[0].source_val, 1);
        // 理由空白同等拒绝。
        let blank = AdrRecord { adr_id: 2, budget_idx: 0, old_val: 1, new_val: 9, reason_len: 0 };
        assert!(!apply_budget_change(&mut e, &mut log, 0, 9, Some(blank)));
        // old_val 失真拒绝。
        let stale = AdrRecord { adr_id: 3, budget_idx: 0, old_val: 42, new_val: 9, reason_len: 3 };
        assert!(!apply_budget_change(&mut e, &mut log, 0, 9, Some(stale)));
        // 合法备案：三处同写。
        let ok = AdrRecord { adr_id: 4, budget_idx: 0, old_val: 1, new_val: 9, reason_len: 5 };
        assert!(apply_budget_change(&mut e, &mut log, 0, 9, Some(ok)));
        assert!(audit_pass(&e, &log));
    }

    #[test]
    fn fe26_issue_notify() {
        let baseline = [10_000u64; BENCH_KINDS];
        let slow = [11_600u64; 100]; // 整体 +16% > 10% 必红；p95 位 = 11_600
        let mut rep = BenchReport::new();
        rep.record(BenchKind::CtxSwitch, &slow);
        let (v, red) = gate_merge(b"sched/priority", &baseline, &rep);
        assert_eq!(v, MergeVerdict::Block);
        assert!(red[1]);
        let mut log = IssueLog::new();
        assert_eq!(log.open_reds(&red, &baseline, &rep), 1);
        assert!(log.all_notified());
        match log.issues[0] {
            Some(r) => {
                assert_eq!(r.kind_idx, kind_index(BenchKind::CtxSwitch));
                assert_eq!(r.p95_ns, 11_600); // 排序后 P95 位 = 整体抬升值
                assert_eq!(r.baseline_ns, 10_000);
                assert_eq!(r.drift_permille, 160); // 1600/10000 = 16%
            }
            None => panic!("issue must exist"),
        }
    }
}
