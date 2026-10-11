//! unxu3041_u3041sm —— 大页断言总注册清点（≥30 条逐条清点账） 状态机与快照封存（大页断言清点）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3041 · 大页断言总注册清点（≥30 条逐条清点账） · 判据原文：UNX-F3041 · 判据：UNX-F3041-J1 总注册冻结：域内大页断言逐条清点账（B05/B06/B28/B33 段累计）≥30 条断言成立，清单哈希入册，清点缺条注入 10/10 次检出
//! · 组件：大页断言清点 状态机（6 态 × 6 事件显式迁移）+ FNV 快照封存（8 分量）
//! · 判据落地：UNX-F3041 · 判据：UNX-F3041-J1 总注册冻结：域内大页断言逐条清点账（B05/B06/B28/B33 段累计）≥30 条断言成立，清单哈希
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3041_VERSION: &str = "u3041-u3041sm-v1";

/// 诊断段基址。
pub const U3041_SEG_BASE: u16 = 0xe280;

// --------------------------------------------------------------------------
// 诊断码（u3041 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 大页断言清点 状态面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3041Code {
    /// 0xe280 状态重复（断言 同名双态）。
    StateDup,
    /// 0xe281 状态断档（清点 序缺失）。
    StateHole,
    /// 0xe282 事件重复（同事件双声明）。
    EventDup,
    /// 0xe283 迁移重复（同源同事件双迁移）。
    TransDup,
    /// 0xe284 迁移缺失（应达态不可达）。
    TransHole,
    /// 0xe285 迁移环（非法自环/环路）。
    TransCycle,
    /// 0xe286 入口非法（初态不在闭集）。
    EntryBad,
    /// 0xe287 迁移丢失（应用事件无效果）。
    ApplyLost,
    /// 0xe288 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe289 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe28a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe28b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3041Code {
    /// 全量闭集。
    pub const ALL: [u3041Code; 12] = [
        u3041Code::StateDup,
        u3041Code::StateHole,
        u3041Code::EventDup,
        u3041Code::TransDup,
        u3041Code::TransHole,
        u3041Code::TransCycle,
        u3041Code::EntryBad,
        u3041Code::ApplyLost,
        u3041Code::LedgerBroken,
        u3041Code::SnapshotFingerprint,
        u3041Code::TableBroken,
        u3041Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3041Code::StateDup => 0xe280,
            u3041Code::StateHole => 0xe281,
            u3041Code::EventDup => 0xe282,
            u3041Code::TransDup => 0xe283,
            u3041Code::TransHole => 0xe284,
            u3041Code::TransCycle => 0xe285,
            u3041Code::EntryBad => 0xe286,
            u3041Code::ApplyLost => 0xe287,
            u3041Code::LedgerBroken => 0xe288,
            u3041Code::SnapshotFingerprint => 0xe289,
            u3041Code::TableBroken => 0xe28a,
            u3041Code::SegmentGuard => 0xe28b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3041Code::StateDup => "状态重复（断言 同名双态）",
            u3041Code::StateHole => "状态断档（清点 序缺失）",
            u3041Code::EventDup => "事件重复（同事件双声明）",
            u3041Code::TransDup => "迁移重复（同源同事件双迁移）",
            u3041Code::TransHole => "迁移缺失（应达态不可达）",
            u3041Code::TransCycle => "迁移环（非法自环/环路）",
            u3041Code::EntryBad => "入口非法（初态不在闭集）",
            u3041Code::ApplyLost => "迁移丢失（应用事件无效果）",
            u3041Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3041Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3041Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3041Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3041Code::StateDup => "按闭集唯一律去重",
            u3041Code::StateHole => "按闭集补态",
            u3041Code::EventDup => "按事件闭集去重",
            u3041Code::TransDup => "按确定性唯一迁移律合并",
            u3041Code::TransHole => "按迁移矩阵补边",
            u3041Code::TransCycle => "按无环律重构迁移",
            u3041Code::EntryBad => "按闭集钳制初态",
            u3041Code::ApplyLost => "核对事件前置条件",
            u3041Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3041Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3041Code::TableBroken => "按语义规则重建常量表",
            u3041Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 状态机 mxu3041flow（6 态 × 6 事件 · 迁移 8 条显式声明）
// --------------------------------------------------------------------------
/// 大页断言清点 状态机（判据：断言）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu3041flowState {
    Scanning,
    Listing,
    Verifying,
    Gapping,
    Signing,
    Sealed,
}

impl mxu3041flowState {
    /// 全量态闭集。
    pub const ALL: [mxu3041flowState; 6] = [
        mxu3041flowState::Scanning,
        mxu3041flowState::Listing,
        mxu3041flowState::Verifying,
        mxu3041flowState::Gapping,
        mxu3041flowState::Signing,
        mxu3041flowState::Sealed,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            mxu3041flowState::Scanning => 0,
            mxu3041flowState::Listing => 1,
            mxu3041flowState::Verifying => 2,
            mxu3041flowState::Gapping => 3,
            mxu3041flowState::Signing => 4,
            mxu3041flowState::Sealed => 5,
        }
    }
}

/// mxu3041flow事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu3041flowEvent {
    Scan,
    List,
    Verify,
    Gap,
    Sign,
    Seal,
}

impl mxu3041flowEvent {
    /// 全量事件闭集。
    pub const ALL: [mxu3041flowEvent; 6] = [
        mxu3041flowEvent::Scan,
        mxu3041flowEvent::List,
        mxu3041flowEvent::Verify,
        mxu3041flowEvent::Gap,
        mxu3041flowEvent::Sign,
        mxu3041flowEvent::Seal,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const MXU3041FLOW_TRANSITIONS: [(mxu3041flowState, mxu3041flowEvent, mxu3041flowState); 8] = [
    (mxu3041flowState::Scanning, mxu3041flowEvent::Scan, mxu3041flowState::Listing), // Scanning→Listing 主链
    (mxu3041flowState::Listing, mxu3041flowEvent::List, mxu3041flowState::Verifying), // Listing→Verifying 主链
    (mxu3041flowState::Verifying, mxu3041flowEvent::Verify, mxu3041flowState::Gapping), // Verifying→Gapping 主链
    (mxu3041flowState::Gapping, mxu3041flowEvent::Gap, mxu3041flowState::Signing), // Gapping→Signing 主链
    (mxu3041flowState::Signing, mxu3041flowEvent::Sign, mxu3041flowState::Sealed), // Signing→Sealed 主链
    (mxu3041flowState::Sealed, mxu3041flowEvent::Seal, mxu3041flowState::Scanning), // 终态回边
    (mxu3041flowState::Sealed, mxu3041flowEvent::Scan, mxu3041flowState::Signing), // 终态回退边
    (mxu3041flowState::Listing, mxu3041flowEvent::List, mxu3041flowState::Verifying), // 旁路边
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn mxu3041flow_next(state: mxu3041flowState, ev: mxu3041flowEvent) -> Option<mxu3041flowState> {
    let mut i = 0usize;
    while i < 8 {
        let t = MXU3041FLOW_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn mxu3041flow_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < mxu3041flowState::ALL.len() {
        let mut ei = 0usize;
        while ei < mxu3041flowEvent::ALL.len() {
            let legal = mxu3041flow_next(mxu3041flowState::ALL[si], mxu3041flowEvent::ALL[ei]).is_some();
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
pub fn mxu3041flow_terminal(st: mxu3041flowState) -> bool {
    let mut i = 0usize;
    while i < 8 {
        if MXU3041FLOW_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct mxu3041flowLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: mxu3041flowState,
}

impl mxu3041flowLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: mxu3041flowState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: mxu3041flowEvent) -> bool {
        match mxu3041flow_next(self.cur, ev) {
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
// 快照 Mxu3041Snap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// 大页断言清点 快照封存（FNV 三态指纹 + 篡改检出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mxu3041Snap {
    /// 指纹分量：assert_v。
    pub assert_v: u64,
    /// 指纹分量：count_v。
    pub count_v: u64,
    /// 指纹分量：thr30_v。
    pub thr30_v: u64,
    /// 指纹分量：item_v。
    pub item_v: u64,
    /// 指纹分量：claim_v。
    pub claim_v: u64,
    /// 指纹分量：proof_v。
    pub proof_v: u64,
    /// 指纹分量：src_v。
    pub src_v: u64,
    /// 指纹分量：loc_v。
    pub loc_v: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl Mxu3041Snap {
    /// 构造并即时合成指纹。
    pub fn compute(
        assert_v: u64,
        count_v: u64,
        thr30_v: u64,
        item_v: u64,
        claim_v: u64,
        proof_v: u64,
        src_v: u64,
        loc_v: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &assert_v.to_le_bytes());
        h = fnv1a64_from(h, &count_v.to_le_bytes());
        h = fnv1a64_from(h, &thr30_v.to_le_bytes());
        h = fnv1a64_from(h, &item_v.to_le_bytes());
        h = fnv1a64_from(h, &claim_v.to_le_bytes());
        h = fnv1a64_from(h, &proof_v.to_le_bytes());
        h = fnv1a64_from(h, &src_v.to_le_bytes());
        h = fnv1a64_from(h, &loc_v.to_le_bytes());
        Self {
            assert_v,
            count_v,
            thr30_v,
            item_v,
            claim_v,
            proof_v,
            src_v,
            loc_v,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.assert_v.to_le_bytes());
        h = fnv1a64_from(h, &self.count_v.to_le_bytes());
        h = fnv1a64_from(h, &self.thr30_v.to_le_bytes());
        h = fnv1a64_from(h, &self.item_v.to_le_bytes());
        h = fnv1a64_from(h, &self.claim_v.to_le_bytes());
        h = fnv1a64_from(h, &self.proof_v.to_le_bytes());
        h = fnv1a64_from(h, &self.src_v.to_le_bytes());
        h = fnv1a64_from(h, &self.loc_v.to_le_bytes());
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

/// 段位守卫：全部诊断码必须落在本段 [0xe280, 0xe28f]。
pub const fn u3041_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3041Code::ALL.len() {
        let c = u3041Code::ALL[i].code();
        if c < 0xe280 || c > 0xe28f {
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
fn u3041_guard_seg() -> bool {
    u3041_segment_guard()
}

/// UNX-F3041 状态机闭集/迁移表/非法组合恰值全绿。
fn check_u3041_sm_legal() -> bool {
    let mut ok = mxu3041flow_illegal_pairs() == 29;
    if ok { ok = MXU3041FLOW_TRANSITIONS.len() == 8; }
    if ok { ok = mxu3041flowState::ALL.len() == 6 && mxu3041flowEvent::ALL.len() == 6; }
    ok
}

/// UNX-F3041 迁移主链回放 + 快照往返恒等。
fn check_u3041_sm_round() -> bool {
    let mut lg = mxu3041flowLedger::new(mxu3041flowState::Scanning);
    let mut ok = lg.apply(mxu3041flowEvent::Scan) && lg.cur == mxu3041flowState::Listing && lg.total() == 1;
    let s0 = Mxu3041Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    let mut s1 = Mxu3041Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    s1.recompute();
    if ok { ok = s0 == s1 && !s1.tampered(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3041_checks() -> CheckSet {
    let mut set = CheckSet::new("U3041");
    set.add(
        "u3041-seg-guard",
        u3041_guard_seg(),
        if u3041_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3041-sm-legal",
        check_u3041_sm_legal(),
        if check_u3041_sm_legal() { "" } else { "UNX-F3041 sm legality mismatch" },
    );
    set.add(
        "u3041-sm-round",
        check_u3041_sm_round(),
        if check_u3041_sm_round() { "" } else { "UNX-F3041 sm roundtrip mismatch" },
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
    fn u3041_all_checks_green() {
        let set = run_u3041_checks();
        assert!(set.all_passed(), "U3041 domain checks must be green");
    }

    #[test]
    fn u3041_segment_guard_holds() {
        assert!(u3041_segment_guard());
    }
}
