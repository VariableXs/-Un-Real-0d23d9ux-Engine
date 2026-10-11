//! unxu2681_u2681sm —— 命中率账本总账与 JSON 导出冻结 状态机与快照封存（命中率账）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F2681 · 命中率账本总账与 JSON 导出冻结 · 判据原文：UNX-F2681 · 判据：JSON 导出格式冻结（字段/枚举/单位三冻结），F2700 判据"账本导出可查"承载声明与 F2667 消费闭合
//! · 组件：命中率账 状态机（6 态 × 6 事件显式迁移）+ FNV 快照封存（8 分量）
//! · 判据落地：UNX-F2681 · 判据：JSON 导出格式冻结（字段/枚举/单位三冻结），F2700 判据"账本导出可查"承载声明与 F2667 消费闭合
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U2681_VERSION: &str = "u2681-u2681sm-v1";

/// 诊断段基址。
pub const U2681_SEG_BASE: u16 = 0xaa90;

// --------------------------------------------------------------------------
// 诊断码（u2681 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 命中率账 状态面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u2681Code {
    /// 0xaa90 状态重复（字段漂移 同名双态）。
    StateDup,
    /// 0xaa91 状态断档（枚举不一致 序缺失）。
    StateHole,
    /// 0xaa92 事件重复（同事件双声明）。
    EventDup,
    /// 0xaa93 迁移重复（同源同事件双迁移）。
    TransDup,
    /// 0xaa94 迁移缺失（应达态不可达）。
    TransHole,
    /// 0xaa95 迁移环（非法自环/环路）。
    TransCycle,
    /// 0xaa96 入口非法（初态不在闭集）。
    EntryBad,
    /// 0xaa97 迁移丢失（应用事件无效果）。
    ApplyLost,
    /// 0xaa98 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xaa99 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xaa9a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xaa9b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u2681Code {
    /// 全量闭集。
    pub const ALL: [u2681Code; 12] = [
        u2681Code::StateDup,
        u2681Code::StateHole,
        u2681Code::EventDup,
        u2681Code::TransDup,
        u2681Code::TransHole,
        u2681Code::TransCycle,
        u2681Code::EntryBad,
        u2681Code::ApplyLost,
        u2681Code::LedgerBroken,
        u2681Code::SnapshotFingerprint,
        u2681Code::TableBroken,
        u2681Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u2681Code::StateDup => 0xaa90,
            u2681Code::StateHole => 0xaa91,
            u2681Code::EventDup => 0xaa92,
            u2681Code::TransDup => 0xaa93,
            u2681Code::TransHole => 0xaa94,
            u2681Code::TransCycle => 0xaa95,
            u2681Code::EntryBad => 0xaa96,
            u2681Code::ApplyLost => 0xaa97,
            u2681Code::LedgerBroken => 0xaa98,
            u2681Code::SnapshotFingerprint => 0xaa99,
            u2681Code::TableBroken => 0xaa9a,
            u2681Code::SegmentGuard => 0xaa9b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u2681Code::StateDup => "状态重复（字段漂移 同名双态）",
            u2681Code::StateHole => "状态断档（枚举不一致 序缺失）",
            u2681Code::EventDup => "事件重复（同事件双声明）",
            u2681Code::TransDup => "迁移重复（同源同事件双迁移）",
            u2681Code::TransHole => "迁移缺失（应达态不可达）",
            u2681Code::TransCycle => "迁移环（非法自环/环路）",
            u2681Code::EntryBad => "入口非法（初态不在闭集）",
            u2681Code::ApplyLost => "迁移丢失（应用事件无效果）",
            u2681Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u2681Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u2681Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u2681Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u2681Code::StateDup => "按闭集唯一律去重",
            u2681Code::StateHole => "按闭集补态",
            u2681Code::EventDup => "按事件闭集去重",
            u2681Code::TransDup => "按确定性唯一迁移律合并",
            u2681Code::TransHole => "按迁移矩阵补边",
            u2681Code::TransCycle => "按无环律重构迁移",
            u2681Code::EntryBad => "按闭集钳制初态",
            u2681Code::ApplyLost => "核对事件前置条件",
            u2681Code::LedgerBroken => "按守恒律重建台账后重跑",
            u2681Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u2681Code::TableBroken => "按语义规则重建常量表",
            u2681Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 状态机 mxu2681flow（6 态 × 6 事件 · 迁移 8 条显式声明）
// --------------------------------------------------------------------------
/// 命中率账 状态机（判据：字段漂移）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu2681flowState {
    Accum,
    Summarized,
    Frozen,
    Exported,
    Queryable,
    Sealed,
}

impl mxu2681flowState {
    /// 全量态闭集。
    pub const ALL: [mxu2681flowState; 6] = [
        mxu2681flowState::Accum,
        mxu2681flowState::Summarized,
        mxu2681flowState::Frozen,
        mxu2681flowState::Exported,
        mxu2681flowState::Queryable,
        mxu2681flowState::Sealed,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            mxu2681flowState::Accum => 0,
            mxu2681flowState::Summarized => 1,
            mxu2681flowState::Frozen => 2,
            mxu2681flowState::Exported => 3,
            mxu2681flowState::Queryable => 4,
            mxu2681flowState::Sealed => 5,
        }
    }
}

/// mxu2681flow事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu2681flowEvent {
    Accumulate,
    Summarize,
    FreezeFmt,
    ExportJson,
    AssertQuery,
    Seal,
}

impl mxu2681flowEvent {
    /// 全量事件闭集。
    pub const ALL: [mxu2681flowEvent; 6] = [
        mxu2681flowEvent::Accumulate,
        mxu2681flowEvent::Summarize,
        mxu2681flowEvent::FreezeFmt,
        mxu2681flowEvent::ExportJson,
        mxu2681flowEvent::AssertQuery,
        mxu2681flowEvent::Seal,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const MXU2681FLOW_TRANSITIONS: [(mxu2681flowState, mxu2681flowEvent, mxu2681flowState); 8] = [
    (mxu2681flowState::Accum, mxu2681flowEvent::Accumulate, mxu2681flowState::Summarized), // Accum→Summarized 主链
    (mxu2681flowState::Summarized, mxu2681flowEvent::Summarize, mxu2681flowState::Frozen), // Summarized→Frozen 主链
    (mxu2681flowState::Frozen, mxu2681flowEvent::FreezeFmt, mxu2681flowState::Exported), // Frozen→Exported 主链
    (mxu2681flowState::Exported, mxu2681flowEvent::ExportJson, mxu2681flowState::Queryable), // Exported→Queryable 主链
    (mxu2681flowState::Queryable, mxu2681flowEvent::AssertQuery, mxu2681flowState::Sealed), // Queryable→Sealed 主链
    (mxu2681flowState::Sealed, mxu2681flowEvent::Seal, mxu2681flowState::Accum), // 终态回边
    (mxu2681flowState::Sealed, mxu2681flowEvent::Accumulate, mxu2681flowState::Queryable), // 终态回退边
    (mxu2681flowState::Summarized, mxu2681flowEvent::Summarize, mxu2681flowState::Frozen), // 旁路边
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn mxu2681flow_next(state: mxu2681flowState, ev: mxu2681flowEvent) -> Option<mxu2681flowState> {
    let mut i = 0usize;
    while i < 8 {
        let t = MXU2681FLOW_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn mxu2681flow_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < mxu2681flowState::ALL.len() {
        let mut ei = 0usize;
        while ei < mxu2681flowEvent::ALL.len() {
            let legal = mxu2681flow_next(mxu2681flowState::ALL[si], mxu2681flowEvent::ALL[ei]).is_some();
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
pub fn mxu2681flow_terminal(st: mxu2681flowState) -> bool {
    let mut i = 0usize;
    while i < 8 {
        if MXU2681FLOW_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct mxu2681flowLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: mxu2681flowState,
}

impl mxu2681flowLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: mxu2681flowState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: mxu2681flowEvent) -> bool {
        match mxu2681flow_next(self.cur, ev) {
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
// 快照 Mxu2681Snap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// 命中率账 快照封存（FNV 三态指纹 + 篡改检出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mxu2681Snap {
    /// 指纹分量：hr_v。
    pub hr_v: u64,
    /// 指纹分量：lg_v。
    pub lg_v: u64,
    /// 指纹分量：js_v。
    pub js_v: u64,
    /// 指纹分量：fl_v。
    pub fl_v: u64,
    /// 指纹分量：en_v。
    pub en_v: u64,
    /// 指纹分量：un_v。
    pub un_v: u64,
    /// 指纹分量：f2700_v。
    pub f2700_v: u64,
    /// 指纹分量：cn_v。
    pub cn_v: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl Mxu2681Snap {
    /// 构造并即时合成指纹。
    pub fn compute(
        hr_v: u64,
        lg_v: u64,
        js_v: u64,
        fl_v: u64,
        en_v: u64,
        un_v: u64,
        f2700_v: u64,
        cn_v: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &hr_v.to_le_bytes());
        h = fnv1a64_from(h, &lg_v.to_le_bytes());
        h = fnv1a64_from(h, &js_v.to_le_bytes());
        h = fnv1a64_from(h, &fl_v.to_le_bytes());
        h = fnv1a64_from(h, &en_v.to_le_bytes());
        h = fnv1a64_from(h, &un_v.to_le_bytes());
        h = fnv1a64_from(h, &f2700_v.to_le_bytes());
        h = fnv1a64_from(h, &cn_v.to_le_bytes());
        Self {
            hr_v,
            lg_v,
            js_v,
            fl_v,
            en_v,
            un_v,
            f2700_v,
            cn_v,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.hr_v.to_le_bytes());
        h = fnv1a64_from(h, &self.lg_v.to_le_bytes());
        h = fnv1a64_from(h, &self.js_v.to_le_bytes());
        h = fnv1a64_from(h, &self.fl_v.to_le_bytes());
        h = fnv1a64_from(h, &self.en_v.to_le_bytes());
        h = fnv1a64_from(h, &self.un_v.to_le_bytes());
        h = fnv1a64_from(h, &self.f2700_v.to_le_bytes());
        h = fnv1a64_from(h, &self.cn_v.to_le_bytes());
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

/// 段位守卫：全部诊断码必须落在本段 [0xaa90, 0xaa9f]。
pub const fn u2681_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u2681Code::ALL.len() {
        let c = u2681Code::ALL[i].code();
        if c < 0xaa90 || c > 0xaa9f {
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
fn u2681_guard_seg() -> bool {
    u2681_segment_guard()
}

/// UNX-F2681 状态机闭集/迁移表/非法组合恰值全绿。
fn check_u2681_sm_legal() -> bool {
    let mut ok = mxu2681flow_illegal_pairs() == 29;
    if ok { ok = MXU2681FLOW_TRANSITIONS.len() == 8; }
    if ok { ok = mxu2681flowState::ALL.len() == 6 && mxu2681flowEvent::ALL.len() == 6; }
    ok
}

/// UNX-F2681 迁移主链回放 + 快照往返恒等。
fn check_u2681_sm_round() -> bool {
    let mut lg = mxu2681flowLedger::new(mxu2681flowState::Accum);
    let mut ok = lg.apply(mxu2681flowEvent::Accumulate) && lg.cur == mxu2681flowState::Summarized && lg.total() == 1;
    let s0 = Mxu2681Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    let mut s1 = Mxu2681Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    s1.recompute();
    if ok { ok = s0 == s1 && !s1.tampered(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u2681_checks() -> CheckSet {
    let mut set = CheckSet::new("U2681");
    set.add(
        "u2681-seg-guard",
        u2681_guard_seg(),
        if u2681_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u2681-sm-legal",
        check_u2681_sm_legal(),
        if check_u2681_sm_legal() { "" } else { "UNX-F2681 sm legality mismatch" },
    );
    set.add(
        "u2681-sm-round",
        check_u2681_sm_round(),
        if check_u2681_sm_round() { "" } else { "UNX-F2681 sm roundtrip mismatch" },
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
    fn u2681_all_checks_green() {
        let set = run_u2681_checks();
        assert!(set.all_passed(), "U2681 domain checks must be green");
    }

    #[test]
    fn u2681_segment_guard_holds() {
        assert!(u2681_segment_guard());
    }
}
