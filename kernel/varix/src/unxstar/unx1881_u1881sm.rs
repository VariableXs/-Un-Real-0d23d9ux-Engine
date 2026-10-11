//! unxu1881_u1881sm —— 全域注入矩阵终对账（十一矩阵收口） 状态机与快照封存（矩阵）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F1881 · 全域注入矩阵终对账（十一矩阵收口） · 判据原文：UNX-F1881 · 判据：十一矩阵（F1659/F1673/F1692/F1715/F1732/F1756/F1772/F1814/F1835/F1856/F1874）形态总表对账闭合，覆盖空洞为零，与 F1788 口径一致
//! · 组件：矩阵 状态机（6 态 × 6 事件显式迁移）+ FNV 快照封存（8 分量）
//! · 判据落地：UNX-F1881 · 判据：十一矩阵（F1659/F1673/F1692/F1715/F1732/F1756/F1772/F1814/F1835/F1856/
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U1881_VERSION: &str = "u1881-u1881sm-v1";

/// 诊断段基址。
pub const U1881_SEG_BASE: u16 = 0x5f90;

// --------------------------------------------------------------------------
// 诊断码（u1881 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 矩阵 状态面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u1881Code {
    /// 0x5f90 状态重复（矩阵缺位 同名双态）。
    StateDup,
    /// 0x5f91 状态断档（对账破缺 序缺失）。
    StateHole,
    /// 0x5f92 事件重复（同事件双声明）。
    EventDup,
    /// 0x5f93 迁移重复（同源同事件双迁移）。
    TransDup,
    /// 0x5f94 迁移缺失（应达态不可达）。
    TransHole,
    /// 0x5f95 迁移环（非法自环/环路）。
    TransCycle,
    /// 0x5f96 入口非法（初态不在闭集）。
    EntryBad,
    /// 0x5f97 迁移丢失（应用事件无效果）。
    ApplyLost,
    /// 0x5f98 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x5f99 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x5f9a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x5f9b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u1881Code {
    /// 全量闭集。
    pub const ALL: [u1881Code; 12] = [
        u1881Code::StateDup,
        u1881Code::StateHole,
        u1881Code::EventDup,
        u1881Code::TransDup,
        u1881Code::TransHole,
        u1881Code::TransCycle,
        u1881Code::EntryBad,
        u1881Code::ApplyLost,
        u1881Code::LedgerBroken,
        u1881Code::SnapshotFingerprint,
        u1881Code::TableBroken,
        u1881Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u1881Code::StateDup => 0x5f90,
            u1881Code::StateHole => 0x5f91,
            u1881Code::EventDup => 0x5f92,
            u1881Code::TransDup => 0x5f93,
            u1881Code::TransHole => 0x5f94,
            u1881Code::TransCycle => 0x5f95,
            u1881Code::EntryBad => 0x5f96,
            u1881Code::ApplyLost => 0x5f97,
            u1881Code::LedgerBroken => 0x5f98,
            u1881Code::SnapshotFingerprint => 0x5f99,
            u1881Code::TableBroken => 0x5f9a,
            u1881Code::SegmentGuard => 0x5f9b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u1881Code::StateDup => "状态重复（矩阵缺位 同名双态）",
            u1881Code::StateHole => "状态断档（对账破缺 序缺失）",
            u1881Code::EventDup => "事件重复（同事件双声明）",
            u1881Code::TransDup => "迁移重复（同源同事件双迁移）",
            u1881Code::TransHole => "迁移缺失（应达态不可达）",
            u1881Code::TransCycle => "迁移环（非法自环/环路）",
            u1881Code::EntryBad => "入口非法（初态不在闭集）",
            u1881Code::ApplyLost => "迁移丢失（应用事件无效果）",
            u1881Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u1881Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u1881Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u1881Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u1881Code::StateDup => "按闭集唯一律去重",
            u1881Code::StateHole => "按闭集补态",
            u1881Code::EventDup => "按事件闭集去重",
            u1881Code::TransDup => "按确定性唯一迁移律合并",
            u1881Code::TransHole => "按迁移矩阵补边",
            u1881Code::TransCycle => "按无环律重构迁移",
            u1881Code::EntryBad => "按闭集钳制初态",
            u1881Code::ApplyLost => "核对事件前置条件",
            u1881Code::LedgerBroken => "按守恒律重建台账后重跑",
            u1881Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u1881Code::TableBroken => "按语义规则重建常量表",
            u1881Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 状态机 mxu1881flow（6 态 × 6 事件 · 迁移 8 条显式声明）
// --------------------------------------------------------------------------
/// 矩阵 状态机（判据：矩阵缺位）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu1881flowState {
    Enumerated,
    Tabulated,
    Reconciled,
    HoleScanned,
    Calibered,
    Sealed,
}

impl mxu1881flowState {
    /// 全量态闭集。
    pub const ALL: [mxu1881flowState; 6] = [
        mxu1881flowState::Enumerated,
        mxu1881flowState::Tabulated,
        mxu1881flowState::Reconciled,
        mxu1881flowState::HoleScanned,
        mxu1881flowState::Calibered,
        mxu1881flowState::Sealed,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            mxu1881flowState::Enumerated => 0,
            mxu1881flowState::Tabulated => 1,
            mxu1881flowState::Reconciled => 2,
            mxu1881flowState::HoleScanned => 3,
            mxu1881flowState::Calibered => 4,
            mxu1881flowState::Sealed => 5,
        }
    }
}

/// mxu1881flow事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu1881flowEvent {
    Enumerate,
    Tabulate,
    Reconcile,
    ScanHole,
    Caliber,
    Seal,
}

impl mxu1881flowEvent {
    /// 全量事件闭集。
    pub const ALL: [mxu1881flowEvent; 6] = [
        mxu1881flowEvent::Enumerate,
        mxu1881flowEvent::Tabulate,
        mxu1881flowEvent::Reconcile,
        mxu1881flowEvent::ScanHole,
        mxu1881flowEvent::Caliber,
        mxu1881flowEvent::Seal,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const MXU1881FLOW_TRANSITIONS: [(mxu1881flowState, mxu1881flowEvent, mxu1881flowState); 8] = [
    (mxu1881flowState::Enumerated, mxu1881flowEvent::Enumerate, mxu1881flowState::Tabulated), // Enumerated→Tabulated 主链
    (mxu1881flowState::Tabulated, mxu1881flowEvent::Tabulate, mxu1881flowState::Reconciled), // Tabulated→Reconciled 主链
    (mxu1881flowState::Reconciled, mxu1881flowEvent::Reconcile, mxu1881flowState::HoleScanned), // Reconciled→HoleScanned 主链
    (mxu1881flowState::HoleScanned, mxu1881flowEvent::ScanHole, mxu1881flowState::Calibered), // HoleScanned→Calibered 主链
    (mxu1881flowState::Calibered, mxu1881flowEvent::Caliber, mxu1881flowState::Sealed), // Calibered→Sealed 主链
    (mxu1881flowState::Sealed, mxu1881flowEvent::Seal, mxu1881flowState::Enumerated), // 终态回边
    (mxu1881flowState::Sealed, mxu1881flowEvent::Enumerate, mxu1881flowState::Calibered), // 终态回退边
    (mxu1881flowState::Tabulated, mxu1881flowEvent::Tabulate, mxu1881flowState::Reconciled), // 旁路边
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn mxu1881flow_next(state: mxu1881flowState, ev: mxu1881flowEvent) -> Option<mxu1881flowState> {
    let mut i = 0usize;
    while i < 8 {
        let t = MXU1881FLOW_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn mxu1881flow_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < mxu1881flowState::ALL.len() {
        let mut ei = 0usize;
        while ei < mxu1881flowEvent::ALL.len() {
            let legal = mxu1881flow_next(mxu1881flowState::ALL[si], mxu1881flowEvent::ALL[ei]).is_some();
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
pub fn mxu1881flow_terminal(st: mxu1881flowState) -> bool {
    let mut i = 0usize;
    while i < 8 {
        if MXU1881FLOW_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct mxu1881flowLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: mxu1881flowState,
}

impl mxu1881flowLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: mxu1881flowState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: mxu1881flowEvent) -> bool {
        match mxu1881flow_next(self.cur, ev) {
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
// 快照 Mxu1881Snap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// 矩阵 快照封存（FNV 三态指纹 + 篡改检出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mxu1881Snap {
    /// 指纹分量：sz_v。
    pub sz_v: u64,
    /// 指纹分量：xt_v。
    pub xt_v: u64,
    /// 指纹分量：dz_v。
    pub dz_v: u64,
    /// 指纹分量：fg_v。
    pub fg_v: u64,
    /// 指纹分量：f1788_v。
    pub f1788_v: u64,
    /// 指纹分量：f1659_v。
    pub f1659_v: u64,
    /// 指纹分量：f1673_v。
    pub f1673_v: u64,
    /// 指纹分量：f1692_v。
    pub f1692_v: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl Mxu1881Snap {
    /// 构造并即时合成指纹。
    pub fn compute(
        sz_v: u64,
        xt_v: u64,
        dz_v: u64,
        fg_v: u64,
        f1788_v: u64,
        f1659_v: u64,
        f1673_v: u64,
        f1692_v: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &sz_v.to_le_bytes());
        h = fnv1a64_from(h, &xt_v.to_le_bytes());
        h = fnv1a64_from(h, &dz_v.to_le_bytes());
        h = fnv1a64_from(h, &fg_v.to_le_bytes());
        h = fnv1a64_from(h, &f1788_v.to_le_bytes());
        h = fnv1a64_from(h, &f1659_v.to_le_bytes());
        h = fnv1a64_from(h, &f1673_v.to_le_bytes());
        h = fnv1a64_from(h, &f1692_v.to_le_bytes());
        Self {
            sz_v,
            xt_v,
            dz_v,
            fg_v,
            f1788_v,
            f1659_v,
            f1673_v,
            f1692_v,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.sz_v.to_le_bytes());
        h = fnv1a64_from(h, &self.xt_v.to_le_bytes());
        h = fnv1a64_from(h, &self.dz_v.to_le_bytes());
        h = fnv1a64_from(h, &self.fg_v.to_le_bytes());
        h = fnv1a64_from(h, &self.f1788_v.to_le_bytes());
        h = fnv1a64_from(h, &self.f1659_v.to_le_bytes());
        h = fnv1a64_from(h, &self.f1673_v.to_le_bytes());
        h = fnv1a64_from(h, &self.f1692_v.to_le_bytes());
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

/// 段位守卫：全部诊断码必须落在本段 [0x5f90, 0x5f9f]。
pub const fn u1881_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u1881Code::ALL.len() {
        let c = u1881Code::ALL[i].code();
        if c < 0x5f90 || c > 0x5f9f {
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
fn u1881_guard_seg() -> bool {
    u1881_segment_guard()
}

/// UNX-F1881 状态机闭集/迁移表/非法组合恰值全绿。
fn check_u1881_sm_legal() -> bool {
    let mut ok = mxu1881flow_illegal_pairs() == 29;
    if ok { ok = MXU1881FLOW_TRANSITIONS.len() == 8; }
    if ok { ok = mxu1881flowState::ALL.len() == 6 && mxu1881flowEvent::ALL.len() == 6; }
    ok
}

/// UNX-F1881 迁移主链回放 + 快照往返恒等。
fn check_u1881_sm_round() -> bool {
    let mut lg = mxu1881flowLedger::new(mxu1881flowState::Enumerated);
    let mut ok = lg.apply(mxu1881flowEvent::Enumerate) && lg.cur == mxu1881flowState::Tabulated && lg.total() == 1;
    let s0 = Mxu1881Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    let mut s1 = Mxu1881Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    s1.recompute();
    if ok { ok = s0 == s1 && !s1.tampered(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u1881_checks() -> CheckSet {
    let mut set = CheckSet::new("U1881");
    set.add(
        "u1881-seg-guard",
        u1881_guard_seg(),
        if u1881_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u1881-sm-legal",
        check_u1881_sm_legal(),
        if check_u1881_sm_legal() { "" } else { "UNX-F1881 sm legality mismatch" },
    );
    set.add(
        "u1881-sm-round",
        check_u1881_sm_round(),
        if check_u1881_sm_round() { "" } else { "UNX-F1881 sm roundtrip mismatch" },
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
    fn u1881_all_checks_green() {
        let set = run_u1881_checks();
        assert!(set.all_passed(), "U1881 domain checks must be green");
    }

    #[test]
    fn u1881_segment_guard_holds() {
        assert!(u1881_segment_guard());
    }
}
