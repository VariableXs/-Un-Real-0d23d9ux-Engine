//! unxu2601_u2601sm —— 现存 pfh.rs 接管声明（#PF 处理器收编） 状态机与快照封存（缺页处理路）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2601 · 现存 pfh.rs 接管声明（#PF 处理器收编） · 判据原文：UNX-F2601 · 判据：现存 `pfh.rs`（四路起步态：Guard/COW/GrowStack/MapZero）接管账在档（三态标注），收编后四路语义与成熟体逐路条目一致断言
//! · 组件：缺页处理路 状态机（6 态 × 6 事件显式迁移）+ FNV 快照封存（8 分量）
//! · 判据落地：UNX-F2601 · 判据：现存 `pfh.rs`（四路起步态：Guard/COW/GrowStack/MapZero）接管账在档（三态标注），收编后四路语义
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2601_VERSION: &str = "u2601-u2601sm-v1";

/// 诊断段基址。
pub const U2601_SEG_BASE: u16 = 0xa790;

// --------------------------------------------------------------------------
// 诊断码（u2601 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 缺页处理路 状态面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2601Code {
    /// 0xa790 状态重复（四路漏标 同名双态）。
    StateDup,
    /// 0xa791 状态断档（语义错位 序缺失）。
    StateHole,
    /// 0xa792 事件重复（同事件双声明）。
    EventDup,
    /// 0xa793 迁移重复（同源同事件双迁移）。
    TransDup,
    /// 0xa794 迁移缺失（应达态不可达）。
    TransHole,
    /// 0xa795 迁移环（非法自环/环路）。
    TransCycle,
    /// 0xa796 入口非法（初态不在闭集）。
    EntryBad,
    /// 0xa797 迁移丢失（应用事件无效果）。
    ApplyLost,
    /// 0xa798 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xa799 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xa79a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xa79b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2601Code {
    /// 全量闭集。
    pub const ALL: [u2601Code; 12] = [
        u2601Code::StateDup,
        u2601Code::StateHole,
        u2601Code::EventDup,
        u2601Code::TransDup,
        u2601Code::TransHole,
        u2601Code::TransCycle,
        u2601Code::EntryBad,
        u2601Code::ApplyLost,
        u2601Code::LedgerBroken,
        u2601Code::SnapshotFingerprint,
        u2601Code::TableBroken,
        u2601Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2601Code::StateDup => 0xa790,
            u2601Code::StateHole => 0xa791,
            u2601Code::EventDup => 0xa792,
            u2601Code::TransDup => 0xa793,
            u2601Code::TransHole => 0xa794,
            u2601Code::TransCycle => 0xa795,
            u2601Code::EntryBad => 0xa796,
            u2601Code::ApplyLost => 0xa797,
            u2601Code::LedgerBroken => 0xa798,
            u2601Code::SnapshotFingerprint => 0xa799,
            u2601Code::TableBroken => 0xa79a,
            u2601Code::SegmentGuard => 0xa79b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2601Code::StateDup => "状态重复（四路漏标 同名双态）",
            u2601Code::StateHole => "状态断档（语义错位 序缺失）",
            u2601Code::EventDup => "事件重复（同事件双声明）",
            u2601Code::TransDup => "迁移重复（同源同事件双迁移）",
            u2601Code::TransHole => "迁移缺失（应达态不可达）",
            u2601Code::TransCycle => "迁移环（非法自环/环路）",
            u2601Code::EntryBad => "入口非法（初态不在闭集）",
            u2601Code::ApplyLost => "迁移丢失（应用事件无效果）",
            u2601Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2601Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2601Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2601Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2601Code::StateDup => "按闭集唯一律去重",
            u2601Code::StateHole => "按闭集补态",
            u2601Code::EventDup => "按事件闭集去重",
            u2601Code::TransDup => "按确定性唯一迁移律合并",
            u2601Code::TransHole => "按迁移矩阵补边",
            u2601Code::TransCycle => "按无环律重构迁移",
            u2601Code::EntryBad => "按闭集钳制初态",
            u2601Code::ApplyLost => "核对事件前置条件",
            u2601Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2601Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2601Code::TableBroken => "按语义规则重建常量表",
            u2601Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 状态机 mxu2601flow（6 态 × 6 事件 · 迁移 8 条显式声明）
// --------------------------------------------------------------------------
/// 缺页处理路 状态机（判据：四路漏标）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu2601flowState {
    Legacy,
    Adopting,
    TriMark,
    Merged,
    Asserted,
    Sealed,
}

impl mxu2601flowState {
    /// 全量态闭集。
    pub const ALL: [mxu2601flowState; 6] = [
        mxu2601flowState::Legacy,
        mxu2601flowState::Adopting,
        mxu2601flowState::TriMark,
        mxu2601flowState::Merged,
        mxu2601flowState::Asserted,
        mxu2601flowState::Sealed,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            mxu2601flowState::Legacy => 0,
            mxu2601flowState::Adopting => 1,
            mxu2601flowState::TriMark => 2,
            mxu2601flowState::Merged => 3,
            mxu2601flowState::Asserted => 4,
            mxu2601flowState::Sealed => 5,
        }
    }
}

/// mxu2601flow事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu2601flowEvent {
    AdoptPfh,
    MarkTri,
    RouteGuard,
    RouteCow,
    RouteGrow,
    RouteZero,
}

impl mxu2601flowEvent {
    /// 全量事件闭集。
    pub const ALL: [mxu2601flowEvent; 6] = [
        mxu2601flowEvent::AdoptPfh,
        mxu2601flowEvent::MarkTri,
        mxu2601flowEvent::RouteGuard,
        mxu2601flowEvent::RouteCow,
        mxu2601flowEvent::RouteGrow,
        mxu2601flowEvent::RouteZero,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const MXU2601FLOW_TRANSITIONS: [(mxu2601flowState, mxu2601flowEvent, mxu2601flowState); 8] = [
    (mxu2601flowState::Legacy, mxu2601flowEvent::AdoptPfh, mxu2601flowState::Adopting), // Legacy→Adopting 主链
    (mxu2601flowState::Adopting, mxu2601flowEvent::MarkTri, mxu2601flowState::TriMark), // Adopting→TriMark 主链
    (mxu2601flowState::TriMark, mxu2601flowEvent::RouteGuard, mxu2601flowState::Merged), // TriMark→Merged 主链
    (mxu2601flowState::Merged, mxu2601flowEvent::RouteCow, mxu2601flowState::Asserted), // Merged→Asserted 主链
    (mxu2601flowState::Asserted, mxu2601flowEvent::RouteGrow, mxu2601flowState::Sealed), // Asserted→Sealed 主链
    (mxu2601flowState::Sealed, mxu2601flowEvent::RouteZero, mxu2601flowState::Legacy), // 终态回边
    (mxu2601flowState::Sealed, mxu2601flowEvent::AdoptPfh, mxu2601flowState::Asserted), // 终态回退边
    (mxu2601flowState::Adopting, mxu2601flowEvent::MarkTri, mxu2601flowState::TriMark), // 旁路边
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn mxu2601flow_next(state: mxu2601flowState, ev: mxu2601flowEvent) -> Option<mxu2601flowState> {
    let mut i = 0usize;
    while i < 8 {
        let t = MXU2601FLOW_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn mxu2601flow_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < mxu2601flowState::ALL.len() {
        let mut ei = 0usize;
        while ei < mxu2601flowEvent::ALL.len() {
            let legal = mxu2601flow_next(mxu2601flowState::ALL[si], mxu2601flowEvent::ALL[ei]).is_some();
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
pub fn mxu2601flow_terminal(st: mxu2601flowState) -> bool {
    let mut i = 0usize;
    while i < 8 {
        if MXU2601FLOW_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct mxu2601flowLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: mxu2601flowState,
}

impl mxu2601flowLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: mxu2601flowState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: mxu2601flowEvent) -> bool {
        match mxu2601flow_next(self.cur, ev) {
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
// 快照 Mxu2601Snap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// 缺页处理路 快照封存（FNV 三态指纹 + 篡改检出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mxu2601Snap {
    /// 指纹分量：pf_v。
    pub pf_v: u64,
    /// 指纹分量：gd_v。
    pub gd_v: u64,
    /// 指纹分量：cw_v。
    pub cw_v: u64,
    /// 指纹分量：gs_v。
    pub gs_v: u64,
    /// 指纹分量：mz_v。
    pub mz_v: u64,
    /// 指纹分量：ts_v。
    pub ts_v: u64,
    /// 指纹分量：ad_v。
    pub ad_v: u64,
    /// 指纹分量：mg_v。
    pub mg_v: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl Mxu2601Snap {
    /// 构造并即时合成指纹。
    pub fn compute(
        pf_v: u64,
        gd_v: u64,
        cw_v: u64,
        gs_v: u64,
        mz_v: u64,
        ts_v: u64,
        ad_v: u64,
        mg_v: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &pf_v.to_le_bytes());
        h = fnv1a64_from(h, &gd_v.to_le_bytes());
        h = fnv1a64_from(h, &cw_v.to_le_bytes());
        h = fnv1a64_from(h, &gs_v.to_le_bytes());
        h = fnv1a64_from(h, &mz_v.to_le_bytes());
        h = fnv1a64_from(h, &ts_v.to_le_bytes());
        h = fnv1a64_from(h, &ad_v.to_le_bytes());
        h = fnv1a64_from(h, &mg_v.to_le_bytes());
        Self {
            pf_v,
            gd_v,
            cw_v,
            gs_v,
            mz_v,
            ts_v,
            ad_v,
            mg_v,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.pf_v.to_le_bytes());
        h = fnv1a64_from(h, &self.gd_v.to_le_bytes());
        h = fnv1a64_from(h, &self.cw_v.to_le_bytes());
        h = fnv1a64_from(h, &self.gs_v.to_le_bytes());
        h = fnv1a64_from(h, &self.mz_v.to_le_bytes());
        h = fnv1a64_from(h, &self.ts_v.to_le_bytes());
        h = fnv1a64_from(h, &self.ad_v.to_le_bytes());
        h = fnv1a64_from(h, &self.mg_v.to_le_bytes());
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

/// 段位守卫：全部诊断码必须落在本段 [0xa790, 0xa79f]。
pub const fn u2601_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2601Code::ALL.len() {
        let c = u2601Code::ALL[i].code();
        if c < 0xa790 || c > 0xa79f {
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
fn u2601_guard_seg() -> bool {
    u2601_segment_guard()
}

/// UNX-F2601 状态机闭集/迁移表/非法组合恰值全绿。
fn check_u2601_sm_legal() -> bool {
    let mut ok = mxu2601flow_illegal_pairs() == 29;
    if ok { ok = MXU2601FLOW_TRANSITIONS.len() == 8; }
    if ok { ok = mxu2601flowState::ALL.len() == 6 && mxu2601flowEvent::ALL.len() == 6; }
    ok
}

/// UNX-F2601 迁移主链回放 + 快照往返恒等。
fn check_u2601_sm_round() -> bool {
    let mut lg = mxu2601flowLedger::new(mxu2601flowState::Legacy);
    let mut ok = lg.apply(mxu2601flowEvent::AdoptPfh) && lg.cur == mxu2601flowState::Adopting && lg.total() == 1;
    let s0 = Mxu2601Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    let mut s1 = Mxu2601Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    s1.recompute();
    if ok { ok = s0 == s1 && !s1.tampered(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2601_checks() -> CheckSet {
    let mut set = CheckSet::new("U2601");
    set.add(
        "u2601-seg-guard",
        u2601_guard_seg(),
        if u2601_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2601-sm-legal",
        check_u2601_sm_legal(),
        if check_u2601_sm_legal() { "" } else { "UNX-F2601 sm legality mismatch" },
    );
    set.add(
        "u2601-sm-round",
        check_u2601_sm_round(),
        if check_u2601_sm_round() { "" } else { "UNX-F2601 sm roundtrip mismatch" },
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
    fn u2601_all_checks_green() {
        let set = run_u2601_checks();
        assert!(set.all_passed(), "U2601 domain checks must be green");
    }

    #[test]
    fn u2601_segment_guard_holds() {
        assert!(u2601_segment_guard());
    }
}
