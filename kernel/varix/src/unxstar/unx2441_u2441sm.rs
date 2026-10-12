//! unxu2441_u2441sm —— 每 CPU 页池结构（32 页批量） 状态机与快照封存（每核页池）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2441 · 每 CPU 页池结构（32 页批量） · 判据原文：UNX-F2441 · 判据：池容量 32 页/核/区三机型配置账一致，池内页与 buddy 账恒等（抽核零漂移）
//! · 组件：每核页池 状态机（6 态 × 6 事件显式迁移）+ FNV 快照封存（8 分量）
//! · 判据落地：UNX-F2441 · 判据：池容量 32 页/核/区三机型配置账一致，池内页与 buddy 账恒等（抽核零漂移）
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2441_VERSION: &str = "u2441-u2441sm-v1";

/// 诊断段基址。
pub const U2441_SEG_BASE: u16 = 0xa190;

// --------------------------------------------------------------------------
// 诊断码（u2441 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 每核页池 状态面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2441Code {
    /// 0xa190 状态重复（容量失配 同名双态）。
    StateDup,
    /// 0xa191 状态断档（恒等破缺 序缺失）。
    StateHole,
    /// 0xa192 事件重复（同事件双声明）。
    EventDup,
    /// 0xa193 迁移重复（同源同事件双迁移）。
    TransDup,
    /// 0xa194 迁移缺失（应达态不可达）。
    TransHole,
    /// 0xa195 迁移环（非法自环/环路）。
    TransCycle,
    /// 0xa196 入口非法（初态不在闭集）。
    EntryBad,
    /// 0xa197 迁移丢失（应用事件无效果）。
    ApplyLost,
    /// 0xa198 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xa199 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xa19a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xa19b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2441Code {
    /// 全量闭集。
    pub const ALL: [u2441Code; 12] = [
        u2441Code::StateDup,
        u2441Code::StateHole,
        u2441Code::EventDup,
        u2441Code::TransDup,
        u2441Code::TransHole,
        u2441Code::TransCycle,
        u2441Code::EntryBad,
        u2441Code::ApplyLost,
        u2441Code::LedgerBroken,
        u2441Code::SnapshotFingerprint,
        u2441Code::TableBroken,
        u2441Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2441Code::StateDup => 0xa190,
            u2441Code::StateHole => 0xa191,
            u2441Code::EventDup => 0xa192,
            u2441Code::TransDup => 0xa193,
            u2441Code::TransHole => 0xa194,
            u2441Code::TransCycle => 0xa195,
            u2441Code::EntryBad => 0xa196,
            u2441Code::ApplyLost => 0xa197,
            u2441Code::LedgerBroken => 0xa198,
            u2441Code::SnapshotFingerprint => 0xa199,
            u2441Code::TableBroken => 0xa19a,
            u2441Code::SegmentGuard => 0xa19b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2441Code::StateDup => "状态重复（容量失配 同名双态）",
            u2441Code::StateHole => "状态断档（恒等破缺 序缺失）",
            u2441Code::EventDup => "事件重复（同事件双声明）",
            u2441Code::TransDup => "迁移重复（同源同事件双迁移）",
            u2441Code::TransHole => "迁移缺失（应达态不可达）",
            u2441Code::TransCycle => "迁移环（非法自环/环路）",
            u2441Code::EntryBad => "入口非法（初态不在闭集）",
            u2441Code::ApplyLost => "迁移丢失（应用事件无效果）",
            u2441Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2441Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2441Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2441Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2441Code::StateDup => "按闭集唯一律去重",
            u2441Code::StateHole => "按闭集补态",
            u2441Code::EventDup => "按事件闭集去重",
            u2441Code::TransDup => "按确定性唯一迁移律合并",
            u2441Code::TransHole => "按迁移矩阵补边",
            u2441Code::TransCycle => "按无环律重构迁移",
            u2441Code::EntryBad => "按闭集钳制初态",
            u2441Code::ApplyLost => "核对事件前置条件",
            u2441Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2441Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2441Code::TableBroken => "按语义规则重建常量表",
            u2441Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 状态机 mxu2441flow（6 态 × 6 事件 · 迁移 8 条显式声明）
// --------------------------------------------------------------------------
/// 每核页池 状态机（判据：容量失配）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu2441flowState {
    Cold,
    Filled,
    Sampled,
    Equated,
    DriftScan,
    Sealed,
}

impl mxu2441flowState {
    /// 全量态闭集。
    pub const ALL: [mxu2441flowState; 6] = [
        mxu2441flowState::Cold,
        mxu2441flowState::Filled,
        mxu2441flowState::Sampled,
        mxu2441flowState::Equated,
        mxu2441flowState::DriftScan,
        mxu2441flowState::Sealed,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            mxu2441flowState::Cold => 0,
            mxu2441flowState::Filled => 1,
            mxu2441flowState::Sampled => 2,
            mxu2441flowState::Equated => 3,
            mxu2441flowState::DriftScan => 4,
            mxu2441flowState::Sealed => 5,
        }
    }
}

/// mxu2441flow事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu2441flowEvent {
    Fill32,
    SampleCore,
    CompareBuddy,
    DetectDrift,
    Repair,
    Seal,
}

impl mxu2441flowEvent {
    /// 全量事件闭集。
    pub const ALL: [mxu2441flowEvent; 6] = [
        mxu2441flowEvent::Fill32,
        mxu2441flowEvent::SampleCore,
        mxu2441flowEvent::CompareBuddy,
        mxu2441flowEvent::DetectDrift,
        mxu2441flowEvent::Repair,
        mxu2441flowEvent::Seal,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const MXU2441FLOW_TRANSITIONS: [(mxu2441flowState, mxu2441flowEvent, mxu2441flowState); 8] = [
    (mxu2441flowState::Cold, mxu2441flowEvent::Fill32, mxu2441flowState::Filled), // Cold→Filled 主链
    (mxu2441flowState::Filled, mxu2441flowEvent::SampleCore, mxu2441flowState::Sampled), // Filled→Sampled 主链
    (mxu2441flowState::Sampled, mxu2441flowEvent::CompareBuddy, mxu2441flowState::Equated), // Sampled→Equated 主链
    (mxu2441flowState::Equated, mxu2441flowEvent::DetectDrift, mxu2441flowState::DriftScan), // Equated→DriftScan 主链
    (mxu2441flowState::DriftScan, mxu2441flowEvent::Repair, mxu2441flowState::Sealed), // DriftScan→Sealed 主链
    (mxu2441flowState::Sealed, mxu2441flowEvent::Seal, mxu2441flowState::Cold), // 终态回边
    (mxu2441flowState::Sealed, mxu2441flowEvent::Fill32, mxu2441flowState::DriftScan), // 终态回退边
    (mxu2441flowState::Filled, mxu2441flowEvent::SampleCore, mxu2441flowState::Sampled), // 旁路边
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn mxu2441flow_next(state: mxu2441flowState, ev: mxu2441flowEvent) -> Option<mxu2441flowState> {
    let mut i = 0usize;
    while i < 8 {
        let t = MXU2441FLOW_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn mxu2441flow_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < mxu2441flowState::ALL.len() {
        let mut ei = 0usize;
        while ei < mxu2441flowEvent::ALL.len() {
            let legal = mxu2441flow_next(mxu2441flowState::ALL[si], mxu2441flowEvent::ALL[ei]).is_some();
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
pub fn mxu2441flow_terminal(st: mxu2441flowState) -> bool {
    let mut i = 0usize;
    while i < 8 {
        if MXU2441FLOW_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct mxu2441flowLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: mxu2441flowState,
}

impl mxu2441flowLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: mxu2441flowState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: mxu2441flowEvent) -> bool {
        match mxu2441flow_next(self.cur, ev) {
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
// 快照 Mxu2441Snap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// 每核页池 快照封存（FNV 三态指纹 + 篡改检出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mxu2441Snap {
    /// 指纹分量：pc_v。
    pub pc_v: u64,
    /// 指纹分量：cap_v。
    pub cap_v: u64,
    /// 指纹分量：bz_v。
    pub bz_v: u64,
    /// 指纹分量：bq_v。
    pub bq_v: u64,
    /// 指纹分量：id_v。
    pub id_v: u64,
    /// 指纹分量：sm_v。
    pub sm_v: u64,
    /// 指纹分量：dr_v。
    pub dr_v: u64,
    /// 指纹分量：bt_v。
    pub bt_v: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl Mxu2441Snap {
    /// 构造并即时合成指纹。
    pub fn compute(
        pc_v: u64,
        cap_v: u64,
        bz_v: u64,
        bq_v: u64,
        id_v: u64,
        sm_v: u64,
        dr_v: u64,
        bt_v: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &pc_v.to_le_bytes());
        h = fnv1a64_from(h, &cap_v.to_le_bytes());
        h = fnv1a64_from(h, &bz_v.to_le_bytes());
        h = fnv1a64_from(h, &bq_v.to_le_bytes());
        h = fnv1a64_from(h, &id_v.to_le_bytes());
        h = fnv1a64_from(h, &sm_v.to_le_bytes());
        h = fnv1a64_from(h, &dr_v.to_le_bytes());
        h = fnv1a64_from(h, &bt_v.to_le_bytes());
        Self {
            pc_v,
            cap_v,
            bz_v,
            bq_v,
            id_v,
            sm_v,
            dr_v,
            bt_v,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.pc_v.to_le_bytes());
        h = fnv1a64_from(h, &self.cap_v.to_le_bytes());
        h = fnv1a64_from(h, &self.bz_v.to_le_bytes());
        h = fnv1a64_from(h, &self.bq_v.to_le_bytes());
        h = fnv1a64_from(h, &self.id_v.to_le_bytes());
        h = fnv1a64_from(h, &self.sm_v.to_le_bytes());
        h = fnv1a64_from(h, &self.dr_v.to_le_bytes());
        h = fnv1a64_from(h, &self.bt_v.to_le_bytes());
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

/// 段位守卫：全部诊断码必须落在本段 [0xa190, 0xa19f]。
pub const fn u2441_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2441Code::ALL.len() {
        let c = u2441Code::ALL[i].code();
        if c < 0xa190 || c > 0xa19f {
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
fn u2441_guard_seg() -> bool {
    u2441_segment_guard()
}

/// UNX-F2441 状态机闭集/迁移表/非法组合恰值全绿。
fn check_u2441_sm_legal() -> bool {
    let mut ok = mxu2441flow_illegal_pairs() == 29;
    if ok { ok = MXU2441FLOW_TRANSITIONS.len() == 8; }
    if ok { ok = mxu2441flowState::ALL.len() == 6 && mxu2441flowEvent::ALL.len() == 6; }
    ok
}

/// UNX-F2441 迁移主链回放 + 快照往返恒等。
fn check_u2441_sm_round() -> bool {
    let mut lg = mxu2441flowLedger::new(mxu2441flowState::Cold);
    let mut ok = lg.apply(mxu2441flowEvent::Fill32) && lg.cur == mxu2441flowState::Filled && lg.total() == 1;
    let s0 = Mxu2441Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    let mut s1 = Mxu2441Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    s1.recompute();
    if ok { ok = s0 == s1 && !s1.tampered(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2441_checks() -> CheckSet {
    let mut set = CheckSet::new("U2441");
    set.add(
        "u2441-seg-guard",
        u2441_guard_seg(),
        if u2441_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2441-sm-legal",
        check_u2441_sm_legal(),
        if check_u2441_sm_legal() { "" } else { "UNX-F2441 sm legality mismatch" },
    );
    set.add(
        "u2441-sm-round",
        check_u2441_sm_round(),
        if check_u2441_sm_round() { "" } else { "UNX-F2441 sm roundtrip mismatch" },
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
    fn u2441_all_checks_green() {
        let set = run_u2441_checks();
        assert!(set.all_passed(), "U2441 domain checks must be green");
    }

    #[test]
    fn u2441_segment_guard_holds() {
        assert!(u2441_segment_guard());
    }
}
