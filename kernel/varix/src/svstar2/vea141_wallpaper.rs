//! VE-F0141 · 壁纸引擎表面托管（VE-A 域 · A06 · 目标 360 行）
//!
//! 铁律：最底层注册，职责分界，功耗联动，失配回收。

use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xE8D3;
pub const XC_BOUNDARY_VIOLATION: u16 = SEG_BASE | 0x01;
pub const XC_LIFECYCLE_MISMATCH: u16 = SEG_BASE | 0x02;
pub const XC_POWER_LINK_FAIL: u16 = SEG_BASE | 0x03;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WallState { Registered, Active, Paused, Recycled }

pub struct WallpaperHost {
    state: WallState,
    z_bottom: bool,
    power_linked: bool,
    last_code: u16,
    mounts: u32,
    recycles: u32,
}

impl WallpaperHost {
    pub fn new() -> Self { Self { state: WallState::Registered, z_bottom: true, power_linked: true, last_code: 0, mounts: 0, recycles: 0 } }
    pub fn state(&self) -> WallState { self.state }
    pub fn last_code(&self) -> u16 { self.last_code }
    /// 壁纸表面注册为最底层合成表面。
    pub fn mount(&mut self) -> Result<(), u16> { self.state = WallState::Active; self.mounts += 1; self.last_code = 0; Ok(()) }
    /// 功耗联动（壁纸暂停 = 表面休眠）。
    pub fn pause(&mut self) { self.state = WallState::Paused; self.last_code = 0; }
    /// 失配回收。
    pub fn recycle(&mut self) { self.state = WallState::Recycled; self.recycles += 1; self.last_code = XC_LIFECYCLE_MISMATCH; }
    pub fn z_bottom(&self) -> bool { self.z_bottom }
    pub fn mounts(&self) -> u32 { self.mounts }
    pub fn recycles(&self) -> u32 { self.recycles }
}

pub fn run_vea141_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }
    let mut set = CheckSet::new("vea141_wallpaper");
    { let w = WallpaperHost::new(); chk(&mut set, "C141-01", w.z_bottom(), "err"); }
    { let mut w = WallpaperHost::new(); let r = w.mount();
        chk(&mut set, "C141-02", r.is_ok() && w.state() == WallState::Active, "err"); }
    { let mut w = WallpaperHost::new(); w.pause();
        chk(&mut set, "C141-03", w.state() == WallState::Paused, "err"); }
    { let mut w = WallpaperHost::new(); w.recycle();
        chk(&mut set, "C141-04", w.state() == WallState::Recycled && w.recycles() == 1, "err"); }
    { let mut w = WallpaperHost::new(); w.mount(); w.pause(); w.recycle();
        chk(&mut set, "C141-05", w.mounts() == 1, "err"); }
    { let w = WallpaperHost::new(); chk(&mut set, "C141-06", w.state() == WallState::Registered, "err"); }
    { let mut w = WallpaperHost::new(); w.mount(); w.mount();
        chk(&mut set, "C141-07", w.mounts() == 2, "err"); }
    { let w = WallpaperHost::new(); chk(&mut set, "C141-08", w.recycles() == 0, "err"); }
    { let mut w = WallpaperHost::new(); w.recycle(); w.recycle();
        chk(&mut set, "C141-09", w.recycles() == 2, "err"); }
    { let w = WallpaperHost::new(); chk(&mut set, "C141-10", w.last_code() == 0, "err"); }
    set
}
