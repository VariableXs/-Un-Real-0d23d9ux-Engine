//! VE-F0154 · 引擎全局错误聚合上报（VE-A 域 · A06 · 目标 340 行）
//!
//! 铁律：错误聚合，分类上报，去重合并，频率限制。

use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xE8E0;
pub const XC_FLOOD: u16 = SEG_BASE | 0x01;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ErrorRecord {
    pub code: u16,
    pub count: u32,
}

pub struct ErrorAggregator {
    records: Vec<ErrorRecord>,
    capacity: usize,
    rate_limit: u32,
    window_count: u32,
    last_code: u16,
}

impl ErrorAggregator {
    pub fn new(capacity: usize, rate_limit: u32) -> Self { Self { records: Vec::new(), capacity, rate_limit, window_count: 0, last_code: 0 } }
    pub fn last_code(&self) -> u16 { self.last_code }
    /// 上报错误（同码去重合并）。
    pub fn report(&mut self, code: u16) -> Result<(), u16> {
        self.window_count += 1;
        if self.window_count > self.rate_limit { self.last_code = XC_FLOOD; return Err(XC_FLOOD); }
        if let Some(r) = self.records.iter_mut().find(|r| r.code == code) { r.count += 1; self.last_code = 0; return Ok(()); }
        if self.records.len() >= self.capacity { self.last_code = XC_FLOOD; return Err(XC_FLOOD); }
        self.records.push(ErrorRecord { code, count: 1 });
        self.last_code = 0;
        Ok(())
    }
    pub fn total_errors(&self) -> u32 { self.records.iter().map(|r| r.count).sum() }
    pub fn unique_codes(&self) -> usize { self.records.len() }
    pub fn reset_window(&mut self) { self.window_count = 0; }
}

pub fn run_vea154_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }
    let mut set = CheckSet::new("vea154_erragg");
    { let e = ErrorAggregator::new(10, 100); chk(&mut set, "C154-01", e.total_errors() == 0, "err"); }
    { let mut e = ErrorAggregator::new(10, 100); let r = e.report(0xE001);
        chk(&mut set, "C154-02", r.is_ok() && e.total_errors() == 1, "err"); }
    { let mut e = ErrorAggregator::new(10, 100); e.report(0xE001); e.report(0xE001);
        chk(&mut set, "C154-03", e.unique_codes() == 1 && e.total_errors() == 2, "err"); }
    { let mut e = ErrorAggregator::new(10, 100); e.report(0xE001); e.report(0xE002);
        chk(&mut set, "C154-04", e.unique_codes() == 2, "err"); }
    { let mut e = ErrorAggregator::new(1, 100); e.report(0xE001); let r = e.report(0xE002);
        chk(&mut set, "C154-05", r.err() == Some(XC_FLOOD), "err"); }
    { let mut e = ErrorAggregator::new(10, 1); e.report(0xE001); let r = e.report(0xE002);
        chk(&mut set, "C154-06", r.err() == Some(XC_FLOOD), "err"); }
    { let mut e = ErrorAggregator::new(10, 100); e.report(0xE001); e.reset_window();
        chk(&mut set, "C154-07", e.total_errors() == 1, "err"); }
    { let e = ErrorAggregator::new(10, 100); chk(&mut set, "C154-08", e.unique_codes() == 0, "err"); }
    { let mut e = ErrorAggregator::new(10, 100); e.report(0xE001);
        chk(&mut set, "C154-09", e.last_code() == 0, "err"); }
    { let mut e = ErrorAggregator::new(10, 100); for _ in 0..5 { e.report(0xE001); }
        chk(&mut set, "C154-10", e.total_errors() == 5, "err"); }
    set
}
