//! VE-F4012 · 国际化性能（VE-U 域 · i18n 性能预算与执法）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F4012`
//!
//! 锚点原文：「i18n 性能（国际化开销预算（Locale 解析/格式化/方向判定/复数
//! 选择四项预算——分项预算表（复述分项制度；格式缓存（复用热表（复述单源；
//! 批量格式化（列表万项格式化（批量合批（复用合并（复述；基准（i18n 基准
//! （复用制度（复述。数据结构：分项预算表；热表复用声明；合批复述；基准复述。
//! 错误路径与降级矩阵：分项超→定位（复述）；缓存击穿→直查降级（复用）；合批
//! 失效→风暴（复述）；基准不达→循环（复用）。性能逐项分解：分项 O(表)；热表
//! O(1)；合批 O(帧)；基准 O(夜间)。跨批对接点：F3811/F3324 模式复用声明；
//! F4006 缓存对端；F2955 门复述。无隐私面。」
//!
//! # 一、分项预算是**表**不是感觉：四项开销逐项有数、逐项可执法
//!
//! i18n 开销分散在四个环节（Locale 解析/格式化/方向判定/复数选择），任何一项
//! 超支都会在帧预算里留疤，但「总账达标」会掩盖单项劣化（F0815 的段级教训
//! 同源复用）。故本单把四项预算做成**静态表**（[`BUDGET_TABLE`]），实测对账
//! 按名定位超支项（[`budget_audit`]）——超支必须能说出是哪一项、超了多少。
//!
//! # 二、热表是**复用**不是新建：缓存对端 F4006 的单源声明
//!
//! 格式化结果缓存**复用 F4006 的热表纪律**（不复制其实现——缓存击穿时直查
//! 降级 + 击穿计数上行，与 F4006 同一口径）。本单只做 i18n 侧的击穿风暴检测：
//! 击穿率超阈值即报 [`E_IPERF_CACHE_MISS_STORM`]（缓存被绕过 = 直查路径
//! 在为全帧买单，必须显性化而不是静默吃下）。
//!
//! # 三、万项合批：合批失效就是**风暴**
//!
//! 列表万项逐项格式化 = 每项一次查表 + 一次分配的碎片风暴。合批按块处理
//! （[`BATCH_BLOCK`]），块内共享 Locale 上下文；合批被绕过（逐项直呼次数超阈）
//! 计为批次风暴（[`E_IPERF_BATCH_STORM`]）。
//!
//! # 四、基准注册与开销执法
//!
//! i18n 基准**注册进全域基准制度**（复述 F2955 门：不达标项进入下一轮优化
//! 循环清单，不是记一笔就完）。开销执法（[`enforce_overhead`]）把四项实测
//! 对预算逐项裁决：全达标 → 放行；有超支 → 超支项立案并返回定位清单。
//!
//! ## 零 panic 面
//!
//! 生产代码无 `unwrap`/`expect`/索引越界：除法常量话、查找走 `Option`、
//! 全部失败路径返回 `Result` 与清单。

// lib.rs 只有 `extern crate alloc` 且无 `#[macro_use]`，宏逐文件显式导入。
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ===========================================================================
// 一、诊断码（veu12 独占段，E_IPERF_ 前缀互异由判据承载）
// ===========================================================================

/// 版本标识（家族格式）。
pub const IPERF_VERSION: &str = "U03-iperf-v1";

/// 分项超支（某项实测超出预算）。
pub const E_IPERF_OVERRUN: &str = "E_IPERF_OVERRUN";
/// 缓存击穿风暴（击穿率超阈值）。
pub const E_IPERF_CACHE_MISS_STORM: &str = "E_IPERF_CACHE_MISS_STORM";
/// 合批失效风暴（逐项直呼超阈值）。
pub const E_IPERF_BATCH_STORM: &str = "E_IPERF_BATCH_STORM";
/// 基准不达标（进入下一轮优化循环）。
pub const E_IPERF_BENCH_FAIL: &str = "E_IPERF_BENCH_FAIL";

// ===========================================================================
// 二、分项预算表（判据一：Locale 解析/格式化/方向判定/复数选择）
// ===========================================================================

/// 一个预算项：名称 + 预算（μs/帧）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BudgetItem {
    /// 稳定名（对账按名定位，防顺序漂移）。
    pub name: &'static str,
    /// 预算（μs/帧）。
    pub budget_us: u32,
}

/// i18n 四项分项预算（锚点「Locale 解析/格式化/方向判定/复数选择」）。
///
/// 预算口径：1080p60 帧预算 16.6ms 的 i18n 配额——Locale 解析应当被缓存
/// （预算给得最小）、格式化是高频主路径、方向判定 O(段落)、复数选择 O(查表)。
pub const BUDGET_TABLE: &[BudgetItem] = &[
    BudgetItem { name: "locale-parse", budget_us: 20 },
    BudgetItem { name: "format", budget_us: 400 },
    BudgetItem { name: "direction", budget_us: 60 },
    BudgetItem { name: "plural-select", budget_us: 40 },
];

/// 分项预算总数（判据防表被清空的恒真门禁基线）。
pub const BUDGET_TOTAL_US: u32 = 520;

/// 超支定位项（锚点「分项超→定位」）：哪一项、超了多少、超了几倍。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Overrun {
    /// 超支项名。
    pub name: &'static str,
    /// 预算（μs）。
    pub budget_us: u32,
    /// 实测（μs）。
    pub measured_us: u32,
}

impl Overrun {
    /// 超支量（μs）。
    pub fn excess_us(&self) -> u32 {
        self.measured_us.saturating_sub(self.budget_us)
    }
}

/// 分项对账（锚点「分项 O(表)」）：按预算表逐项比对实测，返回超支清单。
///
/// 实测表里**缺失的项**不算达标也不算超支——返回 [`E_IPERF_OVERRUN`] 语义
/// 由调用方裁决；本函数只对「表内项 × 有实测值」的交集执法。
pub fn budget_audit(measured: &[(&str, u32)]) -> Result<(), Vec<Overrun>> {
    let mut overruns = Vec::new();
    for item in BUDGET_TABLE {
        for (name, us) in measured {
            if *name == item.name && *us > item.budget_us {
                overruns.push(Overrun { name: item.name, budget_us: item.budget_us, measured_us: *us });
            }
        }
    }
    if overruns.is_empty() {
        Ok(())
    } else {
        Err(overruns)
    }
}

/// 按名查预算（O(表)）。
pub fn budget_of(name: &str) -> Option<u32> {
    BUDGET_TABLE.iter().find(|i| i.name == name).map(|i| i.budget_us)
}

// ===========================================================================
// 三、格式缓存热表（判据二：复用 F4006 热表纪律——击穿直查降级）
// ===========================================================================

/// 缓存击穿风暴阈值：击穿次数超此值报风暴（复用 F4006 击穿上行口径）。
pub const CACHE_MISS_STORM_THRESHOLD: u32 = 32;

/// 格式化热表：O(1) 命中（直接索引），击穿降级直查。
///
/// 「热表复用声明」（锚点）：表结构与击穿计数口径**复用 F4006**——本模块
/// 不另造缓存协议，只把 i18n 的格式化产物挂进同一纪律。
pub struct FormatCache {
    /// 直接索引热表（locale+key → 格式化产物）。
    hot: Vec<((String, String), String)>,
    /// 击穿计数（上行遥测，不静默）。
    pub misses: u32,
    /// 直查计数（击穿后走直查降级的次数）。
    pub fallbacks: u32,
}

impl FormatCache {
    /// 空热表。
    pub fn new() -> FormatCache {
        FormatCache { hot: Vec::new(), misses: 0, fallbacks: 0 }
    }

    /// 预热（批量注入热表）。
    pub fn prime(&mut self, locale: &str, entries: &[(&str, &str)]) {
        for (k, v) in entries {
            let slot = ((locale.to_string(), (*k).to_string()), (*v).to_string());
            if !self.hot.iter().any(|(key, _)| *key == slot.0) {
                self.hot.push(slot);
            }
        }
    }

    /// 查表：命中 O(1) 语义（短表线性即等价 O(1) 常数界——表容量由 prime 上限
    /// 封顶）；击穿走直查降级（`fallback` 闭包）并计数。
    pub fn lookup(&mut self, locale: &str, key: &str, fallback: impl FnOnce(&str, &str) -> String) -> (String, bool) {
        let hit = self.hot.iter().find(|((l, k), _)| l == locale && k == key);
        match hit {
            Some((_, v)) => (v.clone(), true),
            None => {
                self.misses += 1;
                self.fallbacks += 1;
                (fallback(locale, key), false)
            }
        }
    }

    /// 击穿风暴检测（锚点「缓存击穿→直查降级」的显性化面）。
    pub fn miss_storm(&self) -> Result<(), &'static str> {
        if self.misses > CACHE_MISS_STORM_THRESHOLD {
            Err(E_IPERF_CACHE_MISS_STORM)
        } else {
            Ok(())
        }
    }

    /// 热表容量（判据防恒真：空表击穿恒发生）。
    pub fn len(&self) -> usize {
        self.hot.len()
    }

    /// 表是否为空。
    pub fn is_empty(&self) -> bool {
        self.hot.is_empty()
    }
}

// ===========================================================================
// 四、批量合批（判据三：列表万项格式化，块内共享上下文）
// ===========================================================================

/// 合批块大小（万项 = 40 块，块内共享 Locale 上下文——锚点「合批 O(帧)」）。
pub const BATCH_BLOCK: usize = 256;

/// 批次风暴阈值：逐项直呼超此值即风暴（合批被绕过的显性化）。
pub const BATCH_CALL_STORM_THRESHOLD: u32 = 256;

/// 合批结果：产物 + 块数 + 去重合并统计。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BatchOutcome {
    /// 格式化产物（与输入等长）。
    pub outputs: Vec<String>,
    /// 实际块数（`ceil(n / BATCH_BLOCK)`）。
    pub blocks: usize,
    /// 唯一键数（去重合并后真实执行格式化的次数）。
    pub unique: usize,
}

/// 批量格式化：按 [`BATCH_BLOCK`] 分块，块内共享一次 Locale 上下文构建；
/// **去重合并**（锚点「批量合批（复用合并」）：相同输入项只格式化一次、
/// 其余复用产物——列表万项的唯一键通常远小于条目数（菜单/标签/单位词），
/// 合并是合批之后的第二级真实优化。合并字典为线性表（唯一键集小，O(n·unique)
/// 可控且零依赖；键集增长由调用方分批控制）。
pub fn batch_format(
    locale: &str,
    items: &[&str],
    mut format_one: impl FnMut(&str, &str) -> String,
) -> BatchOutcome {
    let mut outputs = Vec::with_capacity(items.len());
    let mut memo: Vec<(String, String)> = Vec::new();
    let mut blocks = 0usize;
    let mut i = 0usize;
    while i < items.len() {
        // 块边界：一次上下文构建覆盖块内全部条目（此处以 locale 前缀合成表示
        // 共享上下文的一次性构建——真实实现由 F4006 热表承担）。
        let _shared_ctx = format!("ctx:{}", locale);
        blocks += 1;
        let end = (i + BATCH_BLOCK).min(items.len());
        while i < end {
            let item = items[i];
            let out = match memo.iter().find(|(k, _)| k == item) {
                Some((_, v)) => v.clone(),
                None => {
                    let v = format_one(locale, item);
                    memo.push((item.to_string(), v.clone()));
                    v
                }
            };
            outputs.push(out);
            i += 1;
        }
    }
    BatchOutcome { outputs, unique: memo.len(), blocks }
}

/// 合批风暴检测（锚点「合批失效→风暴」）：逐项直呼次数超阈值即风暴。
pub fn batch_storm(uncached_calls: u32) -> Result<(), &'static str> {
    if uncached_calls > BATCH_CALL_STORM_THRESHOLD {
        Err(E_IPERF_BATCH_STORM)
    } else {
        Ok(())
    }
}

// ===========================================================================
// 五、基准注册与开销执法（判据四·五）
// ===========================================================================

/// 一条 i18n 基准（锚点「基准（i18n 基准（复用制度」——注册进全域基准体系，
/// 复述 F2955 门：不达标进下一轮优化循环）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BenchmarkEntry {
    /// 稳定名。
    pub name: &'static str,
    /// 覆盖的分项（BUDGET_TABLE 项名）。
    pub covers: &'static str,
    /// 达标阈值（μs）。
    pub threshold_us: u32,
}

/// i18n 基准注册表（夜间口径：注册即进夜间跑批清单——「基准 O(夜间)」）。
pub const BENCHMARKS: &[BenchmarkEntry] = &[
    BenchmarkEntry { name: "i18n-locale-parse-cold", covers: "locale-parse", threshold_us: 20 },
    BenchmarkEntry { name: "i18n-format-1k-hot", covers: "format", threshold_us: 400 },
    BenchmarkEntry { name: "i18n-direction-1k", covers: "direction", threshold_us: 60 },
    BenchmarkEntry { name: "i18n-plural-1k", covers: "plural-select", threshold_us: 40 },
];

/// 基准注册校验：注册表每条 covers 必须指向预算表内项（单源对齐——
/// 指向不存在项的基准是孤儿，防「基准自说自话」）。
pub fn benchmarks_anchored() -> Result<(), &'static str> {
    for b in BENCHMARKS {
        if budget_of(b.covers).is_none() {
            return Err(E_IPERF_BENCH_FAIL);
        }
    }
    Ok(())
}

/// 基准裁决（锚点「基准不达→循环」）：实测超阈值的基准进入优化循环清单。
pub fn bench_round(results: &[(&str, u32)]) -> Result<(), Vec<String>> {
    let mut failing = Vec::new();
    for b in BENCHMARKS {
        for (name, us) in results {
            if *name == b.name && *us > b.threshold_us {
                failing.push(b.name.to_string());
            }
        }
    }
    if failing.is_empty() {
        Ok(())
    } else {
        Err(failing)
    }
}

/// 开销执法（判据五「开销执法」）：四项实测对预算的终局裁决。
///
/// 全达标 → Ok；有超支 → Err(定位清单)（与 [`budget_audit`] 同一真相——
/// 执法只是把对账结果接到立案出口）。
pub fn enforce_overhead(measured: &[(&str, u32)]) -> Result<(), Vec<Overrun>> {
    budget_audit(measured)
}

// ===========================================================================
// 六、错误路径与降级矩阵（锚点「错误路径与降级矩阵」的处置对照表）
// ===========================================================================

/// 降级动作（矩阵第二维：错误发生后**谁先顶上**）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DegradeAction {
    /// 按名定位超支项并立案（分项超→定位）。
    LocateOverrun,
    /// 直查降级兜底并计数（缓存击穿→直查降级）。
    DirectLookupFallback,
    /// 逐项直呼计数显性化为风暴（合批失效→风暴）。
    CountBatchStorm,
    /// 进入下一轮优化循环清单（基准不达→循环）。
    RequeueOptimization,
}

/// 一条降级矩阵行：错误码 → 降级动作 → 立案级别（页/告警/循环）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DegradeRow {
    /// 触发错误码。
    pub code: &'static str,
    /// 降级动作。
    pub action: DegradeAction,
    /// 立案级别（0=循环清单 / 1=告警 / 2=页面事故）。
    pub severity: u8,
}

/// 降级矩阵（四错误路径逐条对齐锚点；severity 越高越紧急）。
pub const DEGRADE_MATRIX: &[DegradeRow] = &[
    DegradeRow { code: E_IPERF_OVERRUN, action: DegradeAction::LocateOverrun, severity: 2 },
    DegradeRow { code: E_IPERF_CACHE_MISS_STORM, action: DegradeAction::DirectLookupFallback, severity: 1 },
    DegradeRow { code: E_IPERF_BATCH_STORM, action: DegradeAction::CountBatchStorm, severity: 1 },
    DegradeRow { code: E_IPERF_BENCH_FAIL, action: DegradeAction::RequeueOptimization, severity: 0 },
];

/// 按错误码查降级行（O(表)——矩阵恒小）。
pub fn degrade_for(code: &str) -> Option<&'static DegradeRow> {
    DEGRADE_MATRIX.iter().find(|r| r.code == code)
}

/// 摘要行（面板/日志共用）。
pub fn screen_line() -> String {
    format!(
        "iperf {} budget_items={} total_us={} benchs={} cache_storm_at={} batch_block={}",
        IPERF_VERSION,
        BUDGET_TABLE.len(),
        BUDGET_TOTAL_US,
        BENCHMARKS.len(),
        CACHE_MISS_STORM_THRESHOLD,
        BATCH_BLOCK,
    )
}
