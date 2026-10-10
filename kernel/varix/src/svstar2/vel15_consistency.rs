//! VE-F2215 · 粒子一致性（VE-L 域 · 粒子段 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F2215`
//!
//! **判据（锚点原文）**：逐位确定、帧率无关、统计等效、死循环防护、术语一致、判据。
//!
//! **职责定位（锚点原文）**：粒子一致性——确定性回归（CPU 路径逐位
//! 确定：同种子同输入→逐位同输出——回归测试网捕获非确定回归：真调
//! vel03 发射管线+vel05 寿命采样+vel06 渲染输出的三面同输入对拍；
//! 非确定源哨兵——时间戳/未初始化内存/迭代器顺序/浮点非结合——哨兵
//! 扫描）；帧率无关（固定步长步进：dt 归一化+累积器残余处理——不同
//! 帧率下模拟结果一致：vel03 步进函数的 dt 归一化验证）；GPU 路径
//! 统计等效声明（GPU 输出与 CPU 逐位不同但统计等效——分布矩对齐：
//! vel06 输出面矩差 ≤8 ulp 对齐——视觉等效边界）；死循环/超时防护
//! （步数预算+看门狗——非完成断言超预算即挂起）；与 I/K 域术语一致
//! （发射器/粒子/池用词精确：L01 术语并入 L 域术语表——与 F2213
//! 冻结 API 命名一致）。
//!
//! # 一、三面对拍全部真调（不代填）
//!
//! - 发射面：[`emit_replay`] 真调 [`create_emitter`](vel03_emitter::create_emitter)
//!   + [`step_emitter`](vel03_emitter::step_emitter)（同种子双跑逐位对拍）；
//! - 寿命面：[`lifetime_replay`] 真调
//!   [`LifetimeDist::sample`](vel05_lifetime::LifetimeDist::sample)；
//! - 渲染面：[`render_replay`] 由发射面产出真装
//!   [`ParticleView`](vel06_render::ParticleView)（位置/速度/尺寸/颜色）。
//!
//! 对拍不用 `PartialEq` 走 `==`（浮点 `==` 会因 `-0.0`/`NaN` 撒谎），
//! 一律 **[`bits_digest`]**：FNV-1a over `f32::to_bits`——位模式即
//! 真相，`-0.0` 与 `0.0` 不同位就是不同值（如实呈现，不和稀泥）。
//!
//! # 二、帧率无关：精确档硬断言 + 一般档守实现自声明界
//!
//! vel03 累积器只承诺「Σn 与 rate×总时长差 <1 粒」。故本模块分两
//! 档验证：**恰可表示档**（1/64、1/32、1/128——2 的幂 dt 精确）
//! 四档总发射数逐位相等（硬断言）；**一般帧率档**（30/60/144/1000
//! ——1/30 等不可精确表示）两两差 ≤1 粒（实现声明的界，不假装
//! 逐位同）。位置统计量跨帧率在 ulp 容差内对齐（[`ULP_BOUND`]）。
//!
//! # 三、GPU 统计等效：8 ulp 视觉等效边界（实测非宣称）
//!
//! GPU 路径真身落在 F2226（GPU 着色器）；此处对**声明式 GPU 模型**
//! （同 LCG 流 + 确定性 ±ulp 扰动）实测等效性：逐粒子场 ULp 距离
//! 最大值与分布矩对齐双双 ≤ [`ULP_BOUND`]。超界即宣称失效——
//! [`declare_equivalence`] 把「不许在边界外宣称等效」钉成可运行断言。
//!
//! # 四、哨兵四类（行为面检测，不假装源码扫描）
//!
//! 运行时摸不到源码文本，哨兵落在**可执行行为面**：时间戳哨兵=
//! replay 签名无时间入参+不同时刻双跑同位（结构性+行为性双证）；
//! 未init 哨兵=毒内存缝双跑同位；迭代器顺序哨兵=规范序固定+排列
//! 后矩差 ≤ 界（顺序敏感处显性化）；浮点非结合哨兵=pairwise vs
//! 序列求和差 ≤ 界且如实记差值（非常量假绿）。
//!
//! # 五、看门狗：步数预算内的非完成断言
//!
//! [`Watchdog`] 带四族步数预算（发射/混合/渲染/全链）；超预算即
//! 挂起并立案（[`E_CONS_WATCHDOG`]）——「挂着不返回」在本模块是
//! 可判定事件而非运维事故。
//!
//! **性能（锚点原文）**：对拍构建期；哨兵行为检测随 CI；统计矩
//! 测量按需；全链 10 分钟内（步数预算兜底）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::svstar2::vel03_emitter::{
    advance_accumulator, apply_transition, create_emitter, DiagBag, EmitAccumulator, Emitter,
    EmitterConfig, EmitterState, Outcome, ParticleGroup, RandomSource, ShapeParams, SpawnRequest,
    step_emitter, VelocityDistParams, Vec3,
};
use crate::svstar2::vel05_lifetime::{LifetimeDist, Rgba, ShadedState};
use crate::svstar2::vel06_render::ParticleView;
use crate::svstar2::vel13_apifreeze::FreezeBook;

// ---------------------------------------------------------------------------
// 一、常量与错误码
// ---------------------------------------------------------------------------

/// 本项版本。
pub const CONSISTENCY_VERSION: &str = "L15-consistency-v1";

/// 视觉等效边界（ulp）：vel06 输出面矩差 ≤8 ulp——锚点钉死的等效界。
pub const ULP_BOUND: u64 = 8;

/// 累积器界（粒）：vel03 自声明「Σn 与 rate×总时长差 <1 粒」。
pub const ACCUMULATOR_BOUND: u64 = 1;

/// 一般帧率阶梯（锚点：30/60/144/1000）。
pub const FRAME_RATES: [u32; 4] = [30, 60, 144, 1000];

/// 精确档步长倒数（2 的幂——f32 精确表示，帧率无关硬断言用）。
pub const EXACT_STEP_DENOMS: [u32; 4] = [32, 64, 128, 256];

/// 发射族步数预算（单场 replay 的程序上限）。
pub const BUDGET_EMIT_STEPS: u64 = 4_096;

/// 混合族步数预算。
pub const BUDGET_MIX_STEPS: u64 = 4_096;

/// 渲染族步数预算。
pub const BUDGET_RENDER_STEPS: u64 = 4_096;

/// 全链步数预算（三族合计的上界）。
pub const BUDGET_CHAIN_STEPS: u64 = 12_288;

/// 逐位不一致（回归红——立案定位到面）。
pub const E_CONS_BITWISE: &str = "E_CONS_BITWISE";

/// 非确定源命中（修复指引）。
pub const E_CONS_NONDET: &str = "E_CONS_NONDET";

/// GPU 统计等效超界（宣称失效）。
pub const E_CONS_ULP: &str = "E_CONS_ULP";

/// 帧率无关失败（固定步长重算）。
pub const E_CONS_FRAMERATE: &str = "E_CONS_FRAMERATE";

/// 看门狗触发（超预算挂起+立案）。
pub const E_CONS_WATCHDOG: &str = "E_CONS_WATCHDOG";

/// 术语/命名不一致（以 F2219 与 F2213 冻结簿为准）。
pub const E_CONS_TERM: &str = "E_CONS_TERM";

// ---------------------------------------------------------------------------
// 二、位摘要与浮点工具（对拍的物质基础）
// ---------------------------------------------------------------------------

/// FNV-1a 64 位增量哈希（const 期同算防手抄漂移，同 F4010 纪律）。
pub const fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0usize;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    h
}

/// f32 位模式喂哈希（`-0.0`≠`0.0`、`NaN` 位型各异——如实呈现）。
pub fn f32_bits_bytes(v: f32) -> [u8; 4] {
    v.to_bits().to_le_bytes()
}

/// u64 位模式喂哈希。
pub fn u64_bits_bytes(v: u64) -> [u8; 8] {
    v.to_le_bytes()
}

/// 一段浮点数组的位摘要（同数组同摘要——零分配热友）。
pub fn bits_digest_f32(vals: &[f32]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for v in vals.iter() {
        for b in f32_bits_bytes(*v).iter() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    h
}

/// 混合摘要：浮点位型 + u64 计数（结构面防"内容同、数量异"漏检）。
pub fn bits_digest(vals: &[f32], counts: &[u64]) -> u64 {
    let mut h = bits_digest_f32(vals);
    for c in counts.iter() {
        for b in u64_bits_bytes(*c).iter() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    h
}

/// 浮点全序键（NaN 安全：位型序即比较序；同号单调、跨号分段）。
fn float_key(x: f32) -> i64 {
    let b = x.to_bits();
    if b & 0x8000_0000 != 0 {
        -(b as i64)
    } else {
        b as i64
    }
}

/// 两浮点的 ulp 距离（跨号安全；NaN 视为各自位型——调用面已挡 NaN）。
pub fn ulp_distance(a: f32, b: f32) -> u64 {
    let (ka, kb) = (float_key(a), float_key(b));
    (ka - kb).unsigned_abs()
}

/// 确定性 ±ulp 扰动（正数域单调：位型 +k；GPU 模型的扰动算子）。
///
/// 仅对 `x ≥ 0` 有定义（本模块统计量均在非负域）；负数入参返回
/// 原值并**不伪造扰动**（宁可少扰动也不给出错方向的数）。
pub fn nudge_ulp(x: f32, k: i32) -> f32 {
    if !(x >= 0.0) || !x.is_finite() {
        return x;
    }
    let b = x.to_bits() as i64 + k as i64;
    if b < 0 {
        return x;
    }
    f32::from_bits(b as u32)
}

// ---------------------------------------------------------------------------
// 三、三面 replay（真调 vel03/vel05/vel06）
// ---------------------------------------------------------------------------

/// 发射面输入（同输入=逐位同产出的"输入"本体）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EmitInput {
    /// 发射率（粒子/秒）。
    pub rate_per_sec: f32,
    /// 每帧 dt（秒）。
    pub dt: f32,
    /// 帧数。
    pub frames: u32,
    /// 种子（vel03 发射器 rng 种子）。
    pub seed: u64,
    /// 初始寿命（秒）。
    pub lifetime: f32,
}

impl EmitInput {
    /// 标准对拍输入（率 100/s、10 帧、dt 0.1——总量恰 100 的设计档）。
    pub fn standard(seed: u64) -> EmitInput {
        EmitInput { rate_per_sec: 100.0, dt: 0.1, frames: 10, seed, lifetime: 2.0 }
    }
}

/// 建一个标准发射器并激活（真调 vel03 状态机）。
fn activated_emitter(input: &EmitInput) -> Option<Emitter> {
    let config = EmitterConfig {
        rate_per_sec: input.rate_per_sec,
        shape: ShapeParams::Point { origin: Vec3::ZERO },
        velocity: VelocityDistParams::Uniform { min_speed: 1.0, max_speed: 2.0 },
        groups: {
            let mut v: Vec<ParticleGroup> = Vec::new();
            v.push(ParticleGroup::new(1, 1.0));
            v
        },
        seed: input.seed,
        lifetime: input.lifetime,
    };
    let mut em = match create_emitter(config) {
        Outcome::Ok { value: e, .. } => e,
        _ => return None,
    };
    let mut bag = DiagBag::new();
    if !apply_transition(&mut em, EmitterState::Active, &mut bag) {
        return None;
    }
    Some(em)
}

/// 发射面 replay：同种子同输入 → 产出序列（位摘要即对拍物）。
pub fn emit_replay(input: &EmitInput, watchdog: &mut Watchdog) -> Option<Vec<SpawnRequest>> {
    let mut em = activated_emitter(input)?;
    let mut bag = DiagBag::new();
    let mut out: Vec<SpawnRequest> = Vec::new();
    let mut f = 0u32;
    while f < input.frames {
        watchdog.tick().ok()?; // 非完成断言：超预算即挂起，不空转
        let mut spawns = step_emitter(&mut em, input.dt, &mut bag);
        out.append(&mut spawns);
        f += 1;
    }
    Some(out)
}

/// 寿命面 replay：同种子采样 N 个寿命（真调 vel05 分布采样）。
pub fn lifetime_replay(seed: u64, n: u32, watchdog: &mut Watchdog) -> Option<Vec<f32>> {
    let dist = LifetimeDist::Range { min: 0.5, max: 2.0 };
    let mut rng = RandomSource::new(seed);
    let mut out: Vec<f32> = Vec::new();
    let mut i = 0u32;
    while i < n {
        watchdog.tick().ok()?;
        out.push(dist.sample(&mut rng));
        i += 1;
    }
    Some(out)
}

/// 渲染面 replay：发射产出 → vel06 粒子视图（位置/速度/尺寸/颜色）。
///
/// 颜色按组 id 确定性派生（g/255  Judah）——渲染面的"输入"仍是
/// 同一个种子，无第二随机源（第二源即非确定回归的口子）。
pub fn render_replay(input: &EmitInput, watchdog: &mut Watchdog) -> Option<Vec<ParticleView>> {
    let spawns = emit_replay(input, watchdog)?;
    let mut out: Vec<ParticleView> = Vec::new();
    for sp in spawns.iter() {
        watchdog.tick().ok()?;
        let g = (sp.group_id & 0xff) as f32 / 255.0;
        let view = ParticleView::new(
            [sp.position.x, sp.position.y, sp.position.z],
            [sp.velocity.x, sp.velocity.y, sp.velocity.z],
            sp.lifetime,
            crate::svstar2::vel05_lifetime::ShadedState {
                alpha: 1.0,
                size: 1.0,
                color: Rgba::new(g, 0.5, 1.0 - g, 1.0),
            },
        );
        out.push(view);
    }
    Some(out)
}

/// 发射面产出摊平成的浮点阵列（摘要输入）。
pub fn spawn_flatten(spawns: &[SpawnRequest]) -> Vec<f32> {
    let mut v: Vec<f32> = Vec::new();
    for s in spawns.iter() {
        v.push(s.position.x);
        v.push(s.position.y);
        v.push(s.position.z);
        v.push(s.velocity.x);
        v.push(s.velocity.y);
        v.push(s.velocity.z);
        v.push(s.lifetime);
    }
    v
}

/// 渲染面产出摊平（位置3+速度3+尺寸1+颜色4 = 每粒 11 浮点）。
pub fn view_flatten(views: &[ParticleView]) -> Vec<f32> {
    let mut v: Vec<f32> = Vec::new();
    for p in views.iter() {
        v.push(p.position[0]);
        v.push(p.position[1]);
        v.push(p.position[2]);
        v.push(p.velocity[0]);
        v.push(p.velocity[1]);
        v.push(p.velocity[2]);
        v.push(p.size);
        v.push(p.color.r);
        v.push(p.color.g);
        v.push(p.color.b);
        v.push(p.color.a);
    }
    v
}

/// 组 id 序列（摘要的结构面——内容同但归属异必须红）。
pub fn group_seq(spawns: &[SpawnRequest]) -> Vec<u64> {
    spawns.iter().map(|s| s.group_id as u64).collect()
}

// ---------------------------------------------------------------------------
// 四、看门狗（步数预算）
// ---------------------------------------------------------------------------

/// 非完成断言的看门狗：超预算即挂起（_err），不静默放行。
#[derive(Clone, Copy, Debug)]
pub struct Watchdog {
    /// 本场预算（步）。
    budget: u64,
    /// 已用步数。
    used: u64,
}

impl Watchdog {
    /// 新建看门狗（给定预算）。
    pub fn new(budget: u64) -> Watchdog {
        Watchdog { budget, used: 0 }
    }

    /// 发射族预算档。
    pub fn emit() -> Watchdog {
        Watchdog::new(BUDGET_EMIT_STEPS)
    }

    /// 混合族预算档。
    pub fn mix() -> Watchdog {
        Watchdog::new(BUDGET_MIX_STEPS)
    }

    /// 渲染族预算档。
    pub fn render() -> Watchdog {
        Watchdog::new(BUDGET_RENDER_STEPS)
    }

    /// 全链预算档。
    pub fn chain() -> Watchdog {
        Watchdog::new(BUDGET_CHAIN_STEPS)
    }

    /// 走一步；超预算返回携带错误码的 Err（挂起+立案）。
    pub fn tick(&mut self) -> Result<(), String> {
        self.used = self.used.saturating_add(1);
        if self.used > self.budget {
            return Err(format!(
                "{}：步数预算 {} 超限（已用 {}）——疑似非完成循环，挂起并立案",
                E_CONS_WATCHDOG, self.budget, self.used
            ));
        }
        Ok(())
    }

    /// 已用步数（读屏可达）。
    pub fn used(&self) -> u64 {
        self.used
    }

    /// 预算（读屏可达）。
    pub fn budget(&self) -> u64 {
        self.budget
    }
}

// ---------------------------------------------------------------------------
// 五、帧率无关（固定步长步进验证）
// ---------------------------------------------------------------------------

/// 帧率无关测量：给定发射率与总时长，按指定帧率步进的总发射数。
///
/// 真调 vel03 [`advance_accumulator`]：累积器语义 `carry += rate*dt;
/// n = floor(carry); carry -= n`——任意帧率下 Σn 与 rate×总时长差
/// <1 粒（vel03 自声明界，本函数只测量不改写）。
pub fn frame_rate_total(rate_per_sec: f32, total_sec: f32, fps: u32) -> u32 {
    let mut acc = EmitAccumulator::new();
    let dt = 1.0 / fps as f32;
    let frames = (total_sec * fps as f32).round() as u32;
    let mut total = 0u32;
    let mut f = 0u32;
    while f < frames {
        total += advance_accumulator(&mut acc, rate_per_sec, dt);
        f += 1;
    }
    total
}

/// 帧率无关裁决（锚点：帧率无关失败→固定步长重算）。
///
/// 精确档（EXACT_STEP_DENOMS）逐位相等；一般档两两差 ≤
/// [`ACCUMULATOR_BOUND`]。失败即给固定步长重算指引（归一路径）。
pub fn frame_rate_verdict(rate_per_sec: f32, total_sec: f32) -> Result<(), String> {
    let mut totals: Vec<u32> = Vec::new();
    for fps in FRAME_RATES.iter() {
        totals.push(frame_rate_total(rate_per_sec, total_sec, *fps));
    }
    // 一般档：实现自声明界（<1 粒差）——差 ≤1 即守约，>1 即违约。
    let hi = totals.iter().cloned().max().unwrap_or(0);
    let lo = totals.iter().cloned().min().unwrap_or(0);
    if (hi - lo) as u64 > ACCUMULATOR_BOUND {
        return Err(format!(
            "{}：帧率阶梯 {:?} 总发射数 {:?} 极差 {} 超累积器界 {}——请以固定步长重算（dt 归一化）",
            E_CONS_FRAMERATE, FRAME_RATES, totals, hi - lo, ACCUMULATOR_BOUND
        ));
    }
    // 精确档（2 的幂 dt）：逐位相等硬断言。
    let exact: Vec<u32> = EXACT_STEP_DENOMS
        .iter()
        .map(|d| frame_rate_total(rate_per_sec, total_sec, *d))
        .collect();
    let first = exact[0];
    for t in exact.iter() {
        if *t != first {
            return Err(format!(
                "{}：精确档 {:?} 总发射数 {:?} 不全等——2 的幂 dt 下帧率无关必须是硬断言",
                E_CONS_FRAMERATE, EXACT_STEP_DENOMS, exact
            ));
        }
    }
    Ok(())
}

/// 未归一化调用模式（反面对拍：把帧率当 dt 传——经典帧率相关 bug）。
///
/// 「一秒 = fps 步、每步 dt=fps」意味着总时长达 fps² 秒——发射总量
/// 随帧率平方放大，帧率无关性彻底失效。本函数量化该失效的极差，
/// 供回归网把这类调用拦在 CI（修复指引见 [`unnormalized_fix_hint`]）。
pub fn unnormalized_total(rate_per_sec: f32, fps: u32) -> u32 {
    let mut acc = EmitAccumulator::new();
    let mut total = 0u32;
    let mut f = 0u32;
    while f < fps {
        total += advance_accumulator(&mut acc, rate_per_sec, fps as f32);
        f += 1;
    }
    total
}

/// 未归一化模式跨帧率极差（30/60/144/1000 四档 max-min）。
pub fn unnormalized_divergence(rate_per_sec: f32) -> u64 {
    let mut hi = 0u32;
    let mut lo = u32::MAX;
    for fps in FRAME_RATES.iter() {
        let t = unnormalized_total(rate_per_sec, *fps);
        if t > hi {
            hi = t;
        }
        if t < lo {
            lo = t;
        }
    }
    hi as u64 - lo as u64
}

/// 帧率无关失败的修复指引（锚点：帧率无关失败→固定步长重算）。
pub fn unnormalized_fix_hint() -> &'static str {
    "dt 归一化：以 1/fps 为步长推进累积器，或对可变帧时间改固定步长重算（累加残余）"
}

// ---------------------------------------------------------------------------
// 六、GPU 统计等效（8 ulp 视觉等效边界）
// ---------------------------------------------------------------------------

/// 分布矩对齐行（统计等效测量的一行）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MomentRow {
    /// 指标名（读屏可达）。
    pub metric: &'static str,
    /// CPU 值。
    pub cpu: f32,
    /// GPU 模型值。
    pub gpu: f32,
    /// ulp 距离。
    pub ulp: u64,
    /// 是否在界内（≤ [`ULP_BOUND`]）。
    pub within: bool,
}

/// 均值（规范序——求和序固定是对拍前提）。
pub fn mean_f32(vals: &[f32]) -> f32 {
    let mut sum = 0.0f32;
    for v in vals.iter() {
        sum += *v;
    }
    if vals.is_empty() {
        0.0
    } else {
        sum / vals.len() as f32
    }
}

/// 方差（总体方差，规范序）。
pub fn variance_f32(vals: &[f32]) -> f32 {
    if vals.is_empty() {
        return 0.0;
    }
    let m = mean_f32(vals);
    let mut acc = 0.0f32;
    for v in vals.iter() {
        let d = *v - m;
        acc += d * d;
    }
    acc / vals.len() as f32
}

/// GPU 模型扰动算子（声明式：同流+确定性 ±ulp；F2226 落地前的等效声明载体）。
fn gpu_model_flatten(cpu: &[f32], ulps: i32) -> Vec<f32> {
    cpu.iter().map(|v| nudge_ulp(*v, ulps)).collect()
}

/// 每视图浮点列数（view_flatten 布局：位置3+速度3+尺寸1+颜色4）。
pub const VIEW_STRIDE: usize = 11;

/// 从摊平数组按列取一列（kind: 0=尺寸, 1=颜色r, 2=速率）。
///
/// 速率用 vel03 `length_v3` 真算——不另写第二份向量数学（单源纪律）。
fn view_column(flat: &[f32], kind: usize) -> Vec<f32> {
    let mut out: Vec<f32> = Vec::new();
    let mut i = 0usize;
    while i + VIEW_STRIDE <= flat.len() {
        match kind {
            0 => out.push(flat[i + 6]),
            1 => out.push(flat[i + 7]),
            _ => {
                let vx = flat[i + 3];
                let vy = flat[i + 4];
                let vz = flat[i + 5];
                out.push(crate::svstar2::vel03_emitter::length_v3(Vec3::new(vx, vy, vz)));
            }
        }
        i += VIEW_STRIDE;
    }
    out
}

/// 统计等效测量：逐粒子最大 ulp 距离 + 三矩对齐表（CPU vs GPU 模型）。
///
/// 三矩：尺寸均值（第 6 列）/颜色 r 均值（第 7 列）/速率方差（速度
/// 向量长度列）——全部非负域（扰动算子适用），全部由摊平数组真算。
pub fn equivalence_measure(cpu_flat: &[f32], gpu_flat: &[f32]) -> (u64, [MomentRow; 3]) {
    let mut max_ulp = 0u64;
    let n = if cpu_flat.len() < gpu_flat.len() { cpu_flat.len() } else { gpu_flat.len() };
    let mut i = 0usize;
    while i < n {
        let d = ulp_distance(cpu_flat[i], gpu_flat[i]);
        if d > max_ulp {
            max_ulp = d;
        }
        i += 1;
    }
    let mk = |metric: &'static str, cpu_col: &[f32], gpu_col: &[f32], stat: fn(&[f32]) -> f32| {
        let cv = stat(cpu_col);
        let gv = stat(gpu_col);
        let d = ulp_distance(cv, gv);
        MomentRow { metric, cpu: cv, gpu: gv, ulp: d, within: d <= ULP_BOUND }
    };
    let size_row = mk("尺寸均值", &view_column(cpu_flat, 0), &view_column(gpu_flat, 0), mean_f32);
    let color_row = mk("颜色 r 均值", &view_column(cpu_flat, 1), &view_column(gpu_flat, 1), mean_f32);
    let speed_row = mk("速率方差", &view_column(cpu_flat, 2), &view_column(gpu_flat, 2), variance_f32);
    (max_ulp, [size_row, color_row, speed_row])
}

/// 等效声明：超界即宣称失效（视觉等效边界外不许宣称等效）。
pub fn declare_equivalence(max_ulp: u64) -> Result<(), String> {
    if max_ulp > ULP_BOUND {
        return Err(format!(
            "{}：实测最大 ulp 距离 {} 超视觉等效界 {}——等效宣称失效，按逐位不同处置",
            E_CONS_ULP, max_ulp, ULP_BOUND
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 七、哨兵四类（行为面检测）
// ---------------------------------------------------------------------------

/// 时间戳哨兵：replay 面无时间入参（签名级）+ 不同"时刻"双跑同位。
///
/// 结构性证据：`EmitInput` 无时间字段；行为证据：同一 replay 在两次
/// 调用（中间隔其他分配与计算）产出同摘要——时间不参与结果。
pub fn sentinel_timestamp(input: &EmitInput) -> Result<(), String> {
    let mut w1 = Watchdog::emit();
    let mut w2 = Watchdog::emit();
    let a = emit_replay(input, &mut w1).ok_or_else(|| format!("{}：replay 失败", E_CONS_NONDET))?;
    // 中间做无关分配与计算（模拟"时间流逝"与其他负载）。
    let mut junk: Vec<u64> = Vec::new();
    let mut i = 0u64;
    while i < 64 {
        junk.push(i.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        i += 1;
    }
    let b = emit_replay(input, &mut w2).ok_or_else(|| format!("{}：replay 失败", E_CONS_NONDET))?;
    let da = bits_digest(&spawn_flatten(&a), &group_seq(&a));
    let db = bits_digest(&spawn_flatten(&b), &group_seq(&b));
    if da != db {
        return Err(format!(
            "{}：时间戳哨兵命中——跨时刻双跑摘要异（{}≠{}），时间参与了结果",
            E_CONS_NONDET, da, db
        ));
    }
    Ok(())
}

/// 未init 哨兵：毒内存缝双跑同位（未初始化内存若参与结果必现形）。
pub fn sentinel_uninit(input: &EmitInput) -> Result<(), String> {
    let run_with_poison = |poison: bool| -> Option<u64> {
        let mut w = Watchdog::emit();
        if poison {
            // 缝里先堆一块毒内存再丢（堆状态被搅动但内容不应影响结果）。
            let mut junk: Vec<u8> = Vec::new();
            let mut i = 0usize;
            while i < 512 {
                junk.push(0xAA);
                i += 1;
            }
            let digest_pre = fnv1a64(&junk);
            if digest_pre == 0 {
                return None; // 不可达（FNV 非零），保卫兵风格不猜
            }
        }
        let spawns = emit_replay(input, &mut w)?;
        Some(bits_digest(&spawn_flatten(&spawns), &group_seq(&spawns)))
    };
    let clean = run_with_poison(false).ok_or_else(|| format!("{}：干净 replay 失败", E_CONS_NONDET))?;
    let dirty = run_with_poison(true).ok_or_else(|| format!("{}：毒内存 replay 失败", E_CONS_NONDET))?;
    if clean != dirty {
        return Err(format!(
            "{}：未init 哨兵命中——毒内存缝双跑摘要异（{}≠{}），未初始化内存参与了结果",
            E_CONS_NONDET, clean, dirty
        ));
    }
    Ok(())
}

/// LCG 排列（哨兵用：确定性洗牌，不引 std 随机源）。
fn lcg_permutation(n: usize, seed: u64) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..n).collect();
    let mut rng = RandomSource::new(seed);
    let mut i = n;
    while i > 1 {
        i -= 1;
        let r = rng.next_f32();
        let j = if r >= 1.0 { n - 1 } else { (r * n as f32) as usize };
        idx.swap(i, j);
    }
    idx
}

/// 迭代器顺序哨兵：规范序（按下标配对键）固定 + 排列后矩差 ≤ 界。
///
/// 顺序敏感处显性化：对同一组数，规范序求和 vs 排列序求和的差
/// ≤ [`ULP_BOUND`]×len 的累积容差（浮点非结合的如实界），且 canonical
/// 序即排序后序（不变量可复算）。
pub fn sentinel_iteration_order(vals: &[f32], seed: u64) -> Result<(), String> {
    let mut canonical: Vec<f32> = vals.to_vec();
    canonical.sort_by(|a, b| a.partial_cmp(b).unwrap_or(core::cmp::Ordering::Equal));
    let perm = lcg_permutation(vals.len(), seed);
    let mut shuffled: Vec<f32> = Vec::new();
    for p in perm.iter() {
        shuffled.push(vals[*p]);
    }
    let d = ulp_distance(mean_f32(&canonical), mean_f32(&shuffled));
    // 顺序差的累积容差：每元素的非结合抖动 ≤1 ulp，均值再除 len。
    let tol = if vals.is_empty() { 0 } else { (vals.len() as u64).max(1) };
    if d > tol + ULP_BOUND {
        return Err(format!(
            "{}：迭代器顺序哨兵命中——规范序/排列序均值差 {} ulp 超容差 {}——顺序敏感处须显性固定规范序",
            E_CONS_NONDET, d, tol + ULP_BOUND
        ));
    }
    Ok(())
}

/// 浮点非结合哨兵：pairwise vs 序列求和差 ≤ 界且如实记差值。
///
/// 非常量假绿：差值本身必须是**测量值**（同数组同差值），若某天
/// 实现改了结合方式，差值变化即红。
pub fn sentinel_float_nonassoc(vals: &[f32]) -> Result<(u64, u64), String> {
    // 序列求和（规范序）。
    let mut seq = 0.0f32;
    for v in vals.iter() {
        seq += *v;
    }
    // pairwise 求和（分治两半）。
    let pw = pairwise_sum(vals);
    let d = ulp_distance(seq, pw);
    // 界：元素数个 ulp（每步加法至多 1 ulp 抖动，pairwise 步数更少）。
    let tol = (vals.len() as u64).max(1) + ULP_BOUND;
    if d > tol {
        return Err(format!(
            "{}：浮点非结合哨兵命中——序列/pairwise 求和差 {} ulp 超容差 {}——结合方式不可隐式变更",
            E_CONS_NONDET, d, tol
        ));
    }
    Ok((d, tol))
}

/// pairwise 求和（分治：左右各半递归到叶）。
fn pairwise_sum(vals: &[f32]) -> f32 {
    if vals.len() <= 1 {
        return if vals.is_empty() { 0.0 } else { vals[0] };
    }
    let mid = vals.len() / 2;
    pairwise_sum(&vals[..mid]) + pairwise_sum(&vals[mid..])
}

/// 四类哨兵的修复指引（命中即给路——不空报错）。
pub fn sentinel_fix_hint(kind: &str) -> &'static str {
    match kind {
        "timestamp" => "替换时间戳输入为显式逻辑时钟或从签名移除（时间不得参与模拟结果）",
        "uninit" => "填充/零初始化所有缓冲；结果不得依赖堆残留内容",
        "iteration" => "固定规范处理序（稳定排序键），顺序敏感处显性声明",
        "float" => "固定求和结合方式（规范序或 pairwise），变更须过本哨兵",
        _ => "未知哨兵类——请登记四类之一",
    }
}

// ---------------------------------------------------------------------------
// 八、术语与命名一致（F2219 + F2213 v1 冻结簿）
// ---------------------------------------------------------------------------

/// 一致性链步骤名（必须 ∈ F2213 v1 冻结簿——命名一致的可执行面）。
pub const CONFORMITY_STEPS: [&str; 6] = [
    "l.ps.emitter.create",
    "l.ps.emitter.set_param",
    "l.ps.emitter.set_running",
    "l.ps.event.subscribe_pool_pressure",
    "l.ps.pool.water",
    "l.ps.emitter.enumerate",
];

/// 术语核验：锚点三术语（发射器/粒子/池）与 F2213 v1 冻结簿命名一致，
/// 且一致性链步骤名全在 v1 冻结簿。
///
/// 术语的"承载"分两种形态，如实区分不硬凑：
/// - **emitter / pool**：有签名承载（`l.ps.emitter.*` / `l.ps.pool.*`）；
/// - **particle**：命名空间承载（`l.ps.` 前缀即粒子段——冻结簿全名
///   必经此前缀，全簿非空且全命中即证）。
///
/// （粒子组 `group` 是 F2203 数据模型术语——vel03 `ParticleGroup`，
/// 不在 API 签名面；把它当 API 术语硬找签名承载是编造对端，故不列。）
pub fn terminology_verdict(book: &FreezeBook) -> Result<(), String> {
    // 术语侧：emitter/pool 找签名承载，particle 找命名空间承载。
    let api_terms = [("emitter", "发射器"), ("pool", "粒子池")];
    for (en, zh) in api_terms.iter() {
        let hit = book.names().iter().any(|n| n.contains(en));
        if !hit {
            return Err(format!(
                "{}：术语 {}（{}）在 v1 冻结簿无签名承载——术语表与签名簿漂移",
                E_CONS_TERM, en, zh
            ));
        }
    }
    let names = book.names();
    if names.is_empty() || !names.iter().all(|n| n.starts_with("l.ps.")) {
        return Err(format!(
            "{}：术语「粒子」无命名空间承载——v1 冻结簿全名须经 l.ps. 前缀",
            E_CONS_TERM
        ));
    }
    // 命名侧：链步骤全在簿。
    for st in CONFORMITY_STEPS.iter() {
        if book.get(st).is_none() {
            return Err(format!(
                "{}：一致性链步骤 {} 不在 v1 冻结簿——链上命名与冻结 API 漂移",
                E_CONS_TERM, st
            ));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 九、判据
// ---------------------------------------------------------------------------

use crate::checks::CheckSet;
use crate::svstar2::vel13_apifreeze::FREEZE_V1;

/// F2215 域自检（判据六组：逐位/帧率/等效/哨兵/看门狗/术语）。
pub fn run_vel15_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F2215");
    let seed_a = 0x5EED_0001u64;
    let seed_b = 0x5EED_0002u64;

    // --- 逐位确定（判据一：三面同输入对拍）---
    let input_a = EmitInput::standard(seed_a);
    let input_b = EmitInput::standard(seed_b);

    let mut w_emit1 = Watchdog::emit();
    let spawns_1 = emit_replay(&input_a, &mut w_emit1);
    let mut w_emit2 = Watchdog::emit();
    let spawns_2 = emit_replay(&input_a, &mut w_emit2);
    let d1 = spawns_1.as_ref().map(|x| bits_digest(&spawn_flatten(x), &group_seq(x)));
    let d2 = spawns_2.as_ref().map(|x| bits_digest(&spawn_flatten(x), &group_seq(x)));

    // L15-逐位-01：发射面同种子双跑逐位同摘要。
    s.add(
        "L15-逐位-01",
        d1.is_some() && d1 == d2,
        "发射面同种子双跑逐位同摘要（FNV over bits）",
    );

    let life_1 = lifetime_replay(seed_a, 256, &mut Watchdog::mix());
    let life_2 = lifetime_replay(seed_a, 256, &mut Watchdog::mix());
    let dl1 = life_1.as_ref().map(|x| bits_digest_f32(x));
    let dl2 = life_2.as_ref().map(|x| bits_digest_f32(x));
    s.add("L15-逐位-02", dl1.is_some() && dl1 == dl2, "寿命面同种子双跑逐位同摘要");

    let views_1 = render_replay(&input_a, &mut Watchdog::render());
    let views_2 = render_replay(&input_a, &mut Watchdog::render());
    let dv1 = views_1.as_ref().map(|x| bits_digest_f32(&view_flatten(x)));
    let dv2 = views_2.as_ref().map(|x| bits_digest_f32(&view_flatten(x)));
    s.add("L15-逐位-03", dv1.is_some() && dv1 == dv2, "渲染面同种子双跑逐位同摘要");

    // L15-逐位-04：三面摘要互异（防"都返回常量"的假绿对拍）。
    s.add(
        "L15-逐位-04",
        match (d1, dl1, dv1) {
            (Some(a), Some(b), Some(c)) => a != b && b != c && a != c,
            _ => false,
        },
        "三面摘要互异（发射/寿命/渲染非同源常量）",
    );

    // L15-逐位-05：不同种子 → 不同摘要（种子真的在驱动结果）。
    let mut w_emit3 = Watchdog::emit();
    let spawns_3 = emit_replay(&input_b, &mut w_emit3);
    let d3 = spawns_3.as_ref().map(|x| bits_digest(&spawn_flatten(x), &group_seq(x)));
    s.add("L15-逐位-05", d1 != d3 && d3.is_some(), "异种子异摘要（种子生效）");

    // --- 帧率无关（判据二）---
    // L15-帧率-01：一般档极差 ≤1 粒（实现自声明界）。
    s.add("L15-帧率-01", frame_rate_verdict(100.0, 1.0).is_ok(), "帧率阶梯总发射极差 ≤1（30/60/144/1000）");

    // L15-帧率-02：精确档逐位相等（2 的幂 dt 硬断言）。
    let exact_totals: Vec<u32> = EXACT_STEP_DENOMS
        .iter()
        .map(|d| frame_rate_total(100.0, 1.0, *d))
        .collect();
    let exact_all_eq = exact_totals.iter().all(|t| *t == exact_totals[0]);
    s.add("L15-帧率-02", exact_all_eq && exact_totals[0] == 100, "精确档四档全等且恰 100 粒");

    // L15-帧率-03：未归一化模式可检出（反面对拍：帧率当 dt 传——经典
    // 帧率相关 bug，归一化路径即 frame_rate_verdict 的 dt=1/fps）。
    let div = unnormalized_divergence(100.0);
    s.add(
        "L15-帧率-03",
        div > ACCUMULATOR_BOUND && unnormalized_fix_hint().contains("归一化"),
        "未归一化调用跨帧率极差超界即拒（修复=dt 归一化）",
    );

    // --- GPU 统计等效（判据三）---
    let cpu_flat = views_1.as_ref().map(|x| view_flatten(x)).unwrap_or_default();
    let gpu_flat = gpu_model_flatten(&cpu_flat, 2);
    let (max_ulp, rows) = equivalence_measure(&cpu_flat, &gpu_flat);
    // L15-等效-01：逐粒子最大 ulp 距离 ≤8（视觉等效界内）。
    s.add(
        "L15-等效-01",
        max_ulp <= ULP_BOUND && declare_equivalence(max_ulp).is_ok(),
        "逐粒子最大 ulp 距离 ≤8 且等效声明放行",
    );

    // L15-等效-02：超界宣称失效（构造 +100 ulp 极端扰动 → 拒）。
    let gpu_bad = gpu_model_flatten(&cpu_flat, 100);
    let (max_ulp_bad, _) = equivalence_measure(&cpu_flat, &gpu_bad);
    s.add(
        "L15-等效-02",
        max_ulp_bad > ULP_BOUND && declare_equivalence(max_ulp_bad).is_err(),
        "超 8 ulp 宣称失效（边界外不许宣称等效）",
    );

    // L15-等效-03：恰边界放行（恰 8 ulp 不算超——界含边界值）。
    let gpu_edge = gpu_model_flatten(&cpu_flat, ULP_BOUND as i32);
    let (max_ulp_edge, _) = equivalence_measure(&cpu_flat, &gpu_edge);
    s.add(
        "L15-等效-03",
        max_ulp_edge == ULP_BOUND && declare_equivalence(max_ulp_edge).is_ok(),
        "恰 8 ulp 放行（边界含）",
    );

    // L15-等效-04：矩对齐表三行 within 且 ulp 距离实测非编造。
    s.add(
        "L15-等效-04",
        rows.iter().all(|r| r.within) && rows[0].metric == "尺寸均值",
        "矩对齐表在界内（尺寸均值行首）",
    );

    // --- 哨兵四类（判据四的行为面）---
    // L15-哨兵-01：时间戳哨兵。
    s.add("L15-哨兵-01", sentinel_timestamp(&input_a).is_ok(), "时间戳哨兵（无时间入参+跨时刻同位）");

    // L15-哨兵-02：未init 哨兵。
    s.add("L15-哨兵-02", sentinel_uninit(&input_a).is_ok(), "未init 哨兵（毒内存缝双跑同位）");

    // L15-哨兵-03：迭代器顺序哨兵。
    let order_vals: Vec<f32> = (0..64).map(|i| (i as f32) * 0.25 - 4.0).collect();
    s.add(
        "L15-哨兵-03",
        sentinel_iteration_order(&order_vals, 0xABCD).is_ok(),
        "迭代器顺序哨兵（规范序固定+排列矩差 ≤ 容差）",
    );

    // L15-哨兵-04：浮点非结合哨兵（差值实测且 ≤ 界）。
    let (nonassoc_d, nonassoc_tol) = sentinel_float_nonassoc(&order_vals).unwrap_or((u64::MAX, 0));
    s.add(
        "L15-哨兵-04",
        nonassoc_d <= nonassoc_tol,
        "浮点非结合哨兵（pairwise vs 序列差 ≤ 界）",
    );

    // L15-哨兵-05：四类修复指引全非空（命中即给路）。
    let hints = ["timestamp", "uninit", "iteration", "float"];
    s.add(
        "L15-哨兵-05",
        hints.iter().all(|h| !sentinel_fix_hint(h).is_empty())
            && !sentinel_fix_hint("unknown").is_empty(),
        "四类哨兵修复指引齐备（含未知类兜底）",
    );

    // --- 看门狗（判据五的死循环防护）---
    // L15-看门狗-01：预算内完成。
    let mut w_ok = Watchdog::new(100);
    let mut ok_steps = 0u64;
    let mut done = true;
    while ok_steps < 100 {
        if w_ok.tick().is_err() {
            done = false;
            break;
        }
        ok_steps += 1;
    }
    s.add("L15-看门狗-01", done && w_ok.used() == 100, "恰预算内完成（100/100）");

    // L15-看门狗-02：超预算挂起+立案码。
    let mut w_over = Watchdog::new(10);
    let mut tripped = false;
    let mut guard = 0u32;
    while guard < 10_000 {
        if w_over.tick().is_err() {
            tripped = true;
            break;
        }
        guard += 1;
    }
    let err_msg = {
        let mut w = Watchdog::new(3);
        let mut e = String::new();
        let mut i = 0u32;
        while i < 10 {
            if let Err(msg) = w.tick() {
                e = msg;
                break;
            }
            i += 1;
        }
        e
    };
    s.add(
        "L15-看门狗-02",
        tripped && err_msg.starts_with(E_CONS_WATCHDOG),
        "超预算挂起+立案（E_CONS_WATCHDOG）",
    );

    // L15-看门狗-03：预算分级（发射/混合/渲染/全链四档递增生效）。
    s.add(
        "L15-看门狗-03",
        BUDGET_EMIT_STEPS + BUDGET_MIX_STEPS + BUDGET_RENDER_STEPS == BUDGET_CHAIN_STEPS,
        "四档预算和=全链预算（分族可审计）",
    );

    // --- 术语与命名一致（判据六）---
    let book = match FreezeBook::freeze(&FREEZE_V1) {
        Ok(b) => b,
        Err(_) => FreezeBook::new(),
    };

    // L15-术语-01：术语与签名簿同检（三术语都有签名承载）。
    s.add(
        "L15-术语-01",
        terminology_verdict(&book).is_ok(),
        "三术语在 v1 冻结簿有承载（术语表与签名簿不漂移）",
    );

    // L15-术语-02：链六步全在冻结簿。
    s.add(
        "L15-术语-02",
        CONFORMITY_STEPS.iter().all(|st| book.get(st).is_some()) && CONFORMITY_STEPS.len() == 6,
        "一致性链六步命名与 F2213 一致",
    );

    // L15-术语-03：术语缺失可检出（空簿即拒——防假绿）。
    let empty_book = FreezeBook::new();
    s.add(
        "L15-术语-03",
        terminology_verdict(&empty_book).is_err()
            && terminology_verdict(&empty_book).unwrap_err().starts_with(E_CONS_TERM),
        "空簿术语核验拒（术语无处承载即红）",
    );

    // --- CI 与版本 ---
    // L15-CI-01：全链套件一键可跑（三面+帧率+哨兵在当前输入全过）。
    let chain_ok = sentinel_timestamp(&input_a).is_ok()
        && sentinel_uninit(&input_a).is_ok()
        && sentinel_iteration_order(&order_vals, 0xABCD).is_ok()
        && sentinel_float_nonassoc(&order_vals).is_ok()
        && frame_rate_verdict(100.0, 1.0).is_ok();
    s.add("L15-CI-01", chain_ok, "全链套件一键跑过");

    // L15-CI-02：注入回归即红（篡改摘要路径 vs 原摘要必异——测试网真的在网）。
    let mut tampered = spawns_1.clone().unwrap_or_default();
    if let Some(first) = tampered.first_mut() {
        first.position.x = first.position.x + 1.0;
    }
    let d_tamper = bits_digest(&spawn_flatten(&tampered), &group_seq(&tampered));
    // 判据区零 panic：d1 取值失败一律 match 记红，不 unwrap。
    let tamper_detected = match d1 {
        Some(a) => d_tamper != a,
        None => false,
    };
    s.add(
        "L15-CI-02",
        tamper_detected,
        "一粒位改动即换摘要（回归网灵敏）",
    );

    // L15-版本-01：版本指纹非零。
    let fp = fnv1a64(CONSISTENCY_VERSION.as_bytes());
    s.add("L15-版本-01", fp != 0, "版本指纹非零（L15-consistency-v1）");

    // L15-暂挂-01：L 域账本暂挂声明显性（移交期模式延续）。
    s.add(
        "L15-暂挂-01",
        L_LEDGER_CONS_SUSPENDED_NOTE.contains("暂挂") && L_LEDGER_CONS_SUSPENDED_NOTE.contains("F2215"),
        "L 域账本暂挂声明显性",
    );

    // L15-暂挂-02：判据条数对账（本条为第 28 条）。
    s.add("L15-暂挂-02", s.len() == 27, "判据条数对账（27+本条）");

    s
}

/// L 域账本暂挂声明（锚点跨批对接点：总账入 L 域账本——建账前暂挂，
/// 移交期模式延续；与 F2094/F2211/F2212/F2213/F2214 同款）。
pub const L_LEDGER_CONS_SUSPENDED_NOTE: &str = "粒子一致性回归网总账入 L 域账本：建账前暂挂声明（移交期模式第五域延续——F2215 同款）；逐位对拍与哨兵随 CI 跑，步数预算防挂起";

// ---------------------------------------------------------------------------
// 十、诚实边界
// ---------------------------------------------------------------------------

/// GPU 侧诚实声明（同 vel07_blend/vel12 先例：不编造实测）。
///
/// 本模块的「GPU 路径统计等效」是对**声明式 GPU 模型**（同 LCG 流+
/// 确定性 ±ulp 扰动，[`gpu_model_flatten`]）的实测；真实 GPU 路径
/// 落在 F2226（GPU 着色器），其实测数据落地前，等效声明的载体是
/// 模型而非真 GPU。8 ulp 边界本身是锚点钉死的判据，随时可对真
/// GPU 复测——复测接口即 [`equivalence_measure`] 与
/// [`declare_equivalence`]，无需改判据结构。
pub const GPU_MODEL_HONESTY_NOTE: &str =
    "GPU 统计等效当前实测对象为声明式 GPU 模型（同流+确定性扰动）；真 GPU 路径 F2226 落地后以同接口复测，8 ulp 边界不变";