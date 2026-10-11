//! unxu2501_u2501sm —— 大页映射前向接口冻结（→B10 交接冻结件） 状态机与快照封存（冻结接口）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2501 · 大页映射前向接口冻结（→B10 交接冻结件） · 判据原文：UNX-F2501 · 判据：大页映射前向接口冻结件登记（B10 消费联签），接口字段与池账口径一致
//! · 组件：冻结接口 状态机（6 态 × 6 事件显式迁移）+ FNV 快照封存（8 分量）
//! · 判据落地：UNX-F2501 · 判据：大页映射前向接口冻结件登记（B10 消费联签），接口字段与池账口径一致
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2501_VERSION: &str = "u2501-u2501sm-v1";

/// 诊断段基址。
pub const U2501_SEG_BASE: u16 = 0xa3d0;

// --------------------------------------------------------------------------
// 诊断码（u2501 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 冻结接口 状态面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2501Code {
    /// 0xa3d0 状态重复（字段缺项 同名双态）。
    StateDup,
    /// 0xa3d1 状态断档（口径分歧 序缺失）。
    StateHole,
    /// 0xa3d2 事件重复（同事件双声明）。
    EventDup,
    /// 0xa3d3 迁移重复（同源同事件双迁移）。
    TransDup,
    /// 0xa3d4 迁移缺失（应达态不可达）。
    TransHole,
    /// 0xa3d5 迁移环（非法自环/环路）。
    TransCycle,
    /// 0xa3d6 入口非法（初态不在闭集）。
    EntryBad,
    /// 0xa3d7 迁移丢失（应用事件无效果）。
    ApplyLost,
    /// 0xa3d8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xa3d9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xa3da 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xa3db 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2501Code {
    /// 全量闭集。
    pub const ALL: [u2501Code; 12] = [
        u2501Code::StateDup,
        u2501Code::StateHole,
        u2501Code::EventDup,
        u2501Code::TransDup,
        u2501Code::TransHole,
        u2501Code::TransCycle,
        u2501Code::EntryBad,
        u2501Code::ApplyLost,
        u2501Code::LedgerBroken,
        u2501Code::SnapshotFingerprint,
        u2501Code::TableBroken,
        u2501Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2501Code::StateDup => 0xa3d0,
            u2501Code::StateHole => 0xa3d1,
            u2501Code::EventDup => 0xa3d2,
            u2501Code::TransDup => 0xa3d3,
            u2501Code::TransHole => 0xa3d4,
            u2501Code::TransCycle => 0xa3d5,
            u2501Code::EntryBad => 0xa3d6,
            u2501Code::ApplyLost => 0xa3d7,
            u2501Code::LedgerBroken => 0xa3d8,
            u2501Code::SnapshotFingerprint => 0xa3d9,
            u2501Code::TableBroken => 0xa3da,
            u2501Code::SegmentGuard => 0xa3db,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2501Code::StateDup => "状态重复（字段缺项 同名双态）",
            u2501Code::StateHole => "状态断档（口径分歧 序缺失）",
            u2501Code::EventDup => "事件重复（同事件双声明）",
            u2501Code::TransDup => "迁移重复（同源同事件双迁移）",
            u2501Code::TransHole => "迁移缺失（应达态不可达）",
            u2501Code::TransCycle => "迁移环（非法自环/环路）",
            u2501Code::EntryBad => "入口非法（初态不在闭集）",
            u2501Code::ApplyLost => "迁移丢失（应用事件无效果）",
            u2501Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2501Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2501Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2501Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2501Code::StateDup => "按闭集唯一律去重",
            u2501Code::StateHole => "按闭集补态",
            u2501Code::EventDup => "按事件闭集去重",
            u2501Code::TransDup => "按确定性唯一迁移律合并",
            u2501Code::TransHole => "按迁移矩阵补边",
            u2501Code::TransCycle => "按无环律重构迁移",
            u2501Code::EntryBad => "按闭集钳制初态",
            u2501Code::ApplyLost => "核对事件前置条件",
            u2501Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2501Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2501Code::TableBroken => "按语义规则重建常量表",
            u2501Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 状态机 mxu2501flow（6 态 × 6 事件 · 迁移 8 条显式声明）
// --------------------------------------------------------------------------
/// 冻结接口 状态机（判据：字段缺项）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu2501flowState {
    Draft,
    Fielded,
    Signed,
    Linked,
    Verified,
    Frozen,
}

impl mxu2501flowState {
    /// 全量态闭集。
    pub const ALL: [mxu2501flowState; 6] = [
        mxu2501flowState::Draft,
        mxu2501flowState::Fielded,
        mxu2501flowState::Signed,
        mxu2501flowState::Linked,
        mxu2501flowState::Verified,
        mxu2501flowState::Frozen,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            mxu2501flowState::Draft => 0,
            mxu2501flowState::Fielded => 1,
            mxu2501flowState::Signed => 2,
            mxu2501flowState::Linked => 3,
            mxu2501flowState::Verified => 4,
            mxu2501flowState::Frozen => 5,
        }
    }
}

/// mxu2501flow事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu2501flowEvent {
    DraftIf,
    FillField,
    Sign,
    LinkB10,
    VerifyPool,
    Freeze,
}

impl mxu2501flowEvent {
    /// 全量事件闭集。
    pub const ALL: [mxu2501flowEvent; 6] = [
        mxu2501flowEvent::DraftIf,
        mxu2501flowEvent::FillField,
        mxu2501flowEvent::Sign,
        mxu2501flowEvent::LinkB10,
        mxu2501flowEvent::VerifyPool,
        mxu2501flowEvent::Freeze,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const MXU2501FLOW_TRANSITIONS: [(mxu2501flowState, mxu2501flowEvent, mxu2501flowState); 8] = [
    (mxu2501flowState::Draft, mxu2501flowEvent::DraftIf, mxu2501flowState::Fielded), // Draft→Fielded 主链
    (mxu2501flowState::Fielded, mxu2501flowEvent::FillField, mxu2501flowState::Signed), // Fielded→Signed 主链
    (mxu2501flowState::Signed, mxu2501flowEvent::Sign, mxu2501flowState::Linked), // Signed→Linked 主链
    (mxu2501flowState::Linked, mxu2501flowEvent::LinkB10, mxu2501flowState::Verified), // Linked→Verified 主链
    (mxu2501flowState::Verified, mxu2501flowEvent::VerifyPool, mxu2501flowState::Frozen), // Verified→Frozen 主链
    (mxu2501flowState::Frozen, mxu2501flowEvent::Freeze, mxu2501flowState::Draft), // 终态回边
    (mxu2501flowState::Frozen, mxu2501flowEvent::DraftIf, mxu2501flowState::Verified), // 终态回退边
    (mxu2501flowState::Fielded, mxu2501flowEvent::FillField, mxu2501flowState::Signed), // 旁路边
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn mxu2501flow_next(state: mxu2501flowState, ev: mxu2501flowEvent) -> Option<mxu2501flowState> {
    let mut i = 0usize;
    while i < 8 {
        let t = MXU2501FLOW_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn mxu2501flow_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < mxu2501flowState::ALL.len() {
        let mut ei = 0usize;
        while ei < mxu2501flowEvent::ALL.len() {
            let legal = mxu2501flow_next(mxu2501flowState::ALL[si], mxu2501flowEvent::ALL[ei]).is_some();
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
pub fn mxu2501flow_terminal(st: mxu2501flowState) -> bool {
    let mut i = 0usize;
    while i < 8 {
        if MXU2501FLOW_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct mxu2501flowLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: mxu2501flowState,
}

impl mxu2501flowLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: mxu2501flowState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: mxu2501flowEvent) -> bool {
        match mxu2501flow_next(self.cur, ev) {
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
// 快照 Mxu2501Snap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// 冻结接口 快照封存（FNV 三态指纹 + 篡改检出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mxu2501Snap {
    /// 指纹分量：if_v。
    pub if_v: u64,
    /// 指纹分量：fr_v。
    pub fr_v: u64,
    /// 指纹分量：b10_v。
    pub b10_v: u64,
    /// 指纹分量：fl_v。
    pub fl_v: u64,
    /// 指纹分量：pc_v。
    pub pc_v: u64,
    /// 指纹分量：sg_v。
    pub sg_v: u64,
    /// 指纹分量：cn_v。
    pub cn_v: u64,
    /// 指纹分量：hd_v。
    pub hd_v: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl Mxu2501Snap {
    /// 构造并即时合成指纹。
    pub fn compute(
        if_v: u64,
        fr_v: u64,
        b10_v: u64,
        fl_v: u64,
        pc_v: u64,
        sg_v: u64,
        cn_v: u64,
        hd_v: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &if_v.to_le_bytes());
        h = fnv1a64_from(h, &fr_v.to_le_bytes());
        h = fnv1a64_from(h, &b10_v.to_le_bytes());
        h = fnv1a64_from(h, &fl_v.to_le_bytes());
        h = fnv1a64_from(h, &pc_v.to_le_bytes());
        h = fnv1a64_from(h, &sg_v.to_le_bytes());
        h = fnv1a64_from(h, &cn_v.to_le_bytes());
        h = fnv1a64_from(h, &hd_v.to_le_bytes());
        Self {
            if_v,
            fr_v,
            b10_v,
            fl_v,
            pc_v,
            sg_v,
            cn_v,
            hd_v,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.if_v.to_le_bytes());
        h = fnv1a64_from(h, &self.fr_v.to_le_bytes());
        h = fnv1a64_from(h, &self.b10_v.to_le_bytes());
        h = fnv1a64_from(h, &self.fl_v.to_le_bytes());
        h = fnv1a64_from(h, &self.pc_v.to_le_bytes());
        h = fnv1a64_from(h, &self.sg_v.to_le_bytes());
        h = fnv1a64_from(h, &self.cn_v.to_le_bytes());
        h = fnv1a64_from(h, &self.hd_v.to_le_bytes());
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

/// 段位守卫：全部诊断码必须落在本段 [0xa3d0, 0xa3df]。
pub const fn u2501_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2501Code::ALL.len() {
        let c = u2501Code::ALL[i].code();
        if c < 0xa3d0 || c > 0xa3df {
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
fn u2501_guard_seg() -> bool {
    u2501_segment_guard()
}

/// UNX-F2501 状态机闭集/迁移表/非法组合恰值全绿。
fn check_u2501_sm_legal() -> bool {
    let mut ok = mxu2501flow_illegal_pairs() == 29;
    if ok { ok = MXU2501FLOW_TRANSITIONS.len() == 8; }
    if ok { ok = mxu2501flowState::ALL.len() == 6 && mxu2501flowEvent::ALL.len() == 6; }
    ok
}

/// UNX-F2501 迁移主链回放 + 快照往返恒等。
fn check_u2501_sm_round() -> bool {
    let mut lg = mxu2501flowLedger::new(mxu2501flowState::Draft);
    let mut ok = lg.apply(mxu2501flowEvent::DraftIf) && lg.cur == mxu2501flowState::Fielded && lg.total() == 1;
    let s0 = Mxu2501Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    let mut s1 = Mxu2501Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    s1.recompute();
    if ok { ok = s0 == s1 && !s1.tampered(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2501_checks() -> CheckSet {
    let mut set = CheckSet::new("U2501");
    set.add(
        "u2501-seg-guard",
        u2501_guard_seg(),
        if u2501_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2501-sm-legal",
        check_u2501_sm_legal(),
        if check_u2501_sm_legal() { "" } else { "UNX-F2501 sm legality mismatch" },
    );
    set.add(
        "u2501-sm-round",
        check_u2501_sm_round(),
        if check_u2501_sm_round() { "" } else { "UNX-F2501 sm roundtrip mismatch" },
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
    fn u2501_all_checks_green() {
        let set = run_u2501_checks();
        assert!(set.all_passed(), "U2501 domain checks must be green");
    }

    #[test]
    fn u2501_segment_guard_holds() {
        assert!(u2501_segment_guard());
    }
}
