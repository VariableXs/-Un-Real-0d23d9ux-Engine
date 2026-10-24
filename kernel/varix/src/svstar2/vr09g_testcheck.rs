//! VE-F3767 · R 域测试面终版清点
//!
//! 锚点：R 域测试面终版清点——测试清点（八组测试面清单（复述清单红线：
//! 测试必须有断言（断言硬门）；测试分类（单元/集成/验收三级（复述分级）；
//! 覆盖对拍（八组测试覆盖声明一致（复述对拍）；测试冻结（终版冻结（复述冻结）。
//!
//! 数据结构：测试清单；三级分类；覆盖对拍；冻结清单。
//! 错误路径与降级矩阵：无断言→拒（硬门实测）；分类缺→补（复述）；覆盖分歧→
//! 立案（复述红线）；冻结后改→回滚（复述）。
//! 跨批对接点：八组档案；F3765 对账输入；F3768 性能对端。
//!
//! 诊断码段：独占 0x9Exx。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ─────────────────────────────────────────────────────────────
// 诊断码（0x9Exx）
// ─────────────────────────────────────────────────────────────

/// F3767 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct B9Code(pub u16);

impl B9Code {
    /// 测试无断言——硬门。
    pub const NO_ASSERT: B9Code = B9Code(0x9E01);
    /// 分类缺失——三级。
    pub const CLASS_MISS: B9Code = B9Code(0x9E02);
    /// 覆盖分歧——立案。
    pub const COVER_DIVERGE: B9Code = B9Code(0x9E03);
    /// 冻结后改——回滚。
    pub const FREEZE_VIOLATE: B9Code = B9Code(0x9E04);
    /// 清单不全——补。
    pub const LIST_INCOMPLETE: B9Code = B9Code(0x9E05);
    /// 覆盖不足——红线。
    pub const COVER_LOW: B9Code = B9Code(0x9E06);

    /// 诊断说明。
    pub fn explain(self) -> &'static str {
        match self.0 {
            0x9E01 => "测试无断言，硬门拒收。",
            0x9E02 => "测试分级缺失（单元/集成/验收）。",
            0x9E03 => "覆盖声明分歧，需立案。",
            0x9E04 => "冻结后修改，需回滚。",
            0x9E05 => "测试清单不全。",
            0x9E06 => "覆盖不足，红线不通过。",
            _ => "未知诊断码。",
        }
    }

    /// 数值。
    pub fn raw(self) -> u16 {
        self.0
    }
}

// ─────────────────────────────────────────────────────────────
// 测试三级分类
// ─────────────────────────────────────────────────────────────

/// 测试级别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TestLevel {
    /// 单元。
    Unit,
    /// 集成。
    Integration,
    /// 验收。
    Acceptance,
}

impl TestLevel {
    /// 稳定序号。
    pub fn index(self) -> u8 {
        match self {
            TestLevel::Unit => 0,
            TestLevel::Integration => 1,
            TestLevel::Acceptance => 2,
        }
    }

    /// 全集。
    pub fn all() -> [TestLevel; 3] {
        [
            TestLevel::Unit,
            TestLevel::Integration,
            TestLevel::Acceptance,
        ]
    }
}

/// 测试条目。
#[derive(Clone, Debug)]
pub struct TestEntry {
    /// id。
    pub id: u32,
    /// 级别。
    pub level: TestLevel,
    /// 是否有断言。
    pub has_assert: bool,
    /// 覆盖声明（万分比）。
    pub cover_bp: u16,
    /// 是否冻结后被改。
    pub changed_after_freeze: bool,
    /// 是否已冻结。
    pub frozen: bool,
}

impl TestEntry {
    /// 构造。
    pub fn new(id: u32, level: TestLevel) -> TestEntry {
        TestEntry {
            id,
            level,
            has_assert: false,
            cover_bp: 0,
            changed_after_freeze: false,
            frozen: false,
        }
    }

    /// 断言合规。
    pub fn assert_ok(&self) -> bool {
        self.has_assert
    }

    /// 冻结合规。
    pub fn freeze_ok(&self) -> bool {
        !self.frozen || !self.changed_after_freeze
    }
}

// ─────────────────────────────────────────────────────────────
// 测试清单
// ─────────────────────────────────────────────────────────────

/// 测试清单。
#[derive(Clone, Debug, Default)]
pub struct TestCatalog {
    entries: Vec<TestEntry>,
}

impl TestCatalog {
    /// 空清单。
    pub fn new() -> TestCatalog {
        TestCatalog {
            entries: Vec::new(),
        }
    }

    /// 登记。
    pub fn add(&mut self, e: TestEntry) {
        self.entries.push(e);
    }

    /// 扫描无断言。
    pub fn scan_no_assert(&self) -> Vec<u32> {
        let mut bad = Vec::new();
        for e in self.entries.iter() {
            if !e.assert_ok() {
                bad.push(e.id);
            }
        }
        bad
    }

    /// 扫描冻结违例。
    pub fn scan_freeze_violate(&self) -> Vec<u32> {
        let mut bad = Vec::new();
        for e in self.entries.iter() {
            if !e.freeze_ok() {
                bad.push(e.id);
            }
        }
        bad
    }

    /// 三级齐备。
    pub fn all_levels(&self) -> bool {
        for l in TestLevel::all() {
            if !self.entries.iter().any(|e| e.level == l) {
                return false;
            }
        }
        true
    }

    /// 缺级。
    pub fn missing_levels(&self) -> Vec<TestLevel> {
        let mut bad = Vec::new();
        for l in TestLevel::all() {
            if !self.entries.iter().any(|e| e.level == l) {
                bad.push(l);
            }
        }
        bad
    }

    /// 平均覆盖（万分制）。
    pub fn avg_cover_bp(&self) -> u16 {
        if self.entries.is_empty() {
            return 0;
        }
        let sum: u32 = self.entries.iter().map(|e| e.cover_bp as u32).sum();
        (sum / self.entries.len() as u32) as u16
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 空判断。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// ─────────────────────────────────────────────────────────────
// 覆盖对拍
// ─────────────────────────────────────────────────────────────

/// 覆盖对拍对。
#[derive(Clone, Debug)]
pub struct CoverPair {
    /// 测试 id。
    pub id: u32,
    /// 甲方覆盖声明。
    pub cover_a: u16,
    /// 乙方覆盖声明。
    pub cover_b: u16,
}

impl CoverPair {
    /// 构造。
    pub fn new(id: u32, cover_a: u16, cover_b: u16) -> CoverPair {
        CoverPair {
            id,
            cover_a,
            cover_b,
        }
    }

    /// 一致（容差 100 = 1%）。
    pub fn match_ok(&self) -> bool {
        let a = self.cover_a as i32;
        let b = self.cover_b as i32;
        let d = if a > b { a - b } else { b - a };
        d <= 100
    }
}

/// 覆盖对拍表。
#[derive(Clone, Debug, Default)]
pub struct CoverLedger {
    pairs: Vec<CoverPair>,
}

impl CoverLedger {
    /// 空。
    pub fn new() -> CoverLedger {
        CoverLedger { pairs: Vec::new() }
    }

    /// 登记。
    pub fn add(&mut self, p: CoverPair) {
        self.pairs.push(p);
    }

    /// 扫描分歧。
    pub fn scan_diverge(&self) -> Vec<u32> {
        let mut bad = Vec::new();
        for p in self.pairs.iter() {
            if !p.match_ok() {
                bad.push(p.id);
            }
        }
        bad
    }

    /// 对数。
    pub fn len(&self) -> usize {
        self.pairs.len()
    }

    /// 空判断。
    pub fn is_empty(&self) -> bool {
        self.pairs.is_empty()
    }
}

// ─────────────────────────────────────────────────────────────
// 判据
// ─────────────────────────────────────────────────────────────

/// F3767 判据集。
pub fn run_vr09g_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3767");

    // 0x9E01 断言硬门
    {
        let mut c = TestCatalog::new();
        c.add(TestEntry::new(1, TestLevel::Unit));
        out.add("0x9E01-无断言检出", c.scan_no_assert().len() == 1, "");
    }

    // 0x9E02 三级
    {
        let mut c = TestCatalog::new();
        out.add("0x9E02-全缺检出", c.missing_levels().len() == 3, "");
        for l in TestLevel::all() {
            let mut e = TestEntry::new(l.index() as u32 + 10, l);
            e.has_assert = true;
            c.add(e);
        }
        out.add("0x9E02-三级齐过", c.all_levels(), "");
    }

    // 0x9E03 覆盖对拍
    {
        let mut l = CoverLedger::new();
        l.add(CoverPair::new(1, 8000, 8050));
        out.add("0x9E03-容差内过", l.scan_diverge().is_empty(), "");
        l.add(CoverPair::new(2, 5000, 7000));
        out.add("0x9E03-超容差检出", l.scan_diverge().len() == 1, "");
    }

    // 0x9E04 冻结
    {
        let mut c = TestCatalog::new();
        let mut e = TestEntry::new(5, TestLevel::Unit);
        e.has_assert = true;
        e.frozen = true;
        e.changed_after_freeze = true;
        c.add(e);
        out.add("0x9E04-冻结后改检出", c.scan_freeze_violate().len() == 1, "");
    }

    // 0x9E06 覆盖
    {
        let mut c = TestCatalog::new();
        let mut e = TestEntry::new(6, TestLevel::Unit);
        e.has_assert = true;
        e.cover_bp = 8500;
        c.add(e);
        out.add("0x9E06-覆盖记录", c.avg_cover_bp() == 8500, "");
    }

    out
}
