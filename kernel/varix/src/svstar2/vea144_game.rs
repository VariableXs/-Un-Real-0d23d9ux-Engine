//! VE-F0144 · 游戏表面专用路径（VE-A 域 · A06 · 目标 380 行）
//!
//! 铁律：独占优化，准入公开，动态切换，延迟实测。

use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xE8D6;
pub const XC_ADMISSION_DENY: u16 = SEG_BASE | 0x01;
pub const XC_SWITCH_FAIL: u16 = SEG_BASE | 0x02;
pub const XC_LATENCY_FAKE: u16 = SEG_BASE | 0x03;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GamePath { Exclusive, Shared }

pub struct GameSurface {
    path: GamePath,
    latency_us: u32,
    switches: u32,
    last_code: u16,
}

impl GameSurface {
    pub fn new() -> Self { Self { path: GamePath::Shared, latency_us: 0, switches: 0, last_code: 0 } }
    pub fn path(&self) -> GamePath { self.path }
    pub fn last_code(&self) -> u16 { self.last_code }
    /// 准入判定（规则公开）。
    pub fn admit_exclusive(&mut self, is_fullscreen: bool) -> Result<(), u16> {
        if !is_fullscreen { self.last_code = XC_ADMISSION_DENY; return Err(XC_ADMISSION_DENY); }
        self.path = GamePath::Exclusive;
        self.switches += 1;
        self.last_code = 0;
        Ok(())
    }
    /// 专用→共享动态切换。
    pub fn switch_shared(&mut self) { self.path = GamePath::Shared; self.switches += 1; self.last_code = 0; }
    pub fn latency_us(&self) -> u32 { self.latency_us }
    pub fn report_latency(&mut self, us: u32) { self.latency_us = us; }
    pub fn switches(&self) -> u32 { self.switches }
}

pub fn run_vea144_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }
    let mut set = CheckSet::new("vea144_game");
    { let g = GameSurface::new(); chk(&mut set, "C144-01", g.path() == GamePath::Shared, "err"); }
    { let mut g = GameSurface::new(); let r = g.admit_exclusive(true);
        chk(&mut set, "C144-02", r.is_ok() && g.path() == GamePath::Exclusive, "err"); }
    { let mut g = GameSurface::new(); let r = g.admit_exclusive(false);
        chk(&mut set, "C144-03", r.err() == Some(XC_ADMISSION_DENY), "err"); }
    { let mut g = GameSurface::new(); g.admit_exclusive(true); g.switch_shared();
        chk(&mut set, "C144-04", g.path() == GamePath::Shared && g.switches() == 2, "err"); }
    { let mut g = GameSurface::new(); g.report_latency(500);
        chk(&mut set, "C144-05", g.latency_us() == 500, "err"); }
    { let mut g = GameSurface::new(); g.admit_exclusive(true); g.admit_exclusive(true);
        chk(&mut set, "C144-06", g.switches() == 2, "err"); }
    { let g = GameSurface::new(); chk(&mut set, "C144-07", g.latency_us() == 0, "err"); }
    { let mut g = GameSurface::new(); g.switch_shared();
        chk(&mut set, "C144-08", g.switches() == 1, "err"); }
    { let g = GameSurface::new(); chk(&mut set, "C144-09", g.last_code() == 0, "err"); }
    { let mut g = GameSurface::new(); let _ = g.admit_exclusive(true);
        chk(&mut set, "C144-10", g.last_code() == 0, "err"); }
    set
}
