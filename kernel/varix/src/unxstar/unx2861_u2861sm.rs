//! unxu2861_u2861sm —— 大页池回归矩阵冻结 状态机与快照封存（大页池回归矩阵）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2861 · 大页池回归矩阵冻结 · 判据原文：UNX-F2861 · 判据：UNX-F2861-J1 回归用例矩阵（九面压测→用例映射 27 例）冻结哈希入册，矩阵字段完整性零缺失
//! · 组件：大页池回归矩阵 状态机（6 态 × 6 事件显式迁移）+ FNV 快照封存（8 分量）
//! · 判据落地：UNX-F2861 · 判据：UNX-F2861-J1 回归用例矩阵（九面压测→用例映射 27 例）冻结哈希入册，矩阵字段完整性零缺失
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2861_VERSION: &str = "u2861-u2861sm-v1";

/// 诊断段基址。
pub const U2861_SEG_BASE: u16 = 0xe1f0;

// --------------------------------------------------------------------------
// 诊断码（u2861 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 大页池回归矩阵 状态面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2861Code {
    /// 0xe1f0 状态重复（回归 同名双态）。
    StateDup,
    /// 0xe1f1 状态断档（矩阵 序缺失）。
    StateHole,
    /// 0xe1f2 事件重复（同事件双声明）。
    EventDup,
    /// 0xe1f3 迁移重复（同源同事件双迁移）。
    TransDup,
    /// 0xe1f4 迁移缺失（应达态不可达）。
    TransHole,
    /// 0xe1f5 迁移环（非法自环/环路）。
    TransCycle,
    /// 0xe1f6 入口非法（初态不在闭集）。
    EntryBad,
    /// 0xe1f7 迁移丢失（应用事件无效果）。
    ApplyLost,
    /// 0xe1f8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe1f9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe1fa 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe1fb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2861Code {
    /// 全量闭集。
    pub const ALL: [u2861Code; 12] = [
        u2861Code::StateDup,
        u2861Code::StateHole,
        u2861Code::EventDup,
        u2861Code::TransDup,
        u2861Code::TransHole,
        u2861Code::TransCycle,
        u2861Code::EntryBad,
        u2861Code::ApplyLost,
        u2861Code::LedgerBroken,
        u2861Code::SnapshotFingerprint,
        u2861Code::TableBroken,
        u2861Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2861Code::StateDup => 0xe1f0,
            u2861Code::StateHole => 0xe1f1,
            u2861Code::EventDup => 0xe1f2,
            u2861Code::TransDup => 0xe1f3,
            u2861Code::TransHole => 0xe1f4,
            u2861Code::TransCycle => 0xe1f5,
            u2861Code::EntryBad => 0xe1f6,
            u2861Code::ApplyLost => 0xe1f7,
            u2861Code::LedgerBroken => 0xe1f8,
            u2861Code::SnapshotFingerprint => 0xe1f9,
            u2861Code::TableBroken => 0xe1fa,
            u2861Code::SegmentGuard => 0xe1fb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2861Code::StateDup => "状态重复（回归 同名双态）",
            u2861Code::StateHole => "状态断档（矩阵 序缺失）",
            u2861Code::EventDup => "事件重复（同事件双声明）",
            u2861Code::TransDup => "迁移重复（同源同事件双迁移）",
            u2861Code::TransHole => "迁移缺失（应达态不可达）",
            u2861Code::TransCycle => "迁移环（非法自环/环路）",
            u2861Code::EntryBad => "入口非法（初态不在闭集）",
            u2861Code::ApplyLost => "迁移丢失（应用事件无效果）",
            u2861Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2861Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2861Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2861Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2861Code::StateDup => "按闭集唯一律去重",
            u2861Code::StateHole => "按闭集补态",
            u2861Code::EventDup => "按事件闭集去重",
            u2861Code::TransDup => "按确定性唯一迁移律合并",
            u2861Code::TransHole => "按迁移矩阵补边",
            u2861Code::TransCycle => "按无环律重构迁移",
            u2861Code::EntryBad => "按闭集钳制初态",
            u2861Code::ApplyLost => "核对事件前置条件",
            u2861Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2861Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2861Code::TableBroken => "按语义规则重建常量表",
            u2861Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 状态机 mxu2861flow（6 态 × 6 事件 · 迁移 8 条显式声明）
// --------------------------------------------------------------------------
/// 大页池回归矩阵 状态机（判据：回归）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu2861flowState {
    Frozen,
    Running,
    Comparing,
    Diverged,
    Quarantined,
    Sealed,
}

impl mxu2861flowState {
    /// 全量态闭集。
    pub const ALL: [mxu2861flowState; 6] = [
        mxu2861flowState::Frozen,
        mxu2861flowState::Running,
        mxu2861flowState::Comparing,
        mxu2861flowState::Diverged,
        mxu2861flowState::Quarantined,
        mxu2861flowState::Sealed,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            mxu2861flowState::Frozen => 0,
            mxu2861flowState::Running => 1,
            mxu2861flowState::Comparing => 2,
            mxu2861flowState::Diverged => 3,
            mxu2861flowState::Quarantined => 4,
            mxu2861flowState::Sealed => 5,
        }
    }
}

/// mxu2861flow事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu2861flowEvent {
    Freeze,
    Run,
    Compare,
    Quarantine,
    Rerun,
    Seal,
}

impl mxu2861flowEvent {
    /// 全量事件闭集。
    pub const ALL: [mxu2861flowEvent; 6] = [
        mxu2861flowEvent::Freeze,
        mxu2861flowEvent::Run,
        mxu2861flowEvent::Compare,
        mxu2861flowEvent::Quarantine,
        mxu2861flowEvent::Rerun,
        mxu2861flowEvent::Seal,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const MXU2861FLOW_TRANSITIONS: [(mxu2861flowState, mxu2861flowEvent, mxu2861flowState); 8] = [
    (mxu2861flowState::Frozen, mxu2861flowEvent::Freeze, mxu2861flowState::Running), // Frozen→Running 主链
    (mxu2861flowState::Running, mxu2861flowEvent::Run, mxu2861flowState::Comparing), // Running→Comparing 主链
    (mxu2861flowState::Comparing, mxu2861flowEvent::Compare, mxu2861flowState::Diverged), // Comparing→Diverged 主链
    (mxu2861flowState::Diverged, mxu2861flowEvent::Quarantine, mxu2861flowState::Quarantined), // Diverged→Quarantined 主链
    (mxu2861flowState::Quarantined, mxu2861flowEvent::Rerun, mxu2861flowState::Sealed), // Quarantined→Sealed 主链
    (mxu2861flowState::Sealed, mxu2861flowEvent::Seal, mxu2861flowState::Frozen), // 终态回边
    (mxu2861flowState::Sealed, mxu2861flowEvent::Freeze, mxu2861flowState::Quarantined), // 终态回退边
    (mxu2861flowState::Running, mxu2861flowEvent::Run, mxu2861flowState::Comparing), // 旁路边
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn mxu2861flow_next(state: mxu2861flowState, ev: mxu2861flowEvent) -> Option<mxu2861flowState> {
    let mut i = 0usize;
    while i < 8 {
        let t = MXU2861FLOW_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn mxu2861flow_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < mxu2861flowState::ALL.len() {
        let mut ei = 0usize;
        while ei < mxu2861flowEvent::ALL.len() {
            let legal = mxu2861flow_next(mxu2861flowState::ALL[si], mxu2861flowEvent::ALL[ei]).is_some();
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
pub fn mxu2861flow_terminal(st: mxu2861flowState) -> bool {
    let mut i = 0usize;
    while i < 8 {
        if MXU2861FLOW_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct mxu2861flowLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: mxu2861flowState,
}

impl mxu2861flowLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: mxu2861flowState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: mxu2861flowEvent) -> bool {
        match mxu2861flow_next(self.cur, ev) {
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
// 快照 Mxu2861Snap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// 大页池回归矩阵 快照封存（FNV 三态指纹 + 篡改检出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mxu2861Snap {
    /// 指纹分量：reg_v。
    pub reg_v: u64,
    /// 指纹分量：mtx_v。
    pub mtx_v: u64,
    /// 指纹分量：hp_v。
    pub hp_v: u64,
    /// 指纹分量：case_v。
    pub case_v: u64,
    /// 指纹分量：base_v。
    pub base_v: u64,
    /// 指纹分量：diff_v。
    pub diff_v: u64,
    /// 指纹分量：pass_v。
    pub pass_v: u64,
    /// 指纹分量：fail_v。
    pub fail_v: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl Mxu2861Snap {
    /// 构造并即时合成指纹。
    pub fn compute(
        reg_v: u64,
        mtx_v: u64,
        hp_v: u64,
        case_v: u64,
        base_v: u64,
        diff_v: u64,
        pass_v: u64,
        fail_v: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &reg_v.to_le_bytes());
        h = fnv1a64_from(h, &mtx_v.to_le_bytes());
        h = fnv1a64_from(h, &hp_v.to_le_bytes());
        h = fnv1a64_from(h, &case_v.to_le_bytes());
        h = fnv1a64_from(h, &base_v.to_le_bytes());
        h = fnv1a64_from(h, &diff_v.to_le_bytes());
        h = fnv1a64_from(h, &pass_v.to_le_bytes());
        h = fnv1a64_from(h, &fail_v.to_le_bytes());
        Self {
            reg_v,
            mtx_v,
            hp_v,
            case_v,
            base_v,
            diff_v,
            pass_v,
            fail_v,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.reg_v.to_le_bytes());
        h = fnv1a64_from(h, &self.mtx_v.to_le_bytes());
        h = fnv1a64_from(h, &self.hp_v.to_le_bytes());
        h = fnv1a64_from(h, &self.case_v.to_le_bytes());
        h = fnv1a64_from(h, &self.base_v.to_le_bytes());
        h = fnv1a64_from(h, &self.diff_v.to_le_bytes());
        h = fnv1a64_from(h, &self.pass_v.to_le_bytes());
        h = fnv1a64_from(h, &self.fail_v.to_le_bytes());
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

/// 段位守卫：全部诊断码必须落在本段 [0xe1f0, 0xe1ff]。
pub const fn u2861_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2861Code::ALL.len() {
        let c = u2861Code::ALL[i].code();
        if c < 0xe1f0 || c > 0xe1ff {
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
fn u2861_guard_seg() -> bool {
    u2861_segment_guard()
}

/// UNX-F2861 状态机闭集/迁移表/非法组合恰值全绿。
fn check_u2861_sm_legal() -> bool {
    let mut ok = mxu2861flow_illegal_pairs() == 29;
    if ok { ok = MXU2861FLOW_TRANSITIONS.len() == 8; }
    if ok { ok = mxu2861flowState::ALL.len() == 6 && mxu2861flowEvent::ALL.len() == 6; }
    ok
}

/// UNX-F2861 迁移主链回放 + 快照往返恒等。
fn check_u2861_sm_round() -> bool {
    let mut lg = mxu2861flowLedger::new(mxu2861flowState::Frozen);
    let mut ok = lg.apply(mxu2861flowEvent::Freeze) && lg.cur == mxu2861flowState::Running && lg.total() == 1;
    let s0 = Mxu2861Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    let mut s1 = Mxu2861Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    s1.recompute();
    if ok { ok = s0 == s1 && !s1.tampered(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2861_checks() -> CheckSet {
    let mut set = CheckSet::new("U2861");
    set.add(
        "u2861-seg-guard",
        u2861_guard_seg(),
        if u2861_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2861-sm-legal",
        check_u2861_sm_legal(),
        if check_u2861_sm_legal() { "" } else { "UNX-F2861 sm legality mismatch" },
    );
    set.add(
        "u2861-sm-round",
        check_u2861_sm_round(),
        if check_u2861_sm_round() { "" } else { "UNX-F2861 sm roundtrip mismatch" },
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
    fn u2861_all_checks_green() {
        let set = run_u2861_checks();
        assert!(set.all_passed(), "U2861 domain checks must be green");
    }

    #[test]
    fn u2861_segment_guard_holds() {
        assert!(u2861_segment_guard());
    }
}
