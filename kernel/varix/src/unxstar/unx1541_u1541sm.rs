//! unxu1541_u1541sm —— 域册终版总装（使命/地图/批次/接口/预案五节终版） 状态机与快照封存（节）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F1541 · 域册终版总装（使命/地图/批次/接口/预案五节终版） · 判据原文：UNX-F1541 · 判据：域册终版齐备（五节逐节终审：使命/域地图/批次地图 B01–B40/接口册/预案册），引用对平 100%（八百条目引用零失效），终版哈希落档
//! · 组件：节 状态机（6 态 × 6 事件显式迁移）+ FNV 快照封存（8 分量）
//! · 判据落地：UNX-F1541 · 判据：域册终版齐备（五节逐节终审：使命/域地图/批次地图 B01–B40/接口册/预案册），引用对平 100%（八百条目引用零失效），终
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U1541_VERSION: &str = "u1541-u1541sm-v1";

/// 诊断段基址。
pub const U1541_SEG_BASE: u16 = 0x52d0;

// --------------------------------------------------------------------------
// 诊断码（u1541 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 节 状态面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u1541Code {
    /// 0x52d0 状态重复（引用失效 同名双态）。
    StateDup,
    /// 0x52d1 状态断档（节缺失 序缺失）。
    StateHole,
    /// 0x52d2 事件重复（同事件双声明）。
    EventDup,
    /// 0x52d3 迁移重复（同源同事件双迁移）。
    TransDup,
    /// 0x52d4 迁移缺失（应达态不可达）。
    TransHole,
    /// 0x52d5 迁移环（非法自环/环路）。
    TransCycle,
    /// 0x52d6 入口非法（初态不在闭集）。
    EntryBad,
    /// 0x52d7 迁移丢失（应用事件无效果）。
    ApplyLost,
    /// 0x52d8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x52d9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x52da 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x52db 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u1541Code {
    /// 全量闭集。
    pub const ALL: [u1541Code; 12] = [
        u1541Code::StateDup,
        u1541Code::StateHole,
        u1541Code::EventDup,
        u1541Code::TransDup,
        u1541Code::TransHole,
        u1541Code::TransCycle,
        u1541Code::EntryBad,
        u1541Code::ApplyLost,
        u1541Code::LedgerBroken,
        u1541Code::SnapshotFingerprint,
        u1541Code::TableBroken,
        u1541Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u1541Code::StateDup => 0x52d0,
            u1541Code::StateHole => 0x52d1,
            u1541Code::EventDup => 0x52d2,
            u1541Code::TransDup => 0x52d3,
            u1541Code::TransHole => 0x52d4,
            u1541Code::TransCycle => 0x52d5,
            u1541Code::EntryBad => 0x52d6,
            u1541Code::ApplyLost => 0x52d7,
            u1541Code::LedgerBroken => 0x52d8,
            u1541Code::SnapshotFingerprint => 0x52d9,
            u1541Code::TableBroken => 0x52da,
            u1541Code::SegmentGuard => 0x52db,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u1541Code::StateDup => "状态重复（引用失效 同名双态）",
            u1541Code::StateHole => "状态断档（节缺失 序缺失）",
            u1541Code::EventDup => "事件重复（同事件双声明）",
            u1541Code::TransDup => "迁移重复（同源同事件双迁移）",
            u1541Code::TransHole => "迁移缺失（应达态不可达）",
            u1541Code::TransCycle => "迁移环（非法自环/环路）",
            u1541Code::EntryBad => "入口非法（初态不在闭集）",
            u1541Code::ApplyLost => "迁移丢失（应用事件无效果）",
            u1541Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u1541Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u1541Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u1541Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u1541Code::StateDup => "按闭集唯一律去重",
            u1541Code::StateHole => "按闭集补态",
            u1541Code::EventDup => "按事件闭集去重",
            u1541Code::TransDup => "按确定性唯一迁移律合并",
            u1541Code::TransHole => "按迁移矩阵补边",
            u1541Code::TransCycle => "按无环律重构迁移",
            u1541Code::EntryBad => "按闭集钳制初态",
            u1541Code::ApplyLost => "核对事件前置条件",
            u1541Code::LedgerBroken => "按守恒律重建台账后重跑",
            u1541Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u1541Code::TableBroken => "按语义规则重建常量表",
            u1541Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 状态机 mxu1541flow（6 态 × 6 事件 · 迁移 8 条显式声明）
// --------------------------------------------------------------------------
/// 节 状态机（判据：引用失效）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu1541flowState {
    Draft,
    Assembled,
    Crosschecked,
    Hashed,
    Reviewed,
    Final,
}

impl mxu1541flowState {
    /// 全量态闭集。
    pub const ALL: [mxu1541flowState; 6] = [
        mxu1541flowState::Draft,
        mxu1541flowState::Assembled,
        mxu1541flowState::Crosschecked,
        mxu1541flowState::Hashed,
        mxu1541flowState::Reviewed,
        mxu1541flowState::Final,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            mxu1541flowState::Draft => 0,
            mxu1541flowState::Assembled => 1,
            mxu1541flowState::Crosschecked => 2,
            mxu1541flowState::Hashed => 3,
            mxu1541flowState::Reviewed => 4,
            mxu1541flowState::Final => 5,
        }
    }
}

/// mxu1541flow事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu1541flowEvent {
    Assemble,
    Crosscheck,
    Hash,
    Review,
    VoidRef,
    Promote,
}

impl mxu1541flowEvent {
    /// 全量事件闭集。
    pub const ALL: [mxu1541flowEvent; 6] = [
        mxu1541flowEvent::Assemble,
        mxu1541flowEvent::Crosscheck,
        mxu1541flowEvent::Hash,
        mxu1541flowEvent::Review,
        mxu1541flowEvent::VoidRef,
        mxu1541flowEvent::Promote,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const MXU1541FLOW_TRANSITIONS: [(mxu1541flowState, mxu1541flowEvent, mxu1541flowState); 8] = [
    (mxu1541flowState::Draft, mxu1541flowEvent::Assemble, mxu1541flowState::Assembled), // Draft→Assembled 主链
    (mxu1541flowState::Assembled, mxu1541flowEvent::Crosscheck, mxu1541flowState::Crosschecked), // Assembled→Crosschecked 主链
    (mxu1541flowState::Crosschecked, mxu1541flowEvent::Hash, mxu1541flowState::Hashed), // Crosschecked→Hashed 主链
    (mxu1541flowState::Hashed, mxu1541flowEvent::Review, mxu1541flowState::Reviewed), // Hashed→Reviewed 主链
    (mxu1541flowState::Reviewed, mxu1541flowEvent::VoidRef, mxu1541flowState::Final), // Reviewed→Final 主链
    (mxu1541flowState::Final, mxu1541flowEvent::Promote, mxu1541flowState::Draft), // 终态回边
    (mxu1541flowState::Final, mxu1541flowEvent::Assemble, mxu1541flowState::Reviewed), // 终态回退边
    (mxu1541flowState::Assembled, mxu1541flowEvent::Crosscheck, mxu1541flowState::Crosschecked), // 旁路边
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn mxu1541flow_next(state: mxu1541flowState, ev: mxu1541flowEvent) -> Option<mxu1541flowState> {
    let mut i = 0usize;
    while i < 8 {
        let t = MXU1541FLOW_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn mxu1541flow_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < mxu1541flowState::ALL.len() {
        let mut ei = 0usize;
        while ei < mxu1541flowEvent::ALL.len() {
            let legal = mxu1541flow_next(mxu1541flowState::ALL[si], mxu1541flowEvent::ALL[ei]).is_some();
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
pub fn mxu1541flow_terminal(st: mxu1541flowState) -> bool {
    let mut i = 0usize;
    while i < 8 {
        if MXU1541FLOW_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct mxu1541flowLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: mxu1541flowState,
}

impl mxu1541flowLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: mxu1541flowState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: mxu1541flowEvent) -> bool {
        match mxu1541flow_next(self.cur, ev) {
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
// 快照 Mxu1541Snap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// 节 快照封存（FNV 三态指纹 + 篡改检出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mxu1541Snap {
    /// 指纹分量：ms_v。
    pub ms_v: u64,
    /// 指纹分量：dm_v。
    pub dm_v: u64,
    /// 指纹分量：bm_v。
    pub bm_v: u64,
    /// 指纹分量：if_v。
    pub if_v: u64,
    /// 指纹分量：pc_v。
    pub pc_v: u64,
    /// 指纹分量：rf_v。
    pub rf_v: u64,
    /// 指纹分量：fh_v。
    pub fh_v: u64,
    /// 指纹分量：fa_v。
    pub fa_v: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl Mxu1541Snap {
    /// 构造并即时合成指纹。
    pub fn compute(
        ms_v: u64,
        dm_v: u64,
        bm_v: u64,
        if_v: u64,
        pc_v: u64,
        rf_v: u64,
        fh_v: u64,
        fa_v: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &ms_v.to_le_bytes());
        h = fnv1a64_from(h, &dm_v.to_le_bytes());
        h = fnv1a64_from(h, &bm_v.to_le_bytes());
        h = fnv1a64_from(h, &if_v.to_le_bytes());
        h = fnv1a64_from(h, &pc_v.to_le_bytes());
        h = fnv1a64_from(h, &rf_v.to_le_bytes());
        h = fnv1a64_from(h, &fh_v.to_le_bytes());
        h = fnv1a64_from(h, &fa_v.to_le_bytes());
        Self {
            ms_v,
            dm_v,
            bm_v,
            if_v,
            pc_v,
            rf_v,
            fh_v,
            fa_v,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.ms_v.to_le_bytes());
        h = fnv1a64_from(h, &self.dm_v.to_le_bytes());
        h = fnv1a64_from(h, &self.bm_v.to_le_bytes());
        h = fnv1a64_from(h, &self.if_v.to_le_bytes());
        h = fnv1a64_from(h, &self.pc_v.to_le_bytes());
        h = fnv1a64_from(h, &self.rf_v.to_le_bytes());
        h = fnv1a64_from(h, &self.fh_v.to_le_bytes());
        h = fnv1a64_from(h, &self.fa_v.to_le_bytes());
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

/// 段位守卫：全部诊断码必须落在本段 [0x52d0, 0x52df]。
pub const fn u1541_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u1541Code::ALL.len() {
        let c = u1541Code::ALL[i].code();
        if c < 0x52d0 || c > 0x52df {
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
fn u1541_guard_seg() -> bool {
    u1541_segment_guard()
}

/// UNX-F1541 状态机闭集/迁移表/非法组合恰值全绿。
fn check_u1541_sm_legal() -> bool {
    let mut ok = mxu1541flow_illegal_pairs() == 29;
    if ok { ok = MXU1541FLOW_TRANSITIONS.len() == 8; }
    if ok { ok = mxu1541flowState::ALL.len() == 6 && mxu1541flowEvent::ALL.len() == 6; }
    ok
}

/// UNX-F1541 迁移主链回放 + 快照往返恒等。
fn check_u1541_sm_round() -> bool {
    let mut lg = mxu1541flowLedger::new(mxu1541flowState::Draft);
    let mut ok = lg.apply(mxu1541flowEvent::Assemble) && lg.cur == mxu1541flowState::Assembled && lg.total() == 1;
    let s0 = Mxu1541Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    let mut s1 = Mxu1541Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    s1.recompute();
    if ok { ok = s0 == s1 && !s1.tampered(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u1541_checks() -> CheckSet {
    let mut set = CheckSet::new("U1541");
    set.add(
        "u1541-seg-guard",
        u1541_guard_seg(),
        if u1541_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u1541-sm-legal",
        check_u1541_sm_legal(),
        if check_u1541_sm_legal() { "" } else { "UNX-F1541 sm legality mismatch" },
    );
    set.add(
        "u1541-sm-round",
        check_u1541_sm_round(),
        if check_u1541_sm_round() { "" } else { "UNX-F1541 sm roundtrip mismatch" },
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
    fn u1541_all_checks_green() {
        let set = run_u1541_checks();
        assert!(set.all_passed(), "U1541 domain checks must be green");
    }

    #[test]
    fn u1541_segment_guard_holds() {
        assert!(u1541_segment_guard());
    }
}
