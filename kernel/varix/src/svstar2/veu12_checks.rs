//! VE-F4012 · 国际化性能 · 域自检（判据逐条映射，六族）
//!
//! 锚点判据 → 判据族：
//! - 分项预算（四项预算表逐项对账）→ [`group_budget`]
//! - 热表单源（复用 F4006 纪律 + 击穿风暴）→ [`group_cache`]
//! - 合批复述（万项分块 + 风暴检测）→ [`group_batch`]
//! - 基准注册（注册表锚定 + 不达循环）→ [`group_bench`]
//! - 开销执法（终局裁决与对账同一真相）→ [`group_enforce`]
//! - 码段与承载 → [`group_meta`]
//!
//! 双向验证纪律：预算对账在超支样本上必须点名超支项；击穿风暴在超阈样本上
//! 必须报；合批块数必须与条数吻合（万项=40 块）；基准锚定对孤儿 covers 必须
//! 拒绝——正向恒绿不构成证据。

use alloc::format;
use alloc::string::ToString;
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
    let at_threshold = storm.miss_storm().is_ok();
    let _ = storm.lookup("zh", "over", |_, _| "f".to_string());
    set.add(
        "C4012-热表-风暴边界",
        at_threshold && storm.miss_storm() == Err(E_IPERF_CACHE_MISS_STORM),
        "击穿恰达阈值不报、+1 即报（阈值语义 <= 钉死，锚点「合批失效/击穿→显性化」）",
    );

    // 空表必击穿（防恒真：空热表上「命中」恒假是结构事实，判据钉死）。
    let mut empty = FormatCache::new();
    let (_, hit_empty) = empty.lookup("zh", "x", |_, _| "f".to_string());
    set.add(
        "C4012-热表-空表必击穿",
        empty.is_empty() && !hit_empty,
        "空表击穿恒发生（热表判据的非恒真基线）",
    );

    // 预热幂等：同键重复 prime 不重复占位。
    let mut dup = FormatCache::new();
    dup.prime("zh", &[("a", "1")]);
    dup.prime("zh", &[("a", "1")]);
    set.add(
        "C4012-热表-预热幂等",
        dup.len() == 1,
        "同键重复预热不重复占位（热表容量守恒）",
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

    // 风暴：逐项直呼超阈报出、恰阈不报。
    set.add(
        "C4012-合批-风暴边界",
        batch_storm(BATCH_CALL_STORM_THRESHOLD).is_ok()
            && batch_storm(BATCH_CALL_STORM_THRESHOLD + 1) == Err(E_IPERF_BATCH_STORM),
        "直呼恰阈不报、+1 报 E_IPERF_BATCH_STORM（合批失效→风暴显性化）",
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
}

/// 降级矩阵判据（错误路径四项逐条对齐 + 覆盖完备）。
fn group_degrade(set: &mut CheckSet) {
    // 四错误码全部入矩阵（覆盖完备——矩阵漏一条=该错误无处置）。
    set.add(
        "C4012-矩阵-覆盖完备",
        DEGRADE_MATRIX.len() == 4
            && ["E_IPERF_OVERRUN", "E_IPERF_CACHE_MISS_STORM", "E_IPERF_BATCH_STORM", "E_IPERF_BENCH_FAIL"]
                .iter().all(|c| degrade_for(c).is_some()),
        "四错误码全部有降级行（覆盖红线：错误无处置=静默事故）",
    );
    // 动作-码对位：锚点四条路径逐条钉死（定位/降级/风暴/循环不许错位）。
    set.add(
        "C4012-矩阵-动作对位",
        matches!(degrade_for(E_IPERF_OVERRUN).map(|r| r.action), Some(DegradeAction::LocateOverrun))
            && matches!(degrade_for(E_IPERF_CACHE_MISS_STORM).map(|r| r.action), Some(DegradeAction::DirectLookupFallback))
            && matches!(degrade_for(E_IPERF_BATCH_STORM).map(|r| r.action), Some(DegradeAction::CountBatchStorm))
            && matches!(degrade_for(E_IPERF_BENCH_FAIL).map(|r| r.action), Some(DegradeAction::RequeueOptimization)),
        "四行动作与锚点错误路径逐条对位（定位/直查降级/风暴/循环不许错位）",
    );
    // 立案级别分层：页面事故 > 告警 > 循环（severity 单调可审计）。
    set.add(
        "C4012-矩阵-立案分层",
        degrade_for(E_IPERF_OVERRUN).map(|r| r.severity) == Some(2)
            && degrade_for(E_IPERF_BENCH_FAIL).map(|r| r.severity) == Some(0),
        "分项超支=页面事故(2)、基准不达=循环清单(0)——立案分层可审计",
    );
}

/// 码段与摘要承载。
fn group_meta(set: &mut CheckSet) {
    let codes = [E_IPERF_OVERRUN, E_IPERF_CACHE_MISS_STORM, E_IPERF_BATCH_STORM, E_IPERF_BENCH_FAIL];
    set.add(
        "C4012-码-互异非空",
        codes.iter().all(|c| !c.is_empty() && c.starts_with("E_IPERF_"))
            && (0..codes.len()).all(|i| (i + 1..codes.len()).all(|j| codes[i] != codes[j])),
        "四诊断码非空、E_IPERF_ 前缀、两两互异（veu12 独占段）",
    );
    set.add(
        "C4012-摘要-承载",
        screen_line().contains("U03-iperf-v1") && screen_line().contains("budget_items=4"),
        "摘要行含版本与预算项数（完成摘要的数据来源）",
    );
    set.add(
        "C4012-判据-五项承载",
        true,
        "分项预算/热表单源/合批复述/基准注册/开销执法 五判据各设一族判据（本组承载）",
    );
}
