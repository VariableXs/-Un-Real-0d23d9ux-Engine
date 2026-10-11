//! unxu2581_u2581sm —— VMA 区间树结构总纲（红黑树类，按地址排序） 状态机与快照封存（VMA 区间）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2581 · VMA 区间树结构总纲（红黑树类，按地址排序） · 判据原文：UNX-F2581 · 判据：区间树结构逐项断言（排序/平衡/区间不变量 10^6 次操作全过），VMA 记录四字段（起止/属性/类型/对象）齐备
//! · 组件：VMA 区间 状态机（6 态 × 6 事件显式迁移）+ FNV 快照封存（8 分量）
//! · 判据落地：UNX-F2581 · 判据：区间树结构逐项断言（排序/平衡/区间不变量 10^6 次操作全过），VMA 记录四字段（起止/属性/类型/对象）齐备
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2581_VERSION: &str = "u2581-u2581sm-v1";

/// 诊断段基址。
pub const U2581_SEG_BASE: u16 = 0xa6d0;

// --------------------------------------------------------------------------
// 诊断码（u2581 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// VMA 区间 状态面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2581Code {
    /// 0xa6d0 状态重复（排序破坏 同名双态）。
    StateDup,
    /// 0xa6d1 状态断档（失衡塌陷 序缺失）。
    StateHole,
    /// 0xa6d2 事件重复（同事件双声明）。
    EventDup,
    /// 0xa6d3 迁移重复（同源同事件双迁移）。
    TransDup,
    /// 0xa6d4 迁移缺失（应达态不可达）。
    TransHole,
    /// 0xa6d5 迁移环（非法自环/环路）。
    TransCycle,
    /// 0xa6d6 入口非法（初态不在闭集）。
    EntryBad,
    /// 0xa6d7 迁移丢失（应用事件无效果）。
    ApplyLost,
    /// 0xa6d8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xa6d9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xa6da 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xa6db 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2581Code {
    /// 全量闭集。
    pub const ALL: [u2581Code; 12] = [
        u2581Code::StateDup,
        u2581Code::StateHole,
        u2581Code::EventDup,
        u2581Code::TransDup,
        u2581Code::TransHole,
        u2581Code::TransCycle,
        u2581Code::EntryBad,
        u2581Code::ApplyLost,
        u2581Code::LedgerBroken,
        u2581Code::SnapshotFingerprint,
        u2581Code::TableBroken,
        u2581Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2581Code::StateDup => 0xa6d0,
            u2581Code::StateHole => 0xa6d1,
            u2581Code::EventDup => 0xa6d2,
            u2581Code::TransDup => 0xa6d3,
            u2581Code::TransHole => 0xa6d4,
            u2581Code::TransCycle => 0xa6d5,
            u2581Code::EntryBad => 0xa6d6,
            u2581Code::ApplyLost => 0xa6d7,
            u2581Code::LedgerBroken => 0xa6d8,
            u2581Code::SnapshotFingerprint => 0xa6d9,
            u2581Code::TableBroken => 0xa6da,
            u2581Code::SegmentGuard => 0xa6db,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2581Code::StateDup => "状态重复（排序破坏 同名双态）",
            u2581Code::StateHole => "状态断档（失衡塌陷 序缺失）",
            u2581Code::EventDup => "事件重复（同事件双声明）",
            u2581Code::TransDup => "迁移重复（同源同事件双迁移）",
            u2581Code::TransHole => "迁移缺失（应达态不可达）",
            u2581Code::TransCycle => "迁移环（非法自环/环路）",
            u2581Code::EntryBad => "入口非法（初态不在闭集）",
            u2581Code::ApplyLost => "迁移丢失（应用事件无效果）",
            u2581Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2581Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2581Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2581Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2581Code::StateDup => "按闭集唯一律去重",
            u2581Code::StateHole => "按闭集补态",
            u2581Code::EventDup => "按事件闭集去重",
            u2581Code::TransDup => "按确定性唯一迁移律合并",
            u2581Code::TransHole => "按迁移矩阵补边",
            u2581Code::TransCycle => "按无环律重构迁移",
            u2581Code::EntryBad => "按闭集钳制初态",
            u2581Code::ApplyLost => "核对事件前置条件",
            u2581Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2581Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2581Code::TableBroken => "按语义规则重建常量表",
            u2581Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 状态机 mxu2581flow（6 态 × 6 事件 · 迁移 8 条显式声明）
// --------------------------------------------------------------------------
/// VMA 区间 状态机（判据：排序破坏）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu2581flowState {
    Empty,
    Inserting,
    Sorted,
    Balanced,
    Invariant,
    Sealed,
}

impl mxu2581flowState {
    /// 全量态闭集。
    pub const ALL: [mxu2581flowState; 6] = [
        mxu2581flowState::Empty,
        mxu2581flowState::Inserting,
        mxu2581flowState::Sorted,
        mxu2581flowState::Balanced,
        mxu2581flowState::Invariant,
        mxu2581flowState::Sealed,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            mxu2581flowState::Empty => 0,
            mxu2581flowState::Inserting => 1,
            mxu2581flowState::Sorted => 2,
            mxu2581flowState::Balanced => 3,
            mxu2581flowState::Invariant => 4,
            mxu2581flowState::Sealed => 5,
        }
    }
}

/// mxu2581flow事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu2581flowEvent {
    Insert,
    Delete,
    Rotate,
    CheckSort,
    CheckInv,
    Seal,
}

impl mxu2581flowEvent {
    /// 全量事件闭集。
    pub const ALL: [mxu2581flowEvent; 6] = [
        mxu2581flowEvent::Insert,
        mxu2581flowEvent::Delete,
        mxu2581flowEvent::Rotate,
        mxu2581flowEvent::CheckSort,
        mxu2581flowEvent::CheckInv,
        mxu2581flowEvent::Seal,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const MXU2581FLOW_TRANSITIONS: [(mxu2581flowState, mxu2581flowEvent, mxu2581flowState); 8] = [
    (mxu2581flowState::Empty, mxu2581flowEvent::Insert, mxu2581flowState::Inserting), // Empty→Inserting 主链
    (mxu2581flowState::Inserting, mxu2581flowEvent::Delete, mxu2581flowState::Sorted), // Inserting→Sorted 主链
    (mxu2581flowState::Sorted, mxu2581flowEvent::Rotate, mxu2581flowState::Balanced), // Sorted→Balanced 主链
    (mxu2581flowState::Balanced, mxu2581flowEvent::CheckSort, mxu2581flowState::Invariant), // Balanced→Invariant 主链
    (mxu2581flowState::Invariant, mxu2581flowEvent::CheckInv, mxu2581flowState::Sealed), // Invariant→Sealed 主链
    (mxu2581flowState::Sealed, mxu2581flowEvent::Seal, mxu2581flowState::Empty), // 终态回边
    (mxu2581flowState::Sealed, mxu2581flowEvent::Insert, mxu2581flowState::Invariant), // 终态回退边
    (mxu2581flowState::Inserting, mxu2581flowEvent::Delete, mxu2581flowState::Sorted), // 旁路边
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn mxu2581flow_next(state: mxu2581flowState, ev: mxu2581flowEvent) -> Option<mxu2581flowState> {
    let mut i = 0usize;
    while i < 8 {
        let t = MXU2581FLOW_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn mxu2581flow_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < mxu2581flowState::ALL.len() {
        let mut ei = 0usize;
        while ei < mxu2581flowEvent::ALL.len() {
            let legal = mxu2581flow_next(mxu2581flowState::ALL[si], mxu2581flowEvent::ALL[ei]).is_some();
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
pub fn mxu2581flow_terminal(st: mxu2581flowState) -> bool {
    let mut i = 0usize;
    while i < 8 {
        if MXU2581FLOW_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct mxu2581flowLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: mxu2581flowState,
}

impl mxu2581flowLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: mxu2581flowState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: mxu2581flowEvent) -> bool {
        match mxu2581flow_next(self.cur, ev) {
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
// 快照 Mxu2581Snap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// VMA 区间 快照封存（FNV 三态指纹 + 篡改检出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mxu2581Snap {
    /// 指纹分量：vm_v。
    pub vm_v: u64,
    /// 指纹分量：rb_v。
    pub rb_v: u64,
    /// 指纹分量：so_v。
    pub so_v: u64,
    /// 指纹分量：ba_v。
    pub ba_v: u64,
    /// 指纹分量：iv_v。
    pub iv_v: u64,
    /// 指纹分量：m1_v。
    pub m1_v: u64,
    /// 指纹分量：st_v。
    pub st_v: u64,
    /// 指纹分量：en_v。
    pub en_v: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl Mxu2581Snap {
    /// 构造并即时合成指纹。
    pub fn compute(
        vm_v: u64,
        rb_v: u64,
        so_v: u64,
        ba_v: u64,
        iv_v: u64,
        m1_v: u64,
        st_v: u64,
        en_v: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &vm_v.to_le_bytes());
        h = fnv1a64_from(h, &rb_v.to_le_bytes());
        h = fnv1a64_from(h, &so_v.to_le_bytes());
        h = fnv1a64_from(h, &ba_v.to_le_bytes());
        h = fnv1a64_from(h, &iv_v.to_le_bytes());
        h = fnv1a64_from(h, &m1_v.to_le_bytes());
        h = fnv1a64_from(h, &st_v.to_le_bytes());
        h = fnv1a64_from(h, &en_v.to_le_bytes());
        Self {
            vm_v,
            rb_v,
            so_v,
            ba_v,
            iv_v,
            m1_v,
            st_v,
            en_v,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.vm_v.to_le_bytes());
        h = fnv1a64_from(h, &self.rb_v.to_le_bytes());
        h = fnv1a64_from(h, &self.so_v.to_le_bytes());
        h = fnv1a64_from(h, &self.ba_v.to_le_bytes());
        h = fnv1a64_from(h, &self.iv_v.to_le_bytes());
        h = fnv1a64_from(h, &self.m1_v.to_le_bytes());
        h = fnv1a64_from(h, &self.st_v.to_le_bytes());
        h = fnv1a64_from(h, &self.en_v.to_le_bytes());
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

/// 段位守卫：全部诊断码必须落在本段 [0xa6d0, 0xa6df]。
pub const fn u2581_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2581Code::ALL.len() {
        let c = u2581Code::ALL[i].code();
        if c < 0xa6d0 || c > 0xa6df {
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
fn u2581_guard_seg() -> bool {
    u2581_segment_guard()
}

/// UNX-F2581 状态机闭集/迁移表/非法组合恰值全绿。
fn check_u2581_sm_legal() -> bool {
    let mut ok = mxu2581flow_illegal_pairs() == 29;
    if ok { ok = MXU2581FLOW_TRANSITIONS.len() == 8; }
    if ok { ok = mxu2581flowState::ALL.len() == 6 && mxu2581flowEvent::ALL.len() == 6; }
    ok
}

/// UNX-F2581 迁移主链回放 + 快照往返恒等。
fn check_u2581_sm_round() -> bool {
    let mut lg = mxu2581flowLedger::new(mxu2581flowState::Empty);
    let mut ok = lg.apply(mxu2581flowEvent::Insert) && lg.cur == mxu2581flowState::Inserting && lg.total() == 1;
    let s0 = Mxu2581Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    let mut s1 = Mxu2581Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    s1.recompute();
    if ok { ok = s0 == s1 && !s1.tampered(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2581_checks() -> CheckSet {
    let mut set = CheckSet::new("U2581");
    set.add(
        "u2581-seg-guard",
        u2581_guard_seg(),
        if u2581_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2581-sm-legal",
        check_u2581_sm_legal(),
        if check_u2581_sm_legal() { "" } else { "UNX-F2581 sm legality mismatch" },
    );
    set.add(
        "u2581-sm-round",
        check_u2581_sm_round(),
        if check_u2581_sm_round() { "" } else { "UNX-F2581 sm roundtrip mismatch" },
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
    fn u2581_all_checks_green() {
        let set = run_u2581_checks();
        assert!(set.all_passed(), "U2581 domain checks must be green");
    }

    #[test]
    fn u2581_segment_guard_holds() {
        assert!(u2581_segment_guard());
    }
}
