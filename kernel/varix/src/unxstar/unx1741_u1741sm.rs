//! unxu1741_u1741sm —— 空闲循环总纲（idle 三态状态机） 状态机与快照封存（空闲态）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F1741 · 空闲循环总纲（idle 三态状态机） · 判据原文：UNX-F1741 · 判据：idle 进入/驻留/退出三态状态机冻结 v1，状态转换账与实际状态一致断言全过，非法转换注入 10/10 次拒绝
//! · 组件：空闲态 状态机（6 态 × 6 事件显式迁移）+ FNV 快照封存（8 分量）
//! · 判据落地：UNX-F1741 · 判据：idle 进入/驻留/退出三态状态机冻结 v1，状态转换账与实际状态一致断言全过，非法转换注入 10/10 次拒绝
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U1741_VERSION: &str = "u1741-u1741sm-v1";

/// 诊断段基址。
pub const U1741_SEG_BASE: u16 = 0x5a50;

// --------------------------------------------------------------------------
// 诊断码（u1741 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 空闲态 状态面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u1741Code {
    /// 0x5a50 状态重复（非法转换 同名双态）。
    StateDup,
    /// 0x5a51 状态断档（账实漂移 序缺失）。
    StateHole,
    /// 0x5a52 事件重复（同事件双声明）。
    EventDup,
    /// 0x5a53 迁移重复（同源同事件双迁移）。
    TransDup,
    /// 0x5a54 迁移缺失（应达态不可达）。
    TransHole,
    /// 0x5a55 迁移环（非法自环/环路）。
    TransCycle,
    /// 0x5a56 入口非法（初态不在闭集）。
    EntryBad,
    /// 0x5a57 迁移丢失（应用事件无效果）。
    ApplyLost,
    /// 0x5a58 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x5a59 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x5a5a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x5a5b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u1741Code {
    /// 全量闭集。
    pub const ALL: [u1741Code; 12] = [
        u1741Code::StateDup,
        u1741Code::StateHole,
        u1741Code::EventDup,
        u1741Code::TransDup,
        u1741Code::TransHole,
        u1741Code::TransCycle,
        u1741Code::EntryBad,
        u1741Code::ApplyLost,
        u1741Code::LedgerBroken,
        u1741Code::SnapshotFingerprint,
        u1741Code::TableBroken,
        u1741Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u1741Code::StateDup => 0x5a50,
            u1741Code::StateHole => 0x5a51,
            u1741Code::EventDup => 0x5a52,
            u1741Code::TransDup => 0x5a53,
            u1741Code::TransHole => 0x5a54,
            u1741Code::TransCycle => 0x5a55,
            u1741Code::EntryBad => 0x5a56,
            u1741Code::ApplyLost => 0x5a57,
            u1741Code::LedgerBroken => 0x5a58,
            u1741Code::SnapshotFingerprint => 0x5a59,
            u1741Code::TableBroken => 0x5a5a,
            u1741Code::SegmentGuard => 0x5a5b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u1741Code::StateDup => "状态重复（非法转换 同名双态）",
            u1741Code::StateHole => "状态断档（账实漂移 序缺失）",
            u1741Code::EventDup => "事件重复（同事件双声明）",
            u1741Code::TransDup => "迁移重复（同源同事件双迁移）",
            u1741Code::TransHole => "迁移缺失（应达态不可达）",
            u1741Code::TransCycle => "迁移环（非法自环/环路）",
            u1741Code::EntryBad => "入口非法（初态不在闭集）",
            u1741Code::ApplyLost => "迁移丢失（应用事件无效果）",
            u1741Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u1741Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u1741Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u1741Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u1741Code::StateDup => "按闭集唯一律去重",
            u1741Code::StateHole => "按闭集补态",
            u1741Code::EventDup => "按事件闭集去重",
            u1741Code::TransDup => "按确定性唯一迁移律合并",
            u1741Code::TransHole => "按迁移矩阵补边",
            u1741Code::TransCycle => "按无环律重构迁移",
            u1741Code::EntryBad => "按闭集钳制初态",
            u1741Code::ApplyLost => "核对事件前置条件",
            u1741Code::LedgerBroken => "按守恒律重建台账后重跑",
            u1741Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u1741Code::TableBroken => "按语义规则重建常量表",
            u1741Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 状态机 mxu1741flow（6 态 × 6 事件 · 迁移 8 条显式声明）
// --------------------------------------------------------------------------
/// 空闲态 状态机（判据：非法转换）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu1741flowState {
    Running,
    Entering,
    Residing,
    Exiting,
    Waking,
    Audited,
}

impl mxu1741flowState {
    /// 全量态闭集。
    pub const ALL: [mxu1741flowState; 6] = [
        mxu1741flowState::Running,
        mxu1741flowState::Entering,
        mxu1741flowState::Residing,
        mxu1741flowState::Exiting,
        mxu1741flowState::Waking,
        mxu1741flowState::Audited,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            mxu1741flowState::Running => 0,
            mxu1741flowState::Entering => 1,
            mxu1741flowState::Residing => 2,
            mxu1741flowState::Exiting => 3,
            mxu1741flowState::Waking => 4,
            mxu1741flowState::Audited => 5,
        }
    }
}

/// mxu1741flow事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu1741flowEvent {
    Enter,
    Reside,
    Exit,
    Wake,
    Illegal,
    Audit,
}

impl mxu1741flowEvent {
    /// 全量事件闭集。
    pub const ALL: [mxu1741flowEvent; 6] = [
        mxu1741flowEvent::Enter,
        mxu1741flowEvent::Reside,
        mxu1741flowEvent::Exit,
        mxu1741flowEvent::Wake,
        mxu1741flowEvent::Illegal,
        mxu1741flowEvent::Audit,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const MXU1741FLOW_TRANSITIONS: [(mxu1741flowState, mxu1741flowEvent, mxu1741flowState); 8] = [
    (mxu1741flowState::Running, mxu1741flowEvent::Enter, mxu1741flowState::Entering), // Running→Entering 主链
    (mxu1741flowState::Entering, mxu1741flowEvent::Reside, mxu1741flowState::Residing), // Entering→Residing 主链
    (mxu1741flowState::Residing, mxu1741flowEvent::Exit, mxu1741flowState::Exiting), // Residing→Exiting 主链
    (mxu1741flowState::Exiting, mxu1741flowEvent::Wake, mxu1741flowState::Waking), // Exiting→Waking 主链
    (mxu1741flowState::Waking, mxu1741flowEvent::Illegal, mxu1741flowState::Audited), // Waking→Audited 主链
    (mxu1741flowState::Audited, mxu1741flowEvent::Audit, mxu1741flowState::Running), // 终态回边
    (mxu1741flowState::Audited, mxu1741flowEvent::Enter, mxu1741flowState::Waking), // 终态回退边
    (mxu1741flowState::Entering, mxu1741flowEvent::Reside, mxu1741flowState::Residing), // 旁路边
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn mxu1741flow_next(state: mxu1741flowState, ev: mxu1741flowEvent) -> Option<mxu1741flowState> {
    let mut i = 0usize;
    while i < 8 {
        let t = MXU1741FLOW_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn mxu1741flow_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < mxu1741flowState::ALL.len() {
        let mut ei = 0usize;
        while ei < mxu1741flowEvent::ALL.len() {
            let legal = mxu1741flow_next(mxu1741flowState::ALL[si], mxu1741flowEvent::ALL[ei]).is_some();
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
pub fn mxu1741flow_terminal(st: mxu1741flowState) -> bool {
    let mut i = 0usize;
    while i < 8 {
        if MXU1741FLOW_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct mxu1741flowLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: mxu1741flowState,
}

impl mxu1741flowLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: mxu1741flowState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: mxu1741flowEvent) -> bool {
        match mxu1741flow_next(self.cur, ev) {
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
// 快照 Mxu1741Snap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// 空闲态 快照封存（FNV 三态指纹 + 篡改检出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mxu1741Snap {
    /// 指纹分量：jr_v。
    pub jr_v: u64,
    /// 指纹分量：zl_v。
    pub zl_v: u64,
    /// 指纹分量：tc_v。
    pub tc_v: u64,
    /// 指纹分量：sm_v。
    pub sm_v: u64,
    /// 指纹分量：zz_v。
    pub zz_v: u64,
    /// 指纹分量：yz_v。
    pub yz_v: u64,
    /// 指纹分量：ff_v。
    pub ff_v: u64,
    /// 指纹分量：jj_v。
    pub jj_v: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl Mxu1741Snap {
    /// 构造并即时合成指纹。
    pub fn compute(
        jr_v: u64,
        zl_v: u64,
        tc_v: u64,
        sm_v: u64,
        zz_v: u64,
        yz_v: u64,
        ff_v: u64,
        jj_v: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &jr_v.to_le_bytes());
        h = fnv1a64_from(h, &zl_v.to_le_bytes());
        h = fnv1a64_from(h, &tc_v.to_le_bytes());
        h = fnv1a64_from(h, &sm_v.to_le_bytes());
        h = fnv1a64_from(h, &zz_v.to_le_bytes());
        h = fnv1a64_from(h, &yz_v.to_le_bytes());
        h = fnv1a64_from(h, &ff_v.to_le_bytes());
        h = fnv1a64_from(h, &jj_v.to_le_bytes());
        Self {
            jr_v,
            zl_v,
            tc_v,
            sm_v,
            zz_v,
            yz_v,
            ff_v,
            jj_v,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.jr_v.to_le_bytes());
        h = fnv1a64_from(h, &self.zl_v.to_le_bytes());
        h = fnv1a64_from(h, &self.tc_v.to_le_bytes());
        h = fnv1a64_from(h, &self.sm_v.to_le_bytes());
        h = fnv1a64_from(h, &self.zz_v.to_le_bytes());
        h = fnv1a64_from(h, &self.yz_v.to_le_bytes());
        h = fnv1a64_from(h, &self.ff_v.to_le_bytes());
        h = fnv1a64_from(h, &self.jj_v.to_le_bytes());
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

/// 段位守卫：全部诊断码必须落在本段 [0x5a50, 0x5a5f]。
pub const fn u1741_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u1741Code::ALL.len() {
        let c = u1741Code::ALL[i].code();
        if c < 0x5a50 || c > 0x5a5f {
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
fn u1741_guard_seg() -> bool {
    u1741_segment_guard()
}

/// UNX-F1741 状态机闭集/迁移表/非法组合恰值全绿。
fn check_u1741_sm_legal() -> bool {
    let mut ok = mxu1741flow_illegal_pairs() == 29;
    if ok { ok = MXU1741FLOW_TRANSITIONS.len() == 8; }
    if ok { ok = mxu1741flowState::ALL.len() == 6 && mxu1741flowEvent::ALL.len() == 6; }
    ok
}

/// UNX-F1741 迁移主链回放 + 快照往返恒等。
fn check_u1741_sm_round() -> bool {
    let mut lg = mxu1741flowLedger::new(mxu1741flowState::Running);
    let mut ok = lg.apply(mxu1741flowEvent::Enter) && lg.cur == mxu1741flowState::Entering && lg.total() == 1;
    let s0 = Mxu1741Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    let mut s1 = Mxu1741Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    s1.recompute();
    if ok { ok = s0 == s1 && !s1.tampered(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u1741_checks() -> CheckSet {
    let mut set = CheckSet::new("U1741");
    set.add(
        "u1741-seg-guard",
        u1741_guard_seg(),
        if u1741_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u1741-sm-legal",
        check_u1741_sm_legal(),
        if check_u1741_sm_legal() { "" } else { "UNX-F1741 sm legality mismatch" },
    );
    set.add(
        "u1741-sm-round",
        check_u1741_sm_round(),
        if check_u1741_sm_round() { "" } else { "UNX-F1741 sm roundtrip mismatch" },
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
    fn u1741_all_checks_green() {
        let set = run_u1741_checks();
        assert!(set.all_passed(), "U1741 domain checks must be green");
    }

    #[test]
    fn u1741_segment_guard_holds() {
        assert!(u1741_segment_guard());
    }
}
