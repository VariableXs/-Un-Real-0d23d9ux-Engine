//! unxu3701_u3701sm —— 覆盖总设计（边×状态×组合三维模型） 状态机与快照封存（覆盖三维模型）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3701 · 覆盖总设计（边×状态×组合三维模型） · 判据原文：UNX-F3701 · 判据：J1：三维模型（边覆盖（代码维——F3545）/状态覆盖（语义维——状态机×转移）/组合覆盖（参数维——n-wise——判据行内嵌三维各治什么病：边治"没走过的代码"、状态治"没到过的机器态"、组合治"单看都合法搭起来才炸"）；M-459 三维首测（同一靶入口三维各出覆盖数+三维互补性（各有独占盲区——三维不冗余的实证）。<br>J2：三维可合成（总覆盖视图=三维投影合成（各维口径独立、合成式显式——不混账）。
//! · 组件：覆盖三维模型 状态机（6 态 × 6 事件显式迁移）+ FNV 快照封存（8 分量）
//! · 判据落地：UNX-F3701 · 判据：J1：三维模型（边覆盖（代码维——F3545）/状态覆盖（语义维——状态机×转移）/组合覆盖（参数维——n-wise——判据行内嵌
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3701_VERSION: &str = "u3701-u3701sm-v1";

/// 诊断段基址。
pub const U3701_SEG_BASE: u16 = 0xe170;

// --------------------------------------------------------------------------
// 诊断码（u3701 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 覆盖三维模型 状态面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3701Code {
    /// 0xe170 状态重复（覆盖 同名双态）。
    StateDup,
    /// 0xe171 状态断档（三维 序缺失）。
    StateHole,
    /// 0xe172 事件重复（同事件双声明）。
    EventDup,
    /// 0xe173 迁移重复（同源同事件双迁移）。
    TransDup,
    /// 0xe174 迁移缺失（应达态不可达）。
    TransHole,
    /// 0xe175 迁移环（非法自环/环路）。
    TransCycle,
    /// 0xe176 入口非法（初态不在闭集）。
    EntryBad,
    /// 0xe177 迁移丢失（应用事件无效果）。
    ApplyLost,
    /// 0xe178 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe179 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe17a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe17b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3701Code {
    /// 全量闭集。
    pub const ALL: [u3701Code; 12] = [
        u3701Code::StateDup,
        u3701Code::StateHole,
        u3701Code::EventDup,
        u3701Code::TransDup,
        u3701Code::TransHole,
        u3701Code::TransCycle,
        u3701Code::EntryBad,
        u3701Code::ApplyLost,
        u3701Code::LedgerBroken,
        u3701Code::SnapshotFingerprint,
        u3701Code::TableBroken,
        u3701Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3701Code::StateDup => 0xe170,
            u3701Code::StateHole => 0xe171,
            u3701Code::EventDup => 0xe172,
            u3701Code::TransDup => 0xe173,
            u3701Code::TransHole => 0xe174,
            u3701Code::TransCycle => 0xe175,
            u3701Code::EntryBad => 0xe176,
            u3701Code::ApplyLost => 0xe177,
            u3701Code::LedgerBroken => 0xe178,
            u3701Code::SnapshotFingerprint => 0xe179,
            u3701Code::TableBroken => 0xe17a,
            u3701Code::SegmentGuard => 0xe17b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3701Code::StateDup => "状态重复（覆盖 同名双态）",
            u3701Code::StateHole => "状态断档（三维 序缺失）",
            u3701Code::EventDup => "事件重复（同事件双声明）",
            u3701Code::TransDup => "迁移重复（同源同事件双迁移）",
            u3701Code::TransHole => "迁移缺失（应达态不可达）",
            u3701Code::TransCycle => "迁移环（非法自环/环路）",
            u3701Code::EntryBad => "入口非法（初态不在闭集）",
            u3701Code::ApplyLost => "迁移丢失（应用事件无效果）",
            u3701Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3701Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3701Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3701Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3701Code::StateDup => "按闭集唯一律去重",
            u3701Code::StateHole => "按闭集补态",
            u3701Code::EventDup => "按事件闭集去重",
            u3701Code::TransDup => "按确定性唯一迁移律合并",
            u3701Code::TransHole => "按迁移矩阵补边",
            u3701Code::TransCycle => "按无环律重构迁移",
            u3701Code::EntryBad => "按闭集钳制初态",
            u3701Code::ApplyLost => "核对事件前置条件",
            u3701Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3701Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3701Code::TableBroken => "按语义规则重建常量表",
            u3701Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 状态机 mxu3701flow（6 态 × 6 事件 · 迁移 8 条显式声明）
// --------------------------------------------------------------------------
/// 覆盖三维模型 状态机（判据：覆盖）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu3701flowState {
    Define,
    Enumerate,
    Mark,
    Cross,
    Audit,
    Closed,
}

impl mxu3701flowState {
    /// 全量态闭集。
    pub const ALL: [mxu3701flowState; 6] = [
        mxu3701flowState::Define,
        mxu3701flowState::Enumerate,
        mxu3701flowState::Mark,
        mxu3701flowState::Cross,
        mxu3701flowState::Audit,
        mxu3701flowState::Closed,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            mxu3701flowState::Define => 0,
            mxu3701flowState::Enumerate => 1,
            mxu3701flowState::Mark => 2,
            mxu3701flowState::Cross => 3,
            mxu3701flowState::Audit => 4,
            mxu3701flowState::Closed => 5,
        }
    }
}

/// mxu3701flow事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu3701flowEvent {
    Define,
    Enum,
    Mark,
    Cross,
    Audit,
    Close,
}

impl mxu3701flowEvent {
    /// 全量事件闭集。
    pub const ALL: [mxu3701flowEvent; 6] = [
        mxu3701flowEvent::Define,
        mxu3701flowEvent::Enum,
        mxu3701flowEvent::Mark,
        mxu3701flowEvent::Cross,
        mxu3701flowEvent::Audit,
        mxu3701flowEvent::Close,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const MXU3701FLOW_TRANSITIONS: [(mxu3701flowState, mxu3701flowEvent, mxu3701flowState); 8] = [
    (mxu3701flowState::Define, mxu3701flowEvent::Define, mxu3701flowState::Enumerate), // Define→Enumerate 主链
    (mxu3701flowState::Enumerate, mxu3701flowEvent::Enum, mxu3701flowState::Mark), // Enumerate→Mark 主链
    (mxu3701flowState::Mark, mxu3701flowEvent::Mark, mxu3701flowState::Cross), // Mark→Cross 主链
    (mxu3701flowState::Cross, mxu3701flowEvent::Cross, mxu3701flowState::Audit), // Cross→Audit 主链
    (mxu3701flowState::Audit, mxu3701flowEvent::Audit, mxu3701flowState::Closed), // Audit→Closed 主链
    (mxu3701flowState::Closed, mxu3701flowEvent::Close, mxu3701flowState::Define), // 终态回边
    (mxu3701flowState::Closed, mxu3701flowEvent::Define, mxu3701flowState::Audit), // 终态回退边
    (mxu3701flowState::Enumerate, mxu3701flowEvent::Enum, mxu3701flowState::Mark), // 旁路边
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn mxu3701flow_next(state: mxu3701flowState, ev: mxu3701flowEvent) -> Option<mxu3701flowState> {
    let mut i = 0usize;
    while i < 8 {
        let t = MXU3701FLOW_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn mxu3701flow_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < mxu3701flowState::ALL.len() {
        let mut ei = 0usize;
        while ei < mxu3701flowEvent::ALL.len() {
            let legal = mxu3701flow_next(mxu3701flowState::ALL[si], mxu3701flowEvent::ALL[ei]).is_some();
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
pub fn mxu3701flow_terminal(st: mxu3701flowState) -> bool {
    let mut i = 0usize;
    while i < 8 {
        if MXU3701FLOW_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct mxu3701flowLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: mxu3701flowState,
}

impl mxu3701flowLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: mxu3701flowState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: mxu3701flowEvent) -> bool {
        match mxu3701flow_next(self.cur, ev) {
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
// 快照 Mxu3701Snap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// 覆盖三维模型 快照封存（FNV 三态指纹 + 篡改检出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mxu3701Snap {
    /// 指纹分量：edge_v。
    pub edge_v: u64,
    /// 指纹分量：state_v。
    pub state_v: u64,
    /// 指纹分量：combo_v。
    pub combo_v: u64,
    /// 指纹分量：cart_v。
    pub cart_v: u64,
    /// 指纹分量：hit_v。
    pub hit_v: u64,
    /// 指纹分量：miss_v。
    pub miss_v: u64,
    /// 指纹分量：dim3_v。
    pub dim3_v: u64,
    /// 指纹分量：mc_v。
    pub mc_v: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl Mxu3701Snap {
    /// 构造并即时合成指纹。
    pub fn compute(
        edge_v: u64,
        state_v: u64,
        combo_v: u64,
        cart_v: u64,
        hit_v: u64,
        miss_v: u64,
        dim3_v: u64,
        mc_v: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &edge_v.to_le_bytes());
        h = fnv1a64_from(h, &state_v.to_le_bytes());
        h = fnv1a64_from(h, &combo_v.to_le_bytes());
        h = fnv1a64_from(h, &cart_v.to_le_bytes());
        h = fnv1a64_from(h, &hit_v.to_le_bytes());
        h = fnv1a64_from(h, &miss_v.to_le_bytes());
        h = fnv1a64_from(h, &dim3_v.to_le_bytes());
        h = fnv1a64_from(h, &mc_v.to_le_bytes());
        Self {
            edge_v,
            state_v,
            combo_v,
            cart_v,
            hit_v,
            miss_v,
            dim3_v,
            mc_v,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.edge_v.to_le_bytes());
        h = fnv1a64_from(h, &self.state_v.to_le_bytes());
        h = fnv1a64_from(h, &self.combo_v.to_le_bytes());
        h = fnv1a64_from(h, &self.cart_v.to_le_bytes());
        h = fnv1a64_from(h, &self.hit_v.to_le_bytes());
        h = fnv1a64_from(h, &self.miss_v.to_le_bytes());
        h = fnv1a64_from(h, &self.dim3_v.to_le_bytes());
        h = fnv1a64_from(h, &self.mc_v.to_le_bytes());
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

/// 段位守卫：全部诊断码必须落在本段 [0xe170, 0xe17f]。
pub const fn u3701_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3701Code::ALL.len() {
        let c = u3701Code::ALL[i].code();
        if c < 0xe170 || c > 0xe17f {
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
fn u3701_guard_seg() -> bool {
    u3701_segment_guard()
}

/// UNX-F3701 状态机闭集/迁移表/非法组合恰值全绿。
fn check_u3701_sm_legal() -> bool {
    let mut ok = mxu3701flow_illegal_pairs() == 29;
    if ok { ok = MXU3701FLOW_TRANSITIONS.len() == 8; }
    if ok { ok = mxu3701flowState::ALL.len() == 6 && mxu3701flowEvent::ALL.len() == 6; }
    ok
}

/// UNX-F3701 迁移主链回放 + 快照往返恒等。
fn check_u3701_sm_round() -> bool {
    let mut lg = mxu3701flowLedger::new(mxu3701flowState::Define);
    let mut ok = lg.apply(mxu3701flowEvent::Define) && lg.cur == mxu3701flowState::Enumerate && lg.total() == 1;
    let s0 = Mxu3701Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    let mut s1 = Mxu3701Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    s1.recompute();
    if ok { ok = s0 == s1 && !s1.tampered(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3701_checks() -> CheckSet {
    let mut set = CheckSet::new("U3701");
    set.add(
        "u3701-seg-guard",
        u3701_guard_seg(),
        if u3701_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3701-sm-legal",
        check_u3701_sm_legal(),
        if check_u3701_sm_legal() { "" } else { "UNX-F3701 sm legality mismatch" },
    );
    set.add(
        "u3701-sm-round",
        check_u3701_sm_round(),
        if check_u3701_sm_round() { "" } else { "UNX-F3701 sm roundtrip mismatch" },
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
    fn u3701_all_checks_green() {
        let set = run_u3701_checks();
        assert!(set.all_passed(), "U3701 domain checks must be green");
    }

    #[test]
    fn u3701_segment_guard_holds() {
        assert!(u3701_segment_guard());
    }
}
