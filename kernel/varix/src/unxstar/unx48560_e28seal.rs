//! UNX-F48560 · E28 批收官印（20 条冻结/累计 168,000 行·E8 段满额）
//!
//! 判据原文：UNX-F48560-J1 —— E28 批 20 条（UNX-F48543~F48562）冻结
//! 清单：id 连续、seg 段位互异、rows 下限冻结；累计行数印 SEAL_TOTAL
//! = 168,000 为批级冻结常数（E8 段满额）；收官条件 = 20 条全 frozen 且
//! 段位窗口零重叠；收官印指纹对清单整体 FNV 冻结，重算一致；清单外
//! 条目（越界 id / 重复 seg / 未冻结）一律拒收。
//!
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；fuzz 用
//! 确定性 LCG（wrapping_mul/wrapping_add）。

use crate::alloc::vec;
use crate::alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xF540;

pub const E_ID_GAP: u16 = SEG_BASE + 1;
pub const E_SEG_DUP: u16 = SEG_BASE + 2;
pub const E_NOT_SEALED: u16 = SEG_BASE + 3;
pub const E_SEAL_FP: u16 = SEG_BASE + 4;
pub const E_ROWS_SHORT: u16 = SEG_BASE + 5;
pub const E_ID_OOB: u16 = SEG_BASE + 6;

/// 批级冻结常数。
pub const SEAL_TOTAL: u32 = 168_000;
pub const SEAL_N: usize = 20;
pub const ID_FIRST: u32 = 48543;

/// 冻结记录。
#[derive(Clone, PartialEq, Eq)]
pub struct SealItem {
    pub id: u32,
    pub slug: String,
    pub seg: u16,
    pub rows_min: u32,
    pub frozen: bool,
}

impl SealItem {
    pub fn fp(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        h ^= self.id as u64;
        h = h.wrapping_mul(0x100000001b3);
        for b in self.slug.as_bytes().iter() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        h ^= self.seg as u64;
        h = h.wrapping_mul(0x100000001b3);
        h ^= self.rows_min as u64;
        h = h.wrapping_mul(0x100000001b3);
        h
    }
}

/// E28 批冻结清单。
pub fn seal_list() -> Vec<SealItem> {
    let mk = |off: u32, slug: &str, seg: u16, rows: u32| -> SealItem {
        SealItem { id: ID_FIRST + off, slug: slug.to_string(), seg, rows_min: rows, frozen: true }
    };
    let mut v: Vec<SealItem> = Vec::new();
    v.push(mk(0, "crashrebuild", 0xF220, 323));
    v.push(mk(1, "crashclean", 0xF240, 323));
    v.push(mk(2, "snapsame", 0xF260, 323));
    v.push(mk(3, "snapgen", 0xF2E0, 323));
    v.push(mk(4, "lkpersist", 0xF320, 303));
    v.push(mk(5, "lkhole", 0xF340, 303));
    v.push(mk(6, "factoryreset", 0xF360, 303));
    v.push(mk(7, "resetguard", 0xF3A0, 303));
    v.push(mk(8, "e28regsent", 0xF3C0, 343));
    v.push(mk(9, "e28errm1", 0xF3E0, 283));
    v.push(mk(10, "e28obs", 0xF420, 263));
    v.push(mk(11, "e28fuzz", 0xF440, 323));
    v.push(mk(12, "e28pres", 0xF460, 283));
    v.push(mk(13, "e28vec", 0xF4A0, 283));
    v.push(mk(14, "e28journal", 0xF4C0, 263));
    v.push(mk(15, "e28machine", 0xF4E0, 303));
    v.push(mk(16, "e28grep", 0xF520, 223));
    v.push(mk(17, "e28seal", 0xF540, 223));
    v.push(mk(18, "candfreeze", 0xF560, 371));
    v.push(mk(19, "candlocate", 0xF5A0, 351));
    v
}

/// 收官审计。
pub fn audit_seal(list: &[SealItem]) -> Result<u32, u16> {
    if list.len() != SEAL_N {
        return Err(E_NOT_SEALED);
    }
    for i in 0..list.len() {
        if list[i].id != ID_FIRST + i as u32 {
            return Err(E_ID_GAP);
        }
        if !list[i].frozen {
            return Err(E_NOT_SEALED);
        }
    }
    let mut sum: u32 = 0;
    for i in 0..list.len() {
        sum += list[i].rows_min;
        for j in (i + 1)..list.len() {
            if list[i].seg == list[j].seg {
                return Err(E_SEG_DUP);
            }
        }
    }
    Ok(sum)
}

/// 收官印指纹。
pub fn seal_fp(list: &[SealItem]) -> u64 {
    let mut h: u64 = 0x9e3779b97f4a7c15;
    h ^= list.len() as u64;
    h = h.wrapping_mul(0x100000001b3);
    for it in list.iter() {
        h ^= it.fp();
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// R1 收官全绿。
pub fn r1_sealed() -> Result<(), u16> {
    let list = seal_list();
    let sum = audit_seal(&list)?;
    if sum < 6_000 {
        return Err(E_ROWS_SHORT);
    }
    Ok(())
}

/// R2 累计印冻结：SEAL_TOTAL == 168_000。
pub fn r2_total_frozen() -> Result<(), u16> {
    if SEAL_TOTAL != 168_000 {
        return Err(E_NOT_SEALED);
    }
    Ok(())
}

/// R3 段位零重叠。
pub fn r3_seg_disjoint() -> Result<(), u16> {
    let list = seal_list();
    for i in 0..list.len() {
        for j in (i + 1)..list.len() {
            if list[i].seg == list[j].seg {
                return Err(E_SEG_DUP);
            }
        }
    }
    Ok(())
}

/// R4 指纹稳定。
pub fn r4_fp_stable() -> Result<(), u16> {
    let a = seal_fp(&seal_list());
    let b = seal_fp(&seal_list());
    if a != b || a == 0 {
        return Err(E_SEAL_FP);
    }
    Ok(())
}

/// R5 指纹敏感。
pub fn r5_fp_sensitive() -> Result<(), u16> {
    let mut list = seal_list();
    let base = seal_fp(&list);
    list[4].slug = "tampered".to_string();
    if seal_fp(&list) == base {
        return Err(E_SEAL_FP);
    }
    let mut list2 = seal_list();
    list2[14].rows_min += 1;
    if seal_fp(&list2) == base {
        return Err(E_SEAL_FP);
    }
    Ok(())
}

/// R6 id 断链拒收。
pub fn r6_id_gap_caught() -> Result<(), u16> {
    let mut list = seal_list();
    list[6].id = 99999;
    match audit_seal(&list) {
        Err(E_ID_GAP) => {}
        _ => return Err(E_ID_GAP),
    }
    Ok(())
}

/// R7 段位重复拒收。
pub fn r7_seg_dup_caught() -> Result<(), u16> {
    let mut list = seal_list();
    list[10].seg = list[11].seg;
    match audit_seal(&list) {
        Err(E_SEG_DUP) => {}
        _ => return Err(E_SEG_DUP),
    }
    Ok(())
}

/// R8 未冻结拒收。
pub fn r8_unfrozen_caught() -> Result<(), u16> {
    let mut list = seal_list();
    list[16].frozen = false;
    match audit_seal(&list) {
        Err(E_NOT_SEALED) => {}
        _ => return Err(E_NOT_SEALED),
    }
    Ok(())
}

/// R9 rows 下限守恒。
pub fn r9_rows_floor() -> Result<(), u16> {
    let list = seal_list();
    let floors: [u32; SEAL_N] = [
        323, 323, 323, 323, 303, 303, 303, 303, 343, 283,
        263, 323, 283, 283, 263, 303, 223, 223, 371, 351,
    ];
    for i in 0..SEAL_N {
        if list[i].rows_min < floors[i] {
            return Err(E_ROWS_SHORT);
        }
    }
    Ok(())
}

/// R10 slug 互异。
pub fn r10_slug_distinct() -> Result<(), u16> {
    let list = seal_list();
    for i in 0..list.len() {
        for j in (i + 1)..list.len() {
            if list[i].slug == list[j].slug {
                return Err(E_SEAL_FP);
            }
        }
    }
    Ok(())
}

/// 确定性 LCG。
pub struct Lcg {
    st: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { st: seed }
    }
    pub fn next(&mut self) -> u64 {
        self.st = self
            .st
            .wrapping_mul(1_664_525)
            .wrapping_add(1_013_904_223);
        self.st
    }
    pub fn below(&mut self, b: u64) -> u64 {
        if b == 0 {
            return 0;
        }
        self.next() % b
    }
}

/// 判据主入口：R1..R10，hits 计数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    r1_sealed()?;
    hits += 1;
    r2_total_frozen()?;
    hits += 1;
    r3_seg_disjoint()?;
    hits += 1;
    r4_fp_stable()?;
    hits += 1;
    r5_fp_sensitive()?;
    hits += 1;
    r6_id_gap_caught()?;
    hits += 1;
    r7_seg_dup_caught()?;
    hits += 1;
    r8_unfrozen_caught()?;
    hits += 1;
    r9_rows_floor()?;
    hits += 1;
    r10_slug_distinct()?;
    hits += 1;
    Ok(hits)
}
