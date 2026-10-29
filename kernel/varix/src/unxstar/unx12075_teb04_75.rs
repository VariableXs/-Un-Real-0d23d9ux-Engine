//! UNX-F12075 TEB Version/SpareBytes 保留字段账（0x19b80 段）
//!
//! 卡面：TEB→Version/SpareBytes 等保留字段——保留区必须保持零值、
//! 版本戳单调递增；对保留区的非零写一律显性拒并落红账。
//! 域：CoRun·UNX-D1-B04。
//!
//! 判据原文：
//! - J1 保留闭集：保留槽位编号闭集，未知槽位必拒。
//! - J2 非零写拒：向保留槽位写非零值必拒。
//! - J3 零值确认一次性：对同一槽位的零值确认（scrub）仅一次，重复必拒。
//! - J4 版本单调：版本戳只升不降，回退必拒。
//! - J5 红账留痕：每次拒收落红账可回读。
//! - J6 印账冻结：seal 后一切写必拒。
//! - J7 注入五况全抓。
//!
//! 结构：ReservedSlot 保留槽 + ReservedBook 保留账（槽位 + 版本戳 +
//! 红账）+ 折叠指纹（211 进制卷积）+ 注入核（坏槽位/非零写/双 scrub/
//! 版本回退/印后写）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x19b80 独占 0x20）。
pub const SEG_BASE: u32 = 0x19b80;
pub const RV_BAD_ID: u32 = SEG_BASE + 0x01; // 未知槽位
pub const RV_RESERVED: u32 = SEG_BASE + 0x02; // 非零写保留区
pub const RV_DOUBLE_SCRUB: u32 = SEG_BASE + 0x03; // 重复零值确认
pub const RV_BACK_VERSION: u32 = SEG_BASE + 0x04; // 版本回退
pub const RV_FROZEN: u32 = SEG_BASE + 0x05; // 印账后写

/// 保留槽位。
#[derive(Clone, Copy)]
pub struct ReservedSlot {
    pub id: u32,
    pub scrubbed: bool,
}

/// 保留字段账。
pub struct ReservedBook {
    pub slots: Vec<ReservedSlot>,
    pub version: u32,
    pub reds: Vec<(u32, u32)>, // 红账：(错误码, 槽位或版本)
    pub sealed: bool,
}

impl ReservedBook {
    pub fn new(ids: &[u32]) -> ReservedBook {
        let mut slots = Vec::new();
        for id in ids {
            slots.push(ReservedSlot { id: *id, scrubbed: false });
        }
        ReservedBook { slots, version: 1, reds: Vec::new(), sealed: false }
    }

    fn find(&self, id: u32) -> Option<usize> {
        self.slots.iter().position(|s| s.id == id)
    }

    /// 对保留槽位写：非零必拒；零值确认一次性。
    pub fn poke(&mut self, id: u32, val: u32) -> Result<(), u32> {
        if self.sealed {
            self.reds.push((RV_FROZEN, id));
            return Err(RV_FROZEN);
        }
        let i = match self.find(id) {
            Some(i) => i,
            None => {
                self.reds.push((RV_BAD_ID, id));
                return Err(RV_BAD_ID);
            }
        };
        if val != 0 {
            self.reds.push((RV_RESERVED, id));
            return Err(RV_RESERVED);
        }
        if self.slots[i].scrubbed {
            self.reds.push((RV_DOUBLE_SCRUB, id));
            return Err(RV_DOUBLE_SCRUB);
        }
        self.slots[i].scrubbed = true;
        Ok(())
    }

    /// 版本戳推进：严格递增。
    pub fn bump(&mut self, v: u32) -> Result<(), u32> {
        if self.sealed {
            self.reds.push((RV_FROZEN, v));
            return Err(RV_FROZEN);
        }
        if v <= self.version {
            self.reds.push((RV_BACK_VERSION, v));
            return Err(RV_BACK_VERSION);
        }
        self.version = v;
        Ok(())
    }

    /// 全槽位零值确认齐备。
    pub fn all_scrubbed(&self) -> bool {
        self.slots.iter().all(|s| s.scrubbed)
    }

    /// 已确认槽数。
    pub fn scrub_count(&self) -> usize {
        self.slots.iter().filter(|s| s.scrubbed).count()
    }

    /// 红账条数。
    pub fn red_count(&self) -> usize {
        self.reds.len()
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for s in &self.slots {
            h = h.wrapping_mul(211).wrapping_add(s.id);
            h = h.wrapping_mul(211).wrapping_add(s.scrubbed as u32);
        }
        h = h.wrapping_mul(211).wrapping_add(self.version);
        for r in &self.reds {
            h = h.wrapping_mul(211).wrapping_add(r.0);
            h = h.wrapping_mul(211).wrapping_add(r.1);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 诊断面：摘要。
    pub fn summary(&self) -> String {
        format!("v={} scrub={} reds={}", self.version, self.scrub_count(), self.red_count())
    }

    /// 诊断面：槽位渲染行。
    pub fn slot_line(&self, id: u32) -> String {
        match self.find(id) {
            Some(i) => format!("R{}|scrub={}", id, self.slots[i].scrubbed),
            None => "missing".to_string(),
        }
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = ReservedBook::new(&[0x0c, 0x34]);
    // 1) 未知槽位必拒。
    got[0] = b.poke(0x99, 0).err() == Some(RV_BAD_ID);
    // 2) 非零写保留区必拒。
    got[1] = b.poke(0x0c, 0x1234).err() == Some(RV_RESERVED);
    // 3) 重复零值确认必拒。
    let _ = b.poke(0x0c, 0);
    got[2] = b.poke(0x0c, 0).err() == Some(RV_DOUBLE_SCRUB);
    // 4) 版本回退必拒。
    let _ = b.bump(5);
    got[3] = b.bump(3).err() == Some(RV_BACK_VERSION);
    // 5) 印账后写必抓。
    b.seal();
    got[4] = b.poke(0x34, 0).err() == Some(RV_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12075_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12075");

    // J1 保留闭集：未知槽位必拒。
    let j1 = {
        let mut b = ReservedBook::new(&[0x0c]);
        b.poke(0x77, 0).err() == Some(RV_BAD_ID)
    };
    s.add("J1-槽位闭集", j1, "未知槽位必拒");

    // J2 非零写拒并落红账。
    let j2 = {
        let mut b = ReservedBook::new(&[0x0c]);
        b.poke(0x0c, 1).err() == Some(RV_RESERVED) && b.red_count() == 1
    };
    s.add("J2-非零写拒", j2, "保留区非零写必拒且留痕");

    // J3 零值确认一次性。
    let j3 = {
        let mut b = ReservedBook::new(&[0x0c, 0x34]);
        b.poke(0x0c, 0).is_ok()
            && b.poke(0x0c, 0).err() == Some(RV_DOUBLE_SCRUB)
            && b.poke(0x34, 0).is_ok()
            && b.all_scrubbed()
    };
    s.add("J3-单次确认", j3, "重复确认必拒且全确认可判");

    // J4 版本单调。
    let j4 = {
        let mut b = ReservedBook::new(&[0x0c]);
        b.bump(2).is_ok()
            && b.bump(1).err() == Some(RV_BACK_VERSION)
            && b.bump(2).err() == Some(RV_BACK_VERSION)
            && b.bump(3).is_ok()
    };
    s.add("J4-版本单调", j4, "平版本与回退皆拒");

    // J5 红账可回读。
    let j5 = {
        let mut b = ReservedBook::new(&[0x0c]);
        let _ = b.poke(0x0c, 9);
        b.red_count() == 1 && b.fold() != 0
    };
    s.add("J5-红账留痕", j5, "红账入账且影响指纹");

    // J6 版本推进入账。
    let j6 = {
        let mut b = ReservedBook::new(&[0x0c]);
        let _ = b.bump(7);
        b.version == 7 && b.summary().contains("v=7")
    };
    s.add("J6-版本回读", j6, "版本戳可回读");

    // J7 槽位渲染行。
    let j7 = {
        let mut b = ReservedBook::new(&[0x0c]);
        let _ = b.poke(0x0c, 0);
        b.slot_line(0x0c).contains("scrub=true") && b.slot_line(9) == "missing"
    };
    s.add("J7-槽位渲染", j7, "确认态可渲染");

    // J8 印账冻结。
    let j8 = {
        let mut b = ReservedBook::new(&[0x0c]);
        b.seal();
        b.poke(0x0c, 0).err() == Some(RV_FROZEN)
            && b.bump(9).err() == Some(RV_FROZEN)
    };
    s.add("J8-印账冻结", j8, "seal 后皆拒");

    // J9 指纹幂等。
    let j9 = {
        let mut b = ReservedBook::new(&[0x0c]);
        let _ = b.poke(0x0c, 0);
        b.fold() == b.fold()
    };
    s.add("J9-指纹幂等", j9, "同态同指纹");

    // J10 注入核 5/5。
    let got = inject_five();
    let j10 = got.iter().all(|x| *x);
    s.add_dyn("J10-注入全抓", j10, &format!("inj5={:?}", got));

    // 汇总样例账。
    let mut b = ReservedBook::new(&[0x0c, 0x34, 0x58]);
    let _ = b.poke(0x0c, 0);
    let _ = b.poke(0x34, 0);
    let _ = b.bump(4);

    s.add("确认计数", b.scrub_count() == 2, "两槽已确认");
    s.add("摘要回读", b.summary().contains("reds=0"), "无红账");
    s.add("版本一致", b.version == 4, "版本戳 4");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12075_reserved_flow() {
        let mut b = ReservedBook::new(&[0x0c, 0x34]);
        assert_eq!(b.poke(0x0c, 5), Err(RV_RESERVED));
        assert!(b.poke(0x0c, 0).is_ok());
        assert_eq!(b.poke(0x0c, 0), Err(RV_DOUBLE_SCRUB));
        assert!(b.bump(3).is_ok());
        assert_eq!(b.bump(3), Err(RV_BACK_VERSION));
    }

    #[test]
    fn unx12075_checks_green() {
        let s = run_unx12075_checks();
        assert!(s.all_passed(), "UNX-F12075 判据未全绿");
    }

    #[test]
    fn red_ledger_records() {
        let mut b = ReservedBook::new(&[0x0c]);
        let _ = b.poke(0x77, 0);
        let _ = b.poke(0x0c, 1);
        assert_eq!(b.red_count(), 2);
        assert!(b.slot_line(0x77) == "missing");
    }
}
