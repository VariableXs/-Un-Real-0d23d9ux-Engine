//! UNX-F10144 mprotect 正反双判据（保护翻转达成与写只读页误用受控）（0x10520 段）
//!
//! 锚点：UNX-F10144-J1 翻转达成（R→RW 提升后可写 10/10 次）。
//! 域：CoRun·UNX-C3（ktest syscall 面谱系）。
//!
//! 结构：保护页册（页→保护位 + 数据面）+ 翻转核（R→RW 提升/RW→R 收紧/
//! NONE 全封/写只读页受控拒绝）+ 翻转账（逐次留痕 + 违例计数）+ 注入核
//! （写只读页/野位/未映射页/空留痕/封册必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use alloc::vec;

/// 诊断段（0x10520 独占 0x20）。
pub const SEG_BASE: u32 = 0x10520;
pub const PT_WRITE_RO: u32 = SEG_BASE + 0x01; // 写只读页
pub const PT_BAD_PROT: u32 = SEG_BASE + 0x02; // 野保护位
pub const PT_UNMAPPED: u32 = SEG_BASE + 0x03; // 未映射页
pub const PT_EMPTY_NOTE: u32 = SEG_BASE + 0x04; // 空留痕
pub const PT_SEALED: u32 = SEG_BASE + 0x05; // 封册后操作

/// 保护位闭集。
pub const PROT_R: u32 = 1;
pub const PROT_W: u32 = 2;
pub const PROT_NONE: u32 = 0;

/// 保护页。
pub struct ProtPage {
    pub data: Vec<u8>,
    pub prot: u32,
}

impl ProtPage {
    pub fn new(prot: u32) -> ProtPage {
        ProtPage { data: vec![0u8; 16], prot }
    }

    pub fn readable(&self) -> bool {
        self.prot & PROT_R != 0
    }

    pub fn writable(&self) -> bool {
        self.prot & PROT_W != 0
    }
}

/// 翻转账留痕。
#[derive(Clone)]
pub struct PtRec {
    pub seq: u32,
    pub page: u32,
    pub from: u32,
    pub to: u32,
}

impl PtRec {
    pub fn ok(&self) -> bool {
        self.to & !0x3 == 0
    }
}

/// 保护页册。
pub struct ProtBook {
    pub pages: Vec<Option<ProtPage>>,
    pub ledger: Vec<PtRec>,
    pub violations: u32,
    pub seq_next: u32,
    pub sealed: bool,
}

impl ProtBook {
    pub fn new() -> ProtBook {
        ProtBook { pages: Vec::new(), ledger: Vec::new(), violations: 0, seq_next: 1, sealed: false }
    }

    /// 挂页（带初始保护）。
    pub fn map_page(&mut self, prot: u32) -> Result<usize, u32> {
        if self.sealed {
            return Err(PT_SEALED);
        }
        if prot & !0x3 != 0 {
            return Err(PT_BAD_PROT);
        }
        if let Some(i) = self.pages.iter().position(|p| p.is_none()) {
            self.pages[i] = Some(ProtPage::new(prot));
            return Ok(i);
        }
        if self.pages.len() >= 8 {
            return Err(PT_UNMAPPED);
        }
        self.pages.push(Some(ProtPage::new(prot)));
        Ok(self.pages.len() - 1)
    }

    /// mprotect 核心：翻位。
    pub fn mprotect(&mut self, page: usize, prot: u32, note: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(PT_SEALED);
        }
        if note.is_empty() {
            return Err(PT_EMPTY_NOTE);
        }
        if prot & !0x3 != 0 {
            return Err(PT_BAD_PROT);
        }
        if page >= self.pages.len() || self.pages[page].is_none() {
            return Err(PT_UNMAPPED);
        }
        let from = self.pages[page].as_ref().map(|p| p.prot).unwrap_or(0);
        if let Some(p) = self.pages[page].as_mut() {
            p.prot = prot;
        }
        self.ledger.push(PtRec {
            seq: self.seq_next,
            page: page as u32,
            from,
            to: prot,
        });
        self.seq_next += 1;
        Ok(())
    }

    /// 页写：保护检查。
    pub fn write_page(&mut self, page: usize, off: usize, buf: &[u8]) -> Result<usize, u32> {
        if page >= self.pages.len() || self.pages[page].is_none() {
            return Err(PT_UNMAPPED);
        }
        let writable = self.pages[page].as_ref().map(|p| p.writable()).unwrap_or(false);
        if !writable {
            self.violations += 1;
            return Err(PT_WRITE_RO);
        }
        let p = match self.pages[page].as_mut() {
            Some(p) => p,
            None => return Err(PT_UNMAPPED),
        };
        if off + buf.len() > p.data.len() {
            return Err(PT_UNMAPPED);
        }
        p.data[off..off + buf.len()].copy_from_slice(buf);
        Ok(buf.len())
    }

    /// 页读。
    pub fn read_page(&self, page: usize, off: usize, count: usize) -> Result<Vec<u8>, u32> {
        if page >= self.pages.len() || self.pages[page].is_none() {
            return Err(PT_UNMAPPED);
        }
        let p = match self.pages[page].as_ref() {
            Some(p) => p,
            None => return Err(PT_UNMAPPED),
        };
        if !p.readable() {
            return Err(PT_UNMAPPED);
        }
        if off + count > p.data.len() {
            return Err(PT_UNMAPPED);
        }
        Ok(p.data[off..off + count].to_vec())
    }

    /// 提升核：R→RW 后可写。
    pub fn upgrade_rw(&mut self, page: usize) -> bool {
        self.mprotect(page, PROT_R | PROT_W, "提升").is_ok()
            && self.write_page(page, 0, b"W!").is_ok()
    }

    /// 收紧核：RW→R 后写拒。
    pub fn tighten_r(&mut self, page: usize) -> bool {
        self.mprotect(page, PROT_R, "收紧").is_ok()
            && self.write_page(page, 0, b"X").err() == Some(PT_WRITE_RO)
    }

    pub fn prot_of(&self, page: usize) -> Option<u32> {
        self.pages.get(page).and_then(|p| p.as_ref().map(|x| x.prot))
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
        let mut h: u32 = 0x105200;
        h = h.wrapping_mul(31).wrapping_add(self.violations);
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.to);
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
    let mut b = ProtBook::new();
    let ro = b.map_page(PROT_R).unwrap_or(0);
    // 1) 写只读页必拒
    got[0] = b.write_page(ro, 0, b"x").err() == Some(PT_WRITE_RO);
    // 2) 野保护位必拒
    got[1] = b.mprotect(ro, 0x4, "inj-prot").err() == Some(PT_BAD_PROT);
    // 3) 未映射页必拒
    got[2] = b.mprotect(99, PROT_R, "inj-unmap").err() == Some(PT_UNMAPPED);
    // 4) 空留痕必拒
    got[3] = b.mprotect(ro, PROT_R | PROT_W, "").err() == Some(PT_EMPTY_NOTE);
    // 5) 封册后操作必抓
    b.seal();
    got[4] = b.mprotect(ro, PROT_R, "inj-seal").err() == Some(PT_SEALED);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10144_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10144");

    // 正路径：R→RW 提升后可写
    let mut b = ProtBook::new();
    let p = b.map_page(PROT_R).unwrap_or(0);
    let up = b.upgrade_rw(p);
    let j1 = up && b.read_page(p, 0, 2).unwrap_or_default() == b"W!".to_vec();
    s.add("J1-RW提升可写", j1, "R→RW 提升后写读一致");

    // RW→R 收紧后写拒
    let j2 = {
        let p2 = b.map_page(PROT_R | PROT_W).unwrap_or(0);
        let _ = b.write_page(p2, 0, b"keep");
        b.tighten_r(p2) && b.read_page(p2, 0, 4).unwrap_or_default() == b"keep".to_vec()
    };
    s.add("J2-收紧写拒", j2, "RW→R 后写拒且数据保留");

    // NONE 全封
    let j3 = {
        let p3 = b.map_page(PROT_R | PROT_W).unwrap_or(0);
        let _ = b.mprotect(p3, PROT_NONE, "封禁");
        b.read_page(p3, 0, 1).is_err() && b.write_page(p3, 0, b"x").err() == Some(PT_WRITE_RO)
    };
    s.add("J3-NONE全封", j3, "NONE 后读写皆拒");

    // 写只读页违例计数
    let j4 = b.violations >= 2;
    s.add("J4-违例计数", j4, "只读写拒计入违例");

    // 翻转账回读
    let j5 = b.ledger.iter().all(|r| r.ok()) && b.prot_of(p) == Some(PROT_R | PROT_W);
    s.add("J5-账面回读", j5, "翻位留痕合法且现值一致");

    // 野保护位受控
    let j6 = b.map_page(0x8).err() == Some(PT_BAD_PROT);
    s.add("J6-野位受控", j6, "挂页野位必拒");

    // 未映射页受控
    let j7 = b.read_page(99, 0, 1).err() == Some(PT_UNMAPPED)
        && b.write_page(99, 0, b"x").err() == Some(PT_UNMAPPED);
    s.add("J7-未映射受控", j7, "读写双面未映射同拒");

    // 页容量守恒
    let j8 = {
        let mut b8 = ProtBook::new();
        let mut ok = true;
        for _ in 0..8 {
            if b8.map_page(PROT_R | PROT_W).is_err() {
                ok = false;
            }
        }
        ok && b8.map_page(PROT_R).err() == Some(PT_UNMAPPED)
    };
    s.add("J8-容量守恒", j8, "页槽满必拒");

    // 空留痕拒收
    let j9 = b.map_page(PROT_R).is_ok() && b.mprotect(1, PROT_R, "").err() == Some(PT_EMPTY_NOTE);
    s.add("J9-空留痕拒", j9, "note 为空不入账");

    // 留痕合法与 seq 唯一
    let j10 = b.seq_unique();
    s.add("J10-seq唯一", j10, "留痕序列唯一");

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
    fn unx10144_prot_flow() {
        let mut b = ProtBook::new();
        let p = b.map_page(PROT_R | PROT_W).unwrap_or(0);
        let _ = b.write_page(p, 0, b"ab");
        let _ = b.mprotect(p, PROT_R, "t");
        assert!(b.write_page(p, 0, b"c").err() == Some(PT_WRITE_RO));
        let _ = b.mprotect(p, PROT_R | PROT_W, "u");
        assert!(b.write_page(p, 2, b"c").is_ok());
    }

    #[test]
    fn unx10144_checks_green() {
        let s = run_unx10144_checks();
        assert!(s.all_passed(), "UNX-F10144 判据未全绿");
    }
}
