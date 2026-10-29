//! UNX-F10314 变更通知协议（一方变更另一方触达）（0x1b1c0 段）
//!
//! 锚点：UNX-F10314-J1 触达闭环——一方发布的变更逐条由对侧签收回执，
//! 自签/重签/未知号即拒，全部回执齐才闭环。
//! 域：CoRun·UNX-C3（联签/闭账/审计域）。
//!
//! 结构：通知板（变更通知：号+事项+发布方登记）+ 回执账（触达方签收，
//! 自签即拒）+ 闭环核（逐通知对侧回执齐备）+ 注入核（自签/漏回执/
//! 坏方/重签四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b1c0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b1c0;
pub const NT_DUP: u32 = SEG_BASE + 0x01; // 通知号重号
pub const NT_EMPTY: u32 = SEG_BASE + 0x02; // 事项空
pub const NT_BADFROM: u32 = SEG_BASE + 0x03; // 发布方非法
pub const NT_CAP: u32 = SEG_BASE + 0x04; // 板超容
pub const NT_NONOTICE: u32 = SEG_BASE + 0x05; // 未知通知
pub const NT_SELFACK: u32 = SEG_BASE + 0x06; // 自签回执
pub const NT_DUPACK: u32 = SEG_BASE + 0x07; // 重复回执

/// 发布方：1=甲 2=乙。
pub const FROM_A: u8 = 1;
pub const FROM_B: u8 = 2;

/// 通知板定容。
pub const CAP: usize = 16;
/// 通知数锚。
pub const NOTICES_NEEDED: usize = 5;

/// 单条变更通知。
#[derive(Clone)]
pub struct ChangeNotice {
    pub nid: u32,
    pub what: String,
    pub from: u8,
}

impl ChangeNotice {
    /// 对侧方（触达方）。
    pub fn peer(&self) -> u8 {
        if self.from == FROM_A {
            FROM_B
        } else {
            FROM_A
        }
    }
}

/// 单条回执。
#[derive(Clone)]
pub struct Receipt {
    pub nid: u32,
    pub by: u8,
}

/// 变更通知板。
pub struct NotifyBoard {
    pub notices: Vec<ChangeNotice>,
    pub receipts: Vec<Receipt>,
}

impl NotifyBoard {
    pub fn new() -> NotifyBoard {
        NotifyBoard { notices: Vec::new(), receipts: Vec::new() }
    }

    fn pos(&self, nid: u32) -> Option<usize> {
        let mut k = 0usize;
        while k < self.notices.len() {
            if self.notices[k].nid == nid {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// 发布变更：重号、空事项、坏方、超容均拒。
    pub fn publish(&mut self, nid: u32, what: &str, from: u8) -> Result<(), u32> {
        if nid == 0 || self.pos(nid).is_some() {
            return Err(NT_DUP);
        }
        if what.is_empty() {
            return Err(NT_EMPTY);
        }
        if from != FROM_A && from != FROM_B {
            return Err(NT_BADFROM);
        }
        if self.notices.len() >= CAP {
            return Err(NT_CAP);
        }
        self.notices.push(ChangeNotice { nid, what: what.to_string(), from });
        Ok(())
    }

    /// 签收回执：未知号、自签、重签均拒。
    pub fn ack(&mut self, nid: u32, by: u8) -> Result<(), u32> {
        let k = match self.pos(nid) {
            None => return Err(NT_NONOTICE),
            Some(k) => k,
        };
        if by != FROM_A && by != FROM_B {
            return Err(NT_BADFROM);
        }
        if self.notices[k].from == by {
            return Err(NT_SELFACK);
        }
        if self.receipts.iter().any(|r| r.nid == nid) {
            return Err(NT_DUPACK);
        }
        self.receipts.push(Receipt { nid, by });
        Ok(())
    }

    /// 单通知回执齐备：对侧已签。
    pub fn acked(&self, nid: u32) -> bool {
        match self.pos(nid) {
            None => false,
            Some(k) => {
                let peer = self.notices[k].peer();
                self.receipts.iter().any(|r| r.nid == nid && r.by == peer)
            }
        }
    }

    /// 闭环：全部通知回执齐备。
    pub fn closed_loop(&self) -> bool {
        self.notices.len() >= NOTICES_NEEDED && self.notices.iter().all(|n| self.acked(n.nid))
    }

    /// 未触达通知清单。
    pub fn pending(&self) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.notices.len() {
            if !self.acked(self.notices[k].nid) {
                v.push(self.notices[k].nid);
            }
            k += 1;
        }
        v
    }

    /// 板指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1b1c00;
        h = h.wrapping_mul(31).wrapping_add(self.notices.len() as u32);
        let mut k = 0usize;
        while k < self.notices.len() {
            let n = &self.notices[k];
            h = h.wrapping_mul(31).wrapping_add(n.nid);
            h = h.wrapping_mul(31).wrapping_add(n.from as u32);
            let bytes = n.what.as_bytes();
            let mut b = 0usize;
            while b < bytes.len() {
                h = h.wrapping_mul(31).wrapping_add(bytes[b] as u32);
                b += 1;
            }
            k += 1;
        }
        h = h.wrapping_mul(31).wrapping_add(self.receipts.len() as u32);
        h
    }
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    let mut b = NotifyBoard::new();
    let _ = b.publish(1, "样本变更", FROM_A);
    // 1) 自签必拒
    got[0] = b.ack(1, FROM_A).err() == Some(NT_SELFACK);
    // 2) 重签必拒（先合法回执）
    let _ = b.ack(1, FROM_B);
    got[1] = b.ack(1, FROM_B).err() == Some(NT_DUPACK);
    // 3) 未知通知必拒
    got[2] = b.ack(9, FROM_B).err() == Some(NT_NONOTICE);
    // 4) 漏回执堵闭环
    let _ = b.publish(2, "未触达", FROM_B);
    got[3] = !b.closed_loop() && b.pending() == [2u32];
    got
}

/// 批内判据。
pub fn run_unx10314_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10314");

    // J1 正路径：五通知逐条对侧回执闭环
    let mut b = NotifyBoard::new();
    let mut ok = true;
    let mut n = 1u32;
    while n <= NOTICES_NEEDED as u32 {
        let from = if n % 2 == 0 { FROM_B } else { FROM_A };
        if b.publish(n, "口径变更", from).is_err() {
            ok = false;
        }
        n += 1;
    }
    n = 1u32;
    while n <= NOTICES_NEEDED as u32 {
        let from = if n % 2 == 0 { FROM_B } else { FROM_A };
        let peer = if from == FROM_A { FROM_B } else { FROM_A };
        if b.ack(n, peer).is_err() {
            ok = false;
        }
        n += 1;
    }
    s.add("J1-五通知闭环", ok && b.closed_loop(), "五通知逐条对侧回执齐备");

    // J2 逐通知触达核
    let j2 = {
        let mut ok = true;
        let mut n = 1u32;
        while n <= NOTICES_NEEDED as u32 {
            if !b.acked(n) {
                ok = false;
            }
            n += 1;
        }
        ok
    };
    s.add("J2-逐条触达", j2, "五条逐条回执命中");

    // J3 注入检出：自签 + 重签
    let inj = inject_four();
    s.add("J3-自签重签检出", inj[0] && inj[1], "自签必拒且重签必拒");

    // J4 注入检出：未知通知 + 漏回执
    s.add("J4-未知漏执检出", inj[2] && inj[3], "未知通知必拒且漏回执堵闭环");

    // J5 对侧方推导：甲发对乙、乙发对甲
    let j5 = {
        let na = ChangeNotice { nid: 1, what: "x".to_string(), from: FROM_A };
        let nb = ChangeNotice { nid: 2, what: "y".to_string(), from: FROM_B };
        na.peer() == FROM_B && nb.peer() == FROM_A
    };
    s.add("J5-对侧推导", j5, "触达方恒为发布方对侧");

    // J6 空事项拒
    let j6 = NotifyBoard::new().publish(1, "", FROM_A).err() == Some(NT_EMPTY);
    s.add("J6-空事项拒", j6, "变更事项空必拒");

    // J7 坏方拒
    let j7 = NotifyBoard::new().publish(1, "x", 7).err() == Some(NT_BADFROM)
        && NotifyBoard::new().publish(1, "x", 0).err() == Some(NT_BADFROM);
    s.add("J7-坏方拒收", j7, "发布方出两方域必拒");

    // J8 重号通知拒
    let j8 = {
        let mut c = NotifyBoard::new();
        let _ = c.publish(1, "甲", FROM_A);
        c.publish(1, "乙", FROM_B).err() == Some(NT_DUP)
    };
    s.add("J8-重号拒收", j8, "通知号重号必拒");

    // J9 容量边界
    let j9 = {
        let mut c = NotifyBoard::new();
        let mut filled = true;
        let mut n = 1u32;
        while n <= CAP as u32 {
            let what = format!("w{}", n);
            if c.publish(n, &what, FROM_A).is_err() {
                filled = false;
            }
            n += 1;
        }
        filled && c.publish(999, "over", FROM_A).err() == Some(NT_CAP)
    };
    s.add("J9-容量边界", j9, "板超容必拒且原板不损");

    // J10 回执方必须是登记在册的双方之一（先查未知再查自签）
    let j10 = {
        let mut c = NotifyBoard::new();
        let _ = c.publish(1, "x", FROM_A);
        c.ack(1, 9).err() == Some(NT_BADFROM)
    };
    s.add("J10-回执坏方拒", j10, "回执方非法必拒");

    // J11 板指纹稳定且随回执推进
    let j11 = {
        let mut c = NotifyBoard::new();
        let _ = c.publish(1, "x", FROM_A);
        let f0 = c.fold();
        let _ = c.ack(1, FROM_B);
        let f1 = c.fold();
        f0 != f1 && c.fold() == f1
    };
    s.add("J11-指纹随执", j11, "回执一步指纹一步且终态稳定");

    // J12 动态明细：回执计数落账
    let rc = b.receipts.len();
    s.add_dyn("J12-回执计数", rc == NOTICES_NEEDED, &format!("receipts={}", rc));

    // J13 动态明细：未触达零清单
    let pd = b.pending().len();
    s.add_dyn("J13-未触达零", pd == 0, &format!("pending={}", pd));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10314_notify_flow() {
        let mut b = NotifyBoard::new();
        assert!(b.publish(1, "a", FROM_A).is_ok());
        assert!(b.publish(1, "b", FROM_B).err() == Some(NT_DUP));
        assert!(b.publish(2, "", FROM_A).err() == Some(NT_EMPTY));
        assert!(b.publish(3, "x", 7).err() == Some(NT_BADFROM));
        assert!(b.ack(9, FROM_B).err() == Some(NT_NONOTICE));
        assert!(b.ack(1, FROM_A).err() == Some(NT_SELFACK));
        assert!(b.ack(1, 9).err() == Some(NT_BADFROM));
        assert!(b.ack(1, FROM_B).is_ok());
        assert!(b.acked(1));
        assert!(b.ack(1, FROM_B).err() == Some(NT_DUPACK));
        assert!(!b.closed_loop());
    }

    #[test]
    fn unx10314_checks_green() {
        let s = run_unx10314_checks();
        assert!(s.all_passed(), "UNX-F10314 判据未全绿");
    }
}
