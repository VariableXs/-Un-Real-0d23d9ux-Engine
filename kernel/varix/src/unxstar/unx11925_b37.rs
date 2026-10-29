//! UNX-F11925 E 型八批清账判据（0x42C00 段）
//!
//! 锚点：UNX-F11925-J1 E 型清账面判据绿（证据链签收/八批逐清/账平），
//! 与基准机一致；J2 E 型语义（无证闭批拒 + 证据复用拒 + 印后拒三性
//! 质）；J3 清账指纹（账折叠指纹）三判据全绿。
//! 域：CoRun·UNX-C5-B37（E 型清账面）。
//!
//! 结构：证据签收清账器 + 证据核 + 逐批清核 + 账本 + 折叠指纹 + 注入
//! 核（无证闭批/证据复用/野批/印后/红残留必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use crate::checks::CheckSet;

/// 诊断段（0x42C00 独占 0x20）。
pub const SEG_BASE: u32 = 0x42C00;
pub const EC_NO_EVID: u32 = SEG_BASE + 0x01; // 无证闭批
pub const EC_EVID_DUP: u32 = SEG_BASE + 0x02; // 证据复用
pub const EC_BAD_BATCH: u32 = SEG_BASE + 0x03; // 野批号
pub const EC_FROZEN: u32 = SEG_BASE + 0x04; // 印后操作
pub const EC_RED_LEFT: u32 = SEG_BASE + 0x05; // 红残留

/// 证据票：全局唯一序号。
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct EcTicket {
    pub serial: u64,
    pub batch: u8,
}

/// E 型证据清账器。
pub struct EcLedger {
    reds: [u32; 8],
    closed: [bool; 8],
    used_serials: [bool; 256],
    tickets: Vec<EcTicket>,
    next_serial: u64,
    sealed: bool,
}

impl EcLedger {
    pub const BATCHES: usize = 8;

    pub fn new() -> EcLedger {
        EcLedger {
            reds: [0; 8],
            closed: [false; 8],
            used_serials: [false; 256],
            tickets: Vec::new(),
            next_serial: 1,
            sealed: false,
        }
    }

    /// 签发证据票（串号唯一自增）。
    pub fn issue_ticket(&mut self, batch: u8) -> Result<EcTicket, u32> {
        if self.sealed {
            return Err(EC_FROZEN);
        }
        if batch as usize >= Self::BATCHES {
            return Err(EC_BAD_BATCH);
        }
        let t = EcTicket {
            serial: self.next_serial,
            batch,
        };
        self.next_serial += 1;
        Ok(t)
    }

    pub fn record(&mut self, batch: u8, reds: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(EC_FROZEN);
        }
        if batch as usize >= Self::BATCHES {
            return Err(EC_BAD_BATCH);
        }
        self.reds[batch as usize] = self.reds[batch as usize].saturating_add(reds);
        Ok(())
    }

    /// 凭证清批：先清红，再验票闭批；票串号一次性。
    pub fn close_with(&mut self, t: EcTicket) -> Result<u32, u32> {
        if self.sealed {
            return Err(EC_FROZEN);
        }
        if t.batch as usize >= Self::BATCHES {
            return Err(EC_BAD_BATCH);
        }
        if t.serial == 0 || t.serial >= self.next_serial {
            return Err(EC_NO_EVID);
        }
        if self.used_serials[t.serial as usize] {
            return Err(EC_EVID_DUP);
        }
        if self.closed[t.batch as usize] {
            return Err(EC_EVID_DUP);
        }
        self.used_serials[t.serial as usize] = true;
        let r = self.reds[t.batch as usize];
        if r > 0 {
            return Err(EC_RED_LEFT);
        }
        self.closed[t.batch as usize] = true;
        self.tickets.push(t);
        Ok(r)
    }

    /// 凭证强清闭批。
    pub fn close_force_with(&mut self, t: EcTicket) -> Result<u32, u32> {
        if self.sealed {
            return Err(EC_FROZEN);
        }
        if t.batch as usize >= Self::BATCHES {
            return Err(EC_BAD_BATCH);
        }
        if t.serial == 0 || t.serial >= self.next_serial {
            return Err(EC_NO_EVID);
        }
        if self.used_serials[t.serial as usize] {
            return Err(EC_EVID_DUP);
        }
        if self.closed[t.batch as usize] {
            return Err(EC_EVID_DUP);
        }
        self.used_serials[t.serial as usize] = true;
        let r = self.reds[t.batch as usize];
        self.reds[t.batch as usize] = 0;
        self.closed[t.batch as usize] = true;
        self.tickets.push(t);
        Ok(r)
    }

    pub fn balanced(&self) -> bool {
        self.closed.iter().all(|c| *c) && self.reds.iter().all(|r| *r == 0)
    }

    pub fn red_total(&self) -> u32 {
        self.reds.iter().sum()
    }

    pub fn tickets_used(&self) -> usize {
        self.tickets.len()
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x42C00;
        h = h.wrapping_mul(31).wrapping_add(self.tickets_used() as u32);
        for r in &self.reds {
            h = h.wrapping_mul(31).wrapping_add(*r);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 标准夹具：八批凭证强清，账平。
pub fn mk_ledger() -> EcLedger {
    let mut l = EcLedger::new();
    for b in 0..8u8 {
        let _ = l.record(b, 1);
        let t = l.issue_ticket(b).expect("ticket");
        let _ = l.close_force_with(t);
    }
    l
}

/// 注入核：五个必然被拒的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    // 1) 无证闭批（串号超前）
    let mut l = EcLedger::new();
    got[0] = l
        .close_with(EcTicket { serial: 99, batch: 0 })
        .err()
        == Some(EC_NO_EVID);
    // 2) 证据复用
    let mut l2 = EcLedger::new();
    let t = l2.issue_ticket(0).expect("t");
    let _ = l2.close_force_with(t);
    got[1] = l2.close_force_with(t).err() == Some(EC_EVID_DUP);
    // 3) 野批号
    got[2] = EcLedger::new().issue_ticket(9).err() == Some(EC_BAD_BATCH);
    // 4) 红残留凭证闭批
    let mut l3 = EcLedger::new();
    let _ = l3.record(1, 5);
    let t3 = l3.issue_ticket(1).expect("t3");
    got[3] = l3.close_with(t3).err() == Some(EC_RED_LEFT);
    // 5) 印后操作
    let mut l4 = mk_ledger();
    l4.seal();
    let t4 = EcTicket { serial: 250, batch: 0 };
    got[4] = l4.issue_ticket(0).err() == Some(EC_FROZEN)
        && l4.close_force_with(t4).err() == Some(EC_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx11925_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11925");
    let mut l = mk_ledger();

    // J1 八批全闭且账平
    let j1 = l.balanced();
    s.add("J1-账平", j1, "八批凭证闭清");

    // J2 证据票用量
    let j2 = l.tickets_used() == 8;
    s.add("J2-票用量", j2, "8 票一次");

    // J3 红零
    let j3 = l.red_total() == 0;
    s.add("J3-红零", j3, "红目 0");

    // J4 无证闭批拒
    let j4 = {
        let mut l2 = EcLedger::new();
        l2.close_with(EcTicket { serial: 5, batch: 0 }).err() == Some(EC_NO_EVID)
    };
    s.add("J4-无证拒", j4, "串号超前必拒");

    // J5 零串号拒
    let j5 = {
        let mut l2 = EcLedger::new();
        l2.close_force_with(EcTicket { serial: 0, batch: 0 }).err() == Some(EC_NO_EVID)
    };
    s.add("J5-零串拒", j5, "serial=0 必拒");

    // J6 证据复用拒
    let j6 = {
        let mut l2 = EcLedger::new();
        let t = l2.issue_ticket(2).expect("t");
        let _ = l2.close_force_with(t);
        l2.close_force_with(t).err() == Some(EC_EVID_DUP)
    };
    s.add("J6-复用拒", j6, "同串二次必拒");

    // J7 野批号拒
    let j7 = l.issue_ticket(8).err() == Some(EC_BAD_BATCH)
        && l.record(9, 1).err() == Some(EC_BAD_BATCH);
    s.add("J7-野批拒", j7, "批号≥8 必拒");

    // J8 红残留闭批拒
    let j8 = {
        let mut l2 = EcLedger::new();
        let _ = l2.record(0, 3);
        let t = l2.issue_ticket(0).expect("t");
        l2.close_with(t).err() == Some(EC_RED_LEFT)
    };
    s.add("J8-红残留拒", j8, "红未清闭批必拒");

    // J9 串号自增
    let j9 = {
        let mut l2 = EcLedger::new();
        let a = l2.issue_ticket(0).expect("a").serial;
        let b = l2.issue_ticket(1).expect("b").serial;
        a == 1 && b == 2
    };
    s.add("J9-串号自增", j9, "1,2 递增");

    // J10 票批一致性：票记批即闭批
    let j10 = {
        let mut l2 = EcLedger::new();
        let t = l2.issue_ticket(4).expect("t");
        let _ = l2.close_force_with(t);
        l2.tickets.last().map(|x| x.batch) == Some(4)
    };
    s.add("J10-票批一致", j10, "账留票批");

    // J11 印后操作拒
    let j11 = {
        let mut l2 = mk_ledger();
        l2.seal();
        l2.record(0, 1).err() == Some(EC_FROZEN)
    };
    s.add("J11-印后拒", j11, "印发后冻结");

    // J12 指纹稳定
    let j12 = {
        let mut l2 = mk_ledger();
        let a = l2.fold();
        l2.seal();
        l2.fold() == a
    };
    s.add("J12-指纹稳定", j12, "印不改指纹");

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
    fn unx11925_evid_flow() {
        let mut l = EcLedger::new();
        assert_eq!(l.issue_ticket(8), Err(EC_BAD_BATCH));
        let t = l.issue_ticket(0).expect("t");
        assert_eq!(t.serial, 1);
        assert_eq!(l.record(0, 3), Ok(()));
        assert_eq!(l.close_with(t), Err(EC_RED_LEFT));
        assert_eq!(l.close_force_with(t), Ok(3));
        assert_eq!(l.close_force_with(t), Err(EC_EVID_DUP));
        assert_eq!(l.record(9, 1), Err(EC_BAD_BATCH));
        assert!(!l.balanced());
        l.seal();
        assert_eq!(l.issue_ticket(0), Err(EC_FROZEN));
    }

    #[test]
    fn unx11925_checks_green() {
        let s = run_unx11925_checks();
        assert!(s.all_passed(), "UNX-F11925 判据未全绿");
    }
}
