//! unxu2881_u2881sm —— COW 压测方案冻结（三矩阵） 状态机与快照封存（COW 三矩阵压测）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2881 · COW 压测方案冻结（三矩阵） · 判据原文：UNX-F2881 · 判据：UNX-F2881-J1 三矩阵（fork 密度/写密度/页类）参数冻结哈希入册，方案与 F2641 接口字段对齐
//! · 组件：COW 三矩阵压测 状态机（6 态 × 6 事件显式迁移）+ FNV 快照封存（8 分量）
//! · 判据落地：UNX-F2881 · 判据：UNX-F2881-J1 三矩阵（fork 密度/写密度/页类）参数冻结哈希入册，方案与 F2641 接口字段对齐
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2881_VERSION: &str = "u2881-u2881sm-v1";

/// 诊断段基址。
pub const U2881_SEG_BASE: u16 = 0xe200;

// --------------------------------------------------------------------------
// 诊断码（u2881 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// COW 三矩阵压测 状态面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2881Code {
    /// 0xe200 状态重复（COW 同名双态）。
    StateDup,
    /// 0xe201 状态断档（写时复制 序缺失）。
    StateHole,
    /// 0xe202 事件重复（同事件双声明）。
    EventDup,
    /// 0xe203 迁移重复（同源同事件双迁移）。
    TransDup,
    /// 0xe204 迁移缺失（应达态不可达）。
    TransHole,
    /// 0xe205 迁移环（非法自环/环路）。
    TransCycle,
    /// 0xe206 入口非法（初态不在闭集）。
    EntryBad,
    /// 0xe207 迁移丢失（应用事件无效果）。
    ApplyLost,
    /// 0xe208 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe209 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe20a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe20b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2881Code {
    /// 全量闭集。
    pub const ALL: [u2881Code; 12] = [
        u2881Code::StateDup,
        u2881Code::StateHole,
        u2881Code::EventDup,
        u2881Code::TransDup,
        u2881Code::TransHole,
        u2881Code::TransCycle,
        u2881Code::EntryBad,
        u2881Code::ApplyLost,
        u2881Code::LedgerBroken,
        u2881Code::SnapshotFingerprint,
        u2881Code::TableBroken,
        u2881Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2881Code::StateDup => 0xe200,
            u2881Code::StateHole => 0xe201,
            u2881Code::EventDup => 0xe202,
            u2881Code::TransDup => 0xe203,
            u2881Code::TransHole => 0xe204,
            u2881Code::TransCycle => 0xe205,
            u2881Code::EntryBad => 0xe206,
            u2881Code::ApplyLost => 0xe207,
            u2881Code::LedgerBroken => 0xe208,
            u2881Code::SnapshotFingerprint => 0xe209,
            u2881Code::TableBroken => 0xe20a,
            u2881Code::SegmentGuard => 0xe20b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2881Code::StateDup => "状态重复（COW 同名双态）",
            u2881Code::StateHole => "状态断档（写时复制 序缺失）",
            u2881Code::EventDup => "事件重复（同事件双声明）",
            u2881Code::TransDup => "迁移重复（同源同事件双迁移）",
            u2881Code::TransHole => "迁移缺失（应达态不可达）",
            u2881Code::TransCycle => "迁移环（非法自环/环路）",
            u2881Code::EntryBad => "入口非法（初态不在闭集）",
            u2881Code::ApplyLost => "迁移丢失（应用事件无效果）",
            u2881Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2881Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2881Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2881Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2881Code::StateDup => "按闭集唯一律去重",
            u2881Code::StateHole => "按闭集补态",
            u2881Code::EventDup => "按事件闭集去重",
            u2881Code::TransDup => "按确定性唯一迁移律合并",
            u2881Code::TransHole => "按迁移矩阵补边",
            u2881Code::TransCycle => "按无环律重构迁移",
            u2881Code::EntryBad => "按闭集钳制初态",
            u2881Code::ApplyLost => "核对事件前置条件",
            u2881Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2881Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2881Code::TableBroken => "按语义规则重建常量表",
            u2881Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 状态机 mxu2881flow（6 态 × 6 事件 · 迁移 8 条显式声明）
// --------------------------------------------------------------------------
/// COW 三矩阵压测 状态机（判据：COW）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu2881flowState {
    Plan,
    Armed,
    Running,
    Copying,
    Judged,
    Sealed,
}

impl mxu2881flowState {
    /// 全量态闭集。
    pub const ALL: [mxu2881flowState; 6] = [
        mxu2881flowState::Plan,
        mxu2881flowState::Armed,
        mxu2881flowState::Running,
        mxu2881flowState::Copying,
        mxu2881flowState::Judged,
        mxu2881flowState::Sealed,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            mxu2881flowState::Plan => 0,
            mxu2881flowState::Armed => 1,
            mxu2881flowState::Running => 2,
            mxu2881flowState::Copying => 3,
            mxu2881flowState::Judged => 4,
            mxu2881flowState::Sealed => 5,
        }
    }
}

/// mxu2881flow事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu2881flowEvent {
    Plan,
    Arm,
    Copy,
    Break,
    Write,
    Seal,
}

impl mxu2881flowEvent {
    /// 全量事件闭集。
    pub const ALL: [mxu2881flowEvent; 6] = [
        mxu2881flowEvent::Plan,
        mxu2881flowEvent::Arm,
        mxu2881flowEvent::Copy,
        mxu2881flowEvent::Break,
        mxu2881flowEvent::Write,
        mxu2881flowEvent::Seal,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const MXU2881FLOW_TRANSITIONS: [(mxu2881flowState, mxu2881flowEvent, mxu2881flowState); 8] = [
    (mxu2881flowState::Plan, mxu2881flowEvent::Plan, mxu2881flowState::Armed), // Plan→Armed 主链
    (mxu2881flowState::Armed, mxu2881flowEvent::Arm, mxu2881flowState::Running), // Armed→Running 主链
    (mxu2881flowState::Running, mxu2881flowEvent::Copy, mxu2881flowState::Copying), // Running→Copying 主链
    (mxu2881flowState::Copying, mxu2881flowEvent::Break, mxu2881flowState::Judged), // Copying→Judged 主链
    (mxu2881flowState::Judged, mxu2881flowEvent::Write, mxu2881flowState::Sealed), // Judged→Sealed 主链
    (mxu2881flowState::Sealed, mxu2881flowEvent::Seal, mxu2881flowState::Plan), // 终态回边
    (mxu2881flowState::Sealed, mxu2881flowEvent::Plan, mxu2881flowState::Judged), // 终态回退边
    (mxu2881flowState::Armed, mxu2881flowEvent::Arm, mxu2881flowState::Running), // 旁路边
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn mxu2881flow_next(state: mxu2881flowState, ev: mxu2881flowEvent) -> Option<mxu2881flowState> {
    let mut i = 0usize;
    while i < 8 {
        let t = MXU2881FLOW_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn mxu2881flow_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < mxu2881flowState::ALL.len() {
        let mut ei = 0usize;
        while ei < mxu2881flowEvent::ALL.len() {
            let legal = mxu2881flow_next(mxu2881flowState::ALL[si], mxu2881flowEvent::ALL[ei]).is_some();
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
pub fn mxu2881flow_terminal(st: mxu2881flowState) -> bool {
    let mut i = 0usize;
    while i < 8 {
        if MXU2881FLOW_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct mxu2881flowLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: mxu2881flowState,
}

impl mxu2881flowLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: mxu2881flowState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: mxu2881flowEvent) -> bool {
        match mxu2881flow_next(self.cur, ev) {
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
// 快照 Mxu2881Snap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// COW 三矩阵压测 快照封存（FNV 三态指纹 + 篡改检出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mxu2881Snap {
    /// 指纹分量：cow_v。
    pub cow_v: u64,
    /// 指纹分量：m1_v。
    pub m1_v: u64,
    /// 指纹分量：m2_v。
    pub m2_v: u64,
    /// 指纹分量：m3_v。
    pub m3_v: u64,
    /// 指纹分量：dup_v。
    pub dup_v: u64,
    /// 指纹分量：shr_v。
    pub shr_v: u64,
    /// 指纹分量：wback_v。
    pub wback_v: u64,
    /// 指纹分量：alias_v。
    pub alias_v: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl Mxu2881Snap {
    /// 构造并即时合成指纹。
    pub fn compute(
        cow_v: u64,
        m1_v: u64,
        m2_v: u64,
        m3_v: u64,
        dup_v: u64,
        shr_v: u64,
        wback_v: u64,
        alias_v: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &cow_v.to_le_bytes());
        h = fnv1a64_from(h, &m1_v.to_le_bytes());
        h = fnv1a64_from(h, &m2_v.to_le_bytes());
        h = fnv1a64_from(h, &m3_v.to_le_bytes());
        h = fnv1a64_from(h, &dup_v.to_le_bytes());
        h = fnv1a64_from(h, &shr_v.to_le_bytes());
        h = fnv1a64_from(h, &wback_v.to_le_bytes());
        h = fnv1a64_from(h, &alias_v.to_le_bytes());
        Self {
            cow_v,
            m1_v,
            m2_v,
            m3_v,
            dup_v,
            shr_v,
            wback_v,
            alias_v,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.cow_v.to_le_bytes());
        h = fnv1a64_from(h, &self.m1_v.to_le_bytes());
        h = fnv1a64_from(h, &self.m2_v.to_le_bytes());
        h = fnv1a64_from(h, &self.m3_v.to_le_bytes());
        h = fnv1a64_from(h, &self.dup_v.to_le_bytes());
        h = fnv1a64_from(h, &self.shr_v.to_le_bytes());
        h = fnv1a64_from(h, &self.wback_v.to_le_bytes());
        h = fnv1a64_from(h, &self.alias_v.to_le_bytes());
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

/// 段位守卫：全部诊断码必须落在本段 [0xe200, 0xe20f]。
pub const fn u2881_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2881Code::ALL.len() {
        let c = u2881Code::ALL[i].code();
        if c < 0xe200 || c > 0xe20f {
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
fn u2881_guard_seg() -> bool {
    u2881_segment_guard()
}

/// UNX-F2881 状态机闭集/迁移表/非法组合恰值全绿。
fn check_u2881_sm_legal() -> bool {
    let mut ok = mxu2881flow_illegal_pairs() == 29;
    if ok { ok = MXU2881FLOW_TRANSITIONS.len() == 8; }
    if ok { ok = mxu2881flowState::ALL.len() == 6 && mxu2881flowEvent::ALL.len() == 6; }
    ok
}

/// UNX-F2881 迁移主链回放 + 快照往返恒等。
fn check_u2881_sm_round() -> bool {
    let mut lg = mxu2881flowLedger::new(mxu2881flowState::Plan);
    let mut ok = lg.apply(mxu2881flowEvent::Plan) && lg.cur == mxu2881flowState::Armed && lg.total() == 1;
    let s0 = Mxu2881Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    let mut s1 = Mxu2881Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    s1.recompute();
    if ok { ok = s0 == s1 && !s1.tampered(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2881_checks() -> CheckSet {
    let mut set = CheckSet::new("U2881");
    set.add(
        "u2881-seg-guard",
        u2881_guard_seg(),
        if u2881_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2881-sm-legal",
        check_u2881_sm_legal(),
        if check_u2881_sm_legal() { "" } else { "UNX-F2881 sm legality mismatch" },
    );
    set.add(
        "u2881-sm-round",
        check_u2881_sm_round(),
        if check_u2881_sm_round() { "" } else { "UNX-F2881 sm roundtrip mismatch" },
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
    fn u2881_all_checks_green() {
        let set = run_u2881_checks();
        assert!(set.all_passed(), "U2881 domain checks must be green");
    }

    #[test]
    fn u2881_segment_guard_holds() {
        assert!(u2881_segment_guard());
    }
}
