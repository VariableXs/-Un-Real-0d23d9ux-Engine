//! VE-F0151 · 服务健康检查端点（VE-A 域 · A06 · 目标 300 行）
//!
//! 铁律：定期自检，故障检出，恢复通告，状态可达。

use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xE8DD;
pub const XC_HEALTH_FAIL: u16 = SEG_BASE | 0x01;
pub const XC_RECOVERY_MISS: u16 = SEG_BASE | 0x02;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Health { Healthy, Degraded, Down }

pub struct HealthEndpoint {
    status: Health,
    checks: u64,
    failures: u32,
    recoveries: u32,
    last_code: u16,
}

impl HealthEndpoint {
    pub fn new() -> Self { Self { status: Health::Healthy, checks: 0, failures: 0, recoveries: 0, last_code: 0 } }
    pub fn status(&self) -> Health { self.status }
    pub fn last_code(&self) -> u16 { self.last_code }
    pub fn check(&mut self, ok: bool) -> Health {
        self.checks += 1;
        if ok {
            if self.status != Health::Healthy { self.recoveries += 1; }
            self.status = Health::Healthy;
            self.last_code = 0;
        } else {
            self.failures += 1;
            self.status = Health::Down;
            self.last_code = XC_HEALTH_FAIL;
        }
        self.status
    }
    pub fn checks(&self) -> u64 { self.checks }
    pub fn failures(&self) -> u32 { self.failures }
    pub fn recoveries(&self) -> u32 { self.recoveries }
}

pub fn run_vea151_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }
    let mut set = CheckSet::new("vea151_health");
    { let h = HealthEndpoint::new(); chk(&mut set, "C151-01", h.status() == Health::Healthy, "err"); }
    { let mut h = HealthEndpoint::new(); let s = h.check(true);
        chk(&mut set, "C151-02", s == Health::Healthy, "err"); }
    { let mut h = HealthEndpoint::new(); let s = h.check(false);
        chk(&mut set, "C151-03", s == Health::Down && h.failures() == 1, "err"); }
    { let mut h = HealthEndpoint::new(); h.check(false); h.check(true);
        chk(&mut set, "C151-04", h.status() == Health::Healthy && h.recoveries() == 1, "err"); }
    { let mut h = HealthEndpoint::new(); h.check(true); h.check(true); h.check(true);
        chk(&mut set, "C151-05", h.checks() == 3, "err"); }
    { let h = HealthEndpoint::new(); chk(&mut set, "C151-06", h.failures() == 0, "err"); }
    { let mut h = HealthEndpoint::new(); h.check(false); h.check(false);
        chk(&mut set, "C151-07", h.failures() == 2, "err"); }
    { let mut h = HealthEndpoint::new(); h.check(false); h.check(true); h.check(false); h.check(true);
        chk(&mut set, "C151-08", h.recoveries() == 2, "err"); }
    { let h = HealthEndpoint::new(); chk(&mut set, "C151-09", h.last_code() == 0, "err"); }
    { let mut h = HealthEndpoint::new(); h.check(false);
        chk(&mut set, "C151-10", h.last_code() == XC_HEALTH_FAIL, "err"); }
    set
}
