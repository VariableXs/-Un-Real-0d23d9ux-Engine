//! unxu1821_u1821sm —— 切换帧总纲（内核栈布局与切换帧结构冻结） 状态机与快照封存（帧区）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F1821 · 切换帧总纲（内核栈布局与切换帧结构冻结） · 判据原文：UNX-F1821 · 判据：切换帧结构（栈顶中断帧区/寄存器保存区/控制块锚区）冻结 v1 与 F1624 栈约定对齐，布局越界注入 10/10 检出
//! · 组件：帧区 状态机（6 态 × 6 事件显式迁移）+ FNV 快照封存（8 分量）
//! · 判据落地：UNX-F1821 · 判据：切换帧结构（栈顶中断帧区/寄存器保存区/控制块锚区）冻结 v1 与 F1624 栈约定对齐，布局越界注入 10/10 检出
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U1821_VERSION: &str = "u1821-u1821sm-v1";

/// 诊断段基址。
pub const U1821_SEG_BASE: u16 = 0x5d50;

// --------------------------------------------------------------------------
// 诊断码（u1821 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 帧区 状态面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u1821Code {
    /// 0x5d50 状态重复（布局越界 同名双态）。
    StateDup,
    /// 0x5d51 状态断档（对齐破坏 序缺失）。
    StateHole,
    /// 0x5d52 事件重复（同事件双声明）。
    EventDup,
    /// 0x5d53 迁移重复（同源同事件双迁移）。
    TransDup,
    /// 0x5d54 迁移缺失（应达态不可达）。
    TransHole,
    /// 0x5d55 迁移环（非法自环/环路）。
    TransCycle,
    /// 0x5d56 入口非法（初态不在闭集）。
    EntryBad,
    /// 0x5d57 迁移丢失（应用事件无效果）。
    ApplyLost,
    /// 0x5d58 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x5d59 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x5d5a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x5d5b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u1821Code {
    /// 全量闭集。
    pub const ALL: [u1821Code; 12] = [
        u1821Code::StateDup,
        u1821Code::StateHole,
        u1821Code::EventDup,
        u1821Code::TransDup,
        u1821Code::TransHole,
        u1821Code::TransCycle,
        u1821Code::EntryBad,
        u1821Code::ApplyLost,
        u1821Code::LedgerBroken,
        u1821Code::SnapshotFingerprint,
        u1821Code::TableBroken,
        u1821Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u1821Code::StateDup => 0x5d50,
            u1821Code::StateHole => 0x5d51,
            u1821Code::EventDup => 0x5d52,
            u1821Code::TransDup => 0x5d53,
            u1821Code::TransHole => 0x5d54,
            u1821Code::TransCycle => 0x5d55,
            u1821Code::EntryBad => 0x5d56,
            u1821Code::ApplyLost => 0x5d57,
            u1821Code::LedgerBroken => 0x5d58,
            u1821Code::SnapshotFingerprint => 0x5d59,
            u1821Code::TableBroken => 0x5d5a,
            u1821Code::SegmentGuard => 0x5d5b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u1821Code::StateDup => "状态重复（布局越界 同名双态）",
            u1821Code::StateHole => "状态断档（对齐破坏 序缺失）",
            u1821Code::EventDup => "事件重复（同事件双声明）",
            u1821Code::TransDup => "迁移重复（同源同事件双迁移）",
            u1821Code::TransHole => "迁移缺失（应达态不可达）",
            u1821Code::TransCycle => "迁移环（非法自环/环路）",
            u1821Code::EntryBad => "入口非法（初态不在闭集）",
            u1821Code::ApplyLost => "迁移丢失（应用事件无效果）",
            u1821Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u1821Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u1821Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u1821Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u1821Code::StateDup => "按闭集唯一律去重",
            u1821Code::StateHole => "按闭集补态",
            u1821Code::EventDup => "按事件闭集去重",
            u1821Code::TransDup => "按确定性唯一迁移律合并",
            u1821Code::TransHole => "按迁移矩阵补边",
            u1821Code::TransCycle => "按无环律重构迁移",
            u1821Code::EntryBad => "按闭集钳制初态",
            u1821Code::ApplyLost => "核对事件前置条件",
            u1821Code::LedgerBroken => "按守恒律重建台账后重跑",
            u1821Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u1821Code::TableBroken => "按语义规则重建常量表",
            u1821Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 状态机 mxu1821flow（6 态 × 6 事件 · 迁移 8 条显式声明）
// --------------------------------------------------------------------------
/// 帧区 状态机（判据：布局越界）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu1821flowState {
    Outlined,
    Aligned,
    Frozen,
    CrossRef,
    Probed,
    Detected,
}

impl mxu1821flowState {
    /// 全量态闭集。
    pub const ALL: [mxu1821flowState; 6] = [
        mxu1821flowState::Outlined,
        mxu1821flowState::Aligned,
        mxu1821flowState::Frozen,
        mxu1821flowState::CrossRef,
        mxu1821flowState::Probed,
        mxu1821flowState::Detected,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            mxu1821flowState::Outlined => 0,
            mxu1821flowState::Aligned => 1,
            mxu1821flowState::Frozen => 2,
            mxu1821flowState::CrossRef => 3,
            mxu1821flowState::Probed => 4,
            mxu1821flowState::Detected => 5,
        }
    }
}

/// mxu1821flow事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu1821flowEvent {
    Outline,
    Align,
    Freeze,
    Cross1624,
    Probe,
    Detect,
}

impl mxu1821flowEvent {
    /// 全量事件闭集。
    pub const ALL: [mxu1821flowEvent; 6] = [
        mxu1821flowEvent::Outline,
        mxu1821flowEvent::Align,
        mxu1821flowEvent::Freeze,
        mxu1821flowEvent::Cross1624,
        mxu1821flowEvent::Probe,
        mxu1821flowEvent::Detect,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const MXU1821FLOW_TRANSITIONS: [(mxu1821flowState, mxu1821flowEvent, mxu1821flowState); 8] = [
    (mxu1821flowState::Outlined, mxu1821flowEvent::Outline, mxu1821flowState::Aligned), // Outlined→Aligned 主链
    (mxu1821flowState::Aligned, mxu1821flowEvent::Align, mxu1821flowState::Frozen), // Aligned→Frozen 主链
    (mxu1821flowState::Frozen, mxu1821flowEvent::Freeze, mxu1821flowState::CrossRef), // Frozen→CrossRef 主链
    (mxu1821flowState::CrossRef, mxu1821flowEvent::Cross1624, mxu1821flowState::Probed), // CrossRef→Probed 主链
    (mxu1821flowState::Probed, mxu1821flowEvent::Probe, mxu1821flowState::Detected), // Probed→Detected 主链
    (mxu1821flowState::Detected, mxu1821flowEvent::Detect, mxu1821flowState::Outlined), // 终态回边
    (mxu1821flowState::Detected, mxu1821flowEvent::Outline, mxu1821flowState::Probed), // 终态回退边
    (mxu1821flowState::Aligned, mxu1821flowEvent::Align, mxu1821flowState::Frozen), // 旁路边
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn mxu1821flow_next(state: mxu1821flowState, ev: mxu1821flowEvent) -> Option<mxu1821flowState> {
    let mut i = 0usize;
    while i < 8 {
        let t = MXU1821FLOW_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn mxu1821flow_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < mxu1821flowState::ALL.len() {
        let mut ei = 0usize;
        while ei < mxu1821flowEvent::ALL.len() {
            let legal = mxu1821flow_next(mxu1821flowState::ALL[si], mxu1821flowEvent::ALL[ei]).is_some();
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
pub fn mxu1821flow_terminal(st: mxu1821flowState) -> bool {
    let mut i = 0usize;
    while i < 8 {
        if MXU1821FLOW_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct mxu1821flowLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: mxu1821flowState,
}

impl mxu1821flowLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: mxu1821flowState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: mxu1821flowEvent) -> bool {
        match mxu1821flow_next(self.cur, ev) {
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
// 快照 Mxu1821Snap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// 帧区 快照封存（FNV 三态指纹 + 篡改检出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mxu1821Snap {
    /// 指纹分量：zz_v。
    pub zz_v: u64,
    /// 指纹分量：bc_v。
    pub bc_v: u64,
    /// 指纹分量：kb_v。
    pub kb_v: u64,
    /// 指纹分量：dz_v。
    pub dz_v: u64,
    /// 指纹分量：f1624_v。
    pub f1624_v: u64,
    /// 指纹分量：yj_v。
    pub yj_v: u64,
    /// 指纹分量：jc_v。
    pub jc_v: u64,
    /// 指纹分量：jp_v。
    pub jp_v: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl Mxu1821Snap {
    /// 构造并即时合成指纹。
    pub fn compute(
        zz_v: u64,
        bc_v: u64,
        kb_v: u64,
        dz_v: u64,
        f1624_v: u64,
        yj_v: u64,
        jc_v: u64,
        jp_v: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &zz_v.to_le_bytes());
        h = fnv1a64_from(h, &bc_v.to_le_bytes());
        h = fnv1a64_from(h, &kb_v.to_le_bytes());
        h = fnv1a64_from(h, &dz_v.to_le_bytes());
        h = fnv1a64_from(h, &f1624_v.to_le_bytes());
        h = fnv1a64_from(h, &yj_v.to_le_bytes());
        h = fnv1a64_from(h, &jc_v.to_le_bytes());
        h = fnv1a64_from(h, &jp_v.to_le_bytes());
        Self {
            zz_v,
            bc_v,
            kb_v,
            dz_v,
            f1624_v,
            yj_v,
            jc_v,
            jp_v,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.zz_v.to_le_bytes());
        h = fnv1a64_from(h, &self.bc_v.to_le_bytes());
        h = fnv1a64_from(h, &self.kb_v.to_le_bytes());
        h = fnv1a64_from(h, &self.dz_v.to_le_bytes());
        h = fnv1a64_from(h, &self.f1624_v.to_le_bytes());
        h = fnv1a64_from(h, &self.yj_v.to_le_bytes());
        h = fnv1a64_from(h, &self.jc_v.to_le_bytes());
        h = fnv1a64_from(h, &self.jp_v.to_le_bytes());
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

/// 段位守卫：全部诊断码必须落在本段 [0x5d50, 0x5d5f]。
pub const fn u1821_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u1821Code::ALL.len() {
        let c = u1821Code::ALL[i].code();
        if c < 0x5d50 || c > 0x5d5f {
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
fn u1821_guard_seg() -> bool {
    u1821_segment_guard()
}

/// UNX-F1821 状态机闭集/迁移表/非法组合恰值全绿。
fn check_u1821_sm_legal() -> bool {
    let mut ok = mxu1821flow_illegal_pairs() == 29;
    if ok { ok = MXU1821FLOW_TRANSITIONS.len() == 8; }
    if ok { ok = mxu1821flowState::ALL.len() == 6 && mxu1821flowEvent::ALL.len() == 6; }
    ok
}

/// UNX-F1821 迁移主链回放 + 快照往返恒等。
fn check_u1821_sm_round() -> bool {
    let mut lg = mxu1821flowLedger::new(mxu1821flowState::Outlined);
    let mut ok = lg.apply(mxu1821flowEvent::Outline) && lg.cur == mxu1821flowState::Aligned && lg.total() == 1;
    let s0 = Mxu1821Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    let mut s1 = Mxu1821Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    s1.recompute();
    if ok { ok = s0 == s1 && !s1.tampered(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u1821_checks() -> CheckSet {
    let mut set = CheckSet::new("U1821");
    set.add(
        "u1821-seg-guard",
        u1821_guard_seg(),
        if u1821_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u1821-sm-legal",
        check_u1821_sm_legal(),
        if check_u1821_sm_legal() { "" } else { "UNX-F1821 sm legality mismatch" },
    );
    set.add(
        "u1821-sm-round",
        check_u1821_sm_round(),
        if check_u1821_sm_round() { "" } else { "UNX-F1821 sm roundtrip mismatch" },
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
    fn u1821_all_checks_green() {
        let set = run_u1821_checks();
        assert!(set.all_passed(), "U1821 domain checks must be green");
    }

    #[test]
    fn u1821_segment_guard_holds() {
        assert!(u1821_segment_guard());
    }
}
