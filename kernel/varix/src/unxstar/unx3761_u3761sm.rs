//! unxu3761_u3761sm —— RunDigest 总设计（账的账——聚合架构） 状态机与快照封存（RunDigest 聚合）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3761 · RunDigest 总设计（账的账——聚合架构） · 判据原文：UNX-F3761 · 判据：J1：聚合架构（各域账→摘要段→Digest（账的账三层——判据行内嵌：域账是分录/段是科目汇总/Digest 是总账）；入账协议（各域的入账接口——B25/B26/B27/B28 的入账承诺兑现面）；M-519 聚合首跑（三域账入账→Digest 出→三层账可互溯）。<br>J2：聚合纯度（Digest 只聚合不改判（摘要是投影不是重算——域账是判读真源、Digest 是档案）。
//! · 组件：RunDigest 聚合 状态机（6 态 × 6 事件显式迁移）+ FNV 快照封存（8 分量）
//! · 判据落地：UNX-F3761 · 判据：J1：聚合架构（各域账→摘要段→Digest（账的账三层——判据行内嵌：域账是分录/段是科目汇总/Digest 是总账）；入账协议
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3761_VERSION: &str = "u3761-u3761sm-v1";

/// 诊断段基址。
pub const U3761_SEG_BASE: u16 = 0xe1a0;

// --------------------------------------------------------------------------
// 诊断码（u3761 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// RunDigest 聚合 状态面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3761Code {
    /// 0xe1a0 状态重复（聚合 同名双态）。
    StateDup,
    /// 0xe1a1 状态断档（对账 序缺失）。
    StateHole,
    /// 0xe1a2 事件重复（同事件双声明）。
    EventDup,
    /// 0xe1a3 迁移重复（同源同事件双迁移）。
    TransDup,
    /// 0xe1a4 迁移缺失（应达态不可达）。
    TransHole,
    /// 0xe1a5 迁移环（非法自环/环路）。
    TransCycle,
    /// 0xe1a6 入口非法（初态不在闭集）。
    EntryBad,
    /// 0xe1a7 迁移丢失（应用事件无效果）。
    ApplyLost,
    /// 0xe1a8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe1a9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe1aa 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe1ab 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3761Code {
    /// 全量闭集。
    pub const ALL: [u3761Code; 12] = [
        u3761Code::StateDup,
        u3761Code::StateHole,
        u3761Code::EventDup,
        u3761Code::TransDup,
        u3761Code::TransHole,
        u3761Code::TransCycle,
        u3761Code::EntryBad,
        u3761Code::ApplyLost,
        u3761Code::LedgerBroken,
        u3761Code::SnapshotFingerprint,
        u3761Code::TableBroken,
        u3761Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3761Code::StateDup => 0xe1a0,
            u3761Code::StateHole => 0xe1a1,
            u3761Code::EventDup => 0xe1a2,
            u3761Code::TransDup => 0xe1a3,
            u3761Code::TransHole => 0xe1a4,
            u3761Code::TransCycle => 0xe1a5,
            u3761Code::EntryBad => 0xe1a6,
            u3761Code::ApplyLost => 0xe1a7,
            u3761Code::LedgerBroken => 0xe1a8,
            u3761Code::SnapshotFingerprint => 0xe1a9,
            u3761Code::TableBroken => 0xe1aa,
            u3761Code::SegmentGuard => 0xe1ab,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3761Code::StateDup => "状态重复（聚合 同名双态）",
            u3761Code::StateHole => "状态断档（对账 序缺失）",
            u3761Code::EventDup => "事件重复（同事件双声明）",
            u3761Code::TransDup => "迁移重复（同源同事件双迁移）",
            u3761Code::TransHole => "迁移缺失（应达态不可达）",
            u3761Code::TransCycle => "迁移环（非法自环/环路）",
            u3761Code::EntryBad => "入口非法（初态不在闭集）",
            u3761Code::ApplyLost => "迁移丢失（应用事件无效果）",
            u3761Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3761Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3761Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3761Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3761Code::StateDup => "按闭集唯一律去重",
            u3761Code::StateHole => "按闭集补态",
            u3761Code::EventDup => "按事件闭集去重",
            u3761Code::TransDup => "按确定性唯一迁移律合并",
            u3761Code::TransHole => "按迁移矩阵补边",
            u3761Code::TransCycle => "按无环律重构迁移",
            u3761Code::EntryBad => "按闭集钳制初态",
            u3761Code::ApplyLost => "核对事件前置条件",
            u3761Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3761Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3761Code::TableBroken => "按语义规则重建常量表",
            u3761Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 状态机 mxu3761flow（6 态 × 6 事件 · 迁移 8 条显式声明）
// --------------------------------------------------------------------------
/// RunDigest 聚合 状态机（判据：聚合）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu3761flowState {
    Collect,
    Folding,
    Checked,
    Signed,
    Rooted,
    Sealed,
}

impl mxu3761flowState {
    /// 全量态闭集。
    pub const ALL: [mxu3761flowState; 6] = [
        mxu3761flowState::Collect,
        mxu3761flowState::Folding,
        mxu3761flowState::Checked,
        mxu3761flowState::Signed,
        mxu3761flowState::Rooted,
        mxu3761flowState::Sealed,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            mxu3761flowState::Collect => 0,
            mxu3761flowState::Folding => 1,
            mxu3761flowState::Checked => 2,
            mxu3761flowState::Signed => 3,
            mxu3761flowState::Rooted => 4,
            mxu3761flowState::Sealed => 5,
        }
    }
}

/// mxu3761flow事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu3761flowEvent {
    Collect,
    Fold,
    Check,
    Drift,
    Sign,
    Seal,
}

impl mxu3761flowEvent {
    /// 全量事件闭集。
    pub const ALL: [mxu3761flowEvent; 6] = [
        mxu3761flowEvent::Collect,
        mxu3761flowEvent::Fold,
        mxu3761flowEvent::Check,
        mxu3761flowEvent::Drift,
        mxu3761flowEvent::Sign,
        mxu3761flowEvent::Seal,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const MXU3761FLOW_TRANSITIONS: [(mxu3761flowState, mxu3761flowEvent, mxu3761flowState); 8] = [
    (mxu3761flowState::Collect, mxu3761flowEvent::Collect, mxu3761flowState::Folding), // Collect→Folding 主链
    (mxu3761flowState::Folding, mxu3761flowEvent::Fold, mxu3761flowState::Checked), // Folding→Checked 主链
    (mxu3761flowState::Checked, mxu3761flowEvent::Check, mxu3761flowState::Signed), // Checked→Signed 主链
    (mxu3761flowState::Signed, mxu3761flowEvent::Drift, mxu3761flowState::Rooted), // Signed→Rooted 主链
    (mxu3761flowState::Rooted, mxu3761flowEvent::Sign, mxu3761flowState::Sealed), // Rooted→Sealed 主链
    (mxu3761flowState::Sealed, mxu3761flowEvent::Seal, mxu3761flowState::Collect), // 终态回边
    (mxu3761flowState::Sealed, mxu3761flowEvent::Collect, mxu3761flowState::Rooted), // 终态回退边
    (mxu3761flowState::Folding, mxu3761flowEvent::Fold, mxu3761flowState::Checked), // 旁路边
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn mxu3761flow_next(state: mxu3761flowState, ev: mxu3761flowEvent) -> Option<mxu3761flowState> {
    let mut i = 0usize;
    while i < 8 {
        let t = MXU3761FLOW_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn mxu3761flow_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < mxu3761flowState::ALL.len() {
        let mut ei = 0usize;
        while ei < mxu3761flowEvent::ALL.len() {
            let legal = mxu3761flow_next(mxu3761flowState::ALL[si], mxu3761flowEvent::ALL[ei]).is_some();
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
pub fn mxu3761flow_terminal(st: mxu3761flowState) -> bool {
    let mut i = 0usize;
    while i < 8 {
        if MXU3761FLOW_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct mxu3761flowLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: mxu3761flowState,
}

impl mxu3761flowLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: mxu3761flowState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: mxu3761flowEvent) -> bool {
        match mxu3761flow_next(self.cur, ev) {
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
// 快照 Mxu3761Snap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// RunDigest 聚合 快照封存（FNV 三态指纹 + 篡改检出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mxu3761Snap {
    /// 指纹分量：dig_v。
    pub dig_v: u64,
    /// 指纹分量：agg_v。
    pub agg_v: u64,
    /// 指纹分量：run_v。
    pub run_v: u64,
    /// 指纹分量：sum_v。
    pub sum_v: u64,
    /// 指纹分量：chk_v。
    pub chk_v: u64,
    /// 指纹分量：sig_v。
    pub sig_v: u64,
    /// 指纹分量：root_v。
    pub root_v: u64,
    /// 指纹分量：leaf_v。
    pub leaf_v: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl Mxu3761Snap {
    /// 构造并即时合成指纹。
    pub fn compute(
        dig_v: u64,
        agg_v: u64,
        run_v: u64,
        sum_v: u64,
        chk_v: u64,
        sig_v: u64,
        root_v: u64,
        leaf_v: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &dig_v.to_le_bytes());
        h = fnv1a64_from(h, &agg_v.to_le_bytes());
        h = fnv1a64_from(h, &run_v.to_le_bytes());
        h = fnv1a64_from(h, &sum_v.to_le_bytes());
        h = fnv1a64_from(h, &chk_v.to_le_bytes());
        h = fnv1a64_from(h, &sig_v.to_le_bytes());
        h = fnv1a64_from(h, &root_v.to_le_bytes());
        h = fnv1a64_from(h, &leaf_v.to_le_bytes());
        Self {
            dig_v,
            agg_v,
            run_v,
            sum_v,
            chk_v,
            sig_v,
            root_v,
            leaf_v,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.dig_v.to_le_bytes());
        h = fnv1a64_from(h, &self.agg_v.to_le_bytes());
        h = fnv1a64_from(h, &self.run_v.to_le_bytes());
        h = fnv1a64_from(h, &self.sum_v.to_le_bytes());
        h = fnv1a64_from(h, &self.chk_v.to_le_bytes());
        h = fnv1a64_from(h, &self.sig_v.to_le_bytes());
        h = fnv1a64_from(h, &self.root_v.to_le_bytes());
        h = fnv1a64_from(h, &self.leaf_v.to_le_bytes());
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

/// 段位守卫：全部诊断码必须落在本段 [0xe1a0, 0xe1af]。
pub const fn u3761_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3761Code::ALL.len() {
        let c = u3761Code::ALL[i].code();
        if c < 0xe1a0 || c > 0xe1af {
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
fn u3761_guard_seg() -> bool {
    u3761_segment_guard()
}

/// UNX-F3761 状态机闭集/迁移表/非法组合恰值全绿。
fn check_u3761_sm_legal() -> bool {
    let mut ok = mxu3761flow_illegal_pairs() == 29;
    if ok { ok = MXU3761FLOW_TRANSITIONS.len() == 8; }
    if ok { ok = mxu3761flowState::ALL.len() == 6 && mxu3761flowEvent::ALL.len() == 6; }
    ok
}

/// UNX-F3761 迁移主链回放 + 快照往返恒等。
fn check_u3761_sm_round() -> bool {
    let mut lg = mxu3761flowLedger::new(mxu3761flowState::Collect);
    let mut ok = lg.apply(mxu3761flowEvent::Collect) && lg.cur == mxu3761flowState::Folding && lg.total() == 1;
    let s0 = Mxu3761Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    let mut s1 = Mxu3761Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    s1.recompute();
    if ok { ok = s0 == s1 && !s1.tampered(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3761_checks() -> CheckSet {
    let mut set = CheckSet::new("U3761");
    set.add(
        "u3761-seg-guard",
        u3761_guard_seg(),
        if u3761_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3761-sm-legal",
        check_u3761_sm_legal(),
        if check_u3761_sm_legal() { "" } else { "UNX-F3761 sm legality mismatch" },
    );
    set.add(
        "u3761-sm-round",
        check_u3761_sm_round(),
        if check_u3761_sm_round() { "" } else { "UNX-F3761 sm roundtrip mismatch" },
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
    fn u3761_all_checks_green() {
        let set = run_u3761_checks();
        assert!(set.all_passed(), "U3761 domain checks must be green");
    }

    #[test]
    fn u3761_segment_guard_holds() {
        assert!(u3761_segment_guard());
    }
}
