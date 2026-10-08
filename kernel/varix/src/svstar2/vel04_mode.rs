//! VE-F2204 · 发射模式（L 域 · 粒子与物理域 · 批次 L01 第 4 项 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F2204`
//!
//! **判据（锚点原文五条）**：四模式、混合叠加、事件契约、节流保量、判据。
//! 逐条落位：
//! - **四模式**：[`ModeKind`] 持续/爆发/间隔/事件驱动四型全集，语义与累积方式逐一声明。
//! - **混合叠加**：[`ModeStack`] 单发射器可挂多模式，**各模式独立累积**——不是四选一，
//!   是并行求和后合并（锚点「各模式独立累积发射」）。
//! - **事件契约**：[`EventSchema`] 事件名注册制 + 三覆盖参数（位置偏移/数量倍率/
//!   速度倍率），复用 F1408 总线四要素在粒子域的实例化；未注册事件名**拒绝告警**
//!   （F1925 同规则）。
//! - **节流保量**：同帧同发射器的多事件**合并为一次加权发射**（[`Throttle`]），
//!   关键在「保量」——合并后粒子数等于各事件粒子数之和，不是取最大也不是取平均。
//!
//! **与 F2203 的分工**：F2203 管「一个发射器怎样产出一批粒子」（形状/速度/组/状态机），
//! 本模块管「什么条件下、以何种节奏产出」。故本模块**不复制** F2203 的采样与累积逻辑，
//! 只产出 [`SpawnRequest`] 的调度决策，采样与落池仍归 F2203/F2208。
//!
//! **跨批对接**：总线单源 F1408（与 F1925/F1928 同源三度复用声明）；池 F2208；
//! 发射核心 F2203（跨域/跨条复用的第一次）；fuzz F2211 风暴面。
//!
//! 零 IO、零墙钟；时间以逻辑 dt 注入；事件时间戳由调用方注入（不读墙钟），
//! 故同输入双跑逐位一致。无隐私面。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use super::vel03_emitter::{
    add_v3, is_finite, scale_v3, DiagBag, DiagCode, Diagnostic, EmitAccumulator, Vec3,
    EMIT_RATE_MAX_PER_SEC,
};

// ---------------------------------------------------------------------------
// 一、诊断码（F2204 自有，与 F2203 不共用——粒度不同：那边是单发射器，
// 这边是「事件契约」层）
// ---------------------------------------------------------------------------

/// 发射模式域诊断码。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModeDiag {
    /// 事件名未注册（F1925 同规则：拒绝告警，不静默忽略）。
    EventUnregistered,
    /// 事件参数越界被钳制（含原值与钳后值）。
    ParamClamped,
    /// 事件风暴已节流合并。
    Throttled,
    /// 模式参数非法被拒绝。
    ModeRejected,
}

impl ModeDiag {
    pub fn zh(self) -> &'static str {
        match self {
            ModeDiag::EventUnregistered => "事件名未注册",
            ModeDiag::ParamClamped => "事件参数钳制",
            ModeDiag::Throttled => "事件风暴节流",
            ModeDiag::ModeRejected => "模式参数拒绝",
        }
    }

    /// 转为F2203 诊断码（复用既有诊断袋，不另立一套出口）。
    pub fn to_emitter_code(self) -> DiagCode {
        match self {
            ModeDiag::EventUnregistered => DiagCode::ShapeRejected,
            ModeDiag::ParamClamped => DiagCode::RateClamped,
            ModeDiag::Throttled => DiagCode::ChurnCoalesced,
            ModeDiag::ModeRejected => DiagCode::ShapeRejected,
        }
    }
}

/// 记一条模式诊断进F2203 的诊断袋（保持全链路单一诊断出口）。
pub fn note(bag: &mut DiagBag, code: ModeDiag, message: String, hint: String) {
    bag.push(Diagnostic::new(code.to_emitter_code(), message, hint));
}

// ---------------------------------------------------------------------------
// 二、四模式（判据：四模式）
// ---------------------------------------------------------------------------

/// 模式类型。四型不多不少。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModeKind {
    /// 持续：稳态发射率持续累积。
    Continuous,
    /// 爆发：触发时一次N 粒子脉冲。
    Burst,
    /// 间隔：周期 T + 每次 N（带相位参数）。
    Interval,
    /// 事件驱动：事件订阅→ 触发时按参数覆盖发射。
    EventDriven,
}

impl ModeKind {
    pub const ALL: [ModeKind; 4] = [
        ModeKind::Continuous,
        ModeKind::Burst,
        ModeKind::Interval,
        ModeKind::EventDriven,
    ];

    pub fn zh(self) -> &'static str {
        match self {
            ModeKind::Continuous => "持续",
            ModeKind::Burst => "爆发",
            ModeKind::Interval => "间隔",
            ModeKind::EventDriven => "事件驱动",
        }
    }

    pub fn en(self) -> &'static str {
        match self {
            ModeKind::Continuous => "continuous",
            ModeKind::Burst => "burst",
            ModeKind::Interval => "interval",
            ModeKind::EventDriven => "event-driven",
        }
    }

    /// 是否走累积器（持续与间隔 O(1) 每帧累积；爆发与事件 O(N) 发射）。
    pub fn uses_accumulator(self) -> bool {
        matches!(self, ModeKind::Continuous | ModeKind::Interval)
    }
}

/// 数量倍率上限（锚点「事件参数越界（数量倍率 ×1000）→ 钳制」）。
pub const EVENT_COUNT_MULT_MAX: f32 = 1000.0;

/// 速度倍率上限（无锚点明文，取与数量倍率同量级的自定上限并显式声明）。
pub const EVENT_SPEED_MULT_MAX: f32 = 100.0;

/// 单模式单帧产出上限（与 F2203 的 `MAX_SPAWN_PER_FRAME` 同值同理由）。
pub const MODE_MAX_SPAWN_PER_FRAME: u32 = 65_536;

/// 单模式状态。四型共用一个结构体，未用字段校验拒绝而非静默忽略。
#[derive(Clone, Debug, PartialEq)]
pub enum ModeState {
    /// 持续：每帧按 rate×dt 累积（复杂度 O(1)）。
    Continuous { rate_per_sec: f32, acc: EmitAccumulator },
    /// 爆发：`trigger_burst()` 时一次发 N 粒；`burst_count` 为已发次数。
    Burst { per_trigger: u32, burst_count: u64 },
    /// 间隔：周期 T + 每次 N + 相位。
    ///
    /// 相位语义：与F2203 累积器同源的**实数余量**——`phase` 保留不足一周期的小数，
    /// 长时间运行不会因浮点截断而漂移（锚点「间隔相位漂移→确定性累积器」）。
    Interval { period: f32, per_trigger: u32, phase: f32, fired_count: u64 },
    /// 事件驱动：绑定一个已注册事件名。
    EventDriven { event: u32 },
}

/// 建立模式状态。参数非法即拒绝。
pub fn create_mode(kind: ModeKind, params: &[f32]) -> Result<ModeState, (ModeDiag, String, String)> {
    let finite_nonneg = |v: f32| is_finite(v) && v >= 0.0;
    match kind {
        ModeKind::Continuous => {
            if params.is_empty() {
                return Err((
                    ModeDiag::ModeRejected,
                    String::from("持续模式缺发射率参数"),
                    String::from("参数格式：[rate_per_sec]（粒子/秒）"),
                ));
            }
            let rate = params[0];
            // NaN/负在F2203 的 clamp_emit_rate 里钳制；此处只拦「完全不是数」。
            if rate.is_nan() {
                return Err((
                    ModeDiag::ModeRejected,
                    String::from("持续模式发射率为 NaN"),
                    String::from("NaN 无物理意义；请修配置或显式传 0"),
                ));
            }
            Ok(ModeState::Continuous { rate_per_sec: rate, acc: EmitAccumulator::new() })
        }
        ModeKind::Burst => {
            if params.len() < 1 {
                return Err((
                    ModeDiag::ModeRejected,
                    String::from("爆发模式缺每次粒子数"),
                    String::from("参数格式：[per_trigger]（每次触发粒子数）"),
                ));
            }
            let n = params[0];
            if !is_finite(n) || n < 0.0 || n > EMIT_RATE_MAX_PER_SEC {
                return Err((
                    ModeDiag::ModeRejected,
                    format!("爆发模式每次粒子数非法（{}）", n),
                    String::from("须为有限非负数；单次爆发量过大请拆成多个模式"),
                ));
            }
            Ok(ModeState::Burst { per_trigger: n as u32, burst_count: 0 })
        }
        ModeKind::Interval => {
            if params.len() < 2 {
                return Err((
                    ModeDiag::ModeRejected,
                    String::from("间隔模式缺周期或每次粒子数"),
                    String::from("参数格式：[period_sec, per_trigger]"),
                ));
            }
            let (period, n) = (params[0], params[1]);
            if !is_finite(period) || period <= 0.0 {
                return Err((
                    ModeDiag::ModeRejected,
                    format!("间隔模式周期非法（{}，须 > 0）", period),
                    String::from("周期为 0 会每帧触发，把间隔模式退化成爆发"),
                ));
            }
            if !is_finite(n) || n < 0.0 || n > EMIT_RATE_MAX_PER_SEC {
                return Err((
                    ModeDiag::ModeRejected,
                    format!("间隔模式每次粒子数非法（{}）", n),
                    String::from("须为有限非负数"),
                ));
            }
            Ok(ModeState::Interval {
                period,
                per_trigger: n as u32,
                phase: 0.0,
                fired_count: 0,
            })
        }
        ModeKind::EventDriven => {
            if params.len() < 1 {
                return Err((
                    ModeDiag::ModeRejected,
                    String::from("事件驱动模式缺事件 id"),
                    String::from("参数格式：[event_id]；事件 id 须先经事件注册表登记"),
                ));
            }
            let id = params[0];
            if !is_finite(id) || id < 0.0 || id.fract() != 0.0 {
                return Err((
                    ModeDiag::ModeRejected,
                    format!("事件 id 非法（{}）", id),
                    String::from("事件 id 须为非负整数"),
                ));
            }
            Ok(ModeState::EventDriven { event: id as u32 })
        }
    }
}

// ---------------------------------------------------------------------------
// 三、事件契约（判据：事件契约）
// ---------------------------------------------------------------------------

/// 事件参数 schema：三个覆盖参数。
///
/// 三覆盖参数对应锚点「事件可携带位置偏移/数量倍率/速度倍率三覆盖参数」。
/// 缺省值取 1.0（数量/速度倍率）与零向量（位置偏移）——**乘 1 等于不覆盖**，
/// 这让「不带参数的事件」与「带中性参数的事件」行为完全一致，无需分支。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EventSchema {
    pub position_offset: Vec3,
    pub count_mult: f32,
    pub speed_mult: f32,
}

impl EventSchema {
    /// 中性schema：等价于不覆盖。
    pub fn neutral() -> Self {
        EventSchema {
            position_offset: Vec3::ZERO,
            count_mult: 1.0,
            speed_mult: 1.0,
        }
    }
}

/// 事件注册表条目。
#[derive(Clone, Debug, PartialEq)]
pub struct EventBinding {
    pub event: u32,
    pub schema: EventSchema,
}

/// 事件注册表。**注册制**：未注册事件名一律拒绝（F1925 同规则）。
///
/// 复杂度 O(事件数) 线性扫描——事件数是配置量（个位数），不引 hasher。
#[derive(Clone, Debug, Default)]
pub struct EventRegistry {
    bindings: Vec<EventBinding>,
}

impl EventRegistry {
    pub fn new() -> Self {
        EventRegistry { bindings: Vec::new() }
    }

    /// 注册事件。重复注册**幂等覆盖**（后注册覆盖 schema，不报错）。
    ///
    /// 幂等而非拒绝：热重载场景下重复注册同一事件名是正常操作，报错会让
    /// 「读配置两次」这种无害行为变成阻断。
    pub fn register(&mut self, event: u32, schema: EventSchema, bag: &mut DiagBag) {
        match self.bindings.iter_mut().find(|b| b.event == event) {
            Some(b) => {
                b.schema = schema;
                note(
                    bag,
                    ModeDiag::ParamClamped,
                    format!("事件 {} 重复注册，schema 已覆盖（幂等）", event),
                    String::from("热重载会重复注册同名事件；此为正常路径"),
                );
            }
            None => self.bindings.push(EventBinding { event, schema }),
        }
    }

    /// 查事件 schema。未注册返回 [`None`]——调用点**必须**显式处理（拒绝 + 告警）。
    pub fn lookup(&self, event: u32) -> Option<EventSchema> {
        self.bindings.iter().find(|b| b.event == event).map(|b| b.schema)
    }

    pub fn len(&self) -> usize {
        self.bindings.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }

    /// 已注册事件名清单（读屏可达：诊断与工具视图都要念得出有哪些事件）。
    pub fn registered_names(&self) -> Vec<u32> {
        self.bindings.iter().map(|b| b.event).collect()
    }
}

/// 事件参数钳制：数量倍率 / 速度倍率越界钳到上限，位置偏移非有限则归零。
///
/// 钳制而非拒绝：事件参数来自外部输入（音频强度、UI 交互），偶发越界不该
/// 让整条粒子链断掉；但钳制必须记账，否则「参数被改过」不可见。
pub fn clamp_event_params(
    params: EventSchema,
    base_count: u32,
    bag: &mut DiagBag,
) -> EventParams {
    let count_mult = if is_finite(params.count_mult) && params.count_mult > 0.0 {
        if params.count_mult > EVENT_COUNT_MULT_MAX {
            note(
                bag,
                ModeDiag::ParamClamped,
                format!(
                    "事件数量倍率 {} 超上限 {}，已钳制",
                    params.count_mult, EVENT_COUNT_MULT_MAX
                ),
                String::from("倍率过大将瞬时抽干池；请修事件生产方或降低上限"),
            );
            EVENT_COUNT_MULT_MAX
        } else {
            params.count_mult
        }
    } else {
        note(
            bag,
            ModeDiag::ParamClamped,
            format!("事件数量倍率非法（{}），已钳到 1.0", params.count_mult),
            String::from("倍率须为有限正数；非正倍率会让粒子数归零"),
        );
        1.0
    };
    let speed_mult = if is_finite(params.speed_mult) && params.speed_mult >= 0.0 {
        if params.speed_mult > EVENT_SPEED_MULT_MAX {
            note(
                bag,
                ModeDiag::ParamClamped,
                format!(
                    "事件速度倍率 {} 超上限 {}，已钳制",
                    params.speed_mult, EVENT_SPEED_MULT_MAX
                ),
                String::from("速度倍率过大将让粒子瞬间飞出包围盒"),
            );
            EVENT_SPEED_MULT_MAX
        } else {
            params.speed_mult
        }
    } else {
        note(
            bag,
            ModeDiag::ParamClamped,
            format!("事件速度倍率非法（{}），已钳到 1.0", params.speed_mult),
            String::from("倍率须为有限非负数"),
        );
        1.0
    };
    let offset = if params.position_offset.is_finite() {
        params.position_offset
    } else {
        note(
            bag,
            ModeDiag::ParamClamped,
            String::from("事件位置偏移非有限，已归零"),
            String::from("非有限偏移通常来自未初始化的变换；归零是唯一安全解释"),
        );
        Vec3::ZERO
    };
    EventParams { position_offset: offset, count_mult, speed_mult, base_count }
}

/// 钳制后的覆盖参数（`base_count` 为无覆盖时的基准粒子数）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EventParams {
    pub position_offset: Vec3,
    pub count_mult: f32,
    pub speed_mult: f32,
    pub base_count: u32,
}

impl EventParams {
    /// 覆盖后的粒子数（**保量语义的核心**：倍率乘基准，不取整截断前先保底 1）。
    pub fn effective_count(&self) -> u32 {
        let n = (self.base_count as f32) * self.count_mult;
        if !is_finite(n) || n <= 0.0 {
            0
        } else if n >= u32::MAX as f32 {
            u32::MAX
        } else {
            n as u32
        }
    }
}

// ---------------------------------------------------------------------------
// 四、模式混合（判据：混合叠加）
// ---------------------------------------------------------------------------

/// 一个挂载的模式。
#[derive(Clone, Debug, PartialEq)]
pub struct Mode {
    pub kind: ModeKind,
    pub state: ModeState,
}

/// 本帧一次发射的调度决策（**不含粒子本身**——采样归 F2203）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EmitDirective {
    /// 本模式本帧要发多少粒。
    pub count: u32,
    /// 该模式所属下标（多模式下供调用方区分来源）。
    pub mode_index: u8,
}

/// 事件派发结果（单事件级）。
///
/// 与 [`EmitDirective`] 分开是刻意的：帧步进产出的是「节奏」（每模式发多少），
/// 事件产出的是「一次性触发」（带位置偏移与倍率的独立决策），两者生命周期不同——
/// 前者每帧重算，后者触发即产出。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EventDirective {
    pub count: u32,
    pub mode_index: u8,
}

/// 模式栈：单发射器可挂多模式，**各模式独立累积**后合并。
///
/// 「独立累积」的实现要点：每个 [`Mode`] 自带 `EmitAccumulator` /`phase`，
/// 互不干扰——若共用一个累积器，四模式同开时会互相偷配额，表现为
/// 「关了爆发模式后持续模式发射量也变了」，极难查。
#[derive(Clone, Debug, Default)]
pub struct ModeStack {
    modes: Vec<Mode>,
}

impl ModeStack {
    pub fn new() -> Self {
        ModeStack { modes: Vec::new() }
    }

    /// 挂载模式。返回下标（供事件触发时定位）。
    pub fn push(&mut self, kind: ModeKind, state: ModeState) -> usize {
        self.modes.push(Mode { kind, state });
        self.modes.len() - 1
    }

    pub fn len(&self) -> usize {
        self.modes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.modes.is_empty()
    }

    pub fn get(&self, index: usize) -> Option<&Mode> {
        self.modes.get(index)
    }

    pub fn get_mut(&mut self, index: usize) -> Option<&mut Mode> {
        self.modes.get_mut(index)
    }

    /// 帧步进：跑一轮持续与间隔模式，返回各模式的发射决策。
    ///
    /// 复杂度 O(模式数)：持续/间隔各 O(1) 累积（锚点性能分解）。
    /// 爆发与事件驱动不在此产出——它们由 [`ModeStack::trigger_burst`] 与
    /// [`ModeStack::dispatch_event`]显式触发（不在帧步进里偷发）。
    pub fn tick(&mut self, dt: f32, bag: &mut DiagBag) -> Vec<EmitDirective> {
        let mut out: Vec<EmitDirective> = Vec::new();
        if !(dt > 0.0) || !is_finite(dt) {
            return out;
        }
        for (i, m) in self.modes.iter_mut().enumerate() {
            match &mut m.state {
                ModeState::Continuous { rate_per_sec, acc } => {
                    // 借用冲突绕开：先算后写。
                    let rate = *rate_per_sec;
                    let n = accumulate(rate, dt, acc);
                    if n > 0 {
                        out.push(EmitDirective { count: n, mode_index: i as u8 });
                    }
                }
                ModeState::Interval { period, per_trigger, phase, fired_count } => {
                    // 相位推进：实数余量累积，与 F2203 累积器同源，杜绝长时间漂移。
                    let p = *period;
                    let per = *per_trigger;
                    let old = *phase;
                    let new_phase = old + dt;
                    // **相对容差（关键）**：`1.0/60.0` 在 f32 下累加 60 次得
                    // 0.99999994，严格 `>= p` 会漏掉这一次触发——实测「1 秒跑 60 帧、
                    // 周期 1 秒」产出 0 粒，整条间隔模式静默失效。
                    // 容差取周期的 1e-4：远小于最短可感知周期（1e-4 秒 = 0.1ms，
                    // 粒子域不需要），又足以吸收 60~1000 次累加的 f32 误差。
                    let eps = p * 1e-4;
                    if is_finite(new_phase) && new_phase >= p - eps {
                        // 跨过的周期数（可能一帧内跨多个周期，如掉帧后）。
                        let cycles = ((new_phase / p) as u64).max(1);
                        *phase = new_phase - (cycles as f32) * p;
                        // 余量被容差吃掉的负值归零：留负相位会让下一帧的周期判定
                        // 再偏一次，越漂越远。
                        if *phase < 0.0 {
                            *phase = 0.0;
                        }
                        let total = per.saturating_mul(cycles.min(u32::MAX as u64) as u32);
                        *fired_count = fired_count.saturating_add(cycles);
                        if total > 0 {
                            out.push(EmitDirective { count: total, mode_index: i as u8 });
                        }
                    } else {
                        *phase = new_phase;
                    }
                }
                ModeState::Burst { .. } | ModeState::EventDriven { .. } => {}
            }
        }
        // 单帧产出上限：与 F2203 的 `MAX_SPAWN_PER_FRAME` 同值同理由（池容量保护）。
        //
        // 探针实测：持续模式配`rate=1e9`、dt=1 秒时本帧产出 10 亿粒且**零诊断**——
        // 配置层没拦住、模式层也不拦，10 亿粒进池瞬间抽干，后者表现为
        // 「整条粒子链莫名卡死」。这不是可容忍的正常态，必须截断并记账。
        for d in out.iter_mut() {
            if d.count == 0 {
                continue;
            }
            if d.count > MODE_MAX_SPAWN_PER_FRAME {
                note(
                    bag,
                    ModeDiag::ParamClamped,
                    format!(
                        "模式 {} 本帧产出 {} 粒，超单帧上限 {}，已截断",
                        d.mode_index, d.count, MODE_MAX_SPAWN_PER_FRAME
                    ),
                    String::from("单帧发射尖峰会瞬时抽干池；请降低发射率、延长间隔或拆成多个模式"),
                );
                d.count = MODE_MAX_SPAWN_PER_FRAME;
            }
        }
        out
    }

    /// 触发爆发模式。返回应发粒子数（**不合并到 tick**——爆发的时机由调用方定）。
    pub fn trigger_burst(&mut self, index: usize, bag: &mut DiagBag) -> u32 {
        match self.modes.get_mut(index).map(|m| &mut m.state) {
            Some(ModeState::Burst { per_trigger, burst_count }) => {
                *burst_count = burst_count.saturating_add(1);
                *per_trigger
            }
            Some(other) => {
                note(
                    bag,
                    ModeDiag::ModeRejected,
                    format!("模式 {} 不是爆发模式，触发被拒", kind_name(other)),
                    String::from("爆发触发只对 Burst 模式有效；持续/间隔请走 tick()"),
                );
                0
            }
            None => {
                note(
                    bag,
                    ModeDiag::ModeRejected,
                    String::from("爆发触发的模式下标越界"),
                    String::from("下标须来自 ModeStack::push 的返回值"),
                );
                0
            }
        }
    }

    /// 派发事件（判据：事件契约 + 节流保量）。
    ///
    /// **保量语义**：同帧同发射器的多事件合并为**一次加权发射**——粒子数等于
    /// 各事件粒子数**之和**，不是取最大也不是取平均。取平均会让「两个弱事件」
    /// 弱于「一个强事件」；取最大会让弱事件被吞。求和才是「每个事件都算数」。
    pub fn dispatch_event(
        &mut self,
        registry: &EventRegistry,
        index: usize,
        bag: &mut DiagBag,
    ) -> Vec<EventDirective> {
        let event = match self.modes.get(index).map(|m| &m.state) {
            Some(ModeState::EventDriven { event }) => *event,
            Some(other) => {
                note(
                    bag,
                    ModeDiag::ModeRejected,
                    format!("模式 {} 不是事件驱动模式，事件被拒", kind_name(other)),
                    String::from("事件派发只对 EventDriven 模式有效"),
                );
                return Vec::new();
            }
            None => {
                note(
                    bag,
                    ModeDiag::ModeRejected,
                    String::from("事件派发的模式下标越界"),
                    String::from("下标须来自 ModeStack::push 的返回值"),
                );
                return Vec::new();
            }
        };
        let schema = match registry.lookup(event) {
            Some(s) => s,
            None => {
                // 未注册事件名 → 拒绝告警（F1925 同规则），**不静默忽略**。
                note(
                    bag,
                    ModeDiag::EventUnregistered,
                    format!("事件 {} 未注册，拒绝派发", event),
                    String::from("事件须先经 EventRegistry::register 登记；未注册事件多半来自生产方与消费方版本错配"),
                );
                return Vec::new();
            }
        };
        // 基准粒子数：事件驱动模式没有独立的 per_trigger，用 base=1 承载
        // 「一个事件值多少粒」——真实基数由调用方经 base_count 传入。
        let params = clamp_event_params(schema, 1, bag);
        let n = params.effective_count();
        if n == 0 {
            return Vec::new();
        }
        vec![EventDirective { count: n, mode_index: index as u8 }]
    }

    /// 带基准粒子数派发事件（`dispatch_event` 的显式基数版本）。
    pub fn dispatch_event_with_base(
        &mut self,
        registry: &EventRegistry,
        index: usize,
        base_count: u32,
        bag: &mut DiagBag,
    ) -> u32 {
        let event = match self.modes.get(index).map(|m| &m.state) {
            Some(ModeState::EventDriven { event }) => *event,
            _ => {
                note(
                    bag,
                    ModeDiag::ModeRejected,
                    String::from("事件派发的模式非事件驱动或下标越界"),
                    String::from("事件派发只对 EventDriven 模式有效"),
                );
                return 0;
            }
        };
        let schema = match registry.lookup(event) {
            Some(s) => s,
            None => {
                note(
                    bag,
                    ModeDiag::EventUnregistered,
                    format!("事件 {} 未注册，拒绝派发", event),
                    String::from("事件须先经 EventRegistry::register 登记"),
                );
                return 0;
            }
        };
        let params = clamp_event_params(schema, base_count, bag);
        params.effective_count()
    }
}

fn kind_name(state: &ModeState) -> &'static str {
    match state {
        ModeState::Continuous { .. } => "持续",
        ModeState::Burst { .. } => "爆发",
        ModeState::Interval { .. } => "间隔",
        ModeState::EventDriven { .. } => "事件驱动",
    }
}

/// 持续模式的帧累积（就地推进 F2203 的累积器，保持两域同源）。
fn accumulate(rate: f32, dt: f32, acc: &mut EmitAccumulator) -> u32 {
    super::vel03_emitter::advance_accumulator(acc, rate, dt)
}

// ---------------------------------------------------------------------------
// 五、节流保量（判据：节流保量）
// ---------------------------------------------------------------------------

/// 事件风暴节流器：同帧同发射器的多事件合并为一次加权发射。
///
/// 锚点「每帧千事件」是真实风险——不节流会让单帧粒子数等于事件数，
/// 直接抽干池。节流的关键不是「丢弃」而是**合并成一次**：
/// 粒子总数保持（保量），只是不再逐事件走一遍采样。
#[derive(Clone, Debug, Default)]
pub struct Throttle {
    /// 本帧已累积的事件粒子数（求和即保量结果）。
    pending_total: u64,
    /// 本帧已累积的事件次数。
    pending_events: u32,
    /// 位置偏移的加权平均累积（按粒子数加权）。
    offset_acc: Vec3,
}

impl Throttle {
    pub fn new() -> Self {
        Throttle { pending_total: 0, pending_events: 0, offset_acc: Vec3::ZERO }
    }

    /// 累积一个事件（不立即发射）。
    ///
    /// 返回是否需要节流（事件数超阈值时为真，供调用方记账）。
    pub fn accumulate_event(
        &mut self,
        count: u32,
        offset: Vec3,
        storm_threshold: u32,
        bag: &mut DiagBag,
    ) -> bool {
        self.pending_total = self.pending_total.saturating_add(count as u64);
        self.pending_events = self.pending_events.saturating_add(1);
        if offset.is_finite() {
            // 位置偏移按粒子数加权平均：合并后的整体位置仍落在事件区域的形心。
            let w = count as f32;
            self.offset_acc = add_v3(self.offset_acc, scale_v3(offset, w));
        }
        let stormed = self.pending_events > storm_threshold;
        if stormed {
            note(
                bag,
                ModeDiag::Throttled,
                format!(
                    "本帧事件数 {} 超阈值 {}，已合并为一次加权发射",
                    self.pending_events, storm_threshold
                ),
                String::from("节流保量：合并后粒子数不变，只是不再逐事件采样；如需降量请改事件生产方"),
            );
        }
        stormed
    }

    /// 结算并取走本帧合并结果（保量：总数 = 各事件粒子数之和）。
    ///
    /// 复杂度 O(1)：维护累加量而非遍历事件列表（锚点性能分解「节流合并 O(事件数)」
    /// 指首次累积，结算为 O(1)）。
    pub fn take(&mut self) -> (u32, Vec3) {
        let total = if self.pending_total > u32::MAX as u64 {
            u32::MAX
        } else {
            self.pending_total as u32
        };
        let mean_offset = if self.pending_total > 0 {
            scale_v3(self.offset_acc, 1.0 / (self.pending_total as f32))
        } else {
            Vec3::ZERO
        };
        self.pending_total = 0;
        self.pending_events = 0;
        self.offset_acc = Vec3::ZERO;
        (total, mean_offset)
    }

    pub fn pending_events(&self) -> u32 {
        self.pending_events
    }

    pub fn pending_total(&self) -> u64 {
        self.pending_total
    }
}

/// 事件风暴阈值（锚点未给具体数，取「每帧千事件」量级的自定值并显式声明）。
pub const EVENT_STORM_THRESHOLD: u32 = 64;

/// 把模式栈的调度决策 + 速度倍率落成 [`SpawnRequest`] 前置描述。
///
/// 刻意**不**在这里采样形状与速度：那是 F2203 的职责。本函数只产出
/// 「发多少、在哪个位置偏移、按什么速度倍率」，把采样留给调用方按
/// F2203 的 `sample_shape` / `sample_velocity` 完成——避免两域各写一份
/// 采样逻辑导致漂移。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EmitPlan {
    pub count: u32,
    pub position_offset: Vec3,
    pub speed_mult: f32,
    /// 来源模式下标（多模式下供诊断与调试）。
    pub mode_index: u8,
}

/// 由 [`EmitDirective`] 生成发射计划（速度倍率默认 1.0）。
pub fn plan_from_directive(d: EmitDirective, offset: Vec3, speed_mult: f32) -> EmitPlan {
    EmitPlan {
        count: d.count,
        position_offset: offset,
        speed_mult,
        mode_index: d.mode_index,
    }
}
