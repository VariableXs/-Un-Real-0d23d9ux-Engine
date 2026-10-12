//! unxu3501_u3501sm —— 跟踪点总设计（TP 注册表/编译期埋点/运行时开关） 状态机与快照封存（跟踪点体系）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3501 · 跟踪点总设计（TP 注册表/编译期埋点/运行时开关） · 判据原文：UNX-F3501 · 判据：J1：TP 语义冻结（埋点位置 × 负载类型 × 默认态三列矩阵）；M-259 TP 全链路（埋点→开关→负载→消费端可读）。<br>J2：与日志分工声明（文本流 vs 结构化事件流——F3506 协议对端，禁用态零开销）。
//! · 组件：跟踪点体系 状态机（6 态 × 6 事件显式迁移）+ FNV 快照封存（8 分量）
//! · 判据落地：UNX-F3501 · 判据：J1：TP 语义冻结（埋点位置 × 负载类型 × 默认态三列矩阵）；M-259 TP 全链路（埋点→开关→负载→消费端可读）。<b
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3501_VERSION: &str = "u3501-u3501sm-v1";

/// 诊断段基址。
pub const U3501_SEG_BASE: u16 = 0xe0d0;

// --------------------------------------------------------------------------
// 诊断码（u3501 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 跟踪点体系 状态面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3501Code {
    /// 0xe0d0 状态重复（跟踪点 同名双态）。
    StateDup,
    /// 0xe0d1 状态断档（埋点 序缺失）。
    StateHole,
    /// 0xe0d2 事件重复（同事件双声明）。
    EventDup,
    /// 0xe0d3 迁移重复（同源同事件双迁移）。
    TransDup,
    /// 0xe0d4 迁移缺失（应达态不可达）。
    TransHole,
    /// 0xe0d5 迁移环（非法自环/环路）。
    TransCycle,
    /// 0xe0d6 入口非法（初态不在闭集）。
    EntryBad,
    /// 0xe0d7 迁移丢失（应用事件无效果）。
    ApplyLost,
    /// 0xe0d8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe0d9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe0da 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe0db 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3501Code {
    /// 全量闭集。
    pub const ALL: [u3501Code; 12] = [
        u3501Code::StateDup,
        u3501Code::StateHole,
        u3501Code::EventDup,
        u3501Code::TransDup,
        u3501Code::TransHole,
        u3501Code::TransCycle,
        u3501Code::EntryBad,
        u3501Code::ApplyLost,
        u3501Code::LedgerBroken,
        u3501Code::SnapshotFingerprint,
        u3501Code::TableBroken,
        u3501Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3501Code::StateDup => 0xe0d0,
            u3501Code::StateHole => 0xe0d1,
            u3501Code::EventDup => 0xe0d2,
            u3501Code::TransDup => 0xe0d3,
            u3501Code::TransHole => 0xe0d4,
            u3501Code::TransCycle => 0xe0d5,
            u3501Code::EntryBad => 0xe0d6,
            u3501Code::ApplyLost => 0xe0d7,
            u3501Code::LedgerBroken => 0xe0d8,
            u3501Code::SnapshotFingerprint => 0xe0d9,
            u3501Code::TableBroken => 0xe0da,
            u3501Code::SegmentGuard => 0xe0db,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3501Code::StateDup => "状态重复（跟踪点 同名双态）",
            u3501Code::StateHole => "状态断档（埋点 序缺失）",
            u3501Code::EventDup => "事件重复（同事件双声明）",
            u3501Code::TransDup => "迁移重复（同源同事件双迁移）",
            u3501Code::TransHole => "迁移缺失（应达态不可达）",
            u3501Code::TransCycle => "迁移环（非法自环/环路）",
            u3501Code::EntryBad => "入口非法（初态不在闭集）",
            u3501Code::ApplyLost => "迁移丢失（应用事件无效果）",
            u3501Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3501Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3501Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3501Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3501Code::StateDup => "按闭集唯一律去重",
            u3501Code::StateHole => "按闭集补态",
            u3501Code::EventDup => "按事件闭集去重",
            u3501Code::TransDup => "按确定性唯一迁移律合并",
            u3501Code::TransHole => "按迁移矩阵补边",
            u3501Code::TransCycle => "按无环律重构迁移",
            u3501Code::EntryBad => "按闭集钳制初态",
            u3501Code::ApplyLost => "核对事件前置条件",
            u3501Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3501Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3501Code::TableBroken => "按语义规则重建常量表",
            u3501Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 状态机 mxu3501flow（6 态 × 6 事件 · 迁移 8 条显式声明）
// --------------------------------------------------------------------------
/// 跟踪点体系 状态机（判据：跟踪点）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu3501flowState {
    Reg,
    Armed,
    Firing,
    Sampled,
    Dumped,
    Muted,
}

impl mxu3501flowState {
    /// 全量态闭集。
    pub const ALL: [mxu3501flowState; 6] = [
        mxu3501flowState::Reg,
        mxu3501flowState::Armed,
        mxu3501flowState::Firing,
        mxu3501flowState::Sampled,
        mxu3501flowState::Dumped,
        mxu3501flowState::Muted,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            mxu3501flowState::Reg => 0,
            mxu3501flowState::Armed => 1,
            mxu3501flowState::Firing => 2,
            mxu3501flowState::Sampled => 3,
            mxu3501flowState::Dumped => 4,
            mxu3501flowState::Muted => 5,
        }
    }
}

/// mxu3501flow事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu3501flowEvent {
    Register,
    Arm,
    Fire,
    Sample,
    Overflow,
    Dump,
}

impl mxu3501flowEvent {
    /// 全量事件闭集。
    pub const ALL: [mxu3501flowEvent; 6] = [
        mxu3501flowEvent::Register,
        mxu3501flowEvent::Arm,
        mxu3501flowEvent::Fire,
        mxu3501flowEvent::Sample,
        mxu3501flowEvent::Overflow,
        mxu3501flowEvent::Dump,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const MXU3501FLOW_TRANSITIONS: [(mxu3501flowState, mxu3501flowEvent, mxu3501flowState); 8] = [
    (mxu3501flowState::Reg, mxu3501flowEvent::Register, mxu3501flowState::Armed), // Reg→Armed 主链
    (mxu3501flowState::Armed, mxu3501flowEvent::Arm, mxu3501flowState::Firing), // Armed→Firing 主链
    (mxu3501flowState::Firing, mxu3501flowEvent::Fire, mxu3501flowState::Sampled), // Firing→Sampled 主链
    (mxu3501flowState::Sampled, mxu3501flowEvent::Sample, mxu3501flowState::Dumped), // Sampled→Dumped 主链
    (mxu3501flowState::Dumped, mxu3501flowEvent::Overflow, mxu3501flowState::Muted), // Dumped→Muted 主链
    (mxu3501flowState::Muted, mxu3501flowEvent::Dump, mxu3501flowState::Reg), // 终态回边
    (mxu3501flowState::Muted, mxu3501flowEvent::Register, mxu3501flowState::Dumped), // 终态回退边
    (mxu3501flowState::Armed, mxu3501flowEvent::Arm, mxu3501flowState::Firing), // 旁路边
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn mxu3501flow_next(state: mxu3501flowState, ev: mxu3501flowEvent) -> Option<mxu3501flowState> {
    let mut i = 0usize;
    while i < 8 {
        let t = MXU3501FLOW_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn mxu3501flow_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < mxu3501flowState::ALL.len() {
        let mut ei = 0usize;
        while ei < mxu3501flowEvent::ALL.len() {
            let legal = mxu3501flow_next(mxu3501flowState::ALL[si], mxu3501flowEvent::ALL[ei]).is_some();
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
pub fn mxu3501flow_terminal(st: mxu3501flowState) -> bool {
    let mut i = 0usize;
    while i < 8 {
        if MXU3501FLOW_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct mxu3501flowLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: mxu3501flowState,
}

impl mxu3501flowLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: mxu3501flowState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: mxu3501flowEvent) -> bool {
        match mxu3501flow_next(self.cur, ev) {
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
// 快照 Mxu3501Snap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// 跟踪点体系 快照封存（FNV 三态指纹 + 篡改检出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mxu3501Snap {
    /// 指纹分量：tp_v。
    pub tp_v: u64,
    /// 指纹分量：reg_v。
    pub reg_v: u64,
    /// 指纹分量：ins_v。
    pub ins_v: u64,
    /// 指纹分量：sw_v。
    pub sw_v: u64,
    /// 指纹分量：cid_v。
    pub cid_v: u64,
    /// 指纹分量：hit_v。
    pub hit_v: u64,
    /// 指纹分量：buf_v。
    pub buf_v: u64,
    /// 指纹分量：flt_v。
    pub flt_v: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl Mxu3501Snap {
    /// 构造并即时合成指纹。
    pub fn compute(
        tp_v: u64,
        reg_v: u64,
        ins_v: u64,
        sw_v: u64,
        cid_v: u64,
        hit_v: u64,
        buf_v: u64,
        flt_v: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &tp_v.to_le_bytes());
        h = fnv1a64_from(h, &reg_v.to_le_bytes());
        h = fnv1a64_from(h, &ins_v.to_le_bytes());
        h = fnv1a64_from(h, &sw_v.to_le_bytes());
        h = fnv1a64_from(h, &cid_v.to_le_bytes());
        h = fnv1a64_from(h, &hit_v.to_le_bytes());
        h = fnv1a64_from(h, &buf_v.to_le_bytes());
        h = fnv1a64_from(h, &flt_v.to_le_bytes());
        Self {
            tp_v,
            reg_v,
            ins_v,
            sw_v,
            cid_v,
            hit_v,
            buf_v,
            flt_v,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.tp_v.to_le_bytes());
        h = fnv1a64_from(h, &self.reg_v.to_le_bytes());
        h = fnv1a64_from(h, &self.ins_v.to_le_bytes());
        h = fnv1a64_from(h, &self.sw_v.to_le_bytes());
        h = fnv1a64_from(h, &self.cid_v.to_le_bytes());
        h = fnv1a64_from(h, &self.hit_v.to_le_bytes());
        h = fnv1a64_from(h, &self.buf_v.to_le_bytes());
        h = fnv1a64_from(h, &self.flt_v.to_le_bytes());
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

/// 段位守卫：全部诊断码必须落在本段 [0xe0d0, 0xe0df]。
pub const fn u3501_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3501Code::ALL.len() {
        let c = u3501Code::ALL[i].code();
        if c < 0xe0d0 || c > 0xe0df {
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
fn u3501_guard_seg() -> bool {
    u3501_segment_guard()
}

/// UNX-F3501 状态机闭集/迁移表/非法组合恰值全绿。
fn check_u3501_sm_legal() -> bool {
    let mut ok = mxu3501flow_illegal_pairs() == 29;
    if ok { ok = MXU3501FLOW_TRANSITIONS.len() == 8; }
    if ok { ok = mxu3501flowState::ALL.len() == 6 && mxu3501flowEvent::ALL.len() == 6; }
    ok
}

/// UNX-F3501 迁移主链回放 + 快照往返恒等。
fn check_u3501_sm_round() -> bool {
    let mut lg = mxu3501flowLedger::new(mxu3501flowState::Reg);
    let mut ok = lg.apply(mxu3501flowEvent::Register) && lg.cur == mxu3501flowState::Armed && lg.total() == 1;
    let s0 = Mxu3501Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    let mut s1 = Mxu3501Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    s1.recompute();
    if ok { ok = s0 == s1 && !s1.tampered(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3501_checks() -> CheckSet {
    let mut set = CheckSet::new("U3501");
    set.add(
        "u3501-seg-guard",
        u3501_guard_seg(),
        if u3501_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3501-sm-legal",
        check_u3501_sm_legal(),
        if check_u3501_sm_legal() { "" } else { "UNX-F3501 sm legality mismatch" },
    );
    set.add(
        "u3501-sm-round",
        check_u3501_sm_round(),
        if check_u3501_sm_round() { "" } else { "UNX-F3501 sm roundtrip mismatch" },
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
    fn u3501_all_checks_green() {
        let set = run_u3501_checks();
        assert!(set.all_passed(), "U3501 domain checks must be green");
    }

    #[test]
    fn u3501_segment_guard_holds() {
        assert!(u3501_segment_guard());
    }
}
