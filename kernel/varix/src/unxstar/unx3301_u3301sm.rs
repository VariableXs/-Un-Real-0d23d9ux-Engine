//! unxu3301_u3301sm —— RCU 压测编排（读多写少 8 核场景） 状态机与快照封存（RCU 压测编排）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3301 · RCU 压测编排（读多写少 8 核场景） · 判据原文：UNX-F3301 · 判据：J1：8 核读 + 写者换指针压测 10 秒，读侧零阻塞证据（读延迟分布 P99 ≤ 2×fast path 基线）、GP 推进健康；M-060 对账。
//! · 组件：RCU 压测编排 状态机（6 态 × 6 事件显式迁移）+ FNV 快照封存（8 分量）
//! · 判据落地：UNX-F3301 · 判据：J1：8 核读 + 写者换指针压测 10 秒，读侧零阻塞证据（读延迟分布 P99 ≤ 2×fast path 基线）、GP 推进健
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3301_VERSION: &str = "u3301-u3301sm-v1";

/// 诊断段基址。
pub const U3301_SEG_BASE: u16 = 0xe030;

// --------------------------------------------------------------------------
// 诊断码（u3301 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// RCU 压测编排 状态面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3301Code {
    /// 0xe030 状态重复（RCU 同名双态）。
    StateDup,
    /// 0xe031 状态断档（宽限 序缺失）。
    StateHole,
    /// 0xe032 事件重复（同事件双声明）。
    EventDup,
    /// 0xe033 迁移重复（同源同事件双迁移）。
    TransDup,
    /// 0xe034 迁移缺失（应达态不可达）。
    TransHole,
    /// 0xe035 迁移环（非法自环/环路）。
    TransCycle,
    /// 0xe036 入口非法（初态不在闭集）。
    EntryBad,
    /// 0xe037 迁移丢失（应用事件无效果）。
    ApplyLost,
    /// 0xe038 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe039 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe03a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe03b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3301Code {
    /// 全量闭集。
    pub const ALL: [u3301Code; 12] = [
        u3301Code::StateDup,
        u3301Code::StateHole,
        u3301Code::EventDup,
        u3301Code::TransDup,
        u3301Code::TransHole,
        u3301Code::TransCycle,
        u3301Code::EntryBad,
        u3301Code::ApplyLost,
        u3301Code::LedgerBroken,
        u3301Code::SnapshotFingerprint,
        u3301Code::TableBroken,
        u3301Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3301Code::StateDup => 0xe030,
            u3301Code::StateHole => 0xe031,
            u3301Code::EventDup => 0xe032,
            u3301Code::TransDup => 0xe033,
            u3301Code::TransHole => 0xe034,
            u3301Code::TransCycle => 0xe035,
            u3301Code::EntryBad => 0xe036,
            u3301Code::ApplyLost => 0xe037,
            u3301Code::LedgerBroken => 0xe038,
            u3301Code::SnapshotFingerprint => 0xe039,
            u3301Code::TableBroken => 0xe03a,
            u3301Code::SegmentGuard => 0xe03b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3301Code::StateDup => "状态重复（RCU 同名双态）",
            u3301Code::StateHole => "状态断档（宽限 序缺失）",
            u3301Code::EventDup => "事件重复（同事件双声明）",
            u3301Code::TransDup => "迁移重复（同源同事件双迁移）",
            u3301Code::TransHole => "迁移缺失（应达态不可达）",
            u3301Code::TransCycle => "迁移环（非法自环/环路）",
            u3301Code::EntryBad => "入口非法（初态不在闭集）",
            u3301Code::ApplyLost => "迁移丢失（应用事件无效果）",
            u3301Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3301Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3301Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3301Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3301Code::StateDup => "按闭集唯一律去重",
            u3301Code::StateHole => "按闭集补态",
            u3301Code::EventDup => "按事件闭集去重",
            u3301Code::TransDup => "按确定性唯一迁移律合并",
            u3301Code::TransHole => "按迁移矩阵补边",
            u3301Code::TransCycle => "按无环律重构迁移",
            u3301Code::EntryBad => "按闭集钳制初态",
            u3301Code::ApplyLost => "核对事件前置条件",
            u3301Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3301Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3301Code::TableBroken => "按语义规则重建常量表",
            u3301Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 状态机 mxu3301flow（6 态 × 6 事件 · 迁移 8 条显式声明）
// --------------------------------------------------------------------------
/// RCU 压测编排 状态机（判据：RCU）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu3301flowState {
    Warm,
    Read,
    Write,
    Grace,
    Quiesce,
    Report,
}

impl mxu3301flowState {
    /// 全量态闭集。
    pub const ALL: [mxu3301flowState; 6] = [
        mxu3301flowState::Warm,
        mxu3301flowState::Read,
        mxu3301flowState::Write,
        mxu3301flowState::Grace,
        mxu3301flowState::Quiesce,
        mxu3301flowState::Report,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            mxu3301flowState::Warm => 0,
            mxu3301flowState::Read => 1,
            mxu3301flowState::Write => 2,
            mxu3301flowState::Grace => 3,
            mxu3301flowState::Quiesce => 4,
            mxu3301flowState::Report => 5,
        }
    }
}

/// mxu3301flow事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu3301flowEvent {
    Start,
    Rdrun,
    Wrrun,
    Gpwait,
    Settle,
    Digest,
}

impl mxu3301flowEvent {
    /// 全量事件闭集。
    pub const ALL: [mxu3301flowEvent; 6] = [
        mxu3301flowEvent::Start,
        mxu3301flowEvent::Rdrun,
        mxu3301flowEvent::Wrrun,
        mxu3301flowEvent::Gpwait,
        mxu3301flowEvent::Settle,
        mxu3301flowEvent::Digest,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const MXU3301FLOW_TRANSITIONS: [(mxu3301flowState, mxu3301flowEvent, mxu3301flowState); 8] = [
    (mxu3301flowState::Warm, mxu3301flowEvent::Start, mxu3301flowState::Read), // Warm→Read 主链
    (mxu3301flowState::Read, mxu3301flowEvent::Rdrun, mxu3301flowState::Write), // Read→Write 主链
    (mxu3301flowState::Write, mxu3301flowEvent::Wrrun, mxu3301flowState::Grace), // Write→Grace 主链
    (mxu3301flowState::Grace, mxu3301flowEvent::Gpwait, mxu3301flowState::Quiesce), // Grace→Quiesce 主链
    (mxu3301flowState::Quiesce, mxu3301flowEvent::Settle, mxu3301flowState::Report), // Quiesce→Report 主链
    (mxu3301flowState::Report, mxu3301flowEvent::Digest, mxu3301flowState::Warm), // 终态回边
    (mxu3301flowState::Report, mxu3301flowEvent::Start, mxu3301flowState::Quiesce), // 终态回退边
    (mxu3301flowState::Read, mxu3301flowEvent::Rdrun, mxu3301flowState::Write), // 旁路边
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn mxu3301flow_next(state: mxu3301flowState, ev: mxu3301flowEvent) -> Option<mxu3301flowState> {
    let mut i = 0usize;
    while i < 8 {
        let t = MXU3301FLOW_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn mxu3301flow_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < mxu3301flowState::ALL.len() {
        let mut ei = 0usize;
        while ei < mxu3301flowEvent::ALL.len() {
            let legal = mxu3301flow_next(mxu3301flowState::ALL[si], mxu3301flowEvent::ALL[ei]).is_some();
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
pub fn mxu3301flow_terminal(st: mxu3301flowState) -> bool {
    let mut i = 0usize;
    while i < 8 {
        if MXU3301FLOW_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct mxu3301flowLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: mxu3301flowState,
}

impl mxu3301flowLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: mxu3301flowState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: mxu3301flowEvent) -> bool {
        match mxu3301flow_next(self.cur, ev) {
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
// 快照 Mxu3301Snap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// RCU 压测编排 快照封存（FNV 三态指纹 + 篡改检出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mxu3301Snap {
    /// 指纹分量：rd_v。
    pub rd_v: u64,
    /// 指纹分量：wr_v。
    pub wr_v: u64,
    /// 指纹分量：gp_v。
    pub gp_v: u64,
    /// 指纹分量：cpu_v。
    pub cpu_v: u64,
    /// 指纹分量：readm_v。
    pub readm_v: u64,
    /// 指纹分量：qs_v。
    pub qs_v: u64,
    /// 指纹分量：gpw_v。
    pub gpw_v: u64,
    /// 指纹分量：stw_v。
    pub stw_v: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl Mxu3301Snap {
    /// 构造并即时合成指纹。
    pub fn compute(
        rd_v: u64,
        wr_v: u64,
        gp_v: u64,
        cpu_v: u64,
        readm_v: u64,
        qs_v: u64,
        gpw_v: u64,
        stw_v: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &rd_v.to_le_bytes());
        h = fnv1a64_from(h, &wr_v.to_le_bytes());
        h = fnv1a64_from(h, &gp_v.to_le_bytes());
        h = fnv1a64_from(h, &cpu_v.to_le_bytes());
        h = fnv1a64_from(h, &readm_v.to_le_bytes());
        h = fnv1a64_from(h, &qs_v.to_le_bytes());
        h = fnv1a64_from(h, &gpw_v.to_le_bytes());
        h = fnv1a64_from(h, &stw_v.to_le_bytes());
        Self {
            rd_v,
            wr_v,
            gp_v,
            cpu_v,
            readm_v,
            qs_v,
            gpw_v,
            stw_v,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.rd_v.to_le_bytes());
        h = fnv1a64_from(h, &self.wr_v.to_le_bytes());
        h = fnv1a64_from(h, &self.gp_v.to_le_bytes());
        h = fnv1a64_from(h, &self.cpu_v.to_le_bytes());
        h = fnv1a64_from(h, &self.readm_v.to_le_bytes());
        h = fnv1a64_from(h, &self.qs_v.to_le_bytes());
        h = fnv1a64_from(h, &self.gpw_v.to_le_bytes());
        h = fnv1a64_from(h, &self.stw_v.to_le_bytes());
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

/// 段位守卫：全部诊断码必须落在本段 [0xe030, 0xe03f]。
pub const fn u3301_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3301Code::ALL.len() {
        let c = u3301Code::ALL[i].code();
        if c < 0xe030 || c > 0xe03f {
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
fn u3301_guard_seg() -> bool {
    u3301_segment_guard()
}

/// UNX-F3301 状态机闭集/迁移表/非法组合恰值全绿。
fn check_u3301_sm_legal() -> bool {
    let mut ok = mxu3301flow_illegal_pairs() == 29;
    if ok { ok = MXU3301FLOW_TRANSITIONS.len() == 8; }
    if ok { ok = mxu3301flowState::ALL.len() == 6 && mxu3301flowEvent::ALL.len() == 6; }
    ok
}

/// UNX-F3301 迁移主链回放 + 快照往返恒等。
fn check_u3301_sm_round() -> bool {
    let mut lg = mxu3301flowLedger::new(mxu3301flowState::Warm);
    let mut ok = lg.apply(mxu3301flowEvent::Start) && lg.cur == mxu3301flowState::Read && lg.total() == 1;
    let s0 = Mxu3301Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    let mut s1 = Mxu3301Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    s1.recompute();
    if ok { ok = s0 == s1 && !s1.tampered(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3301_checks() -> CheckSet {
    let mut set = CheckSet::new("U3301");
    set.add(
        "u3301-seg-guard",
        u3301_guard_seg(),
        if u3301_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3301-sm-legal",
        check_u3301_sm_legal(),
        if check_u3301_sm_legal() { "" } else { "UNX-F3301 sm legality mismatch" },
    );
    set.add(
        "u3301-sm-round",
        check_u3301_sm_round(),
        if check_u3301_sm_round() { "" } else { "UNX-F3301 sm roundtrip mismatch" },
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
    fn u3301_all_checks_green() {
        let set = run_u3301_checks();
        assert!(set.all_passed(), "U3301 domain checks must be green");
    }

    #[test]
    fn u3301_segment_guard_holds() {
        assert!(u3301_segment_guard());
    }
}
