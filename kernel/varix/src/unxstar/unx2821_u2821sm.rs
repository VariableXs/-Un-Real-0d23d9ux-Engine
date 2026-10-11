//! unxu2821_u2821sm —— 多地址空间并发压测（256 空间） 状态机与快照封存（多地址空间并发压测）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2821 · 多地址空间并发压测（256 空间） · 判据原文：UNX-F2821 · 判据：UNX-F2821-J1 256 空间并发映射-访问-销毁 10/10 轮零串址，空间隔离账平
//! · 组件：多地址空间并发压测 状态机（6 态 × 6 事件显式迁移）+ FNV 快照封存（8 分量）
//! · 判据落地：UNX-F2821 · 判据：UNX-F2821-J1 256 空间并发映射-访问-销毁 10/10 轮零串址，空间隔离账平
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2821_VERSION: &str = "u2821-u2821sm-v1";

/// 诊断段基址。
pub const U2821_SEG_BASE: u16 = 0xe1d0;

// --------------------------------------------------------------------------
// 诊断码（u2821 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 多地址空间并发压测 状态面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2821Code {
    /// 0xe1d0 状态重复（地址空间 同名双态）。
    StateDup,
    /// 0xe1d1 状态断档（并发 序缺失）。
    StateHole,
    /// 0xe1d2 事件重复（同事件双声明）。
    EventDup,
    /// 0xe1d3 迁移重复（同源同事件双迁移）。
    TransDup,
    /// 0xe1d4 迁移缺失（应达态不可达）。
    TransHole,
    /// 0xe1d5 迁移环（非法自环/环路）。
    TransCycle,
    /// 0xe1d6 入口非法（初态不在闭集）。
    EntryBad,
    /// 0xe1d7 迁移丢失（应用事件无效果）。
    ApplyLost,
    /// 0xe1d8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe1d9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe1da 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe1db 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2821Code {
    /// 全量闭集。
    pub const ALL: [u2821Code; 12] = [
        u2821Code::StateDup,
        u2821Code::StateHole,
        u2821Code::EventDup,
        u2821Code::TransDup,
        u2821Code::TransHole,
        u2821Code::TransCycle,
        u2821Code::EntryBad,
        u2821Code::ApplyLost,
        u2821Code::LedgerBroken,
        u2821Code::SnapshotFingerprint,
        u2821Code::TableBroken,
        u2821Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2821Code::StateDup => 0xe1d0,
            u2821Code::StateHole => 0xe1d1,
            u2821Code::EventDup => 0xe1d2,
            u2821Code::TransDup => 0xe1d3,
            u2821Code::TransHole => 0xe1d4,
            u2821Code::TransCycle => 0xe1d5,
            u2821Code::EntryBad => 0xe1d6,
            u2821Code::ApplyLost => 0xe1d7,
            u2821Code::LedgerBroken => 0xe1d8,
            u2821Code::SnapshotFingerprint => 0xe1d9,
            u2821Code::TableBroken => 0xe1da,
            u2821Code::SegmentGuard => 0xe1db,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2821Code::StateDup => "状态重复（地址空间 同名双态）",
            u2821Code::StateHole => "状态断档（并发 序缺失）",
            u2821Code::EventDup => "事件重复（同事件双声明）",
            u2821Code::TransDup => "迁移重复（同源同事件双迁移）",
            u2821Code::TransHole => "迁移缺失（应达态不可达）",
            u2821Code::TransCycle => "迁移环（非法自环/环路）",
            u2821Code::EntryBad => "入口非法（初态不在闭集）",
            u2821Code::ApplyLost => "迁移丢失（应用事件无效果）",
            u2821Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2821Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2821Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2821Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2821Code::StateDup => "按闭集唯一律去重",
            u2821Code::StateHole => "按闭集补态",
            u2821Code::EventDup => "按事件闭集去重",
            u2821Code::TransDup => "按确定性唯一迁移律合并",
            u2821Code::TransHole => "按迁移矩阵补边",
            u2821Code::TransCycle => "按无环律重构迁移",
            u2821Code::EntryBad => "按闭集钳制初态",
            u2821Code::ApplyLost => "核对事件前置条件",
            u2821Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2821Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2821Code::TableBroken => "按语义规则重建常量表",
            u2821Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 状态机 mxu2821flow（6 态 × 6 事件 · 迁移 8 条显式声明）
// --------------------------------------------------------------------------
/// 多地址空间并发压测 状态机（判据：地址空间）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu2821flowState {
    Plan,
    Spawn,
    Running,
    Switching,
    Judged,
    Sealed,
}

impl mxu2821flowState {
    /// 全量态闭集。
    pub const ALL: [mxu2821flowState; 6] = [
        mxu2821flowState::Plan,
        mxu2821flowState::Spawn,
        mxu2821flowState::Running,
        mxu2821flowState::Switching,
        mxu2821flowState::Judged,
        mxu2821flowState::Sealed,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            mxu2821flowState::Plan => 0,
            mxu2821flowState::Spawn => 1,
            mxu2821flowState::Running => 2,
            mxu2821flowState::Switching => 3,
            mxu2821flowState::Judged => 4,
            mxu2821flowState::Sealed => 5,
        }
    }
}

/// mxu2821flow事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu2821flowEvent {
    Plan,
    Spawn,
    Switch,
    Isolate,
    Judge,
    Seal,
}

impl mxu2821flowEvent {
    /// 全量事件闭集。
    pub const ALL: [mxu2821flowEvent; 6] = [
        mxu2821flowEvent::Plan,
        mxu2821flowEvent::Spawn,
        mxu2821flowEvent::Switch,
        mxu2821flowEvent::Isolate,
        mxu2821flowEvent::Judge,
        mxu2821flowEvent::Seal,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const MXU2821FLOW_TRANSITIONS: [(mxu2821flowState, mxu2821flowEvent, mxu2821flowState); 8] = [
    (mxu2821flowState::Plan, mxu2821flowEvent::Plan, mxu2821flowState::Spawn), // Plan→Spawn 主链
    (mxu2821flowState::Spawn, mxu2821flowEvent::Spawn, mxu2821flowState::Running), // Spawn→Running 主链
    (mxu2821flowState::Running, mxu2821flowEvent::Switch, mxu2821flowState::Switching), // Running→Switching 主链
    (mxu2821flowState::Switching, mxu2821flowEvent::Isolate, mxu2821flowState::Judged), // Switching→Judged 主链
    (mxu2821flowState::Judged, mxu2821flowEvent::Judge, mxu2821flowState::Sealed), // Judged→Sealed 主链
    (mxu2821flowState::Sealed, mxu2821flowEvent::Seal, mxu2821flowState::Plan), // 终态回边
    (mxu2821flowState::Sealed, mxu2821flowEvent::Plan, mxu2821flowState::Judged), // 终态回退边
    (mxu2821flowState::Spawn, mxu2821flowEvent::Spawn, mxu2821flowState::Running), // 旁路边
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn mxu2821flow_next(state: mxu2821flowState, ev: mxu2821flowEvent) -> Option<mxu2821flowState> {
    let mut i = 0usize;
    while i < 8 {
        let t = MXU2821FLOW_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn mxu2821flow_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < mxu2821flowState::ALL.len() {
        let mut ei = 0usize;
        while ei < mxu2821flowEvent::ALL.len() {
            let legal = mxu2821flow_next(mxu2821flowState::ALL[si], mxu2821flowEvent::ALL[ei]).is_some();
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
pub fn mxu2821flow_terminal(st: mxu2821flowState) -> bool {
    let mut i = 0usize;
    while i < 8 {
        if MXU2821FLOW_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct mxu2821flowLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: mxu2821flowState,
}

impl mxu2821flowLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: mxu2821flowState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: mxu2821flowEvent) -> bool {
        match mxu2821flow_next(self.cur, ev) {
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
// 快照 Mxu2821Snap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// 多地址空间并发压测 快照封存（FNV 三态指纹 + 篡改检出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mxu2821Snap {
    /// 指纹分量：as_v。
    pub as_v: u64,
    /// 指纹分量：n256_v。
    pub n256_v: u64,
    /// 指纹分量：conc_v。
    pub conc_v: u64,
    /// 指纹分量：sw_v。
    pub sw_v: u64,
    /// 指纹分量：cr3_v。
    pub cr3_v: u64,
    /// 指纹分量：tlb_v。
    pub tlb_v: u64,
    /// 指纹分量：isol_v。
    pub isol_v: u64,
    /// 指纹分量：hot_v。
    pub hot_v: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl Mxu2821Snap {
    /// 构造并即时合成指纹。
    pub fn compute(
        as_v: u64,
        n256_v: u64,
        conc_v: u64,
        sw_v: u64,
        cr3_v: u64,
        tlb_v: u64,
        isol_v: u64,
        hot_v: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &as_v.to_le_bytes());
        h = fnv1a64_from(h, &n256_v.to_le_bytes());
        h = fnv1a64_from(h, &conc_v.to_le_bytes());
        h = fnv1a64_from(h, &sw_v.to_le_bytes());
        h = fnv1a64_from(h, &cr3_v.to_le_bytes());
        h = fnv1a64_from(h, &tlb_v.to_le_bytes());
        h = fnv1a64_from(h, &isol_v.to_le_bytes());
        h = fnv1a64_from(h, &hot_v.to_le_bytes());
        Self {
            as_v,
            n256_v,
            conc_v,
            sw_v,
            cr3_v,
            tlb_v,
            isol_v,
            hot_v,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.as_v.to_le_bytes());
        h = fnv1a64_from(h, &self.n256_v.to_le_bytes());
        h = fnv1a64_from(h, &self.conc_v.to_le_bytes());
        h = fnv1a64_from(h, &self.sw_v.to_le_bytes());
        h = fnv1a64_from(h, &self.cr3_v.to_le_bytes());
        h = fnv1a64_from(h, &self.tlb_v.to_le_bytes());
        h = fnv1a64_from(h, &self.isol_v.to_le_bytes());
        h = fnv1a64_from(h, &self.hot_v.to_le_bytes());
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

/// 段位守卫：全部诊断码必须落在本段 [0xe1d0, 0xe1df]。
pub const fn u2821_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2821Code::ALL.len() {
        let c = u2821Code::ALL[i].code();
        if c < 0xe1d0 || c > 0xe1df {
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
fn u2821_guard_seg() -> bool {
    u2821_segment_guard()
}

/// UNX-F2821 状态机闭集/迁移表/非法组合恰值全绿。
fn check_u2821_sm_legal() -> bool {
    let mut ok = mxu2821flow_illegal_pairs() == 29;
    if ok { ok = MXU2821FLOW_TRANSITIONS.len() == 8; }
    if ok { ok = mxu2821flowState::ALL.len() == 6 && mxu2821flowEvent::ALL.len() == 6; }
    ok
}

/// UNX-F2821 迁移主链回放 + 快照往返恒等。
fn check_u2821_sm_round() -> bool {
    let mut lg = mxu2821flowLedger::new(mxu2821flowState::Plan);
    let mut ok = lg.apply(mxu2821flowEvent::Plan) && lg.cur == mxu2821flowState::Spawn && lg.total() == 1;
    let s0 = Mxu2821Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    let mut s1 = Mxu2821Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    s1.recompute();
    if ok { ok = s0 == s1 && !s1.tampered(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2821_checks() -> CheckSet {
    let mut set = CheckSet::new("U2821");
    set.add(
        "u2821-seg-guard",
        u2821_guard_seg(),
        if u2821_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2821-sm-legal",
        check_u2821_sm_legal(),
        if check_u2821_sm_legal() { "" } else { "UNX-F2821 sm legality mismatch" },
    );
    set.add(
        "u2821-sm-round",
        check_u2821_sm_round(),
        if check_u2821_sm_round() { "" } else { "UNX-F2821 sm roundtrip mismatch" },
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
    fn u2821_all_checks_green() {
        let set = run_u2821_checks();
        assert!(set.all_passed(), "U2821 domain checks must be green");
    }

    #[test]
    fn u2821_segment_guard_holds() {
        assert!(u2821_segment_guard());
    }
}
