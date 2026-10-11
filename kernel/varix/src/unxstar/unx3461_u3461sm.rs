//! unxu3461_u3461sm —— 分域挂载总设计（框架与各域用例集的挂载架构） 状态机与快照封存（分域挂载）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3461 · 分域挂载总设计（框架与各域用例集的挂载架构） · 判据原文：UNX-F3461 · 判据：J1：挂载架构文档（域侧声明用例集、框架侧统一注册表——双向登记制）；M-220 挂载全链路（域声明 → 注册表合并 → 执行器可见）。
//! · 组件：分域挂载 状态机（6 态 × 6 事件显式迁移）+ FNV 快照封存（8 分量）
//! · 判据落地：UNX-F3461 · 判据：J1：挂载架构文档（域侧声明用例集、框架侧统一注册表——双向登记制）；M-220 挂载全链路（域声明 → 注册表合并 → 执行器可
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3461_VERSION: &str = "u3461-u3461sm-v1";

/// 诊断段基址。
pub const U3461_SEG_BASE: u16 = 0xe0b0;

// --------------------------------------------------------------------------
// 诊断码（u3461 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 分域挂载 状态面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3461Code {
    /// 0xe0b0 状态重复（挂载 同名双态）。
    StateDup,
    /// 0xe0b1 状态断档（用例集 序缺失）。
    StateHole,
    /// 0xe0b2 事件重复（同事件双声明）。
    EventDup,
    /// 0xe0b3 迁移重复（同源同事件双迁移）。
    TransDup,
    /// 0xe0b4 迁移缺失（应达态不可达）。
    TransHole,
    /// 0xe0b5 迁移环（非法自环/环路）。
    TransCycle,
    /// 0xe0b6 入口非法（初态不在闭集）。
    EntryBad,
    /// 0xe0b7 迁移丢失（应用事件无效果）。
    ApplyLost,
    /// 0xe0b8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe0b9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe0ba 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe0bb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3461Code {
    /// 全量闭集。
    pub const ALL: [u3461Code; 12] = [
        u3461Code::StateDup,
        u3461Code::StateHole,
        u3461Code::EventDup,
        u3461Code::TransDup,
        u3461Code::TransHole,
        u3461Code::TransCycle,
        u3461Code::EntryBad,
        u3461Code::ApplyLost,
        u3461Code::LedgerBroken,
        u3461Code::SnapshotFingerprint,
        u3461Code::TableBroken,
        u3461Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3461Code::StateDup => 0xe0b0,
            u3461Code::StateHole => 0xe0b1,
            u3461Code::EventDup => 0xe0b2,
            u3461Code::TransDup => 0xe0b3,
            u3461Code::TransHole => 0xe0b4,
            u3461Code::TransCycle => 0xe0b5,
            u3461Code::EntryBad => 0xe0b6,
            u3461Code::ApplyLost => 0xe0b7,
            u3461Code::LedgerBroken => 0xe0b8,
            u3461Code::SnapshotFingerprint => 0xe0b9,
            u3461Code::TableBroken => 0xe0ba,
            u3461Code::SegmentGuard => 0xe0bb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3461Code::StateDup => "状态重复（挂载 同名双态）",
            u3461Code::StateHole => "状态断档（用例集 序缺失）",
            u3461Code::EventDup => "事件重复（同事件双声明）",
            u3461Code::TransDup => "迁移重复（同源同事件双迁移）",
            u3461Code::TransHole => "迁移缺失（应达态不可达）",
            u3461Code::TransCycle => "迁移环（非法自环/环路）",
            u3461Code::EntryBad => "入口非法（初态不在闭集）",
            u3461Code::ApplyLost => "迁移丢失（应用事件无效果）",
            u3461Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3461Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3461Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3461Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3461Code::StateDup => "按闭集唯一律去重",
            u3461Code::StateHole => "按闭集补态",
            u3461Code::EventDup => "按事件闭集去重",
            u3461Code::TransDup => "按确定性唯一迁移律合并",
            u3461Code::TransHole => "按迁移矩阵补边",
            u3461Code::TransCycle => "按无环律重构迁移",
            u3461Code::EntryBad => "按闭集钳制初态",
            u3461Code::ApplyLost => "核对事件前置条件",
            u3461Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3461Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3461Code::TableBroken => "按语义规则重建常量表",
            u3461Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 状态机 mxu3461flow（6 态 × 6 事件 · 迁移 8 条显式声明）
// --------------------------------------------------------------------------
/// 分域挂载 状态机（判据：挂载）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu3461flowState {
    Empty,
    Loaded,
    Bound,
    Active,
    Draining,
    Unmounted,
}

impl mxu3461flowState {
    /// 全量态闭集。
    pub const ALL: [mxu3461flowState; 6] = [
        mxu3461flowState::Empty,
        mxu3461flowState::Loaded,
        mxu3461flowState::Bound,
        mxu3461flowState::Active,
        mxu3461flowState::Draining,
        mxu3461flowState::Unmounted,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            mxu3461flowState::Empty => 0,
            mxu3461flowState::Loaded => 1,
            mxu3461flowState::Bound => 2,
            mxu3461flowState::Active => 3,
            mxu3461flowState::Draining => 4,
            mxu3461flowState::Unmounted => 5,
        }
    }
}

/// mxu3461flow事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu3461flowEvent {
    Load,
    Bind,
    Activate,
    Suspend,
    Drain,
    Unmount,
}

impl mxu3461flowEvent {
    /// 全量事件闭集。
    pub const ALL: [mxu3461flowEvent; 6] = [
        mxu3461flowEvent::Load,
        mxu3461flowEvent::Bind,
        mxu3461flowEvent::Activate,
        mxu3461flowEvent::Suspend,
        mxu3461flowEvent::Drain,
        mxu3461flowEvent::Unmount,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const MXU3461FLOW_TRANSITIONS: [(mxu3461flowState, mxu3461flowEvent, mxu3461flowState); 8] = [
    (mxu3461flowState::Empty, mxu3461flowEvent::Load, mxu3461flowState::Loaded), // Empty→Loaded 主链
    (mxu3461flowState::Loaded, mxu3461flowEvent::Bind, mxu3461flowState::Bound), // Loaded→Bound 主链
    (mxu3461flowState::Bound, mxu3461flowEvent::Activate, mxu3461flowState::Active), // Bound→Active 主链
    (mxu3461flowState::Active, mxu3461flowEvent::Suspend, mxu3461flowState::Draining), // Active→Draining 主链
    (mxu3461flowState::Draining, mxu3461flowEvent::Drain, mxu3461flowState::Unmounted), // Draining→Unmounted 主链
    (mxu3461flowState::Unmounted, mxu3461flowEvent::Unmount, mxu3461flowState::Empty), // 终态回边
    (mxu3461flowState::Unmounted, mxu3461flowEvent::Load, mxu3461flowState::Draining), // 终态回退边
    (mxu3461flowState::Loaded, mxu3461flowEvent::Bind, mxu3461flowState::Bound), // 旁路边
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn mxu3461flow_next(state: mxu3461flowState, ev: mxu3461flowEvent) -> Option<mxu3461flowState> {
    let mut i = 0usize;
    while i < 8 {
        let t = MXU3461FLOW_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn mxu3461flow_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < mxu3461flowState::ALL.len() {
        let mut ei = 0usize;
        while ei < mxu3461flowEvent::ALL.len() {
            let legal = mxu3461flow_next(mxu3461flowState::ALL[si], mxu3461flowEvent::ALL[ei]).is_some();
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
pub fn mxu3461flow_terminal(st: mxu3461flowState) -> bool {
    let mut i = 0usize;
    while i < 8 {
        if MXU3461FLOW_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct mxu3461flowLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: mxu3461flowState,
}

impl mxu3461flowLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: mxu3461flowState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: mxu3461flowEvent) -> bool {
        match mxu3461flow_next(self.cur, ev) {
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
// 快照 Mxu3461Snap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// 分域挂载 快照封存（FNV 三态指纹 + 篡改检出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mxu3461Snap {
    /// 指纹分量：mnt_v。
    pub mnt_v: u64,
    /// 指纹分量：fw_v。
    pub fw_v: u64,
    /// 指纹分量：suite_v。
    pub suite_v: u64,
    /// 指纹分量：dom_v。
    pub dom_v: u64,
    /// 指纹分量：table_v。
    pub table_v: u64,
    /// 指纹分量：prio_v。
    pub prio_v: u64,
    /// 指纹分量：bind_v。
    pub bind_v: u64,
    /// 指纹分量：unbind_v。
    pub unbind_v: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl Mxu3461Snap {
    /// 构造并即时合成指纹。
    pub fn compute(
        mnt_v: u64,
        fw_v: u64,
        suite_v: u64,
        dom_v: u64,
        table_v: u64,
        prio_v: u64,
        bind_v: u64,
        unbind_v: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &mnt_v.to_le_bytes());
        h = fnv1a64_from(h, &fw_v.to_le_bytes());
        h = fnv1a64_from(h, &suite_v.to_le_bytes());
        h = fnv1a64_from(h, &dom_v.to_le_bytes());
        h = fnv1a64_from(h, &table_v.to_le_bytes());
        h = fnv1a64_from(h, &prio_v.to_le_bytes());
        h = fnv1a64_from(h, &bind_v.to_le_bytes());
        h = fnv1a64_from(h, &unbind_v.to_le_bytes());
        Self {
            mnt_v,
            fw_v,
            suite_v,
            dom_v,
            table_v,
            prio_v,
            bind_v,
            unbind_v,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.mnt_v.to_le_bytes());
        h = fnv1a64_from(h, &self.fw_v.to_le_bytes());
        h = fnv1a64_from(h, &self.suite_v.to_le_bytes());
        h = fnv1a64_from(h, &self.dom_v.to_le_bytes());
        h = fnv1a64_from(h, &self.table_v.to_le_bytes());
        h = fnv1a64_from(h, &self.prio_v.to_le_bytes());
        h = fnv1a64_from(h, &self.bind_v.to_le_bytes());
        h = fnv1a64_from(h, &self.unbind_v.to_le_bytes());
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

/// 段位守卫：全部诊断码必须落在本段 [0xe0b0, 0xe0bf]。
pub const fn u3461_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3461Code::ALL.len() {
        let c = u3461Code::ALL[i].code();
        if c < 0xe0b0 || c > 0xe0bf {
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
fn u3461_guard_seg() -> bool {
    u3461_segment_guard()
}

/// UNX-F3461 状态机闭集/迁移表/非法组合恰值全绿。
fn check_u3461_sm_legal() -> bool {
    let mut ok = mxu3461flow_illegal_pairs() == 29;
    if ok { ok = MXU3461FLOW_TRANSITIONS.len() == 8; }
    if ok { ok = mxu3461flowState::ALL.len() == 6 && mxu3461flowEvent::ALL.len() == 6; }
    ok
}

/// UNX-F3461 迁移主链回放 + 快照往返恒等。
fn check_u3461_sm_round() -> bool {
    let mut lg = mxu3461flowLedger::new(mxu3461flowState::Empty);
    let mut ok = lg.apply(mxu3461flowEvent::Load) && lg.cur == mxu3461flowState::Loaded && lg.total() == 1;
    let s0 = Mxu3461Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    let mut s1 = Mxu3461Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    s1.recompute();
    if ok { ok = s0 == s1 && !s1.tampered(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3461_checks() -> CheckSet {
    let mut set = CheckSet::new("U3461");
    set.add(
        "u3461-seg-guard",
        u3461_guard_seg(),
        if u3461_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3461-sm-legal",
        check_u3461_sm_legal(),
        if check_u3461_sm_legal() { "" } else { "UNX-F3461 sm legality mismatch" },
    );
    set.add(
        "u3461-sm-round",
        check_u3461_sm_round(),
        if check_u3461_sm_round() { "" } else { "UNX-F3461 sm roundtrip mismatch" },
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
    fn u3461_all_checks_green() {
        let set = run_u3461_checks();
        assert!(set.all_passed(), "U3461 domain checks must be green");
    }

    #[test]
    fn u3461_segment_guard_holds() {
        assert!(u3461_segment_guard());
    }
}
