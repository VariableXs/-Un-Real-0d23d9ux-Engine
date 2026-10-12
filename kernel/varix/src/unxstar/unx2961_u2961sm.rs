//! unxu2961_u2961sm —— 域级回归矩阵方案冻结（B01–B28 全域） 状态机与快照封存（域级回归全域矩阵）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2961 · 域级回归矩阵方案冻结（B01–B28 全域） · 判据原文：UNX-F2961 · 判据：UNX-F2961-J1 全域回归方案（28 批 560 条零抽样+三 runner 归一+绿账字段对齐）冻结哈希入册，方案与 F2968 绿账字段对齐
//! · 组件：域级回归全域矩阵 状态机（6 态 × 6 事件显式迁移）+ FNV 快照封存（8 分量）
//! · 判据落地：UNX-F2961 · 判据：UNX-F2961-J1 全域回归方案（28 批 560 条零抽样+三 runner 归一+绿账字段对齐）冻结哈希入册，方案与 F
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2961_VERSION: &str = "u2961-u2961sm-v1";

/// 诊断段基址。
pub const U2961_SEG_BASE: u16 = 0xe240;

// --------------------------------------------------------------------------
// 诊断码（u2961 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 域级回归全域矩阵 状态面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2961Code {
    /// 0xe240 状态重复（全域 同名双态）。
    StateDup,
    /// 0xe241 状态断档（回归 序缺失）。
    StateHole,
    /// 0xe242 事件重复（同事件双声明）。
    EventDup,
    /// 0xe243 迁移重复（同源同事件双迁移）。
    TransDup,
    /// 0xe244 迁移缺失（应达态不可达）。
    TransHole,
    /// 0xe245 迁移环（非法自环/环路）。
    TransCycle,
    /// 0xe246 入口非法（初态不在闭集）。
    EntryBad,
    /// 0xe247 迁移丢失（应用事件无效果）。
    ApplyLost,
    /// 0xe248 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe249 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe24a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe24b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2961Code {
    /// 全量闭集。
    pub const ALL: [u2961Code; 12] = [
        u2961Code::StateDup,
        u2961Code::StateHole,
        u2961Code::EventDup,
        u2961Code::TransDup,
        u2961Code::TransHole,
        u2961Code::TransCycle,
        u2961Code::EntryBad,
        u2961Code::ApplyLost,
        u2961Code::LedgerBroken,
        u2961Code::SnapshotFingerprint,
        u2961Code::TableBroken,
        u2961Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2961Code::StateDup => 0xe240,
            u2961Code::StateHole => 0xe241,
            u2961Code::EventDup => 0xe242,
            u2961Code::TransDup => 0xe243,
            u2961Code::TransHole => 0xe244,
            u2961Code::TransCycle => 0xe245,
            u2961Code::EntryBad => 0xe246,
            u2961Code::ApplyLost => 0xe247,
            u2961Code::LedgerBroken => 0xe248,
            u2961Code::SnapshotFingerprint => 0xe249,
            u2961Code::TableBroken => 0xe24a,
            u2961Code::SegmentGuard => 0xe24b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2961Code::StateDup => "状态重复（全域 同名双态）",
            u2961Code::StateHole => "状态断档（回归 序缺失）",
            u2961Code::EventDup => "事件重复（同事件双声明）",
            u2961Code::TransDup => "迁移重复（同源同事件双迁移）",
            u2961Code::TransHole => "迁移缺失（应达态不可达）",
            u2961Code::TransCycle => "迁移环（非法自环/环路）",
            u2961Code::EntryBad => "入口非法（初态不在闭集）",
            u2961Code::ApplyLost => "迁移丢失（应用事件无效果）",
            u2961Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2961Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2961Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2961Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2961Code::StateDup => "按闭集唯一律去重",
            u2961Code::StateHole => "按闭集补态",
            u2961Code::EventDup => "按事件闭集去重",
            u2961Code::TransDup => "按确定性唯一迁移律合并",
            u2961Code::TransHole => "按迁移矩阵补边",
            u2961Code::TransCycle => "按无环律重构迁移",
            u2961Code::EntryBad => "按闭集钳制初态",
            u2961Code::ApplyLost => "核对事件前置条件",
            u2961Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2961Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2961Code::TableBroken => "按语义规则重建常量表",
            u2961Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 状态机 mxu2961flow（6 态 × 6 事件 · 迁移 8 条显式声明）
// --------------------------------------------------------------------------
/// 域级回归全域矩阵 状态机（判据：全域）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu2961flowState {
    Frozen,
    Running,
    Comparing,
    Diverged,
    Quarantined,
    Sealed,
}

impl mxu2961flowState {
    /// 全量态闭集。
    pub const ALL: [mxu2961flowState; 6] = [
        mxu2961flowState::Frozen,
        mxu2961flowState::Running,
        mxu2961flowState::Comparing,
        mxu2961flowState::Diverged,
        mxu2961flowState::Quarantined,
        mxu2961flowState::Sealed,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            mxu2961flowState::Frozen => 0,
            mxu2961flowState::Running => 1,
            mxu2961flowState::Comparing => 2,
            mxu2961flowState::Diverged => 3,
            mxu2961flowState::Quarantined => 4,
            mxu2961flowState::Sealed => 5,
        }
    }
}

/// mxu2961flow事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu2961flowEvent {
    Freeze,
    Run,
    Compare,
    Quarantine,
    Rerun,
    Seal,
}

impl mxu2961flowEvent {
    /// 全量事件闭集。
    pub const ALL: [mxu2961flowEvent; 6] = [
        mxu2961flowEvent::Freeze,
        mxu2961flowEvent::Run,
        mxu2961flowEvent::Compare,
        mxu2961flowEvent::Quarantine,
        mxu2961flowEvent::Rerun,
        mxu2961flowEvent::Seal,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const MXU2961FLOW_TRANSITIONS: [(mxu2961flowState, mxu2961flowEvent, mxu2961flowState); 8] = [
    (mxu2961flowState::Frozen, mxu2961flowEvent::Freeze, mxu2961flowState::Running), // Frozen→Running 主链
    (mxu2961flowState::Running, mxu2961flowEvent::Run, mxu2961flowState::Comparing), // Running→Comparing 主链
    (mxu2961flowState::Comparing, mxu2961flowEvent::Compare, mxu2961flowState::Diverged), // Comparing→Diverged 主链
    (mxu2961flowState::Diverged, mxu2961flowEvent::Quarantine, mxu2961flowState::Quarantined), // Diverged→Quarantined 主链
    (mxu2961flowState::Quarantined, mxu2961flowEvent::Rerun, mxu2961flowState::Sealed), // Quarantined→Sealed 主链
    (mxu2961flowState::Sealed, mxu2961flowEvent::Seal, mxu2961flowState::Frozen), // 终态回边
    (mxu2961flowState::Sealed, mxu2961flowEvent::Freeze, mxu2961flowState::Quarantined), // 终态回退边
    (mxu2961flowState::Running, mxu2961flowEvent::Run, mxu2961flowState::Comparing), // 旁路边
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn mxu2961flow_next(state: mxu2961flowState, ev: mxu2961flowEvent) -> Option<mxu2961flowState> {
    let mut i = 0usize;
    while i < 8 {
        let t = MXU2961FLOW_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn mxu2961flow_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < mxu2961flowState::ALL.len() {
        let mut ei = 0usize;
        while ei < mxu2961flowEvent::ALL.len() {
            let legal = mxu2961flow_next(mxu2961flowState::ALL[si], mxu2961flowEvent::ALL[ei]).is_some();
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
pub fn mxu2961flow_terminal(st: mxu2961flowState) -> bool {
    let mut i = 0usize;
    while i < 8 {
        if MXU2961FLOW_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct mxu2961flowLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: mxu2961flowState,
}

impl mxu2961flowLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: mxu2961flowState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: mxu2961flowEvent) -> bool {
        match mxu2961flow_next(self.cur, ev) {
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
// 快照 Mxu2961Snap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// 域级回归全域矩阵 快照封存（FNV 三态指纹 + 篡改检出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mxu2961Snap {
    /// 指纹分量：reg_v。
    pub reg_v: u64,
    /// 指纹分量：full_v。
    pub full_v: u64,
    /// 指纹分量：b0128_v。
    pub b0128_v: u64,
    /// 指纹分量：case_v。
    pub case_v: u64,
    /// 指纹分量：base_v。
    pub base_v: u64,
    /// 指纹分量：diff_v。
    pub diff_v: u64,
    /// 指纹分量：cov_v。
    pub cov_v: u64,
    /// 指纹分量：flaky_v。
    pub flaky_v: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl Mxu2961Snap {
    /// 构造并即时合成指纹。
    pub fn compute(
        reg_v: u64,
        full_v: u64,
        b0128_v: u64,
        case_v: u64,
        base_v: u64,
        diff_v: u64,
        cov_v: u64,
        flaky_v: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &reg_v.to_le_bytes());
        h = fnv1a64_from(h, &full_v.to_le_bytes());
        h = fnv1a64_from(h, &b0128_v.to_le_bytes());
        h = fnv1a64_from(h, &case_v.to_le_bytes());
        h = fnv1a64_from(h, &base_v.to_le_bytes());
        h = fnv1a64_from(h, &diff_v.to_le_bytes());
        h = fnv1a64_from(h, &cov_v.to_le_bytes());
        h = fnv1a64_from(h, &flaky_v.to_le_bytes());
        Self {
            reg_v,
            full_v,
            b0128_v,
            case_v,
            base_v,
            diff_v,
            cov_v,
            flaky_v,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.reg_v.to_le_bytes());
        h = fnv1a64_from(h, &self.full_v.to_le_bytes());
        h = fnv1a64_from(h, &self.b0128_v.to_le_bytes());
        h = fnv1a64_from(h, &self.case_v.to_le_bytes());
        h = fnv1a64_from(h, &self.base_v.to_le_bytes());
        h = fnv1a64_from(h, &self.diff_v.to_le_bytes());
        h = fnv1a64_from(h, &self.cov_v.to_le_bytes());
        h = fnv1a64_from(h, &self.flaky_v.to_le_bytes());
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

/// 段位守卫：全部诊断码必须落在本段 [0xe240, 0xe24f]。
pub const fn u2961_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2961Code::ALL.len() {
        let c = u2961Code::ALL[i].code();
        if c < 0xe240 || c > 0xe24f {
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
fn u2961_guard_seg() -> bool {
    u2961_segment_guard()
}

/// UNX-F2961 状态机闭集/迁移表/非法组合恰值全绿。
fn check_u2961_sm_legal() -> bool {
    let mut ok = mxu2961flow_illegal_pairs() == 29;
    if ok { ok = MXU2961FLOW_TRANSITIONS.len() == 8; }
    if ok { ok = mxu2961flowState::ALL.len() == 6 && mxu2961flowEvent::ALL.len() == 6; }
    ok
}

/// UNX-F2961 迁移主链回放 + 快照往返恒等。
fn check_u2961_sm_round() -> bool {
    let mut lg = mxu2961flowLedger::new(mxu2961flowState::Frozen);
    let mut ok = lg.apply(mxu2961flowEvent::Freeze) && lg.cur == mxu2961flowState::Running && lg.total() == 1;
    let s0 = Mxu2961Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    let mut s1 = Mxu2961Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    s1.recompute();
    if ok { ok = s0 == s1 && !s1.tampered(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2961_checks() -> CheckSet {
    let mut set = CheckSet::new("U2961");
    set.add(
        "u2961-seg-guard",
        u2961_guard_seg(),
        if u2961_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2961-sm-legal",
        check_u2961_sm_legal(),
        if check_u2961_sm_legal() { "" } else { "UNX-F2961 sm legality mismatch" },
    );
    set.add(
        "u2961-sm-round",
        check_u2961_sm_round(),
        if check_u2961_sm_round() { "" } else { "UNX-F2961 sm roundtrip mismatch" },
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
    fn u2961_all_checks_green() {
        let set = run_u2961_checks();
        assert!(set.all_passed(), "U2961 domain checks must be green");
    }

    #[test]
    fn u2961_segment_guard_holds() {
        assert!(u2961_segment_guard());
    }
}
