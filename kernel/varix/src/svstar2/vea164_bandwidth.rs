//! VE-F0164 · 带宽可视化（VE-A 域 · A07 · 目标 300 行）
//!
//! 铁律：带宽可量，峰值可查，瓶颈可定，优化可追。

use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xE8EA;
pub const XC_BANDWIDTH_SATURATED: u16 = SEG_BASE | 0x01;

pub struct BandwidthViz {
    peak_mbps: u32,
    current_mbps: u32,
    capacity_mbps: u32,
    samples: u64,
    last_code: u16,
}

impl BandwidthViz {
    pub fn new(capacity_mbps: u32) -> Self { Self { peak_mbps: 0, current_mbps: 0, capacity_mbps, samples: 0, last_code: 0 } }
    pub fn last_code(&self) -> u16 { self.last_code }
    pub fn sample(&mut self, mbps: u32) -> Result<(), u16> {
        self.current_mbps = mbps;
        self.samples += 1;
        if mbps > self.peak_mbps { self.peak_mbps = mbps; }
        if mbps >= self.capacity_mbps { self.last_code = XC_BANDWIDTH_SATURATED; return Err(XC_BANDWIDTH_SATURATED); }
        self.last_code = 0;
        Ok(())
    }
    pub fn peak_mbps(&self) -> u32 { self.peak_mbps }
    pub fn current_mbps(&self) -> u32 { self.current_mbps }
    pub fn samples(&self) -> u64 { self.samples }
    pub fn utilization(&self) -> f32 {
        if self.capacity_mbps == 0 { return 0.0; }
        self.current_mbps as f32 / self.capacity_mbps as f32
    }
}

pub fn run_vea164_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }
    let mut set = CheckSet::new("vea164_bandwidth");
    { let b = BandwidthViz::new(1000); chk(&mut set, "C164-01", b.peak_mbps() == 0, "err"); }
    { let mut b = BandwidthViz::new(1000); let r = b.sample(500);
        chk(&mut set, "C164-02", r.is_ok() && b.current_mbps() == 500, "err"); }
    { let mut b = BandwidthViz::new(1000); let r = b.sample(1000);
        chk(&mut set, "C164-03", r.err() == Some(XC_BANDWIDTH_SATURATED), "err"); }
    { let mut b = BandwidthViz::new(1000); b.sample(500); b.sample(800);
        chk(&mut set, "C164-04", b.peak_mbps() == 800, "err"); }
    { let mut b = BandwidthViz::new(1000); b.sample(250);
        chk(&mut set, "C164-05", b.utilization() > 0.24 && b.utilization() < 0.26, "err"); }
    { let mut b = BandwidthViz::new(1000); b.sample(100); b.sample(200);
        chk(&mut set, "C164-06", b.samples() == 2, "err"); }
    { let b = BandwidthViz::new(0); chk(&mut set, "C164-07", b.utilization() == 0.0, "err"); }
    { let mut b = BandwidthViz::new(1000); b.sample(100);
        chk(&mut set, "C164-08", b.last_code() == 0, "err"); }
    { let b = BandwidthViz::new(1000); chk(&mut set, "C164-09", b.samples() == 0, "err"); }
    { let mut b = BandwidthViz::new(1000); b.sample(500); b.sample(300);
        chk(&mut set, "C164-10", b.current_mbps() == 300 && b.peak_mbps() == 500, "err"); }
    set
}
