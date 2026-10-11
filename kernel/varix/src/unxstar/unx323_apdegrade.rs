//! unxt323_apdegrade —— UNX-F0341 降级路径（超时未达的兜底）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0341 · AP 汇合栅栏（全部到达点同步） · 判据：栅栏语义（全员到达放行）正确，部分未达超时路径有账
//! · 组件：降级状态机 + 降级账（放行+超时守恒）+ 重试路径
//! · 判据落地：超时路径有账
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T323_VERSION: &str = "t323-apdegrade-v1";

/// 诊断段基址。
pub const T323_SEG_BASE: u16 = 0x3e20;

// --------------------------------------------------------------------------
// 诊断码（t323 十六码 · 14 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 降级路径诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t323Code {
    /// 0x3e20 重复降级。
    DegradeTwice,
    /// 0x3e21 重试超限。
    RetryOver,
    /// 0x3e22 降级账破缺。
    LedgerBreak,
    /// 0x3e23 降级序违例。
    OrderViolated,
    /// 0x3e24 降级后仍判全员。
    QuorumAfterDegrade,
    /// 0x3e25 报告陈旧。
    ReportStale,
    /// 0x3e26 脱敏缺失。
    MaskMissing,
    /// 0x3e27 守卫触发。
    GuardTripped,
    /// 0x3e28 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x3e29 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x3e2a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x3e2b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x3e2c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x3e2d 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t323Code {
    /// 全量闭集。
    pub const ALL: [t323Code; 14] = [
        t323Code::DegradeTwice,
        t323Code::RetryOver,
        t323Code::LedgerBreak,
        t323Code::OrderViolated,
        t323Code::QuorumAfterDegrade,
        t323Code::ReportStale,
        t323Code::MaskMissing,
        t323Code::GuardTripped,
        t323Code::LedgerBroken,
        t323Code::SnapshotFingerprint,
        t323Code::TableBroken,
        t323Code::IndexMisaligned,
        t323Code::DuplicateEntry,
        t323Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t323Code::DegradeTwice => 0x3e20,
            t323Code::RetryOver => 0x3e21,
            t323Code::LedgerBreak => 0x3e22,
            t323Code::OrderViolated => 0x3e23,
            t323Code::QuorumAfterDegrade => 0x3e24,
            t323Code::ReportStale => 0x3e25,
            t323Code::MaskMissing => 0x3e26,
            t323Code::GuardTripped => 0x3e27,
            t323Code::LedgerBroken => 0x3e28,
            t323Code::SnapshotFingerprint => 0x3e29,
            t323Code::TableBroken => 0x3e2a,
            t323Code::IndexMisaligned => 0x3e2b,
            t323Code::DuplicateEntry => 0x3e2c,
            t323Code::SegmentGuard => 0x3e2d,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t323Code::DegradeTwice => "重复降级",
            t323Code::RetryOver => "重试超限",
            t323Code::LedgerBreak => "降级账破缺",
            t323Code::OrderViolated => "降级序违例",
            t323Code::QuorumAfterDegrade => "降级后仍判全员",
            t323Code::ReportStale => "报告陈旧",
            t323Code::MaskMissing => "脱敏缺失",
            t323Code::GuardTripped => "守卫触发",
            t323Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t323Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t323Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t323Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t323Code::DuplicateEntry => "条目重复（同键双写）",
            t323Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t323Code::DegradeTwice => "幂等拒止",
            t323Code::RetryOver => "按上限登记失败",
            t323Code::LedgerBreak => "按守恒重建",
            t323Code::OrderViolated => "按序重排",
            t323Code::QuorumAfterDegrade => "按降级口径重判",
            t323Code::ReportStale => "重渲染",
            t323Code::MaskMissing => "补打码",
            t323Code::GuardTripped => "检查临界区",
            t323Code::LedgerBroken => "按守恒律重建台账后重跑",
            t323Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t323Code::TableBroken => "按语义规则重建常量表",
            t323Code::IndexMisaligned => "按声明序归一化后重跑",
            t323Code::DuplicateEntry => "合并同键条目为末次生效",
            t323Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 状态机 deg（4 态 × 4 事件 · 迁移 5 条显式声明）
// --------------------------------------------------------------------------
/// 降级状态机（超时-重试-放弃）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum degState {
    Waiting,
    Complete,
    Degraded,
    Failed,
}

impl degState {
    /// 全量态闭集。
    pub const ALL: [degState; 4] = [
        degState::Waiting,
        degState::Complete,
        degState::Degraded,
        degState::Failed,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            degState::Waiting => 0,
            degState::Complete => 1,
            degState::Degraded => 2,
            degState::Failed => 3,
        }
    }
}

/// deg事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum degEvent {
    AllArrive,
    Timeout,
    Retry,
    GiveUp,
}

impl degEvent {
    /// 全量事件闭集。
    pub const ALL: [degEvent; 4] = [
        degEvent::AllArrive,
        degEvent::Timeout,
        degEvent::Retry,
        degEvent::GiveUp,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const DEG_TRANSITIONS: [(degState, degEvent, degState); 5] = [
    (degState::Waiting, degEvent::AllArrive, degState::Complete), // 全员到达放行
    (degState::Waiting, degEvent::Timeout, degState::Degraded), // 超时降级
    (degState::Degraded, degEvent::Retry, degState::Waiting), // 降级后重试一轮
    (degState::Degraded, degEvent::GiveUp, degState::Failed), // 放弃并登记
    (degState::Waiting, degEvent::GiveUp, degState::Failed), // 直接放弃
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn deg_next(state: degState, ev: degEvent) -> Option<degState> {
    let mut i = 0usize;
    while i < 5 {
        let t = DEG_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn deg_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < degState::ALL.len() {
        let mut ei = 0usize;
        while ei < degEvent::ALL.len() {
            let legal = deg_next(degState::ALL[si], degEvent::ALL[ei]).is_some();
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
pub fn deg_terminal(st: degState) -> bool {
    let mut i = 0usize;
    while i < 5 {
        if DEG_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct degLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: degState,
}

impl degLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: degState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: degEvent) -> bool {
        match deg_next(self.cur, ev) {
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

// --------------------------------------------------------------------------
// 守恒账 deg（降级账（完整 + 降级 + 失败 == 总轮））
// --------------------------------------------------------------------------
/// 降级账（完整 + 降级 + 失败 == 总轮）。
#[derive(Clone, Copy, Debug)]
pub struct deg {
    /// 完整放行轮数
    pub complete: u64,
    /// 降级轮数
    pub degraded: u64,
    /// 失败轮数
    pub failed: u64,
    /// 总轮数
    pub rounds: u64,
}

impl deg {
    /// 全零构造。
    pub const fn new() -> Self {
        Self {
            complete: 0,
            degraded: 0,
            failed: 0,
            rounds: 0,
        }
    }

    /// 完整放行。
    pub fn round_complete(&mut self) {
        self.complete += 1; self.rounds += 1;
    }

    /// 降级轮。
    pub fn round_degrade(&mut self) {
        self.degraded += 1; self.rounds += 1;
    }

    /// 失败轮。
    pub fn round_fail(&mut self) {
        self.failed += 1; self.rounds += 1;
    }

}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x3e20, 0x3e2f]。
pub const fn t323_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t323Code::ALL.len() {
        let c = t323Code::ALL[i].code();
        if c < 0x3e20 || c > 0x3e2f {
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
fn t323_guard_seg() -> bool {
    t323_segment_guard()
}

/// 降级状态机（超时降级-重试-放行）。
fn check_c1_deg_sm() -> bool {
    let mut led = degLedger::new(degState::Waiting);
    let mut ok = led.apply(degEvent::Timeout);
    ok = ok && led.apply(degEvent::Retry);
    ok = ok && led.apply(degEvent::AllArrive);
    if ok {
        ok = led.cur == degState::Complete;
    }
    ok
}

/// 降级账守恒（三种终态合计 = 总轮）。
fn check_c2_deg_ledger() -> bool {
    let mut d = deg::new();
    d.round_complete();
    d.round_degrade();
    d.round_degrade();
    d.round_fail();
    d.round_complete();
    let mut ok = d.rounds == 5;
    if ok {
        ok = d.complete + d.degraded + d.failed == d.rounds;
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t323_checks() -> CheckSet {
    let mut set = CheckSet::new("T323");
    set.add(
        "t323-seg-guard",
        t323_guard_seg(),
        if t323_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t323-c1-deg-sm",
        check_c1_deg_sm(),
        if check_c1_deg_sm() { "" } else { "degrade state machine mismatch" },
    );
    set.add(
        "t323-c2-deg-ledger",
        check_c2_deg_ledger(),
        if check_c2_deg_ledger() { "" } else { "degrade ledger conservation mismatch" },
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
    fn t323_all_checks_green() {
        let set = run_t323_checks();
        assert!(set.all_passed(), "T323 domain checks must be green");
    }

    #[test]
    fn t323_segment_guard_holds() {
        assert!(t323_segment_guard());
    }
}
