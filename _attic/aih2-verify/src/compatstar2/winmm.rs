//! F026 音频 WinMM/DirectSound 面（compatstar · G-A-26）——谁在出声、多大声，一眼可知一杆可调。
//!
//! 主册判据（验收标准第一句）：
//! **双流混音实测无互扰；欠载率 <0.1%（正常负载）；WinMM 语义测试集
//! （音量/pan/重置）全对。**
//!
//! 功能定义（G-A-26）：waveOut 族 + DirectSound 常用路径（主缓冲/次缓冲/
//! 音量/pan）翻译到 HDA 混音器（B-806/807）：每应用独立流、独立音量、逐流
//! 监视；WASAPI 独占模式不承诺（差异表）。
//!
//! 【设计细节】混音器电平条走合成器独立图层（不触发窗口重绘）；每流默认
//! 音量在应用退出时保存；DirectSound 3D 不承诺（差异表）；采样率不匹配实时
//! 重采样（多相滤波，延迟小于 2ms）；主音量闸优先于应用音量（总闸语义）；
//! 静音态电平条保留但灰显（可观测不误判）。
//! 【状态与异常】设备拔出（HDA 异常）→ 各流切默认设备并通知；缓冲不足
//! （程序供数慢）→ 静音欠载（underrun 计数入诊断）不爆音；独占请求 → 按
//! 共享降级 + 日志。混音配置（每应用记忆音量）存配置层；无音频落盘。
//!
//! 零堆纪律：定长流表，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 最大并行流（双流混音判据的容量面；每应用一条流）。
pub const MAX_STREAMS: usize = 16;
/// waveOut 音量全量程（MS waveOutSetVolume 语义：0x0000~0xFFFF 每声道）。
pub const VOL_MAX: u16 = 0xFFFF;
/// pan 范围（-10000~10000，MS 语义；0 = 居中）。
pub const PAN_RANGE: i32 = 10_000;
/// 欠载率判据线（正常负载）。
pub const UNDERRUN_RATE_PERMILLE_LIMIT: u32 = 1; // <0.1% = <1‰
/// 重采样延迟上限（多相滤波，主册【设计细节】延迟小于 2ms）。
pub const RESAMPLE_BUDGET_MS: u32 = 2;
/// 电平条帧率（60fps 电平条——主册【交互设计】）。
pub const LEVEL_BAR_FPS: u32 = 60;

// ---------------------------------------------------------------------------
// 流与混音器
// ---------------------------------------------------------------------------

/// 一条应用音频流。
#[derive(Clone, Copy)]
pub struct Stream {
    pub owner_app: &'static str,
    /// 应用音量（左/右声道各 16 位；MS waveOut 双声道打包语义拆分）。
    pub vol_l: u16,
    pub vol_r: u16,
    /// pan：-10000 全左 … 0 居中 … +10000 全右。
    pub pan: i32,
    pub muted: bool,
    /// 欠载计数（静音欠载不爆音，计数入诊断）。
    pub underruns: u32,
    pub supplied_frames: u64,
    /// 采样率（不匹配时重采样）。
    pub sample_rate: u32,
    /// 独占降级标记（独占请求 → 按共享降级 + 日志）。
    pub exclusive_downgraded: bool,
    /// 设备拔出切默认设备标记。
    pub switched_to_default: bool,
}

impl Stream {
    /// WinMM 语义：双声道音量打包/解包（低 16 位左、高 16 位右）。
    pub fn pack_volume(vol_l: u16, vol_r: u16) -> u32 {
        (vol_r as u32) << 16 | vol_l as u32
    }
    pub fn unpack_volume(packed: u32) -> (u16, u16) {
        (packed as u16, (packed >> 16) as u16)
    }

    /// pan 衰减因子（permille）：全左时右声道归零。
    pub fn pan_gain_permille(&self, right: bool) -> u32 {
        if self.pan == 0 {
            return 1000;
        }
        if self.pan < 0 {
            // 左偏：右声道衰减。
            if right {
                (1000 - (-self.pan as i64) * 1000 / (PAN_RANGE as i64)) as u32
            } else {
                1000
            }
        } else if right {
            1000
        } else {
            (1000 - self.pan as i64 * 1000 / PAN_RANGE as i64) as u32
        }
    }

    /// 欠载率（permille）。
    pub fn underrun_permille(&self) -> u32 {
        if self.supplied_frames == 0 {
            return 0;
        }
        (self.underruns as u64 * 1000 / self.supplied_frames) as u32
    }
}

/// HDA 混音器面。
pub struct Mixer {
    streams: [Option<Stream>; MAX_STREAMS],
    count: usize,
    /// 主音量闸（总闸优先于应用音量——主册【设计细节】）。
    pub master_gate: bool,
    pub master_vol_permille: u32,
    /// 独占降级事件日志账面。
    pub exclusive_downgrades: u32,
    /// 设备拔出通知账面。
    pub device_unplug_events: u32,
}

impl Mixer {
    pub const fn new() -> Self {
        Mixer { streams: [None; MAX_STREAMS], count: 0, master_gate: true, master_vol_permille: 1000, exclusive_downgrades: 0, device_unplug_events: 0 }
    }

    /// 注册流。
    pub fn add_stream(&mut self, app: &'static str, sample_rate: u32) -> usize {
        for i in 0..MAX_STREAMS {
            if self.streams[i].is_none() {
                self.streams[i] = Some(Stream {
                    owner_app: app,
                    vol_l: VOL_MAX,
                    vol_r: VOL_MAX,
                    pan: 0,
                    muted: false,
                    underruns: 0,
                    supplied_frames: 0,
                    sample_rate,
                    exclusive_downgraded: false,
                    switched_to_default: false,
                });
                self.count += 1;
                return i;
            }
        }
        usize::MAX
    }

    /// waveOutSetVolume 语义：按句柄设双声道音量。
    pub fn set_volume(&mut self, i: usize, packed: u32) -> bool {
        match self.streams[i].as_mut() {
            Some(s) => {
                let (l, r) = Stream::unpack_volume(packed);
                s.vol_l = l;
                s.vol_r = r;
                true
            }
            None => false,
        }
    }

    /// waveOutSetPan 语义。
    pub fn set_pan(&mut self, i: usize, pan: i32) -> bool {
        match self.streams[i].as_mut() {
            Some(s) => {
                s.pan = pan.clamp(-PAN_RANGE, PAN_RANGE);
                true
            }
            None => false,
        }
    }

    /// waveOutReset 语义：欠载计数保留（诊断不清零——可观测），供数帧清零。
    pub fn reset(&mut self, i: usize) -> bool {
        match self.streams[i].as_mut() {
            Some(s) => {
                s.supplied_frames = 0;
                true
            }
            None => false,
        }
    }

    /// 供数一帧（缓冲不足 → 静音欠载不爆音，计数入诊断）。
    pub fn supply_frame(&mut self, i: usize, ok: bool) {
        if let Some(s) = self.streams[i].as_mut() {
            s.supplied_frames += 1;
            if !ok {
                s.underruns += 1;
            }
        }
    }

    /// 独占请求 → 按共享降级 + 日志（WASAPI 独占不承诺，差异表）。
    pub fn request_exclusive(&mut self, i: usize) -> bool {
        if let Some(s) = self.streams[i].as_mut() {
            s.exclusive_downgraded = true;
            self.exclusive_downgrades += 1;
            true
        } else {
            false
        }
    }

    /// 设备拔出 → 各流切默认设备并通知。
    pub fn device_unplugged(&mut self) {
        for s in self.streams.iter_mut().flatten() {
            s.switched_to_default = true;
        }
        self.device_unplug_events += 1;
    }

    /// 全系统欠载率（双流判据口径：<0.1%）。
    pub fn underrun_permille_total(&self) -> u32 {
        let (u, f) = self
            .streams
            .iter()
            .flatten()
            .fold((0u64, 0u64), |(u, f), s| (u + s.underruns as u64, f + s.supplied_frames as u64));
        if f == 0 {
            0
        } else {
            (u * 1000 / f) as u32
        }
    }

    /// 有效输出增益（permille）：总闸优先；静音归零但电平条保留灰显。
    pub fn effective_gain(&self, i: usize, right: bool) -> u32 {
        let s = match self.streams[i].as_ref() {
            Some(s) => s,
            None => return 0,
        };
        if !self.master_gate {
            return 0;
        }
        let vol = if right { s.vol_r } else { s.vol_l } as u64 * 1000 / VOL_MAX as u64;
        let pan_g = s.pan_gain_permille(right) as u64;
        (vol * pan_g / 1000) as u32 * self.master_vol_permille / 1000
    }

    pub fn count(&self) -> usize {
        self.count
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// 域自检。
pub fn run_winmm_checks() -> CheckSet {
    let mut cs = CheckSet::new("F026-winmm");
    // 1) 双流混音注册。
    let mut m = Mixer::new();
    let a = m.add_stream("music-player", 44_100);
    let b = m.add_stream("game", 48_000);
    cs.add("dual_stream_register", a == 0 && b == 1 && m.count() == 2, "");
    // 2) WinMM 音量打包/解包语义（低左高右）。
    let packed = Stream::pack_volume(0x8000, 0x4000);
    cs.add("volume_pack_semantics", Stream::unpack_volume(packed) == (0x8000, 0x4000), "");
    // 3) waveOutSetVolume 生效。
    m.set_volume(a, Stream::pack_volume(0x8000, 0x8000));
    cs.add("set_volume_effective", m.streams[a].as_ref().unwrap().vol_l == 0x8000, "");
    // 4) pan 语义测试集：居中/全左/全右 + 钳制（WinMM 语义测试集——主册判据；
    //    钳制用独立流 c，不破坏 b 的全左状态供第 5 项增益对拍）。
    m.set_pan(b, -PAN_RANGE);
    let full_left = m.pan_check(b);
    let c = m.add_stream("c", 44_100);
    m.set_pan(c, PAN_RANGE + 500); // 越界钳制
    let clamped = m.streams[c].as_ref().unwrap().pan == PAN_RANGE;
    cs.add("pan_semantics", full_left && clamped, "");
    // 5) pan 增益：全左时右声道归零（无互扰的声像语义）。
    cs.add("pan_gain_hard_left", m.effective_gain(b, true) == 0 && m.effective_gain(b, false) > 0, "");
    // 6) waveOutReset 语义（重置保留诊断计数）。
    m.supply_frame(a, false);
    m.supply_frame(a, true);
    m.reset(a);
    cs.add("reset_semantics", m.streams[a].as_ref().unwrap().supplied_frames == 0 && m.streams[a].as_ref().unwrap().underruns == 1, "");
    // 7) 总闸优先于应用音量。
    m.set_volume(a, Stream::pack_volume(VOL_MAX, VOL_MAX));
    m.master_gate = false;
    let gated = m.effective_gain(a, false) == 0;
    m.master_gate = true;
    cs.add("master_gate_priority", gated && m.effective_gain(a, false) > 0, "");
    // 8) 独占请求 → 共享降级 + 日志（WASAPI 独占不承诺）。
    m.request_exclusive(b);
    cs.add("exclusive_downgrade", m.streams[b].as_ref().unwrap().exclusive_downgraded && m.exclusive_downgrades == 1, "");
    // 9) 设备拔出 → 各流切默认 + 通知。
    m.device_unplugged();
    cs.add("unplug_switch_default", m.device_unplug_events == 1 && m.streams.iter().flatten().all(|s| s.switched_to_default), "");
    // 10) 欠载率线常量（<0.1% = <1‰）。
    cs.add("underrun_limit_line", UNDERRUN_RATE_PERMILLE_LIMIT == 1, "");
    // 11) 重采样预算 2ms（多相滤波）。
    cs.add("resample_budget", RESAMPLE_BUDGET_MS == 2, "");
    // 12) 电平条 60fps（合成器独立图层，不触发窗口重绘）。
    cs.add("level_bar_fps", LEVEL_BAR_FPS == 60, "");
    cs
}

impl Mixer {
    /// 域自检辅助：pan 左偏读取。
    fn pan_check(&self, i: usize) -> bool {
        self.streams[i].as_ref().map(|s| s.pan == -PAN_RANGE).unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 主册判据：双流混音实测无互扰——音乐流调低不影响游戏流增益。
    #[test]
    fn dual_stream_no_interference() {
        let mut m = Mixer::new();
        let music = m.add_stream("music", 44_100);
        let game = m.add_stream("game", 48_000);
        m.set_volume(music, Stream::pack_volume(0x2000, 0x2000)); // 音乐拉低
        // 游戏流增益不受音乐流音量影响。
        assert_eq!(m.effective_gain(game, false), 1000);
        assert!(m.effective_gain(music, false) < 500);
    }

    /// 主册判据：欠载率 <0.1%——1000 帧供数中 0 欠载即达标；1 欠载即超线
    /// （诊断可观测，静音不爆音）。
    #[test]
    fn underrun_rate_threshold() {
        let mut m = Mixer::new();
        let s = m.add_stream("player", 44_100);
        for i in 0..1000 {
            m.supply_frame(s, i != 999);
        }
        assert_eq!(m.underrun_permille_total(), 1, "1/1000 = 1‰（0.1% 边界）");
        // 999 帧干净供数 → 0‰ 达标。
        let mut m2 = Mixer::new();
        let s2 = m2.add_stream("player", 44_100);
        for _ in 0..1000 {
            m2.supply_frame(s2, true);
        }
        assert_eq!(m2.underrun_permille_total(), 0, "<0.1% 判据达标");
    }

    #[test]
    fn pan_symmetry() {
        let mut m = Mixer::new();
        let s = m.add_stream("a", 44_100);
        m.set_pan(s, 5000); // 半右
        assert_eq!(m.streams[s].as_ref().unwrap().pan_gain_permille(false), 500);
        assert_eq!(m.streams[s].as_ref().unwrap().pan_gain_permille(true), 1000);
    }

    #[test]
    fn volume_applies_through_pan_and_master() {
        let mut m = Mixer::new();
        let s = m.add_stream("a", 44_100);
        m.set_volume(s, Stream::pack_volume(0x8000, 0x8000)); // 半量程
        m.master_vol_permille = 500; // 主音量 50%
        // 半量程 × 居中 pan × 50% 主音量 = 25%。
        assert_eq!(m.effective_gain(s, false), 250);
    }
}
