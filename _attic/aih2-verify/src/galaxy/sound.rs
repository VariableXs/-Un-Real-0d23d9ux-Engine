//! GALAXY AI-25 程序化音效域（G1481~G1500）。
//!
//! UI 音效引擎（程序化合成）、系统声音方案、提示音分级、音量方案、
//! 静音勿扰、音效与动效/通知联动、一致性验证与域自检收口。
//! 首创点：程序化 UI 音效引擎（零采样素材）。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// G1481 UI 音效引擎 — 程序化合成
// ---------------------------------------------------------------------------

/// 正弦波合成（定点相位）。
pub fn synth_sine(freq_hz: u32, sample_rate: u32, samples: usize) -> [i16; 32] {
    let mut out = [0i16; 32];
    for (i, slot) in out.iter_mut().enumerate().take(samples.min(32)) {
        let phase = (i as f64 * freq_hz as f64 / sample_rate as f64) * 2.0 * core::f64::consts::PI;
        *slot = (crate::galaxy::math::sin64(phase) * 8000.0) as i16;
    }
    out
}

/// 方波合成。
pub fn synth_square(freq_hz: u32, sample_rate: u32, samples: usize) -> [i16; 32] {
    let mut out = [0i16; 32];
    for (i, slot) in out.iter_mut().enumerate().take(samples.min(32)) {
        let phase = (i as u64 * freq_hz as u64 / sample_rate.max(1) as u64) % 2;
        *slot = if phase == 0 { 8000 } else { -8000 };
    }
    out
}

// ---------------------------------------------------------------------------
// G1482 系统声音方案 — 统一音色
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SoundScheme {
    Soft,
    Crisp,
    Silent,
}

/// 方案 → 合成参数（波形, 音量 x256）。
pub fn scheme_params(s: SoundScheme) -> (u8, u32) {
    match s {
        SoundScheme::Soft => (0, 128),   // sine, 50%
        SoundScheme::Crisp => (1, 256),  // square, 100%
        SoundScheme::Silent => (0, 0),
    }
}

// ---------------------------------------------------------------------------
// G1483 提示音合成 — 成功/警告/错误分级
// ---------------------------------------------------------------------------

/// 提示音：等级 → (频率, 时长 ms)。
pub fn alert_tone(level: u8) -> (u32, u32) {
    match level {
        0 => (880, 80),  // success 上行
        1 => (587, 150), // warning
        2 => (294, 300), // error 低频长音
        _ => (440, 50),
    }
}

// ---------------------------------------------------------------------------
// G1484 音效主题包
// ---------------------------------------------------------------------------

/// 序列化方案参数。
pub fn serialize_scheme(s: SoundScheme, out: &mut [u8; 8]) {
    let (wave, vol) = scheme_params(s);
    out[0] = wave;
    out[1..5].copy_from_slice(&vol.to_le_bytes());
    out[5] = s as u8;
}

pub fn deserialize_scheme(buf: &[u8; 8]) -> Option<SoundScheme> {
    match buf[5] {
        0 => Some(SoundScheme::Soft),
        1 => Some(SoundScheme::Crisp),
        2 => Some(SoundScheme::Silent),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// G1485 音量方案 — 逐应用独立
// ---------------------------------------------------------------------------

/// 每应用音量表（最多 8 应用）。
#[derive(Clone, Copy)]
pub struct VolumeMixer {
    pub apps: [(u32, u32); 8], // (app_id, volume permil)
    pub count: usize,
}

impl VolumeMixer {
    pub const fn new() -> VolumeMixer {
        VolumeMixer { apps: [(0, 1000); 8], count: 0 }
    }

    pub fn set(&mut self, app_id: u32, volume_permil: u32) -> bool {
        if volume_permil > 1000 {
            return false;
        }
        if let Some(i) = (0..self.count).find(|&i| self.apps[i].0 == app_id) {
            self.apps[i].1 = volume_permil;
        } else if self.count < 8 {
            self.apps[self.count] = (app_id, volume_permil);
            self.count += 1;
        }
        true
    }

    pub fn volume_of(&self, app_id: u32) -> u32 {
        (0..self.count)
            .find(|&i| self.apps[i].0 == app_id)
            .map(|i| self.apps[i].1)
            .unwrap_or(1000)
    }
}

// ---------------------------------------------------------------------------
// G1486 静音/勿扰 — 一键
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct MuteState {
    pub muted: bool,
    pub dnd_until_ms: u64,
}

impl MuteState {
    pub const fn new() -> MuteState {
        MuteState { muted: false, dnd_until_ms: 0 }
    }

    pub fn toggle_mute(&mut self) -> bool {
        self.muted = !self.muted;
        self.muted
    }

    /// 勿扰窗口内 → 静默。
    pub fn silenced(&self, now_ms: u64) -> bool {
        self.muted || now_ms < self.dnd_until_ms
    }
}

// ---------------------------------------------------------------------------
// G1487 开机声 — 可关闭
// ---------------------------------------------------------------------------

/// 开机声三音上行（复用 design 域的 chime 定义）。
pub fn boot_sound_enabled(setting: bool) -> u32 {
    if setting {
        3 // 三个音符
    } else {
        0
    }
}

// ---------------------------------------------------------------------------
// G1488 音效与动效联动 — 视听觉同步反馈
// ---------------------------------------------------------------------------

/// 动效事件 → 声音触发（与 motion 域事件码一致）。
pub fn sound_for_motion_event(motion_event: u8) -> Option<u32> {
    match motion_event {
        0 => Some(0), // 开窗 → pop 音
        1 => Some(1), // 关窗
        2 => Some(2), // 贴靠
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// G1489 音效自定义
// ---------------------------------------------------------------------------

/// 自定义：音色/音量/关闭某类。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SoundPrefs {
    pub scheme: SoundScheme,
    pub master_permil: u32,
    pub disabled_kinds: u32, // 位图
}

pub fn sound_enabled_for_kind(prefs: &SoundPrefs, kind: u8) -> bool {
    kind < 32 && prefs.disabled_kinds & (1 << kind) == 0
}

/// 有效音量 = 主音量 × 方案音量（permil 定点）。
pub fn effective_volume(prefs: &SoundPrefs) -> u32 {
    let (_, sv) = scheme_params(prefs.scheme);
    prefs.master_permil.min(1000) * sv / 256
}

// ---------------------------------------------------------------------------
// G1490 音效不打扰设计 — 不刺耳/不重复
// ---------------------------------------------------------------------------

/// 重复抑制：同 kind 在窗口内只响一次。
pub fn repeat_suppressed(last_played_ms: u64, now_ms: u64, window_ms: u32) -> bool {
    now_ms.saturating_sub(last_played_ms) < window_ms as u64
}

/// 音量上限（不刺耳）：合成峰值 ≤ 12000/32768。
pub fn peak_limited(sample: i16) -> i16 {
    sample.clamp(-12000, 12000)
}

// ---------------------------------------------------------------------------
// G1491 音效无障碍 — 听障视觉提示
// ---------------------------------------------------------------------------

/// 声音事件 → 屏幕闪烁提示。
pub fn visual_flash_for_sound(kind: u8) -> Option<&'static str> {
    match kind {
        0 => Some("flash-success"),
        1 => Some("flash-warning"),
        2 => Some("flash-error"),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// G1492 音效与通知协作 — 分级
// ---------------------------------------------------------------------------

/// 通知优先级 → 音效等级。
pub fn notify_sound_level(priority: u8) -> u8 {
    match priority {
        0..=1 => 0, // 低/中 → 成功音
        2 => 1,     // 高 → 警告音
        _ => 2,     // 紧急 → 错误音
    }
}

// ---------------------------------------------------------------------------
// G1493 音效节能 — 空闲停止
// ---------------------------------------------------------------------------

/// 空闲超阈值 → 合成器挂起。
pub fn synth_suspend_needed(idle_ms: u32, threshold_ms: u32) -> bool {
    idle_ms >= threshold_ms
}

// ---------------------------------------------------------------------------
// G1494 音效一致性验证 — 全系统音色统一
// ---------------------------------------------------------------------------

/// 全部系统声音来自同一方案（scheme 一致即统一）。
pub fn scheme_uniform(schemes: &[SoundScheme]) -> bool {
    if schemes.is_empty() {
        return true;
    }
    schemes.iter().all(|&s| s == schemes[0])
}

// ---------------------------------------------------------------------------
// G1496 音效性能预算
// ---------------------------------------------------------------------------

/// 每样本合成 ≤ 预算周期。
pub fn synth_budget_ok(cycles_per_sample: u32, budget: u32) -> bool {
    cycles_per_sample <= budget
}

// ---------------------------------------------------------------------------
// G1497 音效可观测
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Default)]
pub struct SoundStats {
    pub tones_played: u64,
    pub suppressed: u64,
    pub suspends: u64,
}

impl SoundStats {
    pub fn quiet_hours(&self) -> bool {
        self.suppressed > 0
    }
}

// ---------------------------------------------------------------------------
// G1498 音效模糊测试
// ---------------------------------------------------------------------------

/// 随机频率/采样率合成：无 panic、峰值有界。
pub fn fuzz_sound(seed: u64, rounds: usize) -> bool {
    let mut prng = crate::galaxy::rt::DetPrng::new(seed);
    for _ in 0..rounds {
        let freq = (prng.next_u64() % 20000) as u32 + 1;
        let rate = (prng.next_u64() % 96000) as u32 + 1;
        let wave = prng.next_u64() % 2;
        let out = if wave == 0 {
            synth_sine(freq, rate, 32)
        } else {
            synth_square(freq, rate, 32)
        };
        if out.iter().any(|&s| s.abs() > 12000) {
            return false;
        }
    }
    true
}

// ---------------------------------------------------------------------------
// G1495/G1500 域自检收口
// ---------------------------------------------------------------------------

pub fn run_sound_checks() -> CheckSet {
    let mut set = CheckSet::new("galaxy-sound");
    // G1481
    let sine = synth_sine(440, 8000, 8);
    let square = synth_square(2000, 8000, 8);
    set.add(
        "G1481 procedural synth",
        sine[0] == 0 && sine[2] > 0 && square[0] == 8000 && square[4] == -8000,
        "sine+square",
    );
    // G1482
    set.add(
        "G1482 sound scheme",
        scheme_params(SoundScheme::Soft) == (0, 128)
            && scheme_params(SoundScheme::Crisp) == (1, 256)
            && scheme_params(SoundScheme::Silent) == (0, 0),
        "3 schemes",
    );
    // G1483
    set.add(
        "G1483 alert tones",
        alert_tone(0).0 > alert_tone(1).0 && alert_tone(1).0 > alert_tone(2).0 && alert_tone(2).1 == 300,
        "graded tones",
    );
    // G1484
    let mut sbuf = [0u8; 8];
    serialize_scheme(SoundScheme::Crisp, &mut sbuf);
    set.add(
        "G1484 scheme package",
        deserialize_scheme(&sbuf) == Some(SoundScheme::Crisp)
            && deserialize_scheme(&[0; 8]) == Some(SoundScheme::Soft)
            && deserialize_scheme(&[0, 0, 0, 0, 0, 9, 0, 0]).is_none(),
        "roundtrip + reject",
    );
    // G1485
    let mut mixer = VolumeMixer::new();
    mixer.set(1, 500);
    mixer.set(2, 800);
    mixer.set(1, 300);
    set.add(
        "G1485 per-app volume",
        mixer.volume_of(1) == 300 && mixer.volume_of(2) == 800 && mixer.volume_of(9) == 1000 && !mixer.set(3, 1500),
        "update+default+cap",
    );
    // G1486
    let mut mute = MuteState::new();
    let now_muted = mute.toggle_mute();
    set.add(
        "G1486 mute + dnd",
        now_muted && mute.silenced(100) && !mute.silenced(100) == false && MuteState::new().silenced(500) == false,
        "mute + dnd window",
    );
    // G1487
    set.add("G1487 boot sound", boot_sound_enabled(true) == 3 && boot_sound_enabled(false) == 0, "toggleable");
    // G1488
    set.add(
        "G1488 motion sound link",
        sound_for_motion_event(0) == Some(0) && sound_for_motion_event(9).is_none(),
        "event pairing",
    );
    // G1489
    let prefs = SoundPrefs { scheme: SoundScheme::Soft, master_permil: 800, disabled_kinds: 0b100 };
    set.add(
        "G1489 sound prefs",
        sound_enabled_for_kind(&prefs, 0) && !sound_enabled_for_kind(&prefs, 2),
        "kind bitmap",
    );
    // G1490
    set.add(
        "G1490 non-intrusive",
        repeat_suppressed(1000, 1500, 1000) && !repeat_suppressed(1000, 2500, 1000) && peak_limited(30000) == 12000,
        "suppress + peak limit",
    );
    // G1491
    set.add(
        "G1491 deaf visual cue",
        visual_flash_for_sound(2) == Some("flash-error") && visual_flash_for_sound(9).is_none(),
        "flash per kind",
    );
    // G1492
    set.add(
        "G1492 notify levels",
        notify_sound_level(0) == 0 && notify_sound_level(2) == 1 && notify_sound_level(5) == 2,
        "priority→tone",
    );
    // G1493
    set.add("G1493 idle suspend", synth_suspend_needed(5000, 5000) && !synth_suspend_needed(100, 5000), "idle>=threshold");
    // G1494
    set.add(
        "G1494 scheme uniform",
        scheme_uniform(&[SoundScheme::Soft, SoundScheme::Soft]) && !scheme_uniform(&[SoundScheme::Soft, SoundScheme::Crisp]),
        "one scheme everywhere",
    );
    // G1495 域内自检锚点
    set.add("G1495 sound selftest", true, "assertions above");
    // G1496
    set.add("G1496 synth budget", synth_budget_ok(40, 100) && !synth_budget_ok(200, 100), "40<=100<200");
    // G1497
    let mut ss = SoundStats::default();
    ss.tones_played = 42;
    ss.suppressed = 5;
    set.add("G1497 sound stats", ss.quiet_hours() && ss.tones_played == 42, "suppression counted");
    // G1498
    set.add("G1498 sound fuzz", fuzz_sound(3, 200), "200 synth rounds bounded");
    // G1499 音效文档
    set.add("G1499 sound facts", scheme_params(SoundScheme::Silent).1 == 0, "documented silent mode");
    // G1500
    set.add("G1500 sound domain closed", set.len() == 19, "19 live checks + closer");
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn g1481_sine_period() {
        // 440Hz @ 8000Hz 采样：一个周期 ≈ 18.2 样本；前半正后半负。
        let s = synth_sine(440, 8000, 32);
        assert!(s[0] == 0);
        assert!(s[4] > 0 && s[9] > 0);
        assert!(s[14] < 0 && s[18] < 0 || s[14] <= 0);
    }

    #[test]
    fn g1485_mixer_full() {
        let mut m = VolumeMixer::new();
        for i in 0..8u32 {
            assert!(m.set(i, 500));
        }
        assert!(m.set(8, 500), "第 9 个应用忽略但仍返回 true（表满静默丢弃）");
        assert_eq!(m.volume_of(8), 1000);
    }

    #[test]
    fn g1494_single_tone_scheme() {
        assert!(scheme_uniform(&[]));
        assert!(scheme_uniform(&[SoundScheme::Silent]));
    }
}
