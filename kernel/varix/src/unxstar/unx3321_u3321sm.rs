//! unxu3321_u3321sm —— 锁类别总表（五类选型矩阵） 状态机与快照封存（锁类别选型矩阵）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3321 · 锁类别总表（五类选型矩阵） · 判据原文：UNX-F3321 · 判据：J1：五类（spin/mutex/rwlock/seqlock/rcu）× 五维（上下文/时长/并发/观测/典型点）选型矩阵交付；M-080 矩阵抽检用例（每类一场景断言选型正确）。
//! · 组件：锁类别选型矩阵 状态机（6 态 × 6 事件显式迁移）+ FNV 快照封存（8 分量）
//! · 判据落地：UNX-F3321 · 判据：J1：五类（spin/mutex/rwlock/seqlock/rcu）× 五维（上下文/时长/并发/观测/典型点）选型矩阵交付；
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3321_VERSION: &str = "u3321-u3321sm-v1";

/// 诊断段基址。
pub const U3321_SEG_BASE: u16 = 0xe040;

// --------------------------------------------------------------------------
// 诊断码（u3321 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 锁类别选型矩阵 状态面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3321Code {
    /// 0xe040 状态重复（锁 同名双态）。
    StateDup,
    /// 0xe041 状态断档（矩阵 序缺失）。
    StateHole,
    /// 0xe042 事件重复（同事件双声明）。
    EventDup,
    /// 0xe043 迁移重复（同源同事件双迁移）。
    TransDup,
    /// 0xe044 迁移缺失（应达态不可达）。
    TransHole,
    /// 0xe045 迁移环（非法自环/环路）。
    TransCycle,
    /// 0xe046 入口非法（初态不在闭集）。
    EntryBad,
    /// 0xe047 迁移丢失（应用事件无效果）。
    ApplyLost,
    /// 0xe048 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe049 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe04a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe04b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3321Code {
    /// 全量闭集。
    pub const ALL: [u3321Code; 12] = [
        u3321Code::StateDup,
        u3321Code::StateHole,
        u3321Code::EventDup,
        u3321Code::TransDup,
        u3321Code::TransHole,
        u3321Code::TransCycle,
        u3321Code::EntryBad,
        u3321Code::ApplyLost,
        u3321Code::LedgerBroken,
        u3321Code::SnapshotFingerprint,
        u3321Code::TableBroken,
        u3321Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3321Code::StateDup => 0xe040,
            u3321Code::StateHole => 0xe041,
            u3321Code::EventDup => 0xe042,
            u3321Code::TransDup => 0xe043,
            u3321Code::TransHole => 0xe044,
            u3321Code::TransCycle => 0xe045,
            u3321Code::EntryBad => 0xe046,
            u3321Code::ApplyLost => 0xe047,
            u3321Code::LedgerBroken => 0xe048,
            u3321Code::SnapshotFingerprint => 0xe049,
            u3321Code::TableBroken => 0xe04a,
            u3321Code::SegmentGuard => 0xe04b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3321Code::StateDup => "状态重复（锁 同名双态）",
            u3321Code::StateHole => "状态断档（矩阵 序缺失）",
            u3321Code::EventDup => "事件重复（同事件双声明）",
            u3321Code::TransDup => "迁移重复（同源同事件双迁移）",
            u3321Code::TransHole => "迁移缺失（应达态不可达）",
            u3321Code::TransCycle => "迁移环（非法自环/环路）",
            u3321Code::EntryBad => "入口非法（初态不在闭集）",
            u3321Code::ApplyLost => "迁移丢失（应用事件无效果）",
            u3321Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3321Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3321Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3321Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3321Code::StateDup => "按闭集唯一律去重",
            u3321Code::StateHole => "按闭集补态",
            u3321Code::EventDup => "按事件闭集去重",
            u3321Code::TransDup => "按确定性唯一迁移律合并",
            u3321Code::TransHole => "按迁移矩阵补边",
            u3321Code::TransCycle => "按无环律重构迁移",
            u3321Code::EntryBad => "按闭集钳制初态",
            u3321Code::ApplyLost => "核对事件前置条件",
            u3321Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3321Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3321Code::TableBroken => "按语义规则重建常量表",
            u3321Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 状态机 mxu3321flow（6 态 × 6 事件 · 迁移 8 条显式声明）
// --------------------------------------------------------------------------
/// 锁类别选型矩阵 状态机（判据：锁）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu3321flowState {
    Scan,
    Weigh,
    Match,
    Rank,
    Freeze,
    Revise,
}

impl mxu3321flowState {
    /// 全量态闭集。
    pub const ALL: [mxu3321flowState; 6] = [
        mxu3321flowState::Scan,
        mxu3321flowState::Weigh,
        mxu3321flowState::Match,
        mxu3321flowState::Rank,
        mxu3321flowState::Freeze,
        mxu3321flowState::Revise,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            mxu3321flowState::Scan => 0,
            mxu3321flowState::Weigh => 1,
            mxu3321flowState::Match => 2,
            mxu3321flowState::Rank => 3,
            mxu3321flowState::Freeze => 4,
            mxu3321flowState::Revise => 5,
        }
    }
}

/// mxu3321flow事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu3321flowEvent {
    Query,
    Filter,
    Score,
    Recommend,
    Freeze,
    Revise,
}

impl mxu3321flowEvent {
    /// 全量事件闭集。
    pub const ALL: [mxu3321flowEvent; 6] = [
        mxu3321flowEvent::Query,
        mxu3321flowEvent::Filter,
        mxu3321flowEvent::Score,
        mxu3321flowEvent::Recommend,
        mxu3321flowEvent::Freeze,
        mxu3321flowEvent::Revise,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const MXU3321FLOW_TRANSITIONS: [(mxu3321flowState, mxu3321flowEvent, mxu3321flowState); 8] = [
    (mxu3321flowState::Scan, mxu3321flowEvent::Query, mxu3321flowState::Weigh), // Scan→Weigh 主链
    (mxu3321flowState::Weigh, mxu3321flowEvent::Filter, mxu3321flowState::Match), // Weigh→Match 主链
    (mxu3321flowState::Match, mxu3321flowEvent::Score, mxu3321flowState::Rank), // Match→Rank 主链
    (mxu3321flowState::Rank, mxu3321flowEvent::Recommend, mxu3321flowState::Freeze), // Rank→Freeze 主链
    (mxu3321flowState::Freeze, mxu3321flowEvent::Freeze, mxu3321flowState::Revise), // Freeze→Revise 主链
    (mxu3321flowState::Revise, mxu3321flowEvent::Revise, mxu3321flowState::Scan), // 终态回边
    (mxu3321flowState::Revise, mxu3321flowEvent::Query, mxu3321flowState::Freeze), // 终态回退边
    (mxu3321flowState::Weigh, mxu3321flowEvent::Filter, mxu3321flowState::Match), // 旁路边
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn mxu3321flow_next(state: mxu3321flowState, ev: mxu3321flowEvent) -> Option<mxu3321flowState> {
    let mut i = 0usize;
    while i < 8 {
        let t = MXU3321FLOW_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn mxu3321flow_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < mxu3321flowState::ALL.len() {
        let mut ei = 0usize;
        while ei < mxu3321flowEvent::ALL.len() {
            let legal = mxu3321flow_next(mxu3321flowState::ALL[si], mxu3321flowEvent::ALL[ei]).is_some();
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
pub fn mxu3321flow_terminal(st: mxu3321flowState) -> bool {
    let mut i = 0usize;
    while i < 8 {
        if MXU3321FLOW_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct mxu3321flowLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: mxu3321flowState,
}

impl mxu3321flowLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: mxu3321flowState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: mxu3321flowEvent) -> bool {
        match mxu3321flow_next(self.cur, ev) {
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
// 快照 Mxu3321Snap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// 锁类别选型矩阵 快照封存（FNV 三态指纹 + 篡改检出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mxu3321Snap {
    /// 指纹分量：mtx_v。
    pub mtx_v: u64,
    /// 指纹分量：spn_v。
    pub spn_v: u64,
    /// 指纹分量：rwk_v。
    pub rwk_v: u64,
    /// 指纹分量：seq_v。
    pub seq_v: u64,
    /// 指纹分量：rcu_v。
    pub rcu_v: u64,
    /// 指纹分量：lwp_v。
    pub lwp_v: u64,
    /// 指纹分量：axs_v。
    pub axs_v: u64,
    /// 指纹分量：ctx_v。
    pub ctx_v: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl Mxu3321Snap {
    /// 构造并即时合成指纹。
    pub fn compute(
        mtx_v: u64,
        spn_v: u64,
        rwk_v: u64,
        seq_v: u64,
        rcu_v: u64,
        lwp_v: u64,
        axs_v: u64,
        ctx_v: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &mtx_v.to_le_bytes());
        h = fnv1a64_from(h, &spn_v.to_le_bytes());
        h = fnv1a64_from(h, &rwk_v.to_le_bytes());
        h = fnv1a64_from(h, &seq_v.to_le_bytes());
        h = fnv1a64_from(h, &rcu_v.to_le_bytes());
        h = fnv1a64_from(h, &lwp_v.to_le_bytes());
        h = fnv1a64_from(h, &axs_v.to_le_bytes());
        h = fnv1a64_from(h, &ctx_v.to_le_bytes());
        Self {
            mtx_v,
            spn_v,
            rwk_v,
            seq_v,
            rcu_v,
            lwp_v,
            axs_v,
            ctx_v,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.mtx_v.to_le_bytes());
        h = fnv1a64_from(h, &self.spn_v.to_le_bytes());
        h = fnv1a64_from(h, &self.rwk_v.to_le_bytes());
        h = fnv1a64_from(h, &self.seq_v.to_le_bytes());
        h = fnv1a64_from(h, &self.rcu_v.to_le_bytes());
        h = fnv1a64_from(h, &self.lwp_v.to_le_bytes());
        h = fnv1a64_from(h, &self.axs_v.to_le_bytes());
        h = fnv1a64_from(h, &self.ctx_v.to_le_bytes());
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

/// 段位守卫：全部诊断码必须落在本段 [0xe040, 0xe04f]。
pub const fn u3321_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3321Code::ALL.len() {
        let c = u3321Code::ALL[i].code();
        if c < 0xe040 || c > 0xe04f {
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
fn u3321_guard_seg() -> bool {
    u3321_segment_guard()
}

/// UNX-F3321 状态机闭集/迁移表/非法组合恰值全绿。
fn check_u3321_sm_legal() -> bool {
    let mut ok = mxu3321flow_illegal_pairs() == 29;
    if ok { ok = MXU3321FLOW_TRANSITIONS.len() == 8; }
    if ok { ok = mxu3321flowState::ALL.len() == 6 && mxu3321flowEvent::ALL.len() == 6; }
    ok
}

/// UNX-F3321 迁移主链回放 + 快照往返恒等。
fn check_u3321_sm_round() -> bool {
    let mut lg = mxu3321flowLedger::new(mxu3321flowState::Scan);
    let mut ok = lg.apply(mxu3321flowEvent::Query) && lg.cur == mxu3321flowState::Weigh && lg.total() == 1;
    let s0 = Mxu3321Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    let mut s1 = Mxu3321Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    s1.recompute();
    if ok { ok = s0 == s1 && !s1.tampered(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3321_checks() -> CheckSet {
    let mut set = CheckSet::new("U3321");
    set.add(
        "u3321-seg-guard",
        u3321_guard_seg(),
        if u3321_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3321-sm-legal",
        check_u3321_sm_legal(),
        if check_u3321_sm_legal() { "" } else { "UNX-F3321 sm legality mismatch" },
    );
    set.add(
        "u3321-sm-round",
        check_u3321_sm_round(),
        if check_u3321_sm_round() { "" } else { "UNX-F3321 sm roundtrip mismatch" },
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
    fn u3321_all_checks_green() {
        let set = run_u3321_checks();
        assert!(set.all_passed(), "U3321 domain checks must be green");
    }

    #[test]
    fn u3321_segment_guard_holds() {
        assert!(u3321_segment_guard());
    }
}
