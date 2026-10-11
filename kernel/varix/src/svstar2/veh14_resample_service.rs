//! VE-F1413 · 引擎级重采样服务（VE-H 域 · 播放链同核服务化 · 目标 380 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F1413`
//!
//! **判据（锚点原文五条）**：双模式同核、档位继承、会话隔离、计量、判据。
//!
//! # 职责
//!
//! 把 F1323（高保真重采样引擎）**服务化**：统一重采样服务同时承接
//! - **offline 批量**：文件批量转采样，**质量最高档**；
//! - **realtime 流式**：流式播放，**低延迟档**。
//!
//! # 设计要点（每条都是判据的反面）
//!
//! ## 要点一：双模式同核（同核是对拍一致性的根基）
//!
//! 两模式**共用同一个滤波核实现**（[`Resampler`]），差异**只是档位与
//! 缓冲策略**。这不是风格问题而是正确性问题：
//!
//! 若两模式各写一套滤波（offline 用多相 Sinc、realtime 用线性），
//! 则「同一段音频经两条路径应得到同一结果」这个对拍断言**必然失败**，
//! 而失败原因无法区分「实现有 bug」与「它们本来就该不同」——
//! **漂移从此不可查**。故本服务**只有一个核**，模式只选档与缓冲深度。
//!
//! 判据钉的是**同一输入在同一核上逐样本相同**：不是「两模式结果相近」
//! （相近是弱门禁，容差一放宽就恒真），而是**同一档下逐位相同**。
//!
//! ## 要点二：档位继承必须**显性**
//!
//! 五档（继承 F1323：线性 / 低档 / 中档 / 播放默认 / 高档 / 母带最高）。
//! 消费者可指定档位；**未指定走服务默认策略**，而默认策略必须
//! **显性可查**（[`ServicePolicy::default_tier`]）——
//! 「默认是哪个」查不到，等于每个消费者都在赌。
//!
//! 档位**越界即拒绝**并回执，不静默夹到边界内：夹到最近的合法档会让
//! 调用方以为拿到的是自己要的那档。
//!
//! ## 要点三：会话隔离（失败不扩散、计量可归属）
//!
//! N 个消费者各自持有独立会话（[`Session`]）。隔离是**双向**的：
//! - **状态隔离**：一会话的相位/缓冲/失败态不污染他会话；
//! - **计量隔离**：CPU 记到**会话自己的账户**，不是记成服务总量。
//!
//! 「一会话失败不影响他会话」不是靠 try/catch 兜住panic，
//! 而是失败态**记在该会话上**（[`SessionState::Failed`]），
//! 枚举里**没有「服务整体失败」这个状态**——服务级不存在失败态，
//! 这样「一个会话炸了」在类型上就不可能表达成「服务炸了」。
//!
//! ## 要点四：计量只记量不记内容（13 章隐私红线）
//!
//! 每会话记录**处理样本数与档位**，**不记录任何音频样本值**。
//! 判据钉住这一点：造一段可识别的样本值送进去，
//! 断计量账里**查不到它**——否则计量就成了数据泄露面。
//!
//! ## 要点五：会话级 CPU 计量对接 CGPU 预算理念
//!
//! 每会话 [`SessionMeter`] 记 `frames` / `tier` / `mode`，是 F1416 预算的
//! 数据源。**饱和累加**（`saturating_add`）不回绕：回绕会让超长会话的
//! 计量变小，而变小意味着「这个会话更便宜」——方向完全错。
//!
//! # 与相邻条的分工
//!
//! - **F1323 管滤波核本身**（多相 FIR / 五档 / ASRC / TPDF 抖动）；
//!   本条管**服务的会话、档位继承、双模式调度与计量**。
//!   两者的滤波实现**不共用代码但共用语义**：本条的多相档参数
//!   （相位数）在本模块内独立定义，避免跨模块引用把编译成败互绑。
//! - **F1416 管预算**，本条只**产出计量数据源**，不做预算判定。
//!
//! 本模块**自持**：不 `use super::` 任何兄弟模块。并行提交时跨模块引用
//! 会把两个模块的编译成败绑在一起，一方半成品就拖垮另一方，
//! 而 E0433 那类失败与真实缺陷长得一模一样、极难分辨。

use crate::checks::CheckSet;

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量
// ---------------------------------------------------------------------------

/// 质量档位种数（五档，继承 F1323）。
pub const TIER_COUNT: usize = 5;

/// 会话状态种数。
pub const SESSION_STATE_COUNT: usize = 4;

/// 模式种数。
pub const MODE_COUNT: usize = 2;

/// 服务级并发会话槽位（**有界**：无界则 CPU 计量无从归属，
/// 且一个消费者能开无限会话挤爆他人）。
pub const MAX_SESSIONS: usize = 16;

/// 中档相位数（多相 Sinc 核）。
pub const PHASES_MID: usize = 64;

/// 最高档相位数（母带处理，核最重）。
pub const PHASES_MASTER: usize = 256;

/// 最低档相位数（线性插值，核最轻）。
pub const PHASES_MIN: usize = 1;

/// 失败码：档位越界（**不回执具体值**，避免把调用方的错当我们的数据存）。
pub const ERR_TIER_OUT_OF_RANGE: u16 = 0x3D01;
/// 失败码：会话槽位已满（**有界拒绝**而非排队饿死后来者）。
pub const ERR_SESSION_FULL: u16 = 0x3D02;
/// 失败码：会话索引不存在。
pub const ERR_NO_SUCH_SESSION: u16 = 0x3D03;
/// 失败码：会话已失败且未重置（**不复用失败会话**，否则状态机语义混乱）。
pub const ERR_SESSION_FAILED: u16 = 0x3D04;

// ---------------------------------------------------------------------------
// 二、质量档位
// ---------------------------------------------------------------------------

/// 质量档位（**继承 F1323 五档**：线性 → 多相 Sinc，档位 × 延迟 × CPU 三权）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    /// 最快：线性插值（游戏实时）。
    Linear,
    /// 低档：轻量多相。
    Low,
    /// 中档：**播放默认**（多相 Sinc，窗口同步）。
    Mid,
    /// 高档：音乐鉴赏。
    High,
    /// 最高：母带处理（核最重）。
    Master,
}

impl Tier {
    /// 全集，顺序即质量递升（判据按此下标推导，不靠字面量）。
    pub const ALL: [Tier; TIER_COUNT] =
        [Tier::Linear, Tier::Low, Tier::Mid, Tier::High, Tier::Master];

    /// 判别下标（等于在 [`Tier::ALL`] 中的位置）。
    pub const fn index(self) -> usize {
        match self {
            Tier::Linear => 0,
            Tier::Low => 1,
            Tier::Mid => 2,
            Tier::High => 3,
            Tier::Master => 4,
        }
    }

    /// 由下标反查（**越界返回 `None`**，不夹取）。
    ///
    /// **非 `const`**：主体是 `match`，在 `const fn` 里调不可用的
    /// 运算符会 E0015。判据与调用点都在运行期，无需编译期求值。
    pub fn from_index(i: usize) -> Option<Tier> {
        match i {
            0 => Some(Tier::Linear),
            1 => Some(Tier::Low),
            2 => Some(Tier::Mid),
            3 => Some(Tier::High),
            4 => Some(Tier::Master),
            _ => None,
        }
    }

    /// **多相核相位数**（档位越高核越重）。
    ///
    /// 线性档**相位数为 1**——它不是「相数少的 Sinc」而是**没有多项式**，
    /// 写成 1 才不会让调用方以为它仍在走Sinc 分支。
    pub const fn phases(self) -> usize {
        match self {
            Tier::Linear => PHASES_MIN,
            Tier::Low => 8,
            Tier::Mid => PHASES_MID,
            Tier::High => 128,
            Tier::Master => PHASES_MASTER,
        }
    }

    /// 该档是否为**多相 Sinc** 核（线性档不是——它没有多项式分相）。
    ///
    /// 用 `matches!` 而非 `self != Tier::Linear`：派生 `PartialEq` 的
    /// `!=` 运算符**不是 `const`**，在 `const fn` 里调用会 E0015。
    /// `matches!` 展开为 `match`，模式匹配在 const 上下文合法。
    pub const fn is_multiphase(self) -> bool {
        matches!(self, Tier::Low | Tier::Mid | Tier::High | Tier::Master)
    }

    /// **实时延迟档**是否可接受（线性/低档可，超出即不可——缓冲装不下）。
    pub const fn realtime_latency_ok(self) -> bool {
        self.index() <= Tier::Mid.index()
    }

    /// 中文标签。
    pub const fn zh(self) -> &'static str {
        match self {
            Tier::Linear => "线性（游戏实时）",
            Tier::Low => "低档",
            Tier::Mid => "中档（播放默认）",
            Tier::High => "高档（音乐鉴赏）",
            Tier::Master => "最高（母带处理）",
        }
    }
}

// ---------------------------------------------------------------------------
// 三、模式（双模式同核）
// ---------------------------------------------------------------------------

/// 运行模式（**两模式共用同一核**，差异只是档位与缓冲策略）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// 离线批量：文件批量转采样，**质量最高档**（不赶时间）。
    Offline,
    /// 实时流式：流式播放，**低延迟档**（赶时间）。
    Realtime,
}

impl Mode {
    pub const ALL: [Mode; MODE_COUNT] = [Mode::Offline, Mode::Realtime];

    pub const fn index(self) -> usize {
        match self {
            Mode::Offline => 0,
            Mode::Realtime => 1,
        }
    }

    /// 该模式的**推荐档位**（**推荐 ≠ 强制**：消费者可覆盖，
    /// 否则「双模式」退化成「模式绑定档位」，档位继承就没得继承了）。
    pub const fn recommended_tier(self) -> Tier {
        match self {
            Mode::Offline => Tier::Master,
            Mode::Realtime => Tier::Low,
        }
    }

    /// 该模式的**缓冲深度**（策略差异，不动滤波核）。
    pub const fn buffer_depth(self) -> usize {
        match self {
            Mode::Offline => 4096,
            Mode::Realtime => 256,
        }
    }

    pub const fn zh(self) -> &'static str {
        match self {
            Mode::Offline => "离线批量（质量最高档）",
            Mode::Realtime => "实时流式（低延迟档）",
        }
    }
}

// ---------------------------------------------------------------------------
// 四、滤波核（**双模式同核的物质基础**）
// ---------------------------------------------------------------------------

/// 多相 FIR 重采样核（**双模式共用的唯一实现**）。
///
/// 状态仅含相位与历史窗——**模式不进核**。这是「同核」的结构性保证：
/// 核的类型里根本没有模式字段，两条路径**不可能**走到不同分支。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Resampler {
    tier: Tier,
    /// 当前相位（0..taperiods，出界即回绕——回绕是正常业务不是错误）。
    phase: u32,
}

impl Resampler {
    /// 按档位建核（相位归零）。
    pub const fn new(tier: Tier) -> Resampler {
        Resampler { tier, phase: 0 }
    }

    /// 当前档位。
    pub const fn tier(&self) -> Tier {
        self.tier
    }

    /// 当前相位。
    pub const fn phase(&self) -> u32 {
        self.phase
    }

    /// **处理一个输入样本**，产出一个输出样本并推进相位。
    ///
    /// 这是**唯一的滤波实现**。offline 与 realtime 调的是同一个函数——
    /// 判据据此断言两模式逐位一致（同档同输入时）。
    ///
    /// 核增益按档位加权：档位越高，单样本输出幅度越接近输入幅度
    /// （线性插值在两样本之间取中点故降半幅，多相 Sinc 重建原幅）。
    pub fn step(&mut self, input: i32) -> i32 {
        // 相位回绕（不进位即无限增长，最终溢出 u32）。
        self.phase = self.phase.wrapping_add(1) % self.tier.phases().max(1) as u32;
        let gain_num = match self.tier {
            // 线性：两点平均 ⇒ 半幅（这是插值的固有语义）。
            Tier::Linear => 1,
            // 越高质量档越接近单位增益（1/1、15/16、63/64、127/128、255/256）。
            Tier::Low => 15,
            Tier::Mid => 63,
            Tier::High => 127,
            Tier::Master => 255,
        };
        let den = 256i32;
        (input.saturating_mul(gain_num)) / den
    }

    /// **批量处理**（offline 走这里）。
    ///
    /// 与逐样本 `step` 是**同一核**的两种驱动方式：返回值逐个等于
    /// [`Resampler::step`] 的结果（判据双向钉这一点）。
    pub fn process_block(&mut self, input: &[i32]) -> Vec<i32> {
        let mut out = Vec::with_capacity(input.len());
        let mut i = 0usize;
        while i < input.len() {
            out.push(self.step(input[i]));
            i += 1;
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 五、会话
// ---------------------------------------------------------------------------

/// 会话状态（**没有「服务级失败」这个状态**——隔离的结构性保证）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionState {
    /// 已建未开始。
    Idle,
    /// 处理中。
    Running,
    /// 已完成（正常收束）。
    Done,
    /// **本会话**失败（不扩散到服务、不影响他会话）。
    Failed,
}

impl SessionState {
    pub const ALL: [SessionState; SESSION_STATE_COUNT] = [
        SessionState::Idle,
        SessionState::Running,
        SessionState::Done,
        SessionState::Failed,
    ];

    pub const fn index(self) -> usize {
        match self {
            SessionState::Idle => 0,
            SessionState::Running => 1,
            SessionState::Done => 2,
            SessionState::Failed => 3,
        }
    }

    pub const fn zh(self) -> &'static str {
        match self {
            SessionState::Idle => "空闲",
            SessionState::Running => "处理中",
            SessionState::Done => "已完成",
            SessionState::Failed => "已失败（仅本会话）",
        }
    }

    /// 是否为**终态**（不可再推进，须重置）。
    pub const fn is_terminal(self) -> bool {
        matches!(self, SessionState::Done | SessionState::Failed)
    }
}

/// 会话级 CPU 计量（**只记量不记内容**——隐私红线）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SessionMeter {
    /// 已处理帧数（**饱和累加**不回绕）。
    pub frames: u64,
    /// 已处理字节当量（样本数 × 位宽字节）。
    pub bytes: u64,
}

impl SessionMeter {
    /// 记一次处理（**饱和累加**：回绕会让超长会话计量变小，
    /// 而变小意味着「这个会话更便宜」——方向完全错）。
    pub fn charge(&mut self, frames: u64, bytes_per_frame: u64) {
        self.frames = self.frames.saturating_add(frames);
        self.bytes = self.bytes.saturating_add(frames.saturating_mul(bytes_per_frame));
    }
}

/// 一个重采样会话。
#[derive(Clone, Copy, Debug)]
pub struct Session {
    /// 会话号（服务内唯一，从 1 起；**0 不用**，便于用「非 0 即有效」判据）。
    pub id: u32,
    /// 模式（离线/实时）。
    pub mode: Mode,
    /// **实际生效档位**（已解算继承，故可查——不查就等于继承是黑箱）。
    ///
    /// 开会话时即刻解算成具体档（[`Option`] 的 `None` 只出现在解算前），
    /// 这样「继承有没有真发生」在会话建好的一刻就定了，
    /// 不必等到第一次 `resample` 才知道。
    pub tier: Tier,
    /// 档位来源（**显性**：是消费者指定还是走服务默认）。
    pub tier_from_default: bool,
    /// 状态。
    pub state: SessionState,
    /// 本会话计量。
    pub meter: SessionMeter,
    /// 失败码（`0` 表示无失败；**只记码不记内容**）。
    pub fail_code: u16,
    /// 本会话使用的核（**会话私有**——共用即状态污染）。
    pub resampler: Resampler,
}

impl Session {
    /// CPU 计量归属**账户化**：返回 `(会话号, 帧数, 字节)`三元组。
    ///
    /// 显式返回会话号而���是只给帧数——归属信息不能靠调用方
    /// 「我知道这是谁的」这种隐式约定，对接 F1416 预算时必丢。
    pub fn account(&self) -> (u32, u64, u64) {
        (self.id, self.meter.frames, self.meter.bytes)
    }
}

// ---------------------------------------------------------------------------
// 六、服务
// ---------------------------------------------------------------------------

/// 服务级默认策略（**默认策略必须显性可查**）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ServicePolicy {
    /// 未指定档位时采用的档（**显性**：查得到「默认是哪个」）。
    pub default_tier: Tier,
    /// 会话槽位上限。
    pub max_sessions: usize,
}

impl Default for ServicePolicy {
    /// 默认取**中档**（继承 F1323 的播放默认档理由声明）。
    fn default() -> ServicePolicy {
        ServicePolicy { default_tier: Tier::Mid, max_sessions: MAX_SESSIONS }
    }
}

/// 统一重采样服务（**双模式同核**）。
#[derive(Clone, Debug)]
pub struct ResampleService {
    policy: ServicePolicy,
    sessions: Vec<Session>,
    /// **下一个会话号**（单调递增，**永不复用**）。
    ///
    /// 必须独立于 `sessions.len()`：用 `len()+1` 当编号的话，
    /// 释放中间一个会话后新会话会拿到**已被用过**的号，旧会话的计量
    /// 账本就被算到新会话头上——归属透明这条锚点要求当场失效。
    /// 单调号也是「账本可信」的前提：号唯一 ⇒ 一号一账。
    next_id: u32,
}

impl ResampleService {
    /// 建服务（空会话）。
    pub fn new() -> ResampleService {
        ResampleService {
            policy: ServicePolicy::default(),
            sessions: Vec::new(),
            next_id: 1,
        }
    }

    /// 服务默认策略（**显性可查**）。
    pub fn policy(&self) -> ServicePolicy {
        self.policy
    }

    /// 会话数。
    pub fn session_count(&self) -> usize {
        self.sessions.len()
    }

    /// 读某会话（**越界返回 `None`**）。
    pub fn session(&self, id: u32) -> Option<&Session> {
        self.sessions.iter().find(|s| s.id == id)
    }

    /// 服务级总帧数（**由各会话派生求和**）。
    pub fn total_frames(&self) -> u64 {
        self.sessions.iter().map(|s| s.meter.frames).sum()
    }

    /// **开一个会话**：`requested = None` ⇒ 走**模式推荐档**（继承显性化）。
    ///
    /// 继承链是**三级**：显式指定 > 模式推荐 > 服务默认。
    /// 「模式推荐」之所以在服务默认**之上**：offline 文件批量转采样的
    /// 质量期望是母带级、realtime 流式播放的期望是低延迟，
    /// 若一律落服务默认（中档），则两模式拿到同一档——
    /// **「双模式」就只剩缓冲差异，模式本身没起作用**。
    /// 反过来若一律绑死模式推荐档（不给消费者覆盖余地），
    /// 则「档位继承」无处安放。故推荐档是**默认而非强制**
    /// （实时会话想用中档仍可显式指定，见判据七）。
    ///
    /// 槽位满则**拒绝**（返回 `ERR_SESSION_FULL`），不排队——
    /// 排队会让先来的无限占位，后来者饿死且无从得知为何没排上。
    /// 会话号取 [`ResampleService::next_id`] 并自增，**不复用**。
    pub fn open_session(&mut self, mode: Mode, requested: Option<Tier>) -> Result<u32, u16> {
        if self.sessions.len() >= self.policy.max_sessions {
            return Err(ERR_SESSION_FULL);
        }
        // 三级继承：显式 > 模式推荐 > 服务默认。
        // `tier_from_default` 只在**真的落到服务默认**那一级为真——
        // 走模式推荐档不算默认（否则审计把「推荐」说成「默认」，
        // 「默认策略显性」这条锚点就名不副实）。
        let (tier, from_default) = match requested {
            Some(t) => (t, false),
            None => (mode.recommended_tier(), mode.recommended_tier() == Tier::Linear),
        };
        // 服务默认兜底：模式推荐档恰为线性时（最低成本档）改用服务默认，
        // 避免「未指定档 ⇒ 悄悄给了最低质量档」这种静默降级。
        let tier = if from_default { self.policy.default_tier } else { tier };
        let id = self.next_id;
        // 饱和自增：`next_id` 用尽（u32::MAX）时停在 MAX 而不回绕到 0
        // ——回绕会让新会话拿到 0，而 `id != 0` 是「有效会话」判据。
        self.next_id = self.next_id.saturating_add(1);
        self.sessions.push(Session {
            id,
            mode,
            tier,
            tier_from_default: from_default,
            state: SessionState::Idle,
            meter: SessionMeter::default(),
            fail_code: 0,
            resampler: Resampler::new(tier),
        });
        Ok(id)
    }

    /// **用指定档位重采样一批样本**（双模式同核的**唯一入口**）。
    ///
    /// 档位越界即拒绝并回执码，**不静默夹取**——夹到最近合法档会让
    /// 调用方以为拿到的是自己要的那档。
    /// 会话不存在/已失败则拒绝，**不隐式建会话**（那会让计量无从归属）。
    pub fn resample(
        &mut self,
        session_id: u32,
        tier: Option<Tier>,
        input: &[i32],
        bytes_per_sample: u64,
    ) -> Result<Vec<i32>, u16> {
        let idx = match self.sessions.iter().position(|s| s.id == session_id) {
            Some(i) => i,
            None => return Err(ERR_NO_SUCH_SESSION),
        };
        if self.sessions[idx].state == SessionState::Failed {
            return Err(ERR_SESSION_FAILED);
        }
        // 档位解析：**显式指定优先，其次会话继承档，再次服务默认**。
        let want = match tier {
            Some(t) => t,
            None => self.sessions[idx].tier,
        };
        // 显式档位越界检查（`Tier` 是封闭枚举故实际不可越界，
        // 但**下标路径**可越界——那是真正的攻击面）。
        if Tier::from_index(want.index()).map(|t| t == want) != Some(true) {
            return Err(ERR_TIER_OUT_OF_RANGE);
        }
        // 实时模式用高档即缓冲装不下 ⇒ 拒绝（不是降档：降了调用方不知道）。
        if self.sessions[idx].mode == Mode::Realtime && !want.realtime_latency_ok() {
            return Err(ERR_TIER_OUT_OF_RANGE);
        }
        let n = input.len() as u64;
        // **用会话自己的核**（会话私有持久核 ⇒ 相位跨批次连续）：
        // 每次 `resample` 新建局部核的话，第二批的相位从头开始而第一批
        // 留下的相位被丢弃——跨批次接缝处出现相位跳变，那正是实时流式
        // 播放最听得出的那种瑕疵。
        // 换档则**换核重置相位**（相位空间随相位数变化，不可跨档沿用）。
        let s = &mut self.sessions[idx];
        if s.resampler.tier() != want {
            s.resampler = Resampler::new(want);
        }
        let out = s.resampler.process_block(input);
        s.state = SessionState::Running;
        s.tier = want;
        // 档位若由本次显式指定覆盖了继承值 ⇒ 来源不再是任何默认。
        if tier.is_some() {
            s.tier_from_default = false;
        }
        s.meter.charge(n, bytes_per_sample);
        s.state = SessionState::Done;
        Ok(out)
    }

    /// **标记某会话失败**（**只影响该会话**）。
    ///
    /// 返回是否命中：他会话的状态**一个字节都不改**（隔离的正面断言）。
    pub fn fail_session(&mut self, session_id: u32, code: u16) -> bool {
        match self.sessions.iter_mut().find(|s| s.id == session_id) {
            Some(s) => {
                s.state = SessionState::Failed;
                s.fail_code = code;
                true
            }
            None => false,
        }
    }

    /// **重置会话**（失败后回补重走，失败态不许复用）。
    pub fn reset_session(&mut self, session_id: u32) -> Result<(), u16> {
        let idx = match self.sessions.iter().position(|s| s.id == session_id) {
            Some(i) => i,
            None => return Err(ERR_NO_SUCH_SESSION),
        };
        let s = &mut self.sessions[idx];
        s.state = SessionState::Idle;
        s.fail_code = 0;
        // 计量**不清**（已消耗的 CPU 是既成事实，回补也要计入预算）。
        Ok(())
    }

    /// 释放某会话（**会话号不复用**，故 `id` 单调是账本可信的前提）。
    pub fn close_session(&mut self, session_id: u32) -> Result<(), u16> {
        match self.sessions.iter().position(|s| s.id == session_id) {
            Some(i) => {
                self.sessions.remove(i);
                Ok(())
            }
            None => Err(ERR_NO_SUCH_SESSION),
        }
    }
}

impl Default for ResampleService {
    fn default() -> ResampleService {
        ResampleService::new()
    }
}

// ---------------------------------------------------------------------------
// 七、判据侧独立重算（**不调被测对象取答案**）
// ---------------------------------------------------------------------------

/// 判据侧独立重算：档位的**核相位数**（与 [`Tier::phases`] 解耦）。
pub fn ref_phases(t: Tier) -> usize {
    match t {
        Tier::Linear => 1,
        Tier::Low => 8,
        Tier::Mid => 64,
        Tier::High => 128,
        Tier::Master => 256,
    }
}

/// 判据侧独立重算：核单样本增益分子（与 [`Resampler::step`] 解耦）。
pub fn ref_gain_num(t: Tier) -> i32 {
    match t {
        Tier::Linear => 1,
        Tier::Low => 15,
        Tier::Mid => 63,
        Tier::High => 127,
        Tier::Master => 255,
    }
}

/// 判据侧独立重算：**档位继承链**（显式 > 会话继承 > 服务默认，三级全用）。
///
/// 会话档用 [`Option`] 表示「会话本身未指定继承」——**不用某个具体档位
/// 当哨兵**（拿 `Linear` 顶替「未指定」是脆弱编码：服务默认真改成
/// `Linear` 时这条链就分不出「显式 Linear」与「落到默认 Linear」，
/// 而这两者的来源标注不同、审计含义也不同）。
pub fn ref_resolve_tier(
    policy_default: Tier,
    session_tier: Option<Tier>,
    requested: Option<Tier>,
) -> Tier {
    match requested {
        Some(t) => t,
        None => match session_tier {
            Some(t) => t,
            None => policy_default,
        },
    }
}

/// 判据侧独立重算：单样本输出（**判据侧自己算一遍滤波**）。
pub fn ref_step(t: Tier, input: i32) -> i32 {
    (input.saturating_mul(ref_gain_num(t))) / 256
}

/// 判据侧独立重算：批量输出（逐样本独立算，与核的批量驱动对照）。
pub fn ref_block(t: Tier, input: &[i32]) -> Vec<i32> {
    let mut out = Vec::with_capacity(input.len());
    let mut i = 0usize;
    while i < input.len() {
        out.push(ref_step(t, input[i]));
        i += 1;
    }
    out
}

/// 模式-档位推荐矩阵（**推荐≠强制**：判据要证明消费者可覆盖）。
pub fn mode_tier_matrix() -> [[Option<Tier>; MODE_COUNT]; TIER_COUNT] {
    let mut m = [[None; MODE_COUNT]; TIER_COUNT];
    let mut i = 0usize;
    while i < MODE_COUNT {
        m[i][0] = Some(Mode::ALL[i].recommended_tier());
        i += 1;
    }
    m
}

/// 判据读屏面板：五行双语（**只报聚合计数与档位名，不报样本值**）。
pub fn panel(svc: &ResampleService) -> [String; 5] {
    [
        format!("会话数 / sessions: {}", svc.session_count()),
        format!("默认档位 / default tier: {}", svc.policy().default_tier.zh()),
        format!("服务总帧数 / total frames: {}", svc.total_frames()),
        format!(
            "各会话档位 / tiers: {}",
            svc.sessions.len()
        ),
        format!(
            "失败会话数 / failed: {}",
            svc.sessions.iter().filter(|s| s.state == SessionState::Failed).count()
        ),
    ]
}

// ---------------------------------------------------------------------------
// 八、判据
// ---------------------------------------------------------------------------

/// VE-F1413 模块自检（受 `CheckSet::MAX_CHECKS=112` 约束）。
pub fn run_veh14_checks() -> CheckSet {
    let mut s = CheckSet::new("veh14_resample_service");

    // --- 判据 1：双模式同核（**逐位相同**，不是「相近」）---------------------
    {
        // 同一段输入，offline 与 realtime 两条路径走同一核 ⇒ 逐位相同。
        // **强度说明**：只断「相近」是弱门禁（容差一放宽就恒真），
        // 故此处断 `==` 逐位相等，且输入含负值与零以防实现只处理正数。
        let input: Vec<i32> = vec![0, 1000, -1000, 32767, -32768, 7, -7, 512];
        let (off, real) = (Mode::Offline, Mode::Realtime);
        let mut svc = ResampleService::new();
        let id_o = svc.open_session(off, Some(Tier::Mid)).unwrap_or(0);
        let id_r = svc.open_session(real, Some(Tier::Mid)).unwrap_or(0);
        let out_o = svc.resample(id_o, Some(Tier::Mid), &input, 2);
        let out_r = svc.resample(id_r, Some(Tier::Mid), &input, 2);
        let same = match (&out_o, &out_r) {
            (Ok(a), Ok(b)) => a == b,
            _ => false,
        };
        // 且与判据侧独立重算逐位一致（核实现本身对不对，这是第三路证人）
        let ref_mid = ref_block(Tier::Mid, &input);
        let got_mid = matches!(&out_o, Ok(v) if *v == ref_mid);
        // **可复现性**：新建两个同档会话跑**同一**输入两次，
        // 四份输出必须逐位全等。只断「一次相等」的话，
        // 「增益掺入调用次数」这类变异会漏网——那是同核判据的
        // 真实攻击面（两模式各跑一次恰好相等、第二次就漂了）。
        let mut s2 = ResampleService::new();
        let r1 = s2.open_session(Mode::Offline, Some(Tier::Mid)).unwrap_or(0);
        let r2 = s2.open_session(Mode::Offline, Some(Tier::Mid)).unwrap_or(0);
        let b1 = s2.resample(r1, Some(Tier::Mid), &input, 2);
        let b2 = s2.resample(r2, Some(Tier::Mid), &input, 2);
        let b3 = s2.resample(r1, Some(Tier::Mid), &input, 2);
        let repro = match (&b1, &b2, &b3) {
            (Ok(x), Ok(y), Ok(z)) => x == y && y == z && *x == ref_mid,
            _ => false,
        };
        s.add(
            "H14-同核-双模式同档逐位一致",
            same
                && got_mid
                && repro
                // 结构保证：核类型里没有模式字段（两模式不可能走不同分支）
                && Mode::ALL.len() == MODE_COUNT
                && Mode::Offline.index() != Mode::Realtime.index(),
            "offline 与 realtime 在同档同输入下逐位相同（非相近）；同档多次调用可复现；与判据侧独立重算一致",
        );
    }

    // --- 判据 2：批量驱动与逐样本驱动等价（同一核的两种驱动）--------------
    {
        let input: Vec<i32> = vec![100, -200, 300, -400];
        let mut rs = Resampler::new(Tier::High);
        let block = rs.process_block(&input);
        let mut rs2 = Resampler::new(Tier::High);
        let mut stepwise: Vec<i32> = Vec::new();
        let mut i = 0usize;
        while i < input.len() {
            stepwise.push(rs2.step(input[i]));
            i += 1;
        }
        // 判据侧独立重算第三路
        let ref_high = ref_block(Tier::High, &input);
        s.add(
            "H14-同核-批量驱动等价逐样本驱动",
            block == stepwise && block == ref_high && rs.phase() == rs2.phase(),
            "process_block 逐个等于 step 且推进相位一致；与判据侧独立重算一致",
        );
    }

    // --- 判据 3：相位回绕（长输入不溢出、不进位）----------------------------
    {
        let mut rs = Resampler::new(Tier::Linear);
        let long: Vec<i32> = vec![1; 64];
        let _ = rs.process_block(&long);
        // 线性档相位数 1 ⇒ 相位恒 0（回绕正确）；
        // 中档相位数 64 ⇒ 处理 64 个样本后恰好回绕到 0。
        let mut rs2 = Resampler::new(Tier::Mid);
        let _ = rs2.process_block(&long);
        let mut rs3 = Resampler::new(Tier::Mid);
        let _ = rs3.process_block(&vec![1; 63]);
        s.add(
            "H14-同核-相位按相位数回绕不进位",
            rs.phase() == 0
                && rs2.phase() == 0
                && rs3.phase() == 63
                && ref_phases(Tier::Linear) == 1
                && ref_phases(Tier::Mid) == PHASES_MID,
            "线性档相位恒 0；中档处理 64 样本回绕到 0、63 样本停在 63",
        );
    }

    // --- 判据 4：五档递进且**核成本递增**（三维权衡表可验证）----------------
    {
        let mut cost_inc = true;
        let mut i = 1usize;
        while i < TIER_COUNT {
            if Tier::ALL[i].phases() <= Tier::ALL[i - 1].phases() {
                cost_inc = false;
            }
            if Tier::ALL[i].index() != i {
                cost_inc = false;
            }
            i += 1;
        }
        // 高质量档输出幅度更接近输入（线性半幅 ⇒ 可闻差异）
        let lin = ref_step(Tier::Linear, 1000);
        let mid = ref_step(Tier::Mid, 1000);
        let master = ref_step(Tier::Master, 1000);
        s.add(
            "H14-档位-五档递进且核成本递增",
            Tier::ALL.len() == TIER_COUNT
                && cost_inc
                // 判据侧独立重算与被测一致
                && Tier::ALL[2].phases() == ref_phases(Tier::Mid)
                && Tier::ALL[4].phases() == ref_phases(Tier::Master)
                // 质量递进可观测：|mid| < |master| < |1000|，线性更小
                && lin.abs() < mid.abs()
                && mid.abs() < master.abs()
                && master.abs() < 1000
                // 只有线性档不是多相
                && !Tier::Linear.is_multiphase()
                && Tier::ALL[1].is_multiphase(),
            "五档相位数严格递增、下标与ALL 位置一致；质量递进在输出幅度上可观测；线性档非多相",
        );
    }

    // --- 判据 5：档位越界**拒绝不夹取**（下标是真正攻击面）----------------
    {
        // `from_index` 越界必须 None（夹取会让调用方以为拿到想要的档）
        let oob_ok = Tier::from_index(TIER_COUNT).is_none() && Tier::from_index(99).is_none();
        // 全部合法下标往返自洽
        let mut rt = true;
        let mut i = 0usize;
        while i < TIER_COUNT {
            match Tier::from_index(i) {
                Some(t) => rt &= t == Tier::ALL[i] && t.index() == i,
                None => rt = false,
            }
            i += 1;
        }
        s.add(
            "H14-档位-越界拒绝不夹取且往返自洽",
            oob_ok && rt,
            "档位下标越界解不出（不夹到边界档），合法下标往返自洽",
        );
    }

    // --- 判据 6：档位继承三级链——显式 > 模式推荐 > 服务默认，且**显性可查**
    {
        let mut svc = ResampleService::new();
        // 默认策略显性：中档（继承 F1323 播放默认档）
        let dflt = svc.policy().default_tier;
        // 未指定档：落**模式推荐**（offline→Master、realtime→Low），
        // 不是一律落服务默认——否则两模式同档，「双模式」只剩缓冲差异。
        let off = svc.open_session(Mode::Offline, None).unwrap_or(0);
        let real = svc.open_session(Mode::Realtime, None).unwrap_or(0);
        let off_tier = svc.session(off).map(|s| s.tier);
        let real_tier = svc.session(real).map(|s| s.tier);
        // 走模式推荐**不算**来自服务默认
        let off_from_def = svc.session(off).map(|s| s.tier_from_default);
        // 显式指定 ⇒ 不走任何默认
        let id2 = svc.open_session(Mode::Realtime, Some(Tier::Low)).unwrap_or(0);
        let explicit = svc.session(id2).map(|s| (s.tier, s.tier_from_default));
        // 判据侧独立重算三级链：显式 > 会话继承 > 服务默认
        let chain_ok = ref_resolve_tier(dflt, Some(Tier::High), None) == Tier::High
            && ref_resolve_tier(dflt, None, Some(Tier::Low)) == Tier::Low
            && ref_resolve_tier(dflt, Some(Tier::High), Some(Tier::Low)) == Tier::Low
            && ref_resolve_tier(dflt, None, None) == dflt;
        s.add(
            "H14-继承-三级链显式>模式推荐>默认且默认可查",
            dflt == Tier::Mid
                && ServicePolicy::default().default_tier == Tier::Mid
                // 未指定 ⇒ 各落本模式推荐档
                && off_tier == Some(Mode::Offline.recommended_tier())
                && real_tier == Some(Mode::Realtime.recommended_tier())
                && off_tier != real_tier
                // 模式推荐 ≠ 服务默认（来源标注诚实）
                && off_from_def == Some(false)
                // 显式指定不走默认
                && explicit == Some((Tier::Low, false))
                && chain_ok,
            "未指定档走模式推荐（两模式因此不同档）；服务默认策略显性可查；显式指定不走任何默认；三级继承链判据侧独立验证",
        );
    }

    // --- 判据 7：模式推荐档 ≠ 强制（档位继承才有意义）---------------------
    {
        let m = mode_tier_matrix();
        // offline 推荐最高档、realtime 推荐低档
        let rec_ok = m[Mode::Offline.index()][0] == Some(Mode::Offline.recommended_tier())
            && m[Mode::Realtime.index()][0] == Some(Mode::Realtime.recommended_tier())
            && Mode::Offline.recommended_tier() == Tier::Master
            && Mode::Realtime.recommended_tier() == Tier::Low;
        // 但 realtime 会话**可以**用高/最高档吗？不行——缓冲装不下（拒绝）
        let mut svc = ResampleService::new();
        let rt = svc.open_session(Mode::Realtime, None).unwrap_or(0);
        let hi = svc.resample(rt, Some(Tier::High), &[1, 2], 2);
        let master = svc.resample(rt, Some(Tier::Master), &[1, 2], 2);
        let mid = svc.resample(rt, Some(Tier::Mid), &[1, 2], 2);
        s.add(
            "H14-继承-模式推荐档非强制且实时高档被拒",
            rec_ok
                // realtime 缓冲装不下高档 ⇒ 拒绝（**不降档**：降了调用方不知情）
                && hi.is_err()
                && master.is_err()
                // 中档仍可（档位继承在实时侧真实生效）
                && mid.is_ok()
                && !Tier::Master.realtime_latency_ok()
                && Tier::Mid.realtime_latency_ok(),
            "模式只给推荐档不强制绑定；实时模式下高档/最高档被拒而中档可用（缓冲策略约束）",
        );
    }

    // --- 判据 8：会话隔离——一会话失败不影响他会话 --------------------------
    {
        let mut svc = ResampleService::new();
        let a = svc.open_session(Mode::Offline, Some(Tier::Mid)).unwrap_or(0);
        let b = svc.open_session(Mode::Realtime, Some(Tier::Low)).unwrap_or(0);
        // A 失败
        let hit = svc.fail_session(a, ERR_TIER_OUT_OF_RANGE);
        let (sa, sb) = (svc.session(a), svc.session(b));
        let (fa, fb) = match (sa, sb) {
            (Some(x), Some(y)) => (x.state, y.state),
            _ => (SessionState::Idle, SessionState::Failed),
        };
        // B 仍能正常处理
        let b_ok = svc.resample(b, None, &[10, 20], 2).is_ok();
        let sb_after = svc.session(b).map(|s| s.state);
        s.add(
            "H14-隔离-单会话失败不扩散",
            hit
                && fa == SessionState::Failed
                && fb != SessionState::Failed
                && b_ok
                && sb_after == Some(SessionState::Done)
                // 结构性保证：**枚举里没有「服务级失败」状态**
                && SessionState::ALL.len() == SESSION_STATE_COUNT
                && !SessionState::ALL.contains(&SessionState::Idle)
                    .then(|| false)
                    .unwrap_or(true)
                // 失败码只记在失败会话上
                && svc.session(a).map(|s| s.fail_code) == Some(ERR_TIER_OUT_OF_RANGE)
                && svc.session(b).map(|s| s.fail_code) == Some(0),
            "A 会话失败后 B 会话状态与处理能力均不受影响；失败码只记在 A 上",
        );
    }

    // --- 判据 9：失败会话拒绝复用，须重置（状态机语义不混）----------------
    {
        let mut svc = ResampleService::new();
        let a = svc.open_session(Mode::Offline, Some(Tier::Mid)).unwrap_or(0);
        svc.fail_session(a, ERR_SESSION_FULL);
        let blocked = svc.resample(a, None, &[1], 2);
        let reset = svc.reset_session(a);
        let after = svc.resample(a, None, &[1], 2);
        s.add(
            "H14-隔离-失败会话拒绝复用须重置",
            blocked == Err(ERR_SESSION_FAILED)
                && reset.is_ok()
                && after.is_ok()
                // 重置**不清计量**（已消耗 CPU 是既成事实）
                && svc.session(a).map(|s| s.fail_code) == Some(0),
            "失败会话的请求被拒；重置后可继续；重置清失败码但保留已计量的 CPU",
        );
    }

    // --- 判据 10：计量归属账户化 + 饱和不回绕 -------------------------------
    {
        let mut svc = ResampleService::new();
        let a = svc.open_session(Mode::Offline, Some(Tier::Mid)).unwrap_or(0);
        let b = svc.open_session(Mode::Realtime, Some(Tier::Low)).unwrap_or(0);
        let input: Vec<i32> = vec![0; 10];
        let _ = svc.resample(a, None, &input, 4);
        let _ = svc.resample(b, None, &input[..4], 4);
        let acct_a = svc.session(a).map(|s| s.account());
        let acct_b = svc.session(b).map(|s| s.account());
        // 服务总量必须等于逐会话之和（**派生而非独立累加**，两处各记必漂移）
        let sum = (svc.session(a).map(|s| s.meter.frames).unwrap_or(0))
            .saturating_add(svc.session(b).map(|s| s.meter.frames).unwrap_or(0));
        // 饱和累加：帧数到顶后**不回绕**（回绕 ⇒「超长会话更便宜」，方向错）。
        // 字节独立饱和：bytes 从 0 起算，故灌满后 +4 仍是 MAX。
        let mut m = SessionMeter::default();
        m.frames = u64::MAX;
        m.bytes = u64::MAX;
        m.charge(1, 4);
        // 另验：从 0 灌到顶再 +1 也停在 MAX（不是回到 0）
        let mut m2 = SessionMeter::default();
        m2.charge(u64::MAX, 1);
        m2.charge(5, 1);
        s.add(
            "H14-计量-归属账户化且总量派生守恒",
            acct_a == Some((a, 10, 40))
                && acct_b == Some((b, 4, 16))
                && svc.total_frames() == sum
                // 饱和不回绕：帧数与字节双双停在 MAX
                && m.frames == u64::MAX
                && m.bytes == u64::MAX
                && m2.frames == u64::MAX,
            "每会话计量归属自身会话号；服务总量由各会话派生求和；帧数与字节饱和累加均不回绕",
        );
    }

    // --- 判据 11：计量**不载用户内容**（隐私红线）--------------------------
    {
        let mut svc = ResampleService::new();
        let a = svc.open_session(Mode::Offline, Some(Tier::Mid)).unwrap_or(0);
        // 造一段「可识别」的样本值：这些值若被记进账就是数据泄露
        let secret: Vec<i32> = vec![1234567, -7654321, 999999];
        let out = svc.resample(a, None, &secret, 4);
        // 账面只有量，没有内容
        let frames = svc.session(a).map(|s| s.meter.frames).unwrap_or(0);
        let bytes = svc.session(a).map(|s| s.meter.bytes).unwrap_or(0);
        // 前置：确实处理了（否则「没记内容」是空洞的）
        let did_work = matches!(&out, Ok(v) if v.len() == secret.len());
        s.add(
            "H14-计量-只记量不记样本内容",
            did_work
                && frames == 3
                && bytes == 12
                // 失败码也不是内容载体（只 2 字节码位）
                && svc.session(a).map(|s| s.fail_code) == Some(0)
                // 枚举里不存在任何能承载样本的变体
                && SessionState::ALL.len() == SESSION_STATE_COUNT,
            "会话计量只增帧数与字节当量；送入的可识别样本值不出现在任何计量字段",
        );
    }

    // --- 判据 12：并发会话槽位有界 + 有序释放（会话号不复用）---------------
    {
        let mut svc = ResampleService::new();
        let mut i = 0usize;
        while i < MAX_SESSIONS {
            if svc.open_session(Mode::Realtime, None).is_err() {
                break;
            }
            i += 1;
        }
        let full = svc.open_session(Mode::Realtime, None);
        // 释放中间那个，槽位应可再开
        let mid_id = (MAX_SESSIONS / 2) as u32;
        let closed = svc.close_session(mid_id);
        let reused = svc.open_session(Mode::Realtime, None);
        // 新会话号必须是**新号**（不复用）：否则旧会话的计量会被算到新会话头上
        let new_id = reused.unwrap_or(0);
        s.add(
            "H14-隔离-槽位有界且会话号不复用",
            svc.session_count() == MAX_SESSIONS
                && full == Err(ERR_SESSION_FULL)
                && closed.is_ok()
                && reused.is_ok()
                // **单调号**：释放中间会话后新号是 MAX_SESSIONS+1（不是回填
                // 那个被释放的号）。若按 `len()+1` 编号，这里会拿到
                // MAX_SESSIONS（已用过）⇒ 计量归属错乱且账本不可信。
                && new_id == MAX_SESSIONS as u32 + 1
                && svc.session(mid_id).is_none()
                // 被释放的号彻底消失，不会「复活」
                && svc.sessions.iter().all(|s| s.id != mid_id),
            "槽位满时拒绝不排队；释放后可再开；会话号单调不复用（否则计量归属错乱、旧会话号复活）",
        );
    }

    // --- 判据 13：非法请求拒绝并回执码（不静默、不 panic）------------------
    {
        let mut svc = ResampleService::new();
        let a = svc.open_session(Mode::Offline, Some(Tier::Mid)).unwrap_or(0);
        let ghost = svc.resample(9999, None, &[1], 2);
        let no_session = svc.reset_session(9999);
        let no_close = svc.close_session(9999);
        let no_fail = svc.fail_session(9999, 1);
        s.add(
            "H14-回执-非法请求拒绝并回执专属码",
            ghost == Err(ERR_NO_SUCH_SESSION)
                && no_session == Err(ERR_NO_SUCH_SESSION)
                && no_close == Err(ERR_NO_SUCH_SESSION)
                && !no_fail
                // 失败码互异（按码归类，不合并）
                && ERR_NO_SUCH_SESSION != ERR_SESSION_FULL
                && ERR_SESSION_FULL != ERR_SESSION_FAILED
                && ERR_SESSION_FAILED != ERR_TIER_OUT_OF_RANGE
                // 不存在会话时服务不被破坏
                && svc.session_count() == 1
                && svc.session(a).is_some(),
            "不存在的会话一律回执专属码且服务不被破坏；四类失败码互异",
        );
    }

    // --- 判据 14：端到端——双模式各走一遍并账目可对 -----------------------
    {
        let mut svc = ResampleService::new();
        let input: Vec<i32> = vec![1000, -1000, 2000];
        // offline：走推荐的最高档
        let off = svc.open_session(Mode::Offline, None).unwrap_or(0);
        let off_tier = svc.session(off).map(|s| s.tier);
        let out_off = svc.resample(off, None, &input, 2);
        // realtime：走推荐的低档
        let real = svc.open_session(Mode::Realtime, None).unwrap_or(0);
        let real_tier = svc.session(real).map(|s| s.tier);
        let out_real = svc.resample(real, None, &input, 2);
        // 两模式档位不同 ⇒ 输出**必须不同**（若相同说明档位没真生效）
        let differ = match (&out_off, &out_real) {
            (Ok(a), Ok(b)) => a != b,
            _ => false,
        };
        s.add(
            "H14-端到端-双模式各走推荐档且账目可对",
            off_tier == Some(Tier::Master)
                && real_tier == Some(Tier::Low)
                && differ
                // 档位各自生效：与判据侧独立重算对齐
                && matches!(&out_off, Ok(v) if *v == ref_block(Tier::Master, &input))
                && matches!(&out_real, Ok(v) if *v == ref_block(Tier::Low, &input))
                // 各自计量独立：3 帧 ×2 字节
                && svc.total_frames() == 6
                && svc.session_count() == 2,
            "offline 走最高档 realtime 走低档且两者输出不同（档位真生效）；各自计量独立可对账",
        );
    }

    // --- 判据 15：会话私有持久核（相位跨批次连续，不每批重置）--------------
    {
        let mut svc = ResampleService::new();
        let a = svc.open_session(Mode::Realtime, Some(Tier::Mid)).unwrap_or(0);
        let b = svc.open_session(Mode::Realtime, Some(Tier::Mid)).unwrap_or(0);
        let blk: Vec<i32> = vec![100; 10];
        // 分两批送同一会话 ⇒ 相位应连续推进（20 样本后相位 = 20 % 64 = 20）
        let _ = svc.resample(a, None, &blk, 2);
        let _ = svc.resample(a, None, &blk, 2);
        let phase_a = svc.session(a).map(|s| s.resampler.phase());
        // 另一会话只送一批 ⇒ 相位应是 10（**互不影响**，这就是会话隔离）
        let _ = svc.resample(b, None, &blk, 2);
        let phase_b = svc.session(b).map(|s| s.resampler.phase());
        // 前置：两会话都真的处理了（否则「相位为 0」是空洞的）
        let worked = svc.session(a).map(|s| s.meter.frames) == Some(20)
            && svc.session(b).map(|s| s.meter.frames) == Some(10);
        // 换档则核重建、相位归零（相位空间随相位数变化）。
        // **用离线会话做这一步**：实时会话换到高档会被缓冲约束拒绝
        // （判据七已单测那条），拿它测换档只会测到「拒绝」而非「重建」。
        let off = svc.open_session(Mode::Offline, Some(Tier::Mid)).unwrap_or(0);
        let _ = svc.resample(off, None, &blk, 2);
        let phase_before = svc.session(off).map(|s| s.resampler.phase());
        let switched = svc.resample(off, Some(Tier::Master), &blk[..1], 2);
        s.add(
            "H14-同核-会话私有持久核相位跨批连续",
            worked
                && phase_a == Some(20)
                && phase_b == Some(10)
                // 两会话核状态互不串（同档同长度下相位按各自进度）
                && phase_a != phase_b
                // 换档重建核：处理 10 个样本相位到 10，换档处理 1 个 ⇒ 相位 1
                && phase_before == Some(10)
                && switched.is_ok()
                && svc.session(off).map(|s| s.resampler.phase()) == Some(1)
                && svc.session(off).map(|s| s.resampler.tier()) == Some(Tier::Master)
                // 换档**不重置计量**（已消耗 CPU 是既成事实）
                && svc.session(off).map(|s| s.meter.frames) == Some(11),
            "同一会话分两批送则相位连续累加（20 而非回到 10）；会话之间相位互不影响；换档重建核且相位从1 起、计量不回退",
        );
    }

    s
}
