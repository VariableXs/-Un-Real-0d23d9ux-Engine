//! VE-F3752 · 创作无障碍文档
//!
//! 锚点：创作无障碍文档（三册复刻（用户册（无障碍创作者指南/偏好设置——可发现
//! 复述；开发者册（创作工具无障碍开发规范（义务前置复述；审计册（审查工具使用
//! （复述；同源纪律复述；自举复述（文档自身无障碍）。
//!
//! 数据结构：三册复刻；义务前置复用；自举复述；同源清单。
//! 错误路径与降级矩阵：漂移→对账（复述）；义务章缺→审查（复述红线）；自举不过→
//! 修（复述）；迷宫→2 跳（复用红线）。
//! 跨批对接点：F3153/F2879 模式复用；F3750 工具对端；CI 钩子。
//!
//! 诊断码段：独占 0x8Fxx。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ─────────────────────────────────────────────────────────────
// 诊断码（0x8Fxx）
// ─────────────────────────────────────────────────────────────

/// F3752 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct D7Code(pub u16);

impl D7Code {
    /// 三册缺——复刻断言。
    pub const BOOK_MISSING: D7Code = D7Code(0x8F01);
    /// 义务章缺——审查红线。
    pub const OBLIGATION_MISS: D7Code = D7Code(0x8F02);
    /// 自举不过——文档自身无障碍。
    pub const BOOTSTRAP_FAIL: D7Code = D7Code(0x8F03);
    /// 导航超 2 跳——迷宫红线。
    pub const NAV_DEEP: D7Code = D7Code(0x8F04);
    /// 同源漂移——对账。
    pub const SOURCE_DRIFT: D7Code = D7Code(0x8F05);
    /// 可发现性不足——用户册。
    pub const DISCOVER_MISS: D7Code = D7Code(0x8F06);

    /// 诊断说明。
    pub fn explain(self) -> &'static str {
        match self.0 {
            0x8F01 => "三册缺册，复刻断言不通过。",
            0x8F02 => "开发者册缺义务章，审查红线不通过。",
            0x8F03 => "文档自举无障碍审查不过，需修正。",
            0x8F04 => "导航深度超 2 跳，迷宫红线不通过。",
            0x8F05 => "文档与源声明漂移，需对账。",
            0x8F06 => "用户册可发现性不足，复述不完整。",
            _ => "未知诊断码。",
        }
    }

    /// 数值。
    pub fn raw(self) -> u16 {
        self.0
    }
}

// ─────────────────────────────────────────────────────────────
// 三册结构
// ─────────────────────────────────────────────────────────────

/// 册类型。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BookKind {
    /// 用户册。
    User,
    /// 开发者册。
    Dev,
    /// 审计册。
    Audit,
}

impl BookKind {
    /// 稳定序号。
    pub fn index(self) -> u8 {
        match self {
            BookKind::User => 0,
            BookKind::Dev => 1,
            BookKind::Audit => 2,
        }
    }

    /// 全集。
    pub fn all() -> [BookKind; 3] {
        [BookKind::User, BookKind::Dev, BookKind::Audit]
    }
}

/// 单册。
#[derive(Clone, Debug)]
pub struct DocBook {
    /// 册类型。
    pub kind: BookKind,
    /// 是否存在。
    pub present: bool,
    /// 是否有义务章（开发者册必需）。
    pub has_obligation: bool,
    /// 导航深度（跳数，≤2 合规）。
    pub nav_depth: u8,
    /// 是否可发现（用户册）。
    pub discoverable: bool,
    /// 自举无障碍审查通过。
    pub bootstrap_ok: bool,
}

impl DocBook {
    /// 构造（缺省态）。
    pub fn new(kind: BookKind) -> DocBook {
        DocBook {
            kind,
            present: false,
            has_obligation: false,
            nav_depth: 99,
            discoverable: false,
            bootstrap_ok: false,
        }
    }

    /// 义务前置合规：开发者册必须有义务章。
    pub fn obligation_ok(&self) -> bool {
        if self.kind != BookKind::Dev {
            return true;
        }
        self.has_obligation
    }

    /// 导航合规。
    pub fn nav_ok(&self) -> bool {
        self.nav_depth <= 2
    }

    /// 自举合规。
    pub fn bootstrap(&self) -> bool {
        self.bootstrap_ok
    }

    /// 可发现合规（仅用户册强制）。
    pub fn discover_ok(&self) -> bool {
        if self.kind != BookKind::User {
            return true;
        }
        self.discoverable
    }
}

/// 三册套。
#[derive(Clone, Debug)]
pub struct DocSet {
    books: [DocBook; 3],
    /// 同源标识。
    source_tag: u64,
}

impl DocSet {
    /// 空套。
    pub fn new(source_tag: u64) -> DocSet {
        DocSet {
            books: [
                DocBook::new(BookKind::User),
                DocBook::new(BookKind::Dev),
                DocBook::new(BookKind::Audit),
            ],
            source_tag,
        }
    }

    /// 取册（可变）。
    pub fn book_mut(&mut self, kind: BookKind) -> &mut DocBook {
        &mut self.books[kind.index() as usize]
    }

    /// 取册（只读）。
    pub fn book(&self, kind: BookKind) -> &DocBook {
        &self.books[kind.index() as usize]
    }

    /// 三册齐备。
    pub fn complete(&self) -> bool {
        self.books.iter().all(|b| b.present)
    }

    /// 缺册清单。
    pub fn missing(&self) -> Vec<BookKind> {
        let mut bad = Vec::new();
        for k in BookKind::all() {
            if !self.books[k.index() as usize].present {
                bad.push(k);
            }
        }
        bad
    }

    /// 全套自举。
    pub fn bootstrap_all(&self) -> bool {
        self.books.iter().all(|b| b.present && b.bootstrap())
    }

    /// 全套导航合规。
    pub fn nav_all_ok(&self) -> bool {
        self.books.iter().filter(|b| b.present).all(|b| b.nav_ok())
    }

    /// 义务合规。
    pub fn obligation_all_ok(&self) -> bool {
        self.books.iter().all(|b| b.obligation_ok())
    }

    /// 可发现合规。
    pub fn discover_all_ok(&self) -> bool {
        self.books.iter().all(|b| b.discover_ok())
    }

    /// 同源标识。
    pub fn source_tag(&self) -> u64 {
        self.source_tag
    }

    /// 与另一套对账（漂移 id）。
    pub fn reconcile(&self, other: &DocSet) -> bool {
        self.source_tag == other.source_tag
    }
}

// ─────────────────────────────────────────────────────────────
// 文档审查器
// ─────────────────────────────────────────────────────────────

/// 文档审查器。
#[derive(Clone, Debug)]
pub struct DocAuditor {
    set: DocSet,
}

impl DocAuditor {
    /// 构造。
    pub fn new(source_tag: u64) -> DocAuditor {
        DocAuditor {
            set: DocSet::new(source_tag),
        }
    }

    /// 取套（可变）。
    pub fn set_mut(&mut self) -> &mut DocSet {
        &mut self.set
    }

    /// 取套。
    pub fn set(&self) -> &DocSet {
        &self.set
    }

    /// 全量审查。
    pub fn audit(&self) -> Vec<D7Code> {
        let mut bad = Vec::new();
        if !self.set.complete() {
            bad.push(D7Code::BOOK_MISSING);
        }
        if !self.set.obligation_all_ok() {
            bad.push(D7Code::OBLIGATION_MISS);
        }
        if !self.set.bootstrap_all() {
            bad.push(D7Code::BOOTSTRAP_FAIL);
        }
        if !self.set.nav_all_ok() {
            bad.push(D7Code::NAV_DEEP);
        }
        if !self.set.discover_all_ok() {
            bad.push(D7Code::DISCOVER_MISS);
        }
        bad
    }
}

// ─────────────────────────────────────────────────────────────
// 判据
// ─────────────────────────────────────────────────────────────

/// F3752 判据集。
pub fn run_vr08l_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3752");

    // 0x8F01 三册齐备
    {
        let mut a = DocAuditor::new(100);
        out.add("0x8F01-全缺检出", a.set().missing().len() == 3, "");
        for k in BookKind::all() {
            let b = a.set_mut().book_mut(k);
            b.present = true;
            b.bootstrap_ok = true;
            b.nav_depth = 1;
        }
        a.set_mut().book_mut(BookKind::Dev).has_obligation = true;
        a.set_mut().book_mut(BookKind::User).discoverable = true;
        out.add("0x8F01-补齐后过", a.set().complete(), "");
    }

    // 0x8F02 义务前置
    {
        let mut a = DocAuditor::new(100);
        a.set_mut().book_mut(BookKind::Dev).present = true;
        out.add("0x8F02-缺义务章拦", !a.set().obligation_all_ok(), "");
        a.set_mut().book_mut(BookKind::Dev).has_obligation = true;
        out.add("0x8F02-补义务后过", a.set().obligation_all_ok(), "");
    }

    // 0x8F03 自举
    {
        let mut a = DocAuditor::new(100);
        a.set_mut().book_mut(BookKind::User).present = true;
        out.add("0x8F03-自举不过", !a.set().bootstrap_all(), "");
        a.set_mut().book_mut(BookKind::User).bootstrap_ok = true;
        out.add("0x8F03-自举过", a.set().book(BookKind::User).bootstrap(), "");
    }

    // 0x8F04 导航 2 跳
    {
        let mut a = DocAuditor::new(100);
        {
            let b = a.set_mut().book_mut(BookKind::Audit);
            b.present = true;
            b.bootstrap_ok = true;
            b.nav_depth = 3;
        }
        out.add("0x8F04-超2跳拦", !a.set().nav_all_ok(), "");
        a.set_mut().book_mut(BookKind::Audit).nav_depth = 2;
        out.add("0x8F04-2跳恰合规", a.set().nav_all_ok(), "");
    }

    // 0x8F05 同源
    {
        let a = DocAuditor::new(100);
        let b = DocAuditor::new(100);
        out.add("0x8F05-同源过", a.set().reconcile(b.set()), "");
        let c = DocAuditor::new(200);
        out.add("0x8F05-异源检出", !a.set().reconcile(c.set()), "");
    }

    out
}
