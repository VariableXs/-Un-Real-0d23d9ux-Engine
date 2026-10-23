//! VE-F0166 · 显存地图可视化（VE-A 域 · A07 · 目标 300 行）
//!
//! 铁律：显存可查，碎片可量，分配可追，回收可验。

use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xE8EC;
pub const XC_VRAM_FULL: u16 = SEG_BASE | 0x01;

pub struct VramMap {
    total_bytes: u64,
    used_bytes: u64,
    allocations: u32,
    frees: u32,
    last_code: u16,
}

impl VramMap {
    pub fn new(total_bytes: u64) -> Self { Self { total_bytes, used_bytes: 0, allocations: 0, frees: 0, last_code: 0 } }
    pub fn last_code(&self) -> u16 { self.last_code }
    pub fn alloc(&mut self, bytes: u64) -> Result<(), u16> {
        if self.used_bytes + bytes > self.total_bytes { self.last_code = XC_VRAM_FULL; return Err(XC_VRAM_FULL); }
        self.used_bytes += bytes;
        self.allocations += 1;
        self.last_code = 0;
        Ok(())
    }
    pub fn free(&mut self, bytes: u64) { self.used_bytes = self.used_bytes.saturating_sub(bytes); self.frees += 1; }
    pub fn used_bytes(&self) -> u64 { self.used_bytes }
    pub fn free_bytes(&self) -> u64 { self.total_bytes.saturating_sub(self.used_bytes) }
    pub fn allocations(&self) -> u32 { self.allocations }
    pub fn utilization(&self) -> f32 {
        if self.total_bytes == 0 { return 0.0; }
        self.used_bytes as f32 / self.total_bytes as f32
    }
}

pub fn run_vea166_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }
    let mut set = CheckSet::new("vea166_vrammap");
    { let v = VramMap::new(1000); chk(&mut set, "C166-01", v.used_bytes() == 0, "err"); }
    { let mut v = VramMap::new(1000); let r = v.alloc(500);
        chk(&mut set, "C166-02", r.is_ok() && v.used_bytes() == 500, "err"); }
    { let mut v = VramMap::new(1000); let r = v.alloc(2000);
        chk(&mut set, "C166-03", r.err() == Some(XC_VRAM_FULL), "err"); }
    { let mut v = VramMap::new(1000); v.alloc(500); v.free(300);
        chk(&mut set, "C166-04", v.used_bytes() == 200, "err"); }
    { let mut v = VramMap::new(1000); v.alloc(250);
        chk(&mut set, "C166-05", v.utilization() > 0.24 && v.utilization() < 0.26, "err"); }
    { let mut v = VramMap::new(1000); v.alloc(500); v.alloc(500);
        chk(&mut set, "C166-06", v.free_bytes() == 0, "err"); }
    { let v = VramMap::new(0); chk(&mut set, "C166-07", v.utilization() == 0.0, "err"); }
    { let mut v = VramMap::new(1000); v.alloc(100);
        chk(&mut set, "C166-08", v.allocations() == 1, "err"); }
    { let mut v = VramMap::new(1000); v.free(100);
        chk(&mut set, "C166-09", v.used_bytes() == 0, "err"); }
    { let mut v = VramMap::new(1000); v.alloc(100);
        chk(&mut set, "C166-10", v.last_code() == 0, "err"); }
    set
}
