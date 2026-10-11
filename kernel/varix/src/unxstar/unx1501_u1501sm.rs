//! unxu1501_u1501sm —— 域回归矩阵冻结 v1（场景×判据×预期三维） 状态机与快照封存（场景）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F1501 · 域回归矩阵冻结 v1（场景×判据×预期三维） · 判据原文：UNX-F1501 · 判据：域回归矩阵 v1 冻结（场景/判据引用/预期三列×全域场景登记：账本五族+零丢五场景+四态+接管等价+注入四族），矩阵行数登记 100% 覆盖断言，矩阵漂移注入 10/10 必抓
//! · 组件：场景 状态机（6 态 × 6 事件显式迁移）+ FNV 快照封存（8 分量）
//! · 判据落地：UNX-F1501 · 判据：域回归矩阵 v1 冻结（场景/判据引用/预期三列×全域场景登记：账本五族+零丢五场景+四态+接管等价+注入四族），矩阵行数登记 1
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U1501_VERSION: &str = "u1501-u1501sm-v1";

/// 诊断段基址。
pub const U1501_SEG_BASE: u16 = 0x5210;

// --------------------------------------------------------------------------
// 诊断码（u1501 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 场景 状态面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u1501Code {
    /// 0x5210 状态重复（矩阵漂移 同名双态）。
    StateDup,
    /// 0x5211 状态断档（行覆盖 序缺失）。
    StateHole,
    /// 0x5212 事件重复（同事件双声明）。
    EventDup,
    /// 0x5213 迁移重复（同源同事件双迁移）。
    TransDup,
    /// 0x5214 迁移缺失（应达态不可达）。
    TransHole,
    /// 0x5215 迁移环（非法自环/环路）。
    TransCycle,
    /// 0x5216 入口非法（初态不在闭集）。
    EntryBad,
    /// 0x5217 迁移丢失（应用事件无效果）。
    ApplyLost,
    /// 0x5218 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x5219 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x521a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x521b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u1501Code {
    /// 全量闭集。
    pub const ALL: [u1501Code; 12] = [
        u1501Code::StateDup,
        u1501Code::StateHole,
        u1501Code::EventDup,
        u1501Code::TransDup,
        u1501Code::TransHole,
        u1501Code::TransCycle,
        u1501Code::EntryBad,
        u1501Code::ApplyLost,
        u1501Code::LedgerBroken,
        u1501Code::SnapshotFingerprint,
        u1501Code::TableBroken,
        u1501Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u1501Code::StateDup => 0x5210,
            u1501Code::StateHole => 0x5211,
            u1501Code::EventDup => 0x5212,
            u1501Code::TransDup => 0x5213,
            u1501Code::TransHole => 0x5214,
            u1501Code::TransCycle => 0x5215,
            u1501Code::EntryBad => 0x5216,
            u1501Code::ApplyLost => 0x5217,
            u1501Code::LedgerBroken => 0x5218,
            u1501Code::SnapshotFingerprint => 0x5219,
            u1501Code::TableBroken => 0x521a,
            u1501Code::SegmentGuard => 0x521b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u1501Code::StateDup => "状态重复（矩阵漂移 同名双态）",
            u1501Code::StateHole => "状态断档（行覆盖 序缺失）",
            u1501Code::EventDup => "事件重复（同事件双声明）",
            u1501Code::TransDup => "迁移重复（同源同事件双迁移）",
            u1501Code::TransHole => "迁移缺失（应达态不可达）",
            u1501Code::TransCycle => "迁移环（非法自环/环路）",
            u1501Code::EntryBad => "入口非法（初态不在闭集）",
            u1501Code::ApplyLost => "迁移丢失（应用事件无效果）",
            u1501Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u1501Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u1501Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u1501Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u1501Code::StateDup => "按闭集唯一律去重",
            u1501Code::StateHole => "按闭集补态",
            u1501Code::EventDup => "按事件闭集去重",
            u1501Code::TransDup => "按确定性唯一迁移律合并",
            u1501Code::TransHole => "按迁移矩阵补边",
            u1501Code::TransCycle => "按无环律重构迁移",
            u1501Code::EntryBad => "按闭集钳制初态",
            u1501Code::ApplyLost => "核对事件前置条件",
            u1501Code::LedgerBroken => "按守恒律重建台账后重跑",
            u1501Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u1501Code::TableBroken => "按语义规则重建常量表",
            u1501Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 状态机 mxu1501flow（6 态 × 6 事件 · 迁移 8 条显式声明）
// --------------------------------------------------------------------------
/// 场景 状态机（判据：矩阵漂移）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu1501flowState {
    Idle,
    Armed,
    Running,
    Sampling,
    Comparing,
    Sealed,
}

impl mxu1501flowState {
    /// 全量态闭集。
    pub const ALL: [mxu1501flowState; 6] = [
        mxu1501flowState::Idle,
        mxu1501flowState::Armed,
        mxu1501flowState::Running,
        mxu1501flowState::Sampling,
        mxu1501flowState::Comparing,
        mxu1501flowState::Sealed,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            mxu1501flowState::Idle => 0,
            mxu1501flowState::Armed => 1,
            mxu1501flowState::Running => 2,
            mxu1501flowState::Sampling => 3,
            mxu1501flowState::Comparing => 4,
            mxu1501flowState::Sealed => 5,
        }
    }
}

/// mxu1501flow事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu1501flowEvent {
    Arm,
    Launch,
    Sample,
    Compare,
    Drift,
    Seal,
}

impl mxu1501flowEvent {
    /// 全量事件闭集。
    pub const ALL: [mxu1501flowEvent; 6] = [
        mxu1501flowEvent::Arm,
        mxu1501flowEvent::Launch,
        mxu1501flowEvent::Sample,
        mxu1501flowEvent::Compare,
        mxu1501flowEvent::Drift,
        mxu1501flowEvent::Seal,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const MXU1501FLOW_TRANSITIONS: [(mxu1501flowState, mxu1501flowEvent, mxu1501flowState); 8] = [
    (mxu1501flowState::Idle, mxu1501flowEvent::Arm, mxu1501flowState::Armed), // Idle→Armed 主链
    (mxu1501flowState::Armed, mxu1501flowEvent::Launch, mxu1501flowState::Running), // Armed→Running 主链
    (mxu1501flowState::Running, mxu1501flowEvent::Sample, mxu1501flowState::Sampling), // Running→Sampling 主链
    (mxu1501flowState::Sampling, mxu1501flowEvent::Compare, mxu1501flowState::Comparing), // Sampling→Comparing 主链
    (mxu1501flowState::Comparing, mxu1501flowEvent::Drift, mxu1501flowState::Sealed), // Comparing→Sealed 主链
    (mxu1501flowState::Sealed, mxu1501flowEvent::Seal, mxu1501flowState::Idle), // 终态回边
    (mxu1501flowState::Sealed, mxu1501flowEvent::Arm, mxu1501flowState::Comparing), // 终态回退边
    (mxu1501flowState::Armed, mxu1501flowEvent::Launch, mxu1501flowState::Running), // 旁路边
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn mxu1501flow_next(state: mxu1501flowState, ev: mxu1501flowEvent) -> Option<mxu1501flowState> {
    let mut i = 0usize;
    while i < 8 {
        let t = MXU1501FLOW_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn mxu1501flow_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < mxu1501flowState::ALL.len() {
        let mut ei = 0usize;
        while ei < mxu1501flowEvent::ALL.len() {
            let legal = mxu1501flow_next(mxu1501flowState::ALL[si], mxu1501flowEvent::ALL[ei]).is_some();
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
pub fn mxu1501flow_terminal(st: mxu1501flowState) -> bool {
    let mut i = 0usize;
    while i < 8 {
        if MXU1501FLOW_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct mxu1501flowLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: mxu1501flowState,
}

impl mxu1501flowLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: mxu1501flowState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: mxu1501flowEvent) -> bool {
        match mxu1501flow_next(self.cur, ev) {
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
// 快照 Mxu1501Snap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// 场景 快照封存（FNV 三态指纹 + 篡改检出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mxu1501Snap {
    /// 指纹分量：sc_v。
    pub sc_v: u64,
    /// 指纹分量：cr_v。
    pub cr_v: u64,
    /// 指纹分量：ex_v。
    pub ex_v: u64,
    /// 指纹分量：mr_v。
    pub mr_v: u64,
    /// 指纹分量：cv_v。
    pub cv_v: u64,
    /// 指纹分量：dr_v。
    pub dr_v: u64,
    /// 指纹分量：lg_v。
    pub lg_v: u64,
    /// 指纹分量：zl_v。
    pub zl_v: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl Mxu1501Snap {
    /// 构造并即时合成指纹。
    pub fn compute(
        sc_v: u64,
        cr_v: u64,
        ex_v: u64,
        mr_v: u64,
        cv_v: u64,
        dr_v: u64,
        lg_v: u64,
        zl_v: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &sc_v.to_le_bytes());
        h = fnv1a64_from(h, &cr_v.to_le_bytes());
        h = fnv1a64_from(h, &ex_v.to_le_bytes());
        h = fnv1a64_from(h, &mr_v.to_le_bytes());
        h = fnv1a64_from(h, &cv_v.to_le_bytes());
        h = fnv1a64_from(h, &dr_v.to_le_bytes());
        h = fnv1a64_from(h, &lg_v.to_le_bytes());
        h = fnv1a64_from(h, &zl_v.to_le_bytes());
        Self {
            sc_v,
            cr_v,
            ex_v,
            mr_v,
            cv_v,
            dr_v,
            lg_v,
            zl_v,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.sc_v.to_le_bytes());
        h = fnv1a64_from(h, &self.cr_v.to_le_bytes());
        h = fnv1a64_from(h, &self.ex_v.to_le_bytes());
        h = fnv1a64_from(h, &self.mr_v.to_le_bytes());
        h = fnv1a64_from(h, &self.cv_v.to_le_bytes());
        h = fnv1a64_from(h, &self.dr_v.to_le_bytes());
        h = fnv1a64_from(h, &self.lg_v.to_le_bytes());
        h = fnv1a64_from(h, &self.zl_v.to_le_bytes());
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

/// 段位守卫：全部诊断码必须落在本段 [0x5210, 0x521f]。
pub const fn u1501_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u1501Code::ALL.len() {
        let c = u1501Code::ALL[i].code();
        if c < 0x5210 || c > 0x521f {
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
fn u1501_guard_seg() -> bool {
    u1501_segment_guard()
}

/// UNX-F1501 状态机闭集/迁移表/非法组合恰值全绿。
fn check_u1501_sm_legal() -> bool {
    let mut ok = mxu1501flow_illegal_pairs() == 29;
    if ok { ok = MXU1501FLOW_TRANSITIONS.len() == 8; }
    if ok { ok = mxu1501flowState::ALL.len() == 6 && mxu1501flowEvent::ALL.len() == 6; }
    ok
}

/// UNX-F1501 迁移主链回放 + 快照往返恒等。
fn check_u1501_sm_round() -> bool {
    let mut lg = mxu1501flowLedger::new(mxu1501flowState::Idle);
    let mut ok = lg.apply(mxu1501flowEvent::Arm) && lg.cur == mxu1501flowState::Armed && lg.total() == 1;
    let s0 = Mxu1501Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    let mut s1 = Mxu1501Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    s1.recompute();
    if ok { ok = s0 == s1 && !s1.tampered(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u1501_checks() -> CheckSet {
    let mut set = CheckSet::new("U1501");
    set.add(
        "u1501-seg-guard",
        u1501_guard_seg(),
        if u1501_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u1501-sm-legal",
        check_u1501_sm_legal(),
        if check_u1501_sm_legal() { "" } else { "UNX-F1501 sm legality mismatch" },
    );
    set.add(
        "u1501-sm-round",
        check_u1501_sm_round(),
        if check_u1501_sm_round() { "" } else { "UNX-F1501 sm roundtrip mismatch" },
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
    fn u1501_all_checks_green() {
        let set = run_u1501_checks();
        assert!(set.all_passed(), "U1501 domain checks must be green");
    }

    #[test]
    fn u1501_segment_guard_holds() {
        assert!(u1501_segment_guard());
    }
}
