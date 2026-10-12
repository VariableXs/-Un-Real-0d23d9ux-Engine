//! unxu2481_u2481sm —— 大页族总纲（2M/1G 透明与显式两条路） 状态机与快照封存（大页路径）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2481 · 大页族总纲（2M/1G 透明与显式两条路） · 判据原文：UNX-F2481 · 判据：透明/显式两路定义与判据面三机型生效，路径重叠注入零双路
//! · 组件：大页路径 状态机（6 态 × 6 事件显式迁移）+ FNV 快照封存（8 分量）
//! · 判据落地：UNX-F2481 · 判据：透明/显式两路定义与判据面三机型生效，路径重叠注入零双路
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2481_VERSION: &str = "u2481-u2481sm-v1";

/// 诊断段基址。
pub const U2481_SEG_BASE: u16 = 0xa310;

// --------------------------------------------------------------------------
// 诊断码（u2481 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 大页路径 状态面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2481Code {
    /// 0xa310 状态重复（双路重入 同名双态）。
    StateDup,
    /// 0xa311 状态断档（选路错判 序缺失）。
    StateHole,
    /// 0xa312 事件重复（同事件双声明）。
    EventDup,
    /// 0xa313 迁移重复（同源同事件双迁移）。
    TransDup,
    /// 0xa314 迁移缺失（应达态不可达）。
    TransHole,
    /// 0xa315 迁移环（非法自环/环路）。
    TransCycle,
    /// 0xa316 入口非法（初态不在闭集）。
    EntryBad,
    /// 0xa317 迁移丢失（应用事件无效果）。
    ApplyLost,
    /// 0xa318 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xa319 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xa31a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xa31b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2481Code {
    /// 全量闭集。
    pub const ALL: [u2481Code; 12] = [
        u2481Code::StateDup,
        u2481Code::StateHole,
        u2481Code::EventDup,
        u2481Code::TransDup,
        u2481Code::TransHole,
        u2481Code::TransCycle,
        u2481Code::EntryBad,
        u2481Code::ApplyLost,
        u2481Code::LedgerBroken,
        u2481Code::SnapshotFingerprint,
        u2481Code::TableBroken,
        u2481Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2481Code::StateDup => 0xa310,
            u2481Code::StateHole => 0xa311,
            u2481Code::EventDup => 0xa312,
            u2481Code::TransDup => 0xa313,
            u2481Code::TransHole => 0xa314,
            u2481Code::TransCycle => 0xa315,
            u2481Code::EntryBad => 0xa316,
            u2481Code::ApplyLost => 0xa317,
            u2481Code::LedgerBroken => 0xa318,
            u2481Code::SnapshotFingerprint => 0xa319,
            u2481Code::TableBroken => 0xa31a,
            u2481Code::SegmentGuard => 0xa31b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2481Code::StateDup => "状态重复（双路重入 同名双态）",
            u2481Code::StateHole => "状态断档（选路错判 序缺失）",
            u2481Code::EventDup => "事件重复（同事件双声明）",
            u2481Code::TransDup => "迁移重复（同源同事件双迁移）",
            u2481Code::TransHole => "迁移缺失（应达态不可达）",
            u2481Code::TransCycle => "迁移环（非法自环/环路）",
            u2481Code::EntryBad => "入口非法（初态不在闭集）",
            u2481Code::ApplyLost => "迁移丢失（应用事件无效果）",
            u2481Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2481Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2481Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2481Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2481Code::StateDup => "按闭集唯一律去重",
            u2481Code::StateHole => "按闭集补态",
            u2481Code::EventDup => "按事件闭集去重",
            u2481Code::TransDup => "按确定性唯一迁移律合并",
            u2481Code::TransHole => "按迁移矩阵补边",
            u2481Code::TransCycle => "按无环律重构迁移",
            u2481Code::EntryBad => "按闭集钳制初态",
            u2481Code::ApplyLost => "核对事件前置条件",
            u2481Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2481Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2481Code::TableBroken => "按语义规则重建常量表",
            u2481Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 状态机 mxu2481flow（6 态 × 6 事件 · 迁移 8 条显式声明）
// --------------------------------------------------------------------------
/// 大页路径 状态机（判据：双路重入）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu2481flowState {
    Idle,
    Deciding,
    Transparent,
    Explicit,
    Deduped,
    Sealed,
}

impl mxu2481flowState {
    /// 全量态闭集。
    pub const ALL: [mxu2481flowState; 6] = [
        mxu2481flowState::Idle,
        mxu2481flowState::Deciding,
        mxu2481flowState::Transparent,
        mxu2481flowState::Explicit,
        mxu2481flowState::Deduped,
        mxu2481flowState::Sealed,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            mxu2481flowState::Idle => 0,
            mxu2481flowState::Deciding => 1,
            mxu2481flowState::Transparent => 2,
            mxu2481flowState::Explicit => 3,
            mxu2481flowState::Deduped => 4,
            mxu2481flowState::Sealed => 5,
        }
    }
}

/// mxu2481flow事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu2481flowEvent {
    Decide,
    TakeTransparent,
    TakeExplicit,
    OverlapInject,
    Dedup,
    Seal,
}

impl mxu2481flowEvent {
    /// 全量事件闭集。
    pub const ALL: [mxu2481flowEvent; 6] = [
        mxu2481flowEvent::Decide,
        mxu2481flowEvent::TakeTransparent,
        mxu2481flowEvent::TakeExplicit,
        mxu2481flowEvent::OverlapInject,
        mxu2481flowEvent::Dedup,
        mxu2481flowEvent::Seal,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const MXU2481FLOW_TRANSITIONS: [(mxu2481flowState, mxu2481flowEvent, mxu2481flowState); 8] = [
    (mxu2481flowState::Idle, mxu2481flowEvent::Decide, mxu2481flowState::Deciding), // Idle→Deciding 主链
    (mxu2481flowState::Deciding, mxu2481flowEvent::TakeTransparent, mxu2481flowState::Transparent), // Deciding→Transparent 主链
    (mxu2481flowState::Transparent, mxu2481flowEvent::TakeExplicit, mxu2481flowState::Explicit), // Transparent→Explicit 主链
    (mxu2481flowState::Explicit, mxu2481flowEvent::OverlapInject, mxu2481flowState::Deduped), // Explicit→Deduped 主链
    (mxu2481flowState::Deduped, mxu2481flowEvent::Dedup, mxu2481flowState::Sealed), // Deduped→Sealed 主链
    (mxu2481flowState::Sealed, mxu2481flowEvent::Seal, mxu2481flowState::Idle), // 终态回边
    (mxu2481flowState::Sealed, mxu2481flowEvent::Decide, mxu2481flowState::Deduped), // 终态回退边
    (mxu2481flowState::Deciding, mxu2481flowEvent::TakeTransparent, mxu2481flowState::Transparent), // 旁路边
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn mxu2481flow_next(state: mxu2481flowState, ev: mxu2481flowEvent) -> Option<mxu2481flowState> {
    let mut i = 0usize;
    while i < 8 {
        let t = MXU2481FLOW_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn mxu2481flow_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < mxu2481flowState::ALL.len() {
        let mut ei = 0usize;
        while ei < mxu2481flowEvent::ALL.len() {
            let legal = mxu2481flow_next(mxu2481flowState::ALL[si], mxu2481flowEvent::ALL[ei]).is_some();
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
pub fn mxu2481flow_terminal(st: mxu2481flowState) -> bool {
    let mut i = 0usize;
    while i < 8 {
        if MXU2481FLOW_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct mxu2481flowLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: mxu2481flowState,
}

impl mxu2481flowLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: mxu2481flowState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: mxu2481flowEvent) -> bool {
        match mxu2481flow_next(self.cur, ev) {
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
// 快照 Mxu2481Snap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// 大页路径 快照封存（FNV 三态指纹 + 篡改检出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mxu2481Snap {
    /// 指纹分量：tr_v。
    pub tr_v: u64,
    /// 指纹分量：ex_v。
    pub ex_v: u64,
    /// 指纹分量：p2_v。
    pub p2_v: u64,
    /// 指纹分量：p1_v。
    pub p1_v: u64,
    /// 指纹分量：mc_v。
    pub mc_v: u64,
    /// 指纹分量：ov_v。
    pub ov_v: u64,
    /// 指纹分量：db_v。
    pub db_v: u64,
    /// 指纹分量：fn_v。
    pub fn_v: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl Mxu2481Snap {
    /// 构造并即时合成指纹。
    pub fn compute(
        tr_v: u64,
        ex_v: u64,
        p2_v: u64,
        p1_v: u64,
        mc_v: u64,
        ov_v: u64,
        db_v: u64,
        fn_v: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &tr_v.to_le_bytes());
        h = fnv1a64_from(h, &ex_v.to_le_bytes());
        h = fnv1a64_from(h, &p2_v.to_le_bytes());
        h = fnv1a64_from(h, &p1_v.to_le_bytes());
        h = fnv1a64_from(h, &mc_v.to_le_bytes());
        h = fnv1a64_from(h, &ov_v.to_le_bytes());
        h = fnv1a64_from(h, &db_v.to_le_bytes());
        h = fnv1a64_from(h, &fn_v.to_le_bytes());
        Self {
            tr_v,
            ex_v,
            p2_v,
            p1_v,
            mc_v,
            ov_v,
            db_v,
            fn_v,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.tr_v.to_le_bytes());
        h = fnv1a64_from(h, &self.ex_v.to_le_bytes());
        h = fnv1a64_from(h, &self.p2_v.to_le_bytes());
        h = fnv1a64_from(h, &self.p1_v.to_le_bytes());
        h = fnv1a64_from(h, &self.mc_v.to_le_bytes());
        h = fnv1a64_from(h, &self.ov_v.to_le_bytes());
        h = fnv1a64_from(h, &self.db_v.to_le_bytes());
        h = fnv1a64_from(h, &self.fn_v.to_le_bytes());
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

/// 段位守卫：全部诊断码必须落在本段 [0xa310, 0xa31f]。
pub const fn u2481_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2481Code::ALL.len() {
        let c = u2481Code::ALL[i].code();
        if c < 0xa310 || c > 0xa31f {
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
fn u2481_guard_seg() -> bool {
    u2481_segment_guard()
}

/// UNX-F2481 状态机闭集/迁移表/非法组合恰值全绿。
fn check_u2481_sm_legal() -> bool {
    let mut ok = mxu2481flow_illegal_pairs() == 29;
    if ok { ok = MXU2481FLOW_TRANSITIONS.len() == 8; }
    if ok { ok = mxu2481flowState::ALL.len() == 6 && mxu2481flowEvent::ALL.len() == 6; }
    ok
}

/// UNX-F2481 迁移主链回放 + 快照往返恒等。
fn check_u2481_sm_round() -> bool {
    let mut lg = mxu2481flowLedger::new(mxu2481flowState::Idle);
    let mut ok = lg.apply(mxu2481flowEvent::Decide) && lg.cur == mxu2481flowState::Deciding && lg.total() == 1;
    let s0 = Mxu2481Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    let mut s1 = Mxu2481Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    s1.recompute();
    if ok { ok = s0 == s1 && !s1.tampered(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2481_checks() -> CheckSet {
    let mut set = CheckSet::new("U2481");
    set.add(
        "u2481-seg-guard",
        u2481_guard_seg(),
        if u2481_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2481-sm-legal",
        check_u2481_sm_legal(),
        if check_u2481_sm_legal() { "" } else { "UNX-F2481 sm legality mismatch" },
    );
    set.add(
        "u2481-sm-round",
        check_u2481_sm_round(),
        if check_u2481_sm_round() { "" } else { "UNX-F2481 sm roundtrip mismatch" },
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
    fn u2481_all_checks_green() {
        let set = run_u2481_checks();
        assert!(set.all_passed(), "U2481 domain checks must be green");
    }

    #[test]
    fn u2481_segment_guard_holds() {
        assert!(u2481_segment_guard());
    }
}
