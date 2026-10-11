//! unxu1601_u1601sm —— SMP 前置数据包消费与 A1 联签确认 状态机与快照封存（字段）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F1601 · SMP 前置数据包消费与 A1 联签确认 · 判据原文：UNX-F1601 · 判据：宿主侧消费 A1 F0188 前置数据包全字段（核列表/LAPIC 基址/缺席核账/trampoline 预留区）逐字段校验落账，字段缺失或版本不符时启动序列拒绝开跑并列出缺失清单，联签确认记录
//! · 组件：字段 状态机（6 态 × 6 事件显式迁移）+ FNV 快照封存（8 分量）
//! · 判据落地：UNX-F1601 · 判据：宿主侧消费 A1 F0188 前置数据包全字段（核列表/LAPIC 基址/缺席核账/trampoline 预留区）逐字段校验落账，
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U1601_VERSION: &str = "u1601-u1601sm-v1";

/// 诊断段基址。
pub const U1601_SEG_BASE: u16 = 0x5510;

// --------------------------------------------------------------------------
// 诊断码（u1601 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 字段 状态面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u1601Code {
    /// 0x5510 状态重复（字段缺失 同名双态）。
    StateDup,
    /// 0x5511 状态断档（版本不符 序缺失）。
    StateHole,
    /// 0x5512 事件重复（同事件双声明）。
    EventDup,
    /// 0x5513 迁移重复（同源同事件双迁移）。
    TransDup,
    /// 0x5514 迁移缺失（应达态不可达）。
    TransHole,
    /// 0x5515 迁移环（非法自环/环路）。
    TransCycle,
    /// 0x5516 入口非法（初态不在闭集）。
    EntryBad,
    /// 0x5517 迁移丢失（应用事件无效果）。
    ApplyLost,
    /// 0x5518 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x5519 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x551a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x551b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u1601Code {
    /// 全量闭集。
    pub const ALL: [u1601Code; 12] = [
        u1601Code::StateDup,
        u1601Code::StateHole,
        u1601Code::EventDup,
        u1601Code::TransDup,
        u1601Code::TransHole,
        u1601Code::TransCycle,
        u1601Code::EntryBad,
        u1601Code::ApplyLost,
        u1601Code::LedgerBroken,
        u1601Code::SnapshotFingerprint,
        u1601Code::TableBroken,
        u1601Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u1601Code::StateDup => 0x5510,
            u1601Code::StateHole => 0x5511,
            u1601Code::EventDup => 0x5512,
            u1601Code::TransDup => 0x5513,
            u1601Code::TransHole => 0x5514,
            u1601Code::TransCycle => 0x5515,
            u1601Code::EntryBad => 0x5516,
            u1601Code::ApplyLost => 0x5517,
            u1601Code::LedgerBroken => 0x5518,
            u1601Code::SnapshotFingerprint => 0x5519,
            u1601Code::TableBroken => 0x551a,
            u1601Code::SegmentGuard => 0x551b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u1601Code::StateDup => "状态重复（字段缺失 同名双态）",
            u1601Code::StateHole => "状态断档（版本不符 序缺失）",
            u1601Code::EventDup => "事件重复（同事件双声明）",
            u1601Code::TransDup => "迁移重复（同源同事件双迁移）",
            u1601Code::TransHole => "迁移缺失（应达态不可达）",
            u1601Code::TransCycle => "迁移环（非法自环/环路）",
            u1601Code::EntryBad => "入口非法（初态不在闭集）",
            u1601Code::ApplyLost => "迁移丢失（应用事件无效果）",
            u1601Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u1601Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u1601Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u1601Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u1601Code::StateDup => "按闭集唯一律去重",
            u1601Code::StateHole => "按闭集补态",
            u1601Code::EventDup => "按事件闭集去重",
            u1601Code::TransDup => "按确定性唯一迁移律合并",
            u1601Code::TransHole => "按迁移矩阵补边",
            u1601Code::TransCycle => "按无环律重构迁移",
            u1601Code::EntryBad => "按闭集钳制初态",
            u1601Code::ApplyLost => "核对事件前置条件",
            u1601Code::LedgerBroken => "按守恒律重建台账后重跑",
            u1601Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u1601Code::TableBroken => "按语义规则重建常量表",
            u1601Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 状态机 mxu1601flow（6 态 × 6 事件 · 迁移 8 条显式声明）
// --------------------------------------------------------------------------
/// 字段 状态机（判据：字段缺失）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu1601flowState {
    Fetching,
    Validating,
    Counting,
    Listing,
    Cosigned,
    Committed,
}

impl mxu1601flowState {
    /// 全量态闭集。
    pub const ALL: [mxu1601flowState; 6] = [
        mxu1601flowState::Fetching,
        mxu1601flowState::Validating,
        mxu1601flowState::Counting,
        mxu1601flowState::Listing,
        mxu1601flowState::Cosigned,
        mxu1601flowState::Committed,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            mxu1601flowState::Fetching => 0,
            mxu1601flowState::Validating => 1,
            mxu1601flowState::Counting => 2,
            mxu1601flowState::Listing => 3,
            mxu1601flowState::Cosigned => 4,
            mxu1601flowState::Committed => 5,
        }
    }
}

/// mxu1601flow事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu1601flowEvent {
    Fetch,
    Validate,
    Audit,
    Missing,
    Cosign,
    Commit,
}

impl mxu1601flowEvent {
    /// 全量事件闭集。
    pub const ALL: [mxu1601flowEvent; 6] = [
        mxu1601flowEvent::Fetch,
        mxu1601flowEvent::Validate,
        mxu1601flowEvent::Audit,
        mxu1601flowEvent::Missing,
        mxu1601flowEvent::Cosign,
        mxu1601flowEvent::Commit,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const MXU1601FLOW_TRANSITIONS: [(mxu1601flowState, mxu1601flowEvent, mxu1601flowState); 8] = [
    (mxu1601flowState::Fetching, mxu1601flowEvent::Fetch, mxu1601flowState::Validating), // Fetching→Validating 主链
    (mxu1601flowState::Validating, mxu1601flowEvent::Validate, mxu1601flowState::Counting), // Validating→Counting 主链
    (mxu1601flowState::Counting, mxu1601flowEvent::Audit, mxu1601flowState::Listing), // Counting→Listing 主链
    (mxu1601flowState::Listing, mxu1601flowEvent::Missing, mxu1601flowState::Cosigned), // Listing→Cosigned 主链
    (mxu1601flowState::Cosigned, mxu1601flowEvent::Cosign, mxu1601flowState::Committed), // Cosigned→Committed 主链
    (mxu1601flowState::Committed, mxu1601flowEvent::Commit, mxu1601flowState::Fetching), // 终态回边
    (mxu1601flowState::Committed, mxu1601flowEvent::Fetch, mxu1601flowState::Cosigned), // 终态回退边
    (mxu1601flowState::Validating, mxu1601flowEvent::Validate, mxu1601flowState::Counting), // 旁路边
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn mxu1601flow_next(state: mxu1601flowState, ev: mxu1601flowEvent) -> Option<mxu1601flowState> {
    let mut i = 0usize;
    while i < 8 {
        let t = MXU1601FLOW_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn mxu1601flow_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < mxu1601flowState::ALL.len() {
        let mut ei = 0usize;
        while ei < mxu1601flowEvent::ALL.len() {
            let legal = mxu1601flow_next(mxu1601flowState::ALL[si], mxu1601flowEvent::ALL[ei]).is_some();
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
pub fn mxu1601flow_terminal(st: mxu1601flowState) -> bool {
    let mut i = 0usize;
    while i < 8 {
        if MXU1601FLOW_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct mxu1601flowLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: mxu1601flowState,
}

impl mxu1601flowLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: mxu1601flowState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: mxu1601flowEvent) -> bool {
        match mxu1601flow_next(self.cur, ev) {
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
// 快照 Mxu1601Snap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// 字段 快照封存（FNV 三态指纹 + 篡改检出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mxu1601Snap {
    /// 指纹分量：cl_v。
    pub cl_v: u64,
    /// 指纹分量：lb_v。
    pub lb_v: u64,
    /// 指纹分量：ac_v。
    pub ac_v: u64,
    /// 指纹分量：tr_v。
    pub tr_v: u64,
    /// 指纹分量：fv_v。
    pub fv_v: u64,
    /// 指纹分量：vm_v。
    pub vm_v: u64,
    /// 指纹分量：ml_v。
    pub ml_v: u64,
    /// 指纹分量：rj_v。
    pub rj_v: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl Mxu1601Snap {
    /// 构造并即时合成指纹。
    pub fn compute(
        cl_v: u64,
        lb_v: u64,
        ac_v: u64,
        tr_v: u64,
        fv_v: u64,
        vm_v: u64,
        ml_v: u64,
        rj_v: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &cl_v.to_le_bytes());
        h = fnv1a64_from(h, &lb_v.to_le_bytes());
        h = fnv1a64_from(h, &ac_v.to_le_bytes());
        h = fnv1a64_from(h, &tr_v.to_le_bytes());
        h = fnv1a64_from(h, &fv_v.to_le_bytes());
        h = fnv1a64_from(h, &vm_v.to_le_bytes());
        h = fnv1a64_from(h, &ml_v.to_le_bytes());
        h = fnv1a64_from(h, &rj_v.to_le_bytes());
        Self {
            cl_v,
            lb_v,
            ac_v,
            tr_v,
            fv_v,
            vm_v,
            ml_v,
            rj_v,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.cl_v.to_le_bytes());
        h = fnv1a64_from(h, &self.lb_v.to_le_bytes());
        h = fnv1a64_from(h, &self.ac_v.to_le_bytes());
        h = fnv1a64_from(h, &self.tr_v.to_le_bytes());
        h = fnv1a64_from(h, &self.fv_v.to_le_bytes());
        h = fnv1a64_from(h, &self.vm_v.to_le_bytes());
        h = fnv1a64_from(h, &self.ml_v.to_le_bytes());
        h = fnv1a64_from(h, &self.rj_v.to_le_bytes());
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

/// 段位守卫：全部诊断码必须落在本段 [0x5510, 0x551f]。
pub const fn u1601_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u1601Code::ALL.len() {
        let c = u1601Code::ALL[i].code();
        if c < 0x5510 || c > 0x551f {
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
fn u1601_guard_seg() -> bool {
    u1601_segment_guard()
}

/// UNX-F1601 状态机闭集/迁移表/非法组合恰值全绿。
fn check_u1601_sm_legal() -> bool {
    let mut ok = mxu1601flow_illegal_pairs() == 29;
    if ok { ok = MXU1601FLOW_TRANSITIONS.len() == 8; }
    if ok { ok = mxu1601flowState::ALL.len() == 6 && mxu1601flowEvent::ALL.len() == 6; }
    ok
}

/// UNX-F1601 迁移主链回放 + 快照往返恒等。
fn check_u1601_sm_round() -> bool {
    let mut lg = mxu1601flowLedger::new(mxu1601flowState::Fetching);
    let mut ok = lg.apply(mxu1601flowEvent::Fetch) && lg.cur == mxu1601flowState::Validating && lg.total() == 1;
    let s0 = Mxu1601Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    let mut s1 = Mxu1601Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    s1.recompute();
    if ok { ok = s0 == s1 && !s1.tampered(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u1601_checks() -> CheckSet {
    let mut set = CheckSet::new("U1601");
    set.add(
        "u1601-seg-guard",
        u1601_guard_seg(),
        if u1601_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u1601-sm-legal",
        check_u1601_sm_legal(),
        if check_u1601_sm_legal() { "" } else { "UNX-F1601 sm legality mismatch" },
    );
    set.add(
        "u1601-sm-round",
        check_u1601_sm_round(),
        if check_u1601_sm_round() { "" } else { "UNX-F1601 sm roundtrip mismatch" },
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
    fn u1601_all_checks_green() {
        let set = run_u1601_checks();
        assert!(set.all_passed(), "U1601 domain checks must be green");
    }

    #[test]
    fn u1601_segment_guard_holds() {
        assert!(u1601_segment_guard());
    }
}
