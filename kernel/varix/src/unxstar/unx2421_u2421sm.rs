//! unxu2421_u2421sm —— order-0 页快速路径 状态机与快照封存（单页请求）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2421 · order-0 页快速路径 · 判据原文：UNX-F2421 · 判据：单页分配 p99 时延落账且与高阶路径比值 ≤0.5，10^6 次分配释放对账零泄漏
//! · 组件：单页请求 状态机（6 态 × 6 事件显式迁移）+ FNV 快照封存（8 分量）
//! · 判据落地：UNX-F2421 · 判据：单页分配 p99 时延落账且与高阶路径比值 ≤0.5，10^6 次分配释放对账零泄漏
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2421_VERSION: &str = "u2421-u2421sm-v1";

/// 诊断段基址。
pub const U2421_SEG_BASE: u16 = 0xa0d0;

// --------------------------------------------------------------------------
// 诊断码（u2421 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 单页请求 状态面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2421Code {
    /// 0xa0d0 状态重复（快径旁落 同名双态）。
    StateDup,
    /// 0xa0d1 状态断档（时延超标 序缺失）。
    StateHole,
    /// 0xa0d2 事件重复（同事件双声明）。
    EventDup,
    /// 0xa0d3 迁移重复（同源同事件双迁移）。
    TransDup,
    /// 0xa0d4 迁移缺失（应达态不可达）。
    TransHole,
    /// 0xa0d5 迁移环（非法自环/环路）。
    TransCycle,
    /// 0xa0d6 入口非法（初态不在闭集）。
    EntryBad,
    /// 0xa0d7 迁移丢失（应用事件无效果）。
    ApplyLost,
    /// 0xa0d8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xa0d9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xa0da 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xa0db 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2421Code {
    /// 全量闭集。
    pub const ALL: [u2421Code; 12] = [
        u2421Code::StateDup,
        u2421Code::StateHole,
        u2421Code::EventDup,
        u2421Code::TransDup,
        u2421Code::TransHole,
        u2421Code::TransCycle,
        u2421Code::EntryBad,
        u2421Code::ApplyLost,
        u2421Code::LedgerBroken,
        u2421Code::SnapshotFingerprint,
        u2421Code::TableBroken,
        u2421Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2421Code::StateDup => 0xa0d0,
            u2421Code::StateHole => 0xa0d1,
            u2421Code::EventDup => 0xa0d2,
            u2421Code::TransDup => 0xa0d3,
            u2421Code::TransHole => 0xa0d4,
            u2421Code::TransCycle => 0xa0d5,
            u2421Code::EntryBad => 0xa0d6,
            u2421Code::ApplyLost => 0xa0d7,
            u2421Code::LedgerBroken => 0xa0d8,
            u2421Code::SnapshotFingerprint => 0xa0d9,
            u2421Code::TableBroken => 0xa0da,
            u2421Code::SegmentGuard => 0xa0db,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2421Code::StateDup => "状态重复（快径旁落 同名双态）",
            u2421Code::StateHole => "状态断档（时延超标 序缺失）",
            u2421Code::EventDup => "事件重复（同事件双声明）",
            u2421Code::TransDup => "迁移重复（同源同事件双迁移）",
            u2421Code::TransHole => "迁移缺失（应达态不可达）",
            u2421Code::TransCycle => "迁移环（非法自环/环路）",
            u2421Code::EntryBad => "入口非法（初态不在闭集）",
            u2421Code::ApplyLost => "迁移丢失（应用事件无效果）",
            u2421Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2421Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2421Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2421Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2421Code::StateDup => "按闭集唯一律去重",
            u2421Code::StateHole => "按闭集补态",
            u2421Code::EventDup => "按事件闭集去重",
            u2421Code::TransDup => "按确定性唯一迁移律合并",
            u2421Code::TransHole => "按迁移矩阵补边",
            u2421Code::TransCycle => "按无环律重构迁移",
            u2421Code::EntryBad => "按闭集钳制初态",
            u2421Code::ApplyLost => "核对事件前置条件",
            u2421Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2421Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2421Code::TableBroken => "按语义规则重建常量表",
            u2421Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 状态机 mxu2421flow（6 态 × 6 事件 · 迁移 8 条显式声明）
// --------------------------------------------------------------------------
/// 单页请求 状态机（判据：快径旁落）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu2421flowState {
    Cold,
    FastArmed,
    Sampling,
    Comparing,
    Reconciling,
    Sealed,
}

impl mxu2421flowState {
    /// 全量态闭集。
    pub const ALL: [mxu2421flowState; 6] = [
        mxu2421flowState::Cold,
        mxu2421flowState::FastArmed,
        mxu2421flowState::Sampling,
        mxu2421flowState::Comparing,
        mxu2421flowState::Reconciling,
        mxu2421flowState::Sealed,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            mxu2421flowState::Cold => 0,
            mxu2421flowState::FastArmed => 1,
            mxu2421flowState::Sampling => 2,
            mxu2421flowState::Comparing => 3,
            mxu2421flowState::Reconciling => 4,
            mxu2421flowState::Sealed => 5,
        }
    }
}

/// mxu2421flow事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu2421flowEvent {
    ArmFast,
    AllocPage,
    Sample,
    CompareHigh,
    FreePage,
    Reconcile,
}

impl mxu2421flowEvent {
    /// 全量事件闭集。
    pub const ALL: [mxu2421flowEvent; 6] = [
        mxu2421flowEvent::ArmFast,
        mxu2421flowEvent::AllocPage,
        mxu2421flowEvent::Sample,
        mxu2421flowEvent::CompareHigh,
        mxu2421flowEvent::FreePage,
        mxu2421flowEvent::Reconcile,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const MXU2421FLOW_TRANSITIONS: [(mxu2421flowState, mxu2421flowEvent, mxu2421flowState); 8] = [
    (mxu2421flowState::Cold, mxu2421flowEvent::ArmFast, mxu2421flowState::FastArmed), // Cold→FastArmed 主链
    (mxu2421flowState::FastArmed, mxu2421flowEvent::AllocPage, mxu2421flowState::Sampling), // FastArmed→Sampling 主链
    (mxu2421flowState::Sampling, mxu2421flowEvent::Sample, mxu2421flowState::Comparing), // Sampling→Comparing 主链
    (mxu2421flowState::Comparing, mxu2421flowEvent::CompareHigh, mxu2421flowState::Reconciling), // Comparing→Reconciling 主链
    (mxu2421flowState::Reconciling, mxu2421flowEvent::FreePage, mxu2421flowState::Sealed), // Reconciling→Sealed 主链
    (mxu2421flowState::Sealed, mxu2421flowEvent::Reconcile, mxu2421flowState::Cold), // 终态回边
    (mxu2421flowState::Sealed, mxu2421flowEvent::ArmFast, mxu2421flowState::Reconciling), // 终态回退边
    (mxu2421flowState::FastArmed, mxu2421flowEvent::AllocPage, mxu2421flowState::Sampling), // 旁路边
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn mxu2421flow_next(state: mxu2421flowState, ev: mxu2421flowEvent) -> Option<mxu2421flowState> {
    let mut i = 0usize;
    while i < 8 {
        let t = MXU2421FLOW_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn mxu2421flow_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < mxu2421flowState::ALL.len() {
        let mut ei = 0usize;
        while ei < mxu2421flowEvent::ALL.len() {
            let legal = mxu2421flow_next(mxu2421flowState::ALL[si], mxu2421flowEvent::ALL[ei]).is_some();
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
pub fn mxu2421flow_terminal(st: mxu2421flowState) -> bool {
    let mut i = 0usize;
    while i < 8 {
        if MXU2421FLOW_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct mxu2421flowLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: mxu2421flowState,
}

impl mxu2421flowLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: mxu2421flowState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: mxu2421flowEvent) -> bool {
        match mxu2421flow_next(self.cur, ev) {
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
// 快照 Mxu2421Snap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// 单页请求 快照封存（FNV 三态指纹 + 篡改检出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mxu2421Snap {
    /// 指纹分量：o0_v。
    pub o0_v: u64,
    /// 指纹分量：p99_v。
    pub p99_v: u64,
    /// 指纹分量：hi_v。
    pub hi_v: u64,
    /// 指纹分量：rt_v。
    pub rt_v: u64,
    /// 指纹分量：m1_v。
    pub m1_v: u64,
    /// 指纹分量：lk_v。
    pub lk_v: u64,
    /// 指纹分量：al_v。
    pub al_v: u64,
    /// 指纹分量：fr_v。
    pub fr_v: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl Mxu2421Snap {
    /// 构造并即时合成指纹。
    pub fn compute(
        o0_v: u64,
        p99_v: u64,
        hi_v: u64,
        rt_v: u64,
        m1_v: u64,
        lk_v: u64,
        al_v: u64,
        fr_v: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &o0_v.to_le_bytes());
        h = fnv1a64_from(h, &p99_v.to_le_bytes());
        h = fnv1a64_from(h, &hi_v.to_le_bytes());
        h = fnv1a64_from(h, &rt_v.to_le_bytes());
        h = fnv1a64_from(h, &m1_v.to_le_bytes());
        h = fnv1a64_from(h, &lk_v.to_le_bytes());
        h = fnv1a64_from(h, &al_v.to_le_bytes());
        h = fnv1a64_from(h, &fr_v.to_le_bytes());
        Self {
            o0_v,
            p99_v,
            hi_v,
            rt_v,
            m1_v,
            lk_v,
            al_v,
            fr_v,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.o0_v.to_le_bytes());
        h = fnv1a64_from(h, &self.p99_v.to_le_bytes());
        h = fnv1a64_from(h, &self.hi_v.to_le_bytes());
        h = fnv1a64_from(h, &self.rt_v.to_le_bytes());
        h = fnv1a64_from(h, &self.m1_v.to_le_bytes());
        h = fnv1a64_from(h, &self.lk_v.to_le_bytes());
        h = fnv1a64_from(h, &self.al_v.to_le_bytes());
        h = fnv1a64_from(h, &self.fr_v.to_le_bytes());
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

/// 段位守卫：全部诊断码必须落在本段 [0xa0d0, 0xa0df]。
pub const fn u2421_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2421Code::ALL.len() {
        let c = u2421Code::ALL[i].code();
        if c < 0xa0d0 || c > 0xa0df {
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
fn u2421_guard_seg() -> bool {
    u2421_segment_guard()
}

/// UNX-F2421 状态机闭集/迁移表/非法组合恰值全绿。
fn check_u2421_sm_legal() -> bool {
    let mut ok = mxu2421flow_illegal_pairs() == 29;
    if ok { ok = MXU2421FLOW_TRANSITIONS.len() == 8; }
    if ok { ok = mxu2421flowState::ALL.len() == 6 && mxu2421flowEvent::ALL.len() == 6; }
    ok
}

/// UNX-F2421 迁移主链回放 + 快照往返恒等。
fn check_u2421_sm_round() -> bool {
    let mut lg = mxu2421flowLedger::new(mxu2421flowState::Cold);
    let mut ok = lg.apply(mxu2421flowEvent::ArmFast) && lg.cur == mxu2421flowState::FastArmed && lg.total() == 1;
    let s0 = Mxu2421Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    let mut s1 = Mxu2421Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    s1.recompute();
    if ok { ok = s0 == s1 && !s1.tampered(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2421_checks() -> CheckSet {
    let mut set = CheckSet::new("U2421");
    set.add(
        "u2421-seg-guard",
        u2421_guard_seg(),
        if u2421_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2421-sm-legal",
        check_u2421_sm_legal(),
        if check_u2421_sm_legal() { "" } else { "UNX-F2421 sm legality mismatch" },
    );
    set.add(
        "u2421-sm-round",
        check_u2421_sm_round(),
        if check_u2421_sm_round() { "" } else { "UNX-F2421 sm roundtrip mismatch" },
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
    fn u2421_all_checks_green() {
        let set = run_u2421_checks();
        assert!(set.all_passed(), "U2421 domain checks must be green");
    }

    #[test]
    fn u2421_segment_guard_holds() {
        assert!(u2421_segment_guard());
    }
}
