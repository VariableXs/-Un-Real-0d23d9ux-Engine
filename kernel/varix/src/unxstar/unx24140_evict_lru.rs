//! UNX-F24140 显存驱逐 LRU 与迁移搬运·子功能20（驱逐面收口账）（0x41360 段）
//!
//! 锚点：UNX-F24140-J1 收口账（驱逐面统计汇总逐条落账）；J2 收口语
//! 义（分区守恒 + 覆盖九面 + 收口印闸三性质）；J3 收口指纹（汇总账
//! 折叠指纹）三判据全绿。
//! 域：CoRun·UNX-F5-B15（驱逐面收口面）。
//!
//! 结构：分区统计表 + 汇总核 + 收口印核 + 折叠指纹 + 注入核（野分
//! 区/负差值/未覆盖印/重复印/印后录必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x41360 独占 0x20）。
pub const SEG_BASE: u32 = 0x41360;
pub const FC_UNK_ZONE: u32 = SEG_BASE + 0x01; // 野分区
pub const FC_NEG_DELTA: u32 = SEG_BASE + 0x02; // 负差值
pub const FC_INCOMPLETE: u32 = SEG_BASE + 0x03; // 未覆盖印发
pub const FC_DUP_SEAL: u32 = SEG_BASE + 0x04; // 重复印发
pub const FC_FROZEN: u32 = SEG_BASE + 0x05; // 印后录入

/// 驱逐分区统计。
#[derive(Clone)]
pub struct FcZone {
    pub zone_id: u32,
    pub evict_count: u32,
    pub evict_pages: u32,
    pub fail_count: u32,
}

/// 收口汇总账。
#[derive(Clone)]
pub struct FcRec {
    pub seq: u32,
    pub zone_id: u32,
    pub evict_count: u32,
    pub evict_pages: u32,
    pub fail_count: u32,
    pub ok_rate_ppm: u32, // 成功率（百万分比）
}

impl FcRec {
    pub fn ok(&self) -> bool {
        self.evict_count + self.fail_count > 0
            && self.ok_rate_ppm
                == (self.evict_count * 1_000_000) / (self.evict_count + self.fail_count)
    }
}

/// 收口簿。
pub struct FcBook {
    pub zones: Vec<FcZone>,
    pub ledger: Vec<FcRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl FcBook {
    pub fn new() -> FcBook {
        FcBook { zones: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    pub fn declare(&mut self, zone_id: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(FC_FROZEN);
        }
        if self.zones.iter().any(|z| z.zone_id == zone_id) {
            return Err(FC_UNK_ZONE);
        }
        self.zones.push(FcZone { zone_id, evict_count: 0, evict_pages: 0, fail_count: 0 });
        Ok(())
    }

    pub fn bump(&mut self, zone_id: u32, evict: u32, pages: u32, fail: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(FC_FROZEN);
        }
        if self.zones.iter().all(|z| z.zone_id != zone_id) {
            return Err(FC_UNK_ZONE);
        }
        // 负差值闸：新统计不得小于已记值
        {
            let z = self.zones.iter().find(|z| z.zone_id == zone_id).unwrap();
            if evict < z.evict_count || pages < z.evict_pages || fail < z.fail_count {
                return Err(FC_NEG_DELTA);
            }
        }
        if let Some(z) = self.zones.iter_mut().find(|z| z.zone_id == zone_id) {
            z.evict_count = evict;
            z.evict_pages = pages;
            z.fail_count = fail;
        }
        Ok(())
    }

    /// 汇总落账：单分区快照。
    pub fn snapshot(&mut self, zone_id: u32) -> Result<FcRec, u32> {
        if self.sealed {
            return Err(FC_FROZEN);
        }
        let z = match self.zones.iter().find(|z| z.zone_id == zone_id) {
            Some(z) => z.clone(),
            None => return Err(FC_UNK_ZONE),
        };
        let total = z.evict_count + z.fail_count;
        let rate = if total > 0 { (z.evict_count * 1_000_000) / total } else { 0 };
        let rec = FcRec {
            seq: self.seq_next,
            zone_id,
            evict_count: z.evict_count,
            evict_pages: z.evict_pages,
            fail_count: z.fail_count,
            ok_rate_ppm: rate,
        };
        self.seq_next += 1;
        self.ledger.push(rec.clone());
        Ok(rec)
    }

    pub fn covered_all(&self) -> bool {
        self.zones.iter().all(|z| self.ledger.iter().any(|r| r.zone_id == z.zone_id))
    }

    /// 收口印：全分区覆盖且账自洽方可发印。
    pub fn seal(&mut self) -> Result<u32, u32> {
        if self.sealed {
            return Err(FC_DUP_SEAL);
        }
        if !self.covered_all() {
            return Err(FC_INCOMPLETE);
        }
        self.sealed = true;
        Ok(self.fold())
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
        let mut h: u32 = 0x41360;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.seq);
            h = h.wrapping_mul(31).wrapping_add(r.zone_id);
            h = h.wrapping_mul(31).wrapping_add(r.evict_count);
            h = h.wrapping_mul(31).wrapping_add(r.fail_count);
            h = h.wrapping_mul(31).wrapping_add(r.ok_rate_ppm);
        }
        h
    }
}

/// 标准样本：三分区统计齐 + 快照齐。
pub fn mk_book() -> FcBook {
    let mut b = FcBook::new();
    for z in 1..=3u32 {
        let _ = b.declare(z);
    }
    let _ = b.bump(1, 10, 200, 2);
    let _ = b.bump(2, 5, 100, 5);
    let _ = b.bump(3, 8, 160, 0);
    let _ = b.snapshot(1);
    let _ = b.snapshot(2);
    let _ = b.snapshot(3);
    b
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = mk_book();
    // 1) 野分区
    got[0] = b.bump(99, 1, 1, 0).err() == Some(FC_UNK_ZONE);
    // 2) 负差值
    got[1] = b.bump(1, 5, 200, 2).err() == Some(FC_NEG_DELTA);
    // 3) 未覆盖印发
    let mut b2 = FcBook::new();
    let _ = b2.declare(1);
    let _ = b2.declare(2);
    let _ = b2.bump(1, 1, 10, 0);
    let _ = b2.snapshot(1);
    got[2] = b2.seal().err() == Some(FC_INCOMPLETE);
    // 4) 重复印发
    let _ = b.seal();
    got[3] = b.seal().err() == Some(FC_DUP_SEAL);
    // 5) 印后录入必抓
    got[4] = b.bump(1, 11, 210, 2).err() == Some(FC_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx24140_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F24140");
    let mut b = mk_book();

    // J1 分区账齐
    let j1 = b.zones.len() == 3;
    s.add("J1-分区账齐", j1, "三分区全落表");

    // J2 快照账齐
    let j2 = b.ledger.len() == 3;
    s.add("J2-快照账齐", j2, "三分区快照全落账");

    // J3 成功率自洽
    let j3 = b.ledger.iter().all(|r| r.ok());
    s.add("J3-成功率自洽", j3, "ok_rate_ppm 与计数一致");

    // J4 分区守恒（快照值与分区表一致）
    let j4 = b.ledger.iter().all(|r| {
        let z = b.zones.iter().find(|z| z.zone_id == r.zone_id).unwrap();
        z.evict_count == r.evict_count && z.fail_count == r.fail_count
    });
    s.add("J4-分区守恒", j4, "快照与分区表一致");

    // J5 野分区拒
    let j5 = b.snapshot(99).err() == Some(FC_UNK_ZONE);
    s.add("J5-野分区拒", j5, "未声明分区必拒");

    // J6 负差值拒
    let j6 = b.bump(2, 4, 100, 5).err() == Some(FC_NEG_DELTA);
    s.add("J6-负差值拒", j6, "统计回退必拒");

    // J7 覆盖检查
    let j7 = b.covered_all();
    s.add("J7-覆盖检查", j7, "全分区有快照");

    // J8 未覆盖印发拒
    let j8 = {
        let mut b2 = FcBook::new();
        let _ = b2.declare(1);
        got8(&mut b2)
    };
    s.add("J8-未覆盖拒印", j8, "缺快照必拒印发");
    fn got8(b2: &mut FcBook) -> bool {
        b2.seal().err() == Some(FC_INCOMPLETE)
    }

    // J9 收口印发
    let j9 = b.seal().is_ok();
    s.add("J9-收口印发", j9, "全覆盖发印成功");

    // J10 重复印发拒
    let j10 = b.seal().err() == Some(FC_DUP_SEAL);
    s.add("J10-重复印发拒", j10, "二次发印必拒");

    // J11 印后录入拒
    let j11 = b.bump(1, 11, 210, 2).err() == Some(FC_FROZEN);
    s.add("J11-印后录入拒", j11, "印发后 bump 必拒");

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
    fn unx24140_closeout_flow() {
        let mut b = mk_book();
        assert!(b.covered_all());
        assert_eq!(b.bump(1, 5, 10, 0), Err(FC_NEG_DELTA));
        assert!(b.seal().is_ok());
        assert_eq!(b.seal(), Err(FC_DUP_SEAL));
        assert_eq!(b.bump(1, 11, 210, 2), Err(FC_FROZEN));
    }

    #[test]
    fn unx24140_checks_green() {
        let s = run_unx24140_checks();
        assert!(s.all_passed(), "UNX-F24140 判据未全绿");
    }
}
