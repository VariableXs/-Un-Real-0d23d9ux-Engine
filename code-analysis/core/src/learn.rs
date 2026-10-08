//! 学习与导航（#347~#364，AI-05 域二）。
//!
//! 全部为确定性算法：预设概念库查表、复杂度加权评分、LCS 行级 diff、
//! 倒排索引+同义词表、定长分块——不调用任何网络模型（零 AI）。
//! 三端等价：只产出纯数据，渲染由壳A/壳B/壳C 各自完成。

use crate::checks::CheckSet;
use std::collections::HashMap;

// ───────────────────────── F347 概念悬浮卡片 ─────────────────────────

/// 悬停 1s 弹出，移开 300ms 淡出；卡片宽 280px、圆角 8px。
pub const HOVER_DELAY_MS: f64 = 1000.0;
pub const FADE_OUT_MS: f64 = 300.0;
pub const CARD_WIDTH_PX: f64 = 280.0;

/// 预设概念库：50 个基础概念（关键词 → 比喻 → 跨语言写法）。
pub const CONCEPTS: &[(&str, &str, &str)] = &[
    ("async", "就像你点了外卖不用在门口等，可以先干别的，外卖到了手机会通知你。", "Python: await / Go: goroutine / Rust: .await"),
    ("await", "外卖到了你去门口拿一下的那一步。", "Python: await / JS: await / C#: await"),
    ("thread", "厨房里多请一个厨师，同时做不同的菜。", "Java: Thread / Rust: thread::spawn / Go: go"),
    ("mutex", "厕所门锁，进去的人锁上，别人得等。", "Rust: Mutex / C++: std::mutex / Java: synchronized"),
    ("lock", "把门反锁，保证同一时间只有一个人用。", "Rust: .lock() / Go: sync.Mutex"),
    ("semaphore", "停车场剩余车位牌，满了就得等。", "Java: Semaphore / Rust: Semaphore"),
    ("atomic", "要么全做完要么全不做，没有做一半的状态。", "C++: std::atomic / Rust: AtomicUsize"),
    ("deadlock", "两个人各拿一把钥匙互等对方先开门，谁也进不去。", "OS: circular wait"),
    ("race", "两个人同时改同一块白板，最后写成啥全看谁手快。", "TSan: data race"),
    ("closure", "带着自己小背包出门的函数，包里装着它要用的变量。", "JS: () => {} / Rust: |x| x"),
    ("callback", "留个电话号码，事情办完打给你。", "JS: cb() / C: function pointer"),
    ("promise", "一张取餐小票，凭它之后能拿到餐。", "JS: Promise / Rust: Future"),
    ("future", "还没做好但保证会有的东西。", "Rust: Future / C++: std::future"),
    ("iterator", "翻书用的手指，一次指一行。", "Rust: Iterator / Python: iter()"),
    ("generator", "边做边卖的摊子，要一份做一份。", "Python: yield / JS: function*"),
    ("recursion", "俄罗斯套娃，一层层打开直到最小的那个。", "All: self-call"),
    ("memoization", "把算过的答案记在本子上，下次直接抄。", "Python: lru_cache"),
    ("cache", "把常用东西放抽屉里，不用每次去仓库拿。", "Redis / LRU"),
    ("buffer", "水桶，先把水攒一攒再一次性倒。", "C: char buf[N]"),
    ("queue", "排队买票，先来的先买。", "Rust: VecDeque / Java: Queue"),
    ("stack", "一摞盘子，只能从最上面拿。", "Rust: Vec / CPU: call stack"),
    ("heap", "自由市场，谁有空位谁摆摊。", "C: malloc / Rust: Box"),
    ("gc", "保洁阿姨定时来收没人要的垃圾。", "Java: GC / Go: GC"),
    ("pointer", "写着地址的便签，按地址能找到真正的房子。", "C: int* / Rust: &T"),
    ("reference", "房子的另一把钥匙，不复制房子本身。", "C++: T& / Rust: &T"),
    ("ownership", "一件东西同一时间只有一个主人。", "Rust: ownership"),
    ("borrow", "把东西借给你用，用完要还。", "Rust: &T / &mut T"),
    ("lifetime", "这把钥匙的有效期。", "Rust: 'a"),
    ("trait", "能力证书，有证就能干这活。", "Rust: trait / Java: interface"),
    ("interface", "插座标准，符合就能插上。", "Go: interface / TS: interface"),
    ("generic", "同一套模具，倒进去什么材料出什么形状。", "Rust: <T> / Java: <T>"),
    ("inheritance", "儿子继承老子的家产和本事。", "Java: extends / C++: :"),
    ("polymorphism", "同一个按钮，按下后不同机器做不同的事。", "OOP: virtual / override"),
    ("encapsulation", "把零件装在盒子里，只留几个按钮在外面。", "OOP: private/public"),
    ("exception", "着火了拉警报，程序跳到救火流程。", "Java: try-catch / Rust: Result"),
    ("try", "试着做，坏了走备用方案。", "JS: try / Python: try"),
    ("panic", "实在撑不住了直接掀桌子。", "Rust: panic! / Go: panic"),
    ("unwrap", "不检查就拆包裹，空的就炸。", "Rust: .unwrap()"),
    ("option", "盒子里可能有东西也可能是空的，打开前得先确认。", "Rust: Option / Java: Optional"),
    ("result", "回执单：要么成功带着结果，要么失败带着原因。", "Rust: Result"),
    ("null", "空盒子，打开它就出事。", "C: NULL / Java: null"),
    ("immutable", "刻在石头上的字，改不了。", "Rust: let / JS: const"),
    ("mutable", "白板上的字，随时能擦了重写。", "Rust: let mut"),
    ("pure", "同样的输入永远给同样的输出，不偷偷干别的。", "FP: pure function"),
    ("sideeffect", "除了算答案，还偷偷改了别处的东西。", "FP: side effect"),
    ("idempotent", "做一次和做一百次结果一样。", "HTTP: PUT"),
    ("singleton", "全班只有一个班长。", "Java: Singleton"),
    ("factory", "专门负责造东西的车间。", "GoF: Factory"),
    ("observer", "订阅了公众号，一发文你就收到。", "GoF: Observer"),
    ("middleware", "过安检，每个人都要过一遍才能进。", "Express: middleware"),
    ("orm", "把数据库的表当成对象来用，不用手写 SQL。", "Python: SQLAlchemy"),
];

/// 概念卡片（悬浮卡片内容）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConceptCard {
    pub keyword: String,
    pub metaphor: String,
    pub cross_lang: String,
    /// 该关键词在项目中的出现次数（卡片底部统计）。
    pub usages: usize,
}

/// 查卡片；未收录返回 None。
pub fn concept_card(keyword: &str, usages: usize) -> Option<ConceptCard> {
    CONCEPTS
        .iter()
        .find(|(k, _, _)| *k == keyword.trim().to_lowercase().as_str())
        .map(|(k, m, c)| ConceptCard {
            keyword: k.to_string(),
            metaphor: m.to_string(),
            cross_lang: c.to_string(),
            usages,
        })
}

/// 悬停逻辑：停留 ≥1s 才弹卡片。
pub fn should_show(dwell_ms: f64) -> bool {
    dwell_ms >= HOVER_DELAY_MS
}

// ───────────────────────── F348 代码难度评级 ─────────────────────────

/// 难度评级输入指标。
#[derive(Debug, Clone, Copy)]
pub struct DifficultyInput {
    pub loc: usize,
    pub cyclomatic: usize,
    pub max_nesting: usize,
    /// 注释行占比 0.0~1.0
    pub comment_ratio: f64,
}

/// 评级结果：1~5 星 + 评分依据（悬停显示）。
#[derive(Debug, Clone, PartialEq)]
pub struct Difficulty {
    pub stars: u8,
    pub reasons: Vec<String>,
}

pub fn rate_difficulty(i: DifficultyInput) -> Difficulty {
    let mut score = 0.0_f64;
    let mut reasons = Vec::new();
    if i.cyclomatic > 10 {
        score += 2.0;
        reasons.push(format!("圈复杂度 {} 偏高", i.cyclomatic));
    } else if i.cyclomatic > 5 {
        score += 1.0;
        reasons.push(format!("圈复杂度 {} 中等", i.cyclomatic));
    }
    if i.max_nesting > 3 {
        score += 1.5;
        reasons.push(format!("嵌套 {} 层过深", i.max_nesting));
    }
    if i.loc > 300 {
        score += 1.0;
        reasons.push(format!("{} 行偏长", i.loc));
    }
    if i.comment_ratio < 0.05 {
        score += 0.5;
        reasons.push("注释过少".to_string());
    }
    if reasons.is_empty() {
        reasons.push("结构简单、注释充分".to_string());
    }
    let stars = (score.ceil() as u8).clamp(1, 5);
    Difficulty { stars, reasons }
}

// ───────────────────────── F349 相似代码对比 ─────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffKind {
    Same,
    Add,
    Del,
}

/// 并排对比的一行：左/右行号 + 内容 + 状态（绿=新增，红=删除）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffRow {
    pub kind: DiffKind,
    pub left: Option<(usize, String)>,
    pub right: Option<(usize, String)>,
}

/// LCS 行级 diff，确定性。
pub fn diff_lines(left: &[&str], right: &[&str]) -> Vec<DiffRow> {
    let n = left.len();
    let m = right.len();
    let mut dp = vec![vec![0usize; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[i][j] = if left[i] == right[j] {
                dp[i + 1][j + 1] + 1
            } else {
                dp[i + 1][j].max(dp[i][j + 1])
            };
        }
    }
    let mut out = Vec::new();
    let (mut i, mut j) = (0usize, 0usize);
    while i < n && j < m {
        if left[i] == right[j] {
            out.push(DiffRow {
                kind: DiffKind::Same,
                left: Some((i + 1, left[i].to_string())),
                right: Some((j + 1, right[j].to_string())),
            });
            i += 1;
            j += 1;
        } else if dp[i + 1][j] >= dp[i][j + 1] {
            out.push(DiffRow {
                kind: DiffKind::Del,
                left: Some((i + 1, left[i].to_string())),
                right: None,
            });
            i += 1;
        } else {
            out.push(DiffRow {
                kind: DiffKind::Add,
                left: None,
                right: Some((j + 1, right[j].to_string())),
            });
            j += 1;
        }
    }
    while i < n {
        out.push(DiffRow {
            kind: DiffKind::Del,
            left: Some((i + 1, left[i].to_string())),
            right: None,
        });
        i += 1;
    }
    while j < m {
        out.push(DiffRow {
            kind: DiffKind::Add,
            left: None,
            right: Some((j + 1, right[j].to_string())),
        });
        j += 1;
    }
    out
}

// ───────────────────────── F350 代码词典 ─────────────────────────

/// 词典条目（术语 → 比喻），按字母排序展示。
pub fn dictionary() -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = CONCEPTS.iter().map(|(k, m, _)| (k.to_string(), m.to_string())).collect();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v
}

pub fn dict_lookup(term: &str) -> Option<&'static str> {
    CONCEPTS
        .iter()
        .find(|(k, _, _)| *k == term.trim().to_lowercase().as_str())
        .map(|(_, m, _)| *m)
}

// ───────────────────────── F351 逻辑测验 ─────────────────────────

/// 由 CFG 分支生成的选择题。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Question {
    pub prompt: String,
    pub options: Vec<String>,
    /// 正确选项下标
    pub answer: usize,
}

/// `branches` = [(条件, 命中后做的事)]；题目问「若条件不成立会怎样」。
pub fn quiz_from_cfg(func: &str, branches: &[(&str, &str)]) -> Option<Question> {
    let (cond, then) = *branches.first()?;
    let mut options = vec![then.to_string(), format!("跳过，继续往下执行")];
    options.extend(branches.iter().skip(1).map(|(_, t)| t.to_string()));
    Some(Question {
        prompt: format!("在 {func} 里，如果「{cond}」不成立，会发生什么？"),
        options,
        answer: 1,
    })
}

pub fn answer_quiz(q: &Question, pick: usize) -> bool {
    pick == q.answer
}

// ───────────────────────── F352 成长日记 ─────────────────────────

/// 阅读追踪：日期 → 当天看过的函数数。
#[derive(Debug, Default)]
pub struct Diary {
    pub days: Vec<(String, usize)>,
}

impl Diary {
    pub fn new() -> Self {
        Diary::default()
    }

    /// 记录一天（同日累加，去重登记）。
    pub fn log(&mut self, date: &str, funcs: usize) {
        if let Some(d) = self.days.iter_mut().find(|(d, _)| d == date) {
            d.1 += funcs;
        } else {
            self.days.push((date.to_string(), funcs));
        }
    }

    pub fn today(&self, date: &str) -> usize {
        self.days.iter().find(|(d, _)| d == date).map(|(_, n)| *n).unwrap_or(0)
    }

    /// 进步百分比：相对前一天（首日按 100%）。
    pub fn progress(&self, date: &str) -> i32 {
        let idx = match self.days.iter().position(|(d, _)| d == date) {
            Some(i) => i,
            None => return 0,
        };
        if idx == 0 {
            return 100;
        }
        let prev = self.days[idx - 1].1.max(1);
        let cur = self.days[idx].1;
        ((cur as i32 - prev as i32) * 100) / prev as i32
    }

    /// 状态栏文案。
    pub fn summary(&self, date: &str) -> String {
        format!("今天看了 {} 个函数，进步 {}%", self.today(date), self.progress(date))
    }
}

// ───────────────────────── F353 逻辑目录树 ─────────────────────────

/// 目录树条目（业务 → 模块 → 函数）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeEntry {
    pub depth: usize,
    pub label: String,
}

/// `tree` = [(业务, [模块], [函数])] 按业务逻辑聚类组织。
pub fn logic_tree(domains: &[(&str, &[&str], &[&str])]) -> Vec<TreeEntry> {
    let mut out = Vec::new();
    for (domain, modules, funcs) in domains {
        out.push(TreeEntry { depth: 0, label: domain.to_string() });
        for m in *modules {
            out.push(TreeEntry { depth: 1, label: m.to_string() });
        }
        for f in *funcs {
            out.push(TreeEntry { depth: 2, label: f.to_string() });
        }
    }
    out
}

// ───────────────────────── F354 逻辑面包屑 ─────────────────────────

/// 调用栈 → 面包屑，可点击跳转（返回可点击的层级）。
pub fn breadcrumb(stack: &[&str]) -> Vec<String> {
    std::iter::once("项目")
        .chain(stack.iter().copied())
        .map(|s| s.to_string())
        .collect()
}

pub fn render_crumbs(stack: &[&str]) -> String {
    breadcrumb(stack).join(" > ")
}

/// 点击第 i 级 = 跳到栈的第 i-1 层（0 级是项目根）。
pub fn crumb_target<'a>(stack: &[&'a str], idx: usize) -> Option<&'a str> {
    if idx == 0 {
        return Some("项目");
    }
    stack.get(idx - 1).copied()
}

// ───────────────────────── F355 函数迷你图 ─────────────────────────

pub const MINI_SIZE_PX: (f64, f64) = (30.0, 30.0);

/// 函数节点旁的 30×30 迷你调用图：只取直接调用边。
pub fn mini_graph(func: &str, calls: &[(&str, &str)], limit: usize) -> Vec<(String, String)> {
    calls
        .iter()
        .filter(|(f, _)| *f == func)
        .take(limit)
        .map(|(f, t)| (f.to_string(), t.to_string()))
        .collect()
}

// ───────────────────────── F356 语义搜索 ─────────────────────────

/// 倒排索引 + 同义词表：搜「处理登录」能匹配 `authenticate()`。
#[derive(Debug, Default)]
pub struct SemanticIndex {
    pub postings: HashMap<String, Vec<String>>,
    pub synonyms: HashMap<String, Vec<String>>,
}

impl SemanticIndex {
    pub fn new() -> Self {
        SemanticIndex::default()
    }

    /// 同义词登记（双向展开在查询时做）。
    pub fn add_synonym(&mut self, word: &str, alt: &str) {
        self.synonyms
            .entry(word.to_string())
            .or_default()
            .push(alt.to_string());
    }

    /// 索引一个符号（分词：下划线/驼峰/小写）。
    pub fn index(&mut self, symbol: &str) {
        for tok in tokenize(symbol) {
            self.postings.entry(tok).or_default().push(symbol.to_string());
        }
    }

    /// 语义查询：先直查，再走同义词扩展，最后子串兜底。
    pub fn search(&self, query: &str) -> Vec<String> {
        let mut hits: Vec<String> = Vec::new();
        for tok in tokenize(query) {
            if let Some(v) = self.postings.get(&tok) {
                push_all(&mut hits, v);
            }
            if let Some(alts) = self.synonyms.get(&tok) {
                for a in alts {
                    if let Some(v) = self.postings.get(a) {
                        push_all(&mut hits, v);
                    }
                }
            }
        }
        if hits.is_empty() {
            // 同义词词根命中：查询里含某个同义词 → 展开别名 → 别名作为子串匹配索引键
            let q = query.to_lowercase();
            let mut keys: Vec<&String> = self.synonyms.keys().collect();
            keys.sort();
            for word in keys {
                if q.contains(word.as_str()) {
                    if let Some(alts) = self.synonyms.get(word) {
                        for a in alts {
                            let mut pks: Vec<&String> = self.postings.keys().collect();
                            pks.sort();
                            for k in pks {
                                if k.contains(a.as_str()) {
                                    if let Some(v) = self.postings.get(k) {
                                        push_all(&mut hits, v);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        if hits.is_empty() {
            let mut pks: Vec<&String> = self.postings.keys().collect();
            pks.sort();
            for k in pks {
                if k.contains(&query.to_lowercase()) {
                    if let Some(v) = self.postings.get(k) {
                        push_all(&mut hits, v);
                    }
                }
            }
        }
        hits
    }
}

fn push_all(dst: &mut Vec<String>, src: &[String]) {
    for s in src {
        if !dst.contains(s) {
            dst.push(s.clone());
        }
    }
}

/// 分词：下划线 + 驼峰切分，全小写。
pub fn tokenize(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in s.chars() {
        // 词边界：空格/下划线/连字符/括号等非字母数字符号都切分。
        if c == '_' || c == '-' || c == ' ' || !c.is_alphanumeric() {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur).to_lowercase());
            }
        } else if c.is_uppercase() && !cur.is_empty() {
            out.push(std::mem::take(&mut cur).to_lowercase());
            cur.push(c.to_lowercase().next().unwrap_or(c));
        } else {
            cur.push(c);
        }
    }
    if !cur.is_empty() {
        out.push(cur.to_lowercase());
    }
    out
}

// ───────────────────────── F357 阅读进度条 ─────────────────────────

/// 已浏览百分比（0.0~1.0），画布顶部细条绿色填充。
pub fn reading_progress(viewed: usize, total: usize) -> f64 {
    if total == 0 {
        return 0.0;
    }
    (viewed as f64 / total as f64).clamp(0.0, 1.0)
}

// ───────────────────────── F358 预设问答 ─────────────────────────

/// 右侧面板底部按钮的预设问题。
pub const PRESETS: &[&str] = &[
    "这函数干嘛？",
    "哪里会出错？",
    "为什么这么写？",
    "能不能更简单？",
    "谁在调用它？",
];

/// 模板填充式回答（确定性，不调用模型）。
pub fn answer_preset(q: &str, func: &str, callers: usize, risks: usize) -> String {
    match q {
        "这函数干嘛？" => format!("{func} 负责一段独立的小任务，输入输出都在签名里写明了。"),
        "哪里会出错？" => format!("{func} 有 {risks} 处风险点（空值/越界/未捕获），已在画布上标红。"),
        "为什么这么写？" => format!("{func} 现在的写法是历史演进的结果，改动前建议先看调用它的 {callers} 处。"),
        "能不能更简单？" => format!("{func} 可以抽取重复分支，预计能省下约 20% 行数。"),
        "谁在调用它？" => format!("共有 {callers} 处调用了 {func}，点击可逐个跳转。"),
        _ => format!("暂时没有「{q}」的答案，试试其他预设问题。"),
    }
}

// ───────────────────────── F359 项目地图 ─────────────────────────

/// 首次加载自动生成 2D 模块关系图（目录 + import）。
pub fn project_map(modules: &[&str], imports: &[(&str, &str)]) -> (Vec<String>, Vec<(String, String)>) {
    let nodes: Vec<String> = modules.iter().map(|m| m.to_string()).collect();
    let edges: Vec<(String, String)> = imports
        .iter()
        .filter(|(a, b)| nodes.contains(&a.to_string()) && nodes.contains(&b.to_string()))
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect();
    (nodes, edges)
}

// ───────────────────────── F360 搜索定位 ─────────────────────────

/// 全文索引：词 → 行号列表。
#[derive(Debug, Default)]
pub struct FullTextIndex {
    pub map: HashMap<String, Vec<usize>>,
}

impl FullTextIndex {
    pub fn new() -> Self {
        FullTextIndex::default()
    }

    pub fn add(&mut self, line_no: usize, text: &str) {
        for tok in tokenize(text) {
            let e = self.map.entry(tok).or_default();
            if !e.contains(&line_no) {
                e.push(line_no);
            }
        }
    }

    /// 点击结果 → 飞到该行并打聚光灯。
    pub fn locate(&self, query: &str) -> Vec<usize> {
        let mut v = self.map.get(&query.to_lowercase()).cloned().unwrap_or_default();
        v.sort_unstable();
        v
    }
}

pub const SPOTLIGHT_RADIUS_PX: f64 = 120.0;

// ───────────────────────── F361 只看主线 ─────────────────────────

/// 节点类别：主线 = 核心逻辑；其余在「只看主线」时隐藏。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeCat {
    Main,
    Exception,
    Log,
    Boundary,
}

pub fn is_main(cat: NodeCat) -> bool {
    matches!(cat, NodeCat::Main)
}

/// 过滤：只留核心逻辑路径。
pub fn mainline_only(nodes: &[(usize, NodeCat)]) -> Vec<usize> {
    nodes.iter().filter(|(_, c)| is_main(*c)).map(|(i, _)| *i).collect()
}

// ───────────────────────── F362 自动分块 ─────────────────────────

/// 逻辑块。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub title: String,
    pub summary: String,
    pub start: usize,
    pub end: usize,
}

/// 大文件按 max_lines 切块，每块给一句话摘要。
pub fn auto_blocks(src: &str, max_lines: usize) -> Vec<Block> {
    let lines: Vec<&str> = src.lines().collect();
    let step = max_lines.max(1);
    let mut out = Vec::new();
    let mut i = 0usize;
    let mut n = 0usize;
    while i < lines.len() {
        let end = (i + step).min(lines.len());
        let chunk = &lines[i..end];
        let title = chunk
            .iter()
            .find(|l| l.trim_start().starts_with("fn ") || l.trim_start().starts_with("pub fn "))
            .map(|l| l.trim().to_string())
            .unwrap_or_else(|| format!("第 {} 块", n + 1));
        out.push(Block {
            title,
            summary: format!("{} 行：{}", chunk.len(), chunk.first().unwrap_or(&"").trim()),
            start: i + 1,
            end,
        });
        i = end;
        n += 1;
    }
    out
}

// ───────────────────────── F363 块间导航 ─────────────────────────

/// 左侧目录：块标题 + 一句话摘要 + 跳转行号。
pub fn block_index(blocks: &[Block]) -> Vec<(String, String, usize)> {
    blocks
        .iter()
        .map(|b| (b.title.clone(), b.summary.clone(), b.start))
        .collect()
}

// ───────────────────────── F364 跨文件追踪 ─────────────────────────

/// 点击函数 → 飞到另一个文件的调用处。
pub fn cross_file_target<'a>(
    calls: &'a [(&'a str, &'a str, &'a str, usize)],
    func: &str,
    from_file: &str,
) -> Option<(&'a str, usize)> {
    calls
        .iter()
        .find(|(_, to, file, _)| *to == func && *file != from_file)
        .map(|(_, _, file, line)| (*file, *line))
}

// ───────────────────────── 域自检 ─────────────────────────

/// #347~#364 自检（18 项）。
pub fn run_learn_checks() -> CheckSet {
    let mut s = CheckSet::new("learn");

    // F347 概念悬浮卡片
    let card = concept_card("mutex", 12);
    s.add(
        "F347 概念悬浮卡片",
        CONCEPTS.len() >= 50 && card.is_some() && card.as_ref().unwrap().usages == 12
            && HOVER_DELAY_MS == 1000.0 && FADE_OUT_MS == 300.0 && should_show(1000.0) && !should_show(999.0),
        "50 概念预设库+悬停1s/淡出300ms",
    );

    // F348 代码难度评级
    let easy = rate_difficulty(DifficultyInput { loc: 20, cyclomatic: 2, max_nesting: 1, comment_ratio: 0.2 });
    let hard = rate_difficulty(DifficultyInput { loc: 900, cyclomatic: 30, max_nesting: 6, comment_ratio: 0.0 });
    s.add(
        "F348 代码难度评级",
        easy.stars == 1 && hard.stars == 5 && !hard.reasons.is_empty() && hard.reasons.len() >= 3,
        "⭐~⭐⭐⭐⭐⭐ + 依据",
    );

    // F349 相似代码对比
    let rows = diff_lines(&["a", "b", "c"], &["a", "x", "c"]);
    let adds = rows.iter().filter(|r| r.kind == DiffKind::Add).count();
    let dels = rows.iter().filter(|r| r.kind == DiffKind::Del).count();
    s.add(
        "F349 相似代码对比",
        adds == 1 && dels == 1 && rows.iter().any(|r| r.kind == DiffKind::Same) && rows.len() == 4,
        "行级 diff 绿新增/红删除",
    );

    // F350 代码词典
    let dict = dictionary();
    let sorted = dict.windows(2).all(|w| w[0].0 <= w[1].0);
    s.add(
        "F350 代码词典",
        dict.len() >= 50 && sorted && dict_lookup("closure").is_some() && dict_lookup("nope").is_none(),
        "术语→比喻，按字母排序",
    );

    // F351 逻辑测验
    let q = quiz_from_cfg("login", &[("已登录", "进入首页"), ("未登录", "跳登录页")]).unwrap();
    s.add(
        "F351 逻辑测验",
        q.options.len() == 3 && answer_quiz(&q, q.answer) && !answer_quiz(&q, 0) && q.prompt.contains("login"),
        "CFG→选择题，答对变绿",
    );

    // F352 成长日记
    let mut d = Diary::new();
    d.log("2026-09-12", 2);
    d.log("2026-09-13", 5);
    d.log("2026-09-13", 1);
    let today = d.today("2026-09-13");
    let prog = d.progress("2026-09-13");
    s.add(
        "F352 成长日记",
        today == 6 && prog == 200 && d.summary("2026-09-13").contains("6"),
        "阅读追踪+进步百分比",
    );

    // F353 逻辑目录树
    let t = logic_tree(&[("用户管理", &["登录"], &["验证密码", "签发 token"])]);
    s.add(
        "F353 逻辑目录树",
        t.len() == 4 && t[0].depth == 0 && t[0].label == "用户管理" && t[3].label == "签发 token",
        "业务→模块→函数聚类",
    );

    // F354 逻辑面包屑
    let stack = ["用户模块", "登录", "验证密码"];
    let crumbs = render_crumbs(&stack);
    s.add(
        "F354 逻辑面包屑",
        crumbs == "项目 > 用户模块 > 登录 > 验证密码" && crumb_target(&stack, 0) == Some("项目") && crumb_target(&stack, 2) == Some("登录"),
        "调用栈→可点击路径",
    );

    // F355 函数迷你图
    let mini = mini_graph("f", &[("f", "g"), ("f", "h"), ("x", "y")], 5);
    s.add(
        "F355 函数迷你图",
        mini.len() == 2 && MINI_SIZE_PX == (30.0, 30.0) && mini_graph("z", &[("f", "g")], 5).is_empty(),
        "30×30 迷你调用子图",
    );

    // F356 语义搜索
    let mut idx = SemanticIndex::new();
    idx.index("authenticate");
    idx.add_synonym("处理", "auth");
    idx.add_synonym("登录", "auth");
    idx.index("auth");
    let hits = idx.search("处理登录");
    s.add(
        "F356 语义搜索",
        hits.iter().any(|h| h == "authenticate") && tokenize("getUserName") == vec!["get", "user", "name"],
        "倒排索引+同义词+子串兜底",
    );

    // F357 阅读进度条
    s.add(
        "F357 阅读进度条",
        reading_progress(25, 100) == 0.25 && reading_progress(200, 100) == 1.0 && reading_progress(0, 0) == 0.0,
        "已浏览百分比（夹紧 0~1）",
    );

    // F358 预设问答
    let a1 = answer_preset("这函数干嘛？", "f", 3, 2);
    let a2 = answer_preset("哪里会出错？", "f", 3, 2);
    s.add(
        "F358 预设问答",
        PRESETS.len() == 5 && a1.contains("f") && a2.contains("2") && answer_preset("未知", "f", 0, 0).contains("没有"),
        "模板填充式预设问题",
    );

    // F359 项目地图
    let (nodes, edges) = project_map(&["app", "core", "ui"], &[("app", "core"), ("ui", "core"), ("app", "zzz")]);
    s.add(
        "F359 项目地图",
        nodes.len() == 3 && edges.len() == 2 && edges.contains(&("app".into(), "core".into())),
        "目录+import 生成模块关系图",
    );

    // F360 搜索定位
    let mut ft = FullTextIndex::new();
    ft.add(3, "fn login()");
    ft.add(9, "login ok");
    let loc = ft.locate("login");
    s.add(
        "F360 搜索定位",
        loc == vec![3, 9] && SPOTLIGHT_RADIUS_PX == 120.0 && ft.locate("nope").is_empty(),
        "全文索引→飞到节点+聚光灯",
    );

    // F361 只看主线
    let nodes2 = [(0, NodeCat::Main), (1, NodeCat::Log), (2, NodeCat::Main), (3, NodeCat::Exception)];
    s.add(
        "F361 只看主线",
        mainline_only(&nodes2) == vec![0, 2] && is_main(NodeCat::Main) && !is_main(NodeCat::Boundary),
        "隐藏异常/日志/边界节点",
    );

    // F362 自动分块
    let src = (0..7).map(|i| format!("fn f{i}() {{}}")).collect::<Vec<_>>().join("\n");
    let blocks = auto_blocks(&src, 3);
    s.add(
        "F362 自动分块",
        blocks.len() == 3 && blocks[0].start == 1 && blocks[2].end == 7 && blocks[0].title.contains("f0"),
        "大文件切块+每块摘要",
    );

    // F363 块间导航
    let bi = block_index(&blocks);
    s.add(
        "F363 块间导航",
        bi.len() == 3 && bi[1].2 == 4 && !bi[0].1.is_empty(),
        "目录列出块+摘要+行号",
    );

    // F364 跨文件追踪
    let calls = [("main.rs", "ping", "net.rs", 12), ("net.rs", "ping", "net.rs", 30)];
    let target = cross_file_target(&calls, "ping", "main.rs");
    s.add(
        "F364 跨文件追踪",
        target == Some(("net.rs", 12)) && cross_file_target(&calls, "pong", "main.rs").is_none(),
        "跳到另一文件的调用处",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f347_card_content_and_gate() {
        let c = concept_card("async", 12).unwrap();
        assert!(c.metaphor.contains("外卖"));
        assert!(c.cross_lang.contains("await"));
        assert_eq!(c.usages, 12);
        assert!(should_show(1000.0));
        assert!(!should_show(500.0));
    }

    #[test]
    fn f348_stars_monotonic_with_complexity() {
        let a = rate_difficulty(DifficultyInput { loc: 10, cyclomatic: 1, max_nesting: 1, comment_ratio: 0.5 });
        let b = rate_difficulty(DifficultyInput { loc: 900, cyclomatic: 40, max_nesting: 8, comment_ratio: 0.0 });
        assert!(a.stars < b.stars);
        assert!(a.stars >= 1 && b.stars <= 5);
    }

    #[test]
    fn f349_diff_pure_add_and_pure_del() {
        let rows = diff_lines(&["a"], &[]);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].kind, DiffKind::Del);
        let rows2 = diff_lines(&[], &["b"]);
        assert_eq!(rows2[0].kind, DiffKind::Add);
    }

    #[test]
    fn f352_diary_dedup_and_first_day() {
        let mut d = Diary::new();
        d.log("d1", 2);
        d.log("d1", 3);
        assert_eq!(d.days.len(), 1);
        assert_eq!(d.today("d1"), 5);
        assert_eq!(d.progress("d1"), 100);
        assert_eq!(d.today("dx"), 0);
    }

    #[test]
    fn f356_semantic_synonym_expansion() {
        let mut idx = SemanticIndex::new();
        idx.index("checkLogin");
        idx.add_synonym("校验", "check");
        let hits = idx.search("校验");
        assert!(hits.contains(&"checkLogin".to_string()));
    }

    #[test]
    fn f362_blocks_cover_all_lines() {
        let src = "a\nb\nc\nd\n";
        let b = auto_blocks(src, 2);
        assert_eq!(b.len(), 2);
        assert_eq!(b[0].start, 1);
        assert_eq!(b[1].end, 4);
    }
}
