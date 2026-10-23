//! VE-F0147 · 拖放 ghost 表面（VE-A 域 · A06 · 目标 320 行）
//!
//! 铁律：独立合成，贴手承诺，取消消失，回退重绘。

use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xE8D9;
pub const XC_LAG: u16 = SEG_BASE | 0x01;
pub const XC_RESIDUE: u16 = SEG_BASE | 0x02;
pub const XC_COMPOSITE_FAIL: u16 = SEG_BASE | 0x03;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GhostState { Idle, Dragging, Vanishing }

pub struct GhostSurface {
    state: GhostState,
    follow_lag_us: u32,
    budget_us: u32,
    drags: u32,
    last_code: u16,
}

impl GhostSurface {
    pub fn new(budget_us: u32) -> Self { Self { state: GhostState::Idle, follow_lag_us: 0, budget_us, drags: 0, last_code: 0 } }
    pub fn state(&self) -> GhostState { self.state }
    pub fn last_code(&self) -> u16 { self.last_code }
    pub fn begin_drag(&mut self) { self.state = GhostState::Dragging; self.drags += 1; self.last_code = 0; }
    /// ghost 跟手延迟承诺（影子贴手不拖沓）。
    pub fn follow(&mut self, lag_us: u32) -> Result<(), u16> {
        self.follow_lag_us = lag_us;
        if lag_us > self.budget_us { self.last_code = XC_LAG; return Err(XC_LAG); }
        self.last_code = 0;
        Ok(())
    }
    /// 拖放取消的 ghost 消失动画。
    pub fn cancel(&mut self) { self.state = GhostState::Vanishing; self.last_code = 0; }
    pub fn follow_lag_us(&self) -> u32 { self.follow_lag_us }
    pub fn drags(&self) -> u32 { self.drags }
}

pub fn run_vea147_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }
    let mut set = CheckSet::new("vea147_ghost");
    { let g = GhostSurface::new(5000); chk(&mut set, "C147-01", g.state() == GhostState::Idle, "err"); }
    { let mut g = GhostSurface::new(5000); g.begin_drag();
        chk(&mut set, "C147-02", g.state() == GhostState::Dragging, "err"); }
    { let mut g = GhostSurface::new(5000); g.begin_drag(); let r = g.follow(1000);
        chk(&mut set, "C147-03", r.is_ok(), "err"); }
    { let mut g = GhostSurface::new(5000); g.begin_drag(); let r = g.follow(10000);
        chk(&mut set, "C147-04", r.err() == Some(XC_LAG), "err"); }
    { let mut g = GhostSurface::new(5000); g.begin_drag(); g.cancel();
        chk(&mut set, "C147-05", g.state() == GhostState::Vanishing, "err"); }
    { let mut g = GhostSurface::new(5000); g.begin_drag(); g.cancel(); g.begin_drag();
        chk(&mut set, "C147-06", g.drags() == 2, "err"); }
    { let g = GhostSurface::new(5000); chk(&mut set, "C147-07", g.follow_lag_us() == 0, "err"); }
    { let mut g = GhostSurface::new(5000); g.begin_drag(); let _ = g.follow(2000);
        chk(&mut set, "C147-08", g.follow_lag_us() == 2000, "err"); }
    { let g = GhostSurface::new(0); chk(&mut set, "C147-09", g.drags() == 0, "err"); }
    { let mut g = GhostSurface::new(5000); g.cancel();
        chk(&mut set, "C147-10", g.state() == GhostState::Vanishing, "err"); }
    set
}
