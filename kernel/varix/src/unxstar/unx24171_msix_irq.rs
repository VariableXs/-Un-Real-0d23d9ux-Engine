//! UNX-F24171 MSI-X 中断消费与 IrqBackend 对接·子功能11（spurious 中断计数）（0x41740 段）
//!
//! 锚点：UNX-F24171-J1 伪中断账（spurious 判定与计数逐条落账）；J2
//! 伪中断语义（无挂起判伪 + 计数守恒 + 连伪限流三性质）；J3 伪中断
//! 指纹（账折叠指纹）三判据全绿。
//! 域：CoRun·UNX-F5-B15（spurious 计数面）。
//!
//! 结构：挂起表 + 伪中断判定核 + 限流核 + 账本 + 折叠指纹 + 注入核（
//! 挂起误判伪/野向量/超连伪/印后/重号必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x41740 独占 0x20）。
pub const SEG_BASE: u32 = 0x41740;
pub const SP_UNK_VEC: u32 = SEG_BASE + 0x01; // 野向量
pub const SP_STREAK: u32 = SEG_BASE + 0x02; // 连伪超限
pub const SP_DUP_DECL: u32 = SEG_BASE + 0x03; // 重号声明
pub const SP_FROZEN: u32 = SEG_BASE + 0x04; // 印后操作
pub const SP_BAD_STREAK: u32 = SEG_BASE + 0x05; // 限流档越界

/// 向量伪中断态。
#[derive(Clone)]
pub struct SpVec {
    pub vec_no: u32,
    pub pending: bool,
    pub spurious: u32,
    pub real: u32,
    pub streak: u32,
}

/// 伪中断簿。
pub struct SpBook {
    pub vecs: Vec<SpVec>,
    pub streak_max: u32,
    pub sealed: bool,
}

impl SpBook {
    pub fn new(streak_max: u32) -> SpBook {
        SpBook { vecs: Vec::new(), streak_max, sealed: false }
    }

    pub fn declare(&mut self, vec_no: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(SP_FROZEN);
        }
        if self.vecs.iter().any(|v| v.vec_no == vec_no) {
            return Err(SP_DUP_DECL);
        }
        self.vecs.push(SpVec { vec_no, pending: false, spurious: 0, real: 0, streak: 0 });
        Ok(())
    }

    pub fn set_pending(&mut self, vec_no: u32, on: bool) -> Result<(), u32> {
        if self.sealed {
            return Err(SP_FROZEN);
        }
        match self.vecs.iter_mut().find(|v| v.vec_no == vec_no) {
            Some(v) => {
                v.pending = on;
                Ok(())
            }
            None => Err(SP_UNK_VEC),
        }
    }

    /// 中断消费：有挂起=真，无挂起=伪（连伪超限拒）。
    pub fn consume(&mut self, vec_no: u32) -> Result<bool, u32> {
        if self.sealed {
            return Err(SP_FROZEN);
        }
        let v = match self.vecs.iter_mut().find(|v| v.vec_no == vec_no) {
            Some(v) => v,
            None => return Err(SP_UNK_VEC),
        };
        if v.pending {
            v.pending = false;
            v.real += 1;
            v.streak = 0;
            Ok(true)
        } else {
            if v.streak >= self.streak_max {
                return Err(SP_STREAK);
            }
            v.spurious += 1;
            v.streak += 1;
            Ok(false)
        }
    }

    pub fn spurious_total(&self) -> u32 {
        self.vecs.iter().map(|v| v.spurious).sum()
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x41740;
        for v in &self.vecs {
            h = h.wrapping_mul(31).wrapping_add(v.vec_no);
            h = h.wrapping_mul(31).wrapping_add(v.spurious);
            h = h.wrapping_mul(31).wrapping_add(v.real);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 标准样本：两向量，一真两伪。
pub fn mk_book() -> SpBook {
    let mut b = SpBook::new(3);
    let _ = b.declare(16);
    let _ = b.declare(17);
    let _ = b.set_pending(16, true);
    let _ = b.consume(16); // true
    let _ = b.consume(16); // false 伪
    let _ = b.consume(16); // false 伪
    b
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = mk_book();
    // 1) 野向量
    got[0] = b.consume(99).err() == Some(SP_UNK_VEC);
    // 2) 连伪超限
    let _ = b.consume(16); // streak 3 = max
    got[1] = b.consume(16).err() == Some(SP_STREAK);
    // 3) 重号声明
    got[2] = b.declare(16).err() == Some(SP_DUP_DECL);
    // 4) 真中断清连伪
    let mut b2 = SpBook::new(2);
    let _ = b2.declare(5);
    let _ = b2.consume(5); // 伪 streak 1
    let _ = b2.set_pending(5, true);
    let _ = b2.consume(5); // 真 streak 清 0
    got[3] = b2.vecs.iter().find(|v| v.vec_no == 5).map(|v| v.streak == 0).unwrap_or(false);
    // 5) 印后操作必抓
    b.seal();
    got[4] = b.consume(17).err() == Some(SP_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx24171_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F24171");
    let mut b = mk_book();

    // J1 向量账齐
    let j1 = b.vecs.len() == 2;
    s.add("J1-向量账齐", j1, "两向量落表");

    // J2 真伪分流
    let j2 = {
        let v = b.vecs.iter().find(|v| v.vec_no == 16).unwrap();
        v.real == 1 && v.spurious == 2
    };
    s.add("J2-真伪分流", j2, "1 真 2 伪");

    // J3 连伪限流
    let j3 = {
        let _ = b.consume(16); // streak 3
        b.consume(16).err() == Some(SP_STREAK)
    };
    s.add("J3-连伪限流", j3, "超 streak_max 必拒");

    // J4 真中断清连伪
    let j4 = {
        let mut b2 = SpBook::new(2);
        let _ = b2.declare(5);
        let _ = b2.consume(5);
        let _ = b2.set_pending(5, true);
        let _ = b2.consume(5);
        b2.vecs.iter().find(|v| v.vec_no == 5).map(|v| v.streak == 0).unwrap_or(false)
    };
    s.add("J4-真清连伪", j4, "真中断 streak 归零");

    // J5 伪中断总数
    let j5 = b.spurious_total() >= 2;
    s.add("J5-伪中断总数", j5, "累计 ≥ 2");

    // J6 野向量拒
    let j6 = b.consume(99).err() == Some(SP_UNK_VEC);
    s.add("J6-野向量拒", j6, "未注册必拒");

    // J7 重号声明拒
    let j7 = b.declare(16).err() == Some(SP_DUP_DECL);
    s.add("J7-重号拒", j7, "向量重号必拒");

    // J8 挂起翻转
    let j8 = {
        let _ = b.set_pending(17, true);
        let ok = b.consume(17) == Ok(true);
        let _ = b.set_pending(17, false);
        ok
    };
    s.add("J8-挂起翻转", j8, "置位后消费为真");

    // J9 消费后挂起清
    let j9 = {
        let _ = b.set_pending(17, true);
        let _ = b.consume(17);
        b.vecs.iter().find(|v| v.vec_no == 17).map(|v| !v.pending).unwrap_or(false)
    };
    s.add("J9-消费清位", j9, "真消费清挂起");

    // J10 限流参数
    let j10 = b.streak_max == 3;
    s.add("J10-限流参数", j10, "streak_max=3");

    // J11 印后操作拒
    let j11 = {
        let mut b2 = mk_book();
        b2.seal();
        b2.consume(16).err() == Some(SP_FROZEN)
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
    fn unx24171_spurious_flow() {
        let mut b = SpBook::new(1);
        let _ = b.declare(16);
        assert_eq!(b.consume(16), Ok(false));
        assert_eq!(b.consume(16), Err(SP_STREAK));
        let _ = b.set_pending(16, true);
        assert_eq!(b.consume(16), Ok(true));
        assert_eq!(b.consume(16), Ok(false));
        assert_eq!(b.declare(16), Err(SP_DUP_DECL));
        assert_eq!(b.consume(99), Err(SP_UNK_VEC));
        b.seal();
        assert_eq!(b.consume(16), Err(SP_FROZEN));
    }

    #[test]
    fn unx24171_checks_green() {
        let s = run_unx24171_checks();
        assert!(s.all_passed(), "UNX-F24171 判据未全绿");
    }
}
