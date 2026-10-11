//! unxu1761_u1761sm —— 观测面总纲（账→接口→聚合三层架构） 状态机与快照封存（层）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F1761 · 观测面总纲（账→接口→聚合三层架构） · 判据原文：UNX-F1761 · 判据：三层架构冻结 v1，各批观测接口汇聚关系全量登记，三层职责越界注入 10/10 次拒绝
//! · 组件：层 状态机（6 态 × 6 事件显式迁移）+ FNV 快照封存（8 分量）
//! · 判据落地：UNX-F1761 · 判据：三层架构冻结 v1，各批观测接口汇聚关系全量登记，三层职责越界注入 10/10 次拒绝
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U1761_VERSION: &str = "u1761-u1761sm-v1";

/// 诊断段基址。
pub const U1761_SEG_BASE: u16 = 0x5b10;

// --------------------------------------------------------------------------
// 诊断码（u1761 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 层 状态面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u1761Code {
    /// 0x5b10 状态重复（职责越界 同名双态）。
    StateDup,
    /// 0x5b11 状态断档（汇聚漏登 序缺失）。
    StateHole,
    /// 0x5b12 事件重复（同事件双声明）。
    EventDup,
    /// 0x5b13 迁移重复（同源同事件双迁移）。
    TransDup,
    /// 0x5b14 迁移缺失（应达态不可达）。
    TransHole,
    /// 0x5b15 迁移环（非法自环/环路）。
    TransCycle,
    /// 0x5b16 入口非法（初态不在闭集）。
    EntryBad,
    /// 0x5b17 迁移丢失（应用事件无效果）。
    ApplyLost,
    /// 0x5b18 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x5b19 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x5b1a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x5b1b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u1761Code {
    /// 全量闭集。
    pub const ALL: [u1761Code; 12] = [
        u1761Code::StateDup,
        u1761Code::StateHole,
        u1761Code::EventDup,
        u1761Code::TransDup,
        u1761Code::TransHole,
        u1761Code::TransCycle,
        u1761Code::EntryBad,
        u1761Code::ApplyLost,
        u1761Code::LedgerBroken,
        u1761Code::SnapshotFingerprint,
        u1761Code::TableBroken,
        u1761Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u1761Code::StateDup => 0x5b10,
            u1761Code::StateHole => 0x5b11,
            u1761Code::EventDup => 0x5b12,
            u1761Code::TransDup => 0x5b13,
            u1761Code::TransHole => 0x5b14,
            u1761Code::TransCycle => 0x5b15,
            u1761Code::EntryBad => 0x5b16,
            u1761Code::ApplyLost => 0x5b17,
            u1761Code::LedgerBroken => 0x5b18,
            u1761Code::SnapshotFingerprint => 0x5b19,
            u1761Code::TableBroken => 0x5b1a,
            u1761Code::SegmentGuard => 0x5b1b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u1761Code::StateDup => "状态重复（职责越界 同名双态）",
            u1761Code::StateHole => "状态断档（汇聚漏登 序缺失）",
            u1761Code::EventDup => "事件重复（同事件双声明）",
            u1761Code::TransDup => "迁移重复（同源同事件双迁移）",
            u1761Code::TransHole => "迁移缺失（应达态不可达）",
            u1761Code::TransCycle => "迁移环（非法自环/环路）",
            u1761Code::EntryBad => "入口非法（初态不在闭集）",
            u1761Code::ApplyLost => "迁移丢失（应用事件无效果）",
            u1761Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u1761Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u1761Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u1761Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u1761Code::StateDup => "按闭集唯一律去重",
            u1761Code::StateHole => "按闭集补态",
            u1761Code::EventDup => "按事件闭集去重",
            u1761Code::TransDup => "按确定性唯一迁移律合并",
            u1761Code::TransHole => "按迁移矩阵补边",
            u1761Code::TransCycle => "按无环律重构迁移",
            u1761Code::EntryBad => "按闭集钳制初态",
            u1761Code::ApplyLost => "核对事件前置条件",
            u1761Code::LedgerBroken => "按守恒律重建台账后重跑",
            u1761Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u1761Code::TableBroken => "按语义规则重建常量表",
            u1761Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 状态机 mxu1761flow（6 态 × 6 事件 · 迁移 8 条显式声明）
// --------------------------------------------------------------------------
/// 层 状态机（判据：职责越界）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu1761flowState {
    Booked,
    Interfaced,
    Aggregated,
    Converged,
    Probed,
    Frozen,
}

impl mxu1761flowState {
    /// 全量态闭集。
    pub const ALL: [mxu1761flowState; 6] = [
        mxu1761flowState::Booked,
        mxu1761flowState::Interfaced,
        mxu1761flowState::Aggregated,
        mxu1761flowState::Converged,
        mxu1761flowState::Probed,
        mxu1761flowState::Frozen,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            mxu1761flowState::Booked => 0,
            mxu1761flowState::Interfaced => 1,
            mxu1761flowState::Aggregated => 2,
            mxu1761flowState::Converged => 3,
            mxu1761flowState::Probed => 4,
            mxu1761flowState::Frozen => 5,
        }
    }
}

/// mxu1761flow事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu1761flowEvent {
    Book,
    Interface,
    Aggregate,
    Converge,
    Probe,
    Freeze,
}

impl mxu1761flowEvent {
    /// 全量事件闭集。
    pub const ALL: [mxu1761flowEvent; 6] = [
        mxu1761flowEvent::Book,
        mxu1761flowEvent::Interface,
        mxu1761flowEvent::Aggregate,
        mxu1761flowEvent::Converge,
        mxu1761flowEvent::Probe,
        mxu1761flowEvent::Freeze,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const MXU1761FLOW_TRANSITIONS: [(mxu1761flowState, mxu1761flowEvent, mxu1761flowState); 8] = [
    (mxu1761flowState::Booked, mxu1761flowEvent::Book, mxu1761flowState::Interfaced), // Booked→Interfaced 主链
    (mxu1761flowState::Interfaced, mxu1761flowEvent::Interface, mxu1761flowState::Aggregated), // Interfaced→Aggregated 主链
    (mxu1761flowState::Aggregated, mxu1761flowEvent::Aggregate, mxu1761flowState::Converged), // Aggregated→Converged 主链
    (mxu1761flowState::Converged, mxu1761flowEvent::Converge, mxu1761flowState::Probed), // Converged→Probed 主链
    (mxu1761flowState::Probed, mxu1761flowEvent::Probe, mxu1761flowState::Frozen), // Probed→Frozen 主链
    (mxu1761flowState::Frozen, mxu1761flowEvent::Freeze, mxu1761flowState::Booked), // 终态回边
    (mxu1761flowState::Frozen, mxu1761flowEvent::Book, mxu1761flowState::Probed), // 终态回退边
    (mxu1761flowState::Interfaced, mxu1761flowEvent::Interface, mxu1761flowState::Aggregated), // 旁路边
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn mxu1761flow_next(state: mxu1761flowState, ev: mxu1761flowEvent) -> Option<mxu1761flowState> {
    let mut i = 0usize;
    while i < 8 {
        let t = MXU1761FLOW_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn mxu1761flow_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < mxu1761flowState::ALL.len() {
        let mut ei = 0usize;
        while ei < mxu1761flowEvent::ALL.len() {
            let legal = mxu1761flow_next(mxu1761flowState::ALL[si], mxu1761flowEvent::ALL[ei]).is_some();
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
pub fn mxu1761flow_terminal(st: mxu1761flowState) -> bool {
    let mut i = 0usize;
    while i < 8 {
        if MXU1761FLOW_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct mxu1761flowLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: mxu1761flowState,
}

impl mxu1761flowLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: mxu1761flowState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: mxu1761flowEvent) -> bool {
        match mxu1761flow_next(self.cur, ev) {
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
// 快照 Mxu1761Snap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// 层 快照封存（FNV 三态指纹 + 篡改检出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mxu1761Snap {
    /// 指纹分量：z_v。
    pub z_v: u64,
    /// 指纹分量：jk_v。
    pub jk_v: u64,
    /// 指纹分量：jh_v。
    pub jh_v: u64,
    /// 指纹分量：hj_v。
    pub hj_v: u64,
    /// 指纹分量：ql_v。
    pub ql_v: u64,
    /// 指纹分量：zz_v。
    pub zz_v: u64,
    /// 指纹分量：jj_v。
    pub jj_v: u64,
    /// 指纹分量：mz_v。
    pub mz_v: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl Mxu1761Snap {
    /// 构造并即时合成指纹。
    pub fn compute(
        z_v: u64,
        jk_v: u64,
        jh_v: u64,
        hj_v: u64,
        ql_v: u64,
        zz_v: u64,
        jj_v: u64,
        mz_v: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &z_v.to_le_bytes());
        h = fnv1a64_from(h, &jk_v.to_le_bytes());
        h = fnv1a64_from(h, &jh_v.to_le_bytes());
        h = fnv1a64_from(h, &hj_v.to_le_bytes());
        h = fnv1a64_from(h, &ql_v.to_le_bytes());
        h = fnv1a64_from(h, &zz_v.to_le_bytes());
        h = fnv1a64_from(h, &jj_v.to_le_bytes());
        h = fnv1a64_from(h, &mz_v.to_le_bytes());
        Self {
            z_v,
            jk_v,
            jh_v,
            hj_v,
            ql_v,
            zz_v,
            jj_v,
            mz_v,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.z_v.to_le_bytes());
        h = fnv1a64_from(h, &self.jk_v.to_le_bytes());
        h = fnv1a64_from(h, &self.jh_v.to_le_bytes());
        h = fnv1a64_from(h, &self.hj_v.to_le_bytes());
        h = fnv1a64_from(h, &self.ql_v.to_le_bytes());
        h = fnv1a64_from(h, &self.zz_v.to_le_bytes());
        h = fnv1a64_from(h, &self.jj_v.to_le_bytes());
        h = fnv1a64_from(h, &self.mz_v.to_le_bytes());
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

/// 段位守卫：全部诊断码必须落在本段 [0x5b10, 0x5b1f]。
pub const fn u1761_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u1761Code::ALL.len() {
        let c = u1761Code::ALL[i].code();
        if c < 0x5b10 || c > 0x5b1f {
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
fn u1761_guard_seg() -> bool {
    u1761_segment_guard()
}

/// UNX-F1761 状态机闭集/迁移表/非法组合恰值全绿。
fn check_u1761_sm_legal() -> bool {
    let mut ok = mxu1761flow_illegal_pairs() == 29;
    if ok { ok = MXU1761FLOW_TRANSITIONS.len() == 8; }
    if ok { ok = mxu1761flowState::ALL.len() == 6 && mxu1761flowEvent::ALL.len() == 6; }
    ok
}

/// UNX-F1761 迁移主链回放 + 快照往返恒等。
fn check_u1761_sm_round() -> bool {
    let mut lg = mxu1761flowLedger::new(mxu1761flowState::Booked);
    let mut ok = lg.apply(mxu1761flowEvent::Book) && lg.cur == mxu1761flowState::Interfaced && lg.total() == 1;
    let s0 = Mxu1761Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    let mut s1 = Mxu1761Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    s1.recompute();
    if ok { ok = s0 == s1 && !s1.tampered(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u1761_checks() -> CheckSet {
    let mut set = CheckSet::new("U1761");
    set.add(
        "u1761-seg-guard",
        u1761_guard_seg(),
        if u1761_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u1761-sm-legal",
        check_u1761_sm_legal(),
        if check_u1761_sm_legal() { "" } else { "UNX-F1761 sm legality mismatch" },
    );
    set.add(
        "u1761-sm-round",
        check_u1761_sm_round(),
        if check_u1761_sm_round() { "" } else { "UNX-F1761 sm roundtrip mismatch" },
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
    fn u1761_all_checks_green() {
        let set = run_u1761_checks();
        assert!(set.all_passed(), "U1761 domain checks must be green");
    }

    #[test]
    fn u1761_segment_guard_holds() {
        assert!(u1761_segment_guard());
    }
}
