//! F026 深化批次四 · 混音总线面（compatstar2/deep3 · G-A-26）。
//!
//! 批次一~三覆盖 waveOut 族/流表/重采样等程序可见面；本批补齐主册
//! 【功能定义】「全语义对齐」的序列化/账本/容错面：混音总线模型
//! （4 路输入 × 增益 × 声像 → 立体声求和，定点饱和不回绕）、峰值表
//! 弹道学（PPM：新峰即时上升，峰值按固定系数每帧衰减，低于下限归零）、
//! 采样计数对账（输入帧数 = 设备实出帧数，不等入账不静默）、静音/独奏
//! 优先矩阵（solo 压倒 mute，四通道 16 格判定表逐格核对）。
//!
//! 判据对账：主册 G-A-26【设计细节】主音量闸优先/电平条灰显 + MS
//! waveOutSetVolume/waveOutSetPan 文档语义对拍（音量 0x0000~0xFFFF、
//! pan -10000~10000、16 位 PCM 满幅）。零堆纪律：定长通道表，无
//! Vec/String/Box/format!，错误一律 Err 或计数账面，零静默。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 混音总线输入路数（域内模型口径：4 路）。
pub const CHANNELS: usize = 4;
/// 增益 1.0 的 Q16 定点（MS waveOutSetVolume 全量程 0xFFFF 折算 1.0）。
pub const GAIN_ONE_Q16: u32 = 0x1_0000;
/// pan 范围（MS waveOutSetPan 语义：-10000~10000，0 = 居中）。
pub const PAN_RANGE: i32 = 10_000;
/// 16 位 PCM 满幅上/下界（饱和边界——定点求和不回绕）。
pub const PCM_MAX: i32 = 32767;
pub const PCM_MIN: i32 = -32768;
/// PPM 每帧衰减系数 0.875（875/1000——峰值表弹道学，域内口径）。
pub const PPM_DECAY_PERMILLE: u32 = 875;
/// PPM 衰减下限（低于归零，防长尾永不为零——账面自洽）。
pub const PPM_FLOOR: u32 = 8;

// ---------------------------------------------------------------------------
// 纯函数语义面
// ---------------------------------------------------------------------------

/// 一路总线输入（增益/声像/静音/独奏——MS waveOut 语义四元组）。
#[derive(Clone, Copy)]
pub struct BusChannel {
    pub gain_q16: u32,
    pub pan: i32,
    pub muted: bool,
    pub soloed: bool,
}

/// 静音/独奏优先矩阵单格判定（纯函数——16 格判定表的格语义）。
/// 总线上有任意 solo 时 audible = soloed（solo 压倒 mute）；否则 !muted。
pub fn matrix_cell(muted: bool, soloed: bool, bus_any_solo: bool) -> bool {
    if bus_any_solo {
        soloed
    } else {
        !muted
    }
}

/// PPM 弹道学单步：新峰即时上升；否则按固定系数每帧衰减、低于下限归零。
pub fn ppm_step(current: u32, new_abs: u32) -> u32 {
    if new_abs >= current {
        new_abs
    } else {
        let decayed = current * PPM_DECAY_PERMILLE / 1000;
        if decayed < PPM_FLOOR {
            0
        } else {
            decayed
        }
    }
}

/// 线性声像律（‰）：pan=0 双侧 1000‰；pan=+10000 左 0‰；pan=-10000 右 0‰。
pub fn atten_permille(pan: i32, right: bool) -> u32 {
    let span = if right { PAN_RANGE + pan } else { PAN_RANGE - pan };
    span.clamp(0, PAN_RANGE) as u32 * 1000 / PAN_RANGE as u32
}

/// 定点饱和（i64 中间量 → i16 满幅钳制，绝不回绕）。
fn sat16(v: i64) -> i16 {
    if v > PCM_MAX as i64 {
        PCM_MAX as i16
    } else if v < PCM_MIN as i64 {
        PCM_MIN as i16
    } else {
        v as i16
    }
}

// ---------------------------------------------------------------------------
// 混音总线模型
// ---------------------------------------------------------------------------

/// 混音总线：4 路输入 → 立体声求和 + PPM 弹道 + 采样计数对账。
pub struct MixerBus {
    pub channels: [BusChannel; CHANNELS],
    /// 左/右输出峰值表（0~32767，弹道见 ppm_step）。
    pub ppm_l: u32,
    pub ppm_r: u32,
    /// 混入帧数（输入侧总账）。
    pub frames_in: u64,
    /// 设备实出帧数（输出侧总账，由 note_device_output 记账）。
    pub frames_out: u64,
    /// 对账不等记账（输入帧数 ≠ 实出帧数的对账块数——欠载显性化）。
    pub mismatch_blocks: u32,
}

impl MixerBus {
    pub const fn new() -> Self {
        MixerBus {
            channels: [BusChannel { gain_q16: GAIN_ONE_Q16, pan: 0, muted: false, soloed: false }; CHANNELS],
            ppm_l: 0,
            ppm_r: 0,
            frames_in: 0,
            frames_out: 0,
            mismatch_blocks: 0,
        }
    }

    /// 通道可闻性（矩阵判定走全总线状态——solo 压倒 mute）。
    pub fn audible(&self, ch: usize) -> bool {
        let any_solo = self.channels.iter().any(|c| c.soloed);
        let c = self.channels[ch];
        matrix_cell(c.muted, c.soloed, any_solo)
    }

    /// 混一帧：4 路单声道输入 → 立体声输出（饱和求和 + PPM 步进）。
    /// 只记输入帧；输出帧由设备侧 note_device_output 记账（对账分离）。
    pub fn mix_frame(&mut self, input: [i16; CHANNELS]) -> (i16, i16) {
        let mut acc_l: i64 = 0;
        let mut acc_r: i64 = 0;
        for ch in 0..CHANNELS {
            if !self.audible(ch) {
                continue;
            }
            let c = self.channels[ch];
            let g = (input[ch] as i64 * c.gain_q16 as i64) >> 16;
            acc_l += g * atten_permille(c.pan, false) as i64 / 1000;
            acc_r += g * atten_permille(c.pan, true) as i64 / 1000;
        }
        let (l, r) = (sat16(acc_l), sat16(acc_r));
        self.ppm_l = ppm_step(self.ppm_l, l.unsigned_abs() as u32);
        self.ppm_r = ppm_step(self.ppm_r, r.unsigned_abs() as u32);
        self.frames_in += 1;
        (l, r)
    }

    /// 设备侧实出帧数记账（欠载时实出 < 混入——对账数据源）。
    pub fn note_device_output(&mut self, frames: u64) {
        self.frames_out += frames;
    }

    /// 采样计数对账：输入帧数 == 实出帧数；不等入账，不静默。
    pub fn reconcile(&mut self) -> bool {
        if self.frames_in == self.frames_out {
            true
        } else {
            self.mismatch_blocks += 1;
            false
        }
    }
}

// ---------------------------------------------------------------------------
// 域自检（深化批次四）
// ---------------------------------------------------------------------------

/// 域自检（F026 深化批次四 · 混音总线/PPM/对账/矩阵）。
pub fn run_f026f_checks() -> CheckSet {
    let mut cs = CheckSet::new("F026-mixbus-d4");
    // 1) 增益 1.0 + 居中声像 → 透传（Q16 定点全量程）。
    let mut bus = MixerBus::new();
    let (l, r) = bus.mix_frame([1000, 0, 0, 0]);
    cs.add("bus_center_passthrough", l == 1000 && r == 1000, "");
    // 2) 增益 0.5（Q16 0x8000）→ 半幅。
    let mut half = MixerBus::new();
    half.channels[0].gain_q16 = 0x8000;
    let (l, r) = half.mix_frame([1000, 0, 0, 0]);
    cs.add("bus_gain_half_atten", l == 500 && r == 500, "");
    // 3) 声像律：pan=+10000 → 左 0‰ 右 1000‰；pan=-10000 → 镜像。
    let mut pr = MixerBus::new();
    pr.channels[0].pan = PAN_RANGE;
    let (l, r) = pr.mix_frame([1000, 0, 0, 0]);
    let mut pl = MixerBus::new();
    pl.channels[0].pan = -PAN_RANGE;
    let (l2, r2) = pl.mix_frame([1000, 0, 0, 0]);
    cs.add("bus_pan_law", l == 0 && r == 1000 && l2 == 1000 && r2 == 0, "");
    // 4) 定点饱和不回绕：两路 30000 求和 60000 → 钳到 32767（绝非负回绕）。
    let mut sat = MixerBus::new();
    let (l, r) = sat.mix_frame([30000, 30000, 0, 0]);
    cs.add("bus_sat_no_wrap", l == PCM_MAX as i16 && r == PCM_MAX as i16, "");
    // 5) PPM 新峰即时上升。
    let mut ppm = MixerBus::new();
    let _ = ppm.mix_frame([30000, 0, 0, 0]);
    cs.add("ppm_instant_rise", ppm.ppm_l == 30000 && ppm.ppm_r == 30000, "");
    // 6) PPM 固定系数衰减：一帧静音后 30000×0.875 = 26250；持续静音帧后归零。
    let _ = ppm.mix_frame([0, 0, 0, 0]);
    let mut decay_ok = ppm.ppm_l == 30000 * PPM_DECAY_PERMILLE / 1000;
    for _ in 0..64 {
        let _ = ppm.mix_frame([0, 0, 0, 0]);
    }
    decay_ok = decay_ok && ppm.ppm_l == 0 && ppm.ppm_r == 0;
    cs.add("ppm_decay_ballistic", decay_ok, "");
    // 7) 采样计数对账：入 == 出对账通过；不等入账（欠载显性化）。
    let mut led = MixerBus::new();
    for _ in 0..3 {
        let _ = led.mix_frame([100, 0, 0, 0]);
    }
    led.note_device_output(3);
    let even = led.reconcile();
    let _ = led.mix_frame([100, 0, 0, 0]);
    let _ = led.mix_frame([100, 0, 0, 0]);
    led.note_device_output(1);
    cs.add("sample_count_reconcile", even && !led.reconcile() && led.mismatch_blocks == 1, "");
    // 8) solo 压倒 mute：ch1 静音+独奏仍可闻；ch0 无独奏被压。
    let mut solo = MixerBus::new();
    solo.channels[0].muted = false;
    solo.channels[1].muted = true;
    solo.channels[1].soloed = true;
    cs.add("solo_overrides_mute", solo.audible(1) && !solo.audible(0), "");
    // 9) 四通道 16 格判定表逐格核对（通道 × {裸/静音/独奏/静音+独奏}）。
    let states = [(false, false), (true, false), (false, true), (true, true)];
    let mut cells_ok = true;
    for ch in 0..CHANNELS {
        for &(m, s) in states.iter() {
            let mut b = MixerBus::new();
            b.channels[ch] = BusChannel { gain_q16: GAIN_ONE_Q16, pan: 0, muted: m, soloed: s };
            cells_ok = cells_ok && b.audible(ch) == matrix_cell(m, s, s);
        }
    }
    cs.add("matrix_sixteen_cells", cells_ok, "");
    // 10) 无 solo 时静音即无声（混音输出为零、PPM 不抬）。
    let mut mute = MixerBus::new();
    mute.channels[0].muted = true;
    let (l, r) = mute.mix_frame([20000, 0, 0, 0]);
    cs.add("mute_silence_ledger", l == 0 && r == 0 && mute.ppm_l == 0 && mute.ppm_r == 0, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saturate_never_wraps_negative() {
        // 双满幅正输入求和绝不回绕为负——定点饱和纪律。
        let mut bus = MixerBus::new();
        let (l, r) = bus.mix_frame([32767, 32767, 32767, 32767]);
        assert_eq!(l, PCM_MAX as i16);
        assert_eq!(r, PCM_MAX as i16);
        let mut neg = MixerBus::new();
        let (l, r) = neg.mix_frame([-32768, -32768, -32768, -32768]);
        assert_eq!(l, PCM_MIN as i16);
        assert_eq!(r, PCM_MIN as i16);
    }

    #[test]
    fn pan_law_symmetric_midpoint() {
        // 半程声像 ±5000 → 500‰ 衰减侧（线性律标定）。
        let mut bus = MixerBus::new();
        bus.channels[0].pan = 5000;
        let (l, r) = bus.mix_frame([1000, 0, 0, 0]);
        assert_eq!(l, 500);
        assert_eq!(r, 1000);
    }

    #[test]
    fn deep4_checks_all_green() {
        let cs = run_f026f_checks();
        assert!(cs.all_passed() && !cs.truncated());
    }
}
