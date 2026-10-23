//! VE-F0155 · VE-B 驱动矩阵接口冻结（VE-A 域 · A06 · 目标 320 行）
//!
//! 铁律：接口冻结，版本锁定，变更审计，兼容保证。

use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xE8E1;
pub const XC_FREEZE_VIOLATION: u16 = SEG_BASE | 0x01;
pub const XC_VERSION_DRIFT: u16 = SEG_BASE | 0x02;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IfaceVersion {
    pub major: u8,
    pub minor: u8,
}

pub struct MatrixFreeze {
    frozen: bool,
    version: IfaceVersion,
    change_attempts: u32,
    last_code: u16,
}

impl MatrixFreeze {
    pub fn new(major: u8, minor: u8) -> Self { Self { frozen: false, version: IfaceVersion { major, minor }, change_attempts: 0, last_code: 0 } }
    pub fn last_code(&self) -> u16 { self.last_code }
    /// 接口冻结（冻结后不可变更）。
    pub fn freeze(&mut self) { self.frozen = true; self.last_code = 0; }
    pub fn try_change(&mut self, major: u8, minor: u8) -> Result<(), u16> {
        if self.frozen { self.change_attempts += 1; self.last_code = XC_FREEZE_VIOLATION; return Err(XC_FREEZE_VIOLATION); }
        self.version = IfaceVersion { major, minor };
        self.last_code = 0;
        Ok(())
    }
    pub fn frozen(&self) -> bool { self.frozen }
    pub fn version(&self) -> IfaceVersion { self.version }
    pub fn change_attempts(&self) -> u32 { self.change_attempts }
}

pub fn run_vea155_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }
    let mut set = CheckSet::new("vea155_freeze");
    { let f = MatrixFreeze::new(1, 0); chk(&mut set, "C155-01", !f.frozen(), "err"); }
    { let mut f = MatrixFreeze::new(1, 0); f.freeze();
        chk(&mut set, "C155-02", f.frozen(), "err"); }
    { let mut f = MatrixFreeze::new(1, 0); f.freeze(); let r = f.try_change(2, 0);
        chk(&mut set, "C155-03", r.err() == Some(XC_FREEZE_VIOLATION), "err"); }
    { let mut f = MatrixFreeze::new(1, 0); let r = f.try_change(1, 1);
        chk(&mut set, "C155-04", r.is_ok() && f.version().minor == 1, "err"); }
    { let mut f = MatrixFreeze::new(1, 0); f.freeze(); f.try_change(2, 0); f.try_change(3, 0);
        chk(&mut set, "C155-05", f.change_attempts() == 2, "err"); }
    { let f = MatrixFreeze::new(2, 3); chk(&mut set, "C155-06", f.version().major == 2 && f.version().minor == 3, "err"); }
    { let mut f = MatrixFreeze::new(1, 0); f.freeze(); f.freeze();
        chk(&mut set, "C155-07", f.frozen(), "err"); }
    { let f = MatrixFreeze::new(1, 0); chk(&mut set, "C155-08", f.change_attempts() == 0, "err"); }
    { let mut f = MatrixFreeze::new(1, 0); f.try_change(1, 0);
        chk(&mut set, "C155-09", f.last_code() == 0, "err"); }
    { let mut f = MatrixFreeze::new(1, 0); f.freeze();
        chk(&mut set, "C155-10", f.version().major == 1, "err"); }
    set
}
