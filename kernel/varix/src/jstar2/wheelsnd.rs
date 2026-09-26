//! F622 滚轮交互音效 · 完整设计（STAR I 主册 J-B 组）。
//!
//! **判据（主册原文）**：音效触发与档位同步（逐档边界）；40% 音量与
//! 两音色；E5 交互音类目登记；与 F341 勿扰联动降音；平滑滚静默判据；
//! 默认关。
//!
//! **语义**：
//! - 滚轮「咔哒」微音效，**默认关**（开了才有「实体轮子」的确认感，
//!   关了零声音零痕迹）；
//! - **逐档边界**：只在逐档滚（F605 notch 模式）的档位事件发声——
//!   平滑滚无档无声（物理逻辑自洽：音效跟着刻度走不跟速度走）；
//! - **40% 音量**（400/1000）与**两音色**（机械/软胶——两种合成波形
//!   参数族）；
//! - **E5 类目**：声音方案新增「交互音」类目（与提示音/通知音并立、
//!   总闸独立——`E5Category::Interactive` 登记进类目表，一处登记不
//!   散设）；
//! - **勿扰联动**：F341 勿扰档把交互音量按比例压低（深夜自动轻下去，
//!   不用手动管——勿扰比例由显式注入口承接）。

use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量
// ---------------------------------------------------------------------------

/// 基础音量（40%，千分位）。
pub const BASE_VOLUME_M: i64 = 400;
/// E5 交互音类目名（登记唯一事实源）。
pub const E5_INTERACTIVE_CATEGORY: &str = "interactive";

// ---------------------------------------------------------------------------
// 音效模型
// ---------------------------------------------------------------------------

/// 两音色（合成波形参数族——宿主面用确定性整数波形，实机接 E5 引擎）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Timbre {
    /// 机械：短促方波族（attack 1ms、decay 18ms、基频 2.1kHz）。
    Mechanical,
    /// 软胶：圆润正弦族（attack 3ms、decay 28ms、基频 1.4kHz）。
    Soft,
}

impl Timbre {
    /// 波形合成参数（attack/decay ms、基频 Hz）。
    pub fn envelope(self) -> (u32, u32, u32) {
        match self {
            Timbre::Mechanical => (1, 18, 2100),
            Timbre::Soft => (3, 28, 1400),
        }
    }

    pub fn zh(self) -> &'static str {
        match self {
            Timbre::Mechanical => "机械",
            Timbre::Soft => "软胶",
        }
    }
}

/// 滚轮模式（F605 刻度档的显式注入口——逐档边界的数据面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WheelMode {
    /// 逐档：每格一格触发一次 notch 事件。
    Notch,
    /// 平滑连滚：无档位事件（音效静默的物理基础）。
    Smooth,
    /// 按应用默认（调用方解析后的实际模式由 `resolve` 给出）。
    Auto,
}

/// 勿扰面（F341 的显式注入口）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DndState {
    /// 勿扰是否生效。
    pub active: bool,
    /// 生效时的交互音缩放（千分位；勿扰不是静音交互音——是「轻下去」）。
    pub volume_scale_m: i64,
}

impl Default for DndState {
    fn default() -> Self {
        DndState { active: false, volume_scale_m: 1000 }
    }
}

/// 滚轮音效偏好（默认关——判据原文）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WheelSoundPrefs {
    pub enabled: bool,
    pub timbre: Timbre,
}

impl Default for WheelSoundPrefs {
    fn default() -> Self {
        WheelSoundPrefs { enabled: false, timbre: Timbre::Mechanical }
    }
}

/// 一次发声请求（引擎消费面：参数齐全、可直接接 E5 引擎）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SoundCue {
    pub category: &'static str,
    pub timbre: Timbre,
    /// 有效音量（千分位；base × 勿扰缩放）。
    pub volume_m: i64,
    pub at_ms: u64,
}

/// 滚轮事件 → 发声决策（纯函数；判据的决策面）。
///
/// - 关档：恒静默；
/// - 平滑模式：恒静默（平滑滚静默判据）；
/// - 逐档模式 + 开档：每格一响（音效触发与档位同步——1:1），
///   音量 = base(40%) × 勿扰缩放（勿扰联动降音）。
pub fn notch_event(
    prefs: &WheelSoundPrefs,
    mode: WheelMode,
    dnd: &DndState,
    at_ms: u64,
) -> Option<SoundCue> {
    if !prefs.enabled {
        return None;
    }
    if mode != WheelMode::Notch {
        return None;
    }
    let scale = if dnd.active { dnd.volume_scale_m } else { 1000 };
    Some(SoundCue {
        category: E5_INTERACTIVE_CATEGORY,
        timbre: prefs.timbre,
        volume_m: BASE_VOLUME_M * scale / 1000,
        at_ms,
    })
}

// ---------------------------------------------------------------------------
// E5 类目登记（一处登记不散设）
// ---------------------------------------------------------------------------

/// E5 声音类目表（含既有类目 + 新增交互音——扩容一处登记）。
pub const E5_CATEGORIES: [&str; 3] = ["notify", "alert", E5_INTERACTIVE_CATEGORY];

/// 交互音类目登记校验（总闸独立：交互音有独立总闸字段）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct E5CategoryRegistry {
    pub registered: Vec<&'static str>,
    /// 交互音总闸（独立于提示音/通知音总闸）。
    pub interactive_master: bool,
    pub registered_at: Option<u64>,
}

impl E5CategoryRegistry {
    pub fn new() -> E5CategoryRegistry {
        E5CategoryRegistry {
            registered: alloc::vec!["notify", "alert"],
            interactive_master: true,
            registered_at: None,
        }
    }

    /// 登记（幂等；记录登记时刻——对账面）。
    pub fn register_interactive(&mut self, at_ms: u64) -> bool {
        if self.registered.contains(&E5_INTERACTIVE_CATEGORY) {
            return false;
        }
        self.registered.push(E5_INTERACTIVE_CATEGORY);
        self.registered_at = Some(at_ms);
        true
    }

    pub fn is_registered(&self) -> bool {
        self.registered.contains(&E5_INTERACTIVE_CATEGORY)
    }
}

impl Default for E5CategoryRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 档位同步回放器（逐档边界的行为面：事件流 → 音频流 1:1）
// ---------------------------------------------------------------------------

/// 回放器：喂入滚轮事件流（模式, 时刻），产出音效流。
pub struct NotchSyncPlayer {
    prefs: WheelSoundPrefs,
    cues: Vec<SoundCue>,
    /// 静默计数（平滑事件数——对账面）。
    pub smooth_silent: u64,
    /// 发声计数。
    pub notched_sounded: u64,
}

impl NotchSyncPlayer {
    pub fn new(prefs: WheelSoundPrefs) -> NotchSyncPlayer {
        NotchSyncPlayer { prefs, cues: Vec::new(), smooth_silent: 0, notched_sounded: 0 }
    }

    /// 喂一个滚轮事件。
    pub fn feed(&mut self, mode: WheelMode, dnd: &DndState, at_ms: u64) {
        match notch_event(&self.prefs, mode, dnd, at_ms) {
            Some(cue) => {
                self.notched_sounded += 1;
                self.cues.push(cue);
            }
            None => {
                if mode == WheelMode::Smooth {
                    self.smooth_silent += 1;
                }
            }
        }
    }

    pub fn cues(&self) -> &[SoundCue] {
        &self.cues
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F622 自检。

// ---------------------------------------------------------------------------
// v2 深化：包络采样模型 / 勿扰阶梯 / cue 时长
// ---------------------------------------------------------------------------

/// 包络采样（t 毫秒时刻的振幅 0..1000 千分位；attack 线性升到峰值后
/// decay 线性衰减到零——确定性整数模型，载波波形归 E5 引擎，
/// 本模块只钉「响多久、多响」的行为面）。
pub fn envelope_sample(timbre: Timbre, t_ms: u32) -> i64 {
    let (attack, decay, _f) = timbre.envelope();
    let total = attack + decay;
    if t_ms == 0 || t_ms > total {
        return 0;
    }
    if t_ms <= attack {
        return 1000 * t_ms as i64 / attack.max(1) as i64;
    }
    let into_decay = (t_ms - attack) as i64;
    1000 - 1000 * into_decay / decay.max(1) as i64
}

/// SoundCue 时长（attack + decay——调度器据此排音频时间线）。
pub fn cue_duration_ms(cue: &SoundCue) -> u32 {
    let (a, d, _) = cue.timbre.envelope();
    a + d
}

/// 勿扰阶梯（F341 三档：强=250‰、弱=600‰、关=1000‰——深夜自动轻
/// 下去的档位化口径，与 DndState 自定义缩放并存）。
pub const DND_LADDER: [(&str, i64); 3] = [("strong", 250), ("weak", 600), ("off", 1000)];

impl DndState {
    /// 从阶梯档构造（查无档位 → 关档 1000‰——不猜）。
    pub fn from_ladder(level: &str) -> DndState {
        let scale = DND_LADDER
            .iter()
            .find(|(k, _)| *k == level)
            .map(|(_, v)| *v)
            .unwrap_or(1000);
        DndState { active: scale != 1000, volume_scale_m: scale }
    }
}

pub fn run_wheelsnd_checks() -> CheckSet {
    let mut set = CheckSet::new("jstar2-F622");
    let on = WheelSoundPrefs { enabled: true, timbre: Timbre::Mechanical };
    let off = WheelSoundPrefs::default();
    let dnd = DndState { active: true, volume_scale_m: 400 };
    let no_dnd = DndState::default();

    // 1. 音效触发与档位同步：10 个逐档事件 → 10 响（1:1）。
    let mut p = NotchSyncPlayer::new(on);
    for i in 0..10u64 {
        p.feed(WheelMode::Notch, &no_dnd, i * 40);
    }
    set.add(
        "notch events 1:1 with cues",
        p.notched_sounded == 10 && p.cues().len() == 10,
        "",
    );

    // 2. 平滑滚静默判据：平滑事件零发声 + 计数如实。
    let mut p2 = NotchSyncPlayer::new(on);
    for i in 0..50u64 {
        p2.feed(WheelMode::Smooth, &no_dnd, i);
    }
    set.add("smooth scroll silent with honest count", p2.cues().is_empty() && p2.smooth_silent == 50, "");

    // 3. 逐档边界：模式切换即刻生效（平滑段无残留、恢复逐档续响）。
    let mut p3 = NotchSyncPlayer::new(on);
    p3.feed(WheelMode::Notch, &no_dnd, 0);
    p3.feed(WheelMode::Smooth, &no_dnd, 30);
    p3.feed(WheelMode::Smooth, &no_dnd, 60);
    p3.feed(WheelMode::Notch, &no_dnd, 90);
    set.add(
        "mode switch boundary no residue",
        p3.notched_sounded == 2 && p3.smooth_silent == 2,
        "",
    );

    // 4. 40% 音量与两音色：参数面 + 勿扰联动降音。
    let cue = notch_event(&on, WheelMode::Notch, &no_dnd, 0).unwrap();
    let cued = notch_event(&on, WheelMode::Notch, &dnd, 0).unwrap();
    set.add(
        "40% volume and dnd scaling",
        cue.volume_m == 400 && cued.volume_m == 160 && cue.category == "interactive",
        "",
    );
    let soft = WheelSoundPrefs { enabled: true, timbre: Timbre::Soft };
    let (a1, d1, f1) = Timbre::Mechanical.envelope();
    let (a2, d2, f2) = Timbre::Soft.envelope();
    set.add(
        "two timbres distinct envelopes",
        (a1, d1, f1) != (a2, d2, f2) && notch_event(&soft, WheelMode::Notch, &no_dnd, 0).unwrap().timbre == Timbre::Soft,
        "",
    );

    // 5. 默认关：关档任何事件零发声（零痕迹）。
    let mut p4 = NotchSyncPlayer::new(off);
    for i in 0..20u64 {
        p4.feed(WheelMode::Notch, &no_dnd, i);
    }
    set.add("default off zero cues", p4.cues().is_empty() && p4.notched_sounded == 0, "");

    // 6. E5 交互音类目登记：一处登记、幂等、总闸独立。
    let mut reg = E5CategoryRegistry::new();
    let first = reg.register_interactive(500);
    let again = reg.register_interactive(600);
    set.add(
        "E5 interactive category registered idempotent",
        first && !again && reg.is_registered() && reg.interactive_master && E5_CATEGORIES.contains(&"interactive"),
        "",
    );

    // 7. Auto 模式不在音效层解析（显式注入纪律：调用方 resolve 后喂实际模式）。
    set.add("auto mode silent at cue layer", notch_event(&on, WheelMode::Auto, &no_dnd, 0).is_none(), "");


    // 7. 包络采样：attack 段升、decay 段降、总时长外归零、同参同出。
    let (at, de, _) = Timbre::Mechanical.envelope();
    let s0 = envelope_sample(Timbre::Mechanical, 0);
    let s_attack = envelope_sample(Timbre::Mechanical, at);
    let s_mid = envelope_sample(Timbre::Mechanical, at + de / 2);
    let s_end = envelope_sample(Timbre::Mechanical, at + de);
    let s_again = envelope_sample(Timbre::Mechanical, at + de / 2);
    set.add(
        "envelope rises decays and is deterministic",
        s0 == 0 && s_attack == 1000 && s_mid > 0 && s_mid < 1000 && s_end == 0 && s_mid == s_again,
        "",
    );

    // 8. 两音色时长不同（机械短促、软胶圆润——参数族的可闻差异面）。
    let mech_cue = SoundCue { category: E5_INTERACTIVE_CATEGORY, timbre: Timbre::Mechanical, volume_m: 400, at_ms: 0 };
    let soft_cue = SoundCue { category: E5_INTERACTIVE_CATEGORY, timbre: Timbre::Soft, volume_m: 400, at_ms: 0 };
    set.add(
        "cue duration follows timbre",
        cue_duration_ms(&mech_cue) == 19 && cue_duration_ms(&soft_cue) == 31,
        "",
    );

    // 9. 勿扰阶梯：强/弱/关三档有效音量逐级递增 + 未知档诚实回关。
    let mut prefs9 = WheelSoundPrefs::default();
    prefs9.enabled = true;
    let strong = notch_event(&prefs9, WheelMode::Notch, &DndState::from_ladder("strong"), 1).unwrap();
    let weak = notch_event(&prefs9, WheelMode::Notch, &DndState::from_ladder("weak"), 1).unwrap();
    let off = notch_event(&prefs9, WheelMode::Notch, &DndState::from_ladder("off"), 1).unwrap();
    let unknown_dnd = DndState::from_ladder("深夜");
    let unknown = notch_event(&prefs9, WheelMode::Notch, &unknown_dnd, 1).unwrap();
    set.add(
        "dnd ladder scales volume honestly",
        strong.volume_m == 100
            && weak.volume_m == 240
            && off.volume_m == BASE_VOLUME_M
            && unknown.volume_m == BASE_VOLUME_M
            && !unknown_dnd.active,
        "",
    );

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cue_volume_floor_never_negative() {
        // 极端勿扰缩放 0 → 音量 0（仍发声但无声级——诚实映射）。
        let dnd0 = DndState { active: true, volume_scale_m: 0 };
        let cue = notch_event(&WheelSoundPrefs { enabled: true, timbre: Timbre::Soft }, WheelMode::Notch, &dnd0, 0).unwrap();
        assert_eq!(cue.volume_m, 0);
    }

    #[test]
    fn mechanical_is_default_timbre() {
        assert_eq!(WheelSoundPrefs::default().timbre, Timbre::Mechanical);
    }

    #[test]
    fn dnd_inactive_uses_full_base() {
        let cue = notch_event(&WheelSoundPrefs { enabled: true, timbre: Timbre::Mechanical }, WheelMode::Notch, &DndState { active: false, volume_scale_m: 10 }, 7).unwrap();
        assert_eq!(cue.volume_m, 400, "勿扰未生效时不缩放");
        assert_eq!(cue.at_ms, 7);
    }

    #[test]
    fn player_keeps_chronological_order() {
        let mut p = NotchSyncPlayer::new(WheelSoundPrefs { enabled: true, timbre: Timbre::Soft });
        p.feed(WheelMode::Notch, &DndState::default(), 30);
        p.feed(WheelMode::Notch, &DndState::default(), 10);
        let times: Vec<u64> = p.cues().iter().map(|c| c.at_ms).collect();
        assert_eq!(times, alloc::vec![30, 10], "事件流保序（不做排序魔法）");
    }

    #[test]
    fn envelope_values_sane() {
        let (a, d, f) = Timbre::Mechanical.envelope();
        assert!(a > 0 && d > a && f > 1000);
        let (a2, d2, f2) = Timbre::Soft.envelope();
        assert!(a2 > a && d2 > d && f2 < f);
    }
}

// ---------------------------------------------------------------------------
// v3 深化批：载波合成模型 · 防爆音渐变 · 档位音高映射 · 连滚合流队列
// · 总闸链 · 静默可视化替代 · 偏好 KV 序列化
// ---------------------------------------------------------------------------

/// 合成采样率（Hz）——模型面按 48kHz 逐 ms 采样粒度推导（实机面由
/// E5 引擎消费同参数；此处钉「波形长什么样」的确定性事实）。
pub const SAMPLE_RATE_HZ: u32 = 48_000;
/// 防爆音渐变时长（ms）：cue 首尾各 5ms 线性渐入渐出（三·五「莫名的
/// 卡顿/爆音」纪律——突起的波形不交出去）。
pub const FADE_MS: u32 = 5;
/// 连滚合流窗口（ms）：窗口内超量 cue 合并为一响（节流不丢节拍感）。
pub const COALESCE_WINDOW_MS: u64 = 10;
/// 队列容量上限：同窗超限即合流（防事件风暴）。
pub const MAX_QUEUED_CUES: usize = 16;

/// 防爆音渐变系数（千分位）：首尾 FADE_MS 线性渐入渐出，中段恒 1000。
/// 独立纯函数（可对拍、不掺载波过零干扰），`synth_sample` 内与包络相乘。
pub fn fade_scale(t_ms: u32, total_ms: u32) -> i64 {
    if total_ms == 0 {
        return 0;
    }
    if t_ms == 0 || t_ms > total_ms {
        return 0;
    }
    if t_ms <= FADE_MS {
        return 1000 * t_ms as i64 / FADE_MS as i64;
    }
    if t_ms + FADE_MS > total_ms {
        return 1000 * (total_ms - t_ms) as i64 / FADE_MS as i64;
    }
    1000
}

/// 载波样本（1 ms 粒度的确定性整数模型）。
///
/// - 载波：sin(2π·f·t)——经 `jbase::sin_fp`（千分位相位）定点合成；
/// - 包络：`envelope_sample`（attack/decay 线性族）；
/// - 渐变：`fade_scale`（首尾 FADE_MS 防爆音）；
/// - 输出：-1000..=1000 的千分位振幅（符号面留给消费方）。
pub fn synth_sample(timbre: Timbre, t_ms: u32, notch_pitch_m: i64) -> i64 {
    let (_, _, f) = timbre.envelope();
    let total = cue_duration_ms(&SoundCue {
        category: E5_INTERACTIVE_CATEGORY,
        timbre,
        volume_m: 1000,
        at_ms: 0,
    });
    if t_ms == 0 || t_ms > total {
        return 0;
    }
    // 相位：每 ms 前进 f/1000 个 2π——sin_fp 以千分位全周为 6283 相位单位。
    let f_eff = f.max(200) as i64 * notch_pitch_m.max(100) / 1000;
    let phase_per_ms_m = 6283 * f_eff / 1000;
    let carrier = crate::jstar2::jbase::sin_fp((phase_per_ms_m * t_ms as i64) % 6283);
    let env = envelope_sample(timbre, t_ms);
    let out = carrier * env / 1000;
    // 防爆音：首尾渐变与包络相乘（双保险不放大）。
    (out * fade_scale(t_ms, total) / 1000).clamp(-1000, 1000)
}

/// 档位音高映射：真实滚轮 8 格一圈的「棘轮」手感——notch 序号模 8 映射
/// 到 880..=1120 的音高缩放（千分位；同圈微升微降，转起来有声浪起伏）。
pub fn notch_pitch_m(notch_index: u32) -> i64 {
    const PITCH_LADDER: [i64; 8] = [880, 920, 960, 1000, 1040, 1080, 1120, 1000];
    PITCH_LADDER[(notch_index % 8) as usize]
}

/// 连滚合流队列：逐档事件进队；与上一事件间隔 < COALESCE_WINDOW_MS
/// 或队列满 → 合并为一响（保留最新——最新手感优先；合并计数如实登记）。
pub struct CoalesceQueue {
    queued: Vec<SoundCue>,
    last_seen_ms: Option<u64>,
    pub merged_count: u64,
    pub emitted_count: u64,
}

impl CoalesceQueue {
    pub fn new() -> CoalesceQueue {
        CoalesceQueue { queued: Vec::new(), last_seen_ms: None, merged_count: 0, emitted_count: 0 }
    }

    /// 喂一个候选 cue（由 `notch_event` 产出的非 None 值）。
    pub fn offer(&mut self, cue: SoundCue) {
        let coalesce = match self.last_seen_ms {
            Some(t) => cue.at_ms.saturating_sub(t) < COALESCE_WINDOW_MS,
            None => false, // 首事件永不自并
        } || self.queued.len() >= MAX_QUEUED_CUES;
        if coalesce {
            self.merged_count += 1;
            if let Some(last) = self.queued.last_mut() {
                *last = cue;
            } else {
                self.queued.push(cue);
            }
        } else {
            self.queued.push(cue);
        }
        self.last_seen_ms = Some(cue.at_ms);
    }

    /// 取走待发声队列（E5 引擎消费；取走即计数）。
    pub fn drain(&mut self) -> Vec<SoundCue> {
        self.emitted_count += self.queued.len() as u64;
        let out = core::mem::take(&mut self.queued);
        out
    }

    pub fn pending(&self) -> usize {
        self.queued.len()
    }
}
impl Default for CoalesceQueue {
    fn default() -> Self {
        Self::new()
    }
}

/// 总闸链（三道闸全开才出声——一处一事实的闸门序）：
/// 1. 偏好开关（`WheelSoundPrefs::enabled`）；
/// 2. E5 交互音总闸（`E5CategoryRegistry::interactive_master`）；
/// 3. 系统静音（显式注入参数）。
pub fn gate_chain(
    prefs: &WheelSoundPrefs,
    registry: &E5CategoryRegistry,
    system_muted: bool,
) -> bool {
    prefs.enabled && registry.interactive_master && registry.is_registered() && !system_muted
}

/// 静默可视化替代（无障碍对等——声音通道关掉时，指针旁的闪光指示
/// 代偿同拍反馈：与 cue 同时刻、同档位节奏）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MuteFlash {
    pub at_ms: u64,
    /// 闪光强度（千分位）：跟随原 cue 音量比例（1000 = 满强度）。
    pub intensity_m: i64,
}

/// 静默替代决策：任何一道闸关掉但偏好开 → 出闪光代偿（声音没了，
/// 反馈不能没——键盘/视觉对等纪律）。偏好本身关 → 什么都不出。
pub fn mute_flash_for(
    prefs: &WheelSoundPrefs,
    registry: &E5CategoryRegistry,
    system_muted: bool,
    cue: &SoundCue,
) -> Option<MuteFlash> {
    if !prefs.enabled {
        return None;
    }
    if gate_chain(prefs, registry, system_muted) {
        return None; // 声音正常出，无需代偿
    }
    Some(MuteFlash { at_ms: cue.at_ms, intensity_m: cue.volume_m.max(300) })
}

/// 偏好 KV 序列化（8 字节：魔数 | enabled | timbre | 保留位全 0）。
pub fn prefs_to_kv(p: &WheelSoundPrefs) -> [u8; 8] {
    let timbre = match p.timbre {
        Timbre::Mechanical => 0u8,
        Timbre::Soft => 1,
    };
    [0x57, 0x53, u8::from(p.enabled), timbre, 0, 0, 0, 0]
}

pub fn prefs_from_kv(kv: &[u8]) -> Option<WheelSoundPrefs> {
    if kv.len() != 8 || kv[0] != 0x57 || kv[1] != 0x53 || kv[4] != 0 || kv[5] != 0 || kv[6] != 0 || kv[7] != 0 {
        return None;
    }
    let timbre = match kv[3] {
        0 => Timbre::Mechanical,
        1 => Timbre::Soft,
        _ => return None,
    };
    Some(WheelSoundPrefs { enabled: kv[2] == 1, timbre })
}

/// v3 自检。
pub fn run_wheelsnd_v3_checks() -> CheckSet {
    let mut set = CheckSet::new("jstar2-F622-v3");

    // 1. 载波合成：峰值在包络 attack 顶、包络外归零、确定复现。
    let s_peak = synth_sample(Timbre::Mechanical, 1, 1000);
    let s_tail = synth_sample(Timbre::Mechanical, 19, 1000);
    let s_again = synth_sample(Timbre::Mechanical, 1, 1000);
    set.add(
        "synth peaks at attack and silent after total",
        s_peak.abs() <= 1000 && s_peak != 0 && s_tail == 0 && s_peak == s_again,
        "",
    );

    // 2. 防爆音渐变：首 ms 渐入、attack 顶全幅、尾 ms 渐出、窗外归零。
    let soft_total = 31; // Soft: attack 3 + decay 28
    set.add(
        "fade guards both edges",
        fade_scale(1, soft_total) == 200
            && fade_scale(5, soft_total) == 1000
            && fade_scale(10, soft_total) == 1000
            && fade_scale(30, soft_total) == 200
            && fade_scale(31, soft_total) == 0
            && fade_scale(32, soft_total) == 0,
        "",
    );

    // 3. 档位音高：8 格一圈、序号 0 与 8 同音高（周期性）。
    set.add(
        "notch pitch 8-cycle",
        notch_pitch_m(0) == 880 && notch_pitch_m(3) == 1000 && notch_pitch_m(6) == 1120 && notch_pitch_m(8) == notch_pitch_m(0),
        "",
    );

    // 4. 音高作用于合成频率：同 t 不同 pitch 输出不同（有声浪起伏）。
    let p880 = synth_sample(Timbre::Mechanical, 2, 880);
    let p1120 = synth_sample(Timbre::Mechanical, 2, 1120);
    set.add("pitch changes waveform", p880 != p1120, "");

    // 5. 连滚合流：10ms 窗内连喂 20 档 → 大量合并、drain 后计数如实。
    let mut q = CoalesceQueue::new();
    for i in 0..20u64 {
        q.offer(SoundCue { category: E5_INTERACTIVE_CATEGORY, timbre: Timbre::Mechanical, volume_m: 400, at_ms: i });
    }
    let drained = q.drain();
    set.add(
        "rapid notches coalesce honestly",
        q.merged_count > 0 && drained.len() < 20 && q.emitted_count == drained.len() as u64,
        "",
    );

    // 6. 窗外事件不合流：间隔 >10ms 逐条保留。
    let mut q2 = CoalesceQueue::new();
    for i in 0..5u64 {
        q2.offer(SoundCue { category: E5_INTERACTIVE_CATEGORY, timbre: Timbre::Soft, volume_m: 400, at_ms: i * 50 });
    }
    let d2 = q2.drain();
    set.add("spaced notches all kept", d2.len() == 5 && q2.merged_count == 0, "");

    // 7. 总闸链：三闸全开才 true；任一关 false（含未登记类目）。
    let mut reg = E5CategoryRegistry::new();
    let _ = reg.register_interactive(0);
    let prefs_on = WheelSoundPrefs { enabled: true, timbre: Timbre::Mechanical };
    let reg_no_reg = E5CategoryRegistry::new();
    set.add(
        "gate chain all three gates",
        gate_chain(&prefs_on, &reg, false)
            && !gate_chain(&prefs_on, &reg, true)
            && !gate_chain(&WheelSoundPrefs::default(), &reg, false)
            && !gate_chain(&prefs_on, &reg_no_reg, false),
        "",
    );

    // 8. 静默可视化替代：闸关 + 偏好开 → 闪光；声音正常 → 无闪光；
    //    偏好关 → 什么都不出。
    let cue = SoundCue { category: E5_INTERACTIVE_CATEGORY, timbre: Timbre::Mechanical, volume_m: 400, at_ms: 9 };
    let flash = mute_flash_for(&prefs_on, &reg, true, &cue);
    let no_flash = mute_flash_for(&prefs_on, &reg, false, &cue);
    let dead = mute_flash_for(&WheelSoundPrefs::default(), &reg, true, &cue);
    set.add(
        "mute flash substitutes when silenced",
        flash.map(|f| f.at_ms == 9 && f.intensity_m == 400).unwrap_or(false)
            && no_flash.is_none()
            && dead.is_none(),
        "",
    );

    // 9. 低音量 cue 的闪光有下限（300‰——代偿不能弱到看不见）。
    let quiet = SoundCue { category: E5_INTERACTIVE_CATEGORY, timbre: Timbre::Soft, volume_m: 100, at_ms: 1 };
    let f2 = mute_flash_for(&prefs_on, &reg, true, &quiet).unwrap();
    set.add("mute flash floor at 300", f2.intensity_m == 300, "");

    // 10. 偏好 KV 往返 + 篡改拒绝。
    let kv = prefs_to_kv(&prefs_on);
    let round = prefs_from_kv(&kv);
    let mut bad = kv;
    bad[3] = 7;
    set.add(
        "wheel prefs kv roundtrip tamper reject",
        round.map(|r| r.enabled && r.timbre == Timbre::Mechanical).unwrap_or(false)
            && prefs_from_kv(&bad).is_none()
            && prefs_from_kv(&[0u8; 8]).is_none(),
        "",
    );

    set
}

#[cfg(test)]
mod tests_v3 {
    use super::*;

    #[test]
    fn synth_is_bounded() {
        for t in 0..25u32 {
            let s = synth_sample(Timbre::Mechanical, t, 1120);
            assert!(s.abs() <= 1000, "t={t} 越界 {s}");
        }
    }

    #[test]
    fn coalesce_keeps_latest_in_window() {
        let mut q = CoalesceQueue::new();
        q.offer(SoundCue { category: E5_INTERACTIVE_CATEGORY, timbre: Timbre::Mechanical, volume_m: 400, at_ms: 1 });
        let later = SoundCue { category: E5_INTERACTIVE_CATEGORY, timbre: Timbre::Soft, volume_m: 400, at_ms: 5 };
        q.offer(later);
        let d = q.drain();
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].at_ms, 5, "窗口内保留最新");
    }

    #[test]
    fn kv_default_roundtrip() {
        let kv = prefs_to_kv(&WheelSoundPrefs::default());
        let back = prefs_from_kv(&kv).unwrap();
        assert!(!back.enabled);
        assert_eq!(back.timbre, Timbre::Mechanical);
    }
}

// ---------------------------------------------------------------------------
// v4 深化批：音高梯人话名表 · cue 分类统计 · 超长 cue 时长审计
// ---------------------------------------------------------------------------

use alloc::string::String;

/// 音高梯人话名表（与 `notch_pitch_m` 的 8 格梯一一对应——调试面板与
/// 静默闪光提示共用同一套叫法：档位说人话，不报千分位数字）。
pub const PITCH_NAMES: [&str; 8] =
    ["低咚", "次低咚", "中哒", "正拍", "次高哒", "高叮", "顶叮", "回落拍"];

/// 档位 → 人话音名（与音高梯同模 8 周期——索引 0 与 8 同名同高）。
pub fn pitch_name(notch_index: u32) -> &'static str {
    PITCH_NAMES[(notch_index % 8) as usize]
}

/// 音高梯对账行（人话名 × 音高值逐格渲染——面板一屏看清「哪格什么声、
/// 多高」；两表同行出现，漏格即对账可见）。
pub fn pitch_ladder_report() -> String {
    let mut out = String::new();
    for i in 0..8u32 {
        out.push_str(&alloc::format!("第{}格 {}（{}‰）\n", i + 1, pitch_name(i), notch_pitch_m(i)));
    }
    out
}

/// 音量带界（千分位）：≥ 此值为「醒耳」，低于为「轻声」——勿扰压音
/// 后 cue 落哪一带一眼可辨（勿扰联动降音的对账尺）。
pub const AUDIBLE_BAND_M: i64 = 300;

/// cue 分类统计（按音色两族 + 按音量两带——合成调度与勿扰对账的数据面；
/// 纯计数不改 cue，字段与总数恒自洽：mechanical + soft = total）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CueTally {
    pub total: u64,
    pub mechanical: u64,
    pub soft: u64,
    pub audible: u64,
    pub faint: u64,
}

/// 对一串 cue 做分类统计（音色两族互斥计数、音量两带按 `AUDIBLE_BAND_M`
/// 分界——空表诚实归零）。
pub fn tally_cues(cues: &[SoundCue]) -> CueTally {
    let mut t = CueTally::default();
    for c in cues {
        t.total += 1;
        match c.timbre {
            Timbre::Mechanical => t.mechanical += 1,
            Timbre::Soft => t.soft += 1,
        }
        if c.volume_m >= AUDIBLE_BAND_M {
            t.audible += 1;
        } else {
            t.faint += 1;
        }
    }
    t
}

/// 统计 → 一行人话（调试面板渲染面：数字与族名同屏）。
pub fn tally_summary(t: &CueTally) -> String {
    alloc::format!(
        "共{}响：机械{}+软胶{}；醒耳{}+轻声{}",
        t.total, t.mechanical, t.soft, t.audible, t.faint
    )
}

/// 单响时长上限（ms）：滚轮咔哒是「点声」，逐档 40ms 节奏下 24ms 已占
/// 过半，再长就糊成连续音——超线 cue 上线前必须清出去。
pub const MAX_CUE_DURATION_MS: u32 = 24;

/// 超长 cue 审计（时长超过上限的 cue 清单：序号 + 实测时长——排障面，
/// 上线前过一遍，不留拖拍提示音；机械族 19ms 恒过线，软胶 31ms 必挂）。
pub fn overlong_cues(cues: &[SoundCue]) -> Vec<(usize, u32)> {
    cues.iter()
        .enumerate()
        .map(|(i, c)| (i, cue_duration_ms(c)))
        .filter(|(_, d)| *d > MAX_CUE_DURATION_MS)
        .collect()
}

/// v4 自检。
pub fn run_wheelsnd_v4_checks() -> CheckSet {
    let mut set = CheckSet::new("jstar2-F622-v4");

    // 1. 音名表与音高梯同周期：索引 0 与 8 同名（周期性对账）。
    set.add(
        "pitch name follows 8-cycle",
        pitch_name(0) == PITCH_NAMES[0]
            && pitch_name(8) == pitch_name(0)
            && pitch_name(6) == PITCH_NAMES[6],
        "",
    );

    // 2. 人话名表全覆盖且不重名（8 格 8 叫法——面板不出现重名档）。
    let mut dup_free = true;
    for i in 0..8usize {
        for j in (i + 1)..8usize {
            if PITCH_NAMES[i] == PITCH_NAMES[j] {
                dup_free = false;
            }
        }
    }
    set.add("pitch names distinct across ladder", dup_free, "");

    // 3. 音高梯对账行：8 行齐全、首行含格号与音高起点 880。
    let rep = pitch_ladder_report();
    set.add(
        "pitch ladder report has 8 lines",
        rep.matches('\n').count() == 8 && rep.contains("第1格") && rep.contains("880"),
        "",
    );

    // 4. cue 分类统计：音色两族 + 音量两带计数与总数对账。
    let cues = alloc::vec![
        SoundCue { category: E5_INTERACTIVE_CATEGORY, timbre: Timbre::Mechanical, volume_m: 400, at_ms: 0 },
        SoundCue { category: E5_INTERACTIVE_CATEGORY, timbre: Timbre::Soft, volume_m: 400, at_ms: 50 },
        SoundCue { category: E5_INTERACTIVE_CATEGORY, timbre: Timbre::Mechanical, volume_m: 160, at_ms: 100 },
    ];
    let t = tally_cues(&cues);
    set.add(
        "cue tally splits by timbre and band",
        t.total == 3 && t.mechanical == 2 && t.soft == 1 && t.audible == 2 && t.faint == 1,
        "",
    );

    // 5. 统计人话行：数字与族名都进渲染。
    let sum = tally_summary(&t);
    set.add(
        "tally summary renders counts",
        sum.contains("共3响") && sum.contains("机械") && sum.contains("软胶"),
        "",
    );

    // 6. 空表统计诚实归零 + 摘要行如实。
    let t0 = tally_cues(&[]);
    set.add(
        "empty tally zeros honestly",
        t0 == CueTally::default() && tally_summary(&t0).contains("共0响"),
        "",
    );

    // 7. 超长审计：软胶（31ms）超线入清单、机械（19ms）不超、序号如实。
    set.add(
        "overlong audit flags soft only",
        overlong_cues(&cues) == alloc::vec![(1usize, 31u32)],
        "",
    );

    // 8. 真实链路端到端：逐档 8 格（40ms 间隔）+ 连滚 50 平滑事件 →
    //    8 响全机械全醒耳、零超长（回放器 → 统计 → 审计一条线）。
    let mut p = NotchSyncPlayer::new(WheelSoundPrefs { enabled: true, timbre: Timbre::Mechanical });
    let no_dnd = DndState::default();
    for i in 0..8u64 {
        p.feed(WheelMode::Notch, &no_dnd, i * 40);
    }
    for i in 0..50u64 {
        p.feed(WheelMode::Smooth, &no_dnd, 1000 + i);
    }
    let t2 = tally_cues(p.cues());
    set.add(
        "real playback passes audit",
        t2.total == 8 && t2.mechanical == 8 && t2.audible == 8 && overlong_cues(p.cues()).is_empty(),
        "",
    );

    set
}

#[cfg(test)]
mod tests_v4 {
    use super::*;

    #[test]
    fn pitch_name_matches_pitch_ladder() {
        // 音名与音高同周期：同一格查两表必须自洽（音高全程落在 880..=1120）。
        for i in 0..16u32 {
            assert_eq!(pitch_name(i), PITCH_NAMES[(i % 8) as usize]);
            assert!(notch_pitch_m(i) >= 880 && notch_pitch_m(i) <= 1120);
        }
    }

    #[test]
    fn dnd_cues_all_land_in_faint_band() {
        // 强勿扰（250‰）下 40% 基音量 → 100‰ 全落轻声带（联动降音可对账）。
        let prefs = WheelSoundPrefs { enabled: true, timbre: Timbre::Soft };
        let dnd = DndState::from_ladder("strong");
        let cues: Vec<SoundCue> = (0..4u64)
            .filter_map(|i| notch_event(&prefs, WheelMode::Notch, &dnd, i * 40))
            .collect();
        let t = tally_cues(&cues);
        assert_eq!(t.total, 4);
        assert_eq!(t.faint, 4);
        assert_eq!(t.audible, 0);
    }

    #[test]
    fn overlong_audit_never_flags_mechanical() {
        // 机械族 19ms 恒低于上限——审计只可能抓到软胶（拖拍面不误伤）。
        let mech = SoundCue {
            category: E5_INTERACTIVE_CATEGORY,
            timbre: Timbre::Mechanical,
            volume_m: 400,
            at_ms: 0,
        };
        assert!(overlong_cues(&[mech]).is_empty());
        assert_eq!(cue_duration_ms(&mech), 19);
    }
}
