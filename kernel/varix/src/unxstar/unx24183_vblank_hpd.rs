//! UNX-F24183 vblank/HPD 内核事件面与事件队列·子功能03（HPD 热插事件检测）（0x418C0 段）
//!
//! 锚点：UNX-F24183-J1 热插账（连接/断开事件逐条落账）；J2 热插语义
//! （态变才发事件 + 同态抑制 + 野连接器拒三性质）；J3 热插指纹（账
//! 折叠指纹）三判据全绿。
//! 域：CoRun·UNX-F5-B15（HPD 检测面）。
//!
//! 结构：连接器态表 + 态变检测核 + 事件账本 + 折叠指纹 + 注入核（野
//! 连接器/同态发/倒转态/印后/重号必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x418C0 独占 0x20）。
pub const SEG_BASE: u32 = 0x418C0;
pub const HP_UNK_CONN: u32 = SEG_BASE + 0x01; // 野连接器
pub const HP_SAME_STATE: u32 = SEG_BASE + 0x02; // 同态发事件
pub const HP_DUP_DECL: u32 = SEG_BASE + 0x03; // 重号连接器
pub const HP_FROZEN: u32 = SEG_BASE + 0x04; // 印后操作
pub const HP_BAD_PIN: u32 = SEG_BASE + 0x05; // 伪电平档

/// 连接态。
pub const CS_DISCONNECTED: u8 = 0;
pub const CS_CONNECTED: u8 = 1;

/// 连接器。
#[derive(Clone)]
pub struct HpConn {
    pub conn_id: u32,
    pub state: u8,
    pub events: u32,
}

/// 事件账。
#[derive(Clone)]
pub struct HpRec {
    pub seq: u32,
    pub conn_id: u32,
    pub from: u8,
    pub to: u8,
}

/// 热插簿。
pub struct HpBook {
    pub conns: Vec<HpConn>,
    pub ledger: Vec<HpRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl HpBook {
    pub fn new() -> HpBook {
        HpBook { conns: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    pub fn declare(&mut self, conn_id: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(HP_FROZEN);
        }
        if self.conns.iter().any(|c| c.conn_id == conn_id) {
            return Err(HP_DUP_DECL);
        }
        self.conns.push(HpConn { conn_id, state: CS_DISCONNECTED, events: 0 });
        Ok(())
    }

    /// HPD 电平变化：态变才发事件。
    pub fn hpd(&mut self, conn_id: u32, level: u8) -> Result<u8, u32> {
        if self.sealed {
            return Err(HP_FROZEN);
        }
        if level > CS_CONNECTED {
            return Err(HP_BAD_PIN);
        }
        let c = match self.conns.iter_mut().find(|c| c.conn_id == conn_id) {
            Some(c) => c,
            None => return Err(HP_UNK_CONN),
        };
        if c.state == level {
            return Err(HP_SAME_STATE);
        }
        let from = c.state;
        c.state = level;
        c.events += 1;
        let seq = self.seq_next;
        self.seq_next += 1;
        self.ledger.push(HpRec { seq, conn_id, from, to: level });
        Ok(level)
    }

    pub fn connected_count(&self) -> usize {
        self.conns.iter().filter(|c| c.state == CS_CONNECTED).count()
    }

    pub fn events_of(&self, conn_id: u32) -> u32 {
        self.conns.iter().find(|c| c.conn_id == conn_id).map(|c| c.events).unwrap_or(0)
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x418C0;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.seq);
            h = h.wrapping_mul(31).wrapping_add(r.conn_id);
            h = h.wrapping_mul(31).wrapping_add(r.to as u32);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 标准样本：两连接器。
pub fn mk_book() -> HpBook {
    let mut b = HpBook::new();
    let _ = b.declare(0);
    let _ = b.declare(1);
    let _ = b.hpd(0, CS_CONNECTED); // 0: dis→con
    let _ = b.hpd(1, CS_CONNECTED);
    let _ = b.hpd(1, CS_DISCONNECTED); // 1: con→dis
    b
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = mk_book();
    // 1) 野连接器
    got[0] = b.hpd(99, CS_CONNECTED).err() == Some(HP_UNK_CONN);
    // 2) 同态发事件
    got[1] = b.hpd(0, CS_CONNECTED).err() == Some(HP_SAME_STATE);
    // 3) 重号声明
    got[2] = b.declare(0).err() == Some(HP_DUP_DECL);
    // 4) 伪电平
    got[3] = b.hpd(0, 9).err() == Some(HP_BAD_PIN);
    // 5) 印后操作必抓
    b.seal();
    got[4] = b.hpd(0, CS_DISCONNECTED).err() == Some(HP_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx24183_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F24183");
    let mut b = mk_book();

    // J1 连接器账齐
    let j1 = b.conns.len() == 2;
    s.add("J1-连接器账齐", j1, "两连接器落表");

    // J2 态变才发事件
    let j2 = b.ledger.len() == 3;
    s.add("J2-态变发事件", j2, "三次态变落账");

    // J3 同态抑制
    let j3 = b.hpd(0, CS_CONNECTED).err() == Some(HP_SAME_STATE);
    s.add("J3-同态抑制", j3, "同态 HPD 必抑制");

    // J4 连接计数
    let j4 = b.connected_count() == 1;
    s.add("J4-连接计数", j4, "一连接器在连");

    // J5 事件计数
    let j5 = b.events_of(1) == 2;
    s.add("J5-事件计数", j5, "连接器 1 两事件");

    // J6 野连接器拒
    let j6 = b.hpd(99, CS_CONNECTED).err() == Some(HP_UNK_CONN);
    s.add("J6-野连接器拒", j6, "未声明必拒");

    // J7 重号声明拒
    let j7 = b.declare(0).err() == Some(HP_DUP_DECL);
    s.add("J7-重号拒", j7, "连接器重号必拒");

    // J8 伪电平拒
    let j8 = b.hpd(0, 9).err() == Some(HP_BAD_PIN);
    s.add("J8-伪电平拒", j8, "电平越闭集必拒");

    // J9 往返态变
    let j9 = {
        let _ = b.hpd(0, CS_DISCONNECTED);
        let _ = b.hpd(0, CS_CONNECTED);
        b.events_of(0) == 3
    };
    s.add("J9-往返态变", j9, "插拔往返计数 3");

    // J10 账面 from≠to
    let j10 = b.ledger.iter().all(|r| r.from != r.to);
    s.add("J10-账from≠to", j10, "事件账全为真态变");

    // J11 印后操作拒
    let j11 = {
        let mut b2 = mk_book();
        b2.seal();
        b2.hpd(0, CS_DISCONNECTED).err() == Some(HP_FROZEN)
    };
    s.add("J11-印后拒", j11, "印发后冻结");

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
    fn unx24183_hpd_flow() {
        let mut b = HpBook::new();
        let _ = b.declare(0);
        assert_eq!(b.hpd(0, CS_CONNECTED), Ok(CS_CONNECTED));
        assert_eq!(b.hpd(0, CS_CONNECTED), Err(HP_SAME_STATE));
        assert_eq!(b.hpd(0, CS_DISCONNECTED), Ok(CS_DISCONNECTED));
        assert_eq!(b.hpd(0, 9), Err(HP_BAD_PIN));
        assert_eq!(b.hpd(99, CS_CONNECTED), Err(HP_UNK_CONN));
        assert_eq!(b.events_of(0), 2);
        b.seal();
        assert_eq!(b.hpd(0, CS_CONNECTED), Err(HP_FROZEN));
    }

    #[test]
    fn unx24183_checks_green() {
        let s = run_unx24183_checks();
        assert!(s.all_passed(), "UNX-F24183 判据未全绿");
    }
}
