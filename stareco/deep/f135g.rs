//! 深化层四 · F135 开发者文档站（2026-09-27 深化批次四 · g 层）。
//!
//! 全站链接图（死链/孤页检测）、版本化路由（冲突与弃用标记）、代码
//! 示例提取器（围栏块抽取+行数门禁）、倒排搜索索引（tf 计分）、
//! 阅读进度模型。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 链接图：页 → 出链；死链（目标不存在）/ 孤页（零入链）检测
// ---------------------------------------------------------------------------

pub struct LinkGraph {
    /// 有内容实体的页登记表（死链判定的锚——只看边集合永远检不出死链）。
    pages: alloc::vec::Vec<&'static str>,
    /// (页, 出链目标) 序列。
    edges: alloc::vec::Vec<(&'static str, &'static str)>,
}

impl LinkGraph {
    pub fn new() -> LinkGraph {
        LinkGraph { pages: alloc::vec::Vec::new(), edges: alloc::vec::Vec::new() }
    }

    /// 登记内容页（首页/正文页都要登记）。
    pub fn page(&mut self, name: &'static str) {
        if !self.pages.contains(&name) {
            self.pages.push(name);
        }
    }

    pub fn link(&mut self, from: &'static str, to: &'static str) {
        self.edges.push((from, to));
    }

    /// 死链：出链目标不在页登记表（写了链接但没有对应页面）。
    pub fn dead_links(&self) -> alloc::vec::Vec<(&'static str, &'static str)> {
        self.edges
            .iter()
            .filter(|(_, to)| !self.pages.contains(to))
            .copied()
            .collect()
    }

    /// 孤页：登记表中从未被任何出链指向（无入链即孤——首页豁免由
    /// 调用方以「入口页」口径处理，机器面如实全列）。
    pub fn orphans(&self) -> alloc::vec::Vec<&'static str> {
        self.pages
            .iter()
            .filter(|p| !self.edges.iter().any(|(_, t)| t == *p))
            .copied()
            .collect()
    }
}

// ---------------------------------------------------------------------------
// 版本化路由：(页, 版本) 登记；同页双版本并存合法，版本冲突=重复登记
// ---------------------------------------------------------------------------

pub struct VersionRouter {
    routes: alloc::vec::Vec<(&'static str, u32, bool)>, // (页, 版本, deprecated)
}

impl VersionRouter {
    pub fn new() -> VersionRouter {
        VersionRouter { routes: alloc::vec::Vec::new() }
    }

    pub fn register(&mut self, page: &'static str, ver: u32, deprecated: bool) -> Result<(), &'static str> {
        if self.routes.iter().any(|(p, v, _)| *p == page && *v == ver) {
            return Err("同页同版本重复登记");
        }
        self.routes.push((page, ver, deprecated));
        Ok(())
    }

    pub fn resolve(&self, page: &str, ver: u32) -> Option<bool> {
        self.routes.iter().find(|(p, v, _)| *p == page && *v == ver).map(|(_, _, d)| *d)
    }

    /// 最新可用版本（取非弃用最大版本；全弃用如实返回最大弃用版）。
    pub fn latest(&self, page: &str) -> Option<(u32, bool)> {
        let mut best: Option<(u32, bool)> = None;
        for (p, v, d) in &self.routes {
            if *p != page {
                continue;
            }
            match best {
                Some((bv, bd)) if *v <= bv && !(*v == bv && bd && !*d) => {}
                _ => best = Some((*v, *d)),
            }
        }
        best
    }
}

// ---------------------------------------------------------------------------
// 代码示例提取器：从文档文本抽 ``` 围栏块；行数 ≥3 才算示例
// ---------------------------------------------------------------------------

/// 返回 (块序号, 行数) 序列。围栏以 ``` 行界——提取器不解析语言标注。
pub fn extract_code_blocks(doc: &str) -> alloc::vec::Vec<usize> {
    let mut out: alloc::vec::Vec<usize> = alloc::vec::Vec::new();
    let mut inside = false;
    let mut lines = 0usize;
    for line in doc.lines() {
        let t = line.trim_start();
        if t.starts_with("```") {
            if inside {
                if lines >= 3 {
                    out.push(lines);
                }
                lines = 0;
            }
            inside = !inside;
            continue;
        }
        if inside {
            lines += 1;
        }
    }
    // 未闭合围栏：文档被截断——按零静默原则丢弃尾巴并交给门禁判
    // （不闭合 = 非法文档，不计入块清单）。
    out
}

/// 未闭合围栏检测（文档门禁面）。
pub fn unclosed_fence(doc: &str) -> bool {
    let mut count = 0usize;
    for line in doc.lines() {
        if line.trim_start().starts_with("```") {
            count += 1;
        }
    }
    count % 2 == 1
}

// ---------------------------------------------------------------------------
// 倒排搜索索引：词 → (页, tf)；查询 = 全词交集，按 tf 和降序
// ---------------------------------------------------------------------------

pub struct SearchIndex {
    /// (词, 页) 出现序列（同词同页重复出现即 tf 累积）。
    postings: alloc::vec::Vec<(&'static str, &'static str)>,
}

impl SearchIndex {
    pub fn new() -> SearchIndex {
        SearchIndex { postings: alloc::vec::Vec::new() }
    }

    pub fn index(&mut self, term: &'static str, page: &'static str) {
        self.postings.push((term, page));
    }

    fn tf(&self, term: &str, page: &str) -> u32 {
        self.postings.iter().filter(|(t, p)| *t == term && *p == page).count() as u32
    }

    /// 查询：包含全部词的页，按 tf 总和降序（平局页名字序）。
    pub fn query(&self, terms: &[&'static str]) -> alloc::vec::Vec<(&'static str, u32)> {
        if terms.is_empty() {
            return alloc::vec::Vec::new();
        }
        let mut pages: alloc::vec::Vec<&'static str> = alloc::vec::Vec::new();
        for (t, p) in &self.postings {
            if terms.contains(t) && !pages.contains(p) {
                pages.push(p);
            }
        }
        let mut scored: alloc::vec::Vec<(&'static str, u32)> = pages
            .into_iter()
            .filter(|p| terms.iter().all(|t| self.tf(t, p) > 0))
            .map(|p| {
                let s: u32 = terms.iter().map(|t| self.tf(t, p)).sum();
                (p, s)
            })
            .collect();
        for i in 1..scored.len() {
            let k = scored[i];
            let mut j = i;
            while j > 0
                && (scored[j - 1].1 < k.1 || (scored[j - 1].1 == k.1 && scored[j - 1].0 > k.0))
            {
                scored[j] = scored[j - 1];
                j -= 1;
            }
            scored[j] = k;
        }
        scored
    }
}

// ---------------------------------------------------------------------------
// 阅读进度：读者 × 页 × 百分比 → 完读清单与放弃清单
// ---------------------------------------------------------------------------

pub struct ReadingMark {
    pub reader: &'static str,
    pub page: &'static str,
    pub pct: u32,
}

pub fn reading_outcomes(marks: &[ReadingMark]) -> (alloc::vec::Vec<&'static str>, alloc::vec::Vec<&'static str>) {
    let mut finished: alloc::vec::Vec<&'static str> = alloc::vec::Vec::new();
    let mut abandoned: alloc::vec::Vec<&'static str> = alloc::vec::Vec::new();
    for m in marks {
        if m.pct >= 95 && !finished.contains(&m.page) {
            finished.push(m.page);
        } else if m.pct < 30 && !finished.contains(&m.page) && !abandoned.contains(&m.page) {
            abandoned.push(m.page);
        }
    }
    (finished, abandoned)
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F135G_TAG: &str = "stareco-F135-deep4";

pub fn run_f135_deep4_checks() -> CheckSet {
    let mut set = CheckSet::new(F135G_TAG);

    // 链接图（页注册表 + 边；死链锚定在注册表上）
    let mut g = LinkGraph::new();
    g.page("home");
    g.page("quickstart");
    g.page("api");
    g.link("home", "quickstart");
    g.link("quickstart", "api");
    g.link("quickstart", "ghost-page");
    set.add(
        "f135g dead link",
        g.dead_links() == alloc::vec![("quickstart", "ghost-page")],
        "死链检出",
    );
    set.add("f135g orphan", g.orphans() == alloc::vec!["home"], "孤页=零入链（home 是入口）");

    // 版本路由
    let mut r = VersionRouter::new();
    let _ = r.register("api", 1, true);
    let _ = r.register("api", 2, false);
    set.add("f135g dup route", r.register("api", 2, false).is_err(), "重复登记拒绝");
    set.add("f135g latest", r.latest("api") == Some((2, false)), "最新非弃用版");
    set.add("f135g latest missing", r.latest("nope").is_none(), "未知页 None");

    // 代码块提取
    let doc = "intro\n```rust\nlet a = 1;\nlet b = 2;\nlet c = 3;\n```\nmid\n```\nshort\n```\ntail";
    let blocks = extract_code_blocks(doc);
    set.add("f135g blocks", blocks == alloc::vec![3], "仅 ≥3 行块入选");
    set.add("f135g closed", !unclosed_fence(doc), "围栏闭合");
    set.add("f135g unclosed", unclosed_fence("```rust\nlet a = 1;\n"), "未闭合检出");

    // 搜索索引
    let mut idx = SearchIndex::new();
    idx.index("open", "api");
    idx.index("open", "api");
    idx.index("open", "faq");
    idx.index("file", "api");
    let q1 = idx.query(&["open"]);
    set.add("f135g tf rank", q1 == alloc::vec![("api", 2), ("faq", 1)], "tf 降序");
    let q2 = idx.query(&["open", "file"]);
    set.add("f135g and", q2 == alloc::vec![("api", 3)], "AND 交集");
    set.add("f135g empty", idx.query(&[]).is_empty(), "空词空结果");

    // 阅读进度
    let marks = [
        ReadingMark { reader: "a", page: "api", pct: 97 },
        ReadingMark { reader: "b", page: "faq", pct: 12 },
        ReadingMark { reader: "c", page: "api", pct: 50 },
    ];
    let (fin, abn) = reading_outcomes(&marks);
    set.add(
        "f135g reading",
        fin == alloc::vec!["api"] && abn == alloc::vec!["faq"],
        "完读/放弃分列",
    );

    set
}

#[cfg(test)]
mod deep4_tests {
    use super::*;

    #[test]
    fn link_graph_page_set() {
        let mut g = LinkGraph::new();
        g.page("a");
        g.page("b");
        g.link("a", "b");
        g.link("b", "a");
        assert!(g.dead_links().is_empty());
        assert!(g.orphans().is_empty()); // 互链无孤页
        // 未登记页 = 死链；无入链登记页 = 孤页。
        g.link("b", "ghost");
        assert_eq!(g.dead_links(), alloc::vec![("b", "ghost")]);
        g.page("lonely");
        assert_eq!(g.orphans(), alloc::vec!["lonely"]);
    }

    #[test]
    fn fence_pairs_only() {
        // 两对围栏 → 各按行数入选/淘汰。
        let doc = "```\na\nb\nc\n```\ntext\n```\nx\n```\nend";
        assert!(unclosed_fence(doc) == false);
        assert_eq!(extract_code_blocks(doc), alloc::vec![3]);
    }
}
