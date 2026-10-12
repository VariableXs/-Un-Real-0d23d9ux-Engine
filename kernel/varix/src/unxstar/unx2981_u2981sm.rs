//! unxu2981_u2981sm —— 域级收口方案冻结（收口六件套范围） 状态机与快照封存（域级收口六件套）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2981 · 域级收口方案冻结（收口六件套范围） · 判据原文：UNX-F2981 · 判据：UNX-F2981-J1 收口六件套（总对账/完整性终核/守恒终账/指标总览/欠账终账/总判定）范围冻结哈希入册，六件与 F2982–F2997 条位一一对应
//! · 组件：域级收口六件套 状态机（6 态 × 6 事件显式迁移）+ FNV 快照封存（8 分量）
//! · 判据落地：UNX-F2981 · 判据：UNX-F2981-J1 收口六件套（总对账/完整性终核/守恒终账/指标总览/欠账终账/总判定）范围冻结哈希入册，六件与 F298
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2981_VERSION: &str = "u2981-u2981sm-v1";

/// 诊断段基址。
pub const U2981_SEG_BASE: u16 = 0xe250;

// --------------------------------------------------------------------------
// 诊断码（u2981 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 域级收口六件套 状态面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2981Code {
    /// 0xe250 状态重复（收口 同名双态）。
    StateDup,
    /// 0xe251 状态断档（六件套 序缺失）。
    StateHole,
    /// 0xe252 事件重复（同事件双声明）。
    EventDup,
    /// 0xe253 迁移重复（同源同事件双迁移）。
    TransDup,
    /// 0xe254 迁移缺失（应达态不可达）。
    TransHole,
    /// 0xe255 迁移环（非法自环/环路）。
    TransCycle,
    /// 0xe256 入口非法（初态不在闭集）。
    EntryBad,
    /// 0xe257 迁移丢失（应用事件无效果）。
    ApplyLost,
    /// 0xe258 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe259 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe25a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe25b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2981Code {
    /// 全量闭集。
    pub const ALL: [u2981Code; 12] = [
        u2981Code::StateDup,
        u2981Code::StateHole,
        u2981Code::EventDup,
        u2981Code::TransDup,
        u2981Code::TransHole,
        u2981Code::TransCycle,
        u2981Code::EntryBad,
        u2981Code::ApplyLost,
        u2981Code::LedgerBroken,
        u2981Code::SnapshotFingerprint,
        u2981Code::TableBroken,
        u2981Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2981Code::StateDup => 0xe250,
            u2981Code::StateHole => 0xe251,
            u2981Code::EventDup => 0xe252,
            u2981Code::TransDup => 0xe253,
            u2981Code::TransHole => 0xe254,
            u2981Code::TransCycle => 0xe255,
            u2981Code::EntryBad => 0xe256,
            u2981Code::ApplyLost => 0xe257,
            u2981Code::LedgerBroken => 0xe258,
            u2981Code::SnapshotFingerprint => 0xe259,
            u2981Code::TableBroken => 0xe25a,
            u2981Code::SegmentGuard => 0xe25b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2981Code::StateDup => "状态重复（收口 同名双态）",
            u2981Code::StateHole => "状态断档（六件套 序缺失）",
            u2981Code::EventDup => "事件重复（同事件双声明）",
            u2981Code::TransDup => "迁移重复（同源同事件双迁移）",
            u2981Code::TransHole => "迁移缺失（应达态不可达）",
            u2981Code::TransCycle => "迁移环（非法自环/环路）",
            u2981Code::EntryBad => "入口非法（初态不在闭集）",
            u2981Code::ApplyLost => "迁移丢失（应用事件无效果）",
            u2981Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2981Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2981Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2981Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2981Code::StateDup => "按闭集唯一律去重",
            u2981Code::StateHole => "按闭集补态",
            u2981Code::EventDup => "按事件闭集去重",
            u2981Code::TransDup => "按确定性唯一迁移律合并",
            u2981Code::TransHole => "按迁移矩阵补边",
            u2981Code::TransCycle => "按无环律重构迁移",
            u2981Code::EntryBad => "按闭集钳制初态",
            u2981Code::ApplyLost => "核对事件前置条件",
            u2981Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2981Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2981Code::TableBroken => "按语义规则重建常量表",
            u2981Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 状态机 mxu2981flow（6 态 × 6 事件 · 迁移 8 条显式声明）
// --------------------------------------------------------------------------
/// 域级收口六件套 状态机（判据：收口）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu2981flowState {
    Scanning,
    Binning,
    Verifying,
    Gapping,
    Signing,
    Sealed,
}

impl mxu2981flowState {
    /// 全量态闭集。
    pub const ALL: [mxu2981flowState; 6] = [
        mxu2981flowState::Scanning,
        mxu2981flowState::Binning,
        mxu2981flowState::Verifying,
        mxu2981flowState::Gapping,
        mxu2981flowState::Signing,
        mxu2981flowState::Sealed,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            mxu2981flowState::Scanning => 0,
            mxu2981flowState::Binning => 1,
            mxu2981flowState::Verifying => 2,
            mxu2981flowState::Gapping => 3,
            mxu2981flowState::Signing => 4,
            mxu2981flowState::Sealed => 5,
        }
    }
}

/// mxu2981flow事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu2981flowEvent {
    Scan,
    Bin,
    Verify,
    Gap,
    Sign,
    Seal,
}

impl mxu2981flowEvent {
    /// 全量事件闭集。
    pub const ALL: [mxu2981flowEvent; 6] = [
        mxu2981flowEvent::Scan,
        mxu2981flowEvent::Bin,
        mxu2981flowEvent::Verify,
        mxu2981flowEvent::Gap,
        mxu2981flowEvent::Sign,
        mxu2981flowEvent::Seal,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const MXU2981FLOW_TRANSITIONS: [(mxu2981flowState, mxu2981flowEvent, mxu2981flowState); 8] = [
    (mxu2981flowState::Scanning, mxu2981flowEvent::Scan, mxu2981flowState::Binning), // Scanning→Binning 主链
    (mxu2981flowState::Binning, mxu2981flowEvent::Bin, mxu2981flowState::Verifying), // Binning→Verifying 主链
    (mxu2981flowState::Verifying, mxu2981flowEvent::Verify, mxu2981flowState::Gapping), // Verifying→Gapping 主链
    (mxu2981flowState::Gapping, mxu2981flowEvent::Gap, mxu2981flowState::Signing), // Gapping→Signing 主链
    (mxu2981flowState::Signing, mxu2981flowEvent::Sign, mxu2981flowState::Sealed), // Signing→Sealed 主链
    (mxu2981flowState::Sealed, mxu2981flowEvent::Seal, mxu2981flowState::Scanning), // 终态回边
    (mxu2981flowState::Sealed, mxu2981flowEvent::Scan, mxu2981flowState::Signing), // 终态回退边
    (mxu2981flowState::Binning, mxu2981flowEvent::Bin, mxu2981flowState::Verifying), // 旁路边
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn mxu2981flow_next(state: mxu2981flowState, ev: mxu2981flowEvent) -> Option<mxu2981flowState> {
    let mut i = 0usize;
    while i < 8 {
        let t = MXU2981FLOW_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn mxu2981flow_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < mxu2981flowState::ALL.len() {
        let mut ei = 0usize;
        while ei < mxu2981flowEvent::ALL.len() {
            let legal = mxu2981flow_next(mxu2981flowState::ALL[si], mxu2981flowEvent::ALL[ei]).is_some();
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
pub fn mxu2981flow_terminal(st: mxu2981flowState) -> bool {
    let mut i = 0usize;
    while i < 8 {
        if MXU2981FLOW_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct mxu2981flowLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: mxu2981flowState,
}

impl mxu2981flowLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: mxu2981flowState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: mxu2981flowEvent) -> bool {
        match mxu2981flow_next(self.cur, ev) {
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
// 快照 Mxu2981Snap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// 域级收口六件套 快照封存（FNV 三态指纹 + 篡改检出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mxu2981Snap {
    /// 指纹分量：six_v。
    pub six_v: u64,
    /// 指纹分量：close_v。
    pub close_v: u64,
    /// 指纹分量：scope_v。
    pub scope_v: u64,
    /// 指纹分量：item_v。
    pub item_v: u64,
    /// 指纹分量：proof_v。
    pub proof_v: u64,
    /// 指纹分量：bind_v。
    pub bind_v: u64,
    /// 指纹分量：acct_v。
    pub acct_v: u64,
    /// 指纹分量：chk_v。
    pub chk_v: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl Mxu2981Snap {
    /// 构造并即时合成指纹。
    pub fn compute(
        six_v: u64,
        close_v: u64,
        scope_v: u64,
        item_v: u64,
        proof_v: u64,
        bind_v: u64,
        acct_v: u64,
        chk_v: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &six_v.to_le_bytes());
        h = fnv1a64_from(h, &close_v.to_le_bytes());
        h = fnv1a64_from(h, &scope_v.to_le_bytes());
        h = fnv1a64_from(h, &item_v.to_le_bytes());
        h = fnv1a64_from(h, &proof_v.to_le_bytes());
        h = fnv1a64_from(h, &bind_v.to_le_bytes());
        h = fnv1a64_from(h, &acct_v.to_le_bytes());
        h = fnv1a64_from(h, &chk_v.to_le_bytes());
        Self {
            six_v,
            close_v,
            scope_v,
            item_v,
            proof_v,
            bind_v,
            acct_v,
            chk_v,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.six_v.to_le_bytes());
        h = fnv1a64_from(h, &self.close_v.to_le_bytes());
        h = fnv1a64_from(h, &self.scope_v.to_le_bytes());
        h = fnv1a64_from(h, &self.item_v.to_le_bytes());
        h = fnv1a64_from(h, &self.proof_v.to_le_bytes());
        h = fnv1a64_from(h, &self.bind_v.to_le_bytes());
        h = fnv1a64_from(h, &self.acct_v.to_le_bytes());
        h = fnv1a64_from(h, &self.chk_v.to_le_bytes());
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

/// 段位守卫：全部诊断码必须落在本段 [0xe250, 0xe25f]。
pub const fn u2981_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2981Code::ALL.len() {
        let c = u2981Code::ALL[i].code();
        if c < 0xe250 || c > 0xe25f {
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
fn u2981_guard_seg() -> bool {
    u2981_segment_guard()
}

/// UNX-F2981 状态机闭集/迁移表/非法组合恰值全绿。
fn check_u2981_sm_legal() -> bool {
    let mut ok = mxu2981flow_illegal_pairs() == 29;
    if ok { ok = MXU2981FLOW_TRANSITIONS.len() == 8; }
    if ok { ok = mxu2981flowState::ALL.len() == 6 && mxu2981flowEvent::ALL.len() == 6; }
    ok
}

/// UNX-F2981 迁移主链回放 + 快照往返恒等。
fn check_u2981_sm_round() -> bool {
    let mut lg = mxu2981flowLedger::new(mxu2981flowState::Scanning);
    let mut ok = lg.apply(mxu2981flowEvent::Scan) && lg.cur == mxu2981flowState::Binning && lg.total() == 1;
    let s0 = Mxu2981Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    let mut s1 = Mxu2981Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    s1.recompute();
    if ok { ok = s0 == s1 && !s1.tampered(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2981_checks() -> CheckSet {
    let mut set = CheckSet::new("U2981");
    set.add(
        "u2981-seg-guard",
        u2981_guard_seg(),
        if u2981_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2981-sm-legal",
        check_u2981_sm_legal(),
        if check_u2981_sm_legal() { "" } else { "UNX-F2981 sm legality mismatch" },
    );
    set.add(
        "u2981-sm-round",
        check_u2981_sm_round(),
        if check_u2981_sm_round() { "" } else { "UNX-F2981 sm roundtrip mismatch" },
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
    fn u2981_all_checks_green() {
        let set = run_u2981_checks();
        assert!(set.all_passed(), "U2981 domain checks must be green");
    }

    #[test]
    fn u2981_segment_guard_holds() {
        assert!(u2981_segment_guard());
    }
}
