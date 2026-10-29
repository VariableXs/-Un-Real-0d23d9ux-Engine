//! UNX-F10318 联签确认书（双签）（0x1b240 段）
//!
//! 锚点：UNX-F10318-J1 双签确认——确认书逐份甲乙各签一次（先后皆可），
//! 同方双签/零签章/未草签即拒，全册双签齐才成约。
//! 域：CoRun·UNX-C3（联签/闭账/审计域）。
//!
//! 结构：确认书册（号+题+甲签章/乙签章+签序四元登记）+ 签章核（签章
//! 非零且同方不可重签，先签方定签序）+ 成约核（双章齐备且签序落定）+
//! 注入核（同方重签/签前无书/零签章/重号四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b240 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b240;
pub const CF_DUP: u32 = SEG_BASE + 0x01; // 书号重号
pub const CF_EMPTY: u32 = SEG_BASE + 0x02; // 书题空
pub const CF_CAP: u32 = SEG_BASE + 0x03; // 册超容
pub const CF_NOLID: u32 = SEG_BASE + 0x04; // 签前无书
pub const CF_RESIGN: u32 = SEG_BASE + 0x05; // 同方重签
pub const CF_ZERO: u32 = SEG_BASE + 0x06; // 零签章

/// 签序：1=甲先 2=乙先 0=未签。
pub const ORDER_NONE: u8 = 0;
pub const ORDER_A_FIRST: u8 = 1;
pub const ORDER_B_FIRST: u8 = 2;

/// 确认书册定容。
pub const CAP: usize = 12;
/// 确认书数锚。
pub const LETTERS_NEEDED: usize = 4;

/// 单份确认书。
#[derive(Clone)]
pub struct ConfirmLetter {
    pub lid: u32,
    pub subject: String,
    pub sign_a: Option<u32>,
    pub sign_b: Option<u32>,
    pub order: u8,
}

impl ConfirmLetter {
    /// 双签齐备且签序落定。
    pub fn duly(&self) -> bool {
        self.sign_a.is_some() && self.sign_b.is_some() && self.order != ORDER_NONE
    }

    /// 首签方。
    pub fn first_signer(&self) -> u8 {
        if self.order == ORDER_A_FIRST {
            1
        } else if self.order == ORDER_B_FIRST {
            2
        } else {
            0
        }
    }
}

/// 联签确认书册。
pub struct ConfirmBook {
    pub letters: Vec<ConfirmLetter>,
}

impl ConfirmBook {
    pub fn new() -> ConfirmBook {
        ConfirmBook { letters: Vec::new() }
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

    /// 草拟确认书：重号、空题、超容均拒。
    pub fn draft(&mut self, lid: u32, subject: &str) -> Result<(), u32> {
        if lid == 0 || self.pos(lid).is_some() {
            return Err(CF_DUP);
        }
        if subject.is_empty() {
            return Err(CF_EMPTY);
        }
        if self.letters.len() >= CAP {
            return Err(CF_CAP);
        }
        self.letters.push(ConfirmLetter {
            lid,
            subject: subject.to_string(),
            sign_a: None,
            sign_b: None,
            order: ORDER_NONE,
        });
        Ok(())
    }

    /// 甲侧签章。
    pub fn sign_a(&mut self, lid: u32, token: u32) -> Result<(), u32> {
        match self.pos(lid) {
            None => Err(CF_NOLID),
            Some(k) => {
                if token == 0 {
                    return Err(CF_ZERO);
                }
                if self.letters[k].sign_a.is_some() {
                    return Err(CF_RESIGN);
                }
                self.letters[k].sign_a = Some(token);
                if self.letters[k].order == ORDER_NONE {
                    self.letters[k].order = ORDER_A_FIRST;
                }
                Ok(())
            }
        }
    }

    /// 乙侧签章。
    pub fn sign_b(&mut self, lid: u32, token: u32) -> Result<(), u32> {
        match self.pos(lid) {
            None => Err(CF_NOLID),
            Some(k) => {
                if token == 0 {
                    return Err(CF_ZERO);
                }
                if self.letters[k].sign_b.is_some() {
                    return Err(CF_RESIGN);
                }
                self.letters[k].sign_b = Some(token);
                if self.letters[k].order == ORDER_NONE {
                    self.letters[k].order = ORDER_B_FIRST;
                }
                Ok(())
            }
        }
    }

    /// 全册成约：书数达锚且逐份双签齐。
    pub fn all_duly(&self) -> bool {
        self.letters.len() >= LETTERS_NEEDED && self.letters.iter().all(|l| l.duly())
    }

    /// 未成约书清单。
    pub fn unsigned(&self) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.letters.len() {
            if !self.letters[k].duly() {
                v.push(self.letters[k].lid);
            }
            k += 1;
        }
        v
    }

    /// 甲先签书数（签序分布面）。
    pub fn a_first_count(&self) -> usize {
        self.letters.iter().filter(|l| l.order == ORDER_A_FIRST).count()
    }

    /// 册指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1b2400;
        h = h.wrapping_mul(31).wrapping_add(self.letters.len() as u32);
        let mut k = 0usize;
        while k < self.letters.len() {
            let l = &self.letters[k];
            h = h.wrapping_mul(31).wrapping_add(l.lid);
            h = h.wrapping_mul(31).wrapping_add(l.order as u32);
            h = h.wrapping_mul(31).wrapping_add(l.sign_a.unwrap_or(0));
            h = h.wrapping_mul(31).wrapping_add(l.sign_b.unwrap_or(0));
            let bytes = l.subject.as_bytes();
            let mut b = 0usize;
            while b < bytes.len() {
                h = h.wrapping_mul(31).wrapping_add(bytes[b] as u32);
                b += 1;
            }
            k += 1;
        }
        h
    }
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    let mut b = ConfirmBook::new();
    let _ = b.draft(1, "样本书");
    // 1) 同方重签必拒
    let _ = b.sign_a(1, 111);
    got[0] = b.sign_a(1, 222).err() == Some(CF_RESIGN);
    // 2) 签前无书必拒
    got[1] = b.sign_b(9, 1).err() == Some(CF_NOLID);
    // 3) 零签章必拒
    got[2] = b.sign_b(1, 0).err() == Some(CF_ZERO);
    // 4) 半签不成约
    got[3] = !b.letters[0].duly() && b.unsigned() == [1u32];
    got
}

/// 批内判据。
pub fn run_unx10318_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10318");

    // J1 正路径：四书草拟双签成约（甲乙交替先签）
    let mut b = ConfirmBook::new();
    let mut ok = true;
    let mut n = 1u32;
    while n <= LETTERS_NEEDED as u32 {
        let sub = format!("确认书{}", n);
        if b.draft(n, &sub).is_err() {
            ok = false;
        }
        n += 1;
    }
    n = 1u32;
    while n <= LETTERS_NEEDED as u32 {
        let a_first = n % 2 == 1;
        let r = if a_first {
            b.sign_a(n, 100 + n).and_then(|_| b.sign_b(n, 200 + n))
        } else {
            b.sign_b(n, 200 + n).and_then(|_| b.sign_a(n, 100 + n))
        };
        if r.is_err() {
            ok = false;
        }
        n += 1;
    }
    s.add("J1-四书双签成约", ok && b.all_duly(), "四书双签齐备且签序落定");

    // J2 签序分布：甲先两条乙先两条
    let j2 = b.a_first_count() == 2
        && b.letters[0].first_signer() == 1
        && b.letters[1].first_signer() == 2;
    s.add("J2-签序分布", j2, "先签方按实际签章次序落定");

    // J3 注入检出：同方重签 + 签前无书
    let inj = inject_four();
    s.add("J3-重签无书检出", inj[0] && inj[1], "同方重签必拒且签前无书必拒");

    // J4 注入检出：零签章 + 半签不成约
    s.add("J4-零章半签检出", inj[2] && inj[3], "零签章必拒且半签不成约");

    // J5 重号书拒
    let j5 = {
        let mut c = ConfirmBook::new();
        let _ = c.draft(1, "甲");
        c.draft(1, "乙").err() == Some(CF_DUP)
    };
    s.add("J5-重号拒收", j5, "书号重号必拒");

    // J6 空题拒
    let j6 = ConfirmBook::new().draft(1, "").err() == Some(CF_EMPTY);
    s.add("J6-空题拒收", j6, "书题空必拒");

    // J7 交替签序不影响成约：乙先甲后同样 duly
    let j7 = {
        let mut c = ConfirmBook::new();
        let _ = c.draft(1, "乙先");
        let _ = c.sign_b(1, 7);
        let _ = c.sign_a(1, 3);
        c.letters[0].duly() && c.letters[0].order == ORDER_B_FIRST
    };
    s.add("J7-乙先成约", j7, "乙先甲后签序合法且成约");

    // J8 容量边界
    let j8 = {
        let mut c = ConfirmBook::new();
        let mut filled = true;
        let mut n = 1u32;
        while n <= CAP as u32 {
            let sub = format!("c{}", n);
            if c.draft(n, &sub).is_err() {
                filled = false;
            }
            n += 1;
        }
        filled && c.draft(999, "over").err() == Some(CF_CAP)
    };
    s.add("J8-容量边界", j8, "册超容必拒且原册不损");

    // J9 签章入指纹且终态稳定
    let j9 = {
        let mut c = ConfirmBook::new();
        let _ = c.draft(1, "指");
        let f0 = c.fold();
        let _ = c.sign_a(1, 5);
        let f1 = c.fold();
        let _ = c.sign_b(1, 6);
        let f2 = c.fold();
        f0 != f1 && f1 != f2 && c.fold() == f2
    };
    s.add("J9-指纹随签", j9, "每签一步指纹一步且终态稳定");

    // J10 零号书拒
    let j10 = ConfirmBook::new().draft(0, "零").err() == Some(CF_DUP);
    s.add("J10-零号拒收", j10, "零号书按重号面拒");

    // J11 动态明细：成约书数落账
    let dc = b.letters.iter().filter(|l| l.duly()).count();
    s.add_dyn("J11-成约计数", dc == LETTERS_NEEDED, &format!("duly={}", dc));

    // J12 动态明细：未成约零清单
    let u = b.unsigned().len();
    s.add_dyn("J12-未签零清单", u == 0, &format!("unsigned={}", u));

    // J13 动态明细：册指纹落账
    let fp = b.fold();
    s.add_dyn("J13-册指纹", fp != 0, &format!("fold={:#x}", fp));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10318_confirm_flow() {
        let mut b = ConfirmBook::new();
        assert!(b.draft(1, "s").is_ok());
        assert!(b.draft(1, "dup").err() == Some(CF_DUP));
        assert!(b.draft(0, "zero").err() == Some(CF_DUP));
        assert!(b.draft(2, "").err() == Some(CF_EMPTY));
        assert!(b.sign_a(9, 1).err() == Some(CF_NOLID));
        assert!(b.sign_a(1, 0).err() == Some(CF_ZERO));
        assert!(b.sign_a(1, 11).is_ok());
        assert!(b.sign_a(1, 12).err() == Some(CF_RESIGN));
        assert!(b.sign_b(1, 21).is_ok());
        assert!(b.letters[0].duly());
        assert_eq!(b.letters[0].order, ORDER_A_FIRST);
        assert!(b.unsigned().is_empty());
    }

    #[test]
    fn unx10318_checks_green() {
        let s = run_unx10318_checks();
        assert!(s.all_passed(), "UNX-F10318 判据未全绿");
    }
}
