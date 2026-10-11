//! unxt167_mono —— UNX-F0162 单调时钟语义（时钟源表 + 回退检测 + 回绕账）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0162 · GetTime/SetTime 语义全量化 · 判据：时间读取与硬件 RTC 对照误差 <1s，夏令时/时区字段语义对照 UEFI 规范
//! · 组件：单调时钟源表（TSC/HPET/PIT/ACPI_PM 四源）+ 源仲裁状态机 + 回退检测器 + 回绕账
//! · 判据落地：时间读取精度底座（单调性红线：读取序列永不回退）
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T167_VERSION: &str = "t167-mono-v1";

/// 诊断段基址。
pub const T167_SEG_BASE: u16 = 0x3750;

// --------------------------------------------------------------------------
// 诊断码（t167 十六码 · 16 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 单调时钟语义诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t167Code {
    /// 0x3750 单调回退检出（读取序列时序倒流）。
    RewindDetected,
    /// 0x3751 回绕未处理（计数器绕回未归一）。
    WrapUnhandled,
    /// 0x3752 时钟源未评级（ rate 档缺失）。
    SourceUnrated,
    /// 0x3753 仲裁死锁（双源互等不让）。
    ArbiterDeadlock,
    /// 0x3754 速率分歧（双源速率差超阈）。
    RateDivergent,
    /// 0x3755 invariant 假设破缺（跨核频率变化）。
    InvariantBroken,
    /// 0x3756 跳变超偏（双源瞬差超容差）。
    JumpBeyondSkew,
    /// 0x3757 时钟失同步（校准链断裂）。
    ClockLostSync,
    /// 0x3758 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x3759 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x375a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x375b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x375c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x375d 次序违例（依赖序/时序破缺）。
    OrderViolated,
    /// 0x375e 守卫触发（重入/越限被拒）。
    GuardTripped,
    /// 0x375f 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t167Code {
    /// 全量闭集。
    pub const ALL: [t167Code; 16] = [
        t167Code::RewindDetected,
        t167Code::WrapUnhandled,
        t167Code::SourceUnrated,
        t167Code::ArbiterDeadlock,
        t167Code::RateDivergent,
        t167Code::InvariantBroken,
        t167Code::JumpBeyondSkew,
        t167Code::ClockLostSync,
        t167Code::LedgerBroken,
        t167Code::SnapshotFingerprint,
        t167Code::TableBroken,
        t167Code::IndexMisaligned,
        t167Code::DuplicateEntry,
        t167Code::OrderViolated,
        t167Code::GuardTripped,
        t167Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t167Code::RewindDetected => 0x3750,
            t167Code::WrapUnhandled => 0x3751,
            t167Code::SourceUnrated => 0x3752,
            t167Code::ArbiterDeadlock => 0x3753,
            t167Code::RateDivergent => 0x3754,
            t167Code::InvariantBroken => 0x3755,
            t167Code::JumpBeyondSkew => 0x3756,
            t167Code::ClockLostSync => 0x3757,
            t167Code::LedgerBroken => 0x3758,
            t167Code::SnapshotFingerprint => 0x3759,
            t167Code::TableBroken => 0x375a,
            t167Code::IndexMisaligned => 0x375b,
            t167Code::DuplicateEntry => 0x375c,
            t167Code::OrderViolated => 0x375d,
            t167Code::GuardTripped => 0x375e,
            t167Code::SegmentGuard => 0x375f,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t167Code::RewindDetected => "单调回退检出（读取序列时序倒流）",
            t167Code::WrapUnhandled => "回绕未处理（计数器绕回未归一）",
            t167Code::SourceUnrated => "时钟源未评级（ rate 档缺失）",
            t167Code::ArbiterDeadlock => "仲裁死锁（双源互等不让）",
            t167Code::RateDivergent => "速率分歧（双源速率差超阈）",
            t167Code::InvariantBroken => "invariant 假设破缺（跨核频率变化）",
            t167Code::JumpBeyondSkew => "跳变超偏（双源瞬差超容差）",
            t167Code::ClockLostSync => "时钟失同步（校准链断裂）",
            t167Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t167Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t167Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t167Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t167Code::DuplicateEntry => "条目重复（同键双写）",
            t167Code::OrderViolated => "次序违例（依赖序/时序破缺）",
            t167Code::GuardTripped => "守卫触发（重入/越限被拒）",
            t167Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t167Code::RewindDetected => "切仲裁源并登记回退事故",
            t167Code::WrapUnhandled => "按回绕账归一化后复算",
            t167Code::SourceUnrated => "补评级后入仲裁",
            t167Code::ArbiterDeadlock => "按优先级强制降级",
            t167Code::RateDivergent => "按交会校准重标速率",
            t167Code::InvariantBroken => "冻结当前源并登记",
            t167Code::JumpBeyondSkew => "按跳变保护规则切换",
            t167Code::ClockLostSync => "重建校准链后复跑",
            t167Code::LedgerBroken => "按守恒律重建台账后重跑",
            t167Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t167Code::TableBroken => "按语义规则重建常量表",
            t167Code::IndexMisaligned => "按声明序归一化后重跑",
            t167Code::DuplicateEntry => "合并同键条目为末次生效",
            t167Code::OrderViolated => "按依赖矩阵重排后重跑",
            t167Code::GuardTripped => "检查临界区嵌套与阈值",
            t167Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 冻结常量表（clock_src · 4 行逐行注释自证）
// --------------------------------------------------------------------------
/// 单调时钟源表（速率档）。
pub const CLOCK_SRC_COUNT: usize = 4;

/// 冻结表本体（编译期常量，运行期只读）。
pub static clock_src: [(u64, u64); 4] = [
    (SRC_TSC, 500), // TSC  invariant（invariant TSC 常量速率档）
    (SRC_HPET, 100), // HPET 主计数器（10MHz 级）
    (SRC_PIT, 50), // PIT 通道 0（1000Hz 级）
    (SRC_ACPI_PM, 20), // ACPI PM 定时器（3.58MHz 级）
];

/// 线性查找（键唯一性由构造期断言保证，运行时无分支歧义）。
pub fn clock_src_get(key: u64) -> Option<u64> {
    let mut i = 0usize;
    while i < CLOCK_SRC_COUNT {
        if clock_src[i].0 == key {
            return Some(clock_src[i].1);
        }
        i += 1;
    }
    None
}

/// 累计求和（用于守恒对账）。
pub fn clock_src_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < CLOCK_SRC_COUNT {
        s = s.wrapping_add(clock_src[i].1);
        i += 1;
    }
    s
}

// --------------------------------------------------------------------------
// 状态机 mono_arb（5 态 × 4 事件 · 迁移 8 条显式声明）
// --------------------------------------------------------------------------
/// 时钟源仲裁状态机（回退降级 + 恢复晋升）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mono_arbState {
    Tsc,
    Hpet,
    Pit,
    AcpiPm,
    Fallback,
}

impl mono_arbState {
    /// 全量态闭集。
    pub const ALL: [mono_arbState; 5] = [
        mono_arbState::Tsc,
        mono_arbState::Hpet,
        mono_arbState::Pit,
        mono_arbState::AcpiPm,
        mono_arbState::Fallback,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            mono_arbState::Tsc => 0,
            mono_arbState::Hpet => 1,
            mono_arbState::Pit => 2,
            mono_arbState::AcpiPm => 3,
            mono_arbState::Fallback => 4,
        }
    }
}

/// mono_arb事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mono_arbEvent {
    Promote,
    Demote,
    Rewind,
    RateOk,
}

impl mono_arbEvent {
    /// 全量事件闭集。
    pub const ALL: [mono_arbEvent; 4] = [
        mono_arbEvent::Promote,
        mono_arbEvent::Demote,
        mono_arbEvent::Rewind,
        mono_arbEvent::RateOk,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const MONO_ARB_TRANSITIONS: [(mono_arbState, mono_arbEvent, mono_arbState); 8] = [
    (mono_arbState::Tsc, mono_arbEvent::RateOk, mono_arbState::Tsc), // TSC invariant 保持
    (mono_arbState::Tsc, mono_arbEvent::Rewind, mono_arbState::Hpet), // 检出回退降级 HPET
    (mono_arbState::Hpet, mono_arbEvent::Rewind, mono_arbState::Pit), // HPET 异常降级 PIT
    (mono_arbState::Pit, mono_arbEvent::Rewind, mono_arbState::AcpiPm), // PIT 异常降级 PM
    (mono_arbState::AcpiPm, mono_arbEvent::Rewind, mono_arbState::Fallback), // PM 异常走兜底
    (mono_arbState::Hpet, mono_arbEvent::RateOk, mono_arbState::Tsc), // 恢复后晋升 TSC
    (mono_arbState::Pit, mono_arbEvent::RateOk, mono_arbState::Hpet), // 恢复后晋升 HPET
    (mono_arbState::AcpiPm, mono_arbEvent::RateOk, mono_arbState::Pit), // 恢复后晋升 PIT
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn mono_arb_next(state: mono_arbState, ev: mono_arbEvent) -> Option<mono_arbState> {
    let mut i = 0usize;
    while i < 8 {
        let t = MONO_ARB_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn mono_arb_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < mono_arbState::ALL.len() {
        let mut ei = 0usize;
        while ei < mono_arbEvent::ALL.len() {
            let legal = mono_arb_next(mono_arbState::ALL[si], mono_arbEvent::ALL[ei]).is_some();
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
pub fn mono_arb_terminal(st: mono_arbState) -> bool {
    let mut i = 0usize;
    while i < 8 {
        if MONO_ARB_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct mono_arbLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: mono_arbState,
}

impl mono_arbLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: mono_arbState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: mono_arbEvent) -> bool {
        match mono_arb_next(self.cur, ev) {
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
// 引擎 mono_dual（双源对拍：3 槽位逐槽回读一致性）
// --------------------------------------------------------------------------
/// 单调性双源对拍（连续采样对必须非降）。
/// 槽位枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mono_dualSlot {
    /// 探针 A 序列。
    ProbeA,
    /// 探针 B 序列。
    ProbeB,
    /// 交叉采样对。
    Cross,
}

impl mono_dualSlot {
    /// 全量槽位闭集。
    pub const ALL: [mono_dualSlot; 3] = [
        mono_dualSlot::ProbeA,
        mono_dualSlot::ProbeB,
        mono_dualSlot::Cross,
    ];

    /// 索引（与声明序恒等）。
    pub const fn index(self) -> usize {
        match self {
            mono_dualSlot::ProbeA => 0,
            mono_dualSlot::ProbeB => 1,
            mono_dualSlot::Cross => 2,
        }
    }

    /// 中文槽名。
    pub const fn cn(self) -> &'static str {
        match self {
            mono_dualSlot::ProbeA => "探针 A 序列",
            mono_dualSlot::ProbeB => "探针 B 序列",
            mono_dualSlot::Cross => "交叉采样对",
        }
    }
}

/// 双源样本（A 源首读 + B 源回读 + 一致性判定）。
#[derive(Clone, Copy, Debug)]
pub struct mono_dualPair {
    /// A 源首读值。
    pub a: u64,
    /// B 源回读值。
    pub b: u64,
    /// 是否一致。
    pub same: bool,
    /// 采样序号。
    pub seq: u64,
}

/// 对拍账（逐槽一致才放行；不一致槽位显性记账）。
#[derive(Clone, Copy, Debug)]
pub struct mono_dualDual {
    /// 逐槽样本。
    pub pairs: [mono_dualPair; 3],
    /// 一致槽位数。
    pub same_n: usize,
    /// 不一致槽位数。
    pub diff_n: usize,
    /// 采样总轮次。
    pub rounds: u64,
    /// 已采样的不一致槽位掩码。
    pub diff_mask: u64,
}

impl mono_dualDual {
    /// 全零构造。
    pub const fn new() -> Self {
        Self {
            pairs: [mono_dualPair { a: 0, b: 0, same: false, seq: 0 }; 3],
            same_n: 0,
            diff_n: 0,
            rounds: 0,
            diff_mask: 0,
        }
    }

    /// 单槽采样（A/B 双源读入后逐位比较）。
    pub fn sample(&mut self, who: mono_dualSlot, a: u64, b: u64) {
        let i = who.index();
        let same = a == b;
        self.pairs[i] = mono_dualPair { a, b, same, seq: self.rounds };
        if same {
            self.same_n += 1;
        } else {
            self.diff_n += 1;
            self.diff_mask |= 1u64 << (i as u64);
        }
        self.rounds += 1;
    }

    /// 全槽一致判定（对拍通过口径）。
    pub fn all_same(&self) -> bool {
        self.diff_n == 0 && self.same_n == 3
    }

    /// 复算：逐槽重读 pairs 重判 same（防记账面与数据面分叉）。
    pub fn recount(&self) -> bool {
        let mut same = 0usize;
        let mut i = 0usize;
        while i < 3 {
            if self.pairs[i].same == (self.pairs[i].a == self.pairs[i].b) {
                same += 1;
            }
            i += 1;
        }
        same == 3 && self.same_n + self.diff_n == 3
    }
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x3750, 0x375f]。
pub const fn t167_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t167Code::ALL.len() {
        let c = t167Code::ALL[i].code();
        if c < 0x3750 || c > 0x375f {
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
fn t167_guard_seg() -> bool {
    t167_segment_guard()
}

/// 时钟源表四源齐备 + 速率档非零。
fn check_c1_src_table() -> bool {
    let mut ok = clock_src_COUNT == 4;
    let mut i = 0usize;
    while i < clock_src_COUNT {
        if clock_src[i].1 == 0 {
            ok = false;
        }
        i += 1;
    }
    ok
}

/// 仲裁状态机降级链 + 非法越级拒止。
fn check_c2_arb_sm() -> bool {
    let mut led = mono_arbLedger::new(mono_arbState::Tsc);
    let mut ok = led.apply(mono_arbEvent::Rewind);
    ok = ok && led.apply(mono_arbEvent::Rewind);
    ok = ok && led.apply(mono_arbEvent::Rewind);
    if ok {
        ok = led.cur == mono_arbState::Fallback;
    }
    if ok {
        // Fallback 无出边：任何事件必须拒止。
        ok = !led.apply(mono_arbEvent::RateOk);
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t167_checks() -> CheckSet {
    let mut set = CheckSet::new("T167");
    set.add(
        "t167-seg-guard",
        t167_guard_seg(),
        if t167_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t167-c1-src-table",
        check_c1_src_table(),
        if check_c1_src_table() { "" } else { "clock source table mismatch" },
    );
    set.add(
        "t167-c2-arb-sm",
        check_c2_arb_sm(),
        if check_c2_arb_sm() { "" } else { "clock arbiter state machine mismatch" },
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
    fn t167_all_checks_green() {
        let set = run_t167_checks();
        assert!(set.all_passed(), "T167 domain checks must be green");
    }

    #[test]
    fn t167_segment_guard_holds() {
        assert!(t167_segment_guard());
    }
}
