//! VE-F0148 · 光标表面独立通道（VE-A 域 · A06 · 目标 320 行）
//!
//! 铁律：独立小表面，高频更新，延迟最优，失效回退。

use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xE8DA;
pub const XC_CHANNEL_FAIL: u16 = SEG_BASE | 0x01;
pub const XC_LATENCY_OVER: u16 = SEG_BASE | 0x02;

pub struct CursorChannel {
    independent: bool,
    latency_us: u32,
    budget_us: u32,
    updates: u64,
    last_code: u16,
}

impl CursorChannel {
    pub fn new(budget_us: u32) -> Self { Self { independent: true, latency_us: 0, budget_us, updates: 0, last_code: 0 } }
    pub fn last_code(&self) -> u16 { self.last_code }
    /// 光标更新不重绘整帧——独立小表面高频更新。
    pub fn update(&mut self, latency_us: u32) -> Result<(), u16> {
        self.latency_us = latency_us;
        self.updates += 1;
        if latency_us > self.budget_us { self.last_code = XC_LATENCY_OVER; return Err(XC_LATENCY_OVER); }
        self.last_code = 0;
        Ok(())
    }
    /// 通道失效回退整帧。
    pub fn fallback(&mut self) { self.independent = false; self.last_code = XC_CHANNEL_FAIL; }
    pub fn independent(&self) -> bool { self.independent }
    pub fn updates(&self) -> u64 { self.updates }
    pub fn latency_us(&self) -> u32 { self.latency_us }
}

pub fn run_vea148_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }
    let mut set = CheckSet::new("vea148_cursor");
    { let c = CursorChannel::new(1000); chk(&mut set, "C148-01", c.independent(), "err"); }
    { let mut c = CursorChannel::new(1000); let r = c.update(500);
        chk(&mut set, "C148-02", r.is_ok() && c.updates() == 1, "err"); }
    { let mut c = CursorChannel::new(1000); let r = c.update(5000);
        chk(&mut set, "C148-03", r.err() == Some(XC_LATENCY_OVER), "err"); }
    { let mut c = CursorChannel::new(1000); c.fallback();
        chk(&mut set, "C148-04", !c.independent() && c.last_code() == XC_CHANNEL_FAIL, "err"); }
    { let mut c = CursorChannel::new(1000); c.update(100); c.update(200); c.update(300);
        chk(&mut set, "C148-05", c.updates() == 3, "err"); }
    { let c = CursorChannel::new(1000); chk(&mut set, "C148-06", c.latency_us() == 0, "err"); }
    { let mut c = CursorChannel::new(1000); let _ = c.update(500);
        chk(&mut set, "C148-07", c.latency_us() == 500, "err"); }
    { let c = CursorChannel::new(0); chk(&mut set, "C148-08", c.updates() == 0, "err"); }
    { let mut c = CursorChannel::new(1000); c.fallback(); c.fallback();
        chk(&mut set, "C148-09", !c.independent(), "err"); }
    { let mut c = CursorChannel::new(1000); let _ = c.update(100);
        chk(&mut set, "C148-10", c.last_code() == 0, "err"); }
    set
}
