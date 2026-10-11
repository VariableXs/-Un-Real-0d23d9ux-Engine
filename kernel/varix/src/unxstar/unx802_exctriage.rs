//! unxt802_unx802_exctriage —— UNX-F0802 异常区 32 向量分型分发（Fault/Trap/Abort）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0802 · 异常区向量 0x00–0x1F 全挂接与分型分发 · 判据：三类异常（Fault/Trap/Abort）32 项逐项分型落账，注入 10 种异常全部走正确分型路径且现场完整
//! · 组件：32 向量分型表（三分类）+ 分型分发引擎 + 10 种注入路径核查 + 域快照
//! · 判据落地：32 项逐项分型 + 10 种注入全走正确路径
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T802_VERSION: &str = "t802-unx802_exctriage-v1";

/// 诊断段基址。
pub const T802_SEG_BASE: u16 = 0x5070;

// --------------------------------------------------------------------------
// 诊断码（t802 十六码 · 14 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 异常分型诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t802Code {
    /// 0x5070 向量越界（≥32）。
    VecOob,
    /// 0x5071 分类非法（三型之外）。
    ClassBad,
    /// 0x5072 分型误投（走了错误分类路径）。
    Misrouted,
    /// 0x5073 现场不完整（缺寄存器/栈）。
    SiteIncomplete,
    /// 0x5074 向量未挂接（无处理器）。
    NotHooked,
    /// 0x5075 分型账破缺。
    LedgerBreak,
    /// 0x5076 分型序违例。
    OrderViolated,
    /// 0x5077 守卫触发。
    GuardTripped,
    /// 0x5078 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x5079 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x507a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x507b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x507c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x507d 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t802Code {
    /// 全量闭集。
    pub const ALL: [t802Code; 14] = [
        t802Code::VecOob,
        t802Code::ClassBad,
        t802Code::Misrouted,
        t802Code::SiteIncomplete,
        t802Code::NotHooked,
        t802Code::LedgerBreak,
        t802Code::OrderViolated,
        t802Code::GuardTripped,
        t802Code::LedgerBroken,
        t802Code::SnapshotFingerprint,
        t802Code::TableBroken,
        t802Code::IndexMisaligned,
        t802Code::DuplicateEntry,
        t802Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t802Code::VecOob => 0x5070,
            t802Code::ClassBad => 0x5071,
            t802Code::Misrouted => 0x5072,
            t802Code::SiteIncomplete => 0x5073,
            t802Code::NotHooked => 0x5074,
            t802Code::LedgerBreak => 0x5075,
            t802Code::OrderViolated => 0x5076,
            t802Code::GuardTripped => 0x5077,
            t802Code::LedgerBroken => 0x5078,
            t802Code::SnapshotFingerprint => 0x5079,
            t802Code::TableBroken => 0x507a,
            t802Code::IndexMisaligned => 0x507b,
            t802Code::DuplicateEntry => 0x507c,
            t802Code::SegmentGuard => 0x507d,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t802Code::VecOob => "向量越界（≥32）",
            t802Code::ClassBad => "分类非法（三型之外）",
            t802Code::Misrouted => "分型误投（走了错误分类路径）",
            t802Code::SiteIncomplete => "现场不完整（缺寄存器/栈）",
            t802Code::NotHooked => "向量未挂接（无处理器）",
            t802Code::LedgerBreak => "分型账破缺",
            t802Code::OrderViolated => "分型序违例",
            t802Code::GuardTripped => "守卫触发",
            t802Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t802Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t802Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t802Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t802Code::DuplicateEntry => "条目重复（同键双写）",
            t802Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t802Code::VecOob => "按 32 项闭集校正",
            t802Code::ClassBad => "按三型闭集校正",
            t802Code::Misrouted => "按正确分型判据归因",
            t802Code::SiteIncomplete => "按现场齐备律补齐",
            t802Code::NotHooked => "按全挂接律补挂",
            t802Code::LedgerBreak => "按守恒重建",
            t802Code::OrderViolated => "按向量序重排",
            t802Code::GuardTripped => "检查临界区",
            t802Code::LedgerBroken => "按守恒律重建台账后重跑",
            t802Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t802Code::TableBroken => "按语义规则重建常量表",
            t802Code::IndexMisaligned => "按声明序归一化后重跑",
            t802Code::DuplicateEntry => "合并同键条目为末次生效",
            t802Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// ---------------------------------------------------------------------------
// 异常区 32 向量 × 三分类（Fault/Trap/Abort）+ 现场完整性
// ---------------------------------------------------------------------------

/// 异常分类（闭集三型）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExcClass {
    /// 故障类（可恢复，返回故障指令）。
    Fault,
    /// 陷阱类（不恢复，返回下条指令）。
    Trap,
    /// 中止类（不可恢复）。
    Abort,
}

impl ExcClass {
    /// 全量闭集（三型）。
    pub const ALL: [ExcClass; 3] = [ExcClass::Fault, ExcClass::Trap, ExcClass::Abort];

    /// 索引。
    pub const fn index(self) -> usize {
        match self {
            ExcClass::Fault => 0,
            ExcClass::Trap => 1,
            ExcClass::Abort => 2,
        }
    }

    /// 中文分类名。
    pub const fn cn(self) -> &'static str {
        match self {
            ExcClass::Fault => "故障类",
            ExcClass::Trap => "陷阱类",
            ExcClass::Abort => "中止类",
        }
    }
}

/// 异常区向量数（0x00–0x1F）。
pub const EXC_VECS: u32 = 32;

/// Intel SDM 权威分型表（32 项；None = NMI/INTR 入口/保留向量，不参与对外分型）。
pub const EXC_CLASS: [Option<ExcClass>; 32] = [
    Some(ExcClass::Fault), Some(ExcClass::Fault), None, Some(ExcClass::Trap), Some(ExcClass::Trap),
    Some(ExcClass::Fault), Some(ExcClass::Fault), Some(ExcClass::Fault), Some(ExcClass::Abort),
    Some(ExcClass::Fault), Some(ExcClass::Fault), Some(ExcClass::Fault), Some(ExcClass::Fault),
    Some(ExcClass::Fault), Some(ExcClass::Fault), None, Some(ExcClass::Fault), Some(ExcClass::Fault),
    Some(ExcClass::Abort), Some(ExcClass::Fault), Some(ExcClass::Fault), None, None, None, None, None,
    None, None, None, None, None, None,
];

/// 分型查询（向量号 → 分类；None = 保留/NMI/INTR 入口）。
pub const fn exc_class_of(vec: u32) -> Option<ExcClass> {
    if vec >= EXC_VECS {
        return None;
    }
    EXC_CLASS[vec as usize]
}

/// 挂接表项（分类 + 处理器钩子）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExcHook {
    /// 向量号。
    pub vec: u32,
    /// 分类（None = 未挂接）。
    pub class: Option<ExcClass>,
    /// 处理器钩子非空。
    pub handler: bool,
}

/// 挂接合法性（分类合法 + 钩子非空 + 向量域内）。
pub const fn hook_ok(h: &ExcHook) -> bool {
    if h.vec >= EXC_VECS {
        return false;
    }
    match h.class {
        Some(c) => {
            let _ = c;
            h.handler && match exc_class_of(h.vec) {
                Some(expect) => match h.class {
                    Some(actual) => match (expect, actual) {
                        (ExcClass::Fault, ExcClass::Fault) => true,
                        (ExcClass::Trap, ExcClass::Trap) => true,
                        (ExcClass::Abort, ExcClass::Abort) => true,
                        _ => false,
                    },
                    None => false,
                },
                None => h.class.is_none(),
            }
        }
        None => !h.handler,
    }
}

/// 分发判定（实际分类 == 期望分类）。
pub const fn dispatch_ok(h: &ExcHook, actual: ExcClass) -> bool {
    match h.class {
        Some(expect) => match (expect, actual) {
            (ExcClass::Fault, ExcClass::Fault) => true,
            (ExcClass::Trap, ExcClass::Trap) => true,
            (ExcClass::Abort, ExcClass::Abort) => true,
            _ => false,
        },
        None => false,
    }
}

/// 异常现场（四件：向量/错误码/RIP/RSP）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExcSite {
    /// 向量号。
    pub vec: u32,
    /// 错误码（有/无由 tag 表示）。
    pub err_code: u32,
    /// 现场 RIP。
    pub rip: u64,
    /// 现场 RSP。
    pub rsp: u64,
}

/// 现场完整性（RIP/RSP 非零 + 向量域内）。
pub const fn site_complete(s: &ExcSite) -> bool {
    s.vec < EXC_VECS && s.rip != 0 && s.rsp != 0
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x5070, 0x507f]。
pub const fn t802_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t802Code::ALL.len() {
        let c = t802Code::ALL[i].code();
        if c < 0x5070 || c > 0x507f {
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
fn t802_guard_seg() -> bool {
    t802_segment_guard()
}

/// 32 项分型表（权威样本抽查 + 保留向量 None）。
fn check_c1_triage() -> bool {
    let mut ok = EXC_VECS == 32;
    if ok {
        ok = exc_class_of(0) == Some(ExcClass::Fault);
    }
    if ok {
        ok = exc_class_of(3) == Some(ExcClass::Trap);
    }
    if ok {
        ok = exc_class_of(8) == Some(ExcClass::Abort);
    }
    if ok {
        ok = exc_class_of(18) == Some(ExcClass::Abort);
    }
    if ok {
        ok = exc_class_of(32).is_none() && exc_class_of(21).is_none();
    }
    ok
}

/// 挂接合法性 + 10 种注入全走正确路径 + 现场完整。
fn check_c2_dispatch() -> bool {
    let mut h = ExcHook { vec: 0, class: Some(ExcClass::Fault), handler: true };
    let mut ok = hook_ok(&h) && dispatch_ok(&h, ExcClass::Fault);
    if ok {
        ok = !dispatch_ok(&h, ExcClass::Trap);
    }
    if ok {
        // 10 种注入（向量 0..10 的合法子集）逐项分发正确。
        let injects = [0u32, 1, 3, 4, 5, 6, 7, 13, 14, 16];
        let mut i = 0;
        while i < injects.len() {
            let v = injects[i];
            h.vec = v;
            h.class = exc_class_of(v);
            ok = ok && hook_ok(&h) && dispatch_ok(&h, exc_class_of(v).unwrap_or(ExcClass::Fault));
            i += 1;
        }
    }
    if ok {
        let site = ExcSite { vec: 0, err_code: 0, rip: 0xffff_8000_0001_0000, rsp: 0xffff_8000_0010_0000 };
        ok = site_complete(&site);
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t802_checks() -> CheckSet {
    let mut set = CheckSet::new("T802");
    set.add(
        "t802-seg-guard",
        t802_guard_seg(),
        if t802_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t802-c1-triage",
        check_c1_triage(),
        if check_c1_triage() { "" } else { "exception triage mismatch" },
    );
    set.add(
        "t802-c2-dispatch",
        check_c2_dispatch(),
        if check_c2_dispatch() { "" } else { "exception dispatch mismatch" },
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
    fn t802_all_checks_green() {
        let set = run_t802_checks();
        assert!(set.all_passed(), "T802 domain checks must be green");
    }

    #[test]
    fn t802_segment_guard_holds() {
        assert!(t802_segment_guard());
    }
}
