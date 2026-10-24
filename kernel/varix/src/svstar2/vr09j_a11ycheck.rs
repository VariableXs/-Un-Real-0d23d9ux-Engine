//! VE-F3770 · R 域无障碍面终版清点
//!
//! 锚点：R 域无障碍面终版清点——无障碍清点（八组无障碍面清单（复述清单红线：
//! 无障碍项必须有 WCAG 对应（WCAG 硬门）；无障碍分级（A/AA/AAA 三级（复述分级）；
//! 无障碍对拍（八组无障碍声明一致（复述对拍）；无障碍冻结（终版冻结（复述冻结）。
//!
//! 数据结构：无障碍清单；三级分级；对拍记录；冻结清单。
//! 错误路径与降级矩阵：无 WCAG 对应→拒（硬门实测）；分级缺→补（复述）；对拍分歧→
//! 立案（复述红线）；冻结后改→回滚（复述）。
//! 跨批对接点：八组档案；F3765 对账输入；F3771 文档对端。
//!
//! 诊断码段：独占 0xA1xx。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ─────────────────────────────────────────────────────────────
// 诊断码（0xA1xx）
// ─────────────────────────────────────────────────────────────

/// F3770 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct T9Code(pub u16);

impl T9Code {
    /// 无 WCAG 对应——硬门。
    pub const NO_WCAG: T9Code = T9Code(0xA101);
    /// 分级缺失——三级。
    pub const LEVEL_MISS: T9Code = T9Code(0xA102);
    /// 对拍分歧——立案。
    pub const PAIR_DIVERGE: T9Code = T9Code(0xA103);
    /// 冻结后改——回滚。
    pub const FREEZE_VIOLATE: T9Code = T9Code(0xA104);
    /// 清单不全——补。
    pub const LIST_INCOMPLETE: T9Code = T9Code(0xA105);
    /// AA 未达——红线。
    pub const AA_MISS: T9Code = T9Code(0xA106);

    /// 诊断说明。
    pub fn explain(self) -> &'static str {
        match self.0 {
            0xA101 => "无障碍项无 WCAG 对应，硬门拒收。",
            0xA102 => "无障碍分级缺失（A/AA/AAA）。",
            0xA103 => "无障碍声明对拍分歧，需立案。",
            0xA104 => "冻结后修改，需回滚。",
            0xA105 => "无障碍清单不全。",
            0xA106 => "WCAG AA 级未达，红线不通过。",
            _ => "未知诊断码。",
        }
    }

    /// 数值。
    pub fn raw(self) -> u16 {
        self.0
    }
}

// ─────────────────────────────────────────────────────────────
// WCAG 三级
// ─────────────────────────────────────────────────────────────

/// WCAG 级别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WcagLevel {
    /// A。
    A,
    /// AA。
    AA,
    /// AAA。
    AAA,
}

impl WcagLevel {
    /// 稳定序号。
    pub fn index(self) -> u8 {
        match self {
            WcagLevel::A => 0,
            WcagLevel::AA => 1,
            WcagLevel::AAA => 2,
        }
    }

    /// 全集。
    pub fn all() -> [WcagLevel; 3] {
        [WcagLevel::A, WcagLevel::AA, WcagLevel::AAA]
    }

    /// 是否 ≥ AA。
    pub fn at_least_aa(self) -> bool {
        match self {
            WcagLevel::AA | WcagLevel::AAA => true,
            WcagLevel::A => false,
        }
    }
}

/// 无障碍条目。
#[derive(Clone, Debug)]
pub struct A11yEntry {
    /// id。
    pub id: u32,
    /// WCAG 级别。
    pub level: WcagLevel,
    /// 是否有 WCAG 对应条款。
    pub has_wcag: bool,
    /// WCAG 条款 id（0=无）。
    pub wcag_id: u32,
    /// 冻结后被改。
    pub changed_after_freeze: bool,
    /// 已冻结。
    pub frozen: bool,
}

impl A11yEntry {
    /// 构造。
    pub fn new(id: u32, level: WcagLevel) -> A11yEntry {
        A11yEntry {
            id,
            level,
            has_wcag: false,
            wcag_id: 0,
            changed_after_freeze: false,
            frozen: false,
        }
    }

    /// WCAG 合规：有对应 + 条款 id。
    pub fn wcag_ok(&self) -> bool {
        self.has_wcag && self.wcag_id != 0
    }

    /// AA 达标。
    pub fn aa_ok(&self) -> bool {
        self.level.at_least_aa()
    }

    /// 冻结合规。
    pub fn freeze_ok(&self) -> bool {
        !self.frozen || !self.changed_after_freeze
    }
}

// ─────────────────────────────────────────────────────────────
// 无障碍清单
// ─────────────────────────────────────────────────────────────

/// 无障碍清单。
#[derive(Clone, Debug, Default)]
pub struct A11yCatalog {
    entries: Vec<A11yEntry>,
}

impl A11yCatalog {
    /// 空清单。
    pub fn new() -> A11yCatalog {
        A11yCatalog {
            entries: Vec::new(),
        }
    }

    /// 登记。
    pub fn add(&mut self, e: A11yEntry) {
        self.entries.push(e);
    }

    /// 扫描无 WCAG。
    pub fn scan_no_wcag(&self) -> Vec<u32> {
        let mut bad = Vec::new();
        for e in self.entries.iter() {
            if !e.wcag_ok() {
                bad.push(e.id);
            }
        }
        bad
    }

    /// 扫描 AA 未达。
    pub fn scan_aa_miss(&self) -> Vec<u32> {
        let mut bad = Vec::new();
        for e in self.entries.iter() {
            if !e.aa_ok() {
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
        for l in WcagLevel::all() {
            if !self.entries.iter().any(|e| e.level == l) {
                return false;
            }
        }
        true
    }

    /// 缺级。
    pub fn missing_levels(&self) -> Vec<WcagLevel> {
        let mut bad = Vec::new();
        for l in WcagLevel::all() {
            if !self.entries.iter().any(|e| e.level == l) {
                bad.push(l);
            }
        }
        bad
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
// 无障碍对拍
// ─────────────────────────────────────────────────────────────

/// 对拍对。
#[derive(Clone, Debug)]
pub struct A11yPair {
    /// id。
    pub id: u32,
    /// 甲方级别。
    pub level_a: WcagLevel,
    /// 乙方级别。
    pub level_b: WcagLevel,
}

impl A11yPair {
    /// 构造。
    pub fn new(id: u32, level_a: WcagLevel, level_b: WcagLevel) -> A11yPair {
        A11yPair {
            id,
            level_a,
            level_b,
        }
    }

    /// 一致。
    pub fn match_ok(&self) -> bool {
        self.level_a == self.level_b
    }
}

/// 对拍表。
#[derive(Clone, Debug, Default)]
pub struct A11yPairLedger {
    pairs: Vec<A11yPair>,
}

impl A11yPairLedger {
    /// 空。
    pub fn new() -> A11yPairLedger {
        A11yPairLedger { pairs: Vec::new() }
    }

    /// 登记。
    pub fn add(&mut self, p: A11yPair) {
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

/// F3770 判据集。
pub fn run_vr09j_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3770");

    // 0xA101 WCAG 硬门
    {
        let mut c = A11yCatalog::new();
        c.add(A11yEntry::new(1, WcagLevel::A));
        out.add("0xA101-无WCAG检出", c.scan_no_wcag().len() == 1, "");
    }

    // 0xA102 三级
    {
        let mut c = A11yCatalog::new();
        out.add("0xA102-全缺检出", c.missing_levels().len() == 3, "");
        for l in WcagLevel::all() {
            let mut e = A11yEntry::new(l.index() as u32 + 10, l);
            e.has_wcag = true;
            e.wcag_id = 100 + l.index() as u32;
            c.add(e);
        }
        out.add("0xA102-三级齐过", c.all_levels(), "");
    }

    // 0xA106 AA 达标
    {
        let mut c = A11yCatalog::new();
        let mut e = A11yEntry::new(5, WcagLevel::A);
        e.has_wcag = true;
        e.wcag_id = 1;
        c.add(e);
        out.add("0xA106-A未达AA检出", c.scan_aa_miss().len() == 1, "");
        c.entries[0].level = WcagLevel::AA;
        out.add("0xA106-AA达标过", c.scan_aa_miss().is_empty(), "");
    }

    // 0xA103 对拍
    {
        let mut l = A11yPairLedger::new();
        l.add(A11yPair::new(1, WcagLevel::AA, WcagLevel::AA));
        out.add("0xA103-一致过", l.scan_diverge().is_empty(), "");
        l.add(A11yPair::new(2, WcagLevel::A, WcagLevel::AAA));
        out.add("0xA103-分歧检出", l.scan_diverge().len() == 1, "");
    }

    // 0xA104 冻结
    {
        let mut c = A11yCatalog::new();
        let mut e = A11yEntry::new(6, WcagLevel::AA);
        e.has_wcag = true;
        e.wcag_id = 1;
        e.frozen = true;
        e.changed_after_freeze = true;
        c.add(e);
        out.add("0xA104-冻结后改检出", c.scan_freeze_violate().len() == 1, "");
    }

    out
}
