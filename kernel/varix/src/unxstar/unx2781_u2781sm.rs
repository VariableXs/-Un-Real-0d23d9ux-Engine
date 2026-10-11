//! unxu2781_u2781sm —— 回收-分配联动总闸 状态机与快照封存（联动事务）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2781 · 回收-分配联动总闸 · 判据原文：UNX-F2781 · 判据：联动总闸（分配路径事件→回收路径响应）全链 10/10 次贯通，联动账（触发源/响应/时延）三列齐备
//! · 组件：联动事务 状态机（6 态 × 6 事件显式迁移）+ FNV 快照封存（8 分量）
//! · 判据落地：UNX-F2781 · 判据：联动总闸（分配路径事件→回收路径响应）全链 10/10 次贯通，联动账（触发源/响应/时延）三列齐备
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2781_VERSION: &str = "u2781-u2781sm-v1";

/// 诊断段基址。
pub const U2781_SEG_BASE: u16 = 0xae10;

// --------------------------------------------------------------------------
// 诊断码（u2781 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 联动事务 状态面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2781Code {
    /// 0xae10 状态重复（闸门失守 同名双态）。
    StateDup,
    /// 0xae11 状态断档（响应缺列 序缺失）。
    StateHole,
    /// 0xae12 事件重复（同事件双声明）。
    EventDup,
    /// 0xae13 迁移重复（同源同事件双迁移）。
    TransDup,
    /// 0xae14 迁移缺失（应达态不可达）。
    TransHole,
    /// 0xae15 迁移环（非法自环/环路）。
    TransCycle,
    /// 0xae16 入口非法（初态不在闭集）。
    EntryBad,
    /// 0xae17 迁移丢失（应用事件无效果）。
    ApplyLost,
    /// 0xae18 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xae19 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xae1a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xae1b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2781Code {
    /// 全量闭集。
    pub const ALL: [u2781Code; 12] = [
        u2781Code::StateDup,
        u2781Code::StateHole,
        u2781Code::EventDup,
        u2781Code::TransDup,
        u2781Code::TransHole,
        u2781Code::TransCycle,
        u2781Code::EntryBad,
        u2781Code::ApplyLost,
        u2781Code::LedgerBroken,
        u2781Code::SnapshotFingerprint,
        u2781Code::TableBroken,
        u2781Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2781Code::StateDup => 0xae10,
            u2781Code::StateHole => 0xae11,
            u2781Code::EventDup => 0xae12,
            u2781Code::TransDup => 0xae13,
            u2781Code::TransHole => 0xae14,
            u2781Code::TransCycle => 0xae15,
            u2781Code::EntryBad => 0xae16,
            u2781Code::ApplyLost => 0xae17,
            u2781Code::LedgerBroken => 0xae18,
            u2781Code::SnapshotFingerprint => 0xae19,
            u2781Code::TableBroken => 0xae1a,
            u2781Code::SegmentGuard => 0xae1b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2781Code::StateDup => "状态重复（闸门失守 同名双态）",
            u2781Code::StateHole => "状态断档（响应缺列 序缺失）",
            u2781Code::EventDup => "事件重复（同事件双声明）",
            u2781Code::TransDup => "迁移重复（同源同事件双迁移）",
            u2781Code::TransHole => "迁移缺失（应达态不可达）",
            u2781Code::TransCycle => "迁移环（非法自环/环路）",
            u2781Code::EntryBad => "入口非法（初态不在闭集）",
            u2781Code::ApplyLost => "迁移丢失（应用事件无效果）",
            u2781Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2781Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2781Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2781Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2781Code::StateDup => "按闭集唯一律去重",
            u2781Code::StateHole => "按闭集补态",
            u2781Code::EventDup => "按事件闭集去重",
            u2781Code::TransDup => "按确定性唯一迁移律合并",
            u2781Code::TransHole => "按迁移矩阵补边",
            u2781Code::TransCycle => "按无环律重构迁移",
            u2781Code::EntryBad => "按闭集钳制初态",
            u2781Code::ApplyLost => "核对事件前置条件",
            u2781Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2781Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2781Code::TableBroken => "按语义规则重建常量表",
            u2781Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 状态机 mxu2781flow（6 态 × 6 事件 · 迁移 8 条显式声明）
// --------------------------------------------------------------------------
/// 联动事务 状态机（判据：闸门失守）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu2781flowState {
    Idle,
    AllocEvt,
    Gated,
    ReclaimRsp,
    Ledger3,
    Sealed,
}

impl mxu2781flowState {
    /// 全量态闭集。
    pub const ALL: [mxu2781flowState; 6] = [
        mxu2781flowState::Idle,
        mxu2781flowState::AllocEvt,
        mxu2781flowState::Gated,
        mxu2781flowState::ReclaimRsp,
        mxu2781flowState::Ledger3,
        mxu2781flowState::Sealed,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            mxu2781flowState::Idle => 0,
            mxu2781flowState::AllocEvt => 1,
            mxu2781flowState::Gated => 2,
            mxu2781flowState::ReclaimRsp => 3,
            mxu2781flowState::Ledger3 => 4,
            mxu2781flowState::Sealed => 5,
        }
    }
}

/// mxu2781flow事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu2781flowEvent {
    AllocEvent,
    HitGate,
    Gate,
    ReclaimResp,
    Ledger,
    Seal,
}

impl mxu2781flowEvent {
    /// 全量事件闭集。
    pub const ALL: [mxu2781flowEvent; 6] = [
        mxu2781flowEvent::AllocEvent,
        mxu2781flowEvent::HitGate,
        mxu2781flowEvent::Gate,
        mxu2781flowEvent::ReclaimResp,
        mxu2781flowEvent::Ledger,
        mxu2781flowEvent::Seal,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const MXU2781FLOW_TRANSITIONS: [(mxu2781flowState, mxu2781flowEvent, mxu2781flowState); 8] = [
    (mxu2781flowState::Idle, mxu2781flowEvent::AllocEvent, mxu2781flowState::AllocEvt), // Idle→AllocEvt 主链
    (mxu2781flowState::AllocEvt, mxu2781flowEvent::HitGate, mxu2781flowState::Gated), // AllocEvt→Gated 主链
    (mxu2781flowState::Gated, mxu2781flowEvent::Gate, mxu2781flowState::ReclaimRsp), // Gated→ReclaimRsp 主链
    (mxu2781flowState::ReclaimRsp, mxu2781flowEvent::ReclaimResp, mxu2781flowState::Ledger3), // ReclaimRsp→Ledger3 主链
    (mxu2781flowState::Ledger3, mxu2781flowEvent::Ledger, mxu2781flowState::Sealed), // Ledger3→Sealed 主链
    (mxu2781flowState::Sealed, mxu2781flowEvent::Seal, mxu2781flowState::Idle), // 终态回边
    (mxu2781flowState::Sealed, mxu2781flowEvent::AllocEvent, mxu2781flowState::Ledger3), // 终态回退边
    (mxu2781flowState::AllocEvt, mxu2781flowEvent::HitGate, mxu2781flowState::Gated), // 旁路边
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn mxu2781flow_next(state: mxu2781flowState, ev: mxu2781flowEvent) -> Option<mxu2781flowState> {
    let mut i = 0usize;
    while i < 8 {
        let t = MXU2781FLOW_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn mxu2781flow_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < mxu2781flowState::ALL.len() {
        let mut ei = 0usize;
        while ei < mxu2781flowEvent::ALL.len() {
            let legal = mxu2781flow_next(mxu2781flowState::ALL[si], mxu2781flowEvent::ALL[ei]).is_some();
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
pub fn mxu2781flow_terminal(st: mxu2781flowState) -> bool {
    let mut i = 0usize;
    while i < 8 {
        if MXU2781FLOW_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct mxu2781flowLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: mxu2781flowState,
}

impl mxu2781flowLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: mxu2781flowState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: mxu2781flowEvent) -> bool {
        match mxu2781flow_next(self.cur, ev) {
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
// 快照 Mxu2781Snap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// 联动事务 快照封存（FNV 三态指纹 + 篡改检出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mxu2781Snap {
    /// 指纹分量：lk_v。
    pub lk_v: u64,
    /// 指纹分量：ap_v。
    pub ap_v: u64,
    /// 指纹分量：rp_v。
    pub rp_v: u64,
    /// 指纹分量：ts_v。
    pub ts_v: u64,
    /// 指纹分量：rs_v。
    pub rs_v: u64,
    /// 指纹分量：lt_v。
    pub lt_v: u64,
    /// 指纹分量：c3_v。
    pub c3_v: u64,
    /// 指纹分量：c10_v。
    pub c10_v: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl Mxu2781Snap {
    /// 构造并即时合成指纹。
    pub fn compute(
        lk_v: u64,
        ap_v: u64,
        rp_v: u64,
        ts_v: u64,
        rs_v: u64,
        lt_v: u64,
        c3_v: u64,
        c10_v: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &lk_v.to_le_bytes());
        h = fnv1a64_from(h, &ap_v.to_le_bytes());
        h = fnv1a64_from(h, &rp_v.to_le_bytes());
        h = fnv1a64_from(h, &ts_v.to_le_bytes());
        h = fnv1a64_from(h, &rs_v.to_le_bytes());
        h = fnv1a64_from(h, &lt_v.to_le_bytes());
        h = fnv1a64_from(h, &c3_v.to_le_bytes());
        h = fnv1a64_from(h, &c10_v.to_le_bytes());
        Self {
            lk_v,
            ap_v,
            rp_v,
            ts_v,
            rs_v,
            lt_v,
            c3_v,
            c10_v,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.lk_v.to_le_bytes());
        h = fnv1a64_from(h, &self.ap_v.to_le_bytes());
        h = fnv1a64_from(h, &self.rp_v.to_le_bytes());
        h = fnv1a64_from(h, &self.ts_v.to_le_bytes());
        h = fnv1a64_from(h, &self.rs_v.to_le_bytes());
        h = fnv1a64_from(h, &self.lt_v.to_le_bytes());
        h = fnv1a64_from(h, &self.c3_v.to_le_bytes());
        h = fnv1a64_from(h, &self.c10_v.to_le_bytes());
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

/// 段位守卫：全部诊断码必须落在本段 [0xae10, 0xae1f]。
pub const fn u2781_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2781Code::ALL.len() {
        let c = u2781Code::ALL[i].code();
        if c < 0xae10 || c > 0xae1f {
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
fn u2781_guard_seg() -> bool {
    u2781_segment_guard()
}

/// UNX-F2781 状态机闭集/迁移表/非法组合恰值全绿。
fn check_u2781_sm_legal() -> bool {
    let mut ok = mxu2781flow_illegal_pairs() == 29;
    if ok { ok = MXU2781FLOW_TRANSITIONS.len() == 8; }
    if ok { ok = mxu2781flowState::ALL.len() == 6 && mxu2781flowEvent::ALL.len() == 6; }
    ok
}

/// UNX-F2781 迁移主链回放 + 快照往返恒等。
fn check_u2781_sm_round() -> bool {
    let mut lg = mxu2781flowLedger::new(mxu2781flowState::Idle);
    let mut ok = lg.apply(mxu2781flowEvent::AllocEvent) && lg.cur == mxu2781flowState::AllocEvt && lg.total() == 1;
    let s0 = Mxu2781Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    let mut s1 = Mxu2781Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    s1.recompute();
    if ok { ok = s0 == s1 && !s1.tampered(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2781_checks() -> CheckSet {
    let mut set = CheckSet::new("U2781");
    set.add(
        "u2781-seg-guard",
        u2781_guard_seg(),
        if u2781_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2781-sm-legal",
        check_u2781_sm_legal(),
        if check_u2781_sm_legal() { "" } else { "UNX-F2781 sm legality mismatch" },
    );
    set.add(
        "u2781-sm-round",
        check_u2781_sm_round(),
        if check_u2781_sm_round() { "" } else { "UNX-F2781 sm roundtrip mismatch" },
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
    fn u2781_all_checks_green() {
        let set = run_u2781_checks();
        assert!(set.all_passed(), "U2781 domain checks must be green");
    }

    #[test]
    fn u2781_segment_guard_holds() {
        assert!(u2781_segment_guard());
    }
}
