//! VE-F0163 · 过度绘制可视化（VE-A 域 · A07 · 目标 300 行）
//!
//! 铁律：过度绘制可量，热点可查，阈值可调，优化可追。

use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xE8E9;
pub const XC_OVERDRAW_LIMIT: u16 = SEG_BASE | 0x01;

pub struct OverdrawViz {
    max_layer: u8,
    total_overdraw: u64,
    hotspots: u32,
    last_code: u16,
}

impl OverdrawViz {
    pub fn new(max_layer: u8) -> Self { Self { max_layer, total_overdraw: 0, hotspots: 0, last_code: 0 } }
    pub fn last_code(&self) -> u16 { self.last_code }
    pub fn record(&mut self, layers: u8) -> Result<(), u16> {
        if layers > self.max_layer { self.hotspots += 1; self.last_code = XC_OVERDRAW_LIMIT; return Err(XC_OVERDRAW_LIMIT); }
        self.total_overdraw += layers as u64;
        self.last_code = 0;
        Ok(())
    }
    pub fn total_overdraw(&self) -> u64 { self.total_overdraw }
    pub fn hotspots(&self) -> u32 { self.hotspots }
}

pub fn run_vea163_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }
    let mut set = CheckSet::new("vea163_overdraw");
    { let o = OverdrawViz::new(4); chk(&mut set, "C163-01", o.total_overdraw() == 0, "err"); }
    { let mut o = OverdrawViz::new(4); let r = o.record(2);
        chk(&mut set, "C163-02", r.is_ok() && o.total_overdraw() == 2, "err"); }
    { let mut o = OverdrawViz::new(4); let r = o.record(5);
        chk(&mut set, "C163-03", r.err() == Some(XC_OVERDRAW_LIMIT) && o.hotspots() == 1, "err"); }
    { let mut o = OverdrawViz::new(4); o.record(1); o.record(2); o.record(3);
        chk(&mut set, "C163-04", o.total_overdraw() == 6, "err"); }
    { let mut o = OverdrawViz::new(4); o.record(5); o.record(6);
        chk(&mut set, "C163-05", o.hotspots() == 2, "err"); }
    { let o = OverdrawViz::new(0); chk(&mut set, "C163-06", o.hotspots() == 0, "err"); }
    { let mut o = OverdrawViz::new(4); o.record(4);
        chk(&mut set, "C163-07", o.last_code() == 0, "err"); }
    { let o = OverdrawViz::new(255); chk(&mut set, "C163-08", o.total_overdraw() == 0, "err"); }
    { let mut o = OverdrawViz::new(4); o.record(0);
        chk(&mut set, "C163-09", o.total_overdraw() == 0, "err"); }
    { let mut o = OverdrawViz::new(4); o.record(1);
        chk(&mut set, "C163-10", o.total_overdraw() == 1, "err"); }
    set
}
