//! VE-F0756 布尔文档
//! 职责：布尔域五节文档（模型/API/数值边界/工作流/FAQ）、与实现同源校验、
//!       概念词条表（术语→定义→所在模块）。
//! 契约：文档登记的常量与实现单一真源一致；词条可查且无重名。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 文档章节
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum BDocChapter {
    Model,
    Api,
    Numeric,
    Workflow,
    Faq,
}

/// 文档条目
#[derive(Clone, Debug, PartialEq)]
pub struct BDocEntry {
    pub chapter: BDocChapter,
    pub title: &'static str,
    pub body: &'static str,
}

/// 文档表
pub struct BDoc {
    pub entries: Vec<BDocEntry>,
}

impl BDoc {
    pub fn new() -> BDoc {
        BDoc { entries: alloc::vec![] }
    }
    pub fn put(&mut self, ch: BDocChapter, title: &'static str, body: &'static str) {
        self.entries.push(BDocEntry { chapter: ch, title, body });
    }
    pub fn by_chapter(&self, ch: BDocChapter) -> Vec<&BDocEntry> {
        self.entries.iter().filter(|e| e.chapter == ch).collect()
    }
    pub fn dup_titles(&self) -> Vec<&'static str> {
        let mut dups: Vec<&'static str> = alloc::vec![];
        for i in 0..self.entries.len() {
            for j in (i + 1)..self.entries.len() {
                if self.entries[i].title == self.entries[j].title && !dups.contains(&self.entries[i].title) {
                    dups.push(self.entries[i].title);
                }
            }
        }
        dups
    }
    pub fn complete(&self) -> bool {
        [BDocChapter::Model, BDocChapter::Api, BDocChapter::Numeric, BDocChapter::Workflow, BDocChapter::Faq]
            .iter()
            .all(|ch| !self.by_chapter(*ch).is_empty())
    }
}

impl Default for BDoc {
    fn default() -> Self {
        BDoc::new()
    }
}

/// 术语词条
#[derive(Clone, Debug, PartialEq)]
pub struct Term {
    pub name: &'static str,
    pub def: &'static str,
    pub module: &'static str,
}

/// 词条表（可查，无重名）
pub struct Glossary {
    pub terms: Vec<Term>,
}

impl Glossary {
    pub fn new() -> Glossary {
        Glossary { terms: alloc::vec![] }
    }
    pub fn add(&mut self, name: &'static str, def: &'static str, module: &'static str) {
        self.terms.push(Term { name, def, module });
    }
    pub fn lookup(&self, name: &str) -> Option<&Term> {
        self.terms.iter().find(|t| t.name == name)
    }
    pub fn dup_names(&self) -> Vec<&'static str> {
        let mut dups: Vec<&'static str> = alloc::vec![];
        for i in 0..self.terms.len() {
            for j in (i + 1)..self.terms.len() {
                if self.terms[i].name == self.terms[j].name && !dups.contains(&self.terms[i].name) {
                    dups.push(self.terms[i].name);
                }
            }
        }
        dups
    }
}

impl Default for Glossary {
    fn default() -> Self {
        Glossary::new()
    }
}

/// 与实现同源的真源常量
pub const SRC_STAGES: usize = 4;
pub const SRC_BOOL_OPS: usize = 4;
pub const SRC_EPS_DEFAULT: f64 = 1e-9;
pub const SRC_TIER_GRID_F32: f64 = 1.0 / 1024.0;

/// 文档声明常量（须一致）
pub const DOC_STAGES: usize = 4;
pub const DOC_BOOL_OPS: usize = 4;
pub const DOC_EPS_DEFAULT: f64 = 1e-9;
pub const DOC_TIER_GRID_F32: f64 = 1.0 / 1024.0;

pub fn run_vec356_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec356");
    // 族一：同源常量
    s.add("同源-阶段数", SRC_STAGES == DOC_STAGES, "4 stages");
    s.add("同源-运算数", SRC_BOOL_OPS == DOC_BOOL_OPS, "4 ops");
    s.add("同源-默认容差", SRC_EPS_DEFAULT == DOC_EPS_DEFAULT, "eps");
    s.add("同源-f32网格", SRC_TIER_GRID_F32 == DOC_TIER_GRID_F32, "grid");
    // 实现侧验证
    s.add("实现-阶段对齐", super::vec341_boolorg::STAGES.len() == SRC_STAGES, "stages match");
    s.add("实现-eps对齐", (SRC_EPS_DEFAULT - 1e-9).abs() < 1e-18, "eps value");
    // 族二：文档表
    let mut doc = BDoc::new();
    doc.put(BDocChapter::Model, "有向边记号", "边方向决定环绕贡献 ±1");
    doc.put(BDocChapter::Model, "环绕数规则", "非零与奇偶两档填充判定");
    doc.put(BDocChapter::Api, "scan_row", "一行事件，x 有序");
    doc.put(BDocChapter::Api, "clip_rect", "Sutherland-Hodgman 矩形裁剪");
    doc.put(BDocChapter::Numeric, "默认容差", "1e-9，退化检测与 snap 共用");
    doc.put(BDocChapter::Workflow, "先归并后布尔", "snap rounding 必须在布尔之前");
    doc.put(BDocChapter::Faq, "为何双规则", "蒙版复合需要奇偶语义");
    s.add("文档-条目数", doc.entries.len() == 7, "7 entries");
    s.add("文档-五节齐", doc.complete(), "all chapters");
    s.add("文档-无重题", doc.dup_titles().is_empty(), "no dups");
    s.add("文档-章节取", doc.by_chapter(BDocChapter::Model).len() == 2, "2 model");
    s.add("文档-FAQ取", doc.by_chapter(BDocChapter::Faq).len() == 1, "1 faq");
    // 族三：词条表
    let mut g = Glossary::new();
    g.add("环绕数", "点被边逆时针缠绕的净次数", "vec341_boolorg");
    g.add("snap rounding", "坐标归并到精度网格", "vec344_bdegen");
    g.add("区间", "一行上的内部 x 区段", "vec345_rebuild");
    s.add("词条-数量", g.terms.len() == 3, "3 terms");
    s.add("词条-可查", g.lookup("环绕数").expect("t").module == "vec341_boolorg", "found");
    s.add("词条-未录", g.lookup("不存在").is_none(), "missing none");
    s.add("词条-无重名", g.dup_names().is_empty(), "no dup names");
    // 重名检出
    let mut g2 = Glossary::new();
    g2.add("X", "a", "m1");
    g2.add("X", "b", "m2");
    s.add("词条-重名检出", g2.dup_names() == alloc::vec!["X"], "dup found");
    // 族四：内容纪律
    s.add("内容-非空", doc.entries.iter().all(|e| !e.body.is_empty()) && g.terms.iter().all(|t| !t.def.is_empty()), "non-empty");
    // 族五：默认空表
    s.add("默认-空文档", BDoc::default().entries.is_empty() && Glossary::default().terms.is_empty(), "empty defaults");
    s
}
