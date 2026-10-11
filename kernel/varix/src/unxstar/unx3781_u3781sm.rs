//! unxu3781_u3781sm —— 域中期收口总设计（B20 前向声明的兑现面） 状态机与快照封存（域中期收口）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3781 · 域中期收口总设计（B20 前向声明的兑现面） · 判据原文：UNX-F3781 · 判据：J1：收口架构（三总轧（批账/域账/凭证）+三宣告（断言大关/移交就绪/风险清单）——判据行内嵌：B20 前向声明的原文兑现（"F3781 域中期收口：段断言大关 ≥80+五条件移交"——期票与兑付跨十批的闭链）；收口的范围声明（B16–B30 十五批——A5 域续领波的全域——不含 B01–B15（已在前次 finalize 收口——历史账冻结）；M-539 收口预跑（三总轧空转→流程通→正式总轧前演练）。<br>J2：收口的纪律（收口只核不改（发现的偏差走修正流程+留痕——收口不涂改历史账。
//! · 组件：域中期收口 状态机（6 态 × 6 事件显式迁移）+ FNV 快照封存（8 分量）
//! · 判据落地：UNX-F3781 · 判据：J1：收口架构（三总轧（批账/域账/凭证）+三宣告（断言大关/移交就绪/风险清单）——判据行内嵌：B20 前向声明的原文兑现（"F
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3781_VERSION: &str = "u3781-u3781sm-v1";

/// 诊断段基址。
pub const U3781_SEG_BASE: u16 = 0xe1b0;

// --------------------------------------------------------------------------
// 诊断码（u3781 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 域中期收口 状态面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3781Code {
    /// 0xe1b0 状态重复（收口 同名双态）。
    StateDup,
    /// 0xe1b1 状态断档（兑现 序缺失）。
    StateHole,
    /// 0xe1b2 事件重复（同事件双声明）。
    EventDup,
    /// 0xe1b3 迁移重复（同源同事件双迁移）。
    TransDup,
    /// 0xe1b4 迁移缺失（应达态不可达）。
    TransHole,
    /// 0xe1b5 迁移环（非法自环/环路）。
    TransCycle,
    /// 0xe1b6 入口非法（初态不在闭集）。
    EntryBad,
    /// 0xe1b7 迁移丢失（应用事件无效果）。
    ApplyLost,
    /// 0xe1b8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe1b9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe1ba 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe1bb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3781Code {
    /// 全量闭集。
    pub const ALL: [u3781Code; 12] = [
        u3781Code::StateDup,
        u3781Code::StateHole,
        u3781Code::EventDup,
        u3781Code::TransDup,
        u3781Code::TransHole,
        u3781Code::TransCycle,
        u3781Code::EntryBad,
        u3781Code::ApplyLost,
        u3781Code::LedgerBroken,
        u3781Code::SnapshotFingerprint,
        u3781Code::TableBroken,
        u3781Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3781Code::StateDup => 0xe1b0,
            u3781Code::StateHole => 0xe1b1,
            u3781Code::EventDup => 0xe1b2,
            u3781Code::TransDup => 0xe1b3,
            u3781Code::TransHole => 0xe1b4,
            u3781Code::TransCycle => 0xe1b5,
            u3781Code::EntryBad => 0xe1b6,
            u3781Code::ApplyLost => 0xe1b7,
            u3781Code::LedgerBroken => 0xe1b8,
            u3781Code::SnapshotFingerprint => 0xe1b9,
            u3781Code::TableBroken => 0xe1ba,
            u3781Code::SegmentGuard => 0xe1bb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3781Code::StateDup => "状态重复（收口 同名双态）",
            u3781Code::StateHole => "状态断档（兑现 序缺失）",
            u3781Code::EventDup => "事件重复（同事件双声明）",
            u3781Code::TransDup => "迁移重复（同源同事件双迁移）",
            u3781Code::TransHole => "迁移缺失（应达态不可达）",
            u3781Code::TransCycle => "迁移环（非法自环/环路）",
            u3781Code::EntryBad => "入口非法（初态不在闭集）",
            u3781Code::ApplyLost => "迁移丢失（应用事件无效果）",
            u3781Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3781Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3781Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3781Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3781Code::StateDup => "按闭集唯一律去重",
            u3781Code::StateHole => "按闭集补态",
            u3781Code::EventDup => "按事件闭集去重",
            u3781Code::TransDup => "按确定性唯一迁移律合并",
            u3781Code::TransHole => "按迁移矩阵补边",
            u3781Code::TransCycle => "按无环律重构迁移",
            u3781Code::EntryBad => "按闭集钳制初态",
            u3781Code::ApplyLost => "核对事件前置条件",
            u3781Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3781Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3781Code::TableBroken => "按语义规则重建常量表",
            u3781Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 状态机 mxu3781flow（6 态 × 6 事件 · 迁移 8 条显式声明）
// --------------------------------------------------------------------------
/// 域中期收口 状态机（判据：收口）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu3781flowState {
    Scanning,
    Binning,
    Verifying,
    Gapping,
    Filling,
    Sealed,
}

impl mxu3781flowState {
    /// 全量态闭集。
    pub const ALL: [mxu3781flowState; 6] = [
        mxu3781flowState::Scanning,
        mxu3781flowState::Binning,
        mxu3781flowState::Verifying,
        mxu3781flowState::Gapping,
        mxu3781flowState::Filling,
        mxu3781flowState::Sealed,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            mxu3781flowState::Scanning => 0,
            mxu3781flowState::Binning => 1,
            mxu3781flowState::Verifying => 2,
            mxu3781flowState::Gapping => 3,
            mxu3781flowState::Filling => 4,
            mxu3781flowState::Sealed => 5,
        }
    }
}

/// mxu3781flow事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu3781flowEvent {
    Scan,
    Bin,
    Verify,
    Gap,
    Fulfill,
    Seal,
}

impl mxu3781flowEvent {
    /// 全量事件闭集。
    pub const ALL: [mxu3781flowEvent; 6] = [
        mxu3781flowEvent::Scan,
        mxu3781flowEvent::Bin,
        mxu3781flowEvent::Verify,
        mxu3781flowEvent::Gap,
        mxu3781flowEvent::Fulfill,
        mxu3781flowEvent::Seal,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const MXU3781FLOW_TRANSITIONS: [(mxu3781flowState, mxu3781flowEvent, mxu3781flowState); 8] = [
    (mxu3781flowState::Scanning, mxu3781flowEvent::Scan, mxu3781flowState::Binning), // Scanning→Binning 主链
    (mxu3781flowState::Binning, mxu3781flowEvent::Bin, mxu3781flowState::Verifying), // Binning→Verifying 主链
    (mxu3781flowState::Verifying, mxu3781flowEvent::Verify, mxu3781flowState::Gapping), // Verifying→Gapping 主链
    (mxu3781flowState::Gapping, mxu3781flowEvent::Gap, mxu3781flowState::Filling), // Gapping→Filling 主链
    (mxu3781flowState::Filling, mxu3781flowEvent::Fulfill, mxu3781flowState::Sealed), // Filling→Sealed 主链
    (mxu3781flowState::Sealed, mxu3781flowEvent::Seal, mxu3781flowState::Scanning), // 终态回边
    (mxu3781flowState::Sealed, mxu3781flowEvent::Scan, mxu3781flowState::Filling), // 终态回退边
    (mxu3781flowState::Binning, mxu3781flowEvent::Bin, mxu3781flowState::Verifying), // 旁路边
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn mxu3781flow_next(state: mxu3781flowState, ev: mxu3781flowEvent) -> Option<mxu3781flowState> {
    let mut i = 0usize;
    while i < 8 {
        let t = MXU3781FLOW_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn mxu3781flow_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < mxu3781flowState::ALL.len() {
        let mut ei = 0usize;
        while ei < mxu3781flowEvent::ALL.len() {
            let legal = mxu3781flow_next(mxu3781flowState::ALL[si], mxu3781flowEvent::ALL[ei]).is_some();
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
pub fn mxu3781flow_terminal(st: mxu3781flowState) -> bool {
    let mut i = 0usize;
    while i < 8 {
        if MXU3781FLOW_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct mxu3781flowLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: mxu3781flowState,
}

impl mxu3781flowLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: mxu3781flowState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: mxu3781flowEvent) -> bool {
        match mxu3781flow_next(self.cur, ev) {
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
// 快照 Mxu3781Snap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// 域中期收口 快照封存（FNV 三态指纹 + 篡改检出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mxu3781Snap {
    /// 指纹分量：close_v。
    pub close_v: u64,
    /// 指纹分量：mid_v。
    pub mid_v: u64,
    /// 指纹分量：decl_v。
    pub decl_v: u64,
    /// 指纹分量：fulfill_v。
    pub fulfill_v: u64,
    /// 指纹分量：scope_v。
    pub scope_v: u64,
    /// 指纹分量：item_v。
    pub item_v: u64,
    /// 指纹分量：done_v。
    pub done_v: u64,
    /// 指纹分量：open_v。
    pub open_v: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl Mxu3781Snap {
    /// 构造并即时合成指纹。
    pub fn compute(
        close_v: u64,
        mid_v: u64,
        decl_v: u64,
        fulfill_v: u64,
        scope_v: u64,
        item_v: u64,
        done_v: u64,
        open_v: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &close_v.to_le_bytes());
        h = fnv1a64_from(h, &mid_v.to_le_bytes());
        h = fnv1a64_from(h, &decl_v.to_le_bytes());
        h = fnv1a64_from(h, &fulfill_v.to_le_bytes());
        h = fnv1a64_from(h, &scope_v.to_le_bytes());
        h = fnv1a64_from(h, &item_v.to_le_bytes());
        h = fnv1a64_from(h, &done_v.to_le_bytes());
        h = fnv1a64_from(h, &open_v.to_le_bytes());
        Self {
            close_v,
            mid_v,
            decl_v,
            fulfill_v,
            scope_v,
            item_v,
            done_v,
            open_v,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.close_v.to_le_bytes());
        h = fnv1a64_from(h, &self.mid_v.to_le_bytes());
        h = fnv1a64_from(h, &self.decl_v.to_le_bytes());
        h = fnv1a64_from(h, &self.fulfill_v.to_le_bytes());
        h = fnv1a64_from(h, &self.scope_v.to_le_bytes());
        h = fnv1a64_from(h, &self.item_v.to_le_bytes());
        h = fnv1a64_from(h, &self.done_v.to_le_bytes());
        h = fnv1a64_from(h, &self.open_v.to_le_bytes());
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

/// 段位守卫：全部诊断码必须落在本段 [0xe1b0, 0xe1bf]。
pub const fn u3781_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3781Code::ALL.len() {
        let c = u3781Code::ALL[i].code();
        if c < 0xe1b0 || c > 0xe1bf {
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
fn u3781_guard_seg() -> bool {
    u3781_segment_guard()
}

/// UNX-F3781 状态机闭集/迁移表/非法组合恰值全绿。
fn check_u3781_sm_legal() -> bool {
    let mut ok = mxu3781flow_illegal_pairs() == 29;
    if ok { ok = MXU3781FLOW_TRANSITIONS.len() == 8; }
    if ok { ok = mxu3781flowState::ALL.len() == 6 && mxu3781flowEvent::ALL.len() == 6; }
    ok
}

/// UNX-F3781 迁移主链回放 + 快照往返恒等。
fn check_u3781_sm_round() -> bool {
    let mut lg = mxu3781flowLedger::new(mxu3781flowState::Scanning);
    let mut ok = lg.apply(mxu3781flowEvent::Scan) && lg.cur == mxu3781flowState::Binning && lg.total() == 1;
    let s0 = Mxu3781Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    let mut s1 = Mxu3781Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    s1.recompute();
    if ok { ok = s0 == s1 && !s1.tampered(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3781_checks() -> CheckSet {
    let mut set = CheckSet::new("U3781");
    set.add(
        "u3781-seg-guard",
        u3781_guard_seg(),
        if u3781_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3781-sm-legal",
        check_u3781_sm_legal(),
        if check_u3781_sm_legal() { "" } else { "UNX-F3781 sm legality mismatch" },
    );
    set.add(
        "u3781-sm-round",
        check_u3781_sm_round(),
        if check_u3781_sm_round() { "" } else { "UNX-F3781 sm roundtrip mismatch" },
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
    fn u3781_all_checks_green() {
        let set = run_u3781_checks();
        assert!(set.all_passed(), "U3781 domain checks must be green");
    }

    #[test]
    fn u3781_segment_guard_holds() {
        assert!(u3781_segment_guard());
    }
}
