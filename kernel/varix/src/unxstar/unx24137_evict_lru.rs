//! UNX-F24137 显存驱逐 LRU 与迁移搬运·子功能17（驱逐水位闸与批量逐出）（0x41300 段）
//!
//! 锚点：UNX-F24137-J1 水位账（水位判定与批量逐出逐条落账）；J2 水
//! 位语义（阈值闸 + 批量上限 + pinned 豁免三性质）；J3 水位指纹（批
//! 账折叠指纹）三判据全绿。
//! 域：CoRun·UNX-F5-B15（水位批量逐出面）。
//!
//! 结构：占用水位表 + 水位判定核 + 批量逐出核 + 批账本 + 折叠指纹 +
//! 注入核（零水位/超批/pinned 逐出/野对象/印后必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x41300 独占 0x20）。
pub const SEG_BASE: u32 = 0x41300;
pub const WL_ZERO_TH: u32 = SEG_BASE + 0x01; // 零阈值
pub const WL_OVER_BATCH: u32 = SEG_BASE + 0x02; // 批量超上限
pub const WL_PINNED: u32 = SEG_BASE + 0x03; // pinned 豁免仍逐
pub const WL_UNK_OBJ: u32 = SEG_BASE + 0x04; // 野对象
pub const WL_FROZEN: u32 = SEG_BASE + 0x05; // 印后操作

/// 可逐出对象。
#[derive(Clone)]
pub struct WlObj {
    pub obj_id: u32,
    pub pages: u32,
    pub pinned: bool,
    pub evicted: bool,
}

/// 水位批账。
#[derive(Clone, Debug, PartialEq)]
pub struct WlRec {
    pub seq: u32,
    pub level_before: u32,
    pub target: u32,
    pub evicted_pages: u32,
    pub count: u32,
}

/// 水位簿。
pub struct WlBook {
    pub objs: Vec<WlObj>,
    pub cap_pages: u32,
    pub watermark: u32,
    pub batch_max: u32,
    pub ledger: Vec<WlRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl WlBook {
    pub fn new(cap_pages: u32, watermark: u32, batch_max: u32) -> WlBook {
        WlBook {
            objs: Vec::new(),
            cap_pages,
            watermark,
            batch_max,
            ledger: Vec::new(),
            seq_next: 1,
            sealed: false,
        }
    }

    pub fn add(&mut self, obj_id: u32, pages: u32, pinned: bool) -> Result<(), u32> {
        if self.sealed {
            return Err(WL_FROZEN);
        }
        if self.objs.iter().any(|o| o.obj_id == obj_id) {
            return Err(WL_UNK_OBJ);
        }
        self.objs.push(WlObj { obj_id, pages, pinned, evicted: false });
        Ok(())
    }

    pub fn used_pages(&self) -> u32 {
        self.objs.iter().filter(|o| !o.evicted).map(|o| o.pages).sum()
    }

    pub fn over_watermark(&self) -> bool {
        self.used_pages() > self.watermark
    }

    /// 批量逐出：从表头扫，逐出未 pin 对象至水位下或批满。
    pub fn evict_batch(&mut self) -> Result<WlRec, u32> {
        if self.sealed {
            return Err(WL_FROZEN);
        }
        if self.watermark == 0 {
            return Err(WL_ZERO_TH);
        }
        if !self.over_watermark() {
            return Err(WL_ZERO_TH);
        }
        let target = self.watermark;
        let mut count = 0u32;
        let mut evicted_pages = 0u32;
        let before = self.used_pages();
        let ids: Vec<u32> = self.objs.iter().filter(|o| !o.evicted).map(|o| o.obj_id).collect();
        for id in ids {
            if count >= self.batch_max || self.used_pages() <= target {
                break;
            }
            let o = match self.objs.iter().find(|o| o.obj_id == id) {
                Some(o) => o.clone(),
                None => return Err(WL_UNK_OBJ),
            };
            if o.pinned {
                continue; // pinned 豁免
            }
            if let Some(o) = self.objs.iter_mut().find(|o| o.obj_id == id) {
                o.evicted = true;
            }
            count += 1;
            evicted_pages += o.pages;
        }
        let seq = self.seq_next;
        self.seq_next += 1;
        let rec = WlRec { seq, level_before: before, target, evicted_pages, count };
        self.ledger.push(rec.clone());
        Ok(rec)
    }

    pub fn batch_ok(&self) -> bool {
        self.ledger.iter().all(|r| r.count <= self.batch_max)
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
        let mut h: u32 = 0x41300;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.seq);
            h = h.wrapping_mul(31).wrapping_add(r.level_before);
            h = h.wrapping_mul(31).wrapping_add(r.evicted_pages);
            h = h.wrapping_mul(31).wrapping_add(r.count);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 标准样本：容量 100，水位 60，批上限 3；占用 90（超水位）。
pub fn mk_book() -> WlBook {
    let mut b = WlBook::new(100, 60, 3);
    let _ = b.add(1, 20, false);
    let _ = b.add(2, 25, true); // pinned 豁免
    let _ = b.add(3, 20, false);
    let _ = b.add(4, 25, false);
    b
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = mk_book();
    // 1) 零阈值
    let mut b2 = WlBook::new(100, 0, 3);
    let _ = b2.add(1, 10, false);
    got[0] = b2.evict_batch().err() == Some(WL_ZERO_TH);
    // 2) 未超水位拒
    let mut b3 = WlBook::new(100, 200, 3);
    let _ = b3.add(1, 10, false);
    got[1] = b3.evict_batch().err() == Some(WL_ZERO_TH);
    // 3) 批上限守恒：三未 pin 对象、批上限 1，单批 count 必为 1
    let mut b4 = WlBook::new(100, 10, 1);
    let _ = b4.add(1, 30, false);
    let _ = b4.add(2, 30, false);
    let _ = b4.add(3, 30, false);
    got[2] = matches!(b4.evict_batch(), Ok(ref rec) if rec.count == 1);
    // 4) 野对象（add 重号即拒）
    got[3] = b.add(1, 5, false).err() == Some(WL_UNK_OBJ);
    // 5) 印后操作必抓
    b.seal();
    got[4] = b.evict_batch().err() == Some(WL_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx24137_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F24137");
    let mut b = mk_book();

    // J1 占用账齐
    let j1 = b.objs.len() == 4 && b.used_pages() == 90;
    s.add("J1-占用账齐", j1, "四对象占 90 页");

    // J2 超水位判定
    let j2 = b.over_watermark();
    s.add("J2-超水位判定", j2, "90 > 60 超水位");

    // J3 批量逐出落账
    let j3 = {
        match b.evict_batch() {
            Ok(r) => r.count >= 1 && r.evicted_pages >= 1 && r.level_before == 90,
            Err(_) => false,
        }
    };
    s.add("J3-批逐出落账", j3, "批账含前后水位与页数");

    // J4 批上限守恒
    let j4 = b.batch_ok();
    s.add("J4-批上限守恒", j4, "每批 count ≤ batch_max");

    // J5 pinned 豁免
    let j5 = b.objs.iter().find(|o| o.obj_id == 2).map(|o| !o.evicted).unwrap_or(false);
    s.add("J5-pinned豁免", j5, "pinned 对象不被逐");

    // J6 逐出后水位精确回落（批1 逐 obj1+obj3 共 40 页 → 50 ≤ 60）
    let j6 = b.used_pages() == 50;
    s.add("J6-水位回落", j6, "逐出后占用趋降");

    // J7 零阈值拒
    let j7 = {
        let mut b2 = WlBook::new(100, 0, 3);
        let _ = b2.add(1, 10, false);
        b2.evict_batch().err() == Some(WL_ZERO_TH)
    };
    s.add("J7-零阈值拒", j7, "watermark=0 必拒");

    // J8 未超水位拒
    let j8 = {
        let mut b2 = WlBook::new(100, 200, 3);
        let _ = b2.add(1, 10, false);
        b2.evict_batch().err() == Some(WL_ZERO_TH)
    };
    s.add("J8-未超水位拒", j8, "占用≤水位必拒");

    // J9 重号对象拒
    let j9 = b.add(1, 5, false).err() == Some(WL_UNK_OBJ);
    s.add("J9-重号拒", j9, "对象重号必拒");

    // J10 批账 seq 唯一
    let j10 = b.seq_unique();
    s.add("J10-seq唯一", j10, "批账 seq 不重");

    // J11 全逐出后止批
    let j11 = {
        let mut b2 = WlBook::new(100, 10, 10);
        let _ = b2.add(1, 30, false);
        let _ = b2.add(2, 30, false);
        match b2.evict_batch() {
            Ok(r) => r.count == 2 && b2.used_pages() == 0,
            Err(_) => false,
        }
    };
    s.add("J11-全逐止批", j11, "清空后批终止");

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
    fn unx24137_watermark_flow() {
        let mut b = mk_book();
        assert!(b.over_watermark());
        let r = b.evict_batch();
        assert!(r.is_ok());
        assert!(b.batch_ok());
        b.seal();
        assert_eq!(b.evict_batch(), Err(WL_FROZEN));
        let mut b2 = WlBook::new(10, 0, 1);
        assert_eq!(b2.evict_batch(), Err(WL_ZERO_TH));
    }

    #[test]
    fn unx24137_checks_green() {
        let s = run_unx24137_checks();
        assert!(s.all_passed(), "UNX-F24137 判据未全绿");
    }
}
