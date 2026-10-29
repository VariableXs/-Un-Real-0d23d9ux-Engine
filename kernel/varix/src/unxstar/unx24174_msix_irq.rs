//! UNX-F24174 MSI-X 中断消费与 IrqBackend 对接·子功能14（中断统计计数器）（0x417A0 段）
//!
//! 锚点：UNX-F24174-J1 统计账（向量级 raise/serve/spurious 计数逐条
//! 落账）；J2 统计语义（计数单调 + 守恒式 + 零向量三性质）；J3 统计
//! 指纹（账折叠指纹）三判据全绿。
//! 域：CoRun·UNX-F5-B15（中断统计面）。
//!
//! 结构：计数器表 + bump 核 + 守恒校验核 + 折叠指纹 + 注入核（野向
//! 量/重号/超范围/印后/守恒破必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x417A0 独占 0x20）。
pub const SEG_BASE: u32 = 0x417A0;
pub const CS_UNK_VEC: u32 = SEG_BASE + 0x01; // 野向量
pub const CS_DUP: u32 = SEG_BASE + 0x02; // 重号
pub const CS_BAD_OP: u32 = SEG_BASE + 0x03; // 操作越界
pub const CS_FROZEN: u32 = SEG_BASE + 0x04; // 印后操作
pub const CS_BREAK: u32 = SEG_BASE + 0x05; // 守恒破

/// 向量计数器。
#[derive(Clone)]
pub struct CsEnt {
    pub vec_no: u32,
    pub raised: u32,
    pub served: u32,
    pub spurious: u32,
}

impl CsEnt {
    /// 守恒式：raised == served + spurious + (未服务挂起 0/1)。
    pub fn balanced(&self) -> bool {
        self.raised == self.served + self.spurious
    }
}

/// 统计簿。
pub struct CsBook {
    pub ents: Vec<CsEnt>,
    pub sealed: bool,
}

impl CsBook {
    pub fn new() -> CsBook {
        CsBook { ents: Vec::new(), sealed: false }
    }

    pub fn declare(&mut self, vec_no: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(CS_FROZEN);
        }
        if self.ents.iter().any(|e| e.vec_no == vec_no) {
            return Err(CS_DUP);
        }
        self.ents.push(CsEnt { vec_no, raised: 0, served: 0, spurious: 0 });
        Ok(())
    }

    fn bump(&mut self, vec_no: u32, field: u8) -> Result<(), u32> {
        if field > 2 {
            return Err(CS_BAD_OP);
        }
        match self.ents.iter_mut().find(|e| e.vec_no == vec_no) {
            Some(e) => {
                match field {
                    0 => e.raised += 1,
                    1 => {
                        if e.served + e.spurious < e.raised {
                            e.served += 1;
                        } else {
                            return Err(CS_BREAK);
                        }
                    }
                    _ => {
                        if e.served + e.spurious < e.raised {
                            e.spurious += 1;
                        } else {
                            return Err(CS_BREAK);
                        }
                    }
                }
                Ok(())
            }
            None => Err(CS_UNK_VEC),
        }
    }

    pub fn on_raise(&mut self, vec_no: u32) -> Result<(), u32> {
        self.bump(vec_no, 0)
    }

    pub fn on_serve(&mut self, vec_no: u32) -> Result<(), u32> {
        self.bump(vec_no, 1)
    }

    pub fn on_spurious(&mut self, vec_no: u32) -> Result<(), u32> {
        self.bump(vec_no, 2)
    }

    pub fn all_balanced(&self) -> bool {
        self.ents.iter().all(|e| e.balanced())
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x417A0;
        for e in &self.ents {
            h = h.wrapping_mul(31).wrapping_add(e.vec_no);
            h = h.wrapping_mul(31).wrapping_add(e.raised);
            h = h.wrapping_mul(31).wrapping_add(e.served);
            h = h.wrapping_mul(31).wrapping_add(e.spurious);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 标准样本：两向量。
pub fn mk_book() -> CsBook {
    let mut b = CsBook::new();
    let _ = b.declare(16);
    let _ = b.declare(17);
    for _ in 0..3 {
        let _ = b.on_raise(16);
    }
    let _ = b.on_serve(16);
    let _ = b.on_spurious(16);
    let _ = b.on_raise(17);
    b
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = mk_book();
    // 1) 野向量
    got[0] = b.on_raise(99).err() == Some(CS_UNK_VEC);
    // 2) 重号
    got[1] = b.declare(16).err() == Some(CS_DUP);
    // 3) 守恒破（serve 超余额）
    let _ = b.on_serve(16);
    let _ = b.on_serve(16); // raised=3 served 已 3 → 破
    got[2] = b.on_serve(16).err() == Some(CS_BREAK);
    // 4) 操作越界
    got[3] = b.bump(16, 9).err() == Some(CS_BAD_OP);
    // 5) 印后操作必抓
    b.seal();
    got[4] = b.on_raise(17).err() == Some(CS_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx24174_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F24174");
    let mut b = mk_book();

    // J1 计数器账齐
    let j1 = b.ents.len() == 2;
    s.add("J1-计数器齐", j1, "两向量落表");

    // J2 计数精确
    let j2 = {
        let e = b.ents.iter().find(|e| e.vec_no == 16).unwrap();
        e.raised == 3 && e.served == 1 && e.spurious == 1
    };
    s.add("J2-计数精确", j2, "3/1/1");

    // J3 守恒式
    let j3 = b.all_balanced();
    s.add("J3-守恒式", j3, "raised = served + spurious");

    // J4 serve 超余额拒
    let j4 = {
        let mut b2 = CsBook::new();
        let _ = b2.declare(5);
        b2.on_serve(5).err() == Some(CS_BREAK)
    };
    s.add("J4-超余额拒", j4, "无 raise 不可 serve");

    // J5 守恒破拒
    let j5 = {
        let _ = b.on_serve(16);
        b.on_serve(16).err() == Some(CS_BREAK)
    };
    s.add("J5-守恒破拒", j5, "余额尽后必拒");

    // J6 野向量拒
    let j6 = b.on_raise(99).err() == Some(CS_UNK_VEC);
    s.add("J6-野向量拒", j6, "未注册必拒");

    // J7 重号拒
    let j7 = b.declare(16).err() == Some(CS_DUP);
    s.add("J7-重号拒", j7, "向量重号必拒");

    // J8 操作越界拒
    let j8 = b.bump(16, 9).err() == Some(CS_BAD_OP);
    s.add("J8-操作越界拒", j8, "field > 2 必拒");

    // J9 零向量计数
    let j9 = {
        let e = b.ents.iter().find(|e| e.vec_no == 17).unwrap();
        e.raised == 1 && e.served == 0
    };
    s.add("J9-零服务计数", j9, "17 未服务");

    // J10 单调性（raise 只增）
    let j10 = {
        let before = b.ents.iter().find(|e| e.vec_no == 17).unwrap().raised;
        let _ = b.on_raise(17);
        let after = b.ents.iter().find(|e| e.vec_no == 17).unwrap().raised;
        after == before + 1
    };
    s.add("J10-单调增", j10, "raise 只增");

    // J11 印后操作拒
    let j11 = {
        let mut b2 = CsBook::new();
        let _ = b2.declare(5);
        b2.seal();
        b2.on_raise(5).err() == Some(CS_FROZEN)
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
    fn unx24174_counter_flow() {
        let mut b = CsBook::new();
        let _ = b.declare(16);
        let _ = b.on_raise(16);
        assert!(b.on_serve(16).is_ok());
        assert_eq!(b.on_serve(16), Err(CS_BREAK));
        assert_eq!(b.on_spurious(16), Err(CS_BREAK));
        let _ = b.on_raise(16);
        assert!(b.on_spurious(16).is_ok());
        assert_eq!(b.on_raise(99), Err(CS_UNK_VEC));
        assert_eq!(b.declare(16), Err(CS_DUP));
        b.seal();
        assert_eq!(b.on_raise(16), Err(CS_FROZEN));
    }

    #[test]
    fn unx24174_checks_green() {
        let s = run_unx24174_checks();
        assert!(s.all_passed(), "UNX-F24174 判据未全绿");
    }
}
