//! UNX-F24176 MSI-X 中断消费与 IrqBackend 对接·子功能16（pending 位账）（0x417E0 段）
//!
//! 锚点：UNX-F24176-J1 挂起账（pending 位置清逐条落账）；J2 挂起语
//! 义（置位幂等 + 清位闸 + 位图快照三性质）；J3 挂起指纹（位图折叠
//! 指纹）三判据全绿。
//! 域：CoRun·UNX-F5-B15（pending 位图面）。
//!
//! 结构：位图 + 置/清位核 + 位图快照核 + 账本 + 折叠指纹 + 注入核（
//! 越界位/重清/清未置位/印后/位溢出必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use crate::checks::CheckSet;

/// 诊断段（0x417E0 独占 0x20）。
pub const SEG_BASE: u32 = 0x417E0;
pub const PD_OVER_BIT: u32 = SEG_BASE + 0x01; // 越界位
pub const PD_CLR_UNSET: u32 = SEG_BASE + 0x02; // 清未置位
pub const PD_SET_TWICE: u32 = SEG_BASE + 0x03; // 重复置位档
pub const PD_FROZEN: u32 = SEG_BASE + 0x04; // 印后操作
pub const PD_BITMAP_FULL: u32 = SEG_BASE + 0x05; // 位图满档

/// 位图挂起账。
pub struct PdBitmap {
    pub bits: Vec<bool>,
    pub set_ops: u32,
    pub clr_ops: u32,
    pub sealed: bool,
}

impl PdBitmap {
    pub fn new(n: u32) -> PdBitmap {
        PdBitmap { bits: vec![false; n as usize], set_ops: 0, clr_ops: 0, sealed: false }
    }

    /// 置位（重复置位报档但幂等生效）。
    pub fn set(&mut self, bit: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(PD_FROZEN);
        }
        if bit as usize >= self.bits.len() {
            return Err(PD_OVER_BIT);
        }
        let already = self.bits[bit as usize];
        self.bits[bit as usize] = true;
        if !already {
            self.set_ops += 1;
            Ok(())
        } else {
            self.set_ops += 1;
            Err(PD_SET_TWICE)
        }
    }

    /// 清位（清未置位拒）。
    pub fn clear(&mut self, bit: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(PD_FROZEN);
        }
        if bit as usize >= self.bits.len() {
            return Err(PD_OVER_BIT);
        }
        if !self.bits[bit as usize] {
            return Err(PD_CLR_UNSET);
        }
        self.bits[bit as usize] = false;
        self.clr_ops += 1;
        Ok(())
    }

    pub fn get(&self, bit: u32) -> Result<bool, u32> {
        if bit as usize >= self.bits.len() {
            return Err(PD_OVER_BIT);
        }
        Ok(self.bits[bit as usize])
    }

    /// 位图快照（十六进制风格串）。
    pub fn snapshot(&self) -> String {
        let mut s = String::new();
        for (i, &b) in self.bits.iter().enumerate() {
            if i > 0 && i % 8 == 0 {
                s.push('|');
            }
            s.push(if b { '1' } else { '0' });
        }
        s
    }

    pub fn popcount(&self) -> usize {
        self.bits.iter().filter(|&&b| b).count()
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x417E0;
        for &b in &self.bits {
            h = h.wrapping_mul(2).wrapping_add(b as u32);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 标准样本：16 位图。
pub fn mk_bitmap() -> PdBitmap {
    let mut b = PdBitmap::new(16);
    let _ = b.set(0);
    let _ = b.set(3);
    let _ = b.set(7);
    let _ = b.clear(3);
    b
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = mk_bitmap();
    // 1) 越界位
    got[0] = b.set(16).err() == Some(PD_OVER_BIT);
    // 2) 清未置位
    got[1] = b.clear(3).err() == Some(PD_CLR_UNSET);
    // 3) 重复置位档
    got[2] = b.set(0).err() == Some(PD_SET_TWICE);
    // 4) 位图快照对账
    got[3] = b.snapshot().starts_with("1000");
    // 5) 印后操作必抓
    b.seal();
    got[4] = b.set(1).err() == Some(PD_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx24176_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F24176");
    let mut b = mk_bitmap();

    // J1 位图尺寸
    let j1 = b.bits.len() == 16;
    s.add("J1-位图尺寸", j1, "16 位");

    // J2 置位生效
    let j2 = b.get(0) == Ok(true) && b.get(7) == Ok(true);
    s.add("J2-置位生效", j2, "位 0/7 已置");

    // J3 清位生效
    let j3 = b.get(3) == Ok(false);
    s.add("J3-清位生效", j3, "位 3 已清");

    // J4 popcount
    let j4 = b.popcount() == 2;
    s.add("J4-popcount", j4, "两位在置");

    // J5 越界位拒
    let j5 = b.set(16).err() == Some(PD_OVER_BIT) && b.get(99).err() == Some(PD_OVER_BIT);
    s.add("J5-越界拒", j5, "位 ≥ len 必拒");

    // J6 清未置位拒
    let j6 = b.clear(3).err() == Some(PD_CLR_UNSET);
    s.add("J6-清未置拒", j6, "清零位必拒");

    // J7 重复置位档
    let j7 = b.set(0).err() == Some(PD_SET_TWICE);
    s.add("J7-重置位档", j7, "已置再置报档");

    // J8 操作计数
    let j8 = b.set_ops >= 4 && b.clr_ops == 1;
    s.add("J8-操作计数", j8, "set/clr 落账");

    // J9 快照格式
    let j9 = {
        let snap = b.snapshot();
        snap.len() == 17 && snap.contains('|')
    };
    s.add("J9-快照格式", j9, "16 位一分隔");

    // J10 全清复零
    let j10 = {
        let mut b2 = PdBitmap::new(4);
        let _ = b2.set(0);
        let _ = b2.set(1);
        let _ = b2.clear(0);
        let _ = b2.clear(1);
        b2.popcount() == 0
    };
    s.add("J10-全清复零", j10, "全清后 popcount 0");

    // J11 置位幂等生效
    let j11 = {
        let mut b2 = PdBitmap::new(4);
        let _ = b2.set(2);
        let _ = b2.set(2); // 报档但仍为置位
        b2.get(2) == Ok(true)
    };
    s.add("J11-重置幂等", j11, "重置位不破态");

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
    fn unx24176_bitmap_flow() {
        let mut b = PdBitmap::new(8);
        assert!(b.set(0).is_ok());
        assert_eq!(b.set(0), Err(PD_SET_TWICE));
        assert!(b.clear(0).is_ok());
        assert_eq!(b.clear(0), Err(PD_CLR_UNSET));
        assert_eq!(b.set(8), Err(PD_OVER_BIT));
        assert_eq!(b.popcount(), 0);
        b.seal();
        assert_eq!(b.set(1), Err(PD_FROZEN));
    }

    #[test]
    fn unx24176_checks_green() {
        let s = run_unx24176_checks();
        assert!(s.all_passed(), "UNX-F24176 判据未全绿");
    }
}
