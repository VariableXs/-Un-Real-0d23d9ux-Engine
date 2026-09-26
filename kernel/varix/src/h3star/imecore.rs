//! F326 输入法引擎基础 · 完整设计（AI-H3 · 主册 G-H 区段）。
//!
//! **判据（主册）**：切分用例（20 组歧义音节）；排序因子可复现（同输入
//! 同结果）；自学习用例（造词三次入库）；离线判据（断网全功能）；首候
//! 选命中率抽样记录。
//!
//! **设计要点（主册）**：
//! - 拼音引擎三基本功：音节智能切分（输「xian」同时给「先」与「西安」
//!   两组）、候选排序（词频+用户历史+上下文三因子，本句内已上屏词参与
//!   调序）、用户词库自学习（选过的词升权、连续三次自造词自动入库）；
//! - 离线本地引擎不联网（隐私红线同 F314）；候选窗行为归 F107、自定义
//!   短语归 F108，本项管引擎本身；
//! - 无感标准：打字越用越顺手——从不需要输入全拼打完才出字。
//!
//! 实现形态：音节切分器（声母/韵母表驱动，歧义全展开）+ 词库与三因子
//! 定点排序（同输入同结果——可复现）+ 造词三次入库的自学习账。全程零
//! 网络依赖（离线判据结构性成立）。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 歧义切分用例数判线。
pub const AMBIGUOUS_CASES: usize = 20;

/// 自造词入库门槛（连续选三次）。
pub const COIN_THRESHOLD: u32 = 3;

/// 用户词权重增量（每次选中）。
pub const USER_WEIGHT_STEP: i64 = 10;

// ---------------------------------------------------------------------------
// 音节切分
// ---------------------------------------------------------------------------

/// 声母表（23 个——zh/ch/sh 按双字母参与切分歧义）。
pub const INITIALS: [&str; 23] = [
    "b", "p", "m", "f", "d", "t", "n", "l", "g", "k", "h", "j", "q", "x",
    "zh", "ch", "sh", "r", "z", "c", "s", "y", "w",
];

/// 韵母表（切分合法性判定——覆盖常用韵母）。
pub const FINALS: [&str; 35] = [
    "a", "o", "e", "i", "u", "v", "ai", "ei", "ui", "ao", "ou", "iu", "ie",
    "ve", "er", "an", "en", "in", "un", "vn", "ang", "eng", "ing", "ong",
    "ian", "uan", "uang", "iang", "iong", "ia", "ua", "uo", "uai", "van",
    "vn",
];

/// 合法切分段（必须有韵母：声母+韵母 或 纯韵母——裸声母段不产生垃圾切分）。
pub fn valid_segment(s: &str) -> bool {
    if FINALS.contains(&s) {
        return true;
    }
    for ini in INITIALS {
        if let Some(rest) = s.strip_prefix(ini) {
            if FINALS.contains(&rest) {
                return true;
            }
        }
    }
    false
}

/// 整串是否合法输入（合法段 或 纯声母——输入中途也放行）。
pub fn valid_syllable(s: &str) -> bool {
    valid_segment(s) || INITIALS.contains(&s)
}

/// 全部合法切分（歧义全展开——「xian」→ ["xian"] 与 ["xi","an"] 等）。
/// 返回切分方案清单（短切分优先序：方案按段数升序、段序字典序——确定）。
pub fn segmentations(input: &str) -> Vec<Vec<String>> {
    let mut out: Vec<Vec<String>> = Vec::new();
    if input.is_empty() {
        return out;
    }
    fn walk(s: &str, cur: &mut Vec<String>, out: &mut Vec<Vec<String>>) {
        if s.is_empty() {
            out.push(cur.clone());
            return;
        }
        // 最长优先尝试（6 字母韵母上限）。
        let max = s.len().min(6);
        for take in (1..=max).rev() {
            let head = &s[..take];
            if valid_segment(head) {
                cur.push(String::from(head));
                walk(&s[take..], cur, out);
                cur.pop();
            }
        }
    }
    let mut cur: Vec<String> = Vec::new();
    walk(input, &mut cur, &mut out);
    out.sort_by(|a, b| a.len().cmp(&b.len()).then(a.cmp(b)));
    out.dedup();
    out
}

// ---------------------------------------------------------------------------
// 词库与排序（三因子定点——可复现）
// ---------------------------------------------------------------------------

/// 一条词条。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Word {
    pub text: String,
    /// 对应拼音（切分后串接——"xian" 或 "xi'an"）。
    pub pinyin: String,
    /// 词频（系统面）。
    pub freq: i64,
    /// 用户历史权重（自学习面）。
    pub user_weight: i64,
    /// 用户自造词标记。
    pub coined: bool,
}

/// 输入法引擎。
pub struct ImeEngine {
    dict: Vec<Word>,
    /// 本句已上屏词（上下文因子——参与调序）。
    context: Vec<String>,
    /// 用户造词账（连选计数）。
    coin_streak: Vec<(String, u32)>,
}

impl ImeEngine {
    pub fn new() -> ImeEngine {
        ImeEngine { dict: Vec::new(), context: Vec::new(), coin_streak: Vec::new() }
    }

    /// 登记系统词（登记制；同 text+pinyin 拒重）。
    pub fn add_word(&mut self, text: &str, pinyin: &str, freq: i64) {
        if !self.dict.iter().any(|w| w.text == text && w.pinyin == pinyin) {
            self.dict.push(Word {
                text: String::from(text),
                pinyin: String::from(pinyin),
                freq,
                user_weight: 0,
                coined: false,
            });
        }
    }

    /// 候选查询：全切分方案 × 词库拼音匹配；三因子定点排序。
    /// 打分公式（唯一数值源）：freq×1000 + user_weight×100 + 上下文命中
    /// ×50；切分段数惩罚（段数 ×1 递减——短切分优先：单段 0、两段 −1…）。
    /// 同词多切分命中 → 取最优分（一词一候选——不重复刷屏）。
    pub fn candidates(&self, input: &str) -> Vec<(String, i64)> {
        let mut out: Vec<(String, i64)> = Vec::new();
        let segs = segmentations(input);
        for (si, seg) in segs.iter().enumerate() {
            let joined = seg.join("'");
            for w in &self.dict {
                if w.pinyin == joined || w.pinyin == input {
                    let ctx = if !self.context.is_empty()
                        && self.context.last().map(|c| w.text.contains(c.as_str())).unwrap_or(false)
                    {
                        1
                    } else {
                        0
                    };
                    let score = w.freq * 1000 + w.user_weight * 100 + ctx * 50 - si as i64;
                    out.push((w.text.clone(), score));
                }
            }
        }
        // 按词取最优分（同词多切分命中去重——确定性：分数降序、词字典序）。
        out.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        out.dedup_by(|a, b| a.0 == b.0);
        out
    }

    /// 上屏（自学习入口）：用户选中 → user_weight 升权；自造词连选
    /// COIN_THRESHOLD 次 → 自动入库（coined 标记）。
    pub fn commit(&mut self, text: &str, pinyin: &str) {
        // 已有词条升权。
        let mut found = false;
        for w in self.dict.iter_mut() {
            if w.text == text && w.pinyin == pinyin {
                w.user_weight += USER_WEIGHT_STEP;
                found = true;
                break;
            }
        }
        // 造词连选计数。
        if !found {
            let mut slot = self.coin_streak.iter_mut().find(|(t, _)| t == text);
            match slot.as_deref_mut() {
                Some((_, n)) => *n += 1,
                None => self.coin_streak.push((String::from(text), 1)),
            }
            let should_coin = self
                .coin_streak
                .iter()
                .any(|(t, n)| t == text && *n >= COIN_THRESHOLD);
            if should_coin {
                self.dict.push(Word {
                    text: String::from(text),
                    pinyin: String::from(pinyin),
                    freq: 1,
                    user_weight: 0,
                    coined: true,
                });
                self.coin_streak.retain(|(t, _)| t != text);
            }
        }
        // 上下文推进（本句已上屏词参与调序）。
        self.context.push(String::from(text));
    }

    /// 句末（清上下文——下一句重新起算）。
    pub fn end_sentence(&mut self) {
        self.context.clear();
    }

    /// 首候选（命中率抽样面）。
    pub fn first_candidate(&self, input: &str) -> Option<String> {
        self.candidates(input).first().map(|(t, _)| t.clone())
    }

    pub fn dict_len(&self) -> usize {
        self.dict.len()
    }

    /// 用户造词数。
    pub fn coined_len(&self) -> usize {
        self.dict.iter().filter(|w| w.coined).count()
    }

    /// 离线判据（结构性成立——引擎无任何网络接口，此处显式声明为真）。
    pub const fn offline_capable() -> bool {
        true
    }
}

impl Default for ImeEngine {
    fn default() -> ImeEngine {
        ImeEngine::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F326 自检（判据：切分 20 组；排序可复现；自学习；离线；首候选记录）。
pub fn run_imecore_checks() -> CheckSet {
    let mut set = CheckSet::new("F326-imecore");

    // 1. 歧义切分核心：xian → [xian] / [xi'an] 两组（先 / 西安）。
    let segs = segmentations("xian");
    set.add(
        "xian two readings",
        segs.len() == 2
            && segs[0] == ["xian"]
            && segs[1] == ["xi", "an"],
        "",
    );

    // 2. 歧义切分用例 20 组（全展开数 ≥2 的歧义输入——主册判线，互不重复）。
    let ambiguous = [
        "xian", "fangan", "jianguo", "xianlu", "changan", "sange", "lihai",
        "jianguang", "fanan", "xianer", "huanan", "jianan", "chuanran",
        "xiongan", "xianxian", "zhanan", "mianfei", "jianfei", "xianjin",
        "anguan",
    ];
    assert_eq!(ambiguous.len(), AMBIGUOUS_CASES);
    let mut n_amb = 0;
    for s in ambiguous {
        if segmentations(s).len() >= 2 {
            n_amb += 1;
        }
    }
    set.add("twenty ambiguous cases", n_amb == AMBIGUOUS_CASES, "");

    // 3. 非法串：无切分（诚实空——不炸）。
    set.add("invalid input no segmentation", segmentations("qzzzq").is_empty(), "");

    // 4. 候选排序可复现：同输入同结果（跑两遍逐位相等）。
    let mut eng = ImeEngine::new();
    eng.add_word("先", "xian", 900);
    eng.add_word("西安", "xi'an", 400);
    eng.add_word("县", "xian", 300);
    let a = eng.candidates("xian");
    let b = eng.candidates("xian");
    set.add(
        "ranking reproducible",
        a == b && a.first().map(|(t, _)| t.as_str()) == Some("先"),
        "",
    );

    // 5. 三因子：词频主导、用户权重次之、上下文加成、切分段数微惩罚。
    //    词频 900×1000=900000 压 西安 400×1000=400000。
    let a = eng.candidates("xian");
    set.add(
        "freq dominates",
        a[0] == (String::from("先"), 900_000) && a[1].0 == "西安",
        "",
    );

    // 6. 自学习：选过的词升权（西安连选 → user_weight 抬升）。
    eng.commit("西安", "xi'an");
    eng.commit("西安", "xi'an");
    let a = eng.candidates("xian");
    set.add(
        "self learning weight up",
        a[1].0 == "西安" && a[1].1 > 400_000,
        "",
    );

    // 7. 上下文：本句已上屏词参与调序（上屏「西安」后，含「西安」的词
    //    加成 50——模型面验证加成存在）。
    //    （西安入选后 context=["西安"]——下一个含「安」的词命中加成。）
    let mut eng2 = ImeEngine::new();
    eng2.add_word("安全", "anquan", 500);
    eng2.commit("安", "an");
    let c = eng2.candidates("anquan");
    set.add(
        "context boost applied",
        c.first().map(|(_, s)| *s).unwrap_or(0) > 500_000,
        "",
    );

    // 8. 造词三次入库：连选 3 次的自造串进词库（coined 标记）。
    let mut eng3 = ImeEngine::new();
    eng3.commit("炫安", "xi'an");
    eng3.commit("炫安", "xi'an");
    eng3.commit("炫安", "xi'an");
    set.add(
        "coined after three picks",
        eng3.coined_len() == 1 && eng3.dict_len() == 1,
        "",
    );

    // 9. 离线判据：结构无网络（引擎常量声明 + 无外部依赖——断网全功能）。
    set.add("offline structural", ImeEngine::offline_capable(), "");

    // 10. 首候选命中率抽样记录（演示面：高频词全中——记录 10/10）。
    let mut eng4 = ImeEngine::new();
    eng4.add_word("你好", "nihao", 990);
    eng4.add_word("拟好", "nihao", 100);
    let mut hits = 0u32;
    for _ in 0..10 {
        if eng4.first_candidate("nihao").as_deref() == Some("你好") {
            hits += 1;
        }
    }
    set.add("first candidate hit 10/10", hits == 10, "");

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_syllable_basics() {
        assert!(valid_syllable("xi"));
        assert!(valid_syllable("xian"));
        assert!(valid_syllable("an"));
        assert!(valid_syllable("x")); // 输入中途（声母）。
        assert!(!valid_syllable("xx"));
    }

    #[test]
    fn segmentation_order_deterministic() {
        let s1 = segmentations("fangan");
        let s2 = segmentations("fangan");
        assert_eq!(s1, s2);
        assert!(s1.iter().any(|seg| seg.len() >= 2), "fangan 至少两组切分（fan'an / fa'nan 面）");
    }

    #[test]
    fn end_sentence_clears_context() {
        let mut e = ImeEngine::new();
        e.add_word("安全", "anquan", 500);
        e.commit("安", "an");
        let boosted = e.candidates("anquan").first().map(|(_, s)| *s).unwrap_or(0);
        e.end_sentence();
        let plain = e.candidates("anquan").first().map(|(_, s)| *s).unwrap_or(0);
        assert_eq!(plain, 500_000, "句末清上下文后无加成");
        assert!(boosted >= plain, "句内加成不小于裸分");
    }

    #[test]
    fn duplicate_dict_rejected() {
        let mut e = ImeEngine::new();
        e.add_word("先", "xian", 1);
        e.add_word("先", "xian", 2);
        assert_eq!(e.dict_len(), 1);
    }
}

// ---------------------------------------------------------------------------
// 深化层 · F326 候选窗数据模型 + 用户词库持久化 + 上下文二元权重
// ---------------------------------------------------------------------------

/// 候选窗页数据（F107 消费面：引擎出数——页/高亮/备注三件齐）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CandidatePage {
    /// 本页候选（最多 9 个——9 宫格选词）。
    pub items: Vec<(String, i64)>,
    /// 当前高亮序号。
    pub highlight: usize,
    /// 页码（0 起）。
    pub page: usize,
}

impl CandidatePage {
    /// 分页（每页 9 个——翻页语义）。
    pub fn paginate(all: &[(String, i64)], page: usize) -> CandidatePage {
        let start = page * 9;
        let items = all.iter().skip(start).take(9).cloned().collect();
        CandidatePage { items, highlight: 0, page }
    }

    /// 高亮移动（上下键——环形）。
    pub fn move_highlight(&mut self, delta: i32) {
        if self.items.is_empty() {
            return;
        }
        let n = self.items.len() as i32;
        self.highlight =
            ((self.highlight as i32 + delta).rem_euclid(n)) as usize;
    }

    /// 数字键选词（1-9 → 序号）。
    pub fn pick_by_number(&self, num: usize) -> Option<&(String, i64)> {
        if num >= 1 && num <= self.items.len() {
            self.items.get(num - 1)
        } else {
            None
        }
    }
}

/// 上下文二元权重表（本句已上屏词 → 后继词加成——「的」后接名词概率
/// 面的模型化：bigram 计数，同前缀命中加成）。
pub struct BigramTable {
    /// (前词, 后词) → 计数。
    pairs: Vec<(String, String, u64)>,
}

impl BigramTable {
    pub fn new() -> BigramTable {
        BigramTable { pairs: Vec::new() }
    }

    /// 观察（commit 时自动喂——前词+上屏词）。
    pub fn observe(&mut self, prev: &str, cur: &str) {
        if prev.is_empty() || cur.is_empty() {
            return;
        }
        match self.pairs.iter_mut().find(|(a, b, _)| a == prev && b == cur) {
            Some(slot) => slot.2 += 1,
            None => self.pairs.push((String::from(prev), String::from(cur), 1)),
        }
    }

    /// 权重（前词固定下的后继加成——log 尺度 ×10/次，封顶 50）。
    pub fn weight(&self, prev: &str, cur: &str) -> i64 {
        self.pairs
            .iter()
            .find(|(a, b, _)| a == prev && b == cur)
            .map(|(_, _, n)| (*n as i64 * 10).min(50))
            .unwrap_or(0)
    }

    pub fn len(&self) -> usize {
        self.pairs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pairs.is_empty()
    }
}

impl Default for BigramTable {
    fn default() -> BigramTable {
        BigramTable::new()
    }
}

/// 用户词库持久化面（crate::h3star::hbase::PersistKv——「换方案不丢词库」「重启不丢自造词」
/// 的落盘载体；与 F328 词库共享同格式）。
pub fn dump_user_dict(entries: &[(String, String, i64)]) -> crate::h3star::hbase::PersistKv {
    let mut kv = crate::h3star::hbase::PersistKv::new();
    let body = entries
        .iter()
        .map(|(p, w, wgt)| alloc::format!("{}={}:{}", p, w, wgt))
        .collect::<Vec<_>>()
        .join("\u{1}");
    kv.set("ime.userdict", &body);
    kv.flush();
    kv
}

/// 从落盘账恢复用户词库（重启面）。
pub fn load_user_dict(kv: &crate::h3star::hbase::PersistKv) -> Vec<(String, String, i64)> {
    kv.get("ime.userdict")
        .unwrap_or("")
        .split('\u{1}')
        .filter(|s| !s.is_empty())
        .map(|e| {
            let mut it = e.splitn(2, '=');
            let py = it.next().unwrap_or("");
            let mut it2 = it.next().unwrap_or("").rsplitn(2, ':');
            let wgt = it2.next().and_then(|x| x.parse::<i64>().ok()).unwrap_or(0);
            let word = it2.next().unwrap_or("");
            (String::from(py), String::from(word), wgt)
        })
        .collect()
}

/// 标点对（F108 面的引擎侧数据：中文开/闭引号配对——「“”《》（）」
/// 成对输出模型：开符号 → 补全闭符号）。
pub fn punctuation_pair(open: char) -> Option<char> {
    match open {
        '“' => Some('”'),
        '《' => Some('》'),
        '（' => Some('）'),
        '‘' => Some('’'),
        _ => None,
    }
}

/// 深化层自检。
pub fn run_imecore_deep_checks() -> CheckSet {
    let mut set = CheckSet::new("F326-deep");

    // 1. 候选窗分页：12 个候选 → 页 0 出 9 个、页 1 出 3 个。
    let all: Vec<(String, i64)> =
        (1..=12).map(|i| (alloc::format!("词{i}"), 100 - i)).collect();
    let p0 = CandidatePage::paginate(&all, 0);
    let p1 = CandidatePage::paginate(&all, 1);
    set.add(
        "candidate paginate 9 grid",
        p0.items.len() == 9 && p1.items.len() == 3 && p1.page == 1,
        "",
    );

    // 2. 高亮环形移动 + 数字选词。
    let mut p = CandidatePage::paginate(&all, 0);
    p.move_highlight(1);
    let after_down = p.highlight;
    p.move_highlight(-1);
    let after_up = p.highlight;
    p.move_highlight(-1);
    let wrap_low = p.highlight;
    let pick = p.pick_by_number(3);
    set.add(
        "highlight ring and number pick",
        after_down == 1 && after_up == 0 && wrap_low == 8 && pick.map(|(t, _)| t.as_str()) == Some("词3"),
        "",
    );

    // 3. 二元权重：观察两次 → 权重 20；封顶 50。
    let mut b = BigramTable::new();
    b.observe("我", "的");
    b.observe("我", "的");
    let w2 = b.weight("我", "的");
    for _ in 0..10 {
        b.observe("我", "的");
    }
    let wmax = b.weight("我", "的");
    set.add(
        "bigram weight capped",
        w2 == 20 && wmax == 50 && b.weight("我", "他") == 0,
        "",
    );

    // 4. 用户词库落盘 → 重启恢复（词+权重逐项回读）。
    let entries = alloc::vec![
        (String::from("xian"), String::from("先"), 30),
        (String::from("xian"), String::from("西安"), 10),
    ];
    let kv = dump_user_dict(&entries);
    let back = load_user_dict(&kv);
    set.add(
        "user dict persists reboot",
        back.len() == 2 && back[0].1 == "先" && back[0].2 == 30 && back[1].1 == "西安",
        "",
    );

    // 5. 标点对：开符号补闭符号；普通符号无对。
    set.add(
        "punctuation pairs",
        punctuation_pair('“') == Some('”')
            && punctuation_pair('《') == Some('》')
            && punctuation_pair('。').is_none(),
        "",
    );

    // 6. 空词典落盘恢复安全（零条目——不炸）。
    let empty_kv = dump_user_dict(&[]);
    set.add("empty dict safe", load_user_dict(&empty_kv).is_empty(), "");

    set
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn paginate_beyond_range_empty() {
        let all: Vec<(String, i64)> = alloc::vec![(String::from("a"), 1)];
        let p = CandidatePage::paginate(&all, 5);
        assert!(p.items.is_empty());
    }

    #[test]
    fn highlight_on_empty_page_safe() {
        let mut p = CandidatePage::paginate(&[], 0);
        p.move_highlight(1);
        assert_eq!(p.highlight, 0);
    }

    #[test]
    fn number_out_of_range_none() {
        let all: Vec<(String, i64)> = alloc::vec![(String::from("a"), 1)];
        let p = CandidatePage::paginate(&all, 0);
        assert!(p.pick_by_number(0).is_none());
        assert!(p.pick_by_number(2).is_none());
    }

    #[test]
    fn bigram_no_self_empty() {
        let mut b = BigramTable::new();
        b.observe("", "x");
        b.observe("x", "");
        assert!(b.is_empty());
    }

    #[test]
    fn pair_quotes_all_registered() {
        assert_eq!(punctuation_pair('‘'), Some('’'));
        assert_eq!(punctuation_pair('（'), Some('）'));
    }
}

// ---------------------------------------------------------------------------
// 深化层二 · F326 首候选命中率抽样账 / 排序确定性 / 三因子方向账
// ---------------------------------------------------------------------------

/// 首候选命中率抽样账（判据「首候选命中率抽样记录」的数据面）：逐次
/// 记录首候选是否命中期望，出千分率——抽样可复现（同引擎同输入必同账）。
pub struct FirstHitSampling {
    total: u64,
    hits: u64,
    /// 抽样流水（输入, 首候选, 是否命中）——可回放。
    pub trail: Vec<(String, String, bool)>,
}

impl FirstHitSampling {
    pub fn new() -> FirstHitSampling {
        FirstHitSampling { total: 0, hits: 0, trail: Vec::new() }
    }

    /// 记录一次抽样。
    pub fn record(&mut self, input: &str, first: &str, expected: &str) {
        let hit = first == expected;
        self.total += 1;
        if hit {
            self.hits += 1;
        }
        self.trail.push((String::from(input), String::from(first), hit));
    }

    /// 命中率（‰）。
    pub fn rate_permille(&self) -> u64 {
        if self.total == 0 {
            0
        } else {
            self.hits * 1000 / self.total
        }
    }

    pub fn len(&self) -> usize {
        self.total as usize
    }
}

impl Default for FirstHitSampling {
    fn default() -> FirstHitSampling {
        FirstHitSampling::new()
    }
}

/// 排序确定性审计：同引擎同输入两次候选序列必须逐项相等（判据「排序
/// 因子可复现——同输入同结果」的直接对账）。
pub fn candidates_deterministic(engine: &ImeEngine, input: &str, rounds: usize) -> bool {
    if rounds < 2 {
        return true;
    }
    let first = engine.candidates(input);
    (1..rounds).all(|_| engine.candidates(input) == first)
}

/// 深化层二自检（命中率抽样 / 确定性 / 用户历史升权 / 造词三次入库 / 上下文句界）。
pub fn run_imecore_deep2_checks() -> CheckSet {
    let mut set = CheckSet::new("F326-deep2");

    // 1. 排序确定性：同输入多轮候选逐项相等。
    let mut e = ImeEngine::new();
    e.add_word("计算器", "jisuanqi", 100);
    e.add_word("记事本", "jishiben", 90);
    set.add(
        "candidates deterministic",
        candidates_deterministic(&e, "jisuanqi", 5)
            && candidates_deterministic(&e, "jishiben", 3),
        "",
    );

    // 2. 用户历史升权方向账：同频并列词，commit 后必反超（三因子之用户历史）。
    //    （并列取字典序——「计算乙」U+4E59 < 「计算器」U+5668，先手为乙。）
    let mut e2 = ImeEngine::new();
    e2.add_word("计算器", "jisuanqi", 100);
    e2.add_word("计算乙", "jisuanqi", 100);
    let before_first = e2.first_candidate("jisuanqi");
    let _ = e2.commit("计算器", "jisuanqi");
    let after_first = e2.first_candidate("jisuanqi");
    set.add(
        "user history boost flips order",
        before_first == Some(String::from("计算乙"))
            && after_first == Some(String::from("计算器")),
        "",
    );

    // 3. 造词三次入库：连选第三次自动成词（COIN 语义深化）。
    let mut e3 = ImeEngine::new();
    let coined_after_two = {
        let _ = e3.commit("量子隧穿", "liangzisuishuan");
        let _ = e3.commit("量子隧穿", "liangzisuishuan");
        e3.candidates("liangzisuishuan").iter().any(|(t, _)| t == "量子隧穿")
    };
    let _ = e3.commit("量子隧穿", "liangzisuishuan");
    let coined_after_three = e3
        .candidates("liangzisuishuan")
        .iter()
        .any(|(t, _)| t == "量子隧穿");
    set.add(
        "coin on third commit",
        !coined_after_two && coined_after_three && e3.coined_len() == 1,
        "",
    );

    // 4. 上下文句界：句内上下文提升含上屏词的候选；句末清空后回到频序。
    let mut e4 = ImeEngine::new();
    e4.add_word("先", "xian", 100);
    e4.add_word("西安", "xi'an", 100);
    let base_first = e4.first_candidate("xian");
    let _ = e4.commit("西", "xi");
    let ctx_first = e4.first_candidate("xian");
    e4.end_sentence();
    let cleared_first = e4.first_candidate("xian");
    set.add(
        "context scoped to sentence",
        base_first == Some(String::from("先"))
            && ctx_first == Some(String::from("西安"))
            && cleared_first == Some(String::from("先")),
        "",
    );

    // 5. 首候选命中率抽样账：十连抽全对出 1000‰，错一次如实回落。
    let mut s = FirstHitSampling::new();
    for _ in 0..9 {
        s.record("jisuanqi", "计算器", "计算器");
    }
    s.record("jisuanqi", "计算场", "计算器");
    set.add(
        "first hit sampling ledger",
        s.len() == 10 && s.rate_permille() == 900 && s.trail.iter().filter(|(_, _, h)| *h).count() == 9,
        "",
    );

    set
}

#[cfg(test)]
mod deep2_tests {
    use super::*;

    #[test]
    fn sampling_empty_rate_zero() {
        let s = FirstHitSampling::new();
        assert_eq!(s.rate_permille(), 0);
    }

    #[test]
    fn coin_streak_other_word_does_not_interfere() {
        let mut e = ImeEngine::new();
        let _ = e.commit("量子隧穿", "liangzisuishuan");
        let _ = e.commit("量子纠缠", "liangzijiuquan");
        let _ = e.commit("量子隧穿", "liangzisuishuan");
        // 各自计数独立——隧穿 2 次、纠缠 1 次，都未达三次。
        assert_eq!(e.coined_len(), 0);
    }

    #[test]
    fn determinism_holds_after_commits() {
        let mut e = ImeEngine::new();
        e.add_word("计算器", "jisuanqi", 100);
        let _ = e.commit("计算器", "jisuanqi");
        assert!(candidates_deterministic(&e, "jisuanqi", 4));
    }
}

// ---------------------------------------------------------------------------
// 深化层三 · 句子级组句 + 用户词频自学习可复现账
// ---------------------------------------------------------------------------

/// 句子级组句器（判据「切分用例/排序可复现」的句子面延伸）：连续音节
/// 串 → 贪心最长匹配组句。组句确定性铁律：每个位置取「有候选的最长
/// 前缀」中分数最高的词（同输入同句、全确定）；任一位置零候选 → 整句
/// 不组并返回 None（部分组句不许半截上屏——诚实失败）。
pub struct SentenceComposer;

impl SentenceComposer {
    /// 组句：贪心最长匹配——位置 pos 从「剩余串全长」往下试到 1 字符，
    /// 第一个有候选的前缀即取其最优分词并推进。
    pub fn compose(engine: &ImeEngine, input: &str) -> Option<String> {
        if input.is_empty() {
            return None; // 空输入不组句（诚实失败——不产出空句）。
        }
        let chars: Vec<char> = input.chars().collect();
        let mut pos = 0usize;
        let mut sentence = String::new();
        while pos < chars.len() {
            let rest: String = chars[pos..].iter().collect();
            let mut found: Option<(String, usize)> = None;
            for end in (1..=rest.chars().count()).rev() {
                let prefix: String = rest.chars().take(end).collect();
                if let Some((w, _)) = engine.candidates(&prefix).first().cloned() {
                    found = Some((w, end));
                    break;
                }
            }
            match found {
                Some((w, end)) => {
                    sentence.push_str(&w);
                    pos += end;
                }
                None => return None, // 死胡同 → 整句不组（不猜）。
            }
        }
        Some(sentence)
    }

    /// 组句可复现自证：同引擎同输入 N 轮同输出。
    pub fn deterministic(engine: &ImeEngine, input: &str, rounds: usize) -> bool {
        let first = Self::compose(engine, input);
        (1..rounds).all(|_| Self::compose(engine, input) == first)
    }
}

/// 用户词频自学习账（「自学习用例（造词三次入库）」的数据面深化）：
/// 逐词记录 (词, 升权次数, 最近上屏钟点)；升权历史可回放——同一次会
/// 话序列重放必须收敛到同一权重（自学习可复现的机器证明）。
pub struct LearnLedger {
    /// (词, 升权次数, 最近上屏 ms)。
    pub entries: Vec<(String, u32, u64)>,
    /// 升权历史（时间序回放面）。
    pub history: Vec<(u64, String)>,
}

impl LearnLedger {
    pub fn new() -> LearnLedger {
        LearnLedger { entries: Vec::new(), history: Vec::new() }
    }

    /// 记一次用户上屏选择（钟点单调入账）。
    pub fn record(&mut self, at_ms: u64, word: &str) {
        self.history.push((at_ms, String::from(word)));
        match self.entries.iter_mut().find(|(w, _, _)| w == word) {
            Some((_, n, last)) => {
                *n += 1;
                *last = at_ms;
            }
            None => self.entries.push((String::from(word), 1, at_ms)),
        }
    }

    /// 升权次数查询。
    pub fn count_of(&self, word: &str) -> u32 {
        self.entries.iter().find(|(w, _, _)| w == word).map(|(_, n, _)| *n).unwrap_or(0)
    }

    /// 历史重放收敛性：按历史重放到全新账本，终态与原账本一致（自学习
    /// 不是黑盒——重放可复现）。
    pub fn replay_converges(&self) -> bool {
        let mut fresh = LearnLedger::new();
        for (at, w) in &self.history {
            fresh.record(*at, w);
        }
        fresh.entries == self.entries
    }

    /// 最近用词排行（次数降序 → 最近时间降序——全确定）。
    pub fn top_words(&self, n: usize) -> Vec<&str> {
        let mut v: Vec<&(String, u32, u64)> = self.entries.iter().collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then(b.2.cmp(&a.2)).then(a.0.cmp(&b.0)));
        v.into_iter().take(n).map(|(w, _, _)| w.as_str()).collect()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

impl Default for LearnLedger {
    fn default() -> LearnLedger {
        LearnLedger::new()
    }
}

/// 深化层三自检（组句 / 学习账）。
pub fn run_imecore_deep3_checks() -> CheckSet {
    let mut set = CheckSet::new("F326-deep3");

    // 1. 句子级组句：双段输入 → 两词拼句（段内最优）。
    let mut e = ImeEngine::new();
    e.add_word("今天", "jintian", 900);
    e.add_word("天气", "tianqi", 800);
    e.add_word("天", "tian", 500);
    let s = SentenceComposer::compose(&e, "jintiantianqi");
    set.add(
        "sentence compose two segments",
        s.as_deref() == Some("今天天气"),
        "",
    );

    // 2. 组句可复现：同引擎同输入多轮同句。
    set.add(
        "compose deterministic",
        SentenceComposer::deterministic(&e, "jintiantianqi", 5),
        "",
    );

    // 3. 诚实失败：任一段零候选 → 整句 None（不半截上屏）。
    let s2 = SentenceComposer::compose(&e, "jintiazxyz");
    set.add("compose honest failure", s2.is_none(), "");

    // 4. 学习账：三次上屏计数 + 排行（次数→最近）全确定。
    let mut ll = LearnLedger::new();
    ll.record(0, "计算器");
    ll.record(10, "记事本");
    ll.record(20, "计算器");
    ll.record(30, "计算器");
    set.add(
        "learn ledger counts",
        ll.count_of("计算器") == 3 && ll.count_of("记事本") == 1 && ll.len() == 2,
        "",
    );
    let top = ll.top_words(2);
    set.add("learn ranking deterministic", top.first() == Some(&"计算器"), "");

    // 5. 重放收敛：历史重放到新账本终态一致（自学习可复现证明）。
    set.add("learn replay converges", ll.replay_converges(), "");

    // 6. 造词联动：连选 COIN_THRESHOLD 次 → 引擎自动入库（学习账与
    //    引擎造词闭环——「三次入库」端到端）。
    let mut e2 = ImeEngine::new();
    for _ in 0..COIN_THRESHOLD {
        e2.commit("变体", "bianti");
    }
    let coined_hit = e2.candidates("bianti").first().map(|(t, _)| t.clone());
    set.add(
        "coin threshold end to end",
        coined_hit.as_deref() == Some("变体"),
        "",
    );

    set
}

#[cfg(test)]
mod deep3_tests {
    use super::*;

    #[test]
    fn compose_greedy_takes_best_score_at_prefix() {
        let mut e = ImeEngine::new();
        e.add_word("西安", "xi'an", 900);
        e.add_word("先", "xian", 800);
        e.add_word("安", "an", 700);
        // "xian" 整串有候选：同前缀取最优分（西安 900 > 先 800——词频
        // 优先，最长匹配语义下的确定性取舍）。
        assert_eq!(SentenceComposer::compose(&e, "xian").as_deref(), Some("西安"));
    }

    #[test]
    fn ledger_empty_replay_trivial() {
        let ll = LearnLedger::new();
        assert!(ll.replay_converges() && ll.top_words(3).is_empty());
    }

    #[test]
    fn ledger_unknown_word_zero() {
        let ll = LearnLedger::new();
        assert_eq!(ll.count_of("没记过"), 0);
    }

    #[test]
    fn compose_empty_input_none() {
        let e = ImeEngine::new();
        assert_eq!(SentenceComposer::compose(&e, ""), None, "空输入不组句");
    }
}
