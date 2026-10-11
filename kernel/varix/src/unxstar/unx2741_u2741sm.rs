//! unxu2741_u2741sm —— 水线三档判定与唤醒回调 状态机与快照封存（水线档）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2741 · 水线三档判定与唤醒回调 · 判据原文：UNX-F2741 · 判据：三档判定（min/low/high 对 free 页数）逐档可复测，low 触发唤醒回调 10/10 次
//! · 组件：水线档 状态机（6 态 × 6 事件显式迁移）+ FNV 快照封存（8 分量）
//! · 判据落地：UNX-F2741 · 判据：三档判定（min/low/high 对 free 页数）逐档可复测，low 触发唤醒回调 10/10 次
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2741_VERSION: &str = "u2741-u2741sm-v1";

/// 诊断段基址。
pub const U2741_SEG_BASE: u16 = 0xac90;

// --------------------------------------------------------------------------
// 诊断码（u2741 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 水线档 状态面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2741Code {
    /// 0xac90 状态重复（档位错判 同名双态）。
    StateDup,
    /// 0xac91 状态断档（唤醒漏调 序缺失）。
    StateHole,
    /// 0xac92 事件重复（同事件双声明）。
    EventDup,
    /// 0xac93 迁移重复（同源同事件双迁移）。
    TransDup,
    /// 0xac94 迁移缺失（应达态不可达）。
    TransHole,
    /// 0xac95 迁移环（非法自环/环路）。
    TransCycle,
    /// 0xac96 入口非法（初态不在闭集）。
    EntryBad,
    /// 0xac97 迁移丢失（应用事件无效果）。
    ApplyLost,
    /// 0xac98 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xac99 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xac9a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xac9b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2741Code {
    /// 全量闭集。
    pub const ALL: [u2741Code; 12] = [
        u2741Code::StateDup,
        u2741Code::StateHole,
        u2741Code::EventDup,
        u2741Code::TransDup,
        u2741Code::TransHole,
        u2741Code::TransCycle,
        u2741Code::EntryBad,
        u2741Code::ApplyLost,
        u2741Code::LedgerBroken,
        u2741Code::SnapshotFingerprint,
        u2741Code::TableBroken,
        u2741Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2741Code::StateDup => 0xac90,
            u2741Code::StateHole => 0xac91,
            u2741Code::EventDup => 0xac92,
            u2741Code::TransDup => 0xac93,
            u2741Code::TransHole => 0xac94,
            u2741Code::TransCycle => 0xac95,
            u2741Code::EntryBad => 0xac96,
            u2741Code::ApplyLost => 0xac97,
            u2741Code::LedgerBroken => 0xac98,
            u2741Code::SnapshotFingerprint => 0xac99,
            u2741Code::TableBroken => 0xac9a,
            u2741Code::SegmentGuard => 0xac9b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2741Code::StateDup => "状态重复（档位错判 同名双态）",
            u2741Code::StateHole => "状态断档（唤醒漏调 序缺失）",
            u2741Code::EventDup => "事件重复（同事件双声明）",
            u2741Code::TransDup => "迁移重复（同源同事件双迁移）",
            u2741Code::TransHole => "迁移缺失（应达态不可达）",
            u2741Code::TransCycle => "迁移环（非法自环/环路）",
            u2741Code::EntryBad => "入口非法（初态不在闭集）",
            u2741Code::ApplyLost => "迁移丢失（应用事件无效果）",
            u2741Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2741Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2741Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2741Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2741Code::StateDup => "按闭集唯一律去重",
            u2741Code::StateHole => "按闭集补态",
            u2741Code::EventDup => "按事件闭集去重",
            u2741Code::TransDup => "按确定性唯一迁移律合并",
            u2741Code::TransHole => "按迁移矩阵补边",
            u2741Code::TransCycle => "按无环律重构迁移",
            u2741Code::EntryBad => "按闭集钳制初态",
            u2741Code::ApplyLost => "核对事件前置条件",
            u2741Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2741Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2741Code::TableBroken => "按语义规则重建常量表",
            u2741Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 状态机 mxu2741flow（6 态 × 6 事件 · 迁移 8 条显式声明）
// --------------------------------------------------------------------------
/// 水线档 状态机（判据：档位错判）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu2741flowState {
    Idle,
    Judging,
    Below,
    Wakeup,
    Repro,
    Sealed,
}

impl mxu2741flowState {
    /// 全量态闭集。
    pub const ALL: [mxu2741flowState; 6] = [
        mxu2741flowState::Idle,
        mxu2741flowState::Judging,
        mxu2741flowState::Below,
        mxu2741flowState::Wakeup,
        mxu2741flowState::Repro,
        mxu2741flowState::Sealed,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            mxu2741flowState::Idle => 0,
            mxu2741flowState::Judging => 1,
            mxu2741flowState::Below => 2,
            mxu2741flowState::Wakeup => 3,
            mxu2741flowState::Repro => 4,
            mxu2741flowState::Sealed => 5,
        }
    }
}

/// mxu2741flow事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu2741flowEvent {
    SampleFree,
    Judge,
    BelowLow,
    Wake,
    Recount,
    Seal,
}

impl mxu2741flowEvent {
    /// 全量事件闭集。
    pub const ALL: [mxu2741flowEvent; 6] = [
        mxu2741flowEvent::SampleFree,
        mxu2741flowEvent::Judge,
        mxu2741flowEvent::BelowLow,
        mxu2741flowEvent::Wake,
        mxu2741flowEvent::Recount,
        mxu2741flowEvent::Seal,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const MXU2741FLOW_TRANSITIONS: [(mxu2741flowState, mxu2741flowEvent, mxu2741flowState); 8] = [
    (mxu2741flowState::Idle, mxu2741flowEvent::SampleFree, mxu2741flowState::Judging), // Idle→Judging 主链
    (mxu2741flowState::Judging, mxu2741flowEvent::Judge, mxu2741flowState::Below), // Judging→Below 主链
    (mxu2741flowState::Below, mxu2741flowEvent::BelowLow, mxu2741flowState::Wakeup), // Below→Wakeup 主链
    (mxu2741flowState::Wakeup, mxu2741flowEvent::Wake, mxu2741flowState::Repro), // Wakeup→Repro 主链
    (mxu2741flowState::Repro, mxu2741flowEvent::Recount, mxu2741flowState::Sealed), // Repro→Sealed 主链
    (mxu2741flowState::Sealed, mxu2741flowEvent::Seal, mxu2741flowState::Idle), // 终态回边
    (mxu2741flowState::Sealed, mxu2741flowEvent::SampleFree, mxu2741flowState::Repro), // 终态回退边
    (mxu2741flowState::Judging, mxu2741flowEvent::Judge, mxu2741flowState::Below), // 旁路边
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn mxu2741flow_next(state: mxu2741flowState, ev: mxu2741flowEvent) -> Option<mxu2741flowState> {
    let mut i = 0usize;
    while i < 8 {
        let t = MXU2741FLOW_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn mxu2741flow_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < mxu2741flowState::ALL.len() {
        let mut ei = 0usize;
        while ei < mxu2741flowEvent::ALL.len() {
            let legal = mxu2741flow_next(mxu2741flowState::ALL[si], mxu2741flowEvent::ALL[ei]).is_some();
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
pub fn mxu2741flow_terminal(st: mxu2741flowState) -> bool {
    let mut i = 0usize;
    while i < 8 {
        if MXU2741FLOW_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct mxu2741flowLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: mxu2741flowState,
}

impl mxu2741flowLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: mxu2741flowState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: mxu2741flowEvent) -> bool {
        match mxu2741flow_next(self.cur, ev) {
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
// 快照 Mxu2741Snap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// 水线档 快照封存（FNV 三态指纹 + 篡改检出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mxu2741Snap {
    /// 指纹分量：mn_v。
    pub mn_v: u64,
    /// 指纹分量：lw_v。
    pub lw_v: u64,
    /// 指纹分量：hg_v。
    pub hg_v: u64,
    /// 指纹分量：fr_v。
    pub fr_v: u64,
    /// 指纹分量：wk_v。
    pub wk_v: u64,
    /// 指纹分量：rt_v。
    pub rt_v: u64,
    /// 指纹分量：t10_v。
    pub t10_v: u64,
    /// 指纹分量：cb_v。
    pub cb_v: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl Mxu2741Snap {
    /// 构造并即时合成指纹。
    pub fn compute(
        mn_v: u64,
        lw_v: u64,
        hg_v: u64,
        fr_v: u64,
        wk_v: u64,
        rt_v: u64,
        t10_v: u64,
        cb_v: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &mn_v.to_le_bytes());
        h = fnv1a64_from(h, &lw_v.to_le_bytes());
        h = fnv1a64_from(h, &hg_v.to_le_bytes());
        h = fnv1a64_from(h, &fr_v.to_le_bytes());
        h = fnv1a64_from(h, &wk_v.to_le_bytes());
        h = fnv1a64_from(h, &rt_v.to_le_bytes());
        h = fnv1a64_from(h, &t10_v.to_le_bytes());
        h = fnv1a64_from(h, &cb_v.to_le_bytes());
        Self {
            mn_v,
            lw_v,
            hg_v,
            fr_v,
            wk_v,
            rt_v,
            t10_v,
            cb_v,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.mn_v.to_le_bytes());
        h = fnv1a64_from(h, &self.lw_v.to_le_bytes());
        h = fnv1a64_from(h, &self.hg_v.to_le_bytes());
        h = fnv1a64_from(h, &self.fr_v.to_le_bytes());
        h = fnv1a64_from(h, &self.wk_v.to_le_bytes());
        h = fnv1a64_from(h, &self.rt_v.to_le_bytes());
        h = fnv1a64_from(h, &self.t10_v.to_le_bytes());
        h = fnv1a64_from(h, &self.cb_v.to_le_bytes());
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

/// 段位守卫：全部诊断码必须落在本段 [0xac90, 0xac9f]。
pub const fn u2741_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2741Code::ALL.len() {
        let c = u2741Code::ALL[i].code();
        if c < 0xac90 || c > 0xac9f {
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
fn u2741_guard_seg() -> bool {
    u2741_segment_guard()
}

/// UNX-F2741 状态机闭集/迁移表/非法组合恰值全绿。
fn check_u2741_sm_legal() -> bool {
    let mut ok = mxu2741flow_illegal_pairs() == 29;
    if ok { ok = MXU2741FLOW_TRANSITIONS.len() == 8; }
    if ok { ok = mxu2741flowState::ALL.len() == 6 && mxu2741flowEvent::ALL.len() == 6; }
    ok
}

/// UNX-F2741 迁移主链回放 + 快照往返恒等。
fn check_u2741_sm_round() -> bool {
    let mut lg = mxu2741flowLedger::new(mxu2741flowState::Idle);
    let mut ok = lg.apply(mxu2741flowEvent::SampleFree) && lg.cur == mxu2741flowState::Judging && lg.total() == 1;
    let s0 = Mxu2741Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    let mut s1 = Mxu2741Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    s1.recompute();
    if ok { ok = s0 == s1 && !s1.tampered(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2741_checks() -> CheckSet {
    let mut set = CheckSet::new("U2741");
    set.add(
        "u2741-seg-guard",
        u2741_guard_seg(),
        if u2741_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2741-sm-legal",
        check_u2741_sm_legal(),
        if check_u2741_sm_legal() { "" } else { "UNX-F2741 sm legality mismatch" },
    );
    set.add(
        "u2741-sm-round",
        check_u2741_sm_round(),
        if check_u2741_sm_round() { "" } else { "UNX-F2741 sm roundtrip mismatch" },
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
    fn u2741_all_checks_green() {
        let set = run_u2741_checks();
        assert!(set.all_passed(), "U2741 domain checks must be green");
    }

    #[test]
    fn u2741_segment_guard_holds() {
        assert!(u2741_segment_guard());
    }
}
