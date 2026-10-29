//! UNX-F10338 联签确认书（三面双签）（0x1b4c0 段）
//!
//! 锚点：UNX-F10338-J1 三面双签确认——每封确认书由三面中至少两面签署
//! （法定人数为二），首个签下面须留名，未达人数的书不得入账讫，全部
//! 确认书达人数才算确认环节成立。
//! 域：CoRun·UNX-C3（联签/闭账/审计域）。
//!
//! 结构：确认书册（封=书号+案由+三面签署位+首签面 Option）+ 签署核
//! （同面重签即拒、坏面号即拒）+ 人数核（异面签署≥2）+ 注入核（零号/
//! 重封/未知封/重签四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b4c0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b4c0;
pub const CF_ZERO: u32 = SEG_BASE + 0x01; // 书号零
pub const CF_DUP: u32 = SEG_BASE + 0x02; // 书号重号
pub const CF_NOLETTER: u32 = SEG_BASE + 0x03; // 未知封
pub const CF_RESIGN: u32 = SEG_BASE + 0x04; // 同面重签
pub const CF_FACE: u32 = SEG_BASE + 0x05; // 面号越界
pub const CF_CAP: u32 = SEG_BASE + 0x06; // 册超容
pub const CF_EMPTY: u32 = SEG_BASE + 0x07; // 案由空

/// 三面数（开区间）。
pub const FACE_N: u8 = 3;
/// 法定人数锚（三选二）。
pub const QUORUM: usize = 2;
/// 确认书锚（三封）。
pub const LETTERS_NEEDED: usize = 3;
/// 册定容。
pub const CAP: usize = 8;

/// 确认书。
#[derive(Clone)]
pub struct TriLetter {
    pub lid: u32,
    pub subject: String,
    pub signs: [bool; 3],
    pub first: Option<u8>,
}

impl TriLetter {
    /// 签署面数。
    pub fn sign_count(&self) -> usize {
        let mut n = 0usize;
        let mut f = 0u8;
        while f < FACE_N {
            if self.signs[f as usize] {
                n += 1;
            }
            f += 1;
        }
        n
    }

    /// 达法定人数。
    pub fn quorum(&self) -> bool {
        self.sign_count() >= QUORUM
    }
}

/// 三面确认书册。
pub struct TriConfirm {
    pub letters: Vec<TriLetter>,
    pub sealed: bool,
}

impl TriConfirm {
    pub fn new() -> TriConfirm {
        TriConfirm { letters: Vec::new(), sealed: false }
    }

    fn pos(&self, lid: u32) -> Option<usize> {
        let mut k = 0usize;
        while k < self.letters.len() {
            if self.letters[k].lid == lid {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// 起草确认书。
    pub fn draft(&mut self, lid: u32, subject: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(CF_RESIGN);
        }
        if lid == 0 {
            return Err(CF_ZERO);
        }
        if self.pos(lid).is_some() {
            return Err(CF_DUP);
        }
        if subject.is_empty() {
            return Err(CF_EMPTY);
        }
        if self.letters.len() >= CAP {
            return Err(CF_CAP);
        }
        self.letters.push(TriLetter {
            lid,
            subject: subject.to_string(),
            signs: [false; 3],
            first: None,
        });
        Ok(())
    }

    /// 签署：同面重签即拒，首签面留名。
    pub fn sign(&mut self, lid: u32, face: u8) -> Result<(), u32> {
        if self.sealed {
            return Err(CF_RESIGN);
        }
        if face >= FACE_N {
            return Err(CF_FACE);
        }
        match self.pos(lid) {
            None => Err(CF_NOLETTER),
            Some(p) => {
                if self.letters[p].signs[face as usize] {
                    return Err(CF_RESIGN);
                }
                if self.letters[p].first.is_none() {
                    self.letters[p].first = Some(face);
                }
                self.letters[p].signs[face as usize] = true;
                Ok(())
            }
        }
    }

    /// 未达人数清单。
    pub fn unsigned(&self) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.letters.len() {
            if !self.letters[k].quorum() {
                v.push(self.letters[k].lid);
            }
            k += 1;
        }
        v
    }

    /// 首签面缺失清单。
    pub fn no_first(&self) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.letters.len() {
            if self.letters[k].first.is_none() {
                v.push(self.letters[k].lid);
            }
            k += 1;
        }
        v
    }

    /// 确认成立：封数达锚、零未达人数、首签全留名。
    pub fn all_duly(&self) -> bool {
        self.letters.len() >= LETTERS_NEEDED
            && self.unsigned().is_empty()
            && self.no_first().is_empty()
    }

    pub fn seal(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(CF_RESIGN);
        }
        if !self.all_duly() {
            return Err(CF_NOLETTER);
        }
        self.sealed = true;
        Ok(())
    }

    /// 首签面分布（面号→封数）。
    pub fn first_counts(&self) -> [usize; 3] {
        let mut c = [0usize; 3];
        let mut k = 0usize;
        while k < self.letters.len() {
            if let Some(f) = self.letters[k].first {
                if (f as usize) < 3 {
                    c[f as usize] += 1;
                }
            }
            k += 1;
        }
        c
    }

    /// 确认册指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1b4c00;
        h = h.wrapping_mul(31).wrapping_add(self.letters.len() as u32);
        let mut k = 0usize;
        while k < self.letters.len() {
            let l = &self.letters[k];
            h = h
                .wrapping_mul(31)
                .wrapping_add(l.lid)
                .wrapping_mul(31)
                .wrapping_add(l.sign_count() as u32)
                .wrapping_mul(31)
                .wrapping_add(l.first.unwrap_or(9) as u32 + 1);
            let sb = l.subject.as_bytes();
            let mut i = 0usize;
            while i < sb.len() {
                h = h.wrapping_mul(31).wrapping_add(sb[i] as u32);
                i += 1;
            }
            k += 1;
        }
        h
    }
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    let mut c = TriConfirm::new();
    // 1) 零号必拒
    got[0] = c.draft(0, "零封").err() == Some(CF_ZERO);
    // 2) 重封必拒
    let _ = c.draft(1, "口径确认");
    got[1] = c.draft(1, "重封").err() == Some(CF_DUP);
    // 3) 未知封签署必拒
    got[2] = c.sign(99, 0).err() == Some(CF_NOLETTER);
    // 4) 同面重签必拒
    let _ = c.sign(1, 1);
    got[3] = c.sign(1, 1).err() == Some(CF_RESIGN);
    got
}

/// 批内判据。
pub fn run_unx10338_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10338");

    // J1 正路径：三封确认书各两异面签署并封册
    let mut c = TriConfirm::new();
    let mut ok = true;
    let mut n = 1u32;
    while n <= LETTERS_NEEDED as u32 {
        if c.draft(n, "三面双签样").is_err() {
            ok = false;
        }
        n += 1;
    }
    n = 1;
    while n <= LETTERS_NEEDED as u32 {
        let f0 = (n % 3) as u8;
        let f1 = ((n + 1) % 3) as u8;
        if c.sign(n, f0).is_err() || c.sign(n, f1).is_err() {
            ok = false;
        }
        n += 1;
    }
    let sealed = c.seal();
    s.add(
        "J1-三面双签",
        ok && c.all_duly() && sealed.is_ok(),
        "三封确认书各两异面签署并封册",
    );

    // J2 首签面留名逐封核
    let fc = c.first_counts();
    let j2 = fc[0] + fc[1] + fc[2] == LETTERS_NEEDED && c.no_first().is_empty();
    s.add("J2-首签留名", j2, "每封首签面均留名");

    // J3 注入检出：零号 + 重封
    let inj = inject_four();
    s.add("J3-零号重封检出", inj[0] && inj[1], "零书号与重封必拒");

    // J4 注入检出：未知封 + 重签
    s.add("J4-未知重签检出", inj[2] && inj[3], "未知封签署与同面重签必拒");

    // J5 单签不成法定人数
    let j5 = {
        let mut t = TriConfirm::new();
        let _ = t.draft(1, "单签封");
        let _ = t.sign(1, 2);
        !t.letters[0].quorum() && t.unsigned().contains(&1)
    };
    s.add("J5-单签不成", j5, "仅一面签署不成人数");

    // J6 三面全签亦过（人数为下限）
    let j6 = {
        let mut t = TriConfirm::new();
        let _ = t.draft(1, "全签封");
        let _ = t.sign(1, 0);
        let _ = t.sign(1, 1);
        let _ = t.sign(1, 2);
        t.letters[0].quorum() && t.letters[0].sign_count() == 3
    };
    s.add("J6-全签亦过", j6, "三面全签超人数仍有效");

    // J7 坏面号必拒
    let j7 = {
        let mut t = TriConfirm::new();
        let _ = t.draft(1, "面号封");
        t.sign(1, 3).err() == Some(CF_FACE)
    };
    s.add("J7-坏面拒签", j7, "面号越界必拒");

    // J8 空案由必拒
    let j8 = TriConfirm::new().draft(1, "").err() == Some(CF_EMPTY);
    s.add("J8-空由拒收", j8, "案由空必拒");

    // J9 未达人数阻断封册
    let j9 = {
        let mut t = TriConfirm::new();
        let _ = t.draft(1, "挂封");
        let _ = t.sign(1, 0);
        t.seal().err() == Some(CF_NOLETTER)
    };
    s.add("J9-挂封拒册", j9, "存在未达人数封即不得封册");

    // J10 容量边界
    let j10 = {
        let mut t = TriConfirm::new();
        let mut n = 1u32;
        let mut filled = true;
        while n <= CAP as u32 {
            if t.draft(n, "容封").is_err() {
                filled = false;
            }
            n += 1;
        }
        filled && t.draft(999, "超").err() == Some(CF_CAP)
    };
    s.add("J10-容量边界", j10, "超容必拒且原册不损");

    // J11 指纹随签署推进且封册后稳定
    let j11 = {
        let mut t = TriConfirm::new();
        let _ = t.draft(1, "指封");
        let f0 = t.fold();
        let _ = t.sign(1, 0);
        let f1 = t.fold();
        let _ = t.sign(1, 1);
        let f2 = t.fold();
        f0 != f1 && f1 != f2 && t.fold() == f2
    };
    s.add("J11-指纹推进", j11, "每签一步指纹一步且终态稳定");

    // J12 动态明细：未达人数计数落账
    let un = c.unsigned().len();
    s.add_dyn("J12-未达人数", un == 0, &format!("unsigned={}", un));

    // J13 动态明细：确认册指纹落账
    let fp = c.fold();
    s.add_dyn("J13-确认指纹", fp != 0, &format!("fold={:#x}", fp));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10338_tri_confirm_flow() {
        let mut c = TriConfirm::new();
        assert!(c.draft(0, "零").err() == Some(CF_ZERO));
        assert!(c.draft(1, "").err() == Some(CF_EMPTY));
        assert!(c.draft(1, "甲封").is_ok());
        assert!(c.draft(1, "重").err() == Some(CF_DUP));
        assert!(c.sign(99, 0).err() == Some(CF_NOLETTER));
        assert!(c.sign(1, 3).err() == Some(CF_FACE));
        assert!(c.sign(1, 0).is_ok());
        assert!(c.sign(1, 0).err() == Some(CF_RESIGN));
        assert!(c.letters[0].first == Some(0));
        assert!(c.sign(1, 2).is_ok());
        assert!(c.letters[0].quorum());
        assert!(c.seal().err() == Some(CF_NOLETTER));
        let mut q = 2u32;
        while q <= LETTERS_NEEDED as u32 {
            let _ = c.draft(q, "乙封");
            let _ = c.sign(q, 1);
            let _ = c.sign(q, 2);
            q += 1;
        }
        assert!(c.all_duly());
        assert!(c.seal().is_ok());
        assert!(c.sign(2, 0).err() == Some(CF_RESIGN));
    }

    #[test]
    fn unx10338_checks_green() {
        let s = run_unx10338_checks();
        assert!(s.all_passed(), "UNX-F10338 判据未全绿");
    }
}
