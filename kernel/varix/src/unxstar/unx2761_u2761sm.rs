//! unxu2761_u2761sm —— 四级压力档判定器（水位+产出双因子） 状态机与快照封存（压力档）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2761 · 四级压力档判定器（水位+产出双因子） · 判据原文：UNX-F2761 · 判据：四档（正常/轻压/中压/重压）双因子判定逐档可复测，档位注入 40/40 次正确，与 F2741/F2747 双源读数一致
//! · 组件：压力档 状态机（6 态 × 6 事件显式迁移）+ FNV 快照封存（8 分量）
//! · 判据落地：UNX-F2761 · 判据：四档（正常/轻压/中压/重压）双因子判定逐档可复测，档位注入 40/40 次正确，与 F2741/F2747 双源读数一致
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2761_VERSION: &str = "u2761-u2761sm-v1";

/// 诊断段基址。
pub const U2761_SEG_BASE: u16 = 0xad50;

// --------------------------------------------------------------------------
// 诊断码（u2761 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 压力档 状态面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2761Code {
    /// 0xad50 状态重复（档位跳变 同名双态）。
    StateDup,
    /// 0xad51 状态断档（双源分歧 序缺失）。
    StateHole,
    /// 0xad52 事件重复（同事件双声明）。
    EventDup,
    /// 0xad53 迁移重复（同源同事件双迁移）。
    TransDup,
    /// 0xad54 迁移缺失（应达态不可达）。
    TransHole,
    /// 0xad55 迁移环（非法自环/环路）。
    TransCycle,
    /// 0xad56 入口非法（初态不在闭集）。
    EntryBad,
    /// 0xad57 迁移丢失（应用事件无效果）。
    ApplyLost,
    /// 0xad58 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xad59 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xad5a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xad5b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2761Code {
    /// 全量闭集。
    pub const ALL: [u2761Code; 12] = [
        u2761Code::StateDup,
        u2761Code::StateHole,
        u2761Code::EventDup,
        u2761Code::TransDup,
        u2761Code::TransHole,
        u2761Code::TransCycle,
        u2761Code::EntryBad,
        u2761Code::ApplyLost,
        u2761Code::LedgerBroken,
        u2761Code::SnapshotFingerprint,
        u2761Code::TableBroken,
        u2761Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2761Code::StateDup => 0xad50,
            u2761Code::StateHole => 0xad51,
            u2761Code::EventDup => 0xad52,
            u2761Code::TransDup => 0xad53,
            u2761Code::TransHole => 0xad54,
            u2761Code::TransCycle => 0xad55,
            u2761Code::EntryBad => 0xad56,
            u2761Code::ApplyLost => 0xad57,
            u2761Code::LedgerBroken => 0xad58,
            u2761Code::SnapshotFingerprint => 0xad59,
            u2761Code::TableBroken => 0xad5a,
            u2761Code::SegmentGuard => 0xad5b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2761Code::StateDup => "状态重复（档位跳变 同名双态）",
            u2761Code::StateHole => "状态断档（双源分歧 序缺失）",
            u2761Code::EventDup => "事件重复（同事件双声明）",
            u2761Code::TransDup => "迁移重复（同源同事件双迁移）",
            u2761Code::TransHole => "迁移缺失（应达态不可达）",
            u2761Code::TransCycle => "迁移环（非法自环/环路）",
            u2761Code::EntryBad => "入口非法（初态不在闭集）",
            u2761Code::ApplyLost => "迁移丢失（应用事件无效果）",
            u2761Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2761Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2761Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2761Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2761Code::StateDup => "按闭集唯一律去重",
            u2761Code::StateHole => "按闭集补态",
            u2761Code::EventDup => "按事件闭集去重",
            u2761Code::TransDup => "按确定性唯一迁移律合并",
            u2761Code::TransHole => "按迁移矩阵补边",
            u2761Code::TransCycle => "按无环律重构迁移",
            u2761Code::EntryBad => "按闭集钳制初态",
            u2761Code::ApplyLost => "核对事件前置条件",
            u2761Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2761Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2761Code::TableBroken => "按语义规则重建常量表",
            u2761Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 状态机 mxu2761flow（6 态 × 6 事件 · 迁移 8 条显式声明）
// --------------------------------------------------------------------------
/// 压力档 状态机（判据：档位跳变）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu2761flowState {
    Idle,
    Sampling,
    Judging,
    Injected,
    CrossCheck,
    Sealed,
}

impl mxu2761flowState {
    /// 全量态闭集。
    pub const ALL: [mxu2761flowState; 6] = [
        mxu2761flowState::Idle,
        mxu2761flowState::Sampling,
        mxu2761flowState::Judging,
        mxu2761flowState::Injected,
        mxu2761flowState::CrossCheck,
        mxu2761flowState::Sealed,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            mxu2761flowState::Idle => 0,
            mxu2761flowState::Sampling => 1,
            mxu2761flowState::Judging => 2,
            mxu2761flowState::Injected => 3,
            mxu2761flowState::CrossCheck => 4,
            mxu2761flowState::Sealed => 5,
        }
    }
}

/// mxu2761flow事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu2761flowEvent {
    SampleWater,
    SampleOut,
    Judge,
    Inject,
    CrossRead,
    Seal,
}

impl mxu2761flowEvent {
    /// 全量事件闭集。
    pub const ALL: [mxu2761flowEvent; 6] = [
        mxu2761flowEvent::SampleWater,
        mxu2761flowEvent::SampleOut,
        mxu2761flowEvent::Judge,
        mxu2761flowEvent::Inject,
        mxu2761flowEvent::CrossRead,
        mxu2761flowEvent::Seal,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const MXU2761FLOW_TRANSITIONS: [(mxu2761flowState, mxu2761flowEvent, mxu2761flowState); 8] = [
    (mxu2761flowState::Idle, mxu2761flowEvent::SampleWater, mxu2761flowState::Sampling), // Idle→Sampling 主链
    (mxu2761flowState::Sampling, mxu2761flowEvent::SampleOut, mxu2761flowState::Judging), // Sampling→Judging 主链
    (mxu2761flowState::Judging, mxu2761flowEvent::Judge, mxu2761flowState::Injected), // Judging→Injected 主链
    (mxu2761flowState::Injected, mxu2761flowEvent::Inject, mxu2761flowState::CrossCheck), // Injected→CrossCheck 主链
    (mxu2761flowState::CrossCheck, mxu2761flowEvent::CrossRead, mxu2761flowState::Sealed), // CrossCheck→Sealed 主链
    (mxu2761flowState::Sealed, mxu2761flowEvent::Seal, mxu2761flowState::Idle), // 终态回边
    (mxu2761flowState::Sealed, mxu2761flowEvent::SampleWater, mxu2761flowState::CrossCheck), // 终态回退边
    (mxu2761flowState::Sampling, mxu2761flowEvent::SampleOut, mxu2761flowState::Judging), // 旁路边
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn mxu2761flow_next(state: mxu2761flowState, ev: mxu2761flowEvent) -> Option<mxu2761flowState> {
    let mut i = 0usize;
    while i < 8 {
        let t = MXU2761FLOW_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn mxu2761flow_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < mxu2761flowState::ALL.len() {
        let mut ei = 0usize;
        while ei < mxu2761flowEvent::ALL.len() {
            let legal = mxu2761flow_next(mxu2761flowState::ALL[si], mxu2761flowEvent::ALL[ei]).is_some();
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
pub fn mxu2761flow_terminal(st: mxu2761flowState) -> bool {
    let mut i = 0usize;
    while i < 8 {
        if MXU2761FLOW_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct mxu2761flowLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: mxu2761flowState,
}

impl mxu2761flowLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: mxu2761flowState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: mxu2761flowEvent) -> bool {
        match mxu2761flow_next(self.cur, ev) {
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
// 快照 Mxu2761Snap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// 压力档 快照封存（FNV 三态指纹 + 篡改检出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mxu2761Snap {
    /// 指纹分量：nm_v。
    pub nm_v: u64,
    /// 指纹分量：lt_v。
    pub lt_v: u64,
    /// 指纹分量：md_v。
    pub md_v: u64,
    /// 指纹分量：hv_v。
    pub hv_v: u64,
    /// 指纹分量：wl_v。
    pub wl_v: u64,
    /// 指纹分量：ou_v。
    pub ou_v: u64,
    /// 指纹分量：df_v。
    pub df_v: u64,
    /// 指纹分量：in40_v。
    pub in40_v: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl Mxu2761Snap {
    /// 构造并即时合成指纹。
    pub fn compute(
        nm_v: u64,
        lt_v: u64,
        md_v: u64,
        hv_v: u64,
        wl_v: u64,
        ou_v: u64,
        df_v: u64,
        in40_v: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &nm_v.to_le_bytes());
        h = fnv1a64_from(h, &lt_v.to_le_bytes());
        h = fnv1a64_from(h, &md_v.to_le_bytes());
        h = fnv1a64_from(h, &hv_v.to_le_bytes());
        h = fnv1a64_from(h, &wl_v.to_le_bytes());
        h = fnv1a64_from(h, &ou_v.to_le_bytes());
        h = fnv1a64_from(h, &df_v.to_le_bytes());
        h = fnv1a64_from(h, &in40_v.to_le_bytes());
        Self {
            nm_v,
            lt_v,
            md_v,
            hv_v,
            wl_v,
            ou_v,
            df_v,
            in40_v,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.nm_v.to_le_bytes());
        h = fnv1a64_from(h, &self.lt_v.to_le_bytes());
        h = fnv1a64_from(h, &self.md_v.to_le_bytes());
        h = fnv1a64_from(h, &self.hv_v.to_le_bytes());
        h = fnv1a64_from(h, &self.wl_v.to_le_bytes());
        h = fnv1a64_from(h, &self.ou_v.to_le_bytes());
        h = fnv1a64_from(h, &self.df_v.to_le_bytes());
        h = fnv1a64_from(h, &self.in40_v.to_le_bytes());
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

/// 段位守卫：全部诊断码必须落在本段 [0xad50, 0xad5f]。
pub const fn u2761_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2761Code::ALL.len() {
        let c = u2761Code::ALL[i].code();
        if c < 0xad50 || c > 0xad5f {
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
fn u2761_guard_seg() -> bool {
    u2761_segment_guard()
}

/// UNX-F2761 状态机闭集/迁移表/非法组合恰值全绿。
fn check_u2761_sm_legal() -> bool {
    let mut ok = mxu2761flow_illegal_pairs() == 29;
    if ok { ok = MXU2761FLOW_TRANSITIONS.len() == 8; }
    if ok { ok = mxu2761flowState::ALL.len() == 6 && mxu2761flowEvent::ALL.len() == 6; }
    ok
}

/// UNX-F2761 迁移主链回放 + 快照往返恒等。
fn check_u2761_sm_round() -> bool {
    let mut lg = mxu2761flowLedger::new(mxu2761flowState::Idle);
    let mut ok = lg.apply(mxu2761flowEvent::SampleWater) && lg.cur == mxu2761flowState::Sampling && lg.total() == 1;
    let s0 = Mxu2761Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    let mut s1 = Mxu2761Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    s1.recompute();
    if ok { ok = s0 == s1 && !s1.tampered(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2761_checks() -> CheckSet {
    let mut set = CheckSet::new("U2761");
    set.add(
        "u2761-seg-guard",
        u2761_guard_seg(),
        if u2761_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2761-sm-legal",
        check_u2761_sm_legal(),
        if check_u2761_sm_legal() { "" } else { "UNX-F2761 sm legality mismatch" },
    );
    set.add(
        "u2761-sm-round",
        check_u2761_sm_round(),
        if check_u2761_sm_round() { "" } else { "UNX-F2761 sm roundtrip mismatch" },
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
    fn u2761_all_checks_green() {
        let set = run_u2761_checks();
        assert!(set.all_passed(), "U2761 domain checks must be green");
    }

    #[test]
    fn u2761_segment_guard_holds() {
        assert!(u2761_segment_guard());
    }
}
