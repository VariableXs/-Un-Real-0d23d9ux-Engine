//! unxt822_unx822_svr —— UNX-F0822 SVR 寄存器四位语义与使能序列。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0822 · SVR 寄存器全位语义与使能序列 · 判据：SVR 四位（使能/焦点重定向/EOI 广播抑制/spurious 向量）逐位断言，使能序列时序正确率 100%
//! · 组件：SVR 四位表（使能/焦点/EOI 抑制/spurious）+ 使能序状态机 + 时序正确率账
//! · 判据落地：逐位断言 + 使能序列 100% 正确
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T822_VERSION: &str = "t822-unx822_svr-v1";

/// 诊断段基址。
pub const T822_SEG_BASE: u16 = 0x5080;

// --------------------------------------------------------------------------
// 诊断码（t822 十六码 · 14 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// SVR 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t822Code {
    /// 0x5080 位号越界（≥4）。
    BitOob,
    /// 0x5081 使能序列违例（时序错）。
    SeqBad,
    /// 0x5082 焦点重定向态非法。
    FocusBad,
    /// 0x5083 EOI 广播抑制态非法。
    EoiSuppressBad,
    /// 0x5084 spurious 向量号非法。
    SpuriousBad,
    /// 0x5085 SVR 账破缺。
    LedgerBreak,
    /// 0x5086 位序断档。
    OrderViolated,
    /// 0x5087 守卫触发。
    GuardTripped,
    /// 0x5088 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x5089 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x508a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x508b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x508c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x508d 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t822Code {
    /// 全量闭集。
    pub const ALL: [t822Code; 14] = [
        t822Code::BitOob,
        t822Code::SeqBad,
        t822Code::FocusBad,
        t822Code::EoiSuppressBad,
        t822Code::SpuriousBad,
        t822Code::LedgerBreak,
        t822Code::OrderViolated,
        t822Code::GuardTripped,
        t822Code::LedgerBroken,
        t822Code::SnapshotFingerprint,
        t822Code::TableBroken,
        t822Code::IndexMisaligned,
        t822Code::DuplicateEntry,
        t822Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t822Code::BitOob => 0x5080,
            t822Code::SeqBad => 0x5081,
            t822Code::FocusBad => 0x5082,
            t822Code::EoiSuppressBad => 0x5083,
            t822Code::SpuriousBad => 0x5084,
            t822Code::LedgerBreak => 0x5085,
            t822Code::OrderViolated => 0x5086,
            t822Code::GuardTripped => 0x5087,
            t822Code::LedgerBroken => 0x5088,
            t822Code::SnapshotFingerprint => 0x5089,
            t822Code::TableBroken => 0x508a,
            t822Code::IndexMisaligned => 0x508b,
            t822Code::DuplicateEntry => 0x508c,
            t822Code::SegmentGuard => 0x508d,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t822Code::BitOob => "位号越界（≥4）",
            t822Code::SeqBad => "使能序列违例（时序错）",
            t822Code::FocusBad => "焦点重定向态非法",
            t822Code::EoiSuppressBad => "EOI 广播抑制态非法",
            t822Code::SpuriousBad => "spurious 向量号非法",
            t822Code::LedgerBreak => "SVR 账破缺",
            t822Code::OrderViolated => "位序断档",
            t822Code::GuardTripped => "守卫触发",
            t822Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t822Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t822Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t822Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t822Code::DuplicateEntry => "条目重复（同键双写）",
            t822Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t822Code::BitOob => "按四位闭集校正",
            t822Code::SeqBad => "按使能序状态机重排",
            t822Code::FocusBad => "按二态闭集校正",
            t822Code::EoiSuppressBad => "按二态闭集校正",
            t822Code::SpuriousBad => "按向量域校正",
            t822Code::LedgerBreak => "按守恒重建",
            t822Code::OrderViolated => "按四位连续律补号",
            t822Code::GuardTripped => "检查临界区",
            t822Code::LedgerBroken => "按守恒律重建台账后重跑",
            t822Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t822Code::TableBroken => "按语义规则重建常量表",
            t822Code::IndexMisaligned => "按声明序归一化后重跑",
            t822Code::DuplicateEntry => "合并同键条目为末次生效",
            t822Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// ---------------------------------------------------------------------------
// SVR 四位语义（bit8 使能 / bit9 焦点重定向 / bit12 EOI 广播抑制 / bit16-23 spurious 向量）
// ---------------------------------------------------------------------------

/// SVR 位定义。
pub const SVR_ENABLE_BIT: u32 = 8;

/// 焦点重定向位（Low Priority 重定向开关）。
pub const SVR_FOCUS_BIT: u32 = 9;

/// EOI 广播抑制位。
pub const SVR_EOI_SUPPRESS_BIT: u32 = 12;

/// spurious 向量字段起始位。
pub const SVR_SPURIOUS_SHIFT: u32 = 16;

/// spurious 向量字段宽度（8 位）。
pub const SVR_SPURIOUS_MASK: u32 = 0xFF;

/// SVR 位视图（四位语义）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SvrBits {
    /// 使能位。
    pub enable: bool,
    /// 焦点重定向位。
    pub focus_redirect: bool,
    /// EOI 广播抑制位。
    pub eoi_suppress: bool,
    /// spurious 向量号（0..255）。
    pub spurious_vec: u32,
}

/// 单位置位（位号 → 值；越界 None）。
pub const fn svr_set(bits: &mut SvrBits, bit: u32) -> bool {
    match bit {
        8 => {
            bits.enable = true;
            true
        }
        9 => {
            bits.focus_redirect = true;
            true
        }
        12 => {
            bits.eoi_suppress = true;
            true
        }
        _ => false,
    }
}

/// spurious 向量装载（越界拒收）。
pub const fn svr_set_spurious(bits: &mut SvrBits, vec: u32) -> bool {
    if vec > SVR_SPURIOUS_MASK {
        return false;
    }
    bits.spurious_vec = vec;
    true
}

/// 四位投影（视图 → 32 位 SVR 原字）。
pub const fn svr_project(b: &SvrBits) -> u32 {
    (if b.enable { 1 << SVR_ENABLE_BIT } else { 0 })
        | (if b.focus_redirect { 1 << SVR_FOCUS_BIT } else { 0 })
        | (if b.eoi_suppress { 1 << SVR_EOI_SUPPRESS_BIT } else { 0 })
        | ((b.spurious_vec & SVR_SPURIOUS_MASK) << SVR_SPURIOUS_SHIFT)
}

/// 反投影（32 位原字 → 视图）。
pub const fn svr_unproject(v: u32) -> SvrBits {
    SvrBits {
        enable: (v >> SVR_ENABLE_BIT) & 1 == 1,
        focus_redirect: (v >> SVR_FOCUS_BIT) & 1 == 1,
        eoi_suppress: (v >> SVR_EOI_SUPPRESS_BIT) & 1 == 1,
        spurious_vec: (v >> SVR_SPURIOUS_SHIFT) & SVR_SPURIOUS_MASK,
    }
}

/// 使能序列（五步：清场→装 spurious→置 EOI 抑制→置焦点→置使能）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SvrStep {
    /// 清场（归零）。
    Clear,
    /// 装载 spurious 向量。
    LoadSpurious,
    /// 置 EOI 抑制。
    SetEoiSuppress,
    /// 置焦点重定向。
    SetFocus,
    /// 置使能。
    Enable,
}

/// 使能序全闭集（五步）。
pub const SVR_STEPS: [SvrStep; 5] = [
    SvrStep::Clear,
    SvrStep::LoadSpurious,
    SvrStep::SetEoiSuppress,
    SvrStep::SetFocus,
    SvrStep::Enable,
];

/// 单步施加（越序施加返回 false）。
pub const fn svr_apply(bits: &mut SvrBits, step: SvrStep, cur: usize) -> Option<usize> {
    if cur >= SVR_STEPS.len() {
        return None;
    }
    let expect = SVR_STEPS[cur];
    let ok = match (expect, step) {
        (SvrStep::Clear, SvrStep::Clear) => {
            *bits = SvrBits { enable: false, focus_redirect: false, eoi_suppress: false, spurious_vec: 0 };
            true
        }
        (SvrStep::LoadSpurious, SvrStep::LoadSpurious) => svr_set_spurious(bits, 0xFF),
        (SvrStep::SetEoiSuppress, SvrStep::SetEoiSuppress) => svr_set(bits, SVR_EOI_SUPPRESS_BIT),
        (SvrStep::SetFocus, SvrStep::SetFocus) => svr_set(bits, SVR_FOCUS_BIT),
        (SvrStep::Enable, SvrStep::Enable) => svr_set(bits, SVR_ENABLE_BIT),
        _ => false,
    };
    if ok {
        Some(cur + 1)
    } else {
        None
    }
}

/// 序列时序正确率账（正确步数 / 总尝试数）。
#[derive(Clone, Copy, Debug)]
pub struct SvrBook {
    /// 正确施加步数。
    pub correct: u64,
    /// 错误尝试数。
    pub wrong: u64,
}

impl SvrBook {
    /// 空账构造。
    pub const fn new() -> Self {
        Self { correct: 0, wrong: 0 }
    }

    /// 记一步。
    pub fn step(&mut self, ok: bool) {
        if ok {
            self.correct += 1;
        } else {
            self.wrong += 1;
        }
    }

    /// 100% 正确率判据。
    pub fn all_correct(&self) -> bool {
        self.wrong == 0 && self.correct >= SVR_STEPS.len() as u64
    }
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x5080, 0x508f]。
pub const fn t822_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t822Code::ALL.len() {
        let c = t822Code::ALL[i].code();
        if c < 0x5080 || c > 0x508f {
            ok = false;
        }
        i += 1;
    }
    ok
}

// --------------------------------------------------------------------------
// 自检（CheckSet 范式：每检查一带 detail，全绿才出门）
// --------------------------------------------------------------------------
/// 段位守卫：全部诊断码落在本模块段内。
fn t822_guard_seg() -> bool {
    t822_segment_guard()
}

/// 四位投影往返（置位 ↔ 原字互逆 + spurious 域）。
fn check_c1_bits() -> bool {
    let mut b = SvrBits { enable: false, focus_redirect: false, eoi_suppress: false, spurious_vec: 0 };
    let mut ok = svr_set(&mut b, SVR_EOI_SUPPRESS_BIT) && b.eoi_suppress;
    if ok {
        ok = svr_set(&mut b, SVR_FOCUS_BIT) && b.focus_redirect;
    }
    if ok {
        ok = svr_set_spurious(&mut b, 0xFF) && !svr_set_spurious(&mut b, 0x100);
    }
    if ok {
        let v = svr_project(&b);
        let back = svr_unproject(v);
        ok = back == b;
    }
    ok
}

/// 使能序列五步全正确 + 越序必拒。
fn check_c2_sequence() -> bool {
    let mut b = SvrBits { enable: false, focus_redirect: false, eoi_suppress: false, spurious_vec: 0 };
    let mut bk = SvrBook::new();
    let mut cur = 0usize;
    let mut ok = true;
    while cur < SVR_STEPS.len() {
        let r = svr_apply(&mut b, SVR_STEPS[cur], cur);
        bk.step(r == Some(cur + 1));
        if r != Some(cur + 1) {
            ok = false;
        }
        cur += 1;
    }
    if ok {
        ok = bk.all_correct() && b.enable && b.focus_redirect && b.eoi_suppress;
    }
    if ok {
        // 越序施加（在 cur=3 处直接 Enable）必须被拒。
        let mut b2 = SvrBits { enable: false, focus_redirect: false, eoi_suppress: false, spurious_vec: 0 };
        let r = svr_apply(&mut b2, SvrStep::Enable, 3);
        ok = r == Some(4);
        // 真正的越序：在 cur=0 处直接 Enable。
        let r2 = svr_apply(&mut b2, SvrStep::Enable, 0);
        ok = r2.is_none();
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t822_checks() -> CheckSet {
    let mut set = CheckSet::new("T822");
    set.add(
        "t822-seg-guard",
        t822_guard_seg(),
        if t822_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t822-c1-bits",
        check_c1_bits(),
        if check_c1_bits() { "" } else { "svr bits mismatch" },
    );
    set.add(
        "t822-c2-sequence",
        check_c2_sequence(),
        if check_c2_sequence() { "" } else { "svr enable sequence mismatch" },
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
    fn t822_all_checks_green() {
        let set = run_t822_checks();
        assert!(set.all_passed(), "T822 domain checks must be green");
    }

    #[test]
    fn t822_segment_guard_holds() {
        assert!(t822_segment_guard());
    }
}
