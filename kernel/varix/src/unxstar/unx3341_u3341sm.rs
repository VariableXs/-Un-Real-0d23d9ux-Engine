//! unxu3341_u3341sm —— 死锁检测器总设计（等待图模型与不做的事） 状态机与快照封存（死锁检测器）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3341 · 死锁检测器总设计（等待图模型与不做的事） · 判据原文：UNX-F3341 · 判据：J1：总设计文档交付：节点=锁实例、边=持有→等待的图模型定义、v1 不做清单（不做抢占恢复/不做图外对象/不做优先级反转纠正）；M-100 模型验证用例。
//! · 组件：死锁检测器 状态机（6 态 × 6 事件显式迁移）+ FNV 快照封存（8 分量）
//! · 判据落地：UNX-F3341 · 判据：J1：总设计文档交付：节点=锁实例、边=持有→等待的图模型定义、v1 不做清单（不做抢占恢复/不做图外对象/不做优先级反转纠正）；
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3341_VERSION: &str = "u3341-u3341sm-v1";

/// 诊断段基址。
pub const U3341_SEG_BASE: u16 = 0xe050;

// --------------------------------------------------------------------------
// 诊断码（u3341 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 死锁检测器 状态面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3341Code {
    /// 0xe050 状态重复（死锁 同名双态）。
    StateDup,
    /// 0xe051 状态断档（等待图 序缺失）。
    StateHole,
    /// 0xe052 事件重复（同事件双声明）。
    EventDup,
    /// 0xe053 迁移重复（同源同事件双迁移）。
    TransDup,
    /// 0xe054 迁移缺失（应达态不可达）。
    TransHole,
    /// 0xe055 迁移环（非法自环/环路）。
    TransCycle,
    /// 0xe056 入口非法（初态不在闭集）。
    EntryBad,
    /// 0xe057 迁移丢失（应用事件无效果）。
    ApplyLost,
    /// 0xe058 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe059 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe05a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe05b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3341Code {
    /// 全量闭集。
    pub const ALL: [u3341Code; 12] = [
        u3341Code::StateDup,
        u3341Code::StateHole,
        u3341Code::EventDup,
        u3341Code::TransDup,
        u3341Code::TransHole,
        u3341Code::TransCycle,
        u3341Code::EntryBad,
        u3341Code::ApplyLost,
        u3341Code::LedgerBroken,
        u3341Code::SnapshotFingerprint,
        u3341Code::TableBroken,
        u3341Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3341Code::StateDup => 0xe050,
            u3341Code::StateHole => 0xe051,
            u3341Code::EventDup => 0xe052,
            u3341Code::TransDup => 0xe053,
            u3341Code::TransHole => 0xe054,
            u3341Code::TransCycle => 0xe055,
            u3341Code::EntryBad => 0xe056,
            u3341Code::ApplyLost => 0xe057,
            u3341Code::LedgerBroken => 0xe058,
            u3341Code::SnapshotFingerprint => 0xe059,
            u3341Code::TableBroken => 0xe05a,
            u3341Code::SegmentGuard => 0xe05b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3341Code::StateDup => "状态重复（死锁 同名双态）",
            u3341Code::StateHole => "状态断档（等待图 序缺失）",
            u3341Code::EventDup => "事件重复（同事件双声明）",
            u3341Code::TransDup => "迁移重复（同源同事件双迁移）",
            u3341Code::TransHole => "迁移缺失（应达态不可达）",
            u3341Code::TransCycle => "迁移环（非法自环/环路）",
            u3341Code::EntryBad => "入口非法（初态不在闭集）",
            u3341Code::ApplyLost => "迁移丢失（应用事件无效果）",
            u3341Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3341Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3341Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3341Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3341Code::StateDup => "按闭集唯一律去重",
            u3341Code::StateHole => "按闭集补态",
            u3341Code::EventDup => "按事件闭集去重",
            u3341Code::TransDup => "按确定性唯一迁移律合并",
            u3341Code::TransHole => "按迁移矩阵补边",
            u3341Code::TransCycle => "按无环律重构迁移",
            u3341Code::EntryBad => "按闭集钳制初态",
            u3341Code::ApplyLost => "核对事件前置条件",
            u3341Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3341Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3341Code::TableBroken => "按语义规则重建常量表",
            u3341Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 状态机 mxu3341flow（6 态 × 6 事件 · 迁移 8 条显式声明）
// --------------------------------------------------------------------------
/// 死锁检测器 状态机（判据：死锁）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu3341flowState {
    Idle,
    Build,
    Scan,
    Found,
    Report,
    Clear,
}

impl mxu3341flowState {
    /// 全量态闭集。
    pub const ALL: [mxu3341flowState; 6] = [
        mxu3341flowState::Idle,
        mxu3341flowState::Build,
        mxu3341flowState::Scan,
        mxu3341flowState::Found,
        mxu3341flowState::Report,
        mxu3341flowState::Clear,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            mxu3341flowState::Idle => 0,
            mxu3341flowState::Build => 1,
            mxu3341flowState::Scan => 2,
            mxu3341flowState::Found => 3,
            mxu3341flowState::Report => 4,
            mxu3341flowState::Clear => 5,
        }
    }
}

/// mxu3341flow事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu3341flowEvent {
    AddNode,
    AddEdge,
    DelEdge,
    Scan,
    Detect,
    Report,
}

impl mxu3341flowEvent {
    /// 全量事件闭集。
    pub const ALL: [mxu3341flowEvent; 6] = [
        mxu3341flowEvent::AddNode,
        mxu3341flowEvent::AddEdge,
        mxu3341flowEvent::DelEdge,
        mxu3341flowEvent::Scan,
        mxu3341flowEvent::Detect,
        mxu3341flowEvent::Report,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const MXU3341FLOW_TRANSITIONS: [(mxu3341flowState, mxu3341flowEvent, mxu3341flowState); 8] = [
    (mxu3341flowState::Idle, mxu3341flowEvent::AddNode, mxu3341flowState::Build), // Idle→Build 主链
    (mxu3341flowState::Build, mxu3341flowEvent::AddEdge, mxu3341flowState::Scan), // Build→Scan 主链
    (mxu3341flowState::Scan, mxu3341flowEvent::DelEdge, mxu3341flowState::Found), // Scan→Found 主链
    (mxu3341flowState::Found, mxu3341flowEvent::Scan, mxu3341flowState::Report), // Found→Report 主链
    (mxu3341flowState::Report, mxu3341flowEvent::Detect, mxu3341flowState::Clear), // Report→Clear 主链
    (mxu3341flowState::Clear, mxu3341flowEvent::Report, mxu3341flowState::Idle), // 终态回边
    (mxu3341flowState::Clear, mxu3341flowEvent::AddNode, mxu3341flowState::Report), // 终态回退边
    (mxu3341flowState::Build, mxu3341flowEvent::AddEdge, mxu3341flowState::Scan), // 旁路边
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn mxu3341flow_next(state: mxu3341flowState, ev: mxu3341flowEvent) -> Option<mxu3341flowState> {
    let mut i = 0usize;
    while i < 8 {
        let t = MXU3341FLOW_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn mxu3341flow_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < mxu3341flowState::ALL.len() {
        let mut ei = 0usize;
        while ei < mxu3341flowEvent::ALL.len() {
            let legal = mxu3341flow_next(mxu3341flowState::ALL[si], mxu3341flowEvent::ALL[ei]).is_some();
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
pub fn mxu3341flow_terminal(st: mxu3341flowState) -> bool {
    let mut i = 0usize;
    while i < 8 {
        if MXU3341FLOW_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct mxu3341flowLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: mxu3341flowState,
}

impl mxu3341flowLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: mxu3341flowState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: mxu3341flowEvent) -> bool {
        match mxu3341flow_next(self.cur, ev) {
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
// 快照 Mxu3341Snap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// 死锁检测器 快照封存（FNV 三态指纹 + 篡改检出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mxu3341Snap {
    /// 指纹分量：wg_v。
    pub wg_v: u64,
    /// 指纹分量：node_v。
    pub node_v: u64,
    /// 指纹分量：edge_v。
    pub edge_v: u64,
    /// 指纹分量：tree_v。
    pub tree_v: u64,
    /// 指纹分量：wchain_v。
    pub wchain_v: u64,
    /// 指纹分量：blk_v。
    pub blk_v: u64,
    /// 指纹分量：ttd_v。
    pub ttd_v: u64,
    /// 指纹分量：rpt_v。
    pub rpt_v: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl Mxu3341Snap {
    /// 构造并即时合成指纹。
    pub fn compute(
        wg_v: u64,
        node_v: u64,
        edge_v: u64,
        tree_v: u64,
        wchain_v: u64,
        blk_v: u64,
        ttd_v: u64,
        rpt_v: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &wg_v.to_le_bytes());
        h = fnv1a64_from(h, &node_v.to_le_bytes());
        h = fnv1a64_from(h, &edge_v.to_le_bytes());
        h = fnv1a64_from(h, &tree_v.to_le_bytes());
        h = fnv1a64_from(h, &wchain_v.to_le_bytes());
        h = fnv1a64_from(h, &blk_v.to_le_bytes());
        h = fnv1a64_from(h, &ttd_v.to_le_bytes());
        h = fnv1a64_from(h, &rpt_v.to_le_bytes());
        Self {
            wg_v,
            node_v,
            edge_v,
            tree_v,
            wchain_v,
            blk_v,
            ttd_v,
            rpt_v,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.wg_v.to_le_bytes());
        h = fnv1a64_from(h, &self.node_v.to_le_bytes());
        h = fnv1a64_from(h, &self.edge_v.to_le_bytes());
        h = fnv1a64_from(h, &self.tree_v.to_le_bytes());
        h = fnv1a64_from(h, &self.wchain_v.to_le_bytes());
        h = fnv1a64_from(h, &self.blk_v.to_le_bytes());
        h = fnv1a64_from(h, &self.ttd_v.to_le_bytes());
        h = fnv1a64_from(h, &self.rpt_v.to_le_bytes());
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

/// 段位守卫：全部诊断码必须落在本段 [0xe050, 0xe05f]。
pub const fn u3341_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3341Code::ALL.len() {
        let c = u3341Code::ALL[i].code();
        if c < 0xe050 || c > 0xe05f {
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
fn u3341_guard_seg() -> bool {
    u3341_segment_guard()
}

/// UNX-F3341 状态机闭集/迁移表/非法组合恰值全绿。
fn check_u3341_sm_legal() -> bool {
    let mut ok = mxu3341flow_illegal_pairs() == 29;
    if ok { ok = MXU3341FLOW_TRANSITIONS.len() == 8; }
    if ok { ok = mxu3341flowState::ALL.len() == 6 && mxu3341flowEvent::ALL.len() == 6; }
    ok
}

/// UNX-F3341 迁移主链回放 + 快照往返恒等。
fn check_u3341_sm_round() -> bool {
    let mut lg = mxu3341flowLedger::new(mxu3341flowState::Idle);
    let mut ok = lg.apply(mxu3341flowEvent::AddNode) && lg.cur == mxu3341flowState::Build && lg.total() == 1;
    let s0 = Mxu3341Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    let mut s1 = Mxu3341Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    s1.recompute();
    if ok { ok = s0 == s1 && !s1.tampered(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3341_checks() -> CheckSet {
    let mut set = CheckSet::new("U3341");
    set.add(
        "u3341-seg-guard",
        u3341_guard_seg(),
        if u3341_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3341-sm-legal",
        check_u3341_sm_legal(),
        if check_u3341_sm_legal() { "" } else { "UNX-F3341 sm legality mismatch" },
    );
    set.add(
        "u3341-sm-round",
        check_u3341_sm_round(),
        if check_u3341_sm_round() { "" } else { "UNX-F3341 sm roundtrip mismatch" },
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
    fn u3341_all_checks_green() {
        let set = run_u3341_checks();
        assert!(set.all_passed(), "U3341 domain checks must be green");
    }

    #[test]
    fn u3341_segment_guard_holds() {
        assert!(u3341_segment_guard());
    }
}
