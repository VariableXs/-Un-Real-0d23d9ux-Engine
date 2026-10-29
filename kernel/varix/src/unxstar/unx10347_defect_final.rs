//! UNX-F10347 缺陷回流终态核（工单闭环率核）（0x1b5e0 段）
//!
//! 锚点：UNX-F10347-J1 工单闭环率——回流工单按 新立/分诊/修复/验证/
//! 闭单 五段状态机逐段推进（越段即拒、到顶再推即拒），全部工单闭单
//! 后闭环率千分比应为 1000，工单数达锚且率满值才算终态核过。
//! 域：CoRun·UNX-C3（联签/闭账/审计域）。
//!
//! 结构：工单账（单=号+段位）+ 推进核（逐段推进、到顶即拒）+ 率核
//! （闭单率千分比出数）+ 注入核（零号/重单/未知单/越顶四路必抓）。

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b5e0 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b5e0;
pub const DF_ZERO: u32 = SEG_BASE + 0x01; // 单号零
pub const DF_DUP: u32 = SEG_BASE + 0x02; // 单号重号
pub const DF_NOTID: u32 = SEG_BASE + 0x03; // 未知工单
pub const DF_TOP: u32 = SEG_BASE + 0x04; // 已到顶再推
pub const DF_CAP: u32 = SEG_BASE + 0x05; // 账超容

/// 工单五段。
pub const TG_NEW: u8 = 1;
pub const TG_TRIAGE: u8 = 2;
pub const TG_FIX: u8 = 3;
pub const TG_VERIFY: u8 = 4;
pub const TG_CLOSED: u8 = 5;
pub const TG_TOP: u8 = 5;

/// 工单数锚（六单）。
pub const TICKETS_NEEDED: usize = 6;
/// 账定容。
pub const CAP: usize = 12;

/// 工单。
#[derive(Clone)]
pub struct Ticket {
    pub tid: u32,
    pub stage: u8,
}

impl Ticket {
    /// 闭单。
    pub fn closed(&self) -> bool {
        self.stage == TG_CLOSED
    }
}

/// 工单账。
pub struct TicketBook {
    pub tickets: Vec<Ticket>,
    pub sealed: bool,
}

impl TicketBook {
    pub fn new() -> TicketBook {
        TicketBook { tickets: Vec::new(), sealed: false }
    }

    fn pos(&self, tid: u32) -> Option<usize> {
        let mut k = 0usize;
        while k < self.tickets.len() {
            if self.tickets[k].tid == tid {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// 新立工单（新立段）。
    pub fn open(&mut self, tid: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(DF_TOP);
        }
        if tid == 0 {
            return Err(DF_ZERO);
        }
        if self.pos(tid).is_some() {
            return Err(DF_DUP);
        }
        if self.tickets.len() >= CAP {
            return Err(DF_CAP);
        }
        self.tickets.push(Ticket { tid, stage: TG_NEW });
        Ok(())
    }

    /// 逐段推进。
    pub fn advance(&mut self, tid: u32) -> Result<u8, u32> {
        if self.sealed {
            return Err(DF_TOP);
        }
        match self.pos(tid) {
            None => Err(DF_NOTID),
            Some(p) => {
                if self.tickets[p].stage >= TG_TOP {
                    return Err(DF_TOP);
                }
                self.tickets[p].stage += 1;
                Ok(self.tickets[p].stage)
            }
        }
    }

    /// 闭单数。
    pub fn closed_count(&self) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < self.tickets.len() {
            if self.tickets[k].closed() {
                n += 1;
            }
            k += 1;
        }
        n
    }

    /// 段位分布（1..=5 各段单数）。
    pub fn stage_counts(&self) -> [usize; 5] {
        let mut c = [0usize; 5];
        let mut k = 0usize;
        while k < self.tickets.len() {
            let st = self.tickets[k].stage as usize;
            if st >= 1 && st <= 5 {
                c[st - 1] += 1;
            }
            k += 1;
        }
        c
    }

    /// 闭环率（千分比）。
    pub fn rate_permille(&self) -> u32 {
        if self.tickets.is_empty() {
            return 0;
        }
        self.closed_count() as u32 * 1000 / self.tickets.len() as u32
    }

    /// 终态核过：单数达锚且全闭单（率 1000）。
    pub fn final_ok(&self) -> bool {
        self.tickets.len() >= TICKETS_NEEDED
            && self.closed_count() == self.tickets.len()
            && self.rate_permille() == 1000
    }

    pub fn seal(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(DF_TOP);
        }
        if !self.final_ok() {
            return Err(DF_NOTID);
        }
        self.sealed = true;
        Ok(())
    }

    /// 工单账指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1b5e00;
        h = h.wrapping_mul(31).wrapping_add(self.tickets.len() as u32);
        let mut k = 0usize;
        while k < self.tickets.len() {
            h = h
                .wrapping_mul(31)
                .wrapping_add(self.tickets[k].tid)
                .wrapping_mul(31)
                .wrapping_add(self.tickets[k].stage as u32);
            k += 1;
        }
        h = h.wrapping_mul(31).wrapping_add(self.sealed as u32);
        h
    }
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    let mut b = TicketBook::new();
    // 1) 零号必拒
    got[0] = b.open(0).err() == Some(DF_ZERO);
    // 2) 重单必拒
    let _ = b.open(1);
    got[1] = b.open(1).err() == Some(DF_DUP);
    // 3) 未知单推进必拒
    got[2] = b.advance(99).err() == Some(DF_NOTID);
    // 4) 越顶推进必拒
    let _ = b.advance(1);
    let _ = b.advance(1);
    let _ = b.advance(1);
    let _ = b.advance(1);
    got[3] = b.advance(1).err() == Some(DF_TOP) && b.tickets[0].closed();
    got
}

/// 批内判据。
pub fn run_unx10347_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10347");

    // J1 正路径：六工单逐段推进全闭单并封账
    let mut b = TicketBook::new();
    let mut ok = true;
    let mut n = 1u32;
    while n <= TICKETS_NEEDED as u32 {
        if b.open(n).is_err() {
            ok = false;
        }
        n += 1;
    }
    n = 1;
    while n <= TICKETS_NEEDED as u32 {
        let mut st = 0u8;
        while st < TG_TOP {
            match b.advance(n) {
                Ok(v) => st = v,
                Err(_) => {
                    ok = false;
                    st = TG_TOP;
                }
            }
        }
        n += 1;
    }
    let sealed = b.seal();
    s.add(
        "J1-工单闭环率",
        ok && b.final_ok() && sealed.is_ok() && b.sealed,
        "六工单逐段推进全闭单且率满千分比",
    );

    // J2 闭环率恰为千分之千
    s.add(
        "J2-率满值",
        b.rate_permille() == 1000,
        "闭单率千分比恰为一千",
    );

    // J3 注入检出：零号 + 重单
    let inj = inject_four();
    s.add("J3-零号重单检出", inj[0] && inj[1], "零单号与重单必拒");

    // J4 注入检出：未知单 + 越顶
    s.add("J4-未知越顶检出", inj[2] && inj[3], "未知单推进与越顶推进必拒");

    // J5 半程率核（三闭三新=500‰）
    let j5 = {
        let mut t = TicketBook::new();
        let mut q = 1u32;
        while q <= 6u32 {
            let _ = t.open(q);
            q += 1;
        }
        q = 1;
        while q <= 3u32 {
            let _ = t.advance(q);
            let _ = t.advance(q);
            let _ = t.advance(q);
            let _ = t.advance(q);
            q += 1;
        }
        t.rate_permille() == 500 && !t.final_ok()
    };
    s.add("J5-半程率核", j5, "半数闭单率恰五百且不算终态核过");

    // J6 段位分布对平（全闭单时闭单段六单）
    let sc = b.stage_counts();
    let j6 = sc[4] == TICKETS_NEEDED
        && sc[0] == 0
        && sc[1] == 0
        && sc[2] == 0
        && sc[3] == 0;
    s.add("J6-段位分布", j6, "全闭单时全部落闭单段");

    // J7 逐段值核（推进返回段位递增）
    let j7 = {
        let mut t = TicketBook::new();
        let _ = t.open(1);
        let a = t.advance(1);
        let c = t.advance(1);
        match (a, c) {
            (Ok(x), Ok(y)) => x == TG_TRIAGE && y == TG_FIX,
            _ => false,
        }
    };
    s.add("J7-段位递增", j7, "推进返回段位严格递增");

    // J8 封账后推进必拒
    let j8 = {
        let mut t = TicketBook::new();
        let mut q = 1u32;
        while q <= TICKETS_NEEDED as u32 {
            let _ = t.open(q);
            q += 1;
        }
        q = 1;
        while q <= TICKETS_NEEDED as u32 {
            let _ = t.advance(q);
            let _ = t.advance(q);
            let _ = t.advance(q);
            let _ = t.advance(q);
            q += 1;
        }
        let _ = t.seal();
        t.advance(1).err() == Some(DF_TOP) && t.open(9).err() == Some(DF_TOP)
    };
    s.add("J8-封账拒动", j8, "封账后推进与开单均拒");

    // J9 容量边界
    let j9 = {
        let mut t = TicketBook::new();
        let mut n = 1u32;
        let mut filled = true;
        while n <= CAP as u32 {
            if t.open(n).is_err() {
                filled = false;
            }
            n += 1;
        }
        filled && t.open(999).err() == Some(DF_CAP)
    };
    s.add("J9-容量边界", j9, "超容必拒且原账不损");

    // J10 指纹随推进且封账后稳定
    let j10 = {
        let mut t = TicketBook::new();
        let _ = t.open(1);
        let f0 = t.fold();
        let _ = t.advance(1);
        let f1 = t.fold();
        f0 != f1 && t.fold() == f1
    };
    s.add("J10-指纹推进", j10, "每推一段指纹一变且终态稳定");

    // J11 空账率零
    let j11 = TicketBook::new().rate_permille() == 0;
    s.add("J11-空账率零", j11, "无单时率出零");

    // J12 动态明细：闭单计数落账
    let cc = b.closed_count();
    s.add_dyn(
        "J12-闭单计数",
        cc == TICKETS_NEEDED,
        &format!("closed={}/{}", cc, b.tickets.len()),
    );

    // J13 动态明细：工单账指纹落账
    let fp = b.fold();
    s.add_dyn("J13-工单指纹", fp != 0, &format!("fold={:#x}", fp));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10347_defect_final_flow() {
        let mut b = TicketBook::new();
        assert!(b.open(0).err() == Some(DF_ZERO));
        assert!(b.open(1).is_ok());
        assert!(b.open(1).err() == Some(DF_DUP));
        assert!(b.advance(99).err() == Some(DF_NOTID));
        assert!(b.advance(1) == Ok(TG_TRIAGE));
        assert!(b.advance(1) == Ok(TG_FIX));
        assert!(b.advance(1) == Ok(TG_VERIFY));
        assert!(b.advance(1) == Ok(TG_CLOSED));
        assert!(b.advance(1).err() == Some(DF_TOP));
        assert!(!b.final_ok());
        let mut q = 2u32;
        while q <= TICKETS_NEEDED as u32 {
            let _ = b.open(q);
            let _ = b.advance(q);
            let _ = b.advance(q);
            let _ = b.advance(q);
            let _ = b.advance(q);
            q += 1;
        }
        assert!(b.final_ok());
        assert!(b.rate_permille() == 1000);
        assert!(b.seal().is_ok());
        assert!(b.advance(2).err() == Some(DF_TOP));
    }

    #[test]
    fn unx10347_checks_green() {
        let s = run_unx10347_checks();
        assert!(s.all_passed(), "UNX-F10347 判据未全绿");
    }
}
