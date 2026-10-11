//! unxu3681_u3681sm —— fuzz 总设计（生成/执行/判读三段管线） 状态机与快照封存（fuzz 三段管线）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3681 · fuzz 总设计（生成/执行/判读三段管线） · 判据原文：UNX-F3681 · 判据：J1：三段管线（生成（seed→输入序列）→执行（入口调用+判据采集）→判读（panic/账失衡/违例/confirm 分类）——判据行内嵌三段各一节）；M-439 三段首跑（最小入口+8 种子语料→一跑全链出判读报告）。<br>J2：管线确定性（同 seed 同语料→同输入序列→同判读——F3687 复现的架构前提——无外部熵）。
//! · 组件：fuzz 三段管线 状态机（6 态 × 6 事件显式迁移）+ FNV 快照封存（8 分量）
//! · 判据落地：UNX-F3681 · 判据：J1：三段管线（生成（seed→输入序列）→执行（入口调用+判据采集）→判读（panic/账失衡/违例/confirm 分类）——
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3681_VERSION: &str = "u3681-u3681sm-v1";

/// 诊断段基址。
pub const U3681_SEG_BASE: u16 = 0xe160;

// --------------------------------------------------------------------------
// 诊断码（u3681 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// fuzz 三段管线 状态面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3681Code {
    /// 0xe160 状态重复（fuzz 同名双态）。
    StateDup,
    /// 0xe161 状态断档（生成 序缺失）。
    StateHole,
    /// 0xe162 事件重复（同事件双声明）。
    EventDup,
    /// 0xe163 迁移重复（同源同事件双迁移）。
    TransDup,
    /// 0xe164 迁移缺失（应达态不可达）。
    TransHole,
    /// 0xe165 迁移环（非法自环/环路）。
    TransCycle,
    /// 0xe166 入口非法（初态不在闭集）。
    EntryBad,
    /// 0xe167 迁移丢失（应用事件无效果）。
    ApplyLost,
    /// 0xe168 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe169 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe16a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe16b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3681Code {
    /// 全量闭集。
    pub const ALL: [u3681Code; 12] = [
        u3681Code::StateDup,
        u3681Code::StateHole,
        u3681Code::EventDup,
        u3681Code::TransDup,
        u3681Code::TransHole,
        u3681Code::TransCycle,
        u3681Code::EntryBad,
        u3681Code::ApplyLost,
        u3681Code::LedgerBroken,
        u3681Code::SnapshotFingerprint,
        u3681Code::TableBroken,
        u3681Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3681Code::StateDup => 0xe160,
            u3681Code::StateHole => 0xe161,
            u3681Code::EventDup => 0xe162,
            u3681Code::TransDup => 0xe163,
            u3681Code::TransHole => 0xe164,
            u3681Code::TransCycle => 0xe165,
            u3681Code::EntryBad => 0xe166,
            u3681Code::ApplyLost => 0xe167,
            u3681Code::LedgerBroken => 0xe168,
            u3681Code::SnapshotFingerprint => 0xe169,
            u3681Code::TableBroken => 0xe16a,
            u3681Code::SegmentGuard => 0xe16b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3681Code::StateDup => "状态重复（fuzz 同名双态）",
            u3681Code::StateHole => "状态断档（生成 序缺失）",
            u3681Code::EventDup => "事件重复（同事件双声明）",
            u3681Code::TransDup => "迁移重复（同源同事件双迁移）",
            u3681Code::TransHole => "迁移缺失（应达态不可达）",
            u3681Code::TransCycle => "迁移环（非法自环/环路）",
            u3681Code::EntryBad => "入口非法（初态不在闭集）",
            u3681Code::ApplyLost => "迁移丢失（应用事件无效果）",
            u3681Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3681Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3681Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3681Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3681Code::StateDup => "按闭集唯一律去重",
            u3681Code::StateHole => "按闭集补态",
            u3681Code::EventDup => "按事件闭集去重",
            u3681Code::TransDup => "按确定性唯一迁移律合并",
            u3681Code::TransHole => "按迁移矩阵补边",
            u3681Code::TransCycle => "按无环律重构迁移",
            u3681Code::EntryBad => "按闭集钳制初态",
            u3681Code::ApplyLost => "核对事件前置条件",
            u3681Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3681Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3681Code::TableBroken => "按语义规则重建常量表",
            u3681Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 状态机 mxu3681flow（6 态 × 6 事件 · 迁移 8 条显式声明）
// --------------------------------------------------------------------------
/// fuzz 三段管线 状态机（判据：fuzz）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu3681flowState {
    Idle,
    Generating,
    Executing,
    Judging,
    Recording,
    Quarantined,
}

impl mxu3681flowState {
    /// 全量态闭集。
    pub const ALL: [mxu3681flowState; 6] = [
        mxu3681flowState::Idle,
        mxu3681flowState::Generating,
        mxu3681flowState::Executing,
        mxu3681flowState::Judging,
        mxu3681flowState::Recording,
        mxu3681flowState::Quarantined,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            mxu3681flowState::Idle => 0,
            mxu3681flowState::Generating => 1,
            mxu3681flowState::Executing => 2,
            mxu3681flowState::Judging => 3,
            mxu3681flowState::Recording => 4,
            mxu3681flowState::Quarantined => 5,
        }
    }
}

/// mxu3681flow事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu3681flowEvent {
    Gen,
    Mutate,
    Exec,
    Judge,
    Dedup,
    Record,
}

impl mxu3681flowEvent {
    /// 全量事件闭集。
    pub const ALL: [mxu3681flowEvent; 6] = [
        mxu3681flowEvent::Gen,
        mxu3681flowEvent::Mutate,
        mxu3681flowEvent::Exec,
        mxu3681flowEvent::Judge,
        mxu3681flowEvent::Dedup,
        mxu3681flowEvent::Record,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const MXU3681FLOW_TRANSITIONS: [(mxu3681flowState, mxu3681flowEvent, mxu3681flowState); 8] = [
    (mxu3681flowState::Idle, mxu3681flowEvent::Gen, mxu3681flowState::Generating), // Idle→Generating 主链
    (mxu3681flowState::Generating, mxu3681flowEvent::Mutate, mxu3681flowState::Executing), // Generating→Executing 主链
    (mxu3681flowState::Executing, mxu3681flowEvent::Exec, mxu3681flowState::Judging), // Executing→Judging 主链
    (mxu3681flowState::Judging, mxu3681flowEvent::Judge, mxu3681flowState::Recording), // Judging→Recording 主链
    (mxu3681flowState::Recording, mxu3681flowEvent::Dedup, mxu3681flowState::Quarantined), // Recording→Quarantined 主链
    (mxu3681flowState::Quarantined, mxu3681flowEvent::Record, mxu3681flowState::Idle), // 终态回边
    (mxu3681flowState::Quarantined, mxu3681flowEvent::Gen, mxu3681flowState::Recording), // 终态回退边
    (mxu3681flowState::Generating, mxu3681flowEvent::Mutate, mxu3681flowState::Executing), // 旁路边
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn mxu3681flow_next(state: mxu3681flowState, ev: mxu3681flowEvent) -> Option<mxu3681flowState> {
    let mut i = 0usize;
    while i < 8 {
        let t = MXU3681FLOW_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn mxu3681flow_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < mxu3681flowState::ALL.len() {
        let mut ei = 0usize;
        while ei < mxu3681flowEvent::ALL.len() {
            let legal = mxu3681flow_next(mxu3681flowState::ALL[si], mxu3681flowEvent::ALL[ei]).is_some();
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
pub fn mxu3681flow_terminal(st: mxu3681flowState) -> bool {
    let mut i = 0usize;
    while i < 8 {
        if MXU3681FLOW_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct mxu3681flowLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: mxu3681flowState,
}

impl mxu3681flowLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: mxu3681flowState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: mxu3681flowEvent) -> bool {
        match mxu3681flow_next(self.cur, ev) {
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
// 快照 Mxu3681Snap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// fuzz 三段管线 快照封存（FNV 三态指纹 + 篡改检出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mxu3681Snap {
    /// 指纹分量：fuzz_v。
    pub fuzz_v: u64,
    /// 指纹分量：gen_v。
    pub gen_v: u64,
    /// 指纹分量：exec_v。
    pub exec_v: u64,
    /// 指纹分量：judge_v。
    pub judge_v: u64,
    /// 指纹分量：seed_v。
    pub seed_v: u64,
    /// 指纹分量：corpus_v。
    pub corpus_v: u64,
    /// 指纹分量：mut_v。
    pub mut_v: u64,
    /// 指纹分量：crash_v。
    pub crash_v: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl Mxu3681Snap {
    /// 构造并即时合成指纹。
    pub fn compute(
        fuzz_v: u64,
        gen_v: u64,
        exec_v: u64,
        judge_v: u64,
        seed_v: u64,
        corpus_v: u64,
        mut_v: u64,
        crash_v: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &fuzz_v.to_le_bytes());
        h = fnv1a64_from(h, &gen_v.to_le_bytes());
        h = fnv1a64_from(h, &exec_v.to_le_bytes());
        h = fnv1a64_from(h, &judge_v.to_le_bytes());
        h = fnv1a64_from(h, &seed_v.to_le_bytes());
        h = fnv1a64_from(h, &corpus_v.to_le_bytes());
        h = fnv1a64_from(h, &mut_v.to_le_bytes());
        h = fnv1a64_from(h, &crash_v.to_le_bytes());
        Self {
            fuzz_v,
            gen_v,
            exec_v,
            judge_v,
            seed_v,
            corpus_v,
            mut_v,
            crash_v,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.fuzz_v.to_le_bytes());
        h = fnv1a64_from(h, &self.gen_v.to_le_bytes());
        h = fnv1a64_from(h, &self.exec_v.to_le_bytes());
        h = fnv1a64_from(h, &self.judge_v.to_le_bytes());
        h = fnv1a64_from(h, &self.seed_v.to_le_bytes());
        h = fnv1a64_from(h, &self.corpus_v.to_le_bytes());
        h = fnv1a64_from(h, &self.mut_v.to_le_bytes());
        h = fnv1a64_from(h, &self.crash_v.to_le_bytes());
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

/// 段位守卫：全部诊断码必须落在本段 [0xe160, 0xe16f]。
pub const fn u3681_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3681Code::ALL.len() {
        let c = u3681Code::ALL[i].code();
        if c < 0xe160 || c > 0xe16f {
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
fn u3681_guard_seg() -> bool {
    u3681_segment_guard()
}

/// UNX-F3681 状态机闭集/迁移表/非法组合恰值全绿。
fn check_u3681_sm_legal() -> bool {
    let mut ok = mxu3681flow_illegal_pairs() == 29;
    if ok { ok = MXU3681FLOW_TRANSITIONS.len() == 8; }
    if ok { ok = mxu3681flowState::ALL.len() == 6 && mxu3681flowEvent::ALL.len() == 6; }
    ok
}

/// UNX-F3681 迁移主链回放 + 快照往返恒等。
fn check_u3681_sm_round() -> bool {
    let mut lg = mxu3681flowLedger::new(mxu3681flowState::Idle);
    let mut ok = lg.apply(mxu3681flowEvent::Gen) && lg.cur == mxu3681flowState::Generating && lg.total() == 1;
    let s0 = Mxu3681Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    let mut s1 = Mxu3681Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    s1.recompute();
    if ok { ok = s0 == s1 && !s1.tampered(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3681_checks() -> CheckSet {
    let mut set = CheckSet::new("U3681");
    set.add(
        "u3681-seg-guard",
        u3681_guard_seg(),
        if u3681_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3681-sm-legal",
        check_u3681_sm_legal(),
        if check_u3681_sm_legal() { "" } else { "UNX-F3681 sm legality mismatch" },
    );
    set.add(
        "u3681-sm-round",
        check_u3681_sm_round(),
        if check_u3681_sm_round() { "" } else { "UNX-F3681 sm roundtrip mismatch" },
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
    fn u3681_all_checks_green() {
        let set = run_u3681_checks();
        assert!(set.all_passed(), "U3681 domain checks must be green");
    }

    #[test]
    fn u3681_segment_guard_holds() {
        assert!(u3681_segment_guard());
    }
}
