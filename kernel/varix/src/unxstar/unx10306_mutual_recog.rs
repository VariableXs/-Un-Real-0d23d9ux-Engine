//! UNX-F10306 判据互认协议（双方判据交叉登记）（0x1b0c0 段）
//!
//! 锚点：UNX-F10306-J1 交叉登记双射——甲侧每条判据恰映射乙侧一条、
//! 乙侧每条恰被甲侧一条映射，双向确认齐备才算互认成约。
//! 域：CoRun·UNX-C3（联签/闭账/审计域）。
//!
//! 结构：映射册（a_cid→b_cid 有序对，双射约束：a 键唯一且 b 键唯一）+
//! 确认核（甲/乙两侧各对映射逐条确认）+ 双射核（覆盖 1..=n 全域无缺
//! 无重）+ 注入核（a 键复用/b 键复用/确认未知映射/缺映射四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b0c0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b0c0;
pub const MR_DUPA: u32 = SEG_BASE + 0x01; // 甲侧键复用
pub const MR_DUPB: u32 = SEG_BASE + 0x02; // 乙侧键复用
pub const MR_EMPTY: u32 = SEG_BASE + 0x03; // 映射题空
pub const MR_ZERO: u32 = SEG_BASE + 0x04; // 键为零
pub const MR_CAP: u32 = SEG_BASE + 0x05; // 册超容
pub const MR_NOMAP: u32 = SEG_BASE + 0x06; // 确认未知映射
pub const MR_GAP: u32 = SEG_BASE + 0x07; // 双射有缺

/// 映射册定容。
pub const CAP: usize = 12;
/// 互认映射数锚（双方各 6 条判据全交叉）。
pub const MAPS_NEEDED: usize = 6;

/// 单条交叉映射：甲 cid ↔ 乙 cid。
#[derive(Clone)]
pub struct CrossPair {
    pub a_cid: u32,
    pub b_cid: u32,
    pub title: String,
    pub a_ok: bool,
    pub b_ok: bool,
}

impl CrossPair {
    pub fn confirmed(&self) -> bool {
        self.a_ok && self.b_ok
    }
}

/// 判据互认协议册。
pub struct MutualReg {
    pub pairs: Vec<CrossPair>,
}

impl MutualReg {
    pub fn new() -> MutualReg {
        MutualReg { pairs: Vec::new() }
    }

    fn has_a(&self, a: u32) -> bool {
        self.pairs.iter().any(|p| p.a_cid == a)
    }

    fn has_b(&self, b: u32) -> bool {
        self.pairs.iter().any(|p| p.b_cid == b)
    }

    fn pos_a(&self, a: u32) -> Option<usize> {
        let mut k = 0usize;
        while k < self.pairs.len() {
            if self.pairs[k].a_cid == a {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// 登记映射：键零、空题、a 复用、b 复用、超容均拒。
    pub fn map(&mut self, a: u32, b: u32, title: &str) -> Result<(), u32> {
        if a == 0 || b == 0 {
            return Err(MR_ZERO);
        }
        if title.is_empty() {
            return Err(MR_EMPTY);
        }
        if self.has_a(a) {
            return Err(MR_DUPA);
        }
        if self.has_b(b) {
            return Err(MR_DUPB);
        }
        if self.pairs.len() >= CAP {
            return Err(MR_CAP);
        }
        self.pairs.push(CrossPair { a_cid: a, b_cid: b, title: title.to_string(), a_ok: false, b_ok: false });
        Ok(())
    }

    /// 甲侧确认（按甲键）。
    pub fn a_confirm(&mut self, a: u32) -> Result<(), u32> {
        match self.pos_a(a) {
            None => Err(MR_NOMAP),
            Some(k) => {
                self.pairs[k].a_ok = true;
                Ok(())
            }
        }
    }

    /// 乙侧确认（按乙键）。
    pub fn b_confirm(&mut self, b: u32) -> Result<(), u32> {
        let mut k = 0usize;
        while k < self.pairs.len() {
            if self.pairs[k].b_cid == b {
                self.pairs[k].b_ok = true;
                return Ok(());
            }
            k += 1;
        }
        Err(MR_NOMAP)
    }

    /// 双射核：甲键 1..=an 与乙键 1..=bn 全覆盖且一一对应。
    pub fn bijection(&self, an: usize, bn: usize) -> Result<(), u32> {
        if self.pairs.len() != an || an != bn {
            return Err(MR_GAP);
        }
        // 甲键覆盖核：1..=an 每键恰出现一次
        let mut i = 1u32;
        while i <= an as u32 {
            if !self.has_a(i) {
                return Err(MR_GAP);
            }
            i += 1;
        }
        // 乙键覆盖核：1..=bn 每键恰出现一次（唯一性已由登记约束保证）
        let mut j = 1u32;
        while j <= bn as u32 {
            if !self.has_b(j) {
                return Err(MR_GAP);
            }
            j += 1;
        }
        Ok(())
    }

    /// 全确认裁断：映射齐、双射成立、逐条双侧确认。
    pub fn agreed(&self, an: usize, bn: usize) -> bool {
        self.bijection(an, bn).is_ok() && self.pairs.iter().all(|p| p.confirmed())
    }

    /// 未确认映射数。
    pub fn pending(&self) -> usize {
        self.pairs.iter().filter(|p| !p.confirmed()).count()
    }

    /// 册折叠指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1b0c00;
        h = h.wrapping_mul(31).wrapping_add(self.pairs.len() as u32);
        let mut k = 0usize;
        while k < self.pairs.len() {
            let p = &self.pairs[k];
            h = h.wrapping_mul(31).wrapping_add(p.a_cid);
            h = h.wrapping_mul(31).wrapping_add(p.b_cid);
            h = h.wrapping_mul(31).wrapping_add((p.a_ok as u32) * 2 + p.b_ok as u32);
            let bytes = p.title.as_bytes();
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
    // 1) 甲键复用必拒
    let mut m = MutualReg::new();
    let _ = m.map(1, 1, "甲");
    got[0] = m.map(1, 2, "乙").err() == Some(MR_DUPA);
    // 2) 乙键复用必拒
    got[1] = m.map(2, 1, "丙").err() == Some(MR_DUPB);
    // 3) 确认未知映射必拒
    got[2] = m.a_confirm(9).err() == Some(MR_NOMAP) && m.b_confirm(9).err() == Some(MR_NOMAP);
    // 4) 缺映射必判双射失败（甲 1..=3 只登两条）
    let mut g = MutualReg::new();
    let _ = g.map(1, 1, "一");
    let _ = g.map(2, 2, "二");
    got[3] = g.bijection(3, 3).err() == Some(MR_GAP);
    got
}

/// 批内判据。
pub fn run_unx10306_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10306");

    // J1 正路径：六映射交叉登记并双侧全确认
    let mut m = MutualReg::new();
    let mut n = 1u32;
    let mut ok = true;
    while n <= MAPS_NEEDED as u32 {
        if m.map(n, n, "互认映射").is_err() {
            ok = false;
        }
        n += 1;
    }
    n = 1u32;
    while n <= MAPS_NEEDED as u32 {
        if m.a_confirm(n).is_err() || m.b_confirm(n).is_err() {
            ok = false;
        }
        n += 1;
    }
    s.add("J1-六映射全确认", ok && m.agreed(MAPS_NEEDED, MAPS_NEEDED), "双射成立且逐条双确认");

    // J2 交叉映射（a 序与 b 序错位仍双射）
    let j2 = {
        let mut c = MutualReg::new();
        let _ = c.map(1, 2, "错位一");
        let _ = c.map(2, 1, "错位二");
        c.bijection(2, 2).is_ok()
    };
    s.add("J2-错位双射", j2, "a/b 键错位对应仍是合法双射");

    // J3 注入检出：甲键复用 + 乙键复用
    let inj = inject_four();
    s.add("J3-键复用检出", inj[0] && inj[1], "两侧键复用必拒");

    // J4 注入检出：未知映射 + 缺映射
    s.add("J4-缺映检出", inj[2] && inj[3], "未知确认必拒且缺映射判双射失败");

    // J5 待确认语义：只签甲侧不全成
    let j5 = {
        let mut c = MutualReg::new();
        let _ = c.map(1, 1, "半签");
        let _ = c.a_confirm(1);
        !c.agreed(1, 1) && c.pending() == 1
    };
    s.add("J5-半签不成约", j5, "单侧确认不算互认成约且缺位可数");

    // J6 键为零拒
    let j6 = MutualReg::new().map(0, 1, "零键").err() == Some(MR_ZERO)
        && MutualReg::new().map(1, 0, "零键").err() == Some(MR_ZERO);
    s.add("J6-零键拒收", j6, "任一侧键为零必拒");

    // J7 空题拒
    let j7 = MutualReg::new().map(1, 1, "").err() == Some(MR_EMPTY);
    s.add("J7-空题拒收", j7, "映射题空必拒");

    // J8 容量边界
    let j8 = {
        let mut c = MutualReg::new();
        let mut filled = true;
        let mut n = 1u32;
        while n <= CAP as u32 {
            if c.map(n, n, "x").is_err() {
                filled = false;
            }
            n += 1;
        }
        filled && c.map(99, 99, "over").err() == Some(MR_CAP)
    };
    s.add("J8-容量边界", j8, "超容必拒且原册不损");

    // J9 指纹随确认推进且终态稳定
    let j9 = {
        let mut c = MutualReg::new();
        let _ = c.map(1, 1, "指");
        let f0 = c.fold();
        let _ = c.a_confirm(1);
        let f1 = c.fold();
        let _ = c.b_confirm(1);
        f0 != f1 && c.fold() != f1 && c.fold() == c.fold()
    };
    s.add("J9-指纹随签", j9, "每步确认指纹推进且终态稳定");

    // J10 双射规模失配拒（甲 3 条乙 4 条）
    let j10 = {
        let mut c = MutualReg::new();
        let mut n = 1u32;
        while n <= 3 {
            let _ = c.map(n, n, "甲");
            n += 1;
        }
        c.bijection(3, 4).err() == Some(MR_GAP)
    };
    s.add("J10-规模失配", j10, "两侧判据数不等即双射失败");

    // J11 确认序无关：先乙后甲同样成约
    let j11 = {
        let mut c = MutualReg::new();
        let _ = c.map(1, 1, "序");
        let _ = c.b_confirm(1);
        let _ = c.a_confirm(1);
        c.agreed(1, 1)
    };
    s.add("J11-确认序无关", j11, "确认先后不影响成约裁断");

    // J12 动态明细：成约映射数落账
    let ac = m.pairs.iter().filter(|p| p.confirmed()).count();
    s.add_dyn("J12-成约计数", ac == MAPS_NEEDED, &format!("agreed={}", ac));

    // J13 动态明细：待确认零清单
    let pd = m.pending();
    s.add_dyn("J13-待确认零", pd == 0, &format!("pending={}", pd));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10306_mutual_flow() {
        let mut m = MutualReg::new();
        assert!(m.map(1, 1, "a").is_ok());
        assert!(m.map(1, 2, "dupa").err() == Some(MR_DUPA));
        assert!(m.map(2, 1, "dupb").err() == Some(MR_DUPB));
        assert!(m.map(0, 3, "zero").err() == Some(MR_ZERO));
        assert!(m.a_confirm(7).err() == Some(MR_NOMAP));
        assert!(m.a_confirm(1).is_ok() && m.b_confirm(1).is_ok());
        assert!(m.agreed(1, 1));
        assert!(m.map(2, 2, "b").is_ok());
        assert!(m.bijection(2, 2).is_ok());
        assert!(m.bijection(3, 3).err() == Some(MR_GAP));
    }

    #[test]
    fn unx10306_checks_green() {
        let s = run_unx10306_checks();
        assert!(s.all_passed(), "UNX-F10306 判据未全绿");
    }
}
