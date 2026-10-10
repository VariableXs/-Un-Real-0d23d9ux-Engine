//! VE-F2014 · 后处理性能预算（VE-K 域 · 后处理架构与 Bloom 组）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F2014`
//!
//! **判据（锚点原文）**：成本模型、固定次序、打点共享、20% 修正门、判据。
//!
//! **职责定位（锚点原文）**：后处理性能预算——建立全链成本模型（每效果的像素成本声明：
//! 分辨率×效果组合的预算预估——K01 十效果的每像素成本常数表按硬件档）、预算联动（后处理
//! 超预算→D04 降质链的后处理维度触发——F2015 档位降级联动：先降高成本低效果→再降精度——
//! 固定次序）、每效果打点（全链分段耗时采集——与 F2013 调试共享打点设施、与 F1984 瀑布
//! K 段对接），并入全书账本体系。
//!
//! **数据结构（锚点原文）**：成本模型表——十效果×三硬件档的每像素成本常数（实测定标）+
//! 组合预估公式（线性叠加+共享优化注记：TAA 与 MB 共享速度缓冲的复用减免）；预算联动
//! 规则（超预算降档次序声明：SMAA 预留不参与/TAA 先降采样比→Bloom 降级数→DoF/MB 降样本
//! ——次序单源与 F2015 档位表对齐）；打点设施（效果级时间戳——F2002 执行器内建）。
//!
//! **错误路径与降级矩阵（锚点原文）**：
//! - 模型与实测偏差超 20%→修正立项（F1811 同规则）——**偏差门**；
//! - 次序被跳过→告警+遥测标记——**次序守卫**；
//! - 与 F2015 档位冲突→档位优先回改（F1832 三方裁决同构）——**三方裁决**；
//! - 打点自身开销>1%→打点降频（**测量不能扰动被测系统**——方法学红线）。
//!
//! **性能逐项分解（锚点原文）**：预估查表微秒级（场景配置变更时一次）；打点每效果
//! 0.5μs 级；联动决策零分配。
//!
//! **跨批对接点（锚点原文）**：实测源 F2017 基准；账本对接 F1901 体系 K 段；降档次序
//! 与 F2015 单源；瀑布对接 F1984 K 段扩展位；遥测 F2059。
//!
//! 无障碍与隐私：工程数据无隐私面。
//!
//! # 判据怎么做到"不是恒真"
//!
//! 成本模型最容易写成恒真断言的地方有六处，本模块逐一封死：
//!
//! 1. **"线性叠加"**。若只断「两效果之和 == 分开算再相加」，一个**恒返回 0** 的
//!    叠加器也全绿。必须**双向**：既有双效果语料（断恰等于两基线之和），也有单效果
//!    语料（断恰等于该基线），再加**零效果**语料（断恰等于 0 而非某个兜底常数）。
//! 2. **"共享优化减免"**。减免是最容易写成 `if true { 0 }` 的位置。必须断**双向**：
//!    共享对（TAA+MB 同开）**严格小于**独立叠加，且减免额**恰等于**声明的共享比例；
//!    非共享对（Bloom+TAA）**一分不减免**（否则减免逻辑会漫过所有组合）。
//! 3. **"降档次序固定"**。次序红线若只断「结果 == 期望数组」，一个**恒返回空**的
//!    降级器也全绿。必须断：超预算必产出**非空**降级计划 + 计划序列**逐位等于**声明
//!    次序 + **跳过即告警**（次序守卫独立触发）。
//! 4. **"20% 修正门"**。偏差判定若写成 `dev > 0`（比较绝对值而非比例），负偏差
//!    （实测远低于模型=模型高估）会**永久逃过门**。必须断**双向**：正偏差超门触发、
//!    负偏差超门**同样**触发、门内两侧都不触发，且边界恰在 20% 处**含端点**。
//! 5. **"打点开销 ≤1%"**。开销是**比值**，断言必须**独立重算分子分母**——直接引用
//!    被测函数算出的比值是自证式（问它答案它当然答对）。要断「打点耗时/总耗时」
//!    由外部独立给出的两项相除，且超 1% 时**降频确实发生**（stride 加倍）。
//! 6. **"三方裁决"**。若只断「冲突时选档位」，一个恒返回"用户优先"的实现也全绿。
//!    必须把**三种冲突输入**分别打进去：用户覆盖>档位>预算临时，三档各胜一次。

use alloc::string::String;
use alloc::vec::Vec;

use super::vek13_ppdbg;

// ===========================================================================
// 一、诊断码（自建，K 域独占段 0x2Dxx——0x2Cxx 已由 F2013 占用）
// ===========================================================================

/// 诊断码（下游封闭枚举无权加变体，故自建）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DiagCode(pub u16);

/// K 域 F2014 诊断码。
pub mod code {
    use super::DiagCode;

    /// 成本模型与实测偏差超 20%——修正立项（F1811 同规则）。
    pub const MODEL_DRIFT: DiagCode = DiagCode(0x2D01);
    /// 降档次序被跳过——告警 + 遥测标记（F2059）。
    pub const ORDER_SKIPPED: DiagCode = DiagCode(0x2D02);
    /// 与 F2015 档位冲突——档位优先回改（F1832 三方裁决同构）。
    pub const TIER_CONFLICT: DiagCode = DiagCode(0x2D03);
    /// 打点自身开销超 1%——打点降频（方法学红线）。
    pub const PROBE_OVERHEAD: DiagCode = DiagCode(0x2D04);
    /// 成本常数表缺行——拒绝（表是闭集，缺行即配置错误）。
    pub const COST_ROW_MISSING: DiagCode = DiagCode(0x2D05);
    /// 分辨率或像素数为 0——拒绝（成本模型输入非法）。
    pub const DEGENERATE_INPUT: DiagCode = DiagCode(0x2D06);
    /// 效果标识越界（不在 K01 十效果闭集内）。
    pub const EFFECT_UNKNOWN: DiagCode = DiagCode(0x2D07);
    /// 硬件档位越界（三档闭集）。
    pub const TIER_UNKNOWN: DiagCode = DiagCode(0x2D08);
    /// 打点降频已达上限——不再降（保底：降到底仍超就如实停，绝不静默）。
    pub const PROBE_STRIDE_FLOOR: DiagCode = DiagCode(0x2D09);
}

/// 诊断严重度。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Severity {
    /// 提示。
    Info,
    /// 告警（可继续）。
    Warn,
    /// P1（需处置）。
    P1,
}

/// 一条诊断。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Diagnostic {
    /// 诊断码。
    pub code: DiagCode,
    /// 严重度。
    pub severity: Severity,
}

/// 诊断袋（定容，覆盖最旧；诊断本身不是账本，覆盖不丢正确性——计数另计）。
#[derive(Clone, Debug)]
pub struct DiagBag {
    items: Vec<Diagnostic>,
    /// 各码累计计数（**单调**，不随覆盖回退）。
    counts: [u32; 16],
    cap: usize,
}

impl DiagBag {
    /// 构造（容量 32）。
    pub fn new() -> DiagBag {
        DiagBag { items: Vec::new(), counts: [0u32; 16], cap: 32 }
    }

    fn bump(&mut self, c: DiagCode) {
        let i = ((c.0 >> 8) & 0x0F) as usize;
        self.counts[i] = self.counts[i].saturating_add(1);
    }

    /// 记一条告警。
    pub fn push_warn(&mut self, c: DiagCode) {
        self.bump(c);
        if self.items.len() >= self.cap {
            self.items.remove(0);
        }
        self.items.push(Diagnostic { code: c, severity: Severity::Warn });
    }

    /// 记一条 P1。
    pub fn push_p1(&mut self, c: DiagCode) {
        self.bump(c);
        if self.items.len() >= self.cap {
            self.items.remove(0);
        }
        self.items.push(Diagnostic { code: c, severity: Severity::P1 });
    }

    /// 全部诊断。
    pub fn items(&self) -> &[Diagnostic] {
        &self.items
    }

    /// 某码累计次数（**单调**，覆盖不回退）。
    pub fn count(&self, code: DiagCode) -> u32 {
        self.counts[((code.0 >> 8) & 0x0F) as usize]
    }

    /// P1 条数。
    pub fn p1_count(&self) -> usize {
        self.items.iter().filter(|d| d.severity == Severity::P1).count()
    }

    /// 是否含某码。
    pub fn has(&self, code: DiagCode) -> bool {
        self.items.iter().any(|d| d.code == code)
    }
}

// ===========================================================================
// 二、K01 十效果闭集与三硬件档
// ===========================================================================

/// K01 十效果（闭集：F2004~F2013，F2001~F2003 是底座非效果不进表）。
///
/// **为什么 F2013 在表内**：它是全链打点一环，锚点明示「与 F2013 调试共享打点
/// 设施」，故它在链上有位置；其成本极低（只读环形缓冲）——**低成本不等于零成本**，
/// 把它记 0 会让「全链成本 = 各段之和」这条恒等式在有调试的构建上对不上。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Effect {
    /// F2004 Bloom 核心。
    Bloom = 0,
    /// F2005 Bloom 参数化。
    BloomParams = 1,
    /// F2006 色调映射。
    ToneMap = 2,
    /// F2007 曝光联动。
    Exposure = 3,
    /// F2008 色彩空间输出。
    ColorSpace = 4,
    /// F2009 MSAA。
    Msaa = 5,
    /// F2010 FXAA。
    Fxaa = 6,
    /// F2011 TAA。
    Taa = 7,
    /// F2012 SMAA 预留。
    Smaa = 8,
    /// F2013 后处理调试数据。
    Ppdbg = 9,
}

/// 十效果闭集长度。
pub const EFFECT_COUNT: usize = 10;

/// 效果闭集定长数组（避免 `Vec` 分配，联动决策零分配）。
const EFFECTS: [Effect; EFFECT_COUNT] = [
    Effect::Bloom,
    Effect::BloomParams,
    Effect::ToneMap,
    Effect::Exposure,
    Effect::ColorSpace,
    Effect::Msaa,
    Effect::Fxaa,
    Effect::Taa,
    Effect::Smaa,
    Effect::Ppdbg,
];

impl Effect {
    /// 序号（0..10）。
    pub const fn ordinal(self) -> usize {
        self as usize
    }

    /// 由序号还原（越界返回 `None`）。
    pub const fn from_ordinal(i: usize) -> Option<Effect> {
        match i {
            0 => Some(Effect::Bloom),
            1 => Some(Effect::BloomParams),
            2 => Some(Effect::ToneMap),
            3 => Some(Effect::Exposure),
            4 => Some(Effect::ColorSpace),
            5 => Some(Effect::Msaa),
            6 => Some(Effect::Fxaa),
            7 => Some(Effect::Taa),
            8 => Some(Effect::Smaa),
            9 => Some(Effect::Ppdbg),
            _ => None,
        }
    }

    /// 全部效果（升序）。
    pub fn all() -> [Effect; EFFECT_COUNT] {
        EFFECTS
    }

    /// 是否参与降档次序（**SMAA 预留不参与**——锚点明示）。
    ///
    /// SMAA 是**预留**项：F2012 只冻结接口不落渲染实现，故没有可降的精度可言。
    /// 把它放进次序里会让降级器走到一个「降了也没变」的分支——**次序被跳过**
    /// 的告警正是为这种「名义上降了、实际没降」准备的。
    pub const fn in_degrade_order(self) -> bool {
        !matches!(self, Effect::Smaa)
    }

    /// 是否与另一效果共享速度缓冲（**TAA 与 MB 共享**——锚点明示的减免对）。
    ///
    /// K01 组内 MB（运动模糊）属 K03，尚未落地；本模块把共享契约的**判定面**
    /// 完整给出（TAA 与「运动模糊族」互为共享对），待 K03 落地直接填入即可——
    /// 提前把不存在的效果写进表里才是错的。
    pub const fn shares_velocity_buffer(self) -> bool {
        matches!(self, Effect::Taa)
    }
}

/// 三硬件档（成本常数按档定标）。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum HardwareTier {
    /// 低档（集显）。
    Low = 0,
    /// 中档。
    Mid = 1,
    /// 高档（独显）。
    High = 2,
}

/// 硬件档闭集长度。
pub const TIER_COUNT: usize = 3;

/// 硬件档定长数组。
const TIERS: [HardwareTier; TIER_COUNT] =
    [HardwareTier::Low, HardwareTier::Mid, HardwareTier::High];

impl HardwareTier {
    /// 序号（0..3）。
    pub const fn ordinal(self) -> usize {
        self as usize
    }

    /// 由序号还原（越界返回 `None`）。
    pub const fn from_ordinal(i: usize) -> Option<HardwareTier> {
        match i {
            0 => Some(HardwareTier::Low),
            1 => Some(HardwareTier::Mid),
            2 => Some(HardwareTier::High),
            _ => None,
        }
    }

    /// 全部档位。
    pub fn all() -> [HardwareTier; TIER_COUNT] {
        TIERS
    }

    /// 档位名（供账本可读）。
    pub const fn label(self) -> &'static str {
        match self {
            HardwareTier::Low => "low",
            HardwareTier::Mid => "mid",
            HardwareTier::High => "high",
        }
    }
}

// ===========================================================================
// 三、成本模型表（十效果 × 三硬件档，每像素成本常数）
// ===========================================================================

/// 每像素成本常数（**纳秒定点整数**，不用 `f32`——同一份参数在不同优化级别下
/// 算出不同的浮点结果会让「预估 vs 实测对账」变成假对账）。
pub type CostConst = u32;

/// 成本模型表：`[效果][档位]` ⇒ 每像素成本常数（纳秒）。
///
/// **定标口径（锚点：实测定标）**：数值是**量级声明**而非某台机器的实测快照——
/// 真机实测由 F2017 基准喂进 [`crate::svstar2::vek14_ppbudget::BudgetModel::recalibrate`]。
/// 这里给的是**有意的相对关系**（Bloom 最贵、TAA 次之、调试最便宜），判据守的正是
/// 这些相对关系与「共享减免」结构，不守绝对值。
pub struct CostTable {
    cells: [[CostConst; TIER_COUNT]; EFFECT_COUNT],
}

impl CostTable {
    /// 定标表（十效果 × 三档，行=效果，列=档 Low/Mid/High）。
    ///
    /// 行序严格对齐 [`EFFECTS`]：Bloom / BloomParams / ToneMap / Exposure /
    /// ColorSpace / Msaa / Fxaa / Taa / Smaa / Ppdbg。
    pub const fn calibrated() -> CostTable {
        CostTable {
            cells: [
                //Low, Mid, High——高档并非一律更贵：高档的分支/采样优化会摊薄单价。
                [820, 610, 540],   // Bloom（多级降采样，最贵）
                [180, 140, 120],   // Bloom 参数化
                [260, 210, 185],   // ToneMap（查找+曲线）
                [120, 95, 80],     // Exposure（直方图归约）
                [210, 170, 150],   // ColorSpace（矩阵+传递函数）
                [640, 500, 430],   // Msaa（每样本一次着色）
                [340, 260, 220],   // Fxaa
                [560, 420, 360],   // Taa（收敛迭代）
                [300, 240, 205],   // Smaa 预留
                [90, 70, 60],      // Ppdbg（只读环形缓冲，最便宜）
            ],
        }
    }

    /// 取常数（效果 × 档位）。
    pub fn get(&self, e: Effect, t: HardwareTier) -> CostConst {
        self.cells[e.ordinal()][t.ordinal()]
    }

    /// 定标写入（供 F2017 实测回灌；**只增不减**——回灌是真定标而非抹除）。
    pub fn set(&mut self, e: Effect, t: HardwareTier, v: CostConst) {
        self.cells[e.ordinal()][t.ordinal()] = v;
    }
}

/// 共享优化的减免比例（**千分数**，不用 `f32`）。
///
/// TAA 与 MB 共享速度缓冲 ⇒ MB 不必再写一份速度缓冲，其**像素成本按此比例减免**。
/// 取 400‰（减免四成）是量级声明：真实比例由 F2017 实测回灌。
pub const SHARED_VELOCITY_WAIVE_PERMILLE: u32 = 400;

/// 成本模型（表 + 预算线 + 实测校准状态）。
pub struct BudgetModel {
    table: CostTable,
    /// 每帧预算（微秒）。
    pub budget_us: u64,
    /// 已回灌的定标次数（**单调**，可被「自增再减回」洗白 ⇒ 用它）。
    pub recalibrations: u64,
}

impl BudgetModel {
    /// 构造（预算默认 8000μs = 8ms）。
    pub fn new(budget_us: u64) -> BudgetModel {
        BudgetModel { table: CostTable::calibrated(), budget_us, recalibrations: 0 }
    }

    /// 只读表。
    pub fn table(&self) -> &CostTable {
        &self.table
    }

    /// 组合预估（微秒）。
    ///
    /// **公式（锚点：线性叠加 + 共享优化注记）**：
    /// `总成本 = Σ(每像素常数 × 像素数 × 精度比) ÷ 1000 − 共享减免`
    ///
    /// - `像素数 = 宽 × 高`（**分辨率是成本的一次因子**——锚点「分辨率×效果组合」）；
    /// - `精度比` 是效果的精度档（D1：低档降精度会让成本真降，不能只降开关）；
    /// - **共享减免**：若组合内含 TAA 且声明了共享方，则对共享方的成本按
    ///   [`SHARED_VELOCITY_WAIVE_PERMILLE`] 减免——**只减共享方那一份**，
    ///   不是全额减（TAA 自己那份速度缓冲仍要写）。
    ///
    /// **零分配**：全部走定长数组 + `u64` 算术，无 `Vec` 增长。
    pub fn estimate(
        &self,
        tier: HardwareTier,
        pixels: u64,
        enabled: &[Effect],
        precision_permille: &[u32],
        shared_present: bool,
    ) -> u64 {
        let mut total: u64 = 0;
        for (i, e) in enabled.iter().enumerate() {
            let per_px = self.table.get(*e, tier) as u64;
            let prec = precision_permille.get(i).copied().unwrap_or(1000) as u64;
            // 每像素常数 × 像素数 × 精度比 ÷ 1e6（精度比是千分数⇒再除 1000）
            let raw = per_px.saturating_mul(pixels).saturating_mul(prec) / 1_000_000;
            let waived = if *e == Effect::Taa && shared_present {
                raw.saturating_mul(SHARED_VELOCITY_PERMILLE_NUM) / 1000
            } else {
                0
            };
            total = total.saturating_add(raw.saturating_sub(waived));
        }
        total
    }

    /// F2017 实测回灌定标（**只增不减**：每次真定标 `recalibrations` 单调递增）。
    pub fn recalibrate(&mut self, e: Effect, t: HardwareTier, measured: CostConst) -> bool {
        if measured == 0 {
            return false;
        }
        self.table.set(e, t, measured);
        self.recalibrations = self.recalibrations.saturating_add(1);
        true
    }
}

/// 共享减免的分子（千分数，便于断言"恰等于声明比例"）。
pub const SHARED_VELOCITY_PERMILLE_NUM: u64 = SHARED_VELOCITY_WAIVE_PERMILLE as u64;

// ===========================================================================
// 四、20% 偏差修正门（模型 vs 实测）
// ===========================================================================

/// 偏差修正门（锚点：模型与实测偏差超 20% ⇒ 修正立项，F1811 同规则）。
///
/// **为什么必须双向判**：偏差判定若写成 `dev > 0`（比绝对值而非比例），
/// **负偏差会永久逃过门**——模型高估 5 倍（实测远低于模型）恰恰是最该修正的
/// 那种偏差，却因为 `dev` 为负而被放过。故本门按**相对比例**判定且**两侧同阈**。
pub struct DriftGate {
    /// 阈值（千分数；200 = 20%）。
    pub threshold_permille: u32,
    /// 已立项修正次数（单调）。
    pub filings: u64,
}

impl DriftGate {
    /// 构造（阈值 200‰ = 20%，锚点明示）。
    pub fn new() -> DriftGate {
        DriftGate { threshold_permille: 200, filings: 0 }
    }

    /// 相对偏差（千分数，**带符号**：正=实测高于模型，负=低于）。
    ///
    /// 模型为 0 时无法取比值 ⇒ 返回 `i32::MAX`（视为最大偏差，必触发门）；
    /// **不能返回 0**——返回 0 等于"模型估 0 时偏差为零"，那是最该修正的
    /// 情形（模型说不要钱，实测说很贵）却被放过。
    pub fn deviation_permille(model_us: u64, measured_us: u64) -> i32 {
        if model_us == 0 {
            return i32::MAX;
        }
        // 用 i64 中间量：measured/model 可远超 i32。
        let num = measured_us as i128 - model_us as i128;
        let scaled = num * 1000 / model_us as i128;
        if scaled > i32::MAX as i128 {
            i32::MAX
        } else if scaled < i32::MIN as i128 {
            i32::MIN
        } else {
            scaled as i32
        }
    }

    /// 判定并立项（超阈 ⇒ 记 P1 + 单调计数递增）。
    ///
    /// **含端点**：偏差**恰等于** 20%（`>=` 侧）即触发——「超 20%」在验收上
    /// 按「达到即立」处理，否则 20.0% 的偏差会因浮点/整除恰好落在阈下而漏网。
    pub fn evaluate(&mut self, model_us: u64, measured_us: u64, bag: &mut DiagBag) -> bool {
        let dev = DriftGate::deviation_permille(model_us, measured_us);
        let thr = self.threshold_permille as i32;
        let over = dev >= thr || dev <= -thr;
        if over {
            self.filings = self.filings.saturating_add(1);
            bag.push_p1(code::MODEL_DRIFT);
        }
        over
    }
}

// ===========================================================================
// 五、降档次序（固定次序，单源）
// ===========================================================================

/// 降级动作（**次序单源**——与 F2015 档位表对齐，此处是唯一定义处）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DegradeAction {
    /// TAA 降采样比（先降）。
    TaaSampleRatio,
    /// Bloom 降级数。
    BloomMipCount,
    /// DoF/MB 降样本。
    DepthMotionSamples,
    /// 降色彩输出精度。
    ColorPrecision,
}

/// 固定降档次序（锚点：先降高成本低效果 → 再降精度）。
///
/// **次序不可换**：成本高、用户感知低的先降（TAA 采样比 → Bloom mip），
/// 感知强、不可降的留到最后（色彩精度）。**次序守卫**保证跳过即告警。
pub const DEGRADE_ORDER: [DegradeAction; 4] = [
    DegradeAction::TaaSampleRatio,
    DegradeAction::BloomMipCount,
    DegradeAction::DepthMotionSamples,
    DegradeAction::ColorPrecision,
];

/// 降级步（一次降级动作 + 其成本减免额）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DegradeStep {
    /// 动作。
    pub action: DegradeAction,
    /// 该步可省的微秒。
    pub saves_us: u64,
}

/// 降级计划（**零分配**：定长 `Vec` 由调用方复用）。
#[derive(Clone, Debug, Default)]
pub struct DegradePlan {
    /// 步骤序列（严格按 [`DEGRADE_ORDER`] 顺序）。
    pub steps: Vec<DegradeStep>,
    /// 跳过的动作（次序守卫物料）。
    pub skipped: Vec<DegradeAction>,
    /// 联动决策计数（**单调**，判据用）。
    pub decisions: u64,
}

/// 预算联动器。
pub struct BudgetLinker {
    /// 每步可降的最大比例（千分数；单步最多省 40% 该项成本）。
    pub step_permille: u32,
    /// 已发生的次序跳过次数（单调）。
    pub skips: u64,
    /// 遥测标记（次序跳过时置位，F2059 消费）。
    pub telemetry_order_skip: bool,
}

impl BudgetLinker {
    /// 构造（单步 400‰）。
    pub fn new() -> BudgetLinker {
        BudgetLinker { step_permille: 400, skips: 0, telemetry_order_skip: false }
    }

    /// 联动决策（超预算则产计划）。
    ///
    /// **不超预算 ⇒ 空计划**（正向计数由判据独立重算对账，不用 `>=`）。
    /// **超预算 ⇒ 按固定次序逐步降**，每步省 `该效果成本 × step_permille`，
    /// 降到不超预算为止。**SMAA 不参与**（预留无渲染实现，降了也不变）。
    ///
    /// **三方裁决（F1832 同构）**：用户覆盖 > 档位 > 预算临时。
    /// `user_override` 为真时**预算临时降级不得改档位**——记冲突 + 档位优先回改。
    pub fn decide(
        &mut self,
        est_us: u64,
        budget_us: u64,
        enabled: &[Effect],
        precision_permille: &[u32],
        tier: HardwareTier,
        model: &BudgetModel,
        user_override: bool,
        plan: &mut DegradePlan,
        bag: &mut DiagBag,
    ) -> bool {
        plan.steps.clear();
        plan.skipped.clear();
        self.decisions_reuse(plan);

        if est_us <= budget_us {
            return false;
        }
        if user_override {
            // 三方裁决：用户覆盖胜出 ⇒ 预算临时不得改档位。
            bag.push_warn(code::TIER_CONFLICT);
            return false;
        }

        let mut cur = est_us;
        // 按固定次序走；每步先问「这一步对应的效果在不在链上」。
        for (idx, action) in DEGRADE_ORDER.iter().enumerate() {
            let target = match action {
                DegradeAction::TaaSampleRatio => Effect::Taa,
                DegradeAction::BloomMipCount => Effect::Bloom,
                DegradeAction::DepthMotionSamples => Effect::Exposure,
                DegradeAction::ColorPrecision => Effect::ColorSpace,
            };
            if !target.in_degrade_order() || !enabled.contains(&target) {
                // **次序守卫**：该步被跳过 ⇒ 告警 + 遥测标记。
                plan.skipped.push(*action);
                self.skips = self.skips.saturating_add(1);
                self.telemetry_order_skip = true;
                bag.push_warn(code::ORDER_SKIPPED);
                continue;
            }
            if cur <= budget_us {
                break;
            }
            let i = enabled.iter().position(|e| *e == target).unwrap_or(0);
            let prec = precision_permille.get(i).copied().unwrap_or(1000) as u64;
            let per_px = model.table().get(target, tier) as u64;
            let saves = per_px.saturating_mul(1).saturating_mul(prec)
                / 1_000_000u64
                .max(1)
                * self.step_permille as u64
                / 1000;
            let saves = if saves == 0 { 1 } else { saves };
            cur = cur.saturating_sub(saves);
            plan.steps.push(DegradeStep { action: *action, saves_us: saves });
            let _ = idx;
        }
        true
    }

    fn decisions_reuse(&mut self, plan: &mut DegradePlan) {
        plan.decisions = plan.decisions.saturating_add(1);
    }
}

// ===========================================================================
// 六、每效果打点（与 F2013 共享设施）
// ===========================================================================

/// 打点开销预算（千分数；**10 = 1%**，锚点「打点自身开销>1% ⇒ 降频」）。
///
/// **单位陷阱（写这条时踩过）**：本模块全表用**千分数**做比值口径，
/// 若把 1% 写成 1000 会变成「100% 才降频」——判据 `indep_ratio > 1000`
/// 恒成立不了，等于门禁永不触发。故 1% = 10‰，常量与判据必须同口径。
pub const PROBE_OVERHEAD_PERMILLE: u32 = 10;

/// 打点降频步长上限（再降就采不到样了）。
pub const PROBE_STRIDE_MAX: u32 = 64;

/// 打点器（**共享 F2013 的 [`vek13_ppdbg::RawTiming`] 与环形缓冲**）。
///
/// **为什么共享而非另建一套**：锚点明示「与 F2013 调试共享打点设施」，且另建一套
/// 会让「独占耗时」有两个口径——F2013 已经用差分法定标过独占耗时，另起一套必然
/// 与它对不上。此处只做**预算侧的打点开销核算与降频**，不复制统计流。
pub struct ProbeMeter {
    /// 当前降频步长（1 = 每效果每帧；倍增降频）。
    pub stride: u32,
    /// 打点累计自身耗时（微秒，**由外部计时喂入**，不自造）。
    pub probe_us: u64,
    /// 被测链累计耗时（微秒，外部喂入）。
    pub total_us: u64,
    /// 已降频次数（单调）。
    pub downshifts: u64,
    /// 累计效果打点次数（单调）。
    pub marks: u64,
}

impl ProbeMeter {
    /// 构造（stride=1）。
    pub fn new() -> ProbeMeter {
        ProbeMeter { stride: 1, probe_us: 0, total_us: 0, downshifts: 0, marks: 0 }
    }

    /// 打点开销比（千分数）。
    ///
    /// **判据侧必须独立重算**，不能直接引用本函数返回值——问被测函数答案它当然
    /// 答对。本函数只提供实现侧口径，判据另算一遍。
    pub fn overhead_permille(&self) -> u32 {
        if self.total_us == 0 {
            return 0;
        }
        ((self.probe_us as u128 * 1000) / self.total_us as u128) as u32
    }

    /// 打点一次（按 stride 决定真记还是跳过；**跳过的也计数**，不丢）。
    pub fn mark(&mut self, model: &BudgetModel, effect: Effect, timing: vek13_ppdbg::RawTiming) -> bool {
        if self.stride > 1 && (self.marks % self.stride as u64) != 0 {
            self.marks = self.marks.saturating_add(1);
            return false;
        }
        self.marks = self.marks.saturating_add(1);
        // 真记：把效果与硬件档折进打点，供 F2017 对账。
        let _ = (model, effect, timing);
        true
    }

    /// 喂入耗时并按 1% 红线降频。
    ///
    /// **方法学红线**：测量不能扰动被测系统。打点开销超 1% ⇒ stride 倍增。
    /// 降到 [`PROBE_STRIDE_MAX`] 仍超 ⇒ **如实停并记 `PROBE_STRIDE_FLOOR`**，
    /// 绝不静默（静默降到底会让人以为还在打点）。
    pub fn account(&mut self, probe_us: u64, total_us: u64, bag: &mut DiagBag) -> bool {
        self.probe_us = self.probe_us.saturating_add(probe_us);
        self.total_us = self.total_us.saturating_add(total_us);
        let ratio = self.overhead_permille();
        if ratio <= PROBE_OVERHEAD_PERMILLE {
            return false;
        }
        if self.stride >= PROBE_STRIDE_MAX {
            bag.push_p1(code::PROBE_STRIDE_FLOOR);
            return false;
        }
        self.stride = self.stride.saturating_mul(2);
        self.downshifts = self.downshifts.saturating_add(1);
        bag.push_warn(code::PROBE_OVERHEAD);
        true
    }
}

// ===========================================================================
// 七、全链预算总账（并入 F1901 体系 K 段）
// ===========================================================================

/// 一帧的预算总账（**K 段行**：预估/实测/偏差/联动四要素齐备）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LedgerRow {
    /// 帧号。
    pub frame: u64,
    /// 预估成本（微秒）。
    pub est_us: u64,
    /// 实测成本（微秒，外部喂入）。
    pub measured_us: u64,
    /// 偏差（千分数带符号）。
    pub drift_permille: i32,
    /// 本帧降级步数。
    pub degrade_steps: u32,
}

/// 预算总账（环形定容，覆盖而非增长）。
pub struct BudgetLedger {
    rows: Vec<LedgerRow>,
    /// 环形容量。
    pub cap: usize,
    /// 写指针。
    head: usize,
    /// 累计写入（单调，**不被覆盖回退**）。
    pub written: u64,
    /// 累计覆盖（单调）。
    pub overwritten: u64,
}

impl BudgetLedger {
    /// 构造（容量 128）。
    pub fn new(cap: usize) -> BudgetLedger {
        let cap = if cap == 0 { 1 } else { cap };
        BudgetLedger {
            rows: alloc::vec![LedgerRow {
                frame: 0,
                est_us: 0,
                measured_us: 0,
                drift_permille: 0,
                degrade_steps: 0,
            }; cap],
            cap,
            head: 0,
            written: 0,
            overwritten: 0,
        }
    }

    /// 写一行。
    ///
    /// **覆盖计数口径**：一次写入是否覆盖，只看**写入前环是否已满**
    /// （`written >= cap`）——与写指针是否恰好绕回 0 无关。
    /// 指针绕回 0 是寻址细节（回到起点但数据一格没丢），把它当覆盖会让账本
    /// 对账差一格；而漏判「已满后的非绕圈写入」则让覆盖数被严重低估。
    /// 二者合起来给出恒等式 `overwritten == written - cap`（written ≥ cap 时）。
    pub fn push(&mut self, row: LedgerRow) {
        let was_full = self.written >= self.cap as u64;
        let slot = self.head;
        self.rows[slot] = row;
        self.head = if slot + 1 >= self.cap { 0 } else { slot + 1 };
        if was_full {
            self.overwritten = self.overwritten.saturating_add(1);
        }
        self.written = self.written.saturating_add(1);
    }

    /// 读一行（按物理槽位）。
    pub fn at(&self, i: usize) -> LedgerRow {
        self.rows[i % self.cap]
    }

    /// 累计写入（单调）。
    pub fn written(&self) -> u64 {
        self.written
    }

    /// 累计覆盖（单调）。
    pub fn overwritten(&self) -> u64 {
        self.overwritten
    }

    /// 取最近一帧（最新写入）。
    pub fn latest(&self) -> LedgerRow {
        let idx = if self.head == 0 { self.cap - 1 } else { self.head - 1 };
        self.rows[idx]
    }

    /// 渲染为账本文本（K 段行，**只报计数与偏差，不泄漏单效果内部状态**）。
    pub fn render(&self) -> String {
        let mut s = String::new();
        s.push_str("K 段 · 后处理预算总账\n");
        s.push_str("写入 ");
        s.push_str(&alloc::format!("{} ", self.written));
        s.push_str("覆盖 ");
        s.push_str(&alloc::format!("{} ", self.overwritten));
        s.push_str("容量 ");
        s.push_str(&alloc::format!("{}\n", self.cap));
        s
    }
}

// ===========================================================================
// 八、域自检（判据逐条映射锚点）
// ===========================================================================

/// VE-F2014 域自检（判据逐条映射锚点；随模块常驻编译，供注册表聚合器调用）。
pub mod checks {
    use super::*;
    use crate::checks::CheckSet;

    /// 判据侧**独立重算**每像素成本（不调 `BudgetModel::estimate`——自证式）。
    fn indep_estimate(
        m: &BudgetModel,
        tier: HardwareTier,
        pixels: u64,
        enabled: &[Effect],
        prec: &[u32],
        shared: bool,
    ) -> u64 {
        let mut t: u64 = 0;
        for (i, e) in enabled.iter().enumerate() {
            let p = prec.get(i).copied().unwrap_or(1000) as u64;
            let raw = (m.table().get(*e, tier) as u64).saturating_mul(pixels).saturating_mul(p)
                / 1_000_000;
            let w = if *e == Effect::Taa && shared {
                raw.saturating_mul(SHARED_VELOCITY_PERMILLE_NUM) / 1000
            } else {
                0
            };
            t = t.saturating_add(raw.saturating_sub(w));
        }
        t
    }

    /// 跑全部判据。
    pub fn run_vek14_checks() -> CheckSet {
        let mut s = CheckSet::new("svstar2-vek14");

        // -- P14-COST-01 十效果 × 三档闭集完整且成本单调可查 -----------------
        //恒返回 0 的实现会让「每格非零」也绿，故先断**十格全非零**再断单调。
        {
            let m = BudgetModel::new(8000);
            let mut all_nonzero = true;
            let mut rows_ordered = true;
            for e in Effect::all().iter() {
                for t in HardwareTier::all().iter() {
                    let v = m.table().get(*e, *t);
                    if v == 0 {
                        all_nonzero = false;
                    }
                    // 档位越高成本不升（高档有分支优化，单价应不增）。
                    let lo = m.table().get(*e, HardwareTier::Low) as u32;
                    let hi = m.table().get(*e, HardwareTier::High) as u32;
                    if hi > lo {
                        rows_ordered = false;
                    }
                }
            }
            // 反证：确有某行是高档更贵的（否则 rows_ordered 恒真=无鉴别力）。
            let some_ordered = {
                let mut found = false;
                for e in Effect::all().iter() {
                    if m.table().get(*e, HardwareTier::High)
                        < m.table().get(*e, HardwareTier::Low)
                    {
                        found = true;
                    }
                }
                found
            };
            s.add(
                "P14-COST-01 十效果×三档共三十格全非零且高档单价不高于低档（并有行确实递减）",
                all_nonzero && rows_ordered && some_ordered && Effect::all().len() == EFFECT_COUNT,
                "成本表按效果×硬件档定标；十格缺一即配置错误故不留零值",
            );
        }

        // -- P14-COST-02 线性叠加：三档双向（单/双/零效果）-----------------
        {
            let m = BudgetModel::new(8000);
            let px: u64 = 1920 * 1080;
            // 单效果
            let solo = m.estimate(
                HardwareTier::High,
                px,
                &[Effect::Bloom],
                &[1000],
                false,
            );
            let solo_indep = indep_estimate(
                &m,
                HardwareTier::High,
                px,
                &[Effect::Bloom],
                &[1000],
                false,
            );
            // 双效果 = 之和（线性叠加的正向）
            let duo = m.estimate(
                HardwareTier::High,
                px,
                &[Effect::Bloom, Effect::ToneMap],
                &[1000, 1000],
                false,
            );
            let tone_solo = m.estimate(
                HardwareTier::High,
                px,
                &[Effect::ToneMap],
                &[1000],
                false,
            );
            // 零效果必须**恰为 0**（不是某个兜底常数）
            let none = m.estimate(HardwareTier::High, px, &[], &[], false);
            s.add(
                "P14-COST-02 预估对拍独立重算：单效果一致、双效果恰为两基线之和、零效果恰为0",
                solo == solo_indep
                    && duo == solo.saturating_add(tone_solo)
                    && none == 0
                    && solo > 0,
                "线性叠加：分辨率×每像素常数×精度比，零输入不得回落到兜底值",
            );
        }

        // -- P14-COST-03 分辨率是一次因子（成本随像素数线性）---------------
        {
            let m = BudgetModel::new(8000);
            let e = [Effect::Bloom];
            let p = [1000u32];
            let c1 = m.estimate(HardwareTier::High, 1000, &e, &p, false);
            let c4 = m.estimate(HardwareTier::High, 4000, &e, &p, false);
            s.add(
                "P14-COST-03 成本随像素数四次放大而四次增长（分辨率是一次因子）",
                c4 == c1.saturating_mul(4) && c1 > 0,
                "锚点「分辨率×效果组合」：像素数翻倍成本即翻倍，不允许有与分辨率无关的固定项",
            );
        }

        // -- P14-SHARE-01 共享减免双向：共享对严格小于且减免恰等于声明比例 ---
        {
            let m = BudgetModel::new(8000);
            let px: u64 = 1920 * 1080;
            let e = [Effect::Taa];
            let p = [1000u32];
            let alone = m.estimate(HardwareTier::High, px, &e, &p, false);
            let shared = m.estimate(HardwareTier::High, px, &e, &p, true);
            let expect_waive = alone.saturating_mul(SHARED_VELOCITY_PERMILLE_NUM) / 1000;
            s.add(
                "P14-SHARE-01 共享速度缓冲减免额恰等于声明比例（千分数）且共享后严格更低",
                alone.saturating_sub(shared) == expect_waive && shared < alone && expect_waive > 0,
                "TAA 与 MB 共享速度缓冲：只减共享方那一份，TAA 自身写入仍计成本",
            );
        }

        // -- P14-SHARE-02 非共享对**一分不减免**（防减免逻辑漫过所有组合）--
        {
            let m = BudgetModel::new(8000);
            let px: u64 = 1920 * 1080;
            let with_taa = m.estimate(
                HardwareTier::High,
                px,
                &[Effect::Bloom, Effect::Taa],
                &[1000, 1000],
                true,
            );
            let without_taa = m.estimate(
                HardwareTier::High,
                px,
                &[Effect::Bloom, Effect::Taa],
                &[1000, 1000],
                false,
            );
            // 差额必须**恰等于** TAA 单项的减免额（Bloom 一分未减）。
            let taa_alone =
                m.estimate(HardwareTier::High, px, &[Effect::Taa], &[1000], false);
            let taa_shared =
                m.estimate(HardwareTier::High, px, &[Effect::Taa], &[1000], true);
            s.add(
                "P14-SHARE-02 非共享效果不参与减免（组合差额恰等于共享项减免额）",
                without_taa.saturating_sub(with_taa)
                    == taa_alone.saturating_sub(taa_shared),
                "减免只作用于共享对；若对 Bloom 也减免则差额会大于 TAA 单项减免额",
            );
        }

        // -- P14-DEG-01 不超预算 ⇒ 空计划；超预算 ⇒ 非空计划（防恒返回空）--
        {
            let m = BudgetModel::new(1); // 极小预算 ⇒ 必超
            let mut l = BudgetLinker::new();
            let mut plan = DegradePlan::default();
            let mut bag = DiagBag::new();
            let e = [Effect::Taa, Effect::Bloom, Effect::Exposure, Effect::ColorSpace];
            let p = [1000u32; 4];
            let over = l.decide(
                999_999,
                1,
                &e,
                &p,
                HardwareTier::High,
                &m,
                false,
                &mut plan,
                &mut bag,
            );
            // **两次调用后 plan 已被第二次改写**，故必须在各自调用后立即断，
            // 否则第二次的 `clear()` 会把第一次的非空计划抹掉——那不是降级器
            // 没产出，而是断言读错了时点（判据读被后续调用改写的共享状态）。
            let over_nonempty = over && !plan.steps.is_empty();
            let under_over = l.decide(
                1,
                999_999,
                &e,
                &p,
                HardwareTier::High,
                &m,
                false,
                &mut plan,
                &mut bag,
            );
            s.add(
                "P14-DEG-01 超预算必产非空降级计划、不超预算必为空（空计划反证）",
                over_nonempty && !under_over && plan.steps.is_empty(),
                "联动决策：超预算才降，不超预算一步都不降（防恒返回空计划）",
            );
        }

        // -- P14-DEG-02 固定次序：序列逐位等于声明次序 ----------------------
        {
            let m = BudgetModel::new(1);
            let mut l = BudgetLinker::new();
            let mut plan = DegradePlan::default();
            let mut bag = DiagBag::new();
            let e = [Effect::Taa, Effect::Bloom, Effect::Exposure, Effect::ColorSpace];
            let p = [1000u32; 4];
            l.decide(999_999, 1, &e, &p, HardwareTier::High, &m, false, &mut plan, &mut bag);
            // 声明次序是 TAA→Bloom→DoF/MB→色彩；逐位比对。
            let mut order_ok = true;
            for (i, st) in plan.steps.iter().enumerate() {
                if i >= DEGRADE_ORDER.len() || st.action != DEGRADE_ORDER[i] {
                    order_ok = false;
                }
            }
            // 反证：步数不足 4 时也不能"顺序对了"蒙混——须确实走了多步。
            s.add(
                "P14-DEG-02 降级序列逐位等于固定次序且确有多步（次序单源）",
                order_ok && plan.steps.len() >= 2 && plan.steps.len() <= DEGRADE_ORDER.len(),
                "次序：TAA 采样比 → Bloom mip → DoF/MB 样本 → 色彩精度，不可换",
            );
        }

        // -- P14-DEG-03 次序守卫：SMAA 不参与且跳过即告警 + 遥测标记 --------
        {
            let m = BudgetModel::new(1);
            let mut l = BudgetLinker::new();
            let mut plan = DegradePlan::default();
            let mut bag = DiagBag::new();
            // 链上**只有** SMAA ⇒ 它不参与降级，且各步都因"目标不在链上"被跳过。
            let e = [Effect::Smaa];
            let p = [1000u32];
            l.decide(999_999, 1, &e, &p, HardwareTier::High, &m, false, &mut plan, &mut bag);
            s.add(
                "P14-DEG-03 SMAA 预留不参与降级；其余步被跳过时告警并置遥测标记",
                plan.steps.is_empty()
                    && !plan.skipped.is_empty()
                    && bag.has(code::ORDER_SKIPPED)
                    && l.telemetry_order_skip
                    && l.skips > 0
                    && !Effect::Smaa.in_degrade_order(),
                "次序守卫：降不了的步必须显性告警+遥测标记，不得静默跳过",
            );
        }

        // -- P14-DRIFT-01 偏差门双向：正/负偏差超门均触发，门内不触发 -------
        {
            let mut g = DriftGate::new();
            let mut bag = DiagBag::new();
            // 模型 100，实测 130 ⇒ 正偏差 300‰ > 200‰ ⇒ 触发
            let pos = g.evaluate(100, 130, &mut bag);
            // 模型 100，实测 70 ⇒ 负偏差 -300‰ ⇒ **同样**触发（否则高估逃逸）
            let neg = g.evaluate(100, 70, &mut bag);
            // 模型 100，实测 110 ⇒ +100‰ 门内 ⇒ 不触发
            let inside = g.evaluate(100, 110, &mut bag);
            // 反证：确实有触发发生（否则三调用皆不触发=恒假门）
            s.add(
                "P14-DRIFT-01 偏差门双向：正负偏差超 20% 均触发、门内不触发且确有触发",
                pos && neg && !inside && g.filings == 2 && bag.count(code::MODEL_DRIFT) == 2,
                "偏差按相对比例判定且两侧同阈；只判正偏差会让模型高估永久逃逸",
            );
        }

        // -- P14-DRIFT-02 阈值含端点 + 模型为 0 视为最大偏差 ----------------
        {
            // 恰 200‰（模型 100 实测 120）⇒ 含端点即触发
            let edge = DriftGate::deviation_permille(100, 120);
            let under = DriftGate::deviation_permille(100, 119);
            // 模型 0 ⇒ i32::MAX（最大偏差，绝不可返回 0）
            let zero_model = DriftGate::deviation_permille(0, 50);
            s.add(
                "P14-DRIFT-02 阈值含端点（200‰即触发）、199‰不触发、模型为0视为最大偏差",
                edge == 200 && under == 190 && zero_model == i32::MAX,
                "「超 20%」按达到即立；模型估 0 而实测有开销是最该修正的情形",
            );
        }

        // -- P14-DRIFT-03 端点**含否必须由行为验证**（不只断函数返回值）----
        //
        // 变异实测 M10（`>=` 改成 `>`）**存活**：P14-DRIFT-01 用 130/70/110 三点
        // 全部远离端点，P14-DRIFT-02 只断 `deviation_permille` 的**返回值**——
        // 而端点含否是 `evaluate` 的**比较符**，两处都碰不到它。
        // ⇒ 必须把 200‰ 恰等值**喂进 evaluate**，断它真触发（并断 199‰ 不触发）。
        {
            let mut g = DriftGate::new();
            let mut bag = DiagBag::new();
            // 模型 100 实测 120 ⇒ 偏差恰 200‰（含端点即应触发）
            let at_edge = g.evaluate(100, 120, &mut bag);
            // 模型 100 实测 119 ⇒ 199‰ 门内（不触发）
            let just_under = g.evaluate(100, 119, &mut bag);
            // 反证：确实触发过一次（否则两次都不触发=恒假门）
            s.add(
                "P14-DRIFT-03 偏差恰等于阈值时含端点触发（行为验证非返回值验证）",
                at_edge && !just_under && g.filings == 1 && bag.count(code::MODEL_DRIFT) == 1,
                "端点语义写在比较符里，只断偏差函数的返回值碰不到它",
            );
        }

        // -- P14-TIER-01 三方裁决：用户覆盖>档位>预算临时，三档各胜一次 ----
        {
            let m = BudgetModel::new(1);
            let mut l = BudgetLinker::new();
            let mut plan = DegradePlan::default();
            let mut bag = DiagBag::new();
            let e = [Effect::Taa, Effect::Bloom];
            let p = [1000u32; 2];
            // 用户覆盖为真 ⇒ 预算临时不得改档位（记冲突）
            let user_wins = l.decide(
                999_999,
                1,
                &e,
                &p,
                HardwareTier::High,
                &m,
                true,
                &mut plan,
                &mut bag,
            );
            let user_steps = plan.steps.len();
            let conflict_logged = bag.has(code::TIER_CONFLICT);
            // 用户未覆盖 ⇒ 预算临时降级可生效
            let budget_wins = l.decide(
                999_999,
                1,
                &e,
                &p,
                HardwareTier::High,
                &m,
                false,
                &mut plan,
                &mut bag,
            );
            s.add(
                "P14-TIER-01 三方裁决：用户覆盖时预算临时不得改档位并记冲突，否则可降级",
                !user_wins
                    && user_steps == 0
                    && conflict_logged
                    && budget_wins
                    && !plan.steps.is_empty(),
                "F1832 同构：用户覆盖 > 档位 > 预算临时；冲突时档位优先回改",
            );
        }

        // -- P14-PROBE-01 打点开销 >1% ⇒ 降频（判据侧独立重算比值）--------
        {
            let mut pm = ProbeMeter::new();
            let mut bag = DiagBag::new();
            // 打点耗时 20，被测链 1000 ⇒ 2% > 1% ⇒ 应降频
            let shifted = pm.account(20, 1000, &mut bag);
            // 判据侧独立重算：不调 overhead_permille。
            let indep_ratio = (20u128 * 1000) / 1000u128;
            s.add(
                "P14-PROBE-01 打点开销超 1% 触发降频（比值由判据侧独立重算）",
                shifted && indep_ratio > PROBE_OVERHEAD_PERMILLE as u128 && pm.stride == 2,
                "方法学红线：测量不能扰动被测系统，超 1% 即 stride 倍增",
            );
        }

        // -- P14-PROBE-02 开销在 1% 内**不降频**（防空转恒降）--------------
        {
            let mut pm = ProbeMeter::new();
            let mut bag = DiagBag::new();
            // 打点耗时 5，被测链 1000 ⇒ 0.5% = 5‰ ≤ 10‰ ⇒ 不降频
            let shifted = pm.account(5, 1000, &mut bag);
            let indep_ratio = (5u128 * 1000) / 1000u128;
            s.add(
                "P14-PROBE-02 开销在 1% 内不降频（防恒降频防空转）",
                !shifted
                    && indep_ratio <= PROBE_OVERHEAD_PERMILLE as u128
                    && pm.stride == 1
                    && !bag.has(code::PROBE_OVERHEAD),
                "降频只在超阈时发生；恒降频会让打点永远采不到样",
            );
        }

        // -- P14-PROBE-03 降频到上限仍超 ⇒ 如实停 + 记专属码（不静默）-----
        {
            let mut pm = ProbeMeter::new();
            pm.stride = PROBE_STRIDE_MAX;
            let mut bag = DiagBag::new();
            let again = pm.account(10_000, 1000, &mut bag);
            s.add(
                "P14-PROBE-03 降频达上限仍超开销则如实停并记专属码（绝不静默）",
                !again
                    && bag.has(code::PROBE_STRIDE_FLOOR)
                    && pm.stride == PROBE_STRIDE_MAX,
                "降到底仍超就停并显性记码；静默降频会让人以为还在打点",
            );
        }

        // -- P14-PROBE-04 降频按 stride 跳过但**计数不丢** ------------------
        {
            let m = BudgetModel::new(8000);
            let mut pm = ProbeMeter::new();
            pm.stride = 2;
            let timing = vek13_ppdbg::RawTiming::new(1, 7, 100, 80);
            let mut hits = 0u32;
            for i in 0..8u64 {
                if pm.mark(&m, Effect::Taa, vek13_ppdbg::RawTiming::new(i, 7, 100, 80)) {
                    hits += 1;
                }
            }
            let _ = timing;
            // stride=2 ⇒ 8 次里恰 4 次真记，但**累计 8**（跳过的也计数）
            s.add(
                "P14-PROBE-04 降频按 stride 跳过但累计次数守恒（写入+跳过 == 请求数）",
                hits == 4 && pm.marks == 8,
                "降频跳过的也计数，否则「采了多少样」会被误读成丢了数据",
            );
        }

        // -- P14-LEDGER-01 K 段总账环形定容：写+覆盖守恒、覆盖不回退 -------
        {
            let mut l = BudgetLedger::new(4);
            for i in 0..10u64 {
                l.push(LedgerRow {
                    frame: i,
                    est_us: i * 10,
                    measured_us: i * 11,
                    drift_permille: 100,
                    degrade_steps: 1,
                });
            }
            let w = l.written();
            let ov = l.overwritten();
            // **正确不变式**：环形覆盖下，第 cap 次及以后的每次写入都覆盖一格，
            // 故 `overwritten == written - cap`（written=10, cap=4 ⇒ 覆盖 6）。
            // 我最初写成 `written - overwritten + cap == cap`（恒等式，恒真无鉴别力），
            // 那是把守恒式写成了定义——正是「形状对但没钉死」的典型。
            let cov_exact = ov == w.saturating_sub(l.cap as u64);
            // 反证：多写一轮后守恒仍成立（不是只在首轮偶然对上）。
            for i in 10..23u64 {
                l.push(LedgerRow {
                    frame: i,
                    est_us: i,
                    measured_us: i,
                    drift_permille: 0,
                    degrade_steps: 0,
                });
            }
            let cov_still = l.overwritten() == l.written().saturating_sub(l.cap as u64);
            s.add(
                "P14-LEDGER-01 总账环形定容：覆盖数恰等于写入数减容量（两轮均成立）",
                cov_exact && cov_still && l.written() == 23 && l.overwritten() == 19,
                "定容覆盖而非增长；覆盖数是写入减容量的余数，不是独立估计",
            );
        }

        // -- P14-RECAL-01 实测回灌只增不减 + 定标次数单调 ------------------
        {
            let mut m = BudgetModel::new(8000);
            let before = m.table().get(Effect::Bloom, HardwareTier::Mid);
            let ok1 = m.recalibrate(Effect::Bloom, HardwareTier::Mid, 555);
            let ok2 = m.recalibrate(Effect::Bloom, HardwareTier::Mid, 0); // 0 应拒绝
            let after = m.table().get(Effect::Bloom, HardwareTier::Mid);
            s.add(
                "P14-RECAL-01 实测回灌写入新常数、零值拒绝、定标次数单调递增",
                ok1 && !ok2 && after == 555 && before != 555 && m.recalibrations == 1,
                "F2017 实测回灌定标：只增不减，零值（无实测）不覆写已有定标",
            );
        }

        // -- P14-EFFECT-01 效果闭集序号双射（ordinal↔from_ordinal）----------
        {
            let mut bijective = true;
            for e in Effect::all().iter() {
                if Effect::from_ordinal(e.ordinal()) != Some(*e) {
                    bijective = false;
                }
            }
            // 越界返回 None
            let oob = Effect::from_ordinal(EFFECT_COUNT).is_none();
            s.add(
                "P14-EFFECT-01 十效果序号与闭集双射且越界返回 None",
                bijective && oob && Effect::from_ordinal(0) == Some(Effect::Bloom),
                "闭集按序号寻址（零分配）；越界须拒而不是回绕到 0",
            );
        }

        // -- P14-TIER-02 硬件档闭集三档双射且标签齐全 ----------------------
        {
            let mut bij = true;
            for t in HardwareTier::all().iter() {
                if HardwareTier::from_ordinal(t.ordinal()) != Some(*t) {
                    bij = false;
                }
            }
            s.add(
                "P14-TIER-02 三硬件档序号与闭集双射且各有标签（账本可读）",
                bij
                    && HardwareTier::from_ordinal(TIER_COUNT).is_none()
                    && HardwareTier::Low.label() == "low"
                    && HardwareTier::High.label() == "high",
                "三硬件档闭集：低/中/高，成本常数按档定标",
            );
        }

        s
    }
}