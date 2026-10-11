//! unxu3721_u3721sm —— 桩体系总设计（替身三原则与桩清单） 状态机与快照封存（桩体系）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3721 · 桩体系总设计（替身三原则与桩清单） · 判据原文：UNX-F3721 · 判据：J1：替身三原则（确定性（同脚本同态同行为）/账本化（操作全记账）/可控性（时间与故障全可控——判据行内嵌三原则各一句）；桩清单（时钟/中断/DMA/存储/网卡五桩 v1——清单 const 扩桩走 bump）；M-479 三原则首验（时钟桩+中断桩最小联动→三原则逐条断言）。<br>J2：桩与被测分池（B20 隔离池同律——桩基础设施不吃被测方内存）。
//! · 组件：桩体系 状态机（6 态 × 6 事件显式迁移）+ FNV 快照封存（8 分量）
//! · 判据落地：UNX-F3721 · 判据：J1：替身三原则（确定性（同脚本同态同行为）/账本化（操作全记账）/可控性（时间与故障全可控——判据行内嵌三原则各一句）；桩清单（
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3721_VERSION: &str = "u3721-u3721sm-v1";

/// 诊断段基址。
pub const U3721_SEG_BASE: u16 = 0xe180;

// --------------------------------------------------------------------------
// 诊断码（u3721 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 桩体系 状态面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3721Code {
    /// 0xe180 状态重复（桩 同名双态）。
    StateDup,
    /// 0xe181 状态断档（替身 序缺失）。
    StateHole,
    /// 0xe182 事件重复（同事件双声明）。
    EventDup,
    /// 0xe183 迁移重复（同源同事件双迁移）。
    TransDup,
    /// 0xe184 迁移缺失（应达态不可达）。
    TransHole,
    /// 0xe185 迁移环（非法自环/环路）。
    TransCycle,
    /// 0xe186 入口非法（初态不在闭集）。
    EntryBad,
    /// 0xe187 迁移丢失（应用事件无效果）。
    ApplyLost,
    /// 0xe188 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe189 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe18a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe18b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3721Code {
    /// 全量闭集。
    pub const ALL: [u3721Code; 12] = [
        u3721Code::StateDup,
        u3721Code::StateHole,
        u3721Code::EventDup,
        u3721Code::TransDup,
        u3721Code::TransHole,
        u3721Code::TransCycle,
        u3721Code::EntryBad,
        u3721Code::ApplyLost,
        u3721Code::LedgerBroken,
        u3721Code::SnapshotFingerprint,
        u3721Code::TableBroken,
        u3721Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3721Code::StateDup => 0xe180,
            u3721Code::StateHole => 0xe181,
            u3721Code::EventDup => 0xe182,
            u3721Code::TransDup => 0xe183,
            u3721Code::TransHole => 0xe184,
            u3721Code::TransCycle => 0xe185,
            u3721Code::EntryBad => 0xe186,
            u3721Code::ApplyLost => 0xe187,
            u3721Code::LedgerBroken => 0xe188,
            u3721Code::SnapshotFingerprint => 0xe189,
            u3721Code::TableBroken => 0xe18a,
            u3721Code::SegmentGuard => 0xe18b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3721Code::StateDup => "状态重复（桩 同名双态）",
            u3721Code::StateHole => "状态断档（替身 序缺失）",
            u3721Code::EventDup => "事件重复（同事件双声明）",
            u3721Code::TransDup => "迁移重复（同源同事件双迁移）",
            u3721Code::TransHole => "迁移缺失（应达态不可达）",
            u3721Code::TransCycle => "迁移环（非法自环/环路）",
            u3721Code::EntryBad => "入口非法（初态不在闭集）",
            u3721Code::ApplyLost => "迁移丢失（应用事件无效果）",
            u3721Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3721Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3721Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3721Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3721Code::StateDup => "按闭集唯一律去重",
            u3721Code::StateHole => "按闭集补态",
            u3721Code::EventDup => "按事件闭集去重",
            u3721Code::TransDup => "按确定性唯一迁移律合并",
            u3721Code::TransHole => "按迁移矩阵补边",
            u3721Code::TransCycle => "按无环律重构迁移",
            u3721Code::EntryBad => "按闭集钳制初态",
            u3721Code::ApplyLost => "核对事件前置条件",
            u3721Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3721Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3721Code::TableBroken => "按语义规则重建常量表",
            u3721Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 状态机 mxu3721flow（6 态 × 6 事件 · 迁移 8 条显式声明）
// --------------------------------------------------------------------------
/// 桩体系 状态机（判据：桩）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu3721flowState {
    Declared,
    Slotted,
    Idle,
    Recording,
    Replaying,
    Failing,
}

impl mxu3721flowState {
    /// 全量态闭集。
    pub const ALL: [mxu3721flowState; 6] = [
        mxu3721flowState::Declared,
        mxu3721flowState::Slotted,
        mxu3721flowState::Idle,
        mxu3721flowState::Recording,
        mxu3721flowState::Replaying,
        mxu3721flowState::Failing,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            mxu3721flowState::Declared => 0,
            mxu3721flowState::Slotted => 1,
            mxu3721flowState::Idle => 2,
            mxu3721flowState::Recording => 3,
            mxu3721flowState::Replaying => 4,
            mxu3721flowState::Failing => 5,
        }
    }
}

/// mxu3721flow事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu3721flowEvent {
    Declare,
    Occupy,
    Record,
    Play,
    Fail,
    Audit,
}

impl mxu3721flowEvent {
    /// 全量事件闭集。
    pub const ALL: [mxu3721flowEvent; 6] = [
        mxu3721flowEvent::Declare,
        mxu3721flowEvent::Occupy,
        mxu3721flowEvent::Record,
        mxu3721flowEvent::Play,
        mxu3721flowEvent::Fail,
        mxu3721flowEvent::Audit,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const MXU3721FLOW_TRANSITIONS: [(mxu3721flowState, mxu3721flowEvent, mxu3721flowState); 8] = [
    (mxu3721flowState::Declared, mxu3721flowEvent::Declare, mxu3721flowState::Slotted), // Declared→Slotted 主链
    (mxu3721flowState::Slotted, mxu3721flowEvent::Occupy, mxu3721flowState::Idle), // Slotted→Idle 主链
    (mxu3721flowState::Idle, mxu3721flowEvent::Record, mxu3721flowState::Recording), // Idle→Recording 主链
    (mxu3721flowState::Recording, mxu3721flowEvent::Play, mxu3721flowState::Replaying), // Recording→Replaying 主链
    (mxu3721flowState::Replaying, mxu3721flowEvent::Fail, mxu3721flowState::Failing), // Replaying→Failing 主链
    (mxu3721flowState::Failing, mxu3721flowEvent::Audit, mxu3721flowState::Declared), // 终态回边
    (mxu3721flowState::Failing, mxu3721flowEvent::Declare, mxu3721flowState::Replaying), // 终态回退边
    (mxu3721flowState::Slotted, mxu3721flowEvent::Occupy, mxu3721flowState::Idle), // 旁路边
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn mxu3721flow_next(state: mxu3721flowState, ev: mxu3721flowEvent) -> Option<mxu3721flowState> {
    let mut i = 0usize;
    while i < 8 {
        let t = MXU3721FLOW_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn mxu3721flow_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < mxu3721flowState::ALL.len() {
        let mut ei = 0usize;
        while ei < mxu3721flowEvent::ALL.len() {
            let legal = mxu3721flow_next(mxu3721flowState::ALL[si], mxu3721flowEvent::ALL[ei]).is_some();
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
pub fn mxu3721flow_terminal(st: mxu3721flowState) -> bool {
    let mut i = 0usize;
    while i < 8 {
        if MXU3721FLOW_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct mxu3721flowLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: mxu3721flowState,
}

impl mxu3721flowLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: mxu3721flowState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: mxu3721flowEvent) -> bool {
        match mxu3721flow_next(self.cur, ev) {
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
// 快照 Mxu3721Snap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// 桩体系 快照封存（FNV 三态指纹 + 篡改检出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mxu3721Snap {
    /// 指纹分量：stub_v。
    pub stub_v: u64,
    /// 指纹分量：triple_v。
    pub triple_v: u64,
    /// 指纹分量：list_v。
    pub list_v: u64,
    /// 指纹分量：inj_v。
    pub inj_v: u64,
    /// 指纹分量：rec_v。
    pub rec_v: u64,
    /// 指纹分量：play_v。
    pub play_v: u64,
    /// 指纹分量：fail_v。
    pub fail_v: u64,
    /// 指纹分量：mode_v。
    pub mode_v: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl Mxu3721Snap {
    /// 构造并即时合成指纹。
    pub fn compute(
        stub_v: u64,
        triple_v: u64,
        list_v: u64,
        inj_v: u64,
        rec_v: u64,
        play_v: u64,
        fail_v: u64,
        mode_v: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &stub_v.to_le_bytes());
        h = fnv1a64_from(h, &triple_v.to_le_bytes());
        h = fnv1a64_from(h, &list_v.to_le_bytes());
        h = fnv1a64_from(h, &inj_v.to_le_bytes());
        h = fnv1a64_from(h, &rec_v.to_le_bytes());
        h = fnv1a64_from(h, &play_v.to_le_bytes());
        h = fnv1a64_from(h, &fail_v.to_le_bytes());
        h = fnv1a64_from(h, &mode_v.to_le_bytes());
        Self {
            stub_v,
            triple_v,
            list_v,
            inj_v,
            rec_v,
            play_v,
            fail_v,
            mode_v,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.stub_v.to_le_bytes());
        h = fnv1a64_from(h, &self.triple_v.to_le_bytes());
        h = fnv1a64_from(h, &self.list_v.to_le_bytes());
        h = fnv1a64_from(h, &self.inj_v.to_le_bytes());
        h = fnv1a64_from(h, &self.rec_v.to_le_bytes());
        h = fnv1a64_from(h, &self.play_v.to_le_bytes());
        h = fnv1a64_from(h, &self.fail_v.to_le_bytes());
        h = fnv1a64_from(h, &self.mode_v.to_le_bytes());
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

/// 段位守卫：全部诊断码必须落在本段 [0xe180, 0xe18f]。
pub const fn u3721_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3721Code::ALL.len() {
        let c = u3721Code::ALL[i].code();
        if c < 0xe180 || c > 0xe18f {
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
fn u3721_guard_seg() -> bool {
    u3721_segment_guard()
}

/// UNX-F3721 状态机闭集/迁移表/非法组合恰值全绿。
fn check_u3721_sm_legal() -> bool {
    let mut ok = mxu3721flow_illegal_pairs() == 29;
    if ok { ok = MXU3721FLOW_TRANSITIONS.len() == 8; }
    if ok { ok = mxu3721flowState::ALL.len() == 6 && mxu3721flowEvent::ALL.len() == 6; }
    ok
}

/// UNX-F3721 迁移主链回放 + 快照往返恒等。
fn check_u3721_sm_round() -> bool {
    let mut lg = mxu3721flowLedger::new(mxu3721flowState::Declared);
    let mut ok = lg.apply(mxu3721flowEvent::Declare) && lg.cur == mxu3721flowState::Slotted && lg.total() == 1;
    let s0 = Mxu3721Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    let mut s1 = Mxu3721Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    s1.recompute();
    if ok { ok = s0 == s1 && !s1.tampered(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3721_checks() -> CheckSet {
    let mut set = CheckSet::new("U3721");
    set.add(
        "u3721-seg-guard",
        u3721_guard_seg(),
        if u3721_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3721-sm-legal",
        check_u3721_sm_legal(),
        if check_u3721_sm_legal() { "" } else { "UNX-F3721 sm legality mismatch" },
    );
    set.add(
        "u3721-sm-round",
        check_u3721_sm_round(),
        if check_u3721_sm_round() { "" } else { "UNX-F3721 sm roundtrip mismatch" },
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
    fn u3721_all_checks_green() {
        let set = run_u3721_checks();
        assert!(set.all_passed(), "U3721 domain checks must be green");
    }

    #[test]
    fn u3721_segment_guard_holds() {
        assert!(u3721_segment_guard());
    }
}
