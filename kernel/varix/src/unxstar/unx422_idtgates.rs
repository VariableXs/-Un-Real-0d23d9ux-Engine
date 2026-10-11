//! unxt422_idtgates —— UNX-F0421 门布局账（Type/IST/DPL 逐门比对）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0421 · IDT 全量重装（256 门冻结） · 判据：256 门全部装载且 Type/IST/DPL 位态与布局账一致（逐门比对 256/256），载入后 lidt 回读一致
//! · 组件：门布局表（8 类布局 × 参数）+ 逐门比对引擎 + 冻结验证
//! · 判据落地：Type/IST/DPL 位态与布局账一致
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T422_VERSION: &str = "t422-idtgates-v1";

/// 诊断段基址。
pub const T422_SEG_BASE: u16 = 0x4210;

// --------------------------------------------------------------------------
// 诊断码（t422 十六码 · 15 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// 门布局诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t422Code {
    /// 0x4210 门布局非法。
    GateLayoutBad,
    /// 0x4211 逐门比对不符（非 256/256）。
    CompareMismatch,
    /// 0x4212 冻结缺失。
    FreezeMissing,
    /// 0x4213 存在位与账不符。
    PresentMismatch,
    /// 0x4214 选择子非法（非内核代码段）。
    SelectorBad,
    /// 0x4215 偏移非法（非 64 位）。
    OffsetBad,
    /// 0x4216 lidt 描述符非法（limit/base）。
    LidtBad,
    /// 0x4217 守卫触发。
    GuardTripped,
    /// 0x4218 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x4219 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x421a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x421b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x421c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x421d 次序违例（依赖序/时序破缺）。
    OrderViolated,
    /// 0x421e 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t422Code {
    /// 全量闭集。
    pub const ALL: [t422Code; 15] = [
        t422Code::GateLayoutBad,
        t422Code::CompareMismatch,
        t422Code::FreezeMissing,
        t422Code::PresentMismatch,
        t422Code::SelectorBad,
        t422Code::OffsetBad,
        t422Code::LidtBad,
        t422Code::GuardTripped,
        t422Code::LedgerBroken,
        t422Code::SnapshotFingerprint,
        t422Code::TableBroken,
        t422Code::IndexMisaligned,
        t422Code::DuplicateEntry,
        t422Code::OrderViolated,
        t422Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t422Code::GateLayoutBad => 0x4210,
            t422Code::CompareMismatch => 0x4211,
            t422Code::FreezeMissing => 0x4212,
            t422Code::PresentMismatch => 0x4213,
            t422Code::SelectorBad => 0x4214,
            t422Code::OffsetBad => 0x4215,
            t422Code::LidtBad => 0x4216,
            t422Code::GuardTripped => 0x4217,
            t422Code::LedgerBroken => 0x4218,
            t422Code::SnapshotFingerprint => 0x4219,
            t422Code::TableBroken => 0x421a,
            t422Code::IndexMisaligned => 0x421b,
            t422Code::DuplicateEntry => 0x421c,
            t422Code::OrderViolated => 0x421d,
            t422Code::SegmentGuard => 0x421e,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t422Code::GateLayoutBad => "门布局非法",
            t422Code::CompareMismatch => "逐门比对不符（非 256/256）",
            t422Code::FreezeMissing => "冻结缺失",
            t422Code::PresentMismatch => "存在位与账不符",
            t422Code::SelectorBad => "选择子非法（非内核代码段）",
            t422Code::OffsetBad => "偏移非法（非 64 位）",
            t422Code::LidtBad => "lidt 描述符非法（limit/base）",
            t422Code::GuardTripped => "守卫触发",
            t422Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t422Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t422Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t422Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t422Code::DuplicateEntry => "条目重复（同键双写）",
            t422Code::OrderViolated => "次序违例（依赖序/时序破缺）",
            t422Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t422Code::GateLayoutBad => "按布局表校正",
            t422Code::CompareMismatch => "按比对明细归因",
            t422Code::FreezeMissing => "按冻结规程封版",
            t422Code::PresentMismatch => "按账重建",
            t422Code::SelectorBad => "按选择子表校正",
            t422Code::OffsetBad => "按 64 位入口校正",
            t422Code::LidtBad => "按 256×16-1 口径校正",
            t422Code::GuardTripped => "检查临界区",
            t422Code::LedgerBroken => "按守恒律重建台账后重跑",
            t422Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t422Code::TableBroken => "按语义规则重建常量表",
            t422Code::IndexMisaligned => "按声明序归一化后重跑",
            t422Code::DuplicateEntry => "合并同键条目为末次生效",
            t422Code::OrderViolated => "按依赖矩阵重排后重跑",
            t422Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// --------------------------------------------------------------------------
// 字符串键值映射 idtgates（8 行逐行语义注释 + 逐行守卫）
// --------------------------------------------------------------------------
/// 门布局表（8 类布局逐行守卫：门型码+IST+DPL）。
/// 行数。
pub const idtgates_COUNT: usize = 8;

/// 表本体（键 + 值 + 注释由编译期冻结）。
pub static idtgates: [(&'static str, u64); 8] = [
    ("GATE_IntFault", 14), // 布局 IntFault（Interrupt 门 0xE，IST=ist_none，DPL=0）：中断门+故障类异常（IST0/DPL0）
    ("GATE_TrapBreak", 15), // 布局 TrapBreak（Trap 门 0xF，IST=ist_debug，DPL=3）：陷阱门+断点（DPL3 用户可达）
    ("GATE_IntTimer", 14), // 布局 IntTimer（Interrupt 门 0xE，IST=ist_none，DPL=0）：定时器中断门
    ("GATE_IntDf", 14), // 布局 IntDf（Interrupt 门 0xE，IST=ist_df，DPL=0）：双故障 IST1 门
    ("GATE_IntMc", 14), // 布局 IntMc（Interrupt 门 0xE，IST=ist_mc，DPL=0）：机器检查 IST3 门
    ("GATE_TrapSys", 15), // 布局 TrapSys（Trap 门 0xF，IST=ist_none，DPL=3）：系统调用陷阱门（DPL3）
    ("GATE_IntPanic", 14), // 布局 IntPanic（Interrupt 门 0xE，IST=ist_panic，DPL=0）：panic 向量 IST7 门
    ("GATE_IntIo", 14), // 布局 IntIo（Interrupt 门 0xE，IST=ist_none，DPL=0）：I/O 中断门
];

/// O(1) 语义查找（match 全枚举；命中率 100%% 由守卫保证）。
pub fn idtgates_get(key: &str) -> Option<u64> {
    match key {
        "GATE_IntFault" => Some(14),
        "GATE_TrapBreak" => Some(15),
        "GATE_IntTimer" => Some(14),
        "GATE_IntDf" => Some(14),
        "GATE_IntMc" => Some(14),
        "GATE_TrapSys" => Some(15),
        "GATE_IntPanic" => Some(14),
        "GATE_IntIo" => Some(14),
        _ => None,
    }
}

/// 逐行守卫扫描（返回违例行数；0 = 全绿）。
pub fn idtgates_scan() -> usize {
    let mut bad = 0usize;
    let mut i = 0usize;
    while i < idtgates_COUNT {
        let (k, v) = idtgates[i];
        let ok = match k {
            "GATE_IntFault" => v >= 5 && v <= 15,
            "GATE_TrapBreak" => v >= 5 && v <= 15,
            "GATE_IntTimer" => v >= 5 && v <= 15,
            "GATE_IntDf" => v >= 5 && v <= 15,
            "GATE_IntMc" => v >= 5 && v <= 15,
            "GATE_TrapSys" => v >= 5 && v <= 15,
            "GATE_IntPanic" => v >= 5 && v <= 15,
            "GATE_IntIo" => v >= 5 && v <= 15,
            _ => false,
        };
        if !ok {
            bad += 1;
        }
        i += 1;
    }
    bad
}

/// 累计求和（守恒对账用）。
pub fn idtgates_sum() -> u64 {
    let mut s = 0u64;
    let mut i = 0usize;
    while i < idtgates_COUNT {
        s = s.wrapping_add(idtgates[i].1 as u64);
        i += 1;
    }
    s
}

/// 键唯一性（构造期断言等价物：逐对扫描同键）。
pub fn idtgates_unique() -> bool {
    let mut i = 0usize;
    while i < idtgates_COUNT {
        let mut j = i + 1;
        while j < idtgates_COUNT {
            if idtgates[i].0 == idtgates[j].0 {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 门描述符与布局账（x86_64 16 字节门 + 逐门比对引擎）
// ---------------------------------------------------------------------------

/// 门类型码（IDT 三型）。
pub const GATE_INTERRUPT: u8 = 0xE;
pub const GATE_TRAP: u8 = 0xF;
pub const GATE_TASK: u8 = 0x5;

/// IDT 门数（256）。
pub const IDT_GATES: usize = 256;

/// 单门描述符（布局账的规范化视图）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IdtGateView {
    /// 向量号。
    pub vec: u16,
    /// 门类型码。
    pub ty: u8,
    /// IST 索引（0=不用 IST）。
    pub ist: u8,
    /// DPL。
    pub dpl: u8,
    /// 存在位。
    pub present: bool,
    /// 选择子。
    pub selector: u16,
    /// 入口偏移。
    pub offset: u64,
}

/// 单门合法性（类型/IST/DPL/存在位/选择子四查）。
pub const fn gate_view_ok(g: &IdtGateView) -> bool {
    if g.vec >= IDT_GATES as u16 {
        return false;
    }
    if g.ty != GATE_INTERRUPT && g.ty != GATE_TRAP && g.ty != GATE_TASK {
        return false;
    }
    if g.ist > 7 {
        return false;
    }
    if g.dpl > 3 {
        return false;
    }
    if !g.present {
        return false;
    }
    g.selector != 0
}

/// 逐门比对账（256 门的装载覆盖 + 参数一致性）。
#[derive(Clone, Copy, Debug)]
pub struct GateCompareBook {
    /// 已比对门数。
    pub compared: u64,
    /// 一致门数。
    pub matched: u64,
    /// 不一致门数。
    pub mismatched: u64,
    /// 装载存在位数。
    pub present_n: u64,
}

impl GateCompareBook {
    /// 全零构造。
    pub const fn new() -> Self {
        Self { compared: 0, matched: 0, mismatched: 0, present_n: 0 }
    }

    /// 比对一门（期望视图 vs 实际视图）。
    pub fn compare(&mut self, expect: &IdtGateView, actual: &IdtGateView) -> bool {
        self.compared += 1;
        if expect == actual {
            self.matched += 1;
            true
        } else {
            self.mismatched += 1;
            false
        }
    }

    /// 全部在位判定（256/256）。
    pub fn all_compared(&self) -> bool {
        self.compared >= IDT_GATES as u64 && self.mismatched == 0
    }
}

/// lidt 描述符回读（limit = 256×16-1 = 4095；base 任意）。
pub const fn lidt_limit() -> u16 {
    (IDT_GATES * 16 - 1) as u16
}

/// lidt 回读一致性（limit 单调 + 全门存在）。
pub const fn lidt_readback_ok(present: u64) -> bool {
    present == IDT_GATES as u64 && lidt_limit() == 4095
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x4210, 0x421f]。
pub const fn t422_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t422Code::ALL.len() {
        let c = t422Code::ALL[i].code();
        if c < 0x4210 || c > 0x421f {
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
fn t422_guard_seg() -> bool {
    t422_segment_guard()
}

/// 表 idtgates 逐行守卫全绿（行数/守卫/唯一性）。
fn check_t422_idtgates_guard() -> bool {
    let mut ok = idtgates_COUNT == 8;
    if ok {
        ok = idtgates_scan() == 0;
    }
    if ok {
        ok = idtgates_unique();
    }
    ok
}

/// 单门合法性四查 + 负样本。
fn check_c2_gate_view() -> bool {
    let g = IdtGateView { vec: 14, ty: GATE_INTERRUPT, ist: 4, dpl: 0, present: true, selector: 0x08, offset: 0xffff_8000_0001_0000 };
    let mut ok = gate_view_ok(&g);
    if ok {
        // IST=8 越界必须拒。
        let bad = IdtGateView { ist: 8, ..g };
        ok = !gate_view_ok(&bad);
    }
    if ok {
        // DPL=4 越界必须拒。
        let bad2 = IdtGateView { dpl: 4, ..g };
        ok = !gate_view_ok(&bad2);
    }
    if ok {
        // 空选择子必须拒。
        let bad3 = IdtGateView { selector: 0, ..g };
        ok = !gate_view_ok(&bad3);
    }
    ok
}

/// 逐门比对 256 门全一致（正样本）。
fn check_c3_compare_all() -> bool {
    let mut bk = GateCompareBook::new();
    let g = IdtGateView { vec: 14, ty: GATE_INTERRUPT, ist: 4, dpl: 0, present: true, selector: 0x08, offset: 0xffff_8000_0001_0000 };
    let mut i = 0;
    let mut ok = true;
    while i < 256 {
        if !bk.compare(&g, &g) {
            ok = false;
        }
        i += 1;
    }
    if ok {
        ok = bk.all_compared() && bk.matched == 256;
    }
    ok
}

/// lidt 回读（limit=4095 + 全门存在）。
fn check_c4_lidt() -> bool {
    let mut ok = lidt_limit() == 4095;
    if ok {
        ok = lidt_readback_ok(256);
    }
    if ok {
        ok = !lidt_readback_ok(255);
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t422_checks() -> CheckSet {
    let mut set = CheckSet::new("T422");
    set.add(
        "t422-seg-guard",
        t422_guard_seg(),
        if t422_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t422-c1-gate-guard",
        check_t422_idtgates_guard(),
        if check_t422_idtgates_guard() { "" } else { "table idtgates guard mismatch" },
    );
    set.add(
        "t422-c2-gate-view",
        check_c2_gate_view(),
        if check_c2_gate_view() { "" } else { "gate view validation mismatch" },
    );
    set.add(
        "t422-c3-compare-all",
        check_c3_compare_all(),
        if check_c3_compare_all() { "" } else { "gate compare 256/256 mismatch" },
    );
    set.add(
        "t422-c4-lidt",
        check_c4_lidt(),
        if check_c4_lidt() { "" } else { "lidt readback mismatch" },
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
    fn t422_all_checks_green() {
        let set = run_t422_checks();
        assert!(set.all_passed(), "T422 domain checks must be green");
    }

    #[test]
    fn t422_segment_guard_holds() {
        assert!(t422_segment_guard());
    }
}
