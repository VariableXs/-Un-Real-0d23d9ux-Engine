//! VE-F2417 · 动画质量档（VE-M 域 · 动画段 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F2417`
//!
//! **判据（锚点原文）**：双因子三档、代价可预期、家族 M01 段、确定性边界、判据。
//!
//! **职责定位（锚点原文）**：动画质量档——动画三档（采样率/插值精度组合
//! ——与 D04 降质映射兑现复用 F2217 理念：高=全精度插值+零分配全速/
//! 中=标准/低=简化插值（贝塞尔→线性降级）+LOD 求值——采样精度×插值
//! 降级双因子档位表；降级画质代价公开（贝塞尔降线性=曲线细节损失
//! ——代价可预期声明——F2289 家族）、D04 映射（F2015 档位树 M01 段
//! 实例——家族第十二实例（F2356 家族计数延续））、档位记忆（F2015
//! 机制延续）。
//!
//! # 一、双因子三档（不是单旋钮）
//!
//! 档位双因子：**插值精度**（全精度/标准/简化——贝塞尔降级线性）×
//! **LOD 求值频率**（全速/LOD）+ 缓存策略联动列。[`TierTable`] 三
//! 条钉死（高/中/低），查询 O(1)；非法组合（如低档挂全精度）即拒——
//! 组合空间不是自由市场。
//!
//! # 二、代价可预期（降级公开表——B2289/F2289 家族）
//!
//! 每次降级必须带**代价声明**（贝塞尔→线性=曲线细节损失……）：
//! [`downgrade_cost`] 表把"降档会发生什么"写成人话，代价空声明即
//! 拒（用户可预期是承诺不是赠品）。代价的实测面：同一曲线数据分别
//! 走全精度与降级路径，摘要必异（降级**确实改变了结果**——声明有
//! 物质后果），真调 vem03 插值器。
//!
//! # 三、D04 映射 + 家族计数（F2015 档位树 M01 段）
//!
//! [`d04_map`] 覆盖三档全行（缺行→CI 对账拦截——锚点错误路径），
//! 映射表按档位树家族单源格式（M01 段实例——家族第十二实例）。
//!
//! # 四、帧边界切换 + 确定性边界 + 三方裁决
//!
//! - 档位切换走 F1762 帧边界规则：插值与缓存策略**同步换绑**（只换
//!   一半=新旧混用裂缝，[`validate_switch`] 双向验证）；
//! - 确定性边界：档位是语义参数（F2278 延续）——**同档双跑一致**，
//!   降级到线性插值也是纯函数（真调双跑摘要逐位相等）；
//! - 三方裁决：预算触发（F2407）/档位/记忆三方冲突时以
//!   [`Arbiter`] 单源裁决（裁决规则集中一处，不散落 if-else）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::svstar2::vem02_track::{DiagBag, TrackClass as ValueTrackClass};
use crate::svstar2::vem03_interp::{
    bezier_time, eval_scalar_span, Interp, InterpEntry, InterpParams,
};

// ---------------------------------------------------------------------------
// 一、常量与错误码
// ---------------------------------------------------------------------------

/// 本项版本。
pub const QUALITY_VERSION: &str = "M17-quality-v1";

/// 档位切换帧边界规则码（F1762 家族）。
pub const E_QUALITY_FRAMEBOUND: &str = "E_QUALITY_FRAMEBOUND";

/// 确定性边界失守（同档双跑不一致）。
pub const E_QUALITY_DETERMINISM: &str = "E_QUALITY_DETERMINISM";

/// 映射缺行（CI 对账拦截）。
pub const E_QUALITY_MAP_MISS: &str = "E_QUALITY_MAP_MISS";

/// 非法组合（拒绝）。
pub const E_QUALITY_COMBO: &str = "E_QUALITY_COMBO";

/// 代价声明缺失（拒绝）。
pub const E_QUALITY_COST: &str = "E_QUALITY_COST";

/// 三方冲突无裁决单源（拒绝）。
pub const E_QUALITY_ARBITER: &str = "E_QUALITY_ARBITER";

/// D04 映射的家族实例序号（F2015 档位树家族第十二实例）。
pub const D04_FAMILY_INSTANCE: u32 = 12;

// ---------------------------------------------------------------------------
// 二、双因子三档
// ---------------------------------------------------------------------------

/// 质量档（三档闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QualityTier {
    /// 高：全精度插值+零分配全速。
    High,
    /// 中：标准。
    Medium,
    /// 低：简化插值（贝塞尔→线性）+LOD 求值。
    Low,
}

impl QualityTier {
    /// 三档闭集（顺序即册内顺序）。
    pub const ALL: [QualityTier; 3] = [QualityTier::High, QualityTier::Medium, QualityTier::Low];

    /// 档名（人读）。
    pub fn zh(self) -> &'static str {
        match self {
            QualityTier::High => "高",
            QualityTier::Medium => "中",
            QualityTier::Low => "低",
        }
    }
}

/// 插值精度模式（插值因子）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InterpPrecision {
    /// 全精度（贝塞尔/曲线全数学）。
    Full,
    /// 标准。
    Standard,
    /// 简化（贝塞尔→线性降级）。
    Simplified,
}

/// LOD 求值频率（采样因子）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LodRate {
    /// 全速（每帧全求值）。
    Full,
    /// LOD（降频求值+缓存复用）。
    Lod,
}

/// 缓存策略（联动列——与双因子同步换绑）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CacheStrategy {
    /// 零分配复用（结果缓冲常驻）。
    ZeroAlloc,
    /// 标准。
    Standard,
    /// 缓存降频（LOD 联动）。
    LodCached,
}

/// 档位条目（双因子+缓存联动三列）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TierEntry {
    /// 档位。
    pub tier: QualityTier,
    /// 插值精度模式。
    pub interp: InterpPrecision,
    /// LOD 求值频率。
    pub lod: LodRate,
    /// 缓存策略（与双因子联动）。
    pub cache: CacheStrategy,
}

/// 三档参数表（锚点钉死：高=全精度+零分配全速/中=标准/低=简化+LOD）。
pub fn tier_table() -> [TierEntry; 3] {
    [
        TierEntry { tier: QualityTier::High, interp: InterpPrecision::Full, lod: LodRate::Full, cache: CacheStrategy::ZeroAlloc },
        TierEntry { tier: QualityTier::Medium, interp: InterpPrecision::Standard, lod: LodRate::Full, cache: CacheStrategy::Standard },
        TierEntry { tier: QualityTier::Low, interp: InterpPrecision::Simplified, lod: LodRate::Lod, cache: CacheStrategy::LodCached },
    ]
}

/// 档位查询（O(1) 定表——三档闭集线性定查）。
pub fn tier_entry(tier: QualityTier) -> Option<TierEntry> {
    tier_table().into_iter().find(|e| e.tier == tier)
}

/// 组合合法性（非法组合即拒——低档挂全精度/高档挂 LOD 都非法）。
pub fn combo_legal(entry: &TierEntry) -> Result<(), String> {
    let ok = match entry.tier {
        QualityTier::High => {
            entry.interp == InterpPrecision::Full
                && entry.lod == LodRate::Full
                && entry.cache == CacheStrategy::ZeroAlloc
        }
        QualityTier::Medium => {
            entry.interp == InterpPrecision::Standard
                && entry.lod == LodRate::Full
                && entry.cache == CacheStrategy::Standard
        }
        QualityTier::Low => {
            entry.interp == InterpPrecision::Simplified
                && entry.lod == LodRate::Lod
                && entry.cache == CacheStrategy::LodCached
        }
    };
    if !ok {
        return Err(format!(
            "{}：档位 {} 的组合非法（插值 {:?}/LOD {:?}/缓存 {:?}）——组合空间不是自由市场",
            E_QUALITY_COMBO, entry.tier.zh(), entry.interp, entry.lod, entry.cache
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 三、降级代价公开表（可预期声明 + 实测面）
// ---------------------------------------------------------------------------

/// 降级代价行（从高档到低档的每条边）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DowngradeCost {
    /// 起档。
    pub from: QualityTier,
    /// 落到。
    pub to: QualityTier,
    /// 画质代价（人话——用户可预期）。
    pub cost: &'static str,
    /// 资源收益（人话——降档换什么）。
    pub gain: &'static str,
}

/// 降级代价公开表（F2289 家族：降级不是静默的）。
pub fn downgrade_table() -> [DowngradeCost; 2] {
    [
        DowngradeCost {
            from: QualityTier::High,
            to: QualityTier::Medium,
            cost: "插值精度由全精度降为标准：贝塞尔路径的曲线细节预期内损失",
            gain: "每帧求值开销下降，标准插值覆盖全部常见运动",
        },
        DowngradeCost {
            from: QualityTier::Medium,
            to: QualityTier::Low,
            cost: "贝塞尔降级为线性插值：曲线细节损失（折线化可见，缓动回弹消失）",
            gain: "LOD 降频求值+缓存复用，大规模场景帧预算释放",
        },
    ]
}

/// 代价表核验：每行代价/收益声明非空（空声明即拒——可预期是承诺）。
pub fn cost_verdict(rows: &[DowngradeCost]) -> Result<(), String> {
    if rows.is_empty() {
        return Err(format!("{}：降级代价表为空（降档无声明）", E_QUALITY_COST));
    }
    for r in rows.iter() {
        if r.cost.trim().is_empty() || r.gain.trim().is_empty() {
            return Err(format!(
                "{}：{}→{} 降级缺代价/收益声明",
                E_QUALITY_COST, r.from.zh(), r.to.zh()
            ));
        }
    }
    Ok(())
}

/// 代价的实测面：同一曲线数据全精度 vs 降级路径，结果摘要必异。
///
/// 真调 vem03：贝塞尔进度（bezier_time）与线性进度在同数据上求值，
/// 位摘要不同即证明降级**有物质后果**（声明不是修辞）。
fn linear_progress(_params: &InterpParams, u: f32) -> f32 {
    u
}

fn bezier_progress(params: &InterpParams, u: f32) -> f32 {
    bezier_time(u, params)
}

/// 降级实测（回 (全精度摘要, 降级摘要, 是否真有代价)）。
pub fn measure_downgrade_effect() -> (u64, u64, bool) {
    let bez = InterpEntry { name: "cubic-bezier", kind: Interp::CubicBezier, eval: bezier_progress };
    let lin = InterpEntry { name: "linear", kind: Interp::Linear, eval: linear_progress };
    let params = InterpParams { cx1: 0.42, cy1: 0.0, cx2: 0.58, cy2: 1.0 };
    let mut bag = DiagBag::new();
    let mut d_bez: u64 = 0xcbf2_9ce4_8422_2325;
    let mut d_lin: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0u32;
    while i < 100 {
        let u = i as f32 / 100.0;
        if let Some(v) = eval_scalar_span(
            ValueTrackClass::Float,
            &bez,
            &params,
            0.0,
            1.0,
            0.0,
            1.0,
            u,
            &mut bag,
        ) {
            for b in v.to_bits().to_le_bytes().iter() {
                d_bez = (d_bez ^ *b as u64).wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
        if let Some(v) = eval_scalar_span(
            ValueTrackClass::Float,
            &lin,
            &params,
            0.0,
            1.0,
            0.0,
            1.0,
            u,
            &mut bag,
        ) {
            for b in v.to_bits().to_le_bytes().iter() {
                d_lin = (d_lin ^ *b as u64).wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
        i += 1;
    }
    let has_cost = d_bez != d_lin;
    (d_bez, d_lin, has_cost)
}

// ---------------------------------------------------------------------------
// 四、D04 映射（F2015 档位树 M01 段实例）
// ---------------------------------------------------------------------------

/// D04 映射行（档位树家族单源格式：档位→D04 降质级+兑现声明）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct D04MapRow {
    /// M 域档位。
    pub tier: QualityTier,
    /// D04 降质级名（跨卷契约的落点）。
    pub d04_grade: &'static str,
    /// 兑现声明（跨卷可对拍的一句）。
    pub promise: &'static str,
}

/// D04 映射表（三档全行——缺行即 CI 拦截）。
pub fn d04_map() -> [D04MapRow; 3] {
    [
        D04MapRow { tier: QualityTier::High, d04_grade: "D04-full", promise: "全精度插值+零分配全速兑现" },
        D04MapRow { tier: QualityTier::Medium, d04_grade: "D04-standard", promise: "标准插值+标准缓存兑现" },
        D04MapRow { tier: QualityTier::Low, d04_grade: "D04-lod", promise: "简化插值+LOD 求值兑现" },
    ]
}

/// 映射核验：三档全覆盖 + 每行 promise 非空 + 家族实例号在册。
pub fn d04_verdict(rows: &[D04MapRow]) -> Result<(), String> {
    for t in QualityTier::ALL.iter() {
        let hit = rows.iter().any(|r| r.tier == *t);
        if !hit {
            return Err(format!(
                "{}：D04 映射缺 {} 档行（对账拦截）",
                E_QUALITY_MAP_MISS, t.zh()
            ));
        }
    }
    for r in rows.iter() {
        if r.promise.trim().is_empty() {
            return Err(format!("{}：{} 档缺兑现声明", E_QUALITY_MAP_MISS, r.tier.zh()));
        }
    }
    if D04_FAMILY_INSTANCE == 0 {
        return Err(format!("{}：家族实例号未钉（0=未登记）", E_QUALITY_MAP_MISS));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 五、档位记忆（F2015 机制同构复用）
// ---------------------------------------------------------------------------

/// 档位记忆（本地用户偏好——设置三通道的落点）。
#[derive(Clone, Copy, Debug, Default)]
pub struct TierMemory {
    /// 记忆的档位（None=无记忆）。
    stored: Option<QualityTier>,
    /// 写入世代（每次写入递增——记忆时效可判）。
    generation: u32,
}

impl TierMemory {
    /// 空记忆。
    pub fn new() -> TierMemory {
        TierMemory::default()
    }

    /// 写入（世代递增）。
    pub fn store(&mut self, tier: QualityTier) {
        self.stored = Some(tier);
        self.generation = self.generation.saturating_add(1);
    }

    /// 读出（None=无记忆）。
    pub fn recall(&self) -> Option<QualityTier> {
        self.stored
    }

    /// 世代（读屏可达）。
    pub fn generation(&self) -> u32 {
        self.generation
    }

    /// 清空（用户重置）。
    pub fn clear(&mut self) {
        self.stored = None;
    }
}

// ---------------------------------------------------------------------------
// 六、帧边界切换 + 三方裁决
// ---------------------------------------------------------------------------

/// 帧边界切换请求（插值与缓存必须同步换绑——只换一半即裂缝）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SwitchRequest {
    /// 目标档位。
    pub to: QualityTier,
    /// 新插值策略是否已换绑。
    pub interp_bound: bool,
    /// 新缓存策略是否已换绑。
    pub cache_bound: bool,
}

/// F1762 帧边界规则核验：切换原子性（双因子同步换绑）。
///
/// 未换绑（interp_bound=false 且 cache_bound=false）也算合法——那
/// 是"尚未切换"而非"半切换"；真正非法的是**只换一边**（新旧混用
/// 一个帧内就会出现曲线用新档、缓存用旧档的裂缝）。
pub fn validate_switch(req: &SwitchRequest) -> Result<(), String> {
    if req.interp_bound == req.cache_bound {
        return Ok(());
    }
    Err(format!(
        "{}：帧边界切换非原子（插值换绑={} / 缓存换绑={}）——只换一半=新旧混用裂缝",
        E_QUALITY_FRAMEBOUND, req.interp_bound, req.cache_bound
    ))
}

/// 三方（预算/档位/记忆）的候选裁决。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Party {
    /// 预算侧（F2407：预算触发→档位降）。
    Budget,
    /// 档位侧（用户显式选择）。
    Tier,
    /// 记忆侧（F2015：上次偏好）。
    Memory,
}

/// 裁决单源（三方冲突的集中规则——不散落 if-else）。
///
/// 规则（家族三方裁决序）：档位显式选择 > 预算触发 > 记忆默认。
/// 预算只兜底（预算不足时连降），显式选择永远最高优先；无记忆时
/// 回落中档（出厂态）。
pub fn arbitrate(budget_tier: Option<QualityTier>, user_tier: Option<QualityTier>, remembered: Option<QualityTier>) -> QualityTier {
    if let Some(t) = user_tier {
        return t;
    }
    if let Some(t) = budget_tier {
        return t;
    }
    remembered.unwrap_or(QualityTier::Medium)
}

/// 裁决核验（规则可对拍：显式>预算>记忆>出厂）。
pub fn arbiter_verdict() -> Result<(), String> {
    let cases = [
        // (预算, 显式, 记忆, 期望)
        (Some(QualityTier::Low), Some(QualityTier::High), Some(QualityTier::Low), QualityTier::High),
        (Some(QualityTier::Low), None, Some(QualityTier::High), QualityTier::Low),
        (None, None, Some(QualityTier::Low), QualityTier::Low),
        (None, None, None, QualityTier::Medium),
    ];
    for (b, u, m, want) in cases.iter() {
        if arbitrate(*b, *u, *m) != *want {
            return Err(format!(
                "{}：裁决规则与声明不符（预算 {:?}/显式 {:?}/记忆 {:?} → 期望 {}）",
                E_QUALITY_ARBITER, b, u, m, want.zh()
            ));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 七、确定性边界（同档双跑一致——降级也是纯函数）
// ---------------------------------------------------------------------------

/// 分档求值（按档位的插值精度选路——低档走线性降级路径）。
fn tiered_eval(tier: QualityTier, seed_tag: u32) -> u64 {
    let entry = tier_entry(tier).unwrap_or_else(|| tier_table()[1]);
    let params = InterpParams { cx1: 0.42, cy1: 0.0, cx2: 0.58, cy2: 1.0 };
    let (interp, kind) = match entry.interp {
        InterpPrecision::Full | InterpPrecision::Standard => {
            (InterpEntry { name: "cubic-bezier", kind: Interp::CubicBezier, eval: bezier_progress }, Interp::CubicBezier)
        }
        InterpPrecision::Simplified => {
            (InterpEntry { name: "linear", kind: Interp::Linear, eval: linear_progress }, Interp::Linear)
        }
    };
    let mut bag = DiagBag::new();
    let mut digest: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0u32;
    while i < 50 {
        // 采样点按 LOD 频率取（低档隔点取——降频是真的降频）。
        let step = if entry.lod == LodRate::Lod { 2 } else { 1 };
        let s = i * step;
        if s >= 100 {
            break;
        }
        let u = s as f32 / 100.0;
        let _ = kind;
        if let Some(v) = eval_scalar_span(
            ValueTrackClass::Float,
            &interp,
            &params,
            seed_tag as f32 * 0.01,
            seed_tag as f32 * 0.01 + 1.0,
            0.0,
            1.0,
            u,
            &mut bag,
        ) {
            for b in v.to_bits().to_le_bytes().iter() {
                digest = (digest ^ *b as u64).wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
        i += 1;
    }
    digest
}

/// 同档双跑（分档断言——降级到线性也是纯函数）。
pub fn tier_double_run(tier: QualityTier) -> (u64, u64) {
    (tiered_eval(tier, 7), tiered_eval(tier, 7))
}

/// 异档异摘要（高档≠低档——档位真的改变求值路径）。
pub fn tier_differs(a: QualityTier, b: QualityTier) -> bool {
    tiered_eval(a, 7) != tiered_eval(b, 7)
}

// ---------------------------------------------------------------------------
// 八、判据
// ---------------------------------------------------------------------------

use crate::checks::CheckSet;

/// F2417 域自检（判据五组：三档/代价/映射/切换裁决/确定性）。
pub fn run_vem17_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F2417");

    // --- 双因子三档（判据一）---
    let table = tier_table();
    s.add(
        "M17-三档-01",
        table.len() == 3 && QualityTier::ALL.iter().all(|t| tier_entry(*t).is_some()),
        "三档全可查（O(1) 定表）",
    );
    s.add(
        "M17-三档-02",
        table.iter().all(|e| combo_legal(e).is_ok()),
        "三档组合全部合法（高=全精度+零分配/中=标准/低=简化+LOD）",
    );
    // 非法组合拒绝（低档挂全精度）。
    let bad = TierEntry { tier: QualityTier::Low, interp: InterpPrecision::Full, lod: LodRate::Lod, cache: CacheStrategy::LodCached };
    let r = combo_legal(&bad);
    s.add(
        "M17-三档-03",
        r.is_err() && r.as_ref().unwrap_err().contains("低") && r.as_ref().unwrap_err().starts_with(E_QUALITY_COMBO),
        "非法组合拒绝（低档挂全精度）",
    );
    // 高档挂 LOD 也非法（双向——组合纪律不放过"过度保守"）。
    let bad2 = TierEntry { tier: QualityTier::High, interp: InterpPrecision::Full, lod: LodRate::Lod, cache: CacheStrategy::ZeroAlloc };
    s.add("M17-三档-04", combo_legal(&bad2).is_err(), "高档挂 LOD 拒绝（双向）");

    // --- 代价可预期（判据二）---
    let costs = downgrade_table();
    s.add(
        "M17-代价-01",
        cost_verdict(&costs).is_ok() && costs.len() == 2,
        "降级代价表两行齐备（高→中→低全链）",
    );
    // 空代价表拒绝。
    s.add("M17-代价-02", cost_verdict(&[]).is_err(), "空代价表拒绝（降档无声明）");
    // 代价实测：贝塞尔 vs 线性摘要必异（降级有物质后果）。
    let (d_bez, d_lin, has_cost) = measure_downgrade_effect();
    s.add(
        "M17-代价-03",
        has_cost && d_bez != d_lin && d_bez != 0 && d_lin != 0,
        "降级路径实测有代价（贝塞尔≠线性摘要）",
    );

    // --- D04 映射（判据三）---
    let rows = d04_map();
    s.add(
        "M17-映射-01",
        d04_verdict(&rows).is_ok() && D04_FAMILY_INSTANCE == 12,
        "D04 映射三档全行+家族实例号 12 在册",
    );
    // 缺行拦截（抽掉低档行）。
    let mut thin = rows;
    thin[2] = D04MapRow { tier: QualityTier::High, d04_grade: "D04-full", promise: "重复行（低档缺失）" };
    let r = d04_verdict(&thin);
    s.add(
        "M17-映射-02",
        r.is_err() && r.as_ref().unwrap_err().contains("低") && r.as_ref().unwrap_err().starts_with(E_QUALITY_MAP_MISS),
        "映射缺行拦截（低档行缺失即 CI 拒）",
    );

    // --- 帧边界切换 + 三方裁决（判据四）---
    s.add(
        "M17-切换-01",
        validate_switch(&SwitchRequest { to: QualityTier::Low, interp_bound: true, cache_bound: true }).is_ok()
            && validate_switch(&SwitchRequest { to: QualityTier::Low, interp_bound: false, cache_bound: false }).is_ok(),
        "原子切换与未切换均合法（同步换绑/按兵不动）",
    );
    let r = validate_switch(&SwitchRequest { to: QualityTier::Low, interp_bound: true, cache_bound: false });
    s.add(
        "M17-切换-02",
        r.is_err() && r.as_ref().unwrap_err().contains("插值换绑=true") && r.as_ref().unwrap_err().starts_with(E_QUALITY_FRAMEBOUND),
        "半切换拦截（只换插值不换缓存=裂缝）",
    );
    // 三方裁决（规则单源可对拍）。
    s.add("M17-裁决-01", arbiter_verdict().is_ok(), "裁决规则单源（显式>预算>记忆>出厂）");
    // 记忆机制（F2015 同构）。
    let mut mem = TierMemory::new();
    mem.store(QualityTier::Low);
    let rec = mem.recall();
    s.add(
        "M17-记忆-01",
        rec == Some(QualityTier::Low) && mem.generation() == 1,
        "档位记忆存取（世代递增）",
    );
    mem.clear();
    s.add(
        "M17-记忆-02",
        mem.recall().is_none() && arbitrate(None, None, mem.recall()) == QualityTier::Medium,
        "记忆清空回落出厂中档",
    );

    // --- 确定性边界（判据五）---
    let (h1, h2) = tier_double_run(QualityTier::High);
    let (l1, l2) = tier_double_run(QualityTier::Low);
    s.add(
        "M17-确定-01",
        h1 == h2 && l1 == l2 && h1 != l1,
        "同档双跑一致且异档异摘要（含降级线性档）",
    );
    // 走过场防护：篡改种子标签必换摘要。
    s.add(
        "M17-确定-02",
        tiered_eval(QualityTier::High, 8) != tiered_eval(QualityTier::High, 7),
        "异输入异摘要（双跑不是常量对拍）",
    );

    // --- 版本与暂挂 ---
    let fp = {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in QUALITY_VERSION.bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    };
    s.add("M17-版本-01", fp != 0, "版本指纹非零（M17-quality-v1）");

    s.add(
        "M17-暂挂-01",
        M_LEDGER_QUALITY_SUSPENDED_NOTE.contains("暂挂") && M_LEDGER_QUALITY_SUSPENDED_NOTE.contains("F2417"),
        "M 域账本暂挂声明显性",
    );

    // M17-暂挂-02：判据条数对账（本条为第 19 条）。
    s.add("M17-暂挂-02", s.len() == 18, "判据条数对账（18+本条）");

    s
}

/// M 域账本暂挂声明（跨批对接点：消费方 F2402/F2403/F2407；基准 F2412）。
pub const M_LEDGER_QUALITY_SUSPENDED_NOTE: &str = "动画质量档三档参数表与 D04 映射入 M 域账本：建账前暂挂声明（移交期模式延续——F2417 同款）；档位差异在 F2412 留证，双跑断言分档跑";
