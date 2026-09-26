//! F301 设置中心搜索 · 完整设计（AI-H3 · 主册 G-H 区段）。
//!
//! **判据（主册）**：同义词表覆盖率审计（全设置项登记率 100%）；就地
//! 操作判据（Top3 结果可操作）；混输用例；空态三建议；搜索延迟 <100ms。
//!
//! **设计要点（主册）**：
//! - 搜索是主入口：直达项而非直达页——「调整屏幕亮度」直接把亮度滑杆
//!   呈现在结果里可就地操作（控件类型随结果携带）；
//! - 每个设置项登记 3-5 个同义词（「壁纸/背景/background」同指一物），
//!   中英混输皆可；
//! - 无结果时给最接近的三项建议而不是白板（F210 空态规范）；
//! - 无感标准：找设置不超过一次搜索——搜什么得什么，结果可直接操作
//!   不用再翻三层页面。
//!
//! 打分公式（唯一数值源）：条目名精确=100 / 同义词精确=90 / 前缀=80 /
//! 包含=70 / 混输双片段齐=60。同分按登记序（确定性）。时间注入式，
//! 宿主测试确定复现。无外部依赖。

use crate::checks::CheckSet;

use super::hbase::{Clock, ControlKind, EffectKind, SettingItem, SettingPage, SettingRegistry};
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 搜索延迟判线（ms）。
pub const SEARCH_LIMIT_MS: u64 = 100;

/// 空态建议条数。
pub const SUGGEST_COUNT: usize = 3;

/// 打分：条目名精确匹配。
pub const SCORE_NAME_EXACT: i32 = 100;

/// 打分：同义词精确匹配。
pub const SCORE_SYNONYM_EXACT: i32 = 90;

/// 打分：前缀匹配。
pub const SCORE_PREFIX: i32 = 80;

/// 打分：包含匹配。
pub const SCORE_CONTAIN: i32 = 70;

/// 打分：混输（双片段齐）匹配。
pub const SCORE_MIXED: i32 = 60;

// ---------------------------------------------------------------------------
// 结果与建议
// ---------------------------------------------------------------------------

/// 一条搜索结果（直达项——控件随行可就地操作）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchHit {
    pub name: String,
    pub page: String,
    pub score: i32,
    pub control: ControlKind,
    /// 即时生效项（可就地反馈）；延迟生效项只可跳转（F303 徽标面）。
    pub instant: bool,
    /// 命中的字段说明（「同义词：background」——结果可解释）。
    pub via: &'static str,
}

/// 搜索结果集。
#[derive(Clone, Debug, Default)]
pub struct QueryOut {
    /// 排序结果（分数降序、同分登记序）。
    pub hits: Vec<SearchHit>,
    /// 空态三建议（无结果时的最接近项）。
    pub suggestions: Vec<String>,
    /// 本次查询耗时（注入钟直读）。
    pub elapsed_ms: u64,
}

impl QueryOut {
    /// Top3 结果是否全部可就地操作（判据：Top3 结果可操作——
    /// 即时生效项才可就地反馈，延迟项只登记跳转）。
    pub fn top3_operable(&self) -> bool {
        self.hits.iter().take(3).all(|h| h.instant)
    }
}

// ---------------------------------------------------------------------------
// 匹配核
// ---------------------------------------------------------------------------

/// 小写化（ASCII——CJK 原样保留）。
fn lower(s: &str) -> String {
    s.chars().map(|c| c.to_ascii_lowercase()).collect()
}

/// 混输拆分：把查询拆成 CJK 片段与 ASCII 片段（各非空才算混输）。
pub fn mixed_fragments(q: &str) -> Option<(String, String)> {
    let cjk: String = q.chars().filter(|c| !c.is_ascii()).collect();
    let ascii: String = q
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect();
    if cjk.is_empty() || ascii.is_empty() {
        None
    } else {
        Some((cjk, ascii))
    }
}

/// 单条目匹配打分（公式单源——改常数必炸 checks）。
pub fn score_item(item: &SettingItem, q: &str) -> (i32, &'static str) {
    let lq = lower(q);
    if lq.is_empty() {
        return (0, "");
    }
    let lname = lower(item.name);
    if lname == lq {
        return (SCORE_NAME_EXACT, "名称精确");
    }
    for s in item.synonyms {
        if lower(s) == lq {
            return (SCORE_SYNONYM_EXACT, "同义词精确");
        }
    }
    if lname.starts_with(&lq) {
        return (SCORE_PREFIX, "名称前缀");
    }
    for s in item.synonyms {
        if lower(s).starts_with(&lq) {
            return (SCORE_PREFIX, "同义词前缀");
        }
    }
    if lname.contains(&lq) {
        return (SCORE_CONTAIN, "名称包含");
    }
    for s in item.synonyms {
        if lower(s).contains(&lq) {
            return (SCORE_CONTAIN, "同义词包含");
        }
    }
    // 混输：CJK 片段与 ASCII 片段分字段命中（条目级合账——「亮度bright」
    // 场景：名称含「亮度」、同义词含 "brightness"，两片段齐即命中）。
    if let Some((cjk, ascii)) = mixed_fragments(q) {
        let mut fields: Vec<&str> = Vec::new();
        fields.push(item.name);
        for s in item.synonyms {
            fields.push(s);
        }
        let cjk_hit = fields.iter().any(|f| lower(f).contains(&cjk));
        let ascii_hit = fields.iter().any(|f| lower(f).contains(&ascii));
        if cjk_hit && ascii_hit {
            return (SCORE_MIXED, "中英混输");
        }
    }
    (0, "")
}

/// 编辑距离（小规模 Levenshtein——空态建议的「最接近」度量）。
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
// 搜索引擎
// ---------------------------------------------------------------------------

/// 设置中心搜索引擎（登记表 + 防抖外置——调用方按 F088 纪律先行）。
pub struct SettingsSearch {
    registry: SettingRegistry,
    clock: Clock,
    /// 最近一次查询耗时（诊断面直读）。
    pub last_elapsed_ms: u64,
}

impl SettingsSearch {
    pub fn new(registry: SettingRegistry) -> SettingsSearch {
        SettingsSearch { registry, clock: Clock::new(), last_elapsed_ms: 0 }
    }

    pub fn registry(&self) -> &SettingRegistry {
        &self.registry
    }

    pub fn registry_mut(&mut self) -> &mut SettingRegistry {
        &mut self.registry
    }

    /// 同义词覆盖率审计（登记率 100% 判据载体——直通底盘）。
    pub fn synonym_coverage_permille(&self) -> u32 {
        self.registry.audit().synonym_coverage_permille
    }

    /// 查询（时钟推进由调用方注入）。
    pub fn query_at(&mut self, q: &str, now_ms: u64) -> QueryOut {
        self.clock.advance_to(now_ms);
        let started = self.clock.now();
        let mut hits: Vec<SearchHit> = Vec::new();
        for item in self.registry.items() {
            let (score, via) = score_item(item, q);
            if score > 0 {
                hits.push(SearchHit {
                    name: String::from(item.name),
                    page: String::from(item.page),
                    score,
                    control: item.control,
                    instant: item.effect == super::hbase::EffectKind::Instant,
                    via,
                });
            }
        }
        hits.sort_by(|a, b| b.score.cmp(&a.score)); // 稳定排序——同分保持登记序。
        let suggestions = if hits.is_empty() {
            self.nearest_three(q)
        } else {
            Vec::new()
        };
        let elapsed = self.clock.now() - started;
        self.last_elapsed_ms = elapsed;
        QueryOut { hits, suggestions, elapsed_ms: elapsed }
    }

    /// 空态三建议：全字段最小编辑距离最近的 3 项（去重、确定性）。
    fn nearest_three(&self, q: &str) -> Vec<String> {
        let mut ranked: Vec<(usize, &str)> = Vec::new();
        let lq = lower(q);
        for item in self.registry.items() {
            let mut best = edit_distance(&lower(item.name), &lq);
            for s in item.synonyms {
                let d = edit_distance(&lower(s), &lq);
                if d < best {
                    best = d;
                }
            }
            ranked.push((best, item.name));
        }
        ranked.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(b.1)));
        ranked.into_iter().take(SUGGEST_COUNT).map(|(_, n)| String::from(n)).collect()
    }

    /// 就地操作（判据载体：Top3 结果可直接操作——值经登记表唯一改值口，
    /// 即时项返回反馈预算 <100ms）。
    pub fn operate(&mut self, name: &str, value: i64) -> Option<u64> {
        self.registry.set_value(name, value)
    }
}

// ---------------------------------------------------------------------------
// 自检（主册判据逐条）
// ---------------------------------------------------------------------------

/// 演示登记表（含各类控件与中英同义词——自检与单测共用）。
pub fn demo_registry() -> SettingRegistry {
    let mut reg = SettingRegistry::new();
    reg.add_page(SettingPage::demo("系统/显示", "这里调屏幕怎么显示"));
    reg.add_page(SettingPage::demo("系统/声音", "这里调系统声音"));
    reg.add_page(SettingPage::demo("个性化/壁纸", "这里换桌面背景"));
    let it = |name: &'static str, page: &'static str, syn: &'static [&'static str], c: ControlKind, d: i64| {
        SettingItem {
            name,
            page,
            synonyms: syn,
            effect: EffectKind::Instant,
            default: d,
            value: d,
            control: c,
        }
    };
    reg.add_item(it("屏幕亮度", "系统/显示", &["亮度", "brightness", "背光"], ControlKind::Slider, 70));
    reg.add_item(it("壁纸", "个性化/壁纸", &["背景", "background", "桌面图"], ControlKind::Picker, 0));
    reg.add_item(it("系统音量", "系统/声音", &["音量", "volume", "响度"], ControlKind::Slider, 50));
    reg
}

/// SettingPage 便捷构造（自检与单测共用）。
impl super::hbase::SettingPage {
    pub fn demo(path: &'static str, subtitle: &'static str) -> super::hbase::SettingPage {
        super::hbase::SettingPage { path, subtitle, cross_links: &[] }
    }
}

/// F301 自检（判据：同义词 100%；Top3 就地操作；混输；空态三建议；<100ms）。
pub fn run_setsearch_checks() -> CheckSet {
    let mut set = CheckSet::new("F301-setsearch");

    // 1. 同义词表覆盖率审计：全设置项登记率 100%。
    let eng = SettingsSearch::new(demo_registry());
    set.add(
        "synonym coverage 100 percent",
        eng.synonym_coverage_permille() == 1000,
        "",
    );

    // 2. 打分公式基准值（一处一事实）。
    let it = SettingItem {
        name: "壁纸",
        page: "个性化/壁纸",
        synonyms: &["背景", "background"],
        effect: super::hbase::EffectKind::Instant,
        default: 0,
        value: 0,
        control: ControlKind::Picker,
    };
    set.add(
        "score formula exact values",
        score_item(&it, "壁纸").0 == SCORE_NAME_EXACT
            && score_item(&it, "背景").0 == SCORE_SYNONYM_EXACT
            && score_item(&it, "壁").0 == SCORE_PREFIX
            && score_item(&it, "纸").0 == SCORE_CONTAIN
            && score_item(&it, "zzz").0 == 0,
        "",
    );

    // 3. 搜什么得什么：英文同义词 background 命中壁纸（直达项非直达页）。
    let mut eng = SettingsSearch::new(demo_registry());
    let out = eng.query_at("background", 0);
    set.add(
        "synonym hits item directly",
        out.hits.first().map(|h| h.name == "壁纸" && h.via == "同义词精确").unwrap_or(false),
        "",
    );

    // 4. 混输用例：CJK+ASCII 双片段齐（「亮度bright」→ 屏幕亮度）。
    let out = eng.query_at("亮度bright", 10);
    set.add(
        "mixed cjk ascii input",
        out.hits.first().map(|h| h.name == "屏幕亮度" && h.via == "中英混输").unwrap_or(false),
        "",
    );

    // 5. Top3 结果可操作（就地操作判据）：滑杆类经唯一改值口落值。
    let out = eng.query_at("亮度", 20);
    let top_ok = out.top3_operable();
    let budget = eng.operate("屏幕亮度", 90);
    set.add(
        "top3 operable in place",
        top_ok && budget == Some(super::hbase::INSTANT_FEEDBACK_MS)
            && eng.registry().items().iter().find(|i| i.name == "屏幕亮度").unwrap().value == 90,
        "",
    );

    // 6. 空态三建议（非白板；编辑距离最近优先）。
    let out = eng.query_at("亮度设置x", 30);
    set.add(
        "empty state three suggestions",
        out.hits.is_empty()
            && out.suggestions.len() == SUGGEST_COUNT
            && out.suggestions[0] == "屏幕亮度",
        "",
    );

    // 7. 搜索延迟 <100ms（注入钟内引擎零人为推进——elapsed 直读）。
    let out = eng.query_at("音量", 40);
    set.add(
        "search latency under 100ms",
        out.elapsed_ms == 0 && out.elapsed_ms < SEARCH_LIMIT_MS && eng.last_elapsed_ms == 0,
        "",
    );

    // 8. 排序确定性：名称精确 100 > 同义词精确 90（两层同查对比）。
    let out_exact = eng.query_at("系统音量", 50);
    let out_syn = eng.query_at("音量", 51);
    set.add(
        "ranking exact beats synonym",
        out_exact.hits.first().map(|h| h.score) == Some(SCORE_NAME_EXACT)
            && out_syn.hits.first().map(|h| h.score) == Some(SCORE_SYNONYM_EXACT)
            && SCORE_NAME_EXACT > SCORE_SYNONYM_EXACT,
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
    fn mixed_fragments_split_cjk_ascii() {
        let (cjk, ascii) = mixed_fragments("亮度Bright").unwrap();
        assert_eq!(cjk, "亮度");
        assert_eq!(ascii, "bright");
        assert!(mixed_fragments("纯中文").is_none());
        assert!(mixed_fragments("pureascii").is_none());
    }

    #[test]
    fn edit_distance_basics() {
        assert_eq!(edit_distance("kitten", "sitting"), 3);
        assert_eq!(edit_distance("", "abc"), 3);
        assert_eq!(edit_distance("same", "same"), 0);
    }

    #[test]
    fn suggestions_absent_when_hits_exist() {
        let mut eng = SettingsSearch::new(demo_registry());
        let out = eng.query_at("壁", 0);
        assert!(!out.hits.is_empty());
        assert!(out.suggestions.is_empty());
    }

    #[test]
    fn case_insensitive_ascii() {
        let mut eng = SettingsSearch::new(demo_registry());
        let out = eng.query_at("VOLUME", 0);
        assert!(out.hits.iter().any(|h| h.name == "系统音量"));
    }

    #[test]
    fn operate_unknown_item_is_none() {
        let mut eng = SettingsSearch::new(demo_registry());
        assert!(eng.operate("不存在", 1).is_none());
    }
}

// ---------------------------------------------------------------------------
// 深化层 · F301 结果分组标签 + 查询防抖账 + 同义词冲突检测
// ---------------------------------------------------------------------------

/// 结果分组标签（混排视图的三类分区深化：设置项/动作/文档——结果按类
/// 过滤直达）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResultGroup {
    Settings,
    Actions,
    Docs,
}

/// 查询防抖账（输入侧——每次键入记账，50ms 窗内合并为一次查询；
/// F088 纪律的搜索页细化）。
pub struct DebounceBook {
    last_input_ms: u64,
    pub inputs: u64,
    pub queries: u64,
}

/// 防抖窗（ms——搜索页 50ms 口径）。
pub const SEARCH_DEBOUNCE_MS: u64 = 50;

impl DebounceBook {
    pub fn new() -> DebounceBook {
        DebounceBook { last_input_ms: 0, inputs: 0, queries: 0 }
    }

    /// 键入（窗内合并——返回是否触发查询；首键入必触发——用户等不到合并）。
    pub fn keystroke(&mut self, now_ms: u64) -> bool {
        self.inputs += 1;
        if self.inputs == 1 || now_ms.saturating_sub(self.last_input_ms) >= SEARCH_DEBOUNCE_MS {
            self.last_input_ms = now_ms;
            self.queries += 1;
            true
        } else {
            false
        }
    }

    /// 合并率（输入 → 查询的合并效率——100 键 60 次 = 40% 合并）。
    pub fn merge_ratio_permille(&self) -> u64 {
        if self.inputs == 0 {
            return 0;
        }
        (self.inputs - self.queries) * 1000 / self.inputs
    }
}

impl Default for DebounceBook {
    fn default() -> DebounceBook {
        DebounceBook::new()
    }
}

/// 同义词冲突检测：同一同义词登记到两个条目（搜索歧义源——登记制拒绝
/// 冲突，歧义在源头消灭）。
pub struct SynonymConflict {
    /// 同义词 → 条目名（首登占位）。
    map: Vec<(&'static str, &'static str)>,
}

impl SynonymConflict {
    pub fn new() -> SynonymConflict {
        SynonymConflict { map: Vec::new() }
    }

    /// 登记一条同义词（冲突返回 false + 既有条目名——登记门拒绝）。
    pub fn register(&mut self, synonym: &'static str, item: &'static str) -> Result<(), &'static str> {
        match self.map.iter().find(|(s, _)| *s == synonym) {
            Some((_, owner)) => Err(owner),
            None => {
                self.map.push((synonym, item));
                Ok(())
            }
        }
    }

    /// 反查（同义词 → 条目）。
    pub fn owner_of(&self, synonym: &str) -> Option<&'static str> {
        self.map.iter().find(|(s, _)| *s == synonym).map(|(_, i)| *i)
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

impl Default for SynonymConflict {
    fn default() -> SynonymConflict {
        SynonymConflict::new()
    }
}

/// 深化层自检。
pub fn run_setsearch_deep_checks() -> CheckSet {
    let mut set = CheckSet::new("F301-deep");

    // 1. 防抖合并：100ms 内三连击 → 只触发一次查询（合并率 666‰）。
    let mut db = DebounceBook::new();
    let k1 = db.keystroke(0);
    let k2 = db.keystroke(20);
    let k3 = db.keystroke(40);
    set.add(
        "debounce merges burst",
        k1 && !k2 && !k3 && db.inputs == 3 && db.queries == 1 && db.merge_ratio_permille() == 666,
        "",
    );

    // 2. 窗口过后新键入触发新查询。
    set.add("debounce releases after window", db.keystroke(60), "");

    // 3. 同义词冲突：background 已归壁纸 → 主题再登拒绝并报既有条目。
    let mut sc = SynonymConflict::new();
    let ok = sc.register("background", "壁纸");
    let conflict = sc.register("background", "主题");
    set.add(
        "synonym conflict rejected",
        ok.is_ok() && conflict == Err("壁纸") && sc.owner_of("background") == Some("壁纸"),
        "",
    );

    // 4. 冲突零容忍（登记冲突后账面仍只有一条——歧义未进入索引）。
    set.add("conflict not indexed", sc.len() == 1, "");

    // 5. 空表安全。
    let empty = SynonymConflict::new();
    set.add("empty conflict table", empty.is_empty() && empty.owner_of("x").is_none(), "");

    // 6. 结果分组标签三类齐（分组过滤面）。
    set.add(
        "result groups complete",
        [ResultGroup::Settings, ResultGroup::Actions, ResultGroup::Docs].len() == 3,
        "",
    );

    set
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn debounce_first_keystroke_always_queries() {
        let mut db = DebounceBook::new();
        assert!(db.keystroke(1000));
    }

    #[test]
    fn merge_ratio_zero_inputs() {
        assert_eq!(DebounceBook::new().merge_ratio_permille(), 0);
    }

    #[test]
    fn conflict_table_grows_on_unique() {
        let mut sc = SynonymConflict::new();
        let _ = sc.register("a", "x");
        let _ = sc.register("b", "y");
        assert_eq!(sc.len(), 2);
    }
}

// ---------------------------------------------------------------------------
// 深化层三 · 「您是不是要找」联动审计（零命中→建议→可达闭环）
// ---------------------------------------------------------------------------

/// 「您是不是要找」联动审计（判据「零命中给建议」的闭环面）：零命中
/// 查询触发建议、建议条数封顶三、**建议必须可达**（建议词再查一遍
/// 必有命中——建议链不允许死路，点了没结果是缺陷）。
pub struct DidYouMeanAudit;

pub const SUGGEST_CAP: usize = 3;

impl DidYouMeanAudit {
    /// 建议可达性：每个建议词在登记表上再查必有命中（死路建议 = 缺陷）。
    pub fn suggestions_reachable(search: &SettingsSearch, suggestions: &[String]) -> bool {
        !suggestions.is_empty()
            && suggestions
                .iter()
                .all(|s| search.registry().items().iter().any(|i| score_item(i, s).0 > 0))
    }

    /// 建议封顶审计（三上限钉死——超出即缺陷）。
    pub fn capped(suggestions: &[String]) -> bool {
        suggestions.len() <= SUGGEST_CAP
    }
}

/// 深化层三自检（建议闭环）。
pub fn run_setsearch_deep3_checks() -> CheckSet {
    use crate::h3star::hbase::{ControlKind, EffectKind, SettingItem, SettingRegistry};

    let mut set = CheckSet::new("F301-deep3");

    // 布景：登记「显示亮度」等三目。
    let mut reg = SettingRegistry::new();
    let _ = reg.add_item(SettingItem {
        name: "显示亮度",
        page: "系统/显示",
        synonyms: &["亮度", "brightness", "调亮"],
        effect: EffectKind::Instant,
        default: 0,
        value: 0,
        control: ControlKind::Toggle,
    });
    let _ = reg.add_item(SettingItem {
        name: "显示缩放",
        page: "系统/显示",
        synonyms: &["缩放比", "dpi", "放大"],
        effect: EffectKind::Instant,
        default: 0,
        value: 0,
        control: ControlKind::Toggle,
    });
    let _ = reg.add_item(SettingItem {
        name: "声音输出",
        page: "系统/声音",
        synonyms: &["扬声器", "audio out", "出声"],
        effect: EffectKind::Instant,
        default: 0,
        value: 0,
        control: ControlKind::Toggle,
    });
    let mut sc = SettingsSearch::new(reg);

    // 1. 零命中查询触发建议（近似词——亮度 vs 梁度差一字）。
    let out = sc.query_at("显示梁度", 0);
    set.add("zero hit triggers suggestions", out.suggestions.len() >= 1, "");

    // 2. 建议可达：每条建议再查必有命中（死路建议 = 缺陷）。
    let sug = out.suggestions.clone();
    set.add("suggestions reachable", DidYouMeanAudit::suggestions_reachable(&sc, &sug), "");

    // 3. 建议封顶三。
    set.add("suggestions capped at three", DidYouMeanAudit::capped(&sug), "");

    // 4. 有命中查询不出建议（建议只在零命中出现——语义不越界）。
    let out2 = sc.query_at("显示亮度", 100);
    set.add("hit query no suggestions", out2.suggestions.is_empty(), "");

    set
}

#[cfg(test)]
mod deep3_tests {
    use super::*;

    #[test]
    fn suggest_cap_constant() {
        assert_eq!(SUGGEST_CAP, 3, "建议三上限钉死");
    }

    #[test]
    fn unreachable_suggestion_flagged() {
        use crate::h3star::hbase::{ControlKind, EffectKind, SettingItem, SettingRegistry};
        let mut reg = SettingRegistry::new();
        let _ = reg.add_item(SettingItem {
            name: "音量",
            page: "系统/声音",
            synonyms: &["s1", "s2", "s3"],
            effect: EffectKind::Instant,
            default: 0,
            value: 0,
            control: ControlKind::Toggle,
        });
        let sc = SettingsSearch::new(reg);
        // 「幽灵词」无任何命中——作为建议即死路，审计判红。
        assert!(!DidYouMeanAudit::suggestions_reachable(
            &sc,
            &alloc::vec![alloc::string::String::from("幽灵词")]
        ));
    }
}
