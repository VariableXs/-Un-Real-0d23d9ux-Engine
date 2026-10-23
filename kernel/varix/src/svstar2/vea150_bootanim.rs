//! VE-F0150 · 系统开关机动画通道（VE-A 域 · A06 · 目标 340 行）
//!
//! 铁律：独立通道，优先级最高，不可中断，延迟预算。

use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xE8DC;
pub const XC_ANIM_INTERRUPTED: u16 = SEG_BASE | 0x01;
pub const XC_PRIORITY_CONFLICT: u16 = SEG_BASE | 0x02;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BootAnimState { Idle, Playing, Done }

pub struct BootAnimChannel {
    state: BootAnimState,
    priority: u8,
    frames: u64,
    interrupts: u32,
    last_code: u16,
}

impl BootAnimChannel {
    pub fn new() -> Self { Self { state: BootAnimState::Idle, priority: 255, frames: 0, interrupts: 0, last_code: 0 } }
    pub fn state(&self) -> BootAnimState { self.state }
    pub fn last_code(&self) -> u16 { self.last_code }
    pub fn play(&mut self) -> Result<(), u16> { self.state = BootAnimState::Playing; self.last_code = 0; Ok(()) }
    pub fn frame(&mut self) -> Result<(), u16> {
        if self.state != BootAnimState::Playing { self.last_code = XC_ANIM_INTERRUPTED; return Err(XC_ANIM_INTERRUPTED); }
        self.frames += 1;
        self.last_code = 0;
        Ok(())
    }
    /// 开关机动画不可中断。
    pub fn finish(&mut self) { self.state = BootAnimState::Done; self.last_code = 0; }
    pub fn priority(&self) -> u8 { self.priority }
    pub fn frames(&self) -> u64 { self.frames }
}

pub fn run_vea150_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }
    let mut set = CheckSet::new("vea150_bootanim");
    { let b = BootAnimChannel::new(); chk(&mut set, "C150-01", b.state() == BootAnimState::Idle, "err"); }
    { let mut b = BootAnimChannel::new(); let r = b.play();
        chk(&mut set, "C150-02", r.is_ok() && b.state() == BootAnimState::Playing, "err"); }
    { let mut b = BootAnimChannel::new(); b.play(); let r = b.frame();
        chk(&mut set, "C150-03", r.is_ok() && b.frames() == 1, "err"); }
    { let mut b = BootAnimChannel::new(); let r = b.frame();
        chk(&mut set, "C150-04", r.err() == Some(XC_ANIM_INTERRUPTED), "err"); }
    { let mut b = BootAnimChannel::new(); b.play(); b.frame(); b.finish();
        chk(&mut set, "C150-05", b.state() == BootAnimState::Done, "err"); }
    { let b = BootAnimChannel::new(); chk(&mut set, "C150-06", b.priority() == 255, "err"); }
    { let mut b = BootAnimChannel::new(); b.play(); b.frame(); b.frame(); b.frame();
        chk(&mut set, "C150-07", b.frames() == 3, "err"); }
    { let mut b = BootAnimChannel::new(); b.finish();
        chk(&mut set, "C150-08", b.state() == BootAnimState::Done, "err"); }
    { let b = BootAnimChannel::new(); chk(&mut set, "C150-09", b.frames() == 0, "err"); }
    { let mut b = BootAnimChannel::new(); b.play(); b.finish(); let r = b.frame();
        chk(&mut set, "C150-10", r.err() == Some(XC_ANIM_INTERRUPTED), "err"); }
    set
}
