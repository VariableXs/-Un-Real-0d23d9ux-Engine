//! UNX-F24153 reservation 锁与显存互斥纪律·子功能13（锁泄漏检测·漏 release 账）（0x41500 段）
//!
//! 锚点：UNX-F24153-J1 泄漏账（acquire/release 差额逐条落账）；J2 泄
//! 漏语义（差额判泄漏 + 全释清零 + 逃逸豁免三性质）；J3 泄漏指纹（账
//! 折叠指纹）三判据全绿。
//! 域：CoRun·UNX-F5-B15（锁泄漏检测面）。
//!
//! 结构：持有差额表 + 泄漏判定核 + 逃逸豁免核 + 折叠指纹 + 注入核（
//! 野锁/超额释放/豁免后再判/印后/重复豁免必抓）。

use crate::alloc::string::{String, ToString};
use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x41500 独占 0x20）。
pub const SEG_BASE: u32 = 0x41500;
pub const LK2_UNK: u32 = SEG_BASE + 0x01; // 野锁
pub const LK2_OVER_REL: u32 = SEG_BASE + 0x02; // 超额释放
pub const LK2_DUP_EXEMPT: u32 = SEG_BASE + 0x03; // 重复豁免
pub const LK2_FROZEN: u32 = SEG_BASE + 0x04; // 印后操作
pub const LK2_LEAK: u32 = SEG_BASE + 0x05; // 泄漏仍关账

/// 持有差额条目。
#[derive(Clone)]
pub struct LeakEnt {
    pub key: String, // "holder:lock"
    pub acq: u32,
    pub rel: u32,
    pub exempt: bool, // 逃逸豁免（已知故意遗留）
}

/// 泄漏簿。
pub struct LeakBook {
    pub ents: Vec<LeakEnt>,
    pub sealed: bool,
}

impl LeakBook {
    pub fn new() -> LeakBook {
        LeakBook { ents: Vec::new(), sealed: false }
    }

    fn ent_of(&self, key: &str) -> Option<usize> {
        self.ents.iter().position(|e| e.key == key)
    }

    pub fn acquire(&mut self, holder: &str, lock_id: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(LK2_FROZEN);
        }
        if holder.is_empty() {
            return Err(LK2_UNK);
        }
        let key = format!("{}:{}", holder, lock_id);
        match self.ent_of(&key) {
            Some(i) => self.ents[i].acq += 1,
            None => self.ents.push(LeakEnt { key, acq: 1, rel: 0, exempt: false }),
        }
        Ok(())
    }

    pub fn release(&mut self, holder: &str, lock_id: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(LK2_FROZEN);
        }
        let key = format!("{}:{}", holder, lock_id);
        match self.ent_of(&key) {
            Some(i) => {
                if self.ents[i].rel >= self.ents[i].acq {
                    return Err(LK2_OVER_REL);
                }
                self.ents[i].rel += 1;
                Ok(())
            }
            None => Err(LK2_OVER_REL),
        }
    }

    /// 泄漏判定：acq > rel 且未豁免。
    pub fn leaked(&self) -> Vec<String> {
        let mut v = Vec::new();
        for e in &self.ents {
            if e.acq > e.rel && !e.exempt {
                v.push(e.key.clone());
            }
        }
        v
    }

    /// 逃逸豁免：标记已知故意遗留，重复豁免拒。
    pub fn exempt(&mut self, holder: &str, lock_id: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(LK2_FROZEN);
        }
        let key = format!("{}:{}", holder, lock_id);
        match self.ent_of(&key) {
            Some(i) => {
                if self.ents[i].exempt {
                    return Err(LK2_DUP_EXEMPT);
                }
                self.ents[i].exempt = true;
                Ok(())
            }
            None => Err(LK2_UNK),
        }
    }

    /// 全释清零：全部平衡即无泄漏。
    pub fn all_balanced(&self) -> bool {
        self.ents.iter().all(|e| e.acq == e.rel || e.exempt)
    }

    pub fn fold(&self) -> u32 {
        let mut h: u32 = 0x41500;
        for e in &self.ents {
            for by in e.key.bytes() {
                h = h.wrapping_mul(31).wrapping_add(by as u32);
            }
            h = h.wrapping_mul(31).wrapping_add(e.acq);
            h = h.wrapping_mul(31).wrapping_add(e.rel);
            h = h.wrapping_mul(31).wrapping_add(e.exempt as u32);
        }
        h
    }

    pub fn seal(&mut self) -> u32 {
        self.sealed = true;
        self.fold()
    }
}

/// 标准样本：一对平衡、一对泄漏、一豁免。
pub fn mk_book() -> LeakBook {
    let mut b = LeakBook::new();
    let _ = b.acquire("a", 1);
    let _ = b.release("a", 1); // 平衡
    let _ = b.acquire("b", 2); // 泄漏（未 release）
    let _ = b.acquire("c", 3);
    let _ = b.exempt("c", 3); // 豁免
    b
}

/// 注入核：五个必然被检出的坏样本。
pub fn inject_five() -> [bool; 5] {
    let mut got = [false; 5];
    let mut b = mk_book();
    // 1) 超额释放
    got[0] = b.release("a", 1).err() == Some(LK2_OVER_REL);
    // 2) 野键释放
    got[1] = b.release("ghost", 9).err() == Some(LK2_OVER_REL);
    // 3) 重复豁免
    got[2] = b.exempt("c", 3).err() == Some(LK2_DUP_EXEMPT);
    // 4) 泄漏名单识破 b
    got[3] = b.leaked() == vec_str2(&["b:2"]);
    // 5) 印后操作必抓
    b.seal();
    got[4] = b.acquire("x", 1).err() == Some(LK2_FROZEN);
    got
}

/// 小工具。
fn vec_str2(xs: &[&str]) -> Vec<String> {
    let mut v = Vec::new();
    for x in xs {
        v.push(x.to_string());
    }
    v
}

/// 判据主体。
pub fn run_unx24153_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F24153");
    let mut b = mk_book();

    // J1 差额账齐
    let j1 = b.ents.len() == 3;
    s.add("J1-差额账齐", j1, "三键全落表");

    // J2 平衡键不泄漏
    let j2 = !b.leaked().iter().any(|k| k == "a:1");
    s.add("J2-平衡不漏", j2, "a:1 acq==rel");

    // J3 泄漏键识破
    let j3 = b.leaked() == vec_str2(&["b:2"]);
    s.add("J3-泄漏识破", j3, "b:2 未释入名单");

    // J4 豁免键不泄漏
    let j4 = !b.leaked().iter().any(|k| k == "c:3");
    s.add("J4-豁免不漏", j4, "c:3 豁免出名单");

    // J5 全释清零判定
    let j5 = {
        let mut b2 = LeakBook::new();
        let _ = b2.acquire("x", 1);
        let _ = b2.release("x", 1);
        b2.all_balanced()
    };
    s.add("J5-全释清零", j5, "平衡即全清");

    // J6 豁免也计入平衡
    let j6 = b.all_balanced();
    s.add("J6-豁免计平衡", j6, "豁免键不破全平衡");

    // J7 超额释放拒
    let j7 = b.release("a", 1).err() == Some(LK2_OVER_REL);
    s.add("J7-超额释放拒", j7, "rel>acq 必拒");

    // J8 重复豁免拒
    let j8 = b.exempt("c", 3).err() == Some(LK2_DUP_EXEMPT);
    s.add("J8-重复豁免拒", j8, "二次豁免必拒");

    // J9 补释出名单
    let j9 = {
        let mut b2 = mk_book();
        let _ = b2.release("b", 2);
        b2.leaked().is_empty()
    };
    s.add("J9-补释出名单", j9, "补 release 后无泄漏");

    // J10 野键释放拒
    let j10 = b.release("ghost", 9).err() == Some(LK2_OVER_REL);
    s.add("J10-野键释放拒", j10, "未 acq 键必拒");

    // J11 空持有者拒
    let j11 = b.acquire("", 1).err() == Some(LK2_UNK);
    s.add("J11-空持有者拒", j11, "空名 acq 必拒");

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
    fn unx24153_leak_flow() {
        let mut b = LeakBook::new();
        let _ = b.acquire("a", 1);
        assert!(b.leaked().len() == 1);
        assert!(b.release("a", 1).is_ok());
        assert!(b.leaked().is_empty());
        assert_eq!(b.release("a", 1), Err(LK2_OVER_REL));
        let _ = b.acquire("b", 2);
        assert!(b.exempt("b", 2).is_ok());
        assert_eq!(b.exempt("b", 2), Err(LK2_DUP_EXEMPT));
        assert!(b.all_balanced());
        b.seal();
        assert_eq!(b.acquire("c", 1), Err(LK2_FROZEN));
    }

    #[test]
    fn unx24153_checks_green() {
        let s = run_unx24153_checks();
        assert!(s.all_passed(), "UNX-F24153 判据未全绿");
    }
}
