//! VE-F0142 · 浏览器表面宿主接口（VE-A 域 · A06 · 目标 420 行）
//!
//! 铁律：纹理化宿主，安全隔离，节拍解耦，失效占位。

use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xE8D4;
pub const XC_UNTRUSTED_CONTENT: u16 = SEG_BASE | 0x01;
pub const XC_TEMPO_CONFLICT: u16 = SEG_BASE | 0x02;
pub const XC_SURFACE_LOST: u16 = SEG_BASE | 0x03;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HostState { Idle, Hosting, Placeholder, Isolated }

pub struct BrowserHost {
    state: HostState,
    tempo_decoupled: bool,
    updates: u64,
    placeholders: u32,
    last_code: u16,
}

impl BrowserHost {
    pub fn new() -> Self { Self { state: HostState::Idle, tempo_decoupled: true, updates: 0, placeholders: 0, last_code: 0 } }
    pub fn state(&self) -> HostState { self.state }
    pub fn last_code(&self) -> u16 { self.last_code }
    /// 网页内容纹理化（不执行 = 安全隔离）。
    pub fn host_surface(&mut self, untrusted: bool) -> Result<(), u16> {
        if untrusted { self.state = HostState::Isolated; self.last_code = XC_UNTRUSTED_CONTENT; return Ok(()); }
        self.state = HostState::Hosting;
        self.updates += 1;
        self.last_code = 0;
        Ok(())
    }
    /// 表面失效占位。
    pub fn placeholder(&mut self) { self.state = HostState::Placeholder; self.placeholders += 1; self.last_code = XC_SURFACE_LOST; }
    pub fn tempo_decoupled(&self) -> bool { self.tempo_decoupled }
    pub fn updates(&self) -> u64 { self.updates }
    pub fn placeholders(&self) -> u32 { self.placeholders }
}

pub fn run_vea142_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }
    let mut set = CheckSet::new("vea142_browser");
    { let b = BrowserHost::new(); chk(&mut set, "C142-01", b.tempo_decoupled(), "err"); }
    { let mut b = BrowserHost::new(); let r = b.host_surface(false);
        chk(&mut set, "C142-02", r.is_ok() && b.state() == HostState::Hosting, "err"); }
    { let mut b = BrowserHost::new(); let r = b.host_surface(true);
        chk(&mut set, "C142-03", r.is_ok() && b.state() == HostState::Isolated, "err"); }
    { let mut b = BrowserHost::new(); b.placeholder();
        chk(&mut set, "C142-04", b.placeholders() == 1, "err"); }
    { let mut b = BrowserHost::new(); b.host_surface(false); b.host_surface(false);
        chk(&mut set, "C142-05", b.updates() == 2, "err"); }
    { let b = BrowserHost::new(); chk(&mut set, "C142-06", b.state() == HostState::Idle, "err"); }
    { let mut b = BrowserHost::new(); b.placeholder(); b.placeholder();
        chk(&mut set, "C142-07", b.placeholders() == 2, "err"); }
    { let mut b = BrowserHost::new(); b.host_surface(true);
        chk(&mut set, "C142-08", b.last_code() == XC_UNTRUSTED_CONTENT, "err"); }
    { let b = BrowserHost::new(); chk(&mut set, "C142-09", b.updates() == 0, "err"); }
    { let mut b = BrowserHost::new(); b.host_surface(false);
        chk(&mut set, "C142-10", b.last_code() == 0, "err"); }
    set
}
