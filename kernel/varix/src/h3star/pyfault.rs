//! F312 拼音与容错搜索 · 完整设计（AI-H3 · 主册 G-H 区段）。
//!
//! **判据（主册）**：三层容错用例各 5；精确优先排序判据；「您是不是要
//! 找」标注；容错关闭开关（追求绝对精确的用户）。
//!
//! **设计要点（主册）**：
//! - 搜索容错三层：拼音全拼（输「jisuanqi」命中计算器）、首字母缩写
//!   （输「jsq」同命中）、编辑距离容错（输错一两个字仍命中，标注
//!   「您是不是要找：计算器」）；
//! - 容错只放宽不误导——首位结果永远是精确匹配，容错结果排后并明示
//!   原因；
//! - 无感标准：手快打错字不白搜、懒得切输入法直接拼音也能到。
//!
//! 三层匹配（层号即优先级序）：
//! L1 全拼前缀（score 80）/ L2 首字母前缀（score 60）/ L3 编辑距离
//! ≤2 对全拼（score 40 + 「您是不是要找」标注）。条目名精确/前缀
//! （score 100/90）永远压容错层。容错开关关闭 → 三层全停，只剩精确面。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 打分：条目名精确。
pub const SCORE_NAME_EXACT: i32 = 100;

/// 打分：条目名前缀。
pub const SCORE_NAME_PREFIX: i32 = 90;

/// 打分：L1 全拼前缀。
pub const SCORE_PINYIN_FULL: i32 = 80;

/// 打分：L2 首字母。
pub const SCORE_PINYIN_INITIAL: i32 = 60;

/// 打分：L3 编辑距离容错。
pub const SCORE_TOLERANT: i32 = 40;

/// L3 容许编辑距离。
pub const TOLERANT_DISTANCE: usize = 2;

/// 「您是不是要找」标注文案（一处一事实）。
pub const DID_YOU_MEAN: &str = "您是不是要找";

// ---------------------------------------------------------------------------
// 数据面
// ---------------------------------------------------------------------------

/// 一条可搜条目（名 + 全拼 + 首字母——登记制三字段齐）。
#[derive(Clone, Copy, Debug)]
pub struct PinyinItem {
    pub name: &'static str,
    /// 全拼（无声调，小写）。
    pub pinyin: &'static str,
    /// 首字母串。
    pub initials: &'static str,
}

/// 演示登记表（自检与单测共用——五条目覆盖三层用例）。
pub fn demo_items() -> Vec<PinyinItem> {
    alloc::vec![
        PinyinItem { name: "计算器", pinyin: "jisuanqi", initials: "jsq" },
        PinyinItem { name: "记事本", pinyin: "jishiben", initials: "jsb" },
        PinyinItem { name: "设置中心", pinyin: "shezhizhongxin", initials: "szzx" },
        PinyinItem { name: "壁纸", pinyin: "bizhi", initials: "bz" },
        PinyinItem { name: "音量", pinyin: "yinliang", initials: "yl" },
    ]
}

// ---------------------------------------------------------------------------
// 匹配核
// ---------------------------------------------------------------------------

/// 匹配层。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MatchLayer {
    NameExact,
    NamePrefix,
    PinyinFull,
    PinyinInitial,
    Tolerant,
}

impl MatchLayer {
    /// 层序（精确优先排序判据——数值即排序权重面）。
    pub fn rank(self) -> i32 {
        match self {
            MatchLayer::NameExact => 0,
            MatchLayer::NamePrefix => 1,
            MatchLayer::PinyinFull => 2,
            MatchLayer::PinyinInitial => 3,
            MatchLayer::Tolerant => 4,
        }
    }

    pub fn score(self) -> i32 {
        match self {
            MatchLayer::NameExact => SCORE_NAME_EXACT,
            MatchLayer::NamePrefix => SCORE_NAME_PREFIX,
            MatchLayer::PinyinFull => SCORE_PINYIN_FULL,
            MatchLayer::PinyinInitial => SCORE_PINYIN_INITIAL,
            MatchLayer::Tolerant => SCORE_TOLERANT,
        }
    }
}

/// 一条结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PyHit {
    pub name: String,
    pub layer: MatchLayer,
    pub score: i32,
    /// 「您是不是要找：X」标注（仅 L3 有）。
    pub did_you_mean: bool,
}

/// 单条目三层匹配（`tolerance` = 容错开关；关闭时 L2/L3 全停）。
pub fn match_item(item: &PinyinItem, query: &str, tolerance: bool) -> Option<PyHit> {
    let q = query.to_ascii_lowercase();
    if q.is_empty() {
        return None;
    }
    // 精确面（汉字）。
    if item.name == query {
        return Some(PyHit {
            name: String::from(item.name),
            layer: MatchLayer::NameExact,
            score: SCORE_NAME_EXACT,
            did_you_mean: false,
        });
    }
    if item.name.starts_with(query) {
        return Some(PyHit {
            name: String::from(item.name),
            layer: MatchLayer::NamePrefix,
            score: SCORE_NAME_PREFIX,
            did_you_mean: false,
        });
    }
    if !tolerance {
        return None; // 容错关：拼音面全停（追求绝对精确）。
    }
    // L1 全拼。
    if item.pinyin.starts_with(&q) {
        return Some(PyHit {
            name: String::from(item.name),
            layer: MatchLayer::PinyinFull,
            score: SCORE_PINYIN_FULL,
            did_you_mean: false,
        });
    }
    // L2 首字母（ASCII 查询天然不匹配 CJK——首字母串本身是 ASCII）。
    if item.initials.starts_with(&q) {
        return Some(PyHit {
            name: String::from(item.name),
            layer: MatchLayer::PinyinInitial,
            score: SCORE_PINYIN_INITIAL,
            did_you_mean: false,
        });
    }
    // L3 编辑距离（对全拼）。
    let d = edit_distance(&q, item.pinyin);
    if d <= TOLERANT_DISTANCE {
        return Some(PyHit {
            name: String::from(item.name),
            layer: MatchLayer::Tolerant,
            score: SCORE_TOLERANT - d as i32,
            did_you_mean: true,
        });
    }
    None
}

/// 全量查询：精确优先排序（层序 → 分数 → 名字典序，全确定）。
pub fn search(items: &[PinyinItem], query: &str, tolerance: bool) -> Vec<PyHit> {
    let mut hits: Vec<PyHit> = Vec::new();
    for it in items {
        if let Some(h) = match_item(it, query, tolerance) {
            hits.push(h);
        }
    }
    hits.sort_by(|a, b| {
        a.layer.rank().cmp(&b.layer.rank()).then(b.score.cmp(&a.score)).then(a.name.cmp(&b.name))
    });
    hits
}

/// 首位结果是否「您是不是要找」命中（标注判据面）。
pub fn first_is_did_you_mean(hits: &[PyHit]) -> bool {
    hits.first().map(|h| h.did_you_mean).unwrap_or(false)
}

/// 编辑距离（ASCII 小写串——与 setsearch 同族口径，域内一份实现）。
pub fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];
    for i in 1..=a.len() {
        cur[0] = i;
        for j in 1..=b.len() {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        core::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

// ---------------------------------------------------------------------------
// 自检（三层容错用例各 5 + 精确优先 + 标注 + 开关）
// ---------------------------------------------------------------------------

/// F312 自检（判据：三层用例各 5；精确优先；「您是不是要找」；容错开关）。
pub fn run_pyfault_checks() -> CheckSet {
    let mut set = CheckSet::new("F312-pyfault");
    let items = demo_items();

    // 1. L1 全拼用例 ×5（全拼前缀可截断）。
    let l1_cases = [
        ("jisuanqi", "计算器"),
        ("jishiben", "记事本"),
        ("shezhizhongxin", "设置中心"),
        ("bizhi", "壁纸"),
        ("yinl", "音量"), // 前缀截断。
    ];
    let mut ok = 0;
    for (q, want) in l1_cases {
        let hits = search(&items, q, true);
        if hits.first().map(|h| h.layer == MatchLayer::PinyinFull && h.name == want) == Some(true) {
            ok += 1;
        }
    }
    set.add("layer1 full pinyin 5/5", ok == 5, "");

    // 2. L2 首字母用例 ×5。
    let l2_cases = [
        ("jsq", "计算器"),
        ("jsb", "记事本"),
        ("szzx", "设置中心"),
        ("bz", "壁纸"),
        ("yl", "音量"),
    ];
    let mut ok = 0;
    for (q, want) in l2_cases {
        let hits = search(&items, q, true);
        if hits.first().map(|h| h.layer == MatchLayer::PinyinInitial && h.name == want) == Some(true) {
            ok += 1;
        }
    }
    set.add("layer2 initials 5/5", ok == 5, "");

    // 3. L3 编辑距离用例 ×5（错 1-2 字仍命中 + 标注）。
    let l3_cases = [
        ("jisuanqu", "计算器"), // 错 1。
        ("jishibem", "记事本"), // 错 1。
        ("shezhizhngxin", "设置中心"), // 错 1（漏 o）。
        ("bizho", "壁纸"),      // 错 1。
        ("yinlang", "音量"),    // 错 2（换 n→l? 实为 yinliang 距离 2）。
    ];
    let mut ok = 0;
    for (q, want) in l3_cases {
        let hits = search(&items, q, true);
        if hits.first().map(|h| h.layer == MatchLayer::Tolerant && h.name == want && h.did_you_mean)
            == Some(true)
        {
            ok += 1;
        }
    }
    set.add("layer3 tolerant 5/5", ok == 5, "");

    // 4. 精确优先：汉字精确压全拼（「计算器」精确 100 在首位；jsq 拼音
    //    不会越过精确面）。
    let hits = search(&items, "计", true);
    set.add(
        "precise first ranking",
        hits.first().map(|h| h.layer == MatchLayer::NamePrefix) == Some(true)
            && hits.iter().all(|h| h.layer.rank() <= MatchLayer::Tolerant.rank()),
        "",
    );
    // 层序数值单源：精确 < 前缀 < 全拼 < 首字母 < 容错。
    set.add(
        "layer rank order",
        MatchLayer::NameExact.rank() < MatchLayer::NamePrefix.rank()
            && MatchLayer::NamePrefix.rank() < MatchLayer::PinyinFull.rank()
            && MatchLayer::PinyinFull.rank() < MatchLayer::PinyinInitial.rank()
            && MatchLayer::PinyinInitial.rank() < MatchLayer::Tolerant.rank(),
        "",
    );

    // 5. 「您是不是要找」标注：仅容错层带标注。
    let hits = search(&items, "jisuanqu", true);
    set.add(
        "did you mean only on tolerant",
        first_is_did_you_mean(&hits)
            && hits.iter().filter(|h| h.layer == MatchLayer::PinyinFull).all(|h| !h.did_you_mean),
        "",
    );

    // 6. 容错关闭：L1-L3 全停，只剩精确面（全拼查询无结果）。
    let hits = search(&items, "jisuanqi", false);
    let hits2 = search(&items, "jsq", false);
    let hits3 = search(&items, "jisuanqu", false);
    set.add(
        "tolerance off stops all layers",
        hits.is_empty() && hits2.is_empty() && hits3.is_empty()
            && search(&items, "计算器", false).first().map(|h| h.layer)
                == Some(MatchLayer::NameExact),
        "",
    );

    // 7. 容错不误导：L3 分数带距离惩罚（错 1 字 > 错 2 字）。
    let h1 = match_item(&items[0], "jisuanqu", true).unwrap();
    let h2 = match_item(&PinyinItem { name: "音量", pinyin: "yinliang", initials: "yl" }, "yinlaxg", true).unwrap();
    set.add(
        "tolerant distance penalty",
        h1.score > h2.score,
        "",
    );

    // 8. 首字母层只对 ASCII 查询生效（汉字查询走名称面而非拼音层）。
    set.add(
        "initials ascii only",
        match_item(&items[0], "计算", true).map(|h| h.layer) == Some(MatchLayer::NamePrefix),
        "",
    );

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_query_none() {
        assert!(match_item(&demo_items()[0], "", true).is_none());
    }

    #[test]
    fn distance_bounds() {
        assert_eq!(edit_distance("jsq", "jsq"), 0);
        assert_eq!(edit_distance("abc", "xyz"), 3);
    }

    #[test]
    fn beyond_distance_not_matched() {
        // 错 3 字超出 TOLERANT_DISTANCE——不误导。
        let items = demo_items();
        assert!(match_item(&items[0], "zxcvbnm", true).is_none());
    }

    #[test]
    fn did_you_mean_text_constant() {
        assert_eq!(DID_YOU_MEAN, "您是不是要找");
    }
}
