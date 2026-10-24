//! VE-F3769 · R 域安全面终版清点
//!
//! 锚点：R 域安全面终版清点——安全清点（八组安全面清单（复述清单红线：
//! 安全项必须有威胁模型（威胁模型硬门）；安全分级（P0/P1/P2 三级（复述分级）；
//! 安全对拍（八组安全声明一致（复述对拍）；安全冻结（终版冻结（复述冻结）。
//!
//! 数据结构：安全清单；三级分级；对拍记录；冻结清单。
//! 错误路径与降级矩阵：无威胁模型→拒（硬门实测）；分级缺→补（复述）；对拍分歧→
//! 立案（复述红线）；冻结后改→回滚（复述）。
//! 跨批对接点：八组档案；F3765 对账输入；F3770 无障碍对端。
//!
//! 诊断码段：独占 0xA0xx。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ─────────────────────────────────────────────────────────────
// 诊断码（0xA0xx）
// ─────────────────────────────────────────────────────────────

/// F3769 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct S9Code(pub u16);

impl S9Code {
    /// 无威胁模型——硬门。
    pub const NO_THREAT_MODEL: S9Code = S9Code(0xA001);
    /// 分级缺失——三级。
    pub const SEVERITY_MISS: S9Code = S9Code(0xA002);
    /// 对拍分歧——立案。
    pub const PAIR_DIVERGE: S9Code = S9Code(0xA003);
    /// 冻结后改——回滚。
    pub const FREEZE_VIOLATE: S9Code = S9Code(0xA004);
    /// 清单不全——补。
    pub const LIST_INCOMPLETE: S9Code = S9Code(0xA005);
    /// P0 未闭环——阻断。
    pub const P0_OPEN: S9Code = S9Code(0xA006);

    /// 诊断说明。
    pub fn explain(self) -> &'static str {
        match self.0 {
            0xA001 => "安全项无威胁模型，硬门拒收。",
            0xA002 => "安全分级缺失（P0/P1/P2）。",
            0xA003 => "安全声明对拍分歧，需立案。",
            0xA004 => "冻结后修改，需回滚。",
            0xA005 => "安全清单不全。",
            0xA006 => "P0 安全问题未闭环，阻断。",
            _ => "未知诊断码。",
        }
    }

    /// 数值。
    pub fn raw(self) -> u16 {
        self.0
    }
}

// ─────────────────────────────────────────────────────────────
// 安全三级
// ─────────────────────────────────────────────────────────────

/// 安全级别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SecLevel {
    /// P0（阻断）。
    P0,
    /// P1（严重）。
    P1,
    /// P2（一般）。
    P2,
}

impl SecLevel {
    /// 稳定序号。
    pub fn index(self) -> u8 {
        match self {
            SecLevel::P0 => 0,
            SecLevel::P1 => 1,
            SecLevel::P2 => 2,
        }
    }

    /// 全集。
    pub fn all() -> [SecLevel; 3] {
        [SecLevel::P0, SecLevel::P1, SecLevel::P2]
    }
}

/// 安全条目。
#[derive(Clone, Debug)]
pub struct SecEntry {
    /// id。
    pub id: u32,
    /// 级别。
    pub level: SecLevel,
    /// 是否有威胁模型。
    pub has_threat_model: bool,
    /// 是否已闭环。
    pub closed: bool,
    /// 冻结后被改。
    pub changed_after_freeze: bool,
    /// 已冻结。
    pub frozen: bool,
}

impl SecEntry {
    /// 构造。
    pub fn new(id: u32, level: SecLevel) -> SecEntry {
        SecEntry {
            id,
            level,
            has_threat_model: false,
            closed: false,
            changed_after_freeze: false,
            frozen: false,
        }
    }

    /// 威胁模型合规。
    pub fn model_ok(&self) -> bool {
        self.has_threat_model
    }

    /// P0 闭环合规：非 P0 或 已闭环。
    pub fn p0_ok(&self) -> bool {
        if self.level != SecLevel::P0 {
            return true;
        }
        self.closed
    }

    /// 冻结合规。
    pub fn freeze_ok(&self) -> bool {
        !self.frozen || !self.changed_after_freeze
    }
}

// ─────────────────────────────────────────────────────────────
// 安全清单
// ─────────────────────────────────────────────────────────────

/// 安全清单。
#[derive(Clone, Debug, Default)]
pub struct SecCatalog {
    entries: Vec<SecEntry>,
}

impl SecCatalog {
    /// 空清单。
    pub fn new() -> SecCatalog {
        SecCatalog {
            entries: Vec::new(),
        }
    }

    /// 登记。
    pub fn add(&mut self, e: SecEntry) {
        self.entries.push(e);
    }

    /// 扫描无威胁模型。
    pub fn scan_no_model(&self) -> Vec<u32> {
        let mut bad = Vec::new();
        for e in self.entries.iter() {
            if !e.model_ok() {
                bad.push(e.id);
            }
        }
        bad
    }

    /// 扫描 P0 未闭环。
    pub fn scan_p0_open(&self) -> Vec<u32> {
        let mut bad = Vec::new();
        for e in self.entries.iter() {
            if !e.p0_ok() {
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
        for l in SecLevel::all() {
            if !self.entries.iter().any(|e| e.level == l) {
                return false;
            }
        }
        true
    }

    /// 缺级。
    pub fn missing_levels(&self) -> Vec<SecLevel> {
        let mut bad = Vec::new();
        for l in SecLevel::all() {
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
// 安全对拍
// ─────────────────────────────────────────────────────────────

/// 对拍对。
#[derive(Clone, Debug)]
pub struct SecPair {
    /// id。
    pub id: u32,
    /// 甲方级别。
    pub level_a: SecLevel,
    /// 乙方级别。
    pub level_b: SecLevel,
}

impl SecPair {
    /// 构造。
    pub fn new(id: u32, level_a: SecLevel, level_b: SecLevel) -> SecPair {
        SecPair {
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
pub struct SecPairLedger {
    pairs: Vec<SecPair>,
}

impl SecPairLedger {
    /// 空。
    pub fn new() -> SecPairLedger {
        SecPairLedger { pairs: Vec::new() }
    }

    /// 登记。
    pub fn add(&mut self, p: SecPair) {
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

/// F3769 判据集。
pub fn run_vr09i_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3769");

    // 0xA001 威胁模型硬门
    {
        let mut c = SecCatalog::new();
        c.add(SecEntry::new(1, SecLevel::P1));
        out.add("0xA001-无模型检出", c.scan_no_model().len() == 1, "");
    }

    // 0xA002 三级
    {
        let mut c = SecCatalog::new();
        out.add("0xA002-全缺检出", c.missing_levels().len() == 3, "");
        for l in SecLevel::all() {
            let mut e = SecEntry::new(l.index() as u32 + 10, l);
            e.has_threat_model = true;
            e.closed = true;
            c.add(e);
        }
        out.add("0xA002-三级齐过", c.all_levels(), "");
    }

    // 0xA006 P0 闭环
    {
        let mut c = SecCatalog::new();
        let mut e = SecEntry::new(5, SecLevel::P0);
        e.has_threat_model = true;
        e.closed = false;
        c.add(e);
        out.add("0xA006-P0未闭环检出", c.scan_p0_open().len() == 1, "");
        c.entries[0].closed = true;
        out.add("0xA006-闭环后过", c.scan_p0_open().is_empty(), "");
    }

    // 0xA003 对拍
    {
        let mut l = SecPairLedger::new();
        l.add(SecPair::new(1, SecLevel::P1, SecLevel::P1));
        out.add("0xA003-一致过", l.scan_diverge().is_empty(), "");
        l.add(SecPair::new(2, SecLevel::P0, SecLevel::P2));
        out.add("0xA003-分歧检出", l.scan_diverge().len() == 1, "");
    }

    // 0xA004 冻结
    {
        let mut c = SecCatalog::new();
        let mut e = SecEntry::new(6, SecLevel::P1);
        e.has_threat_model = true;
        e.frozen = true;
        e.changed_after_freeze = true;
        c.add(e);
        out.add("0xA004-冻结后改检出", c.scan_freeze_violate().len() == 1, "");
    }

    out
}
