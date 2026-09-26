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
