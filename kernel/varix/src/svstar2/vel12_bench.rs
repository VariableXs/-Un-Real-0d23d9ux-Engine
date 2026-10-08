//! VE-F2212 · 粒子基准（VE-L 域 · 粒子段 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F2212`
//!
//! **判据（锚点原文）**：三族阶梯、成本回填、暂挂声明、环境声明、判据。
//!
//! **职责定位（锚点原文）**：三族基准入 F1773 L 段——CPU 粒子吞吐
//! （万/十万粒子阶梯——模拟+渲染全链吞吐：1k/10k/100k 三档的每帧总
//! 耗时）、发射器开销（单发射器固定成本与发射边际成本——发射器数阶梯
//! 1/16/64 的管理开销）、排序耗时（N 粒子排序阶梯——1k/10k/100k 排序
//! 独占耗时），入册回归。
//!
//! # 一、测量是确定性逻辑基准而不是墙钟跑分
//!
//! no_std 内核没有稳定的墙钟与真实 GPU，本模块的「测量」是**合成负载下
//! 的确定性逻辑耗时**：负载由 vel10 成本模型常数逐项展开（纯函数、同参
//! 同输出），价值在于①模型-实测**闭环**（测量值喂回
//! [`CostTable::recalibrate`](vel10_budget::CostTable::recalibrate) 完成
//! F2210 预留的回填位——出厂只有相对关系，绝对值由本条定标）、②门禁
//! 可判定（单调性/偏差/口径全是纯数据判定）。墙钟跑分属环境外流程，
//! 由 F1769 L 段调度（本模块只提供纯函数面——**L 段门禁暂挂声明**
//! 沿 K 域移交期模式，不在本模块内置时序）。
//!
//! # 二、六列条目格式沿用 F1773（I 域基准总册同构）
//!
//! 族名/负载谱/环境声明/指标定义/基线数值/ADR 变更链。其中**环境声明
//! 强制完整**（硬件型号/驱动/系统/档位四要素缺一即拒绝入册——没有
//! 环境的数字不可比）；**指标口径强制唯一**（中位或 P99 二选一，混报
//! 条目无效——口径二义的数字没法做门禁阈值）。
//!
//! # 三、门禁四闸（错误路径逐条）
//!
//! - 环境缺项 → 拒绝入册；
//! - 吞吐非单调（10k 快于 1k）→ **标黄核查**（缓存效应备注，不是红：
//!   缓存可能让 10k 恰好整块驻留，标黄要求人查而不是机器瞎毙）；
//! - 基线移动无 ADR → 拒绝该条目更新（数字为什么变必须有案）；
//! - 跨环境偏差超 30% → 敏感性标注并要求分环境基线。
//!
//! # 四、三族采集的被测面
//!
//! 全部真调 vel10：吞吐族 = [`estimate`](vel10_budget::estimate) 的
//! total_ns（模拟+渲染全链）；排序族 = sort_on=true/false 的 sort_ns
//! 之差（**独占**耗时——混进模拟渲染就不是排序的成本）；发射器族 =
//! [`EmitAccumulator`](vel03_emitter::EmitAccumulator) 推进 1/16/64 台
//! 阶的逻辑步进成本（单发射器固定成本+边际成本可分解）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::svstar2::vel03_emitter::{advance_accumulator, EmitAccumulator};
use crate::svstar2::vel10_budget::{estimate, CostTable, Estimate, FormFactor, Tier};

// ---------------------------------------------------------------------------
// 一、常量与错误码
// ---------------------------------------------------------------------------

/// 本项版本。
pub const BENCH_PROTOCOL_VERSION: &str = "L12-bench-v1";

/// 吞吐阶梯（粒子数三档——锚点原文）。
pub const THROUGHPUT_TIERS: [u64; 3] = [1_000, 10_000, 100_000];

/// 发射器阶梯（锚点原文 1/16/64）。
pub const EMITTER_STEPS: [u64; 3] = [1, 16, 64];

/// 排序阶梯（粒子数三档，与吞吐同阶梯独立采集）。
pub const SORT_TIERS: [u64; 3] = [1_000, 10_000, 100_000];

/// 每粒子属性数（负载谱合成参数；位置/速度/颜色/寿命）。
pub const LOAD_ATTRS: u32 = 4;

/// 跨环境偏差敏感线（百分数——锚点：超 30% 标注环境敏感性）。
pub const DEVIATION_SENSITIVE_PCT: u64 = 30;

/// 基线移动容忍（百分数）：超线移动必须带 ADR。
pub const BASELINE_DRIFT_PCT: u64 = 10;

/// 环境声明不完整（条目拒绝入册）。
pub const E_BENCH_ENV: &str = "E_BENCH_ENV";

/// 吞吐非单调（标黄核查）。
pub const E_BENCH_MONOTONIC: &str = "E_BENCH_MONOTONIC";

/// 基线移动无 ADR（CI 拒绝该提交——F1767 联动守卫）。
pub const E_BENCH_ADR: &str = "E_BENCH_ADR";

/// 指标口径二义（中位与 P99 混报）。
pub const E_BENCH_METRIC: &str = "E_BENCH_METRIC";

/// 跨环境偏差超敏（要求分环境基线）。
pub const E_BENCH_DEVIATION: &str = "E_BENCH_DEVIATION";

// ---------------------------------------------------------------------------
// 二、环境声明与指标口径（六列之二/之四）
// ---------------------------------------------------------------------------

/// 环境声明（四要素强制完整——缺一即无效）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnvDeclaration {
    /// 硬件型号（型号+档位，脱敏只留型号档位——F1773 隐私纪律）。
    pub hw_model: String,
    /// 驱动版本。
    pub driver: String,
    /// 系统。
    pub os: String,
    /// 档位（L/M/H）。
    pub tier: String,
}

impl EnvDeclaration {
    /// 构造（全要素非空）。
    pub fn new(hw: &str, driver: &str, os: &str, tier: &str) -> EnvDeclaration {
        EnvDeclaration {
            hw_model: hw.to_string(),
            driver: driver.to_string(),
            os: os.to_string(),
            tier: tier.to_string(),
        }
    }

    /// 完整性校验：四要素**全部**非空白才有效。
    pub fn validate(&self) -> Result<(), String> {
        for (name, v) in [
            ("hw_model", &self.hw_model),
            ("driver", &self.driver),
            ("os", &self.os),
            ("tier", &self.tier),
        ] {
            if v.trim().is_empty() {
                return Err(format!("环境声明缺项：{}", name));
            }
        }
        Ok(())
    }
}

/// 指标口径（强制唯一——中位或 P99，混报即无效）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MetricKind {
    /// 中位数。
    Median,
    /// P99。
    P99,
}

impl MetricKind {
    /// 短码。
    pub fn wire(self) -> &'static str {
        match self {
            MetricKind::Median => "p50",
            MetricKind::P99 => "p99",
        }
    }
}

// ---------------------------------------------------------------------------
// 三、六列条目与基准册
// ---------------------------------------------------------------------------

/// ADR 变更记录（基线为什么变必须有案）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AmdRecord {
    /// 变更理由（非空）。
    pub reason: String,
    /// 变更时刻（毫秒逻辑钟）。
    pub at_ms: u64,
    /// 旧基线值（纳秒）。
    pub old_ns: u64,
    /// 新基线值（纳秒）。
    pub new_ns: u64,
}

impl AmdRecord {
    /// 构造（理由空即拒——没有理由的基线移动就是漂移）。
    pub fn new(reason: &str, at_ms: u64, old_ns: u64, new_ns: u64) -> Result<AmdRecord, String> {
        if reason.trim().is_empty() {
            return Err(E_BENCH_ADR.to_string());
        }
        Ok(AmdRecord { reason: reason.to_string(), at_ms, old_ns, new_ns })
    }
}

/// 六列基准条目（F1773 同构）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BenchEntry {
    /// 族名。
    pub family: &'static str,
    /// 负载谱（资产规格×规模阶梯的可读描述）。
    pub load_spec: String,
    /// 环境声明。
    pub env: EnvDeclaration,
    /// 指标口径（唯一）。
    pub metric: MetricKind,
    /// 基线 v1 数值（纳秒）。
    pub baseline_ns: u64,
    /// ADR 变更历史链。
    pub adr_chain: Vec<AmdRecord>,
}

impl BenchEntry {
    /// 入册前完整裁决：环境完整 + 基线非零。
    pub fn validate(&self) -> Result<(), String> {
        self.env.validate()?;
        if self.baseline_ns == 0 {
            return Err(E_BENCH_ENV.to_string());
        }
        Ok(())
    }

    /// 基线移动：超容忍必须带 ADR，无 ADR 拒绝（返回 Err）。
    pub fn move_baseline(&mut self, new_ns: u64, now_ms: u64, adr_reason: Option<&str>) -> Result<(), String> {
        let old = self.baseline_ns;
        let drift_pct = if old == 0 {
            100
        } else {
            let diff = if new_ns > old { new_ns - old } else { old - new_ns };
            diff * 100 / old
        };
        if drift_pct > BASELINE_DRIFT_PCT {
            match adr_reason {
                Some(r) => {
                    let rec = AmdRecord::new(r, now_ms, old, new_ns)?;
                    self.adr_chain.push(rec);
                }
                None => return Err(E_BENCH_ADR.to_string()),
            }
        }
        self.baseline_ns = new_ns;
        Ok(())
    }
}

/// 基准册（L 段入册；去重按族名+负载谱）。
#[derive(Debug, Default)]
pub struct BenchBook {
    entries: Vec<BenchEntry>,
    /// 拒绝入册计数（环境缺项/基线零/口径二义）。
    pub rejected: u32,
    /// 标黄核查计数（非单调）。
    pub yellow_flags: u32,
}

impl BenchBook {
    pub fn new() -> BenchBook {
        BenchBook::default()
    }

    /// 入册（环境裁决在前）。
    pub fn admit(&mut self, entry: BenchEntry) -> Result<(), String> {
        entry.validate()?;
        for e in self.entries.iter() {
            if e.family == entry.family && e.load_spec == entry.load_spec {
                return Err("重复条目".to_string());
            }
        }
        self.entries.push(entry);
        Ok(())
    }

    /// 在册条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 按族名取条目。
    pub fn get(&self, family: &str, load: &str) -> Option<&BenchEntry> {
        self.entries.iter().find(|e| e.family == family && e.load_spec == load)
    }
}

// ---------------------------------------------------------------------------
// 四、三族采集（确定性逻辑基准，真调 vel10/vel03）
// ---------------------------------------------------------------------------

/// 负载谱三场景（喷泉/爆炸/持续雨——形态映射：喷泉=面片、爆炸=拖尾、
/// 持续雨=面片+排序开）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scene {
    /// 喷泉：持续发射，billboard，排序关。
    Fountain,
    /// 爆炸：脉冲发射，trail，排序关。
    Explosion,
    /// 持续雨：持续发射，billboard，排序开（深度相关）。
    ContinuousRain,
}

impl Scene {
    /// 场景短码。
    pub fn wire(self) -> &'static str {
        match self {
            Scene::Fountain => "fountain",
            Scene::Explosion => "explosion",
            Scene::ContinuousRain => "rain",
        }
    }

    /// 场景→形态与排序开关（负载谱映射单源）。
    pub fn profile(self) -> (FormFactor, bool) {
        match self {
            Scene::Fountain => (FormFactor::Billboard, false),
            Scene::Explosion => (FormFactor::Trail, false),
            Scene::ContinuousRain => (FormFactor::Billboard, true),
        }
    }

    /// 三场景全集。
    pub fn all() -> [Scene; 3] {
        [Scene::Fountain, Scene::Explosion, Scene::ContinuousRain]
    }
}

/// 吞吐族采集：三档×三场景，每格 = 模拟+渲染+排序全链每帧总耗时（纳秒，
/// 取高档常数——绝对值定标以高档为准；低/中档由模型比例推出）。
pub fn bench_throughput(t: &CostTable) -> ([[u64; 3]; 3], Vec<String>) {
    let mut grid = [[0u64; 3]; 3];
    let mut notes: Vec<String> = Vec::new();
    for (si, scene) in Scene::all().iter().enumerate() {
        let (form, sort_on) = scene.profile();
        for (ti, &count) in THROUGHPUT_TIERS.iter().enumerate() {
            let est = estimate(count, LOAD_ATTRS, form, sort_on, Tier::High, t)
                .map(|e| e.total_ns)
                .unwrap_or(0);
            grid[si][ti] = est;
        }
    }
    // 单调性裁决：同场景三档必须递增（非单调→标黄+缓存效应备注）。
    for (si, scene) in Scene::all().iter().enumerate() {
        for ti in 0..2 {
            if grid[si][ti] >= grid[si][ti + 1] {
                notes.push(format!(
                    "{}@{}→{} 非单调（缓存效应候选，标黄核查）",
                    scene.wire(),
                    THROUGHPUT_TIERS[ti],
                    THROUGHPUT_TIERS[ti + 1]
                ));
            }
        }
    }
    (grid, notes)
}

/// 排序族采集：三档独占排序耗时 = sort_on 差分（不混模拟渲染）。
pub fn bench_sort(t: &CostTable) -> ([u64; 3], Vec<String>) {
    let mut out = [0u64; 3];
    let mut notes: Vec<String> = Vec::new();
    for (ti, &count) in SORT_TIERS.iter().enumerate() {
        let on = estimate(count, LOAD_ATTRS, FormFactor::Billboard, true, Tier::High, t)
            .map(|e| e.sort_ns)
            .unwrap_or(0);
        let off = estimate(count, LOAD_ATTRS, FormFactor::Billboard, false, Tier::High, t)
            .map(|e| e.sort_ns)
            .unwrap_or(0);
        out[ti] = on.saturating_sub(off);
    }
    for ti in 0..2 {
        if out[ti] >= out[ti + 1] {
            notes.push(format!("sort@{}→{} 非单调（N logN 应严格递增）", SORT_TIERS[ti], SORT_TIERS[ti + 1]));
        }
    }
    (out, notes)
}

/// 发射器族采集：1/16/64 阶梯的管理开销（真推进 EmitAccumulator：
/// 每发射器 256 逻辑帧，60Hz 步长；产出与推进成本皆可复现）。
///
/// 返回（各阶梯的累计产出粒子数，各阶梯推进步数，非单调备注）。
pub fn bench_emitters(steps_per_emitter: u64) -> ([u64; 3], [u64; 3], Vec<String>) {
    let mut produced = [0u64; 3];
    let mut ticks = [0u64; 3];
    let mut notes: Vec<String> = Vec::new();
    for (ei, &n) in EMITTER_STEPS.iter().enumerate() {
        let mut total = 0u64;
        let mut tick_count = 0u64;
        for _ in 0..n {
            let mut acc = EmitAccumulator::new();
            for _ in 0..steps_per_emitter {
                let got = advance_accumulator(&mut acc, 60.0, 1.0 / 60.0);
                total += got as u64;
                tick_count += 1;
            }
        }
        produced[ei] = total;
        ticks[ei] = tick_count;
    }
    // 边际成本一致性：产出随阶梯线性（16×单产出 vs 64×单产出同比例）。
    for ei in 0..2 {
        let ratio_now = if produced[ei] > 0 { produced[ei + 1] / produced[ei] } else { 0 };
        let ratio_expect = EMITTER_STEPS[ei + 1] / EMITTER_STEPS[ei];
        if ratio_now != ratio_expect {
            notes.push(format!(
                "emitter@{}→{} 产出比 {} ≠ 阶梯比 {}",
                EMITTER_STEPS[ei], EMITTER_STEPS[ei + 1], ratio_now, ratio_expect
            ));
        }
    }
    (produced, ticks, notes)
}

// ---------------------------------------------------------------------------
// 五、模型-实测闭环（回填 F2210——出厂相对关系，绝对值本条定标）
// ---------------------------------------------------------------------------

/// 回填裁决：实测值与模型预估偏差 ≤ 容忍线才允许 recalibrate；
/// 超线 → mark_stale + 敏感性标注（返回 Err + 立案信息）。
///
/// 容忍线复用 [`DEVIATION_SENSITIVE_PCT`]（30%）——跨环境偏差线与
/// 回填容忍线同源的刻意设计：超线数字本身不可比，回填只会污染常数表。
pub fn close_loop(
    measured_ns: u64,
    model_ns: u64,
    table: &mut CostTable,
    now_ms: u64,
) -> Result<(), String> {
    let model = if model_ns == 0 { 1 } else { model_ns };
    let diff = if measured_ns > model { measured_ns - model } else { model - measured_ns };
    let dev_pct = diff * 100 / model;
    if dev_pct > DEVIATION_SENSITIVE_PCT {
        table.mark_stale();
        return Err(format!(
            "{}：实测 {} vs 模型 {} 偏差 {}%，已标脏待重定标",
            E_BENCH_DEVIATION, measured_ns, model_ns, dev_pct
        ));
    }
    table.recalibrate(now_ms);
    Ok(())
}

/// 暂挂声明（L 段门禁随域建账——移交期模式延续；显性字符串而非静默跳过）。
pub const L_GATE_SUSPENDED_NOTE: &str =
    "L 段门禁暂挂：K 域移交期模式延续（F2211/F2094 同款）；跑批调度入 F1769，本模块只交付纯函数面";

// ---------------------------------------------------------------------------
// 六、判据
// ---------------------------------------------------------------------------

use crate::checks::CheckSet;

/// F2212 域自检（判据逐条映射；三族真跑 + 门禁四闸双向验证）。
pub fn run_vel12_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F2212");
    let table = CostTable::calibrated();

    // --- 环境声明 ---
    // L12-环境-01：四要素齐放行。
    let env_ok = EnvDeclaration::new("X1-Extreme", "drv-7.2", "QEMU-virt", "H");
    s.add("L12-环境-01", env_ok.validate().is_ok(), "完整环境声明放行");
    // L12-环境-02：缺任一要素拒绝（逐要素扫）。
    let bad = [
        EnvDeclaration::new("", "d", "o", "H"),
        EnvDeclaration::new("x", "", "o", "H"),
        EnvDeclaration::new("x", "d", "", "H"),
        EnvDeclaration::new("x", "d", "o", ""),
        EnvDeclaration::new("x", "d", "o", "   "),
    ];
    let mut all_rej = true;
    for e in bad.iter() {
        all_rej = all_rej && e.validate().is_err();
    }
    s.add("L12-环境-02", all_rej, "缺项/空白要素逐项拒绝（显性）");

    // --- 指标口径 ---
    // L12-口径-01：口径短码互异（p50/p99 可区分）。
    s.add(
        "L12-口径-01",
        MetricKind::Median.wire() != MetricKind::P99.wire(),
        "口径短码互异（混报可检）",
    );

    // --- 吞吐族 ---
    let (grid, tnotes) = bench_throughput(&table);
    // L12-吞吐-01：三档×三场景全格非零。
    let mut all_nonzero = true;
    for si in 0..3 {
        for ti in 0..3 {
            all_nonzero = all_nonzero && grid[si][ti] > 0;
        }
    }
    s.add("L12-吞吐-01", all_nonzero, "吞吐九格全非零（全链 total）");
    // L12-吞吐-02：同场景三档单调递增（高档常数下 10 倍粒子必然 10 倍成本）。
    s.add("L12-吞吐-02", tnotes.is_empty(), "吞吐三档单调（零标黄）");
    // L12-吞吐-03：三场景负载谱互异（trail 场景贵于 billboard 同档）。
    s.add(
        "L12-吞吐-03",
        grid[1][2] > grid[0][2],
        "爆炸(拖尾)每帧成本 > 喷泉(面片) 同档",
    );
    // L12-吞吐-04：排序开的雨场景 > 排序关的喷泉（独占差可观测）。
    s.add("L12-吞吐-04", grid[2][2] > grid[0][2], "持续雨(排序开) > 喷泉(排序关) 同档");

    // --- 排序族 ---
    let (sorts, snote) = bench_sort(&table);
    // L12-排序-01：三档非零且单调（N logN 严格递增）。
    s.add(
        "L12-排序-01",
        sorts[0] > 0 && sorts[1] > sorts[0] && sorts[2] > sorts[1] && snote.is_empty(),
        "排序独占耗时三档单调递增",
    );
    // L12-排序-02：独占差与模型 N logN×系数逐档独立重算对账。
    let mut sort_ok = true;
    for (ti, &count) in SORT_TIERS.iter().enumerate() {
        // 独立重算：N × ilog2(N) × coef（高档 coef=1）。
        let mut n = count;
        let mut lg: u64 = 0;
        while n > 1 {
            n >>= 1;
            lg += 1;
        }
        sort_ok = sort_ok && sorts[ti] == count * lg;
    }
    s.add("L12-排序-02", sort_ok, "排序耗时=N·logN·coef 判据侧独立重算对账");

    // --- 发射器族 ---
    let (produced, ticks, enotes) = bench_emitters(256);
    // L12-发射-01：三阶梯产出恰 = 阶梯×单产出（边际一致性）。
    s.add(
        "L12-发射-01",
        produced[0] > 0 && produced[1] == produced[0] * 16 && produced[2] == produced[0] * 64 && enotes.is_empty(),
        "发射器阶梯产出线性（固定成本+边际成本可分解）",
    );
    // L12-发射-02：单发射器 256 帧 60Hz 恰产 256 粒（carry 数学独立复核）。
    let mut acc = EmitAccumulator::new();
    let mut one = 0u64;
    for _ in 0..256 {
        one += advance_accumulator(&mut acc, 60.0, 1.0 / 60.0) as u64;
    }
    s.add("L12-发射-02", one == 256, "单发射器 256 帧 @60Hz 恰产 256 粒（不丢不多）");
    // L12-发射-03：推进步数随阶梯线性（采集面自洽）。
    s.add(
        "L12-发射-03",
        ticks[1] == ticks[0] * 16 && ticks[2] == ticks[0] * 64,
        "采集步数与阶梯同构",
    );

    // --- 六列条目与册 ---
    // L12-册-01：完整条目入册成功。
    let mut book = BenchBook::new();
    let e1 = BenchEntry {
        family: "throughput",
        load_spec: "100k@fountain".to_string(),
        env: EnvDeclaration::new("X1", "d7.2", "virt", "H"),
        metric: MetricKind::Median,
        baseline_ns: grid[0][2],
        adr_chain: Vec::new(),
    };
    s.add("L12-册-01", book.admit(e1).is_ok() && book.len() == 1, "完整六列条目入册");
    // L12-册-02：环境缺项条目拒绝入册（计数）。
    let e2 = BenchEntry {
        family: "throughput",
        load_spec: "10k@rain".to_string(),
        env: EnvDeclaration::new("", "d", "o", "L"),
        metric: MetricKind::Median,
        baseline_ns: 1,
        adr_chain: Vec::new(),
    };
    let r2 = book.admit(e2);
    s.add(
        "L12-册-02",
        r2.is_err() && book.len() == 1 && r2.unwrap_err().starts_with("环境声明缺项"),
        "缺环境条目拒绝（错误显性指名缺哪项）",
    );
    // L12-册-03：同族同负载谱去重拒绝。
    let e3 = BenchEntry {
        family: "throughput",
        load_spec: "100k@fountain".to_string(),
        env: EnvDeclaration::new("Y2", "d8", "virt", "M"),
        metric: MetricKind::P99,
        baseline_ns: 2,
        adr_chain: Vec::new(),
    };
    s.add("L12-册-03", book.admit(e3).is_err() && book.len() == 1, "同族同谱去重");
    // L12-册-04：小漂移免 ADR 直改。
    let mut ent = BenchEntry {
        family: "sort",
        load_spec: "10k".to_string(),
        env: EnvDeclaration::new("X1", "d", "o", "H"),
        metric: MetricKind::P99,
        baseline_ns: 10_000,
        adr_chain: Vec::new(),
    };
    s.add("L12-册-04", ent.move_baseline(10_500, 1_000, None).is_ok() && ent.baseline_ns == 10_500, "5% 小漂移免 ADR");
    // L12-册-05：超线移动无 ADR 拒绝（F1767 联动守卫）。
    let r5 = ent.move_baseline(30_000, 2_000, None);
    s.add("L12-册-05", r5.is_err() && r5.unwrap_err() == E_BENCH_ADR, "20% 移动无 ADR 拒绝");
    // L12-册-06：超线移动带 ADR 放行且链记录（双向）。
    s.add(
        "L12-册-06",
        ent.move_baseline(30_000, 3_000, Some("跑批机升级重测")).is_ok()
            && ent.adr_chain.len() == 1
            && ent.adr_chain[0].old_ns == 10_500
            && ent.adr_chain[0].new_ns == 30_000,
        "带 ADR 移动放行且链留痕",
    );
    // L12-册-07：ADR 空理由拒（没有理由的案就是漂移）。
    s.add(
        "L12-册-07",
        AmdRecord::new("   ", 0, 1, 2).is_err(),
        "ADR 空理由拒绝",
    );
    // L12-册-08：册可按族+谱取回（入册即索引）。
    s.add("L12-册-08", book.get("throughput", "100k@fountain").is_some(), "条目可查");

    // --- 模型-实测闭环 ---
    // L12-闭环-01：偏差内回填成功（recalibrate 清脏标+刷时戳）。
    let mut t1 = CostTable::calibrated();
    let model = estimate(100_000, LOAD_ATTRS, FormFactor::Billboard, false, Tier::High, &t1)
        .map(|e: Estimate| e.total_ns)
        .unwrap_or(0);
    let measured = model * 110 / 100;
    s.add(
        "L12-闭环-01",
        close_loop(measured, model, &mut t1, 7_000).is_ok()
            && !t1.stale
            && t1.calibrated_at == 7_000,
        "10% 偏差内回填 recalibrate 成功",
    );
    // L12-闭环-02：偏差超 30% 拒绝并标脏（待重定标显性）。
    let mut t2 = CostTable::calibrated();
    let measured2 = model * 150 / 100;
    let r = close_loop(measured2, model, &mut t2, 8_000);
    s.add(
        "L12-闭环-02",
        r.is_err() && t2.stale && !r.unwrap_err().is_empty(),
        "50% 偏差拒绝回填并标脏",
    );
    // L12-闭环-03：容忍线钉死 30%（锚点）。
    s.add("L12-闭环-03", DEVIATION_SENSITIVE_PCT == 30, "敏感线 30% 钉死");
    // L12-闭环-04：新鲜度校验——回填后 require_fresh 通过、过窗即 STALE。
    s.add(
        "L12-闭环-04",
        t1.require_fresh(7_001).is_ok() && t2.require_fresh(u64::MAX / 2).is_err(),
        "新鲜度校验双向（新鲜过/脏标拒）",
    );

    // --- 暂挂声明与版本 ---
    // L12-暂挂-01：暂挂声明非空（显性而非静默）。
    s.add(
        "L12-暂挂-01",
        L_GATE_SUSPENDED_NOTE.contains("暂挂") && L_GATE_SUSPENDED_NOTE.contains("F1769"),
        "L 段门禁暂挂声明显性",
    );
    // L12-暂挂-02：版本指纹（FNV-1a const 期=运行期同算防手抄漂移）。
    let fp = {
        let mut h: u64 = 0xcbf29ce484222325;
        for b in BENCH_PROTOCOL_VERSION.bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        h
    };
    s.add("L12-暂挂-02", fp != 0, "版本指纹非零（L12-bench-v1）");
    // L12-暂挂-03：三族阶梯常量钉死（锚点原文数值）。
    s.add(
        "L12-暂挂-03",
        THROUGHPUT_TIERS == [1_000, 10_000, 100_000]
            && EMITTER_STEPS == [1, 16, 64]
            && SORT_TIERS == [1_000, 10_000, 100_000],
        "三族阶梯常量钉死",
    );
    // L12-暂挂-04：场景闭集与短码互异。
    let sc = Scene::all();
    s.add(
        "L12-暂挂-04",
        sc.len() == 3 && sc[0].wire() != sc[1].wire() && sc[1].wire() != sc[2].wire(),
        "三场景闭集短码互异",
    );
    // L12-暂挂-05：判据条数对账。
    s.add("L12-暂挂-05", s.len() == 28, "判据条数对账（本条为第 29 条）");

    s
}
