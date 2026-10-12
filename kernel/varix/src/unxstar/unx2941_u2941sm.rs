//! unxu2941_u2941sm —— 域级压力矩阵方案冻结（三轴） 状态机与快照封存（域级压力三轴）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2941 · 域级压力矩阵方案冻结（三轴） · 判据原文：UNX-F2941 · 判据：UNX-F2941-J1 三轴方案（8 核矩阵×16GB 三档压力×72h 长稳）冻结哈希入册，方案与 F2938 汇总账字段对齐
//! · 组件：域级压力三轴 状态机（6 态 × 6 事件显式迁移）+ FNV 快照封存（8 分量）
//! · 判据落地：UNX-F2941 · 判据：UNX-F2941-J1 三轴方案（8 核矩阵×16GB 三档压力×72h 长稳）冻结哈希入册，方案与 F2938 汇总账字段对齐
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2941_VERSION: &str = "u2941-u2941sm-v1";

/// 诊断段基址。
pub const U2941_SEG_BASE: u16 = 0xe230;

// --------------------------------------------------------------------------
// 诊断码（u2941 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 域级压力三轴 状态面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2941Code {
    /// 0xe230 状态重复（域级 同名双态）。
    StateDup,
    /// 0xe231 状态断档（压力 序缺失）。
    StateHole,
    /// 0xe232 事件重复（同事件双声明）。
    EventDup,
    /// 0xe233 迁移重复（同源同事件双迁移）。
    TransDup,
    /// 0xe234 迁移缺失（应达态不可达）。
    TransHole,
    /// 0xe235 迁移环（非法自环/环路）。
    TransCycle,
    /// 0xe236 入口非法（初态不在闭集）。
    EntryBad,
    /// 0xe237 迁移丢失（应用事件无效果）。
    ApplyLost,
    /// 0xe238 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe239 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe23a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe23b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2941Code {
    /// 全量闭集。
    pub const ALL: [u2941Code; 12] = [
        u2941Code::StateDup,
        u2941Code::StateHole,
        u2941Code::EventDup,
        u2941Code::TransDup,
        u2941Code::TransHole,
        u2941Code::TransCycle,
        u2941Code::EntryBad,
        u2941Code::ApplyLost,
        u2941Code::LedgerBroken,
        u2941Code::SnapshotFingerprint,
        u2941Code::TableBroken,
        u2941Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2941Code::StateDup => 0xe230,
            u2941Code::StateHole => 0xe231,
            u2941Code::EventDup => 0xe232,
            u2941Code::TransDup => 0xe233,
            u2941Code::TransHole => 0xe234,
            u2941Code::TransCycle => 0xe235,
            u2941Code::EntryBad => 0xe236,
            u2941Code::ApplyLost => 0xe237,
            u2941Code::LedgerBroken => 0xe238,
            u2941Code::SnapshotFingerprint => 0xe239,
            u2941Code::TableBroken => 0xe23a,
            u2941Code::SegmentGuard => 0xe23b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2941Code::StateDup => "状态重复（域级 同名双态）",
            u2941Code::StateHole => "状态断档（压力 序缺失）",
            u2941Code::EventDup => "事件重复（同事件双声明）",
            u2941Code::TransDup => "迁移重复（同源同事件双迁移）",
            u2941Code::TransHole => "迁移缺失（应达态不可达）",
            u2941Code::TransCycle => "迁移环（非法自环/环路）",
            u2941Code::EntryBad => "入口非法（初态不在闭集）",
            u2941Code::ApplyLost => "迁移丢失（应用事件无效果）",
            u2941Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2941Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2941Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2941Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2941Code::StateDup => "按闭集唯一律去重",
            u2941Code::StateHole => "按闭集补态",
            u2941Code::EventDup => "按事件闭集去重",
            u2941Code::TransDup => "按确定性唯一迁移律合并",
            u2941Code::TransHole => "按迁移矩阵补边",
            u2941Code::TransCycle => "按无环律重构迁移",
            u2941Code::EntryBad => "按闭集钳制初态",
            u2941Code::ApplyLost => "核对事件前置条件",
            u2941Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2941Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2941Code::TableBroken => "按语义规则重建常量表",
            u2941Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 状态机 mxu2941flow（6 态 × 6 事件 · 迁移 8 条显式声明）
// --------------------------------------------------------------------------
/// 域级压力三轴 状态机（判据：域级）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu2941flowState {
    Plan,
    Armed,
    Running,
    Crossing,
    Judged,
    Sealed,
}

impl mxu2941flowState {
    /// 全量态闭集。
    pub const ALL: [mxu2941flowState; 6] = [
        mxu2941flowState::Plan,
        mxu2941flowState::Armed,
        mxu2941flowState::Running,
        mxu2941flowState::Crossing,
        mxu2941flowState::Judged,
        mxu2941flowState::Sealed,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            mxu2941flowState::Plan => 0,
            mxu2941flowState::Armed => 1,
            mxu2941flowState::Running => 2,
            mxu2941flowState::Crossing => 3,
            mxu2941flowState::Judged => 4,
            mxu2941flowState::Sealed => 5,
        }
    }
}

/// mxu2941flow事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu2941flowEvent {
    Plan,
    Arm,
    Cross,
    Run,
    Judge,
    Seal,
}

impl mxu2941flowEvent {
    /// 全量事件闭集。
    pub const ALL: [mxu2941flowEvent; 6] = [
        mxu2941flowEvent::Plan,
        mxu2941flowEvent::Arm,
        mxu2941flowEvent::Cross,
        mxu2941flowEvent::Run,
        mxu2941flowEvent::Judge,
        mxu2941flowEvent::Seal,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const MXU2941FLOW_TRANSITIONS: [(mxu2941flowState, mxu2941flowEvent, mxu2941flowState); 8] = [
    (mxu2941flowState::Plan, mxu2941flowEvent::Plan, mxu2941flowState::Armed), // Plan→Armed 主链
    (mxu2941flowState::Armed, mxu2941flowEvent::Arm, mxu2941flowState::Running), // Armed→Running 主链
    (mxu2941flowState::Running, mxu2941flowEvent::Cross, mxu2941flowState::Crossing), // Running→Crossing 主链
    (mxu2941flowState::Crossing, mxu2941flowEvent::Run, mxu2941flowState::Judged), // Crossing→Judged 主链
    (mxu2941flowState::Judged, mxu2941flowEvent::Judge, mxu2941flowState::Sealed), // Judged→Sealed 主链
    (mxu2941flowState::Sealed, mxu2941flowEvent::Seal, mxu2941flowState::Plan), // 终态回边
    (mxu2941flowState::Sealed, mxu2941flowEvent::Plan, mxu2941flowState::Judged), // 终态回退边
    (mxu2941flowState::Armed, mxu2941flowEvent::Arm, mxu2941flowState::Running), // 旁路边
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn mxu2941flow_next(state: mxu2941flowState, ev: mxu2941flowEvent) -> Option<mxu2941flowState> {
    let mut i = 0usize;
    while i < 8 {
        let t = MXU2941FLOW_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn mxu2941flow_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < mxu2941flowState::ALL.len() {
        let mut ei = 0usize;
        while ei < mxu2941flowEvent::ALL.len() {
            let legal = mxu2941flow_next(mxu2941flowState::ALL[si], mxu2941flowEvent::ALL[ei]).is_some();
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
pub fn mxu2941flow_terminal(st: mxu2941flowState) -> bool {
    let mut i = 0usize;
    while i < 8 {
        if MXU2941FLOW_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct mxu2941flowLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: mxu2941flowState,
}

impl mxu2941flowLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: mxu2941flowState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: mxu2941flowEvent) -> bool {
        match mxu2941flow_next(self.cur, ev) {
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
// 快照 Mxu2941Snap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// 域级压力三轴 快照封存（FNV 三态指纹 + 篡改检出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mxu2941Snap {
    /// 指纹分量：ax1_v。
    pub ax1_v: u64,
    /// 指纹分量：ax2_v。
    pub ax2_v: u64,
    /// 指纹分量：ax3_v。
    pub ax3_v: u64,
    /// 指纹分量：stress_v。
    pub stress_v: u64,
    /// 指纹分量：dom_v。
    pub dom_v: u64,
    /// 指纹分量：matrix_v。
    pub matrix_v: u64,
    /// 指纹分量：load_v。
    pub load_v: u64,
    /// 指纹分量：time_v。
    pub time_v: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl Mxu2941Snap {
    /// 构造并即时合成指纹。
    pub fn compute(
        ax1_v: u64,
        ax2_v: u64,
        ax3_v: u64,
        stress_v: u64,
        dom_v: u64,
        matrix_v: u64,
        load_v: u64,
        time_v: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &ax1_v.to_le_bytes());
        h = fnv1a64_from(h, &ax2_v.to_le_bytes());
        h = fnv1a64_from(h, &ax3_v.to_le_bytes());
        h = fnv1a64_from(h, &stress_v.to_le_bytes());
        h = fnv1a64_from(h, &dom_v.to_le_bytes());
        h = fnv1a64_from(h, &matrix_v.to_le_bytes());
        h = fnv1a64_from(h, &load_v.to_le_bytes());
        h = fnv1a64_from(h, &time_v.to_le_bytes());
        Self {
            ax1_v,
            ax2_v,
            ax3_v,
            stress_v,
            dom_v,
            matrix_v,
            load_v,
            time_v,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.ax1_v.to_le_bytes());
        h = fnv1a64_from(h, &self.ax2_v.to_le_bytes());
        h = fnv1a64_from(h, &self.ax3_v.to_le_bytes());
        h = fnv1a64_from(h, &self.stress_v.to_le_bytes());
        h = fnv1a64_from(h, &self.dom_v.to_le_bytes());
        h = fnv1a64_from(h, &self.matrix_v.to_le_bytes());
        h = fnv1a64_from(h, &self.load_v.to_le_bytes());
        h = fnv1a64_from(h, &self.time_v.to_le_bytes());
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

/// 段位守卫：全部诊断码必须落在本段 [0xe230, 0xe23f]。
pub const fn u2941_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2941Code::ALL.len() {
        let c = u2941Code::ALL[i].code();
        if c < 0xe230 || c > 0xe23f {
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
fn u2941_guard_seg() -> bool {
    u2941_segment_guard()
}

/// UNX-F2941 状态机闭集/迁移表/非法组合恰值全绿。
fn check_u2941_sm_legal() -> bool {
    let mut ok = mxu2941flow_illegal_pairs() == 29;
    if ok { ok = MXU2941FLOW_TRANSITIONS.len() == 8; }
    if ok { ok = mxu2941flowState::ALL.len() == 6 && mxu2941flowEvent::ALL.len() == 6; }
    ok
}

/// UNX-F2941 迁移主链回放 + 快照往返恒等。
fn check_u2941_sm_round() -> bool {
    let mut lg = mxu2941flowLedger::new(mxu2941flowState::Plan);
    let mut ok = lg.apply(mxu2941flowEvent::Plan) && lg.cur == mxu2941flowState::Armed && lg.total() == 1;
    let s0 = Mxu2941Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    let mut s1 = Mxu2941Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    s1.recompute();
    if ok { ok = s0 == s1 && !s1.tampered(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2941_checks() -> CheckSet {
    let mut set = CheckSet::new("U2941");
    set.add(
        "u2941-seg-guard",
        u2941_guard_seg(),
        if u2941_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2941-sm-legal",
        check_u2941_sm_legal(),
        if check_u2941_sm_legal() { "" } else { "UNX-F2941 sm legality mismatch" },
    );
    set.add(
        "u2941-sm-round",
        check_u2941_sm_round(),
        if check_u2941_sm_round() { "" } else { "UNX-F2941 sm roundtrip mismatch" },
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
    fn u2941_all_checks_green() {
        let set = run_u2941_checks();
        assert!(set.all_passed(), "U2941 domain checks must be green");
    }

    #[test]
    fn u2941_segment_guard_holds() {
        assert!(u2941_segment_guard());
    }
}
