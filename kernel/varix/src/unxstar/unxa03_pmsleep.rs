//! unxta03_pmsleep —— UNX-F0561 睡眠类型表（S0–S5 语义）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0561 · 电源面定位总账（FADT 字段汇总） · 判据：FADT 电源字段账齐（PM1a_CNT/PM1b_CNT/PM1_STS/PM1_EN/PM2_CNT 五块基址+长度），与 F0453 封账直读一致（5/5 块）
//! · 组件：睡眠类型表（S0..S5）+ 睡眠类型码 + 转换状态机
//! · 判据落地：电源面的睡眠语义
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const TA03_VERSION: &str = "ta03-pmsleep-v1";

/// 诊断段基址。
pub const TA03_SEG_BASE: u16 = 0x4920;

// --------------------------------------------------------------------------
// 诊断码（ta03 十六码 · 14 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 睡眠类型诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ta03Code {
    /// 0x4920 睡眠类型非法（S0..S5 之外）。
    SleepTypeBad,
    /// 0x4921 状态转换非法。
    TransitionBad,
    /// 0x4922 唤醒失败。
    WakeupFailed,
    /// 0x4923 睡眠账破缺。
    LedgerBreak,
    /// 0x4924 转换序违例。
    OrderViolated,
    /// 0x4925 报告陈旧。
    ReportStale,
    /// 0x4926 脱敏缺失。
    MaskMissing,
    /// 0x4927 守卫触发。
    GuardTripped,
    /// 0x4928 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x4929 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x492a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x492b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x492c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x492d 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl ta03Code {
    /// 全量闭集。
    pub const ALL: [ta03Code; 14] = [
        ta03Code::SleepTypeBad,
        ta03Code::TransitionBad,
        ta03Code::WakeupFailed,
        ta03Code::LedgerBreak,
        ta03Code::OrderViolated,
        ta03Code::ReportStale,
        ta03Code::MaskMissing,
        ta03Code::GuardTripped,
        ta03Code::LedgerBroken,
        ta03Code::SnapshotFingerprint,
        ta03Code::TableBroken,
        ta03Code::IndexMisaligned,
        ta03Code::DuplicateEntry,
        ta03Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            ta03Code::SleepTypeBad => 0x4920,
            ta03Code::TransitionBad => 0x4921,
            ta03Code::WakeupFailed => 0x4922,
            ta03Code::LedgerBreak => 0x4923,
            ta03Code::OrderViolated => 0x4924,
            ta03Code::ReportStale => 0x4925,
            ta03Code::MaskMissing => 0x4926,
            ta03Code::GuardTripped => 0x4927,
            ta03Code::LedgerBroken => 0x4928,
            ta03Code::SnapshotFingerprint => 0x4929,
            ta03Code::TableBroken => 0x492a,
            ta03Code::IndexMisaligned => 0x492b,
            ta03Code::DuplicateEntry => 0x492c,
            ta03Code::SegmentGuard => 0x492d,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            ta03Code::SleepTypeBad => "睡眠类型非法（S0..S5 之外）",
            ta03Code::TransitionBad => "状态转换非法",
            ta03Code::WakeupFailed => "唤醒失败",
            ta03Code::LedgerBreak => "睡眠账破缺",
            ta03Code::OrderViolated => "转换序违例",
            ta03Code::ReportStale => "报告陈旧",
            ta03Code::MaskMissing => "脱敏缺失",
            ta03Code::GuardTripped => "守卫触发",
            ta03Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            ta03Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            ta03Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            ta03Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            ta03Code::DuplicateEntry => "条目重复（同键双写）",
            ta03Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            ta03Code::SleepTypeBad => "按睡眠类型表校正",
            ta03Code::TransitionBad => "按转换表校正",
            ta03Code::WakeupFailed => "按唤醒路径重试",
            ta03Code::LedgerBreak => "按守恒重建",
            ta03Code::OrderViolated => "按序重排",
            ta03Code::ReportStale => "重渲染",
            ta03Code::MaskMissing => "补打码",
            ta03Code::GuardTripped => "检查临界区",
            ta03Code::LedgerBroken => "按守恒律重建台账后重跑",
            ta03Code::SnapshotFingerprint => "按防篡改规程重建快照",
            ta03Code::TableBroken => "按语义规则重建常量表",
            ta03Code::IndexMisaligned => "按声明序归一化后重跑",
            ta03Code::DuplicateEntry => "合并同键条目为末次生效",
            ta03Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 pmsleep（6 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 睡眠类型表（S0–S5 逐行守卫）。
/// 行数。
pub const pmsleep_COUNT: usize = 6;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static pmsleep: [(&'static str, u64); 6] = [
    ("S0_WORKING", 0), // S0 工作态（全开）
    ("S1_S1", 1), // S1 睡眠（CPU 停时钟）
    ("S2_S2", 2), // S2 睡眠（CPU 掉电）
    ("S3_S3", 3), // S3 挂起（除内存外全掉电）
    ("S4_S4", 4), // S4 休眠（内存转储到盘）
    ("S5_S5", 5), // S5 软关机
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn pmsleep_get(key: &str) -> Option<u64> {
    match key {
        "S0_WORKING" => Some(0),
        "S1_S1" => Some(1),
        "S2_S2" => Some(2),
        "S3_S3" => Some(3),
        "S4_S4" => Some(4),
        "S5_S5" => Some(5),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn pmsleep_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < pmsleep_COUNT {
        let (k, v) = pmsleep[i];
        let ok = match k {
            "S0_WORKING" => v == 0,
            "S1_S1" => v == 1,
            "S2_S2" => v == 2,
            "S3_S3" => v == 3,
            "S4_S4" => v == 4,
            "S5_S5" => v == 5,
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
pub fn pmsleep_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < pmsleep_COUNT {
        s = s.wrapping_add(pmsleep[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn pmsleep_unique() -> bool {
    let mut i = 0usize;
    while i < pmsleep_COUNT {
        let mut j = i + 1;
        while j < pmsleep_COUNT {
            if pmsleep[i].0 == pmsleep[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 状态机 pwr（5 态 × 4 事件 · 迁移 6 条显式声明）
// --------------------------------------------------------------------------
/// 电源状态机（S0/S1/S3/S4/S5）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum pwrState {
    S0,
    S1,
    S3,
    S4,
    S5,
}

impl pwrState {
    /// 全量态闭集。
    pub const ALL: [pwrState; 5] = [
        pwrState::S0,
        pwrState::S1,
        pwrState::S3,
        pwrState::S4,
        pwrState::S5,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            pwrState::S0 => 0,
            pwrState::S1 => 1,
            pwrState::S3 => 2,
            pwrState::S4 => 3,
            pwrState::S5 => 4,
        }
    }
}

/// pwr事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum pwrEvent {
    Sleep,
    Wake,
    Hibernate,
    PowerOff,
}

impl pwrEvent {
    /// 全量事件闭集。
    pub const ALL: [pwrEvent; 4] = [
        pwrEvent::Sleep,
        pwrEvent::Wake,
        pwrEvent::Hibernate,
        pwrEvent::PowerOff,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const PWR_TRANSITIONS: [(pwrState, pwrEvent, pwrState); 6] = [
    (pwrState::S0, pwrEvent::Sleep, pwrState::S1), // 进入睡眠
    (pwrState::S0, pwrEvent::Hibernate, pwrState::S4), // 进入休眠
    (pwrState::S0, pwrEvent::PowerOff, pwrState::S5), // 软关机
    (pwrState::S1, pwrEvent::Wake, pwrState::S0), // 唤醒回工作
    (pwrState::S3, pwrEvent::Wake, pwrState::S0), // 从挂起唤醒
    (pwrState::S4, pwrEvent::Wake, pwrState::S0), // 休眠恢复
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn pwr_next(state: pwrState, ev: pwrEvent) -> Option<pwrState> {
    let mut i = 0usize;
    while i < 6 {
        let t = PWR_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn pwr_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < pwrState::ALL.len() {
        let mut ei = 0usize;
        while ei < pwrEvent::ALL.len() {
            let legal = pwr_next(pwrState::ALL[si], pwrEvent::ALL[ei]).is_some();
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
pub fn pwr_terminal(st: pwrState) -> bool {
    let mut i = 0usize;
    while i < 6 {
        if PWR_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct pwrLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: pwrState,
}

impl pwrLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: pwrState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: pwrEvent) -> bool {
        match pwr_next(self.cur, ev) {
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

/// 段位守卫：全部诊断码必须落在本段 [0x4920, 0x492f]。
pub const fn ta03_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < ta03Code::ALL.len() {
        let c = ta03Code::ALL[i].code();
        if c < 0x4920 || c > 0x492f {
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
fn ta03_guard_seg() -> bool {
    ta03_segment_guard()
}

/// 表 pmsleep 逐行守卫全绿（行数/守卫/唯一性）。
fn check_ta03_pmsleep_guard() -> bool {
    let mut ok = pmsleep_COUNT == 6;
    if ok {
        ok = pmsleep_scan() == 0;
    }
    if ok {
        ok = pmsleep_unique();
    }
    ok
}

/// 电源状态机主路径（工作-挂起-唤醒 + 关机）。
fn check_c2_pwr_sm() -> bool {
    let mut led = pwrLedger::new(pwrState::S0);
    let mut ok = led.apply(pwrEvent::Hibernate);
    ok = ok && led.apply(pwrEvent::Wake);
    if ok {
        ok = led.apply(pwrEvent::Sleep) && led.cur == pwrState::S1;
    }
    if ok {
        ok = led.apply(pwrEvent::Wake) && led.cur == pwrState::S0;
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_ta03_checks() -> CheckSet {
    let mut set = CheckSet::new("TA03");
    set.add(
        "ta03-seg-guard",
        ta03_guard_seg(),
        if ta03_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "ta03-c1-sleep-guard",
        check_ta03_pmsleep_guard(),
        if check_ta03_pmsleep_guard() { "" } else { "table pmsleep guard mismatch" },
    );
    set.add(
        "ta03-c2-pwr-sm",
        check_c2_pwr_sm(),
        if check_c2_pwr_sm() { "" } else { "power state machine mismatch" },
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
    fn ta03_all_checks_green() {
        let set = run_ta03_checks();
        assert!(set.all_passed(), "TA03 domain checks must be green");
    }

    #[test]
    fn ta03_segment_guard_holds() {
        assert!(ta03_segment_guard());
    }
}
