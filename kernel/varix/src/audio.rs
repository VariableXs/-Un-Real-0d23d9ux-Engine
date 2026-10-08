//! AI-12 音频域（F276~F300）。
//!
//! HDA/USB-Audio transport arithmetic, widget topology walk, a fixed-point
//! mixer/effects chain, latency accounting, device policy and the audio
//! self-test. All DSP is integer/fixed-point: the kernel has no FPU context
//! on the interrupt path, and fixed point keeps the mix bit-identical
//! between QEMU and real hardware.

use crate::checks::CheckSet;

/// VARIX-M500 AI-08 音声设计（F176~F200）。
pub mod tone;

/// Q15 gain: 1.0 == 32768 (stored as u32 to leave headroom).
pub const Q15_ONE: u32 = 32768;
/// Hard ceiling for a mixed sample (16-bit PCM).
pub const PCM_MAX: i32 = 32767;
pub const PCM_MIN: i32 = -32768;

// ---------------------------------------------------------------------------
// F276 — HDA 控制器
// ---------------------------------------------------------------------------

/// CORB/RIRB entry counts selectable by the size field.
pub fn ring_capacity(size_field: u8) -> usize {
    match size_field & 0x3 {
        0 => 2,
        1 => 16,
        2 => 256,
        _ => 0, // reserved — treated as "not configured"
    }
}

/// Advance a CORB/RIRB index modulo capacity (0 disallowed).
pub fn ring_next(current: usize, capacity: usize) -> usize {
    if capacity == 0 {
        return 0;
    }
    (current + 1) % capacity
}

/// Encode a codec command: (codec<<28) | (node<<20) | (verb<<8) | payload.
pub fn verb_encode(codec: u8, node: u8, verb: u16, payload: u16) -> u32 {
    ((codec as u32 & 0xF) << 28)
        | ((node as u32 & 0xFF) << 20)
        | (((verb as u32) & 0xFFF) << 8)
        | (payload as u32 & 0xFF)
}

/// Decode an RIRB response: (payload, solicited).
pub fn response_decode(response: u32) -> (u32, bool) {
    (response, response & (1 << 4) != 0)
}

pub const MAX_BDL_ENTRIES: usize = 256;

/// Bytes per frame for a stream format (channels × bytes per sample).
pub fn frame_bytes(channels: u8, bit_depth: u8) -> usize {
    channels as usize * ((bit_depth as usize + 7) / 8)
}

/// Period size in bytes for a stream given frames per interrupt.
pub fn period_bytes(frames: usize, channels: u8, bit_depth: u8) -> usize {
    frames * frame_bytes(channels, bit_depth)
}

// ---------------------------------------------------------------------------
// F277 — USB Audio
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UsbAudioFormat {
    pub channels: u8,
    pub bit_depth: u8,
    pub sample_rate: u32,
}

/// Parse a USB Audio class-specific FORMAT_TYPE descriptor (subtype 2).
/// Layout: len, type(0x24), subtype, formatType, nrChannels, subframeSize,
/// bitResolution, samFreqType, tSamFreq[3].
pub fn parse_format_descriptor(desc: &[u8]) -> Option<UsbAudioFormat> {
    if desc.len() < 11 || desc[1] != 0x24 || desc[2] != 0x02 {
        return None;
    }
    if desc[3] != 1 {
        return None; // only PCM format type I
    }
    let channels = desc[4];
    let bit_depth = desc[6];
    let rate = u32::from_le_bytes([desc[8], desc[9], desc[10], 0]);
    if channels == 0 || bit_depth == 0 || rate == 0 {
        return None;
    }
    Some(UsbAudioFormat { channels, bit_depth, sample_rate: rate })
}

/// Isochronous packet payload bytes for one USB service interval.
pub fn iso_packet_bytes(fmt: UsbAudioFormat, interval_us: u32) -> usize {
    let rate = fmt.sample_rate as u64;
    if rate == 0 {
        return 0;
    }
    let frames = (rate * interval_us as u64) / 1_000_000;
    (frames as usize) * frame_bytes(fmt.channels, fmt.bit_depth)
}

// ---------------------------------------------------------------------------
// F278 — 音频拓扑解析
// ---------------------------------------------------------------------------

pub const MAX_WIDGETS: usize = 16;
pub const MAX_CONNECTIONS: usize = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WidgetKind {
    AudioOutput,
    AudioInput,
    Mixer,
    Selector,
    PinComplex,
    Power,
    Vendor,
}

#[derive(Clone, Copy, Debug)]
pub struct Widget {
    pub nid: u8,
    pub kind: WidgetKind,
    /// Connection list (downstream NIDs).
    pub connections: [u8; MAX_CONNECTIONS],
    pub connection_count: usize,
}

#[derive(Clone, Copy, Debug)]
pub struct Topology {
    widgets: [Option<Widget>; MAX_WIDGETS],
    count: usize,
}

impl Topology {
    pub const fn new() -> Topology {
        Topology { widgets: [None; MAX_WIDGETS], count: 0 }
    }

    pub fn add(&mut self, widget: Widget) -> bool {
        if self.count >= MAX_WIDGETS {
            return false;
        }
        self.widgets[self.count] = Some(widget);
        self.count += 1;
        true
    }

    pub fn find(&self, nid: u8) -> Option<&Widget> {
        for i in 0..self.count {
            if let Some(w) = self.widgets[i] {
                if w.nid == nid {
                    return self.widgets[i].as_ref();
                }
            }
        }
        None
    }

    pub fn len(&self) -> usize {
        self.count
    }

    fn index_of(&self, nid: u8) -> Option<usize> {
        for i in 0..self.count {
            if let Some(w) = self.widgets[i] {
                if w.nid == nid {
                    return Some(i);
                }
            }
        }
        None
    }

    /// Shortest path (BFS) from `from` to `to` through connection lists.
    /// Returns the hop count, or `None` when disconnected (cycles terminate).
    pub fn path_len(&self, from: u8, to: u8) -> Option<usize> {
        if from == to {
            return Some(0);
        }
        let mut queue = [0u8; MAX_WIDGETS];
        let mut depth = [0usize; MAX_WIDGETS];
        let mut head = 0usize;
        let mut tail = 0usize;
        queue[tail] = from;
        depth[tail] = 0;
        tail += 1;
        while head < tail {
            let nid = queue[head];
            let d = depth[head];
            head += 1;
            let idx = match self.index_of(nid) {
                Some(i) => i,
                None => continue,
            };
            if let Some(w) = self.widgets[idx] {
                for c in 0..w.connection_count {
                    let next = w.connections[c];
                    if next == to {
                        return Some(d + 1);
                    }
                    let mut already = false;
                    for k in 0..tail {
                        if queue[k] == next {
                            already = true;
                            break;
                        }
                    }
                    if !already && tail < MAX_WIDGETS {
                        queue[tail] = next;
                        depth[tail] = d + 1;
                        tail += 1;
                    }
                }
            }
        }
        None
    }

    /// Output-pin count — used by the mixer path builder.
    pub fn outputs(&self) -> usize {
        (0..self.count)
            .filter(|i| {
                self.widgets[*i]
                    .map(|w| w.kind == WidgetKind::AudioOutput)
                    .unwrap_or(false)
            })
            .count()
    }
}

// ---------------------------------------------------------------------------
// F279 — 音量混合器
// ---------------------------------------------------------------------------

pub const MAX_STREAMS: usize = 8;

/// Attenuation table: 0, -5, -10 ... -60 dB in Q15.
const DB_TABLE: [u32; 13] = [
    Q15_ONE, 18432, 10368, 5832, 3280, 1844, 1037, 583, 328, 184, 104, 58, 0,
];

/// Decibels (in 1/10 dB) → Q15 gain. Positive dB never amplifies: the mixer
/// only attenuates, so headroom is preserved by construction.
pub fn db10_to_q15(db10: i32) -> u32 {
    let clamped = db10.clamp(-600, 600);
    if clamped >= 0 {
        return Q15_ONE;
    }
    let att = -clamped as usize;
    let idx = att / 50;
    if idx >= 12 {
        return 0;
    }
    let a = DB_TABLE[idx];
    let b = DB_TABLE[idx + 1];
    a - ((a - b) * (att % 50) as u32) / 50
}

/// Apply a Q15 gain to a sample with rounding.
pub fn apply_gain(sample: i32, gain_q15: u32) -> i32 {
    ((sample as i64 * gain_q15 as i64) / Q15_ONE as i64) as i32
}

#[derive(Clone, Copy, Debug)]
pub struct Stream {
    pub name: &'static str,
    pub gain_q15: u32,
    pub muted: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct Mixer {
    streams: [Option<Stream>; MAX_STREAMS],
    count: usize,
    pub master_q15: u32,
    pub master_muted: bool,
}

impl Mixer {
    pub const fn new() -> Mixer {
        Mixer {
            streams: [None; MAX_STREAMS],
            count: 0,
            master_q15: Q15_ONE,
            master_muted: false,
        }
    }

    pub fn add_stream(&mut self, stream: Stream) -> bool {
        if self.count >= MAX_STREAMS {
            return false;
        }
        self.streams[self.count] = Some(stream);
        self.count += 1;
        true
    }

    pub fn set_gain(&mut self, name: &str, gain_q15: u32) -> bool {
        for i in 0..self.count {
            if let Some(mut s) = self.streams[i] {
                if s.name == name {
                    s.gain_q15 = gain_q15;
                    self.streams[i] = Some(s);
                    return true;
                }
            }
        }
        false
    }

    pub fn len(&self) -> usize {
        self.count
    }

    /// Mix one frame from per-stream samples, then master gain + clip.
    pub fn mix(&self, inputs: &[i32]) -> i32 {
        let mut acc: i64 = 0;
        for i in 0..self.count.min(inputs.len()) {
            if let Some(s) = self.streams[i] {
                if !s.muted {
                    acc += apply_gain(inputs[i], s.gain_q15) as i64;
                }
            }
        }
        let master = if self.master_muted { 0 } else { self.master_q15 };
        let scaled = (acc * master as i64) / Q15_ONE as i64;
        soft_clip(scaled as i32)
    }
}

/// Hard limit at the PCM bounds (the soft knee lives in the compressor).
pub fn soft_clip(sample: i32) -> i32 {
    if sample > PCM_MAX {
        PCM_MAX
    } else if sample < PCM_MIN {
        PCM_MIN
    } else {
        sample
    }
}

// ---------------------------------------------------------------------------
// F280 — 采样率转换
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Resampler {
    pub src_rate: u32,
    pub dst_rate: u32,
    /// Fractional read position in 1/65536 of an input frame.
    pub position: u32,
}

impl Resampler {
    pub fn new(src_rate: u32, dst_rate: u32) -> Resampler {
        Resampler { src_rate, dst_rate, position: 0 }
    }

    /// Input advance per output frame, in 1/65536 units.
    pub fn step(&self) -> u32 {
        if self.dst_rate == 0 {
            return 0;
        }
        (((self.src_rate as u64) << 16) / self.dst_rate as u64) as u32
    }

    /// Linear interpolation of `input` into `output` (mono, i16).
    pub fn process(&mut self, input: &[i16], output: &mut [i16]) -> usize {
        if self.src_rate == 0 || self.dst_rate == 0 || input.len() < 2 {
            return 0;
        }
        let step = self.step();
        let mut written = 0usize;
        for out in output.iter_mut() {
            let pos = self.position;
            let idx = (pos >> 16) as usize;
            if idx + 1 >= input.len() {
                break;
            }
            let frac = (pos & 0xFFFF) as i32;
            let a = input[idx] as i32;
            let b = input[idx + 1] as i32;
            *out = (a + (((b - a) * frac) >> 16)) as i16;
            self.position = pos.wrapping_add(step);
            written += 1;
        }
        written
    }

    /// Latency added by resampling, in microseconds.
    pub fn latency_us(&self) -> u32 {
        if self.src_rate == 0 {
            return 0;
        }
        500_000 / self.src_rate
    }
}

// ---------------------------------------------------------------------------
// F281 — 低延迟路径
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LatencyBudget {
    pub period_frames: u32,
    pub periods: u32,
    pub rate_hz: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LatencyVerdict {
    /// <= 10 ms — imperceptible for UI feedback.
    Realtime,
    /// <= 20 ms — fine for media, tight for games.
    Interactive,
    Latent,
}

impl LatencyBudget {
    pub fn buffer_us(&self) -> u32 {
        if self.rate_hz == 0 {
            return 0;
        }
        (self.period_frames as u64 * self.periods as u64 * 1_000_000 / self.rate_hz as u64) as u32
    }

    pub fn buffer_bytes(&self, channels: u8, bit_depth: u8) -> usize {
        (self.period_frames as usize * self.periods as usize) * frame_bytes(channels, bit_depth)
    }
}

pub fn latency_verdict(us: u32) -> LatencyVerdict {
    if us <= 10_000 {
        LatencyVerdict::Realtime
    } else if us <= 20_000 {
        LatencyVerdict::Interactive
    } else {
        LatencyVerdict::Latent
    }
}

// ---------------------------------------------------------------------------
// F282 — underrun 守卫
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnderrunAction {
    Ok,
    /// Insert `frames` silence to keep the DMA ring alive.
    Pad(usize),
    /// Report a glitch to the event log (F296).
    Report(usize),
}

#[derive(Clone, Copy, Debug)]
pub struct UnderrunGuard {
    pub ring_frames: usize,
    pub watermark_frames: usize,
    pub consecutive: u32,
}

impl UnderrunGuard {
    /// Decide what to do when `available_frames` are queued.
    pub fn check(&mut self, available_frames: usize) -> UnderrunAction {
        if available_frames >= self.watermark_frames {
            self.consecutive = 0;
            return UnderrunAction::Ok;
        }
        let missing = self.watermark_frames - available_frames;
        self.consecutive += 1;
        if self.consecutive >= 3 {
            UnderrunReport::record(missing);
            UnderrunAction::Report(missing)
        } else {
            UnderrunAction::Pad(missing.min(self.ring_frames))
        }
    }
}

static UNDERRUNS: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
static UNDERRUN_FRAMES: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

/// Process-wide underrun accounting (single writer: the audio thread).
pub struct UnderrunReport;

impl UnderrunReport {
    pub fn record(missing: usize) {
        UNDERRUNS.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        UNDERRUN_FRAMES.fetch_add(missing as u32, core::sync::atomic::Ordering::Relaxed);
    }

    pub fn count() -> u32 {
        UNDERRUNS.load(core::sync::atomic::Ordering::Relaxed)
    }

    pub fn lost_frames() -> u32 {
        UNDERRUN_FRAMES.load(core::sync::atomic::Ordering::Relaxed)
    }
}

// ---------------------------------------------------------------------------
// F283 — 音效引擎
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Compressor {
    pub threshold_q15: u32,
    /// Ratio as permille (4000 = 4:1).
    pub ratio_permille: u32,
    pub makeup_q15: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct EffectChain {
    pub gain_q15: u32,
    /// Bass shelf in 1/10 dB, -600..+120.
    pub bass_db10: i32,
    pub treble_db10: i32,
    pub compressor: Compressor,
    low_state: i32,
}

impl EffectChain {
    pub const fn new() -> EffectChain {
        EffectChain {
            gain_q15: Q15_ONE,
            bass_db10: 0,
            treble_db10: 0,
            compressor: Compressor {
                threshold_q15: Q15_ONE / 2,
                ratio_permille: 4000,
                makeup_q15: Q15_ONE,
            },
            low_state: 0,
        }
    }

    /// Process one mono sample: gain → bass/high shelf → compressor.
    pub fn process(&mut self, sample: i32) -> i32 {
        let gained = apply_gain(sample, self.gain_q15);
        // One-pole low-pass (coeff 1/8) splits low vs. high band.
        self.low_state += (gained - self.low_state) / 8;
        let bass = apply_gain(self.low_state - gained, db10_to_q15(self.bass_db10));
        let high = gained - self.low_state;
        let treble = apply_gain(high, db10_to_q15(self.treble_db10));
        let out = gained + bass + treble;
        soft_clip(self.compress(out))
    }

    fn compress(&self, sample: i32) -> i32 {
        let threshold = self.compressor.threshold_q15 as i32;
        let mag = sample.abs();
        if mag <= threshold {
            return apply_gain(sample, self.compressor.makeup_q15);
        }
        let over = mag - threshold;
        let reduced = over * 1000 / self.compressor.ratio_permille.max(1000) as i32;
        let shaped = if sample < 0 { -(threshold + reduced) } else { threshold + reduced };
        apply_gain(shaped, self.compressor.makeup_q15)
    }
}

// ---------------------------------------------------------------------------
// F284/F285 — 提示音分级与系统音色包
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlertLevel {
    Info,
    Notice,
    Warning,
    Critical,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tone {
    pub freq_hz: u32,
    pub duration_ms: u32,
    pub attack_ms: u32,
    pub release_ms: u32,
    pub gain_q15: u32,
}

pub fn alert_tone(level: AlertLevel) -> Tone {
    match level {
        AlertLevel::Info => Tone {
            freq_hz: 660,
            duration_ms: 60,
            attack_ms: 4,
            release_ms: 30,
            gain_q15: db10_to_q15(-180),
        },
        AlertLevel::Notice => Tone {
            freq_hz: 880,
            duration_ms: 90,
            attack_ms: 4,
            release_ms: 40,
            gain_q15: db10_to_q15(-120),
        },
        AlertLevel::Warning => Tone {
            freq_hz: 520,
            duration_ms: 160,
            attack_ms: 3,
            release_ms: 60,
            gain_q15: db10_to_q15(-60),
        },
        AlertLevel::Critical => Tone {
            freq_hz: 340,
            duration_ms: 260,
            attack_ms: 2,
            release_ms: 90,
            gain_q15: db10_to_q15(-20),
        },
    }
}

/// System tone pack (F285): original, licence-free, tiny.
pub const TONE_PACK: [(&'static str, Tone); 6] = [
    ("boot", Tone { freq_hz: 528, duration_ms: 220, attack_ms: 8, release_ms: 120, gain_q15: Q15_ONE / 4 }),
    ("shutdown", Tone { freq_hz: 396, duration_ms: 260, attack_ms: 6, release_ms: 160, gain_q15: Q15_ONE / 4 }),
    ("plug", Tone { freq_hz: 740, duration_ms: 70, attack_ms: 3, release_ms: 30, gain_q15: Q15_ONE / 5 }),
    ("unplug", Tone { freq_hz: 560, duration_ms: 70, attack_ms: 3, release_ms: 30, gain_q15: Q15_ONE / 5 }),
    ("notify", Tone { freq_hz: 880, duration_ms: 90, attack_ms: 4, release_ms: 40, gain_q15: Q15_ONE / 6 }),
    ("error", Tone { freq_hz: 300, duration_ms: 240, attack_ms: 2, release_ms: 90, gain_q15: Q15_ONE / 3 }),
];

pub fn tone_by_name(name: &str) -> Option<Tone> {
    for (n, t) in TONE_PACK.iter() {
        if *n == name {
            return Some(*t);
        }
    }
    None
}

/// sin on [0, pi/2], `s` normalized to Q15 (32768 == pi/2).
fn sin_quarter_q15(s: i64) -> i32 {
    let x = (s * 51472) / 32768; // radians in Q15
    let x2 = (x * x) / 32768;
    let x3 = (x2 * x) / 32768;
    let x5 = (x3 * x2) / 32768;
    let x7 = (x5 * x2) / 32768;
    (x - x3 / 6 + x5 / 120 - x7 / 5040) as i32
}

/// cos on [0, pi/2], `s` normalized to Q15 (32768 == pi/2).
fn cos_quarter_q15(s: i64) -> i32 {
    let x = (s * 51472) / 32768;
    let x2 = (x * x) / 32768;
    let x4 = (x2 * x2) / 32768;
    let x6 = (x4 * x2) / 32768;
    (32768 - x2 / 2 + x4 / 24 - x6 / 720) as i32
}

/// Integer sine (milli-radians in, Q15 out) — Taylor, ~1e-4 accuracy.
pub fn sin_q15(angle_milli: i32) -> i32 {
    const TWO_PI: i32 = 6283;
    let mut x = angle_milli % TWO_PI;
    if x < 0 {
        x += TWO_PI;
    }
    let quad = (x / 1571) % 4;
    let s = ((x % 1571) as i64 * 32768) / 1571;
    match quad {
        0 => sin_quarter_q15(s),
        1 => cos_quarter_q15(s),
        2 => -sin_quarter_q15(s),
        _ => -cos_quarter_q15(s),
    }
}

/// Render a tone with an ADSR envelope into `out` at `rate_hz`.
pub fn render_tone(tone: Tone, rate_hz: u32, out: &mut [i16]) -> usize {
    if rate_hz == 0 || out.is_empty() {
        return 0;
    }
    let total = ((tone.duration_ms as u64 * rate_hz as u64) / 1000) as usize;
    let n = total.min(out.len());
    for i in 0..n {
        let t_ms = ((i as u64 * 1000) / rate_hz as u64) as u32;
        let env = if t_ms < tone.attack_ms {
            (t_ms * Q15_ONE) / tone.attack_ms.max(1)
        } else if t_ms + tone.release_ms > tone.duration_ms {
            let left = tone.duration_ms.saturating_sub(t_ms).max(1);
            (left * Q15_ONE) / tone.release_ms.max(1)
        } else {
            Q15_ONE
        };
        let phase = (((i as u64 * tone.freq_hz as u64 * 6283) / rate_hz as u64) % 6283) as i32;
        let s = sin_q15(phase);
        let v = (s as i64 * tone.gain_q15 as i64 * env as i64) / (Q15_ONE as i64 * Q15_ONE as i64);
        out[i] = soft_clip(v as i32) as i16;
    }
    n
}

// ---------------------------------------------------------------------------
// F286 — 空间音效
// ---------------------------------------------------------------------------

/// Equal-power pan table replaced by the shared sine helper: the pan angle
/// runs 0° (hard left) .. 90° (hard right), L = cos θ, R = sin θ.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Spatializer {
    /// -1800..1800 (1/10 degree), 0 = front centre.
    pub azimuth_deci_deg: i16,
    /// Inter-aural time difference cap in frames.
    pub max_itd_frames: u16,
}

impl Spatializer {
    /// Pan gains (left, right) in Q15 — constant-power law.
    pub fn pan_gains(&self) -> (u32, u32) {
        let a = self.azimuth_deci_deg.clamp(-1800, 1800) as i32;
        let theta_d10 = 450 + a / 4; // 0..900 in 1/10 degree
        let milli = theta_d10 * 17453 / 10000; // milli-radians
        let left = sin_q15(milli + 1571).max(0) as u32; // cos θ
        let right = sin_q15(milli).max(0) as u32; // sin θ
        (left, right)
    }

    /// ITD in frames (0 at centre, cap at 90 degrees).
    pub fn itd_frames(&self) -> u16 {
        let a = self.azimuth_deci_deg.abs().min(1800);
        ((a as u32 * self.max_itd_frames as u32) / 1800) as u16
    }
}

// ---------------------------------------------------------------------------
// F287 — 麦克风输入
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CaptureConfig {
    pub rate_hz: u32,
    pub channels: u8,
    pub gain_db10: i32,
}

impl CaptureConfig {
    pub fn gain_q15(&self) -> u32 {
        db10_to_q15(self.gain_db10)
    }

    pub fn bytes_per_second(&self) -> usize {
        self.rate_hz as usize * frame_bytes(self.channels, 16)
    }
}

// ---------------------------------------------------------------------------
// F288 — 底噪抑制
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GateState {
    Closed,
    Opening,
    Open,
    Closing,
}

#[derive(Clone, Copy, Debug)]
pub struct NoiseGate {
    pub open_threshold: u32,
    pub close_threshold: u32,
    pub hold_ms: u32,
    state: GateState,
    hold_remaining_ms: u32,
}

impl NoiseGate {
    pub const fn new(open_threshold: u32, close_threshold: u32, hold_ms: u32) -> NoiseGate {
        NoiseGate {
            open_threshold,
            close_threshold,
            hold_ms,
            state: GateState::Closed,
            hold_remaining_ms: 0,
        }
    }

    pub fn state(&self) -> GateState {
        self.state
    }

    /// Advance the gate by `dt_ms` with the current envelope `level`.
    pub fn tick(&mut self, level: u32, dt_ms: u32) -> GateState {
        match self.state {
            GateState::Closed | GateState::Closing => {
                if level >= self.open_threshold {
                    self.state = GateState::Open;
                    self.hold_remaining_ms = self.hold_ms;
                } else {
                    self.state = GateState::Closed;
                }
            }
            GateState::Open | GateState::Opening => {
                if level >= self.close_threshold {
                    self.hold_remaining_ms = self.hold_ms;
                    self.state = GateState::Open;
                } else if self.hold_remaining_ms > dt_ms {
                    self.hold_remaining_ms -= dt_ms;
                } else {
                    self.hold_remaining_ms = 0;
                    self.state = GateState::Closed;
                }
            }
        }
        self.state
    }

    pub fn gain_q15(&self) -> u32 {
        match self.state {
            GateState::Open => Q15_ONE,
            GateState::Opening | GateState::Closing => Q15_ONE / 2,
            GateState::Closed => 0,
        }
    }
}

/// Spectral-subtraction magnitude shaping on pre-computed bins (F290).
pub fn denoise_magnitude(magnitude: u32, noise_floor: u32, over_subtract_permille: u32) -> u32 {
    let sub = (noise_floor * over_subtract_permille) / 1000;
    magnitude.saturating_sub(sub)
}

// ---------------------------------------------------------------------------
// F289 — 录音服务
// ---------------------------------------------------------------------------

/// Build a 44-byte canonical WAV header.
pub fn wav_header(data_bytes: u32, rate_hz: u32, channels: u8, bit_depth: u8) -> [u8; 44] {
    let mut h = [0u8; 44];
    let block_align = frame_bytes(channels, bit_depth) as u16;
    let byte_rate = rate_hz * block_align as u32;
    h[0..4].copy_from_slice(b"RIFF");
    h[4..8].copy_from_slice(&(36u32 + data_bytes).to_le_bytes());
    h[8..12].copy_from_slice(b"WAVE");
    h[12..16].copy_from_slice(b"fmt ");
    h[16..20].copy_from_slice(&16u32.to_le_bytes());
    h[20..22].copy_from_slice(&1u16.to_le_bytes()); // PCM
    h[22..24].copy_from_slice(&(channels as u16).to_le_bytes());
    h[24..28].copy_from_slice(&rate_hz.to_le_bytes());
    h[28..32].copy_from_slice(&byte_rate.to_le_bytes());
    h[32..34].copy_from_slice(&block_align.to_le_bytes());
    h[34..36].copy_from_slice(&(bit_depth as u16).to_le_bytes());
    h[36..40].copy_from_slice(b"data");
    h[40..44].copy_from_slice(&data_bytes.to_le_bytes());
    h
}

/// Seconds of audio that fit in `free_bytes`.
pub fn recordable_seconds(free_bytes: u64, rate_hz: u32, channels: u8, bit_depth: u8) -> u64 {
    let bps = rate_hz as u64 * frame_bytes(channels, bit_depth) as u64;
    if bps == 0 {
        return 0;
    }
    free_bytes.saturating_sub(44) / bps
}

// ---------------------------------------------------------------------------
// F290 — 音频可视化
// ---------------------------------------------------------------------------

pub const SPECTRUM_BINS: usize = 16;

#[derive(Clone, Copy, Debug)]
pub struct Spectrum {
    pub bins: [u16; SPECTRUM_BINS],
    pub peak_hold: [u16; SPECTRUM_BINS],
}

impl Spectrum {
    pub const fn new() -> Spectrum {
        Spectrum { bins: [0; SPECTRUM_BINS], peak_hold: [0; SPECTRUM_BINS] }
    }

    /// Update from raw bin magnitudes with a log scaling and peak decay.
    pub fn update(&mut self, magnitudes: &[u32]) {
        for i in 0..SPECTRUM_BINS {
            let mut v = magnitudes.get(i).copied().unwrap_or(0);
            let mut bits = 0u32;
            while v > 1 {
                v >>= 1;
                bits += 1;
            }
            let value = (bits * 4369).min(0xFFFF) as u16;
            self.bins[i] = value;
            if value >= self.peak_hold[i] {
                self.peak_hold[i] = value;
            } else {
                self.peak_hold[i] = self.peak_hold[i].saturating_sub(self.peak_hold[i] / 16);
            }
        }
    }

    /// Bar height in pixels for a bin (0..max_height).
    pub fn bar_height(&self, bin: usize, max_height: u16) -> u16 {
        let v = self.bins.get(bin).copied().unwrap_or(0);
        ((v as u32 * max_height as u32) / 0xFFFF) as u16
    }
}

// ---------------------------------------------------------------------------
// F291 — 设备热切换
// ---------------------------------------------------------------------------

pub const MAX_DEVICES: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceRole {
    Output,
    Input,
}

#[derive(Clone, Copy, Debug)]
pub struct AudioDevice {
    pub name: &'static str,
    pub role: DeviceRole,
    pub present: bool,
    pub rate_hz: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct DeviceRegistry {
    devices: [Option<AudioDevice>; MAX_DEVICES],
    count: usize,
    active: &'static str,
}

impl DeviceRegistry {
    pub const fn new() -> DeviceRegistry {
        DeviceRegistry { devices: [None; MAX_DEVICES], count: 0, active: "" }
    }

    pub fn upsert(&mut self, device: AudioDevice) {
        for i in 0..self.count {
            if let Some(mut d) = self.devices[i] {
                if d.name == device.name {
                    d.present = device.present;
                    d.rate_hz = device.rate_hz;
                    self.devices[i] = Some(d);
                    return;
                }
            }
        }
        if self.count < MAX_DEVICES {
            self.devices[self.count] = Some(device);
            self.count += 1;
        }
    }

    /// Pick the active output: keep the last choice while it is present,
    /// otherwise fall back so a hot-unplug never leaves permanent silence.
    pub fn select_output(&mut self) -> Option<&'static str> {
        let mut found: Option<&'static str> = None;
        for i in 0..self.count {
            if let Some(d) = self.devices[i] {
                if d.role == DeviceRole::Output && d.present {
                    if found.is_none() {
                        found = Some(d.name);
                    }
                    if d.name == self.active {
                        found = Some(d.name);
                        break;
                    }
                }
            }
        }
        if let Some(name) = found {
            self.active = name;
        }
        found
    }

    /// Present output device count (multi-output support, F299).
    pub fn present_outputs(&self) -> usize {
        (0..self.count)
            .filter(|i| {
                self.devices[*i]
                    .map(|d| d.present && d.role == DeviceRole::Output)
                    .unwrap_or(false)
            })
            .count()
    }
}

// ---------------------------------------------------------------------------
// F292 — 静音热键
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MuteState {
    pub muted: bool,
    pub restore_q15: u32,
}

impl MuteState {
    pub const fn new() -> MuteState {
        MuteState { muted: false, restore_q15: Q15_ONE }
    }

    /// Toggle mute; the previous gain is remembered for restore.
    pub fn toggle(&mut self, current_q15: u32) -> u32 {
        if self.muted {
            self.muted = false;
            self.restore_q15.max(1)
        } else {
            self.restore_q15 = current_q15;
            self.muted = true;
            0
        }
    }
}

// ---------------------------------------------------------------------------
// F293 — 音频延迟测量
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LatencyMeter {
    pub buffer_us: u32,
    pub hardware_us: u32,
    pub resampler_us: u32,
}

impl LatencyMeter {
    pub fn total_us(&self) -> u32 {
        self.buffer_us + self.hardware_us + self.resampler_us
    }

    /// A/V sync offset in milliseconds (positive = audio late).
    pub fn sync_offset_ms(&self, video_us: u32) -> i32 {
        ((self.total_us() as i64 - video_us as i64) / 1000) as i32
    }
}

// ---------------------------------------------------------------------------
// F294 — 音量曲线
// ---------------------------------------------------------------------------

/// Perceptual slider curve: slider 0..100 → Q15 gain.
/// Cubic taper keeps the low half of the travel in the quiet range where
/// hearing is most sensitive, is exactly invertible in Q15, and 0 == mute.
pub fn slider_to_q15(slider: u8) -> u32 {
    let s = slider.min(100) as u64;
    ((s * s * s * Q15_ONE as u64) / 1_000_000) as u32
}

/// Inverse curve (monotonic binary search) for UI sync.
pub fn q15_to_slider(gain_q15: u32) -> u8 {
    if gain_q15 >= Q15_ONE {
        return 100;
    }
    let mut lo = 0u8;
    let mut hi = 100u8;
    while lo < hi {
        let mid = (lo + hi + 1) / 2;
        if slider_to_q15(mid) <= gain_q15 {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    lo
}

// ---------------------------------------------------------------------------
// F295 — 帧率感知混音
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameAwareMixer {
    pub frame_period_us: u32,
    pub rate_hz: u32,
    pub max_quantum_frames: u32,
}

impl FrameAwareMixer {
    /// Frames to render per display frame.
    pub fn quantum_frames(&self) -> u32 {
        if self.rate_hz == 0 {
            return 0;
        }
        let want = (self.frame_period_us as u64 * self.rate_hz as u64) / 1_000_000;
        want.max(1).min(self.max_quantum_frames as u64) as u32
    }

    /// Jitter guard: when the display frame slips, shrink the quantum
    /// instead of accumulating latency.
    pub fn align(&self, measured_frame_us: u32) -> u32 {
        if measured_frame_us > self.frame_period_us * 2 {
            self.quantum_frames() / 2
        } else {
            self.quantum_frames()
        }
    }
}

// ---------------------------------------------------------------------------
// F296 — 音频事件日志
// ---------------------------------------------------------------------------

const AUDIO_EVENT_CAP: usize = 12;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioEventKind {
    DevicePlug,
    DeviceUnplug,
    Underrun,
    Overrun,
    StreamStart,
    StreamStop,
    RouteChange,
    Mute,
}

#[derive(Clone, Copy, Debug)]
pub struct AudioEvent {
    pub kind: AudioEventKind,
    pub stamp: u64,
    pub arg: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct AudioEventLog {
    events: [AudioEvent; AUDIO_EVENT_CAP],
    head: usize,
    count: usize,
}

impl AudioEventLog {
    pub const fn new() -> AudioEventLog {
        AudioEventLog {
            events: [AudioEvent { kind: AudioEventKind::Mute, stamp: 0, arg: 0 }; AUDIO_EVENT_CAP],
            head: 0,
            count: 0,
        }
    }

    pub fn push(&mut self, event: AudioEvent) {
        self.events[self.head] = event;
        self.head = (self.head + 1) % AUDIO_EVENT_CAP;
        if self.count < AUDIO_EVENT_CAP {
            self.count += 1;
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn get(&self, index: usize) -> Option<AudioEvent> {
        if index >= self.count {
            return None;
        }
        let pos = (self.head + AUDIO_EVENT_CAP - 1 - index) % AUDIO_EVENT_CAP;
        Some(self.events[pos])
    }

    pub fn count_of(&self, kind: AudioEventKind) -> usize {
        (0..self.count).filter(|i| self.get(*i).map(|e| e.kind == kind).unwrap_or(false)).count()
    }
}

// ---------------------------------------------------------------------------
// F297 — 音频降级
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DegradeAction {
    /// Real device at the native rate.
    Native,
    /// Device exists but cannot do the requested rate: resample.
    Resample,
    /// No device at all: null sink keeps every API honest.
    NullSink,
}

pub fn degrade_action(device_present: bool, device_rate: u32, want_rate: u32) -> DegradeAction {
    if !device_present || device_rate == 0 {
        DegradeAction::NullSink
    } else if device_rate == want_rate {
        DegradeAction::Native
    } else {
        DegradeAction::Resample
    }
}

// ---------------------------------------------------------------------------
// F298 — 提醒音设计规范
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ToneRules {
    pub max_duration_ms: u32,
    pub max_gain_q15: u32,
    /// Quiet-hours gain scale (permille).
    pub night_scale_permille: u32,
}

impl Default for ToneRules {
    fn default() -> ToneRules {
        ToneRules { max_duration_ms: 400, max_gain_q15: Q15_ONE / 2, night_scale_permille: 400 }
    }
}

/// Apply the "do not disturb" rules to a tone.
pub fn apply_tone_rules(tone: Tone, rules: ToneRules, quiet_hours: bool) -> Tone {
    let mut out = tone;
    if out.duration_ms > rules.max_duration_ms {
        out.duration_ms = rules.max_duration_ms;
    }
    if out.gain_q15 > rules.max_gain_q15 {
        out.gain_q15 = rules.max_gain_q15;
    }
    if quiet_hours {
        out.gain_q15 = (out.gain_q15 * rules.night_scale_permille) / 1000;
        out.freq_hz = (out.freq_hz * 3) / 4; // warmer, less piercing
    }
    out
}

// ---------------------------------------------------------------------------
// F299 — 多设备输出
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OutputDevice {
    pub name: &'static str,
    /// Hardware latency in microseconds (wireless > wired).
    pub latency_us: u32,
    pub enabled: bool,
}

/// Frames of delay to add so every device lands on the same instant.
pub fn compensation_frames(dev: OutputDevice, reference_us: u32, rate_hz: u32) -> u32 {
    if rate_hz == 0 || dev.latency_us >= reference_us {
        return 0;
    }
    ((reference_us - dev.latency_us) as u64 * rate_hz as u64 / 1_000_000) as u32
}

/// Reference latency = slowest enabled device.
pub fn reference_latency(devices: &[OutputDevice]) -> u32 {
    devices.iter().filter(|d| d.enabled).map(|d| d.latency_us).max().unwrap_or(0)
}

// ---------------------------------------------------------------------------
// F300 — 音频自检
// ---------------------------------------------------------------------------

pub fn run_audio_checks() -> CheckSet {
    let mut set = CheckSet::new("audio");

    set.add(
        "F276 hda verb",
        verb_encode(0, 1, 0xF00, 0x12) == 0x001F_0012
            && ring_capacity(2) == 256
            && ring_capacity(0) == 2
            && ring_next(255, 256) == 0
            && frame_bytes(2, 16) == 4
            && period_bytes(48, 2, 16) == 192,
        "codec cmd",
    );

    let desc = [0x0Bu8, 0x24, 0x02, 0x01, 0x02, 0x02, 0x10, 0x00, 0x80, 0xBB, 0x00];
    let fmt = parse_format_descriptor(&desc).expect("format");
    set.add(
        "F277 usb audio",
        fmt.channels == 2 && fmt.bit_depth == 16 && fmt.sample_rate == 48_000
            && iso_packet_bytes(fmt, 1000) == 192,
        "format desc",
    );

    let mut topo = Topology::new();
    topo.add(Widget { nid: 1, kind: WidgetKind::AudioOutput, connections: [2, 0, 0, 0, 0, 0], connection_count: 1 });
    topo.add(Widget { nid: 2, kind: WidgetKind::Mixer, connections: [3, 0, 0, 0, 0, 0], connection_count: 1 });
    topo.add(Widget { nid: 3, kind: WidgetKind::PinComplex, connections: [0; 6], connection_count: 0 });
    set.add(
        "F278 topology",
        topo.path_len(1, 3) == Some(2) && topo.path_len(3, 1) == None && topo.outputs() == 1
            && topo.find(2).is_some(),
        "path walk",
    );

    let mut mixer = Mixer::new();
    mixer.add_stream(Stream { name: "music", gain_q15: Q15_ONE, muted: false });
    mixer.add_stream(Stream { name: "sfx", gain_q15: Q15_ONE / 2, muted: false });
    set.add(
        "F279 mixer",
        mixer.mix(&[10_000, 10_000]) == 15_000 && mixer.mix(&[30_000, 30_000]) == PCM_MAX
            && soft_clip(40_000) == PCM_MAX && soft_clip(-40_000) == PCM_MIN,
        "mix + clip",
    );
    set.add(
        "F279 mute",
        mixer.set_gain("sfx", 0) && mixer.mix(&[10_000, 10_000]) == 10_000,
        "per-stream",
    );

    let mut rs = Resampler::new(48_000, 44_100);
    let input = [0i16, 1_000, 2_000, 3_000, 4_000, 5_000];
    let mut out = [0i16; 8];
    let n = rs.process(&input, &mut out);
    set.add("F280 resampler", n > 0 && rs.step() > 65536 && rs.latency_us() == 10, "rate convert");

    let budget = LatencyBudget { period_frames: 48, periods: 2, rate_hz: 48_000 };
    set.add(
        "F281 latency",
        budget.buffer_us() == 2000
            && budget.buffer_bytes(2, 16) == 384
            && latency_verdict(5_000) == LatencyVerdict::Realtime
            && latency_verdict(50_000) == LatencyVerdict::Latent,
        "low latency",
    );

    let mut guard = UnderrunGuard { ring_frames: 512, watermark_frames: 128, consecutive: 0 };
    set.add(
        "F282 underrun",
        guard.check(200) == UnderrunAction::Ok
            && matches!(guard.check(100), UnderrunAction::Pad(_))
            && matches!(guard.check(100), UnderrunAction::Pad(_))
            && matches!(guard.check(100), UnderrunAction::Report(_)),
        "underrun guard",
    );

    let mut chain = EffectChain::new();
    let processed = chain.process(20_000);
    set.add(
        "F283 effects",
        processed != 0 && processed.abs() <= PCM_MAX && processed != 20_000,
        "effect chain",
    );
    let mut loud = EffectChain::new();
    loud.compressor.threshold_q15 = Q15_ONE / 4;
    set.add(
        "F283 compressor",
        loud.process(32_000).abs() < apply_gain(32_000, Q15_ONE).abs(),
        "compress",
    );

    let info = alert_tone(AlertLevel::Info);
    let critical = alert_tone(AlertLevel::Critical);
    set.add(
        "F284 alert levels",
        critical.gain_q15 > info.gain_q15 && critical.duration_ms > info.duration_ms
            && critical.freq_hz < info.freq_hz,
        "graded alerts",
    );

    let boot = tone_by_name("boot").expect("tone");
    let mut pcm = [0i16; 64];
    set.add(
        "F285 tone pack",
        tone_by_name("missing").is_none()
            && render_tone(boot, 8000, &mut pcm) > 0
            && pcm.iter().any(|s| *s != 0)
            && sin_q15(0) == 0,
        "tone render",
    );

    let sp = Spatializer { azimuth_deci_deg: 900, max_itd_frames: 16 };
    let (l, r) = sp.pan_gains();
    set.add(
        "F286 spatial",
        r > l && l + r >= 32768 - 4096 && sp.itd_frames() == 8,
        "panning",
    );

    let cap = CaptureConfig { rate_hz: 48_000, channels: 1, gain_db10: 200 };
    set.add(
        "F287 capture",
        cap.bytes_per_second() == 96_000 && cap.gain_q15() == Q15_ONE,
        "mic input",
    );

    let mut gate = NoiseGate::new(1000, 400, 200);
    set.add(
        "F288 noise gate",
        gate.tick(2000, 10) == GateState::Open
            && gate.gain_q15() == Q15_ONE
            && gate.tick(10, 10) == GateState::Open
            && denoise_magnitude(1000, 400, 1500) == 400,
        "gate + denoise",
    );

    let hdr = wav_header(1000, 48_000, 2, 16);
    set.add(
        "F289 recorder",
        &hdr[0..4] == b"RIFF" && &hdr[8..12] == b"WAVE" && &hdr[36..40] == b"data"
            && recordable_seconds(1_000_000, 48_000, 2, 16) == 5,
        "wav header",
    );

    let mut spec = Spectrum::new();
    spec.update(&[0xFFFF, 0x0FFF, 0]);
    set.add(
        "F290 spectrum",
        spec.bar_height(0, 100) == 100 && spec.bar_height(2, 100) == 0
            && spec.bar_height(99, 100) == 0,
        "visualizer",
    );

    let mut reg = DeviceRegistry::new();
    reg.upsert(AudioDevice { name: "hda", role: DeviceRole::Output, present: true, rate_hz: 48_000 });
    reg.upsert(AudioDevice { name: "usb", role: DeviceRole::Output, present: false, rate_hz: 48_000 });
    set.add(
        "F291 hot switch",
        reg.select_output() == Some("hda") && reg.present_outputs() == 1,
        "device switch",
    );

    let mut mute = MuteState::new();
    set.add(
        "F292 mute key",
        mute.toggle(Q15_ONE / 2) == 0 && mute.muted && mute.toggle(0) == Q15_ONE / 2 && !mute.muted,
        "mute toggle",
    );

    let meter = LatencyMeter { buffer_us: 2000, hardware_us: 1500, resampler_us: 10 };
    set.add(
        "F293 latency meter",
        meter.total_us() == 3510 && meter.sync_offset_ms(16_000) == -12,
        "sync offset",
    );

    set.add(
        "F294 volume curve",
        slider_to_q15(0) == 0 && slider_to_q15(100) == Q15_ONE && slider_to_q15(50) < Q15_ONE / 4
            && q15_to_slider(slider_to_q15(70)) >= 68,
        "perceptual curve",
    );

    let fam = FrameAwareMixer { frame_period_us: 16_667, rate_hz: 48_000, max_quantum_frames: 4096 };
    set.add(
        "F295 frame aware",
        fam.quantum_frames() == 800 && fam.align(40_000) == 400 && fam.align(16_000) == 800,
        "quantum",
    );

    let mut log = AudioEventLog::new();
    log.push(AudioEvent { kind: AudioEventKind::Underrun, stamp: 1, arg: 0 });
    log.push(AudioEvent { kind: AudioEventKind::DevicePlug, stamp: 2, arg: 1 });
    set.add(
        "F296 event log",
        log.len() == 2 && log.get(0).unwrap().kind == AudioEventKind::DevicePlug
            && log.count_of(AudioEventKind::Underrun) == 1,
        "events",
    );

    set.add(
        "F297 degrade",
        degrade_action(false, 0, 48_000) == DegradeAction::NullSink
            && degrade_action(true, 48_000, 48_000) == DegradeAction::Native
            && degrade_action(true, 44_100, 48_000) == DegradeAction::Resample,
        "degrade policy",
    );

    let rules = ToneRules::default();
    let loud_tone =
        Tone { freq_hz: 1000, duration_ms: 900, attack_ms: 1, release_ms: 1, gain_q15: Q15_ONE };
    let quiet = apply_tone_rules(loud_tone, rules, true);
    set.add(
        "F298 tone rules",
        quiet.duration_ms == 400 && quiet.gain_q15 < rules.max_gain_q15 && quiet.freq_hz == 750
            && apply_tone_rules(loud_tone, rules, false).gain_q15 == rules.max_gain_q15,
        "do not disturb",
    );

    let devices = [
        OutputDevice { name: "wired", latency_us: 2_000, enabled: true },
        OutputDevice { name: "bt", latency_us: 20_000, enabled: true },
        OutputDevice { name: "off", latency_us: 100_000, enabled: false },
    ];
    set.add(
        "F299 multi output",
        reference_latency(&devices) == 20_000
            && compensation_frames(devices[0], 20_000, 48_000) == 864
            && compensation_frames(devices[1], 20_000, 48_000) == 0,
        "delay compensation",
    );

    set
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f276_ring_and_verbs() {
        assert_eq!(ring_capacity(3), 0);
        assert_eq!(ring_next(1, 16), 2);
        assert_eq!(
            verb_encode(2, 0x14, 0x701, 0x3F),
            (2 << 28) | (0x14 << 20) | (0x701 << 8) | 0x3F
        );
        let (payload, solicited) = response_decode(0x02);
        assert_eq!(payload, 0x02);
        assert!(!solicited);
        assert!(response_decode(0x12).1);
        assert_eq!(frame_bytes(1, 24), 3);
    }

    #[test]
    fn f277_rejects_bad_descriptors() {
        assert!(parse_format_descriptor(&[0u8; 4]).is_none());
        let mut d = [0x0Bu8, 0x24, 0x02, 0x01, 0x02, 0x02, 0x10, 0x00, 0x80, 0xBB, 0x00];
        d[2] = 0x03;
        assert!(parse_format_descriptor(&d).is_none());
        d[2] = 0x02;
        d[4] = 0;
        assert!(parse_format_descriptor(&d).is_none());
        d[4] = 2;
        d[3] = 2;
        assert!(parse_format_descriptor(&d).is_none());
    }

    #[test]
    fn f278_topology_capacity_and_cycles() {
        let mut t = Topology::new();
        for i in 0..MAX_WIDGETS + 2 {
            let ok = t.add(Widget {
                nid: i as u8,
                kind: WidgetKind::Vendor,
                connections: [0; 6],
                connection_count: 0,
            });
            if i >= MAX_WIDGETS {
                assert!(!ok);
            }
        }
        assert_eq!(t.len(), MAX_WIDGETS);
        // Cycle 1 -> 2 -> 1 must terminate.
        let mut c = Topology::new();
        c.add(Widget { nid: 1, kind: WidgetKind::Mixer, connections: [2, 0, 0, 0, 0, 0], connection_count: 1 });
        c.add(Widget { nid: 2, kind: WidgetKind::Mixer, connections: [1, 0, 0, 0, 0, 0], connection_count: 1 });
        assert_eq!(c.path_len(1, 9), None);
        assert_eq!(c.path_len(1, 1), Some(0));
        assert!(c.find(9).is_none());
    }

    #[test]
    fn f279_gain_table_monotonic() {
        let mut prev = 0u32;
        for db in (-600..=0).step_by(50) {
            let g = db10_to_q15(db);
            assert!(g >= prev, "gain must rise monotonically at {} dB", db);
            prev = g;
        }
        assert_eq!(db10_to_q15(0), Q15_ONE);
        assert_eq!(db10_to_q15(600), Q15_ONE);
        assert_eq!(db10_to_q15(-6000), db10_to_q15(-600));
        assert_eq!(apply_gain(10_000, Q15_ONE / 2), 5_000);
    }

    #[test]
    fn f279_mixer_master_and_capacity() {
        let mut m = Mixer::new();
        m.add_stream(Stream { name: "a", gain_q15: Q15_ONE, muted: false });
        m.master_muted = true;
        assert_eq!(m.mix(&[10_000]), 0);
        m.master_q15 = Q15_ONE / 2;
        m.master_muted = false;
        assert_eq!(m.mix(&[10_000]), 5_000);
        for _ in 0..MAX_STREAMS {
            m.add_stream(Stream { name: "x", gain_q15: 0, muted: true });
        }
        assert_eq!(m.len(), MAX_STREAMS);
        assert!(!m.set_gain("nope", 0));
    }

    #[test]
    fn f280_resampler_ratios() {
        let up = Resampler::new(44_100, 48_000);
        assert!(up.step() < 65536);
        let down = Resampler::new(48_000, 44_100);
        assert!(down.step() > 65536);
        let mut r = Resampler::new(48_000, 48_000);
        let input = [0i16, 100, 200, 300, 400, 500, 600, 700];
        let mut out = [0i16; 4];
        assert_eq!(r.process(&input, &mut out), 4);
        assert_eq!(out[0], 0);
        let mut empty = Resampler::new(0, 48_000);
        let mut o2 = [0i16; 4];
        assert_eq!(empty.process(&input, &mut o2), 0);
    }

    #[test]
    fn f281_latency_classes() {
        let rt = LatencyBudget { period_frames: 32, periods: 2, rate_hz: 48_000 };
        assert_eq!(rt.buffer_us(), 1333);
        assert_eq!(latency_verdict(rt.buffer_us()), LatencyVerdict::Realtime);
        let media = LatencyBudget { period_frames: 512, periods: 4, rate_hz: 48_000 };
        assert_eq!(latency_verdict(media.buffer_us()), LatencyVerdict::Latent);
        assert_eq!(LatencyBudget { period_frames: 0, periods: 0, rate_hz: 0 }.buffer_us(), 0);
    }

    #[test]
    fn f282_underrun_counter() {
        let before = UnderrunReport::count();
        let mut g = UnderrunGuard { ring_frames: 64, watermark_frames: 128, consecutive: 0 };
        for _ in 0..3 {
            g.check(0);
        }
        assert_eq!(UnderrunReport::count(), before + 1);
        assert!(UnderrunReport::lost_frames() >= 128);
        g.check(200);
        assert_eq!(g.consecutive, 0);
    }

    #[test]
    fn f283_tone_controls_move_energy() {
        let mut full = EffectChain::new();
        let mut cut = EffectChain::new();
        cut.treble_db10 = -600;
        let mut a = 0i64;
        let mut b = 0i64;
        for i in 0..64 {
            let s = if i % 2 == 0 { 8000 } else { -8000 };
            a += full.process(s).abs() as i64;
            b += cut.process(s).abs() as i64;
        }
        assert!(a > b, "treble cut must lower high-frequency energy: {} vs {}", a, b);
    }

    #[test]
    fn f285_sine_accuracy() {
        assert_eq!(sin_q15(0), 0);
        let peak = sin_q15(1571);
        assert!((peak - 32768).abs() < 400, "peak {} off", peak);
        assert!(sin_q15(3141).abs() < 400);
        let neg = sin_q15(4712);
        assert!(neg < -32000, "third quadrant must be negative: {}", neg);
        assert_eq!(sin_q15(-6283), 0);
    }

    #[test]
    fn f286_pan_symmetry() {
        let left = Spatializer { azimuth_deci_deg: -900, max_itd_frames: 16 }.pan_gains();
        let right = Spatializer { azimuth_deci_deg: 900, max_itd_frames: 16 }.pan_gains();
        // Mirror symmetry: the two branches use different Taylor series, so
        // allow a rounding slack well below the audible threshold.
        assert!(left.0.abs_diff(right.1) < 64, "{} vs {}", left.0, right.1);
        assert!(left.1.abs_diff(right.0) < 64, "{} vs {}", left.1, right.0);
        let centre = Spatializer { azimuth_deci_deg: 0, max_itd_frames: 16 }.pan_gains();
        assert!(centre.0.abs_diff(centre.1) < 64, "centre {} vs {}", centre.0, centre.1);
        assert!(right.1 > right.0, "positive azimuth must favour the right ear");
        assert_eq!(Spatializer { azimuth_deci_deg: 0, max_itd_frames: 16 }.itd_frames(), 0);
    }

    #[test]
    fn f288_gate_hold_behaviour() {
        let mut g = NoiseGate::new(1000, 400, 300);
        assert_eq!(g.tick(2000, 100), GateState::Open);
        assert_eq!(g.tick(500, 100), GateState::Open);
        assert_eq!(g.tick(100, 100), GateState::Open);
        assert_eq!(g.tick(100, 100), GateState::Open);
        assert_eq!(g.tick(100, 100), GateState::Closed);
        assert_eq!(g.gain_q15(), 0);
    }

    #[test]
    fn f289_wav_header_fields() {
        let h = wav_header(0, 44_100, 1, 16);
        assert_eq!(u32::from_le_bytes([h[24], h[25], h[26], h[27]]), 44_100);
        assert_eq!(u16::from_le_bytes([h[32], h[33]]), 2);
        assert_eq!(u32::from_le_bytes([h[4], h[5], h[6], h[7]]), 36);
        assert_eq!(recordable_seconds(100, 48_000, 2, 16), 0);
    }

    #[test]
    fn f290_peak_hold_decays() {
        let mut s = Spectrum::new();
        s.update(&[0xFFFF; SPECTRUM_BINS]);
        assert_eq!(s.peak_hold[0], 0xFFFF);
        for _ in 0..64 {
            s.update(&[0; SPECTRUM_BINS]);
        }
        assert_eq!(s.bins[0], 0);
        assert!(s.peak_hold[0] < 0xFFFF);
    }

    #[test]
    fn f291_registry_falls_back_on_unplug() {
        let mut r = DeviceRegistry::new();
        assert_eq!(r.select_output(), None);
        r.upsert(AudioDevice { name: "hda", role: DeviceRole::Output, present: true, rate_hz: 48_000 });
        r.upsert(AudioDevice { name: "usb", role: DeviceRole::Output, present: false, rate_hz: 48_000 });
        assert_eq!(r.select_output(), Some("hda"));
        r.upsert(AudioDevice { name: "hda", role: DeviceRole::Output, present: false, rate_hz: 48_000 });
        r.upsert(AudioDevice { name: "usb", role: DeviceRole::Output, present: true, rate_hz: 48_000 });
        assert_eq!(r.select_output(), Some("usb"));
        assert_eq!(r.present_outputs(), 1);
    }

    #[test]
    fn f294_curve_round_trip() {
        for s in [0u8, 10, 25, 50, 75, 90, 100] {
            let g = slider_to_q15(s);
            let back = q15_to_slider(g);
            assert!((back as i32 - s as i32).abs() <= 3, "{} -> {} -> {}", s, g, back);
        }
        assert_eq!(slider_to_q15(200), Q15_ONE);
    }

    #[test]
    fn f295_quantum_bounds() {
        let m = FrameAwareMixer { frame_period_us: 16_667, rate_hz: 48_000, max_quantum_frames: 128 };
        assert_eq!(m.quantum_frames(), 128);
        let slow = FrameAwareMixer { frame_period_us: 100_000, rate_hz: 48_000, max_quantum_frames: 8192 };
        assert_eq!(slow.quantum_frames(), 4800);
        assert_eq!(slow.align(300_000), 2400);
    }

    #[test]
    fn f296_event_ring_wraps() {
        let mut l = AudioEventLog::new();
        for i in 0..AUDIO_EVENT_CAP + 4 {
            l.push(AudioEvent { kind: AudioEventKind::Mute, stamp: i as u64, arg: 0 });
        }
        assert_eq!(l.len(), AUDIO_EVENT_CAP);
        assert_eq!(l.get(0).unwrap().stamp, (AUDIO_EVENT_CAP + 3) as u64);
        assert!(l.get(AUDIO_EVENT_CAP).is_none());
    }

    #[test]
    fn f299_reference_excludes_disabled() {
        let d = [OutputDevice { name: "off", latency_us: 90_000, enabled: false }];
        assert_eq!(reference_latency(&d), 0);
        assert_eq!(reference_latency(&[]), 0);
        assert_eq!(compensation_frames(d[0], 0, 0), 0);
    }

    #[test]
    fn f300_self_test_passes() {
        let set = run_audio_checks();
        if !set.all_passed() {
            let mut buf = [0u8; 1024];
            let n = set.render(&mut buf);
            panic!("audio self-test:\n{}", core::str::from_utf8(&buf[..n]).unwrap());
        }
        assert!(set.len() >= 24);
    }
}
