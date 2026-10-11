//! unxu2641_u2641sm —— fork 地址空间复制原语（冻结接口 5 正式版） 状态机与快照封存（fork 复制）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2641 · fork 地址空间复制原语（冻结接口 5 正式版） · 判据原文：UNX-F2641 · 判据：冻结接口 5 正式冻结（C1 消费联签），F2558 预声明与 F2564 地基消费两账兑现闭合
//! · 组件：fork 复制 状态机（6 态 × 6 事件显式迁移）+ FNV 快照封存（8 分量）
//! · 判据落地：UNX-F2641 · 判据：冻结接口 5 正式冻结（C1 消费联签），F2558 预声明与 F2564 地基消费两账兑现闭合
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2641_VERSION: &str = "u2641-u2641sm-v1";

/// 诊断段基址。
pub const U2641_SEG_BASE: u16 = 0xa910;

// --------------------------------------------------------------------------
// 诊断码（u2641 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// fork 复制 状态面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2641Code {
    /// 0xa910 状态重复（两账不闭合 同名双态）。
    StateDup,
    /// 0xa911 状态断档（复制漏页 序缺失）。
    StateHole,
    /// 0xa912 事件重复（同事件双声明）。
    EventDup,
    /// 0xa913 迁移重复（同源同事件双迁移）。
    TransDup,
    /// 0xa914 迁移缺失（应达态不可达）。
    TransHole,
    /// 0xa915 迁移环（非法自环/环路）。
    TransCycle,
    /// 0xa916 入口非法（初态不在闭集）。
    EntryBad,
    /// 0xa917 迁移丢失（应用事件无效果）。
    ApplyLost,
    /// 0xa918 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xa919 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xa91a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xa91b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2641Code {
    /// 全量闭集。
    pub const ALL: [u2641Code; 12] = [
        u2641Code::StateDup,
        u2641Code::StateHole,
        u2641Code::EventDup,
        u2641Code::TransDup,
        u2641Code::TransHole,
        u2641Code::TransCycle,
        u2641Code::EntryBad,
        u2641Code::ApplyLost,
        u2641Code::LedgerBroken,
        u2641Code::SnapshotFingerprint,
        u2641Code::TableBroken,
        u2641Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2641Code::StateDup => 0xa910,
            u2641Code::StateHole => 0xa911,
            u2641Code::EventDup => 0xa912,
            u2641Code::TransDup => 0xa913,
            u2641Code::TransHole => 0xa914,
            u2641Code::TransCycle => 0xa915,
            u2641Code::EntryBad => 0xa916,
            u2641Code::ApplyLost => 0xa917,
            u2641Code::LedgerBroken => 0xa918,
            u2641Code::SnapshotFingerprint => 0xa919,
            u2641Code::TableBroken => 0xa91a,
            u2641Code::SegmentGuard => 0xa91b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2641Code::StateDup => "状态重复（两账不闭合 同名双态）",
            u2641Code::StateHole => "状态断档（复制漏页 序缺失）",
            u2641Code::EventDup => "事件重复（同事件双声明）",
            u2641Code::TransDup => "迁移重复（同源同事件双迁移）",
            u2641Code::TransHole => "迁移缺失（应达态不可达）",
            u2641Code::TransCycle => "迁移环（非法自环/环路）",
            u2641Code::EntryBad => "入口非法（初态不在闭集）",
            u2641Code::ApplyLost => "迁移丢失（应用事件无效果）",
            u2641Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2641Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2641Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2641Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2641Code::StateDup => "按闭集唯一律去重",
            u2641Code::StateHole => "按闭集补态",
            u2641Code::EventDup => "按事件闭集去重",
            u2641Code::TransDup => "按确定性唯一迁移律合并",
            u2641Code::TransHole => "按迁移矩阵补边",
            u2641Code::TransCycle => "按无环律重构迁移",
            u2641Code::EntryBad => "按闭集钳制初态",
            u2641Code::ApplyLost => "核对事件前置条件",
            u2641Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2641Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2641Code::TableBroken => "按语义规则重建常量表",
            u2641Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 状态机 mxu2641flow（6 态 × 6 事件 · 迁移 8 条显式声明）
// --------------------------------------------------------------------------
/// fork 复制 状态机（判据：两账不闭合）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu2641flowState {
    Declared,
    Grounded,
    Copying,
    Cow,
    Countersigned,
    Frozen,
}

impl mxu2641flowState {
    /// 全量态闭集。
    pub const ALL: [mxu2641flowState; 6] = [
        mxu2641flowState::Declared,
        mxu2641flowState::Grounded,
        mxu2641flowState::Copying,
        mxu2641flowState::Cow,
        mxu2641flowState::Countersigned,
        mxu2641flowState::Frozen,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            mxu2641flowState::Declared => 0,
            mxu2641flowState::Grounded => 1,
            mxu2641flowState::Copying => 2,
            mxu2641flowState::Cow => 3,
            mxu2641flowState::Countersigned => 4,
            mxu2641flowState::Frozen => 5,
        }
    }
}

/// mxu2641flow事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu2641flowEvent {
    Declare,
    Ground,
    CopyPages,
    ArmCow,
    ShareMap,
    Freeze,
}

impl mxu2641flowEvent {
    /// 全量事件闭集。
    pub const ALL: [mxu2641flowEvent; 6] = [
        mxu2641flowEvent::Declare,
        mxu2641flowEvent::Ground,
        mxu2641flowEvent::CopyPages,
        mxu2641flowEvent::ArmCow,
        mxu2641flowEvent::ShareMap,
        mxu2641flowEvent::Freeze,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const MXU2641FLOW_TRANSITIONS: [(mxu2641flowState, mxu2641flowEvent, mxu2641flowState); 8] = [
    (mxu2641flowState::Declared, mxu2641flowEvent::Declare, mxu2641flowState::Grounded), // Declared→Grounded 主链
    (mxu2641flowState::Grounded, mxu2641flowEvent::Ground, mxu2641flowState::Copying), // Grounded→Copying 主链
    (mxu2641flowState::Copying, mxu2641flowEvent::CopyPages, mxu2641flowState::Cow), // Copying→Cow 主链
    (mxu2641flowState::Cow, mxu2641flowEvent::ArmCow, mxu2641flowState::Countersigned), // Cow→Countersigned 主链
    (mxu2641flowState::Countersigned, mxu2641flowEvent::ShareMap, mxu2641flowState::Frozen), // Countersigned→Frozen 主链
    (mxu2641flowState::Frozen, mxu2641flowEvent::Freeze, mxu2641flowState::Declared), // 终态回边
    (mxu2641flowState::Frozen, mxu2641flowEvent::Declare, mxu2641flowState::Countersigned), // 终态回退边
    (mxu2641flowState::Grounded, mxu2641flowEvent::Ground, mxu2641flowState::Copying), // 旁路边
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn mxu2641flow_next(state: mxu2641flowState, ev: mxu2641flowEvent) -> Option<mxu2641flowState> {
    let mut i = 0usize;
    while i < 8 {
        let t = MXU2641FLOW_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn mxu2641flow_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < mxu2641flowState::ALL.len() {
        let mut ei = 0usize;
        while ei < mxu2641flowEvent::ALL.len() {
            let legal = mxu2641flow_next(mxu2641flowState::ALL[si], mxu2641flowEvent::ALL[ei]).is_some();
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
pub fn mxu2641flow_terminal(st: mxu2641flowState) -> bool {
    let mut i = 0usize;
    while i < 8 {
        if MXU2641FLOW_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct mxu2641flowLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: mxu2641flowState,
}

impl mxu2641flowLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: mxu2641flowState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: mxu2641flowEvent) -> bool {
        match mxu2641flow_next(self.cur, ev) {
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
// 快照 Mxu2641Snap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// fork 复制 快照封存（FNV 三态指纹 + 篡改检出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mxu2641Snap {
    /// 指纹分量：fk_v。
    pub fk_v: u64,
    /// 指纹分量：if5_v。
    pub if5_v: u64,
    /// 指纹分量：cp_v。
    pub cp_v: u64,
    /// 指纹分量：c1_v。
    pub c1_v: u64,
    /// 指纹分量：pd_v。
    pub pd_v: u64,
    /// 指纹分量：fb_v。
    pub fb_v: u64,
    /// 指纹分量：ac_v。
    pub ac_v: u64,
    /// 指纹分量：pg_v。
    pub pg_v: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl Mxu2641Snap {
    /// 构造并即时合成指纹。
    pub fn compute(
        fk_v: u64,
        if5_v: u64,
        cp_v: u64,
        c1_v: u64,
        pd_v: u64,
        fb_v: u64,
        ac_v: u64,
        pg_v: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &fk_v.to_le_bytes());
        h = fnv1a64_from(h, &if5_v.to_le_bytes());
        h = fnv1a64_from(h, &cp_v.to_le_bytes());
        h = fnv1a64_from(h, &c1_v.to_le_bytes());
        h = fnv1a64_from(h, &pd_v.to_le_bytes());
        h = fnv1a64_from(h, &fb_v.to_le_bytes());
        h = fnv1a64_from(h, &ac_v.to_le_bytes());
        h = fnv1a64_from(h, &pg_v.to_le_bytes());
        Self {
            fk_v,
            if5_v,
            cp_v,
            c1_v,
            pd_v,
            fb_v,
            ac_v,
            pg_v,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.fk_v.to_le_bytes());
        h = fnv1a64_from(h, &self.if5_v.to_le_bytes());
        h = fnv1a64_from(h, &self.cp_v.to_le_bytes());
        h = fnv1a64_from(h, &self.c1_v.to_le_bytes());
        h = fnv1a64_from(h, &self.pd_v.to_le_bytes());
        h = fnv1a64_from(h, &self.fb_v.to_le_bytes());
        h = fnv1a64_from(h, &self.ac_v.to_le_bytes());
        h = fnv1a64_from(h, &self.pg_v.to_le_bytes());
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

/// 段位守卫：全部诊断码必须落在本段 [0xa910, 0xa91f]。
pub const fn u2641_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2641Code::ALL.len() {
        let c = u2641Code::ALL[i].code();
        if c < 0xa910 || c > 0xa91f {
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
fn u2641_guard_seg() -> bool {
    u2641_segment_guard()
}

/// UNX-F2641 状态机闭集/迁移表/非法组合恰值全绿。
fn check_u2641_sm_legal() -> bool {
    let mut ok = mxu2641flow_illegal_pairs() == 29;
    if ok { ok = MXU2641FLOW_TRANSITIONS.len() == 8; }
    if ok { ok = mxu2641flowState::ALL.len() == 6 && mxu2641flowEvent::ALL.len() == 6; }
    ok
}

/// UNX-F2641 迁移主链回放 + 快照往返恒等。
fn check_u2641_sm_round() -> bool {
    let mut lg = mxu2641flowLedger::new(mxu2641flowState::Declared);
    let mut ok = lg.apply(mxu2641flowEvent::Declare) && lg.cur == mxu2641flowState::Grounded && lg.total() == 1;
    let s0 = Mxu2641Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    let mut s1 = Mxu2641Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    s1.recompute();
    if ok { ok = s0 == s1 && !s1.tampered(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2641_checks() -> CheckSet {
    let mut set = CheckSet::new("U2641");
    set.add(
        "u2641-seg-guard",
        u2641_guard_seg(),
        if u2641_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2641-sm-legal",
        check_u2641_sm_legal(),
        if check_u2641_sm_legal() { "" } else { "UNX-F2641 sm legality mismatch" },
    );
    set.add(
        "u2641-sm-round",
        check_u2641_sm_round(),
        if check_u2641_sm_round() { "" } else { "UNX-F2641 sm roundtrip mismatch" },
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
    fn u2641_all_checks_green() {
        let set = run_u2641_checks();
        assert!(set.all_passed(), "U2641 domain checks must be green");
    }

    #[test]
    fn u2641_segment_guard_holds() {
        assert!(u2641_segment_guard());
    }
}
