//! 深化层五 · F135 开发者文档站（2026-09-27 深化批次五 · h 层装配）。
//!
//! 装配与跨域接口件：文档站 ↔ F119 帮助中心的锚点映射（死锚检出）、
//! 上下文帮助查询接口、站内搜索结果的页面卡片装配。

use super::f135g::LinkGraph;
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 锚点映射：文档页 id ↔ 帮助中心文章 id；死锚 = 映射目标不存在
// ---------------------------------------------------------------------------

pub struct AnchorMap {
    /// (文档页, 帮助文章 id)
    pairs: alloc::vec::Vec<(&'static str, u32)>,
    /// 帮助中心实际存在的文章 id 集。
    known_articles: alloc::vec::Vec<u32>,
}

impl AnchorMap {
    pub fn new(known_articles: &[u32]) -> AnchorMap {
        AnchorMap {
            pairs: alloc::vec::Vec::new(),
            known_articles: known_articles.iter().copied().collect(),
        }
    }

    pub fn anchor(&mut self, doc_page: &'static str, article: u32) {
        self.pairs.push((doc_page, article));
    }

    /// 死锚：映射指向的帮助文章不存在（点了跳空——零静默）。
    pub fn dead_anchors(&self) -> alloc::vec::Vec<(&'static str, u32)> {
        self.pairs
            .iter()
            .filter(|(_, a)| !self.known_articles.contains(a))
            .map(|(p, a)| (*p, *a))
            .collect()
    }

    /// 上下文帮助查询：从文档页找帮助文章。
    pub fn lookup(&self, doc_page: &str) -> Option<u32> {
        self.pairs.iter().find(|(p, _)| *p == doc_page).map(|(_, a)| *a)
    }
}

// ---------------------------------------------------------------------------
// 链接图接线：帮助侧消费文档站死链报告 → 过滤出本站需修的行
// ---------------------------------------------------------------------------

/// 跨域输入：文档站死链清单（f135g 口径）→ 帮助中心待修行
/// （只关心指向帮助域的链接：路径前缀 help/）。
pub fn help_side_fixes(dead: &[(&'static str, &'static str)]) -> alloc::vec::Vec<(&'static str, &'static str)> {
    dead.iter()
        .filter(|(_, to)| to.starts_with("help/"))
        .copied()
        .collect()
}

/// 文档站链接图直读接口（复用 f135g 类型——一处一事实）。
pub fn doc_dead_links(g: &LinkGraph) -> alloc::vec::Vec<(&'static str, &'static str)> {
    g.dead_links()
}

// ---------------------------------------------------------------------------
// 搜索结果卡片装配：命中 → 卡片（标题/摘要位/得分），得分降序
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct ResultCard {
    pub page: &'static str,
    pub score: u32,
    pub has_summary: bool,
}

pub fn assemble_cards(
    hits: &[(&'static str, u32)],
    summaries: &[&'static str],
) -> alloc::vec::Vec<ResultCard> {
    let mut cards: alloc::vec::Vec<ResultCard> = hits
        .iter()
        .map(|(p, s)| ResultCard {
            page: *p,
            score: *s,
            has_summary: summaries.contains(p),
        })
        .collect();
    for i in 1..cards.len() {
        let k = (cards[i].score, cards[i].page);
        let tmp = cards[i].clone();
        let mut j = i;
        while j > 0 && (cards[j - 1].score, cards[j - 1].page) < k {
            cards[j] = cards[j - 1].clone();
            j -= 1;
        }
        cards[j] = tmp;
    }
    cards
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F135H_TAG: &str = "stareco-F135-deep5";

pub fn run_f135_deep5_checks() -> CheckSet {
    let mut set = CheckSet::new(F135H_TAG);

    // 锚点映射
    let mut am = AnchorMap::new(&[101, 102]);
    am.anchor("api-open", 101);
    am.anchor("quickstart", 999);
    set.add(
        "f135h dead anchor",
        am.dead_anchors() == alloc::vec![("quickstart", 999)],
        "死锚检出",
    );
    set.add("f135h lookup", am.lookup("api-open") == Some(101) && am.lookup("nope").is_none(), "上下文查询");

    // 帮助侧待修
    let dead = [
        ("api", "help/install"),
        ("faq", "ghost-page"),
        ("guide", "help/troubleshoot"),
    ];
    let fixes = help_side_fixes(&dead);
    set.add(
        "f135h help fixes",
        fixes.len() == 2 && fixes[0].1 == "help/install",
        "只收 help/ 域死链",
    );

    // 链接图直读
    let mut g = LinkGraph::new();
    g.page("home");
    g.link("home", "ghost");
    set.add("f135h doc dead", doc_dead_links(&g) == alloc::vec![("home", "ghost")], "复用 f135g 判定");

    // 结果卡片
    let cards = assemble_cards(
        &[("api", 5), ("faq", 9)],
        &["api"],
    );
    set.add(
        "f135h cards order",
        cards[0].page == "faq" && cards[1].page == "api",
        "得分降序",
    );
    set.add(
        "f135h summary flag",
        cards[1].has_summary && !cards[0].has_summary,
        "摘要位标注",
    );

    set
}

#[cfg(test)]
mod deep5_tests {
    use super::*;

    #[test]
    fn anchor_duplicate_allowed_last_wins_lookup() {
        let mut am = AnchorMap::new(&[1]);
        am.anchor("p", 1);
        am.anchor("p", 1);
        assert!(am.dead_anchors().is_empty());
        assert_eq!(am.lookup("p"), Some(1));
    }

    #[test]
    fn cards_empty() {
        assert!(assemble_cards(&[], &[]).is_empty());
    }
}
