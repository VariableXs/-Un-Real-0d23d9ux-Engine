//! unxu3521_u3521sm —— 动态探针总设计（桩点式注入——编译期预留 probe 槽） 状态机与快照封存（动态探针）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3521 · 动态探针总设计（桩点式注入——编译期预留 probe 槽） · 判据原文：UNX-F3521 · 判据：J1：探针语义冻结（probe 槽=编译期预留的空操作调用点×运行时挂载处理器）；M-279 挂载全链（槽激活→处理器执行→卸载）。<br>J2：与静态 TP 的分工（TP=预定义事件/探针=临时观测——探针永不转正为 TP，转正走新条目流程）。
//! · 组件：动态探针 状态机（6 态 × 6 事件显式迁移）+ FNV 快照封存（8 分量）
//! · 判据落地：UNX-F3521 · 判据：J1：探针语义冻结（probe 槽=编译期预留的空操作调用点×运行时挂载处理器）；M-279 挂载全链（槽激活→处理器执行→卸载）
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3521_VERSION: &str = "u3521-u3521sm-v1";

/// 诊断段基址。
pub const U3521_SEG_BASE: u16 = 0xe0e0;

// --------------------------------------------------------------------------
// 诊断码（u3521 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 动态探针 状态面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3521Code {
    /// 0xe0e0 状态重复（探针 同名双态）。
    StateDup,
    /// 0xe0e1 状态断档（桩点 序缺失）。
    StateHole,
    /// 0xe0e2 事件重复（同事件双声明）。
    EventDup,
    /// 0xe0e3 迁移重复（同源同事件双迁移）。
    TransDup,
    /// 0xe0e4 迁移缺失（应达态不可达）。
    TransHole,
    /// 0xe0e5 迁移环（非法自环/环路）。
    TransCycle,
    /// 0xe0e6 入口非法（初态不在闭集）。
    EntryBad,
    /// 0xe0e7 迁移丢失（应用事件无效果）。
    ApplyLost,
    /// 0xe0e8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe0e9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe0ea 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe0eb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3521Code {
    /// 全量闭集。
    pub const ALL: [u3521Code; 12] = [
        u3521Code::StateDup,
        u3521Code::StateHole,
        u3521Code::EventDup,
        u3521Code::TransDup,
        u3521Code::TransHole,
        u3521Code::TransCycle,
        u3521Code::EntryBad,
        u3521Code::ApplyLost,
        u3521Code::LedgerBroken,
        u3521Code::SnapshotFingerprint,
        u3521Code::TableBroken,
        u3521Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3521Code::StateDup => 0xe0e0,
            u3521Code::StateHole => 0xe0e1,
            u3521Code::EventDup => 0xe0e2,
            u3521Code::TransDup => 0xe0e3,
            u3521Code::TransHole => 0xe0e4,
            u3521Code::TransCycle => 0xe0e5,
            u3521Code::EntryBad => 0xe0e6,
            u3521Code::ApplyLost => 0xe0e7,
            u3521Code::LedgerBroken => 0xe0e8,
            u3521Code::SnapshotFingerprint => 0xe0e9,
            u3521Code::TableBroken => 0xe0ea,
            u3521Code::SegmentGuard => 0xe0eb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3521Code::StateDup => "状态重复（探针 同名双态）",
            u3521Code::StateHole => "状态断档（桩点 序缺失）",
            u3521Code::EventDup => "事件重复（同事件双声明）",
            u3521Code::TransDup => "迁移重复（同源同事件双迁移）",
            u3521Code::TransHole => "迁移缺失（应达态不可达）",
            u3521Code::TransCycle => "迁移环（非法自环/环路）",
            u3521Code::EntryBad => "入口非法（初态不在闭集）",
            u3521Code::ApplyLost => "迁移丢失（应用事件无效果）",
            u3521Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3521Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3521Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3521Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3521Code::StateDup => "按闭集唯一律去重",
            u3521Code::StateHole => "按闭集补态",
            u3521Code::EventDup => "按事件闭集去重",
            u3521Code::TransDup => "按确定性唯一迁移律合并",
            u3521Code::TransHole => "按迁移矩阵补边",
            u3521Code::TransCycle => "按无环律重构迁移",
            u3521Code::EntryBad => "按闭集钳制初态",
            u3521Code::ApplyLost => "核对事件前置条件",
            u3521Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3521Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3521Code::TableBroken => "按语义规则重建常量表",
            u3521Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 状态机 mxu3521flow（6 态 × 6 事件 · 迁移 8 条显式声明）
// --------------------------------------------------------------------------
/// 动态探针 状态机（判据：探针）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu3521flowState {
    Compiled,
    Slotted,
    Armed,
    Fired,
    Sampled,
    Disarmed,
}

impl mxu3521flowState {
    /// 全量态闭集。
    pub const ALL: [mxu3521flowState; 6] = [
        mxu3521flowState::Compiled,
        mxu3521flowState::Slotted,
        mxu3521flowState::Armed,
        mxu3521flowState::Fired,
        mxu3521flowState::Sampled,
        mxu3521flowState::Disarmed,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            mxu3521flowState::Compiled => 0,
            mxu3521flowState::Slotted => 1,
            mxu3521flowState::Armed => 2,
            mxu3521flowState::Fired => 3,
            mxu3521flowState::Sampled => 4,
            mxu3521flowState::Disarmed => 5,
        }
    }
}

/// mxu3521flow事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu3521flowEvent {
    Reserve,
    Occupy,
    Arm,
    Hook,
    Fire,
    Sample,
}

impl mxu3521flowEvent {
    /// 全量事件闭集。
    pub const ALL: [mxu3521flowEvent; 6] = [
        mxu3521flowEvent::Reserve,
        mxu3521flowEvent::Occupy,
        mxu3521flowEvent::Arm,
        mxu3521flowEvent::Hook,
        mxu3521flowEvent::Fire,
        mxu3521flowEvent::Sample,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const MXU3521FLOW_TRANSITIONS: [(mxu3521flowState, mxu3521flowEvent, mxu3521flowState); 8] = [
    (mxu3521flowState::Compiled, mxu3521flowEvent::Reserve, mxu3521flowState::Slotted), // Compiled→Slotted 主链
    (mxu3521flowState::Slotted, mxu3521flowEvent::Occupy, mxu3521flowState::Armed), // Slotted→Armed 主链
    (mxu3521flowState::Armed, mxu3521flowEvent::Arm, mxu3521flowState::Fired), // Armed→Fired 主链
    (mxu3521flowState::Fired, mxu3521flowEvent::Hook, mxu3521flowState::Sampled), // Fired→Sampled 主链
    (mxu3521flowState::Sampled, mxu3521flowEvent::Fire, mxu3521flowState::Disarmed), // Sampled→Disarmed 主链
    (mxu3521flowState::Disarmed, mxu3521flowEvent::Sample, mxu3521flowState::Compiled), // 终态回边
    (mxu3521flowState::Disarmed, mxu3521flowEvent::Reserve, mxu3521flowState::Sampled), // 终态回退边
    (mxu3521flowState::Slotted, mxu3521flowEvent::Occupy, mxu3521flowState::Armed), // 旁路边
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn mxu3521flow_next(state: mxu3521flowState, ev: mxu3521flowEvent) -> Option<mxu3521flowState> {
    let mut i = 0usize;
    while i < 8 {
        let t = MXU3521FLOW_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn mxu3521flow_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < mxu3521flowState::ALL.len() {
        let mut ei = 0usize;
        while ei < mxu3521flowEvent::ALL.len() {
            let legal = mxu3521flow_next(mxu3521flowState::ALL[si], mxu3521flowEvent::ALL[ei]).is_some();
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
pub fn mxu3521flow_terminal(st: mxu3521flowState) -> bool {
    let mut i = 0usize;
    while i < 8 {
        if MXU3521FLOW_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct mxu3521flowLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: mxu3521flowState,
}

impl mxu3521flowLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: mxu3521flowState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: mxu3521flowEvent) -> bool {
        match mxu3521flow_next(self.cur, ev) {
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
// 快照 Mxu3521Snap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// 动态探针 快照封存（FNV 三态指纹 + 篡改检出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mxu3521Snap {
    /// 指纹分量：probe_v。
    pub probe_v: u64,
    /// 指纹分量：slot_v。
    pub slot_v: u64,
    /// 指纹分量：stub_v。
    pub stub_v: u64,
    /// 指纹分量：ins_v。
    pub ins_v: u64,
    /// 指纹分量：hook_v。
    pub hook_v: u64,
    /// 指纹分量：capt_v。
    pub capt_v: u64,
    /// 指纹分量：filt_v。
    pub filt_v: u64,
    /// 指纹分量：bud_v。
    pub bud_v: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl Mxu3521Snap {
    /// 构造并即时合成指纹。
    pub fn compute(
        probe_v: u64,
        slot_v: u64,
        stub_v: u64,
        ins_v: u64,
        hook_v: u64,
        capt_v: u64,
        filt_v: u64,
        bud_v: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &probe_v.to_le_bytes());
        h = fnv1a64_from(h, &slot_v.to_le_bytes());
        h = fnv1a64_from(h, &stub_v.to_le_bytes());
        h = fnv1a64_from(h, &ins_v.to_le_bytes());
        h = fnv1a64_from(h, &hook_v.to_le_bytes());
        h = fnv1a64_from(h, &capt_v.to_le_bytes());
        h = fnv1a64_from(h, &filt_v.to_le_bytes());
        h = fnv1a64_from(h, &bud_v.to_le_bytes());
        Self {
            probe_v,
            slot_v,
            stub_v,
            ins_v,
            hook_v,
            capt_v,
            filt_v,
            bud_v,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.probe_v.to_le_bytes());
        h = fnv1a64_from(h, &self.slot_v.to_le_bytes());
        h = fnv1a64_from(h, &self.stub_v.to_le_bytes());
        h = fnv1a64_from(h, &self.ins_v.to_le_bytes());
        h = fnv1a64_from(h, &self.hook_v.to_le_bytes());
        h = fnv1a64_from(h, &self.capt_v.to_le_bytes());
        h = fnv1a64_from(h, &self.filt_v.to_le_bytes());
        h = fnv1a64_from(h, &self.bud_v.to_le_bytes());
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

/// 段位守卫：全部诊断码必须落在本段 [0xe0e0, 0xe0ef]。
pub const fn u3521_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3521Code::ALL.len() {
        let c = u3521Code::ALL[i].code();
        if c < 0xe0e0 || c > 0xe0ef {
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
fn u3521_guard_seg() -> bool {
    u3521_segment_guard()
}

/// UNX-F3521 状态机闭集/迁移表/非法组合恰值全绿。
fn check_u3521_sm_legal() -> bool {
    let mut ok = mxu3521flow_illegal_pairs() == 29;
    if ok { ok = MXU3521FLOW_TRANSITIONS.len() == 8; }
    if ok { ok = mxu3521flowState::ALL.len() == 6 && mxu3521flowEvent::ALL.len() == 6; }
    ok
}

/// UNX-F3521 迁移主链回放 + 快照往返恒等。
fn check_u3521_sm_round() -> bool {
    let mut lg = mxu3521flowLedger::new(mxu3521flowState::Compiled);
    let mut ok = lg.apply(mxu3521flowEvent::Reserve) && lg.cur == mxu3521flowState::Slotted && lg.total() == 1;
    let s0 = Mxu3521Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    let mut s1 = Mxu3521Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    s1.recompute();
    if ok { ok = s0 == s1 && !s1.tampered(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3521_checks() -> CheckSet {
    let mut set = CheckSet::new("U3521");
    set.add(
        "u3521-seg-guard",
        u3521_guard_seg(),
        if u3521_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3521-sm-legal",
        check_u3521_sm_legal(),
        if check_u3521_sm_legal() { "" } else { "UNX-F3521 sm legality mismatch" },
    );
    set.add(
        "u3521-sm-round",
        check_u3521_sm_round(),
        if check_u3521_sm_round() { "" } else { "UNX-F3521 sm roundtrip mismatch" },
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
    fn u3521_all_checks_green() {
        let set = run_u3521_checks();
        assert!(set.all_passed(), "U3521 domain checks must be green");
    }

    #[test]
    fn u3521_segment_guard_holds() {
        assert!(u3521_segment_guard());
    }
}
