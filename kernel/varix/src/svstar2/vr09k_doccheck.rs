//! VE-F3771 · R 域文档面终版清点
//!
//! 锚点：R 域文档面终版清点——文档清点（八组文档面清单（复述清单红线：
//! 文档必须有版本号（版本硬门）；文档分级（用户/开发/审计三级（复述分级）；
//! 文档对拍（八组文档声明一致（复述对拍）；文档冻结（终版冻结（复述冻结）。
//!
//! 数据结构：文档清单；三级分类；对拍记录；冻结清单。
//! 错误路径与降级矩阵：无版本号→拒（硬门实测）；分级缺→补（复述）；对拍分歧→
//! 立案（复述红线）；冻结后改→回滚（复述）。
//! 跨批对接点：八组档案；F3765 对账输入；F3772 预算对端。
//!
//! 诊断码段：独占 0xA2xx。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ─────────────────────────────────────────────────────────────
// 诊断码（0xA2xx）
// ─────────────────────────────────────────────────────────────

/// F3771 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct D9Code(pub u16);

impl D9Code {
    /// 无版本号——硬门。
    pub const NO_VERSION: D9Code = D9Code(0xA201);
    /// 分级缺失——三级。
    pub const CLASS_MISS: D9Code = D9Code(0xA202);
    /// 对拍分歧——立案。
    pub const PAIR_DIVERGE: D9Code = D9Code(0xA203);
    /// 冻结后改——回滚。
    pub const FREEZE_VIOLATE: D9Code = D9Code(0xA204);
    /// 清单不全——补。
    pub const LIST_INCOMPLETE: D9Code = D9Code(0xA205);
    /// 自举失败——文档自身无障碍。
    pub const BOOTSTRAP_FAIL: D9Code = D9Code(0xA206);

    /// 诊断说明。
    pub fn explain(self) -> &'static str {
        match self.0 {
            0xA201 => "文档无版本号，硬门拒收。",
            0xA202 => "文档分级缺失（用户/开发/审计）。",
            0xA203 => "文档声明对拍分歧，需立案。",
            0xA204 => "冻结后修改，需回滚。",
            0xA205 => "文档清单不全。",
            0xA206 => "文档自举无障碍审查失败。",
            _ => "未知诊断码。",
        }
    }

    /// 数值。
    pub fn raw(self) -> u16 {
        self.0
    }
}

// ─────────────────────────────────────────────────────────────
// 文档三级
// ─────────────────────────────────────────────────────────────

/// 文档级别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DocLevel {
    /// 用户文档。
    User,
    /// 开发者文档。
    Dev,
    /// 审计文档。
    Audit,
}

impl DocLevel {
    /// 稳定序号。
    pub fn index(self) -> u8 {
        match self {
            DocLevel::User => 0,
            DocLevel::Dev => 1,
            DocLevel::Audit => 2,
        }
    }

    /// 全集。
    pub fn all() -> [DocLevel; 3] {
        [DocLevel::User, DocLevel::Dev, DocLevel::Audit]
    }
}

/// 文档条目。
#[derive(Clone, Debug)]
pub struct DocEntry {
    /// id。
    pub id: u32,
    /// 级别。
    pub level: DocLevel,
    /// 版本号（0=无版本）。
    pub version: u32,
    /// 自举是否通过。
    pub bootstrap_ok: bool,
    /// 冻结后被改。
    pub changed_after_freeze: bool,
    /// 已冻结。
    pub frozen: bool,
}

impl DocEntry {
    /// 构造。
    pub fn new(id: u32, level: DocLevel) -> DocEntry {
        DocEntry {
            id,
            level,
            version: 0,
            bootstrap_ok: false,
            changed_after_freeze: false,
            frozen: false,
        }
    }

    /// 版本合规。
    pub fn version_ok(&self) -> bool {
        self.version > 0
    }

    /// 冻结合规。
    pub fn freeze_ok(&self) -> bool {
        !self.frozen || !self.changed_after_freeze
    }
}

// ─────────────────────────────────────────────────────────────
// 文档清单
// ─────────────────────────────────────────────────────────────

/// 文档清单。
#[derive(Clone, Debug, Default)]
pub struct DocCatalog {
    entries: Vec<DocEntry>,
}

impl DocCatalog {
    /// 空清单。
    pub fn new() -> DocCatalog {
        DocCatalog {
            entries: Vec::new(),
        }
    }

    /// 登记。
    pub fn add(&mut self, e: DocEntry) {
        self.entries.push(e);
    }

    /// 扫描无版本。
    pub fn scan_no_version(&self) -> Vec<u32> {
        let mut bad = Vec::new();
        for e in self.entries.iter() {
            if !e.version_ok() {
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
        for l in DocLevel::all() {
            if !self.entries.iter().any(|e| e.level == l) {
                return false;
            }
        }
        true
    }

    /// 缺级。
    pub fn missing_levels(&self) -> Vec<DocLevel> {
        let mut bad = Vec::new();
        for l in DocLevel::all() {
            if !self.entries.iter().any(|e| e.level == l) {
                bad.push(l);
            }
        }
        bad
    }

    /// 自举全过。
    pub fn bootstrap_all(&self) -> bool {
        self.entries.iter().all(|e| e.bootstrap_ok)
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
// 文档对拍
// ─────────────────────────────────────────────────────────────

/// 对拍对。
#[derive(Clone, Debug)]
pub struct DocPair {
    /// id。
    pub id: u32,
    /// 甲方版本。
    pub ver_a: u32,
    /// 乙方版本。
    pub ver_b: u32,
}

impl DocPair {
    /// 构造。
    pub fn new(id: u32, ver_a: u32, ver_b: u32) -> DocPair {
        DocPair { id, ver_a, ver_b }
    }

    /// 一致。
    pub fn match_ok(&self) -> bool {
        self.ver_a == self.ver_b
    }
}

/// 对拍表。
#[derive(Clone, Debug, Default)]
pub struct DocPairLedger {
    pairs: Vec<DocPair>,
}

impl DocPairLedger {
    /// 空。
    pub fn new() -> DocPairLedger {
        DocPairLedger { pairs: Vec::new() }
    }

    /// 登记。
    pub fn add(&mut self, p: DocPair) {
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

/// F3771 判据集。
pub fn run_vr09k_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3771");

    // 0xA201 版本硬门
    {
        let mut c = DocCatalog::new();
        c.add(DocEntry::new(1, DocLevel::User));
        out.add("0xA201-无版本检出", c.scan_no_version().len() == 1, "");
    }

    // 0xA202 三级
    {
        let mut c = DocCatalog::new();
        out.add("0xA202-全缺检出", c.missing_levels().len() == 3, "");
        for l in DocLevel::all() {
            let mut e = DocEntry::new(l.index() as u32 + 10, l);
            e.version = 1;
            e.bootstrap_ok = true;
            c.add(e);
        }
        out.add("0xA202-三级齐过", c.all_levels(), "");
    }

    // 0xA203 对拍
    {
        let mut l = DocPairLedger::new();
        l.add(DocPair::new(1, 2, 2));
        out.add("0xA203-一致过", l.scan_diverge().is_empty(), "");
        l.add(DocPair::new(2, 1, 2));
        out.add("0xA203-分歧检出", l.scan_diverge().len() == 1, "");
    }

    // 0xA204 冻结
    {
        let mut c = DocCatalog::new();
        let mut e = DocEntry::new(5, DocLevel::User);
        e.version = 1;
        e.frozen = true;
        e.changed_after_freeze = true;
        c.add(e);
        out.add("0xA204-冻结后改检出", c.scan_freeze_violate().len() == 1, "");
    }

    // 0xA206 自举
    {
        let mut c = DocCatalog::new();
        let mut e = DocEntry::new(6, DocLevel::User);
        e.version = 1;
        e.bootstrap_ok = true;
        c.add(e);
        out.add("0xA206-自举过", c.bootstrap_all(), "");
        c.entries[0].bootstrap_ok = false;
        out.add("0xA206-自举失败检出", !c.bootstrap_all(), "");
    }

    out
}
