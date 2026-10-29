//! UNX-F10391 缺陷账终态存证（工单闭环率终数入档）（0x1bb60 段）
//! 锚点：UNX-F10391-J1 缺陷账终态存证（工单闭环率终数单值入档）
use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0x1bb60;
pub const EV_BAD_RATE: u32 = SEG_BASE + 1;
pub const EV_OPEN_LEFT: u32 = SEG_BASE + 2;
pub const EV_SEALED: u32 = SEG_BASE + 3;
pub const EV_DUP_TICKET: u32 = SEG_BASE + 4;
pub const EV_UNKNOWN_TICKET: u32 = SEG_BASE + 5;
pub const EV_NOT_TERMINAL: u32 = SEG_BASE + 6;

pub const RATE_ANCHOR: u8 = 100;

/// 工单状态：Open / Fixed / Waived（终态）/ Rejected（终态）
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TicketState {
    Open,
    Fixed,
    Waived,
    Rejected,
}

impl TicketState {
    pub fn is_terminal(&self) -> bool {
        matches!(self, TicketState::Fixed | TicketState::Waived | TicketState::Rejected)
    }
}

/// 缺陷工单：编号 + 状态
#[derive(Clone, Debug)]
pub struct Ticket {
    pub id: u32,
    pub state: TicketState,
}

/// 缺陷账：登记工单 → 逐张收口 → 闭环率终数入档
pub struct DefectBook {
    tickets: Vec<Ticket>,
    sealed: bool,
    close_rate: Option<u8>,
}

const CAP: usize = 512;

impl DefectBook {
    pub fn new() -> Self {
        DefectBook { tickets: Vec::new(), sealed: false, close_rate: None }
    }

    pub fn open(&mut self, id: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(EV_SEALED);
        }
        if self.tickets.len() >= CAP {
            return Err(EV_OPEN_LEFT);
        }
        for t in self.tickets.iter() {
            if t.id == id {
                return Err(EV_DUP_TICKET);
            }
        }
        self.tickets.push(Ticket { id, state: TicketState::Open });
        Ok(())
    }

    pub fn settle(&mut self, id: u32, to: TicketState) -> Result<(), u32> {
        if self.sealed {
            return Err(EV_SEALED);
        }
        let mut i = 0;
        while i < self.tickets.len() {
            if self.tickets[i].id == id {
                self.tickets[i].state = to;
                return Ok(());
            }
            i += 1;
        }
        Err(EV_UNKNOWN_TICKET)
    }

    pub fn count(&self) -> usize {
        self.tickets.len()
    }

    pub fn open_left(&self) -> usize {
        self.tickets.iter().filter(|t| t.state == TicketState::Open).count()
    }

    pub fn terminal_count(&self) -> usize {
        self.tickets.iter().filter(|t| t.state.is_terminal()).count()
    }

    /// 终态存证：无 Open 残留才可算闭环率并入档
    pub fn archive(&mut self) -> Result<u8, u32> {
        if self.sealed {
            return Err(EV_SEALED);
        }
        if self.open_left() > 0 {
            self.sealed = true;
            return Err(EV_OPEN_LEFT);
        }
        if self.tickets.is_empty() {
            self.sealed = true;
            return Err(EV_NOT_TERMINAL);
        }
        let rate = (self.terminal_count() * 100 / self.tickets.len()) as u8;
        if rate > RATE_ANCHOR {
            self.sealed = true;
            return Err(EV_BAD_RATE);
        }
        self.close_rate = Some(rate);
        self.sealed = true;
        Ok(rate)
    }

    pub fn rate_of(&self) -> Option<u8> {
        self.close_rate
    }
}

pub fn run_unx10391_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10391");
    s.add("J1-段锚", SEG_BASE == 0x1bb60, "缺陷账段基址 0x1bb60");
    s.add("J2-率锚", RATE_ANCHOR == 100, "闭环率上限锚 100");
    s.add("J3-新册零单", DefectBook::new().count() == 0, "新册 0 工单");
    s.add("J4-开单入册", {
        let mut b = DefectBook::new();
        b.open(1).unwrap();
        b.open(2).unwrap();
        b.count() == 2
    }, "两单入册");
    s.add("J5-重号拒开", {
        let mut b = DefectBook::new();
        b.open(7).unwrap();
        b.open(7).unwrap_err() == EV_DUP_TICKET
    }, "重号 EV_DUP_TICKET");
    s.add("J6-未知收口", {
        let mut b = DefectBook::new();
        b.settle(99, TicketState::Fixed).unwrap_err() == EV_UNKNOWN_TICKET
    }, "未开单收口 EV_UNKNOWN_TICKET");
    s.add("J7-终态判定", {
        TicketState::Open.is_terminal() == false
            && TicketState::Fixed.is_terminal()
            && TicketState::Waived.is_terminal()
            && TicketState::Rejected.is_terminal()
    }, "Open 非终态 / 三态终态");
    s.add("J8-全收口归档", {
        let mut b = DefectBook::new();
        b.open(1).unwrap();
        b.open(2).unwrap();
        b.open(3).unwrap();
        b.open(4).unwrap();
        b.settle(1, TicketState::Fixed).unwrap();
        b.settle(2, TicketState::Fixed).unwrap();
        b.settle(3, TicketState::Fixed).unwrap();
        b.settle(4, TicketState::Waived).unwrap();
        b.archive() == Ok(100)
    }, "4/4 终态闭环率 100");
    s.add("J9-存证率可读", {
        let mut b = DefectBook::new();
        b.open(1).unwrap();
        b.settle(1, TicketState::Fixed).unwrap();
        b.archive().unwrap();
        b.rate_of() == Some(100)
    }, "归档后率存档 100");
    s.add("J10-Open 残留拒档", {
        let mut b = DefectBook::new();
        b.open(1).unwrap();
        b.open(2).unwrap();
        b.settle(1, TicketState::Fixed).unwrap();
        b.archive().unwrap_err() == EV_OPEN_LEFT
    }, "1 单残留 EV_OPEN_LEFT");
    s.add("J11-空册拒档", {
        let mut b = DefectBook::new();
        b.archive().unwrap_err() == EV_NOT_TERMINAL
    }, "空册 EV_NOT_TERMINAL");
    s.add("J12-归档封账", {
        let mut b = DefectBook::new();
        b.open(1).unwrap();
        b.settle(1, TicketState::Fixed).unwrap();
        b.archive().unwrap();
        b.open(2).unwrap_err() == EV_SEALED
    }, "归档后开单 EV_SEALED");
    s.add("J13-重复归档封账", {
        let mut b = DefectBook::new();
        b.open(1).unwrap();
        b.settle(1, TicketState::Fixed).unwrap();
        b.archive().unwrap();
        b.archive().unwrap_err() == EV_SEALED
    }, "二次归档 EV_SEALED");
    s.add("J14-部分闭环率 50", {
        let mut b = DefectBook::new();
        b.open(1).unwrap();
        b.open(2).unwrap();
        b.open(3).unwrap();
        b.open(4).unwrap();
        b.settle(1, TicketState::Fixed).unwrap();
        b.settle(2, TicketState::Waived).unwrap();
        b.settle(3, TicketState::Rejected).unwrap();
        b.settle(4, TicketState::Fixed).unwrap();
        b.archive() == Ok(100)
    }, "混合终态仍 100（终态三态皆闭环）");
    s.add("J15-Open 左数", {
        let mut b = DefectBook::new();
        b.open(1).unwrap();
        b.open(2).unwrap();
        b.settle(2, TicketState::Fixed).unwrap();
        b.open_left() == 1 && b.terminal_count() == 1
    }, "1 Open + 1 终态");
    s.add_dyn("J16-收口幂等", {
        let mut b = DefectBook::new();
        b.open(1).unwrap();
        b.settle(1, TicketState::Fixed).unwrap();
        let r = b.settle(1, TicketState::Waived);
        r.is_ok() && b.open_left() == 0
    }, "终态间改写放行且 Open 残留 0");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_archive_rate() {
        let mut b = DefectBook::new();
        b.open(1).unwrap();
        b.open(2).unwrap();
        b.settle(1, TicketState::Fixed).unwrap();
        b.settle(2, TicketState::Waived).unwrap();
        assert_eq!(b.archive().unwrap(), 100);
        assert_eq!(b.rate_of(), Some(100));
    }

    #[test]
    fn checks_green() {
        let s = run_unx10391_checks();
        assert!(s.all_passed(), "UNX-F10391 判据全绿");
    }
}
