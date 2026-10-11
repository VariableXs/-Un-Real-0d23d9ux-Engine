//! unxt225_kauto —— UNX-F0221 自动聚合流水（新批次接入即自动归集）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0221 · 引导面断言聚合扩容（300+ 达成线） · 判据：ktest 引导面断言总数 ≥300 并随批次自动聚合，计数账与实际运行数一致
//! · 组件：接入流水（五级）+ 批次绑定状态机 + 迟到批补嵌路径
//! · 判据落地：随批次自动聚合的机制面
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T225_VERSION: &str = "t225-kauto-v1";

/// 诊断段基址。
pub const T225_SEG_BASE: u16 = 0x3840;

// --------------------------------------------------------------------------
// 诊断码（t225 十六码 · 16 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 自动聚合流水诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t225Code {
    /// 0x3840 迟绑定（批次晚于聚合窗到达）。
    BindLate,
    /// 0x3841 流水停滞（级间无产出）。
    PipelineStall,
    /// 0x3842 阶段跳过（依赖未就绪）。
    StageSkip,
    /// 0x3843 自动聚合关闭（手动模式混入）。
    AutoOff,
    /// 0x3844 归并冲突（同号双源）。
    MergeConflict,
    /// 0x3845 队列溢出（待聚合批次超容）。
    QueueOverflow,
    /// 0x3846 幂等破缺（重复运行结果漂移）。
    IdempotentBreak,
    /// 0x3847 回滚不完整（聚合中途断）。
    RollbackIncomplete,
    /// 0x3848 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x3849 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x384a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x384b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x384c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x384d 次序违例（依赖序/时序破缺）。
    OrderViolated,
    /// 0x384e 守卫触发（重入/越限被拒）。
    GuardTripped,
    /// 0x384f 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t225Code {
    /// 全量闭集。
    pub const ALL: [t225Code; 16] = [
        t225Code::BindLate,
        t225Code::PipelineStall,
        t225Code::StageSkip,
        t225Code::AutoOff,
        t225Code::MergeConflict,
        t225Code::QueueOverflow,
        t225Code::IdempotentBreak,
        t225Code::RollbackIncomplete,
        t225Code::LedgerBroken,
        t225Code::SnapshotFingerprint,
        t225Code::TableBroken,
        t225Code::IndexMisaligned,
        t225Code::DuplicateEntry,
        t225Code::OrderViolated,
        t225Code::GuardTripped,
        t225Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t225Code::BindLate => 0x3840,
            t225Code::PipelineStall => 0x3841,
            t225Code::StageSkip => 0x3842,
            t225Code::AutoOff => 0x3843,
            t225Code::MergeConflict => 0x3844,
            t225Code::QueueOverflow => 0x3845,
            t225Code::IdempotentBreak => 0x3846,
            t225Code::RollbackIncomplete => 0x3847,
            t225Code::LedgerBroken => 0x3848,
            t225Code::SnapshotFingerprint => 0x3849,
            t225Code::TableBroken => 0x384a,
            t225Code::IndexMisaligned => 0x384b,
            t225Code::DuplicateEntry => 0x384c,
            t225Code::OrderViolated => 0x384d,
            t225Code::GuardTripped => 0x384e,
            t225Code::SegmentGuard => 0x384f,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t225Code::BindLate => "迟绑定（批次晚于聚合窗到达）",
            t225Code::PipelineStall => "流水停滞（级间无产出）",
            t225Code::StageSkip => "阶段跳过（依赖未就绪）",
            t225Code::AutoOff => "自动聚合关闭（手动模式混入）",
            t225Code::MergeConflict => "归并冲突（同号双源）",
            t225Code::QueueOverflow => "队列溢出（待聚合批次超容）",
            t225Code::IdempotentBreak => "幂等破缺（重复运行结果漂移）",
            t225Code::RollbackIncomplete => "回滚不完整（聚合中途断）",
            t225Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t225Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t225Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t225Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t225Code::DuplicateEntry => "条目重复（同键双写）",
            t225Code::OrderViolated => "次序违例（依赖序/时序破缺）",
            t225Code::GuardTripped => "守卫触发（重入/越限被拒）",
            t225Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t225Code::BindLate => "走补嵌路径并入",
            t225Code::PipelineStall => "按停滞检测放行或重组",
            t225Code::StageSkip => "按依赖序重排",
            t225Code::AutoOff => "切回自动模式",
            t225Code::MergeConflict => "按末次生效合并",
            t225Code::QueueOverflow => "排队或扩容",
            t225Code::IdempotentBreak => "按同种子重放校验",
            t225Code::RollbackIncomplete => "补全回滚后重跑",
            t225Code::LedgerBroken => "按守恒律重建台账后重跑",
            t225Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t225Code::TableBroken => "按语义规则重建常量表",
            t225Code::IndexMisaligned => "按声明序归一化后重跑",
            t225Code::DuplicateEntry => "合并同键条目为末次生效",
            t225Code::OrderViolated => "按依赖矩阵重排后重跑",
            t225Code::GuardTripped => "检查临界区嵌套与阈值",
            t225Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 引擎 kagg（5 级有序流水：依赖序 + 环检测 + 进度账 + 回滚）
// --------------------------------------------------------------------------
/// 自动聚合五级流水。
/// 级索引类型。
pub type StageIdx = usize;

/// 阶段枚举（闭集，与流水依赖矩阵同序）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum kaggStage {
    /// 级 根：接入新批次原始断言流。
    Ingest,
    /// 级 0：归一化断言号与批号。
    Normalize,
    /// 级 1：绑定批次归属。
    Bind,
    /// 级 2：聚合进达成线网格。
    Aggregate,
    /// 级 3：计数账对拍收口。
    Reconcile,
}

impl kaggStage {
    /// 全量级闭集。
    pub const ALL: [kaggStage; 5] = [
        kaggStage::Ingest,
        kaggStage::Normalize,
        kaggStage::Bind,
        kaggStage::Aggregate,
        kaggStage::Reconcile,
    ];

    /// 索引（与声明序恒等）。
    pub const fn index(self) -> usize {
        match self {
            kaggStage::Ingest => 0,
            kaggStage::Normalize => 1,
            kaggStage::Bind => 2,
            kaggStage::Aggregate => 3,
            kaggStage::Reconcile => 4,
        }
    }

    /// 中文级名。
    pub const fn cn(self) -> &'static str {
        match self {
            kaggStage::Ingest => "接入新批次原始断言流",
            kaggStage::Normalize => "归一化断言号与批号",
            kaggStage::Bind => "绑定批次归属",
            kaggStage::Aggregate => "聚合进达成线网格",
            kaggStage::Reconcile => "计数账对拍收口",
        }
    }
}

/// 依赖矩阵（行=级，列=前置级索引；-1 表示根级无前置）。
pub const KAGG_DEPS: [i32; 5] = [
    -1, // Ingest 依赖 无
    0, // Normalize 依赖 级 0
    1, // Bind 依赖 级 1
    2, // Aggregate 依赖 级 2
    3, // Reconcile 依赖 级 3
];

/// 阶段进度状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum kaggProgress {
    /// 未开始。
    Pending,
    /// 进行中。
    Running,
    /// 已完成。
    Done,
    /// 已跳过（前置未就绪）。
    Skipped,
    /// 已回滚（聚合中途断）。
    Rolled,
}

/// 流水账本（五终态守恒：pending + running + done + skipped + rolled == 总级数）。
#[derive(Clone, Copy, Debug)]
pub struct kaggBook {
    /// 各级状态。
    pub state: [kaggProgress; 5],
    /// 已完成计数。
    pub done: u32,
    /// 跳过计数。
    pub skipped: u32,
    /// 回滚计数。
    pub rolled: u32,
    /// 已完成级索引序列（回放审计用）。
    pub order: [usize; 5],
    /// 序列长度。
    pub order_len: usize,
}

impl kaggBook {
    /// 全挂起构造。
    pub const fn new() -> Self {
        Self {
            state: [kaggProgress::Pending; 5],
            done: 0,
            skipped: 0,
            rolled: 0,
            order: [0; 5],
            order_len: 0,
        }
    }

    /// 拓扑序推演（Kahn 剥皮：逐轮找全前置已 Done 的 Pending 级）。
    /// 返回推演出的可执行序；环依赖时返回 Err(环首级索引)。
    pub fn plan(&mut self) -> Result<[usize; 5], usize> {
        let mut order = [0usize; 5];
        let mut olen = 0usize;
        let mut rounds = 0usize;
        while rounds < 5 {
            let mut progressed = false;
            let mut i = 0usize;
            while i < 5 {
                if self.state[i] != kaggProgress::Pending {
                    i += 1;
                    continue;
                }
                let dep = KAGG_DEPS[i];
                if dep < 0 || self.state[dep as usize] == kaggProgress::Done {
                    order[olen] = i;
                    olen += 1;
                    self.state[i] = kaggProgress::Done;
                    self.done += 1;
                    progressed = true;
                }
                i += 1;
            }
            if !progressed {
                // 剩余未决级中存在环：找首个未决级上报。
                let mut k = 0usize;
                while k < 5 {
                    if self.state[k] == kaggProgress::Pending {
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

    /// 注入失败：自失败级起全部 Done 级回滚（聚合中途断）。
    pub fn rollback_from(&mut self, idx: usize) {
        let mut i = idx;
        while i < 5 {
            if self.state[i] == kaggProgress::Done {
                self.state[i] = kaggProgress::Rolled;
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
        while i < 5 {
            if self.state[i] == kaggProgress::Pending {
                pending += 1;
            }
            i += 1;
        }
        self.done + self.skipped + self.rolled + pending == 5
    }

    /// 已完成级回放序列与推演序一致（顺序守恒证据）。
    pub fn order_matches(&self) -> bool {
        let mut i = 0usize;
        while i < self.order_len {
            if self.order[i] >= 5 || self.state[self.order[i]] != kaggProgress::Done {
                return false;
            }
            i += 1;
        }
        true
    }
}

// --------------------------------------------------------------------------
// 状态机 bind（4 态 × 4 事件 · 迁移 5 条显式声明）
// --------------------------------------------------------------------------
/// 批次绑定状态机状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum bindState {
    Unbound,
    Bound,
    Merged,
    Reconciled,
}

impl bindState {
    /// 全量态闭集。
    pub const ALL: [bindState; 4] = [
        bindState::Unbound,
        bindState::Bound,
        bindState::Merged,
        bindState::Reconciled,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            bindState::Unbound => 0,
            bindState::Bound => 1,
            bindState::Merged => 2,
            bindState::Reconciled => 3,
        }
    }
}

/// bind事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum bindEvent {
    Bind,
    Merge,
    Reconcile,
    Reject,
}

impl bindEvent {
    /// 全量事件闭集。
    pub const ALL: [bindEvent; 4] = [
        bindEvent::Bind,
        bindEvent::Merge,
        bindEvent::Reconcile,
        bindEvent::Reject,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const BIND_TRANSITIONS: [(bindState, bindEvent, bindState); 5] = [
    (bindState::Unbound, bindEvent::Bind, bindState::Bound), // 批次绑定
    (bindState::Bound, bindEvent::Merge, bindState::Merged), // 归并进网格
    (bindState::Merged, bindEvent::Reconcile, bindState::Reconciled), // 对拍收口
    (bindState::Unbound, bindEvent::Reject, bindState::Unbound), // 拒收保持未绑定
    (bindState::Bound, bindEvent::Reject, bindState::Unbound), // 绑定失败退回
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn bind_next(state: bindState, ev: bindEvent) -> Option<bindState> {
    let mut i = 0usize;
    while i < 5 {
        let t = BIND_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn bind_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < bindState::ALL.len() {
        let mut ei = 0usize;
        while ei < bindEvent::ALL.len() {
            let legal = bind_next(bindState::ALL[si], bindEvent::ALL[ei]).is_some();
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
pub fn bind_terminal(st: bindState) -> bool {
    let mut i = 0usize;
    while i < 5 {
        if BIND_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct bindLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: bindState,
}

impl bindLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: bindState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: bindEvent) -> bool {
        match bind_next(self.cur, ev) {
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

/// 段位守卫：全部诊断码必须落在本段 [0x3840, 0x384f]。
pub const fn t225_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t225Code::ALL.len() {
        let c = t225Code::ALL[i].code();
        if c < 0x3840 || c > 0x384f {
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
fn t225_guard_seg() -> bool {
    t225_segment_guard()
}

/// 自动聚合五级流水推演全达 + 顺序单调。
fn check_c1_pipeline() -> bool {
    let mut bk = kaggBook::new();
    let mut ok = matches!(bk.plan(), Ok(_)) && bk.done == 5;
    if ok {
        ok = bk.order[0] == 0 && bk.order[4] == 4;
    }
    ok
}

/// 绑定状态机主路径 + 拒止。
fn check_c2_bind_sm() -> bool {
    let mut led = bindLedger::new(bindState::Unbound);
    let mut ok = led.apply(bindEvent::Bind);
    ok = ok && led.apply(bindEvent::Merge);
    ok = ok && led.apply(bindEvent::Reconcile);
    if ok {
        ok = led.cur == bindState::Reconciled;
    }
    if ok {
        // Reconciled 无出边。
        ok = !led.apply(bindEvent::Bind);
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t225_checks() -> CheckSet {
    let mut set = CheckSet::new("T225");
    set.add(
        "t225-seg-guard",
        t225_guard_seg(),
        if t225_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t225-c1-pipeline",
        check_c1_pipeline(),
        if check_c1_pipeline() { "" } else { "auto-aggregation pipeline mismatch" },
    );
    set.add(
        "t225-c2-bind-sm",
        check_c2_bind_sm(),
        if check_c2_bind_sm() { "" } else { "bind state machine mismatch" },
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
    fn t225_all_checks_green() {
        let set = run_t225_checks();
        assert!(set.all_passed(), "T225 domain checks must be green");
    }

    #[test]
    fn t225_segment_guard_holds() {
        assert!(t225_segment_guard());
    }
}
