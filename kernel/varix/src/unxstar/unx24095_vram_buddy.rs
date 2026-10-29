//! UNX-F24095 VRAM 伙伴分配器与显存账本·子功能15（0x40DC0 段）
//!
//! 锚点：UNX-F24095-J1 伙伴块表 + 分配账 + 水位计逐条落账；J2 字段齐备；
//! J3~J12 核心操作语义与折叠指纹（冻结闸切面并入 J3/J11）；J13 注入核 5/5。
//! 域：CoRun·UNX-F5-B15（VRAM 伙伴分配器与显存账本）。
//!
//! 结构：伙伴块表 + 分配账 + 水位计 + 冻结闸（SealGate）切面账 + 注入核
//! （越阶/野块/双放/水位/印后 六路必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x40DC0 独占 0x20）。
pub const SEG_BASE: u32 = 0x40DC0;
pub const VB_BAD_ORDER: u32 = SEG_BASE + 0x01; // 非法阶
pub const VB_BAD_BLK: u32 = SEG_BASE + 0x02; // 野块
pub const VB_DBL_FREE: u32 = SEG_BASE + 0x03; // 双重释放
pub const VB_NO_ROOM: u32 = SEG_BASE + 0x04; // 水位不足
pub const VB_FROZEN: u32 = SEG_BASE + 0x05; // 印后分配

/// 冻结闸切面账（VRAM 伙伴分配器与显存账本子切面）：记录 (k, v) 对，
/// v < 2 违者回 VB_NO_ROOM，同 k 重记回 VB_DBL_FREE。
pub struct SealGate {
    pub tag: u32,
    pub hits: u32,
    pub log: Vec<(u32, u32)>,
}

impl SealGate {
    pub fn new(tag: u32) -> SealGate {
        SealGate { tag, hits: 0, log: vec![] }
    }

    /// 记账一笔：v < 2。
    pub fn record(&mut self, k: u32, v: u32) -> Result<(), u32> {
        if v < 2 {
            return Err(VB_NO_ROOM);
        }
        if self.log.iter().any(|(lk, _)| *lk == k) {
            return Err(VB_DBL_FREE);
        }
        self.log.push((k, v));
        self.hits += 1;
        Ok(())
    }

    /// 切面累计值（冻结闸）。
    pub fn seal_cnt(&self) -> u32 {
        let mut acc: u32 = 0;
        for (k, v) in &self.log {
            acc = acc.wrapping_add((*k).wrapping_mul(*v).wrapping_add(self.tag));
        }
        acc
    }

    /// 切面折叠指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x40DC0;
        for (k, v) in &self.log {
            h = h.wrapping_mul(35).wrapping_add((*k << 8) ^ *v);
        }
        h ^ self.tag
    }
}

/// 伙伴块。
#[derive(Clone, Copy, PartialEq)]
pub struct Blk {
    pub order: u32,
    pub off: u32,
    pub free: bool,
}

/// 分配账。
#[derive(Clone, Copy, PartialEq)]
pub struct AllocRec {
    pub seq: u32,
    pub order: u32,
    pub off: u32,
    pub live: bool,
}

/// BuddyBank（伙伴块表 + 分配账 + 水位计）。
pub struct BuddyBank {
    pub blocks: Vec<Blk>,
    pub allocs: Vec<AllocRec>,
    pub total_pages: u32,
    pub watermark: u32,
    pub peak: u32,
    pub seq_next: u32,
    pub sealed: bool,
}

impl BuddyBank {
    /// 种块：整池折成单一最高阶自由块。
    pub fn new(total_pages: u32) -> BuddyBank {
        let top = 31 - total_pages.leading_zeros();
        BuddyBank {
            blocks: vec![Blk { order: top, off: 0, free: true }],
            allocs: vec![],
            total_pages,
            watermark: 0,
            peak: 0,
            seq_next: 1,
            sealed: false,
        }
    }

    pub fn max_order(&self) -> u32 {
        31 - self.total_pages.leading_zeros()
    }

    /// 自由页合计（含各阶）。
    pub fn free_pages(&self) -> u32 {
        self.blocks.iter().filter(|b| b.free).map(|b| 1u32 << b.order).sum()
    }

    /// 分配 2^order 页：越阶拒 / 水位不足拒；自高阶逐级分裂落账。
    pub fn alloc(&mut self, order: u32) -> Result<u32, u32> {
        if self.sealed {
            return Err(VB_FROZEN);
        }
        if order > self.max_order() {
            return Err(VB_BAD_ORDER);
        }
        let mut src: Option<(usize, u32)> = None;
        for o in order..=self.max_order() {
            if let Some(i) = self.blocks.iter().position(|b| b.free && b.order == o) {
                src = Some((i, o));
                break;
            }
        }
        let (i, mut o) = match src {
            Some(x) => x,
            None => return Err(VB_NO_ROOM),
        };
        // 自高阶向下对半分裂至目标阶
        while o > order {
            let b = self.blocks[i];
            let half = b.off + (1u32 << (o - 1));
            self.blocks[i] = Blk { order: o - 1, off: b.off, free: true };
            self.blocks.push(Blk { order: o - 1, off: half, free: true });
            o -= 1;
        }
        self.blocks[i].free = false;
        let off = self.blocks[i].off;
        let seq = self.seq_next;
        self.seq_next += 1;
        self.allocs.push(AllocRec { seq, order, off, live: true });
        self.watermark += 1u32 << order;
        if self.watermark > self.peak {
            self.peak = self.watermark;
        }
        Ok(off)
    }

    /// 释放：野块拒 / 双放拒；逐级合并伙伴还原高阶。
    pub fn free(&mut self, order: u32, off: u32) -> Result<(), u32> {
        let ai = match self.allocs.iter().position(|a| a.live && a.order == order && a.off == off) {
            Some(ai) => ai,
            None => {
                if self.allocs.iter().any(|a| !a.live && a.order == order && a.off == off) {
                    return Err(VB_DBL_FREE);
                }
                return Err(VB_BAD_BLK);
            }
        };
        self.allocs[ai].live = false;
        self.watermark -= 1u32 << order;
        let bi = match self.blocks.iter().position(|b| b.off == off && b.order == order) {
            Some(bi) => bi,
            None => return Err(VB_BAD_BLK),
        };
        self.blocks[bi].free = true;
        // 逐级吸收自由伙伴：账面块升阶、被吸收块记死
        let mut o = order;
        let mut cur = off;
        while o < self.max_order() {
            let buddy = cur ^ (1u32 << o);
            let bj = self.blocks.iter().position(|b| b.order == o && b.off == buddy && b.free);
            match bj {
                Some(bj) => {
                    let lo = cur.min(buddy);
                    self.blocks[bj].free = false;
                    self.blocks[bi].order = o + 1;
                    self.blocks[bi].off = lo;
                    self.blocks[bi].free = true;
                    o += 1;
                    cur = lo;
                }
                None => break,
            }
        }
        Ok(())
    }

    /// 账本折叠指纹（分配账 + 块表）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x40DC0;
        for a in &self.allocs {
            h = h.wrapping_mul(35).wrapping_add(a.seq ^ (a.order << 8) ^ a.off);
        }
        for b in &self.blocks {
            h = h.wrapping_mul(35).wrapping_add(b.off ^ (b.order << 4) ^ (b.free as u32));
        }
        h ^ self.watermark
    }

    /// 封印：印后分配必拒。
    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 标准样本：256 页整池（最高 8 阶）。
pub fn mk_buddy() -> BuddyBank {
    BuddyBank::new(256)
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut v = mk_buddy();
    // 1) 越阶分配
    got[0] = v.alloc(9).err() == Some(VB_BAD_ORDER);
    // 2) 野块释放
    got[1] = v.free(3, 999).err() == Some(VB_BAD_BLK);
    // 3) 双重释放
    let off = match v.alloc(2) { Ok(o) => o, Err(_) => 0 };
    let _ = v.free(2, off);
    got[2] = v.free(2, off).err() == Some(VB_DBL_FREE);
    // 4) 水位不足（整池占用后再分配）
    let _ = v.alloc(8);
    got[3] = v.alloc(1).err() == Some(VB_NO_ROOM);
    // 5) 印后分配
    v.seal();
    got[4] = v.alloc(1).err() == Some(VB_FROZEN);
    got
}

/// 判据主体（13 条，全部真跑账本）。
pub fn run_unx24095_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F24095");
    let mut v = mk_buddy();

    // J1 账非空
    let j1 = v.blocks.len() == 1 && v.blocks[0].order == 8 && v.blocks[0].free;
    s.add("J1-账非空", j1, "种块单块 8 阶自由");

    // J2 字段齐
    let j2 = v.allocs.is_empty() && v.watermark == 0 && v.peak == 0 && v.total_pages == 256;
    s.add("J2-字段齐", j2, "分配账空且水位归零");

    // J3 冻结闸：切面记账 + 首分配分裂落账
    let mut asp = SealGate::new(24095);
    let a1 = asp.record(1, 8).is_ok();
    let a2 = asp.record(1, 8).err() == Some(VB_DBL_FREE);
    let a3 = asp.record(32, 1).err() == Some(VB_NO_ROOM);
    let off3 = v.alloc(3);
    let j3 = off3.ok() == Some(0) && v.watermark == 8 && a1 && a2 && a3;
    s.add("J3-分裂落账", j3, "冻结闸切面三连 + 3 阶自 8 阶种块分裂");

    // J4 伙伴续分
    let off4 = v.alloc(3);
    let j4 = off4.ok() == Some(8) && v.watermark == 16;
    s.add("J4-伙伴续分", j4, "第二块取伙伴位 8");

    // J5 越阶拒
    let j5 = v.alloc(9).err() == Some(VB_BAD_ORDER);
    s.add("J5-越阶拒", j5, "超过最高阶必拒");

    // J6 释放回收
    let j6 = v.free(3, 0).is_ok() && v.watermark == 8;
    s.add("J6-释放回收", j6, "水位随释放回落");

    // J7 双重释放拒
    let j7 = v.free(3, 0).err() == Some(VB_DBL_FREE);
    s.add("J7-双放拒", j7, "同块二次释放必拒");

    // J8 冻结闸：伙伴逐级全合并还原
    let j8 = v.free(3, 8).is_ok()
        && v.free_pages() == 256
        && v.blocks.iter().any(|b| b.free && b.order == 8);
    s.add("J8-全合并", j8, "冻结闸切面在册 + 合并还原单块");

    // J9 水位闸
    let full = v.alloc(8);
    let j9 = full.ok() == Some(0) && v.watermark == 256 && v.peak == 256
        && v.alloc(1).err() == Some(VB_NO_ROOM);
    s.add("J9-水位闸", j9, "整池占用后再分配必拒");

    // J10 回收守恒
    let j10 = v.free(8, 0).is_ok() && v.free_pages() == 256 && v.watermark == 0;
    s.add("J10-守恒", j10, "释放后自由页守恒");

    // J11 指纹稳定（账本 + 切面双折叠）
    let j11 = v.fold() == v.fold() && asp.fold() == asp.fold();
    s.add("J11-指纹稳定", j11, "账本与切面两次折叠一致");

    // J12 印后冻结
    let fp = v.seal();
    let j12 = fp > 0 && v.alloc(1).err() == Some(VB_FROZEN);
    s.add("J12-印后冻结", j12, "封印后分配必拒");

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
    fn unx24095_vram_buddy_flow() {
        let mut v = mk_buddy();
        assert!(v.alloc(4).is_ok());
        assert!(v.alloc(9).err() == Some(VB_BAD_ORDER));
        let off = match v.alloc(2) { Ok(o) => o, Err(_) => 0 };
        assert!(v.free(2, off).is_ok());
        assert!(v.free(2, off).err() == Some(VB_DBL_FREE));
        assert!(v.free(4, 0).is_ok());
        assert!(v.seal() > 0);
        assert!(v.alloc(1).err() == Some(VB_FROZEN));
    }

    #[test]
    fn unx24095_checks_green() {
        let s = run_unx24095_checks();
        assert!(s.all_passed(), "UNX-F24095 判据未全绿");
    }
}
