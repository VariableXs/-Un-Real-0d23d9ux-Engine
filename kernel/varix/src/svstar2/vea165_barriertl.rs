//! VE-F0165 · 屏障时间线可视化（VE-A 域 · A07 · 目标 300 行）
//!
//! 铁律：屏障可追，等待可量，冲突可查，优化可定。

use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xE8EB;
pub const XC_BARRIER_STALL: u16 = SEG_BASE | 0x01;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BarrierEvent {
    pub id: u32,
    pub wait_us: u32,
}

pub struct BarrierTimeline {
    events: Vec<BarrierEvent>,
    capacity: usize,
    total_wait_us: u64,
    stalls: u32,
    last_code: u16,
}

impl BarrierTimeline {
    pub fn new(capacity: usize) -> Self { Self { events: Vec::new(), capacity, total_wait_us: 0, stalls: 0, last_code: 0 } }
    pub fn last_code(&self) -> u16 { self.last_code }
    pub fn record(&mut self, id: u32, wait_us: u32) -> Result<(), u16> {
        if self.events.len() >= self.capacity { self.last_code = XC_BARRIER_STALL; return Err(XC_BARRIER_STALL); }
        self.events.push(BarrierEvent { id, wait_us });
        self.total_wait_us += wait_us as u64;
        if wait_us > 1000 { self.stalls += 1; }
        self.last_code = 0;
        Ok(())
    }
    pub fn total_wait_us(&self) -> u64 { self.total_wait_us }
    pub fn stalls(&self) -> u32 { self.stalls }
    pub fn event_count(&self) -> usize { self.events.len() }
}

pub fn run_vea165_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }
    let mut set = CheckSet::new("vea165_barriertl");
    { let b = BarrierTimeline::new(10); chk(&mut set, "C165-01", b.event_count() == 0, "err"); }
    { let mut b = BarrierTimeline::new(10); let r = b.record(1, 100);
        chk(&mut set, "C165-02", r.is_ok() && b.event_count() == 1, "err"); }
    { let mut b = BarrierTimeline::new(1); b.record(1, 100); let r = b.record(2, 200);
        chk(&mut set, "C165-03", r.err() == Some(XC_BARRIER_STALL), "err"); }
    { let mut b = BarrierTimeline::new(10); b.record(1, 500); b.record(2, 600);
        chk(&mut set, "C165-04", b.total_wait_us() == 1100, "err"); }
    { let mut b = BarrierTimeline::new(10); b.record(1, 2000);
        chk(&mut set, "C165-05", b.stalls() == 1, "err"); }
    { let mut b = BarrierTimeline::new(10); b.record(1, 500);
        chk(&mut set, "C165-06", b.stalls() == 0, "err"); }
    { let b = BarrierTimeline::new(0); chk(&mut set, "C165-07", b.total_wait_us() == 0, "err"); }
    { let mut b = BarrierTimeline::new(10); b.record(1, 100);
        chk(&mut set, "C165-08", b.last_code() == 0, "err"); }
    { let b = BarrierTimeline::new(10); chk(&mut set, "C165-09", b.stalls() == 0, "err"); }
    { let mut b = BarrierTimeline::new(10); b.record(1, 0);
        chk(&mut set, "C165-10", b.total_wait_us() == 0, "err"); }
    set
}
