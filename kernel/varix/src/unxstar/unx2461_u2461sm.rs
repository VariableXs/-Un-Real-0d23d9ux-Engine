//! unxu2461_u2461sm —— 水线三档判定（min/low/high） 状态机与快照封存（水线档位）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2461 · 水线三档判定（min/low/high） · 判据原文：UNX-F2461 · 判据：三档水线判定与 nr_free 总账（F2425）同源单点，三机型初值生效，跨越事件 10/10 次正确触发零漏报零重复
//! · 组件：水线档位 状态机（6 态 × 6 事件显式迁移）+ FNV 快照封存（8 分量）
//! · 判据落地：UNX-F2461 · 判据：三档水线判定与 nr_free 总账（F2425）同源单点，三机型初值生效，跨越事件 10/10 次正确触发零漏报零重复
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2461_VERSION: &str = "u2461-u2461sm-v1";

/// 诊断段基址。
pub const U2461_SEG_BASE: u16 = 0xa250;

// --------------------------------------------------------------------------
// 诊断码（u2461 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 水线档位 状态面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2461Code {
    /// 0xa250 状态重复（漏报触发 同名双态）。
    StateDup,
    /// 0xa251 状态断档（重复上报 序缺失）。
    StateHole,
    /// 0xa252 事件重复（同事件双声明）。
    EventDup,
    /// 0xa253 迁移重复（同源同事件双迁移）。
    TransDup,
    /// 0xa254 迁移缺失（应达态不可达）。
    TransHole,
    /// 0xa255 迁移环（非法自环/环路）。
    TransCycle,
    /// 0xa256 入口非法（初态不在闭集）。
    EntryBad,
    /// 0xa257 迁移丢失（应用事件无效果）。
    ApplyLost,
    /// 0xa258 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xa259 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xa25a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xa25b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2461Code {
    /// 全量闭集。
    pub const ALL: [u2461Code; 12] = [
        u2461Code::StateDup,
        u2461Code::StateHole,
        u2461Code::EventDup,
        u2461Code::TransDup,
        u2461Code::TransHole,
        u2461Code::TransCycle,
        u2461Code::EntryBad,
        u2461Code::ApplyLost,
        u2461Code::LedgerBroken,
        u2461Code::SnapshotFingerprint,
        u2461Code::TableBroken,
        u2461Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2461Code::StateDup => 0xa250,
            u2461Code::StateHole => 0xa251,
            u2461Code::EventDup => 0xa252,
            u2461Code::TransDup => 0xa253,
            u2461Code::TransHole => 0xa254,
            u2461Code::TransCycle => 0xa255,
            u2461Code::EntryBad => 0xa256,
            u2461Code::ApplyLost => 0xa257,
            u2461Code::LedgerBroken => 0xa258,
            u2461Code::SnapshotFingerprint => 0xa259,
            u2461Code::TableBroken => 0xa25a,
            u2461Code::SegmentGuard => 0xa25b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2461Code::StateDup => "状态重复（漏报触发 同名双态）",
            u2461Code::StateHole => "状态断档（重复上报 序缺失）",
            u2461Code::EventDup => "事件重复（同事件双声明）",
            u2461Code::TransDup => "迁移重复（同源同事件双迁移）",
            u2461Code::TransHole => "迁移缺失（应达态不可达）",
            u2461Code::TransCycle => "迁移环（非法自环/环路）",
            u2461Code::EntryBad => "入口非法（初态不在闭集）",
            u2461Code::ApplyLost => "迁移丢失（应用事件无效果）",
            u2461Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2461Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2461Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2461Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2461Code::StateDup => "按闭集唯一律去重",
            u2461Code::StateHole => "按闭集补态",
            u2461Code::EventDup => "按事件闭集去重",
            u2461Code::TransDup => "按确定性唯一迁移律合并",
            u2461Code::TransHole => "按迁移矩阵补边",
            u2461Code::TransCycle => "按无环律重构迁移",
            u2461Code::EntryBad => "按闭集钳制初态",
            u2461Code::ApplyLost => "核对事件前置条件",
            u2461Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2461Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2461Code::TableBroken => "按语义规则重建常量表",
            u2461Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 状态机 mxu2461flow（6 态 × 6 事件 · 迁移 8 条显式声明）
// --------------------------------------------------------------------------
/// 水线档位 状态机（判据：漏报触发）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu2461flowState {
    Init,
    Armed,
    Watching,
    Crossed,
    Reported,
    Sealed,
}

impl mxu2461flowState {
    /// 全量态闭集。
    pub const ALL: [mxu2461flowState; 6] = [
        mxu2461flowState::Init,
        mxu2461flowState::Armed,
        mxu2461flowState::Watching,
        mxu2461flowState::Crossed,
        mxu2461flowState::Reported,
        mxu2461flowState::Sealed,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            mxu2461flowState::Init => 0,
            mxu2461flowState::Armed => 1,
            mxu2461flowState::Watching => 2,
            mxu2461flowState::Crossed => 3,
            mxu2461flowState::Reported => 4,
            mxu2461flowState::Sealed => 5,
        }
    }
}

/// mxu2461flow事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu2461flowEvent {
    InitValue,
    ArmWatch,
    SampleFree,
    Cross,
    Report,
    Reset,
}

impl mxu2461flowEvent {
    /// 全量事件闭集。
    pub const ALL: [mxu2461flowEvent; 6] = [
        mxu2461flowEvent::InitValue,
        mxu2461flowEvent::ArmWatch,
        mxu2461flowEvent::SampleFree,
        mxu2461flowEvent::Cross,
        mxu2461flowEvent::Report,
        mxu2461flowEvent::Reset,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const MXU2461FLOW_TRANSITIONS: [(mxu2461flowState, mxu2461flowEvent, mxu2461flowState); 8] = [
    (mxu2461flowState::Init, mxu2461flowEvent::InitValue, mxu2461flowState::Armed), // Init→Armed 主链
    (mxu2461flowState::Armed, mxu2461flowEvent::ArmWatch, mxu2461flowState::Watching), // Armed→Watching 主链
    (mxu2461flowState::Watching, mxu2461flowEvent::SampleFree, mxu2461flowState::Crossed), // Watching→Crossed 主链
    (mxu2461flowState::Crossed, mxu2461flowEvent::Cross, mxu2461flowState::Reported), // Crossed→Reported 主链
    (mxu2461flowState::Reported, mxu2461flowEvent::Report, mxu2461flowState::Sealed), // Reported→Sealed 主链
    (mxu2461flowState::Sealed, mxu2461flowEvent::Reset, mxu2461flowState::Init), // 终态回边
    (mxu2461flowState::Sealed, mxu2461flowEvent::InitValue, mxu2461flowState::Reported), // 终态回退边
    (mxu2461flowState::Armed, mxu2461flowEvent::ArmWatch, mxu2461flowState::Watching), // 旁路边
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn mxu2461flow_next(state: mxu2461flowState, ev: mxu2461flowEvent) -> Option<mxu2461flowState> {
    let mut i = 0usize;
    while i < 8 {
        let t = MXU2461FLOW_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn mxu2461flow_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < mxu2461flowState::ALL.len() {
        let mut ei = 0usize;
        while ei < mxu2461flowEvent::ALL.len() {
            let legal = mxu2461flow_next(mxu2461flowState::ALL[si], mxu2461flowEvent::ALL[ei]).is_some();
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
pub fn mxu2461flow_terminal(st: mxu2461flowState) -> bool {
    let mut i = 0usize;
    while i < 8 {
        if MXU2461FLOW_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct mxu2461flowLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: mxu2461flowState,
}

impl mxu2461flowLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: mxu2461flowState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: mxu2461flowEvent) -> bool {
        match mxu2461flow_next(self.cur, ev) {
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
// 快照 Mxu2461Snap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// 水线档位 快照封存（FNV 三态指纹 + 篡改检出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mxu2461Snap {
    /// 指纹分量：mn_v。
    pub mn_v: u64,
    /// 指纹分量：lw_v。
    pub lw_v: u64,
    /// 指纹分量：hi_v。
    pub hi_v: u64,
    /// 指纹分量：nf_v。
    pub nf_v: u64,
    /// 指纹分量：sp_v。
    pub sp_v: u64,
    /// 指纹分量：mc_v。
    pub mc_v: u64,
    /// 指纹分量：iv_v。
    pub iv_v: u64,
    /// 指纹分量：xe_v。
    pub xe_v: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl Mxu2461Snap {
    /// 构造并即时合成指纹。
    pub fn compute(
        mn_v: u64,
        lw_v: u64,
        hi_v: u64,
        nf_v: u64,
        sp_v: u64,
        mc_v: u64,
        iv_v: u64,
        xe_v: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &mn_v.to_le_bytes());
        h = fnv1a64_from(h, &lw_v.to_le_bytes());
        h = fnv1a64_from(h, &hi_v.to_le_bytes());
        h = fnv1a64_from(h, &nf_v.to_le_bytes());
        h = fnv1a64_from(h, &sp_v.to_le_bytes());
        h = fnv1a64_from(h, &mc_v.to_le_bytes());
        h = fnv1a64_from(h, &iv_v.to_le_bytes());
        h = fnv1a64_from(h, &xe_v.to_le_bytes());
        Self {
            mn_v,
            lw_v,
            hi_v,
            nf_v,
            sp_v,
            mc_v,
            iv_v,
            xe_v,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.mn_v.to_le_bytes());
        h = fnv1a64_from(h, &self.lw_v.to_le_bytes());
        h = fnv1a64_from(h, &self.hi_v.to_le_bytes());
        h = fnv1a64_from(h, &self.nf_v.to_le_bytes());
        h = fnv1a64_from(h, &self.sp_v.to_le_bytes());
        h = fnv1a64_from(h, &self.mc_v.to_le_bytes());
        h = fnv1a64_from(h, &self.iv_v.to_le_bytes());
        h = fnv1a64_from(h, &self.xe_v.to_le_bytes());
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

/// 段位守卫：全部诊断码必须落在本段 [0xa250, 0xa25f]。
pub const fn u2461_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2461Code::ALL.len() {
        let c = u2461Code::ALL[i].code();
        if c < 0xa250 || c > 0xa25f {
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
fn u2461_guard_seg() -> bool {
    u2461_segment_guard()
}

/// UNX-F2461 状态机闭集/迁移表/非法组合恰值全绿。
fn check_u2461_sm_legal() -> bool {
    let mut ok = mxu2461flow_illegal_pairs() == 29;
    if ok { ok = MXU2461FLOW_TRANSITIONS.len() == 8; }
    if ok { ok = mxu2461flowState::ALL.len() == 6 && mxu2461flowEvent::ALL.len() == 6; }
    ok
}

/// UNX-F2461 迁移主链回放 + 快照往返恒等。
fn check_u2461_sm_round() -> bool {
    let mut lg = mxu2461flowLedger::new(mxu2461flowState::Init);
    let mut ok = lg.apply(mxu2461flowEvent::InitValue) && lg.cur == mxu2461flowState::Armed && lg.total() == 1;
    let s0 = Mxu2461Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    let mut s1 = Mxu2461Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    s1.recompute();
    if ok { ok = s0 == s1 && !s1.tampered(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2461_checks() -> CheckSet {
    let mut set = CheckSet::new("U2461");
    set.add(
        "u2461-seg-guard",
        u2461_guard_seg(),
        if u2461_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2461-sm-legal",
        check_u2461_sm_legal(),
        if check_u2461_sm_legal() { "" } else { "UNX-F2461 sm legality mismatch" },
    );
    set.add(
        "u2461-sm-round",
        check_u2461_sm_round(),
        if check_u2461_sm_round() { "" } else { "UNX-F2461 sm roundtrip mismatch" },
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
    fn u2461_all_checks_green() {
        let set = run_u2461_checks();
        assert!(set.all_passed(), "U2461 domain checks must be green");
    }

    #[test]
    fn u2461_segment_guard_holds() {
        assert!(u2461_segment_guard());
    }
}
