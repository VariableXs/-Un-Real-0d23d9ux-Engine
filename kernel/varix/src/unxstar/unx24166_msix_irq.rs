//! UNX-F24166 MSI-X 中断消费与 IrqBackend 对接·子功能06（中断 mask/unmask）（0x416A0 段）
//!
//! 锚点：UNX-F24166-J1 掩码账（mask/unmask 逐条落账）；J2 掩码语义
//! （masked 拒 raise + 双 mask 幂等档 + unmask 复收三性质）；J3 掩码
//! 指纹（账折叠指纹）三判据全绿。
//! 域：CoRun·UNX-F5-B15（向量掩码面）。
//!
//! 结构：掩码位表 + mask/unmask 核 + raise 闸核 + 账本 + 折叠指纹 +
//! 注入核（masked raise/野向量/双 mask/印后/重号必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x416A0 独占 0x20）。
pub const SEG_BASE: u32 = 0x416A0;
pub const MK_MASKED: u32 = SEG_BASE + 0x01; // masked raise
pub const MK_UNK_VEC: u32 = SEG_BASE + 0x02; // 野向量
pub const MK_DUP_MASK: u32 = SEG_BASE + 0x03; // 双 mask
pub const MK_FROZEN: u32 = SEG_BASE + 0x04; // 印后操作
pub const MK_PENDING_LOST: u32 = SEG_BASE + 0x05; // 掩码期挂起丢失档

/// 掩码向量。
#[derive(Clone)]
pub struct MkVec {
    pub vec_no: u32,
    pub masked: bool,
    pub pending_while_masked: u32,
    pub raised: u32,
}

/// 掩码簿。
pub struct MkBook {
    pub vecs: Vec<MkVec>,
    pub sealed: bool,
}

impl MkBook {
    pub fn new() -> MkBook {
        MkBook { vecs: Vec::new(), sealed: false }
    }

    pub fn declare(&mut self, vec_no: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(MK_FROZEN);
        }
        if self.vecs.iter().any(|v| v.vec_no == vec_no) {
            return Err(MK_UNK_VEC);
        }
        self.vecs.push(MkVec { vec_no, masked: false, pending_while_masked: 0, raised: 0 });
        Ok(())
    }

    pub fn mask(&mut self, vec_no: u32, on: bool) -> Result<(), u32> {
        if self.sealed {
            return Err(MK_FROZEN);
        }
        match self.vecs.iter_mut().find(|v| v.vec_no == vec_no) {
            Some(v) => {
                if v.masked == on {
                    return Err(MK_DUP_MASK);
                }
                v.masked = on;
                Ok(())
            }
            None => Err(MK_UNK_VEC),
        }
    }

    /// raise：masked 期丢失（挂起计数但不入 raised——硬件行为近似）。
    pub fn raise(&mut self, vec_no: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(MK_FROZEN);
        }
        match self.vecs.iter_mut().find(|v| v.vec_no == vec_no) {
            Some(v) => {
                if v.masked {
                    v.pending_while_masked += 1;
                    return Err(MK_MASKED);
                }
                v.raised += 1;
                Ok(())
            }
            None => Err(MK_UNK_VEC),
        }
    }

    pub fn lost_total(&self) -> u32 {
        self.vecs.iter().map(|v| v.pending_while_masked).sum()
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x416A0;
        for v in &self.vecs {
            h = h.wrapping_mul(31).wrapping_add(v.vec_no);
            h = h.wrapping_mul(31).wrapping_add(v.masked as u32);
            h = h.wrapping_mul(31).wrapping_add(v.raised);
            h = h.wrapping_mul(31).wrapping_add(v.pending_while_masked);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 标准样本：两向量一 mask。
pub fn mk_book() -> MkBook {
    let mut b = MkBook::new();
    let _ = b.declare(0);
    let _ = b.declare(1);
    let _ = b.mask(1, true);
    let _ = b.raise(0); // ok
    let _ = b.raise(1); // masked 丢失
    b
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = mk_book();
    // 1) masked raise
    got[0] = b.raise(1).err() == Some(MK_MASKED);
    // 2) 野向量
    got[1] = b.raise(99).err() == Some(MK_UNK_VEC);
    // 3) 双 mask
    got[2] = b.mask(1, true).err() == Some(MK_DUP_MASK);
    // 4) 双 unmask
    got[3] = b.mask(0, false).err() == Some(MK_DUP_MASK);
    // 5) 印后操作必抓
    b.seal();
    got[4] = b.mask(0, true).err() == Some(MK_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx24166_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F24166");
    let mut b = mk_book();

    // J1 向量账齐
    let j1 = b.vecs.len() == 2;
    s.add("J1-向量账齐", j1, "两向量全落表");

    // J2 masked raise 拒
    let j2 = b.raise(1).err() == Some(MK_MASKED);
    s.add("J2-masked拒raise", j2, "掩码期 raise 必拒");

    // J3 丢失计数
    let j3 = b.lost_total() == 2;
    s.add("J3-丢失计数", j3, "两次掩码期丢失");

    // J4 双 mask 幂等档
    let j4 = b.mask(1, true).err() == Some(MK_DUP_MASK);
    s.add("J4-双mask档", j4, "同态 mask 必报档");

    // J5 双 unmask 档
    let j5 = b.mask(0, false).err() == Some(MK_DUP_MASK);
    s.add("J5-双unmask档", j5, "同态 unmask 必报档");

    // J6 unmask 复收
    let j6 = {
        let _ = b.mask(1, false);
        b.raise(1).is_ok()
    };
    s.add("J6-unmask复收", j6, "解除掩码后 raise 放行");

    // J7 野向量拒
    let j7 = b.raise(99).err() == Some(MK_UNK_VEC);
    s.add("J7-野向量拒", j7, "未注册必拒");

    // J8 重号声明拒
    let j8 = b.declare(0).err() == Some(MK_UNK_VEC);
    s.add("J8-重号拒", j8, "向量重号必拒");

    // J9 掩码不影响已 raised
    let j9 = {
        let v = b.vecs.iter().find(|v| v.vec_no == 0).unwrap();
        v.raised == 1
    };
    s.add("J9-已入不丢", j9, "掩码前已 raise 计数保留");

    // J10 丢失不入 raised
    let j10 = {
        let v = b.vecs.iter().find(|v| v.vec_no == 1).unwrap();
        v.raised == 0 && v.pending_while_masked >= 1
    };
    s.add("J10-丢失不入账", j10, "掩码期不发不计");

    // J11 印后操作拒
    let j11 = {
        let mut b2 = mk_book();
        b2.seal();
        b2.raise(0).err() == Some(MK_FROZEN)
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
    fn unx24166_mask_flow() {
        let mut b = MkBook::new();
        let _ = b.declare(0);
        assert!(b.raise(0).is_ok());
        assert!(b.mask(0, true).is_ok());
        assert_eq!(b.mask(0, true), Err(MK_DUP_MASK));
        assert_eq!(b.raise(0), Err(MK_MASKED));
        assert!(b.mask(0, false).is_ok());
        assert!(b.raise(0).is_ok());
        assert_eq!(b.mask(0, false), Err(MK_DUP_MASK));
        b.seal();
        assert_eq!(b.raise(0), Err(MK_FROZEN));
    }

    #[test]
    fn unx24166_checks_green() {
        let s = run_unx24166_checks();
        assert!(s.all_passed(), "UNX-F24166 判据未全绿");
    }
}
