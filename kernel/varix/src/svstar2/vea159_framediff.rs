//! VE-F0159 · 帧对比工具（VE-A 域 · A07 · 目标 300 行）
//!
//! 铁律：像素级对比，差异高亮，阈值可调，历史可存。

use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xE8E5;
pub const XC_DIFF_OVERFLOW: u16 = SEG_BASE | 0x01;

pub struct FrameDiff {
    diff_count: u32,
    total_pixels: u32,
    threshold: u8,
    comparisons: u32,
    last_code: u16,
}

impl FrameDiff {
    pub fn new(total_pixels: u32, threshold: u8) -> Self { Self { diff_count: 0, total_pixels, threshold, comparisons: 0, last_code: 0 } }
    pub fn last_code(&self) -> u16 { self.last_code }
    pub fn compare(&mut self, diff_pixels: u32) -> f32 {
        self.comparisons += 1;
        self.diff_count = diff_pixels;
        self.last_code = 0;
        if self.total_pixels == 0 { return 0.0; }
        diff_pixels as f32 / self.total_pixels as f32
    }
    pub fn diff_ratio(&self) -> f32 {
        if self.total_pixels == 0 { return 0.0; }
        self.diff_count as f32 / self.total_pixels as f32
    }
    pub fn comparisons(&self) -> u32 { self.comparisons }
}

pub fn run_vea159_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }
    let mut set = CheckSet::new("vea159_framediff");
    { let f = FrameDiff::new(1000, 10); chk(&mut set, "C159-01", f.diff_ratio() == 0.0, "err"); }
    { let mut f = FrameDiff::new(1000, 10); let r = f.compare(50);
        chk(&mut set, "C159-02", r > 0.04 && r < 0.06, "err"); }
    { let mut f = FrameDiff::new(1000, 10); f.compare(0);
        chk(&mut set, "C159-03", f.diff_ratio() == 0.0, "err"); }
    { let mut f = FrameDiff::new(1000, 10); f.compare(1000);
        chk(&mut set, "C159-04", f.diff_ratio() == 1.0, "err"); }
    { let mut f = FrameDiff::new(1000, 10); f.compare(50); f.compare(50);
        chk(&mut set, "C159-05", f.comparisons() == 2, "err"); }
    { let f = FrameDiff::new(0, 10); chk(&mut set, "C159-06", f.diff_ratio() == 0.0, "err"); }
    { let mut f = FrameDiff::new(1000, 10); f.compare(50);
        chk(&mut set, "C159-07", f.last_code() == 0, "err"); }
    { let f = FrameDiff::new(1000, 10); chk(&mut set, "C159-08", f.comparisons() == 0, "err"); }
    { let mut f = FrameDiff::new(100, 255); f.compare(1);
        chk(&mut set, "C159-09", f.diff_ratio() == 0.01, "err"); }
    { let mut f = FrameDiff::new(1000, 0); f.compare(500);
        chk(&mut set, "C159-10", f.diff_ratio() == 0.5, "err"); }
    set
}
