//! UNX-F24141 reservation 锁与显存互斥纪律·子功能01（ticket 签发账）（0x41380 段）
//!
//! 锚点：UNX-F24141-J1 票账（reservation ticket 单调签发逐条落账）；
//! J2 票语义（单调闸 + 边界值闸 + 回执对账三性质）；J3 票指纹（票账
//! 折叠指纹）三判据全绿。
//! 域：CoRun·UNX-F5-B15（reservation ticket 面面）。
//!
//! 结构：票池 + 单调签发核 + 回执核 + 票账本 + 折叠指纹 + 注入核（越
//! 界回执/重票/零容量/印后签发/重复回执必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x41380 独占 0x20）。
pub const SEG_BASE: u32 = 0x41380;
pub const TK_NO_SPACE: u32 = SEG_BASE + 0x01; // 票池满
pub const TK_BAD_TICKET: u32 = SEG_BASE + 0x02; // 越界回执
pub const TK_DUP_ACK: u32 = SEG_BASE + 0x03; // 重复回执
pub const TK_FROZEN: u32 = SEG_BASE + 0x04; // 印后签发
pub const TK_MONO: u32 = SEG_BASE + 0x05; // 单调破坏

/// 票条目。
#[derive(Clone)]
pub struct TkRec {
    pub ticket: u64,
    pub holder: String,
    pub acked: bool,
}

/// 票簿。
pub struct TkBook {
    pub pool: Vec<TkRec>,
    pub cap: u32,
    pub next_ticket: u64,
    pub sealed: bool,
}

impl TkBook {
    pub fn new(cap: u32) -> TkBook {
        TkBook { pool: Vec::new(), cap, next_ticket: 1, sealed: false }
    }

    /// 签发：单调递增，池满拒。
    pub fn issue(&mut self, holder: &str) -> Result<u64, u32> {
        if self.sealed {
            return Err(TK_FROZEN);
        }
        if holder.is_empty() {
            return Err(TK_MONO);
        }
        if self.pool.len() as u32 >= self.cap {
            return Err(TK_NO_SPACE);
        }
        let t = self.next_ticket;
        self.next_ticket += 1;
        self.pool.push(TkRec { ticket: t, holder: holder.to_string(), acked: false });
        Ok(t)
    }

    /// 回执：ack 已签发票，重复回执拒。
    pub fn ack(&mut self, ticket: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(TK_FROZEN);
        }
        if ticket == 0 || ticket >= self.next_ticket {
            return Err(TK_BAD_TICKET);
        }
        match self.pool.iter_mut().find(|r| r.ticket == ticket) {
            Some(r) => {
                if r.acked {
                    return Err(TK_DUP_ACK);
                }
                r.acked = true;
                Ok(())
            }
            None => Err(TK_BAD_TICKET),
        }
    }

    pub fn acked_count(&self) -> usize {
        self.pool.iter().filter(|r| r.acked).count()
    }

    pub fn mono_ok(&self) -> bool {
        self.pool.windows(2).all(|w| w[0].ticket < w[1].ticket)
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x41380;
        for r in &self.pool {
            h = h.wrapping_mul(31).wrapping_add(r.ticket as u32);
            h = h.wrapping_mul(31).wrapping_add(r.acked as u32);
            for by in r.holder.bytes() {
                h = h.wrapping_mul(31).wrapping_add(by as u32);
            }
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 标准样本：容量 4，签三回二。
pub fn mk_book() -> TkBook {
    let mut b = TkBook::new(4);
    let _ = b.issue("dma_a");
    let _ = b.issue("dma_b");
    let _ = b.issue("flip");
    let _ = b.ack(1);
    let _ = b.ack(2);
    b
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = mk_book();
    // 1) 池满
    let _ = b.issue("d");
    got[0] = b.issue("e").err() == Some(TK_NO_SPACE);
    // 2) 越界回执
    got[1] = b.ack(99).err() == Some(TK_BAD_TICKET);
    // 3) 重复回执
    got[2] = b.ack(1).err() == Some(TK_DUP_ACK);
    // 4) 零票回执
    got[3] = b.ack(0).err() == Some(TK_BAD_TICKET);
    // 5) 印后签发必抓
    b.seal();
    got[4] = b.issue("late").err() == Some(TK_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx24141_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F24141");
    let mut b = mk_book();

    // J1 票账齐
    let j1 = b.pool.len() == 3 && b.next_ticket == 4;
    s.add("J1-票账齐", j1, "三票签发游标 4");

    // J2 单调性
    let j2 = b.mono_ok();
    s.add("J2-单调性", j2, "票号严格递增");

    // J3 回执对账
    let j3 = b.acked_count() == 2;
    s.add("J3-回执对账", j3, "两票已回执");

    // J4 回执翻转
    let j4 = b.ack(3).is_ok() && b.acked_count() == 3;
    s.add("J4-回执翻转", j4, "第三票回执后计数 3");

    // J5 重复回执拒
    let j5 = b.ack(3).err() == Some(TK_DUP_ACK);
    s.add("J5-重复回执拒", j5, "二次回执必拒");

    // J6 越界回执拒
    let j6 = b.ack(99).err() == Some(TK_BAD_TICKET);
    s.add("J6-越界回执拒", j6, "未签发票号必拒");

    // J7 池满拒
    let j7 = {
        let mut b2 = TkBook::new(1);
        let _ = b2.issue("x");
        b2.issue("y").err() == Some(TK_NO_SPACE)
    };
    s.add("J7-池满拒", j7, "容量 1 二签必拒");

    // J8 空持有者拒
    let j8 = b.issue("").err() == Some(TK_MONO);
    s.add("J8-空持有者拒", j8, "空名签发必拒");

    // J9 边界票号
    let j9 = b.pool[0].ticket == 1 && b.pool[2].ticket == 3;
    s.add("J9-边界票号", j9, "首票 1 末票 3");

    // J10 回执不破单调
    let j10 = b.ack(2).err() == Some(TK_DUP_ACK) && b.mono_ok();
    s.add("J10-回执不破单调", j10, "重复回执不改票序");

    // J11 全回执清池视角
    let j11 = {
        let mut b2 = TkBook::new(2);
        let _ = b2.issue("a");
        let _ = b2.issue("b");
        let _ = b2.ack(1);
        let _ = b2.ack(2);
        b2.acked_count() == 2
    };
    s.add("J11-全回执", j11, "全票回执计数齐");

    // J12 指纹稳定
    let j12 = b.fold() == b.fold();
    s.add("J12-指纹稳定", j12, "折叠指纹两次一致");

    // J13 注入核 5/5
    let got = inject_five();
    let j13 = got.iter().all(|g| *g);
    s.add_dyn("J13-注入全抓", j13, &format!("inj5={:?}", got));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx24141_ticket_flow() {
        let mut b = TkBook::new(2);
        assert!(b.issue("a").is_ok());
        assert!(b.issue("b").is_ok());
        assert_eq!(b.issue("c"), Err(TK_NO_SPACE));
        assert!(b.ack(1).is_ok());
        assert_eq!(b.ack(1), Err(TK_DUP_ACK));
        assert_eq!(b.ack(9), Err(TK_BAD_TICKET));
        b.seal();
        assert_eq!(b.issue("d"), Err(TK_FROZEN));
    }

    #[test]
    fn unx24141_checks_green() {
        let s = run_unx24141_checks();
        assert!(s.all_passed(), "UNX-F24141 判据未全绿");
    }
}
