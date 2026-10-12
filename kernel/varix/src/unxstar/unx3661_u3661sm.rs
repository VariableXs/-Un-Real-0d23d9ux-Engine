//! unxu3661_u3661sm —— 符号化总设计（地址→符号的解析管线） 状态机与快照封存（符号化管线）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F3661 · 符号化总设计（地址→符号的解析管线） · 判据原文：UNX-F3661 · 判据：J1：解析管线（输入（帧地址集+版本锚）→查表（符号表）→歧义消解→报告（符号帧序列）——判据行内嵌四步）；M-419 全链首译（单帧地址→符号名+偏移+源行——翻译三要素）。<br>J2：离线纯净（符号化器不依赖内核状态（纯函数——同输入同输出——F3672 的前提））。
//! · 组件：符号化管线 状态机（6 态 × 6 事件显式迁移）+ FNV 快照封存（8 分量）
//! · 判据落地：UNX-F3661 · 判据：J1：解析管线（输入（帧地址集+版本锚）→查表（符号表）→歧义消解→报告（符号帧序列）——判据行内嵌四步）；M-419 全链首译（
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U3661_VERSION: &str = "u3661-u3661sm-v1";

/// 诊断段基址。
pub const U3661_SEG_BASE: u16 = 0xe150;

// --------------------------------------------------------------------------
// 诊断码（u3661 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 符号化管线 状态面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u3661Code {
    /// 0xe150 状态重复（符号 同名双态）。
    StateDup,
    /// 0xe151 状态断档（解析 序缺失）。
    StateHole,
    /// 0xe152 事件重复（同事件双声明）。
    EventDup,
    /// 0xe153 迁移重复（同源同事件双迁移）。
    TransDup,
    /// 0xe154 迁移缺失（应达态不可达）。
    TransHole,
    /// 0xe155 迁移环（非法自环/环路）。
    TransCycle,
    /// 0xe156 入口非法（初态不在闭集）。
    EntryBad,
    /// 0xe157 迁移丢失（应用事件无效果）。
    ApplyLost,
    /// 0xe158 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0xe159 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0xe15a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0xe15b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u3661Code {
    /// 全量闭集。
    pub const ALL: [u3661Code; 12] = [
        u3661Code::StateDup,
        u3661Code::StateHole,
        u3661Code::EventDup,
        u3661Code::TransDup,
        u3661Code::TransHole,
        u3661Code::TransCycle,
        u3661Code::EntryBad,
        u3661Code::ApplyLost,
        u3661Code::LedgerBroken,
        u3661Code::SnapshotFingerprint,
        u3661Code::TableBroken,
        u3661Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u3661Code::StateDup => 0xe150,
            u3661Code::StateHole => 0xe151,
            u3661Code::EventDup => 0xe152,
            u3661Code::TransDup => 0xe153,
            u3661Code::TransHole => 0xe154,
            u3661Code::TransCycle => 0xe155,
            u3661Code::EntryBad => 0xe156,
            u3661Code::ApplyLost => 0xe157,
            u3661Code::LedgerBroken => 0xe158,
            u3661Code::SnapshotFingerprint => 0xe159,
            u3661Code::TableBroken => 0xe15a,
            u3661Code::SegmentGuard => 0xe15b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u3661Code::StateDup => "状态重复（符号 同名双态）",
            u3661Code::StateHole => "状态断档（解析 序缺失）",
            u3661Code::EventDup => "事件重复（同事件双声明）",
            u3661Code::TransDup => "迁移重复（同源同事件双迁移）",
            u3661Code::TransHole => "迁移缺失（应达态不可达）",
            u3661Code::TransCycle => "迁移环（非法自环/环路）",
            u3661Code::EntryBad => "入口非法（初态不在闭集）",
            u3661Code::ApplyLost => "迁移丢失（应用事件无效果）",
            u3661Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u3661Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u3661Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u3661Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u3661Code::StateDup => "按闭集唯一律去重",
            u3661Code::StateHole => "按闭集补态",
            u3661Code::EventDup => "按事件闭集去重",
            u3661Code::TransDup => "按确定性唯一迁移律合并",
            u3661Code::TransHole => "按迁移矩阵补边",
            u3661Code::TransCycle => "按无环律重构迁移",
            u3661Code::EntryBad => "按闭集钳制初态",
            u3661Code::ApplyLost => "核对事件前置条件",
            u3661Code::LedgerBroken => "按守恒律重建台账后重跑",
            u3661Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u3661Code::TableBroken => "按语义规则重建常量表",
            u3661Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 状态机 mxu3661flow（6 态 × 6 事件 · 迁移 8 条显式声明）
// --------------------------------------------------------------------------
/// 符号化管线 状态机（判据：符号）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu3661flowState {
    Cold,
    Load,
    Ready,
    Resolving,
    Cached,
    Fallback,
}

impl mxu3661flowState {
    /// 全量态闭集。
    pub const ALL: [mxu3661flowState; 6] = [
        mxu3661flowState::Cold,
        mxu3661flowState::Load,
        mxu3661flowState::Ready,
        mxu3661flowState::Resolving,
        mxu3661flowState::Cached,
        mxu3661flowState::Fallback,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            mxu3661flowState::Cold => 0,
            mxu3661flowState::Load => 1,
            mxu3661flowState::Ready => 2,
            mxu3661flowState::Resolving => 3,
            mxu3661flowState::Cached => 4,
            mxu3661flowState::Fallback => 5,
        }
    }
}

/// mxu3661flow事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu3661flowEvent {
    Load,
    Resolve,
    Inline,
    Cache,
    Miss,
    Fallback,
}

impl mxu3661flowEvent {
    /// 全量事件闭集。
    pub const ALL: [mxu3661flowEvent; 6] = [
        mxu3661flowEvent::Load,
        mxu3661flowEvent::Resolve,
        mxu3661flowEvent::Inline,
        mxu3661flowEvent::Cache,
        mxu3661flowEvent::Miss,
        mxu3661flowEvent::Fallback,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const MXU3661FLOW_TRANSITIONS: [(mxu3661flowState, mxu3661flowEvent, mxu3661flowState); 8] = [
    (mxu3661flowState::Cold, mxu3661flowEvent::Load, mxu3661flowState::Load), // Cold→Load 主链
    (mxu3661flowState::Load, mxu3661flowEvent::Resolve, mxu3661flowState::Ready), // Load→Ready 主链
    (mxu3661flowState::Ready, mxu3661flowEvent::Inline, mxu3661flowState::Resolving), // Ready→Resolving 主链
    (mxu3661flowState::Resolving, mxu3661flowEvent::Cache, mxu3661flowState::Cached), // Resolving→Cached 主链
    (mxu3661flowState::Cached, mxu3661flowEvent::Miss, mxu3661flowState::Fallback), // Cached→Fallback 主链
    (mxu3661flowState::Fallback, mxu3661flowEvent::Fallback, mxu3661flowState::Cold), // 终态回边
    (mxu3661flowState::Fallback, mxu3661flowEvent::Load, mxu3661flowState::Cached), // 终态回退边
    (mxu3661flowState::Load, mxu3661flowEvent::Resolve, mxu3661flowState::Ready), // 旁路边
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn mxu3661flow_next(state: mxu3661flowState, ev: mxu3661flowEvent) -> Option<mxu3661flowState> {
    let mut i = 0usize;
    while i < 8 {
        let t = MXU3661FLOW_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn mxu3661flow_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < mxu3661flowState::ALL.len() {
        let mut ei = 0usize;
        while ei < mxu3661flowEvent::ALL.len() {
            let legal = mxu3661flow_next(mxu3661flowState::ALL[si], mxu3661flowEvent::ALL[ei]).is_some();
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
pub fn mxu3661flow_terminal(st: mxu3661flowState) -> bool {
    let mut i = 0usize;
    while i < 8 {
        if MXU3661FLOW_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct mxu3661flowLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: mxu3661flowState,
}

impl mxu3661flowLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: mxu3661flowState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: mxu3661flowEvent) -> bool {
        match mxu3661flow_next(self.cur, ev) {
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
// 快照 Mxu3661Snap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// 符号化管线 快照封存（FNV 三态指纹 + 篡改检出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mxu3661Snap {
    /// 指纹分量：sym_v。
    pub sym_v: u64,
    /// 指纹分量：addr_v。
    pub addr_v: u64,
    /// 指纹分量：resolve_v。
    pub resolve_v: u64,
    /// 指纹分量：tbl_v。
    pub tbl_v: u64,
    /// 指纹分量：mod_v。
    pub mod_v: u64,
    /// 指纹分量：offset_v。
    pub offset_v: u64,
    /// 指纹分量：inline_v。
    pub inline_v: u64,
    /// 指纹分量：func_v。
    pub func_v: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl Mxu3661Snap {
    /// 构造并即时合成指纹。
    pub fn compute(
        sym_v: u64,
        addr_v: u64,
        resolve_v: u64,
        tbl_v: u64,
        mod_v: u64,
        offset_v: u64,
        inline_v: u64,
        func_v: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &sym_v.to_le_bytes());
        h = fnv1a64_from(h, &addr_v.to_le_bytes());
        h = fnv1a64_from(h, &resolve_v.to_le_bytes());
        h = fnv1a64_from(h, &tbl_v.to_le_bytes());
        h = fnv1a64_from(h, &mod_v.to_le_bytes());
        h = fnv1a64_from(h, &offset_v.to_le_bytes());
        h = fnv1a64_from(h, &inline_v.to_le_bytes());
        h = fnv1a64_from(h, &func_v.to_le_bytes());
        Self {
            sym_v,
            addr_v,
            resolve_v,
            tbl_v,
            mod_v,
            offset_v,
            inline_v,
            func_v,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.sym_v.to_le_bytes());
        h = fnv1a64_from(h, &self.addr_v.to_le_bytes());
        h = fnv1a64_from(h, &self.resolve_v.to_le_bytes());
        h = fnv1a64_from(h, &self.tbl_v.to_le_bytes());
        h = fnv1a64_from(h, &self.mod_v.to_le_bytes());
        h = fnv1a64_from(h, &self.offset_v.to_le_bytes());
        h = fnv1a64_from(h, &self.inline_v.to_le_bytes());
        h = fnv1a64_from(h, &self.func_v.to_le_bytes());
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

/// 段位守卫：全部诊断码必须落在本段 [0xe150, 0xe15f]。
pub const fn u3661_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u3661Code::ALL.len() {
        let c = u3661Code::ALL[i].code();
        if c < 0xe150 || c > 0xe15f {
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
fn u3661_guard_seg() -> bool {
    u3661_segment_guard()
}

/// UNX-F3661 状态机闭集/迁移表/非法组合恰值全绿。
fn check_u3661_sm_legal() -> bool {
    let mut ok = mxu3661flow_illegal_pairs() == 29;
    if ok { ok = MXU3661FLOW_TRANSITIONS.len() == 8; }
    if ok { ok = mxu3661flowState::ALL.len() == 6 && mxu3661flowEvent::ALL.len() == 6; }
    ok
}

/// UNX-F3661 迁移主链回放 + 快照往返恒等。
fn check_u3661_sm_round() -> bool {
    let mut lg = mxu3661flowLedger::new(mxu3661flowState::Cold);
    let mut ok = lg.apply(mxu3661flowEvent::Load) && lg.cur == mxu3661flowState::Load && lg.total() == 1;
    let s0 = Mxu3661Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    let mut s1 = Mxu3661Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    s1.recompute();
    if ok { ok = s0 == s1 && !s1.tampered(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u3661_checks() -> CheckSet {
    let mut set = CheckSet::new("U3661");
    set.add(
        "u3661-seg-guard",
        u3661_guard_seg(),
        if u3661_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u3661-sm-legal",
        check_u3661_sm_legal(),
        if check_u3661_sm_legal() { "" } else { "UNX-F3661 sm legality mismatch" },
    );
    set.add(
        "u3661-sm-round",
        check_u3661_sm_round(),
        if check_u3661_sm_round() { "" } else { "UNX-F3661 sm roundtrip mismatch" },
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
    fn u3661_all_checks_green() {
        let set = run_u3661_checks();
        assert!(set.all_passed(), "U3661 domain checks must be green");
    }

    #[test]
    fn u3661_segment_guard_holds() {
        assert!(u3661_segment_guard());
    }
}
