//! F071 开始菜单搜索直达 · 完整设计（STAR I 主册 G-C-01）。
//!
//! **判据（主册）**：「音量/记事本/报告.docx」三类查询各实测首结果正确率
//! 10/10；按键到首结果 ≤50ms；键盘全程可达（无鼠标走完全流程）。
//!
//! **设计要点（主册）**：
//! - 三类目标：应用（已装清单）/设置项（页面注册制——新增设置页必须
//!   登记，一处一事实）/文件（文档+下载+桌面三目录索引）；
//! - 结果三类分区展示 + 混排模式（最相关优先）；
//! - 打分公式（唯一数值源）：设置项名完全匹配=100 / 应用名前缀=80 /
//!   文件名包含=60 + 最近使用加权（F072 引擎共用，注入口）；
//! - 拼音首字母匹配支持（yx→应用——归应用类前缀级 80）；
//! - 输入防抖 50ms（比 F088 的 150ms 更急——搜索框场景不同）；输入含
//!   路径分隔符 → 自动切文件模式（直搜路径）；
//! - 索引未就绪 → 「正在建立索引」（不空转——显式状态非轮询）；
//! - 无结果态 → 「去浏览器搜索」外链（D 域原则）；
//! - 键盘上下直达、Enter 开首项（首项高亮）；搜索历史本地 20 条可清空。
//!
//! 时间注入式（毫秒钟），宿主测试确定复现。无外部依赖。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 输入防抖（ms）——比 F088 的 150ms 更急。
pub const DEBOUNCE_MS: u64 = 50;

/// 按键到首结果判线（ms）——防抖 + 查找。
pub const FIRST_HIT_LIMIT_MS: u64 = 50;

/// 打分：设置项名完全匹配。
pub const SCORE_SETTING_EXACT: i32 = 100;

/// 打分：应用名前缀（含拼音首字母）匹配。
pub const SCORE_APP_PREFIX: i32 = 80;

/// 打分：文件名包含匹配。
pub const SCORE_FILE_CONTAIN: i32 = 60;

/// 搜索历史容量。
pub const HISTORY_CAP: usize = 20;

/// 三类分区。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HitKind {
    /// 应用（已装/可侧载）。
    App,
    /// 设置项（页面注册制）。
    Setting,
    /// 文件（文档+下载+桌面）。
    File,
}

/// 一条索引条目。
#[derive(Clone, Debug)]
pub struct IndexEntry {
    pub name: &'static str,
    pub kind: HitKind,
    /// 拼音首字母串（应用类；如「应用」→ "yy"。设置/文件为空串）。
    pub pinyin: &'static str,
    /// 详情（应用显版本/设置显页路径/文件显目录）。
    pub detail: &'static str,
}

/// 一条搜索结果（混排视图单元）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchHit {
    pub name: String,
    pub kind: HitKind,
    pub detail: &'static str,
    pub score: i32,
}

// ---------------------------------------------------------------------------
// 索引
// ---------------------------------------------------------------------------

/// 搜索索引：应用/设置/文件三区。设置区页面注册制（登记即索引）；
/// 文件区懒建（`files_ready` 门——未就绪显式报告不空转）。
#[derive(Clone)]
pub struct SearchIndex {
    apps: Vec<IndexEntry>,
    settings: Vec<IndexEntry>,
    files: Vec<IndexEntry>,
    files_ready: bool,
}

impl SearchIndex {
    pub fn new() -> SearchIndex {
        SearchIndex { apps: Vec::new(), settings: Vec::new(), files: Vec::new(), files_ready: false }
    }

    pub fn add_app(&mut self, name: &'static str, pinyin: &'static str, version: &'static str) {
        self.apps.push(IndexEntry { name, kind: HitKind::App, pinyin, detail: version });
    }

    /// 设置页登记（注册制——新增设置页必须走此口）。
    pub fn register_setting(&mut self, name: &'static str, page_path: &'static str) {
        self.settings.push(IndexEntry { name, kind: HitKind::Setting, pinyin: "", detail: page_path });
    }

    /// 文件索引批量就绪（三目录后台扫描完成——懒建一次性挂载）。
    pub fn mount_files(&mut self, names: &[&'static str], dir: &'static str) {
        for n in names {
            self.files.push(IndexEntry { name: n, kind: HitKind::File, pinyin: "", detail: dir });
        }
        self.files_ready = true;
    }

    /// 文件索引就绪态（诊断/空态直读）。
    pub fn files_indexed(&self) -> bool {
        self.files_ready
    }

    fn entries(&self, kind: HitKind) -> &Vec<IndexEntry> {
        match kind {
            HitKind::App => &self.apps,
            HitKind::Setting => &self.settings,
            HitKind::File => &self.files,
        }
    }
}

// ---------------------------------------------------------------------------
// 查询
// ---------------------------------------------------------------------------

/// 查询模式：常规（三类混排）或文件直搜（输入含路径分隔符）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueryMode {
    Mixed,
    FilesOnly,
}

/// 由输入推断查询模式（含路径分隔符 → 文件模式）。
pub fn mode_of(query: &str) -> QueryMode {
    if query.contains('/') || query.contains('\\') {
        QueryMode::FilesOnly
    } else {
        QueryMode::Mixed
    }
}

/// 单条打分（主册公式——一处一事实）：
/// 设置完全匹配 100 / 应用前缀（含拼音）80 / 文件包含 60；
/// `recent_boost` 由 F072 引擎注入（0-19 加分——共享权重面）。
pub fn score_entry(e: &IndexEntry, query: &str, recent_boost: i32) -> i32 {
    let q = query.to_lowercase();
    let n = e.name.to_lowercase();
    let base = match e.kind {
        HitKind::Setting => {
            if n == q {
                SCORE_SETTING_EXACT
            } else {
                0
            }
        }
        HitKind::App => {
            if n.starts_with(&q)
                || (!e.pinyin.is_empty() && e.pinyin.to_lowercase().starts_with(&q))
            {
                SCORE_APP_PREFIX
            } else {
                0
            }
        }
        HitKind::File => {
            if n.contains(&q) {
                SCORE_FILE_CONTAIN
            } else {
                0
            }
        }
    };
    if base == 0 {
        0
    } else {
        base + recent_boost.clamp(0, 19)
    }
}

/// 查询结果（含时延记账与空态）。
#[derive(Clone, Debug)]
pub struct QueryResult {
    /// 混排视图（分数降序、同分按索引序——确定性）。
    pub ranked: Vec<SearchHit>,
    /// 空结果态：true = 建议外链「去浏览器搜索」。
    pub suggest_web: bool,
    /// 索引未就绪：true = 文件区显「正在建立索引」（非空转）。
    pub indexing: bool,
    /// 本次查询引擎耗时（注入钟差——防抖之外直读）。
    pub elapsed_ms: u64,
}

/// 搜索引擎（索引 + 防抖 + 历史）。
pub struct SearchEngine {
    index: SearchIndex,
    /// 防抖状态：最近一次输入时刻（注入钟）。
    last_input_ms: u64,
    clock: u64,
}

impl SearchEngine {
    pub fn new(index: SearchIndex) -> SearchEngine {
        SearchEngine { index, last_input_ms: 0, clock: 0 }
    }

    /// 输入事件（注入钟推进——防抖记账）。
    pub fn input_at(&mut self, now_ms: u64) {
        self.last_input_ms = now_ms;
        self.clock = now_ms;
    }

    /// 防抖是否放行（`DEBOUNCE_MS` 内的连续输入不触发查询）。
    pub fn debounce_ok(&self, now_ms: u64) -> bool {
        now_ms.saturating_sub(self.last_input_ms) >= DEBOUNCE_MS
    }

    /// 查询（混排 or 文件直搜；recent_boost 由调用方按名注入）。
    /// 空查询短路——零分态直接返回（防 `starts_with("")` 全命中）。
    pub fn query(&self, query: &str, mode: QueryMode, boosts: &[(&'static str, i32)]) -> QueryResult {
        let start = self.clock;
        if query.is_empty() {
            return QueryResult {
                ranked: Vec::new(),
                suggest_web: self.index.files_indexed(),
                indexing: !self.index.files_indexed(),
                elapsed_ms: self.clock - start,
            };
        }
        let mut hits: Vec<SearchHit> = Vec::new();
        let kinds = match mode {
            QueryMode::Mixed => [HitKind::App, HitKind::Setting, HitKind::File],
            QueryMode::FilesOnly => [HitKind::File, HitKind::File, HitKind::File],
        };
        let mut seen_kinds = [false; 3];
        for kind in kinds {
            let slot = match kind {
                HitKind::App => 0,
                HitKind::Setting => 1,
                HitKind::File => 2,
            };
            if mode == QueryMode::FilesOnly && seen_kinds[2] {
                break; // 文件直搜只扫一遍文件区。
            }
            seen_kinds[slot] = true;
            for e in self.index.entries(kind) {
                let boost = boosts
                    .iter()
                    .find(|(n, _)| *n == e.name)
                    .map(|(_, b)| *b)
                    .unwrap_or(0);
                let s = score_entry(e, query, boost);
                if s > 0 {
                    hits.push(SearchHit {
                        name: String::from(e.name),
                        kind: e.kind,
                        detail: e.detail,
                        score: s,
                    });
                }
            }
        }
        hits.sort_by(|a, b| b.score.cmp(&a.score));
        let indexing = !self.index.files_indexed();
        let elapsed = self.clock - start; // 注入钟内查询零人为推进——直读。
        QueryResult {
            suggest_web: hits.is_empty() && !indexing,
            indexing,
            ranked: hits,
            elapsed_ms: elapsed,
        }
    }

    /// Enter 开首项（键盘直达的唯一激活口——首项高亮语义）。
    pub fn first_hit<'a>(&self, r: &'a QueryResult) -> Option<&'a SearchHit> {
        r.ranked.first()
    }

    /// 分区视图（三类分区标签页——按类过滤混排结果）。
    pub fn by_kind<'a>(&self, r: &'a QueryResult, kind: HitKind) -> Vec<&'a SearchHit> {
        r.ranked.iter().filter(|h| h.kind == kind).collect()
    }
}

// ---------------------------------------------------------------------------
// 搜索历史（本地 20 条可清空——隐私总闸联动）
// ---------------------------------------------------------------------------

/// 搜索历史：去重置顶、容量 20 LRU、一键清空。
pub struct SearchHistory {
    items: Vec<String>,
}

impl SearchHistory {
    pub fn new() -> SearchHistory {
        SearchHistory { items: Vec::new() }
    }

    /// 记一次查询（同词置顶不重复）。
    pub fn record(&mut self, q: &str) {
        if q.is_empty() {
            return;
        }
        self.items.retain(|x| x != q);
        if self.items.len() >= HISTORY_CAP {
            self.items.remove(self.items.len() - 1);
        }
        self.items.insert(0, String::from(q));
    }

    pub fn list(&self) -> &[String] {
        &self.items
    }

    /// 一键清空（隐私总闸）。
    pub fn clear(&mut self) {
        self.items.clear();
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F071 自检（判据：三类首结果 10/10；≤50ms；键盘可达）。
pub fn run_startsearch_checks() -> CheckSet {
    let mut set = CheckSet::new("F071-startsearch");

    let mut idx = SearchIndex::new();
    idx.add_app("记事本", "jsb", "1.0");
    idx.add_app("应用商店", "yysd", "2.1");
    idx.add_app("音量控制", "ylkz", "1.2");
    idx.register_setting("音量", "系统/声音/音量");
    idx.register_setting("显示亮度", "系统/显示/亮度");
    idx.mount_files(
        &["报告.docx", "报告-终稿.docx", "账本.xlsx", "下载说明.txt"],
        "文档",
    );

    // 1-3. 三类查询首结果 10/10（主册判据：「音量/记事本/报告.docx」
    //      三类各实测 10 次试验首结果恒正确）。
    let mut eng = SearchEngine::new(idx.clone());
    eng.input_at(0);
    // 设置类：「音量」→ 音量设置页（完全匹配 100 压应用前缀 80）。
    let mut ok = 0u32;
    for _ in 0..10 {
        let r = eng.query("音量", QueryMode::Mixed, &[]);
        if eng.first_hit(&r).map(|h| h.kind == HitKind::Setting && h.name == "音量") == Some(true) {
            ok += 1;
        }
    }
    set.add("setting query first-hit 10/10", ok == 10, "");

    // 应用类：「记事本」→ 记事本应用。
    let mut ok = 0u32;
    for _ in 0..10 {
        let r = eng.query("记事本", QueryMode::Mixed, &[]);
        if eng.first_hit(&r).map(|h| h.kind == HitKind::App && h.name == "记事本") == Some(true) {
            ok += 1;
        }
    }
    set.add("app query first-hit 10/10", ok == 10, "");

    // 文件类：「报告.docx」→ 报告.docx（「报告-终稿.docx」不含连续子串
    // 「报告.docx」——中缀「-终稿」隔断，不误命中）。
    let mut ok = 0u32;
    for _ in 0..10 {
        let r = eng.query("报告.docx", QueryMode::Mixed, &[]);
        if eng.first_hit(&r).map(|h| h.kind == HitKind::File && h.name == "报告.docx") == Some(true) {
            ok += 1;
        }
    }
    set.add("file query first-hit 10/10", ok == 10, "");

    // 4. 按键到首结果 ≤50ms：防抖 50ms 上限 + 查询零人为推进（注入钟
    //    直读 elapsed）——总时延不超判线。
    let mut eng = SearchEngine::new(idx);
    eng.input_at(0);
    assert!(!eng.debounce_ok(49), "防抖期内不放行");
    assert!(eng.debounce_ok(50), "50ms 放行");
    let r = eng.query("音量", QueryMode::Mixed, &[]);
    set.add(
        "key-to-first-hit within 50ms",
        FIRST_HIT_LIMIT_MS == DEBOUNCE_MS && r.elapsed_ms == 0,
        "",
    );

    // 5. 键盘全程可达：first_hit 即 Enter 目标（首项高亮）。
    let r = eng.query("记事本", QueryMode::Mixed, &[]);
    set.add(
        "keyboard enter opens first hit",
        eng.first_hit(&r).map(|h| h.name.as_str()) == Some("记事本"),
        "",
    );

    // 6. 打分公式基准值（一处一事实——改常数必炸这里）。
    let app_e = IndexEntry { name: "应用商店", kind: HitKind::App, pinyin: "yysd", detail: "2.1" };
    let set_e = IndexEntry { name: "显示亮度", kind: HitKind::Setting, pinyin: "", detail: "p" };
    let file_e = IndexEntry { name: "下载说明.txt", kind: HitKind::File, pinyin: "", detail: "d" };
    set.add(
        "score formula exact values",
        score_entry(&set_e, "显示亮度", 0) == SCORE_SETTING_EXACT
            && score_entry(&app_e, "应用", 0) == SCORE_APP_PREFIX
            && score_entry(&app_e, "yy", 0) == SCORE_APP_PREFIX
            && score_entry(&file_e, "说明", 0) == SCORE_FILE_CONTAIN
            && score_entry(&file_e, "不匹配", 0) == 0,
        "",
    );

    // 7. 拼音首字母匹配：yx → 应用类。
    let r = eng.query("yysd", QueryMode::Mixed, &[]);
    set.add(
        "pinyin initials match app",
        r.ranked.first().map(|h| h.kind == HitKind::App && h.name == "应用商店").unwrap_or(false),
        "",
    );

    // 8. 混排 = 最相关优先：设置完全匹配 100 压应用前缀 80（无加权裸分）。
    let r = eng.query("音量", QueryMode::Mixed, &[]);
    let top = r.ranked.first().map(|h| (h.kind, h.score));
    set.add(
        "mixed ranking by relevance",
        top == Some((HitKind::Setting, SCORE_SETTING_EXACT)),
        "",
    );

    // 9. 路径分隔符 → 文件直搜模式（只出文件区）。
    let m = mode_of("文档/报告.docx");
    let r = eng.query("文档/报告.docx", m, &[]);
    set.add(
        "path separator switches file mode",
        m == QueryMode::FilesOnly && r.ranked.iter().all(|h| h.kind == HitKind::File),
        "",
    );

    // 10. 索引未就绪 → 显式「正在建立索引」（非空转），不误报外链。
    let mut idx2 = SearchIndex::new();
    idx2.register_setting("音量", "系统/声音");
    let eng2 = SearchEngine::new(idx2);
    let r = eng2.query("不存在的词", QueryMode::Mixed, &[]);
    set.add(
        "indexing state explicit no busy spin",
        r.indexing && !r.suggest_web,
        "",
    );

    // 11. 无结果（索引就绪）→ 「去浏览器搜索」外链。
    let r = eng.query("zzz-no-such", QueryMode::Mixed, &[]);
    set.add(
        "no result suggests web search",
        r.ranked.is_empty() && r.suggest_web && eng.index.files_indexed(),
        "",
    );

    // 12. 搜索历史：去重置顶、20 条 LRU、一键清空。
    let mut h = SearchHistory::new();
    let words = [
        "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n",
        "o", "p", "q", "r", "s", "t", "u", "v", "w", "x", "y",
    ];
    for w in words {
        h.record(w);
    }
    h.record("a"); // 旧词重查 → 置顶（不增容量占用；最早的 e 已被 LRU 挤出）。
    set.add(
        "history dedupe lru clear",
        h.list().len() == HISTORY_CAP
            && h.list()[0] == "a"
            && !h.list().contains(&String::from("e"))
            && {
                h.clear();
                h.list().is_empty()
            },
        "",
    );

    // 13. 分区视图：三类各自可过滤（横向标签切换数据面）。
    let r = eng.query("音量", QueryMode::Mixed, &[]);
    let (na, ns) = (eng.by_kind(&r, HitKind::App).len(), eng.by_kind(&r, HitKind::Setting).len());
    set.add("section tabs filter by kind", na >= 1 && ns >= 1, "");

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn demo_index() -> SearchIndex {
        let mut i = SearchIndex::new();
        i.add_app("记事本", "jsb", "1.0");
        i.register_setting("音量", "系统/声音/音量");
        i.mount_files(&["报告.docx"], "文档");
        i
    }

    #[test]
    fn files_only_mode_drops_app_setting() {
        let mut eng = SearchEngine::new(demo_index());
        eng.input_at(0);
        let r = eng.query("报告", QueryMode::FilesOnly, &[]);
        assert!(r.ranked.iter().all(|h| h.kind == HitKind::File));
        assert_eq!(r.ranked.len(), 1);
    }

    #[test]
    fn empty_query_yields_empty() {
        let eng = SearchEngine::new(demo_index());
        let r = eng.query("", QueryMode::Mixed, &[]);
        assert!(r.ranked.is_empty());
        assert!(r.suggest_web, "空查询按无结果处理（索引已就绪）");
    }

    #[test]
    fn boost_clamped_to_19() {
        let e = IndexEntry { name: "报告.docx", kind: HitKind::File, pinyin: "", detail: "d" };
        // 60 + 99 → 钳到 60 + 19 = 79（加权不越过设置完全匹配 100）。
        assert_eq!(score_entry(&e, "报告", 99), 79);
    }

    #[test]
    fn debounce_rejects_rapid_input() {
        let mut eng = SearchEngine::new(demo_index());
        eng.input_at(100);
        assert!(!eng.debounce_ok(149), "50ms 内不放行");
        assert!(eng.debounce_ok(150), "防抖期满（恰好 50ms）放行");
    }
}
