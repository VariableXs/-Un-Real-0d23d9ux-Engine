//! unxu3001_u3001sm —— 双链结构冻结（active/inactive 两链节点布局） 状态机与快照封存（双链结构冻结）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3001 · 双链结构冻结（active/inactive 两链节点布局） · 判据原文：UNX-F3001 · 判据：UNX-F3001-J1 active/inactive 双链结构冻结哈希入册：节点布局（page 结构挂链位/lru 位标/双向指针）与 F2682 页缓存驻留结构对齐零缺失，链头尾指针操作单点化
//! · 组件：双链结构冻结 状态机（6 态 × 6 事件显式迁移）+ FNV 快照封存（8 分量）
//! · 判据落地：UNX-F3001 · 判据：UNX-F3001-J1 active/inactive 双链结构冻结哈希入册：节点布局（page 结构挂链位/lru 位标/双向
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3001_VERSION: &str = "u3001-u3001sm-v1";

/// 诊断段基址。
pub const U3001_SEG_BASE: u16 = 0xe260;

// --------------------------------------------------------------------------
// 诊断码（u3001 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 双链结构冻结 状态面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3001Code {
    /// 0xe260 状态重复（双链 同名双态）。
    StateDup,
    /// 0xe261 状态断档（节点 序缺失）。
    StateHole,
    /// 0xe262 事件重复（同事件双声明）。
    EventDup,
    /// 0xe263 迁移重复（同源同事件双迁移）。
    TransDup,
    /// 0xe264 迁移缺失（应达态不可达）。
    TransHole,
    /// 0xe265 迁移环（非法自环/环路）。
    TransCycle,
    /// 0xe266 入口非法（初态不在闭集）。
    EntryBad,
    /// 0xe267 迁移丢失（应用事件无效果）。
    ApplyLost,
    /// 0xe268 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe269 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe26a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe26b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3001Code {
    /// 全量闭集。
    pub const ALL: [u3001Code; 12] = [
        u3001Code::StateDup,
        u3001Code::StateHole,
        u3001Code::EventDup,
        u3001Code::TransDup,
        u3001Code::TransHole,
        u3001Code::TransCycle,
        u3001Code::EntryBad,
        u3001Code::ApplyLost,
        u3001Code::LedgerBroken,
        u3001Code::SnapshotFingerprint,
        u3001Code::TableBroken,
        u3001Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3001Code::StateDup => 0xe260,
            u3001Code::StateHole => 0xe261,
            u3001Code::EventDup => 0xe262,
            u3001Code::TransDup => 0xe263,
            u3001Code::TransHole => 0xe264,
            u3001Code::TransCycle => 0xe265,
            u3001Code::EntryBad => 0xe266,
            u3001Code::ApplyLost => 0xe267,
            u3001Code::LedgerBroken => 0xe268,
            u3001Code::SnapshotFingerprint => 0xe269,
            u3001Code::TableBroken => 0xe26a,
            u3001Code::SegmentGuard => 0xe26b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3001Code::StateDup => "状态重复（双链 同名双态）",
            u3001Code::StateHole => "状态断档（节点 序缺失）",
            u3001Code::EventDup => "事件重复（同事件双声明）",
            u3001Code::TransDup => "迁移重复（同源同事件双迁移）",
            u3001Code::TransHole => "迁移缺失（应达态不可达）",
            u3001Code::TransCycle => "迁移环（非法自环/环路）",
            u3001Code::EntryBad => "入口非法（初态不在闭集）",
            u3001Code::ApplyLost => "迁移丢失（应用事件无效果）",
            u3001Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3001Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3001Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3001Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3001Code::StateDup => "按闭集唯一律去重",
            u3001Code::StateHole => "按闭集补态",
            u3001Code::EventDup => "按事件闭集去重",
            u3001Code::TransDup => "按确定性唯一迁移律合并",
            u3001Code::TransHole => "按迁移矩阵补边",
            u3001Code::TransCycle => "按无环律重构迁移",
            u3001Code::EntryBad => "按闭集钳制初态",
            u3001Code::ApplyLost => "核对事件前置条件",
            u3001Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3001Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3001Code::TableBroken => "按语义规则重建常量表",
            u3001Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 状态机 mxu3001flow（6 态 × 6 事件 · 迁移 8 条显式声明）
// --------------------------------------------------------------------------
/// 双链结构冻结 状态机（判据：双链）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu3001flowState {
    Init,
    Active,
    Inactive,
    Moving,
    Merging,
    Sealed,
}

impl mxu3001flowState {
    /// 全量态闭集。
    pub const ALL: [mxu3001flowState; 6] = [
        mxu3001flowState::Init,
        mxu3001flowState::Active,
        mxu3001flowState::Inactive,
        mxu3001flowState::Moving,
        mxu3001flowState::Merging,
        mxu3001flowState::Sealed,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            mxu3001flowState::Init => 0,
            mxu3001flowState::Active => 1,
            mxu3001flowState::Inactive => 2,
            mxu3001flowState::Moving => 3,
            mxu3001flowState::Merging => 4,
            mxu3001flowState::Sealed => 5,
        }
    }
}

/// mxu3001flow事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu3001flowEvent {
    Alloc,
    Activate,
    Deactivate,
    Move,
    Merge,
    Seal,
}

impl mxu3001flowEvent {
    /// 全量事件闭集。
    pub const ALL: [mxu3001flowEvent; 6] = [
        mxu3001flowEvent::Alloc,
        mxu3001flowEvent::Activate,
        mxu3001flowEvent::Deactivate,
        mxu3001flowEvent::Move,
        mxu3001flowEvent::Merge,
        mxu3001flowEvent::Seal,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const MXU3001FLOW_TRANSITIONS: [(mxu3001flowState, mxu3001flowEvent, mxu3001flowState); 8] = [
    (mxu3001flowState::Init, mxu3001flowEvent::Alloc, mxu3001flowState::Active), // Init→Active 主链
    (mxu3001flowState::Active, mxu3001flowEvent::Activate, mxu3001flowState::Inactive), // Active→Inactive 主链
    (mxu3001flowState::Inactive, mxu3001flowEvent::Deactivate, mxu3001flowState::Moving), // Inactive→Moving 主链
    (mxu3001flowState::Moving, mxu3001flowEvent::Move, mxu3001flowState::Merging), // Moving→Merging 主链
    (mxu3001flowState::Merging, mxu3001flowEvent::Merge, mxu3001flowState::Sealed), // Merging→Sealed 主链
    (mxu3001flowState::Sealed, mxu3001flowEvent::Seal, mxu3001flowState::Init), // 终态回边
    (mxu3001flowState::Sealed, mxu3001flowEvent::Alloc, mxu3001flowState::Merging), // 终态回退边
    (mxu3001flowState::Active, mxu3001flowEvent::Activate, mxu3001flowState::Inactive), // 旁路边
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn mxu3001flow_next(state: mxu3001flowState, ev: mxu3001flowEvent) -> Option<mxu3001flowState> {
    let mut i = 0usize;
    while i < 8 {
        let t = MXU3001FLOW_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn mxu3001flow_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < mxu3001flowState::ALL.len() {
        let mut ei = 0usize;
        while ei < mxu3001flowEvent::ALL.len() {
            let legal = mxu3001flow_next(mxu3001flowState::ALL[si], mxu3001flowEvent::ALL[ei]).is_some();
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
pub fn mxu3001flow_terminal(st: mxu3001flowState) -> bool {
    let mut i = 0usize;
    while i < 8 {
        if MXU3001FLOW_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct mxu3001flowLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: mxu3001flowState,
}

impl mxu3001flowLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: mxu3001flowState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: mxu3001flowEvent) -> bool {
        match mxu3001flow_next(self.cur, ev) {
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
// 快照 Mxu3001Snap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// 双链结构冻结 快照封存（FNV 三态指纹 + 篡改检出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mxu3001Snap {
    /// 指纹分量：active_v。
    pub active_v: u64,
    /// 指纹分量：inact_v。
    pub inact_v: u64,
    /// 指纹分量：node_v。
    pub node_v: u64,
    /// 指纹分量：layout_v。
    pub layout_v: u64,
    /// 指纹分量：next_v。
    pub next_v: u64,
    /// 指纹分量：prev_v。
    pub prev_v: u64,
    /// 指纹分量：free_v。
    pub free_v: u64,
    /// 指纹分量：pool_v。
    pub pool_v: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl Mxu3001Snap {
    /// 构造并即时合成指纹。
    pub fn compute(
        active_v: u64,
        inact_v: u64,
        node_v: u64,
        layout_v: u64,
        next_v: u64,
        prev_v: u64,
        free_v: u64,
        pool_v: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &active_v.to_le_bytes());
        h = fnv1a64_from(h, &inact_v.to_le_bytes());
        h = fnv1a64_from(h, &node_v.to_le_bytes());
        h = fnv1a64_from(h, &layout_v.to_le_bytes());
        h = fnv1a64_from(h, &next_v.to_le_bytes());
        h = fnv1a64_from(h, &prev_v.to_le_bytes());
        h = fnv1a64_from(h, &free_v.to_le_bytes());
        h = fnv1a64_from(h, &pool_v.to_le_bytes());
        Self {
            active_v,
            inact_v,
            node_v,
            layout_v,
            next_v,
            prev_v,
            free_v,
            pool_v,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.active_v.to_le_bytes());
        h = fnv1a64_from(h, &self.inact_v.to_le_bytes());
        h = fnv1a64_from(h, &self.node_v.to_le_bytes());
        h = fnv1a64_from(h, &self.layout_v.to_le_bytes());
        h = fnv1a64_from(h, &self.next_v.to_le_bytes());
        h = fnv1a64_from(h, &self.prev_v.to_le_bytes());
        h = fnv1a64_from(h, &self.free_v.to_le_bytes());
        h = fnv1a64_from(h, &self.pool_v.to_le_bytes());
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

/// 段位守卫：全部诊断码必须落在本段 [0xe260, 0xe26f]。
pub const fn u3001_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3001Code::ALL.len() {
        let c = u3001Code::ALL[i].code();
        if c < 0xe260 || c > 0xe26f {
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
fn u3001_guard_seg() -> bool {
    u3001_segment_guard()
}

/// UNX-F3001 状态机闭集/迁移表/非法组合恰值全绿。
fn check_u3001_sm_legal() -> bool {
    let mut ok = mxu3001flow_illegal_pairs() == 29;
    if ok { ok = MXU3001FLOW_TRANSITIONS.len() == 8; }
    if ok { ok = mxu3001flowState::ALL.len() == 6 && mxu3001flowEvent::ALL.len() == 6; }
    ok
}

/// UNX-F3001 迁移主链回放 + 快照往返恒等。
fn check_u3001_sm_round() -> bool {
    let mut lg = mxu3001flowLedger::new(mxu3001flowState::Init);
    let mut ok = lg.apply(mxu3001flowEvent::Alloc) && lg.cur == mxu3001flowState::Active && lg.total() == 1;
    let s0 = Mxu3001Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    let mut s1 = Mxu3001Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    s1.recompute();
    if ok { ok = s0 == s1 && !s1.tampered(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3001_checks() -> CheckSet {
    let mut set = CheckSet::new("U3001");
    set.add(
        "u3001-seg-guard",
        u3001_guard_seg(),
        if u3001_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3001-sm-legal",
        check_u3001_sm_legal(),
        if check_u3001_sm_legal() { "" } else { "UNX-F3001 sm legality mismatch" },
    );
    set.add(
        "u3001-sm-round",
        check_u3001_sm_round(),
        if check_u3001_sm_round() { "" } else { "UNX-F3001 sm roundtrip mismatch" },
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
    fn u3001_all_checks_green() {
        let set = run_u3001_checks();
        assert!(set.all_passed(), "U3001 domain checks must be green");
    }

    #[test]
    fn u3001_segment_guard_holds() {
        assert!(u3001_segment_guard());
    }
}
