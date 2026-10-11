//! veac03_perception — VE-F5803 感知系统（AI 看到的不是世界真值而是感知模型）。
//!
//! 判据原文（施工书锚点 VE-F5803）：感知系统——视觉、听觉、事件三类感知
//! 通道与感知过滤（AI 看到的不是世界真值而是感知模型——给 AI 开挂式的全
//! 知透视不存在，公平红线）；感知含记忆与遗忘——AI 感知到的信息有记忆时
//! 长（看到过不等于记得），遗忘曲线可配；含遗忘曲线实验数据。数据结构：
//! 感知通道；过滤器。错误路径与降级矩阵：通道失效→降级感知；越权全知→
//! 拒绝；感知漂移→标定。判据：三类通道、感知模型非真值、拒绝全知、判
//! 据。
//!
//! 职责定位：AI Agent 三段抽象（F5802 感知输入/决策核心/行动输出）的感知
//! 输入段——多通道接入（视觉/听觉/事件三类闭集）、感知过滤（O(事件)——
//! 每条感知必经模型标注，真值直通不存在）、感知记忆账（看到过 ≠ 记得——
//! 保留率随时间衰减，遗忘曲线可配三档）与遗忘执行（低于阈值标记
//! Forgotten，查询侧显性不可用）。与 F5801（AI 核心）/F5802（Agent 模
//! 型）分工：彼是决策与注册面，此是输入面；跨批对接点 AB 域音频事件（听
//! 觉通道事件源）、E 域视觉（视觉通道事件源）——本模块只定义通道抽象，
//! 事件源注入走 perceive() 统一入口。
//!
//! ## 深化面（锚点 300 行 → 深化版）
//!
//! 1. **三类通道闭集**：Vision/Auditory/Event 三态 from_code 双向（遥测
//!    防伪根基）；通道生命周期 Active→Degraded（过滤阈值收紧）→Failed
//!    （停用，感知请求降级到其余通道——锚点"通道失效→降级感知"）；通道
//!    漂移记账（偏差累计超阈 → 标定请求留痕——"感知漂移→标定"）。
//! 2. **感知模型非真值（公平红线的结构级保证）**：PerceptRecord 无真值
//!    字段（raw truth 不可达——结构级不存在）；perceive() 强制带感知模型
//!    标注 model_id 与置信度 ppm——无标注的「真值直通」请求显性拒绝
//!    （0x6804）；全知查询 API omniscient_query() 恒拒+留痕（0x6803——
//!    "给 AI 开挂式的全知透视不存在"的机器可查面）。
//! 3. **记忆与遗忘（双写对拍第 7 单）**：FORGET_CURVE_MATRIX 三档记忆强
//!    度 × 五时间点保留率实验数据表（Ebbinghaus 式全整数近似
//!    R(t)=1000·S/(S+t)，t=S 时 R=500 一半遗忘点）↔ retention_rule
//!    规则推导函数 15 格逐格对拍；感知条目随 tick 推进查保留率，低于
//!    FORGET_THRESHOLD(200‰) 标记 Forgotten——"看到过不等于记得"；
//!    遗忘条目查询侧显性不可用（0x6805——不可作为决策输入的机器可查面）。
//! 4. **过滤 O(事件)**：perceive 批量入口逐条过滤（通道状态闸+模型标注
//!    闸+漂移记账），单遍 O(n) 无嵌套扫描。
//! 5. **审计链+遥测全量**：七变体 AuditOp 闭集（Perceived/ChannelDegraded
//!    /ChannelFailed/OmniscientRejected/NoModelRejected/Forgotten/Calibrat
//!    ionRequested），满容 256 拒记不覆盖 seq 严格递增；感知遥测七元闭集
//!    +well_formed 防伪；确定性双跑逐位一致判据固化。
//! 6. 诊断码 0x68 段十码 0x6801~0x680A（全仓扫描：0x62~0x66 已有主，
//!    0x68 仅 FNV 种子常量 0x6868_0001 非段位码——空闲）；no_std 零 std
//!    依赖；全整数零浮点。

use alloc::string::String;
use alloc::vec::Vec;

/// 模块版本。
pub const VEAC03_PERCEPTION_VERSION: &str = "AC03-perception-v1";

// ---------------------------------------------------------------------------
// 一、诊断码（XcCode 0x68 段十码，本单独占）
// ---------------------------------------------------------------------------

pub const XC_PERC_CHANNEL_UNKNOWN: u32 = 0x6801; // 未知通道（闭集外）
pub const XC_PERC_CHANNEL_DEAD: u32 = 0x6802; // 失效通道感知拒（应降级到其余通道）
pub const XC_PERC_OMNISCIENT_DENY: u32 = 0x6803; // 全知请求拒绝（公平红线）
pub const XC_PERC_NO_MODEL: u32 = 0x6804; // 无感知模型标注（真值直通拒）
pub const XC_PERC_FORGOTTEN: u32 = 0x6805; // 已遗忘条目（显性不可用）
pub const XC_PERC_WINDOW_EMPTY: u32 = 0x6806; // 空漂移窗（标定拒）
pub const XC_PERC_TICK_REWIND: u32 = 0x6807; // tick 倒流（时序违约）
pub const XC_PERC_DRIFT_HIGH: u32 = 0x6808; // 漂移超阈（需标定）
pub const XC_PERC_STRENGTH_UNKNOWN: u32 = 0x6809; // 记忆强度档闭集外
pub const XC_PERC_TELEMETRY_LEAK: u32 = 0x680A; // 遥测防伪失败

// ---------------------------------------------------------------------------
// 二、常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 遗忘阈值：保留率低于 200‰（20%）标记 Forgotten。
pub const FORGET_THRESHOLD: u32 = 200;
/// 漂移标定阈值：通道偏差累计超过 30_000 ppm（3%）发标定请求。
pub const DRIFT_CALIBRATION_THRESHOLD: u32 = 30_000;
/// 降级收紧因子：Degraded 通道置信度按 80% 折减（×100 定点）。
pub const DEGRADE_CONFIDENCE_FACTOR: u32 = 80_00;
/// 感知记忆账容量（满容拒记不覆盖——与全域同纪律）。
pub const MEMORY_CAP: usize = 256;
/// 审计账容量。
pub const AUDIT_CAP: usize = 256;

/// 遗忘曲线实验数据矩阵（判据"含遗忘曲线实验数据"的表侧正本）。
/// 语义：（记忆强度档, 时间 tick）→ 保留率‰。三档强度
/// S=10/30/90（S 即一半遗忘点 tick 数），五时间点 0/10/30/90/180。
/// 全整数近似公式 R(t) = 1000·S/(S+t)——t=S 时恰 500‰（一半遗忘点）。
pub const FORGET_CURVE_MATRIX: [(u8, u32, u32); 15] = [
    // S=10（短时记忆档）
    (0, 0, 1000),
    (0, 10, 500),
    (0, 30, 250),
    (0, 90, 100),
    (0, 180, 52),
    // S=30（中时记忆档）
    (1, 0, 1000),
    (1, 10, 750),
    (1, 30, 500),
    (1, 90, 250),
    (1, 180, 142),
    // S=90（长时记忆档）
    (2, 0, 1000),
    (2, 10, 900),
    (2, 30, 750),
    (2, 90, 500),
    (2, 180, 333),
];

/// 规则推导·保留率（双写路径 B：纯公式，与矩阵表独立实现）。
/// R(t) = 1000·S/(S+t)；闭集外强度档显性 None 语义由调用方拒。
pub fn retention_rule(strength_idx: u8, age_ticks: u32) -> Option<u32> {
    let s: u32 = match strength_idx {
        0 => 10,
        1 => 30,
        2 => 90,
        _ => return None,
    };
    Some((1000u64 * s as u64 / (s as u64 + age_ticks as u64)) as u32)
}

/// 强度档 S 常数表（档位语义唯一源）。
pub const STRENGTH_S: [u32; 3] = [10, 30, 90];

// ---------------------------------------------------------------------------
// 三、通道闭集
// ---------------------------------------------------------------------------

/// 感知通道三类闭集（判据"三类通道"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChannelKind {
    /// 视觉（E 域视觉事件源——对接点声明）。
    Vision,
    /// 听觉（AB 域音频事件源——对接点声明）。
    Auditory,
    /// 事件（系统/游戏事件流）。
    Event,
}

impl ChannelKind {
    pub const fn code(self) -> u8 {
        match self {
            ChannelKind::Vision => 0,
            ChannelKind::Auditory => 1,
            ChannelKind::Event => 2,
        }
    }

    pub const fn from_code(c: u8) -> Option<ChannelKind> {
        match c {
            0 => Some(ChannelKind::Vision),
            1 => Some(ChannelKind::Auditory),
            2 => Some(ChannelKind::Event),
            _ => None,
        }
    }

    pub const fn zh(self) -> &'static str {
        match self {
            ChannelKind::Vision => "视觉",
            ChannelKind::Auditory => "听觉",
            ChannelKind::Event => "事件",
        }
    }

    /// 三态闭集全查。
    pub const ALL: [ChannelKind; 3] = [ChannelKind::Vision, ChannelKind::Auditory, ChannelKind::Event];
}

/// 通道状态三态（降级矩阵：Active→Degraded→Failed）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChannelState {
    /// 正常：全量过滤。
    Active,
    /// 降级：置信度按 80% 折减（锚点"通道失效→降级感知"中间态）。
    Degraded,
    /// 失效：停用（感知请求显性拒——调用方降级到其余通道）。
    Failed,
}

impl ChannelState {
    pub const fn code(self) -> u8 {
        match self {
            ChannelState::Active => 0,
            ChannelState::Degraded => 1,
            ChannelState::Failed => 2,
        }
    }
}

/// 感知通道（注册制——kind 闭集 + 状态机 + 漂移记账）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Channel {
    pub kind: ChannelKind,
    pub state: ChannelState,
    /// 漂移累计（ppm——偏差记账，超阈发标定）。
    pub drift_ppm: u32,
    /// 过滤计数（O(事件) 遥测面）。
    pub filtered_count: u64,
}

// ---------------------------------------------------------------------------
// 四、感知记录与遗忘
// ---------------------------------------------------------------------------

/// 感知条目状态（记忆三态：新鲜/淡化/已遗忘）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PerceptStatus {
    /// 保留率 ≥ 500‰。
    Fresh,
    /// 200‰ ≤ 保留率 < 500‰。
    Faded,
    /// 保留率 < 200‰——已遗忘（查询侧显性不可用）。
    Forgotten,
}

impl PerceptStatus {
    /// 由保留率判定（纯函数——遗忘执行的唯一判据面）。
    pub const fn of_retention(r: u32) -> PerceptStatus {
        if r >= 500 {
            PerceptStatus::Fresh
        } else if r >= FORGET_THRESHOLD {
            PerceptStatus::Faded
        } else {
            PerceptStatus::Forgotten
        }
    }

    pub const fn code(self) -> u8 {
        match self {
            PerceptStatus::Fresh => 0,
            PerceptStatus::Faded => 1,
            PerceptStatus::Forgotten => 2,
        }
    }
}

/// 感知条目（**结构级公平红线：无真值字段**——只有模型标注与置信度，
/// "AI 看到的是感知模型"在类型上不可绕过）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PerceptRecord {
    /// 记忆账序号（从 1 起）。
    pub seq: u64,
    /// 感知发生 tick。
    pub tick: u64,
    /// 通道类别。
    pub channel: ChannelKind,
    /// 内容编码（u32 事件码——通道域内语义，账面轻量）。
    pub content_code: u32,
    /// 感知模型标注（非 0——0 即真值直通，perceive 闸拒绝）。
    pub model_id: u32,
    /// 置信度（ppm——Degraded 通道按 80% 折减）。
    pub confidence_ppm: u32,
    /// 记忆强度档（0/1/2——遗忘曲线查表键）。
    pub strength_idx: u8,
    /// 遗忘标志（sweep 跨越阈值置位——一生只计一次）。
    pub forgotten: bool,
}

impl PerceptRecord {
    /// 当前保留率（‰）：按感知年龄查遗忘曲线。
    pub fn retention(&self, now_tick: u64) -> Option<u32> {
        if now_tick < self.tick {
            return None; // tick 倒流——无年龄可言
        }
        let age = (now_tick - self.tick) as u32;
        retention_rule(self.strength_idx, age)
    }

    /// 当前状态（Fresh/Faded/Forgotten）。
    pub fn status(&self, now_tick: u64) -> Option<PerceptStatus> {
        self.retention(now_tick).map(PerceptStatus::of_retention)
    }
}

// ---------------------------------------------------------------------------
// 五、审计与遥测
// ---------------------------------------------------------------------------

/// 审计操作闭集（七变体——感知动作全留痕）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AuditOp {
    /// 感知入账。
    Perceived,
    /// 通道降级（Active→Degraded）。
    ChannelDegraded,
    /// 通道失效（→Failed）。
    ChannelFailed,
    /// 全知请求拒绝。
    OmniscientRejected,
    /// 无模型标注拒绝（真值直通拒）。
    NoModelRejected,
    /// 条目遗忘。
    Forgotten,
    /// 标定请求。
    CalibrationRequested,
}

impl AuditOp {
    pub const fn code(self) -> u8 {
        match self {
            AuditOp::Perceived => 0,
            AuditOp::ChannelDegraded => 1,
            AuditOp::ChannelFailed => 2,
            AuditOp::OmniscientRejected => 3,
            AuditOp::NoModelRejected => 4,
            AuditOp::Forgotten => 5,
            AuditOp::CalibrationRequested => 6,
        }
    }

    pub const fn zh(self) -> &'static str {
        match self {
            AuditOp::Perceived => "感知入账",
            AuditOp::ChannelDegraded => "通道降级",
            AuditOp::ChannelFailed => "通道失效",
            AuditOp::OmniscientRejected => "全知拒",
            AuditOp::NoModelRejected => "无模型拒",
            AuditOp::Forgotten => "遗忘",
            AuditOp::CalibrationRequested => "标定请求",
        }
    }
}

/// 审计条目（全整数 Copy；arg=通道码/诊断码/条目序号）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AuditRecord {
    pub seq: u64,
    pub op: AuditOp,
    pub arg: u32,
}

/// 感知遥测记录（七元闭集——判据"遥测全量"对位锚点纪律）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PerceptionTelemetry {
    /// tick。
    pub tick: u64,
    /// 通道码（0~2 闭集）。
    pub channel_code: u8,
    /// 通道状态码（0~2）。
    pub state_code: u8,
    /// 感知内容码。
    pub content_code: u32,
    /// 模型标注。
    pub model_id: u32,
    /// 置信度 ppm（折减后）。
    pub confidence_ppm: u32,
    /// 记忆强度档（0~2）。
    pub strength_idx: u8,
}

impl PerceptionTelemetry {
    /// 防伪闸：通道/状态/强度三码均在闭集内。
    pub fn well_formed(&self) -> bool {
        ChannelKind::from_code(self.channel_code).is_some()
            && self.state_code <= 2
            && self.strength_idx <= 2
    }
}

// ---------------------------------------------------------------------------
// 六、感知中枢
// ---------------------------------------------------------------------------

/// 感知中枢（三类通道 + 感知过滤 + 记忆遗忘 + 归因审计）。
pub struct PerceptionHub {
    channels: Vec<Channel>,
    memory: Vec<PerceptRecord>,
    /// 记忆账满容拒记计数。
    pub memory_dropped: u32,
    /// 遗忘执行计数。
    pub forgotten_count: u32,
    /// 全知拒绝计数。
    pub omniscient_rejects: u32,
    /// 无模型拒绝计数。
    pub no_model_rejects: u32,
    /// 标定请求计数。
    pub calibration_count: u32,
    /// 遗忘扫描推进到的 tick（单调——避免重复扫描）。
    sweep_tick: u64,
    audit: Vec<AuditRecord>,
    pub audit_dropped: u32,
}

impl PerceptionHub {
    pub fn new() -> Self {
        PerceptionHub {
            channels: Vec::new(),
            memory: Vec::new(),
            memory_dropped: 0,
            forgotten_count: 0,
            omniscient_rejects: 0,
            no_model_rejects: 0,
            calibration_count: 0,
            sweep_tick: 0,
            audit: Vec::new(),
            audit_dropped: 0,
        }
    }

    /// 通道注册（kind 闭集防伪、重复注册拒绝——注册制纪律）。
    pub fn register_channel(&mut self, kind: ChannelKind) -> Result<(), u32> {
        let mut i = 0;
        while i < self.channels.len() {
            if self.channels[i].kind == kind {
                return Err(XC_PERC_CHANNEL_UNKNOWN);
            }
            i += 1;
        }
        self.channels.push(Channel {
            kind,
            state: ChannelState::Active,
            drift_ppm: 0,
            filtered_count: 0,
        });
        Ok(())
    }

    /// 通道查找。
    pub fn find_channel(&self, kind: ChannelKind) -> Option<&Channel> {
        let mut i = 0;
        while i < self.channels.len() {
            if self.channels[i].kind == kind {
                return Some(&self.channels[i]);
            }
            i += 1;
        }
        None
    }

    fn channel_idx(&self, kind: ChannelKind) -> Option<usize> {
        let mut i = 0;
        while i < self.channels.len() {
            if self.channels[i].kind == kind {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 通道状态推进（降级矩阵：Active→Degraded→Failed，非法跳转拒——
    /// Failed 不可复活（复活走重新注册语义外的显性面——本模块不提供，
    /// 诚实降级不装活））。
    pub fn set_channel_state(
        &mut self,
        kind: ChannelKind,
        target: ChannelState,
    ) -> Result<(), u32> {
        let idx = match self.channel_idx(kind) {
            Some(i) => i,
            None => return Err(XC_PERC_CHANNEL_UNKNOWN),
        };
        let cur = self.channels[idx].state;
        // 合法迁移：Active→Degraded、Degraded→Failed、Active→Failed（急停）。
        let legal = match (cur, target) {
            (ChannelState::Active, ChannelState::Degraded) => true,
            (ChannelState::Degraded, ChannelState::Failed) => true,
            (ChannelState::Active, ChannelState::Failed) => true,
            _ => false,
        };
        if !legal {
            return Err(XC_PERC_CHANNEL_UNKNOWN);
        }
        self.channels[idx].state = target;
        let op = match target {
            ChannelState::Degraded => AuditOp::ChannelDegraded,
            ChannelState::Failed => AuditOp::ChannelFailed,
            ChannelState::Active => AuditOp::Perceived, // 不可达（legal 拦）
        };
        self.audit_push(op, kind.code() as u32);
        Ok(())
    }

    /// 漂移记账（偏差 ppm 累计——超阈发标定请求并留痕）。
    pub fn report_drift(&mut self, kind: ChannelKind, deviation_ppm: u32) -> Result<(), u32> {
        let idx = match self.channel_idx(kind) {
            Some(i) => i,
            None => return Err(XC_PERC_CHANNEL_UNKNOWN),
        };
        self.channels[idx].drift_ppm += deviation_ppm;
        if self.channels[idx].drift_ppm >= DRIFT_CALIBRATION_THRESHOLD {
            self.calibration_count += 1;
            self.audit_push(AuditOp::CalibrationRequested, kind.code() as u32);
            return Err(XC_PERC_DRIFT_HIGH);
        }
        Ok(())
    }

    /// 全知查询（公平红线 API 面）：恒拒 + 留痕 + 计数。
    /// "给 AI 开挂式的全知透视不存在"——本函数存在的意义就是把「想要全
    /// 知」变成一条被拒绝且被记录的显性事件。
    pub fn omniscient_query(&mut self) -> Result<(), u32> {
        self.omniscient_rejects += 1;
        self.audit_push(AuditOp::OmniscientRejected, XC_PERC_OMNISCIENT_DENY);
        Err(XC_PERC_OMNISCIENT_DENY)
    }

    /// 感知入口（过滤 O(1)/条——三闸：通道状态闸/模型标注闸/漂移记账）。
    /// 红线：model_id == 0 即「真值直通」请求——显性拒绝（0x6804）。
    /// 失效通道感知拒（0x6802——调用方降级到其余通道）。
    /// Degraded 通道置信度按 80% 折减（降级感知语义）。
    pub fn perceive(
        &mut self,
        tick: u64,
        channel: ChannelKind,
        content_code: u32,
        model_id: u32,
        confidence_ppm: u32,
        strength_idx: u8,
    ) -> Result<u64, u32> {
        if tick < self.sweep_tick {
            return Err(XC_PERC_TICK_REWIND);
        }
        // 模型标注闸（真值直通不存在——公平红线第一道）。
        if model_id == 0 {
            self.no_model_rejects += 1;
            self.audit_push(AuditOp::NoModelRejected, XC_PERC_NO_MODEL);
            return Err(XC_PERC_NO_MODEL);
        }
        // 记忆强度档闭集防伪。
        if strength_idx > 2 {
            return Err(XC_PERC_STRENGTH_UNKNOWN);
        }
        // 通道状态闸。
        let idx = match self.channel_idx(channel) {
            Some(i) => i,
            None => return Err(XC_PERC_CHANNEL_UNKNOWN),
        };
        if self.channels[idx].state == ChannelState::Failed {
            return Err(XC_PERC_CHANNEL_DEAD);
        }
        let degraded = self.channels[idx].state == ChannelState::Degraded;
        let conf = if degraded {
            (confidence_ppm as u64 * DEGRADE_CONFIDENCE_FACTOR as u64 / 100_00) as u32
        } else {
            confidence_ppm
        };
        self.channels[idx].filtered_count += 1;
        // 记忆账（满容拒记不覆盖）。
        let seq = (self.memory.len() as u64) + 1 + (self.memory_dropped as u64);
        if self.memory.len() >= MEMORY_CAP {
            self.memory_dropped += 1;
        } else {
            self.memory.push(PerceptRecord {
                seq,
                tick,
                channel,
                content_code,
                model_id,
                confidence_ppm: conf,
                strength_idx,
                forgotten: false,
            });
        }
        self.audit_push(AuditOp::Perceived, seq as u32);
        // sweep_tick 只前进不回退（时序单调纪律）。
        if tick > self.sweep_tick {
            self.sweep_tick = tick;
        }
        Ok(seq)
    }

    /// 遗忘扫描（判据"感知含记忆与遗忘"执行面）：给定 now_tick 逐条查
    /// 保留率，首次跨越阈值标记遗忘（forgotten 置位 + 计数 + 审计——
    /// 条目一生只计一次；保留率单调减，置位后不再重复）。遗忘是标记不
    /// 是抹除——条目不出账，审计面可查"曾感知"。
    pub fn sweep_forget(&mut self, now_tick: u64) -> u32 {
        let mut n = 0;
        let mut i = 0;
        while i < self.memory.len() {
            if !self.memory[i].forgotten {
                let r = self.memory[i].retention(now_tick);
                if let Some(r) = r {
                    if PerceptStatus::of_retention(r) == PerceptStatus::Forgotten {
                        self.memory[i].forgotten = true;
                        n += 1;
                    }
                }
            }
            i += 1;
        }
        self.forgotten_count += n;
        let mut k = 0;
        while k < n {
            self.audit_push(AuditOp::Forgotten, 0);
            k += 1;
        }
        if now_tick > self.sweep_tick {
            self.sweep_tick = now_tick;
        }
        n
    }

    /// 查询感知条目（判据面：遗忘条目显性不可用——0x6805）。
    /// 返回（条目引用数据快照, 当前状态码）——Forgotten 一律拒。
    pub fn query_percept(&self, seq: u64) -> Result<(PerceptRecord, u8), u32> {
        self.query_percept_at(seq, self.sweep_tick)
    }

    /// 带显式观察 tick 的查询（纯函数变体——判据/调试面，不依赖
    /// sweep_tick 推进；遗忘标志置位后恒拒）。
    pub fn query_percept_at(
        &self,
        seq: u64,
        now_tick: u64,
    ) -> Result<(PerceptRecord, u8), u32> {
        let mut i = 0;
        while i < self.memory.len() {
            if self.memory[i].seq == seq {
                if self.memory[i].forgotten {
                    return Err(XC_PERC_FORGOTTEN);
                }
                let st = match self.memory[i].status(now_tick) {
                    Some(st) => st,
                    None => return Err(XC_PERC_TICK_REWIND),
                };
                if st == PerceptStatus::Forgotten {
                    return Err(XC_PERC_FORGOTTEN);
                }
                return Ok((self.memory[i], st.code()));
            }
            i += 1;
        }
        Err(XC_PERC_FORGOTTEN) // 账外序号——同样显性不可用
    }

    /// 记忆账条数。
    pub fn memory_len(&self) -> usize {
        self.memory.len()
    }

    /// 记忆条目读取（越界 None——判据直读面）。
    pub fn memory_at(&self, i: usize) -> Option<PerceptRecord> {
        if i < self.memory.len() {
            Some(self.memory[i])
        } else {
            None
        }
    }

    /// 通道数。
    pub fn channel_count(&self) -> usize {
        self.channels.len()
    }

    /// 遥测快照（最近一条感知——well_formed 防伪在构造面保证）。
    pub fn telemetry_last(&self) -> Option<PerceptionTelemetry> {
        let n = self.memory.len();
        if n == 0 {
            return None;
        }
        let p = self.memory[n - 1];
        let ch = self.find_channel(p.channel);
        let state_code = match ch {
            Some(c) => c.state.code(),
            None => return None,
        };
        let t = PerceptionTelemetry {
            tick: p.tick,
            channel_code: p.channel.code(),
            state_code,
            content_code: p.content_code,
            model_id: p.model_id,
            confidence_ppm: p.confidence_ppm,
            strength_idx: p.strength_idx,
        };
        if !t.well_formed() {
            return None;
        }
        Some(t)
    }

    /// 审计条数。
    pub fn audit_len(&self) -> usize {
        self.audit.len()
    }

    /// 审计条目读取。
    pub fn audit_at(&self, i: usize) -> Option<AuditRecord> {
        if i < self.audit.len() {
            Some(self.audit[i])
        } else {
            None
        }
    }

    /// 审计推进（满容拒记不覆盖）。
    fn audit_push(&mut self, op: AuditOp, arg: u32) {
        if self.audit.len() >= AUDIT_CAP {
            self.audit_dropped += 1;
            return;
        }
        let seq = (self.audit.len() as u64) + 1 + (self.audit_dropped as u64);
        self.audit.push(AuditRecord { seq, op, arg });
    }
}

// ---------------------------------------------------------------------------
// 七、工具函数
// ---------------------------------------------------------------------------

/// u32 转十进制串。
pub fn u32_to_str(v: u32) -> String {
    if v == 0 {
        return String::from("0");
    }
    let mut buf = [0u8; 10];
    let mut i = 10;
    let mut x = v;
    while x > 0 {
        i -= 1;
        buf[i] = b'0' + (x % 10) as u8;
        x /= 10;
    }
    let mut s = String::with_capacity(10);
    let mut j = i;
    while j < 10 {
        s.push(buf[j] as char);
        j += 1;
    }
    s
}

// ---------------------------------------------------------------------------
// 八、自检（九组——判据侧另有 18 条细判据）
// ---------------------------------------------------------------------------

impl PerceptionHub {
    /// 自检一：双写对拍——FORGET_CURVE_MATRIX 15 格 ↔ retention_rule 全等。
    pub fn selfcheck_curve_doublewrite(&mut self) -> Result<(), u32> {
        let mut i = 0;
        while i < FORGET_CURVE_MATRIX.len() {
            let (s, t, r) = FORGET_CURVE_MATRIX[i];
            match retention_rule(s, t) {
                Some(got) => {
                    if got != r {
                        return Err(XC_PERC_STRENGTH_UNKNOWN);
                    }
                }
                None => return Err(XC_PERC_STRENGTH_UNKNOWN),
            }
            i += 1;
        }
        // 一半遗忘点：t=S 时恒 500。
        let mut k = 0;
        while k < STRENGTH_S.len() {
            if retention_rule(k as u8, STRENGTH_S[k]) != Some(500) {
                return Err(XC_PERC_STRENGTH_UNKNOWN);
            }
            k += 1;
        }
        Ok(())
    }

    /// 自检二：三类通道注册 + 重复拒 + 闭集防伪。
    pub fn selfcheck_channels(&mut self) -> Result<(), u32> {
        let mut h = PerceptionHub::new();
        h.register_channel(ChannelKind::Vision)?;
        h.register_channel(ChannelKind::Auditory)?;
        h.register_channel(ChannelKind::Event)?;
        if h.channel_count() != 3 {
            return Err(XC_PERC_CHANNEL_UNKNOWN);
        }
        if h.register_channel(ChannelKind::Vision) != Err(XC_PERC_CHANNEL_UNKNOWN) {
            return Err(XC_PERC_CHANNEL_UNKNOWN);
        }
        // from_code 闭集外 None。
        if ChannelKind::from_code(3).is_some() || ChannelKind::from_code(255).is_some() {
            return Err(XC_PERC_CHANNEL_UNKNOWN);
        }
        Ok(())
    }

    /// 自检三：感知模型非真值——无模型标注（model_id=0）拒绝 + 全知恒拒。
    pub fn selfcheck_fairness(&mut self) -> Result<(), u32> {
        let mut h = PerceptionHub::new();
        h.register_channel(ChannelKind::Vision)?;
        // 真值直通拒。
        if h.perceive(1, ChannelKind::Vision, 42, 0, 900_000, 1) != Err(XC_PERC_NO_MODEL) {
            return Err(XC_PERC_NO_MODEL);
        }
        if h.no_model_rejects != 1 {
            return Err(XC_PERC_NO_MODEL);
        }
        // 全知恒拒（连拒三次全红）。
        let mut k = 0;
        while k < 3 {
            if h.omniscient_query() != Err(XC_PERC_OMNISCIENT_DENY) {
                return Err(XC_PERC_OMNISCIENT_DENY);
            }
            k += 1;
        }
        if h.omniscient_rejects != 3 {
            return Err(XC_PERC_OMNISCIENT_DENY);
        }
        Ok(())
    }

    /// 自检四：通道降级矩阵——Degraded 置信度折减 80% + Failed 感知拒 +
    /// 非法迁移拒（Failed 不可复活）。
    pub fn selfcheck_degrade(&mut self) -> Result<(), u32> {
        let mut h = PerceptionHub::new();
        h.register_channel(ChannelKind::Auditory)?;
        h.set_channel_state(ChannelKind::Auditory, ChannelState::Degraded)?;
        let seq = h.perceive(1, ChannelKind::Auditory, 7, 101, 500_000, 1)?;
        let (rec, _) = h.query_percept(seq)?;
        // 500_000 × 80% = 400_000。
        if rec.confidence_ppm != 400_000 {
            return Err(XC_PERC_CHANNEL_DEAD);
        }
        h.set_channel_state(ChannelKind::Auditory, ChannelState::Failed)?;
        if h.perceive(2, ChannelKind::Auditory, 8, 101, 500_000, 1) != Err(XC_PERC_CHANNEL_DEAD) {
            return Err(XC_PERC_CHANNEL_DEAD);
        }
        // 非法迁移：Failed→Active 拒。
        if h.set_channel_state(ChannelKind::Auditory, ChannelState::Active) != Err(XC_PERC_CHANNEL_UNKNOWN) {
            return Err(XC_PERC_CHANNEL_UNKNOWN);
        }
        // 其余通道仍可感知（降级感知语义——事件通道接棒）。
        h.register_channel(ChannelKind::Event)?;
        h.perceive(3, ChannelKind::Event, 9, 102, 800_000, 0)?;
        Ok(())
    }

    /// 自检五：遗忘执行——短时记忆档（S=10）随 tick 推进淡出 +
    /// 遗忘条目查询显性不可用。
    pub fn selfcheck_forget(&mut self) -> Result<(), u32> {
        let mut h = PerceptionHub::new();
        h.register_channel(ChannelKind::Vision)?;
        // t=1 感知，S=0 档（S=10）：tick 11 保留率 500（Faded 边界 Fresh/Faded 交界
        // ——status 判定 r>=500 Fresh，t=11 → 1000*10/21=476 → Faded。
        let seq = h.perceive(1, ChannelKind::Vision, 42, 7, 900_000, 0)?;
        // t=12 → age=11 → 476‰ → Faded（t=11 时 age=10 恰 500 Fresh 边界）。
        let (_, st) = h.query_percept_at(seq, 12)?;
        if st != PerceptStatus::Faded.code() {
            return Err(XC_PERC_STRENGTH_UNKNOWN);
        }
        // tick 51：1000*10/61=163 < 200 → Forgotten。
        let n = h.sweep_forget(51);
        if n != 1 || h.forgotten_count != 1 {
            return Err(XC_PERC_WINDOW_EMPTY);
        }
        if h.query_percept(seq) != Err(XC_PERC_FORGOTTEN) {
            return Err(XC_PERC_FORGOTTEN);
        }
        Ok(())
    }

    /// 自检六：漂移标定——偏差累计超阈发标定请求留痕。
    pub fn selfcheck_drift(&mut self) -> Result<(), u32> {
        let mut h = PerceptionHub::new();
        h.register_channel(ChannelKind::Event)?;
        // 两次 15_000 ppm 累计 30_000 → 触发标定（第二次返回 Err DRIFT_HIGH）。
        h.report_drift(ChannelKind::Event, 15_000)?;
        if h.report_drift(ChannelKind::Event, 15_000) != Err(XC_PERC_DRIFT_HIGH) {
            return Err(XC_PERC_DRIFT_HIGH);
        }
        if h.calibration_count != 1 {
            return Err(XC_PERC_DRIFT_HIGH);
        }
        Ok(())
    }

    /// 自检七：tick 倒流拒 + 强度档闭集外拒。
    pub fn selfcheck_temporal(&mut self) -> Result<(), u32> {
        let mut h = PerceptionHub::new();
        h.register_channel(ChannelKind::Vision)?;
        h.perceive(10, ChannelKind::Vision, 1, 5, 900_000, 1)?;
        if h.perceive(9, ChannelKind::Vision, 2, 5, 900_000, 1) != Err(XC_PERC_TICK_REWIND) {
            return Err(XC_PERC_TICK_REWIND);
        }
        if h.perceive(11, ChannelKind::Vision, 2, 5, 900_000, 3) != Err(XC_PERC_STRENGTH_UNKNOWN) {
            return Err(XC_PERC_STRENGTH_UNKNOWN);
        }
        Ok(())
    }

    /// 自检八：记忆账满容拒记（257 条感知 → 256 入账 + dropped 1）。
    pub fn selfcheck_memory_cap(&mut self) -> Result<(), u32> {
        let mut h = PerceptionHub::new();
        h.register_channel(ChannelKind::Vision)?;
        let mut k: u64 = 1;
        while k <= (MEMORY_CAP as u64) + 1 {
            let r = h.perceive(k, ChannelKind::Vision, (k % 97) as u32, 9, 900_000, 2);
            if r.is_err() {
                return Err(XC_PERC_WINDOW_EMPTY);
            }
            k += 1;
        }
        if h.memory_len() != MEMORY_CAP || h.memory_dropped != 1 {
            return Err(XC_PERC_WINDOW_EMPTY);
        }
        Ok(())
    }

    /// 自检九：遥测防伪 + 审计链 seq 严格递增。
    pub fn selfcheck_telemetry_audit(&mut self) -> Result<(), u32> {
        // 防伪：闭集外通道码 well_formed false。
        let bad = PerceptionTelemetry {
            tick: 1,
            channel_code: 9,
            state_code: 0,
            content_code: 1,
            model_id: 5,
            confidence_ppm: 900_000,
            strength_idx: 1,
        };
        if bad.well_formed() {
            return Err(XC_PERC_TELEMETRY_LEAK);
        }
        // 审计 seq 递增。
        let mut h = PerceptionHub::new();
        h.register_channel(ChannelKind::Vision)?;
        h.perceive(1, ChannelKind::Vision, 1, 5, 900_000, 1)?;
        h.perceive(2, ChannelKind::Vision, 2, 5, 900_000, 1)?;
        let mut i = 1;
        while i < h.audit_len() {
            match (h.audit_at(i - 1), h.audit_at(i)) {
                (Some(a), Some(b)) => {
                    if b.seq <= a.seq {
                        return Err(XC_PERC_TELEMETRY_LEAK);
                    }
                }
                _ => return Err(XC_PERC_TELEMETRY_LEAK),
            }
            i += 1;
        }
        // 遥测快照 well_formed。
        match h.telemetry_last() {
            Some(t) => {
                if !t.well_formed() {
                    return Err(XC_PERC_TELEMETRY_LEAK);
                }
            }
            None => return Err(XC_PERC_TELEMETRY_LEAK),
        }
        Ok(())
    }

    /// 全量自检入口（九组）。
    pub fn selfcheck_all(&mut self) -> Result<(), u32> {
        self.selfcheck_curve_doublewrite()?;
        self.selfcheck_channels()?;
        self.selfcheck_fairness()?;
        self.selfcheck_degrade()?;
        self.selfcheck_forget()?;
        self.selfcheck_drift()?;
        self.selfcheck_temporal()?;
        self.selfcheck_memory_cap()?;
        self.selfcheck_telemetry_audit()?;
        Ok(())
    }
}
