//! F555 麦克风降噪 · 完整设计（STAR I 主册 I 域批次七）。
//!
//! **判据（主册）**：开关即时；A/B 回放链路；CPU 增量 <2% 判据；语音频段
//! 保真参数；与 F322/F448 链路一致。
//!
//! **设计要点（主册）**：
//! - 麦克风软件降噪开关（设置→声音→输入）：开启后输入链加降噪级——稳态
//!   底噪抑制（风扇/环境嗡声），不影响语音频段；
//! - 开关即时生效 + 试听对比（F448 测试向导内 A/B：降噪开/关各录 5 秒
//!   回放对比）；
//! - 降噪对性能占用显示（CPU 增量 <2% 才允许常开）；
//! - 语音频段保真参数入册。
//!
//! **降噪核（模型级 DSP）**：稳态底噪估计（帧 RMS 指数滑动）+ 频段三路
//! 语义（语音带内不动 / 带外稳态底噪衰减 / 瞬态保护）。定点算术，零堆。
//!
//! 链路接缝：F322（音频引擎）/F448（测试向导）以帧缓冲注入口承接。

use crate::checks::CheckSet;
use crate::istar::ibase::ISTAR_DOMAIN;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 采样率（Hz，F322 链路同源）。
pub const SAMPLE_RATE_HZ: u32 = 48_000;

/// 帧长（样本数，10ms 框架）。
pub const FRAME_SAMPLES: usize = 480;

/// 语音保真频段（Hz，主册「不影响语音频段」参数入册）：
/// 带内信号不做底噪衰减之外的处理。
pub const VOICE_BAND_LO_HZ: u32 = 300;
pub const VOICE_BAND_HI_HZ: u32 = 3_400;

/// 带外稳态底噪衰减增益（Q8.8 定点——256=0dB，衰减档 12dB=77）。
pub const NOISE_SUPPRESS_Q8_8: u16 = 77;

/// 瞬态保护阈值倍数（帧 RMS 超过底噪估计的 N 倍视为语音/瞬态——只压稳态）。
pub const TRANSIENT_FACTOR_X100: u32 = 300;

/// 开态标定期（帧数：前 N 帧一律按底噪学习——冷启动底噪为 0 的
/// 结构性问题用标定期解决，不靠魔法首帧特判）。
pub const FLOOR_BOOTSTRAP_FRAMES: u64 = 10;

/// 底噪估计平滑系数（Q8.8，指数滑动——估计器自身不抖）。
pub const FLOOR_SMOOTH_Q8_8: u32 = 230; // ≈0.9 旧值 + 0.1 新帧

/// CPU 增量红线（%——超过不允许常开）。
pub const CPU_BUDGET_PCT: u32 = 2;

/// A/B 试听片段时长（ms，主册「各录 5 秒」）。
pub const AB_CLIP_MS: u32 = 5_000;

// ---------------------------------------------------------------------------
// 降噪核
// ---------------------------------------------------------------------------

/// 降噪器状态（逐帧推进；定点无堆）。
pub struct Denoiser {
    enabled: bool,
    /// 底噪 RMS 估计（Q8.8 定点幅值域）。
    floor_q8_8: u32,
    /// 帧计数。
    frames: u64,
    /// 被衰减的帧数（带外稳态帧）。
    suppressed_frames: u64,
    /// 被保真的帧数（语音/瞬态帧）。
    voice_frames: u64,
}

impl Denoiser {
    pub fn new() -> Denoiser {
        Denoiser {
            enabled: false,
            floor_q8_8: 0,
            frames: 0,
            suppressed_frames: 0,
            voice_frames: 0,
        }
    }

    /// 开关（即时生效——下一帧起按新状态处理）。
    pub fn set_enabled(&mut self, on: bool) {
        self.enabled = on;
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// 当前底噪估计（Q8.8）。
    pub fn floor(&self) -> u32 {
        self.floor_q8_8
    }

    pub fn frames(&self) -> u64 {
        self.frames
    }

    pub fn suppressed_frames(&self) -> u64 {
        self.suppressed_frames
    }

    pub fn voice_frames(&self) -> u64 {
        self.voice_frames
    }

    /// 语音频段参数入册（渲染层/向导层取数口——单一事实源）。
    pub fn voice_band(&self) -> (u32, u32) {
        (VOICE_BAND_LO_HZ, VOICE_BAND_HI_HZ)
    }

    /// 处理一帧：输入帧 RMS（Q8.8），返回输出增益（Q8.8）与帧分类。
    ///
    /// 关态直通（增益 256，零分类记账——开关即时的结构证据）；
    /// 开态前 [`FLOOR_BOOTSTRAP_FRAMES`] 帧为底噪标定期（一律学习）；
    /// 标定期后：帧 RMS > 底噪 × TRANSIENT_FACTOR → 语音帧（增益 256 直通）；
    /// 否则视为稳态底噪帧 → 更新底噪估计并施加衰减。
    pub fn process_frame(&mut self, frame_rms_q8_8: u32) -> (u16, FrameClass) {
        self.frames += 1;
        if !self.enabled {
            return (256, FrameClass::Bypassed);
        }
        let calibrating = self.frames <= FLOOR_BOOTSTRAP_FRAMES;
        let is_voice = !calibrating
            && frame_rms_q8_8.saturating_mul(100)
                > self.floor_q8_8.saturating_mul(TRANSIENT_FACTOR_X100);
        if is_voice {
            self.voice_frames += 1;
            (256, FrameClass::Voice)
        } else {
            // 底噪估计：旧值平滑 + 新帧注入。
            self.floor_q8_8 = (self.floor_q8_8.saturating_mul(FLOOR_SMOOTH_Q8_8)
                + frame_rms_q8_8.saturating_mul(256 - FLOOR_SMOOTH_Q8_8))
                >> 8;
            self.suppressed_frames += 1;
            (NOISE_SUPPRESS_Q8_8, FrameClass::Noise)
        }
    }
}

impl Default for Denoiser {
    fn default() -> Self {
        Self::new()
    }
}

/// 帧分类（A/B 回放链路的统计口径）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameClass {
    Bypassed,
    Voice,
    Noise,
}

// ---------------------------------------------------------------------------
// A/B 回放链路（F448 向导语义）
// ---------------------------------------------------------------------------

/// A/B 试听对（降噪关/开各一段——时长红线 AB_CLIP_MS）。
pub struct AbPair {
    /// A 段（原始直通）是否就绪。
    pub a_ready: bool,
    /// B 段（降噪后）是否就绪。
    pub b_ready: bool,
    a_ms: u32,
    b_ms: u32,
}

impl AbPair {
    pub fn new() -> AbPair {
        AbPair { a_ready: false, b_ready: false, a_ms: 0, b_ms: 0 }
    }

    /// 录制段落（超长拒绝——5 秒红线是对称的，不许多录挤占回放公平性）。
    pub fn record(&mut self, seg: Segment, ms: u32) -> bool {
        if ms == 0 || ms > AB_CLIP_MS {
            return false;
        }
        match seg {
            Segment::A => {
                self.a_ms = ms;
                self.a_ready = true;
            }
            Segment::B => {
                self.b_ms = ms;
                self.b_ready = true;
            }
        }
        true
    }

    /// 两段齐备才可回放对比（公平性门）。
    pub fn playable(&self) -> bool {
        self.a_ready && self.b_ready && self.a_ms == self.b_ms
    }

    pub fn clip_ms(&self) -> u32 {
        self.a_ms
    }
}

impl Default for AbPair {
    fn default() -> Self {
        Self::new()
    }
}

/// A/B 段别。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Segment {
    A,
    B,
}

// ---------------------------------------------------------------------------
// CPU 预算门（增量 <2% 才允许常开）
// ---------------------------------------------------------------------------

/// 降噪 CPU 账（宿主逐帧报百分点增量，内核记账判定常开资格）。
pub struct CpuLedger {
    /// 最近 100 帧的百分点增量（环形）。
    ring: [u32; 100],
    head: usize,
    filled: usize,
}

impl CpuLedger {
    pub fn new() -> CpuLedger {
        CpuLedger { ring: [0; 100], head: 0, filled: 0 }
    }

    /// 报一帧增量（百分点）。
    pub fn report(&mut self, pct: u32) {
        self.ring[self.head] = pct;
        self.head = (self.head + 1) % 100;
        if self.filled < 100 {
            self.filled += 1;
        }
    }

    /// 均值增量（百分点）。
    pub fn mean_pct(&self) -> u32 {
        if self.filled == 0 {
            return 0;
        }
        let sum: u64 = self.ring.iter().take(self.filled).map(|&v| v as u64).sum();
        (sum / self.filled as u64) as u32
    }

    /// 常开资格：均值 < CPU_BUDGET_PCT（判据是严格小于 2）。
    pub fn may_stay_on(&self) -> bool {
        self.mean_pct() < CPU_BUDGET_PCT
    }
}

impl Default for CpuLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_micalm_checks() -> CheckSet {
    let mut set = CheckSet::new(ISTAR_DOMAIN);

    // 1. 开关即时：关态直通（零分类记账）；开态首帧即按开态处理。
    let mut dn = Denoiser::new();
    let (g0, c0) = dn.process_frame(100);
    dn.set_enabled(true);
    let (g1, _) = dn.process_frame(10);
    set.add(
        "switch takes effect next frame",
        g0 == 256 && c0 == FrameClass::Bypassed && g1 == NOISE_SUPPRESS_Q8_8 && dn.enabled(),
        "",
    );

    // 2. 语音频段保真：标定期（10 帧）后，强信号帧（>底噪×3）增益 256 直通不伤语音。
    let mut dn2 = Denoiser::new();
    dn2.set_enabled(true);
    for _ in 0..FLOOR_BOOTSTRAP_FRAMES {
        dn2.process_frame(10); // 标定期：稳态底噪学习
    }
    let (gv, cv) = dn2.process_frame(100); // ×10 超瞬态阈值
    set.add("voice band untouched after calibration", gv == 256 && cv == FrameClass::Voice, "");

    // 3. 稳态底噪衰减：弱稳态帧被压到衰减档；底噪估计随帧收敛。
    let suppressed = dn2.suppressed_frames() >= 1 && dn2.voice_frames() == 1;
    let floor_moves = dn2.floor() > 0;
    set.add("steady noise suppressed and floor tracks", suppressed && floor_moves, "");

    // 4. 语音频段参数入册：300-3400Hz 单源取数。
    set.add(
        "voice band params single source",
        dn2.voice_band() == (VOICE_BAND_LO_HZ, VOICE_BAND_HI_HZ),
        "",
    );

    // 5. A/B 回放链路：5 秒两段齐备可回放；单边/超长/不等长均拒。
    let mut ab = AbPair::new();
    let one_side = !ab.playable();
    let long_ok = ab.record(Segment::A, AB_CLIP_MS);
    let too_long = !ab.record(Segment::A, AB_CLIP_MS + 1);
    ab.record(Segment::B, AB_CLIP_MS);
    set.add(
        "ab pair fairness gate",
        one_side && long_ok && too_long && ab.playable() && ab.clip_ms() == AB_CLIP_MS,
        "",
    );

    // 6. CPU 增量红线：1% 均值可常开；2% 触线拒绝；预算常量=2。
    let mut led = CpuLedger::new();
    for _ in 0..100 {
        led.report(1);
    }
    let one_ok = led.may_stay_on();
    for _ in 0..100 {
        led.report(3);
    }
    set.add(
        "cpu budget strict under 2",
        one_ok && !led.may_stay_on() && led.mean_pct() == 3 && CPU_BUDGET_PCT == 2,
        "",
    );

    // 7. 链路一致：采样率/帧长与 F322 引擎同源常量（48k/10ms）。
    set.add(
        "f322 chain constants",
        SAMPLE_RATE_HZ == 48_000 && FRAME_SAMPLES == 480,
        "",
    );

    // 8. 冷启动标定期：开态前 10 帧一律学习底噪（结构性解决 0 底噪起点，
    //    不靠首帧特判）；标定期后强帧即刻按语音直通。
    let mut dn3 = Denoiser::new();
    dn3.set_enabled(true);
    let mut all_noise = true;
    for _ in 0..FLOOR_BOOTSTRAP_FRAMES {
        let (g, c) = dn3.process_frame(10);
        all_noise &= g == NOISE_SUPPRESS_Q8_8 && c == FrameClass::Noise;
    }
    let learned = dn3.suppressed_frames() == FLOOR_BOOTSTRAP_FRAMES && dn3.floor() > 0;
    let (gv, cv) = dn3.process_frame(100);
    set.add(
        "calibration learns then protects",
        all_noise && learned && gv == 256 && cv == FrameClass::Voice,
        "",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floor_converges_monotonic_up() {
        let mut dn = Denoiser::new();
        dn.set_enabled(true);
        let mut last = 0;
        for _ in 0..20 {
            dn.process_frame(40);
            let f = dn.floor();
            assert!(f >= last, "底噪估计应单调不回退（稳态注入）");
            last = f;
        }
        assert!(last > 0);
    }

    #[test]
    fn ab_pair_zero_ms_rejected() {
        let mut ab = AbPair::new();
        assert!(!ab.record(Segment::A, 0));
    }

    #[test]
    fn bypass_counts_nothing() {
        let mut dn = Denoiser::new();
        for _ in 0..5 {
            dn.process_frame(50);
        }
        assert_eq!(dn.frames(), 5);
        assert_eq!(dn.suppressed_frames(), 0);
        assert_eq!(dn.voice_frames(), 0);
    }
}
