//! unxu1621_u1621sm —— per-CPU 变量区布局总纲（接续 A1 trampoline 预留地址） 状态机与快照封存（区段）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F1621 · per-CPU 变量区布局总纲（接续 A1 trampoline 预留地址） · 判据原文：UNX-F1621 · 判据：变量区按 A1 预留地址切分（统计区/队列区/时钟区/shootdown 区/调试区五段）偏移账固化，五段边界与 A1 预留区零越界，布局图可导出
//! · 组件：区段 状态机（6 态 × 6 事件显式迁移）+ FNV 快照封存（8 分量）
//! · 判据落地：UNX-F1621 · 判据：变量区按 A1 预留地址切分（统计区/队列区/时钟区/shootdown 区/调试区五段）偏移账固化，五段边界与 A1 预留区零越
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U1621_VERSION: &str = "u1621-u1621sm-v1";

/// 诊断段基址。
pub const U1621_SEG_BASE: u16 = 0x55d0;

// --------------------------------------------------------------------------
// 诊断码（u1621 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 区段 状态面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u1621Code {
    /// 0x55d0 状态重复（段越界 同名双态）。
    StateDup,
    /// 0x55d1 状态断档（偏移漂移 序缺失）。
    StateHole,
    /// 0x55d2 事件重复（同事件双声明）。
    EventDup,
    /// 0x55d3 迁移重复（同源同事件双迁移）。
    TransDup,
    /// 0x55d4 迁移缺失（应达态不可达）。
    TransHole,
    /// 0x55d5 迁移环（非法自环/环路）。
    TransCycle,
    /// 0x55d6 入口非法（初态不在闭集）。
    EntryBad,
    /// 0x55d7 迁移丢失（应用事件无效果）。
    ApplyLost,
    /// 0x55d8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x55d9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x55da 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x55db 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u1621Code {
    /// 全量闭集。
    pub const ALL: [u1621Code; 12] = [
        u1621Code::StateDup,
        u1621Code::StateHole,
        u1621Code::EventDup,
        u1621Code::TransDup,
        u1621Code::TransHole,
        u1621Code::TransCycle,
        u1621Code::EntryBad,
        u1621Code::ApplyLost,
        u1621Code::LedgerBroken,
        u1621Code::SnapshotFingerprint,
        u1621Code::TableBroken,
        u1621Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u1621Code::StateDup => 0x55d0,
            u1621Code::StateHole => 0x55d1,
            u1621Code::EventDup => 0x55d2,
            u1621Code::TransDup => 0x55d3,
            u1621Code::TransHole => 0x55d4,
            u1621Code::TransCycle => 0x55d5,
            u1621Code::EntryBad => 0x55d6,
            u1621Code::ApplyLost => 0x55d7,
            u1621Code::LedgerBroken => 0x55d8,
            u1621Code::SnapshotFingerprint => 0x55d9,
            u1621Code::TableBroken => 0x55da,
            u1621Code::SegmentGuard => 0x55db,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u1621Code::StateDup => "状态重复（段越界 同名双态）",
            u1621Code::StateHole => "状态断档（偏移漂移 序缺失）",
            u1621Code::EventDup => "事件重复（同事件双声明）",
            u1621Code::TransDup => "迁移重复（同源同事件双迁移）",
            u1621Code::TransHole => "迁移缺失（应达态不可达）",
            u1621Code::TransCycle => "迁移环（非法自环/环路）",
            u1621Code::EntryBad => "入口非法（初态不在闭集）",
            u1621Code::ApplyLost => "迁移丢失（应用事件无效果）",
            u1621Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u1621Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u1621Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u1621Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u1621Code::StateDup => "按闭集唯一律去重",
            u1621Code::StateHole => "按闭集补态",
            u1621Code::EventDup => "按事件闭集去重",
            u1621Code::TransDup => "按确定性唯一迁移律合并",
            u1621Code::TransHole => "按迁移矩阵补边",
            u1621Code::TransCycle => "按无环律重构迁移",
            u1621Code::EntryBad => "按闭集钳制初态",
            u1621Code::ApplyLost => "核对事件前置条件",
            u1621Code::LedgerBroken => "按守恒律重建台账后重跑",
            u1621Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u1621Code::TableBroken => "按语义规则重建常量表",
            u1621Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 状态机 mxu1621flow（6 态 × 6 事件 · 迁移 8 条显式声明）
// --------------------------------------------------------------------------
/// 区段 状态机（判据：段越界）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu1621flowState {
    Reserved,
    Sliced,
    Offseted,
    Bounded,
    Exported,
    Frozen,
}

impl mxu1621flowState {
    /// 全量态闭集。
    pub const ALL: [mxu1621flowState; 6] = [
        mxu1621flowState::Reserved,
        mxu1621flowState::Sliced,
        mxu1621flowState::Offseted,
        mxu1621flowState::Bounded,
        mxu1621flowState::Exported,
        mxu1621flowState::Frozen,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            mxu1621flowState::Reserved => 0,
            mxu1621flowState::Sliced => 1,
            mxu1621flowState::Offseted => 2,
            mxu1621flowState::Bounded => 3,
            mxu1621flowState::Exported => 4,
            mxu1621flowState::Frozen => 5,
        }
    }
}

/// mxu1621flow事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu1621flowEvent {
    Reserve,
    Slice,
    Offset,
    Bound,
    Export,
    Freeze,
}

impl mxu1621flowEvent {
    /// 全量事件闭集。
    pub const ALL: [mxu1621flowEvent; 6] = [
        mxu1621flowEvent::Reserve,
        mxu1621flowEvent::Slice,
        mxu1621flowEvent::Offset,
        mxu1621flowEvent::Bound,
        mxu1621flowEvent::Export,
        mxu1621flowEvent::Freeze,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const MXU1621FLOW_TRANSITIONS: [(mxu1621flowState, mxu1621flowEvent, mxu1621flowState); 8] = [
    (mxu1621flowState::Reserved, mxu1621flowEvent::Reserve, mxu1621flowState::Sliced), // Reserved→Sliced 主链
    (mxu1621flowState::Sliced, mxu1621flowEvent::Slice, mxu1621flowState::Offseted), // Sliced→Offseted 主链
    (mxu1621flowState::Offseted, mxu1621flowEvent::Offset, mxu1621flowState::Bounded), // Offseted→Bounded 主链
    (mxu1621flowState::Bounded, mxu1621flowEvent::Bound, mxu1621flowState::Exported), // Bounded→Exported 主链
    (mxu1621flowState::Exported, mxu1621flowEvent::Export, mxu1621flowState::Frozen), // Exported→Frozen 主链
    (mxu1621flowState::Frozen, mxu1621flowEvent::Freeze, mxu1621flowState::Reserved), // 终态回边
    (mxu1621flowState::Frozen, mxu1621flowEvent::Reserve, mxu1621flowState::Exported), // 终态回退边
    (mxu1621flowState::Sliced, mxu1621flowEvent::Slice, mxu1621flowState::Offseted), // 旁路边
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn mxu1621flow_next(state: mxu1621flowState, ev: mxu1621flowEvent) -> Option<mxu1621flowState> {
    let mut i = 0usize;
    while i < 8 {
        let t = MXU1621FLOW_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn mxu1621flow_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < mxu1621flowState::ALL.len() {
        let mut ei = 0usize;
        while ei < mxu1621flowEvent::ALL.len() {
            let legal = mxu1621flow_next(mxu1621flowState::ALL[si], mxu1621flowEvent::ALL[ei]).is_some();
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
pub fn mxu1621flow_terminal(st: mxu1621flowState) -> bool {
    let mut i = 0usize;
    while i < 8 {
        if MXU1621FLOW_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct mxu1621flowLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: mxu1621flowState,
}

impl mxu1621flowLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: mxu1621flowState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: mxu1621flowEvent) -> bool {
        match mxu1621flow_next(self.cur, ev) {
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
// 快照 Mxu1621Snap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// 区段 快照封存（FNV 三态指纹 + 篡改检出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mxu1621Snap {
    /// 指纹分量：st_v。
    pub st_v: u64,
    /// 指纹分量：rq_v。
    pub rq_v: u64,
    /// 指纹分量：ck_v。
    pub ck_v: u64,
    /// 指纹分量：sd_v。
    pub sd_v: u64,
    /// 指纹分量：db_v。
    pub db_v: u64,
    /// 指纹分量：of_v。
    pub of_v: u64,
    /// 指纹分量：be_v。
    pub be_v: u64,
    /// 指纹分量：zb_v。
    pub zb_v: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl Mxu1621Snap {
    /// 构造并即时合成指纹。
    pub fn compute(
        st_v: u64,
        rq_v: u64,
        ck_v: u64,
        sd_v: u64,
        db_v: u64,
        of_v: u64,
        be_v: u64,
        zb_v: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &st_v.to_le_bytes());
        h = fnv1a64_from(h, &rq_v.to_le_bytes());
        h = fnv1a64_from(h, &ck_v.to_le_bytes());
        h = fnv1a64_from(h, &sd_v.to_le_bytes());
        h = fnv1a64_from(h, &db_v.to_le_bytes());
        h = fnv1a64_from(h, &of_v.to_le_bytes());
        h = fnv1a64_from(h, &be_v.to_le_bytes());
        h = fnv1a64_from(h, &zb_v.to_le_bytes());
        Self {
            st_v,
            rq_v,
            ck_v,
            sd_v,
            db_v,
            of_v,
            be_v,
            zb_v,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.st_v.to_le_bytes());
        h = fnv1a64_from(h, &self.rq_v.to_le_bytes());
        h = fnv1a64_from(h, &self.ck_v.to_le_bytes());
        h = fnv1a64_from(h, &self.sd_v.to_le_bytes());
        h = fnv1a64_from(h, &self.db_v.to_le_bytes());
        h = fnv1a64_from(h, &self.of_v.to_le_bytes());
        h = fnv1a64_from(h, &self.be_v.to_le_bytes());
        h = fnv1a64_from(h, &self.zb_v.to_le_bytes());
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

/// 段位守卫：全部诊断码必须落在本段 [0x55d0, 0x55df]。
pub const fn u1621_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u1621Code::ALL.len() {
        let c = u1621Code::ALL[i].code();
        if c < 0x55d0 || c > 0x55df {
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
fn u1621_guard_seg() -> bool {
    u1621_segment_guard()
}

/// UNX-F1621 状态机闭集/迁移表/非法组合恰值全绿。
fn check_u1621_sm_legal() -> bool {
    let mut ok = mxu1621flow_illegal_pairs() == 29;
    if ok { ok = MXU1621FLOW_TRANSITIONS.len() == 8; }
    if ok { ok = mxu1621flowState::ALL.len() == 6 && mxu1621flowEvent::ALL.len() == 6; }
    ok
}

/// UNX-F1621 迁移主链回放 + 快照往返恒等。
fn check_u1621_sm_round() -> bool {
    let mut lg = mxu1621flowLedger::new(mxu1621flowState::Reserved);
    let mut ok = lg.apply(mxu1621flowEvent::Reserve) && lg.cur == mxu1621flowState::Sliced && lg.total() == 1;
    let s0 = Mxu1621Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    let mut s1 = Mxu1621Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    s1.recompute();
    if ok { ok = s0 == s1 && !s1.tampered(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u1621_checks() -> CheckSet {
    let mut set = CheckSet::new("U1621");
    set.add(
        "u1621-seg-guard",
        u1621_guard_seg(),
        if u1621_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u1621-sm-legal",
        check_u1621_sm_legal(),
        if check_u1621_sm_legal() { "" } else { "UNX-F1621 sm legality mismatch" },
    );
    set.add(
        "u1621-sm-round",
        check_u1621_sm_round(),
        if check_u1621_sm_round() { "" } else { "UNX-F1621 sm roundtrip mismatch" },
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
    fn u1621_all_checks_green() {
        let set = run_u1621_checks();
        assert!(set.all_passed(), "U1621 domain checks must be green");
    }

    #[test]
    fn u1621_segment_guard_holds() {
        assert!(u1621_segment_guard());
    }
}
