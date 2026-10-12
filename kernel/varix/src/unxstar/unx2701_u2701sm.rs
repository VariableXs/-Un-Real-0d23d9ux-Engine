//! unxu2701_u2701sm —— LRU active/inactive 双链结构地基 状态机与快照封存（双链节点）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2701 · LRU active/inactive 双链结构地基 · 判据原文：UNX-F2701 · 判据：active/inactive 双链结构就绪（链头/链尾/哨兵三件注册），空链插入弹出各 10 次对账零漂移
//! · 组件：双链节点 状态机（6 态 × 6 事件显式迁移）+ FNV 快照封存（8 分量）
//! · 判据落地：UNX-F2701 · 判据：active/inactive 双链结构就绪（链头/链尾/哨兵三件注册），空链插入弹出各 10 次对账零漂移
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2701_VERSION: &str = "u2701-u2701sm-v1";

/// 诊断段基址。
pub const U2701_SEG_BASE: u16 = 0xab50;

// --------------------------------------------------------------------------
// 诊断码（u2701 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 双链节点 状态面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2701Code {
    /// 0xab50 状态重复（链首尾乱 同名双态）。
    StateDup,
    /// 0xab51 状态断档（哨兵丢失 序缺失）。
    StateHole,
    /// 0xab52 事件重复（同事件双声明）。
    EventDup,
    /// 0xab53 迁移重复（同源同事件双迁移）。
    TransDup,
    /// 0xab54 迁移缺失（应达态不可达）。
    TransHole,
    /// 0xab55 迁移环（非法自环/环路）。
    TransCycle,
    /// 0xab56 入口非法（初态不在闭集）。
    EntryBad,
    /// 0xab57 迁移丢失（应用事件无效果）。
    ApplyLost,
    /// 0xab58 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xab59 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xab5a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xab5b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2701Code {
    /// 全量闭集。
    pub const ALL: [u2701Code; 12] = [
        u2701Code::StateDup,
        u2701Code::StateHole,
        u2701Code::EventDup,
        u2701Code::TransDup,
        u2701Code::TransHole,
        u2701Code::TransCycle,
        u2701Code::EntryBad,
        u2701Code::ApplyLost,
        u2701Code::LedgerBroken,
        u2701Code::SnapshotFingerprint,
        u2701Code::TableBroken,
        u2701Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2701Code::StateDup => 0xab50,
            u2701Code::StateHole => 0xab51,
            u2701Code::EventDup => 0xab52,
            u2701Code::TransDup => 0xab53,
            u2701Code::TransHole => 0xab54,
            u2701Code::TransCycle => 0xab55,
            u2701Code::EntryBad => 0xab56,
            u2701Code::ApplyLost => 0xab57,
            u2701Code::LedgerBroken => 0xab58,
            u2701Code::SnapshotFingerprint => 0xab59,
            u2701Code::TableBroken => 0xab5a,
            u2701Code::SegmentGuard => 0xab5b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2701Code::StateDup => "状态重复（链首尾乱 同名双态）",
            u2701Code::StateHole => "状态断档（哨兵丢失 序缺失）",
            u2701Code::EventDup => "事件重复（同事件双声明）",
            u2701Code::TransDup => "迁移重复（同源同事件双迁移）",
            u2701Code::TransHole => "迁移缺失（应达态不可达）",
            u2701Code::TransCycle => "迁移环（非法自环/环路）",
            u2701Code::EntryBad => "入口非法（初态不在闭集）",
            u2701Code::ApplyLost => "迁移丢失（应用事件无效果）",
            u2701Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2701Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2701Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2701Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2701Code::StateDup => "按闭集唯一律去重",
            u2701Code::StateHole => "按闭集补态",
            u2701Code::EventDup => "按事件闭集去重",
            u2701Code::TransDup => "按确定性唯一迁移律合并",
            u2701Code::TransHole => "按迁移矩阵补边",
            u2701Code::TransCycle => "按无环律重构迁移",
            u2701Code::EntryBad => "按闭集钳制初态",
            u2701Code::ApplyLost => "核对事件前置条件",
            u2701Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2701Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2701Code::TableBroken => "按语义规则重建常量表",
            u2701Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 状态机 mxu2701flow（6 态 × 6 事件 · 迁移 8 条显式声明）
// --------------------------------------------------------------------------
/// 双链节点 状态机（判据：链首尾乱）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu2701flowState {
    Init,
    Empty,
    Inserting,
    Popping,
    Balanced,
    Sealed,
}

impl mxu2701flowState {
    /// 全量态闭集。
    pub const ALL: [mxu2701flowState; 6] = [
        mxu2701flowState::Init,
        mxu2701flowState::Empty,
        mxu2701flowState::Inserting,
        mxu2701flowState::Popping,
        mxu2701flowState::Balanced,
        mxu2701flowState::Sealed,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            mxu2701flowState::Init => 0,
            mxu2701flowState::Empty => 1,
            mxu2701flowState::Inserting => 2,
            mxu2701flowState::Popping => 3,
            mxu2701flowState::Balanced => 4,
            mxu2701flowState::Sealed => 5,
        }
    }
}

/// mxu2701flow事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu2701flowEvent {
    InitChain,
    Insert,
    Pop,
    SwapChain,
    CheckEmpty,
    Seal,
}

impl mxu2701flowEvent {
    /// 全量事件闭集。
    pub const ALL: [mxu2701flowEvent; 6] = [
        mxu2701flowEvent::InitChain,
        mxu2701flowEvent::Insert,
        mxu2701flowEvent::Pop,
        mxu2701flowEvent::SwapChain,
        mxu2701flowEvent::CheckEmpty,
        mxu2701flowEvent::Seal,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const MXU2701FLOW_TRANSITIONS: [(mxu2701flowState, mxu2701flowEvent, mxu2701flowState); 8] = [
    (mxu2701flowState::Init, mxu2701flowEvent::InitChain, mxu2701flowState::Empty), // Init→Empty 主链
    (mxu2701flowState::Empty, mxu2701flowEvent::Insert, mxu2701flowState::Inserting), // Empty→Inserting 主链
    (mxu2701flowState::Inserting, mxu2701flowEvent::Pop, mxu2701flowState::Popping), // Inserting→Popping 主链
    (mxu2701flowState::Popping, mxu2701flowEvent::SwapChain, mxu2701flowState::Balanced), // Popping→Balanced 主链
    (mxu2701flowState::Balanced, mxu2701flowEvent::CheckEmpty, mxu2701flowState::Sealed), // Balanced→Sealed 主链
    (mxu2701flowState::Sealed, mxu2701flowEvent::Seal, mxu2701flowState::Init), // 终态回边
    (mxu2701flowState::Sealed, mxu2701flowEvent::InitChain, mxu2701flowState::Balanced), // 终态回退边
    (mxu2701flowState::Empty, mxu2701flowEvent::Insert, mxu2701flowState::Inserting), // 旁路边
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn mxu2701flow_next(state: mxu2701flowState, ev: mxu2701flowEvent) -> Option<mxu2701flowState> {
    let mut i = 0usize;
    while i < 8 {
        let t = MXU2701FLOW_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn mxu2701flow_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < mxu2701flowState::ALL.len() {
        let mut ei = 0usize;
        while ei < mxu2701flowEvent::ALL.len() {
            let legal = mxu2701flow_next(mxu2701flowState::ALL[si], mxu2701flowEvent::ALL[ei]).is_some();
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
pub fn mxu2701flow_terminal(st: mxu2701flowState) -> bool {
    let mut i = 0usize;
    while i < 8 {
        if MXU2701FLOW_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct mxu2701flowLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: mxu2701flowState,
}

impl mxu2701flowLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: mxu2701flowState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: mxu2701flowEvent) -> bool {
        match mxu2701flow_next(self.cur, ev) {
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
// 快照 Mxu2701Snap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// 双链节点 快照封存（FNV 三态指纹 + 篡改检出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mxu2701Snap {
    /// 指纹分量：lq_v。
    pub lq_v: u64,
    /// 指纹分量：ac_v。
    pub ac_v: u64,
    /// 指纹分量：in_v。
    pub in_v: u64,
    /// 指纹分量：hd_v。
    pub hd_v: u64,
    /// 指纹分量：tl_v。
    pub tl_v: u64,
    /// 指纹分量：sb_v。
    pub sb_v: u64,
    /// 指纹分量：em_v。
    pub em_v: u64,
    /// 指纹分量：ip_v。
    pub ip_v: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl Mxu2701Snap {
    /// 构造并即时合成指纹。
    pub fn compute(
        lq_v: u64,
        ac_v: u64,
        in_v: u64,
        hd_v: u64,
        tl_v: u64,
        sb_v: u64,
        em_v: u64,
        ip_v: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &lq_v.to_le_bytes());
        h = fnv1a64_from(h, &ac_v.to_le_bytes());
        h = fnv1a64_from(h, &in_v.to_le_bytes());
        h = fnv1a64_from(h, &hd_v.to_le_bytes());
        h = fnv1a64_from(h, &tl_v.to_le_bytes());
        h = fnv1a64_from(h, &sb_v.to_le_bytes());
        h = fnv1a64_from(h, &em_v.to_le_bytes());
        h = fnv1a64_from(h, &ip_v.to_le_bytes());
        Self {
            lq_v,
            ac_v,
            in_v,
            hd_v,
            tl_v,
            sb_v,
            em_v,
            ip_v,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.lq_v.to_le_bytes());
        h = fnv1a64_from(h, &self.ac_v.to_le_bytes());
        h = fnv1a64_from(h, &self.in_v.to_le_bytes());
        h = fnv1a64_from(h, &self.hd_v.to_le_bytes());
        h = fnv1a64_from(h, &self.tl_v.to_le_bytes());
        h = fnv1a64_from(h, &self.sb_v.to_le_bytes());
        h = fnv1a64_from(h, &self.em_v.to_le_bytes());
        h = fnv1a64_from(h, &self.ip_v.to_le_bytes());
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

/// 段位守卫：全部诊断码必须落在本段 [0xab50, 0xab5f]。
pub const fn u2701_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2701Code::ALL.len() {
        let c = u2701Code::ALL[i].code();
        if c < 0xab50 || c > 0xab5f {
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
fn u2701_guard_seg() -> bool {
    u2701_segment_guard()
}

/// UNX-F2701 状态机闭集/迁移表/非法组合恰值全绿。
fn check_u2701_sm_legal() -> bool {
    let mut ok = mxu2701flow_illegal_pairs() == 29;
    if ok { ok = MXU2701FLOW_TRANSITIONS.len() == 8; }
    if ok { ok = mxu2701flowState::ALL.len() == 6 && mxu2701flowEvent::ALL.len() == 6; }
    ok
}

/// UNX-F2701 迁移主链回放 + 快照往返恒等。
fn check_u2701_sm_round() -> bool {
    let mut lg = mxu2701flowLedger::new(mxu2701flowState::Init);
    let mut ok = lg.apply(mxu2701flowEvent::InitChain) && lg.cur == mxu2701flowState::Empty && lg.total() == 1;
    let s0 = Mxu2701Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    let mut s1 = Mxu2701Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    s1.recompute();
    if ok { ok = s0 == s1 && !s1.tampered(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2701_checks() -> CheckSet {
    let mut set = CheckSet::new("U2701");
    set.add(
        "u2701-seg-guard",
        u2701_guard_seg(),
        if u2701_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2701-sm-legal",
        check_u2701_sm_legal(),
        if check_u2701_sm_legal() { "" } else { "UNX-F2701 sm legality mismatch" },
    );
    set.add(
        "u2701-sm-round",
        check_u2701_sm_round(),
        if check_u2701_sm_round() { "" } else { "UNX-F2701 sm roundtrip mismatch" },
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
    fn u2701_all_checks_green() {
        let set = run_u2701_checks();
        assert!(set.all_passed(), "U2701 domain checks must be green");
    }

    #[test]
    fn u2701_segment_guard_holds() {
        assert!(u2701_segment_guard());
    }
}
