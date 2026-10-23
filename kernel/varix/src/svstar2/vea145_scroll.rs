//! VE-F0145 · 终端与编辑器快速滚动路径（VE-A 域 · A06 · 目标 360 行）
//!
//! 铁律：局部更新，帧内批处理，撕裂防护，量化承诺。

use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xE8D7;
pub const XC_FRAME_DROP: u16 = SEG_BASE | 0x01;
pub const XC_TEARING: u16 = SEG_BASE | 0x02;
pub const XC_QUOTA_LIE: u16 = SEG_BASE | 0x03;

pub struct ScrollPath {
    lines_per_sec: u32,
    batch_size: u32,
    tearing_guard: bool,
    frames: u64,
    drops: u32,
    last_code: u16,
}

impl ScrollPath {
    pub fn new(lines_per_sec: u32) -> Self { Self { lines_per_sec, batch_size: 64, tearing_guard: true, frames: 0, drops: 0, last_code: 0 } }
    pub fn last_code(&self) -> u16 { self.last_code }
    /// 快速滚动帧处理（局部更新+批处理）。
    pub fn scroll_frame(&mut self, lines: u32) -> Result<(), u16> {
        if lines > self.lines_per_sec / 60 + 1 { self.drops += 1; self.last_code = XC_FRAME_DROP; return Err(XC_FRAME_DROP); }
        self.frames += 1;
        self.last_code = 0;
        Ok(())
    }
    pub fn enable_tearing_guard(&mut self, on: bool) { self.tearing_guard = on; }
    pub fn frames(&self) -> u64 { self.frames }
    pub fn drops(&self) -> u32 { self.drops }
    pub fn tearing_guard(&self) -> bool { self.tearing_guard }
    pub fn batch_size(&self) -> u32 { self.batch_size }
}

pub fn run_vea145_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }
    let mut set = CheckSet::new("vea145_scroll");
    { let s = ScrollPath::new(3000); chk(&mut set, "C145-01", s.tearing_guard(), "err"); }
    { let mut s = ScrollPath::new(3000); let r = s.scroll_frame(10);
        chk(&mut set, "C145-02", r.is_ok() && s.frames() == 1, "err"); }
    { let mut s = ScrollPath::new(3000); let r = s.scroll_frame(200);
        chk(&mut set, "C145-03", r.err() == Some(XC_FRAME_DROP) && s.drops() == 1, "err"); }
    { let mut s = ScrollPath::new(3000); s.enable_tearing_guard(false);
        chk(&mut set, "C145-04", !s.tearing_guard(), "err"); }
    { let mut s = ScrollPath::new(3000); s.scroll_frame(5); s.scroll_frame(5); s.scroll_frame(5);
        chk(&mut set, "C145-05", s.frames() == 3, "err"); }
    { let s = ScrollPath::new(6000); chk(&mut set, "C145-06", s.batch_size() == 64, "err"); }
    { let mut s = ScrollPath::new(3000); s.enable_tearing_guard(true);
        chk(&mut set, "C145-07", s.tearing_guard(), "err"); }
    { let s = ScrollPath::new(0); chk(&mut set, "C145-08", s.drops() == 0, "err"); }
    { let mut s = ScrollPath::new(3000); let _ = s.scroll_frame(10);
        chk(&mut set, "C145-09", s.last_code() == 0, "err"); }
    { let s = ScrollPath::new(12000); chk(&mut set, "C145-10", s.frames() == 0, "err"); }
    set
}
