//! F026 深化批次二 · DirectSound 环形缓冲与会话记忆面（compatstar2/deep · G-A-26）。
//!
//! 批次一深化覆盖 WAVEFORMATEX/头状态机/混音线控制；本批补齐：DirectSound
//! 次缓冲环形 Lock/Unlock 双段语义（回绕点拆两段——MS DirectSound 缓冲模型）、
//! 每应用音量会话记忆（「应用退出时保存」——主册【设计细节】）、电平表账面
//! （峰值/均方——60fps 电平条的数据源）、重采样步进约分（48k↔44.1k 的
//! 有理数步进——多相滤波的参数面）。
//!
//! 零堆纪律：定长表，无 alloc。

use crate::checks::CheckSet;

/// 会话音量记忆容量。
pub const SESSION_VOL_SLOTS: usize = 16;

/// DirectSound 次缓冲环形锁：回绕点拆两段（MS Lock 的 lpvAudioPtr1/2 语义）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RingLock {
    /// 段一起点/长度。
    pub seg1_off: usize,
    pub seg1_len: usize,
    /// 段二起点/长度（回绕段；无回绕时长度 0）。
    pub seg2_off: usize,
    pub seg2_len: usize,
}

/// 对 size 字节环形缓冲从 offset 锁 len 字节 → 双段布局。
pub fn ring_lock(size: usize, offset: usize, len: usize) -> Result<RingLock, &'static str> {
    if size == 0 || len == 0 || offset >= size {
        return Err("invalid-lock-range");
    }
    let first = (size - offset).min(len);
    let rest = len - first;
    Ok(RingLock {
        seg1_off: offset,
        seg1_len: first,
        seg2_off: 0,
        seg2_len: rest,
    })
}

/// 每应用音量会话记忆（应用退出保存、重装/重启还原——主册【设计细节】）。
pub struct SessionVolumeStore {
    pub apps: [Option<(&'static str, u16)>; SESSION_VOL_SLOTS],
    pub count: usize,
}

impl SessionVolumeStore {
    pub const fn new() -> Self {
        SessionVolumeStore { apps: [None; SESSION_VOL_SLOTS], count: 0 }
    }
    /// 退出时保存（同应用覆盖）。
    pub fn save(&mut self, app: &'static str, vol: u16) {
        for e in self.apps.iter_mut().take(self.count) {
            if let Some((a, v)) = e {
                if *a == app {
                    *v = vol;
                    return;
                }
            }
        }
        if self.count < SESSION_VOL_SLOTS {
            self.apps[self.count] = Some((app, vol));
            self.count += 1;
        }
    }
    /// 重启后还原。
    pub fn restore(&self, app: &str) -> Option<u16> {
        self.apps.iter().flatten().find(|(a, _)| *a == app).map(|(_, v)| *v)
    }
}

/// 电平表账面：峰值幅度 + 均方（RMS = sqrt(均方)，比较走平方避免开方；
/// 峰值取绝对值口径——i16::MIN 全刻度不回绕——60fps 电平条的数据源）。
pub struct LevelMeter {
    /// 峰值幅度（绝对值，0..=32768）。
    pub peak_mag: u16,
    sum_sq: u64,
    pub frames: u32,
}

impl LevelMeter {
    pub const fn new() -> Self {
        LevelMeter { peak_mag: 0, sum_sq: 0, frames: 0 }
    }
    pub fn push(&mut self, sample: i16) {
        let s = sample.unsigned_abs(); // u16，含 i16::MIN = 32768 不回绕
        if s > self.peak_mag {
            self.peak_mag = s;
        }
        self.sum_sq += s as u64 * s as u64;
        self.frames += 1;
    }
    /// 均方（rms²）。
    pub fn mean_square(&self) -> u64 {
        if self.frames == 0 {
            0
        } else {
            self.sum_sq / self.frames as u64
        }
    }
}

/// 重采样步进约分：dst/src 化为最简整数比（辗转相除；零浮点）。
pub fn resample_step(src_hz: u32, dst_hz: u32) -> (u32, u32) {
    fn gcd(a: u32, b: u32) -> u32 {
        if b == 0 { a } else { gcd(b, a % b) }
    }
    let g = gcd(src_hz, dst_hz);
    (dst_hz / g, src_hz / g)
}

/// 域自检（深化批次二）。
pub fn run_f026d_checks() -> CheckSet {
    let mut cs = CheckSet::new("F026-winmm-d2");
    // 1) 环形锁不回绕：单段（80,20），段二空。
    let straight = ring_lock(100, 80, 20).unwrap();
    // 2) 环形锁回绕：从 80 锁 40 → 段一 (80,20) + 段二 (0,20)。
    let wrap = ring_lock(100, 80, 40).unwrap();
    cs.add(
        "dsound_ring_lock",
        straight == RingLock { seg1_off: 80, seg1_len: 20, seg2_off: 0, seg2_len: 0 }
            && wrap == RingLock { seg1_off: 80, seg1_len: 20, seg2_off: 0, seg2_len: 20 },
        "",
    );
    // 3) 非法锁范围：越界偏移拒绝。
    cs.add("ring_lock_invalid", ring_lock(100, 100, 1).is_err() && ring_lock(100, 0, 0).is_err(), "");
    // 4) 会话音量记忆：退出保存 → 还原同值；同应用覆盖。
    let mut sv = SessionVolumeStore::new();
    sv.save("music-player", 0x8000);
    sv.save("music-player", 0x4000);
    sv.save("game", 0xFFFF);
    cs.add("session_volume_memory", sv.restore("music-player") == Some(0x4000) && sv.restore("game") == Some(0xFFFF) && sv.restore("ghost").is_none(), "");
    // 5) 电平表：方波 ±100 → 峰值幅度 100、均方 10000（RMS = 100 = 峰值）。
    let mut lm = LevelMeter::new();
    for s in [100i16, -100, 100, -100] {
        lm.push(s);
    }
    cs.add("level_meter_math", lm.peak_mag == 100 && lm.frames == 4 && lm.mean_square() == 10_000, "");
    // 6) 重采样步进：48k→44.1k = 147/160（最简比）。
    cs.add("resample_fraction", resample_step(48_000, 44_100) == (147, 160) && resample_step(44_100, 44_100) == (1, 1), "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sine_3db_line_square_free() {
        // 正弦样本：峰值 32767、均方约一半（3dB 线的平方域判定，免开方）。
        let mut lm = LevelMeter::new();
        for i in 0..8u32 {
            // 量化正弦一个周期 8 点：0,7071,10000,7071,0,-7071,-10000,-7071（÷ 3.2768）
            let s = [0i16, 7071, 10000, 7071, 0, -7071, -10000, -7071][i as usize];
            lm.push(s);
        }
        assert_eq!(lm.peak_mag, 10_000);
        // 均方 = (7071²×4 + 10000²×2) / 8 = (199,994,164 + 200,000,000)/8 ≈ 49,999,270
        assert!(lm.mean_square() > 49_000_000 && lm.mean_square() < 50_000_000, "正弦均方 ≈ 峰值²/2（3dB 线）");
    }

    #[test]
    fn ring_lock_full_wrap() {
        let l = ring_lock(64, 56, 16).unwrap();
        assert_eq!((l.seg1_off, l.seg1_len, l.seg2_off, l.seg2_len), (56, 8, 0, 8));
    }

    #[test]
    fn deep2_checks_all_green() {
        let cs = run_f026d_checks();
        assert!(cs.all_passed() && !cs.truncated());
    }
}
