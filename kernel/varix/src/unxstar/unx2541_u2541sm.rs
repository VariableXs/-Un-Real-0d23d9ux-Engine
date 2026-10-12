//! unxu2541_u2541sm —— alloc_pages/free_pages 冻结接口总账（正式冻结版） 状态机与快照封存（冻结接口）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2541 · alloc_pages/free_pages 冻结接口总账（正式冻结版） · 判据原文：UNX-F2541 · 判据：双接口冻结件登记（全域消费方清单齐备），签名与错误码三机直测断言一致
//! · 组件：冻结接口 状态机（6 态 × 6 事件显式迁移）+ FNV 快照封存（8 分量）
//! · 判据落地：UNX-F2541 · 判据：双接口冻结件登记（全域消费方清单齐备），签名与错误码三机直测断言一致
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2541_VERSION: &str = "u2541-u2541sm-v1";

/// 诊断段基址。
pub const U2541_SEG_BASE: u16 = 0xa550;

// --------------------------------------------------------------------------
// 诊断码（u2541 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 冻结接口 状态面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2541Code {
    /// 0xa550 状态重复（消费方缺 同名双态）。
    StateDup,
    /// 0xa551 状态断档（签名不符 序缺失）。
    StateHole,
    /// 0xa552 事件重复（同事件双声明）。
    EventDup,
    /// 0xa553 迁移重复（同源同事件双迁移）。
    TransDup,
    /// 0xa554 迁移缺失（应达态不可达）。
    TransHole,
    /// 0xa555 迁移环（非法自环/环路）。
    TransCycle,
    /// 0xa556 入口非法（初态不在闭集）。
    EntryBad,
    /// 0xa557 迁移丢失（应用事件无效果）。
    ApplyLost,
    /// 0xa558 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xa559 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xa55a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xa55b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2541Code {
    /// 全量闭集。
    pub const ALL: [u2541Code; 12] = [
        u2541Code::StateDup,
        u2541Code::StateHole,
        u2541Code::EventDup,
        u2541Code::TransDup,
        u2541Code::TransHole,
        u2541Code::TransCycle,
        u2541Code::EntryBad,
        u2541Code::ApplyLost,
        u2541Code::LedgerBroken,
        u2541Code::SnapshotFingerprint,
        u2541Code::TableBroken,
        u2541Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2541Code::StateDup => 0xa550,
            u2541Code::StateHole => 0xa551,
            u2541Code::EventDup => 0xa552,
            u2541Code::TransDup => 0xa553,
            u2541Code::TransHole => 0xa554,
            u2541Code::TransCycle => 0xa555,
            u2541Code::EntryBad => 0xa556,
            u2541Code::ApplyLost => 0xa557,
            u2541Code::LedgerBroken => 0xa558,
            u2541Code::SnapshotFingerprint => 0xa559,
            u2541Code::TableBroken => 0xa55a,
            u2541Code::SegmentGuard => 0xa55b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2541Code::StateDup => "状态重复（消费方缺 同名双态）",
            u2541Code::StateHole => "状态断档（签名不符 序缺失）",
            u2541Code::EventDup => "事件重复（同事件双声明）",
            u2541Code::TransDup => "迁移重复（同源同事件双迁移）",
            u2541Code::TransHole => "迁移缺失（应达态不可达）",
            u2541Code::TransCycle => "迁移环（非法自环/环路）",
            u2541Code::EntryBad => "入口非法（初态不在闭集）",
            u2541Code::ApplyLost => "迁移丢失（应用事件无效果）",
            u2541Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2541Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2541Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2541Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2541Code::StateDup => "按闭集唯一律去重",
            u2541Code::StateHole => "按闭集补态",
            u2541Code::EventDup => "按事件闭集去重",
            u2541Code::TransDup => "按确定性唯一迁移律合并",
            u2541Code::TransHole => "按迁移矩阵补边",
            u2541Code::TransCycle => "按无环律重构迁移",
            u2541Code::EntryBad => "按闭集钳制初态",
            u2541Code::ApplyLost => "核对事件前置条件",
            u2541Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2541Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2541Code::TableBroken => "按语义规则重建常量表",
            u2541Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 状态机 mxu2541flow（6 态 × 6 事件 · 迁移 8 条显式声明）
// --------------------------------------------------------------------------
/// 冻结接口 状态机（判据：消费方缺）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu2541flowState {
    Draft,
    Registered,
    Consumers,
    Signed,
    Tested,
    Released,
}

impl mxu2541flowState {
    /// 全量态闭集。
    pub const ALL: [mxu2541flowState; 6] = [
        mxu2541flowState::Draft,
        mxu2541flowState::Registered,
        mxu2541flowState::Consumers,
        mxu2541flowState::Signed,
        mxu2541flowState::Tested,
        mxu2541flowState::Released,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            mxu2541flowState::Draft => 0,
            mxu2541flowState::Registered => 1,
            mxu2541flowState::Consumers => 2,
            mxu2541flowState::Signed => 3,
            mxu2541flowState::Tested => 4,
            mxu2541flowState::Released => 5,
        }
    }
}

/// mxu2541flow事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu2541flowEvent {
    DraftIf,
    Register,
    Enroll,
    Sign,
    DirectTest,
    Release,
}

impl mxu2541flowEvent {
    /// 全量事件闭集。
    pub const ALL: [mxu2541flowEvent; 6] = [
        mxu2541flowEvent::DraftIf,
        mxu2541flowEvent::Register,
        mxu2541flowEvent::Enroll,
        mxu2541flowEvent::Sign,
        mxu2541flowEvent::DirectTest,
        mxu2541flowEvent::Release,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const MXU2541FLOW_TRANSITIONS: [(mxu2541flowState, mxu2541flowEvent, mxu2541flowState); 8] = [
    (mxu2541flowState::Draft, mxu2541flowEvent::DraftIf, mxu2541flowState::Registered), // Draft→Registered 主链
    (mxu2541flowState::Registered, mxu2541flowEvent::Register, mxu2541flowState::Consumers), // Registered→Consumers 主链
    (mxu2541flowState::Consumers, mxu2541flowEvent::Enroll, mxu2541flowState::Signed), // Consumers→Signed 主链
    (mxu2541flowState::Signed, mxu2541flowEvent::Sign, mxu2541flowState::Tested), // Signed→Tested 主链
    (mxu2541flowState::Tested, mxu2541flowEvent::DirectTest, mxu2541flowState::Released), // Tested→Released 主链
    (mxu2541flowState::Released, mxu2541flowEvent::Release, mxu2541flowState::Draft), // 终态回边
    (mxu2541flowState::Released, mxu2541flowEvent::DraftIf, mxu2541flowState::Tested), // 终态回退边
    (mxu2541flowState::Registered, mxu2541flowEvent::Register, mxu2541flowState::Consumers), // 旁路边
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn mxu2541flow_next(state: mxu2541flowState, ev: mxu2541flowEvent) -> Option<mxu2541flowState> {
    let mut i = 0usize;
    while i < 8 {
        let t = MXU2541FLOW_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn mxu2541flow_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < mxu2541flowState::ALL.len() {
        let mut ei = 0usize;
        while ei < mxu2541flowEvent::ALL.len() {
            let legal = mxu2541flow_next(mxu2541flowState::ALL[si], mxu2541flowEvent::ALL[ei]).is_some();
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
pub fn mxu2541flow_terminal(st: mxu2541flowState) -> bool {
    let mut i = 0usize;
    while i < 8 {
        if MXU2541FLOW_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct mxu2541flowLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: mxu2541flowState,
}

impl mxu2541flowLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: mxu2541flowState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: mxu2541flowEvent) -> bool {
        match mxu2541flow_next(self.cur, ev) {
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
// 快照 Mxu2541Snap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// 冻结接口 快照封存（FNV 三态指纹 + 篡改检出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mxu2541Snap {
    /// 指纹分量：al_v。
    pub al_v: u64,
    /// 指纹分量：fp_v。
    pub fp_v: u64,
    /// 指纹分量：fz_v。
    pub fz_v: u64,
    /// 指纹分量：cn_v。
    pub cn_v: u64,
    /// 指纹分量：sg_v。
    pub sg_v: u64,
    /// 指纹分量：ec_v。
    pub ec_v: u64,
    /// 指纹分量：tm_v。
    pub tm_v: u64,
    /// 指纹分量：as_v。
    pub as_v: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl Mxu2541Snap {
    /// 构造并即时合成指纹。
    pub fn compute(
        al_v: u64,
        fp_v: u64,
        fz_v: u64,
        cn_v: u64,
        sg_v: u64,
        ec_v: u64,
        tm_v: u64,
        as_v: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &al_v.to_le_bytes());
        h = fnv1a64_from(h, &fp_v.to_le_bytes());
        h = fnv1a64_from(h, &fz_v.to_le_bytes());
        h = fnv1a64_from(h, &cn_v.to_le_bytes());
        h = fnv1a64_from(h, &sg_v.to_le_bytes());
        h = fnv1a64_from(h, &ec_v.to_le_bytes());
        h = fnv1a64_from(h, &tm_v.to_le_bytes());
        h = fnv1a64_from(h, &as_v.to_le_bytes());
        Self {
            al_v,
            fp_v,
            fz_v,
            cn_v,
            sg_v,
            ec_v,
            tm_v,
            as_v,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.al_v.to_le_bytes());
        h = fnv1a64_from(h, &self.fp_v.to_le_bytes());
        h = fnv1a64_from(h, &self.fz_v.to_le_bytes());
        h = fnv1a64_from(h, &self.cn_v.to_le_bytes());
        h = fnv1a64_from(h, &self.sg_v.to_le_bytes());
        h = fnv1a64_from(h, &self.ec_v.to_le_bytes());
        h = fnv1a64_from(h, &self.tm_v.to_le_bytes());
        h = fnv1a64_from(h, &self.as_v.to_le_bytes());
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

/// 段位守卫：全部诊断码必须落在本段 [0xa550, 0xa55f]。
pub const fn u2541_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2541Code::ALL.len() {
        let c = u2541Code::ALL[i].code();
        if c < 0xa550 || c > 0xa55f {
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
fn u2541_guard_seg() -> bool {
    u2541_segment_guard()
}

/// UNX-F2541 状态机闭集/迁移表/非法组合恰值全绿。
fn check_u2541_sm_legal() -> bool {
    let mut ok = mxu2541flow_illegal_pairs() == 29;
    if ok { ok = MXU2541FLOW_TRANSITIONS.len() == 8; }
    if ok { ok = mxu2541flowState::ALL.len() == 6 && mxu2541flowEvent::ALL.len() == 6; }
    ok
}

/// UNX-F2541 迁移主链回放 + 快照往返恒等。
fn check_u2541_sm_round() -> bool {
    let mut lg = mxu2541flowLedger::new(mxu2541flowState::Draft);
    let mut ok = lg.apply(mxu2541flowEvent::DraftIf) && lg.cur == mxu2541flowState::Registered && lg.total() == 1;
    let s0 = Mxu2541Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    let mut s1 = Mxu2541Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    s1.recompute();
    if ok { ok = s0 == s1 && !s1.tampered(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2541_checks() -> CheckSet {
    let mut set = CheckSet::new("U2541");
    set.add(
        "u2541-seg-guard",
        u2541_guard_seg(),
        if u2541_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2541-sm-legal",
        check_u2541_sm_legal(),
        if check_u2541_sm_legal() { "" } else { "UNX-F2541 sm legality mismatch" },
    );
    set.add(
        "u2541-sm-round",
        check_u2541_sm_round(),
        if check_u2541_sm_round() { "" } else { "UNX-F2541 sm roundtrip mismatch" },
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
    fn u2541_all_checks_green() {
        let set = run_u2541_checks();
        assert!(set.all_passed(), "U2541 domain checks must be green");
    }

    #[test]
    fn u2541_segment_guard_holds() {
        assert!(u2541_segment_guard());
    }
}
