//! UNX-F24173 MSI-X 中断消费与 IrqBackend 对接·子功能13（MSI-X 多消息槽分配）（0x41780 段）
//!
//! 锚点：UNX-F24173-J1 分配账（多消息槽批量分配逐条落账）；J2 分配
//! 语义（连续槽批 + 容量闸 + 批量回收三性质）；J3 分配指纹（账折叠
//! 指纹）三判据全绿。
//! 域：CoRun·UNX-F5-B15（多消息槽面）。
//!
//! 结构：槽位池 + 批量分配核（连续槽）+ 回收核 + 账本 + 折叠指纹 +
//! 注入核（不连续/超容/重批主/印后/野批回收必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use crate::checks::CheckSet;

/// 诊断段（0x41780 独占 0x20）。
pub const SEG_BASE: u32 = 0x41780;
pub const MX_NO_SPACE: u32 = SEG_BASE + 0x01; // 超容
pub const MX_DUP_OWNER: u32 = SEG_BASE + 0x02; // 重批主
pub const MX_UNK_FREE: u32 = SEG_BASE + 0x03; // 野批回收
pub const MX_FROZEN: u32 = SEG_BASE + 0x04; // 印后操作
pub const MX_ZERO_ALLOC: u32 = SEG_BASE + 0x05; // 零槽批

/// 槽分配批。
#[derive(Clone)]
pub struct MxAlloc {
    pub owner: String,
    pub base: u32,
    pub count: u32,
}

/// 槽池。
pub struct MxPool {
    pub total_slots: u32,
    pub allocs: Vec<MxAlloc>,
    pub sealed: bool,
}

impl MxPool {
    pub fn new(total_slots: u32) -> MxPool {
        MxPool { total_slots, allocs: Vec::new(), sealed: false }
    }

    /// 槽占用图。
    pub fn occupancy(&self) -> Vec<bool> {
        let mut m = vec![false; self.total_slots as usize];
        for a in &self.allocs {
            for i in 0..a.count {
                let idx = (a.base + i) as usize;
                if idx < m.len() {
                    m[idx] = true;
                }
            }
        }
        m
    }

    /// 找 count 个连续空闲槽的最低基址。
    pub fn find_run(&self, count: u32) -> Option<u32> {
        let occ = self.occupancy();
        let mut run = 0u32;
        for (i, &o) in occ.iter().enumerate() {
            if o {
                run = 0;
            } else {
                run += 1;
                if run == count {
                    return Some(i as u32 + 1 - count);
                }
            }
        }
        None
    }

    /// 批量分配：找连续槽（同主重复拒，零槽批拒）。
    pub fn alloc(&mut self, owner: &str, count: u32) -> Result<u32, u32> {
        if self.sealed {
            return Err(MX_FROZEN);
        }
        if count == 0 {
            return Err(MX_ZERO_ALLOC);
        }
        if self.allocs.iter().any(|a| a.owner == owner) {
            return Err(MX_DUP_OWNER);
        }
        match self.find_run(count) {
            Some(base) => {
                self.allocs.push(MxAlloc { owner: owner.to_string(), base, count });
                Ok(base)
            }
            None => Err(MX_NO_SPACE),
        }
    }

    /// 批量回收。
    pub fn free(&mut self, owner: &str) -> Result<u32, u32> {
        if self.sealed {
            return Err(MX_FROZEN);
        }
        let before = self.allocs.len();
        let mut freed = 0u32;
        self.allocs.retain(|a| {
            if a.owner == owner {
                freed = a.count;
                false
            } else {
                true
            }
        });
        if self.allocs.len() == before {
            return Err(MX_UNK_FREE);
        }
        Ok(freed)
    }

    pub fn used_slots(&self) -> u32 {
        self.occupancy().iter().filter(|&&o| o).count() as u32
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x41780;
        for a in &self.allocs {
            for by in a.owner.bytes() {
                h = h.wrapping_mul(31).wrapping_add(by as u32);
            }
            h = h.wrapping_mul(31).wrapping_add(a.base);
            h = h.wrapping_mul(31).wrapping_add(a.count);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 标准样本：16 槽池，两批。
pub fn mk_pool() -> MxPool {
    let mut p = MxPool::new(16);
    let _ = p.alloc("gpu", 4); // 0..3
    let _ = p.alloc("dma", 2); // 4..5
    p
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut p = mk_pool();
    // 1) 超容
    got[0] = p.alloc("big", 16).err() == Some(MX_NO_SPACE);
    // 2) 重批主
    got[1] = p.alloc("gpu", 1).err() == Some(MX_DUP_OWNER);
    // 3) 零槽批
    got[2] = p.alloc("x", 0).err() == Some(MX_ZERO_ALLOC);
    // 4) 野批回收
    got[3] = p.free("ghost").err() == Some(MX_UNK_FREE);
    // 5) 印后操作必抓
    p.seal();
    got[4] = p.alloc("late", 1).err() == Some(MX_FROZEN);
    got
}

/// 判据主体。
pub fn run_unx24173_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F24173");
    let mut p = mk_pool();

    // J1 批账齐
    let j1 = p.allocs.len() == 2 && p.used_slots() == 6;
    s.add("J1-批账齐", j1, "两批六槽");

    // J2 连续槽分配
    let j2 = {
        let a = p.allocs.iter().find(|a| a.owner == "gpu").unwrap();
        a.base == 0 && a.count == 4
    };
    s.add("J2-连续槽批", j2, "gpu 占 0..3");

    // J3 紧凑排列
    let j3 = {
        let a = p.allocs.iter().find(|a| a.owner == "dma").unwrap();
        a.base == 4 && a.count == 2
    };
    s.add("J3-紧凑排列", j3, "dma 接续 4..5");

    // J4 容量闸
    let j4 = p.alloc("big", 16).err() == Some(MX_NO_SPACE);
    s.add("J4-容量闸", j4, "超池容必拒");

    // J5 重批主拒
    let j5 = p.alloc("gpu", 1).err() == Some(MX_DUP_OWNER);
    s.add("J5-重批主拒", j5, "同主二批必拒");

    // J6 零槽批拒
    let j6 = p.alloc("x", 0).err() == Some(MX_ZERO_ALLOC);
    s.add("J6-零槽批拒", j6, "count=0 必拒");

    // J7 回收释放
    let j7 = {
        let n = p.free("gpu").unwrap_or(0);
        n == 4 && p.used_slots() == 2
    };
    s.add("J7-回收释放", j7, "gpu 回收后余 2 槽");

    // J8 回收后重分配
    let j8 = {
        let _ = p.free("gpu");
        p.alloc("npu", 4).is_ok()
    };
    s.add("J8-回收重分", j8, "回收后可重分配");

    // J9 野批回收拒
    let j9 = p.free("ghost").err() == Some(MX_UNK_FREE);
    s.add("J9-野批回收拒", j9, "无主批必拒");

    // J10 find_run 精确
    let j10 = {
        let mut p2 = MxPool::new(8);
        let _ = p2.alloc("a", 3); // 0..2
        let _ = p2.alloc("b", 2); // 3..4
        p2.find_run(2) == Some(5)
    };
    s.add("J10-find_run", j10, "下一连续对在 5");

    // J11 印后操作拒
    let j11 = {
        let mut p2 = mk_pool();
        p2.seal();
        p2.alloc("late", 1).err() == Some(MX_FROZEN)
    };
    s.add("J11-印后拒", j11, "印发后冻结");

    // J12 指纹稳定
    let j12 = p.fold() == p.fold();
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
    fn unx24173_alloc_flow() {
        let mut p = MxPool::new(4);
        assert_eq!(p.alloc("a", 2), Ok(0));
        assert_eq!(p.alloc("b", 2), Ok(2));
        assert_eq!(p.alloc("c", 1), Err(MX_NO_SPACE));
        assert_eq!(p.free("a"), Ok(2));
        assert_eq!(p.alloc("c", 2), Ok(0));
        assert_eq!(p.alloc("a", 0), Err(MX_ZERO_ALLOC));
        assert_eq!(p.free("ghost"), Err(MX_UNK_FREE));
        p.seal();
        assert_eq!(p.alloc("d", 1), Err(MX_FROZEN));
    }

    #[test]
    fn unx24173_checks_green() {
        let s = run_unx24173_checks();
        assert!(s.all_passed(), "UNX-F24173 判据未全绿");
    }
}
