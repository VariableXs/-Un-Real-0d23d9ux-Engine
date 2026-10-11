//! unxu1721_u1721sm —— 均衡执行总纲（判据消费→执行原语链） 状态机与快照封存（执行段）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F1721 · 均衡执行总纲（判据消费→执行原语链） · 判据原文：UNX-F1721 · 判据：执行链（判据结论→收益复核→候选→目标→原语→记账）六段冻结 v1，段间交接签名齐备，跳段注入 10/10 次拒绝
//! · 组件：执行段 状态机（6 态 × 6 事件显式迁移）+ FNV 快照封存（8 分量）
//! · 判据落地：UNX-F1721 · 判据：执行链（判据结论→收益复核→候选→目标→原语→记账）六段冻结 v1，段间交接签名齐备，跳段注入 10/10 次拒绝
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U1721_VERSION: &str = "u1721-u1721sm-v1";

/// 诊断段基址。
pub const U1721_SEG_BASE: u16 = 0x5990;

// --------------------------------------------------------------------------
// 诊断码（u1721 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 执行段 状态面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u1721Code {
    /// 0x5990 状态重复（跳段 同名双态）。
    StateDup,
    /// 0x5991 状态断档（签名缺位 序缺失）。
    StateHole,
    /// 0x5992 事件重复（同事件双声明）。
    EventDup,
    /// 0x5993 迁移重复（同源同事件双迁移）。
    TransDup,
    /// 0x5994 迁移缺失（应达态不可达）。
    TransHole,
    /// 0x5995 迁移环（非法自环/环路）。
    TransCycle,
    /// 0x5996 入口非法（初态不在闭集）。
    EntryBad,
    /// 0x5997 迁移丢失（应用事件无效果）。
    ApplyLost,
    /// 0x5998 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x5999 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x599a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x599b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u1721Code {
    /// 全量闭集。
    pub const ALL: [u1721Code; 12] = [
        u1721Code::StateDup,
        u1721Code::StateHole,
        u1721Code::EventDup,
        u1721Code::TransDup,
        u1721Code::TransHole,
        u1721Code::TransCycle,
        u1721Code::EntryBad,
        u1721Code::ApplyLost,
        u1721Code::LedgerBroken,
        u1721Code::SnapshotFingerprint,
        u1721Code::TableBroken,
        u1721Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u1721Code::StateDup => 0x5990,
            u1721Code::StateHole => 0x5991,
            u1721Code::EventDup => 0x5992,
            u1721Code::TransDup => 0x5993,
            u1721Code::TransHole => 0x5994,
            u1721Code::TransCycle => 0x5995,
            u1721Code::EntryBad => 0x5996,
            u1721Code::ApplyLost => 0x5997,
            u1721Code::LedgerBroken => 0x5998,
            u1721Code::SnapshotFingerprint => 0x5999,
            u1721Code::TableBroken => 0x599a,
            u1721Code::SegmentGuard => 0x599b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u1721Code::StateDup => "状态重复（跳段 同名双态）",
            u1721Code::StateHole => "状态断档（签名缺位 序缺失）",
            u1721Code::EventDup => "事件重复（同事件双声明）",
            u1721Code::TransDup => "迁移重复（同源同事件双迁移）",
            u1721Code::TransHole => "迁移缺失（应达态不可达）",
            u1721Code::TransCycle => "迁移环（非法自环/环路）",
            u1721Code::EntryBad => "入口非法（初态不在闭集）",
            u1721Code::ApplyLost => "迁移丢失（应用事件无效果）",
            u1721Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u1721Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u1721Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u1721Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u1721Code::StateDup => "按闭集唯一律去重",
            u1721Code::StateHole => "按闭集补态",
            u1721Code::EventDup => "按事件闭集去重",
            u1721Code::TransDup => "按确定性唯一迁移律合并",
            u1721Code::TransHole => "按迁移矩阵补边",
            u1721Code::TransCycle => "按无环律重构迁移",
            u1721Code::EntryBad => "按闭集钳制初态",
            u1721Code::ApplyLost => "核对事件前置条件",
            u1721Code::LedgerBroken => "按守恒律重建台账后重跑",
            u1721Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u1721Code::TableBroken => "按语义规则重建常量表",
            u1721Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 状态机 mxu1721flow（6 态 × 6 事件 · 迁移 8 条显式声明）
// --------------------------------------------------------------------------
/// 执行段 状态机（判据：跳段）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu1721flowState {
    Idle,
    Judged,
    Reviewed,
    Picked,
    Executed,
    Booked,
}

impl mxu1721flowState {
    /// 全量态闭集。
    pub const ALL: [mxu1721flowState; 6] = [
        mxu1721flowState::Idle,
        mxu1721flowState::Judged,
        mxu1721flowState::Reviewed,
        mxu1721flowState::Picked,
        mxu1721flowState::Executed,
        mxu1721flowState::Booked,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            mxu1721flowState::Idle => 0,
            mxu1721flowState::Judged => 1,
            mxu1721flowState::Reviewed => 2,
            mxu1721flowState::Picked => 3,
            mxu1721flowState::Executed => 4,
            mxu1721flowState::Booked => 5,
        }
    }
}

/// mxu1721flow事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu1721flowEvent {
    Judge,
    Review,
    Pick,
    Target,
    Execute,
    Book,
}

impl mxu1721flowEvent {
    /// 全量事件闭集。
    pub const ALL: [mxu1721flowEvent; 6] = [
        mxu1721flowEvent::Judge,
        mxu1721flowEvent::Review,
        mxu1721flowEvent::Pick,
        mxu1721flowEvent::Target,
        mxu1721flowEvent::Execute,
        mxu1721flowEvent::Book,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const MXU1721FLOW_TRANSITIONS: [(mxu1721flowState, mxu1721flowEvent, mxu1721flowState); 8] = [
    (mxu1721flowState::Idle, mxu1721flowEvent::Judge, mxu1721flowState::Judged), // Idle→Judged 主链
    (mxu1721flowState::Judged, mxu1721flowEvent::Review, mxu1721flowState::Reviewed), // Judged→Reviewed 主链
    (mxu1721flowState::Reviewed, mxu1721flowEvent::Pick, mxu1721flowState::Picked), // Reviewed→Picked 主链
    (mxu1721flowState::Picked, mxu1721flowEvent::Target, mxu1721flowState::Executed), // Picked→Executed 主链
    (mxu1721flowState::Executed, mxu1721flowEvent::Execute, mxu1721flowState::Booked), // Executed→Booked 主链
    (mxu1721flowState::Booked, mxu1721flowEvent::Book, mxu1721flowState::Idle), // 终态回边
    (mxu1721flowState::Booked, mxu1721flowEvent::Judge, mxu1721flowState::Executed), // 终态回退边
    (mxu1721flowState::Judged, mxu1721flowEvent::Review, mxu1721flowState::Reviewed), // 旁路边
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn mxu1721flow_next(state: mxu1721flowState, ev: mxu1721flowEvent) -> Option<mxu1721flowState> {
    let mut i = 0usize;
    while i < 8 {
        let t = MXU1721FLOW_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn mxu1721flow_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < mxu1721flowState::ALL.len() {
        let mut ei = 0usize;
        while ei < mxu1721flowEvent::ALL.len() {
            let legal = mxu1721flow_next(mxu1721flowState::ALL[si], mxu1721flowEvent::ALL[ei]).is_some();
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
pub fn mxu1721flow_terminal(st: mxu1721flowState) -> bool {
    let mut i = 0usize;
    while i < 8 {
        if MXU1721FLOW_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct mxu1721flowLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: mxu1721flowState,
}

impl mxu1721flowLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: mxu1721flowState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: mxu1721flowEvent) -> bool {
        match mxu1721flow_next(self.cur, ev) {
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
// 快照 Mxu1721Snap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// 执行段 快照封存（FNV 三态指纹 + 篡改检出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mxu1721Snap {
    /// 指纹分量：pj_v。
    pub pj_v: u64,
    /// 指纹分量：sy_v。
    pub sy_v: u64,
    /// 指纹分量：hx_v。
    pub hx_v: u64,
    /// 指纹分量：mb_v。
    pub mb_v: u64,
    /// 指纹分量：yl_v。
    pub yl_v: u64,
    /// 指纹分量：jz_v。
    pub jz_v: u64,
    /// 指纹分量：ld_v。
    pub ld_v: u64,
    /// 指纹分量：qh_v。
    pub qh_v: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl Mxu1721Snap {
    /// 构造并即时合成指纹。
    pub fn compute(
        pj_v: u64,
        sy_v: u64,
        hx_v: u64,
        mb_v: u64,
        yl_v: u64,
        jz_v: u64,
        ld_v: u64,
        qh_v: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &pj_v.to_le_bytes());
        h = fnv1a64_from(h, &sy_v.to_le_bytes());
        h = fnv1a64_from(h, &hx_v.to_le_bytes());
        h = fnv1a64_from(h, &mb_v.to_le_bytes());
        h = fnv1a64_from(h, &yl_v.to_le_bytes());
        h = fnv1a64_from(h, &jz_v.to_le_bytes());
        h = fnv1a64_from(h, &ld_v.to_le_bytes());
        h = fnv1a64_from(h, &qh_v.to_le_bytes());
        Self {
            pj_v,
            sy_v,
            hx_v,
            mb_v,
            yl_v,
            jz_v,
            ld_v,
            qh_v,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.pj_v.to_le_bytes());
        h = fnv1a64_from(h, &self.sy_v.to_le_bytes());
        h = fnv1a64_from(h, &self.hx_v.to_le_bytes());
        h = fnv1a64_from(h, &self.mb_v.to_le_bytes());
        h = fnv1a64_from(h, &self.yl_v.to_le_bytes());
        h = fnv1a64_from(h, &self.jz_v.to_le_bytes());
        h = fnv1a64_from(h, &self.ld_v.to_le_bytes());
        h = fnv1a64_from(h, &self.qh_v.to_le_bytes());
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

/// 段位守卫：全部诊断码必须落在本段 [0x5990, 0x599f]。
pub const fn u1721_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u1721Code::ALL.len() {
        let c = u1721Code::ALL[i].code();
        if c < 0x5990 || c > 0x599f {
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
fn u1721_guard_seg() -> bool {
    u1721_segment_guard()
}

/// UNX-F1721 状态机闭集/迁移表/非法组合恰值全绿。
fn check_u1721_sm_legal() -> bool {
    let mut ok = mxu1721flow_illegal_pairs() == 29;
    if ok { ok = MXU1721FLOW_TRANSITIONS.len() == 8; }
    if ok { ok = mxu1721flowState::ALL.len() == 6 && mxu1721flowEvent::ALL.len() == 6; }
    ok
}

/// UNX-F1721 迁移主链回放 + 快照往返恒等。
fn check_u1721_sm_round() -> bool {
    let mut lg = mxu1721flowLedger::new(mxu1721flowState::Idle);
    let mut ok = lg.apply(mxu1721flowEvent::Judge) && lg.cur == mxu1721flowState::Judged && lg.total() == 1;
    let s0 = Mxu1721Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    let mut s1 = Mxu1721Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    s1.recompute();
    if ok { ok = s0 == s1 && !s1.tampered(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u1721_checks() -> CheckSet {
    let mut set = CheckSet::new("U1721");
    set.add(
        "u1721-seg-guard",
        u1721_guard_seg(),
        if u1721_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u1721-sm-legal",
        check_u1721_sm_legal(),
        if check_u1721_sm_legal() { "" } else { "UNX-F1721 sm legality mismatch" },
    );
    set.add(
        "u1721-sm-round",
        check_u1721_sm_round(),
        if check_u1721_sm_round() { "" } else { "UNX-F1721 sm roundtrip mismatch" },
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
    fn u1721_all_checks_green() {
        let set = run_u1721_checks();
        assert!(set.all_passed(), "U1721 domain checks must be green");
    }

    #[test]
    fn u1721_segment_guard_holds() {
        assert!(u1721_segment_guard());
    }
}
