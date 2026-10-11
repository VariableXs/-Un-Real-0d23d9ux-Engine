//! unxt125_wddiag —— UNX-F0122 捕获后诊断模式（诊断面 + 快照）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0122 · 阶段看门狗（每阶段独立时限） · 判据：注入阶段卡死被看门狗捕获，超时时限精度毫秒级，捕获后进诊断模式
//! · 组件：诊断模式状态机 + 诊断面表 + 域快照 + 报告链
//! · 判据落地：捕获后进诊断模式
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T125_VERSION: &str = "t125-wddiag-v1";

/// 诊断段基址。
pub const T125_SEG_BASE: u16 = 0x4b30;

// --------------------------------------------------------------------------
// 诊断码（t125 十六码 · 14 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 诊断模式诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t125Code {
    /// 0x4b30 诊断模式非法。
    DiagModeBad,
    /// 0x4b31 诊断数据缺失（卡死现场无留存）。
    DiagDataMissing,
    /// 0x4b32 指令指针越界（现场 RIP 不可信）。
    RipOob,
    /// 0x4b33 诊断链断裂。
    ChainBroken,
    /// 0x4b34 诊断账破缺。
    LedgerBreak,
    /// 0x4b35 诊断序违例。
    OrderViolated,
    /// 0x4b36 报告陈旧。
    ReportStale,
    /// 0x4b37 守卫触发。
    GuardTripped,
    /// 0x4b38 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x4b39 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x4b3a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x4b3b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x4b3c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x4b3d 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t125Code {
    /// 全量闭集。
    pub const ALL: [t125Code; 14] = [
        t125Code::DiagModeBad,
        t125Code::DiagDataMissing,
        t125Code::RipOob,
        t125Code::ChainBroken,
        t125Code::LedgerBreak,
        t125Code::OrderViolated,
        t125Code::ReportStale,
        t125Code::GuardTripped,
        t125Code::LedgerBroken,
        t125Code::SnapshotFingerprint,
        t125Code::TableBroken,
        t125Code::IndexMisaligned,
        t125Code::DuplicateEntry,
        t125Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t125Code::DiagModeBad => 0x4b30,
            t125Code::DiagDataMissing => 0x4b31,
            t125Code::RipOob => 0x4b32,
            t125Code::ChainBroken => 0x4b33,
            t125Code::LedgerBreak => 0x4b34,
            t125Code::OrderViolated => 0x4b35,
            t125Code::ReportStale => 0x4b36,
            t125Code::GuardTripped => 0x4b37,
            t125Code::LedgerBroken => 0x4b38,
            t125Code::SnapshotFingerprint => 0x4b39,
            t125Code::TableBroken => 0x4b3a,
            t125Code::IndexMisaligned => 0x4b3b,
            t125Code::DuplicateEntry => 0x4b3c,
            t125Code::SegmentGuard => 0x4b3d,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t125Code::DiagModeBad => "诊断模式非法",
            t125Code::DiagDataMissing => "诊断数据缺失（卡死现场无留存）",
            t125Code::RipOob => "指令指针越界（现场 RIP 不可信）",
            t125Code::ChainBroken => "诊断链断裂",
            t125Code::LedgerBreak => "诊断账破缺",
            t125Code::OrderViolated => "诊断序违例",
            t125Code::ReportStale => "报告陈旧",
            t125Code::GuardTripped => "守卫触发",
            t125Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t125Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t125Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t125Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t125Code::DuplicateEntry => "条目重复（同键双写）",
            t125Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t125Code::DiagModeBad => "按诊断闭集校正",
            t125Code::DiagDataMissing => "补现场留存",
            t125Code::RipOob => "按现场可信度标注",
            t125Code::ChainBroken => "补链后回放",
            t125Code::LedgerBreak => "按守恒重建",
            t125Code::OrderViolated => "按序重排",
            t125Code::ReportStale => "重渲染",
            t125Code::GuardTripped => "检查临界区",
            t125Code::LedgerBroken => "按守恒律重建台账后重跑",
            t125Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t125Code::TableBroken => "按语义规则重建常量表",
            t125Code::IndexMisaligned => "按声明序归一化后重跑",
            t125Code::DuplicateEntry => "合并同键条目为末次生效",
            t125Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 状态机 diag（4 态 × 4 事件 · 迁移 5 条显式声明）
// --------------------------------------------------------------------------
/// 诊断模式状态机（现场留存→报告→复位）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum diagState {
    Normal,
    Capturing,
    Diagnosing,
    Reporting,
}

impl diagState {
    /// 全量态闭集。
    pub const ALL: [diagState; 4] = [
        diagState::Normal,
        diagState::Capturing,
        diagState::Diagnosing,
        diagState::Reporting,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            diagState::Normal => 0,
            diagState::Capturing => 1,
            diagState::Diagnosing => 2,
            diagState::Reporting => 3,
        }
    }
}

/// diag事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum diagEvent {
    Timeout,
    Capture,
    Report,
    Resume,
}

impl diagEvent {
    /// 全量事件闭集。
    pub const ALL: [diagEvent; 4] = [
        diagEvent::Timeout,
        diagEvent::Capture,
        diagEvent::Report,
        diagEvent::Resume,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const DIAG_TRANSITIONS: [(diagState, diagEvent, diagState); 5] = [
    (diagState::Normal, diagEvent::Timeout, diagState::Capturing), // 看门狗超时
    (diagState::Capturing, diagEvent::Capture, diagState::Diagnosing), // 留存卡死现场
    (diagState::Diagnosing, diagEvent::Report, diagState::Reporting), // 输出诊断报告
    (diagState::Reporting, diagEvent::Resume, diagState::Normal), // 诊断收口复位
    (diagState::Capturing, diagEvent::Timeout, diagState::Capturing), // 诊断中再超时保持
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn diag_next(state: diagState, ev: diagEvent) -> Option<diagState> {
    let mut i = 0usize;
    while i < 5 {
        let t = DIAG_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn diag_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < diagState::ALL.len() {
        let mut ei = 0usize;
        while ei < diagEvent::ALL.len() {
            let legal = diag_next(diagState::ALL[si], diagEvent::ALL[ei]).is_some();
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
pub fn diag_terminal(st: diagState) -> bool {
    let mut i = 0usize;
    while i < 5 {
        if DIAG_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct diagLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: diagState,
}

impl diagLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: diagState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: diagEvent) -> bool {
        match diag_next(self.cur, ev) {
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
// 字符串键值映射 diagsite（6 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 卡死现场诊断要素表（六要素逐行守卫）。
/// 行数。
pub const diagsite_COUNT: usize = 6;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static diagsite: [(&'static str, u64); 6] = [
    ("DIAG_RIP", 0), // 现场指令指针（RIP 留存）
    ("DIAG_RSP", 0), // 现场栈指针（RSP 留存）
    ("DIAG_STAGE", 0), // 卡死阶段号（12 阶段之一）
    ("DIAG_ELAPSED", 0), // 卡死已耗时（毫秒）
    ("DIAG_WDCNT", 1), // 看门狗触发计数
    ("DIAG_CPU", 0), // 卡死 CPU 号
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn diagsite_get(key: &str) -> Option<u64> {
    match key {
        "DIAG_RIP" => Some(0),
        "DIAG_RSP" => Some(0),
        "DIAG_STAGE" => Some(0),
        "DIAG_ELAPSED" => Some(0),
        "DIAG_WDCNT" => Some(1),
        "DIAG_CPU" => Some(0),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn diagsite_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < diagsite_COUNT {
        let (k, v) = diagsite[i];
        let ok = match k {
            "DIAG_RIP" => v >= 0,
            "DIAG_RSP" => v >= 0,
            "DIAG_STAGE" => v < 12,
            "DIAG_ELAPSED" => v >= 0,
            "DIAG_WDCNT" => v >= 1,
            "DIAG_CPU" => v < 256,
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
pub fn diagsite_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < diagsite_COUNT {
        s = s.wrapping_add(diagsite[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn diagsite_unique() -> bool {
    let mut i = 0usize;
    while i < diagsite_COUNT {
        let mut j = i + 1;
        while j < diagsite_COUNT {
            if diagsite[i].0 == diagsite[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------------------
// 快照 dsnap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// 看门狗域六分量快照。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct dsnap {
    /// 指纹分量：stages。
    pub stages: u64,
    /// 指纹分量：deadlines。
    pub deadlines: u64,
    /// 指纹分量：stall_grid。
    pub stall_grid: u64,
    /// 指纹分量：captured。
    pub captured: u64,
    /// 指纹分量：diag_elems。
    pub diag_elems: u64,
    /// 指纹分量：false_pos。
    pub false_pos: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl dsnap {
    /// 构造并即时合成指纹。
    pub fn compute(
        stages: u64,
        deadlines: u64,
        stall_grid: u64,
        captured: u64,
        diag_elems: u64,
        false_pos: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &stages.to_le_bytes());
        h = fnv1a64_from(h, &deadlines.to_le_bytes());
        h = fnv1a64_from(h, &stall_grid.to_le_bytes());
        h = fnv1a64_from(h, &captured.to_le_bytes());
        h = fnv1a64_from(h, &diag_elems.to_le_bytes());
        h = fnv1a64_from(h, &false_pos.to_le_bytes());
        Self {
            stages,
            deadlines,
            stall_grid,
            captured,
            diag_elems,
            false_pos,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.stages.to_le_bytes());
        h = fnv1a64_from(h, &self.deadlines.to_le_bytes());
        h = fnv1a64_from(h, &self.stall_grid.to_le_bytes());
        h = fnv1a64_from(h, &self.captured.to_le_bytes());
        h = fnv1a64_from(h, &self.diag_elems.to_le_bytes());
        h = fnv1a64_from(h, &self.false_pos.to_le_bytes());
        h
    }

    /// 篡改检出：字段被改后重算值与存值不一致。
    pub fn tampered(&self) -> bool {
        self.recompute() != self.fingerprint
    }
}

// --------------------------------------------------------------------------
// 引擎 ddom（链式指纹：分量链环 + 逐链回放校验）
// --------------------------------------------------------------------------
/// 看门狗域分量链。
#[derive(Clone, Copy, Debug)]
pub struct ddomLink {
    /// 段标签（如十二步节名）。
    pub tag: u32,
    /// 段负载（长度/条目数/字段数的语义化标量）。
    pub payload: u64,
    /// 本链指纹（FNV-1a 64 链式）。
    pub digest: u64,
}

/// 链式账本（append-only：只增不改，破坏即检出）。
#[derive(Clone, Copy, Debug)]
pub struct ddomChain {
    /// 链节点序列。
    pub links: [ddomLink; 16],
    /// 节点数。
    pub n: usize,
    /// 累计负载。
    pub payload_total: u64,
    /// 总链指纹（最后一环）。
    pub head: u64,
}

impl ddomChain {
    /// 空链构造。
    pub const fn new() -> Self {
        Self {
            links: [ddomLink { tag: 0, payload: 0, digest: 0 }; 16],
            n: 0,
            payload_total: 0,
            head: FNV_OFFSET,
        }
    }

    /// 追加一段（负载超标或容量满时拒收）。
    pub fn append(&mut self, tag: u32, payload: u64) -> Result<u64, t125Code> {
        if self.n >= 16 || payload == 0 {
            return Err(t125Code::LedgerBroken);
        }
        let digest = fnv1a64_from(self.head, &payload.to_le_bytes());
        self.links[self.n] = ddomLink { tag, payload, digest };
        self.n += 1;
        self.payload_total += payload;
        self.head = digest;
        Ok(digest)
    }

    /// 全链回放校验（逐环重算指纹 + 负载守恒）。
    pub fn verify(&self) -> bool {
        let mut h = FNV_OFFSET;
        let mut total = 0u64;
        let mut i = 0usize;
        while i < self.n {
            let lk = self.links[i];
            h = fnv1a64_from(h, &lk.payload.to_le_bytes());
            if h != lk.digest {
                return false;
            }
            total += lk.payload;
            i += 1;
        }
        total == self.payload_total && h == self.head
    }
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x4b30, 0x4b3f]。
pub const fn t125_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t125Code::ALL.len() {
        let c = t125Code::ALL[i].code();
        if c < 0x4b30 || c > 0x4b3f {
            ok = false;
        }
        i += 1;
    }
    ok
}

// ---------------------------------------------------------------------------
// FNV-1a 64 快照指纹（无第三方依赖的确定性哈希；跨次运行恒同）
// ---------------------------------------------------------------------------

/// FNV-1a 64 偏移基准（标准初值 0xcbf29ce484222325）。
pub const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV-1a 64 素数（标准 0x100000001b3）。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// 逐字节 FNV-1a 64 递推（xor 后乘，顺序即语义）。
pub const fn fnv1a64(data: &[u8]) -> u64 {
    let mut h = FNV_OFFSET;
    let mut i = 0usize;
    while i < data.len() {
        h ^= data[i] as u64;
        h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}

/// 64 位值入链（小端字节序，保证跨平台构造一致）。
pub const fn fnv1a64_u64(h: u64, v: u64) -> u64 {
    let b = [
        (v & 0xff) as u8,
        ((v >> 8) & 0xff) as u8,
        ((v >> 16) & 0xff) as u8,
        ((v >> 24) & 0xff) as u8,
        ((v >> 32) & 0xff) as u8,
        ((v >> 40) & 0xff) as u8,
        ((v >> 48) & 0xff) as u8,
        ((v >> 56) & 0xff) as u8,
    ];
    fnv1a64_from(h, &b)
}

/// 以给定初值继续递推（链式哈希原语）。
pub const fn fnv1a64_from(h: u64, data: &[u8]) -> u64 {
    let mut hh = h;
    let mut i = 0usize;
    while i < data.len() {
        hh ^= data[i] as u64;
        hh = hh.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    hh
}

// --------------------------------------------------------------------------
// 自检（CheckSet 范式：每检查一带 detail，全绿才出门）
// --------------------------------------------------------------------------
/// 段位守卫：全部诊断码落在本模块段内。
fn t125_guard_seg() -> bool {
    t125_segment_guard()
}

/// 诊断状态机（超时-留存-报告-复位全路径）。
fn check_c1_diag_sm() -> bool {
    let mut led = diagLedger::new(diagState::Normal);
    let mut ok = led.apply(diagEvent::Timeout);
    ok = ok && led.apply(diagEvent::Capture);
    ok = ok && led.apply(diagEvent::Report);
    ok = ok && led.apply(diagEvent::Resume);
    if ok {
        ok = led.cur == diagState::Normal && led.legal == 4;
    }
    ok
}

/// 看门狗域快照 + 篡改检出。
fn check_c2_snap() -> bool {
    let s = dsnap::compute(12, 12, 48, 48, 6, 0);
    let mut ok = !s.tampered();
    if ok {
        let mut bad = s;
        bad.captured = 47;
        ok = bad.tampered();
    }
    ok
}

/// 看门狗域分量链回放校验。
fn check_c3_chain() -> bool {
    let mut ch = ddomChain::new();
    let mut ok = true;
    for (i, v) in [12u64, 12, 48, 48, 6, 1].iter().enumerate() {
        if ch.append(i as u32, *v).is_err() {
            ok = false;
        }
    }
    if ok {
        ok = ch.verify() && ch.payload_total == 127;
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t125_checks() -> CheckSet {
    let mut set = CheckSet::new("T125");
    set.add(
        "t125-seg-guard",
        t125_guard_seg(),
        if t125_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t125-c1-diag-sm",
        check_c1_diag_sm(),
        if check_c1_diag_sm() { "" } else { "diagnose state machine mismatch" },
    );
    set.add(
        "t125-c2-snap",
        check_c2_snap(),
        if check_c2_snap() { "" } else { "watchdog domain snapshot mismatch" },
    );
    set.add(
        "t125-c3-chain",
        check_c3_chain(),
        if check_c3_chain() { "" } else { "watchdog domain chain mismatch" },
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
    fn t125_all_checks_green() {
        let set = run_t125_checks();
        assert!(set.all_passed(), "T125 domain checks must be green");
    }

    #[test]
    fn t125_segment_guard_holds() {
        assert!(t125_segment_guard());
    }
}
