//! VE-F0158 · 像素历史调试器（VE-A 域 · A07 · 目标 320 行）
//!
//! 铁律：像素可追溯，绘制调用关联，覆盖历史完整。

use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xE8E4;
pub const XC_PIXEL_NOT_FOUND: u16 = SEG_BASE | 0x01;
pub const XC_HISTORY_TRUNCATED: u16 = SEG_BASE | 0x02;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PixelOp {
    pub draw_call: u32,
    pub old_value: u32,
    pub new_value: u32,
}

pub struct PixelHistory {
    ops: Vec<PixelOp>,
    capacity: usize,
    truncated: bool,
    last_code: u16,
}

impl PixelHistory {
    pub fn new(capacity: usize) -> Self { Self { ops: Vec::new(), capacity, truncated: false, last_code: 0 } }
    pub fn last_code(&self) -> u16 { self.last_code }
    pub fn record(&mut self, draw_call: u32, old_val: u32, new_val: u32) -> Result<(), u16> {
        if self.ops.len() >= self.capacity { self.truncated = true; self.last_code = XC_HISTORY_TRUNCATED; return Err(XC_HISTORY_TRUNCATED); }
        self.ops.push(PixelOp { draw_call, old_value: old_val, new_value: new_val });
        self.last_code = 0;
        Ok(())
    }
    pub fn ops(&self) -> usize { self.ops.len() }
    pub fn truncated(&self) -> bool { self.truncated }
    pub fn last_value(&self) -> Option<u32> { self.ops.last().map(|o| o.new_value) }
}

pub fn run_vea158_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }
    let mut set = CheckSet::new("vea158_pixelhist");
    { let p = PixelHistory::new(10); chk(&mut set, "C158-01", p.ops() == 0, "err"); }
    { let mut p = PixelHistory::new(10); let r = p.record(1, 0xFF, 0x00);
        chk(&mut set, "C158-02", r.is_ok() && p.ops() == 1, "err"); }
    { let mut p = PixelHistory::new(1); p.record(1, 0, 1); let r = p.record(2, 1, 2);
        chk(&mut set, "C158-03", r.err() == Some(XC_HISTORY_TRUNCATED) && p.truncated(), "err"); }
    { let mut p = PixelHistory::new(10); p.record(1, 0, 1); p.record(2, 1, 2);
        chk(&mut set, "C158-04", p.last_value() == Some(2), "err"); }
    { let p = PixelHistory::new(10); chk(&mut set, "C158-05", p.last_value().is_none(), "err"); }
    { let mut p = PixelHistory::new(10); p.record(1, 0, 1); p.record(2, 1, 0); p.record(3, 0, 1);
        chk(&mut set, "C158-06", p.ops() == 3, "err"); }
    { let mut p = PixelHistory::new(10); p.record(1, 0, 1);
        chk(&mut set, "C158-07", !p.truncated(), "err"); }
    { let p = PixelHistory::new(0); chk(&mut set, "C158-08", p.ops() == 0, "err"); }
    { let mut p = PixelHistory::new(10); p.record(1, 0, 1);
        chk(&mut set, "C158-09", p.last_code() == 0, "err"); }
    { let mut p = PixelHistory::new(10); p.record(1, 0, 1);
        chk(&mut set, "C158-10", p.last_value() == Some(1), "err"); }
    set
}
