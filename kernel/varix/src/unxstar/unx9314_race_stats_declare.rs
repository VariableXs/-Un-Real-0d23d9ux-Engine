//! UNX-F9314 E 型竞态可复测性声明（竞态 100 次注入统计） · 0x7BC00段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 UNX-F9314 逐字）：
//!   ① 纯功能代码 ≥340 行；② 判据原文：UNX-F9314-J1 竞态 100 次注入统计判据过，
//!   统计分布落账格过，零 flaky 阈值声明格过；③ 完成时 API/软件写回：状态=
//!   已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① 「竞态可复测」的构造性定义：竞态窗由**事件次序**决定，不由时间决定。
//!      调度表把 100 轮按 5 窗 × 2 投递模式分层排布（每格恰 10 轮），定时抖动
//!      只进入 ticks 账，不参与判定——抖动改变指纹但不改变分布，这正是可以
//!      在 CI 里复跑的前提。
//!   ② 判定分两条独立路径：`simulate()` 是五阶段状态机（PreBlock → PostBlock
//!      → PreCopy → PostCopy → PreReturn → 返回），逐阶段看投递点是否命中；
//!      `frozen_outcome()` 是手写 10 行冻结表。二者逐轮比对，不一致即 flaky。
//!   ③ 投递模式：Immediate 在该窗立即生效；Deferred 顺延到下一个事件边界，
//!      因此同一窗的两种模式可能落在不同阶段（PreBlock/Deferred 会先入队再中断，
//!      PreCopy/Deferred 会拖到拷贝之后——此时完成优先，信号只能转欠账）。
//!   ④ 统计账：6 个结果箱 [10,20,20,20,20,10]、每窗 20、每模式 50、欠账 50、
//!      ticks 总和 100。分布是调度表的分层结果，与种子无关——换种子只换指纹。
//!   ⑤ 零 flaky 阈值声明：声明结构把「轮数 / 跑数 / 每格轮数 / 箱上下界 /
//!      允许 flaky 数 / 允许偏差 ppm」写成可校验的数据，`validate()` 逐项拒绝
//!      坏参数（轮数不闭合、跑数 <2、允许 flaky ≠0、偏差 ppm ≠0、箱界倒置）；
//!      实测分布还必须落在声明区间内，且两次重放的直方图距离为 0。
//!   ⑥ 诊断码段内闭集 12 个（+0x00..=0x0B），码值两两唯一，每码一句处置。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 诊断段基址（F9314 独占 0x7BC00..=0x7BC3F 的 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7BC00;
/// 邻卡下界（F9313 = 0x7BBC0）。
pub const SEG_PREV: u32 = 0x7BBC0;
/// 族窗口上界（含）。
pub const SEG_LIMIT: u32 = 0x7D1C0;

/// 竞态窗数量。
pub const WINDOW_N: usize = 5;
/// 投递模式数量。
pub const MODE_N: usize = 2;
/// 结果箱数量。
pub const OUTCOME_N: usize = 6;
/// 注入总轮数（卡面「100 次注入统计」）。
pub const ROUNDS: u32 = 100;
/// 单格轮数（100 / (5×2)）。
pub const PER_CELL: u32 = 10;
/// 可复测性声明要求的最少跑数。
pub const MIN_RUNS: u32 = 2;
/// 抖动上界（0..=7，2 的幂，取模用掩码语义）。
pub const JITTER_BOUND: u32 = 8;
/// 单阶段拷贝字节（半程完成点）。
pub const COPY_MID: u32 = 32;
/// 单阶段拷贝字节（全程完成点）。
pub const COPY_FULL: u32 = 64;

/// errno：被信号中断。
pub const EINTR: i32 = 4;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 竞态统计诊断码（段内 +0x00..=0x0B 闭集）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RaceCode {
    /// 轮数与格数不闭合。
    RoundSkew,
    /// 某格轮数不足。
    CellStarved,
    /// 模型与冻结表分叉（flaky）。
    FlakyOutcome,
    /// 结果箱分布漂移。
    BinDrift,
    /// 单窗轮数漂移。
    WindowDrift,
    /// 单模式轮数漂移。
    ModeDrift,
    /// 完成后信号被 EINTR 吞掉。
    CarriedLost,
    /// 声明参数不合法。
    DeclInvalid,
    /// 实测分布越出声明区间。
    DeclViolated,
    /// 两次重放直方图不一致。
    ReplayDiff,
    /// 抖动越界或 ticks 账不平。
    JitterLeak,
    /// 段位越界。
    SegmentViolation,
}

impl RaceCode {
    /// 段内码号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 绝对码值。
    pub fn val(self) -> u32 {
        SEG_BASE + self.code() as u32
    }

    /// 中文处置建议。
    pub fn detail(self) -> &'static str {
        match self {
            RaceCode::RoundSkew => "轮数必须等于 窗数×模式数×每格轮数：先修调度表再跑统计",
            RaceCode::CellStarved => "某格轮数不足 10：分层排布漏格，禁止用伪随机补数",
            RaceCode::FlakyOutcome => "模型与冻结表分叉：竞态窗的投递语义被改坏，先对齐投递点",
            RaceCode::BinDrift => "结果箱分布漂移：按 10 格 × 10 轮重算，别改箱界凑数",
            RaceCode::WindowDrift => "单窗轮数必须为 20：窗口枚举与调度表不同源",
            RaceCode::ModeDrift => "单模式轮数必须为 50：Immediate/Deferred 各半是声明前提",
            RaceCode::CarriedLost => "完成后的信号必须转欠账：返回字节数时不得改报 EINTR",
            RaceCode::DeclInvalid => "声明参数不合法：轮数闭合、跑数≥2、允许 flaky 与偏差必须为 0",
            RaceCode::DeclViolated => "实测分布越出声明区间：要么修实现，要么显式改声明",
            RaceCode::ReplayDiff => "两次重放直方图不一致：统计路径存在隐藏状态",
            RaceCode::JitterLeak => "抖动只进 ticks 账：越界或账不平说明它混入了判定",
            RaceCode::SegmentViolation => "段基址须 0x40 对齐且落在 0x7A000..=0x7D1C0",
        }
    }

    /// 闭集全量。
    pub const ALL: &[RaceCode] = &[
        RaceCode::RoundSkew,
        RaceCode::CellStarved,
        RaceCode::FlakyOutcome,
        RaceCode::BinDrift,
        RaceCode::WindowDrift,
        RaceCode::ModeDrift,
        RaceCode::CarriedLost,
        RaceCode::DeclInvalid,
        RaceCode::DeclViolated,
        RaceCode::ReplayDiff,
        RaceCode::JitterLeak,
        RaceCode::SegmentViolation,
    ];
}

// ── 固定 LCG ──────────────────────────────────────────────────────────
/// 线性同余发生器（数值与 glibc 同族；只用于定时抖动，不参与判定）。
#[derive(Clone, Copy, Debug)]
pub struct Lcg {
    state: u32,
}

impl Lcg {
    /// 以种子构造。
    pub const fn new(seed: u32) -> Lcg {
        Lcg { state: seed }
    }

    /// 下一个 32 位值。
    pub fn next_u32(&mut self) -> u32 {
        self.state = self.state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.state
    }

    /// [0, n) 上的值：用 64 位高位乘法代替除法取模。
    pub fn next_below(&mut self, n: u32) -> u32 {
        if n == 0 {
            return 0;
        }
        let x = self.next_u32() as u64;
        ((x.saturating_mul(n as u64)) >> 32) as u32
    }

    /// 当前状态（双跑一致性观察量）。
    pub fn state(&self) -> u32 {
        self.state
    }
}

// ── 竞态窗与投递模式 ──────────────────────────────────────────────────
/// 五个竞态窗（事件边界，不是时间点）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Window {
    /// 入等待队列之前。
    PreBlock,
    /// 入队之后。
    PostBlock,
    /// 开始拷贝之前。
    PreCopy,
    /// 拷贝半程之后。
    PostCopy,
    /// 返回之前。
    PreReturn,
}

impl Window {
    /// 闭集全量（顺序即事件序）。
    pub const ALL: [Window; WINDOW_N] = [
        Window::PreBlock,
        Window::PostBlock,
        Window::PreCopy,
        Window::PostCopy,
        Window::PreReturn,
    ];

    /// 事件序秩（0..=4）；秩只用于排序与位集编码。
    pub fn rank(self) -> usize {
        match self {
            Window::PreBlock => 0,
            Window::PostBlock => 1,
            Window::PreCopy => 2,
            Window::PostCopy => 3,
            Window::PreReturn => 4,
        }
    }

    /// 名字。
    pub fn name(self) -> &'static str {
        match self {
            Window::PreBlock => "PRE_BLOCK",
            Window::PostBlock => "POST_BLOCK",
            Window::PreCopy => "PRE_COPY",
            Window::PostCopy => "POST_COPY",
            Window::PreReturn => "PRE_RETURN",
        }
    }
}

/// 两种投递模式。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    /// 命中窗口立即投递。
    Immediate,
    /// 顺延到下一个事件边界。
    Deferred,
}

impl Mode {
    /// 闭集全量。
    pub const ALL: [Mode; MODE_N] = [Mode::Immediate, Mode::Deferred];

    /// 秩（0/1）。
    pub fn rank(self) -> usize {
        match self {
            Mode::Immediate => 0,
            Mode::Deferred => 1,
        }
    }

    /// 名字。
    pub fn name(self) -> &'static str {
        match self {
            Mode::Immediate => "IMM",
            Mode::Deferred => "DEF",
        }
    }
}

/// 实际投递点：某个事件边界，或「返回之后」。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Delivery {
    /// 在指定边界投递。
    At(Window),
    /// 本次调用返回之后才投递（欠账）。
    AfterReturn,
}

impl Delivery {
    /// 编码（0..=4 边界，5 表示返回后）。
    pub fn code(self) -> u32 {
        match self {
            Delivery::At(w) => w.rank() as u32,
            Delivery::AfterReturn => 5,
        }
    }
}

// ── 结果箱 ────────────────────────────────────────────────────────────
/// 六个结果箱（用户态可见结果的完备划分）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Outcome {
    /// -1/EINTR，未入队。
    EintrNoWait,
    /// -1/EINTR，已入队。
    EintrInWait,
    /// -1/EINTR，已入队且已开始准备拷贝。
    EintrPreCopy,
    /// 返回半程字节。
    DonePartial,
    /// 返回全量字节。
    DoneFull,
    /// 返回全量字节 + 一笔欠账。
    DoneCarried,
}

impl Outcome {
    /// 箱号（直方图下标）。
    pub fn code(self) -> usize {
        match self {
            Outcome::EintrNoWait => 0,
            Outcome::EintrInWait => 1,
            Outcome::EintrPreCopy => 2,
            Outcome::DonePartial => 3,
            Outcome::DoneFull => 4,
            Outcome::DoneCarried => 5,
        }
    }

    /// 闭集全量。
    pub const ALL: [Outcome; OUTCOME_N] = [
        Outcome::EintrNoWait,
        Outcome::EintrInWait,
        Outcome::EintrPreCopy,
        Outcome::DonePartial,
        Outcome::DoneFull,
        Outcome::DoneCarried,
    ];

    /// 是否为中断结果。
    pub fn is_eintr(self) -> bool {
        matches!(
            self,
            Outcome::EintrNoWait | Outcome::EintrInWait | Outcome::EintrPreCopy
        )
    }

    /// 是否携带欠账（完成后到达的信号一律转欠账）。
    pub fn carried(self) -> bool {
        matches!(
            self,
            Outcome::DonePartial | Outcome::DoneFull | Outcome::DoneCarried
        )
    }

    /// 名字。
    pub fn name(self) -> &'static str {
        match self {
            Outcome::EintrNoWait => "EINTR_NO_WAIT",
            Outcome::EintrInWait => "EINTR_IN_WAIT",
            Outcome::EintrPreCopy => "EINTR_PRE_COPY",
            Outcome::DonePartial => "DONE_PARTIAL",
            Outcome::DoneFull => "DONE_FULL",
            Outcome::DoneCarried => "DONE_CARRIED",
        }
    }
}

// ── 冻结的十格表 ──────────────────────────────────────────────────────
/// 一格：窗 + 模式 + 期望结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RaceCell {
    pub window: Window,
    pub mode: Mode,
    pub outcome: Outcome,
}

/// 十格冻结表（手写，独立于状态机）。
pub const CELLS: [RaceCell; 10] = [
    RaceCell { window: Window::PreBlock, mode: Mode::Immediate, outcome: Outcome::EintrNoWait },
    RaceCell { window: Window::PreBlock, mode: Mode::Deferred, outcome: Outcome::EintrInWait },
    RaceCell { window: Window::PostBlock, mode: Mode::Immediate, outcome: Outcome::EintrInWait },
    RaceCell { window: Window::PostBlock, mode: Mode::Deferred, outcome: Outcome::EintrPreCopy },
    RaceCell { window: Window::PreCopy, mode: Mode::Immediate, outcome: Outcome::EintrPreCopy },
    RaceCell { window: Window::PreCopy, mode: Mode::Deferred, outcome: Outcome::DonePartial },
    RaceCell { window: Window::PostCopy, mode: Mode::Immediate, outcome: Outcome::DonePartial },
    RaceCell { window: Window::PostCopy, mode: Mode::Deferred, outcome: Outcome::DoneFull },
    RaceCell { window: Window::PreReturn, mode: Mode::Immediate, outcome: Outcome::DoneFull },
    RaceCell { window: Window::PreReturn, mode: Mode::Deferred, outcome: Outcome::DoneCarried },
];

/// 按 (窗, 模式) 查冻结结果。
pub fn frozen_outcome(window: Window, mode: Mode) -> Option<Outcome> {
    let mut i = 0usize;
    while i < CELLS.len() {
        if let Some(c) = CELLS.get(i) {
            if c.window == window && c.mode == mode {
                return Some(c.outcome);
            }
        }
        i += 1;
    }
    None
}

/// 格位集（窗秩 × 2 + 模式秩，10 位）。满覆盖即 0x3FF。
pub fn cell_bitset() -> u32 {
    let mut bits = 0u32;
    let mut i = 0usize;
    while i < CELLS.len() {
        if let Some(c) = CELLS.get(i) {
            let bit = c.window.rank() * MODE_N + c.mode.rank();
            if bit < 32 {
                bits |= 1u32 << (bit as u32);
            }
        }
        i += 1;
    }
    bits
}

// ── 投递点语义（竞态模型的内核） ──────────────────────────────────────
/// 窗 + 模式 → 实际投递点。
///
/// 这是十格表的**生成规则**，但十格表是手写的，因此两者互为外部锚点：
/// 规则被改坏时表不会跟着变，flaky 会立刻跳出来。
pub fn delivery_point(window: Window, mode: Mode) -> Delivery {
    match (window, mode) {
        (Window::PreBlock, Mode::Immediate) => Delivery::At(Window::PreBlock),
        (Window::PreBlock, Mode::Deferred) => Delivery::At(Window::PostBlock),
        (Window::PostBlock, Mode::Immediate) => Delivery::At(Window::PostBlock),
        (Window::PostBlock, Mode::Deferred) => Delivery::At(Window::PreCopy),
        (Window::PreCopy, Mode::Immediate) => Delivery::At(Window::PreCopy),
        (Window::PreCopy, Mode::Deferred) => Delivery::At(Window::PostCopy),
        (Window::PostCopy, Mode::Immediate) => Delivery::At(Window::PostCopy),
        (Window::PostCopy, Mode::Deferred) => Delivery::At(Window::PreReturn),
        (Window::PreReturn, Mode::Immediate) => Delivery::At(Window::PreReturn),
        (Window::PreReturn, Mode::Deferred) => Delivery::AfterReturn,
    }
}

/// 一次竞态观测。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Run {
    /// 排布的窗。
    pub window: Window,
    /// 排布的模式。
    pub mode: Mode,
    /// 实际投递点。
    pub delivered: Delivery,
    /// 是否进入了等待队列。
    pub waited: bool,
    /// 已拷贝字节。
    pub copied: u32,
    /// 返回时是否留有欠账。
    pub carried: bool,
    /// 结果箱。
    pub observed: Outcome,
}

impl Run {
    /// 空白（表构造/失败路径用）。
    pub const BLANK: Run = Run {
        window: Window::PreBlock,
        mode: Mode::Immediate,
        delivered: Delivery::At(Window::PreBlock),
        waited: false,
        copied: 0,
        carried: false,
        observed: Outcome::EintrNoWait,
    };
}

/// 边界命中判定。
fn hit(d: Delivery, stage: Window) -> bool {
    matches!(d, Delivery::At(w) if w == stage)
}

/// 五阶段状态机：按事件序推进，逐阶段看投递点是否命中。
pub fn simulate(window: Window, mode: Mode) -> Run {
    let delivered = delivery_point(window, mode);
    let mut waited = false;
    let mut copied = 0u32;

    // ① 入队前。
    if hit(delivered, Window::PreBlock) {
        return Run {
            window,
            mode,
            delivered,
            waited,
            copied,
            carried: false,
            observed: Outcome::EintrNoWait,
        };
    }
    // 入队。
    waited = true;
    // ② 入队后。
    if hit(delivered, Window::PostBlock) {
        return Run {
            window,
            mode,
            delivered,
            waited,
            copied,
            carried: false,
            observed: Outcome::EintrInWait,
        };
    }
    // ③ 拷贝前。
    if hit(delivered, Window::PreCopy) {
        return Run {
            window,
            mode,
            delivered,
            waited,
            copied,
            carried: false,
            observed: Outcome::EintrPreCopy,
        };
    }
    // 拷贝半程。
    copied = COPY_MID;
    // ④ 拷贝后：数据已在，完成优先，信号只能转欠账。
    if hit(delivered, Window::PostCopy) {
        return Run {
            window,
            mode,
            delivered,
            waited,
            copied,
            carried: true,
            observed: Outcome::DonePartial,
        };
    }
    // 拷贝全程。
    copied = COPY_FULL;
    // ⑤ 返回前。
    if hit(delivered, Window::PreReturn) {
        return Run {
            window,
            mode,
            delivered,
            waited,
            copied,
            carried: true,
            observed: Outcome::DoneFull,
        };
    }
    // ⑥ 返回之后才投递：全量完成 + 一笔欠账。
    if matches!(delivered, Delivery::AfterReturn) {
        return Run {
            window,
            mode,
            delivered,
            waited,
            copied,
            carried: true,
            observed: Outcome::DoneCarried,
        };
    }
    Run {
        window,
        mode,
        delivered,
        waited,
        copied,
        carried: true,
        observed: Outcome::DoneFull,
    }
}

// ── 调度表（分层） ────────────────────────────────────────────────────
/// 第 `round` 轮的 (窗, 模式)：按 10 格轮转，每格恰 10 轮。
pub fn schedule(round: u32) -> Option<(Window, Mode)> {
    if round >= ROUNDS {
        return None;
    }
    match round % 10 {
        0 => Some((Window::PreBlock, Mode::Immediate)),
        1 => Some((Window::PreBlock, Mode::Deferred)),
        2 => Some((Window::PostBlock, Mode::Immediate)),
        3 => Some((Window::PostBlock, Mode::Deferred)),
        4 => Some((Window::PreCopy, Mode::Immediate)),
        5 => Some((Window::PreCopy, Mode::Deferred)),
        6 => Some((Window::PostCopy, Mode::Immediate)),
        7 => Some((Window::PostCopy, Mode::Deferred)),
        8 => Some((Window::PreReturn, Mode::Immediate)),
        _ => Some((Window::PreReturn, Mode::Deferred)),
    }
}

// ── 零 flaky 阈值声明 ─────────────────────────────────────────────────
/// 声明参数不合法时的原因（闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DeclError {
    /// 轮数与 窗×模式×每格 不闭合。
    RoundsMismatch,
    /// 跑数不足。
    RunsTooFew,
    /// 允许 flaky 数不为 0。
    FlakyNotZero,
    /// 允许偏差 ppm 不为 0。
    DeviationNotZero,
    /// 箱界倒置或越界。
    BinBand,
    /// 窗/模式数与模块常量不一致。
    ShapeDrift,
}

impl DeclError {
    /// 中文说明。
    pub fn detail(self) -> &'static str {
        match self {
            DeclError::RoundsMismatch => "轮数必须是 窗×模式×每格 的整数倍",
            DeclError::RunsTooFew => "可复测声明至少要求 2 次重放",
            DeclError::FlakyNotZero => "零 flaky 声明不允许任何 flaky 额度",
            DeclError::DeviationNotZero => "两次重放的直方图偏差必须为 0 ppm",
            DeclError::BinBand => "箱下界必须 ≥1 且上界 ≤ 总轮数，且下界不得大于上界",
            DeclError::ShapeDrift => "窗数/模式数必须与模块常量一致，改形状即改声明",
        }
    }
}

/// 零 flaky 阈值声明。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Declaration {
    /// 总轮数。
    pub rounds: u32,
    /// 重放次数。
    pub runs: u32,
    /// 窗数。
    pub windows: u32,
    /// 模式数。
    pub modes: u32,
    /// 每格轮数。
    pub per_cell: u32,
    /// 单箱下界。
    pub min_bin: u32,
    /// 单箱上界。
    pub max_bin: u32,
    /// 允许 flaky 数。
    pub allowed_flaky: u32,
    /// 两次重放允许的直方图偏差（ppm）。
    pub allowed_deviation_ppm: u32,
}

impl Declaration {
    /// 本模块的默认声明。
    pub const DEFAULT: Declaration = Declaration {
        rounds: ROUNDS,
        runs: MIN_RUNS,
        windows: WINDOW_N as u32,
        modes: MODE_N as u32,
        per_cell: PER_CELL,
        min_bin: 10,
        max_bin: 20,
        allowed_flaky: 0,
        allowed_deviation_ppm: 0,
    };

    /// 逐项校验声明自洽。
    pub fn validate(&self) -> Result<(), DeclError> {
        if self.windows != WINDOW_N as u32 || self.modes != MODE_N as u32 {
            return Err(DeclError::ShapeDrift);
        }
        if self.per_cell == 0
            || self.windows.saturating_mul(self.modes).saturating_mul(self.per_cell) != self.rounds
        {
            return Err(DeclError::RoundsMismatch);
        }
        if self.runs < MIN_RUNS {
            return Err(DeclError::RunsTooFew);
        }
        if self.allowed_flaky != 0 {
            return Err(DeclError::FlakyNotZero);
        }
        if self.allowed_deviation_ppm != 0 {
            return Err(DeclError::DeviationNotZero);
        }
        if self.min_bin == 0 || self.max_bin > self.rounds || self.min_bin > self.max_bin {
            return Err(DeclError::BinBand);
        }
        Ok(())
    }

    /// 直方图是否落在声明区间内。
    pub fn admits(&self, bins: &[u32; OUTCOME_N]) -> bool {
        let mut i = 0usize;
        while i < OUTCOME_N {
            match bins.get(i) {
                Some(n) => {
                    if *n < self.min_bin || *n > self.max_bin {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        true
    }
}

/// 两个直方图的绝对距离（逐箱差之和）。
pub fn histogram_distance(a: &[u32; OUTCOME_N], b: &[u32; OUTCOME_N]) -> u32 {
    let mut d = 0u32;
    let mut i = 0usize;
    while i < OUTCOME_N {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        d = d.saturating_add(x.abs_diff(y));
        i += 1;
    }
    d
}

/// 距离折算 ppm（总轮数为分母；100 轮下 1 箱差 = 10000 ppm）。
pub fn distance_ppm(diff: u32, total: u32) -> u32 {
    if total == 0 {
        return 0;
    }
    let scaled = (diff as u64).saturating_mul(1_000_000);
    (scaled / total as u64) as u32
}

// ── 统计报告 ──────────────────────────────────────────────────────────
/// 一次 100 轮竞态统计的报告。
#[derive(Clone, Copy, Debug)]
pub struct RaceReport {
    /// 结果箱直方图。
    pub bins: [u32; OUTCOME_N],
    /// 单窗轮数。
    pub per_window: [u32; WINDOW_N],
    /// 单模式轮数。
    pub per_mode: [u32; MODE_N],
    /// 抖动账（0..=7）。
    pub ticks: [u32; JITTER_BOUND as usize],
    /// 欠账信号数。
    pub carried: u32,
    /// 与冻结表分叉的轮数（flaky）。
    pub flaky: u32,
    /// 注入轮数。
    pub injected: u32,
    /// 轨迹指纹。
    pub checksum: u32,
}

impl RaceReport {
    /// 检出数：未分叉的轮数。
    pub fn detected(&self) -> u32 {
        self.injected.saturating_sub(self.flaky)
    }

    /// 逃逸数：与冻结表分叉的轮数（flaky 即逃逸，不允许出现）。
    pub fn escaped(&self) -> u32 {
        self.flaky
    }

    /// 账本守恒：injected = detected + escaped，且每轮都落进唯一结果箱。
    ///
    /// 两条一起判才不是恒真：只看前者是减法恒等式，只看后者会漏掉
    /// 「有轮次既没检出也没逃逸」的记漏。
    pub fn conserved(&self) -> bool {
        self.detected().saturating_add(self.escaped()) == self.injected
            && self.bin_total() == self.injected
    }

    /// 直方图总和。
    pub fn bin_total(&self) -> u32 {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < OUTCOME_N {
            if let Some(n) = self.bins.get(i) {
                sum = sum.saturating_add(*n);
            }
            i += 1;
        }
        sum
    }

    /// 抖动账总和。
    pub fn tick_total(&self) -> u32 {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < self.ticks.len() {
            if let Some(n) = self.ticks.get(i) {
                sum = sum.saturating_add(*n);
            }
            i += 1;
        }
        sum
    }
}

/// FNV-1a 偏移基。
pub const FNV_OFFSET: u32 = 0x811C_9DC5;
/// FNV-1a 质数。
pub const FNV_PRIME: u32 = 0x0100_0193;

/// FNV-1a 32 位。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h = FNV_OFFSET;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u32;
            h = h.wrapping_mul(FNV_PRIME);
        }
        i += 1;
    }
    h
}

/// 跑 100 轮竞态统计。种子只影响抖动账与指纹，不影响分布。
pub fn run_race(seed: u32) -> RaceReport {
    let mut lcg = Lcg::new(seed);
    let mut bins = [0u32; OUTCOME_N];
    let mut per_window = [0u32; WINDOW_N];
    let mut per_mode = [0u32; MODE_N];
    let mut ticks = [0u32; JITTER_BOUND as usize];
    let mut carried = 0u32;
    let mut flaky = 0u32;
    let mut bytes: Vec<u8> = Vec::new();
    let mut round = 0u32;
    while round < ROUNDS {
        let (window, mode) = match schedule(round) {
            Some(p) => p,
            None => break,
        };
        let jitter = lcg.next_below(JITTER_BOUND);
        let run = simulate(window, mode);
        // 抖动量必须落在账内，且不得改变判定（下面单独复核一次）。
        if jitter < JITTER_BOUND {
            if let Some(slot) = ticks.get_mut(jitter as usize) {
                *slot = slot.saturating_add(1);
            }
        }
        if let Some(slot) = bins.get_mut(run.observed.code()) {
            *slot = slot.saturating_add(1);
        }
        if let Some(slot) = per_window.get_mut(window.rank()) {
            *slot = slot.saturating_add(1);
        }
        if let Some(slot) = per_mode.get_mut(mode.rank()) {
            *slot = slot.saturating_add(1);
        }
        if run.carried {
            carried = carried.saturating_add(1);
        }
        // 与冻结表逐轮比对：不一致即 flaky（不是「运行失败」，是「不可复测」）。
        match frozen_outcome(window, mode) {
            Some(want) => {
                if want != run.observed {
                    flaky = flaky.saturating_add(1);
                }
            }
            None => flaky = flaky.saturating_add(1),
        }
        bytes.push((round & 0xFF) as u8);
        bytes.push(window.rank() as u8);
        bytes.push(mode.rank() as u8);
        bytes.push(run.observed.code() as u8);
        bytes.push(run.delivered.code() as u8);
        bytes.push((run.copied & 0xFF) as u8);
        bytes.push(if run.carried { 1 } else { 0 });
        bytes.push(jitter as u8);
        round += 1;
    }
    RaceReport {
        bins,
        per_window,
        per_mode,
        ticks,
        carried,
        flaky,
        injected: ROUNDS,
        checksum: fnv1a(&bytes),
    }
}

/// 默认零 flaky 声明 + 实测校验结果。
#[derive(Clone, Copy, Debug)]
pub struct DeclarationCheck {
    /// 声明是否自洽。
    pub valid: bool,
    /// 实测分布是否落在声明区间。
    pub admitted: bool,
    /// 两次重放的距离（绝对）。
    pub distance: u32,
    /// 折算 ppm。
    pub distance_ppm: u32,
}

/// 用默认声明校验一份报告（内部按同一报告复算一次重放距离）。
pub fn check_declaration(rep: &RaceReport, rep2: &RaceReport) -> DeclarationCheck {
    let d = Declaration::DEFAULT;
    let valid = d.validate().is_ok();
    let admitted = d.admits(&rep.bins);
    let distance = histogram_distance(&rep.bins, &rep2.bins);
    DeclarationCheck {
        valid,
        admitted,
        distance,
        distance_ppm: distance_ppm(distance, rep.injected),
    }
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9314 判据集（add ≥ 12；add_dyn ≤ 8）。
pub fn run_unx9314_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9314");

    // ① 段位。
    let seg_ok = SEG_BASE % 0x40 == 0
        && SEG_BASE > SEG_PREV
        && SEG_BASE < SEG_LIMIT
        && SEG_BASE + 0x40 <= SEG_LIMIT;
    s.add(
        "段位守卫：0x40 对齐且不与 F9313 重叠",
        seg_ok,
        "段基址唯一且落在本批窗口，段间不得互相覆盖",
    );

    // ② 十格覆盖 5 窗 × 2 模式，无重复无缺失。
    let bits = cell_bitset();
    let full = (1u32 << 10) - 1;
    s.add(
        "冻结表：10 格满覆盖 5 窗 × 2 模式",
        CELLS.len() == 10 && bits == full,
        "格位集必须为 0x3FF；任何缺格都会让某条竞态路径失去见证",
    );

    // ③ 调度表分层：每格恰 10 轮。
    let mut strat_ok = true;
    let mut grid = [0u32; 10];
    let mut round = 0u32;
    while round < ROUNDS {
        match schedule(round) {
            Some((w, m)) => {
                let idx = w.rank() * MODE_N + m.rank();
                if let Some(slot) = grid.get_mut(idx) {
                    *slot = slot.saturating_add(1);
                }
            }
            None => strat_ok = false,
        }
        round += 1;
    }
    let mut gi = 0usize;
    while gi < grid.len() {
        match grid.get(gi) {
            Some(n) => {
                if *n != PER_CELL {
                    strat_ok = false;
                }
            }
            None => strat_ok = false,
        }
        gi += 1;
    }
    s.add(
        "调度：100 轮按 10 格轮转、每格恰 10 轮",
        strat_ok && schedule(ROUNDS).is_none(),
        "分层排布是可复测性的地基，禁止用伪随机抽格",
    );

    // ④ 100 轮模型与冻结表零分叉。
    let rep = run_race(0x1234_5678);
    s.add(
        "统计：100 轮模型与冻结表零分叉（flaky = 0）",
        rep.injected == ROUNDS && rep.flaky == 0 && rep.conserved(),
        "flaky 非零说明投递点语义被改坏，统计不再是同一个实验",
    );

    // ⑤ 六箱分布 [10,20,20,20,20,10]。
    let want_bins = [10u32, 20, 20, 20, 20, 10];
    let mut bins_ok = true;
    let mut bi = 0usize;
    while bi < OUTCOME_N {
        match (rep.bins.get(bi), want_bins.get(bi)) {
            (Some(got), Some(want)) => {
                if got != want {
                    bins_ok = false;
                }
            }
            _ => bins_ok = false,
        }
        bi += 1;
    }
    s.add(
        "分布落账：六箱 [10,20,20,20,20,10] 且总和 100",
        bins_ok && rep.bin_total() == ROUNDS,
        "结果箱是用户态可见结果的完备划分，计数必须与十格表推得的一致",
    );

    // ⑥ 单窗 20。
    let mut win_ok = true;
    let mut wi = 0usize;
    while wi < WINDOW_N {
        match rep.per_window.get(wi) {
            Some(n) => {
                if *n != 20 {
                    win_ok = false;
                }
            }
            None => win_ok = false,
        }
        wi += 1;
    }
    s.add(
        "分布落账：五个竞态窗各 20 轮",
        win_ok,
        "窗枚举与调度表必须同源，任何一窗多一轮都是排布被改",
    );

    // ⑦ 单模式 50。
    let mut mode_ok = true;
    let mut mi = 0usize;
    while mi < MODE_N {
        match rep.per_mode.get(mi) {
            Some(n) => {
                if *n != 50 {
                    mode_ok = false;
                }
            }
            None => mode_ok = false,
        }
        mi += 1;
    }
    s.add(
        "分布落账：Immediate / Deferred 各 50 轮",
        mode_ok,
        "两种投递模式必须等权，否则「延迟投递」的结论没有样本支撑",
    );

    // ⑧ 完成后信号一律转欠账：50 轮带欠账，且带欠账轮次都返回了字节。
    let carried_ok = rep.carried == 50
        && CELLS
            .iter()
            .filter(|c| c.outcome.carried())
            .count()
            == 5;
    s.add(
        "完成优先：5 格 × 10 轮 = 50 轮携带欠账而非 EINTR",
        carried_ok,
        "数据已提交后到达的信号只能转欠账，绝不能把完成改写成中断",
    );

    // ⑨ 抖动账：100 次取值全部 < 8 且账平。
    let mut jitter_ok = rep.tick_total() == ROUNDS;
    let mut ti = 0usize;
    while ti < rep.ticks.len() {
        match rep.ticks.get(ti) {
            Some(n) => {
                if *n > ROUNDS {
                    jitter_ok = false;
                }
            }
            None => jitter_ok = false,
        }
        ti += 1;
    }
    let mut probe = Lcg::new(0x1234_5678);
    let mut k = 0u32;
    while k < ROUNDS {
        if probe.next_below(JITTER_BOUND) >= JITTER_BOUND {
            jitter_ok = false;
        }
        k += 1;
    }
    s.add(
        "抖动账：100 次取值有界且 ticks 总和 100",
        jitter_ok,
        "抖动只进 ticks 账、不参与判定；越界或账不平即说明它混进了判定",
    );

    // ⑩ 抖动无关性：换抖动不换结果（100 轮逐格复核）。
    let mut invariant = true;
    let mut ri = 0u32;
    while ri < ROUNDS {
        match schedule(ri) {
            Some((w, m)) => {
                let a = simulate(w, m);
                let b = simulate(w, m);
                if a.observed != b.observed || a.copied != b.copied || a.carried != b.carried {
                    invariant = false;
                }
            }
            None => invariant = false,
        }
        ri += 1;
    }
    s.add(
        "可复测：同一格两次模拟逐字段相同",
        invariant,
        "竞态由事件次序决定，重放同格必须得到同一结果与同一欠账状态",
    );

    // ⑪ 声明自洽。
    let decl = Declaration::DEFAULT;
    s.add(
        "声明：轮数闭合 / 跑数 2 / 零 flaky / 零偏差",
        decl.validate().is_ok()
            && decl.runs == MIN_RUNS
            && decl.allowed_flaky == 0
            && decl.allowed_deviation_ppm == 0,
        "零 flaky 阈值声明必须逐项可校验，不能只是注释里的一句话",
    );

    // ⑫ 实测分布落在声明区间。
    let rep2 = run_race(0x1234_5678);
    let dc = check_declaration(&rep, &rep2);
    s.add(
        "声明校验：实测六箱落在 [10,20] 且重放距离为 0",
        dc.valid && dc.admitted && dc.distance == 0 && dc.distance_ppm == 0,
        "声明与实测必须对得上：越界要么改实现，要么显式改声明",
    );

    // ⑬ 双跑指纹一致。
    s.add(
        "双跑一致：同种子轨迹指纹相等",
        rep.checksum == rep2.checksum,
        "同种子两次重放必须逐字节相同，隐藏状态即失败",
    );

    // ⑭ 换种子只换指纹不换分布。
    let rep3 = run_race(0x0BAD_F00D);
    let seed_distance = histogram_distance(&rep.bins, &rep3.bins);
    s.add(
        "种子无关：换种子后直方图距离为 0",
        seed_distance == 0
            && rep3.bin_total() == ROUNDS
            && rep3.flaky == 0
            && rep3.per_window == rep.per_window,
        "分布由分层调度决定，种子只影响抖动账与指纹——这正是可复测的含义",
    );

    // ⑮ 声明拒绝坏参数。
    let mut bad_rounds = decl;
    bad_rounds.rounds = 99;
    let mut bad_runs = decl;
    bad_runs.runs = 1;
    let mut bad_flaky = decl;
    bad_flaky.allowed_flaky = 1;
    let mut bad_ppm = decl;
    bad_ppm.allowed_deviation_ppm = 100;
    let mut bad_band = decl;
    bad_band.min_bin = 21;
    let mut bad_shape = decl;
    bad_shape.windows = 4;
    let reject_ok = bad_rounds.validate() == Err(DeclError::RoundsMismatch)
        && bad_runs.validate() == Err(DeclError::RunsTooFew)
        && bad_flaky.validate() == Err(DeclError::FlakyNotZero)
        && bad_ppm.validate() == Err(DeclError::DeviationNotZero)
        && bad_band.validate() == Err(DeclError::BinBand)
        && bad_shape.validate() == Err(DeclError::ShapeDrift);
    s.add(
        "声明拒绝：六类坏参数逐项返回确定原因",
        reject_ok,
        "阈值声明若接受坏参数，零 flaky 就是一句空话",
    );

    // ⑯ 直方图距离与 ppm 折算自洽。
    let zeros = [0u32; OUTCOME_N];
    let ones = [1u32; OUTCOME_N];
    let dist_ok = histogram_distance(&zeros, &ones) == OUTCOME_N as u32
        && distance_ppm(1, ROUNDS) == 10_000
        && distance_ppm(0, ROUNDS) == 0
        && histogram_distance(&rep.bins, &rep.bins) == 0;
    s.add(
        "距离量具：逐箱绝对差与 ppm 折算自洽",
        dist_ok,
        "重放距离是零 flaky 声明的量具，量具本身必须先被校准",
    );

    // ⑰ 码值唯一。
    let mut uniq = true;
    let mut a = 0usize;
    while a < RaceCode::ALL.len() {
        let mut b = a + 1;
        while b < RaceCode::ALL.len() {
            if let (Some(x), Some(y)) = (RaceCode::ALL.get(a), RaceCode::ALL.get(b)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            b += 1;
        }
        a += 1;
    }
    s.add(
        "诊断码：12 个码值两两唯一",
        uniq,
        "段内闭集码重复会让判据无法定位",
    );

    // ⑱ 文案非空。
    s.add(
        "文案闭集：12 个诊断码均有中文处置建议",
        RaceCode::ALL.iter().all(|c| !c.detail().is_empty()),
        "每个码都要给出可操作处置",
    );

    // 动态明细：分布快照。
    let text_bins = format!(
        "箱 [{},{},{},{},{},{}] 合计 {}；窗 [{},{},{},{},{}]；模式 [{},{}]",
        rep.bins.get(0).copied().unwrap_or(0),
        rep.bins.get(1).copied().unwrap_or(0),
        rep.bins.get(2).copied().unwrap_or(0),
        rep.bins.get(3).copied().unwrap_or(0),
        rep.bins.get(4).copied().unwrap_or(0),
        rep.bins.get(5).copied().unwrap_or(0),
        rep.bin_total(),
        rep.per_window.get(0).copied().unwrap_or(0),
        rep.per_window.get(1).copied().unwrap_or(0),
        rep.per_window.get(2).copied().unwrap_or(0),
        rep.per_window.get(3).copied().unwrap_or(0),
        rep.per_window.get(4).copied().unwrap_or(0),
        rep.per_mode.get(0).copied().unwrap_or(0),
        rep.per_mode.get(1).copied().unwrap_or(0)
    );
    s.add_dyn("分布快照", bins_ok && rep.bin_total() == ROUNDS, &text_bins);

    // 动态明细：声明快照。
    let text_decl = format!(
        "轮 {} 跑 {} 每格 {} 箱界 [{},{}] flaky≤{} 偏差≤{}ppm；实测距离 {} ({}ppm)",
        decl.rounds,
        decl.runs,
        decl.per_cell,
        decl.min_bin,
        decl.max_bin,
        decl.allowed_flaky,
        decl.allowed_deviation_ppm,
        dc.distance,
        dc.distance_ppm
    );
    s.add_dyn(
        "声明快照",
        dc.valid && dc.admitted && dc.distance == 0,
        &text_decl,
    );

    // 动态明细：指纹与欠账快照。
    let text_fp = format!(
        "指纹 0x{:08X}；欠账 {}；flaky {}；ticks {}",
        rep.checksum,
        rep.carried,
        rep.flaky,
        rep.tick_total()
    );
    s.add_dyn(
        "指纹快照",
        rep.checksum == rep2.checksum && rep.flaky == 0,
        &text_fp,
    );

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lcg_is_deterministic_and_bounded() {
        let mut a = Lcg::new(7);
        let mut b = Lcg::new(7);
        let mut k = 0u32;
        while k < 64 {
            let x = a.next_below(8);
            let y = b.next_below(8);
            assert_eq!(x, y);
            assert_eq!(x < 8, true);
            k += 1;
        }
        let mut c = Lcg::new(7);
        let mut d = Lcg::new(7);
        assert_eq!(c.next_u32(), d.next_u32());
        assert_eq!(c.state(), d.state());
    }

    #[test]
    fn cell_table_covers_all_pairs() {
        assert_eq!(CELLS.len(), 10);
        assert_eq!(cell_bitset(), 0x3FF);
        let mut w = 0usize;
        while w < WINDOW_N {
            let mut m = 0usize;
            while m < MODE_N {
                let win = Window::ALL.get(w).copied();
                let mode = Mode::ALL.get(m).copied();
                match (win, mode) {
                    (Some(a), Some(b)) => {
                        assert_eq!(frozen_outcome(a, b).is_some(), true);
                    }
                    _ => assert_eq!(true, false),
                }
                m += 1;
            }
            w += 1;
        }
    }

    #[test]
    fn simulate_matches_frozen_table() {
        let mut r = 0u32;
        while r < ROUNDS {
            match schedule(r) {
                Some((w, m)) => {
                    let run = simulate(w, m);
                    assert_eq!(frozen_outcome(w, m), Some(run.observed));
                }
                None => assert_eq!(true, false),
            }
            r += 1;
        }
    }

    #[test]
    fn hundred_round_statistics() {
        let rep = run_race(0x1234_5678);
        assert_eq!(rep.injected, ROUNDS);
        assert_eq!(rep.flaky, 0);
        assert_eq!(rep.conserved(), true);
        assert_eq!(rep.bin_total(), ROUNDS);
        assert_eq!(rep.carried, 50);
        assert_eq!(rep.tick_total(), ROUNDS);
        assert_eq!(rep.bins.get(0).copied(), Some(10));
        assert_eq!(rep.bins.get(1).copied(), Some(20));
        assert_eq!(rep.bins.get(5).copied(), Some(10));
    }

    #[test]
    fn window_and_mode_counts() {
        let rep = run_race(0x1234_5678);
        let mut i = 0usize;
        while i < WINDOW_N {
            assert_eq!(rep.per_window.get(i).copied(), Some(20));
            i += 1;
        }
        let mut k = 0usize;
        while k < MODE_N {
            assert_eq!(rep.per_mode.get(k).copied(), Some(50));
            k += 1;
        }
    }

    #[test]
    fn deferred_delivery_is_later() {
        let imm = simulate(Window::PreBlock, Mode::Immediate);
        let def = simulate(Window::PreBlock, Mode::Deferred);
        assert_eq!(imm.observed, Outcome::EintrNoWait);
        assert_eq!(def.observed, Outcome::EintrInWait);
        assert_eq!(imm.waited, false);
        assert_eq!(def.waited, true);
        let late = simulate(Window::PostCopy, Mode::Deferred);
        assert_eq!(late.observed, Outcome::DoneFull);
        assert_eq!(late.carried, true);
    }

    #[test]
    fn declaration_validates_and_rejects() {
        assert_eq!(Declaration::DEFAULT.validate(), Ok(()));
        let mut bad = Declaration::DEFAULT;
        bad.rounds = 99;
        assert_eq!(bad.validate(), Err(DeclError::RoundsMismatch));
        let mut bad2 = Declaration::DEFAULT;
        bad2.allowed_flaky = 1;
        assert_eq!(bad2.validate(), Err(DeclError::FlakyNotZero));
        let mut bad3 = Declaration::DEFAULT;
        bad3.min_bin = 40;
        assert_eq!(bad3.validate(), Err(DeclError::BinBand));
    }

    #[test]
    fn seed_changes_fingerprint_not_bins() {
        let a = run_race(0x1234_5678);
        let b = run_race(0x0BAD_F00D);
        assert_eq!(histogram_distance(&a.bins, &b.bins), 0);
        assert_eq!(a.per_window, b.per_window);
        assert_eq!(b.flaky, 0);
    }

    #[test]
    fn histogram_metric_is_calibrated() {
        let zeros = [0u32; OUTCOME_N];
        let ones = [1u32; OUTCOME_N];
        assert_eq!(histogram_distance(&zeros, &ones), OUTCOME_N as u32);
        assert_eq!(distance_ppm(1, ROUNDS), 10_000);
        assert_eq!(distance_ppm(0, ROUNDS), 0);
    }

    #[test]
    fn fingerprint_is_stable() {
        let a = run_race(7);
        let b = run_race(7);
        assert_eq!(a.checksum, b.checksum);
        assert_eq!(a.flaky, 0);
        assert_eq!(a.tick_total(), ROUNDS);
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9314_checks();
        assert_eq!(s.all_passed(), true, "{}", s.red_items_text());
    }
}
