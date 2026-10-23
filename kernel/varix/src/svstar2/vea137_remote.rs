//! VE-F0137 · 远程桌面渲染通路接口（VE-A 域 · A06 · 目标 380 行）
//!
//! 铁律：通路抽象，带宽自适应，延迟诚实，断线重连。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址 0xEAD0（重排：原 0xE8CF 未对齐）
pub const SEG_BASE: u16 = 0xEAD0;
pub const XC_BANDWIDTH_DROP: u16 = SEG_BASE | 0x01;
pub const XC_LATENCY_LIE: u16 = SEG_BASE | 0x02;
pub const XC_CHANNEL_BROKEN: u16 = SEG_BASE | 0x03;
pub const XC_RECONNECT_FAIL: u16 = SEG_BASE | 0x04;

/// 带宽档位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BandwidthTier { High, Medium, Low, Minimal }

/// 远程通路抽象。
pub struct RemoteChannel {
    bandwidth_kbps: u32,
    latency_ms: u32,
    tier: BandwidthTier,
    connected: bool,
    last_code: u16,
    reconnects: u32,
    frames_sent: u64,
    quality_level: u8,
}

impl RemoteChannel {
    pub fn new(bandwidth_kbps: u32) -> Self {
        let tier = if bandwidth_kbps >= 50000 { BandwidthTier::High }
            else if bandwidth_kbps >= 10000 { BandwidthTier::Medium }
            else if bandwidth_kbps >= 2000 { BandwidthTier::Low }
            else { BandwidthTier::Minimal };
        RemoteChannel { bandwidth_kbps, latency_ms: 0, tier, connected: true,
            last_code: 0, reconnects: 0, frames_sent: 0, quality_level: 3 }
    }
    pub fn tier(&self) -> BandwidthTier { self.tier }
    pub fn latency_ms(&self) -> u32 { self.latency_ms }
    pub fn last_code(&self) -> u16 { self.last_code }

    /// 带宽自适应降画质。
    pub fn adapt(&mut self, new_bw: u32) {
        self.bandwidth_kbps = new_bw;
        self.tier = if new_bw >= 50000 { BandwidthTier::High }
            else if new_bw >= 10000 { BandwidthTier::Medium }
            else if new_bw >= 2000 { BandwidthTier::Low }
            else { BandwidthTier::Minimal };
        self.quality_level = match self.tier {
            BandwidthTier::High => 3, BandwidthTier::Medium => 2,
            BandwidthTier::Low => 1, BandwidthTier::Minimal => 0,
        };
    }

    /// 延迟诚实呈现（实测延迟即显示延迟）。
    pub fn report_latency(&mut self, ms: u32) { self.latency_ms = ms; }

    /// 断线重连。
    pub fn reconnect(&mut self) -> Result<(), u16> {
        self.reconnects += 1;
        if self.reconnects > 10 { self.last_code = XC_RECONNECT_FAIL; return Err(XC_RECONNECT_FAIL); }
        self.connected = true;
        self.last_code = 0;
        Ok(())
    }
    pub fn connected(&self) -> bool { self.connected }
    pub fn quality_level(&self) -> u8 { self.quality_level }
    pub fn reconnects(&self) -> u32 { self.reconnects }
    pub fn frames_sent(&self) -> u64 { self.frames_sent }
    pub fn send_frame(&mut self) { self.frames_sent += 1; }
}

pub fn run_vea137_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }
    let mut set = CheckSet::new("vea137_remote");
    { let c = RemoteChannel::new(60000); chk(&mut set, "C137-01", c.tier() == BandwidthTier::High, "err"); }
    { let c = RemoteChannel::new(500); chk(&mut set, "C137-02", c.tier() == BandwidthTier::Minimal, "err"); }
    { let mut c = RemoteChannel::new(60000); c.adapt(500);
        chk(&mut set, "C137-03", c.quality_level() == 0, "err"); }
    { let mut c = RemoteChannel::new(60000); c.report_latency(150);
        chk(&mut set, "C137-04", c.latency_ms() == 150, "err"); }
    { let mut c = RemoteChannel::new(60000);
        for _ in 0..11 { let _ = c.reconnect(); }
        chk(&mut set, "C137-05", c.last_code() == XC_RECONNECT_FAIL, "err"); }
    { let mut c = RemoteChannel::new(60000); c.send_frame(); c.send_frame();
        chk(&mut set, "C137-06", c.frames_sent() == 2, "err"); }
    { let mut c = RemoteChannel::new(60000); let r = c.reconnect();
        chk(&mut set, "C137-07", r.is_ok() && c.connected(), "err"); }
    { let c = RemoteChannel::new(15000); chk(&mut set, "C137-08", c.tier() == BandwidthTier::Medium, "err"); }
    { let mut c = RemoteChannel::new(60000); c.adapt(5000);
        chk(&mut set, "C137-09", c.tier() == BandwidthTier::Low && c.quality_level() == 1, "err"); }
    { let c = RemoteChannel::new(0); chk(&mut set, "C137-10", c.tier() == BandwidthTier::Minimal, "err"); }
    set
}
