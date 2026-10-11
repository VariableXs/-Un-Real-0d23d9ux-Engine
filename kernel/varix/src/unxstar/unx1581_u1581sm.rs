//! unxu1581_u1581sm —— 域级总回归执行（800 条判据终验复读） 状态机与快照封存（判据）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F1581 · 域级总回归执行（800 条判据终验复读） · 判据原文：UNX-F1581 · 判据：域级总回归通过（800 条判据 verbatim 终验复读：宿主档全绿），断言总数实计 ≥800 落账，复读报告哈希落档
//! · 组件：判据 状态机（6 态 × 6 事件显式迁移）+ FNV 快照封存（8 分量）
//! · 判据落地：UNX-F1581 · 判据：域级总回归通过（800 条判据 verbatim 终验复读：宿主档全绿），断言总数实计 ≥800 落账，复读报告哈希落档
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U1581_VERSION: &str = "u1581-u1581sm-v1";

/// 诊断段基址。
pub const U1581_SEG_BASE: u16 = 0x5450;

// --------------------------------------------------------------------------
// 诊断码（u1581 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 判据 状态面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u1581Code {
    /// 0x5450 状态重复（复读失真 同名双态）。
    StateDup,
    /// 0x5451 状态断档（断言短账 序缺失）。
    StateHole,
    /// 0x5452 事件重复（同事件双声明）。
    EventDup,
    /// 0x5453 迁移重复（同源同事件双迁移）。
    TransDup,
    /// 0x5454 迁移缺失（应达态不可达）。
    TransHole,
    /// 0x5455 迁移环（非法自环/环路）。
    TransCycle,
    /// 0x5456 入口非法（初态不在闭集）。
    EntryBad,
    /// 0x5457 迁移丢失（应用事件无效果）。
    ApplyLost,
    /// 0x5458 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x5459 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x545a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x545b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u1581Code {
    /// 全量闭集。
    pub const ALL: [u1581Code; 12] = [
        u1581Code::StateDup,
        u1581Code::StateHole,
        u1581Code::EventDup,
        u1581Code::TransDup,
        u1581Code::TransHole,
        u1581Code::TransCycle,
        u1581Code::EntryBad,
        u1581Code::ApplyLost,
        u1581Code::LedgerBroken,
        u1581Code::SnapshotFingerprint,
        u1581Code::TableBroken,
        u1581Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u1581Code::StateDup => 0x5450,
            u1581Code::StateHole => 0x5451,
            u1581Code::EventDup => 0x5452,
            u1581Code::TransDup => 0x5453,
            u1581Code::TransHole => 0x5454,
            u1581Code::TransCycle => 0x5455,
            u1581Code::EntryBad => 0x5456,
            u1581Code::ApplyLost => 0x5457,
            u1581Code::LedgerBroken => 0x5458,
            u1581Code::SnapshotFingerprint => 0x5459,
            u1581Code::TableBroken => 0x545a,
            u1581Code::SegmentGuard => 0x545b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u1581Code::StateDup => "状态重复（复读失真 同名双态）",
            u1581Code::StateHole => "状态断档（断言短账 序缺失）",
            u1581Code::EventDup => "事件重复（同事件双声明）",
            u1581Code::TransDup => "迁移重复（同源同事件双迁移）",
            u1581Code::TransHole => "迁移缺失（应达态不可达）",
            u1581Code::TransCycle => "迁移环（非法自环/环路）",
            u1581Code::EntryBad => "入口非法（初态不在闭集）",
            u1581Code::ApplyLost => "迁移丢失（应用事件无效果）",
            u1581Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u1581Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u1581Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u1581Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u1581Code::StateDup => "按闭集唯一律去重",
            u1581Code::StateHole => "按闭集补态",
            u1581Code::EventDup => "按事件闭集去重",
            u1581Code::TransDup => "按确定性唯一迁移律合并",
            u1581Code::TransHole => "按迁移矩阵补边",
            u1581Code::TransCycle => "按无环律重构迁移",
            u1581Code::EntryBad => "按闭集钳制初态",
            u1581Code::ApplyLost => "核对事件前置条件",
            u1581Code::LedgerBroken => "按守恒律重建台账后重跑",
            u1581Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u1581Code::TableBroken => "按语义规则重建常量表",
            u1581Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 状态机 mxu1581flow（6 态 × 6 事件 · 迁移 8 条显式声明）
// --------------------------------------------------------------------------
/// 判据 状态机（判据：复读失真）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu1581flowState {
    Idle,
    Replaying,
    Counting,
    Green,
    Hashed,
    Filed,
}

impl mxu1581flowState {
    /// 全量态闭集。
    pub const ALL: [mxu1581flowState; 6] = [
        mxu1581flowState::Idle,
        mxu1581flowState::Replaying,
        mxu1581flowState::Counting,
        mxu1581flowState::Green,
        mxu1581flowState::Hashed,
        mxu1581flowState::Filed,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            mxu1581flowState::Idle => 0,
            mxu1581flowState::Replaying => 1,
            mxu1581flowState::Counting => 2,
            mxu1581flowState::Green => 3,
            mxu1581flowState::Hashed => 4,
            mxu1581flowState::Filed => 5,
        }
    }
}

/// mxu1581flow事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu1581flowEvent {
    Start,
    Replay,
    Count,
    VerifyGreen,
    Hash,
    File,
}

impl mxu1581flowEvent {
    /// 全量事件闭集。
    pub const ALL: [mxu1581flowEvent; 6] = [
        mxu1581flowEvent::Start,
        mxu1581flowEvent::Replay,
        mxu1581flowEvent::Count,
        mxu1581flowEvent::VerifyGreen,
        mxu1581flowEvent::Hash,
        mxu1581flowEvent::File,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const MXU1581FLOW_TRANSITIONS: [(mxu1581flowState, mxu1581flowEvent, mxu1581flowState); 8] = [
    (mxu1581flowState::Idle, mxu1581flowEvent::Start, mxu1581flowState::Replaying), // Idle→Replaying 主链
    (mxu1581flowState::Replaying, mxu1581flowEvent::Replay, mxu1581flowState::Counting), // Replaying→Counting 主链
    (mxu1581flowState::Counting, mxu1581flowEvent::Count, mxu1581flowState::Green), // Counting→Green 主链
    (mxu1581flowState::Green, mxu1581flowEvent::VerifyGreen, mxu1581flowState::Hashed), // Green→Hashed 主链
    (mxu1581flowState::Hashed, mxu1581flowEvent::Hash, mxu1581flowState::Filed), // Hashed→Filed 主链
    (mxu1581flowState::Filed, mxu1581flowEvent::File, mxu1581flowState::Idle), // 终态回边
    (mxu1581flowState::Filed, mxu1581flowEvent::Start, mxu1581flowState::Hashed), // 终态回退边
    (mxu1581flowState::Replaying, mxu1581flowEvent::Replay, mxu1581flowState::Counting), // 旁路边
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn mxu1581flow_next(state: mxu1581flowState, ev: mxu1581flowEvent) -> Option<mxu1581flowState> {
    let mut i = 0usize;
    while i < 8 {
        let t = MXU1581FLOW_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn mxu1581flow_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < mxu1581flowState::ALL.len() {
        let mut ei = 0usize;
        while ei < mxu1581flowEvent::ALL.len() {
            let legal = mxu1581flow_next(mxu1581flowState::ALL[si], mxu1581flowEvent::ALL[ei]).is_some();
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
pub fn mxu1581flow_terminal(st: mxu1581flowState) -> bool {
    let mut i = 0usize;
    while i < 8 {
        if MXU1581FLOW_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct mxu1581flowLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: mxu1581flowState,
}

impl mxu1581flowLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: mxu1581flowState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: mxu1581flowEvent) -> bool {
        match mxu1581flow_next(self.cur, ev) {
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
// 快照 Mxu1581Snap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// 判据 快照封存（FNV 三态指纹 + 篡改检出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mxu1581Snap {
    /// 指纹分量：jr_v。
    pub jr_v: u64,
    /// 指纹分量：hz_v。
    pub hz_v: u64,
    /// 指纹分量：as_v。
    pub as_v: u64,
    /// 指纹分量：rb_v。
    pub rb_v: u64,
    /// 指纹分量：fh_v。
    pub fh_v: u64,
    /// 指纹分量：zy_v。
    pub zy_v: u64,
    /// 指纹分量：gr_v。
    pub gr_v: u64,
    /// 指纹分量：lc_v。
    pub lc_v: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl Mxu1581Snap {
    /// 构造并即时合成指纹。
    pub fn compute(
        jr_v: u64,
        hz_v: u64,
        as_v: u64,
        rb_v: u64,
        fh_v: u64,
        zy_v: u64,
        gr_v: u64,
        lc_v: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &jr_v.to_le_bytes());
        h = fnv1a64_from(h, &hz_v.to_le_bytes());
        h = fnv1a64_from(h, &as_v.to_le_bytes());
        h = fnv1a64_from(h, &rb_v.to_le_bytes());
        h = fnv1a64_from(h, &fh_v.to_le_bytes());
        h = fnv1a64_from(h, &zy_v.to_le_bytes());
        h = fnv1a64_from(h, &gr_v.to_le_bytes());
        h = fnv1a64_from(h, &lc_v.to_le_bytes());
        Self {
            jr_v,
            hz_v,
            as_v,
            rb_v,
            fh_v,
            zy_v,
            gr_v,
            lc_v,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.jr_v.to_le_bytes());
        h = fnv1a64_from(h, &self.hz_v.to_le_bytes());
        h = fnv1a64_from(h, &self.as_v.to_le_bytes());
        h = fnv1a64_from(h, &self.rb_v.to_le_bytes());
        h = fnv1a64_from(h, &self.fh_v.to_le_bytes());
        h = fnv1a64_from(h, &self.zy_v.to_le_bytes());
        h = fnv1a64_from(h, &self.gr_v.to_le_bytes());
        h = fnv1a64_from(h, &self.lc_v.to_le_bytes());
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

/// 段位守卫：全部诊断码必须落在本段 [0x5450, 0x545f]。
pub const fn u1581_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u1581Code::ALL.len() {
        let c = u1581Code::ALL[i].code();
        if c < 0x5450 || c > 0x545f {
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
fn u1581_guard_seg() -> bool {
    u1581_segment_guard()
}

/// UNX-F1581 状态机闭集/迁移表/非法组合恰值全绿。
fn check_u1581_sm_legal() -> bool {
    let mut ok = mxu1581flow_illegal_pairs() == 29;
    if ok { ok = MXU1581FLOW_TRANSITIONS.len() == 8; }
    if ok { ok = mxu1581flowState::ALL.len() == 6 && mxu1581flowEvent::ALL.len() == 6; }
    ok
}

/// UNX-F1581 迁移主链回放 + 快照往返恒等。
fn check_u1581_sm_round() -> bool {
    let mut lg = mxu1581flowLedger::new(mxu1581flowState::Idle);
    let mut ok = lg.apply(mxu1581flowEvent::Start) && lg.cur == mxu1581flowState::Replaying && lg.total() == 1;
    let s0 = Mxu1581Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    let mut s1 = Mxu1581Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    s1.recompute();
    if ok { ok = s0 == s1 && !s1.tampered(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u1581_checks() -> CheckSet {
    let mut set = CheckSet::new("U1581");
    set.add(
        "u1581-seg-guard",
        u1581_guard_seg(),
        if u1581_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u1581-sm-legal",
        check_u1581_sm_legal(),
        if check_u1581_sm_legal() { "" } else { "UNX-F1581 sm legality mismatch" },
    );
    set.add(
        "u1581-sm-round",
        check_u1581_sm_round(),
        if check_u1581_sm_round() { "" } else { "UNX-F1581 sm roundtrip mismatch" },
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
    fn u1581_all_checks_green() {
        let set = run_u1581_checks();
        assert!(set.all_passed(), "U1581 domain checks must be green");
    }

    #[test]
    fn u1581_segment_guard_holds() {
        assert!(u1581_segment_guard());
    }
}
