//! UNX-F10334 变更通知协议（三面触达回执闭环）（0x1b440 段）
//!
//! 锚点：UNX-F10334-J1 三面触达闭环——每张变更通知由一面发出、其余
//! 两面各自回执（发出面不得自回执），两张回执齐才算该通知闭环，全部
//! 通知闭环才算触达协议成立。
//! 域：CoRun·UNX-C3（联签/闭账/审计域）。
//!
//! 结构：通知板（单=通知号+发面+内容+三面回执位）+ 回执核（发面自回
//! 即拒、同面重回执即拒）+ 闭环核（异面双回执齐）+ 注入核（坏面号/
//! 重单/未知单/自回执四路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b440 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b440;
pub const TN_ZERO: u32 = SEG_BASE + 0x01; // 通知号零
pub const TN_DUP: u32 = SEG_BASE + 0x02; // 通知号重号
pub const TN_NONOTICE: u32 = SEG_BASE + 0x03; // 未知通知
pub const TN_SELF: u32 = SEG_BASE + 0x04; // 发面自回执
pub const TN_REACK: u32 = SEG_BASE + 0x05; // 同面重回执
pub const TN_CAP: u32 = SEG_BASE + 0x06; // 板超容
pub const TN_EMPTY: u32 = SEG_BASE + 0x07; // 内容空
pub const TN_FACE: u32 = SEG_BASE + 0x08; // 面号越界

/// 三面数（开区间）。
pub const FACE_N: u8 = 3;
/// 通知锚（四单）。
pub const NOTICES_NEEDED: usize = 4;
/// 板定容。
pub const CAP: usize = 10;

/// 变更通知单。
#[derive(Clone)]
pub struct ChangeNotice {
    pub nid: u32,
    pub from: u8,
    pub what: String,
    pub acks: [bool; 3],
}

impl ChangeNotice {
    /// 异面回执数。
    pub fn ack_count(&self) -> usize {
        let mut n = 0usize;
        let mut f = 0u8;
        while f < FACE_N {
            if f != self.from && self.acks[f as usize] {
                n += 1;
            }
            f += 1;
        }
        n
    }

    /// 闭环：其余两面全回执。
    pub fn closed(&self) -> bool {
        self.ack_count() == (FACE_N - 1) as usize
    }
}

/// 通知板。
pub struct NotifyBoard {
    pub notices: Vec<ChangeNotice>,
    pub sealed: bool,
}

impl NotifyBoard {
    pub fn new() -> NotifyBoard {
        NotifyBoard { notices: Vec::new(), sealed: false }
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

    /// 发通知。
    pub fn publish(&mut self, nid: u32, from: u8, what: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(TN_REACK);
        }
        if nid == 0 {
            return Err(TN_ZERO);
        }
        if from >= FACE_N {
            return Err(TN_FACE);
        }
        if self.pos(nid).is_some() {
            return Err(TN_DUP);
        }
        if what.is_empty() {
            return Err(TN_EMPTY);
        }
        if self.notices.len() >= CAP {
            return Err(TN_CAP);
        }
        self.notices.push(ChangeNotice {
            nid,
            from,
            what: what.to_string(),
            acks: [false; 3],
        });
        Ok(())
    }

    /// 回执：发面不得自回，同面不得重回。
    pub fn ack(&mut self, nid: u32, face: u8) -> Result<(), u32> {
        if self.sealed {
            return Err(TN_REACK);
        }
        if face >= FACE_N {
            return Err(TN_FACE);
        }
        match self.pos(nid) {
            None => Err(TN_NONOTICE),
            Some(p) => {
                if face == self.notices[p].from {
                    return Err(TN_SELF);
                }
                if self.notices[p].acks[face as usize] {
                    return Err(TN_REACK);
                }
                self.notices[p].acks[face as usize] = true;
                Ok(())
            }
        }
    }

    /// 未闭环单清单。
    pub fn pending(&self) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::new();
        let mut k = 0usize;
        while k < self.notices.len() {
            if !self.notices[k].closed() {
                v.push(self.notices[k].nid);
            }
            k += 1;
        }
        v
    }

    /// 触达闭环成立：单数达锚且零未闭环。
    pub fn closed_loop(&self) -> bool {
        self.notices.len() >= NOTICES_NEEDED && self.pending().is_empty()
    }

    /// 回执总数。
    pub fn ack_total(&self) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.notices.len() {
            n += self.notices[k].ack_count();
            k += 1;
        }
        n
    }

    pub fn seal(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(TN_REACK);
        }
        if !self.closed_loop() {
            return Err(TN_NONOTICE);
        }
        self.sealed = true;
        Ok(())
    }

    /// 通知板指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1b4400;
        h = h.wrapping_mul(31).wrapping_add(self.notices.len() as u32);
        let mut k = 0usize;
        while k < self.notices.len() {
            let n = &self.notices[k];
            h = h
                .wrapping_mul(31)
                .wrapping_add(n.nid)
                .wrapping_mul(31)
                .wrapping_add(n.from as u32)
                .wrapping_mul(31)
                .wrapping_add(n.ack_count() as u32);
            let wb = n.what.as_bytes();
            let mut i = 0usize;
            while i < wb.len() {
                h = h.wrapping_mul(31).wrapping_add(wb[i] as u32);
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
    let mut b = NotifyBoard::new();
    // 1) 零号与坏发面必拒
    got[0] = b.publish(0, 0, "零单").err() == Some(TN_ZERO)
        && b.publish(1, 5, "坏面").err() == Some(TN_FACE);
    // 2) 重单必拒
    let _ = b.publish(1, 0, "口径变更");
    got[1] = b.publish(1, 1, "重单").err() == Some(TN_DUP);
    // 3) 未知单回执必拒
    got[2] = b.ack(99, 1).err() == Some(TN_NONOTICE);
    // 4) 自回执必拒
    got[3] = b.ack(1, 0).err() == Some(TN_SELF);
    got
}

/// 批内判据。
pub fn run_unx10334_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10334");

    // J1 正路径：四单逐单异面双回执闭环
    let mut b = NotifyBoard::new();
    let mut ok = true;
    let mut n = 1u32;
    while n <= NOTICES_NEEDED as u32 {
        let from = (n - 1) % 3;
        let what = "变这一更";
        if b.publish(n, from as u8, what).is_err() {
            ok = false;
        }
        n += 1;
    }
    n = 1;
    while n <= NOTICES_NEEDED as u32 {
        let from = ((n - 1) % 3) as u8;
        let f1 = (from + 1) % 3;
        let f2 = (from + 2) % 3;
        if b.ack(n, f1).is_err() || b.ack(n, f2).is_err() {
            ok = false;
        }
        n += 1;
    }
    let sealed = b.seal();
    s.add(
        "J1-触达闭环",
        ok && b.closed_loop() && sealed.is_ok(),
        "四单异面双回执全闭环并封板",
    );

    // J2 回执总数对平（四单×两面）
    s.add(
        "J2-回执总数",
        b.ack_total() == NOTICES_NEEDED * 2,
        "回执总数恰为单数乘二",
    );

    // J3 注入检出：零号坏面 + 重单
    let inj = inject_four();
    s.add("J3-零号重单检出", inj[0] && inj[1], "零单号与坏发面及重单必拒");

    // J4 注入检出：未知单 + 自回执
    s.add("J4-未知自回检出", inj[2] && inj[3], "未知单回执与发面自回必拒");

    // J5 单回执不成闭环
    let j5 = {
        let mut t = NotifyBoard::new();
        let _ = t.publish(1, 0, "单回单");
        let _ = t.ack(1, 1);
        !t.notices[0].closed() && t.pending().contains(&1)
    };
    s.add("J5-单回不成环", j5, "仅一面回执不成闭环");

    // J6 重回执必拒
    let j6 = {
        let mut t = NotifyBoard::new();
        let _ = t.publish(1, 2, "重回执单");
        let _ = t.ack(1, 0);
        t.ack(1, 0).err() == Some(TN_REACK)
    };
    s.add("J6-重回执拒收", j6, "同面二次回执必拒");

    // J7 空内容必拒
    let j7 = NotifyBoard::new().publish(1, 0, "").err() == Some(TN_EMPTY);
    s.add("J7-空文拒发", j7, "通知内容空必拒");

    // J8 封板后回执与发单均拒
    let j8 = {
        let mut t = NotifyBoard::new();
        let mut q = 1u32;
        while q <= NOTICES_NEEDED as u32 {
            let from = ((q - 1) % 3) as u8;
            let _ = t.publish(q, from, "封板样");
            let _ = t.ack(q, (from + 1) % 3);
            let _ = t.ack(q, (from + 2) % 3);
            q += 1;
        }
        let _ = t.seal();
        t.ack(1, 0).err() == Some(TN_REACK) && t.publish(9, 0, "迟").err() == Some(TN_REACK)
    };
    s.add("J8-封板拒动", j8, "封板后发单与回执均拒");

    // J9 未闭环阻断封板
    let j9 = {
        let mut t = NotifyBoard::new();
        let _ = t.publish(1, 0, "挂单");
        t.seal().err() == Some(TN_NONOTICE)
    };
    s.add("J9-挂单拒封", j9, "存在未闭环单即不得封板");

    // J10 容量边界
    let j10 = {
        let mut t = NotifyBoard::new();
        let mut q = 1u32;
        let mut filled = true;
        while q <= CAP as u32 {
            let from = ((q - 1) % 3) as u8;
            if t.publish(q, from, "容单").is_err() {
                filled = false;
            }
            q += 1;
        }
        filled && t.publish(999, 0, "超").err() == Some(TN_CAP)
    };
    s.add("J10-容量边界", j10, "超容必拒且原板不损");

    // J11 指纹随回执推进且封板后稳定
    let j11 = {
        let mut t = NotifyBoard::new();
        let _ = t.publish(1, 0, "指单");
        let f0 = t.fold();
        let _ = t.ack(1, 1);
        let f1 = t.fold();
        let _ = t.ack(1, 2);
        let f2 = t.fold();
        f0 != f1 && f1 != f2 && t.fold() == f2
    };
    s.add("J11-指纹推进", j11, "每回执一步指纹一步且终态稳定");

    // J12 动态明细：未闭环计数落账
    let pd = b.pending().len();
    s.add_dyn("J12-未闭环数", pd == 0, &format!("pending={}", pd));

    // J13 动态明细：通知板指纹落账
    let fp = b.fold();
    s.add_dyn("J13-通知指纹", fp != 0, &format!("fold={:#x}", fp));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10334_tri_notify_flow() {
        let mut b = NotifyBoard::new();
        assert!(b.publish(0, 0, "零").err() == Some(TN_ZERO));
        assert!(b.publish(1, 3, "面").err() == Some(TN_FACE));
        assert!(b.publish(1, 0, "").err() == Some(TN_EMPTY));
        assert!(b.publish(1, 0, "甲单").is_ok());
        assert!(b.publish(1, 1, "重").err() == Some(TN_DUP));
        assert!(b.ack(99, 1).err() == Some(TN_NONOTICE));
        assert!(b.ack(1, 0).err() == Some(TN_SELF));
        assert!(b.ack(1, 1).is_ok());
        assert!(b.ack(1, 1).err() == Some(TN_REACK));
        assert!(!b.notices[0].closed());
        assert!(b.ack(1, 2).is_ok());
        assert!(b.notices[0].closed());
        assert!(b.seal().err() == Some(TN_NONOTICE));
        let mut q = 2u32;
        while q <= NOTICES_NEEDED as u32 {
            let from = ((q - 1) % 3) as u8;
            let _ = b.publish(q, from, "环单");
            let _ = b.ack(q, (from + 1) % 3);
            let _ = b.ack(q, (from + 2) % 3);
            q += 1;
        }
        assert!(b.closed_loop());
        assert!(b.seal().is_ok());
        assert!(b.ack(2, 0).err() == Some(TN_REACK));
    }

    #[test]
    fn unx10334_checks_green() {
        let s = run_unx10334_checks();
        assert!(s.all_passed(), "UNX-F10334 判据未全绿");
    }
}
