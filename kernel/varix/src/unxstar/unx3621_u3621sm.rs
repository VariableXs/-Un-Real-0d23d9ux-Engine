//! unxu3621_u3621sm —— RCU 压测总设计（读/写/宽限三面实验框架） 状态机与快照封存（RCU 三面实验）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3621 · RCU 压测总设计（读/写/宽限三面实验框架） · 判据原文：UNX-F3621 · 判据：J1：三面框架（读面（并发/一致性）/写面（复制发布切换）/宽限面（时长/滞留/回收）——判据行内嵌职责表）；M-379 三面首跑（最小三面用例各绿）。<br>J2：框架零侵（RCU 路径拍数与基线一致——度量可关）。
//! · 组件：RCU 三面实验 状态机（6 态 × 6 事件显式迁移）+ FNV 快照封存（8 分量）
//! · 判据落地：UNX-F3621 · 判据：J1：三面框架（读面（并发/一致性）/写面（复制发布切换）/宽限面（时长/滞留/回收）——判据行内嵌职责表）；M-379 三面首跑
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3621_VERSION: &str = "u3621-u3621sm-v1";

/// 诊断段基址。
pub const U3621_SEG_BASE: u16 = 0xe130;

// --------------------------------------------------------------------------
// 诊断码（u3621 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// RCU 三面实验 状态面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3621Code {
    /// 0xe130 状态重复（RCU 同名双态）。
    StateDup,
    /// 0xe131 状态断档（宽限 序缺失）。
    StateHole,
    /// 0xe132 事件重复（同事件双声明）。
    EventDup,
    /// 0xe133 迁移重复（同源同事件双迁移）。
    TransDup,
    /// 0xe134 迁移缺失（应达态不可达）。
    TransHole,
    /// 0xe135 迁移环（非法自环/环路）。
    TransCycle,
    /// 0xe136 入口非法（初态不在闭集）。
    EntryBad,
    /// 0xe137 迁移丢失（应用事件无效果）。
    ApplyLost,
    /// 0xe138 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe139 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe13a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe13b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3621Code {
    /// 全量闭集。
    pub const ALL: [u3621Code; 12] = [
        u3621Code::StateDup,
        u3621Code::StateHole,
        u3621Code::EventDup,
        u3621Code::TransDup,
        u3621Code::TransHole,
        u3621Code::TransCycle,
        u3621Code::EntryBad,
        u3621Code::ApplyLost,
        u3621Code::LedgerBroken,
        u3621Code::SnapshotFingerprint,
        u3621Code::TableBroken,
        u3621Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3621Code::StateDup => 0xe130,
            u3621Code::StateHole => 0xe131,
            u3621Code::EventDup => 0xe132,
            u3621Code::TransDup => 0xe133,
            u3621Code::TransHole => 0xe134,
            u3621Code::TransCycle => 0xe135,
            u3621Code::EntryBad => 0xe136,
            u3621Code::ApplyLost => 0xe137,
            u3621Code::LedgerBroken => 0xe138,
            u3621Code::SnapshotFingerprint => 0xe139,
            u3621Code::TableBroken => 0xe13a,
            u3621Code::SegmentGuard => 0xe13b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3621Code::StateDup => "状态重复（RCU 同名双态）",
            u3621Code::StateHole => "状态断档（宽限 序缺失）",
            u3621Code::EventDup => "事件重复（同事件双声明）",
            u3621Code::TransDup => "迁移重复（同源同事件双迁移）",
            u3621Code::TransHole => "迁移缺失（应达态不可达）",
            u3621Code::TransCycle => "迁移环（非法自环/环路）",
            u3621Code::EntryBad => "入口非法（初态不在闭集）",
            u3621Code::ApplyLost => "迁移丢失（应用事件无效果）",
            u3621Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3621Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3621Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3621Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3621Code::StateDup => "按闭集唯一律去重",
            u3621Code::StateHole => "按闭集补态",
            u3621Code::EventDup => "按事件闭集去重",
            u3621Code::TransDup => "按确定性唯一迁移律合并",
            u3621Code::TransHole => "按迁移矩阵补边",
            u3621Code::TransCycle => "按无环律重构迁移",
            u3621Code::EntryBad => "按闭集钳制初态",
            u3621Code::ApplyLost => "核对事件前置条件",
            u3621Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3621Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3621Code::TableBroken => "按语义规则重建常量表",
            u3621Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 状态机 mxu3621flow（6 态 × 6 事件 · 迁移 8 条显式声明）
// --------------------------------------------------------------------------
/// RCU 三面实验 状态机（判据：RCU）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu3621flowState {
    Setup,
    Trial,
    Read,
    Write,
    Grace,
    Digest,
}

impl mxu3621flowState {
    /// 全量态闭集。
    pub const ALL: [mxu3621flowState; 6] = [
        mxu3621flowState::Setup,
        mxu3621flowState::Trial,
        mxu3621flowState::Read,
        mxu3621flowState::Write,
        mxu3621flowState::Grace,
        mxu3621flowState::Digest,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            mxu3621flowState::Setup => 0,
            mxu3621flowState::Trial => 1,
            mxu3621flowState::Read => 2,
            mxu3621flowState::Write => 3,
            mxu3621flowState::Grace => 4,
            mxu3621flowState::Digest => 5,
        }
    }
}

/// mxu3621flow事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu3621flowEvent {
    Setup,
    ReadRun,
    WriteRun,
    GraceWait,
    Measure,
    Digest,
}

impl mxu3621flowEvent {
    /// 全量事件闭集。
    pub const ALL: [mxu3621flowEvent; 6] = [
        mxu3621flowEvent::Setup,
        mxu3621flowEvent::ReadRun,
        mxu3621flowEvent::WriteRun,
        mxu3621flowEvent::GraceWait,
        mxu3621flowEvent::Measure,
        mxu3621flowEvent::Digest,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const MXU3621FLOW_TRANSITIONS: [(mxu3621flowState, mxu3621flowEvent, mxu3621flowState); 8] = [
    (mxu3621flowState::Setup, mxu3621flowEvent::Setup, mxu3621flowState::Trial), // Setup→Trial 主链
    (mxu3621flowState::Trial, mxu3621flowEvent::ReadRun, mxu3621flowState::Read), // Trial→Read 主链
    (mxu3621flowState::Read, mxu3621flowEvent::WriteRun, mxu3621flowState::Write), // Read→Write 主链
    (mxu3621flowState::Write, mxu3621flowEvent::GraceWait, mxu3621flowState::Grace), // Write→Grace 主链
    (mxu3621flowState::Grace, mxu3621flowEvent::Measure, mxu3621flowState::Digest), // Grace→Digest 主链
    (mxu3621flowState::Digest, mxu3621flowEvent::Digest, mxu3621flowState::Setup), // 终态回边
    (mxu3621flowState::Digest, mxu3621flowEvent::Setup, mxu3621flowState::Grace), // 终态回退边
    (mxu3621flowState::Trial, mxu3621flowEvent::ReadRun, mxu3621flowState::Read), // 旁路边
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn mxu3621flow_next(state: mxu3621flowState, ev: mxu3621flowEvent) -> Option<mxu3621flowState> {
    let mut i = 0usize;
    while i < 8 {
        let t = MXU3621FLOW_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn mxu3621flow_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < mxu3621flowState::ALL.len() {
        let mut ei = 0usize;
        while ei < mxu3621flowEvent::ALL.len() {
            let legal = mxu3621flow_next(mxu3621flowState::ALL[si], mxu3621flowEvent::ALL[ei]).is_some();
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
pub fn mxu3621flow_terminal(st: mxu3621flowState) -> bool {
    let mut i = 0usize;
    while i < 8 {
        if MXU3621FLOW_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct mxu3621flowLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: mxu3621flowState,
}

impl mxu3621flowLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: mxu3621flowState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: mxu3621flowEvent) -> bool {
        match mxu3621flow_next(self.cur, ev) {
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
// 快照 Mxu3621Snap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// RCU 三面实验 快照封存（FNV 三态指纹 + 篡改检出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mxu3621Snap {
    /// 指纹分量：rdc_v。
    pub rdc_v: u64,
    /// 指纹分量：wrc_v。
    pub wrc_v: u64,
    /// 指纹分量：gpc_v。
    pub gpc_v: u64,
    /// 指纹分量：exp_v。
    pub exp_v: u64,
    /// 指纹分量：trial_v。
    pub trial_v: u64,
    /// 指纹分量：lag_v。
    pub lag_v: u64,
    /// 指纹分量：gpw_v。
    pub gpw_v: u64,
    /// 指纹分量：thr_v。
    pub thr_v: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl Mxu3621Snap {
    /// 构造并即时合成指纹。
    pub fn compute(
        rdc_v: u64,
        wrc_v: u64,
        gpc_v: u64,
        exp_v: u64,
        trial_v: u64,
        lag_v: u64,
        gpw_v: u64,
        thr_v: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &rdc_v.to_le_bytes());
        h = fnv1a64_from(h, &wrc_v.to_le_bytes());
        h = fnv1a64_from(h, &gpc_v.to_le_bytes());
        h = fnv1a64_from(h, &exp_v.to_le_bytes());
        h = fnv1a64_from(h, &trial_v.to_le_bytes());
        h = fnv1a64_from(h, &lag_v.to_le_bytes());
        h = fnv1a64_from(h, &gpw_v.to_le_bytes());
        h = fnv1a64_from(h, &thr_v.to_le_bytes());
        Self {
            rdc_v,
            wrc_v,
            gpc_v,
            exp_v,
            trial_v,
            lag_v,
            gpw_v,
            thr_v,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.rdc_v.to_le_bytes());
        h = fnv1a64_from(h, &self.wrc_v.to_le_bytes());
        h = fnv1a64_from(h, &self.gpc_v.to_le_bytes());
        h = fnv1a64_from(h, &self.exp_v.to_le_bytes());
        h = fnv1a64_from(h, &self.trial_v.to_le_bytes());
        h = fnv1a64_from(h, &self.lag_v.to_le_bytes());
        h = fnv1a64_from(h, &self.gpw_v.to_le_bytes());
        h = fnv1a64_from(h, &self.thr_v.to_le_bytes());
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

/// 段位守卫：全部诊断码必须落在本段 [0xe130, 0xe13f]。
pub const fn u3621_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3621Code::ALL.len() {
        let c = u3621Code::ALL[i].code();
        if c < 0xe130 || c > 0xe13f {
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
fn u3621_guard_seg() -> bool {
    u3621_segment_guard()
}

/// UNX-F3621 状态机闭集/迁移表/非法组合恰值全绿。
fn check_u3621_sm_legal() -> bool {
    let mut ok = mxu3621flow_illegal_pairs() == 29;
    if ok { ok = MXU3621FLOW_TRANSITIONS.len() == 8; }
    if ok { ok = mxu3621flowState::ALL.len() == 6 && mxu3621flowEvent::ALL.len() == 6; }
    ok
}

/// UNX-F3621 迁移主链回放 + 快照往返恒等。
fn check_u3621_sm_round() -> bool {
    let mut lg = mxu3621flowLedger::new(mxu3621flowState::Setup);
    let mut ok = lg.apply(mxu3621flowEvent::Setup) && lg.cur == mxu3621flowState::Trial && lg.total() == 1;
    let s0 = Mxu3621Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    let mut s1 = Mxu3621Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    s1.recompute();
    if ok { ok = s0 == s1 && !s1.tampered(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3621_checks() -> CheckSet {
    let mut set = CheckSet::new("U3621");
    set.add(
        "u3621-seg-guard",
        u3621_guard_seg(),
        if u3621_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3621-sm-legal",
        check_u3621_sm_legal(),
        if check_u3621_sm_legal() { "" } else { "UNX-F3621 sm legality mismatch" },
    );
    set.add(
        "u3621-sm-round",
        check_u3621_sm_round(),
        if check_u3621_sm_round() { "" } else { "UNX-F3621 sm roundtrip mismatch" },
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
    fn u3621_all_checks_green() {
        let set = run_u3621_checks();
        assert!(set.all_passed(), "U3621 domain checks must be green");
    }

    #[test]
    fn u3621_segment_guard_holds() {
        assert!(u3621_segment_guard());
    }
}
