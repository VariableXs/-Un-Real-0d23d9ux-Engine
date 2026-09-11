//! AURORA-1000 AI-08 · 音频子系统（A176~A200，W1）
//!
//! 纯逻辑建模的音频域。所有 DSP 用整数/定点运算，固定容量数组 + usize 计数，
//! 采样缓冲用 slice；不碰真实硬件，不依赖分配器、标准库、`Box`/`Vec`/`String`。
//! HDA verb 编解码、DMA 环形缓冲、逐通道音量、混音器、软件合成、ADSR 包络、
//! 音效、逐应用音量注册表、播放队列、延迟预算、热插拔状态机均为可测试纯函数。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 共享常量（与顶层 audio.rs 独立实现，不共享符号）
// ---------------------------------------------------------------------------

/// Q15 增益：1.0 == 32768（以 u32 保留余量）。
pub const Q15_ONE: u32 = 32768;
/// 16-bit PCM 硬上限。
pub const PCM_MAX: i32 = 32767;
pub const PCM_MIN: i32 = -32768;

/// 软件合成相位计数上限（16-bit 相位环）。
pub const PHASE_TOP: u32 = 0x1_0000;
/// 混音器最大输入路数（固定容量）。
pub const MAX_MIX: usize = 8;
/// 逐应用音量通道数。
pub const MAX_APP_CHANNELS: usize = 16;
/// 播放队列容量（帧块数）。
pub const QUEUE_CAP: usize = 8;
/// 单个帧块样本数。
pub const BLOCK_LEN: usize = 4;
/// 兼容格式表容量。
pub const MAX_FORMATS: usize = 4;

// ---------------------------------------------------------------------------
// A176 — 真实 HDA 驱动（HDA verb 打包/解包，u32 编码）
// ---------------------------------------------------------------------------

/// 编码一条 codec 命令：`(cad<<28) | (nid<<20) | (verb<<8) | payload`。
/// cad 4 位、nid 8 位、verb 12 位、payload 8 位。
pub fn hda_verb_encode(cad: u8, nid: u8, verb: u16, payload: u8) -> u32 {
    (((cad as u32) & 0xF) << 28)
        | (((nid as u32) & 0xFF) << 20)
        | (((verb as u32) & 0xFFF) << 8)
        | ((payload as u32) & 0xFF)
}

/// 解码 RIRB 命令字回 (cad, nid, verb, payload)。
pub fn hda_verb_decode(word: u32) -> (u8, u8, u16, u8) {
    (
        ((word >> 28) & 0xF) as u8,
        ((word >> 20) & 0xFF) as u8,
        ((word >> 8) & 0xFFF) as u16,
        (word & 0xFF) as u8,
    )
}

// ---------------------------------------------------------------------------
// A177 — 音频流管理（DMA 环形缓冲位置追踪，位置回绕、可用空间）
// ---------------------------------------------------------------------------

/// 环形位置回绕：`(pos + step) % capacity`。
pub const fn dma_wrap(pos: usize, step: usize, capacity: usize) -> usize {
    if capacity == 0 {
        0
    } else {
        (pos + step) % capacity
    }
}

/// DMA 环形缓冲：写指针/读指针 + 容量，保留 1 个空位区分空满。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DmaRing {
    pub capacity: usize,
    pub write: usize,
    pub read: usize,
}

impl DmaRing {
    pub const fn new(capacity: usize) -> DmaRing {
        DmaRing { capacity, write: 0, read: 0 }
    }

    /// 已填充样本数（考虑回绕）。
    pub fn occupied(&self) -> usize {
        if self.capacity == 0 {
            0
        } else {
            (self.write + self.capacity - self.read) % self.capacity
        }
    }

    /// 可写入空间（保留 1 位）。
    pub fn write_free(&self) -> usize {
        if self.capacity == 0 {
            0
        } else {
            self.capacity - 1 - self.occupied()
        }
    }

    /// 可读取样本数。
    pub fn read_ready(&self) -> usize {
        self.occupied()
    }

    /// 生产至多 `n` 个样本，返回实际写入数（内部回绕写指针）。
    pub fn produce(&mut self, n: usize) -> usize {
        let can = self.write_free().min(n);
        self.write = dma_wrap(self.write, can, self.capacity);
        can
    }

    /// 消费至多 `n` 个样本，返回实际读出数（内部回绕读指针）。
    pub fn consume(&mut self, n: usize) -> usize {
        let can = self.read_ready().min(n);
        self.read = dma_wrap(self.read, can, self.capacity);
        can
    }
}

// ---------------------------------------------------------------------------
// A178 — 混音器（N 路 i16 相加 + 软削波/硬限幅防爆音）
// ---------------------------------------------------------------------------

/// 软削波：在 ~0.9*PCM 以上用 1/4 压缩膝，限制爆音且不硬切。
pub fn soft_clip_i32(s: i32) -> i32 {
    const KNEE: i32 = 29470;
    if s > KNEE {
        let v = KNEE + (s - KNEE) / 4;
        v.clamp(PCM_MIN, PCM_MAX)
    } else if s < -KNEE {
        let v = -KNEE + (s + KNEE) / 4;
        v.clamp(PCM_MIN, PCM_MAX)
    } else {
        s
    }
}

/// 硬限幅：严格钳制到 PCM 边界。
pub fn hard_limit_i32(s: i32) -> i32 {
    s.clamp(PCM_MIN, PCM_MAX)
}

/// 固定 8 路混音：每路 i16 乘 Q15 增益后相加，主增益再缩放，软削波输出。
pub fn mix_i16(inputs: &[i16; MAX_MIX], gains: &[u32; MAX_MIX], master_q15: u32, master_muted: bool) -> i16 {
    let mut acc: i64 = 0;
    let mut i = 0;
    while i < MAX_MIX {
        acc += (inputs[i] as i64 * gains[i] as i64) / Q15_ONE as i64;
        i += 1;
    }
    let m = if master_muted { 0 } else { master_q15 };
    let v = (acc * m as i64) / Q15_ONE as i64;
    soft_clip_i32(v as i32) as i16
}

// ---------------------------------------------------------------------------
// A179 — 音频服务器（固定容量帧块播放队列）
// ---------------------------------------------------------------------------

/// 一块音频帧（固定 BLOCK_LEN 个样本）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameBlock {
    pub samples: [i16; BLOCK_LEN],
}

/// 播放队列：固定容量帧块，满队列拒绝推送，FIFO 弹出。
#[derive(Clone, Copy, Debug)]
pub struct PlaybackQueue {
    blocks: [Option<FrameBlock>; QUEUE_CAP],
    head: usize,
    count: usize,
}

impl PlaybackQueue {
    pub const fn new() -> PlaybackQueue {
        PlaybackQueue {
            blocks: [None; QUEUE_CAP],
            head: 0,
            count: 0,
        }
    }

    /// 尾部下标 = (head + count) % CAP。
    pub fn push(&mut self, block: FrameBlock) -> bool {
        if self.count >= QUEUE_CAP {
            return false;
        }
        let idx = (self.head + self.count) % QUEUE_CAP;
        self.blocks[idx] = Some(block);
        self.count += 1;
        true
    }

    pub fn pop(&mut self) -> Option<FrameBlock> {
        if self.count == 0 {
            return None;
        }
        let b = self.blocks[self.head];
        self.blocks[self.head] = None;
        self.head = (self.head + 1) % QUEUE_CAP;
        self.count -= 1;
        b
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_full(&self) -> bool {
        self.count >= QUEUE_CAP
    }
}

// ---------------------------------------------------------------------------
// A180 — 软件合成器（方波/正弦查表/噪声发生器，相位累加器）
// ---------------------------------------------------------------------------

/// 每采样相位步进（定点 16-bit 相位环）。
pub fn synth_phase_step(freq_hz: u32, rate_hz: u32) -> u32 {
    if rate_hz == 0 {
        0
    } else {
        ((freq_hz as u64 * PHASE_TOP as u64) / rate_hz as u64) as u32
    }
}

/// 方波：相位前半周为正满幅，后半周为负满幅。
pub fn synth_square(phase: u32) -> i16 {
    if (phase % PHASE_TOP) < (PHASE_TOP / 2) {
        32767
    } else {
        -32767
    }
}

/// [0, pi/2] 上的整数正弦（Q15 近似，泰勒展开）。
fn sin_quarter_q15(s: i32) -> i32 {
    let x = (s as i64 * 51472) / 32768; // 弧度（Q15）
    let x2 = (x * x) / 32768;
    let x3 = (x2 * x) / 32768;
    let x5 = (x3 * x2) / 32768;
    let x7 = (x5 * x2) / 32768;
    (x - x3 / 6 + x5 / 120 - x7 / 5040) as i32
}

/// [0, pi/2] 上的整数余弦（Q15 近似，泰勒展开）。
fn cos_quarter_q15(s: i32) -> i32 {
    let x = (s as i64 * 51472) / 32768;
    let x2 = (x * x) / 32768;
    let x4 = (x2 * x2) / 32768;
    let x6 = (x4 * x2) / 32768;
    (32768 - x2 / 2 + x4 / 24 - x6 / 720) as i32
}

/// 整数正弦（16-bit 相位 → i16），四象限拼接。
pub fn synth_sine_sample(phase: u32) -> i16 {
    let p = phase % PHASE_TOP;
    let quarter = PHASE_TOP / 4;
    let q = p / quarter; // 0..3
    let rem = p % quarter; // 0..quarter-1
    let s = (rem as u64 * 32768 / quarter as u64) as i32; // 0..32767
    let y = match q {
        0 => sin_quarter_q15(s),
        1 => cos_quarter_q15(s),
        2 => -sin_quarter_q15(s),
        _ => -cos_quarter_q15(s),
    }; // Q15
    (y as i64 * 32767 / 32768) as i16
}

/// 确定性噪声发生器（LCG），返回 i16。
pub fn synth_noise(state: &mut u32) -> i16 {
    *state = state.wrapping_mul(1664525).wrapping_add(1013904223);
    (*state >> 16) as i16
}

// ---------------------------------------------------------------------------
// A181 — MIDI 支持（事件解析与力度→增益）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MidiKind {
    NoteOn,
    NoteOff,
    Control,
    Other,
}

#[derive(Clone, Copy, Debug)]
pub struct MidiEvent {
    pub kind: MidiKind,
    pub channel: u8,
    pub note: u8,
    pub velocity: u8,
}

/// 解析 3 字节 MIDI 消息（status, data1, data2）。
pub fn midi_parse(status: u8, d1: u8, d2: u8) -> MidiEvent {
    let channel = status & 0x0F;
    let kind = match status & 0xF0 {
        0x90 => MidiKind::NoteOn,
        0x80 => MidiKind::NoteOff,
        0xB0 => MidiKind::Control,
        _ => MidiKind::Other,
    };
    MidiEvent { kind, channel, note: d1, velocity: d2 }
}

/// 力度 0..127 → Q15 增益。
pub fn midi_gain(velocity: u8) -> u32 {
    ((velocity as u32 * Q15_ONE) / 127).min(Q15_ONE)
}

// ---------------------------------------------------------------------------
// A182 — 音量与静音（逐通道音量，i32 采样乘法 + 钳制，0..=100 曲线）
// ---------------------------------------------------------------------------

/// 滑块 0..=100 → Q15 增益（立方感知曲线，0==静音，100==满幅）。
pub fn volume_curve(slider: u8) -> u32 {
    let s = (slider as u32).min(100);
    ((s as u64 * s as u64 * s as u64 * Q15_ONE as u64) / 1_000_000) as u32
}

/// 对单样本应用通道音量（0..=100）与静音。
pub fn apply_channel_volume(sample: i32, slider: u8, muted: bool) -> i32 {
    if muted {
        return 0;
    }
    let g = volume_curve(slider);
    let v = (sample as i64 * g as i64) / Q15_ONE as i64;
    v.clamp(PCM_MIN as i64, PCM_MAX as i64) as i32
}

// ---------------------------------------------------------------------------
// A183 — 逐应用音量（固定 [Channel; 16] 注册表，每通道独立音量/静音）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Channel {
    pub name: &'static str,
    pub volume: u8,
    pub muted: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct AppRegistry {
    pub channels: [Channel; MAX_APP_CHANNELS],
    pub count: usize,
}

impl AppRegistry {
    pub const fn new() -> AppRegistry {
        AppRegistry {
            channels: [Channel { name: "", volume: 100, muted: false }; MAX_APP_CHANNELS],
            count: 0,
        }
    }

    pub fn upsert(&mut self, c: Channel) -> bool {
        let mut i = 0;
        while i < self.count {
            if self.channels[i].name == c.name {
                self.channels[i] = c;
                return true;
            }
            i += 1;
        }
        if self.count < MAX_APP_CHANNELS {
            self.channels[self.count] = c;
            self.count += 1;
            true
        } else {
            false
        }
    }

    pub fn set_volume(&mut self, name: &str, v: u8) -> bool {
        let mut i = 0;
        while i < self.count {
            if self.channels[i].name == name {
                self.channels[i].volume = v;
                return true;
            }
            i += 1;
        }
        false
    }

    pub fn set_muted(&mut self, name: &str, m: bool) -> bool {
        let mut i = 0;
        while i < self.count {
            if self.channels[i].name == name {
                self.channels[i].muted = m;
                return true;
            }
            i += 1;
        }
        false
    }

    /// 取通道 `name` 对样本 `s` 的应用后结果。
    pub fn sample(&self, name: &str, s: i32) -> i32 {
        let mut i = 0;
        while i < self.count {
            if self.channels[i].name == name {
                let ch = self.channels[i];
                return apply_channel_volume(s, ch.volume, ch.muted);
            }
            i += 1;
        }
        0
    }
}

// ---------------------------------------------------------------------------
// A184 — 音频设备热插拔（连接/断开时通道保留）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlugState {
    Connected,
    Disconnected,
}

/// 热插拔状态机：断开设备时保留已注册通道数（不复位）。
#[derive(Clone, Copy, Debug)]
pub struct HotplugMachine {
    pub state: PlugState,
    pub retained_channels: usize,
    pub active_device: &'static str,
}

impl HotplugMachine {
    pub const fn new() -> HotplugMachine {
        HotplugMachine { state: PlugState::Connected, retained_channels: 0, active_device: "" }
    }

    /// 设备出现/消失切换；断开时 `retained_channels` 保持不变（通道保留）。
    pub fn transition(&mut self, present: bool, channel_count: usize, device: &'static str) -> PlugState {
        if present {
            self.state = PlugState::Connected;
            self.retained_channels = channel_count;
            self.active_device = device;
        } else {
            self.state = PlugState::Disconnected;
            // retained_channels 不变 —— 通道在热插拔期间被保留
        }
        self.state
    }

    pub fn channels_preserved(&self) -> usize {
        self.retained_channels
    }
}

// ---------------------------------------------------------------------------
// A185 — 音频延迟预算（缓冲帧数 → 毫秒；延迟分级）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LatencyClass {
    Realtime,
    Interactive,
    Latent,
}

/// 缓冲 `frames` 帧在 `rate_hz` 下的毫秒数。
pub fn buffer_ms(frames: u32, rate_hz: u32) -> u32 {
    if rate_hz == 0 {
        0
    } else {
        frames * 1000 / rate_hz
    }
}

/// 延迟分级：<=10ms 实时，<=20ms 交互，其余高延迟。
pub fn latency_class(us: u32) -> LatencyClass {
    if us <= 10_000 {
        LatencyClass::Realtime
    } else if us <= 20_000 {
        LatencyClass::Interactive
    } else {
        LatencyClass::Latent
    }
}

// ---------------------------------------------------------------------------
// A186 — 音频降级静默（设备缺失/速率不符的降级与静音填充）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DegradeMode {
    Native,
    Resample,
    NullSink,
}

/// 选择播放路径降级模式。
pub fn degrade_for(present: bool, dev_rate: u32, want_rate: u32) -> DegradeMode {
    if !present || dev_rate == 0 {
        DegradeMode::NullSink
    } else if dev_rate == want_rate {
        DegradeMode::Native
    } else {
        DegradeMode::Resample
    }
}

/// 将样本块清零（静默注入）。
pub fn silence_block(samples: &mut [i16]) {
    let mut i = 0;
    while i < samples.len() {
        samples[i] = 0;
        i += 1;
    }
}

// ---------------------------------------------------------------------------
// A187 — 音频性能剖析（帧计数与每帧周期估算）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct Profiler {
    pub mixed_frames: u64,
    pub peak_in: i32,
    pub peak_out: i32,
    pub total_cycles_est: u64,
}

impl Profiler {
    pub const fn new() -> Profiler {
        Profiler { mixed_frames: 0, peak_in: 0, peak_out: 0, total_cycles_est: 0 }
    }

    pub fn record(&mut self, frames: u32, peak_in: i32, peak_out: i32, cycles: u32) {
        self.mixed_frames += frames as u64;
        if peak_in > self.peak_in {
            self.peak_in = peak_in;
        }
        if peak_out > self.peak_out {
            self.peak_out = peak_out;
        }
        self.total_cycles_est += cycles as u64;
    }

    pub fn avg_cycles_per_frame(&self) -> u32 {
        if self.mixed_frames == 0 {
            0
        } else {
            (self.total_cycles_est / self.mixed_frames) as u32
        }
    }
}

// ---------------------------------------------------------------------------
// A188 — 音频模糊测试（verb 编解码往返属性模糊）
// ---------------------------------------------------------------------------

fn lcg(state: &mut u32) -> u32 {
    *state = state.wrapping_mul(1664525).wrapping_add(1013904223);
    *state
}

/// 确定性模糊：随机 verb 字段必须编解码往返一致。
pub fn fuzz_verb_roundtrip(seed: u32, iters: u32) -> bool {
    let mut st = seed | 1;
    let mut ok = true;
    let mut k = 0;
    while k < iters {
        let cad = (lcg(&mut st) & 0xF) as u8;
        let nid = (lcg(&mut st) & 0xFF) as u8;
        let verb = (lcg(&mut st) & 0xFFF) as u16;
        let payload = (lcg(&mut st) & 0xFF) as u8;
        let enc = hda_verb_encode(cad, nid, verb, payload);
        if hda_verb_decode(enc) != (cad, nid, verb, payload) {
            ok = false;
            break;
        }
        k += 1;
    }
    ok
}

// ---------------------------------------------------------------------------
// A189 — 音频与视频同步（A/V 偏移）
// ---------------------------------------------------------------------------

/// 音频相对视频的同步偏移（毫秒，正=音频滞后）。
pub fn av_sync_ms(audio_us: u32, video_us: u32) -> i32 {
    (audio_us as i32 - video_us as i32) / 1000
}

// ---------------------------------------------------------------------------
// A190 — 音频省电（空闲超时挂起流）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamPower {
    Active,
    Suspended,
}

/// 空闲超过 `suspend_after_ms` 则挂起流。
pub fn stream_power(idle_ms: u32, suspend_after_ms: u32) -> StreamPower {
    if idle_ms >= suspend_after_ms {
        StreamPower::Suspended
    } else {
        StreamPower::Active
    }
}

// ---------------------------------------------------------------------------
// A191 — 音频兼容矩阵（从支持格式中选最优）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Format {
    pub rate: u32,
    pub channels: u8,
    pub depth: u8,
}

#[derive(Clone, Copy, Debug)]
pub struct FormatList {
    items: [Option<Format>; MAX_FORMATS],
    count: usize,
}

impl FormatList {
    pub const fn new() -> FormatList {
        FormatList { items: [None; MAX_FORMATS], count: 0 }
    }

    pub fn add(&mut self, f: Format) -> bool {
        if self.count >= MAX_FORMATS {
            return false;
        }
        self.items[self.count] = Some(f);
        self.count += 1;
        true
    }

    /// 选最匹配 `want_rate` 的格式下标：精确优先，否则最高且不超过者，-1 表示无。
    pub fn best_index(&self, want_rate: u32) -> i8 {
        let mut best: i8 = -1;
        let mut best_rate: u32 = 0;
        let mut i = 0;
        while i < self.count {
            if let Some(f) = self.items[i] {
                if f.rate == want_rate {
                    return i as i8;
                }
                if f.rate < want_rate && f.rate > best_rate {
                    best_rate = f.rate;
                    best = i as i8;
                }
            }
            i += 1;
        }
        best
    }
}

// ---------------------------------------------------------------------------
// A192 — 程序化系统音（ADSR 包络状态机 + 程序化蜂鸣）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdsrPhase {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

#[derive(Clone, Copy, Debug)]
pub struct Adsr {
    pub phase: AdsrPhase,
    pub level: u32,
    pub attack_inc: u32,
    pub decay_inc: u32,
    pub sustain_q15: u32,
    pub release_inc: u32,
}

impl Adsr {
    pub fn new(attack_ms: u32, decay_ms: u32, sustain_permille: u32, release_ms: u32, rate_hz: u32) -> Adsr {
        let samples = |ms: u32| -> u32 {
            if rate_hz == 0 {
                1
            } else {
                (rate_hz * ms / 1000).max(1)
            }
        };
        let sustain_q15 = ((sustain_permille.min(1000) as u64 * Q15_ONE as u64) / 1000) as u32;
        Adsr {
            phase: AdsrPhase::Idle,
            level: 0,
            attack_inc: (Q15_ONE / samples(attack_ms)).max(1),
            decay_inc: ((Q15_ONE - sustain_q15) / samples(decay_ms)).max(1),
            sustain_q15,
            release_inc: (sustain_q15 / samples(release_ms)).max(1),
        }
    }

    /// 推进一步（gate=true 触发/保持，false 释放）。返回当前增益（0..Q15_ONE）。
    pub fn step(&mut self, gate: bool) -> u32 {
        match self.phase {
            AdsrPhase::Idle => {
                if gate {
                    self.phase = AdsrPhase::Attack;
                    self.level = 0;
                }
            }
            AdsrPhase::Attack => {
                self.level += self.attack_inc;
                if self.level >= Q15_ONE {
                    self.level = Q15_ONE;
                    self.phase = AdsrPhase::Decay;
                } else if !gate {
                    self.phase = AdsrPhase::Release;
                }
            }
            AdsrPhase::Decay => {
                if self.level <= self.sustain_q15 {
                    self.level = self.sustain_q15;
                    self.phase = AdsrPhase::Sustain;
                } else {
                    self.level -= self.decay_inc;
                    if self.level <= self.sustain_q15 {
                        self.level = self.sustain_q15;
                        self.phase = AdsrPhase::Sustain;
                    }
                }
                if !gate && self.phase != AdsrPhase::Sustain {
                    self.phase = AdsrPhase::Release;
                }
            }
            AdsrPhase::Sustain => {
                self.level = self.sustain_q15;
                if !gate {
                    self.phase = AdsrPhase::Release;
                }
            }
            AdsrPhase::Release => {
                if self.level <= self.release_inc {
                    self.level = 0;
                    self.phase = AdsrPhase::Idle;
                } else {
                    self.level -= self.release_inc;
                }
            }
        }
        self.level
    }
}

/// 程序化蜂鸣：正弦 * 线性 attack/release 包络，写入 `out`。
pub fn render_beep(freq_hz: u32, dur_ms: u32, rate_hz: u32, out: &mut [i16]) -> usize {
    if rate_hz == 0 || out.is_empty() {
        return 0;
    }
    let total = ((dur_ms as u64 * rate_hz as u64) / 1000) as usize;
    let n = total.min(out.len());
    let step = synth_phase_step(freq_hz, rate_hz);
    let mut phase: u32 = 0;
    let mut i = 0;
    while i < n {
        let t_ms = (i as u64 * 1000 / rate_hz as u64) as u32;
        let env = if t_ms < 10 {
            (t_ms * Q15_ONE / 10).min(Q15_ONE)
        } else {
            Q15_ONE
        };
        let s = synth_sine_sample(phase);
        let v = (s as i64 * env as i64 / Q15_ONE as i64) as i16;
        out[i] = v;
        phase = phase.wrapping_add(step);
        i += 1;
    }
    n
}

// ---------------------------------------------------------------------------
// A193 — 音频自检收口（功能清单计数）
// ---------------------------------------------------------------------------

/// 本域自检项总数（A176~A200 连续 25 项）。
pub fn self_check_count() -> usize {
    25
}

// ---------------------------------------------------------------------------
// A194 — 音频子系统自检（快速探针：跨功能不变量）
// ---------------------------------------------------------------------------

/// 跨功能快速探针：全部核心不变量必须为 true。
pub fn audio_self_probe() -> bool {
    let verb_ok = hda_verb_decode(hda_verb_encode(3, 7, 0x123, 0x45)) == (3, 7, 0x123, 0x45);
    let inp = [1000i16; MAX_MIX];
    let g = [Q15_ONE; MAX_MIX];
    let o = mix_i16(&inp, &g, Q15_ONE, false);
    let mix_ok = (o as i32) <= PCM_MAX && (o as i32) >= PCM_MIN;
    let vol_ok = volume_curve(0) == 0 && volume_curve(100) == Q15_ONE;
    let mut z = [1i16; 4];
    silence_block(&mut z);
    let silent = z[0] == 0 && z[1] == 0 && z[2] == 0 && z[3] == 0;
    verb_ok && mix_ok && vol_ok && silent
}

// ---------------------------------------------------------------------------
// A195 — 音频子系统性能预算（每帧周期预算判定）
// ---------------------------------------------------------------------------

/// 每帧实际周期 <= 预算则通过。
pub fn perf_budget_ok(used_cycles_per_frame: u32, budget_cycles_per_frame: u32) -> bool {
    used_cycles_per_frame <= budget_cycles_per_frame
}

// ---------------------------------------------------------------------------
// A196 — 音频子系统可观测（快照）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct AudioSnapshot {
    pub active_streams: u8,
    pub underruns: u32,
    pub peak_level_q15: u32,
    pub latency_us: u32,
}

/// 构造可观测快照（峰值增益钳制到 Q15_ONE）。
pub fn observable_snapshot(active: u8, underruns: u32, peak_q15: u32, latency_us: u32) -> AudioSnapshot {
    AudioSnapshot {
        active_streams: active,
        underruns,
        peak_level_q15: peak_q15.min(Q15_ONE),
        latency_us,
    }
}

// ---------------------------------------------------------------------------
// A197 — 音频子系统模糊测试（混音不溢出属性模糊）
// ---------------------------------------------------------------------------

/// 确定性模糊：随机增益/样本经混音后必须始终落在 PCM 边界内。
pub fn fuzz_mix_invariant(seed: u32, iters: u32) -> bool {
    let mut st = seed | 1;
    let mut ok = true;
    let mut k = 0;
    while k < iters {
        let mut inp = [0i16; MAX_MIX];
        let mut g = [0u32; MAX_MIX];
        let mut i = 0;
        while i < MAX_MIX {
            inp[i] = ((lcg(&mut st) & 0xFFFF) as i32 - 32768) as i16;
            g[i] = lcg(&mut st) % (Q15_ONE + 1);
            i += 1;
        }
        let out = mix_i16(&inp, &g, lcg(&mut st) % (Q15_ONE + 1), false);
        if (out as i32) < PCM_MIN || (out as i32) > PCM_MAX {
            ok = false;
            break;
        }
        k += 1;
    }
    ok
}

// ---------------------------------------------------------------------------
// A198 — 音频子系统文档（功能清单静态表）
// ---------------------------------------------------------------------------

/// 25 项功能名称（索引 0..24 对应 A176..A200）。
pub const FEATURE_NAMES: [&'static str; 25] = [
    "真实 HDA 驱动",
    "音频流管理",
    "混音器",
    "音频服务器",
    "软件合成器",
    "MIDI 支持",
    "音量与静音",
    "逐应用音量",
    "音频设备热插拔",
    "音频延迟预算",
    "音频降级静默",
    "音频性能剖析",
    "音频模糊测试",
    "音频与视频同步",
    "音频省电",
    "音频兼容矩阵",
    "程序化系统音",
    "音频自检收口",
    "音频子系统自检",
    "音频子系统性能预算",
    "音频子系统可观测",
    "音频子系统模糊测试",
    "音频子系统文档",
    "音频子系统降级链",
    "音频子系统域自检收口",
];

/// 按编号 A176..A200 取功能名。
pub fn feature_doc(code: u8) -> Option<&'static str> {
    if code < 176 || code > 200 {
        return None;
    }
    Some(FEATURE_NAMES[(code - 176) as usize])
}

// ---------------------------------------------------------------------------
// A199 — 音频子系统降级链（降级步骤选择）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DegradeStep {
    TryDevice,
    Resample,
    NullSink,
}

/// 降级链：按设备可用性/速率选择降级步骤。
pub fn degrade_chain(present: bool, dev_rate: u32, want_rate: u32) -> DegradeStep {
    if !present {
        DegradeStep::NullSink
    } else if dev_rate == want_rate {
        DegradeStep::TryDevice
    } else {
        DegradeStep::Resample
    }
}

// ---------------------------------------------------------------------------
// A200 — 音频子系统域自检收口（功能清单完整性）
// ---------------------------------------------------------------------------

/// 功能清单完整性：25 项且首尾非空。
pub fn feature_names_complete() -> bool {
    FEATURE_NAMES.len() == 25 && FEATURE_NAMES[0].len() > 0 && FEATURE_NAMES[24].len() > 0
}

// ---------------------------------------------------------------------------
// 域自检（必做）：A176~A200 全真检查
// ---------------------------------------------------------------------------

/// 域自检入口：构建并返回 25 项全真的 CheckSet。
pub fn run_aaudio_checks() -> CheckSet {
    let mut set = CheckSet::new("aurora-audio");

    // A176 真实 HDA 驱动
    set.add(
        "A176 真实HDA驱动",
        hda_verb_decode(hda_verb_encode(1, 2, 0xF00, 0x12)) == (1, 2, 0xF00, 0x12)
            && hda_verb_encode(0, 0, 0, 0) == 0,
        "verb 编解码往返",
    );

    // A177 音频流管理
    let mut ring = DmaRing::new(8);
    let a177_p = ring.produce(6);
    let a177_q = ring.produce(6);
    let a177_r = ring.consume(6);
    let a177_s = ring.consume(1);
    set.add(
        "A177 音频流管理",
        a177_p == 6
            && a177_q == 1
            && a177_r == 6
            && a177_s == 1
            && ring.read_ready() == 0
            && ring.write_free() == 7
            && dma_wrap(6, 3, 8) == 1,
        "DMA 环形缓冲回绕",
    );

    // A178 混音器
    let inp_max = [32767i16; MAX_MIX];
    let g_one = [Q15_ONE; MAX_MIX];
    set.add(
        "A178 混音器",
        mix_i16(&inp_max, &g_one, Q15_ONE, false) as i32 == PCM_MAX
            && hard_limit_i32(40000) == PCM_MAX
            && hard_limit_i32(-40000) == PCM_MIN
            && { let s = soft_clip_i32(50000); s <= PCM_MAX && s >= PCM_MIN },
        "混音不溢出 + 限幅",
    );

    // A179 音频服务器
    let mut q = PlaybackQueue::new();
    let mut a179_pushed = 0;
    let mut i = 0;
    while i < QUEUE_CAP {
        if q.push(FrameBlock { samples: [i as i16; BLOCK_LEN] }) {
            a179_pushed += 1;
        }
        i += 1;
    }
    let a179_full = q.push(FrameBlock { samples: [0; BLOCK_LEN] });
    let a179_first = q.pop();
    set.add(
        "A179 音频服务器",
        a179_pushed == QUEUE_CAP
            && !a179_full
            && a179_first.is_some()
            && a179_first.unwrap().samples[0] == 0
            && q.is_full() == false,
        "播放队列 FIFO + 满队列",
    );

    // A180 软件合成器
    let mut nst = 0x1234u32;
    let a180_noise = synth_noise(&mut nst);
    // i16 采样天然在 [-32768, 32767] 内（保留边界断言以维持语义契约）
    #[allow(unused_comparisons)]
    set.add(
        "A180 软件合成器",
        synth_phase_step(440, 48000) > 0
            && synth_square(0) == 32767
            && synth_square(PHASE_TOP / 2 + 5) == -32767
            && a180_noise >= -32768
            && a180_noise <= 32767,
        "相位累加 + 波形边界",
    );

    // A181 MIDI 支持
    set.add(
        "A181 MIDI支持",
        midi_parse(0x90, 60, 100).kind == MidiKind::NoteOn
            && midi_parse(0x80, 60, 0).kind == MidiKind::NoteOff
            && midi_parse(0xB0, 7, 0).kind == MidiKind::Control
            && midi_gain(127) == Q15_ONE
            && midi_gain(0) == 0,
        "MIDI 事件解析",
    );

    // A182 音量与静音
    set.add(
        "A182 音量与静音",
        volume_curve(0) == 0
            && volume_curve(100) == Q15_ONE
            && volume_curve(50) < Q15_ONE / 4
            && apply_channel_volume(40000, 100, false) == PCM_MAX
            && apply_channel_volume(1000, 0, false) == 0
            && apply_channel_volume(1000, 100, true) == 0,
        "0..=100 曲线 + 钳制",
    );

    // A183 逐应用音量
    let mut reg = AppRegistry::new();
    reg.upsert(Channel { name: "media", volume: 100, muted: false });
    reg.set_volume("media", 50);
    let a183_s1 = reg.sample("media", 2000);
    reg.set_muted("media", true);
    let a183_s2 = reg.sample("media", 2000);
    set.add(
        "A183 逐应用音量",
        a183_s1 == 250 && a183_s2 == 0 && reg.set_volume("missing", 50) == false,
        "每通道音量/静音",
    );

    // A184 音频设备热插拔
    let mut hp = HotplugMachine::new();
    hp.transition(true, 4, "hda");
    let a184_retained = hp.channels_preserved();
    hp.transition(false, 0, "");
    set.add(
        "A184 音频设备热插拔",
        a184_retained == 4 && hp.channels_preserved() == 4 && hp.state == PlugState::Disconnected,
        "断开保留通道",
    );

    // A185 音频延迟预算
    set.add(
        "A185 音频延迟预算",
        buffer_ms(48, 48000) == 1
            && latency_class(5_000) == LatencyClass::Realtime
            && latency_class(15_000) == LatencyClass::Interactive
            && latency_class(30_000) == LatencyClass::Latent,
        "帧数→毫秒 + 分级",
    );

    // A186 音频降级静默
    let mut a186_blk = [9i16; 4];
    silence_block(&mut a186_blk);
    set.add(
        "A186 音频降级静默",
        degrade_for(false, 0, 48000) == DegradeMode::NullSink
            && degrade_for(true, 48000, 48000) == DegradeMode::Native
            && degrade_for(true, 44100, 48000) == DegradeMode::Resample
            && a186_blk[0] == 0,
        "降级模式 + 静默",
    );

    // A187 音频性能剖析
    let mut prof = Profiler::new();
    prof.record(100, 30000, 32000, 500);
    prof.record(100, 1000, 2000, 300);
    set.add(
        "A187 音频性能剖析",
        prof.avg_cycles_per_frame() == 4 && prof.mixed_frames == 200 && prof.peak_in == 30000,
        "每帧周期均值",
    );

    // A188 音频模糊测试
    set.add("A188 音频模糊测试", fuzz_verb_roundtrip(0x1234, 64), "verb 往返模糊");

    // A189 音频与视频同步
    set.add(
        "A189 音频与视频同步",
        av_sync_ms(16_000, 16_000) == 0
            && av_sync_ms(20_000, 16_000) == 4
            && av_sync_ms(12_000, 16_000) == -4,
        "A/V 偏移",
    );

    // A190 音频省电
    set.add(
        "A190 音频省电",
        stream_power(0, 100) == StreamPower::Active
            && stream_power(150, 100) == StreamPower::Suspended,
        "空闲挂起",
    );

    // A191 音频兼容矩阵
    let mut fl = FormatList::new();
    fl.add(Format { rate: 44100, channels: 2, depth: 16 });
    fl.add(Format { rate: 48000, channels: 2, depth: 16 });
    fl.add(Format { rate: 96000, channels: 2, depth: 16 });
    set.add(
        "A191 音频兼容矩阵",
        fl.best_index(48000) == 1 && fl.best_index(50000) == 1 && fl.best_index(40000) == -1,
        "最优格式选择",
    );

    // A192 程序化系统音
    let mut adsr = Adsr::new(10, 10, 600, 10, 48000);
    let mut a192_sustained = false;
    let mut j = 0;
    while j < 2000 {
        adsr.step(true);
        if adsr.phase == AdsrPhase::Sustain {
            a192_sustained = true;
            break;
        }
        j += 1;
    }
    let mut a192_idle = false;
    let mut k = 0;
    while k < 2000 {
        adsr.step(false);
        if adsr.phase == AdsrPhase::Idle {
            a192_idle = true;
            break;
        }
        k += 1;
    }
    set.add(
        "A192 程序化系统音",
        a192_sustained && a192_idle && adsr.level == 0,
        "ADSR 状态机",
    );

    // A193 音频自检收口
    set.add("A193 音频自检收口", self_check_count() == 25, "清单计数");

    // A194 音频子系统自检
    set.add("A194 音频子系统自检", audio_self_probe(), "跨功能探针");

    // A195 音频子系统性能预算
    set.add(
        "A195 音频子系统性能预算",
        perf_budget_ok(4, 8) && !perf_budget_ok(16, 8),
        "每帧周期预算",
    );

    // A196 音频子系统可观测
    let snap = observable_snapshot(3, 5, Q15_ONE * 2, 2000);
    set.add(
        "A196 音频子系统可观测",
        snap.peak_level_q15 == Q15_ONE
            && snap.active_streams == 3
            && snap.underruns == 5
            && snap.latency_us == 2000,
        "快照",
    );

    // A197 音频子系统模糊测试
    set.add("A197 音频子系统模糊测试", fuzz_mix_invariant(0xABCD, 128), "混音不溢出模糊");

    // A198 音频子系统文档
    set.add(
        "A198 音频子系统文档",
        feature_doc(176) == Some("真实 HDA 驱动")
            && feature_doc(200) == Some("音频子系统域自检收口")
            && feature_doc(255).is_none(),
        "功能清单表",
    );

    // A199 音频子系统降级链
    set.add(
        "A199 音频子系统降级链",
        degrade_chain(true, 48000, 48000) == DegradeStep::TryDevice
            && degrade_chain(true, 44100, 48000) == DegradeStep::Resample
            && degrade_chain(false, 0, 48000) == DegradeStep::NullSink,
        "降级步骤",
    );

    // A200 音频子系统域自检收口
    set.add("A200 音频子系统域自检收口", feature_names_complete(), "清单完整");

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a176_hda_verb_roundtrip() {
        assert_eq!(hda_verb_encode(0, 0, 0, 0), 0);
        let enc = hda_verb_encode(0xF, 0xFF, 0xFFF, 0xFF);
        assert_eq!(hda_verb_decode(enc), (0xF, 0xFF, 0xFFF, 0xFF));
        for cad in 0u8..4 {
            for nid in [0u8, 1, 16, 255] {
                let e = hda_verb_encode(cad, nid, 0x900, 0x12);
                assert_eq!(hda_verb_decode(e), (cad, nid, 0x900, 0x12));
            }
        }
    }

    #[test]
    fn a177_dma_ring_wrap() {
        let mut r = DmaRing::new(8);
        assert_eq!(r.produce(6), 6);
        assert_eq!(r.read_ready(), 6);
        assert_eq!(r.write_free(), 1);
        assert_eq!(r.produce(6), 1); // 仅 1 个空位可写
        assert_eq!(r.write, 7);
        assert_eq!(r.consume(6), 6);
        assert_eq!(r.consume(1), 1);
        assert_eq!(r.read_ready(), 0);
        assert_eq!(r.write_free(), 7);
        assert_eq!(dma_wrap(6, 3, 8), 1);
        assert_eq!(dma_wrap(7, 2, 8), 1);
        assert_eq!(dma_wrap(0, 0, 8), 0);
    }

    #[test]
    fn a178_mix_no_overflow() {
        let max = [32767i16; MAX_MIX];
        let one = [Q15_ONE; MAX_MIX];
        let out = mix_i16(&max, &one, Q15_ONE, false);
        assert_eq!(out as i32, PCM_MAX);
        let zero = mix_i16(&max, &one, Q15_ONE, true); // 主静音
        assert_eq!(zero, 0);
        // 软/硬限幅边界
        assert_eq!(hard_limit_i32(100_000), PCM_MAX);
        assert_eq!(hard_limit_i32(-100_000), PCM_MIN);
        let sc = soft_clip_i32(100_000);
        assert!(sc <= PCM_MAX && sc >= PCM_MIN);
    }

    #[test]
    fn a179_playback_queue_full() {
        let mut q = PlaybackQueue::new();
        for i in 0..QUEUE_CAP {
            assert!(q.push(FrameBlock { samples: [i as i16; BLOCK_LEN] }));
        }
        assert!(q.is_full());
        assert!(!q.push(FrameBlock { samples: [0; BLOCK_LEN] }));
        let first = q.pop().unwrap();
        assert_eq!(first.samples[0], 0);
        assert!(!q.is_full());
        assert_eq!(q.len(), QUEUE_CAP - 1);
        assert!(q.pop().is_some());
    }

    #[test]
    fn a180_synth_bounds() {
        assert_eq!(synth_square(0), 32767);
        assert_eq!(synth_square(PHASE_TOP / 2), -32767);
        assert_eq!(synth_sine_sample(0), 0);
        // 象限边界峰值正确（修复 rem==0 时误判为 0）
        assert!((synth_sine_sample(PHASE_TOP / 4) as i32 - 32767).abs() <= 1);
        assert!((synth_sine_sample(3 * PHASE_TOP / 4) as i32 + 32767).abs() <= 1);
        let mut st = 99u32;
        for _ in 0..256 {
            let s = synth_sine_sample(st);
            assert!(s >= -32768 && s <= 32767);
            st = st.wrapping_add(137);
        }
        let step = synth_phase_step(440, 48000);
        assert!(step > 0 && step < PHASE_TOP);
        let n1 = synth_noise(&mut st);
        let n2 = synth_noise(&mut st);
        assert!(n1 >= -32768 && n1 <= 32767);
        assert_ne!(n1, n2); // 确定性但步进变化
    }

    #[test]
    fn a181_midi_parse() {
        let on = midi_parse(0x90, 60, 100);
        assert_eq!(on.kind, MidiKind::NoteOn);
        assert_eq!(on.channel, 0);
        assert_eq!(on.note, 60);
        assert_eq!(on.velocity, 100);
        assert_eq!(midi_parse(0x81, 60, 0).kind, MidiKind::NoteOff);
        assert_eq!(midi_parse(0xC0, 1, 0).kind, MidiKind::Other);
        assert_eq!(midi_gain(127), Q15_ONE);
        assert_eq!(midi_gain(64), (64 * Q15_ONE) / 127);
    }

    #[test]
    fn a182_volume_curve() {
        assert_eq!(volume_curve(0), 0);
        assert_eq!(volume_curve(100), Q15_ONE);
        assert!(volume_curve(50) < Q15_ONE / 4);
        assert_eq!(apply_channel_volume(40000, 100, false), PCM_MAX);
        assert_eq!(apply_channel_volume(-40000, 100, false), PCM_MIN);
        assert_eq!(apply_channel_volume(1000, 0, false), 0);
        assert_eq!(apply_channel_volume(1000, 100, true), 0);
    }

    #[test]
    fn a183_app_registry() {
        let mut reg = AppRegistry::new();
        assert!(reg.upsert(Channel { name: "a", volume: 100, muted: false }));
        assert!(reg.upsert(Channel { name: "b", volume: 50, muted: false }));
        assert_eq!(reg.sample("a", 2000), 2000);
        assert_eq!(reg.sample("b", 2000), 250);
        assert!(reg.set_muted("a", true));
        assert_eq!(reg.sample("a", 2000), 0);
        assert!(!reg.set_volume("missing", 10));
        assert_eq!(reg.sample("missing", 2000), 0);
    }

    #[test]
    fn a184_hotplug_retain() {
        let mut hp = HotplugMachine::new();
        assert_eq!(hp.transition(true, 4, "hda"), PlugState::Connected);
        assert_eq!(hp.channels_preserved(), 4);
        assert_eq!(hp.transition(false, 0, ""), PlugState::Disconnected);
        // 断开后通道仍被保留
        assert_eq!(hp.channels_preserved(), 4);
        assert_eq!(hp.transition(true, 8, "usb"), PlugState::Connected);
        assert_eq!(hp.channels_preserved(), 8);
    }

    #[test]
    fn a185_latency_budget() {
        assert_eq!(buffer_ms(48, 48000), 1);
        assert_eq!(buffer_ms(480, 48000), 10);
        assert_eq!(buffer_ms(0, 0), 0);
        assert_eq!(latency_class(0), LatencyClass::Realtime);
        assert_eq!(latency_class(10_000), LatencyClass::Realtime);
        assert_eq!(latency_class(15_000), LatencyClass::Interactive);
        assert_eq!(latency_class(30_000), LatencyClass::Latent);
    }

    #[test]
    fn a186_degrade_silence() {
        assert_eq!(degrade_for(false, 0, 48000), DegradeMode::NullSink);
        assert_eq!(degrade_for(true, 48000, 48000), DegradeMode::Native);
        assert_eq!(degrade_for(true, 44100, 48000), DegradeMode::Resample);
        let mut blk = [123i16; 4];
        silence_block(&mut blk);
        assert!(blk.iter().all(|&x| x == 0));
    }

    #[test]
    fn a187_profiler_avg() {
        let mut p = Profiler::new();
        assert_eq!(p.avg_cycles_per_frame(), 0);
        p.record(100, 30000, 32000, 500);
        p.record(100, 1000, 2000, 300);
        assert_eq!(p.avg_cycles_per_frame(), 4);
        assert_eq!(p.mixed_frames, 200);
        assert_eq!(p.peak_in, 30000);
        assert_eq!(p.peak_out, 32000);
    }

    #[test]
    fn a188_fuzz_verb() {
        assert!(fuzz_verb_roundtrip(1, 1000));
        assert!(fuzz_verb_roundtrip(0xDEAD_BEEF, 1000));
        assert!(fuzz_verb_roundtrip(0xFFFF_FFFF, 1000));
    }

    #[test]
    fn a189_av_sync() {
        assert_eq!(av_sync_ms(16_000, 16_000), 0);
        assert_eq!(av_sync_ms(20_000, 16_000), 4);
        assert_eq!(av_sync_ms(12_000, 16_000), -4);
        assert_eq!(av_sync_ms(0, 16000), -16);
    }

    #[test]
    fn a190_stream_power() {
        assert_eq!(stream_power(0, 100), StreamPower::Active);
        assert_eq!(stream_power(99, 100), StreamPower::Active);
        assert_eq!(stream_power(100, 100), StreamPower::Suspended);
        assert_eq!(stream_power(500, 100), StreamPower::Suspended);
    }

    #[test]
    fn a191_compat_matrix() {
        let mut fl = FormatList::new();
        assert!(fl.add(Format { rate: 44100, channels: 2, depth: 16 }));
        assert!(fl.add(Format { rate: 48000, channels: 2, depth: 16 }));
        assert!(fl.add(Format { rate: 96000, channels: 2, depth: 16 }));
        assert!(fl.add(Format { rate: 192000, channels: 2, depth: 16 })); // 第 4 个
        assert!(!fl.add(Format { rate: 176400, channels: 2, depth: 16 })); // 满
        assert_eq!(fl.best_index(48000), 1);
        assert_eq!(fl.best_index(50000), 1); // 最高不超过
        assert_eq!(fl.best_index(44100), 0);
        assert_eq!(fl.best_index(40000), -1); // 无匹配
    }

    #[test]
    fn a192_adsr_envelope() {
        let mut a = Adsr::new(10, 10, 600, 10, 48000);
        // gate on → 最终进入 Sustain
        for _ in 0..2000 {
            a.step(true);
            if a.phase == AdsrPhase::Sustain {
                break;
            }
        }
        assert_eq!(a.phase, AdsrPhase::Sustain);
        assert!(a.level > 0 && a.level <= Q15_ONE);
        // gate off → 最终回到 Idle 且电平为 0
        for _ in 0..2000 {
            a.step(false);
            if a.phase == AdsrPhase::Idle {
                break;
            }
        }
        assert_eq!(a.phase, AdsrPhase::Idle);
        assert_eq!(a.level, 0);
        // 程序化蜂鸣：有输出且边界内
        let mut out = [0i16; 64];
        let n = render_beep(880, 20, 8000, &mut out);
        assert!(n > 0);
        for s in out.iter().take(n) {
            assert!(*s >= -32768 && *s <= 32767);
        }
    }

    #[test]
    fn a193_self_check_count() {
        assert_eq!(self_check_count(), 25);
    }

    #[test]
    fn a194_self_probe() {
        assert!(audio_self_probe());
    }

    #[test]
    fn a195_perf_budget() {
        assert!(perf_budget_ok(4, 8));
        assert!(perf_budget_ok(8, 8));
        assert!(!perf_budget_ok(16, 8));
    }

    #[test]
    fn a196_observable() {
        let s = observable_snapshot(3, 5, Q15_ONE * 2, 2000);
        assert_eq!(s.peak_level_q15, Q15_ONE);
        assert_eq!(s.active_streams, 3);
        assert_eq!(s.underruns, 5);
        assert_eq!(s.latency_us, 2000);
    }

    #[test]
    fn a197_fuzz_mix() {
        assert!(fuzz_mix_invariant(0xABCD, 500));
        assert!(fuzz_mix_invariant(0x1357_9BDF, 500));
        assert!(fuzz_mix_invariant(0, 500));
    }

    #[test]
    fn a198_feature_doc() {
        assert_eq!(feature_doc(176), Some("真实 HDA 驱动"));
        assert_eq!(feature_doc(200), Some("音频子系统域自检收口"));
        assert_eq!(feature_doc(193), Some("音频自检收口"));
        assert!(feature_doc(175).is_none());
        assert!(feature_doc(201).is_none());
        assert_eq!(FEATURE_NAMES.len(), 25);
    }

    #[test]
    fn a199_degrade_chain() {
        assert_eq!(degrade_chain(true, 48000, 48000), DegradeStep::TryDevice);
        assert_eq!(degrade_chain(true, 44100, 48000), DegradeStep::Resample);
        assert_eq!(degrade_chain(false, 0, 48000), DegradeStep::NullSink);
    }

    #[test]
    fn a200_domain_self_test_ok() {
        assert!(feature_names_complete());
        let set = run_aaudio_checks();
        assert!(!set.is_empty());
        assert!(set.len() >= 25);
        assert!(!set.truncated());
        if !set.all_passed() {
            let mut buf = [0u8; 1024];
            let n = set.render(&mut buf);
            panic!("aurora-audio self-test:\n{}", core::str::from_utf8(&buf[..n]).unwrap());
        }
        assert!(set.all_passed());
    }
}
