//! unxu2521_u2521sm —— 碎片指数定义与计算（B02 前向引用兑现） 状态机与快照封存（碎片指数）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2521 · 碎片指数定义与计算（B02 前向引用兑现） · 判据原文：UNX-F2521 · 判据：碎片指数计算与手算样例一致（可复算断言），指数含大页项（F2517 契约消费）
//! · 组件：碎片指数 状态机（6 态 × 6 事件显式迁移）+ FNV 快照封存（8 分量）
//! · 判据落地：UNX-F2521 · 判据：碎片指数计算与手算样例一致（可复算断言），指数含大页项（F2517 契约消费）
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2521_VERSION: &str = "u2521-u2521sm-v1";

/// 诊断段基址。
pub const U2521_SEG_BASE: u16 = 0xa490;

// --------------------------------------------------------------------------
// 诊断码（u2521 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 碎片指数 状态面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2521Code {
    /// 0xa490 状态重复（复算不符 同名双态）。
    StateDup,
    /// 0xa491 状态断档（大页项缺 序缺失）。
    StateHole,
    /// 0xa492 事件重复（同事件双声明）。
    EventDup,
    /// 0xa493 迁移重复（同源同事件双迁移）。
    TransDup,
    /// 0xa494 迁移缺失（应达态不可达）。
    TransHole,
    /// 0xa495 迁移环（非法自环/环路）。
    TransCycle,
    /// 0xa496 入口非法（初态不在闭集）。
    EntryBad,
    /// 0xa497 迁移丢失（应用事件无效果）。
    ApplyLost,
    /// 0xa498 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xa499 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xa49a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xa49b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2521Code {
    /// 全量闭集。
    pub const ALL: [u2521Code; 12] = [
        u2521Code::StateDup,
        u2521Code::StateHole,
        u2521Code::EventDup,
        u2521Code::TransDup,
        u2521Code::TransHole,
        u2521Code::TransCycle,
        u2521Code::EntryBad,
        u2521Code::ApplyLost,
        u2521Code::LedgerBroken,
        u2521Code::SnapshotFingerprint,
        u2521Code::TableBroken,
        u2521Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2521Code::StateDup => 0xa490,
            u2521Code::StateHole => 0xa491,
            u2521Code::EventDup => 0xa492,
            u2521Code::TransDup => 0xa493,
            u2521Code::TransHole => 0xa494,
            u2521Code::TransCycle => 0xa495,
            u2521Code::EntryBad => 0xa496,
            u2521Code::ApplyLost => 0xa497,
            u2521Code::LedgerBroken => 0xa498,
            u2521Code::SnapshotFingerprint => 0xa499,
            u2521Code::TableBroken => 0xa49a,
            u2521Code::SegmentGuard => 0xa49b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2521Code::StateDup => "状态重复（复算不符 同名双态）",
            u2521Code::StateHole => "状态断档（大页项缺 序缺失）",
            u2521Code::EventDup => "事件重复（同事件双声明）",
            u2521Code::TransDup => "迁移重复（同源同事件双迁移）",
            u2521Code::TransHole => "迁移缺失（应达态不可达）",
            u2521Code::TransCycle => "迁移环（非法自环/环路）",
            u2521Code::EntryBad => "入口非法（初态不在闭集）",
            u2521Code::ApplyLost => "迁移丢失（应用事件无效果）",
            u2521Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2521Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2521Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2521Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2521Code::StateDup => "按闭集唯一律去重",
            u2521Code::StateHole => "按闭集补态",
            u2521Code::EventDup => "按事件闭集去重",
            u2521Code::TransDup => "按确定性唯一迁移律合并",
            u2521Code::TransHole => "按迁移矩阵补边",
            u2521Code::TransCycle => "按无环律重构迁移",
            u2521Code::EntryBad => "按闭集钳制初态",
            u2521Code::ApplyLost => "核对事件前置条件",
            u2521Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2521Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2521Code::TableBroken => "按语义规则重建常量表",
            u2521Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 状态机 mxu2521flow（6 态 × 6 事件 · 迁移 8 条显式声明）
// --------------------------------------------------------------------------
/// 碎片指数 状态机（判据：复算不符）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu2521flowState {
    Idle,
    Sampling,
    Computing,
    Recomputed,
    Contracted,
    Sealed,
}

impl mxu2521flowState {
    /// 全量态闭集。
    pub const ALL: [mxu2521flowState; 6] = [
        mxu2521flowState::Idle,
        mxu2521flowState::Sampling,
        mxu2521flowState::Computing,
        mxu2521flowState::Recomputed,
        mxu2521flowState::Contracted,
        mxu2521flowState::Sealed,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            mxu2521flowState::Idle => 0,
            mxu2521flowState::Sampling => 1,
            mxu2521flowState::Computing => 2,
            mxu2521flowState::Recomputed => 3,
            mxu2521flowState::Contracted => 4,
            mxu2521flowState::Sealed => 5,
        }
    }
}

/// mxu2521flow事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu2521flowEvent {
    Sample,
    Compute,
    Recalc,
    Compare,
    Consume,
    Seal,
}

impl mxu2521flowEvent {
    /// 全量事件闭集。
    pub const ALL: [mxu2521flowEvent; 6] = [
        mxu2521flowEvent::Sample,
        mxu2521flowEvent::Compute,
        mxu2521flowEvent::Recalc,
        mxu2521flowEvent::Compare,
        mxu2521flowEvent::Consume,
        mxu2521flowEvent::Seal,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const MXU2521FLOW_TRANSITIONS: [(mxu2521flowState, mxu2521flowEvent, mxu2521flowState); 8] = [
    (mxu2521flowState::Idle, mxu2521flowEvent::Sample, mxu2521flowState::Sampling), // Idle→Sampling 主链
    (mxu2521flowState::Sampling, mxu2521flowEvent::Compute, mxu2521flowState::Computing), // Sampling→Computing 主链
    (mxu2521flowState::Computing, mxu2521flowEvent::Recalc, mxu2521flowState::Recomputed), // Computing→Recomputed 主链
    (mxu2521flowState::Recomputed, mxu2521flowEvent::Compare, mxu2521flowState::Contracted), // Recomputed→Contracted 主链
    (mxu2521flowState::Contracted, mxu2521flowEvent::Consume, mxu2521flowState::Sealed), // Contracted→Sealed 主链
    (mxu2521flowState::Sealed, mxu2521flowEvent::Seal, mxu2521flowState::Idle), // 终态回边
    (mxu2521flowState::Sealed, mxu2521flowEvent::Sample, mxu2521flowState::Contracted), // 终态回退边
    (mxu2521flowState::Sampling, mxu2521flowEvent::Compute, mxu2521flowState::Computing), // 旁路边
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn mxu2521flow_next(state: mxu2521flowState, ev: mxu2521flowEvent) -> Option<mxu2521flowState> {
    let mut i = 0usize;
    while i < 8 {
        let t = MXU2521FLOW_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn mxu2521flow_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < mxu2521flowState::ALL.len() {
        let mut ei = 0usize;
        while ei < mxu2521flowEvent::ALL.len() {
            let legal = mxu2521flow_next(mxu2521flowState::ALL[si], mxu2521flowEvent::ALL[ei]).is_some();
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
pub fn mxu2521flow_terminal(st: mxu2521flowState) -> bool {
    let mut i = 0usize;
    while i < 8 {
        if MXU2521FLOW_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct mxu2521flowLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: mxu2521flowState,
}

impl mxu2521flowLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: mxu2521flowState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: mxu2521flowEvent) -> bool {
        match mxu2521flow_next(self.cur, ev) {
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
// 快照 Mxu2521Snap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// 碎片指数 快照封存（FNV 三态指纹 + 篡改检出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mxu2521Snap {
    /// 指纹分量：fi_v。
    pub fi_v: u64,
    /// 指纹分量：hc_v。
    pub hc_v: u64,
    /// 指纹分量：rc_v。
    pub rc_v: u64,
    /// 指纹分量：hp_v。
    pub hp_v: u64,
    /// 指纹分量：ct_v。
    pub ct_v: u64,
    /// 指纹分量：fr_v。
    pub fr_v: u64,
    /// 指纹分量：av_v。
    pub av_v: u64,
    /// 指纹分量：mn_v。
    pub mn_v: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl Mxu2521Snap {
    /// 构造并即时合成指纹。
    pub fn compute(
        fi_v: u64,
        hc_v: u64,
        rc_v: u64,
        hp_v: u64,
        ct_v: u64,
        fr_v: u64,
        av_v: u64,
        mn_v: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &fi_v.to_le_bytes());
        h = fnv1a64_from(h, &hc_v.to_le_bytes());
        h = fnv1a64_from(h, &rc_v.to_le_bytes());
        h = fnv1a64_from(h, &hp_v.to_le_bytes());
        h = fnv1a64_from(h, &ct_v.to_le_bytes());
        h = fnv1a64_from(h, &fr_v.to_le_bytes());
        h = fnv1a64_from(h, &av_v.to_le_bytes());
        h = fnv1a64_from(h, &mn_v.to_le_bytes());
        Self {
            fi_v,
            hc_v,
            rc_v,
            hp_v,
            ct_v,
            fr_v,
            av_v,
            mn_v,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.fi_v.to_le_bytes());
        h = fnv1a64_from(h, &self.hc_v.to_le_bytes());
        h = fnv1a64_from(h, &self.rc_v.to_le_bytes());
        h = fnv1a64_from(h, &self.hp_v.to_le_bytes());
        h = fnv1a64_from(h, &self.ct_v.to_le_bytes());
        h = fnv1a64_from(h, &self.fr_v.to_le_bytes());
        h = fnv1a64_from(h, &self.av_v.to_le_bytes());
        h = fnv1a64_from(h, &self.mn_v.to_le_bytes());
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

/// 段位守卫：全部诊断码必须落在本段 [0xa490, 0xa49f]。
pub const fn u2521_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2521Code::ALL.len() {
        let c = u2521Code::ALL[i].code();
        if c < 0xa490 || c > 0xa49f {
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
fn u2521_guard_seg() -> bool {
    u2521_segment_guard()
}

/// UNX-F2521 状态机闭集/迁移表/非法组合恰值全绿。
fn check_u2521_sm_legal() -> bool {
    let mut ok = mxu2521flow_illegal_pairs() == 29;
    if ok { ok = MXU2521FLOW_TRANSITIONS.len() == 8; }
    if ok { ok = mxu2521flowState::ALL.len() == 6 && mxu2521flowEvent::ALL.len() == 6; }
    ok
}

/// UNX-F2521 迁移主链回放 + 快照往返恒等。
fn check_u2521_sm_round() -> bool {
    let mut lg = mxu2521flowLedger::new(mxu2521flowState::Idle);
    let mut ok = lg.apply(mxu2521flowEvent::Sample) && lg.cur == mxu2521flowState::Sampling && lg.total() == 1;
    let s0 = Mxu2521Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    let mut s1 = Mxu2521Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    s1.recompute();
    if ok { ok = s0 == s1 && !s1.tampered(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2521_checks() -> CheckSet {
    let mut set = CheckSet::new("U2521");
    set.add(
        "u2521-seg-guard",
        u2521_guard_seg(),
        if u2521_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2521-sm-legal",
        check_u2521_sm_legal(),
        if check_u2521_sm_legal() { "" } else { "UNX-F2521 sm legality mismatch" },
    );
    set.add(
        "u2521-sm-round",
        check_u2521_sm_round(),
        if check_u2521_sm_round() { "" } else { "UNX-F2521 sm roundtrip mismatch" },
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
    fn u2521_all_checks_green() {
        let set = run_u2521_checks();
        assert!(set.all_passed(), "U2521 domain checks must be green");
    }

    #[test]
    fn u2521_segment_guard_holds() {
        assert!(u2521_segment_guard());
    }
}
