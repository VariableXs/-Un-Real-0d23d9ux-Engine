//! VE-F0046 · 呈现器抽象与交换链管理（VE-A 域 · A03 呈现子系统总纲 · 目标 460 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0046`
//!
//! **职责定位（锚点原文）**：A03 呈现器抽象与交换链管理——呈现子系统的总纲
//! （交换链抽象/多缓冲/同步/热切换四件+两律：呈现诚实、失败可见），交换链的
//! 跨 API 抽象（D3D12/Vulkan/Metal 呈现模型统一），交换链重建的原子事务。
//! 呈现是渲染的**最后一厘米**——这一厘米出问题前面全白干；含交换链重建的
//! **演练注入**（重建失败的常态化测试）。数据结构：四件总纲；跨 API 抽象。
//!
//! **错误路径与降级矩阵（锚点原文）**：
//!
//! - 重建失败 → **保留旧链**；
//! - 抽象违例 → **拒绝**；
//! - 撕裂 → **防护**。
//!
//! **跨批对接点**：V01 显示协商衔接——本条只管「链怎么建、怎么换、怎么诚实」，
//! 显示器本身的识别与模式协商归V01。
//!
//! **无障碍与隐私**：呈现状态读屏可达（[`PresentHub::a11y_lines`]）——中英双语
//! 逐行报**聚合计数与降级事实**，不报窗口标题等私有形态。
//!
//! ## 四件与两律（锚点核心，逐一落实）
//!
//! - **交换链抽象**（[`SwapChain`]）：交换链是不可变快照 + 显式世代号。
//!   重建产出**新世代**，旧世代在新世代present 完之前仍持有引用——
//!   直接原地改写旧链会让正在飞行中的帧引用到已失效的缓冲。
//! - **多缓冲**（[`BufferTier`]）：三/二/单档，**不承诺自适应**——自适应归
//!   F0047，本条只保证「缓冲数与描述符一致」（数量与声明不符 ⇒ 抽象违例）。
//! - **同步**（[`PresentState`] 四值）：呈现同步点与等待的**诚实报告**——
//!   「已提交」与「已上屏」是两件事，只报前者就是把没上屏说成上了。
//! - **热切换**（[`HotSwapOutcome`]）：热切换是**两段事务**（建新链 → 原子换指针），
//!   中途失败必须能回到旧链，而不是停在「一半」。
//!
//! 两律：
//!
//! - **呈现诚实**（[`PresentState`] 四值齐全）：每一帧的呈现结论必须四值齐全——
//!   提交 / 上屏 / 撕裂 / 降级，且**上屏帧号**必须单调。宁可报「未知」
//!   也不报「成功」（见 [`PresentState::Unknown`]）。
//! - **失败可见**（[`FailureLedger`]）：重建失败**必须留痕**且计数递增，
//!   静默重试等于把问题藏起来——调用方无法区分「没失败」与「失败了但没说」。
//!
//! ## 与相邻条的分工（易混，故写明）
//!
//! - **F0047（多缓冲策略自适应）管「缓冲数怎么自动选」**，本条管「选定后
//!   缓冲数与描述符必须一致、重建必须原子」。F0047 的输出是本条的输入。
//! - **F0048（垂直同步与邮箱模式）管「同步的具体机制（VSync 档位/邮箱）」**，
//!   本条只管「提交与上屏必须分别记账」这一层诚实性，不选机制。
//! - **F0050（丢弃帧检测）管「丢帧归因」**，本条管「本条自己发出的呈现
//!   结论是否诚实」，不替F0050 归因。
//! - **V01（显示协商）管「显示器认不认、模式配不配」**，本条管「链建成后
//!   怎么换、换失败怎么办」。
//!
//! ## 设计要点
//!
//! - **重建是原子事务，不是逐字段赋值**：三段（校验新描述符 → 建候选链 →
//!   原子换指针），任一段失败都**不动现役链**，且失败必进
//!   [`FailureLedger`]。分不清「换了但换坏了」与「没换」是最常见的现场故障。
//! - **演练注入常态化**（[`FaultInjector`]）：重建失败是**常态**而非异常，
//!   因此失败路径必须有常态化注入点，且注入的失败**不许被吞掉**
//!   （注入后仍返回成功 ⇒ 演练本身失效，比不演练更坏）。
//! - **抽象违例一律拒绝**（[`CODE_BUFFER_MISMATCH`] / [`CODE_NO_CHAIN`]/ 
//!   [`CODE_UNSUPPORTED_MODE`]）：缓冲数与描述符不符、世代号倒退、未建链就
//!   present、后端做不到的模式——都拒绝，不「尽力而为」。跨 API 的违例
//!   （Metal 没有 mailbox）尤其不能降级：降级会让调用方按 mailbox 的延迟
//!   预算排帧，拿到 fifo 的行为却收到「已用 mailbox」的确认。
//! - **零 panic 面**：所有下标访问走 [`SwapChain::buffer`] 的 `Option` 返回，
//!   失败路径以枚举携带原因码，不 `unwrap`。
//!
//! ## 跨 API 统一（D3D12 / Vulkan / Metal）
//!
//! 锚点明文要求「交换链的跨 API 抽象（D3D12/Vulkan/Metal 呈现模型统一）」，
//! 故本条把它落成可被检查的结构而非一句注释：
//!
//! - **能力分级而非支持/不支持bool**（[`CapTier`]/[`Caps`]）：Vulkan 只有
//!   fifo 是规范**必选**，mailbox/immediate 均为扩展**可选**；D3D12 的
//!   immediate 走 independent flip 同样可选；Metal 只有 fifo。拍平成一个
//!   bool 就丢掉了「这一档在**这台机器**上保证可用吗」这个唯一有用户意义的
//!   信息。判据穷举 3×3 全组合。
//! - **静态能力与查询能力必须分开**（[`CapSource`]）：Vulkan 的
//!   `maxBuffers` 来自 surface **查询**，不是编译期常量。拿抽象层写死的兜底
//!   值当上限去校验，等于替 Vulkan 撒了个谎——真机 surface 只给 2 时，
//!   3 缓冲的请求会被「通过」然后在驱动里失败。故查询源**缺查询值即拒**
//!   （[`CODE_CAP_UNQUERIED`]），且查询值真参与上界裁决（夹逼对判据）。
//! - **必选 mailbox 需三缓冲是语义要求**（[`CrossApi::unify`] 规则 3）：
//!   两缓冲下邮箱退化为保序，拿不到「不撕裂且低延迟」同时成立。把它与
//!   各家能力下界混为一谈，会放行「D3D12 必选 mailbox + 双缓冲」这条
//!   不成立的计划。
//! - **归一层在出货路径上**（[`CrossApi::to_desc`] → [`PresentHub::rebuild`]）：
//!   这是两节唯一的接缝。少了它，归一器可以判据全绿而现役链其实由别处
//!   建出——正是「判据覆盖不到出货路径」那种弱门禁。
//!
//! ## 性能逐项分解
//!
//! O(交换链) —— [`PresentHub::present`]对每帧做 O(1) 判定；
//! [`PresentHub::rebuild`] 是 O(缓冲数) 的一次性事务。
//! 失败账本为定容环形，不随重建次数增长。
//!
//! 两条各自**自持**定义类型，不跨模块 `use`——并行提交时跨模块引用会把两个
//! 模块的编译成败绑在一起，一方半成品就拖垮另一方，而这类失败报 E0583，
//! 与真实缺陷长得一样、极难分辨。

use crate::checks::CheckSet;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量
// ---------------------------------------------------------------------------

/// 交换链允许的缓冲档位（单/双/三）。
pub const BUFFER_TIERS: usize = 3;

/// 单缓冲（无交换，可能撕裂）。
pub const BUFFERS_SINGLE: u8 = 1;

/// 双缓冲。
pub const BUFFERS_DOUBLE: u8 = 2;

/// 三缓冲（本条默认档）。
pub const BUFFERS_TRIPLE: u8 = 3;

/// 描述符允许的最小宽（像素）。
pub const MIN_WIDTH: u32 = 2;

/// 描述符允许的最小高（像素）。
pub const MIN_HEIGHT: u32 = 2;

/// 描述符允许的最大宽（像素）——超过即视为不可能配置，拒绝而非截断。
pub const MAX_WIDTH: u32 = 16384;

/// 描述符允许的最大高（像素）。
pub const MAX_HEIGHT: u32 = 16384;

/// 单条链允许的最大缓冲数（超过即抽象违例：驱动侧无此资源）。
pub const MAX_BUFFERS: u8 = 8;

/// 失败账本定容（只关心最近若干次，不存全量历史）。
pub const FAILURE_WINDOW: usize = 32;

/// 重建事务的段数（校验 / 建链/ 换指针）。段数是定值，改它须改判据。
pub const REBUILD_STAGES: usize = 3;

/// 呈现失败原因码（自建诊断码；下游封闭枚举无权加变体）。
pub type PresentCode = u16;

/// 码段: 重建描述符非法。
pub const CODE_BAD_DESCRIPTOR: PresentCode = 0x4601;

/// 码段: 缓冲数与描述符不符（抽象违例）。
pub const CODE_BUFFER_MISMATCH: PresentCode = 0x4602;

/// 码段: 演练注入的重建失败。
pub const CODE_INJECTED_FAULT: PresentCode = 0x4603;

/// 码段: 未建链即呈现。
pub const CODE_NO_CHAIN: PresentCode = 0x4604;

/// 码段: 世代号倒退。
pub const CODE_GENERATION_BACKWARD: PresentCode = 0x4605;

/// 码段: 热切换事务中途失败。
pub const CODE_HOTSWAP_ABORTED: PresentCode = 0x4606;

/// 码段: 旧世代仍被引用即销毁（飞帧仍在用）。
pub const CODE_STILL_REFERENCED: PresentCode = 0x4607;

/// 码段: 上屏帧号未单调。
pub const CODE_PRESENT_NOT_MONOTONIC: PresentCode = 0x4608;

/// 码段: 面板行数与实际不符。
pub const CODE_PANEL_SHAPE: PresentCode = 0x4609;

/// 码段: 已上屏帧号越界。
pub const CODE_ONSCREEN_RANGE: PresentCode = 0x460A;

// ---------------------------------------------------------------------------
// 二、几何与描述符
// ---------------------------------------------------------------------------

/// 交换链描述符（重建的唯一输入）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChainDesc {
    pub width: u32,
    pub height: u32,
    pub buffers: u8,
    /// 是否允许撕裂（仅在无同步能力的链上生效）。
    pub allow_tearing: bool,
}

impl ChainDesc {
    pub const fn new(width: u32, height: u32, buffers: u8, allow_tearing: bool) -> Self {
        ChainDesc { width, height, buffers, allow_tearing }
    }

    /// 描述符自检：几何与缓冲档位是否落在规格内。
    ///
    /// **不做静默截断**——把 99999×1 截成 16384×1 会让调用方以为
    /// 请求的分辨率生效了，而实际拿到的是另一个尺寸。
    pub const fn validate(&self) -> Result<(), PresentCode> {
        if self.width < MIN_WIDTH || self.width > MAX_WIDTH {
            return Err(CODE_BAD_DESCRIPTOR);
        }
        if self.height < MIN_HEIGHT || self.height > MAX_HEIGHT {
            return Err(CODE_BAD_DESCRIPTOR);
        }
        if self.buffers == 0 || self.buffers > MAX_BUFFERS {
            return Err(CODE_BAD_DESCRIPTOR);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 三、四件之一：交换链抽象
// ---------------------------------------------------------------------------

/// 交换链：不可变快照 + 显式世代号。
///
/// 世代号是**单调**的：重建产出新世代，旧世代保留到引用归零。
/// 原地改写旧链会让正在飞行中的帧引用到已失效的缓冲——
/// 这是「重建后随机花屏/黑屏」最常见的成因。
#[derive(Clone, Debug)]
pub struct SwapChain {
    desc: ChainDesc,
    generation: u32,
    /// 各缓冲的引用计数（0 = 无人持有）。
    refs: [u8; MAX_BUFFERS as usize],
    /// 已上屏帧号（该链上最近一次成功上屏的帧号）。
    presented_frame: u64,
    /// 该链是否已销毁。
    destroyed: bool,
}

impl SwapChain {
    /// 新建一条链。**调用方须先 `validate`** ——本函数不做校验，
    /// 校验在事务第一段统一做（避免两处口径不一致）。
    pub fn new(desc: ChainDesc, generation: u32) -> SwapChain {
        SwapChain {
            desc,
            generation,
            refs: [0u8; MAX_BUFFERS as usize],
            presented_frame: 0,
            destroyed: false,
        }
    }

    pub fn desc(&self) -> ChainDesc {
        self.desc
    }

    pub fn generation(&self) -> u32 {
        self.generation
    }

    pub fn buffer_count(&self) -> u8 {
        self.desc.buffers
    }

    pub fn destroyed(&self) -> bool {
        self.destroyed
    }

    pub fn presented_frame(&self) -> u64 {
        self.presented_frame
    }

    /// 取第 `idx` 号缓冲的引用计数。越界返回 `None`（零 panic 面）。
    pub fn buffer(&self, idx: u8) -> Option<u8> {
        if idx < self.desc.buffers {
            Some(self.refs[idx as usize])
        } else {
            None
        }
    }

    /// 持有一个缓冲（飞帧开始）。
    pub fn acquire(&mut self, idx: u8) -> Result<(), PresentCode> {
        if self.destroyed {
            return Err(CODE_NO_CHAIN);
        }
        if idx >= self.desc.buffers {
            return Err(CODE_BUFFER_MISMATCH);
        }
        let slot = &mut self.refs[idx as usize];
        *slot = slot.saturating_add(1);
        Ok(())
    }

    /// 归还一个缓冲（飞帧结束）。
    pub fn release(&mut self, idx: u8) -> Result<(), PresentCode> {
        if idx >= self.desc.buffers {
            return Err(CODE_BUFFER_MISMATCH);
        }
        let slot = &mut self.refs[idx as usize];
        if *slot == 0 {
            return Err(CODE_BUFFER_MISMATCH);
        }
        *slot -= 1;
        Ok(())
    }

    /// 本链当前被持有的缓冲数。
    pub fn held(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.desc.buffers as usize {
            if self.refs[i] > 0 {
                n = n.saturating_add(1);
            }
            i += 1;
        }
        n
    }

    /// 记录一次成功上屏。**帧号必须单调递增**——不上屏而帧号前进，
    /// 等于把没画出来的帧算成画出来了（呈现不诚实的典型形态）。
    pub fn mark_presented(&mut self, frame: u64) -> Result<(), PresentCode> {
        if frame <= self.presented_frame {
            return Err(CODE_PRESENT_NOT_MONOTONIC);
        }
        self.presented_frame = frame;
        Ok(())
    }

    /// 销毁本链。**仍有缓冲被持有时拒绝**——飞帧还在用，销毁即花屏。
    pub fn destroy(&mut self) -> Result<(), PresentCode> {
        if self.held() > 0 {
            return Err(CODE_STILL_REFERENCED);
        }
        if self.destroyed {
            return Err(CODE_NO_CHAIN);
        }
        self.destroyed = true;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 四、四件之二：多缓冲（一致性侧）
// ---------------------------------------------------------------------------

/// 缓冲档位。**本条不做自适应**——自适应归 F0047。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BufferTier {
    Single,
    Double,
    Triple,
}

impl BufferTier {
    pub const ALL: [BufferTier; BUFFER_TIERS] =
        [BufferTier::Single, BufferTier::Double, BufferTier::Triple];

    pub const fn count(self) -> u8 {
        match self {
            BufferTier::Single => BUFFERS_SINGLE,
            BufferTier::Double => BUFFERS_DOUBLE,
            BufferTier::Triple => BUFFERS_TRIPLE,
        }
    }

    pub const fn ordinal(self) -> usize {
        match self {
            BufferTier::Single => 0,
            BufferTier::Double => 1,
            BufferTier::Triple => 2,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            BufferTier::Single => "单缓冲 / single buffered",
            BufferTier::Double => "双缓冲 / double buffered",
            BufferTier::Triple => "三缓冲 / triple buffered",
        }
    }

    /// 按数量反查档位。数量不属三档 ⇒ `None`（不静默归到最近档）。
    pub const fn from_count(n: u8) -> Option<BufferTier> {
        if n == BUFFERS_SINGLE {
            Some(BufferTier::Single)
        } else if n == BUFFERS_DOUBLE {
            Some(BufferTier::Double)
        } else if n == BUFFERS_TRIPLE {
            Some(BufferTier::Triple)
        } else {
            None
        }
    }
}

// ---------------------------------------------------------------------------
// 五、四件之三：同步（诚实侧）
// ---------------------------------------------------------------------------

/// 一帧的呈现结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PresentState {
    /// 已提交但未上屏（等Vsync 或等合成器）。
    Submitted,
    /// 已上屏，且给出了上屏帧号。
    OnScreen,
    /// 已提交且伴随撕裂。
    Tore,
    /// **不知道**——宁可报未知也不报成功。
    Unknown,
}

impl PresentState {
    pub const ALL: [PresentState; 4] =
        [PresentState::Submitted, PresentState::OnScreen, PresentState::Tore, PresentState::Unknown];

    pub const fn ordinal(self) -> usize {
        match self {
            PresentState::Submitted => 0,
            PresentState::OnScreen => 1,
            PresentState::Tore => 2,
            PresentState::Unknown => 3,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            PresentState::Submitted => "已提交未上屏 / submitted not on screen",
            PresentState::OnScreen => "已上屏 / on screen",
            PresentState::Tore => "撕裂 / torn",
            PresentState::Unknown => "未知 / unknown",
        }
    }

    /// 是否算「真的让用户看到了」。
    pub const fn is_visible(self) -> bool {
        matches!(self, PresentState::OnScreen)
    }

    /// 线编码往返。
    pub const fn wire(self) -> u8 {
        self.ordinal() as u8
    }

    pub const fn from_wire(w: u8) -> Option<PresentState> {
        if (w as usize) < 4 {
            Some(PresentState::ALL[w as usize])
        } else {
            None
        }
    }
}

/// 四值齐全性检查（呈现诚实的第一道闸）。
///
/// 四值缺一即不诚实：报「成功」却没给上屏帧号、把撕裂当成功、
/// 把未知当成功——都算破律。
pub const fn honesty_complete(s: PresentState, frame: u64) -> bool {
    match s {
        PresentState::OnScreen => frame > 0,
        PresentState::Submitted => frame == 0,
        PresentState::Tore => frame > 0,
        // 「未知」允许带任何帧号：它本就是不承诺任何事。
        PresentState::Unknown => true,
    }
}

// ---------------------------------------------------------------------------
// 六、四件之四：热切换（两段事务）
// ---------------------------------------------------------------------------

/// 热切换结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HotSwapOutcome {
    /// 已切换到新链。
    Swapped,
    /// 失败并**已回到旧链**（降级成功）。
    RolledBack,
    /// 失败且旧链也不可用（最坏档）。
    Failed,
}

impl HotSwapOutcome {
    pub const ALL: [HotSwapOutcome; 3] =
        [HotSwapOutcome::Swapped, HotSwapOutcome::RolledBack, HotSwapOutcome::Failed];

    pub const fn ordinal(self) -> usize {
        match self {
            HotSwapOutcome::Swapped => 0,
            HotSwapOutcome::RolledBack => 1,
            HotSwapOutcome::Failed => 2,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            HotSwapOutcome::Swapped => "已切换 / swapped",
            HotSwapOutcome::RolledBack => "失败已回旧链 / rolled back",
            HotSwapOutcome::Failed => "失败且旧链不可用 / failed",
        }
    }
}

// ---------------------------------------------------------------------------
// 七、演练注入
// ---------------------------------------------------------------------------

/// 重建失败的常态化注入点。
///
/// 重建失败是**常态**不是异常（驱动升级、显示器热插拔、模式切换都会触发），
/// 因此失败路径必须有常态化注入点，否则失败路径只有等真故障才被走过一次。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaultInjector {
    /// 不注入（走真实路径）。
    None,
    /// 在「校验段」注入。
    AtValidate,
    /// 在「建链段」注入。
    AtBuild,
    /// 在「换指针段」注入。
    AtSwap,
}

impl FaultInjector {
    pub const ALL: [FaultInjector; 4] = [
        FaultInjector::None,
        FaultInjector::AtValidate,
        FaultInjector::AtBuild,
        FaultInjector::AtSwap,
    ];

    pub const fn ordinal(self) -> usize {
        match self {
            FaultInjector::None => 0,
            FaultInjector::AtValidate => 1,
            FaultInjector::AtBuild => 2,
            FaultInjector::AtSwap => 3,
        }
    }

    /// 注入点是否命中某段（段号 0/1/2，`None` 永不命中）。
    ///
    /// 注意：这里用 `ordinal() == 0` 判「无注入」而**不写** `self == FaultInjector::None`
    /// ——`PartialEq` 的 `==` 是**非 const 运算符**，在 `const fn` 里直接编译失败
    /// （E0015）。`ordinal()` 是 `const fn` 且 `None.ordinal()` 恒为 0，等价。
    pub const fn hits(self, stage: usize) -> bool {
        if self.ordinal() == 0 {
            return false;
        }
        // 注入点 1/2/3 对应段 0/1/2。
        self.ordinal() == stage + 1
    }
}

// ---------------------------------------------------------------------------
// 八、失败账本（失败可见）
// ---------------------------------------------------------------------------

/// 失败留痕：环形定容，只记最近 [`FAILURE_WINDOW`] 次。
#[derive(Clone, Debug)]
pub struct FailureLedger {
    /// 每笔的代码。
    codes: [u16; FAILURE_WINDOW],
    /// 每笔是否来自演练注入（真实故障与演练必须可区分）。
    injected: [bool; FAILURE_WINDOW],
    count: usize,
    /// 累计失败笔数（**不减**，含被环形淘汰的）。
    pub total: u32,
    /// 累计演练注入笔数。
    pub injected_total: u32,
    head: usize,
}

impl Default for FailureLedger {
    fn default() -> Self {
        FailureLedger::new()
    }
}

impl FailureLedger {
    pub const fn new() -> FailureLedger {
        FailureLedger {
            codes: [0u16; FAILURE_WINDOW],
            injected: [false; FAILURE_WINDOW],
            count: 0,
            total: 0,
            injected_total: 0,
            head: 0,
        }
    }

    /// 记一笔失败。**返回值恒为 `false`** 以便调用点写成
    /// `if hub.record(...) { ... }` 时形似可疑——诚实起见不提供「记成功」路径。
    pub fn record(&mut self, code: PresentCode, injected: bool) -> bool {
        self.codes[self.head] = code;
        self.injected[self.head] = injected;
        self.head += 1;
        if self.head == FAILURE_WINDOW {
            self.head = 0;
        }
        if self.count < FAILURE_WINDOW {
            self.count += 1;
        }
        self.total = self.total.saturating_add(1);
        if injected {
            self.injected_total = self.injected_total.saturating_add(1);
        }
        false
    }

    /// 窗口内保留的失败笔数（封顶 [`FAILURE_WINDOW`]）。
    pub fn retained(&self) -> usize {
        self.count
    }

    /// 第 `i` 笔（按写入顺序，0 = 最近 [`FAILURE_WINDOW`] 范围内最早）的代码。
    pub fn code_at(&self, i: usize) -> Option<u16> {
        if i < self.count {
            Some(self.codes[i])
        } else {
            None
        }
    }

    /// 第 `i` 笔是否来自演练注入。
    pub fn injected_at(&self, i: usize) -> Option<bool> {
        if i < self.count {
            Some(self.injected[i])
        } else {
            None
        }
    }

    /// 窗口内**真实**故障（非演练）笔数。
    pub fn real_failures(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.count {
            if !self.injected[i] {
                n = n.saturating_add(1);
            }
            i += 1;
        }
        n
    }
}

// ---------------------------------------------------------------------------
// 九、呈现中枢
// ---------------------------------------------------------------------------

/// 呈现器抽象与交换链管理中枢。
#[derive(Clone, Debug)]
pub struct PresentHub {
    /// 现役链。`None` = 尚未建链（此时 present 必拒）。
    active: Option<SwapChain>,
    /// 世代计数（每次成功重建 +1）。
    next_generation: u32,
    failures: FailureLedger,
    /// 成功重建笔数。
    pub rebuilds_ok: u32,
    /// 已提交帧数。
    pub submitted: u64,
    /// 已上屏帧数。
    pub onscreen: u64,
    /// 撕裂帧数。
    pub torn: u64,
    /// 热切换成功笔数。
    pub swaps_ok: u32,
    /// 热切换失败但已回旧链的笔数。
    pub swaps_rolled_back: u32,
    /// 当前演练注入档。
    injector: FaultInjector,
    /// 读屏面板。
    panel: Vec<String>,
}

impl Default for PresentHub {
    fn default() -> Self {
        PresentHub::new()
    }
}

impl PresentHub {
    pub const fn new() -> PresentHub {
        PresentHub {
            active: None,
            next_generation: 1,
            failures: FailureLedger::new(),
            rebuilds_ok: 0,
            submitted: 0,
            onscreen: 0,
            torn: 0,
            swaps_ok: 0,
            swaps_rolled_back: 0,
            injector: FaultInjector::None,
            panel: Vec::new(),
        }
    }

    pub fn set_injector(&mut self, f: FaultInjector) {
        self.injector = f;
    }

    pub fn injector(&self) -> FaultInjector {
        self.injector
    }

    /// 现役链世代号（未建链 ⇒ 0）。
    pub fn active_generation(&self) -> u32 {
        match &self.active {
            Some(c) => c.generation(),
            None => 0,
        }
    }

    /// 现役链缓冲数（未建链 ⇒ 0）。
    pub fn active_buffers(&self) -> u8 {
        match &self.active {
            Some(c) => c.buffer_count(),
            None => 0,
        }
    }

    pub fn failures(&self) -> &FailureLedger {
        &self.failures
    }

    /// 交换链重建的**原子事务**，三段：`校验 → 建链 → 换指针`。
    ///
    /// 任一段失败：**不动现役链**、失败进账本、返回错误码。
    /// 「重建失败→保留旧链」是锚点明文要求——保住旧链是降级的前提。
    pub fn rebuild(&mut self, desc: ChainDesc) -> Result<u32, PresentCode> {
        // 段 0：校验（注入点可命中）
        if self.injector.hits(0) {
            self.failures.record(CODE_INJECTED_FAULT, true);
            return Err(CODE_INJECTED_FAULT);
        }
        if desc.validate().is_err() {
            self.failures.record(CODE_BAD_DESCRIPTOR, false);
            return Err(CODE_BAD_DESCRIPTOR);
        }
        // 抽象违例：缓冲数与声明不符（档位外的数量一律视为违例）
        if BufferTier::from_count(desc.buffers).is_none() {
            self.failures.record(CODE_BUFFER_MISMATCH, false);
            return Err(CODE_BUFFER_MISMATCH);
        }

        // 段 1：建候选链（注入点可命中）
        if self.injector.hits(1) {
            self.failures.record(CODE_INJECTED_FAULT, true);
            return Err(CODE_INJECTED_FAULT);
        }
        let generation = self.next_generation;
        let candidate = SwapChain::new(desc, generation);

        // 段 2：原子换指针（注入点可命中）
        if self.injector.hits(2) {
            self.failures.record(CODE_INJECTED_FAULT, true);
            return Err(CODE_INJECTED_FAULT);
        }
        // 候选链必须自洽（缓冲数与描述符一致）——不检查就是「建了不知道建成什么」
        if candidate.buffer_count() != desc.buffers || BufferTier::from_count(candidate.buffer_count()).is_none() {
            self.failures.record(CODE_BUFFER_MISMATCH, false);
            return Err(CODE_BUFFER_MISMATCH);
        }
        // 换指针在此发生——之前任何失败都不曾动过现役链。
        // 世代号单调：next_generation 只在成功换指针后 +1。
        self.active = Some(candidate);
        self.next_generation = self.next_generation.saturating_add(1);
        self.rebuilds_ok = self.rebuilds_ok.saturating_add(1);
        self.panel.push(format!("重建成功 / rebuild ok: gen {}", generation));
        Ok(generation)
    }

    /// 热切换：两段事务（建新链 → 原子换指针），失败**必回旧链**。
    pub fn hot_swap(&mut self, desc: ChainDesc) -> HotSwapOutcome {
        let before = self.active_generation();
        match self.rebuild(desc) {
            Ok(_) => {
                self.swaps_ok = self.swaps_ok.saturating_add(1);
                HotSwapOutcome::Swapped
            }
            Err(code) => {
                // 现役链未被 rebuild 动过（失败必在换指针前返回）⇒ 回旧链即恢复。
                let after = self.active_generation();
                if after == before && self.active.is_some() {
                    self.failures.record(CODE_HOTSWAP_ABORTED, code == CODE_INJECTED_FAULT);
                    self.swaps_rolled_back = self.swaps_rolled_back.saturating_add(1);
                    self.panel.push(format!("热切换回旧链 / hot swap rolled back: gen {}", after));
                    HotSwapOutcome::RolledBack
                } else {
                    self.failures.record(CODE_HOTSWAP_ABORTED, code == CODE_INJECTED_FAULT);
                    HotSwapOutcome::Failed
                }
            }
        }
    }

    /// 呈现一帧。
    ///
    /// **已提交 ≠ 已上屏**：未建链一律拒（记账），其余按 `onscreen` 决策
    /// 给出四值之一的结论，并让上屏帧号单调。
    pub fn present(&mut self, frame: u64, want_tearing: bool) -> Result<PresentState, PresentCode> {
        let can_tear = match &self.active {
            Some(c) => c.desc().allow_tearing,
            None => {
                self.failures.record(CODE_NO_CHAIN, false);
                self.panel.push(format!("呈现被拒 / present rejected: {}", CODE_NO_CHAIN));
                return Err(CODE_NO_CHAIN);
            }
        };
        let buffers = self.active.as_ref().map(|c| c.buffer_count()).unwrap_or(0);
        // 缓冲数与档位必须一致（抽象违例）——单缓冲链不能声称做了三重缓冲
        if BufferTier::from_count(buffers).is_none() {
            self.failures.record(CODE_BUFFER_MISMATCH, false);
            self.panel.push(format!("呈现被拒 / present rejected: {}", CODE_BUFFER_MISMATCH));
            return Err(CODE_BUFFER_MISMATCH);
        }

        self.submitted = self.submitted.saturating_add(1);
        // 撕裂**只在允许撕裂且显式请求时**发生；不允许撕裂却 tore 是抽象违例。
        let state = if want_tearing && can_tear {
            self.torn = self.torn.saturating_add(1);
            PresentState::Tore
        } else {
            PresentState::OnScreen
        };

        if state.is_visible() || state == PresentState::Tore {
            if let Some(c) = &mut self.active {
                if c.mark_presented(frame).is_ok() {
                    self.onscreen = self.onscreen.saturating_add(1);
                } else {
                    // 帧号未单调 ⇒ 诚实降级为「已提交未上屏」而不是硬报成功
                    self.failures.record(CODE_PRESENT_NOT_MONOTONIC, false);
                    self.panel.push(format!(
                        "上屏帧号未单调 / present not monotonic: {}",
                        CODE_PRESENT_NOT_MONOTONIC
                    ));
                    return Ok(PresentState::Submitted);
                }
            }
        }
        self.panel.push(format!("呈现 / present: {}", state.label()));
        Ok(state)
    }

    /// 上屏帧号必须单调（跨越全部present 调用）。
    pub fn onscreen_frames(&self) -> u64 {
        self.onscreen
    }

    /// 提交帧数（**含**未上屏的）。
    pub fn submitted_frames(&self) -> u64 {
        self.submitted
    }

    pub fn torn_frames(&self) -> u64 {
        self.torn
    }

    pub fn rebuilds_ok(&self) -> u32 {
        self.rebuilds_ok
    }

    /// 热切换成功笔数。
    pub fn swaps_ok(&self) -> u32 {
        self.swaps_ok
    }

    /// 热切换失败但已回旧链的笔数。
    pub fn swaps_rolled_back(&self) -> u32 {
        self.swaps_rolled_back
    }

    /// 读屏面板（七行双语，只报聚合计数与降级事实）。
    pub fn a11y_lines(&self) -> [String; 7] {
        [
            format!("交换链世代 / swapchain generation: {}", self.active_generation()),
            format!("缓冲数 / buffer count: {}", self.active_buffers()),
            format!("提交帧数 / submitted frames: {}", self.submitted),
            format!("上屏帧数 / onscreen frames: {}", self.onscreen),
            format!("撕裂帧数 / torn frames: {}", self.torn),
            format!(
                "重建失败 / rebuild failures: {}（演练 {}）",
                self.failures.total, self.failures.injected_total
            ),
            format!(
                "重建成功 / rebuilds ok: {}（热切换 {}，回旧链 {}）",
                self.rebuilds_ok, self.swaps_ok, self.swaps_rolled_back
            ),
        ]
    }
}

// ---------------------------------------------------------------------------
// 十、跨 API 呈现模型统一（D3D12 / Vulkan / Metal）
// ---------------------------------------------------------------------------
//
// 锚点明文：「交换链的**跨 API 抽象**（D3D12/Vulkan/Metal 呈现模型统一）」。
// 上面的 [`PresentHub`] 管「链怎么建、怎么换」，本节管「三家后端的呈现语义
// 怎么归一」——两者是同一件事实的两面：归一后的计划仍要交给 [`PresentHub::rebuild`]
// 才能变成现役链，故本节不是旁支，而是重建事务的**输入侧**。
//
// ## 为什么「统一」不等于「取交集」
//
// 三家的呈现模型差异是**结构性**的，不是版本差异：
//
// - **D3D12**：flip 模型的 fifo 与 mailbox 由 DXGI 保证；immediate 走
//   independent flip，属**可选**能力。图像 2..=16（flip 模型要求 ≥2）。
// - **Vulkan**：`VK_KHR_present_mode_fifo` 是规范**唯一必选**模式，mailbox 与
//   immediate 都由扩展**可选**提供；`maxImageCount` 来自 surface **查询**，
//   **不是编译期常量**。图像 1..=8（抽象层兜底上界）。
// - **Metal**：只有 CAMetalLayer 的 vsync 呈现（fifo），mailbox/immediate
//   **都拿不到**；三缓冲由系统隐式管理。图像 2..=3。
//
// 把三者拍平成一个「支持/不支持」bool，就丢掉了唯一有用户意义的信息：
// 「这一档在**这台机器**上保证可用吗」。故 [`CapTier`] 分必选/可选两档，
// 判据穷举全部 3×3 组合。

/// 跨 API 统一的呈现模式档。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncMode {
    /// 保序呈现（等垂直同步）。
    Fifo,
    /// 邮箱（低延迟且不撕裂，非必选）。
    Mailbox,
    /// 立即呈现（最低延迟，允许撕裂，非必选）。
    Immediate,
}

impl SyncMode {
    pub const ALL: [SyncMode; MODE_TIERS] = [SyncMode::Fifo, SyncMode::Mailbox, SyncMode::Immediate];

    pub const fn ordinal(self) -> usize {
        match self {
            SyncMode::Fifo => 0,
            SyncMode::Mailbox => 1,
            SyncMode::Immediate => 2,
        }
    }

    pub const fn wire(self) -> u8 {
        self.ordinal() as u8
    }

    pub const fn from_wire(w: u8) -> Option<SyncMode> {
        if (w as usize) < MODE_TIERS {
            Some(SyncMode::ALL[w as usize])
        } else {
            None
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            SyncMode::Fifo => "保序 / fifo",
            SyncMode::Mailbox => "邮箱 / mailbox",
            SyncMode::Immediate => "立即 / immediate",
        }
    }

    /// 本档要求的最小缓冲数（**必选集**保证的下限）。
    ///
    /// - fifo：三家都保证，双缓冲即可（交换链的基本前提）。
    /// - mailbox：需要**三缓冲**才有意义——两缓冲下邮箱退化为保序，
    ///   拿不到「不撕裂且低延迟」这两条同时成立。
    /// - immediate：双缓冲即可（它就是靠撕裂换延迟）。
    pub const fn min_buffers(self) -> u8 {
        match self {
            SyncMode::Fifo => BUFFERS_DOUBLE,
            SyncMode::Mailbox => BUFFERS_TRIPLE,
            SyncMode::Immediate => BUFFERS_DOUBLE,
        }
    }
}

/// 呈现模式档数。
pub const MODE_TIERS: usize = 3;

/// 跨 API 统一后的后端。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PresentApi {
    D3D12,
    Vulkan,
    Metal,
}

impl PresentApi {
    pub const ALL: [PresentApi; API_TIERS] = [PresentApi::D3D12, PresentApi::Vulkan, PresentApi::Metal];

    pub const fn ordinal(self) -> usize {
        match self {
            PresentApi::D3D12 => 0,
            PresentApi::Vulkan => 1,
            PresentApi::Metal => 2,
        }
    }

    pub const fn wire(self) -> u8 {
        self.ordinal() as u8
    }

    pub const fn from_wire(w: u8) -> Option<PresentApi> {
        if (w as usize) < API_TIERS {
            Some(PresentApi::ALL[w as usize])
        } else {
            None
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            PresentApi::D3D12 => "D3D12",
            PresentApi::Vulkan => "Vulkan",
            PresentApi::Metal => "Metal",
        }
    }
}

/// 后端家数。
pub const API_TIERS: usize = 3;

/// 模式位集（低三位，每位对应一档）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModeMask(u8);

impl ModeMask {
    pub const NONE: ModeMask = ModeMask(0b000);
    pub const ALL: ModeMask = ModeMask(0b111);
    pub const FIFO: ModeMask = ModeMask(0b001);
    pub const MAILBOX: ModeMask = ModeMask(0b010);
    pub const IMMEDIATE: ModeMask = ModeMask(0b100);

    /// 全部 8 个位集（判据穷举用）。
    pub const ALL_MASKS: [ModeMask; 8] = [
        ModeMask(0b000),
        ModeMask(0b001),
        ModeMask(0b010),
        ModeMask(0b011),
        ModeMask(0b100),
        ModeMask(0b101),
        ModeMask(0b110),
        ModeMask(0b111),
    ];

    pub const fn of(m: SyncMode) -> ModeMask {
        ModeMask(1u8 << m.ordinal())
    }

    pub const fn bits(self) -> u8 {
        self.0
    }

    pub const fn from_bits(b: u8) -> ModeMask {
        ModeMask(b & 0b111)
    }

    pub const fn union(self, other: ModeMask) -> ModeMask {
        ModeMask(self.0 | other.0)
    }

    pub const fn contains(self, m: SyncMode) -> bool {
        self.0 & ModeMask::of(m).0 != 0
    }

    pub const fn count(self) -> u32 {
        self.0.count_ones()
    }
}

/// 能力档位：必选保证 vs 扩展可选。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapTier {
    /// 该后端**保证**支持。
    Required,
    /// 该后端**可能**支持（扩展提供，真机上仍可能没有）。
    Optional,
}

impl CapTier {
    pub const fn ordinal(self) -> usize {
        match self {
            CapTier::Required => 0,
            CapTier::Optional => 1,
        }
    }

    /// 是否必选档。
    ///
    /// 刻意**不写** `self == CapTier::Required`——`PartialEq` 的 `==` 是非 const
    /// 运算符，在 `const fn` 里直接编译失败（E0015）。`ordinal()` 是 `const fn`
    /// 且 `Required.ordinal()` 恒为 0，等价。同 [`FaultInjector::hits`] 的处理。
    pub const fn is_required(self) -> bool {
        self.ordinal() == 0
    }

    pub const fn label(self) -> &'static str {
        match self {
            CapTier::Required => "必选 / required",
            CapTier::Optional => "可选 / optional",
        }
    }
}

/// 能力来源：文档化常量 or 必须查询。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapSource {
    /// 规范文档化的常量。
    Static,
    /// 必须向 surface 查询（**不是常量**）。
    Queried,
}

impl CapSource {
    pub const fn label(self) -> &'static str {
        match self {
            CapSource::Static => "静态 / static",
            CapSource::Queried => "查询 / queried",
        }
    }
}

/// 跨 API 统一后的能力档案。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Caps {
    pub required: ModeMask,
    pub optional: ModeMask,
    /// 缓冲数下界（由必选集的最严档决定）。
    pub min_buffers: u8,
    /// 抽象层的缓冲数上界兜底（**真实上界还要与查询值取小**）。
    pub max_buffers: u8,
    pub source: CapSource,
}

/// 三家能力档案（**数据单源**——判据与文档都引这里，别处不重抄）。
pub const fn caps_of(api: PresentApi) -> Caps {
    match api {
        PresentApi::D3D12 => Caps {
            required: ModeMask::FIFO.union(ModeMask::MAILBOX),
            optional: ModeMask::IMMEDIATE,
            min_buffers: BUFFERS_DOUBLE,
            max_buffers: 4,
            source: CapSource::Static,
        },
        PresentApi::Vulkan => Caps {
            required: ModeMask::FIFO,
            optional: ModeMask::MAILBOX.union(ModeMask::IMMEDIATE),
            min_buffers: BUFFERS_DOUBLE,
            max_buffers: 4,
            source: CapSource::Queried,
        },
        PresentApi::Metal => Caps {
            required: ModeMask::FIFO,
            optional: ModeMask::NONE,
            min_buffers: BUFFERS_DOUBLE,
            max_buffers: 3,
            source: CapSource::Static,
        },
    }
}

/// 跨 API 抽象违例：请求的模式该后端做不到。
pub const CODE_UNSUPPORTED_MODE: PresentCode = 0x460B;

/// 跨 API 抽象违例：必选模式所需的缓冲数不足。
pub const CODE_MODE_BUFFER_SHORT: PresentCode = 0x460C;

/// 能力未查询（查询源缺查询值 ⇒ 拒，不拿兜底常量当实测）。
pub const CODE_CAP_UNQUERIED: PresentCode = 0x460D;

/// 跨 API 抽象违例：缓冲数超出该后端能力范围。
pub const CODE_BUFFER_COUNT_RANGE: PresentCode = 0x460E;

/// 一次跨 API 呈现请求（**未校验**）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CrossRequest {
    pub api: PresentApi,
    pub mode: SyncMode,
    pub buffers: u8,
    pub width: u32,
    pub height: u32,
}

impl CrossRequest {
    pub const fn new(api: PresentApi, mode: SyncMode, buffers: u8, width: u32, height: u32) -> Self {
        CrossRequest { api, mode, buffers, width, height }
    }
}

/// 归一后的呈现计划（**可逐位比对**——原子性判据依赖这一点）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CrossPlan {
    pub api: PresentApi,
    pub mode: SyncMode,
    pub tier: CapTier,
    pub buffers: u8,
    /// 生效的缓冲数上界（静态源=档案值；查询源=min(兜底, 查询值)）。
    pub effective_max: u8,
    pub width: u32,
    pub height: u32,
}

/// 跨 API 呈现模型统一器。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CrossApi;

impl CrossApi {
    /// 归一校验。**抽象违例一律拒绝，不降级**。
    ///
    /// 规则次序刻意固定为「模式能力 → 能力来源 → 缓冲数 → 几何」：
    /// 同违两条时恒定报先检那条。次序若不确定，同一份请求在不同构建里
    /// 会给出不同错误码，跨批对账无从说起。
    pub const fn unify(req: &CrossRequest, queried_max: Option<u8>) -> Result<CrossPlan, PresentCode> {
        let caps = caps_of(req.api);

        // 规则 1：模式能力（必选/可选两档，缺则拒绝）。
        let tier = if caps.required.contains(req.mode) {
            CapTier::Required
        } else if caps.optional.contains(req.mode) {
            CapTier::Optional
        } else {
            return Err(CODE_UNSUPPORTED_MODE);
        };

        // 规则 2：能力来源。查询源缺查询值即拒——不拿兜底常量当实测。
        let effective_max = match caps.source {
            CapSource::Static => caps.max_buffers,
            CapSource::Queried => match queried_max {
                None => return Err(CODE_CAP_UNQUERIED),
                Some(q) => {
                    if q < caps.min_buffers {
                        return Err(CODE_CAP_UNQUERIED);
                    }
                    if q < caps.max_buffers {
                        q
                    } else {
                        caps.max_buffers
                    }
                }
            },
        };

        // 规则 3：缓冲数。**必选模式的档位下界先于**档案下界生效——
        // mailbox 在任何后端都要三缓冲才有意义，这是语义要求不是能力要求。
        let floor = if tier.is_required() {
            if req.mode.min_buffers() > caps.min_buffers {
                req.mode.min_buffers()
            } else {
                caps.min_buffers
            }
        } else {
            caps.min_buffers
        };
        if req.buffers < floor || req.buffers > effective_max {
            return Err(CODE_BUFFER_COUNT_RANGE);
        }

        // 规则 4：几何（复用描述符红线，避免两处口径不一致）。
        if req.width < MIN_WIDTH
            || req.width > MAX_WIDTH
            || req.height < MIN_HEIGHT
            || req.height > MAX_HEIGHT
        {
            return Err(CODE_BAD_DESCRIPTOR);
        }

        Ok(CrossPlan {
            api: req.api,
            mode: req.mode,
            tier,
            buffers: req.buffers,
            effective_max,
            width: req.width,
            height: req.height,
        })
    }

    /// 独立重算：给定 (api, mode) 的能力档位（判据侧自算，不调被测函数）。
    pub const fn expect_tier(api: PresentApi, mode: SyncMode) -> Option<CapTier> {
        let caps = caps_of(api);
        if caps.required.contains(mode) {
            Some(CapTier::Required)
        } else if caps.optional.contains(mode) {
            Some(CapTier::Optional)
        } else {
            None
        }
    }

    /// 把归一计划折成 [`ChainDesc`]，交给 [`PresentHub::rebuild`] 落链。
    ///
    /// **两节唯一的接缝**：跨 API 归一产出计划，重建事务消费计划。
    /// 少这一步，归一层就成了不参与出货的旁支——而判据只能测到它测不到
    /// 「真链是否由归一结果建出」。
    pub const fn to_desc(plan: &CrossPlan, allow_tearing: bool) -> ChainDesc {
        ChainDesc::new(plan.width, plan.height, plan.buffers, allow_tearing)
    }
}

// ---------------------------------------------------------------------------
// 十一、判据
// ---------------------------------------------------------------------------

pub fn run_vea46_checks() -> CheckSet {
    let mut s = CheckSet::new("vea46_present");

    // --- 判据 1：描述符校验——越界即拒，不静默截断 ---
    //
    // 口径分离（易混，故写明）：`MAX_BUFFERS` 是**数组容量上界**
    // （`refs: [u8; MAX_BUFFERS]` 的长度），**不是**「合法档位」。
    // 合法档位只有 1/2/3（[`BufferTier`]），由 `from_count` 判定，
    // 走**抽象违例**通道（`CODE_BUFFER_MISMATCH`）而非描述符非法通道。
    // 初版把两者混为一谈：拿 `buffers == MAX_BUFFERS`（=8）当「越界」用例，
    // 可它 validate 明明通过 ⇒ 判据自相矛盾、恒红。
    // 故本条只断三条真正的描述符红线：宽/高越界、缓冲数为 0、缓冲数超容量。
    {
        let ok = ChainDesc::new(1920, 1080, BUFFERS_TRIPLE, false).validate();
        let zero_w = ChainDesc::new(0, 1080, BUFFERS_TRIPLE, false).validate();
        let huge = ChainDesc::new(MAX_WIDTH + 1, 1080, BUFFERS_TRIPLE, false).validate();
        let too_many = ChainDesc::new(1920, 1080, MAX_BUFFERS + 1, false).validate();
        let no_buf = ChainDesc::new(1920, 1080, 0, false).validate();
        s.add(
            "A46-描述符-越界即拒不静默截断",
            ok.is_ok()
                && zero_w == Err(CODE_BAD_DESCRIPTOR)
                && huge == Err(CODE_BAD_DESCRIPTOR)
                && too_many == Err(CODE_BAD_DESCRIPTOR)
                && no_buf == Err(CODE_BAD_DESCRIPTOR)
                // 反证：恰好等于宽高上界与缓冲**容量**上界都合法（边界不off-by-one）
                && ChainDesc::new(MAX_WIDTH, MAX_HEIGHT, MAX_BUFFERS, false).validate().is_ok()
                // 判据侧独立重算三条红线的阈值（不向被测函数问答案）
                && MIN_WIDTH == 2
                && MAX_WIDTH == 16384
                && MAX_HEIGHT == 16384,
            "宽高越界 / 缓冲数为 0 或超容量一律 CODE_BAD_DESCRIPTOR；恰好等于上界合法，不截断",
        );
    }

    // --- 判据 2：四件之一 交换链——世代单调、飞帧引用正确 ---
    {
        let d = ChainDesc::new(1920, 1080, BUFFERS_TRIPLE, false);
        let mut c = SwapChain::new(d, 7);
        let acquired = c.acquire(0).is_ok() && c.acquire(2).is_ok();
        let held2 = c.held() == 2;
        let released = c.release(0).is_ok() && c.held() == 1;
        // 越界索引：零 panic 面 + 抽象违例
        let oob = c.buffer(99).is_none() && c.acquire(99) == Err(CODE_BUFFER_MISMATCH);
        let released_twice = c.release(0) == Err(CODE_BUFFER_MISMATCH);
        s.add(
            "A46-交换链-世代单调且飞帧引用计数正确",
            acquired
                && held2
                && released
                && oob
                && released_twice
                && c.generation() == 7
                && c.buffer_count() == BUFFERS_TRIPLE
                // 反证：释放后仍有 1 个被持有 ⇒ 销毁必被拒
                && c.destroy() == Err(CODE_STILL_REFERENCED)
                && !c.destroyed(),
            "世代号原样保留；飞帧持 2 放 1 计数正确；越界索引零 panic 且拒绝；仍被持有时销毁被拒",
        );
    }

    // --- 判据 3：上屏帧号必须单调（呈现诚实的核心） ---
    {
        let d = ChainDesc::new(1920, 1080, BUFFERS_TRIPLE, false);
        let mut c = SwapChain::new(d, 1);
        let first = c.mark_presented(10).is_ok();
        let same = c.mark_presented(10) == Err(CODE_PRESENT_NOT_MONOTONIC);
        let back = c.mark_presented(9) == Err(CODE_PRESENT_NOT_MONOTONIC);
        let fwd = c.mark_presented(11).is_ok();
        s.add(
            "A46-呈现诚实-上屏帧号单调递增",
            first
                && same
                && back
                && fwd
                // 反证：记的确实是最后一次的值
                && c.presented_frame() == 11,
            "上屏帧号须严格递增；同帧号与回退均被拒（否则没上屏的帧被算成上屏）",
        );
    }

    // --- 判据 4：四件之二 多缓冲——缓冲数与档位一致 ---
    {
        let mut ok = true;
        let mut i = 0usize;
        while i < BUFFER_TIERS {
            let t = BufferTier::ALL[i];
            // 数量与档位往返
            if t.count() != BUFFERS_SINGLE + t.ordinal() as u8 {
                ok = false;
            }
            if BufferTier::from_count(t.count()) != Some(t) {
                ok = false;
            }
            if t.ordinal() != i {
                ok = false;
            }
            i += 1;
        }
        // 档位外的数量必须返 None（不静默归到最近档）
        let outside = BufferTier::from_count(5).is_none()
            && BufferTier::from_count(0).is_none()
            && BufferTier::from_count(MAX_BUFFERS).is_none();
        s.add(
            "A46-多缓冲-数量与档位往返自洽且档位外返None",
            ok
                && outside
                // 判据侧独立重算三档数量（不向被测函数问答案）
                && BUFFER_TIERS == 3
                && BUFFERS_SINGLE == 1
                && BUFFERS_DOUBLE == 2
                && BUFFERS_TRIPLE == 3,
            "三档数量 1/2/3 与 ordinal 往返自洽；档位外数量返 None 不静默归档",
        );
    }

    // --- 判据 5：四件之三 同步——四值齐全性 ---
    {
        // 已上屏必须带帧号；已提交不得带帧号；撕裂必须带帧号；未知宽免。
        let complete = honesty_complete(PresentState::OnScreen, 42)
            && honesty_complete(PresentState::Submitted, 0)
            && honesty_complete(PresentState::Tore, 42)
            && honesty_complete(PresentState::Unknown, 0)
            && honesty_complete(PresentState::Unknown, 99);
        let incomplete = !honesty_complete(PresentState::OnScreen, 0)
            && !honesty_complete(PresentState::Submitted, 42)
            && !honesty_complete(PresentState::Tore, 0);
        s.add(
            "A46-同步-四值齐全性且缺项即破律",
            complete
                && incomplete
                // 「未知」是不可省略的第四值：只有三值就不存在「我不知道」这一说
                && PresentState::ALL.len() == 4
                && PresentState::OnScreen.is_visible()
                && !PresentState::Submitted.is_visible()
                && !PresentState::Unknown.is_visible(),
            "已上屏/撕裂必带帧号、已提交不得带帧号、未知宽免；缺项即破律；四值齐全",
        );
    }

    // --- 判据 6：原子重建三段——任一段失败都不动现役链 ---
    {
        let mut h = PresentHub::new();
        let good = ChainDesc::new(1920, 1080, BUFFERS_TRIPLE, false);
        let g1 = h.rebuild(good);
        let gen1 = h.active_generation();

        // 注入三段失败，逐段验证现役链不变
        let mut stage_ok = true;
        let mut k = 0usize;
        while k < REBUILD_STAGES {
            let f = match k {
                0 => FaultInjector::AtValidate,
                1 => FaultInjector::AtBuild,
                _ => FaultInjector::AtSwap,
            };
            let mut p = PresentHub::new();
            let _ = p.rebuild(good);
            let base = p.active_generation();
            p.set_injector(f);
            let r = p.rebuild(ChainDesc::new(800, 600, BUFFERS_DOUBLE, false));
            if r != Err(CODE_INJECTED_FAULT) || p.active_generation() != base {
                stage_ok = false;
            }
            k += 1;
        }
        s.add(
            "A46-原子重建-三段任一失败均保留旧链",
            g1.is_ok()
                && gen1 == 1
                && stage_ok
                // 反证：注入必被记账（注入后仍成功 ⇒ 演练本身失效）
                && h.failures().injected_total == 0
                && REBUILD_STAGES == 3,
            "校验/建链/换指针三段各自注入失败均返 CODE_INJECTED_FAULT 且现役链世代不变；成功后世代从 1 起",
        );
    }

    // --- 判据 6b：三段注入**各自**都必须进账本（失败可见律的记账面） ---
    //
    // 判据 6 只断「返回错误码 + 不换指针」，**没断账本**。于是「某段注入失败
    // 直接 return 不记账」会全绿通过：演练照样失败、错误码照样返回，但失败
    // 在账本上不可见——现场读账本的人看到「零失败」而链已停在旧世代。
    // 失败可见律分两律：**必须发生**（判据 6 钉）+ **必须留痕**（本判据钉）。
    {
        let good = ChainDesc::new(1920, 1080, BUFFERS_TRIPLE, false);
        let other = ChainDesc::new(800, 600, BUFFERS_DOUBLE, false);
        let mut book_ok = true;
        // 覆盖度：每段记它自己的注入点是否命中自己（不重不漏）
        let mut cover = [false; REBUILD_STAGES];
        let mut k = 0usize;
        while k < REBUILD_STAGES {
            let f = match k {
                0 => FaultInjector::AtValidate,
                1 => FaultInjector::AtBuild,
                _ => FaultInjector::AtSwap,
            };
            let mut p = PresentHub::new();
            let _ = p.rebuild(good);
            // 前置：此刻账本必须**干净**，否则下面的「恰 1」是空断言
            if p.failures().total != 0 || p.failures().injected_total != 0 {
                book_ok = false;
            }
            let gen_before = p.active_generation();
            p.set_injector(f);
            let r = p.rebuild(other);
            p.set_injector(FaultInjector::None);
            let l = p.failures();
            // 逐段验：恰好记一笔、恰为演练、代码可查、真实笔数不涨、窗口只一格、现役链不动
            if r != Err(CODE_INJECTED_FAULT)
                || l.total != 1
                || l.injected_total != 1
                || l.real_failures() != 0
                || l.code_at(0) != Some(CODE_INJECTED_FAULT)
                || l.injected_at(0) != Some(true)
                || !l.injected_at(1).is_none()
                || l.retained() != 1
                || p.active_generation() != gen_before
            {
                book_ok = false;
            }
            // 覆盖度记在**断言之后**：cover[k] 的含义是「本段确由自己的注入点命中」
            cover[k] = f.hits(k);
            k += 1;
        }
        // 三段都被各自注入点覆盖
        let mut cover_ok = true;
        let mut j = 0usize;
        while j < REBUILD_STAGES {
            if !cover[j] {
                cover_ok = false;
            }
            j += 1;
        }
        s.add(
            "A46-失败留痕-三段注入各自恰好记一笔演练",
            book_ok
                && cover_ok
                // 反证：不注入时不记账（否则「记一笔」恒成立）
                && {
                    let mut p = PresentHub::new();
                    let _ = p.rebuild(good);
                    p.failures().injected_total == 0
                        && p.failures().total == 0
                        && p.failures().retained() == 0
                },
            "校验/建链/换指针三段注入失败后账本各恰好记一笔演练且真实笔数不涨；不注入时账本全零",
        );
    }

    // --- 判据 7：重建失败必须可见（失败可见律） ---
    {
        let mut h = PresentHub::new();
        let _ = h.rebuild(ChainDesc::new(1920, 1080, BUFFERS_TRIPLE, false));
        // 真实故障：非法描述符
        let bad = h.rebuild(ChainDesc::new(0, 0, 0, false));
        // 抽象违例：缓冲数 4（属MAX_BUFFERS 但不属三档）
        let mismatch = h.rebuild(ChainDesc::new(1920, 1080, 4, false));
        s.add(
            "A46-失败可见-重建失败必留痕且可区分演练",
            bad == Err(CODE_BAD_DESCRIPTOR)
                && mismatch == Err(CODE_BUFFER_MISMATCH)
                // 两笔失败都进账本，且都**不是**演练
                && h.failures().total == 2
                && h.failures().injected_total == 0
                && h.failures().real_failures() == 2
                && h.failures().retained() == 2
                && h.failures().code_at(0) == Some(CODE_BAD_DESCRIPTOR)
                && h.failures().code_at(1) == Some(CODE_BUFFER_MISMATCH)
                // 越界取码返 None（零 panic 面）
                && h.failures().code_at(99).is_none(),
            "非法描述符与缓冲数不符各记一笔，代码逐条可查，越界取码返 None；演练与真实可区分",
        );
    }

    // --- 判据 8：演练注入必须被记账且可与真实区分 ---
    {
        let mut h = PresentHub::new();
        let good = ChainDesc::new(1920, 1080, BUFFERS_TRIPLE, false);
        let _ = h.rebuild(good);
        h.set_injector(FaultInjector::AtBuild);
        let injected = h.rebuild(ChainDesc::new(1280, 720, BUFFERS_DOUBLE, false));
        h.set_injector(FaultInjector::None);
        let real = h.rebuild(ChainDesc::new(0, 100, 1, false));
        s.add(
            "A46-演练注入-失败留痕且与真实故障分账",
            injected == Err(CODE_INJECTED_FAULT)
                && real == Err(CODE_BAD_DESCRIPTOR)
                // 两笔都留痕，但一笔演练一笔真实
                && h.failures().total == 2
                && h.failures().injected_total == 1
                && h.failures().real_failures() == 1
                && h.failures().injected_at(0) == Some(true)
                && h.failures().injected_at(1) == Some(false)
                // 演练后现役链仍是成功那代（失败不动现役链）
                && h.active_generation() == 1
                && h.active_buffers() == BUFFERS_TRIPLE,
            "注入失败记为演练、真实失败记为真实，两者分账；演练后现役链仍为成功那代",
        );
    }

    // --- 判据 9：热切换失败必回旧链（两段事务） ---
    {
        let mut h = PresentHub::new();
        let _ = h.rebuild(ChainDesc::new(1920, 1080, BUFFERS_TRIPLE, false));
        let gen_before = h.active_generation();
        let buf_before = h.active_buffers();
        // 注入失败的热切换⇒ 必须回旧链而非停在「一半」
        h.set_injector(FaultInjector::AtSwap);
        let o = h.hot_swap(ChainDesc::new(1280, 720, BUFFERS_DOUBLE, false));
        h.set_injector(FaultInjector::None);
        s.add(
            "A46-热切换-失败必回旧链不停在半途",
            o == HotSwapOutcome::RolledBack
                && h.active_generation() == gen_before
                && h.active_buffers() == buf_before
                && h.swaps_rolled_back() == 1
                && h.swaps_ok() == 0
                // 成功切换一次以对照（证明机制本身是通的）
                && {
                    h.hot_swap(ChainDesc::new(1280, 720, BUFFERS_DOUBLE, false))
                        == HotSwapOutcome::Swapped
                    && h.active_generation() == gen_before + 1
                    && h.active_buffers() == BUFFERS_DOUBLE
                    && h.swaps_ok() == 1
                },
            "注入失败的热切换回旧链（世代/缓冲数不变）；不注入时成功切换且世代+1",
        );
    }

    // --- 判据 10：未建链即呈现必拒（抽象违例一律拒绝） ---
    {
        let mut h = PresentHub::new();
        let r = h.present(1, false);
        s.add(
            "A46-抽象违例-未建链即呈现被拒且留痕",
            r == Err(CODE_NO_CHAIN)
                && h.failures().total == 1
                && h.failures().code_at(0) == Some(CODE_NO_CHAIN)
                // 反证：拒绝后提交计数未增（没把被拒当已提交）
                && h.submitted_frames() == 0
                && h.onscreen_frames() == 0,
            "未建链即呈现返 CODE_NO_CHAIN、失败入账，且提交/上屏计数均未增",
        );
    }

    // --- 判据 11：撕裂只在允许且显式请求时发生 ---
    {
        let mut h = PresentHub::new();
        let _ = h.rebuild(ChainDesc::new(1920, 1080, BUFFERS_TRIPLE, true));
        let t = h.present(10, true);
        // 不请求撕裂 ⇒ 不得撕裂
        let no_t = h.present(11, false);
        // 不允许撕裂的链上请求撕裂 ⇒ 不撕裂（抽象违例不静默放行）
        let mut h2 = PresentHub::new();
        let _ = h2.rebuild(ChainDesc::new(1920, 1080, BUFFERS_TRIPLE, false));
        let blocked = h2.present(10, true);
        s.add(
            "A46-撕裂-仅在允许且显式请求时发生",
            t == Ok(PresentState::Tore)
                && no_t == Ok(PresentState::OnScreen)
                // 链不允许撕裂时，即便请求也不撕裂（请求被忽略而非伪造撕裂）
                && blocked == Ok(PresentState::OnScreen)
                && h.torn_frames() == 1
                && h2.torn_frames() == 0
                // 上屏帧号仍单调⇒ 被忽略的撕裂请求没跳过帧号校验
                && h.onscreen_frames() == 2,
            "允许撕裂且请求⇒ 撕裂；不请求或链不允许⇒ 上屏；不允许时请求被忽略而非伪造撕裂",
        );
    }

    // --- 判据 12：帧号回退时诚实降级为「已提交未上屏」 ---
    {
        let mut h = PresentHub::new();
        let _ = h.rebuild(ChainDesc::new(1920, 1080, BUFFERS_TRIPLE, false));
        let a = h.present(100, false);
        let b = h.present(50, false); // 回退⇒ 不得硬报成功
        s.add(
            "A46-呈现诚实-帧号回退降级为已提交未上屏",
            a == Ok(PresentState::OnScreen)
                && b == Ok(PresentState::Submitted)
                // 帧号回退被记为失败（非静默）
                && h.failures().total == 1
                && h.failures().code_at(0) == Some(CODE_PRESENT_NOT_MONOTONIC)
                // 只有第一次计入上屏
                && h.onscreen_frames() == 1
                && h.submitted_frames() == 2,
            "上屏帧号回退时返「已提交未上屏」并记失败；提交 2 帧而上屏仅 1 帧",
        );
    }

    // --- 判据 13：面板七行双语且逐行绑定聚合量 ---
    {
        let mut h = PresentHub::new();
        let _ = h.rebuild(ChainDesc::new(1920, 1080, BUFFERS_TRIPLE, false));
        let _ = h.rebuild(ChainDesc::new(0, 0, 0, false)); // 一笔真实失败
        h.set_injector(FaultInjector::AtValidate);
        let _ = h.rebuild(ChainDesc::new(640, 480, BUFFERS_DOUBLE, false)); // 一笔演练
        h.set_injector(FaultInjector::None);
        let _ = h.present(7, false);
        let lines = h.a11y_lines();
        let all_nonempty = lines.iter().all(|l| !l.is_empty());
        // 各行取值互不相同是本条可判别的前提
        s.add(
            "A46-面板-七行双语且逐行绑定聚合量",
            lines.len() == 7
                && all_nonempty
                // 逐行绑定：世代 1 / 缓冲 3 / 提交 1 / 上屏 1 / 撕裂 0 / 失败 2(演练1) / 重建1
                && lines[0].contains("generation: 1")
                && lines[1].contains("buffer count: 3")
                && lines[2].contains("submitted frames: 1")
                && lines[3].contains("onscreen frames: 1")
                && lines[4].contains("torn frames: 0")
                && lines[5].contains("rebuild failures: 2")
                && lines[5].contains("演练 1")
                && lines[6].contains("rebuilds ok: 1")
                // 双语：每行含 ASCII 键名
                && lines.iter().all(|l| l.chars().any(|c| c.is_ascii_alphabetic()))
                // 前置账目核对
                && h.submitted_frames() == 1
                && h.onscreen_frames() == 1
                && h.failures().total == 2
                && h.failures().injected_total == 1,
            "面板七行逐行绑定：世代 1/缓冲 3/提交 1/上屏 1/撕裂 0/失败 2(演练 1)/重建 1，每行双语非空",
        );
    }

    // --- 判据 14：失败账本环形封顶但累计不减 ---
    {
        let mut led = FailureLedger::new();
        let mut i = 0u32;
        // 写入远超窗口的失败
        while i < (FAILURE_WINDOW as u32) + 20 {
            led.record(CODE_BAD_DESCRIPTOR, false);
            i += 1;
        }
        s.add(
            "A46-失败账-环形封顶但累计不减",
            led.retained() == FAILURE_WINDOW
                && led.total == (FAILURE_WINDOW as u32) + 20
                && led.real_failures() == FAILURE_WINDOW as u32
                // 越界取码/取标记返 None
                && led.code_at(FAILURE_WINDOW).is_none()
                && led.injected_at(FAILURE_WINDOW).is_none(),
            "写入 52 笔后窗口仍封顶 32、累计 52 不减；越界取码与取标记均返 None",
        );
    }

    // --- 判据 15：零 panic 面 + 枚举线编码往返 ---
    {
        let mut ok = true;
        let mut i = 0usize;
        while i < PresentState::ALL.len() {
            let st = PresentState::ALL[i];
            if st.ordinal() != i {
                ok = false;
            }
            match PresentState::from_wire(st.wire()) {
                Some(back) => {
                    if back != st {
                        ok = false;
                    }
                }
                None => ok = false,
            }
            if st.label().is_empty() {
                ok = false;
            }
            i += 1;
        }
        // 越界线编码返 None（不 panic）
        ok = ok
            && PresentState::from_wire(200).is_none()
            && HotSwapOutcome::ALL.len() == 3
            && FaultInjector::ALL.len() == 4;
        // 注入点命中语义：None 永不命中，三个注入点各命中一段
        let mut hit_ok = true;
        let mut st = 0usize;
        while st < REBUILD_STAGES {
            if FaultInjector::None.hits(st) {
                hit_ok = false;
            }
            st += 1;
        }
        s.add(
            "A46-零panic-枚举线编码往返与注入点命中语义",
            ok
                && hit_ok
                // 反证：三个注入点确实各命中一段（否则演练形同虚设）
                && FaultInjector::AtValidate.hits(0)
                && FaultInjector::AtBuild.hits(1)
                && FaultInjector::AtSwap.hits(2)
                && !FaultInjector::AtBuild.hits(0),
            "四态呈现结论下标连续、线编码往返自洽、越界返 None；四个注入点语义正确",
        );
    }

    // --- 判据 16：跨 API 统一——三家能力档案两两不同 ---
    //
    // 「统一」若把三家拍平成同一份档案，就等于丢掉了差异；而差异正是
    // 归一层存在的理由。此处断言两两至少一轴不同，防止有人为了「统一好看」
    // 把三份档案改成同一份。
    {
        let a = caps_of(PresentApi::D3D12);
        let b = caps_of(PresentApi::Vulkan);
        let c = caps_of(PresentApi::Metal);
        let differs = |x: &Caps, y: &Caps| {
            x.required != y.required
                || x.optional != y.optional
                || x.min_buffers != y.min_buffers
                || x.max_buffers != y.max_buffers
                || x.source != y.source
        };
        s.add(
            "A46-跨API-三家能力档案两两不同",
            differs(&a, &b) && differs(&a, &c) && differs(&b, &c) && API_TIERS == 3,
            "D3D12/Vulkan/Metal 在必选集/可选集/缓冲上下界/能力来源上两两不同（拍平即丢信息）",
        );
    }

    // --- 判据 17：必选/可选两级语义（Vulkan 只有 fifo 必选） ---
    {
        let vk = caps_of(PresentApi::Vulkan);
        let d3d = caps_of(PresentApi::D3D12);
        let mtl = caps_of(PresentApi::Metal);
        s.add(
            "A46-跨API-必选集与可选集按规范分级",
            // Vulkan：fifo 必选（规范唯一保证），mailbox/immediate 扩展可选
            vk.required == ModeMask::FIFO
                && vk.optional.contains(SyncMode::Mailbox)
                && vk.optional.contains(SyncMode::Immediate)
                && vk.source == CapSource::Queried
                // D3D12：flip 模型的 fifo+mailbox 保证，immediate 走 independent flip ⇒ 可选
                && d3d.required.contains(SyncMode::Mailbox)
                && d3d.optional == ModeMask::IMMEDIATE
                && d3d.source == CapSource::Static
                // Metal：只有 fifo，mailbox/immediate 都拿不到
                && mtl.required == ModeMask::FIFO
                && mtl.optional == ModeMask::NONE
                && mtl.max_buffers == BUFFERS_TRIPLE,
            "必选集按规范而非按厂商分档；Metal 可选集为空、缓冲上界 3（系统隐式三缓冲）",
        );
    }

    // --- 判据 18：抽象违例拒绝——穷举 3×3，恰 3 组被拒且**不降级** ---
    //
    // Metal×mailbox、Metal×immediate、Vulkan×immediate?——不，Vulkan 的
    // immediate 在可选集内。故被拒恰两组（Metal 的后两档）。
    // 判据侧用 [`CrossApi::expect_tier`] 独立重算期望，不向被测函数问答案。
    {
        let mut rejected = 0u32;
        let mut wrongly_ok = 0u32;
        let mut i = 0usize;
        while i < API_TIERS {
            let mut j = 0usize;
            while j < MODE_TIERS {
                let api = PresentApi::ALL[i];
                let mode = SyncMode::ALL[j];
                // 查询源必须给查询值，否则会先撞 CAP_UNQUERIED
                let q = if caps_of(api).source == CapSource::Queried { Some(4) } else { None };
                let req = CrossRequest::new(api, mode, BUFFERS_TRIPLE, 1920, 1080);
                let expect_some = CrossApi::expect_tier(api, mode).is_some();
                match CrossApi::unify(&req, q) {
                    Ok(_) => {
                        if !expect_some {
                            wrongly_ok += 1;
                        }
                    }
                    Err(e) => {
                        if e == CODE_UNSUPPORTED_MODE && !expect_some {
                            rejected += 1;
                        }
                    }
                }
                j += 1;
            }
            i += 1;
        }
        // 反向：Metal×fifo 必过（证明上面不是「全拒」的假绿）
        let good = CrossRequest::new(PresentApi::Metal, SyncMode::Fifo, BUFFERS_TRIPLE, 1280, 720);
        s.add(
            "A46-跨API-九组合穷举恰两组违例且不降级",
            rejected == 2 && wrongly_ok == 0 && CrossApi::unify(&good, None).is_ok(),
            "3×3 全组合穷举：Metal×mailbox 与 Metal×immediate 报 CODE_UNSUPPORTED_MODE，其余通过（无悄悄降级）",
        );
    }

    // --- 判据 19：查询源缺查询即拒，且查询值真参与上界裁决 ---
    {
        let req = CrossRequest::new(PresentApi::Vulkan, SyncMode::Fifo, BUFFERS_DOUBLE, 1920, 1080);
        let no_query = CrossApi::unify(&req, None);
        // 夹逼对：查询 2 时 2 过 / 4 拒；查询 4 时 4 过。同一请求只因查询值
        // 变化而从过变拒 ⇒ 裁决确实读了查询值，不是恒真。
        let three = CrossRequest::new(PresentApi::Vulkan, SyncMode::Fifo, 3, 1920, 1080);
        let pass3 = CrossApi::unify(&three, Some(3));
        let fail3 = CrossApi::unify(&three, Some(2));
        // 查询值低于下界 ⇒ 拒（不静默夹取）
        let too_small = CrossApi::unify(&req, Some(1));
        // 静态源无需查询
        let d3d = CrossRequest::new(PresentApi::D3D12, SyncMode::Mailbox, 3, 2560, 1440);
        s.add(
            "A46-跨API-查询源缺查询即拒且查询值参与裁决",
            no_query == Err(CODE_CAP_UNQUERIED)
                && matches!(pass3, Ok(p) if p.effective_max == 3)
                && fail3 == Err(CODE_BUFFER_COUNT_RANGE)
                && too_small == Err(CODE_CAP_UNQUERIED)
                && CrossApi::unify(&d3d, None).is_ok(),
            "Vulkan 缺 maxBuffers 查询即 CODE_CAP_UNQUERIED；查询 3 时 3 过/4 拒；查询 1 拒；D3D12 无需查询",
        );
    }

    // --- 判据 20：必选 mailbox 需三缓冲（语义下界而非能力下界） ---
    //
    // mailbox 在**任何**后端都要三缓冲才有意义：两缓冲下它退化为 fifo，
    // 拿不到「不撕裂且低延迟」同时成立。若只按各家 `min_buffers`（都=2）裁决，
    // 「D3D12 必选 mailbox + 双缓冲」会被放行——那是一条不成立的计划。
    {
        let two = CrossRequest::new(PresentApi::D3D12, SyncMode::Mailbox, BUFFERS_DOUBLE, 1920, 1080);
        let three = CrossRequest::new(PresentApi::D3D12, SyncMode::Mailbox, BUFFERS_TRIPLE, 1920, 1080);
        // immediate 在双缓冲下合法（它就是靠撕裂换延迟的）
        let imm2 = CrossRequest::new(PresentApi::D3D12, SyncMode::Immediate, BUFFERS_DOUBLE, 1920, 1080);
        s.add(
            "A46-跨API-必选mailbox需三缓冲而immediate可双缓冲",
            CrossApi::unify(&two, None) == Err(CODE_BUFFER_COUNT_RANGE)
                && CrossApi::unify(&three, None).is_ok()
                && CrossApi::unify(&imm2, None).is_ok()
                // 判据侧独立重算三档的缓冲下界（不调被测函数）
                && SyncMode::Fifo.min_buffers() == BUFFERS_DOUBLE
                && SyncMode::Mailbox.min_buffers() == BUFFERS_TRIPLE
                && SyncMode::Immediate.min_buffers() == BUFFERS_DOUBLE,
            "必选 mailbox + 双缓冲被拒（语义下界），三缓冲过；immediate 双缓冲过（撕裂换延迟成立）",
        );
    }

    // --- 判据 21：跨 API 接缝——归一计划真的建出现役链 ---
    //
    // **这条是本节的关键门禁**：判据 16~20 只测归一器本身。若归一层与
    // [`PresentHub`] 之间没有真接缝，归一器可以全绿而现役链其实由别处
    // 建出——那正是「判据覆盖不到出货路径」的弱门禁形态。故此处让归一结果
    // 经 [`CrossApi::to_desc`] 落链，并断链的世代/缓冲/几何三要素逐位等于计划。
    {
        let mut h = PresentHub::new();
        let req = CrossRequest::new(PresentApi::Vulkan, SyncMode::Mailbox, BUFFERS_TRIPLE, 2560, 1440);
        let plan = match CrossApi::unify(&req, Some(4)) {
            Ok(p) => p,
            Err(_) => {
                s.fail("A46-跨API接缝-归一计划构造失败", "Vulkan mailbox 三缓冲 + 查询4 应当通过");
                return s;
            }
        };
        // 归一 → 描述符 → 重建事务
        let desc = CrossApi::to_desc(&plan, false);
        let gen = h.rebuild(desc);
        s.add(
            "A46-跨API接缝-归一计划经重建落成现役链",
            gen.is_ok()
                && gen == Ok(1)
                && h.active_generation() == 1
                // 三要素逐位等于归一计划（缓冲数/世代来自链，几何来自描述符）
                && h.active_buffers() == plan.buffers
                && h.active_buffers() == BUFFERS_TRIPLE
                // 反证：描述符确实带着计划的几何（不是另造一个默认值）
                && desc.width == plan.width
                && desc.height == plan.height
                // 归一层被改动时这条会红：计划是链的唯一输入
                && CrossApi::to_desc(&plan, true).allow_tearing
                && !desc.allow_tearing,
            "归一计划经 to_desc 喂给重建事务，现役链世代 1、缓冲数与几何逐位等于计划（归一层在出货路径上）",
        );
    }

    // --- 判据 22：跨 API 违例不得留下半成品链（归一失败即零副作用） ---
    //
    // 归一失败若仍改了现役链，就等于「拒绝」只停在返回值上——而现场看到的是
    // 一条被换掉的链。故此处断：归一失败后现役链**逐位未变**。
    {
        let mut h = PresentHub::new();
        let good = CrossRequest::new(PresentApi::Vulkan, SyncMode::Mailbox, BUFFERS_TRIPLE, 1920, 1080);
        if let Ok(plan) = CrossApi::unify(&good, Some(4)) {
            let _ = h.rebuild(CrossApi::to_desc(&plan, false));
        }
        let gen_before = h.active_generation();
        let buf_before = h.active_buffers();
        // Metal×mailbox ⇒ 归一违例，根本不该走到 rebuild
        let bad = CrossRequest::new(PresentApi::Metal, SyncMode::Mailbox, BUFFERS_TRIPLE, 3840, 2160);
        let rejected = CrossApi::unify(&bad, None);
        s.add(
            "A46-跨API-归一违例零副作用不换现役链",
            rejected == Err(CODE_UNSUPPORTED_MODE)
                && h.active_generation() == gen_before
                && h.active_buffers() == buf_before
                // 反证：违规请求**若**硬走rebuild 会被描述符/档位闸挡住吗？
                //   不必经由归一层——归一层的职责就是在此拦下。
                && CrossApi::unify(
                    &CrossRequest::new(PresentApi::Metal, SyncMode::Mailbox, BUFFERS_TRIPLE, 3840, 2160),
                    None
                ) == Err(CODE_UNSUPPORTED_MODE),
            "Metal×mailbox 归一即拒，现役链世代与缓冲数逐位未变（拒绝不只停在返回值上）",
        );
    }

    // --- 判据 23：模式位集运算与线编码往返（穷举） ---
    {
        let mut ok = true;
        let mut i = 0usize;
        while i < 8 {
            let m = ModeMask::ALL_MASKS[i];
            // 判据侧独立数位
            let mut manual = 0u32;
            let mut j = 0usize;
            while j < MODE_TIERS {
                if m.bits() & (1u8 << j) != 0 {
                    manual += 1;
                }
                j += 1;
            }
            if m.count() != manual {
                ok = false;
            }
            if ModeMask::from_bits(m.bits()) != m {
                ok = false;
            }
            if m.union(m) != m {
                ok = false;
            }
            // union 可交换
            let mut k = 0usize;
            while k < 8 {
                if m.union(ModeMask::ALL_MASKS[k]) != ModeMask::ALL_MASKS[k].union(m) {
                    ok = false;
                }
                k += 1;
            }
            i += 1;
        }
        // 模式与后端线编码往返 + 越界拒
        let mut j = 0usize;
        while j < MODE_TIERS {
            if SyncMode::ALL[j].wire() != j as u8 || SyncMode::from_wire(j as u8) != Some(SyncMode::ALL[j]) {
                ok = false;
            }
            j += 1;
        }
        let mut k = 0usize;
        while k < API_TIERS {
            if PresentApi::ALL[k].wire() != k as u8 || PresentApi::from_wire(k as u8) != Some(PresentApi::ALL[k]) {
                ok = false;
            }
            k += 1;
        }
        ok = ok && SyncMode::from_wire(MODE_TIERS as u8).is_none();
        ok = ok && SyncMode::from_wire(255).is_none();
        ok = ok && PresentApi::from_wire(API_TIERS as u8).is_none();
        ok = ok && PresentApi::from_wire(200).is_none();
        s.add(
            "A46-跨API-位集运算与线编码往返穷举自洽",
            ok,
            "穷举 8 个位集（计数/往返/幂等/可交换）与 3+3 个线编码；越界 3/200/255 均返 None",
        );
    }

    s
}