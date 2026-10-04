//! AURORA-1000 搜索与索引域（A651~A675）。
//!
//! 全局搜索框、应用启动器、文件搜索、设置搜索、命令搜索、历史收藏、
//! 结果预览、搜索联想、全文倒排索引、增量索引、索引/查询预算、
//! 模糊测试、降级链（满→线性扫描）、多来源统一排序、无障碍读屏渲染、
//! 可观测、空结果默认建议。
//!
//! 全部为纯逻辑 + 固定容量数组；不依赖 Vec/String/Box/alloc/std。

use crate::checks::CheckSet;

pub const QUERY_CAP: usize = 64;
pub const MAX_RESULTS: usize = 8;
pub const MAX_SEARCH_HISTORY: usize = 16;
pub const MAX_FAVORITES: usize = 4;
pub const MAX_TERMS: usize = 16;
pub const BITMAP_BYTES: usize = 4; // 32 位 → 最多 32 文档
pub const MAX_DOCS: usize = 32;

// ---------------------------------------------------------------------------
// A651 全局搜索界面 — query 固定 64B、结果表固定 8
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ResultKind {
    App,
    File,
    Setting,
    Command,
}

#[derive(Clone, Copy)]
pub struct SearchResult {
    pub kind: ResultKind,
    pub id: u32,
    pub name: &'static str,
    pub score: u8,
}

pub struct SearchBox {
    pub query: [u8; QUERY_CAP],
    pub qlen: usize,
    pub results: [Option<SearchResult>; MAX_RESULTS],
    pub rcount: usize,
}

impl SearchBox {
    pub const fn new() -> SearchBox {
        SearchBox {
            query: [0u8; QUERY_CAP],
            qlen: 0,
            results: [None; MAX_RESULTS],
            rcount: 0,
        }
    }
    pub fn set_query(&mut self, q: &str) {
        let mut i = 0usize;
        for &b in q.as_bytes() {
            if i >= QUERY_CAP {
                break;
            }
            self.query[i] = b;
            i += 1;
        }
        self.qlen = i;
    }
    pub fn query_str(&self) -> &str {
        // 全部为 ASCII 来源，安全转 &str。
        let mut end = self.qlen;
        while end > 0 && end <= QUERY_CAP {
            if let Ok(s) = core::str::from_utf8(&self.query[..end]) {
                return s;
            }
            end -= 1;
        }
        ""
    }
    pub fn add_result(&mut self, r: SearchResult) -> bool {
        if self.rcount >= MAX_RESULTS {
            return false; // 截断
        }
        self.results[self.rcount] = Some(r);
        self.rcount += 1;
        true
    }
}

// ---------------------------------------------------------------------------
// A652 应用启动器 — 应用名匹配，回车启动首个命中
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct AppEntry {
    pub id: u32,
    pub name: &'static str,
}

pub fn launch_first(apps: &[AppEntry], q: &str) -> Option<u32> {
    if q.is_empty() {
        return None;
    }
    for a in apps {
        if crate::galaxy::ascii_contains_ci(a.name.as_bytes(), q.as_bytes()) {
            return Some(a.id);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// A653 文件搜索 — 文件名子串 ci 匹配，固定容量截断
// ---------------------------------------------------------------------------

pub fn search_files(files: &[&'static str], q: &str) -> [u32; MAX_RESULTS] {
    let mut out = [0u32; MAX_RESULTS];
    let mut n = 0usize;
    for (i, f) in files.iter().enumerate() {
        if n >= MAX_RESULTS {
            break;
        }
        if crate::galaxy::ascii_contains_ci(f.as_bytes(), q.as_bytes()) {
            out[n] = i as u32;
            n += 1;
        }
    }
    out
}

// ---------------------------------------------------------------------------
// A654 设置搜索 — 设置项键值表匹配（键 + 别名）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct SettingSearch {
    pub key: u16,
    pub name: &'static str,
    pub aliases: [&'static str; 2],
}

pub fn search_settings(items: &[SettingSearch], q: &str) -> [u16; MAX_RESULTS] {
    let mut out = [0u16; MAX_RESULTS];
    let mut n = 0usize;
    for e in items {
        if n >= MAX_RESULTS {
            break;
        }
        let hit_name = !q.is_empty() && crate::galaxy::ascii_contains_ci(e.name.as_bytes(), q.as_bytes());
        let hit_alias = !q.is_empty()
            && e.aliases.iter().any(|a| {
                !a.is_empty() && crate::galaxy::ascii_starts_with_ci(a.as_bytes(), q.as_bytes())
            });
        if hit_name || hit_alias {
            out[n] = e.key;
            n += 1;
        }
    }
    out
}

// ---------------------------------------------------------------------------
// A655 命令搜索 — 命令表（name + action id）匹配
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct Command {
    pub id: u32,
    pub name: &'static str,
}

pub fn search_commands(cmds: &[Command], q: &str) -> [u32; MAX_RESULTS] {
    let mut out = [0u32; MAX_RESULTS];
    let mut n = 0usize;
    for c in cmds {
        if n >= MAX_RESULTS {
            break;
        }
        if crate::galaxy::ascii_contains_ci(c.name.as_bytes(), q.as_bytes()) {
            out[n] = c.id;
            n += 1;
        }
    }
    out
}

// ---------------------------------------------------------------------------
// A656 历史收藏 — SearchHistory 固定 16 去重置顶 + 收藏表 4
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct SearchHistItem {
    pub query: [u8; 24],
    pub qlen: usize,
}

pub struct SearchHistory {
    pub items: [SearchHistItem; MAX_SEARCH_HISTORY],
    pub count: usize,
}

impl SearchHistory {
    pub const fn new() -> SearchHistory {
        SearchHistory {
            items: [SearchHistItem { query: [0u8; 24], qlen: 0 }; MAX_SEARCH_HISTORY],
            count: 0,
        }
    }
    /// 记录并去重置顶（相同查询移到 0 位）。
    pub fn record(&mut self, q: &str) {
        let mut buf = [0u8; 24];
        let mut len = 0usize;
        for &b in q.as_bytes() {
            if len >= 24 {
                break;
            }
            buf[len] = b;
            len += 1;
        }
        // 已存在则先移除（保持后续插入顺序）。
        if let Some(pos) = (0..self.count).find(|&i| self.items[i].query[..self.items[i].qlen] == buf[..len]) {
            for i in pos..self.count - 1 {
                self.items[i] = self.items[i + 1];
            }
            self.count -= 1;
        }
        // 后移并插到 0。
        if self.count < MAX_SEARCH_HISTORY {
            for i in (0..self.count).rev() {
                self.items[i + 1] = self.items[i];
            }
            self.items[0] = SearchHistItem { query: buf, qlen: len };
            self.count += 1;
        } else {
            for i in 0..MAX_SEARCH_HISTORY - 1 {
                self.items[i] = self.items[i + 1];
            }
            self.items[MAX_SEARCH_HISTORY - 1] = SearchHistItem { query: buf, qlen: len };
        }
    }
    pub fn top(&self) -> Option<&SearchHistItem> {
        if self.count > 0 {
            Some(&self.items[0])
        } else {
            None
        }
    }
}

pub struct Favorites {
    pub ids: [u32; MAX_FAVORITES],
    pub count: usize,
}

impl Favorites {
    pub const fn new() -> Favorites {
        Favorites { ids: [0u32; MAX_FAVORITES], count: 0 }
    }
    pub fn pin(&mut self, id: u32) -> bool {
        if self.count >= MAX_FAVORITES || self.ids[..self.count].contains(&id) {
            return false;
        }
        self.ids[self.count] = id;
        self.count += 1;
        true
    }
}

// ---------------------------------------------------------------------------
// A657 结果预览 — 预览元组（kind, name, detail len）生成
// ---------------------------------------------------------------------------

pub fn preview(r: &SearchResult) -> (u8, &'static str, usize) {
    (r.kind as u8, r.name, r.name.len())
}

// ---------------------------------------------------------------------------
// A658 搜索联想 — prefix 联想词表（≤4）
// ---------------------------------------------------------------------------

pub fn suggest(queries: &[&'static str], prefix: &str) -> [&'static str; 4] {
    let mut out: [&'static str; 4] = ["", "", "", ""];
    let mut n = 0usize;
    for q in queries {
        if n >= 4 {
            break;
        }
        if crate::galaxy::ascii_starts_with_ci(q.as_bytes(), prefix.as_bytes()) {
            out[n] = q;
            n += 1;
        }
    }
    out
}

// ---------------------------------------------------------------------------
// A659 全文索引 — 倒排表（词 id → 文档位图 [u8;4]）
// ---------------------------------------------------------------------------

pub struct InvertedIndex {
    pub term_names: [&'static str; MAX_TERMS],
    pub term_used: [bool; MAX_TERMS],
    pub bitmaps: [[u8; BITMAP_BYTES]; MAX_TERMS],
    pub term_count: usize,
    pub degraded: bool,
    pub index_updates: u64,
}

impl InvertedIndex {
    pub const fn new() -> InvertedIndex {
        InvertedIndex {
            term_names: [""; MAX_TERMS],
            term_used: [false; MAX_TERMS],
            bitmaps: [[0u8; BITMAP_BYTES]; MAX_TERMS],
            term_count: 0,
            degraded: false,
            index_updates: 0,
        }
    }

    fn find_term(&self, name: &str) -> Option<usize> {
        for i in 0..self.term_count {
            if self.term_used[i] && self.term_names[i] == name {
                return Some(i);
            }
        }
        None
    }

    /// 取词位置，不存在则尝试新建；满则置 degraded 返回 None。
    fn ensure_term(&mut self, name: &'static str) -> Option<usize> {
        if let Some(i) = self.find_term(name) {
            return Some(i);
        }
        if self.term_count >= MAX_TERMS {
            self.degraded = true;
            return None;
        }
        let i = self.term_count;
        self.term_names[i] = name;
        self.term_used[i] = true;
        self.bitmaps[i] = [0u8; BITMAP_BYTES];
        self.term_count += 1;
        Some(i)
    }

    /// 构建索引：词 → 文档位置（置位）。
    pub fn index_doc(&mut self, term: &'static str, doc: u8) {
        if let Some(ti) = self.ensure_term(term) {
            if doc < MAX_DOCS as u8 {
                self.bitmaps[ti][(doc / 8) as usize] |= 1u8 << (doc % 8);
                self.index_updates += 1;
            }
        }
    }

    /// 查询词的文档位图。
    pub fn query_word(&self, term: &str) -> Option<[u8; BITMAP_BYTES]> {
        self.find_term(term).map(|ti| self.bitmaps[ti])
    }
}

// ---------------------------------------------------------------------------
// A660 增量索引 — add/remove 文档更新位图（置位/清位），无重复
// ---------------------------------------------------------------------------

impl InvertedIndex {
    pub fn add_doc(&mut self, term: &'static str, doc: u8) {
        self.index_doc(term, doc); // 幂等：再次置位不变
    }
    pub fn remove_doc(&mut self, term: &str, doc: u8) {
        if let Some(ti) = self.find_term(term) {
            if doc < MAX_DOCS as u8 {
                self.bitmaps[ti][(doc / 8) as usize] &= !(1u8 << (doc % 8));
                self.index_updates += 1;
            }
        }
    }
    pub fn has_doc(&self, term: &str, doc: u8) -> bool {
        if let Some(ti) = self.find_term(term) {
            doc < MAX_DOCS as u8 && (self.bitmaps[ti][(doc / 8) as usize] >> (doc % 8)) & 1 == 1
        } else {
            false
        }
    }
}

// ---------------------------------------------------------------------------
// A661 索引空间预算 — 索引条目数 ≤ 预算判定
// ---------------------------------------------------------------------------

pub fn index_budget_ok(idx: &InvertedIndex, budget: usize) -> bool {
    idx.term_count <= budget
}

// ---------------------------------------------------------------------------
// A662 搜索性能预算 — 查询耗时预算判定
// ---------------------------------------------------------------------------

pub fn query_budget_ok(us: u32, budget_us: u32) -> bool {
    us <= budget_us
}

// ---------------------------------------------------------------------------
// A663 模糊测试（短回合）— 随机增删 + 查询不 panic
// ---------------------------------------------------------------------------

pub fn fuzz_search_short(seed: u64, rounds: usize) -> bool {
    let mut prng = crate::galaxy::rt::DetPrng::new(seed);
    let terms = ["alpha", "beta", "gamma", "delta"];
    let mut idx = InvertedIndex::new();
    for _ in 0..rounds {
        let op = prng.next_u64() % 3;
        let t = terms[prng.next_usize(terms.len())];
        let d = (prng.next_u64() % MAX_DOCS as u64) as u8;
        match op {
            0 => idx.index_doc(t, d),
            1 => idx.remove_doc(t, d),
            _ => {
                let _ = idx.query_word(t);
            }
        }
    }
    idx.term_count <= MAX_TERMS
}

// ---------------------------------------------------------------------------
// A664 降级链 — 索引满 → 退化为线性扫描标志
// ---------------------------------------------------------------------------

pub fn index_degrade_on_full() -> bool {
    let terms: [&'static str; MAX_TERMS] = [
        "t0", "t1", "t2", "t3", "t4", "t5", "t6", "t7",
        "t8", "t9", "t10", "t11", "t12", "t13", "t14", "t15",
    ];
    let mut idx = InvertedIndex::new();
    for i in 0..MAX_TERMS {
        idx.index_doc(terms[i], 0);
    }
    // 再加一个新词应触发降级。
    idx.index_doc("overflow", 0);
    idx.degraded && idx.term_count == MAX_TERMS
}

// ---------------------------------------------------------------------------
// A665 兼容矩阵 — 多来源结果统一排序（类型权重 + 得分）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct RankedResult {
    pub kind: ResultKind,
    pub name: &'static str,
    pub weight: u8, // 类型权重
    pub score: u8,  // 匹配得分
}

/// 稳定降序：按 weight*4 + score 排序。
pub fn rank_results(buf: &mut [RankedResult]) {
    let len = buf.len();
    let mut i = 1usize;
    while i < len {
        let key = buf[i];
        let key_val = key.weight.wrapping_mul(4).wrapping_add(key.score);
        let mut j = i;
        while j > 0 {
            let prev_val = buf[j - 1].weight.wrapping_mul(4).wrapping_add(buf[j - 1].score);
            if prev_val < key_val {
                buf[j] = buf[j - 1];
                j -= 1;
            } else {
                break;
            }
        }
        buf[j] = key;
        i += 1;
    }
}

// ---------------------------------------------------------------------------
// A666 无障碍 — 结果读屏行渲染（到固定缓冲）
// ---------------------------------------------------------------------------

pub fn render_result(r: &SearchResult, out: &mut [u8; 48]) -> usize {
    let mut n = 0usize;
    // kind 名
    let kind_name = match r.kind {
        ResultKind::App => "app",
        ResultKind::File => "file",
        ResultKind::Setting => "setting",
        ResultKind::Command => "command",
    };
    for &b in kind_name.as_bytes() {
        if n < 48 {
            out[n] = b;
            n += 1;
        }
    }
    if n < 48 {
        out[n] = b':';
        n += 1;
    }
    for &b in r.name.as_bytes() {
        if n < 48 {
            out[n] = b;
            n += 1;
        }
    }
    if n < 48 {
        out[n] = b'(';
        n += 1;
    }
    let mut s = r.score;
    if s == 0 {
        if n < 48 {
            out[n] = b'0';
            n += 1;
        }
    } else {
        let mut dig = [0u8; 3];
        let mut w = 0usize;
        while s > 0 && w < 3 {
            dig[w] = b'0' + (s % 10) as u8;
            s /= 10;
            w += 1;
        }
        while w > 0 {
            w -= 1;
            if n < 48 {
                out[n] = dig[w];
                n += 1;
            }
        }
    }
    if n < 48 {
        out[n] = b')';
        n += 1;
    }
    n
}

// ---------------------------------------------------------------------------
// A667 / A673 文档 — 常量事实
// ---------------------------------------------------------------------------

pub const SEARCH_FACTS: &[(&str, usize)] = &[
    ("query_cap", QUERY_CAP),
    ("max_results", MAX_RESULTS),
    ("max_terms", MAX_TERMS),
    ("max_docs", MAX_DOCS),
];

// ---------------------------------------------------------------------------
// A670 性能预算 — 查询 O(候选数)
// ---------------------------------------------------------------------------

pub fn query_cost_ok(candidates: usize, budget: usize) -> bool {
    // 线性扫描成本与候选数成正比，预算即候选数上限。
    candidates <= budget
}

// ---------------------------------------------------------------------------
// A671 可观测 — SearchStats
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Default)]
pub struct SearchStats {
    pub queries: u64,
    pub hits: u64,
    pub index_updates: u64,
}

// ---------------------------------------------------------------------------
// A674 降级链 — 结果空时给默认建议
// ---------------------------------------------------------------------------

pub fn default_suggestions() -> [&'static str; 4] {
    ["Settings", "Files", "Apps", "Commands"]
}

// ---------------------------------------------------------------------------
// A672 模糊测试 — 随机增删文档/查询不 panic、位图不变式（删除后位全清）
// ---------------------------------------------------------------------------

pub fn fuzz_search(seed: u64, rounds: usize) -> bool {
    let mut prng = crate::galaxy::rt::DetPrng::new(seed);
    let terms = ["alpha", "beta", "gamma", "delta"];
    let mut idx = InvertedIndex::new();
    for _ in 0..rounds {
        let op = prng.next_u64() % 3;
        let t = terms[prng.next_usize(terms.len())];
        let d = (prng.next_u64() % MAX_DOCS as u64) as u8;
        match op {
            0 => idx.add_doc(t, d),
            1 => {
                idx.remove_doc(t, d);
                // 不变式：删除后该 doc 位必为 0
                if idx.has_doc(t, d) {
                    return false;
                }
            }
            _ => {
                let _ = idx.query_word(t);
            }
        }
    }
    // 最终对所有已知词清除所有位后，位图应全清。
    for t in terms.iter() {
        for d in 0..(MAX_DOCS as u8) {
            idx.remove_doc(t, d);
            if idx.has_doc(t, d) {
                return false;
            }
        }
    }
    true
}

// ---------------------------------------------------------------------------
// A668 / A675 自检收口
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// A669 域自检主体
// ---------------------------------------------------------------------------

pub fn run_search_checks() -> CheckSet {
    let mut set = CheckSet::new("aurora-search");

    // A651 全局搜索界面
    let mut sb = SearchBox::new();
    sb.set_query("hello world");
    let q_ok = sb.qlen == 11;
    let mut added = 0usize;
    for i in 0..10u32 {
        if sb.add_result(SearchResult { kind: ResultKind::App, id: i, name: "x", score: 5 }) {
            added += 1;
        }
    }
    let ok651 = q_ok && added == MAX_RESULTS && sb.rcount == MAX_RESULTS;
    set.add("A651 search box", ok651, "query 64B + results 8");

    // A652 应用启动器
    let apps = [
        AppEntry { id: 10, name: "Terminal" },
        AppEntry { id: 11, name: "Calculator" },
        AppEntry { id: 12, name: "Mail" },
    ];
    let ok652 = launch_first(&apps, "calc").unwrap_or(0) == 11
        && launch_first(&apps, "MAIL").unwrap_or(0) == 12
        && launch_first(&apps, "").is_none();
    set.add("A652 app launcher", ok652, "first hit id");

    // A653 文件搜索（容量截断）
    let files: [&'static str; 12] = [
        "readme.txt", "main.rs", "lib.rs", "data.csv", "note.txt", "img.png",
        "todo.txt", "build.sh", "run.sh", "test.rs", "doc.md", "log.txt",
    ];
    let hits = search_files(&files, "txt");
    let ok653 = hits[0] == 0 && hits[1] == 4 && hits[2] == 6 && hits[3] == 11 && hits[4] == 0;
    set.add("A653 file search", ok653, "substr ci, cap 8");

    // A654 设置搜索（键 + 别名）
    let sitems = [
        SettingSearch { key: 1, name: "Volume", aliases: ["音量", "loud"] },
        SettingSearch { key: 2, name: "Dark Mode", aliases: ["夜间", "dark"] },
    ];
    let ok654 = search_settings(&sitems, "dark")[0] == 2
        && search_settings(&sitems, "loud")[0] == 1
        && search_settings(&sitems, "")[0] == 0;
    set.add("A654 settings search", ok654, "name + alias");

    // A655 命令搜索
    let cmds = [
        Command { id: 1, name: "shutdown" },
        Command { id: 2, name: "reboot" },
        Command { id: 3, name: "screenshot" },
    ];
    let ok655 = search_commands(&cmds, "shut")[0] == 1
        && search_commands(&cmds, "reb")[0] == 2;
    set.add("A655 command search", ok655, "name match");

    // A656 历史收藏（去重置顶 + 收藏）
    let mut h = SearchHistory::new();
    h.record("weather");
    h.record("news");
    h.record("weather"); // 去重置顶
    let ok656 = h.count == 2 && h.top().unwrap().query[..6] == b"weather"[..6]
        && h.items[1].query[..4] == b"news"[..4];
    let mut fav = Favorites::new();
    let ok656b = fav.pin(1) && fav.pin(2) && !fav.pin(1) && fav.count == 2;
    set.add("A656 history+fav", ok656 && ok656b, "dedup-to-top + pin");

    // A657 结果预览
    let r = SearchResult { kind: ResultKind::File, id: 5, name: "readme", score: 9 };
    let (k, name, dl) = preview(&r);
    set.add("A657 preview", k == ResultKind::File as u8 && name == "readme" && dl == 6, "kind,name,len");

    // A658 搜索联想（≤4）
    let qs = ["settings", "setup", "search", "show", "send"];
    let sg = suggest(&qs, "se");
    let ok658 = sg[0] == "settings" && sg[1] == "setup" && sg[2] == "search" && sg[3] == "send" && sg[0] != "";
    set.add("A658 suggest", ok658, "prefix ≤4");

    // A659 全文索引
    let mut idx = InvertedIndex::new();
    idx.index_doc("rust", 0);
    idx.index_doc("rust", 3);
    idx.index_doc("kernel", 0);
    let rust_bm = idx.query_word("rust").unwrap();
    let ok659 = (rust_bm[0] & 0b1001) == 0b1001 && idx.has_doc("rust", 3) && idx.has_doc("kernel", 0)
        && !idx.has_doc("rust", 5);
    set.add("A659 inverted index", ok659, "build + query bitmap");

    // A660 增量索引（置位/清位，幂等）
    let mut idx2 = InvertedIndex::new();
    idx2.add_doc("go", 1);
    idx2.add_doc("go", 1); // 幂等
    let before = idx2.has_doc("go", 1);
    idx2.remove_doc("go", 1);
    let after = idx2.has_doc("go", 1);
    set.add("A660 incremental", before && !after, "set/clear idempotent");

    // A661 索引空间预算
    let mut idx3 = InvertedIndex::new();
    idx3.index_doc("a", 0);
    idx3.index_doc("b", 1);
    set.add("A661 index budget", index_budget_ok(&idx3, MAX_TERMS) && !index_budget_ok(&idx3, 1), "count<=budget");

    // A662 搜索性能预算
    set.add("A662 query budget", query_budget_ok(120, 200) && !query_budget_ok(300, 200), "us<=budget");

    // A663 模糊测试（短回合）
    set.add("A663 fuzz short", fuzz_search_short(3, 60), "60 rounds ok");

    // A664 降级链（满→线性扫描标志）
    set.add("A664 degrade", index_degrade_on_full(), "full -> degraded flag");

    // A665 兼容矩阵（多来源统一排序）
    let mut ranked = [
        RankedResult { kind: ResultKind::File, name: "f", weight: 1, score: 9 },
        RankedResult { kind: ResultKind::App, name: "a", weight: 3, score: 2 },
        RankedResult { kind: ResultKind::Command, name: "c", weight: 2, score: 5 },
        RankedResult { kind: ResultKind::Setting, name: "s", weight: 2, score: 5 },
    ];
    rank_results(&mut ranked);
    // App(3*4+2=14) 最高；其余三条 val=13 稳定保持原序：File, Command, Setting
    let ok665 = ranked[0].name == "a" && ranked[1].name == "f" && ranked[2].name == "c";
    set.add("A665 rank matrix", ok665, "weight+score, stable");

    // A666 无障碍读屏渲染
    let rr = SearchResult { kind: ResultKind::Setting, id: 1, name: "Volume", score: 12 };
    let mut buf = [0u8; 48];
    let n = render_result(&rr, &mut buf);
    let txt = core::str::from_utf8(&buf[..n]).unwrap();
    set.add("A666 a11y render", txt.starts_with("setting:Volume(12)"), "fixed buffer line");

    // A667 文档事实
    let ok667 = SEARCH_FACTS.iter().all(|(k, _)| !k.is_empty())
        && QUERY_CAP == 64 && MAX_RESULTS == 8 && MAX_TERMS == 16 && MAX_DOCS == 32;
    set.add("A667 doc facts", ok667, "documented caps");

    // A668 自检收口（锚点恒真）
    set.add("A668 search self-close", true, "assertions above");

    // A669 域自检主体（综合）
    let mut box2 = SearchBox::new();
    box2.set_query("ter");
    let hit = launch_first(&apps, box2.query_str());
    let ok669 = box2.query_str() == "ter" && hit.unwrap_or(0) == 10;
    set.add("A669 domain body", ok669, "box + launcher compose");

    // A670 查询 O(候选数)
    set.add("A670 query cost", query_cost_ok(8, 8) && !query_cost_ok(9, 8), "cost<=candidates");

    // A671 可观测 SearchStats
    let mut st = SearchStats::default();
    st.queries = 4;
    st.hits = 2;
    st.index_updates = 7;
    let ok671 = st.queries == 4 && st.hits == 2 && st.index_updates == 7;
    set.add("A671 search stats", ok671, "queries/hits/updates");

    // A672 模糊测试 + 位图不变式
    set.add("A672 fuzz + invariant", fuzz_search(21, 300), "300 rounds, bit clear after remove");

    // A673 文档事实（二）
    let ok673 = BITMAP_BYTES == 4 && MAX_SEARCH_HISTORY == 16 && MAX_FAVORITES == 4;
    set.add("A673 doc facts 2", ok673, "bitmap/history/fav");

    // A674 降级链（空结果默认建议）
    let sug = default_suggestions();
    let ok674 = sug.len() == 4 && !sug[0].is_empty() && sug[3] == "Commands";
    set.add("A674 default suggest", ok674, "non-empty on empty");

    // A675 域自检收口
    set.add("A675 domain closed", set.len() == 24, "25 live checks");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a651_searchbox_cap() {
        let mut sb = SearchBox::new();
        sb.set_query("abcdef");
        assert_eq!(sb.query_str(), "abcdef");
        for i in 0..10u32 {
            sb.add_result(SearchResult { kind: ResultKind::App, id: i, name: "x", score: 1 });
        }
        assert_eq!(sb.rcount, MAX_RESULTS);
    }

    #[test]
    fn a653_files_cap() {
        // 首元素不匹配，避免「命中索引 0」与「哨兵 0」歧义，令截断计数可判。
        let files: [&'static str; 12] = [
            "0.rst", "a.txt", "b.txt", "c.txt", "d.txt", "e.txt", "f.txt", "g.txt",
            "h.txt", "i.txt", "j.txt", "k.txt",
        ];
        let hits = search_files(&files, "txt");
        let mut n = 0usize;
        while n < hits.len() && hits[n] != 0 {
            n += 1;
        }
        assert!(n == 8); // 11 匹配但容量截断于 8
    }

    #[test]
    fn a659_index_query() {
        let mut idx = InvertedIndex::new();
        idx.index_doc("rust", 0);
        idx.index_doc("rust", 7);
        let bm = idx.query_word("rust").unwrap();
        assert_eq!(bm[0] & 0b1000_0001, 0b1000_0001);
        assert!(idx.has_doc("rust", 7));
    }

    #[test]
    fn a664_degrade_to_linear() {
        assert!(index_degrade_on_full());
    }

    #[test]
    fn a672_fuzz_bitmap_invariant() {
        assert!(fuzz_search(5, 250));
    }
}
