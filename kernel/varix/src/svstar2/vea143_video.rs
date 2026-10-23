//! VE-F0143 · 视频播放表面专用路径（VE-A 域 · A06 · 目标 380 行）
//!
//! 铁律：帧直通，HDR 衔接，回退路径，断链检出。

use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xE8D5;
pub const XC_PASSTHROUGH_FAIL: u16 = SEG_BASE | 0x01;
pub const XC_HDR_BREAK: u16 = SEG_BASE | 0x02;
pub const XC_PATH_DRIFT: u16 = SEG_BASE | 0x03;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VideoPath { Passthrough, Generic, Fallback }

pub struct VideoSurface {
    path: VideoPath,
    hdr_connected: bool,
    frames: u64,
    fallbacks: u32,
    last_code: u16,
}

impl VideoSurface {
    pub fn new(hdr: bool) -> Self { Self { path: VideoPath::Passthrough, hdr_connected: hdr, frames: 0, fallbacks: 0, last_code: 0 } }
    pub fn path(&self) -> VideoPath { self.path }
    pub fn last_code(&self) -> u16 { self.last_code }
    /// 视频帧直通（不过通用渲染管线）。
    pub fn present_passthrough(&mut self, hdr_ok: bool) -> Result<(), u16> {
        if !hdr_ok && self.hdr_connected { self.last_code = XC_HDR_BREAK; return Err(XC_HDR_BREAK); }
        self.frames += 1;
        self.last_code = 0;
        Ok(())
    }
    /// 直通失败回退通用路径。
    pub fn fallback(&mut self) { self.path = VideoPath::Fallback; self.fallbacks += 1; self.last_code = XC_PASSTHROUGH_FAIL; }
    pub fn frames(&self) -> u64 { self.frames }
    pub fn fallbacks(&self) -> u32 { self.fallbacks }
    pub fn hdr_connected(&self) -> bool { self.hdr_connected }
}

pub fn run_vea143_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }
    let mut set = CheckSet::new("vea143_video");
    { let v = VideoSurface::new(true); chk(&mut set, "C143-01", v.path() == VideoPath::Passthrough, "err"); }
    { let mut v = VideoSurface::new(true); let r = v.present_passthrough(true);
        chk(&mut set, "C143-02", r.is_ok() && v.frames() == 1, "err"); }
    { let mut v = VideoSurface::new(true); let r = v.present_passthrough(false);
        chk(&mut set, "C143-03", r.err() == Some(XC_HDR_BREAK), "err"); }
    { let mut v = VideoSurface::new(false); let r = v.present_passthrough(false);
        chk(&mut set, "C143-04", r.is_ok(), "err"); }
    { let mut v = VideoSurface::new(true); v.fallback();
        chk(&mut set, "C143-05", v.path() == VideoPath::Fallback && v.fallbacks() == 1, "err"); }
    { let mut v = VideoSurface::new(false); v.present_passthrough(true); v.present_passthrough(true);
        chk(&mut set, "C143-06", v.frames() == 2, "err"); }
    { let v = VideoSurface::new(true); chk(&mut set, "C143-07", v.hdr_connected(), "err"); }
    { let mut v = VideoSurface::new(true); v.fallback(); v.fallback();
        chk(&mut set, "C143-08", v.fallbacks() == 2, "err"); }
    { let v = VideoSurface::new(false); chk(&mut set, "C143-09", v.frames() == 0, "err"); }
    { let mut v = VideoSurface::new(true); let _ = v.present_passthrough(true);
        chk(&mut set, "C143-10", v.last_code() == 0, "err"); }
    set
}
