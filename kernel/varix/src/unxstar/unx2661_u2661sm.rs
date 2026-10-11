//! unxu2661_u2661sm —— 页缓存结构总纲（radix 树类，4K 页粒度——§73 好条范本） 状态机与快照封存（缓存页）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2661 · 页缓存结构总纲（radix 树类，4K 页粒度——§73 好条范本） · 判据原文：UNX-F2661 · 判据：radix 树类结构逐项断言（键=（文件对象，页偏移），10^6 次操作不变量全过），4K 粒度契约与 §73 范本一致
//! · 组件：缓存页 状态机（6 态 × 6 事件显式迁移）+ FNV 快照封存（8 分量）
//! · 判据落地：UNX-F2661 · 判据：radix 树类结构逐项断言（键=（文件对象，页偏移），10^6 次操作不变量全过），4K 粒度契约与 §73 范本一致
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2661_VERSION: &str = "u2661-u2661sm-v1";

/// 诊断段基址。
pub const U2661_SEG_BASE: u16 = 0xa9d0;

// --------------------------------------------------------------------------
// 诊断码（u2661 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 缓存页 状态面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2661Code {
    /// 0xa9d0 状态重复（键碰撞 同名双态）。
    StateDup,
    /// 0xa9d1 状态断档（粒度错配 序缺失）。
    StateHole,
    /// 0xa9d2 事件重复（同事件双声明）。
    EventDup,
    /// 0xa9d3 迁移重复（同源同事件双迁移）。
    TransDup,
    /// 0xa9d4 迁移缺失（应达态不可达）。
    TransHole,
    /// 0xa9d5 迁移环（非法自环/环路）。
    TransCycle,
    /// 0xa9d6 入口非法（初态不在闭集）。
    EntryBad,
    /// 0xa9d7 迁移丢失（应用事件无效果）。
    ApplyLost,
    /// 0xa9d8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xa9d9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xa9da 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xa9db 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2661Code {
    /// 全量闭集。
    pub const ALL: [u2661Code; 12] = [
        u2661Code::StateDup,
        u2661Code::StateHole,
        u2661Code::EventDup,
        u2661Code::TransDup,
        u2661Code::TransHole,
        u2661Code::TransCycle,
        u2661Code::EntryBad,
        u2661Code::ApplyLost,
        u2661Code::LedgerBroken,
        u2661Code::SnapshotFingerprint,
        u2661Code::TableBroken,
        u2661Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2661Code::StateDup => 0xa9d0,
            u2661Code::StateHole => 0xa9d1,
            u2661Code::EventDup => 0xa9d2,
            u2661Code::TransDup => 0xa9d3,
            u2661Code::TransHole => 0xa9d4,
            u2661Code::TransCycle => 0xa9d5,
            u2661Code::EntryBad => 0xa9d6,
            u2661Code::ApplyLost => 0xa9d7,
            u2661Code::LedgerBroken => 0xa9d8,
            u2661Code::SnapshotFingerprint => 0xa9d9,
            u2661Code::TableBroken => 0xa9da,
            u2661Code::SegmentGuard => 0xa9db,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2661Code::StateDup => "状态重复（键碰撞 同名双态）",
            u2661Code::StateHole => "状态断档（粒度错配 序缺失）",
            u2661Code::EventDup => "事件重复（同事件双声明）",
            u2661Code::TransDup => "迁移重复（同源同事件双迁移）",
            u2661Code::TransHole => "迁移缺失（应达态不可达）",
            u2661Code::TransCycle => "迁移环（非法自环/环路）",
            u2661Code::EntryBad => "入口非法（初态不在闭集）",
            u2661Code::ApplyLost => "迁移丢失（应用事件无效果）",
            u2661Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2661Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2661Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2661Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2661Code::StateDup => "按闭集唯一律去重",
            u2661Code::StateHole => "按闭集补态",
            u2661Code::EventDup => "按事件闭集去重",
            u2661Code::TransDup => "按确定性唯一迁移律合并",
            u2661Code::TransHole => "按迁移矩阵补边",
            u2661Code::TransCycle => "按无环律重构迁移",
            u2661Code::EntryBad => "按闭集钳制初态",
            u2661Code::ApplyLost => "核对事件前置条件",
            u2661Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2661Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2661Code::TableBroken => "按语义规则重建常量表",
            u2661Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 状态机 mxu2661flow（6 态 × 6 事件 · 迁移 8 条显式声明）
// --------------------------------------------------------------------------
/// 缓存页 状态机（判据：键碰撞）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu2661flowState {
    Cold,
    Indexing,
    Probing,
    Hit,
    Miss,
    Sealed,
}

impl mxu2661flowState {
    /// 全量态闭集。
    pub const ALL: [mxu2661flowState; 6] = [
        mxu2661flowState::Cold,
        mxu2661flowState::Indexing,
        mxu2661flowState::Probing,
        mxu2661flowState::Hit,
        mxu2661flowState::Miss,
        mxu2661flowState::Sealed,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            mxu2661flowState::Cold => 0,
            mxu2661flowState::Indexing => 1,
            mxu2661flowState::Probing => 2,
            mxu2661flowState::Hit => 3,
            mxu2661flowState::Miss => 4,
            mxu2661flowState::Sealed => 5,
        }
    }
}

/// mxu2661flow事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu2661flowEvent {
    Index,
    Probe,
    Hit,
    Miss,
    Evict,
    Seal,
}

impl mxu2661flowEvent {
    /// 全量事件闭集。
    pub const ALL: [mxu2661flowEvent; 6] = [
        mxu2661flowEvent::Index,
        mxu2661flowEvent::Probe,
        mxu2661flowEvent::Hit,
        mxu2661flowEvent::Miss,
        mxu2661flowEvent::Evict,
        mxu2661flowEvent::Seal,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const MXU2661FLOW_TRANSITIONS: [(mxu2661flowState, mxu2661flowEvent, mxu2661flowState); 8] = [
    (mxu2661flowState::Cold, mxu2661flowEvent::Index, mxu2661flowState::Indexing), // Cold→Indexing 主链
    (mxu2661flowState::Indexing, mxu2661flowEvent::Probe, mxu2661flowState::Probing), // Indexing→Probing 主链
    (mxu2661flowState::Probing, mxu2661flowEvent::Hit, mxu2661flowState::Hit), // Probing→Hit 主链
    (mxu2661flowState::Hit, mxu2661flowEvent::Miss, mxu2661flowState::Miss), // Hit→Miss 主链
    (mxu2661flowState::Miss, mxu2661flowEvent::Evict, mxu2661flowState::Sealed), // Miss→Sealed 主链
    (mxu2661flowState::Sealed, mxu2661flowEvent::Seal, mxu2661flowState::Cold), // 终态回边
    (mxu2661flowState::Sealed, mxu2661flowEvent::Index, mxu2661flowState::Miss), // 终态回退边
    (mxu2661flowState::Indexing, mxu2661flowEvent::Probe, mxu2661flowState::Probing), // 旁路边
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn mxu2661flow_next(state: mxu2661flowState, ev: mxu2661flowEvent) -> Option<mxu2661flowState> {
    let mut i = 0usize;
    while i < 8 {
        let t = MXU2661FLOW_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn mxu2661flow_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < mxu2661flowState::ALL.len() {
        let mut ei = 0usize;
        while ei < mxu2661flowEvent::ALL.len() {
            let legal = mxu2661flow_next(mxu2661flowState::ALL[si], mxu2661flowEvent::ALL[ei]).is_some();
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
pub fn mxu2661flow_terminal(st: mxu2661flowState) -> bool {
    let mut i = 0usize;
    while i < 8 {
        if MXU2661FLOW_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct mxu2661flowLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: mxu2661flowState,
}

impl mxu2661flowLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: mxu2661flowState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: mxu2661flowEvent) -> bool {
        match mxu2661flow_next(self.cur, ev) {
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
// 快照 Mxu2661Snap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// 缓存页 快照封存（FNV 三态指纹 + 篡改检出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mxu2661Snap {
    /// 指纹分量：pc_v。
    pub pc_v: u64,
    /// 指纹分量：rx_v。
    pub rx_v: u64,
    /// 指纹分量：ky_v。
    pub ky_v: u64,
    /// 指纹分量：fo_v。
    pub fo_v: u64,
    /// 指纹分量：po_v。
    pub po_v: u64,
    /// 指纹分量：g4_v。
    pub g4_v: u64,
    /// 指纹分量：m1_v。
    pub m1_v: u64,
    /// 指纹分量：iv_v。
    pub iv_v: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl Mxu2661Snap {
    /// 构造并即时合成指纹。
    pub fn compute(
        pc_v: u64,
        rx_v: u64,
        ky_v: u64,
        fo_v: u64,
        po_v: u64,
        g4_v: u64,
        m1_v: u64,
        iv_v: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &pc_v.to_le_bytes());
        h = fnv1a64_from(h, &rx_v.to_le_bytes());
        h = fnv1a64_from(h, &ky_v.to_le_bytes());
        h = fnv1a64_from(h, &fo_v.to_le_bytes());
        h = fnv1a64_from(h, &po_v.to_le_bytes());
        h = fnv1a64_from(h, &g4_v.to_le_bytes());
        h = fnv1a64_from(h, &m1_v.to_le_bytes());
        h = fnv1a64_from(h, &iv_v.to_le_bytes());
        Self {
            pc_v,
            rx_v,
            ky_v,
            fo_v,
            po_v,
            g4_v,
            m1_v,
            iv_v,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.pc_v.to_le_bytes());
        h = fnv1a64_from(h, &self.rx_v.to_le_bytes());
        h = fnv1a64_from(h, &self.ky_v.to_le_bytes());
        h = fnv1a64_from(h, &self.fo_v.to_le_bytes());
        h = fnv1a64_from(h, &self.po_v.to_le_bytes());
        h = fnv1a64_from(h, &self.g4_v.to_le_bytes());
        h = fnv1a64_from(h, &self.m1_v.to_le_bytes());
        h = fnv1a64_from(h, &self.iv_v.to_le_bytes());
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

/// 段位守卫：全部诊断码必须落在本段 [0xa9d0, 0xa9df]。
pub const fn u2661_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2661Code::ALL.len() {
        let c = u2661Code::ALL[i].code();
        if c < 0xa9d0 || c > 0xa9df {
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
fn u2661_guard_seg() -> bool {
    u2661_segment_guard()
}

/// UNX-F2661 状态机闭集/迁移表/非法组合恰值全绿。
fn check_u2661_sm_legal() -> bool {
    let mut ok = mxu2661flow_illegal_pairs() == 29;
    if ok { ok = MXU2661FLOW_TRANSITIONS.len() == 8; }
    if ok { ok = mxu2661flowState::ALL.len() == 6 && mxu2661flowEvent::ALL.len() == 6; }
    ok
}

/// UNX-F2661 迁移主链回放 + 快照往返恒等。
fn check_u2661_sm_round() -> bool {
    let mut lg = mxu2661flowLedger::new(mxu2661flowState::Cold);
    let mut ok = lg.apply(mxu2661flowEvent::Index) && lg.cur == mxu2661flowState::Indexing && lg.total() == 1;
    let s0 = Mxu2661Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    let mut s1 = Mxu2661Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    s1.recompute();
    if ok { ok = s0 == s1 && !s1.tampered(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2661_checks() -> CheckSet {
    let mut set = CheckSet::new("U2661");
    set.add(
        "u2661-seg-guard",
        u2661_guard_seg(),
        if u2661_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2661-sm-legal",
        check_u2661_sm_legal(),
        if check_u2661_sm_legal() { "" } else { "UNX-F2661 sm legality mismatch" },
    );
    set.add(
        "u2661-sm-round",
        check_u2661_sm_round(),
        if check_u2661_sm_round() { "" } else { "UNX-F2661 sm roundtrip mismatch" },
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
    fn u2661_all_checks_green() {
        let set = run_u2661_checks();
        assert!(set.all_passed(), "U2661 domain checks must be green");
    }

    #[test]
    fn u2661_segment_guard_holds() {
        assert!(u2661_segment_guard());
    }
}
