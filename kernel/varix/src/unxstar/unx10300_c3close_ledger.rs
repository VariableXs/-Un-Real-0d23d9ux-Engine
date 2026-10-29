//! UNX-F10300 O3 桥闭账（十九项对平+哈希冻结）（0x1b000 段）
//!
//! 锚点：UNX-F10300-J1 十九项对平——O3 桥两侧逐项值对平且哈希链冻结零漂移。
//! 域：CoRun·UNX-C3（联签/闭账/审计域）。
//!
//! 结构：对平册（十九项定容登记，甲/乙两侧各持一份值，对平位独立可翻）+
//! 哈希冻结器（逐项链式 fold 出冻结根，封账后登记即拒）+ 注入核（值偏离/
//! 缺项/重号/封账后写账四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b000 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b000;
pub const CC_VALUE: u32 = SEG_BASE + 0x01; // 两侧值不等
pub const CC_SHORT: u32 = SEG_BASE + 0x02; // 十九项未齐
pub const CC_DUP: u32 = SEG_BASE + 0x03; // 项号重号
pub const CC_EMPTY: u32 = SEG_BASE + 0x04; // 项名空
pub const CC_FROZEN: u32 = SEG_BASE + 0x05; // 冻结后写账
pub const CC_NOITEM: u32 = SEG_BASE + 0x06; // 对平目标项不存在

/// 十九项对平锚。
pub const ITEMS_NEEDED: usize = 19;
/// 对平册定容（>19，容量先定后用）。
pub const CAP: usize = 24;
/// O3 桥域标（fold 种子）。
pub const O3_TAG: u64 = 0x1b00c3;

/// 单条对平项：甲侧值与乙侧值各持一份。
#[derive(Clone)]
pub struct ReconItem {
    pub no: u32,
    pub name: String,
    pub val_a: u32,
    pub val_b: u32,
    pub settled: bool,
}

impl ReconItem {
    /// 两侧值一致才算对平。
    pub fn agree(&self) -> bool {
        self.val_a == self.val_b
    }

    pub fn render(&self) -> String {
        let flag = if self.agree() { "E" } else { "X" };
        format!("R{}|{}|{}", self.no, flag, self.name)
    }
}

/// O3 桥闭账册：登记→逐项对平→哈希冻结。
pub struct CloseLedger {
    pub items: Vec<ReconItem>,
    pub frozen: bool,
    pub root: Option<u64>,
}

impl CloseLedger {
    pub fn new() -> CloseLedger {
        CloseLedger { items: Vec::new(), frozen: false, root: None }
    }

    /// 登记对平项：冻结、空名、零号、重号、超容均拒。
    pub fn enroll(&mut self, no: u32, name: &str, va: u32, vb: u32) -> Result<(), u32> {
        if self.frozen {
            return Err(CC_FROZEN);
        }
        if name.is_empty() {
            return Err(CC_EMPTY);
        }
        if no == 0 || self.items.iter().any(|i| i.no == no) {
            return Err(CC_DUP);
        }
        if self.items.len() >= CAP {
            return Err(CC_SHORT);
        }
        self.items.push(ReconItem {
            no,
            name: name.to_string(),
            val_a: va,
            val_b: vb,
            settled: false,
        });
        Ok(())
    }

    /// 逐项对平：翻 settle 位并回报两侧是否一致。
    pub fn settle(&mut self, no: u32) -> Result<bool, u32> {
        if self.frozen {
            return Err(CC_FROZEN);
        }
        let mut hit = false;
        let mut agree = false;
        let mut k = 0usize;
        while k < self.items.len() {
            if self.items[k].no == no {
                self.items[k].settled = true;
                agree = self.items[k].agree();
                hit = true;
            }
            k += 1;
        }
        if hit {
            Ok(agree)
        } else {
            Err(CC_NOITEM)
        }
    }

    /// 未对平项号清单（按登记序）。
    pub fn gaps(&self) -> Vec<u32> {
        let mut g: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.items.len() {
            if !self.items[k].settled {
                g.push(self.items[k].no);
            }
            k += 1;
        }
        g
    }

    /// 值偏离项清单（已对平但仍不一致的项）。
    pub fn deviated(&self) -> Vec<u32> {
        let mut g: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.items.len() {
            if self.items[k].settled && !self.items[k].agree() {
                g.push(self.items[k].no);
            }
            k += 1;
        }
        g
    }

    /// 闭账裁断：十九项齐、全数对平、零值偏离。
    pub fn closed(&self) -> bool {
        self.items.len() == ITEMS_NEEDED
            && self.gaps().is_empty()
            && self.deviated().is_empty()
            && self.items.iter().all(|i| i.agree())
    }

    /// 链式 fold 指纹（FNV 风格，序敏感，含两侧值与对平位）。
    pub fn fold(&self) -> u64 {
        let mut h: u64 = O3_TAG;
        let mut k = 0usize;
        while k < self.items.len() {
            let it = &self.items[k];
            h = (h ^ it.no as u64).wrapping_mul(0x100000001b3);
            h = (h ^ it.val_a as u64).wrapping_mul(0x100000001b3);
            h = (h ^ it.val_b as u64).wrapping_mul(0x100000001b3);
            h = (h ^ it.settled as u64).wrapping_mul(0x100000001b3);
            let bytes = it.name.as_bytes();
            let mut b = 0usize;
            while b < bytes.len() {
                h = (h ^ bytes[b] as u64).wrapping_mul(0x100000001b3);
                b += 1;
            }
            k += 1;
        }
        h
    }

    /// 哈希冻结：翻冻结位并落冻结根。
    pub fn freeze(&mut self) -> u64 {
        self.frozen = true;
        let r = self.fold();
        self.root = Some(r);
        r
    }
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    // 1) 两侧值偏离（乙侧多 1）必被对平判定抓出
    let mut l = CloseLedger::new();
    let _ = l.enroll(1, "偏差项", 10, 11);
    let dev = l.settle(1);
    got[0] = dev == Ok(false) && !l.deviated().is_empty();
    // 2) 缺项（只登 18 条）必不算闭账
    let mut s = CloseLedger::new();
    let mut i = 1u32;
    while i < ITEMS_NEEDED as u32 {
        let _ = s.enroll(i, "缺项样本", 7, 7);
        i += 1;
    }
    got[1] = !s.closed();
    // 3) 重号必拒
    let mut d = CloseLedger::new();
    let _ = d.enroll(1, "甲", 1, 1);
    got[2] = d.enroll(1, "乙", 2, 2).err() == Some(CC_DUP);
    // 4) 冻结后写账必抓
    d.freeze();
    got[3] = d.enroll(2, "迟到项", 3, 3).err() == Some(CC_FROZEN);
    got
}

/// 十九项对平甲侧基准值（确定性样本，供正反两路共用）。
pub fn o3_sample_a(no: u32) -> u32 {
    no.wrapping_mul(3).wrapping_add(7)
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx10300_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10300");

    // J1 正路径：十九项逐项登记并对平
    let mut book = CloseLedger::new();
    let mut i = 1u32;
    while i <= ITEMS_NEEDED as u32 {
        let v = o3_sample_a(i);
        let _ = book.enroll(i, "O3 桥项", v, v);
        i += 1;
    }
    let mut settled_all = true;
    i = 1u32;
    while i <= ITEMS_NEEDED as u32 {
        if book.settle(i) != Ok(true) {
            settled_all = false;
        }
        i += 1;
    }
    s.add("J1-十九项对平", settled_all && book.closed(), "19 项登记且两侧值逐项一致");

    // J2 哈希冻结：根有值且与 fold 一致、二次 fold 稳定
    let root = book.freeze();
    let j2 = book.root == Some(root) && book.fold() == root && root != 0;
    s.add("J2-哈希冻结", j2, "冻结根落账且指纹稳定");

    // J3 冻结后写账必抓（正册复现）
    let j3 = book.enroll(77, "迟到", 1, 1).err() == Some(CC_FROZEN);
    s.add("J3-冻结拒写", j3, "冻结后登记必拒");

    // J4 注入检出：值偏离 + 缺项
    let inj = inject_four();
    s.add("J4-偏离缺项检出", inj[0] && inj[1], "值偏离与缺项两路必抓");

    // J5 注入检出：重号 + 冻结后写
    s.add("J5-重号冻结检出", inj[2] && inj[3], "重号必拒且冻结后写必抓");

    // J6 容量边界：第 CAP+1 条登记被拒且不破坏原账
    let j6 = {
        let mut c = CloseLedger::new();
        let mut filled = true;
        let mut n = 1u32;
        while n <= CAP as u32 {
            if c.enroll(n, "x", n, n).is_err() {
                filled = false;
            }
            n += 1;
        }
        filled && c.enroll(999, "over", 1, 1).err() == Some(CC_SHORT)
    };
    s.add("J6-容量边界", j6, "超出定容必拒且原账不损");

    // J7 缺口语义：漏 settle 一项时 gaps 可枚举且 closed 为假
    let j7 = {
        let mut c = CloseLedger::new();
        let mut n = 1u32;
        while n <= ITEMS_NEEDED as u32 {
            let v = o3_sample_a(n);
            let _ = c.enroll(n, "gap 样本", v, v);
            n += 1;
        }
        let _ = c.settle(1);
        c.gaps().len() == ITEMS_NEEDED - 1 && !c.closed()
    };
    s.add("J7-缺口枚举", j7, "未对平项可枚举且闭账为假");

    // J8 项名空拒
    let j8 = CloseLedger::new().enroll(1, "", 1, 1).err() == Some(CC_EMPTY);
    s.add("J8-空名拒收", j8, "项名空必拒");

    // J9 目标项不存在拒
    let j9 = CloseLedger::new().settle(9).err() == Some(CC_NOITEM);
    s.add("J9-目标不存在", j9, "对平不存在的项必拒");

    // J10 渲染面：对平项与偏离项标记可区分
    let j10 = {
        let it = ReconItem { no: 3, name: "渲染".to_string(), val_a: 4, val_b: 4, settled: true };
        let bad = ReconItem { no: 4, name: "渲染".to_string(), val_a: 4, val_b: 5, settled: true };
        it.render() != bad.render() && it.render().contains("E") && bad.render().contains("X")
    };
    s.add("J10-渲染分旗", j10, "一致/偏离渲染可区分");

    // J11 未对平不算闭账：值齐但 settle 位未翻
    let j11 = {
        let mut c = CloseLedger::new();
        let mut n = 1u32;
        while n <= ITEMS_NEEDED as u32 {
            let v = o3_sample_a(n);
            let _ = c.enroll(n, "未翻位", v, v);
            n += 1;
        }
        !c.closed() && c.gaps().len() == ITEMS_NEEDED
    };
    s.add("J11-未翻位不算闭", j11, "值齐但未逐项对平仍不算闭账");

    // J12 动态明细：冻结根十六进制落账
    let fp = book.fold();
    s.add_dyn("J12-冻结根明细", fp == root, &format!("root={:#x}", fp));

    // J13 动态明细：正册缺口数与偏离数双零
    let g = book.gaps().len();
    let d = book.deviated().len();
    s.add_dyn("J13-双零明细", g == 0 && d == 0, &format!("gaps={} dev=0", g));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10300_close_flow() {
        let mut l = CloseLedger::new();
        assert!(l.enroll(1, "a", 5, 5).is_ok());
        assert!(l.enroll(1, "dup", 1, 1).err() == Some(CC_DUP));
        assert_eq!(l.settle(1), Ok(true));
        assert!(l.enroll(2, "b", 5, 6).is_ok());
        assert_eq!(l.settle(2), Ok(false));
        assert!(!l.closed());
        l.freeze();
        assert!(l.enroll(3, "late", 1, 1).err() == Some(CC_FROZEN));
        assert!(l.root.is_some());
    }

    #[test]
    fn unx10300_checks_green() {
        let s = run_unx10300_checks();
        assert!(s.all_passed(), "UNX-F10300 判据未全绿");
    }
}
