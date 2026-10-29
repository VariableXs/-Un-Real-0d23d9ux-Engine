//! UNX-F56078 内存对照规程文档件（0x609A0 段）
//!
//! 锚点：UNX-F56078-J1 规程章节全量（规程五章节——窗规则/负载冻结/缺侧
//! 纪律/失败痕迹/存证归档——逐章落账：章号 × 标题 × 条款数）；J2 条款
//! 引用一致（章内条款编号与条款文本逐条对齐，引用越界即拒）；J3 规程
//! 审计链（章指纹链、封账只读、章节闭集）三判据全绿。
//! 域：CoRun·UNX-F2-B40（内存域·规程文档面）。
//!
//! 结构：规程五章节登记册（章号闭集 → 标题 × 条款数逐章落账）+ 条款
//! 注册核（条款文本非空、章内编号 1..n 连续）+ 条款引用核（引用号越章
//! 界拒收）+ 章指纹链（前章指纹混入后章）+ 封账语义 + 注入核（未知章号/
//! 空标题/空条款/引用越界/重复章/封账改写/空册复算必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x609A0 独占 0x20）。
pub const SEG_BASE: u32 = 0x609A0;
pub const DOC_UNK_CHAPTER: u32 = SEG_BASE + 0x01; // 未知章号（五闭集外）
pub const DOC_TITLE_EMPTY: u32 = SEG_BASE + 0x02; // 章标题为空
pub const DOC_CLAUSE_EMPTY: u32 = SEG_BASE + 0x03; // 条款文本为空
pub const DOC_REF_RANGE: u32 = SEG_BASE + 0x04; // 条款引用越章界
pub const DOC_CHAPTER_DUP: u32 = SEG_BASE + 0x05; // 同章号重复落账
pub const DOC_SEALED: u32 = SEG_BASE + 0x06; // 规程册已封账拒写
pub const DOC_EMPTY: u32 = SEG_BASE + 0x07; // 空册复算

/// 规程五章节闭集。
pub const C_WINDOW: usize = 0;
pub const C_LOADFROZEN: usize = 1;
pub const C_NADISC: usize = 2;
pub const C_FAILTRACE: usize = 3;
pub const C_ARCHIVE: usize = 4;
pub const C_COUNT: usize = 5;
pub const C_NAMES: [&str; C_COUNT] =
    ["window_rule", "load_frozen", "na_discipline", "fail_trace", "archive_vault"];

/// 单章条款：章内编号 × 文本。
#[derive(Clone)]
pub struct Clause {
    pub no: u32,
    pub text: String,
}

/// 单章：章号 × 标题 × 条款集。
#[derive(Clone)]
pub struct DocChapter {
    pub chapter: usize,
    pub title: String,
    pub clauses: Vec<Clause>,
    pub fp: u32,
    pub chain: u32,
}

pub const CHAIN_SEED: u32 = 0x811C_9DC5;

/// 章指纹：FNV-1a 32 位（章名 × 标题 × 条款数复合）。
pub fn chapter_fingerprint(chapter: usize, title: &str, clause_count: usize) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let line = format!("{}|{}|{}", C_NAMES[chapter], title, clause_count);
    for b in line.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 规程册总账。
pub struct DocBook {
    pub chapters: Vec<DocChapter>,
    pub sealed: bool,
}

impl DocBook {
    pub fn new() -> Self {
        DocBook { chapters: Vec::new(), sealed: false }
    }

    fn find(&self, chapter: usize) -> Option<usize> {
        for i in 0..self.chapters.len() {
            if self.chapters[i].chapter == chapter {
                return Some(i);
            }
        }
        None
    }

    /// 章落账：章号闭集 + 标题非空 + 条款逐条非空且编号 1..n 连续 + 重复章拒。
    pub fn write_chapter(&mut self, chapter: usize, title: &str, clauses: &[&str]) -> Result<u32, u32> {
        if self.sealed {
            return Err(DOC_SEALED);
        }
        if chapter >= C_COUNT {
            return Err(DOC_UNK_CHAPTER);
        }
        if title.is_empty() {
            return Err(DOC_TITLE_EMPTY);
        }
        if clauses.is_empty() {
            return Err(DOC_CLAUSE_EMPTY);
        }
        for (i, t) in clauses.iter().enumerate() {
            if t.is_empty() {
                return Err(DOC_CLAUSE_EMPTY);
            }
            let _ = i;
        }
        if self.find(chapter).is_some() {
            return Err(DOC_CHAPTER_DUP);
        }
        let mut cs: Vec<Clause> = Vec::new();
        for (i, t) in clauses.iter().enumerate() {
            cs.push(Clause { no: (i + 1) as u32, text: t.to_string() });
        }
        let fp = chapter_fingerprint(chapter, title, cs.len());
        let chain = match self.chapters.last() {
            None => CHAIN_SEED ^ fp,
            Some(prev) => prev.fp ^ fp,
        };
        self.chapters.push(DocChapter {
            chapter,
            title: title.to_string(),
            clauses: cs,
            fp,
            chain,
        });
        Ok(chain)
    }

    /// 条款引用核：章内编号必须 1..=条款数。
    pub fn cite_clause(&self, chapter: usize, no: u32) -> Result<String, u32> {
        let idx = match self.find(chapter) {
            Some(i) => i,
            None => return Err(DOC_EMPTY),
        };
        let ch = &self.chapters[idx];
        if no == 0 || no as usize > ch.clauses.len() {
            return Err(DOC_REF_RANGE);
        }
        Ok(ch.clauses[(no - 1) as usize].text.clone())
    }

    /// J1：规程全量（五章齐 + 逐章标题非空）。
    pub fn chapters_ok(&self) -> bool {
        if self.chapters.len() != C_COUNT {
            return false;
        }
        let mut seen = [false; C_COUNT];
        for ch in self.chapters.iter() {
            if ch.chapter >= C_COUNT || seen[ch.chapter] || ch.title.is_empty() {
                return false;
            }
            seen[ch.chapter] = true;
        }
        true
    }

    /// J1：章指纹逐章复算一致。
    pub fn fingerprints_ok(&self) -> bool {
        self.chapters.iter().all(|ch| {
            ch.fp == chapter_fingerprint(ch.chapter, &ch.title, ch.clauses.len())
        })
    }

    /// J3：章指纹链复核。
    pub fn chain_ok(&self) -> bool {
        if self.chapters.is_empty() {
            return false;
        }
        for i in 0..self.chapters.len() {
            let expect = if i == 0 {
                CHAIN_SEED ^ self.chapters[0].fp
            } else {
                self.chapters[i - 1].fp ^ self.chapters[i].fp
            };
            if self.chapters[i].chain != expect {
                return false;
            }
        }
        true
    }

    /// 条款总数闭式（J1 审计面）。
    pub fn clause_census(&self) -> usize {
        let mut n = 0;
        for ch in self.chapters.iter() {
            n += ch.clauses.len();
        }
        n
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 链尾指纹（封账材料）。
    pub fn chain_tail(&self) -> u32 {
        match self.chapters.last() {
            None => CHAIN_SEED,
            Some(ch) => ch.chain,
        }
    }
}

/// 标准夹具：五章齐（每章 2 条款）。
pub fn standard_book() -> DocBook {
    let mut b = DocBook::new();
    let titles: [&str; C_COUNT] =
        ["采集窗规则", "负载脚本冻结", "缺侧 N/A 纪律", "采集失败痕迹", "对照行存证归档"];
    let clause_sets: [[&str; 2]; C_COUNT] = [
        ["窗长固定 30 秒", "窗间禁并行采集"],
        ["负载脚本入冻结库", "签名逐字比对"],
        ["缺侧行只出 N/A", "补采后转正留痕"],
        ["失败逐次落痕", "痕迹链复合防抽改"],
        ["对照行逐行归档", "封档后只读"],
    ];
    for c in 0..C_COUNT {
        if b.write_chapter(c, titles[c], &clause_sets[c]).is_err() {
            panic!("standard fixture write_chapter must succeed");
        }
    }
    b
}

/// 注入核：7 注入必抓（未知章号/空标题/空条款/引用越界/重复章/封账改写/未登记引用）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 未知章号
    let mut b = DocBook::new();
    if b.write_chapter(9, "t", &["x"]) == Err(DOC_UNK_CHAPTER) {
        caught += 1;
    }
    // 2) 空标题
    if b.write_chapter(C_WINDOW, "", &["x"]) == Err(DOC_TITLE_EMPTY) {
        caught += 1;
    }
    // 3) 空条款
    if b.write_chapter(C_WINDOW, "t", &["ok", ""]) == Err(DOC_CLAUSE_EMPTY) {
        caught += 1;
    }
    // 4) 重复章
    if b.write_chapter(C_WINDOW, "t", &["x"]).is_ok()
        && b.write_chapter(C_WINDOW, "t2", &["y"]) == Err(DOC_CHAPTER_DUP)
    {
        caught += 1;
    }
    // 5) 引用越界（条款数 1，引用 2）
    let sb = standard_book();
    if sb.cite_clause(C_WINDOW, 3).err() == Some(DOC_REF_RANGE) {
        caught += 1;
    }
    // 6) 未登记章引用
    let mut b2 = DocBook::new();
    if b2.cite_clause(C_ARCHIVE, 1).err() == Some(DOC_EMPTY) {
        caught += 1;
    }
    // 7) 封账后改写拒
    let mut sb2 = standard_book();
    sb2.seal();
    if sb2.write_chapter(C_ARCHIVE, "late", &["x"]) == Err(DOC_SEALED) {
        caught += 1;
    }
    (caught, 7)
}

/// 规程册渲染导出（逐章一行：章名|标题|条款数）。
pub fn render_book(b: &DocBook) -> String {
    let mut out = String::new();
    for ch in b.chapters.iter() {
        out.push_str(&format!("{}|{}|{}\n", C_NAMES[ch.chapter], ch.title, ch.clauses.len()));
    }
    out
}

/// 渲染完整性核：行数齐 + 章名闭集命中 + 三段。
pub fn render_complete(text: &str, expect_rows: usize) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() != expect_rows {
        return false;
    }
    for ln in lines.iter() {
        let parts: Vec<&str> = ln.split('|').collect();
        if parts.len() != 3 {
            return false;
        }
        let mut hit = false;
        for c in C_NAMES.iter() {
            if parts[0] == *c {
                hit = true;
            }
        }
        if !hit {
            return false;
        }
    }
    true
}

/// 判据入口（CheckSet）。
pub fn run_unx56078_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56078");
    // J1-1 规程五章全量
    let b = standard_book();
    let ok = b.chapters_ok() && b.chapters.len() == C_COUNT;
    s.add("56078-J1-规程全量", ok, if ok { "" } else { "章节残缺" });
    // J1-2 章指纹逐章复算一致
    let ok = b.fingerprints_ok();
    s.add("56078-J1-章指纹一致", ok, if ok { "" } else { "章指纹漂移" });
    // J1-3 章节闭集齐
    let ok = C_NAMES.len() == C_COUNT && C_NAMES[4] == "archive_vault";
    s.add("56078-J1-章节闭集齐", ok, if ok { "" } else { "闭集漂移" });
    // J1-4 条款总数闭式（五章各 2 条 = 10）
    let ok = b.clause_census() == 10;
    s.add("56078-J1-条款闭式", ok, if ok { "" } else { "条款计数漂移" });
    // J2-1 条款引用命中（第 1 章第 1 条）
    let ok = b.cite_clause(C_WINDOW, 1) == Ok("窗长固定 30 秒".to_string());
    s.add("56078-J2-条款引用命中", ok, if ok { "" } else { "引用漂移" });
    // J2-2 引用越界拒
    let mut b2 = standard_book();
    let ok = b2.cite_clause(C_NADISC, 5).err() == Some(DOC_REF_RANGE)
        && b2.cite_clause(C_NADISC, 0).err() == Some(DOC_REF_RANGE);
    s.add("56078-J2-引用越界拒", ok, if ok { "" } else { "引用闸失效" });
    // J2-3 条款编号连续（逐章 1..n）
    let ok = b.chapters.iter().all(|ch| {
        let mut expect = 1u32;
        for cl in ch.clauses.iter() {
            if cl.no != expect {
                return false;
            }
            expect += 1;
        }
        true
    });
    s.add("56078-J2-条款编号连续", ok, if ok { "" } else { "编号断档" });
    // J3-1 章指纹链复核
    let ok = b.chain_ok();
    s.add("56078-J3-章指纹链", ok, if ok { "" } else { "指纹链断裂" });
    // J3-2 链尾稳定
    let ok = standard_book().chain_tail() == standard_book().chain_tail();
    s.add("56078-J3-链尾稳定", ok, if ok { "" } else { "链尾漂移" });
    // J3-3 链尾敏感（少一章链尾变）
    let mut b3 = DocBook::new();
    for c in 0..C_COUNT - 1 {
        let _ = b3.write_chapter(c, "t", &["x", "y"]);
    }
    let ok = b3.chain_tail() != standard_book().chain_tail();
    s.add("56078-J3-链尾敏感", ok, if ok { "" } else { "链尾钝感" });
    // J3-4 封账拒写
    let mut b4 = standard_book();
    b4.seal();
    let ok = b4.write_chapter(C_ARCHIVE, "late", &["x"]) == Err(DOC_SEALED);
    s.add("56078-J3-封账拒写", ok, if ok { "" } else { "封账穿透" });
    // J2-4 渲染导出五行齐
    let text = render_book(&standard_book());
    let ok = render_complete(&text, 5) && text.contains("window_rule|采集窗规则|2");
    s.add("56078-J2-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // 注入核 7/7
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56078-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56078_all_checks_green() {
        let set = run_unx56078_checks();
        assert!(set.all_passed(), "UNX-F56078 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56078_chain_stable() {
        assert_eq!(standard_book().chain_tail(), standard_book().chain_tail());
    }
}
