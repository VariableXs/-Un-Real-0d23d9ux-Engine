//! UNX-F10342 wave-06 判据面汇总（C3 域全判据清点）（0x1b540 段）
//!
//! 锚点：UNX-F10342-J1 全判据清点——C3 域判据按批片 B31..B40 十批逐批
//! 清点计数，十批齐报后合计即域判据总量锚，宣称总量与合计不符即对不
//! 上账，十批全报且宣称对平才算汇总落账。
//! 域：CoRun·UNX-C3（联签/闭账/审计域）。
//!
//! 结构：批片账（片=批号 31..=40+计数）+ 合计核（宣称值与片合计比对）
//! + 缺批核（未报批可枚举）+ 封账闸（十批齐方可封账）+ 注入核（坏批
//! 号/重批/零计数/宣称不符四路必抓）。

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x1b540 独占 0x20）。
pub const SEG_BASE: u32 = 0x1b540;
pub const TO_BATCH: u32 = SEG_BASE + 0x01; // 批号越界
pub const TO_DUP: u32 = SEG_BASE + 0x02; // 批号重号
pub const TO_ZERO: u32 = SEG_BASE + 0x03; // 零计数
pub const TO_CAP: u32 = SEG_BASE + 0x04; // 账超容
pub const TO_MISMATCH: u32 = SEG_BASE + 0x05; // 宣称与合计不符
pub const TO_SEALED: u32 = SEG_BASE + 0x06; // 封账后报

/// 批号锚：B31..B40。
pub const BATCH_LO: u32 = 31;
pub const BATCH_HI: u32 = 40;
/// 批数锚（十批）。
pub const BATCHES_NEEDED: usize = 10;
/// 账定容。
pub const CAP: usize = 10;

/// 批片。
#[derive(Clone)]
pub struct BatchSlice {
    pub batch: u32,
    pub count: u32,
}

/// wave-06 判据汇总账。
pub struct CriteriaTot {
    pub slices: Vec<BatchSlice>,
    pub sealed: bool,
}

impl CriteriaTot {
    pub fn new() -> CriteriaTot {
        CriteriaTot { slices: Vec::new(), sealed: false }
    }

    fn pos(&self, batch: u32) -> Option<usize> {
        let mut k = 0usize;
        while k < self.slices.len() {
            if self.slices[k].batch == batch {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// 报一批片计数。
    pub fn register(&mut self, batch: u32, count: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(TO_SEALED);
        }
        if batch < BATCH_LO || batch > BATCH_HI {
            return Err(TO_BATCH);
        }
        if self.pos(batch).is_some() {
            return Err(TO_DUP);
        }
        if count == 0 {
            return Err(TO_ZERO);
        }
        if self.slices.len() >= CAP {
            return Err(TO_CAP);
        }
        self.slices.push(BatchSlice { batch, count });
        Ok(())
    }

    /// 片合计。
    pub fn total(&self) -> u32 {
        let mut t = 0u32;
        let mut k = 0usize;
        while k < self.slices.len() {
            t = t.wrapping_add(self.slices[k].count);
            k += 1;
        }
        t
    }

    /// 宣称总量核：与合计比对。
    pub fn verify_total(&self, claimed: u32) -> Result<u32, u32> {
        if claimed != self.total() {
            return Err(TO_MISMATCH);
        }
        Ok(claimed)
    }

    /// 未报批清单（升序）。
    pub fn missing_batches(&self) -> Vec<u32> {
        let mut v: Vec<u32> = Vec::new();
        let mut b = BATCH_LO;
        while b <= BATCH_HI {
            if self.pos(b).is_none() {
                v.push(b);
            }
            b += 1;
        }
        v
    }

    /// 汇总齐：十批全报。
    pub fn complete(&self) -> bool {
        self.slices.len() >= BATCHES_NEEDED && self.missing_batches().is_empty()
    }

    /// 封账：十批齐方可。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(TO_SEALED);
        }
        if !self.complete() {
            return Err(TO_BATCH);
        }
        self.sealed = true;
        Ok(())
    }

    /// 汇总账指纹。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x1b5400;
        h = h.wrapping_mul(31).wrapping_add(self.slices.len() as u32);
        let mut k = 0usize;
        while k < self.slices.len() {
            h = h
                .wrapping_mul(31)
                .wrapping_add(self.slices[k].batch)
                .wrapping_mul(31)
                .wrapping_add(self.slices[k].count);
            k += 1;
        }
        h = h.wrapping_mul(31).wrapping_add(self.sealed as u32);
        h
    }
}

/// 注入核：四路坏样本必抓。
pub fn inject_four() -> [bool; 4] {
    let mut got = [false; 4];
    let mut t = CriteriaTot::new();
    // 1) 坏批号必拒（30 与 41）
    got[0] = t.register(30, 5).err() == Some(TO_BATCH)
        && t.register(41, 5).err() == Some(TO_BATCH);
    // 2) 重批必拒
    let _ = t.register(31, 5);
    got[1] = t.register(31, 6).err() == Some(TO_DUP);
    // 3) 零计数必拒
    got[2] = t.register(32, 0).err() == Some(TO_ZERO);
    // 4) 宣称不符必拒
    let _ = t.register(33, 7);
    got[3] = t.verify_total(13).err() == Some(TO_MISMATCH)
        && t.verify_total(12).is_ok();
    got
}

/// C3 域十批样例计数（每批 5 条，合计 50）。
pub fn batch_counts() -> Vec<(u32, u32)> {
    let mut v: Vec<(u32, u32)> = Vec::new();
    let mut b = BATCH_LO;
    while b <= BATCH_HI {
        v.push((b, 5));
        b += 1;
    }
    v
}

/// 批内判据。
pub fn run_unx10342_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10342");

    // J1 正路径：十批全报、宣称对平、封账
    let mut t = CriteriaTot::new();
    let counts = batch_counts();
    let mut ok = true;
    let mut k = 0usize;
    while k < counts.len() {
        if t.register(counts[k].0, counts[k].1).is_err() {
            ok = false;
        }
        k += 1;
    }
    let vt = t.verify_total(50);
    let sealed = t.seal();
    s.add(
        "J1-全判据清点",
        ok && t.complete() && vt.is_ok() && sealed.is_ok(),
        "十批计数全报且宣称五十对平并封账",
    );

    // J2 合计对平
    s.add("J2-合计对平", t.total() == 50, "十批合计恰为五十条");

    // J3 注入检出：坏批号 + 重批
    let inj = inject_four();
    s.add("J3-批号重批检出", inj[0] && inj[1], "越界批号与重批必拒");

    // J4 注入检出：零计数 + 宣称不符
    s.add("J4-零数不符检出", inj[2] && inj[3], "零计数与宣称不符必拒");

    // J5 缺批枚举且不得封账
    let j5 = {
        let mut u = CriteriaTot::new();
        let mut b = BATCH_LO;
        while b <= BATCH_HI - 2u32 {
            let _ = u.register(b, 4);
            b += 1;
        }
        u.missing_batches() == [39u32, 40] && u.seal().err() == Some(TO_BATCH)
    };
    s.add("J5-缺批拒封", j5, "未报批可枚举且不得封账");

    // J6 宣称随报批推进
    let j6 = {
        let mut u = CriteriaTot::new();
        let _ = u.register(31, 3);
        let t0 = u.total();
        let _ = u.register(32, 4);
        let t1 = u.total();
        t0 == 3 && t1 == 7 && u.verify_total(7).is_ok()
    };
    s.add("J6-合计推进", j6, "合计随报批推进且宣称随时可核");

    // J7 封账后报批必拒
    let j7 = {
        let mut u = CriteriaTot::new();
        let counts2 = batch_counts();
        let mut q = 0usize;
        while q < counts2.len() {
            let _ = u.register(counts2[q].0, counts2[q].1);
            q += 1;
        }
        let _ = u.seal();
        u.register(31, 9).err() == Some(TO_SEALED)
    };
    s.add("J7-封账拒报", j7, "封账后再报必拒");

    // J8 计数为正核（零计数拒、大数允许）
    let j8 = {
        let mut u = CriteriaTot::new();
        u.register(31, u32::MAX - 1).is_ok()
    };
    s.add("J8-计数域核", j8, "计数为正即可入账不设上界");

    // J9 容量边界
    let j9 = {
        let mut u = CriteriaTot::new();
        let mut b = BATCH_LO;
        let mut filled = true;
        while b <= BATCH_HI {
            if u.register(b, 1).is_err() {
                filled = false;
            }
            b += 1;
        }
        // 十批已满（批域封闭），续报必拒
        filled && u.register(31, 1).err() == Some(TO_DUP)
    };
    s.add("J9-批域封闭", j9, "批域恰十批且重批必拒");

    // J10 指纹随报批推进且封账后稳定
    let j10 = {
        let mut u = CriteriaTot::new();
        let f0 = u.fold();
        let _ = u.register(31, 3);
        let f1 = u.fold();
        let _ = u.register(32, 4);
        let f2 = u.fold();
        f0 != f1 && f1 != f2 && u.fold() == f2
    };
    s.add("J10-指纹推进", j10, "每报一批指纹一变且终态稳定");

    // J11 未报批完整枚举（空账列全十批）
    let j11 = CriteriaTot::new().missing_batches().len() == BATCHES_NEEDED;
    s.add("J11-空账全缺", j11, "空账未报批恰为十");

    // J12 动态明细：合计落账
    let tot = t.total();
    s.add_dyn("J12-合计落账", tot == 50, &format!("total={}", tot));

    // J13 动态明细：汇总账指纹落账
    let fp = t.fold();
    s.add_dyn("J13-汇总指纹", fp != 0, &format!("fold={:#x}", fp));

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10342_criteria_tot_flow() {
        let mut t = CriteriaTot::new();
        assert!(t.register(30, 1).err() == Some(TO_BATCH));
        assert!(t.register(41, 1).err() == Some(TO_BATCH));
        assert!(t.register(31, 5).is_ok());
        assert!(t.register(31, 5).err() == Some(TO_DUP));
        assert!(t.register(32, 0).err() == Some(TO_ZERO));
        assert!(t.verify_total(5).is_ok());
        assert!(t.verify_total(6).err() == Some(TO_MISMATCH));
        assert!(t.missing_batches().len() == 9);
        let counts = batch_counts();
        let mut k = 1usize;
        while k < counts.len() {
            let _ = t.register(counts[k].0, counts[k].1);
            k += 1;
        }
        assert!(t.complete());
        assert!(t.verify_total(50).is_ok());
        assert!(t.seal().is_ok());
        assert!(t.register(35, 1).err() == Some(TO_SEALED));
    }

    #[test]
    fn unx10342_checks_green() {
        let s = run_unx10342_checks();
        assert!(s.all_passed(), "UNX-F10342 判据未全绿");
    }
}
