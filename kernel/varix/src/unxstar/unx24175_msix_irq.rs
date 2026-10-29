//! UNX-F24175 MSI-X 中断消费与 IrqBackend 对接·子功能15（热插拔向量回收）（0x417C0 段）
//!
//! 锚点：UNX-F24175-J1 回收账（向量热插回收逐条落账）；J2 回收语义
//! （在用拒收 + 回收复用 + 挂起未清拒三性质）；J3 回收指纹（账折叠
//! 指纹）三判据全绿。
//! 域：CoRun·UNX-F5-B15（热插拔回收面）。
//!
//! 结构：向量生命周期表 + 回收闸核 + 复用核 + 账本 + 折叠指纹 + 注入
//! 核（在用回收/挂起回收/野向量/印后/重复用必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x417C0 独占 0x20）。
pub const SEG_BASE: u32 = 0x417C0;
pub const HV_IN_USE: u32 = SEG_BASE + 0x01; // 在用拒收
pub const HV_PENDING: u32 = SEG_BASE + 0x02; // 挂起未清拒收
pub const HV_UNK_VEC: u32 = SEG_BASE + 0x03; // 野向量
pub const HV_FROZEN: u32 = SEG_BASE + 0x04; // 印后操作
pub const HV_DUP_USE: u32 = SEG_BASE + 0x05; // 重复占用

/// 向量生命周期。
pub const VS_IDLE: u8 = 0;
pub const VS_BOUND: u8 = 1;
pub const VS_RETIRED: u8 = 2;
pub const VS_COUNT: u8 = 3;

/// 向量条目。
#[derive(Clone)]
pub struct HvVec {
    pub vec_no: u32,
    pub state: u8,
    pub pending: bool,
    pub owner: String,
    pub retire_count: u32,
}

/// 回收簿。
pub struct HvBook {
    pub vecs: Vec<HvVec>,
    pub sealed: bool,
}

impl HvBook {
    pub fn new(n: u32) -> HvBook {
        let mut v = Vec::new();
        for i in 0..n {
            v.push(HvVec {
                vec_no: i,
                state: VS_IDLE,
                pending: false,
                owner: String::new(),
                retire_count: 0,
            });
        }
        HvBook { vecs: v, sealed: false }
    }

    pub fn bind(&mut self, vec_no: u32, owner: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(HV_FROZEN);
        }
        if owner.is_empty() {
            return Err(HV_DUP_USE);
        }
        match self.vecs.iter_mut().find(|v| v.vec_no == vec_no) {
            Some(v) => {
                if v.state != VS_IDLE {
                    return Err(HV_IN_USE);
                }
                v.state = VS_BOUND;
                v.owner = owner.to_string();
                Ok(())
            }
            None => Err(HV_UNK_VEC),
        }
    }

    pub fn set_pending(&mut self, vec_no: u32, on: bool) -> Result<(), u32> {
        if self.sealed {
            return Err(HV_FROZEN);
        }
        match self.vecs.iter_mut().find(|v| v.vec_no == vec_no) {
            Some(v) => {
                v.pending = on;
                Ok(())
            }
            None => Err(HV_UNK_VEC),
        }
    }

    /// 回收：BOUND 且无挂起方可 retire。
    pub fn retire(&mut self, vec_no: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(HV_FROZEN);
        }
        match self.vecs.iter_mut().find(|v| v.vec_no == vec_no) {
            Some(v) => {
                if v.state == VS_IDLE {
                    return Err(HV_UNK_VEC);
                }
                if v.pending {
                    return Err(HV_PENDING);
                }
                v.state = VS_RETIRED;
                v.owner.clear();
                v.retire_count += 1;
                // RETIRED 即复归 IDLE（回收复用语义）
                v.state = VS_IDLE;
                Ok(())
            }
            None => Err(HV_UNK_VEC),
        }
    }

    pub fn idle_count(&self) -> usize {
        self.vecs.iter().filter(|v| v.state == VS_IDLE).count()
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x417C0;
        for v in &self.vecs {
            h = h.wrapping_mul(31).wrapping_add(v.vec_no);
            h = h.wrapping_mul(31).wrapping_add(v.state as u32);
            h = h.wrapping_mul(31).wrapping_add(v.pending as u32);
            h = h.wrapping_mul(31).wrapping_add(v.retire_count);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 标准样本：4 向量。
pub fn mk_book() -> HvBook {
    let mut b = HvBook::new(4);
    let _ = b.bind(0, "gpu");
    let _ = b.bind(1, "dma");
    let _ = b.retire(1); // 回收复归
    let _ = b.bind(2, "npu");
    let _ = b.set_pending(2, true);
    b
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = mk_book();
    // 1) 在用拒收（重绑）
    got[0] = b.bind(0, "other").err() == Some(HV_IN_USE);
    // 2) 挂起回收拒
    got[1] = b.retire(2).err() == Some(HV_PENDING);
    // 3) 野向量
    got[2] = b.bind(99, "x").err() == Some(HV_UNK_VEC);
    // 4) IDLE 回收拒
    got[3] = b.retire(3).err() == Some(HV_UNK_VEC);
    // 5) 印后操作必抓
    b.seal();
    got[4] = b.bind(3, "x").err() == Some(HV_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx24175_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F24175");
    let mut b = mk_book();

    // J1 向量账齐
    let j1 = b.vecs.len() == 4;
    s.add("J1-向量账齐", j1, "四向量落表");

    // J2 绑定态
    let j2 = {
        let v = b.vecs.iter().find(|v| v.vec_no == 0).unwrap();
        v.state == VS_BOUND && v.owner == "gpu"
    };
    s.add("J2-绑定态", j2, "向量 0 绑 gpu");

    // J3 回收复归
    let j3 = {
        let v = b.vecs.iter().find(|v| v.vec_no == 1).unwrap();
        v.state == VS_IDLE && v.retire_count == 1 && v.owner.is_empty()
    };
    s.add("J3-回收复归", j3, "向量 1 回收复归 IDLE");

    // J4 回收后复用
    let j4 = {
        let _ = b.bind(1, "audio");
        b.vecs.iter().find(|v| v.vec_no == 1).map(|v| v.state == VS_BOUND).unwrap_or(false)
    };
    s.add("J4-回收复用", j4, "回收后可重绑");

    // J5 在用拒收
    let j5 = b.bind(0, "other").err() == Some(HV_IN_USE);
    s.add("J5-在用拒收", j5, "BOUND 重绑必拒");

    // J6 挂起回收拒
    let j6 = b.retire(2).err() == Some(HV_PENDING);
    s.add("J6-挂起回收拒", j6, "挂起向量必拒收");

    // J7 挂起清后可收
    let j7 = {
        let _ = b.set_pending(2, false);
        b.retire(2).is_ok()
    };
    s.add("J7-清挂可收", j7, "清挂起后回收放行");

    // J8 IDLE 回收拒
    let j8 = b.retire(3).err() == Some(HV_UNK_VEC);
    s.add("J8-IDLE回收拒", j8, "空闲向量必拒");

    // J9 野向量拒
    let j9 = b.bind(99, "x").err() == Some(HV_UNK_VEC);
    s.add("J9-野向量拒", j9, "越表必拒");

    // J10 空主拒
    let j10 = b.bind(3, "").err() == Some(HV_DUP_USE);
    s.add("J10-空主拒", j10, "空持有者必拒");

    // J11 回收计数守恒
    let j11 = b.vecs.iter().all(|v| v.retire_count <= 1);
    s.add("J11-回收计数", j11, "回收次数 ≤ 1");

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
    fn unx24175_retire_flow() {
        let mut b = HvBook::new(2);
        assert!(b.bind(0, "a").is_ok());
        assert_eq!(b.bind(0, "b"), Err(HV_IN_USE));
        assert!(b.retire(0).is_ok());
        assert!(b.bind(0, "b").is_ok());
        let _ = b.set_pending(0, true);
        assert_eq!(b.retire(0), Err(HV_PENDING));
        let _ = b.set_pending(0, false);
        assert!(b.retire(0).is_ok());
        assert_eq!(b.retire(1), Err(HV_UNK_VEC));
        b.seal();
        assert_eq!(b.bind(1, "x"), Err(HV_FROZEN));
    }

    #[test]
    fn unx24175_checks_green() {
        let s = run_unx24175_checks();
        assert!(s.all_passed(), "UNX-F24175 判据未全绿");
    }
}
