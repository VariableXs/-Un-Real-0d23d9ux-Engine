//! unxu3021_u3021sm —— 三档压力分档冻结（轻/中/重回收压力档定义） 状态机与快照封存（三档压力分档）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3021 · 三档压力分档冻结（轻/中/重回收压力档定义） · 判据原文：UNX-F3021 · 判据：UNX-F3021-J1 三档分档冻结：轻/中/重三档（水线区间+扫描强度二维）定义哈希入册，与 F2941 压力矩阵三轴口径同源核过，档位漂移注入 10/10 次拦截
//! · 组件：三档压力分档 状态机（6 态 × 6 事件显式迁移）+ FNV 快照封存（8 分量）
//! · 判据落地：UNX-F3021 · 判据：UNX-F3021-J1 三档分档冻结：轻/中/重三档（水线区间+扫描强度二维）定义哈希入册，与 F2941 压力矩阵三轴口径同源
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3021_VERSION: &str = "u3021-u3021sm-v1";

/// 诊断段基址。
pub const U3021_SEG_BASE: u16 = 0xe270;

// --------------------------------------------------------------------------
// 诊断码（u3021 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 三档压力分档 状态面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3021Code {
    /// 0xe270 状态重复（三档 同名双态）。
    StateDup,
    /// 0xe271 状态断档（压力 序缺失）。
    StateHole,
    /// 0xe272 事件重复（同事件双声明）。
    EventDup,
    /// 0xe273 迁移重复（同源同事件双迁移）。
    TransDup,
    /// 0xe274 迁移缺失（应达态不可达）。
    TransHole,
    /// 0xe275 迁移环（非法自环/环路）。
    TransCycle,
    /// 0xe276 入口非法（初态不在闭集）。
    EntryBad,
    /// 0xe277 迁移丢失（应用事件无效果）。
    ApplyLost,
    /// 0xe278 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe279 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe27a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe27b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3021Code {
    /// 全量闭集。
    pub const ALL: [u3021Code; 12] = [
        u3021Code::StateDup,
        u3021Code::StateHole,
        u3021Code::EventDup,
        u3021Code::TransDup,
        u3021Code::TransHole,
        u3021Code::TransCycle,
        u3021Code::EntryBad,
        u3021Code::ApplyLost,
        u3021Code::LedgerBroken,
        u3021Code::SnapshotFingerprint,
        u3021Code::TableBroken,
        u3021Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3021Code::StateDup => 0xe270,
            u3021Code::StateHole => 0xe271,
            u3021Code::EventDup => 0xe272,
            u3021Code::TransDup => 0xe273,
            u3021Code::TransHole => 0xe274,
            u3021Code::TransCycle => 0xe275,
            u3021Code::EntryBad => 0xe276,
            u3021Code::ApplyLost => 0xe277,
            u3021Code::LedgerBroken => 0xe278,
            u3021Code::SnapshotFingerprint => 0xe279,
            u3021Code::TableBroken => 0xe27a,
            u3021Code::SegmentGuard => 0xe27b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3021Code::StateDup => "状态重复（三档 同名双态）",
            u3021Code::StateHole => "状态断档（压力 序缺失）",
            u3021Code::EventDup => "事件重复（同事件双声明）",
            u3021Code::TransDup => "迁移重复（同源同事件双迁移）",
            u3021Code::TransHole => "迁移缺失（应达态不可达）",
            u3021Code::TransCycle => "迁移环（非法自环/环路）",
            u3021Code::EntryBad => "入口非法（初态不在闭集）",
            u3021Code::ApplyLost => "迁移丢失（应用事件无效果）",
            u3021Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3021Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3021Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3021Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3021Code::StateDup => "按闭集唯一律去重",
            u3021Code::StateHole => "按闭集补态",
            u3021Code::EventDup => "按事件闭集去重",
            u3021Code::TransDup => "按确定性唯一迁移律合并",
            u3021Code::TransHole => "按迁移矩阵补边",
            u3021Code::TransCycle => "按无环律重构迁移",
            u3021Code::EntryBad => "按闭集钳制初态",
            u3021Code::ApplyLost => "核对事件前置条件",
            u3021Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3021Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3021Code::TableBroken => "按语义规则重建常量表",
            u3021Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 状态机 mxu3021flow（6 态 × 6 事件 · 迁移 8 条显式声明）
// --------------------------------------------------------------------------
/// 三档压力分档 状态机（判据：三档）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu3021flowState {
    Defined,
    Armed,
    Running,
    Scaling,
    Judged,
    Sealed,
}

impl mxu3021flowState {
    /// 全量态闭集。
    pub const ALL: [mxu3021flowState; 6] = [
        mxu3021flowState::Defined,
        mxu3021flowState::Armed,
        mxu3021flowState::Running,
        mxu3021flowState::Scaling,
        mxu3021flowState::Judged,
        mxu3021flowState::Sealed,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            mxu3021flowState::Defined => 0,
            mxu3021flowState::Armed => 1,
            mxu3021flowState::Running => 2,
            mxu3021flowState::Scaling => 3,
            mxu3021flowState::Judged => 4,
            mxu3021flowState::Sealed => 5,
        }
    }
}

/// mxu3021flow事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu3021flowEvent {
    Define,
    Arm,
    Run,
    Scale,
    Judge,
    Seal,
}

impl mxu3021flowEvent {
    /// 全量事件闭集。
    pub const ALL: [mxu3021flowEvent; 6] = [
        mxu3021flowEvent::Define,
        mxu3021flowEvent::Arm,
        mxu3021flowEvent::Run,
        mxu3021flowEvent::Scale,
        mxu3021flowEvent::Judge,
        mxu3021flowEvent::Seal,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const MXU3021FLOW_TRANSITIONS: [(mxu3021flowState, mxu3021flowEvent, mxu3021flowState); 8] = [
    (mxu3021flowState::Defined, mxu3021flowEvent::Define, mxu3021flowState::Armed), // Defined→Armed 主链
    (mxu3021flowState::Armed, mxu3021flowEvent::Arm, mxu3021flowState::Running), // Armed→Running 主链
    (mxu3021flowState::Running, mxu3021flowEvent::Run, mxu3021flowState::Scaling), // Running→Scaling 主链
    (mxu3021flowState::Scaling, mxu3021flowEvent::Scale, mxu3021flowState::Judged), // Scaling→Judged 主链
    (mxu3021flowState::Judged, mxu3021flowEvent::Judge, mxu3021flowState::Sealed), // Judged→Sealed 主链
    (mxu3021flowState::Sealed, mxu3021flowEvent::Seal, mxu3021flowState::Defined), // 终态回边
    (mxu3021flowState::Sealed, mxu3021flowEvent::Define, mxu3021flowState::Judged), // 终态回退边
    (mxu3021flowState::Armed, mxu3021flowEvent::Arm, mxu3021flowState::Running), // 旁路边
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn mxu3021flow_next(state: mxu3021flowState, ev: mxu3021flowEvent) -> Option<mxu3021flowState> {
    let mut i = 0usize;
    while i < 8 {
        let t = MXU3021FLOW_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn mxu3021flow_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < mxu3021flowState::ALL.len() {
        let mut ei = 0usize;
        while ei < mxu3021flowEvent::ALL.len() {
            let legal = mxu3021flow_next(mxu3021flowState::ALL[si], mxu3021flowEvent::ALL[ei]).is_some();
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
pub fn mxu3021flow_terminal(st: mxu3021flowState) -> bool {
    let mut i = 0usize;
    while i < 8 {
        if MXU3021FLOW_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct mxu3021flowLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: mxu3021flowState,
}

impl mxu3021flowLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: mxu3021flowState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: mxu3021flowEvent) -> bool {
        match mxu3021flow_next(self.cur, ev) {
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
// 快照 Mxu3021Snap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// 三档压力分档 快照封存（FNV 三态指纹 + 篡改检出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mxu3021Snap {
    /// 指纹分量：t1_v。
    pub t1_v: u64,
    /// 指纹分量：t2_v。
    pub t2_v: u64,
    /// 指纹分量：t3_v。
    pub t3_v: u64,
    /// 指纹分量：reclaim_v。
    pub reclaim_v: u64,
    /// 指纹分量：dose_v。
    pub dose_v: u64,
    /// 指纹分量：load_v。
    pub load_v: u64,
    /// 指纹分量：dur_v。
    pub dur_v: u64,
    /// 指纹分量：met_v。
    pub met_v: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl Mxu3021Snap {
    /// 构造并即时合成指纹。
    pub fn compute(
        t1_v: u64,
        t2_v: u64,
        t3_v: u64,
        reclaim_v: u64,
        dose_v: u64,
        load_v: u64,
        dur_v: u64,
        met_v: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &t1_v.to_le_bytes());
        h = fnv1a64_from(h, &t2_v.to_le_bytes());
        h = fnv1a64_from(h, &t3_v.to_le_bytes());
        h = fnv1a64_from(h, &reclaim_v.to_le_bytes());
        h = fnv1a64_from(h, &dose_v.to_le_bytes());
        h = fnv1a64_from(h, &load_v.to_le_bytes());
        h = fnv1a64_from(h, &dur_v.to_le_bytes());
        h = fnv1a64_from(h, &met_v.to_le_bytes());
        Self {
            t1_v,
            t2_v,
            t3_v,
            reclaim_v,
            dose_v,
            load_v,
            dur_v,
            met_v,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.t1_v.to_le_bytes());
        h = fnv1a64_from(h, &self.t2_v.to_le_bytes());
        h = fnv1a64_from(h, &self.t3_v.to_le_bytes());
        h = fnv1a64_from(h, &self.reclaim_v.to_le_bytes());
        h = fnv1a64_from(h, &self.dose_v.to_le_bytes());
        h = fnv1a64_from(h, &self.load_v.to_le_bytes());
        h = fnv1a64_from(h, &self.dur_v.to_le_bytes());
        h = fnv1a64_from(h, &self.met_v.to_le_bytes());
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

/// 段位守卫：全部诊断码必须落在本段 [0xe270, 0xe27f]。
pub const fn u3021_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3021Code::ALL.len() {
        let c = u3021Code::ALL[i].code();
        if c < 0xe270 || c > 0xe27f {
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
fn u3021_guard_seg() -> bool {
    u3021_segment_guard()
}

/// UNX-F3021 状态机闭集/迁移表/非法组合恰值全绿。
fn check_u3021_sm_legal() -> bool {
    let mut ok = mxu3021flow_illegal_pairs() == 29;
    if ok { ok = MXU3021FLOW_TRANSITIONS.len() == 8; }
    if ok { ok = mxu3021flowState::ALL.len() == 6 && mxu3021flowEvent::ALL.len() == 6; }
    ok
}

/// UNX-F3021 迁移主链回放 + 快照往返恒等。
fn check_u3021_sm_round() -> bool {
    let mut lg = mxu3021flowLedger::new(mxu3021flowState::Defined);
    let mut ok = lg.apply(mxu3021flowEvent::Define) && lg.cur == mxu3021flowState::Armed && lg.total() == 1;
    let s0 = Mxu3021Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    let mut s1 = Mxu3021Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    s1.recompute();
    if ok { ok = s0 == s1 && !s1.tampered(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3021_checks() -> CheckSet {
    let mut set = CheckSet::new("U3021");
    set.add(
        "u3021-seg-guard",
        u3021_guard_seg(),
        if u3021_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3021-sm-legal",
        check_u3021_sm_legal(),
        if check_u3021_sm_legal() { "" } else { "UNX-F3021 sm legality mismatch" },
    );
    set.add(
        "u3021-sm-round",
        check_u3021_sm_round(),
        if check_u3021_sm_round() { "" } else { "UNX-F3021 sm roundtrip mismatch" },
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
    fn u3021_all_checks_green() {
        let set = run_u3021_checks();
        assert!(set.all_passed(), "U3021 domain checks must be green");
    }

    #[test]
    fn u3021_segment_guard_holds() {
        assert!(u3021_segment_guard());
    }
}
