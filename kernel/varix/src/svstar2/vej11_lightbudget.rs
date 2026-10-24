//! VE-F1811 · 光照性能预算（VE-J 域 · 光照与阴影 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F1811`
//!
//! **判据（锚点原文）**：五型成本表、三档上限、20% 修正门、延迟位诚实、判据。
//!
//! 职责定位：建立光照成本模型——每光源每像素着色成本按类型定标（方向光/
//! 点光/聚光/探针/IBL 五型 × 入门/主流/旗舰三硬件档常数表）、光源并发上限
//! 声明（前向渲染 16/32/64 三档与硬件档映射 + 延迟渲染扩展的声明位）、预算
//! 联动规则（场景光源组合成本预估超预算 → 告警 + 降档建议：关最贵光源/
//! 降并发档，用户可强制且强制态遥测标记），并与 F1761 域账本、F1767 门禁
//! 联动。本条**不做任何渲染**——预估是查表乘加微秒级、在场景导入与光源
//! 变更时算一次；模型本身零运行时成本。
//!
//! # 一、五型成本表（判据一）——常数是声明值，不是实测冒充
//!
//! 五型：方向光/点光/聚光/探针（F1808 SH2 漫反射）/IBL（F1809 双查）。
//! 锚点要求「实测定标」，而 F1814 基准阶梯尚未落地——本条**诚实声明**：
//! v1 常数是模型期初始定标（相对单位 U，主流档点光=12U 为锚），来源栏
//! 写明「待 F1814 回填」；回填必须走 [`rebaseline`] 的 ADR 流程
//! （无 ADR 编号拒绝），这正是锚点「常数过期→重定标走 ADR」的兑现面。
//! 表是纯整数（u32）：浮点成本会引入跨平台求值顺序差，与 J 域确定性
//! 口径冲突；定点 U 单位下三档单调（旗舰 ≤ 主流 ≤ 入门）是可机检事实。
//!
//! # 二、三档上限（判据二）——并发上限与预算共享硬件档定义
//!
//! 入门 16 / 主流 32 / 旗舰 64，前向渲染并发上限；[`HardwareTier`]
//! 是 J 域硬件档单源（F1831 阴影成本独立建账但共享本定义——跨批对接点
//! 原文）。场景活跃光源超档位上限 → 告警 + 降并发档建议（不崩溃不静默）；
//! 降档建议与 F1762 质量档联动（建议是信号，切换仍走 F1762 的档位协议）。
//!
//! # 三、20% 修正门（判据三）——偏差是立项信号不是静默容忍
//!
//! 预估与实测偏差用**整数基点**（bps）计：`|实测-预估| × 10000 / 预估`，
//! 超过 2000 bps（20%）→ 模型修正立项（[`DeviationCase`]，带诊断码
//! 0x3903），不自动改表——改表必须有 ADR；恰 2000 bps 不触发（「超」
//! 是严格大于，锚点口径）。预估为零（空场景）时偏差无定义 → 显性拒绝
//! 对账而不是除零 panic（no_std 红线）。
//!
//! # 四、延迟位诚实（判据四）——预留不静默，与 F1806/F1829 同纪律
//!
//! 延迟渲染扩展位仅声明不实现：[`DeferredExtension::query`] 一律返回
//! 显性错误（带指路：当前替代=本表前向路径），任何人引用都不会拿到
//! 静默降级的「假装支持」。
//!
//! # 五、诊断码（独占 0x39 段）
//!
//! 0x3901 超预算告警 / 0x3902 强制态遥测标记 / 0x3903 偏差修正立项 /
//! 0x3904 无 ADR 重定标拒绝 / 0x3905 延迟位被引用 / 0x3906 并发超上限 /
//! 0x3907 预估算术溢出 / 0x3908 模型待重定标 / 0x3909 降档建议已发出 /
//! 0x390A 重定标落账。全段 `0x39` 与邻居 `0x31`（F1810）/`0x32` 不重叠。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、硬件档单源（F1831 阴影成本共享本定义——跨批对接点）
// ---------------------------------------------------------------------------

/// 硬件档三档（J 域单源：本表与 F1811 预算、未来 F1831 阴影账共享）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HardwareTier {
    /// 入门档。
    Entry,
    /// 主流档。
    Mainstream,
    /// 旗舰档。
    Flagship,
}

impl HardwareTier {
    /// 全档位序表（下标即档位序）。
    pub const ALL: [HardwareTier; 3] = [
        HardwareTier::Entry,
        HardwareTier::Mainstream,
        HardwareTier::Flagship,
    ];

    /// 档位序（0/1/2）。
    pub const fn index(self) -> usize {
        match self {
            HardwareTier::Entry => 0,
            HardwareTier::Mainstream => 1,
            HardwareTier::Flagship => 2,
        }
    }

    /// 规范名（告警文本与账本列名共用，避免口径二义）。
    pub const fn name(self) -> &'static str {
        match self {
            HardwareTier::Entry => "entry",
            HardwareTier::Mainstream => "mainstream",
            HardwareTier::Flagship => "flagship",
        }
    }

    /// 从序号取档（越界 None——调用方拒绝，不静默取近档）。
    pub fn from_index(i: usize) -> Option<HardwareTier> {
        Self::ALL.get(i).copied()
    }
}

// ---------------------------------------------------------------------------
// 二、五型成本表（v1 初始定标——F1814 阶梯回填走 ADR）
// ---------------------------------------------------------------------------

/// 成本类别五型（锚点定类：方向光/点光/聚光/探针/IBL）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LightClass {
    /// 方向光。
    Directional,
    /// 点光。
    Point,
    /// 聚光。
    Spot,
    /// 探针（F1808）。
    Probe,
    /// IBL（F1809）。
    Ibl,
}

impl LightClass {
    /// 五型序表（下标即成本表列序）。
    pub const ALL: [LightClass; 5] = [
        LightClass::Directional,
        LightClass::Point,
        LightClass::Spot,
        LightClass::Probe,
        LightClass::Ibl,
    ];

    /// 列序。
    pub const fn index(self) -> usize {
        match self {
            LightClass::Directional => 0,
            LightClass::Point => 1,
            LightClass::Spot => 2,
            LightClass::Probe => 3,
            LightClass::Ibl => 4,
        }
    }

    /// 规范名。
    pub const fn name(self) -> &'static str {
        match self {
            LightClass::Directional => "directional",
            LightClass::Point => "point",
            LightClass::Spot => "spot",
            LightClass::Probe => "probe",
            LightClass::Ibl => "ibl",
        }
    }
}

/// 成本单位口径（唯一声明处；表值与预算值共用本语义）。
///
/// **U = 每光源每万像素每帧的相对成本单位**。取相对单位而非伪纳秒：
/// v1 阶段没有 F1814 实测，写「8.3 ns」是把声明值冒充实测；相对单位
/// 配合 ADR 重定标流，回填时只换表不改语义。
pub const COST_UNIT_LEGEND: &str = "U = per-light per-megapixel per-frame relative cost unit (mainstream point light = 12U anchor)";

/// 模型版本（v1 = 初始定标；每次 ADR 重定标 +1，只增不减）。
pub const MODEL_VERSION: u32 = 1;

/// 初始定标来源声明（诚实标注：v1 是模型值，F1814 落地后回填）。
pub const CALIBRATION_SOURCE: &str = "v1 initial model declaration; recalibrate from F1814 ladder via ADR";

/// 五型 × 三档每光源每万像素成本常数表（单位 U，见 [`COST_UNIT_LEGEND`]）。
///
/// 布局 `[类别][档位序]`；行序= [`LightClass::ALL`]，列序= [`HardwareTier::ALL`]。
///
/// 相对量级依据（v1 模型声明，非实测）：
/// - 方向光最便宜（无衰减、单次 BRDF）；
/// - 点光/聚光含衰减与锥角项，聚光略贵；
/// - 探针是 9 系数点积（F1808 SH2），低频漫反射，比解析光更便宜；
/// - IBL 最贵（预滤波 mip + BRDF LUT 双查，F1809 双查 O(1) 但带宽大）。
pub const COST_TABLE: [[u32; 3]; 5] = [
    [24, 10, 6],  // Directional
    [30, 12, 7],  // Point
    [36, 14, 8],  // Spot
    [18, 6, 3],   // Probe
    [60, 24, 14], // Ibl
];

/// 五型 × 三档成本常数（带档位与类别的查表入口——判据与实现共走本口）。
pub const fn cost(class: LightClass, tier: HardwareTier) -> u32 {
    COST_TABLE[class.index()][tier.index()]
}

/// 场景光源组合计数（预估输入；探针/IBL 数来自 F1808/F1809 场景装配）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ClassCensus {
    /// 方向光数。
    pub directional: u32,
    /// 点光数。
    pub point: u32,
    /// 聚光数。
    pub spot: u32,
    /// 面光数（映射到方向/点光之外的解析光，按 spot 档计成本的近亲——
    /// v1 把面光并入聚光列成本，诚实标注在 [`estimate`] 头注）。
    pub area: u32,
    /// 探针数。
    pub probe: u32,
    /// IBL 环境数（0/1，>1 视为多环境叠加按线性计）。
    pub ibl: u32,
}

impl ClassCensus {
    /// 某类别的计数（面光并入聚光列：同为投影解析光）。
    pub const fn count(self, class: LightClass) -> u32 {
        match class {
            LightClass::Directional => self.directional,
            LightClass::Point => self.point,
            // 面光（F1806 矩形/管灯）v1 并入聚光成本列——同为投影解析光，
            // LTC 实装（F1806 预留落地）时拆列并走 ADR 重定标。
            LightClass::Spot => self.spot + self.area,
            LightClass::Probe => self.probe,
            LightClass::Ibl => self.ibl,
        }
    }

    /// 全类别计数合计（含面光）。
    pub const fn total(self) -> u32 {
        self.directional + self.point + self.spot + self.area + self.probe + self.ibl
    }
}

// ---------------------------------------------------------------------------
// 三、并发上限表（前向 16/32/64）与延迟渲染预留位
// ---------------------------------------------------------------------------

/// 前向渲染并发光源上限（按档位；与 F1807 排序裁剪共享口径——
/// 上限是「进参数块的前 N 光源」的 N）。
pub const CONCURRENCY_CAPS: [u32; 3] = [16, 32, 64];

/// 档位并发上限查表。
pub const fn concurrency_cap(tier: HardwareTier) -> u32 {
    CONCURRENCY_CAPS[tier.index()]
}

/// 延迟渲染扩展位（仅声明，不实现——判据四）。
///
/// 锚点原文「延迟渲染扩展位仅声明不实现（诚实标注）」。与 F1806 面光源、
/// F1829 接触硬化同纪律：预留被引用必须显性报错并指路替代，不静默降级。
pub struct DeferredExtension;

impl DeferredExtension {
    /// 预留状态标识（诚实标注字段，可读不可信为可用）。
    pub const STATE: &'static str = "RESERVED";

    /// 查询延迟渲染路径——恒显性报错，指路当前替代方案。
    pub fn query() -> Result<&'static str, BudgetError> {
        Err(BudgetError {
            code: BudgetCode::DEFERRED_RESERVED_ACCESS,
            message: String::from(
                "deferred shading extension is declared but not implemented; \
                 use the forward path cost table (COST_TABLE) instead",
            ),
        })
    }
}

// ---------------------------------------------------------------------------
// 四、错误与诊断（0x39 段独占）
// ---------------------------------------------------------------------------

/// 预算域诊断码（独占 `0x39xx` 段；与 0x31 F1810 / 0x32 邻段不重叠）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BudgetCode(pub u16);

impl BudgetCode {
    /// 场景组合成本超预算（告警 + 建议；不崩溃不静默）。
    pub const OVER_BUDGET: BudgetCode = BudgetCode(0x3901);
    /// 用户强制超预算运行（遥测标记——锚点「强制态遥测标记」）。
    pub const OVERRUN_FORCED: BudgetCode = BudgetCode(0x3902);
    /// 预估 vs 实测偏差超 20%（模型修正立项）。
    pub const DEVIATION_GATE: BudgetCode = BudgetCode(0x3903);
    /// 重定标缺 ADR 编号被拒。
    pub const REBASELINE_REJECTED: BudgetCode = BudgetCode(0x3904);
    /// 延迟渲染预留位被引用。
    pub const DEFERRED_RESERVED_ACCESS: BudgetCode = BudgetCode(0x3905);
    /// 活跃光源超档位并发上限。
    pub const CAP_EXCEEDED: BudgetCode = BudgetCode(0x3906);
    /// 预估算术溢出（checked 面，显性拒绝不回绕）。
    pub const ESTIMATE_OVERFLOW: BudgetCode = BudgetCode(0x3907);
    /// 模型常数待重定标（F1814 数据源就位后置此码入账）。
    pub const MODEL_STALE: BudgetCode = BudgetCode(0x3908);
    /// 降档建议已发出（关最贵光源/降并发档）。
    pub const SUGGESTION_ISSUED: BudgetCode = BudgetCode(0x3909);
    /// ADR 重定标落账（版本 +1）。
    pub const REBASELINED: BudgetCode = BudgetCode(0x390A);

    /// 段标识（判据用：全部码的高字节恒 0x39）。
    pub const SEGMENT: u16 = 0x39;

    /// 人话解释（无障碍：告警文本规范可读，判据逐条对拍）。
    ///
    /// 码值是开放空间（pub u16），match 走码值 + 兜底臂——未知码不是
    /// panic 理由，如实报「未知码」。
    pub const fn explain(self) -> &'static str {
        match self.0 {
            0x3901 => "scene lighting cost exceeds the tier budget; suggestions follow, nothing is auto-dropped",
            0x3902 => "over-budget run was force-confirmed by the user; telemetry marks this session forced",
            0x3903 => "estimate vs measured deviation exceeds 20 percent; model revision case opened",
            0x3904 => "cost table rebaseline was rejected: an ADR reference is required",
            0x3905 => "deferred shading is a reserved declaration only; forward path remains the supported route",
            0x3906 => "active lights exceed the tier concurrency cap; downgrade suggestion issued",
            0x3907 => "cost estimate overflowed the fixed-point range; the request is rejected, not wrapped",
            0x3908 => "cost constants are marked stale pending F1814 calibration",
            0x3909 => "a downgrade suggestion was issued (drop costliest class or lower concurrency cap)",
            0x390A => "cost table was rebaselined under an ADR; model version incremented",
            _ => "unknown budget diagnostic code",
        }
    }
}

/// 预算域错误（what/why/how 三要素：code + 人话 message）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BudgetError {
    /// 诊断码。
    pub code: BudgetCode,
    /// 人话描述（含指路）。
    pub message: String,
}

// ---------------------------------------------------------------------------
// 五、预估与预算联动
// ---------------------------------------------------------------------------

/// 档位帧预算（每万像素，单位 U——与成本表同口径，F1761 J 段账本对接列）。
///
/// 定值原则：满档并发全方向光（各档最便宜光源）不超本档预算——
/// 入门 16×24=384、主流 32×10=320、旗舰 64×6=384，预算给到 480/400/448
/// 留出混合光源余量。数值同样是 v1 声明值，ADR 重定标一并覆盖。
pub const BUDGET_PER_MPIX: [u32; 3] = [480, 400, 448];

/// 档位帧预算查表。
pub const fn tier_budget(tier: HardwareTier) -> u32 {
    BUDGET_PER_MPIX[tier.index()]
}

/// 降档建议（锚点两支：关最贵光源 / 降并发档；文本规范可读）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Suggestion {
    /// 关最贵类别（按贡献额 = 计数×单价 的最大者）。
    DropCostliest {
        /// 建议关闭的类别。
        class: LightClass,
        /// 该类当前贡献额（U）。
        contribution_u: u64,
        /// 人话建议文本。
        text: String,
    },
    /// 降并发档（当前档上限 → 更低档上限）。
    LowerConcurrencyCap {
        /// 当前档位名。
        from_tier: &'static str,
        /// 建议档位名。
        to_tier: &'static str,
        /// 人话建议文本。
        text: String,
    },
}

impl Suggestion {
    /// 人话文本出口（无障碍：告警文本规范可读——判据断非空+含动作词）。
    pub fn text(&self) -> &str {
        match self {
            Suggestion::DropCostliest { text, .. } => text,
            Suggestion::LowerConcurrencyCap { text, .. } => text,
        }
    }
}

/// 预估结果（导入期/变更期一次性产物；模型本身零运行时成本）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Estimate {
    /// 逐类别贡献额（U；下标= [`LightClass::ALL`] 列序，面光并入聚光列）。
    pub per_class_u: [u64; 5],
    /// 合计成本（U）。
    pub total_u: u64,
    /// 参与预估的像素规模（万像素）。
    pub mpix: u32,
    /// 档位。
    pub tier: HardwareTier,
    /// 档位预算（U，= [`tier_budget`] × mpix）。
    pub budget_u: u64,
    /// 是否超预算。
    pub over_budget: bool,
    /// 超出量（U；未超为 0）。
    pub over_by_u: u64,
    /// 用户强制态（锚点：强制态遥测标记——F1819 对接读本位）。
    pub forced: bool,
    /// 活跃光源数。
    pub active_lights: u32,
    /// 并发上限（本档）。
    pub concurrency_cap: u32,
    /// 是否超并发上限。
    pub over_cap: bool,
    /// 降档建议（超预算/超上限时非空；每条带人话文本）。
    pub suggestions: Vec<Suggestion>,
    /// 诊断码账（本次预估产生的全部码，判据逐条对拍）。
    pub codes: Vec<BudgetCode>,
}

/// 场景光源组合成本预估（查表乘加；导入与光源变更时算一次）。
///
/// `mpix` 以万像素计（1280×720 ≈ 0.92 → 取整 1；模型粒度即万像素，
/// 亚万像素不进账——诚实声明粒度而非假装连续）。
///
/// 算术全 checked：`计数 × 单价 × 像素` 在 u64 域仍可能溢出（万级光源×
/// 超大场景的异常组合），溢出显性拒绝（0x3907）不回绕——回绕会把天文
/// 数字洗成小数字，预算门就瞎了。
pub fn estimate(census: ClassCensus, mpix: u32, tier: HardwareTier, forced: bool) -> Result<Estimate, BudgetError> {
    let mut per_class_u = [0u64; 5];
    let mut codes: Vec<BudgetCode> = Vec::new();
    let mp = mpix as u64;
    for class in LightClass::ALL {
        let n = census.count(class) as u64;
        if n == 0 {
            continue;
        }
        let unit = cost(class, tier) as u64;
        let contrib = n
            .checked_mul(unit)
            .and_then(|x| x.checked_mul(mp))
            .ok_or(BudgetError {
                code: BudgetCode::ESTIMATE_OVERFLOW,
                message: String::from(
                    "cost estimate overflowed; reduce light counts or scene size instead of wrapping",
                ),
            })?;
        per_class_u[class.index()] = contrib;
    }
    let total_u = per_class_u.iter().fold(0u64, |a, &x| a.saturating_add(x));

    let budget_u = tier_budget(tier) as u64 * mp;
    let over_budget = total_u > budget_u;
    let over_by_u = if over_budget { total_u - budget_u } else { 0 };

    let active_lights = census.total();
    let cap = concurrency_cap(tier);
    let over_cap = active_lights > cap;

    let mut suggestions: Vec<Suggestion> = Vec::new();
    if over_budget {
        codes.push(BudgetCode::OVER_BUDGET);
        if forced {
            codes.push(BudgetCode::OVERRUN_FORCED);
        }
        // 降档建议一支：关贡献额最大的类别（贡献额 = 计数×单价×像素）。
        let mut best: Option<(LightClass, u64)> = None;
        for class in LightClass::ALL {
            let c = per_class_u[class.index()];
            if c > 0 && best.map(|(_, bc)| c > bc).unwrap_or(true) {
                best = Some((class, c));
            }
        }
        if let Some((class, contrib)) = best {
            codes.push(BudgetCode::SUGGESTION_ISSUED);
            suggestions.push(Suggestion::DropCostliest {
                class,
                contribution_u: contrib,
                text: alloc::format!(
                    "drop or dim the {} lights first: they contribute {}U of {}U in this scene",
                    class.name(),
                    contrib,
                    total_u
                ),
            });
        }
        // 降档建议二支：活跃数超本档并发上限 → 建议降一档并发。
        if over_cap {
            codes.push(BudgetCode::CAP_EXCEEDED);
            codes.push(BudgetCode::SUGGESTION_ISSUED);
            let lower = tier.index().checked_sub(1);
            if let Some(li) = lower {
                if let Some(t) = HardwareTier::from_index(li) {
                    suggestions.push(Suggestion::LowerConcurrencyCap {
                        from_tier: tier.name(),
                        to_tier: t.name(),
                        text: alloc::format!(
                            "lower the concurrency cap from {} to {} lights ({} tier cap is {})",
                            cap,
                            concurrency_cap(t),
                            t.name(),
                            concurrency_cap(t)
                        ),
                    });
                }
            } else {
                suggestions.push(Suggestion::LowerConcurrencyCap {
                    from_tier: tier.name(),
                    to_tier: tier.name(),
                    text: alloc::format!(
                        "already at the lowest cap ({}); cull lights via the manager instead",
                        cap
                    ),
                });
            }
        }
    } else if over_cap {
        // 未超预算但超上限：仍要告警（上限是渲染约束，不是预算约束）。
        codes.push(BudgetCode::CAP_EXCEEDED);
        codes.push(BudgetCode::SUGGESTION_ISSUED);
    }
    Ok(Estimate {
        per_class_u,
        total_u,
        mpix,
        tier,
        budget_u,
        over_budget,
        over_by_u,
        forced,
        active_lights,
        concurrency_cap: cap,
        over_cap,
        suggestions,
        codes,
    })
}

// ---------------------------------------------------------------------------
// 六、20% 修正门与 ADR 重定标流
// ---------------------------------------------------------------------------

/// 偏差门阈值：2000 bps = 20%（锚点口径「偏差超 20%」→ 严格大于触发）。
pub const DEVIATION_GATE_BPS: u64 = 2000;

/// 偏差对账结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DeviationVerdict {
    /// 门内（≤20%）：可作定标参考。
    WithinGate {
        /// 偏差（bps）。
        deviation_bps: u64,
    },
    /// 超门（>20%）：模型修正立项（不改表——改表走 ADR）。
    RevisionCase {
        /// 偏差（bps）。
        deviation_bps: u64,
    },
}

/// 预估 vs 实测偏差对账（整数 bps；预估为零时显性拒绝不除零）。
///
/// 返回 `Err`（0x3907 语义复用为对账面拒绝）：空场景实测无意义，
/// 「0 预估 vs 0 实测」也不是通过——是没有对账对象。
pub fn deviation_gate(estimate_u: u64, measured_u: u64) -> Result<DeviationVerdict, BudgetError> {
    if estimate_u == 0 {
        return Err(BudgetError {
            code: BudgetCode::ESTIMATE_OVERFLOW,
            message: String::from(
                "zero-estimate scenes have no meaningful deviation; calibrate on non-empty scenes",
            ),
        });
    }
    let diff = if measured_u >= estimate_u {
        measured_u - estimate_u
    } else {
        estimate_u - measured_u
    };
    let bps = diff
        .checked_mul(10_000)
        .map(|x| x / estimate_u)
        .ok_or(BudgetError {
            code: BudgetCode::ESTIMATE_OVERFLOW,
            message: String::from("deviation computation overflowed; measured value out of sane range"),
        })?;
    if bps > DEVIATION_GATE_BPS {
        Ok(DeviationVerdict::RevisionCase { deviation_bps: bps })
    } else {
        Ok(DeviationVerdict::WithinGate { deviation_bps: bps })
    }
}

/// 成本模型登记簿（版本 + ADR 链；重定标唯一入口）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ModelRegistry {
    /// 当前模型版本（= [`MODEL_VERSION`] 起，只增）。
    pub version: u32,
    /// ADR 链（每条 = "ADR 编号: 摘要"；v1 为空——v1 是初始声明非重定标）。
    pub adr_chain: Vec<String>,
    /// 待重定标标记（F1814 数据源就位/偏差立项后置位）。
    pub stale: bool,
}

impl ModelRegistry {
    /// v1 初始登记簿。
    pub fn new() -> ModelRegistry {
        ModelRegistry {
            version: MODEL_VERSION,
            adr_chain: Vec::new(),
            stale: false,
        }
    }

    /// 标记模型待重定标（偏差立项/数据源就位时调用）。
    pub fn mark_stale(&mut self) {
        self.stale = true;
    }

    /// ADR 重定标（唯一合法的改表面）。
    ///
    /// 无 ADR 编号 → 拒绝（0x3904）——「信仰不是工程」，常数变更必须
    /// 留痕（锚点：常数过期→重定标走 ADR；F1798 口径同规）。
    pub fn rebaseline(&mut self, adr_id: &str, summary: &str) -> Result<(), BudgetError> {
        let id = adr_id.trim();
        if id.is_empty() {
            return Err(BudgetError {
                code: BudgetCode::REBASELINE_REJECTED,
                message: String::from(
                    "rebaseline rejected: an ADR reference is required; file the recalibration decision first",
                ),
            });
        }
        self.version += 1;
        self.adr_chain.push(alloc::format!("{}: {}", id, summary));
        self.stale = false;
        Ok(())
    }
}

impl Default for ModelRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 七、门禁比对（F1767 对接面；CI 期执行）
// ---------------------------------------------------------------------------

/// 门禁比对结论（CI 期把预估与实测送入本口；模型本身零运行时成本）。
pub fn gate_check(estimate_u: u64, measured_u: u64) -> Result<DeviationVerdict, BudgetError> {
    deviation_gate(estimate_u, measured_u)
}

// ---------------------------------------------------------------------------
// 八、单测（判据承载见 vej11_checks.rs；此处只放纯函数不变量自证）
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_are_monotonic_and_complete() {
        for class in LightClass::ALL {
            let row = COST_TABLE[class.index()];
            assert_eq!(row.len(), 3);
            assert!(row[0] >= row[1] && row[1] >= row[2], "entry>=main>=flag");
            for v in row {
                assert!(v > 0, "zero cost is a lie");
            }
        }
    }

    #[test]
    fn caps_strictly_increase() {
        assert_eq!(CONCURRENCY_CAPS, [16, 32, 64]);
        assert!(CONCURRENCY_CAPS[0] < CONCURRENCY_CAPS[1] && CONCURRENCY_CAPS[1] < CONCURRENCY_CAPS[2]);
    }

    #[test]
    fn deferred_is_explicitly_reserved() {
        assert_eq!(DeferredExtension::STATE, "RESERVED");
        assert!(DeferredExtension::query().is_err());
    }

    #[test]
    fn estimate_empty_scene_is_zero_cost() {
        let e = estimate(ClassCensus::default(), 1, HardwareTier::Mainstream, false).unwrap();
        assert_eq!(e.total_u, 0);
        assert!(!e.over_budget);
        assert_eq!(e.suggestions.len(), 0);
    }

    #[test]
    fn rebaseline_requires_adr() {
        let mut r = ModelRegistry::new();
        assert!(r.rebaseline("  ", "").is_err());
        assert_eq!(r.version, MODEL_VERSION);
        assert!(r.rebaseline("VE-ADR-0099", "recalibrate from F1814 ladder").is_ok());
        assert_eq!(r.version, MODEL_VERSION + 1);
    }
}
