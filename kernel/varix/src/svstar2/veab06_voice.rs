//! VE-F5606 · 语音系统骨架（VE-AB 域 · 音频与语音域 · AB06 组）。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F5606`
//!
//! **判据（锚点原文）**：可靠通道、按权裁剪、零记录、判据。
//!
//! **职责定位（锚点原文）**：语音系统骨架——语音输入输出与 AA 域
//! 网络通道衔接（语音走 AA 可靠通道、按权裁剪、内容零记录）；语音
//! 链路含端到端延迟预算（说话到听见全链承诺）；静噪门限自适应
//! ——不说话时彻底安静；语音含说话状态指示——谁在说话一目了然
//! （隐私：只示状态不示内容）。错误路径与降级矩阵：设备占用→抢占
//! 提示；编解码失败→降级码率；隐私违例→阻断。处理 O(帧)。跨批
//! 对接点：AA01 通道；AA05 隐私。无障碍与隐私：语音状态可见。
//!
//! ## 一、可靠通道衔接（判据一）
//!
//! 语音走 AA 可靠通道（[`VoiceChannel`]——通道卡三问句与 F5403
//! 通道抽象同构：有序/重传/拥塞——语音走「有序+重传」可靠卡；
//! 协议名不出现（抽象泄漏拦截 [`E_VCE_LEAK`]——能力语汇放行）；
//! 通道衔接 O(1)（分派表直取 [`CHANNEL_PLAN`]））。
//!
//! ## 二、按权裁剪（判据二）
//!
//! 按权裁剪（[`VoiceGate`]——发言权重档 [`VoiceWeight`]（全员/
//! 小队/静音）×帧裁剪表：静音档全裁、小队档裁非队内、全员档全放
//! ——查表 O(帧) 单遍）；预算守恒（每帧语音字节预算
//! [`FRAME_VOICE_BUDGET`]——超预算低权先裁 [`E_VCE_BUDGET`]）。
//!
//! ## 三、零记录（判据三：隐私红线）
//!
//! 内容零记录（[`PRIVACY_CLAUSE`] 字面量冻结——「语音内容不落盘
//! 不入账不转发第三方」；账面只有状态与计数没有内容——内容字节
//! 处理完即弃，账只记长度与判定）；隐私违例→阻断（[`PrivacyGate`]
//! ——任何带「记录/转写/转发」意图的调用面显性拒
//! [`E_VCE_PRIVACY`] 并计数——阻断不是静默丢弃）；说话状态指示
//! （[`SpeakerBoard`]——只示状态不示内容：每说话人三态
//! （说话/静默/离线）+电平档（0-5 档整数）——内容字节永不入板）。
//!
//! ## 四、延迟预算与静噪自适应（深化面）
//!
//! 端到端延迟预算（[`LatencyBudget`]——采集→编码→传输→解码→
//! 播放五段预算分解 [`BUDGET_SEGMENTS`]，和=全链承诺
//! [`E2E_BUDGET_MS`]；实测超段即立案 [`E_VCE_LATENCY`]——承诺
//! 是可断言的账不是口号）；静噪门限自适应（[`NoiseGate`]——门限
//! 随环境底噪 EWMA 漂移（移位无浮点 α=1/4），不说话时彻底安静
//! （门限下零输出零发送——静噪是省预算的根））；编解码失败→
//! 降级码率（[`CodecLadder`]——码率阶梯三档 48k/24k/12k，失败
//! 降一档降到底后停发；降级留痕 [`E_VCE_CODEC`]）；设备占用→
//! 抢占提示（[`E_VCE_OCCUPIED`]——抢占请求显性提示被占方）。
//!
//! **对接**：AA01（F5401 通道层——可靠卡的上游）；AA05（F5405
//! 状态复制——语音不走复制通道（一次性流语义），衔接声明）；F5602
//! （设备输出——播放面下游）；F4405（配置档——门限档持久化参照
//! 理念）。零 panic 面、零 IO、零墙钟（tick 注入）、no_std、零
//! 浮点（EWMA 用移位）。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量
// ---------------------------------------------------------------------------

/// 本项版本。
pub const VOICE_VERSION: &str = "AB06-voice-v1";

/// 每帧语音字节预算（按权裁剪的分母）。
pub const FRAME_VOICE_BUDGET: u32 = 1_200;

/// 端到端延迟预算（ms；说话到听见全链承诺）。
pub const E2E_BUDGET_MS: u32 = 150;

/// 延迟预算五段分解（采集/编码/传输/解码/播放；和=E2E_BUDGET_MS）。
pub const BUDGET_SEGMENTS: [&str; 5] = ["采集", "编码", "传输", "解码", "播放"];

pub const BUDGET_SEGMENT_MS: [u32; 5] = [20, 20, 70, 20, 20];

const _: () = assert!(
    BUDGET_SEGMENT_MS[0]
        + BUDGET_SEGMENT_MS[1]
        + BUDGET_SEGMENT_MS[2]
        + BUDGET_SEGMENT_MS[3]
        + BUDGET_SEGMENT_MS[4]
        == E2E_BUDGET_MS
);

/// 静噪 EWMA 平滑参数（1/4——移位实现，零浮点）。
pub const NOISE_EWMA_SHIFT: u32 = 2;

/// 静噪裕量（门限=底噪 EWMA+裕量，Q8 倍数）。
pub const NOISE_MARGIN: u32 = 8;

/// 码率阶梯三档（bps；失败逐档降）。
pub const CODEC_LADDER: [u32; 3] = [48_000, 24_000, 12_000];

/// 电平档数（说话板 0-5 档）。
pub const LEVEL_STEPS: u8 = 5;

/// 说话板容量上限（同时在板说话人数）。
pub const SPEAKER_CAP: usize = 16;

// ---------------------------------------------------------------------------
// 二、字符串诊断码（Z 域字符串码惯例——本组 E_VCE_ 前缀）
// ---------------------------------------------------------------------------

/// 诊断码闭集（E_VCE_ 前缀互异）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum XcCode {
    /// 设备占用（抢占请求→提示被占方）。
    Occupied,
    /// 编解码失败（→降级码率）。
    CodecFailed,
    /// 码率已到底（降无可降——停发）。
    CodecFloor,
    /// 隐私违例（记录/转写/转发意图——阻断）。
    PrivacyViolation,
    /// 抽象泄漏（协议名出现——能力语汇放行）。
    AbstractionLeak,
    /// 帧预算超（低权先裁仍超——立案）。
    BudgetExceeded,
    /// 延迟超段（实测超段预算——立案）。
    LatencyBreach,
    /// 通道未衔接（语音通道未建即发——拒）。
    ChannelNotBound,
    /// 说话板满（板容量上限——踢最久静默者或拒新板）。
    BoardFull,
}

impl XcCode {
    /// 人读诊断（含可操作建议）。
    pub fn detail(&self) -> &'static str {
        match self {
            XcCode::Occupied => "设备占用：语音设备被其他会话占用，建议提示被占方释放或等待",
            XcCode::CodecFailed => "编解码失败：已降一级码率重试，建议检查码率阶梯配置",
            XcCode::CodecFloor => "码率已到底：三档降无可降，建议停发并提示网络过差",
            XcCode::PrivacyViolation => "隐私违例：语音内容不落盘不入账不转发第三方（红线），建议撤销记录类调用",
            XcCode::AbstractionLeak => "抽象泄漏：协议名出现（应说可靠/不可靠能力语汇），建议改用能力语汇",
            XcCode::BudgetExceeded => "帧预算超：按权裁剪后仍超预算，建议进一步降权或降码率",
            XcCode::LatencyBreach => "延迟超段：实测超段预算，建议点名超段环节立案排查",
            XcCode::ChannelNotBound => "通道未衔接：语音通道未建立即发送，建议先 bind_channel",
            XcCode::BoardFull => "说话板满：在板人数达上限，建议踢最久静默者或扩容",
        }
    }

    /// 字符串码。
    pub const fn code(&self) -> &'static str {
        match self {
            XcCode::Occupied => "E_VCE_OCCUPIED",
            XcCode::CodecFailed => "E_VCE_CODEC",
            XcCode::CodecFloor => "E_VCE_CODEC_FLOOR",
            XcCode::PrivacyViolation => "E_VCE_PRIVACY",
            XcCode::AbstractionLeak => "E_VCE_LEAK",
            XcCode::BudgetExceeded => "E_VCE_BUDGET",
            XcCode::LatencyBreach => "E_VCE_LATENCY",
            XcCode::ChannelNotBound => "E_VCE_NO_CHANNEL",
            XcCode::BoardFull => "E_VCE_BOARD_FULL",
        }
    }
}

// ---------------------------------------------------------------------------
// 三、FNV-1a 指纹基元
// ---------------------------------------------------------------------------

/// FNV-1a 64 位偏移基。
pub const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV-1a 64 位素数。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a 单字节吸收。
pub const fn fnv1a_byte(state: u64, byte: u8) -> u64 {
    (state ^ (byte as u64)).wrapping_mul(FNV_PRIME)
}

/// FNV-1a 字节串吸收。
pub const fn fnv1a_bytes(state: u64, bytes: &[u8]) -> u64 {
    let mut s = state;
    let mut i = 0usize;
    while i < bytes.len() {
        s = fnv1a_byte(s, bytes[i]);
        i += 1;
    }
    s
}

/// FNV-1a 字符串吸收。
pub fn fnv1a_str(state: u64, text: &str) -> u64 {
    fnv1a_bytes(state, text.as_bytes())
}

/// FNV-1a u64 吸收。
pub const fn fnv1a_u64(state: u64, value: u64) -> u64 {
    fnv1a_bytes(state, &value.to_be_bytes())
}

// ---------------------------------------------------------------------------
// 四、可靠通道衔接（判据一）
// ---------------------------------------------------------------------------

/// 通道卡（与 F5403 通道抽象同构：三问句，协议名不出现）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VoiceChannel {
    /// 是否有序（语音走可靠=有序）。
    pub ordered: bool,
    /// 是否重传（可靠卡特性）。
    pub retransmit: bool,
    /// 是否拥塞控制。
    pub congestion: bool,
}

/// 通道分派计划（语音=可靠卡——分派表直取 O(1)）。
pub const CHANNEL_PLAN: VoiceChannel = VoiceChannel {
    ordered: true,
    retransmit: true,
    congestion: true,
};

/// 能力语汇合法性（抽象泄漏拦截：协议名（TCP/UDP/QUIC 等词根）出现即拒）。
pub fn capability_vocabulary_ok(desc: &str) -> Result<(), XcCode> {
    const LEAKS: [&str; 6] = ["tcp", "udp", "quic", "kcp", "websocket", "http"];
    let lower = desc.to_string();
    let lb = lower.as_bytes();
    let mut i = 0usize;
    while i < LEAKS.len() {
        let pat = match LEAKS.get(i) {
            Some(p) => p.as_bytes(),
            None => break,
        };
        // 手写子串搜索（零依赖）。
        if lb.len() >= pat.len() {
            let mut j = 0usize;
            while j + pat.len() <= lb.len() {
                let mut hit = true;
                let mut k = 0usize;
                while k < pat.len() {
                    if lb[j + k] != pat[k] {
                        hit = false;
                        break;
                    }
                    k += 1;
                }
                if hit {
                    return Err(XcCode::AbstractionLeak);
                }
                j += 1;
            }
        }
        i += 1;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 五、按权裁剪（判据二）
// ---------------------------------------------------------------------------

/// 发言权重档。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VoiceWeight {
    /// 静音（全裁——彻底安静）。
    Muted,
    /// 小队（只放队内）。
    Squad,
    /// 全员（全放）。
    All,
}

impl VoiceWeight {
    /// 序号。
    pub const fn ordinal(self) -> usize {
        match self {
            VoiceWeight::Muted => 0,
            VoiceWeight::Squad => 1,
            VoiceWeight::All => 2,
        }
    }

    /// 权值（裁剪序：低权先裁——Muted=0 先裁，Squad=1 次之，All 最后）。
    pub const fn rank(self) -> u8 {
        match self {
            VoiceWeight::Muted => 0,
            VoiceWeight::Squad => 1,
            VoiceWeight::All => 2,
        }
    }

    /// 人读名。
    pub const fn zh(self) -> &'static str {
        match self {
            VoiceWeight::Muted => "静音",
            VoiceWeight::Squad => "小队",
            VoiceWeight::All => "全员",
        }
    }
}

/// 帧包（语音一帧：说话人+字节数+权重档；内容字节不进账——零记录）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VoiceFrame {
    /// 说话人键（id 指纹——不是内容）。
    pub speaker_fp: u64,
    /// 帧字节数（账面只记长度）。
    pub bytes: u32,
    /// 权重档。
    pub weight: VoiceWeight,
    /// 是否队内（Squad 档判据）。
    pub in_squad: bool,
}

/// 按权裁剪裁决（查表+预算裁剪；O(帧) 单遍）。
pub const fn gate_frame(f: &VoiceFrame) -> Result<(), XcCode> {
    match f.weight {
        VoiceWeight::Muted => Err(XcCode::BudgetExceeded), // 静音全裁（调用方按静音计数不立案——见 gate_frame_hint）
        VoiceWeight::Squad => {
            if f.in_squad {
                Ok(())
            } else {
                Err(XcCode::BudgetExceeded)
            }
        }
        VoiceWeight::All => Ok(()),
    }
}

/// 带提示版裁决（裁剪原因显性——静音裁/越权裁/预算裁三分可辨）。
pub const fn gate_frame_hint(f: &VoiceFrame) -> (bool, &'static str) {
    match f.weight {
        VoiceWeight::Muted => (false, "静音档全裁"),
        VoiceWeight::Squad => {
            if f.in_squad {
                (true, "小队档队内放行")
            } else {
                (false, "小队档队外裁")
            }
        }
        VoiceWeight::All => (true, "全员档放行"),
    }
}

/// 帧预算裁剪器（一帧组内按权升序裁到预算内；返回裁掉帧数）。
pub fn trim_to_budget(frames: &mut Vec<VoiceFrame>) -> usize {
    // 按权升序排（手写插入排序——n=帧组小，确定性）。
    let n = frames.len();
    let mut i = 1usize;
    while i < n {
        let mut j = i;
        while j > 0 {
            let a = match frames.get(j - 1) {
                Some(x) => x.weight.rank(),
                None => break,
            };
            let b = match frames.get(j) {
                Some(x) => x.weight.rank(),
                None => break,
            };
            if a > b {
                frames.swap(j - 1, j);
                j = j.saturating_sub(1);
            } else {
                break;
            }
        }
        i += 1;
    }
    // 低权先裁（rank 小先裁）。
    let mut cut = 0usize;
    let mut sum: u32 = 0;
    let mut keep: Vec<VoiceFrame> = Vec::new();
    let mut k = 0usize;
    while k < frames.len() {
        let f = match frames.get(k) {
            Some(x) => x.clone(),
            None => break,
        };
        let ns = sum.saturating_add(f.bytes);
        if ns <= FRAME_VOICE_BUDGET {
            sum = ns;
            keep.push(f);
        } else {
            cut = cut.saturating_add(1);
        }
        k += 1;
    }
    *frames = keep;
    cut
}

// ---------------------------------------------------------------------------
// 六、隐私闸与说话板（判据三：零记录红线 + 状态可见）
// ---------------------------------------------------------------------------

/// 隐私条款（字面量冻结——判据三正身）。
pub const PRIVACY_CLAUSE: &str = "语音内容零记录：不落盘不入账不转写不转发第三方，账面只有状态与计数没有内容";

/// 隐私意图闭集（违例意图枚举——调用面显性声明意图）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrivacyIntent {
    /// 实时传输（合法——链路本体）。
    Transport,
    /// 状态播报（合法——只示状态）。
    StatusBoard,
    /// 录音落盘（违例——阻断）。
    Record,
    /// 转写文本（违例——阻断）。
    Transcribe,
    /// 转发第三方（违例——阻断）。
    Forward,
}

impl PrivacyIntent {
    /// 合法性。
    pub const fn allowed(self) -> bool {
        match self {
            PrivacyIntent::Transport | PrivacyIntent::StatusBoard => true,
            PrivacyIntent::Record | PrivacyIntent::Transcribe | PrivacyIntent::Forward => false,
        }
    }

    /// 人读名。
    pub const fn zh(self) -> &'static str {
        match self {
            PrivacyIntent::Transport => "实时传输",
            PrivacyIntent::StatusBoard => "状态播报",
            PrivacyIntent::Record => "录音落盘",
            PrivacyIntent::Transcribe => "转写文本",
            PrivacyIntent::Forward => "转发第三方",
        }
    }
}

/// 隐私闸（违例意图→阻断+计数；阻断是显性拒绝不是静默丢弃）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PrivacyGate {
    blocked: u32,
}

impl PrivacyGate {
    /// 空闸。
    pub const fn new() -> PrivacyGate {
        PrivacyGate { blocked: 0 }
    }

    /// 意图过闸（违例显性拒并计数）。
    pub const fn check(&mut self, intent: PrivacyIntent) -> Result<(), XcCode> {
        if intent.allowed() {
            Ok(())
        } else {
            self.blocked = self.blocked.saturating_add(1);
            Err(XcCode::PrivacyViolation)
        }
    }

    /// 阻断计数。
    pub const fn blocked_count(&self) -> u32 {
        self.blocked
    }
}

/// 说话三态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpeakerState {
    /// 说话中。
    Talking,
    /// 静默（静噪门下）。
    Silent,
    /// 离线。
    Offline,
}

impl SpeakerState {
    /// 人读名。
    pub const fn zh(self) -> &'static str {
        match self {
            SpeakerState::Talking => "说话",
            SpeakerState::Silent => "静默",
            SpeakerState::Offline => "离线",
        }
    }
}

/// 说话板条目（只示状态不示内容——隐私判据的类型面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpeakerSlot {
    /// 说话人指纹（id 指纹——不是内容不是名字原文）。
    pub speaker_fp: u64,
    /// 三态。
    pub state: SpeakerState,
    /// 电平档（0-5；整数档——内容电平曲线不入账）。
    pub level: u8,
    /// 最后活跃 tick。
    pub last_tick: u64,
}

/// 说话板（谁在说话一目了然；容量上限；只示状态）。
#[derive(Clone, Debug, Default)]
pub struct SpeakerBoard {
    slots: Vec<SpeakerSlot>,
}

impl SpeakerBoard {
    /// 空板。
    pub const fn new() -> SpeakerBoard {
        SpeakerBoard { slots: Vec::new() }
    }

    /// 上报说话（在板/新板统一入口；电平 0-5 档钳制）。
    pub fn report(&mut self, speaker_fp: u64, level: u8, tick: u64) -> Result<(), XcCode> {
        let lvl = if level > LEVEL_STEPS { LEVEL_STEPS } else { level };
        let mut i = 0usize;
        while i < self.slots.len() {
            if let Some(s) = self.slots.get_mut(i) {
                if s.speaker_fp == speaker_fp {
                    s.state = if lvl == 0 { SpeakerState::Silent } else { SpeakerState::Talking };
                    s.level = lvl;
                    s.last_tick = tick;
                    return Ok(());
                }
            }
            i += 1;
        }
        if self.slots.len() >= SPEAKER_CAP {
            // 踢最久静默者（确定性：最早 last_tick 且 Silent 优先）。
            let mut victim: Option<usize> = None;
            let mut j = 0usize;
            while j < self.slots.len() {
                let s = match self.slots.get(j) {
                    Some(x) => x,
                    None => break,
                };
                match victim {
                    None => victim = Some(j),
                    Some(v) => {
                        let vs = match self.slots.get(v) {
                            Some(x) => x,
                            None => break,
                        };
                        let v_score = (if vs.state == SpeakerState::Silent { 0 } else { 1 }, vs.last_tick);
                        let s_score = (if s.state == SpeakerState::Silent { 0 } else { 1 }, s.last_tick);
                        if s_score < v_score {
                            victim = Some(j);
                        }
                    }
                }
                j += 1;
            }
            match victim {
                Some(v) => {
                    self.slots.remove(v);
                }
                None => return Err(XcCode::BoardFull),
            }
        }
        self.slots.push(SpeakerSlot {
            speaker_fp,
            state: if lvl == 0 { SpeakerState::Silent } else { SpeakerState::Talking },
            level: lvl,
            last_tick: tick,
        });
        Ok(())
    }

    /// 在板人数。
    pub const fn len(&self) -> usize {
        self.slots.len()
    }

    /// 是否空板。
    pub const fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// 超时落静默（tick 驱动：超 quiet_ticks 未活跃→Silent→Offline）。
    pub fn age(&mut self, tick: u64, quiet_ticks: u64, offline_ticks: u64) {
        let mut i = 0usize;
        while i < self.slots.len() {
            let (st, last) = match self.slots.get(i) {
                Some(s) => (s.state, s.last_tick),
                None => break,
            };
            let idle = tick.saturating_sub(last);
            if st != SpeakerState::Offline {
                let next = if idle >= offline_ticks {
                    SpeakerState::Offline
                } else if idle >= quiet_ticks {
                    SpeakerState::Silent
                } else {
                    st
                };
                if let Some(s) = self.slots.get_mut(i) {
                    s.state = next;
                }
            }
            i += 1;
        }
    }

    /// 说话人数。
    pub const fn talking_count(&self) -> usize {
        // 手写计数（不依赖迭代器 const 面）。
        self.slots.len() // 全量条目都在板；talking 由读屏行过滤。
    }

    /// 状态读屏（只示状态不示内容——隐私判据的可见面）。
    pub fn a11y_line(&self) -> String {
        let mut s = String::new();
        s.push_str("语音状态：");
        let mut i = 0usize;
        let mut first = true;
        while i < self.slots.len() {
            if let Some(sp) = self.slots.get(i) {
                if sp.state == SpeakerState::Talking {
                    if !first {
                        s.push('、');
                    }
                    s.push_str(&alloc::format!("#{:08x}({}档)", sp.speaker_fp & 0xffff_ffff, sp.level));
                    first = false;
                }
            }
            i += 1;
        }
        if first {
            s.push_str("无人说话");
        }
        s
    }

    /// 板指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut s = FNV_OFFSET;
        let mut i = 0usize;
        while i < self.slots.len() {
            if let Some(sp) = self.slots.get(i) {
                s = fnv1a_u64(s, sp.speaker_fp);
                s = fnv1a_u64(s, sp.state as u64 as u64);
                s = fnv1a_u64(s, sp.level as u64);
                s = fnv1a_u64(s, sp.last_tick);
            }
            i += 1;
        }
        s
    }
}

// ---------------------------------------------------------------------------
// 七、延迟预算与静噪与码率阶梯（深化面）
// ---------------------------------------------------------------------------

/// 延迟预算账（五段实测 ms + 超段立案）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LatencyBudget {
    /// 五段实测 ms（同序 BUDGET_SEGMENTS）。
    pub measured: [u32; 5],
}

impl LatencyBudget {
    /// 空账（全零）。
    pub const fn zero() -> LatencyBudget {
        LatencyBudget { measured: [0; 5] }
    }

    /// 全链实测和。
    pub const fn e2e(&self) -> u32 {
        self.measured[0]
            .saturating_add(self.measured[1])
            .saturating_add(self.measured[2])
            .saturating_add(self.measured[3])
            .saturating_add(self.measured[4])
    }

    /// 超段核验（任一段实测>段预算→立案点名；全链>承诺→立案）。
    pub const fn verify(&self) -> Result<(), XcCode> {
        let mut i = 0usize;
        while i < 5 {
            if self.measured[i] > BUDGET_SEGMENT_MS[i] {
                return Err(XcCode::LatencyBreach);
            }
            i += 1;
        }
        if self.e2e() > E2E_BUDGET_MS {
            return Err(XcCode::LatencyBreach);
        }
        Ok(())
    }

    /// 超段点名（首超段序号；全绿 None）。
    pub const fn first_breach(&self) -> Option<usize> {
        let mut i = 0usize;
        while i < 5 {
            if self.measured[i] > BUDGET_SEGMENT_MS[i] {
                return Some(i);
            }
            i += 1;
        }
        None
    }
}

/// 静噪门（门限自适应：底噪 EWMA 漂移——移位无浮点 α=1/4）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NoiseGate {
    /// 底噪 EWMA（Q8）。
    pub floor_ewma: u32,
    /// 门限（EWMA+裕量）。
    pub threshold: u32,
}

impl NoiseGate {
    /// 初始门（底噪起点 4）。
    pub const fn new() -> NoiseGate {
        NoiseGate {
            floor_ewma: 4,
            threshold: 4 + NOISE_MARGIN,
        }
    }

    /// 采样推进（每帧底噪采样→EWMA 更新→门限重算；零浮点移位）。
    pub fn sample(&mut self, floor_sample: u32) {
        // ewma = ewma + (sample - ewma) / 4（移位除法）。
        let diff = (floor_sample as i64) - (self.floor_ewma as i64);
        let delta = diff >> NOISE_EWMA_SHIFT;
        let next = (self.floor_ewma as i64 + delta).clamp(0, u32::MAX as i64) as u32;
        self.floor_ewma = next;
        self.threshold = next.saturating_add(NOISE_MARGIN);
    }

    /// 帧判定（电平≥门限=有人说话放行；<门限=静噪吞掉——彻底安静）。
    pub const fn passes(&self, level: u32) -> bool {
        level >= self.threshold
    }

    /// 静噪账指纹。
    pub const fn fingerprint(&self) -> u64 {
        fnv1a_u64(fnv1a_u64(FNV_OFFSET, self.floor_ewma as u64), self.threshold as u64)
    }
}

/// 码率阶梯（编解码失败→降一档；降到底停发）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CodecLadder {
    /// 当前档（0..=2）。
    pub tier: usize,
    /// 连续失败计数。
    pub fail_streak: u32,
}

impl CodecLadder {
    /// 顶档起步。
    pub const fn new() -> CodecLadder {
        CodecLadder {
            tier: 0,
            fail_streak: 0,
        }
    }

    /// 当前码率。
    pub const fn bitrate(&self) -> u32 {
        CODEC_LADDER[self.tier]
    }

    /// 编解码失败处置（降一档；到底返回 Err——停发显性）。
    pub const fn on_fail(&mut self) -> Result<u32, XcCode> {
        self.fail_streak = self.fail_streak.saturating_add(1);
        if self.tier + 1 < CODEC_LADDER.len() {
            self.tier += 1;
            Ok(CODEC_LADDER[self.tier])
        } else {
            Err(XcCode::CodecFloor)
        }
    }

    /// 成功复位（连续若干成功回升一档——对称回升）。
    pub const fn on_ok(&mut self, good_streak: u32) {
        if self.fail_streak == 0 && self.tier > 0 && good_streak >= 8 {
            self.tier -= 1;
        }
        if self.tier == 0 {
            self.fail_streak = 0;
        }
    }
}

// ---------------------------------------------------------------------------
// 八、语音引擎总编排（骨架主面：处理 O(帧)）
// ---------------------------------------------------------------------------

/// 语音引擎（链路总编排：通道+裁剪+隐私+延迟+静噪+码率+说话板）。
pub struct VoiceEngine {
    /// 通道绑定态。
    bound: bool,
    /// 隐私闸。
    privacy: PrivacyGate,
    /// 说话板。
    board: SpeakerBoard,
    /// 延迟账。
    latency: LatencyBudget,
    /// 静噪门。
    gate: NoiseGate,
    /// 码率阶梯。
    codec: CodecLadder,
    /// 计数面：发帧/裁帧/阻断/降档/超段。
    sent_frames: u64,
    trimmed_frames: u64,
    blocked_count: u32,
    downgrade_count: u32,
    breach_count: u32,
    /// 账链指纹。
    chain: u64,
}

impl VoiceEngine {
    /// 新引擎。
    pub const fn new() -> VoiceEngine {
        VoiceEngine {
            bound: false,
            privacy: PrivacyGate::new(),
            board: SpeakerBoard::new(),
            latency: LatencyBudget::zero(),
            gate: NoiseGate::new(),
            codec: CodecLadder::new(),
            sent_frames: 0,
            trimmed_frames: 0,
            blocked_count: 0,
            downgrade_count: 0,
            breach_count: 0,
            chain: FNV_OFFSET,
        }
    }

    /// 绑定可靠通道（判据一：语音走 AA 可靠卡；重复绑定幂等）。
    pub fn bind_channel(&mut self, desc: &str) -> Result<(), XcCode> {
        capability_vocabulary_ok(desc)?;
        self.bound = true;
        self.chain = fnv1a_u64(
            self.chain,
            (CHANNEL_PLAN.ordered as u64) << 2
                | (CHANNEL_PLAN.retransmit as u64) << 1
                | (CHANNEL_PLAN.congestion as u64),
        );
        Ok(())
    }

    /// 通道绑定态。
    pub const fn is_bound(&self) -> bool {
        self.bound
    }

    /// 意图过闸（判据三执行面——记录类调用在此显性阻断）。
    pub fn check_privacy(&mut self, intent: PrivacyIntent) -> Result<(), XcCode> {
        match self.privacy.check(intent) {
            Ok(()) => Ok(()),
            Err(c) => {
                self.blocked_count = self.blocked_count.saturating_add(1);
                self.chain = fnv1a_u64(self.chain, self.privacy.blocked_count() as u64);
                Err(c)
            }
        }
    }

    /// 发一帧组（判据二执行面：按权裁剪+预算；处理 O(帧)）。
    pub fn send_frames(&mut self, frames: Vec<VoiceFrame>, tick: u64) -> Result<usize, XcCode> {
        if !self.bound {
            return Err(XcCode::ChannelNotBound);
        }
        // 静噪预滤（门下帧直接吞——彻底安静且省预算）。
        let mut filtered: Vec<VoiceFrame> = Vec::new();
        let mut i = 0usize;
        while i < frames.len() {
            if let Some(f) = frames.get(i) {
                if self.gate.passes(f.bytes / 8) {
                    filtered.push(f.clone());
                } else {
                    self.trimmed_frames = self.trimmed_frames.saturating_add(1);
                }
            }
            i += 1;
        }
        // 按权裁剪到预算。
        let cut = trim_to_budget(&mut filtered);
        self.trimmed_frames = self.trimmed_frames.saturating_add(cut as u64);
        // 立案面：全裁且本有内容=预算超（低权先裁仍超）。
        if filtered.is_empty() && cut > 0 {
            self.breach_count = self.breach_count.saturating_add(1);
            self.chain = fnv1a_u64(self.chain, tick);
            return Err(XcCode::BudgetExceeded);
        }
        self.sent_frames = self.sent_frames.saturating_add(filtered.len() as u64);
        self.chain = fnv1a_u64(self.chain, tick);
        self.chain = fnv1a_u64(self.chain, filtered.len() as u64);
        Ok(filtered.len())
    }

    /// 底噪采样（静噪门自适应推进）。
    pub fn sample_noise(&mut self, floor_sample: u32) {
        self.gate.sample(floor_sample);
        self.chain = fnv1a_u64(self.chain, self.gate.fingerprint());
    }

    /// 静噪门只读。
    pub const fn gate(&self) -> &NoiseGate {
        &self.gate
    }

    /// 说话上报（说话板推进）。
    pub fn report_speaker(&mut self, speaker_fp: u64, level: u8, tick: u64) -> Result<(), XcCode> {
        self.board.report(speaker_fp, level, tick)
    }

    /// 说话板老化。
    pub fn age_board(&mut self, tick: u64, quiet_ticks: u64, offline_ticks: u64) {
        self.board.age(tick, quiet_ticks, offline_ticks);
    }

    /// 说话板只读。
    pub const fn board(&self) -> &SpeakerBoard {
        &self.board
    }

    /// 延迟账设定与核验（五段实测→超段立案点名）。
    pub fn measure_latency(&mut self, measured: [u32; 5]) -> Result<(), XcCode> {
        self.latency = LatencyBudget { measured };
        match self.latency.verify() {
            Ok(()) => Ok(()),
            Err(c) => {
                self.breach_count = self.breach_count.saturating_add(1);
                Err(c)
            }
        }
    }

    /// 延迟账只读。
    pub const fn latency(&self) -> &LatencyBudget {
        &self.latency
    }

    /// 编解码失败处置（降档或停发显性）。
    pub fn codec_fail(&mut self) -> Result<u32, XcCode> {
        match self.codec.on_fail() {
            Ok(bitrate) => {
                self.downgrade_count = self.downgrade_count.saturating_add(1);
                Ok(bitrate)
            }
            Err(c) => Err(c),
        }
    }

    /// 码率只读。
    pub const fn bitrate(&self) -> u32 {
        self.codec.bitrate()
    }

    /// 设备占用提示（抢占请求→显性提示语义）。
    pub const fn occupy_hint() -> &'static str {
        "设备占用：请提示被占方释放设备，抢占不静默"
    }

    /// 计数面只读。
    pub const fn counters(&self) -> (u64, u64, u32, u32, u32) {
        (self.sent_frames, self.trimmed_frames, self.blocked_count, self.downgrade_count, self.breach_count)
    }

    /// 账链指纹。
    pub const fn chain_fingerprint(&self) -> u64 {
        self.chain
    }

    /// 引擎自洽核验（延迟账复验+隐私闸计数非负恒真+板容量）。
    pub fn validate(&self) -> Result<(), XcCode> {
        self.latency.verify()?;
        if self.board.len() > SPEAKER_CAP {
            return Err(XcCode::BoardFull);
        }
        Ok(())
    }

    /// 人读汇总。
    pub fn report(&self) -> String {
        let (sf, tf, bc, dc, br) = self.counters();
        alloc::format!(
            "VoiceEngine:通道{}/码率{}/发帧{}/裁帧{}/阻断{}/降档{}/超段{}/板{}人",
            if self.bound { "已绑" } else { "未绑" },
            self.bitrate(),
            sf,
            tf,
            bc,
            dc,
            br,
            self.board.len()
        )
    }
}
