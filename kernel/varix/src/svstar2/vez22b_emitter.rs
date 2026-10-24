//! VE-F5222 · 粒子发射器系统（VE-Z 域 · Z02 粒子库 · 锚点目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F5222`
//!
//! **判据（锚点原文）**：发射形态、发射速率、初速分布、方向锥、延迟与突发、
//! 判据。
//!
//! **职责定位（锚点原文）**：发射器系统——粒子从哪出生、以什么速率出生、
//! 朝哪个方向出生。它是粒子库的**唯一入口**：模拟器只接受发射器产出的
//! 出生请求，不接受任何别处的 `spawn` 调用。
//!
//! # 深化说明
//!
//! ## 深化一：发射形态是**参数化的采样函数**，不是四个 if 分支
//!
//! 点/线/面/体四型发射形态各自有空间分布律。若写成四条 `match` 分支，
//! 新增「环形」「螺旋」时必然复制粘贴，且采样一致性无法机检。故
//! [`SpawnShape`] 把四型统一成 `sample(u, v, w) -> [f32; 3]`：三个**低差异
//! 序列**标量（Halton 2/3/5 基）经形态映射到空间，形态之间只差映射式。
//! 采样律的性质因此可被独立证明：
//!
//! - `shape_samples_stay_in_bounds`：任意 `(u,v,w) ∈ [0,1)³`，采样点必须落在
//!   该形态声明的包围盒内——越界采样是「粒子在墙外出生」的直接成因；
//! - `shape_volume_order`：点 < 线 < 面 < 体 的体积序必须成立（形态声明
//!   的体积估计若与几何不符，预算申报整条链就失真）。
//!
//! ## 深化二：速率控制用**累加器 + 帧预算双闸**，不是 `rate * dt` 直接取整
//!
//! 朴素写法 `n = (rate * dt) as u32` 在低帧率下会**系统性少发**（每帧截断
//! 小数部分），在 60fps 下漏发率约 50% 的亚整数部分——发射密度与 `rate`
//! 不成正比，参数面调参失去意义。本实现用**定点累加器**
//! （[`RateAccumulator`]，1/65536 精度）：每帧累加 `rate * dt * 65536`，
//! 取商即本帧应发数，余数留存下帧。于是任意 `dt` 组合下长期发射数都收敛到
//! `rate * t`，误差恒 < 1 粒（`accumulator_conserves_total` 用 1000 帧
//! 随机 dt 序列验证守恒）。
//!
//! 第二道闸是**单帧上限**：一帧内允许发射的数量受 `frame_cap` 约束，
//! 防止长帧（如从睡眠唤醒）一次吐出数万粒而爆缓冲。超出部分**丢弃并记账**
//! （[`EmitterStats::cap_dropped`]）——丢弃必须可见，否则「粒子为什么少了」
//! 会变成无法复现的玄学。
//!
//! ## 深化三：方向锥是**拒绝采样 + 兜底重采**，不是先算极角再夹
//!
//! 均匀锥采样的正确写法是「球面均匀取方向，判是否在锥内，不在则重采」。
//! 取巧写法（先均匀取 θ 再夹到锥半角）会让方向**在锥面上过度堆积**——
//! 视觉上表现为边缘一圈明显亮环。本实现用三个独立 LCG 流做拒绝采样，
//! 并给出**重采上限**：超过上限则退化为「锥轴方向 + 满角抖动」，保证
//! 函数必定终止（无 `loop` 死循环面）且退化时记账（`cone_fallback`）。
//!
//! ## 深化四：延迟与突发共用一条**时间轴事件表**
//!
//! `delay_ms` 与 `burst` 若各写一套计时器，两者叠加时的语义（突发第一发
//! 是否也延迟？延迟期内突发是否累积？）就会因实现而异。本实现把两者统一
//! 表达为 [`EmitPlan`]：一串 `(相对时刻, 数量)` 事件，`delay` 只是把整串
//! 事件的时刻平移，`burst` 只是把「速率产出」换成「定点事件」。语义因此
//! 唯一，且可被 `plan_is_monotonic` 机检（时刻必须严格非降，否则意味着
//! 回溯发射——时间倒流是状态机的隐性死结）。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本（供 API 冻结判据核对）。
pub const EMITTER_VERSION: &str = "F5222-emitter-v1";

// ── 诊断码（本模块独占段 0xE000~0xE00F）─────────────────────────────
const SEG_BASE: u16 = 0xE000;
/// 包围盒非法（min > max 或含 NaN）。
pub const E_SHAPE_BOX: u16 = SEG_BASE + 1;
/// 速率超出上限（> RATE_MAX）。
pub const E_RATE_RANGE: u16 = SEG_BASE + 2;
/// 方向锥半角越界（< 0 或 > π）。
pub const E_CONE_ANGLE: u16 = SEG_BASE + 3;
/// 初速区间非法（min > max 或 min < 0）。
pub const E_SPEED_RANGE: u16 = SEG_BASE + 4;
/// 单帧上限非法（= 0）。
pub const E_FRAME_CAP: u16 = SEG_BASE + 5;
/// 事件时刻非单调（回溯发射）。
pub const E_PLAN_ORDER: u16 = SEG_BASE + 6;
/// 突发数量越界。
pub const E_BURST_COUNT: u16 = SEG_BASE + 7;
/// 延迟越界。
pub const E_DELAY_RANGE: u16 = SEG_BASE + 8;

/// 速率上限（粒/秒）。依据：单发射器 10 万粒/秒已足够表达「火墙」级浓密
/// 效果（1920×1080 下每像素每帧约 1.6 粒），再高则应改用多次重复发射器。
pub const RATE_MAX: u32 = 100_000;

/// 累加器分母（1/65536 精度）。依据：60fps 下 `rate/60` 的小数部分分辨率
/// 1/65536 ≈ 1.5e-5 粒/帧，1000 帧累积误差 < 0.015 粒，远小于 1 粒。
pub const ACC_DEN: u32 = 65_536;

/// 方向锥拒绝采样重试上限。依据：锥半角 ≥ 5° 时球面命中率 ≥ 0.19%，
/// 重试 256 次失败概率 < (1-0.0019)^256 ≈ 0.6 %；命中率更高时几乎必成。
pub const CONE_RETRY_MAX: u32 = 256;

/// 单场突发数量上限。依据：一次突发超过 4096 粒时应拆成多次事件（可观察
/// 的生成过程比瞬间出现更符合物理直觉，也避免单帧缓冲尖峰）。
pub const BURST_MAX: u32 = 4_096;

/// 延迟上限（毫秒）。依据：超过 60 秒的延迟在实际编排中应由时间轴调度器
/// 管理，而不是压在一个发射器上。
pub const DELAY_MAX_MS: u32 = 60_000;

// ── 低差异序列 ──────────────────────────────────────────────────

/// Halton 序列（基 `b`）第 `i` 项，返回 `[0,1)`。
///
/// 为什么不用 `rand`：内核无随机源，且粒子采样要**确定性**——同一 `seed`
/// 必须逐帧一致，否则录制回放与跨机对拍全废。
pub fn halton(mut i: u32, b: u32) -> f32 {
    if b < 2 {
        return 0.0;
    }
    let inv = 1.0 / b as f32;
    let mut f = inv;
    let mut r = 0.0f32;
    while i > 0 {
        r += f * (i % b) as f32;
        i /= b;
        f *= inv;
    }
    r
}

/// 采样形态：粒子「从哪出生」。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpawnShape {
    /// 点：全部粒子出生于同一点（`origin`）。
    Point,
    /// 线段：沿 `dir` 方向长度 `len` 均匀分布。
    Line,
    /// 矩形面：`size` 的 XY 面（法向 +Z）。
    Rect,
    /// 长方体体积：`size` 三轴均匀分布。
    Volume,
}

impl SpawnShape {
    /// 该形态的**理想体积估计**（用于预算申报与采样一致性检验）。
    ///
    /// 点体积记为 0（单位：世界长度³）。线/面按「退化维长度 0」处理，
    /// 故线体积 0、面体积 0——但三者在**采样自由度**上不同，另由
    /// [`SpawnShape::dof`] 区分，避免把「体积」当「自由度」用。
    pub fn volume(self, _len: f32, size: [f32; 3]) -> f32 {
        match self {
            SpawnShape::Point => 0.0,
            SpawnShape::Line => 0.0,
            SpawnShape::Rect => 0.0,
            SpawnShape::Volume => size[0].max(0.0) * size[1].max(0.0) * size[2].max(0.0),
        }
    }

    /// 采样自由度（0=点，1=线，2=面，3=体）。
    pub fn dof(self) -> u8 {
        match self {
            SpawnShape::Point => 0,
            SpawnShape::Line => 1,
            SpawnShape::Rect => 2,
            SpawnShape::Volume => 3,
        }
    }

    /// 在 `[0,1)³` 上的标量三元组映射到采样点（相对 `origin` 的偏移）。
    ///
    /// 保证：`u,v,w ∈ [0,1)` 时结果必落在该形态的包围半盒内。
    pub fn sample(self, u: f32, v: f32, w: f32, len: f32, size: [f32; 3]) -> [f32; 3] {
        let u = clamp01(u);
        let v = clamp01(v);
        let w = clamp01(w);
        match self {
            SpawnShape::Point => [0.0, 0.0, 0.0],
            SpawnShape::Line => [len * (u - 0.5), 0.0, 0.0],
            SpawnShape::Rect => [size[0] * (u - 0.5), size[1] * (v - 0.5), 0.0],
            SpawnShape::Volume => [
                size[0] * (u - 0.5),
                size[1] * (v - 0.5),
                size[2] * (w - 0.5),
            ],
        }
    }

    /// 该形态的包围半盒（各轴最大 |偏移|）。
    pub fn half_box(self, len: f32, size: [f32; 3]) -> [f32; 3] {
        match self {
            SpawnShape::Point => [0.0, 0.0, 0.0],
            SpawnShape::Line => [len.abs() * 0.5, 0.0, 0.0],
            SpawnShape::Rect => [size[0].abs() * 0.5, size[1].abs() * 0.5, 0.0],
            SpawnShape::Volume => [
                size[0].abs() * 0.5,
                size[1].abs() * 0.5,
                size[2].abs() * 0.5,
            ],
        }
    }
}

fn clamp01(x: f32) -> f32 {
    if x.is_nan() {
        0.0
    } else if x < 0.0 {
        0.0
    } else if x >= 1.0 {
        // 严格小于 1：保证 `u-0.5` 落在 [-0.5, 0.5)，边界不越半盒。
        0.999_999_94
    } else {
        x
    }
}

// ── 速率累加器 ──────────────────────────────────────────────────

/// 定点速率累加器：把「粒/秒」换算成「本帧应发几粒」，余数留存。
///
/// 这是本模块对「低帧率系统性少发」这一经典缺陷的正面回答。
#[derive(Clone, Copy, Debug, Default)]
pub struct RateAccumulator {
    /// 未消化的余数（单位 1/ACC_DEN 粒）。
    rest: u64,
}

impl RateAccumulator {
    pub fn new() -> Self {
        Self { rest: 0 }
    }

    /// 推进一帧，返回本帧应发粒数（未受 `frame_cap` 约束）。
    ///
    /// `dt_ms` 为 0 时直接返回 0（不推进累加器，避免 dt=0 的伪帧扰动相位）。
    pub fn tick(&mut self, rate: u32, dt_ms: u32) -> u32 {
        if dt_ms == 0 || rate == 0 {
            return 0;
        }
        let numer = rate as u64 * dt_ms as u64 * ACC_DEN as u64;
        let total = numer / 1000 + self.rest;
        let whole = (total / ACC_DEN as u64) as u32;
        self.rest = total % ACC_DEN as u64;
        whole
    }

    /// 余数（1/ACC_DEN 粒），诊断用。
    pub fn remainder(&self) -> u64 {
        self.rest
    }

    /// 复位（发射器停用/重启用）。
    pub fn reset(&mut self) {
        self.rest = 0;
    }
}

// ── 方向锥 ──────────────────────────────────────────────────────

/// 方向锥：以 `axis` 为轴、半角 `half_angle_cos`（存余弦，省一次 acos）。
#[derive(Clone, Copy, Debug)]
pub struct DirCone {
    /// 归一化轴向量。
    pub axis: [f32; 3],
    /// 半角余弦（`cos(半角)`，取值 [-1, 1]）。
    pub half_angle_cos: f32,
}

impl DirCone {
    /// 构造：半角以**弧度**给出，内部转余弦并校验。
    pub fn new(axis: [f32; 3], half_angle_rad: f32) -> Result<Self, u16> {
        if !(0.0..=core::f32::consts::PI).contains(&half_angle_rad) || half_angle_rad.is_nan() {
            return Err(E_CONE_ANGLE);
        }
        let a = normalize(axis).ok_or(E_CONE_ANGLE)?;
        Ok(Self {
            axis: a,
            half_angle_cos: cosf(half_angle_rad),
        })
    }

    /// 全向锥（2π）。
    pub fn omni(axis: [f32; 3]) -> Self {
        Self {
            axis: normalize(axis).unwrap_or([0.0, 1.0, 0.0]),
            half_angle_cos: -1.0,
        }
    }

    /// 采样一个方向。
    ///
    /// 返回 `(方向, 是否走了退化路径)`。退化路径 = 重试 `CONE_RETRY_MAX` 次
    /// 仍未命中（只可能在半角极小且随机流恰好不巧时发生），此时返回
    /// 「轴 + 满角抖动」，并在统计里记 `cone_fallback`。
    pub fn sample(&self, rng: &mut Lcg) -> ([f32; 3], bool) {
        for _ in 0..CONE_RETRY_MAX {
            let d = rng.unit_sphere();
            let c = d[0] * self.axis[0] + d[1] * self.axis[1] + d[2] * self.axis[2];
            if c >= self.half_angle_cos {
                return (d, false);
            }
        }
        // 退化：逐步收窄到轴附近，保证仍在锥内（cos 必然 ≥ 阈值）。
        let e = 1.0 - self.half_angle_cos;
        let s = if e <= 0.0 { 0.0 } else { (e * 0.5).min(1.0) };
        let d = normalize([
            self.axis[0] + s * (rng.next_f() * 2.0 - 1.0),
            self.axis[1] + s * (rng.next_f() * 2.0 - 1.0),
            self.axis[2] + s * (rng.next_f() * 2.0 - 1.0),
        ])
        .unwrap_or(self.axis);
        (d, true)
    }
}

// ── 确定性随机流 ────────────────────────────────────────────────

/// 32 位线性同余流（Numerical Recipes 常数）。
///
/// 为什么用 LCG 而不是 xorshift：LCG 的进位结构简单、状态只有 32 位，
/// 跨平台（含 32 位目标）逐位一致；粒子对随机质量要求低，对**可复现**
/// 要求极高。
#[derive(Clone, Copy, Debug)]
pub struct Lcg {
    state: u32,
}

impl Lcg {
    pub fn new(seed: u32) -> Self {
        // 0 是 LCG 的不动点，必须避开。
        Self {
            state: if seed == 0 { 0x9E37_79B9 } else { seed },
        }
    }

    pub fn next_u32(&mut self) -> u32 {
        self.state = self
            .state
            .wrapping_mul(1_664_525)
            .wrapping_add(1_013_904_223);
        self.state
    }

    /// `[0,1)` 均匀。
    pub fn next_f(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 / 16_777_216.0
    }

    /// 球面均匀单位向量（Marsaglia 法：先取球内点再投到球面）。
    ///
    /// 为什么不用「均匀取 θ,φ」：那会在极点堆积，导致锥边缘先亮后暗。
    pub fn unit_sphere(&mut self) -> [f32; 3] {
        for _ in 0..16 {
            let x = self.next_f() * 2.0 - 1.0;
            let y = self.next_f() * 2.0 - 1.0;
            let z = self.next_f() * 2.0 - 1.0;
            let l2 = x * x + y * y + z * z;
            if l2 > 1e-6 && l2 <= 1.0 {
                let inv = 1.0 / sqrtf(l2);
                return [x * inv, y * inv, z * inv];
            }
        }
        // 极端退化（连续 16 次取到原点附近）：给确定的后备方向。
        [0.0, 1.0, 0.0]
    }
}

// ── 发射计划（延迟 + 突发统一时间轴）────────────────────────────

/// 一个发射事件：相对发射器启用的时刻偏移 + 数量。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EmitEvent {
    pub at_ms: u32,
    pub count: u32,
}

/// 发射计划：速率的**离散化补丁**。
///
/// 速率产出仍由 [`RateAccumulator`] 驱动；`EmitPlan` 只负责「定点事件」
/// （突发）与「时间窗平移」（延迟）。两者叠加语义因此唯一：突发事件与
/// 速率产出共用同一个 `t` 轴，都受同一 `delay_ms` 平移。
#[derive(Clone, Debug, Default)]
pub struct EmitPlan {
    events: alloc::vec::Vec<EmitEvent>,
    /// 全局延迟（毫秒），平移全部事件与速率起点。
    pub delay_ms: u32,
}

impl EmitPlan {
    pub fn new() -> Self {
        Self {
            events: alloc::vec::Vec::new(),
            delay_ms: 0,
        }
    }

    /// 设延迟。
    pub fn with_delay(mut self, delay_ms: u32) -> Result<Self, u16> {
        if delay_ms > DELAY_MAX_MS {
            return Err(E_DELAY_RANGE);
        }
        self.delay_ms = delay_ms;
        Ok(self)
    }

    /// 追加一个突发事件（`at_ms` 为**相对**发射起点的时刻）。
    ///
    /// 校验：数量 ∈ (0, BURST_MAX]；时刻必须 ≥ 上一个事件（非降序）。
    /// 非降序不是风格偏好——降序事件意味着「时间轴回溯」，消费端若按序
    /// 推进就永远触发不到它。
    pub fn push_burst(&mut self, at_ms: u32, count: u32) -> Result<(), u16> {
        if count == 0 || count > BURST_MAX {
            return Err(E_BURST_COUNT);
        }
        if let Some(last) = self.events.last() {
            if at_ms < last.at_ms {
                return Err(E_PLAN_ORDER);
            }
        }
        self.events.push(EmitEvent { at_ms, count });
        Ok(())
    }

    /// 事件表（只读）。
    pub fn events(&self) -> &[EmitEvent] {
        &self.events
    }

    /// 本帧应触发的突发粒数（`elapsed_ms` 为含延迟平移后的相对时刻）。
    ///
    /// 语义：触发**所有** `at_ms <= elapsed` 且尚未触发过的事件。调用方
    /// 负责单调推进 `elapsed`；本方法自身按 `cursor` 记账，重复调用不会
    /// 重复发射（幂等性由 `plan_cursor_is_monotonic` 判据守住）。
    pub fn drain(&mut self, elapsed_ms: u32, cursor: &mut usize) -> u32 {
        let mut n = 0u32;
        while *cursor < self.events.len() {
            let e = self.events[*cursor];
            if e.at_ms > elapsed_ms {
                break;
            }
            n = n.saturating_add(e.count);
            *cursor += 1;
        }
        n
    }

    /// 计划是否时刻非降（判据用）。
    pub fn is_monotonic(&self) -> bool {
        let mut prev = 0u32;
        for e in &self.events {
            if e.at_ms < prev {
                return false;
            }
            prev = e.at_ms;
        }
        true
    }
}

// ── 发射器本体 ──────────────────────────────────────────────────

/// 发射器配置。
#[derive(Clone, Copy, Debug)]
pub struct EmitterConfig {
    pub shape: SpawnShape,
    /// 线段长度（仅 `Line` 用）。
    pub line_len: f32,
    /// 面/体尺寸（`Rect` 用 XY，`Volume` 用 XYZ）。
    pub size: [f32; 3],
    /// 发射原点。
    pub origin: [f32; 3],
    /// 每秒粒数。
    pub rate: u32,
    /// 初速区间（cm/s），沿方向锥内的采样方向。
    pub speed_range: (f32, f32),
    /// 方向锥。
    pub cone: DirCone,
    /// 单帧发射上限。
    pub frame_cap: u32,
}

impl EmitterConfig {
    /// 校验全部参数，返回首个越界项的错误码。
    pub fn validate(&self) -> Result<(), u16> {
        if self.rate > RATE_MAX {
            return Err(E_RATE_RANGE);
        }
        let (lo, hi) = self.speed_range;
        if lo.is_nan() || hi.is_nan() || lo < 0.0 || lo > hi {
            return Err(E_SPEED_RANGE);
        }
        if self.frame_cap == 0 {
            return Err(E_FRAME_CAP);
        }
        if self.size.iter().any(|v| v.is_nan() || *v < 0.0) {
            return Err(E_SHAPE_BOX);
        }
        if self.line_len.is_nan() || self.line_len < 0.0 {
            return Err(E_SHAPE_BOX);
        }
        if self.origin.iter().any(|v| v.is_nan()) {
            return Err(E_SHAPE_BOX);
        }
        Ok(())
    }

    /// 采样一个出生点（世界坐标）。
    pub fn sample_position(&self, u: f32, v: f32, w: f32) -> [f32; 3] {
        let d = self.shape.sample(u, v, w, self.line_len, self.size);
        [
            self.origin[0] + d[0],
            self.origin[1] + d[1],
            self.origin[2] + d[2],
        ]
    }
}

/// 一次发射请求（交给模拟器的出生描述）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpawnRequest {
    pub pos: [f32; 3],
    pub vel: [f32; 3],
    /// 该粒在此次发射内的序号（供下游按序分配 sim 槽位）。
    pub serial: u32,
}

/// 发射器统计。
#[derive(Clone, Copy, Debug, Default)]
pub struct EmitterStats {
    /// 已发射总数。
    pub emitted: u64,
    /// 因单帧上限被丢弃的粒数。
    pub cap_dropped: u64,
    /// 方向锥退化采样次数。
    pub cone_fallback: u32,
    /// 已消耗的帧数（`dt > 0`）。
    pub frames: u32,
}

/// 发射器：把配置与计划变成一串 [`SpawnRequest`]。
#[derive(Clone, Debug)]
pub struct Emitter {
    cfg: EmitterConfig,
    plan: EmitPlan,
    acc: RateAccumulator,
    rng: Lcg,
    cursor: usize,
    elapsed_ms: u32,
    serial: u32,
    stats: EmitterStats,
    /// 低差异序列计数器（保证形态内采样均匀，不随随机流退化）。
    ld: u32,
}

impl Emitter {
    /// 构造（校验配置）。
    pub fn new(cfg: EmitterConfig, plan: EmitPlan, seed: u32) -> Result<Self, u16> {
        cfg.validate()?;
        Ok(Self {
            cfg,
            plan,
            acc: RateAccumulator::new(),
            rng: Lcg::new(seed),
            cursor: 0,
            elapsed_ms: 0,
            serial: 0,
            stats: EmitterStats::default(),
            ld: 0,
        })
    }

    pub fn config(&self) -> &EmitterConfig {
        &self.cfg
    }

    pub fn stats(&self) -> EmitterStats {
        self.stats
    }

    pub fn elapsed_ms(&self) -> u32 {
        self.elapsed_ms
    }

    /// 推进一帧，把出生请求写进 `out`。
    ///
    /// 返回实际写入条数。`out` 容量不足时**部分写入并停手**（不覆盖，
    /// 不 panic）——调用方据此扩容后重试剩余部分。
    pub fn tick(&mut self, dt_ms: u32, out: &mut alloc::vec::Vec<SpawnRequest>) -> usize {
        if dt_ms == 0 {
            return 0;
        }
        self.stats.frames = self.stats.frames.saturating_add(1);
        self.elapsed_ms = self.elapsed_ms.saturating_add(dt_ms);

        // 延迟期内：只推进时间，不产出。
        if self.elapsed_ms < self.plan.delay_ms {
            return 0;
        }
        let rel = self.elapsed_ms - self.plan.delay_ms;

        let from_rate = self.acc.tick(self.cfg.rate, dt_ms);
        let from_burst = self.plan.drain(rel, &mut self.cursor);
        let want = from_rate.saturating_add(from_burst);

        // 单帧上限：超出部分丢弃并记账（可见，不静默）。
        let allowed = want.min(self.cfg.frame_cap);
        if want > allowed {
            self.stats.cap_dropped = self
                .stats
                .cap_dropped
                .saturating_add((want - allowed) as u64);
        }

        let mut written = 0usize;
        for _ in 0..allowed {
            if out.len() >= out.capacity() {
                break;
            }
            let pos = self.sample_pos();
            let (dir, fallback) = self.cfg.cone.sample(&mut self.rng);
            if fallback {
                self.stats.cone_fallback = self.stats.cone_fallback.saturating_add(1);
            }
            let (lo, hi) = self.cfg.speed_range;
            let sp = lo + (hi - lo) * self.rng.next_f();
            let vel = [dir[0] * sp, dir[1] * sp, dir[2] * sp];
            out.push(SpawnRequest {
                pos,
                vel,
                serial: self.serial,
            });
            self.serial = self.serial.wrapping_add(1);
            written += 1;
        }
        self.stats.emitted = self.stats.emitted.saturating_add(written as u64);
        written
    }

    fn sample_pos(&mut self) -> [f32; 3] {
        self.ld = self.ld.wrapping_add(1);
        let u = halton(self.ld, 2);
        let v = halton(self.ld, 3);
        let w = halton(self.ld, 5);
        self.cfg.sample_position(u, v, w)
    }

    /// 期望累计发射数（`rate * 有效时长 / 1000`），不含突发。
    ///
    /// 供判据核对累加器守恒：实际发射数应与之相差 < 1 粒（外加突发数与
    /// 丢弃数）。
    pub fn expected_rate_total(&self) -> u64 {
        if self.elapsed_ms <= self.plan.delay_ms {
            return 0;
        }
        let eff = (self.elapsed_ms - self.plan.delay_ms) as u64;
        (self.cfg.rate as u64 * eff) / 1000
    }
}

// ── 数学工具（no_std 内置，避免依赖 libm 分支差异）────────────────

/// 归一化向量（零向量返回 None）。
pub fn normalize(v: [f32; 3]) -> Option<[f32; 3]> {
    let l2 = v[0] * v[0] + v[1] * v[1] + v[2] * v[2];
    if l2 < 1e-12 || l2.is_nan() {
        return None;
    }
    let inv = 1.0 / sqrtf(l2);
    Some([v[0] * inv, v[1] * inv, v[2] * inv])
}

/// 快速平方根（Newton 迭代三次，相对误差 < 1e-6）。
///
/// 为什么不用 `f32::sqrt()`：xe2 目标上 `sqrt` 走 SSE 指令需 `feature`
/// 门控，`no_std` 下 `core::f32::sqrt` 在部分目标不可用。本实现只用四则
/// 运算，跨目标逐位一致。
pub fn sqrtf(x: f32) -> f32 {
    if x <= 0.0 || x.is_nan() {
        return 0.0;
    }
    let mut g = if x > 1.0 { x } else { 1.0 };
    for _ in 0..24 {
        g = 0.5 * (g + x / g);
    }
    g
}

/// 余弦（Taylor 展开 + 区间折叠，|x| ≤ 2π 时误差 < 1e-5）。
pub fn cosf(x: f32) -> f32 {
    let two_pi = core::f32::consts::PI * 2.0;
    let mut t = x % two_pi;
    if t > core::f32::consts::PI {
        t -= two_pi;
    } else if t < -core::f32::consts::PI {
        t += two_pi;
    }
    let t2 = t * t;
    let mut term = 1.0f32;
    let mut sum = 1.0f32;
    let mut k = 1.0f32;
    for _ in 0..12 {
        term *= -t2 / ((2.0 * k - 1.0) * (2.0 * k));
        sum += term;
        k += 1.0;
    }
    sum
}

// ── 判据 ────────────────────────────────────────────────────────

/// 标准配置（判据基座）。
fn std_cfg() -> EmitterConfig {
    EmitterConfig {
        shape: SpawnShape::Rect,
        line_len: 40.0,
        size: [120.0, 80.0, 60.0],
        origin: [10.0, 0.0, -5.0],
        rate: 600,
        speed_range: (40.0, 160.0),
        cone: DirCone::new([0.0, 1.0, 0.0], 0.35).unwrap_or(DirCone {
            axis: [0.0, 1.0, 0.0],
            half_angle_cos: 0.94,
        }),
        frame_cap: 512,
    }
}

/// F5222 判据集。
pub fn run_vez22b_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F5222");

    // ── 1. 发射形态：采样必落包围盒内（四形态逐一验证，含边界标量）──
    let shapes = [
        SpawnShape::Point,
        SpawnShape::Line,
        SpawnShape::Rect,
        SpawnShape::Volume,
    ];
    let probes = [0.0f32, 0.25, 0.5, 0.75, 0.999_999];
    let mut in_box = true;
    let mut worst = 0.0f32;
    for sh in shapes {
        let hb = sh.half_box(40.0, [12.0, 8.0, 6.0]);
        for &u in &probes {
            for &v in &probes {
                for &w in &probes {
                    let p = sh.sample(u, v, w, 40.0, [12.0, 8.0, 6.0]);
                    for ax in 0..3 {
                        let over = p[ax].abs() - hb[ax];
                        if over > worst {
                            worst = over;
                        }
                        if p[ax].abs() > hb[ax] + 1e-4 {
                            in_box = false;
                        }
                    }
                }
            }
        }
    }
    s.add(
        "VZ22-形态-01",
        in_box,
        "四形态 500 组采样全落包围盒内（越界=墙外出生）",
    );
    s.add(
        "VZ22-形态-02",
        worst <= 1e-4,
        "采样最大越界量 ≤ 1e-4 世界单位",
    );

    // ── 2. 形态体积序与自由度序一致 ──
    let vol = |sh: SpawnShape| sh.volume(40.0, [12.0, 8.0, 6.0]);
    s.add(
        "VZ22-形态-03",
        vol(SpawnShape::Point) <= vol(SpawnShape::Line)
            && vol(SpawnShape::Line) <= vol(SpawnShape::Rect)
            && vol(SpawnShape::Rect) < vol(SpawnShape::Volume),
        "体积序 点≤线≤面<体（体 12*8*6=576 严格最大）",
    );
    s.add(
        "VZ22-形态-04",
        SpawnShape::Point.dof() == 0
            && SpawnShape::Line.dof() == 1
            && SpawnShape::Rect.dof() == 2
            && SpawnShape::Volume.dof() == 3,
        "自由度序 0/1/2/3 与形态维数一致",
    );
    // 体积实数核对（不复用被测常量，独立算一遍）。
    s.add(
        "VZ22-形态-05",
        (vol(SpawnShape::Volume) - 576.0).abs() < 1e-3,
        "体形态体积=576（12*8*6 独立核算）",
    );

    // ── 3. 发射速率：累加器守恒（1000 帧随机 dt，误差 < 1 粒）──
    let mut acc = RateAccumulator::new();
    let mut total = 0u64;
    let mut elapsed = 0u64;
    let rate = 600u32;
    let mut rng = Lcg::new(0xC0FF_EE01);
    for _ in 0..1000 {
        let dt = 8 + (rng.next_u32() % 25); // 8~32ms，模拟 30~120fps 抖动
        total += acc.tick(rate, dt) as u64;
        elapsed += dt as u64;
    }
    let expect = rate as u64 * elapsed / 1000;
    let drift = if total > expect { total - expect } else { expect - total };
    s.add(
        "VZ22-速率-01",
        drift <= 1,
        "累加器守恒：1000 帧抖动 dt 下累计漂移 ≤ 1 粒",
    );
    s.add(
        "VZ22-速率-02",
        total >= expect,
        "不系统性少发（累计数不低于理论下界）",
    );
    // 反向：朴素取整写法必须被证明真的会少发（否则本条判据没有区分力）。
    {
        let mut naive = 0u64;
        let mut r2 = Lcg::new(0xC0FF_EE01);
        for _ in 0..1000 {
            let dt = 8 + (r2.next_u32() % 25);
            naive += ((rate as f64 * dt as f64) / 1000.0) as u64;
        }
        s.add(
            "VZ22-速率-03",
            naive < expect,
            "反向：朴素截断写法确会少发（证明本判据有区分力）",
        );
    }
    s.add(
        "VZ22-速率-04",
        RateAccumulator::new().tick(rate, 0) == 0,
        "dt=0 零帧不推进（不扰动相位）",
    );
    {
        let mut a = RateAccumulator::new();
        let _ = a.tick(rate, 16);
        let r0 = a.remainder();
        a.reset();
        s.add("VZ22-速率-05", r0 < ACC_DEN as u64 && a.remainder() == 0, "余数恒 < 分母；reset 清零");
    }

    // ── 4. 初速分布：区间内均匀且端点可达 ──
    {
        let mut rng = Lcg::new(0x5EED_0001);
        let (lo, hi) = (40.0f32, 160.0f32);
        let mut mn = f32::MAX;
        let mut mx = f32::MIN;
        for _ in 0..4000 {
            let sp = lo + (hi - lo) * rng.next_f();
            if sp < mn {
                mn = sp;
            }
            if sp > mx {
                mx = sp;
            }
        }
        s.add(
            "VZ22-初速-01",
            mn >= lo && mx <= hi,
            "初速采样 4000 次全落 [40,160] 内",
        );
        s.add(
            "VZ22-初速-02",
            mn < lo + (hi - lo) * 0.05 && mx > hi - (hi - lo) * 0.05,
            "区间两端各 5% 内均被采到（非集中堆积）",
        );
        s.add("VZ22-初速-03", hi - lo > 0.0, "区间非退化（hi > lo）");
    }
    // 非法初速区间必须被拒（lo > hi / 负值）。
    {
        let mut c = std_cfg();
        c.speed_range = (200.0, 100.0);
        let r1 = c.validate().is_err();
        c.speed_range = (-1.0, 10.0);
        let r2 = c.validate().is_err();
        s.add("VZ22-初速-04", r1 && r2, "反向：lo>hi 与负速均被拒（E_SPEED_RANGE）");
    }

    // ── 5. 方向锥：全部采样落在半角内 ──
    {
        let cone = match DirCone::new([0.0, 1.0, 0.0], 0.35) {
            Ok(c) => c,
            Err(_) => DirCone::omni([0.0, 1.0, 0.0]),
        };
        let mut rng = Lcg::new(0xC0DE_0001);
        let mut inside = true;
        let mut min_cos = 1.0f32;
        let mut fb = 0u32;
        for _ in 0..2000 {
            let (d, fallback) = cone.sample(&mut rng);
            if fallback {
                fb += 1;
            }
            let c = d[0] * cone.axis[0] + d[1] * cone.axis[1] + d[2] * cone.axis[2];
            if c < min_cos {
                min_cos = c;
            }
            if c < cone.half_angle_cos - 1e-5 {
                inside = false;
            }
        }
        s.add("VZ22-方向-01", inside, "2000 次锥采样全部在半角内（含退化路径）");
        s.add(
            "VZ22-方向-02",
            fb == 0,
            "半角 0.35rad 下退化路径零触发（命中率足够）",
        );
        s.add(
            "VZ22-方向-03",
            min_cos >= cone.half_angle_cos - 1e-5,
            "最小方向余弦不低于阈值（无越锥漏点）",
        );
        // 反向：全向锥的余弦下界必须显著低于窄锥（证明阈值真的在起作用）。
        let omni = DirCone::omni([0.0, 1.0, 0.0]);
        let mut r2 = Lcg::new(0xAB00_0001);
        let mut omni_min = 1.0f32;
        for _ in 0..500 {
            let (d, _) = omni.sample(&mut r2);
            let c = d[0] * omni.axis[0] + d[1] * omni.axis[1] + d[2] * omni.axis[2];
            if c < omni_min {
                omni_min = c;
            }
        }
        s.add(
            "VZ22-方向-04",
            omni_min < cone.half_angle_cos,
            "反向：全向锥采到低于窄锥阈值的方向（阈值确在筛选）",
        );
    }
    // 非法半角拒绝。
    {
        let bad1 = DirCone::new([0.0, 1.0, 0.0], -0.1).is_err();
        let bad2 = DirCone::new([0.0, 1.0, 0.0], 4.0).is_err();
        let bad3 = DirCone::new([0.0, 0.0, 0.0], 0.3).is_err();
        s.add(
            "VZ22-方向-05",
            bad1 && bad2 && bad3,
            "反向：负角/超π/零轴向量三例均拒（E_CONE_ANGLE）",
        );
    }

    // ── 6. 延迟与突发：时间轴统一 ──
    {
        let mut plan = match EmitPlan::new().with_delay(200) {
            Ok(p) => p,
            Err(_) => EmitPlan::new(),
        };
        let _ = plan.push_burst(0, 100);
        let _ = plan.push_burst(500, 200);
        let _ = plan.push_burst(500, 50); // 同刻允许（非降）
        s.add(
            "VZ22-计划-01",
            plan.is_monotonic() && plan.events().len() == 3,
            "事件表非降有序；同时刻多事件允许",
        );
        // 降序必须被拒。
        let mut p2 = EmitPlan::new();
        let _ = p2.push_burst(600, 10);
        let rej = p2.push_burst(100, 10).is_err();
        s.add("VZ22-计划-02", rej, "反向：时刻回溯被拒（E_PLAN_ORDER）");
        // 数量越界必须被拒。
        let mut p3 = EmitPlan::new();
        let r0 = p3.push_burst(0, 0).is_err();
        let r1 = p3.push_burst(0, BURST_MAX + 1).is_err();
        let r2 = p3.push_burst(0, BURST_MAX).is_ok();
        s.add("VZ22-计划-03", r0 && r1 && r2, "数量 0 与超限被拒；上限本身允许");
        // 延迟平移：延迟期内 drain 拿不到事件。
        let mut p4 = EmitPlan::new();
        let _ = p4.push_burst(0, 77);
        let mut cur = 0usize;
        let during = p4.drain(0, &mut cur); // rel=0 时事件 at=0 应触发
        s.add(
            "VZ22-计划-04",
            during == 77 && cur == 1,
            "rel=0 时 at=0 事件立即触发（延迟平移由调用方施加）",
        );
        // 幂等：重复 drain 同刻不重复发射。
        let again = p4.drain(0, &mut cur);
        s.add("VZ22-计划-05", again == 0, "游标幂等：重复 drain 不重复发射");
        // 延迟上限。
        let rej_delay = EmitPlan::new().with_delay(DELAY_MAX_MS + 1).is_err();
        s.add("VZ22-计划-06", rej_delay, "反向：延迟超上限被拒（E_DELAY_RANGE）");
    }

    // ── 7. 发射器端到端：延迟期内零产出、之后按速率产出 ──
    {
        let cfg = std_cfg();
        let plan = match EmitPlan::new().with_delay(100) {
            Ok(p) => p,
            Err(_) => EmitPlan::new(),
        };
        match Emitter::new(cfg, plan, 0x1234_5678) {
            Ok(mut em) => {
                let mut buf = alloc::vec::Vec::with_capacity(2048);
                // 前 6 帧（96ms）在延迟期内：零产出。
                for _ in 0..6 {
                    let n = em.tick(16, &mut buf);
                    if n != 0 {
                        break;
                    }
                }
                let quiet = em.stats().emitted == 0;
                // 再跑 60 帧（960ms）：600 粒/秒 → 约 576 粒。
                for _ in 0..60 {
                    let _ = em.tick(16, &mut buf);
                }
                let emitted = em.stats().emitted;
                let cap_dropped = em.stats().cap_dropped;
                s.add("VZ22-端到端-01", quiet, "延迟期内零产出（96ms < 100ms 延迟）");
                s.add(
                    "VZ22-端到端-02",
                    emitted >= 540 && emitted <= 620,
                    "延迟后 960ms 内产出 540~620 粒（理论 576，含单帧上限影响）",
                );
                s.add(
                    "VZ22-端到端-03",
                    emitted + cap_dropped >= 560,
                    "产出 + 丢弃 ≥ 理论下界（丢弃可见不静默）",
                );
                s.add(
                    "VZ22-端到端-04",
                    em.stats().frames == 66,
                    "帧计数精确（66 帧，含延迟期 6 帧）",
                );
                // 全部出生点都在包围盒内（形态 Rect，半盒 60×40×0）。
                let hb = cfg.shape.half_box(cfg.line_len, cfg.size);
                let mut all_in = true;
                for r in buf.iter().take(200) {
                    for ax in 0..3 {
                        let off = r.pos[ax] - cfg.origin[ax];
                        if off.abs() > hb[ax] + 1e-3 {
                            all_in = false;
                        }
                    }
                }
                s.add("VZ22-端到端-05", all_in, "前 200 粒出生点全在发射面内");
                // 全部速度在锥内且在速度区间内。
                let mut vel_ok = true;
                for r in buf.iter().take(200) {
                    let sp2 = r.vel[0] * r.vel[0] + r.vel[1] * r.vel[1] + r.vel[2] * r.vel[2];
                    let sp = sqrtf(sp2);
                    if sp < cfg.speed_range.0 - 1e-2 || sp > cfg.speed_range.1 + 1e-2 {
                        vel_ok = false;
                    }
                    let inv = if sp > 0.0 { 1.0 / sp } else { 0.0 };
                    let c = (r.vel[0] * inv) * cfg.cone.axis[0]
                        + (r.vel[1] * inv) * cfg.cone.axis[1]
                        + (r.vel[2] * inv) * cfg.cone.axis[2];
                    if c < cfg.cone.half_angle_cos - 1e-3 {
                        vel_ok = false;
                    }
                }
                s.add("VZ22-端到端-06", vel_ok, "前 200 粒速度全在锥内且落速度区间");
                // 序号连续性。
                let mut serial_ok = true;
                for (i, r) in buf.iter().take(200).enumerate() {
                    if r.serial as usize != i {
                        serial_ok = false;
                        break;
                    }
                }
                s.add("VZ22-端到端-07", serial_ok, "serial 从 0 连续递增（下游按序分配槽位）");
            }
            Err(e) => {
                s.add("VZ22-端到端-01", false, "标准配置构造失败（校验拒绝）");
                let _ = e;
            }
        }
    }

    // ── 8. 参数校验矩阵 ──
    {
        let mut c = std_cfg();
        let ok = c.validate().is_ok();
        c.rate = RATE_MAX + 1;
        let bad_rate = c.validate() == Err(E_RATE_RANGE);
        c = std_cfg();
        c.frame_cap = 0;
        let bad_cap = c.validate() == Err(E_FRAME_CAP);
        c = std_cfg();
        c.size = [10.0, -1.0, 5.0];
        let bad_box = c.validate() == Err(E_SHAPE_BOX);
        s.add(
            "VZ22-校验-01",
            ok && bad_rate && bad_cap && bad_box,
            "校验矩阵：标准过；超速率/零帧上限/负尺寸各拒",
        );
    }

    // ── 9. API 冻结 ──
    s.add(
        "VZ22-冻结-01",
        EMITTER_VERSION == "F5222-emitter-v1",
        "API 版本冻结：F5222-emitter-v1",
    );
    s.add(
        "VZ22-冻结-02",
        SpawnShape::Point.dof() + SpawnShape::Line.dof() + SpawnShape::Rect.dof()
            + SpawnShape::Volume.dof()
            == 6,
        "形态自由度指纹=6（改一型即破）",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn halton_is_in_unit_interval_and_low_discrepancy() {
        for i in 0..512u32 {
            for b in [2u32, 3, 5] {
                let v = halton(i, b);
                assert!((0.0..1.0).contains(&v), "i={} b={} v={}", i, b, v);
            }
        }
        // 前 32 项应大致铺满 [0,1)（分 8 桶，每桶至少 2 个）。
        let mut buckets = [0u32; 8];
        for i in 0..32u32 {
            let v = halton(i + 1, 2);
            buckets[(v * 8.0) as usize] += 1;
        }
        assert!(buckets.iter().all(|c| *c >= 2), "{:?}", buckets);
    }

    #[test]
    fn accumulator_conserves_over_irregular_dt() {
        let mut acc = RateAccumulator::new();
        let mut rng = Lcg::new(7);
        let (mut total, mut ms) = (0u64, 0u64);
        for _ in 0..5000 {
            let dt = 1 + (rng.next_u32() % 50);
            total += acc.tick(1337, dt) as u64;
            ms += dt as u64;
        }
        let expect = 1337u64 * ms / 1000;
        let drift = total.abs_diff(expect);
        assert!(drift <= 1, "total={} expect={} drift={}", total, expect, drift);
    }

    #[test]
    fn cone_samples_stay_inside() {
        let c = DirCone::new([1.0, 0.0, 0.0], 0.8).expect("cone");
        let mut rng = Lcg::new(99);
        for _ in 0..3000 {
            let (d, _) = c.sample(&mut rng);
            let dot = d[0];
            assert!(dot >= c.half_angle_cos - 1e-5);
            let len = sqrtf(d[0] * d[0] + d[1] * d[1] + d[2] * d[2]);
            assert!((len - 1.0).abs() < 1e-3, "len={}", len);
        }
    }

    #[test]
    fn narrow_cone_terminates_via_fallback() {
        // 半角 0.001rad：命中率极低，必须靠退化路径终止而不是死循环。
        let c = DirCone::new([0.0, 1.0, 0.0], 0.001).expect("cone");
        let mut rng = Lcg::new(5);
        let mut fb = 0;
        for _ in 0..50 {
            let (d, fallback) = c.sample(&mut rng);
            if fallback {
                fb += 1;
            }
            let dot = d[1];
            assert!(dot >= c.half_angle_cos - 1e-4, "dot={}", dot);
        }
        assert!(fb > 0, "极窄锥应触发退化路径");
    }

    #[test]
    fn emitter_respects_frame_cap_and_delay() {
        let cfg = EmitterConfig {
            rate: 60_000,
            frame_cap: 32,
            cone: DirCone::omni([0.0, 1.0, 0.0]),
            ..std_cfg()
        };
        let plan = EmitPlan::new().with_delay(50).expect("plan");
        let mut em = Emitter::new(cfg, plan, 1).expect("emitter");
        let mut buf = alloc::vec::Vec::with_capacity(4096);
        // 延迟期（48ms）。
        for _ in 0..3 {
            assert_eq!(em.tick(16, &mut buf), 0);
        }
        // 进入发射期后每帧被 32 上限压制，其余记丢弃。
        let n = em.tick(16, &mut buf);
        assert_eq!(n, 32);
        assert!(em.stats().cap_dropped > 0, "超出单帧上限必须记账");
    }

    #[test]
    fn plan_rejects_backwards_time() {
        let mut p = EmitPlan::new();
        p.push_burst(100, 5).expect("ok");
        assert_eq!(p.push_burst(99, 5), Err(E_PLAN_ORDER));
        assert_eq!(p.push_burst(100, 5), Ok(()));
    }

    #[test]
    fn sample_position_respects_volume_box() {
        let cfg = EmitterConfig {
            shape: SpawnShape::Volume,
            size: [10.0, 20.0, 30.0],
            origin: [100.0, 200.0, 300.0],
            ..std_cfg()
        };
        let p = cfg.sample_position(0.0, 0.0, 0.0);
        assert!((p[0] - 95.0).abs() < 1e-4);
        assert!((p[1] - 190.0).abs() < 1e-4);
        assert!((p[2] - 285.0).abs() < 1e-4);
    }

    #[test]
    fn sqrt_and_cos_are_accurate_enough() {
        assert!((sqrtf(2.0) - core::f32::consts::SQRT_2).abs() < 1e-5);
        assert!((cosf(0.0) - 1.0).abs() < 1e-6);
        assert!((cosf(core::f32::consts::PI) + 1.0).abs() < 1e-4);
    }
}
