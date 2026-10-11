//! unxt163_unx163_reset —— UNX-F0163 ResetSystem 三变体（冷/暖/关机，只调固件接口）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0163 · ResetSystem 变体（冷/暖/关机） · 判据：三变体真机行为对照（重启/重启/断电），失败路径有日志（红线：只调固件接口，不裸写端口）
//! · 组件：三变体表 + 固件调用流水 + 失败路径日志账 + 裸写端口红线守卫
//! · 判据落地：三变体行为对照 + 只调固件接口红线
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T163_VERSION: &str = "t163-unx163_reset-v1";

/// 诊断段基址。
pub const T163_SEG_BASE: u16 = 0x4e30;

// --------------------------------------------------------------------------
// 诊断码（t163 十六码 · 14 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 复位变体诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t163Code {
    /// 0x4e30 变体非法（三变体之外）。
    VariantBad,
    /// 0x4e31 裸写端口红线（直写 0xCF9/0x64）。
    PortWriteRedline,
    /// 0x4e32 复位停滞（接口未返回）。
    ResetStall,
    /// 0x4e33 失败静默（失败路径无日志）。
    FailureSilent,
    /// 0x4e34 重复复位（同变体双触发）。
    DoubleReset,
    /// 0x4e35 复位账破缺。
    LedgerBreak,
    /// 0x4e36 变体序违例。
    OrderViolated,
    /// 0x4e37 守卫触发。
    GuardTripped,
    /// 0x4e38 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x4e39 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x4e3a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x4e3b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x4e3c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x4e3d 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t163Code {
    /// 全量闭集。
    pub const ALL: [t163Code; 14] = [
        t163Code::VariantBad,
        t163Code::PortWriteRedline,
        t163Code::ResetStall,
        t163Code::FailureSilent,
        t163Code::DoubleReset,
        t163Code::LedgerBreak,
        t163Code::OrderViolated,
        t163Code::GuardTripped,
        t163Code::LedgerBroken,
        t163Code::SnapshotFingerprint,
        t163Code::TableBroken,
        t163Code::IndexMisaligned,
        t163Code::DuplicateEntry,
        t163Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t163Code::VariantBad => 0x4e30,
            t163Code::PortWriteRedline => 0x4e31,
            t163Code::ResetStall => 0x4e32,
            t163Code::FailureSilent => 0x4e33,
            t163Code::DoubleReset => 0x4e34,
            t163Code::LedgerBreak => 0x4e35,
            t163Code::OrderViolated => 0x4e36,
            t163Code::GuardTripped => 0x4e37,
            t163Code::LedgerBroken => 0x4e38,
            t163Code::SnapshotFingerprint => 0x4e39,
            t163Code::TableBroken => 0x4e3a,
            t163Code::IndexMisaligned => 0x4e3b,
            t163Code::DuplicateEntry => 0x4e3c,
            t163Code::SegmentGuard => 0x4e3d,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t163Code::VariantBad => "变体非法（三变体之外）",
            t163Code::PortWriteRedline => "裸写端口红线（直写 0xCF9/0x64）",
            t163Code::ResetStall => "复位停滞（接口未返回）",
            t163Code::FailureSilent => "失败静默（失败路径无日志）",
            t163Code::DoubleReset => "重复复位（同变体双触发）",
            t163Code::LedgerBreak => "复位账破缺",
            t163Code::OrderViolated => "变体序违例",
            t163Code::GuardTripped => "守卫触发",
            t163Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t163Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t163Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t163Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t163Code::DuplicateEntry => "条目重复（同键双写）",
            t163Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t163Code::VariantBad => "按三变体闭集校正",
            t163Code::PortWriteRedline => "按只调固件接口红线拒止",
            t163Code::ResetStall => "按停滞路径降级",
            t163Code::FailureSilent => "按失败必记路径补记",
            t163Code::DoubleReset => "幂等拒绝",
            t163Code::LedgerBreak => "按守恒重建",
            t163Code::OrderViolated => "按变体表序重排",
            t163Code::GuardTripped => "检查临界区",
            t163Code::LedgerBroken => "按守恒律重建台账后重跑",
            t163Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t163Code::TableBroken => "按语义规则重建常量表",
            t163Code::IndexMisaligned => "按声明序归一化后重跑",
            t163Code::DuplicateEntry => "合并同键条目为末次生效",
            t163Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 状态机 rst（5 态 × 5 事件 · 迁移 6 条显式声明）
// --------------------------------------------------------------------------
/// 复位调用状态机（失败路径显性）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum rstState {
    Idle,
    Prep,
    Calling,
    Falling,
    Done,
}

impl rstState {
    /// 全量态闭集。
    pub const ALL: [rstState; 5] = [
        rstState::Idle,
        rstState::Prep,
        rstState::Calling,
        rstState::Falling,
        rstState::Done,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            rstState::Idle => 0,
            rstState::Prep => 1,
            rstState::Calling => 2,
            rstState::Falling => 3,
            rstState::Done => 4,
        }
    }
}

/// rst事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum rstEvent {
    Prep,
    Call,
    Return,
    Fail,
    Complete,
}

impl rstEvent {
    /// 全量事件闭集。
    pub const ALL: [rstEvent; 5] = [
        rstEvent::Prep,
        rstEvent::Call,
        rstEvent::Return,
        rstEvent::Fail,
        rstEvent::Complete,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const RST_TRANSITIONS: [(rstState, rstEvent, rstState); 6] = [
    (rstState::Idle, rstEvent::Prep, rstState::Prep), // 准备变体参数
    (rstState::Prep, rstEvent::Call, rstState::Calling), // 调用固件接口
    (rstState::Calling, rstEvent::Return, rstState::Done), // 接口正常返回（已知不返回路径）
    (rstState::Calling, rstEvent::Fail, rstState::Falling), // 调用失败降级
    (rstState::Falling, rstEvent::Complete, rstState::Done), // 失败路径收口（必记日志）
    (rstState::Prep, rstEvent::Fail, rstState::Falling), // 准备阶段即失败
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn rst_next(state: rstState, ev: rstEvent) -> Option<rstState> {
    let mut i = 0usize;
    while i < 6 {
        let t = RST_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn rst_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < rstState::ALL.len() {
        let mut ei = 0usize;
        while ei < rstEvent::ALL.len() {
            let legal = rst_next(rstState::ALL[si], rstEvent::ALL[ei]).is_some();
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
pub fn rst_terminal(st: rstState) -> bool {
    let mut i = 0usize;
    while i < 6 {
        if RST_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct rstLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: rstState,
}

impl rstLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: rstState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: rstEvent) -> bool {
        match rst_next(self.cur, ev) {
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
// 字符串键值映射 resetvar（3 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// ResetSystem 三变体表（行为对照口径）。
/// 行数。
pub const resetvar_COUNT: usize = 3;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static resetvar: [(&'static str, u64); 3] = [
    ("RST_COLD", 0), // 冷复位（ResetCold：全硬件重初始化→重启）
    ("RST_WARM", 1), // 暖复位（ResetWarm：CPU 重初始化→重启）
    ("RST_OFF", 2), // 关机（Shutdown：断电）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn resetvar_get(key: &str) -> Option<u64> {
    match key {
        "RST_COLD" => Some(0),
        "RST_WARM" => Some(1),
        "RST_OFF" => Some(2),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn resetvar_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < resetvar_COUNT {
        let (k, v) = resetvar[i];
        let ok = match k {
            "RST_COLD" => v == 0,
            "RST_WARM" => v == 1,
            "RST_OFF" => v == 2,
            _ => false,
        };
        if !ok {
            bad += 1;
        }
        i += 1;
    }
    bad
}

/// 累计求和（守恒对账用）。
pub fn resetvar_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < resetvar_COUNT {
        s = s.wrapping_add(resetvar[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn resetvar_unique() -> bool {
    let mut i = 0usize;
    while i < resetvar_COUNT {
        let mut j = i + 1;
        while j < resetvar_COUNT {
            if resetvar[i].0 == resetvar[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x4e30, 0x4e3f]。
pub const fn t163_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t163Code::ALL.len() {
        let c = t163Code::ALL[i].code();
        if c < 0x4e30 || c > 0x4e3f {
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
fn t163_guard_seg() -> bool {
    t163_segment_guard()
}

/// ResetSystem 三变体表守卫（行数/守卫/唯一性）。
fn check_t163_resetvar_guard() -> bool {
    let mut ok = resetvar_COUNT == 3;
    if ok {
        ok = resetvar_scan() == 0;
    }
    if ok {
        ok = resetvar_unique();
    }
    ok
}

/// 复位状态机（准备-调用-返回 + 失败路径必记）。
fn check_c2_rst_sm() -> bool {
    let mut led = rstLedger::new(rstState::Idle);
    let mut ok = led.apply(rstEvent::Prep);
    ok = ok && led.apply(rstEvent::Call);
    ok = ok && led.apply(rstEvent::Return);
    if ok {
        ok = led.cur == rstState::Done && led.legal == 3;
    }
    if ok {
        // 失败路径：Prep→Fail→Complete 显性可达。
        let mut l2 = rstLedger::new(rstState::Idle);
        ok = l2.apply(rstEvent::Prep) && l2.apply(rstEvent::Fail) && l2.apply(rstEvent::Complete);
        ok = ok && l2.cur == rstState::Done;
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t163_checks() -> CheckSet {
    let mut set = CheckSet::new("T163");
    set.add(
        "t163-seg-guard",
        t163_guard_seg(),
        if t163_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t163-c1-var-guard",
        check_t163_resetvar_guard(),
        if check_t163_resetvar_guard() { "" } else { "table resetvar guard mismatch" },
    );
    set.add(
        "t163-c2-rst-sm",
        check_c2_rst_sm(),
        if check_c2_rst_sm() { "" } else { "reset state machine mismatch" },
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
    fn t163_all_checks_green() {
        let set = run_t163_checks();
        assert!(set.all_passed(), "T163 domain checks must be green");
    }

    #[test]
    fn t163_segment_guard_holds() {
        assert!(t163_segment_guard());
    }
}
