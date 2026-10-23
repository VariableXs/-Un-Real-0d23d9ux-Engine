//! VE-F0168 · 实验特性开关框架（VE-Lab · 目标 360 行）
//!
//! 铁律：开关可控，灰度可调，回滚可保，数据可采。

use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xE8EE;
pub const XC_FLAG_NOT_FOUND: u16 = SEG_BASE | 0x01;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FeatureFlag {
    pub id: u16,
    pub enabled: bool,
    pub rollout_pct: u8,
}

pub struct LabFramework {
    flags: Vec<FeatureFlag>,
    capacity: usize,
    last_code: u16,
}

impl LabFramework {
    pub fn new(capacity: usize) -> Self { Self { flags: Vec::new(), capacity, last_code: 0 } }
    pub fn last_code(&self) -> u16 { self.last_code }
    pub fn register(&mut self, id: u16) -> Result<(), u16> {
        if self.flags.len() >= self.capacity { self.last_code = XC_FLAG_NOT_FOUND; return Err(XC_FLAG_NOT_FOUND); }
        if self.flags.iter().any(|f| f.id == id) { self.last_code = 0; return Ok(()); }
        self.flags.push(FeatureFlag { id, enabled: false, rollout_pct: 0 });
        self.last_code = 0;
        Ok(())
    }
    pub fn set_enabled(&mut self, id: u16, enabled: bool) -> Result<(), u16> {
        if let Some(f) = self.flags.iter_mut().find(|f| f.id == id) { f.enabled = enabled; self.last_code = 0; return Ok(()); }
        self.last_code = XC_FLAG_NOT_FOUND;
        Err(XC_FLAG_NOT_FOUND)
    }
    pub fn set_rollout(&mut self, id: u16, pct: u8) -> Result<(), u16> {
        if let Some(f) = self.flags.iter_mut().find(|f| f.id == id) { f.rollout_pct = pct.min(100); self.last_code = 0; return Ok(()); }
        self.last_code = XC_FLAG_NOT_FOUND;
        Err(XC_FLAG_NOT_FOUND)
    }
    pub fn flag_count(&self) -> usize { self.flags.len() }
    pub fn is_enabled(&self, id: u16) -> bool { self.flags.iter().find(|f| f.id == id).map(|f| f.enabled).unwrap_or(false) }
}

pub fn run_vea168_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }
    let mut set = CheckSet::new("vea168_velab");
    { let l = LabFramework::new(10); chk(&mut set, "C168-01", l.flag_count() == 0, "err"); }
    { let mut l = LabFramework::new(10); let r = l.register(1);
        chk(&mut set, "C168-02", r.is_ok() && l.flag_count() == 1, "err"); }
    { let mut l = LabFramework::new(1); l.register(1); let r = l.register(2);
        chk(&mut set, "C168-03", r.err() == Some(XC_FLAG_NOT_FOUND), "err"); }
    { let mut l = LabFramework::new(10); l.register(1); l.set_enabled(1, true);
        chk(&mut set, "C168-04", l.is_enabled(1), "err"); }
    { let mut l = LabFramework::new(10); l.register(1); l.set_rollout(1, 50);
        chk(&mut set, "C168-05", l.is_enabled(1) == false, "err"); }
    { let mut l = LabFramework::new(10); l.register(1); l.set_enabled(1, true); l.set_enabled(1, false);
        chk(&mut set, "C168-06", !l.is_enabled(1), "err"); }
    { let mut l = LabFramework::new(10); l.register(1); l.register(1);
        chk(&mut set, "C168-07", l.flag_count() == 1, "err"); }
    { let l = LabFramework::new(10); chk(&mut set, "C168-08", !l.is_enabled(99), "err"); }
    { let mut l = LabFramework::new(10); l.register(1);
        chk(&mut set, "C168-09", l.last_code() == 0, "err"); }
    { let mut l = LabFramework::new(10); let r = l.set_enabled(99, true);
        chk(&mut set, "C168-10", r.err() == Some(XC_FLAG_NOT_FOUND), "err"); }
    set
}
