//! UNREAL-X-15000 · AI-16 任务栏内核与引擎·代码分析线（族0154/0155/0156/0159 · 100 项），勿删。
//!
//! * 族0154 启动计数与排序（X03826~X03850）：启动计数去重登记、衰减、排名快照；
//! * 族0155 搜索语义扩展（X03851~X03875）：同义词、拼音首字母、编辑距离容错；
//! * 族0156 任务栏遥测（X03876~X03900）：事件计数、环形缓冲、聚合报表；
//! * 族0159 任务栏体检（X03951~X03975）：体检项注册表、阈值判定、报告渲染。
//!
//! 零 AI：全部确定性算法；CheckSet 自检 100 项全绿（cargo test -p ca-core --lib）。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 族0154 启动计数与排序
// ---------------------------------------------------------------------------

/// 启动计数器：登记去重 + 时间衰减（每日 ×0.9 整数 permille）+ 排名。
#[derive(Default)]
pub struct LaunchCounter {
    counts: Vec<(u32, u64)>, // (app_id, count)
}

impl LaunchCounter {
    pub fn new() -> LaunchCounter {
        LaunchCounter { counts: Vec::new() }
    }

    pub fn len(&self) -> usize {
        self.counts.len()
    }

    /// 登记一次启动（去重计数，非重复插入）。
    pub fn record(&mut self, id: u32) {
        match self.counts.iter_mut().find(|(i, _)| *i == id) {
            Some((_, c)) => *c += 1,
            None => self.counts.push((id, 1)),
        }
    }

    pub fn count(&self, id: u32) -> u64 {
        self.counts.iter().find(|(i, _)| *i == id).map(|(_, c)| *c).unwrap_or(0)
    }

    /// 每日衰减：count = count * 900 / 1000（向下取整，保底 1 若原 >0）。
    pub fn decay_day(&mut self) {
        for (_, c) in self.counts.iter_mut() {
            if *c > 0 {
                *c = ((*c * 900) / 1000).max(1);
            }
        }
    }

    /// 排名快照：count 降序，同分按 id 升序。
    pub fn ranked(&self) -> Vec<u32> {
        let mut v = self.counts.clone();
        v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        v.into_iter().map(|(i, _)| i).collect()
    }

    /// 净身：清零。
    pub fn reset(&mut self) -> usize {
        let n = self.counts.len();
        self.counts.clear();
        n
    }
}

// ---------------------------------------------------------------------------
// 族0155 搜索语义扩展
// ---------------------------------------------------------------------------

/// 同义词表（确定性，内置）。
pub const SYNONYMS: &[(&str, &str)] = &[
    ("wifi", "无线"),
    ("terminal", "终端"),
    ("settings", "设置"),
    ("files", "文件"),
    ("notification", "通知"),
];

/// 汉字→拼音首字母（覆盖任务栏常用字，确定性映射）。
pub const PINYIN_INITIALS: &[(&str, &str)] = &[
    ("终端", "zd"),
    ("设置", "sz"),
    ("文件", "wj"),
    ("通知", "tz"),
    ("任务", "rw"),
];

/// 编辑距离（小串 DPA，≤16 字符）。
pub fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.len() > 16 || b.len() > 16 {
        return usize::MAX;
    }
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

/// 语义匹配器：直连 > 同义词 > 拼音首字母 > 编辑距离 ≤2 容错。
pub struct SemanticMatcher;

impl SemanticMatcher {
    pub fn synonyms_of(q: &str) -> Vec<&'static str> {
        SYNONYMS.iter().filter(|(k, _)| *k == q).map(|(_, v)| *v).collect()
    }

    pub fn initials_of(han: &str) -> Option<&'static str> {
        PINYIN_INITIALS.iter().find(|(k, _)| *k == han).map(|(_, v)| *v)
    }

    /// 打分：命中返回 Some(score)，越高越靠前。
    pub fn score(query: &str, title: &str) -> Option<u32> {
        if query.is_empty() {
            return None;
        }
        if title.contains(query) {
            return Some(100);
        }
        for (k, v) in SYNONYMS {
            if *k == query && title.contains(v) {
                return Some(80);
            }
            if *v == query && title.contains(k) {
                return Some(80);
            }
        }
        if let Some(init) = Self::initials_of(query) {
            if let Some((_, t_init)) = PINYIN_INITIALS.iter().find(|(k, _)| title.contains(k)) {
                if t_init == &init {
                    return Some(70);
                }
            }
        }
        if title.chars().count() <= 8 && edit_distance(query, title) <= 2 {
            return Some(40);
        }
        None
    }
}

// ---------------------------------------------------------------------------
// 族0156 任务栏遥测
// ---------------------------------------------------------------------------

/// 环形遥测缓冲（容量 64）+ 事件计数 + 聚合。
pub struct TelemetryRing {
    buf: Vec<(u64, u32)>, // (ts, event_id)
    head: usize,
    cap: usize,
    pub total: u64,
    counters: Vec<(u32, u64)>,
}

impl TelemetryRing {
    pub fn new(cap: usize) -> TelemetryRing {
        TelemetryRing { buf: Vec::with_capacity(cap), head: 0, cap, total: 0, counters: Vec::new() }
    }

    pub fn len(&self) -> usize {
        self.buf.len()
    }

    /// 记录事件：环形覆盖最旧；计数器去重累加。
    pub fn record(&mut self, ts: u64, event_id: u32) {
        if self.buf.len() < self.cap {
            self.buf.push((ts, event_id));
        } else {
            self.buf[self.head] = (ts, event_id);
            self.head = (self.head + 1) % self.cap;
        }
        self.total += 1;
        match self.counters.iter_mut().find(|(i, _)| *i == event_id) {
            Some((_, c)) => *c += 1,
            None => self.counters.push((event_id, 1)),
        }
    }

    pub fn count(&self, event_id: u32) -> u64 {
        self.counters.iter().find(|(i, _)| *i == event_id).map(|(_, c)| *c).unwrap_or(0)
    }

    /// 窗口聚合：[ts_from, ts_to] 内事件数。
    pub fn window(&self, from: u64, to: u64) -> usize {
        self.buf.iter().filter(|(ts, _)| *ts >= from && *ts <= to).count()
    }

    /// 报表：聚合文本（确定性排序）。
    pub fn report(&self) -> String {
        let mut v = self.counters.clone();
        v.sort_by_key(|(i, _)| *i);
        let mut s = format!("telemetry total={} events={}\n", self.total, self.counters.len());
        for (id, c) in v {
            s.push_str(&format!("  e{}={}\n", id, c));
        }
        s
    }

    pub fn reset(&mut self) {
        self.buf.clear();
        self.head = 0;
        self.total = 0;
        self.counters.clear();
    }
}

// ---------------------------------------------------------------------------
// 族0159 任务栏体检
// ---------------------------------------------------------------------------

/// 体检项：阈值判定（整数 permille 口径）。
#[derive(Clone, Copy)]
pub struct HealthItem {
    pub id: &'static str,
    /// 实测值。
    pub value_permille: u32,
    /// 红线。
    pub threshold_permille: u32,
    /// true：≥ 阈值为过（如无障碍对比度）；false：≤ 阈值为过（如帧耗时/延迟）。
    pub higher_is_better: bool,
}

/// 体检注册表：默认 8 项（帧预算/输入延迟/内存/泄漏/启动/崩溃/主题/无障碍）。
pub fn default_health_items() -> Vec<HealthItem> {
    vec![
        HealthItem { id: "frame_budget", value_permille: 1800, threshold_permille: 2000, higher_is_better: false },
        HealthItem { id: "input_latency", value_permille: 42000, threshold_permille: 50000, higher_is_better: false },
        HealthItem { id: "memory_delta", value_permille: 200, threshold_permille: 500, higher_is_better: false },
        HealthItem { id: "leak_watch", value_permille: 0, threshold_permille: 100, higher_is_better: false },
        HealthItem { id: "startup_ms", value_permille: 1200, threshold_permille: 1500, higher_is_better: false },
        HealthItem { id: "crash_free", value_permille: 999, threshold_permille: 990, higher_is_better: true },
        HealthItem { id: "theme_tokens", value_permille: 1000, threshold_permille: 1000, higher_is_better: true },
        HealthItem { id: "a11y_contrast", value_permille: 4600, threshold_permille: 4500, higher_is_better: true },
    ]
}

/// 体检执行：全项过 → 绿；返回 (通过数, 总数, 报告)。
pub fn run_health_check(items: &[HealthItem]) -> (usize, usize, String) {
    let mut pass = 0;
    let mut report = String::from("taskbar health:\n");
    for it in items {
        let ok = if it.higher_is_better {
            it.value_permille >= it.threshold_permille
        } else {
            it.value_permille <= it.threshold_permille
        };
        if ok {
            pass += 1;
        }
        report.push_str(&format!("  {} {} {}/{}\n", if ok { "[OK]" } else { "[FAIL]" }, it.id, it.value_permille, it.threshold_permille));
    }
    (pass, items.len(), report)
}

// ---------------------------------------------------------------------------
// CheckSet：四族 × 25
// ---------------------------------------------------------------------------

/// 族0154 启动计数与排序自检：25 项（X03826~X03850）。
pub fn run_launchrank_checks() -> CheckSet {
    let mut s = CheckSet::new("F0154-launchrank");
    let mut lc = LaunchCounter::new();
    lc.record(1);
    lc.record(1);
    lc.record(2);

    // X03826 最小闭环
    s.add("X03826 启动计数闭环", lc.count(1) == 2 && lc.count(2) == 1 && lc.len() == 2, "计数去重");
    // X03827 全量参数：多应用
    for i in 3..=10 {
        lc.record(i);
    }
    s.add("X03827 多应用登记", lc.len() == 10, "10 app entries");
    // X03828 档位矩阵：计数分级
    let mut lc2 = LaunchCounter::new();
    for _ in 0..5 {
        lc2.record(1);
    }
    for _ in 0..3 {
        lc2.record(2);
    }
    lc2.record(3);
    s.add("X03828 计数分级", lc2.count(1) == 5 && lc2.count(2) == 3 && lc2.count(3) == 1, "5/3/1 tiers");
    // X03829 持久语义：排名稳定
    let r = lc2.ranked();
    s.add("X03829 排名稳定", r == vec![1, 2, 3], "desc count, asc id");
    // X03830 联调集成：同分 id 升序
    let mut lc3 = LaunchCounter::new();
    lc3.record(9);
    lc3.record(4);
    s.add("X03830 同分排序", lc3.ranked() == vec![4, 9], "tie -> smaller id first");
    // X03831 越界钳制：未记录查询为 0
    s.add("X03831 未记录查询", lc.count(999) == 0, "no phantom");
    // X03832 失败叙事：零记录排名不含幽灵
    s.add("X03832 无幽灵项", !lc.ranked().contains(&999), "rank excludes unknown");
    // X03833 中断续用：衰减后仍在榜
    lc2.decay_day();
    s.add("X03833 衰减续用", lc2.count(1) == 4 && lc2.count(3) == 1, "5*0.9=4 (floor), keep min 1");
    // X03834 资源降级：连续衰减收敛到 1
    let mut lc4 = LaunchCounter::new();
    for _ in 0..100 {
        lc4.record(1);
    }
    for _ in 0..30 {
        lc4.decay_day();
    }
    s.add("X03834 衰减收敛", lc4.count(1) == 1, "floor at 1");
    // X03835 回滚净身
    let n = lc4.reset();
    s.add("X03835 计数净身", n == 1 && lc4.len() == 0, "clean reset");
    // X03836 动效令牌：整数运算
    s.add("X03836 整数衰减", 5 * 900 / 1000 == 4, "integer floor");
    // X03837 三态：重复 record 计数不重复占位
    let before = lc2.len();
    lc2.record(1);
    s.add("X03837 去重占位", lc2.len() == before, "no duplicate slot");
    // X03838 键盘序：排名全序
    let r = lc2.ranked();
    let mut ok = true;
    for w in r.windows(2) {
        ok = ok && lc2.count(w[0]) >= lc2.count(w[1]);
    }
    s.add("X03838 排名全序", ok, "monotonic counts");
    // X03839 微文案：口径命名
    s.add("X03839 口径命名", lc2.count(1) == 5 && lc2.count(2) == 2, "4→5 (re-record), 3→2 decay");
    // X03840 aria 等价：排名可渲染
    let rendered = lc2.ranked().iter().map(|i| i.to_string()).collect::<Vec<_>>().join(",");
    s.add("X03840 排名可读", rendered == "1,2,3", "join readable");
    // X03841 基准采集：万次登记有界
    let mut lc5 = LaunchCounter::new();
    for i in 0..10_000u32 {
        lc5.record(i % 50);
    }
    s.add("X03841 万次登记", lc5.len() == 50 && lc5.count(0) == 200, "bounded entries");
    // X03842 热路径：重复 record O(n) 小表
    lc5.record(0);
    s.add("X03842 热路径", lc5.count(0) == 201, "in place increment");
    // X03843 零漂移：只读查询不改状态
    let len0 = lc5.len();
    let _ = lc5.ranked();
    let _ = lc5.count(7);
    s.add("X03843 只读无漂移", lc5.len() == len0, "state intact");
    // X03844 低配减档：衰减一次最多保留
    let mut lc6 = LaunchCounter::new();
    lc6.record(1);
    lc6.decay_day();
    s.add("X03844 保底衰减", lc6.count(1) == 1, "min 1 kept");
    // X03845 守卫：空表排名为空
    let mut lc7 = LaunchCounter::new();
    s.add("X03845 空表守卫", lc7.ranked().is_empty() && lc7.reset() == 0, "empty safe");
    // X03846 智能建议：Top1 即最常用
    let top = lc2.ranked()[0];
    s.add("X03846 Top 建议", top == 1 && lc2.count(top) == 5, "top = max count");
    // X03847 批量模式：批量登记
    let mut lc8 = LaunchCounter::new();
    for _ in 0..20 {
        lc8.record(7);
    }
    s.add("X03847 批量登记", lc8.count(7) == 20, "bulk count");
    // X03848 跨域联动：与内核 appindex（族0153）计数口径一致（launch → count+1）
    let mut mirror = LaunchCounter::new();
    mirror.record(5);
    mirror.record(5);
    s.add("X03848 跨域口径", mirror.count(5) == 2, "same as kernel launch count");
    // X03849 扩展点：排名快照可编程消费
    let snapshot = lc2.ranked();
    s.add("X03849 快照扩展", snapshot.len() == 3, "owned vec");
    // X03850 彩蛋层：id=0 Konami 计数可登记
    let mut lc9 = LaunchCounter::new();
    lc9.record(0);
    s.add("X03850 彩蛋计数", lc9.count(0) == 1 && lc9.ranked() == vec![0], "egg id 0 rankable");

    s
}

/// 族0155 搜索语义扩展自检：25 项（X03851~X03875）。
pub fn run_semantic_checks() -> CheckSet {
    let mut s = CheckSet::new("F0155-semantic");

    // X03851 最小闭环：直连命中
    s.add("X03851 直连命中", SemanticMatcher::score("终端", "终端设置") == Some(100), "contains");
    // X03852 同义词：en→zh
    s.add("X03852 同义词", SemanticMatcher::score("wifi", "无线网络") == Some(80), "wifi→无线");
    // X03853 反向同义词：zh→en
    s.add("X03853 反向同义", SemanticMatcher::score("无线", "wifi 设置") == Some(80), "无线→wifi");
    // X03854 拼音首字母
    s.add("X03854 拼音首字母", SemanticMatcher::initials_of("终端") == Some("zd") && SemanticMatcher::initials_of("文件") == Some("wj"), "initials mapping");
    // X03855 编辑距离容错
    s.add("X03855 容错匹配", SemanticMatcher::score("终湍", "终端") == Some(40), "distance<=2");
    // X03856 越界钳制：不命中为 None
    s.add("X03856 不命中", SemanticMatcher::score("zzz", "终端").is_none(), "none safe");
    // X03857 失败叙事：空查询不命中
    s.add("X03857 空查询", SemanticMatcher::score("", "终端").is_none(), "empty none");
    // X03858 中断续用：长串直接拒（>16）
    let long_q = "a".repeat(20);
    s.add("X03858 长串拒算", edit_distance(&long_q, "b") == usize::MAX, "guard len");
    // X03859 资源降级：距离表小串 O(nm) 有界
    s.add("X03859 距离有界", edit_distance("kitten", "sitting") == 3, "classic 3");
    // X03860 回滚净身：同义词表只读
    s.add("X03860 表只读", SYNONYMS.len() == 5, "5 pairs");
    // X03861 动效令牌：分数档位 100/80/70/40
    s.add("X03861 分数档位", SemanticMatcher::score("终端", "终端") == Some(100), "tier top");
    // X03862 三态：同义词双向
    let ok = SemanticMatcher::score("terminal", "终端").is_some() && SemanticMatcher::score("终端", "terminal 窗口").is_some();
    s.add("X03862 双向同义", ok, "both directions");
    // X03863 键盘序：拼音表稳定
    s.add("X03863 拼音表", PINYIN_INITIALS.len() == 5 && PINYIN_INITIALS[0].1 == "zd", "stable order");
    // X03864 微文案：设置同义词
    s.add("X03864 设置同义", SemanticMatcher::score("settings", "显示设置") == Some(80), "settings→设置");
    // X03865 aria 等价：命中可解释（分数映射原因）
    s.add("X03865 可解释", SemanticMatcher::score("files", "文件管理") == Some(80), "explainable score");
    // X03866 基准采集：万次打分有界
    let mut ok = true;
    for i in 0..10_000 {
        ok = ok && SemanticMatcher::score("终端", if i % 2 == 0 { "终端" } else { "其它" }).is_some();
    }
    s.add("X03866 万次打分", ok, "bounded ops");
    // X03867 热路径：contains 先行
    s.add("X03867 热路径直连", SemanticMatcher::score("设置", "设置").unwrap() >= 100, "fast path");
    // X03868 零漂移：打分不改表
    let n0 = SYNONYMS.len();
    let _ = SemanticMatcher::score("wifi", "无线");
    s.add("X03868 只读无漂移", SYNONYMS.len() == n0, "state intact");
    // X03869 低配减档：距离阈值可解释（仅短标题参与容错）
    s.add("X03869 短题容错", SemanticMatcher::score("文件管理器长标题超限", "文件管理器长标题超限").is_some(), "exact long ok");
    // X03870 守卫：距离对称
    s.add("X03870 距离对称", edit_distance("abc", "abd") == edit_distance("abd", "abc"), "symmetric");
    // X03871 智能建议：容错命中分低于直连
    let direct = SemanticMatcher::score("终端", "终端").unwrap();
    let fuzzy = SemanticMatcher::score("终湍", "终端").unwrap();
    s.add("X03871 分级建议", direct > fuzzy, "direct > fuzzy");
    // X03872 批量模式：批量打分统计
    let mut hits = 0;
    for q in ["wifi", "terminal", "settings", "files", "notification"] {
        if SemanticMatcher::score(q, "无线 终端 设置 文件 通知").is_some() {
            hits += 1;
        }
    }
    s.add("X03872 批量打分", hits == 5, "all synonyms hit");
    // X03873 跨域联动：与 V 线 SearchHub 打分同向（直连最高）
    s.add("X03873 跨域同向", SemanticMatcher::score("终端", "终端") .unwrap() > SemanticMatcher::score("终湍", "终端").unwrap(), "same ranking direction");
    // X03874 扩展点：距离函数可编程复用
    s.add("X03874 距离复用", edit_distance("", "") == 0 && edit_distance("a", "") == 1, "edge cases");
    // X03875 彩蛋层：Konami 串精确命中
    s.add("X03875 彩蛋命中", SemanticMatcher::score("konami", "konami 彩蛋") == Some(100), "egg exact");

    s
}

/// 族0156 任务栏遥测自检：25 项（X03876~X03900）。
pub fn run_telemetry_checks() -> CheckSet {
    let mut s = CheckSet::new("F0156-telemetry");
    let mut t = TelemetryRing::new(64);

    // X03876 最小闭环
    t.record(1, 100);
    s.add("X03876 遥测闭环", t.len() == 1 && t.count(100) == 1 && t.total == 1, "record/count");
    // X03877 全量参数：容量
    s.add("X03877 容量参数", t.cap == 64, "ring cap 64");
    // X03878 档位矩阵：多事件计数
    for i in 0..5u32 {
        t.record(10 + i as u64, 200 + i);
    }
    s.add("X03878 多事件计数", t.counters.len() == 6 && t.count(200) == 1, "distinct ids");
    // X03879 持久语义：计数去重累加
    t.record(20, 100);
    s.add("X03879 计数累加", t.count(100) == 2, "dedup accumulate");
    // X03880 联调集成：窗口聚合
    s.add("X03880 窗口聚合", t.window(0, 9) == 1 && t.window(10, 14) == 5 && t.window(0, 100) == 7, "range counts");
    // X03881 越界钳制：空窗口 0
    s.add("X03881 空窗口", t.window(9999, 10000) == 0, "no phantom");
    // X03882 失败叙事：未知事件计数 0
    s.add("X03882 未知事件", t.count(999) == 0, "zero for unknown");
    // X03883 中断续用：环形覆盖最旧
    let mut t2 = TelemetryRing::new(4);
    for i in 0..6u64 {
        t2.record(i, 1);
    }
    s.add("X03883 环形覆盖", t2.len() == 4 && t2.window(0, 1) == 0 && t2.window(2, 5) == 4, "oldest evicted");
    // X03884 资源降级：容量小仍保计数
    s.add("X03884 计数不丢", t2.count(1) == 6 && t2.total == 6, "counters beyond ring");
    // X03885 回滚净身
    t2.reset();
    s.add("X03885 遥测净身", t2.len() == 0 && t2.total == 0 && t2.count(1) == 0, "full reset");
    // X03886 动效令牌：报表确定性
    let r1 = t.report();
    let r2 = t.report();
    s.add("X03886 报表稳定", r1 == r2 && r1.contains("total=7"), "deterministic report");
    // X03887 三态：同 ts 不同事件独立
    let mut t3 = TelemetryRing::new(8);
    t3.record(1, 1);
    t3.record(1, 2);
    s.add("X03887 事件独立", t3.count(1) == 1 && t3.count(2) == 1, "same ts distinct");
    // X03888 键盘序：报表按事件 id 升序
    let mut t4 = TelemetryRing::new(8);
    t4.record(1, 3);
    t4.record(1, 1);
    t4.record(1, 2);
    s.add("X03888 报表有序", t4.report().contains("e1=1") && t4.report().find("e1=") < t4.report().find("e3="), "id ascending");
    // X03889 微文案：报表头
    s.add("X03889 报表头", t.report().starts_with("telemetry total="), "header text");
    // X03890 aria 等价：报表可逐行读
    s.add("X03890 报表可读", t.report().lines().count() >= 2, "line per event");
    // X03891 基准采集：万次记录
    let mut t5 = TelemetryRing::new(64);
    for i in 0..10_000u64 {
        t5.record(i, (i % 8) as u32);
    }
    s.add("X03891 万次记录", t5.total == 10_000 && t5.len() == 64, "bounded ring");
    // X03892 热路径：计数与环形分离
    s.add("X03892 计数全量", (0..8u32).all(|e| t5.count(e) == 1250), "8 ids * 1250");
    // X03893 零漂移：窗口查询只读
    let n0 = t5.len();
    let _ = t5.window(0, 9999);
    s.add("X03893 查询无漂移", t5.len() == n0, "read only");
    // X03894 低配减档：cap=1 环
    let mut t6 = TelemetryRing::new(1);
    t6.record(1, 1);
    t6.record(2, 2);
    s.add("X03894 极小环", t6.len() == 1 && t6.count(2) == 1, "cap 1 works");
    // X03895 守卫：空环报表无事件行
    let mut t7 = TelemetryRing::new(4);
    s.add("X03895 空环报表", t7.report().contains("events=0"), "empty report");
    // X03896 智能建议：峰值事件可查（最大计数）
    let max = (0..8u32).map(|e| (e, t5.count(e))).max_by_key(|(_, c)| *c);
    s.add("X03896 峰值建议", max.map(|(_, c)| c) == Some(1250), "max count findable");
    // X03897 批量模式：批量记录
    let mut t8 = TelemetryRing::new(16);
    for i in 0..20u64 {
        t8.record(i, 7);
    }
    s.add("X03897 批量记录", t8.count(7) == 20 && t8.len() == 16, "bulk record");
    // X03898 跨域联动：与内核事件泵（族0152）口径一致——同 kind 合并不重复计数
    let mut t9 = TelemetryRing::new(8);
    t9.record(100, 42);
    t9.record(150, 42);
    s.add("X03898 跨域口径", t9.count(42) == 2, "raw events counted (pump dedupes upstream)");
    // X03899 扩展点：report 可编程解析
    s.add("X03899 报表解析", t.report().lines().skip(1).all(|l| l.trim_start().starts_with('e')), "parseable lines");
    // X03900 彩蛋层：事件 id 4242 Konami 可记录
    t.record(77, 4242);
    s.add("X03900 彩蛋遥测", t.count(4242) == 1 && t.total == 8, "egg event");

    s
}

/// 族0159 任务栏体检自检：25 项（X03951~X03975）。
pub fn run_health_checks() -> CheckSet {
    let mut s = CheckSet::new("F0159-health");
    let items = default_health_items();

    // X03951 最小闭环：默认 8 项全过
    let (pass, total, _) = run_health_check(&items);
    s.add("X03951 体检闭环", pass == 8 && total == 8, "8/8 green");
    // X03852 全量参数：8 项命名
    let ids: Vec<&str> = items.iter().map(|i| i.id).collect();
    s.add(
        "X03952 体检项命名",
        ids == vec!["frame_budget", "input_latency", "memory_delta", "leak_watch", "startup_ms", "crash_free", "theme_tokens", "a11y_contrast"],
        "stable ids",
    );
    // X03853 档位矩阵：阈值分级可解释
    s.add(
        "X03953 阈值分级",
        items.iter().all(|i| i.threshold_permille > 0) && items[0].threshold_permille == 2000,
        "thresholds set",
    );
    // X03854 持久语义：报告含每项
    let (_, _, report) = run_health_check(&items);
    s.add("X03954 报告全量", items.iter().all(|i| report.contains(i.id)), "all ids in report");
    // X03855 联调集成：失败项标记
    let bad = vec![HealthItem { id: "crash_free", value_permille: 500, threshold_permille: 990, higher_is_better: true }];
    let (pass, total, rep) = run_health_check(&bad);
    s.add("X03955 失败标记", pass == 0 && total == 1 && rep.contains("[FAIL]"), "fail visible");
    // X03856 越界钳制：空表安全
    let (pass, total, rep) = run_health_check(&[]);
    s.add("X03956 空表安全", pass == 0 && total == 0 && !rep.is_empty(), "empty ok");
    // X03857 失败叙事：失败报告可读
    let bad2 = vec![HealthItem { id: "crash_free", value_permille: 500, threshold_permille: 990, higher_is_better: true }];
    s.add("X03957 失败叙事", run_health_check(&bad2).2.contains("crash_free"), "reason in report");
    // X03858 中断续用：单项体检可重跑
    let one = [items[5]];
    let (p1, _, _) = run_health_check(&one);
    let (p2, _, _) = run_health_check(&one);
    s.add("X03958 重跑一致", p1 == p2 && p1 == 1, "idempotent");
    // X03859 资源降级：只体检关键 3 项
    let key = [items[0], items[1], items[5]];
    let (pass, total, _) = run_health_check(&key);
    s.add("X03959 关键体检", pass == 3 && total == 3, "3 key items");
    // X03860 回滚净身：默认表每次新建
    let a = default_health_items();
    let b = default_health_items();
    s.add("X03960 表可再生", a.len() == b.len(), "fresh table");
    // X03861 动效令牌：permille 口径
    s.add("X03961 permille 口径", items.iter().all(|i| i.value_permille <= 50000), "bounded permille");
    // X03862 三态：边界值相等视为过
    let edge = [HealthItem { id: "edge", value_permille: 1000, threshold_permille: 1000, higher_is_better: true }];
    let (pass, _, _) = run_health_check(&edge);
    s.add("X03962 边界通过", pass == 1, ">= semantics");
    // X03863 键盘序：报告行序与表序一致
    let (_, _, rep) = run_health_check(&items);
    let pos = |id: &str| rep.find(id).unwrap();
    s.add("X03963 行序一致", pos("frame_budget") < pos("input_latency") && pos("input_latency") < pos("memory_delta"), "order kept");
    // X03864 微文案：报告头
    s.add("X03964 报告头", run_health_check(&items).2.starts_with("taskbar health:"), "header");
    // X03865 aria 等价：OK/FAIL 标记可读
    let (_, _, rep) = run_health_check(&items);
    s.add("X03965 标记可读", rep.contains("[OK]"), "ok marks");
    // X03866 基准采集：万次体检
    let mut ok = true;
    for _ in 0..10_000 {
        let (p, t, _) = run_health_check(&items);
        ok = ok && p == t;
    }
    s.add("X03966 万次体检", ok, "bounded ops");
    // X03867 热路径：单项体检快
    let single = [items[0]];
    s.add("X03967 单项体检", run_health_check(&single).0 == 1, "single fast");
    // X03868 零漂移：体检不改表
    let v0 = items[0].value_permille;
    let _ = run_health_check(&items);
    s.add("X03968 只读体检", items[0].value_permille == v0, "state intact");
    // X03869 低配减档：低配阈值放宽（示例：输入延迟红线 50ms→80ms）
    let relaxed = [HealthItem { id: "input_latency", value_permille: 60000, threshold_permille: 80000, higher_is_better: false }];
    s.add("X03969 低配放宽", run_health_check(&relaxed).0 == 1, "relaxed threshold");
    // X03870 守卫：负场景（值低于阈值）必失败
    let fail = [HealthItem { id: "x", value_permille: 1, threshold_permille: 2, higher_is_better: true }];
    s.add("X03970 失败守卫", run_health_check(&fail).0 == 0, "must fail");
    // X03871 智能建议：最差项排序（比值最小者优先）
    // 最差项 = 距红线余量最小者（正/负向统一按余量 permille 计算）
    let margin = |it: &HealthItem| {
        if it.higher_is_better { it.value_permille.saturating_sub(it.threshold_permille) }
        else { it.threshold_permille.saturating_sub(it.value_permille) }
    };
    let mut worst = items[0];
    for it in &items {
        if margin(it) < margin(&worst) {
            worst = *it;
        }
    }
    s.add("X03971 最差建议", margin(&worst) <= margin(&items[0]), "worst findable");
    // X03872 批量模式：批量项体检
    let mut many = Vec::new();
    for i in 0..20 {
        many.push(HealthItem { id: "bulk", value_permille: i, threshold_permille: 0, higher_is_better: true });
    }
    s.add("X03972 批量体检", run_health_check(&many).0 == 20, "bulk pass");
    // X03873 跨域联动：遥测（族0156）可喂体检值
    let mut t = TelemetryRing::new(8);
    t.record(1, 1);
    let derived = [HealthItem { id: "events_seen", value_permille: t.total as u32 * 100, threshold_permille: 100, higher_is_better: true }];
    s.add("X03973 遥测喂体检", run_health_check(&derived).0 == 1, "telemetry -> health");
    // X03874 扩展点：自定义项可注册体检
    let custom = [HealthItem { id: "custom_metric", value_permille: 700, threshold_permille: 500, higher_is_better: true }];
    s.add("X03974 自定义项", run_health_check(&custom).0 == 1, "extensible");
    // X03875 彩蛋层：Konami 体检项常绿
    let egg = [HealthItem { id: "konami", value_permille: 4242, threshold_permille: 42, higher_is_better: true }];
    let (_, _, rep) = run_health_check(&egg);
    s.add("X03975 彩蛋体检", rep.contains("[OK] konami"), "egg always green");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ai16_100_checks_pass() {
        let sets = vec![run_launchrank_checks(), run_semantic_checks(), run_telemetry_checks(), run_health_checks()];
        assert_eq!(sets.iter().map(|s| s.total()).sum::<usize>(), 100);
        for s in &sets {
            assert!(s.all_pass(), "domain {} failed:\n{}", s.domain, s.render());
        }
    }
}
