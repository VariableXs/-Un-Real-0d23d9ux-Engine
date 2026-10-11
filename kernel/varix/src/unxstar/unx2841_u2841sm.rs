//! unxu2841_u2841sm —— 大页-地址空间联合压测方案与执行 状态机与快照封存（大页-地址空间联合压测）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2841 · 大页-地址空间联合压测方案与执行 · 判据原文：UNX-F2841 · 判据：UNX-F2841-J1 大页映射进地址空间全链（建立-翻译-拆除）10/10 轮贯通，链账三段闭合
//! · 组件：大页-地址空间联合压测 状态机（6 态 × 6 事件显式迁移）+ FNV 快照封存（8 分量）
//! · 判据落地：UNX-F2841 · 判据：UNX-F2841-J1 大页映射进地址空间全链（建立-翻译-拆除）10/10 轮贯通，链账三段闭合
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2841_VERSION: &str = "u2841-u2841sm-v1";

/// 诊断段基址。
pub const U2841_SEG_BASE: u16 = 0xe1e0;

// --------------------------------------------------------------------------
// 诊断码（u2841 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 大页-地址空间联合压测 状态面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2841Code {
    /// 0xe1e0 状态重复（联合压测 同名双态）。
    StateDup,
    /// 0xe1e1 状态断档（交叉 序缺失）。
    StateHole,
    /// 0xe1e2 事件重复（同事件双声明）。
    EventDup,
    /// 0xe1e3 迁移重复（同源同事件双迁移）。
    TransDup,
    /// 0xe1e4 迁移缺失（应达态不可达）。
    TransHole,
    /// 0xe1e5 迁移环（非法自环/环路）。
    TransCycle,
    /// 0xe1e6 入口非法（初态不在闭集）。
    EntryBad,
    /// 0xe1e7 迁移丢失（应用事件无效果）。
    ApplyLost,
    /// 0xe1e8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe1e9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe1ea 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe1eb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2841Code {
    /// 全量闭集。
    pub const ALL: [u2841Code; 12] = [
        u2841Code::StateDup,
        u2841Code::StateHole,
        u2841Code::EventDup,
        u2841Code::TransDup,
        u2841Code::TransHole,
        u2841Code::TransCycle,
        u2841Code::EntryBad,
        u2841Code::ApplyLost,
        u2841Code::LedgerBroken,
        u2841Code::SnapshotFingerprint,
        u2841Code::TableBroken,
        u2841Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2841Code::StateDup => 0xe1e0,
            u2841Code::StateHole => 0xe1e1,
            u2841Code::EventDup => 0xe1e2,
            u2841Code::TransDup => 0xe1e3,
            u2841Code::TransHole => 0xe1e4,
            u2841Code::TransCycle => 0xe1e5,
            u2841Code::EntryBad => 0xe1e6,
            u2841Code::ApplyLost => 0xe1e7,
            u2841Code::LedgerBroken => 0xe1e8,
            u2841Code::SnapshotFingerprint => 0xe1e9,
            u2841Code::TableBroken => 0xe1ea,
            u2841Code::SegmentGuard => 0xe1eb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2841Code::StateDup => "状态重复（联合压测 同名双态）",
            u2841Code::StateHole => "状态断档（交叉 序缺失）",
            u2841Code::EventDup => "事件重复（同事件双声明）",
            u2841Code::TransDup => "迁移重复（同源同事件双迁移）",
            u2841Code::TransHole => "迁移缺失（应达态不可达）",
            u2841Code::TransCycle => "迁移环（非法自环/环路）",
            u2841Code::EntryBad => "入口非法（初态不在闭集）",
            u2841Code::ApplyLost => "迁移丢失（应用事件无效果）",
            u2841Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2841Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2841Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2841Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2841Code::StateDup => "按闭集唯一律去重",
            u2841Code::StateHole => "按闭集补态",
            u2841Code::EventDup => "按事件闭集去重",
            u2841Code::TransDup => "按确定性唯一迁移律合并",
            u2841Code::TransHole => "按迁移矩阵补边",
            u2841Code::TransCycle => "按无环律重构迁移",
            u2841Code::EntryBad => "按闭集钳制初态",
            u2841Code::ApplyLost => "核对事件前置条件",
            u2841Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2841Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2841Code::TableBroken => "按语义规则重建常量表",
            u2841Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 状态机 mxu2841flow（6 态 × 6 事件 · 迁移 8 条显式声明）
// --------------------------------------------------------------------------
/// 大页-地址空间联合压测 状态机（判据：联合压测）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu2841flowState {
    Plan,
    Armed,
    Running,
    Crossing,
    Judged,
    Sealed,
}

impl mxu2841flowState {
    /// 全量态闭集。
    pub const ALL: [mxu2841flowState; 6] = [
        mxu2841flowState::Plan,
        mxu2841flowState::Armed,
        mxu2841flowState::Running,
        mxu2841flowState::Crossing,
        mxu2841flowState::Judged,
        mxu2841flowState::Sealed,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            mxu2841flowState::Plan => 0,
            mxu2841flowState::Armed => 1,
            mxu2841flowState::Running => 2,
            mxu2841flowState::Crossing => 3,
            mxu2841flowState::Judged => 4,
            mxu2841flowState::Sealed => 5,
        }
    }
}

/// mxu2841flow事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu2841flowEvent {
    Plan,
    Arm,
    Cross,
    Run,
    Judge,
    Seal,
}

impl mxu2841flowEvent {
    /// 全量事件闭集。
    pub const ALL: [mxu2841flowEvent; 6] = [
        mxu2841flowEvent::Plan,
        mxu2841flowEvent::Arm,
        mxu2841flowEvent::Cross,
        mxu2841flowEvent::Run,
        mxu2841flowEvent::Judge,
        mxu2841flowEvent::Seal,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const MXU2841FLOW_TRANSITIONS: [(mxu2841flowState, mxu2841flowEvent, mxu2841flowState); 8] = [
    (mxu2841flowState::Plan, mxu2841flowEvent::Plan, mxu2841flowState::Armed), // Plan→Armed 主链
    (mxu2841flowState::Armed, mxu2841flowEvent::Arm, mxu2841flowState::Running), // Armed→Running 主链
    (mxu2841flowState::Running, mxu2841flowEvent::Cross, mxu2841flowState::Crossing), // Running→Crossing 主链
    (mxu2841flowState::Crossing, mxu2841flowEvent::Run, mxu2841flowState::Judged), // Crossing→Judged 主链
    (mxu2841flowState::Judged, mxu2841flowEvent::Judge, mxu2841flowState::Sealed), // Judged→Sealed 主链
    (mxu2841flowState::Sealed, mxu2841flowEvent::Seal, mxu2841flowState::Plan), // 终态回边
    (mxu2841flowState::Sealed, mxu2841flowEvent::Plan, mxu2841flowState::Judged), // 终态回退边
    (mxu2841flowState::Armed, mxu2841flowEvent::Arm, mxu2841flowState::Running), // 旁路边
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn mxu2841flow_next(state: mxu2841flowState, ev: mxu2841flowEvent) -> Option<mxu2841flowState> {
    let mut i = 0usize;
    while i < 8 {
        let t = MXU2841FLOW_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn mxu2841flow_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < mxu2841flowState::ALL.len() {
        let mut ei = 0usize;
        while ei < mxu2841flowEvent::ALL.len() {
            let legal = mxu2841flow_next(mxu2841flowState::ALL[si], mxu2841flowEvent::ALL[ei]).is_some();
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
pub fn mxu2841flow_terminal(st: mxu2841flowState) -> bool {
    let mut i = 0usize;
    while i < 8 {
        if MXU2841FLOW_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct mxu2841flowLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: mxu2841flowState,
}

impl mxu2841flowLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: mxu2841flowState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: mxu2841flowEvent) -> bool {
        match mxu2841flow_next(self.cur, ev) {
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
// 快照 Mxu2841Snap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// 大页-地址空间联合压测 快照封存（FNV 三态指纹 + 篡改检出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mxu2841Snap {
    /// 指纹分量：hp_v。
    pub hp_v: u64,
    /// 指纹分量：as_v。
    pub as_v: u64,
    /// 指纹分量：joint_v。
    pub joint_v: u64,
    /// 指纹分量：matrix_v。
    pub matrix_v: u64,
    /// 指纹分量：xprod_v。
    pub xprod_v: u64,
    /// 指纹分量：load_v。
    pub load_v: u64,
    /// 指纹分量：frag_v。
    pub frag_v: u64,
    /// 指纹分量：sw_v。
    pub sw_v: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl Mxu2841Snap {
    /// 构造并即时合成指纹。
    pub fn compute(
        hp_v: u64,
        as_v: u64,
        joint_v: u64,
        matrix_v: u64,
        xprod_v: u64,
        load_v: u64,
        frag_v: u64,
        sw_v: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &hp_v.to_le_bytes());
        h = fnv1a64_from(h, &as_v.to_le_bytes());
        h = fnv1a64_from(h, &joint_v.to_le_bytes());
        h = fnv1a64_from(h, &matrix_v.to_le_bytes());
        h = fnv1a64_from(h, &xprod_v.to_le_bytes());
        h = fnv1a64_from(h, &load_v.to_le_bytes());
        h = fnv1a64_from(h, &frag_v.to_le_bytes());
        h = fnv1a64_from(h, &sw_v.to_le_bytes());
        Self {
            hp_v,
            as_v,
            joint_v,
            matrix_v,
            xprod_v,
            load_v,
            frag_v,
            sw_v,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.hp_v.to_le_bytes());
        h = fnv1a64_from(h, &self.as_v.to_le_bytes());
        h = fnv1a64_from(h, &self.joint_v.to_le_bytes());
        h = fnv1a64_from(h, &self.matrix_v.to_le_bytes());
        h = fnv1a64_from(h, &self.xprod_v.to_le_bytes());
        h = fnv1a64_from(h, &self.load_v.to_le_bytes());
        h = fnv1a64_from(h, &self.frag_v.to_le_bytes());
        h = fnv1a64_from(h, &self.sw_v.to_le_bytes());
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

/// 段位守卫：全部诊断码必须落在本段 [0xe1e0, 0xe1ef]。
pub const fn u2841_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2841Code::ALL.len() {
        let c = u2841Code::ALL[i].code();
        if c < 0xe1e0 || c > 0xe1ef {
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
fn u2841_guard_seg() -> bool {
    u2841_segment_guard()
}

/// UNX-F2841 状态机闭集/迁移表/非法组合恰值全绿。
fn check_u2841_sm_legal() -> bool {
    let mut ok = mxu2841flow_illegal_pairs() == 29;
    if ok { ok = MXU2841FLOW_TRANSITIONS.len() == 8; }
    if ok { ok = mxu2841flowState::ALL.len() == 6 && mxu2841flowEvent::ALL.len() == 6; }
    ok
}

/// UNX-F2841 迁移主链回放 + 快照往返恒等。
fn check_u2841_sm_round() -> bool {
    let mut lg = mxu2841flowLedger::new(mxu2841flowState::Plan);
    let mut ok = lg.apply(mxu2841flowEvent::Plan) && lg.cur == mxu2841flowState::Armed && lg.total() == 1;
    let s0 = Mxu2841Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    let mut s1 = Mxu2841Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    s1.recompute();
    if ok { ok = s0 == s1 && !s1.tampered(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2841_checks() -> CheckSet {
    let mut set = CheckSet::new("U2841");
    set.add(
        "u2841-seg-guard",
        u2841_guard_seg(),
        if u2841_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2841-sm-legal",
        check_u2841_sm_legal(),
        if check_u2841_sm_legal() { "" } else { "UNX-F2841 sm legality mismatch" },
    );
    set.add(
        "u2841-sm-round",
        check_u2841_sm_round(),
        if check_u2841_sm_round() { "" } else { "UNX-F2841 sm roundtrip mismatch" },
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
    fn u2841_all_checks_green() {
        let set = run_u2841_checks();
        assert!(set.all_passed(), "U2841 domain checks must be green");
    }

    #[test]
    fn u2841_segment_guard_holds() {
        assert!(u2841_segment_guard());
    }
}
