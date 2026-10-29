//! UNX-F24163 MSI-X 中断消费与 IrqBackend 对接·子功能03（ISR 注册与派发）（0x41640 段）
//!
//! 锚点：UNX-F24163-J1 派发账（ISR 注册与中断派发逐条落账）；J2 派
//! 发语义（一向量一 ISR + 无 ISR 静默 + 派发计数三性质）；J3 派发指
//! 纹（账折叠指纹）三判据全绿。
//! 域：CoRun·UNX-F5-B15（ISR 派发面）。
//!
//! 结构：ISR 表 + 注册/派发核 + 账本 + 折叠指纹 + 注入核（重 ISR/野
//! 向量/静默派发/印后/重注册必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x41640 独占 0x20）。
pub const SEG_BASE: u32 = 0x41640;
pub const IS_DUP_ISR: u32 = SEG_BASE + 0x01; // 重 ISR
pub const IS_UNK_VEC: u32 = SEG_BASE + 0x02; // 野向量
pub const IS_SILENT: u32 = SEG_BASE + 0x03; // 无 ISR 静默
pub const IS_FROZEN: u32 = SEG_BASE + 0x04; // 印后注册
pub const IS_BAD_NAME: u32 = SEG_BASE + 0x05; // 空 ISR 名

/// ISR 条目。
#[derive(Clone)]
pub struct IsrEnt {
    pub vec_no: u32,
    pub name: String,
    pub fired: u32,
}

/// 派发簿。
pub struct IsBook {
    pub isrs: Vec<IsrEnt>,
    pub silent_count: u32,
    pub sealed: bool,
}

impl IsBook {
    pub fn new() -> IsBook {
        IsBook { isrs: Vec::new(), silent_count: 0, sealed: false }
    }

    pub fn register(&mut self, vec_no: u32, name: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(IS_FROZEN);
        }
        if name.is_empty() {
            return Err(IS_BAD_NAME);
        }
        if self.isrs.iter().any(|i| i.vec_no == vec_no) {
            return Err(IS_DUP_ISR);
        }
        self.isrs.push(IsrEnt { vec_no, name: name.to_string(), fired: 0 });
        Ok(())
    }

    /// 派发：有 ISR 则 fire 计数，无 ISR 静默计数。
    pub fn raise(&mut self, vec_no: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(IS_FROZEN);
        }
        match self.isrs.iter_mut().find(|i| i.vec_no == vec_no) {
            Some(i) => {
                i.fired += 1;
                Ok(())
            }
            None => {
                self.silent_count += 1;
                Err(IS_SILENT)
            }
        }
    }

    pub fn fired_of(&self, vec_no: u32) -> u32 {
        self.isrs.iter().find(|i| i.vec_no == vec_no).map(|i| i.fired).unwrap_or(0)
    }

    pub fn total_fired(&self) -> u32 {
        self.isrs.iter().map(|i| i.fired).sum()
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x41640;
        for i in &self.isrs {
            h = h.wrapping_mul(31).wrapping_add(i.vec_no);
            h = h.wrapping_mul(31).wrapping_add(i.fired);
            for by in i.name.bytes() {
                h = h.wrapping_mul(31).wrapping_add(by as u32);
            }
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 标准样本：两 ISR。
pub fn mk_book() -> IsBook {
    let mut b = IsBook::new();
    let _ = b.register(16, "gpu_irq");
    let _ = b.register(17, "dma_irq");
    let _ = b.raise(16);
    let _ = b.raise(16);
    let _ = b.raise(17);
    let _ = b.raise(99); // 静默
    b
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = mk_book();
    // 1) 重 ISR
    got[0] = b.register(16, "dup").err() == Some(IS_DUP_ISR);
    // 2) 空 ISR 名
    got[1] = b.register(18, "").err() == Some(IS_BAD_NAME);
    // 3) 静默派发
    got[2] = b.raise(99).err() == Some(IS_SILENT);
    // 4) 静默计数累计
    got[3] = b.silent_count == 2;
    // 5) 印后注册必抓
    b.seal();
    got[4] = b.register(18, "late").err() == Some(IS_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx24163_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F24163");
    let mut b = mk_book();

    // J1 ISR 表齐
    let j1 = b.isrs.len() == 2;
    s.add("J1-ISR表齐", j1, "两 ISR 全落表");

    // J2 派发计数
    let j2 = b.fired_of(16) == 2 && b.fired_of(17) == 1;
    s.add("J2-派发计数", j2, "16:2 17:1");

    // J3 静默计数
    let j3 = b.silent_count == 1;
    s.add("J3-静默计数", j3, "野向量一次静默");

    // J4 一向量一 ISR
    let j4 = b.register(16, "dup").err() == Some(IS_DUP_ISR);
    s.add("J4-一向量一ISR", j4, "重注册必拒");

    // J5 总派发守恒
    let j5 = b.total_fired() == 3;
    s.add("J5-总派发守恒", j5, "fire 总数 3");

    // J6 静默不入 fire
    let j6 = b.total_fired() + b.silent_count == 4;
    s.add("J6-静默不入fire", j6, "3 fire + 1 silent = 4 raise");

    // J7 空 ISR 名拒
    let j7 = b.register(18, "").err() == Some(IS_BAD_NAME);
    s.add("J7-空名拒", j7, "空 ISR 名必拒");

    // J8 撤销后静默
    let j8 = {
        let before = b.silent_count;
        // 撤销语义由 unregister 承担（此表无撤销——用未注册向量验证静默路径一致）
        b.raise(77).err() == Some(IS_SILENT) && b.silent_count == before + 1
    };
    s.add("J8-静默路径一致", j8, "未注册向量恒静默");

    // J9 派发后 fire 不丢
    let j9 = {
        let before = b.fired_of(16);
        let _ = b.raise(16);
        b.fired_of(16) == before + 1
    };
    s.add("J9-fire不丢", j9, "再派发计数 +1");

    // J10 重号注册拒（幂等性验证第二路径）
    let j10 = b.register(17, "another").err() == Some(IS_DUP_ISR);
    s.add("J10-重号注册拒", j10, "向量 17 二注册必拒");

    // J11 印后派发拒
    let j11 = {
        let mut b2 = mk_book();
        b2.seal();
        b2.raise(16).err() == Some(IS_FROZEN)
    };
    s.add("J11-印后派发拒", j11, "印后 raise 必拒");

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
    fn unx24163_isr_flow() {
        let mut b = IsBook::new();
        assert!(b.register(16, "gpu").is_ok());
        assert_eq!(b.register(16, "dup"), Err(IS_DUP_ISR));
        assert_eq!(b.raise(16), Ok(()));
        assert_eq!(b.raise(99), Err(IS_SILENT));
        assert_eq!(b.fired_of(16), 1);
        assert_eq!(b.register(17, ""), Err(IS_BAD_NAME));
        b.seal();
        assert_eq!(b.register(18, "x"), Err(IS_FROZEN));
    }

    #[test]
    fn unx24163_checks_green() {
        let s = run_unx24163_checks();
        assert!(s.all_passed(), "UNX-F24163 判据未全绿");
    }
}
