//! VE-F1412 · 低延迟路径（VE-H 域 · 音频引擎 · 目标 400 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F1412`
//!
//! **规格原文**：低延迟模式（端到端 ≤20ms 目标——游戏/乐器/实时互动；与播放链
//! 高缓冲路径并存互不拖累：20ms 目标（端到端（输入→处理→输出 ≤20ms（乐器
//! 演奏（20ms 是可演奏阈值（音乐人感知 30ms 为显著延迟——目标有感知依据）、
//! 并存互不拖累（低延迟会话与播放会话共存（播放链的高缓冲不拖累低延迟会话的
//! 调度优先级（MMCSS 优先级分级（F1336 纪律）、小缓冲策略（独占模式+最小
//! 缓冲+零处理直通选项：小缓冲（独占模式（F1327）+最小缓冲（缓冲越小延迟越低
//! 但 underrun 风险升——权衡表（1ms/2ms/5ms 档）、零处理直通（乐器监听（输入
//! 直通输出（零 DSP（纯监听路径）、underrun 特化（小缓冲更敏感——预测性补零+
//! 低延迟专属恢复路径：underrun 特化（小缓冲的 underrun 更频繁（预测性补零
//! （水位预测提前补——低延迟专属恢复（比标准三级恢复（F1327）更快的恢复（牺牲
//! 音质保时延——模式内权衡声明）、延迟实测（真实测量上报——不标称）、切换协议
//! （进入/退出低延迟的安全切换——不爆音）。
//!
//! **工程量构成**（锚点原文）：模式与并存 90 行＋小缓冲策略 90 行＋underrun
//! 特化 90 行＋实测与切换 70 行＝目标 400 行构成。
//!
//! **判据**：≤20ms 目标、并存、恢复特化、实测、切换协议、判据。
//!
//! ---
//!
//! ## 设计要点
//!
//! ### 1. 20ms 目标必须有牙齿：存在必然不达标的组合
//!
//! 若只断"三档缓冲的端到端延迟都 ≤20ms"，则该判据恒真——把
//! [`TARGET_LATENCY_MS`] 改成 1000.0 照样全绿。故本实现配一个**必然不达标的
//! 反例**：块长 10ms（480 帧 @48k）时端到端 = `10 + 0.5 + 10 = 20.5ms > 20ms`。
//! 判据双向断：三档小缓冲达标 **且** 10ms 块不达标。
//!
//! 「20ms 是可演奏阈值」的感知依据也进入代码而非注释：[`TARGET_LATENCY_MS`]
//! 与 [`PLAYABLE_PERCEIVED_MS`] 是两个**独立**常量，判据断前者严格小于后者
//! （单边符号）——把两者调成同一个数即转红。
//!
//! ### 2. 延迟是分解求和，且分解项与总量必须守恒
//!
//! 端到端 = `input_period + process + output_period`（锚点原文的三段）。
//! [`breakdown`] 给出三段分解与总量，总量由**判据侧独立重算的三项之和**核对
//! （`==` 精确等式，非"约等于"）：一个"总量走另一条路算"的实现会在分解自洽时
//! 仍被抓住。
//!
//! 可判定性依赖浮点可精确表示：本模块所有闭式值都落在 `.0 / .5` 上
//! （1.0 / 2.0 / 5.0 / 10.0 周期 + 0.5 处理），二进制精确，故 `==` 是**单边
//! 等式**而非双边阈值。
//!
//! ### 3. 并存互不拖累的可判定形式：提前量与 play 侧零耦合
//!
//! 「播放链的高缓冲不拖累低延迟会话的调度优先级」不能写成一句声明。本实现
//! 把它落成**机械不变式**：低延迟会话的调度提前量
//! [`CoexistScheduler::low_wake_lead_ms`] 只由它自己的 spec 决定；`attach`
//! 播放会话（无论 1 个还是 8 个、缓冲档是 1ms 还是 5ms）**逐位不变**。
//!
//! 反向也钉住：低延迟存在时 tick **先**服务低延迟（`low_served_first`），但
//! 播放会话在其后**仍拿到剩余槽位**（需求 ≤ 槽位时 `play_starved == 0`）——
//! 这就是"并存"与"抢占"的分野。只断"低延迟优先"会把"低延迟把播放挤掉"的实现
//! 放过。
//!
//! ### 4. 诚实边界：不上 Realtime 是刻意的
//!
//! 低延迟会话拿到音频域最高可用档 [`MmcssPriority::Capture`] 而非 `Realtime`。
//! 这不是"没实现"而是**有理由的不做**：`Realtime` 会与系统关键线程（磁盘/网卡
//! ISR）争抢，一次抢占造成的长尾抖动远大于把 rank 从 `Capture` 挪到 `Realtime`
//! 换来的一两毫秒。判据 `H12-并存-不上Realtime边界` 钉住它，使"顺手改成
//! Realtime"这种**看起来像优化**的改动直接转红。
//!
//! ### 5. 权衡表是两条反向单调律，不是一张互异性表
//!
//! 锚点「缓冲越小延迟越低但 underrun 风险升——权衡表」。若只断"三档互不相等"，
//! 则随机常数也能过。故本实现给两条**方向相反**的序关系：周期升序
//! （`1ms < 2ms < 5ms`）、风险**降序**（`1ms 档 > 2ms 档 > 5ms 档`）。一个把风险
//! 也写成随缓冲升序的实现（复制粘贴错误的典型形态）在周期律上绿、在风险律上红。
//!
//! ### 6. 零处理直通的判据断"恰等于两倍周期"
//!
//! 直通不是"少算一点"，是 `process = 0`。判据断
//! `effective_latency(passthrough) == 2 × period`（精确等式）**且**
//! `processing_frames == 0` **且** 直通延迟**低于**处理路径（单边符号）。只断
//! "直通更快"的话，一个把 process 减半而非归零的实现照样过。
//!
//! ### 7. 预测性补零的可判定形态是「夹逼对 + 提前半程」
//!
//! 锚点「水位预测提前补」。若只断"水位为 0 时补了零"，则**非预测**实现（等到
//! 真的断了才补）也通过。故判据取两条：
//!
//! - **夹逼对**：`water == lead` 触发（边界取补）、`water == lead + 1` 不触发；
//!   两个夹逼点把提前量的界位置钉死；
//! - **半程**：`water == lead / 2`（水位尚未耗尽）已触发——这才是"预测"的
//!   牙齿。非预测实现在这里必然不触发。
//!
//! 计数双断：**无事件时恰为 0**，且**走真实路径造出 N 个事件后恰等于 N**（用
//! `==` 不用 `>=`，否则"每次 +2"也过）。
//!
//! ### 8. 实测上报必须能被与标称**不同**的值区分
//!
//! 锚点「真实测量上报——不标称」。判据若断 `reported == nominal`，则"直接返回
//! 标称常量"的实现满分通过。故 [`LatencyReport`] 把 `nominal_ms`（闭式算出）
//! 与 `reported_ms`（`observe` 写入）拆成两个**独立字段**。判据喂一个与标称
//! **不相等**的观测值，然后断 `reported == observed`（不是 `== nominal`）**且**
//! `reported != nominal`（否则上一条恒真，无区分力）。
//!
//! 命中率按 `hits / (hits + misses)` 整数 ppm 计算，判据独立重算并断分母是
//! **总次数**（净值口径而非绝对值口径）。非有限观测**拒收**而非兜底。

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、模式与并存（90 行 · 锚点：≤20ms 目标 / 并存互不拖累 / MMCSS 分级）
// ---------------------------------------------------------------------------

/// 端到端延迟目标（毫秒）——锚点「端到端 ≤20ms 目标」。
///
/// 感知依据：20ms 是可演奏阈值。与 [`PLAYABLE_PERCEIVED_MS`] 是**两个独立
/// 常量**，判据断二者严格有序。
pub const TARGET_LATENCY_MS: f32 = 20.0;

/// 音乐人可感知的显著延迟阈值（毫秒）——锚点「音乐人感知 30ms 为显著延迟」。
pub const PLAYABLE_PERCEIVED_MS: f32 = 30.0;

/// 固定处理开销（毫秒）——「处理」段的标称值。直通模式下被**归零**。
pub const PROC_OVERHEAD_MS: f32 = 0.5;

/// 采样率下界（含）。
pub const MIN_SAMPLE_RATE: u32 = 8_000;

/// 采样率上界（含）。
pub const MAX_SAMPLE_RATE: u32 = 384_000;

/// 48kHz 基准采样率——档位帧数在此基准定义，按实际采样率等比缩放。
pub const REFERENCE_SAMPLE_RATE: u32 = 48_000;

/// 会话族：低延迟会话或播放会话（锚点「低延迟会话与播放会话共存」）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SessionClass {
    LowLatency,
    Playback,
}

impl SessionClass {
    /// 两个族（判据断"恰好两族"而非"至少两族"）。
    pub const ALL: [SessionClass; 2] = [SessionClass::LowLatency, SessionClass::Playback];

    /// 稳定线序号（判据/遥测面用）。
    pub fn wire(self) -> u8 {
        match self {
            SessionClass::LowLatency => 0,
            SessionClass::Playback => 1,
        }
    }

    /// 可读标签（非空由判据断）。
    pub fn label(self) -> &'static str {
        match self {
            SessionClass::LowLatency => "LowLatency",
            SessionClass::Playback => "Playback",
        }
    }
}

/// MMCSS 优先级分级（F1336 纪律）——rank 越小越优先。
///
/// 分级顺序即 Windows MMCSS 的音频调度序：`Realtime` > `Capture` >
/// `Playback` > `Distribution` > `Games`。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MmcssPriority {
    Realtime,
    Capture,
    Playback,
    Distribution,
    Games,
}

impl MmcssPriority {
    /// 全部五档（判据断"恰好五档"且 rank 连续降序）。
    pub const ALL: [MmcssPriority; 5] = [
        MmcssPriority::Realtime,
        MmcssPriority::Capture,
        MmcssPriority::Playback,
        MmcssPriority::Distribution,
        MmcssPriority::Games,
    ];

    /// 调度 rank（越小越优先）。
    pub fn rank(self) -> u8 {
        match self {
            MmcssPriority::Realtime => 0,
            MmcssPriority::Capture => 1,
            MmcssPriority::Playback => 2,
            MmcssPriority::Distribution => 3,
            MmcssPriority::Games => 4,
        }
    }

    /// 由 rank 反查（遥测/日志消费面）。
    pub fn from_rank(r: u8) -> Option<MmcssPriority> {
        MmcssPriority::ALL.iter().copied().find(|p| p.rank() == r)
    }

    /// 可读标签。
    pub fn label(self) -> &'static str {
        match self {
            MmcssPriority::Realtime => "Realtime",
            MmcssPriority::Capture => "Capture",
            MmcssPriority::Playback => "Playback",
            MmcssPriority::Distribution => "Distribution",
            MmcssPriority::Games => "Games",
        }
    }
}

/// 会话族 → MMCSS 优先级。
///
/// 低延迟取 [`MmcssPriority::Capture`] 而**非** `Realtime`（头注要点 4）；
/// 播放取 [`MmcssPriority::Distribution`]（媒体回放是分布型负载）。
pub fn mmcss_of(class: SessionClass) -> MmcssPriority {
    match class {
        SessionClass::LowLatency => MmcssPriority::Capture,
        SessionClass::Playback => MmcssPriority::Distribution,
    }
}

/// 会话规格——低延迟会话与播放会话的共同描述。
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct SessionSpec {
    pub class: SessionClass,
    /// 缓冲档（决定周期）。
    pub tier: BufferTier,
    pub sample_rate: u32,
    /// 处理开销（毫秒）。直通模式下被**忽略**并按 0 计入。
    pub proc_ms: f32,
    /// 独占模式（F1327 独占模式）。低延迟会话**必须**为 `true`。
    pub exclusive: bool,
    /// 零处理直通（乐器监听）。
    pub passthrough: bool,
}

impl SessionSpec {
    /// 构造并校验规格。非法即拒收整表——**不静默修正**。
    ///
    /// 拒收四类：采样率越界、非有限或负的处理开销、低延迟未申请独占、
    /// 播放申请了独占（独占是低延迟专属工具，播放链抢独占会拖垮全系统音频）。
    pub fn new(
        class: SessionClass,
        tier: BufferTier,
        sample_rate: u32,
        proc_ms: f32,
        exclusive: bool,
        passthrough: bool,
    ) -> Result<Self, String> {
        if sample_rate < MIN_SAMPLE_RATE || sample_rate > MAX_SAMPLE_RATE {
            return Err(String::from("sample_rate_out_of_range"));
        }
        if !is_finite(proc_ms) || proc_ms < 0.0 {
            return Err(String::from("proc_ms_invalid"));
        }
        if class == SessionClass::LowLatency && !exclusive {
            return Err(String::from("low_latency_requires_exclusive"));
        }
        if class == SessionClass::Playback && exclusive {
            return Err(String::from("playback_must_not_grab_exclusive"));
        }
        Ok(SessionSpec {
            class,
            tier,
            sample_rate,
            proc_ms,
            exclusive,
            passthrough,
        })
    }

    /// 直通模式下的实际处理帧数（每周期）。直通为 `0`——「零 DSP（纯监听
    /// 路径）」的字面量，非"近似为零"。
    pub fn processing_frames(&self) -> u32 {
        if self.passthrough {
            0
        } else {
            self.tier.frames(self.sample_rate)
        }
    }

    /// 该会话的端到端延迟（毫秒，闭式）。
    pub fn endpoint_ms(&self) -> f32 {
        effective_latency_ms(self.tier, self.sample_rate, self.proc_ms, self.passthrough)
    }
}

/// 端到端延迟分解（输入→处理→输出三段）。
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct LatencyBreakdown {
    pub input_period_ms: f32,
    pub process_ms: f32,
    pub output_period_ms: f32,
    pub total_ms: f32,
}

/// 三段之和（判据侧独立重算的**对照式**，与被测量无调用关系——避免"向被测
/// 函数问答案"的自证式判据）。
pub fn total_of_parts(input_ms: f32, process_ms: f32, output_ms: f32) -> f32 {
    input_ms + process_ms + output_ms
}

/// 求延迟三段分解。
pub fn breakdown(spec: &SessionSpec) -> LatencyBreakdown {
    let period_ms = spec.tier.period_ms(spec.sample_rate);
    let process_ms = if spec.passthrough { 0.0 } else { spec.proc_ms };
    LatencyBreakdown {
        input_period_ms: period_ms,
        process_ms,
        output_period_ms: period_ms,
        total_ms: total_of_parts(period_ms, process_ms, period_ms),
    }
}

/// 是否达到端到端目标（`<=`，边界归属明确：恰好 20ms 算达标）。
pub fn meets_target(endpoint_ms: f32) -> bool {
    is_finite(endpoint_ms) && endpoint_ms <= TARGET_LATENCY_MS
}

/// 每 tick 分给低延迟会话的调度槽位。
pub const LOW_SLOTS_PER_TICK: u32 = 1;

/// 每 tick 分给播放会话的调度槽位（低延迟之后）。
pub const PLAY_SLOTS_PER_TICK: u32 = 4;

/// 单次调度裁决（互不拖累的显式面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CoexistTick {
    /// 低延迟会话本 tick 是否被服务。
    pub low_served: bool,
    /// 低延迟是否**先于**播放被服务（并存调度优先级）。
    pub low_served_first: bool,
    /// 播放会话实际获服务的个数。
    pub play_served: usize,
    /// 播放会话未获服务的个数（**显性**，不静默降级）。
    pub play_starved: usize,
}

/// 低延迟与播放会话的并存调度器。
///
/// 承载锚点「并存互不拖累」：低延迟会话至多一个（第二个即拒收），播放会话
/// 多个。低延迟的调度提前量与优先级**只由它自己的 spec 决定**，与 play 侧的
/// 数量和缓冲档**零耦合**。
#[derive(Clone, Debug)]
pub struct CoexistScheduler {
    low: Option<SessionSpec>,
    play: Vec<SessionSpec>,
    ticks: u32,
}

impl CoexistScheduler {
    /// 构造空调度器。
    pub fn new() -> Self {
        CoexistScheduler {
            low: None,
            play: Vec::new(),
            ticks: 0,
        }
    }

    /// 挂载会话。低延迟会话至多一个，重复挂载即拒收。
    pub fn attach(&mut self, spec: SessionSpec) -> Result<(), String> {
        match spec.class {
            SessionClass::LowLatency => {
                if self.low.is_some() {
                    return Err(String::from("low_latency_session_already_attached"));
                }
                self.low = Some(spec);
            }
            SessionClass::Playback => self.play.push(spec),
        }
        Ok(())
    }

    /// 低延迟会话规格（只读）。
    pub fn low_spec(&self) -> Option<&SessionSpec> {
        self.low.as_ref()
    }

    /// 低延迟会话的调度提前量（毫秒）。
    ///
    /// **只由低延迟 spec 决定**：挂载播放会话不改变它一个比特。判据用
    /// "0/1/8 个不同档位的播放会话下逐位相等"钉住。
    pub fn low_wake_lead_ms(&self) -> Option<f32> {
        self.low.as_ref().map(|s| s.endpoint_ms())
    }

    /// 低延迟会话的 MMCSS 优先级。
    pub fn low_priority(&self) -> Option<MmcssPriority> {
        self.low.as_ref().map(|s| mmcss_of(s.class))
    }

    /// 播放会话数。
    pub fn play_count(&self) -> usize {
        self.play.len()
    }

    /// 累计 tick 数。
    pub fn ticks(&self) -> u32 {
        self.ticks
    }

    /// 执行一次调度：低延迟先行，播放取剩余槽位。
    ///
    /// `play_starved` **显性**记录未获服务的播放会话数——超槽位时不静默丢弃。
    pub fn tick(&mut self) -> CoexistTick {
        self.ticks = self.ticks.saturating_add(1);
        let low_served = self.low.is_some();
        let capacity = PLAY_SLOTS_PER_TICK as usize;
        let demand = self.play.len();
        let play_served = if demand < capacity { demand } else { capacity };
        CoexistTick {
            low_served,
            low_served_first: low_served,
            play_served,
            play_starved: demand - play_served,
        }
    }
}

// ---------------------------------------------------------------------------
// 二、小缓冲策略（90 行 · 锚点：独占模式 + 最小缓冲 + 零处理直通选项）
// ---------------------------------------------------------------------------

/// 小缓冲档位：1ms / 2ms / 5ms（锚点字面「1ms/2ms/5ms 档」）。
///
/// 帧数在 48kHz 基准定义，按实际采样率等比缩放；周期由帧数与采样率导出，
/// 保证换采样率时档位语义（周期毫秒）不变。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BufferTier {
    Ms1,
    Ms2,
    Ms5,
}

impl BufferTier {
    /// 三档（判据断"恰好三档"）。
    pub const ALL: [BufferTier; 3] = [BufferTier::Ms1, BufferTier::Ms2, BufferTier::Ms5];

    /// 48kHz 基准帧数。
    pub fn base_frames(self) -> u32 {
        match self {
            BufferTier::Ms1 => 48,
            BufferTier::Ms2 => 96,
            BufferTier::Ms5 => 240,
        }
    }

    /// 指定采样率下的周期帧数（整数缩放）。
    pub fn frames(self, sample_rate: u32) -> u32 {
        let scaled = (self.base_frames() as u64 * sample_rate as u64)
            / REFERENCE_SAMPLE_RATE as u64;
        scaled as u32
    }

    /// 指定采样率下的周期（毫秒）。
    pub fn period_ms(self, sample_rate: u32) -> f32 {
        self.frames(sample_rate) as f32 * 1000.0 / sample_rate as f32
    }

    /// 稳定线序号。
    pub fn wire(self) -> u8 {
        match self {
            BufferTier::Ms1 => 0,
            BufferTier::Ms2 => 1,
            BufferTier::Ms5 => 2,
        }
    }

    /// 可读标签。
    pub fn label(self) -> &'static str {
        match self {
            BufferTier::Ms1 => "1ms",
            BufferTier::Ms2 => "2ms",
            BufferTier::Ms5 => "5ms",
        }
    }
}

/// 缓冲档 → underrun 风险（ppm，整数）。
///
/// 权衡表的**反向**半边：缓冲越小，underrun 越频繁。判据断该列**降序**，
/// 与周期列的升序构成两条方向相反的序关系——两条律同时成立才是"权衡"，
/// 只断互异性则随机常数也能过。
pub const UNDERRUN_RISK_PPM: [(BufferTier, u32); 3] = [
    (BufferTier::Ms1, 180_000),
    (BufferTier::Ms2, 60_000),
    (BufferTier::Ms5, 10_000),
];

/// 取档位的 underrun 风险（ppm）。
pub fn underrun_risk_ppm(tier: BufferTier) -> u32 {
    UNDERRUN_RISK_PPM
        .iter()
        .find(|(t, _)| *t == tier)
        .map(|(_, ppm)| *ppm)
        .unwrap_or(0)
}

/// 端到端延迟闭式：`2 × period + process`（直通时 process 取 0）。
///
/// 系数 `2` 是输入周期与输出周期各一段（锚点「输入→处理→输出」）。所有闭式值
/// 落在 `.0 / .5` 上，二进制精确，判据可用 `==` 精确对账。
pub fn effective_latency_ms(
    tier: BufferTier,
    sample_rate: u32,
    proc_ms: f32,
    passthrough: bool,
) -> f32 {
    let process_ms = if passthrough { 0.0 } else { proc_ms };
    total_of_parts(
        tier.period_ms(sample_rate),
        process_ms,
        tier.period_ms(sample_rate),
    )
}

/// 构造低延迟会话规格：强制独占（独占模式 F1327）。
///
/// 这是锚点「独占模式+最小缓冲+零处理直通选项」的**推荐路径**：独占已由本函数
/// 保证，调用方无需也无法忘记申请独占。
pub fn low_latency_spec(
    tier: BufferTier,
    sample_rate: u32,
    passthrough: bool,
) -> Result<SessionSpec, String> {
    SessionSpec::new(
        SessionClass::LowLatency,
        tier,
        sample_rate,
        PROC_OVERHEAD_MS,
        true,
        passthrough,
    )
}

/// 构造播放会话规格：不得申请独占。
pub fn playback_spec(tier: BufferTier, sample_rate: u32) -> Result<SessionSpec, String> {
    SessionSpec::new(
        SessionClass::Playback,
        tier,
        sample_rate,
        PROC_OVERHEAD_MS,
        false,
        false,
    )
}

// ---------------------------------------------------------------------------
// 三、underrun 特化（90 行 · 锚点：预测性补零 + 低延迟专属恢复路径）
// ---------------------------------------------------------------------------

/// 恢复策略。
///
/// [`StandardTiered`] 是 F1327 的标准三级恢复；[`LowLatencyFast`] 是本模块的
/// **低延迟专属恢复**：更快恢复（时延代价更小），代价是音质保真度下降——
/// 模式内的显式权衡，不是缺陷。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RecoveryPolicy {
    StandardTiered,
    LowLatencyFast,
}

impl RecoveryPolicy {
    /// 两档。
    pub const ALL: [RecoveryPolicy; 2] = [
        RecoveryPolicy::StandardTiered,
        RecoveryPolicy::LowLatencyFast,
    ];

    /// 恢复引入的额外延迟代价（毫秒）。
    ///
    /// 特化档**严格小于**标准档（判据单边符号）；且标准档**大于 0**——否则
    /// "更快恢复"退化成"不恢复"（直接掩盖 underrun），那是更坏的失效。
    pub fn penalty_ms(self) -> f32 {
        match self {
            RecoveryPolicy::StandardTiered => 8.0,
            RecoveryPolicy::LowLatencyFast => 2.0,
        }
    }

    /// 档位序（0 = 标准，1 = 特化；特化即更激进的一档）。
    pub fn rank(self) -> u8 {
        match self {
            RecoveryPolicy::StandardTiered => 0,
            RecoveryPolicy::LowLatencyFast => 1,
        }
    }

    /// 可读标签。
    pub fn label(self) -> &'static str {
        match self {
            RecoveryPolicy::StandardTiered => "StandardTiered",
            RecoveryPolicy::LowLatencyFast => "LowLatencyFast",
        }
    }
}

/// 模式内权衡的显式声明（锚点「牺牲音质保时延——模式内权衡声明」）。
///
/// 该权衡是**本模式成立的必要代价**：小缓冲换来低时延，必然换来更脆弱的
/// underrun 恢复与更低的音质保真。声明使其可被审、可被遥测（F1418），而不是
/// 藏在实现里。
pub const MODE_TRADEOFF_DECLARATION: &str = "\
VE-F1412 模式内权衡（锚点：低延迟专属恢复（比标准三级恢复（F1327）更快的恢复\
（牺牲音质保时延——模式内权衡声明））：
  1. 小缓冲（1ms 档 underrun 风险 18%）换取端到端 ≤20ms 的可演奏时延。
  2. 恢复策略取 LowLatencyFast：额外延迟代价 2.0ms，低于标准三级恢复的 8.0ms；
     代价是恢复期间的音高/相位保真度下降（时延换音质，方向不可逆）。
  3. 预测性补零在水位尚未耗尽时提前触发，会引入可听的短促静音；
     这是小缓冲必然的代价，不做静默处理（十三·补：不静默降质）。
  4. 三档并存由调用方显式选择，引擎不代为降档到更稳的档（用户主权）。";

/// 预测性补零判据：`water <= lead` 时提前补零。
///
/// 锚点「水位预测提前补」。`lead` 是提前量（帧）。**边界取补**：恰等于提前量
/// 即触发；`lead + 1` 不触发——判据用夹逼对把提前量的界位置钉死。
pub fn should_pad_predicatively(water_frames: u32, lead_frames: u32) -> bool {
    water_frames <= lead_frames
}

/// underrun 守卫与预测补零计数。
#[derive(Clone, Debug)]
pub struct UnderrunGuard {
    policy: RecoveryPolicy,
    /// 预测提前量（帧）。低延迟档取整周期：提前一整个周期补零。
    pub lead_frames: u32,
    pads: u32,
    underruns: u32,
    recoveries: u32,
    recovery_latency_ms: f32,
}

impl UnderrunGuard {
    /// 构造守卫。`lead_frames == 0` 即"无预测"——拒绝（预测是本模式的必需
    /// 能力，不是可选项）。
    pub fn new(policy: RecoveryPolicy, lead_frames: u32) -> Result<Self, String> {
        if lead_frames == 0 {
            return Err(String::from("predictive_lead_must_be_positive"));
        }
        Ok(UnderrunGuard {
            policy,
            lead_frames,
            pads: 0,
            underruns: 0,
            recoveries: 0,
            recovery_latency_ms: 0.0,
        })
    }

    /// 当前恢复策略。
    pub fn policy(&self) -> RecoveryPolicy {
        self.policy
    }

    /// 推进一个周期：水位低于提前量则提前补零。
    ///
    /// 返回 `true` 表示本周期触发预测补零。**计数双断**（无事件为 0 / 真实
    /// 路径恰等于事件数）见判据 `H12-恢复-*`。
    pub fn advance(&mut self, water_frames: u32) -> bool {
        if should_pad_predicatively(water_frames, self.lead_frames) {
            self.pads = self.pads.saturating_add(1);
            self.underruns = self.underruns.saturating_add(1);
            self.recoveries = self.recoveries.saturating_add(1);
            self.recovery_latency_ms += self.policy.penalty_ms();
            true
        } else {
            false
        }
    }

    /// 累计恢复时延代价（毫秒）。
    pub fn recovery_latency_ms(&self) -> f32 {
        self.recovery_latency_ms
    }

    pub fn pads(&self) -> u32 {
        self.pads
    }

    pub fn underruns(&self) -> u32 {
        self.underruns
    }

    pub fn recoveries(&self) -> u32 {
        self.recoveries
    }
}

/// 低延迟守卫的推荐构造：提前量取整周期。
pub fn low_latency_guard(tier: BufferTier, sample_rate: u32) -> Result<UnderrunGuard, String> {
    UnderrunGuard::new(RecoveryPolicy::LowLatencyFast, tier.frames(sample_rate))
}

// ---------------------------------------------------------------------------
// 四、实测与切换（70 行 · 锚点：真实测量上报 / 进入退出不爆音）
// ---------------------------------------------------------------------------

/// 延迟实测上报账本。
///
/// **标称与实测是两个独立字段**：`nominal_ms` 由闭式算出，`reported_ms` 由
/// [`LatencyReport::observe`] 写入实测值。二者永不同源——这是「真实测量上报
/// ——不标称」的可判定形态。
#[derive(Clone, Debug)]
pub struct LatencyReport {
    nominal_ms: f32,
    reported_ms: f32,
    samples: u32,
    hits: u32,
    misses: u32,
    max_observed_ms: f32,
    rejected: u32,
}

impl LatencyReport {
    /// 以标称值建账（`reported_ms` 初值同标称，但**首个 observe 后即由实测
    /// 接管**）。
    pub fn new(nominal_ms: f32) -> Result<Self, String> {
        if !is_finite(nominal_ms) || nominal_ms < 0.0 {
            return Err(String::from("nominal_ms_invalid"));
        }
        Ok(LatencyReport {
            nominal_ms,
            reported_ms: nominal_ms,
            samples: 0,
            hits: 0,
            misses: 0,
            max_observed_ms: 0.0,
            rejected: 0,
        })
    }

    /// 记入一次实测观测。
    ///
    /// 非有限或负的观测**拒收**并计入 `rejected`——不写入、不兜底、不参与
    /// 命中统计（污染进上报值是不可查故障的典型来源）。
    pub fn observe(&mut self, observed_ms: f32) -> bool {
        if !is_finite(observed_ms) || observed_ms < 0.0 {
            self.rejected = self.rejected.saturating_add(1);
            return false;
        }
        self.samples = self.samples.saturating_add(1);
        self.reported_ms = observed_ms;
        if observed_ms > self.max_observed_ms {
            self.max_observed_ms = observed_ms;
        }
        if observed_ms <= TARGET_LATENCY_MS {
            self.hits = self.hits.saturating_add(1);
        } else {
            self.misses = self.misses.saturating_add(1);
        }
        true
    }

    /// 标称延迟（闭式值）。
    pub fn nominal_ms(&self) -> f32 {
        self.nominal_ms
    }

    /// 上报延迟（**实测**最后一次观测值）。
    pub fn reported_ms(&self) -> f32 {
        self.reported_ms
    }

    /// 观测次数（被拒的观测不计入）。
    pub fn samples(&self) -> u32 {
        self.samples
    }

    /// 达标次数（≤20ms）。
    pub fn hits(&self) -> u32 {
        self.hits
    }

    /// 未达标次数。
    pub fn misses(&self) -> u32 {
        self.misses
    }

    /// 被拒观测次数（显性，不静默）。
    pub fn rejected(&self) -> u32 {
        self.rejected
    }

    /// 最大观测值。
    pub fn max_observed_ms(&self) -> f32 {
        self.max_observed_ms
    }

    /// ≤20ms 命中率（ppm，整数）。分母是**总观测次数**（净值口径）。
    pub fn hit_rate_ppm(&self) -> u32 {
        let total = self.hits + self.misses;
        if total == 0 {
            return 0;
        }
        ((self.hits as u64 * 1_000_000u64) / total as u64) as u32
    }
}

/// 切换斜坡的默认帧数（@48kHz 约 2ms）。
pub const DEFAULT_RAMP_FRAMES: u32 = 96;

/// 模式切换的增益斜坡器（锚点「进入/退出低延迟的安全切换——不爆音」）。
///
/// 爆音的成因是增益**不连续**，故本器只保证一件事：相邻两帧增益差
/// `<= |to − from| / ramp_frames`（线性斜坡的精确斜率）。进入（`0→1`）与退出
/// （`1→0`）走同一逻辑，方向对称。
#[derive(Clone, Debug)]
pub struct SwitchSmoother {
    from: f32,
    to: f32,
    pos: u32,
    ramp: u32,
    steps: u32,
}

impl SwitchSmoother {
    /// 构造斜坡器。`ramp_frames == 0` 拒收（零帧斜坡即硬切 = 爆音）。
    pub fn new(from: f32, to: f32, ramp_frames: u32) -> Result<Self, String> {
        if !is_finite(from) || !is_finite(to) {
            return Err(String::from("switch_gain_non_finite"));
        }
        if ramp_frames == 0 {
            return Err(String::from("ramp_frames_must_be_positive"));
        }
        Ok(SwitchSmoother {
            from,
            to,
            pos: 0,
            ramp: ramp_frames,
            steps: 0,
        })
    }

    /// 当前增益（不推进）。
    pub fn gain(&self) -> f32 {
        self.at(self.pos)
    }

    /// 推进一帧并返回增益。`ramp == 1` 时为一步到位（最小无阶跃斜坡）。
    pub fn step(&mut self) -> f32 {
        self.steps = self.steps.saturating_add(1);
        if self.pos < self.ramp {
            self.pos += 1;
        }
        self.at(self.pos)
    }

    /// 斜坡是否走完（到位后增益**保持**，不再漂移）。
    pub fn is_done(&self) -> bool {
        self.pos >= self.ramp
    }

    /// 已推进帧数。
    pub fn steps(&self) -> u32 {
        self.steps
    }

    /// 斜坡长度（帧）。
    pub fn ramp_frames(&self) -> u32 {
        self.ramp
    }

    /// 相邻增益步长的**精确上界**（`|to − from| / ramp`）。
    pub fn max_step(&self) -> f32 {
        (self.to - self.from).abs() / self.ramp as f32
    }

    /// 内部插值（`pos` 越界即钳到终点，保证全程落在凸包内）。
    fn at(&self, pos: u32) -> f32 {
        let p = if pos > self.ramp { self.ramp } else { pos };
        self.from + (self.to - self.from) * (p as f32) / (self.ramp as f32)
    }
}

// ---------------------------------------------------------------------------
// 五、边界声明
// ---------------------------------------------------------------------------

/// 低延迟路径的边界声明（锚点「并存互不拖累」的边界侧重申）。
///
/// 该边界**不以注释兑现**，而由判据反查公开面：模块公开面中不存在平台独占
/// API 调用入口，也不存在全局独占开关——独占模式的**申请动作**属平台层，
/// 本模块只产出优先级与档位参数。新增"申请独占"的函数会直接转红。
pub const COEXIST_BOUNDARY: &str = "\
VE-F1412 边界声明（锚点：并存互不拖累）：
  1. 低延迟路径与播放链高缓冲路径**并存**：两者同时活动，互不挤占（F1416 预算
     仲裁、F1425 分池在更高层解决争用，本模块只保证低延迟侧的调度提前量与
     优先级不被 play 侧参数影响）。
  2. 本模块**不接管播放链**：播放会话只以规格（档位/采样率/独占=false）参与
     并存调度，低延迟路径不得改写播放会话的任何参数。
  3. 独占模式（F1327）的**申请动作**属平台层，本模块只产出 MMCSS 优先级与
     缓冲档参数；公开面无平台 API 入口、无全局独占开关。
  4. 延迟实测上报的是**实测值**（observe 写入），标称值仅作对照，不上报。";

/// 本模块公开面清单（判据反查用：新增"平台独占入口"类函数即转红）。
pub const PUBLIC_SURFACE: [&str; 14] = [
    "mmcss_of",
    "total_of_parts",
    "breakdown",
    "meets_target",
    "effective_latency_ms",
    "underrun_risk_ppm",
    "low_latency_spec",
    "playback_spec",
    "should_pad_predicatively",
    "low_latency_guard",
    "LatencyReport::new",
    "LatencyReport::observe",
    "SwitchSmoother::new",
    "SwitchSmoother::step",
];

/// 公开面中**禁止**出现的关键词（平台独占入口 / 全局开关）。
pub const FORBIDDEN_SURFACE_TOKENS: [&str; 4] =
    ["platform", "grant_exclusive", "global_exclusive", "set_global"];

/// `no_std` 下的有限性判定（不引入 libm）。
fn is_finite(x: f32) -> bool {
    x == x && x.abs() != f32::INFINITY
}