//! m600audio — VARIX-M600 AI-09 声音设计域 (F201~F225)
//!
//! 系统声景总谱/空间音频引擎/音频低延迟直通/声卡能力档案/混音器策展/
//! 声音主题工坊/按键声学/UI 微声效库/通知声音礼仪/音量曲线大师/
//! 独占接管让位/蓝牙音频调优/麦克风阵列处理/回声消除舱/声纹识别门卫/
//! 听障视觉化/静音仪式/音频故障解剖/声音诊断室/环境声自适应/
//! 音频时间轴回放/杜比兼容层/采样率诚实化/声景回归套件/声学年报。
//!
//! 硬约束：no_std / 无 alloc / 无浮点（全部 permille/定点）/ 纯逻辑。
//! 登记类接口自带去重或容量上限拒绝；自检断言遵守"末态读取"禁令。

use crate::checks::CheckSet;

// ===========================================================================
// F201 — 系统声景总谱：total = active + muted 恒等式
// ===========================================================================

pub const SOUND_LEDGER_CAP: u32 = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SoundLedger {
    pub total: u32,
    pub active: u32,
    pub muted: u32,
}

impl SoundLedger {
    pub const fn new() -> SoundLedger {
        SoundLedger { total: 0, active: 0, muted: 0 }
    }

    /// 入账一条流。账满（≥ 256 条）拒绝并返回 false。
    pub fn add_stream(&mut self, muted: bool) -> bool {
        if self.total >= SOUND_LEDGER_CAP {
            return false;
        }
        self.total += 1;
        if muted {
            self.muted += 1;
        } else {
            self.active += 1;
        }
        true
    }

    /// 把 n 条活跃流转入静音；活跃不足则拒绝。
    pub fn mark_muted(&mut self, n: u32) -> bool {
        if n > self.active {
            return false;
        }
        self.active -= n;
        self.muted += n;
        true
    }

    pub fn consistent(&self) -> bool {
        self.active + self.muted == self.total
    }

    pub fn muted_permille(&self) -> u32 {
        if self.total == 0 {
            0
        } else {
            self.muted * 1000 / self.total
        }
    }
}

// ===========================================================================
// F202 — 空间音频引擎：方位角 → 右声道声像 permille，仰角钳 ±90°
// ===========================================================================

pub const SPHERE_FULL_DEG: u32 = 360;
pub const SPHERE_HALF_DEG: u32 = 180;
pub const ELEVATION_LIMIT_DEG: i32 = 90;

/// 0° 正前 → 0；90° 右 → 500；180° 正后 → 1000；270° 左 → 500。
pub fn pan_right_permille(azimuth_deg: u32) -> u32 {
    let az = azimuth_deg % SPHERE_FULL_DEG;
    if az <= SPHERE_HALF_DEG {
        az * 1000 / SPHERE_HALF_DEG
    } else {
        2000 - az * 1000 / SPHERE_HALF_DEG
    }
}

pub fn elevation_clamped(elevation_deg: i32) -> i32 {
    if elevation_deg > ELEVATION_LIMIT_DEG {
        ELEVATION_LIMIT_DEG
    } else if elevation_deg < -ELEVATION_LIMIT_DEG {
        -ELEVATION_LIMIT_DEG
    } else {
        elevation_deg
    }
}

// ===========================================================================
// F203 — 音频低延迟直通：缓冲与块数双闸
// ===========================================================================

pub const PASSTHRU_BUFFER_MAX_US: u32 = 5000;
pub const PASSTHRU_BLOCK_MAX: u32 = 2;

pub fn latency_passthrough_ok(buffer_us: u32, blocks: u32) -> bool {
    buffer_us <= PASSTHRU_BUFFER_MAX_US && blocks <= PASSTHRU_BLOCK_MAX
}

// ===========================================================================
// F204 — 声卡能力档案：采样率位图 + 通道数档案
// ===========================================================================

pub const AUDIO_RATE_TABLE: [u32; 5] = [8000, 22050, 44100, 48000, 96000];
pub const SOUNDCARD_CHANNEL_MAX: u8 = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SoundCardProfile {
    pub name_hash: u64,
    pub max_channels: u8,
    pub rates_mask: u32,
}

pub fn soundcard_profile_ok(p: SoundCardProfile) -> bool {
    p.name_hash != 0 && p.max_channels >= 1 && p.max_channels <= SOUNDCARD_CHANNEL_MAX
}

impl SoundCardProfile {
    pub fn supports_rate(&self, hz: u32) -> bool {
        let mut i = 0usize;
        while i < AUDIO_RATE_TABLE.len() {
            if AUDIO_RATE_TABLE[i] == hz {
                return self.rates_mask & (1u32 << i) != 0;
            }
            i += 1;
        }
        false
    }
}

// ===========================================================================
// F205 — 混音器策展：8 路增益钳位 + 总线过载探测
// ===========================================================================

pub const MIX_STRIP_COUNT: usize = 8;
pub const MIX_GAIN_MAX: u32 = 1000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MixConsole {
    pub gains: [u32; MIX_STRIP_COUNT],
}

impl MixConsole {
    pub const fn new() -> MixConsole {
        MixConsole { gains: [0; MIX_STRIP_COUNT] }
    }

    /// 设置增益：条带非法返回 false；超上限静默钳位。
    pub fn set_gain(&mut self, strip: usize, gain_permille: u32) -> bool {
        if strip >= MIX_STRIP_COUNT {
            return false;
        }
        self.gains[strip] = if gain_permille > MIX_GAIN_MAX { MIX_GAIN_MAX } else { gain_permille };
        true
    }

    pub fn bus_sum(&self) -> u32 {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < MIX_STRIP_COUNT {
            sum += self.gains[i];
            i += 1;
        }
        sum
    }

    /// 总和超过一路满增益即过载。
    pub fn bus_overload(&self) -> bool {
        self.bus_sum() > MIX_GAIN_MAX
    }
}

/// 两路按各自增益混合：out = (a*ga + b*gb) / 1000。
pub fn mix_two(a: u32, gain_a: u32, b: u32, gain_b: u32) -> u32 {
    (a * gain_a + b * gain_b) / 1000
}

// ===========================================================================
// F206 — 声音主题工坊：主题包登记（去重 + 容量上限）
// ===========================================================================

pub const SOUND_THEME_CAP: usize = 8;

#[derive(Clone, Copy, Debug)]
pub struct SoundThemeShelf {
    pub ids: [u16; SOUND_THEME_CAP],
    pub count: usize,
}

impl SoundThemeShelf {
    pub const fn new() -> SoundThemeShelf {
        SoundThemeShelf { ids: [0; SOUND_THEME_CAP], count: 0 }
    }

    pub fn register(&mut self, id: u16) -> bool {
        if self.count >= SOUND_THEME_CAP {
            return false;
        }
        let mut i = 0usize;
        while i < self.count {
            if self.ids[i] == id {
                return false;
            }
            i += 1;
        }
        self.ids[self.count] = id;
        self.count += 1;
        true
    }
}

// ===========================================================================
// F207 — 按键声学：键类 → 咔哒响度 permille
// ===========================================================================

pub const CLICK_LOUD_ALPHA: u32 = 300;
pub const CLICK_LOUD_SPACE: u32 = 500;
pub const CLICK_LOUD_ENTER: u32 = 700;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyAcoustic {
    Alpha,
    Space,
    Enter,
}

pub fn key_click_loudness(key: KeyAcoustic) -> u32 {
    match key {
        KeyAcoustic::Alpha => CLICK_LOUD_ALPHA,
        KeyAcoustic::Space => CLICK_LOUD_SPACE,
        KeyAcoustic::Enter => CLICK_LOUD_ENTER,
    }
}

// ===========================================================================
// F208 — UI 微声效库：短促声效登记（去重）+ 时长礼节
// ===========================================================================

pub const UI_SFX_CAP: usize = 16;
pub const UI_SFX_MAX_MS: u32 = 200;

#[derive(Clone, Copy, Debug)]
pub struct UiSfxShelf {
    pub ids: [u16; UI_SFX_CAP],
    pub count: usize,
}

impl UiSfxShelf {
    pub const fn new() -> UiSfxShelf {
        UiSfxShelf { ids: [0; UI_SFX_CAP], count: 0 }
    }

    pub fn register(&mut self, id: u16) -> bool {
        if self.count >= UI_SFX_CAP {
            return false;
        }
        let mut i = 0usize;
        while i < self.count {
            if self.ids[i] == id {
                return false;
            }
            i += 1;
        }
        self.ids[self.count] = id;
        self.count += 1;
        true
    }
}

/// 微声效必须短促：1..=200ms。
pub fn sfx_length_ok(ms: u32) -> bool {
    ms > 0 && ms <= UI_SFX_MAX_MS
}

// ===========================================================================
// F209 — 通知声音礼仪：静音时段内高优先级才可发声
// ===========================================================================

pub const QUIET_HOUR_FROM: u32 = 22;
pub const QUIET_HOUR_TO: u32 = 7;
pub const PRIORITY_BREAKTHROUGH: u32 = 900;

/// 静音时段 [22,24)∪[0,7) 内仅 900‰ 以上优先级可破例。
pub fn notify_sound_allowed(hour: u32, priority_permille: u32) -> bool {
    let quiet = hour >= QUIET_HOUR_FROM || hour < QUIET_HOUR_TO;
    !quiet || priority_permille >= PRIORITY_BREAKTHROUGH
}

// ===========================================================================
// F210 — 音量曲线大师：感知音量 = vol² / 1000（平方曲线）
// ===========================================================================

pub fn perceived_volume(vol_permille: u32) -> u32 {
    let v = if vol_permille > 1000 { 1000 } else { vol_permille };
    v * v / 1000
}

// ===========================================================================
// F211 — 独占接管让位：单座独占，新主入座旧主让位
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExclusiveSeat {
    pub current: Option<u32>,
    pub handoffs: u32,
}

impl ExclusiveSeat {
    pub const fn new() -> ExclusiveSeat {
        ExclusiveSeat { current: None, handoffs: 0 }
    }

    /// 占座：返回被让位的旧主（无人让位返回 None）。
    /// 同一 owner 重复占座不产生让位（去重）。
    pub fn claim(&mut self, owner: u32) -> Option<u32> {
        if self.current == Some(owner) {
            return None;
        }
        let displaced = self.current;
        self.current = Some(owner);
        if displaced.is_some() {
            self.handoffs += 1;
        }
        displaced
    }
}

// ===========================================================================
// F212 — 蓝牙音频调优：编码后码率预算
// ===========================================================================

pub const BT_A2DP_BUDGET_KBPS: u32 = 512;

/// 码率 kbps = hz*depth*ch/1000 * ratio/1000（ratio 为压缩比 permille）。
pub fn bt_bitrate_kbps(hz: u32, depth: u32, ch: u32, ratio_permille: u32) -> u32 {
    hz * depth * ch / 1000 * ratio_permille / 1000
}

pub fn bt_fits(kbps: u32) -> bool {
    kbps <= BT_A2DP_BUDGET_KBPS
}

// ===========================================================================
// F213 — 麦克风阵列处理：波束时延 + 阵元容量
// ===========================================================================

pub const MIC_ARRAY_CAP: usize = 8;

/// 声速 343 m/s：时延 us = 间距mm * 1000 / 343。
pub fn mic_delay_us(dist_mm: u32) -> u32 {
    dist_mm * 1000 / 343
}

#[derive(Clone, Copy, Debug)]
pub struct MicArrayBook {
    pub attached: u8,
}

impl MicArrayBook {
    pub const fn new() -> MicArrayBook {
        MicArrayBook { attached: 0 }
    }

    /// 挂载阵元，超容量拒绝。
    pub fn attach(&mut self) -> bool {
        if self.attached as usize >= MIC_ARRAY_CAP {
            return false;
        }
        self.attached += 1;
        true
    }
}

// ===========================================================================
// F214 — 回声消除舱：残响逐帧衰减，≤100‰ 判收敛
// ===========================================================================

pub const AEC_ADAPT_STEP: u32 = 200;
pub const AEC_CONVERGED: u32 = 100;

/// 一帧自适应：residual' = residual * 800 / 1000。
pub fn aec_step(residual_permille: u32) -> u32 {
    residual_permille * (1000 - AEC_ADAPT_STEP) / 1000
}

/// 收敛所需帧数（上限 1000 防死循环）。
pub fn aec_converge_steps(mut residual_permille: u32) -> u32 {
    let mut steps = 0u32;
    while residual_permille > AEC_CONVERGED && steps < 1000 {
        residual_permille = aec_step(residual_permille);
        steps += 1;
    }
    steps
}

// ===========================================================================
// F215 — 声纹识别门卫：850‰ 过闸，三连败锁门
// ===========================================================================

pub const VOICEPRINT_GRANT: u32 = 850;
pub const VOICEPRINT_STRIKE_MAX: u8 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VoiceGate {
    pub strikes: u8,
    pub locked: bool,
}

impl VoiceGate {
    pub const fn new() -> VoiceGate {
        VoiceGate { strikes: 0, locked: false }
    }

    pub fn attempt(&mut self, score_permille: u32) -> bool {
        if self.locked {
            return false;
        }
        if score_permille >= VOICEPRINT_GRANT {
            self.strikes = 0;
            true
        } else {
            self.strikes += 1;
            if self.strikes >= VOICEPRINT_STRIKE_MAX {
                self.locked = true;
            }
            false
        }
    }
}

// ===========================================================================
// F216 — 听障视觉化：响度 → 脉冲数 / 闪光时长
// ===========================================================================

pub const VISUAL_PULSE_MAX: u32 = 8;

pub fn visual_pulses(loudness_permille: u32) -> u32 {
    let l = if loudness_permille > 1000 { 1000 } else { loudness_permille };
    l * VISUAL_PULSE_MAX / 1000
}

pub fn visual_flash_ms(loudness_permille: u32) -> u32 {
    loudness_permille / 2
}

// ===========================================================================
// F217 — 静音仪式：独占流在唱不可静音；渐静按 100‰ 步进
// ===========================================================================

pub fn mute_ceremony_ok(exclusive_active: u32) -> bool {
    exclusive_active == 0
}

/// 渐静步数：每步降 100‰，向上取整。
pub fn fade_steps(volume_permille: u32) -> u32 {
    if volume_permille == 0 {
        0
    } else {
        (volume_permille + 99) / 100
    }
}

// ===========================================================================
// F218 — 音频故障解剖：错误码归类 + 恢复退避
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioFaultKind {
    Underrun,
    ClockDrift,
    DeviceGone,
}

pub fn fault_classify(code: u32) -> AudioFaultKind {
    match code {
        1 => AudioFaultKind::Underrun,
        2 => AudioFaultKind::ClockDrift,
        _ => AudioFaultKind::DeviceGone,
    }
}

/// 恢复退避：50ms 起翻倍，封顶 800ms。
pub fn fault_backoff_ms(attempt: u8) -> u32 {
    let mut ms = 50u32;
    let mut i = 0u8;
    while i < attempt && ms < 800 {
        ms *= 2;
        i += 1;
    }
    if ms > 800 {
        800
    } else {
        ms
    }
}

// ===========================================================================
// F219 — 声音诊断室：THD 低 + SNR 高才算体检通过
// ===========================================================================

pub const DIAG_THD_MAX: u32 = 50;
pub const DIAG_SNR_MIN: u32 = 800;

pub fn sound_diagnostic_pass(thd_permille: u32, snr_permille: u32) -> bool {
    thd_permille <= DIAG_THD_MAX && snr_permille >= DIAG_SNR_MIN
}

// ===========================================================================
// F220 — 环境声自适应：噪底越高，麦克风增益压得越低
// ===========================================================================

pub const AMBIENT_GAIN_FLOOR: u32 = 200;

/// gain = 1000 - noise*9/10，钳到下限 200。
pub fn ambient_mic_gain(noise_permille: u32) -> u32 {
    let n = if noise_permille > 1000 { 1000 } else { noise_permille };
    let g = 1000 - n * 9 / 10;
    if g < AMBIENT_GAIN_FLOOR {
        AMBIENT_GAIN_FLOOR
    } else {
        g
    }
}

// ===========================================================================
// F221 — 音频时间轴回放：时间戳严格递增 + 单步间隔上限
// ===========================================================================

pub const TIMELINE_GAP_MAX_MS: u64 = 100;

pub fn audio_timeline_ok(stamps: &[u64]) -> bool {
    let mut i = 1usize;
    while i < stamps.len() {
        if stamps[i] <= stamps[i - 1] {
            return false;
        }
        i += 1;
    }
    true
}

pub fn timeline_gap_ok(prev_ms: u64, next_ms: u64) -> bool {
    next_ms > prev_ms && next_ms - prev_ms <= TIMELINE_GAP_MAX_MS
}

// ===========================================================================
// F222 — 杜比兼容层：AC-3 同步字 0x0B77 直通探测
// ===========================================================================

pub const DOLBY_SYNCWORD: [u8; 2] = [0x0B, 0x77];

pub fn dolby_sync_ok(frame: &[u8]) -> bool {
    frame.len() >= 2 && frame[0] == DOLBY_SYNCWORD[0] && frame[1] == DOLBY_SYNCWORD[1]
}

// ===========================================================================
// F223 — 采样率诚实化：标称与实测偏差 ≤ 10‰（1%）
// ===========================================================================

pub const RATE_TOLERANCE_PERMILLE: u32 = 10;

pub fn rate_honest(claimed_hz: u32, actual_hz: u32) -> bool {
    if claimed_hz == 0 {
        return false;
    }
    let diff = if actual_hz > claimed_hz { actual_hz - claimed_hz } else { claimed_hz - actual_hz };
    diff * 1000 <= claimed_hz * RATE_TOLERANCE_PERMILLE
}

// ===========================================================================
// F224 — 声景回归套件：金样混音台剧本 → 固定末态
// ===========================================================================

/// 金样剧本：三路增益 300/300/200，总线 800 不过载。
pub fn sound_golden_mixer() -> MixConsole {
    let mut m = MixConsole::new();
    m.set_gain(0, 300);
    m.set_gain(1, 300);
    m.set_gain(2, 200);
    m
}

/// 期望末态：三路 300/300/200，总和 800，不过载。
pub fn sound_golden_matches(m: &MixConsole) -> bool {
    m.gains[0] == 300 && m.gains[1] == 300 && m.gains[2] == 200 && m.bus_sum() == 800 && !m.bus_overload()
}

// ===========================================================================
// F225 — 声学年报：章节完备性
// ===========================================================================

pub const M600_AUDIO_REPORT_SECTIONS: [&str; 5] =
    ["soundscape", "spatial", "latency", "privacy", "regression"];

pub fn m600_audio_report_complete(filled: u32) -> bool {
    filled >= M600_AUDIO_REPORT_SECTIONS.len() as u32
}

// ===========================================================================
// 域自检
// ===========================================================================

pub fn run_m600audio_checks() -> CheckSet {
    let mut set = CheckSet::new("m600audio");

    // F201 系统声景总谱
    let mut ledger = SoundLedger::new();
    let mut i = 0;
    while i < 8 {
        ledger.add_stream(i < 3);
        i += 1;
    }
    let muted_before = ledger.muted_permille();
    ledger.mark_muted(2);
    set.add(
        "F201 soundscape ledger",
        ledger.consistent() && ledger.total == 8 && ledger.muted == 5 && ledger.active == 3,
        "identity holds",
    );
    set.add(
        "F201 soundscape ratio",
        muted_before == 375 && ledger.muted_permille() == 625,
        "3/8 then 5/8 permille",
    );
    set.add("F201 soundscape overdraft", !ledger.mark_muted(9) && ledger.active == 3, "no over-mute");

    // F202 空间音频引擎
    set.add(
        "F202 spatial pan",
        pan_right_permille(0) == 0 && pan_right_permille(90) == 500 && pan_right_permille(180) == 1000
            && pan_right_permille(270) == 500,
        "azimuth to pan",
    );
    set.add(
        "F202 spatial elevation",
        elevation_clamped(120) == 90 && elevation_clamped(-120) == -90 && elevation_clamped(45) == 45,
        "clamped to +/-90",
    );

    // F203 音频低延迟直通
    set.add(
        "F203 passthrough gate",
        latency_passthrough_ok(3000, 2) && !latency_passthrough_ok(6000, 1)
            && !latency_passthrough_ok(3000, 3),
        "buffer + blocks double gate",
    );

    // F204 声卡能力档案
    let profile = SoundCardProfile { name_hash: 9, max_channels: 8, rates_mask: 0b01100 };
    set.add(
        "F204 soundcard rates",
        profile.supports_rate(44100) && profile.supports_rate(48000)
            && !profile.supports_rate(8000) && !profile.supports_rate(96000)
            && !profile.supports_rate(12345),
        "mask bit per rate",
    );
    set.add(
        "F204 soundcard profile",
        soundcard_profile_ok(profile)
            && !soundcard_profile_ok(SoundCardProfile { name_hash: 0, max_channels: 8, rates_mask: 1 })
            && !soundcard_profile_ok(SoundCardProfile { name_hash: 9, max_channels: 0, rates_mask: 1 }),
        "hash + channels sane",
    );

    // F205 混音器策展
    let mut console = MixConsole::new();
    console.set_gain(0, 400);
    console.set_gain(1, 300);
    console.set_gain(2, 500);
    let bus = console.bus_sum();
    let hot = console.bus_overload();
    set.add("F205 mixer overload", bus == 1200 && hot, "sum over full gain");
    let mut clamp = MixConsole::new();
    clamp.set_gain(0, 1500);
    let clamped = clamp.gains[0];
    set.add("F205 mixer clamp", clamped == 1000, "gain pinned at 1000");
    set.add("F205 mix two", mix_two(800, 500, 600, 500) == 700, "(800*500+600*500)/1000");

    // F206 声音主题工坊
    let mut shelf = SoundThemeShelf::new();
    let t1 = shelf.register(1);
    let t2 = shelf.register(1);
    let t3 = shelf.register(2);
    let tcount = shelf.count;
    set.add("F206 theme shelf dedup", t1 && !t2 && t3 && tcount == 2, "same id rejected");
    let mut tfull = SoundThemeShelf::new();
    let mut tid = 10u16;
    let mut tgot = 0usize;
    while tid < 18 {
        if tfull.register(tid) {
            tgot += 1;
        }
        tid += 1;
    }
    let toverflow = tfull.register(99);
    set.add("F206 theme shelf capacity", tgot == 8 && !toverflow, "8 themes cap");

    // F207 按键声学
    set.add(
        "F207 key acoustics",
        key_click_loudness(KeyAcoustic::Alpha) == 300 && key_click_loudness(KeyAcoustic::Space) == 500
            && key_click_loudness(KeyAcoustic::Enter) == 700,
        "alpha < space < enter",
    );

    // F208 UI 微声效库
    let mut sfx = UiSfxShelf::new();
    let u1 = sfx.register(7);
    let u2 = sfx.register(7);
    let ucount = sfx.count;
    set.add("F208 ui sfx dedup", u1 && !u2 && ucount == 1, "same sfx rejected");
    set.add(
        "F208 ui sfx length",
        sfx_length_ok(150) && !sfx_length_ok(250) && !sfx_length_ok(0),
        "1..=200ms only",
    );

    // F209 通知声音礼仪
    set.add(
        "F209 notify etiquette",
        !notify_sound_allowed(23, 500) && notify_sound_allowed(23, 950)
            && notify_sound_allowed(12, 500) && !notify_sound_allowed(6, 800)
            && notify_sound_allowed(7, 500),
        "quiet hours + priority",
    );

    // F210 音量曲线大师
    set.add(
        "F210 volume curve",
        perceived_volume(500) == 250 && perceived_volume(1000) == 1000 && perceived_volume(100) == 10,
        "square law",
    );
    set.add("F210 volume monotonic", perceived_volume(800) > perceived_volume(400), "louder stays louder");

    // F211 独占接管让位
    let mut seat = ExclusiveSeat::new();
    let d1 = seat.claim(1);
    let d2 = seat.claim(2);
    let handoffs = seat.handoffs;
    let d3 = seat.claim(2);
    let handoffs_after = seat.handoffs;
    set.add(
        "F211 exclusive seat",
        d1.is_none() && d2 == Some(1) && handoffs == 1 && d3.is_none() && handoffs_after == 1,
        "displace once, self-reclaim free",
    );

    // F212 蓝牙音频调优
    set.add(
        "F212 bt bitrate",
        bt_bitrate_kbps(44100, 16, 2, 250) == 352 && bt_fits(352),
        "sbc quarter ratio fits",
    );
    set.add(
        "F212 bt budget",
        bt_bitrate_kbps(44100, 16, 2, 400) == 564 && !bt_fits(564) && bt_fits(512),
        "over 512 kbps rejected",
    );

    // F213 麦克风阵列处理
    set.add(
        "F213 mic delay",
        mic_delay_us(40) == 116 && mic_delay_us(343) == 1000 && mic_delay_us(0) == 0,
        "343 m/s",
    );
    let mut array = MicArrayBook::new();
    let mut attached = 0usize;
    let mut probe = 0;
    while probe < 10 {
        if array.attach() {
            attached += 1;
        }
        probe += 1;
    }
    set.add("F213 mic array cap", attached == 8 && array.attached == 8, "8 elements max");

    // F214 回声消除舱
    set.add(
        "F214 aec step",
        aec_step(1000) == 800 && aec_step(500) == 400 && aec_step(0) == 0,
        "80% per frame",
    );
    set.add(
        "F214 aec converge",
        aec_converge_steps(1000) == 11 && aec_converge_steps(50) == 0,
        "11 frames to converge",
    );

    // F215 声纹识别门卫
    let mut gate = VoiceGate::new();
    let g1 = gate.attempt(900);
    let g2 = gate.attempt(700);
    let strikes_mid = gate.strikes;
    let g3 = gate.attempt(700);
    let g4 = gate.attempt(700);
    let locked = gate.locked;
    let g5 = gate.attempt(990);
    set.add(
        "F215 voiceprint gate",
        g1 && !g2 && strikes_mid == 1 && !g3 && !g4 && locked && !g5,
        "3 strikes lock the door",
    );

    // F216 听障视觉化
    set.add(
        "F216 visual pulses",
        visual_pulses(1000) == 8 && visual_pulses(500) == 4 && visual_pulses(0) == 0
            && visual_pulses(999) == 7,
        "loudness to pulses",
    );
    set.add("F216 visual flash", visual_flash_ms(800) == 400, "half of loudness");

    // F217 静音仪式
    set.add(
        "F217 mute ceremony",
        mute_ceremony_ok(0) && !mute_ceremony_ok(1),
        "exclusive stream blocks mute",
    );
    set.add(
        "F217 fade steps",
        fade_steps(950) == 10 && fade_steps(1) == 1 && fade_steps(0) == 0,
        "ceil to 100‰ steps",
    );

    // F218 音频故障解剖
    set.add(
        "F218 fault classify",
        fault_classify(1) == AudioFaultKind::Underrun && fault_classify(2) == AudioFaultKind::ClockDrift
            && fault_classify(99) == AudioFaultKind::DeviceGone,
        "code to kind",
    );
    set.add(
        "F218 fault backoff",
        fault_backoff_ms(0) == 50 && fault_backoff_ms(3) == 400 && fault_backoff_ms(9) == 800,
        "doubling capped at 800",
    );

    // F219 声音诊断室
    set.add(
        "F219 diagnostic",
        sound_diagnostic_pass(30, 900) && !sound_diagnostic_pass(60, 900)
            && !sound_diagnostic_pass(30, 700),
        "low thd + high snr",
    );

    // F220 环境声自适应
    set.add(
        "F220 ambient gain",
        ambient_mic_gain(0) == 1000 && ambient_mic_gain(400) == 640 && ambient_mic_gain(1000) == 200,
        "noise squeezes gain",
    );

    // F221 音频时间轴回放
    let good = [10u64, 20, 30];
    let bad = [10u64, 10, 20];
    set.add(
        "F221 timeline order",
        audio_timeline_ok(&good) && !audio_timeline_ok(&bad),
        "strictly increasing",
    );
    set.add(
        "F221 timeline gap",
        timeline_gap_ok(10, 110) && !timeline_gap_ok(10, 111) && !timeline_gap_ok(10, 10),
        "gap <= 100ms",
    );

    // F222 杜比兼容层
    set.add(
        "F222 dolby sync",
        dolby_sync_ok(&[0x0B, 0x77, 0x01]) && !dolby_sync_ok(&[0x0B, 0x78]) && !dolby_sync_ok(&[0x0B]),
        "0x0B77 syncword",
    );

    // F223 采样率诚实化
    set.add(
        "F223 rate honest",
        rate_honest(48000, 48000) && rate_honest(48000, 48047) && !rate_honest(48000, 47500)
            && !rate_honest(0, 100),
        "1% tolerance",
    );

    // F224 声景回归套件
    let golden = sound_golden_mixer();
    set.add("F224 golden mixer", sound_golden_matches(&golden), "scripted end state");
    let mut drifted = sound_golden_mixer();
    drifted.set_gain(2, 500);
    let drift_sum = drifted.bus_sum();
    set.add(
        "F224 golden catches drift",
        !sound_golden_matches(&drifted) && drift_sum == 1100 && drifted.bus_overload(),
        "mutation detected",
    );

    // F225 声学年报
    set.add(
        "F225 audio report",
        M600_AUDIO_REPORT_SECTIONS.len() == 5 && m600_audio_report_complete(5)
            && !m600_audio_report_complete(4),
        "sections complete",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f201_ledger_math() {
        let mut l = SoundLedger::new();
        assert!(l.add_stream(true));
        assert!(l.add_stream(false));
        assert!(l.add_stream(false));
        assert!(l.consistent());
        assert_eq!(l.muted_permille(), 333);
        assert!(l.mark_muted(1));
        assert_eq!(l.muted, 2);
    }

    #[test]
    fn f202_pan_symmetry() {
        assert_eq!(pan_right_permille(360), 0); // 环回
        assert_eq!(pan_right_permille(450), 500); // 450 % 360 = 90
        assert_eq!(pan_right_permille(180), 1000);
    }

    #[test]
    fn f214_aec_boundaries() {
        assert_eq!(aec_converge_steps(100), 0); // 恰在阈值不算需收敛
        assert_eq!(aec_converge_steps(101), 1); // 101*800/1000=80 <= 100
        assert_eq!(aec_step(125), 100);
    }

    #[test]
    fn f211_seat_semantics() {
        let mut seat = ExclusiveSeat::new();
        assert_eq!(seat.claim(5), None);
        assert_eq!(seat.claim(6), Some(5));
        assert_eq!(seat.claim(6), None); // 重入座不让位
        assert_eq!(seat.handoffs, 1);
        assert_eq!(seat.current, Some(6));
    }

    #[test]
    fn f223_rate_honesty_bounds() {
        // 48000 的 1% = 480，偏差 479 诚实、481 撒谎
        assert!(rate_honest(48000, 48479));
        assert!(!rate_honest(48000, 48481));
    }

    #[test]
    fn m600audio_selfcheck_all_pass() {
        let set = run_m600audio_checks();
        assert!(set.len() >= 25, "got {}", set.len());
        assert!(set.all_passed());
    }
}
