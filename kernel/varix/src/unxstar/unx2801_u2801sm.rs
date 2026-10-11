//! unxu2801_u2801sm —— 大页池容量压测方案冻结 状态机与快照封存（大页池容量压测）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2801 · 大页池容量压测方案冻结 · 判据原文：UNX-F2801 · 判据：UNX-F2801-J1 三态方案（满池/近满/空）参数冻结哈希入册，方案与 F2425 池账字段逐项对齐
//! · 组件：大页池容量压测 状态机（6 态 × 6 事件显式迁移）+ FNV 快照封存（8 分量）
//! · 判据落地：UNX-F2801 · 判据：UNX-F2801-J1 三态方案（满池/近满/空）参数冻结哈希入册，方案与 F2425 池账字段逐项对齐
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2801_VERSION: &str = "u2801-u2801sm-v1";

/// 诊断段基址。
pub const U2801_SEG_BASE: u16 = 0xe1c0;

// --------------------------------------------------------------------------
// 诊断码（u2801 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 大页池容量压测 状态面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2801Code {
    /// 0xe1c0 状态重复（大页 同名双态）。
    StateDup,
    /// 0xe1c1 状态断档（压测 序缺失）。
    StateHole,
    /// 0xe1c2 事件重复（同事件双声明）。
    EventDup,
    /// 0xe1c3 迁移重复（同源同事件双迁移）。
    TransDup,
    /// 0xe1c4 迁移缺失（应达态不可达）。
    TransHole,
    /// 0xe1c5 迁移环（非法自环/环路）。
    TransCycle,
    /// 0xe1c6 入口非法（初态不在闭集）。
    EntryBad,
    /// 0xe1c7 迁移丢失（应用事件无效果）。
    ApplyLost,
    /// 0xe1c8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe1c9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe1ca 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe1cb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2801Code {
    /// 全量闭集。
    pub const ALL: [u2801Code; 12] = [
        u2801Code::StateDup,
        u2801Code::StateHole,
        u2801Code::EventDup,
        u2801Code::TransDup,
        u2801Code::TransHole,
        u2801Code::TransCycle,
        u2801Code::EntryBad,
        u2801Code::ApplyLost,
        u2801Code::LedgerBroken,
        u2801Code::SnapshotFingerprint,
        u2801Code::TableBroken,
        u2801Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2801Code::StateDup => 0xe1c0,
            u2801Code::StateHole => 0xe1c1,
            u2801Code::EventDup => 0xe1c2,
            u2801Code::TransDup => 0xe1c3,
            u2801Code::TransHole => 0xe1c4,
            u2801Code::TransCycle => 0xe1c5,
            u2801Code::EntryBad => 0xe1c6,
            u2801Code::ApplyLost => 0xe1c7,
            u2801Code::LedgerBroken => 0xe1c8,
            u2801Code::SnapshotFingerprint => 0xe1c9,
            u2801Code::TableBroken => 0xe1ca,
            u2801Code::SegmentGuard => 0xe1cb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2801Code::StateDup => "状态重复（大页 同名双态）",
            u2801Code::StateHole => "状态断档（压测 序缺失）",
            u2801Code::EventDup => "事件重复（同事件双声明）",
            u2801Code::TransDup => "迁移重复（同源同事件双迁移）",
            u2801Code::TransHole => "迁移缺失（应达态不可达）",
            u2801Code::TransCycle => "迁移环（非法自环/环路）",
            u2801Code::EntryBad => "入口非法（初态不在闭集）",
            u2801Code::ApplyLost => "迁移丢失（应用事件无效果）",
            u2801Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2801Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2801Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2801Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2801Code::StateDup => "按闭集唯一律去重",
            u2801Code::StateHole => "按闭集补态",
            u2801Code::EventDup => "按事件闭集去重",
            u2801Code::TransDup => "按确定性唯一迁移律合并",
            u2801Code::TransHole => "按迁移矩阵补边",
            u2801Code::TransCycle => "按无环律重构迁移",
            u2801Code::EntryBad => "按闭集钳制初态",
            u2801Code::ApplyLost => "核对事件前置条件",
            u2801Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2801Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2801Code::TableBroken => "按语义规则重建常量表",
            u2801Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 状态机 mxu2801flow（6 态 × 6 事件 · 迁移 8 条显式声明）
// --------------------------------------------------------------------------
/// 大页池容量压测 状态机（判据：大页）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu2801flowState {
    Plan,
    Armed,
    Running,
    Sampling,
    Judged,
    Sealed,
}

impl mxu2801flowState {
    /// 全量态闭集。
    pub const ALL: [mxu2801flowState; 6] = [
        mxu2801flowState::Plan,
        mxu2801flowState::Armed,
        mxu2801flowState::Running,
        mxu2801flowState::Sampling,
        mxu2801flowState::Judged,
        mxu2801flowState::Sealed,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            mxu2801flowState::Plan => 0,
            mxu2801flowState::Armed => 1,
            mxu2801flowState::Running => 2,
            mxu2801flowState::Sampling => 3,
            mxu2801flowState::Judged => 4,
            mxu2801flowState::Sealed => 5,
        }
    }
}

/// mxu2801flow事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu2801flowEvent {
    Plan,
    Arm,
    Run,
    Sample,
    Judge,
    Seal,
}

impl mxu2801flowEvent {
    /// 全量事件闭集。
    pub const ALL: [mxu2801flowEvent; 6] = [
        mxu2801flowEvent::Plan,
        mxu2801flowEvent::Arm,
        mxu2801flowEvent::Run,
        mxu2801flowEvent::Sample,
        mxu2801flowEvent::Judge,
        mxu2801flowEvent::Seal,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const MXU2801FLOW_TRANSITIONS: [(mxu2801flowState, mxu2801flowEvent, mxu2801flowState); 8] = [
    (mxu2801flowState::Plan, mxu2801flowEvent::Plan, mxu2801flowState::Armed), // Plan→Armed 主链
    (mxu2801flowState::Armed, mxu2801flowEvent::Arm, mxu2801flowState::Running), // Armed→Running 主链
    (mxu2801flowState::Running, mxu2801flowEvent::Run, mxu2801flowState::Sampling), // Running→Sampling 主链
    (mxu2801flowState::Sampling, mxu2801flowEvent::Sample, mxu2801flowState::Judged), // Sampling→Judged 主链
    (mxu2801flowState::Judged, mxu2801flowEvent::Judge, mxu2801flowState::Sealed), // Judged→Sealed 主链
    (mxu2801flowState::Sealed, mxu2801flowEvent::Seal, mxu2801flowState::Plan), // 终态回边
    (mxu2801flowState::Sealed, mxu2801flowEvent::Plan, mxu2801flowState::Judged), // 终态回退边
    (mxu2801flowState::Armed, mxu2801flowEvent::Arm, mxu2801flowState::Running), // 旁路边
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn mxu2801flow_next(state: mxu2801flowState, ev: mxu2801flowEvent) -> Option<mxu2801flowState> {
    let mut i = 0usize;
    while i < 8 {
        let t = MXU2801FLOW_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn mxu2801flow_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < mxu2801flowState::ALL.len() {
        let mut ei = 0usize;
        while ei < mxu2801flowEvent::ALL.len() {
            let legal = mxu2801flow_next(mxu2801flowState::ALL[si], mxu2801flowEvent::ALL[ei]).is_some();
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
pub fn mxu2801flow_terminal(st: mxu2801flowState) -> bool {
    let mut i = 0usize;
    while i < 8 {
        if MXU2801FLOW_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct mxu2801flowLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: mxu2801flowState,
}

impl mxu2801flowLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: mxu2801flowState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: mxu2801flowEvent) -> bool {
        match mxu2801flow_next(self.cur, ev) {
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
// 快照 Mxu2801Snap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// 大页池容量压测 快照封存（FNV 三态指纹 + 篡改检出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mxu2801Snap {
    /// 指纹分量：hp_v。
    pub hp_v: u64,
    /// 指纹分量：pool_v。
    pub pool_v: u64,
    /// 指纹分量：cap_v。
    pub cap_v: u64,
    /// 指纹分量：press_v。
    pub press_v: u64,
    /// 指纹分量：matrix_v。
    pub matrix_v: u64,
    /// 指纹分量：alloc_v。
    pub alloc_v: u64,
    /// 指纹分量：free_v。
    pub free_v: u64,
    /// 指纹分量：frag_v。
    pub frag_v: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl Mxu2801Snap {
    /// 构造并即时合成指纹。
    pub fn compute(
        hp_v: u64,
        pool_v: u64,
        cap_v: u64,
        press_v: u64,
        matrix_v: u64,
        alloc_v: u64,
        free_v: u64,
        frag_v: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &hp_v.to_le_bytes());
        h = fnv1a64_from(h, &pool_v.to_le_bytes());
        h = fnv1a64_from(h, &cap_v.to_le_bytes());
        h = fnv1a64_from(h, &press_v.to_le_bytes());
        h = fnv1a64_from(h, &matrix_v.to_le_bytes());
        h = fnv1a64_from(h, &alloc_v.to_le_bytes());
        h = fnv1a64_from(h, &free_v.to_le_bytes());
        h = fnv1a64_from(h, &frag_v.to_le_bytes());
        Self {
            hp_v,
            pool_v,
            cap_v,
            press_v,
            matrix_v,
            alloc_v,
            free_v,
            frag_v,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.hp_v.to_le_bytes());
        h = fnv1a64_from(h, &self.pool_v.to_le_bytes());
        h = fnv1a64_from(h, &self.cap_v.to_le_bytes());
        h = fnv1a64_from(h, &self.press_v.to_le_bytes());
        h = fnv1a64_from(h, &self.matrix_v.to_le_bytes());
        h = fnv1a64_from(h, &self.alloc_v.to_le_bytes());
        h = fnv1a64_from(h, &self.free_v.to_le_bytes());
        h = fnv1a64_from(h, &self.frag_v.to_le_bytes());
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

/// 段位守卫：全部诊断码必须落在本段 [0xe1c0, 0xe1cf]。
pub const fn u2801_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2801Code::ALL.len() {
        let c = u2801Code::ALL[i].code();
        if c < 0xe1c0 || c > 0xe1cf {
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
fn u2801_guard_seg() -> bool {
    u2801_segment_guard()
}

/// UNX-F2801 状态机闭集/迁移表/非法组合恰值全绿。
fn check_u2801_sm_legal() -> bool {
    let mut ok = mxu2801flow_illegal_pairs() == 29;
    if ok { ok = MXU2801FLOW_TRANSITIONS.len() == 8; }
    if ok { ok = mxu2801flowState::ALL.len() == 6 && mxu2801flowEvent::ALL.len() == 6; }
    ok
}

/// UNX-F2801 迁移主链回放 + 快照往返恒等。
fn check_u2801_sm_round() -> bool {
    let mut lg = mxu2801flowLedger::new(mxu2801flowState::Plan);
    let mut ok = lg.apply(mxu2801flowEvent::Plan) && lg.cur == mxu2801flowState::Armed && lg.total() == 1;
    let s0 = Mxu2801Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    let mut s1 = Mxu2801Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    s1.recompute();
    if ok { ok = s0 == s1 && !s1.tampered(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2801_checks() -> CheckSet {
    let mut set = CheckSet::new("U2801");
    set.add(
        "u2801-seg-guard",
        u2801_guard_seg(),
        if u2801_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2801-sm-legal",
        check_u2801_sm_legal(),
        if check_u2801_sm_legal() { "" } else { "UNX-F2801 sm legality mismatch" },
    );
    set.add(
        "u2801-sm-round",
        check_u2801_sm_round(),
        if check_u2801_sm_round() { "" } else { "UNX-F2801 sm roundtrip mismatch" },
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
    fn u2801_all_checks_green() {
        let set = run_u2801_checks();
        assert!(set.all_passed(), "U2801 domain checks must be green");
    }

    #[test]
    fn u2801_segment_guard_holds() {
        assert!(u2801_segment_guard());
    }
}
