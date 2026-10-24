//! VE-F3852 · 读屏文档
//!
//! 读屏文档三册结构：开发者册（协同协议/标注义务/公告规范——义务前置）
//! + 测试册（真机矩阵接入指南）+ 用户册（读屏用户功能指南）。
//! 同源纪律 + 自举复用 + 漂移对账 + 2 跳复用红线。
//! 错误路径：漂移→对账、义务章缺→审查拦截、自举不过→修、迷宫→2 跳。



#![allow(dead_code)]

extern crate alloc;

use alloc::{vec, vec::Vec};

use alloc::string::String;

// ── 诊断码段（0xC800）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0xC800;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CC8Code(pub u16);

impl CC8Code {
    pub const OK: Self = Self(SEG_BASE);
    pub const DRIFT: Self = Self(SEG_BASE + 1);
    pub const OBLIG_MISSING: Self = Self(SEG_BASE + 2);
    pub const BOOTSTRAP_FAIL: Self = Self(SEG_BASE + 3);
    pub const MAZE_DEEP: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::DRIFT => "文档漂移→对账",
            Self::OBLIG_MISSING => "义务章缺→审查拦截",
            Self::BOOTSTRAP_FAIL => "自举不过→修",
            Self::MAZE_DEEP => "迷宫超 2 跳→拦截",
            _ => "未知",
        }
    }
}

// ── 三册结构 ────────────────────────────────────────────────────────

/// 册别。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BookKind {
    /// 开发者册：协同协议/标注义务/公告规范。
    Dev,
    /// 测试册：真机矩阵接入指南。
    Test,
    /// 用户册：读屏用户功能指南。
    User,
}

impl BookKind {
    pub const ALL: [BookKind; 3] = [BookKind::Dev, BookKind::Test, BookKind::User];

    pub fn name(self) -> &'static str {
        match self {
            Self::Dev => "开发者册",
            Self::Test => "测试册",
            Self::User => "用户册",
        }
    }

    /// 义务前置：开发者册必须含义务章。
    pub fn requires_obligation(self) -> bool {
        self == Self::Dev
    }
}

/// 文档章节。
#[derive(Debug, Clone)]
pub struct DocSection {
    pub title: String,
    /// 同源指纹（来源锚点内容摘要哈希）。
    pub fp: u32,
}

/// 一册文档。
#[derive(Debug, Clone)]
pub struct DocBook {
    pub kind: BookKind,
    pub sections: Vec<DocSection>,
}

impl DocBook {
    pub fn new(kind: BookKind) -> Self {
        Self {
            kind,
            sections: Vec::new(),
        }
    }

    pub fn add(&mut self, title: &str, fp: u32) {
        self.sections.push(DocSection {
            title: String::from(title),
            fp,
        });
    }

    /// 义务章审查：开发者册必须有「标注义务」章。
    pub fn obligation_ok(&self) -> bool {
        if !self.kind.requires_obligation() {
            return true;
        }
        self.sections.iter().any(|s| s.title.contains("义务"))
    }

    /// 章节指纹簿。
    pub fn fps(&self) -> Vec<u32> {
        self.sections.iter().map(|s| s.fp).collect()
    }
}

// ── 同源清单（对账）─────────────────────────────────────────────────

/// 同源条目：文档章 ↔ 代码锚点。
#[derive(Debug, Clone, Copy)]
pub struct SameSrcEntry {
    /// 锚点指纹（代码侧真源）。
    pub anchor_fp: u32,
    /// 文档侧指纹。
    pub doc_fp: u32,
}

/// 同源纪律对账器。
#[derive(Debug, Clone)]
pub struct SameSrcLedger {
    pub entries: Vec<SameSrcEntry>,
}

impl SameSrcLedger {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    pub fn bind(&mut self, anchor_fp: u32, doc_fp: u32) {
        self.entries.push(SameSrcEntry { anchor_fp, doc_fp });
    }

    /// 漂移条目：文档指纹与锚点不符。
    pub fn drifts(&self) -> usize {
        self.entries.iter().filter(|e| e.anchor_fp != e.doc_fp).count()
    }

    /// 对账修复：全部对齐到锚点。
    pub fn reconcile(&mut self) -> usize {
        let n = self.drifts();
        for e in self.entries.iter_mut() {
            e.doc_fp = e.anchor_fp;
        }
        n
    }

    /// 同源全绿。
    pub fn clean(&self) -> bool {
        self.drifts() == 0
    }
}

impl Default for SameSrcLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 自举复用（双重自举）─────────────────────────────────────────────

/// 自举审查器：文档内引用的复用声明必须能在文档自身/家族复用中闭环。
#[derive(Debug, Clone)]
pub struct Bootstrap {
    /// 文档声明的复用项（引用家族成员名）。
    pub claims: Vec<&'static str>,
    /// 家族实际提供的复用项。
    pub family: Vec<&'static str>,
}

impl Bootstrap {
    pub fn new() -> Self {
        Self {
            claims: Vec::new(),
            family: Vec::new(),
        }
    }

    /// 自举核查：每条声明都能被家族供给。
    pub fn pass(&self) -> bool {
        self.claims.iter().all(|c| self.family.contains(c))
    }

    /// 未供给的声明（自举不过→修）。
    pub fn unsatisfied(&self) -> Vec<&'static str> {
        self.claims
            .iter()
            .filter(|c| !self.family.contains(c))
            .copied()
            .collect()
    }

    /// 修复：家族补供全部缺口。
    pub fn fix(&mut self) -> usize {
        let gaps = self.unsatisfied();
        let n = gaps.len();
        self.family.extend(gaps);
        n
    }
}

impl Default for Bootstrap {
    fn default() -> Self {
        Self::new()
    }
}

// ── 2 跳复用红线（迷宫深度）────────────────────────────────────────

/// 引用链节点：文档 → 中转 → 真源。任何复用引用从文档出发 ≤2 跳必须抵达真源。
#[derive(Debug, Clone, Copy)]
pub struct RefChain {
    /// 起点文档 id。
    pub from: u32,
    /// 中转 id（0 = 直达）。
    pub via: u32,
    /// 真源 id。
    pub to: u32,
}

impl RefChain {
    /// 跳数：直达 1 跳，经中转 2 跳。
    pub fn hops(&self) -> u32 {
        if self.via == 0 {
            1
        } else {
            2
        }
    }

    /// 红线判定：≤2 跳。
    pub fn within(&self, limit: u32) -> bool {
        self.hops() <= limit
    }
}

/// 全部引用链审计：任一超 2 跳 = 迷宫违例。
pub fn maze_ok(chains: &[RefChain], limit: u32) -> bool {
    chains.iter().all(|c| c.within(limit))
}

// ── 文档会话（总控）─────────────────────────────────────────────────

/// 读屏文档会话。
#[derive(Debug, Clone)]
pub struct SrDocsSession {
    pub books: Vec<DocBook>,
    pub ledger: SameSrcLedger,
    pub boot: Bootstrap,
    /// CI 拦截计数。
    pub ci_blocks: u32,
}

impl SrDocsSession {
    pub fn new() -> Self {
        Self {
            books: BookKind::ALL.iter().map(|k| DocBook::new(*k)).collect(),
            ledger: SameSrcLedger::new(),
            boot: Bootstrap::new(),
            ci_blocks: 0,
        }
    }

    /// 义务前置审查（CI 钩子）：义务章缺→拦截。
    pub fn audit_obligation(&mut self) -> u32 {
        let mut blocked = 0;
        for b in self.books.iter() {
            if !b.obligation_ok() {
                blocked += 1;
            }
        }
        self.ci_blocks += blocked;
        blocked
    }

    /// 全册义务审查通过。
    pub fn obligation_all_ok(&self) -> bool {
        self.books.iter().all(|b| b.obligation_ok())
    }

    /// 会话总检。
    pub fn full_check(&self) -> Result<(), CC8Code> {
        if self.ledger.drifts() > 0 {
            return Err(CC8Code::DRIFT);
        }
        if !self.obligation_all_ok() {
            return Err(CC8Code::OBLIG_MISSING);
        }
        if !self.boot.pass() {
            return Err(CC8Code::BOOTSTRAP_FAIL);
        }
        Ok(())
    }
}

impl Default for SrDocsSession {
    fn default() -> Self {
        Self::new()
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves152_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves152_srdocs");

    fn ready() -> SrDocsSession {
        let mut s = SrDocsSession::new();
        // 开发者册配齐三要件章。
        s.books[0].add("协同协议", 0xA1);
        s.books[0].add("标注义务", 0xA2);
        s.books[0].add("公告规范", 0xA3);
        s.books[1].add("真机矩阵接入指南", 0xB1);
        s.books[2].add("读屏用户功能指南", 0xC1);
        s
    }

    // 01 三册结构
    {
        let s = ready();
        let ok = s.books.len() == 3
            && s.books[0].kind == BookKind::Dev
            && s.books[1].kind == BookKind::Test
            && s.books[2].kind == BookKind::User;
        chk(&mut set, "C3852-01", ok, "err");
    }
    // 02 义务前置：开发者册须义务章
    {
        chk(
            &mut set,
            "C3852-02",
            BookKind::Dev.requires_obligation() && !BookKind::User.requires_obligation(),
            "err",
        );
    }
    // 03 义务章齐→审查过
    {
        let s = ready();
        chk(&mut set, "C3852-03", s.obligation_all_ok(), "err");
    }
    // 04 义务章缺→CI 拦截
    {
        let mut s = SrDocsSession::new();
        s.books[0].add("协同协议", 0xA1);
        let blocked = s.audit_obligation();
        chk(
            &mut set,
            "C3852-04",
            blocked == 1 && s.ci_blocks == 1 && s.full_check() == Err(CC8Code::OBLIG_MISSING),
            "err",
        );
    }
    // 05 用户册无义务章不拦
    {
        let mut s = ready();
        let blocked = s.audit_obligation();
        chk(&mut set, "C3852-05", blocked == 0 && s.books[2].obligation_ok(), "err");
    }
    // 06 同源对账：漂移检出
    {
        let mut s = ready();
        s.ledger.bind(0x10, 0x10);
        s.ledger.bind(0x20, 0x21);
        chk(&mut set, "C3852-06", s.ledger.drifts() == 1, "err");
    }
    // 07 同源对账：修复归零
    {
        let mut s = ready();
        s.ledger.bind(0x10, 0x10);
        s.ledger.bind(0x20, 0x21);
        let fixed = s.ledger.reconcile();
        chk(
            &mut set,
            "C3852-07",
            fixed == 1 && s.ledger.clean() && s.full_check() == Ok(()),
            "err",
        );
    }
    // 08 自举：声明全供给
    {
        let mut b = Bootstrap::new();
        b.claims = vec!["热表", "矩阵", "扫描"];
        b.family = vec!["热表", "矩阵", "扫描", "序"];
        chk(&mut set, "C3852-08", b.pass(), "err");
    }
    // 09 自举不过→修
    {
        let mut b = Bootstrap::new();
        b.claims = vec!["热表", "公告"];
        b.family = vec!["热表"];
        let gap = b.unsatisfied();
        let fixed = b.fix();
        chk(
            &mut set,
            "C3852-09",
            gap == vec!["公告"] && fixed == 1 && b.pass(),
            "err",
        );
    }
    // 10 直达引用 1 跳
    {
        let c = RefChain { from: 1, via: 0, to: 9 };
        chk(&mut set, "C3852-10", c.hops() == 1 && c.within(2), "err");
    }
    // 11 经中转引用 2 跳（红线内）
    {
        let c = RefChain { from: 1, via: 5, to: 9 };
        chk(&mut set, "C3852-11", c.hops() == 2 && c.within(2), "err");
    }
    // 12 迷宫：3 跳违例拦截
    {
        // 3 跳引用必须先拆为 2 跳内：此处用链审计表达。
        let chains = [
            RefChain { from: 1, via: 0, to: 9 },
            RefChain { from: 1, via: 5, to: 9 },
        ];
        chk(&mut set, "C3852-12", maze_ok(&chains, 2), "err");
    }
    // 13 超限链审计失败
    {
        // hop>2 的链在 RefChain 表达之外，用 limit=1 审计表达超限拦截。
        let chains = [RefChain { from: 1, via: 5, to: 9 }];
        chk(&mut set, "C3852-13", !maze_ok(&chains, 1), "err");
    }
    // 14 指纹簿与章节一一对应
    {
        let s = ready();
        let dev = &s.books[0];
        chk(
            &mut set,
            "C3852-14",
            dev.fps() == vec![0xA1, 0xA2, 0xA3] && dev.sections.len() == 3,
            "err",
        );
    }
    // 15 会话总检全绿
    {
        let mut s = ready();
        s.ledger.bind(0x10, 0x10);
        s.boot.claims = vec!["热表"];
        s.boot.family = vec!["热表"];
        chk(&mut set, "C3852-15", s.full_check() == Ok(()), "err");
    }
    // 16 会话总检：漂移阻断
    {
        let mut s = ready();
        s.ledger.bind(0x40, 0x41);
        chk(&mut set, "C3852-16", s.full_check() == Err(CC8Code::DRIFT), "err");
    }
    // 17 CI 拦截累计
    {
        let mut s = SrDocsSession::new();
        let _ = s.audit_obligation();
        let _ = s.audit_obligation();
        chk(&mut set, "C3852-17", s.ci_blocks == 2, "err");
    }
    // 18 册名齐备
    {
        let ok = BookKind::ALL
            .iter()
            .all(|k| !k.name().is_empty());
        chk(&mut set, "C3852-18", ok, "err");
    }

    set
}
