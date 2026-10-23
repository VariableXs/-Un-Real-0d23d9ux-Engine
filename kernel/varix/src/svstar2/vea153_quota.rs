//! VE-F0153 · 配额申请与内核仲裁对接（VE-A 域 · A06 · 目标 320 行）
//!
//! 铁律：申请透明，仲裁公平，超限拒绝，用量诚实。

use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xE8DF;
pub const XC_QUOTA_DENY: u16 = SEG_BASE | 0x01;
pub const XC_OVER_LIMIT: u16 = SEG_BASE | 0x02;

pub struct QuotaBroker {
    total: u64,
    used: u64,
    grants: u32,
    denies: u32,
    last_code: u16,
}

impl QuotaBroker {
    pub fn new(total: u64) -> Self { Self { total, used: 0, grants: 0, denies: 0, last_code: 0 } }
    pub fn last_code(&self) -> u16 { self.last_code }
    pub fn request(&mut self, amount: u64) -> Result<(), u16> {
        if self.used + amount > self.total { self.denies += 1; self.last_code = XC_QUOTA_DENY; return Err(XC_QUOTA_DENY); }
        self.used += amount;
        self.grants += 1;
        self.last_code = 0;
        Ok(())
    }
    pub fn release(&mut self, amount: u64) { self.used = self.used.saturating_sub(amount); }
    pub fn used(&self) -> u64 { self.used }
    pub fn remaining(&self) -> u64 { self.total.saturating_sub(self.used) }
    pub fn grants(&self) -> u32 { self.grants }
    pub fn denies(&self) -> u32 { self.denies }
}

pub fn run_vea153_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }
    let mut set = CheckSet::new("vea153_quota");
    { let q = QuotaBroker::new(1000); chk(&mut set, "C153-01", q.remaining() == 1000, "err"); }
    { let mut q = QuotaBroker::new(1000); let r = q.request(500);
        chk(&mut set, "C153-02", r.is_ok() && q.used() == 500, "err"); }
    { let mut q = QuotaBroker::new(1000); let r = q.request(2000);
        chk(&mut set, "C153-03", r.err() == Some(XC_QUOTA_DENY) && q.denies() == 1, "err"); }
    { let mut q = QuotaBroker::new(1000); q.request(500); q.release(300);
        chk(&mut set, "C153-04", q.used() == 200, "err"); }
    { let mut q = QuotaBroker::new(1000); q.request(500); q.request(500);
        chk(&mut set, "C153-05", q.grants() == 2, "err"); }
    { let mut q = QuotaBroker::new(1000); q.release(500);
        chk(&mut set, "C153-06", q.used() == 0, "err"); }
    { let q = QuotaBroker::new(0); chk(&mut set, "C153-07", q.remaining() == 0, "err"); }
    { let mut q = QuotaBroker::new(100); q.request(100);
        chk(&mut set, "C153-08", q.remaining() == 0, "err"); }
    { let q = QuotaBroker::new(1000); chk(&mut set, "C153-09", q.grants() == 0, "err"); }
    { let mut q = QuotaBroker::new(1000); let _ = q.request(500);
        chk(&mut set, "C153-10", q.last_code() == 0, "err"); }
    set
}
