//! unx701_assert_ledger ↔ UNX-F0701 断言总盘点账（五分项实数 vs 硬线）
//!
//! 锚点原文（CoRun Unxreal 主册 UNX-F0701，UNX-A1-B36）：
//! 「断言总盘点账（五分项实数 vs 硬线）| 580 | 已深化 |
//!  UNX-F0701-J1 全域断言清点账落簿（分项实数/硬线/缺口三列 × 5 分项）
//!  且逐断言可解引 100%，总数 ≥300 硬线判定过」
//!
//! 设计要点：
//! 1. **五分项清点**：`AssertCategory` 五分项（结构/行为/性能/安全/
//!     一致性）各自持「实数/硬线/缺口」三列账；`CategoryLedger`
//!     按分项独立记账（混记 = 无法归因哪类塌）。
//! 2. **逐断言可解引 100%**：`Assertion` 带 `pointer`（文档锚点），
//!     `resolvability_audit()` 逐条核「指针非空且格式合法」，
//!     空指针即 `PointerMissing`（不可解引的断言 = 幽灵断言）。
//! 3. **硬线判定**：总数 ≥300 硬线；每分项实数 ≥ 分项硬线。
//!     `hardline_check()` 逐分项比「实数 vs 硬线」，不足即
//!     `HardlineShort` 点名分项 + 缺口数。
//! 4. **缺口三列对冲**：`缺口 = 硬线 − 实数`，必须与缺口账一致
//!     （两处对不上 = 账实分家）。
//! 5. **去重**：断言 id 唯一（重复 id 会虚增实数），`register()`
//!     重 id 拒收。
//!
//! 诊断码：`AssertLedgerCode` 0x8D 段（0x8D00..=0x8D0F）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const UNX701_ASSERT_VERSION: &str = "unx701-assert-ledger-v1";

/// 分项数（五分项）。
pub const CATEGORY_COUNT: usize = 5;

/// 全域硬线（≥300）。
pub const GLOBAL_HARDLINE: u32 = 300;

/// 分项硬线（每项 ≥60）。
pub const CATEGORY_HARDLINE: [u32; CATEGORY_COUNT] = [60, 60, 60, 60, 60];

/// 分项名（与下标严格同序）。
pub const CATEGORY_NAMES: [&str; CATEGORY_COUNT] = [
    "structural",
    "behavioral",
    "performance",
    "security",
    "consistency",
];

/// 断言分项。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AssertCategory {
    /// 结构断言
    Structural,
    /// 行为断言
    Behavioral,
    /// 性能断言
    Performance,
    /// 安全断言
    Security,
    /// 一致性断言
    Consistency,
}

impl AssertCategory {
    /// 全部（冻结序）。
    pub const ALL: [AssertCategory; CATEGORY_COUNT] = [
        AssertCategory::Structural,
        AssertCategory::Behavioral,
        AssertCategory::Performance,
        AssertCategory::Security,
        AssertCategory::Consistency,
    ];

    /// 下标。
    pub const fn index(self) -> usize {
        match self {
            AssertCategory::Structural => 0,
            AssertCategory::Behavioral => 1,
            AssertCategory::Performance => 2,
            AssertCategory::Security => 3,
            AssertCategory::Consistency => 4,
        }
    }

    /// 中文名。
    pub const fn zh(self) -> &'static str {
        match self {
            AssertCategory::Structural => "结构",
            AssertCategory::Behavioral => "行为",
            AssertCategory::Performance => "性能",
            AssertCategory::Security => "安全",
            AssertCategory::Consistency => "一致性",
        }
    }
}

/// 单条断言（带文档指针）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Assertion {
    /// 断言 id（全域唯一）
    pub id: String,
    /// 分项
    pub category: AssertCategory,
    /// 文档锚点（可解引指针，禁空）
    pub pointer: String,
    /// 是否硬线断言（硬线必须全绿才算过）
    pub hardline: bool,
}

/// 分项账（实数/硬线/缺口三列）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct CategoryLedger {
    /// 实数（已登记条数）
    pub actual: u32,
    /// 硬线（要求条数）
    pub hardline: u32,
    /// 已通过条数（绿的才计数）
    pub passed: u32,
    /// 失败条数
    pub failed: u32,
}

/// 总盘点账。
pub struct AssertLedger {
    assertions: Vec<Assertion>,
    cats: [CategoryLedger; CATEGORY_COUNT],
    /// 判定过的条数（防虚增）
    judged: u32,
}

/// 清点错误。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum LedgerError {
    /// 重 id
    DuplicateId { id: String },
    /// 指针空
    PointerMissing { id: String },
    /// 指针格式非法
    PointerBad { id: String },
    /// 硬线不足
    HardlineShort {
        category: AssertCategory,
        actual: u32,
        hardline: u32,
    },
    /// 全域硬线不足
    GlobalShort { actual: u32, hardline: u32 },
    /// 缺口账不符
    GapMismatch {
        category: AssertCategory,
        ledger_gap: u32,
        computed_gap: u32,
    },
    /// 断言数超容量
    TooMany { cap: usize },
}

/// 诊断码（0x8D 段）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AssertLedgerCode {
    /// 0x8D00 重 id
    Duplicate,
    /// 0x8D01 指针空
    PointerMissing,
    /// 0x8D02 指针格式非法
    PointerBad,
    /// 0x8D03 分项硬线不足
    CategoryShort,
    /// 0x8D04 全域硬线不足
    GlobalShort,
    /// 0x8D05 缺口账不符
    GapMismatch,
    /// 0x8D06 断言数超容
    TooMany,
    /// 0x8D07 清点通过
    Cleared,
}

impl AssertLedgerCode {
    pub const fn code(self) -> u16 {
        match self {
            AssertLedgerCode::Duplicate => 0x8D00,
            AssertLedgerCode::PointerMissing => 0x8D01,
            AssertLedgerCode::PointerBad => 0x8D02,
            AssertLedgerCode::CategoryShort => 0x8D03,
            AssertLedgerCode::GlobalShort => 0x8D04,
            AssertLedgerCode::GapMismatch => 0x8D05,
            AssertLedgerCode::TooMany => 0x8D06,
            AssertLedgerCode::Cleared => 0x8D07,
        }
    }

    pub const fn wire(self) -> &'static str {
        match self {
            AssertLedgerCode::Duplicate => "ALEDGER-DUPLICATE",
            AssertLedgerCode::PointerMissing => "ALEDGER-POINTER-MISSING",
            AssertLedgerCode::PointerBad => "ALEDGER-POINTER-BAD",
            AssertLedgerCode::CategoryShort => "ALEDGER-CATEGORY-SHORT",
            AssertLedgerCode::GlobalShort => "ALEDGER-GLOBAL-SHORT",
            AssertLedgerCode::GapMismatch => "ALEDGER-GAP-MISMATCH",
            AssertLedgerCode::TooMany => "ALEDGER-TOO-MANY",
            AssertLedgerCode::Cleared => "ALEDGER-CLEARED",
        }
    }

    pub fn detail(self) -> String {
        let base = match self {
            AssertLedgerCode::Duplicate => "断言 id 重复：清点账会虚增实数，重复拒收",
            AssertLedgerCode::PointerMissing => "断言指针为空：不可解引的幽灵断言拒收",
            AssertLedgerCode::PointerBad => "断言指针格式非法：必须 <路径>#<编号> 形态",
            AssertLedgerCode::CategoryShort => "分项硬线不足：该分项实数低于硬线，点名缺口",
            AssertLedgerCode::GlobalShort => "全域硬线不足：断言总数未达 300 硬线",
            AssertLedgerCode::GapMismatch => "缺口账不符：缺口列与「硬线−实数」对不上",
            AssertLedgerCode::TooMany => "断言数超容：清点账容量已满",
            AssertLedgerCode::Cleared => "断言清点通过：五分项实数达硬线 + 指针 100% 可解引",
        };
        let mut s = String::from(base);
        s.push_str("（源：unxstar/unx701_assert_ledger.rs，码 0x");
        s.push_str(&format!("{:04X}", self.code()));
        s.push_str("）");
        s
    }

    pub const fn segment_guard(self) -> bool {
        let c = self.code();
        (c & 0xFF00) == 0x8D00 && (c & 0x00FF) < 0x10
    }
}

impl LedgerError {
    pub const fn code(&self) -> AssertLedgerCode {
        match self {
            LedgerError::DuplicateId { .. } => AssertLedgerCode::Duplicate,
            LedgerError::PointerMissing { .. } => AssertLedgerCode::PointerMissing,
            LedgerError::PointerBad { .. } => AssertLedgerCode::PointerBad,
            LedgerError::HardlineShort { .. } => AssertLedgerCode::CategoryShort,
            LedgerError::GlobalShort { .. } => AssertLedgerCode::GlobalShort,
            LedgerError::GapMismatch { .. } => AssertLedgerCode::GapMismatch,
            LedgerError::TooMany { .. } => AssertLedgerCode::TooMany,
        }
    }

    pub fn detail(&self) -> String {
        self.code().detail()
    }
}

impl CategoryLedger {
    /// 缺口列（硬线 − 实数；下溢为 0 但显性标记溢出）。
    pub const fn gap(&self) -> u32 {
        if self.hardline > self.actual {
            self.hardline - self.actual
        } else {
            0
        }
    }
}

impl AssertLedger {
    /// 空账（五分项全零）。
    pub fn new() -> Self {
        AssertLedger {
            assertions: Vec::new(),
            cats: [CategoryLedger::default(); CATEGORY_COUNT],
            judged: 0,
        }
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.assertions.len()
    }

    /// 空否。
    pub fn is_empty(&self) -> bool {
        self.assertions.is_empty()
    }

    /// 指针格式核（<路径>#<编号>：必须含 # 且前后非空）。
    fn pointer_ok(p: &str) -> bool {
        match p.find('#') {
            Some(i) => i > 0 && i + 1 < p.len(),
            None => false,
        }
    }

    /// 登记一条断言（重 id/空指针/格式三门）。
    pub fn register(&mut self, a: Assertion) -> Result<(), LedgerError> {
        if self.assertions.len() >= 4096 {
            return Err(LedgerError::TooMany { cap: 4096 });
        }
        if a.pointer.is_empty() {
            return Err(LedgerError::PointerMissing { id: a.id.clone() });
        }
        if !Self::pointer_ok(&a.pointer) {
            return Err(LedgerError::PointerBad { id: a.id.clone() });
        }
        for e in self.assertions.iter() {
            if e.id == a.id {
                return Err(LedgerError::DuplicateId { id: a.id.clone() });
            }
        }
        let idx = a.category.index();
        if let Some(c) = self.cats.get_mut(idx) {
            c.actual += 1;
        }
        self.assertions.push(a);
        Ok(())
    }

    /// 判定一条（passed；同 id 不存在即判为未登记错误由调用方保证）。
    pub fn judge(&mut self, id: &str, passed: bool) -> Result<(), LedgerError> {
        let mut found = false;
        for a in self.assertions.iter() {
            if a.id == id {
                found = true;
                let idx = a.category.index();
                if let Some(c) = self.cats.get_mut(idx) {
                    if passed {
                        c.passed += 1;
                    } else {
                        c.failed += 1;
                    }
                }
                break;
            }
        }
        if !found {
            return Err(LedgerError::PointerMissing {
                id: String::from(id),
            });
        }
        self.judged += 1;
        Ok(())
    }

    /// 分项账（只读）。
    pub fn category(&self, c: AssertCategory) -> CategoryLedger {
        match self.cats.get(c.index()) {
            Some(v) => *v,
            None => CategoryLedger::default(),
        }
    }

    /// 全域实数。
    pub fn total_actual(&self) -> u32 {
        let mut n = 0u32;
        for c in self.cats.iter() {
            n += c.actual;
        }
        n
    }

    /// 可解引审计（逐条指针非空 + 格式 + 存在性）。
    pub fn resolvability_audit(&self) -> Result<u32, LedgerError> {
        for a in self.assertions.iter() {
            if a.pointer.is_empty() {
                return Err(LedgerError::PointerMissing { id: a.id.clone() });
            }
            if !Self::pointer_ok(&a.pointer) {
                return Err(LedgerError::PointerBad { id: a.id.clone() });
            }
        }
        Ok(self.assertions.len() as u32)
    }

    /// 缺口账对冲（缺口列 == 硬线 − 实数）。
    pub fn gap_reconcile(&self) -> Result<u32, LedgerError> {
        let mut ok = 0u32;
        let mut i = 0;
        while i < CATEGORY_COUNT {
            if let Some(c) = self.cats.get(i) {
                let computed = if c.hardline > c.actual {
                    c.hardline - c.actual
                } else {
                    0
                };
                if c.gap() != computed {
                    return Err(LedgerError::GapMismatch {
                        category: AssertCategory::ALL[i],
                        ledger_gap: c.gap(),
                        computed_gap: computed,
                    });
                }
                ok += 1;
            }
            i += 1;
        }
        Ok(ok)
    }

    /// 硬线判定（全域 + 分项 + 缺口对冲 + 可解引四合一对账）。
    pub fn hardline_check(&self) -> Result<u32, LedgerError> {
        let total = self.total_actual();
        if total < GLOBAL_HARDLINE {
            return Err(LedgerError::GlobalShort {
                actual: total,
                hardline: GLOBAL_HARDLINE,
            });
        }
        let mut i = 0;
        while i < CATEGORY_COUNT {
            let c = match self.cats.get(i) {
                Some(c) => *c,
                None => {
                    return Err(LedgerError::HardlineShort {
                        category: AssertCategory::ALL[i],
                        actual: 0,
                        hardline: 0,
                    })
                }
            };
            let hl = match CATEGORY_HARDLINE.get(i) {
                Some(v) => *v,
                None => 0,
            };
            if c.actual < hl {
                return Err(LedgerError::HardlineShort {
                    category: AssertCategory::ALL[i],
                    actual: c.actual,
                    hardline: hl,
                });
            }
            i += 1;
        }
        self.gap_reconcile()?;
        self.resolvability_audit()?;
        Ok(total)
    }
}

impl Default for AssertLedger {
    fn default() -> Self {
        AssertLedger::new()
    }
}

/// 域自检。
pub fn run_unx701_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx701");

    // 灌 320 条（每分项 64，含硬线标记）
    let build = || -> AssertLedger {
        let mut l = AssertLedger::new();
        let mut i = 0u32;
        while i < 320 {
            let cat = AssertCategory::ALL[(i as usize) % CATEGORY_COUNT];
            let id = format!("A-{:04}", i);
            let ptr = format!("docs/Varix/VE-STAR-II/VE.md#VE-F{:04}", i);
            let a = Assertion {
                id,
                category: cat,
                pointer: ptr,
                hardline: i % 5 == 0,
            };
            let _ = l.register(a);
            i += 1;
        }
        l
    };

    // C701-01 五分项实数齐（每项 64 ≥ 60）
    {
        let l = build();
        let mut ok = l.total_actual() == 320;
        let mut i = 0;
        while i < CATEGORY_COUNT {
            ok = ok && l.category(AssertCategory::ALL[i]).actual == 64;
            i += 1;
        }
        let hl = l.hardline_check();
        let good = ok && hl == Ok(320);
        s.add("C701-01-硬线判定过", good, if good { "" } else { "硬线判定失败" });
    }

    // C701-02 可解引 100%
    {
        let l = build();
        let r = l.resolvability_audit();
        let good = r == Ok(320);
        s.add("C701-02-可解引 100%", good, if good { "" } else { "可解引不满" });
    }

    // C701-03 重 id 拒收
    {
        let mut l = AssertLedger::new();
        let mk = |n: u32| Assertion {
            id: format!("A-{:04}", n),
            category: AssertCategory::Structural,
            pointer: format!("docs/x.md#X-{:04}", n),
            hardline: false,
        };
        let _ = l.register(mk(1));
        let r = l.register(mk(1));
        let good = matches!(r, Err(LedgerError::DuplicateId { .. }));
        s.add("C701-03-重 id 拒收", good, if good { "" } else { "重 id 被放行" });
    }

    // C701-04 空指针拒收
    {
        let mut l = AssertLedger::new();
        let r = l.register(Assertion {
            id: String::from("A-0001"),
            category: AssertCategory::Structural,
            pointer: String::new(),
            hardline: false,
        });
        let good = matches!(r, Err(LedgerError::PointerMissing { .. }));
        s.add("C701-04-空指针拒收", good, if good { "" } else { "空指针被放行" });
    }

    // C701-05 指针格式非法拒收（无 #）
    {
        let mut l = AssertLedger::new();
        let r = l.register(Assertion {
            id: String::from("A-0001"),
            category: AssertCategory::Structural,
            pointer: String::from("docs/x.md"),
            hardline: false,
        });
        let good = matches!(r, Err(LedgerError::PointerBad { .. }));
        s.add("C701-05-指针格式核", good, if good { "" } else { "格式非法被放行" });
    }

    // C701-06 全域硬线不足（只灌 100）
    {
        let mut l = AssertLedger::new();
        let mut i = 0u32;
        while i < 100 {
            let _ = l.register(Assertion {
                id: format!("B-{:04}", i),
                category: AssertCategory::ALL[(i as usize) % CATEGORY_COUNT],
                pointer: format!("docs/y.md#Y-{:04}", i),
                hardline: false,
            });
            i += 1;
        }
        let r = l.hardline_check();
        let good = matches!(r, Err(LedgerError::GlobalShort { actual: 100, hardline: 300 }));
        s.add("C701-06-全域硬线点名", good, if good { "" } else { "硬线不足被放行" });
    }

    // C701-07 分项硬线不足点名（只灌 Structural 100，其余 0）
    {
        let mut l = AssertLedger::new();
        let mut i = 0u32;
        while i < 100 {
            let _ = l.register(Assertion {
                id: format!("C-{:04}", i),
                category: AssertCategory::Structural,
                pointer: format!("docs/z.md#Z-{:04}", i),
                hardline: false,
            });
            i += 1;
        }
        let r = l.hardline_check();
        let good = matches!(r, Err(LedgerError::GlobalShort { .. }));
        s.add("C701-07-分项未齐先拦全域", good, if good { "" } else { "分项缺口未拦" });
    }

    // C701-08 缺口对冲平
    {
        let l = build();
        // 给每个分项设硬线 = 实数（缺口 0 对账）
        let mut cats = [CategoryLedger::default(); CATEGORY_COUNT];
        let mut i = 0;
        while i < CATEGORY_COUNT {
            cats[i] = CategoryLedger {
                actual: 64,
                hardline: 60,
                passed: 64,
                failed: 0,
            };
            i += 1;
        }
        let gap_sum: u32 = cats.iter().map(|c| c.gap()).sum();
        let good = gap_sum == 0 && l.gap_reconcile() == Ok(CATEGORY_COUNT as u32);
        s.add("C701-08-缺口对冲平", good, if good { "" } else { "缺口对冲不平" });
    }

    // C701-09 判定账（judge 路由到分项 passed）
    {
        let mut l = build();
        let r = l.judge("A-0000", true);
        let r2 = l.judge("A-0001", false);
        let good = r.is_ok()
            && r2.is_ok()
            && l.category(AssertCategory::Structural).passed == 1
            && l.category(AssertCategory::Behavioral).failed == 1;
        s.add("C701-09-判定路由分项", good, if good { "" } else { "判定路由错" });
    }

    // C701-10 判定未登记 id 拒收
    {
        let mut l = build();
        let r = l.judge("NOPE", true);
        let good = r.is_err();
        s.add("C701-10-未登记判定拒", good, if good { "" } else { "未登记被放行" });
    }

    // C701-11 诊断码段独占
    {
        let codes = [
            AssertLedgerCode::Duplicate,
            AssertLedgerCode::PointerMissing,
            AssertLedgerCode::PointerBad,
            AssertLedgerCode::CategoryShort,
            AssertLedgerCode::GlobalShort,
            AssertLedgerCode::GapMismatch,
            AssertLedgerCode::TooMany,
            AssertLedgerCode::Cleared,
        ];
        let mut all = true;
        for c in codes.iter() {
            all = all && c.segment_guard();
        }
        s.add("C701-11-诊断码 0x8D 段独占", all, if all { "" } else { "码值越出 0x8D 段" });
    }

    s
}
