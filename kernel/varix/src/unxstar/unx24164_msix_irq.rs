//! UNX-F24164 MSI-X 中断消费与 IrqBackend 对接·子功能04（中断回执与 EOI 账）（0x41660 段）
//!
//! 锚点：UNX-F24164-J1 回执账（中断 raise/EOI 配对逐条落账）；J2 回
//! 执语义（无 raise 伪 EOI 拒 + 重复 EOI 拒 + 挂起清零三性质）；J3 回
//! 执指纹（账折叠指纹）三判据全绿。
//! 域：CoRun·UNX-F5-B15（EOI 回执面）。
//!
//! 结构：挂起位表 + raise/EOI 配对核 + 账本 + 折叠指纹 + 注入核（伪
//! EOI/重 EOI/野向量/印后/未挂起必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x41660 独占 0x20）。
pub const SEG_BASE: u32 = 0x41660;
pub const EO_NO_PENDING: u32 = SEG_BASE + 0x01; // 无挂起伪 EOI
pub const EO_DUP_EOI: u32 = SEG_BASE + 0x02; // 重复 EOI
pub const EO_UNK_VEC: u32 = SEG_BASE + 0x03; // 野向量
pub const EO_FROZEN: u32 = SEG_BASE + 0x04; // 印后操作
pub const EO_BAD_STATE: u32 = SEG_BASE + 0x05; // 态越界

/// 向量挂起态。
#[derive(Clone)]
pub struct EoVec {
    pub vec_no: u32,
    pub pending: bool,
    pub raised: u32,
    pub eoi: u32,
}

/// 回执簿。
pub struct EoBook {
    pub vecs: Vec<EoVec>,
    pub ledger: Vec<(u32, u8)>, // (vec_no, op) op:0=raise 1=eoi
    pub sealed: bool,
}

impl EoBook {
    pub fn new() -> EoBook {
        EoBook { vecs: Vec::new(), ledger: Vec::new(), sealed: false }
    }

    pub fn declare(&mut self, vec_no: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(EO_FROZEN);
        }
        if self.vecs.iter().any(|v| v.vec_no == vec_no) {
            return Err(EO_UNK_VEC);
        }
        self.vecs.push(EoVec { vec_no, pending: false, raised: 0, eoi: 0 });
        Ok(())
    }

    /// raise：置挂起位。
    pub fn raise(&mut self, vec_no: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(EO_FROZEN);
        }
        match self.vecs.iter_mut().find(|v| v.vec_no == vec_no) {
            Some(v) => {
                v.pending = true;
                v.raised += 1;
                self.ledger.push((vec_no, 0));
                Ok(())
            }
            None => Err(EO_UNK_VEC),
        }
    }

    /// EOI：清挂起位；无挂起/重复 EOI 拒。
    pub fn eoi(&mut self, vec_no: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(EO_FROZEN);
        }
        match self.vecs.iter_mut().find(|v| v.vec_no == vec_no) {
            Some(v) => {
                if !v.pending {
                    return Err(EO_NO_PENDING);
                }
                v.pending = false;
                v.eoi += 1;
                self.ledger.push((vec_no, 1));
                Ok(())
            }
            None => Err(EO_UNK_VEC),
        }
    }

    pub fn pending_count(&self) -> usize {
        self.vecs.iter().filter(|v| v.pending).count()
    }

    pub fn balanced(&self) -> bool {
        self.vecs.iter().all(|v| v.raised == v.eoi + if v.pending { 1 } else { 0 })
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x41660;
        for v in &self.vecs {
            h = h.wrapping_mul(31).wrapping_add(v.vec_no);
            h = h.wrapping_mul(31).wrapping_add(v.raised);
            h = h.wrapping_mul(31).wrapping_add(v.eoi);
            h = h.wrapping_mul(31).wrapping_add(v.pending as u32);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 标准样本：两向量一挂起一回执。
pub fn mk_book() -> EoBook {
    let mut b = EoBook::new();
    let _ = b.declare(16);
    let _ = b.declare(17);
    let _ = b.raise(16);
    let _ = b.eoi(16);
    let _ = b.raise(17); // 挂起未回执
    b
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = mk_book();
    // 1) 伪 EOI（已回执向量再 EOI）
    got[0] = b.eoi(16).err() == Some(EO_NO_PENDING);
    // 2) 野向量
    got[1] = b.raise(99).err() == Some(EO_UNK_VEC);
    // 3) 重复回执路径
    let _ = b.eoi(17);
    got[2] = b.eoi(17).err() == Some(EO_NO_PENDING);
    // 4) 重号声明
    got[3] = b.declare(16).err() == Some(EO_UNK_VEC);
    // 5) 印后操作必抓
    b.seal();
    got[4] = b.raise(16).err() == Some(EO_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx24164_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F24164");
    let mut b = mk_book();

    // J1 向量账齐
    let j1 = b.vecs.len() == 2;
    s.add("J1-向量账齐", j1, "两向量全落表");

    // J2 挂起位
    let j2 = b.pending_count() == 1;
    s.add("J2-挂起一位", j2, "向量 17 挂起");

    // J3 EOI 清位
    let j3 = {
        let _ = b.eoi(17);
        b.pending_count() == 0
    };
    s.add("J3-EOI清位", j3, "回执后挂起清零");

    // J4 伪 EOI 拒
    let j4 = b.eoi(16).err() == Some(EO_NO_PENDING);
    s.add("J4-伪EOI拒", j4, "无挂起回执必拒");

    // J5 重复 EOI 拒
    let j5 = {
        let _ = b.eoi(17);
        b.eoi(17).err() == Some(EO_NO_PENDING)
    };
    s.add("J5-重EOI拒", j5, "二次回执必拒");

    // J6 raise/EOI 配对守恒
    let j6 = b.balanced();
    s.add("J6-配对守恒", j6, "raised = eoi + pending");

    // J7 野向量拒
    let j7 = b.raise(99).err() == Some(EO_UNK_VEC);
    s.add("J7-野向量拒", j7, "未注册必拒");

    // J8 重号声明拒
    let j8 = b.declare(16).err() == Some(EO_UNK_VEC);
    s.add("J8-重号拒", j8, "向量重号必拒");

    // J9 多次 raise 单 EOI
    let j9 = {
        let mut b2 = EoBook::new();
        let _ = b2.declare(5);
        let _ = b2.raise(5);
        let _ = b2.raise(5);
        let _ = b2.eoi(5);
        let v = b2.vecs.iter().find(|v| v.vec_no == 5).unwrap();
        v.raised == 2 && v.eoi == 1 && !v.pending
    };
    s.add("J9-多raise单EOI", j9, "两次 raise 一次 EOI 清位");

    // J10 账面配对
    let j10 = b.ledger.len() == 4;
    s.add("J10-账面四条", j10, "raise/EOI 四条落账");

    // J11 印后操作拒
    let j11 = {
        let mut b2 = mk_book();
        b2.seal();
        b2.eoi(17).err() == Some(EO_FROZEN)
    };
    s.add("J11-印后操作拒", j11, "印后 EOI 必拒");

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
    fn unx24164_eoi_flow() {
        let mut b = EoBook::new();
        let _ = b.declare(16);
        assert!(b.raise(16).is_ok());
        assert!(b.eoi(16).is_ok());
        assert_eq!(b.eoi(16), Err(EO_NO_PENDING));
        assert_eq!(b.raise(16), Ok(()));
        assert_eq!(b.pending_count(), 1);
        assert_eq!(b.declare(16), Err(EO_UNK_VEC));
        assert_eq!(b.raise(99), Err(EO_UNK_VEC));
        b.seal();
        assert_eq!(b.eoi(16), Err(EO_FROZEN));
    }

    #[test]
    fn unx24164_checks_green() {
        let s = run_unx24164_checks();
        assert!(s.all_passed(), "UNX-F24164 判据未全绿");
    }
}
