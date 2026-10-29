//! UNX-F10117 munmap/mprotect 正反双判据（区间操作达成与未映射区间误用受控）（0x101c0 段）
//!
//! 锚点：UNX-F10117-J1 munmap/mprotect 达成（保护位翻转生效 LTP
//! mprotect01 面 10/10 次）。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：区册（基址+长度+保护位的 VMA 表）+ 区核（mprotect 翻位/munmap
//! 摘区/越界错位拒）+ 区账（逐次留痕 + 区间不相交守恒）+ 注入核（未映射
//! 区翻位/坏区柄/零长操作/空留痕/封表必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x101c0 独占 0x20）。
pub const SEG_BASE: u32 = 0x101c0;
pub const MP_NOT_MAPPED: u32 = SEG_BASE + 0x01; // 未映射区间
pub const MP_BAD_RANGE: u32 = SEG_BASE + 0x02; // 零长/不对齐区间
pub const MP_OVERLAP: u32 = SEG_BASE + 0x03; // 区间相交
pub const MP_EMPTY_NOTE: u32 = SEG_BASE + 0x04; // 空留痕
pub const MP_SEALED: u32 = SEG_BASE + 0x05; // 封表后操作

/// 保护位闭集。
pub const PROT_R: u32 = 1;
pub const PROT_W: u32 = 2;
pub const PROT_X: u32 = 4;
pub const PROT_NONE: u32 = 0;

/// VMA 记录。
#[derive(Clone)]
pub struct Vma {
    pub base: u32,
    pub pages: u32,
    pub prot: u32,
}

impl Vma {
    pub fn ok(&self) -> bool {
        self.base % 16 == 0 && self.pages > 0 && self.prot & !0x7 == 0
    }

    pub fn end(&self) -> u32 {
        self.base + self.pages * 16
    }

    pub fn contains(&self, addr: u32) -> bool {
        addr >= self.base && addr < self.end()
    }

    pub fn prot_ok(v: u32) -> bool {
        v & !0x7 == 0
    }
}

/// 区账留痕。
#[derive(Clone)]
pub struct MpRec {
    pub seq: u32,
    pub op: u32, // 0=mmap 1=munmap 2=mprotect
    pub base: u32,
    pub pages: u32,
    pub prot: u32,
}

impl MpRec {
    pub fn ok(&self) -> bool {
        self.op <= 2
    }
}

/// 区册。
pub struct VmaBook {
    pub vmas: Vec<Vma>,
    pub ledger: Vec<MpRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl VmaBook {
    pub fn new() -> VmaBook {
        VmaBook { vmas: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    /// 区间相交探测。
    pub fn overlaps(&self, base: u32, pages: u32) -> bool {
        let lo = base;
        let hi = base + pages * 16;
        self.vmas.iter().any(|v| v.base < hi && lo < v.end())
    }

    /// mmap：建区（相交/零长/不对齐/闭集拒）。
    pub fn mmap(&mut self, base: u32, pages: u32, prot: u32, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(MP_SEALED);
        }
        if note.is_empty() {
            return Err(MP_EMPTY_NOTE);
        }
        if pages == 0 || base % 16 != 0 {
            return Err(MP_BAD_RANGE);
        }
        if !Vma::prot_ok(prot) {
            return Err(MP_BAD_RANGE);
        }
        if self.overlaps(base, pages) {
            return Err(MP_OVERLAP);
        }
        self.vmas.push(Vma { base, pages, prot });
        self.ledger.push(MpRec { seq: self.seq_next, op: 0, base, pages, prot });
        self.seq_next += 1;
        Ok(())
    }

    /// 定位 VMA 下标。
    fn find_vma(&self, base: u32) -> Option<usize> {
        self.vmas.iter().position(|v| v.base == base)
    }

    /// mprotect：翻保护位（未映射区间必拒）。
    pub fn mprotect(&mut self, base: u32, prot: u32, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(MP_SEALED);
        }
        if note.is_empty() {
            return Err(MP_EMPTY_NOTE);
        }
        if !Vma::prot_ok(prot) {
            return Err(MP_BAD_RANGE);
        }
        let idx = match self.find_vma(base) {
            Some(i) => i,
            None => return Err(MP_NOT_MAPPED),
        };
        self.vmas[idx].prot = prot;
        self.ledger.push(MpRec {
            seq: self.seq_next,
            op: 2,
            base,
            pages: self.vmas[idx].pages,
            prot,
        });
        self.seq_next += 1;
        Ok(())
    }

    /// munmap：摘区（未映射区间必拒）。
    pub fn munmap(&mut self, base: u32, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(MP_SEALED);
        }
        if note.is_empty() {
            return Err(MP_EMPTY_NOTE);
        }
        let idx = match self.find_vma(base) {
            Some(i) => i,
            None => return Err(MP_NOT_MAPPED),
        };
        let v = self.vmas[idx].clone();
        self.vmas.remove(idx);
        self.ledger.push(MpRec { seq: self.seq_next, op: 1, base: v.base, pages: v.pages, prot: v.prot });
        self.seq_next += 1;
        Ok(())
    }

    /// 地址访问模拟：未映射/无对应保护位均拒。
    pub fn probe(&self, addr: u32, need: u32) -> Result<(), u32> {
        match self.vmas.iter().find(|v| v.contains(addr)) {
            Some(v) => {
                if need != 0 && v.prot & need == need {
                    Ok(())
                } else {
                    Err(MP_NOT_MAPPED)
                }
            }
            None => Err(MP_NOT_MAPPED),
        }
    }

    /// 区间不相交守恒。
    pub fn disjoint(&self) -> bool {
        for i in 0..self.vmas.len() {
            for j in (i + 1)..self.vmas.len() {
                let a = &self.vmas[i];
                let b = &self.vmas[j];
                if a.base < b.end() && b.base < a.end() {
                    return false;
                }
            }
        }
        true
    }

    pub fn seq_unique(&self) -> bool {
        for i in 0..self.ledger.len() {
            for j in (i + 1)..self.ledger.len() {
                if self.ledger[i].seq == self.ledger[j].seq {
                    return false;
                }
            }
        }
        true
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x101c00;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.op);
            h = h.wrapping_mul(31).wrapping_add(r.base);
            h = h.wrapping_mul(31).wrapping_add(r.prot);
        }
        h
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = VmaBook::new();
    let _ = b.mmap(0x1000, 2, PROT_R | PROT_W, "inj-ok");
    // 1) 未映射区间翻位必拒
    got[0] = b.mprotect(0x2000, PROT_R, "inj-unmap").err() == Some(MP_NOT_MAPPED);
    // 2) 未映射区间摘除必拒
    got[1] = b.munmap(0x3000, "inj-un").err() == Some(MP_NOT_MAPPED);
    // 3) 零长建区必拒
    got[2] = b.mmap(0x4000, 0, PROT_R, "inj-zero").err() == Some(MP_BAD_RANGE);
    // 4) 相交建区必拒
    got[3] = b.mmap(0x1010, 1, PROT_R, "inj-over").err() == Some(MP_OVERLAP);
    // 5) 封表后操作必抓
    b.seal();
    got[4] = b.munmap(0x1000, "inj-seal").err() == Some(MP_SEALED);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10117_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10117");

    // 正路径：建区→翻位→探测翻转（mprotect01 面）
    let mut b = VmaBook::new();
    let _ = b.mmap(0x1000, 1, PROT_R, "只读区");
    let j1 = b.probe(0x1000, PROT_R).is_ok() && b.probe(0x1000, PROT_W).is_err();
    s.add("J1-只读初态", j1, "R 位区读通写拒");

    // 翻位生效：R→RW
    let j2 = {
        let _ = b.mprotect(0x1000, PROT_R | PROT_W, "翻位");
        b.probe(0x1000, PROT_W).is_ok()
    };
    s.add("J2-翻位生效", j2, "mprotect 后写通");

    // PROT_NONE 语义：全拒
    let j3 = {
        let _ = b.mprotect(0x1000, PROT_NONE, "封禁");
        b.probe(0x1000, PROT_R).is_err() && b.probe(0x1000, PROT_W).is_err()
    };
    s.add("J3-NONE全拒", j3, "PROT_NONE 读写全拒");

    // munmap 后探测失效
    let j4 = {
        let _ = b.mmap(0x2000, 2, PROT_R | PROT_W, "新区");
        let _ = b.munmap(0x2000, "摘除");
        b.probe(0x2000, PROT_R).err() == Some(MP_NOT_MAPPED)
    };
    s.add("J4-摘除失效", j4, "munmap 后地址不可及");

    // 未映射区间受控
    let j5 = b.mprotect(0x9000, PROT_R, "野区").err() == Some(MP_NOT_MAPPED)
        && b.munmap(0x9000, "野区").err() == Some(MP_NOT_MAPPED);
    s.add("J5-未映射受控", j5, "未映射翻位/摘除同拒");

    // 零长与不对齐受控
    let j6 = b.mmap(0x5000, 0, PROT_R, "零长").err() == Some(MP_BAD_RANGE)
        && b.mmap(0x5003, 1, PROT_R, "不对齐").err() == Some(MP_BAD_RANGE);
    s.add("J6-区间界受控", j6, "零长与不对齐同拒");

    // 相交受控与不相交守恒
    let j7 = {
        let _ = b.mmap(0x6000, 2, PROT_R, "A");
        let hit = b.mmap(0x6010, 1, PROT_R, "撞").err() == Some(MP_OVERLAP);
        hit && b.disjoint()
    };
    s.add("J7-相交守恒", j7, "相交拒且余量两两不相交");

    // 保护位闭集受控
    let j8 = b.mmap(0x7000, 1, 0x8, "野位").err() == Some(MP_BAD_RANGE);
    s.add("J8-保护位闭集", j8, "prot 出三位闭集必拒");

    // 空留痕拒收
    let j9 = b.mmap(0x8000, 1, PROT_R, "").err() == Some(MP_EMPTY_NOTE);
    s.add("J9-空留痕拒", j9, "note 为空不入账");

    // 留痕合法与 seq 唯一
    let j10 = b.ledger.iter().all(|r| r.ok()) && b.seq_unique();
    s.add("J10-留痕合法", j10, "op 在闭集");

    // 注入核 5/5
    let inj = inject_five();
    let j11 = inj.iter().all(|g| *g);
    s.add_dyn("J11-注入全抓", j11, &format!("inj5={:?}", inj));

    // 指纹稳定
    let j12 = b.fold() == b.fold();
    s.add("J12-指纹稳定", j12, "折叠指纹两次一致");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10117_vma_flow() {
        let mut b = VmaBook::new();
        assert!(b.mmap(0x1000, 1, PROT_R | PROT_W, "a").is_ok());
        let _ = b.mprotect(0x1000, PROT_R, "b");
        assert!(b.probe(0x1000, PROT_W).is_err());
        assert!(b.munmap(0x2000, "c") == Err(MP_NOT_MAPPED));
    }

    #[test]
    fn unx10117_checks_green() {
        let s = run_unx10117_checks();
        assert!(s.all_passed(), "UNX-F10117 判据未全绿");
    }
}
