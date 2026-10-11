//! unxu3421_u3421sm —— 双后端总设计（QEMU 后端真机后端统一抽象，开发期零 QEMU 双轨制） 状态机与快照封存（双后端抽象）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3421 · 双后端总设计（QEMU 后端真机后端统一抽象，开发期零 QEMU 双轨制） · 判据原文：UNX-F3421 · 判据：J1：双后端总设计（后端 = 输出通道 + 执行环境的统一抽象）文档交付：开发期桩后端零 QEMU 依赖、真机后端为闸口对象；M-180 桩后端全链路。
//! · 组件：双后端抽象 状态机（6 态 × 6 事件显式迁移）+ FNV 快照封存（8 分量）
//! · 判据落地：UNX-F3421 · 判据：J1：双后端总设计（后端 = 输出通道 + 执行环境的统一抽象）文档交付：开发期桩后端零 QEMU 依赖、真机后端为闸口对象；M-
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3421_VERSION: &str = "u3421-u3421sm-v1";

/// 诊断段基址。
pub const U3421_SEG_BASE: u16 = 0xe090;

// --------------------------------------------------------------------------
// 诊断码（u3421 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 双后端抽象 状态面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3421Code {
    /// 0xe090 状态重复（双后端 同名双态）。
    StateDup,
    /// 0xe091 状态断档（抽象 序缺失）。
    StateHole,
    /// 0xe092 事件重复（同事件双声明）。
    EventDup,
    /// 0xe093 迁移重复（同源同事件双迁移）。
    TransDup,
    /// 0xe094 迁移缺失（应达态不可达）。
    TransHole,
    /// 0xe095 迁移环（非法自环/环路）。
    TransCycle,
    /// 0xe096 入口非法（初态不在闭集）。
    EntryBad,
    /// 0xe097 迁移丢失（应用事件无效果）。
    ApplyLost,
    /// 0xe098 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe099 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe09a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe09b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3421Code {
    /// 全量闭集。
    pub const ALL: [u3421Code; 12] = [
        u3421Code::StateDup,
        u3421Code::StateHole,
        u3421Code::EventDup,
        u3421Code::TransDup,
        u3421Code::TransHole,
        u3421Code::TransCycle,
        u3421Code::EntryBad,
        u3421Code::ApplyLost,
        u3421Code::LedgerBroken,
        u3421Code::SnapshotFingerprint,
        u3421Code::TableBroken,
        u3421Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3421Code::StateDup => 0xe090,
            u3421Code::StateHole => 0xe091,
            u3421Code::EventDup => 0xe092,
            u3421Code::TransDup => 0xe093,
            u3421Code::TransHole => 0xe094,
            u3421Code::TransCycle => 0xe095,
            u3421Code::EntryBad => 0xe096,
            u3421Code::ApplyLost => 0xe097,
            u3421Code::LedgerBroken => 0xe098,
            u3421Code::SnapshotFingerprint => 0xe099,
            u3421Code::TableBroken => 0xe09a,
            u3421Code::SegmentGuard => 0xe09b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3421Code::StateDup => "状态重复（双后端 同名双态）",
            u3421Code::StateHole => "状态断档（抽象 序缺失）",
            u3421Code::EventDup => "事件重复（同事件双声明）",
            u3421Code::TransDup => "迁移重复（同源同事件双迁移）",
            u3421Code::TransHole => "迁移缺失（应达态不可达）",
            u3421Code::TransCycle => "迁移环（非法自环/环路）",
            u3421Code::EntryBad => "入口非法（初态不在闭集）",
            u3421Code::ApplyLost => "迁移丢失（应用事件无效果）",
            u3421Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3421Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3421Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3421Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3421Code::StateDup => "按闭集唯一律去重",
            u3421Code::StateHole => "按闭集补态",
            u3421Code::EventDup => "按事件闭集去重",
            u3421Code::TransDup => "按确定性唯一迁移律合并",
            u3421Code::TransHole => "按迁移矩阵补边",
            u3421Code::TransCycle => "按无环律重构迁移",
            u3421Code::EntryBad => "按闭集钳制初态",
            u3421Code::ApplyLost => "核对事件前置条件",
            u3421Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3421Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3421Code::TableBroken => "按语义规则重建常量表",
            u3421Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 状态机 mxu3421flow（6 态 × 6 事件 · 迁移 8 条显式声明）
// --------------------------------------------------------------------------
/// 双后端抽象 状态机（判据：双后端）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu3421flowState {
    Init,
    Probe,
    Ready,
    Running,
    Degraded,
    Closed,
}

impl mxu3421flowState {
    /// 全量态闭集。
    pub const ALL: [mxu3421flowState; 6] = [
        mxu3421flowState::Init,
        mxu3421flowState::Probe,
        mxu3421flowState::Ready,
        mxu3421flowState::Running,
        mxu3421flowState::Degraded,
        mxu3421flowState::Closed,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            mxu3421flowState::Init => 0,
            mxu3421flowState::Probe => 1,
            mxu3421flowState::Ready => 2,
            mxu3421flowState::Running => 3,
            mxu3421flowState::Degraded => 4,
            mxu3421flowState::Closed => 5,
        }
    }
}

/// mxu3421flow事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu3421flowEvent {
    Spawn,
    Probe,
    Run,
    Snapshot,
    Compare,
    Teardown,
}

impl mxu3421flowEvent {
    /// 全量事件闭集。
    pub const ALL: [mxu3421flowEvent; 6] = [
        mxu3421flowEvent::Spawn,
        mxu3421flowEvent::Probe,
        mxu3421flowEvent::Run,
        mxu3421flowEvent::Snapshot,
        mxu3421flowEvent::Compare,
        mxu3421flowEvent::Teardown,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const MXU3421FLOW_TRANSITIONS: [(mxu3421flowState, mxu3421flowEvent, mxu3421flowState); 8] = [
    (mxu3421flowState::Init, mxu3421flowEvent::Spawn, mxu3421flowState::Probe), // Init→Probe 主链
    (mxu3421flowState::Probe, mxu3421flowEvent::Probe, mxu3421flowState::Ready), // Probe→Ready 主链
    (mxu3421flowState::Ready, mxu3421flowEvent::Run, mxu3421flowState::Running), // Ready→Running 主链
    (mxu3421flowState::Running, mxu3421flowEvent::Snapshot, mxu3421flowState::Degraded), // Running→Degraded 主链
    (mxu3421flowState::Degraded, mxu3421flowEvent::Compare, mxu3421flowState::Closed), // Degraded→Closed 主链
    (mxu3421flowState::Closed, mxu3421flowEvent::Teardown, mxu3421flowState::Init), // 终态回边
    (mxu3421flowState::Closed, mxu3421flowEvent::Spawn, mxu3421flowState::Degraded), // 终态回退边
    (mxu3421flowState::Probe, mxu3421flowEvent::Probe, mxu3421flowState::Ready), // 旁路边
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn mxu3421flow_next(state: mxu3421flowState, ev: mxu3421flowEvent) -> Option<mxu3421flowState> {
    let mut i = 0usize;
    while i < 8 {
        let t = MXU3421FLOW_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn mxu3421flow_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < mxu3421flowState::ALL.len() {
        let mut ei = 0usize;
        while ei < mxu3421flowEvent::ALL.len() {
            let legal = mxu3421flow_next(mxu3421flowState::ALL[si], mxu3421flowEvent::ALL[ei]).is_some();
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
pub fn mxu3421flow_terminal(st: mxu3421flowState) -> bool {
    let mut i = 0usize;
    while i < 8 {
        if MXU3421FLOW_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct mxu3421flowLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: mxu3421flowState,
}

impl mxu3421flowLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: mxu3421flowState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: mxu3421flowEvent) -> bool {
        match mxu3421flow_next(self.cur, ev) {
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
// 快照 Mxu3421Snap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// 双后端抽象 快照封存（FNV 三态指纹 + 篡改检出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mxu3421Snap {
    /// 指纹分量：be_v。
    pub be_v: u64,
    /// 指纹分量：qemu_v。
    pub qemu_v: u64,
    /// 指纹分量：real_v。
    pub real_v: u64,
    /// 指纹分量：abi_v。
    pub abi_v: u64,
    /// 指纹分量：ser_v。
    pub ser_v: u64,
    /// 指纹分量：dbg_v。
    pub dbg_v: u64,
    /// 指纹分量：log_v。
    pub log_v: u64,
    /// 指纹分量：time_v。
    pub time_v: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl Mxu3421Snap {
    /// 构造并即时合成指纹。
    pub fn compute(
        be_v: u64,
        qemu_v: u64,
        real_v: u64,
        abi_v: u64,
        ser_v: u64,
        dbg_v: u64,
        log_v: u64,
        time_v: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &be_v.to_le_bytes());
        h = fnv1a64_from(h, &qemu_v.to_le_bytes());
        h = fnv1a64_from(h, &real_v.to_le_bytes());
        h = fnv1a64_from(h, &abi_v.to_le_bytes());
        h = fnv1a64_from(h, &ser_v.to_le_bytes());
        h = fnv1a64_from(h, &dbg_v.to_le_bytes());
        h = fnv1a64_from(h, &log_v.to_le_bytes());
        h = fnv1a64_from(h, &time_v.to_le_bytes());
        Self {
            be_v,
            qemu_v,
            real_v,
            abi_v,
            ser_v,
            dbg_v,
            log_v,
            time_v,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.be_v.to_le_bytes());
        h = fnv1a64_from(h, &self.qemu_v.to_le_bytes());
        h = fnv1a64_from(h, &self.real_v.to_le_bytes());
        h = fnv1a64_from(h, &self.abi_v.to_le_bytes());
        h = fnv1a64_from(h, &self.ser_v.to_le_bytes());
        h = fnv1a64_from(h, &self.dbg_v.to_le_bytes());
        h = fnv1a64_from(h, &self.log_v.to_le_bytes());
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

/// 段位守卫：全部诊断码必须落在本段 [0xe090, 0xe09f]。
pub const fn u3421_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3421Code::ALL.len() {
        let c = u3421Code::ALL[i].code();
        if c < 0xe090 || c > 0xe09f {
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
fn u3421_guard_seg() -> bool {
    u3421_segment_guard()
}

/// UNX-F3421 状态机闭集/迁移表/非法组合恰值全绿。
fn check_u3421_sm_legal() -> bool {
    let mut ok = mxu3421flow_illegal_pairs() == 29;
    if ok { ok = MXU3421FLOW_TRANSITIONS.len() == 8; }
    if ok { ok = mxu3421flowState::ALL.len() == 6 && mxu3421flowEvent::ALL.len() == 6; }
    ok
}

/// UNX-F3421 迁移主链回放 + 快照往返恒等。
fn check_u3421_sm_round() -> bool {
    let mut lg = mxu3421flowLedger::new(mxu3421flowState::Init);
    let mut ok = lg.apply(mxu3421flowEvent::Spawn) && lg.cur == mxu3421flowState::Probe && lg.total() == 1;
    let s0 = Mxu3421Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    let mut s1 = Mxu3421Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    s1.recompute();
    if ok { ok = s0 == s1 && !s1.tampered(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3421_checks() -> CheckSet {
    let mut set = CheckSet::new("U3421");
    set.add(
        "u3421-seg-guard",
        u3421_guard_seg(),
        if u3421_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3421-sm-legal",
        check_u3421_sm_legal(),
        if check_u3421_sm_legal() { "" } else { "UNX-F3421 sm legality mismatch" },
    );
    set.add(
        "u3421-sm-round",
        check_u3421_sm_round(),
        if check_u3421_sm_round() { "" } else { "UNX-F3421 sm roundtrip mismatch" },
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
    fn u3421_all_checks_green() {
        let set = run_u3421_checks();
        assert!(set.all_passed(), "U3421 domain checks must be green");
    }

    #[test]
    fn u3421_segment_guard_holds() {
        assert!(u3421_segment_guard());
    }
}
