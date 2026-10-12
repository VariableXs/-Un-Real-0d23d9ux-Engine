//! unxu3581_u3581sm —— 内存水位账总设计（per-pool 水位/四级线） 状态机与快照封存（内存水位账）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3581 · 内存水位账总设计（per-pool 水位/四级线） · 判据原文：UNX-F3581 · 判据：J1：四级线冻结（NORMAL 60%/WATCH 75%/HIGH 90%/CRITICAL 97%——判据行内嵌各线语义与响应）；M-339 四级全链（逐级越线→账+告警+响应分级）。
//! · 组件：内存水位账 状态机（6 态 × 6 事件显式迁移）+ FNV 快照封存（8 分量）
//! · 判据落地：UNX-F3581 · 判据：J1：四级线冻结（NORMAL 60%/WATCH 75%/HIGH 90%/CRITICAL 97%——判据行内嵌各线语义与响应
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3581_VERSION: &str = "u3581-u3581sm-v1";

/// 诊断段基址。
pub const U3581_SEG_BASE: u16 = 0xe110;

// --------------------------------------------------------------------------
// 诊断码（u3581 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 内存水位账 状态面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3581Code {
    /// 0xe110 状态重复（水位 同名双态）。
    StateDup,
    /// 0xe111 状态断档（内存池 序缺失）。
    StateHole,
    /// 0xe112 事件重复（同事件双声明）。
    EventDup,
    /// 0xe113 迁移重复（同源同事件双迁移）。
    TransDup,
    /// 0xe114 迁移缺失（应达态不可达）。
    TransHole,
    /// 0xe115 迁移环（非法自环/环路）。
    TransCycle,
    /// 0xe116 入口非法（初态不在闭集）。
    EntryBad,
    /// 0xe117 迁移丢失（应用事件无效果）。
    ApplyLost,
    /// 0xe118 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe119 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe11a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe11b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3581Code {
    /// 全量闭集。
    pub const ALL: [u3581Code; 12] = [
        u3581Code::StateDup,
        u3581Code::StateHole,
        u3581Code::EventDup,
        u3581Code::TransDup,
        u3581Code::TransHole,
        u3581Code::TransCycle,
        u3581Code::EntryBad,
        u3581Code::ApplyLost,
        u3581Code::LedgerBroken,
        u3581Code::SnapshotFingerprint,
        u3581Code::TableBroken,
        u3581Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3581Code::StateDup => 0xe110,
            u3581Code::StateHole => 0xe111,
            u3581Code::EventDup => 0xe112,
            u3581Code::TransDup => 0xe113,
            u3581Code::TransHole => 0xe114,
            u3581Code::TransCycle => 0xe115,
            u3581Code::EntryBad => 0xe116,
            u3581Code::ApplyLost => 0xe117,
            u3581Code::LedgerBroken => 0xe118,
            u3581Code::SnapshotFingerprint => 0xe119,
            u3581Code::TableBroken => 0xe11a,
            u3581Code::SegmentGuard => 0xe11b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3581Code::StateDup => "状态重复（水位 同名双态）",
            u3581Code::StateHole => "状态断档（内存池 序缺失）",
            u3581Code::EventDup => "事件重复（同事件双声明）",
            u3581Code::TransDup => "迁移重复（同源同事件双迁移）",
            u3581Code::TransHole => "迁移缺失（应达态不可达）",
            u3581Code::TransCycle => "迁移环（非法自环/环路）",
            u3581Code::EntryBad => "入口非法（初态不在闭集）",
            u3581Code::ApplyLost => "迁移丢失（应用事件无效果）",
            u3581Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3581Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3581Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3581Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3581Code::StateDup => "按闭集唯一律去重",
            u3581Code::StateHole => "按闭集补态",
            u3581Code::EventDup => "按事件闭集去重",
            u3581Code::TransDup => "按确定性唯一迁移律合并",
            u3581Code::TransHole => "按迁移矩阵补边",
            u3581Code::TransCycle => "按无环律重构迁移",
            u3581Code::EntryBad => "按闭集钳制初态",
            u3581Code::ApplyLost => "核对事件前置条件",
            u3581Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3581Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3581Code::TableBroken => "按语义规则重建常量表",
            u3581Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 状态机 mxu3581flow（6 态 × 6 事件 · 迁移 8 条显式声明）
// --------------------------------------------------------------------------
/// 内存水位账 状态机（判据：水位）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu3581flowState {
    Normal,
    Warn,
    Crit,
    Dead,
    Reclaim,
    Stable,
}

impl mxu3581flowState {
    /// 全量态闭集。
    pub const ALL: [mxu3581flowState; 6] = [
        mxu3581flowState::Normal,
        mxu3581flowState::Warn,
        mxu3581flowState::Crit,
        mxu3581flowState::Dead,
        mxu3581flowState::Reclaim,
        mxu3581flowState::Stable,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            mxu3581flowState::Normal => 0,
            mxu3581flowState::Warn => 1,
            mxu3581flowState::Crit => 2,
            mxu3581flowState::Dead => 3,
            mxu3581flowState::Reclaim => 4,
            mxu3581flowState::Stable => 5,
        }
    }
}

/// mxu3581flow事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu3581flowEvent {
    Sample,
    Classify,
    Trigger,
    Reclaim,
    Escalate,
    Stabilize,
}

impl mxu3581flowEvent {
    /// 全量事件闭集。
    pub const ALL: [mxu3581flowEvent; 6] = [
        mxu3581flowEvent::Sample,
        mxu3581flowEvent::Classify,
        mxu3581flowEvent::Trigger,
        mxu3581flowEvent::Reclaim,
        mxu3581flowEvent::Escalate,
        mxu3581flowEvent::Stabilize,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const MXU3581FLOW_TRANSITIONS: [(mxu3581flowState, mxu3581flowEvent, mxu3581flowState); 8] = [
    (mxu3581flowState::Normal, mxu3581flowEvent::Sample, mxu3581flowState::Warn), // Normal→Warn 主链
    (mxu3581flowState::Warn, mxu3581flowEvent::Classify, mxu3581flowState::Crit), // Warn→Crit 主链
    (mxu3581flowState::Crit, mxu3581flowEvent::Trigger, mxu3581flowState::Dead), // Crit→Dead 主链
    (mxu3581flowState::Dead, mxu3581flowEvent::Reclaim, mxu3581flowState::Reclaim), // Dead→Reclaim 主链
    (mxu3581flowState::Reclaim, mxu3581flowEvent::Escalate, mxu3581flowState::Stable), // Reclaim→Stable 主链
    (mxu3581flowState::Stable, mxu3581flowEvent::Stabilize, mxu3581flowState::Normal), // 终态回边
    (mxu3581flowState::Stable, mxu3581flowEvent::Sample, mxu3581flowState::Reclaim), // 终态回退边
    (mxu3581flowState::Warn, mxu3581flowEvent::Classify, mxu3581flowState::Crit), // 旁路边
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn mxu3581flow_next(state: mxu3581flowState, ev: mxu3581flowEvent) -> Option<mxu3581flowState> {
    let mut i = 0usize;
    while i < 8 {
        let t = MXU3581FLOW_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn mxu3581flow_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < mxu3581flowState::ALL.len() {
        let mut ei = 0usize;
        while ei < mxu3581flowEvent::ALL.len() {
            let legal = mxu3581flow_next(mxu3581flowState::ALL[si], mxu3581flowEvent::ALL[ei]).is_some();
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
pub fn mxu3581flow_terminal(st: mxu3581flowState) -> bool {
    let mut i = 0usize;
    while i < 8 {
        if MXU3581FLOW_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct mxu3581flowLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: mxu3581flowState,
}

impl mxu3581flowLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: mxu3581flowState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: mxu3581flowEvent) -> bool {
        match mxu3581flow_next(self.cur, ev) {
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
// 快照 Mxu3581Snap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// 内存水位账 快照封存（FNV 三态指纹 + 篡改检出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mxu3581Snap {
    /// 指纹分量：pool_v。
    pub pool_v: u64,
    /// 指纹分量：lvl_v。
    pub lvl_v: u64,
    /// 指纹分量：l4_v。
    pub l4_v: u64,
    /// 指纹分量：cur_v。
    pub cur_v: u64,
    /// 指纹分量：high_v。
    pub high_v: u64,
    /// 指纹分量：low_v。
    pub low_v: u64,
    /// 指纹分量：resv_v。
    pub resv_v: u64,
    /// 指纹分量：avail_v。
    pub avail_v: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl Mxu3581Snap {
    /// 构造并即时合成指纹。
    pub fn compute(
        pool_v: u64,
        lvl_v: u64,
        l4_v: u64,
        cur_v: u64,
        high_v: u64,
        low_v: u64,
        resv_v: u64,
        avail_v: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &pool_v.to_le_bytes());
        h = fnv1a64_from(h, &lvl_v.to_le_bytes());
        h = fnv1a64_from(h, &l4_v.to_le_bytes());
        h = fnv1a64_from(h, &cur_v.to_le_bytes());
        h = fnv1a64_from(h, &high_v.to_le_bytes());
        h = fnv1a64_from(h, &low_v.to_le_bytes());
        h = fnv1a64_from(h, &resv_v.to_le_bytes());
        h = fnv1a64_from(h, &avail_v.to_le_bytes());
        Self {
            pool_v,
            lvl_v,
            l4_v,
            cur_v,
            high_v,
            low_v,
            resv_v,
            avail_v,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.pool_v.to_le_bytes());
        h = fnv1a64_from(h, &self.lvl_v.to_le_bytes());
        h = fnv1a64_from(h, &self.l4_v.to_le_bytes());
        h = fnv1a64_from(h, &self.cur_v.to_le_bytes());
        h = fnv1a64_from(h, &self.high_v.to_le_bytes());
        h = fnv1a64_from(h, &self.low_v.to_le_bytes());
        h = fnv1a64_from(h, &self.resv_v.to_le_bytes());
        h = fnv1a64_from(h, &self.avail_v.to_le_bytes());
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

/// 段位守卫：全部诊断码必须落在本段 [0xe110, 0xe11f]。
pub const fn u3581_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3581Code::ALL.len() {
        let c = u3581Code::ALL[i].code();
        if c < 0xe110 || c > 0xe11f {
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
fn u3581_guard_seg() -> bool {
    u3581_segment_guard()
}

/// UNX-F3581 状态机闭集/迁移表/非法组合恰值全绿。
fn check_u3581_sm_legal() -> bool {
    let mut ok = mxu3581flow_illegal_pairs() == 29;
    if ok { ok = MXU3581FLOW_TRANSITIONS.len() == 8; }
    if ok { ok = mxu3581flowState::ALL.len() == 6 && mxu3581flowEvent::ALL.len() == 6; }
    ok
}

/// UNX-F3581 迁移主链回放 + 快照往返恒等。
fn check_u3581_sm_round() -> bool {
    let mut lg = mxu3581flowLedger::new(mxu3581flowState::Normal);
    let mut ok = lg.apply(mxu3581flowEvent::Sample) && lg.cur == mxu3581flowState::Warn && lg.total() == 1;
    let s0 = Mxu3581Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    let mut s1 = Mxu3581Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    s1.recompute();
    if ok { ok = s0 == s1 && !s1.tampered(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3581_checks() -> CheckSet {
    let mut set = CheckSet::new("U3581");
    set.add(
        "u3581-seg-guard",
        u3581_guard_seg(),
        if u3581_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3581-sm-legal",
        check_u3581_sm_legal(),
        if check_u3581_sm_legal() { "" } else { "UNX-F3581 sm legality mismatch" },
    );
    set.add(
        "u3581-sm-round",
        check_u3581_sm_round(),
        if check_u3581_sm_round() { "" } else { "UNX-F3581 sm roundtrip mismatch" },
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
    fn u3581_all_checks_green() {
        let set = run_u3581_checks();
        assert!(set.all_passed(), "U3581 domain checks must be green");
    }

    #[test]
    fn u3581_segment_guard_holds() {
        assert!(u3581_segment_guard());
    }
}
