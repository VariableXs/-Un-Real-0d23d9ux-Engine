//! VE-F0140 · 系统浮层渲染支持（VE-A 域 · A06 · 目标 340 行）
//!
//! 铁律：独立通道，层级协调，永不遮挡，延迟预算。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址 0xE8D2
pub const SEG_BASE: u16 = 0xE8D2;
pub const XC_LAYER_OCCLUDED: u16 = SEG_BASE | 0x01;
pub const XC_LATENCY_OVER: u16 = SEG_BASE | 0x02;
pub const XC_CHANNEL_FAIL: u16 = SEG_BASE | 0x03;

pub struct OverlayChannel {
    z_order: u32,
    latency_us: u32,
    budget_us: u32,
    visible: bool,
    last_code: u16,
    updates: u64,
}

impl OverlayChannel {
    pub fn new(budget_us: u32) -> Self { Self { z_order: 0xFFFF, latency_us: 0, budget_us, visible: true, last_code: 0, updates: 0 } }
    pub fn last_code(&self) -> u16 { self.last_code }
    /// 浮层永远在最上（游戏全屏也不遮浮层）。
    pub fn ensure_topmost(&mut self) { self.z_order = 0xFFFF; }
    pub fn update(&mut self, latency_us: u32) -> Result<(), u16> {
        self.latency_us = latency_us;
        self.updates += 1;
        if latency_us > self.budget_us { self.last_code = XC_LATENCY_OVER; return Err(XC_LATENCY_OVER); }
        self.last_code = 0;
        Ok(())
    }
    pub fn z_order(&self) -> u32 { self.z_order }
    pub fn latency_us(&self) -> u32 { self.latency_us }
    pub fn updates(&self) -> u64 { self.updates }
    pub fn visible(&self) -> bool { self.visible }
}

pub fn run_vea140_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }
    let mut set = CheckSet::new("vea140_overlay");
    { let c = OverlayChannel::new(16000); chk(&mut set, "C140-01", c.visible(), "err"); }
    { let mut c = OverlayChannel::new(16000); c.ensure_topmost();
        chk(&mut set, "C140-02", c.z_order() == 0xFFFF, "err"); }
    { let mut c = OverlayChannel::new(16000); let r = c.update(1000);
        chk(&mut set, "C140-03", r.is_ok(), "err"); }
    { let mut c = OverlayChannel::new(16000); let r = c.update(20000);
        chk(&mut set, "C140-04", r.err() == Some(XC_LATENCY_OVER), "err"); }
    { let mut c = OverlayChannel::new(16000); c.update(100); c.update(200);
        chk(&mut set, "C140-05", c.updates() == 2, "err"); }
    { let c = OverlayChannel::new(5000); chk(&mut set, "C140-06", c.latency_us() == 0, "err"); }
    { let mut c = OverlayChannel::new(16000); c.ensure_topmost(); c.ensure_topmost();
        chk(&mut set, "C140-07", c.z_order() == 0xFFFF, "err"); }
    { let mut c = OverlayChannel::new(0); let r = c.update(1);
        chk(&mut set, "C140-08", r.err() == Some(XC_LATENCY_OVER), "err"); }
    { let c = OverlayChannel::new(100000); chk(&mut set, "C140-09", c.updates() == 0, "err"); }
    { let mut c = OverlayChannel::new(16000); let _ = c.update(500);
        chk(&mut set, "C140-10", c.last_code() == 0, "err"); }
    set
}
