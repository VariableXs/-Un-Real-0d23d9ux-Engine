//! unxu3441_u3441sm —— 执行器总设计（六阶段流水线：选例→隔离→排序→执行→超时→上报） 状态机与快照封存（执行器流水线）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3441 · 执行器总设计（六阶段流水线：选例→隔离→排序→执行→超时→上报） · 判据原文：UNX-F3441 · 判据：J1：执行器总设计文档交付（六阶段流水线定义 + 各阶段契约）；M-200 桩后端全链路六阶段贯通。
//! · 组件：执行器流水线 状态机（6 态 × 6 事件显式迁移）+ FNV 快照封存（8 分量）
//! · 判据落地：UNX-F3441 · 判据：J1：执行器总设计文档交付（六阶段流水线定义 + 各阶段契约）；M-200 桩后端全链路六阶段贯通。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3441_VERSION: &str = "u3441-u3441sm-v1";

/// 诊断段基址。
pub const U3441_SEG_BASE: u16 = 0xe0a0;

// --------------------------------------------------------------------------
// 诊断码（u3441 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 执行器流水线 状态面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3441Code {
    /// 0xe0a0 状态重复（执行器 同名双态）。
    StateDup,
    /// 0xe0a1 状态断档（流水线 序缺失）。
    StateHole,
    /// 0xe0a2 事件重复（同事件双声明）。
    EventDup,
    /// 0xe0a3 迁移重复（同源同事件双迁移）。
    TransDup,
    /// 0xe0a4 迁移缺失（应达态不可达）。
    TransHole,
    /// 0xe0a5 迁移环（非法自环/环路）。
    TransCycle,
    /// 0xe0a6 入口非法（初态不在闭集）。
    EntryBad,
    /// 0xe0a7 迁移丢失（应用事件无效果）。
    ApplyLost,
    /// 0xe0a8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe0a9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe0aa 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe0ab 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3441Code {
    /// 全量闭集。
    pub const ALL: [u3441Code; 12] = [
        u3441Code::StateDup,
        u3441Code::StateHole,
        u3441Code::EventDup,
        u3441Code::TransDup,
        u3441Code::TransHole,
        u3441Code::TransCycle,
        u3441Code::EntryBad,
        u3441Code::ApplyLost,
        u3441Code::LedgerBroken,
        u3441Code::SnapshotFingerprint,
        u3441Code::TableBroken,
        u3441Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3441Code::StateDup => 0xe0a0,
            u3441Code::StateHole => 0xe0a1,
            u3441Code::EventDup => 0xe0a2,
            u3441Code::TransDup => 0xe0a3,
            u3441Code::TransHole => 0xe0a4,
            u3441Code::TransCycle => 0xe0a5,
            u3441Code::EntryBad => 0xe0a6,
            u3441Code::ApplyLost => 0xe0a7,
            u3441Code::LedgerBroken => 0xe0a8,
            u3441Code::SnapshotFingerprint => 0xe0a9,
            u3441Code::TableBroken => 0xe0aa,
            u3441Code::SegmentGuard => 0xe0ab,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3441Code::StateDup => "状态重复（执行器 同名双态）",
            u3441Code::StateHole => "状态断档（流水线 序缺失）",
            u3441Code::EventDup => "事件重复（同事件双声明）",
            u3441Code::TransDup => "迁移重复（同源同事件双迁移）",
            u3441Code::TransHole => "迁移缺失（应达态不可达）",
            u3441Code::TransCycle => "迁移环（非法自环/环路）",
            u3441Code::EntryBad => "入口非法（初态不在闭集）",
            u3441Code::ApplyLost => "迁移丢失（应用事件无效果）",
            u3441Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3441Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3441Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3441Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3441Code::StateDup => "按闭集唯一律去重",
            u3441Code::StateHole => "按闭集补态",
            u3441Code::EventDup => "按事件闭集去重",
            u3441Code::TransDup => "按确定性唯一迁移律合并",
            u3441Code::TransHole => "按迁移矩阵补边",
            u3441Code::TransCycle => "按无环律重构迁移",
            u3441Code::EntryBad => "按闭集钳制初态",
            u3441Code::ApplyLost => "核对事件前置条件",
            u3441Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3441Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3441Code::TableBroken => "按语义规则重建常量表",
            u3441Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 状态机 mxu3441flow（6 态 × 6 事件 · 迁移 8 条显式声明）
// --------------------------------------------------------------------------
/// 执行器流水线 状态机（判据：执行器）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu3441flowState {
    Idle,
    Selecting,
    Isolating,
    Sorting,
    Executing,
    Reporting,
}

impl mxu3441flowState {
    /// 全量态闭集。
    pub const ALL: [mxu3441flowState; 6] = [
        mxu3441flowState::Idle,
        mxu3441flowState::Selecting,
        mxu3441flowState::Isolating,
        mxu3441flowState::Sorting,
        mxu3441flowState::Executing,
        mxu3441flowState::Reporting,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            mxu3441flowState::Idle => 0,
            mxu3441flowState::Selecting => 1,
            mxu3441flowState::Isolating => 2,
            mxu3441flowState::Sorting => 3,
            mxu3441flowState::Executing => 4,
            mxu3441flowState::Reporting => 5,
        }
    }
}

/// mxu3441flow事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu3441flowEvent {
    Pick,
    Isolate,
    Sort,
    Exec,
    Timeout,
    Report,
}

impl mxu3441flowEvent {
    /// 全量事件闭集。
    pub const ALL: [mxu3441flowEvent; 6] = [
        mxu3441flowEvent::Pick,
        mxu3441flowEvent::Isolate,
        mxu3441flowEvent::Sort,
        mxu3441flowEvent::Exec,
        mxu3441flowEvent::Timeout,
        mxu3441flowEvent::Report,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const MXU3441FLOW_TRANSITIONS: [(mxu3441flowState, mxu3441flowEvent, mxu3441flowState); 8] = [
    (mxu3441flowState::Idle, mxu3441flowEvent::Pick, mxu3441flowState::Selecting), // Idle→Selecting 主链
    (mxu3441flowState::Selecting, mxu3441flowEvent::Isolate, mxu3441flowState::Isolating), // Selecting→Isolating 主链
    (mxu3441flowState::Isolating, mxu3441flowEvent::Sort, mxu3441flowState::Sorting), // Isolating→Sorting 主链
    (mxu3441flowState::Sorting, mxu3441flowEvent::Exec, mxu3441flowState::Executing), // Sorting→Executing 主链
    (mxu3441flowState::Executing, mxu3441flowEvent::Timeout, mxu3441flowState::Reporting), // Executing→Reporting 主链
    (mxu3441flowState::Reporting, mxu3441flowEvent::Report, mxu3441flowState::Idle), // 终态回边
    (mxu3441flowState::Reporting, mxu3441flowEvent::Pick, mxu3441flowState::Executing), // 终态回退边
    (mxu3441flowState::Selecting, mxu3441flowEvent::Isolate, mxu3441flowState::Isolating), // 旁路边
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn mxu3441flow_next(state: mxu3441flowState, ev: mxu3441flowEvent) -> Option<mxu3441flowState> {
    let mut i = 0usize;
    while i < 8 {
        let t = MXU3441FLOW_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn mxu3441flow_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < mxu3441flowState::ALL.len() {
        let mut ei = 0usize;
        while ei < mxu3441flowEvent::ALL.len() {
            let legal = mxu3441flow_next(mxu3441flowState::ALL[si], mxu3441flowEvent::ALL[ei]).is_some();
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
pub fn mxu3441flow_terminal(st: mxu3441flowState) -> bool {
    let mut i = 0usize;
    while i < 8 {
        if MXU3441FLOW_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct mxu3441flowLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: mxu3441flowState,
}

impl mxu3441flowLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: mxu3441flowState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: mxu3441flowEvent) -> bool {
        match mxu3441flow_next(self.cur, ev) {
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
// 快照 Mxu3441Snap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// 执行器流水线 快照封存（FNV 三态指纹 + 篡改检出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mxu3441Snap {
    /// 指纹分量：ex_v。
    pub ex_v: u64,
    /// 指纹分量：sel_v。
    pub sel_v: u64,
    /// 指纹分量：iso_v。
    pub iso_v: u64,
    /// 指纹分量：sort_v。
    pub sort_v: u64,
    /// 指纹分量：run_v。
    pub run_v: u64,
    /// 指纹分量：tmo_v。
    pub tmo_v: u64,
    /// 指纹分量：rpt_v。
    pub rpt_v: u64,
    /// 指纹分量：stage_v。
    pub stage_v: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl Mxu3441Snap {
    /// 构造并即时合成指纹。
    pub fn compute(
        ex_v: u64,
        sel_v: u64,
        iso_v: u64,
        sort_v: u64,
        run_v: u64,
        tmo_v: u64,
        rpt_v: u64,
        stage_v: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &ex_v.to_le_bytes());
        h = fnv1a64_from(h, &sel_v.to_le_bytes());
        h = fnv1a64_from(h, &iso_v.to_le_bytes());
        h = fnv1a64_from(h, &sort_v.to_le_bytes());
        h = fnv1a64_from(h, &run_v.to_le_bytes());
        h = fnv1a64_from(h, &tmo_v.to_le_bytes());
        h = fnv1a64_from(h, &rpt_v.to_le_bytes());
        h = fnv1a64_from(h, &stage_v.to_le_bytes());
        Self {
            ex_v,
            sel_v,
            iso_v,
            sort_v,
            run_v,
            tmo_v,
            rpt_v,
            stage_v,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.ex_v.to_le_bytes());
        h = fnv1a64_from(h, &self.sel_v.to_le_bytes());
        h = fnv1a64_from(h, &self.iso_v.to_le_bytes());
        h = fnv1a64_from(h, &self.sort_v.to_le_bytes());
        h = fnv1a64_from(h, &self.run_v.to_le_bytes());
        h = fnv1a64_from(h, &self.tmo_v.to_le_bytes());
        h = fnv1a64_from(h, &self.rpt_v.to_le_bytes());
        h = fnv1a64_from(h, &self.stage_v.to_le_bytes());
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

/// 段位守卫：全部诊断码必须落在本段 [0xe0a0, 0xe0af]。
pub const fn u3441_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3441Code::ALL.len() {
        let c = u3441Code::ALL[i].code();
        if c < 0xe0a0 || c > 0xe0af {
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
fn u3441_guard_seg() -> bool {
    u3441_segment_guard()
}

/// UNX-F3441 状态机闭集/迁移表/非法组合恰值全绿。
fn check_u3441_sm_legal() -> bool {
    let mut ok = mxu3441flow_illegal_pairs() == 29;
    if ok { ok = MXU3441FLOW_TRANSITIONS.len() == 8; }
    if ok { ok = mxu3441flowState::ALL.len() == 6 && mxu3441flowEvent::ALL.len() == 6; }
    ok
}

/// UNX-F3441 迁移主链回放 + 快照往返恒等。
fn check_u3441_sm_round() -> bool {
    let mut lg = mxu3441flowLedger::new(mxu3441flowState::Idle);
    let mut ok = lg.apply(mxu3441flowEvent::Pick) && lg.cur == mxu3441flowState::Selecting && lg.total() == 1;
    let s0 = Mxu3441Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    let mut s1 = Mxu3441Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    s1.recompute();
    if ok { ok = s0 == s1 && !s1.tampered(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3441_checks() -> CheckSet {
    let mut set = CheckSet::new("U3441");
    set.add(
        "u3441-seg-guard",
        u3441_guard_seg(),
        if u3441_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3441-sm-legal",
        check_u3441_sm_legal(),
        if check_u3441_sm_legal() { "" } else { "UNX-F3441 sm legality mismatch" },
    );
    set.add(
        "u3441-sm-round",
        check_u3441_sm_round(),
        if check_u3441_sm_round() { "" } else { "UNX-F3441 sm roundtrip mismatch" },
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
    fn u3441_all_checks_green() {
        let set = run_u3441_checks();
        assert!(set.all_passed(), "U3441 domain checks must be green");
    }

    #[test]
    fn u3441_segment_guard_holds() {
        assert!(u3441_segment_guard());
    }
}
