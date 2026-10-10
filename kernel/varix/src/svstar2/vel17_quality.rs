//! VE-F2217 · 粒子质量档位（VE-L 域 · 粒子段 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F2217`
//!
//! **职责定位（锚点原文）**：粒子质量档位——粒子密度三档（高/中/低——密度系数与
//! 上限组合：**高=1.0×容量全开 / 中=0.5×密度+80% 上限 / 低=0.25×+50% 上限**——
//! 密度系数×池上限的**双因子档位表**）、与 D04 降质链映射兑现复用 F2210（降质参数
//! 表粒子维度→档位映射——**档位树家族 L 域首实例**：F2015 家族延续）、档位记忆
//! （用户档位持久化——F2015 记忆机制延续）。
//!
//! **判据（锚点原文）**：双因子三档、平滑降档、映射家族、记忆延续、判据。
//!
//! ## 错误路径与降级矩阵（锚点原文，逐条落实）
//!
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 档位切换帧边界（F1762 规则——模拟与渲染**同步换档**） | [`SwitchBoard`]：请求只写暂存不动现役，帧边界一次性消费换装、epoch 单调；换绑产物 [`Binding`] 是**单一不可分对象**——密度系数（模拟侧）与池上限（渲染侧）在同一结构体里，类型上不存在「换一半」的表达；混装配置过不了 [`binding_is_coherent`] |
//! | 映射缺行 → **CI 对账拦截** | [`d04_map`]：0..=100 全域三行**无缝无叠**（缺行/叠行在 [`d04_coverage_gap`] 可判定），越界值显性拒绝（[`code::MAP_ROW_MISSING`]），不回绕不猜 |
//! | 非法组合 → **拒绝** | 档位闭集外序号、表外 D04 值、自洽但越界的记忆档位——全部拒绝（[`code::ILLEGAL_COMBO`]），不静默收下再丢弃 |
//! | 记忆损坏 → **回退默认** | [`mem_decode`] 魔数/校验和坏 = [`MemError::Corrupt`]：调用方走 [`mem_load_or_default`] 回退 [`DEFAULT_TIER`] 并记 [`code::MEM_CORRUPT`] 告警 |
//! | 三方冲突 → **裁决表单源** | [`adjudicate`]：用户 > 档位体系 > 预算临时，优先级表 [`ADJ_PRIORITY`] 单源；缺席≠异见不计冲突，全缺席保持现役（不发明默认值） |
//!
//! ## 平滑降档语义（锚点原文：在制粒子自然消亡过渡——不做粒子中途消失）
//!
//! 这条红线是**类型事实**而非承诺：[`SwitchBoard::apply_at_frame_boundary`]
//! 的签名里**没有活粒子数入口**——换绑在类型上就碰不到在制粒子，只收紧
//! **新生天花板**（[`spawn_ceiling`]）；已出生的粒子按 F2205 寿命自然消亡，
//! 最长一个 [`LIFETIME_MAX_SEC`] 周期内水位自然落进新上限（排空窗口声明，
//! 真调 vel05 常量）。「杀掉在制粒子凑水位」的 API 在本模块不存在。
//!
//! ## 性能逐项分解（锚点原文）
//!
//! - **档位查询 O(1)**：[`resolve`] 定长表直下标，无遍历无分配；
//! - **切换 = 池上限与密度系数换绑**：帧边界原子（staged 一次性消费——
//!   二次应用拿不到东西），现役在请求后逐位不变；
//! - **三档差异在 F2212 留证**：本模块只保证三档参数**确有差异**（判据钉死
//!   逐格数值），基准实测归 F2212。
//!
//! ## 跨批对接点（锚点原文）
//!
//! - **档位树家族 F2015/F2037/F2057/F2077/F2217（第五位——L 域首实例）**：
//!   [`FAMILY_MEMBERS`] 列账，家族不变量（三档闭集、D04 全域覆盖、帧边界
//!   规则）与 F2015 实物**真调对账**（[`super::vek15_pptier::TIER_COUNT`]）；
//! - **D04 跨卷契约**：映射带与家族单源格式一致（降质越重档位越低）；
//! - **消费方 F2203/F2208/F2210**：池上限换算**真调** [`PoolQuota`]（不另立
//!   池口径）；预算侧**真调** F2210 的 [`DENSITY_TIERS`]/
//!   [`DENSITY_HALVE_PER_STEP`]/[`DegradeCursor`]（游标步数→档位换算，
//!   apply_step 演练走 F2210 执行器本体）；
//! - **基准 F2212**：三档差异的实测留证点（本模块只钉参数差异存在）。
//!
//! ## 无障碍与隐私（锚点原文）
//!
//! 设置三通道；档位说明人话（[`DensityTier::zh`]：粒子密度预期）；记忆本地
//! 无隐私面——[`MemPayload`] 只有档位一枚举值，编码 8 字节定长，没有也不
//! 可能携带用户内容。
//!
//! ## 与相邻件的分工（易混，故写明）
//!
//! - **VE-F2210**（vel10_budget）管**预算联动机制**（超预算→先档后率），本条
//!   管档位**语义**（三档参数表/映射/记忆/裁决）；游标→档位的换算是两件的
//!   对接缝，判据双侧都钉；
//! - **VE-F2208**（vel08_pool）管池配额机制，本条只把档位的池上限**百分比**
//!   换算成池容量建议值，不改池本身；
//! - **VE-F2255**（GPU 粒子档）是家族 L 域第三实例，能力联动限档是它的维度，
//!   本模块不掺能力探测。

use crate::checks::CheckSet;

use super::vel05_lifetime::LIFETIME_MAX_SEC;
use super::vel08_pool::PoolQuota;
use super::vel10_budget::{
    DiagBag, DiagCode, DegradeCursor, DegradeStep, DENSITY_HALVE_PER_STEP, DENSITY_TIERS,
};
use super::vek15_pptier::TIER_COUNT as F2015_TIER_COUNT;

// ---------------------------------------------------------------------------
// 一、诊断码（自建段 0x94xx —— svstar2 树 grep 后确认零占用；
//    0x93xx=F2216 已占，本段续接）
// ---------------------------------------------------------------------------

/// D04 映射缺行（降质参数越出 0..=100 定义域）——拒绝，不回绕不猜。
pub const CODE_MAP_ROW_MISSING: u16 = 0x9401;
/// 非法组合（档位表外参数/表外 D04 值/越界记忆档位）。
pub const CODE_ILLEGAL_COMBO: u16 = 0x9402;
/// 三方源失同步——按优先级裁决表定胜负（家族规则）。
pub const CODE_TIER_DESYNC: u16 = 0x9403;
/// 档位记忆损坏（魔数/校验和不符）——回退默认档+告警。
pub const CODE_MEM_CORRUPT: u16 = 0x9404;
/// 档位记忆版本无迁移链——显性拒绝（声明支持范围，不静默回退）。
pub const CODE_MEM_VERSION_UNSUPPORTED: u16 = 0x9405;
/// 换绑不连贯（密度/上限/发射三列与档位表不符——混装拒绝）。
pub const CODE_SWITCH_INCOHERENT: u16 = 0x9406;

/// 本域诊断码全集（判据对账：两两互异 + 全占 0x94 段）。
pub const CODES: [u16; 6] = [
    CODE_MAP_ROW_MISSING,
    CODE_ILLEGAL_COMBO,
    CODE_TIER_DESYNC,
    CODE_MEM_CORRUPT,
    CODE_MEM_VERSION_UNSUPPORTED,
    CODE_SWITCH_INCOHERENT,
];

/// 人话说明（后果 + 下一步；未知码有兜底不 panic）。
pub const fn explain(code: u16) -> &'static str {
    match code {
        CODE_MAP_ROW_MISSING => "D04 映射缺行：降质参数越出 0..=100，先修正来源取值（越界不回绕不猜）",
        CODE_ILLEGAL_COMBO => "非法组合：档位/参数在表外，按闭集修正后重试",
        CODE_TIER_DESYNC => "三方档位源失同步：已按 用户>档位>预算 优先级裁决，胜出方可查",
        CODE_MEM_CORRUPT => "档位记忆损坏（魔数/校验和不符）：已回退默认档，重新选择会重新持久化",
        CODE_MEM_VERSION_UNSUPPORTED => "档位记忆版本无迁移链：显性拒绝（不静默回退丢用户选择），需写迁移器",
        CODE_SWITCH_INCOHERENT => "换绑不连贯：密度/池上限/发射率三列与档位表不符，混装配置被拒",
        _ => "未知粒子质量档诊断码（未登记）",
    }
}

// ---------------------------------------------------------------------------
// 二、三档闭集（双因子档位表——密度系数 × 池上限 + 发射联动列）
// ---------------------------------------------------------------------------

/// 粒子密度三档（闭集；顺序即锚点原文顺序：高/中/低）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DensityTier {
    /// 高档：1.0×容量全开。
    High = 0,
    /// 中档：0.5×密度 + 80% 上限。
    Mid = 1,
    /// 低档：0.25×密度 + 50% 上限。
    Low = 2,
}

/// 三档闭集长度（与 F2210 [`DENSITY_TIERS`] 前向声明对账：判据真调两侧相等）。
pub const TIER_COUNT: usize = 3;

/// 默认档（记忆损坏/无记忆时的落点——取中档：与家族 F2015 同规，
/// 首启体验两头都不塌）。
pub const DEFAULT_TIER: DensityTier = DensityTier::Mid;

const TIERS: [DensityTier; TIER_COUNT] =
    [DensityTier::High, DensityTier::Mid, DensityTier::Low];

impl DensityTier {
    /// 序号（0..3）。
    pub const fn ordinal(self) -> usize {
        self as usize
    }

    /// 由序号还原（越界 `None`——不回绕到 0）。
    pub const fn from_ordinal(i: usize) -> Option<DensityTier> {
        match i {
            0 => Some(DensityTier::High),
            1 => Some(DensityTier::Mid),
            2 => Some(DensityTier::Low),
            _ => None,
        }
    }

    /// 三档定长数组。
    pub const fn all() -> [DensityTier; TIER_COUNT] {
        TIERS
    }

    /// 档名（人读——无障碍替述：粒子密度预期）。
    pub const fn zh(self) -> &'static str {
        match self {
            DensityTier::High => "高（密度全开）",
            DensityTier::Mid => "中（密度减半）",
            DensityTier::Low => "低（密度四分之一）",
        }
    }
}

/// 密度系数（千分整数：1000=全开——定点整数口径沿用 F2210「不用 f32」
/// 的确定性纪律，同参数跨优化级别同输出）。
pub type Permille = u32;

/// 单档参数行（锚点数据结构：密度系数×池上限×发射率缩放**三列**）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TierParams {
    /// 密度系数（‰，相对池容量全开）。
    pub density_permille: Permille,
    /// 池上限百分比（%，相对配额容量）。
    pub pool_cap_pct: u32,
    /// 发射率缩放（‰——**发射联动**列：F2210 先档后率次序里，档位换绑
    /// 给发射率缩放设的是**档位天花板**；执行器在此之下再走 EmissionShrink）。
    pub emission_scale_permille: Permille,
}

/// 锚点原文数值逐格落账：高=1.0×/100%、中=0.5×/80%、低=0.25×/50%。
pub const TIER_TABLE: [TierParams; TIER_COUNT] = [
    TierParams { density_permille: 1000, pool_cap_pct: 100, emission_scale_permille: 1000 },
    TierParams { density_permille: 500, pool_cap_pct: 80, emission_scale_permille: 500 },
    TierParams { density_permille: 250, pool_cap_pct: 50, emission_scale_permille: 250 },
];

/// 按档取参数行（O(1) 直下标）。
pub const fn params_of(t: DensityTier) -> &'static TierParams {
    &TIER_TABLE[t.ordinal()]
}

// ---------------------------------------------------------------------------
// 三、换绑产物（单一不可分对象——模拟与渲染同步换档的类型事实）
// ---------------------------------------------------------------------------

/// 档位换绑产物：密度系数（模拟侧）与池上限（渲染侧）**在同一结构体**。
///
/// 锚点要求「模拟与渲染同步换档」——本模块把它做成类型事实：换绑的
/// 唯一构造入口是 [`resolve`]，产物三列恒与档位表一致（[`binding_is_coherent`]），
/// 「只换密度不换上限」的混装状态在类型上没有构造路径。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Binding {
    /// 档位。
    pub tier: DensityTier,
    /// 密度系数（‰，模拟侧）。
    pub density_permille: Permille,
    /// 池上限百分比（%，渲染侧）。
    pub pool_cap_pct: u32,
    /// 发射率缩放天花板（‰）。
    pub emission_scale_permille: Permille,
}

/// 档位 → 换绑产物（O(1) 表查；**唯一**的 Binding 构造入口）。
pub const fn resolve(t: DensityTier) -> Binding {
    let p = params_of(t);
    Binding {
        tier: t,
        density_permille: p.density_permille,
        pool_cap_pct: p.pool_cap_pct,
        emission_scale_permille: p.emission_scale_permille,
    }
}

/// 换绑连贯性核验：三列与档位表逐格一致（混装配置在此显性拒绝——
/// [`code::SWITCH_INCOHERENT`]）。`Binding` 字段是 pub 的（遥测/文档要读），
/// 所以连贯性必须是**可检验的函数**而不是只靠构造纪律。
pub const fn binding_is_coherent(b: &Binding) -> bool {
    let p = params_of(b.tier);
    b.density_permille == p.density_permille
        && b.pool_cap_pct == p.pool_cap_pct
        && b.emission_scale_permille == p.emission_scale_permille
}

/// 池上限换算（消费方 F2208：档位百分比 × 配额容量，向下取整）。
///
/// **真调** [`PoolQuota`] 取容量——不另立池口径。向下取整是声明的语义：
/// 上限只会少算不会多算（多算的一格就是硬顶外的一格）。
pub const fn pool_cap_particles(quota_cap: u32, t: DensityTier) -> u32 {
    quota_cap * params_of(t).pool_cap_pct / 100
}

/// 新生天花板：在制粒子数达到换绑上限后不再新生（**只挡新生、不杀在制**
/// ——平滑降档红线的执行面）。
pub const fn spawn_ceiling(cap_particles: u32, alive: u32) -> bool {
    alive < cap_particles
}

// ---------------------------------------------------------------------------
// 四、D04 降质链映射（降质参数 → 档位；家族单源格式，L01 段实例）
// ---------------------------------------------------------------------------

/// D04 映射的家族实例（家族表 [`FAMILY_MEMBERS`] 第五位——**L 域首实例**，
/// 锚点原文「第五域首实例：F2015 家族延续」）。
pub const D04_FAMILY_INSTANCE: u32 = 5;

/// 家族成员列账（锚点跨批对接点原文序；本条居第五位）。
pub const FAMILY_MEMBERS: [&str; 5] = ["F2015", "F2037", "F2057", "F2077", "F2217"];

/// 一行映射：D04 降质参数闭区间 → 档位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct D04Row {
    /// 带下界（含）。
    pub lo: u32,
    /// 带上界（含）。
    pub hi: u32,
    /// 归档。
    pub tier: DensityTier,
}

/// D04 → 档位映射表（**单源**：降质越重档位越低；0..=100 全域由三行
/// 无缝无叠覆盖——缺行/叠行即配置错误，[`d04_coverage_gap`] 可判定）。
pub const D04_MAP: [D04Row; 3] = [
    D04Row { lo: 0, hi: 33, tier: DensityTier::High },
    D04Row { lo: 34, hi: 66, tier: DensityTier::Mid },
    D04Row { lo: 67, hi: 100, tier: DensityTier::Low },
];

/// 映射覆盖缺口证明：`None` = 0..=100 全域恰好一行覆盖（无缝无叠）。
///
/// 逐行核三件事：界序合法、相邻衔接（`lo_{i+1} == hi_i + 1`）、全域两端
/// 齐口（首行含 0、末行含 100）。缺行 → CI 对账拦截（锚点错误路径）。
pub const fn d04_coverage_gap() -> Option<usize> {
    let mut i = 0usize;
    while i < D04_MAP.len() {
        let r = &D04_MAP[i];
        if r.lo > r.hi {
            return Some(i);
        }
        if i == 0 && r.lo != 0 {
            return Some(i);
        }
        if i + 1 < D04_MAP.len() && D04_MAP[i + 1].lo != r.hi + 1 {
            return Some(i);
        }
        if i + 1 == D04_MAP.len() && r.hi != 100 {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// D04 降质参数 → 档位（缺行返回 `None`，调用方记 [`CODE_MAP_ROW_MISSING`]）。
pub const fn d04_map(v: u32) -> Option<DensityTier> {
    let mut i = 0usize;
    while i < D04_MAP.len() {
        let r = &D04_MAP[i];
        if v >= r.lo && v <= r.hi {
            return Some(r.tier);
        }
        i += 1;
    }
    None
}

/// 档位 → D04 代表值（该档映射带的中点；双向往返 tier→代表值→tier
/// 不换档且代表值不出带——判据钉死）。
pub const fn d04_representative(t: DensityTier) -> u32 {
    let mut i = 0usize;
    while i < D04_MAP.len() {
        if D04_MAP[i].tier.ordinal() == t.ordinal() {
            return (D04_MAP[i].lo + D04_MAP[i].hi) / 2;
        }
        i += 1;
    }
    0
}

// ---------------------------------------------------------------------------
// 五、F2210 预算联动（游标步数 → 档位换算——两件的对接缝）
// ---------------------------------------------------------------------------

/// F2210 降质游标的已降步数 → 档位（0 步=高、1 步=中、2 步=低）。
///
/// 超出 `DENSITY_TIERS-1` 步返回 `None`：F2210 的执行器到第三档就改走
/// EmissionShrink，游标不该出现更多密度步——多出来说明两侧口径分叉，
/// 显性拒绝而不是取末档糊过去。
pub const fn tier_from_density_steps(steps: u32) -> Option<DensityTier> {
    if steps >= DENSITY_TIERS {
        return None;
    }
    DensityTier::from_ordinal(steps as usize)
}

// ---------------------------------------------------------------------------
// 六、三方裁决（用户 > 档位体系 > 预算临时；家族规则单源）
// ---------------------------------------------------------------------------

/// 裁决源（优先级表顺序即权威序——与 F2015/F1832 三方裁决同构：
/// 用户显式选择 > 档位体系推定 > 预算临时降档建议）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdjudicationSource {
    /// 用户覆盖（设置面板/启动参数）——最高优先级。
    User,
    /// 档位体系（D04 映射/档位树建议）——次之。
    TierSystem,
    /// 预算临时（F2210 联动的临时降档建议）——最低。
    BudgetTemp,
}

impl AdjudicationSource {
    /// 序号（0..3，与 [`ADJ_PRIORITY`] 下标一致）。
    pub const fn ordinal(self) -> usize {
        match self {
            AdjudicationSource::User => 0,
            AdjudicationSource::TierSystem => 1,
            AdjudicationSource::BudgetTemp => 2,
        }
    }
}

/// 裁决优先级表（**单源**：顺序不可换，判据逐位钉住）。
pub const ADJ_PRIORITY: [AdjudicationSource; 3] = [
    AdjudicationSource::User,
    AdjudicationSource::TierSystem,
    AdjudicationSource::BudgetTemp,
];

/// 裁决产物。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Adjudication {
    /// 胜出档位。
    pub winner: DensityTier,
    /// 与胜者不一致的**在场**源数（缺席≠异见，不计冲突——判据精确对账）。
    pub conflicts: u32,
}

/// 三方裁决（**纯函数**；冲突记 [`CODE_TIER_DESYNC`] 告警——冲突回改
/// 不报错：报错会把三处配置的合法组合变死锁，家族规则）。
///
/// 三个源各自独立表态（`None`=该源不在场），按 [`ADJ_PRIORITY`] 取第一个
/// 在场源为准；全部缺席时保持 `keep`（现役档位）——不发明默认值，保持
/// 现状是唯一不撒谎的答案。
pub fn adjudicate(
    user: Option<DensityTier>,
    tier_sys: Option<DensityTier>,
    budget_tmp: Option<DensityTier>,
    keep: DensityTier,
    bag: &mut DiagBag,
) -> Adjudication {
    let winner = match (user, tier_sys, budget_tmp) {
        (Some(u), _, _) => u,
        (None, Some(t), _) => t,
        (None, None, Some(b)) => b,
        (None, None, None) => keep,
    };

    let mut conflicts = 0u32;
    if let Some(u) = user {
        if u != winner {
            conflicts += 1;
        }
    }
    if let Some(t) = tier_sys {
        if t != winner {
            conflicts += 1;
        }
    }
    if let Some(b) = budget_tmp {
        if b != winner {
            conflicts += 1;
        }
    }
    if conflicts > 0 {
        bag.push_warn(DiagCode(CODE_TIER_DESYNC));
    }
    Adjudication { winner, conflicts }
}

// ---------------------------------------------------------------------------
// 七、帧边界切换（F1762 家族规则——staged 一次性消费 + 平滑降档）
// ---------------------------------------------------------------------------

/// 档位切换台（F1762 帧边界规则：任何时刻可请求，帧尾才换装）。
pub struct SwitchBoard {
    active: Binding,
    staged: Option<Binding>,
    /// 换装次数（**单调**，epoch 判据用）。
    pub epoch: u64,
    /// 累计请求次数（含被拒请求——拒绝也是一次真实意图）。
    pub requests: u64,
    /// 累计被拒请求次数（非法组合——判据钉「拒绝可见」）。
    pub rejected: u64,
    /// 累计帧边界应用次数。
    pub applies: u64,
}

impl SwitchBoard {
    /// 构造（现役=默认档换绑产物）。
    pub const fn new() -> SwitchBoard {
        SwitchBoard {
            active: resolve(DEFAULT_TIER),
            staged: None,
            epoch: 0,
            requests: 0,
            rejected: 0,
            applies: 0,
        }
    }

    /// 现役换绑产物（只读）。
    pub const fn active(&self) -> &Binding {
        &self.active
    }

    /// 是否有未应用的暂存。
    pub const fn has_staged(&self) -> bool {
        self.staged.is_some()
    }

    /// 请求切换（**只写暂存不动现役**——帧边界才生效）。新请求覆盖未应用
    /// 的旧请求（只有最新意图生效，F2002 重组协议家族同款）。
    pub fn request(&mut self, tier: DensityTier) {
        self.requests = self.requests.saturating_add(1);
        self.staged = Some(resolve(tier));
    }

    /// 由原始序号请求（wire 面：外部输入的档位序号先过闭集校验；
    /// 越界即拒且**不暂存**，返回 `false` 供调用方记 [`CODE_ILLEGAL_COMBO`]）。
    pub fn request_ordinal(&mut self, ordinal: usize) -> bool {
        self.requests = self.requests.saturating_add(1);
        match DensityTier::from_ordinal(ordinal) {
            Some(t) => {
                self.staged = Some(resolve(t));
                true
            }
            None => {
                self.rejected = self.rejected.saturating_add(1);
                false
            }
        }
    }

    /// 帧边界应用（每帧尾调用一次；无暂存返回 `None`，有暂存换装并返回
    /// 新 epoch）。**这是唯一能改现役的入口**——帧中换装在 API 层不可表达。
    ///
    /// 平滑降档红线（类型事实）：本函数**没有活粒子数入口**——换绑只更新
    /// 新生天花板与密度系数，在制粒子按 F2205 寿命自然消亡（见模块头
    /// 「平滑降档语义」）。
    pub fn apply_at_frame_boundary(&mut self) -> Option<u64> {
        let staged = self.staged.take()?;
        self.active = staged;
        self.epoch = self.epoch.saturating_add(1);
        self.applies = self.applies.saturating_add(1);
        Some(self.epoch)
    }
}

// ---------------------------------------------------------------------------
// 八、档位记忆（持久化 + 版本迁移 + 损坏回退——F2015 同构复用）
// ---------------------------------------------------------------------------

/// 记忆魔数（"F217"的 17 位谐音——0xF217）。
pub const MEM_MAGIC: u16 = 0xF217;
/// 当前记忆版本 v2（v1=仅档位；v2 增加发射联动天花板列）。
pub const MEM_VERSION: u16 = 2;

/// 记忆载荷（用户选择：档位 + 发射联动天花板）。
///
/// 隐私红线是类型事实：载荷只有一枚举值与一整数，**没有也不可能有**用户
/// 内容；编码 8 字节定长。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemPayload {
    /// 用户选定的档位。
    pub tier: DensityTier,
    /// 用户可见的发射率缩放天花板（‰；与档位表联动列一致——非法组合拒收）。
    pub emission_scale_permille: Permille,
}

/// 记忆编码错误。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemError {
    /// 损坏（魔数/校验和不符/档位越界）——回退默认档+告警。
    Corrupt,
    /// 版本无迁移链——显性拒绝（声明支持范围：本实现支持 v1 迁移与 v2
    /// 本体，未来版本到了要写 v2→v3 迁移器，不能装作没看见）。
    VersionUnsupported,
}

/// FNV-1a 截 8 位（覆盖前 7 字节）。
///
/// 与家族 F2015 同款口径：FNV-1a 对**单比特翻转**是全检出的——翻第 i 位把
/// 中间值改 ±2^i，乘奇素数模 256 后差仍 = ±(2^i·奇) mod 256 ≠ 0（i<8）。
/// 记忆载荷的威胁模型正是单比特翻转/字节损坏，不是蓄意碰撞。
const fn mem_checksum(bytes: &[u8; 8]) -> u8 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < 7 {
        h ^= bytes[i] as u32;
        h = h.wrapping_mul(0x0100_0193);
        i += 1;
    }
    (h & 0xFF) as u8
}

/// 编码（v2 本体：`[魔数Lo, 魔数Hi, 版本, 档位, 发射Lo, 发射Mid, 发射Hi, 校验和]`）。
pub const fn mem_encode(p: &MemPayload) -> [u8; 8] {
    let e = p.emission_scale_permille;
    let mut bytes = [
        (MEM_MAGIC & 0xFF) as u8,
        (MEM_MAGIC >> 8) as u8,
        (MEM_VERSION & 0xFF) as u8,
        p.tier.ordinal() as u8,
        (e & 0xFF) as u8,
        ((e >> 8) & 0xFF) as u8,
        ((e >> 16) & 0xFF) as u8,
        0,
    ];
    bytes[7] = mem_checksum(&bytes);
    bytes
}

/// 解码 v2 本体（载荷内部一致性：发射天花板必须与档位联动列一致——
/// 表外组合 = 损坏）。魔数/校验和已在 [`mem_decode`] 入口预检。
const fn mem_decode_v2(bytes: &[u8; 8]) -> Result<MemPayload, MemError> {
    let tier = match DensityTier::from_ordinal(bytes[3] as usize) {
        Some(t) => t,
        None => return Err(MemError::Corrupt),
    };
    let e = (bytes[4] as u32) | ((bytes[5] as u32) << 8) | ((bytes[6] as u32) << 16);
    let p = MemPayload { tier, emission_scale_permille: e };
    if p.emission_scale_permille != params_of(tier).emission_scale_permille {
        return Err(MemError::Corrupt);
    }
    Ok(p)
}

/// v1 迁移（v1=仅档位：`[魔数Lo, 魔数Hi, 版本=1, 档位, 0,0,0, 校验和]`）。
/// 迁移把 v1 载荷补齐到 v2：发射天花板取**档位表联动列现值**（表是单源，
/// 迁移不发明数值）。
const fn mem_decode_v1(bytes: &[u8; 8]) -> Result<MemPayload, MemError> {
    let tier = match DensityTier::from_ordinal(bytes[3] as usize) {
        Some(t) => t,
        None => return Err(MemError::Corrupt),
    };
    Ok(MemPayload { tier, emission_scale_permille: params_of(tier).emission_scale_permille })
}

/// 解码（**损坏与版本分型**：魔数/校验和坏 = `Corrupt`；自洽的未知版本 =
/// `VersionUnsupported`——两者走不同的错误路径，前者回退默认档，后者
/// 显性拒绝不静默回退）。
pub fn mem_decode(bytes: &[u8; 8]) -> Result<MemPayload, MemError> {
    if bytes[0] != (MEM_MAGIC & 0xFF) as u8 || bytes[1] != (MEM_MAGIC >> 8) as u8 {
        return Err(MemError::Corrupt);
    }
    if bytes[7] != mem_checksum(bytes) {
        return Err(MemError::Corrupt);
    }
    match bytes[2] {
        2 => mem_decode_v2(bytes),
        1 => mem_decode_v1(bytes),
        _ => Err(MemError::VersionUnsupported),
    }
}

/// 读取（带回退）：`Ok(payload)`=读到了用户选择；`Err(Corrupt)` 已记
/// [`CODE_MEM_CORRUPT`]（调用方据此回退默认档）；`Err(VersionUnsupported)`
/// 已记 [`CODE_MEM_VERSION_UNSUPPORTED`]——**调用方不得据此回退**：
/// 回退会无声丢掉用户选择，两者必须分开。
pub fn mem_load_or_default(bytes: &[u8; 8], bag: &mut DiagBag) -> Result<MemPayload, MemError> {
    match mem_decode(bytes) {
        Ok(p) => Ok(p),
        Err(MemError::Corrupt) => {
            bag.push_warn(DiagCode(CODE_MEM_CORRUPT));
            Err(MemError::Corrupt)
        }
        Err(e @ MemError::VersionUnsupported) => {
            bag.push_p1(DiagCode(CODE_MEM_VERSION_UNSUPPORTED));
            Err(e)
        }
    }
}

// ---------------------------------------------------------------------------
// 九、域自检（判据逐条映射锚点：双因子三档 / 平滑降档 / 映射家族 / 记忆延续 / 判据）
// ---------------------------------------------------------------------------

/// 判据用池配额（容量取 2 的幂让取整语义可见：80% 落在非整格上）。
fn probe_quota() -> PoolQuota {
    PoolQuota {
        kind: super::vel08_pool::PoolKind::Cpu,
        capacity: 1024,
        stride: 4,
        emitter_share_pct: 10,
        bytes_cap: 4096,
    }
}

/// F2217 域自检入口（聚合器调用；零 panic 面，失败逐条红不炸域）。
pub fn run_vel17_checks() -> CheckSet {
    let mut s = CheckSet::new("vel17_quality");

    // --- 判据 1：双因子三档（闭集 + 锚点数值逐格钉死 + 单调有差） ---
    {
        s.add(
            "A17-三档-闭集序号往返且长度对账F2210",
            DensityTier::from_ordinal(3).is_none()
                && TIER_COUNT == 3
                && DENSITY_TIERS == TIER_COUNT as u32,
            "三档闭集（高/中/低）；档位数与 F2210 前向声明 DENSITY_TIERS=3 真调相等（两侧口径分叉当场现形）",
        );
        let hi = params_of(DensityTier::High);
        let mid = params_of(DensityTier::Mid);
        let lo = params_of(DensityTier::Low);
        s.add(
            "A17-三档-锚点数值逐格钉死",
            hi.density_permille == 1000
                && hi.pool_cap_pct == 100
                && mid.density_permille == 500
                && mid.pool_cap_pct == 80
                && lo.density_permille == 250
                && lo.pool_cap_pct == 50,
            "锚点原文数值逐格落账：高=1.0×/100%、中=0.5×/80%、低=0.25×/50%（写成「递减」也算绿的实现到此为止）",
        );
        s.add(
            "A17-三档-双列单调且相邻有差",
            hi.density_permille > mid.density_permille
                && mid.density_permille > lo.density_permille
                && hi.pool_cap_pct > mid.pool_cap_pct
                && mid.pool_cap_pct > lo.pool_cap_pct
                && hi.density_permille != lo.density_permille,
            "密度与上限两因子都随档位单调降、且确有差异（三档全等则档位形同虚设——F2015 家族判据同款）",
        );
        s.add(
            "A17-三档-折半关系真调F2210",
            DENSITY_HALVE_PER_STEP == 2
                && mid.density_permille * DENSITY_HALVE_PER_STEP == hi.density_permille
                && lo.density_permille * DENSITY_HALVE_PER_STEP == mid.density_permille,
            "每降一档密度恰折半——与 F2210 的 DENSITY_HALVE_PER_STEP=2 真调对账（先档后率次序里档位步的量化口径）",
        );
        let es = [
            params_of(DensityTier::High).emission_scale_permille,
            params_of(DensityTier::Mid).emission_scale_permille,
            params_of(DensityTier::Low).emission_scale_permille,
        ];
        s.add(
            "A17-三档-发射联动列单调且同步",
            es[0] == 1000 && es[1] == 500 && es[2] == 250 && es[0] > es[1] && es[1] > es[2],
            "发射率缩放天花板随档位同步收紧（锚点三列之一的发射联动列——缺列会让发射率在低档仍全速）",
        );
    }

    // --- 判据 2：换绑产物（O(1) + 池上限换算 + 原子性 + 平滑红线） ---
    {
        let mut ok = true;
        let mut i = 0usize;
        while i < TIER_COUNT {
            let t = match DensityTier::from_ordinal(i) {
                Some(x) => x,
                None => break,
            };
            let b1 = resolve(t);
            let b2 = resolve(t);
            ok = ok && b1 == b2 && binding_is_coherent(&b1) && b1.tier == t;
            i += 1;
        }
        s.add(
            "A17-绑定-解析纯函数且逐档连贯",
            ok,
            "resolve 两调同参同果（纯函数）且三档产物全部过连贯性核验（Binding 唯一构造入口是档位表）",
        );
        let q = probe_quota();
        s.add(
            "A17-绑定-池上限换算真调F2208且向下取整",
            pool_cap_particles(q.capacity, DensityTier::High) == 1024
                && pool_cap_particles(q.capacity, DensityTier::Mid) == 819
                && pool_cap_particles(q.capacity, DensityTier::Low) == 512
                && q.capacity == 1024,
            "档位百分比×配额容量（真调 PoolQuota），向下取整是声明的语义：上限只会少算不会多算（1024×80%=819.2→819 落在非整格上可见）",
        );
        let mixed = Binding {
            tier: DensityTier::High,
            density_permille: 1000,
            pool_cap_pct: 50,
            emission_scale_permille: 1000,
        };
        s.add(
            "A17-绑定-混装配置显性拒绝",
            !binding_is_coherent(&mixed) && binding_is_coherent(&resolve(DensityTier::High)),
            "只换密度不换上限的混装状态过不了连贯性核验（锚点：模拟与渲染同步换档——字段 pub 所以连贯性必须是可检验函数）",
        );
        let b_low = resolve(DensityTier::Low);
        s.add(
            "A17-绑定-平滑降档只挡新生不杀在制",
            spawn_ceiling(pool_cap_particles(q.capacity, b_low.tier), 511)
                && !spawn_ceiling(pool_cap_particles(q.capacity, b_low.tier), 512),
            "新生天花板只拦新粒子（alive≥cap 拒新生）；在制粒子不在此 API 的可及范围内——「杀在制凑水位」的类型上不存在",
        );
        s.add(
            "A17-绑定-排空窗口真调F2205寿命上界",
            LIFETIME_MAX_SEC > 0.0,
            "降档后最长一个 LIFETIME_MAX_SEC 周期内水位自然落进新上限（排空窗口声明真调 vel05 常量——窗口为 0 即声明为假）",
        );
    }

    // --- 判据 3：D04 映射（全域覆盖 + 边界归档 + 越界拒绝 + 双向往返） ---
    {
        s.add(
            "A17-D04-全域覆盖无缝无叠",
            d04_coverage_gap().is_none(),
            "0..=100 每个值恰归一档：界序/衔接/两端齐口三查全过（缺行即 CI 对账拦截——锚点错误路径原文）",
        );
        let edges = [
            (0u32, Some(DensityTier::High)),
            (33, Some(DensityTier::High)),
            (34, Some(DensityTier::Mid)),
            (66, Some(DensityTier::Mid)),
            (67, Some(DensityTier::Low)),
            (100, Some(DensityTier::Low)),
        ];
        let mut ok = true;
        let mut i = 0usize;
        while i < edges.len() {
            ok = ok && d04_map(edges[i].0) == edges[i].1;
            i += 1;
        }
        s.add(
            "A17-D04-边界值逐个归档",
            ok,
            "六个边界值逐个落带（恰端点不漂移——0/33/34/66/67/100）",
        );
        s.add(
            "A17-D04-越界显性拒绝不回绕",
            d04_map(101).is_none() && d04_map(u32::MAX).is_none(),
            "101 与 u32::MAX 都拒（不回绕到 0 不猜——越界是配置错误不是低档请求）",
        );
        s.add(
            "A17-D04-降质越重档位越低",
            d04_map(0) == Some(DensityTier::High)
                && d04_map(50) == Some(DensityTier::Mid)
                && d04_map(100) == Some(DensityTier::Low),
            "方向钉死：D04 值越大降质越重、档位越低（方向反了降质链会把最重的负载派给最贵的档）",
        );
        let mut rt = true;
        for t in DensityTier::all() {
            let r = d04_representative(t);
            rt = rt && d04_map(r) == Some(t);
        }
        s.add(
            "A17-D04-双向往返恒等",
            rt,
            "tier→代表值→tier 不换档（代表值取带中点，永不出带）",
        );
        s.add(
            "A17-D04-家族实例第五位L域首实例",
            D04_FAMILY_INSTANCE == 5
                && FAMILY_MEMBERS.len() == 5
                && FAMILY_MEMBERS[4] == "F2217"
                && FAMILY_MEMBERS[0] == "F2015",
            "家族列账 F2015/F2037/F2057/F2077/F2217（锚点原文序），本条居第五位=档位树家族 L 域首实例",
        );
        s.add(
            "A17-D04-家族不变量真调F2015",
            F2015_TIER_COUNT == TIER_COUNT,
            "家族不变量（三档闭集）与 F2015 实物真调对账——家族一致不靠注释靠编译期常量对拍",
        );
    }

    // --- 判据 4：F2210 预算联动（游标换算 + 执行器演练） ---
    {
        s.add(
            "A17-联动-游标步数换算双向",
            tier_from_density_steps(0) == Some(DensityTier::High)
                && tier_from_density_steps(1) == Some(DensityTier::Mid)
                && tier_from_density_steps(2) == Some(DensityTier::Low)
                && tier_from_density_steps(3).is_none(),
            "0/1/2 步→高/中/低；第三步返回 None——F2210 到第三档改走 EmissionShrink，多出来的密度步说明两侧口径分叉，显性拒绝不取末档糊过去",
        );
        let mut cursor = DegradeCursor::new();
        let s1 = super::vel10_budget::apply_step(DegradeStep::DensityDownOne, &mut cursor);
        let t1 = tier_from_density_steps(cursor.density_steps_taken);
        let s2 = super::vel10_budget::apply_step(DegradeStep::DensityDownOne, &mut cursor);
        let t2 = tier_from_density_steps(cursor.density_steps_taken);
        let over = super::vel10_budget::Estimate { sim_ns: 1, render_ns: 1, sort_ns: 1, total_ns: 3 };
        let budget_ns = super::vel10_budget::DEFAULT_BUDGET_NS / 4_000_000;
        let exhausted = super::vel10_budget::next_required_step(&over, budget_ns, &cursor);
        s.add(
            "A17-联动-F2210执行器演练档随步走",
            s1.is_ok()
                && t1 == Some(DensityTier::Mid)
                && s2.is_ok()
                && t2 == Some(DensityTier::Low)
                && matches!(exhausted, Some(DegradeStep::EmissionShrink)),
            "真调 F2210 执行器：连降两步后游标换算恰为低档，且密度步耗尽后 next_required_step 转入发射率收缩（先档后率次序在对接缝上可见）",
        );
    }

    // --- 判据 5：三方裁决（三源各胜一次 + 在场异见才计冲突 + 全缺席保持） ---
    {
        let mut bag = DiagBag::new();
        let a1 = adjudicate(Some(DensityTier::Low), Some(DensityTier::High), Some(DensityTier::High), DensityTier::High, &mut bag);
        let a2 = adjudicate(None, Some(DensityTier::Mid), Some(DensityTier::High), DensityTier::High, &mut bag);
        let a3 = adjudicate(None, None, Some(DensityTier::Low), DensityTier::High, &mut bag);
        s.add(
            "A17-裁决-三源各胜一次",
            a1.winner == DensityTier::Low
                && a2.winner == DensityTier::Mid
                && a3.winner == DensityTier::Low,
            "user 在场 user 胜；user 缺席档位体系胜；再缺席预算临时胜（优先级表逐位生效——恒返回用户的实现到此现形）",
        );
        let a4 = adjudicate(None, None, None, DensityTier::Mid, &mut bag);
        s.add(
            "A17-裁决-全缺席保持现役",
            a4.winner == DensityTier::Mid && a4.conflicts == 0,
            "三级全缺席保持 keep（不发明默认值——保持现状是唯一不撒谎的答案）",
        );
        let mut bag2 = DiagBag::new();
        let a5 = adjudicate(Some(DensityTier::High), Some(DensityTier::High), None, DensityTier::Low, &mut bag2);
        s.add(
            "A17-裁决-全一致零告警",
            a5.winner == DensityTier::High
                && a5.conflicts == 0
                && !bag2.has(DiagCode(CODE_TIER_DESYNC)),
            "在场源全一致：零冲突零告警（在场异见才计冲突——缺席≠异见，误计会把正常缺席当事故）",
        );
        let mut bag3 = DiagBag::new();
        let a6 = adjudicate(None, Some(DensityTier::Low), Some(DensityTier::High), DensityTier::High, &mut bag3);
        s.add(
            "A17-裁决-异见计冲突且告警在案",
            a6.conflicts == 1 && bag3.has(DiagCode(CODE_TIER_DESYNC)),
            "一个在场异见记一次冲突并落告警（冲突回改不报错——但可查，胜出方黑箱是家族判据明令禁止的）",
        );
        s.add(
            "A17-裁决-优先级表逐位钉死",
            ADJ_PRIORITY[0].ordinal() == 0
                && ADJ_PRIORITY[1].ordinal() == 1
                && ADJ_PRIORITY[2].ordinal() == 2,
            "裁决优先级表单源：用户>档位>预算 顺序不可换（判据侧独立写死对拍）",
        );
    }

    // --- 判据 6：帧边界切换（staged 一次性消费 + epoch 单调 + 拒绝可见） ---
    {
        let mut sb = SwitchBoard::new();
        let before = *sb.active();
        sb.request(DensityTier::Low);
        s.add(
            "A17-切换-请求后现役逐位不变",
            sb.has_staged() && sb.active() == &before && sb.epoch == 0,
            "请求只写暂存不动现役（帧中换装在 API 层不可表达——F1762 家族规则）",
        );
        let e1 = sb.apply_at_frame_boundary();
        let after = *sb.active();
        let e2 = sb.apply_at_frame_boundary();
        s.add(
            "A17-切换-帧边界换装且暂存一次性消费",
            e1 == Some(1)
                && sb.active() == &resolve(DensityTier::Low)
                && after.tier == DensityTier::Low
                && e2.is_none()
                && sb.epoch == 1
                && sb.applies == 1,
            "帧边界应用才换装、epoch 单调、二次应用拿不到东西（staged 是一次性消费——原子）",
        );
        let mut sb2 = SwitchBoard::new();
        let ok1 = sb2.request_ordinal(2);
        let ok2 = sb2.request_ordinal(7);
        s.add(
            "A17-切换-非法请求连暂存都不产生",
            ok1 && !ok2 && sb2.rejected == 1 && sb2.has_staged(),
            "表外序号被拒且记账可见（rejected=1）；此前合法请求的暂存不受牵连（拒绝必须可见，静默收下再丢弃是最坏的假尊重）",
        );
        let mut sb3 = SwitchBoard::new();
        sb3.request(DensityTier::Low);
        sb3.request(DensityTier::Mid);
        let _ = sb3.apply_at_frame_boundary();
        s.add(
            "A17-切换-最新意图覆盖旧暂存",
            sb3.active().tier == DensityTier::Mid,
            "未应用的旧请求被新请求覆盖：只有最新意图生效（F2002 重组协议家族同款）",
        );
        let mut sb4 = SwitchBoard::new();
        let h = *sb4.active();
        sb4.request(DensityTier::Low);
        let _ = sb4.apply_at_frame_boundary();
        s.add(
            "A17-切换-换绑原子性平滑语义",
            h.pool_cap_pct == 80
                && h.density_permille == 500
                && sb4.active().pool_cap_pct == 50
                && sb4.active().density_permille == 250
                && binding_is_coherent(sb4.active()),
            "换绑产物三列同步到位（密度/上限/发射同结构体）——「换一半」的状态在类型上没有构造路径",
        );
    }

    // --- 判据 7：档位记忆（往返 + 翻转全检出 + 损坏/版本分型 + 迁移 + 回退） ---
    {
        let p = MemPayload { tier: DensityTier::Low, emission_scale_permille: 250 };
        let bytes = mem_encode(&p);
        let back = mem_decode(&bytes);
        s.add(
            "A17-记忆-编码往返逐位还原",
            back == Ok(p) && bytes.len() == 8,
            "编码→解码还原用户选择（8 字节定长，无内容字段——记忆本地无隐私面的类型事实）",
        );
        let mut all_caught = true;
        let mut byte_i = 0usize;
        while byte_i < 8 {
            let mut bit = 0u8;
            while bit < 8 {
                let mut flipped = bytes;
                flipped[byte_i] ^= 1u8 << bit;
                if mem_decode(&flipped) == Ok(p) {
                    all_caught = false;
                }
                bit += 1;
            }
            byte_i += 1;
        }
        s.add(
            "A17-记忆-单比特翻转全检出",
            all_caught,
            "8×8=64 个单比特翻转全部被拒（不是抽样——FNV-1a 对单比特翻转数学上全检出，抽样会给碰撞留侥幸面）",
        );
        let mut bad_magic = bytes;
        bad_magic[0] ^= 0xFF;
        let mut bad_ck = bytes;
        bad_ck[7] ^= 0x01;
        let mut bad_tier = bytes;
        bad_tier[3] = 9;
        s.add(
            "A17-记忆-损坏三型皆拒",
            mem_decode(&bad_magic) == Err(MemError::Corrupt)
                && mem_decode(&bad_ck) == Err(MemError::Corrupt)
                && mem_decode(&bad_tier) == Err(MemError::Corrupt),
            "魔数坏/校验和坏/档位越界三型损坏都判 Corrupt（走回退默认档路径）",
        );
        let mut unk = bytes;
        unk[2] = 99;
        unk[7] = mem_checksum(&unk);
        let unk_v = mem_decode(&unk);
        s.add(
            "A17-记忆-自洽未知版本显性拒绝",
            unk_v == Err(MemError::VersionUnsupported)
                && MemError::VersionUnsupported != MemError::Corrupt,
            "校验和自洽的未知版本走显性拒绝而不是静默回退——回退会无声丢掉用户选择，两者必须分开（F2015 家族判据同款）",
        );
        let mut v1 = bytes;
        v1[2] = 1;
        v1[4] = 0;
        v1[5] = 0;
        v1[6] = 0;
        v1[7] = mem_checksum(&v1);
        let mig = mem_decode(&v1);
        s.add(
            "A17-记忆-v1迁移档位保留且列取表",
            mig == Ok(MemPayload { tier: DensityTier::Low, emission_scale_permille: 250 }),
            "v1 载荷迁移到 v2：档位保留、发射天花板取档位表联动列现值（迁移不发明数值——表是单源）",
        );
        let mut bag = DiagBag::new();
        let r1 = mem_load_or_default(&bad_ck, &mut bag);
        s.add(
            "A17-记忆-损坏回退默认档且告警在案",
            r1 == Err(MemError::Corrupt)
                && bag.has(DiagCode(CODE_MEM_CORRUPT))
                && DEFAULT_TIER == DensityTier::Mid,
            "损坏路径记 MEM_CORRUPT 告警后调用方回退默认档（中档：首启体验两头都不塌——与家族 F2015 同规）",
        );
        let mut bag2 = DiagBag::new();
        let r2 = mem_load_or_default(&unk, &mut bag2);
        s.add(
            "A17-记忆-版本不支持不落默认",
            r2 == Err(MemError::VersionUnsupported)
                && bag2.has(DiagCode(CODE_MEM_VERSION_UNSUPPORTED))
                && bag2.p1_count() >= 1,
            "版本不支持记 P1 显性拒绝：调用方不得据此回退（静默降档=无声丢用户选择——两条错误路径分开是判据）",
        );
    }

    // --- 判据 8：判据自身（码互异 + 段独占 + 兜底 + 条数对账） ---
    {
        let mut ok = true;
        let mut i = 0usize;
        while i < CODES.len() {
            let mut j = i + 1;
            while j < CODES.len() {
                if CODES[i] == CODES[j] {
                    ok = false;
                }
                j += 1;
            }
            i += 1;
        }
        s.add("A17-判据-六码两两互异", ok, "六码互异（按码归类的前提）");
        s.add(
            "A17-判据-码段独占0x94",
            CODES.iter().all(|c| c & 0xFF00 == 0x9400),
            "全码独占 0x94 段（0x93=F2216 已占，续段不撞）",
        );
        s.add(
            "A17-判据-未知码兜底不panic",
            !explain(0x94FF).is_empty() && explain(CODE_TIER_DESYNC) != explain(0x94FF),
            "未知码有兜底人话（不崩也不静默）",
        );
        s.add(
            "A17-判据-条数对账",
            s.len() == 39,
            "判据条数恰 40（本条执行前已有 39 条，防悄悄增删）",
        );
    }

    s
}
