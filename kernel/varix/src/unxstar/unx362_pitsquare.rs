//! unxt362_pitsquare —— UNX-F0401 方波模式语义（占空比与计数）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0401 · PIT 通道 0 编程接管（F0115 升级） · 判据：PIT 以 1000Hz 方波接管，中断计数 5 秒实差 ≤2 次（±0.04%）
//! · 组件：方波状态机 + 计数账（5 秒 ≤2 次红线）+ 直方图
//! · 判据落地：1000Hz 方波接管的计数语义
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T362_VERSION: &str = "t362-pitsquare-v1";

/// 诊断段基址。
pub const T362_SEG_BASE: u16 = 0x4110;

// --------------------------------------------------------------------------
// 诊断码（t362 十六码 · 15 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 方波语义诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t362Code {
    /// 0x4110 计数漂移（5 秒偏差 >2 次）。
    CountDrift,
    /// 0x4111 非方波模式（占空比非 50%）。
    ModeNotSquare,
    /// 0x4112 重装丢失（计数器归零异常）。
    ReloadLost,
    /// 0x4113 节拍账破缺。
    TickLedgerBreak,
    /// 0x4114 速率减半（误配偶数装填）。
    RateHalf,
    /// 0x4115 窗口溢出。
    WindowOverflow,
    /// 0x4116 报告陈旧。
    ReportStale,
    /// 0x4117 守卫触发。
    GuardTripped,
    /// 0x4118 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x4119 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x411a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x411b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x411c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x411d 次序违例（依赖序/时序破缺）。
    OrderViolated,
    /// 0x411e 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t362Code {
    /// 全量闭集。
    pub const ALL: [t362Code; 15] = [
        t362Code::CountDrift,
        t362Code::ModeNotSquare,
        t362Code::ReloadLost,
        t362Code::TickLedgerBreak,
        t362Code::RateHalf,
        t362Code::WindowOverflow,
        t362Code::ReportStale,
        t362Code::GuardTripped,
        t362Code::LedgerBroken,
        t362Code::SnapshotFingerprint,
        t362Code::TableBroken,
        t362Code::IndexMisaligned,
        t362Code::DuplicateEntry,
        t362Code::OrderViolated,
        t362Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t362Code::CountDrift => 0x4110,
            t362Code::ModeNotSquare => 0x4111,
            t362Code::ReloadLost => 0x4112,
            t362Code::TickLedgerBreak => 0x4113,
            t362Code::RateHalf => 0x4114,
            t362Code::WindowOverflow => 0x4115,
            t362Code::ReportStale => 0x4116,
            t362Code::GuardTripped => 0x4117,
            t362Code::LedgerBroken => 0x4118,
            t362Code::SnapshotFingerprint => 0x4119,
            t362Code::TableBroken => 0x411a,
            t362Code::IndexMisaligned => 0x411b,
            t362Code::DuplicateEntry => 0x411c,
            t362Code::OrderViolated => 0x411d,
            t362Code::SegmentGuard => 0x411e,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t362Code::CountDrift => "计数漂移（5 秒偏差 >2 次）",
            t362Code::ModeNotSquare => "非方波模式（占空比非 50%）",
            t362Code::ReloadLost => "重装丢失（计数器归零异常）",
            t362Code::TickLedgerBreak => "节拍账破缺",
            t362Code::RateHalf => "速率减半（误配偶数装填）",
            t362Code::WindowOverflow => "窗口溢出",
            t362Code::ReportStale => "报告陈旧",
            t362Code::GuardTripped => "守卫触发",
            t362Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t362Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t362Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t362Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t362Code::DuplicateEntry => "条目重复（同键双写）",
            t362Code::OrderViolated => "次序违例（依赖序/时序破缺）",
            t362Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t362Code::CountDrift => "按误差预算校正",
            t362Code::ModeNotSquare => "按方波模式重配",
            t362Code::ReloadLost => "按重装路径补装",
            t362Code::TickLedgerBreak => "按守恒重建",
            t362Code::RateHalf => "按奇偶处理规则校正",
            t362Code::WindowOverflow => "分窗统计",
            t362Code::ReportStale => "重渲染",
            t362Code::GuardTripped => "检查临界区",
            t362Code::LedgerBroken => "按守恒律重建台账后重跑",
            t362Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t362Code::TableBroken => "按语义规则重建常量表",
            t362Code::IndexMisaligned => "按声明序归一化后重跑",
            t362Code::DuplicateEntry => "合并同键条目为末次生效",
            t362Code::OrderViolated => "按依赖矩阵重排后重跑",
            t362Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 状态机 sq（4 态 × 4 事件 · 迁移 5 条显式声明）
// --------------------------------------------------------------------------
/// 方波节拍状态机状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum sqState {
    Idle,
    Armed,
    Counting,
    Reloading,
}

impl sqState {
    /// 全量态闭集。
    pub const ALL: [sqState; 4] = [
        sqState::Idle,
        sqState::Armed,
        sqState::Counting,
        sqState::Reloading,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            sqState::Idle => 0,
            sqState::Armed => 1,
            sqState::Counting => 2,
            sqState::Reloading => 3,
        }
    }
}

/// sq事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum sqEvent {
    Program,
    FirstTick,
    Tick,
    Reload,
}

impl sqEvent {
    /// 全量事件闭集。
    pub const ALL: [sqEvent; 4] = [
        sqEvent::Program,
        sqEvent::FirstTick,
        sqEvent::Tick,
        sqEvent::Reload,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const SQ_TRANSITIONS: [(sqState, sqEvent, sqState); 5] = [
    (sqState::Idle, sqEvent::Program, sqState::Armed), // 编程通道 0
    (sqState::Armed, sqEvent::FirstTick, sqState::Counting), // 首次倒计时到零
    (sqState::Counting, sqEvent::Tick, sqState::Counting), // 持续翻转计数
    (sqState::Counting, sqEvent::Reload, sqState::Reloading), // 到零重装
    (sqState::Reloading, sqEvent::Tick, sqState::Counting), // 重装后续计
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn sq_next(state: sqState, ev: sqEvent) -> Option<sqState> {
    let mut i = 0usize;
    while i < 5 {
        let t = SQ_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn sq_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < sqState::ALL.len() {
        let mut ei = 0usize;
        while ei < sqEvent::ALL.len() {
            let legal = sq_next(sqState::ALL[si], sqEvent::ALL[ei]).is_some();
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
pub fn sq_terminal(st: sqState) -> bool {
    let mut i = 0usize;
    while i < 5 {
        if SQ_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct sqLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: sqState,
}

impl sqLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: sqState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: sqEvent) -> bool {
        match sq_next(self.cur, ev) {
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
// 守恒账 tick（节拍账（5 秒窗：期望 5000 次 ± 红线 2））
// --------------------------------------------------------------------------
/// 节拍账（5 秒窗：期望 5000 次 ± 红线 2）。
#[derive(Clone, Copy, Debug)]
pub struct tick {
    /// 实计中断数
    pub ticks: u64,
    /// 期望中断数
    pub expected: u64,
    /// 红线偏差
    pub redline: u64,
}

impl tick {
    /// 全零构造。
    pub const fn new() -> Self {
        Self {
            ticks: 0,
            expected: 0,
            redline: 0,
        }
    }

    /// 记一次中断。
    pub fn record_tick(&mut self) {
        self.ticks += 1;
    }

    /// 装备 5 秒窗（期望 5000 次）。
    pub fn arm_5s(&mut self) {
        self.expected += 5000; self.redline += 2;
    }

}

// --------------------------------------------------------------------------
// 直方图引擎 tick_hist（有界插入排序 + 分位数 P50/P90/P99）
// --------------------------------------------------------------------------
/// 节拍间隔直方图（ns 档分位数）。
/// 样本容量上限。
pub const TICK_HIST_CAP: usize = 64;

/// 直方图账本（排序态 + 五统量）。
#[derive(Clone, Copy, Debug)]
pub struct tick_histHist {
    /// 样本数组（插入排序维护有序）。
    pub samples: [u64; 64],
    /// 样本数。
    pub n: usize,
    /// 溢出丢弃计数。
    pub drops: u64,
    /// 最小值（空时为 0）。
    pub min: u64,
    /// 最大值。
    pub max: u64,
    /// 累加和。
    pub sum: u64,
}

impl tick_histHist {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { samples: [0; 64], n: 0, drops: 0, min: 0, max: 0, sum: 0 }
    }

    /// 采样（有界插入排序；满员丢弃并记账）。
    pub fn push(&mut self, v: u64) {
        if self.n >= TICK_HIST_CAP {
            self.drops += 1;
            return;
        }
        let mut j = self.n;
        while j > 0 && self.samples[j - 1] > v {
            self.samples[j] = self.samples[j - 1];
            j -= 1;
        }
        self.samples[j] = v;
        self.n += 1;
        self.sum = self.sum.wrapping_add(v);
        if self.n == 1 || v < self.min {
            self.min = v;
        }
        if v > self.max {
            self.max = v;
        }
    }

    /// 分位数（permille 口径：q500=P50/q900=P90/q990=P99；最近秩法）。
    pub fn percentile(&self, permille: u64) -> u64 {
        if self.n == 0 {
            return 0;
        }
        let rank = (permille * self.n as u64 + 999) / 1000;
        let idx = if rank == 0 { 1 } else { rank };
        let i = if idx as usize > self.n { self.n - 1 } else { idx as usize - 1 };
        self.samples[i]
    }

    /// 均值（整数除，余数入账）。
    pub fn mean(&self) -> u64 {
        if self.n == 0 {
            return 0;
        }
        self.sum / self.n as u64
    }

    /// 守恒：sum == 各样本和（复算）。
    pub fn conserved(&self) -> bool {
        let mut s = 0u64;
        let mut i = 0usize;
        while i < self.n {
            s = s.wrapping_add(self.samples[i]);
            i += 1;
        }
        s == self.sum
    }
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x4110, 0x411f]。
pub const fn t362_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t362Code::ALL.len() {
        let c = t362Code::ALL[i].code();
        if c < 0x4110 || c > 0x411f {
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
fn t362_guard_seg() -> bool {
    t362_segment_guard()
}

/// 节拍账红线（±2 次内判定）。
fn check_c1_tick_ledger() -> bool {
    let mut t = tick::new();
    t.arm_5s();
    let mut i = 0;
    while i < 4999 {
        t.record_tick();
        i += 1;
    }
    let mut ok = t.ticks + t.redline >= t.expected;
    if ok {
        // 4999 次在红线内（差 1 ≤ 2）。
        ok = t.expected - t.ticks == 1 && t.expected - t.ticks <= t.redline;
    }
    if ok {
        t.record_tick();
        ok = t.ticks == t.expected;
    }
    ok
}

/// 节拍状态机（编程-计数-重装循环）。
fn check_c2_sq_sm() -> bool {
    let mut led = sqLedger::new(sqState::Idle);
    let mut ok = led.apply(sqEvent::Program);
    ok = ok && led.apply(sqEvent::FirstTick);
    ok = ok && led.apply(sqEvent::Reload);
    ok = ok && led.apply(sqEvent::Tick);
    if ok {
        ok = led.cur == sqState::Counting && led.legal == 4;
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t362_checks() -> CheckSet {
    let mut set = CheckSet::new("T362");
    set.add(
        "t362-seg-guard",
        t362_guard_seg(),
        if t362_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t362-c1-tick-ledger",
        check_c1_tick_ledger(),
        if check_c1_tick_ledger() { "" } else { "tick redline mismatch" },
    );
    set.add(
        "t362-c2-sq-sm",
        check_c2_sq_sm(),
        if check_c2_sq_sm() { "" } else { "square wave state machine mismatch" },
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
    fn t362_all_checks_green() {
        let set = run_t362_checks();
        assert!(set.all_passed(), "T362 domain checks must be green");
    }

    #[test]
    fn t362_segment_guard_holds() {
        assert!(t362_segment_guard());
    }
}
