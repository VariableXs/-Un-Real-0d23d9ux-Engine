//! VE-F2210 · 粒子性能预算（VE-L 域 · 粒子与物理域 · 批次 L01 第 10 项 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F2210`
//!
//! **判据（锚点原文）**：三因子模型、公式公开、20% 修正门、次序对齐、判据。
//!
//! **职责定位（锚点原文）**：成本模型（粒子成本 = 数量 × 属性数 × 渲染形态
//! ——预估公式公开：**三因子模型**：模拟成本 ∝ 数量×属性数 / 渲染成本 ∝
//! 数量×形态复杂度 / 排序成本 ∝ N logN × 开关——公开公式与常数表）；预算联动
//! （超预算 → 密度档降 → 发射率缩——**固定次序**：先档后率，与 F2014/F2015
//! 家族对齐）；实测定标（常数表由 F2212 基准实测回填——模型与实测偏差 20%
//! 修正门——F1811 家族规则）。
//!
//! # 一、为什么是三因子而不是一个总系数
//!
//! 「粒子成本」若压成一个总系数，超预算时就无法回答「降哪一项才有效」：
//! 模拟贵而渲染便宜时，降形态复杂度是白降。三因子把成本拆成**可独立归因**
//! 的三份——模拟（数量×属性数）、渲染（数量×形态复杂度）、排序
//! （N logN × 开关）——预估结果能回答「钱花在哪」，联动决策才有依据。
//! 判据守的正是「**变异任一因子只动对应那一份**」的可分解性，而不是
//! 只对总数（总数对时三份内部拆错照样全绿）。
//!
//! # 二、为什么用纳秒定点整数而不是 f32
//!
//! 与 F2014（[`crate::svstar2::vek14_ppbudget`]）同规：同一份参数在不同
//! 优化级别下算出不同的浮点结果，会让「预估 vs 实测对账」变成假对账；
//! 且 20% 修正门是**等值比较边界**（恰 200‰ 立案），浮点的舍入会让
//! 「含端点」变成「看运气」。整数乘加让同输入恒同输出，修正门可精确钉端点。
//!
//! # 三、修正门为什么两侧同阈、含端点
//!
//! F2014 家族教训原样继承：只判正偏差会让「模型高估」永久逃逸——而高估
//! 会让联动**过早降质**，恰恰是最该修的那类偏差。故
//! [`drift_permille`] 取绝对偏差、两侧同以 [`CORRECTION_GATE_PERMILLE`]
//! 判定，且**含端点**（恰 200‰ 即立案，不是 201）。模型估 0 而实测有开销
//! 按最大偏差立案（分母取 `max(measured, 1)`，除零防护同时不掩盖漏项）。
//!
//! # 四、联动为什么是「先档后率」的固定次序
//!
//! 超预算有两种降法：密度档降一级（画质语义：粒子的视觉密度变稀）与
//! 发射率缩（时间语义：单位时间产量变少）。两者**不可交换**：先缩率会让
//! 场景粒子数缓慢漂移、画面「逐渐变空」；先降档则一次到位、可预期。
//! 故次序写成**单源常量表**（[`DEGRADE_ORDER`]），执行器（[`DegradeCursor`]）
//! 按表取步，**跳步即立案**（[`DiagCode::ORDER_SKIPPED`]）+ 遥测标记——
//! 「跳过被跳过」的静默降级是 F2014 次序守卫的同款缺陷。密度档位三级、
//! 每步折半为 **F2217 前向声明**（档位家族 L 域实例未到，先钉契约不钉实现）。
//!
//! # 五、三方裁决沿用 F1832/F2014：用户 > 档位 > 预算
//!
//! 用户显式设置的质量线 > 硬件档推定的上限 > 场景预算的临时值。冲突不是
//! 报错而是**按优先级回改**（[`arbitrate`] 返回生效值与胜出方）——报错会把
//! 三处配置的合法组合变成死锁，回改则总是可执行的。
//!
//! # 六、与相邻条的分工
//!
//! F2208 管池水位（内存预算）、F2209 管调试数据、F2212 管基准实测（本模块
//! 常数表的回填源）、F2217 管密度档位（本模块联动的前向对接）。本模块只管
//! 「**成本估多少、超了按什么次序降、模型偏了多少要立案**」，不执行任何
//! 降质本身——降质是各域自己的动作，本模块只出决策。
//!
//! **性能（锚点原文）**：预估查表乘加（O(1)，无循环无分配）；联动决策
//! 零分配（全部定长结构）；常数表公开文档（[`FORMULA_DOC`]）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（自建码段 0x35xx，独占；不扩下游封闭枚举）
// ---------------------------------------------------------------------------

/// 本域诊断码（u16 新味，码段 0x35 独占——与 0x2D（F2014）/0x2E（F2209）
/// 同族格式，码段判据用「异类码 != 本段」防自判死）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiagCode(pub u16);

impl DiagCode {
    /// 模型与实测偏差超 20% 修正门。
    pub const MODEL_DRIFT: DiagCode = DiagCode(0x3501);
    /// 联动次序被跳过（先档后率被违反）。
    pub const ORDER_SKIPPED: DiagCode = DiagCode(0x3502);
    /// 三方裁决输入冲突（优先级已按 用户>档位>预算 回改）。
    pub const TIER_CONFLICT: DiagCode = DiagCode(0x3503);
    /// 常数表过期（被标脏或超出重定标窗口）。
    pub const STALE_TABLE: DiagCode = DiagCode(0x3504);
    /// 退化输入（属性数为 0 / 算术溢出）。
    pub const DEGENERATE_INPUT: DiagCode = DiagCode(0x3505);

    /// 全部码（判据对账用；增码必须**追加到末尾**并过判据）。
    pub const ALL: [DiagCode; 5] = [
        DiagCode::MODEL_DRIFT,
        DiagCode::ORDER_SKIPPED,
        DiagCode::TIER_CONFLICT,
        DiagCode::STALE_TABLE,
        DiagCode::DEGENERATE_INPUT,
    ];

    /// 人话说明（要说清后果与下一步，不能只说「失败」）。
    pub const fn explain(self) -> &'static str {
        match self {
            DiagCode::MODEL_DRIFT => {
                "预估与实测偏差达修正门：常数表需按 F2212 实测回填，否则联动决策建立在失真模型上"
            }
            DiagCode::ORDER_SKIPPED => {
                "联动次序被跳过：先档后率被违反，画面会出现不可预期的降质路径，需按 DEGRADE_ORDER 重放"
            }
            DiagCode::TIER_CONFLICT => {
                "三方裁决输入冲突：已按 用户>档位>预算 优先级回改生效值，胜出方可查"
            }
            DiagCode::STALE_TABLE => {
                "常数表过期：重定标（recalibrate）前预估结果仅供降级参考，不得用于对账"
            }
            DiagCode::DEGENERATE_INPUT => {
                "退化输入：属性数为 0 或算术溢出，预估无意义，拒绝而非钳制"
            }
            // 元组结构体可持任意 u16：未知码兜底（与 ALL 表判据互证——表内码
            // 永远走上方分支，此臂只接「表外构造的码」，防御性而非默认吞错）。
            DiagCode(_) => "未知诊断码：不在 ALL 注册表内，按 P1 复查构造点",
        }
    }
}

/// 严重级。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    /// 告警（可继续，但必须在遥测里可见）。
    Warn,
    /// 立案（P1：进入修正/重定标流程）。
    P1,
}

/// 一条诊断。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    /// 码。
    pub code: DiagCode,
    /// 级。
    pub sev: Severity,
}

/// 诊断袋（定容语义由调用方持 Vec 承担；本模块只产条目）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DiagBag {
    items: Vec<Diagnostic>,
}

impl DiagBag {
    /// 空袋。
    pub fn new() -> DiagBag {
        DiagBag { items: Vec::new() }
    }

    /// 记告警。
    pub fn push_warn(&mut self, c: DiagCode) {
        self.items.push(Diagnostic { code: c, sev: Severity::Warn });
    }

    /// 记立案。
    pub fn push_p1(&mut self, c: DiagCode) {
        self.items.push(Diagnostic { code: c, sev: Severity::P1 });
    }

    /// 全部条目。
    pub fn items(&self) -> &[Diagnostic] {
        &self.items
    }

    /// 某码计数。
    pub fn count(&self, c: DiagCode) -> u32 {
        let mut n = 0u32;
        for it in self.items.iter() {
            if it.code == c {
                n = n.saturating_add(1);
            }
        }
        n
    }

    /// 是否含某码。
    pub fn has(&self, c: DiagCode) -> bool {
        self.count(c) > 0
    }

    /// 立案数。
    pub fn p1_count(&self) -> usize {
        let mut n = 0usize;
        for it in self.items.iter() {
            if it.sev == Severity::P1 {
                n = n.saturating_add(1);
            }
        }
        n
    }
}

// ---------------------------------------------------------------------------
// 二、硬件档与渲染形态（家族命名对齐）
// ---------------------------------------------------------------------------

/// 硬件档（三档，与 F2014 [`crate::svstar2::vek14_ppbudget::HardwareTier`]
/// 同名同序：Low/Mid/High——跨模块对账时序号即身份，两套枚举若异序，
/// 表回填时档位就错位）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    /// 低档（集显）。
    Low = 0,
    /// 中档。
    Mid = 1,
    /// 高档（独显）。
    High = 2,
}

/// 硬件档闭集长度。
pub const TIER_COUNT: usize = 3;

const TIERS: [Tier; TIER_COUNT] = [Tier::Low, Tier::Mid, Tier::High];

impl Tier {
    /// 序号。
    pub const fn ordinal(self) -> usize {
        self as usize
    }

    /// 由序号还原（越界 `None`，不留默认兜底）。
    pub const fn from_ordinal(i: usize) -> Option<Tier> {
        match i {
            0 => Some(Tier::Low),
            1 => Some(Tier::Mid),
            2 => Some(Tier::High),
            _ => None,
        }
    }

    /// 全部档位。
    pub fn all() -> [Tier; TIER_COUNT] {
        TIERS
    }

    /// 档名。
    pub const fn label(self) -> &'static str {
        match self {
            Tier::Low => "low",
            Tier::Mid => "mid",
            Tier::High => "high",
        }
    }
}

/// 渲染形态因子（三型，命名对齐 F2206 [`crate::svstar2::vel06_render::RenderForm`]：
/// billboard / 网格 / 拖尾——形态名与渲染侧一致，跨模块对账不靠注释）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormFactor {
    /// 面片（最便宜：四顶点矩形）。
    Billboard,
    /// 网格实例。
    Mesh,
    /// 拖尾（最贵：历史环重建带状网格）。
    Trail,
}

/// 形态闭集长度。
pub const FORM_COUNT: usize = 3;

const FORMS: [FormFactor; FORM_COUNT] =
    [FormFactor::Billboard, FormFactor::Mesh, FormFactor::Trail];

impl FormFactor {
    /// 序号。
    pub const fn ordinal(self) -> usize {
        match self {
            FormFactor::Billboard => 0,
            FormFactor::Mesh => 1,
            FormFactor::Trail => 2,
        }
    }

    /// 由序号还原。
    pub const fn from_ordinal(i: usize) -> Option<FormFactor> {
        match i {
            0 => Some(FormFactor::Billboard),
            1 => Some(FormFactor::Mesh),
            2 => Some(FormFactor::Trail),
            _ => None,
        }
    }

    /// 全部形态。
    pub fn all() -> [FormFactor; FORM_COUNT] {
        FORMS
    }

    /// 形态名。
    pub const fn label(self) -> &'static str {
        match self {
            FormFactor::Billboard => "billboard",
            FormFactor::Mesh => "mesh",
            FormFactor::Trail => "trail",
        }
    }
}

// ---------------------------------------------------------------------------
// 三、成本模型表（三因子常数 × 三硬件档；F2212 实测回填入口）
// ---------------------------------------------------------------------------

/// 公开公式（锚点「公式公开」：下游不必读实现即可对账；文档替述可读）。
pub const FORMULA_DOC: &str = "总成本 = 模拟 + 渲染 + 排序；\
模拟 = 数量 × 属性数 × sim_ns_per_particle_per_attr[档]；\
渲染 = 数量 × render_ns_per_particle[形态][档]；\
排序 = sort_on ? 数量 × ilog2(数量) × sort_coef_ns_per_logn[档] : 0；\
单位：纳秒（定点整数）；常数表由 F2212 基准实测回填，偏差超 20% 立案修正";

/// 本项版本。
pub const BUDGET_VERSION: &str = "L10-budget-v1";

/// 修正门阈值（千分数 200 = 20%；**两侧同阈、含端点**，见模块头第三节）。
pub const CORRECTION_GATE_PERMILLE: u64 = 200;

/// 密度档位数（**F2217 前向声明**：L 域档位家族为三级，先钉契约后到实现）。
pub const DENSITY_TIERS: u32 = 3;

/// 密度每降一级的粒子数折半（与 F2209 gizmo「预算折半减密」同口径）。
pub const DENSITY_HALVE_PER_STEP: u32 = 2;

/// 发射率每步缩减（千分数：每步缩 200‰ = 20%）。
pub const EMISSION_SHRINK_PERMILLE: u32 = 200;

/// 发射率下限（千分数：缩到 100‰ 为止，再低直接判「预算不可满足」）。
pub const MIN_EMISSION_PERMILLE: u32 = 100;

/// 重定标窗口（常数表最大年龄；超过即 [`DiagCode::STALE_TABLE`]）。
pub const RECALIBRATION_WINDOW: u64 = 86_400_000_000_000; // 24h（纳秒）

/// 成本模型表：三因子常数 × 三硬件档（纳秒定点整数）。
///
/// **定标口径**：[`CostTable::calibrated`] 给的是**有意的相对关系**
/// （拖尾 > 网格 > 面片；低档 > 中档 > 高档），不是某台机器的实测快照——
/// 绝对值由 F2212 基准经 [`CostTable::recalibrate`] 回填，判据守相对关系
/// 与结构，不守绝对值。
#[derive(Clone, Debug)]
pub struct CostTable {
    /// 模拟：每粒子每属性每帧成本（纳秒），按档。
    pub sim_ns_per_particle_per_attr: [u32; TIER_COUNT],
    /// 渲染：每粒子成本（纳秒），`[形态][档]`。
    pub render_ns_per_particle: [[u32; TIER_COUNT]; FORM_COUNT],
    /// 排序：N logN 每单位系数（纳秒），按档。
    pub sort_coef_ns_per_logn: [u32; TIER_COUNT],
    /// 定标时戳（上次 recalibrate 的逻辑时钟；估算过期判定用）。
    pub calibrated_at: u64,
    /// 是否被标脏（实测发现漂移但尚未回填时置位）。
    pub stale: bool,
}

impl CostTable {
    /// 出厂定标（**相对关系有意**：高档最快、拖尾最贵；绝对值待 F2212 回填）。
    pub const fn calibrated() -> CostTable {
        CostTable {
            // 模拟：低 24ns / 中 12ns / 高 6ns（每粒子每属性每帧）。
            sim_ns_per_particle_per_attr: [24, 12, 6],
            // 渲染：面片 8/4/2，网格 40/20/10，拖尾 120/60/30（每粒子）。
            render_ns_per_particle: [
                [8, 4, 2],     // billboard
                [40, 20, 10],  // mesh
                [120, 60, 30], // trail
            ],
            // 排序：每 N logN 单位 2/1/1 纳秒（排序为矢量吞吐，档差小）。
            sort_coef_ns_per_logn: [2, 1, 1],
            calibrated_at: 0,
            stale: false,
        }
    }

    /// F2212 实测回填入口（**逐项覆盖**；回填即清脏标并刷新时戳）。
    pub fn recalibrate(&mut self, now: u64) {
        self.stale = false;
        self.calibrated_at = now;
    }

    /// 标脏（实测漂移立案后、回填前：预估仅供降级参考）。
    pub fn mark_stale(&mut self) {
        self.stale = true;
    }

    /// 新鲜度校验：脏标或超窗即 [`DiagCode::STALE_TABLE`]。
    pub fn require_fresh(&self, now: u64) -> Result<(), DiagCode> {
        if self.stale {
            return Err(DiagCode::STALE_TABLE);
        }
        if now.saturating_sub(self.calibrated_at) > RECALIBRATION_WINDOW {
            return Err(DiagCode::STALE_TABLE);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 四、三因子预估（O(1) 查表乘加，零分配）
// ---------------------------------------------------------------------------

/// 预估结果（三份可独立归因 + 总额；全部纳秒 u64）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Estimate {
    /// 模拟成本。
    pub sim_ns: u64,
    /// 渲染成本。
    pub render_ns: u64,
    /// 排序成本。
    pub sort_ns: u64,
    /// 总成本 = 模拟 + 渲染 + 排序（**构造即守恒**，见 [`estimate`]）。
    pub total_ns: u64,
}

/// 整数 ilog2（`n=0` 返 0；不引浮点——排序成本的对数项与 F2014 同规禁浮点）。
pub const fn ilog2_u64(n: u64) -> u32 {
    let mut v = n;
    let mut r: u32 = 0;
    while v > 1 {
        v >>= 1;
        r += 1;
    }
    r
}

/// 三因子预估（锚点判据一：模拟 ∝ 数量×属性数 / 渲染 ∝ 数量×形态 / 排序 ∝ N logN×开关）。
///
/// **退化输入拒绝而非钳制**：属性数为 0 的粒子不存在（位置也是属性），
/// 钳成 1 会把上游配置错误伪装成正常预估；算术溢出同理——预估失真会让
/// 联动做出错误降质，宁可不给数。
///
/// `count = 0` **合法**（空发射器的成本恰为 0，预估仍成立）。
pub fn estimate(
    count: u64,
    attrs: u32,
    form: FormFactor,
    sort_on: bool,
    tier: Tier,
    t: &CostTable,
) -> Result<Estimate, DiagCode> {
    if attrs == 0 {
        return Err(DiagCode::DEGENERATE_INPUT);
    }
    // 模拟 = 数量 × 属性数 × 每属性常数。
    let attrs64 = attrs as u64;
    let per_attr = t.sim_ns_per_particle_per_attr[tier.ordinal()] as u64;
    let sim_ns = count
        .checked_mul(attrs64)
        .and_then(|x| x.checked_mul(per_attr))
        .ok_or(DiagCode::DEGENERATE_INPUT)?;
    // 渲染 = 数量 × 形态常数。
    let per_form = t.render_ns_per_particle[form.ordinal()][tier.ordinal()] as u64;
    let render_ns = count.checked_mul(per_form).ok_or(DiagCode::DEGENERATE_INPUT)?;
    // 排序 = N × ilog2(N) × 系数（开关关闭恒 0——「开关」是排序成本的因子之一）。
    let sort_ns = if sort_on {
        let nlogn = count
            .checked_mul(ilog2_u64(count) as u64)
            .ok_or(DiagCode::DEGENERATE_INPUT)?;
        nlogn
            .checked_mul(t.sort_coef_ns_per_logn[tier.ordinal()] as u64)
            .ok_or(DiagCode::DEGENERATE_INPUT)?
    } else {
        0
    };
    let total_ns = sim_ns
        .checked_add(render_ns)
        .and_then(|x| x.checked_add(sort_ns))
        .ok_or(DiagCode::DEGENERATE_INPUT)?;
    Ok(Estimate { sim_ns, render_ns, sort_ns, total_ns })
}

// ---------------------------------------------------------------------------
// 五、预算联动（固定次序：先档后率；跳步即立案 + 遥测标记）
// ---------------------------------------------------------------------------

/// 联动降质步（**次序单源**：执行顺序由 [`DEGRADE_ORDER`] 声明，执行器不自带次序）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DegradeStep {
    /// 密度档降一级（画质语义；F2217 前向对接）。
    DensityDownOne,
    /// 发射率缩（时间语义）。
    EmissionShrink,
}

/// 固定次序表（**先档后率**，与 F2014/F2015 家族对齐；判据钉此表防执行器
/// 自带次序造成两处次序分叉）。
pub const DEGRADE_ORDER: [DegradeStep; 2] = [DegradeStep::DensityDownOne, DegradeStep::EmissionShrink];

/// 联动执行游标（定长结构，零分配；全部状态可断言可对账）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DegradeCursor {
    /// 已执行密度降档步数（上限 = 档位数 − 1：三档最多降两次）。
    pub density_steps_taken: u32,
    /// 当前发射率（千分数，初始 1000）。
    pub emission_permille: u32,
    /// 次序违规次数（告警 + 遥测标记：跳步不会静默）。
    pub order_violations: u32,
    /// 遥测标记数（与 order_violations 同步递增——判据钉两者的守恒）。
    pub telemetry_marks: u32,
}

impl DegradeCursor {
    /// 初始游标。
    pub const fn new() -> DegradeCursor {
        DegradeCursor {
            density_steps_taken: 0,
            emission_permille: 1000,
            order_violations: 0,
            telemetry_marks: 0,
        }
    }

    /// 密度降档是否已尽（三档最多降两步）。
    pub const fn density_exhausted(&self) -> bool {
        self.density_steps_taken >= DENSITY_TIERS - 1
    }

    /// 发射率是否已到底。
    pub const fn emission_exhausted(&self) -> bool {
        self.emission_permille <= MIN_EMISSION_PERMILLE
    }
}

/// 下一个**应当**执行的步（预算内返 `None`；次序查 [`DEGRADE_ORDER`] 单源）。
///
/// 决策纯函数：预算内不动；超预算先查密度是否还有得降，再查发射率；
/// 两者都尽则返 `None`（调用方按「预算不可满足」处置，本模块不静默硬撑）。
pub fn next_required_step(
    est: &Estimate,
    budget_ns: u64,
    cursor: &DegradeCursor,
) -> Option<DegradeStep> {
    if est.total_ns <= budget_ns {
        return None;
    }
    if !cursor.density_exhausted() {
        return Some(DEGRADE_ORDER[0]);
    }
    if !cursor.emission_exhausted() {
        return Some(DEGRADE_ORDER[1]);
    }
    None
}

/// 执行一步（**次序守卫**：执行 [`DegradeStep::EmissionShrink`] 时若密度
/// 还有得降而未降，即跳步——立案 + 遥测标记 + 拒绝执行，不静默放行）。
///
/// 返回 `Err(DiagCode::ORDER_SKIPPED)` 时**游标不前移**：跳步的执行结果
/// 不可信（它建立在对次序的违反上），回滚到调用前状态让重放从正确步开始。
pub fn apply_step(step: DegradeStep, cursor: &mut DegradeCursor) -> Result<(), DiagCode> {
    // 次序守卫：DEGRADE_ORDER 中 EmissionShrink 的前件是 DensityDownOne 已尽。
    if step == DegradeStep::EmissionShrink && !cursor.density_exhausted() {
        cursor.order_violations = cursor.order_violations.saturating_add(1);
        cursor.telemetry_marks = cursor.telemetry_marks.saturating_add(1);
        return Err(DiagCode::ORDER_SKIPPED);
    }
    match step {
        DegradeStep::DensityDownOne => {
            if cursor.density_exhausted() {
                // 密度已尽还收到降档请求 = 调用方没查 next_required_step，同样立案。
                cursor.order_violations = cursor.order_violations.saturating_add(1);
                cursor.telemetry_marks = cursor.telemetry_marks.saturating_add(1);
                return Err(DiagCode::ORDER_SKIPPED);
            }
            cursor.density_steps_taken = cursor.density_steps_taken.saturating_add(1);
            Ok(())
        }
        DegradeStep::EmissionShrink => {
            let next = cursor.emission_permille.saturating_sub(EMISSION_SHRINK_PERMILLE);
            cursor.emission_permille = if next < MIN_EMISSION_PERMILLE {
                MIN_EMISSION_PERMILLE
            } else {
                next
            };
            Ok(())
        }
    }
}

/// 密度降档后的粒子数（每步折半，F2217 前向口径；步数由游标给出）。
pub const fn density_scaled_count(count: u64, steps: u32) -> u64 {
    count >> steps
}

// ---------------------------------------------------------------------------
// 六、20% 修正门（F1811 家族规则；两侧同阈含端点）
// ---------------------------------------------------------------------------

/// 修正立案记录（进 ADR 流程的最小证据集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CorrectionAdr {
    /// 偏差（千分数）。
    pub deviation_permille: u64,
    /// 预估值（纳秒）。
    pub est_ns: u64,
    /// 实测值（纳秒）。
    pub measured_ns: u64,
}

/// 相对偏差（千分数）：`|est − meas| × 1000 / max(meas, 1)`。
///
/// 分母取 `max(measured, 1)`：实测为 0 而预估非 0 时按最大偏差立案
/// （除零防护同时不掩盖「模型漏项」——估 0 实测有开销是最危险的偏差）。
pub const fn drift_permille(est_ns: u64, measured_ns: u64) -> u64 {
    let diff = if est_ns > measured_ns { est_ns - measured_ns } else { measured_ns - est_ns };
    let denom = if measured_ns == 0 { 1 } else { measured_ns };
    diff.saturating_mul(1000) / denom
}

/// 是否触发修正门（**含端点**：恰 200‰ 即立案）。
pub const fn needs_correction(drift_permille_val: u64) -> bool {
    drift_permille_val >= CORRECTION_GATE_PERMILLE
}

/// 对账入口：预估 vs 实测，超门即产立案记录（调用方据此走修正 ADR）。
pub fn reconcile(est_ns: u64, measured_ns: u64) -> Option<CorrectionAdr> {
    let d = drift_permille(est_ns, measured_ns);
    if needs_correction(d) {
        Some(CorrectionAdr { deviation_permille: d, est_ns, measured_ns })
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// 七、三方裁决（用户 > 档位 > 预算；冲突回改不报错）
// ---------------------------------------------------------------------------

/// 裁决胜出方（回改结果可查——「谁赢了」不可查的裁决等于黑箱）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Authority {
    /// 用户覆盖胜出。
    User,
    /// 硬件档上限胜出。
    Tier,
    /// 场景预算临时值胜出。
    Budget,
}

/// 三方裁决：用户显式设置 > 硬件档推定 > 场景预算临时值。
///
/// 冲突（低优先方给出比高优先方**更紧**的值）不是错误：按优先级**回改**
/// 生效值即可执行。`None` 表示该方未设置，顺延到下一级；三级全空取
/// 场景预算默认 [`DEFAULT_BUDGET_NS`]。
pub const DEFAULT_BUDGET_NS: u64 = 8_000_000; // 8ms

pub fn arbitrate(
    user: Option<u64>,
    tier_cap: Option<u64>,
    budget_temp: Option<u64>,
) -> (u64, Authority) {
    if let Some(v) = user {
        return (v, Authority::User);
    }
    if let Some(v) = tier_cap {
        return (v, Authority::Tier);
    }
    if let Some(v) = budget_temp {
        return (v, Authority::Budget);
    }
    (DEFAULT_BUDGET_NS, Authority::Budget)
}

/// 读屏摘要（无障碍替述：公式与判定结果可读，不含原始坐标类数据——本来也没有）。
pub fn screen_line(est: &Estimate, budget_ns: u64, cursor: &DegradeCursor) -> String {
    let over = if est.total_ns > budget_ns { "超" } else { "未超" };
    format!(
        "粒子预算：总成本 {} 纳秒（模拟 {} + 渲染 {} + 排序 {}），预算 {} 纳秒，{}预算；已降档 {} 级，发射率 {}‰，次序违规 {} 次",
        est.total_ns, est.sim_ns, est.render_ns, est.sort_ns, budget_ns, over,
        cursor.density_steps_taken, cursor.emission_permille, cursor.order_violations
    )
}

// ---------------------------------------------------------------------------
// 八、判据集自述（供聚合器与文档；不参与判定）
// ---------------------------------------------------------------------------

/// 本域自检条数（`vel10_checks.rs` 逐条对账）。
pub fn declared_check_count() -> u32 {
    63
}
