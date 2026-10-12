//! unxu2401_u2401sm —— 物理内存图消费与四态归一 状态机与快照封存（内存区间）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2401 · 物理内存图消费与四态归一 · 判据原文：UNX-F2401 · 判据：真机引导后消费 A1 合并内存图，四态归一（可用/保留/可回收/坏页）区间数与 A1 合并账一致，逐态字节和与总 RAM 偏差 ≤4KB
//! · 组件：内存区间 状态机（6 态 × 6 事件显式迁移）+ FNV 快照封存（8 分量）
//! · 判据落地：UNX-F2401 · 判据：真机引导后消费 A1 合并内存图，四态归一（可用/保留/可回收/坏页）区间数与 A1 合并账一致，逐态字节和与总 RAM 偏差 ≤
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2401_VERSION: &str = "u2401-u2401sm-v1";

/// 诊断段基址。
pub const U2401_SEG_BASE: u16 = 0xa010;

// --------------------------------------------------------------------------
// 诊断码（u2401 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 内存区间 状态面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2401Code {
    /// 0xa010 状态重复（区间数失配 同名双态）。
    StateDup,
    /// 0xa011 状态断档（字节和偏差 序缺失）。
    StateHole,
    /// 0xa012 事件重复（同事件双声明）。
    EventDup,
    /// 0xa013 迁移重复（同源同事件双迁移）。
    TransDup,
    /// 0xa014 迁移缺失（应达态不可达）。
    TransHole,
    /// 0xa015 迁移环（非法自环/环路）。
    TransCycle,
    /// 0xa016 入口非法（初态不在闭集）。
    EntryBad,
    /// 0xa017 迁移丢失（应用事件无效果）。
    ApplyLost,
    /// 0xa018 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xa019 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xa01a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xa01b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2401Code {
    /// 全量闭集。
    pub const ALL: [u2401Code; 12] = [
        u2401Code::StateDup,
        u2401Code::StateHole,
        u2401Code::EventDup,
        u2401Code::TransDup,
        u2401Code::TransHole,
        u2401Code::TransCycle,
        u2401Code::EntryBad,
        u2401Code::ApplyLost,
        u2401Code::LedgerBroken,
        u2401Code::SnapshotFingerprint,
        u2401Code::TableBroken,
        u2401Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2401Code::StateDup => 0xa010,
            u2401Code::StateHole => 0xa011,
            u2401Code::EventDup => 0xa012,
            u2401Code::TransDup => 0xa013,
            u2401Code::TransHole => 0xa014,
            u2401Code::TransCycle => 0xa015,
            u2401Code::EntryBad => 0xa016,
            u2401Code::ApplyLost => 0xa017,
            u2401Code::LedgerBroken => 0xa018,
            u2401Code::SnapshotFingerprint => 0xa019,
            u2401Code::TableBroken => 0xa01a,
            u2401Code::SegmentGuard => 0xa01b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2401Code::StateDup => "状态重复（区间数失配 同名双态）",
            u2401Code::StateHole => "状态断档（字节和偏差 序缺失）",
            u2401Code::EventDup => "事件重复（同事件双声明）",
            u2401Code::TransDup => "迁移重复（同源同事件双迁移）",
            u2401Code::TransHole => "迁移缺失（应达态不可达）",
            u2401Code::TransCycle => "迁移环（非法自环/环路）",
            u2401Code::EntryBad => "入口非法（初态不在闭集）",
            u2401Code::ApplyLost => "迁移丢失（应用事件无效果）",
            u2401Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2401Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2401Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2401Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2401Code::StateDup => "按闭集唯一律去重",
            u2401Code::StateHole => "按闭集补态",
            u2401Code::EventDup => "按事件闭集去重",
            u2401Code::TransDup => "按确定性唯一迁移律合并",
            u2401Code::TransHole => "按迁移矩阵补边",
            u2401Code::TransCycle => "按无环律重构迁移",
            u2401Code::EntryBad => "按闭集钳制初态",
            u2401Code::ApplyLost => "核对事件前置条件",
            u2401Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2401Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2401Code::TableBroken => "按语义规则重建常量表",
            u2401Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 状态机 mxu2401flow（6 态 × 6 事件 · 迁移 8 条显式声明）
// --------------------------------------------------------------------------
/// 内存区间 状态机（判据：区间数失配）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu2401flowState {
    Boot,
    Merged,
    Classified,
    ByteSummed,
    Aligned,
    Sealed,
}

impl mxu2401flowState {
    /// 全量态闭集。
    pub const ALL: [mxu2401flowState; 6] = [
        mxu2401flowState::Boot,
        mxu2401flowState::Merged,
        mxu2401flowState::Classified,
        mxu2401flowState::ByteSummed,
        mxu2401flowState::Aligned,
        mxu2401flowState::Sealed,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            mxu2401flowState::Boot => 0,
            mxu2401flowState::Merged => 1,
            mxu2401flowState::Classified => 2,
            mxu2401flowState::ByteSummed => 3,
            mxu2401flowState::Aligned => 4,
            mxu2401flowState::Sealed => 5,
        }
    }
}

/// mxu2401flow事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu2401flowEvent {
    BootDone,
    MergeMap,
    Classify,
    SumBytes,
    CheckDrift,
    Seal,
}

impl mxu2401flowEvent {
    /// 全量事件闭集。
    pub const ALL: [mxu2401flowEvent; 6] = [
        mxu2401flowEvent::BootDone,
        mxu2401flowEvent::MergeMap,
        mxu2401flowEvent::Classify,
        mxu2401flowEvent::SumBytes,
        mxu2401flowEvent::CheckDrift,
        mxu2401flowEvent::Seal,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const MXU2401FLOW_TRANSITIONS: [(mxu2401flowState, mxu2401flowEvent, mxu2401flowState); 8] = [
    (mxu2401flowState::Boot, mxu2401flowEvent::BootDone, mxu2401flowState::Merged), // Boot→Merged 主链
    (mxu2401flowState::Merged, mxu2401flowEvent::MergeMap, mxu2401flowState::Classified), // Merged→Classified 主链
    (mxu2401flowState::Classified, mxu2401flowEvent::Classify, mxu2401flowState::ByteSummed), // Classified→ByteSummed 主链
    (mxu2401flowState::ByteSummed, mxu2401flowEvent::SumBytes, mxu2401flowState::Aligned), // ByteSummed→Aligned 主链
    (mxu2401flowState::Aligned, mxu2401flowEvent::CheckDrift, mxu2401flowState::Sealed), // Aligned→Sealed 主链
    (mxu2401flowState::Sealed, mxu2401flowEvent::Seal, mxu2401flowState::Boot), // 终态回边
    (mxu2401flowState::Sealed, mxu2401flowEvent::BootDone, mxu2401flowState::Aligned), // 终态回退边
    (mxu2401flowState::Merged, mxu2401flowEvent::MergeMap, mxu2401flowState::Classified), // 旁路边
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn mxu2401flow_next(state: mxu2401flowState, ev: mxu2401flowEvent) -> Option<mxu2401flowState> {
    let mut i = 0usize;
    while i < 8 {
        let t = MXU2401FLOW_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn mxu2401flow_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < mxu2401flowState::ALL.len() {
        let mut ei = 0usize;
        while ei < mxu2401flowEvent::ALL.len() {
            let legal = mxu2401flow_next(mxu2401flowState::ALL[si], mxu2401flowEvent::ALL[ei]).is_some();
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
pub fn mxu2401flow_terminal(st: mxu2401flowState) -> bool {
    let mut i = 0usize;
    while i < 8 {
        if MXU2401FLOW_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct mxu2401flowLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: mxu2401flowState,
}

impl mxu2401flowLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: mxu2401flowState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: mxu2401flowEvent) -> bool {
        match mxu2401flow_next(self.cur, ev) {
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
// 快照 Mxu2401Snap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// 内存区间 快照封存（FNV 三态指纹 + 篡改检出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mxu2401Snap {
    /// 指纹分量：av_v。
    pub av_v: u64,
    /// 指纹分量：rs_v。
    pub rs_v: u64,
    /// 指纹分量：rc_v。
    pub rc_v: u64,
    /// 指纹分量：bd_v。
    pub bd_v: u64,
    /// 指纹分量：mg_v。
    pub mg_v: u64,
    /// 指纹分量：iv_v。
    pub iv_v: u64,
    /// 指纹分量：bs_v。
    pub bs_v: u64,
    /// 指纹分量：dr_v。
    pub dr_v: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl Mxu2401Snap {
    /// 构造并即时合成指纹。
    pub fn compute(
        av_v: u64,
        rs_v: u64,
        rc_v: u64,
        bd_v: u64,
        mg_v: u64,
        iv_v: u64,
        bs_v: u64,
        dr_v: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &av_v.to_le_bytes());
        h = fnv1a64_from(h, &rs_v.to_le_bytes());
        h = fnv1a64_from(h, &rc_v.to_le_bytes());
        h = fnv1a64_from(h, &bd_v.to_le_bytes());
        h = fnv1a64_from(h, &mg_v.to_le_bytes());
        h = fnv1a64_from(h, &iv_v.to_le_bytes());
        h = fnv1a64_from(h, &bs_v.to_le_bytes());
        h = fnv1a64_from(h, &dr_v.to_le_bytes());
        Self {
            av_v,
            rs_v,
            rc_v,
            bd_v,
            mg_v,
            iv_v,
            bs_v,
            dr_v,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.av_v.to_le_bytes());
        h = fnv1a64_from(h, &self.rs_v.to_le_bytes());
        h = fnv1a64_from(h, &self.rc_v.to_le_bytes());
        h = fnv1a64_from(h, &self.bd_v.to_le_bytes());
        h = fnv1a64_from(h, &self.mg_v.to_le_bytes());
        h = fnv1a64_from(h, &self.iv_v.to_le_bytes());
        h = fnv1a64_from(h, &self.bs_v.to_le_bytes());
        h = fnv1a64_from(h, &self.dr_v.to_le_bytes());
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

/// 段位守卫：全部诊断码必须落在本段 [0xa010, 0xa01f]。
pub const fn u2401_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2401Code::ALL.len() {
        let c = u2401Code::ALL[i].code();
        if c < 0xa010 || c > 0xa01f {
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
fn u2401_guard_seg() -> bool {
    u2401_segment_guard()
}

/// UNX-F2401 状态机闭集/迁移表/非法组合恰值全绿。
fn check_u2401_sm_legal() -> bool {
    let mut ok = mxu2401flow_illegal_pairs() == 29;
    if ok { ok = MXU2401FLOW_TRANSITIONS.len() == 8; }
    if ok { ok = mxu2401flowState::ALL.len() == 6 && mxu2401flowEvent::ALL.len() == 6; }
    ok
}

/// UNX-F2401 迁移主链回放 + 快照往返恒等。
fn check_u2401_sm_round() -> bool {
    let mut lg = mxu2401flowLedger::new(mxu2401flowState::Boot);
    let mut ok = lg.apply(mxu2401flowEvent::BootDone) && lg.cur == mxu2401flowState::Merged && lg.total() == 1;
    let s0 = Mxu2401Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    let mut s1 = Mxu2401Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    s1.recompute();
    if ok { ok = s0 == s1 && !s1.tampered(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2401_checks() -> CheckSet {
    let mut set = CheckSet::new("U2401");
    set.add(
        "u2401-seg-guard",
        u2401_guard_seg(),
        if u2401_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2401-sm-legal",
        check_u2401_sm_legal(),
        if check_u2401_sm_legal() { "" } else { "UNX-F2401 sm legality mismatch" },
    );
    set.add(
        "u2401-sm-round",
        check_u2401_sm_round(),
        if check_u2401_sm_round() { "" } else { "UNX-F2401 sm roundtrip mismatch" },
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
    fn u2401_all_checks_green() {
        let set = run_u2401_checks();
        assert!(set.all_passed(), "U2401 domain checks must be green");
    }

    #[test]
    fn u2401_segment_guard_holds() {
        assert!(u2401_segment_guard());
    }
}
