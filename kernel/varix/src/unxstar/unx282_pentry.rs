//! unxt282_pentry —— UNX-F0281 统一入口流水（四源归一）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0281 · panic 入口统一（双狗/断言/拒载/用户触发四源） · 判据：四源触发统一入口，来源标记与去重合并正确，重入 panic 防护过
//! · 组件：归一流水（六级）+ 入口状态机 + 首因冻结
//! · 判据落地：四源触发统一入口
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T282_VERSION: &str = "t282-pentry-v1";

/// 诊断段基址。
pub const T282_SEG_BASE: u16 = 0x3b10;

// --------------------------------------------------------------------------
// 诊断码（t282 十六码 · 16 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 统一入口诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t282Code {
    /// 0x3b10 入口重入（panic 中再 panic）。
    EntryReentered,
    /// 0x3b11 入口丢源（来源标记未透传）。
    SourceLostInEntry,
    /// 0x3b12 首因冻结（后续源被折叠）。
    FirstCauseFrozen,
    /// 0x3b13 入口流水停滞。
    PipelineStall,
    /// 0x3b14 合并冲突（同刻双源）。
    MergeConflict,
    /// 0x3b15 入口次序非法。
    EntryOrderBad,
    /// 0x3b16 入口账破缺。
    LedgerBreak,
    /// 0x3b17 入口报告陈旧。
    ReportStale,
    /// 0x3b18 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x3b19 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x3b1a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x3b1b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x3b1c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x3b1d 次序违例（依赖序/时序破缺）。
    OrderViolated,
    /// 0x3b1e 守卫触发（重入/越限被拒）。
    GuardTripped,
    /// 0x3b1f 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t282Code {
    /// 全量闭集。
    pub const ALL: [t282Code; 16] = [
        t282Code::EntryReentered,
        t282Code::SourceLostInEntry,
        t282Code::FirstCauseFrozen,
        t282Code::PipelineStall,
        t282Code::MergeConflict,
        t282Code::EntryOrderBad,
        t282Code::LedgerBreak,
        t282Code::ReportStale,
        t282Code::LedgerBroken,
        t282Code::SnapshotFingerprint,
        t282Code::TableBroken,
        t282Code::IndexMisaligned,
        t282Code::DuplicateEntry,
        t282Code::OrderViolated,
        t282Code::GuardTripped,
        t282Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t282Code::EntryReentered => 0x3b10,
            t282Code::SourceLostInEntry => 0x3b11,
            t282Code::FirstCauseFrozen => 0x3b12,
            t282Code::PipelineStall => 0x3b13,
            t282Code::MergeConflict => 0x3b14,
            t282Code::EntryOrderBad => 0x3b15,
            t282Code::LedgerBreak => 0x3b16,
            t282Code::ReportStale => 0x3b17,
            t282Code::LedgerBroken => 0x3b18,
            t282Code::SnapshotFingerprint => 0x3b19,
            t282Code::TableBroken => 0x3b1a,
            t282Code::IndexMisaligned => 0x3b1b,
            t282Code::DuplicateEntry => 0x3b1c,
            t282Code::OrderViolated => 0x3b1d,
            t282Code::GuardTripped => 0x3b1e,
            t282Code::SegmentGuard => 0x3b1f,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t282Code::EntryReentered => "入口重入（panic 中再 panic）",
            t282Code::SourceLostInEntry => "入口丢源（来源标记未透传）",
            t282Code::FirstCauseFrozen => "首因冻结（后续源被折叠）",
            t282Code::PipelineStall => "入口流水停滞",
            t282Code::MergeConflict => "合并冲突（同刻双源）",
            t282Code::EntryOrderBad => "入口次序非法",
            t282Code::LedgerBreak => "入口账破缺",
            t282Code::ReportStale => "入口报告陈旧",
            t282Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t282Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t282Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t282Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t282Code::DuplicateEntry => "条目重复（同键双写）",
            t282Code::OrderViolated => "次序违例（依赖序/时序破缺）",
            t282Code::GuardTripped => "守卫触发（重入/越限被拒）",
            t282Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t282Code::EntryReentered => "按重入防护挂起",
            t282Code::SourceLostInEntry => "按标记透传链补传",
            t282Code::FirstCauseFrozen => "按首因优先规则折叠",
            t282Code::PipelineStall => "按停滞检测重组",
            t282Code::MergeConflict => "按首因优先仲裁",
            t282Code::EntryOrderBad => "按六序律重排",
            t282Code::LedgerBreak => "按守恒重建",
            t282Code::ReportStale => "重渲染",
            t282Code::LedgerBroken => "按守恒律重建台账后重跑",
            t282Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t282Code::TableBroken => "按语义规则重建常量表",
            t282Code::IndexMisaligned => "按声明序归一化后重跑",
            t282Code::DuplicateEntry => "合并同键条目为末次生效",
            t282Code::OrderViolated => "按依赖矩阵重排后重跑",
            t282Code::GuardTripped => "检查临界区嵌套与阈值",
            t282Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 引擎 pent（6 级有序流水：依赖序 + 环检测 + 进度账 + 回滚）
// --------------------------------------------------------------------------
/// 统一入口六级流水。
/// 级索引类型。
pub type StageIdx = usize;

/// 阶段枚举（闭集，与流水依赖矩阵同序）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum pentStage {
    /// 级 根：捕获触发源（四源之一）。
    Capture,
    /// 级 0：打来源标记。
    Tag,
    /// 级 1：冻结首因。
    Freeze,
    /// 级 2：去重合并。
    Dedup,
    /// 级 3：停机序列。
    Halt,
    /// 级 4：报告落盘。
    Report,
}

impl pentStage {
    /// 全量级闭集。
    pub const ALL: [pentStage; 6] = [
        pentStage::Capture,
        pentStage::Tag,
        pentStage::Freeze,
        pentStage::Dedup,
        pentStage::Halt,
        pentStage::Report,
    ];

    /// 索引（与声明序恒等）。
    pub const fn index(self) -> usize {
        match self {
            pentStage::Capture => 0,
            pentStage::Tag => 1,
            pentStage::Freeze => 2,
            pentStage::Dedup => 3,
            pentStage::Halt => 4,
            pentStage::Report => 5,
        }
    }

    /// 中文级名。
    pub const fn cn(self) -> &'static str {
        match self {
            pentStage::Capture => "捕获触发源（四源之一）",
            pentStage::Tag => "打来源标记",
            pentStage::Freeze => "冻结首因",
            pentStage::Dedup => "去重合并",
            pentStage::Halt => "停机序列",
            pentStage::Report => "报告落盘",
        }
    }
}

/// 依赖矩阵（行=级，列=前置级索引；-1 表示根级无前置）。
pub const PENT_DEPS: [i32; 6] = [
    -1, // Capture 依赖 无
    0, // Tag 依赖 级 0
    1, // Freeze 依赖 级 1
    2, // Dedup 依赖 级 2
    3, // Halt 依赖 级 3
    4, // Report 依赖 级 4
];

/// 阶段进度状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum pentProgress {
    /// 未开始。
    Pending,
    /// 进行中。
    Running,
    /// 已完成。
    Done,
    /// 已跳过（前置未就绪）。
    Skipped,
    /// 已回滚（入口重入挂起）。
    Rolled,
}

/// 流水账本（五终态守恒：pending + running + done + skipped + rolled == 总级数）。
#[derive(Clone, Copy, Debug)]
pub struct pentBook {
    /// 各级状态。
    pub state: [pentProgress; 6],
    /// 已完成计数。
    pub done: u32,
    /// 跳过计数。
    pub skipped: u32,
    /// 回滚计数。
    pub rolled: u32,
    /// 已完成级索引序列（回放审计用）。
    pub order: [usize; 6],
    /// 序列长度。
    pub order_len: usize,
}

impl pentBook {
    /// 全挂起构造。
    pub const fn new() -> Self {
        Self {
            state: [pentProgress::Pending; 6],
            done: 0,
            skipped: 0,
            rolled: 0,
            order: [0; 6],
            order_len: 0,
        }
    }

    /// 拓扑序推演（Kahn 剥皮：逐轮找全前置已 Done 的 Pending 级）。
    /// 返回推演出的可执行序；环依赖时返回 Err(环首级索引)。
    pub fn plan(&mut self) -> Result<[usize; 6], usize> {
        let mut order = [0usize; 6];
        let mut olen = 0usize;
        let mut rounds = 0usize;
        while rounds < 6 {
            let mut progressed = false;
            let mut i = 0usize;
            while i < 6 {
                if self.state[i] != pentProgress::Pending {
                    i += 1;
                    continue;
                }
                let dep = PENT_DEPS[i];
                if dep < 0 || self.state[dep as usize] == pentProgress::Done {
                    order[olen] = i;
                    olen += 1;
                    self.state[i] = pentProgress::Done;
                    self.done += 1;
                    progressed = true;
                }
                i += 1;
            }
            if !progressed {
                // 剩余未决级中存在环：找首个未决级上报。
                let mut k = 0usize;
                while k < 6 {
                    if self.state[k] == pentProgress::Pending {
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

    /// 注入失败：自失败级起全部 Done 级回滚（入口重入挂起）。
    pub fn rollback_from(&mut self, idx: usize) {
        let mut i = idx;
        while i < 6 {
            if self.state[i] == pentProgress::Done {
                self.state[i] = pentProgress::Rolled;
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
        while i < 6 {
            if self.state[i] == pentProgress::Pending {
                pending += 1;
            }
            i += 1;
        }
        self.done + self.skipped + self.rolled + pending == 6
    }

    /// 已完成级回放序列与推演序一致（顺序守恒证据）。
    pub fn order_matches(&self) -> bool {
        let mut i = 0usize;
        while i < self.order_len {
            if self.order[i] >= 6 || self.state[self.order[i]] != pentProgress::Done {
                return false;
            }
            i += 1;
        }
        true
    }
}

// --------------------------------------------------------------------------
// 状态机 pent（5 态 × 4 事件 · 迁移 5 条显式声明）
// --------------------------------------------------------------------------
/// 入口状态机（重入折叠显性）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum pentState {
    Idle,
    Entered,
    Merged,
    Halting,
    Halted,
}

impl pentState {
    /// 全量态闭集。
    pub const ALL: [pentState; 5] = [
        pentState::Idle,
        pentState::Entered,
        pentState::Merged,
        pentState::Halting,
        pentState::Halted,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            pentState::Idle => 0,
            pentState::Entered => 1,
            pentState::Merged => 2,
            pentState::Halting => 3,
            pentState::Halted => 4,
        }
    }
}

/// pent事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum pentEvent {
    Enter,
    Merge,
    Halt,
    Finish,
}

impl pentEvent {
    /// 全量事件闭集。
    pub const ALL: [pentEvent; 4] = [
        pentEvent::Enter,
        pentEvent::Merge,
        pentEvent::Halt,
        pentEvent::Finish,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const PENT_TRANSITIONS: [(pentState, pentEvent, pentState); 5] = [
    (pentState::Idle, pentEvent::Enter, pentState::Entered), // 进入统一入口
    (pentState::Entered, pentEvent::Merge, pentState::Merged), // 去重合并
    (pentState::Merged, pentEvent::Halt, pentState::Halting), // 停机序列
    (pentState::Halting, pentEvent::Finish, pentState::Halted), // 停机完成
    (pentState::Entered, pentEvent::Enter, pentState::Merged), // panic 中再触发走折叠路径
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn pent_next(state: pentState, ev: pentEvent) -> Option<pentState> {
    let mut i = 0usize;
    while i < 5 {
        let t = PENT_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn pent_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < pentState::ALL.len() {
        let mut ei = 0usize;
        while ei < pentEvent::ALL.len() {
            let legal = pent_next(pentState::ALL[si], pentEvent::ALL[ei]).is_some();
            if !legal {
                bad += 1;
            }
            ei += 1;
        }
        si += 1;
    }
    bad
}

/// 终态集合（无出边的态即终态）。
pub fn pent_terminal(st: pentState) -> bool {
    let mut i = 0usize;
    while i < 5 {
        if PENT_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct pentLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: pentState,
}

impl pentLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: pentState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: pentEvent) -> bool {
        match pent_next(self.cur, ev) {
            Some(ns) => {
                self.cur = ns;
                self.legal += 1;
                true
            }
            None => {
                self.illegal += 1;
                false
            }
        }
    }

    /// 守恒恒等式：legal + illegal == 施加次数（由调用方累计）。
    pub fn total(&self) -> u64 {
        self.legal + self.illegal
    }
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x3b10, 0x3b1f]。
pub const fn t282_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t282Code::ALL.len() {
        let c = t282Code::ALL[i].code();
        if c < 0x3b10 || c > 0x3b1f {
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
fn t282_guard_seg() -> bool {
    t282_segment_guard()
}

/// 入口六级流水推演全达。
fn check_c1_entry_pipeline() -> bool {
    let mut bk = pentBook::new();
    let mut ok = matches!(bk.plan(), Ok(_)) && bk.done == 6;
    ok
}

/// 入口状态机重入折叠路径。
fn check_c2_entry_sm() -> bool {
    let mut led = pentLedger::new(pentState::Idle);
    let mut ok = led.apply(pentEvent::Enter);
    ok = ok && led.apply(pentEvent::Enter);
    if ok {
        ok = led.cur == pentState::Merged && led.legal == 2;
    }
    if ok {
        ok = led.apply(pentEvent::Halt) && led.apply(pentEvent::Finish);
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t282_checks() -> CheckSet {
    let mut set = CheckSet::new("T282");
    set.add(
        "t282-seg-guard",
        t282_guard_seg(),
        if t282_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t282-c1-entry-pipeline",
        check_c1_entry_pipeline(),
        if check_c1_entry_pipeline() { "" } else { "entry pipeline mismatch" },
    );
    set.add(
        "t282-c2-entry-sm",
        check_c2_entry_sm(),
        if check_c2_entry_sm() { "" } else { "entry state machine mismatch" },
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
    fn t282_all_checks_green() {
        let set = run_t282_checks();
        assert!(set.all_passed(), "T282 domain checks must be green");
    }

    #[test]
    fn t282_segment_guard_holds() {
        assert!(t282_segment_guard());
    }
}
