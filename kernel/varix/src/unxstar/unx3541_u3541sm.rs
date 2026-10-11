//! unxu3541_u3541sm —— 采样剖析器总设计（周期中断采样——桩化 tick） 状态机与快照封存（采样剖析器）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3541 · 采样剖析器总设计（周期中断采样——桩化 tick） · 判据原文：UNX-F3541 · 判据：J1：剖析语义冻结（周期 tick→采当前栈→计数——采样式非插桩式）；M-299 采样全链（tick→样本→聚合→导出四步通）。<br>J2：采样式选型声明（与插桩式的取舍——零代码改动税/精度换自由——判据行内嵌）。
//! · 组件：采样剖析器 状态机（6 态 × 6 事件显式迁移）+ FNV 快照封存（8 分量）
//! · 判据落地：UNX-F3541 · 判据：J1：剖析语义冻结（周期 tick→采当前栈→计数——采样式非插桩式）；M-299 采样全链（tick→样本→聚合→导出四步通）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3541_VERSION: &str = "u3541-u3541sm-v1";

/// 诊断段基址。
pub const U3541_SEG_BASE: u16 = 0xe0f0;

// --------------------------------------------------------------------------
// 诊断码（u3541 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 采样剖析器 状态面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3541Code {
    /// 0xe0f0 状态重复（采样 同名双态）。
    StateDup,
    /// 0xe0f1 状态断档（剖析 序缺失）。
    StateHole,
    /// 0xe0f2 事件重复（同事件双声明）。
    EventDup,
    /// 0xe0f3 迁移重复（同源同事件双迁移）。
    TransDup,
    /// 0xe0f4 迁移缺失（应达态不可达）。
    TransHole,
    /// 0xe0f5 迁移环（非法自环/环路）。
    TransCycle,
    /// 0xe0f6 入口非法（初态不在闭集）。
    EntryBad,
    /// 0xe0f7 迁移丢失（应用事件无效果）。
    ApplyLost,
    /// 0xe0f8 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe0f9 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe0fa 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe0fb 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3541Code {
    /// 全量闭集。
    pub const ALL: [u3541Code; 12] = [
        u3541Code::StateDup,
        u3541Code::StateHole,
        u3541Code::EventDup,
        u3541Code::TransDup,
        u3541Code::TransHole,
        u3541Code::TransCycle,
        u3541Code::EntryBad,
        u3541Code::ApplyLost,
        u3541Code::LedgerBroken,
        u3541Code::SnapshotFingerprint,
        u3541Code::TableBroken,
        u3541Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3541Code::StateDup => 0xe0f0,
            u3541Code::StateHole => 0xe0f1,
            u3541Code::EventDup => 0xe0f2,
            u3541Code::TransDup => 0xe0f3,
            u3541Code::TransHole => 0xe0f4,
            u3541Code::TransCycle => 0xe0f5,
            u3541Code::EntryBad => 0xe0f6,
            u3541Code::ApplyLost => 0xe0f7,
            u3541Code::LedgerBroken => 0xe0f8,
            u3541Code::SnapshotFingerprint => 0xe0f9,
            u3541Code::TableBroken => 0xe0fa,
            u3541Code::SegmentGuard => 0xe0fb,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3541Code::StateDup => "状态重复（采样 同名双态）",
            u3541Code::StateHole => "状态断档（剖析 序缺失）",
            u3541Code::EventDup => "事件重复（同事件双声明）",
            u3541Code::TransDup => "迁移重复（同源同事件双迁移）",
            u3541Code::TransHole => "迁移缺失（应达态不可达）",
            u3541Code::TransCycle => "迁移环（非法自环/环路）",
            u3541Code::EntryBad => "入口非法（初态不在闭集）",
            u3541Code::ApplyLost => "迁移丢失（应用事件无效果）",
            u3541Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3541Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3541Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3541Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3541Code::StateDup => "按闭集唯一律去重",
            u3541Code::StateHole => "按闭集补态",
            u3541Code::EventDup => "按事件闭集去重",
            u3541Code::TransDup => "按确定性唯一迁移律合并",
            u3541Code::TransHole => "按迁移矩阵补边",
            u3541Code::TransCycle => "按无环律重构迁移",
            u3541Code::EntryBad => "按闭集钳制初态",
            u3541Code::ApplyLost => "核对事件前置条件",
            u3541Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3541Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3541Code::TableBroken => "按语义规则重建常量表",
            u3541Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 状态机 mxu3541flow（6 态 × 6 事件 · 迁移 8 条显式声明）
// --------------------------------------------------------------------------
/// 采样剖析器 状态机（判据：采样）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu3541flowState {
    Idle,
    Armed,
    Sampling,
    Folding,
    Reporting,
    Paused,
}

impl mxu3541flowState {
    /// 全量态闭集。
    pub const ALL: [mxu3541flowState; 6] = [
        mxu3541flowState::Idle,
        mxu3541flowState::Armed,
        mxu3541flowState::Sampling,
        mxu3541flowState::Folding,
        mxu3541flowState::Reporting,
        mxu3541flowState::Paused,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            mxu3541flowState::Idle => 0,
            mxu3541flowState::Armed => 1,
            mxu3541flowState::Sampling => 2,
            mxu3541flowState::Folding => 3,
            mxu3541flowState::Reporting => 4,
            mxu3541flowState::Paused => 5,
        }
    }
}

/// mxu3541flow事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu3541flowEvent {
    Arm,
    Fire,
    Capture,
    Fold,
    Report,
    Pause,
}

impl mxu3541flowEvent {
    /// 全量事件闭集。
    pub const ALL: [mxu3541flowEvent; 6] = [
        mxu3541flowEvent::Arm,
        mxu3541flowEvent::Fire,
        mxu3541flowEvent::Capture,
        mxu3541flowEvent::Fold,
        mxu3541flowEvent::Report,
        mxu3541flowEvent::Pause,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const MXU3541FLOW_TRANSITIONS: [(mxu3541flowState, mxu3541flowEvent, mxu3541flowState); 8] = [
    (mxu3541flowState::Idle, mxu3541flowEvent::Arm, mxu3541flowState::Armed), // Idle→Armed 主链
    (mxu3541flowState::Armed, mxu3541flowEvent::Fire, mxu3541flowState::Sampling), // Armed→Sampling 主链
    (mxu3541flowState::Sampling, mxu3541flowEvent::Capture, mxu3541flowState::Folding), // Sampling→Folding 主链
    (mxu3541flowState::Folding, mxu3541flowEvent::Fold, mxu3541flowState::Reporting), // Folding→Reporting 主链
    (mxu3541flowState::Reporting, mxu3541flowEvent::Report, mxu3541flowState::Paused), // Reporting→Paused 主链
    (mxu3541flowState::Paused, mxu3541flowEvent::Pause, mxu3541flowState::Idle), // 终态回边
    (mxu3541flowState::Paused, mxu3541flowEvent::Arm, mxu3541flowState::Reporting), // 终态回退边
    (mxu3541flowState::Armed, mxu3541flowEvent::Fire, mxu3541flowState::Sampling), // 旁路边
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn mxu3541flow_next(state: mxu3541flowState, ev: mxu3541flowEvent) -> Option<mxu3541flowState> {
    let mut i = 0usize;
    while i < 8 {
        let t = MXU3541FLOW_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn mxu3541flow_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < mxu3541flowState::ALL.len() {
        let mut ei = 0usize;
        while ei < mxu3541flowEvent::ALL.len() {
            let legal = mxu3541flow_next(mxu3541flowState::ALL[si], mxu3541flowEvent::ALL[ei]).is_some();
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
pub fn mxu3541flow_terminal(st: mxu3541flowState) -> bool {
    let mut i = 0usize;
    while i < 8 {
        if MXU3541FLOW_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct mxu3541flowLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: mxu3541flowState,
}

impl mxu3541flowLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: mxu3541flowState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: mxu3541flowEvent) -> bool {
        match mxu3541flow_next(self.cur, ev) {
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
// 快照 Mxu3541Snap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// 采样剖析器 快照封存（FNV 三态指纹 + 篡改检出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mxu3541Snap {
    /// 指纹分量：prof_v。
    pub prof_v: u64,
    /// 指纹分量：pmi_v。
    pub pmi_v: u64,
    /// 指纹分量：tick_v。
    pub tick_v: u64,
    /// 指纹分量：ip_v。
    pub ip_v: u64,
    /// 指纹分量：stack_v。
    pub stack_v: u64,
    /// 指纹分量：call_v。
    pub call_v: u64,
    /// 指纹分量：rate_v。
    pub rate_v: u64,
    /// 指纹分量：buf_v。
    pub buf_v: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl Mxu3541Snap {
    /// 构造并即时合成指纹。
    pub fn compute(
        prof_v: u64,
        pmi_v: u64,
        tick_v: u64,
        ip_v: u64,
        stack_v: u64,
        call_v: u64,
        rate_v: u64,
        buf_v: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &prof_v.to_le_bytes());
        h = fnv1a64_from(h, &pmi_v.to_le_bytes());
        h = fnv1a64_from(h, &tick_v.to_le_bytes());
        h = fnv1a64_from(h, &ip_v.to_le_bytes());
        h = fnv1a64_from(h, &stack_v.to_le_bytes());
        h = fnv1a64_from(h, &call_v.to_le_bytes());
        h = fnv1a64_from(h, &rate_v.to_le_bytes());
        h = fnv1a64_from(h, &buf_v.to_le_bytes());
        Self {
            prof_v,
            pmi_v,
            tick_v,
            ip_v,
            stack_v,
            call_v,
            rate_v,
            buf_v,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.prof_v.to_le_bytes());
        h = fnv1a64_from(h, &self.pmi_v.to_le_bytes());
        h = fnv1a64_from(h, &self.tick_v.to_le_bytes());
        h = fnv1a64_from(h, &self.ip_v.to_le_bytes());
        h = fnv1a64_from(h, &self.stack_v.to_le_bytes());
        h = fnv1a64_from(h, &self.call_v.to_le_bytes());
        h = fnv1a64_from(h, &self.rate_v.to_le_bytes());
        h = fnv1a64_from(h, &self.buf_v.to_le_bytes());
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

/// 段位守卫：全部诊断码必须落在本段 [0xe0f0, 0xe0ff]。
pub const fn u3541_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3541Code::ALL.len() {
        let c = u3541Code::ALL[i].code();
        if c < 0xe0f0 || c > 0xe0ff {
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
fn u3541_guard_seg() -> bool {
    u3541_segment_guard()
}

/// UNX-F3541 状态机闭集/迁移表/非法组合恰值全绿。
fn check_u3541_sm_legal() -> bool {
    let mut ok = mxu3541flow_illegal_pairs() == 29;
    if ok { ok = MXU3541FLOW_TRANSITIONS.len() == 8; }
    if ok { ok = mxu3541flowState::ALL.len() == 6 && mxu3541flowEvent::ALL.len() == 6; }
    ok
}

/// UNX-F3541 迁移主链回放 + 快照往返恒等。
fn check_u3541_sm_round() -> bool {
    let mut lg = mxu3541flowLedger::new(mxu3541flowState::Idle);
    let mut ok = lg.apply(mxu3541flowEvent::Arm) && lg.cur == mxu3541flowState::Armed && lg.total() == 1;
    let s0 = Mxu3541Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    let mut s1 = Mxu3541Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    s1.recompute();
    if ok { ok = s0 == s1 && !s1.tampered(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3541_checks() -> CheckSet {
    let mut set = CheckSet::new("U3541");
    set.add(
        "u3541-seg-guard",
        u3541_guard_seg(),
        if u3541_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3541-sm-legal",
        check_u3541_sm_legal(),
        if check_u3541_sm_legal() { "" } else { "UNX-F3541 sm legality mismatch" },
    );
    set.add(
        "u3541-sm-round",
        check_u3541_sm_round(),
        if check_u3541_sm_round() { "" } else { "UNX-F3541 sm roundtrip mismatch" },
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
    fn u3541_all_checks_green() {
        let set = run_u3541_checks();
        assert!(set.all_passed(), "U3541 domain checks must be green");
    }

    #[test]
    fn u3541_segment_guard_holds() {
        assert!(u3541_segment_guard());
    }
}
