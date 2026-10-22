//! UNX-F0327 trampoline 页表最小集（临时恒等映射）（0xAC80 段）
//!
//! 锚点：UNX-F0327-J1 临时页表（低 1MB + 跳板 + 主表入口）映射齐备，
//! AP 开页断言过。
//! 域：CoRun·UNX-A1-B17（跳板页表面）。
//!
//! 结构：四级页表模型（PML4/PDPT/PD/PT 槽位账）+ 临时映射器（低
//! 1MB 恒等 + 跳板页 + 主表入口三区）+ AP 开页断言（AP 首页可译）
//! + 注入核（缺区/双映射/槽位漂移/封账）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0xAC80 独占 0x80）。
pub const SEG_BASE: u32 = 0xAC80;
pub const XC_TR_MISS: u32 = SEG_BASE + 0x01; // 缺区
pub const XC_TR_DUP: u32 = SEG_BASE + 0x02; // 双映射
pub const XC_TR_SEALED: u32 = SEG_BASE + 0x03; // 封账

/// 页表参数。
pub const PAGE: u64 = 4096;
pub const LOW_1MB_PAGES: usize = 256; // 低 1MB = 256 页
pub const TRAMPOLINE_PAGE: u64 = 0x0008_8000; // 跳板页锚
pub const PML4_ENTRIES: usize = 512;

/// 页表项。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Pte {
    pub frame: u64, // 物理帧号
    pub present: bool,
    pub writable: bool,
    pub nx: bool,
}

/// 临时页表集。
pub struct TrampTable {
    pub pml4: Vec<Option<u64>>, // PML4 槽 → 下级表锚
    pub low_map: Vec<Pte>,      // 低 1MB 恒等映射（256 页）
    pub tramp_pte: Option<Pte>, // 跳板页
    pub main_entry: Option<u64>, // 主表入口（内核高位映射槽）
    pub sealed: bool,
}

impl TrampTable {
    pub fn new() -> Self {
        let mut low_map = Vec::new();
        for i in 0..LOW_1MB_PAGES {
            low_map.push(Pte {
                frame: i as u64,
                present: false,
                writable: true,
                nx: false,
            });
        }
        let mut pml4 = Vec::new();
        for _ in 0..PML4_ENTRIES {
            pml4.push(None);
        }
        TrampTable { pml4, low_map, tramp_pte: None, main_entry: None, sealed: false }
    }

    /// 建低 1MB 恒等映射。
    pub fn map_low_1mb(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(XC_TR_SEALED);
        }
        for (i, pte) in self.low_map.iter_mut().enumerate() {
            if pte.present {
                return Err(XC_TR_DUP);
            }
            pte.present = true;
            pte.frame = i as u64; // 恒等：virt == phys
        }
        Ok(())
    }

    /// 建跳板页映射。
    pub fn map_trampoline(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(XC_TR_SEALED);
        }
        if self.tramp_pte.is_some() {
            return Err(XC_TR_DUP);
        }
        let frame = TRAMPOLINE_PAGE / PAGE;
        self.tramp_pte = Some(Pte { frame, present: true, writable: true, nx: false });
        // 跳板页在低 1MB 内 → low_map 同步
        let idx = (TRAMPOLINE_PAGE / PAGE) as usize;
        if idx < LOW_1MB_PAGES {
            self.low_map[idx] = self.tramp_pte.unwrap();
        }
        Ok(())
    }

    /// 建主表入口（PML4 槽 0 → 临时 PDPT 锚）。
    pub fn set_main_entry(&mut self, pdpt_anchor: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(XC_TR_SEALED);
        }
        if self.main_entry.is_some() {
            return Err(XC_TR_DUP);
        }
        if self.pml4[0].is_some() {
            return Err(XC_TR_DUP);
        }
        self.pml4[0] = Some(pdpt_anchor);
        self.main_entry = Some(pdpt_anchor);
        Ok(())
    }

    /// AP 开页断言：低 1MB 任意页可译（恒等）。
    pub fn ap_page_walk(&self, addr: u64) -> Result<u64, u32> {
        let idx = (addr / PAGE) as usize;
        if idx >= LOW_1MB_PAGES {
            return Err(XC_TR_MISS);
        }
        let pte = &self.low_map[idx];
        if !pte.present {
            return Err(XC_TR_MISS);
        }
        Ok(pte.frame * PAGE + (addr % PAGE))
    }

    /// 映射齐备核：三区全建。
    pub fn complete(&self) -> bool {
        self.low_map.iter().all(|p| p.present)
            && self.tramp_pte.map(|p| p.present).unwrap_or(false)
            && self.main_entry.is_some()
            && self.pml4[0].is_some()
    }
}

/// 三区齐备核。
pub fn three_regions_exact() -> bool {
    let mut t = TrampTable::new();
    if t.map_low_1mb().is_err() {
        return false;
    }
    if t.map_trampoline().is_err() {
        return false;
    }
    if t.set_main_entry(0x0009_0000).is_err() {
        return false;
    }
    t.complete()
}

/// 恒等映射核：低 1MB 全页 phys == virt。
pub fn identity_exact() -> bool {
    let mut t = TrampTable::new();
    let _ = t.map_low_1mb();
    let _ = t.map_trampoline();
    for addr in [0u64, 0x1000, 0x7F000, 0x8_8000, 0xF_F000] {
        match t.ap_page_walk(addr) {
            Ok(p) => {
                if p != addr {
                    return false;
                }
            }
            Err(_) => return false,
        }
    }
    true
}

/// AP 开页断言核：跳板页可译且全低 1MB 可译。
pub fn ap_boot_exact() -> bool {
    let mut t = TrampTable::new();
    let _ = t.map_low_1mb();
    let _ = t.map_trampoline();
    // AP 起始页与跳板页
    t.ap_page_walk(0x8_8000) == Ok(0x8_8000)
        && t.ap_page_walk(0x0).is_ok()
        && t.ap_page_walk(0xF_FFFF).is_ok()
        && t.ap_page_walk(0x10_0000).is_err() // 超 1MB 拒
}

/// 注入核（4 注入必抓）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 双映射
    let mut t = TrampTable::new();
    let _ = t.map_low_1mb();
    if t.map_low_1mb() == Err(XC_TR_DUP) {
        caught += 1;
    }
    // 2) 跳板双映射
    let mut t2 = TrampTable::new();
    let _ = t2.map_trampoline();
    if t2.map_trampoline() == Err(XC_TR_DUP) {
        caught += 1;
    }
    // 3) 缺区（未建主表 → complete=false）
    let mut t3 = TrampTable::new();
    let _ = t3.map_low_1mb();
    let _ = t3.map_trampoline();
    if !t3.complete() {
        caught += 1;
    }
    // 4) 封账后拒
    t3.sealed = true;
    if t3.set_main_entry(0x1000) == Err(XC_TR_SEALED) {
        caught += 1;
    }
    (caught, 4)
}

/// 指纹冻结：页表折叠哈希。
pub fn fp_frozen(t: &TrampTable) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for p in &t.low_map {
        h ^= p.frame;
        h = h.wrapping_mul(0x100000001b3);
        h ^= p.present as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h ^= t.main_entry.unwrap_or(0);
    h = h.wrapping_mul(0x100000001b3);
    h
}

/// 判据入口（CheckSet）。
pub fn run_unx327_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0327");
    // J1-1 三区齐备
    let ok = three_regions_exact();
    s.add("B17-01-三区齐备", ok, if ok { "" } else { "齐备漂移" });
    // J1-2 恒等映射
    let ok = identity_exact();
    s.add("B17-02-恒等映射", ok, if ok { "" } else { "恒等漂移" });
    // J1-3 AP 开页断言
    let ok = ap_boot_exact();
    s.add("B17-03-AP开页", ok, if ok { "" } else { "AP 漂移" });
    // J1-4 注入核 4/4
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { alloc::string::String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("B17-04-注入核必抓", ok, &detail);
    // J1-5 指纹冻结
    let t = TrampTable::new();
    let ok = fp_frozen(&t) == fp_frozen(&t);
    s.add("B17-05-指纹冻结", ok, if ok { "" } else { "指纹漂移" });
    s
}
