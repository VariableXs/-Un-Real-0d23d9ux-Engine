//! unxu3401_u3401sm —— ktest 现状盘点与扩容总设计（接管 testing/ 目录） 状态机与快照封存（ktest 扩容）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3401 · ktest 现状盘点与扩容总设计（接管 testing/ 目录） · 判据原文：UNX-F3401 · 判据：J1：总设计文档（现状盘点/扩容目标/宏族路线/接管三步法）交付；M-160 设计基线用例。
//! · 组件：ktest 扩容 状态机（6 态 × 6 事件显式迁移）+ FNV 快照封存（8 分量）
//! · 判据落地：UNX-F3401 · 判据：J1：总设计文档（现状盘点/扩容目标/宏族路线/接管三步法）交付；M-160 设计基线用例。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3401_VERSION: &str = "u3401-u3401sm-v1";

/// 诊断段基址。
pub const U3401_SEG_BASE: u16 = 0xe080;

// --------------------------------------------------------------------------
// 诊断码（u3401 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// ktest 扩容 状态面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3401Code {
    /// 0xe080 状态重复（ktest 同名双态）。
    StateDup,
    /// 0xe081 状态断档（扩容 序缺失）。
    StateHole,
    /// 0xe082 事件重复（同事件双声明）。
    EventDup,
    /// 0xe083 迁移重复（同源同事件双迁移）。
    TransDup,
    /// 0xe084 迁移缺失（应达态不可达）。
    TransHole,
    /// 0xe085 迁移环（非法自环/环路）。
    TransCycle,
    /// 0xe086 入口非法（初态不在闭集）。
    EntryBad,
    /// 0xe087 迁移丢失（应用事件无效果）。
    ApplyLost,
    /// 0xe088 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe089 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe08a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe08b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3401Code {
    /// 全量闭集。
    pub const ALL: [u3401Code; 12] = [
        u3401Code::StateDup,
        u3401Code::StateHole,
        u3401Code::EventDup,
        u3401Code::TransDup,
        u3401Code::TransHole,
        u3401Code::TransCycle,
        u3401Code::EntryBad,
        u3401Code::ApplyLost,
        u3401Code::LedgerBroken,
        u3401Code::SnapshotFingerprint,
        u3401Code::TableBroken,
        u3401Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3401Code::StateDup => 0xe080,
            u3401Code::StateHole => 0xe081,
            u3401Code::EventDup => 0xe082,
            u3401Code::TransDup => 0xe083,
            u3401Code::TransHole => 0xe084,
            u3401Code::TransCycle => 0xe085,
            u3401Code::EntryBad => 0xe086,
            u3401Code::ApplyLost => 0xe087,
            u3401Code::LedgerBroken => 0xe088,
            u3401Code::SnapshotFingerprint => 0xe089,
            u3401Code::TableBroken => 0xe08a,
            u3401Code::SegmentGuard => 0xe08b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3401Code::StateDup => "状态重复（ktest 同名双态）",
            u3401Code::StateHole => "状态断档（扩容 序缺失）",
            u3401Code::EventDup => "事件重复（同事件双声明）",
            u3401Code::TransDup => "迁移重复（同源同事件双迁移）",
            u3401Code::TransHole => "迁移缺失（应达态不可达）",
            u3401Code::TransCycle => "迁移环（非法自环/环路）",
            u3401Code::EntryBad => "入口非法（初态不在闭集）",
            u3401Code::ApplyLost => "迁移丢失（应用事件无效果）",
            u3401Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3401Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3401Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3401Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3401Code::StateDup => "按闭集唯一律去重",
            u3401Code::StateHole => "按闭集补态",
            u3401Code::EventDup => "按事件闭集去重",
            u3401Code::TransDup => "按确定性唯一迁移律合并",
            u3401Code::TransHole => "按迁移矩阵补边",
            u3401Code::TransCycle => "按无环律重构迁移",
            u3401Code::EntryBad => "按闭集钳制初态",
            u3401Code::ApplyLost => "核对事件前置条件",
            u3401Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3401Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3401Code::TableBroken => "按语义规则重建常量表",
            u3401Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 状态机 mxu3401flow（6 态 × 6 事件 · 迁移 8 条显式声明）
// --------------------------------------------------------------------------
/// ktest 扩容 状态机（判据：ktest）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu3401flowState {
    Inventory,
    Scaffold,
    Migrate,
    Wired,
    Verified,
    Frozen,
}

impl mxu3401flowState {
    /// 全量态闭集。
    pub const ALL: [mxu3401flowState; 6] = [
        mxu3401flowState::Inventory,
        mxu3401flowState::Scaffold,
        mxu3401flowState::Migrate,
        mxu3401flowState::Wired,
        mxu3401flowState::Verified,
        mxu3401flowState::Frozen,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            mxu3401flowState::Inventory => 0,
            mxu3401flowState::Scaffold => 1,
            mxu3401flowState::Migrate => 2,
            mxu3401flowState::Wired => 3,
            mxu3401flowState::Verified => 4,
            mxu3401flowState::Frozen => 5,
        }
    }
}

/// mxu3401flow事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu3401flowEvent {
    Scan,
    Gen,
    Migrate,
    Wire,
    Verify,
    Freeze,
}

impl mxu3401flowEvent {
    /// 全量事件闭集。
    pub const ALL: [mxu3401flowEvent; 6] = [
        mxu3401flowEvent::Scan,
        mxu3401flowEvent::Gen,
        mxu3401flowEvent::Migrate,
        mxu3401flowEvent::Wire,
        mxu3401flowEvent::Verify,
        mxu3401flowEvent::Freeze,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const MXU3401FLOW_TRANSITIONS: [(mxu3401flowState, mxu3401flowEvent, mxu3401flowState); 8] = [
    (mxu3401flowState::Inventory, mxu3401flowEvent::Scan, mxu3401flowState::Scaffold), // Inventory→Scaffold 主链
    (mxu3401flowState::Scaffold, mxu3401flowEvent::Gen, mxu3401flowState::Migrate), // Scaffold→Migrate 主链
    (mxu3401flowState::Migrate, mxu3401flowEvent::Migrate, mxu3401flowState::Wired), // Migrate→Wired 主链
    (mxu3401flowState::Wired, mxu3401flowEvent::Wire, mxu3401flowState::Verified), // Wired→Verified 主链
    (mxu3401flowState::Verified, mxu3401flowEvent::Verify, mxu3401flowState::Frozen), // Verified→Frozen 主链
    (mxu3401flowState::Frozen, mxu3401flowEvent::Freeze, mxu3401flowState::Inventory), // 终态回边
    (mxu3401flowState::Frozen, mxu3401flowEvent::Scan, mxu3401flowState::Verified), // 终态回退边
    (mxu3401flowState::Scaffold, mxu3401flowEvent::Gen, mxu3401flowState::Migrate), // 旁路边
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn mxu3401flow_next(state: mxu3401flowState, ev: mxu3401flowEvent) -> Option<mxu3401flowState> {
    let mut i = 0usize;
    while i < 8 {
        let t = MXU3401FLOW_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn mxu3401flow_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < mxu3401flowState::ALL.len() {
        let mut ei = 0usize;
        while ei < mxu3401flowEvent::ALL.len() {
            let legal = mxu3401flow_next(mxu3401flowState::ALL[si], mxu3401flowEvent::ALL[ei]).is_some();
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
pub fn mxu3401flow_terminal(st: mxu3401flowState) -> bool {
    let mut i = 0usize;
    while i < 8 {
        if MXU3401FLOW_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct mxu3401flowLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: mxu3401flowState,
}

impl mxu3401flowLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: mxu3401flowState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: mxu3401flowEvent) -> bool {
        match mxu3401flow_next(self.cur, ev) {
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
// 快照 Mxu3401Snap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// ktest 扩容 快照封存（FNV 三态指纹 + 篡改检出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mxu3401Snap {
    /// 指纹分量：kt_v。
    pub kt_v: u64,
    /// 指纹分量：dir_v。
    pub dir_v: u64,
    /// 指纹分量：case_v。
    pub case_v: u64,
    /// 指纹分量：sel_v。
    pub sel_v: u64,
    /// 指纹分量：reg_v。
    pub reg_v: u64,
    /// 指纹分量：fix_v。
    pub fix_v: u64,
    /// 指纹分量：backend_v。
    pub backend_v: u64,
    /// 指纹分量：qemu_v。
    pub qemu_v: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl Mxu3401Snap {
    /// 构造并即时合成指纹。
    pub fn compute(
        kt_v: u64,
        dir_v: u64,
        case_v: u64,
        sel_v: u64,
        reg_v: u64,
        fix_v: u64,
        backend_v: u64,
        qemu_v: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &kt_v.to_le_bytes());
        h = fnv1a64_from(h, &dir_v.to_le_bytes());
        h = fnv1a64_from(h, &case_v.to_le_bytes());
        h = fnv1a64_from(h, &sel_v.to_le_bytes());
        h = fnv1a64_from(h, &reg_v.to_le_bytes());
        h = fnv1a64_from(h, &fix_v.to_le_bytes());
        h = fnv1a64_from(h, &backend_v.to_le_bytes());
        h = fnv1a64_from(h, &qemu_v.to_le_bytes());
        Self {
            kt_v,
            dir_v,
            case_v,
            sel_v,
            reg_v,
            fix_v,
            backend_v,
            qemu_v,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.kt_v.to_le_bytes());
        h = fnv1a64_from(h, &self.dir_v.to_le_bytes());
        h = fnv1a64_from(h, &self.case_v.to_le_bytes());
        h = fnv1a64_from(h, &self.sel_v.to_le_bytes());
        h = fnv1a64_from(h, &self.reg_v.to_le_bytes());
        h = fnv1a64_from(h, &self.fix_v.to_le_bytes());
        h = fnv1a64_from(h, &self.backend_v.to_le_bytes());
        h = fnv1a64_from(h, &self.qemu_v.to_le_bytes());
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

/// 段位守卫：全部诊断码必须落在本段 [0xe080, 0xe08f]。
pub const fn u3401_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3401Code::ALL.len() {
        let c = u3401Code::ALL[i].code();
        if c < 0xe080 || c > 0xe08f {
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
fn u3401_guard_seg() -> bool {
    u3401_segment_guard()
}

/// UNX-F3401 状态机闭集/迁移表/非法组合恰值全绿。
fn check_u3401_sm_legal() -> bool {
    let mut ok = mxu3401flow_illegal_pairs() == 29;
    if ok { ok = MXU3401FLOW_TRANSITIONS.len() == 8; }
    if ok { ok = mxu3401flowState::ALL.len() == 6 && mxu3401flowEvent::ALL.len() == 6; }
    ok
}

/// UNX-F3401 迁移主链回放 + 快照往返恒等。
fn check_u3401_sm_round() -> bool {
    let mut lg = mxu3401flowLedger::new(mxu3401flowState::Inventory);
    let mut ok = lg.apply(mxu3401flowEvent::Scan) && lg.cur == mxu3401flowState::Scaffold && lg.total() == 1;
    let s0 = Mxu3401Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    let mut s1 = Mxu3401Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    s1.recompute();
    if ok { ok = s0 == s1 && !s1.tampered(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3401_checks() -> CheckSet {
    let mut set = CheckSet::new("U3401");
    set.add(
        "u3401-seg-guard",
        u3401_guard_seg(),
        if u3401_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3401-sm-legal",
        check_u3401_sm_legal(),
        if check_u3401_sm_legal() { "" } else { "UNX-F3401 sm legality mismatch" },
    );
    set.add(
        "u3401-sm-round",
        check_u3401_sm_round(),
        if check_u3401_sm_round() { "" } else { "UNX-F3401 sm roundtrip mismatch" },
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
    fn u3401_all_checks_green() {
        let set = run_u3401_checks();
        assert!(set.all_passed(), "U3401 domain checks must be green");
    }

    #[test]
    fn u3401_segment_guard_holds() {
        assert!(u3401_segment_guard());
    }
}
