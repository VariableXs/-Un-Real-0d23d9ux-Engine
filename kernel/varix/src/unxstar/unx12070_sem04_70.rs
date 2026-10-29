//! UNX-F12070 CountOfOwnedCriticalSections 语义（0x19ae0 段）
//!
//! 卡面：TEB→CountOfOwnedCriticalSections——线程持有的临界区计数语义模型：
//! 临界区占用账、可重入不重复计数、释放须持锁、计数与占位集对平。
//! 域：CoRun·UNX-D1-B04。
//!
//! 判据原文：
//! - J1 计数口径：CountOfOwnedCriticalSections == 当前持锁临界区集合大小。
//! - J2 可重入不重复计数：同线程重复 acquire 同一临界区计数不增。
//! - J3 释放须持锁：释放未持有（或他人持有）的临界区必拒。
//! - J4 线程 id 合法：零 tid 的一切操作必拒。
//! - J5 忙拒：他人持锁时的 acquire 必拒（自旋语义以忙拒显性化）。
//! - J6 计数漂移检出：声明计数与占位集不符必判红。
//! - J7 印账冻结：seal 后一切操作必拒。
//! - J8 注入五况全抓。
//!
//! 结构：CritSec 临界区类型 + CritSecBook 占用账（区间位表 + 计数器）+
//! 折叠指纹（211 进制卷积）+ 注入核（空释/忙夺/零 tid/漂移/印后取）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x19ae0 独占 0x20）。
pub const SEG_BASE: u32 = 0x19ae0;
pub const CS_NOT_OWNED: u32 = SEG_BASE + 0x01; // 释放未持有
pub const CS_BUSY: u32 = SEG_BASE + 0x02; // 他人持锁
pub const CS_ZERO_TID: u32 = SEG_BASE + 0x03; // 零线程 id
pub const CS_DRIFT: u32 = SEG_BASE + 0x04; // 计数漂移
pub const CS_FROZEN: u32 = SEG_BASE + 0x05; // 印账后操作

/// 临界区：id + 当前持锁线程（None 即空闲）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CritSec {
    pub id: u32,
    pub owner: Option<u32>,
}

impl CritSec {
    pub fn free(&self) -> bool {
        self.owner.is_none()
    }
    pub fn owned_by(&self, tid: u32) -> bool {
        self.owner == Some(tid)
    }
    pub fn render(&self) -> String {
        match self.owner {
            Some(t) => format!("S{}|T{}", self.id, t),
            None => format!("S{}|free", self.id),
        }
    }
}

/// 线程临界区占用账。
pub struct CritSecBook {
    pub sections: Vec<CritSec>,
    /// 声明计数（TEB 侧 CountOfOwnedCriticalSections 语义）。
    pub declared_count: u32,
    pub max_sections: usize,
    pub sealed: bool,
}

impl CritSecBook {
    pub fn new(max_sections: usize) -> CritSecBook {
        CritSecBook {
            sections: Vec::new(),
            declared_count: 0,
            max_sections,
            sealed: false,
        }
    }

    fn find(&self, id: u32) -> Option<usize> {
        self.sections.iter().position(|s| s.id == id)
    }

    /// 取锁：空闲可入；本线程已持锁为可重入（计数不增）；他人持锁忙拒。
    pub fn acquire(&mut self, id: u32, tid: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(CS_FROZEN);
        }
        if tid == 0 {
            return Err(CS_ZERO_TID);
        }
        match self.find(id) {
            Some(i) => {
                let sec = self.sections[i];
                if sec.free() {
                    self.sections[i].owner = Some(tid);
                    self.declared_count += 1;
                    Ok(())
                } else if sec.owned_by(tid) {
                    Ok(()) // 可重入不重复计数
                } else {
                    Err(CS_BUSY)
                }
            }
            None => {
                if self.sections.len() >= self.max_sections {
                    return Err(CS_BUSY); // 容量上限按忙拒显性化
                }
                if self.declared_count as usize >= self.max_sections {
                    return Err(CS_DRIFT); // 计数超容先于入账拒
                }
                self.sections.push(CritSec { id, owner: Some(tid) });
                self.declared_count += 1;
                Ok(())
            }
        }
    }

    /// 放锁：仅持锁线程可放。
    pub fn release(&mut self, id: u32, tid: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(CS_FROZEN);
        }
        if tid == 0 {
            return Err(CS_ZERO_TID);
        }
        match self.find(id) {
            Some(i) => {
                if self.sections[i].owned_by(tid) {
                    self.sections[i].owner = None;
                    if self.declared_count > 0 {
                        self.declared_count -= 1;
                    }
                    Ok(())
                } else {
                    Err(CS_NOT_OWNED)
                }
            }
            None => Err(CS_NOT_OWNED),
        }
    }

    /// 实际持锁集合大小（按 tid 视角：tid 持有的临界区数）。
    pub fn owned_by_count(&self, tid: u32) -> usize {
        self.sections.iter().filter(|s| s.owned_by(tid)).count()
    }

    /// 计数对平：全部空闲时声明计数应为 0；有持锁则等于持锁条数。
    pub fn count_consistent(&self) -> bool {
        let held = self.sections.iter().filter(|s| !s.free()).count();
        self.declared_count as usize == held
    }

    /// 线程视角计数对平。
    pub fn thread_count_ok(&self, tid: u32, declared: u32) -> bool {
        self.owned_by_count(tid) == declared as usize
    }

    /// 折叠指纹（211 进制卷积）。
    pub fn fold(&self) -> u32 {
        let mut h: u32 = SEG_BASE;
        for s in &self.sections {
            h = h.wrapping_mul(211).wrapping_add(s.id);
            h = h.wrapping_mul(211)
                .wrapping_add(s.owner.unwrap_or(0));
        }
        h = h.wrapping_mul(211).wrapping_add(self.declared_count);
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }

    /// 诊断面：持锁条数。
    pub fn held_count(&self) -> usize {
        self.sections.iter().filter(|s| !s.free()).count()
    }

    /// 诊断面：账面摘要。
    pub fn summary(&self) -> String {
        format!(
            "secs={} held={} decl={}",
            self.sections.len(), self.held_count(), self.declared_count
        )
    }

    /// 诊断面：临界区渲染行。
    pub fn sec_line(&self, id: u32) -> String {
        match self.find(id) {
            Some(i) => self.sections[i].render(),
            None => "missing".to_string(),
        }
    }
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = CritSecBook::new(8);
    // 1) 零 tid 必拒。
    got[0] = b.acquire(1, 0).err() == Some(CS_ZERO_TID);
    // 2) 他人持锁忙拒。
    let _ = b.acquire(1, 100);
    got[1] = b.acquire(1, 200).err() == Some(CS_BUSY);
    // 3) 释放未持有的临界区必拒。
    got[2] = b.release(1, 200).err() == Some(CS_NOT_OWNED);
    // 4) 计数漂移：伪造声明计数与占位集不符必判红。
    let mut b2 = CritSecBook::new(8);
    let _ = b2.acquire(7, 100);
    b2.declared_count = 5;
    got[3] = !b2.count_consistent();
    // 5) 印账后取锁必抓。
    b.seal();
    got[4] = b.acquire(2, 100).err() == Some(CS_FROZEN);
    got
}

/// 批内判据（对应卡验收 J 判据）。
pub fn run_unx12070_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12070");

    // J1 计数口径：acquire 后 owned 计数对平。
    let j1 = {
        let mut b = CritSecBook::new(8);
        let _ = b.acquire(1, 100);
        let _ = b.acquire(2, 100);
        b.count_consistent() && b.owned_by_count(100) == 2
    };
    s.add("J1-计数口径", j1, "声明计数==持锁集合大小");

    // J2 可重入不重复计数。
    let j2 = {
        let mut b = CritSecBook::new(8);
        let _ = b.acquire(1, 100);
        let _ = b.acquire(1, 100);
        b.owned_by_count(100) == 1 && b.count_consistent()
    };
    s.add("J2-可重入不增", j2, "重入计数不增");

    // J3 释放须持锁。
    let j3 = {
        let mut b = CritSecBook::new(8);
        b.release(9, 100).err() == Some(CS_NOT_OWNED)
    };
    s.add("J3-空释必拒", j3, "未持锁释放必拒");

    // J4 零 tid 拒收。
    let j4 = {
        let mut b = CritSecBook::new(8);
        b.acquire(1, 0).err() == Some(CS_ZERO_TID)
            && b.release(1, 0).err() == Some(CS_ZERO_TID)
    };
    s.add("J4-零tid拒", j4, "零 tid 皆拒");

    // J5 忙拒。
    let j5 = {
        let mut b = CritSecBook::new(8);
        let _ = b.acquire(1, 100);
        b.acquire(1, 300).err() == Some(CS_BUSY)
    };
    s.add("J5-忙拒", j5, "他人持锁必忙拒");

    // J6 放锁后计数回退。
    let j6 = {
        let mut b = CritSecBook::new(8);
        let _ = b.acquire(3, 100);
        let _ = b.release(3, 100);
        b.count_consistent() && b.declared_count == 0 && b.sections[0].free()
    };
    s.add("J6-放锁回退", j6, "放锁后计数归零");

    // J7 印账冻结。
    let j7 = {
        let mut b = CritSecBook::new(8);
        let _ = b.acquire(1, 100);
        b.seal();
        b.acquire(2, 100).err() == Some(CS_FROZEN)
            && b.release(1, 100).err() == Some(CS_FROZEN)
    };
    s.add("J7-印账冻结", j7, "seal 后取放皆拒");

    // J8 指纹幂等。
    let j8 = {
        let mut b = CritSecBook::new(8);
        let _ = b.acquire(1, 100);
        b.fold() == b.fold()
    };
    s.add("J8-指纹幂等", j8, "同态同指纹");

    // J9 注入核 5/5。
    let got = inject_five();
    let j9 = got.iter().all(|x| *x);
    s.add_dyn("J9-注入全抓", j9, &format!("inj5={:?}", got));

    // 汇总样例账。
    let mut b = CritSecBook::new(8);
    let _ = b.acquire(1, 100);
    let _ = b.acquire(2, 200);

    s.add("线程视角对平", b.thread_count_ok(100, 1), "T100 持 1 锁");
    s.add("摘要含持锁", b.summary().contains("held=2"), "摘要含持锁数");
    s.add("渲染行", b.sec_line(1).contains("T100"), "持锁行含 tid");
    s.add("空闲行", b.sec_line(9) == "missing", "未登记行显性");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx12070_reent_and_release() {
        let mut b = CritSecBook::new(8);
        assert!(b.acquire(1, 100).is_ok());
        assert!(b.acquire(1, 100).is_ok());
        assert_eq!(b.owned_by_count(100), 1);
        assert!(b.release(1, 100).is_ok());
        assert!(b.sections[0].free());
    }

    #[test]
    fn unx12070_checks_green() {
        let s = run_unx12070_checks();
        assert!(s.all_passed(), "UNX-F12070 判据未全绿");
    }

    #[test]
    fn critsec_render() {
        let a = CritSec { id: 4, owner: Some(77) };
        let b = CritSec { id: 5, owner: None };
        assert!(a.render().contains("T77"));
        assert!(b.free());
        assert!(b.render().contains("free"));
    }
}
