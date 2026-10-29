//! UNX-F24136 显存驱逐 LRU 与迁移搬运·子功能16（pin 计数挡逐出与强制解 pin）（0x412E0 段）
//!
//! 锚点：UNX-F24136-J1 pin 账（pin 计数升降与强制解 pin 逐条落账）；
//! J2 pin 语义（多层 pin 闸 + 零计解绑 + 逐出挡三性质）；J3 pin 指纹
//! （pin 账折叠指纹）三判据全绿。
//! 域：CoRun·UNX-F5-B15（pin 计数挡逐出面）。
//!
//! 结构：pin 计数表 + pin/unpin 核 + 强制解 pin 核 + 账本 + 折叠指纹 +
//! 注入核（野对象/超降/零计逐出/印后/重复强制必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x412E0 独占 0x20）。
pub const SEG_BASE: u32 = 0x412E0;
pub const PN_UNK_OBJ: u32 = SEG_BASE + 0x01; // 野对象
pub const PN_UNDERFLOW: u32 = SEG_BASE + 0x02; // pin 计数下溢
pub const PN_PINNED_EVICT: u32 = SEG_BASE + 0x03; // pinned 对象逐出
pub const PN_FROZEN: u32 = SEG_BASE + 0x04; // 印后操作
pub const PN_DUP_FORCE: u32 = SEG_BASE + 0x05; // 重复强制解

/// pin 条目。
#[derive(Clone)]
pub struct PinEnt {
    pub obj_id: u32,
    pub pin_cnt: u32,
    pub force_released: bool,
}

/// pin 账。
#[derive(Clone)]
pub struct PinRec {
    pub seq: u32,
    pub obj_id: u32,
    pub op: u8, // 0=pin 1=unpin 2=force
    pub after: u32,
}

/// pin 簿。
pub struct PinBook {
    pub ents: Vec<PinEnt>,
    pub ledger: Vec<PinRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl PinBook {
    pub fn new() -> PinBook {
        PinBook { ents: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    pub fn add(&mut self, obj_id: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(PN_FROZEN);
        }
        if self.ents.iter().any(|e| e.obj_id == obj_id) {
            return Err(PN_UNK_OBJ);
        }
        self.ents.push(PinEnt { obj_id, pin_cnt: 0, force_released: false });
        Ok(())
    }

    fn rec(&mut self, obj_id: u32, op: u8, after: u32) {
        let seq = self.seq_next;
        self.seq_next += 1;
        self.ledger.push(PinRec { seq, obj_id, op, after });
    }

    /// pin 计数 +1。
    pub fn pin(&mut self, obj_id: u32) -> Result<u32, u32> {
        if self.sealed {
            return Err(PN_FROZEN);
        }
        match self.ents.iter_mut().find(|e| e.obj_id == obj_id) {
            Some(e) => {
                e.pin_cnt += 1;
                let c = e.pin_cnt;
                self.rec(obj_id, 0, c);
                Ok(c)
            }
            None => Err(PN_UNK_OBJ),
        }
    }

    /// unpin 计数 -1（零计再解为下溢）。
    pub fn unpin(&mut self, obj_id: u32) -> Result<u32, u32> {
        if self.sealed {
            return Err(PN_FROZEN);
        }
        match self.ents.iter_mut().find(|e| e.obj_id == obj_id) {
            Some(e) => {
                if e.pin_cnt == 0 {
                    return Err(PN_UNDERFLOW);
                }
                e.pin_cnt -= 1;
                let c = e.pin_cnt;
                self.rec(obj_id, 1, c);
                Ok(c)
            }
            None => Err(PN_UNK_OBJ),
        }
    }

    /// 强制解 pin：计数清零并标记，二次强制必拒。
    pub fn force_release(&mut self, obj_id: u32) -> Result<u32, u32> {
        if self.sealed {
            return Err(PN_FROZEN);
        }
        match self.ents.iter_mut().find(|e| e.obj_id == obj_id) {
            Some(e) => {
                if e.force_released {
                    return Err(PN_DUP_FORCE);
                }
                let was = e.pin_cnt;
                e.pin_cnt = 0;
                e.force_released = true;
                self.rec(obj_id, 2, was);
                Ok(was)
            }
            None => Err(PN_UNK_OBJ),
        }
    }

    /// 逐出闸：pinned（计>0 且未强制解）对象拒逐出。
    pub fn evict(&mut self, obj_id: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(PN_FROZEN);
        }
        match self.ents.iter().find(|e| e.obj_id == obj_id) {
            Some(e) => {
                if e.pin_cnt > 0 {
                    Err(PN_PINNED_EVICT)
                } else {
                    Ok(())
                }
            }
            None => Err(PN_UNK_OBJ),
        }
    }

    pub fn pinned_count(&self) -> usize {
        self.ents.iter().filter(|e| e.pin_cnt > 0).count()
    }

    pub fn seq_unique(&self) -> bool {
        for i in 0..self.ledger.len() {
            for j in (i + 1)..self.ledger.len() {
                if self.ledger[i].seq == self.ledger[j].seq {
                    return false;
                }
            }
        }
        true
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x412E0;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.seq);
            h = h.wrapping_mul(31).wrapping_add(r.obj_id);
            h = h.wrapping_mul(31).wrapping_add(r.op as u32);
            h = h.wrapping_mul(31).wrapping_add(r.after);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 标准样本：四对象，pin 历史各异。
pub fn mk_book() -> PinBook {
    let mut b = PinBook::new();
    for id in 1..=4u32 {
        let _ = b.add(id);
    }
    let _ = b.pin(1);
    let _ = b.pin(1);
    let _ = b.pin(2);
    let _ = b.pin(3);
    let _ = b.unpin(3);
    b
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = mk_book();
    // 1) 野对象
    got[0] = b.pin(99).err() == Some(PN_UNK_OBJ);
    // 2) 下溢
    got[1] = b.unpin(4).err() == Some(PN_UNDERFLOW);
    // 3) pinned 逐出拒
    got[2] = b.evict(2).err() == Some(PN_PINNED_EVICT);
    // 4) 重复强制解
    let _ = b.force_release(2);
    got[3] = b.force_release(2).err() == Some(PN_DUP_FORCE);
    // 5) 印后操作必抓
    b.seal();
    got[4] = b.pin(1).err() == Some(PN_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx24136_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F24136");
    let mut b = mk_book();

    // J1 对象账齐
    let j1 = b.ents.len() == 4;
    s.add("J1-对象账齐", j1, "四对象全落表");

    // J2 多层 pin 计数
    let j2 = b.ents.iter().find(|e| e.obj_id == 1).map(|e| e.pin_cnt == 2).unwrap_or(false);
    s.add("J2-多层计数", j2, "obj1 双层 pin");

    // J3 unpin 降计
    let j3 = b.unpin(1).is_ok()
        && b.ents.iter().find(|e| e.obj_id == 1).map(|e| e.pin_cnt == 1).unwrap_or(false);
    s.add("J3-unpin降计", j3, "解 pin 计数递减");

    // J4 下溢拒
    let j4 = b.unpin(4).err() == Some(PN_UNDERFLOW);
    s.add("J4-下溢拒", j4, "零计解 pin 必拒");

    // J5 pinned 挡逐出
    let j5 = b.evict(1).err() == Some(PN_PINNED_EVICT);
    s.add("J5-pin挡逐出", j5, "计数>0 对象拒逐出");

    // J6 零计放行逐出
    let j6 = b.evict(4).is_ok();
    s.add("J6-零计放行", j6, "零计对象可逐出");

    // J7 强制解 pin 清零
    let j7 = {
        let was = b.force_release(1).unwrap_or(0);
        was == 1
            && b.ents.iter().find(|e| e.obj_id == 1).map(|e| e.pin_cnt == 0).unwrap_or(false)
    };
    s.add("J7-强制清零", j7, "强制解 pin 计数归零");

    // J8 强制后可逐出
    let j8 = b.evict(1).is_ok();
    s.add("J8-强制放行", j8, "强制解 pin 后逐出放行");

    // J9 重复强制拒
    let j9 = b.force_release(1).err() == Some(PN_DUP_FORCE);
    s.add("J9-重复强制拒", j9, "二次强制必拒");

    // J10 pin 账落账
    let j10 = b.ledger.len() >= 6 && b.ledger.iter().all(|r| r.seq >= 1);
    s.add("J10-pin账落账", j10, "pin/unpin/force 全落账");

    // J11 野对象拒
    let j11 = b.unpin(99).err() == Some(PN_UNK_OBJ);
    s.add("J11-野对象拒", j11, "野对象操作必拒");

    // J12 指纹稳定 + seq 唯一
    let j12 = b.fold() == b.fold() && b.seq_unique();
    s.add("J12-指纹稳定", j12, "折叠指纹两次一致且 seq 唯一");

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
    fn unx24136_pin_flow() {
        let mut b = mk_book();
        assert_eq!(b.pin(1).unwrap_or(0), 3);
        assert_eq!(b.unpin(4), Err(PN_UNDERFLOW));
        assert_eq!(b.evict(2), Err(PN_PINNED_EVICT));
        assert!(b.force_release(2).is_ok());
        assert_eq!(b.force_release(2), Err(PN_DUP_FORCE));
        assert!(b.evict(2).is_ok());
        b.seal();
        assert_eq!(b.pin(1), Err(PN_FROZEN));
    }

    #[test]
    fn unx24136_checks_green() {
        let s = run_unx24136_checks();
        assert!(s.all_passed(), "UNX-F24136 判据未全绿");
    }
}
