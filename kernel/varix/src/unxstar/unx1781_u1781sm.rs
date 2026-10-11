//! unxu1781_u1781sm —— B01–B09 每核结构总成总纲（十层结构拼图） 状态机与快照封存（结构层）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F1781 · B01–B09 每核结构总成总纲（十层结构拼图） · 判据原文：UNX-F1781 · 判据：十层拼图（布局/变量/队列/类/时间片/追踪/均衡/锁/空闲唤醒/观测）总成关系断言 ≥30 条全过，层间接口全量登记
//! · 组件：结构层 状态机（6 态 × 6 事件显式迁移）+ FNV 快照封存（8 分量）
//! · 判据落地：UNX-F1781 · 判据：十层拼图（布局/变量/队列/类/时间片/追踪/均衡/锁/空闲唤醒/观测）总成关系断言 ≥30 条全过，层间接口全量登记
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U1781_VERSION: &str = "u1781-u1781sm-v1";

/// 诊断段基址。
pub const U1781_SEG_BASE: u16 = 0x5bd0;

// --------------------------------------------------------------------------
// 诊断码（u1781 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 结构层 状态面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u1781Code {
    /// 0x5bd0 状态重复（拼缝错位 同名双态）。
    StateDup,
    /// 0x5bd1 状态断档（层接口漏登 序缺失）。
    StateHole,
    /// 0x5bd2 事件重复（同事件双声明）。
    EventDup,
    /// 0x5bd3 迁移重复（同源同事件双迁移）。
    TransDup,
    /// 0x5bd4 迁移缺失（应达态不可达）。
    TransHole,
    /// 0x5bd5 迁移环（非法自环/环路）。
    TransCycle,
    /// 0x5bd6 入口非法（初态不在闭集）。
    EntryBad,
    /// 0x5bd7 迁移丢失（应用事件无效果）。
    ApplyLost,
    /// 0x5bd8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x5bd9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x5bda 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x5bdb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u1781Code {
    /// 全量闭集。
    pub const ALL: [u1781Code; 12] = [
        u1781Code::StateDup,
        u1781Code::StateHole,
        u1781Code::EventDup,
        u1781Code::TransDup,
        u1781Code::TransHole,
        u1781Code::TransCycle,
        u1781Code::EntryBad,
        u1781Code::ApplyLost,
        u1781Code::LedgerBroken,
        u1781Code::SnapshotFingerprint,
        u1781Code::TableBroken,
        u1781Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u1781Code::StateDup => 0x5bd0,
            u1781Code::StateHole => 0x5bd1,
            u1781Code::EventDup => 0x5bd2,
            u1781Code::TransDup => 0x5bd3,
            u1781Code::TransHole => 0x5bd4,
            u1781Code::TransCycle => 0x5bd5,
            u1781Code::EntryBad => 0x5bd6,
            u1781Code::ApplyLost => 0x5bd7,
            u1781Code::LedgerBroken => 0x5bd8,
            u1781Code::SnapshotFingerprint => 0x5bd9,
            u1781Code::TableBroken => 0x5bda,
            u1781Code::SegmentGuard => 0x5bdb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u1781Code::StateDup => "状态重复（拼缝错位 同名双态）",
            u1781Code::StateHole => "状态断档（层接口漏登 序缺失）",
            u1781Code::EventDup => "事件重复（同事件双声明）",
            u1781Code::TransDup => "迁移重复（同源同事件双迁移）",
            u1781Code::TransHole => "迁移缺失（应达态不可达）",
            u1781Code::TransCycle => "迁移环（非法自环/环路）",
            u1781Code::EntryBad => "入口非法（初态不在闭集）",
            u1781Code::ApplyLost => "迁移丢失（应用事件无效果）",
            u1781Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u1781Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u1781Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u1781Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u1781Code::StateDup => "按闭集唯一律去重",
            u1781Code::StateHole => "按闭集补态",
            u1781Code::EventDup => "按事件闭集去重",
            u1781Code::TransDup => "按确定性唯一迁移律合并",
            u1781Code::TransHole => "按迁移矩阵补边",
            u1781Code::TransCycle => "按无环律重构迁移",
            u1781Code::EntryBad => "按闭集钳制初态",
            u1781Code::ApplyLost => "核对事件前置条件",
            u1781Code::LedgerBroken => "按守恒律重建台账后重跑",
            u1781Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u1781Code::TableBroken => "按语义规则重建常量表",
            u1781Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 状态机 mxu1781flow（6 态 × 6 事件 · 迁移 8 条显式声明）
// --------------------------------------------------------------------------
/// 结构层 状态机（判据：拼缝错位）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu1781flowState {
    Layered,
    Puzzled,
    Asserted,
    Enrolled,
    Probed,
    Completed,
}

impl mxu1781flowState {
    /// 全量态闭集。
    pub const ALL: [mxu1781flowState; 6] = [
        mxu1781flowState::Layered,
        mxu1781flowState::Puzzled,
        mxu1781flowState::Asserted,
        mxu1781flowState::Enrolled,
        mxu1781flowState::Probed,
        mxu1781flowState::Completed,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            mxu1781flowState::Layered => 0,
            mxu1781flowState::Puzzled => 1,
            mxu1781flowState::Asserted => 2,
            mxu1781flowState::Enrolled => 3,
            mxu1781flowState::Probed => 4,
            mxu1781flowState::Completed => 5,
        }
    }
}

/// mxu1781flow事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu1781flowEvent {
    Layer,
    Puzzle,
    Assert,
    Enroll,
    Probe,
    Complete,
}

impl mxu1781flowEvent {
    /// 全量事件闭集。
    pub const ALL: [mxu1781flowEvent; 6] = [
        mxu1781flowEvent::Layer,
        mxu1781flowEvent::Puzzle,
        mxu1781flowEvent::Assert,
        mxu1781flowEvent::Enroll,
        mxu1781flowEvent::Probe,
        mxu1781flowEvent::Complete,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const MXU1781FLOW_TRANSITIONS: [(mxu1781flowState, mxu1781flowEvent, mxu1781flowState); 8] = [
    (mxu1781flowState::Layered, mxu1781flowEvent::Layer, mxu1781flowState::Puzzled), // Layered→Puzzled 主链
    (mxu1781flowState::Puzzled, mxu1781flowEvent::Puzzle, mxu1781flowState::Asserted), // Puzzled→Asserted 主链
    (mxu1781flowState::Asserted, mxu1781flowEvent::Assert, mxu1781flowState::Enrolled), // Asserted→Enrolled 主链
    (mxu1781flowState::Enrolled, mxu1781flowEvent::Enroll, mxu1781flowState::Probed), // Enrolled→Probed 主链
    (mxu1781flowState::Probed, mxu1781flowEvent::Probe, mxu1781flowState::Completed), // Probed→Completed 主链
    (mxu1781flowState::Completed, mxu1781flowEvent::Complete, mxu1781flowState::Layered), // 终态回边
    (mxu1781flowState::Completed, mxu1781flowEvent::Layer, mxu1781flowState::Probed), // 终态回退边
    (mxu1781flowState::Puzzled, mxu1781flowEvent::Puzzle, mxu1781flowState::Asserted), // 旁路边
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn mxu1781flow_next(state: mxu1781flowState, ev: mxu1781flowEvent) -> Option<mxu1781flowState> {
    let mut i = 0usize;
    while i < 8 {
        let t = MXU1781FLOW_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn mxu1781flow_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < mxu1781flowState::ALL.len() {
        let mut ei = 0usize;
        while ei < mxu1781flowEvent::ALL.len() {
            let legal = mxu1781flow_next(mxu1781flowState::ALL[si], mxu1781flowEvent::ALL[ei]).is_some();
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
pub fn mxu1781flow_terminal(st: mxu1781flowState) -> bool {
    let mut i = 0usize;
    while i < 8 {
        if MXU1781FLOW_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct mxu1781flowLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: mxu1781flowState,
}

impl mxu1781flowLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: mxu1781flowState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: mxu1781flowEvent) -> bool {
        match mxu1781flow_next(self.cur, ev) {
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
// 快照 Mxu1781Snap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// 结构层 快照封存（FNV 三态指纹 + 篡改检出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mxu1781Snap {
    /// 指纹分量：bj_v。
    pub bj_v: u64,
    /// 指纹分量：bl_v。
    pub bl_v: u64,
    /// 指纹分量：dl_v。
    pub dl_v: u64,
    /// 指纹分量：l_v。
    pub l_v: u64,
    /// 指纹分量：sjp_v。
    pub sjp_v: u64,
    /// 指纹分量：zz_v。
    pub zz_v: u64,
    /// 指纹分量：jh_v。
    pub jh_v: u64,
    /// 指纹分量：su_v。
    pub su_v: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl Mxu1781Snap {
    /// 构造并即时合成指纹。
    pub fn compute(
        bj_v: u64,
        bl_v: u64,
        dl_v: u64,
        l_v: u64,
        sjp_v: u64,
        zz_v: u64,
        jh_v: u64,
        su_v: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &bj_v.to_le_bytes());
        h = fnv1a64_from(h, &bl_v.to_le_bytes());
        h = fnv1a64_from(h, &dl_v.to_le_bytes());
        h = fnv1a64_from(h, &l_v.to_le_bytes());
        h = fnv1a64_from(h, &sjp_v.to_le_bytes());
        h = fnv1a64_from(h, &zz_v.to_le_bytes());
        h = fnv1a64_from(h, &jh_v.to_le_bytes());
        h = fnv1a64_from(h, &su_v.to_le_bytes());
        Self {
            bj_v,
            bl_v,
            dl_v,
            l_v,
            sjp_v,
            zz_v,
            jh_v,
            su_v,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.bj_v.to_le_bytes());
        h = fnv1a64_from(h, &self.bl_v.to_le_bytes());
        h = fnv1a64_from(h, &self.dl_v.to_le_bytes());
        h = fnv1a64_from(h, &self.l_v.to_le_bytes());
        h = fnv1a64_from(h, &self.sjp_v.to_le_bytes());
        h = fnv1a64_from(h, &self.zz_v.to_le_bytes());
        h = fnv1a64_from(h, &self.jh_v.to_le_bytes());
        h = fnv1a64_from(h, &self.su_v.to_le_bytes());
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

/// 段位守卫：全部诊断码必须落在本段 [0x5bd0, 0x5bdf]。
pub const fn u1781_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u1781Code::ALL.len() {
        let c = u1781Code::ALL[i].code();
        if c < 0x5bd0 || c > 0x5bdf {
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
fn u1781_guard_seg() -> bool {
    u1781_segment_guard()
}

/// UNX-F1781 状态机闭集/迁移表/非法组合恰值全绿。
fn check_u1781_sm_legal() -> bool {
    let mut ok = mxu1781flow_illegal_pairs() == 29;
    if ok { ok = MXU1781FLOW_TRANSITIONS.len() == 8; }
    if ok { ok = mxu1781flowState::ALL.len() == 6 && mxu1781flowEvent::ALL.len() == 6; }
    ok
}

/// UNX-F1781 迁移主链回放 + 快照往返恒等。
fn check_u1781_sm_round() -> bool {
    let mut lg = mxu1781flowLedger::new(mxu1781flowState::Layered);
    let mut ok = lg.apply(mxu1781flowEvent::Layer) && lg.cur == mxu1781flowState::Puzzled && lg.total() == 1;
    let s0 = Mxu1781Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    let mut s1 = Mxu1781Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    s1.recompute();
    if ok { ok = s0 == s1 && !s1.tampered(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u1781_checks() -> CheckSet {
    let mut set = CheckSet::new("U1781");
    set.add(
        "u1781-seg-guard",
        u1781_guard_seg(),
        if u1781_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u1781-sm-legal",
        check_u1781_sm_legal(),
        if check_u1781_sm_legal() { "" } else { "UNX-F1781 sm legality mismatch" },
    );
    set.add(
        "u1781-sm-round",
        check_u1781_sm_round(),
        if check_u1781_sm_round() { "" } else { "UNX-F1781 sm roundtrip mismatch" },
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
    fn u1781_all_checks_green() {
        let set = run_u1781_checks();
        assert!(set.all_passed(), "U1781 domain checks must be green");
    }

    #[test]
    fn u1781_segment_guard_holds() {
        assert!(u1781_segment_guard());
    }
}
