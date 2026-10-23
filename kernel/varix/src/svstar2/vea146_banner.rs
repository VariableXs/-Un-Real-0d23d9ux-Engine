//! VE-F0146 · 通知横幅表面生命周期（VE-A 域 · A06 · 目标 340 行）
//!
//! 铁律：四段全路径，呈现逻辑分界，消失完备，降档。

use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xE8D8;
pub const XC_LIFECYCLE_GAP: u16 = SEG_BASE | 0x01;
pub const XC_LOGIC_MIXED: u16 = SEG_BASE | 0x02;
pub const XC_ANIM_JANK: u16 = SEG_BASE | 0x03;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BannerPhase { Created, Showing, Animating, Gone }

pub struct BannerSurface {
    phase: BannerPhase,
    shows: u32,
    disappears: u32,
    jank_count: u32,
    last_code: u16,
}

impl BannerSurface {
    pub fn new() -> Self { Self { phase: BannerPhase::Created, shows: 0, disappears: 0, jank_count: 0, last_code: 0 } }
    pub fn phase(&self) -> BannerPhase { self.phase }
    pub fn last_code(&self) -> u16 { self.last_code }
    /// 四段全路径：创建→显示→动画→消失。
    pub fn show(&mut self) -> Result<(), u16> { self.phase = BannerPhase::Showing; self.shows += 1; self.last_code = 0; Ok(()) }
    pub fn animate(&mut self, jank: bool) -> Result<(), u16> {
        if jank { self.jank_count += 1; self.last_code = XC_ANIM_JANK; return Err(XC_ANIM_JANK); }
        self.phase = BannerPhase::Animating;
        self.last_code = 0;
        Ok(())
    }
    /// 消失路径完备（横幅不会赖着不走）。
    pub fn dismiss(&mut self) { self.phase = BannerPhase::Gone; self.disappears += 1; self.last_code = 0; }
    pub fn shows(&self) -> u32 { self.shows }
    pub fn disappears(&self) -> u32 { self.disappears }
    pub fn jank_count(&self) -> u32 { self.jank_count }
}

pub fn run_vea146_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }
    let mut set = CheckSet::new("vea146_banner");
    { let b = BannerSurface::new(); chk(&mut set, "C146-01", b.phase() == BannerPhase::Created, "err"); }
    { let mut b = BannerSurface::new(); let r = b.show();
        chk(&mut set, "C146-02", r.is_ok() && b.phase() == BannerPhase::Showing, "err"); }
    { let mut b = BannerSurface::new(); b.show(); let r = b.animate(false);
        chk(&mut set, "C146-03", r.is_ok() && b.phase() == BannerPhase::Animating, "err"); }
    { let mut b = BannerSurface::new(); b.show(); b.animate(false); b.dismiss();
        chk(&mut set, "C146-04", b.phase() == BannerPhase::Gone && b.disappears() == 1, "err"); }
    { let mut b = BannerSurface::new(); b.show(); let r = b.animate(true);
        chk(&mut set, "C146-05", r.err() == Some(XC_ANIM_JANK) && b.jank_count() == 1, "err"); }
    { let mut b = BannerSurface::new(); b.show(); b.dismiss(); b.show(); b.dismiss();
        chk(&mut set, "C146-06", b.shows() == 2 && b.disappears() == 2, "err"); }
    { let b = BannerSurface::new(); chk(&mut set, "C146-07", b.jank_count() == 0, "err"); }
    { let mut b = BannerSurface::new(); b.dismiss();
        chk(&mut set, "C146-08", b.disappears() == 1, "err"); }
    { let b = BannerSurface::new(); chk(&mut set, "C146-09", b.last_code() == 0, "err"); }
    { let mut b = BannerSurface::new(); b.show();
        chk(&mut set, "C146-10", b.shows() == 1, "err"); }
    set
}
