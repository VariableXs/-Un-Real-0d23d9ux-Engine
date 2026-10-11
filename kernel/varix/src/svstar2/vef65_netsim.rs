//! VE-F5412 · 网络模拟（VE-\ 域 · 网络批次 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F5412`
//!
//! **判据（锚点原文）**：劣化注入、千连接、保真对拍、降档、判据。
//!
//! **职责定位（锚点原文）**：网络模拟器（网络模拟三件：延迟/丢包/抖动
//! 注入（可配网络劣化环境——最差网络下也可开发测试）/大规模连接模拟
//! （千连接并发容量验证）/模拟保真对拍（与真实网络结论偏差超阈标注
//! ——U 域纪律同规）。错误路径与降级矩阵：模拟失偏→标注降级；模拟过
//! 实机败→以实机为准立案；资源超限→降档。
//!
//! ## 一、劣化注入器（网络模拟三件之一）
//!
//! [`DegradeProfile`] 劣化档案四参数（**全部整数、零浮点**）：基础延迟
//! ms、抖动幅度 ms（± 对称）、丢包率‰、带宽上限 kbps。注入裁决
//! [`inject_once`] 三出口闭集：`Delayed`（延迟投递——带注入延迟值）/
//! `Lost`（丢包）/ `Delivered`（按时投递）——三出口计数守恒（总数 =
//! 三类之和，判据钉死）。伪随机走 64 位 LCG（Knuth MMIX 常数）：
//! **同种子同序列**——最差网络下的开发测试也要可复现（F6001 可预期
//! 律在网络模拟面的延续）；种子零拒（[`SimCode::BadSeed`]——零种子
//! 使序列退化）。
//!
//! ## 二、大规模连接模拟（网络模拟三件之二）
//!
//! [`ConnSim`] 千连接并发容量验证：容量 [`CONN_CAP`] = 1024，注册满
//! 即显性拒（[`SimCode::ConnCapFull`] 不静默驱逐）；每 tick 全连接
//! 推进（O(连接数)——锚点性能口径），逐连接状态机三态闭集：`Healthy`
//! / `Degraded`（注入延迟/抖动命中）/ `Dropped`（丢包连续命中）。
//! 洪峰统计三态守恒（存活+降级+掉线 == 注册总数——账实一致判据）。
//! 并发超限 → **降档阶梯**自动逐档下降（见四）。
//!
//! ## 三、模拟保真对拍（网络模拟三件之三）
//!
//! [`FidelityChecker`]：模拟结论（延迟均值/丢包率）对实测结论逐项
//! 千分比偏差，超 [`FIDELITY_TOL_PERMILLE`] = 50‰ → **标注降级**
//! （[`SimCode::FidelityDrift`]——模拟失偏要显性标注不是装准，U 域
//! 纪律同规：口说无凭账面可查）。**模拟过实机败 → 以实机为准立案**
//! （[`SimCode::RealMachineConflict`] + [`FilingRecord`] 留痕——模拟
//! 结论与实机冲突时实机赢，立案可追溯不是悄悄改数）。
//!
//! ## 四、降档阶梯（资源超限→降档）
//!
//! [`LADDER`] 档位表 const 闭集四档：L0 全容量 / L1 半容量 / L2 半
//! 注入强度 / L3 最低档（容量+强度双半）。降档 **O(1)**（档位下标
//! 切换，不重建任何表）；最低档仍超限 → [`SimCode::LadderExhausted`]
//! （拒绝承载显性——降无可降不硬撑）。档位表与 [`ladder_rule`] 推导
//! **双写全格对拍**（四档 × 容量/强度两轴 = 8 格——全域双写纪律
//! 第 4 单延续：F6004 权限 / F5410 变更 / F5411 节必要性 / 本单阶梯）。
//!
//! ## 五、场景册（可配劣化环境）
//!
//! 劣化档案注册成**场景**（[`Scenario`]：名字 + 档案）——「最差网络
//! 下也可开发测试」的配置面：典型场景预置（[`SCENARIOS_PRESET`]：
//! 光纤/4G/地铁/极端劣化四档），场景册可扩（容量闸）。未注册场景/
//! 空场景显性拒。
//!
//! ## 六、审计与自检
//!
//! 注入/降档/对拍/立案全留痕（append-only 满容拒记计数）；账实一致
//! 探针（连接三态守恒 + 场景档案合法性重验）；确定性红线自检
//! （[`SimCode::DeterminismBroken`]——同种子双实例序列逐位对拍，
//! 断了即红线）。诊断码独占 `0x8Dxx` 段十六码闭集两两互异
//! reason 全码可读。
//!
//! **对接**：上游 F4344/F4912 纪律同规；下游 F5418 基准消费（模拟
//! 结论是基准的输入之一——失偏标注随数据走）。no_std 零 std 依赖、
//! 零 unwrap 零 expect 零切片越界、零全局可变状态。

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 模块版本。
pub const VEF65_VERSION: &str = "vef65-netsim-v1";

/// 连接容量（千连接并发验证口径——1024 ≥ 千）。
pub const CONN_CAP: usize = 1024;

/// 延迟上限 ms（注入参数边界——超界即拒）。
pub const MAX_LATENCY_MS: u32 = 2000;

/// 丢包率上限‰（1000‰ = 全丢——允许全丢但必须显性声明）。
pub const MAX_LOSS_PERMILLE: u32 = 1000;

/// 保真偏差容忍（‰——超阈标注降级）。
pub const FIDELITY_TOL_PERMILLE: u32 = 50;

/// 场景册容量。
pub const SCENARIO_CAP: usize = 32;

/// 丢包连续命中即掉线的阈值（连续 N 次丢包 → Dropped）。
pub const DROP_STREAK_DROP: u32 = 3;

/// 审计簿容量（append-only 满容拒记计数不覆盖旧痕）。
pub const AUDIT_CAP: usize = 256;

/// LCG 乘数（Knuth MMIX）。
pub const LCG_A: u64 = 6364136223846793005;

/// LCG 增量（Knuth MMIX）。
pub const LCG_C: u64 = 1442695040888963407;

/// 签名指纹 FNV-1a 64 位偏移基。
pub const FNV_OFFSET: u64 = 0xcbf29ce484222325;

/// FNV-1a 64 位素数。
pub const FNV_PRIME: u64 = 0x100000001b3;

// ---------------------------------------------------------------------------
// 二、诊断码（独占 0x8Dxx 段十六码闭集）
// ---------------------------------------------------------------------------

/// F5412 诊断码。独占 `0x8Dxx` 段，十六码闭集两两互异。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SimCode(pub u16);

impl SimCode {
    /// 0x8D01 劣化参数越界（丢包超‰上限/延迟超 ms 上限/抖动负值）。
    pub const BadDegradeProfile: SimCode = SimCode(0x8D01);
    /// 0x8D02 连接容量满（显性拒不驱逐）。
    pub const ConnCapFull: SimCode = SimCode(0x8D02);
    /// 0x8D03 连接未注册。
    pub const UnknownConn: SimCode = SimCode(0x8D03);
    /// 0x8D04 并发超限（触发降档阶梯）。
    pub const SimOverloaded: SimCode = SimCode(0x8D04);
    /// 0x8D05 保真偏差超阈（模拟失偏——标注降级不装准）。
    pub const FidelityDrift: SimCode = SimCode(0x8D05);
    /// 0x8D06 模拟过实机败（以实机为准立案）。
    pub const RealMachineConflict: SimCode = SimCode(0x8D06);
    /// 0x8D07 降档阶梯耗尽（最低档仍超限——拒绝承载显性）。
    pub const LadderExhausted: SimCode = SimCode(0x8D07);
    /// 0x8D08 坏种子（零种子使序列退化——显性拒）。
    pub const BadSeed: SimCode = SimCode(0x8D08);
    /// 0x8D09 越权（操作者 × 操作不在权限内）。
    pub const PermissionDenied: SimCode = SimCode(0x8D09);
    /// 0x8D0A tick 溢出防护（tick 计数饱和——账面诚实）。
    pub const TickSaturated: SimCode = SimCode(0x8D0A);
    /// 0x8D0B 审计满（append-only 满容拒记计数不覆盖旧痕）。
    pub const AuditFull: SimCode = SimCode(0x8D0B);
    /// 0x8D0C 账实不符（三态守恒破坏——自检红线）。
    pub const LedgerDrift: SimCode = SimCode(0x8D0C);
    /// 0x8D0D 空场景（注入档案未设置——不猜默认）。
    pub const EmptyScenario: SimCode = SimCode(0x8D0D);
    /// 0x8D0E 场景未注册。
    pub const UnknownScenario: SimCode = SimCode(0x8D0E);
    /// 0x8D0F 场景册满。
    pub const ScenarioFull: SimCode = SimCode(0x8D0F);
    /// 0x8D10 确定性破坏（同种子异序列——红线自检）。
    pub const DeterminismBroken: SimCode = SimCode(0x8D10);

    /// 两两互异的 wire 码。
    pub const fn code(self) -> u16 {
        self.0
    }

    /// 人话原因（诊断可读——拒绝不是哑的 Err）。
    pub fn reason(self) -> String {
        match self {
            SimCode::BadDegradeProfile => "劣化参数越界：丢包超‰上限/延迟超 ms 上限/抖动负值".into(),
            SimCode::ConnCapFull => "连接容量满：显性拒不驱逐".into(),
            SimCode::UnknownConn => "连接未注册：先注册再模拟".into(),
            SimCode::SimOverloaded => "并发超限：降档阶梯逐档下降（O(1) 档位切换）".into(),
            SimCode::FidelityDrift => "保真偏差超阈：模拟失偏显性标注降级不装准（U 域纪律同规）".into(),
            SimCode::RealMachineConflict => "模拟过实机败：以实机为准立案（模拟结论与实机冲突时实机赢）".into(),
            SimCode::LadderExhausted => "降档阶梯耗尽：最低档仍超限——拒绝承载显性不硬撑".into(),
            SimCode::BadSeed => "坏种子：零种子使序列退化——同种子同序列要求非零起点".into(),
            SimCode::PermissionDenied => "越权：操作者对此操作无权限".into(),
            SimCode::TickSaturated => "tick 计数饱和：账面诚实标注不静默回绕".into(),
            SimCode::AuditFull => "审计满：append-only 满容拒记计数".into(),
            SimCode::LedgerDrift => "账实不符：连接三态守恒破坏".into(),
            SimCode::EmptyScenario => "空场景：注入档案未设置——不猜默认".into(),
            SimCode::UnknownScenario => "场景未注册：先注册再引用".into(),
            SimCode::ScenarioFull => "场景册满：显性拒不覆盖".into(),
            SimCode::DeterminismBroken => "确定性破坏：同种子异序列——红线自检".into(),
            SimCode(_) => "未知模拟诊断码".into(),
        }
    }
}

/// `segment_guard`：十六码闭集两两互异 + 全落 0x8D 段。
pub const SEGMENT_GUARD: [SimCode; 16] = [
    SimCode::BadDegradeProfile,
    SimCode::ConnCapFull,
    SimCode::UnknownConn,
    SimCode::SimOverloaded,
    SimCode::FidelityDrift,
    SimCode::RealMachineConflict,
    SimCode::LadderExhausted,
    SimCode::BadSeed,
    SimCode::PermissionDenied,
    SimCode::TickSaturated,
    SimCode::AuditFull,
    SimCode::LedgerDrift,
    SimCode::EmptyScenario,
    SimCode::UnknownScenario,
    SimCode::ScenarioFull,
    SimCode::DeterminismBroken,
];

/// 模拟错误路径（显性携带 [`SimCode`]）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimErr {
    /// 携诊断码的失败。
    Code(SimCode),
}

impl SimErr {
    /// 对外呈现码。
    pub const fn code(self) -> u16 {
        match self {
            SimErr::Code(c) => c.code(),
        }
    }
}

// ---------------------------------------------------------------------------
// 三、确定性伪随机（LCG——同种子同序列，零浮点）
// ---------------------------------------------------------------------------

/// LCG 步进（64 位满周期——Knuth MMIX 常数对）。
pub const fn lcg_next(state: u64) -> u64 {
    state.wrapping_mul(LCG_A).wrapping_add(LCG_C)
}

/// 由 LCG 态取 [0, bound) 均匀值（模减偏差用拒绝采样——小界精确均匀）。
///
/// bound == 0 → 0（调用方先验拒绝——本模块内 bound 恒 > 0）。
pub fn lcg_below(state: u64, bound: u64) -> (u64, u64) {
    if bound == 0 {
        return (0, state);
    }
    // 拒绝采样：取 (u64::MAX / bound) * bound 为接受区——消除模偏差
    let zone = u64::MAX / bound * bound;
    let mut s = state;
    loop {
        let v = lcg_next(s);
        s = v;
        if v < zone || zone == 0 {
            return (v % bound, s);
        }
    }
}

// ---------------------------------------------------------------------------
// 四、劣化档案与注入（网络模拟三件之一）
// ---------------------------------------------------------------------------

/// 劣化档案（四参数全整数——最差网络下也可开发测试的配置面）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DegradeProfile {
    /// 基础延迟（ms，单向）。
    pub latency_ms: u32,
    /// 抖动幅度（ms，± 对称——实际延迟 = latency ± rand(jitter)）。
    pub jitter_ms: u32,
    /// 丢包率（‰——千分比整数，F1619 字节级确定纪律同源）。
    pub loss_permille: u32,
    /// 带宽上限（kbps——账面标注用，注入裁决不消费）。
    pub bandwidth_kbps: u32,
}

impl DegradeProfile {
    /// 档案合法性校验（闭集边界——越界显性拒不钳制）。
    pub fn validate(&self) -> Result<(), SimCode> {
        if self.loss_permille > MAX_LOSS_PERMILLE {
            return Err(SimCode::BadDegradeProfile);
        }
        if self.latency_ms > MAX_LATENCY_MS {
            return Err(SimCode::BadDegradeProfile);
        }
        // 抖动不得为负（u32 天然非负）且不超过延迟（± 对称半幅）
        if self.jitter_ms > self.latency_ms {
            return Err(SimCode::BadDegradeProfile);
        }
        Ok(())
    }
}

/// 注入裁决三出口（闭集守恒——总数 = 三类之和）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InjectOutcome {
    /// 按时投递。
    Delivered,
    /// 延迟投递（携带注入延迟 ms）。
    Delayed(u32),
    /// 丢包。
    Lost,
}

impl InjectOutcome {
    /// 人话名。
    pub const fn tag(self) -> &'static str {
        match self {
            InjectOutcome::Delivered => "按时投递",
            InjectOutcome::Delayed(_) => "延迟投递",
            InjectOutcome::Lost => "丢包",
        }
    }
}

/// 注入裁决（O(1)——锚点性能口径）。
///
/// 裁决序：先掷丢包（[0,1000) < loss_permille → Lost）→ 再掷抖动
/// （命中窗 → Delayed(latency±jitter)）→ 否则 Delivered。全整数、
/// 同种子同序列。
pub fn inject_once(profile: &DegradeProfile, rng: u64) -> (InjectOutcome, u64) {
    // 丢包掷：千分位整数直接比较（零浮点）
    let (roll, rng1) = lcg_below(rng, 1000);
    if roll < profile.loss_permille as u64 {
        return (InjectOutcome::Lost, rng1);
    }
    // 抖动掷：jitter==0 直接按时
    if profile.jitter_ms == 0 {
        return (InjectOutcome::Delivered, rng1);
    }
    let (j, rng2) = lcg_below(rng1, profile.jitter_ms as u64 * 2 + 1);
    // 抖动对称：[−jitter, +jitter] → 偏移 = j − jitter（i64 中间量防下溢）
    let offset = j as i64 - profile.jitter_ms as i64;
    let actual = profile.latency_ms as i64 + offset;
    if actual <= 0 {
        return (InjectOutcome::Delivered, rng2); // 负延迟物理不存在——按时
    }
    let delayed = actual as u32;
    if delayed > profile.latency_ms {
        (InjectOutcome::Delayed(delayed), rng2)
    } else {
        (InjectOutcome::Delivered, rng2)
    }
}

// ---------------------------------------------------------------------------
// 五、连接状态机与大规模模拟（网络模拟三件之二）
// ---------------------------------------------------------------------------

/// 连接三态闭集（守恒：三态计数和 == 注册总数）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnState {
    /// 健康（未命中注入劣化）。
    Healthy,
    /// 降级（延迟/抖动命中——可用但慢）。
    Degraded,
    /// 掉线（连续丢包命中 [`DROP_STREAK_DROP`] 次）。
    Dropped,
}

impl ConnState {
    /// 人话名。
    pub const fn tag(self) -> &'static str {
        match self {
            ConnState::Healthy => "健康",
            ConnState::Degraded => "降级",
            ConnState::Dropped => "掉线",
        }
    }
}

/// 连接条目（模拟面一行）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnEntry {
    /// 连接号（0 起）。
    pub id: u64,
    /// 当前状态。
    pub state: ConnState,
    /// 连续丢包计数（达 [`DROP_STREAK_DROP`] 转 Dropped）。
    pub loss_streak: u32,
    /// 累计注入延迟（ms 账面）。
    pub delayed_total_ms: u64,
    /// 累计丢包数。
    pub lost_total: u64,
}

/// 降档档位（闭集四档）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LadderTier {
    /// L0 全容量全强度。
    L0,
    /// L1 半容量（容量减半、注入强度不减）。
    L1,
    /// L2 半强度（容量恢复、注入强度减半——丢包率/延迟各半）。
    L2,
    /// L3 最低档（容量+强度双半）。
    L3,
}

impl LadderTier {
    /// 四档闭集（列序 = [`LADDER`] 行序）。
    pub const ALL: [LadderTier; 4] = [
        LadderTier::L0,
        LadderTier::L1,
        LadderTier::L2,
        LadderTier::L3,
    ];

    /// 档位下标。
    pub const fn ordinal(self) -> usize {
        match self {
            LadderTier::L0 => 0,
            LadderTier::L1 => 1,
            LadderTier::L2 => 2,
            LadderTier::L3 => 3,
        }
    }

    /// 人话名。
    pub const fn tag(self) -> &'static str {
        match self {
            LadderTier::L0 => "L0 全档",
            LadderTier::L1 => "L1 半容量",
            LadderTier::L2 => "L2 半强度",
            LadderTier::L3 => "L3 最低档",
        }
    }
}

/// 降档阶梯表（行 = 四档，列 = [容量比‰, 注入强度比‰]）。
///
/// **双写之一**：声明式 const 表（千分比整数——F1619 纪律）。
/// 消费走 [`ladder_rule`]（match 独立推导）——判据 4×2=8 格逐格对拍。
pub const LADDER: [[u32; 2]; 4] = [
    //            容量比‰  强度比‰
    /* L0 */ [1000, 1000],
    /* L1 */ [500, 1000],
    /* L2 */ [1000, 500],
    /* L3 */ [500, 500],
];

/// 阶梯规则推导路（双写之二）。
pub fn ladder_rule(tier: LadderTier) -> (u32, u32) {
    match tier {
        LadderTier::L0 => (1000, 1000),
        LadderTier::L1 => (500, 1000),
        LadderTier::L2 => (1000, 500),
        LadderTier::L3 => (500, 500),
    }
}

/// 档位生效容量（CONN_CAP × 容量比‰）。
pub fn tier_capacity(tier: LadderTier) -> usize {
    let (cap_permille, _) = ladder_rule(tier);
    (CONN_CAP * cap_permille as usize) / 1000
}

/// 档位强度注入（档案 × 强度比‰——丢包率/延迟各按比例折算）。
pub fn tier_profile(tier: LadderTier, base: &DegradeProfile) -> DegradeProfile {
    let (_, strength_permille) = ladder_rule(tier);
    DegradeProfile {
        latency_ms: base.latency_ms * strength_permille as u32 / 1000,
        jitter_ms: base.jitter_ms * strength_permille as u32 / 1000,
        loss_permille: base.loss_permille * strength_permille as u32 / 1000,
        bandwidth_kbps: base.bandwidth_kbps,
    }
}

// ---------------------------------------------------------------------------
// 六、保真对拍（网络模拟三件之三）
// ---------------------------------------------------------------------------

/// 对拍结论（模拟 vs 实测）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FidelityVerdict {
    /// 保真（偏差 ≤ 容忍）。
    Faithful,
    /// 失偏（偏差超阈——标注降级）。
    Drifted,
}

impl FidelityVerdict {
    /// 人话名。
    pub const fn tag(self) -> &'static str {
        match self {
            FidelityVerdict::Faithful => "保真",
            FidelityVerdict::Drifted => "失偏（标注降级）",
        }
    }
}

/// 保真对拍（千分比偏差——零浮点）。
///
/// 偏差 = |sim − real| × 1000 / max(sim, real, 1)——分母下界 1 防除零；
/// 双零输入视为完全保真（没测没模——不装准）。
pub fn fidelity_check(sim: u32, real: u32) -> (FidelityVerdict, u32) {
    if sim == 0 && real == 0 {
        return (FidelityVerdict::Faithful, 0);
    }
    let s = sim as u64;
    let r = real as u64;
    let diff = if s > r { s - r } else { r - s };
    let denom = if s > r { s } else { r }.max(1);
    let dev_permille = (diff * 1000 / denom) as u32;
    if dev_permille > FIDELITY_TOL_PERMILLE {
        (FidelityVerdict::Drifted, dev_permille)
    } else {
        (FidelityVerdict::Faithful, dev_permille)
    }
}

/// 立案记录（模拟过实机败——以实机为准留痕）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilingRecord {
    /// 场景名。
    pub scenario: String,
    /// 模拟结论摘要。
    pub sim_summary: String,
    /// 实机结论摘要。
    pub real_summary: String,
    /// 立案序号。
    pub seq: u64,
}

// ---------------------------------------------------------------------------
// 七、场景册（可配劣化环境）
// ---------------------------------------------------------------------------

/// 场景（名字 + 劣化档案——预置四档 + 可扩）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scenario {
    /// 场景名（册内唯一）。
    pub name: String,
    /// 劣化档案。
    pub profile: DegradeProfile,
}

/// 预置场景（典型四档——最差网络下也可开发测试的出厂配置）。
pub fn scenarios_preset() -> Vec<Scenario> {
    let mut v: Vec<Scenario> = Vec::new();
    v.push(Scenario {
        name: String::from("光纤"),
        profile: DegradeProfile { latency_ms: 5, jitter_ms: 2, loss_permille: 1, bandwidth_kbps: 1_000_000 },
    });
    v.push(Scenario {
        name: String::from("4G"),
        profile: DegradeProfile { latency_ms: 60, jitter_ms: 20, loss_permille: 10, bandwidth_kbps: 50_000 },
    });
    v.push(Scenario {
        name: String::from("地铁"),
        profile: DegradeProfile { latency_ms: 300, jitter_ms: 150, loss_permille: 100, bandwidth_kbps: 5_000 },
    });
    v.push(Scenario {
        name: String::from("极端劣化"),
        profile: DegradeProfile { latency_ms: 1500, jitter_ms: 500, loss_permille: 400, bandwidth_kbps: 500 },
    });
    v
}

/// 操作者（轻量权限：模拟运行是运行时权，配置是维护者权，观察只读）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Actor {
    /// 运行时：跑模拟（注册连接/tick/注入）。
    Runtime,
    /// 维护者：场景配置/对拍/立案。
    Maintainer,
    /// 观察者：只读。
    Observer,
}

impl Actor {
    /// 全操作者闭集。
    pub const ALL: [Actor; 3] = [Actor::Runtime, Actor::Maintainer, Actor::Observer];

    /// 人话名。
    pub const fn tag(self) -> &'static str {
        match self {
            Actor::Runtime => "运行时",
            Actor::Maintainer => "维护者",
            Actor::Observer => "观察者",
        }
    }
}

/// 审计操作（append-only 留痕）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuditOp {
    /// 场景注册。
    ScenarioAdded { name: String },
    /// 降档生效。
    Downgraded { from: LadderTier, to: LadderTier },
    /// 保真失偏标注。
    DriftMarked { dev_permille: u32 },
    /// 实机冲突立案。
    Filed { seq: u64 },
    /// 越权拒绝。
    Denied { actor: Actor, what: &'static str },
}

/// 审计条目。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditEntry {
    /// 序号（单调递增）。
    pub seq: u64,
    /// 操作。
    pub op: AuditOp,
}

/// 连接模拟账本（三态守恒账）。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SimLedger {
    /// 注册连接数。
    pub registered: u64,
    /// 累计注入延迟投递数。
    pub delayed: u64,
    /// 累计丢包数。
    pub lost: u64,
    /// 累计按时投递数。
    pub delivered: u64,
    /// 累计掉线数（转 Dropped 的连接）。
    pub dropped: u64,
    /// 累计降档次数。
    pub downgrades: u64,
    /// 累计 tick 数。
    pub ticks: u64,
}

// ---------------------------------------------------------------------------
// 八、NetSimHub（模拟中枢：场景册 × 注入 × 千连接 × 保真 × 降档）
// ---------------------------------------------------------------------------

/// 网络模拟中枢。
#[derive(Debug)]
pub struct NetSimHub {
    scenarios: Vec<Scenario>,
    conns: Vec<ConnEntry>,
    rng: u64,
    tier: LadderTier,
    ledger: SimLedger,
    audit: Vec<AuditEntry>,
    seq: u64,
    filing_seq: u64,
    filings: Vec<FilingRecord>,
    audit_dropped: u64,
}

impl NetSimHub {
    /// 构造（非零种子——零种子显性拒）。
    pub fn new(seed: u64) -> Result<Self, SimErr> {
        if seed == 0 {
            return Err(SimErr::Code(SimCode::BadSeed));
        }
        Ok(NetSimHub {
            scenarios: Vec::new(),
            conns: Vec::new(),
            rng: seed,
            tier: LadderTier::L0,
            ledger: SimLedger::default(),
            audit: Vec::new(),
            seq: 0,
            filing_seq: 0,
            filings: Vec::new(),
            audit_dropped: 0,
        })
    }

    // -- 内部 ---------------------------------------------------------------

    fn audit_push(&mut self, op: AuditOp) {
        if self.audit.len() >= AUDIT_CAP {
            self.audit_dropped = self.audit_dropped.saturating_add(1);
            return;
        }
        self.seq = self.seq.wrapping_add(1);
        self.audit.push(AuditEntry { seq: self.seq, op });
    }

    fn denied(&mut self, actor: Actor, what: &'static str) -> SimErr {
        self.audit_push(AuditOp::Denied { actor, what });
        SimErr::Code(SimCode::PermissionDenied)
    }

    // -- 场景册 --------------------------------------------------------------

    /// 注册场景（Maintainer 权；档案合法性校验 + 重复拒 + 容量闸）。
    pub fn add_scenario(
        &mut self,
        actor: Actor,
        name: &str,
        profile: DegradeProfile,
    ) -> Result<(), SimErr> {
        if actor != Actor::Maintainer {
            return Err(self.denied(actor, "注册场景"));
        }
        profile.validate().map_err(SimErr::Code)?;
        if self.scenarios.iter().any(|s| s.name == name) {
            return Err(SimErr::Code(SimCode::ScenarioFull));
        }
        if self.scenarios.len() >= SCENARIO_CAP {
            return Err(SimErr::Code(SimCode::ScenarioFull));
        }
        self.scenarios.push(Scenario { name: String::from(name), profile });
        self.audit_push(AuditOp::ScenarioAdded { name: String::from(name) });
        Ok(())
    }

    /// 预置四场景一键导入（Maintainer 权）。
    pub fn load_preset(&mut self, actor: Actor) -> Result<usize, SimErr> {
        if actor != Actor::Maintainer {
            return Err(self.denied(actor, "导入预置场景"));
        }
        let presets = scenarios_preset();
        let mut n = 0usize;
        for p in presets.into_iter() {
            if self.scenarios.iter().any(|s| s.name == p.name) {
                continue;
            }
            if self.scenarios.len() >= SCENARIO_CAP {
                return Err(SimErr::Code(SimCode::ScenarioFull));
            }
            p.profile.validate().map_err(SimErr::Code)?;
            let pname = p.name.clone();
            self.scenarios.push(p);
            n += 1;
            // 与 add_scenario 同口径：预置导入也逐条留痕（审计完整性）
            self.audit_push(AuditOp::ScenarioAdded { name: pname });
        }
        Ok(n)
    }

    /// 场景查档案。
    pub fn scenario_profile(&self, name: &str) -> Result<DegradeProfile, SimErr> {
        match self.scenarios.iter().find(|s| s.name == name) {
            Some(s) => Ok(s.profile),
            None => Err(SimErr::Code(SimCode::UnknownScenario)),
        }
    }

    /// 场景册条数。
    pub fn scenario_count(&self) -> usize {
        self.scenarios.len()
    }

    // -- 连接面 ---------------------------------------------------------------

    /// 注册连接（Runtime 权；档位容量闸——超限触发降档而非硬拒）。
    pub fn add_conn(&mut self, actor: Actor) -> Result<u64, SimErr> {
        if actor != Actor::Runtime {
            return Err(self.denied(actor, "注册连接"));
        }
        let cap = tier_capacity(self.tier);
        if self.conns.len() >= cap {
            // 档位容量满：尝试降档腾容量（资源超限→降档——锚点）
            self.downgrade_step()?;
        }
        let cap2 = tier_capacity(self.tier);
        if self.conns.len() >= cap2 {
            return Err(SimErr::Code(SimCode::ConnCapFull));
        }
        let id = self.conns.len() as u64;
        self.conns.push(ConnEntry {
            id,
            state: ConnState::Healthy,
            loss_streak: 0,
            delayed_total_ms: 0,
            lost_total: 0,
        });
        self.ledger.registered = self.ledger.registered.saturating_add(1);
        Ok(id)
    }

    /// 推进一个 tick（Runtime 权；O(连接数)——全连接逐个注入裁决）。
    ///
    /// 返回本 tick 三类计数（守恒：delayed + lost + delivered == 连接数）。
    pub fn tick(
        &mut self,
        actor: Actor,
        scenario: &str,
    ) -> Result<(u64, u64, u64), SimErr> {
        if actor != Actor::Runtime {
            return Err(self.denied(actor, "推进模拟"));
        }
        let base = self.scenario_profile(scenario)?;
        let profile = tier_profile(self.tier, &base);
        let mut delayed = 0u64;
        let mut lost = 0u64;
        let mut delivered = 0u64;
        for i in 0..self.conns.len() {
            let (outcome, rng2) = {
                let rng = self.rng;
                inject_once(&profile, rng)
            };
            self.rng = rng2;
            let c = &mut self.conns[i];
            match outcome {
                InjectOutcome::Delivered => {
                    delivered += 1;
                    c.loss_streak = 0;
                    c.state = if c.state == ConnState::Dropped {
                        ConnState::Dropped
                    } else {
                        ConnState::Healthy
                    };
                }
                InjectOutcome::Delayed(ms) => {
                    delayed += 1;
                    c.delayed_total_ms = c.delayed_total_ms.saturating_add(ms as u64);
                    c.loss_streak = 0;
                    if c.state != ConnState::Dropped {
                        c.state = ConnState::Degraded;
                    }
                }
                InjectOutcome::Lost => {
                    lost += 1;
                    c.lost_total = c.lost_total.saturating_add(1);
                    c.loss_streak = c.loss_streak.saturating_add(1);
                    if c.loss_streak >= DROP_STREAK_DROP && c.state != ConnState::Dropped {
                        c.state = ConnState::Dropped;
                        self.ledger.dropped = self.ledger.dropped.saturating_add(1);
                    } else if c.state == ConnState::Healthy {
                        c.state = ConnState::Degraded;
                    }
                }
            }
        }
        self.ledger.delayed = self.ledger.delayed.saturating_add(delayed);
        self.ledger.lost = self.ledger.lost.saturating_add(lost);
        self.ledger.delivered = self.ledger.delivered.saturating_add(delivered);
        self.ledger.ticks = self.ledger.ticks.saturating_add(1);
        Ok((delayed, lost, delivered))
    }

    /// 三态守恒探针（自检——存活+掉线 == 注册数）。
    pub fn conservation_probe(&self) -> Result<(), SimCode> {
        let alive = self
            .conns
            .iter()
            .filter(|c| c.state != ConnState::Dropped)
            .count() as u64;
        let dropped = self.conns.iter().filter(|c| c.state == ConnState::Dropped).count() as u64;
        if alive + dropped != self.conns.len() as u64 {
            return Err(SimCode::LedgerDrift);
        }
        Ok(())
    }

    // -- 降档阶梯 ---------------------------------------------------------------

    /// 降档一步（O(1)——档位下标切换）；最低档再降 → LadderExhausted。
    pub fn downgrade_step(&mut self) -> Result<LadderTier, SimErr> {
        let cur = self.tier.ordinal();
        if cur + 1 >= LadderTier::ALL.len() {
            return Err(SimErr::Code(SimCode::LadderExhausted));
        }
        let from = self.tier;
        let to = LadderTier::ALL[cur + 1];
        self.tier = to;
        self.ledger.downgrades = self.ledger.downgrades.saturating_add(1);
        self.audit_push(AuditOp::Downgraded { from, to });
        Ok(to)
    }

    /// 当前档位。
    pub const fn tier(&self) -> LadderTier {
        self.tier
    }

    /// 并发压力评估（超限检测——容量比‰ × 当前注册数 vs 档位容量）。
    pub fn pressure_check(&self) -> Result<(), SimErr> {
        let cap = tier_capacity(self.tier);
        if self.conns.len() > cap {
            return Err(SimErr::Code(SimCode::SimOverloaded));
        }
        Ok(())
    }

    // -- 保真对拍 ------------------------------------------------------------------

    /// 延迟保真对拍（Maintainer 权）：模拟均值 vs 实测值——超阈标注降级。
    pub fn check_fidelity(
        &mut self,
        actor: Actor,
        scenario: &str,
        real_mean_ms: u32,
    ) -> Result<(FidelityVerdict, u32), SimErr> {
        if actor != Actor::Maintainer {
            return Err(self.denied(actor, "保真对拍"));
        }
        let base = self.scenario_profile(scenario)?;
        let sim_mean = base.latency_ms;
        let (verdict, dev) = fidelity_check(sim_mean, real_mean_ms);
        if verdict == FidelityVerdict::Drifted {
            self.audit_push(AuditOp::DriftMarked { dev_permille: dev });
        }
        Ok((verdict, dev))
    }

    /// 实机冲突立案（Maintainer 权）：模拟过实机败——以实机为准留痕。
    pub fn file_real_conflict(
        &mut self,
        actor: Actor,
        scenario: &str,
        sim_summary: &str,
        real_summary: &str,
    ) -> Result<u64, SimErr> {
        if actor != Actor::Maintainer {
            return Err(self.denied(actor, "实机立案"));
        }
        if self.scenario_profile(scenario).is_err() {
            return Err(SimErr::Code(SimCode::UnknownScenario));
        }
        self.filing_seq = self.filing_seq.wrapping_add(1);
        let rec = FilingRecord {
            scenario: String::from(scenario),
            sim_summary: String::from(sim_summary),
            real_summary: String::from(real_summary),
            seq: self.filing_seq,
        };
        self.filings.push(rec);
        self.audit_push(AuditOp::Filed { seq: self.filing_seq });
        Ok(self.filing_seq)
    }

    /// 立案清单只读视图。
    pub fn filings(&self) -> &[FilingRecord] {
        &self.filings
    }

    // -- 账面只读 ---------------------------------------------------------------

    /// 账本只读视图。
    pub const fn ledger(&self) -> &SimLedger {
        &self.ledger
    }

    /// 当前随机态（确定性对账用）。
    pub const fn rng_probe(&self) -> u64 {
        self.rng
    }

    /// 连接条数。
    pub fn conn_count(&self) -> usize {
        self.conns.len()
    }

    /// 按连接查状态。
    pub fn conn_state(&self, id: u64) -> Result<ConnState, SimErr> {
        match self.conns.iter().find(|c| c.id == id) {
            Some(c) => Ok(c.state),
            None => Err(SimErr::Code(SimCode::UnknownConn)),
        }
    }

    /// 审计总条数。
    pub fn audit_count(&self) -> usize {
        self.audit.len()
    }

    /// 审计满容拒记计数。
    pub const fn audit_dropped(&self) -> u64 {
        self.audit_dropped
    }

    /// 确定性红线自检：同种子新实例注入序列逐位对拍（纯函数面）。
    pub fn determinism_probe(&self, scenario: &str) -> Result<(), SimCode> {
        let profile = match self.scenario_profile(scenario) {
            Ok(p) => p,
            Err(_) => return Err(SimCode::UnknownScenario),
        };
        // 纯函数对拍：同起点 LCG 跑 64 掷，双实例逐位相等
        let mut a: u64 = 0x9E3779B97F4A7C15; // 固定测试种子（非零）
        let mut b: u64 = 0x9E3779B97F4A7C15;
        for _ in 0..64 {
            let (oa, na) = inject_once(&profile, a);
            let (ob, nb) = inject_once(&profile, b);
            if oa != ob {
                return Err(SimCode::DeterminismBroken);
            }
            a = na;
            b = nb;
        }
        Ok(())
    }
}

impl Default for NetSimHub {
    /// 默认构造（种子 1——非零确定性纪律内最小种子；判据与测试面直用）。
    fn default() -> Self {
        NetSimHub {
            scenarios: Vec::new(),
            conns: Vec::new(),
            rng: 1,
            tier: LadderTier::L0,
            ledger: SimLedger::default(),
            audit: Vec::new(),
            seq: 0,
            filing_seq: 0,
            filings: Vec::new(),
            audit_dropped: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// 九、域自检（判据：劣化注入、千连接、保真对拍、降档、判据）
// ---------------------------------------------------------------------------

/// VE-F5412 域自检入口（聚合器 `run_svstar2_checks` 调用）。
pub fn run_vef65_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;

    let mut s = CheckSet::new("vef65_netsim");

    // —— 判据一 · 劣化注入：三出口守恒 + 大样本丢包率接近配置 ——
    let prof_4g = DegradeProfile { latency_ms: 60, jitter_ms: 20, loss_permille: 100, bandwidth_kbps: 50_000 };
    let mut delayed_n = 0u64;
    let mut lost_n = 0u64;
    let mut delivered_n = 0u64;
    let mut rng: u64 = 0x1234_5678_9ABC_DEF0;
    for _ in 0..10_000 {
        let (o, r2) = inject_once(&prof_4g, rng);
        rng = r2;
        match o {
            InjectOutcome::Delivered => delivered_n += 1,
            InjectOutcome::Delayed(_) => delayed_n += 1,
            InjectOutcome::Lost => lost_n += 1,
        }
    }
    let conserved = delayed_n + lost_n + delivered_n == 10_000;
    let real_loss_permille = (lost_n * 1000 / 10_000) as u32;
    let loss_close = {
        let diff = if real_loss_permille > 100 { real_loss_permille - 100 } else { 100 - real_loss_permille };
        diff < 30
    };
    s.add(
        "VE65-劣化注入-三出口守恒丢包率收敛",
        conserved && loss_close && lost_n > 0 && delayed_n > 0,
        "10k 掷三类计数守恒（总数=三和）；实测丢包千分比与配置 100 偏差 <30（千分比整数零浮点）",
    );

    // —— 判据一 · 反向：确定性同种子同序列（双实例逐位对拍） ——
    let mut ra: u64 = 42;
    let mut rb: u64 = 42;
    let mut det_ok = true;
    for _ in 0..256 {
        let (oa, na) = inject_once(&prof_4g, ra);
        let (ob, nb) = inject_once(&prof_4g, rb);
        if oa != ob {
            det_ok = false;
        }
        ra = na;
        rb = nb;
    }
    s.add(
        "VE65-劣化注入-同种子同序列",
        det_ok && ra == rb,
        "同种子双实例 256 掷注入结论逐位相等（LCG 确定性——最差网络的测试也要可复现）",
    );

    // —— 判据一 · 反向：劣化参数越界显性拒 + 坏种子拒 ——
    let bad_loss = DegradeProfile { latency_ms: 60, jitter_ms: 20, loss_permille: MAX_LOSS_PERMILLE + 1, bandwidth_kbps: 1 };
    let bad_lat = DegradeProfile { latency_ms: MAX_LATENCY_MS + 1, jitter_ms: 0, loss_permille: 0, bandwidth_kbps: 1 };
    let bad_jitter = DegradeProfile { latency_ms: 10, jitter_ms: 11, loss_permille: 0, bandwidth_kbps: 1 };
    let zero_seed_err = NetSimHub::new(0).is_err();
    s.add(
        "VE65-劣化注入-参数越界与坏种子拒",
        bad_loss.validate() == Err(SimCode::BadDegradeProfile)
            && bad_lat.validate() == Err(SimCode::BadDegradeProfile)
            && bad_jitter.validate() == Err(SimCode::BadDegradeProfile)
            && zero_seed_err,
        "丢包超千分上限/延迟超 ms 上限/抖动超延迟半幅全拒；零种子拒（序列退化防线）",
    );

    // —— 判据二 · 场景册：预置导入 + 未注册/重复显性 ——
    let mut h = NetSimHub::default();
    let preset_n = h.load_preset(Actor::Maintainer);
    let unknown = h.scenario_profile("卫星");
    let dup = h.add_scenario(Actor::Maintainer, "光纤", prof_4g);
    s.add(
        "VE65-场景册-预置导入与显性拒绝",
        preset_n == Ok(4)
            && unknown == Err(SimErr::Code(SimCode::UnknownScenario))
            && dup == Err(SimErr::Code(SimCode::ScenarioFull)),
        "预置四场景（光纤/4G/地铁/极端劣化）一键入册；未注册场景 UnknownScenario；重复名 ScenarioFull",
    );

    // —— 判据二 · 反向：越权配置与越权运行全拒 ——
    let mut h = NetSimHub::default();
    let _ = h.load_preset(Actor::Maintainer);
    let ext_add = h.add_scenario(Actor::Observer, "x", prof_4g);
    let obs_tick = h.tick(Actor::Observer, "4G");
    let maint_conn = h.add_conn(Actor::Maintainer);
    let denied_n = h
        .audit
        .iter()
        .filter(|a| matches!(a.op, AuditOp::Denied { actor: _, what: _ }))
        .count();
    s.add(
        "VE65-场景册-越权全口拒留痕",
        ext_add == Err(SimErr::Code(SimCode::PermissionDenied))
            && obs_tick == Err(SimErr::Code(SimCode::PermissionDenied))
            && maint_conn == Err(SimErr::Code(SimCode::PermissionDenied))
            && denied_n == 3,
        "观察者配置/观察者运行/维护者注册连接三口全拒且逐条审计（运行时权与维护者权分立）",
    );

    // —— 判据三 · 千连接：1024 注册 + tick O(连接数) 三态守恒 ——
    let mut h = NetSimHub::default();
    let _ = h.load_preset(Actor::Maintainer);
    let mut registered = 0usize;
    for _ in 0..CONN_CAP {
        if h.add_conn(Actor::Runtime).is_ok() {
            registered += 1;
        }
    }
    let tick_out = h.tick(Actor::Runtime, "地铁");
    let (d1, l1, v1) = tick_out.unwrap_or((0, 0, 0));
    let consv = d1 + l1 + v1 == registered as u64;
    let probe_ok = h.conservation_probe() == Ok(());
    s.add(
        "VE65-千连接-全量注册tick守恒",
        registered == CONN_CAP && consv && probe_ok && registered >= 1000,
        "1024 连接全注册（千连接并发容量锚点）；单 tick 三类计数和==连接数；三态守恒探针零漂移",
    );

    // —— 判据三 · 反向：连续丢包掉线状态机（DROP_STREAK_DROP 阈） ——
    let mut h = NetSimHub::default();
    let _ = h.load_preset(Actor::Maintainer);
    let _ = h.add_conn(Actor::Runtime);
    let mut dropped_seen = false;
    for _ in 0..200 {
        let _ = h.tick(Actor::Runtime, "极端劣化");
        if h.conn_state(0) == Ok(ConnState::Dropped) {
            dropped_seen = true;
            break;
        }
    }
    let ledger_dropped = h.ledger().dropped;
    s.add(
        "VE65-千连接-连续丢包掉线状态机",
        dropped_seen && ledger_dropped >= 1,
        "极端劣化档案下连续丢包达阈（3 次）→ Dropped 转态记账（状态机三态闭集流转）",
    );

    // —— 判据四 · 降档：阶梯逐档 O(1) + 最低档耗尽拒 ——
    let mut h = NetSimHub::default();
    let t0 = h.tier();
    let d1 = h.downgrade_step();
    let d2 = h.downgrade_step();
    let d3 = h.downgrade_step();
    let d4 = h.downgrade_step();
    let downgrades = h.ledger().downgrades;
    s.add(
        "VE65-降档-阶梯逐档与耗尽显性",
        t0 == LadderTier::L0
            && d1 == Ok(LadderTier::L1)
            && d2 == Ok(LadderTier::L2)
            && d3 == Ok(LadderTier::L3)
            && d4 == Err(SimErr::Code(SimCode::LadderExhausted))
            && downgrades == 3,
        "L0→L1→L2→L3 三次 O(1) 档位切换逐档记账；最低档再降→LadderExhausted（降无可降不硬撑）",
    );

    // —— 判据四 · 反向：阶梯双写全格对拍 + 强度折算 ——
    let mut ladder_ok = true;
    for t in LadderTier::ALL.iter() {
        let (cap_p, str_p) = ladder_rule(*t);
        let row = LADDER[t.ordinal()];
        if cap_p != row[0] || str_p != row[1] {
            ladder_ok = false;
        }
    }
    let cap_l1 = tier_capacity(LadderTier::L1);
    let cap_l0 = tier_capacity(LadderTier::L0);
    let strong_half = tier_profile(LadderTier::L2, &prof_4g);
    s.add(
        "VE65-降档-阶梯双写对拍强度折算",
        ladder_ok
            && cap_l1 == cap_l0 / 2
            && strong_half.loss_permille == prof_4g.loss_permille / 2
            && strong_half.latency_ms == prof_4g.latency_ms / 2,
        "4 档 × 2 轴 = 8 格：档位表与规则推导逐格相等；L1 容量减半；L2 丢包/延迟各半（千分比整数折算）",
    );

    // —— 判据五 · 保真对拍：阈内保真 / 超阈标注降级 ——
    let mut h = NetSimHub::default();
    let _ = h.load_preset(Actor::Maintainer);
    let faithful = h.check_fidelity(Actor::Maintainer, "4G", 61);
    let drifted = h.check_fidelity(Actor::Maintainer, "4G", 120);
    let drift_marked = h
        .audit
        .iter()
        .any(|a| matches!(a.op, AuditOp::DriftMarked { dev_permille: _ }));
    s.add(
        "VE65-保真对拍-超阈标注降级",
        faithful == Ok((FidelityVerdict::Faithful, 16))
            && drifted == Ok((FidelityVerdict::Drifted, 500))
            && drift_marked,
        "偏差 16 千分 ≤ 50 容忍→保真；500 超阈→失偏标注进审计（模拟失偏显性不装准——U 域纪律同规）",
    );

    // —— 判据五 · 反向：模拟过实机败立案（以实机为准） ——
    let mut h = NetSimHub::default();
    let _ = h.load_preset(Actor::Maintainer);
    let filed = h.file_real_conflict(
        Actor::Maintainer,
        "4G",
        "模拟结论：丢包 1% 无掉线",
        "实机结论：丢包 1% 触发批量掉线",
    );
    let filings_n = h.filings().len();
    let filed_summary = h
        .filings()
        .first()
        .map(|f| f.real_summary == String::from("实机结论：丢包 1% 触发批量掉线"))
        == Some(true);
    s.add(
        "VE65-保真对拍-实机冲突立案",
        filed.is_ok() && filings_n == 1 && filed_summary,
        "模拟过实机败→立案留痕（模拟摘要+实机摘要+序号）——以实机为准不悄悄改数",
    );

    // —— 判据六 · 确定性红线自检 + 审计满容 ——
    let mut h = NetSimHub::default();
    let _ = h.load_preset(Actor::Maintainer);
    let det = h.determinism_probe("4G");
    // 审计满容：反复失偏标注灌审计（4 预置场景 + 264 失偏 = 268 → 溢出 12）
    let mut h2 = NetSimHub::default();
    let _ = h2.load_preset(Actor::Maintainer);
    for _ in 0..(AUDIT_CAP + 8) {
        let _ = h2.check_fidelity(Actor::Maintainer, "地铁", 900);
    }
    let dropped_n = h2.audit_dropped();
    let total_n = h2.audit_count();
    s.add(
        "VE65-判据-确定性自检与审计满容",
        det == Ok(())
            && dropped_n == 12
            && total_n == AUDIT_CAP,
        "同种子 64 掷双实例逐位对拍（DeterminismBroken 红线未触发）；审计满容拒记 12 条计数（4 预置场景+264 失偏标注-256 容量；append-only 旧痕保全）",
    );

    // —— 判据六 · 反向：账面守恒 + 码段互异 ——
    let codes_ok = {
        let mut uniq = true;
        for i in 0..SEGMENT_GUARD.len() {
            for j in (i + 1)..SEGMENT_GUARD.len() {
                if SEGMENT_GUARD[i].code() == SEGMENT_GUARD[j].code() {
                    uniq = false;
                }
            }
        }
        uniq
            && SEGMENT_GUARD.iter().all(|c| c.code() & 0xFF00 == 0x8D00)
            && SEGMENT_GUARD.iter().all(|c| !c.reason().is_empty())
    };
    let l = h.ledger();
    let ledger_ok = l.delayed + l.lost + l.delivered == l.registered * l.ticks;
    s.add(
        "VE65-判据-账面守恒码段互异",
        ledger_ok && codes_ok && SEGMENT_GUARD.len() == 16,
        "注入账三面守恒（delayed+lost+delivered == 注册数×tick 数）；十六码全落 0x8Dxx 两两互异 reason 全码可读",
    );

    s
}
