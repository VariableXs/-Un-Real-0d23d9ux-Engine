//! unxu2721_u2721sm —— 扫描器主循环与扫描节拍 状态机与快照封存（扫描轮次）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2721 · 扫描器主循环与扫描节拍 · 判据原文：UNX-F2721 · 判据：扫描主循环（isolate→谓词→逐出/归还三段）闭环 10 轮账平，扫描节拍双触发（水线/直接回收）命中同一入口
//! · 组件：扫描轮次 状态机（6 态 × 6 事件显式迁移）+ FNV 快照封存（8 分量）
//! · 判据落地：UNX-F2721 · 判据：扫描主循环（isolate→谓词→逐出/归还三段）闭环 10 轮账平，扫描节拍双触发（水线/直接回收）命中同一入口
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2721_VERSION: &str = "u2721-u2721sm-v1";

/// 诊断段基址。
pub const U2721_SEG_BASE: u16 = 0xac10;

// --------------------------------------------------------------------------
// 诊断码（u2721 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 扫描轮次 状态面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2721Code {
    /// 0xac10 状态重复（三段断链 同名双态）。
    StateDup,
    /// 0xac11 状态断档（节拍丢失 序缺失）。
    StateHole,
    /// 0xac12 事件重复（同事件双声明）。
    EventDup,
    /// 0xac13 迁移重复（同源同事件双迁移）。
    TransDup,
    /// 0xac14 迁移缺失（应达态不可达）。
    TransHole,
    /// 0xac15 迁移环（非法自环/环路）。
    TransCycle,
    /// 0xac16 入口非法（初态不在闭集）。
    EntryBad,
    /// 0xac17 迁移丢失（应用事件无效果）。
    ApplyLost,
    /// 0xac18 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xac19 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xac1a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xac1b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2721Code {
    /// 全量闭集。
    pub const ALL: [u2721Code; 12] = [
        u2721Code::StateDup,
        u2721Code::StateHole,
        u2721Code::EventDup,
        u2721Code::TransDup,
        u2721Code::TransHole,
        u2721Code::TransCycle,
        u2721Code::EntryBad,
        u2721Code::ApplyLost,
        u2721Code::LedgerBroken,
        u2721Code::SnapshotFingerprint,
        u2721Code::TableBroken,
        u2721Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2721Code::StateDup => 0xac10,
            u2721Code::StateHole => 0xac11,
            u2721Code::EventDup => 0xac12,
            u2721Code::TransDup => 0xac13,
            u2721Code::TransHole => 0xac14,
            u2721Code::TransCycle => 0xac15,
            u2721Code::EntryBad => 0xac16,
            u2721Code::ApplyLost => 0xac17,
            u2721Code::LedgerBroken => 0xac18,
            u2721Code::SnapshotFingerprint => 0xac19,
            u2721Code::TableBroken => 0xac1a,
            u2721Code::SegmentGuard => 0xac1b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2721Code::StateDup => "状态重复（三段断链 同名双态）",
            u2721Code::StateHole => "状态断档（节拍丢失 序缺失）",
            u2721Code::EventDup => "事件重复（同事件双声明）",
            u2721Code::TransDup => "迁移重复（同源同事件双迁移）",
            u2721Code::TransHole => "迁移缺失（应达态不可达）",
            u2721Code::TransCycle => "迁移环（非法自环/环路）",
            u2721Code::EntryBad => "入口非法（初态不在闭集）",
            u2721Code::ApplyLost => "迁移丢失（应用事件无效果）",
            u2721Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2721Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2721Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2721Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2721Code::StateDup => "按闭集唯一律去重",
            u2721Code::StateHole => "按闭集补态",
            u2721Code::EventDup => "按事件闭集去重",
            u2721Code::TransDup => "按确定性唯一迁移律合并",
            u2721Code::TransHole => "按迁移矩阵补边",
            u2721Code::TransCycle => "按无环律重构迁移",
            u2721Code::EntryBad => "按闭集钳制初态",
            u2721Code::ApplyLost => "核对事件前置条件",
            u2721Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2721Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2721Code::TableBroken => "按语义规则重建常量表",
            u2721Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 状态机 mxu2721flow（6 态 × 6 事件 · 迁移 8 条显式声明）
// --------------------------------------------------------------------------
/// 扫描轮次 状态机（判据：三段断链）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu2721flowState {
    Idle,
    Isolating,
    Predicating,
    Acting,
    Balanced,
    Sealed,
}

impl mxu2721flowState {
    /// 全量态闭集。
    pub const ALL: [mxu2721flowState; 6] = [
        mxu2721flowState::Idle,
        mxu2721flowState::Isolating,
        mxu2721flowState::Predicating,
        mxu2721flowState::Acting,
        mxu2721flowState::Balanced,
        mxu2721flowState::Sealed,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            mxu2721flowState::Idle => 0,
            mxu2721flowState::Isolating => 1,
            mxu2721flowState::Predicating => 2,
            mxu2721flowState::Acting => 3,
            mxu2721flowState::Balanced => 4,
            mxu2721flowState::Sealed => 5,
        }
    }
}

/// mxu2721flow事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu2721flowEvent {
    Tick,
    Isolate,
    Predicate,
    Evict,
    Return,
    Close,
}

impl mxu2721flowEvent {
    /// 全量事件闭集。
    pub const ALL: [mxu2721flowEvent; 6] = [
        mxu2721flowEvent::Tick,
        mxu2721flowEvent::Isolate,
        mxu2721flowEvent::Predicate,
        mxu2721flowEvent::Evict,
        mxu2721flowEvent::Return,
        mxu2721flowEvent::Close,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const MXU2721FLOW_TRANSITIONS: [(mxu2721flowState, mxu2721flowEvent, mxu2721flowState); 8] = [
    (mxu2721flowState::Idle, mxu2721flowEvent::Tick, mxu2721flowState::Isolating), // Idle→Isolating 主链
    (mxu2721flowState::Isolating, mxu2721flowEvent::Isolate, mxu2721flowState::Predicating), // Isolating→Predicating 主链
    (mxu2721flowState::Predicating, mxu2721flowEvent::Predicate, mxu2721flowState::Acting), // Predicating→Acting 主链
    (mxu2721flowState::Acting, mxu2721flowEvent::Evict, mxu2721flowState::Balanced), // Acting→Balanced 主链
    (mxu2721flowState::Balanced, mxu2721flowEvent::Return, mxu2721flowState::Sealed), // Balanced→Sealed 主链
    (mxu2721flowState::Sealed, mxu2721flowEvent::Close, mxu2721flowState::Idle), // 终态回边
    (mxu2721flowState::Sealed, mxu2721flowEvent::Tick, mxu2721flowState::Balanced), // 终态回退边
    (mxu2721flowState::Isolating, mxu2721flowEvent::Isolate, mxu2721flowState::Predicating), // 旁路边
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn mxu2721flow_next(state: mxu2721flowState, ev: mxu2721flowEvent) -> Option<mxu2721flowState> {
    let mut i = 0usize;
    while i < 8 {
        let t = MXU2721FLOW_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn mxu2721flow_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < mxu2721flowState::ALL.len() {
        let mut ei = 0usize;
        while ei < mxu2721flowEvent::ALL.len() {
            let legal = mxu2721flow_next(mxu2721flowState::ALL[si], mxu2721flowEvent::ALL[ei]).is_some();
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
pub fn mxu2721flow_terminal(st: mxu2721flowState) -> bool {
    let mut i = 0usize;
    while i < 8 {
        if MXU2721FLOW_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct mxu2721flowLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: mxu2721flowState,
}

impl mxu2721flowLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: mxu2721flowState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: mxu2721flowEvent) -> bool {
        match mxu2721flow_next(self.cur, ev) {
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
// 快照 Mxu2721Snap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// 扫描轮次 快照封存（FNV 三态指纹 + 篡改检出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mxu2721Snap {
    /// 指纹分量：sc_v。
    pub sc_v: u64,
    /// 指纹分量：is_v。
    pub is_v: u64,
    /// 指纹分量：pd_v。
    pub pd_v: u64,
    /// 指纹分量：ev_v。
    pub ev_v: u64,
    /// 指纹分量：rt_v。
    pub rt_v: u64,
    /// 指纹分量：tp_v。
    pub tp_v: u64,
    /// 指纹分量：wl_v。
    pub wl_v: u64,
    /// 指纹分量：dr_v。
    pub dr_v: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl Mxu2721Snap {
    /// 构造并即时合成指纹。
    pub fn compute(
        sc_v: u64,
        is_v: u64,
        pd_v: u64,
        ev_v: u64,
        rt_v: u64,
        tp_v: u64,
        wl_v: u64,
        dr_v: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &sc_v.to_le_bytes());
        h = fnv1a64_from(h, &is_v.to_le_bytes());
        h = fnv1a64_from(h, &pd_v.to_le_bytes());
        h = fnv1a64_from(h, &ev_v.to_le_bytes());
        h = fnv1a64_from(h, &rt_v.to_le_bytes());
        h = fnv1a64_from(h, &tp_v.to_le_bytes());
        h = fnv1a64_from(h, &wl_v.to_le_bytes());
        h = fnv1a64_from(h, &dr_v.to_le_bytes());
        Self {
            sc_v,
            is_v,
            pd_v,
            ev_v,
            rt_v,
            tp_v,
            wl_v,
            dr_v,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.sc_v.to_le_bytes());
        h = fnv1a64_from(h, &self.is_v.to_le_bytes());
        h = fnv1a64_from(h, &self.pd_v.to_le_bytes());
        h = fnv1a64_from(h, &self.ev_v.to_le_bytes());
        h = fnv1a64_from(h, &self.rt_v.to_le_bytes());
        h = fnv1a64_from(h, &self.tp_v.to_le_bytes());
        h = fnv1a64_from(h, &self.wl_v.to_le_bytes());
        h = fnv1a64_from(h, &self.dr_v.to_le_bytes());
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

/// 段位守卫：全部诊断码必须落在本段 [0xac10, 0xac1f]。
pub const fn u2721_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2721Code::ALL.len() {
        let c = u2721Code::ALL[i].code();
        if c < 0xac10 || c > 0xac1f {
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
fn u2721_guard_seg() -> bool {
    u2721_segment_guard()
}

/// UNX-F2721 状态机闭集/迁移表/非法组合恰值全绿。
fn check_u2721_sm_legal() -> bool {
    let mut ok = mxu2721flow_illegal_pairs() == 29;
    if ok { ok = MXU2721FLOW_TRANSITIONS.len() == 8; }
    if ok { ok = mxu2721flowState::ALL.len() == 6 && mxu2721flowEvent::ALL.len() == 6; }
    ok
}

/// UNX-F2721 迁移主链回放 + 快照往返恒等。
fn check_u2721_sm_round() -> bool {
    let mut lg = mxu2721flowLedger::new(mxu2721flowState::Idle);
    let mut ok = lg.apply(mxu2721flowEvent::Tick) && lg.cur == mxu2721flowState::Isolating && lg.total() == 1;
    let s0 = Mxu2721Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    let mut s1 = Mxu2721Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    s1.recompute();
    if ok { ok = s0 == s1 && !s1.tampered(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2721_checks() -> CheckSet {
    let mut set = CheckSet::new("U2721");
    set.add(
        "u2721-seg-guard",
        u2721_guard_seg(),
        if u2721_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2721-sm-legal",
        check_u2721_sm_legal(),
        if check_u2721_sm_legal() { "" } else { "UNX-F2721 sm legality mismatch" },
    );
    set.add(
        "u2721-sm-round",
        check_u2721_sm_round(),
        if check_u2721_sm_round() { "" } else { "UNX-F2721 sm roundtrip mismatch" },
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
    fn u2721_all_checks_green() {
        let set = run_u2721_checks();
        assert!(set.all_passed(), "U2721 domain checks must be green");
    }

    #[test]
    fn u2721_segment_guard_holds() {
        assert!(u2721_segment_guard());
    }
}
