//! VE-F0149 · IME 候选窗表面（VE-A 域 · A06 · 目标 340 行）
//!
//! 铁律：独立表面，光标跟随，组合期兜底，延迟预算。

use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xE8DB;
pub const XC_POSITION_LAG: u16 = SEG_BASE | 0x01;
pub const XC_COMPOSE_FAIL: u16 = SEG_BASE | 0x02;

pub struct ImeCandidate {
    x: u32, y: u32,
    position_lag_us: u32,
    budget_us: u32,
    visible: bool,
    last_code: u16,
}

impl ImeCandidate {
    pub fn new(budget_us: u32) -> Self { Self { x: 0, y: 0, position_lag_us: 0, budget_us, visible: false, last_code: 0 } }
    pub fn last_code(&self) -> u16 { self.last_code }
    /// 候选窗跟随光标位置。
    pub fn follow(&mut self, x: u32, y: u32, lag_us: u32) -> Result<(), u16> {
        self.x = x; self.y = y;
        self.position_lag_us = lag_us;
        if lag_us > self.budget_us { self.last_code = XC_POSITION_LAG; return Err(XC_POSITION_LAG); }
        self.last_code = 0;
        Ok(())
    }
    /// 组合期候选窗渲染兜底。
    pub fn compose_fallback(&mut self) { self.visible = true; self.last_code = XC_COMPOSE_FAIL; }
    pub fn show(&mut self) { self.visible = true; }
    pub fn hide(&mut self) { self.visible = false; }
    pub fn visible(&self) -> bool { self.visible }
    pub fn position(&self) -> (u32, u32) { (self.x, self.y) }
}

pub fn run_vea149_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }
    let mut set = CheckSet::new("vea149_ime");
    { let c = ImeCandidate::new(5000); chk(&mut set, "C149-01", !c.visible(), "err"); }
    { let mut c = ImeCandidate::new(5000); let r = c.follow(100, 200, 1000);
        chk(&mut set, "C149-02", r.is_ok() && c.position() == (100, 200), "err"); }
    { let mut c = ImeCandidate::new(5000); let r = c.follow(100, 200, 10000);
        chk(&mut set, "C149-03", r.err() == Some(XC_POSITION_LAG), "err"); }
    { let mut c = ImeCandidate::new(5000); c.show();
        chk(&mut set, "C149-04", c.visible(), "err"); }
    { let mut c = ImeCandidate::new(5000); c.show(); c.hide();
        chk(&mut set, "C149-05", !c.visible(), "err"); }
    { let mut c = ImeCandidate::new(5000); c.compose_fallback();
        chk(&mut set, "C149-06", c.visible() && c.last_code() == XC_COMPOSE_FAIL, "err"); }
    { let mut c = ImeCandidate::new(5000); c.follow(10, 20, 500);
        chk(&mut set, "C149-07", c.position() == (10, 20), "err"); }
    { let c = ImeCandidate::new(5000); chk(&mut set, "C149-08", c.position() == (0, 0), "err"); }
    { let mut c = ImeCandidate::new(0); let r = c.follow(1, 1, 1);
        chk(&mut set, "C149-09", r.err() == Some(XC_POSITION_LAG), "err"); }
    { let mut c = ImeCandidate::new(5000); c.hide();
        chk(&mut set, "C149-10", !c.visible(), "err"); }
    set
}
