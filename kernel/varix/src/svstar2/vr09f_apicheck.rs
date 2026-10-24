//! VE-F3766 · R 域 API 面终版清点
//!
//! 锚点：R 域 API 面终版清点——API 清点（八组 API 面清单（复述清单红线：
//! 公开 API 必须有文档注释（文档硬门）；API 分类（稳定/实验/内部三级（复述
//! 分级）；API 对拍（八组 API 面双方声明一致（复述对拍）；API 冻结（终版
//! 冻结清单（复述冻结）。
//!
//! 数据结构：API 清单；三级分类；对拍记录；冻结清单。
//! 错误路径与降级矩阵：无文档→拒（硬门实测）；分类缺→补（复述）；对拍分歧→
//! 立案（复述红线）；冻结后改→回滚（复述）。
//! 跨批对接点：八组档案；F3765 对账输入；F3767 冻结对端。
//!
//! 诊断码段：独占 0x9Dxx。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ─────────────────────────────────────────────────────────────
// 诊断码（0x9Dxx）
// ─────────────────────────────────────────────────────────────

/// F3766 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct A9Code(pub u16);

impl A9Code {
    /// API 无文档——硬门。
    pub const NO_DOC: A9Code = A9Code(0x9D01);
    /// 分类缺失——三级。
    pub const CLASS_MISS: A9Code = A9Code(0x9D02);
    /// 对拍分歧——立案。
    pub const PAIR_DIVERGE: A9Code = A9Code(0x9D03);
    /// 冻结后改——回滚。
    pub const FREEZE_VIOLATE: A9Code = A9Code(0x9D04);
    /// 清单不全——补。
    pub const LIST_INCOMPLETE: A9Code = A9Code(0x9D05);
    /// 稳定 API 破坏——变更红线。
    pub const STABLE_BREAK: A9Code = A9Code(0x9D06);

    /// 诊断说明。
    pub fn explain(self) -> &'static str {
        match self.0 {
            0x9D01 => "公开 API 无文档注释，硬门拒收。",
            0x9D02 => "API 分级缺失（稳定/实验/内部）。",
            0x9D03 => "API 对拍分歧，需立案。",
            0x9D04 => "冻结后修改，需回滚。",
            0x9D05 => "API 清单不全。",
            0x9D06 => "稳定 API 被破坏，变更红线不通过。",
            _ => "未知诊断码。",
        }
    }

    /// 数值。
    pub fn raw(self) -> u16 {
        self.0
    }
}

// ─────────────────────────────────────────────────────────────
// API 三级分类
// ─────────────────────────────────────────────────────────────

/// API 级别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ApiLevel {
    /// 稳定。
    Stable,
    /// 实验。
    Experimental,
    /// 内部。
    Internal,
}

impl ApiLevel {
    /// 稳定序号。
    pub fn index(self) -> u8 {
        match self {
            ApiLevel::Stable => 0,
            ApiLevel::Experimental => 1,
            ApiLevel::Internal => 2,
        }
    }

    /// 全集。
    pub fn all() -> [ApiLevel; 3] {
        [ApiLevel::Stable, ApiLevel::Experimental, ApiLevel::Internal]
    }
}

/// API 条目。
#[derive(Clone, Debug)]
pub struct ApiEntry {
    /// API id。
    pub id: u32,
    /// 级别。
    pub level: ApiLevel,
    /// 是否有文档注释。
    pub documented: bool,
    /// 是否已冻结。
    pub frozen: bool,
    /// 冻结后是否被改。
    pub changed_after_freeze: bool,
}

impl ApiEntry {
    /// 构造。
    pub fn new(id: u32, level: ApiLevel) -> ApiEntry {
        ApiEntry {
            id,
            level,
            documented: false,
            frozen: false,
            changed_after_freeze: false,
        }
    }

    /// 文档合规。
    pub fn doc_ok(&self) -> bool {
        self.documented
    }

    /// 冻结合规。
    pub fn freeze_ok(&self) -> bool {
        !self.frozen || !self.changed_after_freeze
    }

    /// 全合规。
    pub fn complete(&self) -> bool {
        self.doc_ok() && self.freeze_ok()
    }
}

// ─────────────────────────────────────────────────────────────
// API 清单
// ─────────────────────────────────────────────────────────────

/// API 清单。
#[derive(Clone, Debug, Default)]
pub struct ApiCatalog {
    entries: Vec<ApiEntry>,
}

impl ApiCatalog {
    /// 空清单。
    pub fn new() -> ApiCatalog {
        ApiCatalog {
            entries: Vec::new(),
        }
    }

    /// 登记。
    pub fn add(&mut self, e: ApiEntry) {
        self.entries.push(e);
    }

    /// 扫描无文档。
    pub fn scan_no_doc(&self) -> Vec<u32> {
        let mut bad = Vec::new();
        for e in self.entries.iter() {
            if !e.doc_ok() {
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
    pub fn all_levels_present(&self) -> bool {
        for l in ApiLevel::all() {
            if !self.entries.iter().any(|e| e.level == l) {
                return false;
            }
        }
        true
    }

    /// 缺级清单。
    pub fn missing_levels(&self) -> Vec<ApiLevel> {
        let mut bad = Vec::new();
        for l in ApiLevel::all() {
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
// API 对拍（八组双方声明一致）
// ─────────────────────────────────────────────────────────────

/// 对拍对。
#[derive(Clone, Debug)]
pub struct ApiPair {
    /// API id。
    pub id: u32,
    /// 甲方声明。
    pub decl_a: u32,
    /// 乙方声明。
    pub decl_b: u32,
}

impl ApiPair {
    /// 构造。
    pub fn new(id: u32, decl_a: u32, decl_b: u32) -> ApiPair {
        ApiPair {
            id,
            decl_a,
            decl_b,
        }
    }

    /// 一致。
    pub fn match_ok(&self) -> bool {
        self.decl_a != 0 && self.decl_a == self.decl_b
    }
}

/// 对拍表。
#[derive(Clone, Debug, Default)]
pub struct ApiPairLedger {
    pairs: Vec<ApiPair>,
}

impl ApiPairLedger {
    /// 空。
    pub fn new() -> ApiPairLedger {
        ApiPairLedger { pairs: Vec::new() }
    }

    /// 登记。
    pub fn add(&mut self, p: ApiPair) {
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
// 冻结清单
// ─────────────────────────────────────────────────────────────

/// 冻结清单。
#[derive(Clone, Debug, Default)]
pub struct FreezeList {
    /// 已冻结 API id 集。
    frozen: Vec<u32>,
}

impl FreezeList {
    /// 空。
    pub fn new() -> FreezeList {
        FreezeList { frozen: Vec::new() }
    }

    /// 冻结。
    pub fn freeze(&mut self, id: u32) {
        if !self.frozen.contains(&id) {
            self.frozen.push(id);
        }
    }

    /// 是否已冻结。
    pub fn is_frozen(&self, id: u32) -> bool {
        self.frozen.contains(&id)
    }

    /// 冻结数。
    pub fn count(&self) -> usize {
        self.frozen.len()
    }
}

// ─────────────────────────────────────────────────────────────
// 判据
// ─────────────────────────────────────────────────────────────

/// F3766 判据集。
pub fn run_vr09f_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3766");

    // 0x9D01 文档硬门
    {
        let mut c = ApiCatalog::new();
        let mut e = ApiEntry::new(1, ApiLevel::Stable);
        c.add(e);
        out.add("0x9D01-无文档检出", c.scan_no_doc().len() == 1, "");
        e = ApiEntry::new(2, ApiLevel::Stable);
        e.documented = true;
        c.add(e);
        out.add("0x9D01-有文档过", c.scan_no_doc().len() == 1, "");
    }

    // 0x9D02 三级分类
    {
        let mut c = ApiCatalog::new();
        out.add("0x9D02-全缺检出", c.missing_levels().len() == 3, "");
        for l in ApiLevel::all() {
            let mut e = ApiEntry::new(l.index() as u32 + 10, l);
            e.documented = true;
            c.add(e);
        }
        out.add("0x9D02-三级齐过", c.all_levels_present(), "");
    }

    // 0x9D03 对拍
    {
        let mut l = ApiPairLedger::new();
        l.add(ApiPair::new(1, 100, 100));
        out.add("0x9D03-一致过", l.scan_diverge().is_empty(), "");
        l.add(ApiPair::new(2, 200, 201));
        out.add("0x9D03-分歧检出", l.scan_diverge().len() == 1, "");
    }

    // 0x9D04 冻结
    {
        let mut c = ApiCatalog::new();
        let mut e = ApiEntry::new(5, ApiLevel::Stable);
        e.documented = true;
        e.frozen = true;
        e.changed_after_freeze = true;
        c.add(e);
        out.add("0x9D04-冻结后改检出", c.scan_freeze_violate().len() == 1, "");
    }

    // 0x9D05 冻结清单
    {
        let mut f = FreezeList::new();
        f.freeze(1);
        f.freeze(1);
        out.add("0x9D05-去重冻结", f.count() == 1, "");
    }

    out
}
