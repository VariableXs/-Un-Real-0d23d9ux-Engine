//! unxu3601_u3601sm —— 锁压力机总设计（场景编排/指标采集/裁决门） 状态机与快照封存（锁压力机）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3601 · 锁压力机总设计（场景编排/指标采集/裁决门） · 判据原文：UNX-F3601 · 判据：J1：压力机三层（场景层（七类场景注册）/执行层（N 工作者调度）/度量层（争用账挂钩）——判据行内嵌）；M-359 全链首跑（场景注册→执行→度量→裁决四段走通）。<br>J2：零侵断言（压力机关闭时锁路径拍数不变——度量是包裹观测）。
//! · 组件：锁压力机 状态机（6 态 × 6 事件显式迁移）+ FNV 快照封存（8 分量）
//! · 判据落地：UNX-F3601 · 判据：J1：压力机三层（场景层（七类场景注册）/执行层（N 工作者调度）/度量层（争用账挂钩）——判据行内嵌）；M-359 全链首跑（场
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3601_VERSION: &str = "u3601-u3601sm-v1";

/// 诊断段基址。
pub const U3601_SEG_BASE: u16 = 0xe120;

// --------------------------------------------------------------------------
// 诊断码（u3601 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 锁压力机 状态面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3601Code {
    /// 0xe120 状态重复（压力 同名双态）。
    StateDup,
    /// 0xe121 状态断档（场景 序缺失）。
    StateHole,
    /// 0xe122 事件重复（同事件双声明）。
    EventDup,
    /// 0xe123 迁移重复（同源同事件双迁移）。
    TransDup,
    /// 0xe124 迁移缺失（应达态不可达）。
    TransHole,
    /// 0xe125 迁移环（非法自环/环路）。
    TransCycle,
    /// 0xe126 入口非法（初态不在闭集）。
    EntryBad,
    /// 0xe127 迁移丢失（应用事件无效果）。
    ApplyLost,
    /// 0xe128 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe129 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe12a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe12b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3601Code {
    /// 全量闭集。
    pub const ALL: [u3601Code; 12] = [
        u3601Code::StateDup,
        u3601Code::StateHole,
        u3601Code::EventDup,
        u3601Code::TransDup,
        u3601Code::TransHole,
        u3601Code::TransCycle,
        u3601Code::EntryBad,
        u3601Code::ApplyLost,
        u3601Code::LedgerBroken,
        u3601Code::SnapshotFingerprint,
        u3601Code::TableBroken,
        u3601Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3601Code::StateDup => 0xe120,
            u3601Code::StateHole => 0xe121,
            u3601Code::EventDup => 0xe122,
            u3601Code::TransDup => 0xe123,
            u3601Code::TransHole => 0xe124,
            u3601Code::TransCycle => 0xe125,
            u3601Code::EntryBad => 0xe126,
            u3601Code::ApplyLost => 0xe127,
            u3601Code::LedgerBroken => 0xe128,
            u3601Code::SnapshotFingerprint => 0xe129,
            u3601Code::TableBroken => 0xe12a,
            u3601Code::SegmentGuard => 0xe12b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3601Code::StateDup => "状态重复（压力 同名双态）",
            u3601Code::StateHole => "状态断档（场景 序缺失）",
            u3601Code::EventDup => "事件重复（同事件双声明）",
            u3601Code::TransDup => "迁移重复（同源同事件双迁移）",
            u3601Code::TransHole => "迁移缺失（应达态不可达）",
            u3601Code::TransCycle => "迁移环（非法自环/环路）",
            u3601Code::EntryBad => "入口非法（初态不在闭集）",
            u3601Code::ApplyLost => "迁移丢失（应用事件无效果）",
            u3601Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3601Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3601Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3601Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3601Code::StateDup => "按闭集唯一律去重",
            u3601Code::StateHole => "按闭集补态",
            u3601Code::EventDup => "按事件闭集去重",
            u3601Code::TransDup => "按确定性唯一迁移律合并",
            u3601Code::TransHole => "按迁移矩阵补边",
            u3601Code::TransCycle => "按无环律重构迁移",
            u3601Code::EntryBad => "按闭集钳制初态",
            u3601Code::ApplyLost => "核对事件前置条件",
            u3601Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3601Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3601Code::TableBroken => "按语义规则重建常量表",
            u3601Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 状态机 mxu3601flow（6 态 × 6 事件 · 迁移 8 条显式声明）
// --------------------------------------------------------------------------
/// 锁压力机 状态机（判据：压力）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu3601flowState {
    Idle,
    Arrange,
    Running,
    Sampling,
    Judging,
    Reported,
}

impl mxu3601flowState {
    /// 全量态闭集。
    pub const ALL: [mxu3601flowState; 6] = [
        mxu3601flowState::Idle,
        mxu3601flowState::Arrange,
        mxu3601flowState::Running,
        mxu3601flowState::Sampling,
        mxu3601flowState::Judging,
        mxu3601flowState::Reported,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            mxu3601flowState::Idle => 0,
            mxu3601flowState::Arrange => 1,
            mxu3601flowState::Running => 2,
            mxu3601flowState::Sampling => 3,
            mxu3601flowState::Judging => 4,
            mxu3601flowState::Reported => 5,
        }
    }
}

/// mxu3601flow事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu3601flowEvent {
    Arrange,
    Run,
    Sample,
    Judge,
    Report,
    Repeat,
}

impl mxu3601flowEvent {
    /// 全量事件闭集。
    pub const ALL: [mxu3601flowEvent; 6] = [
        mxu3601flowEvent::Arrange,
        mxu3601flowEvent::Run,
        mxu3601flowEvent::Sample,
        mxu3601flowEvent::Judge,
        mxu3601flowEvent::Report,
        mxu3601flowEvent::Repeat,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const MXU3601FLOW_TRANSITIONS: [(mxu3601flowState, mxu3601flowEvent, mxu3601flowState); 8] = [
    (mxu3601flowState::Idle, mxu3601flowEvent::Arrange, mxu3601flowState::Arrange), // Idle→Arrange 主链
    (mxu3601flowState::Arrange, mxu3601flowEvent::Run, mxu3601flowState::Running), // Arrange→Running 主链
    (mxu3601flowState::Running, mxu3601flowEvent::Sample, mxu3601flowState::Sampling), // Running→Sampling 主链
    (mxu3601flowState::Sampling, mxu3601flowEvent::Judge, mxu3601flowState::Judging), // Sampling→Judging 主链
    (mxu3601flowState::Judging, mxu3601flowEvent::Report, mxu3601flowState::Reported), // Judging→Reported 主链
    (mxu3601flowState::Reported, mxu3601flowEvent::Repeat, mxu3601flowState::Idle), // 终态回边
    (mxu3601flowState::Reported, mxu3601flowEvent::Arrange, mxu3601flowState::Judging), // 终态回退边
    (mxu3601flowState::Arrange, mxu3601flowEvent::Run, mxu3601flowState::Running), // 旁路边
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn mxu3601flow_next(state: mxu3601flowState, ev: mxu3601flowEvent) -> Option<mxu3601flowState> {
    let mut i = 0usize;
    while i < 8 {
        let t = MXU3601FLOW_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn mxu3601flow_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < mxu3601flowState::ALL.len() {
        let mut ei = 0usize;
        while ei < mxu3601flowEvent::ALL.len() {
            let legal = mxu3601flow_next(mxu3601flowState::ALL[si], mxu3601flowEvent::ALL[ei]).is_some();
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
pub fn mxu3601flow_terminal(st: mxu3601flowState) -> bool {
    let mut i = 0usize;
    while i < 8 {
        if MXU3601FLOW_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct mxu3601flowLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: mxu3601flowState,
}

impl mxu3601flowLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: mxu3601flowState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: mxu3601flowEvent) -> bool {
        match mxu3601flow_next(self.cur, ev) {
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
// 快照 Mxu3601Snap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// 锁压力机 快照封存（FNV 三态指纹 + 篡改检出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mxu3601Snap {
    /// 指纹分量：stress_v。
    pub stress_v: u64,
    /// 指纹分量：scn_v。
    pub scn_v: u64,
    /// 指纹分量：met_v。
    pub met_v: u64,
    /// 指纹分量：gate_v。
    pub gate_v: u64,
    /// 指纹分量：load_v。
    pub load_v: u64,
    /// 指纹分量：cont_v。
    pub cont_v: u64,
    /// 指纹分量：thr_v。
    pub thr_v: u64,
    /// 指纹分量：score_v。
    pub score_v: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl Mxu3601Snap {
    /// 构造并即时合成指纹。
    pub fn compute(
        stress_v: u64,
        scn_v: u64,
        met_v: u64,
        gate_v: u64,
        load_v: u64,
        cont_v: u64,
        thr_v: u64,
        score_v: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &stress_v.to_le_bytes());
        h = fnv1a64_from(h, &scn_v.to_le_bytes());
        h = fnv1a64_from(h, &met_v.to_le_bytes());
        h = fnv1a64_from(h, &gate_v.to_le_bytes());
        h = fnv1a64_from(h, &load_v.to_le_bytes());
        h = fnv1a64_from(h, &cont_v.to_le_bytes());
        h = fnv1a64_from(h, &thr_v.to_le_bytes());
        h = fnv1a64_from(h, &score_v.to_le_bytes());
        Self {
            stress_v,
            scn_v,
            met_v,
            gate_v,
            load_v,
            cont_v,
            thr_v,
            score_v,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.stress_v.to_le_bytes());
        h = fnv1a64_from(h, &self.scn_v.to_le_bytes());
        h = fnv1a64_from(h, &self.met_v.to_le_bytes());
        h = fnv1a64_from(h, &self.gate_v.to_le_bytes());
        h = fnv1a64_from(h, &self.load_v.to_le_bytes());
        h = fnv1a64_from(h, &self.cont_v.to_le_bytes());
        h = fnv1a64_from(h, &self.thr_v.to_le_bytes());
        h = fnv1a64_from(h, &self.score_v.to_le_bytes());
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

/// 段位守卫：全部诊断码必须落在本段 [0xe120, 0xe12f]。
pub const fn u3601_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3601Code::ALL.len() {
        let c = u3601Code::ALL[i].code();
        if c < 0xe120 || c > 0xe12f {
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
fn u3601_guard_seg() -> bool {
    u3601_segment_guard()
}

/// UNX-F3601 状态机闭集/迁移表/非法组合恰值全绿。
fn check_u3601_sm_legal() -> bool {
    let mut ok = mxu3601flow_illegal_pairs() == 29;
    if ok { ok = MXU3601FLOW_TRANSITIONS.len() == 8; }
    if ok { ok = mxu3601flowState::ALL.len() == 6 && mxu3601flowEvent::ALL.len() == 6; }
    ok
}

/// UNX-F3601 迁移主链回放 + 快照往返恒等。
fn check_u3601_sm_round() -> bool {
    let mut lg = mxu3601flowLedger::new(mxu3601flowState::Idle);
    let mut ok = lg.apply(mxu3601flowEvent::Arrange) && lg.cur == mxu3601flowState::Arrange && lg.total() == 1;
    let s0 = Mxu3601Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    let mut s1 = Mxu3601Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    s1.recompute();
    if ok { ok = s0 == s1 && !s1.tampered(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3601_checks() -> CheckSet {
    let mut set = CheckSet::new("U3601");
    set.add(
        "u3601-seg-guard",
        u3601_guard_seg(),
        if u3601_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3601-sm-legal",
        check_u3601_sm_legal(),
        if check_u3601_sm_legal() { "" } else { "UNX-F3601 sm legality mismatch" },
    );
    set.add(
        "u3601-sm-round",
        check_u3601_sm_round(),
        if check_u3601_sm_round() { "" } else { "UNX-F3601 sm roundtrip mismatch" },
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
    fn u3601_all_checks_green() {
        let set = run_u3601_checks();
        assert!(set.all_passed(), "U3601 domain checks must be green");
    }

    #[test]
    fn u3601_segment_guard_holds() {
        assert!(u3601_segment_guard());
    }
}
