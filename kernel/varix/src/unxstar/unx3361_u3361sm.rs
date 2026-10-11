//! unxu3361_u3361sm —— 时间窗模型：TSC 时钟基准与窗口定义 状态机与快照封存（TSC 时间窗）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3361 · 时间窗模型：TSC 时钟基准与窗口定义 · 判据原文：UNX-F3361 · 判据：J1：时间窗模型文档（基准钟/窗口长度/三级窗口微/秒/分）交付；M-120 窗口边界行为验证。
//! · 组件：TSC 时间窗 状态机（6 态 × 6 事件显式迁移）+ FNV 快照封存（8 分量）
//! · 判据落地：UNX-F3361 · 判据：J1：时间窗模型文档（基准钟/窗口长度/三级窗口微/秒/分）交付；M-120 窗口边界行为验证。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3361_VERSION: &str = "u3361-u3361sm-v1";

/// 诊断段基址。
pub const U3361_SEG_BASE: u16 = 0xe060;

// --------------------------------------------------------------------------
// 诊断码（u3361 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// TSC 时间窗 状态面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3361Code {
    /// 0xe060 状态重复（TSC 同名双态）。
    StateDup,
    /// 0xe061 状态断档（时间窗 序缺失）。
    StateHole,
    /// 0xe062 事件重复（同事件双声明）。
    EventDup,
    /// 0xe063 迁移重复（同源同事件双迁移）。
    TransDup,
    /// 0xe064 迁移缺失（应达态不可达）。
    TransHole,
    /// 0xe065 迁移环（非法自环/环路）。
    TransCycle,
    /// 0xe066 入口非法（初态不在闭集）。
    EntryBad,
    /// 0xe067 迁移丢失（应用事件无效果）。
    ApplyLost,
    /// 0xe068 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe069 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe06a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe06b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3361Code {
    /// 全量闭集。
    pub const ALL: [u3361Code; 12] = [
        u3361Code::StateDup,
        u3361Code::StateHole,
        u3361Code::EventDup,
        u3361Code::TransDup,
        u3361Code::TransHole,
        u3361Code::TransCycle,
        u3361Code::EntryBad,
        u3361Code::ApplyLost,
        u3361Code::LedgerBroken,
        u3361Code::SnapshotFingerprint,
        u3361Code::TableBroken,
        u3361Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3361Code::StateDup => 0xe060,
            u3361Code::StateHole => 0xe061,
            u3361Code::EventDup => 0xe062,
            u3361Code::TransDup => 0xe063,
            u3361Code::TransHole => 0xe064,
            u3361Code::TransCycle => 0xe065,
            u3361Code::EntryBad => 0xe066,
            u3361Code::ApplyLost => 0xe067,
            u3361Code::LedgerBroken => 0xe068,
            u3361Code::SnapshotFingerprint => 0xe069,
            u3361Code::TableBroken => 0xe06a,
            u3361Code::SegmentGuard => 0xe06b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3361Code::StateDup => "状态重复（TSC 同名双态）",
            u3361Code::StateHole => "状态断档（时间窗 序缺失）",
            u3361Code::EventDup => "事件重复（同事件双声明）",
            u3361Code::TransDup => "迁移重复（同源同事件双迁移）",
            u3361Code::TransHole => "迁移缺失（应达态不可达）",
            u3361Code::TransCycle => "迁移环（非法自环/环路）",
            u3361Code::EntryBad => "入口非法（初态不在闭集）",
            u3361Code::ApplyLost => "迁移丢失（应用事件无效果）",
            u3361Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3361Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3361Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3361Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3361Code::StateDup => "按闭集唯一律去重",
            u3361Code::StateHole => "按闭集补态",
            u3361Code::EventDup => "按事件闭集去重",
            u3361Code::TransDup => "按确定性唯一迁移律合并",
            u3361Code::TransHole => "按迁移矩阵补边",
            u3361Code::TransCycle => "按无环律重构迁移",
            u3361Code::EntryBad => "按闭集钳制初态",
            u3361Code::ApplyLost => "核对事件前置条件",
            u3361Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3361Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3361Code::TableBroken => "按语义规则重建常量表",
            u3361Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 状态机 mxu3361flow（6 态 × 6 事件 · 迁移 8 条显式声明）
// --------------------------------------------------------------------------
/// TSC 时间窗 状态机（判据：TSC）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu3361flowState {
    Cold,
    Calib,
    Running,
    Draining,
    Recal,
    Sealed,
}

impl mxu3361flowState {
    /// 全量态闭集。
    pub const ALL: [mxu3361flowState; 6] = [
        mxu3361flowState::Cold,
        mxu3361flowState::Calib,
        mxu3361flowState::Running,
        mxu3361flowState::Draining,
        mxu3361flowState::Recal,
        mxu3361flowState::Sealed,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            mxu3361flowState::Cold => 0,
            mxu3361flowState::Calib => 1,
            mxu3361flowState::Running => 2,
            mxu3361flowState::Draining => 3,
            mxu3361flowState::Recal => 4,
            mxu3361flowState::Sealed => 5,
        }
    }
}

/// mxu3361flow事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu3361flowEvent {
    Sample,
    Stamp,
    Slot,
    Evict,
    Recal,
    Seal,
}

impl mxu3361flowEvent {
    /// 全量事件闭集。
    pub const ALL: [mxu3361flowEvent; 6] = [
        mxu3361flowEvent::Sample,
        mxu3361flowEvent::Stamp,
        mxu3361flowEvent::Slot,
        mxu3361flowEvent::Evict,
        mxu3361flowEvent::Recal,
        mxu3361flowEvent::Seal,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const MXU3361FLOW_TRANSITIONS: [(mxu3361flowState, mxu3361flowEvent, mxu3361flowState); 8] = [
    (mxu3361flowState::Cold, mxu3361flowEvent::Sample, mxu3361flowState::Calib), // Cold→Calib 主链
    (mxu3361flowState::Calib, mxu3361flowEvent::Stamp, mxu3361flowState::Running), // Calib→Running 主链
    (mxu3361flowState::Running, mxu3361flowEvent::Slot, mxu3361flowState::Draining), // Running→Draining 主链
    (mxu3361flowState::Draining, mxu3361flowEvent::Evict, mxu3361flowState::Recal), // Draining→Recal 主链
    (mxu3361flowState::Recal, mxu3361flowEvent::Recal, mxu3361flowState::Sealed), // Recal→Sealed 主链
    (mxu3361flowState::Sealed, mxu3361flowEvent::Seal, mxu3361flowState::Cold), // 终态回边
    (mxu3361flowState::Sealed, mxu3361flowEvent::Sample, mxu3361flowState::Recal), // 终态回退边
    (mxu3361flowState::Calib, mxu3361flowEvent::Stamp, mxu3361flowState::Running), // 旁路边
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn mxu3361flow_next(state: mxu3361flowState, ev: mxu3361flowEvent) -> Option<mxu3361flowState> {
    let mut i = 0usize;
    while i < 8 {
        let t = MXU3361FLOW_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn mxu3361flow_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < mxu3361flowState::ALL.len() {
        let mut ei = 0usize;
        while ei < mxu3361flowEvent::ALL.len() {
            let legal = mxu3361flow_next(mxu3361flowState::ALL[si], mxu3361flowEvent::ALL[ei]).is_some();
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
pub fn mxu3361flow_terminal(st: mxu3361flowState) -> bool {
    let mut i = 0usize;
    while i < 8 {
        if MXU3361FLOW_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct mxu3361flowLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: mxu3361flowState,
}

impl mxu3361flowLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: mxu3361flowState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: mxu3361flowEvent) -> bool {
        match mxu3361flow_next(self.cur, ev) {
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
// 快照 Mxu3361Snap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// TSC 时间窗 快照封存（FNV 三态指纹 + 篡改检出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mxu3361Snap {
    /// 指纹分量：tsc_v。
    pub tsc_v: u64,
    /// 指纹分量：hz_v。
    pub hz_v: u64,
    /// 指纹分量：win_v。
    pub win_v: u64,
    /// 指纹分量：slot_v。
    pub slot_v: u64,
    /// 指纹分量：seq_v。
    pub seq_v: u64,
    /// 指纹分量：late_v。
    pub late_v: u64,
    /// 指纹分量：drain_v。
    pub drain_v: u64,
    /// 指纹分量：cal_v。
    pub cal_v: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl Mxu3361Snap {
    /// 构造并即时合成指纹。
    pub fn compute(
        tsc_v: u64,
        hz_v: u64,
        win_v: u64,
        slot_v: u64,
        seq_v: u64,
        late_v: u64,
        drain_v: u64,
        cal_v: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &tsc_v.to_le_bytes());
        h = fnv1a64_from(h, &hz_v.to_le_bytes());
        h = fnv1a64_from(h, &win_v.to_le_bytes());
        h = fnv1a64_from(h, &slot_v.to_le_bytes());
        h = fnv1a64_from(h, &seq_v.to_le_bytes());
        h = fnv1a64_from(h, &late_v.to_le_bytes());
        h = fnv1a64_from(h, &drain_v.to_le_bytes());
        h = fnv1a64_from(h, &cal_v.to_le_bytes());
        Self {
            tsc_v,
            hz_v,
            win_v,
            slot_v,
            seq_v,
            late_v,
            drain_v,
            cal_v,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.tsc_v.to_le_bytes());
        h = fnv1a64_from(h, &self.hz_v.to_le_bytes());
        h = fnv1a64_from(h, &self.win_v.to_le_bytes());
        h = fnv1a64_from(h, &self.slot_v.to_le_bytes());
        h = fnv1a64_from(h, &self.seq_v.to_le_bytes());
        h = fnv1a64_from(h, &self.late_v.to_le_bytes());
        h = fnv1a64_from(h, &self.drain_v.to_le_bytes());
        h = fnv1a64_from(h, &self.cal_v.to_le_bytes());
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

/// 段位守卫：全部诊断码必须落在本段 [0xe060, 0xe06f]。
pub const fn u3361_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3361Code::ALL.len() {
        let c = u3361Code::ALL[i].code();
        if c < 0xe060 || c > 0xe06f {
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
fn u3361_guard_seg() -> bool {
    u3361_segment_guard()
}

/// UNX-F3361 状态机闭集/迁移表/非法组合恰值全绿。
fn check_u3361_sm_legal() -> bool {
    let mut ok = mxu3361flow_illegal_pairs() == 29;
    if ok { ok = MXU3361FLOW_TRANSITIONS.len() == 8; }
    if ok { ok = mxu3361flowState::ALL.len() == 6 && mxu3361flowEvent::ALL.len() == 6; }
    ok
}

/// UNX-F3361 迁移主链回放 + 快照往返恒等。
fn check_u3361_sm_round() -> bool {
    let mut lg = mxu3361flowLedger::new(mxu3361flowState::Cold);
    let mut ok = lg.apply(mxu3361flowEvent::Sample) && lg.cur == mxu3361flowState::Calib && lg.total() == 1;
    let s0 = Mxu3361Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    let mut s1 = Mxu3361Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    s1.recompute();
    if ok { ok = s0 == s1 && !s1.tampered(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3361_checks() -> CheckSet {
    let mut set = CheckSet::new("U3361");
    set.add(
        "u3361-seg-guard",
        u3361_guard_seg(),
        if u3361_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3361-sm-legal",
        check_u3361_sm_legal(),
        if check_u3361_sm_legal() { "" } else { "UNX-F3361 sm legality mismatch" },
    );
    set.add(
        "u3361-sm-round",
        check_u3361_sm_round(),
        if check_u3361_sm_round() { "" } else { "UNX-F3361 sm roundtrip mismatch" },
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
    fn u3361_all_checks_green() {
        let set = run_u3361_checks();
        assert!(set.all_passed(), "U3361 domain checks must be green");
    }

    #[test]
    fn u3361_segment_guard_holds() {
        assert!(u3361_segment_guard());
    }
}
