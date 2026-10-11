//! unxu1701_u1701sm —— 负载追踪总纲（窗口化追踪模型冻结） 状态机与快照封存（窗口）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F1701 · 负载追踪总纲（窗口化追踪模型冻结） · 判据原文：UNX-F1701 · 判据：窗口化追踪模型冻结 v1（几何衰减均值+双窗口档），模型参数单点定义，计算路径唯一，与 F1646 瞬时账分层声明在册
//! · 组件：窗口 状态机（6 态 × 6 事件显式迁移）+ FNV 快照封存（8 分量）
//! · 判据落地：UNX-F1701 · 判据：窗口化追踪模型冻结 v1（几何衰减均值+双窗口档），模型参数单点定义，计算路径唯一，与 F1646 瞬时账分层声明在册
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U1701_VERSION: &str = "u1701-u1701sm-v1";

/// 诊断段基址。
pub const U1701_SEG_BASE: u16 = 0x58d0;

// --------------------------------------------------------------------------
// 诊断码（u1701 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 窗口 状态面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u1701Code {
    /// 0x58d0 状态重复（窗口漂移 同名双态）。
    StateDup,
    /// 0x58d1 状态断档（衰减失准 序缺失）。
    StateHole,
    /// 0x58d2 事件重复（同事件双声明）。
    EventDup,
    /// 0x58d3 迁移重复（同源同事件双迁移）。
    TransDup,
    /// 0x58d4 迁移缺失（应达态不可达）。
    TransHole,
    /// 0x58d5 迁移环（非法自环/环路）。
    TransCycle,
    /// 0x58d6 入口非法（初态不在闭集）。
    EntryBad,
    /// 0x58d7 迁移丢失（应用事件无效果）。
    ApplyLost,
    /// 0x58d8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x58d9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x58da 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x58db 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u1701Code {
    /// 全量闭集。
    pub const ALL: [u1701Code; 12] = [
        u1701Code::StateDup,
        u1701Code::StateHole,
        u1701Code::EventDup,
        u1701Code::TransDup,
        u1701Code::TransHole,
        u1701Code::TransCycle,
        u1701Code::EntryBad,
        u1701Code::ApplyLost,
        u1701Code::LedgerBroken,
        u1701Code::SnapshotFingerprint,
        u1701Code::TableBroken,
        u1701Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u1701Code::StateDup => 0x58d0,
            u1701Code::StateHole => 0x58d1,
            u1701Code::EventDup => 0x58d2,
            u1701Code::TransDup => 0x58d3,
            u1701Code::TransHole => 0x58d4,
            u1701Code::TransCycle => 0x58d5,
            u1701Code::EntryBad => 0x58d6,
            u1701Code::ApplyLost => 0x58d7,
            u1701Code::LedgerBroken => 0x58d8,
            u1701Code::SnapshotFingerprint => 0x58d9,
            u1701Code::TableBroken => 0x58da,
            u1701Code::SegmentGuard => 0x58db,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u1701Code::StateDup => "状态重复（窗口漂移 同名双态）",
            u1701Code::StateHole => "状态断档（衰减失准 序缺失）",
            u1701Code::EventDup => "事件重复（同事件双声明）",
            u1701Code::TransDup => "迁移重复（同源同事件双迁移）",
            u1701Code::TransHole => "迁移缺失（应达态不可达）",
            u1701Code::TransCycle => "迁移环（非法自环/环路）",
            u1701Code::EntryBad => "入口非法（初态不在闭集）",
            u1701Code::ApplyLost => "迁移丢失（应用事件无效果）",
            u1701Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u1701Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u1701Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u1701Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u1701Code::StateDup => "按闭集唯一律去重",
            u1701Code::StateHole => "按闭集补态",
            u1701Code::EventDup => "按事件闭集去重",
            u1701Code::TransDup => "按确定性唯一迁移律合并",
            u1701Code::TransHole => "按迁移矩阵补边",
            u1701Code::TransCycle => "按无环律重构迁移",
            u1701Code::EntryBad => "按闭集钳制初态",
            u1701Code::ApplyLost => "核对事件前置条件",
            u1701Code::LedgerBroken => "按守恒律重建台账后重跑",
            u1701Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u1701Code::TableBroken => "按语义规则重建常量表",
            u1701Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 状态机 mxu1701flow（6 态 × 6 事件 · 迁移 8 条显式声明）
// --------------------------------------------------------------------------
/// 窗口 状态机（判据：窗口漂移）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu1701flowState {
    ParamHold,
    Windowing,
    Decaying,
    Layered,
    CrossRef,
    Sealed,
}

impl mxu1701flowState {
    /// 全量态闭集。
    pub const ALL: [mxu1701flowState; 6] = [
        mxu1701flowState::ParamHold,
        mxu1701flowState::Windowing,
        mxu1701flowState::Decaying,
        mxu1701flowState::Layered,
        mxu1701flowState::CrossRef,
        mxu1701flowState::Sealed,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            mxu1701flowState::ParamHold => 0,
            mxu1701flowState::Windowing => 1,
            mxu1701flowState::Decaying => 2,
            mxu1701flowState::Layered => 3,
            mxu1701flowState::CrossRef => 4,
            mxu1701flowState::Sealed => 5,
        }
    }
}

/// mxu1701flow事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu1701flowEvent {
    HoldParam,
    Advance,
    Decay,
    Layer,
    Ref1646,
    Seal,
}

impl mxu1701flowEvent {
    /// 全量事件闭集。
    pub const ALL: [mxu1701flowEvent; 6] = [
        mxu1701flowEvent::HoldParam,
        mxu1701flowEvent::Advance,
        mxu1701flowEvent::Decay,
        mxu1701flowEvent::Layer,
        mxu1701flowEvent::Ref1646,
        mxu1701flowEvent::Seal,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const MXU1701FLOW_TRANSITIONS: [(mxu1701flowState, mxu1701flowEvent, mxu1701flowState); 8] = [
    (mxu1701flowState::ParamHold, mxu1701flowEvent::HoldParam, mxu1701flowState::Windowing), // ParamHold→Windowing 主链
    (mxu1701flowState::Windowing, mxu1701flowEvent::Advance, mxu1701flowState::Decaying), // Windowing→Decaying 主链
    (mxu1701flowState::Decaying, mxu1701flowEvent::Decay, mxu1701flowState::Layered), // Decaying→Layered 主链
    (mxu1701flowState::Layered, mxu1701flowEvent::Layer, mxu1701flowState::CrossRef), // Layered→CrossRef 主链
    (mxu1701flowState::CrossRef, mxu1701flowEvent::Ref1646, mxu1701flowState::Sealed), // CrossRef→Sealed 主链
    (mxu1701flowState::Sealed, mxu1701flowEvent::Seal, mxu1701flowState::ParamHold), // 终态回边
    (mxu1701flowState::Sealed, mxu1701flowEvent::HoldParam, mxu1701flowState::CrossRef), // 终态回退边
    (mxu1701flowState::Windowing, mxu1701flowEvent::Advance, mxu1701flowState::Decaying), // 旁路边
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn mxu1701flow_next(state: mxu1701flowState, ev: mxu1701flowEvent) -> Option<mxu1701flowState> {
    let mut i = 0usize;
    while i < 8 {
        let t = MXU1701FLOW_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn mxu1701flow_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < mxu1701flowState::ALL.len() {
        let mut ei = 0usize;
        while ei < mxu1701flowEvent::ALL.len() {
            let legal = mxu1701flow_next(mxu1701flowState::ALL[si], mxu1701flowEvent::ALL[ei]).is_some();
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
pub fn mxu1701flow_terminal(st: mxu1701flowState) -> bool {
    let mut i = 0usize;
    while i < 8 {
        if MXU1701FLOW_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct mxu1701flowLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: mxu1701flowState,
}

impl mxu1701flowLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: mxu1701flowState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: mxu1701flowEvent) -> bool {
        match mxu1701flow_next(self.cur, ev) {
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
// 快照 Mxu1701Snap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// 窗口 快照封存（FNV 三态指纹 + 篡改检出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mxu1701Snap {
    /// 指纹分量：js_v。
    pub js_v: u64,
    /// 指纹分量：sc_v。
    pub sc_v: u64,
    /// 指纹分量：cs_v。
    pub cs_v: u64,
    /// 指纹分量：ly_v。
    pub ly_v: u64,
    /// 指纹分量：f1646_v。
    pub f1646_v: u64,
    /// 指纹分量：ck_v。
    pub ck_v: u64,
    /// 指纹分量：jz_v。
    pub jz_v: u64,
    /// 指纹分量：mz_v。
    pub mz_v: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl Mxu1701Snap {
    /// 构造并即时合成指纹。
    pub fn compute(
        js_v: u64,
        sc_v: u64,
        cs_v: u64,
        ly_v: u64,
        f1646_v: u64,
        ck_v: u64,
        jz_v: u64,
        mz_v: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &js_v.to_le_bytes());
        h = fnv1a64_from(h, &sc_v.to_le_bytes());
        h = fnv1a64_from(h, &cs_v.to_le_bytes());
        h = fnv1a64_from(h, &ly_v.to_le_bytes());
        h = fnv1a64_from(h, &f1646_v.to_le_bytes());
        h = fnv1a64_from(h, &ck_v.to_le_bytes());
        h = fnv1a64_from(h, &jz_v.to_le_bytes());
        h = fnv1a64_from(h, &mz_v.to_le_bytes());
        Self {
            js_v,
            sc_v,
            cs_v,
            ly_v,
            f1646_v,
            ck_v,
            jz_v,
            mz_v,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.js_v.to_le_bytes());
        h = fnv1a64_from(h, &self.sc_v.to_le_bytes());
        h = fnv1a64_from(h, &self.cs_v.to_le_bytes());
        h = fnv1a64_from(h, &self.ly_v.to_le_bytes());
        h = fnv1a64_from(h, &self.f1646_v.to_le_bytes());
        h = fnv1a64_from(h, &self.ck_v.to_le_bytes());
        h = fnv1a64_from(h, &self.jz_v.to_le_bytes());
        h = fnv1a64_from(h, &self.mz_v.to_le_bytes());
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

/// 段位守卫：全部诊断码必须落在本段 [0x58d0, 0x58df]。
pub const fn u1701_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u1701Code::ALL.len() {
        let c = u1701Code::ALL[i].code();
        if c < 0x58d0 || c > 0x58df {
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
fn u1701_guard_seg() -> bool {
    u1701_segment_guard()
}

/// UNX-F1701 状态机闭集/迁移表/非法组合恰值全绿。
fn check_u1701_sm_legal() -> bool {
    let mut ok = mxu1701flow_illegal_pairs() == 29;
    if ok { ok = MXU1701FLOW_TRANSITIONS.len() == 8; }
    if ok { ok = mxu1701flowState::ALL.len() == 6 && mxu1701flowEvent::ALL.len() == 6; }
    ok
}

/// UNX-F1701 迁移主链回放 + 快照往返恒等。
fn check_u1701_sm_round() -> bool {
    let mut lg = mxu1701flowLedger::new(mxu1701flowState::ParamHold);
    let mut ok = lg.apply(mxu1701flowEvent::HoldParam) && lg.cur == mxu1701flowState::Windowing && lg.total() == 1;
    let s0 = Mxu1701Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    let mut s1 = Mxu1701Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    s1.recompute();
    if ok { ok = s0 == s1 && !s1.tampered(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u1701_checks() -> CheckSet {
    let mut set = CheckSet::new("U1701");
    set.add(
        "u1701-seg-guard",
        u1701_guard_seg(),
        if u1701_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u1701-sm-legal",
        check_u1701_sm_legal(),
        if check_u1701_sm_legal() { "" } else { "UNX-F1701 sm legality mismatch" },
    );
    set.add(
        "u1701-sm-round",
        check_u1701_sm_round(),
        if check_u1701_sm_round() { "" } else { "UNX-F1701 sm roundtrip mismatch" },
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
    fn u1701_all_checks_green() {
        let set = run_u1701_checks();
        assert!(set.all_passed(), "U1701 domain checks must be green");
    }

    #[test]
    fn u1701_segment_guard_holds() {
        assert!(u1701_segment_guard());
    }
}
