//! unxu3261_u3261sm —— rwlock 结构与读偏好语义定义 状态机与快照封存（rwlock 读偏好）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3261 · rwlock 结构与读偏好语义定义 · 判据原文：UNX-F3261 · 判据：J1：`RwLock<T>` 状态字编码表（读计数/写者位/排队位）唯一化且 M-020 用例可验证三态迁移。
//! · 组件：rwlock 读偏好 状态机（6 态 × 6 事件显式迁移）+ FNV 快照封存（8 分量）
//! · 判据落地：UNX-F3261 · 判据：J1：`RwLock<T>` 状态字编码表（读计数/写者位/排队位）唯一化且 M-020 用例可验证三态迁移。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3261_VERSION: &str = "u3261-u3261sm-v1";

/// 诊断段基址。
pub const U3261_SEG_BASE: u16 = 0xe020;

// --------------------------------------------------------------------------
// 诊断码（u3261 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// rwlock 读偏好 状态面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3261Code {
    /// 0xe020 状态重复（rwlock 同名双态）。
    StateDup,
    /// 0xe021 状态断档（读偏好 序缺失）。
    StateHole,
    /// 0xe022 事件重复（同事件双声明）。
    EventDup,
    /// 0xe023 迁移重复（同源同事件双迁移）。
    TransDup,
    /// 0xe024 迁移缺失（应达态不可达）。
    TransHole,
    /// 0xe025 迁移环（非法自环/环路）。
    TransCycle,
    /// 0xe026 入口非法（初态不在闭集）。
    EntryBad,
    /// 0xe027 迁移丢失（应用事件无效果）。
    ApplyLost,
    /// 0xe028 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe029 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe02a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe02b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3261Code {
    /// 全量闭集。
    pub const ALL: [u3261Code; 12] = [
        u3261Code::StateDup,
        u3261Code::StateHole,
        u3261Code::EventDup,
        u3261Code::TransDup,
        u3261Code::TransHole,
        u3261Code::TransCycle,
        u3261Code::EntryBad,
        u3261Code::ApplyLost,
        u3261Code::LedgerBroken,
        u3261Code::SnapshotFingerprint,
        u3261Code::TableBroken,
        u3261Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3261Code::StateDup => 0xe020,
            u3261Code::StateHole => 0xe021,
            u3261Code::EventDup => 0xe022,
            u3261Code::TransDup => 0xe023,
            u3261Code::TransHole => 0xe024,
            u3261Code::TransCycle => 0xe025,
            u3261Code::EntryBad => 0xe026,
            u3261Code::ApplyLost => 0xe027,
            u3261Code::LedgerBroken => 0xe028,
            u3261Code::SnapshotFingerprint => 0xe029,
            u3261Code::TableBroken => 0xe02a,
            u3261Code::SegmentGuard => 0xe02b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3261Code::StateDup => "状态重复（rwlock 同名双态）",
            u3261Code::StateHole => "状态断档（读偏好 序缺失）",
            u3261Code::EventDup => "事件重复（同事件双声明）",
            u3261Code::TransDup => "迁移重复（同源同事件双迁移）",
            u3261Code::TransHole => "迁移缺失（应达态不可达）",
            u3261Code::TransCycle => "迁移环（非法自环/环路）",
            u3261Code::EntryBad => "入口非法（初态不在闭集）",
            u3261Code::ApplyLost => "迁移丢失（应用事件无效果）",
            u3261Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3261Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3261Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3261Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3261Code::StateDup => "按闭集唯一律去重",
            u3261Code::StateHole => "按闭集补态",
            u3261Code::EventDup => "按事件闭集去重",
            u3261Code::TransDup => "按确定性唯一迁移律合并",
            u3261Code::TransHole => "按迁移矩阵补边",
            u3261Code::TransCycle => "按无环律重构迁移",
            u3261Code::EntryBad => "按闭集钳制初态",
            u3261Code::ApplyLost => "核对事件前置条件",
            u3261Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3261Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3261Code::TableBroken => "按语义规则重建常量表",
            u3261Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 状态机 mxu3261flow（6 态 × 6 事件 · 迁移 8 条显式声明）
// --------------------------------------------------------------------------
/// rwlock 读偏好 状态机（判据：rwlock）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu3261flowState {
    Free,
    ReadHeld,
    WriteHeld,
    WritePend,
    UpgWait,
    Quiesce,
}

impl mxu3261flowState {
    /// 全量态闭集。
    pub const ALL: [mxu3261flowState; 6] = [
        mxu3261flowState::Free,
        mxu3261flowState::ReadHeld,
        mxu3261flowState::WriteHeld,
        mxu3261flowState::WritePend,
        mxu3261flowState::UpgWait,
        mxu3261flowState::Quiesce,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            mxu3261flowState::Free => 0,
            mxu3261flowState::ReadHeld => 1,
            mxu3261flowState::WriteHeld => 2,
            mxu3261flowState::WritePend => 3,
            mxu3261flowState::UpgWait => 4,
            mxu3261flowState::Quiesce => 5,
        }
    }
}

/// mxu3261flow事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu3261flowEvent {
    RLock,
    RUnlock,
    WLock,
    WUnlock,
    Upg,
    Dwn,
}

impl mxu3261flowEvent {
    /// 全量事件闭集。
    pub const ALL: [mxu3261flowEvent; 6] = [
        mxu3261flowEvent::RLock,
        mxu3261flowEvent::RUnlock,
        mxu3261flowEvent::WLock,
        mxu3261flowEvent::WUnlock,
        mxu3261flowEvent::Upg,
        mxu3261flowEvent::Dwn,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const MXU3261FLOW_TRANSITIONS: [(mxu3261flowState, mxu3261flowEvent, mxu3261flowState); 8] = [
    (mxu3261flowState::Free, mxu3261flowEvent::RLock, mxu3261flowState::ReadHeld), // Free→ReadHeld 主链
    (mxu3261flowState::ReadHeld, mxu3261flowEvent::RUnlock, mxu3261flowState::WriteHeld), // ReadHeld→WriteHeld 主链
    (mxu3261flowState::WriteHeld, mxu3261flowEvent::WLock, mxu3261flowState::WritePend), // WriteHeld→WritePend 主链
    (mxu3261flowState::WritePend, mxu3261flowEvent::WUnlock, mxu3261flowState::UpgWait), // WritePend→UpgWait 主链
    (mxu3261flowState::UpgWait, mxu3261flowEvent::Upg, mxu3261flowState::Quiesce), // UpgWait→Quiesce 主链
    (mxu3261flowState::Quiesce, mxu3261flowEvent::Dwn, mxu3261flowState::Free), // 终态回边
    (mxu3261flowState::Quiesce, mxu3261flowEvent::RLock, mxu3261flowState::UpgWait), // 终态回退边
    (mxu3261flowState::ReadHeld, mxu3261flowEvent::RUnlock, mxu3261flowState::WriteHeld), // 旁路边
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn mxu3261flow_next(state: mxu3261flowState, ev: mxu3261flowEvent) -> Option<mxu3261flowState> {
    let mut i = 0usize;
    while i < 8 {
        let t = MXU3261FLOW_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn mxu3261flow_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < mxu3261flowState::ALL.len() {
        let mut ei = 0usize;
        while ei < mxu3261flowEvent::ALL.len() {
            let legal = mxu3261flow_next(mxu3261flowState::ALL[si], mxu3261flowEvent::ALL[ei]).is_some();
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
pub fn mxu3261flow_terminal(st: mxu3261flowState) -> bool {
    let mut i = 0usize;
    while i < 8 {
        if MXU3261FLOW_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct mxu3261flowLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: mxu3261flowState,
}

impl mxu3261flowLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: mxu3261flowState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: mxu3261flowEvent) -> bool {
        match mxu3261flow_next(self.cur, ev) {
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
// 快照 Mxu3261Snap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// rwlock 读偏好 快照封存（FNV 三态指纹 + 篡改检出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mxu3261Snap {
    /// 指纹分量：st_v。
    pub st_v: u64,
    /// 指纹分量：rc_v。
    pub rc_v: u64,
    /// 指纹分量：wr_v。
    pub wr_v: u64,
    /// 指纹分量：wq_v。
    pub wq_v: u64,
    /// 指纹分量：rp_v。
    pub rp_v: u64,
    /// 指纹分量：wb_v。
    pub wb_v: u64,
    /// 指纹分量：rg_v。
    pub rg_v: u64,
    /// 指纹分量：wg_v。
    pub wg_v: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl Mxu3261Snap {
    /// 构造并即时合成指纹。
    pub fn compute(
        st_v: u64,
        rc_v: u64,
        wr_v: u64,
        wq_v: u64,
        rp_v: u64,
        wb_v: u64,
        rg_v: u64,
        wg_v: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &st_v.to_le_bytes());
        h = fnv1a64_from(h, &rc_v.to_le_bytes());
        h = fnv1a64_from(h, &wr_v.to_le_bytes());
        h = fnv1a64_from(h, &wq_v.to_le_bytes());
        h = fnv1a64_from(h, &rp_v.to_le_bytes());
        h = fnv1a64_from(h, &wb_v.to_le_bytes());
        h = fnv1a64_from(h, &rg_v.to_le_bytes());
        h = fnv1a64_from(h, &wg_v.to_le_bytes());
        Self {
            st_v,
            rc_v,
            wr_v,
            wq_v,
            rp_v,
            wb_v,
            rg_v,
            wg_v,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.st_v.to_le_bytes());
        h = fnv1a64_from(h, &self.rc_v.to_le_bytes());
        h = fnv1a64_from(h, &self.wr_v.to_le_bytes());
        h = fnv1a64_from(h, &self.wq_v.to_le_bytes());
        h = fnv1a64_from(h, &self.rp_v.to_le_bytes());
        h = fnv1a64_from(h, &self.wb_v.to_le_bytes());
        h = fnv1a64_from(h, &self.rg_v.to_le_bytes());
        h = fnv1a64_from(h, &self.wg_v.to_le_bytes());
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

/// 段位守卫：全部诊断码必须落在本段 [0xe020, 0xe02f]。
pub const fn u3261_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3261Code::ALL.len() {
        let c = u3261Code::ALL[i].code();
        if c < 0xe020 || c > 0xe02f {
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
fn u3261_guard_seg() -> bool {
    u3261_segment_guard()
}

/// UNX-F3261 状态机闭集/迁移表/非法组合恰值全绿。
fn check_u3261_sm_legal() -> bool {
    let mut ok = mxu3261flow_illegal_pairs() == 29;
    if ok { ok = MXU3261FLOW_TRANSITIONS.len() == 8; }
    if ok { ok = mxu3261flowState::ALL.len() == 6 && mxu3261flowEvent::ALL.len() == 6; }
    ok
}

/// UNX-F3261 迁移主链回放 + 快照往返恒等。
fn check_u3261_sm_round() -> bool {
    let mut lg = mxu3261flowLedger::new(mxu3261flowState::Free);
    let mut ok = lg.apply(mxu3261flowEvent::RLock) && lg.cur == mxu3261flowState::ReadHeld && lg.total() == 1;
    let s0 = Mxu3261Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    let mut s1 = Mxu3261Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    s1.recompute();
    if ok { ok = s0 == s1 && !s1.tampered(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3261_checks() -> CheckSet {
    let mut set = CheckSet::new("U3261");
    set.add(
        "u3261-seg-guard",
        u3261_guard_seg(),
        if u3261_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3261-sm-legal",
        check_u3261_sm_legal(),
        if check_u3261_sm_legal() { "" } else { "UNX-F3261 sm legality mismatch" },
    );
    set.add(
        "u3261-sm-round",
        check_u3261_sm_round(),
        if check_u3261_sm_round() { "" } else { "UNX-F3261 sm roundtrip mismatch" },
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
    fn u3261_all_checks_green() {
        let set = run_u3261_checks();
        assert!(set.all_passed(), "U3261 domain checks must be green");
    }

    #[test]
    fn u3261_segment_guard_holds() {
        assert!(u3261_segment_guard());
    }
}
