//! VE-F1629 · 绘制调用批处理（draw call batching——状态排序/合并/预算）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F1629`
//!
//! **判据（锚点原文）**：状态排序、合并、预算、边界、判据。
//!
//! **职责定位（锚点原文）**：状态排序（材质/PSO 相似的 draw 排序——状态
//! 切换最小化：排序器把 draw 按材质/PSO 排序，PSO 切换是 GPU 最大开销
//! 之一，排序根治）；合并执行器（同 PSO 合并绘制——draw call 数下降量化：
//! 同 PSO 多 draw 合并为 instanced/multi-draw，下降量实测入册，收益透明）；
//! 批处理预算（每帧 draw call 预算——超预算告警：每帧 draw call 上限，
//! 超预算告警，预算告警是优化信号）；边界（批处理与实例化的关系：实例化
//! 在 I04 组，本组管非实例合并，边界声明）。
//!
//! 四件套由 [`run_batching`] 主管线总装：校验→稳定排序→同 PSO 合并→
//! 预算判定，一次产出 [`BatchingReport`] 全量化结论（切换/调用/成本/预算
//! 四项收益透明可审计）。
//!
//! ## 要点一：排序是根治不是缓解
//!
//! PSO（管线状态对象）切换是 GPU 最大开销之一。排序器把 draw 按
//! (PSO, 材质) 字典序重排，切换次数降到「去重 PSO 数 − 1」的理论
//! 下限——这是排序最优性的可判定形式，判据侧手算对拍。排序稳定：
//! 同键保提交序（seq 升序），不透明 draw 顺序无关、语义不失。
//!
//! 排序只是**一种**策略：切换最小化是策略目标而非唯一目标，故策略族
//! 三态封闭（[`SortStrategy`]）——`PsoFirst`（PSO 字典序，PSO 切换
//! 达下限）、`MaterialFirst`（材质字典序，材质绑定切换达下限）、
//! `RarestKeyFirst`（稀键前置：按键出现频次升序排，全键切换数最小）。
//! 三策略共用同一稳定平手规则（seq 升序），策略只决定键序不决定平手。
//!
//! ## 要点二：切换成本分档，计数不等于成本
//!
//! 只数切换次数会把 PSO 切换与材质切换记成同一等价物——这是量化的谎。
//! [`SwitchCost`] 给两档权重（PSO 切换权重严格大于材质切换权重），
//! [`switch_cost`] 按「PSO 变则计 PSO 档，材质变则计材质档，两档可叠加」
//! 出账，切换前后各测一次即 [`CostGain`]。权重是**参数不是事实**，
//! 故 [`SwitchCost::uniform`] 提供退化档供对拍（退化为纯计数）。
//!
//! ## 要点三：排序是置换，语义总量必须守恒
//!
//! 排序只重组状态面不重组语义面——这条断言不该靠人自觉。[`multiset_fp`]
//! 是顺序无关的内容指纹（逐 draw FNV 后 wrapping_add 折叠），排序前后
//! 指纹相等即「只换序不换内容」；不等即排序吞了或造了 draw，
//! [`BtCode::SORT_VIOLATION`] 立刻拦（[`verify_sort_preserves`]）。
//! 稳定排序的实现面自检也补齐：同键保 seq 提交序由 [`verify_order`]
//! 执行面校验（此前只在判据侧验，实现面自检只查键序不查保序——
//! 判据绿而实现面失守即门禁失效）。
//!
//! ## 要点四：合并只合同 PSO——不能猜着合
//!
//! 同 PSO 的连续 draw 合并为一次 multi-draw/instanced 调用；PSO
//! 不同硬合等于伪造管线状态（[`BtCode::MERGE_VIOLATION`]）。合并
//! 收益量化（before/after/saved）实测入册——收益透明才可审计。
//! 空绘制（index_count=0，被剔除的 draw）跳过不计批。
//!
//! 合并另有**容量上限**（[`MAX_BATCH_MEMBERS`]，与 F1628 indirect 批
//! 命令上限同量）：multi-draw 一次能带的成员数有界，触顶即**切批**而非
//! 硬塞——切批数记入 [`MergeGain::splits`] 透明入册。切出来的相邻批
//! 必是触顶切批（容量未满的同 PSO 相邻批=本可合并而未合并=执行器失守，
//! [`verify_merged_with`] 拦截）。
//!
//! 合并**不跨 PSO**，但合并**可以跨材质**：批内材质跨度由
//! [`MergedBatch::mats`] 逐材质记账显式携带，材质差异经描述符偏移表达
//! 而非增 draw call——这是**声明**不是猜测（[`MERGE_MATERIAL_NOTICE`]）：
//! 若目标后端的 multi-draw 不支持逐成员描述符偏移，调用方读该声明
//! 自知须退回分批，不得把跨度当作已白拿的收益。
//!
//! ## 要点五：预算是告警不是闸
//!
//! 每帧 draw call 预算（F1416 预算理念的渲染版）：超预算产告警
//! （over_by 透明），**不拒绘不砍画质**——预算是优化信号不是质量
//! 闸门。恰预算（等于上限）判内不越界。超预算按倍档分级
//! （[`BudgetSeverity`]）：刚超（`Over`）与倍档超标（`Severe`，calls ≥
//! 2×limit）——分级让告警可被下游遥测分档消费（F1638 五指标之一）。
//! 预算内余量由 [`FrameBudget::headroom`] 给出（饱和减，不回绕）。
//!
//! ## 要点六：边界声明钉死不越界
//!
//! 实例化策略归 I04 组；本组只做非实例合并（draw call 计数与状态
//! 面）。合并不拆不合实例语义——实例数原样携带；合并若放大实例
//! 容量即越界（[`BtCode::INSTANCE_FORBIDDEN`]）。
//!
//! ## 要点七：诊断码独占 0x4Exx 段
//!
//! 七码：draw 非法/排序违例/合并跨 PSO/预算非法/边界失配/实例越界/
//! 容量上限非法。与 vef31（0x4Dxx）、vef27（0x44xx）等互不重叠；
//! 码段判据用 `!=` 防自判死。
//!
//! 零 panic 面、零 IO、零墙钟、无全局可变状态、no_std 零 std 依赖。

use alloc::string::String;
use alloc::vec::Vec;
use core::cmp::Ordering;

/// F1629 版本溯源键。
pub const BATCH_VERSION: &str = "F32-batching-v1";

// ---------------------------------------------------------------------------
// 一、规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 单批最大成员数（multi-draw 一次能带的 draw 上限，与 F1628 indirect
/// 批命令上限 1024 同量——上游参数块放不下更多，合并器不许硬塞）。
pub const MAX_BATCH_MEMBERS: usize = 1024;

/// 每帧 draw call 预算默认上限（域内冻结）。
pub const DEFAULT_FRAME_BUDGET: u32 = 2_000;

/// 预算上限域硬界（防注入荒谬值——预算本身也要有预算）。
pub const MAX_FRAME_BUDGET: u32 = 100_000;

/// 排序策略族规模（封闭集——策略不得运行时增删）。
pub const STRATEGY_COUNT: usize = 3;

// ---------------------------------------------------------------------------
// 二、draw 描述与状态键
// ---------------------------------------------------------------------------

/// PSO 标识（排序第一键——PSO 切换是 GPU 最大开销之一，排头位）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct PsoId(pub u32);

/// 材质标识（排序第二键）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct MatId(pub u32);

/// 排序键：(PSO, 材质)——derive Ord 即字典序，PSO 优先。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct SortKey {
    pub pso: PsoId,
    pub mat: MatId,
}

/// 一条绘制调用（提交序即语义序）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DrawCall {
    /// 排序键（PSO × 材质）。
    pub key: SortKey,
    /// 索引/顶点数；0＝空绘制（被剔除，合法）。
    pub index_count: u32,
    /// 实例数（≥1；实例化策略归 I04 组，本组原样携带不拆不合）。
    pub instance_count: u32,
    /// 提交序号（稳定排序依据——同键保提交序）。
    pub seq: u32,
}

impl DrawCall {
    /// 参数域校验：实例数 ≥1（0 实例非绘制语义）。
    pub fn validate(&self) -> Result<(), BtCode> {
        if self.instance_count == 0 {
            return Err(BtCode::DRAW_INVALID);
        }
        Ok(())
    }

    /// 空绘制：索引数 0（被剔除的 draw——合并时跳过，不算真实调用）。
    pub fn is_empty(&self) -> bool {
        self.index_count == 0
    }
}

// ---------------------------------------------------------------------------
// 三、排序策略族（切换最小化是策略目标，故策略可换）
// ---------------------------------------------------------------------------

/// 排序策略（封闭三态）。策略只决定**键序**，平手一律按 seq 升序
/// ——策略不换稳定语义面。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SortStrategy {
    /// (PSO, 材质) 字典序——PSO 切换达「去重 PSO − 1」下限。
    PsoFirst,
    /// (材质, PSO) 字典序——材质绑定切换达下限。
    MaterialFirst,
    /// 稀键前置：按键出现频次升序排，全键切换数最小（同一 key 的
    /// draw 聚成一段，段数即切换数）。
    RarestKeyFirst,
}

impl SortStrategy {
    /// 策略族全集（封闭集——`STRATEGY_COUNT` 与本数组长度同源）。
    pub const ALL: [SortStrategy; STRATEGY_COUNT] = [
        SortStrategy::PsoFirst,
        SortStrategy::MaterialFirst,
        SortStrategy::RarestKeyFirst,
    ];

    /// 键间序（只对两个键定序，不涉平手）。
    pub fn cmp_keys(self, a: &SortKey, b: &SortKey) -> Ordering {
        match self {
            SortStrategy::PsoFirst => a.cmp(b),
            SortStrategy::MaterialFirst => a.mat.cmp(&b.mat).then_with(|| a.pso.cmp(&b.pso)),
            // 稀键前置的键序由频次决定，此处只提供确定性兜底序。
            SortStrategy::RarestKeyFirst => a.cmp(b),
        }
    }

    /// 人话名（呈现面）。
    pub fn name(self) -> &'static str {
        match self {
            SortStrategy::PsoFirst => "PSO 优先（切换达下限）",
            SortStrategy::MaterialFirst => "材质优先（绑定切换达下限）",
            SortStrategy::RarestKeyFirst => "稀键前置（全键切换最小）",
        }
    }
}

/// 键出现频次（稀键前置策略的排序输入）。
fn count_key(draws: &[DrawCall], key: SortKey) -> usize {
    let mut n = 0usize;
    for d in draws.iter() {
        if d.key == key {
            n += 1;
        }
    }
    n
}

/// 键序表：在案键按策略定序后的全序（未在案的键排在末位——
/// 排序后键集合不变，故此兜底分支只在脏输入下可达）。
pub fn strategy_order(strategy: SortStrategy, draws: &[DrawCall]) -> Vec<SortKey> {
    let mut uniq: Vec<SortKey> = Vec::new();
    for d in draws.iter() {
        if !uniq.iter().any(|k| *k == d.key) {
            uniq.push(d.key);
        }
    }
    match strategy {
        SortStrategy::RarestKeyFirst => uniq.sort_by(|a, b| {
            count_key(draws, *a)
                .cmp(&count_key(draws, *b))
                .then_with(|| a.cmp(b))
        }),
        other => uniq.sort_by(|a, b| other.cmp_keys(a, b)),
    }
    uniq
}

/// 键在序表中的秩（不在案者落末位）。
fn rank_of(order: &[SortKey], key: &SortKey) -> usize {
    let mut i = 0usize;
    while i < order.len() {
        if let Some(k) = order.get(i) {
            if k == key {
                return i;
            }
        }
        i += 1;
    }
    order.len()
}

// ---------------------------------------------------------------------------
// 四、状态排序（切换成本量化入册）
// ---------------------------------------------------------------------------

/// 相邻 PSO 切换次数（状态切换成本的可量化面）。
pub fn count_switches(draws: &[DrawCall]) -> usize {
    let mut switches = 0usize;
    let mut i = 1usize;
    while i < draws.len() {
        let prev = match draws.get(i - 1) {
            Some(d) => d,
            None => break,
        };
        let cur = match draws.get(i) {
            Some(d) => d,
            None => break,
        };
        if prev.key.pso != cur.key.pso {
            switches += 1;
        }
        i += 1;
    }
    switches
}

/// (PSO, 材质) 全键切换次数（材质切换成本面——PSO 之外的第二成本）。
pub fn count_key_switches(draws: &[DrawCall]) -> usize {
    let mut switches = 0usize;
    let mut i = 1usize;
    while i < draws.len() {
        let prev = match draws.get(i - 1) {
            Some(d) => d,
            None => break,
        };
        let cur = match draws.get(i) {
            Some(d) => d,
            None => break,
        };
        if prev.key != cur.key {
            switches += 1;
        }
        i += 1;
    }
    switches
}

/// 去重 PSO 数（切换下限的计算输入）。
pub fn distinct_pso_count(draws: &[DrawCall]) -> usize {
    let mut seen: Vec<PsoId> = Vec::new();
    for d in draws.iter() {
        if !seen.iter().any(|p| *p == d.key.pso) {
            seen.push(d.key.pso);
        }
    }
    seen.len()
}

/// 去重全键数（材质字典序策略下限的计算输入）。
pub fn distinct_key_count(draws: &[DrawCall]) -> usize {
    let mut seen: Vec<SortKey> = Vec::new();
    for d in draws.iter() {
        if !seen.iter().any(|k| *k == d.key) {
            seen.push(d.key);
        }
    }
    seen.len()
}

/// 排序器：按策略键序**稳定**排序（同键保 seq 提交序）。
///
/// 稳定是语义要求不是实现偏好：同键 draw 的相对次序必须保持提交序，
/// 排序只重组状态面不重组语义面。
pub fn sort_draws_with(draws: &mut [DrawCall], strategy: SortStrategy) {
    let order = strategy_order(strategy, draws);
    draws.sort_by(|a, b| {
        rank_of(&order, &a.key)
            .cmp(&rank_of(&order, &b.key))
            .then_with(|| a.seq.cmp(&b.seq))
    });
}

/// 域默认策略排序器（PSO 优先）。
pub fn sort_draws(draws: &mut [DrawCall]) {
    sort_draws_with(draws, SortStrategy::PsoFirst);
}

/// 排序结果自检：键序非降 **且** 同秩保 seq 提交序。
///
/// 键序与保序两条同时查——只查键序则「键对但序乱」的实现（不稳定排序）
/// 照样过检，稳定语义失守无人知。
pub fn verify_order(draws: &[DrawCall], strategy: SortStrategy) -> Result<(), BtCode> {
    let order = strategy_order(strategy, draws);
    let mut i = 1usize;
    while i < draws.len() {
        let prev = match draws.get(i - 1) {
            Some(d) => d,
            None => break,
        };
        let cur = match draws.get(i) {
            Some(d) => d,
            None => break,
        };
        let rp = rank_of(&order, &prev.key);
        let rc = rank_of(&order, &cur.key);
        if rp > rc {
            return Err(BtCode::SORT_VIOLATION);
        }
        if rp == rc && prev.seq > cur.seq {
            return Err(BtCode::SORT_VIOLATION);
        }
        i += 1;
    }
    Ok(())
}

/// 域默认策略下的排序自检（(PSO, 材质) 键序 + 保序）。
pub fn verify_sorted(draws: &[DrawCall]) -> Result<(), BtCode> {
    verify_order(draws, SortStrategy::PsoFirst)
}

// ---------------------------------------------------------------------------
// 五、切换成本模型（PSO 档 > 材质档，计数不等于成本）
// ---------------------------------------------------------------------------

/// 切换成本权重（分档记账——PSO 切换权重必须严格大于材质切换权重，
/// 否则等于把 GPU 最大开销之一记成与材质绑定同价）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SwitchCost {
    /// PSO 切换权重。
    pub pso: u32,
    /// 材质切换权重。
    pub mat: u32,
}

impl SwitchCost {
    /// 分档默认权重（PSO:材质 = 100:1——两个数量级的量级差）。
    pub fn default_cost() -> SwitchCost {
        SwitchCost { pso: 100, mat: 1 }
    }

    /// 均匀权重（退化档：两档同价，成本退化为纯切换计数——供对拍）。
    pub fn uniform() -> SwitchCost {
        SwitchCost { pso: 1, mat: 1 }
    }

    /// 分档是否成立（PSO 档严格大于材质档；否则权重配错，排序会为省
    /// 材质切换而放任 PSO 反复切换——与锚点「PSO 切换是最大开销」相悖）。
    pub fn tiered(&self) -> bool {
        self.pso > self.mat
    }

    /// 相邻一对 draw 的切换成本（PSO 变计 PSO 档，材质变计材质档，
    /// 两档可叠加——同 PSO 换材质不豁免材质档）。
    pub fn pair_cost(&self, prev: &DrawCall, cur: &DrawCall) -> u32 {
        let mut c = 0u32;
        if prev.key.pso != cur.key.pso {
            c = c.saturating_add(self.pso);
        }
        if prev.key.mat != cur.key.mat {
            c = c.saturating_add(self.mat);
        }
        c
    }
}

/// 序列切换总成本（相邻转移求和，饱和加防溢出回绕）。
pub fn switch_cost(draws: &[DrawCall], cost: &SwitchCost) -> u32 {
    let mut total = 0u32;
    let mut i = 1usize;
    while i < draws.len() {
        let prev = match draws.get(i - 1) {
            Some(d) => d,
            None => break,
        };
        let cur = match draws.get(i) {
            Some(d) => d,
            None => break,
        };
        total = total.saturating_add(cost.pair_cost(prev, cur));
        i += 1;
    }
    total
}

/// 切换成本收益（排序前后各测一次）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CostGain {
    /// 排序前总成本。
    pub before: u32,
    /// 排序后总成本。
    pub after: u32,
}

impl CostGain {
    /// 省下的成本量（饱和减）。
    pub fn saved(&self) -> u32 {
        self.before.saturating_sub(self.after)
    }

    /// 排序是否让成本变差（真排序器不该如此——变差即策略选错或脏输入）。
    pub fn regressed(&self) -> bool {
        self.after > self.before
    }
}

/// 排序收益（切换成本的量化入册面——收益透明才可审计）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SortGain {
    /// 排序前 PSO 切换次数。
    pub before: usize,
    /// 排序后 PSO 切换次数（`PsoFirst` 下应等于去重 PSO 数 − 1）。
    pub after: usize,
    /// 去重 PSO 数。
    pub distinct_pso: usize,
    /// 排序前全键（材质绑定）切换次数——PSO 之外的第二成本面。
    pub key_before: usize,
    /// 排序后全键切换次数。
    pub key_after: usize,
    /// 所用策略（`at_floor` 的适用范围由它决定）。
    pub strategy: SortStrategy,
    /// 分档成本收益。
    pub cost: CostGain,
}

impl SortGain {
    /// 省掉的切换次数。
    pub fn saved(&self) -> usize {
        self.before.saturating_sub(self.after)
    }

    /// 省掉的材质绑定切换次数（第二成本面的独立收益）。
    pub fn saved_keys(&self) -> usize {
        self.key_before.saturating_sub(self.key_after)
    }

    /// 是否达到 PSO 切换理论下限（排序最优性的可判定形式）。
    ///
    /// 仅对 [`SortStrategy::PsoFirst`] 成立：另两策略为的是材质/全键
    /// 切换最小，PSO 切换本就可能高于下限——拿别的策略判「未达下限」
    /// 是拿错尺子量错东西。
    pub fn at_floor(&self) -> bool {
        if self.strategy != SortStrategy::PsoFirst {
            return false;
        }
        self.distinct_pso == 0 || self.after + 1 == self.distinct_pso
    }
}

/// 校验并排序（`DRAW_INVALID` 先拒后排——非法输入不进排序器）。
pub fn sort_draws_checked_with(
    draws: &mut [DrawCall],
    strategy: SortStrategy,
) -> Result<SortGain, BtCode> {
    for d in draws.iter() {
        d.validate()?;
    }
    let before = count_switches(draws);
    let distinct = distinct_pso_count(draws);
    let key_before = count_key_switches(draws);
    let cost = SwitchCost::default_cost();
    let cost_before = switch_cost(draws, &cost);
    sort_draws_with(draws, strategy);
    verify_order(draws, strategy)?;
    let after = count_switches(draws);
    let key_after = count_key_switches(draws);
    let cost_after = switch_cost(draws, &cost);
    Ok(SortGain {
        before,
        after,
        distinct_pso: distinct,
        key_before,
        key_after,
        strategy,
        cost: CostGain {
            before: cost_before,
            after: cost_after,
        },
    })
}

/// 校验并按域默认策略排序。
pub fn sort_draws_checked(draws: &mut [DrawCall]) -> Result<SortGain, BtCode> {
    sort_draws_checked_with(draws, SortStrategy::PsoFirst)
}

// ---------------------------------------------------------------------------
// 六、排序的置换不变量（只换序不换内容）
// ---------------------------------------------------------------------------

/// 单 draw 内容指纹（FNV-1a over (pso, mat, index, instance) 字节序）。
///
/// 内容面**不含 seq**：seq 是排序的平手键不是被提交的内容——把它算进
/// 指纹会让「只换序」也误报漂移。
fn draw_content_fp(d: &DrawCall) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in d.key.pso.0.to_le_bytes().iter() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    for b in d.key.mat.0.to_le_bytes().iter() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    for b in d.index_count.to_le_bytes().iter() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    for b in d.instance_count.to_le_bytes().iter() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// 内容多重集指纹（顺序无关折叠——wrapping_add 可交换）。
pub fn multiset_fp(draws: &[DrawCall]) -> u64 {
    let mut acc: u64 = 0;
    for d in draws.iter() {
        acc = acc.wrapping_add(draw_content_fp(d));
    }
    acc
}

/// 索引总量（语义面守恒的第二独立量纲——与指纹互为交叉对拍）。
pub fn total_index_of(draws: &[DrawCall]) -> u64 {
    let mut acc: u64 = 0;
    for d in draws.iter() {
        acc = acc.saturating_add(d.index_count as u64);
    }
    acc
}

/// 指纹对拍：排序前后必须同指纹同长度同索引总量。
pub fn verify_multiset_preserved(fp_before: u64, fp_after: u64) -> Result<(), BtCode> {
    if fp_before != fp_after {
        return Err(BtCode::SORT_VIOLATION);
    }
    Ok(())
}

/// 切片对拍版置换不变量（`before`/`after` 两条 draw 序列）。
pub fn verify_sort_preserves(before: &[DrawCall], after: &[DrawCall]) -> Result<(), BtCode> {
    if before.len() != after.len() {
        return Err(BtCode::SORT_VIOLATION);
    }
    if total_index_of(before) != total_index_of(after) {
        return Err(BtCode::SORT_VIOLATION);
    }
    verify_multiset_preserved(multiset_fp(before), multiset_fp(after))
}

// ---------------------------------------------------------------------------
// 七、合并执行器（同 PSO 合并，draw call 数下降量化入册）
// ---------------------------------------------------------------------------

/// 合并批：同 PSO 的连续 draw 合并为一次调用（multi-draw/instanced）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MergedBatch {
    /// 批的 PSO（批内唯一——合并不跨 PSO）。
    pub pso: PsoId,
    /// 成员 draw 提交序号（升序——合并保序）。
    pub members: Vec<u32>,
    /// 批内索引数合计（空绘制计 0）。
    pub total_index: u32,
    /// 批内实例数最大值（实例语义不拆不合，只作容量携带面）。
    pub max_instance: u32,
    /// 批内材质去重表（首见序——材质跨度携带面，跨材质经描述符偏移
    /// 表达）。逐材质记账而非只记个数：只记个数就得靠近似推断（首材质
    /// 不同者递增），而近似在交替材质上会**虚报**跨度，虚报会让调用方
    /// 按不存在的描述符偏移需求分批——谎比少报更贵。
    pub mats: Vec<MatId>,
}

impl MergedBatch {
    /// 成员数（容量面——触顶即切批）。
    pub fn len(&self) -> usize {
        self.members.len()
    }

    /// 空批恒否（成员不可能为空——构造时即入成员）。
    pub fn is_empty(&self) -> bool {
        self.members.is_empty()
    }

    /// 是否触顶（触顶批的同 PSO 后继必是触顶切批）。
    pub fn at_cap(&self, cap: usize) -> bool {
        self.members.len() >= cap
    }

    /// 批内材质去重数（材质跨度）。
    pub fn distinct_mat(&self) -> usize {
        self.mats.len()
    }

    /// 批内首个材质（材质偏移基准位；空批为 `None`）。
    pub fn first_mat(&self) -> Option<MatId> {
        self.mats.first().copied()
    }
}

/// 合并收益（draw call 数下降量化——实测入册）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MergeGain {
    /// 合并前 draw call 数（含空绘制——空绘制也是 CPU 发起的一次调用）。
    pub before: usize,
    /// 合并后批数（空绘制跳过不进批）。
    pub after: usize,
    /// 跳过的空绘制数。
    pub skipped_empty: usize,
    /// 因触顶而切出的批数（容量切批透明入册——切批是合并能力的边界，
    /// 不是收益；把它混进 `after` 会让合并收益被系统性低估）。
    pub splits: usize,
}

impl MergeGain {
    /// 下降量（省掉的 draw call 数）。
    pub fn saved(&self) -> usize {
        self.before.saturating_sub(self.after)
    }
}

/// 新批构造（成员入批即记账——不留空批）。
fn new_batch(d: &DrawCall) -> MergedBatch {
    let mut members: Vec<u32> = Vec::new();
    members.push(d.seq);
    let mut mats: Vec<MatId> = Vec::new();
    mats.push(d.key.mat);
    MergedBatch {
        pso: d.key.pso,
        members,
        total_index: d.index_count,
        max_instance: d.instance_count,
        mats,
    }
}

/// 成员并入既有批（索引饱和加、实例取大、材质去重表首见序记账）。
fn extend_batch(b: &mut MergedBatch, d: &DrawCall) {
    b.members.push(d.seq);
    b.total_index = b.total_index.saturating_add(d.index_count);
    if d.instance_count > b.max_instance {
        b.max_instance = d.instance_count;
    }
    if !b.mats.iter().any(|m| *m == d.key.mat) {
        b.mats.push(d.key.mat);
    }
}

/// 带容量上限的合并执行器：相邻同 PSO 合并，触顶切批，跨 PSO 硬断。
///
/// `cap == 0` 显式拒（[`BtCode::CAP_INVALID`]）——零容量的合并器会把
/// 每条 draw 切成一个批，是配置事故不是合并。
pub fn merge_same_pso_capped(
    draws: &[DrawCall],
    cap: usize,
) -> Result<(Vec<MergedBatch>, usize), BtCode> {
    if cap == 0 {
        return Err(BtCode::CAP_INVALID);
    }
    let mut out: Vec<MergedBatch> = Vec::new();
    let mut splits = 0usize;
    for d in draws.iter() {
        if d.is_empty() {
            continue; // 空绘制不进批（被剔除的 draw 不占真实调用）
        }
        let same_pso = match out.last() {
            Some(b) => b.pso == d.key.pso,
            None => false,
        };
        let full = match out.last() {
            Some(b) => b.at_cap(cap),
            None => false,
        };
        if same_pso && !full {
            if let Some(b) = out.last_mut() {
                extend_batch(b, d);
            }
            continue;
        }
        if same_pso && full {
            splits += 1; // 触顶切批——透明入册
        }
        out.push(new_batch(d));
    }
    Ok((out, splits))
}

/// 域默认容量上限合并（`MAX_BATCH_MEMBERS`；域常量非零故不触 `Err` 分支，
/// 分支保留以保持零 panic 面与零静默）。
pub fn merge_same_pso(draws: &[DrawCall]) -> Result<Vec<MergedBatch>, BtCode> {
    let (out, _splits) = merge_same_pso_capped(draws, MAX_BATCH_MEMBERS)?;
    Ok(out)
}

/// 合并并计量收益（draw call 数下降量化入册的执行入口）。
pub fn merge_with_gain_capped(
    draws: &[DrawCall],
    cap: usize,
) -> Result<(Vec<MergedBatch>, MergeGain), BtCode> {
    let before = draws.len();
    let mut skipped_empty = 0usize;
    for d in draws.iter() {
        if d.is_empty() {
            skipped_empty += 1;
        }
    }
    let (batches, splits) = merge_same_pso_capped(draws, cap)?;
    let gain = MergeGain {
        before,
        after: batches.len(),
        skipped_empty,
        splits,
    };
    Ok((batches, gain))
}

/// 域默认容量上限的合并计量入口。
pub fn merge_with_gain(draws: &[DrawCall]) -> Result<(Vec<MergedBatch>, MergeGain), BtCode> {
    merge_with_gain_capped(draws, MAX_BATCH_MEMBERS)
}

/// 合并结果自检：成员保序、实例不放大、批容量不超限、同 PSO 相邻批
/// 必为触顶切批。
pub fn verify_merged_with(
    batches: &[MergedBatch],
    src_max_instance: u32,
    cap: usize,
) -> Result<(), BtCode> {
    if cap == 0 {
        return Err(BtCode::CAP_INVALID);
    }
    for b in batches.iter() {
        if b.is_empty() {
            return Err(BtCode::MERGE_VIOLATION);
        }
        if b.len() > cap {
            return Err(BtCode::MERGE_VIOLATION);
        }
        if b.distinct_mat() == 0 {
            return Err(BtCode::MERGE_VIOLATION);
        }
        let mut i = 1usize;
        while i < b.members.len() {
            let prev = match b.members.get(i - 1) {
                Some(m) => m,
                None => break,
            };
            let cur = match b.members.get(i) {
                Some(m) => m,
                None => break,
            };
            if prev >= cur {
                return Err(BtCode::MERGE_VIOLATION);
            }
            i += 1;
        }
        if b.max_instance > src_max_instance {
            return Err(BtCode::INSTANCE_FORBIDDEN);
        }
    }
    // 相邻同 PSO 两批：前批必须触顶。未触顶的同 PSO 相邻批 = 本可合并
    // 而未合并 = 执行器失守（合并执行器不得留下"白送的一次调用"）。
    let mut i = 1usize;
    while i < batches.len() {
        let prev = match batches.get(i - 1) {
            Some(b) => b,
            None => break,
        };
        let cur = match batches.get(i) {
            Some(b) => b,
            None => break,
        };
        if prev.pso == cur.pso && !prev.at_cap(cap) {
            return Err(BtCode::MERGE_VIOLATION);
        }
        i += 1;
    }
    Ok(())
}

/// 域默认容量上限的合并自检。
pub fn verify_merged(batches: &[MergedBatch], src_max_instance: u32) -> Result<(), BtCode> {
    verify_merged_with(batches, src_max_instance, MAX_BATCH_MEMBERS)
}

/// 材质跨度交叉对拍：批申报的材质表须与源序列逐批逐序相等。
///
/// 少报会让调用方低估描述符偏移需求，多报是谎——双向硬对拍，宁可红
/// 不可让谎过检。
pub fn verify_material_span(batches: &[MergedBatch], draws: &[DrawCall]) -> Result<(), BtCode> {
    for b in batches.iter() {
        let mut seen: Vec<MatId> = Vec::new();
        for m in b.members.iter() {
            let mat = match draws.iter().find(|d| d.seq == *m) {
                Some(d) => d.key.mat,
                None => return Err(BtCode::MERGE_VIOLATION),
            };
            if !seen.iter().any(|x| *x == mat) {
                seen.push(mat);
            }
        }
        if seen != b.mats {
            return Err(BtCode::MERGE_VIOLATION);
        }
    }
    Ok(())
}

/// 批序列的 PSO 跨度（去重 PSO 数——合并后的切换下限）。
pub fn pso_span(batches: &[MergedBatch]) -> usize {
    let mut seen: Vec<PsoId> = Vec::new();
    for b in batches.iter() {
        if !seen.iter().any(|p| *p == b.pso) {
            seen.push(b.pso);
        }
    }
    seen.len()
}

// ---------------------------------------------------------------------------
// 八、帧预算（每帧 draw call 上限，超预算告警是优化信号）
// ---------------------------------------------------------------------------

/// 超预算分档（刚超 / 倍档超标——分档让下游遥测可分别消费）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BudgetSeverity {
    /// 刚超上限（1× ≤ calls < 2×）。
    Over,
    /// 倍档超标（calls ≥ 2×limit）——成本曲线已失控，优化优先级最高。
    Severe,
}

impl BudgetSeverity {
    /// 人话名。
    pub fn name(self) -> &'static str {
        match self {
            BudgetSeverity::Over => "刚超上限",
            BudgetSeverity::Severe => "倍档超标",
        }
    }
}

/// 预算判定（恰预算判内——等于上限不越界）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BudgetVerdict {
    /// 预算内（含恰预算）。
    Within,
    /// 超预算（over_by 透明 + 分档）。
    Over {
        over_by: u32,
        severity: BudgetSeverity,
    },
}

impl BudgetVerdict {
    /// 是否超预算。
    pub fn is_over(&self) -> bool {
        matches!(self, BudgetVerdict::Over { .. })
    }

    /// 分档（预算内无档，返回 `None`）。
    pub fn severity(&self) -> Option<BudgetSeverity> {
        match self {
            BudgetVerdict::Within => None,
            BudgetVerdict::Over { severity, .. } => Some(*severity),
        }
    }
}

/// 帧 draw call 预算。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FrameBudget {
    pub max_calls: u32,
}

impl FrameBudget {
    /// 构造：上限须 ∈ (0, MAX_FRAME_BUDGET]。
    pub fn new(max_calls: u32) -> Result<FrameBudget, BtCode> {
        if max_calls == 0 || max_calls > MAX_FRAME_BUDGET {
            return Err(BtCode::BUDGET_INVALID);
        }
        Ok(FrameBudget { max_calls })
    }

    /// 域内冻结默认预算。
    pub fn default_budget() -> FrameBudget {
        FrameBudget {
            max_calls: DEFAULT_FRAME_BUDGET,
        }
    }

    /// 判定：合并后的批数与裸调用数都可入（调用方决定拿谁对预算）。
    pub fn judge(&self, calls: usize) -> BudgetVerdict {
        let calls64 = calls as u64;
        let limit64 = self.max_calls as u64;
        if calls64 <= limit64 {
            return BudgetVerdict::Within;
        }
        let over = calls64 - limit64;
        let over_by = if over > u32::MAX as u64 {
            u32::MAX
        } else {
            over as u32
        };
        let severity = if calls64 >= limit64.saturating_mul(2) {
            BudgetSeverity::Severe
        } else {
            BudgetSeverity::Over
        };
        BudgetVerdict::Over {
            over_by,
            severity,
        }
    }

    /// 预算内余量（饱和减——超预算时为 0，不回绕成天文数字）。
    pub fn headroom(&self, calls: usize) -> u32 {
        let calls64 = calls as u64;
        let limit64 = self.max_calls as u64;
        if calls64 >= limit64 {
            0
        } else {
            (limit64 - calls64) as u32
        }
    }
}

/// 预算告警（超预算才产——F1416 预算理念的渲染版：信号不拒绘）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BudgetAlarm {
    /// 实测调用数。
    pub calls: usize,
    /// 预算上限。
    pub limit: u32,
    /// 超限量。
    pub over_by: u32,
    /// 超预算分档。
    pub severity: BudgetSeverity,
    /// 优化建议（告警面固定文案）。
    pub advice: &'static str,
}

/// 预算告警出口：`Within` 产 `None`，`Over` 产透明告警。
pub fn budget_alarm(calls: usize, budget: &FrameBudget) -> Option<BudgetAlarm> {
    match budget.judge(calls) {
        BudgetVerdict::Within => None,
        BudgetVerdict::Over {
            over_by,
            severity,
        } => Some(BudgetAlarm {
            calls,
            limit: budget.max_calls,
            over_by,
            severity,
            advice: "合并同 PSO draw 或重排状态以降低切换；预算是优化信号，不拒绘不砍画质",
        }),
    }
}

// ---------------------------------------------------------------------------
// 九、批处理主管线（validate→排序→合并→预算——四件套的总装编排）
// ---------------------------------------------------------------------------

/// 批内校验：逐 draw 参数域 + 提交序号互异（稳定排序的语义前提——
/// seq 重复则「同键保提交序」无唯一解，排序结论失真）。
pub fn validate_batch(draws: &[DrawCall]) -> Result<(), BtCode> {
    let mut i = 0usize;
    while i < draws.len() {
        let d = match draws.get(i) {
            Some(d) => d,
            None => break,
        };
        d.validate()?;
        let mut j = i + 1;
        while j < draws.len() {
            let e = match draws.get(j) {
                Some(e) => e,
                None => break,
            };
            if d.seq == e.seq {
                return Err(BtCode::DRAW_INVALID);
            }
            j += 1;
        }
        i += 1;
    }
    Ok(())
}

/// 批次最大实例数（实例容量携带面的输入——合并不放大它）。
pub fn max_instance_of(draws: &[DrawCall]) -> u32 {
    let mut max = 0u32;
    for d in draws.iter() {
        if d.instance_count > max {
            max = d.instance_count;
        }
    }
    max
}

/// 批处理报告（一次管线的全部量化结论——收益透明可审计）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BatchingReport {
    /// 排序收益（PSO 切换根治面 + 分档成本面）。
    pub sort: SortGain,
    /// 合并收益（draw call 数下降面）。
    pub merge: MergeGain,
    /// 合并批序列（供调用方回查材质跨度/容量）。
    pub batches: Vec<MergedBatch>,
    /// 预算判定（对合并后批数）。
    pub verdict: BudgetVerdict,
    /// 超预算告警（`None` = 预算内）。
    pub alarm: Option<BudgetAlarm>,
}

impl BatchingReport {
    /// draw call 净下降量（合并省下的调用数）。
    pub fn saved_calls(&self) -> usize {
        self.merge.saved()
    }

    /// PSO 切换净下降量（排序省下的切换数）。
    pub fn saved_switches(&self) -> usize {
        self.sort.saved()
    }

    /// 分档切换成本净下降量。
    pub fn saved_cost(&self) -> u32 {
        self.sort.cost.saved()
    }

    /// 呈现文本：切换/材质绑定/调用/成本/预算一次说全（不修饰不省略）。
    pub fn present(&self) -> String {
        let alarm = match &self.alarm {
            Some(a) => alloc::format!("；超预算 +{}（{}）", a.over_by, a.severity.name()),
            None => String::from("；预算内"),
        };
        alloc::format!(
            "批处理[{}]：PSO 切换 {}→{}（-{}）；全键切换 {}→{}（-{}）；draw call {}→{}（-{}，跳过空绘制 {}，切批 {}）；切换成本 {}→{}（-{}）{}",
            self.sort.strategy.name(),
            self.sort.before,
            self.sort.after,
            self.sort.saved(),
            self.sort.key_before,
            self.sort.key_after,
            self.sort.saved_keys(),
            self.merge.before,
            self.merge.after,
            self.merge.saved(),
            self.merge.skipped_empty,
            self.merge.splits,
            self.sort.cost.before,
            self.sort.cost.after,
            self.saved_cost(),
            alarm
        )
    }
}

/// 批处理主管线（全策略通用形）：校验→稳定排序→同 PSO 合并→预算判定。
///
/// 排序前后同指纹对拍内置于主管线（[`verify_multiset_preserved`]）——
/// 主管线是对外唯一入口，置换不变量必须在入口处守住而不是指望调用方
/// 记得调校验函数。
pub fn run_batching_with(
    draws: &mut [DrawCall],
    strategy: SortStrategy,
    budget: &FrameBudget,
) -> Result<BatchingReport, BtCode> {
    let fp_before = multiset_fp(draws);
    let index_before = total_index_of(draws);
    validate_batch(draws)?;
    let sort = sort_draws_checked_with(draws, strategy)?;
    verify_multiset_preserved(fp_before, multiset_fp(draws))?;
    if index_before != total_index_of(draws) {
        return Err(BtCode::SORT_VIOLATION);
    }
    let (batches, merge) = merge_with_gain(draws)?;
    let src_max = max_instance_of(draws);
    verify_merged(&batches, src_max)?;
    verify_material_span(&batches, draws)?;
    let verdict = budget.judge(batches.len());
    let alarm = budget_alarm(batches.len(), budget);
    Ok(BatchingReport {
        sort,
        merge,
        batches,
        verdict,
        alarm,
    })
}

/// 批处理主管线（域默认策略 `PsoFirst`）。
pub fn run_batching(
    draws: &mut [DrawCall],
    budget: &FrameBudget,
) -> Result<BatchingReport, BtCode> {
    run_batching_with(draws, SortStrategy::PsoFirst, budget)
}

// ---------------------------------------------------------------------------
// 十、边界声明（实例化归 I04 组——本组只做非实例合并）
// ---------------------------------------------------------------------------

/// I04 实例化边界声明（字面量冻结）。
pub const I04_INSTANCE_BOUNDARY: &str =
    "实例化（instanced draw 策略）归 I04 组——本组只做非实例合并（draw call 数下降），不设计实例化策略";

/// 合并不碰实例语义声明。
pub const MERGE_NOT_TOUCH_INSTANCE: &str =
    "合并只合同 PSO 的 draw call 计数与状态面，实例数原样携带不拆不合——实例语义归 I04 组";

/// 预算信号声明（F1416 渲染版）。
pub const BUDGET_SIGNAL_NOTICE: &str =
    "每帧 draw call 预算告警是优化信号（F1416 预算理念的渲染版）——告警不拒绘不砍画质";

/// 批内材质跨度承载声明（合并跨材质经描述符偏移，不是白拿的收益）。
pub const MERGE_MATERIAL_NOTICE: &str =
    "同 PSO 批内可跨材质：材质差异经逐成员描述符偏移表达，不增 draw call；若目标后端的 multi-draw 不支持逐成员描述符偏移，调用方须按材质跨度退回分批";

/// 容量上限声明（触顶切批透明入册，切批不是收益）。
pub const BATCH_CAP_NOTICE: &str =
    "单批成员上限为容量面（与 F1628 indirect 批命令上限同量）：触顶即切批，切批数入册，切批不是合并收益";

/// 边界声明校验：I04 关键词 + 实例关键词 + 非实例合并口径齐备。
pub fn verify_boundary() -> Result<(), BtCode> {
    let decls = [I04_INSTANCE_BOUNDARY, MERGE_NOT_TOUCH_INSTANCE];
    for d in decls.iter() {
        if !d.contains("I04") || !d.contains("实例") {
            return Err(BtCode::BOUNDARY_MISMATCH);
        }
    }
    if !I04_INSTANCE_BOUNDARY.contains("非实例合并") {
        return Err(BtCode::BOUNDARY_MISMATCH);
    }
    if !BUDGET_SIGNAL_NOTICE.contains("F1416") {
        return Err(BtCode::BOUNDARY_MISMATCH);
    }
    if !BUDGET_SIGNAL_NOTICE.contains("不拒绘") {
        return Err(BtCode::BOUNDARY_MISMATCH);
    }
    if !MERGE_MATERIAL_NOTICE.contains("描述符偏移") || !MERGE_MATERIAL_NOTICE.contains("退回分批") {
        return Err(BtCode::BOUNDARY_MISMATCH);
    }
    if !BATCH_CAP_NOTICE.contains("切批不是合并收益") {
        return Err(BtCode::BOUNDARY_MISMATCH);
    }
    Ok(())
}

/// 实例容量守卫：合并前后实例容量不得被放大（放大即越界）。
pub fn guard_merge_instance(before_max: u32, after_max: u32) -> Result<(), BtCode> {
    if after_max > before_max {
        Err(BtCode::INSTANCE_FORBIDDEN)
    } else {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 十一、诊断码（独占 0x4Exx 段）
// ---------------------------------------------------------------------------

/// vef32 诊断码。独占 `0x4Exx` 段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BtCode(pub u16);

impl BtCode {
    /// draw 参数域非法（实例数 0、seq 重复等）。
    pub const DRAW_INVALID: BtCode = BtCode(0x4E01);
    /// 排序结果违例（键序乱、同键失序、内容漂移）。
    pub const SORT_VIOLATION: BtCode = BtCode(0x4E02);
    /// 合并跨 PSO、成员乱序、或留下本可合并的同 PSO 相邻批。
    pub const MERGE_VIOLATION: BtCode = BtCode(0x4E03);
    /// 预算上限非法（0 或超域硬界）。
    pub const BUDGET_INVALID: BtCode = BtCode(0x4E04);
    /// 边界声明关键词缺失。
    pub const BOUNDARY_MISMATCH: BtCode = BtCode(0x4E05);
    /// 合并放大实例容量（实例化越界）。
    pub const INSTANCE_FORBIDDEN: BtCode = BtCode(0x4E06);
    /// 批容量上限非法（0——零容量会把每条 draw 切成一批）。
    pub const CAP_INVALID: BtCode = BtCode(0x4E07);

    /// 全部在案码。
    pub fn all() -> [BtCode; 7] {
        [
            BtCode::DRAW_INVALID,
            BtCode::SORT_VIOLATION,
            BtCode::MERGE_VIOLATION,
            BtCode::BUDGET_INVALID,
            BtCode::BOUNDARY_MISMATCH,
            BtCode::INSTANCE_FORBIDDEN,
            BtCode::CAP_INVALID,
        ]
    }

    /// wire 码。
    pub const fn code(self) -> u16 {
        self.0
    }

    /// 人话原因。
    pub fn say(self) -> String {
        let name = match self {
            BtCode::DRAW_INVALID => "draw 参数域非法（实例数 0 或 seq 重复）",
            BtCode::SORT_VIOLATION => "排序结果违例（键序乱/同键失序/内容漂移）",
            BtCode::MERGE_VIOLATION => "合并违例（跨 PSO/成员乱序/留下可合并相邻批）",
            BtCode::BUDGET_INVALID => "预算上限非法（0 或超域硬界）",
            BtCode::BOUNDARY_MISMATCH => "边界声明关键词缺失",
            BtCode::INSTANCE_FORBIDDEN => "合并放大实例容量（实例化越界）",
            BtCode::CAP_INVALID => "批容量上限非法（0——零容量会把每条 draw 切成一批）",
            BtCode(_) => "vef32 未在案码",
        };
        alloc::format!("0x{:04X} {}", self.0, name)
    }
}