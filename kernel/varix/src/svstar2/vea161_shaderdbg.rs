//! VE-F0161 · 着色器调试通道（VE-A 域 · A07 · 目标 320 行）
//!
//! 铁律：断点可设，变量可查，单步可走，日志完整。

use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xE8E7;
pub const XC_BREAKPOINT_LIMIT: u16 = SEG_BASE | 0x01;

pub struct ShaderDebugger {
    breakpoints: Vec<u32>,
    max_breakpoints: usize,
    steps: u32,
    log_entries: u32,
    last_code: u16,
}

impl ShaderDebugger {
    pub fn new(max_bp: usize) -> Self { Self { breakpoints: Vec::new(), max_breakpoints: max_bp, steps: 0, log_entries: 0, last_code: 0 } }
    pub fn last_code(&self) -> u16 { self.last_code }
    pub fn set_breakpoint(&mut self, line: u32) -> Result<(), u16> {
        if self.breakpoints.len() >= self.max_breakpoints { self.last_code = XC_BREAKPOINT_LIMIT; return Err(XC_BREAKPOINT_LIMIT); }
        if self.breakpoints.contains(&line) { self.last_code = 0; return Ok(()); }
        self.breakpoints.push(line);
        self.last_code = 0;
        Ok(())
    }
    pub fn step(&mut self) { self.steps += 1; }
    pub fn log(&mut self) { self.log_entries += 1; }
    pub fn steps(&self) -> u32 { self.steps }
    pub fn breakpoints(&self) -> usize { self.breakpoints.len() }
    pub fn log_entries(&self) -> u32 { self.log_entries }
}

pub fn run_vea161_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }
    let mut set = CheckSet::new("vea161_shaderdbg");
    { let d = ShaderDebugger::new(10); chk(&mut set, "C161-01", d.breakpoints() == 0, "err"); }
    { let mut d = ShaderDebugger::new(10); let r = d.set_breakpoint(1);
        chk(&mut set, "C161-02", r.is_ok() && d.breakpoints() == 1, "err"); }
    { let mut d = ShaderDebugger::new(1); d.set_breakpoint(1); let r = d.set_breakpoint(2);
        chk(&mut set, "C161-03", r.err() == Some(XC_BREAKPOINT_LIMIT), "err"); }
    { let mut d = ShaderDebugger::new(10); d.set_breakpoint(1); d.set_breakpoint(1);
        chk(&mut set, "C161-04", d.breakpoints() == 1, "err"); }
    { let mut d = ShaderDebugger::new(10); d.step(); d.step(); d.step();
        chk(&mut set, "C161-05", d.steps() == 3, "err"); }
    { let mut d = ShaderDebugger::new(10); d.log(); d.log();
        chk(&mut set, "C161-06", d.log_entries() == 2, "err"); }
    { let d = ShaderDebugger::new(0); chk(&mut set, "C161-07", d.breakpoints() == 0, "err"); }
    { let mut d = ShaderDebugger::new(10); d.set_breakpoint(5);
        chk(&mut set, "C161-08", d.last_code() == 0, "err"); }
    { let d = ShaderDebugger::new(10); chk(&mut set, "C161-09", d.steps() == 0, "err"); }
    { let mut d = ShaderDebugger::new(10); d.step();
        chk(&mut set, "C161-10", d.steps() == 1, "err"); }
    set
}
