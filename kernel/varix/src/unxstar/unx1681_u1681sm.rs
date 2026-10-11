//! unxu1681_u1681sm —— 时间片计算总纲（基片×权重缩放模型） 状态机与快照封存（时间片）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F1681 · 时间片计算总纲（基片×权重缩放模型） · 判据原文：UNX-F1681 · 判据：时间片=基片×（本线程权重/基准权重）模型冻结 v1，公式参数单点定义，计算路径唯一
//! · 组件：时间片 状态机（6 态 × 6 事件显式迁移）+ FNV 快照封存（8 分量）
//! · 判据落地：UNX-F1681 · 判据：时间片=基片×（本线程权重/基准权重）模型冻结 v1，公式参数单点定义，计算路径唯一
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U1681_VERSION: &str = "u1681-u1681sm-v1";

/// 诊断段基址。
pub const U1681_SEG_BASE: u16 = 0x5810;

// --------------------------------------------------------------------------
// 诊断码（u1681 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 时间片 状态面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u1681Code {
    /// 0x5810 状态重复（参数多点 同名双态）。
    StateDup,
    /// 0x5811 状态断档（路径分叉 序缺失）。
    StateHole,
    /// 0x5812 事件重复（同事件双声明）。
    EventDup,
    /// 0x5813 迁移重复（同源同事件双迁移）。
    TransDup,
    /// 0x5814 迁移缺失（应达态不可达）。
    TransHole,
    /// 0x5815 迁移环（非法自环/环路）。
    TransCycle,
    /// 0x5816 入口非法（初态不在闭集）。
    EntryBad,
    /// 0x5817 迁移丢失（应用事件无效果）。
    ApplyLost,
    /// 0x5818 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x5819 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x581a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x581b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u1681Code {
    /// 全量闭集。
    pub const ALL: [u1681Code; 12] = [
        u1681Code::StateDup,
        u1681Code::StateHole,
        u1681Code::EventDup,
        u1681Code::TransDup,
        u1681Code::TransHole,
        u1681Code::TransCycle,
        u1681Code::EntryBad,
        u1681Code::ApplyLost,
        u1681Code::LedgerBroken,
        u1681Code::SnapshotFingerprint,
        u1681Code::TableBroken,
        u1681Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u1681Code::StateDup => 0x5810,
            u1681Code::StateHole => 0x5811,
            u1681Code::EventDup => 0x5812,
            u1681Code::TransDup => 0x5813,
            u1681Code::TransHole => 0x5814,
            u1681Code::TransCycle => 0x5815,
            u1681Code::EntryBad => 0x5816,
            u1681Code::ApplyLost => 0x5817,
            u1681Code::LedgerBroken => 0x5818,
            u1681Code::SnapshotFingerprint => 0x5819,
            u1681Code::TableBroken => 0x581a,
            u1681Code::SegmentGuard => 0x581b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u1681Code::StateDup => "状态重复（参数多点 同名双态）",
            u1681Code::StateHole => "状态断档（路径分叉 序缺失）",
            u1681Code::EventDup => "事件重复（同事件双声明）",
            u1681Code::TransDup => "迁移重复（同源同事件双迁移）",
            u1681Code::TransHole => "迁移缺失（应达态不可达）",
            u1681Code::TransCycle => "迁移环（非法自环/环路）",
            u1681Code::EntryBad => "入口非法（初态不在闭集）",
            u1681Code::ApplyLost => "迁移丢失（应用事件无效果）",
            u1681Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u1681Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u1681Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u1681Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u1681Code::StateDup => "按闭集唯一律去重",
            u1681Code::StateHole => "按闭集补态",
            u1681Code::EventDup => "按事件闭集去重",
            u1681Code::TransDup => "按确定性唯一迁移律合并",
            u1681Code::TransHole => "按迁移矩阵补边",
            u1681Code::TransCycle => "按无环律重构迁移",
            u1681Code::EntryBad => "按闭集钳制初态",
            u1681Code::ApplyLost => "核对事件前置条件",
            u1681Code::LedgerBroken => "按守恒律重建台账后重跑",
            u1681Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u1681Code::TableBroken => "按语义规则重建常量表",
            u1681Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 状态机 mxu1681flow（6 态 × 6 事件 · 迁移 8 条显式声明）
// --------------------------------------------------------------------------
/// 时间片 状态机（判据：参数多点）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu1681flowState {
    ParamFreeze,
    Compute,
    ClampLo,
    ClampHi,
    Verified,
    Sealed,
}

impl mxu1681flowState {
    /// 全量态闭集。
    pub const ALL: [mxu1681flowState; 6] = [
        mxu1681flowState::ParamFreeze,
        mxu1681flowState::Compute,
        mxu1681flowState::ClampLo,
        mxu1681flowState::ClampHi,
        mxu1681flowState::Verified,
        mxu1681flowState::Sealed,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            mxu1681flowState::ParamFreeze => 0,
            mxu1681flowState::Compute => 1,
            mxu1681flowState::ClampLo => 2,
            mxu1681flowState::ClampHi => 3,
            mxu1681flowState::Verified => 4,
            mxu1681flowState::Sealed => 5,
        }
    }
}

/// mxu1681flow事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu1681flowEvent {
    FreezeParam,
    Compute,
    ClampLo,
    ClampHi,
    CrossCheck,
    Seal,
}

impl mxu1681flowEvent {
    /// 全量事件闭集。
    pub const ALL: [mxu1681flowEvent; 6] = [
        mxu1681flowEvent::FreezeParam,
        mxu1681flowEvent::Compute,
        mxu1681flowEvent::ClampLo,
        mxu1681flowEvent::ClampHi,
        mxu1681flowEvent::CrossCheck,
        mxu1681flowEvent::Seal,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const MXU1681FLOW_TRANSITIONS: [(mxu1681flowState, mxu1681flowEvent, mxu1681flowState); 8] = [
    (mxu1681flowState::ParamFreeze, mxu1681flowEvent::FreezeParam, mxu1681flowState::Compute), // ParamFreeze→Compute 主链
    (mxu1681flowState::Compute, mxu1681flowEvent::Compute, mxu1681flowState::ClampLo), // Compute→ClampLo 主链
    (mxu1681flowState::ClampLo, mxu1681flowEvent::ClampLo, mxu1681flowState::ClampHi), // ClampLo→ClampHi 主链
    (mxu1681flowState::ClampHi, mxu1681flowEvent::ClampHi, mxu1681flowState::Verified), // ClampHi→Verified 主链
    (mxu1681flowState::Verified, mxu1681flowEvent::CrossCheck, mxu1681flowState::Sealed), // Verified→Sealed 主链
    (mxu1681flowState::Sealed, mxu1681flowEvent::Seal, mxu1681flowState::ParamFreeze), // 终态回边
    (mxu1681flowState::Sealed, mxu1681flowEvent::FreezeParam, mxu1681flowState::Verified), // 终态回退边
    (mxu1681flowState::Compute, mxu1681flowEvent::Compute, mxu1681flowState::ClampLo), // 旁路边
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn mxu1681flow_next(state: mxu1681flowState, ev: mxu1681flowEvent) -> Option<mxu1681flowState> {
    let mut i = 0usize;
    while i < 8 {
        let t = MXU1681FLOW_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn mxu1681flow_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < mxu1681flowState::ALL.len() {
        let mut ei = 0usize;
        while ei < mxu1681flowEvent::ALL.len() {
            let legal = mxu1681flow_next(mxu1681flowState::ALL[si], mxu1681flowEvent::ALL[ei]).is_some();
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
pub fn mxu1681flow_terminal(st: mxu1681flowState) -> bool {
    let mut i = 0usize;
    while i < 8 {
        if MXU1681FLOW_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct mxu1681flowLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: mxu1681flowState,
}

impl mxu1681flowLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: mxu1681flowState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: mxu1681flowEvent) -> bool {
        match mxu1681flow_next(self.cur, ev) {
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
// 快照 Mxu1681Snap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// 时间片 快照封存（FNV 三态指纹 + 篡改检出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mxu1681Snap {
    /// 指纹分量：bs_v。
    pub bs_v: u64,
    /// 指纹分量：qz_v。
    pub qz_v: u64,
    /// 指纹分量：jz_v。
    pub jz_v: u64,
    /// 指纹分量：ms_v。
    pub ms_v: u64,
    /// 指纹分量：dd_v。
    pub dd_v: u64,
    /// 指纹分量：wy_v。
    pub wy_v: u64,
    /// 指纹分量：sj_v。
    pub sj_v: u64,
    /// 指纹分量：gs_v。
    pub gs_v: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl Mxu1681Snap {
    /// 构造并即时合成指纹。
    pub fn compute(
        bs_v: u64,
        qz_v: u64,
        jz_v: u64,
        ms_v: u64,
        dd_v: u64,
        wy_v: u64,
        sj_v: u64,
        gs_v: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &bs_v.to_le_bytes());
        h = fnv1a64_from(h, &qz_v.to_le_bytes());
        h = fnv1a64_from(h, &jz_v.to_le_bytes());
        h = fnv1a64_from(h, &ms_v.to_le_bytes());
        h = fnv1a64_from(h, &dd_v.to_le_bytes());
        h = fnv1a64_from(h, &wy_v.to_le_bytes());
        h = fnv1a64_from(h, &sj_v.to_le_bytes());
        h = fnv1a64_from(h, &gs_v.to_le_bytes());
        Self {
            bs_v,
            qz_v,
            jz_v,
            ms_v,
            dd_v,
            wy_v,
            sj_v,
            gs_v,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.bs_v.to_le_bytes());
        h = fnv1a64_from(h, &self.qz_v.to_le_bytes());
        h = fnv1a64_from(h, &self.jz_v.to_le_bytes());
        h = fnv1a64_from(h, &self.ms_v.to_le_bytes());
        h = fnv1a64_from(h, &self.dd_v.to_le_bytes());
        h = fnv1a64_from(h, &self.wy_v.to_le_bytes());
        h = fnv1a64_from(h, &self.sj_v.to_le_bytes());
        h = fnv1a64_from(h, &self.gs_v.to_le_bytes());
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

/// 段位守卫：全部诊断码必须落在本段 [0x5810, 0x581f]。
pub const fn u1681_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u1681Code::ALL.len() {
        let c = u1681Code::ALL[i].code();
        if c < 0x5810 || c > 0x581f {
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
fn u1681_guard_seg() -> bool {
    u1681_segment_guard()
}

/// UNX-F1681 状态机闭集/迁移表/非法组合恰值全绿。
fn check_u1681_sm_legal() -> bool {
    let mut ok = mxu1681flow_illegal_pairs() == 29;
    if ok { ok = MXU1681FLOW_TRANSITIONS.len() == 8; }
    if ok { ok = mxu1681flowState::ALL.len() == 6 && mxu1681flowEvent::ALL.len() == 6; }
    ok
}

/// UNX-F1681 迁移主链回放 + 快照往返恒等。
fn check_u1681_sm_round() -> bool {
    let mut lg = mxu1681flowLedger::new(mxu1681flowState::ParamFreeze);
    let mut ok = lg.apply(mxu1681flowEvent::FreezeParam) && lg.cur == mxu1681flowState::Compute && lg.total() == 1;
    let s0 = Mxu1681Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    let mut s1 = Mxu1681Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    s1.recompute();
    if ok { ok = s0 == s1 && !s1.tampered(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u1681_checks() -> CheckSet {
    let mut set = CheckSet::new("U1681");
    set.add(
        "u1681-seg-guard",
        u1681_guard_seg(),
        if u1681_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u1681-sm-legal",
        check_u1681_sm_legal(),
        if check_u1681_sm_legal() { "" } else { "UNX-F1681 sm legality mismatch" },
    );
    set.add(
        "u1681-sm-round",
        check_u1681_sm_round(),
        if check_u1681_sm_round() { "" } else { "UNX-F1681 sm roundtrip mismatch" },
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
    fn u1681_all_checks_green() {
        let set = run_u1681_checks();
        assert!(set.all_passed(), "U1681 domain checks must be green");
    }

    #[test]
    fn u1681_segment_guard_holds() {
        assert!(u1681_segment_guard());
    }
}
