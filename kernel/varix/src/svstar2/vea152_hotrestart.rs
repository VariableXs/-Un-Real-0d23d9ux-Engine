//! VE-F0152 · 服务升级热重启协议（VE-A 域 · A06 · 目标 340 行）
//!
//! 铁律：零停机升级，状态迁移，回滚兜底，版本兼容。

use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xE8DE;
pub const XC_DOWNTIME: u16 = SEG_BASE | 0x01;
pub const XC_STATE_MIGRATE_FAIL: u16 = SEG_BASE | 0x02;
pub const XC_VERSION_INCOMPAT: u16 = SEG_BASE | 0x03;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UpgradePhase { Stable, Migrating, Restarting, RolledBack }

pub struct HotRestarter {
    phase: UpgradePhase,
    old_version: u16,
    new_version: u16,
    upgrades: u32,
    rollbacks: u32,
    last_code: u16,
}

impl HotRestarter {
    pub fn new(version: u16) -> Self { Self { phase: UpgradePhase::Stable, old_version: version, new_version: version, upgrades: 0, rollbacks: 0, last_code: 0 } }
    pub fn phase(&self) -> UpgradePhase { self.phase }
    pub fn last_code(&self) -> u16 { self.last_code }
    /// 零停机热升级。
    pub fn upgrade(&mut self, new_ver: u16, state_ok: bool) -> Result<(), u16> {
        if new_ver < self.old_version { self.last_code = XC_VERSION_INCOMPAT; return Err(XC_VERSION_INCOMPAT); }
        self.phase = UpgradePhase::Migrating;
        if !state_ok { self.phase = UpgradePhase::RolledBack; self.rollbacks += 1; self.last_code = XC_STATE_MIGRATE_FAIL; return Err(XC_STATE_MIGRATE_FAIL); }
        self.old_version = self.new_version;
        self.new_version = new_ver;
        self.phase = UpgradePhase::Stable;
        self.upgrades += 1;
        self.last_code = 0;
        Ok(())
    }
    pub fn upgrades(&self) -> u32 { self.upgrades }
    pub fn rollbacks(&self) -> u32 { self.rollbacks }
    pub fn version(&self) -> u16 { self.new_version }
}

pub fn run_vea152_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }
    let mut set = CheckSet::new("vea152_hotrestart");
    { let h = HotRestarter::new(1); chk(&mut set, "C152-01", h.phase() == UpgradePhase::Stable, "err"); }
    { let mut h = HotRestarter::new(1); let r = h.upgrade(2, true);
        chk(&mut set, "C152-02", r.is_ok() && h.version() == 2, "err"); }
    { let mut h = HotRestarter::new(2); let r = h.upgrade(1, true);
        chk(&mut set, "C152-03", r.err() == Some(XC_VERSION_INCOMPAT), "err"); }
    { let mut h = HotRestarter::new(1); let r = h.upgrade(2, false);
        chk(&mut set, "C152-04", r.err() == Some(XC_STATE_MIGRATE_FAIL) && h.rollbacks() == 1, "err"); }
    { let mut h = HotRestarter::new(1); h.upgrade(2, true); h.upgrade(3, true);
        chk(&mut set, "C152-05", h.upgrades() == 2, "err"); }
    { let h = HotRestarter::new(5); chk(&mut set, "C152-06", h.version() == 5, "err"); }
    { let mut h = HotRestarter::new(1); let _ = h.upgrade(2, false);
        chk(&mut set, "C152-07", h.phase() == UpgradePhase::RolledBack, "err"); }
    { let h = HotRestarter::new(1); chk(&mut set, "C152-08", h.rollbacks() == 0, "err"); }
    { let mut h = HotRestarter::new(1); let r = h.upgrade(1, true);
        chk(&mut set, "C152-09", r.is_ok() && h.upgrades() == 1, "err"); }
    { let mut h = HotRestarter::new(1); let _ = h.upgrade(2, true);
        chk(&mut set, "C152-10", h.last_code() == 0, "err"); }
    set
}
