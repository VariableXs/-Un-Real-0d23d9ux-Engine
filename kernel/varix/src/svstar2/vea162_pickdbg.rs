//! VE-F0162 · 热区拾取调试（VE-A 域 · A07 · 目标 300 行）
//!
//! 铁律：拾取可测，命中可查，穿透可追，边界可调。

use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xE8E8;
pub const XC_PICK_MISS: u16 = SEG_BASE | 0x01;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PickResult {
    pub id: u32,
    pub x: u32,
    pub y: u32,
    pub depth: u8,
}

pub struct PickDebugger {
    picks: Vec<PickResult>,
    misses: u32,
    last_code: u16,
}

impl PickDebugger {
    pub fn new() -> Self { Self { picks: Vec::new(), misses: 0, last_code: 0 } }
    pub fn last_code(&self) -> u16 { self.last_code }
    pub fn pick(&mut self, id: u32, x: u32, y: u32, depth: u8) -> Result<(), u16> {
        self.picks.push(PickResult { id, x, y, depth });
        self.last_code = 0;
        Ok(())
    }
    pub fn miss(&mut self) { self.misses += 1; self.last_code = XC_PICK_MISS; }
    pub fn picks(&self) -> usize { self.picks.len() }
    pub fn misses(&self) -> u32 { self.misses }
}

pub fn run_vea162_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }
    let mut set = CheckSet::new("vea162_pickdbg");
    { let p = PickDebugger::new(); chk(&mut set, "C162-01", p.picks() == 0, "err"); }
    { let mut p = PickDebugger::new(); let r = p.pick(1, 10, 20, 0);
        chk(&mut set, "C162-02", r.is_ok() && p.picks() == 1, "err"); }
    { let mut p = PickDebugger::new(); p.miss();
        chk(&mut set, "C162-03", p.misses() == 1 && p.last_code() == XC_PICK_MISS, "err"); }
    { let mut p = PickDebugger::new(); p.pick(1, 10, 20, 0); p.pick(2, 30, 40, 1);
        chk(&mut set, "C162-04", p.picks() == 2, "err"); }
    { let mut p = PickDebugger::new(); p.miss(); p.miss(); p.miss();
        chk(&mut set, "C162-05", p.misses() == 3, "err"); }
    { let p = PickDebugger::new(); chk(&mut set, "C162-06", p.misses() == 0, "err"); }
    { let mut p = PickDebugger::new(); p.pick(1, 10, 20, 0);
        chk(&mut set, "C162-07", p.last_code() == 0, "err"); }
    { let mut p = PickDebugger::new(); p.pick(1, 10, 20, 0);
        chk(&mut set, "C162-08", p.picks() == 1, "err"); }
    { let p = PickDebugger::new(); chk(&mut set, "C162-09", p.last_code() == 0, "err"); }
    { let mut p = PickDebugger::new(); p.pick(1, 0, 0, 0); p.pick(2, 0, 0, 0); p.pick(3, 0, 0, 0);
        chk(&mut set, "C162-10", p.picks() == 3, "err"); }
    set
}
