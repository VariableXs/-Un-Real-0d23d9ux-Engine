//! VE-F0139 · 系统截图服务对接（VE-A 域 · A06 · 目标 300 行）
//!
//! 铁律：一致性截帧，HDR 保留，完整性校验，健康自检。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址 0xE8D1
pub const SEG_BASE: u16 = 0xE8D1;
pub const XC_TORN_FRAME: u16 = SEG_BASE | 0x01;
pub const XC_HDR_LOST: u16 = SEG_BASE | 0x02;
pub const XC_SERVICE_DOWN: u16 = SEG_BASE | 0x03;

pub struct ScreenshotService {
    healthy: bool,
    hdr_metadata: bool,
    captures: u64,
    torn_frames: u32,
    last_code: u16,
}

impl ScreenshotService {
    pub fn new() -> Self { Self { healthy: true, hdr_metadata: false, captures: 0, torn_frames: 0, last_code: 0 } }
    pub fn healthy(&self) -> bool { self.healthy }
    pub fn last_code(&self) -> u16 { self.last_code }
    pub fn capture(&mut self, torn: bool, hdr: bool) -> Result<(), u16> {
        if !self.healthy { self.last_code = XC_SERVICE_DOWN; return Err(XC_SERVICE_DOWN); }
        if torn { self.torn_frames += 1; self.last_code = XC_TORN_FRAME; return Err(XC_TORN_FRAME); }
        self.hdr_metadata = hdr;
        self.captures += 1;
        self.last_code = 0;
        Ok(())
    }
    pub fn captures(&self) -> u64 { self.captures }
    pub fn torn_frames(&self) -> u32 { self.torn_frames }
    pub fn self_check(&mut self) -> bool { self.healthy }
}

pub fn run_vea139_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }
    let mut set = CheckSet::new("vea139_screenshot");
    { let s = ScreenshotService::new(); chk(&mut set, "C139-01", s.healthy(), "err"); }
    { let mut s = ScreenshotService::new(); let r = s.capture(false, false);
        chk(&mut set, "C139-02", r.is_ok() && s.captures() == 1, "err"); }
    { let mut s = ScreenshotService::new(); let r = s.capture(true, false);
        chk(&mut set, "C139-03", r.err() == Some(XC_TORN_FRAME), "err"); }
    { let mut s = ScreenshotService::new(); let r = s.capture(false, true);
        chk(&mut set, "C139-04", r.is_ok(), "err"); }
    { let mut s = ScreenshotService::new(); s.healthy = false;
        let r = s.capture(false, false);
        chk(&mut set, "C139-05", r.err() == Some(XC_SERVICE_DOWN), "err"); }
    { let mut s = ScreenshotService::new(); let _ = s.capture(false, false);
        let _ = s.capture(false, false);
        chk(&mut set, "C139-06", s.captures() == 2, "err"); }
    { let s = ScreenshotService::new(); chk(&mut set, "C139-07", s.torn_frames() == 0, "err"); }
    { let mut s = ScreenshotService::new(); chk(&mut set, "C139-08", s.self_check(), "err"); }
    { let mut s = ScreenshotService::new(); let _ = s.capture(true, false);
        chk(&mut set, "C139-09", s.torn_frames() == 1, "err"); }
    { let s = ScreenshotService::new(); chk(&mut set, "C139-10", s.captures() == 0, "err"); }
    set
}
