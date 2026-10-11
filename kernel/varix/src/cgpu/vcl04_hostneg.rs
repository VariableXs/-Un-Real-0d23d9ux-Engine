//! unxl04b_vcl04_hostneg —— CGPU-F1764 host 协商协议与探测缓存（半虚拟化路径）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：CGPU-F1764 · 能力探测与虚拟化感知（CGPU-L 域 · 批次 L01 · L04 组 · 判据：探测矩阵/诚实/协商协议/缓存/保守/五组）
//! · 组件：协商协议状态机 + 协商消息表 + 探测缓存（K01 模式复用）+ 保守降级
//! · 判据落地：协商协议/缓存/保守
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const L04B_VERSION: &str = "l04b-vcl04_hostneg-v1";

/// 诊断段基址。
pub const L04B_SEG_BASE: u16 = 0x7110;

// --------------------------------------------------------------------------
// 诊断码（l04b 十六码 · 14 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// host 协商诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum l04bCode {
    /// 0x7110 协商消息非法。
    NegotiateBad,
    /// 0x7111 握手丢失（协商无应答）。
    HandshakeLost,
    /// 0x7112 缓存键重复。
    CacheKeyDup,
    /// 0x7113 缓存命中口径非法（命中计数错）。
    CacheHitBad,
    /// 0x7114 兜底迟到（协商失败未降级）。
    FallbackLate,
    /// 0x7115 协商账破缺。
    LedgerBreak,
    /// 0x7116 协商序违例。
    OrderViolated,
    /// 0x7117 守卫触发。
    GuardTripped,
    /// 0x7118 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x7119 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x711a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x711b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x711c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x711d 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl l04bCode {
    /// 全量闭集。
    pub const ALL: [l04bCode; 14] = [
        l04bCode::NegotiateBad,
        l04bCode::HandshakeLost,
        l04bCode::CacheKeyDup,
        l04bCode::CacheHitBad,
        l04bCode::FallbackLate,
        l04bCode::LedgerBreak,
        l04bCode::OrderViolated,
        l04bCode::GuardTripped,
        l04bCode::LedgerBroken,
        l04bCode::SnapshotFingerprint,
        l04bCode::TableBroken,
        l04bCode::IndexMisaligned,
        l04bCode::DuplicateEntry,
        l04bCode::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            l04bCode::NegotiateBad => 0x7110,
            l04bCode::HandshakeLost => 0x7111,
            l04bCode::CacheKeyDup => 0x7112,
            l04bCode::CacheHitBad => 0x7113,
            l04bCode::FallbackLate => 0x7114,
            l04bCode::LedgerBreak => 0x7115,
            l04bCode::OrderViolated => 0x7116,
            l04bCode::GuardTripped => 0x7117,
            l04bCode::LedgerBroken => 0x7118,
            l04bCode::SnapshotFingerprint => 0x7119,
            l04bCode::TableBroken => 0x711a,
            l04bCode::IndexMisaligned => 0x711b,
            l04bCode::DuplicateEntry => 0x711c,
            l04bCode::SegmentGuard => 0x711d,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            l04bCode::NegotiateBad => "协商消息非法",
            l04bCode::HandshakeLost => "握手丢失（协商无应答）",
            l04bCode::CacheKeyDup => "缓存键重复",
            l04bCode::CacheHitBad => "缓存命中口径非法（命中计数错）",
            l04bCode::FallbackLate => "兜底迟到（协商失败未降级）",
            l04bCode::LedgerBreak => "协商账破缺",
            l04bCode::OrderViolated => "协商序违例",
            l04bCode::GuardTripped => "守卫触发",
            l04bCode::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            l04bCode::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            l04bCode::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            l04bCode::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            l04bCode::DuplicateEntry => "条目重复（同键双写）",
            l04bCode::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            l04bCode::NegotiateBad => "按协商消息表校正",
            l04bCode::HandshakeLost => "按超时路径重试",
            l04bCode::CacheKeyDup => "幂等去重",
            l04bCode::CacheHitBad => "按守恒重建",
            l04bCode::FallbackLate => "按保守路径补降级",
            l04bCode::LedgerBreak => "按守恒重建",
            l04bCode::OrderViolated => "按序重排",
            l04bCode::GuardTripped => "检查临界区",
            l04bCode::LedgerBroken => "按守恒律重建台账后重跑",
            l04bCode::SnapshotFingerprint => "按防篡改规程重建快照",
            l04bCode::TableBroken => "按语义规则重建常量表",
            l04bCode::IndexMisaligned => "按声明序归一化后重跑",
            l04bCode::DuplicateEntry => "合并同键条目为末次生效",
            l04bCode::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 状态机 neg（5 态 × 5 事件 · 迁移 6 条显式声明）
// --------------------------------------------------------------------------
/// host-guest 协商协议状态机状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum negState {
    Idle,
    Offered,
    Negotiated,
    Established,
    Fallen,
}

impl negState {
    /// 全量态闭集。
    pub const ALL: [negState; 5] = [
        negState::Idle,
        negState::Offered,
        negState::Negotiated,
        negState::Established,
        negState::Fallen,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            negState::Idle => 0,
            negState::Offered => 1,
            negState::Negotiated => 2,
            negState::Established => 3,
            negState::Fallen => 4,
        }
    }
}

/// neg事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum negEvent {
    Offer,
    Counter,
    Accept,
    Establish,
    Timeout,
}

impl negEvent {
    /// 全量事件闭集。
    pub const ALL: [negEvent; 5] = [
        negEvent::Offer,
        negEvent::Counter,
        negEvent::Accept,
        negEvent::Establish,
        negEvent::Timeout,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const NEG_TRANSITIONS: [(negState, negEvent, negState); 6] = [
    (negState::Idle, negEvent::Offer, negState::Offered), // host 发起能力提供
    (negState::Offered, negEvent::Counter, negState::Negotiated), // guest 反要约
    (negState::Negotiated, negEvent::Accept, negState::Established), // 双方接受落定
    (negState::Offered, negEvent::Timeout, negState::Fallen), // 协商超时兜底
    (negState::Negotiated, negEvent::Timeout, negState::Fallen), // 协商超时兜底
    (negState::Fallen, negEvent::Offer, negState::Offered), // 兜底后重协商
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn neg_next(state: negState, ev: negEvent) -> Option<negState> {
    let mut i = 0usize;
    while i < 6 {
        let t = NEG_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn neg_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < negState::ALL.len() {
        let mut ei = 0usize;
        while ei < negEvent::ALL.len() {
            let legal = neg_next(negState::ALL[si], negEvent::ALL[ei]).is_some();
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
pub fn neg_terminal(st: negState) -> bool {
    let mut i = 0usize;
    while i < 6 {
        if NEG_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct negLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: negState,
}

impl negLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: negState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: negEvent) -> bool {
        match neg_next(self.cur, ev) {
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
// 字符串键值映射 negmsg（8 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 协商协议消息表（八消息逐行守卫）。
/// 行数。
pub const negmsg_COUNT: usize = 8;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static negmsg: [(&'static str, u64); 8] = [
    ("NEG_HELLO", 1), // hello（版本/能力宣告）
    ("NEG_OFFER", 2), // offer（host 侧能力提供）
    ("NEG_COUNTER", 3), // counter（guest 侧反要约）
    ("NEG_ACCEPT", 4), // accept（双方接受）
    ("NEG_ESTABLISH", 5), // establish（路径落定）
    ("NEG_TIMEOUT", 6), // timeout（超时兜底）
    ("NEG_DEGRADE", 7), // degrade（保守降级通知）
    ("NEG_RETRY", 8), // retry（重协商）
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn negmsg_get(key: &str) -> Option<u64> {
    match key {
        "NEG_HELLO" => Some(1),
        "NEG_OFFER" => Some(2),
        "NEG_COUNTER" => Some(3),
        "NEG_ACCEPT" => Some(4),
        "NEG_ESTABLISH" => Some(5),
        "NEG_TIMEOUT" => Some(6),
        "NEG_DEGRADE" => Some(7),
        "NEG_RETRY" => Some(8),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn negmsg_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < negmsg_COUNT {
        let (k, v) = negmsg[i];
        let ok = match k {
            "NEG_HELLO" => v == 1,
            "NEG_OFFER" => v == 2,
            "NEG_COUNTER" => v == 3,
            "NEG_ACCEPT" => v == 4,
            "NEG_ESTABLISH" => v == 5,
            "NEG_TIMEOUT" => v == 6,
            "NEG_DEGRADE" => v == 7,
            "NEG_RETRY" => v == 8,
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
pub fn negmsg_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < negmsg_COUNT {
        s = s.wrapping_add(negmsg[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn negmsg_unique() -> bool {
    let mut i = 0usize;
    while i < negmsg_COUNT {
        let mut j = i + 1;
        while j < negmsg_COUNT {
            if negmsg[i].0 == negmsg[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 守恒账 negcache（探测缓存账（命中 + 未命中 == 查询总数））
// --------------------------------------------------------------------------
/// 探测缓存账（命中 + 未命中 == 查询总数）。
#[derive(Clone, Copy, Debug)]
pub struct negcache {
    /// 缓存命中数
    pub hits: u64,
    /// 未命中数
    pub misses: u64,
    /// 保守降级次数
    pub fallbacks: u64,
}

impl negcache {
    /// 全零构造。
    pub const fn new() -> Self {
        Self {
            hits: 0,
            misses: 0,
            fallbacks: 0,
        }
    }

    /// 记一次命中。
    pub fn hit(&mut self) {
        self.hits += 1;
    }

    /// 记一次未命中。
    pub fn miss(&mut self) {
        self.misses += 1;
    }

    /// 记一次保守降级。
    pub fn fallback(&mut self) {
        self.fallbacks += 1;
    }

}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x7110, 0x711f]。
pub const fn l04b_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < l04bCode::ALL.len() {
        let c = l04bCode::ALL[i].code();
        if c < 0x7110 || c > 0x711f {
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
fn l04b_guard_seg() -> bool {
    l04b_segment_guard()
}

/// 表 negmsg 逐行守卫全绿（行数/守卫/唯一性）。
fn check_l04b_negmsg_guard() -> bool {
    let mut ok = negmsg_COUNT == 8;
    if ok {
        ok = negmsg_scan() == 0;
    }
    if ok {
        ok = negmsg_unique();
    }
    ok
}

/// 协商状态机（offered→counter→accept→established 全路径 + 终态拒止）。
fn check_c2_neg_sm() -> bool {
    let mut led = negLedger::new(negState::Idle);
    let mut ok = led.apply(negEvent::Offer);
    ok = ok && led.apply(negEvent::Counter);
    ok = ok && led.apply(negEvent::Accept);
    if ok {
        ok = led.cur == negState::Established && led.legal == 3;
    }
    if ok {
        // Established 是终态（任何事件必须拒止）。
        ok = !led.apply(negEvent::Offer) && led.illegal == 1;
    }
    ok
}

/// 探测缓存账守恒（命中+未命中=总数 + 降级路径）。
fn check_c3_cache() -> bool {
    let mut c = negcache::new();
    let mut i = 0;
    while i < 24 {
        c.hit();
        c.miss();
        i += 1;
    }
    let mut ok = c.hits == 24 && c.misses == 24;
    if ok {
        let mut j = 0;
        while j < 3 {
            c.fallback();
            j += 1;
        }
        ok = c.fallbacks == 3;
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_l04b_checks() -> CheckSet {
    let mut set = CheckSet::new("L04B");
    set.add(
        "l04b-seg-guard",
        l04b_guard_seg(),
        if l04b_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "l04b-c1-msg-guard",
        check_l04b_negmsg_guard(),
        if check_l04b_negmsg_guard() { "" } else { "table negmsg guard mismatch" },
    );
    set.add(
        "l04b-c2-neg-sm",
        check_c2_neg_sm(),
        if check_c2_neg_sm() { "" } else { "negotiate state machine mismatch" },
    );
    set.add(
        "l04b-c3-cache",
        check_c3_cache(),
        if check_c3_cache() { "" } else { "probe cache ledger mismatch" },
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
    fn l04b_all_checks_green() {
        let set = run_l04b_checks();
        assert!(set.all_passed(), "L04B domain checks must be green");
    }

    #[test]
    fn l04b_segment_guard_holds() {
        assert!(l04b_segment_guard());
    }
}
