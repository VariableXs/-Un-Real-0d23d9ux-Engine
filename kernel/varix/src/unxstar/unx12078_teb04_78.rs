//! UNX-F12078 TEB 生命周期：创建/销毁/复用语义链（0x19be0 段）
//!
//! 卡面：TEB 槽位生命周期状态机——Fresh→Attached→Detached→Destroyed→
//! Scrubbed 五态闭环，复用前必须 scrub（脏复用显性拒）。
//! 域：CoRun·UNX-D1-B04。
//!
//! 判据原文：
//! - J1 状态机有序：attach 仅 Fresh/Scrubbed 可入，detach 仅 Attached，
//!   destroy 仅 Detached，scrub 仅 Destroyed，违序必拒。
//! - J2 双毁必拒：Destroyed 态再 destroy 显性双毁码。
//! - J3 脏复用拒：仅 Destroyed（未 scrub）槽位可分配时必拒。
//! - J4 池耗尽显性：池满且无脏槽位时分配必拒。
//! - J5 复用换代：Scrubbed 槽位复用时 epoch 递增。
//! - J6 印账冻结：seal 后分配必拒。
//! - J7 注入五况全抓。
//!
//! 结构：TebLife 五态枚举 + TebSlot 槽位（epoch 换代）+ TebSlotBook
//! 槽池状态机 + 折叠指纹（211 进制卷积）+ 注入核（错序毁/双毁/脏复用/
//! 池耗尽/印后分配）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x19be0 独占 0x20）。
pub const SEG_BASE: u32 = 0x19be0;
pub const TL_BAD_STATE: u32 = SEG_BASE + 0x01; // 状态机错序
pub const TL_DOUBLE_DESTROY: u32 = SEG_BASE + 0x02; // 双毁
pub const TL_REUSE_DIRTY: u32 = SEG_BASE + 0x03; // 脏复用
pub const TL_NO_SLOT: u32 = SEG_BASE + 0x04; // 池耗尽
pub const TL_FROZEN: u32 = SEG_BASE + 0x05; // 印账后分配

/// TEB 槽位五态。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TebLife {
    Fresh = 0,
    Attached = 1,
    Detached = 2,
    Destroyed = 3,
    Scrubbed = 4,
}

/// 槽位：态 + 换代号。
#[derive(Clone, Copy)]
pub struct TebSlot {
    pub id: u32,
    pub state: TebLife,
    pub epoch: u32,
}

/// 槽池。
pub struct TebSlotBook {
    pub slots: Vec<TebSlot>,
    pub capacity: usize,
    pub sealed: bool,
}

impl TebSlotBook {
    pub fn new(capacity: usize) -> TebSlotBook {
        TebSlotBook { slots: Vec::new(), capacity, sealed: false }
    }

    fn find(&self, id: u32) -> Option<usize> {
        self.slots.iter().position(|s| s.id == id)
    }

    /// 分配：优先复用 Scrubbed 槽位（epoch+1），否则新建；脏槽在位必拒。
    pub fn allocate(&mut self) -> Result<u32, u32> {
        if self.sealed {
            return Err(TL_FROZEN);
        }
        // 先找可复用。
        if let Some(i) = self.slots.iter().position(|s| s.state == TebLife::Scrubbed) {
            self.slots[i].state = TebLife::Fresh;
            self.slots[i].epoch += 1;
            return Ok(self.slots[i].id);
        }
        if self.slots.len() < self.capacity {
            let id = self.slots.len() as u32 + 1;
            self.slots.push(TebSlot { id, state: TebLife::Fresh, epoch: 0 });
            return Ok(id);
        }
        if self.slots.iter().any(|s| s.state == TebLife::Destroyed) {
            return Err(TL_REUSE_DIRTY);
        }
        Err(TL_NO_SLOT)
    }

    /// attach：Fresh/Scrubbed → Attached。
    pub fn attach(&mut self, id: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(TL_FROZEN);
        }
        let i = match self.find(id) {
            Some(i) => i,
            None => return Err(TL_BAD_STATE),
        };
        match self.slots[i].state {
            TebLife::Fresh | TebLife::Scrubbed => {
                self.slots[i].state = TebLife::Attached;
                Ok(())
            }
            _ => Err(TL_BAD_STATE),
        }
    }

    /// detach：Attached → Detached。
    pub fn detach(&mut self, id: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(TL_FROZEN);
        }
        let i = match self.find(id) {
            Some(i) => i,
            None => return Err(TL_BAD_STATE),
        };
        if self.slots[i].state != TebLife::Attached {
            return Err(TL_BAD_STATE);
        }
        self.slots[i].state = TebLife::Detached;
        Ok(())
    }

    /// destroy：Detached → Destroyed；Destroyed 再毁显性双毁。
    pub fn destroy(&mut self, id: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(TL_FROZEN);
        }
        let i = match self.find(id) {
            Some(i) => i,
            None => return Err(TL_BAD_STATE),
        };
        if self.slots[i].state == TebLife::Destroyed {
            return Err(TL_DOUBLE_DESTROY);
        }
        if self.slots[i].state != TebLife::Detached {
            return Err(TL_BAD_STATE);
        }
        self.slots[i].state = TebLife::Destroyed;
        Ok(())
    }

    /// scrub：Destroyed → Scrubbed。
    pub fn scrub(&mut self, id: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(TL_FROZEN);
        }
        let i = match self.find(id) {
            Some(i) => i,
            None => return Err(TL_BAD_STATE),
        };
        if self.slots[i].state != TebLife::Destroyed {
            return Err(TL_BAD_STATE);
        }
        self.slots[i].state = TebLife::Scrubbed;
        Ok(())
    }

    /// 某态槽位数。
    pub fn count_in(&self, st: TebLife) -> usize {
        self.slots.iter().filter(|s| s.state == st).count()
    }

    /// 槽位回读。
    pub fn slot_of(&self, id: u32) -> Option<(TebLife, u32)> {
        self.find(id).map(|i| (self.slots[i].state, self.slots[i].epoch))
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for s in &self.slots {
            h = h.wrapping_mul(211).wrapping_add(s.id);
            h = h.wrapping_mul(211).wrapping_add(s.state as u32);
            h = h.wrapping_mul(211).wrapping_add(s.epoch);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 诊断面：摘要。
    pub fn summary(&self) -> String {
        format!("slots={} attached={}", self.slots.len(), self.count_in(TebLife::Attached))
    }

    /// 诊断面：槽位渲染行。
    pub fn slot_line(&self, id: u32) -> String {
        match self.slot_of(id) {
            Some((st, ep)) => format!("S{}|st{}|e{}", id, st as u16, ep),
            None => "missing".to_string(),
        }
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = TebSlotBook::new(8);
    let _ = b.allocate();
    // 1) 错序毁：Fresh 直接 destroy 必拒。
    got[0] = b.destroy(1).err() == Some(TL_BAD_STATE);
    // 2) 双毁：毁两次显性双毁码（槽 1 先走完 attach→detach→destroy 链）。
    let _ = b.attach(1);
    let _ = b.detach(1);
    let _ = b.destroy(1);
    got[1] = b.destroy(1).err() == Some(TL_DOUBLE_DESTROY);
    // 3) 脏复用：池满且仅剩 Destroyed 槽位时分配必拒。
    let mut b2 = TebSlotBook::new(1);
    let _ = b2.allocate();
    let _ = b2.attach(1);
    let _ = b2.detach(1);
    let _ = b2.destroy(1);
    got[2] = b2.allocate().err() == Some(TL_REUSE_DIRTY);
    // 4) 池耗尽：池满且无脏槽位时分配必拒。
    let _ = b2.scrub(1); // 复用后回满
    got[3] = {
        let mut b3 = TebSlotBook::new(1);
        let _ = b3.allocate();
        let _ = b3.attach(1);
        let _ = b3.detach(1);
        b3.allocate().err() == Some(TL_NO_SLOT)
    };
    // 5) 印账后分配必抓。
    b.seal();
    got[4] = b.allocate().err() == Some(TL_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12078_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12078");

    // J1 状态机有序全链。
    let j1 = {
        let mut b = TebSlotBook::new(4);
        let id = match b.allocate() { Ok(i) => i, Err(_) => 0 };
        b.attach(id).is_ok()
            && b.detach(id).is_ok()
            && b.destroy(id).is_ok()
            && b.scrub(id).is_ok()
            && b.slot_of(id) == Some((TebLife::Scrubbed, 0))
    };
    s.add("J1-全链有序", j1, "五态链贯通");

    // J2 错序 attach 拒：Detached 不可 attach。
    let j2 = {
        let mut b = TebSlotBook::new(4);
        let _ = b.allocate();
        let _ = b.attach(1);
        let _ = b.detach(1);
        b.attach(1).err() == Some(TL_BAD_STATE)
    };
    s.add("J2-错序拒", j2, "Detached 直 attach 必拒");

    // J3 双毁显性码。
    let j3 = {
        let mut b = TebSlotBook::new(4);
        let _ = b.allocate();
        let _ = b.attach(1);
        let _ = b.detach(1);
        let _ = b.destroy(1);
        b.destroy(1).err() == Some(TL_DOUBLE_DESTROY)
    };
    s.add("J3-双毁码", j3, "二次 destroy 显性");

    // J4 脏复用拒。
    let j4 = {
        let mut b = TebSlotBook::new(1);
        let _ = b.allocate();
        let _ = b.attach(1);
        let _ = b.detach(1);
        let _ = b.destroy(1);
        b.allocate().err() == Some(TL_REUSE_DIRTY)
    };
    s.add("J4-脏复用拒", j4, "未 scrub 复用必拒");

    // J5 池耗尽显性。
    let j5 = {
        let mut b = TebSlotBook::new(1);
        let _ = b.allocate();
        let _ = b.attach(1);
        b.allocate().err() == Some(TL_NO_SLOT)
    };
    s.add("J5-池耗尽", j5, "Attached 占满后分配必拒");

    // J6 复用换代 epoch 递增。
    let j6 = {
        let mut b = TebSlotBook::new(1);
        let _ = b.allocate();
        let _ = b.attach(1);
        let _ = b.detach(1);
        let _ = b.destroy(1);
        let _ = b.scrub(1);
        let id2 = b.allocate().unwrap_or(0);
        b.slot_of(id2) == Some((TebLife::Fresh, 1))
    };
    s.add("J6-复用换代", j6, "epoch 0→1");

    // J7 态统计回读。
    let j7 = {
        let mut b = TebSlotBook::new(4);
        let _ = b.allocate();
        let _ = b.attach(1);
        b.count_in(TebLife::Attached) == 1 && b.count_in(TebLife::Fresh) == 0
    };
    s.add("J7-态统计", j7, "attach 后 Fresh 清零");

    // J8 印账冻结。
    let j8 = {
        let mut b = TebSlotBook::new(4);
        b.seal();
        b.allocate().err() == Some(TL_FROZEN)
    };
    s.add("J8-印账冻结", j8, "seal 后分配必拒");

    // J9 指纹幂等。
    let j9 = {
        let mut b = TebSlotBook::new(4);
        let _ = b.allocate();
        b.fold() == b.fold()
    };
    s.add("J9-指纹幂等", j9, "同态同指纹");

    // J10 注入核 5/5。
    let got = inject_five();
    let j10 = got.iter().all(|x| *x);
    s.add_dyn("J10-注入全抓", j10, &format!("inj5={:?}", got));

    // 汇总样例账。
    let mut b = TebSlotBook::new(4);
    let _ = b.allocate();
    let _ = b.attach(1);

    s.add("摘要回读", b.summary().contains("attached=1"), "摘要含附加数");
    s.add("渲染行", b.slot_line(1).contains("st1"), "Attached 渲染 st1");
    s.add("缺席行", b.slot_line(9) == "missing", "未分配显性");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12078_lifecycle_flow() {
        let mut b = TebSlotBook::new(2);
        let _ = b.allocate();
        assert_eq!(b.attach(1), Ok(()));
        assert_eq!(b.attach(1), Err(TL_BAD_STATE));
        let _ = b.detach(1);
        let _ = b.destroy(1);
        assert_eq!(b.destroy(1), Err(TL_DOUBLE_DESTROY));
    }

    #[test]
    fn unx12078_checks_green() {
        let s = run_unx12078_checks();
        assert!(s.all_passed(), "UNX-F12078 判据未全绿");
    }

    #[test]
    fn reuse_epoch_bump() {
        let mut b = TebSlotBook::new(1);
        let _ = b.allocate();
        let _ = b.attach(1);
        let _ = b.detach(1);
        let _ = b.destroy(1);
        let _ = b.scrub(1);
        let _ = b.allocate();
        assert_eq!(b.slot_of(1), Some((TebLife::Fresh, 1)));
    }
}
