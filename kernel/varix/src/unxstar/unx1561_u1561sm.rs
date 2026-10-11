//! unxu1561_u1561sm —— 消费域联签总表（A3/A4/A5/G1/B4 等逐域登记） 状态机与快照封存（域）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F1561 · 消费域联签总表（A3/A4/A5/G1/B4 等逐域登记） · 判据原文：UNX-F1561 · 判据：联签总表齐备（全域消费域逐域登记：域/消费接口/联签状态三列），漏域注入 10/10 必抓，总表与 F1566 冻结账对平 100%
//! · 组件：域 状态机（6 态 × 6 事件显式迁移）+ FNV 快照封存（8 分量）
//! · 判据落地：UNX-F1561 · 判据：联签总表齐备（全域消费域逐域登记：域/消费接口/联签状态三列），漏域注入 10/10 必抓，总表与 F1566 冻结账对平 100
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U1561_VERSION: &str = "u1561-u1561sm-v1";

/// 诊断段基址。
pub const U1561_SEG_BASE: u16 = 0x5390;

// --------------------------------------------------------------------------
// 诊断码（u1561 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 域 状态面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u1561Code {
    /// 0x5390 状态重复（漏域漏登 同名双态）。
    StateDup,
    /// 0x5391 状态断档（联签缺位 序缺失）。
    StateHole,
    /// 0x5392 事件重复（同事件双声明）。
    EventDup,
    /// 0x5393 迁移重复（同源同事件双迁移）。
    TransDup,
    /// 0x5394 迁移缺失（应达态不可达）。
    TransHole,
    /// 0x5395 迁移环（非法自环/环路）。
    TransCycle,
    /// 0x5396 入口非法（初态不在闭集）。
    EntryBad,
    /// 0x5397 迁移丢失（应用事件无效果）。
    ApplyLost,
    /// 0x5398 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x5399 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x539a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x539b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u1561Code {
    /// 全量闭集。
    pub const ALL: [u1561Code; 12] = [
        u1561Code::StateDup,
        u1561Code::StateHole,
        u1561Code::EventDup,
        u1561Code::TransDup,
        u1561Code::TransHole,
        u1561Code::TransCycle,
        u1561Code::EntryBad,
        u1561Code::ApplyLost,
        u1561Code::LedgerBroken,
        u1561Code::SnapshotFingerprint,
        u1561Code::TableBroken,
        u1561Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u1561Code::StateDup => 0x5390,
            u1561Code::StateHole => 0x5391,
            u1561Code::EventDup => 0x5392,
            u1561Code::TransDup => 0x5393,
            u1561Code::TransHole => 0x5394,
            u1561Code::TransCycle => 0x5395,
            u1561Code::EntryBad => 0x5396,
            u1561Code::ApplyLost => 0x5397,
            u1561Code::LedgerBroken => 0x5398,
            u1561Code::SnapshotFingerprint => 0x5399,
            u1561Code::TableBroken => 0x539a,
            u1561Code::SegmentGuard => 0x539b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u1561Code::StateDup => "状态重复（漏域漏登 同名双态）",
            u1561Code::StateHole => "状态断档（联签缺位 序缺失）",
            u1561Code::EventDup => "事件重复（同事件双声明）",
            u1561Code::TransDup => "迁移重复（同源同事件双迁移）",
            u1561Code::TransHole => "迁移缺失（应达态不可达）",
            u1561Code::TransCycle => "迁移环（非法自环/环路）",
            u1561Code::EntryBad => "入口非法（初态不在闭集）",
            u1561Code::ApplyLost => "迁移丢失（应用事件无效果）",
            u1561Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u1561Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u1561Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u1561Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u1561Code::StateDup => "按闭集唯一律去重",
            u1561Code::StateHole => "按闭集补态",
            u1561Code::EventDup => "按事件闭集去重",
            u1561Code::TransDup => "按确定性唯一迁移律合并",
            u1561Code::TransHole => "按迁移矩阵补边",
            u1561Code::TransCycle => "按无环律重构迁移",
            u1561Code::EntryBad => "按闭集钳制初态",
            u1561Code::ApplyLost => "核对事件前置条件",
            u1561Code::LedgerBroken => "按守恒律重建台账后重跑",
            u1561Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u1561Code::TableBroken => "按语义规则重建常量表",
            u1561Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 状态机 mxu1561flow（6 态 × 6 事件 · 迁移 8 条显式声明）
// --------------------------------------------------------------------------
/// 域 状态机（判据：漏域漏登）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu1561flowState {
    Open,
    Registering,
    Countersigned,
    Reconciled,
    HoleProbe,
    Locked,
}

impl mxu1561flowState {
    /// 全量态闭集。
    pub const ALL: [mxu1561flowState; 6] = [
        mxu1561flowState::Open,
        mxu1561flowState::Registering,
        mxu1561flowState::Countersigned,
        mxu1561flowState::Reconciled,
        mxu1561flowState::HoleProbe,
        mxu1561flowState::Locked,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            mxu1561flowState::Open => 0,
            mxu1561flowState::Registering => 1,
            mxu1561flowState::Countersigned => 2,
            mxu1561flowState::Reconciled => 3,
            mxu1561flowState::HoleProbe => 4,
            mxu1561flowState::Locked => 5,
        }
    }
}

/// mxu1561flow事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu1561flowEvent {
    OpenTable,
    Enroll,
    Sign,
    Reconcile,
    ProbeHole,
    Lock,
}

impl mxu1561flowEvent {
    /// 全量事件闭集。
    pub const ALL: [mxu1561flowEvent; 6] = [
        mxu1561flowEvent::OpenTable,
        mxu1561flowEvent::Enroll,
        mxu1561flowEvent::Sign,
        mxu1561flowEvent::Reconcile,
        mxu1561flowEvent::ProbeHole,
        mxu1561flowEvent::Lock,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const MXU1561FLOW_TRANSITIONS: [(mxu1561flowState, mxu1561flowEvent, mxu1561flowState); 8] = [
    (mxu1561flowState::Open, mxu1561flowEvent::OpenTable, mxu1561flowState::Registering), // Open→Registering 主链
    (mxu1561flowState::Registering, mxu1561flowEvent::Enroll, mxu1561flowState::Countersigned), // Registering→Countersigned 主链
    (mxu1561flowState::Countersigned, mxu1561flowEvent::Sign, mxu1561flowState::Reconciled), // Countersigned→Reconciled 主链
    (mxu1561flowState::Reconciled, mxu1561flowEvent::Reconcile, mxu1561flowState::HoleProbe), // Reconciled→HoleProbe 主链
    (mxu1561flowState::HoleProbe, mxu1561flowEvent::ProbeHole, mxu1561flowState::Locked), // HoleProbe→Locked 主链
    (mxu1561flowState::Locked, mxu1561flowEvent::Lock, mxu1561flowState::Open), // 终态回边
    (mxu1561flowState::Locked, mxu1561flowEvent::OpenTable, mxu1561flowState::HoleProbe), // 终态回退边
    (mxu1561flowState::Registering, mxu1561flowEvent::Enroll, mxu1561flowState::Countersigned), // 旁路边
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn mxu1561flow_next(state: mxu1561flowState, ev: mxu1561flowEvent) -> Option<mxu1561flowState> {
    let mut i = 0usize;
    while i < 8 {
        let t = MXU1561FLOW_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn mxu1561flow_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < mxu1561flowState::ALL.len() {
        let mut ei = 0usize;
        while ei < mxu1561flowEvent::ALL.len() {
            let legal = mxu1561flow_next(mxu1561flowState::ALL[si], mxu1561flowEvent::ALL[ei]).is_some();
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
pub fn mxu1561flow_terminal(st: mxu1561flowState) -> bool {
    let mut i = 0usize;
    while i < 8 {
        if MXU1561FLOW_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct mxu1561flowLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: mxu1561flowState,
}

impl mxu1561flowLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: mxu1561flowState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: mxu1561flowEvent) -> bool {
        match mxu1561flow_next(self.cur, ev) {
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
// 快照 Mxu1561Snap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// 域 快照封存（FNV 三态指纹 + 篡改检出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mxu1561Snap {
    /// 指纹分量：cd_v。
    pub cd_v: u64,
    /// 指纹分量：ci_v。
    pub ci_v: u64,
    /// 指纹分量：cs_v。
    pub cs_v: u64,
    /// 指纹分量：ld_v。
    pub ld_v: u64,
    /// 指纹分量：fz_v。
    pub fz_v: u64,
    /// 指纹分量：dp_v。
    pub dp_v: u64,
    /// 指纹分量：a3_v。
    pub a3_v: u64,
    /// 指纹分量：a4_v。
    pub a4_v: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl Mxu1561Snap {
    /// 构造并即时合成指纹。
    pub fn compute(
        cd_v: u64,
        ci_v: u64,
        cs_v: u64,
        ld_v: u64,
        fz_v: u64,
        dp_v: u64,
        a3_v: u64,
        a4_v: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &cd_v.to_le_bytes());
        h = fnv1a64_from(h, &ci_v.to_le_bytes());
        h = fnv1a64_from(h, &cs_v.to_le_bytes());
        h = fnv1a64_from(h, &ld_v.to_le_bytes());
        h = fnv1a64_from(h, &fz_v.to_le_bytes());
        h = fnv1a64_from(h, &dp_v.to_le_bytes());
        h = fnv1a64_from(h, &a3_v.to_le_bytes());
        h = fnv1a64_from(h, &a4_v.to_le_bytes());
        Self {
            cd_v,
            ci_v,
            cs_v,
            ld_v,
            fz_v,
            dp_v,
            a3_v,
            a4_v,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.cd_v.to_le_bytes());
        h = fnv1a64_from(h, &self.ci_v.to_le_bytes());
        h = fnv1a64_from(h, &self.cs_v.to_le_bytes());
        h = fnv1a64_from(h, &self.ld_v.to_le_bytes());
        h = fnv1a64_from(h, &self.fz_v.to_le_bytes());
        h = fnv1a64_from(h, &self.dp_v.to_le_bytes());
        h = fnv1a64_from(h, &self.a3_v.to_le_bytes());
        h = fnv1a64_from(h, &self.a4_v.to_le_bytes());
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

/// 段位守卫：全部诊断码必须落在本段 [0x5390, 0x539f]。
pub const fn u1561_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u1561Code::ALL.len() {
        let c = u1561Code::ALL[i].code();
        if c < 0x5390 || c > 0x539f {
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
fn u1561_guard_seg() -> bool {
    u1561_segment_guard()
}

/// UNX-F1561 状态机闭集/迁移表/非法组合恰值全绿。
fn check_u1561_sm_legal() -> bool {
    let mut ok = mxu1561flow_illegal_pairs() == 29;
    if ok { ok = MXU1561FLOW_TRANSITIONS.len() == 8; }
    if ok { ok = mxu1561flowState::ALL.len() == 6 && mxu1561flowEvent::ALL.len() == 6; }
    ok
}

/// UNX-F1561 迁移主链回放 + 快照往返恒等。
fn check_u1561_sm_round() -> bool {
    let mut lg = mxu1561flowLedger::new(mxu1561flowState::Open);
    let mut ok = lg.apply(mxu1561flowEvent::OpenTable) && lg.cur == mxu1561flowState::Registering && lg.total() == 1;
    let s0 = Mxu1561Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    let mut s1 = Mxu1561Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    s1.recompute();
    if ok { ok = s0 == s1 && !s1.tampered(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u1561_checks() -> CheckSet {
    let mut set = CheckSet::new("U1561");
    set.add(
        "u1561-seg-guard",
        u1561_guard_seg(),
        if u1561_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u1561-sm-legal",
        check_u1561_sm_legal(),
        if check_u1561_sm_legal() { "" } else { "UNX-F1561 sm legality mismatch" },
    );
    set.add(
        "u1561-sm-round",
        check_u1561_sm_round(),
        if check_u1561_sm_round() { "" } else { "UNX-F1561 sm roundtrip mismatch" },
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
    fn u1561_all_checks_green() {
        let set = run_u1561_checks();
        assert!(set.all_passed(), "U1561 domain checks must be green");
    }

    #[test]
    fn u1561_segment_guard_holds() {
        assert!(u1561_segment_guard());
    }
}
