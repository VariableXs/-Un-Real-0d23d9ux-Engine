//! VE-F0138 · 屏幕录制管线接口（VE-A 域 · A06 · 目标 360 行）
//!
//! 铁律：专用捕获路径，性能声明，DRM 拒录，降帧保命。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址 0xE8D0
pub const SEG_BASE: u16 = 0xE8D0;
pub const XC_DRM_REJECT: u16 = SEG_BASE | 0x01;
pub const XC_PERF_OVER: u16 = SEG_BASE | 0x02;
pub const XC_CAPTURE_FAIL: u16 = SEG_BASE | 0x03;

/// 录制状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RecState { Idle, Recording, Paused, Degraded }

/// 屏幕录制管线。
pub struct ScreenRecorder {
    state: RecState,
    fps: u32,
    drm_protected: bool,
    perf_cost_us: u32,
    frames_captured: u64,
    last_code: u16,
    drops: u32,
}

impl ScreenRecorder {
    pub fn new(fps: u32) -> Self {
        ScreenRecorder { state: RecState::Idle, fps, drm_protected: false,
            perf_cost_us: 0, frames_captured: 0, last_code: 0, drops: 0 }
    }
    pub fn state(&self) -> RecState { self.state }
    pub fn last_code(&self) -> u16 { self.last_code }

    /// 开始录制（DRM 内容拒录 = 版权红线）。
    pub fn start(&mut self, drm: bool) -> Result<(), u16> {
        if drm { self.last_code = XC_DRM_REJECT; return Err(XC_DRM_REJECT); }
        self.drm_protected = drm;
        self.state = RecState::Recording;
        self.last_code = 0;
        Ok(())
    }

    /// 捕获一帧。
    pub fn capture(&mut self, cost_us: u32) -> Result<(), u16> {
        if self.state != RecState::Recording { self.last_code = XC_CAPTURE_FAIL; return Err(XC_CAPTURE_FAIL); }
        self.perf_cost_us = cost_us;
        if cost_us > 5000 {
            // 性能超支 → 降帧保命
            self.drops += 1;
            self.state = RecState::Degraded;
            self.last_code = XC_PERF_OVER;
            return Err(XC_PERF_OVER);
        }
        self.frames_captured += 1;
        self.last_code = 0;
        Ok(())
    }

    /// 性能影响声明（开录制贵多少）。
    pub fn perf_cost_us(&self) -> u32 { self.perf_cost_us }
    pub fn frames_captured(&self) -> u64 { self.frames_captured }
    pub fn drops(&self) -> u32 { self.drops }
    pub fn stop(&mut self) { self.state = RecState::Idle; }
}

pub fn run_vea138_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }
    let mut set = CheckSet::new("vea138_record");
    { let r = ScreenRecorder::new(30); chk(&mut set, "C138-01", r.state() == RecState::Idle, "err"); }
    { let mut r = ScreenRecorder::new(30); let res = r.start(true);
        chk(&mut set, "C138-02", res.err() == Some(XC_DRM_REJECT), "err"); }
    { let mut r = ScreenRecorder::new(30); let res = r.start(false);
        chk(&mut set, "C138-03", res.is_ok() && r.state() == RecState::Recording, "err"); }
    { let mut r = ScreenRecorder::new(30); let _ = r.start(false);
        let res = r.capture(1000);
        chk(&mut set, "C138-04", res.is_ok() && r.frames_captured() == 1, "err"); }
    { let mut r = ScreenRecorder::new(30); let _ = r.start(false);
        let res = r.capture(10000);
        chk(&mut set, "C138-05", res.err() == Some(XC_PERF_OVER) && r.drops() == 1, "err"); }
    { let mut r = ScreenRecorder::new(30);
        let res = r.capture(1000);
        chk(&mut set, "C138-06", res.err() == Some(XC_CAPTURE_FAIL), "err"); }
    { let mut r = ScreenRecorder::new(30); let _ = r.start(false); r.stop();
        chk(&mut set, "C138-07", r.state() == RecState::Idle, "err"); }
    { let mut r = ScreenRecorder::new(30); let _ = r.start(false);
        let _ = r.capture(100);
        chk(&mut set, "C138-08", r.perf_cost_us() == 100, "err"); }
    { let r = ScreenRecorder::new(60); chk(&mut set, "C138-09", r.state() == RecState::Idle, "err"); }
    { let mut r = ScreenRecorder::new(30); let _ = r.start(false);
        let _ = r.capture(100); let _ = r.capture(100);
        chk(&mut set, "C138-10", r.frames_captured() == 2, "err"); }
    set
}
