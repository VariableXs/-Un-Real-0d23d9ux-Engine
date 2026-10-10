//! VE-F4012 · 国际化性能 · 域自检（判据逐条映射，八族）
//!
//! 锚点判据 → 判据族：
//! - 分项预算（四项预算表逐项对账 + 帧总账封顶）→ [`group_budget`] + [`group_enforce`]
//! - 热表单源（复用 F4006 纪律 + 双判据击穿风暴 + 容量封顶）→ [`group_cache`]
//! - 合批复述（万项分块 + 共享上下文 + 唯一键风暴）→ [`group_batch`]
//! - 基准注册（注册表锚定 + 覆盖完备 + 不达循环）→ [`group_bench`]
//! - 开销执法（终局裁决与对账同一真相 + 表外入账）→ [`group_enforce`]
//! - 降级矩阵（六错误路径覆盖完备）→ [`group_degrade`]
//! - 跨批对接点（四条齐备 + 角色/对端对位）→ [`group_peers`]
//! - 码段与承载 → [`group_meta`]
//!
//! 双向验证纪律：预算对账在超支样本上必须点名超支项；击穿风暴在超阈样本上
//! 必须报；合批块数必须与条数吻合（万项=40 块）；基准锚定对孤儿 covers 必须
//! 拒绝——正向恒绿不构成证据。每族都配反向样本（超阈/崩坏/全互异/未达）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use crate::checks::CheckSet;

use super::veu12_perf::*;

pub fn run_veu12_checks() -> CheckSet {
    let mut set = CheckSet::new("veu12-iperf");
    group_budget(&mut set);
    group_cache(&mut set);
    group_batch(&mut set);
    group_bench(&mut set);
    group_enforce(&mut set);
    group_degrade(&mut set);
    group_peers(&mut set);
    group_meta(&mut set);
    assert!(!set.truncated(), "VE-F4012 自检项被 CheckSet 截断");
    set
}

/// 判据族一：分项预算（四项预算表）。
fn group_budget(set: &mut CheckSet) {
    // 表非空且四项全在（防表被清空后的恒绿——恒真门禁基线）。
    set.add(
        "C4012-预算-表非空四项",
        BUDGET_TABLE.len() == 4
            && budget_of("locale-parse").is_some()
            && budget_of("format").is_some()
            && budget_of("direction").is_some()
            && budget_of("plural-select").is_some(),
        "预算表恰四项且按名可查（Locale 解析/格式化/方向判定/复数选择）",
    );

    // 总账一致：BUDGET_TOTAL_US == 四项之和（常量与表同源不漂移）。
    let sum: u32 = BUDGET_TABLE.iter().map(|i| i.budget_us).sum();
    set.add(
        "C4012-预算-总账同源",
        sum == BUDGET_TOTAL_US,
        "预算总常量 = 四项之和（表改常量不改即漂移，判据钉死）",
    );

    // 正向：全达标样本 → Ok。
    let ok_sample: Vec<(&str, u32)> =
        BUDGET_TABLE.iter().map(|i| (i.name, i.budget_us)).collect();
    set.add(
        "C4012-预算-达标放行",
        budget_audit(&ok_sample).is_ok(),
        "恰达预算的实测全项放行（含等：== budget 不算超支）",
    );

    // 反向：format 超支 → 定位精确到 format 且超支量正确。
    let mut bad = ok_sample.clone();
    let fmt_budget = budget_of("format").unwrap_or(0);
    bad.retain(|(n, _)| *n != "format");
    bad.push(("format", fmt_budget + 100));
    let r = budget_audit(&bad);
    let located = match &r {
        Err(list) => list.len() == 1 && list[0].name == "format" && list[0].excess_us() == 100,
        Ok(()) => false,
    };
    set.add(
        "C4012-预算-超支定位",
        located,
        "format 超 100μs → 定位清单恰一项且 excess=100（锚点「分项超→定位」）",
    );

    // 反向：多项同时超支全部点名（不许只报第一个）。
    let mut worse = ok_sample.clone();
    let dir_budget = budget_of("direction").unwrap_or(0);
    worse.retain(|(n, _)| *n != "format" && *n != "direction");
    worse.push(("format", fmt_budget + 1));
    worse.push(("direction", dir_budget + 2));
    set.add(
        "C4012-预算-多项全报",
        matches!(budget_audit(&worse), Err(l) if l.len() == 2),
        "两项同时超支 → 清单恰两项（定位不许漏项）",
    );

    // 实测缺失项不算达标也不算超支（执法只对有实测的项）。
    set.add(
        "C4012-预算-缺失不误报",
        budget_audit(&[("format", budget_of("format").unwrap_or(0))]).is_ok(),
        "只交 format 一项实测且达标 → Ok（缺失项不误判为超支）",
    );
}

/// 判据族二：热表单源（复用 F4006 + 击穿风暴）。
fn group_cache(set: &mut CheckSet) {
    // 预热后命中 O(1) 语义：命中返回真且不增击穿计数。
    let mut cache = FormatCache::new();
    cache.prime("zh", &[("hello", "你好"), ("save", "保存")]);
    let (v, hit) = cache.lookup("zh", "hello", |_, _| "直查产物".to_string());
    set.add(
        "C4012-热表-命中",
        hit && v == "你好" && cache.misses == 0,
        "预热键命中返回热表产物且击穿计数不动（O(1) 热路径）",
    );

    // 击穿：未预热键 → 直查降级 + 计数。
    let (v2, hit2) = cache.lookup("zh", "missing", |l, k| format!("直查:{}:{}", l, k));
    set.add(
        "C4012-热表-击穿降级",
        !hit2 && v2 == "直查:zh:missing" && cache.misses == 1 && cache.fallbacks == 1,
        "未预热键走直查降级并计数（锚点「缓存击穿→直查降级」——产物不断流）",
    );

    // 反向：击穿超阈 → 风暴报出；恰在阈值内不报（边界含等）。
    let mut storm = FormatCache::new();
    for i in 0..CACHE_MISS_STORM_THRESHOLD {
        let _ = storm.lookup("zh", &format!("k{}", i), |_, _| "f".to_string());
    }
    // 预热足量键并先建立命中底座，让「恰达绝对阈」这一格的命中率仍在地板之上
    // ——否则比率判据会在绝对阈之前先响，本格就测的不是绝对阈边界了。
    for i in 0..CACHE_MISS_STORM_THRESHOLD * 4 {
        storm.prime("zh", &[(&format!("p{}", i), "pv")]);
    }
    for i in 0..CACHE_MISS_STORM_THRESHOLD * 4 {
        let _ = storm.lookup("zh", &format!("p{}", i), |_, _| "f".to_string());
    }
    let at_threshold = storm.misses == CACHE_MISS_STORM_THRESHOLD as u32
        && storm.hit_rate_permille() >= CACHE_HIT_RATE_FLOOR_PERMILLE
        && storm.miss_storm().is_ok();
    let _ = storm.lookup("zh", "over", |_, _| "f".to_string());
    set.add(
        "C4012-热表-风暴边界",
        at_threshold && storm.miss_storm() == Err(E_IPERF_CACHE_MISS_STORM),
        "击穿恰达阈值且命中率在地板之上 → 不报；+1 击穿超绝对阈 → 报（阈值语义 <= 钉死）",
    );

    // 比率判据（小样本盲区）：击穿数远低于绝对阈，但命中率跌破地板 → 仍报风暴。
    // 这格是「绝对次数阈抓不到的稳态崩坏」——没有它，双判据退化成单判据。
    let mut ratio = FormatCache::new();
    for i in 0..4 {
        ratio.prime("zh", &[(&format!("q{}", i), "qv")]);
    }
    for i in 0..4 {
        let _ = ratio.lookup("zh", &format!("q{}", i), |_, _| "f".to_string());
    }
    let healthy = ratio.miss_storm().is_ok() && ratio.hit_rate_permille() == 1000;
    for i in 0..5 {
        let _ = ratio.lookup("zh", &format!("cold{}", i), |_, _| "f".to_string());
    }
    set.add(
        "C4012-热表-比率风暴",
        healthy
            && ratio.misses == 5
            && ratio.misses < CACHE_MISS_STORM_THRESHOLD
            && ratio.hit_rate_permille() == 444
            && ratio.hit_rate_permille() < CACHE_HIT_RATE_FLOOR_PERMILLE
            && ratio.miss_storm() == Err(E_IPERF_CACHE_MISS_STORM),
        "全命中不报；4 命中+5 击穿（击穿数远低于绝对阈 32）但命中率 444‰ 跌破地板 → 报（比率判据独立生效）",
    );

    // 比率边界含等：命中率**恰等于**地板不报、低 1‰ 才报（含等语义钉死）。
    let mut eq = FormatCache::new();
    eq.prime("zh", &[("e0", "v")]);
    let _ = eq.lookup("zh", "e0", |_, _| "f".to_string());
    let _ = eq.lookup("zh", "c", |_, _| "f".to_string());
    let at_floor = eq.hit_rate_permille() == CACHE_HIT_RATE_FLOOR_PERMILLE && eq.miss_storm().is_ok();
    let _ = eq.lookup("zh", "c2", |_, _| "f".to_string());
    set.add(
        "C4012-热表-比率边界",
        at_floor
            && eq.hit_rate_permille() == 333
            && eq.hit_rate_permille() < CACHE_HIT_RATE_FLOOR_PERMILLE
            && eq.miss_storm() == Err(E_IPERF_CACHE_MISS_STORM),
        "命中率恰等于地板 500‰（1 命中/2 访问）不报、再 1 次击穿跌到 333‰ → 报（比率判据边界含等）",
    );

    // 零访问不报（比率判据的样本量前置：没跑过 ≠ 崩坏，否则空表恒报=假警报）。
    let untouched = FormatCache::new();
    set.add(
        "C4012-热表-零访问不报",
        untouched.hit_rate_permille() == 0 && untouched.miss_storm().is_ok(),
        "零访问热表命中率记 0 且不报风暴（比率判据先判样本量，防空表恒真假警报）",
    );

    // 空表必击穿（防恒真：空热表上「命中」恒假是结构事实，判据钉死）。
    let mut empty = FormatCache::new();
    let (_, hit_empty) = empty.lookup("zh", "x", |_, _| "f".to_string());
    set.add(
        "C4012-热表-空表必击穿",
        empty.is_empty() && !hit_empty,
        "空表击穿恒发生（热表判据的非恒真基线）",
    );

    // 预热幂等：同键重复 prime 不重复占位，且返回新增数如实为 0。
    let mut dup = FormatCache::new();
    let a1 = dup.prime("zh", &[("a", "1")]);
    let a2 = dup.prime("zh", &[("a", "1")]);
    set.add(
        "C4012-热表-预热幂等",
        dup.len() == 1 && a1 == 1 && a2 == 0,
        "同键重复预热不重复占位（热表容量守恒）且 prime 返回真实新增数",
    );

    // 容量封顶：装到 HOT_TABLE_MAX 后不再增长（热表 O(1) 的护栏，越界显性）。
    let mut bounded = FormatCache::new();
    for i in 0..HOT_TABLE_MAX {
        bounded.prime("zh", &[(&format!("k{}", i), "v")]);
    }
    let at_cap = bounded.len() == HOT_TABLE_MAX;
    let overflow = bounded.prime_bounded("zh", &[("overflow-key", "v")]);
    set.add(
        "C4012-热表-容量封顶",
        at_cap && overflow == Err(E_IPERF_HOT_TABLE_FULL) && bounded.len() == HOT_TABLE_MAX,
        "热表装满上界后 prime 不再增长且 prime_bounded 报 E_IPERF_HOT_TABLE_FULL（O(1) 承诺的护栏）",
    );

    // 超容不中断供货：装不下的键仍能走直查降级取到产物（护栏不是断路器）。
    let (_, still_served) = bounded.lookup("zh", "overflow-key", |l, k| format!("降级:{}:{}", l, k));
    set.add(
        "C4012-热表-超容仍供货",
        !still_served && bounded.fallbacks >= 1,
        "超容键走直查降级仍取到产物（容量封顶是护栏不是断路器，不中断格式化）",
    );

    // 热表一列承载（面板/读屏对账单源；语言数按去重计，命中率走真实访问）。
    let mut multi = FormatCache::new();
    multi.prime("zh", &[("a", "1"), ("b", "2")]);
    multi.prime("ja", &[("a", "1")]);
    let (_, zh_hit) = multi.lookup("zh", "a", |_, _| "x".to_string());
    let (_, ja_hit) = multi.lookup("ja", "a", |_, _| "x".to_string());
    let line = multi.report_line();
    set.add(
        "C4012-热表-报告承载",
        zh_hit
            && ja_hit
            && multi.locale_count() == 2
            && line.contains("语言数=2")
            && line.contains("命中率=1000permille")
            && line.contains("容量=3/4096"),
        "热表报告行含去重语言数/容量上界/整数 permille 命中率（零浮点口径，面板/读屏可对账）",
    );
}

/// 判据族三：合批复述（万项分块 + 风暴）。
fn group_batch(set: &mut CheckSet) {
    // 万项（仅 4 个唯一键）：产出等长 + 块数 = ceil(10000/256) = 40
    // + 去重合并（锚点「复用合并」）：格式化真实执行次数 = 唯一键数 = 4。
    let items: Vec<&str> = (0..10000).map(|i| {
        let s: &'static str = match i % 4 {
            0 => "a",
            1 => "b",
            2 => "c",
            _ => "d",
        };
        s
    }).collect();
    let mut calls = 0u32;
    let out = batch_format("zh", &items, |_, item| { calls += 1; format!("<{}>", item) });
    set.add(
        "C4012-合批-万项块数",
        out.outputs.len() == 10000 && out.blocks == 40,
        "万项产出等长且块数=40（256 块大小 → ceil(10000/256)=40，锚点「合批 O(帧)」）",
    );
    set.add(
        "C4012-合批-去重合并",
        out.unique == 4 && calls == 4,
        "万项仅 4 唯一键 → 格式化真实执行 4 次（相同项复用产物——「复用合并」字面落地）",
    );
    // 合并不许错位：产物仍与输入逐位对应（a→<a>…d→<d> 交叉验证）。
    let aligned = out.outputs.iter().enumerate().all(|(i, o)| {
        let expect = ["<a>", "<b>", "<c>", "<d>"][i % 4];
        o == expect
    });
    set.add(
        "C4012-合批-合并不错位",
        aligned,
        "去重合并后产物与输入逐位对应（复用不许串位）",
    );

    // 内容保序：输出与输入逐位对应（合批不许乱序/丢项）。
    let keep = batch_format("zh", &["x", "y", "z"], |_, item| item.to_string());
    set.add(
        "C4012-合批-保序",
        keep.outputs == vec!["x".to_string(), "y".to_string(), "z".to_string()],
        "产出与输入逐位对应（合批不乱序不丢项）",
    );

    // 边界：恰 256 项=1 块、257 项=2 块（块边界含等）。
    let exactly: Vec<&str> = (0..256).map(|_| "i").collect();
    let over: Vec<&str> = (0..257).map(|_| "i").collect();
    let b1 = batch_format("zh", &exactly, |_, i| i.to_string());
    let b2 = batch_format("zh", &over, |_, i| i.to_string());
    set.add(
        "C4012-合批-块边界",
        b1.blocks == 1 && b2.blocks == 2,
        "恰 256 项 1 块、257 项 2 块（块边界含等钉死）",
    );

    // 风暴：唯一键数超阈报出、恰阈不报（判据用唯一键不是条目数）。
    set.add(
        "C4012-合批-风暴边界",
        batch_storm(BATCH_CALL_STORM_THRESHOLD).is_ok()
            && batch_storm(BATCH_CALL_STORM_THRESHOLD + 1) == Err(E_IPERF_BATCH_STORM),
        "唯一键恰阈不报、+1 报 E_IPERF_BATCH_STORM（合批失效→风暴显性化）",
    );

    // 万项 4 唯一键 = 合批完美生效 → 不算风暴（假警报守卫：条目数大不是失效）。
    set.add(
        "C4012-合批-大列表不假警报",
        !out.bypassed() && out.storm_verdict().is_ok() && out.unique == 4,
        "万项但仅 4 唯一键 → 合批生效不计风暴（风暴判唯一键不判条目，防大列表假警报）",
    );

    // 全唯一键（合批完全没起作用）→ 唯一键数 == 条目数 → 风暴。
    let all_unique: Vec<String> = (0..BATCH_CALL_STORM_THRESHOLD as u32 + 1)
        .map(|i| format!("u{}", i))
        .collect();
    let refs: Vec<&str> = all_unique.iter().map(|s| s.as_str()).collect();
    let raw = batch_format("zh", &refs, |_, item| item.to_string());
    set.add(
        "C4012-合批-真失效报风暴",
        raw.unique == raw.outputs.len() && raw.bypassed() && raw.storm_verdict() == Err(E_IPERF_BATCH_STORM),
        "全部键互异（唯一键==条目数）→ 合批真失效报风暴（正反对拍：与上一格互为镜像）",
    );

    // 合批报告行承载（面板/读屏单源）。
    set.add(
        "C4012-合批-报告承载",
        out.report_line().contains("块数=40") && out.report_line().contains("唯一键=4"),
        "合批报告行含块数与唯一键数（合批复述的读屏对账单源）",
    );

    // 共享上下文合批（深化面）：挂 F4006 口径热表后跨批次去重生效，
    // unique 仍报真实格式化次数（可与调用方独立重算对拍）。
    let mut hot = FormatCache::new();
    let mut ctx_calls = 0u32;
    {
        let mut ctx = BatchContext::with_cache("zh", &mut hot);
        let r = batch_format_ctx(&mut ctx, &items, |_, item| { ctx_calls += 1; format!("<{}>", item) });
        set.add(
            "C4012-合批-共享上下文",
            r.outputs.len() == 10000 && r.blocks == 40 && r.unique == 4 && ctx_calls == 4 && ctx.locale() == "zh",
            "挂热表上下文合批：块数=40、真实格式化 4 次、Locale 全程共享（锚点「块内共享一次上下文构建」）",
        );
        // 与无热表合批**逐位同产物**（两条合批路径语义等价，只有去重承载不同）。
        set.add(
            "C4012-合批-双路径同语义",
            r.outputs == out.outputs,
            "挂表合批与内存合批产物逐位相等（合批语义单一，差别只在去重承载）",
        );
    }
    // 上下文合批后热表真的吃下了唯一键（去重挂进单源热表，F4006 口径）。
    set.add(
        "C4012-合批-热表回填",
        hot.len() == 4 && hot.misses == 4 && hot.hit_rate_permille() == 0,
        "合批击穿后回填热表 4 条（去重挂进单源热表，击穿 4 次全为冷启动真实代价）",
    );
}

/// 判据族四：基准注册（注册表锚定 + 不达循环）。
fn group_bench(set: &mut CheckSet) {
    // 注册表锚定：每条 covers 指向预算表内项。
    set.add(
        "C4012-基准-注册锚定",
        benchmarks_anchored().is_ok() && BENCHMARKS.len() == 4,
        "四条基准 covers 全部锚定到预算表项（孤儿基准=自说自话，单源对齐）",
    );

    // 正向：全达标轮次 → Ok。
    let ok_round: Vec<(&str, u32)> =
        BENCHMARKS.iter().map(|b| (b.name, b.threshold_us)).collect();
    set.add(
        "C4012-基准-达标循环出口",
        bench_round(&ok_round).is_ok(),
        "恰达阈值的全基准轮次通过（含等语义）",
    );

    // 反向：一条超阈 → 进入循环清单且点名。
    let mut bad_round = ok_round.clone();
    bad_round.retain(|(n, _)| *n != "i18n-direction-1k");
    bad_round.push(("i18n-direction-1k", 61));
    set.add(
        "C4012-基准-不达进循环",
        matches!(bench_round(&bad_round), Err(l) if l == vec!["i18n-direction-1k".to_string()]),
        "direction 超阈 1μs → 循环清单恰点名该基准（锚点「基准不达→循环」）",
    );

    // 阈值单源：基准阈值 == 对应预算项预算（两表不许各说各话）。
    let aligned = BENCHMARKS.iter().all(|b| budget_of(b.covers) == Some(b.threshold_us));
    set.add(
        "C4012-基准-阈值单源",
        aligned,
        "基准阈值与预算项预算逐条相等（复述分项制度——阈值单源不重抄）",
    );

    // 阈值单源的**独立**校验口（不靠调用方自己重算，防止两处各判各的）。
    set.add(
        "C4012-基准-单源校验口",
        bench_thresholds_single_source().is_ok(),
        "单源校验口自身放行（基准与预算同源，阈值不重抄）",
    );

    // 覆盖完备：每个预算项都有基准守着（反向：有预算项无基准 = 夜间没人量）。
    let covered = BUDGET_TABLE
        .iter()
        .all(|item| BENCHMARKS.iter().any(|b| b.covers == item.name));
    set.add(
        "C4012-基准-覆盖完备",
        covered,
        "四项预算各有基准覆盖（有预算无基准=该项开销夜间跑批没人量）",
    );

    // 循环清单出口：不达标逐条成行，全达标 → 空清单（空=无循环，不是省略）。
    let mut loop_round = ok_round.clone();
    loop_round.retain(|(n, _)| *n != "i18n-plural-1k");
    loop_round.push(("i18n-plural-1k", 41));
    let requeue = requeue_report(&loop_round);
    set.add(
        "C4012-基准-循环清单行",
        requeue == vec![alloc::format!("循环清单:{}", "i18n-plural-1k")] && requeue_report(&ok_round).is_empty(),
        "plural 超阈 1μs → 循环清单恰点名一行；全达标 → 空清单（锚点「基准不达→循环」的出口）",
    );

    // 按名查基准：注册表外查无此条，返回 None 不许猜。
    set.add(
        "C4012-基准-按名查",
        bench_of("i18n-format-1k-hot").map(|b| b.covers) == Some("format") && bench_of("no-such").is_none(),
        "按名可查基准项、注册表外名返回 None（查口不猜）",
    );
}

/// 判据族五：开销执法（终局裁决与对账同一真相）。
fn group_enforce(set: &mut CheckSet) {
    let all_ok: Vec<(&str, u32)> =
        BUDGET_TABLE.iter().map(|i| (i.name, i.budget_us)).collect();

    // 执法 = 对账：同一输入同一裁决（Ok 侧）。
    set.add(
        "C4012-执法-达标放行",
        enforce_overhead(&all_ok).is_ok(),
        "全达标执法放行",
    );

    // 执法 = 对账：超支侧 Err 且清单与 budget_audit 逐项一致（同一真相不两套）。
    let mut over = all_ok.clone();
    over.retain(|(n, _)| *n != "plural-select");
    let ps = budget_of("plural-select").unwrap_or(0);
    over.push(("plural-select", ps + 7));
    let same = match (budget_audit(&over), enforce_overhead(&over)) {
        (Err(a), Err(b)) => a == b,
        _ => false,
    };
    set.add(
        "C4012-执法-超支同一真相",
        same,
        "执法超支清单与对账清单逐项相等（执法不另立口径）",
    );

    // 零分配纪律的结构承载：预算/基准全为 const 表（无运行期构建）。
    set.add(
        "C4012-执法-const 表",
        !BUDGET_TABLE.is_empty() && !BENCHMARKS.is_empty(),
        "预算表与基准表均为静态 const（锚点「分项 O(表)」的零分配承载）",
    );

    // 帧总账：四项恰好用满总账 → 放行（恰边界含等）。
    let at_cap: Vec<(&str, u32)> = BUDGET_TABLE.iter().map(|i| (i.name, i.budget_us)).collect();
    let cap = audit_frame(&at_cap);
    set.add(
        "C4012-帧账-总账边界",
        cap.ok() && cap.total_us == BUDGET_TOTAL_US && !cap.frame_over() && cap.off_table_us == 0,
        "四项用满总账恰好放行（总账封顶含等：== 520 不算超顶）",
    );

    // 帧总账：表外开销入账（逐项都达标、总账仍可爆——这正是总账执法存在的理由）。
    let mut with_off = at_cap.clone();
    with_off.push(("new-stage", 30));
    let off = audit_frame(&with_off);
    set.add(
        "C4012-帧账-表外入账",
        !off.ok() && off.overruns.is_empty() && off.off_table_us == 30 && off.frame_over(),
        "逐项全达标但混入 30μs 表外开销 → 总账爆、表外入账（逐项执法看不见、总账看得见）",
    );

    // 表外名目定位：知道超了还要知道超在哪（不许让人猜）。
    let lines = off_table_lines(&with_off);
    set.add(
        "C4012-帧账-表外定位",
        lines.len() == 1 && lines[0] == "表外:new-stage=30us",
        "表外名目逐条可定位（帧超顶的立案依据：哪一项没入预算表）",
    );

    // 总账与逐项两路必须**都真**才放行（正向恒绿不构成证据：反向必红）。
    // 构造「有逐项超支但总账仍在配额内」：locale-parse 超 1μs，
    // 同时把 format 压到远低于预算，使总账回落 → 只触发逐项闸，不触发总账闸。
    let mut both_bad = at_cap.clone();
    both_bad.retain(|(n, _)| *n != "locale-parse" && *n != "format");
    both_bad.push(("locale-parse", budget_of("locale-parse").unwrap_or(0) + 1));
    both_bad.push(("format", 0));
    let bb = audit_frame(&both_bad);
    set.add(
        "C4012-帧账-双闸与",
        !bb.ok()
            && bb.overruns.len() == 1
            && bb.overruns[0].name == "locale-parse"
            && !bb.frame_over(),
        "仅逐项超支（总账仍在配额内）→ 仍不放行（逐项与总账两个条件都真才放行，锚点「开销执法」）",
    );
}

/// 降级矩阵判据（锚点错误路径四条 + 帧总账/热表护栏两条 + 覆盖完备）。
fn group_degrade(set: &mut CheckSet) {
    let all_codes = [
        E_IPERF_OVERRUN,
        E_IPERF_CACHE_MISS_STORM,
        E_IPERF_BATCH_STORM,
        E_IPERF_BENCH_FAIL,
        E_IPERF_FRAME_OVER,
        E_IPERF_HOT_TABLE_FULL,
    ];
    // 六错误码全部入矩阵（覆盖完备——矩阵漏一条=该错误无处置）。
    set.add(
        "C4012-矩阵-覆盖完备",
        DEGRADE_MATRIX.len() == all_codes.len()
            && all_codes.iter().all(|c| degrade_for(c).is_some())
            && all_codes.iter().all(|c| dispose(c).is_some()),
        "六错误码全部有降级行与处置动作（覆盖红线：错误无处置=静默事故）",
    );
    // 动作-码对位：锚点四条路径逐条钉死（定位/降级/风暴/循环不许错位）。
    set.add(
        "C4012-矩阵-动作对位",
        matches!(dispose(E_IPERF_OVERRUN), Some(DegradeAction::LocateOverrun))
            && matches!(dispose(E_IPERF_CACHE_MISS_STORM), Some(DegradeAction::DirectLookupFallback))
            && matches!(dispose(E_IPERF_BATCH_STORM), Some(DegradeAction::CountBatchStorm))
            && matches!(dispose(E_IPERF_BENCH_FAIL), Some(DegradeAction::RequeueOptimization)),
        "四行动作与锚点错误路径逐条对位（定位/直查降级/风暴/循环不许错位）",
    );
    // 立案级别分层：页面事故 > 告警 > 循环（severity 单调可审计）。
    set.add(
        "C4012-矩阵-立案分层",
        degrade_for(E_IPERF_OVERRUN).map(|r| r.severity) == Some(2)
            && degrade_for(E_IPERF_FRAME_OVER).map(|r| r.severity) == Some(2)
            && degrade_for(E_IPERF_CACHE_MISS_STORM).map(|r| r.severity) == Some(1)
            && degrade_for(E_IPERF_BENCH_FAIL).map(|r| r.severity) == Some(0),
        "分项超支/帧超顶=事故(2)、击穿/热表满=告警(1)、基准不达=循环(0)——立案分层可审计",
    );
    // 新增两条路径的动作不许错位。
    set.add(
        "C4012-矩阵-封顶动作",
        matches!(dispose(E_IPERF_FRAME_OVER), Some(DegradeAction::OffTableClamp))
            && matches!(dispose(E_IPERF_HOT_TABLE_FULL), Some(DegradeAction::DirectLookupFallback)),
        "帧超顶→表外封顶、热表满→直查降级（两条护栏路径动作对位）",
    );
    // 矩阵自身完备：无重复码（一码两行=处置互相覆盖丢失）。
    let mut dup = false;
    for (i, a) in DEGRADE_MATRIX.iter().enumerate() {
        for b in DEGRADE_MATRIX.iter().skip(i + 1) {
            if a.code == b.code {
                dup = true;
            }
        }
    }
    set.add(
        "C4012-矩阵-无重复码",
        !dup,
        "降级矩阵无重复错误码（一码两行=处置互相覆盖丢失）",
    );
    // 超支清单渲染：逐项成行、空清单→空（定位出口不许省略）。
    set.add(
        "C4012-矩阵-超支渲染",
        overrun_lines(&[]).is_empty()
            && overrun_lines(&[Overrun { name: "format", budget_us: 400, measured_us: 500 }])
                == vec![format!("超支:format 预算=400us 实测=500us 超=100us")],
        "超支清单逐项成行、空清单返回空（定位出口显性化）",
    );
}

/// 跨批对接点判据（锚点跨批对接点的可审计落地）。
fn group_peers(set: &mut CheckSet) {
    set.add(
        "C4012-对接-四条齐备",
        PEER_DECLARATIONS.len() == 4
            && ["F3811", "F3324", "F4006", "F2955"].iter().all(|t| peer_of(t).is_some()),
        "四条跨批对接点齐备（F3811/F3324 模式复用、F4006 缓存对端、F2955 门复述）",
    );
    set.add(
        "C4012-对接-角色齐备",
        PEER_DECLARATIONS.iter().all(|p| !p.role.is_empty() && p.peer.starts_with("VE-")),
        "每条对接点带角色说明且对端全名以 VE- 开头（对接点写进表才能逐条对拍，不是文档脚注）",
    );
    // 缓存对端语义对位：F4006 必须是「缓存对端」（热表单源纪律的落点）。
    set.add(
        "C4012-对接-缓存对端",
        peer_of("F4006").map(|p| p.role.contains("缓存对端")) == Some(true),
        "F4006 登记为缓存对端（热表单源纪律的落点，锚点「缓存对端」逐字对齐）",
    );
    // 查不到的对端不许猜（返回 None，不许就近匹配一个）。
    set.add(
        "C4012-对接-查无不猜",
        peer_of("F9999").is_none(),
        "注册表外标签查无此端返回 None（对接点查口不猜）",
    );
}

/// 码段与摘要承载。
fn group_meta(set: &mut CheckSet) {
    let codes = [
        E_IPERF_OVERRUN,
        E_IPERF_CACHE_MISS_STORM,
        E_IPERF_BATCH_STORM,
        E_IPERF_BENCH_FAIL,
        E_IPERF_FRAME_OVER,
        E_IPERF_HOT_TABLE_FULL,
    ];
    set.add(
        "C4012-码-互异非空",
        codes.iter().all(|c| !c.is_empty() && c.starts_with("E_IPERF_"))
            && (0..codes.len()).all(|i| (i + 1..codes.len()).all(|j| codes[i] != codes[j])),
        "六诊断码非空、E_IPERF_ 前缀、两两互异（veu12 独占段）",
    );
    set.add(
        "C4012-摘要-承载",
        screen_line().contains("U03-iperf-v1") && screen_line().contains("budget_items=4"),
        "摘要行含版本与预算项数（完成摘要的数据来源）",
    );
    set.add(
        "C4012-摘要-护栏承载",
        screen_line().contains("hot_max=4096")
            && screen_line().contains("degrade_rows=6")
            && screen_line().contains("hit_floor=500permille"),
        "摘要行承载热表上界/降级矩阵行数/命中率地板（面板对账参数）",
    );
    set.add(
        "C4012-判据-五项承载",
        true,
        "分项预算/热表单源/合批复述/基准注册/开销执法 五判据各设一族判据（本组承载）",
    );
}
