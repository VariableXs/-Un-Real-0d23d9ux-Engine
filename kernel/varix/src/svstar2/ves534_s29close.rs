//! VE-F4300 · U05 组收口双签（U05 工具集成组）
//! 职责：U05 收口双签（家族模板同域沿用）+ 三件（台账深化/会话号回写/
//! 字数核对）+ 交付声明（九面集成全部可用且判定权单源未被稀释）
//! + 未销红线拒绝 + 回写失败整组回滚。
//! 段位：0x2F60（续表 idx534）。行数预算 300。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x2F60;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum U5ClCode {
    WordMismatch = SEG_BASE,
    RedlineOpen = SEG_BASE + 0x01,
    SingleSrcDiluted = SEG_BASE + 0x02,
    SameSigner = SEG_BASE + 0x03,
    Rollback = SEG_BASE + 0x04,
}

/// 交付声明。
#[derive(Clone, Copy, Debug)]
pub struct Delivery {
    /// 九面可用位图。
    pub faces_ok: [bool; 9],
    /// 判定权单源未被稀释。
    pub single_src: bool,
}

impl Delivery {
    pub fn all_faces(&self) -> bool {
        self.faces_ok.iter().all(|f| *f)
    }
}

/// U05 收口器。
pub struct U05Closer {
    pub ledger_done: bool,
    pub session_back: Option<u32>,
    pub words_claim: u32,
    pub words_real: u32,
    pub delivery: Delivery,
    pub open_redlines: Vec<u16>,
    pub closed: bool,
    pub rolled: u32,
}

impl U05Closer {
    pub fn new(delivery: Delivery) -> Self {
        U05Closer { ledger_done: false, session_back: None, words_claim: 0, words_real: 0, delivery, open_redlines: Vec::new(), closed: false, rolled: 0 }
    }

    pub fn try_close(&mut self, a: u8, b: u8) -> Result<(), U5ClCode> {
        if a == b || a == 0 || b == 0 {
            return Err(U5ClCode::SameSigner);
        }
        if self.words_claim != self.words_real {
            return Err(U5ClCode::WordMismatch);
        }
        if !self.open_redlines.is_empty() {
            return Err(U5ClCode::RedlineOpen);
        }
        if !self.delivery.all_faces() {
            return Err(U5ClCode::Rollback); // 九面缺一不可交付
        }
        if !self.delivery.single_src {
            return Err(U5ClCode::SingleSrcDiluted);
        }
        if !self.ledger_done || self.session_back.is_none() {
            self.rollback();
            return Err(U5ClCode::Rollback);
        }
        self.closed = true;
        Ok(())
    }

    fn rollback(&mut self) {
        self.closed = false;
        self.rolled += 1;
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

/// 判据组：C534-01..12。
pub fn run_ves534_checks() -> CheckSet {
    let mut set = CheckSet::new("ves534");

    let mut cl = U05Closer::new(Delivery { faces_ok: [true; 9], single_src: true });
    cl.ledger_done = true;
    cl.session_back = Some(77);
    cl.words_claim = 300;
    cl.words_real = 290;
    chk(&mut set, "C534-01", cl.try_close(1, 2) == Err(U5ClCode::WordMismatch), "字数不符禁签");
    cl.words_real = 300;
    chk(&mut set, "C534-02", cl.try_close(1, 2) == Err(U5ClCode::RedlineOpen), "未销红线拒绝收口");
    cl.open_redlines.push(5);
    chk(&mut set, "C534-03", cl.try_close(1, 2) == Err(U5ClCode::RedlineOpen), "红线在册即拒");
    cl.open_redlines.clear();

    cl.delivery.faces_ok[8] = false;
    chk(&mut set, "C534-04", cl.try_close(1, 2) == Err(U5ClCode::Rollback), "九面缺一不可交付");
    chk(&mut set, "C534-05", cl.rolled == 1, "回滚计数");
    cl.delivery.faces_ok[8] = true;

    cl.delivery.single_src = false;
    chk(&mut set, "C534-06", cl.try_close(1, 2) == Err(U5ClCode::SingleSrcDiluted), "判定权单源被稀释拒绝");
    cl.delivery.single_src = true;

    chk(&mut set, "C534-07", cl.try_close(1, 1) == Err(U5ClCode::SameSigner), "双签同人");
    chk(&mut set, "C534-08", cl.try_close(1, 2).is_ok(), "全条件收口");
    chk(&mut set, "C534-09", cl.closed, "收口在册");

    let d = Delivery { faces_ok: [true; 9], single_src: true };
    chk(&mut set, "C534-10", d.all_faces(), "交付声明九面语义");
    let d2 = Delivery { faces_ok: [false; 9], single_src: false };
    chk(&mut set, "C534-11", !d2.all_faces() && !d2.single_src, "空声明拒绝");
    chk(&mut set, "C534-12", cl.rolled == 1 && cl.session_back == Some(77), "会话号回写在册");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn u05_closer() {
        let mut cl = U05Closer::new(Delivery { faces_ok: [true; 9], single_src: true });
        cl.words_claim = 1;
        cl.words_real = 1;
        cl.ledger_done = true;
        cl.session_back = Some(1);
        assert!(cl.try_close(1, 2).is_ok());
        cl.open_redlines.push(1);
        assert_eq!(cl.try_close(1, 2), Err(U5ClCode::RedlineOpen));
    }
}
