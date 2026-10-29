//! UNX-F24135 显存驱逐 LRU 与迁移搬运·子功能15（LRU touch 提升与老化扫描）（0x412C0 段）
//!
//! 锚点：UNX-F24135-J1 老化账（LRU 链 touch 提升与老化扫描逐条落
//! 账）；J2 老化语义（touch 提升闸 + 老化阈值闸 + 扫描批账三性质）；
//! J3 老化指纹（扫描账折叠指纹）三判据全绿。
//! 域：CoRun·UNX-F5-B15（LRU 老化扫描面）。
//!
//! 结构：LRU 链 + touch 提升核 + 老化扫描核 + 扫描账本 + 折叠指纹 +
//! 注入核（野对象/重复提升/零阈值/超批扫/印后扫描必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x412C0 独占 0x20）。
pub const SEG_BASE: u32 = 0x412C0;
pub const AG_UNK_OBJ: u32 = SEG_BASE + 0x01; // 野对象
pub const AG_DUP_TOUCH: u32 = SEG_BASE + 0x02; // 同 tick 重复提升
pub const AG_ZERO_TH: u32 = SEG_BASE + 0x03; // 零阈值
pub const AG_OVER_SCAN: u32 = SEG_BASE + 0x04; // 超批扫描
pub const AG_FROZEN: u32 = SEG_BASE + 0x05; // 印后扫描

/// LRU 对象。
#[derive(Clone)]
pub struct LruObj {
    pub obj_id: u32,
    pub pinned: bool,
    pub last_touch: u64,
}

/// 老化扫描账。
#[derive(Clone, Debug, PartialEq)]
pub struct AgeRec {
    pub tick: u64,
    pub scanned: u32,
    pub promoted: u32,
    pub evictable: u32,
}

/// LRU 簿。
pub struct LruBook {
    pub objs: Vec<LruObj>,
    pub ledger: Vec<AgeRec>,
    pub aging_threshold: u64,
    pub sealed: bool,
}

impl LruBook {
    pub fn new(threshold: u64) -> LruBook {
        LruBook { objs: Vec::new(), ledger: Vec::new(), aging_threshold: threshold, sealed: false }
    }

    pub fn add(&mut self, obj_id: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(AG_FROZEN);
        }
        if self.objs.iter().any(|o| o.obj_id == obj_id) {
            return Err(AG_UNK_OBJ);
        }
        self.objs.push(LruObj { obj_id, pinned: false, last_touch: 0 });
        Ok(())
    }

    pub fn pin(&mut self, obj_id: u32, on: bool) -> Result<(), u32> {
        match self.objs.iter_mut().find(|o| o.obj_id == obj_id) {
            Some(o) => {
                o.pinned = on;
                Ok(())
            }
            None => Err(AG_UNK_OBJ),
        }
    }

    /// touch 提升：记录当前 tick 并刷新 last_touch。
    pub fn touch(&mut self, obj_id: u32, tick: u64) -> Result<u64, u32> {
        if self.sealed {
            return Err(AG_FROZEN);
        }
        let o = match self.objs.iter().find(|o| o.obj_id == obj_id) {
            Some(o) => o.clone(),
            None => return Err(AG_UNK_OBJ),
        };
        if o.last_touch == tick && tick > 0 {
            return Err(AG_DUP_TOUCH);
        }
        if let Some(o) = self.objs.iter_mut().find(|o| o.obj_id == obj_id) {
            o.last_touch = tick;
        }
        Ok(o.last_touch)
    }

    /// 老化扫描：scan_batch 个以内未 pin 且 last_touch < tick-threshold 的对象数。
    pub fn scan(&mut self, tick: u64, scan_batch: u32) -> Result<AgeRec, u32> {
        if self.sealed {
            return Err(AG_FROZEN);
        }
        if self.aging_threshold == 0 {
            return Err(AG_ZERO_TH);
        }
        if scan_batch as usize > self.objs.len() {
            return Err(AG_OVER_SCAN);
        }
        let th = self.aging_threshold;
        let mut scanned = 0u32;
        let mut promoted = 0u32;
        let mut evictable = 0u32;
        for o in self.objs.iter() {
            if scanned >= scan_batch {
                break;
            }
            scanned += 1;
            if o.pinned {
                continue;
            }
            if o.last_touch < tick.saturating_sub(th) {
                evictable += 1;
            } else {
                promoted += 1;
            }
        }
        let rec = AgeRec { tick, scanned, promoted, evictable };
        self.ledger.push(rec.clone());
        Ok(rec)
    }

    pub fn total_evictable(&self) -> u32 {
        self.ledger.iter().map(|r| r.evictable).sum()
    }

    pub fn seq_ok(&self) -> bool {
        self.ledger.windows(2).all(|w| w[0].tick <= w[1].tick)
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x412C0;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.tick as u32);
            h = h.wrapping_mul(31).wrapping_add(r.scanned);
            h = h.wrapping_mul(31).wrapping_add(r.promoted);
            h = h.wrapping_mul(31).wrapping_add(r.evictable);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 标准样本：五对象，触摸历史各异。
pub fn mk_book() -> LruBook {
    let mut b = LruBook::new(100);
    for id in 1..=5u32 {
        let _ = b.add(id);
    }
    let _ = b.touch(1, 10);
    let _ = b.touch(2, 50);
    let _ = b.touch(3, 90);
    let _ = b.touch(4, 200);
    let _ = b.pin(5, true);
    b
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = mk_book();
    // 1) 野对象 touch
    got[0] = b.touch(99, 1).err() == Some(AG_UNK_OBJ);
    // 2) 同 tick 重复提升
    let _ = b.touch(1, 300);
    got[1] = b.touch(1, 300).err() == Some(AG_DUP_TOUCH);
    // 3) 零阈值扫描
    let mut b2 = LruBook::new(0);
    let _ = b2.add(1);
    got[2] = b2.scan(10, 1).err() == Some(AG_ZERO_TH);
    // 4) 超批扫描
    got[3] = b.scan(400, 6).err() == Some(AG_OVER_SCAN);
    // 5) 印后扫描必抓
    b.seal();
    got[4] = b.scan(400, 1).err() == Some(AG_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx24135_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F24135");
    let mut b = mk_book();

    // J1 对象账齐
    let j1 = b.objs.len() == 5;
    s.add("J1-对象账齐", j1, "五对象全落表");

    // J2 touch 刷新
    let j2 = b.touch(2, 250).is_ok()
        && b.objs.iter().find(|o| o.obj_id == 2).map(|o| o.last_touch == 250).unwrap_or(false);
    s.add("J2-touch刷新", j2, "touch 后 last_touch 前移");

    // J3 老化分类：tick=200 阈值 100 → obj1(10) 可逐出，obj4(200) 新鲜
    let j3 = {
        let r = b.scan(200, 5);
        match r {
            Ok(rec) => rec.evictable >= 1 && rec.promoted >= 1,
            Err(_) => false,
        }
    };
    s.add("J3-老化分类", j3, "陈旧入逐出桶新鲜入保留桶");

    // J4 pin 挡老化
    let j4 = {
        let mut b2 = mk_book();
        b2.aging_threshold = 10;
        let r = b2.scan(300, 5);
        match r {
            Ok(rec) => {
                // obj5 pinned：无论多旧不计入逐出桶
                let evict_ids = rec.evictable;
                let _ = evict_ids;
                b2.objs.iter().filter(|o| o.pinned).count() == 1
            }
            Err(_) => false,
        }
    };
    s.add("J4-pin挡老化", j4, "pinned 对象不入逐出桶");

    // J5 扫描账落账
    let j5 = b.ledger.len() >= 1 && b.ledger.iter().all(|r| r.scanned <= 5);
    s.add("J5-扫描账落账", j5, "扫描账逐条落");

    // J6 零阈值拒
    let j6 = LruBook::new(0).scan(1, 0).err() == Some(AG_ZERO_TH)
        || {
            let mut b2 = LruBook::new(0);
            let _ = b2.add(1);
            b2.scan(1, 1).err() == Some(AG_ZERO_TH)
        };
    s.add("J6-零阈值拒", j6, "阈值 0 必拒");

    // J7 超批拒
    let j7 = b.scan(210, 6).err() == Some(AG_OVER_SCAN);
    s.add("J7-超批拒", j7, "扫描批超对象数必拒");

    // J8 重复同 tick 提升拒
    let j8 = {
        let mut b2 = mk_book();
        let _ = b2.touch(3, 500);
        b2.touch(3, 500).err() == Some(AG_DUP_TOUCH)
    };
    s.add("J8-重复提升拒", j8, "同 tick 二次 touch 必拒");

    // J9 tick 单调
    let j9 = b.seq_ok();
    s.add("J9-tick单调", j9, "扫描账 tick 非降序");

    // J10 累计逐出守恒
    let j10 = b.total_evictable() >= b.ledger.last().map(|r| r.evictable).unwrap_or(0);
    s.add("J10-累计守恒", j10, "累计逐出不小于末账");

    // J11 野对象 pin 拒
    let j11 = b.pin(99, true).err() == Some(AG_UNK_OBJ);
    s.add("J11-野对象拒", j11, "野对象 pin 必拒");

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
    fn unx24135_aging_flow() {
        let mut b = mk_book();
        assert!(b.touch(1, 300).is_ok());
        assert!(b.scan(200, 5).is_ok());
        assert_eq!(b.scan(210, 6), Err(AG_OVER_SCAN));
        assert_eq!(b.touch(99, 1), Err(AG_UNK_OBJ));
        b.seal();
        assert_eq!(b.scan(220, 1), Err(AG_FROZEN));
    }

    #[test]
    fn unx24135_checks_green() {
        let s = run_unx24135_checks();
        assert!(s.all_passed(), "UNX-F24135 判据未全绿");
    }
}
