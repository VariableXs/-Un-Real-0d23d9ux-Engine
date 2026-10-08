//! VARIX-M500 AI-08 · 音声设计（F176~F200）。
//!
//! 音色令牌、场景音景、混音总线、声像距离、可懂度、听障增强、事件
//! 转写、回声预留、降噪档、路由 DSL、虚拟声卡、指纹、试听、回归基
//! 线、分区播放、响度守护、漂移校正、延迟热图、静默档、麦克风指
//! 示、崩溃隔离、声景编辑器、fuzz、性能预算与域自检。
//! 纪律：纯逻辑 + 固定容量数组；无 `Vec`/`String`/`Box`/`alloc`；
//! 全部定点运算（Q15），内核中断路径无 FPU。

use crate::checks::CheckSet;

/// Q15：1.0 = 32768。
pub const Q15: u32 = 32768;

// ---------------------------------------------------------------------------
// F176 音色设计系统 — 声音令牌体系（统一音色参数）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ToneToken {
    /// 基频 Hz。
    pub freq_hz: u16,
    /// 时长 ms。
    pub dur_ms: u16,
    /// Q15 音量。
    pub gain_q15: u32,
    /// 泛音层数 0..4。
    pub harmonics: u8,
}

pub const TONE_NONE: ToneToken = ToneToken { freq_hz: 0, dur_ms: 0, gain_q15: 0, harmonics: 0 };
pub const TONE_NOTIFY: ToneToken =
    ToneToken { freq_hz: 880, dur_ms: 120, gain_q15: Q15 * 3 / 4, harmonics: 2 };
pub const TONE_ERROR: ToneToken =
    ToneToken { freq_hz: 220, dur_ms: 300, gain_q15: Q15, harmonics: 3 };

/// 令牌降档：受限时按比例缩小音量与时长。
pub fn tone_derate(t: ToneToken, budget_permille: u16) -> ToneToken {
    ToneToken {
        freq_hz: t.freq_hz,
        dur_ms: (t.dur_ms as u32 * budget_permille as u32 / 1000).min(0xFFFF) as u16,
        gain_q15: t.gain_q15 * budget_permille as u32 / 1000,
        harmonics: if budget_permille < 500 { t.harmonics.min(1) } else { t.harmonics },
    }
}

// ---------------------------------------------------------------------------
// F177 场景音景 — 专注/夜间声景模式
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Soundscape {
    Normal,
    Focus,
    Night,
}

#[derive(Clone, Copy)]
pub struct SoundscapePolicy {
    /// 系统 UI 音量 Q15 上限。
    pub ui_gain_cap_q15: u32,
    /// 通知是否静音。
    pub mute_notify: bool,
    /// 环境垫音（0=无）。
    pub ambience: u8,
}

pub fn soundscape_policy(s: Soundscape) -> SoundscapePolicy {
    match s {
        Soundscape::Normal => SoundscapePolicy { ui_gain_cap_q15: Q15, mute_notify: false, ambience: 0 },
        Soundscape::Focus => SoundscapePolicy { ui_gain_cap_q15: Q15 / 2, mute_notify: true, ambience: 32 },
        Soundscape::Night => SoundscapePolicy { ui_gain_cap_q15: Q15 / 4, mute_notify: true, ambience: 16 },
    }
}

// ---------------------------------------------------------------------------
// F178 动态混音总线 — 场景自动压限（简单软限幅）
// ---------------------------------------------------------------------------

/// 软限幅：|x| 超过阈值后按 Q15 曲线压缩。
pub fn soft_limit(x: i32, threshold_q15: u32) -> i32 {
    let abs = x.unsigned_abs();
    if abs <= threshold_q15 {
        return x;
    }
    let over = abs - threshold_q15;
    let compressed = threshold_q15 + over / 4; // 4:1
    if x < 0 {
        -(compressed.min(Q15) as i32)
    } else {
        compressed.min(Q15) as i32
    }
}

#[derive(Clone, Copy)]
pub struct MixBus {
    /// 通道增益 Q15。
    pub gains: [u32; 4],
    pub threshold_q15: u32,
    pub clipped: u32,
}

impl MixBus {
    pub const fn new() -> MixBus {
        MixBus { gains: [Q15; 4], threshold_q15: Q15 * 3 / 4, clipped: 0 }
    }

    pub fn set_gain(&mut self, ch: usize, g: u32) -> bool {
        if ch >= 4 {
            return false;
        }
        self.gains[ch] = g.min(Q15 * 2);
        true
    }

    /// 混合 4 通道样本并压限。
    pub fn mix(&mut self, s: [i32; 4]) -> i32 {
        let mut acc = 0i64;
        for ch in 0..4 {
            acc += (s[ch] as i64 * self.gains[ch] as i64) / Q15 as i64;
        }
        let out = soft_limit(acc as i32, self.threshold_q15);
        if out.unsigned_abs() >= self.threshold_q15 {
            self.clipped += 1;
        }
        out
    }
}

// ---------------------------------------------------------------------------
// F179 声音距离模型 — 通知远近声像（衰减 + 左右平衡）
// ---------------------------------------------------------------------------

/// distance 0..255（0=耳边）；返回 (gain_q15, pan -128..128)。
pub fn distance_model(distance: u8, pan_hint: i8) -> (u32, i16) {
    let gain = Q15 * (255 - distance as u32) / 255;
    let pan = pan_hint as i16 * (distance as i16) / 128;
    (gain, pan)
}

// ---------------------------------------------------------------------------
// F180 语音可懂度优化 — 对话增强（高频提升 + 降噪底）
// ---------------------------------------------------------------------------

/// 简单一阶高通（Q15 系数），提升语音清晰度。
pub fn speech_enhance(x: i32, prev: &mut i32, alpha_q15: u32) -> i32 {
    let out = ((x as i64 - *prev as i64) * alpha_q15 as i64 / Q15 as i64) as i32;
    *prev = x;
    out
}

// ---------------------------------------------------------------------------
// F181 听障增强 — 声源视觉指示
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SoundSource {
    Notify,
    Alarm,
    Voice,
    Media,
}

/// 声源→视觉指示编号（0=无，1..4）。
pub fn visual_indicator(src: SoundSource, audible_q15: u32) -> u8 {
    if audible_q15 < Q15 / 16 {
        return 0;
    }
    match src {
        SoundSource::Notify => 1,
        SoundSource::Alarm => 2,
        SoundSource::Voice => 3,
        SoundSource::Media => 4,
    }
}

// ---------------------------------------------------------------------------
// F182 声音事件转写 — 音频日志字幕化
// ---------------------------------------------------------------------------

pub const EVENTLOG_CAP: usize = 8;

#[derive(Clone, Copy)]
pub struct SoundEventEntry {
    pub t_ms: u32,
    pub src: SoundSource,
    pub level_q15: u32,
}

#[derive(Clone, Copy)]
pub struct SoundEventLog {
    pub entries: [SoundEventEntry; EVENTLOG_CAP],
    pub head: usize,
    pub count: usize,
}

impl SoundEventLog {
    pub const fn new() -> SoundEventLog {
        SoundEventLog {
            entries: [SoundEventEntry { t_ms: 0, src: SoundSource::Notify, level_q15: 0 }; EVENTLOG_CAP],
            head: 0,
            count: 0,
        }
    }

    pub fn append(&mut self, t_ms: u32, src: SoundSource, level_q15: u32) {
        self.entries[self.head] = SoundEventEntry { t_ms, src, level_q15 };
        self.head = (self.head + 1) % EVENTLOG_CAP;
        if self.count < EVENTLOG_CAP {
            self.count += 1;
        }
    }

    /// 生成简短字幕行（槽位字符编码：'N'/'A'/'V'/'M'）。
    pub fn caption(&self, window_ms: u32) -> [u8; EVENTLOG_CAP] {
        let mut out = [0u8; EVENTLOG_CAP];
        let mut n = 0;
        for k in 0..EVENTLOG_CAP {
            let i = (self.head + EVENTLOG_CAP - 1 - k) % EVENTLOG_CAP;
            if k >= self.count {
                break;
            }
            let e = &self.entries[i];
            if e.t_ms >= window_ms {
                out[n] = match e.src {
                    SoundSource::Notify => b'N',
                    SoundSource::Alarm => b'A',
                    SoundSource::Voice => b'V',
                    SoundSource::Media => b'M',
                };
                n += 1;
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------
// F183 回声消除预留 — 通信链路占位（自适应系数占位）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct EchoCancel {
    /// 自适应滤波系数（Q15，占位训练）。
    pub coeff_q15: u32,
    pub converged: bool,
}

impl EchoCancel {
    pub const fn new() -> EchoCancel {
        EchoCancel { coeff_q15: 0, converged: false }
    }

    /// 用远端参考 × 系数预估回声并抵消；简化 LMS 一步。
    pub fn process(&mut self, far: i32, near: i32) -> i32 {
        let est = (far as i64 * self.coeff_q15 as i64 / Q15 as i64) as i32;
        let err = near - est;
        // LMS 更新：coeff += mu * err * far（避免溢出用 >>10 步长）。
        let delta = ((err as i64 * far as i64) >> 12) as i32;
        self.coeff_q15 = (self.coeff_q15 as i64 + delta as i64).clamp(0, Q15 as i64) as u32;
        if err.unsigned_abs() < Q15 / 64 {
            self.converged = true;
        }
        err
    }
}

// ---------------------------------------------------------------------------
// F184 降噪档位 — 可调强度（谱减占位：减去估计底噪）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum NrLevel {
    Off,
    Light,
    Medium,
    Strong,
}

pub fn nr_strength(l: NrLevel) -> u32 {
    match l {
        NrLevel::Off => 0,
        NrLevel::Light => Q15 / 8,
        NrLevel::Medium => Q15 / 4,
        NrLevel::Strong => Q15 / 2,
    }
}

/// 谱减：out = max(0, x - strength)。
pub fn denoise(x: i32, l: NrLevel) -> i32 {
    let s = nr_strength(l) as i64;
    if (x as i64).abs() <= s {
        0
    } else if x > 0 {
        (x as i64 - s) as i32
    } else {
        (x as i64 + s) as i32
    }
}

// ---------------------------------------------------------------------------
// F185 音频路由 DSL — 声音流向声明（规则表求值）
// ---------------------------------------------------------------------------

pub const ROUTE_CAP: usize = 8;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct RouteRule {
    /// 源 app id。
    pub src_app: u16,
    /// 目标设备 id。
    pub dst_dev: u16,
    pub enabled: bool,
}

#[derive(Clone, Copy)]
pub struct RouteTable {
    pub rules: [RouteRule; ROUTE_CAP],
    pub count: usize,
    pub default_dev: u16,
}

impl RouteTable {
    pub const fn new(default_dev: u16) -> RouteTable {
        RouteTable { rules: [RouteRule { src_app: 0, dst_dev: 0, enabled: false }; ROUTE_CAP], count: 0, default_dev }
    }

    pub fn add(&mut self, src_app: u16, dst_dev: u16) -> bool {
        for i in 0..self.count {
            if self.rules[i].src_app == src_app {
                self.rules[i].dst_dev = dst_dev;
                self.rules[i].enabled = true;
                return true;
            }
        }
        if self.count >= ROUTE_CAP {
            return false;
        }
        self.rules[self.count] = RouteRule { src_app, dst_dev, enabled: true };
        self.count += 1;
        true
    }

    /// 求值：最后一条启用的匹配规则胜出，否则默认设备。
    pub fn resolve(&self, src_app: u16) -> u16 {
        for i in (0..self.count).rev() {
            if self.rules[i].enabled && self.rules[i].src_app == src_app {
                return self.rules[i].dst_dev;
            }
        }
        self.default_dev
    }

    pub fn disable(&mut self, src_app: u16) -> bool {
        match (0..self.count).find(|&i| self.rules[i].src_app == src_app) {
            Some(i) => {
                self.rules[i].enabled = false;
                true
            }
            None => false,
        }
    }
}

// ---------------------------------------------------------------------------
// F186 虚拟声卡 — 应用间音频管道
// ---------------------------------------------------------------------------

pub const VPIPE_CAP: usize = 64;

#[derive(Clone, Copy)]
pub struct VirtualPipe {
    pub buf: [i16; VPIPE_CAP],
    pub w: usize,
    pub r: usize,
    pub open: bool,
}

impl VirtualPipe {
    pub const fn new() -> VirtualPipe {
        VirtualPipe { buf: [0; VPIPE_CAP], w: 0, r: 0, open: false }
    }

    pub fn open(&mut self) {
        self.open = true;
        self.w = 0;
        self.r = 0;
    }

    pub fn close(&mut self) {
        self.open = false;
    }

    pub fn write(&mut self, s: i16) -> bool {
        if !self.open || (self.w + 1) % VPIPE_CAP == self.r {
            return false;
        }
        self.buf[self.w] = s;
        self.w = (self.w + 1) % VPIPE_CAP;
        true
    }

    pub fn read(&mut self) -> Option<i16> {
        if !self.open || self.r == self.w {
            return None;
        }
        let s = self.buf[self.r];
        self.r = (self.r + 1) % VPIPE_CAP;
        Some(s)
    }
}

// ---------------------------------------------------------------------------
// F187 音频指纹 — 素材自动标记（能量包络签名）
// ---------------------------------------------------------------------------

pub const FP_BINS: usize = 8;

/// 由 PCM 片段计算 8 桶能量包络作为指纹。
pub fn fingerprint(pcm: &[i16], bins: usize) -> [u16; FP_BINS] {
    let mut out = [0u16; FP_BINS];
    let bins = bins.min(FP_BINS);
    if pcm.is_empty() {
        return out;
    }
    let per = pcm.len() / bins.max(1);
    if per == 0 {
        return out;
    }
    for b in 0..bins {
        let mut acc = 0u64;
        for k in 0..per {
            acc += pcm[b * per + k].unsigned_abs() as u64;
        }
        out[b] = (acc / per as u64).min(0xFFFF) as u16;
    }
    out
}

/// 指纹相似度：逐桶差 ≤ 8% 记命中。
pub fn fp_match(a: &[u16; FP_BINS], b: &[u16; FP_BINS]) -> bool {
    for i in 0..FP_BINS {
        let d = (a[i] as i32 - b[i] as i32).unsigned_abs();
        let base = a[i].max(b[i]) as u32;
        if base > 0 && d as u32 * 100 > base * 8 {
            return false;
        }
    }
    true
}

// ---------------------------------------------------------------------------
// F188 提示音试听器 — A/B 试听（交替播放序）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct Audition {
    pub tokens: [ToneToken; 2],
    pub round: u32,
}

impl Audition {
    pub const fn new(a: ToneToken, b: ToneToken) -> Audition {
        Audition { tokens: [a, b], round: 0 }
    }

    /// 返回本轮播放哪个（0=A,1=B），交替进行。
    pub fn next(&mut self) -> usize {
        let which = (self.round % 2) as usize;
        self.round += 1;
        which
    }

    /// 用户投票后定型。
    pub fn pick(&self, winner: usize) -> ToneToken {
        self.tokens[winner.min(1)]
    }
}

// ---------------------------------------------------------------------------
// F189 声音回归基线 — 延迟/质量基线比对
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct AudioBaseline {
    pub latency_ms: u16,
    pub thd_permille: u16,
}

/// 回归判定：延迟劣化 >10% 或 THD 劣化 >15% 视为回归。
pub fn audio_regress(base: AudioBaseline, now: AudioBaseline) -> bool {
    let lat_bad = now.latency_ms as u32 * 100 > base.latency_ms as u32 * 110;
    let thd_bad = now.thd_permille as u32 * 100 > base.thd_permille as u32 * 115;
    lat_bad || thd_bad
}

// ---------------------------------------------------------------------------
// F190 分区播放 — 多 room 输出
// ---------------------------------------------------------------------------

pub const ROOM_CAP: usize = 4;

#[derive(Clone, Copy)]
pub struct RoomOut {
    pub room_id: u16,
    pub enabled: bool,
    pub gain_q15: u32,
}

#[derive(Clone, Copy)]
pub struct RoomMatrix {
    pub rooms: [RoomOut; ROOM_CAP],
    pub count: usize,
}

impl RoomMatrix {
    pub const fn new() -> RoomMatrix {
        RoomMatrix { rooms: [RoomOut { room_id: 0, enabled: false, gain_q15: 0 }; ROOM_CAP], count: 0 }
    }

    pub fn add(&mut self, room_id: u16, gain_q15: u32) -> bool {
        if self.count >= ROOM_CAP || (0..self.count).any(|i| self.rooms[i].room_id == room_id) {
            return false;
        }
        self.rooms[self.count] = RoomOut { room_id, enabled: true, gain_q15: gain_q15.min(Q15) };
        self.count += 1;
        true
    }

    pub fn set_enabled(&mut self, room_id: u16, on: bool) -> bool {
        match (0..self.count).find(|&i| self.rooms[i].room_id == room_id) {
            Some(i) => {
                self.rooms[i].enabled = on;
                true
            }
            None => false,
        }
    }

    /// 样本向启用房间广播，返回每房间的实际输出电平。
    pub fn broadcast(&self, s: i32) -> [i32; ROOM_CAP] {
        let mut out = [0i32; ROOM_CAP];
        for i in 0..self.count {
            if self.rooms[i].enabled {
                out[i] = (s as i64 * self.rooms[i].gain_q15 as i64 / Q15 as i64) as i32;
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------
// F191 响度守护 — 突发音量限制
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct LoudnessGuard {
    pub ceiling_q15: u32,
    pub trips: u32,
}

impl LoudnessGuard {
    pub const fn new(ceiling_q15: u32) -> LoudnessGuard {
        LoudnessGuard { ceiling_q15, trips: 0 }
    }

    /// 超限则压回天花板并计数。
    pub fn enforce(&mut self, gain_q15: u32) -> u32 {
        if gain_q15 > self.ceiling_q15 {
            self.trips += 1;
            self.ceiling_q15
        } else {
            gain_q15
        }
    }
}

// ---------------------------------------------------------------------------
// F192 时钟漂移校正 — 多设备采样对齐（Bresenham 式重采样）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct DriftCorrector {
    /// 本地时钟相对主时钟的百万分比偏差。
    pub drift_ppm: i32,
    acc: i32,
    pub inserted: u32,
    pub dropped: u32,
}

impl DriftCorrector {
    pub const fn new() -> DriftCorrector {
        DriftCorrector { drift_ppm: 0, acc: 0, inserted: 0, dropped: 0 }
    }

    pub fn set_drift(&mut self, ppm: i32) {
        self.drift_ppm = ppm.clamp(-1_000_000, 1_000_000);
    }

    /// 每个输出样本 tick 一次；偏差累积到阈值时指示插入/丢弃。
    pub fn tick(&mut self) -> u8 {
        self.acc += self.drift_ppm;
        if self.acc >= 1_000_000 {
            self.acc -= 1_000_000;
            self.dropped += 1; // 本地快 → 丢一个样本
            2
        } else if self.acc <= -1_000_000 {
            self.acc += 1_000_000;
            self.inserted += 1; // 本地慢 → 补一个样本
            1
        } else {
            0
        }
    }
}

// ---------------------------------------------------------------------------
// F193 音频延迟热图 — 全链路延迟（采集/处理/输出三段）
// ---------------------------------------------------------------------------

pub const LAT_STAGE_CAP: usize = 4;

#[derive(Clone, Copy)]
pub struct LatencyMap {
    pub stages: [u32; LAT_STAGE_CAP],
    pub names: [&'static str; LAT_STAGE_CAP],
    pub count: usize,
}

impl LatencyMap {
    pub const fn new() -> LatencyMap {
        LatencyMap { stages: [0; LAT_STAGE_CAP], names: ["", "", "", ""], count: 0 }
    }

    pub fn add_stage(&mut self, name: &'static str, us: u32) -> bool {
        if self.count >= LAT_STAGE_CAP {
            return false;
        }
        self.stages[self.count] = us;
        self.names[self.count] = name;
        self.count += 1;
        true
    }

    pub fn total_us(&self) -> u32 {
        (0..self.count).map(|i| self.stages[i]).sum()
    }

    /// 热点：占比最大的阶段。
    pub fn hotspot(&self) -> Option<usize> {
        if self.count == 0 {
            return None;
        }
        Some((0..self.count).reduce(|a, b| if self.stages[b] > self.stages[a] { b } else { a }).unwrap())
    }
}

// ---------------------------------------------------------------------------
// F194 全系统静默档 — 无声模式
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct SilentMode {
    pub on: bool,
    /// 例外（闹钟/火警仍可响）。
    pub allow_critical: bool,
}

impl SilentMode {
    pub const fn new() -> SilentMode {
        SilentMode { on: false, allow_critical: true }
    }

    /// 输出裁决：静默时非关键流归零。
    pub fn gate(&self, gain_q15: u32, critical: bool) -> u32 {
        if self.on && !(self.allow_critical && critical) {
            0
        } else {
            gain_q15
        }
    }
}

// ---------------------------------------------------------------------------
// F195 麦克风使用指示 — 隐私可见
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct MicIndicator {
    pub in_use: bool,
    pub user_app: u16,
    pub use_count: u32,
}

impl MicIndicator {
    pub const fn new() -> MicIndicator {
        MicIndicator { in_use: false, user_app: 0, use_count: 0 }
    }

    pub fn begin(&mut self, app: u16) -> bool {
        if self.in_use {
            return false;
        }
        self.in_use = true;
        self.user_app = app;
        self.use_count += 1;
        true
    }

    pub fn end(&mut self, app: u16) -> bool {
        if self.in_use && self.user_app == app {
            self.in_use = false;
            self.user_app = 0;
            true
        } else {
            false
        }
    }
}

// ---------------------------------------------------------------------------
// F196 音频崩溃隔离 — 声音故障不扩散（熔断重启）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct AudioFuse {
    pub threshold: u8,
    pub fails: u8,
    pub tripped: bool,
    pub restarts: u32,
}

impl AudioFuse {
    pub const fn new(threshold: u8) -> AudioFuse {
        AudioFuse { threshold, fails: 0, tripped: false, restarts: 0 }
    }

    pub fn record_fail(&mut self) {
        if self.tripped {
            return;
        }
        self.fails += 1;
        if self.fails >= self.threshold {
            self.tripped = true;
        }
    }

    /// 熔断后重启：计数并复位。
    pub fn restart(&mut self) -> bool {
        if !self.tripped {
            return false;
        }
        self.tripped = false;
        self.fails = 0;
        self.restarts += 1;
        true
    }
}

// ---------------------------------------------------------------------------
// F197 声景编辑器 — 混音台 UI（轨道状态）
// ---------------------------------------------------------------------------

pub const TRACK_CAP: usize = 4;

#[derive(Clone, Copy)]
pub struct Track {
    pub enabled: bool,
    pub gain_q15: u32,
    pub pan: i16,
}

#[derive(Clone, Copy)]
pub struct SoundscapeMixer {
    pub tracks: [Track; TRACK_CAP],
}

impl SoundscapeMixer {
    pub const fn new() -> SoundscapeMixer {
        SoundscapeMixer {
            tracks: [Track { enabled: false, gain_q15: Q15, pan: 0 }; TRACK_CAP],
        }
    }

    pub fn toggle(&mut self, i: usize) -> bool {
        if i >= TRACK_CAP {
            return false;
        }
        self.tracks[i].enabled = !self.tracks[i].enabled;
        self.tracks[i].enabled
    }

    pub fn set(&mut self, i: usize, gain_q15: u32, pan: i16) -> bool {
        if i >= TRACK_CAP {
            return false;
        }
        self.tracks[i].gain_q15 = gain_q15.min(Q15);
        self.tracks[i].pan = pan.clamp(-128, 128);
        true
    }

    /// 轨道样本合成（含声像衰减）。
    pub fn render(&self, s: [i32; TRACK_CAP]) -> (i32, i32) {
        let mut l = 0i64;
        let mut r = 0i64;
        for i in 0..TRACK_CAP {
            if !self.tracks[i].enabled {
                continue;
            }
            let g = self.tracks[i].gain_q15 as i64;
            let p = self.tracks[i].pan as i64; // -128..128
            let gl = g * (128 - p.max(0)) / 128;
            let gr = g * (128 + p.min(0)) / 128;
            l += s[i] as i64 * gl / Q15 as i64;
            r += s[i] as i64 * gr / Q15 as i64;
        }
        (l as i32, r as i32)
    }
}

// ---------------------------------------------------------------------------
// F198 音频 fuzz — 音频路径对抗（确定性 LCG）
// ---------------------------------------------------------------------------

pub struct AudioFuzzer {
    pub state: u32,
    pub clamped: u32,
    pub passed: u32,
}

impl AudioFuzzer {
    pub const fn new(seed: u32) -> AudioFuzzer {
        AudioFuzzer { state: seed | 1, clamped: 0, passed: 0 }
    }

    pub fn next(&mut self) -> u32 {
        self.state = self.state.wrapping_mul(1664525).wrapping_add(1013904223);
        self.state
    }

    /// 生成对抗样本（全幅/翻转/随机），路径必须不 panic 且限幅。
    pub fn gen_sample(&mut self) -> i32 {
        let r = self.next();
        match r & 3 {
            0 => i32::MAX / 2,
            1 => i32::MIN / 2,
            2 => -((r >> 8) as i32 % 70000),
            _ => (r >> 8) as i32 % 70000,
        }
    }
}

// ---------------------------------------------------------------------------
// F199 音频性能预算 — 声音链路红线
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct AudioBudget {
    pub dsp_us_per_frame: u32,
    pub budget_us: u32,
    pub overruns: u32,
}

impl AudioBudget {
    pub const fn new(budget_us: u32) -> AudioBudget {
        AudioBudget { dsp_us_per_frame: 0, budget_us, overruns: 0 }
    }

    pub fn charge(&mut self, us: u32) {
        self.dsp_us_per_frame = us;
        if us > self.budget_us {
            self.overruns += 1;
        }
    }

    pub fn within(&self) -> bool {
        self.overruns == 0
    }
}

// ---------------------------------------------------------------------------
// F200 音声域自检 — 25 项 CheckSet 汇入总检
// ---------------------------------------------------------------------------

pub fn run_tone_checks() -> CheckSet {
    let mut set = CheckSet::new("audiotone");

    // F176 音色令牌
    let d = tone_derate(TONE_NOTIFY, 500);
    set.add(
        "F176 tone tokens",
        TONE_NOTIFY.freq_hz == 880 && d.gain_q15 == TONE_NOTIFY.gain_q15 / 2 && d.dur_ms == 60,
        "derate half",
    );

    // F177 音景
    let focus = soundscape_policy(Soundscape::Focus);
    let night = soundscape_policy(Soundscape::Night);
    set.add(
        "F177 soundscapes",
        focus.mute_notify && focus.ui_gain_cap_q15 == Q15 / 2 && night.ui_gain_cap_q15 == Q15 / 4,
        "policies",
    );

    // F178 混音总线
    let mut bus = MixBus::new();
    let _ = bus.set_gain(0, Q15);
    let _ = bus.set_gain(1, Q15 / 2);
    let out = bus.mix([Q15 as i32, Q15 as i32, 0, 0]);
    set.add("F178 mix bus", out.unsigned_abs() <= Q15 && bus.clipped >= 1, "soft limit");

    // F179 声像距离
    let (g_close, p0) = distance_model(0, 0);
    let (g_far, p_far) = distance_model(255, 64);
    set.add("F179 distance model", g_close == Q15 && g_far == 0 && p0 == 0 && p_far == 127, "attenuation");

    // F180 可懂度
    let mut prev = 0i32;
    let e1 = speech_enhance(1000, &mut prev, Q15 / 2);
    let e2 = speech_enhance(1000, &mut prev, Q15 / 2);
    set.add("F180 speech enhance", e1 == 500 && e2 == 0, "high-pass");

    // F181 听障
    let v1 = visual_indicator(SoundSource::Alarm, Q15);
    let v2 = visual_indicator(SoundSource::Alarm, 0);
    set.add("F181 hearing assist", v1 == 2 && v2 == 0, "visual mapping");

    // F182 事件转写
    let mut log = SoundEventLog::new();
    log.append(100, SoundSource::Notify, Q15);
    log.append(200, SoundSource::Alarm, Q15);
    let cap = log.caption(0);
    set.add("F182 event caption", cap[0] == b'A' && cap[1] == b'N', "caption order");

    // F183 回声预留
    let mut ec = EchoCancel::new();
    let mut err = 0i32;
    for _ in 0..1000 {
        err = ec.process(1000, 500);
    }
    set.add("F183 echo cancel", ec.coeff_q15 > 0 && err.unsigned_abs() < 600, "converging");

    // F184 降噪
    let out1 = denoise(30000, NrLevel::Medium);
    let out2 = denoise(30000, NrLevel::Off);
    let out3 = denoise(100, NrLevel::Strong);
    set.add("F184 nr levels", out1 == 30000 - Q15 as i32 / 4 && out2 == 30000 && out3 == 0, "spectral subtract");

    // F185 路由 DSL
    let mut rt = RouteTable::new(99);
    let _ = rt.add(1, 10);
    let _ = rt.add(2, 20);
    let _ = rt.add(1, 11);
    let r1 = rt.resolve(1);
    let r2 = rt.resolve(3);
    let _ = rt.disable(2);
    let r3 = rt.resolve(2);
    set.add("F185 routing dsl", r1 == 11 && r2 == 99 && r3 == 99, "last rule wins");

    // F186 虚拟声卡
    let mut vp = VirtualPipe::new();
    vp.open();
    let w1 = vp.write(100);
    let w2 = vp.write(200);
    let r1 = vp.read();
    vp.close();
    let r2 = vp.read();
    set.add("F186 virtual pipe", w1 && w2 && r1 == Some(100) && r2.is_none(), "fifo lifecycle");

    // F187 指纹
    let pcm: [i16; 16] = [100; 16];
    let fp1 = fingerprint(&pcm, 8);
    let fp2 = fingerprint(&pcm, 8);
    let pcm2: [i16; 16] = [30000; 16];
    let fp3 = fingerprint(&pcm2, 8);
    set.add("F187 fingerprint", fp1 == fp2 && fp_match(&fp1, &fp2) && !fp_match(&fp1, &fp3), "envelope sig");

    // F188 试听
    let mut au = Audition::new(TONE_NOTIFY, TONE_ERROR);
    let w1 = au.next();
    let w2 = au.next();
    let pick = au.pick(1);
    set.add("F188 audition", w1 == 0 && w2 == 1 && pick == TONE_ERROR, "alternate + pick");

    // F189 回归基线
    let base = AudioBaseline { latency_ms: 100, thd_permille: 10 };
    let ok = audio_regress(base, AudioBaseline { latency_ms: 105, thd_permille: 11 });
    let bad = audio_regress(base, AudioBaseline { latency_ms: 120, thd_permille: 10 });
    set.add("F189 audio baseline", !ok && bad, "10%/15% thresholds");

    // F190 分区播放
    let mut rm = RoomMatrix::new();
    let _ = rm.add(1, Q15);
    let _ = rm.add(2, Q15 / 2);
    let _ = rm.set_enabled(2, false);
    let out = rm.broadcast(1000);
    set.add("F190 rooms", out[0] == 1000 && out[1] == 0 && !rm.add(1, Q15), "room matrix");

    // F191 响度守护
    let mut guard = LoudnessGuard::new(Q15);
    let a = guard.enforce(Q15 / 2);
    let b = guard.enforce(Q15 * 2);
    set.add("F191 loudness guard", a == Q15 / 2 && b == Q15 && guard.trips == 1, "ceiling");

    // F192 漂移
    let mut dc = DriftCorrector::new();
    dc.set_drift(-500_000);
    let mut inserts = 0;
    for _ in 0..10 {
        if dc.tick() == 1 {
            inserts += 1;
        }
    }
    set.add("F192 drift correct", inserts == 5 && dc.inserted == 5, "resample markers");

    // F193 延迟热图
    let mut lm = LatencyMap::new();
    let _ = lm.add_stage("capture", 100);
    let _ = lm.add_stage("dsp", 500);
    let _ = lm.add_stage("output", 200);
    set.add(
        "F193 latency map",
        lm.total_us() == 800 && lm.hotspot() == Some(1),
        "hotspot dsp",
    );

    // F194 静默档
    let mut silent = SilentMode::new();
    silent.on = true;
    let g1 = silent.gate(Q15, false);
    let g2 = silent.gate(Q15, true);
    set.add("F194 silent mode", g1 == 0 && g2 == Q15, "critical exception");

    // F195 麦克风指示
    let mut mic = MicIndicator::new();
    let b1 = mic.begin(7);
    let b2 = mic.begin(8);
    let e2 = mic.end(8);
    set.add("F195 mic indicator", b1 && !b2 && !e2 && mic.in_use && mic.user_app == 7, "single owner");

    // F196 崩溃隔离
    let mut fuse = AudioFuse::new(3);
    fuse.record_fail();
    fuse.record_fail();
    let mid = !fuse.tripped;
    fuse.record_fail();
    let tripped = fuse.tripped;
    let rs = fuse.restart();
    set.add("F196 audio fuse", mid && tripped && rs && fuse.fails == 0, "trip + restart");

    // F197 声景编辑器
    let mut mx = SoundscapeMixer::new();
    let t1 = mx.toggle(0);
    let _ = mx.set(1, Q15 / 2, -64);
    let t2 = mx.toggle(1);
    let (l, r) = mx.render([1000, 1000, 0, 0]);
    set.add("F197 mixer ui", t1 && t2 && l > 0 && r > 0 && l > r, "pan left louder");

    // F198 fuzz
    let mut fz = AudioFuzzer::new(9);
    let mut all_ok = true;
    for _ in 0..1000 {
        let s = fz.gen_sample();
        let lim = soft_limit(s, Q15 * 3 / 4);
        if lim.unsigned_abs() > Q15 {
            all_ok = false;
        }
    }
    set.add("F198 audio fuzz", all_ok && fz.clamped == 0 && fz.passed == 0, "no panic, bounded");

    // F199 性能预算
    let mut bud = AudioBudget::new(1000);
    bud.charge(900);
    let ok1 = bud.within();
    bud.charge(1200);
    set.add("F199 audio budget", ok1 && !bud.within() && bud.overruns == 1, "redline");

    // F200 域自检可用性
    set.add("F200 tone selftest reachable", set.len() >= 24, "selftest must cover domain");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f178_soft_limit_bounded() {
        assert_eq!(soft_limit(Q15 as i32 / 2, Q15 * 3 / 4), Q15 as i32 / 2);
        assert!(soft_limit(Q15 as i32 * 10, Q15 * 3 / 4).unsigned_abs() <= Q15);
    }

    #[test]
    fn f187_fp_empty() {
        let fp = fingerprint(&[], 8);
        assert!(fp.iter().all(|&v| v == 0));
    }

    #[test]
    fn f200_selftest_passes() {
        let set = run_tone_checks();
        let mut buf = [0u8; 512];
        set.render(&mut buf);
        assert!(set.all_passed() && set.len() >= 25, "{}", core::str::from_utf8(&buf).unwrap_or("?"));
    }
}
