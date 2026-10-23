//! VE-F0134 · 虚拟桌面切换表面休眠（VE-A 域 · A06 环境与系统集成组 · 目标 320 行）
//!
//! 铁律：不可见即休眠，恢复即时，录屏兼容，省电诚实。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址 0xEA90（重排：原 0xE8CC 未对齐）
pub const SEG_BASE: u16 = 0xEA90;
pub const XC_WHITE_SCREEN: u16 = SEG_BASE | 0x01;
pub const XC_RECORD_MISS: u16 = SEG_BASE | 0x02;
pub const XC_SLEEP_FAIL: u16 = SEG_BASE | 0x03;
pub const XC_RESTORE_TIMEOUT: u16 = SEG_BASE | 0x04;
pub const XC_BAD_DESKTOP: u16 = SEG_BASE | 0x05;
pub const XC_NO_SURFACE: u16 = SEG_BASE | 0x06;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SurfState { Active, Sleeping, Restoring }

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DeskSurface {
    pub id: u32,
    pub desktop: u32,
    pub state: SurfState,
    pub sleep_count: u32,
    pub restore_us: u32,
    pub rec_keepalive: bool,
}

pub struct DeskSleeper {
    active: u32,
    desktops: u32,
    surfaces: Vec<DeskSurface>,
    recording: bool,
    saved_power: u64,
    restore_budget_us: u32,
    last_code: u16,
    switches: u32,
    white_screens: u32,
}

impl DeskSleeper {
    pub fn new(desktops: u32) -> Result<Self, u16> {
        if desktops == 0 || desktops > 16 { return Err(XC_BAD_DESKTOP); }
        Ok(DeskSleeper { active: 0, desktops, surfaces: Vec::new(), recording: false,
            saved_power: 0, restore_budget_us: 16000, last_code: 0, switches: 0, white_screens: 0 })
    }
    pub fn active_desktop(&self) -> u32 { self.active }
    pub fn last_code(&self) -> u16 { self.last_code }
    pub fn register(&mut self, id: u32, desktop: u32) -> Result<(), u16> {
        if desktop >= self.desktops { self.last_code = XC_BAD_DESKTOP; return Err(XC_BAD_DESKTOP); }
        if self.surfaces.iter().any(|s| s.id == id) { self.last_code = XC_NO_SURFACE; return Err(XC_NO_SURFACE); }
        let active = desktop == self.active;
        self.surfaces.push(DeskSurface { id, desktop,
            state: if active { SurfState::Active } else { SurfState::Sleeping },
            sleep_count: 0, restore_us: 0, rec_keepalive: false });
        self.last_code = 0; Ok(())
    }
    pub fn switch_to<F>(&mut self, desktop: u32, mut restore_fn: F) -> Result<u32, u16>
    where F: FnMut(u32, u32) -> u32 {
        if desktop >= self.desktops { self.last_code = XC_BAD_DESKTOP; return Err(XC_BAD_DESKTOP); }
        if desktop == self.active { self.last_code = 0; return Ok(0); }
        for s in self.surfaces.iter_mut() {
            if s.desktop == self.active && s.state == SurfState::Active && !s.rec_keepalive {
                s.state = SurfState::Sleeping; s.sleep_count += 1; self.saved_power += 1;
            }
        }
        self.active = desktop; self.switches += 1;
        let mut timeout = false;
        for s in self.surfaces.iter_mut() {
            if s.desktop == desktop && s.state == SurfState::Sleeping {
                s.state = SurfState::Restoring;
                let us = restore_fn(s.id, s.restore_us);
                s.restore_us = us; s.state = SurfState::Active;
                if us > self.restore_budget_us { timeout = true; }
            }
        }
        if timeout { self.last_code = XC_RESTORE_TIMEOUT; return Err(XC_RESTORE_TIMEOUT); }
        self.last_code = 0; Ok(self.switches)
    }
    pub fn set_recording(&mut self, on: bool) {
        self.recording = on;
        for s in self.surfaces.iter_mut() { s.rec_keepalive = on; }
        self.last_code = 0;
    }
    pub fn report_white_screen(&mut self, _id: u32) { self.white_screens += 1; self.last_code = XC_WHITE_SCREEN; }
    pub fn active_count(&self) -> usize { self.surfaces.iter().filter(|s| s.state == SurfState::Active).count() }
    pub fn sleeping_count(&self) -> usize { self.surfaces.iter().filter(|s| s.state == SurfState::Sleeping).count() }
    pub fn saved_power(&self) -> u64 { self.saved_power }
    pub fn switches(&self) -> u32 { self.switches }
    pub fn white_screens(&self) -> u32 { self.white_screens }
}

pub fn run_vea134_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("vea134_desksleep");
    { let d = DeskSleeper::new(4); chk(&mut set, "C134-01", d.is_ok() && d.unwrap().active_desktop() == 0, "err"); }
    { let r = DeskSleeper::new(0); chk(&mut set, "C134-02", r.err() == Some(XC_BAD_DESKTOP), "err"); }
    { let mut d = DeskSleeper::new(3).unwrap();
        let _ = d.register(1, 0); let _ = d.register(2, 1);
        let _ = d.switch_to(1, |_, _| 100);
        chk(&mut set, "C134-03", d.sleeping_count() == 1 && d.active_count() == 1, "err"); }
    { let mut d = DeskSleeper::new(3).unwrap();
        let _ = d.register(1, 0); let _ = d.register(2, 1);
        let _ = d.switch_to(1, |_, _| 100); let r = d.switch_to(0, |_, _| 500);
        // 手算：切1使 surf1 入睡(power=1)；切0使 surf2 入睡(power=2)、surf1 恢复。
        // 不可见即休眠——切走后桌面1 的 surf2 必然入睡，active 只剩恢复的 surf1。
        chk(&mut set, "C134-04", r.is_ok() && d.active_count() == 1 && d.sleeping_count() == 1 && d.saved_power() == 2, "err"); }
    { let mut d = DeskSleeper::new(3).unwrap();
        let _ = d.register(1, 0); let _ = d.register(2, 1);
        d.set_recording(true); let _ = d.switch_to(1, |_, _| 100);
        chk(&mut set, "C134-05", d.sleeping_count() == 0, "err"); }
    { let mut d = DeskSleeper::new(3).unwrap();
        let _ = d.register(1, 0); let _ = d.register(2, 1);
        let _ = d.switch_to(1, |_, _| 100); let _ = d.switch_to(0, |_, _| 100);
        // 手算：切1睡 surf1(power=1)，切0睡 surf2(power=2)、恢复 surf1；switches=2。
        chk(&mut set, "C134-06", d.saved_power() >= 2 && d.switches() == 2, "err"); }
    { let mut d = DeskSleeper::new(3).unwrap();
        let _ = d.register(1, 0);
        let _ = d.switch_to(1, |_, _| 100); // 先正常切走，surf1 入睡
        let r = d.switch_to(0, |_, _| 20000); // 恢复 surf1：20000 > 预算 16000
        chk(&mut set, "C134-07", r.err() == Some(XC_RESTORE_TIMEOUT), "err"); }
    { let mut d = DeskSleeper::new(3).unwrap();
        let r = d.switch_to(5, |_, _| 100);
        chk(&mut set, "C134-08", r.err() == Some(XC_BAD_DESKTOP), "err"); }
    { let mut d = DeskSleeper::new(3).unwrap(); d.report_white_screen(1);
        chk(&mut set, "C134-09", d.white_screens() == 1, "err"); }
    { let mut d = DeskSleeper::new(3).unwrap();
        let r = d.switch_to(0, |_, _| 100);
        chk(&mut set, "C134-10", r.is_ok() && r.unwrap() == 0, "err"); }
    set
}
