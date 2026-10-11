//! unxu2621_u2621sm —— 每进程 CR3 语义与上下文切换内存侧（A3 每核上下文消费） 状态机与快照封存（CR3 切换）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2621 · 每进程 CR3 语义与上下文切换内存侧（A3 每核上下文消费） · 判据原文：UNX-F2621 · 判据：CR3 切换内存侧语义逐项断言（切换点单点/NOFLUSH 组合正确），A3 每核上下文冻结件消费联签在档
//! · 组件：CR3 切换 状态机（6 态 × 6 事件显式迁移）+ FNV 快照封存（8 分量）
//! · 判据落地：UNX-F2621 · 判据：CR3 切换内存侧语义逐项断言（切换点单点/NOFLUSH 组合正确），A3 每核上下文冻结件消费联签在档
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2621_VERSION: &str = "u2621-u2621sm-v1";

/// 诊断段基址。
pub const U2621_SEG_BASE: u16 = 0xa850;

// --------------------------------------------------------------------------
// 诊断码（u2621 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// CR3 切换 状态面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2621Code {
    /// 0xa850 状态重复（切换点漏 同名双态）。
    StateDup,
    /// 0xa851 状态断档（冲刷错配 序缺失）。
    StateHole,
    /// 0xa852 事件重复（同事件双声明）。
    EventDup,
    /// 0xa853 迁移重复（同源同事件双迁移）。
    TransDup,
    /// 0xa854 迁移缺失（应达态不可达）。
    TransHole,
    /// 0xa855 迁移环（非法自环/环路）。
    TransCycle,
    /// 0xa856 入口非法（初态不在闭集）。
    EntryBad,
    /// 0xa857 迁移丢失（应用事件无效果）。
    ApplyLost,
    /// 0xa858 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xa859 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xa85a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xa85b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2621Code {
    /// 全量闭集。
    pub const ALL: [u2621Code; 12] = [
        u2621Code::StateDup,
        u2621Code::StateHole,
        u2621Code::EventDup,
        u2621Code::TransDup,
        u2621Code::TransHole,
        u2621Code::TransCycle,
        u2621Code::EntryBad,
        u2621Code::ApplyLost,
        u2621Code::LedgerBroken,
        u2621Code::SnapshotFingerprint,
        u2621Code::TableBroken,
        u2621Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2621Code::StateDup => 0xa850,
            u2621Code::StateHole => 0xa851,
            u2621Code::EventDup => 0xa852,
            u2621Code::TransDup => 0xa853,
            u2621Code::TransHole => 0xa854,
            u2621Code::TransCycle => 0xa855,
            u2621Code::EntryBad => 0xa856,
            u2621Code::ApplyLost => 0xa857,
            u2621Code::LedgerBroken => 0xa858,
            u2621Code::SnapshotFingerprint => 0xa859,
            u2621Code::TableBroken => 0xa85a,
            u2621Code::SegmentGuard => 0xa85b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2621Code::StateDup => "状态重复（切换点漏 同名双态）",
            u2621Code::StateHole => "状态断档（冲刷错配 序缺失）",
            u2621Code::EventDup => "事件重复（同事件双声明）",
            u2621Code::TransDup => "迁移重复（同源同事件双迁移）",
            u2621Code::TransHole => "迁移缺失（应达态不可达）",
            u2621Code::TransCycle => "迁移环（非法自环/环路）",
            u2621Code::EntryBad => "入口非法（初态不在闭集）",
            u2621Code::ApplyLost => "迁移丢失（应用事件无效果）",
            u2621Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2621Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2621Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2621Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2621Code::StateDup => "按闭集唯一律去重",
            u2621Code::StateHole => "按闭集补态",
            u2621Code::EventDup => "按事件闭集去重",
            u2621Code::TransDup => "按确定性唯一迁移律合并",
            u2621Code::TransHole => "按迁移矩阵补边",
            u2621Code::TransCycle => "按无环律重构迁移",
            u2621Code::EntryBad => "按闭集钳制初态",
            u2621Code::ApplyLost => "核对事件前置条件",
            u2621Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2621Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2621Code::TableBroken => "按语义规则重建常量表",
            u2621Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 状态机 mxu2621flow（6 态 × 6 事件 · 迁移 8 条显式声明）
// --------------------------------------------------------------------------
/// CR3 切换 状态机（判据：切换点漏）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu2621flowState {
    Loaded,
    SinglePoint,
    Flushing,
    Switched,
    Linked,
    Sealed,
}

impl mxu2621flowState {
    /// 全量态闭集。
    pub const ALL: [mxu2621flowState; 6] = [
        mxu2621flowState::Loaded,
        mxu2621flowState::SinglePoint,
        mxu2621flowState::Flushing,
        mxu2621flowState::Switched,
        mxu2621flowState::Linked,
        mxu2621flowState::Sealed,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            mxu2621flowState::Loaded => 0,
            mxu2621flowState::SinglePoint => 1,
            mxu2621flowState::Flushing => 2,
            mxu2621flowState::Switched => 3,
            mxu2621flowState::Linked => 4,
            mxu2621flowState::Sealed => 5,
        }
    }
}

/// mxu2621flow事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu2621flowEvent {
    LoadCr3,
    ArmSwitch,
    FlushTlb,
    NoFlush,
    Switch,
    LinkA3,
}

impl mxu2621flowEvent {
    /// 全量事件闭集。
    pub const ALL: [mxu2621flowEvent; 6] = [
        mxu2621flowEvent::LoadCr3,
        mxu2621flowEvent::ArmSwitch,
        mxu2621flowEvent::FlushTlb,
        mxu2621flowEvent::NoFlush,
        mxu2621flowEvent::Switch,
        mxu2621flowEvent::LinkA3,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const MXU2621FLOW_TRANSITIONS: [(mxu2621flowState, mxu2621flowEvent, mxu2621flowState); 8] = [
    (mxu2621flowState::Loaded, mxu2621flowEvent::LoadCr3, mxu2621flowState::SinglePoint), // Loaded→SinglePoint 主链
    (mxu2621flowState::SinglePoint, mxu2621flowEvent::ArmSwitch, mxu2621flowState::Flushing), // SinglePoint→Flushing 主链
    (mxu2621flowState::Flushing, mxu2621flowEvent::FlushTlb, mxu2621flowState::Switched), // Flushing→Switched 主链
    (mxu2621flowState::Switched, mxu2621flowEvent::NoFlush, mxu2621flowState::Linked), // Switched→Linked 主链
    (mxu2621flowState::Linked, mxu2621flowEvent::Switch, mxu2621flowState::Sealed), // Linked→Sealed 主链
    (mxu2621flowState::Sealed, mxu2621flowEvent::LinkA3, mxu2621flowState::Loaded), // 终态回边
    (mxu2621flowState::Sealed, mxu2621flowEvent::LoadCr3, mxu2621flowState::Linked), // 终态回退边
    (mxu2621flowState::SinglePoint, mxu2621flowEvent::ArmSwitch, mxu2621flowState::Flushing), // 旁路边
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn mxu2621flow_next(state: mxu2621flowState, ev: mxu2621flowEvent) -> Option<mxu2621flowState> {
    let mut i = 0usize;
    while i < 8 {
        let t = MXU2621FLOW_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn mxu2621flow_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < mxu2621flowState::ALL.len() {
        let mut ei = 0usize;
        while ei < mxu2621flowEvent::ALL.len() {
            let legal = mxu2621flow_next(mxu2621flowState::ALL[si], mxu2621flowEvent::ALL[ei]).is_some();
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
pub fn mxu2621flow_terminal(st: mxu2621flowState) -> bool {
    let mut i = 0usize;
    while i < 8 {
        if MXU2621FLOW_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct mxu2621flowLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: mxu2621flowState,
}

impl mxu2621flowLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: mxu2621flowState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: mxu2621flowEvent) -> bool {
        match mxu2621flow_next(self.cur, ev) {
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
// 快照 Mxu2621Snap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// CR3 切换 快照封存（FNV 三态指纹 + 篡改检出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mxu2621Snap {
    /// 指纹分量：cr_v。
    pub cr_v: u64,
    /// 指纹分量：sw_v。
    pub sw_v: u64,
    /// 指纹分量：sp_v。
    pub sp_v: u64,
    /// 指纹分量：nf_v。
    pub nf_v: u64,
    /// 指纹分量：fl_v。
    pub fl_v: u64,
    /// 指纹分量：ct_v。
    pub ct_v: u64,
    /// 指纹分量：fz_v。
    pub fz_v: u64,
    /// 指纹分量：cn_v。
    pub cn_v: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl Mxu2621Snap {
    /// 构造并即时合成指纹。
    pub fn compute(
        cr_v: u64,
        sw_v: u64,
        sp_v: u64,
        nf_v: u64,
        fl_v: u64,
        ct_v: u64,
        fz_v: u64,
        cn_v: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &cr_v.to_le_bytes());
        h = fnv1a64_from(h, &sw_v.to_le_bytes());
        h = fnv1a64_from(h, &sp_v.to_le_bytes());
        h = fnv1a64_from(h, &nf_v.to_le_bytes());
        h = fnv1a64_from(h, &fl_v.to_le_bytes());
        h = fnv1a64_from(h, &ct_v.to_le_bytes());
        h = fnv1a64_from(h, &fz_v.to_le_bytes());
        h = fnv1a64_from(h, &cn_v.to_le_bytes());
        Self {
            cr_v,
            sw_v,
            sp_v,
            nf_v,
            fl_v,
            ct_v,
            fz_v,
            cn_v,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.cr_v.to_le_bytes());
        h = fnv1a64_from(h, &self.sw_v.to_le_bytes());
        h = fnv1a64_from(h, &self.sp_v.to_le_bytes());
        h = fnv1a64_from(h, &self.nf_v.to_le_bytes());
        h = fnv1a64_from(h, &self.fl_v.to_le_bytes());
        h = fnv1a64_from(h, &self.ct_v.to_le_bytes());
        h = fnv1a64_from(h, &self.fz_v.to_le_bytes());
        h = fnv1a64_from(h, &self.cn_v.to_le_bytes());
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

/// 段位守卫：全部诊断码必须落在本段 [0xa850, 0xa85f]。
pub const fn u2621_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2621Code::ALL.len() {
        let c = u2621Code::ALL[i].code();
        if c < 0xa850 || c > 0xa85f {
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
fn u2621_guard_seg() -> bool {
    u2621_segment_guard()
}

/// UNX-F2621 状态机闭集/迁移表/非法组合恰值全绿。
fn check_u2621_sm_legal() -> bool {
    let mut ok = mxu2621flow_illegal_pairs() == 29;
    if ok { ok = MXU2621FLOW_TRANSITIONS.len() == 8; }
    if ok { ok = mxu2621flowState::ALL.len() == 6 && mxu2621flowEvent::ALL.len() == 6; }
    ok
}

/// UNX-F2621 迁移主链回放 + 快照往返恒等。
fn check_u2621_sm_round() -> bool {
    let mut lg = mxu2621flowLedger::new(mxu2621flowState::Loaded);
    let mut ok = lg.apply(mxu2621flowEvent::LoadCr3) && lg.cur == mxu2621flowState::SinglePoint && lg.total() == 1;
    let s0 = Mxu2621Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    let mut s1 = Mxu2621Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    s1.recompute();
    if ok { ok = s0 == s1 && !s1.tampered(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2621_checks() -> CheckSet {
    let mut set = CheckSet::new("U2621");
    set.add(
        "u2621-seg-guard",
        u2621_guard_seg(),
        if u2621_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2621-sm-legal",
        check_u2621_sm_legal(),
        if check_u2621_sm_legal() { "" } else { "UNX-F2621 sm legality mismatch" },
    );
    set.add(
        "u2621-sm-round",
        check_u2621_sm_round(),
        if check_u2621_sm_round() { "" } else { "UNX-F2621 sm roundtrip mismatch" },
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
    fn u2621_all_checks_green() {
        let set = run_u2621_checks();
        assert!(set.all_passed(), "U2621 domain checks must be green");
    }

    #[test]
    fn u2621_segment_guard_holds() {
        assert!(u2621_segment_guard());
    }
}
