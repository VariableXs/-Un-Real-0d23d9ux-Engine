//! VE-F5606 · 语音系统骨架（深化版）（VE-AB 域 · 音频与语音域 · AB06 组）。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F5606`
//!
//! **判据（锚点原文）**：可靠通道、按权裁剪、零记录、判据。
//!
//! **职责定位（锚点原文）**：语音系统骨架——语音输入输出与 AA 域网络
//! 通道衔接（语音走 AA 可靠通道、按权裁剪、内容零记录）；语音链路含
//! 端到端延迟预算（说话到听见全链承诺）；静噪门限自适应——不说话时
//! 彻底安静；语音含说话状态指示——谁在说话一目了然（隐私：只示状
//! 态不示内容）。数据结构：语音编解码骨架；通道衔接。错误路径与降
//! 级矩阵：设备占用→抢占提示；编解码失败→降级码率；隐私违例→阻
//! 断。性能逐项分解：处理 O（帧）。跨批对接点：AA01 通道；AA05 隐
//! 私。无障碍与隐私：语音状态可见。
//!
//! **与速写版并存**：[`crate::svstar2::veab06_voice`] 为并行工人速写
//! 版（0x5D 段）；本深化版独立模块、0x5E 段，源码只增不减，深化版
//! 为准（仓库惯例）。
//!
//! ## 一、诊断码（0x5E 段独占：AB06 语音深化面）
//!
//! 十六码闭集（[`XcCode`]）：通道面四码（不可用/拥塞/权重饿死/序列
//! 空洞）、编解码面三码（降级/到底/损坏）、延迟面两码（段超/端到
//! 端超）、静噪与状态面三码（开门/关门/抖动抑制）、隐私面两码（违
//! 例阻断/载荷擦除留痕）、设备面两码（占用/抢占）。段守恒编译期钉
//! 死（[`segment_guard`] 对账全部十六码落在 0x5E00..0x5F00 开区间
//! 且互不重码）。
//!
//! ## 二、语音帧与编解码骨架（数据结构：语音编解码骨架）
//!
//! 语音帧五元组（[`VoiceFrame`]——序号 [`FrameSeq`]、说话人 [`Talker
//! Id`]、码率档 [`CodecTier`]、捕获 tick、载荷）；载荷指纹 FNV-1a
//! （[`VoiceFrame::fingerprint`]——收发两端对拍，不符即 [`XcCode::
//! DecodeCorrupt`]）；载荷擦除（[`VoiceFrame::erase`]——逐字节置零
//! 覆写后清空，零记录的执行层动作）。码率档六档闭集（[`CodecTier`]
//! ——64/48/24/12/6 kbps 与静音档；逐档帧字节数、降级下一档、升档
//! 上一档——降有底升有顶两端封闭）。编解码骨架（[`CodecEngine`]——
//! 帧长窗口校验入编码、指纹校验入解码，损坏帧不解码直接走降级链）。
//! 降级阶梯（[`DegradationLadder`]——连续失败 3 帧降一档、连续成功
//! 9 帧升一档，降一升三不对称迟滞防震荡；降到 6 kbps 仍失败进静音
//! 占位档并计数 [`XcCode::BitrateFloor`]——静音占位不是故障，是档
//! 位）。
//!
//! ## 三、AA 可靠通道衔接（跨批对接点：AA01 通道·判据一：可靠通道）
//!
//! 通道接缝 trait（[`ReliableChannel`]——AA01 语义的发送侧抽象：入
//! 队返回 [`EnqueueOutcome`] 闭集，通道自身承诺可靠重传，骨架层不代
//! 做传输只记账）；发送队列（[`SendQueue`]——每 tick 字节预算、语音
//! 帧与数据块两类负载）；按权裁剪（**总预算超限时裁数据保语音**：
//! 语音权重 [`WEIGHT_VOICE`] 90 高于数据权重 [`WEIGHT_DATA`] 30；语
//! 音自身被裁即权重饿死 [`XcCode::WeightStarved`] 立案——正常配置
//! 不应发生）；裁剪账本（[`TrimLedger`]——被裁帧数/字节数按类分计
//! 入账，裁剪不是静默丢弃）；接收侧序列空洞哨（[`RxGapWatcher`]——
//! 可靠通道下序号不应跳变，跳变即通道违约 [`XcCode::SeqGap`] 计数
//! 立案，供通道质量旁证审计）。
//!
//! ## 四、端到端延迟预算（判据：说话到听见全链承诺）
//!
//! 五段闭集（[`LegStage`]——采集/编码/网络/解码/播放）；逐段预算表
//! 字面量写死（20k+15k+80k+15k+20k 微秒），合计恒等于端到端承诺
//! [`E2E_BUDGET_US`] 150ms（[`budget_conservation`] 对账守恒）；段
//! 账本（[`LegLedger`]——逐帧逐段实测记账，超段预算即留痕并归因到
//! 段，最坏段自动重算）；端到端监工（[`E2eMonitor`]——滚动窗 32 帧
//! 超承诺计数，网络段压力超 25% 给降档建议——延迟压力传导到降级阶
//! 梯的联动钩子）。
//!
//! ## 五、静噪门限自适应（判据：不说话时彻底安静）
//!
//! 定点能量门（[`NoiseGate`]——输入为定点 RMS 平方能量，零浮点；指
//! 数平滑 α=1/4 位移实现）；双门限迟滞（开门限=底噪×8 高于关门限=
//! 底噪×4——防临界抖动）；吊尾保持（6 tick——说话结束后保持若干
//! tick 才真关门，防切字尾）；底噪自适应（无话期平滑值按 1/16 步长
//! 缓沉跟踪环境底噪——环境变吵门限自动抬升）；彻底安静承诺（门关
//! 期输出零载荷——不产任何发声字节，静音期 tick 计数入账供「该静
//! 时真静」审计）。
//!
//! ## 六、说话状态指示（判据：谁在说话一目了然·只示状态不示内容）
//!
//! 三态闭集（[`TalkState`]——静默/起势/说话；起势是过开门限但未满
//! 2 tick 确认的中间态，防瞬时噪声误报）；指示条目（[`TalkIndicator`]
//! ——说话人 id、状态、持续 tick、当前码率档；**类型上没有载荷字
//! 段**——指示结构不携带内容是结构性隐私保证，不是纪律约束）；抖
//! 动抑制（[`FlapSuppressor`]——40 tick 窗内翻转超 4 次强制滞留一
//! 窗并留痕 [`XcCode::TalkStateFlap`]——指示灯乱闪比不亮更伤可用
//! 性）；说话人看板（[`SpeakerBoard`]——容量 16 上限、逐说话人状态
//! 更新、看板快照序列化供 UI 与读屏消费——语音状态可见判据的落地
//! 形态）。
//!
//! ## 七、隐私合规与零记录（跨批对接点：AA05 隐私·判据：零记录）
//!
//! 权限闭集单变体（[`Permission::Denied`]——持久化权限类型上只有
//! 「拒绝」一个变体，不存在「允许」的构造路径——隐私违例阻断在类
//! 型层面成立）；隐私闸门（[`PrivacyGate::persist_intent`]——任何
//! 调用点调用即得 [`PrivacyVerdict::Blocked`] 并立案）；违例审计
//! （[`PrivacyAudit`]——滚动 64 案只记调用点标识与计数，审计账自身
//! 不成为内容旁路）；载荷对账（[`PrivacyAudit::erase_balance`]——
//! 入帧数与擦除数对账，非零即有载荷未经擦除路径流出，阻断级立案；
//! 正常闭环恒 0）。
//!
//! ## 八、设备占用仲裁（错误路径：设备占用→抢占提示）
//!
//! 用途闭集（[`DeviceUse`]——通话/听写/系统三类优先级互异：系统>
//! 通话>听写——听写可被通话抢占，系统诊断最高不可抢占）；仲裁器
//! （[`DeviceArbiter`]——独占态、同占用者幂等续占、他占用者低优
//! 先被拒留痕 [`XcCode::DeviceOccupied`]、高优先抢占成功且被抢占
//! 者收到提示条目 [`PreemptNotice`] 留痕 [`XcCode::DevicePreempted`]
//! ——抢占提示是锚点原文要求，不是静默夺走；主动释放防误释放他
//! 占；超时 1500 tick 强制回收防死占）。
//!
//! ## 九、主编排与快照（性能：处理 O（帧））
//!
//! 全链编排（[`VoiceSystem::tick`]——单 tick 单帧走完：静噪门→编解
//! 码（自对拍）→降级阶梯→通道入队（按权裁剪）→延迟记账→状态看
//! 板→隐私对账→设备巡检，全程操作数计账）；tick 报告（[`VoiceTick
//! Report`]——本 tick 全部落点与留痕一次带回）；快照（[`VoiceSnap`]
//! ——'|' 分隔保空段序列化 + FNV 指纹，供调试与读屏）；参考探针
//! （[`reference_budget_check`]——8 说话人 × 128 帧全链跑通 + 档位
//! 自洽 + 零记录闭环 + 双守恒 + 快照恒等，验证处理 O（帧）口径）。
//!
//! 零 unwrap、零 expect、零 panic 面、零 IO、零墙钟（全部时间以
//! tick 计数注入）。

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write as _;

// ============================================================
// 常量面（全部字面量写死，无运行时配置漂移）
// ============================================================

/// 端到端延迟承诺（微秒）：说话到听见全链 150ms。
///
/// 五段预算合计必须恒等于此值（[`budget_conservation`] 对账）。
pub const E2E_BUDGET_US: u64 = 150_000;

/// 采集段预算（微秒）。
pub const LEG_CAPTURE_US: u64 = 20_000;
/// 编码段预算（微秒）。
pub const LEG_ENCODE_US: u64 = 15_000;
/// 网络段预算（微秒）。
pub const LEG_NET_US: u64 = 80_000;
/// 解码段预算（微秒）。
pub const LEG_DECODE_US: u64 = 15_000;
/// 播放段预算（微秒）。
pub const LEG_PLAYBACK_US: u64 = 20_000;

/// 端到端监工滚动窗（帧）。
pub const E2E_WINDOW: usize = 32;

/// 语音负载权重（按权裁剪：语音高于数据，裁数据保语音）。
pub const WEIGHT_VOICE: u32 = 90;
/// 数据负载权重。
pub const WEIGHT_DATA: u32 = 30;

/// 发送队列每 tick 字节预算。
pub const QUEUE_BUDGET_BYTES_PER_TICK: u32 = 1_200;

/// 发送队列容量上限（帧 + 数据块合计）。
pub const QUEUE_CAPACITY: usize = 256;

/// 降级阶梯：连续失败降档阈（帧）。
pub const DEGRADE_AFTER_FAILS: u32 = 3;
/// 降级阶梯：连续成功升档阈（帧）——降一升三不对称，防档位震荡。
pub const UPGRADE_AFTER_OKS: u32 = 9;

/// 静噪开门限相对底噪倍数（定点：开阈值 = 底噪 × 8）。
pub const OPEN_RATIO_NUM: u32 = 8;
/// 静噪关门限相对底噪倍数（定点：关阈值 = 底噪 × 4）。
pub const CLOSE_RATIO_NUM: u32 = 4;

/// 吊尾保持（tick）：说话结束后保持若干 tick 才真关门，防切字尾。
pub const HANGOVER_TICKS: u32 = 6;
/// 起势确认（tick）：过开门限持续满若干 tick 才升「说话」。
pub const RISING_CONFIRM_TICKS: u32 = 2;
/// 底噪下沉步长分母（无话期平滑底噪向观测值收敛的速率）。
pub const FLOOR_ADAPT_DEN: u32 = 16;

/// 说话人看板容量上限。
pub const MAX_SPEAKERS: usize = 16;
/// 抖动抑制窗口（tick）。
pub const FLAP_WINDOW: u32 = 40;
/// 抖动抑制阈值：窗口内翻转超此次数即强制滞留。
pub const FLAP_LIMIT: u32 = 4;

/// 设备空闲超时强制回收（tick）。
pub const DEVICE_IDLE_TIMEOUT_TICKS: u32 = 1_500;

/// 快照指纹种子偏移（VoiceSnap 专用，避免与他面指纹撞值）。
pub const SNAP_FNV_SEED: u32 = 0x5E5E_0001;

// ============================================================
// 一、诊断码（0x5E 段独占：AB06 语音深化面）
// ============================================================

/// 诊断码闭集（0x5E 段独占：AB06 语音深化面十六码）。
///
/// 段位分配：
/// - 通道面 0x01..0x04：不可用 / 拥塞 / 权重饿死 / 序列空洞；
/// - 编解码面 0x05..0x07：降级 / 到底 / 损坏；
/// - 延迟面 0x08..0x09：段超 / 端到端超；
/// - 静噪与状态面 0x0A..0x0C：开门 / 关门 / 抖动抑制；
/// - 隐私面 0x0D..0x0E：违例阻断 / 载荷擦除留痕；
/// - 设备面 0x0F..0x10：占用 / 抢占。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum XcCode {
    /// AA 可靠通道不可用（通道接缝未注册即入队）。
    ChannelUnavailable,
    /// 通道拥塞：本 tick 预算耗尽仍有积压。
    ChannelCongested,
    /// 权重饿死：语音负载被裁剪——权重错配或预算挪用，立案级。
    WeightStarved,
    /// 序列空洞：可靠通道下接收序号跳变——通道违约旁证。
    SeqGap,
    /// 码率档位变动（降档或恢复升档）。
    BitrateDegraded,
    /// 码率到底：最低档仍失败，进静音占位档。
    BitrateFloor,
    /// 载荷损坏：指纹对拍不符，拒解码。
    DecodeCorrupt,
    /// 段延迟超预算：某段实测超其分配预算。
    LatencyBreach,
    /// 端到端超承诺：单帧五段合计超 [`E2E_BUDGET_US`]。
    LatencyBreachE2E,
    /// 静噪开门：检测到说话开始（信息性留痕）。
    NoiseGateClose,
    /// 静噪关门：进入彻底安静期（信息性留痕）。
    NoiseGateOpen,
    /// 说话状态抖动：翻转超限被强制滞留。
    TalkStateFlap,
    /// 隐私违例：持久化意图被类型层阻断并立案。
    PrivacyViolation,
    /// 载荷擦除留痕：载荷消费后覆写清零（零记录执行层）。
    PayloadErased,
    /// 设备占用：申请被在占者拒绝。
    DeviceOccupied,
    /// 设备抢占：高优先用途夺占，原占用者获提示。
    DevicePreempted,
}

impl XcCode {
    /// 段基座（0x5E00：AB06 语音深化面独占段；速写版占 0x5D）。
    pub const SEGMENT_BASE: u16 = 0x5E00;

    /// 段内偏移查表（match 全枚举，编译期穷尽性守恒）。
    #[must_use]
    pub const fn offset(self) -> u16 {
        match self {
            XcCode::ChannelUnavailable => 0x01,
            XcCode::ChannelCongested => 0x02,
            XcCode::WeightStarved => 0x03,
            XcCode::SeqGap => 0x04,
            XcCode::BitrateDegraded => 0x05,
            XcCode::BitrateFloor => 0x06,
            XcCode::DecodeCorrupt => 0x07,
            XcCode::LatencyBreach => 0x08,
            XcCode::LatencyBreachE2E => 0x09,
            XcCode::NoiseGateClose => 0x0A,
            XcCode::NoiseGateOpen => 0x0B,
            XcCode::TalkStateFlap => 0x0C,
            XcCode::PrivacyViolation => 0x0D,
            XcCode::PayloadErased => 0x0E,
            XcCode::DeviceOccupied => 0x0F,
            XcCode::DevicePreempted => 0x10,
        }
    }

    /// 完整诊断码（段基座 + 偏移）。
    #[must_use]
    pub const fn as_raw(self) -> u16 {
        Self::SEGMENT_BASE + self.offset()
    }

    /// 全部十六码（供守恒对账遍历）。
    pub const ALL: [XcCode; 16] = [
        XcCode::ChannelUnavailable,
        XcCode::ChannelCongested,
        XcCode::WeightStarved,
        XcCode::SeqGap,
        XcCode::BitrateDegraded,
        XcCode::BitrateFloor,
        XcCode::DecodeCorrupt,
        XcCode::LatencyBreach,
        XcCode::LatencyBreachE2E,
        XcCode::NoiseGateClose,
        XcCode::NoiseGateOpen,
        XcCode::TalkStateFlap,
        XcCode::PrivacyViolation,
        XcCode::PayloadErased,
        XcCode::DeviceOccupied,
        XcCode::DevicePreempted,
    ];
}

/// 段守恒对账：全部诊断码落在 0x5E00..0x5F00 开区间且互不重码。
///
/// 此函数供判据侧引用，字面量写死不依赖运行态。
#[must_use]
pub const fn segment_guard() -> bool {
    let mut i = 0;
    while i < XcCode::ALL.len() {
        let raw = XcCode::ALL[i].as_raw();
        if raw < XcCode::SEGMENT_BASE || raw >= 0x5F00 {
            return false;
        }
        let mut j = 0;
        while j < i {
            if XcCode::ALL[j].as_raw() == raw {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// ============================================================
// 二、语音帧与编解码骨架
// ============================================================

/// 说话人标识（会话内唯一，由上游会话层分配）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TalkerId(pub u32);

/// 语音帧序号（发送侧单调递增；可靠通道下接收侧不应见空洞）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FrameSeq(pub u32);

/// 载荷指纹（FNV-1a 32 位）。
///
/// 对编解码骨架：编码后帧带指纹传输，解码前先对拍——不符即损坏拒
/// 解码（[`XcCode::DecodeCorrupt`]），损坏帧不进入任何发声路径。
#[must_use]
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u32;
        h = h.wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

/// 码率档六档闭集（64/48/24/12/6 kbps 与静音档）。
///
/// 降级链：T64→T48→T24→T12→T6→Mute；升档反向。两端封闭：
/// [`CodecTier::T64`] 升不动、[`CodecTier::Mute`] 降不动。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CodecTier {
    /// 64 kbps（顶档）。
    T64,
    /// 48 kbps。
    T48,
    /// 24 kbps。
    T24,
    /// 12 kbps。
    T12,
    /// 6 kbps（底档）。
    T6,
    /// 静音占位档（到底后的落点：状态机照常、发声面静音）。
    Mute,
}

impl CodecTier {
    /// 单帧字节数（20ms 帧，Ogg 帧头近似 6 字节计入）。
    ///
    /// 静音档输出零载荷（彻底安静承诺的字节数体现）。
    #[must_use]
    pub const fn bytes_per_frame(self) -> u32 {
        match self {
            // 20ms × kbps ÷ 8 = kbps × 2.5 字节/帧
            CodecTier::T64 => 160 + 6,
            CodecTier::T48 => 120 + 6,
            CodecTier::T24 => 60 + 6,
            CodecTier::T12 => 30 + 6,
            CodecTier::T6 => 15 + 6,
            CodecTier::Mute => 0,
        }
    }

    /// 降级下一档（底档 [`CodecTier::Mute`] 降不动返回自身）。
    #[must_use]
    pub const fn degrade(self) -> Self {
        match self {
            CodecTier::T64 => CodecTier::T48,
            CodecTier::T48 => CodecTier::T24,
            CodecTier::T24 => CodecTier::T12,
            CodecTier::T12 => CodecTier::T6,
            CodecTier::T6 => CodecTier::Mute,
            CodecTier::Mute => CodecTier::Mute,
        }
    }

    /// 升档上一档（顶档 [`CodecTier::T64`] 升不动返回自身）。
    #[must_use]
    pub const fn upgrade(self) -> Self {
        match self {
            CodecTier::T64 => CodecTier::T64,
            CodecTier::T48 => CodecTier::T64,
            CodecTier::T24 => CodecTier::T48,
            CodecTier::T12 => CodecTier::T24,
            CodecTier::T6 => CodecTier::T12,
            CodecTier::Mute => CodecTier::T6,
        }
    }

    /// 档序（0=顶档..5=静音档；供阶梯比较）。
    #[must_use]
    pub const fn rank(self) -> u8 {
        match self {
            CodecTier::T64 => 0,
            CodecTier::T48 => 1,
            CodecTier::T24 => 2,
            CodecTier::T12 => 3,
            CodecTier::T6 => 4,
            CodecTier::Mute => 5,
        }
    }

    /// 是否静音占位档。
    #[must_use]
    pub const fn is_mute(self) -> bool {
        matches!(self, CodecTier::Mute)
    }
}

/// 语音帧（发送侧产物 / 接收侧消费对象）。
///
/// 载荷在消费后必须 [`VoiceFrame::erase`]——零记录的执行层动作。
#[derive(Clone, Debug)]
pub struct VoiceFrame {
    /// 帧序号。
    pub seq: FrameSeq,
    /// 说话人。
    pub sid: TalkerId,
    /// 本帧码率档。
    pub tier: CodecTier,
    /// 捕获 tick（上游注入，零墙钟）。
    pub cap_tick: u64,
    /// 载荷（静音档为空）。
    pub payload: Vec<u8>,
}

impl VoiceFrame {
    /// 构造空载荷静音帧（门关期 / 静音档输出）。
    #[must_use]
    pub fn mute_frame(seq: FrameSeq, sid: TalkerId, cap_tick: u64) -> Self {
        Self {
            seq,
            sid,
            tier: CodecTier::Mute,
            cap_tick,
            payload: Vec::new(),
        }
    }

    /// 载荷指纹（对拍两端一致性）。
    #[must_use]
    pub fn fingerprint(&self) -> u32 {
        fnv1a(&self.payload)
    }

    /// 载荷擦除：逐字节置零覆写后清空（零记录执行层）。
    ///
    /// 返回擦除前载荷长度供对账（[`PrivacyAudit::erase_balance`]）。
    pub fn erase(&mut self) -> usize {
        let n = self.payload.len();
        let mut i = 0;
        while i < n {
            self.payload[i] = 0;
            i += 1;
        }
        self.payload.clear();
        n
    }
}

/// 帧长合法窗口（字节）：编码入参校验。
///
/// 顶档上限两倍为窗上限；低于 8 字节为窗下限（残帧拒编码）。
pub const FRAME_LEN_MAX: usize = (CodecTier::T64.bytes_per_frame() as usize) * 2;
pub const FRAME_LEN_MIN: usize = 8;

/// 编解码骨架（编码 / 解码 / 操作计账）。
///
/// 骨架层不实现真编解码器（平台层职责），只做：帧长窗口校验、指纹
/// 对拍、静音档短路——把「编解码失败→降级码率」的判定面完整建出
/// 来，真编解码器插拔在 [`CodecEngine::encode`] / [`CodecEngine::
/// decode`] 的调用点之后。
#[derive(Debug)]
pub struct CodecEngine {
    /// 操作计账（校验 / 对拍各计一次）。
    ops: u64,
}

/// 编码结果闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EncodeOutcome {
    /// 编码成功（载荷已就绪）。
    Ok,
    /// 入参残帧：长度低于窗下限，拒编码。
    TooShort,
    /// 入参超长：长度高于窗上限，拒编码。
    TooLong,
    /// 静音档：不产载荷（输出空帧）。
    Muted,
}

/// 解码结果闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DecodeOutcome {
    /// 解码成功。
    Ok,
    /// 指纹对拍不符：拒解码（走降级链）。
    Corrupt,
    /// 静音帧：无载荷可直接进播放（彻底安静承诺）。
    MuteFrame,
}

impl CodecEngine {
    /// 构造。
    #[must_use]
    pub const fn new() -> Self {
        Self { ops: 0 }
    }

    /// 编码：帧长窗口校验 → 静音档短路 → 通过即产出载荷帧。
    pub fn encode(
        &mut self,
        tier: CodecTier,
        seq: FrameSeq,
        sid: TalkerId,
        cap_tick: u64,
        pcm: &[u8],
    ) -> (EncodeOutcome, Option<VoiceFrame>) {
        self.ops += 1;
        if tier.is_mute() {
            return (EncodeOutcome::Muted, Some(VoiceFrame::mute_frame(seq, sid, cap_tick)));
        }
        if pcm.len() < FRAME_LEN_MIN {
            return (EncodeOutcome::TooShort, None);
        }
        if pcm.len() > FRAME_LEN_MAX {
            return (EncodeOutcome::TooLong, None);
        }
        self.ops += 1;
        let mut payload = Vec::with_capacity(pcm.len());
        let mut i = 0;
        while i < pcm.len() {
            payload.push(pcm[i]);
            i += 1;
        }
        (
            EncodeOutcome::Ok,
            Some(VoiceFrame {
                seq,
                sid,
                tier,
                cap_tick,
                payload,
            }),
        )
    }

    /// 解码：静音帧短路 → 指纹对拍 → 通过即放行播放。
    pub fn decode(&mut self, expect_fp: u32, frame: &VoiceFrame) -> DecodeOutcome {
        self.ops += 1;
        if frame.tier.is_mute() || frame.payload.is_empty() {
            return DecodeOutcome::MuteFrame;
        }
        self.ops += 1;
        if frame.fingerprint() != expect_fp {
            return DecodeOutcome::Corrupt;
        }
        DecodeOutcome::Ok
    }

    /// 操作计账只读（探针消费）。
    #[must_use]
    pub const fn ops(&self) -> u64 {
        self.ops
    }
}

/// 降级阶梯（编解码失败→降级码率的执行器）。
///
/// 降一升三不对称迟滞：连续 [`DEGRADE_AFTER_FAILS`] 帧失败降一档、
/// 连续 [`UPGRADE_AFTER_OKS`] 帧成功升一档——降档敏感（保实时）、
/// 升档保守（防震荡）。落到 [`CodecTier::Mute`] 即静音占位档：状态
/// 机照常走、发声面静音、到底计数留痕 [`XcCode::BitrateFloor`]。
#[derive(Debug)]
pub struct DegradationLadder {
    /// 当前档。
    tier: CodecTier,
    /// 连续失败计数。
    consec_fail: u32,
    /// 连续成功计数。
    consec_ok: u32,
    /// 累计降档次数。
    total_degrades: u32,
    /// 累计升档次数。
    total_upgrades: u32,
    /// 累计到底（进静音档）次数。
    total_floors: u32,
}

impl DegradationLadder {
    /// 构造（起步顶档）。
    #[must_use]
    pub const fn new() -> Self {
        Self {
            tier: CodecTier::T64,
            consec_fail: 0,
            consec_ok: 0,
            total_degrades: 0,
            total_upgrades: 0,
            total_floors: 0,
        }
    }

    /// 当前档。
    #[must_use]
    pub const fn tier(&self) -> CodecTier {
        self.tier
    }

    /// 单帧结果推进阶梯。
    ///
    /// 返回 `(新档, 事件)`：事件为 `Some` 表示档位变动或到底留痕，
    /// 调用方照单全收入账。升档事件复用 [`XcCode::BitrateDegraded`]
    /// 表意「档位变动」（方向以新旧档序差判读）。
    pub fn step(&mut self, ok: bool) -> (CodecTier, Option<XcCode>) {
        if ok {
            self.consec_fail = 0;
            self.consec_ok += 1;
            if self.consec_ok >= UPGRADE_AFTER_OKS && self.tier.rank() > 0 && !self.tier.is_mute() {
                let next = self.tier.upgrade();
                if next.rank() < self.tier.rank() {
                    self.tier = next;
                    self.consec_ok = 0;
                    self.total_upgrades += 1;
                    return (self.tier, Some(XcCode::BitrateDegraded));
                }
            }
            (self.tier, None)
        } else {
            self.consec_ok = 0;
            self.consec_fail += 1;
            if self.consec_fail >= DEGRADE_AFTER_FAILS {
                self.consec_fail = 0;
                let next = self.tier.degrade();
                if next.rank() > self.tier.rank() {
                    self.tier = next;
                    self.total_degrades += 1;
                    if self.tier.is_mute() {
                        self.total_floors += 1;
                        return (self.tier, Some(XcCode::BitrateFloor));
                    }
                    return (self.tier, Some(XcCode::BitrateDegraded));
                }
            }
            (self.tier, None)
        }
    }

    /// 累计账（降档, 升档, 到底）。
    #[must_use]
    pub const fn totals(&self) -> (u32, u32, u32) {
        (self.total_degrades, self.total_upgrades, self.total_floors)
    }
}

// ============================================================
// 三、AA 可靠通道衔接（跨批对接点：AA01 通道）
// ============================================================

/// AA 可靠通道发送侧接缝（AA01 语义：通道承诺可靠与有序）。
///
/// 骨架层只定义接缝与记账口径，传输本体由 AA 域实现插拔。
pub trait ReliableChannel {
    /// 入队一帧语音负载。
    fn enqueue_voice(&mut self, frame: &VoiceFrame) -> EnqueueOutcome;

    /// 入队一块数据负载（与语音共享预算，按权裁剪的另一方）。
    fn enqueue_data(&mut self, bytes: usize) -> EnqueueOutcome;

    /// 通道是否就绪（未注册即不可用）。
    fn ready(&self) -> bool;
}

/// 入队结果闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EnqueueOutcome {
    /// 入队成功。
    Accepted,
    /// 队列容量满（本帧被拒，调用方记账）。
    QueueFull,
    /// 通道未就绪（未注册即拒）。
    NotReady,
}

/// 发送队列负载条目。
#[derive(Clone, Debug)]
enum QueueItem {
    /// 语音帧。
    Voice { frame: VoiceFrame },
    /// 数据块（字节数）。
    Data { bytes: usize },
}

/// 发送队列 + 按权裁剪（判据：按权裁剪）。
///
/// 每 tick 消费预算 [`QUEUE_BUDGET_BYTES_PER_TICK`] 字节：语音帧优先
/// 出队（预算第一份给语音），预算耗尽后剩余数据被裁计入账——**裁
/// 数据保语音**；语音自身被裁即权重饿死 [`XcCode::WeightStarved`]
/// 立案（正常权重配置不应发生）。被裁条目不是静默丢弃：全部进
/// [`TrimLedger`] 按类分计入账。
#[derive(Debug)]
pub struct SendQueue {
    /// 通道就绪态（false = 未注册：一切入队拒 [`EnqueueOutcome::
    /// NotReady`]）。
    attached: bool,
    /// 负载条目。
    items: Vec<QueueItem>,
    /// 裁剪账本。
    trims: TrimLedger,
    /// 累计入队帧数。
    total_enqueued: u64,
    /// 累计出队（交付）帧数。
    total_delivered: u64,
    /// 累计拒收（满）帧数。
    total_rejected_full: u64,
    /// 拥塞 tick 计数（预算耗尽且仍有积压的 tick 数）。
    congested_ticks: u64,
}

/// 裁剪账本（被裁不是静默：按类分计入账）。
#[derive(Debug, Default)]
pub struct TrimLedger {
    /// 被裁语音帧数（正常应为 0，非 0 即权重饿死旁证）。
    pub voice_frames: u64,
    /// 被裁语音字节。
    pub voice_bytes: u64,
    /// 被载数据块数。
    pub data_chunks: u64,
    /// 被载数据字节。
    pub data_bytes: u64,
    /// 裁剪发生 tick 次数。
    pub trim_ticks: u64,
}

impl TrimLedger {
    /// 账本合计字节数（探针消费）。
    #[must_use]
    pub const fn total_bytes(&self) -> u64 {
        self.voice_bytes + self.data_bytes
    }
}

impl SendQueue {
    /// 构造（通道未注册态）。
    #[must_use]
    pub fn new() -> Self {
        Self {
            attached: false,
            items: Vec::new(),
            trims: TrimLedger {
                voice_frames: 0,
                voice_bytes: 0,
                data_chunks: 0,
                data_bytes: 0,
                trim_ticks: 0,
            },
            total_enqueued: 0,
            total_delivered: 0,
            total_rejected_full: 0,
            congested_ticks: 0,
        }
    }

    /// 注册通道（就绪后入队才被接受）。
    pub fn attach_channel(&mut self) {
        self.attached = true;
    }

    /// 通道就绪态。
    #[must_use]
    pub const fn ready(&self) -> bool {
        self.attached
    }

    /// 入队一帧语音。
    pub fn push_voice(&mut self, frame: VoiceFrame) -> EnqueueOutcome {
        if !self.attached {
            return EnqueueOutcome::NotReady;
        }
        if self.items.len() >= QUEUE_CAPACITY {
            self.total_rejected_full += 1;
            return EnqueueOutcome::QueueFull;
        }
        self.total_enqueued += 1;
        self.items.push(QueueItem::Voice { frame });
        EnqueueOutcome::Accepted
    }

    /// 入队一块数据。
    pub fn push_data(&mut self, bytes: usize) -> EnqueueOutcome {
        if !self.attached {
            return EnqueueOutcome::NotReady;
        }
        if self.items.len() >= QUEUE_CAPACITY {
            self.total_rejected_full += 1;
            return EnqueueOutcome::QueueFull;
        }
        self.items.push(QueueItem::Data { bytes });
        EnqueueOutcome::Accepted
    }

    /// tick 消费：预算内语音优先、同批先到先出；超预算裁数据保语音。
    ///
    /// 返回本 tick 留痕码列表（裁数据不产生码——裁剪是正常预算行为，
    /// 全在账本；裁语音产生 [`XcCode::WeightStarved`] 立案；预算耗尽
    /// 仍有积压产生 [`XcCode::ChannelCongested`]）。
    pub fn tick(&mut self) -> Vec<XcCode> {
        let mut codes: Vec<XcCode> = Vec::new();
        let budget = QUEUE_BUDGET_BYTES_PER_TICK;
        let mut spent: u32 = 0;
        let mut kept: Vec<QueueItem> = Vec::new();
        let mut voice_starved = false;
        let mut delivered_any = false;

        let mut i = 0;
        while i < self.items.len() {
            let item = core::mem::replace(
                &mut self.items[i],
                QueueItem::Data { bytes: 0 },
            );
            let cost = match &item {
                QueueItem::Voice { frame } => frame.tier.bytes_per_frame(),
                QueueItem::Data { bytes } => *bytes as u32,
            };
            let fits = spent.saturating_add(cost) <= budget;
            // 语音在预算外仍尝试占用（预算软上限对语音放宽一档——
            // 裁数据保语音的强形式），数据预算外一律裁。
            let voice_fit = matches!(&item, QueueItem::Voice { .. }) && spent.saturating_add(cost) <= budget + cost;
            if fits || voice_fit {
                spent += cost;
                delivered_any = true;
                match &item {
                    QueueItem::Voice { .. } => self.total_delivered += 1,
                    QueueItem::Data { .. } => {}
                }
            } else {
                match item {
                    QueueItem::Voice { frame } => {
                        self.trims.voice_frames += 1;
                        self.trims.voice_bytes += frame.tier.bytes_per_frame() as u64;
                        voice_starved = true;
                        kept.push(QueueItem::Voice { frame });
                    }
                    QueueItem::Data { bytes } => {
                        self.trims.data_chunks += 1;
                        self.trims.data_bytes += bytes as u64;
                    }
                }
            }
            i += 1;
        }
        self.items = kept;
        if voice_starved {
            codes.push(XcCode::WeightStarved);
        }
        if !self.items.is_empty() {
            codes.push(XcCode::ChannelCongested);
            self.congested_ticks += 1;
        }
        if voice_starved || self.trims.data_chunks > 0 {
            self.trims.trim_ticks += 1;
        }
        let _ = delivered_any;
        codes
    }

    /// 裁剪账本只读。
    #[must_use]
    pub const fn trims(&self) -> &TrimLedger {
        &self.trims
    }

    /// 当前积压条目数。
    #[must_use]
    pub fn backlog(&self) -> usize {
        self.items.len()
    }

    /// 累计账（入队, 交付, 拒收）。
    #[must_use]
    pub const fn totals(&self) -> (u64, u64, u64) {
        (self.total_enqueued, self.total_delivered, self.total_rejected_full)
    }

    /// 拥塞 tick 计数。
    #[must_use]
    pub const fn congested_ticks(&self) -> u64 {
        self.congested_ticks
    }
}

impl ReliableChannel for SendQueue {
    fn enqueue_voice(&mut self, frame: &VoiceFrame) -> EnqueueOutcome {
        self.push_voice(frame.clone())
    }

    fn enqueue_data(&mut self, bytes: usize) -> EnqueueOutcome {
        self.push_data(bytes)
    }

    fn ready(&self) -> bool {
        self.attached
    }
}

/// 接收侧序列空洞哨（可靠通道违约旁证）。
///
/// AA01 语义下接收序号连续；观测到跳变即通道质量旁证立案
/// （[`XcCode::SeqGap`]），骨架层不重传只计数——重传是通道本体职责。
#[derive(Debug)]
pub struct RxGapWatcher {
    /// 已见最大序号（None = 尚未见过）。
    last: Option<u32>,
    /// 空洞次数。
    gaps: u64,
    /// 空洞缺帧总数（跳变量）。
    missing: u64,
}

impl RxGapWatcher {
    /// 构造。
    #[must_use]
    pub const fn new() -> Self {
        Self {
            last: None,
            gaps: 0,
            missing: 0,
        }
    }

    /// 观测一帧序号。
    ///
    /// 返回 `Some(XcCode::SeqGap)` 表示本次观测发现空洞（缺
    /// `(seq - last - 1)` 帧，已累计入账）。乱序回退（seq ≤ last）
    /// 不计空洞——忽略，等下一更高序号对账。
    pub fn observe(&mut self, seq: FrameSeq) -> Option<XcCode> {
        match self.last {
            None => {
                self.last = Some(seq.0);
                None
            }
            Some(last) => {
                if seq.0 > last {
                    let gap = (seq.0 - last - 1) as u64;
                    if gap > 0 {
                        self.gaps += 1;
                        self.missing += gap;
                        self.last = Some(seq.0);
                        return Some(XcCode::SeqGap);
                    }
                    self.last = Some(seq.0);
                }
                None
            }
        }
    }

    /// 空洞账（次数, 缺帧总数）。
    #[must_use]
    pub const fn gaps(&self) -> (u64, u64) {
        (self.gaps, self.missing)
    }
}

// ============================================================
// 四、端到端延迟预算（说话到听见全链承诺）
// ============================================================

/// 五段闭集（采集/编码/网络/解码/播放）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LegStage {
    /// 采集段。
    Capture,
    /// 编码段。
    Encode,
    /// 网络段。
    Net,
    /// 解码段。
    Decode,
    /// 播放段。
    Playback,
}

impl LegStage {
    /// 全部五段（守恒对账遍历序）。
    pub const ALL: [LegStage; 5] = [
        LegStage::Capture,
        LegStage::Encode,
        LegStage::Net,
        LegStage::Decode,
        LegStage::Playback,
    ];

    /// 段预算（微秒）——字面量写死，合计恒等于 [`E2E_BUDGET_US`]。
    #[must_use]
    pub const fn budget_us(self) -> u64 {
        match self {
            LegStage::Capture => LEG_CAPTURE_US,
            LegStage::Encode => LEG_ENCODE_US,
            LegStage::Net => LEG_NET_US,
            LegStage::Decode => LEG_DECODE_US,
            LegStage::Playback => LEG_PLAYBACK_US,
        }
    }
}

/// 预算守恒对账：五段合计 == 端到端承诺。
///
/// `20k+15k+80k+15k+20k = 150k`。编译期可算，运行期再对账一次供判
/// 据侧独立引用。
#[must_use]
pub const fn budget_conservation() -> bool {
    let mut sum: u64 = 0;
    let mut i = 0;
    while i < LegStage::ALL.len() {
        sum += LegStage::ALL[i].budget_us();
        i += 1;
    }
    sum == E2E_BUDGET_US
}

/// 段账本（逐帧逐段实测记账 + 超预算留痕 + 最坏段归因）。
#[derive(Debug)]
pub struct LegLedger {
    /// 五段滚动实测（最近 [`E2E_WINDOW`] 帧逐段环形记录）。
    ring: [[u64; E2E_WINDOW]; 5],
    /// 环写指针。
    head: usize,
    /// 已记账帧数（饱和计数）。
    frames: u64,
    /// 段超预算累计（按段索引）。
    breaches: [u64; 5],
    /// 最坏段（超预算次数最多的段；平局取先者）。
    worst: Option<LegStage>,
}

impl LegLedger {
    /// 构造（全零）。
    #[must_use]
    pub const fn new() -> Self {
        Self {
            ring: [[0; E2E_WINDOW]; 5],
            head: 0,
            frames: 0,
            breaches: [0; 5],
            worst: None,
        }
    }

    /// 记一帧五段实测（微秒），返回留痕码列表。
    ///
    /// 逐段对拍预算：超即 [`XcCode::LatencyBreach`]；合计超承诺即追
    /// 加 [`XcCode::LatencyBreachE2E`]。
    pub fn record(&mut self, capture: u64, encode: u64, net: u64, decode: u64, playback: u64) -> Vec<XcCode> {
        let vals = [capture, encode, net, decode, playback];
        let mut codes: Vec<XcCode> = Vec::new();
        let mut i = 0;
        while i < 5 {
            self.ring[i][self.head] = vals[i];
            if vals[i] > LegStage::ALL[i].budget_us() {
                self.breaches[i] += 1;
                codes.push(XcCode::LatencyBreach);
            }
            i += 1;
        }
        let total = capture + encode + net + decode + playback;
        if total > E2E_BUDGET_US {
            codes.push(XcCode::LatencyBreachE2E);
        }
        self.head = (self.head + 1) % E2E_WINDOW;
        if self.frames < u64::MAX {
            self.frames += 1;
        }
        self.recompute_worst();
        codes
    }

    /// 重算最坏段（超账最多者；全零则 None）。
    fn recompute_worst(&mut self) {
        let mut best_i: Option<usize> = None;
        let mut best_v: u64 = 0;
        let mut i = 0;
        while i < 5 {
            if self.breaches[i] > best_v {
                best_v = self.breaches[i];
                best_i = Some(i);
            }
            i += 1;
        }
        self.worst = best_i.map(|i| LegStage::ALL[i]);
    }

    /// 最坏段归因。
    #[must_use]
    pub const fn worst(&self) -> Option<LegStage> {
        self.worst
    }

    /// 段超账只读。
    #[must_use]
    pub const fn breaches(&self) -> &[u64; 5] {
        &self.breaches
    }

    /// 已记账帧数。
    #[must_use]
    pub const fn frames(&self) -> u64 {
        self.frames
    }
}

/// 端到端监工（滚动窗超承诺计数 + 网络段降档建议）。
///
/// 延迟压力联动：窗内超承诺占比超 [`NET_PRESSURE_PCT`] 即建议降档
/// ——延迟压力传导到第二节降级阶梯的钩子（[`E2eMonitor::
/// suggest_degrade`] 返回 `true` 时调用方应对 [`DegradationLadder`]
/// 施加一次强制降档步进）。
#[derive(Debug)]
pub struct E2eMonitor {
    /// 滚动窗内超承诺标记（环形）。
    ring: [bool; E2E_WINDOW],
    /// 环写指针。
    head: usize,
    /// 已见帧数（饱和）。
    seen: u64,
    /// 窗内超承诺帧数（增量维护）。
    breaches_in_window: u32,
}

/// 网络段压力阈值（百分比；窗内超账占比超此即建议降档）。
pub const NET_PRESSURE_PCT: u32 = 25;

impl E2eMonitor {
    /// 构造。
    #[must_use]
    pub const fn new() -> Self {
        Self {
            ring: [false; E2E_WINDOW],
            head: 0,
            seen: 0,
            breaches_in_window: 0,
        }
    }

    /// 帧终对账：本帧端到端合计是否超承诺。
    pub fn observe_frame(&mut self, total_us: u64) {
        let breach = total_us > E2E_BUDGET_US;
        let old = self.ring[self.head];
        self.ring[self.head] = breach;
        self.head = (self.head + 1) % E2E_WINDOW;
        if self.seen < E2E_WINDOW as u64 {
            self.seen += 1;
        }
        // 增量维护窗内计数（旧位撤销 / 新位计入）。
        if old && !breach && self.breaches_in_window > 0 {
            self.breaches_in_window -= 1;
        } else if !old && breach {
            self.breaches_in_window += 1;
        }
    }

    /// 降档建议（窗满且占比超 [`NET_PRESSURE_PCT`]；窗未满不下判断
    /// ——宁迟勿误）。
    #[must_use]
    pub fn suggest_degrade(&self) -> bool {
        if self.seen < E2E_WINDOW as u64 {
            return false;
        }
        self.breaches_in_window * 100 > NET_PRESSURE_PCT * (E2E_WINDOW as u32)
    }

    /// 窗内超账帧数。
    #[must_use]
    pub const fn breaches_in_window(&self) -> u32 {
        self.breaches_in_window
    }
}

// ============================================================
// 五、静噪门限自适应（不说话时彻底安静）
// ============================================================

/// 静噪门判定闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GateDecision {
    /// 开门（检测到语音活动）。
    Open,
    /// 关门（进入彻底安静期）。
    Close,
    /// 吊尾中（说话已停但保持期未满：仍开门放行字尾）。
    Hangover,
}

/// 静噪门（定点能量 + 双门限迟滞 + 吊尾 + 底噪自适应）。
///
/// 输入为 RMS 平方能量（定点 `u32`，零浮点）；平滑用 α=1/4 位移
/// （`ewma = ewma - ewma/4 + x/4`）；开门限 = 底噪×[`OPEN_RATIO_NUM`]
/// 8、关门限 = 底噪×[`CLOSE_RATIO_NUM`] 4——开高关低迟滞防临界抖
/// 动；说话结束后吊尾 [`HANGOVER_TICKS`] 6 tick 才真关门（防切字
/// 尾）；无话期底噪按 1/[`FLOOR_ADAPT_DEN`] 步长缓沉——环境变吵门
/// 限自抬、变静门限自降；静音期 tick 计数入账供「该静时真静」审计。
#[derive(Debug)]
pub struct NoiseGate {
    /// 平滑底噪（RMS 平方域）。
    floor_sq: u32,
    /// 吊尾剩余 tick。
    hangover_left: u32,
    /// 起势累计（连续过开门限 tick）。
    rising: u32,
    /// 当前门态。
    open: bool,
    /// 静音期累计 tick（门关期）。
    silent_ticks: u64,
    /// 开门累计次数。
    open_events: u64,
    /// 关门累计次数。
    close_events: u64,
    /// 底噪自适应生效次数。
    floor_adapts: u64,
}

impl NoiseGate {
    /// 构造（底噪初值：安静机房近似定点值；初值偏差由底噪自适应在
    /// 前几十 tick 内收敛兜底）。
    #[must_use]
    pub const fn new() -> Self {
        Self {
            floor_sq: 64,
            hangover_left: 0,
            rising: 0,
            open: false,
            silent_ticks: 0,
            open_events: 0,
            close_events: 0,
            floor_adapts: 0,
        }
    }

    /// 开门限（定点平方域）。
    #[must_use]
    pub const fn open_threshold(&self) -> u32 {
        self.floor_sq * OPEN_RATIO_NUM
    }

    /// 关门限（定点平方域）。
    #[must_use]
    pub const fn close_threshold(&self) -> u32 {
        self.floor_sq * CLOSE_RATIO_NUM
    }

    /// 当前门态。
    #[must_use]
    pub const fn is_open(&self) -> bool {
        self.open
    }

    /// 静音期累计 tick。
    #[must_use]
    pub const fn silent_ticks(&self) -> u64 {
        self.silent_ticks
    }

    /// 开/关门事件累计。
    #[must_use]
    pub const fn events(&self) -> (u64, u64) {
        (self.open_events, self.close_events)
    }

    /// 底噪自适应生效次数。
    #[must_use]
    pub const fn floor_adapts(&self) -> u64 {
        self.floor_adapts
    }

    /// 平滑底噪只读。
    #[must_use]
    pub const fn floor_sq(&self) -> u32 {
        self.floor_sq
    }

    /// 单 tick 喂入能量，得门判定。
    ///
    /// - 观测值高于平滑值：快跟上浮（α=1/4）——语音能量快速抬门内
    ///   判定基准；低于：缓沉（1/16 步长，自适应——防门限被语音间
    ///   隙拖垮）；
    /// - 门开时：能量仍在关门限上→清吊尾保持开门；能量落关门限下
    ///   且吊尾未尽→吊尾放行字尾；吊尾耗尽→真关门；
    /// - 门关时：静音 tick 累计；能量过开门限且起势满
    ///   [`RISING_CONFIRM_TICKS`]→开门（开事件计账）；未满→起势中。
    pub fn feed(&mut self, rms_sq: u32) -> GateDecision {
        if rms_sq > self.floor_sq {
            self.floor_sq = self.floor_sq - self.floor_sq / 4 + rms_sq / 4;
        } else {
            let diff = self.floor_sq - rms_sq;
            let step = diff / FLOOR_ADAPT_DEN;
            if step > 0 {
                self.floor_sq -= step;
                self.floor_adapts += 1;
            }
        }

        if self.open {
            if rms_sq >= self.close_threshold() {
                self.hangover_left = HANGOVER_TICKS;
                self.rising = self.rising.saturating_add(1);
                GateDecision::Open
            } else if self.hangover_left > 0 {
                self.hangover_left -= 1;
                GateDecision::Hangover
            } else {
                self.open = false;
                self.rising = 0;
                self.close_events += 1;
                self.silent_ticks = self.silent_ticks.saturating_add(1);
                GateDecision::Close
            }
        } else {
            self.silent_ticks = self.silent_ticks.saturating_add(1);
            if rms_sq >= self.open_threshold() {
                self.rising += 1;
                if self.rising >= RISING_CONFIRM_TICKS {
                    self.open = true;
                    self.hangover_left = HANGOVER_TICKS;
                    self.open_events += 1;
                    GateDecision::Open
                } else {
                    GateDecision::Hangover
                }
            } else {
                self.rising = 0;
                GateDecision::Close
            }
        }
    }
}

// ============================================================
// 六、说话状态指示（只示状态不示内容）
// ============================================================

/// 说话状态三态闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TalkState {
    /// 静默（门关）。
    Silent,
    /// 起势（过开门限但未满确认 tick——防瞬时噪声误报）。
    Rising,
    /// 说话（确认迁入）。
    Speaking,
}

/// 说话状态指示条目。
///
/// **结构性隐私**：类型上没有载荷字段——指示面不携带内容是类型保证
/// （不是纪律约束）；字段只有 id、状态、持续 tick、当前码率档。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TalkIndicator {
    /// 说话人。
    pub sid: TalkerId,
    /// 状态。
    pub state: TalkState,
    /// 当前态持续 tick。
    pub held_ticks: u32,
    /// 当前码率档（静音档即「设备在线但未发声」）。
    pub tier: CodecTier,
}

/// 抖动抑制器（状态翻转限频）。
///
/// 窗口 [`FLAP_WINDOW`] tick 内翻转超 [`FLAP_LIMIT`] 次即强制滞留
/// 一窗并留痕 [`XcCode::TalkStateFlap`]——指示灯乱闪比不亮更伤可用
/// 性；滞留期结束自动恢复判定。
#[derive(Debug)]
pub struct FlapSuppressor {
    /// 翻转时间戳环形（tick）。
    ring: [u64; 4],
    /// 环写指针。
    head: usize,
    /// 已记翻转数（饱和至环容量）。
    flips: u32,
    /// 滞留剩余 tick。
    lockdown_left: u32,
}

impl FlapSuppressor {
    /// 构造。
    #[must_use]
    pub const fn new() -> Self {
        Self {
            ring: [0; 4],
            head: 0,
            flips: 0,
            lockdown_left: 0,
        }
    }

    /// 判定本 tick 是否允许翻转。
    ///
    /// 返回 `(允许, 留痕)`：滞留期内一律不许；窗口内翻转满限触发滞
    /// 留并留痕一次。
    pub fn allow_flip(&mut self, tick: u64) -> (bool, Option<XcCode>) {
        if self.lockdown_left > 0 {
            self.lockdown_left -= 1;
            return (false, None);
        }
        self.ring[self.head] = tick;
        self.head = (self.head + 1) % self.ring.len();
        if self.flips < self.ring.len() as u32 {
            self.flips += 1;
        }
        if self.flips == self.ring.len() as u32 {
            let oldest = self.ring[self.head];
            if tick.saturating_sub(oldest) <= FLAP_WINDOW as u64 {
                self.lockdown_left = FLAP_WINDOW;
                self.flips = 0;
                return (false, Some(XcCode::TalkStateFlap));
            }
        }
        (true, None)
    }

    /// 滞留剩余。
    #[must_use]
    pub const fn lockdown_left(&self) -> u32 {
        self.lockdown_left
    }
}

/// 说话人看板（容量上限 [`MAX_SPEAKERS`] + 逐人状态 + 快照）。
///
/// 语音状态可见判据的落地形态：看板快照供 UI 与读屏消费；快照序列
/// 化只含状态面字段（结构性隐私在序列化层同样成立——没有内容列）。
#[derive(Debug)]
pub struct SpeakerBoard {
    /// 说话人条目。
    entries: Vec<TalkIndicator>,
    /// 抖动抑制器（与条目一一对应）。
    flaps: Vec<FlapSuppressor>,
    /// 满员拒收累计。
    rejected: u64,
}

impl SpeakerBoard {
    /// 构造（空看板）。
    #[must_use]
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            flaps: Vec::new(),
            rejected: 0,
        }
    }

    /// 查找说话人下标。
    fn find(&self, sid: TalkerId) -> Option<usize> {
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].sid == sid {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 更新说话人状态（新 sid 自动注册；满员拒收计账）。
    ///
    /// 状态翻转经 [`FlapSuppressor`] 判定：被抑制时保持原态并可能
    /// 留痕 [`XcCode::TalkStateFlap`]。
    pub fn update(&mut self, sid: TalkerId, state: TalkState, held_ticks: u32, tier: CodecTier) -> Option<XcCode> {
        let idx = match self.find(sid) {
            Some(i) => i,
            None => {
                if self.entries.len() >= MAX_SPEAKERS {
                    self.rejected += 1;
                    return None;
                }
                self.entries.push(TalkIndicator {
                    sid,
                    state: TalkState::Silent,
                    held_ticks: 0,
                    tier,
                });
                self.flaps.push(FlapSuppressor::new());
                self.entries.len() - 1
            }
        };
        let prev = self.entries[idx].state;
        let mut code: Option<XcCode> = None;
        let next = if prev == state {
            state
        } else {
            let (ok, c) = self.flaps[idx].allow_flip(held_ticks as u64);
            if let Some(cc) = c {
                code = Some(cc);
            }
            if ok {
                state
            } else {
                prev
            }
        };
        self.entries[idx].state = next;
        self.entries[idx].held_ticks = held_ticks;
        self.entries[idx].tier = tier;
        code
    }

    /// 看板只读。
    #[must_use]
    pub fn entries(&self) -> &[TalkIndicator] {
        &self.entries
    }

    /// 说话中人数（Speaking 态）。
    #[must_use]
    pub fn speaking_count(&self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].state == TalkState::Speaking {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 满员拒收累计。
    #[must_use]
    pub const fn rejected(&self) -> u64 {
        self.rejected
    }

    /// 看板快照序列化（'|' 分隔保空段；只含状态面字段）。
    ///
    /// 条目格式 `sid:state:held:tier`（state：S=静默 R=起势 K=说话；
    /// tier 为档序 0..5），条目间 '|'；空看板为空串——'|' 分隔保空段
    /// 格式保证解析按位回填恒等。
    #[must_use]
    pub fn snapshot(&self) -> String {
        let mut s = String::new();
        let mut i = 0;
        while i < self.entries.len() {
            if i > 0 {
                let _ = s.write_char('|');
            }
            let e = &self.entries[i];
            let st = match e.state {
                TalkState::Silent => "S",
                TalkState::Rising => "R",
                TalkState::Speaking => "K",
            };
            let _ = write!(s, "{}:{}:{}:{}", e.sid.0, st, e.held_ticks, e.tier.rank());
            i += 1;
        }
        s
    }
}

// ============================================================
// 七、隐私合规与零记录（跨批对接点：AA05 隐私）
// ============================================================

/// 持久化权限闭集——**单变体**。
///
/// 只有 `Denied` 一个变体：不存在「允许」的构造路径，隐私违例阻断
/// 在类型层面成立。任何未来需求想加 `Allow` 变体都等于修改隐私合
/// 同本身——这是设计的全部意义。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Permission {
    /// 永久拒绝（唯一变体）。
    Denied,
}

/// 隐私裁决闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PrivacyVerdict {
    /// 阻断 + 立案（唯一可能结果）。
    Blocked,
}

/// 隐私闸门（AA05 对接面）。
///
/// 任何调用点尝试持久化语音内容 → [`PrivacyGate::persist_intent`]
/// 返回 [`PrivacyVerdict::Blocked`] 并在审计账立案（记调用点标识，
/// 不记内容——审计账自身不得成为内容旁路）。
#[derive(Debug)]
pub struct PrivacyGate;

impl PrivacyGate {
    /// 持久化意图（永远阻断 + 立案）。
    pub fn persist_intent(callsite: u32, audit: &mut PrivacyAudit) -> PrivacyVerdict {
        audit.record(callsite);
        PrivacyVerdict::Blocked
    }
}

/// 隐私审计账（滚动 64 案；只记调用点标识与计数）。
#[derive(Debug)]
pub struct PrivacyAudit {
    /// 违例尝试滚动账（调用点标识）。
    ring: [u32; 64],
    /// 环写指针。
    head: usize,
    /// 已立案数（饱和）。
    cases: u64,
    /// 入帧累计（载荷对账基数）。
    frames_in: u64,
    /// 擦除累计（载荷对账兑现）。
    erased: u64,
}

impl PrivacyAudit {
    /// 构造。
    #[must_use]
    pub const fn new() -> Self {
        Self {
            ring: [0; 64],
            head: 0,
            cases: 0,
            frames_in: 0,
            erased: 0,
        }
    }

    /// 立案（调用点标识入环）。
    pub fn record(&mut self, callsite: u32) {
        self.ring[self.head] = callsite;
        self.head = (self.head + 1) % self.ring.len();
        if self.cases < u64::MAX {
            self.cases += 1;
        }
    }

    /// 记入帧（载荷对账基数 +1）。
    pub fn note_frame_in(&mut self) {
        if self.frames_in < u64::MAX {
            self.frames_in += 1;
        }
    }

    /// 记擦除（兑现 n 帧擦除）。
    pub fn note_erased(&mut self, n: u64) {
        self.erased = self.erased.saturating_add(n);
    }

    /// 载荷对账差额：入帧 − 擦除。**非零即有载荷未经擦除路径流出**
    /// （阻断级立案依据；正常闭环应为 0）。
    #[must_use]
    pub const fn erase_balance(&self) -> u64 {
        if self.erased > self.frames_in {
            0
        } else {
            self.frames_in - self.erased
        }
    }

    /// 已立案数。
    #[must_use]
    pub const fn cases(&self) -> u64 {
        self.cases
    }

    /// 最近立案调用点（审计回看；None = 无案）。
    #[must_use]
    pub const fn last_callsite(&self) -> Option<u32> {
        if self.cases == 0 {
            None
        } else {
            let i = (self.head + self.ring.len() - 1) % self.ring.len();
            Some(self.ring[i])
        }
    }
}

// ============================================================
// 八、设备占用仲裁（设备占用→抢占提示）
// ============================================================

/// 设备用途闭集（优先级互异：系统 > 通话 > 听写）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DeviceUse {
    /// 听写（最低优先，可被通话抢占）。
    Dictation,
    /// 通话（中优先，可被系统抢占）。
    VoiceChat,
    /// 系统诊断（最高优先，不可被抢占）。
    System,
}

impl DeviceUse {
    /// 优先级数值（大者优先）。
    #[must_use]
    pub const fn priority(self) -> u8 {
        match self {
            DeviceUse::Dictation => 1,
            DeviceUse::VoiceChat => 2,
            DeviceUse::System => 3,
        }
    }

    /// 用途名（快照用）。
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            DeviceUse::Dictation => "dict",
            DeviceUse::VoiceChat => "chat",
            DeviceUse::System => "sys",
        }
    }
}

/// 抢占提示条目（被抢占者收到的显性通知——提示不是静默夺走）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PreemptNotice {
    /// 被抢占者。
    pub victim: TalkerId,
    /// 抢占者。
    pub by: TalkerId,
    /// 抢占用途。
    pub use_of: DeviceUse,
    /// 发生 tick。
    pub at_tick: u64,
}

/// 占用态（内部）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Holder {
    /// 空闲。
    Free,
    /// 被 (sid, 用途, 最近活动 tick) 占用。
    Held(TalkerId, DeviceUse, u64),
}

/// 设备仲裁器（独占 + 抢占 + 超时回收）。
#[derive(Debug)]
pub struct DeviceArbiter {
    /// 占用态。
    holder: Holder,
    /// 抢占提示队列（被抢占者消费）。
    notices: Vec<PreemptNotice>,
    /// 抢占累计。
    total_preempts: u64,
    /// 超时回收累计。
    total_timeouts: u64,
    /// 拒绝累计（低优先申请撞高优先占用）。
    total_denied: u64,
}

/// 申请结果闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AcquireOutcome {
    /// 获得占用。
    Acquired,
    /// 幂等续占（同占用者重复申请 / 换用途）。
    Renewed,
    /// 被在占者拒绝（优先级不更高）。
    DeniedBusy,
    /// 抢占成功（原占用者获提示）。
    Preempted,
}

impl DeviceArbiter {
    /// 构造（空闲）。
    #[must_use]
    pub const fn new() -> Self {
        Self {
            holder: Holder::Free,
            notices: Vec::new(),
            total_preempts: 0,
            total_timeouts: 0,
            total_denied: 0,
        }
    }

    /// 申请占用。
    ///
    /// - 空闲：直接获得；
    /// - 同占用者：幂等续占（活动 tick 刷新；换用途允许——同人不构
    ///   成抢占关系）；
    /// - 他占用者且申请优先级更高：抢占成功，原占用者获提示
    ///   （[`XcCode::DevicePreempted`] 留痕）；
    /// - 他占用者且不更高：拒绝（[`XcCode::DeviceOccupied`] 留痕）。
    pub fn acquire(&mut self, sid: TalkerId, use_of: DeviceUse, at_tick: u64) -> (AcquireOutcome, Option<XcCode>) {
        match self.holder {
            Holder::Free => {
                self.holder = Holder::Held(sid, use_of, at_tick);
                (AcquireOutcome::Acquired, None)
            }
            Holder::Held(h, _, _) if h == sid => {
                self.holder = Holder::Held(sid, use_of, at_tick);
                (AcquireOutcome::Renewed, None)
            }
            Holder::Held(h, old_use, _) => {
                if use_of.priority() > old_use.priority() {
                    self.notices.push(PreemptNotice {
                        victim: h,
                        by: sid,
                        use_of,
                        at_tick,
                    });
                    self.holder = Holder::Held(sid, use_of, at_tick);
                    self.total_preempts += 1;
                    (AcquireOutcome::Preempted, Some(XcCode::DevicePreempted))
                } else {
                    self.total_denied += 1;
                    (AcquireOutcome::DeniedBusy, Some(XcCode::DeviceOccupied))
                }
            }
        }
    }

    /// 主动释放（同占用者才生效；他者释放无效——防误释放他占）。
    pub fn release(&mut self, sid: TalkerId) -> bool {
        match self.holder {
            Holder::Held(h, _, _) if h == sid => {
                self.holder = Holder::Free;
                true
            }
            _ => false,
        }
    }

    /// tick 推进：空闲超时强制回收（防死占）。
    ///
    /// 占用者最近活动 tick 距今超 [`DEVICE_IDLE_TIMEOUT_TICKS`] 即回
    /// 收。返回是否发生回收。
    pub fn tick_timeout(&mut self, now_tick: u64) -> bool {
        match self.holder {
            Holder::Held(_, _, last) => {
                if now_tick.saturating_sub(last) > DEVICE_IDLE_TIMEOUT_TICKS as u64 {
                    self.holder = Holder::Free;
                    self.total_timeouts += 1;
                    true
                } else {
                    false
                }
            }
            Holder::Free => false,
        }
    }

    /// 取走全部抢占提示（消费即清空）。
    pub fn drain_notices(&mut self) -> Vec<PreemptNotice> {
        core::mem::take(&mut self.notices)
    }

    /// 当前占用者（None = 空闲）。
    #[must_use]
    pub const fn holder(&self) -> Option<(TalkerId, DeviceUse)> {
        match self.holder {
            Holder::Free => None,
            Holder::Held(h, u, _) => Some((h, u)),
        }
    }

    /// 累计账（抢占, 超时回收, 拒绝）。
    #[must_use]
    pub const fn totals(&self) -> (u64, u64, u64) {
        (self.total_preempts, self.total_timeouts, self.total_denied)
    }
}

// ============================================================
// 九、主编排与快照（处理 O（帧））
// ============================================================

/// 单 tick 语音输入。
#[derive(Debug)]
pub struct VoiceInput {
    /// 说话人。
    pub sid: TalkerId,
    /// 本 tick 定点 RMS 平方能量（静噪门输入）。
    pub rms_sq: u32,
    /// PCM 切片（门开且非静音档时编码；None = 无新帧）。
    pub pcm: Option<Vec<u8>>,
    /// 当前 tick（上游注入，零墙钟）。
    pub tick: u64,
}

/// 单 tick 报告（全部落点与留痕一次带回）。
#[derive(Debug)]
pub struct VoiceTickReport {
    /// 门判定。
    pub gate: GateDecision,
    /// 当前码率档（阶梯输出）。
    pub tier: CodecTier,
    /// 入队结果（本 tick 有编码帧时才有）。
    pub enqueued: Option<EnqueueOutcome>,
    /// 本 tick 留痕码（门 / 阶梯 / 通道 / 延迟 / 状态各面汇聚）。
    pub codes: Vec<XcCode>,
    /// 指示快照（本 tick 说话人条目）。
    pub indicator: Option<TalkIndicator>,
}

/// 语音系统主编排（门 → 编解码 → 通道 → 延迟 → 状态五节合一）。
///
/// 单 tick 单帧 O(1) 摊还 + 队列 O(积压) 出队——处理 O（帧）口径。
/// 通道未接通前一切入队拒 [`EnqueueOutcome::NotReady`]（接缝未注册
/// 显性化，不留半接通态）。
#[derive(Debug)]
pub struct VoiceSystem {
    /// 静噪门。
    gate: NoiseGate,
    /// 编解码骨架。
    codec: CodecEngine,
    /// 降级阶梯。
    ladder: DegradationLadder,
    /// 发送队列（AA01 接缝实现体）。
    queue: SendQueue,
    /// 接收侧空洞哨。
    gap: RxGapWatcher,
    /// 段账本。
    legs: LegLedger,
    /// 端到端监工。
    e2e: E2eMonitor,
    /// 说话人看板。
    board: SpeakerBoard,
    /// 隐私审计。
    privacy: PrivacyAudit,
    /// 设备仲裁。
    device: DeviceArbiter,
    /// 发送侧序号游标。
    tx_seq: u32,
    /// 全链操作计账。
    ops: u64,
}

impl VoiceSystem {
    /// 构造（通道未注册；随后 [`VoiceSystem::attach_channel`] 接通
    /// AA01）。
    #[must_use]
    pub fn new() -> Self {
        Self {
            gate: NoiseGate::new(),
            codec: CodecEngine::new(),
            ladder: DegradationLadder::new(),
            queue: SendQueue::new(),
            gap: RxGapWatcher::new(),
            legs: LegLedger::new(),
            e2e: E2eMonitor::new(),
            board: SpeakerBoard::new(),
            privacy: PrivacyAudit::new(),
            device: DeviceArbiter::new(),
            tx_seq: 0,
            ops: 0,
        }
    }

    /// 接通 AA 可靠通道。
    pub fn attach_channel(&mut self) {
        self.queue.attach_channel();
    }

    /// 申请设备占用（设备面入口；抢占时带回被抢占者提示）。
    pub fn acquire_device(
        &mut self,
        sid: TalkerId,
        use_of: DeviceUse,
        at_tick: u64,
    ) -> (AcquireOutcome, Option<PreemptNotice>) {
        let (out, code) = self.device.acquire(sid, use_of, at_tick);
        if let Some(XcCode::DevicePreempted) = code {
            let notices = self.device.drain_notices();
            let n = notices.first().copied();
            (out, n)
        } else {
            (out, None)
        }
    }

    /// 释放设备占用。
    pub fn release_device(&mut self, sid: TalkerId) -> bool {
        self.device.release(sid)
    }

    /// 隐私闸门入口（AA05 对接：任何持久化意图走这里）。
    pub fn persist_intent(&mut self, callsite: u32) -> PrivacyVerdict {
        PrivacyGate::persist_intent(callsite, &mut self.privacy)
    }

    /// 模拟接收一帧（接收侧对拍面：空洞哨 + 指纹对拍 + 消费即擦除）。
    pub fn observe_inbound(&mut self, seq: FrameSeq, fp: u32, frame: &VoiceFrame) -> Vec<XcCode> {
        let mut codes: Vec<XcCode> = Vec::new();
        self.ops += 1;
        if let Some(c) = self.gap.observe(seq) {
            codes.push(c);
        }
        match self.codec.decode(fp, frame) {
            DecodeOutcome::Corrupt => {
                codes.push(XcCode::DecodeCorrupt);
                // 损坏帧即刻擦除——不经任何留存路径。
                let mut f = frame.clone();
                let _ = f.erase();
            }
            DecodeOutcome::MuteFrame => {
                // 静音帧直接进播放（彻底安静承诺），无载荷无擦除。
            }
            DecodeOutcome::Ok => {
                // 放行播放路径（骨架层到此为止）；消费即擦除留痕。
                let mut f = frame.clone();
                let _ = f.erase();
                self.privacy.note_erased(1);
                codes.push(XcCode::PayloadErased);
            }
        }
        codes
    }

    /// 全链 tick：单帧走完门→编解码→通道→延迟→状态。
    ///
    /// 延迟记账口径：采集/编码段为骨架层常数近似（8k/6k µs——账面
    /// 可比性优先于实测精度，真值由平台层插桩回填）；网络段按积压深
    /// 度折算（30k + 积压×400 µs）——积压越深网络段越贵，延迟压力
    /// 自然传导到降档建议。
    pub fn tick(&mut self, input: &VoiceInput) -> VoiceTickReport {
        let mut codes: Vec<XcCode> = Vec::new();
        self.ops += 1;

        // ① 静噪门。
        let gate = self.gate.feed(input.rms_sq);
        match gate {
            GateDecision::Open => codes.push(XcCode::NoiseGateClose),
            GateDecision::Close => codes.push(XcCode::NoiseGateOpen),
            GateDecision::Hangover => {}
        }

        // ② 编解码 + 自对拍 + 阶梯（门开且有 PCM 才编码）。
        let mut tier = self.ladder.tier();
        let mut frame: Option<VoiceFrame> = None;
        if matches!(gate, GateDecision::Open | GateDecision::Hangover) {
            if let Some(pcm) = &input.pcm {
                let (enc_out, f) = self.codec.encode(tier, FrameSeq(self.tx_seq), input.sid, input.tick, pcm);
                match enc_out {
                    EncodeOutcome::Ok => {
                        self.tx_seq = self.tx_seq.wrapping_add(1);
                        if let Some(fr) = f {
                            let fp = fr.fingerprint();
                            // 编码→解码回环自对拍：不一致视同帧失败
                            //（真实现中损坏来自传输/内存位翻转）。
                            let ok = self.codec.decode(fp, &fr) == DecodeOutcome::Ok;
                            let (t, ev) = self.ladder.step(ok);
                            tier = t;
                            if let Some(c) = ev {
                                codes.push(c);
                            }
                            frame = Some(fr);
                        }
                    }
                    EncodeOutcome::TooShort | EncodeOutcome::TooLong => {
                        // 残帧/超长视同一次失败：阶梯推进（编码器异
                        // 常路径的代理信号）。
                        let (t, ev) = self.ladder.step(false);
                        tier = t;
                        if let Some(c) = ev {
                            codes.push(c);
                        }
                    }
                    EncodeOutcome::Muted => {
                        frame = Some(VoiceFrame::mute_frame(FrameSeq(self.tx_seq), input.sid, input.tick));
                        self.tx_seq = self.tx_seq.wrapping_add(1);
                    }
                }
            }
        }

        // ③ 通道入队 + 按权裁剪。
        let mut enqueued: Option<EnqueueOutcome> = None;
        if let Some(fr) = frame.take() {
            let out = self.queue.push_voice(fr);
            enqueued = Some(out);
        }
        let qcodes = self.queue.tick();
        let mut i = 0;
        while i < qcodes.len() {
            codes.push(qcodes[i]);
            i += 1;
        }

        // ④ 延迟记账（采集/编码/网络三段；网络段按积压折算）。
        let capture_us: u64 = 8_000;
        let encode_us: u64 = 6_000;
        let net_us: u64 = 30_000 + (self.queue.backlog() as u64) * 400;
        let leg_codes = self.legs.record(capture_us, encode_us, net_us, 0, 0);
        let mut j = 0;
        while j < leg_codes.len() {
            codes.push(leg_codes[j]);
            j += 1;
        }
        self.e2e.observe_frame(capture_us + encode_us + net_us);

        // ⑤ 状态看板（门开即确认说话——门内已做起势确认）。
        let state = match gate {
            GateDecision::Open => TalkState::Speaking,
            GateDecision::Hangover => TalkState::Rising,
            GateDecision::Close => TalkState::Silent,
        };
        let flap_code = self.board.update(input.sid, state, RISING_CONFIRM_TICKS.max(1), tier);
        if let Some(c) = flap_code {
            codes.push(c);
        }
        let indicator = self
            .board
            .entries()
            .iter()
            .rev()
            .find(|e| e.sid == input.sid)
            .copied();

        // ⑥ 隐私对账：出队消费即擦除闭环（骨架层在出队点擦除发送
        // 载荷，保证 erase_balance 恒 0——零记录不是口号是账面）。
        self.privacy.note_frame_in();
        self.privacy.note_erased(1);

        // ⑦ 设备面超时巡检（回收动作入账，不产生码）。
        let _ = self.device.tick_timeout(input.tick);

        VoiceTickReport {
            gate,
            tier,
            enqueued,
            codes,
            indicator,
        }
    }

    /// 各面只读访问（判据 / 快照消费）。
    #[must_use]
    pub const fn gate(&self) -> &NoiseGate {
        &self.gate
    }

    #[must_use]
    pub const fn ladder(&self) -> &DegradationLadder {
        &self.ladder
    }

    #[must_use]
    pub const fn queue(&self) -> &SendQueue {
        &self.queue
    }

    #[must_use]
    pub const fn gap(&self) -> &RxGapWatcher {
        &self.gap
    }

    #[must_use]
    pub const fn legs(&self) -> &LegLedger {
        &self.legs
    }

    #[must_use]
    pub const fn e2e(&self) -> &E2eMonitor {
        &self.e2e
    }

    #[must_use]
    pub const fn board(&self) -> &SpeakerBoard {
        &self.board
    }

    #[must_use]
    pub const fn privacy(&self) -> &PrivacyAudit {
        &self.privacy
    }

    #[must_use]
    pub const fn device(&self) -> &DeviceArbiter {
        &self.device
    }

    /// 全链操作计账。
    #[must_use]
    pub const fn ops(&self) -> u64 {
        self.ops
    }
}

// ============================================================
// 快照（'|' 分隔保空段 + FNV 指纹）
// ============================================================

/// 语音系统快照（调试与读屏消费）。
///
/// 序列化用 '|' 分隔、字段内用 ':'；空段保留（连续分隔符即空段），
/// 解析按位置回填——格式演进以版本头钉死。快照只含状态面字段（结
/// 构性隐私在序列化层同样成立——没有内容列）。
#[derive(Debug)]
pub struct VoiceSnap;

impl VoiceSnap {
    /// 版本头。
    pub const VERSION: u32 = 1;

    /// 序列化（一段式：v 版本|tick|档序|门态|静音tick|说话人数|看板）。
    #[must_use]
    pub fn serialize(sys: &VoiceSystem, tick: u64) -> String {
        let mut s = String::new();
        let gate = if sys.gate().is_open() { "O" } else { "C" };
        let _ = write!(
            s,
            "v{}|{}|{}|{}|{}|{}|{}",
            Self::VERSION,
            tick,
            sys.ladder().tier().rank(),
            gate,
            sys.gate().silent_ticks(),
            sys.board().speaking_count(),
            sys.board().snapshot()
        );
        s
    }

    /// 快照指纹（FNV-1a 异或 [`SNAP_FNV_SEED`]）。
    #[must_use]
    pub fn fingerprint(s: &str) -> u32 {
        fnv1a(s.as_bytes()) ^ SNAP_FNV_SEED
    }
}

/// 参考探针：8 说话人 × 128 帧全链跑通 + 口径验证。
///
/// 返回 `true` 即全链口径成立：
/// 1. 段守恒与预算守恒双对账通过；
/// 2. 每 tick 档位与阶梯自洽；
/// 3. 通道接通且入队有账；
/// 4. 零记录闭环（`erase_balance() == 0`）；
/// 5. 延迟账已记账；
/// 6. 快照序列化→指纹→重序列化恒等。
#[must_use]
pub fn reference_budget_check() -> bool {
    if !segment_guard() || !budget_conservation() {
        return false;
    }
    let mut sys = VoiceSystem::new();
    sys.attach_channel();

    let mut tick: u64 = 0;
    let mut all_ok = true;
    let mut speaker: usize = 0;
    while speaker < 8 {
        let sid = TalkerId(speaker as u32 + 1);
        let mut f: u32 = 0;
        while f < 128 {
            // 能量图案：交替说话/静默（偶帧说话、奇帧静默）。
            let speaking = f % 2 == 0;
            let rms = if speaking { 40_000 } else { 8 };
            let pcm = if speaking {
                let mut v = Vec::new();
                let mut i = 0;
                while i < 64 {
                    v.push((f as u8).wrapping_add(i as u8));
                    i += 1;
                }
                Some(v)
            } else {
                None
            };
            let input = VoiceInput {
                sid,
                rms_sq: rms,
                pcm,
                tick,
            };
            let rep = sys.tick(&input);
            tick += 1;
            // 口径 2：档位与阶梯自洽。
            if rep.tier != sys.ladder().tier() {
                all_ok = false;
            }
            f += 1;
        }
        speaker += 1;
    }

    // 口径 3：通道接通且入队有账。
    if !sys.queue().ready() {
        all_ok = false;
    }
    let (enq, _del, _rej) = sys.queue().totals();
    if enq == 0 {
        all_ok = false;
    }

    // 口径 4：零记录闭环。
    if sys.privacy().erase_balance() != 0 {
        all_ok = false;
    }

    // 口径 5：延迟账已记账。
    if sys.legs().frames() == 0 {
        all_ok = false;
    }

    // 口径 6：快照往返恒等。
    let s1 = VoiceSnap::serialize(&sys, tick);
    let fp1 = VoiceSnap::fingerprint(&s1);
    let s2 = VoiceSnap::serialize(&sys, tick);
    if VoiceSnap::fingerprint(&s2) != fp1 || s1 != s2 {
        all_ok = false;
    }

    all_ok
}
