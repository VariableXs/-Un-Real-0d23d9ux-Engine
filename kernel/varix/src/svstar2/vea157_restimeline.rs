//! VE-F0157 · 资源历史时间线（VE-A 域 · A07 · 目标 320 行）
//!
//! 铁律：创建/销毁可追溯，状态变迁记录，内存峰值统计。

use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xE8E3;
pub const XC_HISTORY_OVERFLOW: u16 = SEG_BASE | 0x01;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ResEvent {
    pub id: u32,
    pub timestamp: u64,
    pub created: bool,
}

pub struct ResourceTimeline {
    events: Vec<ResEvent>,
    capacity: usize,
    live_count: u32,
    peak_count: u32,
    last_code: u16,
}

impl ResourceTimeline {
    pub fn new(capacity: usize) -> Self { Self { events: Vec::new(), capacity, live_count: 0, peak_count: 0, last_code: 0 } }
    pub fn last_code(&self) -> u16 { self.last_code }
    pub fn record_create(&mut self, id: u32, ts: u64) -> Result<(), u16> {
        if self.events.len() >= self.capacity { self.last_code = XC_HISTORY_OVERFLOW; return Err(XC_HISTORY_OVERFLOW); }
        self.events.push(ResEvent { id, timestamp: ts, created: true });
        self.live_count += 1;
        if self.live_count > self.peak_count { self.peak_count = self.live_count; }
        self.last_code = 0;
        Ok(())
    }
    pub fn record_destroy(&mut self, id: u32, ts: u64) -> Result<(), u16> {
        if self.events.len() >= self.capacity { self.last_code = XC_HISTORY_OVERFLOW; return Err(XC_HISTORY_OVERFLOW); }
        self.events.push(ResEvent { id, timestamp: ts, created: false });
        self.live_count = self.live_count.saturating_sub(1);
        self.last_code = 0;
        Ok(())
    }
    pub fn live_count(&self) -> u32 { self.live_count }
    pub fn peak_count(&self) -> u32 { self.peak_count }
    pub fn event_count(&self) -> usize { self.events.len() }
}

pub fn run_vea157_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }
    let mut set = CheckSet::new("vea157_restimeline");
    { let t = ResourceTimeline::new(10); chk(&mut set, "C157-01", t.live_count() == 0, "err"); }
    { let mut t = ResourceTimeline::new(10); let r = t.record_create(1, 100);
        chk(&mut set, "C157-02", r.is_ok() && t.live_count() == 1, "err"); }
    { let mut t = ResourceTimeline::new(10); t.record_create(1, 100); t.record_destroy(1, 200);
        chk(&mut set, "C157-03", t.live_count() == 0, "err"); }
    { let mut t = ResourceTimeline::new(10); t.record_create(1, 100); t.record_create(2, 200);
        chk(&mut set, "C157-04", t.peak_count() == 2, "err"); }
    { let mut t = ResourceTimeline::new(1); t.record_create(1, 100); let r = t.record_create(2, 200);
        chk(&mut set, "C157-05", r.err() == Some(XC_HISTORY_OVERFLOW), "err"); }
    { let mut t = ResourceTimeline::new(10); t.record_create(1, 100); t.record_create(2, 200); t.record_destroy(1, 300);
        chk(&mut set, "C157-06", t.live_count() == 1 && t.peak_count() == 2, "err"); }
    { let mut t = ResourceTimeline::new(10); t.record_create(1, 100);
        chk(&mut set, "C157-07", t.event_count() == 1, "err"); }
    { let mut t = ResourceTimeline::new(10); t.record_destroy(1, 100);
        chk(&mut set, "C157-08", t.live_count() == 0, "err"); }
    { let t = ResourceTimeline::new(10); chk(&mut set, "C157-09", t.peak_count() == 0, "err"); }
    { let mut t = ResourceTimeline::new(10); t.record_create(1, 100);
        chk(&mut set, "C157-10", t.last_code() == 0, "err"); }
    set
}
