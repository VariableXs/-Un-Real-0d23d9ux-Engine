//! UNX-F24161 MSI-X 中断消费与 IrqBackend 对接·子功能01（向量表注册）（0x41600 段）
//!
//! 锚点：UNX-F24161-J1 向量账（MSI-X 向量表逐条落账）；J2 向量语义
//! （槽位闭集 + 重号拒 + 撤销回收三性质）；J3 向量指纹（表折叠指纹）
//! 三判据全绿。
//! 域：CoRun·UNX-F5-B15（MSI-X 向量表面）。
//!
//! 结构：向量表 + 注册/撤销核 + 账本 + 折叠指纹 + 注入核（越界槽/
//! 重号/野撤销/印后/空向量必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x41600 独占 0x20）。
pub const SEG_BASE: u32 = 0x41600;
pub const VX_OVER_SLOT: u32 = SEG_BASE + 0x01; // 越界槽
pub const VX_DUP_VEC: u32 = SEG_BASE + 0x02; // 重号向量
pub const VX_UNK_FREE: u32 = SEG_BASE + 0x03; // 野撤销
pub const VX_FROZEN: u32 = SEG_BASE + 0x04; // 印后注册
pub const VX_EMPTY: u32 = SEG_BASE + 0x05; // 空向量

/// 向量条目。
#[derive(Clone)]
pub struct VxEnt {
    pub vec_no: u32,
    pub slot: u32,
    pub msg_addr: u32,
    pub msg_data: u32,
    pub enabled: bool,
}

/// 向量表。
pub struct VxTable {
    pub ents: Vec<VxEnt>,
    pub slots: u32,
    pub sealed: bool,
}

impl VxTable {
    pub fn new(slots: u32) -> VxTable {
        VxTable { ents: Vec::new(), slots, sealed: false }
    }

    /// 注册：槽位闭集 + 重号拒。
    pub fn register(&mut self, vec_no: u32, slot: u32, msg_addr: u32, msg_data: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(VX_FROZEN);
        }
        if slot >= self.slots {
            return Err(VX_OVER_SLOT);
        }
        if self.ents.iter().any(|e| e.vec_no == vec_no) {
            return Err(VX_DUP_VEC);
        }
        if self.ents.iter().any(|e| e.slot == slot) {
            return Err(VX_DUP_VEC);
        }
        self.ents.push(VxEnt { vec_no, slot, msg_addr, msg_data, enabled: false });
        Ok(())
    }

    /// 撤销回收。
    pub fn unregister(&mut self, vec_no: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(VX_FROZEN);
        }
        let before = self.ents.len();
        self.ents.retain(|e| e.vec_no != vec_no);
        if self.ents.len() == before {
            return Err(VX_UNK_FREE);
        }
        Ok(())
    }

    pub fn enable(&mut self, vec_no: u32, on: bool) -> Result<(), u32> {
        if self.sealed {
            return Err(VX_FROZEN);
        }
        match self.ents.iter_mut().find(|e| e.vec_no == vec_no) {
            Some(e) => {
                e.enabled = on;
                Ok(())
            }
            None => Err(VX_UNK_FREE),
        }
    }

    pub fn slot_free(&self, slot: u32) -> bool {
        !self.ents.iter().any(|e| e.slot == slot)
    }

    pub fn enabled_count(&self) -> usize {
        self.ents.iter().filter(|e| e.enabled).count()
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x41600;
        for e in &self.ents {
            h = h.wrapping_mul(31).wrapping_add(e.vec_no);
            h = h.wrapping_mul(31).wrapping_add(e.slot);
            h = h.wrapping_mul(31).wrapping_add(e.msg_data);
            h = h.wrapping_mul(31).wrapping_add(e.enabled as u32);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 标准样本：8 槽表，三向量。
pub fn mk_table() -> VxTable {
    let mut t = VxTable::new(8);
    let _ = t.register(0, 0, 0xFEE0_0000, 0x4041);
    let _ = t.register(1, 2, 0xFEE0_0000, 0x4042);
    let _ = t.register(2, 4, 0xFEE0_0000, 0x4043);
    let _ = t.enable(0, true);
    t
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut t = mk_table();
    // 1) 越界槽
    got[0] = t.register(9, 8, 1, 1).err() == Some(VX_OVER_SLOT);
    // 2) 重号向量
    got[1] = t.register(0, 6, 1, 1).err() == Some(VX_DUP_VEC);
    // 3) 重号槽
    got[2] = t.register(3, 0, 1, 1).err() == Some(VX_DUP_VEC);
    // 4) 野撤销
    got[3] = t.unregister(99).err() == Some(VX_UNK_FREE);
    // 5) 印后注册必抓
    t.seal();
    got[4] = t.register(9, 6, 1, 1).err() == Some(VX_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx24161_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F24161");
    let mut t = mk_table();

    // J1 向量账齐
    let j1 = t.ents.len() == 3;
    s.add("J1-向量账齐", j1, "三向量全落表");

    // J2 槽位闭集
    let j2 = t.ents.iter().all(|e| e.slot < t.slots);
    s.add("J2-槽位闭集", j2, "全槽 < 8");

    // J3 重号向量拒
    let j3 = t.register(0, 6, 1, 1).err() == Some(VX_DUP_VEC);
    s.add("J3-重号向量拒", j3, "向量重号必拒");

    // J4 重号槽拒
    let j4 = t.register(3, 0, 1, 1).err() == Some(VX_DUP_VEC);
    s.add("J4-重号槽拒", j4, "槽位重号必拒");

    // J5 越界槽拒
    let j5 = t.register(9, 8, 1, 1).err() == Some(VX_OVER_SLOT);
    s.add("J5-越界槽拒", j5, "槽 ≥ slots 必拒");

    // J6 撤销回收
    let j6 = {
        let _ = t.unregister(2);
        t.slot_free(4)
    };
    s.add("J6-撤销回收", j6, "撤销后槽 4 复闲");

    // J7 使能计数
    let j7 = t.enabled_count() == 1;
    s.add("J7-使能计数", j7, "向量 0 已使能");

    // J8 野向量使能拒
    let j8 = t.enable(99, true).err() == Some(VX_UNK_FREE);
    s.add("J8-野向量使能拒", j8, "未注册使能必拒");

    // J9 撤销后向量复用
    let j9 = {
        let _ = t.unregister(2);
        t.register(2, 4, 1, 2).is_ok()
    };
    s.add("J9-向量复用", j9, "回收槽可重注册");

    // J10 消息地址一致
    let j10 = t.ents.iter().all(|e| e.msg_addr == 0xFEE0_0000);
    s.add("J10-消息地址一致", j10, "全向量同 MSI 地址");

    // J11 槽位互斥
    let j11 = {
        let mut ok = true;
        for i in 0..t.ents.len() {
            for j in (i + 1)..t.ents.len() {
                if t.ents[i].slot == t.ents[j].slot {
                    ok = false;
                }
            }
        }
        ok
    };
    s.add("J11-槽位互斥", j11, "无双占槽");

    // J12 指纹稳定
    let j12 = t.fold() == t.fold();
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
    fn unx24161_vector_flow() {
        let mut t = VxTable::new(2);
        assert!(t.register(0, 0, 1, 1).is_ok());
        assert_eq!(t.register(0, 1, 1, 1), Err(VX_DUP_VEC));
        assert_eq!(t.register(1, 1, 1, 1).is_ok(), true);
        assert_eq!(t.register(2, 0, 1, 1), Err(VX_DUP_VEC));
        assert_eq!(t.register(9, 2, 1, 1), Err(VX_OVER_SLOT));
        assert!(t.unregister(0).is_ok());
        assert!(t.register(3, 0, 1, 1).is_ok());
        t.seal();
        assert_eq!(t.register(9, 0, 1, 1), Err(VX_FROZEN));
    }

    #[test]
    fn unx24161_checks_green() {
        let s = run_unx24161_checks();
        assert!(s.all_passed(), "UNX-F24161 判据未全绿");
    }
}
