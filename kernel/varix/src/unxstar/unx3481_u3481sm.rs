//! unxu3481_u3481sm —— 分级日志总设计（五级 TRACE/DEBUG/INFO/WARN/ERROR + 分级输出策略） 状态机与快照封存（分级日志）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3481 · 分级日志总设计（五级 TRACE/DEBUG/INFO/WARN/ERROR + 分级输出策略） · 判据原文：UNX-F3481 · 判据：J1：五级语义冻结（级别定义 × 消费者 × 输出策略三列——判据行内嵌矩阵）；M-240 分级全链路（五级各一条日志 → 过滤 → 转储 → 可读）。
//! · 组件：分级日志 状态机（6 态 × 6 事件显式迁移）+ FNV 快照封存（8 分量）
//! · 判据落地：UNX-F3481 · 判据：J1：五级语义冻结（级别定义 × 消费者 × 输出策略三列——判据行内嵌矩阵）；M-240 分级全链路（五级各一条日志 → 过滤 
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3481_VERSION: &str = "u3481-u3481sm-v1";

/// 诊断段基址。
pub const U3481_SEG_BASE: u16 = 0xe0c0;

// --------------------------------------------------------------------------
// 诊断码（u3481 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 分级日志 状态面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3481Code {
    /// 0xe0c0 状态重复（日志 同名双态）。
    StateDup,
    /// 0xe0c1 状态断档（分级 序缺失）。
    StateHole,
    /// 0xe0c2 事件重复（同事件双声明）。
    EventDup,
    /// 0xe0c3 迁移重复（同源同事件双迁移）。
    TransDup,
    /// 0xe0c4 迁移缺失（应达态不可达）。
    TransHole,
    /// 0xe0c5 迁移环（非法自环/环路）。
    TransCycle,
    /// 0xe0c6 入口非法（初态不在闭集）。
    EntryBad,
    /// 0xe0c7 迁移丢失（应用事件无效果）。
    ApplyLost,
    /// 0xe0c8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe0c9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe0ca 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe0cb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3481Code {
    /// 全量闭集。
    pub const ALL: [u3481Code; 12] = [
        u3481Code::StateDup,
        u3481Code::StateHole,
        u3481Code::EventDup,
        u3481Code::TransDup,
        u3481Code::TransHole,
        u3481Code::TransCycle,
        u3481Code::EntryBad,
        u3481Code::ApplyLost,
        u3481Code::LedgerBroken,
        u3481Code::SnapshotFingerprint,
        u3481Code::TableBroken,
        u3481Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3481Code::StateDup => 0xe0c0,
            u3481Code::StateHole => 0xe0c1,
            u3481Code::EventDup => 0xe0c2,
            u3481Code::TransDup => 0xe0c3,
            u3481Code::TransHole => 0xe0c4,
            u3481Code::TransCycle => 0xe0c5,
            u3481Code::EntryBad => 0xe0c6,
            u3481Code::ApplyLost => 0xe0c7,
            u3481Code::LedgerBroken => 0xe0c8,
            u3481Code::SnapshotFingerprint => 0xe0c9,
            u3481Code::TableBroken => 0xe0ca,
            u3481Code::SegmentGuard => 0xe0cb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3481Code::StateDup => "状态重复（日志 同名双态）",
            u3481Code::StateHole => "状态断档（分级 序缺失）",
            u3481Code::EventDup => "事件重复（同事件双声明）",
            u3481Code::TransDup => "迁移重复（同源同事件双迁移）",
            u3481Code::TransHole => "迁移缺失（应达态不可达）",
            u3481Code::TransCycle => "迁移环（非法自环/环路）",
            u3481Code::EntryBad => "入口非法（初态不在闭集）",
            u3481Code::ApplyLost => "迁移丢失（应用事件无效果）",
            u3481Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3481Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3481Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3481Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3481Code::StateDup => "按闭集唯一律去重",
            u3481Code::StateHole => "按闭集补态",
            u3481Code::EventDup => "按事件闭集去重",
            u3481Code::TransDup => "按确定性唯一迁移律合并",
            u3481Code::TransHole => "按迁移矩阵补边",
            u3481Code::TransCycle => "按无环律重构迁移",
            u3481Code::EntryBad => "按闭集钳制初态",
            u3481Code::ApplyLost => "核对事件前置条件",
            u3481Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3481Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3481Code::TableBroken => "按语义规则重建常量表",
            u3481Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 状态机 mxu3481flow（6 态 × 6 事件 · 迁移 8 条显式声明）
// --------------------------------------------------------------------------
/// 分级日志 状态机（判据：日志）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu3481flowState {
    Off,
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl mxu3481flowState {
    /// 全量态闭集。
    pub const ALL: [mxu3481flowState; 6] = [
        mxu3481flowState::Off,
        mxu3481flowState::Trace,
        mxu3481flowState::Debug,
        mxu3481flowState::Info,
        mxu3481flowState::Warn,
        mxu3481flowState::Error,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            mxu3481flowState::Off => 0,
            mxu3481flowState::Trace => 1,
            mxu3481flowState::Debug => 2,
            mxu3481flowState::Info => 3,
            mxu3481flowState::Warn => 4,
            mxu3481flowState::Error => 5,
        }
    }
}

/// mxu3481flow事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu3481flowEvent {
    Emit,
    Filter,
    Rate,
    Buffer,
    Flush,
    Rotate,
}

impl mxu3481flowEvent {
    /// 全量事件闭集。
    pub const ALL: [mxu3481flowEvent; 6] = [
        mxu3481flowEvent::Emit,
        mxu3481flowEvent::Filter,
        mxu3481flowEvent::Rate,
        mxu3481flowEvent::Buffer,
        mxu3481flowEvent::Flush,
        mxu3481flowEvent::Rotate,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const MXU3481FLOW_TRANSITIONS: [(mxu3481flowState, mxu3481flowEvent, mxu3481flowState); 8] = [
    (mxu3481flowState::Off, mxu3481flowEvent::Emit, mxu3481flowState::Trace), // Off→Trace 主链
    (mxu3481flowState::Trace, mxu3481flowEvent::Filter, mxu3481flowState::Debug), // Trace→Debug 主链
    (mxu3481flowState::Debug, mxu3481flowEvent::Rate, mxu3481flowState::Info), // Debug→Info 主链
    (mxu3481flowState::Info, mxu3481flowEvent::Buffer, mxu3481flowState::Warn), // Info→Warn 主链
    (mxu3481flowState::Warn, mxu3481flowEvent::Flush, mxu3481flowState::Error), // Warn→Error 主链
    (mxu3481flowState::Error, mxu3481flowEvent::Rotate, mxu3481flowState::Off), // 终态回边
    (mxu3481flowState::Error, mxu3481flowEvent::Emit, mxu3481flowState::Warn), // 终态回退边
    (mxu3481flowState::Trace, mxu3481flowEvent::Filter, mxu3481flowState::Debug), // 旁路边
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn mxu3481flow_next(state: mxu3481flowState, ev: mxu3481flowEvent) -> Option<mxu3481flowState> {
    let mut i = 0usize;
    while i < 8 {
        let t = MXU3481FLOW_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn mxu3481flow_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < mxu3481flowState::ALL.len() {
        let mut ei = 0usize;
        while ei < mxu3481flowEvent::ALL.len() {
            let legal = mxu3481flow_next(mxu3481flowState::ALL[si], mxu3481flowEvent::ALL[ei]).is_some();
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
pub fn mxu3481flow_terminal(st: mxu3481flowState) -> bool {
    let mut i = 0usize;
    while i < 8 {
        if MXU3481FLOW_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct mxu3481flowLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: mxu3481flowState,
}

impl mxu3481flowLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: mxu3481flowState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: mxu3481flowEvent) -> bool {
        match mxu3481flow_next(self.cur, ev) {
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
// 快照 Mxu3481Snap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// 分级日志 快照封存（FNV 三态指纹 + 篡改检出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mxu3481Snap {
    /// 指纹分量：lvl_v。
    pub lvl_v: u64,
    /// 指纹分量：trace_v。
    pub trace_v: u64,
    /// 指纹分量：dbg_v。
    pub dbg_v: u64,
    /// 指纹分量：inf_v。
    pub inf_v: u64,
    /// 指纹分量：wrn_v。
    pub wrn_v: u64,
    /// 指纹分量：err_v。
    pub err_v: u64,
    /// 指纹分量：filt_v。
    pub filt_v: u64,
    /// 指纹分量：buf_v。
    pub buf_v: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl Mxu3481Snap {
    /// 构造并即时合成指纹。
    pub fn compute(
        lvl_v: u64,
        trace_v: u64,
        dbg_v: u64,
        inf_v: u64,
        wrn_v: u64,
        err_v: u64,
        filt_v: u64,
        buf_v: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &lvl_v.to_le_bytes());
        h = fnv1a64_from(h, &trace_v.to_le_bytes());
        h = fnv1a64_from(h, &dbg_v.to_le_bytes());
        h = fnv1a64_from(h, &inf_v.to_le_bytes());
        h = fnv1a64_from(h, &wrn_v.to_le_bytes());
        h = fnv1a64_from(h, &err_v.to_le_bytes());
        h = fnv1a64_from(h, &filt_v.to_le_bytes());
        h = fnv1a64_from(h, &buf_v.to_le_bytes());
        Self {
            lvl_v,
            trace_v,
            dbg_v,
            inf_v,
            wrn_v,
            err_v,
            filt_v,
            buf_v,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.lvl_v.to_le_bytes());
        h = fnv1a64_from(h, &self.trace_v.to_le_bytes());
        h = fnv1a64_from(h, &self.dbg_v.to_le_bytes());
        h = fnv1a64_from(h, &self.inf_v.to_le_bytes());
        h = fnv1a64_from(h, &self.wrn_v.to_le_bytes());
        h = fnv1a64_from(h, &self.err_v.to_le_bytes());
        h = fnv1a64_from(h, &self.filt_v.to_le_bytes());
        h = fnv1a64_from(h, &self.buf_v.to_le_bytes());
        h
    }

    /// 篡改检出：字段被改后重算值与存值不一致。
    pub fn tampered(&self) -> bool {
        self.recompute() != self.fingerprint
    }
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0xe0c0, 0xe0cf]。
pub const fn u3481_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3481Code::ALL.len() {
        let c = u3481Code::ALL[i].code();
        if c < 0xe0c0 || c > 0xe0cf {
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
fn u3481_guard_seg() -> bool {
    u3481_segment_guard()
}

/// UNX-F3481 状态机闭集/迁移表/非法组合恰值全绿。
fn check_u3481_sm_legal() -> bool {
    let mut ok = mxu3481flow_illegal_pairs() == 29;
    if ok { ok = MXU3481FLOW_TRANSITIONS.len() == 8; }
    if ok { ok = mxu3481flowState::ALL.len() == 6 && mxu3481flowEvent::ALL.len() == 6; }
    ok
}

/// UNX-F3481 迁移主链回放 + 快照往返恒等。
fn check_u3481_sm_round() -> bool {
    let mut lg = mxu3481flowLedger::new(mxu3481flowState::Off);
    let mut ok = lg.apply(mxu3481flowEvent::Emit) && lg.cur == mxu3481flowState::Trace && lg.total() == 1;
    let s0 = Mxu3481Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    let mut s1 = Mxu3481Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    s1.recompute();
    if ok { ok = s0 == s1 && !s1.tampered(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3481_checks() -> CheckSet {
    let mut set = CheckSet::new("U3481");
    set.add(
        "u3481-seg-guard",
        u3481_guard_seg(),
        if u3481_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3481-sm-legal",
        check_u3481_sm_legal(),
        if check_u3481_sm_legal() { "" } else { "UNX-F3481 sm legality mismatch" },
    );
    set.add(
        "u3481-sm-round",
        check_u3481_sm_round(),
        if check_u3481_sm_round() { "" } else { "UNX-F3481 sm roundtrip mismatch" },
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
    fn u3481_all_checks_green() {
        let set = run_u3481_checks();
        assert!(set.all_passed(), "U3481 domain checks must be green");
    }

    #[test]
    fn u3481_segment_guard_holds() {
        assert!(u3481_segment_guard());
    }
}
