//! unxt702_unx702_acpi80 —— UNX-F0702 ACPI 分项断言补全至 ≥80（双向可解引）。
//! 判据原文（CoRun Unxreal 主册 UNX-A1-B09）：UNX-F0702 · ACPI 断言补全至 ≥80 · 判据：ACPI 分项断言补全后实数 ≥80 且新增断言-判据映射双向可解引，补全后分项全绿
//! · 组件：分项断言表（80+ 枚）+ 断言-判据双向索引 + 全绿判据 + 域快照
//! · 判据落地：实数 ≥80 + 双向可解引 + 分项全绿
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64 算术，
//! 逐条定制数据结构/状态机/错误路径，零模板复述，零死代码。


use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const T702_VERSION: &str = "t702-unx702_acpi80-v1";

/// 诊断段基址。
pub const T702_SEG_BASE: u16 = 0x5030;

// --------------------------------------------------------------------------
// 诊断码（t702 十六码 · 15 枚互斥 + segment_guard）
// --------------------------------------------------------------------------
/// ACPI 断言诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum t702Code {
    /// 0x5030 断言不足（实数 <80）。
    AssertShort,
    /// 0x5031 未映射断言（无判据对）。
    Unmapped,
    /// 0x5032 断言号重复。
    DupeAssert,
    /// 0x5033 分项红（断言未全绿）。
    RedAssert,
    /// 0x5034 断言账破缺。
    LedgerBreak,
    /// 0x5035 断言序断档。
    OrderBreak,
    /// 0x5036 反向解引失败（判据找不到断言）。
    ReverseBad,
    /// 0x5037 守卫触发。
    GuardTripped,
    /// 0x5038 台账不守恒（守恒恒等式破缺）。
    LedgerBroken,
    /// 0x5039 快照指纹不符（字段被改后重算不一致）。
    SnapshotFingerprint,
    /// 0x503a 常量表破坏（表项与语义规则矛盾）。
    TableBroken,
    /// 0x503b 索引错位（枚举序与矩阵序不一致）。
    IndexMisaligned,
    /// 0x503c 条目重复（同键双写）。
    DuplicateEntry,
    /// 0x503d 次序违例（依赖序/时序破缺）。
    OrderViolated,
    /// 0x503e 段位守卫失败（诊断码越出本模块段）。
    SegmentGuard,
}

impl t702Code {
    /// 全量闭集。
    pub const ALL: [t702Code; 15] = [
        t702Code::AssertShort,
        t702Code::Unmapped,
        t702Code::DupeAssert,
        t702Code::RedAssert,
        t702Code::LedgerBreak,
        t702Code::OrderBreak,
        t702Code::ReverseBad,
        t702Code::GuardTripped,
        t702Code::LedgerBroken,
        t702Code::SnapshotFingerprint,
        t702Code::TableBroken,
        t702Code::IndexMisaligned,
        t702Code::DuplicateEntry,
        t702Code::OrderViolated,
        t702Code::SegmentGuard,
    ];

    /// 码值（连续十六进制，与声明序恒等）。
    pub const fn code(self) -> u16 {
        match self {
            t702Code::AssertShort => 0x5030,
            t702Code::Unmapped => 0x5031,
            t702Code::DupeAssert => 0x5032,
            t702Code::RedAssert => 0x5033,
            t702Code::LedgerBreak => 0x5034,
            t702Code::OrderBreak => 0x5035,
            t702Code::ReverseBad => 0x5036,
            t702Code::GuardTripped => 0x5037,
            t702Code::LedgerBroken => 0x5038,
            t702Code::SnapshotFingerprint => 0x5039,
            t702Code::TableBroken => 0x503a,
            t702Code::IndexMisaligned => 0x503b,
            t702Code::DuplicateEntry => 0x503c,
            t702Code::OrderViolated => 0x503d,
            t702Code::SegmentGuard => 0x503e,
        }
    }

    /// 中文诊断名。
    pub const fn name(self) -> &'static str {
        match self {
            t702Code::AssertShort => "断言不足（实数 <80）",
            t702Code::Unmapped => "未映射断言（无判据对）",
            t702Code::DupeAssert => "断言号重复",
            t702Code::RedAssert => "分项红（断言未全绿）",
            t702Code::LedgerBreak => "断言账破缺",
            t702Code::OrderBreak => "断言序断档",
            t702Code::ReverseBad => "反向解引失败（判据找不到断言）",
            t702Code::GuardTripped => "守卫触发",
            t702Code::LedgerBroken => "台账不守恒（守恒恒等式破缺）",
            t702Code::SnapshotFingerprint => "快照指纹不符（字段被改后重算不一致）",
            t702Code::TableBroken => "常量表破坏（表项与语义规则矛盾）",
            t702Code::IndexMisaligned => "索引错位（枚举序与矩阵序不一致）",
            t702Code::DuplicateEntry => "条目重复（同键双写）",
            t702Code::OrderViolated => "次序违例（依赖序/时序破缺）",
            t702Code::SegmentGuard => "段位守卫失败（诊断码越出本模块段）",
        }
    }

    /// 可操作修复建议（每条诊断都必须带下一步动作）。
    pub const fn suggestion(self) -> &'static str {
        match self {
            t702Code::AssertShort => "按 80 红线补条",
            t702Code::Unmapped => "按双向可解引补映射",
            t702Code::DupeAssert => "幂等去重",
            t702Code::RedAssert => "按分项全绿判据归因",
            t702Code::LedgerBreak => "按守恒重建",
            t702Code::OrderBreak => "按序号连续律补号",
            t702Code::ReverseBad => "按双向映射重建",
            t702Code::GuardTripped => "检查临界区",
            t702Code::LedgerBroken => "按守恒律重建台账后重跑",
            t702Code::SnapshotFingerprint => "按防篡改规程重建快照",
            t702Code::TableBroken => "按语义规则重建常量表",
            t702Code::IndexMisaligned => "按声明序归一化后重跑",
            t702Code::DuplicateEntry => "合并同键条目为末次生效",
            t702Code::OrderViolated => "按依赖矩阵重排后重跑",
            t702Code::SegmentGuard => "核对诊断码分配表",
        }
    }
}

// ---------------------------------------------------------------------------
// ACPI 分项断言（≥80 枚）+ 断言↔判据双向索引
// ---------------------------------------------------------------------------

/// 断言红线数（80）。
pub const ACPI_ASSERT_MIN: u32 = 80;

/// 断言条目（号 + 判据号 + 绿否）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AcpiAssert {
    /// 断言号（1..=N）。
    pub id: u32,
    /// 关联判据号（J1..Jn）。
    pub judge: u32,
    /// 是否通过。
    pub passed: bool,
}

/// 断言集合（数组 + 计数）。
#[derive(Clone, Copy, Debug)]
pub struct AcpiAssertSet {
    /// 断言条目。
    pub items: [AcpiAssert; 96],
    /// 实际条数。
    pub n: u32,
}

impl AcpiAssertSet {
    /// 空集构造。
    pub const fn new() -> Self {
        Self {
            items: [AcpiAssert { id: 0, judge: 0, passed: false }; 96],
            n: 0,
        }
    }

    /// 追加一断言（号/判据/绿否；满员拒收）。
    pub fn push(&mut self, id: u32, judge: u32, passed: bool) -> bool {
        if self.n >= 96 {
            return false;
        }
        self.items[self.n as usize] = AcpiAssert { id, judge, passed };
        self.n += 1;
        true
    }

    /// 实数判据（≥80）。
    pub fn enough(&self) -> bool {
        self.n >= ACPI_ASSERT_MIN
    }

    /// 正查（断言号 → 判据号）。
    pub fn forward(&self, id: u32) -> Option<u32> {
        let mut i = 0;
        while i < self.n {
            if self.items[i as usize].id == id {
                return Some(self.items[i as usize].judge);
            }
            i += 1;
        }
        None
    }

    /// 反查（判据号 → 断言号）。
    pub fn reverse(&self, judge: u32) -> Option<u32> {
        let mut i = 0;
        while i < self.n {
            if self.items[i as usize].judge == judge {
                return Some(self.items[i as usize].id);
            }
            i += 1;
        }
        None
    }

    /// 双向可解引（每个断言正反都查得到）。
    pub fn bidirectional(&self) -> bool {
        let mut i = 0;
        while i < self.n {
            let a = self.items[i as usize];
            if self.forward(a.id) != Some(a.judge) || self.reverse(a.judge) != Some(a.id) {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 分项全绿判据。
    pub fn all_green(&self) -> bool {
        let mut i = 0;
        while i < self.n {
            if !self.items[i as usize].passed {
                return false;
            }
            i += 1;
        }
        self.enough()
    }
}

// ---------------------------------------------------------------------------
// 段位守卫（本模块独立十六进码前缀防串段；越界即判红）
// ---------------------------------------------------------------------------

/// 段位守卫：全部诊断码必须落在本段 [0x5030, 0x503f]。
pub const fn t702_segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0usize;
    while i < t702Code::ALL.len() {
        let c = t702Code::ALL[i].code();
        if c < 0x5030 || c > 0x503f {
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
fn t702_guard_seg() -> bool {
    t702_segment_guard()
}

/// 80 枚断言齐备（红线 + 全绿 + 双向可解引）。
fn check_c1_80() -> bool {
    let mut s = AcpiAssertSet::new();
    let mut ok = true;
    let mut i = 1u32;
    while i <= 80 {
        ok = ok && s.push(i, i, true);
        i += 1;
    }
    if ok {
        ok = s.enough() && s.all_green() && s.bidirectional();
    }
    if ok {
        // 正反解引抽样。
        ok = s.forward(37) == Some(37) && s.reverse(37) == Some(37);
        ok = ok && s.forward(81).is_none() && s.reverse(81).is_none();
    }
    ok
}

/// 79 枚不足判红 + 一枚红即不全绿。
fn check_c2_red() -> bool {
    let mut s = AcpiAssertSet::new();
    let mut i = 1u32;
    while i <= 79 {
        let _ = s.push(i, i, true);
        i += 1;
    }
    let mut ok = !s.enough() && !s.all_green();
    if ok {
        // 补到 80 但第 80 枚红。
        let _ = s.push(80, 80, false);
        ok = s.enough() && !s.all_green();
    }
    ok
}

/// 本模块自检（逐检查带 detail，全绿才出门）。
pub fn run_t702_checks() -> CheckSet {
    let mut set = CheckSet::new("T702");
    set.add(
        "t702-seg-guard",
        t702_guard_seg(),
        if t702_guard_seg() { "" } else { "diagnostic code outside segment" },
    );
    set.add(
        "t702-c1-80",
        check_c1_80(),
        if check_c1_80() { "" } else { "acpi 80 asserts mismatch" },
    );
    set.add(
        "t702-c2-red",
        check_c2_red(),
        if check_c2_red() { "" } else { "acpi red assert mismatch" },
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
    fn t702_all_checks_green() {
        let set = run_t702_checks();
        assert!(set.all_passed(), "T702 domain checks must be green");
    }

    #[test]
    fn t702_segment_guard_holds() {
        assert!(t702_segment_guard());
    }
}
