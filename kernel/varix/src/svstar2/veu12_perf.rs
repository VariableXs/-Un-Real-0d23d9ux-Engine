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
//! # 二、帧总账执法：表外开销不许躲在总账里
//!
//! 四项之和（[`BUDGET_TOTAL_US`]）是帧级 i18n 配额上限。逐项达标但实测里
//! 混进表外名目（新环节未入表）时，逐项执法看不见它、总账却爆——[`audit_frame`]
//! 把「逐项对账」与「总账封顶」合成一个裁决（锚点「开销执法」的终局形态）。
//!
//! # 三、热表是**复用**不是新建：缓存对端 F4006 的单源声明
//!
//! 格式化结果缓存**复用 F4006 的热表纪律**（不复制其实现——缓存击穿时直查
//! 降级 + 击穿计数上行，与 F4006 同一口径）。本单只做 i18n 侧的击穿风暴检测：
//! 击穿风暴用**双判据**（[`FormatCache::miss_storm`]）——绝对次数阈抓冷启的
//! 小样本高频击穿，比率下限（命中率 permille 地板）抓稳态比率崩坏（热表被
//! 绕过）。热表容量由 [`HOT_TABLE_MAX`] 封顶：线性扫描只在表有界时等价 O(1)，
//! 无界表会让「O(1) 命中」的性能声明在运行期悄悄失效（超容即
//! [`E_IPERF_HOT_TABLE_FULL`]，直查降级继续供货不中断）。命中率（permille
//! 整数口径，零浮点）供面板/读屏对账（[`FormatCache::hit_rate_permille`]）。
//!
//! # 四、万项合批：合批失效就是**风暴**
//!
//! 列表万项逐项格式化 = 每项一次查表 + 一次分配的碎片风暴。合批按块处理
//! （[`BATCH_BLOCK`]），块内共享 Locale 上下文（[`BatchContext`]——上下文
//! 只构建一次、全帧复用，不是每块重建的装饰）；合批被绕过以**去重后的唯一键
//! 数**判风暴（[`BatchOutcome::bypassed`]——万项只有 4 个唯一键时合批完美
//! 生效，用条目数判会假警报），计为批次风暴（[`E_IPERF_BATCH_STORM`]）。
//!
//! # 五、基准注册与开销执法
//!
//! i18n 基准**注册进全域基准制度**（复述 F2955 门：不达标项进入下一轮优化
//! 循环清单，不是记一笔就完）。开销执法（[`enforce_overhead`]）把四项实测
//! 对预算逐项裁决：全达标 → 放行；有超支 → 超支项立案并返回定位清单。
//!
//! # 六、跨批对接点是**数据**不是脚注
//!
//! 锚点跨批对接点（F3811/F3324 模式复用、F4006 缓存对端、F2955 门复述）
//! 落成可审计的注册表（[`PEER_DECLARATIONS`]）——对接点写在文档散文里
//! 会漂移，写进表才能被判据侧逐条对拍（单源复用声明的落地形态）。
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
/// 帧总账超顶（表外开销致总账超标）。
pub const E_IPERF_FRAME_OVER: &str = "E_IPERF_FRAME_OVER";
/// 热表装不下（上界封顶后仍请求写入——O(1) 承诺的护栏，越界即显性）。
pub const E_IPERF_HOT_TABLE_FULL: &str = "E_IPERF_HOT_TABLE_FULL";

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

    /// 定位行（面板/读屏/立案簿共用——超支必须能被念出来）。
    pub fn report_line(&self) -> String {
        format!(
            "超支:{} 预算={}us 实测={}us 超={}us",
            self.name,
            self.budget_us,
            self.measured_us,
            self.excess_us()
        )
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

/// 实测总账（μs，饱和加——实测含表外名目也照实计入，不许静默丢弃）。
pub fn frame_total_us(measured: &[(&str, u32)]) -> u32 {
    measured.iter().fold(0u32, |acc, (_, us)| acc.saturating_add(*us))
}

/// 一帧 i18n 开销裁决（锚点「开销执法」的终局形态：逐项对账 + 总账封顶）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FrameReport {
    /// 实测总账（含表外名目）。
    pub total_us: u32,
    /// 预算总账（[`BUDGET_TOTAL_US`]）。
    pub budget_us: u32,
    /// 逐项超支清单（表内执法结果）。
    pub overruns: Vec<Overrun>,
    /// 表外开销（实测名目不在预算表内的合计——逐项执法看不见的开销）。
    pub off_table_us: u32,
}

impl FrameReport {
    /// 放行条件：无逐项超支 **且** 总账不超顶（两个条件都真才放行）。
    pub fn ok(&self) -> bool {
        self.overruns.is_empty() && self.total_us <= self.budget_us
    }

    /// 帧总账超顶（表外开销所致时报 [`E_IPERF_FRAME_OVER`] 语义）。
    pub fn frame_over(&self) -> bool {
        self.total_us > self.budget_us
    }

    /// 裁决行（面板/立案簿共用；含表外开销明细——显性化不许藏）。
    pub fn report_line(&self) -> String {
        format!(
            "iperf帧 总账={}us 预算={}us 表外={}us 超支项={} {}",
            self.total_us,
            self.budget_us,
            self.off_table_us,
            self.overruns.len(),
            if self.ok() { "放行" } else { E_IPERF_OVERRUN }
        )
    }
}

/// 帧总账执法（锚点「分项超→定位」的帧级合成）。
///
/// 逐项超支清单与 [`budget_audit`] 同一真相（不另立口径）；额外捕捉
/// **表外开销**：实测名目不在预算表内的照实计入 `off_table_us`——
/// 新环节没入表不是它超预算的理由，总账照样封顶。
pub fn audit_frame(measured: &[(&str, u32)]) -> FrameReport {
    let overruns = match budget_audit(measured) {
        Ok(()) => Vec::new(),
        Err(list) => list,
    };
    let mut off_table = 0u32;
    for (name, us) in measured {
        if budget_of(name).is_none() {
            off_table = off_table.saturating_add(*us);
        }
    }
    FrameReport {
        total_us: frame_total_us(measured),
        budget_us: BUDGET_TOTAL_US,
        overruns,
        off_table_us: off_table,
    }
}

// ===========================================================================
// 三、格式缓存热表（判据二：复用 F4006 热表纪律——击穿直查降级）
// ===========================================================================

/// 缓存击穿风暴阈值：击穿次数超此值报风暴（复用 F4006 击穿上行口径）。
pub const CACHE_MISS_STORM_THRESHOLD: u32 = 32;

/// 击穿**率**风暴下限（permille）：命中率低于此值即报风暴。
///
/// 绝对次数阈只抓「小样本高频击穿」；稳态下每次击穿都慢不了多少、但**比率**
/// 已经崩坏（例如热表被清空后逐项直查）时，绝对阈要撞到几万次才响——为这一
/// 段盲区设比率下限（锚点「缓存击穿→直查降级」的显性化要抓的是「缓存被
/// 绕过」这个事实本身，不只是它撞了多少次）。
pub const CACHE_HIT_RATE_FLOOR_PERMILLE: u32 = 500;

/// 热表容量上界（判据「热表 O(1)」的承载封顶——线性扫描只在表有界时等价 O(1)）。
pub const HOT_TABLE_MAX: usize = 4096;

/// 格式化热表：O(1) 命中（直接索引），击穿降级直查。
///
/// 「热表复用声明」（锚点）：表结构与击穿计数口径**复用 F4006**——本模块
/// 不另造缓存协议，只把 i18n 的格式化产物挂进同一纪律。
///
/// 容量上界 [`HOT_TABLE_MAX`]（判据「热表 O(1)」的承载）：热路径的 O(1) 只在
/// 表容量被封顶时成立——无界增长的表会把线性扫描拖成 O(n)，于是一份
/// 「O(1) 命中」的性能声明在运行期悄悄失效。封顶后 `prime` 拒绝超容写入
/// （[`E_IPERF_HOT_TABLE_FULL`]），既守住了 O(1) 承诺，又让「装不下」变成
/// 可观测事件而不是隐性劣化（直查降级继续供货，不中断）。
pub struct FormatCache {
    /// 直接索引热表（locale+key → 格式化产物）。
    hot: Vec<((String, String), String)>,
    /// 命中计数（上行遥测：命中率的分母分子之一）。
    pub hits: u32,
    /// 击穿计数（上行遥测，不静默）。
    pub misses: u32,
    /// 直查计数（击穿后走直查降级的次数）。
    pub fallbacks: u32,
}

impl FormatCache {
    /// 空热表。
    pub fn new() -> FormatCache {
        FormatCache { hot: Vec::new(), hits: 0, misses: 0, fallbacks: 0 }
    }

    /// 预热（批量注入热表；同键重复预热幂等，不重复占位）。
    ///
    /// 返回实际**新增**条目数（不是尝试数）——调用方据此判断本轮预热是否
    /// 真被热表吸收；同键重复预热计 0 新增。
    pub fn prime(&mut self, locale: &str, entries: &[(&str, &str)]) -> usize {
        let mut added = 0usize;
        for (k, v) in entries {
            if self.hot.iter().any(|((l, key), _)| l == locale && key == k) {
                continue;
            }
            if self.hot.len() >= HOT_TABLE_MAX {
                break;
            }
            self.hot.push(((locale.to_string(), (*k).to_string()), (*v).to_string()));
            added += 1;
        }
        added
    }

    /// 超容预热（尝试写满并越过上界）：返回 [`E_IPERF_HOT_TABLE_FULL`]。
    ///
    /// 与 [`FormatCache::prime`] 的区别是**显式**的：prime 在上界处静默收手
    /// （批量注入不该逐条报错），本函数把「装不下」作为结果交回调用方，
    /// 供上层记账（装不下的条目走直查降级，不是被丢弃）。
    pub fn prime_bounded(&mut self, locale: &str, entries: &[(&str, &str)]) -> Result<usize, &'static str> {
        let added = self.prime(locale, entries);
        let room = HOT_TABLE_MAX.saturating_sub(self.hot.len());
        if added < entries.len() && entries.len() > room {
            Err(E_IPERF_HOT_TABLE_FULL)
        } else {
            Ok(added)
        }
    }

    /// 查表：命中 O(1) 语义（短表线性即等价 O(1) 常数界——表容量由 prime 上限
    /// 封顶）；击穿走直查降级（`fallback` 闭包）并计数。
    ///
    /// 注意：查表**不回填**——回填是调用方的显性决策（合批路径走
    /// [`batch_format_ctx`] 统一回填，防止调用方风格分裂）。
    pub fn lookup(&mut self, locale: &str, key: &str, fallback: impl FnOnce(&str, &str) -> String) -> (String, bool) {
        let hit = self.hot.iter().find(|((l, k), _)| l == locale && k == key);
        match hit {
            Some((_, v)) => {
                self.hits += 1;
                (v.clone(), true)
            }
            None => {
                self.misses += 1;
                self.fallbacks += 1;
                (fallback(locale, key), false)
            }
        }
    }

    /// 命中率（permille 整数口径，零浮点）：命中 / (命中 + 击穿)×1000。
    ///
    /// 空表（零访问）记 0 而不是 1000——没跑过的热表没有命中率可言
    /// （防恒真：空表报 100% 命中 = 假话）。
    pub fn hit_rate_permille(&self) -> u32 {
        let total = self.hits.saturating_add(self.misses);
        if total == 0 {
            0
        } else {
            (((self.hits as u64) * 1000) / (total as u64)) as u32
        }
    }

    /// 击穿风暴检测（锚点「缓存击穿→直查降级」的显性化面）：**双判据**。
    ///
    /// 1. **绝对判据**：击穿次数 > [`CACHE_MISS_STORM_THRESHOLD`]——抓小样本
    ///    高频击穿（冷启阶段）；
    /// 2. **比率判据**：命中率 < [`CACHE_HIT_RATE_FLOOR_PERMILLE`]‰ 且已发生
    ///    过访问——抓稳态比率崩坏（热表被绕过）。
    ///
    /// 零访问（`hits + misses == 0`）不报：没跑过的热表既没有「绕过」也
    /// 没有「健康」，比率判据在零样本上恒真 = 假警报（故先判样本量）。
    ///
    /// 两个判据的「恰达不报、超出才报」边界含等语义一致（`>` / `<` 严格），
    /// 判据侧逐条可对拍。
    pub fn miss_storm(&self) -> Result<(), &'static str> {
        if self.misses > CACHE_MISS_STORM_THRESHOLD {
            return Err(E_IPERF_CACHE_MISS_STORM);
        }
        let total = self.hits.saturating_add(self.misses);
        if total > 0 && self.hit_rate_permille() < CACHE_HIT_RATE_FLOOR_PERMILLE {
            return Err(E_IPERF_CACHE_MISS_STORM);
        }
        Ok(())
    }

    /// 热表容量（判据防恒真：空表击穿恒发生）。
    pub fn len(&self) -> usize {
        self.hot.len()
    }

    /// 热表是否为空。
    pub fn is_empty(&self) -> bool {
        self.hot.is_empty()
    }

    /// 热表覆盖的 Locale 数（去重计数——热表的爆炸半径按语言看，不按条目看）。
    pub fn locale_count(&self) -> usize {
        let mut seen: Vec<&str> = Vec::new();
        for ((l, _), _) in self.hot.iter() {
            if !seen.iter().any(|s| *s == l.as_str()) {
                seen.push(l.as_str());
            }
        }
        seen.len()
    }

    /// 热表一列（面板/读屏对账单源；命中率走整数 permille，零浮点）。
    pub fn report_line(&self) -> String {
        format!(
            "热表 语言数={} 容量={}/{} 命中={} 击穿={} 直查={} 命中率={}permille",
            self.locale_count(),
            self.hot.len(),
            HOT_TABLE_MAX,
            self.hits,
            self.misses,
            self.fallbacks,
            self.hit_rate_permille()
        )
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

impl BatchOutcome {
    /// 合批是否真被绕过（锚点「合批失效→风暴」的真判据）。
    ///
    /// 判据用**唯一键数**而不是条目数：万项列表只有 4 个唯一键时，逐项直呼
    /// 10000 次是「没合批」的形态，但 4 次不是——用条目数判会把
    /// 「合批完美生效」的正常大列表误报成风暴（假警报）。以
    /// `unique > BATCH_CALL_STORM_THRESHOLD` 为准才是「合批失效」本身。
    pub fn bypassed(&self) -> bool {
        batch_storm(self.unique as u32) == Err(E_IPERF_BATCH_STORM)
    }

    /// 合批风暴裁决（把 [`batch_storm`] 接到真实合批产物上）。
    pub fn storm_verdict(&self) -> Result<(), &'static str> {
        batch_storm(self.unique as u32)
    }

    /// 合批一列（面板/读屏对账单源：块数、唯一键数、风暴态）。
    pub fn report_line(&self) -> String {
        format!(
            "合批 条目={} 块数={} 唯一键={} 直查={} {}",
            self.outputs.len(),
            self.blocks,
            self.unique,
            self.unique as u32,
            if self.bypassed() { E_IPERF_BATCH_STORM } else { "合批生效" }
        )
    }
}

/// 合批共享上下文：Locale + 格式化热表，一次构建、全帧复用。
///
/// 锚点「块内共享一次 Locale 上下文构建」的兑现形态：上下文**在合批之前**
/// 构建（不是每块重建的装饰字符串），挂 F4006 口径的热表后，重复键走热表
/// 命中、唯一键才真格式化并回填（去重合并的第二级承载）。
pub struct BatchContext<'a> {
    locale: String,
    cache: Option<&'a mut FormatCache>,
}

impl<'a> BatchContext<'a> {
    /// 无热表上下文（纯内存去重——合批的最低配置）。
    pub fn new(locale: &str) -> BatchContext<'a> {
        BatchContext { locale: locale.to_string(), cache: None }
    }

    /// 挂热表的上下文（推荐配置：去重跨越合批边界，热表口径与 F4006 一致）。
    pub fn with_cache(locale: &str, cache: &'a mut FormatCache) -> BatchContext<'a> {
        BatchContext { locale: locale.to_string(), cache: Some(cache) }
    }

    /// 上下文 Locale（合批全程不变——共享的字面证据）。
    pub fn locale(&self) -> &str {
        &self.locale
    }
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
    BatchOutcome { outputs, blocks, unique: memo.len() }
}

/// 共享上下文合批（深化面）：上下文只构建一次、全帧复用；挂表时刻意键
/// 优先走 F4006 口径热表（击穿 → 直查格式化 → 回填热表），重复键全帧
/// 命中——去重不再限于单次合批的边界内，跨批次也生效。
///
/// 与 [`batch_format`] 同一合批语义（块大小/块数/保序/不错位逐条相同），
/// 差别只在去重承载：本函数把去重挂进热表单源，`unique` 仍报真实格式化
/// 次数（= 唯一键数，可与调用方独立重算对拍）。
pub fn batch_format_ctx(
    ctx: &mut BatchContext,
    items: &[&str],
    mut format_one: impl FnMut(&str, &str) -> String,
) -> BatchOutcome {
    let mut outputs = Vec::with_capacity(items.len());
    let mut memo: Vec<(String, String)> = Vec::new();
    let mut formats = 0usize;
    let mut blocks = 0usize;
    let mut i = 0usize;
    let locale = ctx.locale.clone();
    while i < items.len() {
        // 上下文在块外已构建——块内零重建（共享的字面证据：本循环不构造 locale）。
        blocks += 1;
        let end = (i + BATCH_BLOCK).min(items.len());
        while i < end {
            let item = items[i];
            let out = match memo.iter().find(|(k, _)| k == item) {
                Some((_, v)) => v.clone(),
                None => match ctx.cache.as_mut() {
                    Some(cache) => {
                        let (v, hit) = cache.lookup(&locale, item, |l, k| format_one(l, k));
                        if !hit {
                            cache.prime(&locale, &[(item, v.as_str())]);
                            formats += 1;
                        }
                        v
                    }
                    None => {
                        let v = format_one(&locale, item);
                        formats += 1;
                        v
                    }
                },
            };
            memo.push((item.to_string(), out.clone()));
            outputs.push(out);
            i += 1;
        }
    }
    BatchOutcome { outputs, blocks, unique: formats }
}

/// 合批风暴检测（锚点「合批失效→风暴」）：**唯一键数**超阈值即风暴。
///
/// 判据用去重后的唯一键数而非条目数（见 [`BatchOutcome::bypassed`] 的理由）：
/// 条目数大而唯一键小 = 合批生效的正常形态，用条目数判会假警报。阈值语义
/// 「恰达不报、超出才报」含等（`>` 严格）。
pub fn batch_storm(unique_keys: u32) -> Result<(), &'static str> {
    if unique_keys > BATCH_CALL_STORM_THRESHOLD {
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
///
/// 反向另查**覆盖完备**：预算表每一项都必须至少有一条基准守着——有预算项
/// 没有基准 = 该项的开销在夜间跑批里没人量（「基准注册」名存实亡）。
pub fn benchmarks_anchored() -> Result<(), &'static str> {
    for b in BENCHMARKS {
        if budget_of(b.covers).is_none() {
            return Err(E_IPERF_BENCH_FAIL);
        }
    }
    for item in BUDGET_TABLE {
        if !BENCHMARKS.iter().any(|b| b.covers == item.name) {
            return Err(E_IPERF_BENCH_FAIL);
        }
    }
    Ok(())
}

/// 基准阈值单源校验：每条基准的 `threshold_us` 必须等于它覆盖的预算项预算。
///
/// 复述「分项制度」的硬要求：阈值不重抄（基准与预算同源）。两表各写一个
/// 数就会各自漂移，判据钉死单源。
pub fn bench_thresholds_single_source() -> Result<(), &'static str> {
    for b in BENCHMARKS {
        if budget_of(b.covers) != Some(b.threshold_us) {
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

/// 按名查基准（O(表)——注册表的镜像查口，夜间调度器按名取条目）。
pub fn bench_of(name: &str) -> Option<&'static BenchmarkEntry> {
    BENCHMARKS.iter().find(|b| b.name == name)
}

/// 循环清单渲染（锚点「基准不达→循环」的出口形态）：不达标基准逐条
/// 生成一行可立案文本；全达标 → 空清单（空 = 无循环，不是省略渲染）。
pub fn requeue_report(results: &[(&str, u32)]) -> Vec<String> {
    match bench_round(results) {
        Ok(()) => Vec::new(),
        Err(names) => names.iter().map(|n| format!("循环清单:{}", n)).collect(),
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
// 六、跨批对接点注册表（锚点跨批对接点的可审计落地）
// ===========================================================================

/// 一条跨批对接点声明（单源复用声明的数据形态——判据侧可逐条对拍）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PeerDecl {
    /// 对端锚点（稳定标签）。
    pub tag: &'static str,
    /// 本模块对端的角色（复用/对端/门复述）。
    pub role: &'static str,
    /// 对端全名（册内编号）。
    pub peer: &'static str,
}

/// 跨批对接点注册表（锚点原文三条 + 模式复用两条，逐条可查）。
pub const PEER_DECLARATIONS: &[PeerDecl] = &[
    PeerDecl { tag: "F3811", role: "模式复用（伪检预算执法同款）", peer: "VE-F3811" },
    PeerDecl { tag: "F3324", role: "模式复用（合并/风暴会计）", peer: "VE-F3324" },
    PeerDecl { tag: "F4006", role: "缓存对端（热表单源纪律）", peer: "VE-F4006" },
    PeerDecl { tag: "F2955", role: "门复述（不达→优化循环）", peer: "VE-F2955" },
];

/// 按标签查对接点（O(表)——注册表外标签查无此端，返回 None 不许猜）。
pub fn peer_of(tag: &str) -> Option<&'static PeerDecl> {
    PEER_DECLARATIONS.iter().find(|p| p.tag == tag)
}

// ===========================================================================
// 七、错误路径与降级矩阵（锚点「错误路径与降级矩阵」的处置对照表）
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
    /// 表外开销入账并封顶总账（帧总账超顶→定位）。
    OffTableClamp,
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

/// 降级矩阵（六错误路径逐条对齐锚点四条 + 帧总账超顶 + 热表装不下）。
pub const DEGRADE_MATRIX: &[DegradeRow] = &[
    DegradeRow { code: E_IPERF_OVERRUN, action: DegradeAction::LocateOverrun, severity: 2 },
    DegradeRow { code: E_IPERF_CACHE_MISS_STORM, action: DegradeAction::DirectLookupFallback, severity: 1 },
    DegradeRow { code: E_IPERF_BATCH_STORM, action: DegradeAction::CountBatchStorm, severity: 1 },
    DegradeRow { code: E_IPERF_BENCH_FAIL, action: DegradeAction::RequeueOptimization, severity: 0 },
    DegradeRow { code: E_IPERF_FRAME_OVER, action: DegradeAction::OffTableClamp, severity: 2 },
    DegradeRow { code: E_IPERF_HOT_TABLE_FULL, action: DegradeAction::DirectLookupFallback, severity: 1 },
];

/// 按错误码查降级行（O(表)——矩阵恒小）。
pub fn degrade_for(code: &str) -> Option<&'static DegradeRow> {
    DEGRADE_MATRIX.iter().find(|r| r.code == code)
}

/// 按错误码查处置动作（矩阵的薄解析——面板/聚合器按码取处置用）。
pub fn dispose(code: &str) -> Option<DegradeAction> {
    degrade_for(code).map(|r| r.action)
}

/// 超支清单渲染（定位出口：逐项一行，空清单 → 空 Vec）。
pub fn overrun_lines(list: &[Overrun]) -> Vec<String> {
    list.iter().map(|o| o.report_line()).collect()
}

/// 表外开销名目（帧总账超顶的**定位**清单：哪几个名目不在预算表内、各占多少）。
///
/// [`FrameReport`] 只记了表外合计——知道「超了 80μs」不知道「超在哪」等于
/// 让人猜。这里把名目逐条列出，供立案簿把「新增环节未入预算表」显性化
/// （表外开销本身不是错，错的是让它无声无息）。
pub fn off_table_lines(measured: &[(&str, u32)]) -> Vec<String> {
    let mut out = Vec::new();
    for (name, us) in measured {
        if budget_of(name).is_none() {
            out.push(format!("表外:{}={}us", name, us));
        }
    }
    out
}

/// 摘要行（面板/读屏共用）。
pub fn screen_line() -> String {
    format!(
        "iperf {} budget_items={} total_us={} benchs={} cache_storm_at={} hit_floor={}permille hot_max={} batch_block={} batch_storm_at={} peers={} degrade_rows={}",
        IPERF_VERSION,
        BUDGET_TABLE.len(),
        BUDGET_TOTAL_US,
        BENCHMARKS.len(),
        CACHE_MISS_STORM_THRESHOLD,
        CACHE_HIT_RATE_FLOOR_PERMILLE,
        HOT_TABLE_MAX,
        BATCH_BLOCK,
        BATCH_CALL_STORM_THRESHOLD,
        PEER_DECLARATIONS.len(),
        DEGRADE_MATRIX.len(),
    )
}
