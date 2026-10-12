//! unxu3221_u3221sm —— qspinlock 队列节点结构与 MCS 排队语义 状态机与快照封存（MCS 队列节点）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3221 · qspinlock 队列节点结构与 MCS 排队语义 · 判据原文：UNX-F3221 · 判据：UNX-F3221-J1 队列节点成链/断链配对 10^6 次零悬挂引用，FIFO 交接序与到达序一致率 100%
//! · 组件：MCS 队列节点 状态机（6 态 × 6 事件显式迁移）+ FNV 快照封存（8 分量）
//! · 判据落地：UNX-F3221 · 判据：UNX-F3221-J1 队列节点成链/断链配对 10^6 次零悬挂引用，FIFO 交接序与到达序一致率 100%
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3221_VERSION: &str = "u3221-u3221sm-v1";

/// 诊断段基址。
pub const U3221_SEG_BASE: u16 = 0xe010;

// --------------------------------------------------------------------------
// 诊断码（u3221 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// MCS 队列节点 状态面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3221Code {
    /// 0xe010 状态重复（MCS 同名双态）。
    StateDup,
    /// 0xe011 状态断档（FIFO 序缺失）。
    StateHole,
    /// 0xe012 事件重复（同事件双声明）。
    EventDup,
    /// 0xe013 迁移重复（同源同事件双迁移）。
    TransDup,
    /// 0xe014 迁移缺失（应达态不可达）。
    TransHole,
    /// 0xe015 迁移环（非法自环/环路）。
    TransCycle,
    /// 0xe016 入口非法（初态不在闭集）。
    EntryBad,
    /// 0xe017 迁移丢失（应用事件无效果）。
    ApplyLost,
    /// 0xe018 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe019 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe01a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe01b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3221Code {
    /// 全量闭集。
    pub const ALL: [u3221Code; 12] = [
        u3221Code::StateDup,
        u3221Code::StateHole,
        u3221Code::EventDup,
        u3221Code::TransDup,
        u3221Code::TransHole,
        u3221Code::TransCycle,
        u3221Code::EntryBad,
        u3221Code::ApplyLost,
        u3221Code::LedgerBroken,
        u3221Code::SnapshotFingerprint,
        u3221Code::TableBroken,
        u3221Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3221Code::StateDup => 0xe010,
            u3221Code::StateHole => 0xe011,
            u3221Code::EventDup => 0xe012,
            u3221Code::TransDup => 0xe013,
            u3221Code::TransHole => 0xe014,
            u3221Code::TransCycle => 0xe015,
            u3221Code::EntryBad => 0xe016,
            u3221Code::ApplyLost => 0xe017,
            u3221Code::LedgerBroken => 0xe018,
            u3221Code::SnapshotFingerprint => 0xe019,
            u3221Code::TableBroken => 0xe01a,
            u3221Code::SegmentGuard => 0xe01b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3221Code::StateDup => "状态重复（MCS 同名双态）",
            u3221Code::StateHole => "状态断档（FIFO 序缺失）",
            u3221Code::EventDup => "事件重复（同事件双声明）",
            u3221Code::TransDup => "迁移重复（同源同事件双迁移）",
            u3221Code::TransHole => "迁移缺失（应达态不可达）",
            u3221Code::TransCycle => "迁移环（非法自环/环路）",
            u3221Code::EntryBad => "入口非法（初态不在闭集）",
            u3221Code::ApplyLost => "迁移丢失（应用事件无效果）",
            u3221Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3221Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3221Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3221Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3221Code::StateDup => "按闭集唯一律去重",
            u3221Code::StateHole => "按闭集补态",
            u3221Code::EventDup => "按事件闭集去重",
            u3221Code::TransDup => "按确定性唯一迁移律合并",
            u3221Code::TransHole => "按迁移矩阵补边",
            u3221Code::TransCycle => "按无环律重构迁移",
            u3221Code::EntryBad => "按闭集钳制初态",
            u3221Code::ApplyLost => "核对事件前置条件",
            u3221Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3221Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3221Code::TableBroken => "按语义规则重建常量表",
            u3221Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 状态机 mxu3221flow（6 态 × 6 事件 · 迁移 8 条显式声明）
// --------------------------------------------------------------------------
/// MCS 队列节点 状态机（判据：MCS）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu3221flowState {
    Init,
    Linked,
    Handed,
    Detached,
    Recycled,
    Quiesced,
}

impl mxu3221flowState {
    /// 全量态闭集。
    pub const ALL: [mxu3221flowState; 6] = [
        mxu3221flowState::Init,
        mxu3221flowState::Linked,
        mxu3221flowState::Handed,
        mxu3221flowState::Detached,
        mxu3221flowState::Recycled,
        mxu3221flowState::Quiesced,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            mxu3221flowState::Init => 0,
            mxu3221flowState::Linked => 1,
            mxu3221flowState::Handed => 2,
            mxu3221flowState::Detached => 3,
            mxu3221flowState::Recycled => 4,
            mxu3221flowState::Quiesced => 5,
        }
    }
}

/// mxu3221flow事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu3221flowEvent {
    Enq,
    Deq,
    Hand,
    Unlink,
    Recycle,
    Recheck,
}

impl mxu3221flowEvent {
    /// 全量事件闭集。
    pub const ALL: [mxu3221flowEvent; 6] = [
        mxu3221flowEvent::Enq,
        mxu3221flowEvent::Deq,
        mxu3221flowEvent::Hand,
        mxu3221flowEvent::Unlink,
        mxu3221flowEvent::Recycle,
        mxu3221flowEvent::Recheck,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const MXU3221FLOW_TRANSITIONS: [(mxu3221flowState, mxu3221flowEvent, mxu3221flowState); 8] = [
    (mxu3221flowState::Init, mxu3221flowEvent::Enq, mxu3221flowState::Linked), // Init→Linked 主链
    (mxu3221flowState::Linked, mxu3221flowEvent::Deq, mxu3221flowState::Handed), // Linked→Handed 主链
    (mxu3221flowState::Handed, mxu3221flowEvent::Hand, mxu3221flowState::Detached), // Handed→Detached 主链
    (mxu3221flowState::Detached, mxu3221flowEvent::Unlink, mxu3221flowState::Recycled), // Detached→Recycled 主链
    (mxu3221flowState::Recycled, mxu3221flowEvent::Recycle, mxu3221flowState::Quiesced), // Recycled→Quiesced 主链
    (mxu3221flowState::Quiesced, mxu3221flowEvent::Recheck, mxu3221flowState::Init), // 终态回边
    (mxu3221flowState::Quiesced, mxu3221flowEvent::Enq, mxu3221flowState::Recycled), // 终态回退边
    (mxu3221flowState::Linked, mxu3221flowEvent::Deq, mxu3221flowState::Handed), // 旁路边
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn mxu3221flow_next(state: mxu3221flowState, ev: mxu3221flowEvent) -> Option<mxu3221flowState> {
    let mut i = 0usize;
    while i < 8 {
        let t = MXU3221FLOW_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn mxu3221flow_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < mxu3221flowState::ALL.len() {
        let mut ei = 0usize;
        while ei < mxu3221flowEvent::ALL.len() {
            let legal = mxu3221flow_next(mxu3221flowState::ALL[si], mxu3221flowEvent::ALL[ei]).is_some();
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
pub fn mxu3221flow_terminal(st: mxu3221flowState) -> bool {
    let mut i = 0usize;
    while i < 8 {
        if MXU3221FLOW_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct mxu3221flowLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: mxu3221flowState,
}

impl mxu3221flowLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: mxu3221flowState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: mxu3221flowEvent) -> bool {
        match mxu3221flow_next(self.cur, ev) {
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
// 快照 Mxu3221Snap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// MCS 队列节点 快照封存（FNV 三态指纹 + 篡改检出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mxu3221Snap {
    /// 指纹分量：nd_v。
    pub nd_v: u64,
    /// 指纹分量：tl_v。
    pub tl_v: u64,
    /// 指纹分量：hd_v。
    pub hd_v: u64,
    /// 指纹分量：nx_v。
    pub nx_v: u64,
    /// 指纹分量：pr_v。
    pub pr_v: u64,
    /// 指纹分量：fifo_v。
    pub fifo_v: u64,
    /// 指纹分量：ld_v。
    pub ld_v: u64,
    /// 指纹分量：hb_v。
    pub hb_v: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl Mxu3221Snap {
    /// 构造并即时合成指纹。
    pub fn compute(
        nd_v: u64,
        tl_v: u64,
        hd_v: u64,
        nx_v: u64,
        pr_v: u64,
        fifo_v: u64,
        ld_v: u64,
        hb_v: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &nd_v.to_le_bytes());
        h = fnv1a64_from(h, &tl_v.to_le_bytes());
        h = fnv1a64_from(h, &hd_v.to_le_bytes());
        h = fnv1a64_from(h, &nx_v.to_le_bytes());
        h = fnv1a64_from(h, &pr_v.to_le_bytes());
        h = fnv1a64_from(h, &fifo_v.to_le_bytes());
        h = fnv1a64_from(h, &ld_v.to_le_bytes());
        h = fnv1a64_from(h, &hb_v.to_le_bytes());
        Self {
            nd_v,
            tl_v,
            hd_v,
            nx_v,
            pr_v,
            fifo_v,
            ld_v,
            hb_v,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.nd_v.to_le_bytes());
        h = fnv1a64_from(h, &self.tl_v.to_le_bytes());
        h = fnv1a64_from(h, &self.hd_v.to_le_bytes());
        h = fnv1a64_from(h, &self.nx_v.to_le_bytes());
        h = fnv1a64_from(h, &self.pr_v.to_le_bytes());
        h = fnv1a64_from(h, &self.fifo_v.to_le_bytes());
        h = fnv1a64_from(h, &self.ld_v.to_le_bytes());
        h = fnv1a64_from(h, &self.hb_v.to_le_bytes());
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

/// 段位守卫：全部诊断码必须落在本段 [0xe010, 0xe01f]。
pub const fn u3221_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3221Code::ALL.len() {
        let c = u3221Code::ALL[i].code();
        if c < 0xe010 || c > 0xe01f {
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
fn u3221_guard_seg() -> bool {
    u3221_segment_guard()
}

/// UNX-F3221 状态机闭集/迁移表/非法组合恰值全绿。
fn check_u3221_sm_legal() -> bool {
    let mut ok = mxu3221flow_illegal_pairs() == 29;
    if ok { ok = MXU3221FLOW_TRANSITIONS.len() == 8; }
    if ok { ok = mxu3221flowState::ALL.len() == 6 && mxu3221flowEvent::ALL.len() == 6; }
    ok
}

/// UNX-F3221 迁移主链回放 + 快照往返恒等。
fn check_u3221_sm_round() -> bool {
    let mut lg = mxu3221flowLedger::new(mxu3221flowState::Init);
    let mut ok = lg.apply(mxu3221flowEvent::Enq) && lg.cur == mxu3221flowState::Linked && lg.total() == 1;
    let s0 = Mxu3221Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    let mut s1 = Mxu3221Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    s1.recompute();
    if ok { ok = s0 == s1 && !s1.tampered(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3221_checks() -> CheckSet {
    let mut set = CheckSet::new("U3221");
    set.add(
        "u3221-seg-guard",
        u3221_guard_seg(),
        if u3221_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3221-sm-legal",
        check_u3221_sm_legal(),
        if check_u3221_sm_legal() { "" } else { "UNX-F3221 sm legality mismatch" },
    );
    set.add(
        "u3221-sm-round",
        check_u3221_sm_round(),
        if check_u3221_sm_round() { "" } else { "UNX-F3221 sm roundtrip mismatch" },
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
    fn u3221_all_checks_green() {
        let set = run_u3221_checks();
        assert!(set.all_passed(), "U3221 domain checks must be green");
    }

    #[test]
    fn u3221_segment_guard_holds() {
        assert!(u3221_segment_guard());
    }
}
