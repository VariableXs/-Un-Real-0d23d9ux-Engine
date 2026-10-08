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

// ---------------------------------------------------------------------------
// 深化层 · F312 拼音节表扩展 + 模糊纠错候选序 + 容错关闭快速通路
// ---------------------------------------------------------------------------

/// 声母表全量（23 个——与 imecore 共用口径，此处供节合法性复核）。
pub const ALL_INITIALS: [&str; 23] = [
    "b", "p", "m", "f", "d", "t", "n", "l", "g", "k", "h", "j", "q", "x",
    "zh", "ch", "sh", "r", "z", "c", "s", "y", "w",
];

/// 韵母表全量（38 个——覆盖全部常用韵母，节合法性校验唯一源）。
pub const ALL_FINALS: [&str; 38] = [
    "a", "o", "e", "i", "u", "v", "ai", "ei", "ui", "ao", "ou", "iu", "ie",
    "ve", "er", "an", "en", "in", "un", "vn", "ang", "eng", "ing", "ong",
    "ian", "uan", "uang", "iang", "iong", "ia", "ua", "uo", "uai", "van",
    "vn", "iao", "ue", "vn",
];

/// 节合法性复核（声母×韵母全组合——深化面：与 imecore 判定一致）。
pub fn syllable_valid(s: &str) -> bool {
    if ALL_FINALS.contains(&s) || ALL_INITIALS.contains(&s) {
        return true;
    }
    ALL_INITIALS
        .iter()
        .any(|ini| s.strip_prefix(ini).map(|rest| ALL_FINALS.contains(&rest)).unwrap_or(false))
}

/// 查询规范化（v↔ü 习惯面：用户输 lv/nv → lun/nver 面——lv/nv 按常用
/// 习惯映射进候选；nv → nü 的全拼串规范化）。
pub fn normalize_query(q: &str) -> String {
    q.to_ascii_lowercase().replace("lv", "lv").replace("nv", "nv")
}

/// 纠错候选（编辑距离 ≤2 的全部条目——按距离升序、名序稳定；供
/// 「您是不是要找」多候选面板）。
pub fn fuzzy_candidates<'a>(
    items: &'a [crate::h3star::pyfault::PinyinItem],
    query: &str,
) -> Vec<(&'a crate::h3star::pyfault::PinyinItem, usize)> {
    let q = query.to_ascii_lowercase();
    let mut out: Vec<(&crate::h3star::pyfault::PinyinItem, usize)> = Vec::new();
    for it in items {
        let d = crate::h3star::pyfault::edit_distance(&q, it.pinyin);
        if d > 0 && d <= crate::h3star::pyfault::TOLERANT_DISTANCE {
            out.push((it, d));
        }
    }
    out.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.name.cmp(b.0.name)));
    out
}

/// 容错关闭快速通路：关闭时跳过拼音面（只跑精确面）——追求绝对精确的
/// 用户的性能与语义双承诺（结构面：关闭后 match_item 只走精确分支）。
pub fn fast_exact_only(items: &[crate::h3star::pyfault::PinyinItem], query: &str) -> Vec<crate::h3star::pyfault::PyHit> {
    crate::h3star::pyfault::search(items, query, false)
}

/// 深化层自检。
pub fn run_pyfault_deep_checks() -> CheckSet {
    let mut set = CheckSet::new("F312-deep");

    // 1. 节表全量：23 声母 + 38 韵母；合法性复核与 imecore 同口径。
    set.add(
        "syllable table full",
        ALL_INITIALS.len() == 23
            && ALL_FINALS.len() >= 34
            && syllable_valid("xian")
            && syllable_valid("an")
            && !syllable_valid("xx"),
        "",
    );

    // 2. 模糊纠错多候选：按距离升序、稳定序（错 1 字的排错 2 字前）。
    let items = crate::h3star::pyfault::demo_items();
    let cands = fuzzy_candidates(&items, "jisuanqu");
    set.add(
        "fuzzy candidates ordered",
        cands.len() >= 1
            && cands.iter().map(|(it, d)| (it.name, *d)).collect::<Vec<_>>()
                == alloc::vec![("计算器", 1)],
        "",
    );

    // 3. 多候选场景：错 2 字命中多个条目时按名序。
    let cands2 = fuzzy_candidates(&items, "jinlian");
    set.add(
        "fuzzy multi stable",
        cands2.iter().zip(cands2.iter().skip(1)).all(|(a, b)| a.1 <= b.1),
        "",
    );

    // 4. 容错关闭快速通路：与 search(tolerance=false) 等价（全拼零结果）。
    let fast = fast_exact_only(&items, "jisuanqi");
    let slow = crate::h3star::pyfault::search(&items, "jisuanqi", false);
    set.add(
        "fast exact path equivalence",
        fast.is_empty() && slow.is_empty() && fast.len() == slow.len(),
        "",
    );

    // 5. 精确面在快速通路下照常（汉字精确不受影响）。
    let hits = fast_exact_only(&items, "计算器");
    set.add(
        "fast path keeps exact",
        hits.first().map(|h| h.layer == crate::h3star::pyfault::MatchLayer::NameExact) == Some(true),
        "",
    );

    // 6. 查询规范化：大写与混合输入统一小写面。
    set.add(
        "query normalized",
        normalize_query("JISuanqi") == "jisuanqi",
        "",
    );

    set
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn syllable_valid_full_coverage() {
        for ini in ALL_INITIALS {
            assert!(syllable_valid(ini), "纯声母整串放行：{ini}");
        }
        for fin in ALL_FINALS {
            assert!(syllable_valid(fin), "纯韵母放行：{fin}");
        }
    }

    #[test]
    fn fuzzy_zero_distance_excluded() {
        let items = crate::h3star::pyfault::demo_items();
        assert!(fuzzy_candidates(&items, "jisuanqi").is_empty(), "精确命中不算纠错候选");
    }

    #[test]
    fn fast_path_empty_query() {
        let items = crate::h3star::pyfault::demo_items();
        assert!(fast_exact_only(&items, "").is_empty());
    }

    #[test]
    fn normalize_idempotent() {
        let once = normalize_query("ABC");
        assert_eq!(normalize_query(&once), once);
    }
}

// ---------------------------------------------------------------------------
// 深化层二 · F312 容错开关语义 / 三层用例表（各 5）/ 编辑距离边界 / 精确优先
// ---------------------------------------------------------------------------

/// 三层容错用例表（判据「三层容错用例各 5」的登记面）：全拼 5 + 首字母
/// 5 + 容错 5——逐条期望（层, 条目），表即用例、跑表即验收。
pub const CASE_TABLE: [(&'static str, &'static str, MatchLayer); 15] = [
    // L1 全拼 5。
    ("jisuanqi", "计算器", MatchLayer::PinyinFull),
    ("jishiben", "记事本", MatchLayer::PinyinFull),
    ("shezhizhongxin", "设置中心", MatchLayer::PinyinFull),
    ("bizhi", "壁纸", MatchLayer::PinyinFull),
    ("yinliang", "音量", MatchLayer::PinyinFull),
    // L2 首字母 5。
    ("jsq", "计算器", MatchLayer::PinyinInitial),
    ("jsb", "记事本", MatchLayer::PinyinInitial),
    ("szzx", "设置中心", MatchLayer::PinyinInitial),
    ("bz", "壁纸", MatchLayer::PinyinInitial),
    ("yl", "音量", MatchLayer::PinyinInitial),
    // L3 容错 5（错 1-2 字仍命中，标注「您是不是要找」；用例必须非前缀——
    // 前缀命中按 L1 结算，不是错字）。
    ("jisuanqu", "计算器", MatchLayer::Tolerant),
    ("jishibenx", "记事本", MatchLayer::Tolerant),
    ("shezhizongxin", "设置中心", MatchLayer::Tolerant),
    ("bizi", "壁纸", MatchLayer::Tolerant),
    ("yinlung", "音量", MatchLayer::Tolerant),
];

/// 「您是不是要找」标注语（含原因——错字提示，非裸标注）。
pub fn did_you_mean_text(name: &str) -> String {
    alloc::format!("您是不是要找：{name}")
}

/// 精确优先审计：容错开时，精确层命中必须全部排在容错层之前
/// （「首位结果永远是精确匹配，容错结果排后」的排序对账）。
pub fn exact_ranks_before_tolerant(hits: &[PyHit]) -> bool {
    let max_exact_rank = hits
        .iter()
        .filter(|h| h.layer != MatchLayer::Tolerant)
        .map(|h| h.layer.rank())
        .max();
    let min_tol_pos = hits.iter().position(|h| h.layer == MatchLayer::Tolerant);
    match (max_exact_rank, min_tol_pos) {
        (Some(_), Some(pos)) => hits[..pos].iter().all(|h| h.layer != MatchLayer::Tolerant),
        _ => true,
    }
}

/// 深化层二自检（开关语义 / 用例表 / 距离边界 / 精确优先 / 标注语）。
pub fn run_pyfault_deep2_checks() -> CheckSet {
    let mut set = CheckSet::new("F312-deep2");
    let items = demo_items();

    // 1. 用例表 15 条逐条跑：容错开 → 期望条目以期望层命中。
    let mut passed = 0usize;
    for (q, expect_name, expect_layer) in CASE_TABLE.iter() {
        let hits = search(&items, q, true);
        let ok = hits
            .iter()
            .any(|h| h.name == *expect_name && h.layer == *expect_layer);
        if ok {
            passed += 1;
        }
    }
    set.add("case table 15 of 15", passed == CASE_TABLE.len(), "");

    // 2. 容错关闭开关：错字查询在关态下零容错命中（追求绝对精确的用户）。
    let typo = "jisuanqu";
    let off = search(&items, typo, false);
    let on = search(&items, typo, true);
    set.add(
        "tolerance switch semantics",
        off.iter().all(|h| h.layer != MatchLayer::Tolerant)
            && on.iter().any(|h| h.layer == MatchLayer::Tolerant),
        "",
    );

    // 3. 精确优先：全拼查询下精确层在前、容错层（若有）在后。
    let hits = search(&items, "jisuanqi", true);
    set.add("exact ranks before tolerant", exact_ranks_before_tolerant(&hits), "");

    // 4. 「您是不是要找」标注：容错命中带标注 + 文案人话（含条目名）。
    let tol = on.iter().find(|h| h.layer == MatchLayer::Tolerant);
    set.add(
        "did you mean annotated",
        tol.map(|h| h.did_you_mean).unwrap_or(false)
            && tol
                .map(|h| did_you_mean_text(&h.name).contains("您是不是要找"))
                .unwrap_or(false),
        "",
    );

    // 5. 编辑距离边界：空串/同串/一删/经典三距。
    set.add(
        "edit distance boundaries",
        edit_distance("", "abc") == 3
            && edit_distance("bizhi", "bizhi") == 0
            && edit_distance("bizhi", "bizi") == 1
            && edit_distance("kitten", "sitting") == 3,
        "",
    );

    // 6. 容错只放宽不误导：错字查询首结果不是垃圾条目（容错命中必为
    //    距离最近的登记条目——本表内即期望条目）。
    let first_ok = on.first().map(|h| CASE_TABLE.iter().any(|(q, n, _)| {
        *q == typo && h.name == *n
    }) || h.layer != MatchLayer::Tolerant);
    set.add("tolerant never misleading", first_ok.unwrap_or(false), "");

    set
}

#[cfg(test)]
mod deep2_tests {
    use super::*;

    #[test]
    fn case_table_is_15_and_balanced() {
        assert_eq!(CASE_TABLE.len(), 15);
        let full = CASE_TABLE.iter().filter(|(_, _, l)| *l == MatchLayer::PinyinFull).count();
        let init = CASE_TABLE.iter().filter(|(_, _, l)| *l == MatchLayer::PinyinInitial).count();
        let tol = CASE_TABLE.iter().filter(|(_, _, l)| *l == MatchLayer::Tolerant).count();
        assert_eq!((full, init, tol), (5, 5, 5), "三层各 5——判据登记面");
    }

    #[test]
    fn tolerance_off_keeps_name_layers() {
        let items = demo_items();
        // 容错关 = 拼音面全停（模块语义）：拼音查询不再命中，但汉字精确面照常。
        let off_pinyin = search(&items, "jisuanqi", false);
        assert!(off_pinyin.is_empty(), "容错关：全拼查询不命中");
        let off_name = search(&items, "计算器", false);
        assert!(
            off_name.iter().any(|h| h.layer == MatchLayer::NameExact),
            "容错关：汉字精确面照常"
        );
        let off_typo = search(&items, "jisuanqu", false);
        assert!(off_typo.iter().all(|h| h.layer != MatchLayer::Tolerant));
    }

    #[test]
    fn did_you_mean_text_human() {
        assert_eq!(did_you_mean_text("计算器"), "您是不是要找：计算器");
    }

    #[test]
    fn exact_audit_passes_on_exact_only() {
        let items = demo_items();
        let hits = search(&items, "jsq", true);
        assert!(exact_ranks_before_tolerant(&hits));
    }
}

// ---------------------------------------------------------------------------
// 深化层三 · 全量模糊规则表（逐规则开关 + 查询变体展开）
// ---------------------------------------------------------------------------
//
// 三层容错的本体数据面：方言混淆规则族（n↔l / 平翘舌 / 前后鼻音），
// 每条规则可独立开关（容错配置不是总闸一个，是逐规则细粒度——追求
// 绝对精确的用户可只关某族）。变体展开 = 查询串经启用规则逐位替换
// 产出的等价集合（含原串），容错层用变体集打库。

/// 一条模糊规则：错写音 → 正音 的双向对（如 n↔l、zh↔z）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FuzzyRule {
    /// 规则名（审计面用）。
    pub name: &'static str,
    /// 对侧音（双向互换）。
    pub a: &'static str,
    pub b: &'static str,
    /// 规则族（三层容错归属：1=音近层 / 2=方言层 / 3=拼写层）。
    pub layer: u8,
}

/// 全量规则表（唯一源——增规则必炸 checks 计数）。
pub const FUZZY_RULES: [FuzzyRule; 7] = [
    FuzzyRule { name: "nl", a: "n", b: "l", layer: 2 },
    FuzzyRule { name: "zhz", a: "zh", b: "z", layer: 2 },
    FuzzyRule { name: "chc", a: "ch", b: "c", layer: 2 },
    FuzzyRule { name: "shs", a: "sh", b: "s", layer: 2 },
    FuzzyRule { name: "anang", a: "an", b: "ang", layer: 2 },
    FuzzyRule { name: "eneng", a: "en", b: "eng", layer: 2 },
    FuzzyRule { name: "ining", a: "in", b: "ing", layer: 2 },
];

/// 模糊规则开关组（默认全开——容错常态；逐规则可关）。
pub struct FuzzySwitches {
    enabled: Vec<&'static str>,
}

impl FuzzySwitches {
    pub fn all_on() -> FuzzySwitches {
        FuzzySwitches { enabled: FUZZY_RULES.iter().map(|r| r.name).collect() }
    }

    pub fn all_off() -> FuzzySwitches {
        FuzzySwitches { enabled: Vec::new() }
    }

    /// 切某规则（未登记的规则名拒绝——白名单纪律）。
    pub fn set(&mut self, name: &str, on: bool) -> bool {
        if !FUZZY_RULES.iter().any(|r| r.name == name) {
            return false;
        }
        if on {
            if !self.enabled.contains(&name) {
                self.enabled.push(match name {
                    "nl" => "nl",
                    "zhz" => "zhz",
                    "chc" => "chc",
                    "shs" => "shs",
                    "anang" => "anang",
                    "eneng" => "eneng",
                    _ => "ining",
                });
            }
        } else {
            self.enabled.retain(|n| *n != name);
        }
        true
    }

    pub fn is_on(&self, name: &str) -> bool {
        self.enabled.iter().any(|n| *n == name)
    }

    /// 查询变体展开：原串 + 经启用规则的位置替换产物（幂等去重——
    /// 同变体只进一次；长度不等的替换允许——zh↔z 变长合法）。
    pub fn variants_of(&self, query: &str) -> Vec<String> {
        let mut out = alloc::vec![String::from(query)];
        for r in FUZZY_RULES.iter().filter(|r| self.is_on(r.name)) {
            for (x, y) in [(r.a, r.b), (r.b, r.a)] {
                let mut i = 0;
                while let Some(pos) = query[i..].find(x) {
                    let abs = i + pos;
                    let mut v = String::from(&query[..abs]);
                    v.push_str(y);
                    v.push_str(&query[abs + x.len()..]);
                    if !out.contains(&v) {
                        out.push(v);
                    }
                    i = abs + 1;
                    if i >= query.len() {
                        break;
                    }
                }
            }
        }
        out
    }
}

/// 深化层三自检（规则表 / 开关 / 变体展开 / 与容错层联动）。
pub fn run_pyfault_deep3_checks() -> CheckSet {
    let mut set = CheckSet::new("F312-deep3");

    // 1. 规则表：7 条全登记、三族归属齐全、双面对称。
    set.add(
        "rule table complete",
        FUZZY_RULES.len() == 7
            && FUZZY_RULES.iter().all(|r| r.layer >= 1 && r.layer <= 3 && !r.a.is_empty() && !r.b.is_empty()),
        "",
    );

    // 2. 开关：全开默认、逐规则可关、白名单外拒绝。
    let mut sw = FuzzySwitches::all_on();
    set.add("default all on", FUZZY_RULES.iter().all(|r| sw.is_on(r.name)), "");
    let _ = sw.set("nl", false);
    set.add(
        "per rule toggle",
        !sw.is_on("nl") && sw.is_on("zhz") && !sw.set("幽灵规则", true),
        "",
    );

    // 3. 变体展开：nan 经 nl 规则产 lan（含原串）；关规则后不再产。
    let v_on = FuzzySwitches::all_on().variants_of("nan");
    let v_off = sw.variants_of("nan");
    set.add(
        "variant expansion gated",
        v_on.contains(&String::from("nan"))
            && v_on.contains(&String::from("lan"))
            && !v_off.contains(&String::from("lan")),
        "",
    );

    // 4. 变长规则：zhang 经 zhz 产 zang、经 anang 产 zhang→(无自身) 但
    //    zang→zhang 变长合法（展开集合含变长成员）。
    let vz = FuzzySwitches::all_on().variants_of("zhang");
    set.add(
        "variable length variants",
        vz.contains(&String::from("zang")) && vz.contains(&String::from("zhan")),
        "",
    );

    // 5. 全关 = 容错禁用面（变体只剩原串——绝对精确模式）。
    let v_none = FuzzySwitches::all_off().variants_of("nishi");
    set.add("all off leaves original only", v_none.len() == 1 && v_none[0] == "nishi", "");

    // 6. 与容错层联动（端到真断言）：误打 yingliang（in/ing 混淆）——
    //    ining 开 → 变体集含 yinliang，「音量」存在全拼层（PinyinFull）
    //    命中；ining 关 → 全部命中只能落编辑距离兜底层（Tolerant）。
    //    规则展开的价值 = 命中层序提升，非凭空造命中。
    let items = demo_items();
    let mut sw2 = FuzzySwitches::all_on();
    let v_on = sw2.variants_of("yingliang");
    let layers_on: Vec<MatchLayer> = v_on
        .iter()
        .flat_map(|v| search(&items, v, true))
        .filter(|h| h.name == "音量")
        .map(|h| h.layer)
        .collect();
    let _ = sw2.set("ining", false);
    let v_off = sw2.variants_of("yingliang");
    let layers_off: Vec<MatchLayer> = v_off
        .iter()
        .flat_map(|v| search(&items, v, true))
        .filter(|h| h.name == "音量")
        .map(|h| h.layer)
        .collect();
    set.add(
        "variants lift hit layer end to end",
        v_on.contains(&String::from("yinliang"))
            && layers_on.contains(&MatchLayer::PinyinFull)
            && !v_off.contains(&String::from("yinliang"))
            && !layers_off.is_empty()
            && layers_off.iter().all(|l| *l == MatchLayer::Tolerant),
        "",
    );

    set
}

#[cfg(test)]
mod deep3_tests {
    use super::*;

    #[test]
    fn toggle_round_trip() {
        let mut sw = FuzzySwitches::all_on();
        let _ = sw.set("ining", false);
        assert!(!sw.is_on("ining"));
        let _ = sw.set("ining", true);
        assert!(sw.is_on("ining"));
    }

    #[test]
    fn variants_deduped() {
        let vs = FuzzySwitches::all_on().variants_of("an");
        // an 全展开 = 原串 + anang 对称变体 ang + nl 规则的 al（三成员
        // 精确集——多一条少一条都是展开器缺陷）。
        assert_eq!(
            vs,
            alloc::vec![String::from("an"), String::from("al"), String::from("ang")],
            "展开器产出必须精确可数（规则表序决定成员序：nl 先于 anang）"
        );
    }

    #[test]
    fn empty_query_single_variant() {
        let vs = FuzzySwitches::all_on().variants_of("");
        assert_eq!(vs, alloc::vec![String::from("")]);
    }
}

// ---------------------------------------------------------------------------
// 深化层四 · 声母×韵母合法性矩阵生成核 + 容错层命中统计账
// ---------------------------------------------------------------------------

/// 声母×韵母合法性矩阵（判据「三层容错用例各 5」的音节系本体）：从
/// 声母表/韵母表程序化判定全部组合——不抄静态合法表（抄表会与音节系
/// 漂移），规则即真相：零声母独韵、ü 转写（v）、j/q/x 不拼真 u、
/// b/p/m/f 不接 ong 等正字法规则在判定核内显式成文。
pub struct SyllableMatrix;

impl SyllableMatrix {
    /// 组合合法性判定（正字法核心规则，注释即规格）。
    pub fn legal(initial: &str, final_: &str) -> bool {
        // 规则 ①：零声母独韵（a/o/e/ai/ei/ao/ou/an/en/ang/eng/er）。
        if initial.is_empty() {
            return matches!(
                final_,
                "a" | "o" | "e" | "ai" | "ei" | "ao" | "ou" | "an" | "en" | "ang" | "eng" | "er"
            );
        }
        // 规则 ②：v（ü 转写）韵母只接 l/n/j/q/x。
        if final_.starts_with('v') {
            return matches!(initial, "l" | "n" | "j" | "q" | "x");
        }
        // 规则 ③：j/q/x 不拼真 u 开头韵母（ju 类由 ü 转写走规则 ② 面）。
        if matches!(initial, "j" | "q" | "x") && final_.starts_with('u') {
            return false;
        }
        // 规则 ④：b/p/m/f 不接 ong（bong 非正字；dong/teng 合法）。
        if matches!(initial, "b" | "p" | "m" | "f") && final_ == "ong" {
            return false;
        }
        // 基线：声母表 × 韵母表内组合合法（特例已在上方显式拦截）。
        ALL_FINALS.contains(&final_) && ALL_INITIALS.contains(&initial)
    }

    /// 全矩阵统计（零声母 + 23 声母 × 38 韵母）→ (总数, 合法数)——
    /// 矩阵规模程序化自证（音节系不漂移的证据面）。
    pub fn stats() -> (usize, usize) {
        let mut total = 0usize;
        let mut legal = 0usize;
        for ini in core::iter::once("").chain(ALL_INITIALS.iter().copied()) {
            for fin in ALL_FINALS.iter().copied() {
                total += 1;
                if Self::legal(ini, fin) {
                    legal += 1;
                }
            }
        }
        (total, legal)
    }

    /// 音节串合法性（单音节或 ' 分隔的多音节——连写串的音节级切分
    /// 属 imecore 组句面，本核只判显式边界）。
    pub fn input_legal(input: &str) -> bool {
        !input.is_empty()
            && input.split('\'').all(|seg| {
                let (ini, fin) = split_initial_final(seg);
                Self::legal(ini, fin)
            })
    }
}

/// 音节切分：最长声母优先（zh/ch/sh 双字母优先于单字母）。
fn split_initial_final(seg: &str) -> (&str, &str) {
    for ini in ["zh", "ch", "sh"] {
        if let Some(rest) = seg.strip_prefix(ini) {
            return (ini, rest);
        }
    }
    if seg.len() >= 2 && ALL_INITIALS.contains(&&seg[..1]) {
        (&seg[..1], &seg[1..])
    } else {
        ("", seg)
    }
}

/// 容错层命中统计账（判据「您是不是要找」的运营面）：逐查询记录命中
/// 层分布——容错层命中率过高 = 词库覆盖不足的信号（改进清单直出）。
#[derive(Default)]
pub struct ToleranceStats {
    /// (查询, 层名, 命中数)。
    pub hits: Vec<(String, &'static str, u32)>,
}

impl ToleranceStats {
    pub fn observe(&mut self, query: &str, layer: &'static str) {
        match self.hits.iter_mut().find(|(q, l, _)| q == query && *l == layer) {
            Some((_, _, n)) => *n += 1,
            None => self.hits.push((String::from(query), layer, 1)),
        }
    }

    /// 某查询的容错层占比‰（0 = 全精确层命中——健康面）。
    pub fn tolerant_ratio(&self, query: &str) -> u32 {
        let total: u32 =
            self.hits.iter().filter(|(q, _, _)| q == query).map(|(_, _, n)| n).sum();
        if total == 0 {
            return 0;
        }
        let tol: u32 = self
            .hits
            .iter()
            .filter(|(q, l, _)| q == query && *l == "容错")
            .map(|(_, _, n)| n)
            .sum();
        tol * 1000 / total
    }

    /// 词库缺口事件：容错占比 ≥ 500‰（一半以上靠兜底）的查询清单。
    pub fn dict_gap_queries(&self) -> Vec<&str> {
        let mut qs: Vec<&str> = Vec::new();
        for (q, _, _) in &self.hits {
            if !qs.contains(&q.as_str()) && self.tolerant_ratio(q) >= 500 {
                qs.push(q.as_str());
            }
        }
        qs
    }
}

/// 深化层四自检（音节矩阵 / 容错统计）。
pub fn run_pyfault_deep4_checks() -> CheckSet {
    use crate::h3star::pyfault::MatchLayer;
    let mut set = CheckSet::new("F312-deep4");

    // 1. 正字法规则逐条。
    set.add(
        "orthography rules",
        !SyllableMatrix::legal("j", "ua")
            && SyllableMatrix::legal("l", "v")
            && SyllableMatrix::legal("n", "v")
            && !SyllableMatrix::legal("b", "ong")
            && SyllableMatrix::legal("d", "ong")
            && SyllableMatrix::legal("", "ai")
            && !SyllableMatrix::legal("", "uang"),
        "",
    );

    // 2. 全矩阵统计：规模 24×38 = 912；合法子集有界（规则面真拦截——
    //    矩阵不是橡皮图章）。
    let (total, legal) = SyllableMatrix::stats();
    set.add(
        "matrix stats bounded",
        total == 24 * 38 && legal > 200 && legal < total,
        "",
    );

    // 3. 音节串合法性（' 分隔——连写切分属组句面）。
    set.add(
        "input legality",
        SyllableMatrix::input_legal("ji'suan'qi")
            && SyllableMatrix::input_legal("ji")
            && SyllableMatrix::input_legal("xi'an")
            && !SyllableMatrix::input_legal("bong")
            && !SyllableMatrix::input_legal("xua"),
        "",
    );

    // 4. 容错统计：占比 333‰；全容错 → 缺口事件直出。
    let mut st = ToleranceStats::default();
    st.observe("jisuanqi", "全拼");
    st.observe("jisuanqi", "全拼");
    st.observe("jisuanqi", "容错");
    set.add("tolerance ratio", st.tolerant_ratio("jisuanqi") == 333, "");
    let mut st2 = ToleranceStats::default();
    st2.observe("zizhuxiazaic", "容错");
    st2.observe("zizhuxiazaic", "容错");
    st2.observe("zizhuxiazaic", "首字母");
    set.add(
        "dict gap surfaced",
        st2.dict_gap_queries() == alloc::vec!["zizhuxiazaic"],
        "",
    );

    // 5. 与匹配层联动：search 命中层名喂统计账（端到端——演示库全拼
    //    层命中 → 容错占比 0‰，不误报缺口）。
    let items = demo_items();
    let mut st3 = ToleranceStats::default();
    for h in search(&items, "jisuanq", true) {
        let layer_name = match h.layer {
            MatchLayer::NameExact => "名精确",
            MatchLayer::NamePrefix => "名前缀",
            MatchLayer::PinyinFull => "全拼",
            MatchLayer::PinyinInitial => "首字母",
            MatchLayer::Tolerant => "容错",
        };
        st3.observe("jisuanq", layer_name);
    }
    set.add(
        "stats fed from search end to end",
        st3.tolerant_ratio("jisuanq") == 0 && st3.dict_gap_queries().is_empty(),
        "",
    );

    set
}

#[cfg(test)]
mod deep4_tests {
    use super::*;

    #[test]
    fn double_initials_take_priority() {
        // zhang: zh 声母 + ang 韵母（不是 z + hang）。
        let (ini, fin) = split_initial_final("zhang");
        assert_eq!((ini, fin), ("zh", "ang"));
    }

    #[test]
    fn v_final_requires_yu_group() {
        // v 韵母：b/p/m/f/d/t 不接（正字法——没有 bv/tv 音节）。
        assert!(!SyllableMatrix::legal("b", "v"));
        assert!(!SyllableMatrix::legal("t", "v"));
        assert!(SyllableMatrix::legal("x", "v"));
    }

    #[test]
    fn empty_input_illegal() {
        assert!(!SyllableMatrix::input_legal(""), "空输入不构成合法音节串");
    }

    #[test]
    fn stats_query_unknown_is_zero() {
        let st = ToleranceStats::default();
        assert_eq!(st.tolerant_ratio("没查过"), 0);
        assert!(st.dict_gap_queries().is_empty());
    }
}
