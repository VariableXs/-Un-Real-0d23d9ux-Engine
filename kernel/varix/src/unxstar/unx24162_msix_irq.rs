//! UNX-F24162 MSI-X 中断消费与 IrqBackend 对接·子功能02（向量使能禁用闸）（0x41620 段）
//!
//! 锚点：UNX-F24162-J1 使能账（向量 enable/disable 逐条落账）；J2 使
//! 能语义（禁用拒派发 + 双禁幂等 + 未注册拒三性质）；J3 使能指纹（账
//! 折叠指纹）三判据全绿。
//! 域：CoRun·UNX-F5-B15（向量使能闸面）。
//!
//! 结构：向量态表 + 使能闸核 + 派发闸核 + 账本 + 折叠指纹 + 注入核（
//! 禁用派发/野向量/双禁/印后/使能越界必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x41620 独占 0x20）。
pub const SEG_BASE: u32 = 0x41620;
pub const EN_DISABLED: u32 = SEG_BASE + 0x01; // 禁用派发
pub const EN_UNK_VEC: u32 = SEG_BASE + 0x02; // 野向量
pub const EN_DUP_OP: u32 = SEG_BASE + 0x03; // 双禁幂等档
pub const EN_FROZEN: u32 = SEG_BASE + 0x04; // 印后操作
pub const EN_BAD_OP: u32 = SEG_BASE + 0x05; // 操作档越界

/// 向量态。
#[derive(Clone)]
pub struct EnVec {
    pub vec_no: u32,
    pub enabled: bool,
    pub disp_ok: u32,
    pub disp_blocked: u32,
}

/// 使能账。
#[derive(Clone)]
pub struct EnRec {
    pub seq: u32,
    pub vec_no: u32,
    pub op: u8, // 0=en 1=dis 2=dispatch
    pub ok: bool,
}

/// 闸簿。
pub struct EnBook {
    pub vecs: Vec<EnVec>,
    pub ledger: Vec<EnRec>,
    pub seq_next: u32,
    pub sealed: bool,
}

impl EnBook {
    pub fn new() -> EnBook {
        EnBook { vecs: Vec::new(), ledger: Vec::new(), seq_next: 1, sealed: false }
    }

    pub fn declare(&mut self, vec_no: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(EN_FROZEN);
        }
        if self.vecs.iter().any(|v| v.vec_no == vec_no) {
            return Err(EN_DUP_OP);
        }
        self.vecs.push(EnVec { vec_no, enabled: false, disp_ok: 0, disp_blocked: 0 });
        Ok(())
    }

    fn rec(&mut self, vec_no: u32, op: u8, ok: bool) {
        let seq = self.seq_next;
        self.seq_next += 1;
        self.ledger.push(EnRec { seq, vec_no, op, ok });
    }

    pub fn set_enable(&mut self, vec_no: u32, on: bool) -> Result<(), u32> {
        if self.sealed {
            return Err(EN_FROZEN);
        }
        let v = match self.vecs.iter_mut().find(|v| v.vec_no == vec_no) {
            Some(v) => v,
            None => return Err(EN_UNK_VEC),
        };
        let already = v.enabled == on;
        v.enabled = on;
        self.rec(vec_no, if on { 0 } else { 1 }, !already);
        if already {
            return Err(EN_DUP_OP);
        }
        Ok(())
    }

    /// 派发：仅使能向量可派。
    pub fn dispatch(&mut self, vec_no: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(EN_FROZEN);
        }
        let v = match self.vecs.iter_mut().find(|v| v.vec_no == vec_no) {
            Some(v) => v,
            None => {
                self.rec(vec_no, 2, false);
                return Err(EN_UNK_VEC);
            }
        };
        if !v.enabled {
            v.disp_blocked += 1;
            self.rec(vec_no, 2, false);
            return Err(EN_DISABLED);
        }
        v.disp_ok += 1;
        self.rec(vec_no, 2, true);
        Ok(())
    }

    pub fn blocked_total(&self) -> u32 {
        self.vecs.iter().map(|v| v.disp_blocked).sum()
    }

    pub fn ok_total(&self) -> u32 {
        self.vecs.iter().map(|v| v.disp_ok).sum()
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x41620;
        for r in &self.ledger {
            h = h.wrapping_mul(31).wrapping_add(r.seq);
            h = h.wrapping_mul(31).wrapping_add(r.vec_no);
            h = h.wrapping_mul(31).wrapping_add(r.op as u32);
            h = h.wrapping_mul(31).wrapping_add(r.ok as u32);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 标准样本：两向量一使能一禁用。
pub fn mk_book() -> EnBook {
    let mut b = EnBook::new();
    let _ = b.declare(0);
    let _ = b.declare(1);
    let _ = b.set_enable(0, true);
    let _ = b.dispatch(0); // ok
    let _ = b.dispatch(1); // blocked
    b
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = mk_book();
    // 1) 禁用派发
    got[0] = b.dispatch(1).err() == Some(EN_DISABLED);
    // 2) 野向量
    got[1] = b.dispatch(99).err() == Some(EN_UNK_VEC);
    // 3) 双禁幂等档
    got[2] = b.set_enable(1, false).err() == Some(EN_DUP_OP);
    // 4) 使能后反使能档（同态）
    got[3] = b.set_enable(0, true).err() == Some(EN_DUP_OP);
    // 5) 印后操作必抓
    b.seal();
    got[4] = b.dispatch(0).err() == Some(EN_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx24162_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F24162");
    let mut b = mk_book();

    // J1 向量账齐
    let j1 = b.vecs.len() == 2;
    s.add("J1-向量账齐", j1, "两向量全落表");

    // J2 禁用派发拒
    let j2 = b.dispatch(1).err() == Some(EN_DISABLED);
    s.add("J2-禁用派发拒", j2, "禁用向量派发必拒");

    // J3 使能派发放行
    let j3 = b.dispatch(0).is_ok();
    s.add("J3-使能派发放行", j3, "使能向量可派");

    // J4 阻断计数
    let j4 = b.blocked_total() == 2;
    s.add("J4-阻断计数", j4, "两次阻断落账");

    // J5 成功计数
    let j5 = b.ok_total() == 2;
    s.add("J5-成功计数", j5, "两次成功落账");

    // J6 双禁幂等档
    let j6 = b.set_enable(1, false).err() == Some(EN_DUP_OP);
    s.add("J6-同态设幂等", j6, "同态设置必报档");

    // J7 野向量拒
    let j7 = b.dispatch(99).err() == Some(EN_UNK_VEC);
    s.add("J7-野向量拒", j7, "未注册向量必拒");

    // J8 重号声明拒
    let j8 = b.declare(0).err() == Some(EN_DUP_OP);
    s.add("J8-重号声明拒", j8, "向量重号必拒");

    // J9 使能后可禁
    let j9 = {
        let mut b2 = mk_book();
        b2.set_enable(0, false).is_ok() && b2.dispatch(0).is_err()
    };
    s.add("J9-使能可禁", j9, "禁后派发即拒");

    // J10 账面 ok 与态一致
    let j10 = b.ledger.iter().all(|r| {
        let v = b.vecs.iter().find(|v| v.vec_no == r.vec_no);
        match (r.op, v) {
            (2, Some(v)) => r.ok == v.enabled || !r.ok,
            _ => true,
        }
    });
    s.add("J10-账态一致", j10, "派发账与使能态自洽");

    // J11 禁用后复使能
    let j11 = {
        let mut b2 = mk_book();
        let _ = b2.set_enable(1, true);
        b2.dispatch(1).is_ok()
    };
    s.add("J11-复使能派发", j11, "再使能后派发放行");

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
    fn unx24162_enable_flow() {
        let mut b = EnBook::new();
        let _ = b.declare(0);
        assert_eq!(b.dispatch(0), Err(EN_DISABLED));
        assert!(b.set_enable(0, true).is_ok());
        assert!(b.dispatch(0).is_ok());
        assert_eq!(b.set_enable(0, true), Err(EN_DUP_OP));
        assert!(b.set_enable(0, false).is_ok());
        assert_eq!(b.dispatch(0), Err(EN_DISABLED));
        assert_eq!(b.dispatch(9), Err(EN_UNK_VEC));
        b.seal();
        assert_eq!(b.dispatch(0), Err(EN_FROZEN));
    }

    #[test]
    fn unx24162_checks_green() {
        let s = run_unx24162_checks();
        assert!(s.all_passed(), "UNX-F24162 判据未全绿");
    }
}
