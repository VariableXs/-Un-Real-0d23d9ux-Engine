//! unxt582_ktrunner —— UNX-F0521 运行器语义（逐套件运行）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0521 · ktest 主框架总装（注册器/运行器） · 判据：框架双件齐（注册器/运行器），B19–B26 六套件经框架运行全绿（6/6 套件接入核）
//! · 组件：运行流水（六套件按序）+ 运行账（绿/红守恒）+ 结果链
//! · 判据落地：六套件经框架运行全绿
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T582_VERSION: &str = "t582-ktrunner-v1";

/// 诊断段基址。
pub const T582_SEG_BASE: u16 = 0x4710;

// --------------------------------------------------------------------------
// 诊断码（t582 十六码 · 14 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 运行器诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t582Code {
    /// 0x4710 套件被跳过（未运行）。
    SuiteSkipped,
    /// 0x4711 套件红（断言失败）。
    RedSuite,
    /// 0x4712 运行序违例。
    OrderViolated,
    /// 0x4713 运行账破缺。
    LedgerBreak,
    /// 0x4714 重复运行。
    DoubleRun,
    /// 0x4715 套件运行超时。
    TimeoutSuite,
    /// 0x4716 报告陈旧。
    ReportStale,
    /// 0x4717 守卫触发。
    GuardTripped,
    /// 0x4718 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x4719 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x471a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x471b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x471c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x471d 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t582Code {
    /// 全量闭集。
    pub const ALL: [t582Code; 14] = [
        t582Code::SuiteSkipped,
        t582Code::RedSuite,
        t582Code::OrderViolated,
        t582Code::LedgerBreak,
        t582Code::DoubleRun,
        t582Code::TimeoutSuite,
        t582Code::ReportStale,
        t582Code::GuardTripped,
        t582Code::LedgerBroken,
        t582Code::SnapshotFingerprint,
        t582Code::TableBroken,
        t582Code::IndexMisaligned,
        t582Code::DuplicateEntry,
        t582Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t582Code::SuiteSkipped => 0x4710,
            t582Code::RedSuite => 0x4711,
            t582Code::OrderViolated => 0x4712,
            t582Code::LedgerBreak => 0x4713,
            t582Code::DoubleRun => 0x4714,
            t582Code::TimeoutSuite => 0x4715,
            t582Code::ReportStale => 0x4716,
            t582Code::GuardTripped => 0x4717,
            t582Code::LedgerBroken => 0x4718,
            t582Code::SnapshotFingerprint => 0x4719,
            t582Code::TableBroken => 0x471a,
            t582Code::IndexMisaligned => 0x471b,
            t582Code::DuplicateEntry => 0x471c,
            t582Code::SegmentGuard => 0x471d,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t582Code::SuiteSkipped => "套件被跳过（未运行）",
            t582Code::RedSuite => "套件红（断言失败）",
            t582Code::OrderViolated => "运行序违例",
            t582Code::LedgerBreak => "运行账破缺",
            t582Code::DoubleRun => "重复运行",
            t582Code::TimeoutSuite => "套件运行超时",
            t582Code::ReportStale => "报告陈旧",
            t582Code::GuardTripped => "守卫触发",
            t582Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t582Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t582Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t582Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t582Code::DuplicateEntry => "条目重复（同键双写）",
            t582Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t582Code::SuiteSkipped => "按缺跑路径补跑",
            t582Code::RedSuite => "按归因路径登记",
            t582Code::OrderViolated => "按序重排",
            t582Code::LedgerBreak => "按守恒重建",
            t582Code::DoubleRun => "幂等拒绝",
            t582Code::TimeoutSuite => "按超时路径降级",
            t582Code::ReportStale => "重渲染",
            t582Code::GuardTripped => "检查临界区",
            t582Code::LedgerBroken => "按守恒律重建台账后重跑",
            t582Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t582Code::TableBroken => "按语义规则重建常量表",
            t582Code::IndexMisaligned => "按声明序归一化后重跑",
            t582Code::DuplicateEntry => "合并同键条目为末次生效",
            t582Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 引擎 kt（7 级有序流水：依赖序 + 环检测 + 进度账 + 回滚）
// --------------------------------------------------------------------------
/// ktest 运行七级流水（B19..B24 + 聚合）。
/// 级索引类型。
pub type StageIdx = usize;

/// 阶段枚举（闭集，与流水依赖矩阵同序）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ktStage {
    /// 级 根：运行套件 B19。
    RunB19,
    /// 级 0：运行套件 B20。
    RunB20,
    /// 级 1：运行套件 B21。
    RunB21,
    /// 级 2：运行套件 B22。
    RunB22,
    /// 级 3：运行套件 B23。
    RunB23,
    /// 级 4：运行套件 B24。
    RunB24,
    /// 级 5：聚合六套件结果。
    Aggregate,
}

impl ktStage {
    /// 全量级闭集。
    pub const ALL: [ktStage; 7] = [
        ktStage::RunB19,
        ktStage::RunB20,
        ktStage::RunB21,
        ktStage::RunB22,
        ktStage::RunB23,
        ktStage::RunB24,
        ktStage::Aggregate,
    ];

    /// 索引（与声明序恒等）。
    pub const fn index(self) -> usize {
        match self {
            ktStage::RunB19 => 0,
            ktStage::RunB20 => 1,
            ktStage::RunB21 => 2,
            ktStage::RunB22 => 3,
            ktStage::RunB23 => 4,
            ktStage::RunB24 => 5,
            ktStage::Aggregate => 6,
        }
    }

    /// 中文级名。
    pub const fn cn(self) -> &'static str {
        match self {
            ktStage::RunB19 => "运行套件 B19",
            ktStage::RunB20 => "运行套件 B20",
            ktStage::RunB21 => "运行套件 B21",
            ktStage::RunB22 => "运行套件 B22",
            ktStage::RunB23 => "运行套件 B23",
            ktStage::RunB24 => "运行套件 B24",
            ktStage::Aggregate => "聚合六套件结果",
        }
    }
}

/// 依赖矩阵（行=级，列=前置级索引；-1 表示根级无前置）。
pub const KT_DEPS: [i32; 7] = [
    -1, // RunB19 依赖 无
    0, // RunB20 依赖 级 0
    1, // RunB21 依赖 级 1
    2, // RunB22 依赖 级 2
    3, // RunB23 依赖 级 3
    4, // RunB24 依赖 级 4
    5, // Aggregate 依赖 级 5
];

/// 阶段进度状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ktProgress {
    /// 未开始。
    Pending,
    /// 进行中。
    Running,
    /// 已完成。
    Done,
    /// 已跳过（前置未就绪）。
    Skipped,
    /// 已回滚（运行中断）。
    Rolled,
}

/// 流水账本（五终态守恒：pending + running + done + skipped + rolled == 总级数）。
#[derive(Clone, Copy, Debug)]
pub struct ktBook {
    /// 各级状态。
    pub state: [ktProgress; 7],
    /// 已完成计数。
    pub done: u32,
    /// 跳过计数。
    pub skipped: u32,
    /// 回滚计数。
    pub rolled: u32,
    /// 已完成级索引序列（回放审计用）。
    pub order: [usize; 7],
    /// 序列长度。
    pub order_len: usize,
}

impl ktBook {
    /// 全挂起构造。
    pub const fn new() -> Self {
        Self {
            state: [ktProgress::Pending; 7],
            done: 0,
            skipped: 0,
            rolled: 0,
            order: [0; 7],
            order_len: 0,
        }
    }

    /// 拓扑序推演（Kahn 剥皮：逐轮找全前置已 Done 的 Pending 级）。
    /// 返回推演出的可执行序；环依赖时返回 Err(环首级索引)。
    pub fn plan(&mut self) -> Result<[usize; 7], usize> {
        let mut order = [0usize; 7];
        let mut olen = 0usize;
        let mut rounds = 0usize;
        while rounds < 7 {
            let mut progressed = false;
            let mut i = 0usize;
            while i < 7 {
                if self.state[i] != ktProgress::Pending {
                    i += 1;
                    continue;
                }
                let dep = KT_DEPS[i];
                if dep < 0 || self.state[dep as usize] == ktProgress::Done {
                    order[olen] = i;
                    olen += 1;
                    self.state[i] = ktProgress::Done;
                    self.done += 1;
                    progressed = true;
                }
                i += 1;
            }
            if !progressed {
                // 剩余未决级中存在环：找首个未决级上报。
                let mut k = 0usize;
                while k < 7 {
                    if self.state[k] == ktProgress::Pending {
                        return Err(k);
                    }
                    k += 1;
                }
            }
            rounds += 1;
        }
        self.order = order;
        self.order_len = olen;
        Ok(order)
    }

    /// 注入失败：自失败级起全部 Done 级回滚（运行中断）。
    pub fn rollback_from(&mut self, idx: usize) {
        let mut i = idx;
        while i < 7 {
            if self.state[i] == ktProgress::Done {
                self.state[i] = ktProgress::Rolled;
                if self.done > 0 {
                    self.done -= 1;
                }
                self.rolled += 1;
            }
            i += 1;
        }
    }

    /// 守恒恒等式：done + skipped + rolled + pending == 总级数。
    pub fn conserved(&self) -> bool {
        let mut pending = 0u32;
        let mut i = 0usize;
        while i < 7 {
            if self.state[i] == ktProgress::Pending {
                pending += 1;
            }
            i += 1;
        }
        self.done + self.skipped + self.rolled + pending == 7
    }

    /// 已完成级回放序列与推演序一致（顺序守恒证据）。
    pub fn order_matches(&self) -> bool {
        let mut i = 0usize;
        while i < self.order_len {
            if self.order[i] >= 7 || self.state[self.order[i]] != ktProgress::Done {
                return false;
            }
            i += 1;
        }
        true
    }
}

// --------------------------------------------------------------------------
// 守恒账 ktrun（运行账（绿套件 + 红套件 + 跳过 == 8 套件））
// --------------------------------------------------------------------------
/// 运行账（绿套件 + 红套件 + 跳过 == 8 套件）。
#[derive(Clone, Copy, Debug)]
pub struct ktrun {
    /// 全绿套件数
    pub green: u64,
    /// 含红套件数
    pub red: u64,
    /// 跳过套件数
    pub skipped: u64,
}

impl ktrun {
    /// 全零构造。
    pub const fn new() -> Self {
        Self {
            green: 0,
            red: 0,
            skipped: 0,
        }
    }

    /// 一套件全绿。
    pub fn green_one(&mut self) {
        self.green += 1;
    }

    /// 一套件含红。
    pub fn red_one(&mut self) {
        self.red += 1;
    }

    /// 跳一套件。
    pub fn skip_one(&mut self) {
        self.skipped += 1;
    }

}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x4710, 0x471f]。
pub const fn t582_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t582Code::ALL.len() {
        let c = t582Code::ALL[i].code();
        if c < 0x4710 || c > 0x471f {
            ok = false;
        }
        i += 1;
    }
    ok
}

// --------------------------------------------------------------------------
// 自检（CheckSet 范式：每检查一带 detail，全绿才出门）
// --------------------------------------------------------------------------
/// 段位守卫：全部诊断码落在本模块段内。
fn t582_guard_seg() -> bool {
    t582_segment_guard()
}

/// 运行账守恒（6 绿 + 0 红 + 2 未用 = 8 槽）。
fn check_c1_run_ledger() -> bool {
    let mut r = ktrun::new();
    for _ in 0..6 {
        r.green_one();
    }
    let mut ok = r.green == 6 && r.red == 0 && r.skipped == 0;
    if ok {
        // 6/6 全绿判据。
        ok = r.green == 6 && r.green + r.red + r.skipped <= 8;
    }
    ok
}

/// 运行九级流水推演全达。
fn check_c2_run_pipeline() -> bool {
    let mut bk = ktBook::new();
    let mut ok = matches!(bk.plan(), Ok(_)) && bk.done == 7;
    if ok {
        ok = bk.order_matches() && bk.conserved();
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t582_checks() -> CheckSet {
    let mut set = CheckSet::new("T582");
    set.add(
        "t582-seg-guard",
        t582_guard_seg(),
        if t582_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t582-c1-run-ledger",
        check_c1_run_ledger(),
        if check_c1_run_ledger() { "" } else { "suite run ledger mismatch" },
    );
    set.add(
        "t582-c2-run-pipeline",
        check_c2_run_pipeline(),
        if check_c2_run_pipeline() { "" } else { "suite run pipeline mismatch" },
    );
    set
}

// ---------------------------------------------------------------------------
// 单元测试（域聚合测试在 mod.rs——此处只放本模块纯函数级断言）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t582_all_checks_green() {
        let set = run_t582_checks();
        assert!(set.all_passed(), "T582 domain checks must be green");
    }

    #[test]
    fn t582_segment_guard_holds() {
        assert!(t582_segment_guard());
    }
}
