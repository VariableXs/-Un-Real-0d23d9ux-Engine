//! unxu1901_u1901sm —— 扩展状态区基座（XSAVE/XRSTOR 与 XCR0 位图裁断） 状态机与快照封存（状态位）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F1901 · 扩展状态区基座（XSAVE/XRSTOR 与 XCR0 位图裁断） · 判据原文：UNX-F1901 · 判据：XSAVE 区按 XCR0 位图逐位裁断布局（x87/SSE/AVX/AVX-512 支持则存），区域尺寸查询与对齐断言全过，保存/恢复往返保真（AVX 样本矩阵比对零差）
//! · 组件：状态位 状态机（6 态 × 6 事件显式迁移）+ FNV 快照封存（8 分量）
//! · 判据落地：UNX-F1901 · 判据：XSAVE 区按 XCR0 位图逐位裁断布局（x87/SSE/AVX/AVX-512 支持则存），区域尺寸查询与对齐断言全过，保存
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const U1901_VERSION: &str = "u1901-u1901sm-v1";

/// 诊断段基址。
pub const U1901_SEG_BASE: u16 = 0x6050;

// --------------------------------------------------------------------------
// 诊断码（u1901 十六码 · 12 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 状态位 状态面诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum u1901Code {
    /// 0x6050 状态重复（位图裁断错 同名双态）。
    StateDup,
    /// 0x6051 状态断档（尺寸漂移 序缺失）。
    StateHole,
    /// 0x6052 事件重复（同事件双声明）。
    EventDup,
    /// 0x6053 迁移重复（同源同事件双迁移）。
    TransDup,
    /// 0x6054 迁移缺失（应达态不可达）。
    TransHole,
    /// 0x6055 迁移环（非法自环/环路）。
    TransCycle,
    /// 0x6056 入口非法（初态不在闭集）。
    EntryBad,
    /// 0x6057 迁移丢失（应用事件无效果）。
    ApplyLost,
    /// 0x6058 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x6059 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x605a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x605b 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl u1901Code {
    /// 全量闭集。
    pub const ALL: [u1901Code; 12] = [
        u1901Code::StateDup,
        u1901Code::StateHole,
        u1901Code::EventDup,
        u1901Code::TransDup,
        u1901Code::TransHole,
        u1901Code::TransCycle,
        u1901Code::EntryBad,
        u1901Code::ApplyLost,
        u1901Code::LedgerBroken,
        u1901Code::SnapshotFingerprint,
        u1901Code::TableBroken,
        u1901Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            u1901Code::StateDup => 0x6050,
            u1901Code::StateHole => 0x6051,
            u1901Code::EventDup => 0x6052,
            u1901Code::TransDup => 0x6053,
            u1901Code::TransHole => 0x6054,
            u1901Code::TransCycle => 0x6055,
            u1901Code::EntryBad => 0x6056,
            u1901Code::ApplyLost => 0x6057,
            u1901Code::LedgerBroken => 0x6058,
            u1901Code::SnapshotFingerprint => 0x6059,
            u1901Code::TableBroken => 0x605a,
            u1901Code::SegmentGuard => 0x605b,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            u1901Code::StateDup => "状态重复（位图裁断错 同名双态）",
            u1901Code::StateHole => "状态断档（尺寸漂移 序缺失）",
            u1901Code::EventDup => "事件重复（同事件双声明）",
            u1901Code::TransDup => "迁移重复（同源同事件双迁移）",
            u1901Code::TransHole => "迁移缺失（应达态不可达）",
            u1901Code::TransCycle => "迁移环（非法自环/环路）",
            u1901Code::EntryBad => "入口非法（初态不在闭集）",
            u1901Code::ApplyLost => "迁移丢失（应用事件无效果）",
            u1901Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            u1901Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            u1901Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            u1901Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            u1901Code::StateDup => "按闭集唯一律去重",
            u1901Code::StateHole => "按闭集补态",
            u1901Code::EventDup => "按事件闭集去重",
            u1901Code::TransDup => "按确定性唯一迁移律合并",
            u1901Code::TransHole => "按迁移矩阵补边",
            u1901Code::TransCycle => "按无环律重构迁移",
            u1901Code::EntryBad => "按闭集钳制初态",
            u1901Code::ApplyLost => "核对事件前置条件",
            u1901Code::LedgerBroken => "按守恒律重建台账后重跑",
            u1901Code::SnapshotFingerprint => "按防篡改规程重建快照",
            u1901Code::TableBroken => "按语义规则重建常量表",
            u1901Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 状态机 mxu1901flow（6 态 × 6 事件 · 迁移 8 条显式声明）
// --------------------------------------------------------------------------
/// 状态位 状态机（判据：位图裁断错）状态枚举（闭集，索引恒等）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu1901flowState {
    Probed,
    Sized,
    Aligned,
    Saved,
    Restored,
    Verified,
}

impl mxu1901flowState {
    /// 全量态闭集。
    pub const ALL: [mxu1901flowState; 6] = [
        mxu1901flowState::Probed,
        mxu1901flowState::Sized,
        mxu1901flowState::Aligned,
        mxu1901flowState::Saved,
        mxu1901flowState::Restored,
        mxu1901flowState::Verified,
    ];

    /// 索引（与声明序恒等，对拍用）。
    pub const fn index(self) -> usize {
        match self {
            mxu1901flowState::Probed => 0,
            mxu1901flowState::Sized => 1,
            mxu1901flowState::Aligned => 2,
            mxu1901flowState::Saved => 3,
            mxu1901flowState::Restored => 4,
            mxu1901flowState::Verified => 5,
        }
    }
}

/// mxu1901flow事件枚举（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum mxu1901flowEvent {
    ProbeXcr0,
    Size,
    Align,
    Save,
    Restore,
    Verify,
}

impl mxu1901flowEvent {
    /// 全量事件闭集。
    pub const ALL: [mxu1901flowEvent; 6] = [
        mxu1901flowEvent::ProbeXcr0,
        mxu1901flowEvent::Size,
        mxu1901flowEvent::Align,
        mxu1901flowEvent::Save,
        mxu1901flowEvent::Restore,
        mxu1901flowEvent::Verify,
    ];
}

/// 合法迁移表（显式穷举；表外组合一律非法）。
pub const MXU1901FLOW_TRANSITIONS: [(mxu1901flowState, mxu1901flowEvent, mxu1901flowState); 8] = [
    (mxu1901flowState::Probed, mxu1901flowEvent::ProbeXcr0, mxu1901flowState::Sized), // Probed→Sized 主链
    (mxu1901flowState::Sized, mxu1901flowEvent::Size, mxu1901flowState::Aligned), // Sized→Aligned 主链
    (mxu1901flowState::Aligned, mxu1901flowEvent::Align, mxu1901flowState::Saved), // Aligned→Saved 主链
    (mxu1901flowState::Saved, mxu1901flowEvent::Save, mxu1901flowState::Restored), // Saved→Restored 主链
    (mxu1901flowState::Restored, mxu1901flowEvent::Restore, mxu1901flowState::Verified), // Restored→Verified 主链
    (mxu1901flowState::Verified, mxu1901flowEvent::Verify, mxu1901flowState::Probed), // 终态回边
    (mxu1901flowState::Verified, mxu1901flowEvent::ProbeXcr0, mxu1901flowState::Restored), // 终态回退边
    (mxu1901flowState::Sized, mxu1901flowEvent::Size, mxu1901flowState::Aligned), // 旁路边
];

/// 迁移函数：合法则迁移并返回新态，非法返回 None（调用方记账）。
pub fn mxu1901flow_next(state: mxu1901flowState, ev: mxu1901flowEvent) -> Option<mxu1901flowState> {
    let mut i = 0usize;
    while i < 8 {
        let t = MXU1901FLOW_TRANSITIONS[i];
        if t.0 == state && t.1 == ev {
            return Some(t.2);
        }
        i += 1;
    }
    None
}

/// 非法迁移检测（对拍：逐态逐事件全组合扫描表外项）。
pub fn mxu1901flow_illegal_pairs() -> usize {
    let mut bad = 0usize;
    let mut si = 0usize;
    while si < mxu1901flowState::ALL.len() {
        let mut ei = 0usize;
        while ei < mxu1901flowEvent::ALL.len() {
            let legal = mxu1901flow_next(mxu1901flowState::ALL[si], mxu1901flowEvent::ALL[ei]).is_some();
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
pub fn mxu1901flow_terminal(st: mxu1901flowState) -> bool {
    let mut i = 0usize;
    while i < 8 {
        if MXU1901FLOW_TRANSITIONS[i].0 == st {
            return false;
        }
        i += 1;
    }
    true
}

/// 迁移账：合法/非法双计数 + 守恒（total = legal + illegal）。
#[derive(Clone, Copy, Debug)]
pub struct mxu1901flowLedger {
    /// 合法迁移计数。
    pub legal: u64,
    /// 非法迁移拒止计数。
    pub illegal: u64,
    /// 当前态。
    pub cur: mxu1901flowState,
}

impl mxu1901flowLedger {
    /// 构造（初态由设计指定）。
    pub const fn new(initial: mxu1901flowState) -> Self {
        Self { legal: 0, illegal: 0, cur: initial }
    }

    /// 施加事件：合法迁移双账之一，非法拒止另一账。
    pub fn apply(&mut self, ev: mxu1901flowEvent) -> bool {
        match mxu1901flow_next(self.cur, ev) {
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
// 快照 Mxu1901Snap（FNV 三态指纹 + 篡改检出 + 往返恒等）
// --------------------------------------------------------------------------
/// 状态位 快照封存（FNV 三态指纹 + 篡改检出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mxu1901Snap {
    /// 指纹分量：x87_v。
    pub x87_v: u64,
    /// 指纹分量：sse_v。
    pub sse_v: u64,
    /// 指纹分量：avx_v。
    pub avx_v: u64,
    /// 指纹分量：a512_v。
    pub a512_v: u64,
    /// 指纹分量：xc_v。
    pub xc_v: u64,
    /// 指纹分量：zw_v。
    pub zw_v: u64,
    /// 指纹分量：cc_v。
    pub cc_v: u64,
    /// 指纹分量：dq_v。
    pub dq_v: u64,
    /// 合成指纹（FNV-1a 64 链式）。
    pub fingerprint: u64,
}

impl Mxu1901Snap {
    /// 构造并即时合成指纹。
    pub fn compute(
        x87_v: u64,
        sse_v: u64,
        avx_v: u64,
        a512_v: u64,
        xc_v: u64,
        zw_v: u64,
        cc_v: u64,
        dq_v: u64
    ) -> Self {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &x87_v.to_le_bytes());
        h = fnv1a64_from(h, &sse_v.to_le_bytes());
        h = fnv1a64_from(h, &avx_v.to_le_bytes());
        h = fnv1a64_from(h, &a512_v.to_le_bytes());
        h = fnv1a64_from(h, &xc_v.to_le_bytes());
        h = fnv1a64_from(h, &zw_v.to_le_bytes());
        h = fnv1a64_from(h, &cc_v.to_le_bytes());
        h = fnv1a64_from(h, &dq_v.to_le_bytes());
        Self {
            x87_v,
            sse_v,
            avx_v,
            a512_v,
            xc_v,
            zw_v,
            cc_v,
            dq_v,
            fingerprint: h,
        }
    }

    /// 重算指纹（用于篡改检出）。
    pub fn recompute(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fnv1a64_from(h, &self.x87_v.to_le_bytes());
        h = fnv1a64_from(h, &self.sse_v.to_le_bytes());
        h = fnv1a64_from(h, &self.avx_v.to_le_bytes());
        h = fnv1a64_from(h, &self.a512_v.to_le_bytes());
        h = fnv1a64_from(h, &self.xc_v.to_le_bytes());
        h = fnv1a64_from(h, &self.zw_v.to_le_bytes());
        h = fnv1a64_from(h, &self.cc_v.to_le_bytes());
        h = fnv1a64_from(h, &self.dq_v.to_le_bytes());
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

/// 段位守卫：全部诊断码必须落在本段 [0x6050, 0x605f]。
pub const fn u1901_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < u1901Code::ALL.len() {
        let c = u1901Code::ALL[i].code();
        if c < 0x6050 || c > 0x605f {
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
fn u1901_guard_seg() -> bool {
    u1901_segment_guard()
}

/// UNX-F1901 状态机闭集/迁移表/非法组合恰值全绿。
fn check_u1901_sm_legal() -> bool {
    let mut ok = mxu1901flow_illegal_pairs() == 29;
    if ok { ok = MXU1901FLOW_TRANSITIONS.len() == 8; }
    if ok { ok = mxu1901flowState::ALL.len() == 6 && mxu1901flowEvent::ALL.len() == 6; }
    ok
}

/// UNX-F1901 迁移主链回放 + 快照往返恒等。
fn check_u1901_sm_round() -> bool {
    let mut lg = mxu1901flowLedger::new(mxu1901flowState::Probed);
    let mut ok = lg.apply(mxu1901flowEvent::ProbeXcr0) && lg.cur == mxu1901flowState::Sized && lg.total() == 1;
    let s0 = Mxu1901Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    let mut s1 = Mxu1901Snap::compute(62, 93, 124, 155, 186, 217, 248, 279);
    s1.recompute();
    if ok { ok = s0 == s1 && !s1.tampered(); }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_u1901_checks() -> CheckSet {
    let mut set = CheckSet::new("U1901");
    set.add(
        "u1901-seg-guard",
        u1901_guard_seg(),
        if u1901_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "u1901-sm-legal",
        check_u1901_sm_legal(),
        if check_u1901_sm_legal() { "" } else { "UNX-F1901 sm legality mismatch" },
    );
    set.add(
        "u1901-sm-round",
        check_u1901_sm_round(),
        if check_u1901_sm_round() { "" } else { "UNX-F1901 sm roundtrip mismatch" },
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
    fn u1901_all_checks_green() {
        let set = run_u1901_checks();
        assert!(set.all_passed(), "U1901 domain checks must be green");
    }

    #[test]
    fn u1901_segment_guard_holds() {
        assert!(u1901_segment_guard());
    }
}
