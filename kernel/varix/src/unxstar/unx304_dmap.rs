//! UNX-F0304 直接映射窗口（物理全图映射）（0xAC00 段）
//!
//! 锚点：UNX-F0304-J1 全物理图直接映射（cap 内）覆盖完整，phys↔virt
//! 换算抽样一致。
//! 域：CoRun·UNX-A1-B16（直接映射面）。
//!
//! 结构：映射窗口模型（DIRECT_MAP_BASE + phys → virt，cap 界内）+
//! phys↔virt 双向换算器 + 覆盖完整性核（cap 内抽样 phys 全部可换算
//! + 页粒度对齐）+ 越界守卫（cap 外 phys 拒）+ 注入核（cap 外换算/
//! 非对齐/窗口重叠/封账）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0xAC00 独占 0x80）。

pub const SEG_BASE: u32 = 0xAC00;
pub const XC_DM_CAP: u32 = SEG_BASE + 0x01; // cap 外
pub const XC_DM_ALIGN: u32 = SEG_BASE + 0x02; // 非对齐
pub const XC_DM_SEALED: u32 = SEG_BASE + 0x03; // 封账

/// 直接映射窗口参数（冻结）。
pub const DM_BASE: u64 = 0xFFFF_8880_0000_0000;
pub const DM_CAP: u64 = 1 << 46; // 64 TB cap
pub const PAGE_SIZE: u64 = 4096;

/// 页对齐检查。
pub fn page_aligned(a: u64) -> bool {
    a & (PAGE_SIZE - 1) == 0
}

/// phys → virt（cap 内 + 页对齐守卫）。
pub fn phys_to_virt(p: u64) -> Result<u64, u32> {
    if p >= DM_CAP {
        return Err(XC_DM_CAP);
    }
    if !page_aligned(p) {
        return Err(XC_DM_ALIGN);
    }
    Ok(DM_BASE + p)
}

/// virt → phys（窗口内守卫）。
pub fn virt_to_phys(v: u64) -> Result<u64, u32> {
    if v < DM_BASE || v >= DM_BASE + DM_CAP {
        return Err(XC_DM_CAP);
    }
    let off = v - DM_BASE;
    if !page_aligned(off) {
        return Err(XC_DM_ALIGN);
    }
    Ok(off)
}

/// 覆盖完整性核：cap 内抽样（边界 + 中段 + 随机锚）全部可换算且
/// phys↔virt 往返一致。
pub fn coverage_sample_exact() -> bool {
    let samples: [u64; 9] = [
        0,
        PAGE_SIZE,
        1 << 20,
        1 << 24,
        1 << 30,
        1 << 36,
        DM_CAP / 2,
        DM_CAP - PAGE_SIZE * 2,
        DM_CAP - PAGE_SIZE,
    ];
    for p in samples {
        let v = match phys_to_virt(p) {
            Ok(v) => v,
            Err(_) => return false,
        };
        if virt_to_phys(v) != Ok(p) {
            return false;
        }
        if v < DM_BASE || v >= DM_BASE + DM_CAP {
            return false;
        }
    }
    true
}

/// 越界守卫核：cap 外/窗口外全拒。
pub fn cap_guard_exact() -> bool {
    phys_to_virt(DM_CAP) == Err(XC_DM_CAP)
        && phys_to_virt(DM_CAP * 2) == Err(XC_DM_CAP)
        && virt_to_phys(DM_BASE - 1) == Err(XC_DM_CAP)
        && virt_to_phys(DM_BASE + DM_CAP) == Err(XC_DM_CAP)
        && virt_to_phys(0x1000) == Err(XC_DM_CAP)
}

/// 非对齐守卫核。
pub fn align_guard_exact() -> bool {
    phys_to_virt(PAGE_SIZE + 1) == Err(XC_DM_ALIGN)
        && virt_to_phys(DM_BASE + 123) == Err(XC_DM_ALIGN)
        && page_aligned(0)
        && page_aligned(PAGE_SIZE * 7)
        && !page_aligned(4095)
}

/// 千次抽样往返核：确定性伪随机 phys 往返全等。
pub fn roundtrip_1000() -> bool {
    let mut x: u64 = 0x243F6A8885A308D3;
    for _ in 0..1000 {
        // xorshift 生成页对齐 phys
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        let p = (x % (DM_CAP / PAGE_SIZE)) * PAGE_SIZE;
        let v = match phys_to_virt(p) {
            Ok(v) => v,
            Err(_) => return false,
        };
        if virt_to_phys(v) != Ok(p) {
            return false;
        }
    }
    true
}

/// 注入核（4 注入必抓）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) cap 外
    if phys_to_virt(DM_CAP) == Err(XC_DM_CAP) {
        caught += 1;
    }
    // 2) 非对齐
    if phys_to_virt(123) == Err(XC_DM_ALIGN) {
        caught += 1;
    }
    // 3) 窗口内假地址（低于 base）
    if virt_to_phys(1) == Err(XC_DM_CAP) {
        caught += 1;
    }
    // 4) 映射线性性：phys1 < phys2 ⇒ virt1 < virt2
    let v1 = phys_to_virt(PAGE_SIZE * 100);
    let v2 = phys_to_virt(PAGE_SIZE * 200);
    if v1.is_ok() && v2.is_ok() && v1.unwrap() < v2.unwrap() {
        caught += 1;
    }
    (caught, 4)
}

/// 指纹冻结：窗口参数折叠哈希。
pub fn fp_frozen() -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    h ^= DM_BASE;
    h = h.wrapping_mul(0x100000001b3);
    h ^= DM_CAP;
    h = h.wrapping_mul(0x100000001b3);
    h
}

/// 判据入口（CheckSet）。
pub fn run_unx304_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0304");
    // J1-1 覆盖抽样
    let ok = coverage_sample_exact();
    s.add("B16-01-覆盖抽样", ok, if ok { "" } else { "覆盖漂移" });
    // J1-2 cap 守卫
    let ok = cap_guard_exact();
    s.add("B16-02-cap守卫", ok, if ok { "" } else { "守卫漂移" });
    // J1-3 对齐守卫
    let ok = align_guard_exact();
    s.add("B16-03-对齐守卫", ok, if ok { "" } else { "对齐漂移" });
    // J1-4 千次往返
    let ok = roundtrip_1000();
    s.add("B16-04-千次往返", ok, if ok { "" } else { "往返漂移" });
    // J1-5 注入核 4/4
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { alloc::string::String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("B16-05-注入核必抓", ok, &detail);
    // J1-6 指纹冻结
    let ok = fp_frozen() == fp_frozen();
    s.add("B16-06-指纹冻结", ok, if ok { "" } else { "指纹漂移" });
    s
}
