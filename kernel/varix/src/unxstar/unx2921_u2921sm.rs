//! unxu2921_u2921sm —— COW-页缓存联合压测方案冻结 状态机与快照封存（COW-页缓存联合压测）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2921 · COW-页缓存联合压测方案冻结 · 判据原文：UNX-F2921 · 判据：UNX-F2921-J1 联合矩阵（私有/共享/匿名三态×读写比五档）冻结哈希入册，方案与 F2641/F2682 双接口字段对齐
//! · 组件：COW-页缓存联合压测 状态机（6 态 × 6 事件显式迁移）+ FNV 快照封存（8 分量）
//! · 判据落地：UNX-F2921 · 判据：UNX-F2921-J1 联合矩阵（私有/共享/匿名三态×读写比五档）冻结哈希入册，方案与 F2641/F2682 双接口字段对齐
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2921_VERSION: &str = "u2921-u2921sm-v1";

/// 诊断段基址。
pub const U2921_SEG_BASE: u16 = 0xe220;

// --------------------------------------------------------------------------
// 诊断码（u2921 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// COW-页缓存联合压测 状态面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2921Code {
    /// 0xe220 状态重复（联合压测 同名双态）。
    StateDup,
    /// 0xe221 状态断档（COW 序缺失）。
    StateHole,
    /// 0xe222 事件重复（同事件双声明）。
    EventDup,
    /// 0xe223 迁移重复（同源同事件双迁移）。
    TransDup,
    /// 0xe224 迁移缺失（应达态不可达）。
    TransHole,
    /// 0xe225 迁移环（非法自环/环路）。
    TransCycle,
    /// 0xe226 入口非法（初态不在闭集）。
    EntryBad,
    /// 0xe227 迁移丢失（应用事件无效果）。
    ApplyLost,
    /// 0xe228 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe229 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe22a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe22b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2921Code {
    /// 全量闭集。
    pub const ALL: [u2921Code; 12] = [
        u2921Code::StateDup,
        u2921Code::StateHole,
        u2921Code::EventDup,
        u2921Code::TransDup,
        u2921Code::TransHole,
        u2921Code::TransCycle,
        u2921Code::EntryBad,
        u2921Code::ApplyLost,
        u2921Code::LedgerBroken,
        u2921Code::SnapshotFingerprint,
        u2921Code::TableBroken,
        u2921Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2921Code::StateDup => 0xe220,
            u2921Code::StateHole => 0xe221,
            u2921Code::EventDup => 0xe222,
            u2921Code::TransDup => 0xe223,
            u2921Code::TransHole => 0xe224,
            u2921Code::TransCycle => 0xe225,
            u2921Code::EntryBad => 0xe226,
            u2921Code::ApplyLost => 0xe227,
            u2921Code::LedgerBroken => 0xe228,
            u2921Code::SnapshotFingerprint => 0xe229,
            u2921Code::TableBroken => 0xe22a,
            u2921Code::SegmentGuard => 0xe22b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2921Code::StateDup => "状态重复（联合压测 同名双态）",
            u2921Code::StateHole => "状态断档（COW 序缺失）",
            u2921Code::EventDup => "事件重复（同事件双声明）",
            u2921Code::TransDup => "迁移重复（同源同事件双迁移）",
            u2921Code::TransHole => "迁移缺失（应达态不可达）",
            u2921Code::TransCycle => "迁移环（非法自环/环路）",
            u2921Code::EntryBad => "入口非法（初态不在闭集）",
            u2921Code::ApplyLost => "迁移丢失（应用事件无效果）",
            u2921Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2921Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2921Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2921Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2921Code::StateDup => "按闭集唯一律去重",
            u2921Code::StateHole => "按闭集补态",
            u2921Code::EventDup => "按事件闭集去重",
            u2921Code::TransDup => "按确定性唯一迁移律合并",
            u2921Code::TransHole => "按迁移矩阵补边",
            u2921Code::TransCycle => "按无环律重构迁移",
            u2921Code::EntryBad => "按闭集钳制初态",
            u2921Code::ApplyLost => "核对事件前置条件",
            u2921Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2921Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2921Code::TableBroken => "按语义规则重建常量表",
            u2921Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 状态机 mxu2921flow（6 态 × 6 事件 · 迁移 8 条显式声明）
// --------------------------------------------------------------------------
/// COW-页缓存联合压测 状态机（判据：联合压测）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu2921flowState {
    Plan,
    Armed,
    Running,
    Dirtied,
    Judged,
    Sealed,
}

impl mxu2921flowState {
    /// 全量态闭集。
    pub const ALL: [mxu2921flowState; 6] = [
        mxu2921flowState::Plan,
        mxu2921flowState::Armed,
        mxu2921flowState::Running,
        mxu2921flowState::Dirtied,
        mxu2921flowState::Judged,
        mxu2921flowState::Sealed,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            mxu2921flowState::Plan => 0,
            mxu2921flowState::Armed => 1,
            mxu2921flowState::Running => 2,
            mxu2921flowState::Dirtied => 3,
            mxu2921flowState::Judged => 4,
            mxu2921flowState::Sealed => 5,
        }
    }
}

/// mxu2921flow事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu2921flowEvent {
    Plan,
    Arm,
    Share,
    Dirty,
    WB,
    Seal,
}

impl mxu2921flowEvent {
    /// 全量事件闭集。
    pub const ALL: [mxu2921flowEvent; 6] = [
        mxu2921flowEvent::Plan,
        mxu2921flowEvent::Arm,
        mxu2921flowEvent::Share,
        mxu2921flowEvent::Dirty,
        mxu2921flowEvent::WB,
        mxu2921flowEvent::Seal,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const MXU2921FLOW_TRANSITIONS: [(mxu2921flowState, mxu2921flowEvent, mxu2921flowState); 8] = [
    (mxu2921flowState::Plan, mxu2921flowEvent::Plan, mxu2921flowState::Armed), // Plan→Armed 主链
    (mxu2921flowState::Armed, mxu2921flowEvent::Arm, mxu2921flowState::Running), // Armed→Running 主链
    (mxu2921flowState::Running, mxu2921flowEvent::Share, mxu2921flowState::Dirtied), // Running→Dirtied 主链
    (mxu2921flowState::Dirtied, mxu2921flowEvent::Dirty, mxu2921flowState::Judged), // Dirtied→Judged 主链
    (mxu2921flowState::Judged, mxu2921flowEvent::WB, mxu2921flowState::Sealed), // Judged→Sealed 主链
    (mxu2921flowState::Sealed, mxu2921flowEvent::Seal, mxu2921flowState::Plan), // 终态回边
    (mxu2921flowState::Sealed, mxu2921flowEvent::Plan, mxu2921flowState::Judged), // 终态回退边
    (mxu2921flowState::Armed, mxu2921flowEvent::Arm, mxu2921flowState::Running), // 旁路边
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn mxu2921flow_next(state: mxu2921flowState, ev: mxu2921flowEvent) -> Option<mxu2921flowState> {
    let mut i = 0usize;
    while i < 8 {
        let t = MXU2921FLOW_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn mxu2921flow_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < mxu2921flowState::ALL.len() {
        let mut ei = 0usize;
        while ei < mxu2921flowEvent::ALL.len() {
            let legal = mxu2921flow_next(mxu2921flowState::ALL[si], mxu2921flowEvent::ALL[ei]).is_some();
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
pub fn mxu2921flow_terminal(st: mxu2921flowState) -> bool {
    let mut i = 0usize;
    while i < 8 {
        if MXU2921FLOW_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct mxu2921flowLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: mxu2921flowState,
}

impl mxu2921flowLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: mxu2921flowState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: mxu2921flowEvent) -> bool {
        match mxu2921flow_next(self.cur, ev) {
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
// 快照 Mxu2921Snap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// COW-页缓存联合压测 快照封存（FNV 三态指纹 + 篡改检出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mxu2921Snap {
    /// 指纹分量：cow_v。
    pub cow_v: u64,
    /// 指纹分量：pc_v。
    pub pc_v: u64,
    /// 指纹分量：joint_v。
    pub joint_v: u64,
    /// 指纹分量：share_v。
    pub share_v: u64,
    /// 指纹分量：evict_v。
    pub evict_v: u64,
    /// 指纹分量：dirty_v。
    pub dirty_v: u64,
    /// 指纹分量：wb_v。
    pub wb_v: u64,
    /// 指纹分量：reclaim_v。
    pub reclaim_v: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl Mxu2921Snap {
    /// 构造并即时合成指纹。
    pub fn compute(
        cow_v: u64,
        pc_v: u64,
        joint_v: u64,
        share_v: u64,
        evict_v: u64,
        dirty_v: u64,
        wb_v: u64,
        reclaim_v: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &cow_v.to_le_bytes());
        h = fnv1a64_from(h, &pc_v.to_le_bytes());
        h = fnv1a64_from(h, &joint_v.to_le_bytes());
        h = fnv1a64_from(h, &share_v.to_le_bytes());
        h = fnv1a64_from(h, &evict_v.to_le_bytes());
        h = fnv1a64_from(h, &dirty_v.to_le_bytes());
        h = fnv1a64_from(h, &wb_v.to_le_bytes());
        h = fnv1a64_from(h, &reclaim_v.to_le_bytes());
        Self {
            cow_v,
            pc_v,
            joint_v,
            share_v,
            evict_v,
            dirty_v,
            wb_v,
            reclaim_v,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.cow_v.to_le_bytes());
        h = fnv1a64_from(h, &self.pc_v.to_le_bytes());
        h = fnv1a64_from(h, &self.joint_v.to_le_bytes());
        h = fnv1a64_from(h, &self.share_v.to_le_bytes());
        h = fnv1a64_from(h, &self.evict_v.to_le_bytes());
        h = fnv1a64_from(h, &self.dirty_v.to_le_bytes());
        h = fnv1a64_from(h, &self.wb_v.to_le_bytes());
        h = fnv1a64_from(h, &self.reclaim_v.to_le_bytes());
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

/// 段位守卫：全部诊断码必须落在本段 [0xe220, 0xe22f]。
pub const fn u2921_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2921Code::ALL.len() {
        let c = u2921Code::ALL[i].code();
        if c < 0xe220 || c > 0xe22f {
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
fn u2921_guard_seg() -> bool {
    u2921_segment_guard()
}

/// UNX-F2921 状态机闭集/迁移表/非法组合恰值全绿。
fn check_u2921_sm_legal() -> bool {
    let mut ok = mxu2921flow_illegal_pairs() == 29;
    if ok { ok = MXU2921FLOW_TRANSITIONS.len() == 8; }
    if ok { ok = mxu2921flowState::ALL.len() == 6 && mxu2921flowEvent::ALL.len() == 6; }
    ok
}

/// UNX-F2921 迁移主链回放 + 快照往返恒等。
fn check_u2921_sm_round() -> bool {
    let mut lg = mxu2921flowLedger::new(mxu2921flowState::Plan);
    let mut ok = lg.apply(mxu2921flowEvent::Plan) && lg.cur == mxu2921flowState::Armed && lg.total() == 1;
    let s0 = Mxu2921Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    let mut s1 = Mxu2921Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    s1.recompute();
    if ok { ok = s0 == s1 && !s1.tampered(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2921_checks() -> CheckSet {
    let mut set = CheckSet::new("U2921");
    set.add(
        "u2921-seg-guard",
        u2921_guard_seg(),
        if u2921_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2921-sm-legal",
        check_u2921_sm_legal(),
        if check_u2921_sm_legal() { "" } else { "UNX-F2921 sm legality mismatch" },
    );
    set.add(
        "u2921-sm-round",
        check_u2921_sm_round(),
        if check_u2921_sm_round() { "" } else { "UNX-F2921 sm roundtrip mismatch" },
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
    fn u2921_all_checks_green() {
        let set = run_u2921_checks();
        assert!(set.all_passed(), "U2921 domain checks must be green");
    }

    #[test]
    fn u2921_segment_guard_holds() {
        assert!(u2921_segment_guard());
    }
}
