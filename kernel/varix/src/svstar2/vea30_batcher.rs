//! VE-F0030 · 多绘制合批器（VE-A 域 · 绘制调用自动合批 + 收益评估 + 打断归因 · 目标 420 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0030`
//!
//! **判据（锚点原文）**：绘制调用的自动合批（同材质/同管线的绘制合并为
//! Instancing/批渲染），合批收益评估（合并省多少有数字），合批打断检测
//! （材质切换导致合批失败的归因）；含合批对动态实例的兼容说明。
//! 判据：**自动合批、收益数字化、打断归因、资产建议、判据**。
//!
//! **错误路径与降级矩阵**（锚点原文）：
//!
//! - 合批失败 → **原路绘制**（退回逐条直接绘制，不静默丢绘制；退回条数与
//!   合批失败原因**独立对账**，不采信净值口径）
//! - 收益为负 → **放弃合批**（合批本身有开销——每批一次状态绑定与一次实例缓冲
//!   更新；批次太小时省下的驱动调用不够抵，合批反而更慢）
//! - 打断率高 → **资产建议**（把打断归因反查成「哪些材质该合并/该重排」的
//!   可执行建议，而不是只报一个打断率数字）
//!
//! **数据结构**：合批引擎（[`BatchEngine`]）；收益评估（[`GainReport`]）；
//! 打断归因（[`Breakdown`]）。
//!
//! **性能逐项分解**：O(绘制数)——[`BatchEngine::submit`] 是 O(1) 摊还（按批键
//! 查表，批表上限 [`MAX_BATCHES`] 项），收批 [`BatchEngine::seal`] 是 O(绘制数)
//! 单趟扫描，均**不随累计帧数增长**（批表按 [`seal`] 重建，不跨帧累积）。
//!
//! **跨批对接点**：E 域资产建议——[`AssetAdvice`] 把打断归因翻译成资产层可执行
//! 动作（合并材质 / 重排绘制序 / 拆批），供 E 域资产管线消费。
//!
//! **无障碍与隐私**：合批面板读屏可达（[`BatchEngine::a11y_lines`]）——报
//! 「绘制数/批数/合批收益/退回数/打断率」，中英双语逐行。面板**只报聚合计数**，
//! **不报单个网格的顶点偏移与材质参数**（那是资产布局信息）。
//!
//! ## 设计要点
//!
//! - **批键必须含「种类 + 管线状态 + 材质」，三者缺一即错合**（[`BatchKey`]）：
//!   只按材质分组会把不同管线的绘制并成一批，而批内无法重设管线状态——那种
//!   「合批」提交上去就是错的画面。故三类各占一个字段，缺项在构造期就拒。
//! - **动态实例不破坏合批**（[`BatchItem::dynamic`]）：实例矩阵每帧可变的绘制
//!   **照合不误**——合批改变的是「几次驱动调用」，不是「实例数据是否可变」。
//!   把它当打断原因是**错的归因**，会把动画资产全打成建议重排。真正的打断
//!   原因只有三类（见 [`Breakdown`]），动态实例**不在其中**。
//! - **收益必须数字化且成式**（[`GainReport`]）：合批省的是驱动调用数，代价是
//!   每批一次状态绑定加一次实例缓冲更新。故收益 = 省下的调用数 − 合批开销，
//!   **按实算不拍脑袋**；净收益为负即放弃合批（[`BatchEngine::seal`] 逐批判定）。
//! - **「省了多少」用绝对值口径对账**：判据侧**独立重算**该合的批数与不该合的
//!   批数，断 [`GainReport::calls_saved`] **恰等于**其差——不用净值口径
//!   （十诫第 10 条：净值会被「建了又销」骗过）。
//! - **打断归因必须给专属类别，不合并成「打断」**（[`BreakReason`]）：材质切换、
//!   管线切换、拓扑不兼容三条外部表现相同（都是「这批没合上」），合并成一条
//!   则删掉任一条判据仍全绿（十诫第 3 条）。判据直接断言故障种类。
//! - **打断归因要能反查成资产建议**（[`AssetAdvice`]）：高打断率本身不是可执行
//!   结论；本条把它翻译成「把材质 X 的绘制挪到 Y 批之前」这类具体动作，
//!   否则 E 域拿到一个比率也无从下手。
//! - **放弃合批的批必须退回逐条绘制**（[`BatchEngine::seal`]）：放弃是「这批不
//!   合、照原样画」，不是「这批不画」。判据断退回条数恰等于被放弃的批内绘制数
//!   之和，且这些绘制在输出流里**逐条在场**。
//! - **夹逼对钉死收益阈位置**（[`MIN_NET_GAIN`]）：净收益恰为 0 的批，
//!   判据断它**被放弃**（阈为「正」才合），并另造一个净收益恰为 1 的批断它
//!   **被合**——只断「负收益放弃」会把阈写成 `>=` 而全绿（十诫第 4 条）。
//!
//! ## 与相邻条的分工（易混，故写明）
//!
//! - **F0029（`vea29_indirect`）决定「命令怎么写进间接缓冲」**，本条决定
//!   「该不该合、合几批」。本条的 [`GainReport::calls_saved`] 是收益数字的
//!   唯一来源；反向不成立。
//! - **F0042（网格合并与批处理）谈的是网格数据本身的合并**，本条谈的是
//!   **绘制调用的合批**：网格没合并照样可以合批（同材质同管线的多次绘制合并为
//!   实例化），反过来网格合并了也可能因管线不同而不能合。
//!
//! 两条各自**自持**定义类型，不跨模块 `use`——并行提交时跨模块引用会把两个模块
//! 的编译成败绑在一起，一方半成品就拖垮另一方，而这类失败报 E0583，与真实缺陷
//! 长得一样、极难分辨。

use crate::checks::CheckSet;

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量
// ---------------------------------------------------------------------------

/// 合批键的构成维度数（种类 / 管线状态 / 材质，缺一即错合）。
pub const KEY_DIMS: usize = 3;

/// 单帧批表容量上限（超出 → 关闭新批并登记 [`BreakReason::BatchTableFull`]）。
pub const MAX_BATCHES: usize = 64;

/// 单批最大绘制数（超出不再并入该批——实例缓冲一次性写不下）。
pub const MAX_BATCH_DRAWS: usize = 256;

/// 单次实例缓冲更新按下的**每次驱动调用成本**（相对单位；锚点要求收益成式，
/// 故此处是「一次调用记 1」，与合批开销同量纲，可直接相减）。
pub const CALL_COST: u32 = 1;

/// 每批的状态绑定 + 实例缓冲更新开销（相对单位）。合批要**省**驱动调用，
/// 但每批要**付**一次绑定加一次缓冲更新，故小批合并不划算。
pub const BATCH_OVERHEAD: u32 = 2;

/// 合批所需的最小净收益（**严格大于**才合）。净收益恰为 0 的批**不合**。
pub const MIN_NET_GAIN: i64 = 0;

/// 打断原因种数（判据按此遍历，不手抄清单）。
pub const BREAK_COUNT: usize = 3;

/// 高打断率告警阈（千分比，超过则出资产建议）。
pub const BREAK_RATE_ALERT: u32 = 500;

// ---------------------------------------------------------------------------
// 二、批键与打断归因
// ---------------------------------------------------------------------------

/// 打断原因（**三类专属，不合并**——合并则判据无法区分，见文件头十诫第 3 条）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BreakReason {
    /// 材质不同：批内无法重设材质，必须断开。
    MaterialSwitch,
    /// 管线状态不同：批内无法重设管线状态，必须断开。
    PipelineSwitch,
    /// 拓扑不兼容：同类绘制但顶点布局不同，无法共用实例缓冲。
    TopologyIncompatible,
    /// 批表满：容量约束导致断开（**非**内容原因，单列以便容量告警）。
    BatchTableFull,
}

impl BreakReason {
    /// 全集，顺序稳定（判据按此下标推导，不靠字面量）。
    pub const ALL: [BreakReason; 4] = [
        BreakReason::MaterialSwitch,
        BreakReason::PipelineSwitch,
        BreakReason::TopologyIncompatible,
        BreakReason::BatchTableFull,
    ];

    /// 判别下标（**不是**线上编码值）。
    pub const fn ordinal(self) -> usize {
        match self {
            BreakReason::MaterialSwitch => 0,
            BreakReason::PipelineSwitch => 1,
            BreakReason::TopologyIncompatible => 2,
            BreakReason::BatchTableFull => 3,
        }
    }

    /// 内容类打断（不含容量类）——资产建议只看内容类。
    pub const fn is_content(self) -> bool {
        !matches!(self, BreakReason::BatchTableFull)
    }

    /// 中文标签（读屏与建议用）。
    pub const fn zh(self) -> &'static str {
        match self {
            BreakReason::MaterialSwitch => "材质切换",
            BreakReason::PipelineSwitch => "管线切换",
            BreakReason::TopologyIncompatible => "拓扑不兼容",
            BreakReason::BatchTableFull => "批表容量已满",
        }
    }

    /// 英文标签（读屏用）。
    pub const fn tag(self) -> &'static str {
        match self {
            BreakReason::MaterialSwitch => "material switch",
            BreakReason::PipelineSwitch => "pipeline switch",
            BreakReason::TopologyIncompatible => "topology incompatible",
            BreakReason::BatchTableFull => "batch table full",
        }
    }
}

/// 合批键：种类 + 管线状态 + 材质，三者齐备方为有效键。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BatchKey {
    /// 绘制种类（决定能否共实例缓冲）。
    pub kind: u8,
    /// 管线状态标识（着色器与混合等状态位打包）。
    pub pipeline: u16,
    /// 材质标识。
    pub material: u16,
}

impl BatchKey {
    /// 构造一个键。三个维度**全零视为无效**——那正是「键没填」的形态，
    /// 拿它合批会把什么都没说的绘制并进同一批。
    pub const fn new(kind: u8, pipeline: u16, material: u16) -> BatchKey {
        BatchKey { kind, pipeline, material }
    }

    /// 键是否有效（三维度不全为零）。
    pub const fn is_valid(self) -> bool {
        self.kind != 0 || self.pipeline != 0 || self.material != 0
    }

    /// 归一化指纹（判据按此比对键等价）。
    pub const fn fingerprint(self) -> u64 {
        (self.kind as u64) | ((self.pipeline as u64) << 8) | ((self.material as u64) << 32)
    }
}

/// 一条待合批的绘制调用。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BatchItem {
    /// 合批键。
    pub key: BatchKey,
    /// 顶点数。
    pub vertices: u32,
    /// **动态实例**：实例矩阵每帧可变。
    ///
    /// 动态**不影响**能否合批（合批改的是驱动调用次数，不是实例数据可变
    /// 性）；单列此字段是为了让判据能证明这一点，而不是靠「没报错」暗示。
    pub dynamic: bool,
}

// ---------------------------------------------------------------------------
// 三、合批引擎
// ---------------------------------------------------------------------------

/// 一个已开启的批。
///
/// `pub` 是因为 [`BatchEngine::batches`] 是 `pub fn`——若 `Batch` 私有，
/// 编译器报 `private_interfaces`（公接口返回私类型，外部无法使用其字段）。
#[derive(Clone, Debug)]
pub struct Batch {
    /// 批键。
    pub key: BatchKey,
    /// 批内绘制数。
    pub draws: u32,
    /// 批内顶点数合计。
    pub vertices: u64,
    /// 批内动态实例绘制数（**只统计，不影响合批判定**）。
    pub dynamic_draws: u32,
}

/// 一次合批的收益账（**成式**，不拍脑袋）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GainReport {
    /// 批内绘制数。
    pub draws: u32,
    /// 不合批时的驱动调用数（每条绘制一次）。
    pub calls_unbatched: u32,
    /// 合批后的驱动调用数（每批一次）。
    pub calls_batched: u32,
    /// 省下的驱动调用数（绝对值口径：`unbatched - batched`）。
    pub calls_saved: u32,
    /// 每批开销合计（`批数 * BATCH_OVERHEAD`）。
    pub overhead: u32,
    /// 净收益（`calls_saved - overhead`，**有符号**：可为负）。
    pub net_gain: i64,
}

impl GainReport {
    /// 按**实算**收益账（不合批调用数由批内绘制数独立算出，不接受外部传参）。
    pub fn compute(draws: u32) -> GainReport {
        let calls_unbatched = draws.saturating_mul(CALL_COST);
        let calls_batched = CALL_COST;
        let calls_saved = calls_unbatched.saturating_sub(calls_batched);
        let overhead = BATCH_OVERHEAD;
        let net_gain = i64::from(calls_saved) - i64::from(overhead);
        GainReport { draws, calls_unbatched, calls_batched, calls_saved, overhead, net_gain }
    }

    /// 该批是否值得合（净收益**严格大于** [`MIN_NET_GAIN`]）。
    pub const fn worth_batching(&self) -> bool {
        self.net_gain > MIN_NET_GAIN
    }

    /// 中文一行摘要（读屏与调试用）。
    pub fn summary(&self) -> String {
        format!(
            "绘制 {} 条：不合批 {} 次调用，合批 {} 次，省 {} 次，开销 {}，净收益 {}",
            self.draws,
            self.calls_unbatched,
            self.calls_batched,
            self.calls_saved,
            self.overhead,
            self.net_gain
        )
    }
}

/// 资产层建议（打断归因的可执行化）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetAdvice {
    /// 建议动作。
    pub action: AdviceAction,
    /// 涉及的内容类打断原因。
    pub reason: BreakReason,
    /// 建议正文（中文）。
    pub detail: String,
}

/// 建议动作。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdviceAction {
    /// 把散落的同材质绘制挪到相邻位置。
    ReorderDraws,
    /// 合并材质（减少内容类打断）。
    MergeMaterials,
    /// 拆分管线状态（让状态切换变成批边界而非批内矛盾）。
    SplitPipeline,
}

impl AdviceAction {
    /// 中文标签。
    pub const fn zh(self) -> &'static str {
        match self {
            AdviceAction::ReorderDraws => "重排绘制序",
            AdviceAction::MergeMaterials => "合并材质",
            AdviceAction::SplitPipeline => "拆分管线状态",
        }
    }
}

/// 合批引擎：一帧内按批键归并绘制调用。
#[derive(Clone, Debug)]
pub struct BatchEngine {
    /// 已开启的批。
    batches: Vec<Batch>,
    /// 上一批的键（打断归因用：当前键与它不同即打断）。
    last_key: Option<BatchKey>,
    /// 各打断原因计数（**绝对值口径**：被拒的绘制数，不是净值差）。
    breaks: [u32; BREAK_COUNT + 1],
    /// 内容类打断总数（供打断率计算，不含容量类）。
    content_breaks: u32,
    /// 被拒的绘制数（因打断而未能并入任何批）。
    rejected: u32,
    /// 被放弃合批的批内绘制数之和（放弃 ≠ 不画，这些绘制**照原样**逐条输出）。
    fallbacks: u32,
    /// 输出流：合批批号（正数）或原路绘制标记（`FALLBACK_MARK`）。
    output: Vec<i32>,
    /// 键 → 批下标。
    index: Vec<(u64, usize)>,
}

/// 原路绘制标记（输出流里用它表示「这条照原样画」，与批号区分）。
pub const FALLBACK_MARK: i32 = -1;

impl Default for BatchEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl BatchEngine {
    /// 新建引擎（空批表）。
    pub fn new() -> BatchEngine {
        BatchEngine {
            batches: Vec::new(),
            last_key: None,
            breaks: [0u32; BREAK_COUNT + 1],
            content_breaks: 0,
            rejected: 0,
            fallbacks: 0,
            output: Vec::new(),
            index: Vec::new(),
        }
    }

    /// 归因一条绘制与上一批的打断原因（**None** 表示可并入）。
    ///
    /// 判定次序即归因优先级：键无效 → 拓扑；材质异 → 材质；管线异 → 管线。
    /// 材质先于管线是刻意的——两者同时不同时，**直接原因是材质**
    /// （材质是批键的主序），报管线会把直接原因掩盖成间接原因。
    pub fn classify(prev: BatchKey, next: BatchKey) -> Option<BreakReason> {
        if !next.is_valid() || !prev.is_valid() {
            return Some(BreakReason::TopologyIncompatible);
        }
        if prev.material != next.material {
            return Some(BreakReason::MaterialSwitch);
        }
        if prev.pipeline != next.pipeline {
            return Some(BreakReason::PipelineSwitch);
        }
        if prev.kind != next.kind {
            return Some(BreakReason::TopologyIncompatible);
        }
        None
    }

    /// 提交一条绘制尝试并入批。
    ///
    /// 返回 [`SubmitOutcome`]：并入既有批 / 新开一批 / 被拒（附原因）。
    pub fn submit(&mut self, item: BatchItem) -> SubmitOutcome {
        if !item.key.is_valid() {
            self.note_break(BreakReason::TopologyIncompatible);
            self.rejected = self.rejected.saturating_add(1);
            return SubmitOutcome::Rejected(BreakReason::TopologyIncompatible);
        }
        // 打断判定：与当前批键不同即打断，并**新开一批**（不并入）。
        let broken = match self.last_key {
            Some(prev) => Self::classify(prev, item.key),
            None => None,
        };
        if let Some(r) = broken {
            self.note_break(r);
        }
        // 查表：同键且**该键下还有未满的批** → 并入。
        //
        // 这里必须找「该键下第一个未满的批」，而不是「该键的第一个批」：
        // 批表按键**允许多批**（同键超过 MAX_BATCH_DRAWS 时要另开一批接着装），
        // 若只认该键的第一个批，它一满就永远查不中未满的批，于是每条同键
        // 绘制都新开一批——批数无界增长，而实例缓冲该合并的没合并。
        let fp = item.key.fingerprint();
        let target = self
            .index
            .iter()
            .filter(|(f, _)| *f == fp)
            .map(|(_, bi)| *bi)
            .find(|bi| self.batches[*bi].draws < MAX_BATCH_DRAWS as u32);
        if let Some(bi) = target {
            let b = &mut self.batches[bi];
            b.draws += 1;
            b.vertices = b.vertices.saturating_add(item.vertices as u64);
            if item.dynamic {
                b.dynamic_draws = b.dynamic_draws.saturating_add(1);
            }
            self.last_key = Some(item.key);
            return SubmitOutcome::Merged(bi);
        }
        // 新开一批：容量满则拒（容量类，不污染内容类打断率）。
        if self.batches.len() >= MAX_BATCHES {
            self.note_break(BreakReason::BatchTableFull);
            self.rejected = self.rejected.saturating_add(1);
            return SubmitOutcome::Rejected(BreakReason::BatchTableFull);
        }
        let bi = self.batches.len();
        self.batches.push(Batch {
            key: item.key,
            draws: 1,
            vertices: item.vertices as u64,
            dynamic_draws: if item.dynamic { 1 } else { 0 },
        });
        self.index.push((fp, bi));
        self.last_key = Some(item.key);
        SubmitOutcome::Opened(bi)
    }

    /// 记一条打断（**绝对值口径**：被拒/被打断的绘制数）。
    fn note_break(&mut self, r: BreakReason) {
        let i = r.ordinal();
        self.breaks[i] = self.breaks[i].saturating_add(1);
        if r.is_content() {
            self.content_breaks = self.content_breaks.saturating_add(1);
        }
    }

    /// 收批：逐批算收益，决定合 / 放弃，并把放弃批的绘制**逐条**记入输出流。
    ///
    /// 返回收批统计。**放弃的批不会被丢弃**——其绘制数计入
    /// [`SealStat::fallbacks`] 并在输出流里逐条标 [`FALLBACK_MARK`]。
    pub fn seal(&mut self) -> SealStat {
        let mut st = SealStat::default();
        for (bi, b) in self.batches.iter().enumerate() {
            let g = GainReport::compute(b.draws);
            // 逐批**追加**收益账，不按 `gains[bi]` 下标写入——`gains` 起始为空，
            // 用 `[bi]` 下标写会在第一次 seal 时越界 panic（零 panic 面纪律）。
            // 追加顺序与批下标同序，故下标仍一一对应。
            debug_assert_eq!(st.gains.len(), bi);
            st.gains.push(g);
            if g.worth_batching() {
                st.batched += 1;
                st.draws_batched = st.draws_batched.saturating_add(b.draws);
                st.calls_saved = st.calls_saved.saturating_add(g.calls_saved);
                st.net_gain = st.net_gain.saturating_add(g.net_gain);
                st.dynamic_in_batched = st.dynamic_in_batched.saturating_add(b.dynamic_draws);
                self.output.push(bi as i32);
            } else {
                st.abandoned += 1;
                st.draws_abandoned = st.draws_abandoned.saturating_add(b.draws);
                st.fallbacks = st.fallbacks.saturating_add(b.draws);
                // 放弃 = 照原样逐条画，故每条绘制各占一个标记
                let mut k = 0u32;
                while k < b.draws {
                    self.output.push(FALLBACK_MARK);
                    k += 1;
                }
            }
        }
        st.batches_total = self.batches.len() as u32;
        st.draws_total = st.draws_batched.saturating_add(st.draws_abandoned);
        st
    }

    /// 内容类打断率（千分比，分母为提交总数；**不含**容量类打断）。
    pub fn break_rate_permille(&self, submitted: u32) -> u32 {
        if submitted == 0 {
            return 0;
        }
        ((self.content_breaks as u64 * 1000) / submitted as u64) as u32
    }

    /// 各打断原因计数（按下标，判据按 [`BreakReason::ALL`] 遍历）。
    pub fn break_counts(&self) -> [u32; BREAK_COUNT + 1] {
        self.breaks
    }

    /// 批表（只读）。
    pub fn batches(&self) -> &[Batch] {
        &self.batches
    }

    /// 输出流（批号为批下标；原路绘制为 [`FALLBACK_MARK`]）。
    pub fn output(&self) -> &[i32] {
        &self.output
    }

    /// 由打断归因生成资产建议（**打断率超阈才出**，且只针对内容类原因）。
    pub fn advise(&self, submitted: u32) -> Vec<AssetAdvice> {
        let mut out = Vec::new();
        if self.break_rate_permille(submitted) <= BREAK_RATE_ALERT {
            return out;
        }
        for r in BreakReason::ALL.iter() {
            if !r.is_content() || self.breaks[r.ordinal()] == 0 {
                continue;
            }
            let action = match r {
                BreakReason::MaterialSwitch => AdviceAction::MergeMaterials,
                BreakReason::PipelineSwitch => AdviceAction::SplitPipeline,
                BreakReason::TopologyIncompatible => AdviceAction::ReorderDraws,
                BreakReason::BatchTableFull => AdviceAction::ReorderDraws,
            };
            out.push(AssetAdvice {
                action,
                reason: *r,
                detail: format!("{}：{} 次，建议{}", r.zh(), self.breaks[r.ordinal()], action.zh()),
            });
        }
        out
    }

    /// 读屏面板：中英双语逐行，**只报聚合计数**，不报单个绘制的参数。
    pub fn a11y_lines(&self, st: &SealStat) -> [String; 6] {
        [
            format!("绘制总数 / total draws: {}", st.draws_total),
            format!("批数 / batches: {}", st.batches_total),
            format!("合批收益 / calls saved: {}", st.calls_saved),
            format!("原路绘制 / fallbacks: {}", st.fallbacks),
            format!("内容打断率 / break rate: {}‰", self.content_breaks),
            format!("动态实例入批 / dynamic merged: {}", st.dynamic_in_batched),
        ]
    }
}

/// 提交结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubmitOutcome {
    /// 并入既有批（批下标）。
    Merged(usize),
    /// 新开一批（批下标）。
    Opened(usize),
    /// 被拒（附专属原因）。
    Rejected(BreakReason),
}

/// 收批统计。
#[derive(Clone, Debug, Default)]
pub struct SealStat {
    /// 批总数。
    pub batches_total: u32,
    /// 走合批的批数。
    pub batched: u32,
    /// 因收益不足被放弃的批数。
    pub abandoned: u32,
    /// 走合批的绘制数。
    pub draws_batched: u32,
    /// 被放弃批内的绘制数（**仍照原样逐条绘制**）。
    pub draws_abandoned: u32,
    /// 提交绘制总数（= `draws_batched + draws_abandoned`；合批是重排不是丢弃，
    /// 故此数恒等于提交量——判据据此断守恒）。
    pub draws_total: u32,
    /// 原路绘制条数（= `draws_abandoned`，独立记账供对账）。
    pub fallbacks: u32,
    /// 省下的驱动调用数（绝对值口径）。
    pub calls_saved: u32,
    /// 净收益合计（有符号）。
    pub net_gain: i64,
    /// 走合批批内的动态实例绘制数。
    pub dynamic_in_batched: u32,
    /// 逐批收益账（按批下标；未及 `seal` 的批为零值）。
    pub gains: Vec<GainReport>,
}

// ---------------------------------------------------------------------------
// 四、判据
// ---------------------------------------------------------------------------

/// VE-F0030 模块自检（受 `CheckSet::MAX_CHECKS=112` 约束，逐条覆盖锚点判据）。
pub fn run_vea30_checks() -> CheckSet {
    let mut s = CheckSet::new("vea30_batcher");

    // --- 判据 1：批键三维度缺一即错合（构造期就拒全零键）---------------------
    s.add(
        "A30-键-三维度齐备且全零键判无效",
        {
            let good = BatchKey::new(1, 2, 3);
            let zero = BatchKey::new(0, 0, 0);
            // 仅一维非零也**有效**——键不要求三维全非零，只要求不是全零
            let kind_only = BatchKey::new(7, 0, 0);
            let mat_only = BatchKey::new(0, 0, 9);
            good.is_valid()
                && !zero.is_valid()
                && kind_only.is_valid()
                && mat_only.is_valid()
                // 指纹把三维打包进不同位段，故三维不同的键指纹必不同
                && good.fingerprint() != BatchKey::new(1, 0, 0).fingerprint()
                && good.fingerprint() != BatchKey::new(0, 2, 0).fingerprint()
                && good.fingerprint() != BatchKey::new(0, 0, 3).fingerprint()
        },
        "批键含种类/管线/材质三维，指纹分段打包故三维各自可辨；全零键判无效",
    );

    // --- 判据 2：同材质同管线的连续绘制自动合批 ------------------------------
    {
        let mut e = BatchEngine::new();
        let k = BatchKey::new(1, 2, 3);
        let it = BatchItem { key: k, vertices: 6, dynamic: false };
        let o1 = e.submit(it);
        let o2 = e.submit(it);
        let o3 = e.submit(it);
        s.add(
            "A30-自动合批-同键三条并入一批且输出一个批号",
            o1 == SubmitOutcome::Opened(0)
                && o2 == SubmitOutcome::Merged(0)
                && o3 == SubmitOutcome::Merged(0)
                && e.batches().len() == 1
                && e.batches()[0].draws == 3,
            "同材质同管线的三次绘制应自动并入同一批",
        );
    }

    // --- 判据 3：材质切换打断并归因到专属类别 --------------------------------
    {
        let prev = BatchKey::new(1, 2, 3);
        let mat = BatchKey::new(1, 2, 4);
        let pipe = BatchKey::new(1, 9, 3);
        let topo = BatchKey::new(2, 2, 3);
        let same = BatchKey::new(1, 2, 3);
        s.add(
            "A30-归因-材质管线拓扑三类专属且同键不打断",
            BatchEngine::classify(prev, mat) == Some(BreakReason::MaterialSwitch)
                && BatchEngine::classify(prev, pipe) == Some(BreakReason::PipelineSwitch)
                && BatchEngine::classify(prev, topo) == Some(BreakReason::TopologyIncompatible)
                && BatchEngine::classify(prev, same).is_none(),
            "材质异/管线异/拓扑异各归各码；三维全同则不打断",
        );
    }

    // --- 判据 4：材质与管线同时不同 → 报直接原因（材质），不报管线 ----------
    {
        // 材质是批键主序；两者同时不同，直接原因是材质。报管线会把直接原因
        // 掩盖成间接原因，按错误码给的修复建议也就错了。
        let prev = BatchKey::new(1, 2, 3);
        let both = BatchKey::new(1, 9, 4);
        s.add(
            "A30-归因-材质与管线同时不同报直接原因材质",
            BatchEngine::classify(prev, both) == Some(BreakReason::MaterialSwitch),
            "材质与管线同时不同时应归因材质（主序），而非管线",
        );
    }

    // --- 判据 5：打断计数为绝对值口径（被拒绘制数），且容量类不污染内容类 ---
    {
        let mut e = BatchEngine::new();
        // 交替材质 → 每次都打断材质
        let a = BatchItem { key: BatchKey::new(1, 1, 1), vertices: 3, dynamic: false };
        let b = BatchItem { key: BatchKey::new(1, 1, 2), vertices: 3, dynamic: false };
        e.submit(a);
        e.submit(b);
        e.submit(a);
        e.submit(b);
        let c = e.break_counts();
        // 内容类打断 = 材质切换计数；容量类恒零（未灌满批表）
        s.add(
            "A30-归因-打断按绝对值计且容量类独立不污染",
            c[BreakReason::MaterialSwitch.ordinal()] == 3
                && c[BreakReason::PipelineSwitch.ordinal()] == 0
                && c[BreakReason::TopologyIncompatible.ordinal()] == 0
                && c[BreakReason::BatchTableFull.ordinal()] == 0,
            "四次提交产生三次材质打断，按被拒绘制数绝对计数；容量类独立为 0",
        );
    }

    // --- 判据 6：全零键绘制被拒且不进入任何批 -------------------------------
    {
        let mut e = BatchEngine::new();
        let bad = BatchItem { key: BatchKey::new(0, 0, 0), vertices: 3, dynamic: false };
        let r = e.submit(bad);
        let st = e.seal();
        s.add(
            "A30-边界-全零键被拒且不进批不参与输出",
            r == SubmitOutcome::Rejected(BreakReason::TopologyIncompatible)
                && e.batches().is_empty()
                && st.draws_total == 0
                && e.output().is_empty(),
            "未填键的绘制应被拒，既不开批也不进输出流",
        );
    }

    // --- 判据 7：收益成式实算（省调用数减批开销，可为负）--------------------
    {
        // 独立重算：draws 条绘制不合批要 draws 次调用，合批后 1 次，
        // 省 draws-1 次，每批开销 BATCH_OVERHEAD=2。
        let mut all_ok = true;
        for d in 1u32..40 {
            let g = GainReport::compute(d);
            let want_saved = d - 1;
            let want_net = i64::from(want_saved) - i64::from(BATCH_OVERHEAD);
            all_ok &= g.calls_unbatched == d
                && g.calls_batched == 1
                && g.calls_saved == want_saved
                && g.overhead == BATCH_OVERHEAD
                && g.net_gain == want_net
                && g.calls_saved == g.calls_unbatched - g.calls_batched;
        }
        // 小批净收益为负（1 条绘制：省 0、开销 2 → 净 -2）
        let tiny = GainReport::compute(1);
        s.add(
            "A30-收益-成式实算且小批净收益为负",
            all_ok && tiny.net_gain < 0 && !tiny.worth_batching(),
            "省下的调用数减每批开销等于净收益；1 条绘制净收益为负故不合批",
        );
    }

    // --- 判据 8：夹逼对钉死收益阈位置（净收益 0 不合、恰 1 才合）-----------
    {
        // 净收益 = (draws-1) - 2 = draws - 3。阈值「严格大于 0」⇒ draws>=4 才合。
        // 找出净收益恰为 0 与恰为 1 的两个批，前者必须放弃、后者必须合。
        let zero_draws = (BATCH_OVERHEAD as u32) + 1; // net = (d-1)-2 = 0
        let one_draws = (BATCH_OVERHEAD as u32) + 2; // net = 1
        let gz = GainReport::compute(zero_draws);
        let go = GainReport::compute(one_draws);
        s.add(
            "A30-收益-夹逼对钉死阈位置（0放弃1合）",
            gz.net_gain == 0
                && !gz.worth_batching()
                && go.net_gain == 1
                && go.worth_batching(),
            "净收益恰为 0 的批不合、恰为 1 的批合——阈值写成 >= 会被此判据抓住",
        );
    }

    // --- 判据 9：净收益为负的批放弃合批，且其绘制逐条原路输出 --------------
    {
        let mut e = BatchEngine::new();
        let k = BatchKey::new(1, 2, 3);
        // 只提交 2 条 → 净收益 (2-1)-2 = -1 < 0 ⇒ 放弃
        let it = BatchItem { key: k, vertices: 3, dynamic: false };
        e.submit(it);
        e.submit(it);
        let st = e.seal();
        let marks = e.output().iter().filter(|&&v| v == FALLBACK_MARK).count();
        s.add(
            "A30-降级-负收益批放弃且绘制逐条原路在场",
            st.abandoned == 1
                && st.batched == 0
                && st.draws_abandoned == 2
                && st.fallbacks == 2
                && marks == 2
                && e.output().len() == 2,
            "净收益为负的批放弃合批，但其 2 条绘制在输出流里逐条标原路（不丢弃）",
        );
    }

    // --- 判据 10：放弃批的原路条数与被放弃批内绘制数独立对账 ---------------
    {
        let mut e = BatchEngine::new();
        // 三个小批（各 2 条，净 -1）⇒ 放弃 3 批、共 6 条
        for m in 1u16..4 {
            let it = BatchItem { key: BatchKey::new(1, 2, m), vertices: 3, dynamic: false };
            e.submit(it);
            e.submit(it);
        }
        let st = e.seal();
        // 独立重算：放弃批数 = 批表中净收益 <= 0 的批数
        let want_abandoned = e
            .batches()
            .iter()
            .filter(|b| !GainReport::compute(b.draws).worth_batching())
            .count() as u32;
        let want_draws: u32 = e
            .batches()
            .iter()
            .filter(|b| !GainReport::compute(b.draws).worth_batching())
            .map(|b| b.draws)
            .sum();
        s.add(
            "A30-降级-原路条数与被放弃批内绘制数绝对对账",
            st.abandoned == want_abandoned
                && st.fallbacks == want_draws
                && st.draws_total == 6
                && st.draws_abandoned == 6,
            "原路条数恰等于独立重算的被放弃批内绘制数之和（绝对值口径）",
        );
    }

    // --- 判据 11：合批收益为绝对值口径（省下的调用数逐批实算求和）---------
    {
        let mut e = BatchEngine::new();
        // 两个大批（各 6 条）：净收益 (6-1)-2 = 3 > 0 ⇒ 合
        for m in 1u16..3 {
            let it = BatchItem { key: BatchKey::new(1, 2, m), vertices: 3, dynamic: false };
            let mut k = 0;
            while k < 6 {
                e.submit(it);
                k += 1;
            }
        }
        let st = e.seal();
        // 独立重算：每批 calls_saved = 批内绘制数 - 1
        let want_saved: u32 = e
            .batches()
            .iter()
            .map(|b| b.draws.saturating_sub(1))
            .sum();
        s.add(
            "A30-收益-合批省下的调用数绝对口径对账",
            st.batched == 2
                && st.calls_saved == want_saved
                && st.calls_saved == 10
                && st.net_gain == 6,
            "两批各 6 条各省 5 次调用，合计 10 次；净收益 3+3=6（绝对值口径）",
        );
    }

    // --- 判据 12：动态实例照合不误（不构成打断原因）------------------------
    {
        let mut e = BatchEngine::new();
        let k = BatchKey::new(1, 2, 3);
        let stat = BatchItem { key: k, vertices: 3, dynamic: true };
        let dyn_a = BatchItem { key: k, vertices: 3, dynamic: true };
        let mut o1 = e.submit(stat);
        o1 = match o1 {
            SubmitOutcome::Opened(_) => o1,
            _ => o1,
        };
        e.submit(dyn_a);
        e.submit(stat);
        let c = e.break_counts();
        let content_breaks: u32 = BreakReason::ALL
            .iter()
            .filter(|r| r.is_content())
            .map(|r| c[r.ordinal()])
            .sum();
        let st = e.seal();
        // 关键：动态实例**不构成打断**（内容打断恒 0、且三条并入同一批），
        // 这与「合不合成批」是两件事——3 条绘制的净收益 (3-1)-2 = 0，
        // 恰在阈值上被**放弃**，那是收益判定，不是动态性导致的。
        s.add(
            "A30-动态-动态实例照合不误且不计打断",
            matches!(o1, SubmitOutcome::Opened(_))
                && e.batches().len() == 1
                && e.batches()[0].draws == 3
                && e.batches()[0].dynamic_draws == 3
                && content_breaks == 0
                && st.abandoned == 1
                && st.batched == 0,
            "三条动态实例绘制合入同一批且零内容打断：合批改的是驱动调用次数，\
             不是实例数据可变性（该批因净收益恰为 0 而放弃，与动态无关）",
        );
    }

    // --- 判据 13：动态与非动态混合同键仍合批（动态性不进批键）--------------
    {
        let mut e = BatchEngine::new();
        let k = BatchKey::new(1, 2, 3);
        e.submit(BatchItem { key: k, vertices: 3, dynamic: true });
        e.submit(BatchItem { key: k, vertices: 3, dynamic: false });
        s.add(
            "A30-动态-动静态混合同键仍合批",
            e.batches().len() == 1
                && e.batches()[0].draws == 2
                && e.batches()[0].dynamic_draws == 1,
            "实例数据可变性不是批键维度，动态与非动态同键绘制仍应合批",
        );
    }

    // --- 判据 13b：动态批次在收益为正时**真的**被合（判据 12 的正向补强）----
    {
        // 判据 12 的批净收益恰为 0（被放弃），那只证了「动态不打断」。
        // 这里用 6 条动态绘制（净收益 (6-1)-2 = 3 > 0）证明动态批次**确实
        // 走合批路**并带上 dynamic 计数——否则「动态一律不合批」这个错实现
        // 也能让判据 12 全绿（十二诫：只在阈值另一侧取样的判据是弱门禁）。
        let mut e = BatchEngine::new();
        let it = BatchItem { key: BatchKey::new(1, 2, 3), vertices: 3, dynamic: true };
        for _ in 0..6 {
            e.submit(it);
        }
        let st = e.seal();
        s.add(
            "A30-动态-收益为正时动态批次真被合并",
            e.batches().len() == 1
                && e.batches()[0].draws == 6
                && e.batches()[0].dynamic_draws == 6
                && st.batched == 1
                && st.abandoned == 0
                && st.draws_batched == 6
                && st.dynamic_in_batched == 6,
            "六条动态绘制净收益为正故走合批路，动态计数随批入账——动态不是不合批的理由",
        );
    }

    // --- 判据 14：批表容量上限拦截且归入容量类（不污染内容类打断率）--------
    {
        let mut e = BatchEngine::new();
        let mut rejected = 0u32;
        for m in 0..(MAX_BATCHES as u16 + 8) {
            // 每个不同材质 = 不同键 ⇒ 每条都新开一批
            let r = e.submit(BatchItem {
                key: BatchKey::new(1, 1, m.wrapping_add(1)),
                vertices: 3,
                dynamic: false,
            });
            if matches!(r, SubmitOutcome::Rejected(_)) {
                rejected += 1;
            }
        }
        let c = e.break_counts();
        s.add(
            "A30-边界-批表容量上限拦截且归容量类",
            e.batches().len() == MAX_BATCHES
                && rejected == 8
                && c[BreakReason::BatchTableFull.ordinal()] == 8
                // 容量类计数恰等于被拒条数（绝对值口径），且**不**依赖内容类：
                // 内容类计数由语料自身的材质切换决定，与容量无关。
                && c[BreakReason::BatchTableFull.ordinal()] == rejected
                && c[BreakReason::PipelineSwitch.ordinal()] == 0
                && c[BreakReason::TopologyIncompatible.ordinal()] == 0,
            "批表满后新批被拒且只记容量类；被拒条数与容量类计数绝对相等",
        );
    }

    // --- 判据 15：单批绘制数上限（超出不再并入该批而是新开一批）-----------
    {
        let mut e = BatchEngine::new();
        let k = BatchKey::new(1, 2, 3);
        let it = BatchItem { key: k, vertices: 3, dynamic: false };
        let mut i = 0;
        let mut merged = 0u32;
        let mut opened = 0u32;
        while i < MAX_BATCH_DRAWS + 4 {
            match e.submit(it) {
                SubmitOutcome::Merged(_) => merged += 1,
                SubmitOutcome::Opened(_) => opened += 1,
                SubmitOutcome::Rejected(_) => {}
            }
            i += 1;
        }
        s.add(
            "A30-边界-单批绘制数到顶后另开新批并接着装",
            e.batches().len() == 2
                && e.batches()[0].draws == MAX_BATCH_DRAWS as u32
                // 第二批承接后续绘制，且**继续装到本批自己的上限**，
                // 不是「第一批一满就每条新开一批」的按条分裂。
                && e.batches()[1].draws == 4
                // 共提交 MAX+4 条：第 1 条开批，余下 MAX+3 条全部并入
                //（前 MAX-1 条进第一批，后 4 条进第二批）。
                && merged == MAX_BATCH_DRAWS as u32 + 2
                && opened == 2
                && e.batches()[0].draws + e.batches()[1].draws == MAX_BATCH_DRAWS as u32 + 4,
            "同键批满 MAX_BATCH_DRAWS 条后另开一批接着装，总批数为 2 而非按条分裂",
        );
    }

    // --- 判据 16：打断率按内容类算，容量类不计入 -----------------------------
    {
        let mut e = BatchEngine::new();
        // 灌满批表后再提交 ⇒ 产生容量类打断。
        // 注意：连续不同材质**本身也是内容类打断**（每条都换材质），所以这里
        // 不能断「内容打断为 0」——那是在断一个不成立的事实。真正要断的是
        // **容量类打断不抬高打断率**：把内容类计数换成排除容量类后的口径，
        // 打断率应恰好等于「内容打断数 / 提交数」，与容量类计数无关。
        for m in 0..(MAX_BATCHES as u16 + 4) {
            e.submit(BatchItem {
                key: BatchKey::new(1, 1, m.wrapping_add(1)),
                vertices: 3,
                dynamic: false,
            });
        }
        let submitted = MAX_BATCHES as u32 + 4;
        let rate = e.break_rate_permille(submitted);
        let c = e.break_counts();
        let cap = c[BreakReason::BatchTableFull.ordinal()];
        let content: u32 = BreakReason::ALL
            .iter()
            .filter(|r| r.is_content())
            .map(|r| c[r.ordinal()])
            .sum();
        // 独立重算：率 = 内容打断 / 提交数（千分比），容量类不出现在分子
        let want = ((content as u64 * 1000) / submitted as u64) as u32;
        s.add(
            "A30-归因-打断率只计内容类（容量类不入分子）",
            cap == 4
                && content > 0
                && rate == want
                && rate == ((content * 1000) / submitted)
                // 若容量类被误计入分子，率会比 want 大（这里内容+容量 > 内容）
                && want < ((content + cap) * 1000 / submitted)
                && e.break_rate_permille(0) == 0,
            "容量类打断不入打断率分子：率恰等于内容打断数除以提交数",
        );
    }

    // --- 判据 17：打断率超阈才出资产建议，且逐内容类原因成条目 -----------
    {
        let mut e = BatchEngine::new();
        // 交替材质制造大量内容类打断 ⇒ 率必然超阈
        let a = BatchItem { key: BatchKey::new(1, 1, 1), vertices: 3, dynamic: false };
        let b = BatchItem { key: BatchKey::new(1, 1, 2), vertices: 3, dynamic: false };
        let mut i = 0;
        while i < 40 {
            e.submit(a);
            e.submit(b);
            i += 1;
        }
        let submitted = 80u32;
        let rate = e.break_rate_permille(submitted);
        let advice = e.advise(submitted);
        let c = e.break_counts();
        // 语料只交替两种材质 ⇒ **只**产生材质类打断，故建议恰一条。
        // （早期版本断「三条建议」是断了个不成立的事实：管线与拓扑并未
        //  被打断，没发生的事不该有建议——建议必须与实际打断一一对应。）
        // 逐类建议的完整性由判据 19（三种原因都造出来）承担。
        s.add(
            "A30-建议-打断率超阈出建议且与实际打断一一对应",
            rate > BREAK_RATE_ALERT
                && c[BreakReason::MaterialSwitch.ordinal()] > 0
                && c[BreakReason::PipelineSwitch.ordinal()] == 0
                && c[BreakReason::TopologyIncompatible.ordinal()] == 0
                && advice.len() == 1
                && advice[0].reason == BreakReason::MaterialSwitch
                && advice.iter().all(|x| x.reason.is_content())
                && advice.iter().all(|x| !x.detail.is_empty()),
            "高打断率下逐**实际发生**的内容类原因各出一条建议，无未发生原因的建议",
        );
    }

    // --- 判据 18：低打断率不出建议（建议不是无条件刷屏）-------------------
    {
        let e = BatchEngine::new();
        // 全新引擎：0 打断 ⇒ 率 0 ≤ 阈 ⇒ 无建议
        let advice_empty = e.advise(10);
        // 全同键连续绘制：0 打断 ⇒ 无建议
        let mut e2 = BatchEngine::new();
        let it = BatchItem { key: BatchKey::new(1, 2, 3), vertices: 3, dynamic: false };
        e2.submit(it);
        e2.submit(it);
        e2.submit(it);
        let advice_none = e2.advise(3);
        s.add(
            "A30-建议-低打断率不出建议",
            advice_empty.is_empty() && advice_none.is_empty(),
            "打断率未超阈时不出资产建议（避免无据建议刷屏）",
        );
    }

    // --- 判据 19：建议动作与原因对应（材质→合并、管线→拆分、拓扑→重排）----
    {
        let mut e = BatchEngine::new();
        // 语料必须**三类原因都真的发生**，否则建议映射表有一整类从未被走到，
        // 那条映射写错也不会被发现。
        //   a→b：材质变（1,1,1)→(1,1,2) ⇒材质类
        //   b→c：材质同、管线变 (1,1,2)→(1,5,2) ⇒ 管线类
        //   c→d：材质同、管线同、**种类**变 (1,5,2)→(2,5,2) ⇒ 拓扑类
        let a = BatchItem { key: BatchKey::new(1, 1, 1), vertices: 3, dynamic: false };
        let b = BatchItem { key: BatchKey::new(1, 1, 2), vertices: 3, dynamic: false };
        let c = BatchItem { key: BatchKey::new(1, 5, 2), vertices: 3, dynamic: false };
        let d = BatchItem { key: BatchKey::new(2, 5, 2), vertices: 3, dynamic: false };
        let mut i = 0;
        while i < 40 {
            e.submit(a);
            e.submit(b);
            e.submit(c);
            e.submit(d);
            i += 1;
        }
        let cnt = e.break_counts();
        let advice = e.advise(160);
        let pick = |r: BreakReason| advice.iter().find(|x| x.reason == r).map(|x| x.action);
        s.add(
            "A30-建议-动作与打断原因逐项对应（三类均实测）",
            // 前置：三类打断**确实都发生过**（否则下面三项是空断言）
            cnt[BreakReason::MaterialSwitch.ordinal()] > 0
                && cnt[BreakReason::PipelineSwitch.ordinal()] > 0
                && cnt[BreakReason::TopologyIncompatible.ordinal()] > 0
                && advice.len() == BREAK_COUNT
                && pick(BreakReason::MaterialSwitch) == Some(AdviceAction::MergeMaterials)
                && pick(BreakReason::PipelineSwitch) == Some(AdviceAction::SplitPipeline)
                && pick(BreakReason::TopologyIncompatible) == Some(AdviceAction::ReorderDraws),
            "三类内容打断均实测发生，各给出对应建议动作：材质合并材质、管线拆分状态、拓扑重排绘制序",
        );
    }

    // --- 判据 20：输出流里批号与原路标记不混淆（正数批号 / -1 原路）--------
    {
        let mut e = BatchEngine::new();
        // 大批（合）+ 小批（弃）并存 ⇒ 输出流应同时含正数批号与 FALLBACK_MARK
        for _ in 0..6 {
            e.submit(BatchItem { key: BatchKey::new(1, 2, 3), vertices: 3, dynamic: false });
        }
        e.submit(BatchItem { key: BatchKey::new(1, 2, 4), vertices: 3, dynamic: false });
        e.submit(BatchItem { key: BatchKey::new(1, 2, 4), vertices: 3, dynamic: false });
        let st = e.seal();
        let batched_ids: Vec<i32> =
            e.output().iter().copied().filter(|&v| v != FALLBACK_MARK).collect();
        let fallback_ids: Vec<i32> =
            e.output().iter().copied().filter(|&v| v == FALLBACK_MARK).collect();
        s.add(
            "A30-输出-批号与原路标记语义不混淆",
            st.batched == 1
                && st.abandoned == 1
                && batched_ids == vec![0]
                && fallback_ids == vec![FALLBACK_MARK, FALLBACK_MARK]
                && FALLBACK_MARK < 0,
            "合批批输出非负批号、放弃批输出 -1 标记，两者语义不混淆",
        );
    }

    // --- 判据 21：收批后批表与批收益账逐批对齐（无悬空批）------------------
    {
        let mut e = BatchEngine::new();
        for m in 1u16..4 {
            let it = BatchItem { key: BatchKey::new(1, 2, m), vertices: 3, dynamic: false };
            e.submit(it);
            e.submit(it);
            let _ = m;
        }
        let st = e.seal();
        // 逐批重算收益账应与 seal 写入的一致，且批数与账目条数相等
        let mut consistent = st.gains.len() == e.batches().len();
        for (bi, b) in e.batches().iter().enumerate() {
            let want = GainReport::compute(b.draws);
            consistent &= st.gains[bi] == want;
            consistent &= want.draws == b.draws;
        }
        s.add(
            "A30-收批-收益账逐批与批表对齐无悬空",
            consistent && st.batches_total == e.batches().len() as u32,
            "seal 写入的逐批收益账与批表逐项重算一致，批数无出入",
        );
    }

    // --- 判据 22：合批不改变绘制总量（合批是重排不是丢弃）------------------
    {
        let mut e = BatchEngine::new();
        let mut submitted = 0u32;
        for m in 1u16..4 {
            let it = BatchItem { key: BatchKey::new(1, 2, m), vertices: 3, dynamic: false };
            e.submit(it);
            e.submit(it);
            submitted += 2;
        }
        let st = e.seal();
        // 输出流长度 = 合批批号 1 条/批 + 原路标记 1 条/绘制
        let expect_out = st.batched as usize + st.fallbacks as usize;
        s.add(
            "A30-守恒-合批不改变绘制总量",
            st.draws_total == submitted && e.output().len() == expect_out,
            "收批后绘制总量等于提交量；输出流条目数等于合批批数加原路条数",
        );
    }

    // --- 判据 23：读屏面板双语齐备且不泄漏单条绘制参数 ---------------------
    {
        let mut e = BatchEngine::new();
        let it = BatchItem { key: BatchKey::new(1, 2, 3), vertices: 4242, dynamic: false };
        for _ in 0..6 {
            e.submit(it);
        }
        let st = e.seal();
        let lines = e.a11y_lines(&st);
        // 六行齐备、双语标签齐备，且**不含**顶点数 4242 这类单条绘制参数
        let joined = lines.join("|");
        s.add(
            "A30-读屏-六行双语且不泄漏单条绘制参数",
            lines.len() == 6
                && lines.iter().all(|l| !l.is_empty())
                && lines.iter().any(|l| l.contains("total draws"))
                && lines.iter().any(|l| l.contains("calls saved"))
                && lines.iter().any(|l| l.contains("fallbacks"))
                && !joined.contains("4242"),
            "面板六行中英双语，只报聚合计数，不泄漏单条绘制的顶点数",
        );
    }

    // --- 判据 24：打断原因下标互不重叠且全集可枚举 -------------------------
    {
        let mut seen = [false; BREAK_COUNT + 1];
        let mut all_distinct = true;
        for r in BreakReason::ALL.iter() {
            let i = r.ordinal();
            if i >= BREAK_COUNT + 1 || seen[i] {
                all_distinct = false;
            }
            seen[i] = true;
        }
        // 内容类恰三条、容量类恰一条
        let content = BreakReason::ALL.iter().filter(|r| r.is_content()).count();
        s.add(
            "A30-归因-打断下标唯一且内容类恰三条",
            all_distinct
                && seen[0]
                && seen[1]
                && seen[2]
                && seen[3]
                && content == BREAK_COUNT
                && BreakReason::MaterialSwitch.zh() != BreakReason::PipelineSwitch.zh()
                && BreakReason::MaterialSwitch.tag() != BreakReason::PipelineSwitch.tag(),
            "四种打断原因下标互不重叠，内容类恰三条，中英标签互异",
        );
    }

    s
}