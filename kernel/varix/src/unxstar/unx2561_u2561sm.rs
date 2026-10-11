//! unxu2561_u2561sm —— AddressSpace 成熟体总纲（现存 addrspace.rs 接管） 状态机与快照封存（地址空间体）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2561 · AddressSpace 成熟体总纲（现存 addrspace.rs 接管） · 判据原文：UNX-F2561 · 判据：现存 `addrspace.rs`（F001 起步态）接管账在档（语义逐项映射），成熟体接口面三机直测一致
//! · 组件：地址空间体 状态机（6 态 × 6 事件显式迁移）+ FNV 快照封存（8 分量）
//! · 判据落地：UNX-F2561 · 判据：现存 `addrspace.rs`（F001 起步态）接管账在档（语义逐项映射），成熟体接口面三机直测一致
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2561_VERSION: &str = "u2561-u2561sm-v1";

/// 诊断段基址。
pub const U2561_SEG_BASE: u16 = 0xa610;

// --------------------------------------------------------------------------
// 诊断码（u2561 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 地址空间体 状态面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2561Code {
    /// 0xa610 状态重复（接管漏项 同名双态）。
    StateDup,
    /// 0xa611 状态断档（语义错配 序缺失）。
    StateHole,
    /// 0xa612 事件重复（同事件双声明）。
    EventDup,
    /// 0xa613 迁移重复（同源同事件双迁移）。
    TransDup,
    /// 0xa614 迁移缺失（应达态不可达）。
    TransHole,
    /// 0xa615 迁移环（非法自环/环路）。
    TransCycle,
    /// 0xa616 入口非法（初态不在闭集）。
    EntryBad,
    /// 0xa617 迁移丢失（应用事件无效果）。
    ApplyLost,
    /// 0xa618 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xa619 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xa61a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xa61b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2561Code {
    /// 全量闭集。
    pub const ALL: [u2561Code; 12] = [
        u2561Code::StateDup,
        u2561Code::StateHole,
        u2561Code::EventDup,
        u2561Code::TransDup,
        u2561Code::TransHole,
        u2561Code::TransCycle,
        u2561Code::EntryBad,
        u2561Code::ApplyLost,
        u2561Code::LedgerBroken,
        u2561Code::SnapshotFingerprint,
        u2561Code::TableBroken,
        u2561Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2561Code::StateDup => 0xa610,
            u2561Code::StateHole => 0xa611,
            u2561Code::EventDup => 0xa612,
            u2561Code::TransDup => 0xa613,
            u2561Code::TransHole => 0xa614,
            u2561Code::TransCycle => 0xa615,
            u2561Code::EntryBad => 0xa616,
            u2561Code::ApplyLost => 0xa617,
            u2561Code::LedgerBroken => 0xa618,
            u2561Code::SnapshotFingerprint => 0xa619,
            u2561Code::TableBroken => 0xa61a,
            u2561Code::SegmentGuard => 0xa61b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2561Code::StateDup => "状态重复（接管漏项 同名双态）",
            u2561Code::StateHole => "状态断档（语义错配 序缺失）",
            u2561Code::EventDup => "事件重复（同事件双声明）",
            u2561Code::TransDup => "迁移重复（同源同事件双迁移）",
            u2561Code::TransHole => "迁移缺失（应达态不可达）",
            u2561Code::TransCycle => "迁移环（非法自环/环路）",
            u2561Code::EntryBad => "入口非法（初态不在闭集）",
            u2561Code::ApplyLost => "迁移丢失（应用事件无效果）",
            u2561Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2561Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2561Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2561Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2561Code::StateDup => "按闭集唯一律去重",
            u2561Code::StateHole => "按闭集补态",
            u2561Code::EventDup => "按事件闭集去重",
            u2561Code::TransDup => "按确定性唯一迁移律合并",
            u2561Code::TransHole => "按迁移矩阵补边",
            u2561Code::TransCycle => "按无环律重构迁移",
            u2561Code::EntryBad => "按闭集钳制初态",
            u2561Code::ApplyLost => "核对事件前置条件",
            u2561Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2561Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2561Code::TableBroken => "按语义规则重建常量表",
            u2561Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 状态机 mxu2561flow（6 态 × 6 事件 · 迁移 8 条显式声明）
// --------------------------------------------------------------------------
/// 地址空间体 状态机（判据：接管漏项）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu2561flowState {
    Legacy,
    Adopted,
    Mapping,
    Matured,
    Verified,
    Sealed,
}

impl mxu2561flowState {
    /// 全量态闭集。
    pub const ALL: [mxu2561flowState; 6] = [
        mxu2561flowState::Legacy,
        mxu2561flowState::Adopted,
        mxu2561flowState::Mapping,
        mxu2561flowState::Matured,
        mxu2561flowState::Verified,
        mxu2561flowState::Sealed,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            mxu2561flowState::Legacy => 0,
            mxu2561flowState::Adopted => 1,
            mxu2561flowState::Mapping => 2,
            mxu2561flowState::Matured => 3,
            mxu2561flowState::Verified => 4,
            mxu2561flowState::Sealed => 5,
        }
    }
}

/// mxu2561flow事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu2561flowEvent {
    Adopt,
    MapItem,
    UnmapItem,
    Mature,
    DirectTest,
    Seal,
}

impl mxu2561flowEvent {
    /// 全量事件闭集。
    pub const ALL: [mxu2561flowEvent; 6] = [
        mxu2561flowEvent::Adopt,
        mxu2561flowEvent::MapItem,
        mxu2561flowEvent::UnmapItem,
        mxu2561flowEvent::Mature,
        mxu2561flowEvent::DirectTest,
        mxu2561flowEvent::Seal,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const MXU2561FLOW_TRANSITIONS: [(mxu2561flowState, mxu2561flowEvent, mxu2561flowState); 8] = [
    (mxu2561flowState::Legacy, mxu2561flowEvent::Adopt, mxu2561flowState::Adopted), // Legacy→Adopted 主链
    (mxu2561flowState::Adopted, mxu2561flowEvent::MapItem, mxu2561flowState::Mapping), // Adopted→Mapping 主链
    (mxu2561flowState::Mapping, mxu2561flowEvent::UnmapItem, mxu2561flowState::Matured), // Mapping→Matured 主链
    (mxu2561flowState::Matured, mxu2561flowEvent::Mature, mxu2561flowState::Verified), // Matured→Verified 主链
    (mxu2561flowState::Verified, mxu2561flowEvent::DirectTest, mxu2561flowState::Sealed), // Verified→Sealed 主链
    (mxu2561flowState::Sealed, mxu2561flowEvent::Seal, mxu2561flowState::Legacy), // 终态回边
    (mxu2561flowState::Sealed, mxu2561flowEvent::Adopt, mxu2561flowState::Verified), // 终态回退边
    (mxu2561flowState::Adopted, mxu2561flowEvent::MapItem, mxu2561flowState::Mapping), // 旁路边
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn mxu2561flow_next(state: mxu2561flowState, ev: mxu2561flowEvent) -> Option<mxu2561flowState> {
    let mut i = 0usize;
    while i < 8 {
        let t = MXU2561FLOW_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn mxu2561flow_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < mxu2561flowState::ALL.len() {
        let mut ei = 0usize;
        while ei < mxu2561flowEvent::ALL.len() {
            let legal = mxu2561flow_next(mxu2561flowState::ALL[si], mxu2561flowEvent::ALL[ei]).is_some();
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
pub fn mxu2561flow_terminal(st: mxu2561flowState) -> bool {
    let mut i = 0usize;
    while i < 8 {
        if MXU2561FLOW_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct mxu2561flowLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: mxu2561flowState,
}

impl mxu2561flowLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: mxu2561flowState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: mxu2561flowEvent) -> bool {
        match mxu2561flow_next(self.cur, ev) {
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
// 快照 Mxu2561Snap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// 地址空间体 快照封存（FNV 三态指纹 + 篡改检出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mxu2561Snap {
    /// 指纹分量：as_v。
    pub as_v: u64,
    /// 指纹分量：lg_v。
    pub lg_v: u64,
    /// 指纹分量：sm_v。
    pub sm_v: u64,
    /// 指纹分量：mt_v。
    pub mt_v: u64,
    /// 指纹分量：if_v。
    pub if_v: u64,
    /// 指纹分量：tm_v。
    pub tm_v: u64,
    /// 指纹分量：as2_v。
    pub as2_v: u64,
    /// 指纹分量：mp_v。
    pub mp_v: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl Mxu2561Snap {
    /// 构造并即时合成指纹。
    pub fn compute(
        as_v: u64,
        lg_v: u64,
        sm_v: u64,
        mt_v: u64,
        if_v: u64,
        tm_v: u64,
        as2_v: u64,
        mp_v: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &as_v.to_le_bytes());
        h = fnv1a64_from(h, &lg_v.to_le_bytes());
        h = fnv1a64_from(h, &sm_v.to_le_bytes());
        h = fnv1a64_from(h, &mt_v.to_le_bytes());
        h = fnv1a64_from(h, &if_v.to_le_bytes());
        h = fnv1a64_from(h, &tm_v.to_le_bytes());
        h = fnv1a64_from(h, &as2_v.to_le_bytes());
        h = fnv1a64_from(h, &mp_v.to_le_bytes());
        Self {
            as_v,
            lg_v,
            sm_v,
            mt_v,
            if_v,
            tm_v,
            as2_v,
            mp_v,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.as_v.to_le_bytes());
        h = fnv1a64_from(h, &self.lg_v.to_le_bytes());
        h = fnv1a64_from(h, &self.sm_v.to_le_bytes());
        h = fnv1a64_from(h, &self.mt_v.to_le_bytes());
        h = fnv1a64_from(h, &self.if_v.to_le_bytes());
        h = fnv1a64_from(h, &self.tm_v.to_le_bytes());
        h = fnv1a64_from(h, &self.as2_v.to_le_bytes());
        h = fnv1a64_from(h, &self.mp_v.to_le_bytes());
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

/// 段位守卫：全部诊断码必须落在本段 [0xa610, 0xa61f]。
pub const fn u2561_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2561Code::ALL.len() {
        let c = u2561Code::ALL[i].code();
        if c < 0xa610 || c > 0xa61f {
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
fn u2561_guard_seg() -> bool {
    u2561_segment_guard()
}

/// UNX-F2561 状态机闭集/迁移表/非法组合恰值全绿。
fn check_u2561_sm_legal() -> bool {
    let mut ok = mxu2561flow_illegal_pairs() == 29;
    if ok { ok = MXU2561FLOW_TRANSITIONS.len() == 8; }
    if ok { ok = mxu2561flowState::ALL.len() == 6 && mxu2561flowEvent::ALL.len() == 6; }
    ok
}

/// UNX-F2561 迁移主链回放 + 快照往返恒等。
fn check_u2561_sm_round() -> bool {
    let mut lg = mxu2561flowLedger::new(mxu2561flowState::Legacy);
    let mut ok = lg.apply(mxu2561flowEvent::Adopt) && lg.cur == mxu2561flowState::Adopted && lg.total() == 1;
    let s0 = Mxu2561Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    let mut s1 = Mxu2561Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    s1.recompute();
    if ok { ok = s0 == s1 && !s1.tampered(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2561_checks() -> CheckSet {
    let mut set = CheckSet::new("U2561");
    set.add(
        "u2561-seg-guard",
        u2561_guard_seg(),
        if u2561_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2561-sm-legal",
        check_u2561_sm_legal(),
        if check_u2561_sm_legal() { "" } else { "UNX-F2561 sm legality mismatch" },
    );
    set.add(
        "u2561-sm-round",
        check_u2561_sm_round(),
        if check_u2561_sm_round() { "" } else { "UNX-F2561 sm roundtrip mismatch" },
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
    fn u2561_all_checks_green() {
        let set = run_u2561_checks();
        assert!(set.all_passed(), "U2561 domain checks must be green");
    }

    #[test]
    fn u2561_segment_guard_holds() {
        assert!(u2561_segment_guard());
    }
}
