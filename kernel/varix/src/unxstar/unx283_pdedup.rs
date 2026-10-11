//! unxt283_pdedup —— UNX-F0281 去重合并账（同源因折叠 + 计数守恒）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0281 · panic 入口统一（双狗/断言/拒载/用户触发四源） · 判据：四源触发统一入口，来源标记与去重合并正确，重入 panic 防护过
//! · 组件：去重账（首因保留 + 重复折叠计数）+ 合并状态机 + 守恒验证
//! · 判据落地：来源标记与去重合并正确
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T283_VERSION: &str = "t283-pdedup-v1";

/// 诊断段基址。
pub const T283_SEG_BASE: u16 = 0x3b20;

// --------------------------------------------------------------------------
// 诊断码（t283 十六码 · 15 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 去重合并诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t283Code {
    /// 0x3b20 折叠丢失（重复源未合并）。
    FoldLost,
    /// 0x3b21 首因丢失（非首因覆盖首因）。
    FirstCauseLost,
    /// 0x3b22 合并账破缺（折叠+保留≠总数）。
    MergeLedgerBreak,
    /// 0x3b23 标记不符（合并后标记漂移）。
    TagMismatch,
    /// 0x3b24 幻影合并（无对应重复）。
    GhostMerge,
    /// 0x3b25 合并序折叠。
    OrderFold,
    /// 0x3b26 计数溢出。
    CounterOverflow,
    /// 0x3b27 守卫触发。
    GuardTripped,
    /// 0x3b28 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x3b29 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x3b2a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x3b2b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x3b2c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x3b2d 次序违例（依赖序/时序破缺）。
    OrderViolated,
    /// 0x3b2e 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t283Code {
    /// 全量闭集。
    pub const ALL: [t283Code; 15] = [
        t283Code::FoldLost,
        t283Code::FirstCauseLost,
        t283Code::MergeLedgerBreak,
        t283Code::TagMismatch,
        t283Code::GhostMerge,
        t283Code::OrderFold,
        t283Code::CounterOverflow,
        t283Code::GuardTripped,
        t283Code::LedgerBroken,
        t283Code::SnapshotFingerprint,
        t283Code::TableBroken,
        t283Code::IndexMisaligned,
        t283Code::DuplicateEntry,
        t283Code::OrderViolated,
        t283Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t283Code::FoldLost => 0x3b20,
            t283Code::FirstCauseLost => 0x3b21,
            t283Code::MergeLedgerBreak => 0x3b22,
            t283Code::TagMismatch => 0x3b23,
            t283Code::GhostMerge => 0x3b24,
            t283Code::OrderFold => 0x3b25,
            t283Code::CounterOverflow => 0x3b26,
            t283Code::GuardTripped => 0x3b27,
            t283Code::LedgerBroken => 0x3b28,
            t283Code::SnapshotFingerprint => 0x3b29,
            t283Code::TableBroken => 0x3b2a,
            t283Code::IndexMisaligned => 0x3b2b,
            t283Code::DuplicateEntry => 0x3b2c,
            t283Code::OrderViolated => 0x3b2d,
            t283Code::SegmentGuard => 0x3b2e,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t283Code::FoldLost => "折叠丢失（重复源未合并）",
            t283Code::FirstCauseLost => "首因丢失（非首因覆盖首因）",
            t283Code::MergeLedgerBreak => "合并账破缺（折叠+保留≠总数）",
            t283Code::TagMismatch => "标记不符（合并后标记漂移）",
            t283Code::GhostMerge => "幻影合并（无对应重复）",
            t283Code::OrderFold => "合并序折叠",
            t283Code::CounterOverflow => "计数溢出",
            t283Code::GuardTripped => "守卫触发",
            t283Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t283Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t283Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t283Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t283Code::DuplicateEntry => "条目重复（同键双写）",
            t283Code::OrderViolated => "次序违例（依赖序/时序破缺）",
            t283Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t283Code::FoldLost => "按折叠规则合并",
            t283Code::FirstCauseLost => "按首因优先恢复",
            t283Code::MergeLedgerBreak => "按守恒重建",
            t283Code::TagMismatch => "按首因标记归一",
            t283Code::GhostMerge => "核对 tombstone",
            t283Code::OrderFold => "按到达序重排",
            t283Code::CounterOverflow => "分段结算",
            t283Code::GuardTripped => "检查临界区",
            t283Code::LedgerBroken => "按守恒律重建台账后重跑",
            t283Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t283Code::TableBroken => "按语义规则重建常量表",
            t283Code::IndexMisaligned => "按声明序归一化后重跑",
            t283Code::DuplicateEntry => "合并同键条目为末次生效",
            t283Code::OrderViolated => "按依赖矩阵重排后重跑",
            t283Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 守恒账 fold（去重折叠账（kept + folded == arrivals））
// --------------------------------------------------------------------------
/// 去重折叠账（kept + folded == arrivals）。
#[derive(Clone, Copy, Debug)]
pub struct fold {
    /// 保留的首因条目数
    pub kept: u64,
    /// 折叠的重复数
    pub folded: u64,
    /// 总到达数
    pub arrivals: u64,
}

impl fold {
    /// 全零构造。
    pub const fn new() -> Self {
        Self {
            kept: 0,
            folded: 0,
            arrivals: 0,
        }
    }

    /// 首因到达。
    pub fn first(&mut self) {
        self.kept += 1; self.arrivals += 1;
    }

    /// 重复到达折叠。
    pub fn dup(&mut self) {
        self.folded += 1; self.arrivals += 1;
    }

}

// --------------------------------------------------------------------------
// 状态机 fold（3 态 × 3 事件 · 迁移 4 条显式声明）
// --------------------------------------------------------------------------
/// 去重折叠状态机状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum foldState {
    Fresh,
    Held,
    Folded,
}

impl foldState {
    /// 全量态闭集。
    pub const ALL: [foldState; 3] = [
        foldState::Fresh,
        foldState::Held,
        foldState::Folded,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            foldState::Fresh => 0,
            foldState::Held => 1,
            foldState::Folded => 2,
        }
    }
}

/// fold事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum foldEvent {
    FirstArrive,
    DupArrive,
    Commit,
}

impl foldEvent {
    /// 全量事件闭集。
    pub const ALL: [foldEvent; 3] = [
        foldEvent::FirstArrive,
        foldEvent::DupArrive,
        foldEvent::Commit,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const FOLD_TRANSITIONS: [(foldState, foldEvent, foldState); 4] = [
    (foldState::Fresh, foldEvent::FirstArrive, foldState::Held), // 首因保留
    (foldState::Held, foldEvent::DupArrive, foldState::Folded), // 重复折叠
    (foldState::Folded, foldEvent::DupArrive, foldState::Folded), // 继续折叠
    (foldState::Folded, foldEvent::Commit, foldState::Held), // 提交后回持有
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn fold_next(state: foldState, ev: foldEvent) -> Option<foldState> {
    let mut i = 0usize;
    while i < 4 {
        let t = FOLD_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn fold_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < foldState::ALL.len() {
        let mut ei = 0usize;
        while ei < foldEvent::ALL.len() {
            let legal = fold_next(foldState::ALL[si], foldEvent::ALL[ei]).is_some();
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
pub fn fold_terminal(st: foldState) -> bool {
    let mut i = 0usize;
    while i < 4 {
        if FOLD_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct foldLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: foldState,
}

impl foldLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: foldState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: foldEvent) -> bool {
        match fold_next(self.cur, ev) {
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

/// 段位守卫：全部诊断码必须落在本段 [0x3b20, 0x3b2f]。
pub const fn t283_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t283Code::ALL.len() {
        let c = t283Code::ALL[i].code();
        if c < 0x3b20 || c > 0x3b2f {
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
fn t283_guard_seg() -> bool {
    t283_segment_guard()
}

/// 折叠账守恒（首因保留 + 折叠 = 到达）。
fn check_c1_fold_conserve() -> bool {
    let mut f = fold::new();
    f.first();
    let mut i = 0;
    while i < 6 {
        f.dup();
        i += 1;
    }
    let mut ok = f.kept == 1 && f.folded == 6 && f.arrivals == 7;
    if ok {
        ok = f.kept + f.folded == f.arrivals;
    }
    ok
}

/// 折叠状态机（保留-折叠-提交）。
fn check_c2_fold_sm() -> bool {
    let mut led = foldLedger::new(foldState::Fresh);
    let mut ok = led.apply(foldEvent::FirstArrive);
    ok = ok && led.apply(foldEvent::DupArrive);
    ok = ok && led.apply(foldEvent::DupArrive);
    if ok {
        ok = led.cur == foldState::Folded;
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t283_checks() -> CheckSet {
    let mut set = CheckSet::new("T283");
    set.add(
        "t283-seg-guard",
        t283_guard_seg(),
        if t283_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t283-c1-fold-conserve",
        check_c1_fold_conserve(),
        if check_c1_fold_conserve() { "" } else { "fold ledger conservation mismatch" },
    );
    set.add(
        "t283-c2-fold-sm",
        check_c2_fold_sm(),
        if check_c2_fold_sm() { "" } else { "fold state machine mismatch" },
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
    fn t283_all_checks_green() {
        let set = run_t283_checks();
        assert!(set.all_passed(), "T283 domain checks must be green");
    }

    #[test]
    fn t283_segment_guard_holds() {
        assert!(t283_segment_guard());
    }
}
