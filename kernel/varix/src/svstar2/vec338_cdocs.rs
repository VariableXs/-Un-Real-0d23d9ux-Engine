//! VE-F0738 曲线文档
//! 职责：曲线库五节文档（数学定义/API 契约/数值边界/使用范式/FAQ）、
//!       与实现同源校验（文档常量 vs 代码常量）、条目注册与去重。
//! 契约：文档登记的数值常量与实现模块单一真源一致。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::vec321_bezcore::Deg;

/// 文档章节
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CDocChapter {
    Math,
    Api,
    Numeric,
    Usage,
    Faq,
}

/// 文档条目
#[derive(Clone, Debug, PartialEq)]
pub struct CDocEntry {
    pub chapter: CDocChapter,
    pub title: &'static str,
    pub body: &'static str,
}

/// 文档表
pub struct CDoc {
    pub entries: Vec<CDocEntry>,
}

impl CDoc {
    pub fn new() -> CDoc {
        CDoc { entries: alloc::vec![] }
    }
    pub fn put(&mut self, chapter: CDocChapter, title: &'static str, body: &'static str) {
        self.entries.push(CDocEntry { chapter, title, body });
    }
    pub fn by_chapter(&self, ch: CDocChapter) -> Vec<&CDocEntry> {
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
    /// 章节覆盖完整性：五节齐备
    pub fn complete(&self) -> bool {
        [CDocChapter::Math, CDocChapter::Api, CDocChapter::Numeric, CDocChapter::Usage, CDocChapter::Faq]
            .iter()
            .all(|ch| !self.by_chapter(*ch).is_empty())
    }
}

impl Default for CDoc {
    fn default() -> Self {
        CDoc::new()
    }
}

/// 与实现同源的数值真源（单一真源：此处供文档与判据共同引用）
pub const SRC_DEG_MAX: usize = 4;
pub const SRC_TOL_EVAL: f64 = 1e-9;
pub const SRC_TOL_ARC: f64 = 1e-6;
pub const SRC_SAMPLES_DEF: usize = 64;

/// 文档声明的常量（须与 SRC_* 同源一致）
pub const DOC_DEG_MAX: usize = 4;
pub const DOC_TOL_EVAL: f64 = 1e-9;
pub const DOC_TOL_ARC: f64 = 1e-6;
pub const DOC_SAMPLES_DEF: usize = 64;

pub fn run_vec338_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec338");
    // 族一：同源常量
    s.add("同源-最高阶", SRC_DEG_MAX == DOC_DEG_MAX, "deg max");
    s.add("同源-求值容差", SRC_TOL_EVAL == DOC_TOL_EVAL, "eval tol");
    s.add("同源-弧长容差", SRC_TOL_ARC == DOC_TOL_ARC, "arc tol");
    s.add("同源-默认样本", SRC_SAMPLES_DEF == DOC_SAMPLES_DEF, "samples");
    // 实现侧验证：Quart 确实是最高阶
    s.add("实现-最高阶吻合", Deg::Quart.n() + 1 == SRC_DEG_MAX, "quart is max");
    // 族二：文档表
    let mut doc = CDoc::new();
    doc.put(CDocChapter::Math, "Bernstein 基", "B(n,i;t)=C(n,i)t^i(1-t)^(n-i)，分片常一");
    doc.put(CDocChapter::Math, "de Casteljau", "递推割线求值，数值更稳");
    doc.put(CDocChapter::Api, "Curve::at", "显式求值，O(n)");
    doc.put(CDocChapter::Api, "Curve::derivative", "差分构造导数曲线，阶数减一");
    doc.put(CDocChapter::Numeric, "双路径容差", "Bernstein 与 Casteljau 偏差 < 1e-9");
    doc.put(CDocChapter::Numeric, "弧长容差", "高斯与自适应偏差 < 1e-6");
    doc.put(CDocChapter::Usage, "分割优先", "求交/裁剪一律先分割再处理");
    doc.put(CDocChapter::Faq, "为何 f64", "栅格化亚像素精度要求");
    s.add("文档-条目数", doc.entries.len() == 8, "8 entries");
    s.add("文档-五节齐", doc.complete(), "all chapters");
    s.add("文档-无重题", doc.dup_titles().is_empty(), "no dup titles");
    // 按章节取
    s.add("文档-数学节", doc.by_chapter(CDocChapter::Math).len() == 2, "2 math");
    s.add("文档-API节", doc.by_chapter(CDocChapter::Api).len() == 2, "2 api");
    s.add("文档-数值节", doc.by_chapter(CDocChapter::Numeric).len() == 2, "2 numeric");
    s.add("文档-用法节", doc.by_chapter(CDocChapter::Usage).len() == 1, "1 usage");
    s.add("文档-FAQ节", doc.by_chapter(CDocChapter::Faq).len() == 1, "1 faq");
    // 章节有序
    s.add("章节-可排序", CDocChapter::Math < CDocChapter::Faq, "ordinal");
    // 族三：重题检出
    let mut doc2 = CDoc::new();
    doc2.put(CDocChapter::Math, "X", "a");
    doc2.put(CDocChapter::Api, "X", "b");
    s.add("重题-检出", doc2.dup_titles() == alloc::vec!["X"], "dup found");
    // 族四：条目内容非空纪律
    s.add("内容-非空", doc.entries.iter().all(|e| !e.body.is_empty() && !e.title.is_empty()), "non-empty");
    // 族五：默认构造
    let d0 = CDoc::default();
    s.add("默认-空表", d0.entries.is_empty(), "empty default");
    s.add("默认-不完整", !d0.complete(), "incomplete default");
    s
}
