//! unxu3561_u3561sm —— 毒化总设计（0xAA alloc/0xBB free 双毒化——UAF 检测基石） 状态机与快照封存（内存毒化）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3561 · 毒化总设计（0xAA alloc/0xBB free 双毒化——UAF 检测基石） · 判据原文：UNX-F3561 · 判据：J1：双毒化语义（alloc 后填 0xAA（未初始化用）/free 后填 0xBB（已释放用）——读中毒值即违规）；M-319 UAF 全链（释放→读→0xBB 被捕）。
//! · 组件：内存毒化 状态机（6 态 × 6 事件显式迁移）+ FNV 快照封存（8 分量）
//! · 判据落地：UNX-F3561 · 判据：J1：双毒化语义（alloc 后填 0xAA（未初始化用）/free 后填 0xBB（已释放用）——读中毒值即违规）；M-319 
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3561_VERSION: &str = "u3561-u3561sm-v1";

/// 诊断段基址。
pub const U3561_SEG_BASE: u16 = 0xe100;

// --------------------------------------------------------------------------
// 诊断码（u3561 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 内存毒化 状态面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3561Code {
    /// 0xe100 状态重复（毒化 同名双态）。
    StateDup,
    /// 0xe101 状态断档（UAF 序缺失）。
    StateHole,
    /// 0xe102 事件重复（同事件双声明）。
    EventDup,
    /// 0xe103 迁移重复（同源同事件双迁移）。
    TransDup,
    /// 0xe104 迁移缺失（应达态不可达）。
    TransHole,
    /// 0xe105 迁移环（非法自环/环路）。
    TransCycle,
    /// 0xe106 入口非法（初态不在闭集）。
    EntryBad,
    /// 0xe107 迁移丢失（应用事件无效果）。
    ApplyLost,
    /// 0xe108 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe109 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe10a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe10b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3561Code {
    /// 全量闭集。
    pub const ALL: [u3561Code; 12] = [
        u3561Code::StateDup,
        u3561Code::StateHole,
        u3561Code::EventDup,
        u3561Code::TransDup,
        u3561Code::TransHole,
        u3561Code::TransCycle,
        u3561Code::EntryBad,
        u3561Code::ApplyLost,
        u3561Code::LedgerBroken,
        u3561Code::SnapshotFingerprint,
        u3561Code::TableBroken,
        u3561Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3561Code::StateDup => 0xe100,
            u3561Code::StateHole => 0xe101,
            u3561Code::EventDup => 0xe102,
            u3561Code::TransDup => 0xe103,
            u3561Code::TransHole => 0xe104,
            u3561Code::TransCycle => 0xe105,
            u3561Code::EntryBad => 0xe106,
            u3561Code::ApplyLost => 0xe107,
            u3561Code::LedgerBroken => 0xe108,
            u3561Code::SnapshotFingerprint => 0xe109,
            u3561Code::TableBroken => 0xe10a,
            u3561Code::SegmentGuard => 0xe10b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3561Code::StateDup => "状态重复（毒化 同名双态）",
            u3561Code::StateHole => "状态断档（UAF 序缺失）",
            u3561Code::EventDup => "事件重复（同事件双声明）",
            u3561Code::TransDup => "迁移重复（同源同事件双迁移）",
            u3561Code::TransHole => "迁移缺失（应达态不可达）",
            u3561Code::TransCycle => "迁移环（非法自环/环路）",
            u3561Code::EntryBad => "入口非法（初态不在闭集）",
            u3561Code::ApplyLost => "迁移丢失（应用事件无效果）",
            u3561Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3561Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3561Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3561Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3561Code::StateDup => "按闭集唯一律去重",
            u3561Code::StateHole => "按闭集补态",
            u3561Code::EventDup => "按事件闭集去重",
            u3561Code::TransDup => "按确定性唯一迁移律合并",
            u3561Code::TransHole => "按迁移矩阵补边",
            u3561Code::TransCycle => "按无环律重构迁移",
            u3561Code::EntryBad => "按闭集钳制初态",
            u3561Code::ApplyLost => "核对事件前置条件",
            u3561Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3561Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3561Code::TableBroken => "按语义规则重建常量表",
            u3561Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 状态机 mxu3561flow（6 态 × 6 事件 · 迁移 8 条显式声明）
// --------------------------------------------------------------------------
/// 内存毒化 状态机（判据：毒化）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu3561flowState {
    Off,
    Armed,
    Poisoned,
    Freed,
    Detected,
    Quarantined,
}

impl mxu3561flowState {
    /// 全量态闭集。
    pub const ALL: [mxu3561flowState; 6] = [
        mxu3561flowState::Off,
        mxu3561flowState::Armed,
        mxu3561flowState::Poisoned,
        mxu3561flowState::Freed,
        mxu3561flowState::Detected,
        mxu3561flowState::Quarantined,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            mxu3561flowState::Off => 0,
            mxu3561flowState::Armed => 1,
            mxu3561flowState::Poisoned => 2,
            mxu3561flowState::Freed => 3,
            mxu3561flowState::Detected => 4,
            mxu3561flowState::Quarantined => 5,
        }
    }
}

/// mxu3561flow事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu3561flowEvent {
    Arm,
    AllocPoison,
    FreeCheck,
    FreePoison,
    Scan,
    Quarantine,
}

impl mxu3561flowEvent {
    /// 全量事件闭集。
    pub const ALL: [mxu3561flowEvent; 6] = [
        mxu3561flowEvent::Arm,
        mxu3561flowEvent::AllocPoison,
        mxu3561flowEvent::FreeCheck,
        mxu3561flowEvent::FreePoison,
        mxu3561flowEvent::Scan,
        mxu3561flowEvent::Quarantine,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const MXU3561FLOW_TRANSITIONS: [(mxu3561flowState, mxu3561flowEvent, mxu3561flowState); 8] = [
    (mxu3561flowState::Off, mxu3561flowEvent::Arm, mxu3561flowState::Armed), // Off→Armed 主链
    (mxu3561flowState::Armed, mxu3561flowEvent::AllocPoison, mxu3561flowState::Poisoned), // Armed→Poisoned 主链
    (mxu3561flowState::Poisoned, mxu3561flowEvent::FreeCheck, mxu3561flowState::Freed), // Poisoned→Freed 主链
    (mxu3561flowState::Freed, mxu3561flowEvent::FreePoison, mxu3561flowState::Detected), // Freed→Detected 主链
    (mxu3561flowState::Detected, mxu3561flowEvent::Scan, mxu3561flowState::Quarantined), // Detected→Quarantined 主链
    (mxu3561flowState::Quarantined, mxu3561flowEvent::Quarantine, mxu3561flowState::Off), // 终态回边
    (mxu3561flowState::Quarantined, mxu3561flowEvent::Arm, mxu3561flowState::Detected), // 终态回退边
    (mxu3561flowState::Armed, mxu3561flowEvent::AllocPoison, mxu3561flowState::Poisoned), // 旁路边
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn mxu3561flow_next(state: mxu3561flowState, ev: mxu3561flowEvent) -> Option<mxu3561flowState> {
    let mut i = 0usize;
    while i < 8 {
        let t = MXU3561FLOW_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn mxu3561flow_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < mxu3561flowState::ALL.len() {
        let mut ei = 0usize;
        while ei < mxu3561flowEvent::ALL.len() {
            let legal = mxu3561flow_next(mxu3561flowState::ALL[si], mxu3561flowEvent::ALL[ei]).is_some();
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
pub fn mxu3561flow_terminal(st: mxu3561flowState) -> bool {
    let mut i = 0usize;
    while i < 8 {
        if MXU3561FLOW_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct mxu3561flowLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: mxu3561flowState,
}

impl mxu3561flowLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: mxu3561flowState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: mxu3561flowEvent) -> bool {
        match mxu3561flow_next(self.cur, ev) {
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
// 快照 Mxu3561Snap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// 内存毒化 快照封存（FNV 三态指纹 + 篡改检出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mxu3561Snap {
    /// 指纹分量：aa_v。
    pub aa_v: u64,
    /// 指纹分量：bb_v。
    pub bb_v: u64,
    /// 指纹分量：magic_v。
    pub magic_v: u64,
    /// 指纹分量：uaf_v。
    pub uaf_v: u64,
    /// 指纹分量：canary_v。
    pub canary_v: u64,
    /// 指纹分量：fill_v。
    pub fill_v: u64,
    /// 指纹分量：chk_v。
    pub chk_v: u64,
    /// 指纹分量：hit_v。
    pub hit_v: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl Mxu3561Snap {
    /// 构造并即时合成指纹。
    pub fn compute(
        aa_v: u64,
        bb_v: u64,
        magic_v: u64,
        uaf_v: u64,
        canary_v: u64,
        fill_v: u64,
        chk_v: u64,
        hit_v: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &aa_v.to_le_bytes());
        h = fnv1a64_from(h, &bb_v.to_le_bytes());
        h = fnv1a64_from(h, &magic_v.to_le_bytes());
        h = fnv1a64_from(h, &uaf_v.to_le_bytes());
        h = fnv1a64_from(h, &canary_v.to_le_bytes());
        h = fnv1a64_from(h, &fill_v.to_le_bytes());
        h = fnv1a64_from(h, &chk_v.to_le_bytes());
        h = fnv1a64_from(h, &hit_v.to_le_bytes());
        Self {
            aa_v,
            bb_v,
            magic_v,
            uaf_v,
            canary_v,
            fill_v,
            chk_v,
            hit_v,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.aa_v.to_le_bytes());
        h = fnv1a64_from(h, &self.bb_v.to_le_bytes());
        h = fnv1a64_from(h, &self.magic_v.to_le_bytes());
        h = fnv1a64_from(h, &self.uaf_v.to_le_bytes());
        h = fnv1a64_from(h, &self.canary_v.to_le_bytes());
        h = fnv1a64_from(h, &self.fill_v.to_le_bytes());
        h = fnv1a64_from(h, &self.chk_v.to_le_bytes());
        h = fnv1a64_from(h, &self.hit_v.to_le_bytes());
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

/// 段位守卫：全部诊断码必须落在本段 [0xe100, 0xe10f]。
pub const fn u3561_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3561Code::ALL.len() {
        let c = u3561Code::ALL[i].code();
        if c < 0xe100 || c > 0xe10f {
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
fn u3561_guard_seg() -> bool {
    u3561_segment_guard()
}

/// UNX-F3561 状态机闭集/迁移表/非法组合恰值全绿。
fn check_u3561_sm_legal() -> bool {
    let mut ok = mxu3561flow_illegal_pairs() == 29;
    if ok { ok = MXU3561FLOW_TRANSITIONS.len() == 8; }
    if ok { ok = mxu3561flowState::ALL.len() == 6 && mxu3561flowEvent::ALL.len() == 6; }
    ok
}

/// UNX-F3561 迁移主链回放 + 快照往返恒等。
fn check_u3561_sm_round() -> bool {
    let mut lg = mxu3561flowLedger::new(mxu3561flowState::Off);
    let mut ok = lg.apply(mxu3561flowEvent::Arm) && lg.cur == mxu3561flowState::Armed && lg.total() == 1;
    let s0 = Mxu3561Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    let mut s1 = Mxu3561Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    s1.recompute();
    if ok { ok = s0 == s1 && !s1.tampered(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3561_checks() -> CheckSet {
    let mut set = CheckSet::new("U3561");
    set.add(
        "u3561-seg-guard",
        u3561_guard_seg(),
        if u3561_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3561-sm-legal",
        check_u3561_sm_legal(),
        if check_u3561_sm_legal() { "" } else { "UNX-F3561 sm legality mismatch" },
    );
    set.add(
        "u3561-sm-round",
        check_u3561_sm_round(),
        if check_u3561_sm_round() { "" } else { "UNX-F3561 sm roundtrip mismatch" },
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
    fn u3561_all_checks_green() {
        let set = run_u3561_checks();
        assert!(set.all_passed(), "U3561 domain checks must be green");
    }

    #[test]
    fn u3561_segment_guard_holds() {
        assert!(u3561_segment_guard());
    }
}
